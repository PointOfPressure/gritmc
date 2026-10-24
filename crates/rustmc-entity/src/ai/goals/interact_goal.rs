//! `net.minecraft.world.entity.ai.goal.InteractGoal`, from the 26.2 jar:
//! a `LookAtPlayerGoal` whose constructors re-`setFlags` to
//! `LOOK, MOVE`. Used by WanderingTrader.

use crate::ai::goal::{Flag, FlagSet, Goal};
use crate::ai::goals::look_at_player_goal::LookAtPlayerGoal;
use crate::ai::mob_ai::MobAi;
use crate::host::{EntityClass, MobHost};

/// `InteractGoal`.
pub struct InteractGoal<M: MobHost> {
    inner: LookAtPlayerGoal<M>,
}

impl<M: MobHost> InteractGoal<M> {
    /// `InteractGoal(Mob, Class, float lookDistance)`.
    #[must_use]
    pub fn new(cx: &MobAi<M>, look_at_type: EntityClass, look_distance: f32) -> Self {
        InteractGoal {
            inner: LookAtPlayerGoal::new(cx, look_at_type, look_distance),
        }
    }

    /// `InteractGoal(Mob, Class, float lookDistance, float probability)`.
    #[must_use]
    pub fn with_probability(
        cx: &MobAi<M>,
        look_at_type: EntityClass,
        look_distance: f32,
        probability: f32,
    ) -> Self {
        InteractGoal {
            inner: LookAtPlayerGoal::with(cx, look_at_type, look_distance, probability, false),
        }
    }
}

impl<M: MobHost> Goal<MobAi<M>> for InteractGoal<M> {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        self.inner.can_use(cx)
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
        FlagSet::of2(Flag::Look, Flag::Move)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn interact_holds_move_as_well_as_look() {
        let ai = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 64.0, 0.5, 1));
        let goal = InteractGoal::new(&ai, EntityClass::Player, 8.0);
        let flags: Vec<Flag> = Goal::<MobAi<TestMob>>::flags(&goal).iter().collect();
        assert_eq!(flags, vec![Flag::Move, Flag::Look]);
    }
}
