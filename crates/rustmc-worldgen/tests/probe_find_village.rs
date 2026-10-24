//! Find a village a vanilla server can be asked to generate cheaply.
//!
//! The jigsaw assembler needs an oracle before it needs code. `DecorateGolden`
//! runs *features*, not structures, so there is no draw-level harness for a
//! village — the reference has to be a real vanilla world:
//! `ci/gen-vanilla-region.sh` at some seed and centre, then `region-differ
//! diff` restricted to the village's bounding box. That only works if a
//! village sits **close to the origin**, because the generator is capped at
//! radius 7 (256 forceloaded chunks) in one shot.
//!
//! So this looks for the target first, exactly as
//! `probe_find_badlands_mineshaft` did for the mesa mineshaft. The output is
//! coordinates for a shell command, not an assertion.
//!
//! # What this does and does not claim
//!
//! It claims the chunk is a **placement chunk** for `minecraft:villages` —
//! that part is pure arithmetic (`random_spread`, salt 10387312, spacing 34,
//! separation 8) and is already gated by `golden_structure_placement`.
//!
//! It reports the biome at the chunk's middle column at y=64 and which of the
//! five village variants lists it. That is **not** the same test vanilla
//! runs: `Structure.findValidGenerationPoint` samples at the y the structure
//! actually starts at, which for a village is the `WORLD_SURFACE_WG`
//! projection of its start piece, and a variant whose biome tag does not match
//! is simply skipped so the *next* one in the set is tried. A candidate this
//! probe likes can therefore still come out as a different variant, or as no
//! village at all. It is a shortlist, and the vanilla run is the arbiter.
//!
//! ```text
//!   cargo test --release -p rustmc-worldgen --test probe_find_village -- --nocapture
//! ```

mod common;

use std::collections::BTreeSet;

use common::facts::resolve_biome_tag;
use common::{data_path, have_extract, read};
use rustmc_worldgen::structure::placement::StructurePlacement;

/// The five village variants, in the order `villages.json` lists them —
/// which is the order vanilla tries them in, all at weight 1.
const VARIANTS: [&str; 5] = [
    "village_plains",
    "village_desert",
    "village_savanna",
    "village_snowy",
    "village_taiga",
];

#[test]
fn where_a_village_is_close_enough_to_ask_vanilla_for() {
    if !have_extract() {
        return;
    }
    let set = read(
        data_path("worldgen/structure_set/villages.json")
            .to_str()
            .expect("utf-8 path"),
    );
    let placement = StructurePlacement::parse(&set["placement"]).expect("villages placement");

    let tags: Vec<(&str, BTreeSet<String>)> = VARIANTS
        .iter()
        .map(|v| {
            let structure = read(
                data_path(&format!("worldgen/structure/{v}.json"))
                    .to_str()
                    .expect("utf-8 path"),
            );
            let tag = structure["biomes"]
                .as_str()
                .expect("biome tag")
                .trim_start_matches('#')
                .trim_start_matches("minecraft:");
            (*v, resolve_biome_tag(tag).expect("resolve biome tag"))
        })
        .collect();

    // Radius 7 is `gen-vanilla-region.sh`'s one-shot cap, and the window has
    // to hold the whole village: `max_distance_from_center` is 80 blocks, so
    // five chunks of slack either side. Chunks are scanned in rings so the
    // nearest candidate is reported first.
    for seed in [20_260_722_i64, 31_415_926, 0] {
        let mut source = biome_source(seed);
        let mut found: Vec<(i32, i32, String, Vec<&str>)> = Vec::new();
        for cz in -64..=64 {
            for cx in -64..=64 {
                if !placement
                    .is_structure_chunk_ignoring_exclusion(seed, cx, cz)
                    .unwrap_or(false)
                {
                    continue;
                }
                let (bx, by, bz) = (cx * 16 + 8, 64, cz * 16 + 8);
                let biome = source.get_noise_biome(bx >> 2, by >> 2, bz >> 2).clone();
                let matching: Vec<&str> = tags
                    .iter()
                    .filter(|(_, set)| set.contains(&biome))
                    .map(|(v, _)| *v)
                    .collect();
                if !matching.is_empty() {
                    found.push((cx, cz, biome, matching));
                }
            }
        }
        found.sort_by_key(|(cx, cz, _, _)| cx.abs().max(cz.abs()));
        println!(
            "seed {seed}: {} village placement chunks in +-64 whose middle column \
             lists a variant",
            found.len()
        );
        for (cx, cz, biome, matching) in found.iter().take(6) {
            println!(
                "  chunk ({cx},{cz})  chebyshev {}  {biome}  -> {}",
                cx.abs().max(cz.abs()),
                matching.join(", ")
            );
            println!(
                "      ci/gen-vanilla-region.sh {seed} 7 oracle-work/van-village-{seed} \
                 25711 {cx} {cz}"
            );
        }
    }
}

/// The biome table and the noise parameters, both from the golden vectors the
/// climate gate already uses — so this samples through exactly the wiring that
/// is gated bit-exact against vanilla, not a second implementation of it.
fn biome_source(seed: i64) -> rustmc_worldgen::biome::multi_noise::MultiNoiseBiomeSource<String> {
    use rustmc_worldgen::biome::multi_noise::{ClimateSampler, MultiNoiseBiomeSource};
    use rustmc_worldgen::density::router::{OverworldNoises, OverworldRouter};
    use rustmc_worldgen::noise::NoiseParameters;

    let doc = read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/golden/router_vectors.json"
    ));
    let mut noises = OverworldNoises::new();
    for entry in doc["parameters"].as_array().expect("parameters") {
        let amplitudes: Vec<f64> = entry["amplitudes"]
            .as_array()
            .expect("amplitudes")
            .iter()
            .map(|v| f64::from_bits(v.as_str().expect("bits").parse::<i64>().expect("i64") as u64))
            .collect();
        noises = noises.with(
            entry["name"].as_str().expect("name"),
            NoiseParameters::new(
                entry["firstOctave"].as_i64().expect("firstOctave") as i32,
                amplitudes,
            ),
        );
    }
    let router = OverworldRouter::new(seed, &noises, false);
    MultiNoiseBiomeSource::new(ClimateSampler::from_router(&router), biome_table())
}

/// The overworld parameter table, from the climate gate's own golden vectors.
fn biome_table() -> rustmc_worldgen::biome::climate::ParameterList<String> {
    use rustmc_worldgen::biome::climate::{Parameter, ParameterList, ParameterPoint};

    let doc = read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/golden/climate_vectors.json"
    ));
    let long = |v: &serde_json::Value| -> i64 {
        v.as_str()
            .unwrap_or_else(|| panic!("expected a long-as-string, got {v}"))
            .parse()
            .expect("i64")
    };
    let case = doc["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .find(|c| c["case"] == "overworld_climate")
        .expect("no overworld_climate case");
    let entries = case["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .map(|entry| {
            let s = entry["space"].as_array().expect("space");
            let axis = |i: usize| Parameter::new(long(&s[i * 2]), long(&s[i * 2 + 1]));
            (
                ParameterPoint {
                    temperature: axis(0),
                    humidity: axis(1),
                    continentalness: axis(2),
                    erosion: axis(3),
                    depth: axis(4),
                    weirdness: axis(5),
                    offset: long(&s[12]),
                },
                entry["biome"].as_str().expect("biome").to_string(),
            )
        })
        .collect();
    ParameterList::new(entries)
}
