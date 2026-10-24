//! `ChunkTrackingView` — which chunks a player is allowed to have, and what
//! changes when the view moves.
//!
//! Transcribed from 26.2's `net.minecraft.server.level.ChunkTrackingView`,
//! `ChunkTrackingView$Positioned` and `ChunkTrackingView$1` (the `EMPTY`
//! singleton), under `javap -p -c -constants`.
//!
//! # The tracked set is a padded disc, not a square
//!
//! This is the one fact in this module worth reading twice, because every
//! other view calculation in this server was written as a Chebyshev square
//! and [`crate::broadcast::chunk_is_tracked`]'s doc comment said, until this
//! commit, that a square was *correct* for deciding which chunks to send.
//! It is not. `ChunkMap.applyChunkTrackingView` drives the send/forget
//! decision through [`difference`] over two `Positioned` views, and
//! `Positioned.contains` is `isWithinDistance(cx, cz, vd, x, z, true)` — the
//! same padded-Euclidean test the entity tracker uses.
//!
//! From the bytecode of `ChunkTrackingView.isWithinDistance`:
//!
//! ```java
//! int pad = includeOuter ? 2 : 1;
//! long dx = Math.max(0, Math.abs(x - cx) - pad);
//! long dz = Math.max(0, Math.abs(z - cz) - pad);
//! return dx * dx + dz * dz < (long) (viewDistance * viewDistance);
//! ```
//!
//! The two shapes disagree in **both** directions, so neither contains the
//! other: at `view_distance` 10 the corner `(+10, +10)` is in the square and
//! not in the disc (`8² + 8² = 128`, not `< 100`), while `(+11, 0)` is in the
//! disc and not in the square (`9² = 81 < 100`). The disc is the **larger**
//! set of the two at that view distance: 473 chunks against the square's
//! 441, so a square is not even a conservative under-approximation.
//!
//! # The pad and the bounding box are separate numbers
//!
//! `Positioned.minX` is `center.x - viewDistance - 1` and `maxX` is
//! `center.x + viewDistance + 1`: the box is `viewDistance + 1` wide, while
//! the membership test pads by **2**. The box is therefore not a bound on
//! the disc it encloses — `(+11, 0)` above is inside the box only because the
//! box is one wider than the nominal view distance, and `(+12, 0)` would pass
//! the disc test at `view_distance` 11 but be outside the box. Vanilla
//! iterates the box and filters with `contains`, so the box wins; reproduced
//! here rather than rationalised, because "widen the box until the answer
//! stops changing" would be a different function.

/// A chunk position, in chunk coordinates.
pub type ChunkPos = (i32, i32);

/// `ChunkTrackingView.isWithinDistance(int, int, int, int, int, boolean)`.
///
/// `include_outer` is vanilla's `includeOuterChunksAdjacentToViewBorder`: it
/// is `true` on the `contains` path (which is what decides membership of the
/// tracked set) and `false` on the `isInViewDistance` path.
#[must_use]
pub fn is_within_distance(
    center: ChunkPos,
    view_distance: i32,
    chunk: ChunkPos,
    include_outer: bool,
) -> bool {
    let pad = if include_outer { 2 } else { 1 };
    let dx = i64::from(((chunk.0 - center.0).abs() - pad).max(0));
    let dz = i64::from(((chunk.1 - center.1).abs() - pad).max(0));
    // Vanilla squares the view distance as an `int` and widens the result of
    // the sum, so the comparison is `long < long` with an `int` right-hand
    // side. Kept, overflow and all: the two forms differ above 46,340 and a
    // silent difference in an overflow rule is unfindable later.
    let limit = i64::from(view_distance.wrapping_mul(view_distance));
    dx * dx + dz * dz < limit
}

