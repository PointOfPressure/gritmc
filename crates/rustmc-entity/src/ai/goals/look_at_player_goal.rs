//! `net.minecraft.world.entity.ai.goal.LookAtPlayerGoal`, from the 26.2
//! jar. Flag `LOOK`.
//!
//! # `canUse()`, offsets 0..191
//!
//! ```text
//! if nextFloat() >= probability return false                (4..17: fcmpl iflt)
//! if mob.getTarget() != null: lookAt = mob.getTarget()
//! if lookAtType == Player.class:
//!     lookAt = level.getNearestPlayer(lookAtContext, mob, mob.x, mob.eyeY, mob.z)
//! else:
//!     lookAt = level.getNearestEntity(getEntitiesOfClass(type, bb.inflate(d, 3, d)), lookAtContext, mob, x, eyeY, z)
//! return lookAt != null
//! ```
//!
//! Note the target is assigned and then **overwritten** by the search:
//! the `mob.getTarget()` line has no effect on the answer in either
//! branch. It is transcribed anyway.
//!
//! The conditions are `forNonCombat().range(lookDistance)`, with the
//! selector `EntitySelector.notRiding(mob)` for the player class only —
//! a player riding this mob is not looked at.
//!
//! `start`: `lookTime = adjustedTickDelay(40 + nextInt(40))`.
//! `canContinueToUse`: alive, within `lookDistance²`, `lookTime > 0`.
//! `tick`: look at `(x, onlyHorizontal ? mob.eyeY : target.eyeY, z)`,
//! `lookTime--`.

use rustmc_worldgen::rng::RandomSource;

