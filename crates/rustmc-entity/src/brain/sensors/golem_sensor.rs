//! `net.minecraft.world.entity.ai.sensing.GolemSensor`, from the 26.2 jar.
//! `GolemSensor()` is `Sensor(200)`; `requires()` =
//! `{NEAREST_LIVING_ENTITIES, GOLEM_DETECTED_RECENTLY}`.
//! `GOLEM_SCAN_RATE = 200`, `MEMORY_TIME_TO_LIVE = 599`.
//!
//! ```text
//! doTick: checkForNearbyGolem(mob)
//! checkForNearbyGolem (0..50):
//!   list = brain.getMemory(NEAREST_LIVING_ENTITIES); if empty: return
//!   if list.stream().anyMatch(e -> e.is(IRON_GOLEM)): golemDetected(mob)
//! golemDetected (0..17): brain.setMemoryWithExpiry(GOLEM_DETECTED_RECENTLY, true, 599L)
//! ```
//!
//! No random draw. `NEAREST_LIVING_ENTITIES` is the unfiltered sensed
//! list: an unseen golem still counts.

use crate::ai::mob_ai::MobAi;
use crate::brain::memory::{MemoryModuleType, MemoryValue};
use crate::brain::sensor::{Sensor, SensorBody, SensorType};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `GOLEM_SCAN_RATE`.
pub const GOLEM_SCAN_RATE: i32 = 200;
/// `MEMORY_TIME_TO_LIVE`.
pub const MEMORY_TIME_TO_LIVE: i64 = 599;

/// `GolemSensor`.
#[derive(Debug, Clone, Copy, Default)]
pub struct GolemSensor;

impl GolemSensor {
    /// `SensorType.GOLEM_DETECTED.create()`.
    #[must_use]
    pub fn new() -> Sensor<GolemSensor> {
        Sensor::with_scan_rate(GolemSensor, GOLEM_SCAN_RATE)
    }

    /// `checkForNearbyGolem(mob)`.
    pub fn check_for_nearby_golem<M: MobHost>(brain: &mut BrainState, cx: &MobAi<M>) {
        let Some(list) = brain
            .get_memory(MemoryModuleType::NearestLivingEntities)
            .and_then(MemoryValue::as_entities)
        else {
            return;
        };
        if list
            .iter()
            .any(|id| cx.mob.entity_kind_is(*id, "iron_golem"))
        {
            Self::golem_detected(brain);
        }
    }

    /// `golemDetected(mob)`.
    pub fn golem_detected(brain: &mut BrainState) {
        brain.set_memory_with_expiry(
            MemoryModuleType::GolemDetectedRecently,
            MemoryValue::Bool(true),
            MEMORY_TIME_TO_LIVE,
        );
    }
}

impl<M: MobHost> SensorBody<M> for GolemSensor {
    fn do_tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) {
        Self::check_for_nearby_golem(brain, cx);
    }

    fn requires(&self) -> Vec<MemoryModuleType> {
        vec![
            MemoryModuleType::NearestLivingEntities,
            MemoryModuleType::GolemDetectedRecently,
        ]
    }

    fn sensor_type(&self) -> SensorType {
        SensorType::GolemDetected
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn any_sensed_iron_golem_sets_the_599_tick_flag() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        cx.mob.entity_kinds = vec![(2, "villager"), (3, "iron_golem")];
        let mut brain = Brain::<TestMob>::new(
            &[T::NearestLivingEntities, T::GolemDetectedRecently],
            vec![],
        )
        .state;
        let mut s = GolemSensor;
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert!(!brain.has_memory_value(T::GolemDetectedRecently), "no list");
        brain.set_memory_value(T::NearestLivingEntities, MemoryValue::Entities(vec![2]));
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert!(!brain.has_memory_value(T::GolemDetectedRecently));
        brain.set_memory_value(T::NearestLivingEntities, MemoryValue::Entities(vec![2, 3]));
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert_eq!(
            brain.get_memory(T::GolemDetectedRecently),
            Some(&MemoryValue::Bool(true))
        );
        assert_eq!(brain.time_until_expiry(T::GolemDetectedRecently), 599);
        assert_eq!(GolemSensor::new().time_to_tick(), 0);
    }
}
