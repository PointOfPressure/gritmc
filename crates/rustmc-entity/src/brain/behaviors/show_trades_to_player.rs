//! `net.minecraft.world.entity.ai.behavior.ShowTradesToPlayer`, from the
//! 26.2 jar. `Behavior<Villager>` with entry `{INTERACTION_TARGET:
//! VALUE_PRESENT}` and `(minDuration, maxDuration)`.
//! `MAX_LOOK_TIME = 900`, `STARTING_LOOK_TIME = 40`.
//!
//! ```text
//! checkExtraStartConditions (0..86): t = INTERACTION_TARGET; present && t.is(PLAYER) && villager.isAlive() && t.isAlive()
//!                                    && !villager.isBaby() && villager.distanceToSqr(t) <= 17.0
//! canStillUse (0..37): checkExtraStartConditions && lookTime > 0 && INTERACTION_TARGET present
//! start (0..29): lookAtTarget(); cycleCounter = 0; displayIndex = 0; lookTime = 40
//! tick (0..61):  t = lookAtTarget(); findItemsToDisplay(t)
//!                if !displayItems.isEmpty(): displayCyclingItems() else: clearHeldItem(); lookTime = min(lookTime, 40)
//!                lookTime--
//! stop (0..27):  erase INTERACTION_TARGET; clearHeldItem(); playerItemStack = null
//! findItemsToDisplay (0..87): hand = t.getMainHandItem()
//!   if playerItemStack == null || !ItemStack.isSameItem(playerItemStack, hand): playerItemStack = hand; changed = true; displayItems.clear()
//!   if changed && !playerItemStack.isEmpty(): updateDisplayItems(); if !displayItems.isEmpty(): lookTime = 900; displayFirstItem()
//! updateDisplayItems: for offer in getOffers(): if !offer.isOutOfStock() && (isSameItem(stack, costA) || isSameItem(stack, costB)): displayItems.add(offer.assemble())
//! clearHeldItem: setItemSlot(MAINHAND, EMPTY); setDropChance(MAINHAND, 0.085f)
//! displayAsHeldItem(s): setItemSlot(MAINHAND, s); setDropChance(MAINHAND, 0.0f)
//! lookAtTarget: LOOK_TARGET = EntityTracker(t, true); return t
//! displayCyclingItems (0..87): if size >= 2 && ++cycleCounter >= 40: displayIndex++; cycleCounter = 0;
//!                              if displayIndex > size - 1: displayIndex = 0; displayAsHeldItem(displayItems[displayIndex])
//! ```
//!
//! `ItemStack.isSameItem` compares the `Item` only, so two stacks are the
//! same here when their ids match; `EMPTY` is `air`, and `None` stands
//! for it. `playerItemStack` starts `null` (`Option::None` outside), which
//! is distinct from holding nothing.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::memory::{MemoryModuleType, MemoryStatus, MemoryValue, PositionTracker};
use crate::brain::BrainState;
use crate::host::{ItemSpec, MobHost};

/// `MAX_LOOK_TIME`.
pub const MAX_LOOK_TIME: i32 = 900;
/// `STARTING_LOOK_TIME`.
pub const STARTING_LOOK_TIME: i32 = 40;

/// `ItemStack.isSameItem(a, b)`: `a.is(b.getItem())`; `None` is `EMPTY` (air).
#[must_use]
pub fn is_same_item(a: Option<&ItemSpec>, b: Option<&ItemSpec>) -> bool {
    a.map(|s| s.id) == b.map(|s| s.id)
}

/// `ShowTradesToPlayer`'s body; build one with [`ShowTradesToPlayer::new`].
#[derive(Debug, Clone, Default)]
pub struct ShowTradesToPlayer {
    /// `playerItemStack`: `None` is Java `null`; `Some(None)` is `EMPTY`.
    pub player_item_stack: Option<Option<ItemSpec>>,
    pub display_items: Vec<ItemSpec>,
    pub cycle_counter: i32,
    pub display_index: usize,
    pub look_time: i32,
}

