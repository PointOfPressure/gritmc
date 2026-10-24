//! `net.minecraft.world.entity.ai.sensing.SecondaryPoiSensor`, from the
//! 26.2 jar. `Sensor(40)`; `requires()` = `{SECONDARY_JOB_SITE}`.
//!
//! ```text
//! doTick(level, Villager) (0..164):
//!   dim = level.dimension(); pos = villager.blockPosition(); list = []
//!   for dx in -4..=4: for dy in -2..=2: for dz in -4..=4:
//!     p = pos.offset(dx, dy, dz)
//!     if profession.secondaryPoi().contains(level.getBlockState(p).getBlock()): list.add(GlobalPos.of(dim, p))
//!   if !list.isEmpty(): brain.setMemory(SECONDARY_JOB_SITE, list) else brain.eraseMemory(SECONDARY_JOB_SITE)
//! ```
//!
//! No random draw. The profession is re-read for every one of the 405
//! positions; it cannot change mid-tick, so one read stands in.

use crate::ai::mob_ai::MobAi;
use crate::brain::memory::{GlobalPos, MemoryModuleType, MemoryValue};
use crate::brain::sensor::{Sensor, SensorBody, SensorType};
use crate::brain::BrainState;
use crate::host::MobHost;
use crate::phys::offset;

/// `SCAN_RATE`.
pub const SCAN_RATE: i32 = 40;

/// `SecondaryPoiSensor`.
#[derive(Debug, Clone, Copy, Default)]
pub struct SecondaryPoiSensor;

impl SecondaryPoiSensor {
    /// `SensorType.SECONDARY_POIS.create()`.
    #[must_use]
    pub fn new() -> Sensor<SecondaryPoiSensor> {
        Sensor::with_scan_rate(SecondaryPoiSensor, SCAN_RATE)
    }
}

impl<M: MobHost> SensorBody<M> for SecondaryPoiSensor {
    fn do_tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) {
        let dimension = cx.mob.dimension();
        let pos = cx.mob.block_position();
        let mut list: Vec<GlobalPos> = Vec::new();
        for dx in -4..=4 {
            for dy in -2..=2 {
                for dz in -4..=4 {
                    let p = offset(pos, dx, dy, dz);
                    if cx.mob.is_secondary_poi_block_at(p) {
                        list.push(GlobalPos { dimension, pos: p });
                    }
                }
            }
        }
        if list.is_empty() {
            brain.erase_memory(MemoryModuleType::SecondaryJobSite);
        } else {
            brain.set_memory_value(
                MemoryModuleType::SecondaryJobSite,
                MemoryValue::GlobalPosList(list),
            );
        }
    }

    fn requires(&self) -> Vec<MemoryModuleType> {
        vec![MemoryModuleType::SecondaryJobSite]
    }

    fn sensor_type(&self) -> SensorType {
        SensorType::SecondaryPois
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};
    use rustmc_data::{BlockPos, Dimension};

    #[test]
    fn lists_secondary_blocks_in_x_y_z_scan_order_within_4_2_4() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        cx.mob.secondary_poi_blocks = vec![
            BlockPos { x: 4, y: 3, z: -4 },
            BlockPos { x: -4, y: -1, z: 4 },
            BlockPos { x: 0, y: 4, z: 0 },
            BlockPos { x: 5, y: 1, z: 0 },
            BlockPos { x: -4, y: -1, z: 3 },
        ];
        let mut brain = Brain::<TestMob>::new(&[T::SecondaryJobSite], vec![]).state;
        let mut s = SecondaryPoiSensor;
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        let gp = |x, y, z| GlobalPos {
            dimension: Dimension::Overworld,
            pos: BlockPos { x, y, z },
        };
        assert_eq!(
            brain.get_memory(T::SecondaryJobSite),
            Some(&MemoryValue::GlobalPosList(vec![
                gp(-4, -1, 3),
                gp(-4, -1, 4),
                gp(4, 3, -4)
            ])),
            "y 4 and x 5 are out of the box"
        );
        cx.mob.secondary_poi_blocks.clear();
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert!(!brain.has_memory_value(T::SecondaryJobSite));
    }
}
