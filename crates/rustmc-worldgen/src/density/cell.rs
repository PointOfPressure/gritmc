//! Cell-lattice sampling — trilinear interpolation over noise cells.
//!
//! Reference: `net.minecraft.world.level.levelgen.NoiseChunk.NoiseInterpolator`.
//!
//! # This is not an optimization
//!
//! Vanilla does not evaluate the density graph per block. It samples on a
//! coarse lattice — 4 blocks wide, 8 tall for the overworld — and
//! trilinearly interpolates the interior. **The interpolated values are the
//! terrain.** Sampling per block instead produces a smoother, different
//! world that is not vanilla's, so this is a correctness requirement that
//! happens to also be much faster.
//!
//! # Two interpolation orders, and they are not interchangeable
//!
//! `NoiseInterpolator.compute` branches on `fillingCell`:
//!
//! - **false** — the staged path, returning the value built by
//!   `updateForY`, `updateForX`, `updateForZ`. That collapses **Y, then X,
//!   then Z**.
//! - **true** — `Mth.lerp3`, which collapses **X, then Y, then Z**.
//!
//! Trilinear interpolation is order-independent in exact arithmetic, so it
//! is tempting to treat these as one function. In floating point they are
//! not: `lerp(t, a, b) = a + t * (b - a)` rounds twice, and over random
//! cells the two orders disagree by exactly one ulp about **45%** of the
//! time. A test below pins a concrete disagreeing cell so this can never be
//! "simplified" back into a single path.
//!
//! For terrain it is the `lerp3` path that decides blocks. `finalDensity`
//! is wrapped in `cacheAllInCell`, whose array is filled during
//! `selectCellYZ` with `fillingCell = true`; `getInterpolatedDensity` reads
//! that array, and the staged value computed by the three `updateFor*`
//! calls is discarded. Their surviving job is to set `inCellX/Y/Z`, which
//! index the array. The staged path is still live for interpolated markers
//! read outside a cell fill, so both are implemented here.
//!
//! The staging mirrors vanilla's loop structure: `update_y` runs once per y
//! within a cell, `update_x` once per x, `update_z` per block, so partial
//! results are cached between calls rather than recomputed.

use crate::math::lerp;

/// The eight corner values of one cell, staged through the three lerps.
///
/// Named for the corner they come from: `noise_xyz` where each digit is 0
/// (low) or 1 (high) on that axis.
#[derive(Clone, Copy, Debug, Default)]
pub struct CellInterpolator {
    noise_000: f64,
    noise_001: f64,
    noise_100: f64,
    noise_101: f64,
    noise_010: f64,
    noise_011: f64,
    noise_110: f64,
    noise_111: f64,

    // After the Y lerp.
    value_xz00: f64,
    value_xz10: f64,
    value_xz01: f64,
    value_xz11: f64,

    // After the X lerp.
    value_z0: f64,
    value_z1: f64,

    value: f64,
}

