//! The noise stage, onto a canvas.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `NoiseBasedChunkGenerator#fillFromNoise`, `NoiseChunk#forChunk`.
//!
//! This is a thin adapter, not new generation: [`crate::chunk_gen::NoiseFill`]
//! already produces a chunk and is proved bit-exact over 1.77M blocks. What
//! it produces is a grid of [`Substance`] — solid, water, lava, air, or an
//! ore-vein block — and what the later stages want is block *states*. The
//! translation is `anvil_bridge::block_name`, the same one the region writer
//! uses, so the two cannot drift.
//!
//! # The router is rebuilt per chunk, and that is vanilla's shape
//!
//! `NoiseChunk.forChunk` wraps the world's router in per-chunk markers, and
//! this port clones the graphs instead. It is not free — BENCH.md has the
//! number — and it is not the reason a comparison against vanilla is fair or
//! unfair, because vanilla pays for the wrapping too. It is named here so a
//! future optimisation knows what it is allowed to change: the *cost*, never
//! the values.

use crate::canvas::Canvas;
use crate::chunk_gen::{FillInputs, FillSettings, NoiseFill, Wg};
use crate::density::aquifer::AquiferNoises;
use crate::density::ore_vein::OreVeinifier;
use crate::density::router::OverworldRouter;
use crate::noise::NoiseSeeding;

/// What the noise stage kept for the carvers.
///
/// Carvers run long after the fill and ask the **same** aquifer what belongs
/// in the holes they cut — vanilla keeps the `NoiseChunk` alive across both
/// stages for exactly that reason. Rebuilding a fresh aquifer would give the
/// same values (its location cache is a memo of positional draws, so it is a
/// pure function of position) and would recompute every one of them.
///
/// The pipeline dropped this on its first pass, and the whole-world diff said
/// so: 6,638 of 15,985 mismatches — 41% — were a carved block that vanilla
/// filled with water or air and this port filled with `cave_air`, because the
/// carver was asking a constant instead of the water table.
pub struct Retained {
    pub aquifer: Option<crate::density::aquifer::NoiseBasedAquifer>,
    pub flat: crate::density::flat_cache::FlatCaches,
    pub surface: crate::density::surface_level::SurfaceLevels,
    /// `ChunkAccess#markPosForPostProcessing`, as
    /// `NoiseBasedChunkGenerator#doFill` calls it: every fluid position the
    /// aquifer answered while `shouldScheduleFluidUpdate` was set.
    ///
    /// It leaves this stage as a value rather than being written straight
    /// into the canvas because the stage takes a [`ChunkWriter`], which is
    /// one chunk's blocks and deliberately holds no reference to the canvas's
    /// mark set — that is the whole reason the stage can be run on a worker
    /// pool with no lock. The caller unions them back after the pool joins,
    /// which is order-independent because the destination is a set.
    pub post_processing: Vec<(i32, i32, i32)>,
}

/// Fill one chunk's blocks from the density router.
///
/// `default_block` is the generator setting's, which for the overworld is
/// stone — `Substance::Solid` carries no identity of its own on purpose, so
/// a caller generating the nether passes netherrack and nothing else changes.
// The beardifier makes this eight. Bundling the arguments into a struct would
// hide which of them is per-chunk and which is per-world, and that distinction
// is the entire reason the beard rides in `FillInputs` instead of the router.
#[allow(clippy::too_many_arguments)]
pub fn fill_chunk(
    canvas: &mut Canvas,
    router: &OverworldRouter,
    seeding: &NoiseSeeding,
    settings: FillSettings,
    default_block: &str,
    chunk_x: i32,
    chunk_z: i32,
    beardifier: Option<std::sync::Arc<crate::density::beard::Beardifier>>,
) -> Retained {
    let mut writers = canvas.chunk_writers();
    let writer = writers
        .iter_mut()
        .find(|w| w.position() == (chunk_x, chunk_z))
        .expect("the chunk being filled is on the canvas");
    fill_chunk_into(writer, router, seeding, settings, default_block, beardifier)
}

