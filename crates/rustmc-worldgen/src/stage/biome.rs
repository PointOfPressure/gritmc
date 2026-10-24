//! The biome stage — filling a chunk's 4x4x4 climate cells.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `ChunkAccess#fillBiomesFromNoise`, `BiomeResolver`.
//!
//! # It is not decoration that needs this, it is everything after noise
//!
//! Surface rules ask the biome at a block; carvers resolve their carver list
//! from it; decoration's whole attempt order comes from the set of biomes
//! present in the chunk's *palettes*. `SurfaceChunkGolden` had to grow a
//! `doCreateBiomes` call for exactly that reason — a harness that never
//! filled the containers collected an empty biome set, decorated nothing, and
//! reported no error, because an empty set is a legal answer.
//!
//! So this stage is not optional in any pipeline that goes past noise, and it
//! runs for every chunk before the first surface column rather than per-chunk
//! interleaved: a surface rule near a chunk edge may ask the biome across it.
//!
//! # The loop order is behaviour, not taste
//!
//! `Climate.RTree.search` seeds each query with the previous query's result
//! and prunes with a strict `>`, so a box exactly as close as the running
//! candidate never displaces it. At a point where two biomes are *equally*
//! near, the winner is therefore whichever the previous cell resolved to —
//! the answer is a function of the order the cells are visited.
//!
//! Ties are not hypothetical: measured at **0.195%** of sampled cells
//! (`golden_climate_ties.rs`). Filling in a different order than vanilla
//! produces a world that is right almost everywhere and quietly wrong at a
//! coastline, which is exactly the kind of fault a small sample misses — a
//! 9-chunk diff reported zero biome mismatches while a 169-chunk diff found
//! four columns.
//!
//! So the nesting below is copied from the bytecode, not chosen:
//! **sections bottom to top, then x, then y within the section, then z**
//! (`ChunkAccess#fillBiomesFromNoise` driving
//! `LevelChunkSection#fillBiomesFromNoise`). Do not "simplify" it into a
//! single full-height y loop; that is what it used to be.
//!
//! # The sample position is quart, and the y is not ignored
//!
//! One sample per 4x4x4 cell at the cell's own quart coordinates. The y
//! matters — `depth` is one of the six climate axes — which is why a cave at
//! y=-40 can be a different biome from the surface directly above it.

use crate::canvas::Canvas;

/// Answers `getNoiseBiome(quartX, quartY, quartZ)`.
///
/// A closure rather than the biome source itself: `MultiNoiseBiomeSource`
/// needs `&mut self` for its search cache, and threading that through the
/// stage would put a lifetime on the pipeline for one call.
pub type BiomeAt<'a> = dyn FnMut(i32, i32, i32) -> String + 'a;

/// Every cell of one chunk, as **quart** coordinates, in the order
/// `fillBiomesFromNoise` visits them.
///
/// Spelled once and used by everything that walks the cells, because the
/// order is behaviour rather than taste — see the module docs — and two
/// walks that were written out separately are two walks that can drift. The
/// nesting is copied from the bytecode: **sections bottom to top, then x,
/// then y within the section, then z.**
pub fn cells(
    min_y: i32,
    height: i32,
    chunk_x: i32,
    chunk_z: i32,
) -> impl Iterator<Item = (i32, i32, i32)> {
    let quart_base_x = chunk_x << 2;
    let quart_base_z = chunk_z << 2;
    let min_section = min_y >> 4;
    let max_section = (min_y + height - 1) >> 4;
    (min_section..=max_section).flat_map(move |section_y| {
        (0..4).flat_map(move |cell_x| {
            (0..4).flat_map(move |cell_y| {
                (0..4).map(move |cell_z| {
                    (
                        quart_base_x + cell_x,
                        (section_y << 2) + cell_y,
                        quart_base_z + cell_z,
                    )
                })
            })
        })
    })
}

