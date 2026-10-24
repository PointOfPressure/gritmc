# Cold start — branch `corpus-unstable-adjudication`

Every `### COLD START` entry ever written for this branch, oldest and newest alike, moved out of `docs/WORKLIST.md` by the doc-collision-class restructure (1 entry). Order matches how they appeared there — not necessarily chronological, since WORKLIST inserted each new one above the last.

---

### COLD START — branch `corpus-unstable-adjudication`, off `main` (`eae922f6`), 2026-08-09

**The floor now says which comparator measured it, `540`'s silence was that
comparator rather than vanilla, and the four names that had never been
adjudicated are adjudicated.** Worktree `~/grit/work-unstable`, remote dir
`~/rustmc-unstable`. Not merged — the orchestrator gates. Full account in
`docs/PARITY-ROADMAP.md` §15; `docs/PARITY.md` §4.1 is the published figure.

**WHAT CLOSED.**

* **The floor carries `projection`.** `oracle-runner projection-id [--explain]`
  → `proj:<16 hex>` over the comment-stripped, test-stripped text of
  `compare.rs`, `scenario.rs`, `trace.rs`, embedded with `include_str!` so it
  describes the binary that ran. Stamped by `ci/corpus-floor.sh`; `update`
  refuses a measurement without one; `adjudicate` prints `NO PROJECTION` /
  `STALE PROJECTION`; the gate annotation moved into
  `ci/corpus-floor-report.py` and prints `STALE PROJECTION` + `NOT QUOTABLE`
  beside the score. `ci/corpus-floor-falsify.sh` cases 16-21, six injections,
  and case 19 is the "stamp removed" falsifier. The stamp fired on this
  branch's own later commit: `proj:7299b27f954e3585` → `proj:58380c9e4cbeae81`.
* **The four unadjudicated names are declared**, all four `reference`, all four
  re-derived by `adjudicate` from the floor measurement rather than from a
  pairwise verdict count: `a-player-that-leaves-the-ground-is-resynced`,
  `closing-a-crafting-table-returns-the-grid-contents`,
  `container-click-throw-drops-one-item`, `-drops-the-whole-stack`.
  `gameplay-unstable.toml` is 9 unstable + 0 harness and `adjudicate` exits 0.
* **`540`'s silence — mechanism found and the harness half repaired.**
  `compare::place` remembered where `530` announced the actor with `add_entity`
  (r5, scenario 63 of 88, anchor `[-2048,-60,0]`, `kind_id 156` =
  `minecraft:player`, rel `[0,0,77]`); `540` is scenario 64 at
  `[-2080,-60,0]` and `within` is 16. In the two runs where vanilla sent the
  outbound walk as a delta rather than a forced sync, nothing carried a
  position, both deltas were bounded out, and the projection came out empty.
  **A player is no longer placed.** Six recordings all speak; the residual is
  vanilla's own tick ordering, so `540` is `[[unstable]]`, not `[[harness]]`.

**THE NUMBERS.** `ci/corpus-floor.sh`, ensemble `twin-homog-20260809`, six
recordings, 15 pairs, corpus 88, Threadripper, 2026-08-09.

```
  before  9/88 = 8 reference + 1 silent    540 silent, silent_runs [r5, r6]
  after   9/88 = 9 reference + 0 silent    540 reference, 8/15, 2 distinct
```

**NULLS — DO NOT RE-RUN THESE.**

* **The floor does NOT fall from 9 to 7.** Measured under the post-throw
  projection over `twin-homog-20260809`: 9/88, same nine names, the two
  container-throw scenarios at 14/15 and 15/15 either way. The `y_rot`
  exemption reads a field `trace::decode` names only since the branch that
  added the exemption, and those six recordings were taken that morning before
  it — so the exemption is **inert over this ensemble**. Re-record the
  ensemble; do not re-measure hoping for 7.
* **The exemption itself is not too narrow**, measured separately on the only
  two recordings on the box that carry `y_rot` (`~/rustmc-gate` 13:18,
  `~/rustmc-throw` 11:48): their `add_entity` for those two scenarios differs
  in `y_rot` alone (23 vs 20; -99 vs -12) and the floor over that pair is
  **4/88 with neither name in it**. N = 2, `p_hide 77.6%` — a positive control
  on the exemption, not a floor.
* **`HARNESS REPAIRED` cannot print for `540` and that is not a gap.** It fires
  for a name declared in `[[harness]]` whose flaps reach zero. `540` was never
  declared, and after the repair it still flaps 8/15 because what remains is
  real.
* **`recorder_digest`'s fallback was tree-relative — fixed.** The identical six
  recordings gave `recorders = 1` measured from `~/rustmc-twin` and "6 distinct
  recorders, NO HOMOGENEITY CONTROL" measured from `~/rustmc-unstable`.

**THE SINGLE NEXT ITEM: the floor's ensemble has no recorder identity, and a
projection identity does not substitute for one.** The same comparator over
recordings from two decoders is two instruments — measured here in its sharpest
form, a field the projection acts on being absent from the recordings entirely.
`twin-homog-20260809`'s decoder emits 91 `(kind, field)` pairs; the current one
emits the same 91 plus `add_entity.{y_rot, x_rot, y_head_rot, data}`. It closes
when the floor file records the ensemble's **vocabulary** (that sorted union)
and `ci/corpus-floor-report.py` warns, naming the pairs, when the gate's own
`vanilla.json` carries one the ensemble never produced. Direction matters and
is measured: strict equality over-fires, because `r1` alone lacks
`take_item_entity.*`. `docs/PARITY-ROADMAP.md` §15.4.
