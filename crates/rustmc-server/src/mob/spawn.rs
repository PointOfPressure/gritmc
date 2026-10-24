//! How a mob enters the world: `SpawnEggItem.useOn`, `EntityType.spawn`,
//! `EntityType.create`, the `finalizeSpawn` chain, and
//! `SummonCommand.createEntity`.
//!
//! Reference (26.2): `SpawnEggItem.useOn` (offsets 0-254), `spawnMob`
//! (0-42), `SpawnEggItem.getType`, `EntityType.canSpawn`,
//! `EntityType.spawn(ServerLevel, ItemStack, LivingEntity, BlockPos,
//! EntitySpawnReason, boolean, boolean)` (0-35), `spawn(ServerLevel,
//! PostSpawnProcessor, BlockPos, EntitySpawnReason, boolean, boolean)`
//! (0-48), `create(ServerLevel, PostSpawnProcessor, BlockPos,
//! EntitySpawnReason, boolean, boolean)` (0-186), `getYOffset` (0-61),
//! `Mob.finalizeSpawn` (0-88), `AgeableMob.finalizeSpawn` (0-80),
//! `AgeableMob$AgeableMobGroupData.<init>(boolean)`, `Pig.finalizeSpawn`,
//! `VariantUtils.selectVariantToSpawn`, `PriorityProvider.select`/`pick`,
//! `PigSoundVariants.pickRandomSoundVariant`, `MappedRegistry.getRandom`,
//! `Util.getRandom(List, RandomSource)`, `SummonCommand.createEntity`
//! (0-186).
//!
//! # The level random's draws, in order, for a spawn-egg pig
//!
//! ```text
//!   EntityType.create @103   nextFloat() * 360      the spawn yaw
//!   Pig.finalizeSpawn         selectVariantToSpawn → PriorityProvider.pick →
//!                             Util.getRandom(list, random): nextInt(list.size())
//!                             — ONE candidate on a temperate biome, and the
//!                             draw is still made (`nextInt(1)`)
//!                             pickRandomSoundVariant → Registry.getRandom →
//!                             nextInt(3) over `byId`
//!   AgeableMob.finalizeSpawn  NO draw: a fresh AgeableMobGroupData has
//!                             groupSize 0 and the baby roll is gated on
//!                             `getGroupSize() > 0` (offsets 30-35), so the
//!                             first of a group is never a baby
//!   Mob.finalizeSpawn         triangle(0.0, 0.11485): nextDouble, nextDouble
//!                             nextFloat() < 0.05 → setLeftHanded
//! ```
//!
//! `docs/cold-start/mob-tick-surface.md` listed an `AgeableMob` age draw
//! and the bonus as `0.11228`; the bytecode has no age draw on this path
//! and `0.11485000000000001d` at `Mob.finalizeSpawn:46`. Both corrected
//! here and in `docs/FINDINGS.md`.
//!
//! **Every one of these comes from `ServerLevel.getRandom()`**, an
//! unseeded `RandomSource.create()`. Vanilla does not agree with itself
//! on the yaw (`080` flaps 15/15) or the sound variant, so the values are
//! not gradable against a recording; the *count and order* of the draws
//! is what parity means here and is what the tests pin.
//!
//! # What the egg does not do here
//!
//! `useOn`'s spawner arm (offsets 82-185: a `Spawner` block entity takes
//! the egg's type) needs a spawner block, which no scenario places. The
//! `ItemStack.consume(1, player)` at `spawnMob:19-24` is the caller's
//! (creative keeps the egg). `ServerLevel.gameEvent(ENTITY_PLACE)` is a
//! game event, not a packet.
use super::host::{random_spawn_bonus, ServerMob, FOLLOW_RANGE};
use super::level::MobLevel;
use super::{DataLayout, MobKind};
use rustmc_data::BlockPos;
use rustmc_entity::mth;
use rustmc_worldgen::rng::RandomSource;

/// `EntitySpawnReason`, the members this lane reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnReason {
    /// `SPAWN_ITEM_USE` — `SpawnEggItem.spawnMob:6`.
    SpawnItemUse,
    /// `COMMAND` — `SummonCommand.createEntity:91`.
    Command,
}

/// `Mob.RANDOM_SPAWN_BONUS_ID`.
pub const RANDOM_SPAWN_BONUS_ID: &str = "minecraft:random_spawn_bonus";