/// A view: either `ChunkTrackingView.EMPTY` or a `Positioned`.
///
/// Vanilla models these as an interface with two implementations. An enum
/// rather than a trait object because [`difference`] does an `instanceof` on
/// both arguments and takes a completely different path when either is not
/// `Positioned` — a trait would have to expose that test anyway.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrackingView {
    /// `ChunkTrackingView$1`: `contains` is `false`, `forEach` yields nothing.
    #[default]
    Empty,
    /// `ChunkTrackingView$Positioned(center, viewDistance)`.
    Positioned {
        /// `Positioned.center`.
        center: ChunkPos,
        /// `Positioned.viewDistance`.
        view_distance: i32,
    },
}

impl TrackingView {
    /// `ChunkTrackingView.of(ChunkPos, int)`.
    #[must_use]
    pub const fn of(center: ChunkPos, view_distance: i32) -> Self {
        Self::Positioned {
            center,
            view_distance,
        }
    }

    /// `Positioned.minX` — `center.x - viewDistance - 1`.
    #[must_use]
    pub fn min_x(&self) -> Option<i32> {
        match self {
            Self::Empty => None,
            Self::Positioned {
                center,
                view_distance,
            } => Some(center.0 - view_distance - 1),
        }
    }

    /// `Positioned.maxX` — `center.x + viewDistance + 1`.
    #[must_use]
    pub fn max_x(&self) -> Option<i32> {
        match self {
            Self::Empty => None,
            Self::Positioned {
                center,
                view_distance,
            } => Some(center.0 + view_distance + 1),
        }
    }

    /// `Positioned.minZ` — `center.z - viewDistance - 1`.
    #[must_use]
    pub fn min_z(&self) -> Option<i32> {
        match self {
            Self::Empty => None,
            Self::Positioned {
                center,
                view_distance,
            } => Some(center.1 - view_distance - 1),
        }
    }

    /// `Positioned.maxZ` — `center.z + viewDistance + 1`.
    #[must_use]
    pub fn max_z(&self) -> Option<i32> {
        match self {
            Self::Empty => None,
            Self::Positioned {
                center,
                view_distance,
            } => Some(center.1 + view_distance + 1),
        }
    }

    /// `ChunkTrackingView.contains(int, int, boolean)`.
    #[must_use]
    pub fn contains_with(&self, chunk: ChunkPos, include_outer: bool) -> bool {
        match self {
            Self::Empty => false,
            Self::Positioned {
                center,
                view_distance,
            } => is_within_distance(*center, *view_distance, chunk, include_outer),
        }
    }

    /// `ChunkTrackingView.contains(int, int)` — the `includeOuter = true`
    /// default, and the test that decides membership of the tracked set.
    #[must_use]
    pub fn contains(&self, chunk: ChunkPos) -> bool {
        self.contains_with(chunk, true)
    }

    /// `ChunkTrackingView.isInViewDistance(int, int)` — the `false` pad.
    ///
    /// Not the membership test. Vanilla uses it where "is this inside the
    /// view proper" is asked rather than "may the client hold this".
    #[must_use]
    pub fn is_in_view_distance(&self, chunk: ChunkPos) -> bool {
        self.contains_with(chunk, false)
    }

    /// `Positioned.squareIntersects` — whether the two bounding boxes touch.
    ///
    /// `false` whenever either side is `EMPTY`, which is what sends
    /// [`difference`] down its fallback path.
    #[must_use]
    pub fn square_intersects(&self, other: &Self) -> bool {
        let (Some(a_min_x), Some(a_max_x), Some(a_min_z), Some(a_max_z)) =
            (self.min_x(), self.max_x(), self.min_z(), self.max_z())
        else {
            return false;
        };
        let (Some(b_min_x), Some(b_max_x), Some(b_min_z), Some(b_max_z)) =
            (other.min_x(), other.max_x(), other.min_z(), other.max_z())
        else {
            return false;
        };
        a_min_x <= b_max_x && a_max_x >= b_min_x && a_min_z <= b_max_z && a_max_z >= b_min_z
    }

