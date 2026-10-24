//! The transcendentals the fancy tree placer calls, against the JVM.
//!
//! Vectors: `crates/rustmc-worldgen/golden/fancy_math_vectors.json`, from
//! `ci/fancy-math-golden.sh`.
//!
//! # Why this is a gate before the placer is written
//!
//! `FancyTrunkPlacer` is the only worldgen code that reaches for `Math.sin`,
//! `Math.cos`, `Math.pow` and `Math.sqrt` on values it computed itself, and
//! then **floors** the result into a block coordinate. Everything else uses
//! `Mth.sin`, a lookup table this port already matches bit for bit.
//!
//! ORACLE.md §5c measured `Math.sin`/`cos` at the angles the *ore* feature
//! produces — 22 of 4097 differed by one ulp, none of which moved a floored
//! coordinate. That conclusion does not carry over for free: the fancy
//! placer's angles are a different set, and its multipliers are larger, so a
//! one-ulp difference has further to travel before the floor.
//!
//! This test reports rather than only asserting. A difference here is not
//! automatically a failure — it is a *measurement* that says how much of the
//! fancy oak's shape is at the mercy of libm, and it wants to exist before
//! the placer does rather than after it produces a mystery.
//!
//! What is asserted outright is `sqrt`: IEEE 754 requires it to be correctly
//! rounded, so any difference at all is a bug in this port or in the harness,
//! never in the platform.

use std::path::Path;

const VECTORS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/golden/fancy_math_vectors.json"
);

#[test]
fn platform_math_matches_the_jvm_where_it_must() {
    let path = Path::new(VECTORS);
    if !rustmc_data::fixture::available(
        path,
        "fancy-math extract",
        "the golden generator named in this file's module docs",
    ) {
        return;
    }
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("readable")).expect("json");

    // sqrt is correctly rounded by the standard. No tolerance, no report.
    let mut sqrt_cases = 0usize;
    for row in doc["sqrt"].as_array().expect("sqrt") {
        let v = f64::from_bits(bits(&row["v"]));
        let expected = f64::from_bits(bits(&row["sqrt"]));
        assert_eq!(
            v.sqrt().to_bits(),
            expected.to_bits(),
            "sqrt({v}) differs from the JVM's, which IEEE 754 forbids — \
             the bug is here or in the harness, not in libm"
        );
        sqrt_cases += 1;
    }

    // `Math.pow(x, 2.0)`. Not required to be exact, but for an exponent of
    // two every sane implementation is a single multiply.
    let mut pow_cases = 0usize;
    let mut pow_differ = 0usize;
    for row in doc["pow"].as_array().expect("pow") {
        let x = f64::from_bits(bits(&row["x"]));
        let expected = f64::from_bits(bits(&row["pow"]));
        if x.powf(2.0).to_bits() != expected.to_bits() {
            pow_differ += 1;
        }
        pow_cases += 1;
    }

    // sin and cos at the *float* angles the placer produces.
    let mut trig_cases = 0usize;
    let mut sin_differ = 0usize;
    let mut cos_differ = 0usize;
    let mut worst_ulps = 0i64;
    for row in doc["trig"].as_array().expect("trig") {
        let angle = f64::from_bits(bits(&row["bits"]));
        let want_sin = f64::from_bits(bits(&row["sin"]));
        let want_cos = f64::from_bits(bits(&row["cos"]));
        let got_sin = angle.sin();
        let got_cos = angle.cos();
        if got_sin.to_bits() != want_sin.to_bits() {
            sin_differ += 1;
            worst_ulps = worst_ulps.max(ulps(got_sin, want_sin));
        }
        if got_cos.to_bits() != want_cos.to_bits() {
            cos_differ += 1;
            worst_ulps = worst_ulps.max(ulps(got_cos, want_cos));
        }
        trig_cases += 1;
    }

    println!(
        "FANCY MATH: sqrt {sqrt_cases}/{sqrt_cases} exact; \
         pow {}/{pow_cases} exact; \
         sin {}/{trig_cases}, cos {}/{trig_cases} exact, worst {worst_ulps} ulp",
        pow_cases - pow_differ,
        trig_cases - sin_differ,
        trig_cases - cos_differ,
    );

    // The standing claim, and the thing that would have to be revisited: a
    // difference is at most one ulp. More than that is not a libm quirk, it
    // is a different algorithm, and the placer could not be ported without
    // reimplementing it.
    assert!(
        worst_ulps <= 1,
        "the platform disagrees with the JVM by {worst_ulps} ulp — too far to \
         floor away, and the fancy placer cannot be ported against it"
    );
}

fn bits(value: &serde_json::Value) -> u64 {
    #[expect(
        clippy::cast_sign_loss,
        reason = "the harness writes raw bits through a signed Java long"
    )]
    let raw = value.as_i64().expect("a bit pattern") as u64;
    raw
}

/// The distance between two doubles in representable steps.
fn ulps(a: f64, b: f64) -> i64 {
    #[expect(
        clippy::cast_possible_wrap,
        reason = "both are finite and near each other; the difference fits"
    )]
    let (x, y) = (a.to_bits() as i64, b.to_bits() as i64);
    (x - y).abs()
}