/// Why a spawn egg did nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EggRefusal {
    /// `getType(stack) == null`: not a spawn egg, or one for a type this
    /// server has no mob for.
    NotAnEgg,
    /// `!type.canSpawn(level)`: `isAllowedInPeaceful() || difficulty != PEACEFUL`
    /// failed — `InteractionResult.FAIL`.
    Peaceful,
    /// The level's tables are absent: no mob can be built.
    NoTables,
}

/// `SpawnEggItem.useOn`, offsets 0-254, for a click on `clicked` with
/// `face`, reduced to the position the mob is spawned at and the two
/// booleans `spawnMob` passes on.
///
/// ```text
///   186-216  pos = clickedState.getCollisionShape(level, clicked).isEmpty()
///                ? clicked : clicked.relative(face)
///   216-251  spawnMob(type, player, stack, level, pos, true,
///                     !Objects.equals(clicked, pos) && face == UP)
/// ```
///
/// The returned `ServerMob` is built, finalised and positioned; the caller
/// adds it to the level (`addFreshEntityWithPassengers`) and plays the
/// ambient sound (`spawn:26-43`).
///
/// # Errors
/// An [`EggRefusal`] for the two `FAIL` returns and the missing-table case.
pub fn use_spawn_egg(
    level: MobLevel,
    kind: &'static MobKind,
    clicked: BlockPos,
    face_is_up: bool,
    table: &'static crate::attributes::Attributes,
    next_id: impl FnOnce() -> i32,
) -> Result<ServerMob, EggRefusal> {
    // 16-33: `type == null || !type.canSpawn(level)` → FAIL.
    if !can_spawn(kind) {
        return Err(EggRefusal::Peaceful);
    }
    // 186-216.
    let collision_empty = level
        .shape(clicked)
        .is_none_or(crate::collide::Shape::is_empty);
    let pos = if collision_empty {
        clicked
    } else {
        relative(clicked, face_is_up)
    };
    let adjust = pos != clicked && face_is_up;
    // spawnMob → EntityType.spawn(level, stack, player, pos,
    // SPAWN_ITEM_USE, true, adjust) → spawn(level, processor, pos, reason,
    // true, adjust) → create(level, processor, pos, reason, true, adjust).
    create(
        level,
        kind,
        pos,
        SpawnReason::SpawnItemUse,
        true,
        adjust,
        table,
        next_id,
    )
    .ok_or(EggRefusal::NoTables)
}

/// `BlockPos.relative(Direction)` for the one face the egg path needs to
/// tell apart: `UP` lifts by one, and every other face is answered as the
/// position the caller resolved (`clicked.relative(face)` is computed by
/// the handler, which knows the face; the egg only needs to know whether
/// it was `UP`).
fn relative(clicked: BlockPos, face_is_up: bool) -> BlockPos {
    if face_is_up {
        BlockPos {
            x: clicked.x,
            y: clicked.y + 1,
            z: clicked.z,
        }
    } else {
        clicked
    }
}

/// `EntityType.canSpawn(Level)`: `isAllowedInPeaceful() || level.getDifficulty() != PEACEFUL`.
#[must_use]
pub fn can_spawn(kind: &MobKind) -> bool {
    kind.allowed_in_peaceful || super::difficulty() != rustmc_entity::host::Difficulty::Peaceful
}

/// `EntityType.create(ServerLevel, PostSpawnProcessor, BlockPos,
/// EntitySpawnReason, boolean shouldOffsetY, boolean shouldOffsetYMore)`,
/// offsets 0-186.
///
/// ```text
///     0-15   entity = create(level, reason)        (the constructor chain)
///    16-68   if (shouldOffsetY) { setPos(x + .5, y + 1, z + .5);
///                                 d = getYOffset(level, pos, more, box) }
///            else d = 0
///    71-115  snapTo(x + .5, y + d, z + .5,
///                   wrapDegrees(level.random.nextFloat() * 360), 0)
///   118-171  if (Mob) { yHeadRot = yBodyRot = yRot;
///                       finalizeSpawn(level, difficultyAt(pos), reason, null) }
///   172-179  processor.apply(entity)                (an egg's components)
/// ```
///
/// `None` when the type has no attribute supplier.
#[expect(
    clippy::too_many_arguments,
    reason = "vanilla's own six, plus the tables"
)]
pub fn create(
    level: MobLevel,
    kind: &'static MobKind,
    pos: BlockPos,
    reason: SpawnReason,
    offset_y: bool,
    offset_y_more: bool,
    table: &'static crate::attributes::Attributes,
    next_id: impl FnOnce() -> i32,
) -> Option<ServerMob> {
    // 0-7: `create(level, reason)` → `canSpawn` again, then the factory.
    if !can_spawn(kind) {
        return None;
    }
    let id = next_id();
    let mut m = ServerMob::new(level, kind, id, table)?;
    let d = if offset_y {
        m.set_pos_raw(
            f64::from(pos.x) + 0.5,
            f64::from(pos.y + 1),
            f64::from(pos.z) + 0.5,
        );
        y_offset(&level, pos, offset_y_more, &m)
    } else {
        0.0
    };
    // 99-111: the yaw, from the LEVEL random.
    let yaw = mth::wrap_degrees(super::level_random(|r| r.next_f32()) * 360.0);
    m.snap_to(
        f64::from(pos.x) + 0.5,
        f64::from(pos.y) + d,
        f64::from(pos.z) + 0.5,
        yaw,
        0.0,
    );
    // 118-171.
    m.y_head_rot = m.y_rot;
    m.y_body_rot = m.y_rot;
    finalize_spawn(&mut m, reason);
    Some(m)
}

