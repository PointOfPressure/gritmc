# Cold start — branch `mobs-brains`

### COLD START — branch `mobs-brains` off `origin/mobs-goals-2` (`0f64393cc`), 2026-09-05

**Vanilla 26.2's `Brain` system is in `crates/rustmc-entity/src/brain/`,
transcribed from the jar's bytecode: the brain itself, its memory,
activity, behaviour, sensor and gate machinery, the villager schedule as
the timeline data it now is, 32 behaviour classes, 4 sensors, and the 20
brain providers plus 174 activity/behaviour package methods of every
brain mob as data.** Worktree `~/grit/work-mobs`, remote dir
`~/rustmc-mobs` on the Threadripper. Not merged — the orchestrator gates;
this branch sits on `mobs-goals-2` (51 commits, plus one `cargo fmt` fix
this lane pushed to `origin/mobs-goals-2` so its T0 could go green) and
expects a rebase when that lands. 47 commits. `cargo test -p rustmc-entity`:
255 unit + 25 integration tests green, `cargo clippy -p rustmc-entity
--all-targets -- -D warnings` clean, T0 GREEN on the 28-commit tip
(`~/grit/logs/mobs-brains-t0-2.log`); the final tip's T0 is chained through
`ci/run-detached.sh mobs-brains-t0`.

    INFRA         Brain (+BrainState), MemoryModuleType x116 / MemoryStatus / MemorySlot / WalkTarget /
                  PositionTracker / NearestVisibleLivingEntities, Activity x26 + ActivityData, Behavior /
                  BehaviorControl / OneShot / Trigger, Sensor / SensorType x24, ShufflingList / GateBehavior /
                  RunOne / TriggerGate, Schedule (villager_schedule.json)
    BEHAVIOURS    LookAtTargetSink, MoveToTargetSink, SetWalkTargetFromLookTarget, RandomStroll, DoNothing,
    (32)          Swim, UpdateActivityFromSchedule, SetEntityLookTarget, SetEntityLookTargetSometimes (+Ticker,
                  UniformInt), StartAttacking, MeleeAttack, SetWalkTargetFromAttackTargetIfTargetOutOfReach,
                  StopAttackingIfTargetInvalid, InteractWithDoor, BehaviorUtils, CountDownCooldownTicks,
                  EraseMemoryIf, StopBeingAngryIfTargetDead, GoToTargetLocation, SetLookAndInteract,
                  GoToWantedItem, SetWalkTargetAwayFrom, BabyFollowAdult, StrollToPoi, StrollAroundPoi,
                  SetWalkTargetFromBlockMemory, AnimalPanic, FollowTemptation
    SENSORS (4)   DummySensor, HurtBySensor, NearestLivingEntitySensor, PlayerSensor
    TABLES        ci/gen-brain-tables.py -> crates/rustmc-entity/golden/mob_brain_tables.json
                  (20 providers with memory/sensor lists in push order; 174 package methods folded into
                  new/call/lambda/array trees, 35 branching ones kept as raw tokens); ci/brain-tables.sh
                  refuses drift; tests/brain_tables.rs refuses an untranscribed villager/piglin behaviour
                  (32 transcribed, 40 pending, none unknown) and pins the villager's 9 sensors and the
                  piglin's 6 memories + 5 sensors

**Checked against the jar:** the two data tables (`ci/brain-tables.sh`,
about 20 s, and the villager schedule constants against
`golden/villager_schedule.json`, the jar's own data file). **Refuted from
the brief by the tree:** 26.2 has no `Schedule` class — `Brain.schedule`
is an `EnvironmentAttribute<Activity>` and the villager day is timeline
data (`docs/FINDINGS.md`, 2026-09-05); `Brain$Provider` replaced the
per-mob `makeBrain` bodies with `ActivitySupplier` lambdas, and the
villager's is the two-argument provider with no memory list at all.

**Deferred goldens, each named in the class rather than approximated:**
none of the 32 behaviours or 4 sensors has a jar golden, for the reason
the goal lanes gave — every one needs a `LivingEntity` in a `ServerLevel`.
Each carries its offsets and draw order in its doc comment and a test on
`testing::TestMob` / `GridLevel` that replays every RNG draw by seed with
a twin (this port's reading, not the jar's). Unverifiable by construction
and said so in the code: every `SHUFFLED` gate's order (`ShufflingList`
seeds its own `RandomSource.create()`), `DOORS_TO_CLOSE`'s walk order (an
identity-hashed `ResourceKey` inside the record hash), and `Math.pow` in
the shuffle key (1-ulp intrinsic).

**Shape, for whoever wires a brain mob:** `Brain<M>` is a struct beside
`MobAi<M>`, ticked by the mob's `customServerAiStep`; behaviours and
sensors take `(&mut BrainState, &mut MobAi<M>, time)`. `Brain::make` is
`Brain$Provider.makeBrain` (memories, sensors with their `nextInt(scanRate)`
delay draws in order, activities, packed memories, `CORE` + default
activity). The three orders that are behaviour are reproduced: sensors in
add order, behaviours by `TreeMap` priority then the Java `HashMap` walk
of the activities' String hashes then add order, running behaviours
collected before any is ticked. `MobHost` grew ~20 calls (`day_time`,
`dimension`, `schedule_activity`, `entity_by_uuid`, `release_poi`,
`damage_type_is`, …), `BlockFacts` gained `is_mob_interactable_door`,
`GroundPathNavigation` gained `path_generation` for `MoveToTargetSink`'s
path identity test, `Path` gained `PartialEq`.

**Nulls, so nobody re-runs them:** none of the memory map's order is
observable (`memory.rs` says which three walks exist and why each is
order-free); a `Behavior` with a fixed duration still consumes one level
`nextInt(1)` per start, so a "no draw" shortcut would desync the level
random.

**Needs hands:** nothing in this branch. Next on this area, in order of
how many brain mobs share them: `AnimalMakeLove` (8), `SetWalkTargetFromBlockMemory`'s
villager siblings (`AcquirePoi`, `GoToPotentialJobSite`, `YieldJobSite`,
`ValidateNearbyPoi`), the remaining villager core/idle classes named in
`tests/brain_tables.rs::PENDING`, the villager and piglin sensors
(`VillagerHostilesSensor`, `VillagerBabiesSensor`, `NearestItemSensor`,
`NearestBedSensor`, `SecondaryPoiSensor`, `GolemSensor`,
`PiglinSpecificSensor`), then a `Brain::make` of the villager from the
tables. The server-side `MobHost` over the entity store is the other lane.
