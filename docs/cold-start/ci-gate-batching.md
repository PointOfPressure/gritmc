# COLD START — branch `ci-gate-batching`

### COLD START — branch `ci-gate-batching`, 2026-08-17

**Area: gate throughput. Built the batching mode that did not exist.** Full
design, every measured figure and the enable checklist are in
`docs/GATE-BATCHING.md`; this file is only what a successor needs to not repeat
me.

**What landed.** `ci/batch.sh` (N green branches → one integration tip → ONE
gate → one fast-forward), `ci/batch-arith.py` (the expected-gate arithmetic,
`--selftest`), `ci/batch-falsify.sh` (26 cases, 14 injections, 40/40, ~80 s).
Nothing is enabled: `--land` is off by default and `chain-keeper.sh` is
untouched.

**The number.** Width **3**, and on RED the batch dissolves back into the serial
queue rather than halving. Driver-aware `--pack` over the 36 live GREEN branches:
13 groups, 1 collision, **30.64 gates against 36 serial — 1.175x, 7.1 h saved**
at the measured p. Queue-order grouping instead of packing gives 8 collisions and
only 5.2 h.

**READ THIS BEFORE YOU USE ANY OF THE ABOVE. p IS NOT 0.81 AND IT IS NOT A
CONSTANT.** `ci/gate-rate.py` (written for this) classifies every log
`ci/handoff.sh` has ever written by handoff's own stage sentinels:

    all history  p = 0.7911  (N=158)   ->  batch, width 3
    since 08-12  p = 0.5890  (N= 73)   ->  DO NOT BATCH
    since 08-14  p = 0.5000  (N= 56)   ->  DO NOT BATCH
    since 08-17  p = 0.9167  (N= 12)   ->  batch, but N below 20 is not a rate

`ci/batch-arith.py --break-even`: **below p = 0.6934 no width beats serial.** The
all-history rate corroborates the briefed 0.81; the trailing three-day rate is on
the other side of the line, where batching makes the queue SLOWER. The plausible
reading is that today's six tree-only preflight checks now remove, in seconds, the
failure class that used to be found 16 s into an 80-minute gate slot — 11 of
today's 12 gates were green, which is the right shape and the wrong N.

**So the closing condition for enabling this is a measurement, not an opinion:**
`ci/gate-rate.py --since <first post-preflight day>` at N >= 20 and p >= 0.70.
Do not skip it. A wrong p does not make the plan slightly wrong, it makes it the
wrong plan.

The easy way to measure p is also wrong and I did it first: grepping the
orchestrator's `*.log` for handoff's verdict lines gives 48 green of 80, p = 0.60,
because those logs exist only where somebody redirected a gate they were watching
and people watch gates they expect to fail. **You tee what you investigate.**

**What I killed as a null — do not re-run these.**

* **A 13-wide batch. It is WORSE THAN SERIAL** (1.2498 vs 1.2346 gates per
  landed branch). It passes 6.5% of the time at p=0.81. The brief for this work
  proposed 13 wide / 34 commits; that figure counted only collision feasibility
  and treated a batch as landing whenever its members do not textually conflict.
  Feasibility is an upper bound on width, not a throughput figure.
* **The brief's ~12 h and ~17 h savings.** Measured, the honest saving on the
  real queue today is **8.8 h** with packing and 6.4 h without. Same reason.
* **Bisecting a failed batch.** Halving on RED is worse than dissolving to serial
  at every width above 2 at p=0.81. It is implemented (`--on-red halves`) because
  it starts paying above p≈0.94, but it is not the default and should not be made
  one without re-running `ci/batch-arith.py --crossover`.
* **Condemning the last member of a failed batch by elimination.** Cheaper by
  0.0513 gates per landed branch and **false** for an interaction red. Priced in
  the tool as `[unsound]`; do not implement it.
* **Packing on raw file disjointness.** Removes every collision and destroys the
  batching: 4 groups of 3 and **22 singletons**. Driver-managed paths must be
  subtracted, and "has a driver" must be `git config --get merge.<name>.driver`,
  not "the attribute is set".
