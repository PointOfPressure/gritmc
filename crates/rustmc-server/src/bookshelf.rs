//! `ChiseledBookShelfBlockEntity` — six slots that are six block-state
//! booleans.
//!
//! Transcribed from `.updateState(int)` (offsets 0-140), `.setItem(int,
//! ItemStack)` (0-43), `.removeItem(int, int)` (0-46), `.getMaxStackSize()`
//! and `.acceptsItemType(ItemStack)` in the pinned 26.2 jar.
//!
//! # Its container methods do not behave like a container's
//!
//! * **`getMaxStackSize()` is 1** — `iconst_1; ireturn`, overriding
//!   `Container`'s 99. One book per slot, and it is that override rather than
//!   any slot logic that enforces it;
//! * **`setItem` refuses what it does not accept, silently.** Offsets 0-5
//!   test `acceptsItemType`, and a stack that fails it falls to offset 26,
//!   where the *only* thing that can still happen is a `removeItem` for an
//!   **empty** stack. So `setItem(slot, aStone)` is a no-op that reports
//!   nothing;
//! * **`removeItem` ignores its count** and always takes the whole slot
//!   (offsets 21-32 set `EMPTY` unconditionally), then calls `updateState`
//!   only when what it took was non-empty;
//! * **`updateState` rejects a slot outside 0..5 by logging** and returning,
//!   rather than by throwing. A caller cannot tell the two apart from the
//!   return value, so [`Bookshelf::update_state`] reports it.
//!
//! # `updateState` rewrites every slot's property, not the one that changed
//!
//! Its loop at offsets 37-99 walks all six `SLOT_OCCUPIED_PROPERTIES` and
//! sets each from `getItem(i).isEmpty()`, then writes the whole state once
//! with `setBlock(..., 3)`. So one insertion produces one block update
//! carrying six booleans, and `lastInteractedSlot` — written first, at offset
//! 25 — is the only per-slot part.

use crate::item::ItemStack;
use crate::recipes::ItemTags;

/// `ChiseledBookShelfBlock.SLOT_OCCUPIED_PROPERTIES.size()`, and the bound
/// `updateState` rejects outside — `bipush 6` at offset 5.
pub const SHELF_SLOTS: usize = 6;

/// `getMaxStackSize()` — `iconst_1`.
pub const MAX_STACK_SIZE: i32 = 1;

/// The tag `acceptsItemType` reads.
pub const BOOKSHELF_BOOKS_TAG: &str = "bookshelf_books";

/// `acceptsItemType(ItemStack)` — `stack.is(ItemTags.BOOKSHELF_BOOKS)`.
///
/// **This takes the tag table and does not carry a list, and that is not
/// caution — the hand list was written first and it was wrong.** Four items
/// looked obviously right (`book`, `written_book`, `enchanted_book`,
/// `writable_book`) and the datapack's `bookshelf_books` has **five**: it
/// also holds `minecraft:knowledge_book`. A shelf built on the hand list
/// would have refused a knowledge book silently, through `setItem`'s
/// no-report branch, which is the quietest possible way to be wrong.
///
/// `tags` is `data/vanilla/tags_item.json`, which
/// [`crate::recipes::Recipes`] already loads.
#[must_use]
pub fn accepts_item_type(tags: &ItemTags, stack: &ItemStack) -> bool {
    !stack.is_empty() && tags.contains(BOOKSHELF_BOOKS_TAG, stack.item())
}

/// What a shelf write did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShelfUpdate {
    /// The six `slot_N_occupied` booleans, in slot order, as
    /// `updateState`'s loop computes them.
    pub occupied: Option<[bool; SHELF_SLOTS]>,
    /// `lastInteractedSlot` after the write.
    pub last_interacted_slot: i32,
    /// `updateState` was called with a slot outside 0..5 and logged
    /// `Expected slot 0-5, got {}` instead of doing anything.
    pub out_of_range: bool,
}

/// One chiseled bookshelf.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bookshelf {
    /// The six slots.
    pub items: [ItemStack; SHELF_SLOTS],
    /// `lastInteractedSlot`; the constructor leaves it at -1.
    pub last_interacted_slot: i32,
}

impl Default for Bookshelf {
    fn default() -> Self {
        Self {
            items: std::array::from_fn(|_| ItemStack::empty()),
            last_interacted_slot: -1,
        }
    }
}

impl Bookshelf {
    /// `updateState(int)` — offsets 0-140.
    ///
    /// The range test is `slot < 0 || slot >= 6`, and its failure path is a
    /// `LOGGER.error` and a `return`: `lastInteractedSlot` is **not** written
    /// and no block update goes out.
    pub fn update_state(&mut self, slot: i32) -> ShelfUpdate {
        if !(0..6).contains(&slot) {
            return ShelfUpdate {
                occupied: None,
                last_interacted_slot: self.last_interacted_slot,
                out_of_range: true,
            };
        }
        self.last_interacted_slot = slot;
        ShelfUpdate {
            occupied: Some(std::array::from_fn(|i| !self.items[i].is_empty())),
            last_interacted_slot: slot,
            out_of_range: false,
        }
    }

