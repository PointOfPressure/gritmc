//! End-to-end conformance: vanilla's **interpolated** density, per block.
//!
//! The vectors come from `NoiseBasedChunkGenerator.getInterpolatedNoiseValue`,
//! which builds a real one-cell `NoiseChunk` around each block and runs
//! vanilla's own interpolation. Matching it exercises the entire pipeline in
//! one assertion — named noises, the router, the terrain shaper, caves, the
//! slide, post-processing, and cell interpolation — at blocks that are
//! deliberately *off* cell corners, where interpolation is the only thing
//! producing the answer.

use rustmc_worldgen::density::cell::CellGeometry;
use rustmc_worldgen::density::chunk::{ChunkSampler, Root};
use rustmc_worldgen::density::flat_cache::FlatCaches;
use rustmc_worldgen::density::router::{OverworldNoises, OverworldRouter};
use rustmc_worldgen::noise::NoiseParameters;
use serde_json::Value;
use std::collections::BTreeMap;

const ROUTER_GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/golden/router_vectors.json");
const GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/golden/interpolated_density_vectors.json"
);

/// Overworld noise settings: y from -64 over 384 blocks, 4x8 cells.
const MIN_Y: i32 = -64;
const HEIGHT: i32 = 384;
const CELL_WIDTH: i32 = 4;
const CELL_HEIGHT: i32 = 8;

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

/// The noise parameters live in the router vector file, which already emits
/// the whole registry.
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

