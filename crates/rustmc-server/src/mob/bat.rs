//! `Bat`: the one mob in this lane that flies, hangs and despawns.
//!
//! Reference (26.2): `Bat.<init>` (offsets 0-40), `Bat.defineSynchedData`,
//! `Bat.isResting` / `setResting` (bit 1 of `DATA_ID_FLAGS`), `Bat.tick`
//! (0-69), `Bat.customServerAiStep` (0-512), `Bat.createAttributes`
//! (`MAX_HEALTH` 6), `Bat.getSoundVolume` (0.1F), `Bat.getVoicePitch`
//! (`super * 0.95F`), `Bat.getAmbientSound` (resting: `nextInt(4) != 0` →
//! null, else `BAT_AMBIENT`), `Bat.isPushable` (false), `Bat.doPush` /
//! `pushEntities` (empty), `Bat.getMovementEmission` (`EVENTS`: no step
//! sounds), `Bat.checkFallDamage` (empty), `Bat.<clinit>`
//! (`BAT_RESTING_TARGETING = forNonCombat().range(4.0)`), `EntityTypes.BAT`.
//!
//! `Bat` registers no goals and has no `PathfinderMob` in its chain
//! (`AmbientCreature extends Mob`), so `registerGoals` is `Mob`'s bare
//! return and the whole of its behaviour is `customServerAiStep`, which
//! writes `deltaMovement`, `zza` and `yRot` directly and lets
//! `LivingEntity.travel` fly it (`getFlyingSpeed` 0.02F, off the ground).
//!
//! `Mob.removeWhenFarAway` is `iconst_1` and `Bat` does not override it:
//! a bat past 128 blocks from every player is discarded by
//! `checkDespawn`, which `ci/checks/gameplay/082-*` grades.
use super::host::ServerMob;
use super::MobKind;
use rustmc_data::BlockPos;
use rustmc_entity::ai::mob_ai::MobAi;
use rustmc_entity::ai::targeting::{nearest_player_to_mob, TargetingConditions};
use rustmc_entity::host::MobHost;
use rustmc_entity::mth;
use rustmc_entity::tick::MobCategory;
use rustmc_worldgen::rng::RandomSource;

/// `EntityTypes.BAT`: `of(Bat::new, AMBIENT).sized(0.5F, 0.9F)
/// .eyeHeight(0.45F).clientTrackingRange(5)`; `updateInterval` 3 by
/// default. `trackDeltas()` names BAT in its false list.
pub static BAT: MobKind = MobKind {
    name: "minecraft:bat",
    entity_kind: crate::entities::BAT,
    width: 0.5,
    height: 0.9,
    eye_height: 0.45,
    baby_dimensions: None,
    baby_eye_height: None,
    category: MobCategory::Ambient,
    allowed_in_peaceful: true,
    data: super::DataLayout::Bat,
    // `Mob.getAmbientSoundInterval()`: 80.
    ambient_sound_interval: 80,
    sound_volume: 0.1,
    voice_pitch_scale: 0.95,
    pushable: false,
    emits_sounds: false,
    remove_when_far_away: true,
    enemy: false,
};

/// `Bat.FLAG_RESTING`.
const FLAG_RESTING: u8 = 1;

/// `isResting()`.
#[must_use]
pub fn is_resting(m: &ServerMob) -> bool {
    m.data.get_byte(super::data::id::BAT_FLAGS) & FLAG_RESTING != 0
}

/// `setResting(boolean)`.
pub fn set_resting(m: &mut ServerMob, v: bool) {
    let b = m.data.get_byte(super::data::id::BAT_FLAGS);
    let n = if v {
        b | FLAG_RESTING
    } else {
        b & !FLAG_RESTING
    };
    m.data.set_byte(super::data::id::BAT_FLAGS, n);
}

/// `Bat.getAmbientSound()`: resting bats are quiet three ticks in four
/// — a draw on the mob's random each time the ambient roll lands.
pub fn ambient_sound(m: &mut ServerMob) -> Option<&'static str> {
    if is_resting(m) && m.random.next_i32_bound(4) != 0 {
        return None;
    }
    Some("minecraft:entity.bat.ambient")
}

/// `Bat.tick()` after `AmbientCreature.tick()` (`Mob.tick`), offsets 4-65:
/// resting pins the bat to the ceiling with zero motion; flying damps the
/// vertical motion to 0.6.
pub fn tick_after(m: &mut ServerMob) {
    if is_resting(m) {
        m.set_delta_movement(0.0, 0.0, 0.0);
        let y = f64::from(mth_floor(m.y())) + 1.0 - f64::from(m.bb_height());
        let (x, z) = (m.x(), m.z());
        m.set_pos_raw(x, y, z);
    } else {
        let d = m.delta_movement();
        m.set_delta_movement(d.x, d.y * 0.6, d.z);
    }
    // 65-68: setupAnimationStates — client animation.
}

