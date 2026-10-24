//! `net.minecraft.world.entity.ai.sensing.PiglinBruteSpecificSensor`, from
//! the 26.2 jar. `Sensor()` (scan rate 20); `requires()` =
//! `{NEAREST_VISIBLE_LIVING_ENTITIES, NEAREST_VISIBLE_NEMESIS,
//! NEARBY_ADULT_PIGLINS}` in that `ImmutableSet.of` order.
//!
//! ```text
//! doTick (0..74):
//!   visible = brain.getMemory(NEAREST_VISIBLE_LIVING_ENTITIES).orElse(NearestVisibleLivingEntities.empty())
//!   nemesis = visible.findClosest(e -> e instanceof WitherSkeleton || e instanceof WitherBoss).map(Mob.class::cast)
//!   nearby  = PiglinAi.findNearbyAdultPiglins(brain)
//!   brain.setMemory(NEAREST_VISIBLE_NEMESIS, nemesis)
//!   brain.setMemory(NEARBY_ADULT_PIGLINS, nearby)
//! ```
//!
//! The whole class, and it is the piglin sensor's nemesis clause and its
//! adult-piglin list with nothing else: no repellent scan, no gold check,
//! no huntable hoglin. `findNearbyAdultPiglins` reads
//! `NEAREST_LIVING_ENTITIES`, which this sensor does **not** declare in
//! `requires()` — the brute's provider registers it through
//! `NEAREST_LIVING_ENTITIES`'s own sensor, and an absent memory answers an
//! empty list rather than throwing.
//!
//! Both writes are `setMemory(type, Optional)` / `setMemory(type, List)`,
//! so an absent nemesis and an empty list each erase.

use crate::ai::mob_ai::MobAi;
use crate::brain::behaviors::nearest_visible_find_closest;
use crate::brain::memory::{MemoryModuleType, MemoryValue};
use crate::brain::sensor::{Sensor, SensorBody, SensorType};
use crate::brain::sensors::PiglinSpecificSensor;
use crate::brain::BrainState;
use crate::host::MobHost;

/// `PiglinBruteSpecificSensor`.
#[derive(Debug, Clone, Copy, Default)]
pub struct PiglinBruteSpecificSensor;

impl PiglinBruteSpecificSensor {
    /// `SensorType.PIGLIN_BRUTE_SPECIFIC_SENSOR.create()`.
    #[must_use]
    pub fn new() -> Sensor<PiglinBruteSpecificSensor> {
        Sensor::new(PiglinBruteSpecificSensor)
    }
}

impl<M: MobHost> SensorBody<M> for PiglinBruteSpecificSensor {
    fn do_tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) {
        let nemesis = nearest_visible_find_closest(brain, cx, |cx, id| {
            cx.mob.entity_kind_is(id, "wither_skeleton") || cx.mob.entity_kind_is(id, "wither")
        });
        let nearby = PiglinSpecificSensor::find_nearby_adult_piglins(brain, cx);
        brain.set_memory(
            MemoryModuleType::NearestVisibleNemesis,
            nemesis.map(MemoryValue::Entity),
        );
        brain.set_memory_value(
            MemoryModuleType::NearbyAdultPiglins,
            MemoryValue::Entities(nearby),
        );
    }

    fn requires(&self) -> Vec<MemoryModuleType> {
        vec![
            MemoryModuleType::NearestVisibleLivingEntities,
            MemoryModuleType::NearestVisibleNemesis,
            MemoryModuleType::NearbyAdultPiglins,
        ]
    }

    fn sensor_type(&self) -> SensorType {
        SensorType::PiglinBruteSpecificSensor
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::memory::{MemoryModuleType as T, NearestVisibleLivingEntities};
    use crate::brain::Brain;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn the_nemesis_is_the_closest_wither_kind_and_the_piglin_list_comes_from_the_plain_list() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut baby = EntityFacts::living(5, 1.5, 1.0, 0.5);
        baby.age = -50;
        cx.mob.others = vec![
            (EntityClass::Living, EntityFacts::living(2, 2.5, 1.0, 0.5)),
            (EntityClass::Living, EntityFacts::living(3, 3.5, 1.0, 0.5)),
            (EntityClass::Living, EntityFacts::living(4, 4.5, 1.0, 0.5)),
            (EntityClass::Living, baby),
        ];
        cx.mob.entity_kinds = vec![
            (2, "piglin"),
            (3, "wither_skeleton"),
            (4, "wither"),
            (5, "piglin"),
        ];
        cx.mob.visible = vec![2, 3, 4, 5];
        let mut brain = Brain::<TestMob>::new(
            &[
                T::NearestVisibleLivingEntities,
                T::NearestLivingEntities,
                T::NearestVisibleNemesis,
                T::NearbyAdultPiglins,
            ],
            vec![],
        )
        .state;
        brain.set_memory_value(
            T::NearestVisibleLivingEntities,
            MemoryValue::NearestVisible(NearestVisibleLivingEntities::new(vec![2, 3, 4, 5])),
        );
        brain.set_memory_value(
            T::NearestLivingEntities,
            MemoryValue::Entities(vec![2, 3, 4, 5]),
        );
        let mut s = PiglinBruteSpecificSensor;
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert_eq!(
            brain.get_memory(T::NearestVisibleNemesis),
            Some(&MemoryValue::Entity(3))
        );
        assert_eq!(
            brain.get_memory(T::NearbyAdultPiglins),
            Some(&MemoryValue::Entities(vec![2])),
            "the baby piglin is not an adult"
        );
        // The nemesis comes off the VISIBLE list; the piglins off the plain
        // one. Erasing the plain list alone empties only the second.
        brain.erase_memory(T::NearestLivingEntities);
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert_eq!(
            brain.get_memory(T::NearestVisibleNemesis),
            Some(&MemoryValue::Entity(3))
        );
        assert!(!brain.has_memory_value(T::NearbyAdultPiglins));
        // And no visible list erases the nemesis.
        brain.erase_memory(T::NearestVisibleLivingEntities);
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert!(!brain.has_memory_value(T::NearestVisibleNemesis));
    }
}
