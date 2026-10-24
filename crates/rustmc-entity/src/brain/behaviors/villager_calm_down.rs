//! `net.minecraft.world.entity.ai.behavior.VillagerCalmDown`, from the 26.2
//! jar. A `BehaviorBuilder` one-shot over `group(registered(HURT_BY),
//! registered(HURT_BY_ENTITY), registered(NEAREST_HOSTILE))`.
//! `SAFE_DISTANCE_FROM_DANGER = 36` (squared blocks).
//!
//! ```text
//! lambda$create$2 (0..87):
//!   danger = hurtBy.present || hurtByEntity.present
//!            || nearestHostile.filter(h -> h.distanceToSqr(mob) <= 36.0).present
//!   if !danger:
//!     hurtBy.erase(); nearestHostile.erase()                        <- HURT_BY_ENTITY is left alone
//!     brain.updateActivityFromSchedule(level.environmentAttributes(), gameTime, mob.position())
//!   return true
//! ```
//!
//! No random draw. The three tests short-circuit left to right, so the
//! distance is only computed when neither hurt memory is present.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::memory::{MemoryModuleType, MemoryValue};
use crate::brain::BrainState;
use crate::host::{MobHost, PathLevel};

/// `SAFE_DISTANCE_FROM_DANGER`.
pub const SAFE_DISTANCE_FROM_DANGER: i32 = 36;

/// `VillagerCalmDown`'s trigger.
#[derive(Debug, Clone, Copy, Default)]
pub struct VillagerCalmDown;

impl VillagerCalmDown {
    /// `create()`.
    #[must_use]
    pub fn create() -> OneShot<Self> {
        OneShot::new(VillagerCalmDown)
    }
}

impl<M: MobHost> Trigger<M> for VillagerCalmDown {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        let Some(hurt_by) = brain.memory_internal(MemoryModuleType::HurtBy) else {
            return false;
        };
        let hurt_by = hurt_by.is_some();
        let Some(hurt_by_entity) = brain.memory_internal(MemoryModuleType::HurtByEntity) else {
            return false;
        };
        let hurt_by_entity = hurt_by_entity.is_some();
        let Some(hostile) = brain.memory_internal(MemoryModuleType::NearestHostile) else {
            return false;
        };
        let hostile = hostile.and_then(MemoryValue::as_entity);
        let danger = hurt_by
            || hurt_by_entity
            || hostile.is_some_and(|h| {
                cx.mob
                    .distance_to_sqr_entity(h)
                    .is_some_and(|d| d <= f64::from(SAFE_DISTANCE_FROM_DANGER))
            });
        if !danger {
            brain.erase_memory(MemoryModuleType::HurtBy);
            brain.erase_memory(MemoryModuleType::NearestHostile);
            let game_time = cx.mob.level().game_time();
            brain.update_activity_from_schedule(&cx.mob, game_time);
        }
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![
            MemoryModuleType::HurtBy,
            MemoryModuleType::HurtByEntity,
            MemoryModuleType::NearestHostile,
        ]
    }

    fn name(&self) -> &'static str {
        "VillagerCalmDown"
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
    fn calms_once_the_hostile_is_past_six_blocks_and_nothing_hurt_it() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        cx.mob.others = vec![(EntityClass::Living, EntityFacts::living(7, 6.5, 1.0, 0.5))];
        let mut brain =
            Brain::<TestMob>::new(&[T::HurtBy, T::HurtByEntity, T::NearestHostile], vec![]).state;
        brain.set_memory_value(T::NearestHostile, MemoryValue::Entity(7));
        brain.set_memory_value(T::HurtByEntity, MemoryValue::Entity(7));
        let mut b = VillagerCalmDown::create();
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(
            brain.has_memory_value(T::NearestHostile),
            "hurt by an entity: still danger"
        );
        brain.erase_memory(T::HurtByEntity);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(
            brain.has_memory_value(T::NearestHostile),
            "exactly 36 sq is still danger"
        );
        cx.mob.others[0].1.x = 6.6;
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(!brain.has_memory_value(T::NearestHostile));
        assert!(!brain.has_memory_value(T::HurtBy));
    }
}
