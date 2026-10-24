//! `net.minecraft.world.entity.animal.axolotl.PlayDead` and its companion
//! `axolotl.ValidatePlayDead`, from the 26.2 jar.
//!
//! ```text
//! PlayDead()   Behavior<Axolotl>, entry {PLAY_DEAD_TICKS: VALUE_PRESENT, HURT_BY_ENTITY: VALUE_PRESENT}, duration 200
//!   checkExtraStartConditions (0..4):  axolotl.isInWater()
//!   canStillUse (0..25):               axolotl.isInWater() && brain.hasMemoryValue(PLAY_DEAD_TICKS)
//!   start (0..37):                     brain.eraseMemory(WALK_TARGET); brain.eraseMemory(LOOK_TARGET)
//!                                      axolotl.addEffect(new MobEffectInstance(REGENERATION, 200, 0))
//!
//! ValidatePlayDead.create()   one-shot over group(present(PLAY_DEAD_TICKS), registered(HURT_BY_ENTITY))
//!   trigger (lambda$create$2, 0..49):
//!     n = get(PLAY_DEAD_TICKS)
//!     if n <= 0: playDeadTicks.erase(); hurtByEntity.erase(); brain.useDefaultActivity()
//!     else:      playDeadTicks.set(n - 1)
//!     return true
//! ```
//!
//! **`PlayDead` has no `tick` and no `stop`.** It erases the two movement
//! memories once and applies the effect once; the countdown that ends it
//! is `ValidatePlayDead`'s, running in a different activity. The 200-tick
//! duration and the 200-tick `REGENERATION` are the same number by
//! coincidence of the constants, not by construction.
//!
//! **`ValidatePlayDead` returns `true` on both paths** and its `<= 0`
//! branch calls `Brain.useDefaultActivity()`, which is what actually takes
//! the axolotl out of the play-dead activity — the memory erase alone
//! would not.
//!
//! `HURT_BY_ENTITY` is `present` for `PlayDead` and only `registered` for
//! `ValidatePlayDead`, so the countdown keeps running after the attacker
//! memory has expired.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{Behavior, BehaviorBody, OneShot, Trigger};
use crate::brain::memory::{MemoryModuleType, MemoryStatus, MemoryValue};
use crate::brain::BrainState;
use crate::host::MobHost;

/// The `MobEffectInstance(REGENERATION, 200, 0)` `start` applies.
pub const REGENERATION_DURATION: i32 = 200;
/// `PlayDead`'s behaviour duration.
pub const PLAY_DEAD_DURATION: i32 = 200;

/// `axolotl.PlayDead`'s body.
#[derive(Debug, Clone, Copy, Default)]
pub struct PlayDead;

impl PlayDead {
    /// `PlayDead()`.
    #[must_use]
    pub fn new() -> Behavior<PlayDead> {
        Behavior::with_duration(
            PlayDead,
            vec![
                (MemoryModuleType::PlayDeadTicks, MemoryStatus::ValuePresent),
                (MemoryModuleType::HurtByEntity, MemoryStatus::ValuePresent),
            ],
            PLAY_DEAD_DURATION,
        )
    }
}

impl<M: MobHost> BehaviorBody<M> for PlayDead {
    fn check_extra_start_conditions(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        cx.mob.is_in_water()
    }

    fn can_still_use(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        cx.mob.is_in_water() && brain.has_memory_value(MemoryModuleType::PlayDeadTicks)
    }

    fn start(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        brain.erase_memory(MemoryModuleType::WalkTarget);
        brain.erase_memory(MemoryModuleType::LookTarget);
        cx.mob.add_effect("regeneration", REGENERATION_DURATION, 0);
    }

    fn name(&self) -> &'static str {
        "PlayDead"
    }
}

/// `axolotl.ValidatePlayDead`'s trigger.
#[derive(Debug, Clone, Copy, Default)]
pub struct ValidatePlayDead;

