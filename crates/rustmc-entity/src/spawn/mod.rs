//! `net.minecraft.world.level.NaturalSpawner` and what it reads — vanilla
//! 26.2's natural mob spawning, transcribed from the jar's bytecode.
//!
//! # What this module decides, and what it refuses
//!
//! The whole of the spawn *decision* is here: which categories run on this
//! tick, which chunk position is drawn, how the pack walks outward, which
//! mob the biome's weighted list yields, and every gate between that and a
//! mob existing. What it deliberately does **not** do is make one.
//!
//! `spawnCategoryForPosition`'s tail is
//! `EntityType.create` → `Mob.snapTo` → `Mob.finalizeSpawn(level,
//! difficulty, NATURAL, groupData)` → `ServerLevel.addFreshEntityWithPassengers`,
//! and every one of those needs a `Mob` in a `ServerLevel`. This crate has
//! neither: `crates/rustmc-server/src/mob/` is the `mob-in-world` branch's
//! and is not on `main` at the time of writing. So the tail is a
//! [`host::SpawnHost`] call — [`host::SpawnHost::create_and_place`] — the
//! decision that reaches it is fully transcribed, and **the world join is
//! refused rather than approximated**: nothing in `rustmc-server` calls
//! this module, and [`natural::spawn_for_chunk`]'s caller
//! (`ServerChunkCache.tickChunks`, with its `ChunkMap` player-distance
//! query and its `Util.shuffle` of the spawning chunks) is written down in
//! [`chunk_cache`] as a description and is not implemented.
//!
//! # The three tables
//!
//! Nothing here hardcodes a biome, a mob or a placement. Three generated
//! tables carry all of it, each cross-checked against a second source by
//! its generator:
//!
//! | table | generator | what |
//! |---|---|---|
//! | `biome_spawners.json` | `ci/dump-biome-spawners.sh` | 66 biomes, 795 spawner rows, 5 spawn costs |
//! | `spawn_placements.json` | `ci/dump-spawn-placements.sh` | 83 `SpawnPlacements.register` rows |
//! | `entity_spawn_facts.json` | `ci/dump-entity-spawn-facts.sh` | 158 types: category, size, the four flags |
//!
//! # The RNG draws, in order, because that is the part a golden pins
//!
//! Every draw below is on `ServerLevel.random` — one shared, unseeded
//! stream — so their *count* is as observable as their values. Per
//! `spawnCategoryForChunk` call:
//!
//! 1. `getRandomPosWithin`: `nextInt(16)`, `nextInt(16)`, then
//!    `Mth.randomBetweenInclusive(random, minY, height + 1)` — which is one
//!    more `nextInt`. **Three draws before anything is decided**, and they
//!    happen even when the position is immediately rejected.
//! 2. per attempt (three of them): `Mth.ceil(nextFloat() * 4.0f)` for the
//!    provisional pack size.
//! 3. per pack member: `nextInt(6) - nextInt(6)` for x then the same for z —
//!    **four draws, and the subtraction order is left-to-right**, so
//!    swapping the two halves is a different world.
//! 4. once per attempt, the first time a position survives the distance
//!    gate: `WeightedList.getRandom` — `nextInt(totalWeight)`, and **no draw
//!    at all when the list is empty**, because `WeightedList`'s selector is
//!    null at zero total weight and `getRandom` returns empty before
//!    touching the source. Then `nextInt(1 + maxCount - minCount)` for the
//!    real pack size.
//! 5. per surviving position: `nextFloat() * 360.0f` for the spawn yaw,
//!    drawn inside `Mob.snapTo` — recorded here as
//!    [`natural::SpawnAttempt::yaw`] rather than applied, because the mob
//!    is the host's.
//!
//! `SpawnPlacements.checkSpawnRules` draws too, and its draws are inside
//! the per-position ones: `Monster.isDarkEnoughToSpawn` takes `nextInt(32)`
//! and then `monsterSpawnLightTest.sample(random)`.
//!
//! # What a golden over this can and cannot say
//!
//! [`crate::spawn::testing`] runs the whole decision over a synthetic chunk
//! against a seeded `LegacyRandomSource` and records every draw and every
//! rejection reason. **That verifies this transcription's internal
//! consistency and nothing else.** It is not vanilla's answer: no vanilla
//! run has ever been compared against it, because a live capture of natural
//! spawns cannot exist on this box yet — `ci/t2-gameplay.sh` boots
//! `difficulty=peaceful`, `spawn-monsters=false` and `spawn_mobs=false`,
//! three independent suppressions, and `origin/corpus-dark-systems`
//! measured lifting the game rule as a **NULL, 15/15 pairs over 6 runs**.
//! `docs/cold-start/mob-tick-surface.md` has the numbers and
//! `docs/cold-start/natural-spawning.md` names the scenario that would
//! close this if the substrate ever allows one.

