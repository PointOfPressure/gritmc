//! The `SpawnPlacements$SpawnPredicate`s, transcribed.
//!
//! # 6 of 39, and the other 33 are named rather than guessed
//!
//! `SpawnPlacements.<clinit>` registers 83 types against **39 distinct**
//! predicate methods. Six of them carry 48 of the 83 registrations and are
//! transcribed here; the remaining 33 predicates are per-type and are
//! listed in [`PENDING`]. A type whose predicate is pending returns
//! [`PredicateVerdict::Untranscribed`] and **never** a bool, so a caller
//! cannot read "not ported" as "the position was wrong" — that
//! substitution is exactly the class of defect
//! `docs/AGENT-BRIEF.md` opens with, and a bool return type would make it
//! unavoidable rather than merely likely.
//!
//! `crates/rustmc-entity/tests/spawn_tables.rs` sweeps the table and
//! refuses a predicate that is in neither set, so a version bump that adds
//! one is a red rather than a silent `Untranscribed`.

use rustmc_data::BlockPos;

use super::host::{EntitySpawnReason, LightLayer, SpawnLevel, SpawnRandom};

/// A predicate's answer, with "not transcribed" as its own value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PredicateVerdict {
    Allowed,
    Refused,
    /// The type's registered predicate is one of [`PENDING`]. The caller
    /// must decide what to do; [`super::natural`] refuses the position and
    /// records the predicate's name in the decision log.
    Untranscribed,
}

impl PredicateVerdict {
    #[must_use]
    pub const fn of(b: bool) -> Self {
        if b {
            Self::Allowed
        } else {
            Self::Refused
        }
    }

    /// True only for [`Self::Allowed`]. `Untranscribed` is **not** a pass.
    #[must_use]
    pub const fn is_allowed(self) -> bool {
        matches!(self, Self::Allowed)
    }
}

/// The six transcribed predicate methods, by their jar names.
pub const TRANSCRIBED: [&str; 6] = [
    "checkMobSpawnRules",
    "checkMonsterSpawnRules",
    "checkAnyLightMonsterSpawnRules",
    "checkSurfaceMonstersSpawnRules",
    "checkAnimalSpawnRules",
    "checkSurfaceWaterAnimalSpawnRules",
];

/// The 33 predicate methods registered in 26.2 that this module does not
/// evaluate. Each is one type's own rule and each needs facts this crate's
/// [`SpawnLevel`] does not expose — a biome tag, a structure lookup, the
/// mob's own state — so transcribing one is a `SpawnLevel` change and not
/// a line here.
pub const PENDING: [&str; 33] = [
    "checkArmadilloSpawnRules",
    "checkAxolotlSpawnRules",
    "checkBatSpawnRules",
    "checkCamelSpawnRules",
    "checkDrownedSpawnRules",
    "checkEndermiteSpawnRules",
    "checkFoxSpawnRules",
    "checkFrogSpawnRules",
    "checkGhastSpawnRules",
    "checkGlowSquidSpawnRules",
    "checkGoatSpawnRules",
    "checkGuardianSpawnRules",
    "checkHoglinSpawnRules",
    "checkMagmaCubeSpawnRules",
    "checkMushroomSpawnRules",
    "checkNautilusSpawnRules",
    "checkOcelotSpawnRules",
    "checkParrotSpawnRules",
    "checkPatrollingMonsterSpawnRules",
    "checkPiglinSpawnRules",
    "checkPolarBearSpawnRules",
    "checkRabbitSpawnRules",
    "checkSilverfishSpawnRules",
    "checkSkeletonHorseSpawnRules",
    "checkSlimeSpawnRules",
    "checkStraySpawnRules",
    "checkStriderSpawnRules",
    "checkSulfurCubeSpawnRules",
    "checkSurfaceAgeableWaterCreatureSpawnRules",
    "checkTropicalFishSpawnRules",
    "checkTurtleSpawnRules",
    "checkWolfSpawnRules",
    "checkZombifiedPiglinSpawnRules",
];

