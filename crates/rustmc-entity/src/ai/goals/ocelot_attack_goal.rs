//! `net.minecraft.world.entity.ai.goal.OcelotAttackGoal`, from the 26.2
//! jar. Flags `MOVE, LOOK`. Used by Cat and Ocelot.
//!
//! `canUse()`: `target = mob.getTarget(); target != null`, storing it.
//! `canContinueToUse()`, offsets 0..57: `target.isAlive() &&
//! mob.distanceToSqr(target) <= 225.0 && (!navigation.isDone() || canUse())`.
//! `stop()`: `target = null; navigation.stop()`. Every tick.
//!
//! # `tick()`, offsets 0..179
//!
//! ```text
//! lookControl.setLookAt(target, 30.0f, 30.0f)
//! d = (double) (mob.getBbWidth() * 2.0f * (mob.getBbWidth() * 2.0f))     -- float math
//! e = mob.distanceToSqr(target.x, target.y, target.z)
//! speed = 0.8
//! if e > d && e < 16.0: speed = 1.33
//! else if e < 225.0: speed = 0.6
//! navigation.moveTo(target, speed)
//! attackTime = max(attackTime - 1, 0)
//! if e > d return
//! if attackTime > 0 return
//! attackTime = 20; mob.doHurtTarget(level, target)
//! ```

use crate::ai::goal::{Flag, FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::host::MobHost;

/// `OcelotAttackGoal`.
#[derive(Debug, Clone, Default)]
pub struct OcelotAttackGoal {
    target: Option<i32>,
    attack_time: i32,
}

impl OcelotAttackGoal {
    /// `OcelotAttackGoal(Mob)`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `attackTime`, for tests.
    #[must_use]
    pub fn attack_time(&self) -> i32 {
        self.attack_time
    }
}

impl<M: MobHost> Goal<MobAi<M>> for OcelotAttackGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        let Some(t) = cx.mob.target() else {
            return false;
        };
        self.target = Some(t);
        true
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        let Some(facts) = self.target.and_then(|t| cx.mob.entity(t)) else {
            return false;
        };
        if !facts.alive {
            return false;
        }
        if facts.distance_to_sqr(cx.mob.x(), cx.mob.y(), cx.mob.z()) > 225.0 {
            return false;
        }
        !cx.navigation.is_done() || self.can_use(cx)
    }

    fn stop(&mut self, cx: &mut MobAi<M>) {
        self.target = None;
        cx.navigation.stop();
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn tick(&mut self, cx: &mut MobAi<M>) {
        let Some(t) = self.target else {
            return;
        };
        let Some(facts) = cx.mob.entity(t) else {
            return;
        };
        cx.look_control
            .set_look_at_with(facts.x, facts.eye_y, facts.z, 30.0, 30.0);
        let w = cx.mob.bb_width() * 2.0;
        let d = f64::from(w * w);
        let e = facts.distance_to_sqr(cx.mob.x(), cx.mob.y(), cx.mob.z());
        let mut speed = 0.8;
        if e > d && e < 16.0 {
            speed = 1.33;
        } else if e < 225.0 {
            speed = 0.6;
        }
        cx.navigation
            .move_to_entity_at(&mut cx.mob, facts.block_position(), speed);
        self.attack_time = (self.attack_time - 1).max(0);
        if e > d {
            return;
        }
        if self.attack_time > 0 {
            return;
        }
        self.attack_time = 20;
        cx.mob.do_hurt_target(t);
    }

    fn flags(&self) -> FlagSet {
        FlagSet::of2(Flag::Move, Flag::Look)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    fn cat_with_prey_at(dx: f64) -> MobAi<TestMob> {
        let mut g = GridLevel::new();
        g.floor(-40, -40, 40, 40, 63);
        let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, 3);
        mob.on_ground = true;
        mob.bb_width = 0.6;
        mob.others.push((
            EntityClass::Living,
            EntityFacts::living(9, 0.5 + dx, 64.0, 0.5),
        ));
        mob.target_raw = Some(9);
        MobAi::new(mob)
    }

    /// Reach is `(0.6 * 2)^2 = 1.44`: at 1 block the cat bites every
    /// 20 ticks; at 3 blocks it sprints at 1.33 and does not bite; at 16
    /// blocks it can no longer continue.
    #[test]
    fn bites_within_twice_its_width_and_sprints_when_near() {
        let mut goal = OcelotAttackGoal::new();
        let mut ai = cat_with_prey_at(1.0);
        assert!(goal.can_use(&mut ai));
        goal.tick(&mut ai);
        assert_eq!(ai.mob.hurt, vec![9]);
        assert_eq!(goal.attack_time(), 20);
        for _ in 0..19 {
            goal.tick(&mut ai);
        }
        assert_eq!(ai.mob.hurt.len(), 1);
        goal.tick(&mut ai);
        assert_eq!(ai.mob.hurt.len(), 2);
        assert!((ai.navigation.speed_modifier_for_test() - 0.6).abs() < 1e-12);

        let mut ai = cat_with_prey_at(3.0);
        assert!(goal.can_use(&mut ai));
        goal.tick(&mut ai);
        assert!(ai.mob.hurt.is_empty());
        assert!((ai.navigation.speed_modifier_for_test() - 1.33).abs() < 1e-12);

        let mut ai = cat_with_prey_at(16.0);
        assert!(goal.can_use(&mut ai));
        assert!(!goal.can_continue_to_use(&mut ai));
    }
}
