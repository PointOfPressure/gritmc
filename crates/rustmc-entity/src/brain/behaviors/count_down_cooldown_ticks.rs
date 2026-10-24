//! `net.minecraft.world.entity.ai.behavior.CountDownCooldownTicks`, from the
//! 26.2 jar. `Behavior<LivingEntity>` with entry `{cooldownTicks: VALUE_PRESENT}`
//! and the default `(60, 60)` duration — but `timedOut` is overridden to
//! `false`, so the level-random draw on start is made and never read.
//!
//! ```text
//! canStillUse (0..34):  memory present && value > 0
//! tick (0..34):         brain.setMemory(type, value - 1)
//! stop (0..11):         brain.eraseMemory(type)
//! ```
//!
//! So a cooldown of `n` counts `n, n-1, …, 1` over `n` ticks and is erased
//! on the tick it would read `0`.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::memory::{MemoryModuleType, MemoryStatus, MemoryValue};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `CountDownCooldownTicks`'s body; build one with [`CountDownCooldownTicks::new`].
#[derive(Debug, Clone, Copy)]
pub struct CountDownCooldownTicks {
    pub cooldown_ticks: MemoryModuleType,
}

impl CountDownCooldownTicks {
    /// `CountDownCooldownTicks(MemoryModuleType<Integer>)`.
    #[must_use]
    pub fn new(cooldown_ticks: MemoryModuleType) -> Behavior<CountDownCooldownTicks> {
        Behavior::new(
            CountDownCooldownTicks { cooldown_ticks },
            vec![(cooldown_ticks, MemoryStatus::ValuePresent)],
        )
    }

    fn value(&self, brain: &BrainState) -> Option<i32> {
        brain
            .get_memory(self.cooldown_ticks)
            .and_then(MemoryValue::as_int)
    }
}

impl<M: MobHost> BehaviorBody<M> for CountDownCooldownTicks {
    fn timed_out(&self, _end_timestamp: i64, _time: i64) -> bool {
        false
    }

    fn can_still_use(&mut self, brain: &mut BrainState, _cx: &mut MobAi<M>, _time: i64) -> bool {
        self.value(brain).is_some_and(|v| v > 0)
    }

    fn tick(&mut self, brain: &mut BrainState, _cx: &mut MobAi<M>, _time: i64) {
        let v = self.value(brain).expect("present while running");
        brain.set_memory_value(self.cooldown_ticks, MemoryValue::Int(v - 1));
    }

    fn stop(&mut self, brain: &mut BrainState, _cx: &mut MobAi<M>, _time: i64) {
        brain.erase_memory(self.cooldown_ticks);
    }

    fn name(&self) -> &'static str {
        "CountDownCooldownTicks"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::{BehaviorControl, Status};
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn counts_three_down_over_three_ticks_and_erases_never_timing_out() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut brain = Brain::<TestMob>::new(&[T::TemptationCooldownTicks], vec![]).state;
        let mut b = CountDownCooldownTicks::new(T::TemptationCooldownTicks);
        assert!(!b.try_start(&mut brain, &mut cx, 0));
        brain.set_memory_value(T::TemptationCooldownTicks, MemoryValue::Int(3));
        assert!(b.try_start(&mut brain, &mut cx, 0));
        for (t, expect) in [(1000, 2), (1001, 1), (1002, 0)] {
            b.tick_or_stop(&mut brain, &mut cx, t);
            assert_eq!(
                brain.get_memory(T::TemptationCooldownTicks),
                Some(&MemoryValue::Int(expect)),
                "@{t}"
            );
            assert_eq!(BehaviorControl::<TestMob>::status(&b), Status::Running);
        }
        b.tick_or_stop(&mut brain, &mut cx, 1003);
        assert_eq!(BehaviorControl::<TestMob>::status(&b), Status::Stopped);
        assert!(!brain.has_memory_value(T::TemptationCooldownTicks));
    }
}
