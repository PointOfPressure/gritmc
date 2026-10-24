//! What the spawner asks of the level and of the server, as traits the
//! caller answers.
//!
//! Same split and same rule as [`crate::host`]: every method is one Java
//! call, named for it, and where the Java side has a body that body is the
//! default here. Nothing in this crate can answer any of them — there is no
//! level, no light engine and no mob — so a caller that wants natural
//! spawning has to supply all of it, which is the refusal being explicit
//! rather than a stub returning a plausible value.

use rustmc_data::BlockPos;
use rustmc_worldgen::rng::RandomSource;

use crate::phys::Aabb;

/// The two draws the spawner and its predicates make, object-safe so a
/// `&mut dyn` can be threaded through a trait method.
///
/// Every draw in this module is on **one** source — `ServerLevel.random`,
/// the level's shared unseeded stream — so the count matters as much as
/// the values and a caller must never hand out a second generator.
pub trait SpawnRandom {
    /// `RandomSource.nextInt(bound)`.
    fn next_i32_bound(&mut self, bound: i32) -> i32;
    /// `RandomSource.nextFloat()`.
    fn next_f32(&mut self) -> f32;
    /// `Mth.randomBetweenInclusive(random, min, maxInclusive)` —
    /// `nextInt(max - min + 1) + min` in wrapping int arithmetic.
    fn next_i32_between_inclusive(&mut self, min: i32, max_inclusive: i32) -> i32 {
        self.next_i32_bound(max_inclusive.wrapping_sub(min).wrapping_add(1))
            .wrapping_add(min)
    }
}

impl<R: RandomSource> SpawnRandom for R {
    fn next_i32_bound(&mut self, bound: i32) -> i32 {
        RandomSource::next_i32_bound(self, bound)
    }
    fn next_f32(&mut self) -> f32 {
        RandomSource::next_f32(self)
    }
    fn next_i32_between_inclusive(&mut self, min: i32, max_inclusive: i32) -> i32 {
        RandomSource::next_i32_between_inclusive(self, min, max_inclusive)
    }
}

/// `net.minecraft.world.Difficulty`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Difficulty {
    Peaceful,
    Easy,
    Normal,
    Hard,
}

/// `net.minecraft.world.level.LightLayer`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightLayer {
    Sky,
    Block,
}

/// `net.minecraft.world.entity.EntitySpawnReason`, the values the spawn
/// predicates branch on.
///
/// `isSpawner(reason)` is `reason == SPAWNER || reason == TRIAL_SPAWNER`
/// and `ignoresLightRequirements(reason)` is `reason == TRIAL_SPAWNER`
/// **only** — the two are not the same test and three predicates use both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntitySpawnReason {
    Natural,
    Chunk,
    SpawnItemUse,
    Spawner,
    TrialSpawner,
    Structure,
    Breeding,
    MobSummoned,
    JockeyLegacy,
    Command,
    Reinforcement,
    Event,
    Conversion,
    Dispenser,
    Patrol,
    Bucket,
    Load,
}

impl EntitySpawnReason {
    /// `EntitySpawnReason.isSpawner`.
    #[must_use]
    pub const fn is_spawner(self) -> bool {
        matches!(self, Self::Spawner | Self::TrialSpawner)
    }

    /// `EntitySpawnReason.ignoresLightRequirements` — `TRIAL_SPAWNER`
    /// alone. A plain `SPAWNER` still obeys the light rules.
    #[must_use]
    pub const fn ignores_light_requirements(self) -> bool {
        matches!(self, Self::TrialSpawner)
    }
}

