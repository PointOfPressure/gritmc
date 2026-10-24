//! **The carver parity gate.** Whole chunks, after caves and ravines.
//!
//! Vectors from `ci/surface-chunk-golden.sh`, which dumps each chunk twice —
//! once after the surface pass and once after the carvers — from a single
//! generation. That sharing is deliberate: it means a difference here cannot
//! be a terrain or surface difference, because `golden_surface_chunk` reads
//! the *first* dump of the very same chunks and is at 100%.
//!
//! # What this stage is
//!
//! Carving one chunk replays the carvers of every chunk within ±8 — 289
//! source chunks, each contributing whatever its own biome lists, seeded from
//! its own coordinates. So this gate exercises about 870 reseeds and several
//! hundred tunnel walks per chunk, and a single wrong draw anywhere in that
//! moves every cave after it.
//!
//! # The oracle had to be repaired twice to produce these
//!
//! Block tags are datapack content and `Bootstrap.bootStrap()` loads none, so
//! `#minecraft:overworld_carver_replaceables` resolved to an empty holder set
//! and `canReplaceBlock` was false everywhere: 567 carvers ran to completion
//! and changed nothing, with no error. The vectors carry `carversSeen` and
//! `carversStarted` so that failure mode is visible in a diff rather than
//! needing to be rediscovered.

mod common;

use common::{
    biome_data, biome_table, configured_carvers, expand_runs, long, noise_parameters,
    overworld_noises, palette_of, read, BiomeEntry, NameColumn, DATA, SEA_LEVEL,
};
use std::collections::BTreeMap;

use rustmc_worldgen::anvil_bridge::block_name;
use rustmc_worldgen::biome::climate::ParameterList;
use rustmc_worldgen::biome::manager::{obfuscate_seed, quart_for_block};
use rustmc_worldgen::biome::multi_noise::ClimateSampler;
use rustmc_worldgen::biome::temperature::TemperatureNoises;
use rustmc_worldgen::carver::carve::{CarveBounds, CarveTarget};
use rustmc_worldgen::carver::driver::apply_carvers;
use rustmc_worldgen::carver::mask::CarvingMask;
use rustmc_worldgen::chunk_gen::{FillInputs, FillSettings, NoiseFill, PostFill};
use rustmc_worldgen::density::aquifer::AquiferNoises;
use rustmc_worldgen::density::ore_vein::OreVeinifier;
use rustmc_worldgen::density::router::OverworldRouter;
use rustmc_worldgen::density::surface_level::SurfaceLevels;
use rustmc_worldgen::noise::NoiseSeeding;
use rustmc_worldgen::surface::conditions::{min_surface_cell_corners, min_surface_level};
use rustmc_worldgen::surface::context::SurfaceContext;
use rustmc_worldgen::surface::rule::{parse_noise_settings_surface_rule, Rule};
use rustmc_worldgen::surface::system::build_column;
use rustmc_worldgen::surface::world::{ChunkSurfaceState, WorldSurfaceOracle};
use rustmc_worldgen::tags::BlockTags;
use std::cell::RefCell;

/// The chunk a carver writes into: named blocks plus the surviving aquifer.
struct CarveChunk {
    min_y: i32,
    height: i32,
    chunk_x: i32,
    chunk_z: i32,
    blocks: Vec<String>,
    post: PostFill,
    /// Counted rather than implemented — see the assertion at the end.
    top_material_calls: usize,
    /// A mark writes no block, so this gate can only count them.
    marks: usize,
}

impl CarveChunk {
    fn index(&self, x: i32, y: i32, z: i32) -> Option<usize> {
        if y < self.min_y || y >= self.min_y + self.height {
            return None;
        }
        // Only this chunk exists; a carver reading outside it is reading a
        // neighbour vanilla has loaded and we do not.
        if x >> 4 != self.chunk_x || z >> 4 != self.chunk_z {
            return None;
        }
        Some((((y - self.min_y) * 256) + (z & 15) * 16 + (x & 15)) as usize)
    }
}

impl CarveTarget for CarveChunk {
    fn block(&self, x: i32, y: i32, z: i32) -> String {
        self.index(x, y, z)
            .map_or_else(|| "minecraft:air".to_string(), |i| self.blocks[i].clone())
    }

    fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str) {
        if let Some(i) = self.index(x, y, z) {
            self.blocks[i] = name.to_string();
        }
    }

    fn aquifer_substance(&mut self, x: i32, y: i32, z: i32) -> Option<String> {
        // `computeSubstance(pos, 0.0)`: density zero, so the aquifer takes
        // its fluid path rather than returning "solid".
        let PostFill {
            aquifer,
            flat,
            surface,
            ..
        } = &mut self.post;
        let aquifer = aquifer.as_mut()?;
        aquifer
            .compute_substance(flat, surface, (x, y, z), 0.0)
            .map(|s| block_name(s, "minecraft:stone").to_string())
    }

    fn top_material(&mut self, _: i32, _: i32, _: i32, _: bool) -> Option<String> {
        self.top_material_calls += 1;
        None
    }

    fn should_schedule_fluid_update(&self) -> bool {
        self.post.aquifer.as_ref().is_some_and(
            rustmc_worldgen::density::aquifer::NoiseBasedAquifer::should_schedule_fluid_update,
        )
    }

    // This gate compares block bytes against vanilla's carved chunk, and a
    // mark writes no block, so it is counted rather than applied.
    fn mark_post_processing(&mut self, _: i32, _: i32, _: i32) {
        self.marks += 1;
    }
}

