//! **The parity gate.** Whole chunks of blocks against real vanilla 26.2.
//!
//! Every other golden test in this crate proves one layer. This one proves
//! the stack: world seed -> noise -> router -> cell interpolation -> aquifer
//! -> ore veins -> block. A number here is the project's actual claim, and it
//! is a block count, not a density comparison.
//!
//! The vectors come from `NoiseBasedChunkGenerator.getBaseColumn`, which runs
//! vanilla's `iterateNoiseColumn`: a real `NoiseChunk`, the same
//! `getInterpolatedState`, the same MaterialRuleList, and the same
//! `defaultBlock()` substitution for null that `doFill` performs. So it is
//! the fill's own output, obtained without a live level.
//!
//! # Scope, stated honestly
//!
//! This is vanilla's **noise stage** — the last stage that does not depend on
//! the biome source. No surfaces, carvers, features or structures, so no
//! grass, no trees, no ores placed by features (the ore *veins* here are the
//! noise-stage ones, which are a different mechanism). A 100% here does not
//! mean a chunk on disk matches vanilla; it means the terrain, the water and
//! the veins underneath do.
//!
//! One of the two chunks sampled is 1000 chunks from the origin. That is not
//! padding: the upstream `Mth.getSeed` bug this project found in another
//! implementation was invisible within ~686 blocks of spawn, so an oracle
//! that only looks near the origin cannot see a whole class of coordinate
//! fault.

use rustmc_worldgen::chunk_gen::{FillInputs, FillSettings, NoiseFill};
use rustmc_worldgen::density::aquifer::AquiferNoises;
use rustmc_worldgen::density::fill::Substance;
use rustmc_worldgen::density::ore_vein::{OreVeinifier, VeinBlock, VeinType};
use rustmc_worldgen::density::router::{OverworldNoises, OverworldRouter};
use rustmc_worldgen::noise::{NoiseParameters, NoiseSeeding};
use serde_json::Value;
use std::collections::BTreeMap;

const ROUTER_GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/golden/router_vectors.json");
const GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/golden/noise_chunk_vectors.json"
);

fn read(path: &str) -> Value {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("golden vector {path} unreadable: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path} is not JSON: {e}"))
}

fn bits(v: &Value) -> u64 {
    v.as_str()
        .unwrap_or_else(|| panic!("expected bits-as-string, got {v}"))
        .parse::<i64>()
        .unwrap_or_else(|e| panic!("bad i64 {v}: {e}")) as u64
}

fn noises() -> OverworldNoises {
    let doc = read(ROUTER_GOLDEN);
    let mut set = OverworldNoises::new();
    for entry in doc["parameters"].as_array().expect("parameters") {
        let amplitudes: Vec<f64> = entry["amplitudes"]
            .as_array()
            .expect("amplitudes")
            .iter()
            .map(|v| f64::from_bits(bits(v)))
            .collect();
        set = set.with(
            entry["name"].as_str().expect("name"),
            NoiseParameters::new(
                entry["firstOctave"].as_i64().expect("firstOctave") as i32,
                amplitudes,
            ),
        );
    }
    set
}

/// The overworld's `defaultBlock` is stone, which is what vanilla writes
/// wherever the material rules all declined.
fn substance_name(substance: Substance) -> &'static str {
    match substance {
        Substance::Solid => "minecraft:stone",
        Substance::Air => "minecraft:air",
        Substance::Water => "minecraft:water",
        Substance::Lava => "minecraft:lava",
        Substance::Vein(VeinBlock::Ore(VeinType::Copper)) => "minecraft:copper_ore",
        Substance::Vein(VeinBlock::RawOreBlock(VeinType::Copper)) => "minecraft:raw_copper_block",
        Substance::Vein(VeinBlock::Filler(VeinType::Copper)) => "minecraft:granite",
        Substance::Vein(VeinBlock::Ore(VeinType::Iron)) => "minecraft:deepslate_iron_ore",
        Substance::Vein(VeinBlock::RawOreBlock(VeinType::Iron)) => "minecraft:raw_iron_block",
        Substance::Vein(VeinBlock::Filler(VeinType::Iron)) => "minecraft:tuff",
    }
}