impl CellInterpolator {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            noise_000: 0.0,
            noise_001: 0.0,
            noise_100: 0.0,
            noise_101: 0.0,
            noise_010: 0.0,
            noise_011: 0.0,
            noise_110: 0.0,
            noise_111: 0.0,
            value_xz00: 0.0,
            value_xz10: 0.0,
            value_xz01: 0.0,
            value_xz11: 0.0,
            value_z0: 0.0,
            value_z1: 0.0,
            value: 0.0,
        }
    }

    /// `NoiseInterpolator.selectCellYZ` — load one cell's eight corners.
    ///
    /// `slice0` and `slice1` are the two x-slices of the lattice, each
    /// indexed `[z][y]`. Corner naming follows the axis order x, y, z, so
    /// `noise_100` comes from `slice1` (high x), low y, low z.
    pub fn select_cell(
        &mut self,
        slice0: &[Vec<f64>],
        slice1: &[Vec<f64>],
        cell_y: usize,
        cell_z: usize,
    ) {
        self.noise_000 = slice0[cell_z][cell_y];
        self.noise_001 = slice0[cell_z + 1][cell_y];
        self.noise_100 = slice1[cell_z][cell_y];
        self.noise_101 = slice1[cell_z + 1][cell_y];
        self.noise_010 = slice0[cell_z][cell_y + 1];
        self.noise_011 = slice0[cell_z + 1][cell_y + 1];
        self.noise_110 = slice1[cell_z][cell_y + 1];
        self.noise_111 = slice1[cell_z + 1][cell_y + 1];
    }

    /// Load eight corners already read out of a flat lattice.
    ///
    /// Order is `[000, 001, 100, 101, 010, 011, 110, 111]` in `xyz` digits,
    /// matching [`Self::select_cell`]'s reads. Kept as an array rather than
    /// eight arguments so a caller cannot silently transpose two of them.
    pub const fn load(&mut self, corners: [f64; 8]) {
        self.noise_000 = corners[0];
        self.noise_001 = corners[1];
        self.noise_100 = corners[2];
        self.noise_101 = corners[3];
        self.noise_010 = corners[4];
        self.noise_011 = corners[5];
        self.noise_110 = corners[6];
        self.noise_111 = corners[7];
    }

    /// First lerp — collapses the y axis, leaving four xz values.
    pub fn update_y(&mut self, factor_y: f64) {
        self.value_xz00 = lerp(factor_y, self.noise_000, self.noise_010);
        self.value_xz10 = lerp(factor_y, self.noise_100, self.noise_110);
        self.value_xz01 = lerp(factor_y, self.noise_001, self.noise_011);
        self.value_xz11 = lerp(factor_y, self.noise_101, self.noise_111);
    }

    /// Second lerp — collapses x, leaving two z values.
    pub fn update_x(&mut self, factor_x: f64) {
        self.value_z0 = lerp(factor_x, self.value_xz00, self.value_xz10);
        self.value_z1 = lerp(factor_x, self.value_xz01, self.value_xz11);
    }

    /// Third lerp — collapses z to the block's value.
    pub fn update_z(&mut self, factor_z: f64) {
        self.value = lerp(factor_z, self.value_z0, self.value_z1);
    }

    /// The interpolated value for the block the three updates selected.
    #[must_use]
    pub const fn value(&self) -> f64 {
        self.value
    }

    /// Direct trilinear sample, for filling a cell whose corners are already
    /// loaded — `NoiseChunk`'s `fillingCell` path.
    ///
    /// **This is `Mth.lerp3`, not the staged lerps, and the difference is
    /// observable.** `lerp3` collapses X, then Y, then Z; the staged path
    /// collapses Y, then X, then Z. Trilinear interpolation is
    /// order-independent in exact arithmetic but *not* in floating point:
    /// `lerp(t, a, b) = a + t * (b - a)` rounds twice, and the two orders
    /// round differently for roughly a fifth of interior blocks — always by
    /// exactly one ulp, which is precisely why the discrepancy survived four
    /// investigations before the decompile of `NoiseInterpolator.compute`
    /// made the two paths visible side by side.
    ///
    /// Which path a block gets is decided by `fillingCell`, and for terrain
    /// it is always this one: `finalDensity` is wrapped in `cacheAllInCell`,
    /// whose values array is filled during `selectCellYZ` with
    /// `fillingCell = true`. `getInterpolatedDensity` then *reads that array*
    /// — the staged `value` is computed by `updateForY/X/Z` and thrown away.
    /// So the staged path's only job is to move `inCellX/Y/Z`, which index
    /// the array.
    /// The staged path, evaluated in one call — `fillingCell == false`.
    ///
    /// Y, then X, then Z, exactly as `updateForY`/`updateForX`/`updateForZ`
    /// build it, but without keeping the partials. Used where a marker is
    /// read *outside* a cell fill: the ore veinifier's three inputs are the
    /// live example, since they sit in the block-state rule list rather than
    /// under `cacheAllInCell`.
    ///
    /// This is not [`Self::sample`] with the arguments shuffled — see the
    /// module docs. The two disagree by one ulp on about 45% of cells.
    #[must_use]
    pub fn sample_staged(&self, factor_x: f64, factor_y: f64, factor_z: f64) -> f64 {
        let xz00 = lerp(factor_y, self.noise_000, self.noise_010);
        let xz10 = lerp(factor_y, self.noise_100, self.noise_110);
        let xz01 = lerp(factor_y, self.noise_001, self.noise_011);
        let xz11 = lerp(factor_y, self.noise_101, self.noise_111);
        let z0 = lerp(factor_x, xz00, xz10);
        let z1 = lerp(factor_x, xz01, xz11);
        lerp(factor_z, z0, z1)
    }

    #[must_use]
    pub fn sample(&self, factor_x: f64, factor_y: f64, factor_z: f64) -> f64 {
        crate::math::lerp3(
            factor_x,
            factor_y,
            factor_z,
            self.noise_000,
            self.noise_100,
            self.noise_010,
            self.noise_110,
            self.noise_001,
            self.noise_101,
            self.noise_011,
            self.noise_111,
        )
    }
}

