//! The `temperature` surface condition, proved in two layers.
//!
//! Vectors from `ci/temperature-golden.sh`. The condition asks a biome
//! whether it is `coldEnoughToSnow`, which is a float comparison against a
//! height-adjusted base temperature, which is a function of three static
//! `PerlinSimplexNoise` stacks. Two independent things to get wrong, so both
//! are measured separately:
//!
//! 1. **The noise.** Raw `getValue` at the scales the callers actually use,
//!    compared bit for bit. A failure here is `PerlinSimplexNoise` or
//!    `SimplexNoise`.
//! 2. **The chain.** `coldEnoughToSnow` for all 66 biomes over a 478-position
//!    grid. Passing (1) and failing (2) puts the bug in the float arithmetic
//!    or the `frozen` modifier, not the noise.
//!
//! # The oracle audits its own premise
//!
//! The three noises are *private* statics on `Biome`, so the harness rebuilds
//! them from the constructor arguments read out of the decompile — which is a
//! claim, not a fact. `BIOME_INFO_NOISE` is public, so the harness compares
//! its rebuilt 2345/`[0]` stack against the real one and records the answer.
//! This test refuses to run on vectors where that flag is false: they would
//! describe a noise vanilla does not use, and every number below would be
//! meaningless in a way that looks like success.

use std::collections::BTreeMap;

use rustmc_worldgen::biome::temperature::{BiomeClimate, TemperatureModifier, TemperatureNoises};
use rustmc_worldgen::noise::PerlinSimplexNoise;
use rustmc_worldgen::rng::{LegacyRandomSource, WorldgenRandom};
use serde_json::Value;

const GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/golden/temperature_vectors.json"
);
/// Mojang's own datagen output, the same source the surface rule is read
/// from — not a hand-transcribed table of 66 temperatures.
const BIOME_DATA: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../oracle-work/vanilla-26.2/generated-data/data/minecraft/worldgen/biome"
);

fn bits(v: &Value) -> f64 {
    f64::from_bits(
        v.as_str()
            .expect("raw bits as string")
            .parse::<i64>()
            .expect("i64") as u64,
    )
}

fn float_bits(v: &Value) -> f32 {
    f32::from_bits(
        v.as_str()
            .expect("raw bits as string")
            .parse::<i32>()
            .expect("i32") as u32,
    )
}

fn vectors() -> Value {
    let text = std::fs::read_to_string(GOLDEN)
        .unwrap_or_else(|e| panic!("{GOLDEN} unreadable ({e}) — run ci/temperature-golden.sh"));
    serde_json::from_str(&text).expect("vectors are JSON")
}

/// The datapack form of every overworld biome's climate settings.
fn climates() -> BTreeMap<String, BiomeClimate> {
    let dir = std::fs::read_dir(BIOME_DATA).unwrap_or_else(|e| {
        panic!("{BIOME_DATA} unreadable ({e}) — run ci/vanilla-server.sh data")
    });
    let mut out = BTreeMap::new();
    for entry in dir {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("utf-8 filename")
            .to_string();
        let json: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("biome json readable"))
                .expect("biome json parses");
        // `temperature` is a float in the game and a JSON number here, so it
        // has to round-trip through f64 -> f32 exactly the way the codec
        // does. Every value in the data is a short decimal, so it does.
        #[allow(clippy::cast_possible_truncation)]
        let temperature = json["temperature"].as_f64().expect("temperature") as f32;
        let modifier = json
            .get("temperature_modifier")
            .and_then(Value::as_str)
            .map_or(TemperatureModifier::None, |name| {
                TemperatureModifier::parse(name).expect("known temperature modifier")
            });
        out.insert(
            format!("minecraft:{stem}"),
            BiomeClimate {
                temperature,
                modifier,
            },
        );
    }
    out
}

fn stack(seed: i64, octaves: &[i32]) -> PerlinSimplexNoise {
    let mut random = WorldgenRandom::new(LegacyRandomSource::new(seed));
    PerlinSimplexNoise::new(&mut random, octaves)
}

#[test]
fn perlin_simplex_stacks_match_vanilla() {
    let vectors = vectors();
    assert!(
        vectors["rebuiltBiomeInfoMatchesVanilla"]
            .as_bool()
            .expect("premise flag"),
        "the oracle's rebuilt BIOME_INFO_NOISE did not match Biome's own static, \
         so these vectors describe a noise vanilla does not use"
    );

    // Checked twice, against two different constructions, because the first
    // version of this test only did the left-hand one and passed while
    // `TemperatureNoises` held a wrong stack. Building from the vectors'
    // declared octave set proves `PerlinSimplexNoise`; going through
    // `TemperatureNoises` proves the thing the surface system actually uses.
    let held = TemperatureNoises::new();
    let mut checked = 0usize;
    for entry in vectors["noise"].as_array().expect("noise stacks") {
        let name = entry["name"].as_str().expect("name");
        let seed: i64 = entry["seed"].as_str().expect("seed").parse().expect("i64");
        let octaves: Vec<i32> = match entry["octaves"].as_str().expect("octaves") {
            "[0]" => vec![0],
            "[-2,-1,0]" => vec![-2, -1, 0],
            other => panic!("unrecognised octave set {other} — the harness changed"),
        };
        let noise = stack(seed, &octaves);
        let in_use = match name {
            "temperature" => held.temperature_noise(),
            "frozen" => held.frozen_noise(),
            "biome_info" => held.biome_info_noise(),
            other => panic!("unrecognised stack {other} — the harness changed"),
        };

        for sample in entry["samples"].as_array().expect("samples") {
            let sample = sample.as_array().expect("[x, y, value]");
            let (x, y, expected) = (bits(&sample[0]), bits(&sample[1]), bits(&sample[2]));
            let got = noise.get_value(x, y, false);
            assert_eq!(
                got.to_bits(),
                expected.to_bits(),
                "{name} stack diverged at ({x}, {y}): got {got}, vanilla {expected}"
            );
            let got_in_use = in_use.get_value(x, y, false);
            assert_eq!(
                got_in_use.to_bits(),
                expected.to_bits(),
                "TemperatureNoises' {name} stack diverged at ({x}, {y}): \
                 got {got_in_use}, vanilla {expected} — the stack is built correctly \
                 in the test and wrongly in the library"
            );
            checked += 1;
        }
    }
    assert!(
        checked >= 800,
        "only {checked} noise samples — vectors shrank"
    );
    println!("PERLIN SIMPLEX 100.000000% over {checked} samples");
}

