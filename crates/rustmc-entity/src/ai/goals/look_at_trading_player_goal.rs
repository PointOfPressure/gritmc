//! `net.minecraft.world.entity.ai.goal.LookAtTradingPlayerGoal`, from the
//! 26.2 jar: `LookAtPlayerGoal(villager, Player.class, 8.0f)` whose
//! `canUse()` is `if villager.isTrading(): lookAt = getTradingPlayer();
//! return true; else false` — no probability draw, no search. Used by
//! WanderingTrader.

use crate::ai::goal::{FlagSet, Goal};
use crate::ai::goals::look_at_player_goal::LookAtPlayerGoal;
use crate::ai::mob_ai::MobAi;
use crate::host::{EntityClass, MobHost};

/// `LookAtTradingPlayerGoal`.
pub struct LookAtTradingPlayerGoal<M: MobHost> {
    inner: LookAtPlayerGoal<M>,
}

impl<M: MobHost> LookAtTradingPlayerGoal<M> {
    /// `LookAtTradingPlayerGoal(AbstractVillager)`.
    #[must_use]
    pub fn new(cx: &MobAi<M>) -> Self {
        LookAtTradingPlayerGoal {
            inner: LookAtPlayerGoal::new(cx, EntityClass::Player, 8.0),
        }
    }

    /// The entity being looked at, for tests.
    #[must_use]
    pub fn looking_at(&self) -> Option<i32> {
        self.inner.looking_at()
    }
}

impl<M: MobHost> Goal<MobAi<M>> for LookAtTradingPlayerGoal<M> {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        if cx.mob.is_trading() {
            self.inner.set_look_at(cx.mob.trading_player());
            return true;
        }
        false
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        self.inner.can_continue_to_use(cx)
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        self.inner.start(cx);
    }

    fn stop(&mut self, cx: &mut MobAi<M>) {
        self.inner.stop(cx);
    }

    fn tick(&mut self, cx: &mut MobAi<M>) {
        self.inner.tick(cx);
    }

    fn flags(&self) -> FlagSet {
        self.inner.flags()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::EntityFacts;
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn looks_at_the_trading_player_without_a_draw() {
        let mut mob = TestMob::new(GridLevel::new(), 0.5, 64.0, 0.5, 1);
        mob.others
            .push((EntityClass::Player, EntityFacts::player(9, 3.5, 64.0, 0.5)));
        let mut ai = MobAi::new(mob);
        let mut goal = LookAtTradingPlayerGoal::new(&ai);
        let before = format!("{:?}", ai.mob.random);
        assert!(!goal.can_use(&mut ai));
        ai.mob.trading_player = Some(9);
        assert!(goal.can_use(&mut ai));
        assert_eq!(goal.looking_at(), Some(9));
        assert_eq!(before, format!("{:?}", ai.mob.random));
    }
}
