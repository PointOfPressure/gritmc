//! `net.minecraft.world.entity.monster.piglin.StartAdmiringItemIfSeen`,
//! from the 26.2 jar. A `BehaviorBuilder` one-shot over
//! `group(present(NEAREST_VISIBLE_WANTED_ITEM), absent(ADMIRING_ITEM),
//! absent(ADMIRING_DISABLED), absent(DISABLE_WALK_TO_ADMIRE_ITEM))`
//! (`lambda$create$0` 2..40).
//!
//! ```text
//! trigger (lambda$create$2, 0..34):
//!   if !PiglinAi.isLovedItem(get(item).getItem()): return false
//!   admiringItem.setWithExpiry(true, memoryTimeToLive)
//!   return true
//! ```
//!
//! Only the first accessor is read; the other three are pure guards. The
//! write is `setWithExpiry`, so `ADMIRING_ITEM` decays on its own rather
//! than being erased by anything here.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::memory::{MemoryModuleType, MemoryStatus, MemoryValue};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `StartAdmiringItemIfSeen`'s trigger.
#[derive(Debug, Clone, Copy)]
pub struct StartAdmiringItemIfSeen {
    /// `memoryTimeToLive`.
    pub memory_time_to_live: i32,
}

impl StartAdmiringItemIfSeen {
    /// `create(int)`.
    #[must_use]
    pub fn create(memory_time_to_live: i32) -> OneShot<Self> {
        OneShot::new(StartAdmiringItemIfSeen {
            memory_time_to_live,
        })
    }
}

impl<M: MobHost> Trigger<M> for StartAdmiringItemIfSeen {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        if !brain.check_memory(
            MemoryModuleType::NearestVisibleWantedItem,
            MemoryStatus::ValuePresent,
        ) || !brain.check_memory(MemoryModuleType::AdmiringItem, MemoryStatus::ValueAbsent)
            || !brain.check_memory(
                MemoryModuleType::AdmiringDisabled,
                MemoryStatus::ValueAbsent,
            )
            || !brain.check_memory(
                MemoryModuleType::DisableWalkToAdmireItem,
                MemoryStatus::ValueAbsent,
            )
        {
            return false;
        }
        let Some(item) = brain
            .get_memory(MemoryModuleType::NearestVisibleWantedItem)
            .and_then(MemoryValue::as_entity)
            .and_then(|id| cx.mob.item_entity_stack(id))
        else {
            return false;
        };
        if !cx.mob.piglin_is_loved_item(&item) {
            return false;
        }
        brain.set_memory_with_expiry(
            MemoryModuleType::AdmiringItem,
            MemoryValue::Bool(true),
            i64::from(self.memory_time_to_live),
        );
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![
            MemoryModuleType::NearestVisibleWantedItem,
            MemoryModuleType::AdmiringItem,
            MemoryModuleType::AdmiringDisabled,
            MemoryModuleType::DisableWalkToAdmireItem,
        ]
    }

    fn name(&self) -> &'static str {
        "StartAdmiringItemIfSeen"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::host::ItemSpec;
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn only_a_loved_item_starts_admiring_and_each_guard_blocks_alone() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        cx.mob.item_entities = vec![(
            7,
            ItemSpec {
                id: "gold_ingot",
                count: 1,
                component: None,
            },
        )];
        let mut brain = Brain::<TestMob>::new(
            &[
                T::NearestVisibleWantedItem,
                T::AdmiringItem,
                T::AdmiringDisabled,
                T::DisableWalkToAdmireItem,
            ],
            vec![],
        )
        .state;
        let mut b = StartAdmiringItemIfSeen::create(120);
        assert!(!b.try_start(&mut brain, &mut cx, 0), "no wanted item");
        brain.set_memory_value(T::NearestVisibleWantedItem, MemoryValue::Entity(7));
        assert!(!b.try_start(&mut brain, &mut cx, 0), "not a loved item");
        cx.mob.loved_items = vec!["gold_ingot"];
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(
            brain.get_memory(T::AdmiringItem),
            Some(&MemoryValue::Bool(true))
        );
        brain.erase_memory(T::AdmiringItem);
        for guard in [T::AdmiringDisabled, T::DisableWalkToAdmireItem] {
            brain.set_memory_value(guard, MemoryValue::Unit);
            assert!(!b.try_start(&mut brain, &mut cx, 0), "{guard:?} blocks");
            brain.erase_memory(guard);
        }
        assert!(b.try_start(&mut brain, &mut cx, 0));
        // The write is setWithExpiry: the slot carries a countdown rather
        // than living until something erases it.
        assert_eq!(brain.time_until_expiry(T::AdmiringItem), 120);
    }
}
