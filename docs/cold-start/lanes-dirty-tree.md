# COLD START — branch `lanes-dirty-tree`

### COLD START — branch `lanes-dirty-tree`, 2026-08-16

`ci/lanes.sh` merged and was put into service at 19:31. Its first drain dropped
**all 37 queued branches in 33 seconds and exited 0.** Every branch was logged

```
lane N — SKIP <branch>: REBASE CONFLICT onto 5f7332f9, needs hands
```

while `lanes/lanes.log` recorded the real cause under each one:

```
error: cannot rebase: You have unstaged changes.
error: Please commit or stash them.
```

**The lane worktree was dirty and `ci/lanes.sh` is what dirtied it.**
`git -C lanes/lane-1 status --porcelain` read `M .cargo/config.toml` — confirmed
still true on both lanes when this branch started. `lane_setup` wrote
`[build] jobs = N` into the lane's `.cargo/config.toml` before the rsync, so the
per-lane cargo cap would travel with the tree (an exported `CARGO_BUILD_JOBS`
does not survive the ssh, which is true and was the right diagnosis of the wrong
problem). **That file is tracked.** Writing it left every lane permanently
modified, and git refuses to rebase a dirty tree, so the cap and the rebase were
fighting and the rebase lost on every branch, forever. Twenty-six of the 37 were
fine.

Two further faults in the same write, neither of which had had a chance to show:

* it **overwrote** `.cargo/config.toml` rather than adding to it, and that file
  is where `rustflags = ["-C", "target-cpu=x86-64-v2"]` is set. Every lane gate
  would have measured a worldgen build without the `roundsd` that setting exists
  for — the file's own header puts `floor` at ~10% of chunk-generation time.
* `ci/handoff.sh` refuses a dirty working tree outright (`REFUSED  working tree
  dirty — an agent may still be live`). Had the rebase somehow succeeded, every
  lane gate would have been refused one line later for the same reason.

## What was fixed

**1. The cap no longer touches the tree.** `ci/handoff.sh`'s `remote()` and
`remote_script()` splice a validated `export CARGO_BUILD_JOBS=N;` onto the ssh
command string, following `refcache-live`'s handling of
`RUSTMC_REFERENCE_CACHE` and the `RUSTMC_CORPUS_LOCK_ROLE=gate` already on
stage 6. `ci/lanes.sh` already set the variable in `handoff.sh`'s environment, so
nothing there changed. It is spliced as a standalone `export` statement, not as
a `VAR=x cmd` prefix, because `remote()` is handed compound commands and a
prefix binds to the first simple command only. A value that is not a plain
positive integer is a refusal, not an assignment — it is interpolated into a
remote shell.

**The generalised rule, written into both headers: a resource cap must not be
carried in a tracked file.** If a future cap genuinely cannot be an environment
variable, it writes an *untracked* file and proves
`git status --porcelain --untracked-files=no` stays empty.

**2. A dirty tree is named as infrastructure and stops the lane.** Two guards,
at two moments, and they are not redundant:

* `lane_setup` checks once per lane before any worker starts, and refuses the
  whole drain (`exit 4`) naming the paths and printing the repair. **It does not
  `reset --hard`.** Silently repairing the tree would hide whatever dirtied it,
  which is the same shape of mistake as writing the file in the first place.
* `_admit` checks once per admission, *before* the branch is taken off the
  queue, so dirt appearing mid-drain is caught on the first branch after it
  appears rather than shredding every branch behind it. The lane stops; the
  queue is left intact; nothing is written to `train.tsv`.

The rebase failure path also classifies before it names: `rebase --abort`
restores a tree a real conflict left mid-rebase, but it cannot clean a tree that
was dirty before the rebase declined to start, so dirt surviving to that point
means the rebase never ran and the branch is requeued as UNJUDGED.

The predicate is `git status --porcelain --untracked-files=no`. Tracked
modifications are what git refuses a rebase on; untracked files do not block it
and a lane clone legitimately accumulates them. Widening it to plain
`--porcelain` would refuse drains over build litter, which is how a correct
check gets switched off.

**3. The exit status now answers "was this queue judged?"** It previously
answered "did the loop finish", which it always does.

```
0  at least one branch reached a gate. A drain that judged its queue and
   merged nothing is a legitimate 0 — RED and dropped are verdicts.
2  bad usage, or a lane that could not be prepared at all.
3  branches were queued and NOT ONE reached a gate. A destroyed queue.
4  a lane stopped on a tree no rebase could run in. The specific, actionable
   case of 3, checked first because it names the repair. Matches
   ci/handoff.sh's INFRA exit: neither is a verdict about a branch.
```

`$STATE/gated` is the witness, appended just before the gate command runs.
`train.tsv` cannot answer this: a branch dropped at admission and a branch
dropped by a RED gate are both `dropped`, so the summary counts cannot separate
a judged queue from a destroyed one. That is exactly why the 19:31 drain's
`dropped 37` and its exit 0 both looked survivable.

## The falsifier's hole, and what now fills it

