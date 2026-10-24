//! `net.minecraft.world.entity.ai.behavior.TradeWithVillager`, from the
//! 26.2 jar. `Behavior<Villager>` with entry `{INTERACTION_TARGET:
//! VALUE_PRESENT, NEAREST_VISIBLE_LIVING_ENTITIES: VALUE_PRESENT}` and the
//! default duration.
//!
//! ```text
//! checkExtraStartConditions: BehaviorUtils.targetIsValid(brain, INTERACTION_TARGET, VILLAGER);  canStillUse: the same
//! start (0..37):  other = INTERACTION_TARGET; lockGazeAndWalkToEachOther(villager, other, 0.5f, 2); trades = figureOutWhatIAmWillingToTrade(villager, other)
//! tick (0..174):  other; if villager.distanceToSqr(other) > 5.0: lockGazeAndWalkToEachOther(villager, other, 0.5f, 2)
//!   villager.gossip(level, other, time)
//!   farmer = profession.is(FARMER)
//!   if villager.hasExcessFood() && (farmer || other.wantsMoreFood()): throwHalfStack(villager, FOOD_POINTS.keySet(), other)
//!   if farmer && inventory.countItem(WHEAT) > WHEAT.getDefaultMaxStackSize() / 2: throwHalfStack(villager, {WHEAT}, other)
//!   if !trades.isEmpty() && inventory.hasAnyOf(trades): throwHalfStack(villager, trades, other)
//! stop: erase INTERACTION_TARGET
//! figureOutWhatIAmWillingToTrade(me, other): me.requestedItems().filter(i -> !other.requestedItems().contains(i)).toSet()
//! throwHalfStack(villager, items, target) (0..150): for each inventory slot in order:
//!   stack non-empty and items.contains(stack.getItem()):
//!     if stack.count > stack.getMaxStackSize() / 2: n = count / 2
//!     else if stack.count > 24: n = count - 24
//!     else: continue
//!     stack.shrink(n); thrown = new ItemStack(item, n); break
//!   if thrown non-empty: BehaviorUtils.throwItem(villager, thrown, target.position())
//! ```
//!
//! `FOOD_POINTS` keys (`Villager.<clinit>`): bread, potato, carrot,
//! beetroot. The trade set is a `HashSet<Item>` read only for membership.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::behaviors::behavior_utils::{lock_gaze_and_walk_to_each_other, target_is_valid};
use crate::brain::memory::{MemoryModuleType, MemoryStatus, MemoryValue};
use crate::brain::BrainState;
use crate::host::{ItemSpec, MobHost};

/// `Villager.FOOD_POINTS.keySet()`, in map order.
pub const FOOD_ITEMS: [&str; 4] = ["bread", "potato", "carrot", "beetroot"];

/// `TradeWithVillager`'s body; build one with [`TradeWithVillager::new`].
#[derive(Debug, Clone, Default)]
pub struct TradeWithVillager {
    /// `trades`, by item id.
    pub trades: Vec<&'static str>,
}

impl TradeWithVillager {
    /// `TradeWithVillager()`.
    #[must_use]
    pub fn new() -> Behavior<TradeWithVillager> {
        Behavior::new(
            TradeWithVillager::default(),
            vec![
                (
                    MemoryModuleType::InteractionTarget,
                    MemoryStatus::ValuePresent,
                ),
                (
                    MemoryModuleType::NearestVisibleLivingEntities,
                    MemoryStatus::ValuePresent,
                ),
            ],
        )
    }

    fn other(brain: &BrainState) -> i32 {
        brain
            .get_memory(MemoryModuleType::InteractionTarget)
            .and_then(MemoryValue::as_entity)
            .expect("INTERACTION_TARGET is VALUE_PRESENT")
    }

    /// `figureOutWhatIAmWillingToTrade(me, other)`.
    pub fn figure_out_what_i_am_willing_to_trade<M: MobHost>(
        cx: &MobAi<M>,
        other: i32,
    ) -> Vec<&'static str> {
        let theirs = cx.mob.other_requested_items(other);
        cx.mob
            .requested_items()
            .into_iter()
            .filter(|i| !theirs.contains(i))
            .collect()
    }

    /// `throwHalfStack(villager, items, target)`.
    pub fn throw_half_stack<M: MobHost>(cx: &mut MobAi<M>, items: &[&str], target: i32) {
        let mut thrown: Option<ItemSpec> = None;
        for (slot, stack) in cx.mob.inventory_items().into_iter().enumerate() {
            let Some(stack) = stack else {
                continue;
            };
            if !items.contains(&stack.id) {
                continue;
            }
            let n = if stack.count > cx.mob.max_stack_size(&stack) / 2 {
                stack.count / 2
            } else if stack.count > 24 {
                stack.count - 24
            } else {
                continue;
            };
            cx.mob.shrink_inventory_slot(slot, n);
            thrown = Some(ItemSpec {
                id: stack.id,
                count: n,
                component: None,
            });
            break;
        }
        if let Some(stack) = thrown {
            if let Some(t) = cx.mob.entity(target) {
                let pos = crate::phys::Vec3::new(t.x, t.y, t.z);
                cx.mob.throw_item(stack, pos);
            }
        }
    }
}