    /// `ChunkTrackingView.forEach(Consumer<ChunkPos>)`.
    ///
    /// **x-major, both axes ascending**, over the bounding box, yielding only
    /// what `contains` accepts. The order is load-bearing: it is the order
    /// `forget_level_chunk` packets leave the server in when a view is
    /// replaced wholesale.
    pub fn for_each(&self, mut f: impl FnMut(ChunkPos)) {
        let (Some(min_x), Some(max_x), Some(min_z), Some(max_z)) =
            (self.min_x(), self.max_x(), self.min_z(), self.max_z())
        else {
            return;
        };
        for x in min_x..=max_x {
            for z in min_z..=max_z {
                if self.contains((x, z)) {
                    f((x, z));
                }
            }
        }
    }

    /// How many chunks this view tracks. Not a vanilla method; a counter
    /// over [`Self::for_each`], for tests and logging.
    #[must_use]
    pub fn len(&self) -> usize {
        let mut n = 0usize;
        self.for_each(|_| n += 1);
        n
    }

    /// Whether this view tracks nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// `ChunkTrackingView.difference(old, new, onAdd, onRemove)`.
///
/// `on_add` receives the chunks in `new` and not in `old` — the ones to send;
/// `on_remove` receives the chunks in `old` and not in `new` — the ones to
/// forget.
///
/// Three paths, and the third is not an optimisation of the second:
///
/// * the views are equal: nothing is emitted at all, not even a re-send;
/// * both are `Positioned` and their boxes intersect: **one x-major sweep of
///   the union box**, emitting adds and removes interleaved by position;
/// * anything else — either side `EMPTY`, or two boxes far enough apart to
///   miss: every chunk of `old` is removed, *then* every chunk of `new` is
///   added, each in that view's own `forEach` order. Note the order of those
///   two halves is fixed (removes first) and is not the interleaved order the
///   middle path produces.
pub fn difference(
    old: &TrackingView,
    new: &TrackingView,
    mut on_add: impl FnMut(ChunkPos),
    mut on_remove: impl FnMut(ChunkPos),
) {
    if old == new {
        return;
    }
    if matches!(old, TrackingView::Positioned { .. })
        && matches!(new, TrackingView::Positioned { .. })
        && old.square_intersects(new)
    {
        let min_x = old.min_x().unwrap_or(0).min(new.min_x().unwrap_or(0));
        let min_z = old.min_z().unwrap_or(0).min(new.min_z().unwrap_or(0));
        let max_x = old.max_x().unwrap_or(0).max(new.max_x().unwrap_or(0));
        let max_z = old.max_z().unwrap_or(0).max(new.max_z().unwrap_or(0));
        for x in min_x..=max_x {
            for z in min_z..=max_z {
                let in_old = old.contains((x, z));
                let in_new = new.contains((x, z));
                if in_old != in_new {
                    if in_new {
                        on_add((x, z));
                    } else {
                        on_remove((x, z));
                    }
                }
            }
        }
        return;
    }
    old.for_each(&mut on_remove);
    new.for_each(&mut on_add);
}

#[cfg(test)]
mod tests {
    use super::{difference, is_within_distance, TrackingView};

    /// The two shapes disagree in both directions, so a Chebyshev square is
    /// not a conservative approximation of the tracked set. The positive
    /// control is the axis chunk one *beyond* the nominal view distance: a
    /// square of radius 10 does not hold `(11, 0)` and vanilla does.
    #[test]
    fn the_tracked_set_is_a_padded_disc_and_not_a_square() {
        let v = TrackingView::of((0, 0), 10);
        assert!(
            !v.contains((10, 10)),
            "the corner of the square is outside the disc: 8*8 + 8*8 = 128, not < 100"
        );
        assert!(
            v.contains((11, 0)),
            "the pad of two puts one chunk beyond the view distance inside it: 9*9 = 81 < 100"
        );
        assert!(
            !v.contains((12, 0)),
            "and only one: 10*10 = 100 is not < 100"
        );
    }

