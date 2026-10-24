# AGENT-BRIEF — the standing rules every rustmc agent works under

**If a brief points you here, read this file once, first, and do not re-read
it.** It holds everything that does not change between tasks. Your brief holds
only what does: the item, the measured baseline, the closing criterion, and the
nulls already paid for.

This file exists to be read from the tree instead of pasted into a prompt. A
pasted block is re-sent on every tool call; a file is read once. Refined across
~20 orchestrated turns — nearly every line below exists because its absence cost
one.

---

## HARD RULE — a check ships with the evidence it can go red

**This project's central defect is an instrument that measures its own input,
or a verdict whose green cannot be turned red.** It has been rediscovered one
instance at a time, at the price of an agent and a decision each: a readiness
proof grepping for `listening on`, which this server prints *before* it binds
and keeps running after `Address already in use`; `0 of 539 stacks opaque`,
guaranteed because the step feeding the decoder wrote a literal `0`; a
`window` counter coupled to zero checks; `ACCEPT_TELEPORTATION` decoded into a
field read by nothing, behind a doc comment describing a gate no line
implemented; 112 sound events written off against a figure measuring something
else. **Every parity number this project publishes rests on its checks not
being of this kind.**

Two T0 gates mechanise it and they are not separable:

* **`ci/corpus-falsifiable.py`** mutates the *subject* side of a recorded
  trace pair and refuses a gameplay scenario whose outcome no mutation of its
  own measured step can move. Evidence lives in
  `ci/checks/gameplay-falsifiable.toml`; `--check` reads it and needs no
  recordings, so it runs inside `ci/t0.sh`.
* **`ci/liveness-proofs.py`** discovers every wait loop in `ci/*.sh` and
  refuses a readiness proof not registered in `ci/checks/liveness-proofs.toml`
  with a log a **not-ready** subject produces — which it then feeds to the
  real predicate through `bash`.

**So, concretely, when you add or change a check:**

1. **A new `ci/checks/gameplay/*.toml` is UNREGISTERED until you sweep.** Run
   `ci/corpus-falsifiable.py --sweep --bin target/release/oracle-runner \
   oracle-work/gameplay/vanilla.json oracle-work/gameplay/rustmc.json` and
   commit the ledger it writes. It needs no server, no vanilla boot and **not
   the corpus lock** — it re-compares recordings that already exist, in about
   ninety seconds. Editing a check's steps or `observe` list goes STALE for
   the same reason and takes the same re-sweep; editing its `about` does not.
2. **Your check must reach tier 2** — an edit of the step it is *named for*
   must move its outcome. Tier 1, where only deleting the scenario's whole
   output moves it, is refused: that is a check scoring a claim it never
   reads.
3. **A new wait loop must reject a log.** If you cannot write a log your
   predicate calls not-ready, you have not written a proof, and no amount of
   comment will make it one.
4. **Both gates are ratcheted.** The tier-2 floor, the dead-field ceiling and
   the unsound-proof allowance may not rise silently, and the last two are
   bounded by constants in the scripts so widening them is a reviewed change
   rather than a data edit.

The guards' own red-when-broken evidence is `ci/unfalsifiable-falsify.py`,
which reconstructs three shipped defects and asserts each is refused and each
repair accepted. **Hold anything you build to that bar: a guard that cannot be
shown catching a real past instance is itself a check that cannot fail.**
`docs/PARITY-ROADMAP.md` §27.

## HARD RULE — never wait in the foreground. No exceptions.

**A foreground wait loop is the most expensive thing an agent can do, and it
does no work.** A `Bash` call containing `until … sleep` is killed at its 600s
timeout and handed back to you, and your next turn re-caches your *entire
context* as `cache_creation`, not `cache_read`. Measured 2026-08-09: two agents
waiting on gate logs re-wrote **597k and 406k tokens every ten minutes — about
7.5M token-writes an hour, for nothing.** That was substantially the whole of a
2.41 %/h burn against a 0.54 %/h sustainable rate. A long gate is not a reason
to break this. A long gate is the reason for it.

**An agent cannot wake itself. Only a message does** — a tool result, or the
orchestrator. Ending a turn to "wait" is not waiting, it is stopping, and
whatever reaches you next pays a **full context re-read**, not a cache hit. One
lane woke at 615k tokens to say "still queued"; another was woken eight times at
361k each. `ci/run-detached.sh` is the fix and lands separately.

Wait with exactly one `Bash` call carrying `run_in_background: true`, whose
command exits when the condition is met:

```bash
until ssh -n threadripper "grep -qE 'T2-GAMEPLAY (GREEN|REFUSED|RED)|^(MERGED|ALREADY-MERGED|RED|REFUSED|NOT-FF|INFRA|GREEN|UNPUSHED) |^FAIL|died' ~/t2.log"; do sleep 120; done
```

You are re-invoked when it exits. Then:

* **Between arming and that notification, issue no tool call whose only purpose
  is to check on it.** A poll costs a full context write to learn what the
  notification delivers for free. If you have nothing else to do, end the turn.
* **Match the failure signatures too**, not just the success one. A waiter that
  fires only on green is silent through a crash, and silence is
  indistinguishable from still-running.
* **AND THE PATTERN ABOVE USED TO BREAK THAT RULE ITSELF — 2026-08-18.** It
  matched `T2-GAMEPLAY (GREEN|REFUSED)`, and **`ci/t2-gameplay.sh` never
  prints a `T2-GAMEPLAY RED` line**: it prints `GREEN` on success, `REFUSED`
  when it declines to measure, and on a genuine corpus regression it simply
  **exits non-zero, saying neither**. So the recommended waiter was silent
  through the one failure the stage exists to catch. Checked against the real
  logs rather than reasoned about: on the four gate logs that carry a handoff
  RED summary — `chain-cascade-signs` twice, `gate-skips-and-scenario-verbs`,
  `gate-be`, three of them at `t2=1/0` — the old pattern matches **zero**
  lines and `^RED ` matches all four. The handoff verbs are now in the pattern
  above because that summary is the one place a verdict is stated regardless of
  which stage produced it.
* **AND IT WAS STILL FOUR OF EIGHT — 2026-08-18, `ci-handoff-terminal-states`.**
  `ci/handoff.sh` prints eight words, not four. `ALREADY-MERGED`, `NOT-FF`,
  `GREEN` and `UNPUSHED` were all missing, so this waiter **hung forever** on an
  idempotent re-run, on a branch that needed rebasing, on **every `--dry-run`
  gate**, and on the one verdict that means the queue is wedged and somebody
  has to push. That is the same defect as the `T2-GAMEPLAY RED` one directly
  above, found in the same file, one bullet later — which is the argument for
  not maintaining this list by hand. **`ci/check-handoff-terminals.py` now reads
  this line and fails T0 if `ci/handoff.sh` can print a verdict the pattern
  cannot match**, exactly as it already does for `ci/lanes.sh`. The full table
  of what each verb means and what it asks you to do is in `docs/CI.md`.
* **If you are waiting on a DIRECT `ci/t2-gameplay.sh` run rather than on a
  gate, no log string can save you** — there is no handoff summary in that log,
  and the script emits nothing on a regression. Wait on the **exit status**:

  ```bash
  ( ./ci/t2-gameplay.sh > ~/t2.log 2>&1; echo "T2-EXIT=$?" >> ~/t2.log ) &
  until grep -q '^T2-EXIT=' ~/t2.log; do sleep 120; done
  ```

  Write the sentinel yourself and match on that. **The general rule this is an
  instance of: a waiter keyed on strings a script chooses to print can only see
  the outcomes that script has a string for.** Ask which outcomes have no
  string before you arm one.
* **Do not re-arm a waiter on resume** — see the block further down.
* Prefer doing other work on the same branch while it runs; a turn spent
  waiting is a turn paid for twice.

## HARD PRECONDITION — your own worktree, never the canonical checkout

**`/home/sayer/grit/repo` is not yours.** It is the checkout `ci/handoff.sh`
drives, and up to three agents run at once. Two of you checking out branches in
it is the 2026-08-06 mtime hazard by another door: `ci/gate-lock.sh` will refuse
a gate whose tree moved underneath it, and the verdict of a gate that *did* run
while someone else moved `$ROOT` is void rather than merely suspect.

Before anything else, make your own and work only there:

```bash
git -C /home/sayer/grit/repo worktree add --detach /home/sayer/grit/work-<yours> main
cd /home/sayer/grit/work-<yours> && git checkout -b <your-branch>
```

Pick a `work-<yours>` that does not already exist — `git worktree list` first.
If an orchestrator's spawn message tells you to work in `repo`, this rule wins;
say so and make the worktree. Leave it detached when you hand back, per the
block near the end of this file.

## HARD PRECONDITION — nothing compiles or runs on the laptop

