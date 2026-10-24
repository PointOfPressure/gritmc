# Cold start — branch `movement-tick`

Every `### COLD START` entry ever written for this branch, oldest and newest alike, moved out of `docs/WORKLIST.md` by the doc-collision-class restructure (1 entry). Order matches how they appeared there — not necessarily chronological, since WORKLIST inserted each new one above the last.

---

### COLD START — branch `movement-tick` off `entity-move-collision` (`65ced03`), 2026-08-06

**The item was "wire `crate::collide` into the tick and close 070, 500 and
540". Two of those three cannot be closed by any server, and that was measured
before a line of server code changed.** Remote dir `~/rustmc-movetick` on the
**Threadripper**, logs on the laptop under `~/grit/logs/movetick/`, working
tree `~/grit/work-movetick`. Not merged — the orchestrator gates. Expect a
rebase once `entity-move-collision` lands.

Everything below is in `docs/intel/movement-tick-three-checks.md`, committed
before the run that would have let it be written backwards.

#### The one thing to read if you read nothing else

**`540-a-player-that-leaves-the-ground-is-resynced` is not reproducible on the
VANILLA side.** Seven runs of the same scenario against the same jar in the
same tree gave two different *packet-kind sequences* — `SS@0 d@1` on three and
`d@0 d@1` on four (`S` = `entity_position`, `d` = `move_entity`). A probe that
differs only in the ground bit being set throughout is stable at `d@0 d@1`, 6
runs for 6; a probe with the bit clear throughout produced three distinct
sequences in six runs. `ci/.flap540.sh` (untracked) is the loop; the table is
in the intel doc.

Two syncs at one position with **different** `rest` digests means the ground
flag crossed `sendChanges`' predicate twice inside one step with no packet
between, so vanilla restores the flag itself within a tick or two and
`sendChanges` — `updateInterval` 2 for a player — either lands in that window
or does not. Same phase-coin-flip family as the 60-tick heartbeat that
`compare::project` already filters.

**This does not license deleting `was_on_ground != e.on_ground` from
`crate::tracker`.** That disjunct is real and unit-tested. 540 belongs in
`ci/checks/gameplay-unstable.toml`; **not declared there on this branch**,
because that file is verified in both directions by
`ci/t2-gameplay-selfcheck.sh` and five subset runs are not a selfcheck at
N >= 12. `SELFCHECK_RUNS=12 ./ci/t2-gameplay-selfcheck.sh` is the run that
settles it and it is the cheapest next thing anyone can do here.

#### The premise the previous branch handed over is refuted

`entity-move-collision` recorded that the three checks "need the physics above
`Entity.move` … and every one of those needs gravity, and
`LivingEntity.getDefaultGravity()` is `getAttributeValue(Attributes.GRAVITY)`".
That is true of the *attribute*, and false of the conclusion. Read from the
pinned jar, every link checked:

```text
  LivingEntity.aiStep    600: canSimulateMovement()  ifeq -> skip travel
                         607: isEffectiveAi()        ifeq -> skip travel
                         614: travel(Vec3(xxa, yya, zza))
  Entity.canSimulateMovement          -> isLocalInstanceAuthoritative()
  Entity.isLocalInstanceAuthoritative -> !isClientAuthoritative()   (server side)
  Player.isClientAuthoritative        -> iconst_1
```

**A `ServerPlayer` never reaches `travel`.** No server-side player gravity, no
friction step, and inside `Entity.move` the same predicate gates
`restituteMovementAfterCollisions` (offset 563) and `checkFallDamage` (525) off
as well — `handleMovePlayer` calls `doCheckFallDamage` itself instead. There is
no player physics to port. Do not port it.

Nor is gravity an attribute for the two kinds that actually fall:
`ItemEntity.getDefaultGravity()` and `FallingBlockEntity.getDefaultGravity()`
are each `ldc2_w 0.04d`, on entity types `DefaultAttributes` has **no supplier
for at all** — asserted by `ci/attributes-golden.sh`, which fails if either
ever grows one.