/// The same fill, through a borrow of one chunk instead of the whole canvas.
///
/// # Why this shape
///
/// Every write this stage makes lands inside its own chunk — `base_x + 0..16`
/// by `base_z + 0..16`, and its own two `_WG` heightmaps — and it runs before
/// [`Canvas::set_wg_live`], so no write triggers the read-modify-write of a
/// neighbour's column that `update_wg` performs. Those two facts are what
/// make the stage chunk-local, and taking a [`ChunkWriter`] rather than a
/// `&mut Canvas` is how they become a property the compiler checks instead of
/// a comment.
///
/// A worker pool can therefore run this over disjoint chunks with no lock and
/// no `unsafe`, and get the same blocks: the chunk's palette is interned in
/// this function's own write order, which does not depend on what any other
/// chunk did.
pub fn fill_chunk_into(
    writer: &mut crate::canvas::ChunkWriter<'_>,
    router: &OverworldRouter,
    seeding: &NoiseSeeding,
    settings: FillSettings,
    default_block: &str,
    beardifier: Option<std::sync::Arc<crate::density::beard::Beardifier>>,
) -> Retained {
    let (chunk_x, chunk_z) = writer.position();
    let fill = NoiseFill::new(
        FillInputs {
            beardifier,
            ..inputs(router, seeding, settings)
        },
        settings,
        chunk_x,
        chunk_z,
    );
    let post = fill.fill_retaining();
    let chunk = &post.chunk;

    // The two `_WG` heightmaps, recorded now and never again. The surface
    // rules read `WORLD_SURFACE_WG` for their `stone_depth` and the ore
    // chains read `OCEAN_FLOOR_WG`; both are fossils of *this* moment, before
    // a single surface block or feature exists, and a pipeline that let them
    // move later would put the ore chains' ceiling on top of the canopy.
    let mut world_surface = [writer.min_y(); 256];
    let mut ocean_floor = [writer.min_y(); 256];
    for z in 0..16i32 {
        for x in 0..16i32 {
            let column = usize::try_from(z * 16 + x).expect("a positive index");
            world_surface[column] = chunk.world_surface(x, z);
            ocean_floor[column] = chunk.ocean_floor(x, z);
        }
    }
    writer.set_wg_heightmaps(&world_surface, &ocean_floor);

    let base_x = chunk_x * 16;
    let base_z = chunk_z * 16;
    // **The canvas can be taller than the generator.** `NoiseSettings` is
    // clamped to the dimension's height accessor
    // (`NoiseSettings.clampToHeightAccessor`, and `doFill` walks the clamped
    // range), and the nether is the case where they differ: its noise
    // settings are y 0..128 while `DimensionType` gives it a 256-block
    // storage range, so a vanilla nether chunk has sixteen sections and the
    // top eight are air. Walking the canvas's range instead would read
    // `FilledChunk` outside the lattice it was built for.
    let fill_lo = writer.min_y().max(settings.min_y);
    let fill_hi = (writer.min_y() + writer.height()).min(settings.min_y + settings.height);
    for y in fill_lo..fill_hi {
        for z in 0..16 {
            for x in 0..16 {
                let substance = chunk.get(x, y, z);
                let name = crate::anvil_bridge::block_name(substance, default_block);
                // Air is what the canvas already holds, and writing it back
                // would put `minecraft:air` in every chunk's palette at index
                // one as well as zero. Skipping is not an optimisation; it
                // keeps the palette honest.
                if name == "minecraft:air" {
                    continue;
                }
                writer.set_block(base_x + x, y, base_z + z, name);
            }
        }
    }

    let post_processing = post.chunk.post_processing().to_vec();

    Retained {
        aquifer: post.aquifer,
        flat: post.flat,
        surface: post.surface,
        post_processing,
    }
}

