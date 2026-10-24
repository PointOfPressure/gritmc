# Cold start — branch `mobs-brains-2`

### COLD START — branch `mobs-brains-2` off `origin/main` (`bd5be4a36`, the merged `mobs-brains`), 2026-09-05

**Every behaviour class the villager and piglin brain packages name is
transcribed from the 26.2 jar's bytecode, and so are the seven sensors
those two providers list: `tests/brain_tables.rs::PENDING` went from 41
entries to 0 and its positive control is now a floor on the classes the
packages name (68) plus every one of them being in `TRANSCRIBED`.**
Worktree `~/grit/work-mobs`, remote dir `~/rustmc-mobs` on the
Threadripper. Not merged — the orchestrator gates. 49 commits over main,
one per class. `cargo test -p rustmc-entity`: 309 unit + 6 integration
tests green, `cargo clippy -p rustmc-entity --all-targets -- -D warnings`
clean; T0 chained through `ci/run-detached.sh mobs-brains-2-t0`.

    BEHAVIOURS    AcquirePoi (+JitteredLinearRetry, findPathToPois, the Java HashSet<BlockPos> walk),
    (41, 8,596    AssignProfessionFromJobSite, BackUpIfTooClose, CelebrateVillagersSurvivedRaid,
    lines with    CopyMemoryWithExpiry, CrossbowAttack (+CrossbowState), DismountOrSkipMounting,
    the sensors)  GiveGiftToHero, GoToClosestVillage, GoToPotentialJobSite, InsideBrownianWalk (+Util.shuffle),
                  InteractWith, JumpOnBed, LocateHidingPlace, LookAndFollowTradingPlayerSink, Mount,
                  MoveToSkySeeingSpot, PlayTagWithOtherKids, PoiCompetitorScan, ReactToBell, ResetProfession,
                  ResetRaidStatus, RingBell, SetClosestHomeAsWalkTarget, SetHiddenState, SetRaidStatus,
                  ShowTradesToPlayer, SleepInBed, SocializeAtBell, SpearApproach, SpearAttack, SpearRetreat,
                  StartCelebratingIfTargetDead, TradeWithVillager, ValidateNearbyPoi, VillageBoundRandomStroll,
                  VillagerCalmDown, VillagerMakeLove, VillagerPanicTrigger, WakeUp, YieldJobSite
    SENSORS (7+1) NearestVisibleLivingEntitySensor (the abstract base), VillagerHostilesSensor,
                  VillagerBabiesSensor, NearestItemSensor, NearestBedSensor, SecondaryPoiSensor, GolemSensor,
                  PiglinSpecificSensor; `brain::sensors::create(SensorType)` is the factory and
                  `tests/brain_tables.rs` refuses a villager/piglin sensor slot it cannot build (14 slots)
    BEHAVIORUTILS canSee, lockGazeAndWalkToEachOther
    HOST          ~70 new `MobHost` calls with base defaults (POIs: `pois_in_range` in manager order,
                  `poi_type_at`, `take_poi`, `release_poi_at_global`; villager data, raids by id, beds,
                  bells, riding, items, trades, gossip, breeding, gifts, fireworks, the "other villager"
                  reads and writes); `EntityClass::Item`; `RaidFacts`, `VillagerDataFacts`,
                  `PoiRecordFacts`, `MerchantOfferFacts`; `TestMob` mirrors every one

**Checked against the jar:** nothing new in the data tables (unchanged);
every class carries its offsets and draw order in its doc comment and a
test on `testing::TestMob` / `GridLevel` that replays each RNG draw by
seed with a twin — this port's reading of the bytecode, not the jar's
output. **Refuted from the tree:** `SpearStatus`'s middle constant is
`CHARGING`, not `Attack`; `Behavior.tryStart`'s duration draw wraps in
Java int arithmetic and the port overflowed on `Integer.MAX_VALUE`
(`docs/FINDINGS.md`, 2026-09-05, both fixed on this branch).

**Deferred goldens, each named in the class rather than approximated:**
none of the 41 behaviours or 7 sensors has a jar golden, for the reason
the earlier lanes gave — every one needs a `LivingEntity` in a
`ServerLevel`, and the POI ones a `PoiManager`. **Unverifiable by
construction and said so in the code:** `InsideBrownianWalk`'s
`Collections.shuffle` (unseeded `java.util.Random`; a caller-seeded
`LegacyRandomSource` stands in), `AcquirePoi`'s post-failure walk of a
`HashSet<Pair<Holder, BlockPos>>` and `NearestBedSensor`'s set of the same
shape (identity-hashed holders), `PlayTagWithOtherKids`'s tie order in
a `HashMap<LivingEntity, Integer>`. The `HashSet<BlockPos>` handed to
`createPath` is reproduced in Java iteration order (`Vec3i.hashCode`,
`HashMap.hash` spread, 16 buckets doubling past three quarters).

**Shape, for whoever wires the villager:** the POI manager is a host
concern — `pois_in_range(center, radius, occupancy)` answers in the
manager's own order (`ChunkPos.rangeClosed` then records) and the
behaviours apply the type and position predicates in the jar's order on
top of it, because `AcquirePoi`'s position predicate draws the level
random per record *before* the closest-first sort. Cross-entity work
(`YieldJobSite.nearbyWantsJobsite` with its path search on the other
villager, `lockGazeAndWalkToEachOther`'s other half, breeding's child)
is `other_*` host calls, one per jar operation. `raid_at` returns
`RaidFacts` with an id so `CelebrateVillagersSurvivedRaid` can re-read
the live raid. `ItemStack.EMPTY` is `None`; `isSameItem` compares item
ids.

**Nulls, so nobody re-runs them:** `NearestBedSensor` is babies-only by
its first instruction, so no adult ever has `NEAREST_BED`; a
`Behavior` with a fixed duration still draws `nextInt(1)` on the level
random (unchanged from `mobs-brains`); `RingBell` rings 5 % of the time
by its `> 0.95f` comparison — do not "fix" it to 95 %.

**Needs hands:** nothing in this branch. Next on this area: a
`Brain::make` of the villager and the piglin from
`golden/mob_brain_tables.json` (every class they name now exists; the
provider's `ActivitySupplier` lambdas are the remaining transcription),
then the server-side `MobHost` over the entity store, which now has
~250 calls to answer.