#### What actually needs the attribute path, and what it measured

`Entity.maxUpStep()`, which `crate::collide::collide` already takes as an
argument: `Entity`'s is `fconst_0` and `LivingEntity`'s is
`getAttributeValue(STEP_HEIGHT)`. `getAirDrag` and `getEntityBounciness` are
two more reads on the same path.

So the path is ported as **data plus arithmetic**, and `0.08` and `0.6` appear
nowhere in the source. `ci/attributes-golden.sh` ->
`data/vanilla/attributes.json`, measured on the box:

```text
  40 attributes, 93 entity types with a base map, 65 without
  minecraft:gravity default=0.08 range=[-1.0, 1.0] ranged=True
  minecraft:player carries 36 attributes
```

`crates/rustmc-server/src/attributes.rs` implements
`AttributeInstance.calculateValue` (the three operations in vanilla's order,
with `ADD_MULTIPLIED_BASE` multiplying the **post-`ADD_VALUE` base** rather
than the running total) and `RangedAttribute.sanitizeValue` (NaN -> `minValue`,
then `Mth.clamp`, which is not `f64::clamp`). **An attribute the type does not
carry is `None`, not a default** — `AttributeSupplier.getAttributeInstance`
throws, and `unwrap_or(spec.default)` would give a falling block a step height.

#### 500 cannot close either, and porting `ItemEntity.tick` is still right

`set_entity_motion`'s velocity is inside the observation's `rest` digest **on
purpose** — `crate::trace` reads the `LpVec3` beside the digest, not instead of
it, because the packing clamps at 32766 and consuming it would make two
different wire messages compare equal. The item's throw velocity is four
unseeded `nextFloat()` draws and its spawn rotation a fifth, so every
observation downstream is unmatchable and
`ci/checks/gameplay-unstable.toml` already has it at 25/25 at N = 26.

Porting `ItemEntity.tick` correctly therefore **adds** unmatchable
observations. Do it anyway — a thrown item that hangs in the air is a defect a
player sees — but predict `diverge` and do not read the result as a failure.

#### 070 is the one that closes, and it is the next item

Vanilla's arm is deterministic: identical projected observations with identical
digests in `~/rustmc-entitymove-0806`'s full-corpus recording and in a fresh
subset run here, `add_entity [1.5,1.0,0.5] kind 51 rest dc54fa7f9dce24f3` in
both. The chain, none of which exists yet:

1. `FallingBlock.updateShape` -> `scheduleTick(pos, this, getDelayAfterPlace())`,
   delay **2**. `crate::shapes` already records that this is why sand has zero
   rows in `update_shape.json`; the scheduling itself is not wired.
2. `FallingBlock.tick`: `if (isFree(below) && pos.y >= level.getMinY())
   FallingBlockEntity.fall(...)`. `isFree` is
   `isAir || is(#fire) || liquid() || canBeReplaced()`.
3. `FallingBlockEntity.fall`: entity at `(x+0.5, y, z+0.5)`, then
   `setBlock(pos, state.getFluidState().createLegacyBlock(), 3)`, then
   `addFreshEntity` — **setBlock first**, and the trace shows `add_entity`
   reaching the client *before* that `block_update`.
4. `FallingBlockEntity.tick`: `time++`, `applyGravity()`, `move(SELF, delta)`,
   then on landing `setDeltaMovement(delta.multiply(0.7, -0.5, 0.7))`,
   `level.setBlock(pos, blockState, 3)`, an **explicit**
   `chunkMap.sendToTrackingPlayers(this, new ClientboundBlockUpdatePacket(...))`
   — which is the *duplicate* `block_update` at [1,0,0] the trace shows — then
   `discard()`. Every path ends `setDeltaMovement(delta.scale(getAirDrag()))`,
   0.98.
