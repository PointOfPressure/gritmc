//! `net.minecraft.world.entity.ai.behavior.InteractWith`, from the 26.2
//! jar. `of(type, interactionDist, memory, speed, maxDist)` fills both
//! predicates with `e -> true`; `of(type, dist, selfPredicate,
//! targetPredicate, memory, speed, maxDist)` keeps `distSq = dist * dist`
//! and `typePredicate = e -> e.is(type) && targetPredicate.test(e)`, then
//! builds a `BehaviorBuilder` one-shot over `group(registered(memory),
//! registered(LOOK_TARGET), absent(WALK_TARGET),
//! present(NEAREST_VISIBLE_LIVING_ENTITIES))`.
//!
//! ```text
//! lambda$of$5 (0..70):
//!   nv = nearestVisible
//!   if !selfPredicate.test(mob) || !nv.contains(typePredicate): return false
//!   nv.findClosest(e -> e.distanceToSqr(mob) <= distSq && typePredicate.test(e)).ifPresent(e ->
//!       memory.set(e); look.set(EntityTracker(e, true)); walk.set(WalkTarget(EntityTracker(e, false), speed, maxDist)))
//!   return true
//! ```
//!
//! No random draw. `contains` fills the line-of-sight memo for every
//! entity up to the first match; `findClosest` continues it.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::behaviors::{nearest_visible_find_closest, MobPredicate};
use crate::brain::memory::{MemoryModuleType, MemoryValue, PositionTracker, WalkTarget};
use crate::brain::BrainState;
use crate::host::MobHost;

/// A `Predicate<T>` over another entity, by id.
pub type TargetPredicate<M> = Box<dyn FnMut(&mut MobAi<M>, i32) -> bool>;

/// `InteractWith`'s trigger.
pub struct InteractWith<M: MobHost> {
    /// The `EntityType` registry key.
    pub kind: &'static str,
    /// `interactionDist * interactionDist`.
    pub dist_sq: i32,
    pub self_predicate: MobPredicate<M>,
    pub target_predicate: TargetPredicate<M>,
    pub memory: MemoryModuleType,
    pub speed_modifier: f32,
    pub max_dist: i32,
}

impl<M: MobHost> InteractWith<M> {
    /// `of(EntityType, int, MemoryModuleType, float, int)`.
    #[must_use]
    pub fn of(
        kind: &'static str,
        interaction_dist: i32,
        memory: MemoryModuleType,
        speed_modifier: f32,
        max_dist: i32,
    ) -> OneShot<Self> {
        Self::of_with(
            kind,
            interaction_dist,
            Box::new(|_| true),
            Box::new(|_, _| true),
            memory,
            speed_modifier,
            max_dist,
        )
    }

    /// `of(EntityType, int, Predicate<E>, Predicate<T>, MemoryModuleType, float, int)`.
    #[must_use]
    pub fn of_with(
        kind: &'static str,
        interaction_dist: i32,
        self_predicate: MobPredicate<M>,
        target_predicate: TargetPredicate<M>,
        memory: MemoryModuleType,
        speed_modifier: f32,
        max_dist: i32,
    ) -> OneShot<Self> {
        OneShot::new(InteractWith {
            kind,
            dist_sq: interaction_dist * interaction_dist,
            self_predicate,
            target_predicate,
            memory,
            speed_modifier,
            max_dist,
        })
    }
}

impl<M: MobHost> Trigger<M> for InteractWith<M> {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        if brain.memory_internal(self.memory).is_none()
            || brain
                .memory_internal(MemoryModuleType::LookTarget)
                .is_none()
        {
            return false;
        }
        if brain.memory_internal(MemoryModuleType::WalkTarget) != Some(None) {
            return false;
        }
        let Some(Some(_)) = brain.memory_internal(MemoryModuleType::NearestVisibleLivingEntities)
        else {
            return false;
        };
        if !(self.self_predicate)(cx) {
            return false;
        }
        let kind = self.kind;
        let target_predicate = &mut self.target_predicate;
        let contains = nearest_visible_find_closest(brain, cx, |cx, e| {
            cx.mob.entity_kind_is(e, kind) && target_predicate(cx, e)
        })
        .is_some();
        if !contains {
            return false;
        }
        let dist_sq = f64::from(self.dist_sq);
        let target_predicate = &mut self.target_predicate;
        let found = nearest_visible_find_closest(brain, cx, |cx, e| {
            cx.mob
                .distance_to_sqr_entity(e)
                .is_some_and(|d| d <= dist_sq)
                && cx.mob.entity_kind_is(e, kind)
                && target_predicate(cx, e)
        });
        if let Some(e) = found {
            brain.set_memory_value(self.memory, MemoryValue::Entity(e));
            brain.set_memory_value(
                MemoryModuleType::LookTarget,
                MemoryValue::PositionTracker(PositionTracker::entity(e, true)),
            );
            brain.set_memory_value(
                MemoryModuleType::WalkTarget,
                MemoryValue::WalkTarget(WalkTarget::new(
                    PositionTracker::entity(e, false),
                    self.speed_modifier,
                    self.max_dist,
                )),
            );
        }
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![
            self.memory,
            MemoryModuleType::LookTarget,
            MemoryModuleType::WalkTarget,
            MemoryModuleType::NearestVisibleLivingEntities,
        ]
    }

    fn name(&self) -> &'static str {
        "InteractWith"
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
    fn targets_the_closest_visible_villager_within_the_interaction_distance() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        cx.mob.others = vec![
            (EntityClass::Living, EntityFacts::living(2, 1.5, 1.0, 0.5)),
            (EntityClass::Living, EntityFacts::living(3, 3.5, 1.0, 0.5)),
            (EntityClass::Living, EntityFacts::living(4, 9.5, 1.0, 0.5)),
        ];
        cx.mob.entity_kinds = vec![(3, "villager"), (4, "villager")];
        cx.mob.visible = vec![2, 3, 4];
        let mut brain = Brain::<TestMob>::new(
            &[
                T::InteractionTarget,
                T::LookTarget,
                T::WalkTarget,
                T::NearestVisibleLivingEntities,
            ],
            vec![],
        )
        .state;
        brain.set_memory_value(
            T::NearestVisibleLivingEntities,
            MemoryValue::NearestVisible(NearestVisibleLivingEntities::new(vec![2, 3, 4])),
        );
        let mut b = InteractWith::of("villager", 2, T::InteractionTarget, 0.5, 2);
        assert!(
            b.try_start(&mut brain, &mut cx, 0),
            "a villager is visible: triggers"
        );
        assert!(
            !brain.has_memory_value(T::InteractionTarget),
            "but none within distSq 4"
        );
        cx.mob.others[1].1.x = 2.5;
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(
            brain.get_memory(T::InteractionTarget),
            Some(&MemoryValue::Entity(3))
        );
        assert_eq!(
            brain.get_memory(T::WalkTarget).unwrap().as_walk_target(),
            Some(&WalkTarget::new(PositionTracker::entity(3, false), 0.5, 2))
        );
        brain.erase_memory(T::WalkTarget);
        let mut b = InteractWith::of_with(
            "villager",
            2,
            Box::new(|cx: &mut MobAi<TestMob>| cx.mob.baby),
            Box::new(|_, _| true),
            T::InteractionTarget,
            0.5,
            2,
        );
        assert!(!b.try_start(&mut brain, &mut cx, 0), "self predicate");
    }
}
