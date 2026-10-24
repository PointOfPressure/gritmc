//! `net.minecraft.world.entity.ai.behavior.BackUpIfTooClose`, from the 26.2
//! jar. A `BehaviorBuilder` one-shot over `group(absent(WALK_TARGET),
//! registered(LOOK_TARGET), present(ATTACK_TARGET),
//! present(NEAREST_VISIBLE_LIVING_ENTITIES))`, for `Mob`.
//!
//! ```text
//! lambda$create$2 (0..87):
//!   target = attackTarget
//!   if target.closerThan(mob, tooCloseDistance) && nearestVisible.contains(target):
//!     look.set(EntityTracker(target, true))
//!     mob.getMoveControl().strafe(-strafeSpeed, 0.0f)
//!     mob.setYRot(Mth.rotateIfNecessary(mob.getYRot(), mob.yHeadRot, 0.0f))
//!     return true
//!   return false
//! ```
//!
//! No random draw. `contains` fills the visibility memo for the target.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::behaviors::nearest_visible_contains;
use crate::brain::memory::{MemoryModuleType, MemoryValue, PositionTracker};
use crate::brain::BrainState;
use crate::host::MobHost;
use crate::mth::rotate_if_necessary;

/// `BackUpIfTooClose`'s trigger.
#[derive(Debug, Clone, Copy)]
pub struct BackUpIfTooClose {
    pub too_close_distance: i32,
    pub strafe_speed: f32,
}

impl BackUpIfTooClose {
    /// `create(int, float)`.
    #[must_use]
    pub fn create(too_close_distance: i32, strafe_speed: f32) -> OneShot<Self> {
        OneShot::new(BackUpIfTooClose {
            too_close_distance,
            strafe_speed,
        })
    }
}

impl<M: MobHost> Trigger<M> for BackUpIfTooClose {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        if brain.memory_internal(MemoryModuleType::WalkTarget) != Some(None) {
            return false;
        }
        if brain
            .memory_internal(MemoryModuleType::LookTarget)
            .is_none()
        {
            return false;
        }
        let Some(Some(v)) = brain.memory_internal(MemoryModuleType::AttackTarget) else {
            return false;
        };
        let Some(target) = v.as_entity() else {
            return false;
        };
        let Some(Some(_)) = brain.memory_internal(MemoryModuleType::NearestVisibleLivingEntities)
        else {
            return false;
        };
        if cx
            .mob
            .entity_closer_than(target, f64::from(self.too_close_distance))
            && nearest_visible_contains(brain, cx, target)
        {
            brain.set_memory_value(
                MemoryModuleType::LookTarget,
                MemoryValue::PositionTracker(PositionTracker::entity(target, true)),
            );
            cx.move_control.strafe(-self.strafe_speed, 0.0);
            let y = rotate_if_necessary(cx.mob.y_rot(), cx.mob.y_head_rot(), 0.0);
            cx.mob.set_y_rot(y);
            return true;
        }
        false
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
        "BackUpIfTooClose"
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

    #[test]
    fn strafes_back_from_a_close_visible_target_and_squares_the_body() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        cx.mob.others = vec![(EntityClass::Living, EntityFacts::living(7, 3.5, 1.0, 0.5))];
        cx.mob.visible = vec![7];
        cx.mob.y_rot = 10.0;
        cx.mob.y_head_rot = 90.0;
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
        brain.set_memory_value(T::AttackTarget, MemoryValue::Entity(7));
        brain.set_memory_value(
            T::NearestVisibleLivingEntities,
            MemoryValue::NearestVisible(NearestVisibleLivingEntities::new(vec![7])),
        );
        let mut b = BackUpIfTooClose::create(5, 0.75);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(
            cx.mob.y_rot, 90.0,
            "rotateIfNecessary(10, 90, 0) snaps to the head"
        );
        assert_eq!(
            brain
                .get_memory(T::LookTarget)
                .unwrap()
                .as_position_tracker(),
            Some(&PositionTracker::entity(7, true))
        );
        cx.mob.others[0].1.x = 8.5;
        assert!(
            !b.try_start(&mut brain, &mut cx, 0),
            "5 blocks away is not closer than 5"
        );
        cx.mob.others[0].1.x = 3.5;
        brain.set_memory_value(
            T::NearestVisibleLivingEntities,
            MemoryValue::NearestVisible(NearestVisibleLivingEntities::new(vec![])),
        );
        assert!(
            !b.try_start(&mut brain, &mut cx, 0),
            "not in the visible list"
        );
    }
}
