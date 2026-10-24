//! `ArmorStand` — the one `LivingEntity` this server hosts that is **not** a
//! `Mob`, and `ArmorStandItem.useOn`, which puts it in the world.
//!
//! It rides the mob host ([`super::host::ServerMob`]) for the parts of the
//! chain it shares with every living thing — `Entity.baseTick`,
//! `LivingEntity.baseTick`, `LivingEntity.aiStep`'s physics and `travel` —
//! and the tick skips every `Mob` layer for it, by kind:
//!
//! * `Mob.baseTick`'s ambient-sound draw (a `Mob` override);
//! * `Mob.serverAiStep` — `LivingEntity.serverAiStep` is empty, and the
//!   stand's `isEffectiveAi()` adds `hasPhysics()` on top;
//! * `Mob.tick`'s control-flag refresh;
//! * `Mob.checkDespawn` — `Entity.checkDespawn` is empty;
//! * `Mob.tickHeadTurn`'s body-rotation control — `ArmorStand.tickHeadTurn`
//!   is `yBodyRotO = yRotO; yBodyRot = getYRot()`.
//!
//! `ArmorStand.isPushable()` is `iconst_0` and `pushEntities` pushes only
//! ridable minecarts within `0.2` squared, of which this server has none,
//! so the kind is not pushable and pushes nothing.
//!
//! `javap -p -c -constants` on `ArmorStandItem`, `ArmorStand`, `EntityType`.
//!
//! # Not ported, by name
//!
//! Equipment and `interact` (no equipment slots exist here), poses other
//! than the defaults (only an item's `ENTITY_DATA` could set them, and
//! `EntityType.createDefaultStackConfig` finds none on a plain stack),
//! `isMarker`/`isSmall`/`showArms` (flags only an `ENTITY_DATA` sets).

use super::host::ServerMob;
use super::MobKind;
use rustmc_data::BlockPos;
use rustmc_entity::host::MobHost;
use rustmc_entity::mth;
use rustmc_entity::tick::MobCategory;
use rustmc_worldgen::rng::RandomSource;

/// `EntityTypes.ARMOR_STAND`: `of(ArmorStand::new, MISC).sized(0.5F,
/// 1.975F).eyeHeight(1.7775F).clientTrackingRange(10)`.
pub static ARMOR_STAND: MobKind = MobKind {
    name: "minecraft:armor_stand",
    entity_kind: crate::entities::ARMOR_STAND,
    width: 0.5,
    height: 1.975,
    eye_height: 1.7775,
    // `ArmorStand.BABY_DIMENSIONS` is for `isSmall()`, which nothing here
    // sets; `isBaby()` is `isSmall()`.
    baby_dimensions: None,
    baby_eye_height: None,
    category: MobCategory::Misc,
    allowed_in_peaceful: true,
    data: super::DataLayout::ArmorStand,
    ambient_sound_interval: 0,
    sound_volume: 1.0,
    voice_pitch_scale: 1.0,
    pushable: false,
    emits_sounds: true,
    remove_when_far_away: false,
    enemy: false,
};

/// Why `ArmorStandItem.useOn` returned `FAIL`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Offsets 1-15: the clicked face is `DOWN`.
    Down,
    /// Offsets 78-105: the stand's box collides with a block or holds an
    /// entity.
    Obstructed,
    /// The tables a living entity needs are absent.
    NoTables,
}

/// `ArmorStandItem.useOn`, from the placement position on: `pos` is
/// `BlockPlaceContext.getClickedPos()`, which the caller resolved (it needs
/// the replaceability table), `rotation` is `UseOnContext.getRotation()` —
/// the player's yaw.
///
/// ```text
///   46-72    bb = ARMOR_STAND.getDimensions().makeBoundingBox(atBottomCenterOf(pos))
///   78-105   !noCollision(null, bb) || !getEntities(null, bb).isEmpty() -> FAIL
///  132-149   EntityType.create(level, config, pos, SPAWN_ITEM_USE, true, true)
///  164-185   yRot = floor((wrapDegrees(rotation - 180) + 22.5) / 45) * 45
///  192-208   stand.snapTo(x, y, z, yRot, 0)
///  215       addFreshEntityWithPassengers
///  219-245   level.playSound(null, x, y, z, ARMOR_STAND_PLACE, BLOCKS, 0.75, 0.8)
/// ```
///
/// Returns the stand, built and positioned, for the caller to add — the
/// sound comes after the pairing, so the caller plays it.
///
/// # Errors
/// [`Refusal`] for `FAIL`.
pub fn use_on(
    level: super::level::MobLevel,
    pos: BlockPos,
    face_is_down: bool,
    rotation: f32,
    table: &'static crate::attributes::Attributes,
    next_id: impl FnOnce() -> i32,
) -> Result<ServerMob, Refusal> {
    if face_is_down {
        return Err(Refusal::Down);
    }
    let x = f64::from(pos.x) + 0.5;
    let y = f64::from(pos.y);
    let z = f64::from(pos.z) + 0.5;
    // `EntityDimensions.makeBoundingBox(x, y, z)`: `w / 2` either side, `h`
    // up, both widened from float.
    let hw = f64::from(ARMOR_STAND.width) / 2.0;
    let bb = rustmc_entity::phys::Aabb::new(
        x - hw,
        y,
        z - hw,
        x + hw,
        y + f64::from(ARMOR_STAND.height),
        z + hw,
    );
    if !rustmc_entity::host::PathLevel::no_collision(&level, -1, &bb)
        || !super::entities_in(&bb).is_empty()
    {
        return Err(Refusal::Obstructed);
    }
    let id = next_id();
    let mut m = ServerMob::new(level, &ARMOR_STAND, id, table).ok_or(Refusal::NoTables)?;
    // `EntityType.create(level, processor, pos, reason, true, true)`:
    // `setPos` at the block's bottom centre one up, the downward sweep for
    // the y offset, the level-random yaw — and, the stand not being a
    // `Mob`, no head or body rotation and no `finalizeSpawn`.
    m.set_pos_raw(x, f64::from(pos.y + 1), z);
    let d = super::spawn::y_offset(&level, pos, true, &m);
    let yaw = mth::wrap_degrees(super::level_random(|r| r.next_f32()) * 360.0);
    m.snap_to(x, f64::from(pos.y) + d, z, yaw, 0.0);
    // 164-208: the item overwrites that yaw with the player's, snapped to
    // eighths of a turn and turned to face them.
    #[expect(clippy::cast_precision_loss, reason = "vanilla's own i2f")]
    let snapped = mth_floor_f((mth::wrap_degrees(rotation - 180.0) + 22.5) / 45.0) as f32 * 45.0;
    let (px, py, pz) = (m.x(), m.y(), m.z());
    m.snap_to(px, py, pz, snapped, 0.0);
    Ok(m)
}

