# Cold start — branch `entity-ports`

---

### COLD START — branch `entity-ports` off `origin/train-b` (`3d6dae40f`), 2026-09-30

Worktree `~/grit/work-entity-ports`, remote dir `~/rustmc-entports`. The
player-attacks-player half lives on its own branch, **`entity-ports-pvp` off
`origin/damage`**, because the target's hurt path (`LivePlayer`, the level
half, `/damage`'s survival rig) exists only there and the attacker's
arithmetic (`crate::attack::melee`) only on train-b; that branch carries
train-b's `attack.rs` byte-identical so the two merge without touching it.
Its cold start is `docs/cold-start/entity-ports-pvp.md` on that branch.

#### Measured — `ci/t2-gameplay-subset.sh`, `~/rustmc-entports`, vanilla 26.2, 2026-09-30 21:16

Four checks, declared **4/4 match** (before: 776 `absent`, 777/788/789
`diverge`, `ci/checks/gameplay-baseline.toml`). Strict 1/4. Not a corpus
figure — the full `ci/t2-gameplay.sh` is the only thing that may publish one.

| check | before | after | what strict still differs on |
|---|---|---|---|
| 776 boat | absent | match (strict match) | — |
| 777 armor stand | diverge | match | `add_entity.y_head_rot` (unseeded draw, 777's own header), player heartbeat order (not this branch's) |
| 788 copper golem | diverge | match | player heartbeat order |
| 789 iron golem | diverge | match | 788's copper golem WANDERS in vanilla (brain) and stands here; `update_advancements` (SUMMONED_ENTITY unported); vanilla's iron golem strolled and looked around in this run and in `~/rustmc-diverge`'s it did not — unseeded |

#### What is ported, from the jar

* `CarvedPumpkinBlock.onPlace` → `trySpawnGolem`: `BlockPattern.find`'s
  corner walk and orientation order, the three patterns, `clearPatternBlocks`
  (level event 2001 with the cached state), `spawnGolemInWorld`,
  `replaceCopperBlockWithChest` with `getChestType` and the least-oxidised
  rule (`crate::golem`). Spawns ride `shapes::Effects::golems`, drained after
  the clear's level events.
* `IronGolem`, `SnowGolem`, `CopperGolem` as mob kinds (`crate::mob::golem`):
  data layouts, goals (iron, snow), `aiStep` countdowns, `doPush` retarget,
  air supply, sounds per oxidation level, `updateWeathering` on the level
  random, the copper spawn sound, the iron golem's crackiness sound.
* `ArmorStand` as a non-`Mob` `LivingEntity` on the mob host
  (`crate::mob::armor_stand`): `ArmorStandItem.useOn`, the Mob layers
  skipped by kind, `tickHeadTurn`, `hurtServer` for a player's melee (creative
  break, survival wobble then break, `Block.popResource`'s dropped item).
* `BoatItem.use` and `AbstractBoat`'s passenger-less tick (`crate::boat`):
  `getPlayerPOVHitResult` with `Block.OUTLINE` + `Fluid.ANY`, the new
  `data/vanilla/outline_shapes.json` (`ci/outline-shapes-golden.sh`, offset
  blocks carried unshifted, `BlockStateBase.getOffset` ported and checked
  bit-for-bit against 132 vanilla offsets), `FlowingFluid.getShape`'s
  process-wide cache, `getStatus`, `floatBoat`, `move`.
* **Two train-b defects fixed** (FINDINGS): `ServerEntity.sendChanges` sent
  dirty data first and ungated (it is last and gated); `melee` dropped the
  strong sound on a sprint hit.

#### Refused, by name

The copper golem's brain (`CopperGolemAi`, `TransportItemsBetweenContainers`)
— it stands where it is made; the snow golem's melt and snow trail; boat
passengers, paddling, the end-of-tick push/seat, bubble columns, the fluid
current, breaking a boat; `showBreakingParticles` (no `level_particles` on
this server); `SUMMONED_ENTITY`; the golems' attacks beyond what their goals
already carry (nothing here is an `Enemy`).

#### Killed, so nobody re-runs them

* **A snow-golem gameplay check**: 788's header records one was written and
  removed — its snow trail landed in 788's projection 5 pairs in 15.
* **Outline shapes through `CollisionShapes::from_json`**: the collision
  sweep proves collide queries, not clips; the outline table loads through
  `from_json_carrier`.
* **Refusing every offset block**: 17 of the 39 (grasses, ferns, roots, the
  tall flowers) never apply their offset to the outline; the rest are exact
  through `VoxelShape.move`'s `OffsetDoubleList` delegates.

#### The single next item

The copper golem's brain. Closing result: 789's vanilla-only packets from
entity 5 (the wandering copper golem) appear in ours, 789 strict match.

#### T0

**GREEN** at `0c70187db` (`~/rustmc-entports`, `rsync --checksum` -> touch ->
`ci/gen-gameplay-tables.sh` x2 -> plain `ci/t0.sh`, 2026-09-30 21:52). Reds on
the way, fixed: `ci/mob-tick-census.py` and `ci/entity-id-allocator.py`
(re-pinned for the new kinds, 6 -> 30; the allocator's reader fixed), two
clippy reds. **Keep-going reds, located (by the coordinator):** two of this lane's own detached jobs, `ent-t0e2` and `ent-t0e3`, both waited on `ent-sub2` and then ran `ci/t0.sh` in `~/rustmc-entports` at the same time (logs end 21:31:36 and 21:31:59; killing `ent-t0e2`'s wrapper did not stop its payload). `ci/entity-id-allocator-falsify.sh` and `ci/fastutil-open-hash-walk-falsify.sh` each edit a tracked file in place and restore it, so each run restored the other's doctored copy: `ci/checks/mob-blind.toml` with one `observes_behaviour` flipped, `ci/checks/fastutil-walk-history-golden.txt` cut to 505 lines, and four stages red on them. Not a keep-going defect. gate-pipeline (`d48904cb5`) now refuses a second T0 in the same tree and reds on any tracked-file change. **Never chain two T0s on one remote dir.**
