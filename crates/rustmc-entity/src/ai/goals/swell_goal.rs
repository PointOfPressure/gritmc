//! `net.minecraft.world.entity.ai.goal.SwellGoal`, from the 26.2 jar.
//! Flag `MOVE`. Used by Creeper.
//!
//! `canUse()`, offsets 0..49: `t = creeper.getTarget(); creeper.getSwellDir() > 0
//! || t != null && !t.isDeadOrDying() && creeper.distanceToSqr(t) < 9.0`.
//! `start()`: `navigation.stop(); target = creeper.getTarget()`.
//! `stop()`: `target = null`. Every tick.
//!
//! `tick()`, offsets 0..87: `if target == null || target.isDeadOrDying():
//! setSwellDir(-1); else if distanceToSqr(target) > 49.0: setSwellDir(-1);
//! else if !sensing.hasLineOfSight(target): setSwellDir(-1); else setSwellDir(1)`.

use crate::ai::goal::{Flag, FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::host::MobHost;

/// `SwellGoal`.
#[derive(Debug, Clone, Default)]
pub struct SwellGoal {
    target: Option<i32>,
}

impl SwellGoal {
    /// `SwellGoal(Creeper)`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl<M: MobHost> Goal<MobAi<M>> for SwellGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        let t = cx.mob.target();
        if cx.mob.swell_dir() > 0 {
            return true;
        }
        let Some(t) = t else {
            return false;
        };
        if cx.mob.entity_dead_or_dying(t) {
            return false;
        }
        cx.mob.distance_to_sqr_entity(t).is_some_and(|d| d < 9.0)
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        cx.navigation.stop();
        self.target = cx.mob.target();
    }

    fn stop(&mut self, _cx: &mut MobAi<M>) {
        self.target = None;
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn tick(&mut self, cx: &mut MobAi<M>) {
        let Some(t) = self.target else {
            cx.mob.set_swell_dir(-1);
            return;
        };
        if cx.mob.entity_dead_or_dying(t) {
            cx.mob.set_swell_dir(-1);
            return;
        }
        if cx.mob.distance_to_sqr_entity(t).is_none_or(|d| d > 49.0) {
            cx.mob.set_swell_dir(-1);
            return;
        }
        let los = {
            let MobAi { mob, sensing, .. } = cx;
            sensing.has_line_of_sight(t, || mob.has_line_of_sight_to(t))
        };
        if !los {
            cx.mob.set_swell_dir(-1);
            return;
        }
        cx.mob.set_swell_dir(1);
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
    fn swells_within_three_and_keeps_swelling_to_seven_with_sight() {
        let mut mob = TestMob::new(GridLevel::new(), 0.5, 64.0, 0.5, 1);
        mob.others
            .push((EntityClass::Player, EntityFacts::player(9, 2.5, 64.0, 0.5)));
        mob.target_raw = Some(9);
        mob.visible.push(9);
        let mut ai = MobAi::new(mob);
        let mut goal = SwellGoal::new();
        assert!(goal.can_use(&mut ai));
        goal.start(&mut ai);
        goal.tick(&mut ai);
        assert_eq!(ai.mob.swell_dir, 1);
        ai.mob.others[0].1.x = 6.5;
        assert!(
            goal.can_use(&mut ai),
            "already swelling: swellDir > 0 keeps it"
        );
        goal.tick(&mut ai);
        assert_eq!(ai.mob.swell_dir, 1, "6 blocks is within 7");
        ai.mob.others[0].1.x = 8.5;
        goal.tick(&mut ai);
        assert_eq!(ai.mob.swell_dir, -1);
        assert!(!goal.can_use(&mut ai), "far and no longer swelling");
        ai.mob.others[0].1.x = 2.5;
        ai.mob.visible.clear();
        ai.sensing.tick();
        goal.tick(&mut ai);
        assert_eq!(ai.mob.swell_dir, -1, "no line of sight");
    }
}
