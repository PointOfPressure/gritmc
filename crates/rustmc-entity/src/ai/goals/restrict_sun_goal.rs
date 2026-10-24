//! `net.minecraft.world.entity.ai.goal.RestrictSunGoal`, from the 26.2
//! jar. No flags. Used by AbstractSkeleton.
//!
//! `canUse()`: `level.isBrightOutside() && getItemBySlot(HEAD).isEmpty()
//! && GoalUtils.hasGroundPathNavigation(mob)`. `start()`: `((GroundPathNavigation)
//! navigation).setAvoidSun(true)`; `stop()`: the same with `false`, guarded
//! by `hasGroundPathNavigation`.

use crate::ai::goal::{FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::ai::random_pos::has_ground_path_navigation;
use crate::host::{MobHost, PathLevel};

/// `RestrictSunGoal`.
#[derive(Debug, Clone, Default)]
pub struct RestrictSunGoal;

impl RestrictSunGoal {
    /// `RestrictSunGoal(PathfinderMob)`.
    #[must_use]
    pub fn new() -> Self {
        RestrictSunGoal
    }
}

impl<M: MobHost> Goal<MobAi<M>> for RestrictSunGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        cx.mob.level().is_bright_outside()
            && cx.mob.head_slot_empty()
            && has_ground_path_navigation(cx)
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        cx.navigation.set_avoid_sun(true);
    }

    fn stop(&mut self, cx: &mut MobAi<M>) {
        if has_ground_path_navigation(cx) {
            cx.navigation.set_avoid_sun(false);
        }
    }

    fn flags(&self) -> FlagSet {
        FlagSet::NONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn avoids_the_sun_by_day_with_a_bare_head() {
        let mut g = GridLevel::new();
        g.bright_outside = true;
        let mob = TestMob::new(g, 0.5, 64.0, 0.5, 1);
        let mut ai = MobAi::new(mob);
        let mut goal = RestrictSunGoal::new();
        assert!(goal.can_use(&mut ai));
        goal.start(&mut ai);
        assert!(ai.navigation.avoids_sun());
        goal.stop(&mut ai);
        assert!(!ai.navigation.avoids_sun());
        ai.mob.head_slot_empty = false;
        assert!(!goal.can_use(&mut ai));
    }
}
