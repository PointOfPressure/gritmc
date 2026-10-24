//! `net.minecraft.world.entity.ai.behavior.SetEntityLookTargetSometimes`
//! (+ `$Ticker`), from the 26.2 jar. A `BehaviorBuilder` one-shot over
//! `group(absent(LOOK_TARGET), present(NEAREST_VISIBLE_LIVING_ENTITIES))`
//! (`lambda$create$4`, 0..74):
//!
//! ```text
//! create(float maxDist, UniformInt interval, pred):  f = maxDist * maxDist; ticker = new Ticker(interval)
//! trigger: e = nearestVisible.findClosest(pred.and(e -> e.distanceToSqr(mob) <= (double) f))    <- no hasPassenger test here
//!          if empty: return false
//!          if !ticker.tickDownAndCheck(level.getRandom()): return false
//!          look.set(new EntityTracker(e, true)); return true
//! Ticker(interval):  if interval.minInclusive() <= 1: throw IllegalArgumentException
//! tickDownAndCheck(random) (0..42):
//!          if ticksUntilNextStart == 0: ticksUntilNextStart = interval.sample(random) - 1; return false
//!          return --ticksUntilNextStart == 0
//! UniformInt.sample(random):  random.nextInt(max - min + 1) + min
//! ```
//!
//! So the ticker only counts on ticks where a candidate exists, draws from
//! the **level** random when it resets, and fires once per interval.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::behaviors::{nearest_visible_find_closest, EntitySelector};
use crate::brain::memory::{MemoryModuleType, MemoryValue, PositionTracker};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `net.minecraft.util.valueproviders.UniformInt`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UniformInt {
    pub min_inclusive: i32,
    pub max_inclusive: i32,
}

impl UniformInt {
    /// `UniformInt.of(min, max)`.
    #[must_use]
    pub fn of(min_inclusive: i32, max_inclusive: i32) -> Self {
        UniformInt {
            min_inclusive,
            max_inclusive,
        }
    }

    /// `sample(random)` on the level random: `nextInt(max - min + 1) + min`.
    pub fn sample_level<M: MobHost>(self, mob: &mut M) -> i32 {
        mob.level_random_next_int(self.max_inclusive - self.min_inclusive + 1) + self.min_inclusive
    }

    /// `sample(random)` on the **mob** random, which is what
    /// `RandomLookAround` and `AnimalMakeLove` take.
    pub fn sample_mob<M: MobHost>(self, mob: &mut M) -> i32 {
        use rustmc_worldgen::rng::RandomSource;
        mob.random()
            .next_i32_bound(self.max_inclusive - self.min_inclusive + 1)
            + self.min_inclusive
    }
}

/// `SetEntityLookTargetSometimes$Ticker`.
#[derive(Debug, Clone, Copy)]
pub struct Ticker {
    pub interval: UniformInt,
    ticks_until_next_start: i32,
}

impl Ticker {
    /// `Ticker(UniformInt)`.
    ///
    /// # Panics
    /// If `interval.minInclusive() <= 1`, as the constructor throws.
    #[must_use]
    pub fn new(interval: UniformInt) -> Self {
        assert!(
            interval.min_inclusive > 1,
            "IllegalArgumentException: interval min must exceed 1"
        );
        Ticker {
            interval,
            ticks_until_next_start: 0,
        }
    }

    /// `tickDownAndCheck(random)`.
    pub fn tick_down_and_check<M: MobHost>(&mut self, mob: &mut M) -> bool {
        if self.ticks_until_next_start == 0 {
            self.ticks_until_next_start = self.interval.sample_level(mob) - 1;
            false
        } else {
            self.ticks_until_next_start -= 1;
            self.ticks_until_next_start == 0
        }
    }

    /// `ticksUntilNextStart`, for tests.
    #[must_use]
    pub fn ticks_until_next_start(&self) -> i32 {
        self.ticks_until_next_start
    }
}