pub mod chunk_cache;
pub mod host;
pub mod natural;
pub mod placements;
pub mod rules;
pub mod state;
pub mod tables;
pub mod testing;

/// `net.minecraft.world.entity.MobCategory`, whole.
///
/// # Why a third copy in this crate, and why it is not a merge
///
/// [`crate::tick::MobCategory`] carries the two despawn distances and
/// [`crate::brain::behaviors::MobCategory`] carries the bare discriminants
/// for `SetEntityLookTarget`. Both are correct for what they were written
/// for and neither carries `max`, `isFriendly` or `isPersistent`, which is
/// the half spawning is about. Rather than widen one of them from a lane
/// that does not own it, this one is complete and
/// [`crate::spawn::tables::tests`] asserts it agrees with
/// `tick::MobCategory` on the distances — so the duplication is checked
/// rather than merely noted, and a future merge is a deletion.
///
/// The constructor is
/// `(String name, String debugAbbreviation, int max, boolean isFriendly,
///   boolean isPersistent, int despawnDistance)` and the declaration order
/// below is the enum's own, which is load-bearing: `SPAWNING_CATEGORIES` is
/// `values()` filtered on `!= MISC` and [`natural::spawn_for_chunk`] walks
/// that array in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MobCategory {
    Monster,
    Creature,
    Ambient,
    Axolotls,
    UndergroundWaterCreature,
    WaterCreature,
    WaterAmbient,
    Misc,
}

impl MobCategory {
    /// `getSerializedName()` — also `getName()`, the same field.
    #[must_use]
    pub const fn serialized_name(self) -> &'static str {
        match self {
            Self::Monster => "monster",
            Self::Creature => "creature",
            Self::Ambient => "ambient",
            Self::Axolotls => "axolotls",
            Self::UndergroundWaterCreature => "underground_water_creature",
            Self::WaterCreature => "water_creature",
            Self::WaterAmbient => "water_ambient",
            Self::Misc => "misc",
        }
    }

    /// The reverse of [`Self::serialized_name`], for the tables.
    #[must_use]
    pub fn from_serialized_name(name: &str) -> Option<Self> {
        ALL.iter().copied().find(|c| c.serialized_name() == name)
    }

    /// `getMaxInstancesPerChunk()` — the constructor's third argument, and
    /// `MISC` is **-1**, not 0. It never reaches the cap arithmetic because
    /// `SPAWNING_CATEGORIES` filters it out, but the value is the enum's.
    #[must_use]
    pub const fn max_instances_per_chunk(self) -> i32 {
        match self {
            Self::Monster => 70,
            Self::Creature => 10,
            Self::Ambient => 15,
            Self::Axolotls | Self::UndergroundWaterCreature | Self::WaterCreature => 5,
            Self::WaterAmbient => 20,
            Self::Misc => -1,
        }
    }

    /// `isFriendly()`. **Only `MONSTER` is unfriendly**, so
    /// `getFilteredSpawningCategories`' first gate suppresses exactly one
    /// of the seven spawning categories when `spawn-monsters=false`.
    #[must_use]
    pub const fn is_friendly(self) -> bool {
        !matches!(self, Self::Monster)
    }

    /// `isPersistent()`. **`CREATURE` and `MISC` only** — and `MISC` is not
    /// a spawning category, so the 400-tick `spawnFriendlies` cadence in
    /// `ServerChunkCache.tickChunks` gates `CREATURE` and nothing else.
    #[must_use]
    pub const fn is_persistent(self) -> bool {
        matches!(self, Self::Creature | Self::Misc)
    }

    /// `getDespawnDistance()` — 128 everywhere but `WATER_AMBIENT`.
    #[must_use]
    pub const fn despawn_distance(self) -> i32 {
        match self {
            Self::WaterAmbient => 64,
            _ => 128,
        }
    }

    /// `getNoDespawnDistance()` — `bipush 32; ireturn`, a constant on the
    /// enum and not the per-category field above.
    #[must_use]
    pub const fn no_despawn_distance(self) -> i32 {
        32
    }
}