/// `EntityType.getYOffset(LevelReader, BlockPos, boolean, AABB)`, offsets
/// 0-61:
///
/// ```text
///   box = new AABB(pos);  if (more) box = box.expandTowards(0, -1, 0)
///   collisions = level.getCollisions(null, box)
///   return 1 + Shapes.collide(Y, entityBox, collisions, more ? -2 : -1)
/// ```
///
/// So the mob's box is swept downward through the block(s) at the clicked
/// position and the one below (when `more`), and lands on top of whatever
/// it hits — which for an egg used on the top of a full block is `0`
/// (the sweep from `y + 1` down by one is stopped at once), and for an
/// egg used on the side of a block is a drop through the empty target.
pub(super) fn y_offset(level: &MobLevel, pos: BlockPos, more: bool, m: &ServerMob) -> f64 {
    let mut probe = crate::collide::Aabb::new(
        f64::from(pos.x),
        f64::from(pos.y),
        f64::from(pos.z),
        f64::from(pos.x + 1),
        f64::from(pos.y + 1),
        f64::from(pos.z + 1),
    );
    if more {
        probe = probe.expand_towards(0.0, -1.0, 0.0);
    }
    let Ok(colliders) = crate::collide::collect_block_collisions(level.shapes, level, probe) else {
        return 0.0;
    };
    let entity_box = m.profile().bounding_box(m.body.pos);
    let d = if more { -2.0 } else { -1.0 };
    1.0 + crate::collide::shapes_collide(crate::collide::Axis::Y, entity_box, &colliders, d)
}

