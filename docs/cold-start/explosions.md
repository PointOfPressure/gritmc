# Cold start — branch `explosions`

---

### COLD START — branch `explosions`, off `main` (`1d109abc`), 2026-09-06

**The item: vanilla 26.2's explosion on the server, transcribed from the jar.**
Worktree `/home/sayer/grit/work-boom`, remote dir `~/rustmc-boom`. No corpus
run was queued and the box-wide corpus lock was never taken: everything below
is the pinned jar, four JVM dumper runs and Rust unit tests.

## What landed

Three generated tables, all from the jar, all cross-checked against a source
their own dumper did not produce:

| table | generator | what it is | cross-check |
|---|---|---|---|
| `block_explosion.json` | `ci/block-explosion-golden.sh` | 1,196 blocks, 35 distinct `(resistance, dropFromExplosion)` rows, 5 fluids | 32,366 states against `block_props.json`'s, a different dumper |
| `explosion_ray.json` | `ci/explosion-ray-golden.sh` | vanilla's own `calculateExplodedPositions` over 12 worlds, 5,395 positions, both before and after `Util.shuffle` | four cases empty for four different reasons; two seeds and two worlds must differ |
| `explosion_wire.json` | `ci/explosion-wire-golden.sh` | vanilla's own `STREAM_CODEC.encode` for `explode`, 5 cases, 289 bytes | packet id against `reports/packets.json`, sound id against `protocol_ids.json`, `blockCount` against `explosion_ray.json` |

`crates/rustmc-server/src/explosion.rs` is the port: the table reader,
`block_explosion_resistance`, the two interaction enums and
`ServerLevel.explode`'s five-arm switch, `calculate_exploded_positions`,
`Util.shuffle`, the `getSeenPercent` sampling grid, `getEntityDamageAmount`,
the knockback scale, the `StackCollector` merge, `explode()` end to end over an
`ExplosionWorld` trait, and `explode_body` for the packet. 36 unit tests plus
two golden test files.

**The ray walk matched vanilla position-for-position on the first run**, in
`java.util.HashSet` order, and its shuffle matched too.

## The one thing that would make this live, and it is not in this area

**Nothing on this tree can start an explosion.** Not a scenario, not a command,
not a block:

* `crates/rustmc-server/src/commands.rs` has nine roots and `summon` is not one;
* `tools/oracle-runner/src/scenario.rs:531-533` — `Step::Command` cannot run a
  permission-level-2 command, because the harness players are not operators;
* the console fifo carries only `[[gamerule]]`, and `scenario.rs:1210-1219`
  validates its three strings to `[A-Za-z0-9_-]` with no spaces, so no other
  command fits through it; `crates/rustmc-server/src/console.rs:114-116`
  answers anything but `gamerule` with "not understood";
* there is no `TntBlock`, no `RespawnAnchorBlock` and no `BedBlock` explosion
  arm, so no block initiates one either.

So **no gameplay scenario was registered**, and that is a refusal rather than an
omission: a scenario would record vanilla exploding and this server doing
nothing, at tier 1, moved by no edit to any line of `explosion.rs`. The brief
this branch was given said `/summon tnt` was reachable "now that `commands` has
merged"; it is wrong on all four counts above.

The live `ExplosionWorld` implementation is deliberately absent for the same
reason. It is about forty lines over `crate::edits::Blocks`,
`crate::shapes::set_block` and `rustmc_loot::Loot::block_drops`, and writing it
before there is a caller would put a dead adapter in the tree.

## What was killed as a null — do not re-run these

* **Do not try to pin `STEP` or `STEP_DECAY` with a position golden.**
  Measured: `0.30000001192092896 -> 0.3` and `0.22500001f -> 0.225f` both leave
  all 5,395 recorded positions unchanged, as do normalising in f32 and
  computing the ray direction in f64. The effect is ~1e-7 blocks over ~23 steps
  and the instrument floors to whole blocks. Five *structural* mutations were
  caught; the full table is in `crates/rustmc-server/tests/golden_explosion_ray.rs`.
  An instrument that could see these would have to record the pre-floor
  coordinates, which vanilla never exposes.
* **Do not cross-check `Block.getExplosionResistance()` against the
  `BlockBehaviour.explosionResistance` field alone.** They agree for all 1,196
  blocks: `StairBlock` is the only override and it returns its base's, which a
  stair's own field already holds. The check refused itself on its first run.
