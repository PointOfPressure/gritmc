//! `net.minecraft.world.entity.ai.behavior.CopyMemoryWithExpiry`, from the
//! 26.2 jar. A `BehaviorBuilder` one-shot over `group(present(source),
//! absent(target))`.
//!
//! ```text
//! lambda$create$2 (0..34):
//!   if !predicate.test(mob): return false
//!   target.setWithExpiry(source.get(), uniform.sample(level.getRandom()))   <- LEVEL random
//!   return true
//! ```
//!
//! The predicate runs before the draw, so a refused trigger draws nothing.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::behaviors::set_entity_look_target_sometimes::UniformInt;
use crate::brain::behaviors::MobPredicate;
use crate::brain::memory::MemoryModuleType;
use crate::brain::BrainState;
use crate::host::MobHost;

/// `CopyMemoryWithExpiry`'s trigger.
pub struct CopyMemoryWithExpiry<M: MobHost> {
    pub predicate: MobPredicate<M>,
    pub source: MemoryModuleType,
    pub target: MemoryModuleType,
    pub expiry: UniformInt,
}

impl<M: MobHost> CopyMemoryWithExpiry<M> {
    /// `create(Predicate<E>, MemoryModuleType<? extends T>, MemoryModuleType<T>, UniformInt)`.
    #[must_use]
    pub fn create(
        predicate: MobPredicate<M>,
        source: MemoryModuleType,
        target: MemoryModuleType,
        expiry: UniformInt,
    ) -> OneShot<Self> {
        OneShot::new(CopyMemoryWithExpiry {
            predicate,
            source,
            target,
            expiry,
        })
    }
}

impl<M: MobHost> Trigger<M> for CopyMemoryWithExpiry<M> {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        let Some(Some(v)) = brain.memory_internal(self.source) else {
            return false;
        };
        let v = v.clone();
        if brain.memory_internal(self.target) != Some(None) {
            return false;
        }
        if !(self.predicate)(cx) {
            return false;
        }
        let ttl = i64::from(self.expiry.sample_level(&mut cx.mob));
        brain.set_memory_with_expiry(self.target, v, ttl);
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![self.source, self.target]
    }

    fn name(&self) -> &'static str {
        "CopyMemoryWithExpiry"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::{MemoryModuleType as T, MemoryValue};
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};
    use rustmc_worldgen::rng::{LegacyRandomSource, RandomSource};

    #[test]
    fn copies_with_a_level_random_expiry_only_past_the_predicate() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 11));
        let mut twin = LegacyRandomSource::new(11 ^ 0x5DEECE66D);
        let mut brain = Brain::<TestMob>::new(&[T::NearestHostile, T::AvoidTarget], vec![]).state;
        let mut b = CopyMemoryWithExpiry::create(
            Box::new(|cx: &mut MobAi<TestMob>| cx.mob.baby),
            T::NearestHostile,
            T::AvoidTarget,
            UniformInt::of(5, 20),
        );
        assert!(!b.try_start(&mut brain, &mut cx, 0), "source absent");
        brain.set_memory_value(T::NearestHostile, MemoryValue::Entity(9));
        assert!(
            !b.try_start(&mut brain, &mut cx, 0),
            "predicate false: no draw"
        );
        cx.mob.baby = true;
        assert!(b.try_start(&mut brain, &mut cx, 0));
        let ttl = twin.next_i32_bound(16) + 5;
        assert_eq!(
            brain.get_memory(T::AvoidTarget),
            Some(&MemoryValue::Entity(9))
        );
        assert_eq!(brain.time_until_expiry(T::AvoidTarget), i64::from(ttl));
        assert!(!b.try_start(&mut brain, &mut cx, 0), "target now present");
    }
}
