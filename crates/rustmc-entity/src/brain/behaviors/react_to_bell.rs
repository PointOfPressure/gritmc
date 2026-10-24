//! `net.minecraft.world.entity.ai.behavior.ReactToBell`, from the 26.2 jar.
//! A `BehaviorBuilder` one-shot over `group(present(HEARD_BELL_TIME))`.
//!
//! ```text
//! lambda$create$2 (0..26):
//!   raid = level.getRaidAt(mob.blockPosition())
//!   if raid == null: brain.setActiveActivityIfPossible(HIDE)
//!   return true
//! ```
//!
//! No random draw; the bell time is read for presence only.

use crate::ai::mob_ai::MobAi;
use crate::brain::activity::Activity;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::memory::MemoryModuleType;
use crate::brain::BrainState;
use crate::host::MobHost;

/// `ReactToBell`'s trigger.
#[derive(Debug, Clone, Copy, Default)]
pub struct ReactToBell;

impl ReactToBell {
    /// `create()`.
    #[must_use]
    pub fn create() -> OneShot<Self> {
        OneShot::new(ReactToBell)
    }
}

impl<M: MobHost> Trigger<M> for ReactToBell {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        let Some(Some(_)) = brain.memory_internal(MemoryModuleType::HeardBellTime) else {
            return false;
        };
        if cx.mob.raid_at(cx.mob.block_position()).is_none() {
            brain.set_active_activity_if_possible(Activity::Hide);
        }
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![MemoryModuleType::HeardBellTime]
    }

    fn name(&self) -> &'static str {
        "ReactToBell"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::{MemoryModuleType as T, MemoryValue};
    use crate::brain::Brain;
    use crate::host::RaidFacts;
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn hides_without_a_raid_and_stays_with_one() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut brain = Brain::<TestMob>::new(&[T::HeardBellTime], vec![]);
        brain.add_activity_with_priority(Activity::Hide, 0, vec![]);
        let mut brain = brain.state;
        brain.set_core_activities(vec![Activity::Core]);
        brain.set_active_activity_if_possible(Activity::Idle);
        let mut b = ReactToBell::create();
        assert!(!b.try_start(&mut brain, &mut cx, 0), "no bell heard");
        brain.set_memory_value(T::HeardBellTime, MemoryValue::Long(5));
        cx.mob.raid = Some(RaidFacts::default());
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(!brain.is_active(Activity::Hide), "a raid: no hiding");
        cx.mob.raid = None;
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(brain.is_active(Activity::Hide));
    }
}
