//! `LecternBlockEntity` — one book, a page number and a comparator.
//!
//! Transcribed from `LecternBlockEntity.hasBook` (offsets 0-31),
//! `.setBook(ItemStack, Player)` (0-30), `.setPage(int)` (0-44) and
//! `.getRedstoneSignal()` (0-46) in the pinned 26.2 jar.
//!
//! # `hasBook` is not `!getBook().isEmpty()`
//!
//! It is `book.has(WRITABLE_BOOK_CONTENT) || book.has(WRITTEN_BOOK_CONTENT)` —
//! two data-component tests and no emptiness test at all. A lectern holding
//! something that is not a book therefore answers **false** to `hasBook` while
//! `getBook()` returns a non-empty stack, and the comparator loses its +1.
//! This server carries no data components, so [`has_book`] takes the item id
//! and the two component names are recorded here rather than approximated by
//! `!is_empty()`, which would be right for every book and wrong for
//! everything else.

use crate::item::ItemStack;

/// The float `getRedstoneSignal` multiplies the page fraction by — `ldc 14.0f`
/// at offset 27.
///
/// Fourteen and not fifteen: the remaining one comes from the `hasBook` term
/// added at offset 45, so a lectern on its last page reads **15** and an empty
/// one reads 0.
pub const SIGNAL_SCALE: f32 = 14.0;

/// `LecternBlockEntity.hasBook()` — offsets 0-31.
///
/// Vanilla asks for the `WRITABLE_BOOK_CONTENT` or `WRITTEN_BOOK_CONTENT`
/// data component. This server has no components, so the two items that carry
/// them in vanilla are named instead. **That is a narrower rule than
/// vanilla's, not a wider one**: a modded book with the component would be
/// missed, and nothing this server can produce has one.
#[must_use]
pub fn has_book(stack: &ItemStack) -> bool {
    matches!(
        stack.item(),
        "minecraft:writable_book" | "minecraft:written_book"
    )
}

/// One lectern.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Lectern {
    /// `book`.
    pub book: ItemStack,
    /// `page` — zero-based.
    pub page: i32,
    /// `pageCount`, stamped by `setBook` and never recomputed.
    pub page_count: i32,
}

impl Lectern {
    /// `setBook(ItemStack, Player)` — offsets 0-30.
    ///
    /// Writes the book, **zeroes the page**, and stamps `pageCount` from
    /// `getPageCount(book)`. The page reset is unconditional: putting the
    /// same book back on a lectern turns it to page one.
    ///
    /// `page_count` is the caller's, because vanilla reads it out of the
    /// book's own component and this server has none. Passing 1 for a book
    /// whose pages are unknown is the honest answer and gives the same
    /// comparator reading as vanilla's `pageCount <= 1` branch.
    pub fn set_book(&mut self, book: ItemStack, page_count: i32) {
        self.book = book;
        self.page = 0;
        self.page_count = page_count;
    }

    /// `setPage(int)` — offsets 0-44.
    ///
    /// `Mth.clamp(page, 0, pageCount - 1)`, and then **only if the clamped
    /// value differs from the current one** does anything happen:
    /// `setChanged()` and `LecternBlock.signalPageChange`. Returns whether it
    /// changed, which is exactly the condition for both of those.
    ///
    /// The upper bound is `pageCount - 1`, so a lectern whose `pageCount` is
    /// 0 clamps into `[0, -1]`. `Mth.clamp` is `min(max(v, min), max)`, which
    /// for `max < min` answers **max**, so the page becomes -1. Transcribed
    /// rather than corrected; vanilla only reaches it for a book with no
    /// pages.
    pub fn set_page(&mut self, page: i32) -> bool {
        let clamped = clamp(page, 0, self.page_count - 1);
        if clamped == self.page {
            return false;
        }
        self.page = clamped;
        true
    }

    /// `getRedstoneSignal()` — offsets 0-46.
    ///
    /// `Mth.floor(fraction * 14.0f) + (hasBook() ? 1 : 0)`, where the fraction
    /// is `page / (pageCount - 1)` for a book of more than one page and
    /// **1.0f** for one page or fewer (the `if_icmple 24` at offset 5). So a
    /// one-page book reads 15 the moment it is on the lectern, and an empty
    /// lectern reads `floor(1.0 * 14) + 0` = **14**, not 0.
    ///
    /// That last consequence is worth stating because it is surprising and it
    /// is what the bytecode says: `pageCount` is 0 for an empty lectern, `0 <=
    /// 1`, so the fraction is `fconst_1` and the signal is 14. `hasBook` adds
    /// the fifteenth only when a real book is there.
    ///
    /// **A comparator never sees that 14**, and the reason is one level up:
    /// `LecternBlock.getAnalogOutputSignal` returns 0 unless the block state's
    /// `HAS_BOOK` is true (its `ifeq 44` at offset 13), so this method is only
    /// ever called on a lectern with a book. Both are transcribed because
    /// folding the gate into this function would make the 14 unreachable for
    /// a reason no line records, and the day something else calls
    /// `getRedstoneSignal` the two answers differ.
    #[must_use]
    pub fn redstone_signal(&self) -> i32 {
        let fraction = if self.page_count > 1 {
            self.page as f32 / (self.page_count as f32 - 1.0)
        } else {
            1.0
        };
        let floor = (fraction * SIGNAL_SCALE).floor() as i32;
        floor + i32::from(has_book(&self.book))
    }
}

