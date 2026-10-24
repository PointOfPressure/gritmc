//! `net.minecraft.world.entity.ai.goal.MeleeAttackGoal`, from the 26.2 jar.
//! Flags `MOVE, LOOK`; `requiresUpdateEveryTick` true.
//!
//! # `canUse()`, offsets 0..93
//!
//! ```text
//! now = level.getGameTime()
//! if now - lastCanUseCheck < 20 return false          (COOLDOWN_BETWEEN_CAN_USE_CHECKS)
//! lastCanUseCheck = now
//! t = mob.getTarget(); if t == null || !t.isAlive() return false
//! path = navigation.createPath(t, 0)
//! if path != null return true
//! return mob.isWithinMeleeAttackRange(t)
//! ```
//!
//! # `tick()`, offsets 0..289
//!
//! ```text
//! t = mob.getTarget(); if t == null return
//! lookControl.setLookAt(t, 30, 30)
//! ticksUntilNextPathRecalculation = max(ticks - 1, 0)
//! if (followingTargetEvenIfNotSeen || sensing.hasLineOfSight(t))
//!    && ticksUntilNextPathRecalculation <= 0
//!    && (pathedTarget == (0,0,0) || t.distanceToSqr(pathedTarget) >= 1 || nextFloat() < 0.05f):
//!     pathedTarget = t.position
//!     ticks = 4 + nextInt(7)                                        (offset 171)
//!     d = mob.distanceToSqr(t)
//!     if d > 1024 ticks += 10 else if d > 256 ticks += 5
//!     if !navigation.moveTo(t, speed) ticks += 15
//!     ticks = adjustedTickDelay(ticks)
//! ticksUntilNextAttack = max(ticksUntilNextAttack - 1, 0)
//! checkAndPerformAttack(t)
//! ```
//!
//! The `nextFloat() < 0.05f` at offset 125 is drawn only when the target
//! is within one block of where it was last pathed to; the `nextInt(7)`
//! only when a path is recomputed.

use rustmc_worldgen::rng::RandomSource;