impl ShowTradesToPlayer {
    /// `ShowTradesToPlayer(int, int)`.
    #[must_use]
    pub fn new(min_duration: i32, max_duration: i32) -> Behavior<ShowTradesToPlayer> {
        Behavior::with_durations(
            ShowTradesToPlayer::default(),
            vec![(
                MemoryModuleType::InteractionTarget,
                MemoryStatus::ValuePresent,
            )],
            min_duration,
            max_duration,
        )
    }

    fn target(brain: &BrainState) -> Option<i32> {
        brain
            .get_memory(MemoryModuleType::InteractionTarget)
            .and_then(MemoryValue::as_entity)
    }

    fn look_at_target(brain: &mut BrainState) -> i32 {
        let t = Self::target(brain).expect("INTERACTION_TARGET is VALUE_PRESENT");
        brain.set_memory_value(
            MemoryModuleType::LookTarget,
            MemoryValue::PositionTracker(PositionTracker::entity(t, true)),
        );
        t
    }

    fn clear_held_item<M: MobHost>(cx: &mut MobAi<M>) {
        cx.mob.set_main_hand_item(None);
        cx.mob.set_drop_chance_main_hand(0.085);
    }

    fn display_as_held_item<M: MobHost>(cx: &mut MobAi<M>, stack: ItemSpec) {
        cx.mob.set_main_hand_item(Some(&stack));
        cx.mob.set_drop_chance_main_hand(0.0);
    }

    fn find_items_to_display<M: MobHost>(&mut self, cx: &mut MobAi<M>, target: i32) {
        let mut changed = false;
        let hand = cx.mob.other_main_hand_item(target);
        let same = self
            .player_item_stack
            .as_ref()
            .is_some_and(|s| is_same_item(s.as_ref(), hand.as_ref()));
        if !same {
            self.player_item_stack = Some(hand);
            changed = true;
            self.display_items.clear();
        }
        let stack = self.player_item_stack.clone().flatten();
        if changed && stack.is_some() {
            self.update_display_items(cx, stack.as_ref());
            if !self.display_items.is_empty() {
                self.look_time = MAX_LOOK_TIME;
                Self::display_as_held_item(cx, self.display_items[0].clone());
            }
        }
    }

    fn update_display_items<M: MobHost>(&mut self, cx: &MobAi<M>, stack: Option<&ItemSpec>) {
        for offer in cx.mob.merchant_offers() {
            if !offer.out_of_stock
                && (is_same_item(stack, offer.cost_a.as_ref())
                    || is_same_item(stack, offer.cost_b.as_ref()))
            {
                self.display_items.push(offer.result);
            }
        }
    }

    fn display_cycling_items<M: MobHost>(&mut self, cx: &mut MobAi<M>) {
        if self.display_items.len() >= 2 {
            self.cycle_counter += 1;
            if self.cycle_counter >= 40 {
                self.display_index += 1;
                self.cycle_counter = 0;
            }
        }
        if self.display_index > self.display_items.len() - 1 {
            self.display_index = 0;
        }
        Self::display_as_held_item(cx, self.display_items[self.display_index].clone());
    }
}

impl<M: MobHost> BehaviorBody<M> for ShowTradesToPlayer {
    fn check_extra_start_conditions(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        let Some(t) = Self::target(brain) else {
            return false;
        };
        let Some(facts) = cx.mob.entity(t) else {
            return false;
        };
        facts.is_player
            && cx.mob.is_alive()
            && facts.alive
            && !cx.mob.is_baby()
            && cx.mob.distance_to_sqr_entity(t).is_some_and(|d| d <= 17.0)
    }

    fn can_still_use(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        self.check_extra_start_conditions(brain, cx)
            && self.look_time > 0
            && Self::target(brain).is_some()
    }

    fn start(&mut self, brain: &mut BrainState, _cx: &mut MobAi<M>, _time: i64) {
        Self::look_at_target(brain);
        self.cycle_counter = 0;
        self.display_index = 0;
        self.look_time = STARTING_LOOK_TIME;
    }

