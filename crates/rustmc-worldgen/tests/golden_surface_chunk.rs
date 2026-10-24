//! **The surface parity gate.** Whole chunks, after surface rules.
//!
//! `golden_noise_chunk` proves the stage that decides stone, water and ore.
//! This proves the stage that decides everything you can see: grass, dirt,
//! sand, gravel, bedrock, the badlands stripes.
//!
//! Vectors come from `SurfaceSystem.buildSurface` run over a real
//! `ProtoChunk` (`ci/surface-chunk-golden.sh`), which is the same entry point
//! `NoiseBasedChunkGenerator` uses, minus the `StructureManager` it does not
//! need.
//!
//! # A retracted 100%
//!
//! This gate first read `100.000000% over 589,824 blocks`. **That number was
//! worth much less than it looked.** The harness built a second
//! `MappedRegistry<Biome>` for `PalettedContainerFactory` and resolved the
//! biome table from *it*, while the surface rule had been resolved against
//! `VanillaRegistries.createLookup()`. `BiomeConditionSource` tests
//! `holderSet.contains(context.getBiome())` and holder equality is identity,
//! so **every `biome` condition in the rule tree was false**. The dump still
//! looked like a world — grass and dirt come from the rule's
//! biome-independent default — it had just quietly lost desert sand,
//! badlands bands, frozen-ocean ice and the sulfur caves.
//!
//! It surfaced only because a chunk was added that contained a biome whose
//! rule places blocks *underground*, where nothing else competes: the port
//! wrote cinnabar and the oracle wrote deepslate. Every earlier chunk agreed
//! because both sides were being asked an easier question than intended.
//!
//! Two things follow, and both are now enforced below rather than
//! remembered. The gate checks the **biome grid** before it checks a single
//! block, so a biome fault reports as a biome fault. And it asserts that the
//! rule tree actually **reached** the conditions it claims to cover — a
//! condition that is never evaluated is not evidence, however green the
//! percentage next to it.
//!
//! # Reading a failure
//!
//! The report is a confusion matrix *plus* a y-band histogram, because
//! *which* blocks differ and *where they are* answer different questions.
//! The one true port bug this gate has caught needed both: the matrix said
//! "terrain, not surface materials" and the histogram said "spread through
//! bands no surface rule reaches", which together pointed at the harness's
//! own missing `swapSlices()`.

mod common;

use common::{
    biome_data, biome_table, long, noise_parameters, overworld_noises, plain_name, read,
    BiomeEntry, NameColumn, SEA_LEVEL,
};

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

use rustmc_worldgen::anvil_bridge::block_name;
use rustmc_worldgen::biome::climate::ParameterList;
use rustmc_worldgen::biome::manager::{obfuscate_seed, quart_for_block};
use rustmc_worldgen::biome::multi_noise::ClimateSampler;
use rustmc_worldgen::biome::temperature::TemperatureNoises;
use rustmc_worldgen::chunk_gen::{FillInputs, FillSettings, NoiseFill};
use rustmc_worldgen::density::aquifer::AquiferNoises;
use rustmc_worldgen::density::flat_cache::FlatCaches;
use rustmc_worldgen::density::ore_vein::OreVeinifier;
use rustmc_worldgen::density::router::OverworldRouter;
use rustmc_worldgen::density::surface_level::SurfaceLevels;
use rustmc_worldgen::noise::NoiseSeeding;
use rustmc_worldgen::surface::conditions::{min_surface_cell_corners, min_surface_level};
use rustmc_worldgen::surface::context::SurfaceContext;
use rustmc_worldgen::surface::rule::{parse_noise_settings_surface_rule, Rule};
use rustmc_worldgen::surface::system::build_column;
use rustmc_worldgen::surface::world::{ChunkSurfaceState, WorldSurfaceOracle};

/// How many of the overworld rule's 26 placeable blocks the chunk set must
/// exercise. A ratchet — raise it when the chunk set improves, never lower it
/// to make a run pass.
///
/// The one still missing is `red_sandstone`, and its guard chain explains
/// why: on top of a badlands biome it wants `stone_depth` **floor** 0 *and*
/// `stone_depth` **ceiling** 0 at the same block — the top and the bottom of
/// a stone run at once, which is a one-block-thick ledge with air or fluid
/// above and below it. Rare rather than unreachable.
const COVERAGE_FLOOR: usize = 26;

