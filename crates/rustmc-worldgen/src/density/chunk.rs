//! Chunk-scoped density evaluation.
//!
//! Reference: `net.minecraft.world.level.levelgen.NoiseChunk`.
//!
//! # Why each interpolator is separate
//!
//! `final_density` contains **five** `interpolated(...)` markers — one in
//! `postProcess`, four in `noodle`. Vanilla gives each its own interpolator,
//! fills each on the cell lattice, and only then evaluates the surrounding
//! operators per block.
//!
//! Sampling `final_density` itself on the lattice and interpolating the
//! result is therefore wrong: it would interpolate *across* the `min`, `abs`
//! and `rangeChoice` boundaries that vanilla evaluates per block. Caves in
//! particular are carved by `min`, so smoothing across it fills them in.
//!
//! # Relation to vanilla's streaming
//!
//! Vanilla keeps two x-slices alive and swaps them as it advances, which is
//! a memory optimization over the whole-chunk lattice held here. The values
//! are identical — it fills the same corners from the same functions — and
//! the whole lattice is a few KB per chunk. Streaming can come back if a
//! benchmark asks for it; correctness first.

use super::cache_once::CacheOnceScratch;
use super::cell::{CellGeometry, CellInterpolator};
use super::flat_cache::FlatCaches;
use super::{DensityFunction, FunctionContext};

/// Which of `NoiseInterpolator.compute`'s two paths a root's markers take.
///
/// Vanilla decides this at read time from the `fillingCell` flag rather than
/// per function, but the flag is a property of *where* the read happens, and
/// in practice each root is only ever read from one place:
///
/// - `finalDensity` is wrapped in `cacheAllInCell`, whose array is filled
///   during `selectCellYZ` with `fillingCell = true`. Its markers therefore
///   go through `Mth.lerp3`.
/// - The ore veinifier's three inputs sit directly in the block-state rule
///   list, read with `fillingCell = false`, so they take the staged
///   `updateForY`/`updateForX`/`updateForZ` path.
///
/// The two orders disagree by one ulp on roughly 45% of cells, which is
/// enough to move a vein boundary — sampling the vein functions per block
/// instead of off the lattice, or off the lattice with the wrong order, both
/// show up as scattered ore/stone swaps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interpolation {
    /// Read during a cell fill — `Mth.lerp3`, X then Y then Z.
    CellFilled,
    /// Read outside one — staged, Y then X then Z.
    Staged,
}

/// One root function and how its interpolated markers are read.
pub struct Root {
    pub function: DensityFunction,
    pub interpolation: Interpolation,
}

impl Root {
    #[must_use]
    pub const fn cell_filled(function: DensityFunction) -> Self {
        Self {
            function,
            interpolation: Interpolation::CellFilled,
        }
    }

    #[must_use]
    pub const fn staged(function: DensityFunction) -> Self {
        Self {
            function,
            interpolation: Interpolation::Staged,
        }
    }
}

/// Evaluates one or more density graphs over one chunk, with cell
/// interpolation.
///
/// Multiple roots share a sampler because they share a lattice: vanilla's
/// `NoiseChunk.wrap` keeps one interpolator list for the whole chunk and
/// fills it once per cell, whatever reads it afterwards. Building a separate
/// sampler per root would still give the right values, but it would also
/// re-evaluate every corner, and it would hide that the roots must agree on
/// cell geometry.
pub struct ChunkSampler {
    geometry: CellGeometry,
    roots: Vec<Root>,
    /// The interpolator index range each root owns, so a marker's read path
    /// follows the root it belongs to.
    root_interpolators: Vec<std::ops::Range<usize>>,
    /// Corner values, flattened: `((interpolator * nx + x) * nz + z) * ny + y`.
    ///
    /// One allocation rather than a tree of `Vec<Vec<Vec<Vec<f64>>>>`. The
    /// nested form chased three pointers per corner, and a block reads eight
    /// corners per interpolator — 192 dependent loads per block, which is
    /// most of a cache miss each. Same values, same order, one array.
    lattice: Vec<f64>,
    nx: usize,
    nz: usize,
    ny: usize,
    /// The loaded corners per interpolator, refreshed when the cell changes.
    cells: Vec<CellInterpolator>,
    /// Which cell `cells` currently holds, so a run of blocks inside one cell
    /// reloads nothing. Vanilla gets this from `selectCellYZ` being called
    /// once per cell by the fill loop; ours is driven by arbitrary
    /// coordinates, so it has to notice for itself.
    loaded_cell: Option<(usize, usize, usize)>,
    /// Scratch for the current block's per-interpolator values.
    values: Vec<f64>,
    /// `CacheAllInCell` markers, numbered like the interpolators.
    cell_all: Vec<DensityFunction>,
    /// Each marker's values for every position in the loaded cell, indexed
    /// `[marker][cell_local]`. Vanilla fills these in `selectCellYZ` inside a
    /// `fillingCell` window, during which an interpolated descendant yields
    /// its **raw** value — so these are raw, and serving them for the whole
    /// cell is what bypasses interpolation for that subtree.
    cell_all_values: Vec<Vec<f64>>,
    /// Scratch for the current block's per-marker values.
    cell_all_scratch: Vec<f64>,
}

