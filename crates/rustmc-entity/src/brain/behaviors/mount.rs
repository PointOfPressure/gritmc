//! `net.minecraft.world.entity.ai.behavior.Mount`, from the 26.2 jar. A
//! `BehaviorBuilder` one-shot over `group(registered(LOOK_TARGET),
//! absent(WALK_TARGET), present(RIDE_TARGET))`.
//! `CLOSE_ENOUGH_TO_START_RIDING_DIST = 1`.
//!
//! ```text
//! lambda$create$2 (0..81):
//!   if mob.isPassenger(): return false
//!   entity = rideTarget
//!   if entity.closerThan(mob, 1.0): mob.startRiding(entity)          <- result popped
//!   else: look.set(EntityTracker(entity, true)); walk.set(WalkTarget(EntityTracker(entity, false), speed, 1))
//!   return true
//! ```
//!
//! No random draw.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::memory::{MemoryModuleType, MemoryValue, PositionTracker, WalkTarget};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `CLOSE_ENOUGH_TO_START_RIDING_DIST`.
pub const CLOSE_ENOUGH_TO_START_RIDING_DIST: i32 = 1;

/// `Mount`'s trigger.
#[derive(Debug, Clone, Copy)]
pub struct Mount {
    pub speed_modifier: f32,
}

impl Mount {
    /// `create(float)`.
    #[must_use]
    pub fn create(speed_modifier: f32) -> OneShot<Self> {
        OneShot::new(Mount { speed_modifier })
    }
}

impl<M: MobHost> Trigger<M> for Mount {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        if brain
            .memory_internal(MemoryModuleType::LookTarget)
            .is_none()
        {
            return false;
        }
        if brain.memory_internal(MemoryModuleType::WalkTarget) != Some(None) {
            return false;
        }
        let Some(Some(v)) = brain.memory_internal(MemoryModuleType::RideTarget) else {
            return false;
        };
        let Some(entity) = v.as_entity() else {
            return false;
        };
        if cx.mob.is_passenger() {
            return false;
        }
        if cx.mob.entity_closer_than(entity, 1.0) {
            cx.mob.start_riding(entity);
        } else {
            brain.set_memory_value(
                MemoryModuleType::LookTarget,
                MemoryValue::PositionTracker(PositionTracker::entity(entity, true)),
            );
            brain.set_memory_value(
                MemoryModuleType::WalkTarget,
                MemoryValue::WalkTarget(WalkTarget::new(
                    PositionTracker::entity(entity, false),
                    self.speed_modifier,
                    CLOSE_ENOUGH_TO_START_RIDING_DIST,
                )),
            );
        }
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![
            MemoryModuleType::LookTarget,
            MemoryModuleType::WalkTarget,
            MemoryModuleType::RideTarget,
        ]
    }

    fn name(&self) -> &'static str {
        "Mount"
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
    fn walks_to_a_far_mount_and_mounts_a_near_one() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        cx.mob.others = vec![(EntityClass::Living, EntityFacts::living(7, 4.5, 1.0, 0.5))];
        let mut brain =
            Brain::<TestMob>::new(&[T::LookTarget, T::WalkTarget, T::RideTarget], vec![]).state;
        let mut b = Mount::create(1.2);
        assert!(!b.try_start(&mut brain, &mut cx, 0), "no ride target");
        brain.set_memory_value(T::RideTarget, MemoryValue::Entity(7));
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(
            brain.get_memory(T::WalkTarget).unwrap().as_walk_target(),
            Some(&WalkTarget::new(PositionTracker::entity(7, false), 1.2, 1))
        );
        assert_eq!(
            brain
                .get_memory(T::LookTarget)
                .unwrap()
                .as_position_tracker(),
            Some(&PositionTracker::entity(7, true))
        );
        assert!(cx.mob.calls.is_empty());
        brain.erase_memory(T::WalkTarget);
        cx.mob.others[0].1.x = 1.4;
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(cx.mob.calls, ["start_riding 7"]);
        assert!(!brain.has_memory_value(T::WalkTarget));
        cx.mob.is_passenger = true;
        assert!(!b.try_start(&mut brain, &mut cx, 0), "already riding");
    }
}
