//! The wrapped function of every `Interpolated` marker, at cell corners.
//!
//! This exists because `golden_interpolated` cannot localise a fault.
//! `final_density` is `min(squeeze(interpolated(pp)), noodle)`, and `noodle`
//! is `1.5 * max(|A|, |B|) + thickness` over three more markers — every
//! `min`, `max` and `rangeChoice` on that path **discards a branch**. A
//! wrapped value can be several ulp wrong at a corner, be masked there by a
//! losing branch, and only surface once the blend mixes two corners. So
//! "final_density is exact at corners" does not imply "the corner values fed
//! to the blend are exact", and this test asserts the stronger thing.
//!
//! Vectors come from `GoldenDump.interpolatorCornerVectors`, which walks
//! vanilla's own `finalDensity` with the public `mapAll` visitor and reads
//! each marker's `wrapped()` directly.

use rustmc_worldgen::density::router::{OverworldNoises, OverworldRouter};
use rustmc_worldgen::density::FunctionContext;
use rustmc_worldgen::noise::NoiseParameters;
use serde_json::Value;
use std::collections::BTreeMap;

const ROUTER_GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/golden/router_vectors.json");
const GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/golden/interpolator_corner_vectors.json"
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

/// The markers are identified by their declared bounds, which are distinct
/// enough to catch a traversal-order mismatch before it looks like a
/// numeric one:
///
/// | marker | bounds            | is                                  |
/// |--------|-------------------|-------------------------------------|
/// | 0      | ±inf              | `postProcess` — `blendDensity` is unbounded |
/// | 1      | ±3.33…            | `noodleToggle`                      |
/// | 2      | [-0.158…, 0.0083…]| `noodleThickness`                   |
/// | 3, 4   | ±3.33…            | `noodleRidgeA`, `noodleRidgeB`      |
///
/// A wrong post-order would put marker 2's narrow band somewhere else, and
/// the bounds check below fails with a legible message rather than 392
/// mismatched doubles.
#[test]
fn interpolator_wrapped_values_match_vanilla_at_corners() {
    let noises = noises();
    let doc = read(GOLDEN);
    let cases = doc["cases"].as_array().expect("cases");

    let mut checked = 0usize;
    let mut mismatched = Vec::new();
    let mut by_marker: BTreeMap<(i64, usize), (usize, usize, u64)> = BTreeMap::new();
    let mut seen_seeds = BTreeMap::new();

    for case in cases {
        let seed: i64 = case["seed"].as_str().expect("seed").parse().expect("i64");
        let marker = case["marker"].as_u64().expect("marker") as usize;

        // Rebuilt per seed, not per case: numbering the interpolators
        // mutates the graph, so each router must be numbered exactly once.
        let wrapped = seen_seeds.entry(seed).or_insert_with(|| {
            let mut router = OverworldRouter::new(seed, &noises, false);
            router.final_density.number_interpolators()
        });

        assert!(
            marker < wrapped.len(),
            "vanilla has more than {} interpolated markers in final_density \
             (asked for marker {marker}); the graph shape differs, which is a \
             bigger problem than any numeric gap",
            wrapped.len()
        );
        let f = &wrapped[marker];

        // Bounds first — a traversal-order mismatch is far more likely than
        // a bounds bug, and it makes every value look wrong.
        let want_min = f64::from_bits(bits(&case["minValue"]));
        let want_max = f64::from_bits(bits(&case["maxValue"]));
        assert_eq!(
            (f.min_value().to_bits(), f.max_value().to_bits()),
            (want_min.to_bits(), want_max.to_bits()),
            "seed={seed} marker {marker}: bounds [{}, {}] != vanilla's [{want_min}, {want_max}] \
             — the markers are almost certainly in a different order",
            f.min_value(),
            f.max_value()
        );

        let points = case["blockPoints"].as_array().expect("blockPoints");
        let expected = case["output_bits"].as_array().expect("output_bits");
        for (point, want) in points.iter().zip(expected) {
            let p = point.as_array().expect("[x,y,z]");
            let (bx, by, bz) = (
                p[0].as_i64().expect("x") as i32,
                p[1].as_i64().expect("y") as i32,
                p[2].as_i64().expect("z") as i32,
            );
            let got = f.compute(FunctionContext::new(bx, by, bz));
            checked += 1;
            let row = by_marker.entry((seed, marker)).or_insert((0, 0, 0));
            row.1 += 1;
            if got.to_bits() != bits(want) {
                row.0 += 1;
                row.2 = row.2.max(got.to_bits().abs_diff(bits(want)));
                if mismatched.len() < 8 {
                    mismatched.push(format!(
                        "seed={seed} marker={marker} at ({bx}, {by}, {bz}): expected {} got {got}",
                        f64::from_bits(bits(want))
                    ));
                }
            }
        }
    }

    assert!(checked > 0, "no samples were checked");

    let summary = by_marker
        .iter()
        .map(|((seed, marker), (bad, total, worst))| {
            format!("  seed={seed} marker={marker}: {bad}/{total}, worst {worst} ulp")
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        mismatched.is_empty(),
        "{} of {checked} wrapped corner values diverged from vanilla.\n\
         Per marker:\n{summary}\nFirst few:\n  {}",
        mismatched.len(),
        mismatched.join("\n  ")
    );
}
