//! `net.minecraft.world.entity.ai.behavior.warden.Roar`, from the 26.2
//! jar. `Behavior<Warden>` with entry `{ROAR_TARGET: VALUE_PRESENT,
//! ATTACK_TARGET: VALUE_ABSENT, ROAR_SOUND_COOLDOWN: REGISTERED,
//! ROAR_SOUND_DELAY: REGISTERED}` and duration `WardenAi.ROAR_DURATION` =
//! `Mth.ceil(84.0f)` = **84**.
//!
//! ```text
//! TICKS_BEFORE_PLAYING_ROAR_SOUND = 25;  ROAR_ANGER_INCREASE = 20
//! start (0..65):
//!   brain.setMemoryWithExpiry(ROAR_SOUND_DELAY, Unit.INSTANCE, 25L)
//!   brain.eraseMemory(WALK_TARGET)
//!   target = brain.getMemory(ROAR_TARGET).get()
//!   BehaviorUtils.lookAtEntity(warden, target)
//!   warden.setPose(Pose.ROARING)
//!   warden.increaseAngerAt(target, 20, false)
//! canStillUse: true
//! tick (0..54):
//!   if !brain.hasMemoryValue(ROAR_SOUND_DELAY) && !brain.hasMemoryValue(ROAR_SOUND_COOLDOWN):
//!       brain.setMemoryWithExpiry(ROAR_SOUND_COOLDOWN, Unit.INSTANCE, ROAR_DURATION - 25)
//!       warden.playSound(WARDEN_ROAR, 3.0f, 1.0f)
//! stop (0..51):
//!   if warden.hasPose(ROARING): warden.setPose(STANDING)
//!   brain.getMemory(ROAR_TARGET).ifPresent(warden::setAttackTarget)
//!   brain.eraseMemory(ROAR_TARGET)
//! ```
//!
//! **The roar sound is played by `tick`, not by `start`.** `start` arms a
//! 25-tick `ROAR_SOUND_DELAY` and the first tick that finds it expired
//! *and* no `ROAR_SOUND_COOLDOWN` plays it and arms a `84 - 25 = 59`-tick
//! cooldown — which outlives the behaviour, so a second roar started
//! immediately after this one is silent.
//!
//! `stop` promotes the roar target to `ATTACK_TARGET` through
//! `Warden::setAttackTarget` **before** erasing `ROAR_TARGET`, so the
//! warden always leaves a roar with something to attack.
//!
//! `increaseAngerAt(target, 20, false)` is the three-argument form: the
//! `false` is `alertOthers`, so a roar angers this warden alone.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::behaviors::behavior_utils::look_at_entity;
use crate::brain::memory::{MemoryModuleType, MemoryStatus, MemoryValue};
use crate::brain::BrainState;
use crate::host::{MobHost, Pose, SoundSpec};

/// `WardenAi.ROAR_DURATION` = `Mth.ceil(84.0f)`.
pub const ROAR_DURATION: i32 = 84;
/// `TICKS_BEFORE_PLAYING_ROAR_SOUND`.
pub const TICKS_BEFORE_PLAYING_ROAR_SOUND: i32 = 25;
/// `ROAR_ANGER_INCREASE`.
pub const ROAR_ANGER_INCREASE: i32 = 20;

/// `warden.Roar`'s body.
#[derive(Debug, Clone, Copy, Default)]
pub struct Roar;

impl Roar {
    /// `Roar()`.
    #[must_use]
    pub fn new() -> Behavior<Roar> {
        Behavior::with_duration(
            Roar,
            vec![
                (MemoryModuleType::RoarTarget, MemoryStatus::ValuePresent),
                (MemoryModuleType::AttackTarget, MemoryStatus::ValueAbsent),
                (
                    MemoryModuleType::RoarSoundCooldown,
                    MemoryStatus::Registered,
                ),
                (MemoryModuleType::RoarSoundDelay, MemoryStatus::Registered),
            ],
            ROAR_DURATION,
        )
    }
}