    fn tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        let t = Self::look_at_target(brain);
        self.find_items_to_display(cx, t);
        if self.display_items.is_empty() {
            Self::clear_held_item(cx);
            self.look_time = self.look_time.min(STARTING_LOOK_TIME);
        } else {
            self.display_cycling_items(cx);
        }
        self.look_time -= 1;
    }

    fn stop(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        brain.erase_memory(MemoryModuleType::InteractionTarget);
        Self::clear_held_item(cx);
        self.player_item_stack = None;
    }

    fn name(&self) -> &'static str {
        "ShowTradesToPlayer"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::{BehaviorControl, Status};
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::host::{EntityClass, EntityFacts, MerchantOfferFacts};
    use crate::testing::{GridLevel, TestMob};

    fn item(id: &'static str, count: i32) -> ItemSpec {
        ItemSpec {
            id,
            count,
            component: None,
        }
    }

    #[test]
    fn shows_the_offers_matching_the_players_hand_cycling_every_forty_ticks() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        cx.mob.others = vec![(EntityClass::Player, EntityFacts::player(9, 3.5, 1.0, 0.5))];
        cx.mob.offers = vec![
            MerchantOfferFacts {
                cost_a: Some(item("emerald", 1)),
                cost_b: None,
                result: item("bread", 6),
                out_of_stock: false,
            },
            MerchantOfferFacts {
                cost_a: Some(item("wheat", 20)),
                cost_b: None,
                result: item("emerald", 1),
                out_of_stock: false,
            },
            MerchantOfferFacts {
                cost_a: Some(item("emerald", 3)),
                cost_b: None,
                result: item("cookie", 18),
                out_of_stock: true,
            },
            MerchantOfferFacts {
                cost_a: Some(item("diamond", 1)),
                cost_b: Some(item("emerald", 1)),
                result: item("apple", 4),
                out_of_stock: false,
            },
        ];
        let mut brain = Brain::<TestMob>::new(&[T::InteractionTarget, T::LookTarget], vec![]).state;
        brain.set_memory_value(T::InteractionTarget, MemoryValue::Entity(9));
        let mut b = ShowTradesToPlayer::new(400, 400);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(b.body.look_time, 40);
        b.tick_or_stop(&mut brain, &mut cx, 1);
        assert_eq!(cx.mob.main_hand, None, "empty hand: nothing to show");
        assert_eq!(cx.mob.drop_chance, Some(0.085));
        assert_eq!(b.body.look_time, 39);
        cx.mob.other_main_hand.insert(9, item("emerald", 5));
        b.tick_or_stop(&mut brain, &mut cx, 2);
        assert_eq!(
            b.body.display_items,
            vec![item("bread", 6), item("apple", 4)],
            "in stock and costing emeralds, offer order"
        );
        assert_eq!(cx.mob.main_hand, Some(item("bread", 6)));
        assert_eq!(cx.mob.drop_chance, Some(0.0));
        assert_eq!(b.body.look_time, 899);
        for t in 3..=40 {
            b.tick_or_stop(&mut brain, &mut cx, t);
        }
        assert_eq!(b.body.cycle_counter, 39);
        assert_eq!(
            cx.mob.main_hand,
            Some(item("bread", 6)),
            "cycle 39: still the first"
        );
        b.tick_or_stop(&mut brain, &mut cx, 41);
        assert_eq!(
            cx.mob.main_hand,
            Some(item("apple", 4)),
            "cycle 40 moves on"
        );
        cx.mob.other_main_hand.insert(9, item("stick", 1));
        b.tick_or_stop(&mut brain, &mut cx, 42);
        assert!(b.body.display_items.is_empty());
        assert_eq!(cx.mob.main_hand, None);
        assert_eq!(b.body.look_time, 39, "min(lookTime, 40) then --");
        b.tick_or_stop(&mut brain, &mut cx, 401);
        assert_eq!(
            BehaviorControl::<TestMob>::status(&b),
            Status::Stopped,
            "400-tick duration"
        );
        assert!(!brain.has_memory_value(T::InteractionTarget));
        assert_eq!(b.body.player_item_stack, None);
    }
}
