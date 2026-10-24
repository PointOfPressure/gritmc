//! `BiomeManager.getBiome(BlockPos)` — the fuzzed lookup, against vanilla's
//! own answers.
//!
//! # Why this is not the biome container gate
//!
//! A chunk's biome *container* is quart-aligned and this port matches vanilla
//! on it with zero mismatches over a 25-chunk window. The lookup a **feature**
//! uses is different: `WorldGenLevel.getBiome(BlockPos)` goes through
//! `BiomeManager`, which offsets by −2, takes the quart cell, and then adds a
//! per-cell fuzz derived from the world seed. Two positions inside one
//! container cell can therefore answer differently, and a port can match every
//! container and still place features in the wrong biomes.
//!
//! That is not hypothetical here. `BiomeFilter` — the last modifier on
//! `sulfur_spike`'s chain — is the only thing rejecting its candidates, and it
//! rejects ten more of them than vanilla's does in chunk (62,62).
//!
//! # The oracle, and why the order matters
//!
//! `DecorateGolden` records every position vanilla's `getBiome` was asked
//! about and what it answered, and `decorate_vectors_sulfur.json` carries
//! 4,667 of them.
//!
//! They are replayed **in the recorded order**, which is not fussiness.
//! `Climate.RTree.search` seeds itself from the previous query's result and
//! prunes with a strict `>`, so a position that is exactly equidistant between
//! two biomes answers according to what was asked before it. Both sides have
//! that property; querying them in different orders would manufacture
//! disagreements that neither generator would produce.

use rustmc_worldgen::biome::climate::ParameterList;
use rustmc_worldgen::biome::multi_noise::ClimateSampler;
use rustmc_worldgen::density::router::OverworldRouter;

const VECTOR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../crates/rustmc-worldgen/golden/decorate_vectors_sulfur.json"
);

#[test]
fn the_fuzzed_biome_lookup_matches_vanillas_answers() {
    if !worldgen_assembly::have_extract() {
        eprintln!("skipped: no vanilla extract");
        return;
    }
    let Ok(text) = std::fs::read_to_string(VECTOR) else {
        eprintln!("skipped: no {VECTOR}");
        return;
    };
    let doc: serde_json::Value = serde_json::from_str(&text).expect("vector is JSON");
    let seed: i64 = doc["seed"]
        .as_str()
        .expect("seed is a long-as-string")
        .parse()
        .expect("i64");

    let params = worldgen_assembly::noise_parameters();
    let noises = worldgen_assembly::overworld_noises(&params);
    let router = OverworldRouter::new(seed, &noises, false);
    let sampler = ClimateSampler::from_router(&router);
    let mut list = ParameterList::new(worldgen_assembly::biome_table());
    let zoom = rustmc_worldgen::biome::manager::obfuscate_seed(seed);

    let mut asked = 0usize;
    let mut wrong = 0usize;
    let mut examples: Vec<String> = Vec::new();

    for group in doc["biomeAnswers"].as_array().expect("biomeAnswers") {
        for pair in group.as_array().expect("a list of [pos, biome]") {
            let entry = pair.as_array().expect("[pos, biome]");
            let pos = entry[0].as_str().expect("position");
            let want = entry[1].as_str().expect("biome");
            let mut parts = pos.split(',').map(|v| v.parse::<i32>().expect("i32"));
            let (x, y, z) = (
                parts.next().expect("x"),
                parts.next().expect("y"),
                parts.next().expect("z"),
            );

            let (qx, qy, qz) = rustmc_worldgen::biome::manager::quart_for_block(zoom, x, y, z);
            let got = list.find_value(sampler.sample(qx, qy, qz)).clone();
            asked += 1;
            if got != want {
                wrong += 1;
                if examples.len() < 10 {
                    examples.push(format!("({x},{y},{z}) want {want}, got {got}"));
                }
            }
        }
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "a percentage over a few thousand lookups"
    )]
    let exactness = 100.0 * (asked - wrong) as f64 / asked as f64;
    println!("BIOME LOOKUP EXACTNESS {exactness:.6}% over {asked} positions ({wrong} wrong)");
    for e in &examples {
        println!("  {e}");
    }

    assert!(asked > 1000, "the vector should carry thousands of answers");
    assert_eq!(
        wrong, 0,
        "the fuzzed biome lookup disagrees with vanilla at {wrong} of {asked} positions"
    );
}