impl ValidatePlayDead {
    /// `create()`.
    #[must_use]
    pub fn create() -> OneShot<Self> {
        OneShot::new(ValidatePlayDead)
    }
}

impl<M: MobHost> Trigger<M> for ValidatePlayDead {
    fn trigger(&mut self, brain: &mut BrainState, _cx: &mut MobAi<M>, _time: i64) -> bool {
        if !brain.check_memory(MemoryModuleType::PlayDeadTicks, MemoryStatus::ValuePresent) {
            return false;
        }
        let Some(MemoryValue::Int(n)) = brain.get_memory(MemoryModuleType::PlayDeadTicks) else {
            return false;
        };
        let n = *n;
        if n <= 0 {
            brain.erase_memory(MemoryModuleType::PlayDeadTicks);
            brain.erase_memory(MemoryModuleType::HurtByEntity);
            brain.use_default_activity();
        } else {
            brain.set_memory_value(MemoryModuleType::PlayDeadTicks, MemoryValue::Int(n - 1));
        }
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![
            MemoryModuleType::PlayDeadTicks,
            MemoryModuleType::HurtByEntity,
        ]
    }

    fn name(&self) -> &'static str {
        "ValidatePlayDead"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};

    fn setup() -> (MobAi<TestMob>, BrainState) {
        let cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let brain = Brain::<TestMob>::new(
            &[
                T::PlayDeadTicks,
                T::HurtByEntity,
                T::WalkTarget,
                T::LookTarget,
            ],
            vec![],
        )
        .state;
        (cx, brain)
    }

    #[test]
    fn play_dead_needs_water_and_clears_both_movement_memories_once() {
        let (mut cx, mut brain) = setup();
        brain.set_memory_value(T::PlayDeadTicks, MemoryValue::Int(200));
        brain.set_memory_value(T::HurtByEntity, MemoryValue::Entity(9));
        brain.set_memory_value(
            T::WalkTarget,
            MemoryValue::WalkTarget(crate::brain::memory::WalkTarget::at_block(
                rustmc_data::BlockPos { x: 1, y: 1, z: 1 },
                1.0,
                0,
            )),
        );
        let mut b = PlayDead::new();
        assert!(!b.try_start(&mut brain, &mut cx, 0), "not in water");
        cx.mob.in_water = true;
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(!brain.has_memory_value(T::WalkTarget));
        assert!(!brain.has_memory_value(T::LookTarget));
        assert_eq!(
            cx.mob.calls,
            vec!["add_effect regeneration 200 0".to_string()]
        );
        // Leaving the water ends it even with ticks left.
        cx.mob.in_water = false;
        b.tick_or_stop(&mut brain, &mut cx, 1);
        assert_eq!(
            BehaviorControl::<TestMob>::status(&b),
            crate::brain::behavior::Status::Stopped
        );
    }

    #[test]
    fn validate_counts_down_and_the_zero_branch_leaves_the_activity() {
        let (mut cx, mut brain) = setup();
        brain.set_memory_value(T::PlayDeadTicks, MemoryValue::Int(2));
        brain.set_memory_value(T::HurtByEntity, MemoryValue::Entity(9));
        let mut v = ValidatePlayDead::create();
        for expect in [1, 0] {
            assert!(v.try_start(&mut brain, &mut cx, 0));
            assert_eq!(
                brain.get_memory(T::PlayDeadTicks),
                Some(&MemoryValue::Int(expect))
            );
        }
        // 0 is not > 0: the next run takes the erase branch.
        assert!(v.try_start(&mut brain, &mut cx, 0));
        assert!(!brain.has_memory_value(T::PlayDeadTicks));
        assert!(
            !brain.has_memory_value(T::HurtByEntity),
            "the attacker memory goes too"
        );
        assert!(
            !v.try_start(&mut brain, &mut cx, 0),
            "nothing left to count"
        );
    }
}