/// The `BlockState` facts the spawner tests, one bit each.
///
/// `NaturalSpawner.isValidEmptySpawnBlock` and the four
/// `SpawnPlacementTypes` between them read exactly these. They are facts
/// about a block and not about a spawn, so a level answers them once per
/// position and the transcription does no block lookup of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SpawnBlockFacts {
    /// `BlockState.isRedstoneConductor(level, pos)` — the very first gate
    /// in `spawnCategoryForPosition`, on the chunk's *drawn* position.
    pub is_redstone_conductor: bool,
    /// `BlockState.isCollisionShapeFullBlock(level, pos)`.
    pub is_collision_shape_full_block: bool,
    /// `BlockState.isSignalSource()`.
    pub is_signal_source: bool,
    /// `FluidState.isEmpty()`.
    pub fluid_is_empty: bool,
    /// `FluidState.is(FluidTags.WATER)`.
    pub fluid_is_water: bool,
    /// `FluidState.is(FluidTags.LAVA)`.
    pub fluid_is_lava: bool,
    /// `BlockState.is(BlockTags.PREVENT_MOB_SPAWNING_INSIDE)`.
    pub prevents_mob_spawning_inside: bool,
    /// `BlockState.is(Blocks.WATER)` — a **block** identity test, which
    /// `WaterAnimal.checkSurfaceWaterAnimalSpawnRules` uses on the block
    /// above and which is not the same question as `fluid_is_water`.
    pub is_water_block: bool,
    /// `BlockState.is(BlockTags.ANIMALS_SPAWNABLE_ON)`, for
    /// `Animal.checkAnimalSpawnRules`.
    pub animals_spawnable_on: bool,
    /// `BlockState.isPathfindable(PathComputationType.LAND)`, for
    /// `SpawnPlacementTypes.ON_GROUND.adjustSpawnPosition`.
    pub is_pathfindable_land: bool,
}

/// `net.minecraft.world.level.storage.LevelData$RespawnData`, the two
/// fields `isRightDistanceToPlayerAndSpawnPoint` reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RespawnData {
    pub dimension: rustmc_data::Dimension,
    pub pos: BlockPos,
}

/// The `Level` / `LevelReader` / `ServerLevelAccessor` surface.
pub trait SpawnLevel {
    /// `ServerLevel.getMinY()`.
    fn min_y(&self) -> i32;

    /// `LevelAccessor.getSeaLevel()`.
    fn sea_level(&self) -> i32;

    /// `ServerLevel.dimension()`.
    fn dimension(&self) -> rustmc_data::Dimension;

    /// `ServerLevel.getRespawnData()`.
    fn respawn_data(&self) -> RespawnData;

    /// `ServerLevelAccessor.getDifficulty()`.
    fn difficulty(&self) -> Difficulty;

    /// `ServerLevel.isThundering()`.
    fn is_thundering(&self) -> bool;

    /// `LevelChunk.getHeight(Heightmap.Types.WORLD_SURFACE, x, z)` — the
    /// heightmap `getRandomPosWithin` reads, which is **`WORLD_SURFACE`**
    /// and not the per-type one `SpawnPlacements` names.
    fn world_surface_height(&self, x: i32, z: i32) -> i32;

    /// [`SpawnBlockFacts`] at a position.
    fn block_facts(&self, pos: BlockPos) -> SpawnBlockFacts;

    /// `ServerLevel.getBiome(pos)` — the biome registry name.
    fn biome(&self, pos: BlockPos) -> String;

    /// `NaturalSpawner.getRoughBiome(pos, chunk)`:
    /// `chunk.getNoiseBiome(QuartPos.fromBlock(x), …y, …z)`. **Not the
    /// same call as [`Self::biome`]** — it reads the chunk's noise biome
    /// at quart resolution and skips the biome-blending the level applies,
    /// which is why `SpawnState` and `getRandomSpawnMobAt` can disagree
    /// about which biome a position is in.
    fn rough_biome(&self, pos: BlockPos) -> String;

    /// `ServerLevel.getNearestPlayer(x, y, z, -1.0, false)` — the exact
    /// call `spawnCategoryForPosition` makes: no distance limit, and
    /// spectators **included** (`false` is `entityPredicate`'s
    /// `EntitySelector.NO_SPECTATORS` being off). Returns the player's
    /// position, which is all the caller uses.
    fn nearest_player(&self, x: f64, y: f64, z: f64) -> Option<(f64, f64, f64)>;

    /// `ServerLevel.canSpawnEntitiesInChunk(ChunkPos)`.
    fn can_spawn_entities_in_chunk(&self, chunk_x: i32, chunk_z: i32) -> bool;

    /// `ServerLevel.noCollision(AABB)`.
    fn no_collision(&self, aabb: Aabb) -> bool;