/// # RESOLVED 2026-07-24 — 1024/1024 exact
///
/// This test spent four investigations ignored, at 801/1024 with the rest
/// off by exactly one ulp. It was never loosened to a tolerance, and the two
/// bugs it was reporting turned out to be real. Both are worth keeping on
/// record, because both were invisible to every check that came before.
///
/// ## Bug 1 — the cell-fill lerp order
///
/// `NoiseInterpolator.compute` branches on `fillingCell`: the staged
/// `updateForY/X/Z` collapses Y then X then Z, while `Mth.lerp3` collapses X
/// then Y then Z. Those disagree by one ulp on about 45% of cells, and
/// terrain goes through `lerp3` — `finalDensity` is wrapped in
/// `cacheAllInCell`, whose array is filled with `fillingCell = true`, and
/// `getInterpolatedDensity` reads that array while the staged value is
/// discarded. `cell.rs` used the staged order for both. See `density/cell.rs`.
///
/// ## Bug 2 — the `0.64` on the wrong side of the marker
///
/// `postProcess` is `interpolated(mul(blended, 0.64)).squeeze()`: the scale
/// is applied at the lattice corners, *before* the blend. Ours applied it
/// after. That is algebraically identical and bit-identical at every corner,
/// since the blend is the identity there — but `0.64 * lerp(t, a, b)` and
/// `lerp(t, 0.64a, 0.64b)` round differently in a cell interior. See
/// `density::build::post_process`.
///
/// ## Why it took four passes
///
/// Every hypothesis was tested against `final_density`, and `final_density`
/// is `min(squeeze(interpolated(pp)), noodle)` where `noodle` is
/// `1.5 * max(|A|, |B|) + thickness`. Every `min`, `max` and `rangeChoice`
/// on that path **discards a branch**, so a wrapped value can be badly wrong
/// and still be masked. Bug 2 was off by a factor of 1.5625 — not one ulp,
/// a 56% error — and no corner test could see it.
///
/// What broke the deadlock was measuring the thing directly instead of
/// inferring it: `golden_interpolator_corners` dumps each `Interpolated`
/// marker's *wrapped* value straight out of vanilla's graph. It localised
/// the fault to one of five markers in a single run, after four rounds of
/// reasoning about the aggregate had failed to. The lesson is cheap to state
/// and was expensive to learn: **when an aggregate diverges, do not bisect
/// hypotheses about it — find a way to observe its inputs.**
#[test]
fn interpolated_density_matches_vanilla() {
    let noises = noises();
    let doc = read(GOLDEN);
    let cases = doc["cases"].as_array().expect("cases");

    let mut checked = 0usize;
    let mut mismatched = Vec::new();
    let mut off_corner = 0usize;
    let mut by_column: BTreeMap<(i64, i32, i32), (usize, usize, u64)> = BTreeMap::new();

    for case in cases {
        let seed: i64 = case["seed"].as_str().expect("seed").parse().expect("i64");
        let router = OverworldRouter::new(seed, &noises, false);

        let points = case["blockPoints"].as_array().expect("blockPoints");
        let expected = case["output_bits"].as_array().expect("output_bits");

        // Vanilla builds its one-cell chunk at the block's own cell; the
        // lattice tiles the same way from the containing chunk, so aligning
        // on the chunk origin gives the identical corners.
        let first = points[0].as_array().expect("[x,y,z]");
        let (x, z) = (
            first[0].as_i64().expect("x") as i32,
            first[2].as_i64().expect("z") as i32,
        );
        // Vanilla's one-cell NoiseChunk caches two quart columns; ours caches
        // the chunk's five. Both agree at cell corners, which are quart
        // aligned, and nothing outside an interpolated marker reads a flat
        // cache — so the wider table is the same table where it is read.
        let (chunk_min_x, chunk_min_z) = (x.div_euclid(16) * 16, z.div_euclid(16) * 16);
        let mut density = router.final_density.clone();
        let flat = FlatCaches::build(&mut [&mut density], chunk_min_x, chunk_min_z, 16);
        let mut sampler = ChunkSampler::new(
            vec![Root::cell_filled(density)],
            &flat,
            CellGeometry::new(
                chunk_min_x,
                chunk_min_z,
                MIN_Y,
                HEIGHT,
                CELL_WIDTH,
                CELL_HEIGHT,
            ),
        );

        for (point, want) in points.iter().zip(expected) {
            let p = point.as_array().expect("[x,y,z]");
            let (bx, by, bz) = (
                p[0].as_i64().expect("x") as i32,
                p[1].as_i64().expect("y") as i32,
                p[2].as_i64().expect("z") as i32,
            );
            if (by - MIN_Y) % CELL_HEIGHT != 0 || bx % CELL_WIDTH != 0 || bz % CELL_WIDTH != 0 {
                off_corner += 1;
            }
            let got = sampler.density(&flat, bx, by, bz);
            checked += 1;
            let row = by_column.entry((seed, bx, bz)).or_insert((0, 0, 0));
            row.1 += 1;
            if got.to_bits() != bits(want) {
                row.0 += 1;
                row.2 = row.2.max(got.to_bits().abs_diff(bits(want)));
                mismatched.push(format!(
                    "seed={seed} at ({bx}, {by}, {bz}): expected {} got {got}",
                    f64::from_bits(bits(want))
                ));
            }
        }
    }

    assert!(checked > 0, "no samples were checked");
    assert!(
        off_corner > checked / 2,
        "most samples should be off cell corners, only {off_corner} of {checked} were"
    );
    // Grouped by column, because *where* the divergences land is the whole
    // diagnostic: a fault in the interpolation arithmetic hits every column,
    // whereas one in the quart-snapping caches spares the columns that are
    // already quart-aligned. A flat list of 200 samples hides that.
    let summary = column_summary(&by_column);
    assert!(
        mismatched.is_empty(),
        "{} of {checked} interpolated samples diverged from vanilla.\n\
         Per column (seed, x, z): count, worst ulp gap:\n{summary}\n\
         First few:\n  {}",
        mismatched.len(),
        mismatched
            .iter()
            .take(8)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

/// Rows are `(seed, x, z) -> (diverged, total, worst ulp gap)`, ordered so
/// the report is stable run to run.
fn column_summary(by_column: &BTreeMap<(i64, i32, i32), (usize, usize, u64)>) -> String {
    by_column
        .iter()
        .map(|((seed, x, z), (bad, total, worst))| {
            // A column on a cell corner in xz has factor_x == factor_z == 0,
            // so only the y lerp runs and the interpolation order cannot
            // matter. Separating those from the rest is what distinguishes a
            // blend fault from a corner-value fault.
            let aligned = if x.rem_euclid(CELL_WIDTH) == 0 && z.rem_euclid(CELL_WIDTH) == 0 {
                "xz on a cell corner"
            } else {
                "xz inside a cell"
            };
            format!("  seed={seed} ({x}, {z}) [{aligned}]: {bad}/{total}, worst {worst} ulp")
        })
        .collect::<Vec<_>>()
        .join("\n")
}
