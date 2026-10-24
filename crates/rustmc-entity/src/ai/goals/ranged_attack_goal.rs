//! `net.minecraft.world.entity.ai.goal.RangedAttackGoal`, from the 26.2
//! jar. Flags `MOVE, LOOK`. Used by Llama, SnowGolem, WitherBoss, Witch.
//!
//! Constructor `(RangedAttackMob, double speed, int minInterval, int
//! maxInterval, float radius)`; the four-argument form passes the interval
//! twice. `attackTime` starts at `-1`; `attackRadiusSqr = radius * radius`
//! in float.
//!
//! `canUse()`: `target = mob.getTarget(); target != null && target.isAlive()`,
//! storing it. `canContinueToUse()`: `canUse() || target.isAlive() &&
//! !navigation.isDone()`. `stop()`: `target = null; seeTime = 0; attackTime = -1`.
//! `requiresUpdateEveryTick()` is true.
//!
//! # `tick()`, offsets 0..256
//!
//! ```text
//! d = mob.distanceToSqr(target.x, target.y, target.z)
//! los = sensing.hasLineOfSight(target)
//! if los: ++seeTime else seeTime = 0                                 (44..66)
//! if d <= attackRadiusSqr && seeTime >= 5: navigation.stop()          (66..116)
//! else navigation.moveTo(target, speedModifier)
//! lookControl.setLookAt(target, 30.0f, 30.0f)                          (117..131)
//! if --attackTime == 0:                                                (135..145)
//!     if !los return
//!     f = (float) sqrt(d) / attackRadius
//!     g = Mth.clamp(f, 0.1f, 1.0f)
//!     rangedAttackMob.performRangedAttack(target, g)
//!     attackTime = Mth.floor(f * (max - min) + min)                    (191..213)
//! else if attackTime < 0:                                              (220..223)
//!     attackTime = Mth.floor(Mth.lerp(sqrt(d) / attackRadius, (double) min, (double) max))
//! ```

