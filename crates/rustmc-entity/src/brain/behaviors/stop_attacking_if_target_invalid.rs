//! `net.minecraft.world.entity.ai.behavior.StopAttackingIfTargetInvalid`,
//! from the 26.2 jar. A `BehaviorBuilder` one-shot over
//! `group(present(ATTACK_TARGET), registered(CANT_REACH_WALK_TARGET_SINCE))`
//! (`lambda$create$6`, 0..92):
//!
//! ```text
//! target = attack
//! if mob.canAttack(target)
//!    && !(canGetTiredOfTryingToReachTarget && isTiredOfTryingToReachTarget(mob, cantReach.tryGet()))
//!    && target.isAlive()
//!    && target.level() == mob.level()
//!    && !stopCondition.test(level, target):
//!     return true                                      <- keeps the target; still "started"
//! onTargetErased.accept(level, mob, target); attack.erase(); return true
//! isTiredOfTryingToReachTarget(mob, since):  since.isPresent() && mob.level().getGameTime() - since.get() > 200
//! ```
//!
//! `TIMEOUT_TO_GET_WITHIN_ATTACK_RANGE = 200`. `create()` is
//! `((level, e) -> false, (level, mob, e) -> {}, true)`; `create(callback)`
//! and `create(condition)` fill the other default.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::memory::MemoryModuleType;
use crate::brain::BrainState;
use crate::host::{MobHost, PathLevel};

/// `TIMEOUT_TO_GET_WITHIN_ATTACK_RANGE`.
pub const TIMEOUT_TO_GET_WITHIN_ATTACK_RANGE: i64 = 200;

/// `StopAttackingIfTargetInvalid$StopAttackCondition`: `(level, target) -> bool`.
pub type StopAttackCondition<M> = Box<dyn FnMut(&mut BrainState, &mut MobAi<M>, i32) -> bool>;
/// `StopAttackingIfTargetInvalid$TargetErasedCallback<E>`: `(level, mob, target)`.
pub type TargetErasedCallback<M> = Box<dyn FnMut(&mut BrainState, &mut MobAi<M>, i32)>;

/// `StopAttackingIfTargetInvalid`'s trigger.
pub struct StopAttackingIfTargetInvalid<M: MobHost> {
    pub stop_condition: StopAttackCondition<M>,
    pub on_target_erased: TargetErasedCallback<M>,
    pub can_get_tired_of_trying_to_reach_target: bool,
}

impl<M: MobHost + 'static> StopAttackingIfTargetInvalid<M> {
    /// `create()`.
    #[must_use]
    pub fn create() -> OneShot<Self> {
        Self::create_full(Box::new(|_, _, _| false), Box::new(|_, _, _| {}), true)
    }

    /// `create(StopAttackCondition)`.
    #[must_use]
    pub fn create_condition(stop_condition: StopAttackCondition<M>) -> OneShot<Self> {
        Self::create_full(stop_condition, Box::new(|_, _, _| {}), true)
    }

    /// `create(TargetErasedCallback)`.
    #[must_use]
    pub fn create_callback(on_target_erased: TargetErasedCallback<M>) -> OneShot<Self> {
        Self::create_full(Box::new(|_, _, _| false), on_target_erased, true)
    }

    /// `create(StopAttackCondition, TargetErasedCallback, boolean)`.
    #[must_use]
    pub fn create_full(
        stop_condition: StopAttackCondition<M>,
        on_target_erased: TargetErasedCallback<M>,
        can_get_tired_of_trying_to_reach_target: bool,
    ) -> OneShot<Self> {
        OneShot::new(StopAttackingIfTargetInvalid {
            stop_condition,
            on_target_erased,
            can_get_tired_of_trying_to_reach_target,
        })
    }

    /// `isTiredOfTryingToReachTarget(mob, Optional<Long>)`.
    fn is_tired_of_trying_to_reach_target(cx: &MobAi<M>, since: Option<i64>) -> bool {
        since.is_some_and(|s| cx.mob.level().game_time() - s > TIMEOUT_TO_GET_WITHIN_ATTACK_RANGE)
    }
}