Not the gate, not a test, not a one-off `cargo run`, not a dump. The laptop is a
Broadwell dual-core the owner is working on; an agent once started
`cargo test --release` there and the orchestrator had to kill four `rustc`
processes at load average 14 while a 24-thread box sat idle. Set this up
**before any other work**, with the `RUSTMC_REMOTE_DIR` your brief gives you:

```bash
export RUSTMC_REMOTE_DIR=<unique-name-from-your-brief>
./ci/t0-remote.sh > /dev/null 2>&1          # creates + populates the remote dir
ssh threadripper "export PATH=\$HOME/.cargo/bin:\$PATH; \
   mkdir -p ~/$RUSTMC_REMOTE_DIR/oracle-work/gameplay \
       ~/$RUSTMC_REMOTE_DIR/oracle-work/gameplay-selfcheck ~/$RUSTMC_REMOTE_DIR/data/vanilla; \
   for e in ~/rustmc/oracle-work/*; do case \$(basename \$e) in gameplay*) continue;; esac; \
       ln -sfn \$e ~/$RUSTMC_REMOTE_DIR/oracle-work/; done; \
   cp -rn ~/rustmc/data/vanilla/. ~/$RUSTMC_REMOTE_DIR/data/vanilla/; \
   cd ~/$RUSTMC_REMOTE_DIR && ./ci/gen-gameplay-tables.sh"
```

**The `export PATH` on the first line is not optional and was missing until
2026-08-12.** The inner-loop snippet further down has always had it; this one
did not, and a non-interactive `ssh` gets no login profile. What that costs is
not an obvious failure: `ci/gen-gameplay-tables.sh` runs eleven generators, gets
most of the way through, and dies on the first one that shells out to cargo
(`ci/dump-recipe-unlocks.sh`, `cargo: command not found`) — **after** the shared
tables have been copied in and **before** they have all been restamped. T0 then
comes back red on `ci/check-use-probe-falsify.sh` with `the dumper swept 1
stances, ci/checks/use-probe-dependent.toml pins 12` and three STALE rows, which
reads exactly like a branch defect in a tree whose branch touched none of it.

**The last line is not optional and the `cp` before it is only a shortcut.**
`cp -rn` brings in the version-pinned jar extracts, which are shared and
read-only and would cost minutes to re-derive. It brings in the
**generated** tables too, and those are branch-dependent and are not yours:
they carry the generating tree's provenance stamp, so `ci/t2-gameplay.sh` will
exit 4 on every one of them rather than measure them.
`ci/gen-gameplay-tables.sh` regenerates exactly that set, under your own
stamps.

**The count is deliberately not written here.** It said "eight" until
2026-08-04, when `entity_types.json` made it nine, and a number in this file
is a number every agent reads first and nobody re-checks. The list is derived
by the script from the `data/vanilla/…` paths the server source actually
mentions, so it is whatever the tree needs; the script prints the count and
that printout is the only figure worth quoting. `./ci/handoff.sh` does
all of this for you and is the preferred route.

**`oracle-work` and `data/vanilla` are directories you own, not symlinks you
borrow.** The line above used to be
`ln -sfn ~/rustmc/oracle-work ~/$RUSTMC_REMOTE_DIR/oracle-work`, and the same
was done to `data/vanilla`; eight trees on the box are still wired that way. It
cost a whole gate on 2026-08-03. `~/rustmc-gate` gated a branch at its own tip
and reported **12/55 declared with 11 regressed** for a branch that was fine,
because both paths resolved into a checkout sitting on `main`:
`data/vanilla/set_placed_by.json` — the table the branch existed to add — was
absent, and `use_without_item.json` was 133 KB behind what the branch's own
dumper generates. Our server then behaved exactly like a server missing half
the branch, and every trace agreed with every other trace while all of them
were wrong.

The three things are different in kind and the convention conflated them:

* the **vanilla references** under `oracle-work/` — 3.8 GB, twinned,
  expensive, read-only. Share these. Link them in individually, which is what
  the loop above does;
* `oracle-work/gameplay/` — live per-run output. Two trees sharing it
  overwrite each other's recordings mid-run;
* `data/vanilla/` — **generated from the tree's own source and therefore
  branch-dependent.** A shared copy gates every branch against whichever
  branch last regenerated it.

**There is a fourth kind, and the `gameplay*` glob above hid it until
2026-08-06: `oracle-work/gameplay-selfcheck`.** The glob skips `gameplay` and
`gameplay-selfcheck` alike, and only `gameplay` was ever `mkdir`ed — so a tree
set up from this file had no `oracle-work/gameplay-selfcheck` at all, and
`ci/selfcheck-recompare.sh` could not run in it. Skipping the *link* is
correct, and that is why this is a missing `mkdir` rather than a missing
`ln`: `ci/t2-gameplay-selfcheck.sh` **writes** there, and two trees sharing
one would overwrite each other exactly as they would `gameplay`. What was
missing is that the same path is also the **read-only input** to
`ci/selfcheck-recompare.sh`, and the recordings on the box are 26 runs and
2.4 GB — about five hours of vanilla boots, which nobody is re-taking to run a
comparator. So the directory is created empty, for this tree's own recorder,
and a recompare is pointed at the shared recordings explicitly:

```bash
RECORDINGS=~/rustmc/oracle-work/gameplay-selfcheck ./ci/selfcheck-recompare.sh
```

> **THE RECORDINGS ARE GONE — 2026-08-08.** After the laptop died and its data
> was restored onto the Threadripper, `~/rustmc/oracle-work/gameplay-selfcheck`
> exists and is **empty**, as are the copies in every other tree on the box;
> `find ~ -type d -name 'selfcheck*'` matches nothing else and no `vanilla.json`
> outside the two live `oracle-work/gameplay` directories survives. So the
> override above now points at an empty directory and
> `ci/selfcheck-recompare.sh` will exit 2 — which is the honest refusal that
> paragraph is about, working as designed. **Everything below about what those
> 26 runs measured is history, not an available instrument.** Re-taking them is
> about five hours of vanilla boots and is nobody's incidental errand: an item
> that needs a vanilla-against-itself rate has to budget for it. The first item
> that does is named in `docs/PARITY-ROADMAP.md` §10.
>
> **AND SINCE 2026-08-09 A RATE NO LONGER COSTS FIVE HOURS — check before you
> decide you cannot afford one.** `ci/corpus-floor.sh` measures vanilla against
> itself over **recordings that already exist**: every tree that has run
> `ci/t2-gameplay.sh` leaves one at `oracle-work/gameplay/vanilla.json`, and
> all pairs of them are compared in seconds.
>
> ```bash
> ./ci/corpus-floor.sh ~/rustmc*/oracle-work/gameplay/vanilla.json
> ```
>
> It refuses on fewer than two, prints the resolving power beside the floor so
> a small N cannot be read as a clean one, and separates what **vanilla** cannot
> reproduce from what **this harness** cannot sample — which are different
> findings with opposite fixes. `docs/PARITY-ROADMAP.md` §13, ORACLE rule 28.
>
> The five-hour figure still stands for `ci/t2-gameplay-selfcheck.sh`, which is
> the stronger instrument because it controls the recorder, and whose
> recordings are the best possible input to the cheap one:
> `./ci/corpus-floor.sh oracle-work/gameplay-selfcheck/r[0-9]*.json`. The
> character class is not decoration — that directory also holds `report-N.json`.

**No past measurement was taken through this hole, and the reason is worth
more than the relief.** `selfcheck-recompare.sh` exits 2 on a missing
recordings directory, exits 2 again on fewer than two recordings, and
`ci/selfcheck-aggregate.py` applies a power test that reports a low-N run as
*unresolved* rather than as stale. The failure mode was refusal, not a quiet
wrong rate — so this is a **hole that could not pass**, which is the opposite
of and much cheaper than this project's usual defect. Checked rather than
assumed: all four recompare runs on the box — `rustmc-blockentities`,
`rustmc-chain-shape`, `rustmc-redstone-gold`, `rustmc-signs-side` — produced
25 pairwise reports each, which is the full N = 26 set. The three of those
that have no local `gameplay-selfcheck` got there with the `RECORDINGS=`
override above.

`ci/t2-gameplay.sh` now refuses rather than reporting a meaningless number:
`ci/gen-gameplay-tables.sh` stamps every generated table with the tree that
made it and the digest of its generator, and the gate exits 4 on a table that
is absent, foreign, or older than the dumper that produces it.
`./ci/handoff.sh <branch>` does the whole setup correctly and is the preferred
route.

Inner loop for the rest of the turn. **The exclude patterns must not end in a
slash** — a trailing slash once replaced the remote's real `oracle-work` with a
self-pointing symlink and cost 200 MB and a re-extract:

