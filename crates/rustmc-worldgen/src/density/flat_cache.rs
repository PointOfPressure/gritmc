//! The chunk's 2D cache — `NoiseChunk.FlatCache`.
//!
//! Every column-constant field in the router is wrapped in `flatCache`:
//! continentalness, erosion, ridges, both shift functions, and the terrain
//! shaper's offset / factor / jaggedness splines. Inside a chunk vanilla
//! evaluates each of them **25 times** — once per quart column of a 16-block
//! chunk, plus the shared edge — and every read after that is an array
//! lookup.
//!
//! # This is not only a speedup
//!
//! `FlatCache.compute` snaps its x and z to quart resolution *before* the
//! lookup, and the value it returns was computed at that snapped position.
//! So asking for continentalness at block 7 genuinely returns the value for
//! block 4, and asking at block 20 — outside the chunk's own five columns —
//! falls through and computes exactly.
//!
//! For terrain that distinction is invisible: the density graph only ever
//! reads these at cell corners, which are multiples of 4 and therefore
//! already quart-aligned. It is **not** invisible to the aquifer, which reads
//! `erosion` and `depth` at jittered aquifer centres that are usually not
//! quart-aligned. Vanilla hands the aquifer the *wrapped* router, so those
//! reads are snapped; a port that hands it the raw functions computes
//! something vanilla never computes.
//!
//! That is why this module exists at all rather than being folded into
//! `ChunkSampler`: the aquifer and the sampler must share one cache, exactly
//! as they share one `NoiseChunk`.

use super::{DensityFunction, FunctionContext, MarkerKind};

/// One chunk's flat caches, one table per `FlatCache` marker.
#[derive(Clone, Debug, Default)]
pub struct FlatCaches {
    first_quart_x: i32,
    first_quart_z: i32,
    /// Side of the square, in quart columns. 5 for a 16-block chunk: four
    /// columns plus the shared edge with the next chunk.
    size_xz: usize,
    tables: Vec<Vec<f64>>,
}

/// Block to quart, flooring — `QuartPos.fromBlock`.
#[must_use]
pub const fn quart_from_block(block: i32) -> i32 {
    block >> 2
}

/// Quart to block — `QuartPos.toBlock`.
#[must_use]
pub const fn block_from_quart(quart: i32) -> i32 {
    quart << 2
}

