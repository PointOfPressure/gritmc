//! `net.minecraft.world.entity.ai.goal.MoveBackToVillageGoal`, from the
//! 26.2 jar: `RandomStrollGoal(mob, speed, 10, checkNoActionTime)` whose
//! `canUse()` refuses when `level.isVillage(mob.blockPosition())` and
//! otherwise defers to the base, and whose `getPosition()` is the
//! [`crate::ai::goals::random_stroll_goal::StrollKind::BackToVillage`]
//! arm. Used by IronGolem.

use crate::ai::goal::{FlagSet, Goal};
use crate::ai::goals::random_stroll_goal::RandomStrollGoal;
use crate::ai::mob_ai::MobAi;
use crate::host::MobHost;
use crate::phys::Vec3;

/// `MoveBackToVillageGoal`.
#[derive(Debug, Clone)]
pub struct MoveBackToVillageGoal {
    inner: RandomStrollGoal,
}

impl MoveBackToVillageGoal {
    /// `MoveBackToVillageGoal(PathfinderMob, double, boolean)`.
    #[must_use]
    pub fn new(speed_modifier: f64, check_no_action_time: bool) -> Self {
        MoveBackToVillageGoal {
            inner: RandomStrollGoal::back_to_village(speed_modifier, check_no_action_time),
        }
    }

    /// The wanted position, for tests.
    #[must_use]
    pub fn wanted(&self) -> Vec3 {
        self.inner.wanted()
    }
}

impl<M: MobHost> Goal<MobAi<M>> for MoveBackToVillageGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        if cx.mob.is_village(cx.mob.block_position()) {
            return false;
        }
        self.inner.can_use(cx)
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        self.inner.can_continue_to_use(cx)
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        self.inner.start(cx);
    }

    fn stop(&mut self, cx: &mut MobAi<M>) {
        self.inner.stop(cx);
    }

    fn flags(&self) -> FlagSet {
        Goal::<MobAi<M>>::flags(&self.inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{GridLevel, TestMob};

    /// A golem two sections east of a village section: forced (no
    /// `nextInt(10)` gate), the candidate lies west of it; inside the
    /// village: refused.
    #[test]
    fn walks_toward_the_closest_village_section() {
        let mut g = GridLevel::new();
        g.floor(-60, -60, 60, 60, 63);
        let mut mob = TestMob::new(g, 40.5, 64.0, 8.5, 3);
        mob.village_sections.push((0, 4, 0));
        let mut ai = MobAi::new(mob);
        let mut goal = MoveBackToVillageGoal::new(0.6, false);
        let mut hit = false;
        for _ in 0..200 {
            if goal.can_use(&mut ai) {
                hit = true;
                break;
            }
        }
        assert!(hit);
        assert!(goal.wanted().x < 40.5, "{:?}", goal.wanted());
        ai.mob.village_sections.push((2, 4, 0));
        assert!(!goal.can_use(&mut ai), "already in a village");
    }
}
