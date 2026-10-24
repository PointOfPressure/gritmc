//! `net.minecraft.world.entity.ai.behavior.DismountOrSkipMounting`, from
//! the 26.2 jar. A `BehaviorBuilder` one-shot over
//! `group(registered(RIDE_TARGET))`.
//!
//! ```text
//! isVehicleValid(mob, e, d) (0..33): e.isAlive() && e.closerThan(mob, d) && e.level() == mob.level()
//! lambda$create$2 (0..83):
//!   vehicle = mob.getVehicle(); ride = rideTarget.tryGet().orElse(null)
//!   if vehicle == null && ride == null: return false
//!   e = vehicle == null ? ride : vehicle
//!   if !isVehicleValid(mob, e, maxDistance) || predicate.test(mob, e):
//!     mob.stopRiding(); rideTarget.erase(); return true
//!   return false
//! ```
//!
//! No random draw. `stopRiding` is called whether or not the mob is
//! riding — the "skip mounting" half of the name.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::memory::{MemoryModuleType, MemoryValue};
use crate::brain::BrainState;
use crate::host::MobHost;

/// A `BiPredicate<E, Entity>` over the mob and its vehicle or ride target.
pub type DismountPredicate<M> = Box<dyn FnMut(&mut MobAi<M>, i32) -> bool>;

/// `DismountOrSkipMounting`'s trigger.
pub struct DismountOrSkipMounting<M: MobHost> {
    pub max_distance_from_vehicle: i32,
    pub predicate: DismountPredicate<M>,
}

impl<M: MobHost> DismountOrSkipMounting<M> {
    /// `create(int, BiPredicate<E, Entity>)`.
    #[must_use]
    pub fn create(
        max_distance_from_vehicle: i32,
        predicate: DismountPredicate<M>,
    ) -> OneShot<Self> {
        OneShot::new(DismountOrSkipMounting {
            max_distance_from_vehicle,
            predicate,
        })
    }

    /// `isVehicleValid(mob, entity, maxDistance)`.
    fn is_vehicle_valid(&self, cx: &mut MobAi<M>, e: i32) -> bool {
        cx.mob.entity(e).is_some_and(|f| f.alive)
            && cx
                .mob
                .entity_closer_than(e, f64::from(self.max_distance_from_vehicle))
            && cx.mob.entity_in_same_level(e)
    }
}

impl<M: MobHost> Trigger<M> for DismountOrSkipMounting<M> {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        let Some(ride) = brain.memory_internal(MemoryModuleType::RideTarget) else {
            return false;
        };
        let ride = ride.and_then(MemoryValue::as_entity);
        let vehicle = cx.mob.vehicle();
        let Some(e) = vehicle.or(ride) else {
            return false;
        };
        if !self.is_vehicle_valid(cx, e) || (self.predicate)(cx, e) {
            cx.mob.stop_riding();
            brain.erase_memory(MemoryModuleType::RideTarget);
            return true;
        }
        false
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![MemoryModuleType::RideTarget]
    }

    fn name(&self) -> &'static str {
        "DismountOrSkipMounting"
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
    fn dismounts_a_dead_far_or_refused_vehicle_and_forgets_the_target() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        cx.mob.others = vec![(EntityClass::Living, EntityFacts::living(7, 3.5, 1.0, 0.5))];
        let mut brain = Brain::<TestMob>::new(&[T::RideTarget], vec![]).state;
        let mut b = DismountOrSkipMounting::create(8, Box::new(|_cx, _e| false));
        assert!(!b.try_start(&mut brain, &mut cx, 0), "nothing to dismount");
        brain.set_memory_value(T::RideTarget, MemoryValue::Entity(7));
        assert!(
            !b.try_start(&mut brain, &mut cx, 0),
            "a valid target is kept"
        );
        cx.mob.others[0].1.x = 12.5;
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(!brain.has_memory_value(T::RideTarget));
        assert_eq!(cx.mob.calls, ["stop_riding"]);
        // The vehicle wins over the memory, and the predicate can refuse it.
        cx.mob.calls.clear();
        cx.mob.others[0].1.x = 3.5;
        cx.mob.vehicle = Some(7);
        let mut b = DismountOrSkipMounting::create(8, Box::new(|_cx, e| e == 7));
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(cx.mob.vehicle, None);
        cx.mob.others[0].1.alive = false;
        cx.mob.vehicle = Some(7);
        let mut b = DismountOrSkipMounting::create(8, Box::new(|_cx, _e| false));
        assert!(b.try_start(&mut brain, &mut cx, 0), "a dead vehicle");
    }
}