impl<M: MobHost + 'static> Trigger<M> for StopAttackingIfTargetInvalid<M> {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        let Some(Some(target)) = brain.memory_internal(MemoryModuleType::AttackTarget) else {
            return false;
        };
        let Some(target) = target.as_entity() else {
            return false;
        };
        let Some(since) = brain.memory_internal(MemoryModuleType::CantReachWalkTargetSince) else {
            return false;
        };
        let since = since.and_then(|v| v.as_long());
        let keep = cx.mob.entity(target).is_some_and(|e| cx.mob.can_attack(&e))
            && !(self.can_get_tired_of_trying_to_reach_target
                && Self::is_tired_of_trying_to_reach_target(cx, since))
            && cx.mob.entity(target).is_some_and(|e| e.alive)
            && cx.mob.entity_in_same_level(target)
            && !(self.stop_condition)(brain, cx, target);
        if keep {
            return true;
        }
        (self.on_target_erased)(brain, cx, target);
        brain.erase_memory(MemoryModuleType::AttackTarget);
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![
            MemoryModuleType::AttackTarget,
            MemoryModuleType::CantReachWalkTargetSince,
        ]
    }

    fn name(&self) -> &'static str {
        "StopAttackingIfTargetInvalid"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::{MemoryModuleType as T, MemoryValue};
    use crate::brain::Brain;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    fn setup() -> (MobAi<TestMob>, BrainState) {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        cx.mob.others = vec![(EntityClass::Living, EntityFacts::living(3, 2.5, 1.0, 0.5))];
        let mut brain =
            Brain::<TestMob>::new(&[T::AttackTarget, T::CantReachWalkTargetSince], vec![]).state;
        brain.set_memory_value(T::AttackTarget, MemoryValue::Entity(3));
        (cx, brain)
    }

    #[test]
    fn keeps_a_valid_target_and_drops_a_dead_one() {
        let (mut cx, mut brain) = setup();
        let mut b = StopAttackingIfTargetInvalid::<TestMob>::create();
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(brain.has_memory_value(T::AttackTarget));
        cx.mob.others[0].1.alive = false;
        assert!(b.try_start(&mut brain, &mut cx, 1), "still answers true");
        assert!(!brain.has_memory_value(T::AttackTarget));
    }

    #[test]
    fn gives_up_after_more_than_200_ticks_of_not_reaching() {
        let (mut cx, mut brain) = setup();
        brain.set_memory_value(T::CantReachWalkTargetSince, MemoryValue::Long(100));
        cx.mob.level.game_time = 300;
        let mut b = StopAttackingIfTargetInvalid::<TestMob>::create();
        assert!(b.try_start(&mut brain, &mut cx, 300));
        assert!(
            brain.has_memory_value(T::AttackTarget),
            "exactly 200 is not > 200"
        );
        cx.mob.level.game_time = 301;
        assert!(b.try_start(&mut brain, &mut cx, 301));
        assert!(!brain.has_memory_value(T::AttackTarget));
        // ... unless the behaviour cannot get tired.
        brain.set_memory_value(T::AttackTarget, MemoryValue::Entity(3));
        let mut b = StopAttackingIfTargetInvalid::<TestMob>::create_full(
            Box::new(|_, _, _| false),
            Box::new(|_, _, _| {}),
            false,
        );
        assert!(b.try_start(&mut brain, &mut cx, 301));
        assert!(brain.has_memory_value(T::AttackTarget));
    }

    #[test]
    fn the_callback_sees_the_target_before_it_is_erased() {
        let (mut cx, mut brain) = setup();
        let seen = std::rc::Rc::new(std::cell::Cell::new(None));
        let s2 = std::rc::Rc::clone(&seen);
        let mut b = StopAttackingIfTargetInvalid::<TestMob>::create_full(
            Box::new(|_, _, _| true),
            Box::new(move |brain, _, t| s2.set(Some((t, brain.has_memory_value(T::AttackTarget))))),
            true,
        );
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(seen.get(), Some((3, true)));
        assert!(!brain.has_memory_value(T::AttackTarget));
    }
}