    /// `setItem(int, ItemStack)` — offsets 0-43.
    ///
    /// Three outcomes and only two of them touch anything: an accepted stack
    /// is written and updates the state; an **empty** stack routes to
    /// `removeItem(slot, getMaxStackSize())`; anything else is dropped on the
    /// floor with no state change and no report.
    pub fn set_item(
        &mut self,
        tags: &ItemTags,
        slot: usize,
        stack: ItemStack,
    ) -> Option<ShelfUpdate> {
        if slot >= SHELF_SLOTS {
            return None;
        }
        if accepts_item_type(tags, &stack) {
            self.items[slot] = stack;
            return Some(self.update_state(i32::try_from(slot).unwrap_or(-1)));
        }
        if stack.is_empty() {
            return self.remove_item(slot, MAX_STACK_SIZE);
        }
        None
    }

    /// `removeItem(int, int)` — offsets 0-46.
    ///
    /// `count` is read and then ignored: the slot is emptied whatever it says.
    /// `updateState` runs only if the slot held something, so removing from an
    /// already-empty slot sends no block update — which is what stops a
    /// double-click emitting two.
    pub fn remove_item(&mut self, slot: usize, _count: i32) -> Option<ShelfUpdate> {
        if slot >= SHELF_SLOTS {
            return None;
        }
        let taken = std::mem::replace(&mut self.items[slot], ItemStack::empty());
        if taken.is_empty() {
            return None;
        }
        Some(self.update_state(i32::try_from(slot).unwrap_or(-1)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real tag, five members, as the datapack has it.
    fn tags() -> ItemTags {
        ItemTags::parse(&serde_json::json!({
            "bookshelf_books": {"values": [
                "minecraft:book", "minecraft:written_book",
                "minecraft:enchanted_book", "minecraft:writable_book",
                "minecraft:knowledge_book"
            ]}
        }))
        .expect("tag parses")
    }

    /// A book goes in, all six booleans are recomputed, and the interacted
    /// slot is recorded.
    #[test]
    fn putting_a_book_in_recomputes_every_slot() {
        let mut s = Bookshelf::default();
        let u = s
            .set_item(&tags(), 3, ItemStack::one("minecraft:book"))
            .expect("accepted");
        assert_eq!(
            u.occupied,
            Some([false, false, false, true, false, false]),
            "one insertion carries all six booleans"
        );
        assert_eq!(u.last_interacted_slot, 3);
    }

    /// A stack the shelf does not accept is dropped with no state change and
    /// no report. The positive control beside it is the same call with a book.
    #[test]
    fn a_non_book_is_refused_silently() {
        let mut s = Bookshelf::default();
        assert!(s
            .set_item(&tags(), 0, ItemStack::one("minecraft:stone"))
            .is_none());
        assert!(s.items[0].is_empty());
        assert_eq!(s.last_interacted_slot, -1);
        assert!(s
            .set_item(&tags(), 0, ItemStack::one("minecraft:enchanted_book"))
            .is_some());
        // THE MEMBER A HAND LIST MISSES. `knowledge_book` is in the tag and
        // is not one of the four obvious books; this assert is the whole
        // reason `accepts_item_type` takes a table.
        let mut s = Bookshelf::default();
        assert!(s
            .set_item(&tags(), 0, ItemStack::one("minecraft:knowledge_book"))
            .is_some());
    }

    /// `setItem` with an empty stack routes to `removeItem`, so it *does*
    /// clear an occupied slot — the one case where the refused branch still
    /// changes something.
    #[test]
    fn setting_an_empty_stack_clears_the_slot() {
        let mut s = Bookshelf::default();
        s.set_item(&tags(), 1, ItemStack::one("minecraft:book"));
        let u = s.set_item(&tags(), 1, ItemStack::empty()).expect("removed");
        assert_eq!(u.occupied, Some([false; SHELF_SLOTS]));
        assert!(s.items[1].is_empty());
    }

    /// Removing from an empty slot reports nothing, so no block update goes
    /// out for a click that took nothing.
    #[test]
    fn removing_from_an_empty_slot_reports_nothing() {
        let mut s = Bookshelf::default();
        assert!(s.remove_item(2, 1).is_none());
        assert_eq!(s.last_interacted_slot, -1);
    }

    /// `removeItem` ignores its count: asking for one takes the whole slot,
    /// and the slot only ever holds one anyway because `getMaxStackSize` is 1.
    #[test]
    fn remove_item_ignores_its_count() {
        let mut s = Bookshelf::default();
        s.items[4] = ItemStack::new("minecraft:book", 5);
        assert!(s.remove_item(4, 1).is_some());
        assert!(s.items[4].is_empty());
        assert_eq!(MAX_STACK_SIZE, 1);
    }

    /// An out-of-range slot logs and returns; `lastInteractedSlot` keeps its
    /// old value rather than taking the bad one.
    #[test]
    fn an_out_of_range_slot_changes_nothing() {
        let mut s = Bookshelf::default();
        s.set_item(&tags(), 0, ItemStack::one("minecraft:book"));
        let u = s.update_state(9);
        assert!(u.out_of_range);
        assert!(u.occupied.is_none());
        assert_eq!(s.last_interacted_slot, 0);
        let u = s.update_state(-1);
        assert!(u.out_of_range);
    }
}