/// Where a named biome is, using the biome source alone.
///
/// `ci/surface-chunk-golden.sh --find` generates a whole chunk per candidate,
/// which is right for "does this chunk contain block X" but wasteful for
/// "where is biome Y" — and a `--find` for podzol ran fifty minutes without a
/// hit, which could equally have meant "no such biome nearby" or "the biome
/// is there and never produces the block". This separates the two questions.
///
/// A port-side scan is sound here for the same reason the temperature one is:
/// it answers a coverage question, and the biome source is proved at 100%
/// against vanilla anyway.
///
/// `RUSTMC_BIOME=old_growth cargo test -p rustmc-worldgen --release --test
/// golden_surface_chunk -- --ignored --nocapture locate_biome`
#[test]
#[ignore = "search, not an assertion — provenance for the chunk list"]
fn locate_biome() {
    let wanted = std::env::var("RUSTMC_BIOME").unwrap_or_else(|_| "old_growth".to_string());
    let params = noise_parameters();
    let noises = overworld_noises(&params);
    let table = biome_table();

    for seed in [0i64, 20_260_725] {
        let router = OverworldRouter::new(seed, &noises, false);
        let sampler = ClimateSampler::from_router(&router);
        let biomes = RefCell::new(ParameterList::new(table.clone()));
        let mut found = 0;
        // Outward in rings, so the chunk it names stays as close to spawn as
        // the biome allows.
        'outer: for radius in 0..192i32 {
            for chunk_x in -radius..=radius {
                for chunk_z in -radius..=radius {
                    if chunk_x.abs().max(chunk_z.abs()) != radius {
                        continue;
                    }
                    let id = biomes
                        .borrow_mut()
                        .find_value(sampler.sample(chunk_x * 4 + 2, 63 >> 2, chunk_z * 4 + 2))
                        .clone();
                    if id.contains(&wanted) {
                        println!("seed {seed}: ({chunk_x}, {chunk_z}) is {id}");
                        found += 1;
                        if found >= 3 {
                            break 'outer;
                        }
                    }
                }
            }
        }
        if found == 0 {
            println!("seed {seed}: no biome matching `{wanted}` within 192 chunks");
        }
    }
}

#[test]
#[ignore = "search, not an assertion — provenance for the chunk list"]
fn locate_frozen_ocean_chunks() {
    let params = noise_parameters();
    let noises = overworld_noises(&params);
    let table = biome_table();

    for seed in [0i64, 20_260_725] {
        let router = OverworldRouter::new(seed, &noises, false);
        let sampler = ClimateSampler::from_router(&router);
        let biomes = RefCell::new(ParameterList::new(table.clone()));
        let mut found = Vec::new();
        // Quart resolution at sea level, stepping a chunk at a time.
        'outer: for chunk_x in -64..64i32 {
            for chunk_z in -64..64i32 {
                let (qx, qy, qz) = (chunk_x * 4 + 2, 63 >> 2, chunk_z * 4 + 2);
                let id = biomes
                    .borrow_mut()
                    .find_value(sampler.sample(qx, qy, qz))
                    .clone();
                if id.contains("frozen_ocean") {
                    found.push((chunk_x, chunk_z, id));
                    if found.len() >= 4 {
                        break 'outer;
                    }
                }
            }
        }
        println!("seed {seed}: {found:?}");
    }
}