/// The lattice geometry for one chunk.
///
/// Mirrors the arithmetic in `NoiseChunk`'s constructor. Kept separate from
/// the interpolator so the geometry can be unit-tested without any noise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellGeometry {
    pub cell_width: i32,
    pub cell_height: i32,
    pub cell_count_xz: i32,
    pub cell_count_y: i32,
    pub cell_noise_min_y: i32,
    pub first_cell_x: i32,
    pub first_cell_z: i32,
}

impl CellGeometry {
    /// `min_y`/`height` come from the noise settings; the overworld uses
    /// -64 and 384 with a 4x8 cell.
    ///
    /// # Panics
    /// If a cell dimension is not positive, or the height is not a whole
    /// number of cells — vanilla assumes both and produces a ragged lattice
    /// otherwise.
    #[must_use]
    pub fn new(
        chunk_min_block_x: i32,
        chunk_min_block_z: i32,
        min_y: i32,
        height: i32,
        cell_width: i32,
        cell_height: i32,
    ) -> Self {
        assert!(
            cell_width > 0 && cell_height > 0,
            "cell dimensions must be positive"
        );
        assert!(
            height % cell_height == 0,
            "height {height} is not a whole number of {cell_height}-block cells"
        );
        Self {
            cell_width,
            cell_height,
            // A 16-block chunk over 4-wide cells is 4 cells across.
            cell_count_xz: 16 / cell_width,
            cell_count_y: height.div_euclid(cell_height),
            // Floor division, so a negative min_y lands on the cell below.
            cell_noise_min_y: min_y.div_euclid(cell_height),
            first_cell_x: chunk_min_block_x.div_euclid(cell_width),
            first_cell_z: chunk_min_block_z.div_euclid(cell_width),
        }
    }

    /// Block y at the bottom of cell row `cell_y`.
    #[must_use]
    pub const fn cell_start_block_y(&self, cell_y: i32) -> i32 {
        (cell_y + self.cell_noise_min_y) * self.cell_height
    }

    /// Block x at the low edge of cell column `cell_x` (chunk-relative).
    #[must_use]
    pub const fn cell_start_block_x(&self, cell_x: i32) -> i32 {
        (self.first_cell_x + cell_x) * self.cell_width
    }