impl<M: MobHost> BehaviorBody<M> for Roar {
    fn start(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        brain.set_memory_with_expiry(
            MemoryModuleType::RoarSoundDelay,
            MemoryValue::Unit,
            i64::from(TICKS_BEFORE_PLAYING_ROAR_SOUND),
        );
        brain.erase_memory(MemoryModuleType::WalkTarget);
        let target = brain
            .get_memory(MemoryModuleType::RoarTarget)
            .and_then(MemoryValue::as_entity)
            .expect("ROAR_TARGET.get()");
        look_at_entity(brain, target);
        cx.mob.set_pose(Pose::Roaring);
        cx.mob
            .warden_increase_anger_at_by(target, ROAR_ANGER_INCREASE, false);
    }

    fn can_still_use(&mut self, _brain: &mut BrainState, _cx: &mut MobAi<M>, _time: i64) -> bool {
        true
    }

    fn tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        if brain.has_memory_value(MemoryModuleType::RoarSoundDelay)
            || brain.has_memory_value(MemoryModuleType::RoarSoundCooldown)
        {
            return;
        }
        brain.set_memory_with_expiry(
            MemoryModuleType::RoarSoundCooldown,
            MemoryValue::Unit,
            i64::from(ROAR_DURATION - TICKS_BEFORE_PLAYING_ROAR_SOUND),
        );
        cx.mob.play_sound(&SoundSpec("warden.roar"), 3.0, 1.0);
    }

    fn stop(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        if cx.mob.has_pose(Pose::Roaring) {
            cx.mob.set_pose(Pose::Standing);
        }
        if let Some(id) = brain
            .get_memory(MemoryModuleType::RoarTarget)
            .and_then(MemoryValue::as_entity)
        {
            cx.mob.warden_set_attack_target(id);
        }
        brain.erase_memory(MemoryModuleType::RoarTarget);
    }

    fn name(&self) -> &'static str {
        "Roar"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn the_roar_sound_waits_25_ticks_and_then_arms_a_59_tick_cooldown() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut brain = Brain::<TestMob>::new(
            &[
                T::RoarTarget,
                T::AttackTarget,
                T::RoarSoundCooldown,
                T::RoarSoundDelay,
                T::WalkTarget,
                T::LookTarget,
            ],
            vec![],
        )
        .state;
        brain.set_memory_value(T::RoarTarget, MemoryValue::Entity(9));
        let mut b = Roar::new();
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(b.end_timestamp(), i64::from(ROAR_DURATION));
        assert_eq!(cx.mob.pose, Pose::Roaring);
        assert_eq!(brain.time_until_expiry(T::RoarSoundDelay), 25);
        assert!(cx
            .mob
            .calls
            .contains(&"increase_anger 9 20 false".to_string()));
        assert!(brain.has_memory_value(T::LookTarget));
        // While the delay stands, tick is silent.
        b.tick_or_stop(&mut brain, &mut cx, 1);
        assert!(!cx
            .mob
            .calls
            .iter()
            .any(|c| c.starts_with("playSound(warden.roar")));
        brain.erase_memory(T::RoarSoundDelay);
        b.tick_or_stop(&mut brain, &mut cx, 2);
        assert!(cx
            .mob
            .calls
            .contains(&"playSound(warden.roar, 3, 1)".to_string()));
        assert_eq!(
            brain.time_until_expiry(T::RoarSoundCooldown),
            i64::from(ROAR_DURATION - TICKS_BEFORE_PLAYING_ROAR_SOUND),
            "84 - 25 = 59, which outlives this roar"
        );
        // And it does not roar twice.
        cx.mob.calls.clear();
        b.tick_or_stop(&mut brain, &mut cx, 3);
        assert!(cx.mob.calls.is_empty());
    }

    #[test]
    fn stop_promotes_the_roar_target_to_the_attack_target_before_erasing_it() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut brain = Brain::<TestMob>::new(
            &[
                T::RoarTarget,
                T::AttackTarget,
                T::RoarSoundCooldown,
                T::RoarSoundDelay,
                T::WalkTarget,
                T::LookTarget,
            ],
            vec![],
        )
        .state;
        brain.set_memory_value(T::RoarTarget, MemoryValue::Entity(9));
        let mut b = Roar::new();
        assert!(b.try_start(&mut brain, &mut cx, 0));
        cx.mob.calls.clear();
        b.do_stop(&mut brain, &mut cx, 84);
        assert_eq!(cx.mob.pose, Pose::Standing);
        assert_eq!(cx.mob.calls, vec!["set_attack_target 9".to_string()]);
        assert!(!brain.has_memory_value(T::RoarTarget));
    }
}
