//! `net.minecraft.world.entity.ai.behavior.warden.SonicBoom`, from the
//! 26.2 jar. `Behavior<Warden>` with entry `{ATTACK_TARGET: VALUE_PRESENT,
//! SONIC_BOOM_COOLDOWN: VALUE_ABSENT, SONIC_BOOM_SOUND_COOLDOWN:
//! REGISTERED, SONIC_BOOM_SOUND_DELAY: REGISTERED}` and duration
//! `DURATION` = `Mth.ceil(60.0f)` = **60**.
//!
//! ```text
//! DISTANCE_XZ = 15; DISTANCE_Y = 20; KNOCKBACK_VERTICAL = 0.5; KNOCKBACK_HORIZONTAL = 2.5
//! COOLDOWN = 40;  TICKS_BEFORE_PLAYING_SOUND = Mth.ceil(34.0) = 34;  DURATION = 60
//!
//! checkExtraStartConditions (0..26):
//!   warden.closerThan(brain.getMemory(ATTACK_TARGET).get(), 15.0, 20.0)
//! canStillUse: true
//! start (0..49):
//!   brain.setMemoryWithExpiry(ATTACK_COOLING_DOWN, true, DURATION)
//!   brain.setMemoryWithExpiry(SONIC_BOOM_SOUND_DELAY, Unit.INSTANCE, TICKS_BEFORE_PLAYING_SOUND)
//!   level.broadcastEntityEvent(warden, (byte) 62)
//!   warden.playSound(WARDEN_SONIC_CHARGE, 3.0f, 1.0f)
//! tick (0..107):
//!   brain.getMemory(ATTACK_TARGET).ifPresent(e -> warden.getLookControl().setLookAt(e.position()))
//!   if !brain.hasMemoryValue(SONIC_BOOM_SOUND_DELAY) && !brain.hasMemoryValue(SONIC_BOOM_SOUND_COOLDOWN):
//!       brain.setMemoryWithExpiry(SONIC_BOOM_SOUND_COOLDOWN, Unit.INSTANCE, DURATION - TICKS_BEFORE_PLAYING_SOUND)
//!       brain.getMemory(ATTACK_TARGET)
//!            .filter(warden::canTargetEntity)
//!            .filter(e -> warden.closerThan(e, 15.0, 20.0))
//!            .ifPresent(e -> the beam)
//! stop (0..4):  setCooldown(warden, 40)
//! setCooldown:  brain.setMemoryWithExpiry(SONIC_BOOM_COOLDOWN, Unit.INSTANCE, cooldown)
//! ```
//!
//! The beam (`lambda$tick$2`, 0..195), which is one host call here because
//! every step of it needs the level:
//! `v = warden.position().add(attachment(WARDEN_CHEST, 0, warden.getYRot()))
//! .subtract(target.getEyePosition())` — note the **subtraction is
//! chest-minus-eye**, so the unit vector points from the target back at
//! the warden and the particles are laid `Mth.floor(v.length()) + 7` steps
//! along it; then `WARDEN_SONIC_BOOM` at 3.0f, `hurtServer(sonicBoom, 10.0f)`,
//! and on a successful hurt a push of
//! `(u.x * 2.5, u.y * 0.5, u.z * 2.5)` each scaled by
//! `1 - getAttributeValue(KNOCKBACK_RESISTANCE)`.
//!
//! **Three separate cooldowns and they do different work.**
//! `SONIC_BOOM_SOUND_DELAY` (34) delays the beam inside one use;
//! `SONIC_BOOM_SOUND_COOLDOWN` (60 - 34 = 26) stops a second beam inside
//! the same use; `SONIC_BOOM_COOLDOWN` (40, written by `stop`) is the gap
//! between uses and is the only one in the entry condition.
//!
//! `checkExtraStartConditions` reads `ATTACK_TARGET` with `Optional.get`
//! rather than `tryGet`, which is safe only because the entry condition
//! already demands `VALUE_PRESENT`.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::memory::{MemoryModuleType, MemoryStatus, MemoryValue};
use crate::brain::BrainState;
use crate::host::{MobHost, SoundSpec};

