//! `net.minecraft.world.entity.ai.goal.FollowFlockLeaderGoal`, from the
//! 26.2 jar. No flags; `INTERVAL_TICKS = 200`. Used by AbstractSchoolingFish.
//!
//! The constructor sets `nextStartTick = nextStartTick(mob)`:
//! `reducedTickDelay(200 + random.nextInt(200) % 20)` — one draw.
//!
//! # `canUse()`, offsets 0..158
//!
//! ```text
//! if mob.hasFollowers() return false
//! if mob.isFollower() return true
//! if nextStartTick > 0: --nextStartTick; return false
//! nextStartTick = nextStartTick(mob)
//! list = level.getEntitiesOfClass(mob.getClass(), bb.inflate(8, 8, 8),
//!                                 f -> f.canBeFollowed() || !f.isFollower())
//! leader = list.stream().filter(f -> f.canBeFollowed()).findAny().orElse(mob)
//! leader.addFollowers(list.stream().filter(f -> !f.isFollower()))
//! return mob.isFollower()
//! ```
//!
//! `findAny` on a sequential stream is the first match.
//! `addFollowers(stream)`: `stream.limit(getMaxSchoolSize() - schoolSize)
//! .filter(f -> f != this).forEach(f -> f.startFollowing(this))`, where
//! the limit is read once before the walk and `startFollowing` bumps the
//! leader's `schoolSize`.
//!
//! `canContinueToUse()`: `isFollower() && inRangeOfLeader()`. `start()`:
//! `timeToRecalcPath = 0`. `stop()`: `stopFollowing()`. `tick()`: `if
//! --timeToRecalcPath > 0 return; timeToRecalcPath = adjustedTickDelay(10);
//! pathToLeader()` — `navigation.moveTo(leader, 1.0)` when a follower.

use rustmc_worldgen::rng::RandomSource;

use crate::ai::goal::{adjusted_tick_delay, reduced_tick_delay, FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::host::{EntityClass, MobHost};

/// `INTERVAL_TICKS`.
pub const INTERVAL_TICKS: i32 = 200;

/// `FollowFlockLeaderGoal`.
#[derive(Debug, Clone)]
pub struct FollowFlockLeaderGoal {
    time_to_recalc_path: i32,
    next_start_tick: i32,
}

impl FollowFlockLeaderGoal {
    /// `FollowFlockLeaderGoal(AbstractSchoolingFish)`: draws once.
    #[must_use]
    pub fn new<M: MobHost>(cx: &mut MobAi<M>) -> Self {
        let n = Self::next_start_tick(cx);
        FollowFlockLeaderGoal {
            time_to_recalc_path: 0,
            next_start_tick: n,
        }
    }

    /// `nextStartTick(AbstractSchoolingFish)`.
    fn next_start_tick<M: MobHost>(cx: &mut MobAi<M>) -> i32 {
        reduced_tick_delay(INTERVAL_TICKS + cx.mob.random().next_i32_bound(200) % 20)
    }

    /// `AbstractSchoolingFish.addFollowers(Stream)` on `leader`.
    fn add_followers<M: MobHost>(cx: &mut MobAi<M>, leader: i32, candidates: &[i32]) {
        let limit = cx.mob.fish_max_school_size(leader) - cx.mob.fish_school_size(leader);
        let limit = usize::try_from(limit).unwrap_or(0);
        for f in candidates.iter().take(limit) {
            if *f == leader {
                continue;
            }
            cx.mob.fish_start_following(*f, leader);
        }
    }

    /// `nextStartTick`, for tests.
    #[must_use]
    pub fn next_start_tick_value(&self) -> i32 {
        self.next_start_tick
    }
}

impl<M: MobHost> Goal<MobAi<M>> for FollowFlockLeaderGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        let me = cx.mob.id();
        if cx.mob.fish_has_followers(me) {
            return false;
        }
        if cx.mob.fish_is_follower(me) {
            return true;
        }
        if self.next_start_tick > 0 {
            self.next_start_tick -= 1;
            return false;
        }
        self.next_start_tick = Self::next_start_tick(cx);
        let area = cx.mob.bounding_box().inflate(8.0, 8.0, 8.0);
        let list: Vec<i32> = cx
            .mob
            .entities_of_class_in(EntityClass::SameKind, &area)
            .into_iter()
            .filter(|f| cx.mob.fish_can_be_followed(*f) || !cx.mob.fish_is_follower(*f))
            .collect();
        let leader = list
            .iter()
            .copied()
            .find(|f| cx.mob.fish_can_be_followed(*f))
            .unwrap_or(me);
        let followers: Vec<i32> = list
            .iter()
            .copied()
            .filter(|f| !cx.mob.fish_is_follower(*f))
            .collect();
        Self::add_followers(cx, leader, &followers);
        cx.mob.fish_is_follower(me)
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        let me = cx.mob.id();
        if !cx.mob.fish_is_follower(me) {
            return false;
        }
        let Some(l) = cx.mob.fish_leader(me) else {
            return false;
        };
        cx.mob.distance_to_sqr_entity(l).is_some_and(|d| d <= 121.0)
    }

    fn start(&mut self, _cx: &mut MobAi<M>) {
        self.time_to_recalc_path = 0;
    }

    fn stop(&mut self, cx: &mut MobAi<M>) {
        cx.mob.fish_stop_following();
    }

    fn tick(&mut self, cx: &mut MobAi<M>) {
        self.time_to_recalc_path -= 1;
        if self.time_to_recalc_path > 0 {
            return;
        }
        self.time_to_recalc_path = adjusted_tick_delay(false, 10);
        let me = cx.mob.id();
        if cx.mob.fish_is_follower(me) {
            if let Some(l) = cx.mob.fish_leader(me).and_then(|l| cx.mob.entity(l)) {
                cx.navigation
                    .move_to_entity_at(&mut cx.mob, l.block_position(), 1.0);
            }
        }
    }

    fn flags(&self) -> FlagSet {
        FlagSet::NONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::EntityFacts;
    use crate::testing::{GridLevel, TestMob};

    /// Three loose cod (school size 1, max 3 each): none can be
    /// followed, so this fish leads itself and takes the two others up
    /// to its limit; the third candidate (itself) is skipped by the
    /// `f != this` filter after counting against the limit.
    #[test]
    fn a_loose_fish_gathers_a_school_around_itself() {
        let mut g = GridLevel::new();
        let mut mob = TestMob::new(g.clone(), 0.5, 64.0, 0.5, 2);
        mob.id = 1;
        mob.others.push((
            EntityClass::SameKind,
            EntityFacts::living(2, 2.5, 64.0, 0.5),
        ));
        mob.others.push((
            EntityClass::SameKind,
            EntityFacts::living(3, 3.5, 64.0, 0.5),
        ));
        mob.others.push((
            EntityClass::SameKind,
            EntityFacts::living(1, 0.5, 64.0, 0.5),
        ));
        for id in 1..=3 {
            mob.fish.insert(id, (1, 3, None));
        }
        let mut ai = MobAi::new(mob);
        let mut goal = FollowFlockLeaderGoal::new(&mut ai);
        let n = goal.next_start_tick_value();
        assert!((100..110).contains(&n), "{n}");
        for _ in 0..n {
            assert!(!goal.can_use(&mut ai));
        }
        assert!(
            !goal.can_use(&mut ai),
            "the leader is not itself a follower"
        );
        assert_eq!(
            ai.mob.calls,
            vec!["startFollowing(2, 1)", "startFollowing(3, 1)"]
        );
        assert_eq!(ai.mob.fish[&1].0, 3);
        g.game_time = 0;
    }
}