/// `Mth.clamp(int, int, int)`.
///
/// Vanilla's is `Math.min(Math.max(value, min), max)` — offsets 0-9 — so when
/// `max < min` it returns **max** rather than `min`, and it does not throw.
/// Rust's `Ord::clamp` panics on `max < min`, which is why this is written
/// out rather than delegated.
const fn clamp(v: i32, min: i32, max: i32) -> i32 {
    let lo = if v > min { v } else { min };
    if lo < max {
        lo
    } else {
        max
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book() -> ItemStack {
        ItemStack::one("minecraft:written_book")
    }

    /// `hasBook` is about the item, not about emptiness. A lectern holding a
    /// stone answers false while its stack is non-empty.
    #[test]
    fn has_book_is_not_the_same_as_a_non_empty_stack() {
        assert!(has_book(&book()));
        assert!(has_book(&ItemStack::one("minecraft:writable_book")));
        assert!(!has_book(&ItemStack::one("minecraft:stone")));
        assert!(!has_book(&ItemStack::empty()));
    }

    /// An **empty** lectern reads 14, not 0, because the fraction defaults to
    /// 1.0 for a page count of 0 or 1. This is the reading a plausible
    /// implementation gets wrong.
    #[test]
    fn an_empty_lectern_reads_fourteen() {
        assert_eq!(Lectern::default().redstone_signal(), 14);
    }

    /// A one-page book reads 15 immediately, and the fifteenth point is the
    /// `hasBook` term rather than the scale.
    #[test]
    fn a_one_page_book_reads_fifteen() {
        let mut l = Lectern::default();
        l.set_book(book(), 1);
        assert_eq!(l.redstone_signal(), 15);
        // The same lectern holding a stone loses exactly one point.
        l.book = ItemStack::one("minecraft:stone");
        assert_eq!(l.redstone_signal(), 14);
    }

    /// A multi-page book walks 1..15 across its pages, and the last page is
    /// 15. The intermediate value is what says the fraction is
    /// `page / (count - 1)` rather than `page / count`.
    #[test]
    fn the_signal_walks_the_pages() {
        let mut l = Lectern::default();
        l.set_book(book(), 15);
        assert_eq!(l.redstone_signal(), 1, "page 0 of 15");
        l.set_page(7);
        assert_eq!(l.redstone_signal(), 8);
        l.set_page(14);
        assert_eq!(l.redstone_signal(), 15, "the last page");
    }

    /// Setting the page to the value it already has changes nothing and
    /// reports so, which is what gates `signalPageChange`.
    #[test]
    fn setting_the_same_page_reports_no_change() {
        let mut l = Lectern::default();
        l.set_book(book(), 10);
        assert!(!l.set_page(0), "already on page 0");
        assert!(l.set_page(3));
        assert!(!l.set_page(3));
    }

    /// The page clamps into `[0, pageCount - 1]`, so a page past the end
    /// lands on the last one rather than being refused.
    #[test]
    fn the_page_clamps_to_the_last_one() {
        let mut l = Lectern::default();
        l.set_book(book(), 5);
        assert!(l.set_page(99));
        assert_eq!(l.page, 4);
        assert!(l.set_page(-4));
        assert_eq!(l.page, 0);
    }

    /// Putting a book on resets the page to zero even when it is the same
    /// book.
    #[test]
    fn setting_a_book_resets_the_page() {
        let mut l = Lectern::default();
        l.set_book(book(), 10);
        l.set_page(6);
        l.set_book(book(), 10);
        assert_eq!(l.page, 0);
    }

    /// `Mth.clamp` is `min(max(v, min), max)`, so with `max < min` it answers
    /// the **max** and does not throw — where Rust's own `clamp` panics. A
    /// zero-page book therefore lands on -1.
    #[test]
    fn a_zero_page_book_clamps_to_minus_one() {
        assert_eq!(clamp(5, 0, -1), -1);
        assert_eq!(clamp(-5, 0, -1), -1);
        let mut l = Lectern::default();
        l.set_book(book(), 0);
        assert!(l.set_page(3));
        assert_eq!(l.page, -1);
    }
}