    /// `LevelReader.getWorldBorder().isWithinBounds(pos)`.
    fn is_within_world_border(&self, pos: BlockPos) -> bool;

    /// `ServerLevelAccessor.getBrightness(LightLayer, pos)`.
    fn brightness(&self, layer: LightLayer, pos: BlockPos) -> i32;

    /// `ServerLevelAccessor.getMaxLocalRawBrightness(pos)` and its
    /// `(pos, amount)` overload — one method, because the no-argument form
    /// is the same call with the level's own skylight subtraction.
    fn max_local_raw_brightness(&self, pos: BlockPos, amount: Option<i32>) -> i32;

    /// `BlockAndLightGetter.getRawBrightness(pos, amount)`, for
    /// `Animal.isBrightEnoughToSpawn`.
    fn raw_brightness(&self, pos: BlockPos, amount: i32) -> i32;

    /// `ServerLevelAccessor.canSeeSky(pos)`.
    fn can_see_sky(&self, pos: BlockPos) -> bool;

    /// `DimensionType.monsterSpawnBlockLightLimit()`.
    fn monster_spawn_block_light_limit(&self) -> i32;

    /// `DimensionType.monsterSpawnLightTest().sample(random)` — an
    /// `IntProvider`, so it **draws**, and the draw is inside
    /// `isDarkEnoughToSpawn`'s sequence. The level owns the provider, so
    /// it owns the draw; a caller that answers this with a constant has
    /// changed the stream and must say so.
    fn sample_monster_spawn_light_test(&self, rng: &mut dyn SpawnRandom) -> i32;

    /// `BlockState.isValidSpawn(level, pos, type)` — the block-below test
    /// in `Mob.checkMobSpawnRules` and `ON_GROUND`. It dispatches on the
    /// block class (`SpawnerBlock`, `LeavesBlock`, the two `Wall`s, …) and
    /// so is the level's, not this crate's.
    fn is_valid_spawn(&self, pos: BlockPos, kind: &str) -> bool;

    /// `EntityType.isBlockDangerous(state)` — the `immuneTo` tag test in
    /// `NaturalSpawner.isValidEmptySpawnBlock`. Needs the block tags, so
    /// it is the level's.
    fn is_block_dangerous(&self, pos: BlockPos, kind: &str) -> bool;

    /// `NaturalSpawner.isInNetherFortressBounds` — MONSTER only, block
    /// below is `nether_bricks`, and a valid `minecraft:fortress`
    /// `StructureStart` at the position. Needs the structure manager.
    fn in_nether_fortress_bounds(&self, pos: BlockPos) -> bool;

    /// `NetherFortressStructure.FORTRESS_ENEMIES` — the hardcoded
    /// `WeightedList` `mobsAt` returns inside a fortress, in place of the
    /// biome's. Left to the caller because it is a `WeightedList` built in
    /// a structure class rather than a row in any table this crate loads.
    fn fortress_enemies(&self) -> &super::tables::WeightedSpawnList;

    /// `Holder<Biome>.is(BiomeTags.REDUCED_WATER_AMBIENT_SPAWNS)` at a
    /// position — the tag `getRandomSpawnMobAt` checks before its 98%
    /// refusal draw. A biome tag, so the level's.
    fn biome_has_reduced_water_ambient_spawns(&self, pos: BlockPos) -> bool;

    /// `ChunkGenerator.getMobsAt`'s first half: walk
    /// `StructureManager.getAllStructuresAt(pos)` for a
    /// `StructureSpawnOverride` registered for this category, honouring
    /// its `BoundingBoxType` (`PIECE` tests the piece boxes, `STRUCTURE`
    /// the whole start), and return that structure's `WeightedList`.
    ///
    /// **Refused in this crate.** It needs a live `StructureManager` and
    /// the structure registry's `spawn_overrides`, neither of which
    /// `rustmc-entity` has. A caller with no structures answers `None`,
    /// which is correct for a superflat and wrong for any real world —
    /// and wrong loudly, because a witch hut that spawns jungle mobs is
    /// visible.
    fn structure_spawn_override(
        &self,
        pos: BlockPos,
        category: super::MobCategory,
    ) -> Option<&super::tables::WeightedSpawnList>;
}

