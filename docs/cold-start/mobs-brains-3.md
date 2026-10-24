# Cold start — branch `mobs-brains-3`

### COLD START — branch `mobs-brains-3` off `origin/mobs-brains-2` (`c182b36a6`), 2026-09-05

**The brain-table ledger's scope is now all twenty providers, not
villager + piglin, and 30 of the 56 classes that widening exposed are
transcribed along with every one of the 13 pending sensors:
`tests/brain_tables.rs::PENDING_SENSORS` went from 13 to **0** and
`PENDING` from 56 to **26**, out of 131 behaviour classes the twenty
packages name.** Worktree `~/grit/work-brains3`, remote dir
`~/rustmc-brains3` on the Threadripper. Not merged — the orchestrator
gates. 17 commits over `mobs-brains-2`. `cargo test -p rustmc-entity`:
367 unit + integration tests green, `cargo clippy -p rustmc-entity
--all-targets -- -D warnings` clean; T0 chained through
`ci/run-detached.sh`.

    LEDGER        the scope rule is the jar's descriptors, not a path prefix: a `new` of any
                  `world/entity/` class outside `ai/behavior/declarative`, or a `call` whose owner
                  is under `ai/behavior/` or whose descriptor returns BehaviorControl/OneShot/
                  Behavior/Trigger. 131 classes named, 87 sensor slots, 23 distinct SensorTypes.
                  Both halves falsified (drop a TRANSCRIBED entry; list a built sensor as pending)
    SENSORS (13)  AdultSensor + AdultSensorAnyType, TemptingSensor (x3 SensorTypes), IsInWaterSensor,
                  AxolotlAttackablesSensor, FrogAttackablesSensor, WardenEntitySensor +
                  BreezeAttackEntitySensor, HoglinSpecificSensor, PiglinBruteSpecificSensor,
                  MobSensor (ARMADILLO_SCARE_DETECTED). `brain::sensors::create` is now EXHAUSTIVE
                  with no wildcard arm, so deleting a sensor is a compile error
    BEHAVIOURS    AnimalMakeLove; LongJumpUtil + LongJumpToRandomPos (+LongJumpToPreferredBlock) +
    (30)          LongJumpMidJump; the piglin's six locals (StartAdmiringItemIfSeen,
                  StopHoldingItemIfNoLongerAdmiring, StopAdmiringIfItemTooFarAway,
                  StopAdmiringIfTiredOfTryingToReachItem, RememberIfHoglinWasKilled,
                  StartHuntingHoglin); the warden's nine (Emerging, Digging, Sniffing, Roar,
                  SonicBoom, SetRoarTarget, SetWardenLookTarget, TryToSniff, ForceUnmount);
                  Croak, TryFindLand, TryFindLandNearWater, TryFindWater,
                  TryLaySpawnOnFluidNearLand; BecomePassiveIfMemoryPresent, RandomLookAround,
                  StayCloseToTarget; axolotl PlayDead + ValidatePlayDead
    INFRA         `mth::direction_from_rotation` (NOT calculateViewVector — see below);
                  `UniformInt::sample_mob` beside `sample_level`;
                  `brain::sensor::attack_target_id`; `host::Pose` (12 values);
                  `host::Temptations`; ~35 new `MobHost` calls with base defaults
    LEDGER FIX    `EntityTracker` was already transcribed — it is `PositionTracker::Entity` in
                  `brain/memory.rs`; the descriptor rule names it only because `AllayAi` builds one

**Checked against the jar:** every class carries its offsets and draw
order in its doc comment and a test on `testing::TestMob` / `GridLevel`.
`ci/brain-tables.sh` is unchanged and still passes (this branch adds no
table). **Refuted from the tree:** `Sensor.isEntityAttackable` must not
throw on an unregistered `ATTACK_TARGET` — it goes through
`Brain.isMemoryValue`, whose `checkMemory` answers `false` — and the
fourth lane's "every class the piglin package names" was true of its
scope and not of the package (six classes in `monster/piglin/`).

**Deferred goldens, each named in the class rather than approximated:**
no class here has a jar golden, for the reason the earlier lanes gave —
every one needs a `LivingEntity` in a `ServerLevel`.
`LongJumpUtil.calculateJumpVectorForAngle` is the one pure function in
the set and it carries a **second independent transcription of the same
bytecode** as its test twin, which is not a jar golden and says so.

**Unverifiable by construction and said so in the code:**
`Collections.shuffle(ALLOWED_ANGLES)` in
`LongJumpToRandomPos.calculateOptimalJumpVector` (unseeded
`java.util.Random`; a caller-seeded `LegacyRandomSource` stands in, as
`InsideBrownianWalk`'s shuffle does).

**Nulls, so nobody re-runs them:** `warden.Sniffing.start` sets **no**
pose (`TryToSniff` set it); `PlayDead` has no `tick` and no `stop`;
`Croak`'s 100-tick duration is unreachable; `MobSensor` with an absent
`NEAREST_LIVING_ENTITIES` is a **no-op**, not a clear;
`BecomePassiveIfMemoryPresent` pacifies with no attack target present.

**Needs hands:** nothing in this branch. What is left in `PENDING` (26),
in the order a next lane should take them: the **sniffer's nine** inner
state classes (`SnifferAi$1/2/3/Digging/FeelingHappy/FinishedDigging/
Scenting/Searching/Sniffing`), the **breeze's five**
(`Shoot`, `ShootWhenStuck`, `Slide`, `LongJump`,
`BreezeAi$SlideToTargetSink`), the **goat's two**
(`PrepareRamNearestTarget`, `RamTarget` — both large, with a
`RamCandidate` record and a `TargetingConditions`), the **armadillo's
three** and the **camel's two** inner classes, `CreakingAi$1`,
`animal/frog/ShootTongue`, `ChargeAttack` (nautilus),
`GoAndGiveItemsToTarget` (allay) and
`TransportItemsBetweenContainers` (copper golem). After those the
`*Ai` provider lambdas themselves — `AxolotlAi`, `FrogAi`, `WardenAi`
with its `AngerManagement`, and the rest — as data in the tables the
same way villager's are.
