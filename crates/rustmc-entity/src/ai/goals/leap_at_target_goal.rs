//! `net.minecraft.world.entity.ai.goal.LeapAtTargetGoal`, from the 26.2 jar.
//! Flags `JUMP, MOVE`. Used by Cat, Ocelot, Fox, Wolf and Spider.
//!
//! # `canUse()`, offsets 0..96
//!
//! ```text
//! if mob.hasControllingPassenger() return false                 (0..11)
//! target = mob.getTarget(); if null return false                (12..31)
//! d = mob.distanceToSqr(target)                                 (32..43)
//! if d < 4.0 || d > 16.0 return false                           (44..61)
//! if !mob.onGround() return false                               (62..73)
//! return mob.getRandom().nextInt(reducedTickDelay(5)) == 0      (74..96)
//! ```
//!
//! `reducedTickDelay(5)` is 3, so the draw is `nextInt(3)` at offset 85
//! and the leap fires on a zero.
//!
//! # `canContinueToUse()`: `!mob.onGround()`.
//!
//! # `start()`, offsets 0..99
//!
//! ```text
//! delta = mob.getDeltaMovement()
//! v = Vec3(target.x - mob.x, 0, target.z - mob.z)
//! if v.lengthSqr() > 1.0E-7: v = v.normalize().scale(0.4) + (delta.scale(0.2))
//! mob.setDeltaMovement(v.x, (double) yd, v.z)
//! ```
//!
//! The target used by `start` is the one `canUse` stored, read again at
//! `start` time for its position.

use rustmc_worldgen::rng::RandomSource;

use crate::ai::goal::{reduced_tick_delay, Flag, FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::host::MobHost;
use crate::phys::Vec3;

/// `LeapAtTargetGoal`.
#[derive(Debug, Clone)]
pub struct LeapAtTargetGoal {
    target: Option<i32>,
    yd: f32,
}

impl LeapAtTargetGoal {
    /// `LeapAtTargetGoal(Mob, float yd)`.
    #[must_use]
    pub fn new(yd: f32) -> Self {
        LeapAtTargetGoal { target: None, yd }
    }

    /// The `target` field.
    #[must_use]
    pub fn target(&self) -> Option<i32> {
        self.target
    }
}

impl<M: MobHost> Goal<MobAi<M>> for LeapAtTargetGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        if cx.mob.has_controlling_passenger() {
            return false;
        }
        self.target = cx.mob.target();
        let Some(t) = self.target else {
            return false;
        };
        let Some(d) = cx.mob.distance_to_sqr_entity(t) else {
            return false;
        };
        if !(4.0..=16.0).contains(&d) {
            return false;
        }
        if !cx.mob.on_ground() {
            return false;
        }
        cx.mob.random().next_i32_bound(reduced_tick_delay(5)) == 0
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        !cx.mob.on_ground()
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        let Some(t) = self.target.and_then(|id| cx.mob.entity(id)) else {
            return;
        };
        let delta = cx.mob.delta_movement();
        let mut v = Vec3::new(t.x - cx.mob.x(), 0.0, t.z - cx.mob.z());
        if v.length_sqr() > 1.0E-7 {
            v = v.normalize().scale(0.4) + (delta.scale(0.2));
        }
        cx.mob.set_delta_movement(v.x, f64::from(self.yd), v.z);
    }

    fn flags(&self) -> FlagSet {
        FlagSet::of2(Flag::Jump, Flag::Move)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    fn ai_with_target_at(dx: f64, seed: i64) -> MobAi<TestMob> {
        let mut g = GridLevel::new();
        g.floor(-20, -20, 20, 20, 63);
        let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, seed);
        mob.on_ground = true;
        mob.others.push((
            EntityClass::Living,
            EntityFacts::living(7, 0.5 + dx, 64.0, 0.5),
        ));
        mob.target_raw = Some(7);
        MobAi::new(mob)
    }

    /// The window is `[4, 16]` on the squared distance: 1.5 blocks is
    /// too close, 3 is in, 5 is too far.
    #[test]
    fn leap_window_is_two_to_four_blocks() {
        let mut goal = LeapAtTargetGoal::new(0.4);
        let mut ai = ai_with_target_at(1.5, 1);
        assert!(!goal.can_use(&mut ai));
        let mut ai = ai_with_target_at(5.0, 1);
        assert!(!goal.can_use(&mut ai));
        let mut ai = ai_with_target_at(3.0, 1);
        // Draw `nextInt(3)` until a zero: the seed decides which call fires.
        let mut fired = false;
        for _ in 0..20 {
            if goal.can_use(&mut ai) {
                fired = true;
                break;
            }
        }
        assert!(fired);
        assert_eq!(goal.target(), Some(7));
    }

    /// `start` sets the horizontal delta to 0.4 toward the target plus a
    /// fifth of the old delta, and `yd` vertically.
    #[test]
    fn start_sets_the_leap_velocity() {
        let mut ai = ai_with_target_at(3.0, 1);
        ai.mob.delta_movement = Vec3::new(0.5, -0.1, 1.0);
        let mut goal = LeapAtTargetGoal::new(0.4);
        goal.target = Some(7);
        goal.start(&mut ai);
        let d = ai.mob.delta_movement;
        assert!((d.x - (0.4 + 0.1)).abs() < 1e-12, "{d:?}");
        assert!((d.y - 0.4f64).abs() < 1e-7);
        assert!((d.z - 0.2).abs() < 1e-12, "{d:?}");
        assert!(!goal.can_continue_to_use(&mut ai));
        ai.mob.on_ground = false;
        assert!(goal.can_continue_to_use(&mut ai));
    }

    /// A ridden mob never leaps.
    #[test]
    fn a_ridden_mob_never_leaps() {
        let mut ai = ai_with_target_at(3.0, 1);
        ai.mob.has_controlling_passenger = true;
        let mut goal = LeapAtTargetGoal::new(0.4);
        assert!(!goal.can_use(&mut ai));
    }
}
