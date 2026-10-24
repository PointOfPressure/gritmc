//! `net.minecraft.world.entity.ai.goal.target.NearestAttackableWitchTargetGoal<T>`,
//! from the 26.2 jar. A `NearestAttackableTargetGoal` with a `canAttack`
//! switch (true at construction, `setCanAttack`); `canUse()` is
//! `canAttack && super.canUse()`. Used by Witch.

use crate::ai::goal::{FlagSet, Goal};
use crate::ai::goals::target_goal::NearestAttackableTargetGoal;
use crate::ai::mob_ai::MobAi;
use crate::ai::targeting::Selector;
use crate::host::{EntityClass, MobHost};

/// `NearestAttackableWitchTargetGoal`.
pub struct NearestAttackableWitchTargetGoal<M: MobHost> {
    inner: NearestAttackableTargetGoal<M>,
    can_attack: bool,
}

impl<M: MobHost> NearestAttackableWitchTargetGoal<M> {
    /// `NearestAttackableWitchTargetGoal(Raider, Class<T>, int, boolean, boolean, Selector)`.
    #[must_use]
    pub fn new(
        cx: &MobAi<M>,
        target_type: EntityClass,
        random_interval: i32,
        must_see: bool,
        must_reach: bool,
        selector: Option<Selector<M>>,
    ) -> Self {
        NearestAttackableWitchTargetGoal {
            inner: NearestAttackableTargetGoal::with(
                cx,
                target_type,
                random_interval,
                must_see,
                must_reach,
                selector,
            ),
            can_attack: true,
        }
    }

    /// `setCanAttack(boolean)`.
    pub fn set_can_attack(&mut self, v: bool) {
        self.can_attack = v;
    }

    /// The found target, for tests.
    #[must_use]
    pub fn found(&self) -> Option<i32> {
        self.inner.found()
    }
}

impl<M: MobHost> Goal<MobAi<M>> for NearestAttackableWitchTargetGoal<M> {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        self.can_attack && self.inner.can_use(cx)
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

    fn flags(&self) -> FlagSet {
        self.inner.flags()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::EntityFacts;
    use crate::testing::{GridLevel, TestMob};

    /// With the switch off no draw is made either: the `&&` short-circuits
    /// before `super.canUse()`.
    #[test]
    fn the_switch_gates_before_the_interval_draw() {
        let mut mob = TestMob::new(GridLevel::new(), 0.5, 64.0, 0.5, 1);
        mob.follow_range = 16.0;
        mob.others
            .push((EntityClass::Player, EntityFacts::player(9, 4.5, 64.0, 0.5)));
        mob.visible.push(9);
        let mut ai = MobAi::new(mob);
        let mut goal =
            NearestAttackableWitchTargetGoal::new(&ai, EntityClass::Player, 10, true, false, None);
        goal.set_can_attack(false);
        let before = ai.mob.random;
        for _ in 0..10 {
            assert!(!goal.can_use(&mut ai));
        }
        assert_eq!(
            format!("{before:?}"),
            format!("{:?}", ai.mob.random),
            "no draw"
        );
        goal.set_can_attack(true);
        let mut found = false;
        for _ in 0..30 {
            found |= goal.can_use(&mut ai);
        }
        assert!(found);
        assert_eq!(goal.found(), Some(9));
    }
}