use crate::ai::goal::{Flag, FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::host::MobHost;
use rustmc_worldgen::math::clamp_f32;

use crate::mth::{floor_f32, lerp};

/// `RangedAttackGoal`.
#[derive(Debug, Clone)]
pub struct RangedAttackGoal {
    target: Option<i32>,
    attack_time: i32,
    speed_modifier: f64,
    see_time: i32,
    attack_interval_min: i32,
    attack_interval_max: i32,
    attack_radius: f32,
    attack_radius_sqr: f32,
}

impl RangedAttackGoal {
    /// `RangedAttackGoal(RangedAttackMob, double, int, float)`.
    #[must_use]
    pub fn new(speed_modifier: f64, attack_interval: i32, attack_radius: f32) -> Self {
        Self::with_intervals(
            speed_modifier,
            attack_interval,
            attack_interval,
            attack_radius,
        )
    }

    /// `RangedAttackGoal(RangedAttackMob, double, int, int, float)`.
    #[must_use]
    pub fn with_intervals(
        speed_modifier: f64,
        attack_interval_min: i32,
        attack_interval_max: i32,
        attack_radius: f32,
    ) -> Self {
        RangedAttackGoal {
            target: None,
            attack_time: -1,
            speed_modifier,
            see_time: 0,
            attack_interval_min,
            attack_interval_max,
            attack_radius,
            attack_radius_sqr: attack_radius * attack_radius,
        }
    }

    /// `attackTime`, for tests.
    #[must_use]
    pub fn attack_time(&self) -> i32 {
        self.attack_time
    }

    fn target_alive<M: MobHost>(&self, cx: &MobAi<M>) -> bool {
        self.target
            .and_then(|t| cx.mob.entity(t))
            .is_some_and(|f| f.alive)
    }
}

impl<M: MobHost> Goal<MobAi<M>> for RangedAttackGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        let t = cx.mob.target();
        let alive = t.and_then(|id| cx.mob.entity(id)).is_some_and(|f| f.alive);
        if !alive {
            return false;
        }
        self.target = t;
        true
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        self.can_use(cx) || (self.target_alive(cx) && !cx.navigation.is_done())
    }

    fn stop(&mut self, _cx: &mut MobAi<M>) {
        self.target = None;
        self.see_time = 0;
        self.attack_time = -1;
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    #[allow(clippy::cast_possible_truncation)]
    fn tick(&mut self, cx: &mut MobAi<M>) {
        let Some(t) = self.target else {
            return;
        };
        let Some(facts) = cx.mob.entity(t) else {
            return;
        };
        let d = facts.distance_to_sqr(cx.mob.x(), cx.mob.y(), cx.mob.z());
        let los = {
            let MobAi { mob, sensing, .. } = cx;
            sensing.has_line_of_sight(t, || mob.has_line_of_sight_to(t))
        };
        if los {
            self.see_time += 1;
        } else {
            self.see_time = 0;
        }
        if d <= f64::from(self.attack_radius_sqr) && self.see_time >= 5 {
            cx.navigation.stop();
        } else {
            let s = self.speed_modifier;
            cx.navigation
                .move_to_entity_at(&mut cx.mob, facts.block_position(), s);
        }
        cx.look_control
            .set_look_at_with(facts.x, facts.eye_y, facts.z, 30.0, 30.0);
        self.attack_time -= 1;
        if self.attack_time == 0 {
            if !los {
                return;
            }
            let f = d.sqrt() as f32 / self.attack_radius;
            let g = clamp_f32(f, 0.1, 1.0);
            cx.mob.perform_ranged_attack(t, g);
            #[allow(clippy::cast_precision_loss)]
            let next = f * (self.attack_interval_max - self.attack_interval_min) as f32
                + self.attack_interval_min as f32;
            self.attack_time = floor_f32(next);
        } else if self.attack_time < 0 {
            self.attack_time = rustmc_worldgen::math::floor(lerp(
                d.sqrt() / f64::from(self.attack_radius),
                f64::from(self.attack_interval_min),
                f64::from(self.attack_interval_max),
            ));
        }
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

    fn shooter_with_target_at(dx: f64) -> MobAi<TestMob> {
        let mut g = GridLevel::new();
        g.floor(-40, -40, 40, 40, 63);
        let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, 3);
        mob.on_ground = true;
        mob.others.push((
            EntityClass::Player,
            EntityFacts::player(9, 0.5 + dx, 64.0, 0.5),
        ));
        mob.target_raw = Some(9);
        mob.visible.push(9);
        MobAi::new(mob)
    }

    /// A witch (`20, 60, 10.0f`) at 5 blocks: the first tick sets
    /// `attackTime = floor(lerp(0.5, 20, 60)) = 40`, walks toward the
    /// target (seeTime not yet 5), and fires on the tick that counts it
    /// down to zero with power `clamp(0.5, 0.1, 1) = 0.5`, resetting to
    /// `floor(0.5 * 40 + 20) = 40`.
    #[test]
    fn witch_timing_from_five_blocks() {
        let mut goal = RangedAttackGoal::with_intervals(1.0, 20, 60, 10.0);
        let mut ai = shooter_with_target_at(5.0);
        assert!(goal.can_use(&mut ai));
        goal.tick(&mut ai);
        assert_eq!(goal.attack_time(), 40);
        assert!(ai.navigation.is_in_progress(), "closing in");
        for _ in 0..4 {
            goal.tick(&mut ai);
        }
        assert!(ai.navigation.is_done(), "seeTime reached 5: stands still");
        for _ in 0..35 {
            goal.tick(&mut ai);
        }
        assert_eq!(goal.attack_time(), 1);
        assert!(!ai
            .mob
            .calls
            .iter()
            .any(|c| c.starts_with("performRangedAttack")));
        goal.tick(&mut ai);
        assert!(ai
            .mob
            .calls
            .iter()
            .any(|c| c == "performRangedAttack(9, 0.5)"));
        assert_eq!(goal.attack_time(), 40);
        goal.stop(&mut ai);
        assert_eq!(goal.attack_time(), -1);
    }

    /// Out of sight on the firing tick: no shot, and `attackTime` stays 0
    /// so the next tick with sight takes the `< 0` branch? No — it stays
    /// at 0 and `--attackTime` makes it -1 next tick, which re-lerps.
    /// Pinned because a plausible rewrite refires immediately.
    #[test]
    fn a_blocked_shot_relerps_the_interval_next_tick() {
        let mut goal = RangedAttackGoal::new(1.0, 10, 4.0);
        let mut ai = shooter_with_target_at(2.0);
        assert!(goal.can_use(&mut ai));
        goal.tick(&mut ai);
        assert_eq!(goal.attack_time(), 10);
        for _ in 0..9 {
            goal.tick(&mut ai);
        }
        assert_eq!(goal.attack_time(), 1);
        ai.mob.visible.clear();
        ai.sensing.tick();
        goal.tick(&mut ai);
        assert_eq!(goal.attack_time(), 0);
        assert!(!ai
            .mob
            .calls
            .iter()
            .any(|c| c.starts_with("performRangedAttack")));
        ai.sensing.tick();
        goal.tick(&mut ai);
        assert_eq!(goal.attack_time(), 10);
    }
}