impl FlatCaches {
    /// An empty cache: every marker falls through to its wrapped function.
    ///
    /// What a marker does outside a chunk, and what every test that does not
    /// care about caching gets.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            first_quart_x: 0,
            first_quart_z: 0,
            size_xz: 0,
            tables: Vec::new(),
        }
    }

    /// Number every `FlatCache` marker across `roots` and fill its table.
    ///
    /// All the roots for one chunk must be passed together: they share the
    /// slot numbering and the table list, which is what makes the aquifer and
    /// the terrain read the same cache.
    ///
    /// `chunk_min_block_x`/`z` are the chunk's low corner; `span_blocks` is
    /// how wide the cached area is (16 for a chunk).
    #[must_use]
    pub fn build(
        roots: &mut [&mut DensityFunction],
        chunk_min_block_x: i32,
        chunk_min_block_z: i32,
        span_blocks: i32,
    ) -> Self {
        let wrapped = Self::prepare(roots);
        Self::fill(&wrapped, chunk_min_block_x, chunk_min_block_z, span_blocks)
    }

    /// Number every `FlatCache` marker across `roots` and hand back the
    /// wrapped functions, without filling anything.
    ///
    /// Split out of [`Self::build`] for a caller that walks many chunks with
    /// one graph. Numbering deep-clones each marker's subgraph — `offset`
    /// carries the whole terrain spline — and the numbering it produces is a
    /// function of the graph alone, so doing it once per worker instead of
    /// once per chunk changes no slot and no value.
    #[must_use]
    pub fn prepare(roots: &mut [&mut DensityFunction]) -> Vec<DensityFunction> {
        let mut wrapped = Vec::new();
        for root in roots.iter_mut() {
            root.number_flat_caches_into(&mut wrapped);
        }
        wrapped
    }

    /// Fill the tables for one chunk from an already-numbered marker list.
    #[must_use]
    pub fn fill(
        wrapped: &[DensityFunction],
        chunk_min_block_x: i32,
        chunk_min_block_z: i32,
        span_blocks: i32,
    ) -> Self {
        let first_quart_x = quart_from_block(chunk_min_block_x);
        let first_quart_z = quart_from_block(chunk_min_block_z);
        let size_xz = quart_from_block(span_blocks) as usize + 1;

        let mut cache = Self {
            first_quart_x,
            first_quart_z,
            size_xz,
            tables: Vec::with_capacity(wrapped.len()),
        };

        // Filled in slot order, which is post-order, so a nested flat cache
        // is already available when the one above it is computed. `continents`
        // wraps a `shiftedNoise` over `shift_x`/`shift_z`, and those are
        // themselves flat-cached — without the ordering the outer table would
        // be built from uncached inner reads, which is merely slower, and from
        // *unsnapped* inner reads, which is wrong.
        for function in wrapped {
            let mut table = Vec::with_capacity(size_xz * size_xz);
            for z in 0..size_xz {
                for x in 0..size_xz {
                    let block_x = block_from_quart(first_quart_x + x as i32);
                    let block_z = block_from_quart(first_quart_z + z as i32);
                    // y is 0, matching vanilla's `SinglePointContext(x, 0, z)`.
                    // These functions are column-constant, so the y never
                    // reaches anything — but passing the caller's y would make
                    // the table depend on who filled it first.
                    let ctx = FunctionContext::with_flat_caches(block_x, 0, block_z, &cache);
                    table.push(function.compute(ctx));
                }
            }
            debug_assert_eq!(table.len(), size_xz * size_xz);
            cache.tables.push(table);
        }

        cache
    }

    #[must_use]
    pub const fn slots(&self) -> usize {
        self.tables.len()
    }

    /// The cached value for `slot` at a block position, or `None` when the
    /// position is outside the cached square.
    ///
    /// `None` is not an error: vanilla computes the wrapped function directly
    /// there, and the aquifer genuinely reads outside the chunk.
    #[must_use]
    pub fn get(&self, slot: usize, block_x: i32, block_z: i32) -> Option<f64> {
        let table = self.tables.get(slot)?;
        let x = quart_from_block(block_x) - self.first_quart_x;
        let z = quart_from_block(block_z) - self.first_quart_z;
        if x < 0 || z < 0 || x as usize >= self.size_xz || z as usize >= self.size_xz {
            return None;
        }
        table.get(x as usize + z as usize * self.size_xz).copied()
    }
}