#[test]
fn cold_enough_to_snow_matches_vanilla_for_every_biome() {
    let vectors = vectors();
    assert!(
        vectors["rebuiltBiomeInfoMatchesVanilla"]
            .as_bool()
            .expect("premise flag"),
        "vectors describe a noise vanilla does not use"
    );

    let sea_level = i32::try_from(vectors["seaLevel"].as_i64().expect("seaLevel")).expect("i32");
    let positions: Vec<[i32; 3]> = vectors["positions"]
        .as_array()
        .expect("positions")
        .iter()
        .map(|p| {
            let p = p.as_array().expect("[x, y, z]");
            [0, 1, 2].map(|i| i32::try_from(p[i].as_i64().expect("coordinate")).expect("i32"))
        })
        .collect();

    let noises = TemperatureNoises::new();
    let climates = climates();

    let mut wrong = 0usize;
    let mut total = 0usize;
    // Which biomes disagree and in which direction — a count alone would not
    // say whether the modifier is missing (frozen biomes wrong only) or the
    // arithmetic is (everything above the snow level wrong).
    let mut by_biome: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    let mut above_snow_level = 0usize;

    for row in vectors["biomes"].as_array().expect("biomes") {
        let id = row["id"].as_str().expect("id");
        let expected_base = float_bits(&row["baseTemperature"]);
        let climate = *climates
            .get(id)
            .unwrap_or_else(|| panic!("{id} has no entry in the generated biome data"));
        assert_eq!(
            climate.temperature.to_bits(),
            expected_base.to_bits(),
            "{id}: base temperature from the datapack ({}) is not the one the game reports ({expected_base})",
            climate.temperature
        );

        let expected: Vec<bool> = row["cold"]
            .as_str()
            .expect("cold bitstring")
            .chars()
            .map(|c| c == '1')
            .collect();
        assert_eq!(
            expected.len(),
            positions.len(),
            "{id}: bitstring length does not match the position list"
        );

        for (position, &want) in positions.iter().zip(&expected) {
            let [x, y, z] = *position;
            let got = noises.cold_enough_to_snow(climate, x, y, z, sea_level);
            total += 1;
            if got != want {
                wrong += 1;
                let counts = by_biome.entry(id).or_default();
                if got {
                    counts.0 += 1;
                } else {
                    counts.1 += 1;
                }
                if y > sea_level + 17 {
                    above_snow_level += 1;
                }
            }
        }
    }

    #[allow(clippy::cast_precision_loss)]
    let exactness = (total - wrong) as f64 * 100.0 / total as f64;
    if wrong > 0 {
        for (id, (false_cold, false_warm)) in &by_biome {
            println!("  {id}: {false_cold} wrongly cold, {false_warm} wrongly warm");
        }
        println!("  {above_snow_level} of {wrong} were above the snow level");
    }
    assert_eq!(
        wrong, 0,
        "COLD ENOUGH TO SNOW {exactness:.6}% over {total} lookups ({wrong} wrong)"
    );
    println!("COLD ENOUGH TO SNOW {exactness:.6}% over {total} lookups");
}

#[test]
fn the_frozen_modifier_is_actually_reached_by_these_vectors() {
    // A gate that only ever exercises `TemperatureModifier::None` would pass
    // with the modifier deleted. This asserts the vectors contain a biome
    // that carries it and that vanilla answered both ways for that biome —
    // otherwise the 100% above is not evidence about the modifier at all.
    let vectors = vectors();
    let climates = climates();
    let frozen: Vec<&str> = climates
        .iter()
        .filter(|(_, c)| c.modifier == TemperatureModifier::Frozen)
        .map(|(id, _)| id.as_str())
        .collect();
    assert_eq!(
        frozen,
        vec!["minecraft:deep_frozen_ocean", "minecraft:frozen_ocean"],
        "the set of biomes carrying the frozen modifier changed"
    );

    for id in frozen {
        let row = vectors["biomes"]
            .as_array()
            .expect("biomes")
            .iter()
            .find(|r| r["id"].as_str() == Some(id))
            .unwrap_or_else(|| panic!("{id} missing from the vectors"));
        let cold = row["cold"].as_str().expect("cold bitstring");
        assert!(
            cold.contains('1') && cold.contains('0'),
            "{id} answered the same everywhere, so the modifier's patches were never sampled"
        );
    }
}
