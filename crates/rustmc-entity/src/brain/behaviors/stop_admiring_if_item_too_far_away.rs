//! `net.minecraft.world.entity.monster.piglin.StopAdmiringIfItemTooFarAway`,
//! from the 26.2 jar. A `BehaviorBuilder` one-shot over
//! `group(present(ADMIRING_ITEM), registered(NEAREST_VISIBLE_WANTED_ITEM))`.
//!
//! ```text
//! trigger (lambda$create$2, 0..53):
//!   if !mob.getOffhandItem().isEmpty(): return false
//!   wanted = tryGet(NEAREST_VISIBLE_WANTED_ITEM)
//!   if wanted.isPresent() && wanted.get().closerThan(mob, (double) maxDistanceToItem): return false
//!   admiringItem.erase()
//!   return true
//! ```
//!
//! **Both refusals are the "keep admiring" case**, which reads backwards
//! from the name: it stops admiring when the off hand is *empty* and the
//! item is *not* near. A full off hand means the item was already picked
//! up, so the admiring is real and is left alone.
//!
//! The distance is `ItemEntity.closerThan(mob, d)`, a strict `<` on
//! `position()`, with the `int` parameter widened at the call (39: `i2d`).

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::memory::{MemoryModuleType, MemoryStatus, MemoryValue};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `StopAdmiringIfItemTooFarAway`'s trigger.
#[derive(Debug, Clone, Copy)]
pub struct StopAdmiringIfItemTooFarAway {
    /// `maxDistanceToItem`.
    pub max_distance_to_item: i32,
}

impl StopAdmiringIfItemTooFarAway {
    /// `create(int)`.
    #[must_use]
    pub fn create(max_distance_to_item: i32) -> OneShot<Self> {
        OneShot::new(StopAdmiringIfItemTooFarAway {
            max_distance_to_item,
        })
    }
}

impl<M: MobHost> Trigger<M> for StopAdmiringIfItemTooFarAway {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        if !brain.check_memory(MemoryModuleType::AdmiringItem, MemoryStatus::ValuePresent) {
            return false;
        }
        if cx.mob.offhand_item().is_some() {
            return false;
        }
        let wanted = brain
            .get_memory_if_registered(MemoryModuleType::NearestVisibleWantedItem)
            .and_then(MemoryValue::as_entity);
        let d = f64::from(self.max_distance_to_item);
        if wanted.is_some_and(|id| cx.mob.distance_to_sqr_entity(id).is_some_and(|s| s < d * d)) {
            return false;
        }
        brain.erase_memory(MemoryModuleType::AdmiringItem);
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![
            MemoryModuleType::AdmiringItem,
            MemoryModuleType::NearestVisibleWantedItem,
        ]
    }

    fn name(&self) -> &'static str {
        "StopAdmiringIfItemTooFarAway"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::host::{EntityClass, EntityFacts, ItemSpec};
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn a_full_off_hand_or_a_near_item_keeps_the_admiring() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        cx.mob.others = vec![(EntityClass::Living, EntityFacts::living(7, 4.5, 1.0, 0.5))];
        let mut brain =
            Brain::<TestMob>::new(&[T::AdmiringItem, T::NearestVisibleWantedItem], vec![]).state;
        let mut b = StopAdmiringIfItemTooFarAway::create(9);
        assert!(!b.try_start(&mut brain, &mut cx, 0), "not admiring");
        brain.set_memory_value(T::AdmiringItem, MemoryValue::Bool(true));
        cx.mob.offhand = Some(ItemSpec {
            id: "gold_ingot",
            count: 1,
            component: None,
        });
        assert!(!b.try_start(&mut brain, &mut cx, 0), "already picked it up");
        cx.mob.offhand = None;
        brain.set_memory_value(T::NearestVisibleWantedItem, MemoryValue::Entity(7));
        assert!(
            !b.try_start(&mut brain, &mut cx, 0),
            "4 blocks is closerThan 9"
        );
        cx.mob.others[0].1.x = 12.5;
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(!brain.has_memory_value(T::AdmiringItem));
        // An absent (but registered) wanted item is also "too far".
        brain.set_memory_value(T::AdmiringItem, MemoryValue::Bool(true));
        brain.erase_memory(T::NearestVisibleWantedItem);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(!brain.has_memory_value(T::AdmiringItem));
    }
}