/// `DISTANCE_XZ`.
pub const DISTANCE_XZ: f64 = 15.0;
/// `DISTANCE_Y`.
pub const DISTANCE_Y: f64 = 20.0;
/// `KNOCKBACK_VERTICAL`.
pub const KNOCKBACK_VERTICAL: f64 = 0.5;
/// `KNOCKBACK_HORIZONTAL`.
pub const KNOCKBACK_HORIZONTAL: f64 = 2.5;
/// `COOLDOWN`.
pub const COOLDOWN: i32 = 40;
/// `TICKS_BEFORE_PLAYING_SOUND` = `Mth.ceil(34.0)`.
pub const TICKS_BEFORE_PLAYING_SOUND: i32 = 34;
/// `DURATION` = `Mth.ceil(60.0f)`.
pub const DURATION: i32 = 60;
/// The entity event `start` broadcasts.
pub const SONIC_BOOM_EVENT: u8 = 62;

/// `warden.SonicBoom`'s body.
#[derive(Debug, Clone, Copy, Default)]
pub struct SonicBoom;

impl SonicBoom {
    /// `SonicBoom()`.
    #[must_use]
    pub fn new() -> Behavior<SonicBoom> {
        Behavior::with_duration(
            SonicBoom,
            vec![
                (MemoryModuleType::AttackTarget, MemoryStatus::ValuePresent),
                (
                    MemoryModuleType::SonicBoomCooldown,
                    MemoryStatus::ValueAbsent,
                ),
                (
                    MemoryModuleType::SonicBoomSoundCooldown,
                    MemoryStatus::Registered,
                ),
                (
                    MemoryModuleType::SonicBoomSoundDelay,
                    MemoryStatus::Registered,
                ),
            ],
            DURATION,
        )
    }

    /// `SonicBoom.setCooldown(mob, ticks)`, which `WardenAi` also calls
    /// from outside this behaviour.
    pub fn set_cooldown(brain: &mut BrainState, ticks: i32) {
        brain.set_memory_with_expiry(
            MemoryModuleType::SonicBoomCooldown,
            MemoryValue::Unit,
            i64::from(ticks),
        );
    }

    /// `warden.closerThan(e, 15.0, 20.0)`.
    fn in_range<M: MobHost>(cx: &MobAi<M>, id: i32) -> bool {
        let Some(f) = cx.mob.entity(id) else {
            return false;
        };
        let dx = f.x - cx.mob.x();
        let dy = f.y - cx.mob.y();
        let dz = f.z - cx.mob.z();
        dx * dx + dz * dz < DISTANCE_XZ * DISTANCE_XZ && dy * dy < DISTANCE_Y * DISTANCE_Y
    }
}

impl<M: MobHost> BehaviorBody<M> for SonicBoom {
    fn check_extra_start_conditions(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        let target = brain
            .get_memory(MemoryModuleType::AttackTarget)
            .and_then(MemoryValue::as_entity)
            .expect("ATTACK_TARGET.get()");
        Self::in_range(cx, target)
    }

    fn can_still_use(&mut self, _brain: &mut BrainState, _cx: &mut MobAi<M>, _time: i64) -> bool {
        true
    }

    fn start(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        brain.set_memory_with_expiry(
            MemoryModuleType::AttackCoolingDown,
            MemoryValue::Bool(true),
            i64::from(DURATION),
        );
        brain.set_memory_with_expiry(
            MemoryModuleType::SonicBoomSoundDelay,
            MemoryValue::Unit,
            i64::from(TICKS_BEFORE_PLAYING_SOUND),
        );
        let me = cx.mob.id();
        cx.mob.other_broadcast_entity_event(me, SONIC_BOOM_EVENT);
        cx.mob
            .play_sound(&SoundSpec("warden.sonic_charge"), 3.0, 1.0);
    }