* **Landing by pushing `origin/main` alone.** There is no `git fetch` in
  `ci/handoff.sh`, `chain.sh` or `chain-keeper.sh`, so local `main` would go
  stale and every later gate would mis-measure BEHIND and then fail to push after
  80 minutes. Landing must advance the local ref too, under a mutex.
* **`git worktree prune` to clean up after ourselves.** Repository-wide, no path
  filter, 212 worktrees belonging to other agents. `git worktree remove` already
  cleans up its own entry.

**Two traps that cost me real time and will cost you the same.**

1. **`GROUPS` is a bash special array** holding the current user's group ids, and
   assignments to it have no effect. `GROUPS=()` and `GROUPS+=(...)` were
   silently ignored while `"${GROUPS[@]}"` still expanded to the real group list,
   so the script threw its batches away and ran `run_group 1000`. The only
   symptom was `MSHA[$name]: unbound variable` from inside `build_tip`, which
   reads like a resolution bug. The variable is `BATCH_GROUPS` now.
2. **`perl -0pi -e 's{\Q...\E}{...}'` interpolates inside `\Q...\E`.** Ten of
   fourteen injection patches were dead on their first run because `$held`
   expanded to nothing; escaping every `$` then collided with the `!` and `{}`
   delimiters. Patterns now travel as plain strings in the environment and are
   `quotemeta`d at run time. Always run `BATCH_FALSIFY_DRYRUN=1` after touching
   `ci/batch.sh`, and **diff each patch** — two of them matched the wrong text
   and reported PATCH-OK, which is worse than dead.

Also: `bash -x` output interleaves unreliably with the script's own stdout, and
that sent me hunting a function that looked like it returned early when it had
not. Reproduce outside the harness before believing a trace's ordering.

Also killed: **counting a killed gate log as RED.** 67 of the 225 gate logs
reached no stage verdict at all — interrupted, ssh died, refused for local state.
Counting them as failures gives p = 0.556 and measures how often this project's
agents get interrupted, not how often a branch is good. `ci/gate-rate.py` reports
them as UNJUDGED and keeps them out of the denominator, with a selftest control.

**THE SECOND USE, AND IT MAY BE THE BIGGER ONE.** `--check` refuses
UNREGISTERED, so a new gameplay scenario cannot pass the tree checks until it is
in a recording, and a recording is ~40 minutes of the box-wide corpus lock. **So
the corpus lock is on the critical path for ADDING a scenario, not only for
gating one** — two agents were stuck exactly there this afternoon, one for 64
minutes. A recording covers whatever scenarios the tree it ran against holds, so
one recording on an integration tip registers every member's at once.
`ci/sweep-tip.sh` does it for a tree (useful on its own, for an agent whose
branch will not pass `--check`); `ci/batch.sh --sweep` runs it on the tip before
the gate; `--sweep-back` grafts each member's own rows back onto its own branch
via `ci/ledger-graft.py` so a RED batch does not waste the recording. **This half
is not conditional on p** — a recording is not a verdict.

Two things about it that are load-bearing rather than polish:

* **`ci/handoff.sh:772` runs the recorder as `RUSTMC_CORPUS_LOCK_ROLE=gate`**,
  which jumps the whole queue because it is the only thing that can move main. A
  measurement that declares `gate` competes with production merges as an equal
  and one did today, costing a production gate 25 minutes. `ci/sweep-tip.sh`
  declares `role=run` **and** refuses to start at all while a merge gate holds or
  waits the lock in another tree.
* **Do not raise `MAX_UNSWEPT`.** It is the deliberately bounded escape hatch and
  that file says growing it is a reviewed code change, not something an agent
  does while adding the check the allowance would hide. Fewer, bigger recordings
  is the other way out and it is the one that does not weaken a guard.

**AND ON TODAY'S QUEUE THE RECORDING SAVING IS ZERO — measure before you promise
it.** `ci/pack-census.py --branches-from preflight.tsv --scenario-branches-only`:
11 branches bring 60 scenarios, ALL 55 pairs collide, so they pack into 11 groups
and batching saves 11 -> 11 recordings. Two files cause all 55, and

    exempting ci/checks/gameplay-falsifiable.toml alone ->  0 pairs freed
    exempting ci/checks/gameplay-baseline.toml    alone ->  0 pairs freed
    exempting BOTH                                      -> 39 freed, 11 -> 5 groups,
                                                           6 recordings, ~4.0 h

