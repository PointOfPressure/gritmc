//! `net.minecraft.world.entity.monster.piglin.RememberIfHoglinWasKilled`,
//! from the 26.2 jar. A `BehaviorBuilder` one-shot over
//! `group(present(ATTACK_TARGET), registered(HUNTED_RECENTLY))`.
//!
//! ```text
//! trigger (lambda$create$2, 0..53):
//!   target = get(ATTACK_TARGET)
//!   if target.is(EntityTypes.HOGLIN) && target.isDeadOrDying():
//!       huntedRecently.setWithExpiry(true, TIME_BETWEEN_HUNTS.sample(level.random))
//!   return true                                   <- ALWAYS, even when nothing was written
//! ```
//!
//! `PiglinAi.TIME_BETWEEN_HUNTS` is `TimeUtil.rangeOfSeconds(30, 120)` =
//! `UniformInt.of(600, 2400)` (the static initialiser at 6..13), one draw
//! on the **level** random, taken only on the write path — so a tick where
//! the target is not a dying hoglin consumes nothing.
//!
//! The trigger returns `true` unconditionally, so this one-shot always
//! counts as having run.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::behaviors::set_entity_look_target_sometimes::UniformInt;
use crate::brain::memory::{MemoryModuleType, MemoryStatus, MemoryValue};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `PiglinAi.TIME_BETWEEN_HUNTS` = `TimeUtil.rangeOfSeconds(30, 120)`.
pub const TIME_BETWEEN_HUNTS: UniformInt = UniformInt {
    min_inclusive: 600,
    max_inclusive: 2400,
};

/// `RememberIfHoglinWasKilled`'s trigger.
#[derive(Debug, Clone, Copy, Default)]
pub struct RememberIfHoglinWasKilled;

impl RememberIfHoglinWasKilled {
    /// `create()`.
    #[must_use]
    pub fn create() -> OneShot<Self> {
        OneShot::new(RememberIfHoglinWasKilled)
    }
}

impl<M: MobHost> Trigger<M> for RememberIfHoglinWasKilled {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        if !brain.check_memory(MemoryModuleType::AttackTarget, MemoryStatus::ValuePresent) {
            return false;
        }
        let Some(target) = brain
            .get_memory(MemoryModuleType::AttackTarget)
            .and_then(MemoryValue::as_entity)
        else {
            return false;
        };
        if cx.mob.entity_kind_is(target, "hoglin") && cx.mob.entity_dead_or_dying(target) {
            let ttl = TIME_BETWEEN_HUNTS.sample_level(&mut cx.mob);
            brain.set_memory_with_expiry(
                MemoryModuleType::HuntedRecently,
                MemoryValue::Bool(true),
                i64::from(ttl),
            );
        }
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![
            MemoryModuleType::AttackTarget,
            MemoryModuleType::HuntedRecently,
        ]
    }

    fn name(&self) -> &'static str {
        "RememberIfHoglinWasKilled"
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
    fn only_a_dying_hoglin_writes_and_only_then_is_a_level_draw_taken() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        cx.mob.others = vec![(EntityClass::Living, EntityFacts::living(7, 2.5, 1.0, 0.5))];
        cx.mob.entity_kinds = vec![(7, "hoglin")];
        let mut brain = Brain::<TestMob>::new(&[T::AttackTarget, T::HuntedRecently], vec![]).state;
        let mut b = RememberIfHoglinWasKilled::create();
        assert!(!b.try_start(&mut brain, &mut cx, 0), "no attack target");
        brain.set_memory_value(T::AttackTarget, MemoryValue::Entity(7));
        assert!(b.try_start(&mut brain, &mut cx, 0), "alive: still true");
        // `isDeadOrDying` reads the entity, so alive means no write.
        assert!(!brain.has_memory_value(T::HuntedRecently));
        cx.mob.others[0].1.alive = false;
        let mut twin = TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1);
        let expected = TIME_BETWEEN_HUNTS.sample_level(&mut twin);
        assert!(
            (600..=2400).contains(&expected),
            "rangeOfSeconds(30, 120) is 600..2400 ticks"
        );
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(
            brain.get_memory(T::HuntedRecently),
            Some(&MemoryValue::Bool(true))
        );
        // A wrong-typed dying target writes nothing and draws nothing.
        cx.mob.entity_kinds = vec![(7, "zombified_piglin")];
        brain.erase_memory(T::HuntedRecently);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(!brain.has_memory_value(T::HuntedRecently));
    }
}
