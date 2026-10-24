//! `net.minecraft.world.entity.monster.piglin.StopHoldingItemIfNoLongerAdmiring`,
//! from the 26.2 jar. A `BehaviorBuilder` one-shot over
//! `group(absent(ADMIRING_ITEM))`.
//!
//! ```text
//! trigger (lambda$create$2, 0..32):
//!   if piglin.getOffhandItem().isEmpty()
//!      || piglin.getOffhandItem().has(DataComponents.BLOCKS_ATTACKS): return false
//!   PiglinAi.stopHoldingOffHandItem(level, piglin, true)
//!   return true
//! ```
//!
//! The shield exemption is `DataComponents.BLOCKS_ATTACKS` in 26.2 — a
//! component test, not `instanceof ShieldItem` — so anything carrying that
//! component stays in the off hand. `shouldBarter` is the constant `true`.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::memory::{MemoryModuleType, MemoryStatus};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `StopHoldingItemIfNoLongerAdmiring`'s trigger.
#[derive(Debug, Clone, Copy, Default)]
pub struct StopHoldingItemIfNoLongerAdmiring;

impl StopHoldingItemIfNoLongerAdmiring {
    /// `create()`.
    #[must_use]
    pub fn create() -> OneShot<Self> {
        OneShot::new(StopHoldingItemIfNoLongerAdmiring)
    }
}

impl<M: MobHost> Trigger<M> for StopHoldingItemIfNoLongerAdmiring {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        if !brain.check_memory(MemoryModuleType::AdmiringItem, MemoryStatus::ValueAbsent) {
            return false;
        }
        let Some(item) = cx.mob.offhand_item() else {
            return false;
        };
        if cx.mob.item_blocks_attacks(&item) {
            return false;
        }
        cx.mob.piglin_stop_holding_off_hand_item(true);
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![MemoryModuleType::AdmiringItem]
    }

    fn name(&self) -> &'static str {
        "StopHoldingItemIfNoLongerAdmiring"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::{MemoryModuleType as T, MemoryValue};
    use crate::brain::Brain;
    use crate::host::ItemSpec;
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn an_empty_hand_and_a_blocks_attacks_item_both_refuse() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut brain = Brain::<TestMob>::new(&[T::AdmiringItem], vec![]).state;
        let mut b = StopHoldingItemIfNoLongerAdmiring::create();
        assert!(!b.try_start(&mut brain, &mut cx, 0), "empty off hand");
        cx.mob.offhand = Some(ItemSpec {
            id: "shield",
            count: 1,
            component: None,
        });
        cx.mob.blocks_attacks = vec!["shield"];
        assert!(!b.try_start(&mut brain, &mut cx, 0), "BLOCKS_ATTACKS");
        cx.mob.offhand = Some(ItemSpec {
            id: "gold_ingot",
            count: 1,
            component: None,
        });
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(cx
            .mob
            .calls
            .contains(&"stop_holding_off_hand true".to_string()));
        brain.set_memory_value(T::AdmiringItem, MemoryValue::Bool(true));
        assert!(!b.try_start(&mut brain, &mut cx, 0), "still admiring");
    }
}
