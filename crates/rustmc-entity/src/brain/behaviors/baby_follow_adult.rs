//! `net.minecraft.world.entity.ai.behavior.BabyFollowAdult`, from the 26.2
//! jar. A `BehaviorBuilder` one-shot over `group(present(type),
//! registered(LOOK_TARGET), absent(WALK_TARGET))` (`lambda$create$3`, 0..119):
//!
//! ```text
//! if !mob.isBaby(): return false
//! adult = memory
//! if mob.closerThan(adult, (double) (followRange.maxInclusive() + 1)) && !mob.closerThan(adult, (double) followRange.minInclusive()):
//!     walk = new WalkTarget(new EntityTracker(adult, targetEyeHeight, targetEyeHeight), speed.apply(mob), followRange.minInclusive() - 1)
//!     look.set(new EntityTracker(adult, true, targetEyeHeight)); walkAccessor.set(walk); return true
//! return false
//! ```
//!
//! `create(UniformInt, float)` is `create(range, e -> speed, NEAREST_VISIBLE_ADULT, false)`.
//! `closerThan` is `distanceToSqr < d*d`, so the band is
//! `min <= dist < max + 1`.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::behaviors::set_entity_look_target_sometimes::UniformInt;
use crate::brain::behaviors::MobSpeed;
use crate::brain::memory::{MemoryModuleType, MemoryValue, PositionTracker, WalkTarget};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `BabyFollowAdult`'s trigger.
pub struct BabyFollowAdult<M: MobHost> {
    pub follow_range: UniformInt,
    pub speed: MobSpeed<M>,
    pub memory: MemoryModuleType,
    pub target_eye_height: bool,
}

impl<M: MobHost + 'static> BabyFollowAdult<M> {
    /// `create(UniformInt, float)`.
    #[must_use]
    pub fn create(follow_range: UniformInt, speed: f32) -> OneShot<Self> {
        Self::create_with(
            follow_range,
            Box::new(move |_| speed),
            MemoryModuleType::NearestVisibleAdult,
            false,
        )
    }

    /// `create(UniformInt, Function, MemoryModuleType, boolean)`.
    #[must_use]
    pub fn create_with(
        follow_range: UniformInt,
        speed: MobSpeed<M>,
        memory: MemoryModuleType,
        target_eye_height: bool,
    ) -> OneShot<Self> {
        OneShot::new(BabyFollowAdult {
            follow_range,
            speed,
            memory,
            target_eye_height,
        })
    }
}

impl<M: MobHost> Trigger<M> for BabyFollowAdult<M> {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        let Some(Some(v)) = brain.memory_internal(self.memory) else {
            return false;
        };
        let Some(adult) = v.as_entity() else {
            return false;
        };
        if brain
            .memory_internal(MemoryModuleType::LookTarget)
            .is_none()
            || brain.memory_internal(MemoryModuleType::WalkTarget) != Some(None)
        {
            return false;
        }
        if !cx.mob.is_baby() {
            return false;
        }
        let Some(sq) = cx.mob.distance_to_sqr_entity(adult) else {
            return false;
        };
        let max = f64::from(self.follow_range.max_inclusive + 1);
        let min = f64::from(self.follow_range.min_inclusive);
        if sq < max * max && sq >= min * min {
            let speed = (self.speed)(cx);
            let walk = WalkTarget::new(
                PositionTracker::entity_full(adult, self.target_eye_height, self.target_eye_height),
                speed,
                self.follow_range.min_inclusive - 1,
            );
            brain.set_memory_value(
                MemoryModuleType::LookTarget,
                MemoryValue::PositionTracker(PositionTracker::entity_full(
                    adult,
                    true,
                    self.target_eye_height,
                )),
            );
            brain.set_memory_value(MemoryModuleType::WalkTarget, MemoryValue::WalkTarget(walk));
            true
        } else {
            false
        }
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![
            self.memory,
            MemoryModuleType::LookTarget,
            MemoryModuleType::WalkTarget,
        ]
    }

    fn name(&self) -> &'static str {
        "BabyFollowAdult"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn a_baby_follows_an_adult_inside_the_band_only() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        cx.mob.baby = true;
        cx.mob.others = vec![(EntityClass::Living, EntityFacts::living(2, 8.5, 1.0, 0.5))];
        let mut brain = Brain::<TestMob>::new(
            &[T::NearestVisibleAdult, T::LookTarget, T::WalkTarget],
            vec![],
        )
        .state;
        brain.set_memory_value(T::NearestVisibleAdult, MemoryValue::Entity(2));
        let mut b = BabyFollowAdult::<TestMob>::create(UniformInt::of(5, 16), 0.5);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(
            brain.get_memory(T::WalkTarget).unwrap().as_walk_target(),
            Some(&WalkTarget::new(
                PositionTracker::entity_full(2, false, false),
                0.5,
                4
            ))
        );
        assert_eq!(
            brain
                .get_memory(T::LookTarget)
                .unwrap()
                .as_position_tracker(),
            Some(&PositionTracker::entity_full(2, true, false))
        );
        brain.erase_memory(T::WalkTarget);
        cx.mob.others[0].1.x = 4.5;
        assert!(
            !b.try_start(&mut brain, &mut cx, 1),
            "4 blocks is inside min 5: no follow"
        );
        cx.mob.others[0].1.x = 17.5;
        assert!(
            !b.try_start(&mut brain, &mut cx, 2),
            "17 is not < max + 1 = 17"
        );
        cx.mob.others[0].1.x = 16.5;
        assert!(b.try_start(&mut brain, &mut cx, 3), "16 < 17");
        brain.erase_memory(T::WalkTarget);
        cx.mob.baby = false;
        assert!(!b.try_start(&mut brain, &mut cx, 4), "adults do not follow");
    }
}
