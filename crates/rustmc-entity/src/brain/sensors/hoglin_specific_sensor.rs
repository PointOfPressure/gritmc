//! `net.minecraft.world.entity.ai.sensing.HoglinSpecificSensor`, from the
//! 26.2 jar. `Sensor()` (scan rate 20); `requires()` =
//! `{NEAREST_VISIBLE_LIVING_ENTITIES, NEAREST_REPELLENT,
//! NEAREST_VISIBLE_ADULT_PIGLIN, NEAREST_VISIBLE_ADULT_HOGLINS,
//! VISIBLE_ADULT_PIGLIN_COUNT, VISIBLE_ADULT_HOGLIN_COUNT}` in that
//! `ImmutableSet.of` order.
//!
//! ```text
//! doTick (0..196):
//!   brain.setMemory(NEAREST_REPELLENT, findNearestRepellent(level, hoglin))
//!   nearestPiglin = Optional.empty(); piglinCount = 0; hoglins = Lists.newArrayList()
//!   for e in visible.findAll(x -> !x.isBaby() && (x instanceof Piglin || x instanceof Hoglin)):
//!       if e instanceof Piglin p:  piglinCount++; if nearestPiglin.isEmpty(): nearestPiglin = Optional.of(p)
//!       if e instanceof Hoglin h:  hoglins.add(h)
//!   brain.setMemory(NEAREST_VISIBLE_ADULT_PIGLIN, nearestPiglin)
//!   brain.setMemory(NEAREST_VISIBLE_ADULT_HOGLINS, hoglins)
//!   brain.setMemory(VISIBLE_ADULT_PIGLIN_COUNT, piglinCount)
//!   brain.setMemory(VISIBLE_ADULT_HOGLIN_COUNT, hoglins.size())
//! findNearestRepellent (0..16): BlockPos.findClosestMatch(hoglin.blockPosition(), 8, 4,
//!                                 p -> level.getBlockState(p).is(BlockTags.HOGLIN_REPELLENTS))
//! ```
//!
//! Two things the offsets settle. The two `instanceof` tests at 90 and
//! 123 are **separate `if`s, not an `else if`** — nothing is both a piglin
//! and a hoglin so it makes no difference to behaviour, but the count and
//! the list are filled in one pass over `findAll`'s order and that order
//! is the sensor's, not a re-sort. And the filter at 49..56 excludes
//! babies of **both** types before either branch, so
//! `VISIBLE_ADULT_HOGLIN_COUNT` counts adults only, as its name says and
//! as `VISIBLE_ADULT_PIGLIN_COUNT` does.
//!
//! `NEAREST_VISIBLE_ADULT_PIGLIN` is the singular memory, distinct from
//! the piglin sensor's `NEAREST_VISIBLE_ADULT_PIGLINS` list.
//! `BlockTags.HOGLIN_REPELLENTS` is a different tag from the piglin's, so
//! the repellent scan is a host call of its own rather than a reuse.

use rustmc_data::BlockPos;

use crate::ai::mob_ai::MobAi;
use crate::brain::behaviors::nearest_visible_find_all;
use crate::brain::memory::{MemoryModuleType, MemoryValue};
use crate::brain::sensor::{Sensor, SensorBody, SensorType};
use crate::brain::BrainState;
use crate::host::MobHost;
use crate::phys::find_closest_match;

/// `HoglinSpecificSensor`.
#[derive(Debug, Clone, Copy, Default)]
pub struct HoglinSpecificSensor;

impl HoglinSpecificSensor {
    /// `SensorType.HOGLIN_SPECIFIC_SENSOR.create()`.
    #[must_use]
    pub fn new() -> Sensor<HoglinSpecificSensor> {
        Sensor::new(HoglinSpecificSensor)
    }

    /// `findNearestRepellent(level, hoglin)`.
    pub fn find_nearest_repellent<M: MobHost>(cx: &MobAi<M>) -> Option<BlockPos> {
        find_closest_match(cx.mob.block_position(), 8, 4, |p| {
            cx.mob.is_hoglin_repellent_at(p)
        })
    }
}

