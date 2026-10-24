//! `net.minecraft.world.entity.ai.goal.TemptGoal`, from the 26.2 jar.
//! Flags `MOVE, LOOK`.
//!
//! `TEMPT_TARGETING = forNonCombat().ignoreLineOfSight()`, copied per
//! goal with the selector `shouldFollow(entity)` — the item predicate on
//! the entity's main or off hand.
//!
//! # `canUse()`, offsets 0..66
//!
//! ```text
//! if calmDown > 0: calmDown--; return false
//! player = level.getNearestPlayer(targetingConditions.range(TEMPT_RANGE), mob)
//! return player != null
//! ```
//!
//! # `canContinueToUse()`, offsets 0..162
//!
//! ```text
//! if canScare():
//!     if mob.distanceToSqr(player) < 36:
//!         if player.distanceToSqr(px, py, pz) > 0.010000000000000002 return false
//!         if |player.xRot - pRotX| > 5 || |player.yRot - pRotY| > 5 return false
//!     else: px, py, pz = player position
//!     pRotX, pRotY = player rotation
//! return canUse()
//! ```
//!
//! So a scared mob within six blocks stops following a player who moves
//! more than a tenth of a block or turns more than five degrees; the
//! remembered position is only refreshed while the player is farther.
//! `canUse()` at the end re-searches the nearest player every tick, which
//! may swap `player`.
//!
//! `start` remembers the player's position; `stop` clears the player,
//! stops the navigation and sets `calmDown = reducedTickDelay(100)`;
//! `tick` looks at the player with `maxHeadYRot + 20` yaw and
//! `maxHeadXRot` pitch, then stops within `stopDistance²` or paths to
//! the player.

use crate::ai::goal::{reduced_tick_delay, Flag, FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::ai::targeting::{nearest_player_to_mob, TargetingConditions};
use crate::host::{EntityFacts, MobHost};
use crate::phys::Vec3;
use rustmc_worldgen::rng::RandomSource;

/// `DEFAULT_STOP_DISTANCE`.
pub const DEFAULT_STOP_DISTANCE: f64 = 2.5;

/// `TemptGoal`.
pub struct TemptGoal<M: MobHost> {
    targeting_conditions: TargetingConditions<M>,
    speed_modifier: f64,
    px: f64,
    py: f64,
    pz: f64,
    p_rot_x: f64,
    p_rot_y: f64,
    player: Option<i32>,
    calm_down: i32,
    is_running: bool,
    can_scare: bool,
    stop_distance: f64,
    /// `TemptGoal.ForNonPathfinders`: the move control instead of the
    /// navigation.
    for_non_pathfinders: bool,
}

impl<M: MobHost> TemptGoal<M> {
    /// `TemptGoal(PathfinderMob, double, Predicate, boolean canScare)`:
    /// stop distance 2.5. The item predicate is the host's
    /// `holds_tempting_item`.
    #[must_use]
    pub fn new(speed_modifier: f64, can_scare: bool) -> Self {
        Self::with_stop_distance(speed_modifier, can_scare, DEFAULT_STOP_DISTANCE)
    }

    /// `TemptGoal(PathfinderMob, double, Predicate, boolean, double stopDistance)`.
    #[must_use]
    pub fn with_stop_distance(speed_modifier: f64, can_scare: bool, stop_distance: f64) -> Self {
        let conditions = TargetingConditions::for_non_combat()
            .ignore_line_of_sight()
            .selector(Box::new(|ai: &MobAi<M>, f| {
                ai.mob.holds_tempting_item(f.id)
            }));
        TemptGoal {
            targeting_conditions: conditions,
            speed_modifier,
            px: 0.0,
            py: 0.0,
            pz: 0.0,
            p_rot_x: 0.0,
            p_rot_y: 0.0,
            player: None,
            calm_down: 0,
            is_running: false,
            can_scare,
            stop_distance,
            for_non_pathfinders: false,
        }
    }

    /// `TemptGoal.ForNonPathfinders(Mob, double, Predicate, boolean, double)`:
    /// `stopNavigation()` is `moveControl.setWait()` and
    /// `navigateTowards(player)` is
    /// `v = (player.getEyePosition() - mob.position()).scale(random.nextDouble()) + mob.position();
    /// moveControl.setWantedPosition(v.x, v.y, v.z, speedModifier)` —
    /// one `nextDouble` per navigating tick, at offset 21.
    #[must_use]
    pub fn for_non_pathfinders(speed_modifier: f64, can_scare: bool, stop_distance: f64) -> Self {
        let mut g = Self::with_stop_distance(speed_modifier, can_scare, stop_distance);
        g.for_non_pathfinders = true;
        g
    }

    /// `canScare()`.
    fn can_scare(&self) -> bool {
        self.can_scare
    }

    /// `stopNavigation()`.
    fn stop_navigation(&self, cx: &mut MobAi<M>) {
        if self.for_non_pathfinders {
            cx.move_control.set_wait();
        } else {
            cx.navigation.stop();
        }
    }

    /// `navigateTowards(Player)`.
    fn navigate_towards(&self, cx: &mut MobAi<M>, player: &EntityFacts) {
        if self.for_non_pathfinders {
            let eye = Vec3::new(player.x, player.eye_y, player.z);
            let me = cx.mob.position();
            let t = cx.mob.random().next_f64();
            let v = eye.subtract(me).scale(t) + (me);
            cx.move_control
                .set_wanted_position(v.x, v.y, v.z, self.speed_modifier);
        } else {
            cx.navigation.move_to_entity_at(
                &mut cx.mob,
                player.block_position(),
                self.speed_modifier,
            );
        }
    }

    /// `isRunning()`.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.is_running
    }

    /// The tempting player, for tests.
    #[must_use]
    pub fn player(&self) -> Option<i32> {
        self.player
    }
}