/// What only a server holding real `Mob`s can answer.
///
/// **Every method here is a refusal made explicit.** `EntityType.create`,
/// `Mob.finalizeSpawn` with its `SpawnGroupData` chain, `Mob.snapTo`,
/// `ServerLevel.addFreshEntityWithPassengers`, `Mob.checkSpawnRules` and
/// `Mob.checkSpawnObstruction` all need a live entity; they belong to the
/// mob branches and to `crates/rustmc-server/src/mob/`, and this crate
/// declares them rather than approximating any of them.
pub trait SpawnHost {
    /// `getMobForSpawn(level, type)` — `EntityType.create(level,
    /// NATURAL)` cast to `Mob`, false where vanilla gets null.
    ///
    /// A false answer makes `spawnCategoryForPosition` **`return`** —
    /// offset 383 is a bare `return`, not a `goto` to the loop increment,
    /// so one un-creatable type ends the whole call for that chunk
    /// position including the two attempts after it.
    ///
    /// `EntityType.create(Level, EntitySpawnReason)` draws **nothing**: it
    /// is `canSpawn(level)` — feature flags, then `isAllowedInPeaceful()
    /// || difficulty != PEACEFUL` — and then the factory. Any draw inside
    /// the mob's own constructor is the host's and is invisible here,
    /// which is a real gap in this module's draw accounting and is said
    /// so rather than assumed away.
    ///
    /// Vanilla's peaceful half of `canSpawn` is already excluded on the
    /// natural path: `SpawnPlacements.checkSpawnRules` applies the same
    /// `isAllowedInPeaceful()` test inside
    /// `isValidSpawnPostitionForType`, which runs first.
    fn create_mob(&mut self, kind: &str) -> bool;

    /// The rest of the tail, in one call because the spawner observes no
    /// state between its parts: `Mob.snapTo(x, y, z, yaw, 0.0f)`, then
    /// `isValidPositionForMob(level, mob, distSqr)`, then
    /// `Mob.finalizeSpawn(level, level.getCurrentDifficultyAt(mob
    /// .blockPosition()), NATURAL, groupData)`, then
    /// `ServerLevel.addFreshEntityWithPassengers(mob)`.
    ///
    /// `None` is `isValidPositionForMob` returning false — offset 421 is
    /// `goto 492`, the pack-member increment, so the next member is
    /// tried.
    ///
    /// The `yaw` is drawn by the caller, **after** [`Self::create_mob`]
    /// has succeeded, because that is where the draw sits (offsets
    /// 394-405) and a host taking the yaw up front would leak a
    /// `nextFloat` into the level's shared stream on the create-failed
    /// path.
    ///
    /// The `SpawnGroupData` vanilla threads through `finalizeSpawn` never
    /// leaves the host; [`SpawnRequest::first_of_group`] says when it is
    /// null.
    fn place(&mut self, request: SpawnRequest<'_>) -> Option<PlacedMob>;
}

/// Everything `create_and_place` is given.
#[derive(Debug, Clone, Copy)]
pub struct SpawnRequest<'a> {
    pub kind: &'a str,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    /// The squared distance to the nearest player, which
    /// `isValidPositionForMob` passes to `Mob.removeWhenFarAway`.
    pub player_dist_sqr: f64,
    /// Whether this is the first mob of the pack — vanilla threads a
    /// `SpawnGroupData` through `finalizeSpawn`, null on the first call
    /// and the previous return value after. The data itself never leaves
    /// the host.
    pub first_of_group: bool,
}

/// What the host reports back about a mob it placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlacedMob {
    /// `Mob.getMaxSpawnClusterSize()`. Vanilla compares the **running
    /// count for the whole `spawnCategoryForPosition` call**, not the
    /// per-attempt one, against this and `return`s when it is reached.
    pub max_spawn_cluster_size: i32,
    /// `Mob.isMaxGroupSizeReached(groupCount)`, evaluated by the host
    /// because `Mob`'s override reads the mob's own fields.
    pub max_group_size_reached: bool,
    /// The mob's `blockPosition()` after `snapTo`, which
    /// `SpawnState.afterSpawn` charges rather than the position tested.
    pub block_pos: BlockPos,
}
