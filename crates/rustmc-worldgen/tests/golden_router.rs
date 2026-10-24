//! Golden-vector conformance for the overworld noise router.
//!
//! This is the one that matters. Everything below it is provable in
//! isolation; the router is the first layer where a wrong answer generates,
//! loads, and looks like Minecraft while being a different world. The
//! vectors come from vanilla building its own router — `VanillaRegistries`
//! resolves `NoiseRouterData`, `RandomState` seeds it from the world seed —
//! so a match here means our terrain density *is* vanilla's.

use rustmc_worldgen::density::router::{OverworldNoises, OverworldRouter};
use rustmc_worldgen::density::FunctionContext;
use rustmc_worldgen::noise::NoiseParameters;
use serde_json::Value;

const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/golden/router_vectors.json");

fn bits(v: &Value) -> u64 {
    v.as_str()
        .unwrap_or_else(|| panic!("expected bits-as-string, got {v}"))
        .parse::<i64>()
        .unwrap_or_else(|e| panic!("bad i64 {v}: {e}")) as u64
}

/// Rebuilds the noise set from the parameters vanilla actually used.
fn noises(doc: &Value) -> OverworldNoises {
    let mut set = OverworldNoises::new();
    for entry in doc["parameters"].as_array().expect("parameters") {
        let name = entry["name"].as_str().expect("name");
        let first_octave = entry["firstOctave"].as_i64().expect("firstOctave") as i32;
        let amplitudes: Vec<f64> = entry["amplitudes"]
            .as_array()
            .expect("amplitudes")
            .iter()
            .map(|v| f64::from_bits(bits(v)))
            .collect();
        set = set.with(name, NoiseParameters::new(first_octave, amplitudes));
    }
    set
}

#[test]
fn overworld_router_matches_vanilla() {
    let text = std::fs::read_to_string(GOLDEN)
        .unwrap_or_else(|e| panic!("golden vector {GOLDEN} unreadable: {e}"));
    let doc: Value =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{GOLDEN} is not JSON: {e}"));
    let noises = noises(&doc);
    let cases = doc["cases"].as_array().expect("cases");

    let mut checked = 0usize;
    let mut mismatched: Vec<String> = Vec::new();

    for case in cases {
        let field = case["case"].as_str().expect("case");
        let seed: i64 = case["seed"].as_str().expect("seed").parse().expect("i64");
        let amplified = case["amplified"].as_bool().expect("amplified");

        let router = OverworldRouter::new(seed, &noises, amplified);
        let function = match field {
            "depth" => &router.depth,
            "continents" => &router.continents,
            "erosion" => &router.erosion,
            "ridges" => &router.ridges,
            "final_density" => &router.final_density,
            "barrier" => &router.barrier,
            "fluid_level_floodedness" => &router.fluid_level_floodedness,
            "fluid_level_spread" => &router.fluid_level_spread,
            "lava" => &router.lava,
            "temperature" => &router.temperature,
            "vegetation" => &router.vegetation,
            "preliminary_surface_level" => &router.preliminary_surface_level,
            "vein_toggle" => &router.vein_toggle,
            "vein_ridged" => &router.vein_ridged,
            "vein_gap" => &router.vein_gap,
            // Not a fallback: a field vanilla emits and we cannot name is a
            // field we have not ported, and silently skipping it would let
            // the suite report green over a hole.
            other => panic!("unknown router field {other:?}"),
        };

        let expected = case["output_bits"].as_array().expect("output_bits");
        let points = case["blockPoints"].as_array().expect("blockPoints");
        for (point, want) in points.iter().zip(expected) {
            let p = point.as_array().expect("[x,y,z]");
            let ctx = FunctionContext::new(
                p[0].as_i64().expect("x") as i32,
                p[1].as_i64().expect("y") as i32,
                p[2].as_i64().expect("z") as i32,
            );
            let got = function.compute(ctx);
            checked += 1;
            if got.to_bits() != bits(want) {
                mismatched.push(format!(
                    "{field} seed={seed} amplified={amplified} at ({}, {}, {}): expected {} got {}",
                    ctx.block_x,
                    ctx.block_y,
                    ctx.block_z,
                    f64::from_bits(bits(want)),
                    got
                ));
            }
        }
    }

    assert!(checked > 0, "no router samples were checked");
    assert!(
        mismatched.is_empty(),
        "{} of {checked} router samples diverged from vanilla:\n  {}",
        mismatched.len(),
        mismatched.join("\n  ")
    );
}