#[test]
fn carved_chunks_match_vanilla() {
    let rule_extract = std::path::Path::new(DATA).join("worldgen/noise_settings/overworld.json");
    if !rustmc_data::fixture::available(&rule_extract, "rule extract", "ci/vanilla-server.sh data")
    {
        return;
    }
    let rule: Rule =
        parse_noise_settings_surface_rule(&read(rule_extract.to_str().expect("utf-8 path")))
            .expect("parse rule");

    let doc = read(&worldgen_assembly::surface_golden());
    assert!(
        doc["carversStarted"].as_u64().expect("carversStarted") > 0,
        "the vectors were produced by a sweep in which no carver started — \
         the oracle's block tags are probably unbound"
    );

    let params = noise_parameters();
    let noises = overworld_noises(&params);
    let table = biome_table();
    let biomes_data = biome_data();
    let carvers = configured_carvers();
    let tags = BlockTags::load(&std::path::Path::new(DATA).join("tags/block")).expect("block tags");
    let temperature_noises = TemperatureNoises::new();

    let mut total = 0usize;
    let mut wrong = 0usize;
    let mut confusion: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut by_band: BTreeMap<i32, usize> = BTreeMap::new();
    let mut examples: Vec<String> = Vec::new();
    let mut carved_by_vanilla = 0usize;
    let mut top_material_calls = 0usize;

    for case in doc["cases"].as_array().expect("cases") {
        let seed = long(&case["seed"]);
        let chunk_x = case["chunkX"].as_i64().expect("chunkX") as i32;
        let chunk_z = case["chunkZ"].as_i64().expect("chunkZ") as i32;
        let min_y = case["minY"].as_i64().expect("minY") as i32;
        let height = case["height"].as_i64().expect("height") as i32;

        // ---- noise stage ----
        let router = OverworldRouter::new(seed, &noises, false);
        let seeding = NoiseSeeding::new(seed);
        let post = NoiseFill::new(
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
                ore_veinifier: Some(OreVeinifier::new(
                    router.vein_toggle.clone(),
                    router.vein_ridged.clone(),
                    router.vein_gap.clone(),
                    seeding.source_for("minecraft:ore").fork_positional(),
                )),
                // No beard: this fixture predates structures entirely.
                beardifier: None,
            },
            FillSettings::overworld(),
            chunk_x,
            chunk_z,
        )
        .fill_retaining();

        let mut blocks: Vec<String> = Vec::with_capacity((height * 256) as usize);
        let mut heights = [min_y; 256];
        for y in min_y..min_y + height {
            for z in 0..16 {
                for x in 0..16 {
                    blocks.push(block_name(post.chunk.get(x, y, z), "minecraft:stone").to_string());
                }
            }
        }
        for z in 0..16i32 {
            for x in 0..16i32 {
                heights[(z * 16 + x) as usize] = post.chunk.world_surface(x, z);
            }
        }

        // ---- biomes ----
        let sampler = ClimateSampler::from_router(&router);
        let biome_list = RefCell::new(ParameterList::new(table.clone()));
        let zoom = obfuscate_seed(seed);
        let biome_at = |x: i32, y: i32, z: i32| {
            let (qx, qy, qz) = quart_for_block(zoom, x, y, z);
            biome_list
                .borrow_mut()
                .find_value(sampler.sample(qx, qy, qz))
                .clone()
        };
        let cold_enough_to_snow = |x: i32, y: i32, z: i32| {
            let id = biome_at(x, y, z);
            let entry: &BiomeEntry = biomes_data
                .get(&id)
                .unwrap_or_else(|| panic!("{id} has no biome data"));
            temperature_noises.cold_enough_to_snow(entry.climate, x, y, z, SEA_LEVEL)
        };
        // `shouldMeltFrozenOceanIcebergSlightly` — a different threshold on
        // the same height-adjusted temperature, for the frozen-ocean pass.
        let melt_iceberg = |x: i32, y: i32, z: i32| {
            let id = biome_at(x, y, z);
            let entry: &BiomeEntry = biomes_data
                .get(&id)
                .unwrap_or_else(|| panic!("{id} has no biome data"));
            temperature_noises.height_adjusted_temperature(entry.climate, x, y, z, SEA_LEVEL) > 0.1
        };

        // ---- surface stage ----
        let levels = RefCell::new(SurfaceLevels::new(router.preliminary_surface_level.clone()));
        let flat = rustmc_worldgen::density::flat_cache::FlatCaches::none();
        let surface_at = |x: i32, z: i32| levels.borrow_mut().at(&flat, x, z);

        let palette = RefCell::new(rustmc_worldgen::surface::BiomePalette::new());
        let biome_id_at = |x: i32, y: i32, z: i32| palette.borrow_mut().intern(&biome_at(x, y, z));
        let biome_name =
            |id: rustmc_worldgen::surface::BiomeId| palette.borrow().name(id).to_string();
        let mut oracle = WorldSurfaceOracle::new(
            seed,
            &params,
            ChunkSurfaceState {
                heights: &heights,
                biome: &biome_id_at,
                biome_name: &biome_name,
                min_surface_level: &|_, _| 0,
                temperature: &cold_enough_to_snow,
                melt_iceberg_slightly: &melt_iceberg,
            },
        );
        let mut column_state = NameColumn {
            min_y,
            height,
            blocks,
            x: 0,
            z: 0,
        };
        for local_z in 0..16i32 {
            for local_x in 0..16i32 {
                let block_x = chunk_x * 16 + local_x;
                let block_z = chunk_z * 16 + local_z;
                column_state.x = block_x;
                column_state.z = block_z;

                let depth = oracle.surface_depth_at(block_x, block_z);
                let mut ctx = SurfaceContext::new(min_y, height, SEA_LEVEL);
                ctx.update_xz(block_x, block_z, depth);
                // Prime the blended minimum before the walk, since the
                // oracle's own closure cannot see this column's depth.
                let corners = min_surface_cell_corners(block_x, block_z);
                let sampled = corners.map(|(cx, cz)| surface_at(cx, cz));
                let blended = min_surface_level(sampled, block_x, block_z, depth);
                let _ = ctx.min_surface_level(|_, _| blended);

                let top = heights[(local_z * 16 + local_x) as usize];
                build_column(
                    &rule,
                    &mut ctx,
                    &mut oracle,
                    &mut column_state,
                    top,
                    min_y,
                    &|s: &str| s == "minecraft:stone",
                );
                // This gate builds a real surface before carving, so it runs
                // the frozen-ocean pass for the same reason `buildSurface`
                // does — the icebergs are there to be carved through.
                oracle.frozen_ocean_extension(
                    &mut column_state,
                    block_x,
                    block_z,
                    top,
                    blended,
                    SEA_LEVEL,
                );
            }
        }

        // ---- carvers ----
        let mut chunk = CarveChunk {
            min_y,
            height,
            chunk_x,
            chunk_z,
            blocks: column_state.blocks,
            post,
            top_material_calls: 0,
            marks: 0,
        };
        let mut mask = CarvingMask::new(height, min_y);
        apply_carvers(
            &mut chunk,
            &tags,
            &|source_x, source_z| {
                // `QuartPos.fromBlock(sourceMinBlockX)` with quart y 0.
                let id = biome_list
                    .borrow_mut()
                    .find_value(sampler.sample(source_x * 4, 0, source_z * 4))
                    .clone();
                let entry: &BiomeEntry = biomes_data
                    .get(&id)
                    .unwrap_or_else(|| panic!("{id} has no biome data"));
                entry
                    .carvers
                    .iter()
                    .map(|name| {
                        carvers
                            .get(name)
                            .unwrap_or_else(|| panic!("no configured carver {name}"))
                            .clone()
                    })
                    .collect()
            },
            seed,
            CarveBounds {
                min_gen_y: min_y,
                gen_depth: height,
                chunk_x,
                chunk_z,
            },
            &mut mask,
        );
        top_material_calls += chunk.top_material_calls;

        // ---- compare ----
        let palette = palette_of(case, "carvedPalette");
        let surface_palette = palette_of(case, "palette");
        let want_carved = expand_runs(palette.len(), &case["carvedRuns"]);
        let want_surface = expand_runs(surface_palette.len(), &case["runs"]);

        for (i, want_index) in want_carved.iter().enumerate() {
            // Full state, not block name — see the same note in
            // `golden_surface_chunk`. A carver that left `water[level=3]`
            // where vanilla left `water[level=0]` would have read 100%.
            let want = palette[*want_index];
            let got = chunk.blocks[i].as_str();
            total += 1;
            if surface_palette[want_surface[i]] != want {
                carved_by_vanilla += 1;
            }
            if got != want {
                wrong += 1;
                *confusion
                    .entry((want.to_string(), got.to_string()))
                    .or_insert(0) += 1;
                let y = min_y + (i / 256) as i32;
                *by_band.entry((y.div_euclid(32)) * 32).or_insert(0) += 1;
                if examples.len() < 6 {
                    examples.push(format!(
                        "seed={seed} chunk=({chunk_x}, {chunk_z}) block=({}, {y}, {}): want {want}, got {got}",
                        chunk_x * 16 + (i % 256 % 16) as i32,
                        chunk_z * 16 + (i % 256 / 16) as i32
                    ));
                }
            }
        }
    }

    assert!(total > 0, "no blocks were compared");
    #[allow(clippy::cast_precision_loss)]
    let exactness = 100.0 * (total - wrong) as f64 / total as f64;
    println!("CARVER CHUNK EXACTNESS {exactness:.6}% over {total} blocks ({wrong} wrong)");
    println!("  vanilla carved {carved_by_vanilla} of them");
    println!("  topMaterial was asked for {top_material_calls} times");

    let bands = by_band
        .iter()
        .map(|(band, n)| format!("  y {band}..{}: {n}", band + 31))
        .collect::<Vec<_>>()
        .join("\n");
    let report = confusion
        .iter()
        .map(|((want, got), n)| format!("  {want} -> {got}: {n}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        wrong,
        0,
        "{wrong} of {total} blocks differ ({exactness:.6}% exact)\n\
         vanilla carved {carved_by_vanilla}\nBy y band:\n{bands}\n\
         Confusion (expected -> got):\n{report}\nExamples:\n  {}",
        examples.join("\n  ")
    );

    // Coverage, not correctness: a gate where vanilla carved nothing would
    // be comparing two untouched chunks and reporting 100%.
    assert!(
        carved_by_vanilla > 1000,
        "vanilla only carved {carved_by_vanilla} blocks — this gate is not exercising carvers"
    );
    // Coverage of the re-grassing trigger, and it is a coverage assertion in
    // the other direction now. It read `assert_eq!(top_material_calls, 0)`
    // until 2026-08-09 with a comment saying topMaterial was not implemented
    // — and it was green for the wrong reason: `carve_block` armed `hasGrass`
    // by comparing a *state* string to a *block* name, so the call site was
    // unreachable and this gate was measuring a dead arm rather than a chunk
    // with no cave mouths in it. Fixing the arm took this chunk from 0 asks
    // to 9 and moved the residual by nothing, which is the measurement.
    //
    // WHAT THIS GATE STILL DOES NOT COVER, and it must be said rather than
    // implied by a green: the double above answers `None`, so this exercises
    // the *trigger* — hasGrass, the DIRT test, the offset — and never the
    // *material* the surface rule picks. The material is adjudicated on the
    // r=16 window instead (docs/WORKLIST.md, `grass_block -> dirt` 646 -> 0).
    assert!(
        top_material_calls > 0,
        "the carvers never asked for topMaterial in this chunk — the trigger is unreachable again"
    );
}
