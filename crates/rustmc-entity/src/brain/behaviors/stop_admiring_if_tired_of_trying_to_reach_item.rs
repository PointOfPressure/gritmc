//! `net.minecraft.world.entity.monster.piglin.StopAdmiringIfTiredOfTryingToReachItem`,
//! from the 26.2 jar. A `BehaviorBuilder` one-shot over
//! `group(present(ADMIRING_ITEM), present(NEAREST_VISIBLE_WANTED_ITEM),
//! registered(TIME_TRYING_TO_REACH_ADMIRE_ITEM),
//! registered(DISABLE_WALK_TO_ADMIRE_ITEM))`.
//!
//! ```text
//! trigger (lambda$create$2, 0..93):
//!   if !mob.getOffhandItem().isEmpty(): return false
//!   t = tryGet(TIME_TRYING_TO_REACH_ADMIRE_ITEM)
//!   if t.isEmpty(): time.set(0)
//!   else if t.get() > maxTimeToReachItem:
//!            admiringItem.erase(); time.erase()
//!            disableWalk.setWithExpiry(true, disableTime)
//!        else: time.set(t.get() + 1)
//!   return true                                     <- ALWAYS true past the off-hand test
//! ```
//!
//! **The counter is compared with a strict `>`** (`if_icmple` at 55 jumps
//! to the increment), so `maxTimeToReachItem` itself is still a tick of
//! trying and the give-up happens on the value after it.
//!
//! **The trigger returns `true` on every path past the off-hand test**,
//! including the plain increment — so this one-shot "succeeds" every tick
//! it runs, which is what keeps it from being skipped in a `RunOne`.
//!
//! The give-up sets `DISABLE_WALK_TO_ADMIRE_ITEM` with an expiry, which is
//! the memory `StartAdmiringItemIfSeen` refuses on, so the two together
//! are the barter cool-off.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::memory::{MemoryModuleType, MemoryStatus, MemoryValue};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `StopAdmiringIfTiredOfTryingToReachItem`'s trigger.
#[derive(Debug, Clone, Copy)]
pub struct StopAdmiringIfTiredOfTryingToReachItem {
    /// `maxTimeToReachItem`.
    pub max_time_to_reach_item: i32,
    /// `disableTime`.
    pub disable_time: i32,
}

impl StopAdmiringIfTiredOfTryingToReachItem {
    /// `create(int, int)`.
    #[must_use]
    pub fn create(max_time_to_reach_item: i32, disable_time: i32) -> OneShot<Self> {
        OneShot::new(StopAdmiringIfTiredOfTryingToReachItem {
            max_time_to_reach_item,
            disable_time,
        })
    }
}

impl<M: MobHost> Trigger<M> for StopAdmiringIfTiredOfTryingToReachItem {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        if !brain.check_memory(MemoryModuleType::AdmiringItem, MemoryStatus::ValuePresent)
            || !brain.check_memory(
                MemoryModuleType::NearestVisibleWantedItem,
                MemoryStatus::ValuePresent,
            )
        {
            return false;
        }
        if cx.mob.offhand_item().is_some() {
            return false;
        }
        let t = brain
            .get_memory_if_registered(MemoryModuleType::TimeTryingToReachAdmireItem)
            .and_then(|v| match v {
                MemoryValue::Int(n) => Some(*n),
                _ => None,
            });
        match t {
            None => brain.set_memory_value(
                MemoryModuleType::TimeTryingToReachAdmireItem,
                MemoryValue::Int(0),
            ),
            Some(n) if n > self.max_time_to_reach_item => {
                brain.erase_memory(MemoryModuleType::AdmiringItem);
                brain.erase_memory(MemoryModuleType::TimeTryingToReachAdmireItem);
                brain.set_memory_with_expiry(
                    MemoryModuleType::DisableWalkToAdmireItem,
                    MemoryValue::Bool(true),
                    i64::from(self.disable_time),
                );
            }
            Some(n) => brain.set_memory_value(
                MemoryModuleType::TimeTryingToReachAdmireItem,
                MemoryValue::Int(n + 1),
            ),
        }
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![
            MemoryModuleType::AdmiringItem,
            MemoryModuleType::NearestVisibleWantedItem,
            MemoryModuleType::TimeTryingToReachAdmireItem,
            MemoryModuleType::DisableWalkToAdmireItem,
        ]
    }

    fn name(&self) -> &'static str {
        "StopAdmiringIfTiredOfTryingToReachItem"
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
    fn the_counter_starts_at_zero_counts_up_and_gives_up_strictly_past_the_max() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut brain = Brain::<TestMob>::new(
            &[
                T::AdmiringItem,
                T::NearestVisibleWantedItem,
                T::TimeTryingToReachAdmireItem,
                T::DisableWalkToAdmireItem,
            ],
            vec![],
        )
        .state;
        brain.set_memory_value(T::AdmiringItem, MemoryValue::Bool(true));
        brain.set_memory_value(T::NearestVisibleWantedItem, MemoryValue::Entity(7));
        let mut b = StopAdmiringIfTiredOfTryingToReachItem::create(2, 200);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(
            brain.get_memory(T::TimeTryingToReachAdmireItem),
            Some(&MemoryValue::Int(0)),
            "the absent case seeds 0 rather than 1"
        );
        for expect in [1, 2, 3] {
            assert!(b.try_start(&mut brain, &mut cx, 0));
            assert_eq!(
                brain.get_memory(T::TimeTryingToReachAdmireItem),
                Some(&MemoryValue::Int(expect)),
                "max itself is still a tick of trying"
            );
        }
        // 3 > 2: the give-up.
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(!brain.has_memory_value(T::AdmiringItem));
        assert!(!brain.has_memory_value(T::TimeTryingToReachAdmireItem));
        assert_eq!(
            brain.get_memory(T::DisableWalkToAdmireItem),
            Some(&MemoryValue::Bool(true))
        );
    }
}
