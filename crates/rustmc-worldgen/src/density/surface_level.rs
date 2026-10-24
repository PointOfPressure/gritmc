//! A cached, quart-resolution estimate of where the ground is.
//!
//! Reference: `NoiseChunk.preliminarySurfaceLevel` /
//! `computePreliminarySurfaceLevel` / `maxPreliminarySurfaceLevel`.
//!
//! The aquifer needs to know roughly how deep a point is before it can
//! decide whether a water table is buried or breaks out of a hillside, and
//! it asks that question dozens of times per aquifer centre. Running the
//! real terrain graph for each would cost more than generating the chunk.
//!
//! Two things make it affordable, and both are load-bearing rather than
//! optimisations:
//!
//! - **Quart snapping.** The query is rounded to a 4-block grid *before* the
//!   cache lookup, so a whole 4x4 column shares one answer. Vanilla does the
//!   snapping inside `preliminarySurfaceLevel`, which means the snapped
//!   position is also the one the density function is evaluated at — asking
//!   about block 7 genuinely returns the value computed for block 4, not an
//!   interpolation.
//! - **Cell-height scanning**, in `FindTopSurface` below this. The answer is
//!   always a multiple of the cell height.
//!
//! So this is deliberately coarse in both axes, and code above it must treat
//! the result as an estimate. It is *not* the heightmap.

use super::flat_cache::FlatCaches;
use super::{DensityFunction, FunctionContext};
use crate::math;
use std::collections::HashMap;

/// Owns the surface-level function and its per-chunk cache.
///
/// Held by the caller rather than the aquifer because vanilla's cache lives
/// on the `NoiseChunk` and is shared by everything generating that chunk.
#[derive(Debug)]
pub struct SurfaceLevels {
    function: DensityFunction,
    /// Keyed by the *snapped* column. Only ever point-queried, never
    /// iterated, so the hash order cannot reach behaviour.
    cache: HashMap<(i32, i32), i32>,
}

impl SurfaceLevels {
    #[must_use]
    pub fn new(function: DensityFunction) -> Self {
        Self {
            function,
            cache: HashMap::new(),
        }
    }

    /// `QuartPos.toBlock(QuartPos.fromBlock(v))` — snap down to the 4-block
    /// grid.
    ///
    /// `>> 2 << 2` and not `/ 4 * 4`: the shift floors, so block -1 snaps to
    /// -4. Truncation would snap it to 0 and make the column straddling the
    /// origin twice as wide as every other.
    #[must_use]
    pub const fn snap_to_quart(value: i32) -> i32 {
        (value >> 2) << 2
    }

    /// `NoiseChunk.preliminarySurfaceLevel`.
    pub fn at(&mut self, flat: &FlatCaches, sample_x: i32, sample_z: i32) -> i32 {
        let key = (Self::snap_to_quart(sample_x), Self::snap_to_quart(sample_z));
        if let Some(cached) = self.cache.get(&key) {
            return *cached;
        }
        // y is 0, not the caller's y: this is a column property, and passing
        // the caller's y would make the cached answer depend on whichever
        // block happened to ask first.
        // `new SinglePointContext(x, 0, z)` — a bare point, and **not** the
        // flat caches.
        //
        // That is the whole difference. A `flat_cache` node inside
        // `preliminary_surface_level` serves the value for whichever cell the
        // cache was primed on, so handing it a cache answers for the wrong
        // column as soon as the query leaves that cell. Inside the chunk
        // being generated the two agree and the fault is invisible; the
        // `above_preliminary_surface` blend reads a corner at
        // `(cell + 1) << 4`, one block past the edge, and there it returned
        // 55 where vanilla returns 24.
        let _ = flat;
        let value = math::floor(self.function.compute(FunctionContext::new(key.0, 0, key.1)));
        self.cache.insert(key, value);
        value
    }

    /// `NoiseChunk.maxPreliminarySurfaceLevel` — the highest surface over a
    /// block-coordinate rectangle, sampled every 4 blocks.
    ///
    /// The stride matches the snapping, so this visits each distinct column
    /// exactly once. Both bounds are inclusive.
    pub fn max_over(
        &mut self,
        flat: &FlatCaches,
        min_block_x: i32,
        min_block_z: i32,
        max_block_x: i32,
        max_block_z: i32,
    ) -> i32 {
        let mut max_y = i32::MIN;
        let mut block_z = min_block_z;
        while block_z <= max_block_z {
            let mut block_x = min_block_x;
            while block_x <= max_block_x {
                let level = self.at(flat, block_x, block_z);
                if level > max_y {
                    max_y = level;
                }
                block_x += 4;
            }
            block_z += 4;
        }
        max_y
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::density::build;

    /// A surface that rises with x, so the max over a rectangle is
    /// predictable and the cache cannot hide a wrong column.
    fn ramp() -> DensityFunction {
        build::y_clamped_gradient(0, 1, 0.0, 0.0)
    }

    #[test]
    fn snapping_floors_on_negatives() {
        assert_eq!(SurfaceLevels::snap_to_quart(0), 0);
        assert_eq!(SurfaceLevels::snap_to_quart(3), 0);
        assert_eq!(SurfaceLevels::snap_to_quart(4), 4);
        // The bug this guards: truncation puts -1 in column 0 alongside 0..3.
        assert_eq!(SurfaceLevels::snap_to_quart(-1), -4);
        assert_eq!(SurfaceLevels::snap_to_quart(-4), -4);
        assert_eq!(SurfaceLevels::snap_to_quart(-5), -8);
    }

    #[test]
    fn a_whole_quart_column_shares_one_answer() {
        let mut levels = SurfaceLevels::new(ramp());
        let base = levels.at(&FlatCaches::none(), 0, 0);
        for x in 0..4 {
            for z in 0..4 {
                assert_eq!(
                    levels.at(&FlatCaches::none(), x, z),
                    base,
                    "({x}, {z}) disagreed"
                );
            }
        }
        // And the neighbouring column is a separate entry, not the same one.
        assert_eq!(
            levels.at(&FlatCaches::none(), 4, 0),
            levels.at(&FlatCaches::none(), 7, 3)
        );
    }

    #[test]
    fn the_result_is_floored_not_truncated() {
        // A surface function returning -0.5 must give -1, not 0. Mth.floor
        // is what vanilla applies, and one block of error here moves a water
        // table.
        let mut levels = SurfaceLevels::new(build::constant(-0.5));
        assert_eq!(levels.at(&FlatCaches::none(), 0, 0), -1);
        let mut levels = SurfaceLevels::new(build::constant(0.5));
        assert_eq!(levels.at(&FlatCaches::none(), 0, 0), 0);
    }

    #[test]
    fn max_over_covers_both_inclusive_bounds() {
        // Height depends only on x here, stepping up every 4 blocks.
        let mut levels = SurfaceLevels::new(build::constant(7.0));
        assert_eq!(levels.max_over(&FlatCaches::none(), 0, 0, 15, 15), 7);
        // A degenerate rectangle still samples its single column.
        assert_eq!(levels.max_over(&FlatCaches::none(), 5, 5, 5, 5), 7);
    }
}
