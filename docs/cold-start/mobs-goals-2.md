# Cold start — branch `mobs-goals-2`

### COLD START — branch `mobs-goals-2` off `f4fa28ff8`, 2026-09-05

**Every goal class in vanilla 26.2's `net.minecraft.world.entity.ai.goal`
package that a mob registers directly is in `crates/rustmc-entity`,
transcribed from the jar's bytecode, and the 54 `registerGoals()` tables
plus the 81 attribute builders are data.** Worktree `~/grit/work-mobs`,
remote dir `~/rustmc-mobs-goals-2` on the Threadripper. Not merged — the
orchestrator gates. `cargo test -p rustmc-entity`: 176 unit + 22
integration tests green, `cargo clippy -p rustmc-entity --all-targets
-- -D warnings` clean.

    GOALS (+56)   LeapAtTarget, RangedAttack, ClimbOnTopOfPowderSnow, SitWhenOrderedTo,
                  MoveTowardsRestriction, FollowOwner (+TamableAnimal.tryToTeleportToOwner),
                  UseItem, RunAroundLikeCrazy, OcelotAttack, ResetUniversalAngerTarget,
                  NonTameRandomTarget, OwnerHurtByTarget, OwnerHurtTarget,
                  NearestHealableRaiderTarget, NearestAttackableWitchTarget,
                  DefendVillageTarget, TradeWithPlayer, TryFindWater, Swell, FleeSun,
                  RestrictSun, RandomStand, LookAtTradingPlayer, Interact, RandomSwimming,
                  WaterAvoidingRandomFlying, MoveBackToVillage, MoveTowardsTarget,
                  OfferFlower, BreathAir, ZombieAttack, TemptGoal.ForNonPathfinders,
                  MoveToBlock + CatSitOnBlock + CatLieOnBed + RemoveBlock,
                  DoorInteract + OpenDoor + BreakDoor, Beg, EatBlock, FollowFlockLeader,
                  LandOnOwnersShoulder, JumpGoal + DolphinJump, RangedBowAttack,
                  TamableAnimal.TamableAnimalPanicGoal, FollowPlayerRiddenEntity,
                  FollowMob, PathfindToRaid, LlamaFollowCaravan, StrollThroughVillage,
                  GolemRandomStrollInVillage, MoveThroughVillage, RangedCrossbowAttack,
                  SpearUse
    RANDOM POS    HoverRandomPos, AirAndWaterRandomPos, BehaviorUtils.getRandomSwimmablePos,
                  RandomPos.moveUpToAboveSolid, LandRandomPos.getPos with a scorer
    ENTITY MATH   Entity.calculateViewVector, getInputVector/moveRelative, Direction.fromYRot,
                  Mth.rotLerp, BlockPos.betweenClosed / neighborColumn orders,
                  java.util.HashSet<Entity> iteration order
    TABLES        ci/gen-mob-goal-tables.py -> crates/rustmc-entity/golden/mob_goal_tables.json
                  (54 registerGoals, 463 addGoal entries, 168 distinct goal classes of which
                  58 are goal-package classes and 110 mob-inner; 81 create*Attributes builders)

**Checked against the jar:** the tables. `ci/mob-goal-tables.sh` regenerates
them from the jar with `javap` and refuses on any byte of difference (about
four minutes; JDK 25); `tests/mob_goal_tables.rs` refuses a goal-package
class the jar registers that this crate has not transcribed, an entry whose
operand tokens do not match its constructor's parameter count, and the
table's shape. Every token in the JSON is a verbatim bytecode operand in
push order — `double 1.5d`, `float 8.0f`, `class .../Player`, `getstatic
Foo.BAR`, `lambda#0:test`, `this call level:()...` — so a later lane that
instantiates a mob from a row interprets them against the descriptor that
sits beside them and nothing here has guessed at a value.

**Deferred goldens, each named in the class rather than approximated:**
none of the 56 goal classes has a jar golden, for the same reason as the
first lane's 13 — every one needs a `Mob` in a `Level`. What there is
instead: each class carries its draw order and bytecode offsets in its
doc comment and one or more tests against `testing::TestMob` /
`GridLevel` that replay the RNG by seed with a twin `LegacyRandomSource`
(the reading of this port, not the jar's). Two specific unverified bits:
`DolphinJumpGoal.tick` uses `java.lang.Math.atan2` (an intrinsic, not
`Mth`'s table), ported as `f64::atan2`, last bit unverified; and
`PathfindToRaidGoal.recruitNearby` walks a `HashSet<Raider>`, reproduced
as `java_hash_set_order` (bucket `id & (n-1)`, table 16 doubling past
3/4, insertion order within a bucket, tree bins not modelled).

**Shape, for whoever wires a mob:** `host::MobHost` grew by about 170
methods, every one a named vanilla call with the base-class body as its
default (`TamableAnimal.unableToMoveToOwner`, `shouldTryTeleportToOwner`,
`Mob.isWithinHome()`, `moveRelative`, `viewVector`, ...) or a bare host
answer where the jar reads state this crate does not own (raids, caravans,
village POIs, item components). `BlockFacts` gained `is_pathfindable_water`,
`block_id`, beds/chests/furnaces, `is_edible_for_sheep`, `is_grass_block`,
`blocks_motion`; `PathLevel` gained `is_bright_outside`. `EntityClass`
gained `Villager`. Goals that subclass in the jar wrap the base struct
(`inner`) and re-implement only the overridden methods; `RandomStrollGoal`
carries a `StrollKind` for its six `getPosition` subclasses.

**Findings** (all in `docs/FINDINGS.md`): `CatLieOnBedGoal`'s
`verticalSearchStart = -2` skips its own level; `StrollThroughVillageGoal`
ends one tick after it starts; `NonTameRandomTargetGoal`'s `super` arm is
dead; `DefendVillageTargetGoal.potentialTarget` is never cleared;
`NearestHealableRaiderTargetGoal` never makes its 500-tick interval draw;
`RemoveBlockGoal`'s cooldown is halved to 10; `TemptGoal.ForNonPathfinders`
draws a `nextDouble` per navigating tick; `betweenClosed` walks x, y, z;
`neighborColumn` walks offset-major; `HoverRandomPos` lifts by
`nextInt(3) + 1`; the supervisor parks worktrees mid-run.

**Needs hands:** nothing in this branch. The remaining goal work is the
~110 mob-inner goal classes (`Wolf$WolfAvoidEntityGoal`, `Fox$...`, the
illager spells, ...) which belong with their mobs, and the server-side
`MobHost` over the entity store.