impl<M: MobHost> SensorBody<M> for HoglinSpecificSensor {
    fn do_tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) {
        let repellent = Self::find_nearest_repellent(cx);
        brain.set_memory(
            MemoryModuleType::NearestRepellent,
            repellent.map(MemoryValue::BlockPos),
        );
        let adults = nearest_visible_find_all(brain, cx, |cx, id| {
            let baby = cx.mob.entity(id).is_some_and(|e| e.age < 0);
            !baby && (cx.mob.entity_kind_is(id, "piglin") || cx.mob.entity_kind_is(id, "hoglin"))
        });
        let mut nearest_piglin: Option<i32> = None;
        let mut piglin_count = 0_i32;
        let mut hoglins: Vec<i32> = Vec::new();
        for id in adults {
            if cx.mob.entity_kind_is(id, "piglin") {
                piglin_count += 1;
                if nearest_piglin.is_none() {
                    nearest_piglin = Some(id);
                }
            }
            if cx.mob.entity_kind_is(id, "hoglin") {
                hoglins.push(id);
            }
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let hoglin_count = hoglins.len() as i32;
        brain.set_memory(
            MemoryModuleType::NearestVisibleAdultPiglin,
            nearest_piglin.map(MemoryValue::Entity),
        );
        brain.set_memory_value(
            MemoryModuleType::NearestVisibleAdultHoglins,
            MemoryValue::Entities(hoglins),
        );
        brain.set_memory_value(
            MemoryModuleType::VisibleAdultPiglinCount,
            MemoryValue::Int(piglin_count),
        );
        brain.set_memory_value(
            MemoryModuleType::VisibleAdultHoglinCount,
            MemoryValue::Int(hoglin_count),
        );
    }

    fn requires(&self) -> Vec<MemoryModuleType> {
        vec![
            MemoryModuleType::NearestVisibleLivingEntities,
            MemoryModuleType::NearestRepellent,
            MemoryModuleType::NearestVisibleAdultPiglin,
            MemoryModuleType::NearestVisibleAdultHoglins,
            MemoryModuleType::VisibleAdultPiglinCount,
            MemoryModuleType::VisibleAdultHoglinCount,
        ]
    }

    fn sensor_type(&self) -> SensorType {
        SensorType::HoglinSpecificSensor
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
    fn babies_are_excluded_from_both_counts_and_the_first_piglin_in_sensed_order_wins() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut baby_piglin = EntityFacts::living(5, 1.5, 1.0, 0.5);
        baby_piglin.age = -100;
        let mut baby_hoglin = EntityFacts::living(6, 1.5, 1.0, 0.5);
        baby_hoglin.age = -100;
        cx.mob.others = vec![
            (EntityClass::Living, EntityFacts::living(2, 2.5, 1.0, 0.5)),
            (EntityClass::Living, EntityFacts::living(3, 3.5, 1.0, 0.5)),
            (EntityClass::Living, EntityFacts::living(4, 4.5, 1.0, 0.5)),
            (EntityClass::Living, baby_piglin),
            (EntityClass::Living, baby_hoglin),
        ];
        cx.mob.entity_kinds = vec![
            (2, "hoglin"),
            (3, "piglin"),
            (4, "piglin"),
            (5, "piglin"),
            (6, "hoglin"),
        ];
        cx.mob.visible = vec![2, 3, 4, 5, 6];
        let mut brain = Brain::<TestMob>::new(
            &[
                T::NearestVisibleLivingEntities,
                T::NearestRepellent,
                T::NearestVisibleAdultPiglin,
                T::NearestVisibleAdultHoglins,
                T::VisibleAdultPiglinCount,
                T::VisibleAdultHoglinCount,
            ],
            vec![],
        )
        .state;
        brain.set_memory_value(
            T::NearestVisibleLivingEntities,
            MemoryValue::NearestVisible(NearestVisibleLivingEntities::new(vec![2, 3, 4, 5, 6])),
        );
        let mut s = HoglinSpecificSensor;
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert_eq!(
            brain.get_memory(T::NearestVisibleAdultPiglin),
            Some(&MemoryValue::Entity(3))
        );
        assert_eq!(
            brain.get_memory(T::NearestVisibleAdultHoglins),
            Some(&MemoryValue::Entities(vec![2]))
        );
        assert_eq!(
            brain.get_memory(T::VisibleAdultPiglinCount),
            Some(&MemoryValue::Int(2))
        );
        assert_eq!(
            brain.get_memory(T::VisibleAdultHoglinCount),
            Some(&MemoryValue::Int(1))
        );
        assert!(
            !brain.has_memory_value(T::NearestRepellent),
            "no repellent block in the grid"
        );
        // With no visible list the pass is empty: an empty list and a zero
        // count are still written, and the empty list CLEARS its memory.
        brain.erase_memory(T::NearestVisibleLivingEntities);
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert!(!brain.has_memory_value(T::NearestVisibleAdultPiglin));
        assert!(!brain.has_memory_value(T::NearestVisibleAdultHoglins));
        assert_eq!(
            brain.get_memory(T::VisibleAdultPiglinCount),
            Some(&MemoryValue::Int(0))
        );
    }
}
