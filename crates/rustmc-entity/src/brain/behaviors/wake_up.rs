//! `net.minecraft.world.entity.ai.behavior.WakeUp`, from the 26.2 jar. A
//! `BehaviorBuilder` one-shot over `point(trigger)` — no memories.
//!
//! ```text
//! lambda$create$1 (0..27):
//!   if brain.isActive(REST) || !mob.isSleeping(): return false
//!   mob.stopSleeping(); return true
//! ```
//!
//! No random draw.

use crate::ai::mob_ai::MobAi;
use crate::brain::activity::Activity;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `WakeUp`'s trigger.
#[derive(Debug, Clone, Copy, Default)]
pub struct WakeUp;

impl WakeUp {
    /// `create()`.
    #[must_use]
    pub fn create() -> OneShot<Self> {
        OneShot::new(WakeUp)
    }
}

impl<M: MobHost> Trigger<M> for WakeUp {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        if brain.is_active(Activity::Rest) || !cx.mob.is_sleeping() {
            return false;
        }
        cx.mob.stop_sleeping();
        true
    }

    fn name(&self) -> &'static str {
        "WakeUp"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn wakes_only_a_sleeper_outside_rest() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut brain = Brain::<TestMob>::new(&[], vec![]);
        brain.add_activity_with_priority(Activity::Rest, 0, vec![]);
        brain.add_activity_with_priority(Activity::Idle, 0, vec![]);
        let mut brain = brain.state;
        let mut b = WakeUp::create();
        assert!(!b.try_start(&mut brain, &mut cx, 0), "awake");
        cx.mob.sleeping = true;
        brain.set_core_activities(vec![Activity::Rest]);
        brain.set_active_activity_if_possible(Activity::Rest);
        assert!(!b.try_start(&mut brain, &mut cx, 0), "REST is active");
        brain.set_core_activities(vec![Activity::Core]);
        brain.set_active_activity_if_possible(Activity::Idle);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(!cx.mob.sleeping);
        assert_eq!(cx.mob.calls, ["stop_sleeping"]);
    }
}