`ci/lanes-falsify.sh` **passed while this defect was live**, so it had a hole
exactly the size of the bug. Two reasons, both now closed: every case ran its
drain in lane clones the file had just created and which were therefore never
dirty, and **no case ever looked at what the drain returned.**

`ci/lanes-falsify.sh` is now **8 cases, 8 injections — 16 of 16 green, 0
uncaught** (was 6 and 4). The two new cases:

* **`dirty_tree_is_infra`** — makes a lane dirty out of real machinery and
  asserts the three things that were wrong: it is named INFRA and not `REBASE
  CONFLICT`, the lane stops rather than manufacturing verdicts, and the branches
  it never reached stay queued rather than being dropped. Two arms: arm A
  dirties a tracked file mid-gate through the stub's hook so the *next* branch's
  rebase is the one that cannot run (`_admit`'s guard); arm B reuses that dirty
  lane in a second drain and requires a refusal before the queue is touched at
  all (`lane_setup`'s guard).
* **`no_gate_reached`** — three branches that do not exist on origin, so every
  one is legitimately skipped, and the drain must exit non-zero. `case_drain`
  holds the other side, that a drain which gated its queue exits 0; without it
  the case is satisfiable by a script that always fails.

Four new injections, each of which turns one of those red:
`dirty_tree_misreported` (blind the predicate — reproduces the 19:31 log
verbatim, `SKIP br2: REBASE CONFLICT onto …, needs hands`),
`lane_dirties_tree` (re-add a tracked-file write to `lane_setup` — the cause),
`silent_zero_exit`, `always_nonzero_exit`.

`LANES_FALSIFY_DRYRUN=1 ./ci/lanes-falsify.sh` applies all 8 patches in ~2 s;
the full sweep is ~2.5 min.

## Found on the way: `ci/handoff-verdict-selftest.sh` could not pass

Not this branch's item, fixed here because this branch's new evidence lives in
it. **On unmodified `origin/main` it reported 0 passed, 17 failed** — a file
ending in `[ "$FAIL" -eq 0 ] || exit 1` that could not reach zero. Two layers,
both self-inflicted by its own fixture:

1. every scenario died on `REFUSED  ci/gate-lock.sh is missing — refusing to
   gate unlocked`. `ci/handoff.sh` grew that guard, correctly, but `build_repo`
   only ever copied `handoff.sh` into the throwaway repo. The real library is
   now copied too (not stubbed — a stub would let the refusal pass against a
   lock that does not exist), pointed at a private `RUSTMC_GATE_LOCK_DIR`.
2. underneath that, every scenario then died on `REFUSED  working tree dirty`.
   `HOME` was `$R/home`, *inside* the fixture checkout, and `ci/handoff.sh`'s
   first action is `mkdir -p $HOME/rustmc-private/gates` — so the gate dirtied
   its own tree and then refused it. `HOME` is now `$WORK/home`.

**Now 22 passed, 0 failed.** Four of those are new and are the only evidence
the job-cap splice works, since `ci/lanes-falsify.sh` stands in for the whole
gate and never builds a real remote command: the fake ssh records every command
string, and the cap was on **5 of 5** of them. A non-integer and a zero are both
refused before the gate starts.

## Killed as a null — do not re-run

* **Do not "fix" this by widening the rebase.** `rebase --autostash`, `git
  stash`, or a `reset --hard` in `_admit` all make the symptom go away and all
  keep a resource cap in a tracked file, where the next reader will find a lane
  whose committed `.cargo/config.toml` disagrees with main and will not know why.
* **Do not weaken the cap.** 24 threads across 2 lanes with cargo defaulting to
  one job per hardware thread will thrash; the reasoning in the header is sound
  and is untouched. What was wrong was the carrier, not the cap.
* **`.cargo/config.toml` is not a stray file.** It carries
  `target-cpu=x86-64-v2` for `Mth.floor`. Anything that writes it wholesale
  silently removes that.

## Before lanes are re-enabled

**`lanes/lane-1` and `lanes/lane-2` are still dirty on disk** — this branch does
not touch them, and with these changes a drain will now refuse (exit 4) rather
than shred the queue. Clear them first:

```bash
git -C /home/sayer/grit/lanes/lane-1 checkout -- .
git -C /home/sayer/grit/lanes/lane-2 checkout -- .
```

## The single next item

**`ci/lanes-falsify.sh` is wired into no gate — `grep -n lanes ci/t0.sh` is
empty.** Both of the new cases ran only because this branch ran them, which is
the same standing exposure that let the original hole ship. The full sweep at
~2.5 min is probably too slow for T0, but `LANES_FALSIFY_DRYRUN=1` is ~2 s and
proves every patch still matches `ci/lanes.sh` — the exact check
`docs/AGENT-BRIEF.md` already prescribes for `ci/corpus-lock-falsify.sh`. It
closes when T0 runs the dry run and a deliberate edit to `ci/lanes.sh` that
unhooks a patch turns T0 red. Deliberately not done here: T0 is what every gate
runs, the keeper is mid-chain, and that is the orchestrator's call to make.