    fn tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        let target = brain
            .get_memory(MemoryModuleType::AttackTarget)
            .and_then(MemoryValue::as_entity);
        if let Some(id) = target {
            if let Some(f) = cx.mob.entity(id) {
                cx.mob.look_control_set_look_at(crate::phys::Vec3 {
                    x: f.x,
                    y: f.y,
                    z: f.z,
                });
            }
        }
        if brain.has_memory_value(MemoryModuleType::SonicBoomSoundDelay)
            || brain.has_memory_value(MemoryModuleType::SonicBoomSoundCooldown)
        {
            return;
        }
        brain.set_memory_with_expiry(
            MemoryModuleType::SonicBoomSoundCooldown,
            MemoryValue::Unit,
            i64::from(DURATION - TICKS_BEFORE_PLAYING_SOUND),
        );
        let Some(id) = target else {
            return;
        };
        if cx.mob.can_target_entity(id) && Self::in_range(cx, id) {
            cx.mob.warden_sonic_boom_at(id);
        }
    }

    fn stop(&mut self, brain: &mut BrainState, _cx: &mut MobAi<M>, _time: i64) {
        Self::set_cooldown(brain, COOLDOWN);
    }

    fn name(&self) -> &'static str {
        "SonicBoom"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    fn setup() -> (MobAi<TestMob>, BrainState) {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        cx.mob.others = vec![(EntityClass::Living, EntityFacts::living(9, 8.5, 1.0, 0.5))];
        cx.mob.targetable = vec![9];
        let mut brain = Brain::<TestMob>::new(
            &[
                T::AttackTarget,
                T::SonicBoomCooldown,
                T::SonicBoomSoundCooldown,
                T::SonicBoomSoundDelay,
                T::AttackCoolingDown,
            ],
            vec![],
        )
        .state;
        brain.set_memory_value(T::AttackTarget, MemoryValue::Entity(9));
        (cx, brain)
    }

    #[test]
    fn the_three_cooldowns_do_three_different_jobs() {
        let (mut cx, mut brain) = setup();
        let mut b = SonicBoom::new();
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(brain.time_until_expiry(T::AttackCoolingDown), 60);
        assert_eq!(brain.time_until_expiry(T::SonicBoomSoundDelay), 34);
        assert!(cx.mob.calls.contains(&"other_event 1 62".to_string()));
        assert!(cx
            .mob
            .calls
            .contains(&"playSound(warden.sonic_charge, 3, 1)".to_string()));
        // The 34-tick delay holds the beam inside this use.
        b.tick_or_stop(&mut brain, &mut cx, 1);
        assert!(!cx.mob.calls.iter().any(|c| c.starts_with("sonic_boom")));
        assert!(cx.mob.calls.contains(&"look_at 8.5 1 0.5".to_string()));
        brain.erase_memory(T::SonicBoomSoundDelay);
        b.tick_or_stop(&mut brain, &mut cx, 2);
        assert!(cx.mob.calls.contains(&"sonic_boom 9".to_string()));
        assert_eq!(
            brain.time_until_expiry(T::SonicBoomSoundCooldown),
            26,
            "60 - 34, which stops a second beam inside this use"
        );
        cx.mob.calls.clear();
        b.tick_or_stop(&mut brain, &mut cx, 3);
        assert!(!cx.mob.calls.iter().any(|c| c.starts_with("sonic_boom")));
        // stop writes the between-uses cooldown, the only one in the entry.
        b.do_stop(&mut brain, &mut cx, 60);
        assert_eq!(brain.time_until_expiry(T::SonicBoomCooldown), 40);
        assert!(!b.try_start(&mut brain, &mut cx, 61));
    }

    #[test]
    fn the_range_is_a_disc_of_15_with_a_vertical_20_and_gates_both_start_and_beam() {
        let (mut cx, mut brain) = setup();
        let mut b = SonicBoom::new();
        cx.mob.others[0].1.x = 16.5;
        assert!(!b.try_start(&mut brain, &mut cx, 0), "16 > 15 horizontally");
        cx.mob.others[0].1.x = 0.5;
        cx.mob.others[0].1.y = 22.0;
        assert!(!b.try_start(&mut brain, &mut cx, 0), "21 > 20 vertically");
        cx.mob.others[0].1.y = 1.0;
        assert!(b.try_start(&mut brain, &mut cx, 0));
        // Out of range by the time the beam is due: no beam, but the sound
        // cooldown is still armed, because the write is before the filters.
        brain.erase_memory(T::SonicBoomSoundDelay);
        cx.mob.others[0].1.x = 40.5;
        b.tick_or_stop(&mut brain, &mut cx, 40);
        assert!(!cx.mob.calls.iter().any(|c| c.starts_with("sonic_boom")));
        assert_eq!(brain.time_until_expiry(T::SonicBoomSoundCooldown), 26);
    }
}
