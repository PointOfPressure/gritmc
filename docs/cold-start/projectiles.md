# Cold start — branch `projectiles`

---

### COLD START — branch `projectiles` off `main` (`1d109abcf`, 2,788), 2026-09-06

**Vanilla 26.2's thrown and shot entities are transcribed into
`crates/rustmc-server/src/projectile/`, and the half of that path reachable
without a `Level` is gated bit-exact against vanilla running it.** Worktree
`~/grit/work-proj`, remote dir `~/rustmc-proj` on the Threadripper. Not merged
— the orchestrator gates.

    math       Vec3/AABB operations `collide.rs` does not carry: normalize,
               horizontalDistance, contains, getCenter, inflate, AABB.clip
    shoot      getMovementToShoot / shoot / shootFromRotation / lerpRotation /
               updateRotation
    clip       BlockGetter.clip -> traverseBlocks -> VoxelShape.clip -> AABB.clip
    hit        ProjectileUtil.computeMargin / getManyEntityHitResult /
               nearest / canHitEntity
    arrow      AbstractArrow.tick, the inGround state machine, onHitBlock,
               shouldFall/startFalling, tickDespawn, the piercing loop
    throwable  ThrowableProjectile.tick and the five item projectiles' onHit
    bow        BowItem.releaseUsing, CrossbowItem's charge/power/shot pitch
    wire       the add_entity `data` field and the six EntityKind rows

**Gates: 39 unit tests in T0, 18 + 11 integration tests in T1**, registered as
their own bucket in `ci/checks/test-targets.toml`. `cargo clippy --all-targets
-D warnings` clean across the workspace.

#### THE ONE THING TO READ IF YOU READ NOTHING ELSE

**`ci/projectile-golden.sh` needed no `Level` and it caught a real defect on
its first run.** `BlockGetter.traverseBlocks`, `AABB.clip`,
`VoxelShape.clip`, `Projectile.lerpRotation`, `BowItem.getPowerForTime`,
`CrossbowItem.getShotPitch` and `RandomSource.triangle` are all static,
public, or reflection-reachable on classes that touch no world state. So 788
golden rows — 325 launch directions, 180 `triangle` draws, 225 `lerpRotation`
pairs, 41 bow powers, 18 crossbow pitches, 55 `AABB.clip` probes, 88
`VoxelShape.clip` probes and 9 DDA walks totalling 73 cells — cost a 300-line
harness instead of `EntityMoveGolden`'s 900-line `MoveLevel`.

The defect: `AABB.clipPoint`'s parameter guard is
`dconst_0; dload h; dcmpg; ifge <return best>`, so `h` must be **strictly**
positive and a segment starting flush on a face and ending inside the box is a
MISS. This lane had written `0.0 <= h` from the same bytecode. In play that is
a projectile spawned against a hitbox passing through it in vanilla and
stopping dead here.

**Before you build a `MoveLevel` for anything, check which of your methods are
static.**

#### FOUR CORRECTIONS TO WHAT THIS TREE BELIEVED

1. **The `shoot` inaccuracy is a triangle, not a Gaussian.**
   `getMovementToShoot` offsets 14-65 are three
   `random.triangle(0.0, 0.0172275 * inaccuracy)` calls and
   `triangle(d, e) = d + e * (nextDouble() - nextDouble())`. **Six
   `nextDouble` draws a shot, no `nextGaussian`, no divisor.** Zero inaccuracy
   still costs all six.
2. **`ProjectileUtil.computeMargin` ramps.**
   `max(0, min(0.3f, (tickCount - 2) / 20.0f))` — 0 at tickCount 2, 0.3 at 8.
   The flat 0.3 of every earlier version hits things vanilla misses.
3. **`57.2957763671875` is `(double)(180.0f / (float) Math.PI)`** and not
   either of the two obvious candidates.
4. **`lerpRotation(350, 10)` is -6, not 354**, and `lerpRotation(0, 180)` is
   324, not 36. Vanilla normalises `current` towards `target`, and the second
   loop is `>=`. Both are inputs where a `wrapDegrees(target - current)`
   rewrite differs, and both are in the golden's 225 pairs.

#### THE TICK ORDERS ARE DIFFERENT AND THE CONSTANTS ARE NOT SHARED

