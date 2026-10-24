# Cold start — branch `step-look-timing`

Every `### COLD START` entry ever written for this branch, oldest and newest alike, moved out of `docs/WORKLIST.md` by the doc-collision-class restructure (1 entry). Order matches how they appeared there — not necessarily chronological, since WORKLIST inserted each new one above the last.

---

### COLD START — branch `step-look-timing`, off `main` (`8e283f57`), 2026-08-09

*(Additive. The `corpus-unstable-adjudication` cold start below belongs to
another agent and was not touched.)*

**`Step::Look`'s 60 ms was a copied constant, it is measured now, and it is
250 ms.** Worktree `~/grit/work-look`, remote dir `~/rustmc-look`. Not merged.
Full account in `docs/PARITY-ROADMAP.md` §16; §12's open entry is retired and
§13's item 2 is closed.

**WHAT CLOSED.**

* **Measured, over recordings that already existed.** Nine full-corpus vanilla
  recordings on the box, all nine recorded at `look = 60`, so the ensemble is
  one world for this constant. The reference's answer to a look arrives
  **32..192 ms** after it and **45 of 71 arrivals (63%) landed outside the
  60 ms window**. Six of eight cells flip step between runs; two are late in
  nine runs out of nine. `p_hide(9) = 28.3%`.
* **250 ms is the mechanism's bound, not a number borrowed from `hold`.**
  `handleMovePlayer` defers to the tick via
  `PacketUtils.ensureRunningOnSameThread` (1 tick); `ServerEntity.sendChanges`
  gates `$Rot` **and** `RotateHead` behind
  `tickCount % updateInterval == 0` with `EntityType.PLAYER` at
  `updateInterval(2)` (2 ticks); `RotateHead` trails `$Rot` by one more gated
  interval clearing its packed-degree deadzone (2 ticks). Five ticks = 250 ms,
  and the measured 192 ms worst case sits inside it.
* **`ci/step-window-audit.py`** — the actual deliverable, because the repair
  moves no score. Two halves: attribution stability across an ensemble
  (model-free) and arrival latency against the window (needs the window
  arithmetic, catches what stability cannot). `--cohort-from` reads each
  recording tree's own `client.rs` and **refuses** an ensemble spanning a
  change to the audited constant. What counts as an answer is a bytecode
  citation in `ANSWER`, never inferred from the recordings.

**NULLS — DO NOT RE-RUN.**

* **There is no look-following divergence in the corpus and there can be
  none.** All five look-bearing scenarios declare `observe_from = "actor"`,
  and a pure rotation reaches the actor not at all —
  `ChunkMap$TrackedEntity.updatePlayer` returns early on
  `player == this.entity`. `strict` keeps `observe_from` too. **§12's
  prediction that this would surface as a divergence after a look is wrong**;
  the fragility is latent, not imminent, and becomes live the first time a
  look-bearing scenario observes the observer (seven scenarios already
  declare `observe_from = "observer"`).
* **Do not re-record to measure a step window.** Nine recordings were already
  on disk; the measurement is seconds of Python. The five-hour figure is
  `ci/t2-gameplay-selfcheck.sh`'s and does not apply.
* **`bed-places-two-blocks` and `repeater-faces-away-from-the-player` are not
  broken.** Their looks are to yaw 0.0 and produce no packet by design. They
  are the mechanism-live control, not a gap.
* **Section A of the audit undercounting is known and deliberate** — per-cell,
  not per-ordinal, so one missing packet demotes a whole cell. Lower bound by
  construction; do not "fix" it by guessing ordinal alignment.

**THE SINGLE NEXT ITEM — a floor taken across a `client.rs` edit is silently
unstamped.** `PROJECTION_SOURCES` is `compare.rs`, `scenario.rs`, `trace.rs`
and **not `client.rs`**, which is where every step window lives. So two
recordings taken either side of a step-window change carry the *same*
projection id; `ci/corpus-floor.sh` groups them as one ensemble; step
attributions that differ because the *recorder* moved are counted as the
reference disagreeing with itself; the floor comes out inflated and every
marker the stamp exists to print (`STALE PROJECTION`, `NOT QUOTABLE`) stays
silent, because by the stamp's own test nothing moved. The stamp answers "same
comparator?" and cannot answer "same recorder?", and both can invalidate a
floor.

Live on the box now: three of the nine recordings are `hold = 60` and six are
`hold = 250`, all nine carrying one projection id. The registered `9 of 88`
is **not** affected — it declares `recorders = "1"` and is homogeneous by
construction. The hazard is for the next floor.

Two candidate shapes, and the choice is real: (1) add `client.rs` and
`record.rs` to `PROJECTION_SOURCES` — one line, but it invalidates every
registered projection id at once; (2) a separate `recorder-id`, stamped
beside the projection id — more code, no ledger churn, and it keeps two
questions apart that invalidate different things. §13's item 3 (`recorder=`
in the recording stamp) is the same hazard from the other end and goes with
whichever lands.

**Deliberately not bundled here, for §12's own reason** — two harness changes
on one corpus run cannot be told apart, and this branch's whole claim is that
one constant moved and nothing else did. It closes on a branch that runs a
corpus with nothing else in it. Full write-up: `docs/PARITY-ROADMAP.md` §16,
"FINDING — the projection id cannot see the recorder".
