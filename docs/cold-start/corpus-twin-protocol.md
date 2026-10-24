# Cold start — branch `corpus-twin-protocol`

Every `### COLD START` entry ever written for this branch, oldest and newest alike, moved out of `docs/WORKLIST.md` by the doc-collision-class restructure (1 entry). Order matches how they appeared there — not necessarily chronological, since WORKLIST inserted each new one above the last.

---

### COLD START — branch `corpus-twin-protocol`, off `main` (`cd521254`), 2026-08-09

**ORACLE rule 6 now exists in the gameplay corpus. The floor is 9 of 88 at
N = 6, one of the nine is ours, and it was measured twice on two ensembles that
share nothing.** Worktree `~/grit/work-twin`, remote dir `~/rustmc-twin`. Not
merged — the orchestrator gates. Full account in `docs/PARITY-ROADMAP.md` §13;
`docs/PARITY.md` §4.1 is the published figure.

**THE NUMBERS.** `ci/corpus-floor.sh`, vanilla 26.2 protocol 776, superflat,
seed 20260803, Threadripper, 2026-08-09.

*Published* — `twin-homog-20260809`, six cold boots of one tree with one
recorder (`SELFCHECK_RUNS=6 ci/t2-gameplay-selfcheck.sh`, ~1h50):

```
CORPUS FLOOR  9/88 scenarios do not reproduce between two vanilla runs
              8 reference (vanilla's) + 1 silent (OURS); 79 reproduced
              N = 6 recordings, 15 pairwise comparisons, p_hide 39.3% at alpha 0.05
              HOMOGENEOUS: one recorder took all six
  KNIFE-EDGE  none — 040's tightest boundary is 217 ms, over four ticks
  cross-check OK: `compare` and `project` agree on all 88
```

*Control* — `nightly6-20260809`, six recordings from six trees and three
recorders, already on the box, corpus 86: **the same nine names, the same
classes.** Rates differ (`370` and `611` are 12/15 there and 8/15 here), which
is the honest size of what N = 6 supports.

The same six recordings through `ci/t2-gameplay-selfcheck.sh`'s own
aggregation: `6 runs, declared 8, measured 8, 79/88 reproduced, 1 did not and
are ours`, strict 77/88, **SELFCHECK GREEN**.

**Three things it settled that were hand-work before.**

1. All seven entries already in `ci/checks/gameplay-unstable.toml` reproduce in
   both ensembles. `605`/`606` raised from 2/2 at N = 3 to **14/15 and 15/15 at
   N = 6**.
2. `611-closing-a-crafting-table-returns-the-grid-contents` promoted at
   **8/15** — its final `container_set_slot` is state 6, 7 or 8 with every
   other observation byte-identical, which is `incrementStateId` counting
   packets rather than changes.
3. **`540-a-player-that-leaves-the-ground-is-resynced` is OURS, and §11 was
   about to file it as vanilla's.** Four of six runs are byte-identical and two
   produced nothing at all — in *both* ensembles, with a different pair silent
   each time, so it is neither a decoder difference nor a property of one tree.
   §11 held off only because N = 3 could not support a rate; nothing available
   to it could have supplied the *reading*, which needs the projections rather
   than the verdicts. Now a `[[harness]]` entry — a repair order that goes red
   when all six speak.
4. **`040`'s knife edge is gone and the instrument said so unprompted.** 35 ms
   with the 60 ms `Step::Hold`, 217-253 ms with the 250 ms one. The control
   ensemble straddles that repair, which is what forced the margin to be
   measured inside one recorder rather than across an ensemble.

**KILLED AS NULLS, explicitly, so nobody re-runs them:**

* **"Two recordings agreeing means the scenario is stable."** No. At N = 2 a
  scenario flapping up to 77.6% of the time still shows two identical runs more
  often than one time in twenty. The arithmetic is in `ci/corpus-floor.py`'s
  header; do not re-derive it.
* **"Raise N until scenarios can be declared stable."** Not affordable. The
  smallest rate this corpus has measured is 1 in 26 and resolving it needs
  **N = 77**, about fifteen hours of vanilla boots. The protocol reports
  `p_hide` instead and that is the answer, not a stopgap.
* **"Count all 15 pairs as 15 samples."** No. The power exponent is
  `runs - 1`, never the pair count; 15 pairs from 6 runs are not 15 independent
  samples and using them would make every absence look resolved.
* **"The boundary-blind lens tells you whose an instability is."** Only in one
  direction. `boundary` is proof it is ours; `reference` proves only that it is
  not the boundary class. `540` is the counter-example and is why the `silent`
  class exists.
* **A floor does not need `ci/t2-gameplay-selfcheck.sh`.** The control ensemble
  cost nothing — six recordings that were already on the box, compared in
  seconds — and `ci/corpus-floor-falsify.sh` builds its own synthetic ensemble
  when none exist at all. The published one *did* use the selfcheck, at
  `SELFCHECK_RUNS=6`, and that is worth ~1h50 when the recorder needs
  controlling; it is not the price of asking the question.
* **"The heterogeneous ensemble was good enough."** Nearly. It got the set
  right and would have got the *margin* wrong by a factor of seven, because
  four of its recordings predate `599aed4b` and two follow it. If you assemble
  an ensemble by hand, read the recorder digests the run prints.

**THE SINGLE NEXT ITEM, and the result that closes it.** `540`'s silence.
Two of six runs record nothing, in both ensembles, with a different pair silent
each time — about one in three, on our side of the wire, mechanism unknown.
Nobody has looked at why the observer's stream carries no packet about the
actor's walk. It closes when an N = 6 ensemble has all six speak:
`ci/corpus-floor.py adjudicate` then prints `HARNESS REPAIRED` and stays red
until the `[[harness]]` entry is deleted. Until then `540`'s own `diverge`
against this server is one coin toss away from `vacuous`, which is a check that
cannot be trusted to fail.

Second, and cheaper: **`Step::Look` still returns 60 ms** and §12 names it as
the identical defect to the one that cost a run on `040`. The knife-edge set is
empty today and is adjudicated in both directions, so the instrument will say
so before a branch finds out the other way — but 250 ms is a one-line change
and the measurement to justify it already exists.

Third, for whoever is running a corpus anyway: **`ci/t2-gameplay.sh`'s
`assert_recording` should stamp `recorder=`** the way
`ci/t2-gameplay-selfcheck.sh` now does. Without it, a `vanilla.json` from
another tree is grouped by hashing that tree's sources *now*, so two recordings
from one tree at two different commits are grouped as one recorder. The
fallback is labelled `tree-now:` so nothing is silently wrong, and one added
`printf` closes it — but it changes a file the corpus reads, so it costs a
fifty-minute gate that a branch already running one gets for free.