/// Fill one chunk's biome cells.
pub fn fill_chunk(canvas: &mut Canvas, biome_at: &mut BiomeAt<'_>, chunk_x: i32, chunk_z: i32) {
    let min_y = canvas.min_y();
    let height = canvas.height();
    for (quart_x, quart_y, quart_z) in cells(min_y, height, chunk_x, chunk_z) {
        let biome = biome_at(quart_x, quart_y, quart_z);
        canvas.set_biome(quart_x << 2, quart_y << 2, quart_z << 2, &biome);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::{Area, Pipeline};

    /// Every cell is sampled once, at its own quart coordinates, and the y
    /// is passed through rather than pinned.
    #[test]
    fn every_cell_is_sampled_once_at_quart_coordinates() {
        let mut pipeline = Pipeline::new(Area::square(0, 0, 0), -64, 384, "minecraft:air");
        let mut seen: Vec<(i32, i32, i32)> = Vec::new();
        {
            let mut at = |x: i32, y: i32, z: i32| {
                seen.push((x, y, z));
                format!("minecraft:b{y}")
            };
            fill_chunk(pipeline.canvas_mut(), &mut at, 0, 0);
        }
        assert_eq!(seen.len(), 96 * 16, "96 layers of 16 cells");
        assert!(seen.contains(&(0, -16, 0)), "the bottom layer is quart -16");
        assert!(seen.contains(&(3, 79, 3)), "and the top is quart 79");
        assert_eq!(
            pipeline.canvas().biome_at(0, -64, 0),
            "minecraft:b-16",
            "the y reaches the sampler rather than being dropped"
        );
    }

    /// The **order** the cells are visited in, which is behaviour rather
    /// than an implementation detail: the climate search resolves exact ties
    /// to whatever the previous query returned, so visiting the same cells
    /// in a different order legitimately produces different biomes. Vanilla
    /// walks sections bottom to top, then x, then y within the section, then
    /// z (`LevelChunkSection#fillBiomesFromNoise`).
    #[test]
    fn cells_are_visited_in_vanillas_order() {
        let mut pipeline = Pipeline::new(Area::square(0, 0, 0), -64, 384, "minecraft:air");
        let mut seen: Vec<(i32, i32, i32)> = Vec::new();
        {
            let mut at = |x: i32, y: i32, z: i32| {
                seen.push((x, y, z));
                String::from("minecraft:plains")
            };
            fill_chunk(pipeline.canvas_mut(), &mut at, 0, 0);
        }

        let mut want: Vec<(i32, i32, i32)> = Vec::with_capacity(96 * 16);
        for section_y in -4..=19 {
            for cell_x in 0..4 {
                for cell_y in 0..4 {
                    for cell_z in 0..4 {
                        want.push((cell_x, (section_y << 2) + cell_y, cell_z));
                    }
                }
            }
        }
        assert_eq!(
            seen, want,
            "the biome fill visits cells in a different order than vanilla; \
             that is invisible in open terrain and wrong at ~0.2% of cells, \
             where two biomes are exactly equidistant in climate space"
        );

        // Spot-check the three ways the nesting could be wrong, so a failure
        // says which one it is rather than dumping 1,536 tuples.
        assert_eq!(
            seen[0],
            (0, -16, 0),
            "first cell: bottom section, x=0,y=0,z=0"
        );
        assert_eq!(seen[1], (0, -16, 1), "z is the innermost axis, not x");
        assert_eq!(
            seen[4],
            (0, -15, 0),
            "y is the middle axis, and stays inside the section"
        );
        assert_eq!(
            seen[16],
            (1, -16, 0),
            "x is the outer axis within a section"
        );
        assert_eq!(
            seen[64],
            (0, -12, 0),
            "the section advances only after all 64 of its cells"
        );
    }

    /// A chunk at a negative position samples the quart coordinates its own
    /// blocks are at, not a shifted copy.
    #[test]
    fn a_negative_chunk_samples_its_own_quarts() {
        let mut pipeline = Pipeline::new(Area::square(-1, -1, 0), -64, 384, "minecraft:air");
        let mut at = |x: i32, _y: i32, z: i32| format!("minecraft:q{x}_{z}");
        fill_chunk(pipeline.canvas_mut(), &mut at, -1, -1);
        // Chunk -1 spans blocks -16..-1, i.e. quarts -4..-1.
        assert_eq!(pipeline.canvas().biome_at(-16, 0, -16), "minecraft:q-4_-4");
        assert_eq!(pipeline.canvas().biome_at(-1, 0, -1), "minecraft:q-1_-1");
    }
}
