//! `net.minecraft.world.entity.ai.goal.TradeWithPlayerGoal`, from the 26.2
//! jar. Flags `JUMP, MOVE`. Used by WanderingTrader.
//!
//! `canUse()`, offsets 0..80: `mob.isAlive() && !mob.isInWater() &&
//! mob.onGround() && !mob.hurtMarked && (p = getTradingPlayer()) != null
//! && mob.distanceToSqr(p) <= 16.0`. `start()`: `navigation.stop()`;
//! `stop()`: `setTradingPlayer(null)`.

use crate::ai::goal::{Flag, FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::host::MobHost;

/// `TradeWithPlayerGoal`.
#[derive(Debug, Clone, Default)]
pub struct TradeWithPlayerGoal;

impl TradeWithPlayerGoal {
    /// `TradeWithPlayerGoal(AbstractVillager)`.
    #[must_use]
    pub fn new() -> Self {
        TradeWithPlayerGoal
    }
}

impl<M: MobHost> Goal<MobAi<M>> for TradeWithPlayerGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        if !cx.mob.is_alive() || cx.mob.is_in_water() || !cx.mob.on_ground() || cx.mob.hurt_marked()
        {
            return false;
        }
        let Some(p) = cx.mob.trading_player() else {
            return false;
        };
        cx.mob.distance_to_sqr_entity(p).is_some_and(|d| d <= 16.0)
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        cx.navigation.stop();
    }

    fn stop(&mut self, cx: &mut MobAi<M>) {
        cx.mob.set_trading_player(None);
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

    #[test]
    fn stands_still_while_a_close_player_trades() {
        let mut mob = TestMob::new(GridLevel::new(), 0.5, 64.0, 0.5, 1);
        mob.on_ground = true;
        mob.others
            .push((EntityClass::Player, EntityFacts::player(9, 3.5, 64.0, 0.5)));
        let mut ai = MobAi::new(mob);
        let mut goal = TradeWithPlayerGoal::new();
        assert!(!goal.can_use(&mut ai), "nobody trading");
        ai.mob.trading_player = Some(9);
        assert!(goal.can_use(&mut ai));
        ai.mob.others[0].1.x = 5.5;
        assert!(!goal.can_use(&mut ai), "5 blocks: over 16 squared");
        ai.mob.others[0].1.x = 3.5;
        ai.mob.hurt_marked = true;
        assert!(!goal.can_use(&mut ai));
        ai.mob.hurt_marked = false;
        goal.stop(&mut ai);
        assert_eq!(ai.mob.trading_player, None);
    }
}
