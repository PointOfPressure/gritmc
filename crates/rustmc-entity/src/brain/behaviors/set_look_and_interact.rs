//! `net.minecraft.world.entity.ai.behavior.SetLookAndInteract`, from the
//! 26.2 jar. A `BehaviorBuilder` one-shot over `group(registered(LOOK_TARGET),
//! absent(INTERACTION_TARGET), present(NEAREST_VISIBLE_LIVING_ENTITIES))`
//! (`lambda$create$2`, 0..65):
//!
//! ```text
//! create(type, int dist):  distSqr = dist * dist            (int multiply)
//! e = nearestVisible.findClosest(e -> e.distanceToSqr(mob) <= (double) distSqr && e.is(type))
//! if empty: return false
//! interaction.set(e); look.set(new EntityTracker(e, true)); return true
//! ```

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::behaviors::nearest_visible_find_closest;
use crate::brain::memory::{MemoryModuleType, MemoryValue, PositionTracker};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `SetLookAndInteract`'s trigger.
#[derive(Debug, Clone, Copy)]
pub struct SetLookAndInteract {
    /// The `EntityType`, by registry name.
    pub kind: &'static str,
    /// `dist * dist`.
    pub dist_sqr: i32,
}

impl SetLookAndInteract {
    /// `create(EntityType, int)`.
    #[must_use]
    pub fn create(kind: &'static str, dist: i32) -> OneShot<Self> {
        OneShot::new(SetLookAndInteract {
            kind,
            dist_sqr: dist * dist,
        })
    }
}

impl<M: MobHost> Trigger<M> for SetLookAndInteract {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        if brain
            .memory_internal(MemoryModuleType::LookTarget)
            .is_none()
            || brain.memory_internal(MemoryModuleType::InteractionTarget) != Some(None)
            || !matches!(
                brain.memory_internal(MemoryModuleType::NearestVisibleLivingEntities),
                Some(Some(_))
            )
        {
            return false;
        }
        let kind = self.kind;
        let max = f64::from(self.dist_sqr);
        let Some(id) = nearest_visible_find_closest(brain, cx, |cx, id| {
            cx.mob.distance_to_sqr_entity(id).is_some_and(|d| d <= max)
                && cx.mob.entity_kind_is(id, kind)
        }) else {
            return false;
        };
        brain.set_memory_value(MemoryModuleType::InteractionTarget, MemoryValue::Entity(id));
        brain.set_memory_value(
            MemoryModuleType::LookTarget,
            MemoryValue::PositionTracker(PositionTracker::entity(id, true)),
        );
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![
            MemoryModuleType::LookTarget,
            MemoryModuleType::InteractionTarget,
            MemoryModuleType::NearestVisibleLivingEntities,
        ]
    }

    fn name(&self) -> &'static str {
        "SetLookAndInteract"
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
    fn picks_the_first_visible_villager_within_two_blocks() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        cx.mob.others = vec![
            (EntityClass::Living, EntityFacts::living(5, 1.5, 1.0, 0.5)),
            (EntityClass::Living, EntityFacts::living(6, 2.5, 1.0, 0.5)),
        ];
        cx.mob.visible = vec![5, 6];
        cx.mob.entity_kinds = vec![(6, "villager")];
        let mut brain = Brain::<TestMob>::new(
            &[
                T::LookTarget,
                T::InteractionTarget,
                T::NearestVisibleLivingEntities,
            ],
            vec![],
        )
        .state;
        brain.set_memory_value(
            T::NearestVisibleLivingEntities,
            MemoryValue::NearestVisible(NearestVisibleLivingEntities::new(vec![5, 6])),
        );
        let mut b = SetLookAndInteract::create("villager", 2);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(
            brain.get_memory(T::InteractionTarget),
            Some(&MemoryValue::Entity(6))
        );
        assert_eq!(
            brain
                .get_memory(T::LookTarget)
                .unwrap()
                .as_position_tracker(),
            Some(&PositionTracker::entity(6, true))
        );
        assert!(
            !b.try_start(&mut brain, &mut cx, 1),
            "INTERACTION_TARGET now present"
        );
    }
}
