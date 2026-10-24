//! `net.minecraft.world.entity.ai.behavior.StartAttacking`, from the 26.2
//! jar. A `BehaviorBuilder` one-shot over
//! `group(absent(ATTACK_TARGET), registered(CANT_REACH_WALK_TARGET_SINCE))`
//! (`lambda$create$3`, 0..70):
//!
//! ```text
//! if !condition.test(level, mob): return false
//! target = finder.get(level, mob); if empty: return false
//! if !mob.canAttack(target): return false
//! attack.set(target); cantReach.erase(); return true
//! ```
//!
//! `create(finder)` is `create((level, mob) -> true, finder)`.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::memory::{MemoryModuleType, MemoryValue};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `StartAttacking$StartAttackingCondition<E>`.
pub type StartAttackingCondition<M> = Box<dyn FnMut(&mut BrainState, &mut MobAi<M>) -> bool>;
/// `StartAttacking$TargetFinder<E>`: `Optional<? extends LivingEntity>`.
pub type TargetFinder<M> = Box<dyn FnMut(&mut BrainState, &mut MobAi<M>) -> Option<i32>>;

/// `StartAttacking`'s trigger.
pub struct StartAttacking<M: MobHost> {
    pub condition: StartAttackingCondition<M>,
    pub finder: TargetFinder<M>,
}

impl<M: MobHost + 'static> StartAttacking<M> {
    /// `create(TargetFinder)`.
    #[must_use]
    pub fn create(finder: TargetFinder<M>) -> OneShot<Self> {
        Self::create_with(Box::new(|_, _| true), finder)
    }

    /// `create(StartAttackingCondition, TargetFinder)`.
    #[must_use]
    pub fn create_with(
        condition: StartAttackingCondition<M>,
        finder: TargetFinder<M>,
    ) -> OneShot<Self> {
        OneShot::new(StartAttacking { condition, finder })
    }
}

impl<M: MobHost> Trigger<M> for StartAttacking<M> {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        if brain.memory_internal(MemoryModuleType::AttackTarget) != Some(None) {
            return false;
        }
        if brain
            .memory_internal(MemoryModuleType::CantReachWalkTargetSince)
            .is_none()
        {
            return false;
        }
        if !(self.condition)(brain, cx) {
            return false;
        }
        let Some(target) = (self.finder)(brain, cx) else {
            return false;
        };
        if !cx.mob.entity(target).is_some_and(|e| cx.mob.can_attack(&e)) {
            return false;
        }
        brain.set_memory_value(MemoryModuleType::AttackTarget, MemoryValue::Entity(target));
        brain.erase_memory(MemoryModuleType::CantReachWalkTargetSince);
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![
            MemoryModuleType::AttackTarget,
            MemoryModuleType::CantReachWalkTargetSince,
        ]
    }

    fn name(&self) -> &'static str {
        "StartAttacking"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn sets_the_target_and_clears_the_unreachable_stamp() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        cx.mob.others = vec![(EntityClass::Living, EntityFacts::living(3, 2.5, 1.0, 0.5))];
        let mut brain =
            Brain::<TestMob>::new(&[T::AttackTarget, T::CantReachWalkTargetSince], vec![]).state;
        brain.set_memory_value(T::CantReachWalkTargetSince, MemoryValue::Long(5));
        let mut b = StartAttacking::<TestMob>::create(Box::new(|_, _| Some(3)));
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(
            brain.get_memory(T::AttackTarget),
            Some(&MemoryValue::Entity(3))
        );
        assert!(!brain.has_memory_value(T::CantReachWalkTargetSince));
        assert!(
            !b.try_start(&mut brain, &mut cx, 1),
            "ATTACK_TARGET now present"
        );
    }

    #[test]
    fn a_false_condition_never_asks_the_finder() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut brain =
            Brain::<TestMob>::new(&[T::AttackTarget, T::CantReachWalkTargetSince], vec![]).state;
        let mut b = StartAttacking::<TestMob>::create_with(
            Box::new(|_, _| false),
            Box::new(|_, _| panic!("finder asked")),
        );
        assert!(!b.try_start(&mut brain, &mut cx, 0));
    }
}
