//! `net.minecraft.world.entity.ai.behavior.SetWalkTargetFromLookTarget`, from
//! the 26.2 jar. A `BehaviorBuilder` one-shot over
//! `group(absent(WALK_TARGET), present(LOOK_TARGET))`:
//!
//! ```text
//! lambda$create$4 (0..50):
//!   if !predicate.test(mob): return false
//!   walk.set(new WalkTarget(lookTarget, speed.apply(mob), closeEnoughDist)); return true
//! ```
//!
//! `create(float speed, int closeEnough)` is `create(e -> true, e -> speed, closeEnough)`.
//! The walk target wraps the look target's `PositionTracker` itself (an
//! `EntityTracker` keeps its `trackEyeHeight`).

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::behaviors::{MobPredicate, MobSpeed};
use crate::brain::memory::{MemoryModuleType, MemoryValue, WalkTarget};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `SetWalkTargetFromLookTarget`'s trigger; build one with
/// [`SetWalkTargetFromLookTarget::create`].
pub struct SetWalkTargetFromLookTarget<M: MobHost> {
    /// `Predicate<LivingEntity>`.
    pub predicate: MobPredicate<M>,
    /// `Function<LivingEntity, Float>`.
    pub speed: MobSpeed<M>,
    pub close_enough_dist: i32,
}

impl<M: MobHost + 'static> SetWalkTargetFromLookTarget<M> {
    /// `create(float, int)`.
    #[must_use]
    pub fn create(speed: f32, close_enough_dist: i32) -> OneShot<Self> {
        Self::create_with(
            Box::new(|_| true),
            Box::new(move |_| speed),
            close_enough_dist,
        )
    }

    /// `create(Predicate, Function, int)`.
    #[must_use]
    pub fn create_with(
        predicate: MobPredicate<M>,
        speed: MobSpeed<M>,
        close_enough_dist: i32,
    ) -> OneShot<Self> {
        OneShot::new(SetWalkTargetFromLookTarget {
            predicate,
            speed,
            close_enough_dist,
        })
    }
}

impl<M: MobHost> Trigger<M> for SetWalkTargetFromLookTarget<M> {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        if brain.memory_internal(MemoryModuleType::WalkTarget) != Some(None) {
            return false;
        }
        let Some(Some(look)) = brain.memory_internal(MemoryModuleType::LookTarget) else {
            return false;
        };
        let Some(tracker) = look.as_position_tracker().cloned() else {
            return false;
        };
        if !(self.predicate)(cx) {
            return false;
        }
        let speed = (self.speed)(cx);
        brain.set_memory_value(
            MemoryModuleType::WalkTarget,
            MemoryValue::WalkTarget(WalkTarget::new(tracker, speed, self.close_enough_dist)),
        );
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![MemoryModuleType::WalkTarget, MemoryModuleType::LookTarget]
    }

    fn name(&self) -> &'static str {
        "SetWalkTargetFromLookTarget"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::{MemoryModuleType as T, PositionTracker};
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn copies_the_look_tracker_into_a_walk_target() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut brain = Brain::<TestMob>::new(&[T::WalkTarget, T::LookTarget], vec![]).state;
        let mut b = SetWalkTargetFromLookTarget::<TestMob>::create(0.6, 2);
        assert!(!b.try_start(&mut brain, &mut cx, 0));
        brain.set_memory_value(
            T::LookTarget,
            MemoryValue::PositionTracker(PositionTracker::entity(9, true)),
        );
        assert!(b.try_start(&mut brain, &mut cx, 0));
        let w = brain
            .get_memory(T::WalkTarget)
            .unwrap()
            .as_walk_target()
            .unwrap()
            .clone();
        assert_eq!(w.target, PositionTracker::entity(9, true));
        assert_eq!((w.speed_modifier, w.close_enough_dist), (0.6, 2));
        assert!(
            !b.try_start(&mut brain, &mut cx, 1),
            "WALK_TARGET now present"
        );
    }
}