```bash
rsync -az --delete --exclude target --exclude oracle-work --exclude .git \
    --exclude 'data/vanilla' ~/rustmc/ threadripper:$RUSTMC_REMOTE_DIR/
ssh -n threadripper "export PATH=\$HOME/.cargo/bin:\$PATH; cd $RUSTMC_REMOTE_DIR && \
    cargo test --release -p rustmc-worldgen --test <name> -- --nocapture"
```

**`rsync -a` PRESERVES MTIMES, AND CARGO TRUSTS MTIMES. 2026-08-08.**
If you ever put an **older** tree in your remote dir — the commonest reason is
a red-before measurement, `git archive <base> | tar -x` and rsync that over —
then rsyncing your branch tip back leaves every source file with **its own
mtime, which is now older than the artifacts built from the base**. Cargo's
freshness check compares the two and reuses the stale artifact. Measured that
day in `~/rustmc-placesound`: `crates/rustmc-proto/src/ids.rs` at 22:02
against `librustmc_proto.rlib` at 22:22, built from the base's copy of that
same file.

**The loud failure is the lucky one.** It surfaced as
`cannot find value WAYPOINT in module rustmc_proto::ids::play::cb` — and only
because the branch added a *new* source file, which has no artifact and so
forced `rustmc-server` to rebuild against a stale `rustmc-proto`. A branch
that only *edits* existing files hits no such forcing: every crate looks
fresh, the release binary is the **base's**, and `ci/t2-gameplay.sh` measures
the base while its log names your branch. Every trace agrees with every other
trace and all of them are wrong.

So after any rsync that could move mtimes backwards:

```bash
ssh threadripper "cd \$RUSTMC_REMOTE_DIR && find crates tools ci -type f \
    \\( -name '*.rs' -o -name '*.toml' -o -name '*.sh' \\) -print0 | xargs -0 touch"
```

Local is fine for: reading, editing, `javap`, `unzip`, `cargo fmt`, git. Nothing
that invokes `rustc` or the JVM. `touch /run/ci-busy` on the box if you start
something long, and release it after. The Threadripper is 192.168.1.122;
`~/bin/tr-wake` sends WoL to `a8:a1:59:12:f8:b8` if it is off.

Export a **unique** `RUSTMC_REMOTE_DIR`. Agents sharing one remote dir rsync
`--delete` over each other, and other trees exist on that box.

## THE TREE DOES NOT BUILD ALONE — 2026-08-06

`crates/rustmc-server` depends on `grit-plugin-api`, which lives in the
**`grit-plugins` repo checked out beside this one** (`../plugins`). It is a
path dependency and not a copy, because a mirrored contract crate would make
this server's plugin types a different crate from the one the first-party
plugins are built against, and no test anywhere could then load a real plugin
into the real server.

**You do not have to do anything about this.** `ci/t0-remote.sh` ships the
sibling to `~/plugins` on the box every time it ships the tree, and every tree
on the box resolves `../plugins` to that one directory. It is shipped rather
than set up once for the reason `data/vanilla` is copied rather than shared: a
directory nobody re-checks is how a gate ends up measuring whichever branch
last touched it. If the sibling is missing on the laptop, `t0-remote.sh` exits
2 and prints the `git clone` — it never skips, because a skip here surfaces as
a Cargo error two steps later that reads as a broken branch.

## The corpus lock — you no longer have to remember this one

**`ci/t2-gameplay.sh` takes a box-wide lock and waits its turn. Just run it.**
Nothing to set, nothing to release, nothing to check first. If another corpus
run or a merge gate has the box, yours queues and then proceeds, printing who
it is waiting for and a heartbeat while it waits.

### Since 2026-09-24 it is a POOL, and a run can be SHARDED (`corpus-parallel`)

* **`RUSTMC_CORPUS_LOCK_POOL` runs hold the box at once (default 3)**; a gate
  holds EVERY slot (it drains the pool, whatever pool size it was started with),
  so nothing records beside a gate; a gate
  still goes before every queued run, and two runs of ONE tree still serialise
  (they share `oracle-work/gameplay`). `=1` is the old mutex exactly.
* **`RUSTMC_CORPUS_SHARDS=N`** records one run as N contiguous shards on N
  isolated vanilla+rustmc pairs at once, merged in corpus order before the
  comparison. Its default may only be a count that
  `ci/checks/corpus-shard-equivalence.toml` proves verdict-equivalent to a
  serial run; `ci/corpus-shard-equivalence.py --check` enforces it in T0.
  **It is 8 since 2026-10-01** (proven over all 179 scenarios against two
  serial controls): a full corpus holds the lock ~516 s instead of ~5,000 s.
  `RUSTMC_CORPUS_SHARDS=1` is the serial run.
* The 2026-08-06/07 "stall" below was **recorders sharing one vanilla** (one
  port pair for every tree, or a symlinked `oracle-work`): a second recorder is
  kicked with `duplicate_login` and a reused world errors half its scenarios.
  Reproduced on demand, `docs/FINDINGS.md` 2026-09-24. So the rules that
  actually matter now: **a corpus harness binds only ports it derived for its
  own tree and probed** (`pick_port_pair`, `ci/corpus-ports.sh`) and **records
  only into its own tree's `oracle-work`**. Never add a fixed default port.

The paragraphs below describe the mutex era and are kept for their history.

### There are two locks and only one of them is box-wide

Confused by the orchestrator on 2026-08-09 and corrected by an agent, so it is
written here rather than left to be re-derived:

* **`ci/gate-lock.sh` keys per gate-*directory*.**
  `box.threadripper_rustmc-gate`, `…-blockage`, `…-twin` are three separate
  keys. It exists so that two gates sharing one remote directory cannot collide
  through rsync mtimes. It is an **mtime mutex, not a CPU one** — taking it
  under your own `RUSTMC_REMOTE_DIR` excludes nobody, and a HELD reading on
  somebody else's key does not mean the box is busy.
* **`ci/corpus-lock.sh` is the box-wide one.** A merge gate declares into it as
  role `gate`, which is what makes gates and corpus runs mutually exclusive.

So: **if you need the box quiet, take the corpus lock — as role `run`, so a
real merge gate still jumps ahead of you.** This is the one sanctioned reason
for a bench to sit inside the corpus lock, and it is bounded: say in your script
header how long your window is, or the next agent will read it as a violation of
the rule that benches stay outside.

**A free gate lock is not a quiet box, and neither is a low load average.**

This replaces a rule that was written down, was correct, and did not hold: *a
corpus run and a merge gate must never overlap; the gate wins.* On 2026-08-06
three ran at once — two agents and a gate — and the damage was invisible,
which is why nobody stopped it. A corpus run sits in `settle_ms` waits and
vanilla boots rather than on a core, so concurrent runs do not share a busy
CPU; they wait out each other's timers. Throughput fell to **one scenario per
ten minutes at load average 0.02**, and that low load actively misleads: one
agent read it as "nothing is contending, so my run is wedged" and spent the
turn hunting a stalled process that did not exist. One run was killed at 74/86,
one at 50/86, one abandoned at 70/86, and every one of them ended because a
human noticed.

What you actually need to know:

* **`./ci/corpus-lock.sh status`** says who holds it, since when, and who is
  waiting — from any tree, at any time, without disturbing the run. Use it
  before concluding that a quiet run is stuck. A wait is never silent, so a
  silent terminal is not a wait.
* **NEVER use the load average to decide whether this box is busy.** It
  measures runnable threads, and a corpus run is asleep in `settle_ms` timers
  and vanilla boots, not on a core. A run that is **completely healthy** shows
  `cputime=00:00:01`, exactly **2 client joins**, and a vanilla log not written
  to for **twenty minutes** — measured against a solo lock-holder at N=34 by
  `ci/corpus-healthy-baseline.sh`. Those three readings were once taken as
  proof that two overlapped runs were starving each other; they do not
  distinguish a stalled run from a fine one, and the run they were taken from
  went on to record 88/88 and 88/88 cleanly. See `docs/CORPUS-LOCK-HOLD.md`
  §13. So a busy corpus run and an idle box look identical, and the inference
  "nothing is contending, so my run is wedged" is backwards either way.
  `status` answers the question the load average cannot: not "is the CPU busy"
  but "does somebody already have the box".
* **What a corpus run costs: about 12.5 s per scenario**, from
  `ci/corpus-phase-mtimes.sh` at **N=113** real gates (reference 22.42 min over
  108 scenarios). The older "about 9 s" came from one run's first 70 s; the
  same eight scenarios measured 12.07 s each at N=16 in 2026-08-18's
  experiment, so 9 s made a normal run look 35% slow. Quote 12.5. Concurrency
  is not the thing to suspect — two recordings at once cost at most 1.3% more,
  `docs/CORPUS-LOCK-HOLD.md` §15. Do not build a ratio against a run whose
  progress you measured by some proxy other than that counter.
