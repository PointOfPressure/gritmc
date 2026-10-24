//! `net.minecraft.world.entity.ai.behavior.MeleeAttack`, from the 26.2 jar.
//! A `BehaviorBuilder` one-shot over `group(registered(LOOK_TARGET),
//! present(ATTACK_TARGET), absent(ATTACK_COOLING_DOWN),
//! present(NEAREST_VISIBLE_LIVING_ENTITIES))` (`lambda$create$3`, 0..103):
//!
//! ```text
//! target = attack
//! if pred.test(mob) && !isHoldingUsableNonMeleeWeapon(mob) && mob.isWithinMeleeAttackRange(target)
//!    && nearestVisible.contains(target):
//!     look.set(new EntityTracker(target, true))
//!     mob.swing(MAIN_HAND); mob.doHurtTarget(level, target)
//!     coolingDown.setWithExpiry(true, cooldownBetweenAttacks); return true
//! return false
//! isHoldingUsableNonMeleeWeapon(mob):  mob.isHolding(mob::canUseNonMeleeWeapon)
//! ```
//!
//! `create(int)` is `create(mob -> true, cooldown)`.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::behaviors::nearest_visible_contains;
use crate::brain::behaviors::MobPredicate;
use crate::brain::memory::{MemoryModuleType, MemoryValue, PositionTracker};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `MeleeAttack`'s trigger.
pub struct MeleeAttack<M: MobHost> {
    /// `Predicate<T>`.
    pub predicate: MobPredicate<M>,
    pub cooldown_between_attacks: i32,
}

impl<M: MobHost + 'static> MeleeAttack<M> {
    /// `create(int)`.
    #[must_use]
    pub fn create(cooldown_between_attacks: i32) -> OneShot<Self> {
        Self::create_with(Box::new(|_| true), cooldown_between_attacks)
    }

    /// `create(Predicate, int)`.
    #[must_use]
    pub fn create_with(predicate: MobPredicate<M>, cooldown_between_attacks: i32) -> OneShot<Self> {
        OneShot::new(MeleeAttack {
            predicate,
            cooldown_between_attacks,
        })
    }
}

impl<M: MobHost> Trigger<M> for MeleeAttack<M> {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        if brain
            .memory_internal(MemoryModuleType::LookTarget)
            .is_none()
        {
            return false;
        }
        let Some(Some(target)) = brain.memory_internal(MemoryModuleType::AttackTarget) else {
            return false;
        };
        let Some(target) = target.as_entity() else {
            return false;
        };
        if brain.memory_internal(MemoryModuleType::AttackCoolingDown) != Some(None) {
            return false;
        }
        if !matches!(
            brain.memory_internal(MemoryModuleType::NearestVisibleLivingEntities),
            Some(Some(_))
        ) {
            return false;
        }
        if (self.predicate)(cx)
            && !cx.mob.is_holding_usable_non_melee_weapon()
            && cx.mob.is_within_melee_attack_range(target)
            && nearest_visible_contains(brain, cx, target)
        {
            brain.set_memory_value(
                MemoryModuleType::LookTarget,
                MemoryValue::PositionTracker(PositionTracker::entity(target, true)),
            );
            cx.mob.swing_main_hand();
            cx.mob.do_hurt_target(target);
            brain.set_memory_with_expiry(
                MemoryModuleType::AttackCoolingDown,
                MemoryValue::Bool(true),
                i64::from(self.cooldown_between_attacks),
            );
            true
        } else {
            false
        }
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![
            MemoryModuleType::LookTarget,
            MemoryModuleType::AttackTarget,
            MemoryModuleType::AttackCoolingDown,
            MemoryModuleType::NearestVisibleLivingEntities,
        ]
    }

    fn name(&self) -> &'static str {
        "MeleeAttack"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::{MemoryModuleType as T, NearestVisibleLivingEntities};
    use crate::brain::Brain;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    fn setup() -> (MobAi<TestMob>, BrainState) {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        cx.mob.others = vec![(EntityClass::Living, EntityFacts::living(3, 1.5, 1.0, 0.5))];
        cx.mob.melee_reach = vec![3];
        cx.mob.visible = vec![3];
        let mut brain = Brain::<TestMob>::new(
            &[
                T::LookTarget,
                T::AttackTarget,
                T::AttackCoolingDown,
                T::NearestVisibleLivingEntities,
            ],
            vec![],
        )
        .state;
        brain.set_memory_value(T::AttackTarget, MemoryValue::Entity(3));
        brain.set_memory_value(
            T::NearestVisibleLivingEntities,
            MemoryValue::NearestVisible(NearestVisibleLivingEntities::new(vec![3])),
        );
        (cx, brain)
    }

    #[test]
    fn hits_once_then_cools_down_for_the_given_ticks() {
        let (mut cx, mut brain) = setup();
        let mut b = MeleeAttack::<TestMob>::create(20);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(cx.mob.hurt, vec![3]);
        assert!(cx.mob.calls.iter().any(|c| c == "swing"));
        assert_eq!(
            brain
                .get_memory(T::LookTarget)
                .unwrap()
                .as_position_tracker(),
            Some(&PositionTracker::entity(3, true))
        );
        assert_eq!(brain.time_until_expiry(T::AttackCoolingDown), 20);
        assert!(!b.try_start(&mut brain, &mut cx, 1), "cooling down");
        for _ in 0..21 {
            brain.forget_outdated_memories();
        }
        assert!(
            b.try_start(&mut brain, &mut cx, 22),
            "the cooldown expired on the 21st tick"
        );
    }

    #[test]
    fn out_of_reach_or_invisible_does_not_swing() {
        let (mut cx, mut brain) = setup();
        cx.mob.melee_reach.clear();
        let mut b = MeleeAttack::<TestMob>::create(20);
        assert!(!b.try_start(&mut brain, &mut cx, 0));
        cx.mob.melee_reach = vec![3];
        brain.set_memory_value(
            T::NearestVisibleLivingEntities,
            MemoryValue::NearestVisible(NearestVisibleLivingEntities::new(vec![])),
        );
        assert!(!b.try_start(&mut brain, &mut cx, 0));
        assert!(cx.mob.hurt.is_empty());
    }
}
