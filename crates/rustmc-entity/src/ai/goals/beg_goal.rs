//! `net.minecraft.world.entity.ai.goal.BegGoal`, from the 26.2 jar. Flag
//! `LOOK`; `begTargeting = forNonCombat().range(lookDistance)`. Used by Wolf.
//!
//! `canUse()`: `player = level.getNearestPlayer(begTargeting, wolf)`; none
//! is false, else `playerHoldingInteresting(player)` — a bone or
//! `wolf.isFood` in either hand. `canContinueToUse()`: alive, within
//! `lookDistance²`, `lookTime > 0 && playerHoldingInteresting`. `start()`:
//! `setIsInterested(true); lookTime = adjustedTickDelay(40 + nextInt(40))`
//! (halved, offset 21). `stop()`: `setIsInterested(false); player = null`.
//! `tick()`: `lookControl.setLookAt(player.x, player.eyeY, player.z, 10.0f,
//! (float) getMaxHeadXRot()); --lookTime`.

use rustmc_worldgen::rng::RandomSource;

use crate::ai::goal::{adjusted_tick_delay, Flag, FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::ai::targeting::{nearest_player_to_mob, TargetingConditions};
use crate::host::MobHost;

/// `BegGoal`.
pub struct BegGoal<M: MobHost> {
    player: Option<i32>,
    look_distance: f32,
    look_time: i32,
    beg_targeting: TargetingConditions<M>,
}

impl<M: MobHost> BegGoal<M> {
    /// `BegGoal(Wolf, float lookDistance)`.
    #[must_use]
    pub fn new(look_distance: f32) -> Self {
        BegGoal {
            player: None,
            look_distance,
            look_time: 0,
            beg_targeting: TargetingConditions::for_non_combat().range(f64::from(look_distance)),
        }
    }

    /// `lookTime`, for tests.
    #[must_use]
    pub fn look_time(&self) -> i32 {
        self.look_time
    }
}

impl<M: MobHost> Goal<MobAi<M>> for BegGoal<M> {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        self.player = nearest_player_to_mob(cx, &self.beg_targeting);
        match self.player {
            None => false,
            Some(p) => cx.mob.player_holding_interesting(p),
        }
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        let Some(p) = self.player else {
            return false;
        };
        let Some(f) = cx.mob.entity(p) else {
            return false;
        };
        if !f.alive {
            return false;
        }
        if f.distance_to_sqr(cx.mob.x(), cx.mob.y(), cx.mob.z())
            > f64::from(self.look_distance * self.look_distance)
        {
            return false;
        }
        self.look_time > 0 && cx.mob.player_holding_interesting(p)
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        cx.mob.set_is_interested(true);
        self.look_time = adjusted_tick_delay(false, 40 + cx.mob.random().next_i32_bound(40));
    }

    fn stop(&mut self, cx: &mut MobAi<M>) {
        cx.mob.set_is_interested(false);
        self.player = None;
    }

    fn tick(&mut self, cx: &mut MobAi<M>) {
        if let Some(f) = self.player.and_then(|p| cx.mob.entity(p)) {
            #[allow(clippy::cast_precision_loss)]
            let max_x = cx.mob.max_head_x_rot() as f32;
            cx.look_control
                .set_look_at_with(f.x, f.eye_y, f.z, 10.0, max_x);
        }
        self.look_time -= 1;
    }

    fn flags(&self) -> FlagSet {
        FlagSet::of(Flag::Look)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn begs_at_a_player_holding_a_bone_within_eight() {
        let mut mob = TestMob::new(GridLevel::new(), 0.5, 64.0, 0.5, 5);
        mob.others
            .push((EntityClass::Player, EntityFacts::player(9, 4.5, 64.0, 0.5)));
        mob.visible.push(9);
        let mut ai = MobAi::new(mob);
        let mut goal = BegGoal::new(8.0);
        assert!(!goal.can_use(&mut ai), "nothing interesting");
        ai.mob.interesting_players.push(9);
        assert!(goal.can_use(&mut ai));
        goal.start(&mut ai);
        assert!(
            goal.look_time() >= 20 && goal.look_time() <= 40,
            "{}",
            goal.look_time()
        );
        assert!(goal.can_continue_to_use(&mut ai));
        ai.mob.others[0].1.x = 9.5;
        assert!(!goal.can_continue_to_use(&mut ai), "nine blocks");
        goal.stop(&mut ai);
        assert_eq!(
            ai.mob.calls,
            vec!["setIsInterested(true)", "setIsInterested(false)"]
        );
    }
}