**Neither alone is worth anything; the pair is worth four hours.** Every
scenario-adding branch writes a row to both, so removing one leaves the other
holding every pair. Do not "fix the ledger" and expect a win — that is the single
change that measurably buys zero, and it would read as a fix that did not work.

Follow-up worth someone's time, and it is BOTH files: give
`ci/checks/gameplay-falsifiable.toml` and `ci/checks/gameplay-baseline.toml` a
keyed-row merge driver. Both are keyed (`file = "010-break-block.toml"` is unique
per entry), the shape `ci/merge-docregions.py` already resolves. Not done here
because `ci-merge-docregions-registry-tomls` is a queued branch already working in
that registry, and two branches editing it is the collision this work is about.

**Three more traps, all found by the falsifier and all silent:**

3. **A variable declared below the argument loop resets the flag that parsed.**
   `SWEEP=""` sat in the middle of the file, so `--sweep` parsed, appeared in
   `--help`, and never ran. Five cases caught it; nothing else would have.
4. **The tip was read before the sweep, so a swept batch gated one commit and
   would have landed an earlier one.** Landing a commit the gate did not measure
   is the single thing this script exists not to do. The tip is now read after
   every step that can add a commit, and there is an explicit refusal if it moves
   between the gate and the land.
5. **An injection anchor that matches twice patches the wrong one and reports
   PATCH-OK.** Adding `sweep_back` introduced a second `if [ -n "$held" ]` above
   land_tip's, and `land_ignores_main_checkout` silently started breaking the
   wrong guard while its case stayed green. `subst` now refuses an anchor with
   more than one match, so that failure is a MISSED line instead of a lie.

**The single next item.** `ci/handoff.sh` takes the main-writer mutex around its
merge step (lines 1059-1062), keyed on
`$(git rev-parse --git-common-dir)/rustmc-main-writer.lock` — one line either
side of the ff-merge. That is the one change that makes `--land` safe to use at
all, and it closes with `ci/batch-falsify.sh` still 40/40 plus one live width-2
batch landing two branches whose loss would cost nothing. The deeper version,
which subsumes it, is handoff comparing against `origin/main` after a fetch
instead of against local `main`.

### COLD START — branch `ci-gate-batching`, 2026-08-18, area "gate throughput"

**The enable gate's p criterion is MET.** It was the one unsatisfied item on
this branch's own checklist (§6 item 5), and it was unsatisfied because
`ci/gate-rate.py` was measuring two things it should not have been. Re-measured
over 229 logs; `./ci/gate-rate.py --decide` now prints all six definitions:

    all history, per-log                          0.7744  N=164  MET
    all history, per-branch                       0.9037  N=135  MET
    since 15 Aug, per-log                         0.5641  N=39   not met
    since 15 Aug, per-branch                      0.6667  N=33   not met
    since 15 Aug, per-log, incidents excluded     0.7857  N=28   MET
    since 15 Aug, per-branch, incidents excluded  0.8800  N=25   MET

**Two instrument corrections, both of which had been pushing p down.**

1. **p is a per-BRANCH rate and the script counted logs.** A red branch stays
   in the queue and is gated again with the same defect; a green branch merges
   and is never gated again. Retries are drawn almost entirely from the failing
   population, so the per-log rate is biased downward *by construction*, not by
   noise.
2. **A main-side outage is not a verdict about a branch.** 14 of the 17 REDs
   since 15 Aug fall in one 27-minute window on 16 Aug across 12 branches.

**CORRECTS THE BRIEFED ACCOUNT OF THAT WINDOW, and the correction strengthens
it.** The brief said twelve branches all died on `FIGURES RED — 37 failing
rule(s)`. They did not: there are **four** distinct signatures —
`a gameplay check carries no evidence it can go red` (7),
`a published figure and its artefact disagree` (4),
`a doc claim collided, or drifted from its heading` (2),
`T1-GATE RED — 2 problem(s)` (1). One branch-side defect cannot make four
signatures across twelve branches in 27 minutes; four simultaneous main-side
breakages can.

