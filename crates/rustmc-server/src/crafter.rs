//! `CrafterBlockEntity` — nine slots, each of which can be switched off.
//!
//! Transcribed from `.isSlotDisabled(int)` (offsets 0-31),
//! `.setSlotState(int, boolean)` (0-32), `.slotCanBeDisabled(int)` (0-33),
//! `.canPlaceItem(int, ItemStack)` (0-70), `.smallerStackExist(int,
//! ItemStack, int)` (0-64), `.setItem(int, ItemStack)` (0-20),
//! `.getRedstoneSignal()` (0-43) and `.serverTick` (0-46), plus
//! `CrafterBlock.dispenseFrom`'s ticket-setting half, in the pinned 26.2 jar.
//!
//! # The disabled flags live in the `ContainerData`, not in a field
//!
//! `containerData` has ten entries: index 0-8 is "slot *i* is disabled" as a
//! 0/1 int, and index 9 is `triggered`. `setSlotState(i, enabled)` writes the
//! **inverse** — `enabled ? 0 : 1` at offsets 14-23 — so the argument is
//! *enabled* and the storage is *disabled*, one negation apart. That
//! inversion is the single most invertible thing in this file and is why
//! [`Crafter::set_slot_state`] keeps vanilla's argument sense.
//!
//! # A slot can only be disabled while it is empty
//!
//! `slotCanBeDisabled` is `slot > -1 && slot < 9 && items.get(slot)
//! .isEmpty()`, and `setSlotState` returns early when it fails. So a click on
//! an occupied slot does nothing at all — and `setItem` closes the other
//! direction by **re-enabling** a disabled slot before writing to it (offsets
//! 0-14), which is how a hopper feeding a crafter cannot leave an item in a
//! slot the crafter is ignoring.
//!
//! # `canPlaceItem` is the even-fill rule
//!
//! Offsets 0-70, in order: a disabled slot refuses; a slot already at its
//! stack limit refuses; an **empty** slot accepts; otherwise the slot accepts
//! only if no *later* slot holds a smaller stack of the same item. That last
//! clause — `smallerStackExist`, which walks `slot + 1 .. 9` — is what makes
//! a crafter fill its nine slots evenly instead of stacking the first one to
//! sixty-four, and it is deliberately one-directional: it never looks left.

use crate::item::ItemStack;

/// `getContainerSize()` — `bipush 9`.
pub const CRAFTER_SLOTS: usize = 9;

/// The `containerData` index holding `triggered` — `bipush 9` in
/// `isTriggered` and `setTriggered`.
pub const TRIGGERED_INDEX: usize = 9;

/// How long `CRAFTING` stays true after a craft — `bipush 6` at
/// `CrafterBlock.dispenseFrom` offset 106.
pub const CRAFTING_TICKS: i32 = 6;

/// The `level_event` a crafter fires when it has nothing to make — `sipush
/// 1050` at `dispenseFrom` offsets 50 and 95, with data 0.
///
/// It is sent from **two** places: no recipe matched, and a recipe matched
/// whose assembled stack is empty. Both look identical on the wire.
pub const FAILED_CRAFT_LEVEL_EVENT: i32 = 1050;

/// One crafter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Crafter {
    /// The nine slots.
    pub items: Vec<ItemStack>,
    /// `containerData[0..9]` — true where the slot is **disabled**.
    pub disabled: [bool; CRAFTER_SLOTS],
    /// `containerData[9]`.
    pub triggered: bool,
    /// `craftingTicksRemaining`.
    pub crafting_ticks_remaining: i32,
}

impl Default for Crafter {
    fn default() -> Self {
        Self {
            items: vec![ItemStack::empty(); CRAFTER_SLOTS],
            disabled: [false; CRAFTER_SLOTS],
            triggered: false,
            crafting_ticks_remaining: 0,
        }
    }
}