use crate::ai::goal::{adjusted_tick_delay, Flag, FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::ai::targeting::{nearest_entity, nearest_player, TargetingConditions};
use crate::host::{EntityClass, MobHost};

/// `DEFAULT_PROBABILITY`.
pub const DEFAULT_PROBABILITY: f32 = 0.02;

/// `LookAtPlayerGoal`.
pub struct LookAtPlayerGoal<M: MobHost> {
    look_at: Option<i32>,
    look_distance: f32,
    look_time: i32,
    probability: f32,
    only_horizontal: bool,
    look_at_type: EntityClass,
    look_at_context: TargetingConditions<M>,
}

impl<M: MobHost> LookAtPlayerGoal<M> {
    /// `LookAtPlayerGoal(Mob, Class, float lookDistance)`: probability 0.02.
    #[must_use]
    pub fn new(cx: &MobAi<M>, look_at_type: EntityClass, look_distance: f32) -> Self {
        Self::with(cx, look_at_type, look_distance, DEFAULT_PROBABILITY, false)
    }

    /// `LookAtPlayerGoal(Mob, Class, float, float probability, boolean onlyHorizontal)`.
    #[must_use]
    pub fn with(
        cx: &MobAi<M>,
        look_at_type: EntityClass,
        look_distance: f32,
        probability: f32,
        only_horizontal: bool,
    ) -> Self {
        let mob_id = cx.mob.id();
        let mut ctx = TargetingConditions::for_non_combat().range(f64::from(look_distance));
        if look_at_type == EntityClass::Player {
            // EntitySelector.notRiding(mob): the candidate's vehicle chain
            // does not include this mob. The facts carry one vehicle; a
            // deeper chain is the host's to resolve.
            ctx = ctx.selector(Box::new(move |_, f| f.vehicle != Some(mob_id)));
        }
        LookAtPlayerGoal {
            look_at: None,
            look_distance,
            look_time: 0,
            probability,
            only_horizontal,
            look_at_type,
            look_at_context: ctx,
        }
    }

    /// The entity being looked at, for tests.
    #[must_use]
    pub fn looking_at(&self) -> Option<i32> {
        self.look_at
    }

    /// The `lookAt` field, assigned by subclasses' `canUse`.
    pub(crate) fn set_look_at(&mut self, id: Option<i32>) {
        self.look_at = id;
    }
}

impl<M: MobHost> Goal<MobAi<M>> for LookAtPlayerGoal<M> {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        if cx.mob.random().next_f32() >= self.probability {
            return false;
        }
        if let Some(t) = cx.mob.target() {
            self.look_at = Some(t);
        }
        let (x, y, z) = (cx.mob.x(), cx.mob.eye_y(), cx.mob.z());
        self.look_at = if self.look_at_type == EntityClass::Player {
            nearest_player(cx, &self.look_at_context, x, y, z)
        } else {
            let d = f64::from(self.look_distance);
            let area = cx.mob.bounding_box().inflate(d, 3.0, d);
            let ids = cx.mob.entities_of_class_in(self.look_at_type, &area);
            nearest_entity(cx, &ids, &self.look_at_context, x, y, z)
        };
        self.look_at.is_some()
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        let Some(id) = self.look_at else {
            return false;
        };
        let Some(f) = cx.mob.entity(id) else {
            return false;
        };
        if !f.alive {
            return false;
        }
        if cx.mob.position().distance_to_sqr3(f.x, f.y, f.z)
            > f64::from(self.look_distance * self.look_distance)
        {
            return false;
        }
        self.look_time > 0
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        self.look_time = adjusted_tick_delay(false, 40 + cx.mob.random().next_i32_bound(40));
    }

    fn stop(&mut self, _cx: &mut MobAi<M>) {
        self.look_at = None;
    }

    fn tick(&mut self, cx: &mut MobAi<M>) {
        let Some(id) = self.look_at else {
            return;
        };
        let Some(f) = cx.mob.entity(id) else {
            return;
        };
        if !f.alive {
            return;
        }
        let y = if self.only_horizontal {
            cx.mob.eye_y()
        } else {
            f.eye_y
        };
        cx.look_control.set_look_at(&cx.mob, f.x, y, f.z);
        self.look_time -= 1;
    }

    fn flags(&self) -> FlagSet {
        FlagSet::of(Flag::Look)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::EntityFacts;
    use crate::testing::{GridLevel, TestMob};
    use rustmc_worldgen::rng::LegacyRandomSource;

    /// One float per ask; on a pass the nearest visible player is chosen
    /// and `start` draws `nextInt(40)` for a halved look time.
    #[test]
    fn draws_one_float_then_picks_the_nearest_player() {
        let mut mob = TestMob::new(GridLevel::new(), 0.5, 64.0, 0.5, 8);
        mob.others = vec![
            (EntityClass::Player, EntityFacts::player(11, 5.5, 64.0, 0.5)),
            (EntityClass::Player, EntityFacts::player(12, 0.5, 64.0, 2.5)),
        ];
        mob.visible = vec![11, 12];
        let mut ai = MobAi::new(mob);
        let mut g = LookAtPlayerGoal::new(&ai, EntityClass::Player, 8.0);
        let mut r = LegacyRandomSource::new(8);
        let mut passed = false;
        for _ in 0..500 {
            let f = r.next_f32();
            let got = g.can_use(&mut ai);
            assert_eq!(got, f < 0.02);
            if got {
                passed = true;
                break;
            }
        }
        assert!(passed);
        assert_eq!(g.looking_at(), Some(12));
        g.start(&mut ai);
        let want = adjusted_tick_delay(false, 40 + r.next_i32_bound(40));
        assert_eq!(g.look_time, want);
        assert!(g.can_continue_to_use(&mut ai));
        g.tick(&mut ai);
        assert!(ai.look_control.is_looking_at_target());
        assert_eq!(ai.look_control.wanted(), (0.5, 64.0 + 1.62, 2.5));
    }

    /// A player riding this mob is skipped by `notRiding`.
    #[test]
    fn a_rider_is_not_looked_at() {
        let mut mob = TestMob::new(GridLevel::new(), 0.5, 64.0, 0.5, 8);
        let mut rider = EntityFacts::player(11, 0.5, 65.0, 0.5);
        rider.vehicle = Some(1);
        mob.others = vec![(EntityClass::Player, rider)];
        mob.visible = vec![11];
        let mut ai = MobAi::new(mob);
        let mut g = LookAtPlayerGoal::with(&ai, EntityClass::Player, 8.0, 1.0, false);
        assert!(!g.can_use(&mut ai));
        ai.mob.others[0].1.vehicle = None;
        assert!(g.can_use(&mut ai));
    }
}