    /// The pad is 2 on the membership path and 1 on `isInViewDistance`, and
    /// the difference is visible at exactly one ring.
    #[test]
    fn include_outer_moves_the_boundary_by_one() {
        assert!(is_within_distance((0, 0), 10, (11, 0), true));
        assert!(
            !is_within_distance((0, 0), 10, (11, 0), false),
            "pad 1 gives 10*10 = 100, which is not < 100"
        );
    }

    /// The counted size of a view. A number rather than a shape assertion so
    /// that a change to either the pad or the box moves it. 441 is what the
    /// Chebyshev square this replaces would have sent at the same distance.
    #[test]
    fn a_view_of_ten_tracks_473_chunks() {
        assert_eq!(TrackingView::of((0, 0), 10).len(), 473);
        assert_eq!(
            TrackingView::of((100, -100), 10).len(),
            473,
            "translation-invariant"
        );
        // Below view distance 4 the disc swallows its whole bounding box, so
        // the count is just `(2 * vd + 3)^2` and the pad is invisible.
        assert_eq!(TrackingView::of((0, 0), 2).len(), 49);
        assert_eq!(TrackingView::of((0, 0), 3).len(), 81);
        assert_eq!(
            TrackingView::of((0, 0), 4).len(),
            117,
            "the first view distance at which the four corners fall out — and \
             the number a real vanilla server at view-distance=4 was measured \
             sending a conforming client on 2026-08-03, recorded in \
             ci/chunk-batch-probe.sh's header a month before this port \
             existed. A Chebyshev square of radius 4 sends 81"
        );
    }

    /// `EMPTY` holds nothing and yields nothing, and is what a joining
    /// player's view is set to before the first `updateChunkTracking`.
    #[test]
    fn the_empty_view_holds_nothing() {
        let e = TrackingView::Empty;
        assert!(!e.contains((0, 0)));
        assert_eq!(e.len(), 0);
        assert!(e.is_empty());
    }

    /// Joining: `difference(EMPTY, view)` adds the whole view and removes
    /// nothing, in `forEach` order — x-major, both axes ascending.
    #[test]
    fn a_join_adds_the_whole_view_x_major() {
        let mut add = Vec::new();
        let mut rem = Vec::new();
        difference(
            &TrackingView::Empty,
            &TrackingView::of((0, 0), 2),
            |c| add.push(c),
            |c| rem.push(c),
        );
        assert!(rem.is_empty());
        assert_eq!(add.len(), 49);
        assert_eq!(add[0], (-3, -3), "x-major: the first column is x = -2 - 1");
        assert_eq!(add[1], (-3, -2));
        let mut sorted = add.clone();
        sorted.sort_unstable();
        assert_eq!(add, sorted, "x-major ascending is lexicographic ascending");
    }

    /// Leaving: `difference(view, EMPTY)` removes the whole view and adds
    /// nothing. The fallback path, because `EMPTY` has no box.
    #[test]
    fn a_leave_removes_the_whole_view() {
        let mut add = Vec::new();
        let mut rem = Vec::new();
        difference(
            &TrackingView::of((0, 0), 2),
            &TrackingView::Empty,
            |c| add.push(c),
            |c| rem.push(c),
        );
        assert!(add.is_empty());
        assert_eq!(rem.len(), 49);
    }

    /// A step of one chunk moves a thin crescent, not a whole ring: the two
    /// discs overlap almost entirely. The counts are the falsifiable part —
    /// a Chebyshev implementation of the same step gives 21 and 21.
    #[test]
    fn a_one_chunk_step_moves_a_crescent() {
        let mut add = Vec::new();
        let mut rem = Vec::new();
        difference(
            &TrackingView::of((0, 0), 10),
            &TrackingView::of((1, 0), 10),
            |c| add.push(c),
            |c| rem.push(c),
        );
        assert_eq!(add.len(), 23);
        assert_eq!(rem.len(), 23);
        for c in &add {
            assert!(!TrackingView::of((0, 0), 10).contains(*c));
            assert!(TrackingView::of((1, 0), 10).contains(*c));
        }
        for c in &rem {
            assert!(TrackingView::of((0, 0), 10).contains(*c));
            assert!(!TrackingView::of((1, 0), 10).contains(*c));
        }
    }

