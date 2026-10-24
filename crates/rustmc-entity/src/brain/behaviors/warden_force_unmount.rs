//! `net.minecraft.world.entity.ai.behavior.warden.ForceUnmount`, from the
//! 26.2 jar. `Behavior<LivingEntity>` with an **empty** entry condition
//! (`ImmutableMap.of()`) and the base's default duration of 60.
//!
//! ```text
//! checkExtraStartConditions (0..4):  mob.isPassenger()
//! start (0..4):                      mob.unRide()
//! ```
//!
//! The whole class. An empty entry map means the gate never refuses it on
//! memories, so the passenger test is the only guard.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `ForceUnmount`'s body.
#[derive(Debug, Clone, Copy, Default)]
pub struct ForceUnmount;

impl ForceUnmount {
    /// `ForceUnmount()`.
    #[must_use]
    pub fn new() -> Behavior<ForceUnmount> {
        Behavior::new(ForceUnmount, vec![])
    }
}

impl<M: MobHost> BehaviorBody<M> for ForceUnmount {
    fn check_extra_start_conditions(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        cx.mob.is_passenger()
    }

    fn start(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        cx.mob.un_ride();
    }

    fn name(&self) -> &'static str {
        "ForceUnmount"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn only_a_passenger_is_unmounted_and_the_entry_map_is_empty() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut brain = Brain::<TestMob>::new(&[], vec![]).state;
        let mut b = ForceUnmount::new();
        assert!(b.entry_condition.is_empty(), "ImmutableMap.of()");
        assert!(!b.try_start(&mut brain, &mut cx, 0), "not riding");
        cx.mob.is_passenger = true;
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(cx.mob.calls.contains(&"un_ride".to_string()));
    }
}
