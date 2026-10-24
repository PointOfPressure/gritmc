//! Surface depth and the badlands band palette, against real vanilla.
//!
//! These are the parts of the surface system that are pure functions of the
//! world seed, so they can be proved without building a chunk. Between them
//! they cover the pieces most likely to be quietly wrong:
//!
//! - `getSurfaceDepth`'s formula, its truncating cast, and the fact that its
//!   quarter-block jitter comes from the world's **base** positional factory
//!   rather than a named stream.
//! - The whole 192-entry band palette — the orange pass's double increment,
//!   three overwrite passes, white bands with grey edges — plus `Math.round`
//!   on the offset noise and the wrapping modulo.
//!
//! Regenerate with `ci/surface-golden.sh`.

use rustmc_worldgen::noise::NoiseParameters;
use rustmc_worldgen::surface::eval::SurfaceOracle;
use rustmc_worldgen::surface::world::{ChunkSurfaceState, WorldSurfaceOracle};
use serde_json::Value;
use std::collections::BTreeMap;

const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/golden/surface_vectors.json");
const ROUTER_GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/golden/router_vectors.json");

fn read(path: &str) -> Value {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("golden vector {path} unreadable: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path} is not JSON: {e}"))
}

/// The real noise parameters, from the router vector file's registry dump.
fn parameters() -> BTreeMap<String, NoiseParameters> {
    let doc = read(ROUTER_GOLDEN);
    doc["parameters"]
        .as_array()
        .expect("parameters")
        .iter()
        .map(|entry| {
            let amplitudes: Vec<f64> = entry["amplitudes"]
                .as_array()
                .expect("amplitudes")
                .iter()
                .map(|v| {
                    let bits: i64 = v.as_str().expect("bits-as-string").parse().expect("i64");
                    f64::from_bits(bits as u64)
                })
                .collect();
            (
                entry["name"].as_str().expect("name").to_string(),
                NoiseParameters::new(
                    entry["firstOctave"].as_i64().expect("firstOctave") as i32,
                    amplitudes,
                ),
            )
        })
        .collect()
}

fn oracle(seed: i64, params: &BTreeMap<String, NoiseParameters>) -> WorldSurfaceOracle<'static> {
    // None of these are consulted by surface depth or the bands; they are
    // here because the state is required to build an oracle at all.
    static HEIGHTS: [i32; 256] = [64; 256];
    WorldSurfaceOracle::new(
        seed,
        params,
        ChunkSurfaceState {
            heights: &HEIGHTS,
            biome: &|_, _, _| 0,
            biome_name: &|_| "minecraft:plains".to_string(),
            min_surface_level: &|_, _| 0,
            temperature: &|_, _, _| false,
            // No frozen ocean in this fixture's columns.
            melt_iceberg_slightly: &|_, _, _| false,
        },
    )
}

#[test]
fn surface_depth_matches_vanilla() {
    let doc = read(GOLDEN);
    let params = parameters();
    let mut total = 0usize;
    let mut wrong = Vec::new();

    for case in doc["cases"].as_array().expect("cases") {
        let seed: i64 = case["seed"].as_str().expect("seed").parse().expect("i64");
        let oracle = oracle(seed, &params);
        let points = case["depthPoints"].as_array().expect("depthPoints");
        let depths = case["depths"].as_array().expect("depths");
        assert_eq!(points.len(), depths.len());

        for (point, want) in points.iter().zip(depths) {
            let p = point.as_array().expect("[x, z]");
            let (x, z) = (
                p[0].as_i64().expect("x") as i32,
                p[1].as_i64().expect("z") as i32,
            );
            let want = want.as_i64().expect("depth") as i32;
            let got = oracle.surface_depth_at(x, z);
            total += 1;
            if got != want && wrong.len() < 8 {
                wrong.push(format!("seed={seed} at ({x}, {z}): want {want}, got {got}"));
            } else if got != want {
                wrong.push(String::new());
            }
        }
    }

    let bad = wrong.iter().filter(|w| !w.is_empty()).count()
        + wrong.iter().filter(|w| w.is_empty()).count();
    let exactness = 100.0 * (total - bad) as f64 / total as f64;
    println!("SURFACE DEPTH EXACTNESS {exactness:.6}% over {total} columns ({bad} wrong)");
    assert!(
        bad == 0,
        "{bad} of {total} surface depths differ ({exactness:.6}% exact)\n  {}",
        wrong
            .iter()
            .filter(|w| !w.is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

#[test]
fn the_badlands_band_palette_matches_vanilla() {
    let doc = read(GOLDEN);
    let params = parameters();
    let mut total = 0usize;
    let mut wrong = Vec::new();

    for case in doc["cases"].as_array().expect("cases") {
        let seed: i64 = case["seed"].as_str().expect("seed").parse().expect("i64");
        let mut oracle = oracle(seed, &params);
        let points = case["bandPoints"].as_array().expect("bandPoints");
        let bands = case["bands"].as_array().expect("bands");
        assert_eq!(points.len(), bands.len());

        for (point, want) in points.iter().zip(bands) {
            let p = point.as_array().expect("[x, y, z]");
            let (x, y, z) = (
                p[0].as_i64().expect("x") as i32,
                p[1].as_i64().expect("y") as i32,
                p[2].as_i64().expect("z") as i32,
            );
            let want = want.as_str().expect("band name");
            let got = oracle
                .bandlands(x, y, z)
                .expect("bandlands always answers")
                .0;
            total += 1;
            if got != want {
                if wrong.len() < 8 {
                    wrong.push(format!(
                        "seed={seed} at ({x}, {y}, {z}): want {want}, got {got}"
                    ));
                } else {
                    wrong.push(String::new());
                }
            }
        }
    }

    let bad = wrong.len();
    let exactness = 100.0 * (total - bad) as f64 / total as f64;
    println!("BADLANDS BAND EXACTNESS {exactness:.6}% over {total} positions ({bad} wrong)");
    assert!(
        bad == 0,
        "{bad} of {total} bands differ ({exactness:.6}% exact)\n  {}",
        wrong
            .iter()
            .filter(|w| !w.is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}