```text
  AbstractArrow.tick                    ThrowableProjectile.tick
    stick-if-inside-a-solid               handleFirstTickBubbleColumn
    shakeTime--                           applyGravity      0.03
    if (inGround) { fall? : despawn;      applyInertia      0.99 air / 0.8 water
                    return }              getHitResultOnMoveVector
    water drag 0.6                        setPos
    rotation FROM THE PRE-TICK DELTA      updateRotation
    checkLeftOwner                        Projectile.tick
    clip + stepMoveAndHit                 hitTargetOrDeflectSelf
    air drag 0.99
    gravity 0.05
    Projectile.tick
```

Three things a reading gets wrong. The arrow drags and falls **after** moving
and a throwable does both **before**. The arrow's rotation is taken from the
delta captured at the top of the tick, so it lags the motion by one tick. And
the grounded arm **returns**, so a stuck arrow gets neither drag nor gravity
and only its `life` counter moves.

#### NO LIVE CAPTURE IS REACHABLE, MEASURED BEFORE ANY SCENARIO WAS WRITTEN

The brief asked for a `/summon arrow` or a bow fired by the probe. **Neither
works on main**, and the measurement is:

* `crates/rustmc-server/src/console.rs:95 dispatch_line` handles exactly one
  verb, `gamerule`. `commands.rs` registers list/seed/say/help/msg/gamerule
  and nothing that creates an entity.
* `Step::Console` **does not exist** in `tools/oracle-runner/src/scenario.rs`
  on main — it is `origin/world-systems-2`'s and unmerged — so even a console
  command could not be scripted, and `console_restore` has nothing to guard.
* `world_send.rs` decodes `USE_ITEM_ON` and there is no `USE_ITEM` handler;
  `itemuse.rs` has no `releaseUsing` path. The scenario verbs are Hold, Look,
  Walk, Dig, FinishDig, Drop, Place, Attack, Sneak, ContainerClick,
  SignUpdate, Swing, Command, Say, Wait, Bucket — none of them holds and
  releases a use.

So **no `ci/checks/gameplay/*.toml` was registered and no
`gameplay-baseline.toml` row was added.** A scenario whose subject cannot emit
the packet it observes is BLIND-BY-POPULATION by construction, and registering
one would be a check that cannot go red — which is the defect
`docs/AGENT-BRIEF.md` opens with.

#### WHAT IS GATED, AND BY WHICH INSTRUMENT

| instrument | what it settles |
|---|---|
| `tests/projectile_golden.rs` (11) | **fidelity**, bit-exact, for everything reachable without a `Level` |
| `tests/projectile_flight.rs` (18) | **self-consistency** of the whole-tick assembly; says so in its own header |
| the modules' 39 unit tests (T0) | constants, draw counts, the rotation seam, the refusals |

`projectile_flight` separates six failure modes, each with a positive control:
the block ray, the entity sort, the 26.2 margin ramp (a flat 0.3 passes BOTH
arms, which is why that case exists), the `inGround` machine, the despawn
counter and the piercing loop.

#### REFUSED, WITH THE CLASS AND THE OWNER

* **Damage.** `AbstractArrow.onHitEntity` ends in
  `target.hurtServer(level, damageSources().arrow(this, owner), damage)`.
  `Tick::hits` reports the target and the figure
  `Mth.ceil(clamp(length * baseDamage, 0, 2.147483647E9))` and applies
  nothing. **The crit bonus `random.nextInt(dmg / 2 + 2)` is deliberately not
  drawn**, because advancing the shared RNG for a number nothing applies is
  worse than not having it. `mob-combat`'s.
* **Tipped-arrow effects.** `Arrow.doPostHurtEffects` walks the potion
  contents component; `data/vanilla` carries no potion table.
* **`AbstractThrownPotion.onHit`** — same reason. The splash potion is ticked
  and is **not discarded** on landing: a projectile that vanished without
  doing its work is a worse lie than one visibly not implemented, and
  `the_refused_kinds_do_not_vanish` pins that with a positive control.
* **`ThrownEnderpearl.onHit`** — `ServerPlayer.teleportTo`,
  `resetFallDistance` and a five-damage `hurtServer`. player-systems'. Same
  no-discard rule.
* **Multishot's fan.** `CrossbowItem.shootProjectile` rotates the view vector
  with `org.joml.Quaternionf.setAngleAxis` and `Vector3f.rotate`, in float.
  `crossbow_shot_direction` handles `angle == 0` (identity quaternion, the
  view vector exactly) and returns `None` otherwise.