/// `Math.signum(double)`: `0.0` stays `0.0` (and `-0.0` stays `-0.0`),
/// unlike `f64::signum`, which answers `1.0` for a positive zero.
fn java_signum(d: f64) -> f64 {
    if d > 0.0 {
        1.0
    } else if d < 0.0 {
        -1.0
    } else {
        d
    }
}

fn mth_floor(d: f64) -> i32 {
    #[expect(clippy::cast_possible_truncation, reason = "Mth.floor")]
    {
        d.floor() as i32
    }
}

/// `Bat.customServerAiStep(ServerLevel)`, offsets 5-512, after
/// `AmbientCreature`'s (`Mob`'s bare return).
pub fn custom_server_ai_step(ai: &mut MobAi<ServerMob>) {
    let pos = ai.mob.block_position();
    let above = BlockPos {
        x: pos.x,
        y: pos.y + 1,
        z: pos.z,
    };
    if is_resting(&ai.mob) {
        let silent = ai.mob.data.get_bool(super::data::id::SILENT);
        // 28-38: `getBlockState(above).isRedstoneConductor(level, pos)`.
        let conductor_above = ai
            .mob
            .level
            .row(above)
            .is_some_and(|r| r.redstone_conductor);
        if conductor_above {
            // 41-70: one tick in 200, a random head yaw.
            if ai.mob.random.next_i32_bound(200) == 0 {
                #[expect(clippy::cast_precision_loss, reason = "vanilla's own i2f")]
                let yaw = ai.mob.random.next_i32_bound(360) as f32;
                ai.mob.y_head_rot = yaw;
            }
            // 73-104: a player within 4 blocks wakes it; level event 1025
            // (the bat takeoff sound) unless silent.
            let conditions = TargetingConditions::for_non_combat().range(4.0);
            if nearest_player_to_mob(ai, &conditions).is_some() {
                set_resting(&mut ai.mob, false);
                if !silent {
                    super::level_event(&ai.mob, 1025, pos);
                }
            }
        } else {
            // 107-124: nothing to hang from.
            set_resting(&mut ai.mob, false);
            if !silent {
                super::level_event(&ai.mob, 1025, pos);
            }
        }
        return;
    }
    // 130-167: drop a target that is no longer empty or is below the world.
    if let Some(t) = ai.mob.target_position {
        let empty = ai.mob.level.row(t).is_some_and(|r| r.air);
        if !empty || t.y <= rustmc_entity::host::PathLevel::min_y(&ai.mob.level) {
            ai.mob.target_position = None;
        }
    }
    // 167-293: pick a new target one tick in 30, or when within 2 blocks
    // of the current one, or when there is none. Seven draws when picked.
    let need_new = match ai.mob.target_position {
        None => true,
        Some(t) => {
            ai.mob.random.next_i32_bound(30) == 0
                || rustmc_entity::phys::closer_to_center_than(t, ai.mob.position(), 2.0)
        }
    };
    if need_new {
        let r = &mut ai.mob.random;
        let dx = f64::from(r.next_i32_bound(7)) - f64::from(r.next_i32_bound(7));
        let dy = f64::from(r.next_i32_bound(6)) - 2.0;
        let dz = f64::from(r.next_i32_bound(7)) - f64::from(r.next_i32_bound(7));
        let x = ai.mob.x() + dx;
        let y = ai.mob.y() + dy;
        let z = ai.mob.z() + dz;
        ai.mob.target_position = Some(rustmc_entity::phys::block_pos_containing(x, y, z));
    }
    let Some(t) = ai.mob.target_position else {
        return;
    };
    // 293-423: steer toward the target's centre.
    let d4 = f64::from(t.x) + 0.5 - ai.mob.x();
    let d6 = f64::from(t.y) + 0.1 - ai.mob.y();
    let d8 = f64::from(t.z) + 0.5 - ai.mob.z();
    let v = ai.mob.delta_movement();
    let nx = v.x + (java_signum(d4) * 0.5 - v.x) * 0.100_000_001_490_116_12;
    let ny = v.y + (java_signum(d6) * 0.699_999_988_079_071 - v.y) * 0.100_000_001_490_116_12;
    let nz = v.z + (java_signum(d8) * 0.5 - v.z) * 0.100_000_001_490_116_12;
    ai.mob.set_delta_movement(nx, ny, nz);
    // 426-477: face the motion, `zza = 0.5F`.
    #[expect(clippy::cast_possible_truncation, reason = "vanilla's own d2f")]
    let f12 = (mth::atan2(nz, nx) * 57.295_776_367_187_5) as f32 - 90.0;
    let f13 = mth::wrap_degrees(f12 - ai.mob.y_rot());
    ai.mob.zza = 0.5;
    let yaw = ai.mob.y_rot() + f13;
    ai.mob.set_y_rot(yaw);
    // 480-509: one tick in 100, hang if there is a conductor above.
    if ai.mob.random.next_i32_bound(100) == 0 {
        let conductor_above = ai
            .mob
            .level
            .row(above)
            .is_some_and(|r| r.redstone_conductor);
        if conductor_above {
            set_resting(&mut ai.mob, true);
        }
    }
}
