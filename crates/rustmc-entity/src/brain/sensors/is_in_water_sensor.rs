//! `net.minecraft.world.entity.ai.sensing.IsInWaterSensor`, from the 26.2
//! jar. `Sensor()` (scan rate 20); `requires()` = `{IS_IN_WATER}`.
//!
//! ```text
//! doTick (0..33):  if mob.isInWater(): brain.setMemory(IS_IN_WATER, Unit.INSTANCE)
//!                  else:               brain.eraseMemory(IS_IN_WATER)
//! ```
//!
//! The whole class. The memory is a `Unit`, so only its presence carries
//! anything, and the else branch is an erase rather than a `false`.

use crate::ai::mob_ai::MobAi;
use crate::brain::memory::{MemoryModuleType, MemoryValue};
use crate::brain::sensor::{Sensor, SensorBody, SensorType};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `IsInWaterSensor`.
#[derive(Debug, Clone, Copy, Default)]
pub struct IsInWaterSensor;

impl IsInWaterSensor {
    /// `SensorType.IS_IN_WATER.create()`.
    #[must_use]
    pub fn new() -> Sensor<IsInWaterSensor> {
        Sensor::new(IsInWaterSensor)
    }
}

impl<M: MobHost> SensorBody<M> for IsInWaterSensor {
    fn do_tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) {
        if cx.mob.is_in_water() {
            brain.set_memory_value(MemoryModuleType::IsInWater, MemoryValue::Unit);
        } else {
            brain.erase_memory(MemoryModuleType::IsInWater);
        }
    }

    fn requires(&self) -> Vec<MemoryModuleType> {
        vec![MemoryModuleType::IsInWater]
    }

    fn sensor_type(&self) -> SensorType {
        SensorType::IsInWater
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn sets_the_unit_in_water_and_erases_out_of_it() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut brain = Brain::<TestMob>::new(&[T::IsInWater], vec![]).state;
        let mut s = IsInWaterSensor;
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert!(!brain.has_memory_value(T::IsInWater));
        cx.mob.in_water = true;
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert_eq!(brain.get_memory(T::IsInWater), Some(&MemoryValue::Unit));
        cx.mob.in_water = false;
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert!(
            !brain.has_memory_value(T::IsInWater),
            "the else is an erase"
        );
    }
}