    /// Equal views emit nothing — not even the chunks both hold. This is the
    /// early return that stops `updateChunkTracking` re-sending a world on
    /// every packet from a stationary player.
    #[test]
    fn an_unchanged_view_emits_nothing() {
        let n = std::cell::Cell::new(0usize);
        difference(
            &TrackingView::of((3, 4), 10),
            &TrackingView::of((3, 4), 10),
            |_| n.set(n.get() + 1),
            |_| n.set(n.get() + 1),
        );
        assert_eq!(n.get(), 0);
    }

    /// A teleport far enough that the boxes miss takes the fallback path:
    /// every old chunk removed first, then every new chunk added. The
    /// positive control is that the counts are the full views, not a
    /// crescent.
    #[test]
    fn a_teleport_out_of_range_removes_all_then_adds_all() {
        let ops: std::cell::RefCell<Vec<(char, (i32, i32))>> = std::cell::RefCell::new(Vec::new());
        let old = TrackingView::of((0, 0), 4);
        let new = TrackingView::of((1000, 1000), 4);
        assert!(!old.square_intersects(&new));
        difference(
            &old,
            &new,
            |c| ops.borrow_mut().push(('a', c)),
            |c| ops.borrow_mut().push(('r', c)),
        );
        let ops = ops.into_inner();
        let removes = ops.iter().filter(|(k, _)| *k == 'r').count();
        let adds = ops.iter().filter(|(k, _)| *k == 'a').count();
        assert_eq!(removes, old.len());
        assert_eq!(adds, new.len());
        let first_add = ops.iter().position(|(k, _)| *k == 'a').expect("some add");
        let last_remove = ops
            .iter()
            .rposition(|(k, _)| *k == 'r')
            .expect("some remove");
        assert!(
            last_remove < first_add,
            "the fallback path is removes-then-adds, never interleaved"
        );
    }

    /// Boxes that touch but discs that do not still take the sweep path, and
    /// it must produce the same set as the fallback would. Two adjacent
    /// views at a separation where `squareIntersects` is true by one chunk.
    #[test]
    fn touching_boxes_take_the_sweep_path_and_agree_with_the_fallback() {
        let old = TrackingView::of((0, 0), 4);
        let new = TrackingView::of((10, 0), 4);
        assert!(old.square_intersects(&new), "boxes are 5 wide each side");
        let (mut add, mut rem) = (Vec::new(), Vec::new());
        difference(&old, &new, |c| add.push(c), |c| rem.push(c));
        let mut want_add: Vec<_> = Vec::new();
        new.for_each(|c| {
            if !old.contains(c) {
                want_add.push(c);
            }
        });
        let mut want_rem: Vec<_> = Vec::new();
        old.for_each(|c| {
            if !new.contains(c) {
                want_rem.push(c);
            }
        });
        add.sort_unstable();
        rem.sort_unstable();
        want_add.sort_unstable();
        want_rem.sort_unstable();
        assert_eq!(add, want_add);
        assert_eq!(rem, want_rem);
    }

    /// A view-distance change with the centre held still is a `difference`
    /// too — `updateChunkTracking` re-runs on `setServerViewDistance`.
    #[test]
    fn shrinking_the_view_distance_removes_the_outer_rings() {
        let (mut add, mut rem) = (Vec::new(), Vec::new());
        difference(
            &TrackingView::of((0, 0), 10),
            &TrackingView::of((0, 0), 8),
            |c| add.push(c),
            |c| rem.push(c),
        );
        assert!(add.is_empty(), "a smaller disc adds nothing");
        assert_eq!(rem.len(), 144);
        assert_eq!(rem.len(), 473 - TrackingView::of((0, 0), 8).len());
    }
}
