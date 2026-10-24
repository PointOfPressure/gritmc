# COLD START — branch `ci-handoff-terminal-states`, off `origin/main` (`cf443588`), 2026-08-18

**`ci/handoff.sh` is the only route to `main`, it is 1,067 lines, and it had
never been owned as an area.** Its record is 41 GREEN / 32 RED. Until this
morning the chain had never once named a RED — 30 of the 32 arrived as "no
verdict line" and 2 as another branch's `MERGED`. That was repaired on
`ci-gate-stage-falsifiability`, where the verdicts are READ. This branch is the
other half: whether the gate **emits** an unambiguous verdict on every path it
can take.

Worktree `~/grit/work-handoff`, remote dir `~/rustmc-handoff`. **No corpus lock
was taken and no box work was done** — every test here drives the real
`ci/handoff.sh` in a throwaway repository against a stubbed box and a private
gate-lock directory.

## THE ONE-LINE ANSWER

It did not. Twenty-one terminal states, of which **eight had ever been executed
by any test**, and the four that can leave `main` and `origin/main` disagreeing
had been executed by none — because `ci/handoff-verdict-selftest.sh` drives all
22 of its scenarios with `--dry-run`, and `--dry-run` returns at the `GREEN`
line, one statement above `release_worktree main`.

There are now 31 declared terminals, 25 of them forced by a named control, and
a T0 check that fails if a new one is added without saying what it prints, what
it leaves behind, and what forces it.

## THE TERMINAL-STATE TABLE

Declared in `ci/checks/handoff-terminals.toml` and enforced by
`ci/check-handoff-terminals.py`, which **derives the printed word from
`ci/handoff.sh` itself** rather than reading it out of the declaration. That
detail is the whole design: a declaration that merely restated the source would
have agreed with `MERGED-NOT-PUSHED`.

| state | word | exit | leaves behind | forced by |
|---|---|---:|---|---|
| `$ROOT` unreadable | REFUSED | 2 | nothing | — unreachable |
| no branch given | REFUSED | 2 | nothing | state:`no branch given` |
| gate dir is the canonical checkout | REFUSED | 2 | nothing | state:`gate dir is ~/rustmc` |
| `ci/gate-lock.sh` missing | REFUSED | 2 | nothing | state:`gate-lock.sh missing` |
| lock not acquired (4 shapes) | REFUSED | 2 | nothing | gate-lock-falsify |
| `CARGO_BUILD_JOBS` invalid | REFUSED | 2 | nothing | state:`job cap not a number` |
| `RUSTMC_REFERENCE_CACHE` invalid | REFUSED | 2 | nothing | state:`reference cache is not 0/1` |
| `RUSTMC_PUSH_RETRY_GAPS` invalid | REFUSED | 2 | nothing | state:`push retry gaps not 0` |
| gate log unwritable | INFRA | 4 | nothing | state:`unwritable gate log` |
| no such branch | REFUSED | 2 | nothing | state:`no such branch` |
| working tree dirty | REFUSED | 2 | nothing | state:`working tree dirty` |
| dirty because of a nested worktree | REFUSED | 2 | nothing | state:`nested worktree is named` |
| local main behind origin/main, held elsewhere | NOT-FF | 3 | nothing | — reachable, no fixture |
| local main behind, would not fast-forward | REFUSED | 2 | nothing | — unreachable in that branch |
| main/origin diverged, or unpushed and unpushable | UNPUSHED | 5 | **main ahead of origin/main** | tail:`3b`, tail:`divergence is named` |
| 0 ahead but not an ancestor | REFUSED | 2 | nothing | — unreachable, an assertion |
| already an ancestor of main | ALREADY-MERGED | 0 | nothing | state:`already merged` |
| behind main | NOT-FF | 3 | nothing | state:`behind main` |
| branch held by a worktree | REFUSED | 2 | nothing | state:`dirty holder refuses` |
| branch checkout returned non-zero | REFUSED | 2 | nothing | — git exits 0 instead |
| branch checkout incomplete | REFUSED | 2 | nothing | state:`checkout exits 0 and lies` |
| a stage could not run (6 causes) | INFRA | 4 | nothing | verdict-selftest ×5, tail:`log eaten` |
| lock anchor gone before the merge | REFUSED | 2 | nothing | gate-lock-falsify |
| log lost this run's header | INFRA | 4 | nothing merged | tail:`log eaten mid-gate` |
| a stage was red | RED | 1 | nothing; ROOT on the branch | verdict-selftest ×6 |
| green, `--dry-run` | GREEN | 0 | nothing | verdict-selftest:`everything green` |
| main held by a dirty worktree | REFUSED | 2 | nothing merged | tail:`main held by a dirty worktree` |
| main checkout returned non-zero | REFUSED | 2 | nothing merged | — git exits 0 instead |
| main checkout incomplete | REFUSED | 2 | nothing merged | tail:`checkout of main exits 0 and lies` |
| main moved under the gate | NOT-FF | 3 | nothing merged; ROOT on main | tail:`main moved: names the cause` |
| merged, pushed | MERGED | 0 | main == origin/main | tail:`green merges` |

