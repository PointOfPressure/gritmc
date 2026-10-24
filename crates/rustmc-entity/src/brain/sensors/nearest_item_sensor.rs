//! `net.minecraft.world.entity.ai.sensing.NearestItemSensor`, from the
//! 26.2 jar. `Sensor()` (scan rate 20); `requires()` =
//! `{NEAREST_VISIBLE_WANTED_ITEM}`. `XZ_RANGE = 32`, `Y_RANGE = 16`,
//! `MAX_DISTANCE_TO_WANTED_ITEM = 32`.
//!
//! ```text
//! doTick(level, Mob) (0..117):
//!   list = level.getEntitiesOfClass(ItemEntity.class, mob.getBoundingBox().inflate(32, 16, 32), e -> true)
//!   list.sort(comparingDouble(mob::distanceToSqr))                       <- stable
//!   first = list.stream().filter(e -> mob.wantsToPickUp(level, e.getItem()))
//!                        .filter(e -> e.closerThan(mob, 32.0))
//!                        .filter(mob::hasLineOfSight).findFirst()
//!   brain.setMemory(NEAREST_VISIBLE_WANTED_ITEM, first)
//! ```
//!
//! No random draw. The three filters run per element in that order, so a
//! rejected item never has its line of sight computed.

use crate::ai::mob_ai::MobAi;
use crate::brain::memory::{MemoryModuleType, MemoryValue};
use crate::brain::sensor::{Sensor, SensorBody, SensorType};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `XZ_RANGE`.
pub const XZ_RANGE: f64 = 32.0;
/// `Y_RANGE`.
pub const Y_RANGE: f64 = 16.0;
/// `MAX_DISTANCE_TO_WANTED_ITEM`.
pub const MAX_DISTANCE_TO_WANTED_ITEM: i32 = 32;

/// `NearestItemSensor`.
#[derive(Debug, Clone, Copy, Default)]
pub struct NearestItemSensor;

impl NearestItemSensor {
    /// `SensorType.NEAREST_ITEMS.create()`.
    #[must_use]
    pub fn new() -> Sensor<NearestItemSensor> {
        Sensor::new(NearestItemSensor)
    }
}

impl<M: MobHost> SensorBody<M> for NearestItemSensor {
    fn do_tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) {
        let aabb = cx.mob.bounding_box().inflate(XZ_RANGE, Y_RANGE, XZ_RANGE);
        let mut list: Vec<(i32, f64)> = cx
            .mob
            .item_entities_in(&aabb)
            .into_iter()
            .map(|id| {
                (
                    id,
                    cx.mob.distance_to_sqr_entity(id).unwrap_or(f64::INFINITY),
                )
            })
            .collect();
        list.sort_by(|a, b| a.1.total_cmp(&b.1));
        let max = f64::from(MAX_DISTANCE_TO_WANTED_ITEM);
        let first = list.into_iter().map(|(id, _)| id).find(|id| {
            cx.mob.wants_to_pick_up(*id)
                && cx.mob.entity_closer_than(*id, max)
                && cx.mob.has_line_of_sight_to(*id)
        });
        brain.set_memory(
            MemoryModuleType::NearestVisibleWantedItem,
            first.map(MemoryValue::Entity),
        );
    }

    fn requires(&self) -> Vec<MemoryModuleType> {
        vec![MemoryModuleType::NearestVisibleWantedItem]
    }

    fn sensor_type(&self) -> SensorType {
        SensorType::NearestItems
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn the_closest_wanted_visible_item_within_32_wins() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        cx.mob.others = vec![
            (EntityClass::Item, EntityFacts::living(10, 20.5, 1.0, 0.5)),
            (EntityClass::Item, EntityFacts::living(11, 4.5, 1.0, 0.5)),
            (EntityClass::Item, EntityFacts::living(12, 2.5, 1.0, 0.5)),
            (EntityClass::Item, EntityFacts::living(13, 40.5, 1.0, 0.5)),
        ];
        cx.mob.wanted_items = vec![10, 11, 13];
        cx.mob.visible = vec![10, 13];
        let mut brain = Brain::<TestMob>::new(&[T::NearestVisibleWantedItem], vec![]).state;
        let mut s = NearestItemSensor;
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert_eq!(
            brain.get_memory(T::NearestVisibleWantedItem),
            Some(&MemoryValue::Entity(10)),
            "12 is unwanted, 11 unseen, 13 beyond 32"
        );
        cx.mob.visible.clear();
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert!(!brain.has_memory_value(T::NearestVisibleWantedItem));
    }
}