fn mth_floor_f(f: f32) -> i32 {
    #[expect(clippy::cast_possible_truncation, reason = "Mth.floor(float)")]
    {
        f.floor() as i32
    }
}

/// `ArmorStand.tickHeadTurn(float)`: `yBodyRotO = yRotO; yBodyRot = getYRot()`.
pub fn tick_head_turn(m: &mut ServerMob) {
    m.y_body_rot_o = m.y_rot_o;
    m.y_body_rot = m.y_rot;
}

/// `ArmorStand.hasPhysics()`: `!isMarker() && !isNoGravity()`. No marker
/// flag is ever set here.
#[must_use]
pub fn has_physics(m: &ServerMob) -> bool {
    !m.data.get_bool(super::data::id::NO_GRAVITY)
}

/// What `ArmorStand.hurtServer` did to a player's melee hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    /// `return false`: the stand is removed already, or the attacker may not
    /// build.
    Refused,
    /// 306-325: the first hit — `broadcastEntityEvent(this, 32)` and
    /// `lastHit = gameTime`. The stand stays.
    Wobble,
    /// 240-259 (a creative player) or 289-298 (a second hit within five
    /// ticks): the stand breaks. `drops` is `brokenByPlayer`'s popped item;
    /// the creative arm does not drop.
    Broken { drops: bool },
}

/// `ArmorStand.hurtServer(level, source, amount)` for `player_attack`, the
/// one source a packet can bring here.
///
/// ```text
///     1-8     isRemoved -> false
///    10-39    !MOB_GRIEFING && source.getEntity() instanceof Mob -> false   (a player is not)
///    41-56    BYPASSES_INVULNERABILITY -> kill, false                      (not player_attack)
///    58-81    isInvulnerableTo || invisible || isMarker -> false           (none set here)
///    83-174   explosion / ignites / burns                                  (not player_attack)
///   176-204   !CAN_BREAK_ARMOR_STAND && !ALWAYS_KILLS -> false             (#is_player_attack: can)
///   206-238   source.getEntity() is a Player && !mayBuild -> false         (survival and creative may)
///   240-260   isCreativePlayer -> playBrokenSound, showBreakingParticles, kill, true
///   262-329   gameTime - lastHit > 5 -> event 32, ENTITY_DAMAGE, lastHit = t
///             else brokenByPlayer, showBreakingParticles, kill;           true
/// ```
///
/// `lastHit` is the stand's own field; it starts at `0L`, so the first hit on
/// a world older than five ticks is always the wobble.
pub fn hurt_server(
    last_hit: &mut i64,
    removed: bool,
    game_time: i64,
    creative_player: bool,
) -> Hit {
    if removed {
        return Hit::Refused;
    }
    if creative_player {
        return Hit::Broken { drops: false };
    }
    if game_time - *last_hit > 5 {
        *last_hit = game_time;
        return Hit::Wobble;
    }
    Hit::Broken { drops: true }
}

#[cfg(test)]
mod tests {
    use super::{hurt_server, Hit};

    #[test]
    fn a_creative_punch_breaks_it_at_once_and_drops_nothing() {
        let mut last = 0;
        assert_eq!(
            hurt_server(&mut last, false, 100, true),
            Hit::Broken { drops: false }
        );
    }

    #[test]
    fn two_survival_punches_within_five_ticks_break_it_and_a_slower_pair_does_not() {
        let mut last = 0;
        assert_eq!(hurt_server(&mut last, false, 100, false), Hit::Wobble);
        assert_eq!(last, 100);
        assert_eq!(
            hurt_server(&mut last, false, 105, false),
            Hit::Broken { drops: true }
        );
        let mut slow = 0;
        assert_eq!(hurt_server(&mut slow, false, 100, false), Hit::Wobble);
        assert_eq!(hurt_server(&mut slow, false, 106, false), Hit::Wobble);
        assert_eq!(slow, 106);
    }

    #[test]
    fn a_removed_stand_refuses() {
        let mut last = 0;
        assert_eq!(hurt_server(&mut last, true, 100, true), Hit::Refused);
    }
}
