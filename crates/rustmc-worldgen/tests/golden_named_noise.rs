//! Golden-vector conformance for the world-seed -> named-noise chain.
//!
//! This is the bridge from primitives to *Minecraft's* noise. It proves the
//! whole path at once: `Xoroshiro(seed).fork_positional().from_hash_of(name)`
//! (an MD5 of the namespaced identifier) feeding `NormalNoise::create` with
//! the real registry parameters.
//!
//! The parameters are carried in the vector file rather than read from
//! `data/vanilla/` — that directory is Mojang-derived and gitignored, so a
//! self-contained vector is what keeps this test runnable on a fresh clone.

use rustmc_worldgen::noise::{NoiseParameters, NoiseSeeding};
use serde_json::Value;

const GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/golden/named_noise_vectors.json"
);

fn bits(v: &Value) -> u64 {
    v.as_str()
        .unwrap_or_else(|| panic!("expected bits-as-string, got {v}"))
        .parse::<i64>()
        .unwrap_or_else(|e| panic!("bad i64 {v}: {e}")) as u64
}

fn double(v: &Value) -> f64 {
    f64::from_bits(bits(v))
}

#[test]
fn named_noise_matches_vanilla() {
    let text = std::fs::read_to_string(GOLDEN)
        .unwrap_or_else(|e| panic!("golden vector {GOLDEN} unreadable: {e}"));
    let doc: Value =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{GOLDEN} is not JSON: {e}"));
    let cases = doc["cases"].as_array().expect("cases");

    let mut names = std::collections::BTreeSet::new();

    for case in cases {
        let seed: i64 = case["seed"].as_str().expect("seed").parse().expect("i64");
        let name = case["name"].as_str().expect("name");
        let first_octave = case["firstOctave"].as_i64().expect("firstOctave") as i32;
        let amplitudes: Vec<f64> = case["amplitudes"]
            .as_array()
            .expect("amplitudes")
            .iter()
            .map(double)
            .collect();
        names.insert(name.to_string());

        let parameters = NoiseParameters::new(first_octave, amplitudes);
        let noise = NoiseSeeding::new(seed).instantiate(name, &parameters);

        assert_eq!(
            noise.max_value().to_bits(),
            bits(&case["maxValue_bits"]),
            "{name} @ seed {seed}: maxValue diverged"
        );

        let expected = case["output_bits"].as_array().expect("output_bits");
        let points = case["points"].as_array().expect("points");
        for (index, (point, want)) in points.iter().zip(expected).enumerate() {
            let p = point.as_array().expect("[x,y,z]");
            let (x, y, z) = (double(&p[0]), double(&p[1]), double(&p[2]));
            let got = noise.get_value(x, y, z);
            let want_bits = bits(want);
            assert_eq!(
                got.to_bits(),
                want_bits,
                "{name} @ seed {seed}: diverged at point {index} ({x}, {y}, {z})\n  \
                 expected {} (0x{want_bits:016x})\n  got      {got} (0x{:016x})",
                f64::from_bits(want_bits),
                got.to_bits()
            );
        }
    }

    // The real overworld field names, not placeholders.
    for expected in [
        "minecraft:continentalness",
        "minecraft:erosion",
        "minecraft:ridge",
        "minecraft:temperature",
        "minecraft:vegetation",
        "minecraft:jagged",
    ] {
        assert!(names.contains(expected), "missing vector for {expected}");
    }
    assert!(
        cases.len() >= 18,
        "expected 6 noises x 3 seeds, saw {}",
        cases.len()
    );
}