* **A merge gate jumps the queue.** `ci/handoff.sh` stage 6 declares itself,
  and a queued run that has not started yet hands the lock back to it. A gate
  still waits out a run already in progress: fifty minutes of corpus is not
  thrown away to save a gate twenty.
* **Nothing goes stale.** The lock is held by a childless anchor process that
  the kernel kills when your run dies, `kill -9` included. There is no timeout
  and no lockfile to clear by hand. If `status` shows a holder whose PID is
  gone, the lock is already free and it is saying so.
* **Runs are served oldest-first since 2026-08-12, and a gate still jumps the
  whole queue.** Before that the queue had no order at all and one branch sat
  in it for 1h58m while three later arrivals took the box: an arrival that
  finds the lock momentarily free takes it with `flock -x -n`, so it never
  blocks and never sees the queue. `./ci/corpus-lock.sh status` now prints the
  waiters in service order, which is a statement about what will happen rather
  than a listing. Nothing about this needs remembering — it is the same one
  `corpus_lock_acquire` call.
* **`ci/corpus-lock-falsify.sh`** is its gate — twenty-three cases, nineteen
  injections, 16m42s measured 2026-08-12, and it runs in a private lock
  namespace so it cannot disturb real work. (This line said twelve and twelve;
  both counts were stale before the queue was added.) `CORPUS_LOCK_FALSIFY_DRYRUN=1`
  applies all nineteen patches and runs no case, in two seconds — run that after
  touching `ci/corpus-lock.sh`, because a patch that has stopped matching is a
  hole in the sweep whether or not you have seventeen minutes.

Still yours to remember: `touch /run/ci-busy` for long non-corpus work, and
`ci/gen-vanilla-region.sh`, the benches and `ci/collide-falsify.sh` are **not**
under this lock — they are a different resource and serialising them behind the
corpus would cost throughput for an exclusion nobody has asked for.

## Read first

1. `docs/RACE.md` — deadline, win conditions, one hard rule.
2. `docs/WORKLIST.md` — **three `## NOW` blocks; they are tracks, not a
   sequence.** An index table at the top says which is which. Unless your brief
   says otherwise yours is `## NOW — worldgen features`, the active one; the
   perf block is deferred by owner priority. Use `grep -n '^## '` — the file is
   ~19,600 lines (2026-08-11, down from ~24,900 the day before — see below) and
   mostly archive, so do **not** read it whole.
3. `docs/BENCH.md`.

**Then `docs/FINDINGS.md`** — append-only, one line per finding. Read it, and
append when you find something another lane could plausibly hit. The same
`figure-provenance` defect was found independently by **five** lanes on
2026-08-18/19, so four area-lanes went on a rediscovery. It carries
`merge=appendonly`: add your line at the bottom, never edit or reorder anyone
else's, and your append will not conflict with a lane doing the same thing at
the same moment.

**A branch's cold start is a file now, not a block inside WORKLIST.** The 25
`### COLD START — branch X` paragraphs that used to be inserted at the same
anchor in `## NOW — server track` — the reason two branches rebasing past
each other there collided textually and usually on the same section number
too — now live one-per-branch at `docs/cold-start/<branch>.md`, with a
generated `branch | date | summary | file` index left where they were. See
the "Before you stop" paragraph under **Budget and handoff**, further down,
for what changed there, and `docs/claims/` and `ci/check-doc-claims.py` for
the same fix applied to `docs/DETERMINISM.md` and `docs/PARITY-ROADMAP.md`
section numbers.

**Trust the tree over any figure in your brief.** Every agent for six turns
running has had a briefed claim refuted by its first measurement: one killed
both of its brief's claims, one had its central premise *and* its named oracle
refuted, and one refuted a comment in our own source. Expect the same, and say
so plainly when it happens.

## Repo policy — absolute

**No tracked file may name or allude to another server implementation.** Not
code, comments, docs, or commit messages. No quoting its source, citing paths
inside another tree, reproducing its figures, or recording that a reading of one
was taken. History was scrubbed clean and stays that way; comparative material
lives in `~/rustmc-private/` only. When an item is motivated by something read
there, justify it **from our own measurements**.

**Clean-room rule, unchanged and absolute: every line is written from vanilla
bytecode.** `unzip oracle-work/vanilla-26.2/versions/26.2/server-26.2.jar`, then
`javap -p -c -constants`. Never transcribe code from any other implementation,
in whole or in part.

## What is being built, and what may be deferred

* **Defer verification, never fidelity.** Every mechanic is derived from
  vanilla's bytecode as faithfully as worldgen was — same draw order, same
  constants, same iteration order, same tie-breaking. What relaxes outside
  worldgen is only how much *proof* is demanded before moving on. The two
  kinds of debt are not comparable: an approximation is a one-way door — a
  "close enough" pathfinder cannot be upgraded to parity without a rewrite,
  and the architecture forecloses the goal — whereas a golden vector can be
  added at any later date and nothing is lost by waiting.
* **The two names are parity mode and fast mode, and no others.** Not "lite",
  not "reduced": feature coverage is identical in both and only the
  exactness/speed trade-off differs. This is the parity server with a fast
  setting, not a cut-down server with a parity option.
* **Fast mode is deferred entirely until all of parity mode is done.** No
  flags, no config switches, no second code paths, no stubbed variants. There
  is exactly one implementation of each mechanic — the parity one.
* **Parity mode must itself be fast, and that work belongs now.** Data
  layout, avoiding virtual dispatch, cache-friendly iteration, fewer
  allocations, threading that cannot change results: all of it is parity-mode
  work. The dividing line is not optimised against unoptimised, it is
  **identical results against changed results**. The public speed claim
  refers to parity mode.
* **A negative result is only evidence if the mechanism was live.** Learned
  twice the hard way here: a datapack grep "proved" bearding inert when the
  marker is never in the datapack in any version, and the gameplay harness
  recorded silence from vanilla for a whole corpus because the player it was
  driving had never moved. Ask what would make every instrument agree while
  all of them are wrong — which is not the same question as whether they
  agree. `tools/oracle-runner`'s **vacuous** verdict is this rule mechanised:
  if the *reference* produced nothing under a check's own projection, the
  check has measured nothing and can neither pass nor fail. Its **inert**
  verdict is the sharper version, added when vacuous stopped being enough:
  once a mechanism's *unconditional* output is reproduced, the reference is
  never silent, so a dead check has to be caught by subtracting what the test
  itself forced and asking whether anything is left.

## Output hygiene — tool output is the largest uncached cost

Reading more than you need is not more rigorous, it is just more expensive,
and every byte is replayed on every later turn of the run.

* **Pipe `javap` through a targeted `grep`/`sed`** for the method or the
  offset range in question. Dumping a whole class costs tens of thousands of
  tokens to answer a question about nine instructions.
* **Bound every exploratory `grep`** with a tight pattern and a `head` limit.
  A loose pattern against a binary, a bundled JS file or a region dump can
  emit hundreds of KB from a single call.
* **Read file ranges, not whole files**, once you know roughly where you are
  going.
* **Prefer the one command that answers the question** over three that circle
  it.
* **Never re-read a file you just wrote** to check the write landed. It did;
  the tool would have failed otherwise.

## Rules that make a number mean something

* **A NUMBER YOU PUBLISH NEEDS A COMMIT, A POOL AND A FLOOR — 2026-08-18.**
  Three ledger rules landed together and each fires on a shape agents were
  producing routinely, so read them before writing a figure rather than after
  `ci/t0.sh` reds:
  * `provenance` must name a commit that **git resolves** (rule 18). A date is
    not a tree, and it can be wrong: one entry was recorded as 2026-08-03 and
    its commit landed at 00:29 on the 4th. If your measurement has no sha,
    `ci/figure-provenance.py <id>` recovers it from the documents. 25 entries
    carried a bare date and 34 more carried a paragraph naming no tree; every
    one of the 103 resolves now, and `--check` re-derives them in T0.
  * a `bench` figure must record `workers` (rule 17). CPU-seconds moves 25.4%
    across the pool widths this box will hand you — 78.54 / 82.69 / 98.48 at
    1 / 12 / 24 threads, same build, same square. `ci/bench-servers.sh` now
    pins and stamps it; `"unrecorded"` is an accepted answer for a run you
    cannot re-take and is ratcheted at eight, so do not add a ninth.
  * `window.scenarios` must agree with the `corpus=N` your artefact stamps
    (rule 20). Eleven entries said 93 where the run said 92 or 106 — the
    93-that-was-really-92 correction reached the documents and never reached
    the ledger. **The stamp wins.** It is what the run printed about itself.
* **`docs/PARITY.md` §2 has a floor column now, and three of its twelve rows
  are UNDER their floor.** The nether r=16 row has been quoted as this
  project's worst number at 254,799 and its floor over the same window is
  317,725, so it is not evidence of anything. Do not quote a residual from that
  table without the floor cell beside it, and do not read a bare `0` as
  exactness — the End's r=16 zero is strong *because* its floor is 252.