/// Dispatch by method name — the string the placement table carries.
///
/// Dispatching on a name rather than on an enum is deliberate: the table
/// is generated and the set of names is the jar's, so a name this function
/// does not know must be visible as itself. It falls through to
/// [`PredicateVerdict::Untranscribed`] for anything in [`PENDING`] and
/// **panics for a name in neither list**, which is the version-bump
/// tripwire; `tests/spawn_tables.rs` reaches it without a level.
pub fn evaluate<L: SpawnLevel + ?Sized>(
    predicate: &str,
    kind: &str,
    level: &L,
    reason: EntitySpawnReason,
    pos: BlockPos,
    rng: &mut dyn SpawnRandom,
) -> PredicateVerdict {
    match predicate {
        "checkMobSpawnRules" => {
            PredicateVerdict::of(check_mob_spawn_rules(kind, level, reason, pos))
        }
        "checkMonsterSpawnRules" => {
            PredicateVerdict::of(check_monster_spawn_rules(kind, level, reason, pos, rng))
        }
        "checkAnyLightMonsterSpawnRules" => {
            PredicateVerdict::of(check_mob_spawn_rules(kind, level, reason, pos))
        }
        "checkSurfaceMonstersSpawnRules" => PredicateVerdict::of(
            check_surface_monsters_spawn_rules(kind, level, reason, pos, rng),
        ),
        "checkAnimalSpawnRules" => {
            PredicateVerdict::of(check_animal_spawn_rules(level, reason, pos))
        }
        "checkSurfaceWaterAnimalSpawnRules" => {
            PredicateVerdict::of(check_surface_water_animal_spawn_rules(level, pos))
        }
        other if PENDING.contains(&other) => PredicateVerdict::Untranscribed,
        other => panic!(
            "SpawnPlacements registers a predicate this crate has never seen: {other}. \
             Add it to spawn::rules::TRANSCRIBED with a body, or to PENDING with a reason."
        ),
    }
}

/// `Mob.checkMobSpawnRules(type, level, reason, pos, random)`.
///
/// `isSpawner(reason) || level.getBlockState(pos.below()).isValidSpawn(…)`.
/// **No draw**, despite taking a `RandomSource`.
#[must_use]
pub fn check_mob_spawn_rules<L: SpawnLevel + ?Sized>(
    kind: &str,
    level: &L,
    reason: EntitySpawnReason,
    pos: BlockPos,
) -> bool {
    reason.is_spawner()
        || level.is_valid_spawn(
            BlockPos {
                y: pos.y - 1,
                ..pos
            },
            kind,
        )
}

/// `Monster.isDarkEnoughToSpawn(level, pos, random)`.
///
/// Four gates and **two draws**, and the draw order is the observable part:
///
/// 1. `getBrightness(SKY, pos) > random.nextInt(32)` -> false. The draw
///    happens whatever the sky light is, because Java evaluates the
///    right-hand side before the comparison;
/// 2. `monsterSpawnBlockLightLimit < 15 && getBrightness(BLOCK, pos) >
///    limit` -> false. **No draw**, and the `< 15` short-circuit means the
///    overworld (limit 0) still evaluates the second half;
/// 3. `maxLocalRawBrightness`, with `amount = 10` while thundering and the
///    no-argument form otherwise;
/// 4. that `<= monsterSpawnLightTest.sample(random)` — the second draw,
///    and it is **not** taken when gate 1 or 2 already returned. So a
///    bright position costs one draw and a dark one costs two.
#[must_use]
pub fn is_dark_enough_to_spawn<L: SpawnLevel + ?Sized>(
    level: &L,
    pos: BlockPos,
    rng: &mut dyn SpawnRandom,
) -> bool {
    if level.brightness(LightLayer::Sky, pos) > rng.next_i32_bound(32) {
        return false;
    }
    let limit = level.monster_spawn_block_light_limit();
    if limit < 15 && level.brightness(LightLayer::Block, pos) > limit {
        return false;
    }
    let brightness = if level.is_thundering() {
        level.max_local_raw_brightness(pos, Some(10))
    } else {
        level.max_local_raw_brightness(pos, None)
    };
    brightness <= level.sample_monster_spawn_light_test(rng)
}