impl Crafter {
    /// `isSlotDisabled(int)` — offsets 0-31.
    ///
    /// Out of range answers **false**, not a panic: the `iflt 30` and
    /// `if_icmpge 30` both jump to `iconst_0; ireturn`.
    #[must_use]
    pub fn is_slot_disabled(&self, slot: i32) -> bool {
        usize::try_from(slot)
            .ok()
            .and_then(|i| self.disabled.get(i))
            .copied()
            .unwrap_or(false)
    }

    /// `slotCanBeDisabled(int)` — offsets 0-33.
    ///
    /// In range **and** the slot is empty. Note the lower bound is written
    /// `slot > -1` rather than `slot >= 0`; same thing, and worth not
    /// "correcting" into a different comparison by accident.
    #[must_use]
    pub fn slot_can_be_disabled(&self, slot: i32) -> bool {
        usize::try_from(slot)
            .ok()
            .and_then(|i| self.items.get(i))
            .is_some_and(ItemStack::is_empty)
    }

    /// `setSlotState(int slot, boolean enabled)` — offsets 0-32.
    ///
    /// **The argument is `enabled` and the stored value is `disabled`.** A
    /// call with `true` clears the flag. Returns whether anything was written,
    /// which is `slotCanBeDisabled` and is also the condition for
    /// `setChanged()`.
    pub fn set_slot_state(&mut self, slot: i32, enabled: bool) -> bool {
        if !self.slot_can_be_disabled(slot) {
            return false;
        }
        let Ok(i) = usize::try_from(slot) else {
            return false;
        };
        self.disabled[i] = !enabled;
        true
    }

    /// `smallerStackExist(int count, ItemStack stack, int fromSlot)` —
    /// offsets 0-64.
    ///
    /// Walks `fromSlot + 1 ..< 9` only. A slot is a hit when it is **not**
    /// disabled and either it is empty **or** it holds fewer than `count` of
    /// the same item.
    ///
    /// The empty case is easy to lose: offsets 29-34 jump straight to the
    /// `iconst_1; ireturn` on `isEmpty`, so an empty later slot means "yes,
    /// something smaller exists" without any item comparison at all.
    #[must_use]
    pub fn smaller_stack_exist(&self, count: i32, stack: &ItemStack, from_slot: usize) -> bool {
        for i in (from_slot + 1)..CRAFTER_SLOTS {
            if self.is_slot_disabled(i32::try_from(i).unwrap_or(-1)) {
                continue;
            }
            let other = &self.items[i];
            if other.is_empty()
                || (other.count() < count && ItemStack::is_same_item_same_components(other, stack))
            {
                return true;
            }
        }
        false
    }

    /// `canPlaceItem(int, ItemStack)` — offsets 0-70.
    #[must_use]
    pub fn can_place_item(&self, slot: usize, _stack: &ItemStack) -> bool {
        if slot >= CRAFTER_SLOTS || self.disabled[slot] {
            return false;
        }
        let existing = &self.items[slot];
        let count = existing.count();
        if count >= existing.max_stack_size() {
            return false;
        }
        if existing.is_empty() {
            return true;
        }
        !self.smaller_stack_exist(count, existing, slot)
    }

    /// `setItem(int, ItemStack)` — offsets 0-20.
    ///
    /// **Re-enables the slot first**, then writes. So a disabled slot that
    /// receives an item stops being disabled, and the `setChanged` from
    /// `setSlotState` fires before the write rather than after it.
    pub fn set_item(&mut self, slot: usize, stack: ItemStack) {
        if slot >= CRAFTER_SLOTS {
            return;
        }
        if self.is_slot_disabled(i32::try_from(slot).unwrap_or(-1)) {
            // `setSlotState(slot, true)`. Its own `slotCanBeDisabled` guard
            // passes here precisely because a disabled slot is empty.
            self.set_slot_state(i32::try_from(slot).unwrap_or(-1), true);
        }
        self.items[slot] = stack;
    }

