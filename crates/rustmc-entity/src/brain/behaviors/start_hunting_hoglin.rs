//! `net.minecraft.world.entity.monster.piglin.StartHuntingHoglin`, from the
//! 26.2 jar. A `BehaviorBuilder` one-shot over
//! `group(present(NEAREST_VISIBLE_HUNTABLE_HOGLIN), absent(ANGRY_AT),
//! absent(HUNTED_RECENTLY), registered(NEAREST_VISIBLE_ADULT_PIGLINS))`.
//!
//! ```text
//! trigger (lambda$create$2, 0..74):
//!   if piglin.isBaby()
//!      || tryGet(NEAREST_VISIBLE_ADULT_PIGLINS)
//!           .filter(list -> list.stream().anyMatch(p -> p.getBrain().hasMemoryValue(ANGRY_AT)))
//!           .isPresent():
//!       return false
//!   hoglin = get(NEAREST_VISIBLE_HUNTABLE_HOGLIN)
//!   PiglinAi.setAngerTarget(level, piglin, hoglin)
//!   PiglinAi.dontKillAnyMoreHoglinsForAWhile(piglin)
//!   PiglinAi.broadcastAngerTarget(level, piglin, hoglin)
//!   tryGet(NEAREST_VISIBLE_ADULT_PIGLINS).ifPresent(list -> list.forEach(PiglinAi::dontKillAnyMoreHoglinsForAWhile))
//!   return true
//! ```
//!
//! **`dontKillAnyMoreHoglinsForAWhile` is called on this piglin AND then
//! again on every visible adult piglin** — and the second call's list may
//! contain this piglin, in which case its `HUNTED_RECENTLY` is written
//! twice with two separate `TIME_BETWEEN_HUNTS` draws, the later winning.
//! The three `PiglinAi` calls run in the order above, before the broadcast
//! to the pack.
//!
//! The refusal reads oddly and is faithful: a hunt is skipped when **any**
//! visible adult piglin is already angry at something, not only at a
//! hoglin.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::memory::{MemoryModuleType, MemoryStatus, MemoryValue};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `StartHuntingHoglin`'s trigger.
#[derive(Debug, Clone, Copy, Default)]
pub struct StartHuntingHoglin;

impl StartHuntingHoglin {
    /// `create()`.
    #[must_use]
    pub fn create() -> OneShot<Self> {
        OneShot::new(StartHuntingHoglin)
    }
}

impl<M: MobHost> Trigger<M> for StartHuntingHoglin {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        if !brain.check_memory(
            MemoryModuleType::NearestVisibleHuntableHoglin,
            MemoryStatus::ValuePresent,
        ) || !brain.check_memory(MemoryModuleType::AngryAt, MemoryStatus::ValueAbsent)
            || !brain.check_memory(MemoryModuleType::HuntedRecently, MemoryStatus::ValueAbsent)
        {
            return false;
        }
        let pack: Vec<i32> = brain
            .get_memory_if_registered(MemoryModuleType::NearestVisibleAdultPiglins)
            .and_then(MemoryValue::as_entities)
            .map(<[i32]>::to_vec)
            .unwrap_or_default();
        if cx.mob.is_baby() || pack.iter().any(|p| cx.mob.other_is_angry(*p)) {
            return false;
        }
        let Some(hoglin) = brain
            .get_memory(MemoryModuleType::NearestVisibleHuntableHoglin)
            .and_then(MemoryValue::as_entity)
        else {
            return false;
        };
        let me = cx.mob.id();
        cx.mob.piglin_set_anger_target(hoglin);
        cx.mob.piglin_dont_kill_any_more_hoglins_for_a_while(me);
        cx.mob.piglin_broadcast_anger_target(hoglin);
        for p in pack {
            cx.mob.piglin_dont_kill_any_more_hoglins_for_a_while(p);
        }
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![
            MemoryModuleType::NearestVisibleHuntableHoglin,
            MemoryModuleType::AngryAt,
            MemoryModuleType::HuntedRecently,
            MemoryModuleType::NearestVisibleAdultPiglins,
        ]
    }

    fn name(&self) -> &'static str {
        "StartHuntingHoglin"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn an_angry_packmate_or_a_baby_blocks_and_the_calls_run_in_the_jars_order() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut brain = Brain::<TestMob>::new(
            &[
                T::NearestVisibleHuntableHoglin,
                T::AngryAt,
                T::HuntedRecently,
                T::NearestVisibleAdultPiglins,
            ],
            vec![],
        )
        .state;
        let mut b = StartHuntingHoglin::create();
        assert!(!b.try_start(&mut brain, &mut cx, 0), "no huntable hoglin");
        brain.set_memory_value(T::NearestVisibleHuntableHoglin, MemoryValue::Entity(9));
        brain.set_memory_value(
            T::NearestVisibleAdultPiglins,
            MemoryValue::Entities(vec![2, 3]),
        );
        cx.mob.baby = true;
        assert!(!b.try_start(&mut brain, &mut cx, 0), "a baby never hunts");
        cx.mob.baby = false;
        cx.mob.angry_others = vec![3];
        assert!(
            !b.try_start(&mut brain, &mut cx, 0),
            "any angry adult piglin blocks"
        );
        cx.mob.angry_others.clear();
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(
            cx.mob.calls,
            vec![
                "set_anger_target 9".to_string(),
                "dont_kill_hoglins 1".to_string(),
                "broadcast_anger 9".to_string(),
                "dont_kill_hoglins 2".to_string(),
                "dont_kill_hoglins 3".to_string(),
            ]
        );
        brain.set_memory_value(T::HuntedRecently, MemoryValue::Bool(true));
        assert!(!b.try_start(&mut brain, &mut cx, 0), "hunted recently");
    }
}