* **The region-differ takes VANILLA FIRST.** Backwards inverts the sign of every
  number.
* **Rule 6: a residual is a port defect only if two independently generated
  vanilla worlds of that seed agree on it.** Known floors: overworld 3,006
  blocks on the r7 window and **4,105** on the centre-(64,64) one — which is
  *larger than our whole disagreement with vanilla there*, so that window is
  corroboration only and never a headline; nether 48; the End **127 across six
  same-order references, NOT 0 and no longer 252** (the published zero was one
  lucky pair, corrected 2026-08-07; the 252 that replaced it came from six
  references of *unrecorded harvest order* and every one of them was destroyed
  in the disk failure, so it was unreproducible from 2026-08-07 until it was
  re-taken on 2026-08-18 — `docs/DETERMINISM.md` §§5, 48). Quote every sub-box
  with its own local floor beside it.
* **A FLOOR BELONGS TO ONE HARVEST ORDER AND THE END IS THE CHEAPEST PLACE TO
  SEE WHY.** Six End references harvested in one order floor at 127; the same
  window's `rows` against `rows_rev` moves vanilla by **944**, which is seven
  times the floor. Mixing orders inside an ensemble does not add noise to a
  floor, it measures a different quantity. Read `dump_order` off each
  reference's own `HARVEST-ORDER` and refuse a reference that has none —
  `ci/end-biome-window.sh` and `ci/nether-biome-window.sh` both do.
* **A DIMENSION'S EXACTNESS IS A WEIGHTED AVERAGE AND YOU MUST STATE THE
  WEIGHTS.** Both dimensions decomposed per biome on 2026-08-18 found something
  an aggregate hid, and in opposite directions. The nether's aggregate hid
  ~152,000 blocks of three unported feature families, because every window ever
  measured was soul sand valley or basalt deltas. The End's hid a *coverage*
  gap: its r=16 window weights `the_end` at **exactly zero** — the centre is
  1,448 blocks from the origin and that biome ends at 1,000 — so the central
  island was outside every published window, and the window itself returns
  `starts=0 pieces=0`. Before quoting any dimension figure, run the differ with
  `--by-biome` and look at the `win%` column. If a biome you would name in a
  sentence about the dimension reads 0.00%, the figure is not about it.
* **Rule 6 fires on the twin-masked r=16 overworld figures, and there is now a
  figure it does not fire on.** That window's floor is 29,560 against a masked
  residual of 29,490, so **no exactness figure from the masked route is
  quotable** and 99.959781% / 99.972035% must not be used; deltas on it are
  still sound. (`ci/figures.toml` disqualifies 99.959781% as **stale** and
  99.972035% as **not quotable** — those are the ledger's own words for them, so
  a grep for either finds this paragraph.) The quotable one is a different
  instrument on the same window:
  charge a position only when our block matches **neither** reference, mask
  nothing, and the answer is **29,605 of 107,053,056 — 99.972345%**, with the
  whole window as the denominator — which the ledger disqualifies as
  **uncontrolled chunk order**, its own words again and the reason spelled out
  three sentences below. **AND THAT ONE IS NOT QUOTABLE EITHER, AS
  OF 2026-08-10 — THE SEED-20260722 r=16 OVERWORLD WINDOW CARRIES NO EXACTNESS
  FIGURE AT ALL.** (Read "that window", not "that instrument": the *same*
  instrument on the *same* geometry at **seed 12345** resolves well clear of its
  floor and is the project's one quotable worldgen exactness. **It is 7,940 of
  107,053,056 — 99.992583% — against a floor of 2,577..4,835 at N=5**, and this
  paragraph said `69,288 / 99.935277%` until 2026-08-17. That older reading is
  `quotable = false, disqualifier = "superseded"` in `ci/figures.toml`: the
  `IcebergFeature`/`minecraft:iceberg` port dropped the charge by 61,348 blocks,
  88.5% of it, **and the floor did not move**, because a floor is a property of
  the reference ensemble and that change did not touch it. This same file records
  that port further down — "the `IcebergFeature` port that took the seed-12345
  window from 69,288 to 7,940" — so the two halves of this document disagreed
  for a week, and this is the half every agent reads first. See the
  bullet below and `docs/DETERMINISM.md` §17 before concluding this project has
  no adjudicating overworld window.) It was taken with our dump walking chunks
  x-major against references
  harvested z-major, and chunk order is a condition of a comparison the way the
  seed is: the same instrument charges an *unmodified vanilla run* **117,628**
  blocks for having the wrong one, against 3,804 for being a different run of
  the right one. Re-taken with the dump order matched to the references'
  `HARVEST-ORDER` it is **2,450**, and that sits *inside* the 2,202..5,263 the
  same instrument charges an innocent vanilla run at the same N — so the
  instrument has stopped separating us from vanilla here. What survives is
  corroboration and deltas. **Read `HARVEST-ORDER` off any reference before you
  compare anything to it, and set `RUSTMC_DECORATE_ORDER` to the `dump_order`
  it names.** ORACLE rule 29; `docs/DETERMINISM.md` §15 and §16. **The "45
  blocks of headroom" this bullet
  used to warn about was not a real quantity** — it subtracted the width of a
  mask from a charge that uses no mask. Corrected 2026-08-09 by measuring the
  admissible instrument's own floor against six new references
  (`docs/DETERMINISM.md` §14): an innocent unmodified vanilla run is itself
  charged 16,932 on this window at N=2. **The rule that replaces it: take
  deltas on this window freely — against a fixed reference set the charge is a
  deterministic function of your output and a one-block delta is real — but
  never quote a LEVEL from it below N=5, because at N=2 the innocent floor
  alone spans ~13,900 blocks.** To know whether your delta generalises,
  intersect your moved positions with the union of the references' pairwise
  disagreements (82,599 positions over eight references, 0.077% of the window):
  what falls outside it is your result. And decompose before you despair — the
  window's aggregate floor is unusable while `grass_block -> dirt` on the same
  window has a floor of **1**. (Until 2026-08-09 this bullet read 35,531 / 29,927 / 30,279 /
  99.971716%. Those were taken against the reference pair lost in the
  2026-08-07 disk failure, and every one of them was re-measured on the
  regenerated pair rather than converted.) `ci/admissible-residual.py`;
  `docs/DETERMINISM.md` §12. Rule 6 is not overridden there, its premise is
  absent: nothing is excluded, so there is no blind spot to be larger than the
  residual. **Do not reach for that construction to rescue a number elsewhere
  without saying so in the doc and registering it** — `ci/check-figures.py`
  will demand a `quotable_override` and an `override_reason` for it.
* **A floor belongs to the pair of runs that produced it, not to the seed and
  the window.** Measured 2026-08-07: over one 121-chunk square, two references
  generated at r=16 disagree by 12,185 blocks and two generated at r=5 by
  2,535. Never carry a floor from someone else's reference pair to yours, and
  never predict a window's resolving power from a floor you did not take.
* **`GEN_ORDER=perchunk` does not reduce the overworld floor.** 2,535 under
  `rows` against 2,728 under `perchunk` on the same square, with a positive
  control of 3,345 between the two orders proving the knob turned. The floor is
  vanilla's own decoration order — lakes, speleothem clusters and overlapping
  stone-variant ore blobs, all of which read the world before they write it.
  `docs/DETERMINISM.md` §11 refutes both harness explanations out of the
  `ChunkPyramid` bytecode. Do not re-open this.
* **A REFERENCE CARRIES A CHUNK ORDER AND YOUR DUMP MUST MATCH IT.** This is
  the single largest uncontrolled variable this project has had, measured
  2026-08-10. The admissible instrument charges an **unmodified vanilla run
  117,628 blocks** for having the wrong chunk order, and 3,804 for being a
  different run of the right one — against an r=16 residual of 14,270 that a
  whole brief was written about. `ci/gen-vanilla-region.sh` stamps
  `HARVEST-ORDER` into every reference it makes and names the matching
  `RUSTMC_DECORATE_ORDER`: `rows` → `zx`, `rows_rev` → `zx_zrev`. Set it. And
  **an N-reference ensemble sampled along one order is not an N-reference
  ensemble** — a ninth same-order reference took that residual down 307 blocks,
  two order-varied ones took it down 11,687. ORACLE rule 29,
  `docs/DETERMINISM.md` §15, `ci/order-charge-sweep.py`.
* **But do NOT estimate what the order is worth on your window from another
  window's** — 2026-08-10, and this is the correction to the bullet above, not
  a footnote to it. Chunk order is **83%** of the 20260722 r=16 residual and
  **6.7%** of the seed-12345 one (74,224 → 69,288) under the identical
  instrument and geometry. Measure it; it takes one extra dump. And a window
  that carries no exactness figure is a fact about **that window**, not about
  the instrument: seed 12345 resolves at 14x its floor where 20260722 resolves
  at none. `docs/DETERMINISM.md` §17.
