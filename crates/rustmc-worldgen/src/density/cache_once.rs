//! `NoiseChunk.CacheOnce` — memoize a subgraph for one evaluation point.
//!
//! Vanilla wraps a handful of functions in `cacheOnce` because they are read
//! more than once at the same position. `slopedCheese` is the important one:
//!
//! ```java
//! DensityFunction slopedCheese = DensityFunctions.cacheOnce(...);
//! DensityFunction surfaceWithEntrances = min(slopedCheese, mul(5, entrances));
//! DensityFunction caves = rangeChoice(slopedCheese, -1e6, 1.5625,
//!                                     surfaceWithEntrances, underground(.., slopedCheese));
//! ```
//!
//! It appears three times, and evaluating `caves` reads it at least twice —
//! once as the `rangeChoice` input and again inside whichever branch wins.
//! It contains `BlendedNoise`, the most expensive function in the graph.
//!
//! # Value-transparent, which is why it can be added after parity
//!
//! Memoizing a pure function of position cannot change a value. Vanilla keys
//! on `interpolationCounter`, which it bumps as the fill loop moves; this
//! keys on a generation the caller bumps when it moves to a new point. Both
//! amount to "same point, same answer".
//!
//! Over-bumping is always safe — it only loses a cache hit. *Under*-bumping
//! is not: a stale value from the previous position would be returned as if
//! it were this one's. So every entry point that changes position bumps,
//! and the default scratch has no slots at all, which makes every marker a
//! passthrough.

use std::cell::Cell;

/// Per-chunk scratch for `CacheOnce` markers.
///
/// `Cell` rather than `RefCell`: the stored pair is `Copy`, no borrow is
/// ever held across the recursive `compute` that fills it, and a marker
/// whose wrapped function reaches another marker must not hit a borrow
/// conflict.
#[derive(Debug, Default)]
pub struct CacheOnceScratch {
    /// Generation of the point currently being evaluated. Starts at 1 so the
    /// zero-initialised cells below can never match it by accident.
    generation: Cell<u64>,
    cells: Vec<Cell<(u64, f64)>>,
}

impl CacheOnceScratch {
    /// A scratch with no slots — every marker computes directly.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            generation: Cell::new(1),
            cells: Vec::new(),
        }
    }

    #[must_use]
    pub fn new(slots: usize) -> Self {
        Self {
            generation: Cell::new(1),
            cells: vec![Cell::new((0, 0.0)); slots],
        }
    }

    /// Move to a new evaluation point, invalidating every cached value.
    ///
    /// Must be called whenever the position changes. Wrapping is not a
    /// concern: at one bump per block, `u64` lasts longer than the universe.
    pub fn advance(&self) {
        self.generation.set(self.generation.get().wrapping_add(1));
    }

    #[must_use]
    pub fn slots(&self) -> usize {
        self.cells.len()
    }

    /// The cached value for `slot`, or `compute()` stored and returned.
    pub fn get_or_compute(&self, slot: usize, compute: impl FnOnce() -> f64) -> f64 {
        let Some(cell) = self.cells.get(slot) else {
            return compute();
        };
        let generation = self.generation.get();
        let (stored_generation, value) = cell.get();
        if stored_generation == generation {
            return value;
        }
        let value = compute();
        cell.set((generation, value));
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn a_repeated_read_at_one_point_computes_once() {
        let scratch = CacheOnceScratch::new(1);
        let calls = RefCell::new(0);
        let compute = || {
            *calls.borrow_mut() += 1;
            42.0
        };
        assert_eq!(scratch.get_or_compute(0, compute), 42.0);
        assert_eq!(scratch.get_or_compute(0, compute), 42.0);
        assert_eq!(scratch.get_or_compute(0, compute), 42.0);
        assert_eq!(*calls.borrow(), 1);
    }

    #[test]
    fn advancing_invalidates_everything() {
        // The safety property. A stale value served after the position moved
        // would be a wrong block, not a slow one.
        let scratch = CacheOnceScratch::new(1);
        assert_eq!(scratch.get_or_compute(0, || 1.0), 1.0);
        scratch.advance();
        assert_eq!(scratch.get_or_compute(0, || 2.0), 2.0);
        assert_eq!(scratch.get_or_compute(0, || 999.0), 2.0, "re-cached");
    }

    #[test]
    fn slots_are_independent() {
        let scratch = CacheOnceScratch::new(3);
        assert_eq!(scratch.get_or_compute(0, || 1.0), 1.0);
        assert_eq!(scratch.get_or_compute(1, || 2.0), 2.0);
        assert_eq!(scratch.get_or_compute(2, || 3.0), 3.0);
        assert_eq!(scratch.get_or_compute(0, || 99.0), 1.0);
        assert_eq!(scratch.get_or_compute(1, || 99.0), 2.0);
    }

    #[test]
    fn an_empty_scratch_always_computes() {
        let scratch = CacheOnceScratch::none();
        assert_eq!(scratch.slots(), 0);
        assert_eq!(scratch.get_or_compute(0, || 1.0), 1.0);
        assert_eq!(scratch.get_or_compute(0, || 2.0), 2.0, "no caching at all");
    }

    #[test]
    fn a_nested_fill_does_not_deadlock_or_clobber() {
        // A marker's wrapped function can reach another marker. With RefCell
        // this would panic on a double borrow; with Cell it must simply work.
        let scratch = CacheOnceScratch::new(2);
        let outer = scratch.get_or_compute(0, || 10.0 + scratch.get_or_compute(1, || 5.0));
        assert_eq!(outer, 15.0);
        assert_eq!(scratch.get_or_compute(1, || 99.0), 5.0, "inner survived");
        assert_eq!(scratch.get_or_compute(0, || 99.0), 15.0, "outer survived");
    }

    #[test]
    fn the_initial_generation_cannot_match_an_unwritten_cell() {
        // Cells start at generation 0; if the counter also started at 0 every
        // slot would report a cached 0.0 before anything was computed.
        let scratch = CacheOnceScratch::new(1);
        assert_eq!(scratch.get_or_compute(0, || 7.0), 7.0);
    }
}
