//! `net.minecraft.world.entity.ai.sensing.NearestLivingEntitySensor`, from
//! the 26.2 jar. `Sensor()` (scan rate 20); `requires()` =
//! `{NEAREST_LIVING_ENTITIES, NEAREST_VISIBLE_LIVING_ENTITIES}`.
//!
//! ```text
//! doTick (0..92):
//!   r = mob.getAttributeValue(FOLLOW_RANGE)
//!   list = level.getEntitiesOfClass(LivingEntity.class, mob.getBoundingBox().inflate(r, r, r), e -> e != mob && e.isAlive())
//!   list.sort(comparingDouble(mob::distanceToSqr))                <- stable; the query order breaks ties
//!   brain.setMemory(NEAREST_LIVING_ENTITIES, list)                 <- an empty list CLEARS
//!   brain.setMemory(NEAREST_VISIBLE_LIVING_ENTITIES, new NearestVisibleLivingEntities(level, mob, list))
//! ```
//!
//! The query order is the host's (`lib.rs` records that this crate's
//! entity query order is not vanilla's section-then-insertion order);
//! two entities at exactly equal distance keep it.

use crate::ai::mob_ai::MobAi;
use crate::brain::memory::{MemoryModuleType, MemoryValue, NearestVisibleLivingEntities};
use crate::brain::sensor::{Sensor, SensorBody, SensorType};
use crate::brain::BrainState;
use crate::host::{EntityClass, MobHost};

/// `NearestLivingEntitySensor<T>`.
#[derive(Debug, Clone, Copy, Default)]
pub struct NearestLivingEntitySensor;

impl NearestLivingEntitySensor {
    /// `SensorType.NEAREST_LIVING_ENTITIES.create()`.
    #[must_use]
    pub fn new() -> Sensor<NearestLivingEntitySensor> {
        Sensor::new(NearestLivingEntitySensor)
    }
}

impl<M: MobHost> SensorBody<M> for NearestLivingEntitySensor {
    fn do_tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) {
        let r = cx.mob.follow_range();
        let aabb = cx.mob.bounding_box().inflate(r, r, r);
        let me = cx.mob.id();
        let mut list: Vec<(i32, f64)> = cx
            .mob
            .entities_of_class_in(EntityClass::Living, &aabb)
            .into_iter()
            .filter(|id| *id != me && cx.mob.entity(*id).is_some_and(|e| e.alive))
            .map(|id| {
                (
                    id,
                    cx.mob.distance_to_sqr_entity(id).unwrap_or(f64::INFINITY),
                )
            })
            .collect();
        list.sort_by(|a, b| a.1.total_cmp(&b.1));
        let ids: Vec<i32> = list.into_iter().map(|(id, _)| id).collect();
        brain.set_memory_value(
            MemoryModuleType::NearestLivingEntities,
            MemoryValue::Entities(ids.clone()),
        );
        brain.set_memory_value(
            MemoryModuleType::NearestVisibleLivingEntities,
            MemoryValue::NearestVisible(NearestVisibleLivingEntities::new(ids)),
        );
    }

    fn requires(&self) -> Vec<MemoryModuleType> {
        vec![
            MemoryModuleType::NearestLivingEntities,
            MemoryModuleType::NearestVisibleLivingEntities,
        ]
    }

    fn sensor_type(&self) -> SensorType {
        SensorType::NearestLivingEntities
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::host::EntityFacts;
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn sorts_by_distance_drops_self_and_the_dead_and_clears_on_empty() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let me = cx.mob.id();
        let mut dead = EntityFacts::living(4, 1.5, 1.0, 0.5);
        dead.alive = false;
        cx.mob.others = vec![
            (EntityClass::Living, EntityFacts::living(2, 5.5, 1.0, 0.5)),
            (EntityClass::Living, EntityFacts::living(3, 2.5, 1.0, 0.5)),
            (EntityClass::Living, dead),
            (EntityClass::Living, EntityFacts::living(me, 0.5, 1.0, 0.5)),
        ];
        let mut brain = Brain::<TestMob>::new(
            &[T::NearestLivingEntities, T::NearestVisibleLivingEntities],
            vec![],
        )
        .state;
        let mut s = NearestLivingEntitySensor;
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert_eq!(
            brain.get_memory(T::NearestLivingEntities),
            Some(&MemoryValue::Entities(vec![3, 2]))
        );
        assert_eq!(
            brain
                .get_memory(T::NearestVisibleLivingEntities)
                .unwrap()
                .as_nearest_visible()
                .unwrap()
                .nearby,
            vec![3, 2]
        );
        cx.mob.others.clear();
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert!(
            !brain.has_memory_value(T::NearestLivingEntities),
            "an empty list clears"
        );
        assert!(
            brain.has_memory_value(T::NearestVisibleLivingEntities),
            "NearestVisibleLivingEntities is not a Collection, so an empty one is stored"
        );
    }
}
