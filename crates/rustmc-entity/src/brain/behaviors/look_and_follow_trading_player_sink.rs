//! `net.minecraft.world.entity.ai.behavior.LookAndFollowTradingPlayerSink`,
//! from the 26.2 jar. `Behavior<Villager>` with entry `{WALK_TARGET:
//! REGISTERED, LOOK_TARGET: REGISTERED}`, a fixed duration of
//! `Integer.MAX_VALUE` and `timedOut` overridden to `false`.
//!
//! ```text
//! checkExtraStartConditions (0..47): p = villager.getTradingPlayer();
//!   villager.isAlive() && p != null && !villager.isInWater() && !villager.hurtMarked && villager.distanceToSqr(p) <= 16.0
//! canStillUse: checkExtraStartConditions
//! start / tick: followPlayer: WALK_TARGET = WalkTarget(EntityTracker(p, false), speed, 2); LOOK_TARGET = EntityTracker(p, true)
//! stop (0..22): erase WALK_TARGET, LOOK_TARGET
//! ```

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::memory::{
    MemoryModuleType, MemoryStatus, MemoryValue, PositionTracker, WalkTarget,
};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `LookAndFollowTradingPlayerSink`'s body; build one with [`LookAndFollowTradingPlayerSink::new`].
#[derive(Debug, Clone, Copy)]
pub struct LookAndFollowTradingPlayerSink {
    pub speed_modifier: f32,
}

impl LookAndFollowTradingPlayerSink {
    /// `LookAndFollowTradingPlayerSink(float)`.
    #[must_use]
    pub fn new(speed_modifier: f32) -> Behavior<LookAndFollowTradingPlayerSink> {
        Behavior::with_duration(
            LookAndFollowTradingPlayerSink { speed_modifier },
            vec![
                (MemoryModuleType::WalkTarget, MemoryStatus::Registered),
                (MemoryModuleType::LookTarget, MemoryStatus::Registered),
            ],
            i32::MAX,
        )
    }

    fn follow_player<M: MobHost>(&self, brain: &mut BrainState, cx: &MobAi<M>) {
        let Some(player) = cx.mob.trading_player() else {
            return;
        };
        brain.set_memory_value(
            MemoryModuleType::WalkTarget,
            MemoryValue::WalkTarget(WalkTarget::new(
                PositionTracker::entity(player, false),
                self.speed_modifier,
                2,
            )),
        );
        brain.set_memory_value(
            MemoryModuleType::LookTarget,
            MemoryValue::PositionTracker(PositionTracker::entity(player, true)),
        );
    }
}

impl<M: MobHost> BehaviorBody<M> for LookAndFollowTradingPlayerSink {
    fn check_extra_start_conditions(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        let Some(player) = cx.mob.trading_player() else {
            return false;
        };
        cx.mob.is_alive()
            && !cx.mob.is_in_water()
            && !cx.mob.hurt_marked()
            && cx
                .mob
                .distance_to_sqr_entity(player)
                .is_some_and(|d| d <= 16.0)
    }

    fn can_still_use(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        self.check_extra_start_conditions(brain, cx)
    }

    fn start(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        self.follow_player(brain, cx);
    }

    fn tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        self.follow_player(brain, cx);
    }

    fn stop(&mut self, brain: &mut BrainState, _cx: &mut MobAi<M>, _time: i64) {
        brain.erase_memory(MemoryModuleType::WalkTarget);
        brain.erase_memory(MemoryModuleType::LookTarget);
    }

    fn timed_out(&self, _end_timestamp: i64, _time: i64) -> bool {
        false
    }

    fn name(&self) -> &'static str {
        "LookAndFollowTradingPlayerSink"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::{BehaviorControl, Status};
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn follows_the_trading_player_within_four_blocks_until_the_trade_ends() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        cx.mob.others = vec![(EntityClass::Player, EntityFacts::living(9, 3.5, 1.0, 0.5))];
        let mut brain = Brain::<TestMob>::new(&[T::WalkTarget, T::LookTarget], vec![]).state;
        let mut b = LookAndFollowTradingPlayerSink::new(0.5);
        assert!(!b.try_start(&mut brain, &mut cx, 0), "nobody trading");
        cx.mob.trading_player = Some(9);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(
            brain.get_memory(T::WalkTarget).unwrap().as_walk_target(),
            Some(&WalkTarget::new(PositionTracker::entity(9, false), 0.5, 2))
        );
        b.tick_or_stop(&mut brain, &mut cx, 5000);
        assert_eq!(
            BehaviorControl::<TestMob>::status(&b),
            Status::Running,
            "never times out"
        );
        cx.mob.others[0].1.x = 5.5;
        b.tick_or_stop(&mut brain, &mut cx, 5001);
        assert_eq!(
            BehaviorControl::<TestMob>::status(&b),
            Status::Stopped,
            "past 16 sq"
        );
        assert!(!brain.has_memory_value(T::WalkTarget));
        assert!(!brain.has_memory_value(T::LookTarget));
    }
}