5. A `FALLING_BLOCK` `EntityKind`: `.sized(0.98, 0.98)`,
   `clientTrackingRange(10)`, `updateInterval(20)`, `trackDeltas` true, and
   `add_entity`'s trailing `data` varint is `Block.getId(blockState)`.

Hand-simulated, the fall from y=1 to y=0 takes 7 ticks; the recording's
add_entity->landing gap is 272-304 ms, so that is the number to check against.

#### What is on this branch, and the thing it does NOT do

`attributes.rs`, `movement_props.rs`, `physics.rs` (`Entity.move`'s outer,
`applyGravity`, `checkSupportingBlock`, `findSupportingBlock`, `getOnPos` and
its two different offsets, `getBlockSpeedFactor`,
`restituteMovementAfterCollisions` with the bounciness arm implemented rather
than reduced away), two generators, two extracts. Unit-tested; T0 green.

**Nothing calls any of it yet, and that is the same criticism this branch was
sent to fix.** It is said here rather than discovered: the run was spent
establishing that two of the three targets are unclosable and that the third
needs a chain nobody had enumerated, and the remaining budget went on the
pieces that chain stands on. The next item is `movement.rs` — `ItemEntity.tick`
and `FallingBlockEntity.tick` — plus a `CollisionBlocks` adapter over
`crate::tiles`, plus the `needs_sync` disjunct in `ServerEntity.sendChanges`'s
gate (`tickCount % updateInterval == 0 || needsSync || dataDirty`, offsets
249-284), which is what puts an item's `set_entity_motion` on ticks 12, 21 and
26 rather than only on multiples of 20.

**Gate it with a golden that runs vanilla's own `ItemEntity.tick` and
`FallingBlockEntity.tick` for N ticks and records position, velocity and
onGround per tick**, in the style of `ci/entity-move-golden.sh`. That golden
does not exist yet and is the single highest-value thing to build next: it is
the only instrument that can grade the item path at all, because check 500
structurally cannot.

**Start from `MoveLevel` in `tools/goldengen/vanilla/EntityMoveGolden.java`** —
a `Level` that is a `HashMap` of blocks and throws, by name, for everything the
collision path does not touch. Two obstacles, both read from the bytecode on
this branch rather than left to be discovered:

* `ItemEntity.tick` needs `getFluidState`, `noCollision`,
  `updateFluidInteraction` and `applyEffectsFromBlocks` on top of what
  `MoveLevel` answers today. All four are reachable with the same
  throw-by-name discipline.
* **`FallingBlockEntity.tick` branches on `level instanceof ServerLevel`** and
  everything after `move` — the landing, the `setBlock`, the explicit
  `sendToTrackingPlayers` — is inside that branch. A `MoveLevel` can never
  enter it. So either the golden covers the falling block's *physics only*
  (`time++`, `applyGravity`, `move`, and the final `scale(getAirDrag())`,
  which is the whole of what `physics.rs` implements) and the landing is
  gated by check 070 instead, or the harness needs a real `ServerLevel`,
  which is a much larger instrument. **Take the first.** The landing has a
  deterministic corpus check and the physics does not.

A physics-only golden is also the one that grades the *item*, whose landing
has no closable check at all — so it is not a compromise on the falling block,
it is the only instrument either kind can have.

#### The falsification sweep — 45 injections, three rounds, 43/2

`ci/movement-falsify.sh`. Each injection is a specific misreading of the
bytecode, not a random mutation.

```text
  round 1   26 caught, 19 NOT caught, 0 not applied
  round 2   35 caught, 10 NOT caught, 0 not applied
  round 3   43 caught,  2 NOT caught, 0 not applied
```

Round 1's nineteen holes were not scattered: thirteen were inside
`move_entity`, `check_supporting_block` and `block_speed_factor`, which had
**no test at all** — testing them needs a CollisionShapes + CollisionBlocks +
MovementProps fixture and none existed. That fixture now exists in
`physics.rs`'s test module and is the thing to reuse.

**The two that remain uncaught are named rather than left:**

