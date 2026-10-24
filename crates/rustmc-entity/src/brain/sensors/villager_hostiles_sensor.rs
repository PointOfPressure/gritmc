//! `net.minecraft.world.entity.ai.sensing.VillagerHostilesSensor`, from
//! the 26.2 jar: a `NearestVisibleLivingEntitySensor` whose memory is
//! `NEAREST_HOSTILE`.
//!
//! ```text
//! ACCEPTABLE_DISTANCE_FROM_HOSTILES (<clinit>, in put order):
//!   DROWNED 8, EVOKER 12, HUSK 8, ILLUSIONER 12, PILLAGER 15, RAVAGER 12,
//!   VEX 8, VINDICATOR 10, ZOGLIN 10, ZOMBIE 8, ZOMBIE_VILLAGER 8   (floats)
//! isMatchingEntity (0..22): isHostile(e) && isClose(mob, e)
//! isHostile: map.containsKey(e.getType())
//! isClose:   f = map.get(e.getType()); e.distanceToSqr(mob) <= (double)(f * f)     <- dcmpg/ifgt at 26..27
//! ```
//!
//! No random draw. The map is an `ImmutableMap` keyed by `EntityType`;
//! only membership and the value are read, so its order is not observed.

use crate::ai::mob_ai::MobAi;
use crate::brain::memory::MemoryModuleType;
use crate::brain::sensor::{Sensor, SensorBody, SensorType};
use crate::brain::sensors::nearest_visible_living_entity_sensor::{do_tick_with, requires_with};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `ACCEPTABLE_DISTANCE_FROM_HOSTILES`, by entity type registry key.
pub const ACCEPTABLE_DISTANCE_FROM_HOSTILES: [(&str, f32); 11] = [
    ("drowned", 8.0),
    ("evoker", 12.0),
    ("husk", 8.0),
    ("illusioner", 12.0),
    ("pillager", 15.0),
    ("ravager", 12.0),
    ("vex", 8.0),
    ("vindicator", 10.0),
    ("zoglin", 10.0),
    ("zombie", 8.0),
    ("zombie_villager", 8.0),
];

/// `VillagerHostilesSensor`.
#[derive(Debug, Clone, Copy, Default)]
pub struct VillagerHostilesSensor;

impl VillagerHostilesSensor {
    /// `SensorType.VILLAGER_HOSTILES.create()`.
    #[must_use]
    pub fn new() -> Sensor<VillagerHostilesSensor> {
        Sensor::new(VillagerHostilesSensor)
    }

    /// `ACCEPTABLE_DISTANCE_FROM_HOSTILES.get(e.getType())`.
    fn acceptable_distance<M: MobHost>(cx: &MobAi<M>, id: i32) -> Option<f32> {
        ACCEPTABLE_DISTANCE_FROM_HOSTILES
            .iter()
            .find(|(kind, _)| cx.mob.entity_kind_is(id, kind))
            .map(|(_, d)| *d)
    }

    /// `isMatchingEntity(level, mob, e)`.
    pub fn is_matching_entity<M: MobHost>(cx: &mut MobAi<M>, id: i32) -> bool {
        let Some(f) = Self::acceptable_distance(cx, id) else {
            return false;
        };
        let d2 = f64::from(f * f);
        cx.mob.distance_to_sqr_entity(id).is_some_and(|d| d <= d2)
    }
}

impl<M: MobHost> SensorBody<M> for VillagerHostilesSensor {
    fn do_tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) {
        do_tick_with(brain, cx, MemoryModuleType::NearestHostile, |cx, id| {
            Self::is_matching_entity(cx, id)
        });
    }

    fn requires(&self) -> Vec<MemoryModuleType> {
        requires_with(MemoryModuleType::NearestHostile)
    }

    fn sensor_type(&self) -> SensorType {
        SensorType::VillagerHostiles
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::memory::{MemoryModuleType as T, MemoryValue, NearestVisibleLivingEntities};
    use crate::brain::Brain;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn the_closest_visible_hostile_within_its_own_radius_is_the_nearest_hostile() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        cx.mob.others = vec![
            (EntityClass::Living, EntityFacts::living(2, 9.5, 1.0, 0.5)),
            (EntityClass::Living, EntityFacts::living(3, 12.5, 1.0, 0.5)),
            (EntityClass::Living, EntityFacts::living(4, 14.5, 1.0, 0.5)),
        ];
        cx.mob.entity_kinds = vec![(2, "zombie"), (3, "pillager"), (4, "pillager")];
        cx.mob.visible = vec![2, 3, 4];
        let mut brain = Brain::<TestMob>::new(
            &[T::NearestHostile, T::NearestVisibleLivingEntities],
            vec![],
        )
        .state;
        brain.set_memory_value(
            T::NearestVisibleLivingEntities,
            MemoryValue::NearestVisible(NearestVisibleLivingEntities::new(vec![2, 3, 4])),
        );
        let mut s = VillagerHostilesSensor;
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert_eq!(
            brain.get_memory(T::NearestHostile),
            Some(&MemoryValue::Entity(3)),
            "the zombie at 9 is past its 8; the pillager at 12 is within 15"
        );
        cx.mob.visible = vec![2, 4];
        cx.sensing.tick();
        brain.set_memory_value(
            T::NearestVisibleLivingEntities,
            MemoryValue::NearestVisible(NearestVisibleLivingEntities::new(vec![2, 3, 4])),
        );
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert_eq!(
            brain.get_memory(T::NearestHostile),
            Some(&MemoryValue::Entity(4))
        );
        brain.erase_memory(T::NearestVisibleLivingEntities);
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert!(
            !brain.has_memory_value(T::NearestHostile),
            "no visible list: erased"
        );
        assert_eq!(
            SensorBody::<TestMob>::requires(&s),
            [T::NearestHostile, T::NearestVisibleLivingEntities]
        );
    }
}
