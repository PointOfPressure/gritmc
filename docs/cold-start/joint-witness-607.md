# Cold start — branch `joint-witness-607`

Every `### COLD START` entry ever written for this branch, oldest and newest alike, moved out of `docs/WORKLIST.md` by the doc-collision-class restructure (1 entry). Order matches how they appeared there — not necessarily chronological, since WORKLIST inserted each new one above the last.

---

### COLD START — branch `joint-witness-607`, off `main` (`7ccad044`), 2026-08-10

**Two items, both "a check that cannot fail". One is closed; one is diagnosed
and belongs to whoever owns the corpus.** Worktree `~/grit/work-joint607`,
remote dir `~/rustmc-joint`. Not merged — the orchestrator gates.
Full account in `docs/DETERMINISM.md` §26.

**WHAT CLOSED — the `Session` table joint.**

`Session`'s six optional tables (`placement`, `shapes`, `sound_types`,
`ticks`, `placed_by`, `recipes`) had six construction sites: five tests that
passed `None` to all six, and `status.rs`, which sets all six. **0 of 6 arms
were covered by anything in `ci/t0.sh`; the only witness was a ~50-minute
corpus run behind a contended lock.** Now 6 of 6, plus 6 negative controls,
in 3.3 s. Not six untested mechanisms — every callee had passing module tests
throughout. One untested joint, six times over.

* `crates/rustmc-server/src/session_tables.rs` is the ONE place the six are
  loaded. `status::listen` and `tests/session_joint.rs` both call
  `SessionTables::load`, differing in a single `root` argument, so there is no
  second construction for a fixture to drift from. `status.rs` loses 132 lines.
* `shapes::shared_with_classes` takes a `root` — the hardcoded relative path
  meant only code running from the repo root could reach the table.
* `ci/t0.sh` names `--test session_joint` explicitly. **`--lib --bins` cannot
  run anything under `tests/`**, so without that line the whole item would have
  been the defect it was written to close. The first T0 after the test landed
  came back GREEN without ever building the binary.

**THE POISON.** Committed controls cut one `Session` field and assert their own
arm dead and the other five alive. Independently, each of the six consumers was
broken at source: all six fire with the signature `cutting_<arm>` passes / the
other five fail naming that arm.

```
  state_for_placement -> default     placement.rs    FIRES
  writes_for -> &[]                  placedby.rs     FIRES
  SoundTypes::row -> None            sound_types.rs  FIRES
  ScheduledTicks::schedule -> false  ticks.rs        FIRES
  Recipes::find -> None              recipes.rs      FIRES
  ShapeTable::update_shape -> None   shapes.rs       FIRES
  typed_update_shape -> None         shapes.rs       INERT — all 7 green
  level_update_shape -> None         shapes.rs       INERT — all 7 green
```

The two inert ones are the finding: `set_block`'s neighbour pass tries three
lookups in order and an unsupported torch is answered by the **first**. A run
that stopped at the first poison would have concluded the arm survives its
poison. Rule 7, fifteenth payout.

**WHAT IS DIAGNOSED AND IS NOT MINE — scenario `607`.**

`607` is **not vacuous; it is comparing two silences**, and the distinction
decides the fix. Read off `~/rustmc-water/oracle-work/gameplay/`, mtime
2026-08-10 21:49:21, 88 scenarios: `reference_obs: 7`, `subject_obs: 7`, so the
comparator's `vacuous` verdict (empty reference) cannot see it. The container
opens and the click is driven. **Step 6, the only step exercising
`pickup_all`, contributes zero on both sides** — its one packet is
`packet:0x60`, which `607` does not `observe` — and the step-6 cursor digest is
byte-identical to step 5's, decoding to one stone. **Vanilla does not gather
here.** The scenario's premise is false and it will stay green whatever
`click.rs` does.

Ranked fix, and **implementing `pickup_all` is NOT it** — with the scenario as
written an implemented arm still observes zero, because the packet carrying the
answer is not in the `observe` list:

1. the scenario needs rewriting by the corpus owner, to reach a state where
   vanilla actually sweeps and to observe `packet:0x60`;
2. the comparator wants a third verdict on the predicate **"did the measured
   step say anything?"** — `vacuous` asks it of the whole scenario and `inert`
   subtracts only acks and `Place` resyncs, so neither can see this.

**THE SWEEP.** Passing with zero observations over the whole scenario: **0**
(by construction — `verdict_for` returns `Vacuous` on an empty reference).
Passing while the **measured step** contributes zero on both sides: **5** —
`561`, `571`, `581`, `607`, `608`. Stable across four independent runs. Four
are negative checks by design ("changes nothing", "is ignored", "is
swallowed") and are weak but not wrong. **`607` is the only positive assertion
in the set and the only one that is broken.**

**NULLS — DO NOT RE-RUN THESE.**

* **Do not poison `typed_update_shape` or `level_update_shape` to test the
  shapes arm.** Both are inert for an unsupported torch; `update_shape` is the
  one that answers.
* **Do not build a `Session` fixture with `items: None`.** `send_reply` numbers
  every stack through the item registry, so a good crafting result goes out as
  an empty stack and a recipe assertion is red for an unrelated reason.
* **Do not assert a place sound on the actor's stream.** It excludes the
  placer; a one-connection harness asserts on a silence it caused itself.
* **Do not take a corpus verdict for the `607` question.** Everything above is
  from recordings already on the box; no run was queued and no lock taken.
* **Do not add `--tests` to `ci/t0.sh`.** It drags in the golden-vector gates
  and turns the two standing reds into a red T0. Name the test.

**NEXT ITEM.** The comparator's third verdict — `Match` where the *measured
step's* contribution is zero on both sides. It closes `607` and puts a floor
under the other four. It is done when the 88-scenario report declares 5 under
the new verdict and `604` (`SWAP`, equally unimplemented, correctly diverging
6/4) still declares `diverged`.
