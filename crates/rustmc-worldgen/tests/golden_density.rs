//! Golden-vector conformance for the density-function layer (ORACLE.md §5).
//!
//! Each case names a tree by key; this file rebuilds the identical tree with
//! `density::build` and replays vanilla's samples through it. The key-to-tree
//! correspondence is the contract — `tools/goldengen/vanilla/GoldenDump.java`
//! builds the same trees under the same keys, and an unrecognised key fails
//! rather than being skipped.
//!
//! Bounds (`minValue`/`maxValue`) are checked alongside the samples because
//! they are not metadata: `Min`/`Max` short-circuit on them at evaluation
//! time, so a wrong bound is a wrong value somewhere else.

use rustmc_worldgen::density::{
    build, terrain, CubicSpline, DensityFunction, FunctionContext, Mapped, NoiseHolder,
    SplineBuilder,
};
use rustmc_worldgen::noise::{BlendedNoise, NormalNoise};
use rustmc_worldgen::rng::XoroshiroRandomSource;
use serde_json::Value;

const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/golden/density_vectors.json");

/// Must match GoldenDump's constants exactly.
const SEED: i64 = 20260708;
const FIRST_OCTAVE: i32 = -7;
const AMPLITUDES: [f64; 2] = [1.0, 1.0];

fn bits(v: &Value) -> u64 {
    v.as_str()
        .unwrap_or_else(|| panic!("expected bits-as-string, got {v}"))
        .parse::<i64>()
        .unwrap_or_else(|e| panic!("bad i64 {v}: {e}")) as u64
}

/// A bound noise holder — the Rust side of GoldenDump's binding visitor.
/// Every holder gets a fresh source at the same seed.
fn holder() -> NoiseHolder {
    let mut rng = XoroshiroRandomSource::from_seed(SEED);
    NoiseHolder::bound(NormalNoise::create(&mut rng, FIRST_OCTAVE, &AMPLITUDES))
}

fn ramp() -> DensityFunction {
    build::y_clamped_gradient(-64, 320, -1.0, 1.0)
}

fn ramp2() -> DensityFunction {
    build::y_clamped_gradient(0, 128, 2.0, -3.0)
}

/// Rebuilds the tree GoldenDump emitted under `key`.
fn tree(key: &str) -> DensityFunction {
    match key {
        "constant" => build::constant(5.0),
        "y_clamped_gradient" => ramp(),
        "add_const_left" => build::add(build::constant(2.5), ramp()),
        "add_const_right" => build::add(ramp(), build::constant(2.5)),
        "mul_const" => build::mul(ramp(), build::constant(-3.0)),
        "add_two" => build::add(ramp(), ramp2()),
        "mul_two" => build::mul(ramp(), ramp2()),
        "min_two" => build::min(ramp(), ramp2()),
        "max_two" => build::max(ramp(), ramp2()),
        "abs" => build::map(Mapped::Abs, ramp()),
        "square" => build::map(Mapped::Square, ramp()),
        "cube" => build::map(Mapped::Cube, ramp()),
        "half_negative" => build::map(Mapped::HalfNegative, ramp()),
        "quarter_negative" => build::map(Mapped::QuarterNegative, ramp()),
        "invert" => build::map(Mapped::Invert, ramp2()),
        "squeeze" => build::map(Mapped::Squeeze, ramp2()),
        "clamp" => build::clamp(ramp2(), -1.5, 0.5),
        "range_choice" => build::range_choice(ramp(), -0.5, 0.5, build::constant(1.0), ramp2()),
        "noise" => build::noise(holder(), 0.25, 0.5),
        "shift" => build::shift(holder()),
        "shift_a" => build::shift_a(holder()),
        "shift_b" => build::shift_b(holder()),
        // `shiftedNoise2d(shiftX, shiftZ, xzScale, noise)` fixes the y shift
        // to zero and the y scale to 0.0.
        "shifted_noise_2d" => build::shifted_noise(
            build::shift_a(holder()),
            build::constant(0.0),
            build::shift_b(holder()),
            0.25,
            0.0,
            holder(),
        ),
        "old_blended_noise" => {
            let mut rng = XoroshiroRandomSource::from_seed(SEED);
            build::old_blended_noise(BlendedNoise::new(&mut rng, 1.0, 1.0, 80.0, 160.0, 8.0))
        }
        "nested" => build::max(
            build::mul(build::map(Mapped::Square, ramp()), build::constant(0.5)),
            build::min(ramp2(), build::noise(holder(), 1.0, 1.0)),
        ),
        "spline_flat" => build::spline(
            SplineBuilder::new(ramp())
                .point(-1.0, 10.0, 0.0)
                .point(0.0, 20.0, 0.0)
                .point(1.0, 30.0, 0.0)
                .build(),
        ),
        "spline_derivatives" => build::spline(
            SplineBuilder::new(ramp())
                .point(-1.0, 0.0, 2.0)
                .point(-0.2, 1.0, -1.5)
                .point(0.6, -0.5, 0.75)
                .point(1.0, 2.0, 0.0)
                .build(),
        ),
        "spline_extend" => build::spline(SplineBuilder::new(ramp()).point(0.0, 5.0, 2.0).build()),
        "spline_nested" => {
            // The control values are splines over a *different* coordinate —
            // the shape the real terrain shaper uses.
            let inner = || -> CubicSpline {
                SplineBuilder::new(ramp2())
                    .point(-3.0, 0.25, 0.1)
                    .point(2.0, 0.75, -0.1)
                    .build()
            };
            build::spline(
                SplineBuilder::new(ramp())
                    .spline_point(-1.0, inner(), 0.0)
                    .point(0.3, 1.5, 0.5)
                    .spline_point(1.0, inner(), 0.0)
                    .build(),
            )
        }
        "end_islands_0" => build::end_islands(0),
        "end_islands_s" => build::end_islands(20260708),
        "blend_alpha" => build::blend_alpha(),
        "blend_offset" => build::blend_offset(),
        "marker_interpolated" => build::interpolated(ramp()),
        "marker_flat_cache" => build::flat_cache(ramp()),
        "marker_cache_2d" => build::cache_2d(ramp()),
        "marker_cache_once" => build::cache_once(&mut 0, ramp()),
        "marker_cache_all_in_cell" => build::cache_all_in_cell(ramp()),
        "marker_blend_density" => build::blend_density(ramp()),
        "lerp_constant_first" => build::lerp(&mut 0, ramp(), build::constant(0.25), ramp2()),
        "lerp_general" => build::lerp(&mut 0, build::blend_alpha(), build::blend_offset(), ramp()),
        "interval_select" => build::interval_select(
            ramp(),
            vec![-0.5, 0.0, 0.5],
            vec![
                build::constant(-2.0),
                build::constant(-1.0),
                build::constant(1.0),
                build::constant(2.0),
            ],
        ),
        "rarity_3d" => build::map(
            Mapped::Abs,
            build::interval_select(
                ramp(),
                vec![-0.5, 0.0, 0.5],
                vec![
                    build::noise_for_rarity(holder(), 0.75),
                    build::noise_for_rarity(holder(), 1.0),
                    build::noise_for_rarity(holder(), 1.5),
                    build::noise_for_rarity(holder(), 2.0),
                ],
            ),
        ),
        key if key.starts_with("slide") || key.starts_with("post_process") => {
            let amplified = key.ends_with("_amplified");
            let slid = build::slide_overworld(amplified, ramp());
            if key.starts_with("post_process") {
                build::post_process(slid)
            } else {
                slid
            }
        }
        key if key.starts_with("terrain_") => {
            let amplified = key.ends_with("_amplified");
            let continents = build::y_clamped_gradient(-64, 320, -1.0, 1.0);
            let erosion = build::y_clamped_gradient(-64, 320, 1.0, -1.0);
            let ridges = build::y_clamped_gradient(0, 256, -1.0, 1.0);
            let weirdness = build::y_clamped_gradient(-64, 128, -1.0, 1.0);
            let spline = if key.starts_with("terrain_offset") {
                terrain::overworld_offset(&continents, &erosion, &ridges, amplified)
            } else if key.starts_with("terrain_factor") {
                terrain::overworld_factor(&continents, &erosion, &weirdness, &ridges, amplified)
            } else {
                terrain::overworld_jaggedness(&continents, &erosion, &weirdness, &ridges, amplified)
            };
            build::spline(spline)
        }
        other => panic!("unknown density tree key {other:?} — add it here, never skip it"),
    }
}