impl ChunkSampler {
    /// Build a sampler for the chunk whose minimum block corner is
    /// (`chunk_min_block_x`, `chunk_min_block_z`).
    ///
    /// Fills every interpolator's lattice up front; after this, evaluation is
    /// pure lookup plus interpolation.
    #[must_use]
    pub fn new(mut roots: Vec<Root>, flat: &FlatCaches, geometry: CellGeometry) -> Self {
        // Numbered across all roots into one list, so every marker gets a
        // globally unique slot and the roots can share `values`.
        let mut wrapped = Vec::new();
        let mut root_interpolators = Vec::with_capacity(roots.len());
        for root in &mut roots {
            let first = wrapped.len();
            root.function.number_interpolators_into(&mut wrapped);
            root_interpolators.push(first..wrapped.len());
        }

        let nx = (geometry.cell_count_xz + 1) as usize;
        let nz = (geometry.cell_count_xz + 1) as usize;
        let ny = (geometry.cell_count_y + 1) as usize;

        // Each interpolator samples its own wrapped function at the corners.
        // The corner context has no interpolated values of its own, which is
        // vanilla's behavior: a marker sampled outside the fill computes its
        // wrapped function directly.
        // `slopedCheese` and the spaghetti rarity modulator are `cacheOnce`
        // and are read more than once per point — the rangeChoice input, then
        // again in whichever branch wins. The lattice prefill is where that
        // happens, so the scratch lives for exactly this loop.
        //
        // The slot count is *read* from the graph rather than assigned here.
        // Slots are fixed when the router is built, which is what lets the
        // three clones of `cheese` share one cache entry; renumbering them
        // per chunk would split them apart again.
        let cache_once_slots = roots
            .iter()
            .map(|root| root.function.cache_once_slots())
            .max()
            .unwrap_or(0);
        let scratch = CacheOnceScratch::new(cache_once_slots);

        let mut lattice = Vec::with_capacity(wrapped.len() * nx * nz * ny);
        for f in &wrapped {
            for cx in 0..nx {
                let x = geometry.cell_start_block_x(cx as i32);
                for cz in 0..nz {
                    let z = geometry.cell_start_block_z(cz as i32);
                    for cy in 0..ny {
                        let y = geometry.cell_start_block_y(cy as i32);
                        // New point, so nothing memoized for the last one may
                        // survive. Over-bumping only costs a cache miss;
                        // under-bumping would serve a neighbour's value.
                        scratch.advance();
                        lattice
                            .push(f.compute(FunctionContext::with_caches(x, y, z, flat, &scratch)));
                    }
                }
            }
        }

        let values = vec![0.0; wrapped.len()];
        let cells = vec![CellInterpolator::new(); wrapped.len()];
        let mut cell_all = Vec::new();
        for root in &mut roots {
            root.function
                .number_markers_into(crate::density::MarkerKind::CacheAllInCell, &mut cell_all);
        }
        let cell_positions =
            (geometry.cell_width * geometry.cell_height * geometry.cell_width) as usize;
        let cell_all_values = vec![vec![0.0; cell_positions]; cell_all.len()];
        let cell_all_scratch = vec![0.0; cell_all.len()];
        Self {
            geometry,
            roots,
            root_interpolators,
            lattice,
            nx,
            nz,
            ny,
            cells,
            loaded_cell: None,
            values,
            cell_all,
            cell_all_values,
            cell_all_scratch,
        }
    }

    /// One corner value out of the flat lattice.
    #[inline]
    fn corner(&self, interpolator: usize, x: usize, z: usize, y: usize) -> f64 {
        self.lattice[((interpolator * self.nx + x) * self.nz + z) * self.ny + y]
    }

