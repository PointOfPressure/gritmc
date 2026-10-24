# COLD START — `ci-t2-emits-a-red-line`

### COLD START — branch `ci-t2-emits-a-red-line`, 2026-08-18, area "gate throughput"

**One sentence: T2 is 66% of every gate and had no string for its own failure,
so for the project's whole history it looked like a stage that had never gone
red.**

**How it was found, which is the transferable part.** `ci/gate-rate.py --cost`
put T2 at **143 GREEN and 0 RED** over 229 gate logs. That is rule 7 — when a
measurement clears every candidate, suspect the measurement — and it was the
measurement. `ci/t2-gameplay.sh` contains no `T2-GAMEPLAY RED` string at all:
it prints GREEN on success, REFUSED when it declines to measure, and on a
genuine corpus regression the comparison prints `  REGRESSED <scenario>` and
`set -e` ends the run **saying neither**.

**Nothing ever merged that should not have.** `ci/handoff.sh` reads the exit
status (`T2_RC`, line 773), so the gate verdict was always correct. What was
broken is every reader of the *log*:

* `ci/gate-rate.py` scored four such gates UNJUDGED. Three are real T2
  failures — `chain-cascade-signs` twice and `gate-skips-and-scenario-verbs`,
  all `t2=1/0`. Fixed on `ci-gate-batching` by also reading handoff's own
  `RED <branch> — …` summary; all-history per-log p moved 0.7937 → 0.7744;
* **`docs/AGENT-BRIEF.md`'s recommended waiter matched
  `T2-GAMEPLAY (GREEN|REFUSED)` and so hung forever through exactly this
  failure** — in the same block that warns that a waiter firing only on green
  is silent through a crash. Measured: on those four logs the old pattern
  matches **zero** lines. Fixed on `docs-waiter-t2-blindspot`;
* a human reading the tail of the log saw no verdict at all.

This branch is the repair at the source, so the next reader does not need a
fourth workaround.

**What the falsifier gates, and why it is not "does the line appear".** The
sentinel is additive — one line from the existing EXIT trap on an
already-failing exit. The risk is entirely the other way: a sentinel that fired
on a *green* run would turn every gate red. So `ci/t2-verdict-falsify.sh`
(16 assertions, 2 injections, ~5 s, in T0) asserts silence on exit 0 (GREEN and
`SUBSET OK`) and exit 4 (REFUSED, which already has its own string), a RED line
on 1/2/3/5/99, and then runs the **real script** twice: a preflight refusal must
still exit 4 and must not *also* print RED, and a non-refusal death must now say
so. It also asserts the old waiter pattern still would not have matched the
second log — which is what makes the case about the defect rather than about the
fix — and that no GREEN line appears, so handoff's `grep -c` pass count is
untouched.

**Part 1 extracts the sentinel from the real file with `sed` rather than
retyping it**, so a later edit cannot leave the cases testing a stale copy.

**NO CORPUS LOCK IS TAKEN.** Both injections kill the script before
`corpus_lock_acquire` (`ci/t2-gameplay.sh` ~line 288), which is why this is a
T0 falsifier and not a bench.

**WHAT I KILLED AS A NULL.** "T2 has never failed" — it has, at least three
times, and the belief was an artefact of the classifier. Do not quote a
zero-T2-failure figure from any log-derived instrument predating today.

**The single next item.** `ci/handoff.sh` still does not print a
`T2-GAMEPLAY RED` line of its own, and its error-extract grep (line 1044) lists
`T2-GAMEPLAY REFUSED` but not RED, so a T2 regression's most useful lines are
not surfaced in the chain's summary. That is a one-line addition to that list —
**left undone deliberately, because `gate-stage-timing` also edits
`ci/handoff.sh` and is the pinned branch**; do it after that lands, not beside
it.
