//! What does our `minecraft:packed_ice` surface noise actually return?
//!
//! The overworld rule places `packed_ice` in `frozen_peaks` behind
//! `noise_threshold` bands of `[0.0, 0.2]` and `[-0.5, 0.2]`, sampled in 2D.
//! We place the open-water fallback at 4,093 positions in one chunk where
//! vanilla places ice, and everything else in the chain is accounted for:
//! the enclosing conditions pass (we reach that sequence's fallback), the
//! biome grid matches at 73,728 quart cells, `steep` is a line-for-line port,
//! and the noise is present in the parameter set.
//!
//! So either the value is wrong, or the band is never tested. A value inside
//! `[-0.5, 0.2]` at these positions would mean the condition is not being
//! reached at all and the fault is in the rule tree rather than the noise.

use rustmc_worldgen::noise::{NoiseParameters, NoiseSeeding};
use serde_json::Value;

const ROUTER: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/golden/router_vectors.json");

fn long(v: &Value) -> i64 {
    v.as_str()
        .unwrap_or_else(|| panic!("expected a long-as-string, got {v}"))
        .parse()
        .unwrap_or_else(|e| panic!("bad i64 {v}: {e}"))
}

#[test]
fn what_the_packed_ice_noise_returns_where_vanilla_places_ice() {
    let text = std::fs::read_to_string(ROUTER).expect("router vectors");
    let doc: Value = serde_json::from_str(&text).expect("json");
    let entry = doc["parameters"]
        .as_array()
        .expect("parameters")
        .iter()
        .find(|e| e["name"] == "minecraft:packed_ice")
        .expect("no packed_ice noise in the parameter set");
    let amplitudes: Vec<f64> = entry["amplitudes"]
        .as_array()
        .expect("amplitudes")
        .iter()
        .map(|v| f64::from_bits(long(v) as u64))
        .collect();
    let first_octave = entry["firstOctave"].as_i64().expect("firstOctave") as i32;
    println!(
        "packed_ice: firstOctave {first_octave}, {} amplitudes",
        amplitudes.len()
    );

    let seeding = NoiseSeeding::new(31_415_926);
    let noise = seeding.instantiate(
        "minecraft:packed_ice",
        &NoiseParameters::new(first_octave, amplitudes),
    );

    // Chunk (0, -135): the failing positions run x 0..15, z -2160..-2145.
    let mut inside = 0usize;
    let mut total = 0usize;
    let mut sample = Vec::new();
    for x in 0..16 {
        for z in -2160..-2144 {
            let v = noise.get_value(f64::from(x), 0.0, f64::from(z));
            total += 1;
            if (-0.5..=0.2).contains(&v) {
                inside += 1;
            }
            if sample.len() < 6 {
                sample.push((x, z, v));
            }
        }
    }
    println!("inside [-0.5, 0.2]: {inside}/{total}");
    for (x, z, v) in &sample {
        println!("  ({x}, 0, {z}) -> {v:+.6}");
    }
}
