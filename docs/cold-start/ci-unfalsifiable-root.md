# Cold start — branch `ci-unfalsifiable-root`

---

### COLD START — branch `ci-unfalsifiable-root` off `a106314d`, 2026-08-13

**The class has a name, two gates and a number now.** *An instrument that
measures its own input, or a verdict whose green cannot be turned red.* Remote
dir `~/rustmc-falsify` on the Threadripper. Not merged — the orchestrator
gates.

**The headline number, which did not exist before this branch:** 39 of 129
distinct gameplay scenarios have never once been observed non-match, across
all 49 `oracle-work/gameplay/report.json` on the box. That is the *historical*
reading and it is weak. The experimental one — mutate the subject side of a
recorded pair and see whose outcome moves — is **106/106 tier 2**, 0 tier 1,
0 tier 0, in 95 s with no server, no vanilla boot and no corpus lock.

**What shipped**

* `ci/corpus-falsifiable.py` — `--sweep` (needs a trace pair), `--check` (T0,
  needs nothing), `--selftest`. Axis A grades every scenario; axis B finds
  `(kind, field)` pairs no check reads. Evidence in
  `ci/checks/gameplay-falsifiable.toml`, ratcheted six ways.
* `ci/liveness-proofs.py` — discovers every wait loop in `ci/*.sh`, requires a
  registered log a **not-ready** subject produces, and runs the real predicate
  against it through `bash`. Registry `ci/checks/liveness-proofs.toml`.
* `ci/unfalsifiable-falsify.py` — three reconstructed shipped defects,
  red-then-green, against the real comparator and real recordings.
* `ci/t0.sh` runs both selftests then both gates. `docs/AGENT-BRIEF.md` gains
  a HARD RULE at the very top. `docs/PARITY-ROADMAP.md` §27.

**Live findings**

* **4 of 90 `(kind, field)` pairs are read by no check**:
  `move_entity_rot.{entity,rest}`, `rotate_head.{entity,rest}` — **nothing in
  this corpus scores which way an entity is facing.** Three more are
  `undetermined` (`packet:0x00.rest`, `set_entity_data.{entity,rest}`): their
  carriers already diverge, so a change to them need not have shown. The
  `window`-counter / `ACCEPT_TELEPORTATION` shape, live today. Ratcheted, not
  refused. The figure was 10 before the detector was sharpened and all six
  that moved were this instrument's false positives.
* **2 of 5 readiness proofs reject a not-ready log.** The three that do not
  are one defect at three call sites, all waiting on `listening on`:
  `ci/t2-gameplay.sh`, `ci/t2-gameplay-subset.sh`, `ci/block-entity-subset.sh`.
  `crates/rustmc-server/src/main.rs` prints it at 162 and calls
  `status::listen` at 170. **Not fixed here — owned elsewhere**, pinned by
  name under `MAX_UNSOUND`.

**Nulls — do not re-run these**

* **"Some mutation reddened it" is not a measurement.** It returns 106/106
  because `drop-all` moves almost any verdict to `absent` and `speak` does the
  same in the other direction. Only *edit* mutations of the measured step
  count. Do not simplify the tiers away.
* **Perturbing every observation of a field is not a liveness probe.** It is a
  uniform additive offset, which `compare::rebase_session_counters` correctly
  discards, so `seq` and `window` read as dead when they are read. Perturbing
  only the last is blind in the other direction (aggregates built before the
  filter — the entity position table). 11 dead under `"all"`, 32 under
  `"last"`, and the two lists are not nested. Both are needed.
* **Nor is a verdict-and-counts signature a change detector.** A scenario at
  `diverge` stays at `diverge` with its counts unchanged, so a field read only
  by diverging scenarios looked dead. `first_difference` is in the signature
  now, and where a change still need not show the answer is `undetermined`,
  not `dead`. Do not fold `undetermined` back into `dead`, and do not ratchet
  it — it rises when the server regresses elsewhere.
* **`604-container-click-swap` is not a corpus defect.** It graded tier 1 on
  the first tiered run; its measured step holds eight rows and none is
  projected (`block_event` collapsed, `move_entity_heartbeat` dropped),
  `measured_subject_obs = 0`. That was the guard's own hole. Both fixed:
  `speak` triggers on an empty *projection*, and a check already red at the
  step it names is tier 2 outright.
* **Axis A will not catch a `listening on`-shaped defect and it is not meant
  to.** Within the comparator, a scenario that names an `observe` list and
  appears in the recording is very hard to make unfalsifiable, because
  noticing changed subject output is the comparator's job. That defect lives
  in shell and is `ci/liveness-proofs.py`'s subject. Do not try to fold the
  two scripts together.

**The single next item, and the result that closes it**

**Kill the 4 dead fields, or explain each one.** Each is a field this harness
records and decodes and no check scores. For each: either widen some
scenario's `observe` list until `ci/corpus-falsifiable.py --sweep` drops it
from the dead list, or write down why it is deliberately unscored (as
`block_changed_ack.seq` turned out to be — a session counter the comparator
rebases on purpose). Closes when `dead_fields` in
`ci/checks/gameplay-falsifiable.toml` reaches 0 or every survivor carries a
recorded reason. All four are rotation: `move_entity_rot` and `rotate_head`.
Nothing in this corpus scores which way an entity faces, and
`CrossCollisionBlock.rotate` is already a known-unimplemented rotation defect
(`docs/AGENT-BRIEF.md`, standing state) — a check that scored head yaw would
be the first instrument able to see that class at all.

**Second item, not mine:** the three `listening on` sites. The repair is
demonstrated both directions in `ci/liveness-proofs.py`'s selftest — require
the marker **and** the absence of `^listen: `, or probe the port instead of
the log. Drop `MAX_UNSOUND` as each is fixed.
