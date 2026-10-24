//! `net.minecraft.world.entity.ai.sensing.AdultSensor` and its subclass
//! `AdultSensorAnyType`, from the 26.2 jar. `Sensor()` (scan rate 20);
//! `requires()` = `{NEAREST_VISIBLE_ADULT, NEAREST_VISIBLE_LIVING_ENTITIES}`
//! in that `ImmutableSet.of` order.
//!
//! ```text
//! AdultSensor.doTick (0..20):        brain.getMemory(NEAREST_VISIBLE_LIVING_ENTITIES)
//!                                      .ifPresent(nv -> setNearestVisibleAdult(mob, nv))
//! setNearestVisibleAdult (0..22):    brain.setMemory(NEAREST_VISIBLE_ADULT, nv.findClosest(pred))
//! AdultSensor pred (0..23):          e.getType() == mob.getType() && !e.isBaby()
//! AdultSensorAnyType pred (0..22):   e.is(EntityTypeTags.FOLLOWABLE_FRIENDLY_MOBS) && !e.isBaby()
//! ```
//!
//! Two consequences of the shape, both reproduced: an **absent**
//! `NEAREST_VISIBLE_LIVING_ENTITIES` leaves `NEAREST_VISIBLE_ADULT`
//! untouched (`ifPresent`, no else branch), while a **present** one with no
//! passing adult erases it, because `Brain.setMemory(type, Optional)` with
//! an empty optional is an erase. The subclass overrides only the predicate
//! and re-emits the same two statements — it does not call `super`, so the
//! `getType()` comparison is gone rather than added to.

use crate::ai::mob_ai::MobAi;
use crate::brain::behaviors::nearest_visible_find_closest;
use crate::brain::memory::{MemoryModuleType, MemoryValue};
use crate::brain::sensor::{Sensor, SensorBody, SensorType};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `EntityTypeTags.FOLLOWABLE_FRIENDLY_MOBS`.
pub const FOLLOWABLE_FRIENDLY_MOBS: &str = "followable_friendly_mobs";

/// Which `setNearestVisibleAdult` override this instance carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdultKind {
    /// `AdultSensor`: the same `EntityType` as the sensing mob.
    SameType,
    /// `AdultSensorAnyType`: anything in `FOLLOWABLE_FRIENDLY_MOBS`.
    AnyType,
}

/// `AdultSensor` / `AdultSensorAnyType`.
#[derive(Debug, Clone, Copy)]
pub struct AdultSensor {
    /// Which predicate `setNearestVisibleAdult` applies.
    pub kind: AdultKind,
}

impl AdultSensor {
    /// `SensorType.NEAREST_ADULT.create()` — `new AdultSensor()`.
    #[must_use]
    pub fn new() -> Sensor<AdultSensor> {
        Sensor::new(AdultSensor {
            kind: AdultKind::SameType,
        })
    }

    /// `SensorType.NEAREST_ADULT_ANY_TYPE.create()` —
    /// `new AdultSensorAnyType()`.
    #[must_use]
    pub fn any_type() -> Sensor<AdultSensor> {
        Sensor::new(AdultSensor {
            kind: AdultKind::AnyType,
        })
    }
}

impl<M: MobHost> SensorBody<M> for AdultSensor {
    fn do_tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) {
        if !brain.has_memory_value(MemoryModuleType::NearestVisibleLivingEntities) {
            return;
        }
        let kind = self.kind;
        let found = nearest_visible_find_closest(brain, cx, |cx, id| {
            let passes = match kind {
                AdultKind::SameType => cx.mob.entity_type_matches_mine(id),
                AdultKind::AnyType => cx.mob.entity_type_tag_is(id, FOLLOWABLE_FRIENDLY_MOBS),
            };
            passes && cx.mob.entity(id).is_none_or(|e| e.age >= 0)
        });
        match found {
            Some(id) => {
                brain.set_memory_value(
                    MemoryModuleType::NearestVisibleAdult,
                    MemoryValue::Entity(id),
                );
            }
            None => brain.erase_memory(MemoryModuleType::NearestVisibleAdult),
        }
    }

    fn requires(&self) -> Vec<MemoryModuleType> {
        vec![
            MemoryModuleType::NearestVisibleAdult,
            MemoryModuleType::NearestVisibleLivingEntities,
        ]
    }

    fn sensor_type(&self) -> SensorType {
        match self.kind {
            AdultKind::SameType => SensorType::NearestAdult,
            AdultKind::AnyType => SensorType::NearestAdultAnyType,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::memory::{MemoryModuleType as T, NearestVisibleLivingEntities};
    use crate::brain::Brain;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    fn setup() -> (MobAi<TestMob>, BrainState) {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut baby = EntityFacts::living(2, 1.5, 1.0, 0.5);
        baby.age = -1200;
        cx.mob.others = vec![
            (EntityClass::Living, baby),
            (EntityClass::Living, EntityFacts::living(3, 2.5, 1.0, 0.5)),
            (EntityClass::Living, EntityFacts::living(4, 3.5, 1.0, 0.5)),
        ];
        cx.mob.visible = vec![2, 3, 4];
        let mut brain = Brain::<TestMob>::new(
            &[T::NearestVisibleAdult, T::NearestVisibleLivingEntities],
            vec![],
        )
        .state;
        brain.set_memory_value(
            T::NearestVisibleLivingEntities,
            MemoryValue::NearestVisible(NearestVisibleLivingEntities::new(vec![2, 3, 4])),
        );
        (cx, brain)
    }

    #[test]
    fn same_type_skips_the_baby_and_the_foreign_type_and_erases_when_none_pass() {
        let (mut cx, mut brain) = setup();
        // 2 and 3 are my type; 2 is a baby, so the closest adult is 3.
        cx.mob.same_type_entities = vec![2, 3];
        let mut s = AdultSensor {
            kind: AdultKind::SameType,
        };
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert_eq!(
            brain.get_memory(T::NearestVisibleAdult),
            Some(&MemoryValue::Entity(3))
        );
        // 4 is a different type, so nothing passes: the empty Optional
        // erases rather than leaving the old value.
        cx.mob.same_type_entities = vec![2];
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert!(!brain.has_memory_value(T::NearestVisibleAdult));
    }

    #[test]
    fn any_type_reads_the_tag_and_an_absent_visible_list_leaves_the_memory_alone() {
        let (mut cx, mut brain) = setup();
        cx.mob.same_type_entities = vec![2, 3, 4];
        cx.mob.entity_type_tags =
            vec![(3, FOLLOWABLE_FRIENDLY_MOBS), (4, FOLLOWABLE_FRIENDLY_MOBS)];
        let mut s = AdultSensor {
            kind: AdultKind::AnyType,
        };
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert_eq!(
            brain.get_memory(T::NearestVisibleAdult),
            Some(&MemoryValue::Entity(3)),
            "the tag, not getType(); 2 is a baby anyway"
        );
        // `ifPresent` with no else: an absent visible list is a no-op, not
        // an erase. This is the half that separates the two branches.
        brain.erase_memory(T::NearestVisibleLivingEntities);
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert_eq!(
            brain.get_memory(T::NearestVisibleAdult),
            Some(&MemoryValue::Entity(3))
        );
        assert_eq!(
            SensorBody::<TestMob>::requires(&s),
            vec![T::NearestVisibleAdult, T::NearestVisibleLivingEntities]
        );
    }
}