/// The per-chunk clone of the router's graphs.
///
/// Lifted verbatim from `worldgen-dump`, which had it first and now uses
/// this: two copies of the aquifer's noise list is exactly the shape of
/// oracle fault 5b, one pipeline diverging from another because someone
/// updated one of them.
fn inputs(router: &OverworldRouter, seeding: &NoiseSeeding, settings: FillSettings) -> FillInputs {
    FillInputs {
        final_density: router.final_density.clone(),
        aquifer_noises: AquiferNoises {
            barrier: router.barrier.clone(),
            fluid_level_floodedness: router.fluid_level_floodedness.clone(),
            fluid_level_spread: router.fluid_level_spread.clone(),
            lava: router.lava.clone(),
            erosion: router.erosion.clone(),
            depth: router.depth.clone(),
        },
        preliminary_surface_level: router.preliminary_surface_level.clone(),
        aquifer_random: seeding.source_for("minecraft:aquifer").fork_positional(),
        // Built only when the settings enable veins. Not an optimisation:
        // `OreVeinifier::new` is where the vein functions become sampler
        // roots, and a dimension with `ore_veins_enabled: false` has a zero
        // vein graph, so constructing one would add three constant roots to
        // every chunk's lattice for nothing.
        ore_veinifier: settings.ore_veins_enabled.then(|| {
            OreVeinifier::new(
                router.vein_toggle.clone(),
                router.vein_ridged.clone(),
                router.vein_gap.clone(),
                seeding.source_for("minecraft:ore").fork_positional(),
            )
        }),
        // **Beardless, and every caller that wants a beard must say so.**
        // This one function feeds both `fill_chunk_into` and `base_height`,
        // and vanilla beards exactly one of them: `iterateNoiseColumn` builds
        // its `NoiseChunk` with `BeardifierMarker.INSTANCE` at offset 187, so
        // `getBaseHeight` sees unbearded terrain by design. Setting a beard
        // here would give it one, and structure placement would start
        // standing on the ground its own structures raised.
        beardifier: None,
    }
}

/// `ChunkGenerator.getBaseHeight` for one column, against the router.
///
/// The structures stage's five canvas heightmap reads exist because the stage
/// runs after the noise stage. Vanilla runs `STRUCTURE_STARTS` **before**
/// `NOISE` and answers those queries with this, which needs no filled chunk.
/// Swapping the reads for this is what unblocks moving the stage, and moving
/// the stage is what makes a `Beardifier` possible at all.
///
/// Deliberately not memoised yet. A structure placement makes a handful of
/// these per chunk, and a cache keyed on the chunk would be a second thing to
/// get wrong before the first one is proved.
#[must_use]
pub fn base_height(
    router: &OverworldRouter,
    seeding: &NoiseSeeding,
    settings: FillSettings,
    x: i32,
    z: i32,
    kind: Wg,
) -> i32 {
    NoiseFill::new(inputs(router, seeding, settings), settings, x >> 4, z >> 4)
        .base_height(x, z, kind)
}