* **AN ASSERTION IS NOT A STAMP, AND AN UNRECOGNISED KNOB IS NOT A TURNED ONE**
  — ORACLE rule 30. Before scoring against a reference, read its
  `HARVEST-ORDER`; if it has none, its order is *recoverable* (diff it against
  a stamped reference of each order — the same-order/cross-order gap is ~5x)
  but it is not *known*, and the anchor of your ensemble must be stamped.
  `ci/classify-12345-ab.sh` is the pattern, controls and all. And when you set
  `RUSTMC_DECORATE_ORDER`, know what would tell you it did not take: until
  2026-08-10 an unrecognised name ran the default raster **and still printed a
  `decorate-order:` line naming the order it had not used**, while the ladder's
  own digest check was reading the biome-palette digest, identical for every
  order by construction. Both are fixed and both have positive controls; the
  class of defect is not.
* **Vanilla is not the stable one.** Two unmodified vanilla runs of one seed and
  window disagree by 32,127 blocks in the same chunk order and **146,169 across
  orders** — 4.6x. Any hypothesis of the form "we are order-dependent where
  vanilla is not" has to clear that before it is worth an hour.
* **`DECORATION_MARGIN` is 2.** A ring one too narrow once made a window wrong
  by 604 blocks — more than its entire disagreement with vanilla, so the number
  was partly measuring its own margin. ORACLE rule 14: widen the context until
  the answer stops changing, and quote the converged one.
* **Structures need a wider margin than decoration** — one can start outside
  your window and write into it. Establish it by measurement.
* **The order sweep has nine names and eight orders.** `diag` writes bytes
  identical to `default` (`diff -rq`, 2026-07-30) while reporting a different
  traversal digest, so distinct digests do not imply distinct worlds. Quote it
  as eight.
* **Decompose a residual by order before opening any investigation on it.**
  `ci/decorate-order-intersect.sh`, nine dumps and nine diffs, about twenty
  minutes. On the r=7 overworld window, twin-masked, **1,130 mismatches fall to
  51** under intersection — 95.5% of that residual is which chunk decorated
  first, not a defect. Two separate agents have now spent a turn each chasing
  a lump that the intersection deletes in one command. **Then ask the second
  question, because the first one cannot answer it:** `ci/order-charge-sweep.py`
  scores the charge *under* each order rather than counting what each order
  moves, costs seconds, needs no dumps, and is what turned "94% of the r=16
  residual is order" into "82% of it was one wrong knob" (ORACLE rule 29).
* **Vanilla fails its own order test.** See `ci/decorate-order-sweep.sh`'s
  header. "Every decoration order agrees with one vanilla reference" is NOT a
  valid closing condition: vanilla has its own order floor — 253 blocks in the
  End walked z-descending, 3,621 in the nether one way and 243 the other.
  `GEN_ORDER` has `rows` **and** `rows_rev`; a control using only `rows` varies
  which *ascending* row goes first, is blind in the only plane that matters, and
  once wrongly convicted our code. **Corollary:** one order run nine times is
  indistinguishable from a pass — confirm distinct traversal digests. At centre
  (0,0) `spiral` and `spawn` are legitimately the same function, so a central
  window is honestly 8-of-9.
* **Rule 7: when a measurement clears every candidate, suspect the measurement.**
  Fourteen payouts so far — two gates green because they had stopped looking; an
  instrument that disabled the thing it measured (`RUSTMC_STAGE_TIMES` forced
  the surface stage sequential); an oracle that replayed instead of comparing; a
  `pgrep` that self-matched into a 60-minute timer; an attribution wrong by 25
  blocks for want of a census; a caveat list stale by a whole commit; a vanilla
  "control" blind in one plane; a structure-density estimate over-read 2.5x
  because `random_spread` with **triangular** spread is not `1/spacing²`; an
  oracle that filtered on `pool_element` and so reported "no jigsaw starts" for a
  world holding a 70-piece end city; and an unbounded census that explained a
  225-chunk window with blocks from its 362-chunk margin.
* **Rule 9: measurements move suspects, readings of code do not.** Attribute a
  residual to specific positions with a census before changing code.
* **A divergence is a finding, not a failure.** A dimension honestly at 99.99%
  beats one falsely at 100%.
* **`region-differ census` needs `--bbox` AND `--match`** — an unbounded census
  pushed 35.6M blocks down an ssh pipe and took the box off the network — **and
  a census must carry the same `--bbox` as the diff it explains.**
* **Pick markers a structure cannot avoid.** `van-village` holds zero
  `cobblestone_stairs` and still contains a village. Use `rail`/`cobweb` for a
  mineshaft, `trial_spawner`/`chiseled_tuff_bricks` for a trial chamber,
  `purpur_block` for an end city.
* **Look for the cheap gate before diffing blocks.** A block diff cannot
  distinguish a wrong template from a wrong rotation from a wrong stream
  position — they all read as "blocks in the wrong place". Pin the draws first.
  The `end_city` port closed because vanilla's own `GD` field turned out to be
  the RNG stream value at the total draw count of the entire walk: one integer,
  milliseconds, no vanilla run, gating element order and every branch. Ask
  whether your structure has the same shape.
* **A gate a window cannot exercise proves nothing.** Say so in the handoff
  rather than letting a green number imply coverage. A structure placed at
  `rot=NONE` tests no rotation; a reference with no ship tests no ship branch.

## Every gate

**A gate that cannot fail is not a gate.** `ci/t0.sh` ran
`cargo test --workspace --lib` for weeks. `tools/oracle-runner` is a **binary**
crate, so `--lib` matched none of its targets: nineteen tests had never run in
any gate, and one of them was failing — it asserted that the cursor for the
`DOWN` face lay on the *x* face, because `Direction`'s ordinal groups the axes
Y, Z, X and the test used the pair index as the coordinate index. Every
"T0 GREEN" ever reported was, on that crate, meaningless. It is now
`--lib --bins` (2026-08-03).

**26.2's gamerule ids are snake_case and several are not renamings**:
`doMobSpawning` is `spawn_mobs`, `doWeatherCycle` is `advance_weather`,
`doDaylightCycle` is `advance_time`, `randomTickSpeed` is
`random_tick_speed`, and `doFireTick` has become the *integer*
`fire_spread_radius_around_player`. Read them out of the
`minecraft:game_rule` registry in Mojang's own report — it has 59 entries —
and never from memory of an older version.

This is written here rather than in one script's header because it has now
been discovered **twice**. `ci/gen-vanilla-region.sh` found and fixed it on
2026-07-28 and left a full comment saying so; `ci/t2-gameplay.sh` was written
a week later with the old ids and repeated the whole thing. A correction that
lives only in the file it corrected does not travel.

It happened **twice on 2026-08-03**, and the second one is the better
example because nothing about it looked like a gate at all. `ci/t2-gameplay.sh`
sent `gamerule randomTickSpeed 0` to vanilla and moved on. In 26.2 game rules
became a registry with snake_case namespaced ids, so the server answered
`Incorrect argument for command` and carried on — for as long as the corpus
had existed. Random ticks ran through every measurement the docs quoted, and
the resulting grass-dying-under-a-block was chased twice as a mystery. **A
console command whose failure is not read is not a command.** It is now
`gamerule_or_die`, which greps the log and exits.

This is the same class as the datapack grep that "proved" bearding inert
because the marker is never in the datapack, and the gameplay harness that
recorded silence from a player who had never moved. **Ask of any green
result: what would have had to happen for this to come back red?** If you
cannot answer, you have measured the instrument. When you add a crate or a
target, check the gate's selector still reaches it.

**That question is mechanised now for the two places it kept being answered by
hand** — gameplay scenarios and readiness proofs. See the HARD RULE at the top
of this file, `ci/corpus-falsifiable.py` and `ci/liveness-proofs.py`. It is
still yours to ask everywhere else: neither gate reads a golden table, a
census or a bench, and the class has appeared in all three.

`ci/t0-remote.sh` on the **committed** tree.

**Do not confirm it ran with `grep -c "skipping: no vanilla" <log>`.** That
number is **0** on every run this project has ever made, whether every
fixture-backed check compared its vectors or none of them did, and quoting it
as evidence is how a lane certifies a gate that did nothing. Two independent
reasons: `cargo test` never prints a passing test's stdout or stderr and a test
that returns early *passes*, so the line is never written; and the pattern
matched 27 of the 119 skip-message sites in this workspace when it was
measured, missing the largest wording entirely. Measured 2026-08-18 on `main`
with the whole datagen extract removed: 111 test binaries, 1749 tests passed
against 1758 with it present, 122 s against 1521 s, and zero lines containing
the word. `docs/PARITY-ROADMAP.md` §73.