* **Do not add a hardness column to `block_explosion.json`.** Nothing on the
  explosion path reads `destroyTime` — not the ray walk, not `onExplosionHit`,
  not `getDrops`.
* **Do not look for a protection-enchantment term in `hurtEntities`.** There is
  none in 26.2. The only reductions are `getKnockbackMultiplier` and
  `Attributes.EXPLOSION_KNOCKBACK_RESISTANCE`.
* **Do not send a `level_particles` packet with an explosion.** The particles
  ride on the `explode` packet itself (`ServerLevel.explode` offsets 252-273);
  sending both draws them twice.
* **Do not copy the position list before shuffling it.** `explode()` holds one
  `List`; `createFire` walks what `interactWithBlocks` permuted in place.

## Traps worth carrying to another lane

* `sun.reflect.ReflectionFactory.newConstructorForSerialization` makes a
  dumper able to run **any** `ServerLevel`-shaped method, not just the ones
  that take a `Level`. `ExplosionRayGolden.java` is the worked example.
* `ClientboundExplodePacket.blockCount` is `ByteBufCodecs.INT`, four fixed
  bytes, next to six varints. Third instance of that trap in this repo.
* `Explosion` is an **interface** in 26.2 and `BlockInteraction` has four
  values, not three; `Level$ExplosionInteraction` has five. A reader of the
  1.20 shape gets both wrong.
* `tnt_explosion_drop_decay` defaults **false** where the other two decay rules
  default true. That is the whole of why TNT gives back what it broke.

## The single next item

**The trigger, and it belongs to whoever lands entity-in-world.** The cheapest
one that needs no entity at all is `RespawnAnchorBlock.explode` — `removeBlock`,
then `ServerLevel.explode(null, badRespawnPointExplosion(centre), <its own
anonymous calculator>, Vec3.atCenterOf(pos), 5.0f, fire = true,
ExplosionInteraction.BLOCK)`, with a **null source entity**, reached from
`useWithoutItem` when `charge > 0` and `EnvironmentAttributes.RESPAWN_ANCHOR_WORKS`
is false. It needs the `CHARGE` property, the glowstone `useItemOn` arm and the
environment-attribute lookup, none of which are explosion work. `PrimedTnt`
(fuse 80, power 4.0f) needs `origin/mob-in-world` `44027b933`.

What closes it: one live `ExplosionWorld`, one corpus scenario that reaches
tier 2 because an edit to `calculate_exploded_positions` moves its outcome, and
the first `explode` packet this server has ever sent.

---

### COLD START — branch `explosions`, addendum, 2026-09-06

**`ci/explosion-falsify.sh` is the instrument that makes the three goldens
mean something**, and it went wrong in the project's own signature way before
it went right.

Seventeen mutations of `crates/rustmc-server/src/explosion.rs`, each applied
alone against the unchanged goldens, each with its outcome **declared**:
13 CAUGHT, 4 MISSED. `EXPLOSION_FALSIFY_DRYRUN=1` applies all seventeen to a
copy and runs none, in about a second, and is wired into T0 —
`ci/checks/required-checks.toml`, `REQUIRED_FLOOR` 15 -> 16. The full sweep
rebuilds seventeen times, takes about ten minutes and is not in T0.

**It read a failed BUILD as a caught mutation.** Its first full run reported
all four declared-MISSED cases as CAUGHT, which would have been a real and
welcome finding, while a gate was compiling on the same box. A clean re-run
one case at a time gave MISSED for all four. `cargo test` exits non-zero for a
failing assertion and equally for a build that never started, and the script
was reading both as evidence. `cargo test --no-run` is now a separate step and
a build failure is `INCONCLUSIVE`, exit 3.

**Run it when the box is quiet, and never edit it while it runs**: it mutates
`explosion.rs` in place, and inserting lines near the top of the script during
its own sweep shifted the byte offset bash was parsing from and left the tree
poisoned. The dry arm works on a copy for that reason. Copying the whole script
elsewhere does *not* work around it — `ROOT` comes from its own `dirname`, so
`--manifest-path "$ROOT/Cargo.toml"` then points outside the tree and every
case fails to build.

The golden grew two worlds in the same session — `wall_east` (obsidian at
x >= 2, the axis `floor_r4` cannot test) and `ceiling` (`isInWorldBounds` false
above y = 68, a `break` where a hard block is a subtraction) — taking it from
12 cases and 5,395 positions to 14 and 6,988. The four precision mutations are
still invisible at that size, which is what makes the "it is `Mth.floor`'s
granularity, not a thin case set" reading the right one.