## THE PARTIAL-FAILURE PATHS, AND WHAT EACH LEAVES BEHIND

These are the four the brief named, and none of them had any evidence.

**Merged but not pushed.** The gate printed `MERGED-NOT-PUSHED  push failed —
main is ahead locally` and exited **2**. Three defects in one line:
`grep -E '^MERGED'` matches it, so the state meaning "the queue is about to
wedge" was namable as the state meaning "everything worked"; the first line of
output was not that line at all but git's `fatal: Could not read from remote
repository.`; and exit 2 is documented four times in that file's own header as
"the gate could not run", when it had run all six stages, passed, and merged.

It now emits `UNPUSHED` on exit **5**, the push is retried three times, and —
the half that matters for the queue — the disagreement is **detected at entry**
before `BEHIND` is computed. `BEHIND` is measured against LOCAL `main`, which is
correct for a fast-forward and is exactly why an unpushed local `main` is
indistinguishable down there from a branch that needs rebasing. On 2026-08-16
one timed-out push presented as 37 branches needing a rebase that would not have
helped. The entry check repairs it where it can, and the argument for pushing
during another branch's gate is that **`git push origin main` sends all of
`main` regardless** — the next green gate pushes those commits anyway. It
changes *when* they leave, never *what* leaves.

Reproduced end to end in two gate runs: arm 1 wedges the repo, arm 2 gets
`REFUSED  next-branch is 1 behind main — not a fast-forward, rebase it`, exit 3.

**Killed mid-merge.** SIGKILL between `git merge --ff-only` and `git push` runs
no trap and prints nothing at all — `main` merged, origin behind, no verdict of
any kind. Only the next run can notice, and the next run said `REFUSED  0
commits ahead of main — nothing to merge`, which is true, useless, and the wrong
instruction. Now `ALREADY-MERGED` exit 0, with origin caught up on the way past
by the entry check. Forced by making the push HANG rather than fail (a `git`
shim that sleeps on `push`), which is both aimable and the more faithful
reproduction — 2026-08-16 was a timeout, not a refusal.

**Pushed but not recorded.** `cc-budget` ran between the successful push and the
`MERGED` line. It is bookkeeping, and a hang or a kill there produced: merge
done, origin has it, everything worked, and the caller sees no verdict and a
non-zero exit — so the queue re-gates a branch that already landed. It now runs
after the verdict.

**Main moved under a green gate.** `REFUSED  ff-merge failed after a green gate`,
exit 3. The entry check had already proved `main` was an ancestor, so this can
only mean main moved *during* the gate — a fact about the queue — while
`REFUSED` is this file's word for state that stopped the gate before it started,
and a full gate had been paid for. Now `NOT-FF` naming how far main moved.
Forced deterministically by moving `main` from inside the fake ssh on the
provenance census, which is the last thing that happens before the merge.

## THE LIVE HOUR, WHICH IS BETTER EVIDENCE THAN ANY SELFTEST PRODUCED

Between 09:12 and 10:14 on 2026-08-18, while this branch was being written, the
chain produced **113 RED verdicts across 40 branches** — every one naming a
parity failure that had not happened. `ci/t0-remote.sh` rsyncs `--exclude .git`
and a T0 check had started needing a repository, so T0 exited 1 in the gate copy
for a reason that had nothing to do with any branch. Fixed by
`gatefix-gitless-checks`. Three things in it belong to this area.

**The verdict line reported a made-up number in two of five fields.** Every one
of the 113 read `t0=1/0 t1=99/0 t2=99/0`, and `99` is this script's own sentinel
for a stage skipped because an earlier one was red. Nothing said so. T1 and T2
were named as having failed with status 99 on 40 branches whose parity was never
measured. A skipped stage now says `skipped`, and the line leads with the first
stage that actually failed — the one fact a reader wants, which previously had
to be derived by scanning five fields for the leftmost non-zero, 113 times.

**A wrong verdict is no longer one misread branch.** `chain-keeper.sh` now
stands down when recent verdicts are uniformly red and probes every tenth cycle,
so it reasons about RUNS of verdicts. Anything ambiguous between RED and INFRA
can now stall the whole chain rather than cost one branch, which raises the
value of every distinction in this branch and lowers the tolerance for a
verdict that overstates what failed.

**And the chain could not name four of the eight words** — found on rebasing.
`gate-chain.sh` still grepped for `MERGED-NOT-PUSHED`, which no longer exists,
and named neither `ALREADY-MERGED`, `NOT-FF` nor `UNPUSHED`. All three would
have logged as `no verdict line at all`, the exact string 30 of the 32 REDs
arrived as — reintroduced by the branch fixing it, with `NOT-FF` being the
commonest refusal in a 67-branch queue. It is not in the repository, which is
why no gate had ever looked at it; it is now a declared `out_of_tree_consumer`,
checked when present.

**One refusal was correct and its diagnosis was not.** A branch was refused with
`working tree dirty — an agent may still be live` because a scratch git worktree
had been created inside the gate's own checkout. The condition was right, the
cause was two directories away, and "commit or stash" does not apply to a nested
repository. Same family as the 861 refusals of 2026-08-17: the right verdict,
naming a condition rather than a cause, so nobody could act on it. It now shows
what is dirty and names a nested worktree as the different thing it is.

## THE THREE OTHER FINDINGS, EACH WORSE THAN THE ONE IT WAS FOUND UNDER

**A gate log that cannot be written certified a green on no evidence.**
`mkdir -p "$LOGDIR"` was unchecked. Every sentinel counter is `grep -c` over
`$LOG`; against a MISSING file `grep -c` leaves the variable EMPTY, every
`[ "" -eq 0 ]` in the `||` chain exits 2, the `if` reads the whole condition as
false, and the gate **falls through to the merge with no assertion having
fired**. `docs/CI.md` carried this as a KNOWN-OPEN with the qualifier "narrow,
because a persistently unwritable log fails the stage redirects first". It is
not narrow: the truncated case reddens a healthy branch instead, which is how it
was found. Closed both ways, and the second assertion is about **identity, not
existence** — the provenance census re-creates a deleted log, so the gate checks
that the log still contains the header *this run* wrote.

**`git checkout` reports a failure and exits 0.** Measured, not assumed: with a
directory the process cannot write into it prints `error: unable to create file
...`, prints `Switched to branch ...`, and returns **0**. So
`git checkout -q "$BRANCH" || REFUSED` could not fire in the failure it most
plausibly guards against, and the gate would rsync a tree missing a file the
branch adds and measure content that is in no commit. Replaced by a
post-condition on both checkouts: HEAD is at the target *and* the tree is clean.

**`ci/lanes.sh` could not name `MERGED`.** Its classifier was
`grep -m1 -E '^(RED|REFUSED|INFRA|GREEN)'` — four of the eight words — so a
successful merge driven through that path recorded `no verdict line`, the exact
string 30 of the 32 REDs arrived as. The checker now reads that line and fails
T0 if a verdict exists it cannot name.

## WHAT COULD NOT BE FORCED — four, and `max_unforced` is a ratchet

* **`cd "$ROOT" || exit`** — `$ROOT` is the parent of the running script's own
  directory. A bash that resolved and executed the script can cd into it. It was
  also the one *silent* terminal in the file and now prints a word regardless:
  an unreachable state that speaks costs two lines, and an unreachable state
  that is silent is indistinguishable from a reachable one that is.
* **0 ahead but not an ancestor** — `AHEAD` is `git rev-list --count
  main..$BRANCH`, which is 0 **if and only if** `$BRANCH` is an ancestor of
  `main`. The two conditions cannot both hold. It is kept as an assertion,
  because an `AHEAD` of 0 arrived at any other way means the arithmetic did not
  happen.
* **the two `git checkout` non-zero guards** — git exits 0 on the failures they
  could catch, so nothing can drive them. Their real work is done by the two
  post-conditions, and those are forced.
* **the two terminals of the behind-origin repair** — a local `main` that is
  behind `origin/main` AND checked out in a worktree this script may not move,
  and a fast-forward of a strict ancestor that fails anyway. The first is
  reachable and has no fixture; the falsifier drives the `$ROOT`-holds-main
  path, which is the one the chain runs in.

`max_unforced` is a **named list** and not only a ceiling, checked as a set, so
adding to it is as loud as removing. It went 5 -> 4 as fixtures landed and 4 -> 6
when the behind-origin repair added two terminals of its own; the forced
proportion went 21/25 to 25/31 over the same span. A bare count cannot tell a
repair that added terminals from two controls that rotted away.

## A GENERAL RULE THAT DID NOT BIND, WRITTEN DOWN SO IT IS NOT RE-DERIVED

`ci/unlanded.sh` establishes that a branch must be sized with `git cherry` and
never `git rev-list --count`, because this repo merges by rebase — 780 naive
against a real 453 over 218 branches. `unpushed-on-branch-at-entry` was wrong
that way and is converted.

**`+N commits` on the `MERGED` line may stay a sha count**, and the proof is one
line above it: `git cherry main $BRANCH` marks a commit `-` only when its
patch-id also appears in `$BRANCH..main`, and the gate refuses unless `BEHIND`
is 0 — so that range is empty and the two instruments are provably equal. This
was implemented the other way first and the falsifier case built to force a
disagreement **could not force one**.

## ONE THING THAT WAS NOT TESTED, AND WHAT WAS DONE INSTEAD

**No real gate was run.** Six recordings were queued and the brief asked for no
corpus-lock time, so nothing here has been driven against the box, cargo, or a
real T2. Every test drives the real `ci/handoff.sh` against a stubbed box.

That is the honest boundary of this branch's evidence, and it is the same
boundary `ci/handoff-verdict-selftest.sh` already states: this file learns what
a stage did from exactly two things, its exit status and what it wrote to the
log, so a stand-in producing the real status and the real sentence is
indistinguishable to it. **What none of it can prove is that a real stage
produces those statuses.**

What was done instead is a smoke run against the real checkout on the paths
that touch nothing — every one terminates before the first ssh:

    $ ./ci/handoff.sh --dry-run somebranch
    REFUSED  '--dry-run' looks like a flag, not a branch — the branch comes first
    $ ./ci/handoff.sh somebranch --dryrun
    REFUSED  unrecognised argument '--dryrun'
    $ ./ci/handoff.sh definitely-no-such-branch-xyz
    REFUSED  no such branch: definitely-no-such-branch-xyz
    $ ./ci/handoff.sh somebranch --dry-run --force
    REFUSED  unrecognised argument '--force'
    $ RUSTMC_REFERENCE_CACHE=yes ./ci/handoff.sh somebranch
    REFUSED  RUSTMC_REFERENCE_CACHE='yes' is not 0 or 1

The first, second and fourth of those merged to main before this branch.

**The first real gate to run this should be watched for one thing specifically:**
the entry check fetches `origin/main` and, when local `main` is strictly behind,
fast-forwards it. That is the only line here that writes to the caller's tree on
a path a normal gate takes. It is safe by construction — local `main` is a
strict ancestor, so nothing can be lost — and it refuses rather than writes when
`main` is checked out in a worktree this script may not move.

## WHAT RUNS IT

All five in T0, +25 s (166 s -> ~191 s; T0 is 4% of a gate, so ~0.8% of one).
None of them touches the box, a real host, cargo, or the corpus lock.

    ci/check-handoff-terminals.py           0.1 s   the declaration
    ci/check-handoff-terminals-falsify.py   1.5 s   10 injections, one per rule
    ci/handoff-state-falsify.sh             5.4 s   16 refusals before the box
    ci/handoff-verdict-selftest.sh          9.7 s   22 stage outcomes
    ci/handoff-merge-tail-falsify.sh        8.0 s   30 assertions below the verdict

The merge-tail file cost 64 s until `RUSTMC_PUSH_RETRY_GAPS=0` landed; 56 s of
it was `sleep`. That knob changes the LENGTH of the gaps and cannot change the
NUMBER of attempts, which is the property those cases assert.