* **21, "gravity is applied unconditionally"** — an *equivalent mutation*.
  There is no `f64` y for which `y - 0.0` differs from `y` in bits, `-0.0`
  included, since `(-0.0) - (+0.0)` is `-0.0`. Vanilla's `if (g != 0)` is a
  short circuit, not a semantic. Not a hole; do not write a test for it.
* **45, "Mth.lerp is written as a plain multiply"** — only reachable on
  `restituteMovementAfterCollisions`' `b > 0` arm, which needs a block with
  non-zero bounce restitution. 17 blocks have one and no ported mechanic can
  place any of them, so the arm is live code with no caller. The test that
  closes it is the one that first makes a slime block reachable.

**Two faults in the sweep itself, both worth carrying forward.** The first
version built a *regex* out of the search text, so `t += v` meant "t, one or
more spaces, = v" and two injections silently edited nothing. They were
reported as NOT APPLIED rather than as caught — an injection that never
happened produces a green test run, which is indistinguishable from a hole —
and **that guard is the load-bearing part of the script**. The second: it ran
`--release`, at minutes per injection, which is how a sweep stops being run.
It is debug now, with the argument for why the profile cannot matter here
written at the call.

One test failure during this work was the *test* being wrong and the code
right: `Block.getSpeedFactor()` is a Java `float`, so an item on soul sand
ends up at `0.4000000059604645` and not `0.4`. Asserted exactly now — a
tolerance loose enough to hide that would hide a real defect the same size.

#### Nulls, so nobody re-runs them

* **Player physics on the server: does not exist in vanilla.** Killed above.
* **540 by making our server correct: impossible.** The reference flaps.
* **500 by making our server correct: impossible.** Random fields in the digest.
* **`Block.builtInRegistryHolder().is(BlockTags.FENCES)` in a dumper: returns
  `false` for every block.** Tags bind on the datapack reload, which a dumper
  never does. `ci/movement-props-golden.sh` reads `#minecraft:fences` and
  `#minecraft:walls` out of the generated data reports and resolves nested
  `#` references itself. **This was caught only by a named per-block assertion
  in the generator** — the aggregate counts were all plausible — which is the
  argument for naming rows rather than counting them.
* **`Float.toString` compared against a Python `float`.** Three rows of that
  same generator failed on the first run because the gate parsed a Java `float`
  literal as a double; `0.6 != float32(0.6)` by 2.4e-8. The bug was in the
  gate, not the data. Both sides now go through a float32 round trip.
* **`slime_block.bounceRestitution` is 1.0, not 0.8.** The 0.8 is
  `Entity.getBlockBounciness`'s multiplier for a non-`LivingEntity`. The
  generator's first run corrected the assertion rather than the data.
* **The full corpus run was KILLED at 74/86 and produced no report.** The
  vanilla arm completed all 86; this server's arm reached 74 and
  `/tmp/t2g.log` on the box then ends in `Terminated`, with no
  `rustmc-movetick` process surviving. It never reached the compare step, so
  there is **no verdict for any check** — not a partial score, none. **The
  cause is attributed, not unknown**: the orchestrator ran
  `pkill -f rustmc-movetick` to stop it contending with the `redstone-power`
  merge gate, and the pattern matched by working directory. This session's own
  `pkill` against `movement-falsify` is exonerated. **Re-running it on a quiet box is the first thing to do here**, and
  `docs/intel/movement-tick-corpus-prediction.md`'s prediction is still the
  pre-registered one to check against. What the run *did* close is the falsifier it was for: two
  new extracts from two new generators went through `t2-gameplay`'s provenance
  preflight untouched and it did not exit 4.
* **Our arm ran at one scenario every two to five minutes** against the
  vanilla arm's two a minute — chunk generation at a fresh workspace on a
  box carrying two other agents' jobs. Not a property of the branch, and the
  reason the re-run wants a quiet box: several checks carry `Timing::Bucket`
  with 500 ms buckets.
