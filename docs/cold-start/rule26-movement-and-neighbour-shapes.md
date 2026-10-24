# Cold start — branch `rule26-movement-and-neighbour-shapes`

Every `### COLD START` entry ever written for this branch, oldest and newest alike, moved out of `docs/WORKLIST.md` by the doc-collision-class restructure (1 entry). Order matches how they appeared there — not necessarily chronological, since WORKLIST inserted each new one above the last.

---

### COLD START — branch `rule26-movement-and-neighbour-shapes` off `15901ea`, 2026-08-05

**Complete and gated, not merged — the orchestrator merges.** `T0 GREEN`,
`grep -c "skipping: no vanilla"` = **0**, on the committed tree. Corpus run 3
`T2-GAMEPLAY GREEN`, declared **35/63** (24 diverge, 4 absent, 0 vacuous, 0
inert, 0 error) with `530` locked at `match`.

Remote dir `~/rustmc-chain-rule26` on the **Threadripper**. Logs: on the
**Threadripper** `~/rustmc-logs/t2-rule26-run{1,2,3}.log`,
`~/rustmc-logs/selfcheck-rule26-n26.log`,
`~/rustmc-logs/recompare-rule26.log`; on the **laptop**
`~/rustmc-logs/t0-rule26-final.log`. The 26 corpus-63 vanilla recordings are
at `~/rustmc-chain-rule26/oracle-work/gameplay-selfcheck` on the
**Threadripper** and are reusable for any projection change.

#### What is measured and settled

* **Corpus runs 1 and 2, both `T2-GAMEPLAY GREEN`, identical:** declared
  **35/63 match, 24 diverge, 4 absent, 0 vacuous, 0 inert, 0 error**; strict
  19/63. The declared floor did not fall. One `BETTER` in both runs and it is
  the intended one: `530-walking-away-drops-a-standing-player-from-view`,
  `diverge -> match`, `reference_obs 2, subject_obs 2`, no missing or extra
  kinds.
* **The strict line carries 1 vacuous and it is not ours.** It is
  `520-a-standing-player-is-rebroadcast-on-a-timer`: strict drops the check's
  `observe` list, which is the very thing that *names* `move_entity_heartbeat`,
  so the heartbeat exemption drops every observation and vanilla is left silent.
  Pre-existing, arrives with the exemption plus 520, nothing to do with this
  branch. Declared vacuous/inert/error remain 0.
* **`add_entity`'s movement is out of the projection** (ORACLE rule 26) and our
  tail digest is now `dc545a7f9dce13f5` — which is `digest` of four zero bytes
  under this repo's multiplier, **computed by hand before the run and matched
  on the wire**. The old five-byte value `449516d9252be94f` is the same
  arithmetic with the movement's one zero byte still in it.

#### What was fixed

* **The velocity limb's sign was inverted.** `ServerEntity.sendChanges` enters
  it on `needsSync || trackDelta || (LivingEntity && isFallFlying)` — offsets
  874–919, `ifne` on `trackDelta` at 887 — and this tree tested `!trackDelta`,
  with a comment whose reasoning was backwards as well. Fixed, with a
  both-directions test, because a one-sided test passes under either sign.
* Three false doc comments about `ClientboundAddEntityPacket`'s field order,
  and two clean-room violations in `trace.rs` comments naming another server.

#### Nulls — do not re-run these

* **The velocity sign fix moves no verdict.** Runs 1 and 2 straddle it and are
  identical. The limb is guarded by a delta test and our item's velocity never
  changes, because `ItemEntity.tick` is not ported. Item physics closes 500's
  `missing_kinds: ["set_entity_motion"]`; this sign never could.
* **`ci/selfcheck-recompare.sh` cannot measure a `trace::decode` change.**
  `trace::observe` runs at record time, so the digest a decoder change would
  have to undo has already been taken. Rule 26 prescribes that control for a
  remedy it cannot reach; ORACLE.md now carries the split.
* **The 26 recordings in `~/rustmc/oracle-work/gameplay-selfcheck` predate
  530** (corpus 61). They cannot answer anything about it. There is now a
  corpus-63 set at `~/rustmc-chain-rule26/oracle-work/gameplay-selfcheck` on
  the Threadripper, 26 runs, taken with this branch's decoder — reusable by
  `ci/selfcheck-recompare.sh` for any *projection* change, but not for a
  decoder or recorder change.
* **Making a scenario boundary forget placements does not work.** Measured
  two-armed on those recordings: it fixes 540 (25/25 with 12 vacuous -> 13/25
  with none) and breaks 520 (0/25 -> 6/25) by re-admitting a neighbouring
  workspace's leftover. Do not re-try it; see the 540 finding below.
* **Generalising `ShapeGolden`'s row to a partition does not carry the
  connective pairs.** `StairBlock.updateShape` ignores the neighbour state it
  is handed and re-reads the level; a widened extractor emits rows that are
  wrong rather than missing.