/// `ChunkGenerator.getBaseColumn` for one column, against the router.
///
/// Element 0 is `settings.min_y`, matching vanilla's `NoiseColumn(minY, ..)`.
///
/// Beardless, and it must stay that way: `inputs` leaves
/// `FillInputs::beardifier` `None`, and `iterateNoiseColumn` passes
/// `BeardifierMarker.INSTANCE` at offset 187 for the same reason. A structure
/// that asked a bearded column where the ground is would be reading the
/// ground its own kind raises.
///
/// Not memoised. `nether_fossil` makes one of these per placement chunk and
/// throws it away; a cache is a second thing to get wrong before the first is
/// proved.
#[must_use]
pub fn base_column(
    router: &OverworldRouter,
    seeding: &NoiseSeeding,
    settings: FillSettings,
    x: i32,
    z: i32,
) -> Vec<crate::density::fill::Substance> {
    NoiseFill::new(inputs(router, seeding, settings), settings, x >> 4, z >> 4).base_column(x, z)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::density::router::{OverworldNoises, OverworldRouter};
    use crate::noise::NoiseParameters;
    use crate::pipeline::{Area, Pipeline};

    /// The noise parameters the other gates already read, so this stage is
    /// measured against the same world they are.
    fn noises() -> Option<OverworldNoises> {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/golden/router_vectors.json");
        // Committed, so this is not the gitignored-extract case — but the two
        // tests behind it are the terrain and determinism claims for the whole
        // noise stage, and a `.ok()?` here turned both into passes for any
        // reason the file failed to read.
        if !rustmc_data::fixture::available(
            std::path::Path::new(path),
            "router vectors",
            "ci/router-golden.sh",
        ) {
            return None;
        }
        let text = std::fs::read_to_string(path).ok()?;
        let doc: serde_json::Value = serde_json::from_str(&text).ok()?;
        let mut set = OverworldNoises::new();
        for entry in doc["parameters"].as_array()? {
            let amplitudes: Vec<f64> = entry["amplitudes"]
                .as_array()?
                .iter()
                .map(|v| {
                    let bits: i64 = v.as_str().expect("a string").parse().expect("an i64");
                    #[expect(
                        clippy::cast_sign_loss,
                        reason = "the golden file carries raw bits through a signed long"
                    )]
                    let bits = bits as u64;
                    f64::from_bits(bits)
                })
                .collect();
            set = set.with(
                entry["name"].as_str()?,
                NoiseParameters::new(
                    i32::try_from(entry["firstOctave"].as_i64()?).expect("i32"),
                    amplitudes,
                ),
            );
        }
        Some(set)
    }

    /// A real chunk, generated onto a canvas: stone at the bottom, air at the
    /// top, and a palette that did not gain a second `minecraft:air`.
    #[test]
    fn a_filled_chunk_has_terrain_and_an_honest_palette() {
        let Some(noises) = noises() else { return };
        let seed = 20260725;
        let seeding = NoiseSeeding::new(seed);
        let router = OverworldRouter::new(seed, &noises, false);
        let settings = FillSettings::overworld();

        let mut pipeline = Pipeline::new(Area::square(0, 0, 0), -64, 384, "minecraft:air");
        pipeline.run_stage(&mut |canvas: &mut Canvas, x: i32, z: i32| {
            let _ = fill_chunk(
                canvas,
                &router,
                &seeding,
                settings,
                "minecraft:stone",
                x,
                z,
                None,
            );
        });

        let canvas = pipeline.canvas();
        assert_eq!(
            canvas.block_at(0, -64, 0),
            "minecraft:stone",
            "the world floor is solid"
        );
        assert_eq!(
            canvas.block_at(0, 319, 0),
            "minecraft:air",
            "and the sky is not"
        );

        // Air is skipped rather than written, so it appears once — as the
        // fill the canvas was created with.
        let (palette, _) = canvas.chunk_data(0, 0).expect("chunk");
        assert_eq!(
            palette.iter().filter(|p| *p == "minecraft:air").count(),
            1,
            "{palette:?}"
        );
        assert!(
            palette.iter().any(|p| p == "minecraft:stone"),
            "{palette:?}"
        );

        // The fossils are recorded, and they are a real surface rather than
        // the floor.
        let surface = canvas.world_surface_wg(8, 8);
        assert!(
            surface > -64 && surface < 320,
            "WORLD_SURFACE_WG is {surface}"
        );
        assert!(
            canvas.ocean_floor_wg(8, 8) <= surface,
            "the floor is not above the surface"
        );
    }

    /// Two runs of the same seed produce the same blocks. The determinism
    /// contract (ORACLE.md §6) starts here rather than at the region file.
    #[test]
    fn two_runs_of_one_seed_agree() {
        let Some(noises) = noises() else { return };
        let run = || {
            let seeding = NoiseSeeding::new(4242);
            let router = OverworldRouter::new(4242, &noises, false);
            let mut pipeline = Pipeline::new(Area::square(3, -2, 0), -64, 384, "minecraft:air");
            pipeline.run_stage(&mut |canvas: &mut Canvas, x: i32, z: i32| {
                let _ = fill_chunk(
                    canvas,
                    &router,
                    &seeding,
                    FillSettings::overworld(),
                    "minecraft:stone",
                    x,
                    z,
                    None,
                );
            });
            let (palette, ids) = pipeline.canvas().chunk_data(3, -2).expect("chunk");
            (palette.to_vec(), ids.to_vec())
        };
        assert_eq!(run(), run());
    }
}