* **`ProjectileDeflection`.** The only deflections a vanilla arrow can suffer
  are `AIM_DEFLECT` from a redirectable projectile and the wind charge's;
  neither entity exists here, so `hitTargetsOrDeflectSelf` is transcribed as
  the all-`NONE` path with the argument written at the line.
* **The per-player broadcast view.** `entity-tracker`'s.
* **`clipWithInteractionOverride`.** `getInteractionShape`'s
  `BlockBehaviour` default is `Shapes.empty()` and the four overriding blocks
  are ones no projectile here can reach.

#### JOIN POINTS, NAMED

* **`mob-in-world`** (`origin/mob-in-world`, unmerged) — a mob is the first
  thing `hit::Target` can be filled from. The trait takes a box, a position
  and three booleans and nothing else, so the join is a `ProjectileTargets`
  impl over that branch's store.
* **`entity-tracker`** (`origin/entity-tracker`, unmerged) — which clients get
  the `add_entity`. `projectile/wire.rs` has the packet's `data` field and the
  six `EntityKind` rows and nothing that broadcasts.
* **`items`** — `BowItem.releaseUsing`'s ammunition (`Player.getProjectile`,
  `draw`, `useAmmo`, `hurtAndBreak`). `bow::bow_release` returns the power,
  velocity, inaccuracy, crit flag and sound pitch and consumes nothing.
* **The entity tick loop.** `Arrow::tick` takes `&CollisionShapes`, a
  `CollisionBlocks`, a `ProjectileTargets`, a `RandomSource` and an `Env`, and
  returns a `Tick` of sounds, hits, a stuck position and a discard flag. It
  has no caller in this server. That is the same criticism `movement-tick`
  wrote about itself and it is said here rather than discovered.

#### NULLS — DO NOT PAY FOR THESE TWICE

* **Do not look for the `shoot` Gaussian.** It is not in 26.2. Six
  `nextDouble`s.
* **Do not build a `MoveLevel` to golden `AABB.clip`, `VoxelShape.clip`,
  `traverseBlocks`, `lerpRotation`, `getPowerForTime` or `getShotPitch`.**
  All six are reachable with none, and `ci/projectile-golden.sh` already does.
* **Do not put a golden harness in one of vanilla's packages if it calls
  `Bootstrap`.** `SecurityException: signer information does not match`. Use
  your own package; the unsigned-jar dance is only needed for package-private
  access.
* **Do not write a gameplay scenario for a projectile until something can
  spawn one.** The three blocked routes are measured above.
* **Do not add the projectile `EntityKind`s to `crate::entities::KINDS`.**
  That list is "every kind this server can construct" and is enumerated by a
  test; a dead kind in it makes that test pass by counting dead ones.
  `no_projectile_kind_is_in_the_servers_constructible_list` fails loudly when
  a spawn path lands and tells you to delete it.

#### NEXT ITEM, AND THE RESULT THAT CLOSES IT

**A `ProjLevel` in the shape of `EntityMoveGolden`'s `MoveLevel`, so that
`AbstractArrow.tick`, `ProjectileUtil.computeMargin` and
`getManyEntityHitResult` can be run by vanilla itself.** Those three are the
whole of what `tests/projectile_flight.rs` grades against this lane's own
reading, and the 26.2 margin ramp — the single most behaviourally visible
finding here — has no fidelity gate at all until it exists.

What closes it: a golden of `AbstractArrow.tick`'s position, delta, rotation,
`inGround`, `shakeTime` and `life` over N ticks on the same layouts, plus
`computeMargin` over a tickCount sweep, reproduced bit-exact by
`crates/rustmc-server/src/projectile/arrow.rs`. `MoveLevel` already answers
`getBlockState`, `getEntities` and `getWorldBorder`; `AbstractArrow.tick`
additionally reaches `isInWaterOrRain`, `isInWater`, `applyEffectsFromBlocks`,
`clipIncludingBorder`, `noCollision`, `addParticle` and `playSound`, and every
one of those is answerable by name or throwable by name in that style.

The second item, cheaper and independent: **a spawn path**. Either a
`/summon` in `console.rs` with its `console_restore` once
`origin/world-systems-2` lands `Step::Console`, or `USE_ITEM` decoded in
`world_send.rs` and wired to `bow::bow_release`. Either one turns every
refusal above into a measurable thing and makes the first gameplay scenario
possible.