    #[must_use]
    pub const fn geometry(&self) -> &CellGeometry {
        &self.geometry
    }

    /// Value of root `root` at one block inside this chunk.
    ///
    /// # Panics
    /// If the block lies outside the chunk's lattice, or `root` is unknown.
    pub fn evaluate(
        &mut self,
        flat: &FlatCaches,
        root: usize,
        block_x: i32,
        block_y: i32,
        block_z: i32,
    ) -> f64 {
        let g = self.geometry;

        // Which cell, and where inside it.
        let rel_x = block_x - g.cell_start_block_x(0);
        let rel_z = block_z - g.cell_start_block_z(0);
        let rel_y = block_y - g.cell_start_block_y(0);
        assert!(
            rel_x >= 0 && rel_z >= 0 && rel_y >= 0,
            "block ({block_x}, {block_y}, {block_z}) is below this chunk's lattice"
        );

        let cell_x = (rel_x / g.cell_width) as usize;
        let cell_z = (rel_z / g.cell_width) as usize;
        let cell_y = (rel_y / g.cell_height) as usize;
        let factor_x = f64::from(rel_x % g.cell_width) / f64::from(g.cell_width);
        let factor_z = f64::from(rel_z % g.cell_width) / f64::from(g.cell_width);
        let factor_y = f64::from(rel_y % g.cell_height) / f64::from(g.cell_height);

        // Corners are reloaded only when the cell changes. A cell is 4x8x4,
        // so this is 128 times less lattice traffic than reloading per block
        // — the same saving vanilla gets from `selectCellYZ` being called
        // once per cell rather than once per block.
        if self.loaded_cell != Some((cell_x, cell_y, cell_z)) {
            self.loaded_cell = Some((cell_x, cell_y, cell_z));
            for index in 0..self.cells.len() {
                let corners = [
                    self.corner(index, cell_x, cell_z, cell_y),
                    self.corner(index, cell_x, cell_z + 1, cell_y),
                    self.corner(index, cell_x + 1, cell_z, cell_y),
                    self.corner(index, cell_x + 1, cell_z + 1, cell_y),
                    self.corner(index, cell_x, cell_z, cell_y + 1),
                    self.corner(index, cell_x, cell_z + 1, cell_y + 1),
                    self.corner(index, cell_x + 1, cell_z, cell_y + 1),
                    self.corner(index, cell_x + 1, cell_z + 1, cell_y + 1),
                ];
                self.cells[index].load(corners);
            }
            // `selectCellYZ`'s other half: fill every `CacheAllInCell`
            // marker across the whole cell. The context carries **no**
            // interpolated values, so an interpolated descendant falls back
            // to its wrapped function — which is exactly what vanilla's
            // `fillingCell` flag makes it do.
            let g = self.geometry;
            for (marker, values) in self.cell_all.iter().zip(self.cell_all_values.iter_mut()) {
                let mut slot = 0usize;
                for dy in 0..g.cell_height {
                    for dx in 0..g.cell_width {
                        for dz in 0..g.cell_width {
                            let bx = g.cell_start_block_x(0) + cell_x as i32 * g.cell_width + dx;
                            let by = g.cell_start_block_y(0) + cell_y as i32 * g.cell_height + dy;
                            let bz = g.cell_start_block_z(0) + cell_z as i32 * g.cell_width + dz;
                            values[slot] = marker.compute(FunctionContext::with_interpolated(
                                bx,
                                by,
                                bz,
                                &[],
                                flat,
                                None,
                            ));
                            slot += 1;
                        }
                    }
                }
            }
        }
        // This block's slice of the cell cache.
        if !self.cell_all.is_empty() {
            let g = self.geometry;
            let local = ((rel_y % g.cell_height) * g.cell_width * g.cell_width
                + (rel_x % g.cell_width) * g.cell_width
                + (rel_z % g.cell_width)) as usize;
            for (i, values) in self.cell_all_values.iter().enumerate() {
                self.cell_all_scratch[i] = values[local];
            }
        }

        // Only this root's markers are blended. The others' slots are stale
        // but unreachable from this root, and blending them would cost a lerp
        // per marker per block for nothing.
        let range = self.root_interpolators[root].clone();
        let interpolation = self.roots[root].interpolation;
        for index in range {
            let cell = &self.cells[index];
            self.values[index] = match interpolation {
                Interpolation::CellFilled => cell.sample(factor_x, factor_y, factor_z),
                Interpolation::Staged => cell.sample_staged(factor_x, factor_y, factor_z),
            };
        }

        self.roots[root]
            .function
            .compute(FunctionContext::with_cell_values(
                block_x,
                block_y,
                block_z,
                &self.values,
                &self.cell_all_scratch,
                flat,
                None,
            ))
    }