/// The `finalizeSpawn` chain for the kind, `SpawnGroupData` null.
///
/// `Pig.finalizeSpawn` → `Animal.finalizeSpawn` (no body of its own) →
/// `AgeableMob.finalizeSpawn` → `PathfinderMob` → `Mob.finalizeSpawn`.
/// `Bat` has no override: `Mob.finalizeSpawn` alone.
pub fn finalize_spawn(m: &mut ServerMob, _reason: SpawnReason) {
    // The kinds' own heads, each before `Animal.finalizeSpawn` (which has no
    // body of its own) → `AgeableMob.finalizeSpawn` (no draw: see the pig's
    // arm below) → `Mob.finalizeSpawn`.
    match m.kind.data {
        // `Cow.finalizeSpawn` 0-37: variant, then sound variant.
        DataLayout::Cow => super::cow::finalize_spawn(m),
        // `Chicken.finalizeSpawn` 0-37: the same two.
        DataLayout::Chicken => super::chicken::finalize_spawn(m),
        // `Sheep.finalizeSpawn` 0-11: the colour.
        DataLayout::Sheep => super::sheep::finalize_spawn(m),
        // `MushroomCow` does not override it: `RED`, the definition, stays.
        DataLayout::Mooshroom
        | DataLayout::Pig
        | DataLayout::Bat
        | DataLayout::IronGolem
        | DataLayout::SnowGolem
        | DataLayout::CopperGolem
        | DataLayout::ArmorStand => {}
    }
    if let DataLayout::Pig = m.kind.data {
        // Pig.finalizeSpawn 2-20: `VariantUtils.selectVariantToSpawn(ctx,
        // PIG_VARIANT).ifPresent(this::setVariant)`.
        if let Some(variant) = super::pig::select_variant_to_spawn(m) {
            super::pig::set_variant(m, variant);
        }
        // 25-37: `setSoundVariant(PigSoundVariants.pickRandomSoundVariant(
        // registryAccess(), level.getRandom()))` — `Registry.getRandom` →
        // `Util.getRandomSafe(byId, random)` → `nextInt(size)`.
        let ids = super::pig::sound_variant_ids();
        let n = i32::try_from(ids.len()).unwrap_or(0);
        if n > 0 {
            let i = super::level_random(|r| r.next_i32_bound(n));
            super::pig::set_sound_variant(m, ids[usize::try_from(i).unwrap_or(0)]);
        }
        // AgeableMob.finalizeSpawn 0-15: a fresh `AgeableMobGroupData(true)`
        // — `shouldSpawnBaby` true, `babySpawnChance` 0.05f, `groupSize` 0.
        // 22-66: the baby roll needs `getGroupSize() > 0`: skipped, and
        // no draw is made. 66-70: `increaseGroupSizeByOne` on a group
        // nobody else joins.
    }
    // Mob.finalizeSpawn 0-88.
    let has_bonus = m
        .attributes
        .instance(FOLLOW_RANGE)
        .is_some_and(|i| i.has_modifier(RANDOM_SPAWN_BONUS_ID));
    if !has_bonus {
        // `RandomSource.triangle(0.0, 0.11485)`: `a + b * (nextDouble() - nextDouble())`.
        let a = super::level_random(|r| r.next_f64());
        let b = super::level_random(|r| r.next_f64());
        let amount = 0.0 + 0.114_850_000_000_000_01 * (a - b);
        m.attributes
            .add_permanent_modifier(FOLLOW_RANGE, random_spawn_bonus(amount));
    }
    // 63-83.
    let left = super::level_random(|r| r.next_f32()) < 0.05;
    m.set_left_handed(left);
}

/// `SummonCommand.createEntity(source, type, pos, nbt, finalize=true)`,
/// offsets 0-186, with an empty tag:
///
/// ```text
///    0-20   isInSpawnableBounds(pos) else INVALID_POSITION
///   21-53   peaceful && !isAllowedInPeaceful → ERROR_FAILED_PEACEFUL
///   54-107  loadEntityRecursive(tag + id, level, COMMAND request,
///                               e -> { e.snapTo(x, y, z, e.getYRot(), e.getXRot()); e })
///  121-166  if (Mob) finalizeSpawn(level, difficultyAt, COMMAND, null)
///  167-183  tryAddFreshEntityWithPassengers
/// ```
///
/// `loadEntityRecursive` builds the entity through `EntityType.create(level,
/// request)` — the constructor chain and **no level-random yaw**: the yaw
/// a summoned mob keeps is `LivingEntity.<init>`'s `random.nextFloat() *
/// 6.2831855f` from the mob's own random, radians used as degrees. The
/// snap keeps it (`e.getYRot()`).
///
/// # Errors
/// `Peaceful` for the difficulty refusal, `NoTables` when no mob can be
/// built, `NotAnEgg` (reused) for a y outside the spawnable bounds.
pub fn summon(
    level: MobLevel,
    kind: &'static MobKind,
    x: f64,
    y: f64,
    z: f64,
    table: &'static crate::attributes::Attributes,
    next_id: impl FnOnce() -> i32,
) -> Result<ServerMob, EggRefusal> {
    // `Level.isInSpawnableBounds(pos)`: `!isOutsideBuildHeight(y) &&
    // isInWorldBounds(x, z)` — the horizontal bound is ±30,000,000.
    let by = rustmc_entity::phys::block_pos_containing(x, y, z).y;
    if by < rustmc_entity::host::PathLevel::min_y(&level)
        || by > rustmc_entity::host::PathLevel::max_y(&level)
    {
        return Err(EggRefusal::NotAnEgg);
    }
    if !can_spawn(kind) {
        return Err(EggRefusal::Peaceful);
    }
    let id = next_id();
    let mut m = ServerMob::new(level, kind, id, table).ok_or(EggRefusal::NoTables)?;
    let (yr, xr) = (m.y_rot, m.x_rot);
    m.snap_to(x, y, z, yr, xr);
    // `EntityType.create(level, request)` set no body/head rotation, so
    // `yHeadRot` is the constructor's (equal to the yaw) and `yBodyRot`
    // stays 0 until the first tick's `tickHeadTurn`.
    finalize_spawn(&mut m, SpawnReason::Command);
    Ok(m)
}