impl<M: MobHost> BehaviorBody<M> for TradeWithVillager {
    fn check_extra_start_conditions(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        target_is_valid(brain, cx, MemoryModuleType::InteractionTarget, "villager")
    }

    fn can_still_use(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        self.check_extra_start_conditions(brain, cx)
    }

    fn start(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        let other = Self::other(brain);
        lock_gaze_and_walk_to_each_other(brain, cx, other, 0.5, 2);
        self.trades = Self::figure_out_what_i_am_willing_to_trade(cx, other);
    }

    fn tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        let other = Self::other(brain);
        if cx
            .mob
            .distance_to_sqr_entity(other)
            .is_some_and(|d| d > 5.0)
        {
            lock_gaze_and_walk_to_each_other(brain, cx, other, 0.5, 2);
        }
        cx.mob.gossip_with(other, time);
        let farmer = cx.mob.villager_data().profession == "farmer";
        if cx.mob.has_excess_food() && (farmer || cx.mob.other_villager_wants_more_food(other)) {
            Self::throw_half_stack(cx, &FOOD_ITEMS, other);
        }
        if farmer {
            let wheat = ItemSpec {
                id: "wheat",
                count: 1,
                component: None,
            };
            let count: i32 = cx
                .mob
                .inventory_items()
                .into_iter()
                .flatten()
                .filter(|s| s.id == "wheat")
                .map(|s| s.count)
                .sum();
            if count > cx.mob.max_stack_size(&wheat) / 2 {
                Self::throw_half_stack(cx, &["wheat"], other);
            }
        }
        if !self.trades.is_empty()
            && cx
                .mob
                .inventory_items()
                .into_iter()
                .flatten()
                .any(|s| self.trades.contains(&s.id))
        {
            let trades = self.trades.clone();
            Self::throw_half_stack(cx, &trades, other);
        }
    }

    fn stop(&mut self, brain: &mut BrainState, _cx: &mut MobAi<M>, _time: i64) {
        brain.erase_memory(MemoryModuleType::InteractionTarget);
    }

    fn name(&self) -> &'static str {
        "TradeWithVillager"
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

    fn item(id: &'static str, count: i32) -> Option<ItemSpec> {
        Some(ItemSpec {
            id,
            count,
            component: None,
        })
    }

    #[test]
    fn a_farmer_with_excess_food_and_wheat_throws_half_stacks_and_its_spare_trades() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        cx.mob.others = vec![(EntityClass::Villager, EntityFacts::living(7, 4.5, 1.0, 0.5))];
        cx.mob.entity_kinds = vec![(7, "villager")];
        cx.mob.villager_data.profession = "farmer";
        cx.mob.excess_food = true;
        cx.mob.inventory = vec![
            item("bread", 20),
            item("wheat", 40),
            item("carrot", 50),
            item("beetroot", 10),
        ];
        cx.mob.requested = vec!["wheat", "potato", "bone_meal"];
        cx.mob.other_requested.insert(7, vec!["potato"]);
        let mut brain = Brain::<TestMob>::new(
            &[
                T::InteractionTarget,
                T::NearestVisibleLivingEntities,
                T::WalkTarget,
                T::LookTarget,
            ],
            vec![],
        )
        .state;
        brain.set_memory_value(T::InteractionTarget, MemoryValue::Entity(7));
        brain.set_memory_value(
            T::NearestVisibleLivingEntities,
            MemoryValue::NearestVisible(NearestVisibleLivingEntities::new(vec![7])),
        );
        cx.mob.visible = vec![7];
        let mut b = TradeWithVillager::new();
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(b.body.trades, vec!["wheat", "bone_meal"]);
        assert_eq!(
            cx.mob.calls,
            ["other_look 7 1", "other_walk_look_entity 7 1 0.5 2"]
        );
        cx.mob.calls.clear();
        b.tick_or_stop(&mut brain, &mut cx, 1);
        assert_eq!(
            cx.mob.calls,
            [
                "other_look 7 1",
                "other_walk_look_entity 7 1 0.5 2",
                "gossip 7 1",
                "throw 25xcarrot at 4.5 1 0.5",
                "throw 20xwheat at 4.5 1 0.5",
            ],
            "bread 20 is under both thresholds and carrot 50 halves; wheat 40 halves; the 20 left is under 24 so the trade pass throws nothing"
        );
    }
}