use crate::ai::goal::{adjusted_tick_delay, Flag, FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::host::{MobHost, PathLevel};
use crate::pathfinder::Path;

/// `MeleeAttackGoal`.
pub struct MeleeAttackGoal {
    speed_modifier: f64,
    following_target_even_if_not_seen: bool,
    path: Option<Path>,
    pathed_target_x: f64,
    pathed_target_y: f64,
    pathed_target_z: f64,
    ticks_until_next_path_recalculation: i32,
    ticks_until_next_attack: i32,
    last_can_use_check: i64,
}

/// `COOLDOWN_BETWEEN_CAN_USE_CHECKS`.
pub const COOLDOWN_BETWEEN_CAN_USE_CHECKS: i64 = 20;
/// `attackInterval`.
pub const ATTACK_INTERVAL: i32 = 20;

impl MeleeAttackGoal {
    /// `MeleeAttackGoal(PathfinderMob, double, boolean)`.
    #[must_use]
    pub fn new(speed_modifier: f64, following_target_even_if_not_seen: bool) -> Self {
        MeleeAttackGoal {
            speed_modifier,
            following_target_even_if_not_seen,
            path: None,
            pathed_target_x: 0.0,
            pathed_target_y: 0.0,
            pathed_target_z: 0.0,
            ticks_until_next_path_recalculation: 0,
            ticks_until_next_attack: 0,
            last_can_use_check: 0,
        }
    }

    /// `checkAndPerformAttack(LivingEntity)`.
    fn check_and_perform_attack<M: MobHost>(&mut self, cx: &mut MobAi<M>, target: i32) {
        if self.can_perform_attack(cx, target) {
            self.reset_attack_cooldown();
            cx.mob.swing_main_hand();
            cx.mob.do_hurt_target(target);
        }
    }

    /// `resetAttackCooldown()`: `adjustedTickDelay(20)`.
    fn reset_attack_cooldown(&mut self) {
        self.ticks_until_next_attack = adjusted_tick_delay(true, ATTACK_INTERVAL);
    }

    /// `isTimeToAttack()`.
    fn is_time_to_attack(&self) -> bool {
        self.ticks_until_next_attack <= 0
    }

    /// `canPerformAttack(LivingEntity)`: time, reach, sight — in that order.
    fn can_perform_attack<M: MobHost>(&self, cx: &mut MobAi<M>, target: i32) -> bool {
        if !self.is_time_to_attack() {
            return false;
        }
        if !cx.mob.is_within_melee_attack_range(target) {
            return false;
        }
        let MobAi { mob, sensing, .. } = cx;
        sensing.has_line_of_sight(target, || mob.has_line_of_sight_to(target))
    }

    /// `getTicksUntilNextAttack()`.
    #[must_use]
    pub fn ticks_until_next_attack(&self) -> i32 {
        self.ticks_until_next_attack
    }

    /// `getAttackInterval()`: `adjustedTickDelay(20)`.
    #[must_use]
    pub fn attack_interval(&self) -> i32 {
        adjusted_tick_delay(true, ATTACK_INTERVAL)
    }
}

impl<M: MobHost> Goal<MobAi<M>> for MeleeAttackGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        let now = cx.mob.level().game_time();
        if now - self.last_can_use_check < COOLDOWN_BETWEEN_CAN_USE_CHECKS {
            return false;
        }
        self.last_can_use_check = now;
        let Some(t) = cx.mob.target() else {
            return false;
        };
        let Some(facts) = cx.mob.entity(t) else {
            return false;
        };
        if !facts.alive {
            return false;
        }
        self.path = cx
            .navigation
            .create_path_to_entity_at(&mut cx.mob, facts.block_position(), 0);
        if self.path.is_some() {
            return true;
        }
        cx.mob.is_within_melee_attack_range(t)
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        let Some(t) = cx.mob.target() else {
            return false;
        };
        let Some(facts) = cx.mob.entity(t) else {
            return false;
        };
        if !facts.alive {
            return false;
        }
        if !self.following_target_even_if_not_seen {
            return !cx.navigation.is_done();
        }
        if !cx.mob.is_within_home_pos(facts.block_position()) {
            return false;
        }
        if facts.is_player && (facts.is_spectator || facts.is_creative) {
            return false;
        }
        true
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        let path = self.path.clone();
        cx.navigation
            .move_along(&mut cx.mob, path, self.speed_modifier);
        cx.mob.set_aggressive(true);
        self.ticks_until_next_path_recalculation = 0;
        self.ticks_until_next_attack = 0;
    }

    fn stop(&mut self, cx: &mut MobAi<M>) {
        let t = cx.mob.target();
        let keep = t
            .and_then(|id| cx.mob.entity(id))
            .is_some_and(|f| f.no_creative_or_spectator());
        if !keep {
            cx.mob.set_target(None);
        }
        cx.mob.set_aggressive(false);
        cx.navigation.stop();
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn tick(&mut self, cx: &mut MobAi<M>) {
        let Some(t) = cx.mob.target() else {
            return;
        };
        let Some(facts) = cx.mob.entity(t) else {
            return;
        };
        cx.look_control
            .set_look_at_with(facts.x, facts.eye_y, facts.z, 30.0, 30.0);
        self.ticks_until_next_path_recalculation =
            (self.ticks_until_next_path_recalculation - 1).max(0);
        let sees = if self.following_target_even_if_not_seen {
            true
        } else {
            let MobAi { mob, sensing, .. } = cx;
            sensing.has_line_of_sight(t, || mob.has_line_of_sight_to(t))
        };
        if sees && self.ticks_until_next_path_recalculation <= 0 {
            let never_pathed = self.pathed_target_x == 0.0
                && self.pathed_target_y == 0.0
                && self.pathed_target_z == 0.0;
            let moved = facts.distance_to_sqr(
                self.pathed_target_x,
                self.pathed_target_y,
                self.pathed_target_z,
            ) >= 1.0;
            let recompute = never_pathed || moved || cx.mob.random().next_f32() < 0.05;
            if recompute {
                self.pathed_target_x = facts.x;
                self.pathed_target_y = facts.y;
                self.pathed_target_z = facts.z;
                self.ticks_until_next_path_recalculation = 4 + cx.mob.random().next_i32_bound(7);
                let d = cx
                    .mob
                    .position()
                    .distance_to_sqr3(facts.x, facts.y, facts.z);
                if d > 1024.0 {
                    self.ticks_until_next_path_recalculation += 10;
                } else if d > 256.0 {
                    self.ticks_until_next_path_recalculation += 5;
                }
                if !cx.navigation.move_to_entity_at(
                    &mut cx.mob,
                    facts.block_position(),
                    self.speed_modifier,
                ) {
                    self.ticks_until_next_path_recalculation += 15;
                }
                self.ticks_until_next_path_recalculation =
                    adjusted_tick_delay(true, self.ticks_until_next_path_recalculation);
            }
        }
        self.ticks_until_next_attack = (self.ticks_until_next_attack - 1).max(0);
        self.check_and_perform_attack(cx, t);
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

    fn ai() -> MobAi<TestMob> {
        let mut g = GridLevel::new();
        g.floor(-20, -20, 20, 20, 63);
        let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, 1);
        mob.others = vec![(EntityClass::Player, EntityFacts::player(11, 4.5, 64.0, 0.5))];
        mob.visible = vec![11];
        mob.target_raw = Some(11);
        MobAi::new(mob)
    }

    /// `canUse` is gated 20 game-ticks apart, paths to the target, and
    /// `start` moves along that path and flags the mob aggressive; the
    /// first tick recomputes (never pathed) and, in reach with sight,
    /// attacks and sets the 20-tick cooldown.
    #[test]
    fn can_use_paths_and_tick_attacks_in_reach() {
        let mut ai = ai();
        let mut g = MeleeAttackGoal::new(1.2, false);
        ai.mob.level.game_time = 0;
        assert!(!g.can_use(&mut ai), "0 - 0 < 20");
        ai.mob.level.game_time = 20;
        assert!(g.can_use(&mut ai));
        g.start(&mut ai);
        assert!(ai.mob.aggressive);
        assert!(!ai.navigation.is_done());
        ai.mob.melee_reach = vec![11];
        g.tick(&mut ai);
        assert_eq!(ai.mob.hurt, vec![11]);
        assert_eq!(g.ticks_until_next_attack(), 20);
        assert!(ai.mob.calls.iter().any(|c| c == "swing"));
        // Not again for twenty ticks.
        for _ in 0..19 {
            g.tick(&mut ai);
        }
        assert_eq!(ai.mob.hurt.len(), 1);
        g.tick(&mut ai);
        assert_eq!(ai.mob.hurt.len(), 2);
        g.stop(&mut ai);
        assert!(!ai.mob.aggressive);
        assert_eq!(
            ai.mob.target(),
            Some(11),
            "a survival player stays the target"
        );
        assert!(ai.navigation.is_done());
    }

    /// Without `followingTargetEvenIfNotSeen`, losing the path ends the goal.
    #[test]
    fn continue_depends_on_the_path_unless_following_blind() {
        let mut ai = ai();
        let mut g = MeleeAttackGoal::new(1.2, false);
        ai.mob.level.game_time = 20;
        assert!(g.can_use(&mut ai));
        g.start(&mut ai);
        assert!(g.can_continue_to_use(&mut ai));
        ai.navigation.stop();
        assert!(!g.can_continue_to_use(&mut ai));
        let mut blind = MeleeAttackGoal::new(1.2, true);
        assert!(blind.can_continue_to_use(&mut ai));
    }
}
