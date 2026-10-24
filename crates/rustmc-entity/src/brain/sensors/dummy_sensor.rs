//! `net.minecraft.world.entity.ai.sensing.DummySensor`, from the 26.2 jar:
//! `Sensor()` (scan rate 20), `doTick` empty, `requires()` empty. It still
//! makes its `nextInt(20)` delay draw in `Brain.<init>` and publishes the
//! follow range on every scan like any other sensor.

use crate::ai::mob_ai::MobAi;
use crate::brain::memory::MemoryModuleType;
use crate::brain::sensor::{Sensor, SensorBody, SensorType};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `DummySensor`.
#[derive(Debug, Clone, Copy, Default)]
pub struct DummySensor;

impl DummySensor {
    /// `SensorType.DUMMY.create()`.
    #[must_use]
    pub fn new() -> Sensor<DummySensor> {
        Sensor::new(DummySensor)
    }
}

impl<M: MobHost> SensorBody<M> for DummySensor {
    fn do_tick(&mut self, _brain: &mut BrainState, _cx: &mut MobAi<M>) {}

    fn requires(&self) -> Vec<MemoryModuleType> {
        Vec::new()
    }

    fn sensor_type(&self) -> SensorType {
        SensorType::Dummy
    }
}
