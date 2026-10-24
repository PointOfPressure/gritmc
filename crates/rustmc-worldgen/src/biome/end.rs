//! The End's biome source.
//!
//! Reference: `net.minecraft.world.level.biome.TheEndBiomeSource`.
//!
//! Not a multi-noise source. The other two dimensions search a climate
//! `ParameterList` over six axes; this is a four-branch classifier over one
//! number, and the number is the island height field the router parks in its
//! `erosion` slot. There is no search, no tie-break and no parameter table —
//! which is why the End needs none of the machinery `biome_parameters_*.json`
//! exists for.

use crate::density::{DensityFunction, FunctionContext};

/// The five End biomes, in the order `collectPossibleBiomes` lists them.
///
/// Order is not load-bearing here — nothing searches this set — but it is the
/// order the codec writes and the order the classifier below reads best in.
pub const BIOMES: [&str; 5] = [
    "minecraft:the_end",
    "minecraft:end_highlands",
    "minecraft:end_midlands",
    "minecraft:small_end_islands",
    "minecraft:end_barrens",
];

/// `TheEndBiomeSource.getNoiseBiome(x, y, z, sampler)`, on quart coordinates.
///
/// `erosion` is the router's erosion slot — `cache_2d(end_islands)`.
///
/// # The bytecode, transcribed (26.2, offsets 0-158)
///
/// ```text
///   bx = QuartPos.toBlock(qx); by = QuartPos.toBlock(qy); bz = QuartPos.toBlock(qz)
///   cx = SectionPos.blockToSectionCoord(bx); cz = ...(bz)
///   if ((long) cx * cx + (long) cz * cz <= 4096L) return THE_END
///   sx = (SectionPos.blockToSectionCoord(bx) * 2 + 1) * 8
///   sz = (SectionPos.blockToSectionCoord(bz) * 2 + 1) * 8
///   d  = sampler.erosion().compute(new SinglePointContext(sx, by, sz))
///   if (d >   0.25)    return END_HIGHLANDS
///   if (d >= -0.0625)  return END_MIDLANDS
///   if (d <  -0.21875) return SMALL_END_ISLANDS
///   return END_BARRENS
/// ```
///
/// # Three things that are easy to get wrong and each move whole islands
///
/// * **The central-island test is on `<= 4096`, in `long` arithmetic, over
///   *chunk* coordinates** — so every quart cell whose chunk is within 64
///   chunks of the origin is `the_end`, whatever the island field says
///   there. `EndIslandDensityFunction` has the same 4096 test on the same
///   quantity, which is why the two agree about where the central island
///   ends.
/// * **The sample is taken at the chunk's centre, not at the quart cell.**
///   `(cx * 2 + 1) * 8` is `cx * 16 + 8`. All sixteen quart cells of a chunk
///   therefore get the *same* island value, and a biome boundary in the End
///   is always a chunk boundary.
/// * **The comparisons are asymmetric and not a sorted ladder**: strictly
///   greater for highlands, greater-or-equal for midlands, strictly less for
///   islands. `-0.0625` belongs to midlands and `-0.21875` to barrens. Two
///   of the four use the other sense of the test, and swapping either loses
///   a biome band at its edge.
#[must_use]
pub fn biome_at_quart(erosion: &DensityFunction, qx: i32, qy: i32, qz: i32) -> &'static str {
    let bx = qx << 2;
    let by = qy << 2;
    let bz = qz << 2;
    let cx = bx >> 4;
    let cz = bz >> 4;
    if i64::from(cx) * i64::from(cx) + i64::from(cz) * i64::from(cz) <= 4096 {
        return BIOMES[0];
    }
    let sx = (cx * 2 + 1) * 8;
    let sz = (cz * 2 + 1) * 8;
    let height = erosion.compute(FunctionContext::new(sx, by, sz));
    if height > 0.25 {
        BIOMES[1]
    } else if height >= -0.0625 {
        BIOMES[2]
    } else if height < -0.21875 {
        BIOMES[3]
    } else {
        BIOMES[4]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::density::router::OverworldRouter;

    /// Everything inside chunk radius 64 is the central island's biome, and
    /// the test is on chunk coordinates — so it holds at every y and every
    /// quart cell of those chunks.
    #[test]
    fn the_central_island_is_one_biome() {
        let router = OverworldRouter::end(20260722);
        // (64,0) and (45,45) sit on the boundary the `<=` includes: 4096 and
        // 4050. (-32,55) is 4049, one inside. (-32,56) is 4160 and is
        // *outside* — the radius is 64 chunks in the diagonal too, so the
        // set is a disc and not a square.
        for (cx, cz) in [(0, 0), (64, 0), (0, -64), (45, 45), (-32, 55)] {
            assert!(
                i64::from(cx) * i64::from(cx) + i64::from(cz) * i64::from(cz) <= 4096,
                "the fixture chunk ({cx},{cz}) is meant to be inside the island test"
            );
            // A quart cell inside that chunk: chunk cx covers quarts 4cx..4cx+3.
            let biome = biome_at_quart(&router.erosion, cx * 4 + 2, 16, cz * 4 + 1);
            assert_eq!(biome, "minecraft:the_end", "chunk ({cx},{cz})");
        }
    }

    /// All sixteen quart cells of one chunk read the same island value, so
    /// they cannot disagree about the biome.
    #[test]
    fn a_chunk_is_one_biome() {
        let router = OverworldRouter::end(20260722);
        for (cx, cz) in [(70, 70), (-90, 12), (65, -65)] {
            let first = biome_at_quart(&router.erosion, cx * 4, 0, cz * 4);
            for qx in 0..4 {
                for qz in 0..4 {
                    assert_eq!(
                        biome_at_quart(&router.erosion, cx * 4 + qx, 8, cz * 4 + qz),
                        first,
                        "chunk ({cx},{cz}) quart ({qx},{qz})"
                    );
                }
            }
        }
    }

    /// Outside the island test the classifier reaches the other four biomes,
    /// and every answer it gives is one of the five it is allowed.
    #[test]
    fn the_outer_islands_are_classified() {
        let router = OverworldRouter::end(20260722);
        let mut seen = std::collections::BTreeSet::new();
        for cx in 60..140 {
            for cz in 60..140 {
                if i64::from(cx) * i64::from(cx) + i64::from(cz) * i64::from(cz) <= 4096 {
                    continue;
                }
                let biome = biome_at_quart(&router.erosion, cx * 4, 0, cz * 4);
                assert!(BIOMES.contains(&biome), "{biome} is not an End biome");
                seen.insert(biome);
            }
        }
        assert!(
            seen.contains("minecraft:end_highlands") && seen.contains("minecraft:end_barrens"),
            "the outer End should reach at least highlands and barrens, saw {seen:?}"
        );
    }
}
