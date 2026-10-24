# COLD START — branch `gameplay-entity-coverage`

Append-only. Newest entry last.

### COLD START — branch `gameplay-entity-coverage`, 2026-08-12, the entity family and what its own floor took back

**Read this first: the probe's control is a POSITIVE control and its 15/15 is
the instrument working.** `probe-spawn-egg-control` is
`080-spawn-egg-makes-a-mob` copied byte for byte, and that check is a declared
member of `ci/checks/gameplay-unstable.toml` at 25 flaps in 25 comparisons over
N = 26 cold vanilla servers. It is in `ci/entity-spawn-probe.sh` precisely so
that a clean golem floor cannot be a statement about how quiet the box was. A
run in which it stopped flapping would be the inadmissible one. Do not re-open
this as "the instrument is broken"; one orchestrator turn has already been
spent on that reading.

#### What was measured

`ci/entity-spawn-probe.sh`, N = 6 cold vanilla servers, seed 20260803,
superflat, `difficulty=peaceful`, Threadripper, 2026-08-12. Ensemble
`entity-probe-n6`, projection `proj:7cefde0fb56d9f3e`, one recorder
(`tree-now:fad70303792d66da`), 15 pairwise comparisons, resolving power 39.3%
at alpha = 0.05.

```text
  a-carved-pumpkin-on-snow-makes-a-snow-golem      11/15 pairs, 3 distinct
  a-carved-pumpkin-on-copper-makes-a-copper-golem   5/15 pairs, 2 distinct
  probe-spawn-egg-control                          15/15 pairs, 6 distinct
  the other six                                     0/15
```

**The field that moves in the control**, field by field over all six runs:
`add_entity.y_rot` and `add_entity.y_head_rot`, equal to each other in every
run, six distinct values (5, 34, -31, -24, -86, 117); and
`set_entity_data`'s residue, four distinct values whose grouping does *not*
follow the yaw. Everything else is identical in all six: position
`[1.5, 0.0, 0.5]`, `kind_id` 100, `data` 0, `x_rot` 0, `add_entity`'s residue
(`cbf29ce484222325`, FNV-1a's offset basis — nothing undecoded), and **the raw
entity id, 11 in every run**. The obvious hypothesis for "six runs, six
projections" is a monotonic counter and it is dead.

The cause is §26.4's, not a new one: `EntityType.spawn` calls
`snapTo(..., Mth.wrapDegrees(serverLevel.getRandom().nextFloat() * 360.0F), 0.0F)`.
`ci/dark-systems-probe.sh` read the same signature the same day through a
different projection. Cite §26.4 rather than re-deriving it.

#### Killed as nulls — do not re-run these

* **`update_attributes` is NOT where `finalizeSpawn`'s FOLLOW_RANGE draw
  shows up.** Its residue is `594df060bec3e117` in all six control runs while
  the yaw takes six values. The `random.triangle(0.0, 0.11485)` draw is real in
  the bytecode and never reaches the wire — the attribute is not
  client-syncable. This branch's own probe header and 778's header both
  asserted otherwise for a day; both are corrected in the tree. A bytecode
  reading is not a wire prediction until something has read the wire.
* **The entity-id-counter hypothesis** — dead, see above, id 11 in all six.
* **A type-keyed decoder exemption for mob yaw** — still the wrong fix, for the
  reason in the probe header: a pig's yaw is random only on the
  `EntityType.create` path, so the type is not a sound key and the exemption
  would stop comparing a value that IS comparable.
* **Declaring the snow golem unstable instead of deleting it** — does not work.
  An entry in `ci/checks/gameplay-unstable.toml` still *runs*; it only declares
  that the check can never match. The snow golem would still be in the world,
  still walking, still laying blocks into its neighbours' windows.

#### What took a scenario away

Every difference in both failing golem rows is a `snow[layers=1]` /
`grass_block[snowy=true]` `section_blocks_update`, plus one `sound`: the trail
a live snow golem lays as it strolls. The copper golem's 5/15 is **not its own
behaviour** — the snow in its diff is at x 14..17, and it is the snow golem,
which had walked into the copper golem's window by run 4.

778's own header had measured that coupling at N = 2 and written it down as
contained: "21 to 26 blocks from that scenario's anchor — outside the default
`within` of 16 with five blocks to spare". At N = 6 the five blocks of spare
turn out to be the width of one run's random walk. **A margin measured once is
not a bound**, and `ci/checks/gameplay/README.md` now says so at the place the
old claim was made.

Narrowing the lens would be the defect this repository keeps catching: the
trail is behaviour, and 778's header itself calls it "the part no server gets
for free".

#### What ships

Seven checks, `773`-`777`, `788`, `789`. Corpus **93 -> 100**. The golems are
renumbered above `780-random-ticks-turn-covered-grass-to-dirt`, which landed on
main while this branch was out, so the "a check that leaves a live entity
behind is numbered last" rule still holds with `790` last. A copper golem and
an iron golem write no blocks, which is the whole of why those two survived and
the snow golem did not.

`docs/PARITY-ROADMAP.md` §30 has the argument and the tables.

#### The single next item

**Decode `set_entity_data`'s metadata blob instead of digesting it.** Four
distinct residues over six runs is one draw more than the branch can account
for: `finalizeSpawn`'s `setLeftHanded` is p = 0.05 and gives two values, and
the FOLLOW_RANGE draw does not reach the wire at all. A third draw is on the
`EntityType.create` path and absent from the golem path. Naming it is what
would let anything ever be said about a spawned mob's synched data, and the
result that closes it is a named field with a bytecode site, the way the yaw
has one.
