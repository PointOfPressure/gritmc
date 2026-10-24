//! `net.minecraft.world.entity.ai.goal.MoveTowardsTargetGoal`, from the
//! 26.2 jar. Flag `MOVE`. Used by IronGolem.
//!
//! `canUse()`, offsets 0..100: `target = mob.getTarget(); if null false;
//! if target.distanceToSqr(mob) > within * within false;
//! v = DefaultRandomPos.getPosTowards(mob, 16, 7, target.position(), (float) PI / 2)`,
//! stored as the wanted position. `canContinueToUse()`: `!navigation.isDone()
//! && target.isAlive() && target.distanceToSqr(mob) < within * within`.
//! `stop()`: `target = null`; `start()`: `navigation.moveTo(wanted, speed)`.

use crate::ai::goal::{Flag, FlagSet, Goal};
use crate::ai::goals::move_towards_restriction_goal::HALF_PI_F32_AS_F64;
use crate::ai::mob_ai::MobAi;
use crate::ai::random_pos::default_get_pos_towards;
use crate::host::MobHost;

/// `MoveTowardsTargetGoal`.
#[derive(Debug, Clone)]
pub struct MoveTowardsTargetGoal {
    target: Option<i32>,
    wanted_x: f64,
    wanted_y: f64,
    wanted_z: f64,
    speed_modifier: f64,
    within: f32,
}

impl MoveTowardsTargetGoal {
    /// `MoveTowardsTargetGoal(PathfinderMob, double, float within)`.
    #[must_use]
    pub fn new(speed_modifier: f64, within: f32) -> Self {
        MoveTowardsTargetGoal {
            target: None,
            wanted_x: 0.0,
            wanted_y: 0.0,
            wanted_z: 0.0,
            speed_modifier,
            within,
        }
    }

    /// The wanted position, for tests.
    #[must_use]
    pub fn wanted(&self) -> (f64, f64, f64) {
        (self.wanted_x, self.wanted_y, self.wanted_z)
    }
}

impl<M: MobHost> Goal<MobAi<M>> for MoveTowardsTargetGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        self.target = cx.mob.target();
        let Some(t) = self.target.and_then(|id| cx.mob.entity(id)) else {
            return false;
        };
        let w = f64::from(self.within * self.within);
        if t.distance_to_sqr(cx.mob.x(), cx.mob.y(), cx.mob.z()) > w {
            return false;
        }
        let Some(v) = default_get_pos_towards(cx, 16, 7, t.position(), HALF_PI_F32_AS_F64) else {
            return false;
        };
        self.wanted_x = v.x;
        self.wanted_y = v.y;
        self.wanted_z = v.z;
        true
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        if cx.navigation.is_done() {
            return false;
        }
        let Some(t) = self.target.and_then(|id| cx.mob.entity(id)) else {
            return false;
        };
        t.alive
            && t.distance_to_sqr(cx.mob.x(), cx.mob.y(), cx.mob.z())
                < f64::from(self.within * self.within)
    }

    fn stop(&mut self, _cx: &mut MobAi<M>) {
        self.target = None;
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        let (x, y, z, s) = (
            self.wanted_x,
            self.wanted_y,
            self.wanted_z,
            self.speed_modifier,
        );
        cx.navigation.move_to(&mut cx.mob, x, y, z, s);
    }

    fn flags(&self) -> FlagSet {
        FlagSet::of(Flag::Move)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn heads_toward_a_target_within_range() {
        let mut g = GridLevel::new();
        g.floor(-40, -40, 40, 40, 63);
        let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, 2);
        mob.others
            .push((EntityClass::Player, EntityFacts::player(9, 20.5, 64.0, 0.5)));
        mob.target_raw = Some(9);
        let mut ai = MobAi::new(mob);
        let mut goal = MoveTowardsTargetGoal::new(0.9, 32.0);
        assert!(goal.can_use(&mut ai));
        assert!(goal.wanted().0 > 0.5);
        goal.start(&mut ai);
        assert!(goal.can_continue_to_use(&mut ai));
        ai.mob.others[0].1.x = 40.5;
        assert!(!goal.can_continue_to_use(&mut ai), "beyond within");
        assert!(!goal.can_use(&mut ai));
    }
}
