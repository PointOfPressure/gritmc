//! `net.minecraft.world.entity.ai.behavior.SetWalkTargetFromAttackTargetIfTargetOutOfReach`,
//! from the 26.2 jar. A `BehaviorBuilder` one-shot over
//! `group(registered(WALK_TARGET), registered(LOOK_TARGET), present(ATTACK_TARGET),
//! registered(NEAREST_VISIBLE_LIVING_ENTITIES))` (`lambda$create$3`, 0..112):
//!
//! ```text
//! target = attack; visible = nearestVisible.tryGet()
//! if visible.isPresent() && visible.get().contains(target) && BehaviorUtils.isWithinAttackRange(mob, target, 1):
//!     walk.erase()
//! else:
//!     look.set(new EntityTracker(target, true))
//!     walk.set(new WalkTarget(new EntityTracker(target, false), speed.apply(mob), 0))
//! return true
//! ```
//!
//! `PROJECTILE_ATTACK_RANGE_BUFFER = 1`. `create(float)` is `create(e -> speed)`.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::behaviors::behavior_utils::is_within_attack_range;
use crate::brain::behaviors::nearest_visible_contains;
use crate::brain::behaviors::MobSpeed;
use crate::brain::memory::{MemoryModuleType, MemoryValue, PositionTracker, WalkTarget};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `PROJECTILE_ATTACK_RANGE_BUFFER`.
pub const PROJECTILE_ATTACK_RANGE_BUFFER: i32 = 1;

/// `SetWalkTargetFromAttackTargetIfTargetOutOfReach`'s trigger.
pub struct SetWalkTargetFromAttackTargetIfTargetOutOfReach<M: MobHost> {
    /// `Function<LivingEntity, Float>`.
    pub speed: MobSpeed<M>,
}

impl<M: MobHost + 'static> SetWalkTargetFromAttackTargetIfTargetOutOfReach<M> {
    /// `create(float)`.
    #[must_use]
    pub fn create(speed: f32) -> OneShot<Self> {
        Self::create_with(Box::new(move |_| speed))
    }

    /// `create(Function)`.
    #[must_use]
    pub fn create_with(speed: MobSpeed<M>) -> OneShot<Self> {
        OneShot::new(SetWalkTargetFromAttackTargetIfTargetOutOfReach { speed })
    }
}

impl<M: MobHost> Trigger<M> for SetWalkTargetFromAttackTargetIfTargetOutOfReach<M> {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        if brain
            .memory_internal(MemoryModuleType::WalkTarget)
            .is_none()
            || brain
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
        let Some(visible) = brain.memory_internal(MemoryModuleType::NearestVisibleLivingEntities)
        else {
            return false;
        };
        let visible_present = visible.is_some();
        if visible_present
            && nearest_visible_contains(brain, cx, target)
            && is_within_attack_range(cx, target, PROJECTILE_ATTACK_RANGE_BUFFER)
        {
            brain.erase_memory(MemoryModuleType::WalkTarget);
        } else {
            brain.set_memory_value(
                MemoryModuleType::LookTarget,
                MemoryValue::PositionTracker(PositionTracker::entity(target, true)),
            );
            let speed = (self.speed)(cx);
            brain.set_memory_value(
                MemoryModuleType::WalkTarget,
                MemoryValue::WalkTarget(WalkTarget::new(
                    PositionTracker::entity(target, false),
                    speed,
                    0,
                )),
            );
        }
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![
            MemoryModuleType::WalkTarget,
            MemoryModuleType::LookTarget,
            MemoryModuleType::AttackTarget,
            MemoryModuleType::NearestVisibleLivingEntities,
        ]
    }

    fn name(&self) -> &'static str {
        "SetWalkTargetFromAttackTargetIfTargetOutOfReach"
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
        cx.mob.others = vec![(EntityClass::Living, EntityFacts::living(3, 6.5, 1.0, 0.5))];
        cx.mob.visible = vec![3];
        let mut brain = Brain::<TestMob>::new(
            &[
                T::WalkTarget,
                T::LookTarget,
                T::AttackTarget,
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
    fn walks_and_looks_at_an_out_of_reach_target() {
        let (mut cx, mut brain) = setup();
        let mut b = SetWalkTargetFromAttackTargetIfTargetOutOfReach::<TestMob>::create(1.2);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(
            brain.get_memory(T::WalkTarget).unwrap().as_walk_target(),
            Some(&WalkTarget::new(PositionTracker::entity(3, false), 1.2, 0))
        );
        assert_eq!(
            brain
                .get_memory(T::LookTarget)
                .unwrap()
                .as_position_tracker(),
            Some(&PositionTracker::entity(3, true))
        );
    }

    #[test]
    fn erases_the_walk_target_when_visible_and_in_melee_reach() {
        let (mut cx, mut brain) = setup();
        cx.mob.melee_reach = vec![3];
        brain.set_memory_value(
            T::WalkTarget,
            MemoryValue::WalkTarget(WalkTarget::new(PositionTracker::entity(3, false), 1.0, 0)),
        );
        let mut b = SetWalkTargetFromAttackTargetIfTargetOutOfReach::<TestMob>::create(1.2);
        assert!(b.try_start(&mut brain, &mut cx, 0), "always true");
        assert!(!brain.has_memory_value(T::WalkTarget));
        assert!(!brain.has_memory_value(T::LookTarget));
    }
}