What replaced it needs no grep. `crates/rustmc-data/src/fixture.rs` makes a
missing fixture **panic** rather than skip; `ci/check-fixture-skips.py` runs in
T0 and ratchets the number of sites that have not adopted it; and
`ci/t1-gate.sh` refuses **before** running when the extracts it needs are
absent, exiting 2 so the refusal is not read as the branch's fault. A green T0
is the confirmation now, and it is one you do not have to remember to ask for.

Never pipe a gate through `head`/`tail` — a pipeline exits with its *last*
command's status, which has masked a failure and let `&&` run anyway, twice. Use:

```bash
./ci/t0-remote.sh > /tmp/gate.log 2>&1; T=$?
```

**`cargo fmt` before every commit.** `ci/t0.sh` opens with `cargo fmt --check`,
and a branch once sat unmerged for a day having failed its gate on nothing else.

## Working rules

* Your worktree needs the gitignored data — see the HARD PRECONDITION block
  above for the exact loop. `data/vanilla` is a **copy**, `oracle-work` is a
  **real directory with the references linked in individually**, and
  `oracle-work/gameplay` and `oracle-work/gameplay-selfcheck` are your own. Without the references three worldgen
  tests fail and it looks like main is broken; with a shared `data/vanilla`
  your gate measures whichever branch last regenerated it, which is a red that
  means nothing.
* **Commit early and often.** An agent was killed mid-run by a transient API
  outage with hours of uncommitted work in its tree.
* Never `git add -A`. Stage explicit paths.
* Commit messages state the measured before/after, the seed, the window and the
  box. A ratio without a named box does not transfer.
* **No `Claude-Session:` trailer.** The 1,000 commits carrying one were
  rewritten out on 2026-08-02; do not reintroduce it. Everything else about
  commit messages stays as above.
* **Watch for stale caveat lists and docs.** `worldgen-dump`'s caveat block was
  found lagging the code by a whole commit; its usage text claimed "end cities do
  not run yet" long after they did; `generate.rs` claimed only mineshafts and
  warm ocean ruins were generated long after villages were being placed; and
  `place_piece`'s comment about `setKnownShape` was simply false. **A caveat list
  that lags the code reads as authoritative and is worse than none.** Fix any
  line your change makes untrue, in the same commit.
* **New behaviour lands opt-in until it is measured.** Waterlogging was added to
  `PlaceSettings` defaulted **off** so no unmeasured structure silently moved.
  Hold that discipline for any new pass.
* Do not regenerate existing vanilla references in `oracle-work` — they are
  expensive and twinned.
* **Editing `docs/WORKLIST.md`: match on text INSIDE your block, never on a
  `## NOW` heading string.** One splice matched the heading in the index table
  and briefly ate the other two tracks. Before staging, `grep -c '^## NOW'` must
  return **3**.
* **Your cold start is `docs/cold-start/<your-branch>.md`, not a paragraph
  spliced into WORKLIST.** Write or append to that file — a branch only ever
  writes its own, so this cannot collide with anyone else's. The only touch
  WORKLIST itself needs is your **one row** in the `COLD START INDEX` table
  (branch, date, one-line summary, link) under the relevant `## NOW` block:
  add a new row if you have not cold-started this branch before, replace your
  existing row if you have. That is still a shared table, so a genuine
  same-branch double-edit can still conflict there — but the payload dropped
  from a multi-paragraph splice to one table row, and a same-branch conflict
  is you against your own earlier self, not you against another branch.
* **That table, `docs/PARITY-ROADMAP.md`'s section body and `ci/figures.toml`'s
  `[[figure]]` body are now declared merge regions, and you do not have to do
  anything about it.** Add or replace your row and rebase; two branches
  appending, and replacing-your-own-row beside somebody else's append, both
  merge without a conflict. Measured 2026-08-16: 26 of 48 unmerged branches
  would not rebase onto main, 9 after. Two things are yours: **do not move or
  delete a `MERGE-REGION` marker line** (`ci/merge-docregions.py --check` runs
  in T0 and a lost marker is a T0 red, because the attribute would then promise
  a behaviour the file no longer declares), and know that **everything outside
  those regions still conflicts exactly as it did** — all three `## NOW` blocks
  included, in both their rewritten and their appended form. That is
  deliberate: a driver that kept both sides of a rewritten `## NOW` would be
  worse than the conflict, because the conflict is visible and the stale figure
  is not.
* **A `docs/DETERMINISM.md` conflict is now a hunk you can read, and if it is
  not, suspect your own branch — 2026-08-17.** `merge=appendonly` used to
  refuse that file *whole* the moment either side had touched existing text,
  which put every line of both sides between markers: **7,318 lines in one
  hunk on `clamped-floor-probe`, 51,298 on `worldgen-sulfur-r18`**, where
  git's own 3-way gives 167 and none of the branches disagreed with main
  about a single existing line. Five of that morning's six blocked worldgen
  branches were that and nothing else. The driver now falls back to git's
  line-level 3-way and refuses only hunks **both sides changed**. So a
  file-sized conflict there today means a commit of yours rewrote the file —
  the usual cause is a botched earlier resolution that duplicated the body,
  which is worth checking with `wc -l` before you try to resolve anything.
  `ci/merge-appendonly.py`'s header has the rules and its falsify script has
  them as cases.
* **Do not pick a `docs/DETERMINISM.md` or `docs/PARITY-ROADMAP.md` section
  number. Ask for one:**

      ci/claim-number.py --claim -d PARITY-ROADMAP -s my-finding -t "What I measured"

  It prints one number, reserves it atomically against every other worktree on
  this box, and writes the claim file with `status = "reserved"`. Write your
  `## N.` heading, set `status = "written"`, commit both together.
  `ci/claim-number.py --show` is who holds what, including reserved-but-unwritten.

  **This is not advice about tidiness — you cannot do it by hand correctly.**
  "The highest number I can see plus one" is how two branches both took 33 and
  a recovered draft took 54 on 2026-08-18, and asking the other lanes does not
  scale and is stale by the time you get an answer. The allocator also sees
  numbers held on **local branches that were never pushed** — that is not
  hypothetical, it is why §59 is 59 and not 58 — so a scan of `origin` is not
  a substitute.

  `ci/check-doc-claims.py`, in `ci/t0.sh`, is the detector behind it: it
  refuses a duplicate `(doc, number)` naming both claimants, a heading with no
  claim, a claim with no heading, and a `reserved` claim whose section now
  exists. It adjudicates **one tree**, so it cannot see a collision until both
  sides have merged — `ci/claim-number.py --check` is the one that sees across
  refs, and it is worth running before you gate.

  None of this applies to `docs/WORKLIST.md` — it is self-evolving, not
  append-only, and has no number to claim.

## Standing state that is not yours

* **Standing reds:** `carved_chunks_match_vanilla` and
  `surface_chunks_match_vanilla`. They are **not** in `ci/t0.sh`. Do not chase
  them — but if your change moves either count, that IS yours, and **since
  2026-08-05 you will not have to notice it yourself**: `ci/handoff.sh` stage 4
  runs T1 and `ci/t1-allowlist.py` adjudicates these two by **signature**
  against `ci/checks/t1-known-red.toml` — the count, the denominator, the y
  bands, the confusion table and the seed/chunk. A known red that gets worse,
  gets better, or changes its nature is as red as a third failure appearing.
  **They are one defect, not two.** Both print the same 106 of 7,077,888, the
  same two y bands (32..63: 54, 128..159: 52) and the same confusion table —
  `calcite -> dirt` 27, `calcite -> grass_block` 18, `sand -> dirt` 32,
  `sandstone -> stone` 22, `stone -> dirt` 5, `stone -> grass_block` 2 — because
  the carver stage runs the surface rule first and inherits its residual whole.
  So it is **106 blocks, reported twice**, and it is in the **surface rule**:
  seed 0, first example chunk (80,153). The surface test adds that the biome
  grid, the preliminary surface and the surface depth are each 0 wrong, so the
  divergence is in the rule tree, where calcite and sandstone should win and we
  fall through to the default. Verified 2026-07-30. Nothing in decoration can
  move it: both stages run before any feature does.
* **The End is closed at 100.000000%** over the city window: seed 20260722,
  centre chunk (83,62), r=7, **y 0..255**, denominator 14,745,600, vanilla
  first, against `oracle-work/van-end-20260722-c8362` **and** its `-b` twin.
  **If you touch `place_piece` or anything shared with `end_city`, re-run that
  window before handing off.** No gate in `ci/t0.sh` would catch a regression
  there.
* **A rotation bug worth generalising:** `rotate_state` once never turned the
  `axis` property (`RotatedPillarBlock.rotatePillar` swaps x/z under quarter
  turns). Forty blocks, invisible until something placed a rotated pillar for
  the *first time*, because no existing gate covered it. **Expect the same from
  the first structure that rotates a block class nothing else has rotated.**
  Still open and deliberately unfixed: `CrossCollisionBlock.rotate` cycles the
  four fence connection booleans and we do not implement it — a rotated village
  fence with a connection is silently mis-rotated today.

