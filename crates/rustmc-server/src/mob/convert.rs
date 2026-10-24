//! `Mob.convertTo` for the one conversion this server makes: a sheared
//! mooshroom becoming a cow.
//!
//! Reference (26.2): `Mob.convertTo(EntityType, ConversionParams,
//! EntitySpawnReason, AfterConversion)` (0-95), `ConversionType$1.convert`
//! (`SINGLE`), `ConversionType.convertCommon`, `ConversionParams.single(mob,
//! keepEquipment, preserveCanPickUpLoot)`, `Entity.copyPosition`.
//!
//! ```text
//!    0  isRemoved()                          -> null
//!    9  newMob = type.create(level, reason)  the constructor; no finalizeSpawn
//!   30  params.type().convert(this, newMob, params)
//!   41  afterConversion.finalizeConversion(newMob)
//!   71  level.addFreshEntity(newMob)         no ambient sound
//!   86  type.shouldDiscardAfterConversion()  SINGLE: true -> discard()
//! ```
//!
//! `SINGLE.convert` (javap `ConversionType$1`): passengers (none here),
//! `copyPosition` (position, `yRot`, `xRot`), `setDeltaMovement`, the vehicle
//! (none), equipment when `keepEquipment` (false for the mooshroom), the
//! fall-flying flag, `onGround`, the sleeping position (none), the leash
//! holder (`setLeashedTo(holder, true)`), then `convertCommon`: absorption and
//! effects (none on this server), `setBaby(isBaby())`, and for two
//! `AgeableMob`s `setAge(getAge())`, `forcedAge`, `forcedAgeTimer`; the
//! brain's `ANGRY_AT` (no brain here); `setCanPickUpLoot` only when
//! `preserveCanPickUpLoot` (false); left-handedness, no-AI, persistence,
//! custom-name visibility, fire, invulnerability, no-gravity, portal
//! cooldown, silence, tags and team.
//!
//! **Not copied here, by name:** a custom name (no name reaches a mob on
//! this server), effects and absorption (none exist), fire (no mob burns),
//! tags and teams (none). The leash copy calls the leash module's
//! `set_leashed_to`, which broadcasts the link on the new mob as vanilla's
//! `setLeashedTo(holder, true)` does.
use super::host::ServerMob;
use rustmc_entity::host::MobHost;

/// Convert `m` (being interacted with, out of the store) into a cow and
/// discard it. `after` is the `AfterConversion` callback, run with the old
/// mob before the cow is added.
pub fn convert_to_cow(m: &mut ServerMob, after: impl FnOnce(&ServerMob)) {
    // 0-8.
    if m.removed {
        return;
    }
    // 9-29: `COW.create(level, CONVERSION)` — the constructor chain, an
    // entity id, its own random's uuid and yaw draws.
    let Some(table) = super::attribute_table() else {
        return;
    };
    let id = crate::entities::Entities::shared().next_id();
    let Some(mut cow) = ServerMob::new(m.level, &super::cow::COW, id, table) else {
        return;
    };
    // SINGLE.convert: `copyPosition(old)` → `snapTo(x, y, z, yRot, xRot)`.
    cow.snap_to(m.x(), m.y(), m.z(), m.y_rot, m.x_rot);
    cow.body.movement = m.body.movement;
    cow.body.on_ground = m.body.on_ground;
    // convertCommon.
    let baby = m.is_baby();
    cow.set_age(if baby {
        super::breed::BABY_START_AGE
    } else {
        0
    });
    cow.set_age(m.get_age());
    cow.forced_age = m.forced_age;
    cow.forced_age_timer = m.forced_age_timer;
    let left = m.data.get_byte(super::data::id::MOB_FLAGS) & super::host::MOB_FLAG_LEFT_HANDED != 0;
    cow.set_left_handed(left);
    let no_ai = m.data.get_byte(super::data::id::MOB_FLAGS) & super::host::MOB_FLAG_NO_AI;
    if no_ai != 0 {
        let b = cow.data.get_byte(super::data::id::MOB_FLAGS);
        cow.data.set_byte(super::data::id::MOB_FLAGS, b | no_ai);
    }
    cow.persistence_required = m.persistence_required;
    for id in [
        super::data::id::CUSTOM_NAME_VISIBLE,
        super::data::id::NO_GRAVITY,
        super::data::id::SILENT,
    ] {
        let v = m.data.get_bool(id);
        cow.data.set_bool(id, v);
    }
    let holder = m.leash.map(|d| d.holder);
    // 41-49.
    after(m);
    // 71-75: `addFreshEntity`, which pairs it with every tracker in range.
    let new_id = super::add_converted(cow);
    if let Some(h) = holder {
        super::Mobs::shared().with_mut(new_id, |s| {
            super::leash::set_leashed_to(&mut s.ai.mob, h, true);
        });
    }
    // 86-90.
    m.discard();
}