impl DensityFunction {
    /// Assign a slot to every `FlatCache` marker, depth-first, and return
    /// each one's wrapped function.
    ///
    /// Separate from the interpolator numbering because the two index
    /// different tables; a marker is exactly one kind, so they never collide.
    pub fn number_flat_caches_into(&mut self, out: &mut Vec<DensityFunction>) {
        self.number_markers_into(MarkerKind::FlatCache, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::density::{build, NoiseHolder};
    use crate::noise::{NoiseParameters, NoiseSeeding};

    /// A function that genuinely varies with x and z and ignores y — which
    /// is what a flat-cached function is.
    ///
    /// The first version of these tests used a `y_clamped_gradient` and
    /// called it "a function that varies with x". It varies with *y*, and
    /// the cache is filled at y = 0, so every table came out all-zero and
    /// the snapping assertions passed without observing any snapping at all.
    /// A real noise with `y_scale = 0.0` is the honest fixture.
    fn xz_noise() -> DensityFunction {
        let seeding = NoiseSeeding::new(20260725);
        let params = NoiseParameters::new(-6, vec![1.0, 1.0]);
        build::noise(
            NoiseHolder::bound(seeding.instantiate("minecraft:offset", &params)),
            1.0,
            0.0,
        )
    }

    /// A y-dependent function, for showing that an *uncached* read still
    /// sees the caller's y.
    fn y_ramp() -> DensityFunction {
        build::y_clamped_gradient(-1000, 1000, -1000.0, 1000.0)
    }

    #[test]
    fn quart_conversion_floors_on_negatives() {
        assert_eq!(quart_from_block(0), 0);
        assert_eq!(quart_from_block(3), 0);
        assert_eq!(quart_from_block(4), 1);
        // The bug this guards: truncation puts -1 in quart 0 alongside 0..3.
        assert_eq!(quart_from_block(-1), -1);
        assert_eq!(quart_from_block(-4), -1);
        assert_eq!(quart_from_block(-5), -2);
        assert_eq!(block_from_quart(-1), -4);
    }

    #[test]
    fn a_chunk_has_five_columns_a_side() {
        // Four quart columns across 16 blocks, plus the edge shared with the
        // next chunk — which the density graph reads when interpolating the
        // last cell.
        let mut f = build::flat_cache(build::constant(1.0));
        let cache = FlatCaches::build(&mut [&mut f], 0, 0, 16);
        assert_eq!(cache.slots(), 1);
        assert_eq!(cache.get(0, 0, 0), Some(1.0));
        assert_eq!(cache.get(0, 16, 16), Some(1.0), "the shared edge is in");
        assert_eq!(cache.get(0, 20, 0), None, "past the edge falls through");
        assert_eq!(cache.get(0, -1, 0), None, "before the chunk falls through");
    }

    #[test]
    fn reads_are_snapped_to_the_quart_column() {
        // The whole point. Every block in a quart column reports the column's
        // value, and the next column over reports a different one — without
        // that second half the test would pass on a constant function.
        let mut f = build::flat_cache(xz_noise());
        let cache = FlatCaches::build(&mut [&mut f], 0, 0, 16);
        let column = cache.get(0, 4, 0).expect("in range");
        for block_x in 4..8 {
            assert_eq!(
                cache.get(0, block_x, 0),
                Some(column),
                "block {block_x} should read column 4's value"
            );
        }
        assert_ne!(
            cache.get(0, 8, 0),
            Some(column),
            "the next column should differ, or this proves nothing"
        );
    }

    #[test]
    fn the_marker_reads_the_cache_and_falls_through_outside_it() {
        // A y-dependent function makes the two paths tell themselves apart:
        // cached reads were filled at y = 0, uncached ones see the real y.
        let mut f = build::flat_cache(y_ramp());
        let cache = FlatCaches::build(&mut [&mut f], 0, 0, 16);

        // Inside the chunk: the cached value, filled at y = 0.
        assert_eq!(
            f.compute(FunctionContext::with_flat_caches(7, 55, 0, &cache)),
            0.0
        );
        // Outside it: computed directly, so the y reaches the ramp.
        assert_eq!(
            f.compute(FunctionContext::with_flat_caches(100, 55, 0, &cache)),
            55.0
        );
        // With no cache at all it always computes directly.
        assert_eq!(f.compute(FunctionContext::new(7, 55, 0)), 55.0);
    }

    #[test]
    fn nested_caches_fill_inner_first() {
        // `flat_cache(add(flat_cache(noise), 1))` — the outer table must be
        // built from the inner cache's values, which post-order numbering is
        // what guarantees. Slot 0 is the inner one.
        let inner = build::flat_cache(xz_noise());
        let mut outer = build::flat_cache(build::add(inner, build::constant(1.0)));
        let cache = FlatCaches::build(&mut [&mut outer], 0, 0, 16);
        assert_eq!(cache.slots(), 2, "both markers were numbered");
        let inner_value = cache.get(0, 4, 0).expect("inner in range");
        assert_eq!(cache.get(1, 4, 0), Some(inner_value + 1.0));
        // And it is not a constant, so the composition is real.
        assert_ne!(cache.get(1, 8, 0), cache.get(1, 4, 0));
    }

    #[test]
    fn separate_roots_share_one_numbering() {
        let mut a = build::flat_cache(build::constant(1.0));
        let mut b = build::flat_cache(build::constant(2.0));
        let cache = FlatCaches::build(&mut [&mut a, &mut b], 0, 0, 16);
        assert_eq!(cache.slots(), 2);
        assert_eq!(
            a.compute(FunctionContext::with_flat_caches(0, 0, 0, &cache)),
            1.0
        );
        assert_eq!(
            b.compute(FunctionContext::with_flat_caches(0, 0, 0, &cache)),
            2.0
        );
    }

    #[test]
    fn an_empty_cache_makes_every_marker_a_passthrough() {
        let f = build::flat_cache(y_ramp());
        let none = FlatCaches::none();
        assert_eq!(
            f.compute(FunctionContext::with_flat_caches(7, 55, 0, &none)),
            55.0
        );
    }
}