/// `MobCategory.values()`, in declaration order.
pub const ALL: [MobCategory; 8] = [
    MobCategory::Monster,
    MobCategory::Creature,
    MobCategory::Ambient,
    MobCategory::Axolotls,
    MobCategory::UndergroundWaterCreature,
    MobCategory::WaterCreature,
    MobCategory::WaterAmbient,
    MobCategory::Misc,
];

/// `NaturalSpawner.SPAWNING_CATEGORIES` — `values()` filtered on
/// `!= MISC`, evaluated in the static initialiser. The order is
/// `spawnForChunk`'s iteration order, through the `List` that
/// `getFilteredSpawningCategories` builds by walking this array.
pub const SPAWNING_CATEGORIES: [MobCategory; 7] = [
    MobCategory::Monster,
    MobCategory::Creature,
    MobCategory::Ambient,
    MobCategory::Axolotls,
    MobCategory::UndergroundWaterCreature,
    MobCategory::WaterCreature,
    MobCategory::WaterAmbient,
];

/// `NaturalSpawner.MIN_SPAWN_DISTANCE` — the world-spawn exclusion radius
/// in blocks. Squared to 576 by `isRightDistanceToPlayerAndSpawnPoint`'s
/// `closerToCenterThan`, which is also the constant the *player* distance
/// is compared against; the two 24s are the same number used twice, and
/// only one of them is this field.
pub const MIN_SPAWN_DISTANCE: i32 = 24;

/// `NaturalSpawner.SPAWN_DISTANCE_CHUNK`.
pub const SPAWN_DISTANCE_CHUNK: i32 = 8;

/// `NaturalSpawner.SPAWN_DISTANCE_BLOCK`.
pub const SPAWN_DISTANCE_BLOCK: i32 = 128;

/// `NaturalSpawner.INSCRIBED_SQUARE_SPAWN_DISTANCE_CHUNK` —
/// `Mth.floor(8.0f / Mth.SQRT_OF_TWO)`, computed in the static
/// initialiser. `8 / 1.41421356… = 5.6568…`, floored to **5**.
pub const INSCRIBED_SQUARE_SPAWN_DISTANCE_CHUNK: i32 = 5;

/// `NaturalSpawner.MAGIC_NUMBER` — `(int) Math.pow(17.0, 2.0)`, computed
/// in the static initialiser rather than written as a literal. **289**, and
/// it is the divisor in `SpawnState.canSpawnForCategoryGlobal`:
/// `max * spawnableChunkCount / 289`, integer division.
pub const MAGIC_NUMBER: i32 = 289;

/// `spawnCategoryForPosition`'s outer loop bound — three attempts per call,
/// `iconst_3` at offset 56.
pub const SPAWN_ATTEMPTS: i32 = 3;

/// The half-width of the per-member walk: `nextInt(6) - nextInt(6)` on each
/// of x and z. `bipush 6` at offsets 118, 129, 144 and 157, and the same
/// `6` is stored dead into local 15 at offset 72.
pub const SPREAD: i32 = 6;

/// `isRightDistanceToPlayerAndSpawnPoint`'s first gate: the squared
/// distance to the nearest player must be **greater than** 576.0, so a
/// position exactly 24 blocks away is refused.
pub const MIN_PLAYER_DISTANCE_SQR: f64 = 576.0;