/// Which chunks reach the `temperature` condition at all.
///
/// The condition needs six guards to align — `above_preliminary_surface`,
/// `stone_depth` floor, `water`, a frozen-ocean biome, `hole`, and then
/// itself — so it is reached in a small minority of chunks and placed ice is
/// rarer still. A search through the *oracle* for a chunk containing
/// `minecraft:ice` ran for an hour without a hit.
///
/// Searching the port instead is sound here in a way it would not be for a
/// correctness question. This is looking for **coverage**: a chunk where the
/// rule tree reaches the condition. Whether the blocks in that chunk are
/// right is still decided by the oracle, independently, in the gate below —
/// so a port that reached the condition spuriously would be caught there
/// rather than credited here.
///
/// Ignored because it is a search whose output belongs in the harness'
/// chunk list. Run with `--ignored --nocapture`.
#[test]
#[ignore = "search, not an assertion — provenance for the chunk list"]
fn locate_chunks_that_reach_the_temperature_condition() {
    let rule: Rule = parse_noise_settings_surface_rule(&read(
        &common::data_path("worldgen/noise_settings/overworld.json").to_string_lossy(),
    ))
    .expect("parse rule");
    let params = noise_parameters();
    let noises = overworld_noises(&params);
    let table = biome_table();
    let biomes_data = biome_data();
    let temperature_noises = TemperatureNoises::new();

    for seed in [0i64, 20_260_725] {
        let router = OverworldRouter::new(seed, &noises, false);
        let seeding = NoiseSeeding::new(seed);
        let sampler = ClimateSampler::from_router(&router);
        let biomes = RefCell::new(ParameterList::new(table.clone()));
        let zoom = obfuscate_seed(seed);
        let mut found = 0;

        // Only frozen oceans can reach it, so the biome check comes first.
        'outer: for chunk_x in -48..48i32 {
            for chunk_z in -48..48i32 {
                let quart = sampler.sample(chunk_x * 4 + 2, 63 >> 2, chunk_z * 4 + 2);
                if !biomes
                    .borrow_mut()
                    .find_value(quart)
                    .contains("frozen_ocean")
                {
                    continue;
                }

                let filled = NoiseFill::new(
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
                .fill();

                let min_y = -64;
                let height = 384;
                let mut blocks: Vec<String> = Vec::with_capacity(height * 256);
                let mut heights = [min_y; 256];
                for y in min_y..min_y + height as i32 {
                    for z in 0..16 {
                        for x in 0..16 {
                            blocks.push(
                                block_name(filled.get(x, y, z), "minecraft:stone").to_string(),
                            );
                        }
                    }
                }
                for z in 0..16i32 {
                    for x in 0..16i32 {
                        heights[(z * 16 + x) as usize] = filled.world_surface(x, z);
                    }
                }

                let biome_at = |x: i32, y: i32, z: i32| {
                    let (qx, qy, qz) = quart_for_block(zoom, x, y, z);
                    biomes
                        .borrow_mut()
                        .find_value(sampler.sample(qx, qy, qz))
                        .clone()
                };
                let reached = Cell::new(0usize);
                let cold = |x: i32, y: i32, z: i32| {
                    reached.set(reached.get() + 1);
                    let id = biome_at(x, y, z);
                    let entry: &BiomeEntry = biomes_data.get(&id).expect("climate");
                    temperature_noises.cold_enough_to_snow(entry.climate, x, y, z, SEA_LEVEL)
                };
                let melt = |x: i32, y: i32, z: i32| {
                    let id = biome_at(x, y, z);
                    let entry: &BiomeEntry = biomes_data.get(&id).expect("climate");
                    temperature_noises.height_adjusted_temperature(
                        entry.climate,
                        x,
                        y,
                        z,
                        SEA_LEVEL,
                    ) > 0.1
                };

                let levels =
                    RefCell::new(SurfaceLevels::new(router.preliminary_surface_level.clone()));
                let flat = FlatCaches::none();
                let surface_at = |x: i32, z: i32| levels.borrow_mut().at(&flat, x, z);
                // The oracle answers with an interned id now. This test keeps
                // its rule un-interned and so takes the name-comparing
                // `Condition::Biome` arm, which resolves back through
                // `biome_name` — deliberately, so that arm stays exercised by
                // something rather than rotting into a path nothing runs.
                let palette = RefCell::new(rustmc_worldgen::surface::BiomePalette::new());
                let biome_id_at =
                    |x: i32, y: i32, z: i32| palette.borrow_mut().intern(&biome_at(x, y, z));
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
                        temperature: &cold,
                        melt_iceberg_slightly: &melt,
                    },
                );
                let mut column = NameColumn {
                    min_y,
                    height: height as i32,
                    blocks,
                    x: 0,
                    z: 0,
                };
                // Counted so a pass that runs and writes nothing is
                // distinguishable from one that never ran.
                let bergs = Cell::new(0usize);
                for local_z in 0..16i32 {
                    for local_x in 0..16i32 {
                        let bx = chunk_x * 16 + local_x;
                        let bz = chunk_z * 16 + local_z;
                        column.x = bx;
                        column.z = bz;
                        let depth = oracle.surface_depth_at(bx, bz);
                        let mut ctx = SurfaceContext::new(min_y, height as i32, SEA_LEVEL);
                        ctx.update_xz(bx, bz, depth);
                        let corners = min_surface_cell_corners(bx, bz);
                        let sampled = corners.map(|(cx, cz)| surface_at(cx, cz));
                        let blended = min_surface_level(sampled, bx, bz, depth);
                        let _ = ctx.min_surface_level(|_, _| blended);
                        let top = filled.world_surface(local_x, local_z);
                        build_column(
                            &rule,
                            &mut ctx,
                            &mut oracle,
                            &mut column,
                            top,
                            min_y,
                            &|s: &str| s == "minecraft:stone",
                        );
                        // `buildSurface` runs the frozen-ocean pass after the
                        // walk, on the same column and the same `top`. It
                        // writes into the water, which no rule can reach.
                        bergs.set(
                            bergs.get()
                                + oracle.frozen_ocean_extension(
                                    &mut column,
                                    bx,
                                    bz,
                                    top,
                                    blended,
                                    SEA_LEVEL,
                                ),
                        );
                    }
                }

                if reached.get() > 0 {
                    let ice = column
                        .blocks
                        .iter()
                        .filter(|b| plain_name(b) == "minecraft:ice")
                        .count();
                    println!(
                        "seed {seed}: chunk ({chunk_x}, {chunk_z}) reached temperature \
                         {} times, placed {ice} ice",
                        reached.get()
                    );
                    found += 1;
                    if found >= 3 {
                        break 'outer;
                    }
                }
            }
        }
        if found == 0 {
            println!(
                "seed {seed}: no chunk in +/-48 reached the temperature condition \
                 (note: seed 0 has no frozen ocean in that range at all, so its \
                 result is vacuous)"
            );
        }
    }
}

