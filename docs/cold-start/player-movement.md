# COLD START, branch `player-movement`, off `main` (`b6237f29c`)

Worktree `~/grit/work-move`. Remote dir `rustmc-move`. Area: the server side
of player movement — the four `move_player` packets, the player's own
`Entity.move`, ground and fall detection, the correction teleport, and the
sprint/sneak/swim/climb states.

## What the server knows about a player now that it did not

`docs/SERVER.md` said "Movement is decoded and streams chunks; nothing else
responds to it" and "Still missing: collision and any validation of impossible
moves". Both were true of `main` and both are stale.

| what | where | checked against |
|---|---|---|
| `handleMovePlayer`'s whole state machine — `firstGood`/`lastGood`, the packet counters, the `100.0f * i` speed budget, `containsInvalidValues`, both clamps, `updateAwaitingTeleport`, the teleport counter | `crates/rustmc-server/src/move_player.rs` | 15 module tests, each with a positive control |
| the player's own `Entity.move` through vanilla's collision | `crates/rustmc-server/src/playermove.rs` | 5 tests against `collision_shapes.json` + `movement_props.json` + `attributes.json` + `block_props.json` |
| `fallDistance`, `doCheckFallDamage`, `checkFallDamage`, `calculateFallPower`/`calculateFallDamage` | `crates/rustmc-server/src/fall.rs` | 7 tests; the damage table 3/4/10/23 -> 0/1/7/20 and a hay bale's 0.2f -> 4 |
| the correction teleport, on the wire, through the real play loop | `crates/rustmc-server/tests/move_correction.rs` | 2 tests; found two joint defects on their first run |
| a sneaking player is silent and its cadence keeps running | `crates/rustmc-server/src/step.rs::advance_emitting` | 2 tests |
| a verb that can be refused | `tools/oracle-runner` `Step::Lurch` | `ci/checks/gameplay/542-…` (registration pending, see below) |

Every one of those is **opt-in**: `playermove::shared()` returns `None`
without `data/vanilla/{collision_shapes,movement_props,attributes}.json` and a
session with `None` behaves exactly as this server did before.

## What was refused, and why — do not re-run these

* **`clientIsFloating` and the flying kick.** Five of the nine terms of its
  conjunction (`allowFlight`, `abilities.mayfly`, `LEVITATION`,
  `isAutoSpinAttack`, `noBlocksAround`'s `getBlockStates` sweep) are
  subsystems this server has no field for, dropping them inverts the
  conjunction's usual value, and its one consumer — `tick()`'s
  `aboveGroundTickCount` kick — is not ported either. The field stays `false`
  and the accessor says so.
* **"Sneaking under a slab" is not a server-side scenario at all.**
  `LivingEntity.getDimensions(Pose)` branches on `SLEEPING` only and
  `getDefaultDimensions(Pose)` **discards its argument**; `EntityDimensions`
  is a record with no pose component; `EntityTypes.<clinit>` 7318-7329
  registers the player `sized(0.6f, 1.8f)`. The shorter crouching hitbox is
  the client's.
* **A collision golden was not written** — `tools/goldengen/vanilla/EntityMoveGolden.java`
  and `crates/rustmc-server/golden/entity_move.json` already are one, over
  the player's own `0.6 x 1.8` box at `maxUpStep` `0.6f`, with flat/slab/
  stair/embedded/fence/corner/sliver/flush families and three assertions per
  probe. It runs in `ci/t1.sh`. The brief asked for one; the tree had it.
* **`onClimbable` was not ported.** It needs `BlockTags.CLIMBABLE` as a
  *generated, stamped* table and `ci/gen-gameplay-tables.sh` has **no
  generator for `tags_block.json`** — the copy on the box arrived through
  `cp -rn` from a shared tree. Reading that unstamped copy is the
  2026-08-03 shared-`data/vanilla` failure by another door. Adding
  `BlockTagsGolden.java` and its `generator_for` arm is the next item, and it
  unblocks `isStateClimbable` (CLIMBABLE **or** `POWDER_SNOW`, used by the
  step cadence) and `LivingEntity.onClimbable` (CLIMBABLE **or**
  trapdoor-over-a-matching-ladder, no powder snow) — two different
  predicates that a port will otherwise conflate.
* **Sprint and pose broadcast were not wired.** `Entity.setSprinting` is
  `setSharedFlag(3, …)` on `DATA_SHARED_FLAGS_ID`, and this server's
  `crate::synched` models only `SynchedEntityData.getNonDefaultValues()` for
  the *pairing* packet — there is no `packDirty`/`ServerEntity.sendChanges`
  path, so a sprint flag would be a field written by `player_command` and
  read by nothing on the wire. That is the dead-field shape the brief opens
  with. Needs an entity-data dirty path first.
* **`checkMovementStatistics` was not ported.** It is `awardStat` and
  `causeFoodExhaustion` nine times over and this server has neither.

## What needs hands

1. **`542-a-move-past-the-speed-budget-is-teleported-back` is written and NOT
   registered.** It has no `ci/checks/gameplay-falsifiable.toml` row and no
   `gameplay-baseline.toml` row, so it must not be committed until a full
   `ci/t2-gameplay.sh` has measured it — `ci/corpus-falsifiable.py --check`
   runs in T0 and refuses an unregistered scenario. The file is in the
   worktree, untracked, at
   `ci/checks/gameplay/542-a-move-past-the-speed-budget-is-teleported-back.toml`.
   The first T2 in `~/rustmc-move` was launched from a tree that still had
   the teleport-id-1 and the non-sticking-correction defects, so **its
   verdict for 542 is not the branch's verdict** and a second run is needed
   before the row is written.
2. **`ci/order-selfconsistency.py --check` is red on unmodified `main`.** Its
   recordings glob is `~/rustmc*/oracle-work/gameplay*/vanilla.json` — every
   tree on the box — so any lane recording a vanilla run reddens T0
   everywhere at once. Reproduced by `git archive b6237f29c ci` into a
   scratch dir. Not mine, not rewritten.
3. The fall damage this branch computes is **not applied**. `origin/player-systems`
   has `hurt` and `set_health`; this is the input it said the server could
   not produce. Wiring the two together is one commit and a scenario.