    /// `getRedstoneSignal()` — offsets 0-43.
    ///
    /// Counts slots that are **empty or disabled**, so a full crafter reads 0
    /// and an empty one reads 9. It is a count and not a scaled fraction, and
    /// the `ifeq 36` structure means a slot that is both empty and disabled is
    /// counted once.
    #[must_use]
    pub fn redstone_signal(&self) -> i32 {
        let mut n = 0;
        for i in 0..CRAFTER_SLOTS {
            if self.items[i].is_empty() || self.disabled[i] {
                n += 1;
            }
        }
        n
    }

    /// `CrafterBlockEntity.serverTick` — offsets 0-46.
    ///
    /// **Decrements into a local first and returns when it goes negative**, so
    /// the field is never written below 0 and a crafter at rest does nothing
    /// at all. When the decrement lands on exactly 0 the block's `CRAFTING`
    /// is cleared with `setBlock(..., 3)`.
    ///
    /// Returns whether `CRAFTING` should be written false this tick.
    pub fn server_tick(&mut self) -> bool {
        let next = self.crafting_ticks_remaining - 1;
        if next < 0 {
            return false;
        }
        self.crafting_ticks_remaining = next;
        next == 0
    }

    /// `CrafterBlock.dispenseFrom`'s bookkeeping half — offsets 104-131.
    ///
    /// A successful craft sets `craftingTicksRemaining` to 6 and writes
    /// `CRAFTING = true` with **flag 2**, where `serverTick` clears it with
    /// flag 3. The asymmetry is vanilla's and is transcribed rather than
    /// smoothed: flag 2 sends the block change without a neighbour update, so
    /// starting a craft does not re-trigger the redstone that started it.
    pub fn begin_craft(&mut self) {
        self.crafting_ticks_remaining = CRAFTING_TICKS;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The argument is `enabled` and the storage is `disabled`. Getting this
    /// backwards is invisible in a round trip, so the assert is on the field.
    #[test]
    fn set_slot_state_takes_enabled_and_stores_disabled() {
        let mut c = Crafter::default();
        assert!(c.set_slot_state(4, false));
        assert!(c.disabled[4]);
        assert!(c.is_slot_disabled(4));
        assert!(c.set_slot_state(4, true));
        assert!(!c.disabled[4]);
    }

    /// A slot with an item in it cannot be disabled, and the call reports so
    /// rather than silently succeeding.
    #[test]
    fn an_occupied_slot_cannot_be_disabled() {
        let mut c = Crafter::default();
        c.items[2] = ItemStack::one("minecraft:stick");
        assert!(!c.set_slot_state(2, false));
        assert!(!c.disabled[2]);
        // The empty slot beside it can be, so this is the emptiness test and
        // not a blanket refusal.
        assert!(c.set_slot_state(3, false));
    }

    /// Out of range is false and not a panic, in both directions.
    #[test]
    fn an_out_of_range_slot_is_not_disabled_and_cannot_be() {
        let c = Crafter::default();
        assert!(!c.is_slot_disabled(-1));
        assert!(!c.is_slot_disabled(9));
        assert!(!c.slot_can_be_disabled(-1));
        assert!(!c.slot_can_be_disabled(9));
    }

    /// Writing to a disabled slot re-enables it. That is what stops an item
    /// sitting in a slot the crafter ignores.
    #[test]
    fn writing_to_a_disabled_slot_re_enables_it() {
        let mut c = Crafter::default();
        c.set_slot_state(0, false);
        assert!(c.disabled[0]);
        c.set_item(0, ItemStack::one("minecraft:stick"));
        assert!(!c.disabled[0]);
        assert_eq!(c.items[0].item(), "minecraft:stick");
    }

    /// The even-fill rule: slot 0 refuses a second item while slot 1 is
    /// emptier, and accepts once slot 1 has caught up.
    #[test]
    fn a_slot_refuses_while_a_later_slot_is_emptier() {
        let mut c = Crafter::default();
        let stick = ItemStack::one("minecraft:stick");
        c.items[0] = stick.clone();
        assert!(
            !c.can_place_item(0, &stick),
            "slot 1 is empty, so slot 0 waits"
        );
        // Fill every later slot to the same count and slot 0 opens up.
        for i in 1..CRAFTER_SLOTS {
            c.items[i] = stick.clone();
        }
        assert!(c.can_place_item(0, &stick));
    }

    /// `smallerStackExist` looks **right only**. Slot 8 accepts even when
    /// slot 0 is emptier, which is what makes the fill sweep left to right.
    #[test]
    fn the_even_fill_rule_never_looks_left() {
        let mut c = Crafter::default();
        let stick = ItemStack::one("minecraft:stick");
        c.items[8] = ItemStack::new("minecraft:stick", 4);
        assert!(c.can_place_item(8, &stick), "nothing to its right");
        // And slot 7 with the same stack does look right, at slot 8's four.
        c.items[7] = ItemStack::new("minecraft:stick", 2);
        assert!(
            c.can_place_item(7, &stick),
            "slot 8 holds four, which is not smaller than two"
        );
        c.items[7] = ItemStack::new("minecraft:stick", 6);
        assert!(!c.can_place_item(7, &stick), "slot 8's four is smaller");
    }

    /// A disabled later slot is skipped by the even-fill scan, so disabling
    /// the rest of a crafter lets its first slot fill up.
    #[test]
    fn a_disabled_later_slot_does_not_hold_an_earlier_one_back() {
        let mut c = Crafter::default();
        let stick = ItemStack::one("minecraft:stick");
        c.items[0] = stick.clone();
        assert!(!c.can_place_item(0, &stick));
        for i in 1..CRAFTER_SLOTS {
            c.set_slot_state(i32::try_from(i).unwrap(), false);
        }
        assert!(c.can_place_item(0, &stick));
    }

    /// A disabled slot refuses everything, and a full one does too.
    #[test]
    fn a_disabled_or_full_slot_refuses() {
        let mut c = Crafter::default();
        let stick = ItemStack::one("minecraft:stick");
        c.set_slot_state(5, false);
        assert!(!c.can_place_item(5, &stick));
        c.items[6] = ItemStack::new("minecraft:stick", 64);
        assert!(!c.can_place_item(6, &stick));
    }

    /// The comparator counts empty-or-disabled slots: 9 for an empty crafter,
    /// 0 for a full one, and a disabled slot counts even though it is empty
    /// anyway.
    #[test]
    fn the_comparator_counts_empty_or_disabled_slots() {
        let mut c = Crafter::default();
        assert_eq!(c.redstone_signal(), 9);
        for i in 0..CRAFTER_SLOTS {
            c.items[i] = ItemStack::one("minecraft:stick");
        }
        assert_eq!(c.redstone_signal(), 0);
        c.items[0] = ItemStack::empty();
        assert_eq!(c.redstone_signal(), 1);
    }

    /// The ticker returns immediately at rest and never drives the field
    /// negative; it clears `CRAFTING` on the tick the counter reaches zero
    /// and not before.
    #[test]
    fn the_crafting_ticker_clears_the_state_on_the_sixth_tick() {
        let mut c = Crafter::default();
        assert!(!c.server_tick(), "a crafter at rest does nothing");
        assert_eq!(c.crafting_ticks_remaining, 0);
        c.begin_craft();
        assert_eq!(c.crafting_ticks_remaining, CRAFTING_TICKS);
        for _ in 0..(CRAFTING_TICKS - 1) {
            assert!(!c.server_tick());
        }
        assert_eq!(c.crafting_ticks_remaining, 1);
        assert!(c.server_tick(), "the sixth tick clears CRAFTING");
        assert_eq!(c.crafting_ticks_remaining, 0);
        assert!(!c.server_tick(), "and it stays cleared");
    }
}
