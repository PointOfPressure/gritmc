# Cold start — branch `mobs-goals`

### COLD START — branch `mobs-goals` off `f4047ae1f`, 2026-09-04

**The goal system and ground navigation of vanilla 26.2's `Mob` are in
`crates/rustmc-entity`, transcribed from the jar's bytecode, and nothing in
`rustmc-server` calls them yet.** Worktree `~/grit/work-mobs`, remote dir
`~/rustmc-mobs-goals` on the Threadripper. Not merged — the orchestrator
gates. 36 commits, `cargo test -p rustmc-entity`: 110 unit + 19 integration
tests green, T0 green after the test-target manifest was taught the four new
targets.

    GOAL SYSTEM   Goal, Goal$Flag, WrappedGoal, GoalSelector
    MOB HOOKS     serverAiStep, tick/updateControlFlags, tickHeadTurn, lookAt, rotlerp
    CONTROLS      Control, LookControl, MoveControl(+Operation), JumpControl, BodyRotationControl, Sensing
    NAVIGATION    PathNavigation + GroundPathNavigation (one struct)
    PATHFINDER    PathType, Node, Target, BinaryHeap, Path, NodeEvaluator, WalkNodeEvaluator,
                  PathFinder, PathfindingContext, PathTypeCache
    RANDOM POS    RandomPos, LandRandomPos, DefaultRandomPos, GoalUtils
    TARGETING     TargetingConditions, ServerEntityGetter.getNearestEntity/getNearestPlayer/getNearbyEntities
    GOALS (13)    RandomLookAround, LookAtPlayer, RandomStroll, WaterAvoidingRandomStroll, Float, Panic,
                  MeleeAttack, TargetGoal, NearestAttackableTarget, HurtByTarget, Tempt, Breed,
                  FollowParent, AvoidEntity
    MTH           wrapDegrees, degreesDifference, rotateIfNecessary, approach(Degrees), atan2 with
                  the JVM's two 257-entry tables embedded

**Golden-checked against the jar (bit for bit):**

* `PathFinder` + `BinaryHeap` + `Node` + `Target` + `Path` — 15 grids
  including 15-target hash-walk ties, a 200-node heap replay with
  changeCost/remove, `createHash` on 16 positions. `ci/mob-path-golden.sh`,
  `tests/golden_mob_path.rs`. The jar is driven with a grid `NodeEvaluator`
  and no server; the golden lives in package `rustmc.golden` because the
  jar's `net.minecraft.*` packages are signed and a class placed in one
  fails at load.
* `Mth.atan2` on 2014 inputs, `wrapDegrees`, `rotateIfNecessary`,
  `approachDegrees`, `degreesDifference`, `positiveCeilDiv`, `Mth.sqrt`.
  `ci/mob-math-golden.sh`, `tests/golden_mob_math.rs`. **The first run was
  red: glibc's `cos(asin(61/256))` is one ulp from fdlibm's**, so the tables
  are the JVM's bits (`src/mth_tables.rs`) and the test asserts they still
  match the vectors.

**Deferred goldens, each named in a test rather than approximated:**
`WalkNodeEvaluator` (`tests/ground_path.rs::walk_node_evaluator_golden_needs_a_running_server`)
and `PathNavigation` (`tests/navigation.rs::path_navigation_golden_needs_a_running_server`)
need a `Mob` in a `Level`; every goal, control and the selectors are
tested against `testing::TestMob` / `GridLevel` fixtures answering
`MobHost` / `PathLevel` from fields, with each RNG draw replayed by seed.
Those tests are this port's reading, not the jar's.

**Shape, for whoever wires a mob:**

* `host::MobHost` is every `Mob`/`LivingEntity`/`Entity` call the AI makes,
  one method each, base-class bodies as defaults; `host::PathLevel` is the
  `Level`/`BlockGetter` surface with block-state predicates as
  `BlockFacts` bits. `ai::MobAi<M>` owns the host and `Mob`'s AI fields;
  goals are `Goal<MobAi<M>>`. `MobAi::server_ai_step()` is `Mob.serverAiStep`.
* The `PathNavigationRegion` snapshot is a read-through on the level; the
  region's out-of-bounds answer is not reproduced (deferred, named in
  `ai/navigation.rs`).
* `getEntitiesOfClass` order is the host's; `lib.rs` already records that
  this crate's query order is not vanilla's section-then-insertion order.
* `Level.clip` for line of sight and `noCollision` are host calls; the grid
  fixture approximates both for tests only.

**Findings** (all in `docs/FINDINGS.md`): the signed jar package; the
`COS_TAB[61]` ulp; `PathFinder` runs without a server; stroll targets are
the first stable candidate of ten (strict `>` over a constant
`getWalkTargetValue`); `moveTo(x, y, z, speed)` stops one node short
(accuracy 1); `LookAtPlayerGoal.canUse` assigns the mob's target and then
overwrites it.

**Needs hands:** nothing in this branch. The next lane on this area writes
the server-side `MobHost` over the entity store, and the remaining ~40 goal
classes, most-shared first (`docs/EXTRACTABILITY.md` §2.2 on
`origin/extract-classify`). Brains are a separate area.