#[test]
fn surface_chunks_match_vanilla() {
    if !common::have_extract() {
        return;
    }
    let rule: Rule = parse_noise_settings_surface_rule(&read(
        &common::data_path("worldgen/noise_settings/overworld.json").to_string_lossy(),
    ))
    .expect("parse rule");
    let doc = read(&worldgen_assembly::surface_golden());
    let params = noise_parameters();
    let noises = overworld_noises(&params);
    let table = biome_table();

    let mut total = 0usize;
    let mut wrong = 0usize;
    let mut confusion: BTreeMap<(String, String), usize> = BTreeMap::new();
    // Which of the rule's output blocks these chunks actually exercise.
    // A parity percentage says how often the port agrees on the blocks it was
    // asked about; this says how much of the rule it was asked about at all.
    let mut produced: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    let mut examples: Vec<String> = Vec::new();
    // Where the differences land is the diagnostic. A fault in the rules
    // clusters near the surface; a fault in the walk's bookkeeping spreads
    // down a column; a fault in the terrain underneath is everywhere.
    let mut by_band: BTreeMap<i32, usize> = BTreeMap::new();
    // Coverage, not correctness: how often the rule tree reached the
    // `temperature` condition, and how often it said yes.
    let temperature_evaluations = Cell::new(0usize);
    let cold_answers = Cell::new(0usize);
    // How many blocks the frozen-ocean pass wrote. A pass that runs and
    // writes nothing is indistinguishable from one that never ran.
    let mut berg_blocks = 0usize;
    let mut biome_quarts_checked = 0usize;
    let mut depth_checked = 0usize;
    let mut depth_wrong = 0usize;
    let mut depth_examples: Vec<String> = Vec::new();
    let mut preliminary_checked = 0usize;
    let mut preliminary_wrong = 0usize;
    let mut preliminary_examples: Vec<String> = Vec::new();

    for case in doc["cases"].as_array().expect("cases") {
        let seed = long(&case["seed"]);
        let chunk_x = case["chunkX"].as_i64().expect("chunkX") as i32;
        let chunk_z = case["chunkZ"].as_i64().expect("chunkZ") as i32;
        let min_y = case["minY"].as_i64().expect("minY") as i32;
        let height = case["height"].as_i64().expect("height") as i32;

        // ---- the noise stage, already proved at 100% ----
        let router = OverworldRouter::new(seed, &noises, false);
        let seeding = NoiseSeeding::new(seed);
        let filled = NoiseFill::new(
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
        .fill();

        let mut blocks: Vec<String> = Vec::with_capacity((height * 256) as usize);
        let mut heights = [min_y; 256];
        for y in min_y..min_y + height {
            for z in 0..16 {
                for x in 0..16 {
                    blocks.push(block_name(filled.get(x, y, z), "minecraft:stone").to_string());
                }
            }
        }
        for z in 0..16i32 {
            for x in 0..16i32 {
                heights[(z * 16 + x) as usize] = filled.world_surface(x, z);
            }
        }

        // ---- the biome source, through BiomeManager's fuzz ----
        let sampler = ClimateSampler::from_router(&router);
        let biomes = RefCell::new(ParameterList::new(table.clone()));
        let zoom = obfuscate_seed(seed);
        let biome_at = |x: i32, y: i32, z: i32| {
            let (qx, qy, qz) = quart_for_block(zoom, x, y, z);
            biomes
                .borrow_mut()
                .find_value(sampler.sample(qx, qy, qz))
                .clone()
        };

        // ---- the biome grid, before any rule reads it ----
        //
        // Checked first and separately because a wrong biome and a wrong rule
        // produce the same symptom. These are raw quart lookups, so a failure
        // here is the climate sampler or the biome table; `golden_biome_zoom`
        // covers the fuzz that sits on top.
        {
            let palette: Vec<&str> = case["biomePalette"]
                .as_array()
                .expect("biomePalette")
                .iter()
                .map(|v| v.as_str().expect("biome name"))
                .collect();
            let quarts = case["biomeQuarts"].as_array().expect("biomeQuarts");
            let quart_min_y = min_y >> 2;
            let mut i = 0usize;
            for qy in quart_min_y..quart_min_y + (height >> 2) {
                for qz in 0..4i32 {
                    for qx in 0..4i32 {
                        let want = palette[quarts[i].as_u64().expect("palette index") as usize];
                        let got = biomes
                            .borrow_mut()
                            .find_value(sampler.sample(chunk_x * 4 + qx, qy, chunk_z * 4 + qz))
                            .clone();
                        assert_eq!(
                            got,
                            want,
                            "seed={seed} chunk=({chunk_x}, {chunk_z}) \
                             quart=({}, {qy}, {}): biome differs",
                            chunk_x * 4 + qx,
                            chunk_z * 4 + qz
                        );
                        i += 1;
                    }
                }
            }
            biome_quarts_checked += i;
        }

        // ---- the temperature condition ----
        let biomes_data = biome_data();
        let temperature_noises = TemperatureNoises::new();
        // `shouldMeltFrozenOceanIcebergSlightly` — the same height-adjusted
        // temperature as `cold_enough_to_snow`, a different threshold. Not
        // counted: it is asked once per frozen-ocean column rather than from
        // inside the rule tree, so it says nothing about rule coverage.
        let melt_iceberg = |x: i32, y: i32, z: i32| {
            let id = biome_at(x, y, z);
            let entry: &BiomeEntry = biomes_data
                .get(&id)
                .unwrap_or_else(|| panic!("{id} has no climate settings in the generated data"));
            temperature_noises.height_adjusted_temperature(entry.climate, x, y, z, SEA_LEVEL) > 0.1
        };
        let cold_enough_to_snow = |x: i32, y: i32, z: i32| {
            let id = biome_at(x, y, z);
            let entry: &BiomeEntry = biomes_data
                .get(&id)
                .unwrap_or_else(|| panic!("{id} has no climate settings in the generated data"));
            let cold = temperature_noises.cold_enough_to_snow(entry.climate, x, y, z, SEA_LEVEL);
            // Counted so the gate can say whether it reached this condition
            // at all. A rule tree where `temperature` is wired to the wrong
            // branch — or to none — still produces a green chunk comparison
            // for terrain that never needed it.
            temperature_evaluations.set(temperature_evaluations.get() + 1);
            if cold {
                cold_answers.set(cold_answers.get() + 1);
            }
            cold
        };

        // ---- the preliminary surface, for min_surface_level ----
        let levels = RefCell::new(SurfaceLevels::new(router.preliminary_surface_level.clone()));

        // `preliminarySurfaceLevel`, compared directly.
        //
        // It is the input to `above_preliminary_surface` and it writes no
        // block of its own, so every other number in these vectors can be
        // exact while this is wrong — a later rule simply takes a different
        // branch and ends at a block that happens to match. That blindness
        // cost two investigations most of their length; see
        // swarm/tasks/w9-gate-the-density-queries.md.
        if let Some(want) = case.get("preliminarySurface").and_then(|v| v.as_array()) {
            let flat_cache = FlatCaches::none();
            for (i, value) in want.iter().enumerate() {
                let want = i32::try_from(value.as_i64().expect("height")).expect("i32");
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_possible_wrap,
                    reason = "an index bounded by 256"
                )]
                let flat_index = i as i32;
                let bx = chunk_x * 16 + flat_index % 16;
                let bz = chunk_z * 16 + flat_index / 16;
                let got = levels.borrow_mut().at(&flat_cache, bx, bz);
                preliminary_checked += 1;
                if got != want {
                    preliminary_wrong += 1;
                    if preliminary_examples.len() < 6 {
                        preliminary_examples.push(format!(
                            "seed={seed} chunk=({chunk_x}, {chunk_z}) column=({bx}, {bz}): \
                             want {want}, got {got}"
                        ));
                    }
                }
            }
        }
        let flat = FlatCaches::none();
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
                // Filled in per column below; this closure is only reached
                // via `above_preliminary_surface`, which needs the blend.
                min_surface_level: &|_, _| 0,
                // The condition reads the biome through the *same* fuzzed
                // lookup the rest of the rule does, not the raw quart grid —
                // `SurfaceRules.Context` memoises one biome per position and
                // both the `biome` condition and this one see it.
                temperature: &cold_enough_to_snow,
                melt_iceberg_slightly: &melt_iceberg,
            },
        );

        // `getSurfaceDepth` — the other context quantity behind packed_ice,
        // and the fourth value in this file that places no block of its own.
        if let Some(want) = case.get("surfaceDepth").and_then(|v| v.as_array()) {
            for (i, value) in want.iter().enumerate() {
                let want = i32::try_from(value.as_i64().expect("depth")).expect("i32");
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_possible_wrap,
                    reason = "an index bounded by 256"
                )]
                let flat_index = i as i32;
                let bx = chunk_x * 16 + flat_index % 16;
                let bz = chunk_z * 16 + flat_index / 16;
                let got = oracle.surface_depth_at(bx, bz);
                depth_checked += 1;
                if got != want {
                    depth_wrong += 1;
                    if depth_examples.len() < 6 {
                        depth_examples.push(format!(
                            "seed={seed} chunk=({chunk_x}, {chunk_z}) column=({bx}, {bz}): \
                             want {want}, got {got}"
                        ));
                    }
                }
            }
        }

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
                // The evaluator's `RUSTMC_TRACE_BLOCK` filters on position
                // only, and the same chunk is generated at every dumped seed.
                // Announce the seed so a trace can be attributed.
                let mut ctx = SurfaceContext::new(min_y, height, SEA_LEVEL);
                ctx.update_xz(block_x, block_z, depth);
                // Prime the blended minimum before the walk, since the
                // oracle's own closure cannot see this column's depth.
                let corners = min_surface_cell_corners(block_x, block_z);
                let sampled = corners.map(|(cx, cz)| surface_at(cx, cz));
                let blended = min_surface_level(sampled, block_x, block_z, depth);
                if std::env::var("RUSTMC_TRACE_BLOCK").is_ok() {
                    println!(
                        "[seed {seed} column ({block_x}, {block_z}) depth {depth} \
                         sampled {sampled:?} blended {blended}]"
                    );
                }
                if std::env::var("RUSTMC_TRACE_BLOCK").is_ok() {
                    println!(
                        "[seed {seed} column ({block_x}, {block_z}) depth {depth} \
                         sampled {sampled:?} blended {blended}]"
                    );
                }
                let _ = ctx.min_surface_level(|_, _| blended);

                let top = filled.world_surface(local_x, local_z);
                if std::env::var("RUSTMC_TRACE_BLOCK").is_ok() {
                    println!(
                        "[seed {seed} column ({block_x}, {block_z}) top {top} bottom {min_y}]"
                    );
                }
                build_column(
                    &rule,
                    &mut ctx,
                    &mut oracle,
                    &mut column_state,
                    top,
                    min_y,
                    &|s: &str| s == "minecraft:stone",
                );
                // `buildSurface` runs the frozen-ocean pass after the walk,
                // on the same column and the same `top`. It writes into the
                // water column, which the walk never offers to a rule.
                berg_blocks += oracle.frozen_ocean_extension(
                    &mut column_state,
                    block_x,
                    block_z,
                    top,
                    blended,
                    SEA_LEVEL,
                );
            }
        }

        // ---- compare ----
        let palette: Vec<&str> = case["palette"]
            .as_array()
            .expect("palette")
            .iter()
            .map(|v| v.as_str().expect("name"))
            .collect();
        let mut expected: Vec<usize> = Vec::with_capacity((height * 256) as usize);
        for run in case["runs"].as_array().expect("runs") {
            let pair = run.as_array().expect("[index, length]");
            let index = pair[0].as_u64().expect("index") as usize;
            let length = pair[1].as_u64().expect("length") as usize;
            expected.extend(std::iter::repeat_n(index, length));
        }

        for (i, want_index) in expected.iter().enumerate() {
            // The **full state**, `minecraft:grass_block[snowy=false]`, not
            // the block name. The dumper recorded names for its first three
            // days and this comparison stripped properties to match, which
            // meant a port placing the right block in the wrong state would
            // have read 100%. Six of the thirty-five states these chunks
            // contain carry properties, `deepslate[axis=y]` among them.
            let want = palette[*want_index];
            let got = column_state.blocks[i].as_str();
            produced.insert(want);
            total += 1;
            if got != want {
                wrong += 1;
                *confusion
                    .entry((want.to_string(), got.to_string()))
                    .or_insert(0) += 1;
                let band = (min_y + (i as i32) / 256).div_euclid(32) * 32;
                *by_band.entry(band).or_insert(0) += 1;
                if examples.len() < 6 {
                    let y = min_y + (i as i32) / 256;
                    let rem = (i as i32) % 256;
                    examples.push(format!(
                        "seed={seed} chunk=({chunk_x}, {chunk_z}) block=({}, {y}, {}): want {want}, got {got}",
                        chunk_x * 16 + rem % 16,
                        chunk_z * 16 + rem / 16
                    ));
                }
            }
        }
    }

    assert!(total > 0, "no blocks were compared");
    let exactness = 100.0 * (total - wrong) as f64 / total as f64;
    println!("SURFACE CHUNK EXACTNESS {exactness:.6}% over {total} blocks ({wrong} wrong)");
    println!("  biome grid matched at {biome_quarts_checked} quart cells");
    println!("  preliminary surface: {preliminary_wrong}/{preliminary_checked} wrong");
    for line in &preliminary_examples {
        println!("    {line}");
    }
    println!("  surface depth: {depth_wrong}/{depth_checked} wrong");
    for line in &depth_examples {
        println!("    {line}");
    }

    // Coverage of the rule's output vocabulary, reported and ratcheted.
    //
    // The number this replaced was 10 of 26, measured after the oracle was
    // fixed — three chunks near spawn will never contain a badlands, a
    // mangrove swamp or a mushroom island, so a hand-picked chunk list
    // measures whatever those chunks happen to be. The chunk set is now
    // chosen by `ci/surface-chunk-golden.sh --survey`, which walks outward
    // from the origin and names a chunk whenever it produces a block no
    // earlier one did.
    //
    // The floor is a ratchet, not a target: it fails if coverage drops, and
    // is raised by hand when the chunk set improves. Asserting 26/26 would be
    // asserting that every block is reachable in some finite chunk set, which
    // has not been shown.
    // Coverage is compared on **plain names**, unlike the block comparison
    // above which is now full-state.
    //
    // Not laziness: the two sides spell properties differently. The rule
    // declares them as the datapack writes them, which `parse_block_state`
    // sorts alphabetically; the palette records
    // `BlockStateParser.serialize`, which emits them in the block's own
    // property-definition order. Those agree for a one-property block and
    // need not for others, so comparing spellings here would measure the
    // spelling. What this number is *about* is which blocks the rule can
    // reach, and that question is name-shaped.
    let (placeable_states, has_bandlands) = rule.placeable_states();
    let placeable: std::collections::BTreeSet<&str> =
        placeable_states.iter().map(|s| plain_name(s)).collect();
    let produced_names: std::collections::BTreeSet<&str> =
        produced.iter().map(|s| plain_name(s)).collect();
    let reached: Vec<&&str> = placeable
        .iter()
        .filter(|b| produced_names.contains(**b))
        .collect();
    let missing: Vec<&&str> = placeable
        .iter()
        .filter(|b| !produced_names.contains(**b))
        .collect();
    println!(
        "  rule output coverage: {}/{} placeable blocks{}",
        reached.len(),
        placeable.len(),
        if has_bandlands { " (+ bandlands)" } else { "" }
    );
    if !missing.is_empty() {
        println!("  not reached: {missing:?}");
    }
    assert!(
        reached.len() >= COVERAGE_FLOOR,
        "rule output coverage fell to {}/{} (floor {COVERAGE_FLOOR}); not reached: {missing:?}",
        reached.len(),
        placeable.len()
    );
    println!(
        "  temperature condition reached {} times, said cold {} times",
        temperature_evaluations.get(),
        cold_answers.get()
    );
    println!("  frozen-ocean pass wrote {berg_blocks} blocks");
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
         By y band:\n{bands}\nConfusion (expected -> got):\n{report}\nExamples:\n  {}",
        examples.join("\n  ")
    );
    // Last, so a real parity failure reports its diagnostics rather than
    // being pre-empted by a coverage complaint.
    //
    // A **pinned measurement**, not a pass. The `temperature` condition is
    // reached zero times by this chunk set, and that is a fact worth failing
    // on if it changes — either the chunk list moved or something upstream
    // started reaching a branch it did not. What it must not become is an
    // unasserted zero that nobody notices.
    //
    // Why zero: the condition sits behind five other guards, the last of
    // which is `hole` — a column whose surface depth is not positive. Two
    // searches went looking. `ci/surface-chunk-golden.sh --find` scanned
    // 36,864 chunks of the oracle for placed ice for an hour without a hit;
    // `locate_chunks_that_reach_the_temperature_condition` scanned every
    // frozen-ocean chunk within +/-48 of two seeds through the port and found
    // none reaching the condition either. It is rarer than "frozen ocean" by
    // a long way.
    //
    // The condition's *value* is not unproven for that reason:
    // `golden_temperature` covers it directly at 31,548 lookups across all 66
    // biomes, at 100%. What stays uncovered is the wiring between that value
    // and this rule tree, and saying so is better than a gate that fails
    // forever or one that quietly says nothing.
    const TEMPERATURE_REACHES: usize = 0;
    assert_eq!(
        temperature_evaluations.get(),
        TEMPERATURE_REACHES,
        "the number of times the rule tree reached the `temperature` condition \
         changed. If it went up, the chunk set now covers it — raise the \
         constant and delete the note above. If it went down from a nonzero \
         value, something stopped reaching a branch it used to."
    );
}