impl<M: MobHost> Goal<MobAi<M>> for TemptGoal<M> {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        if self.calm_down > 0 {
            self.calm_down -= 1;
            return false;
        }
        self.targeting_conditions.set_range(cx.mob.tempt_range());
        self.player = nearest_player_to_mob(cx, &self.targeting_conditions);
        self.player.is_some()
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        if self.can_scare() {
            let Some(p) = self.player.and_then(|id| cx.mob.entity(id)) else {
                return false;
            };
            if cx.mob.position().distance_to_sqr3(p.x, p.y, p.z) < 36.0 {
                if p.distance_to_sqr(self.px, self.py, self.pz) > 0.010_000_000_000_000_002 {
                    return false;
                }
                if (f64::from(p.x_rot) - self.p_rot_x).abs() > 5.0
                    || (f64::from(p.y_rot) - self.p_rot_y).abs() > 5.0
                {
                    return false;
                }
            } else {
                self.px = p.x;
                self.py = p.y;
                self.pz = p.z;
            }
            self.p_rot_x = f64::from(p.x_rot);
            self.p_rot_y = f64::from(p.y_rot);
        }
        self.can_use(cx)
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        if let Some(p) = self.player.and_then(|id| cx.mob.entity(id)) {
            self.px = p.x;
            self.py = p.y;
            self.pz = p.z;
        }
        self.is_running = true;
    }

    fn stop(&mut self, cx: &mut MobAi<M>) {
        self.player = None;
        self.stop_navigation(cx);
        self.calm_down = reduced_tick_delay(100);
        self.is_running = false;
    }

    fn tick(&mut self, cx: &mut MobAi<M>) {
        let Some(p) = self.player.and_then(|id| cx.mob.entity(id)) else {
            return;
        };
        #[allow(clippy::cast_precision_loss)]
        cx.look_control.set_look_at_with(
            p.x,
            p.eye_y,
            p.z,
            (cx.mob.max_head_y_rot() + 20) as f32,
            cx.mob.max_head_x_rot() as f32,
        );
        if cx.mob.position().distance_to_sqr3(p.x, p.y, p.z)
            < self.stop_distance * self.stop_distance
        {
            self.stop_navigation(cx);
        } else {
            self.navigate_towards(cx, &p);
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

    fn ai() -> MobAi<TestMob> {
        let mut g = GridLevel::new();
        g.floor(-20, -20, 20, 20, 63);
        let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, 1);
        mob.others = vec![(EntityClass::Player, EntityFacts::player(11, 4.5, 64.0, 0.5))];
        mob.tempting = vec![11];
        MobAi::new(mob)
    }

    /// Only a player holding the item tempts; a scared mob within six
    /// blocks gives up when the player moves a tenth of a block or turns
    /// five degrees; stopping calms it for 50 asks.
    #[test]
    fn tempts_follows_and_is_scared_off() {
        let mut ai = ai();
        let mut g = TemptGoal::new(1.0, true);
        assert!(g.can_use(&mut ai));
        assert_eq!(g.player(), Some(11));
        g.start(&mut ai);
        assert!(g.is_running());
        g.tick(&mut ai);
        assert!(
            !ai.navigation.is_done(),
            "farther than 2.5: paths to the player"
        );
        assert!(ai.look_control.is_looking_at_target());
        assert!(g.can_continue_to_use(&mut ai));
        ai.mob.others[0].1.x += 0.2;
        assert!(!g.can_continue_to_use(&mut ai));
        ai.mob.others[0].1.x -= 0.2;
        assert!(g.can_continue_to_use(&mut ai));
        ai.mob.others[0].1.y_rot = 6.0;
        assert!(!g.can_continue_to_use(&mut ai));
        g.stop(&mut ai);
        assert!(!g.is_running());
        for _ in 0..50 {
            assert!(!g.can_use(&mut ai));
        }
        assert!(g.can_use(&mut ai));
        ai.mob.tempting.clear();
        assert!(!g.can_use(&mut ai));
    }

    /// Within the stop distance the navigation is stopped instead.
    #[test]
    fn stops_inside_the_stop_distance() {
        let mut ai = ai();
        ai.mob.others[0].1.x = 2.0;
        let mut g = TemptGoal::new(1.0, false);
        assert!(g.can_use(&mut ai));
        g.start(&mut ai);
        g.tick(&mut ai);
        assert!(ai.navigation.is_done());
        assert!(
            g.can_continue_to_use(&mut ai),
            "not scared: keeps following"
        );
    }
}