#[test]
fn density_vectors_match_vanilla() {
    let text = std::fs::read_to_string(GOLDEN)
        .unwrap_or_else(|e| panic!("golden vector {GOLDEN} unreadable: {e}"));
    let doc: Value =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{GOLDEN} is not JSON: {e}"));
    let cases = doc["cases"].as_array().expect("cases");

    for case in cases {
        let key = case["case"].as_str().expect("case key");
        let f = tree(key);

        assert_eq!(
            f.min_value().to_bits(),
            bits(&case["minValue_bits"]),
            "{key}: min_value diverged (expected {}, got {})",
            f64::from_bits(bits(&case["minValue_bits"])),
            f.min_value()
        );
        assert_eq!(
            f.max_value().to_bits(),
            bits(&case["maxValue_bits"]),
            "{key}: max_value diverged (expected {}, got {})",
            f64::from_bits(bits(&case["maxValue_bits"])),
            f.max_value()
        );

        let expected = case["output_bits"].as_array().expect("output_bits");
        let points = case["blockPoints"].as_array().expect("blockPoints");
        for (index, (point, want)) in points.iter().zip(expected).enumerate() {
            let p = point.as_array().expect("[x,y,z]");
            let ctx = FunctionContext::new(
                p[0].as_i64().expect("x") as i32,
                p[1].as_i64().expect("y") as i32,
                p[2].as_i64().expect("z") as i32,
            );
            let got = f.compute(ctx);
            let want_bits = bits(want);
            assert_eq!(
                got.to_bits(),
                want_bits,
                "{key}: diverged at point {index} ({}, {}, {})\n  expected {} (0x{want_bits:016x})\n  \
                 got      {got} (0x{:016x})",
                ctx.block_x,
                ctx.block_y,
                ctx.block_z,
                f64::from_bits(want_bits),
                got.to_bits()
            );
        }
    }

    assert!(
        cases.len() >= 53,
        "expected the full tree set, saw {}",
        cases.len()
    );
}

/// `fill_array` must agree with per-point `compute` — the batched path is an
/// optimization surface, and this is the invariant that keeps it honest when
/// per-operator overrides land later.
#[test]
fn fill_array_agrees_with_compute() {
    let f = tree("nested");
    let contexts: Vec<FunctionContext> = (-4..4)
        .map(|i| FunctionContext::new(i, i * 16, -i))
        .collect();
    let mut output = vec![0.0; contexts.len()];
    f.fill_array(&mut output, &contexts);

    for (slot, ctx) in output.iter().zip(&contexts) {
        assert_eq!(slot.to_bits(), f.compute(*ctx).to_bits());
    }
}