## For perf work only

* Prove every change **output-identical**: `cmp` region files against a pristine
  control from a `git archive` checkout of the parent commit, generated
  sequentially. Prove it at `RUSTMC_WORKERS=1` **and** at 12.
* Report **CPU-seconds alongside wall.** Wall alone on a 24-thread box says more
  about thread count than about the generator, and it is the blind spot that hid
  a 23%-of-cycles bug for weeks.
* **The question that keeps paying out: "how many times do we evaluate something
  vanilla evaluates once?"** Every parity test here compares *values*, none
  compare *call counts*, and a dropped cache marker is invisible to a value
  test. `flat_cache` is one of five markers (`cache_2d`, `cache_once`,
  `cache_all_in_cell`, `interpolated`). It has paid out at 10.3x once already.
* Instruments: `RUSTMC_STAGE_CPU=1`, `ci/stage-scale.sh`, `RUSTMC_THROUGH=<stage>`,
  `perf stat`, `perf record`. Profile AFTER fixing too — one agent's correct fix
  landed on the wrong tenth of the stage.

## Budget and handoff

**There is no token cap. Stop when the item is closed and gated, or when the
diagnosis is complete and the next move belongs to a different item.** Length
is handled for you: the session auto-compacts at 150k, so a long run is a
supported thing to do, not an overrun. Do not pad a run to look thorough and do
not rush a diagnosis to look cheap.

**What this costs you is memory, and that is the rule that matters:**
compaction is lossy and it will fire mid-item. Anything you are holding only in
your own context — a census total, the seed and bbox a number was taken at, a
candidate you already killed — can be gone on the other side of the boundary,
and a re-measured number that disagrees with a half-remembered one has burned
a turn twice now.

So **write findings down as you get them, not at the end**:
* Commit early and often, with the measured before/after, the seed, the window
  and the box in the message. The git log is the part of your memory that
  cannot be compacted away.
* Append to the `## NOW` block **as you go**. Treat it as the live notebook,
  not the exit report — in particular record nulls the moment you kill them.
* When you quote a number to yourself later, quote it from a commit message or
  from `## NOW`, never from recall.

Before you stop, rewrite your cold start at `docs/cold-start/<your-branch>.md`
(append a new `### COLD START — branch \`X\`, ...` entry rather than editing an
old one — the file is that branch's own append-only history now, same shape
as `docs/DETERMINISM.md`, just not claim-numbered): what you measured, what
you ported or fixed, **what you killed as a null — explicitly, so nobody re-runs
it** — and the single next item with the result that closes it. Add or update
your one row in WORKLIST's `COLD START INDEX` table so the next agent finds it.
Leave the tree clean and committed on your branch. **Do not merge or push to
main**; the orchestrator gates and merges.

**PUSH BEFORE YOU DETACH, AND VERIFY THE PUSH.** A commit that exists only in
your worktree is invisible to everything downstream: the merge chain fetches
`origin/<your branch>`, and a branch that is not there is not "late", it is
absent, and the chain either skips it or waits on it forever.

```bash
git push origin <your branch>
git ls-remote --heads origin <your branch>    # not optional — confirm it landed
```

**This is the single most expensive mistake made on this project.** On
2026-08-10 four agents in a row detached with committed, gate-clean,
fully-written-up work sitting unpushed in their worktrees — among it the
`IcebergFeature` port that took the seed-12345 window from 69,288 to 7,940. A
merge chain sat waiting forty minutes on a branch that had never existed on
origin, and the orchestrator had to walk every worktree by hand to find the
work. Nothing was lost, but nothing landed either, and none of the four agents
had done anything wrong except stop one command early.

Note the shape of it: **every one of those agents believed it had finished.**
The gate was green, the write-up was in `## NOW`, the commit message carried
the numbers. The push is the step that feels like bookkeeping and is actually
the delivery, so treat "did I push, and did I check?" as the last question of
every turn, including a turn you are ending early to wait on a background job.
If you are ending your turn with a background job still running, push what you
have **first** — you may not be the one who comes back for it.

**Then leave your worktree detached, holding no branch:**

```bash
git -C <your worktree> checkout --detach
```

**This is not tidiness — a held branch blocks the merge gate.**
`ci/handoff.sh` checks the branch out in the canonical checkout, and git
refuses a branch that is already checked out in another worktree of the same
repository:

```
fatal: 'structures-trial' is already used by worktree at '.../work-structures'
REFUSED  could not check out structures-trial
```

That cost three gate launches on 2026-08-06, and note what the refusal is: it
names your branch while saying nothing whatever about it, which is the exact
failure mode the gate exists to refuse.

**Detaching costs you nothing, and that is measured rather than assumed.**
`git checkout --detach` rewrites HEAD to the commit you are already on and
touches neither the index nor the working tree: your files, uncommitted and
untracked ones included, stay exactly where they are, staged things stay
staged, and your branch still points at your last commit. Nothing is lost and
nothing has to be re-done. `git worktree add --detach` from the start is
better still.

**The gate no longer depends on your remembering** — it releases a *clean*
worktree itself, says so in the log, and continues — but leaving it held still
costs somebody something in both directions. Leave it dirty and the gate
refuses loudly with the path and stops the queue until a human clears it,
because a branch you are mid-edit on is not a branch to gate and detaching a
tree an agent may still be live in would leave that agent committing onto a
detached HEAD without being told. Leave it clean and the gate detaches it
underneath you, correctly but without asking. Detaching it yourself is how
neither of those happens.

## Standing facts as of 2026-08-09 — moved here out of eight briefs

Everything below was being pasted into individual briefs. A pasted rule costs
~2k tokens per brief **and** plants itself in that agent's transcript, where it
is replayed on every later turn forever. New standing rules belong in this file.

**There is one machine now.** The Dell laptop's HDD failed 2026-08-07; the data
was recovered uncorrupted and restored onto the Threadripper. `threadripper` is
an SSH alias to **127.0.0.1** — this same box. Every command in this file works
unchanged, and the rsync-into-a-separate-dir step still buys the per-tree
isolation that the 2026-08-03 shared-`oracle-work` failure cost a gate. Read
"the laptop" anywhere above as "the working tree".

**The toolchain is Rust 1.96.1 and JDK 25 — not 21.** The pinned
`server-26.2.jar` is class-file version 69. Under JDK 21 every `ci/*-golden.sh`
dies at `javac` with `bad class file … wrong version 69.0, should be 65.0`, and
the symbol cascade that follows reads like a classpath bug. If goldengen cannot
see classes that are demonstrably in the jar, check `javac -version` before you
touch the classpath.

**`rsync -a` moves mtimes backwards and cargo then reuses the base's binary.**
This is not the freshness note above; it is worse, because it is silent. A tree
whose sources arrive *older* than release artefacts built minutes earlier will
run the previous binary and report it as your measurement. End every sync with
a `touch`, and before trusting any number confirm the binary contains your
change. It cost a real measurement on 2026-08-08, and it only surfaced because
that branch added *new* files — a branch editing only existing ones measures the
base in silence.

**`ci/handoff.sh` does rsync → generate → T0 in that order. Do not hand-roll
it.** The generators live in the branch's own tree so they cannot run before the
rsync; T0's unit tests read the tables they produce so they cannot run after T0.
Getting it wrong produces a red that looks like a code defect and is not — it
did so three times on 2026-08-08/09, twice to agents and once to the
orchestrator.

**`pgrep -f 'some pattern'` matches the shell carrying that pattern.** A wait
loop written as `until pgrep -f '...t2-gameplay...'` therefore watches itself
and never exits, and a "stray process" count that includes your own `pgrep` is
not a finding. Anchor it — `pgrep -f '[c]i/t2-gameplay'` — or match on a PID.
Two agents and the orchestrator each hit this on 2026-08-09.

**Do not queue a corpus run for a worldgen-only branch.** `ci/t2-gameplay.sh`
serialises on the corpus lock at ~35 minutes per run and is frequently three
deep. `ci/t0-remote.sh` needs no lock. A branch that touches neither
`ci/checks/` nor `crates/rustmc-server/` has `t0-remote` as its whole gate
condition.

**If you stop while blocked, do not re-arm a waiter you already have.** One
agent resumed four times and left four identical `until ssh … test -s` loops
polling the same file, each firing its own notification for one condition.
Check before arming, and poll the artefact rather than re-arming on resume.

**Commit and push at every checkpoint, for a budget reason as well as a
correctness one.** The weekly allowance has no overage cushion
(`disabled_reason: out_of_credits`) and stops without warning. Committed work
survives a hard stop; your context does not.
