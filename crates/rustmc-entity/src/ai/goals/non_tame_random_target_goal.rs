//! `net.minecraft.world.entity.ai.goal.target.NonTameRandomTargetGoal<T>`,
//! from the 26.2 jar. A `NearestAttackableTargetGoal` built with
//! `(mob, class, 10, mustSee, false, selector)`. Used by Cat, Ocelot,
//! Wolf (x2).
//!
//! ```text
//! canUse()             !tamableMob.isTame() && super.canUse()
//! canContinueToUse()   targetConditions != null
//!                        ? targetConditions.test(level, mob, target)
//!                        : super.canContinueToUse()
//! ```
//!
//! `targetConditions` is assigned in the superclass constructor and is
//! never null, so the `super.canContinueToUse()` arm is dead: a wild
//! cat keeps its target exactly as long as the conditions hold against
//! the goal's own `target` field, with no unseen memory.

use crate::ai::goal::{FlagSet, Goal};
use crate::ai::goals::target_goal::NearestAttackableTargetGoal;
use crate::ai::mob_ai::MobAi;
use crate::ai::targeting::Selector;
use crate::host::{EntityClass, MobHost};

/// `NonTameRandomTargetGoal`.
pub struct NonTameRandomTargetGoal<M: MobHost> {
    inner: NearestAttackableTargetGoal<M>,
}

impl<M: MobHost> NonTameRandomTargetGoal<M> {
    /// `NonTameRandomTargetGoal(TamableAnimal, Class<T>, boolean mustSee, Selector)`.
    #[must_use]
    pub fn new(
        cx: &MobAi<M>,
        target_type: EntityClass,
        must_see: bool,
        selector: Option<Selector<M>>,
    ) -> Self {
        NonTameRandomTargetGoal {
            inner: NearestAttackableTargetGoal::with(
                cx,
                target_type,
                10,
                must_see,
                false,
                selector,
            ),
        }
    }

    /// The found target, for tests.
    #[must_use]
    pub fn found(&self) -> Option<i32> {
        self.inner.found()
    }
}

impl<M: MobHost> Goal<MobAi<M>> for NonTameRandomTargetGoal<M> {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        !cx.mob.is_tame() && self.inner.can_use(cx)
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        self.inner.conditions_hold_for_found(cx)
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

    /// A wild cat finds the chicken (draws `nextInt(5)` until a zero); a
    /// tame one never does; continuation is the conditions alone, so a
    /// target that leaves the follow range is dropped at once and one
    /// that stays is kept even out of sight.
    #[test]
    fn wild_only_and_conditions_only_continuation() {
        let mut mob = TestMob::new(GridLevel::new(), 0.5, 64.0, 0.5, 1);
        mob.follow_range = 16.0;
        mob.others
            .push((EntityClass::Animal, EntityFacts::living(7, 4.5, 64.0, 0.5)));
        mob.visible.push(7);
        let mut ai = MobAi::new(mob);
        let mut goal = NonTameRandomTargetGoal::new(&ai, EntityClass::Animal, false, None);
        let mut found = false;
        for _ in 0..30 {
            if goal.can_use(&mut ai) {
                found = true;
                break;
            }
        }
        assert!(found);
        assert_eq!(goal.found(), Some(7));
        goal.start(&mut ai);
        assert_eq!(ai.mob.target_raw, Some(7));
        assert!(goal.can_continue_to_use(&mut ai));
        ai.mob.others[0].1.x = 40.5;
        assert!(
            !goal.can_continue_to_use(&mut ai),
            "out of range: dropped at once"
        );
        ai.mob.tame = true;
        for _ in 0..30 {
            assert!(!goal.can_use(&mut ai));
        }
    }
}