**The exclusion rule is built so it cannot launder a real failure**, which is
the only thing that makes a p-raising correction worth having. Threshold counts
DISTINCT BRANCHES (so one branch retried 8x is not excused, case I3), and a RED
joins an incident only if its own signature is carried by ≥2 distinct branches
(case I4). That second guard is not hypothetical:
`server-power-through-set-block` failed `T1-GATE RED` twice inside the window on
a signature nobody else carried; the first version of the rule excused it and
reported 0.9167, the shipped one does not and reports 0.7857.

**A THIRD INSTRUMENT FIX, FOUND LAST AND WORTH THE MOST.** T2 appeared to have
gone GREEN 143 times and RED never — in the stage that is 66% of the gate.
That is rule 7, and it was the measurement: `ci/t2-gameplay.sh` prints no
`T2-GAMEPLAY RED` string at all (a corpus regression just exits non-zero, and
`ci/handoff.sh` records it only in its own `RED <branch> — …` summary), so the
three per-stage sentinels the classifier was built from are **blind to a T2
failure**. Four logs were misclassified UNJUDGED and three are real T2
failures: `chain-cascade-signs` twice and `gate-skips-and-scenario-verbs`, all
`t2=1/0`. The classifier now also reads handoff's summary line — the one place
a verdict is stated regardless of which stage produced it. All-history per-log
p moves **0.7937/N=160 -> 0.7744/N=164**, and every verdict above survives.

**THE BIGGER FINDING, AND IT MOVES THE ANSWER: a gate is not one unit.**
`gate_cost()` charged 1.0 for every gate whatever it did. Measured:

    GREEN  n=127  65.2 min       RED  n=33  3.4 min
                                   T0 RED  n=28   0.2 min   (twelve seconds)
                                   T1 RED  n= 5  21.3 min
                                   T2 RED  3 known, unstamped logs, not timed

A failure costs **0.053** of a success, because T0's tree-only checks run first
and `preflight.sh` catches most of that class before a slot is taken. Priced in
minutes (`--cost-green 65.2 --cost-red 3.4`), **the optimum moves from width 3 /
`batch_singles` / 1.151x to width 8 / `bisect` / 1.943x** — 26.1 h against
serial's 50.7 h on the 46-branch queue. The *strategy* changes because bisect's
extra gates are precisely the ones that come out red. Robust: a 10%-per-member
premium gives w=8 / 1.736x, and the most pessimistic p gives w=4 / 1.564x.

Defaults are 1.0/1.0, so §2.1's published 1.0596 / 1.2641 / 1.175x are all
still reproduced to the digit (selftest C1), and C3 proves the knob moves the
answer *back* when a red is made expensive — a cost model that can only ever
help batching would be measuring nothing.

**WHAT I KILLED AS A NULL — do not re-run these.**
* **Wall-clock times from the refcache both-ways probe are not a saving and
  never were.** The hit passes clocked 5283s and 5190s against the miss's
  4875s. That probe ran beside the live chain, so wall measures the corpus-lock
  queue. Use `wall − (the log's own "ACQUIRED after waiting Ns")`.
* **Do not look for a seventh merge driver.** Swept all 1035 pairs of the 46
  queued branches; 19 pairs conflict in an undriven file, spread over 20 files,
  best candidate 5 pairs against the docregions pair's 39. See
  `.gitattributes`, which now carries the measurement and the reopening
  condition.

**The single next item is unchanged and is still §6 item 1** — the main-writer
mutex around `ci/handoff.sh`'s merge step. Item 5 is now closed; items 1-4 are
not, and `--land` stays off. **What §2.3 adds to that decision is that the
prize is larger than the checklist assumed**: ~25 h on the current queue rather
than ~6.5 h, at width 8 with `bisect` rather than width 3 with `batch_singles`.
Anyone acting on §6 should re-read §2.3 first, because item 3 ("one measured
live batch at width 2") is still the right first move but the target width it
is a rehearsal for has changed.