/// `Monster.checkMonsterSpawnRules`.
///
/// `(ignoresLightRequirements(reason) || isDarkEnoughToSpawn(…)) &&
///  checkMobSpawnRules(…)`. `ignoresLightRequirements` is `TRIAL_SPAWNER`
/// alone, so a plain `SPAWNER` still pays both light draws.
#[must_use]
pub fn check_monster_spawn_rules<L: SpawnLevel + ?Sized>(
    kind: &str,
    level: &L,
    reason: EntitySpawnReason,
    pos: BlockPos,
    rng: &mut dyn SpawnRandom,
) -> bool {
    (reason.ignores_light_requirements() || is_dark_enough_to_spawn(level, pos, rng))
        && check_mob_spawn_rules(kind, level, reason, pos)
}

/// `Monster.checkSurfaceMonstersSpawnRules`.
///
/// `checkMonsterSpawnRules(…) && (isSpawner(reason) || level.canSeeSky(pos))`.
#[must_use]
pub fn check_surface_monsters_spawn_rules<L: SpawnLevel + ?Sized>(
    kind: &str,
    level: &L,
    reason: EntitySpawnReason,
    pos: BlockPos,
    rng: &mut dyn SpawnRandom,
) -> bool {
    check_monster_spawn_rules(kind, level, reason, pos, rng)
        && (reason.is_spawner() || level.can_see_sky(pos))
}

/// `Animal.isBrightEnoughToSpawn(level, pos)`:
/// `getRawBrightness(pos, 0) > 8`. **Strictly greater**, so light level 9
/// is the first that passes.
#[must_use]
pub fn is_bright_enough_to_spawn<L: SpawnLevel + ?Sized>(level: &L, pos: BlockPos) -> bool {
    level.raw_brightness(pos, 0) > 8
}

/// `Animal.checkAnimalSpawnRules`.
///
/// `state(pos.below()).is(ANIMALS_SPAWNABLE_ON) && bright`, where `bright`
/// is `ignoresLightRequirements(reason) || isBrightEnoughToSpawn(level,
/// pos)`. The bytecode computes `bright` into a local **first** (offsets
/// 0-20) and tests the block tag second, which is not observable but is
/// where the light call sits if a later version adds a draw to it.
#[must_use]
pub fn check_animal_spawn_rules<L: SpawnLevel + ?Sized>(
    level: &L,
    reason: EntitySpawnReason,
    pos: BlockPos,
) -> bool {
    let bright = reason.ignores_light_requirements() || is_bright_enough_to_spawn(level, pos);
    let below = BlockPos {
        y: pos.y - 1,
        ..pos
    };
    level.block_facts(below).animals_spawnable_on && bright
}

/// `WaterAnimal.checkSurfaceWaterAnimalSpawnRules`.
///
/// A band, not a point: `seaLevel - 13 <= y <= seaLevel`, then
/// `getFluidState(pos.below()).is(FluidTags.WATER)` and
/// `getBlockState(pos.above()).is(Blocks.WATER)`. Note the asymmetry — the
/// block **below** is tested as a fluid tag and the block **above** as the
/// water block itself, so waterlogged blocks below count and waterlogged
/// blocks above do not.
#[must_use]
pub fn check_surface_water_animal_spawn_rules<L: SpawnLevel + ?Sized>(
    level: &L,
    pos: BlockPos,
) -> bool {
    let sea = level.sea_level();
    let floor = sea - 13;
    pos.y >= floor
        && pos.y <= sea
        && level
            .block_facts(BlockPos {
                y: pos.y - 1,
                ..pos
            })
            .fluid_is_water
        && level
            .block_facts(BlockPos {
                y: pos.y + 1,
                ..pos
            })
            .is_water_block
}