/// `SetEntityLookTargetSometimes`'s trigger.
#[derive(Debug, Clone, Copy)]
pub struct SetEntityLookTargetSometimes {
    pub selector: EntitySelector,
    pub max_dist_sqr: f32,
    pub ticker: Ticker,
}

impl SetEntityLookTargetSometimes {
    /// `create(float, UniformInt)`.
    #[must_use]
    pub fn create(max_dist: f32, interval: UniformInt) -> OneShot<Self> {
        Self::create_with(EntitySelector::Any, max_dist, interval)
    }

    /// `create(EntityType, float, UniformInt)`.
    #[must_use]
    pub fn create_kind(kind: &'static str, max_dist: f32, interval: UniformInt) -> OneShot<Self> {
        Self::create_with(EntitySelector::Kind(kind), max_dist, interval)
    }

    /// `create(float, UniformInt, Predicate)`.
    #[must_use]
    pub fn create_with(
        selector: EntitySelector,
        max_dist: f32,
        interval: UniformInt,
    ) -> OneShot<Self> {
        OneShot::new(SetEntityLookTargetSometimes {
            selector,
            max_dist_sqr: max_dist * max_dist,
            ticker: Ticker::new(interval),
        })
    }
}

impl<M: MobHost> Trigger<M> for SetEntityLookTargetSometimes {
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
        let Some(id) = nearest_visible_find_closest(brain, cx, |cx, id| {
            selector.test(cx, id) && cx.mob.distance_to_sqr_entity(id).is_some_and(|d| d <= max)
        }) else {
            return false;
        };
        if !self.ticker.tick_down_and_check(&mut cx.mob) {
            return false;
        }
        brain.set_memory_value(
            MemoryModuleType::LookTarget,
            MemoryValue::PositionTracker(PositionTracker::entity(id, true)),
        );
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![
            MemoryModuleType::LookTarget,
            MemoryModuleType::NearestVisibleLivingEntities,
        ]
    }

    fn name(&self) -> &'static str {
        "SetEntityLookTargetSometimes"
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
    use rustmc_worldgen::rng::{LegacyRandomSource, RandomSource};

    #[test]
    fn fires_once_per_sampled_interval_counting_only_ticks_with_a_candidate() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 13));
        cx.mob.others = vec![(EntityClass::Living, EntityFacts::living(4, 2.5, 1.0, 0.5))];
        cx.mob.visible = vec![4];
        let mut brain =
            Brain::<TestMob>::new(&[T::LookTarget, T::NearestVisibleLivingEntities], vec![]).state;
        brain.set_memory_value(
            T::NearestVisibleLivingEntities,
            MemoryValue::NearestVisible(NearestVisibleLivingEntities::new(vec![4])),
        );
        let mut twin = LegacyRandomSource::new(13 ^ 0x5DEECE66D);
        let n = twin.next_i32_bound(80 - 40 + 1) + 40;
        let mut b = SetEntityLookTargetSometimes::create(6.0, UniformInt::of(40, 80));
        assert!(
            !b.try_start(&mut brain, &mut cx, 0),
            "the first ask samples and answers false"
        );
        assert_eq!(b.trigger.ticker.ticks_until_next_start(), n - 1);
        for t in 1..(n - 1) {
            assert!(!b.try_start(&mut brain, &mut cx, i64::from(t)));
        }
        assert!(
            b.try_start(&mut brain, &mut cx, i64::from(n)),
            "fires on the n-th ask"
        );
        assert_eq!(
            brain
                .get_memory(T::LookTarget)
                .unwrap()
                .as_position_tracker(),
            Some(&PositionTracker::entity(4, true))
        );
    }

    #[test]
    #[should_panic(expected = "IllegalArgumentException")]
    fn an_interval_starting_at_one_is_refused() {
        let _ = Ticker::new(UniformInt::of(1, 5));
    }
}