    /// Root 0, which by convention is the density the fill thresholds on.
    pub fn density(&mut self, flat: &FlatCaches, block_x: i32, block_y: i32, block_z: i32) -> f64 {
        self.evaluate(flat, 0, block_x, block_y, block_z)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::density::{build, Mapped};

    /// A graph with an interpolated marker inside a `min`, mirroring how
    /// `final_density` wraps one.
    ///
    /// Two properties the interpolated branch needs, both learned the hard
    /// way while writing these tests:
    ///
    /// - It must be **non-linear**. Trilinear interpolation reproduces
    ///   linear functions exactly, so a plain gradient is indistinguishable
    ///   from per-block evaluation.
    /// - The `min` must actually **select** it. With a competing branch that
    ///   always wins, the interpolated value never reaches the output and
    ///   the test proves nothing.
    fn graph() -> DensityFunction {
        build::min(
            build::interpolated(build::map(
                Mapped::Square,
                build::y_clamped_gradient(-64, 320, 1.0, -1.0),
            )),
            build::constant(10.0),
        )
    }

    #[test]
    fn a_unit_lattice_reproduces_per_block_evaluation() {
        // With 1x1 cells every block is a corner, so interpolation is exact
        // and the chunk fill must equal plain per-block compute bit-for-bit.
        // This is the invariant that proves the plumbing is transparent.
        let mut sampler = ChunkSampler::new(
            vec![Root::cell_filled(graph())],
            &FlatCaches::none(),
            CellGeometry::new(0, 0, -64, 384, 1, 1),
        );
        let plain = graph();
        for y in [-64, -1, 0, 37, 319] {
            for (x, z) in [(0, 0), (5, 11), (15, 15)] {
                assert_eq!(
                    sampler.density(&FlatCaches::none(), x, y, z).to_bits(),
                    plain.compute(FunctionContext::new(x, y, z)).to_bits(),
                    "unit lattice diverged at ({x}, {y}, {z})"
                );
            }
        }
    }

    #[test]
    fn a_coarse_lattice_smooths_only_the_marked_node() {
        // At 4x8 the interpolated branch is smoothed, so it must differ from
        // per-block evaluation somewhere off a corner — otherwise the
        // lattice is not doing anything.
        let mut sampler = ChunkSampler::new(
            vec![Root::cell_filled(graph())],
            &FlatCaches::none(),
            CellGeometry::new(0, 0, -64, 384, 4, 8),
        );
        let plain = graph();
        let differs = (0..16).any(|y| {
            let block_y = -64 + y;
            sampler
                .density(&FlatCaches::none(), 3, block_y, 2)
                .to_bits()
                != plain.compute(FunctionContext::new(3, block_y, 2)).to_bits()
        });
        assert!(differs, "a coarse lattice must change interior values");
    }

    #[test]
    fn corners_are_exact_even_on_a_coarse_lattice() {
        // Cell corners are sampled directly, so they must match per-block
        // evaluation regardless of cell size.
        let mut sampler = ChunkSampler::new(
            vec![Root::cell_filled(graph())],
            &FlatCaches::none(),
            CellGeometry::new(0, 0, -64, 384, 4, 8),
        );
        let plain = graph();
        for (x, y, z) in [(0, -64, 0), (4, -56, 8), (12, -32, 12)] {
            assert_eq!(
                sampler.density(&FlatCaches::none(), x, y, z).to_bits(),
                plain.compute(FunctionContext::new(x, y, z)).to_bits(),
                "corner ({x}, {y}, {z}) should be exact"
            );
        }
    }

    #[test]
    fn every_interpolated_marker_gets_its_own_index() {
        // The point of the design: five markers, five lattices, no sharing.
        let mut f = build::min(
            build::interpolated(build::constant(1.0)),
            build::add(
                build::interpolated(build::constant(2.0)),
                build::interpolated(build::constant(3.0)),
            ),
        );
        let wrapped = f.number_interpolators();
        assert_eq!(wrapped.len(), 3);
    }
}