fn generate(seed: i64, chunk_x: i32, chunk_z: i32) -> rustmc_worldgen::chunk_gen::FilledChunk {
    let noises = noises();
    let router = OverworldRouter::new(seed, &noises, false);
    let seeding = NoiseSeeding::new(seed);
    NoiseFill::new(
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
    .fill()
}

/// Expands the run-length encoding into `(palette index, count)` order —
/// y-major, then z, then x, matching `FilledChunk`'s own layout.
fn expand(case: &Value) -> Vec<usize> {
    let mut out = Vec::with_capacity(98_304);
    for run in case["runs"].as_array().expect("runs") {
        let pair = run.as_array().expect("[index, length]");
        let index = pair[0].as_u64().expect("index") as usize;
        let length = pair[1].as_u64().expect("length") as usize;
        out.extend(std::iter::repeat_n(index, length));
    }
    out
}

#[test]
fn noise_stage_chunks_match_vanilla_block_for_block() {
    let doc = read(GOLDEN);
    let cases = doc["cases"].as_array().expect("cases");
    assert!(!cases.is_empty(), "no chunks to compare");

    let mut total = 0usize;
    let mut wrong = 0usize;
    // (expected, got) -> count, so a systematic swap reads as one row
    // instead of thousands of scattered coordinates.
    let mut confusion: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut first_examples: Vec<String> = Vec::new();
    let mut per_chunk: Vec<String> = Vec::new();

    for case in cases {
        let seed: i64 = case["seed"].as_str().expect("seed").parse().expect("i64");
        let chunk_x = case["chunkX"].as_i64().expect("chunkX") as i32;
        let chunk_z = case["chunkZ"].as_i64().expect("chunkZ") as i32;
        let min_y = case["minY"].as_i64().expect("minY") as i32;
        let height = case["height"].as_i64().expect("height") as i32;

        let palette: Vec<&str> = case["palette"]
            .as_array()
            .expect("palette")
            .iter()
            .map(|v| v.as_str().expect("palette entry"))
            .collect();
        let expected = expand(case);
        assert_eq!(
            expected.len(),
            (height * 256) as usize,
            "run-length data does not cover the chunk"
        );

        let chunk = generate(seed, chunk_x, chunk_z);
        let mut chunk_wrong = 0usize;

        for y in min_y..min_y + height {
            for z in 0..16 {
                for x in 0..16 {
                    let index = ((y - min_y) * 256 + z * 16 + x) as usize;
                    let want = palette[expected[index]];
                    let got = substance_name(chunk.get(x, y, z));
                    total += 1;
                    if want != got {
                        wrong += 1;
                        chunk_wrong += 1;
                        *confusion
                            .entry((want.to_string(), got.to_string()))
                            .or_insert(0) += 1;
                        if first_examples.len() < 6 {
                            first_examples.push(format!(
                                "seed={seed} chunk=({chunk_x}, {chunk_z}) \
                                 block=({}, {y}, {}) want {want}, got {got}",
                                chunk_x * 16 + x,
                                chunk_z * 16 + z
                            ));
                        }
                    }
                }
            }
        }
        per_chunk.push(format!(
            "  seed={seed} chunk=({chunk_x}, {chunk_z}): {chunk_wrong}/{} wrong",
            height * 256
        ));
    }

    let exactness = 100.0 * (total - wrong) as f64 / total as f64;
    // Printed on success too — this number is the headline, and a gate that
    // only speaks when it fails cannot report a result.
    println!("NOISE-STAGE EXACTNESS {exactness:.6}% over {total} blocks ({wrong} wrong)");

    let confusion_report = confusion
        .iter()
        .map(|((want, got), count)| format!("  {want} -> {got}: {count}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        wrong,
        0,
        "{wrong} of {total} blocks differ from vanilla ({exactness:.6}% exact)\n\
         Per chunk:\n{}\nConfusion (expected -> got):\n{confusion_report}\nExamples:\n  {}",
        per_chunk.join("\n"),
        first_examples.join("\n  ")
    );
}
