# Cold start — branch `corpus-falsifiability-audit`

---

### COLD START — branch `corpus-falsifiability-audit` off `f82b2ebb`, 2026-08-18

**The corpus was never asked the question it asks of everything else.** Remote
dir `~/rustmc-corpfals`, working artefacts in `~/corpfals-work` (deliberately
NOT in the remote dir — see the hazard at the bottom). Not merged; the
orchestrator gates.

`ci/checks/gameplay-falsifiable.toml` reads **108 of 108** and has for some
time. A ledger that reads 108 of 108 is either a finished job or an instrument
that cannot report, and it is neither: it is an instrument answering a
different question from the one its name implies, and saying so in its own
module docstring — *"axis A finds no live defect in the corpus today. Its
value is the ratchet."* `--sweep` mutates a **recording**; every recording has
bytes at every step, so mutating them moves the verdict of very nearly
anything.

## What shipped

* **`crates/rustmc-server/src/sabotage.rs`** — eight subsystems replaced with
  the not-yet-written versions they grew out of, each the server's own no-op
  answer rather than an injected value. Armed **by workspace**: `record.rs`
  pins scenario *n* to absolute `x = -(n + 1) * 32`, so that pinning is an
  address and one corpus run carries as many independent controls as there are
  scenarios. Behind the `corpus-sabotage` cargo feature, off by default,
  asserted off by `ci/t0.sh` in two arms.
* **`ci/corpus-controls.py`** — chooses each workspace's controls from the
  check's own recorded projection, not from its prose. All four consequence
  controls everywhere, plus one input control per scenario.
* **`ci/corpus-grades.py`** — the four-way reading, plus two second readings:
  `ungraded_properties` (a block-state property no control could move) and
  `unexercised_controls` (a subsystem in the box that nothing runs, reported
  even when the check GRADES).
* **`ci/corpus-positive-control.sh`** — one lock, one server, no vanilla boot.
* **`ci/corpus-grades-falsify.sh`** — three checks whose verdict is known in
  advance, recorded twice against a real server on a private port.
* **`ci/corpus-history.py`** — the 64 recordings already on this box.

## The numbers that needed no corpus run

**64 recordings, reference held fixed, subject swapped across every build:**

```
moved under the reading the gate acts on   41
moved ONLY under the ungated strict lens    6
frozen across every build                  61
```

All six strict-only movers moved on **`sound`**, which
`gameplay-sound-coverage` holds and which none of the six names in its
`observe` list. That is a correct scoping decision, so the bucket is real and
not actionable here — but it is six checks whose gated reading has never once
moved across 64 real builds, and that is worth knowing before trusting them.

**Frozen is a screen, not a verdict.** A check can be frozen because this
server has been right about it since before the oldest recording on the box.

## Findings in the ledger itself

* **`dead_fields` could not fire.** `--check` compared `[meta] dead_fields`
  against `len(dead_field)`, both written by the same `write_ledger` call.
  Equal by construction. The selftest had noticed and worked around it rather
  than reporting it — its case built a failure *"by lowering the recorded
  ceiling under the recorded population"*, which tests a hand-edit and not a
  sweep. Pinned in the script now, where `MAX_UNSWEPT` already was.
* **`field_universe` had no consumer at all.** §27's *"4 of 84"* is quoted
  against it and the ledger says **90**.
* **12 of 102 recorded `(kind, field)` pairs could not be asked about.** Axis B
  enumerates the SUBJECT's fields, so a pair the reference records and we never
  send has nothing to perturb: `set_entity_motion`, `set_equipment`,
  `take_item_entity`, `update_attributes`. Now `unaskable_field`, ratcheted
  upward only.
* **`write_ledger` still deleted the `MERGE-REGION` marker** on every run.

## `observe_from` defaults to `both`, and three instruments assumed `actor`

`tools/oracle-runner/src/scenario.rs` declares `From` with `#[default] Both`.
Six scenarios leave it unstated and every one watches both connections;
`080-other-player-is-an-entity` reads `move_entity`, which only ever arrives on
the observer, so under an `actor` default its projection would be empty.

`ci/step-window-audit.py` is where it cost a number: it computes
`in_from = frm == "both" or frm == who` to decide an unstable cell's blast
radius, so every observer-side cell in those six printed as *"latent (outside
both projections)"* — a fragility declared unable to fail — when it is inside
both. `ci/unfalsifiable-falsify.py` hardcoded it in the function whose
docstring says getting it wrong cost a run.

## The positive-control recording is queued, and the branch does not need it

The corpus lock queue refilled faster than it drained — 93 minutes of waiting
across three different holders — so the run is still parked. Nothing above
depends on it: the classification ledger carries the cheap reading and gates on
it, and the instrument is demonstrated end to end by
`ci/corpus-grades-falsify.sh` on real recordings.

When it lands, `~/corpfals-work/finish-pc1.sh` reads it (the frozen driver
holding the lock predates the fix that tolerates a non-zero `record`, and a
sabotaged server makes that the ordinary outcome). Then `--write --grades`
folds the verdicts in — **and `GRADES_FLOOR` has to be pinned in the same
commit**, because `--check` refuses a ledger carrying verdicts while the floor
is `None`. That refusal is the instrument working.

## Hazards for whoever picks this up

* **Do not put run artefacts in the remote gate directory.**
  `ci/t0-remote.sh` rsyncs the working tree into `~/$RUSTMC_REMOTE_DIR` with
  `--delete`. A run parked in `~/rustmc-corpfals` lost its recording, its clean
  half and its server log to a gate run in another worktree, an hour after the
  fact and with nothing pointing at the cause. `ci/corpus-positive-control.sh`
  refuses such an `--out` now.
* **Do not edit a shell script while it is running.** Bash reads a script
  incrementally; inserting lines shifts the offsets under a live interpreter.
  The driver is copied to an untracked frozen name before launch.
* **And do not edit a Python gate while a long harness is running either** —
  the failure is different and harder to read. `ci/unfalsifiable-falsify.py`
  runs `ci/corpus-falsifiable.py` as a subprocess a dozen times over twenty
  minutes, so a run that straddles an edit sweeps its baseline ledger with the
  old code and gates it with the new. That came back as two *control* arms
  failing — "the unimplanted corpus passes at that same ceiling" — which reads
  exactly like a regression in the thing under test. It was not: a ledger swept
  by the current code and checked by the current code is green. Finish the edit,
  then run.
* **A readiness probe must stay on one line.** `ci/liveness-proofs.py`
  discovers a proof by finding the probe and the loop exit on the SAME line.
  Splitting this one across a continuation for readability dropped the count
  from 6 proofs to 5 with no refusal — a wait loop that had quietly opted out
  of the check that asks whether it can say "not ready".
* **A subset rehearsal needs a bigger `--radius`.** Nine scenarios run far
  faster than 108, and the world is generated as players walk into it, so the
  far workspaces were still void when the recorder got there. The full corpus
  does not have this problem because its own `settle_ms` waits give the tiles
  time.
