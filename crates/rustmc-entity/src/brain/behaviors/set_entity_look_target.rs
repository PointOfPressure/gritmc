//! `net.minecraft.world.entity.ai.behavior.SetEntityLookTarget`, from the
//! 26.2 jar. A `BehaviorBuilder` one-shot over
//! `group(absent(LOOK_TARGET), present(NEAREST_VISIBLE_LIVING_ENTITIES))`
//! (`lambda$create$5`, 0..59):
//!
//! ```text
//! create(pred, float maxDist):  f = maxDist * maxDist   (float multiply, at build time)
//! trigger: e = nearestVisible.findClosest(pred.and(e -> e.distanceToSqr(mob) <= (double) f && !mob.hasPassenger(e)))
//!          if empty: return false
//!          look.set(new EntityTracker(e, true)); return true
//! ```
//!
//! `create(MobCategory, f)`, `create(EntityType, f)`, `create(f)` are the
//! three [`EntitySelector`] shapes. `findClosest` asks `pred` before the
//! line-of-sight memo, entity by entity, in the sensor's order.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::behaviors::{nearest_visible_find_closest, EntitySelector, MobCategory};
use crate::brain::memory::{MemoryModuleType, MemoryValue, PositionTracker};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `SetEntityLookTarget`'s trigger; build one with the factories below.
#[derive(Debug, Clone, Copy)]
pub struct SetEntityLookTarget {
    pub selector: EntitySelector,
    /// `maxDist * maxDist`.
    pub max_dist_sqr: f32,
}

impl SetEntityLookTarget {
    /// `create(Predicate, float)`.
    #[must_use]
    pub fn create_with(selector: EntitySelector, max_dist: f32) -> OneShot<Self> {
        OneShot::new(SetEntityLookTarget {
            selector,
            max_dist_sqr: max_dist * max_dist,
        })
    }

    /// `create(float)`.
    #[must_use]
    pub fn create(max_dist: f32) -> OneShot<Self> {
        Self::create_with(EntitySelector::Any, max_dist)
    }

    /// `create(MobCategory, float)`.
    #[must_use]
    pub fn create_category(category: MobCategory, max_dist: f32) -> OneShot<Self> {
        Self::create_with(EntitySelector::Category(category), max_dist)
    }

    /// `create(EntityType, float)`.
    #[must_use]
    pub fn create_kind(kind: &'static str, max_dist: f32) -> OneShot<Self> {
        Self::create_with(EntitySelector::Kind(kind), max_dist)
    }
}

impl<M: MobHost> Trigger<M> for SetEntityLookTarget {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        if brain.memory_internal(MemoryModuleType::LookTarget) != Some(None) {
            return false;
        }
        if !matches!(
            brain.memory_internal(MemoryModuleType::NearestVisibleLivingEntities),
            Some(Some(_))
        ) {
            return false;
        }
        let selector = self.selector;
        let max = f64::from(self.max_dist_sqr);
        let found = nearest_visible_find_closest(brain, cx, |cx, id| {
            selector.test(cx, id)
                && cx.mob.distance_to_sqr_entity(id).is_some_and(|d| d <= max)
                && !cx.mob.has_passenger(id)
        });
        match found {
            None => false,
            Some(id) => {
                brain.set_memory_value(
                    MemoryModuleType::LookTarget,
                    MemoryValue::PositionTracker(PositionTracker::entity(id, true)),
                );
                true
            }
        }
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![
            MemoryModuleType::LookTarget,
            MemoryModuleType::NearestVisibleLivingEntities,
        ]
    }

    fn name(&self) -> &'static str {
        "SetEntityLookTarget"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::{MemoryModuleType as T, NearestVisibleLivingEntities};
    use crate::brain::Brain;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn picks_the_first_visible_entity_within_range_in_sensor_order() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 2));
        cx.mob.others = vec![
            (EntityClass::Living, EntityFacts::living(7, 20.5, 1.0, 0.5)),
            (EntityClass::Living, EntityFacts::living(8, 3.5, 1.0, 0.5)),
            (EntityClass::Living, EntityFacts::living(9, 2.5, 1.0, 0.5)),
        ];
        cx.mob.visible = vec![7, 8, 9];
        let mut brain =
            Brain::<TestMob>::new(&[T::LookTarget, T::NearestVisibleLivingEntities], vec![]).state;
        let mut b = SetEntityLookTarget::create(8.0);
        assert!(!b.try_start(&mut brain, &mut cx, 0), "no visible list");
        brain.set_memory_value(
            T::NearestVisibleLivingEntities,
            MemoryValue::NearestVisible(NearestVisibleLivingEntities::new(vec![7, 8, 9])),
        );
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(
            brain
                .get_memory(T::LookTarget)
                .unwrap()
                .as_position_tracker(),
            Some(&PositionTracker::entity(8, true)),
            "7 is 20 blocks off; 8 is the first in list order inside 8"
        );
        let memo = &brain
            .get_memory(T::NearestVisibleLivingEntities)
            .unwrap()
            .as_nearest_visible()
            .unwrap()
            .memo;
        assert_eq!(
            memo,
            &vec![(8, true)],
            "the memo is filled only for entities the predicate passed"
        );
    }
}