#### The three pre-registered predictions, against measurement

`SELFCHECK_RUNS=26 ./ci/t2-gameplay-selfcheck.sh`, 26 cold-booted vanilla
servers, run 1 against runs 2..26, corpus 63, Threadripper, 2026-08-05
(4 h 55 m of recording).

```text
  walking-away-drops-a-standing-player-from-view    0/25   CONFIRMED
  dropping-an-item-throws-it-on-the-floor          25/25   REFUTED (predicted 0/25)
  spawn-egg-makes-a-mob            (control)       25/25   held
  a-dropped-item-goes-into-your-inventory (control) 25/25  held
  water-spreads                                    24/25   unchanged
  swapping-to-the-offhand-shows-on-both-channels   17/25   (published 11/25)
```

**530 is re-locked at `match`.** The controls are what make that mean
anything: two checks declared unstable on the *same packet* for *different*
fields stayed at 25/25, so the exemption is not wider than it claims.

**The refuted one is the useful one.** `ClientboundAddEntityPacket` has **two**
independent random fields for an item, not one: `ItemEntity`'s constructor
calls `setYRot(random.nextFloat() * 360.0F)` at offsets 44–52, and that spawn
rotation is still inside `rest`. Removing one of two cannot stabilise a check.
The entry's named field was wrong as well and is corrected in place.

#### FINDING — `540` is the sixteenth check that could not fail, and it is NOT this branch's

`540-a-player-that-leaves-the-ground-is-resynced` came back **`vacuous`**
against vanilla itself, which is gate-red. It is pre-existing: added in
`788c94b`, an ancestor of `15901ea`, and it observes `entity_position` /
`move_entity`, neither of which this branch's decoder change touches. Nobody
had run a selfcheck with it in the corpus before.

**Vanilla is not silent — it emits 7–9 observations in all 26 runs**, and the
`observe` list is right. Vanilla takes two paths through this scenario in a
**13/13** split, which is precisely the ground-flag race the check exists to
test:

```text
  pattern A  move_entity x2                        13 runs
  pattern B  entity_position x2 + move_entity x1    13 runs
```

`entity_position` carries an absolute position and survives `within` on its
own merits. `move_entity` carries only a delta, so it is bounded by the
`places` table — and **the table is stale.** `compare`'s `starts` accumulated
placements across scenarios; `530` places the observer at 530's workspace; the
recorder then walks both clients 32 blocks to 540's workspace *outside any
measured window*, so no packet says they moved; `within` is 16. Every
`move_entity` about a player standing next to the actor was deleted. So 540
scored `diverge` when vanilla went one way and `vacuous` when it went the
other — and it is `vacuous` that made it visible.

`place`'s comment justifies its insert rule with "a player is never announced
by `add_entity` inside a measured window, because it joined at login". **That
is false, and two checks exist because it is false** — `080` and `530`. The
comment is corrected.

**The obvious fix was tried and MEASURED, and it trades one defect for
another.** Making a scenario boundary forget placements, two arms over the
same 26 recordings, same binary, one variable:

```text
                                              OLD        NEW
  a-player-that-leaves-the-ground-is-resynced 25/25*     13/25
  a-standing-player-is-rebroadcast-on-a-timer  0/25       6/25   <- new
  everything else                             unchanged  unchanged
  * includes 12 vacuous; NEW has none anywhere
```

It works — the vacuous class disappears and what remains is the honest vanilla
race — but it destabilises `520`, whose reference sees 2 heartbeats where the
subject sees 4, from **a second entity id: a leftover at a neighbouring
workspace.** That is exactly the leakage `within` exists to stop, and
`an_entity_placed_by_an_earlier_scenario_is_still_placed` pins it deliberately.
**Reverted, not merged.** You cannot both bound out a stationary leftover and
keep a client that moved, without knowing which is which, and the trace does
not currently say.

The real fix is to make the harness state which entity ids are its own two
clients, so `place` can leave them unplaced — which is what `place`'s comment
already believes is true. That is a **recorder** change, so it needs
re-recording to verify and could not ride on the recordings this run already
paid for.

**Not declared in `gameplay-unstable.toml`, deliberately.** That file asserts
"vanilla draws this value fresh every run". Half of 540's instability is our
own projection deleting real observations, and declaring it would launder an
instrument defect as a vanilla property — the "widen the accepted verdict set"
move ORACLE rule 26 rejects by name. `540` keeps its honest `diverge` floor.

#### The single next item, and what closes it

**Give the trace the two client entity ids, then leave them unplaced in
`place`.** Closes 540 without touching `within`'s leftover bounding, which is
the constraint every cheaper fix fails. It needs one recording run to verify;
`ci/selfcheck-recompare.sh` cannot do it, because it is a recorder change.
Closing condition: 540's vacuous count is 0 **and** `520` stays at 0/25, on
the same sweep.