    /// Block z at the low edge of cell column `cell_z` (chunk-relative).
    #[must_use]
    pub const fn cell_start_block_z(&self, cell_z: i32) -> i32 {
        (self.first_cell_z + cell_z) * self.cell_width
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slices() -> (Vec<Vec<f64>>, Vec<Vec<f64>>) {
        // slice[z][y]; low-x slice all 0 at y=0 and 10 at y=1, high-x slice
        // 100 / 110, and z=1 adds 1. Distinct enough that any wrong corner
        // shows up immediately.
        let slice0 = vec![vec![0.0, 10.0], vec![1.0, 11.0]];
        let slice1 = vec![vec![100.0, 110.0], vec![101.0, 111.0]];
        (slice0, slice1)
    }

    #[test]
    fn corners_are_reproduced_exactly() {
        let (s0, s1) = slices();
        let mut interp = CellInterpolator::new();
        interp.select_cell(&s0, &s1, 0, 0);

        for (fx, fy, fz, want) in [
            (0.0, 0.0, 0.0, 0.0),
            (0.0, 1.0, 0.0, 10.0),
            (1.0, 0.0, 0.0, 100.0),
            (1.0, 1.0, 0.0, 110.0),
            (0.0, 0.0, 1.0, 1.0),
            (1.0, 1.0, 1.0, 111.0),
        ] {
            assert_eq!(interp.sample(fx, fy, fz), want, "corner ({fx}, {fy}, {fz})");
        }
    }

    #[test]
    fn the_midpoint_is_the_mean_of_all_eight_corners() {
        let (s0, s1) = slices();
        let mut interp = CellInterpolator::new();
        interp.select_cell(&s0, &s1, 0, 0);
        let mean = (0.0 + 10.0 + 100.0 + 110.0 + 1.0 + 11.0 + 101.0 + 111.0) / 8.0;
        assert_eq!(interp.sample(0.5, 0.5, 0.5), mean);
    }

    /// A cell where the two orders provably disagree, found by random
    /// search over 200k cells (45% of them disagree; these constants are
    /// just the first hit, kept literal so the test needs no search).
    fn disagreeing_cell() -> (Vec<Vec<f64>>, Vec<Vec<f64>>, f64, f64, f64) {
        // slice[z][y]; low-x slice then high-x slice.
        let s0 = vec![
            vec![-0.163_655_697_258_481_06, 0.102_094_507_582_771_5],
            vec![0.130_907_388_386_159_34, 0.261_251_831_463_474_2],
        ];
        let s1 = vec![
            vec![-0.518_673_999_745_95, -0.881_778_987_842_021_7],
            vec![0.894_899_401_414_974_9, 0.165_993_808_920_814_6],
        ];
        (s0, s1, 0.25, 0.625, 0.75)
    }

    #[test]
    fn the_staged_path_collapses_y_then_x_then_z() {
        // Vanilla's updateForY / updateForX / updateForZ, in that order.
        let (s0, s1, fx, fy, fz) = disagreeing_cell();
        let mut interp = CellInterpolator::new();
        interp.select_cell(&s0, &s1, 0, 0);
        interp.update_y(fy);
        interp.update_x(fx);
        interp.update_z(fz);

        let xz00 = lerp(fy, s0[0][0], s0[0][1]);
        let xz10 = lerp(fy, s1[0][0], s1[0][1]);
        let xz01 = lerp(fy, s0[1][0], s0[1][1]);
        let xz11 = lerp(fy, s1[1][0], s1[1][1]);
        let expected = lerp(fz, lerp(fx, xz00, xz10), lerp(fx, xz01, xz11));

        assert_eq!(interp.value().to_bits(), expected.to_bits());
    }

    #[test]
    fn the_cell_fill_path_is_lerp3_and_collapses_x_first() {
        // `sample` models fillingCell == true, which is Mth.lerp3.
        let (s0, s1, fx, fy, fz) = disagreeing_cell();
        let mut interp = CellInterpolator::new();
        interp.select_cell(&s0, &s1, 0, 0);

        let y0 = lerp(
            fy,
            lerp(fx, s0[0][0], s1[0][0]),
            lerp(fx, s0[0][1], s1[0][1]),
        );
        let y1 = lerp(
            fy,
            lerp(fx, s0[1][0], s1[1][0]),
            lerp(fx, s0[1][1], s1[1][1]),
        );
        let expected = lerp(fz, y0, y1);

        assert_eq!(interp.sample(fx, fy, fz).to_bits(), expected.to_bits());
    }

    #[test]
    fn the_two_orders_disagree_by_one_ulp() {
        // The whole reason both paths exist. If this ever passes as "equal",
        // one of them has been quietly rewritten into the other and every
        // block sitting on a density tie will move.
        let (s0, s1, fx, fy, fz) = disagreeing_cell();
        let mut interp = CellInterpolator::new();
        interp.select_cell(&s0, &s1, 0, 0);
        let filled = interp.sample(fx, fy, fz);
        interp.update_y(fy);
        interp.update_x(fx);
        interp.update_z(fz);
        let staged = interp.value();

        assert_ne!(staged.to_bits(), filled.to_bits());
        let gap = staged.to_bits().abs_diff(filled.to_bits());
        assert_eq!(gap, 1, "expected a one-ulp gap, got {gap}");
    }

    #[test]
    fn overworld_geometry_matches_the_settings() {
        // Overworld: min_y -64, height 384, 4x8 cells.
        let g = CellGeometry::new(16, -32, -64, 384, 4, 8);
        assert_eq!(g.cell_count_xz, 4, "a 16-block chunk is 4 cells across");
        assert_eq!(g.cell_count_y, 48, "384 / 8");
        assert_eq!(g.cell_noise_min_y, -8, "-64 / 8");
        assert_eq!(g.first_cell_x, 4, "block 16 / 4");
        assert_eq!(g.first_cell_z, -8, "block -32 / 4, floored");
        assert_eq!(g.cell_start_block_y(0), -64);
        assert_eq!(g.cell_start_block_y(48), 320, "the top of the world");
    }

    #[test]
    fn negative_coordinates_floor_rather_than_truncate() {
        // Truncating would put block -1 in cell 0 alongside block 0, which
        // shifts every chunk west of the origin by a cell.
        let g = CellGeometry::new(-16, -16, -64, 384, 4, 8);
        assert_eq!(g.first_cell_x, -4);
        let truncating = -16 / 4;
        assert_eq!(g.first_cell_x, truncating, "aligned here by construction");

        let g = CellGeometry::new(-2, -2, -64, 384, 4, 8);
        assert_eq!(g.first_cell_x, -1, "block -2 belongs to cell -1, not 0");
    }
}
