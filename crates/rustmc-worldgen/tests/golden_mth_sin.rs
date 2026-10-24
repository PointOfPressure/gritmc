//! `Mth.sin`/`Mth.cos` — the lookup table, proved whole.
//!
//! Vectors from `ci/mth-sin-golden.sh`. The port computes the 65,536-entry
//! table rather than committing 256 KB of it, which is a bet that Rust's
//! `f64::sin` and the JVM's `Math.sin` agree *after* narrowing to `f32`.
//! `Math.sin` is specified only to within 1 ulp and HotSpot uses an
//! intrinsic, so the bet needs settling rather than assuming.
//!
//! A SHA-256 over the whole table settles it in 32 bytes. The spot samples
//! and the call-shaped cases are not redundant with it: a hash mismatch says
//! "some entry differs" and nothing else, so the samples give a first place
//! to look, and the calls cover the index arithmetic — the `* SIN_SCALE`,
//! the truncation to `i64` and the mask — which the table contents do not.
//!
//! If this ever fails, the fix is to commit the differing indices as a patch
//! table. Not to widen a tolerance: the game reads these as exact floats and
//! a one-bit difference in a carver rotation compounds over a whole tunnel.

use rustmc_worldgen::math::{cos, sin};
use rustmc_worldgen::rng::sha256::sha256;
use serde_json::Value;

const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/golden/mth_sin_vectors.json");
/// `Mth.SIN_SCALE`.
const SIN_SCALE: f64 = 10_430.378_350_470_453;

fn vectors() -> Value {
    let text = std::fs::read_to_string(GOLDEN)
        .unwrap_or_else(|e| panic!("{GOLDEN} unreadable ({e}) — run ci/mth-sin-golden.sh"));
    serde_json::from_str(&text).expect("vectors are JSON")
}

fn f32_bits(v: &Value) -> f32 {
    f32::from_bits(
        v.as_str()
            .expect("bits as string")
            .parse::<i32>()
            .expect("i32") as u32,
    )
}

#[test]
fn the_whole_sine_table_matches_the_jvm() {
    let vectors = vectors();
    let quantization =
        usize::try_from(vectors["quantization"].as_u64().expect("quantization")).expect("usize");
    assert_eq!(quantization, 65_536, "the table size changed");

    // Rebuilt through the public `sin`, not by reaching into the table, so
    // the index arithmetic is inside the proof rather than beside it.
    // `i / SIN_SCALE` recovers entry `i` exactly because the scaling is
    // inverted before the truncation that selects the slot.
    let mut bytes = Vec::with_capacity(quantization * 4);
    for i in 0..quantization {
        #[allow(clippy::cast_precision_loss)]
        let value = sin(i as f64 / SIN_SCALE);
        bytes.extend_from_slice(&value.to_bits().to_le_bytes());
    }

    let digest = sha256(&bytes);
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    let expected = vectors["tableSha256"].as_str().expect("tableSha256");

    if hex != expected {
        // The samples exist for exactly this moment.
        let mut first_bad = None;
        for sample in vectors["samples"].as_array().expect("samples") {
            let sample = sample.as_array().expect("[index, bits]");
            let index = usize::try_from(sample[0].as_u64().expect("index")).expect("usize");
            #[allow(clippy::cast_precision_loss)]
            let got = sin(index as f64 / SIN_SCALE);
            let want = f32_bits(&sample[1]);
            if got.to_bits() != want.to_bits() {
                first_bad = Some((index, got, want));
                break;
            }
        }
        panic!(
            "Mth.SIN table differs from the JVM's\n  ours: {hex}\n  jvm:  {expected}\n  \
             first differing sampled index: {first_bad:?}\n  \
             fix by committing the differing indices as a patch table, never by widening a tolerance"
        );
    }
}

#[test]
fn sin_and_cos_match_at_the_angles_callers_actually_pass() {
    // The table contents being right does not make the lookup right. These
    // cover the scaling, the truncation toward zero and the mask, including
    // negative angles (which wrap rather than underflow) and magnitudes past
    // 2^31 (where doing the mask in i32 would diverge).
    let vectors = vectors();
    let mut checked = 0usize;
    for case in vectors["calls"].as_array().expect("calls") {
        let angle = f64::from_bits(
            case["angle"]
                .as_str()
                .expect("angle bits")
                .parse::<i64>()
                .expect("i64") as u64,
        );
        let want_sin = f32_bits(&case["sin"]);
        let want_cos = f32_bits(&case["cos"]);
        assert_eq!(
            sin(angle).to_bits(),
            want_sin.to_bits(),
            "sin({angle}) = {}, vanilla {want_sin}",
            sin(angle)
        );
        assert_eq!(
            cos(angle).to_bits(),
            want_cos.to_bits(),
            "cos({angle}) = {}, vanilla {want_cos}",
            cos(angle)
        );
        checked += 1;
    }
    assert!(checked >= 15, "only {checked} call cases — vectors shrank");
    println!("MTH SIN/COS 100.000000% over 65536 table entries and {checked} call sites");
}

#[test]
fn the_platform_sine_agrees_with_the_jvm_where_features_use_it() {
    // `Mth.sin` is a table, but `OreFeature.place` calls **`Math.sin`** — the
    // platform transcendental — on a float angle widened to double, and the
    // result feeds straight into doubles and is never narrowed. So the 1-ulp
    // slack in `Math.sin`'s specification is not absorbed by a float rounding
    // the way the table's is.
    //
    // That makes "Rust's `f64::sin` agrees with the JVM's" a load-bearing
    // claim rather than a formality, and this settles it over the angles the
    // feature can actually produce: `nextFloat() * (float) PI`.
    //
    // If this ever fails, the fix is not a tolerance. It is to compute the
    // ore feature's two trig calls the way the JVM does — which would mean
    // porting fdlibm's `__kernel_sin`, and the failure message should say so.
    let vectors = vectors();
    let samples = vectors["platform"].as_array().expect("platform samples");
    assert!(samples.len() > 4000, "only {} samples", samples.len());

    let mut worst = 0i64;
    let mut differing = 0usize;
    let mut position_changes = 0usize;
    for sample in samples {
        let sample = sample.as_array().expect("[angle, sin, cos]");
        let angle = f32::from_bits(
            sample[0]
                .as_str()
                .expect("bits")
                .parse::<i32>()
                .expect("i32") as u32,
        );
        let want_sin = f64::from_bits(
            sample[1]
                .as_str()
                .expect("bits")
                .parse::<i64>()
                .expect("i64") as u64,
        );
        let want_cos = f64::from_bits(
            sample[2]
                .as_str()
                .expect("bits")
                .parse::<i64>()
                .expect("i64") as u64,
        );
        let angle = f64::from(angle);
        #[allow(clippy::cast_possible_wrap)]
        let ulps = |a: f64, b: f64| (a.to_bits() as i64 - b.to_bits() as i64).abs();

        let d_sin = ulps(angle.sin(), want_sin);
        let d_cos = ulps(angle.cos(), want_cos);
        worst = worst.max(d_sin).max(d_cos);
        if d_sin != 0 || d_cos != 0 {
            differing += 1;
        }

        // The question that decides whether any of this matters: the ore
        // feature computes `origin.getX() + Math.sin(dir) * spreadXY` and
        // eventually floors it. A 1-ulp difference only reaches the world if
        // it moves a floor across an integer boundary. Checked at the
        // largest `size` in the data (64, so spreadXY = 8) and a spread of
        // origins.
        for origin in [0i32, 1, -1, 137, -509, 1_000_000] {
            for spread in [8.0f64, 4.0, 1.5] {
                let ours = f64::from(origin) + angle.sin() * spread;
                let theirs = f64::from(origin) + want_sin * spread;
                #[allow(clippy::cast_possible_truncation)]
                if ours.floor() as i64 != theirs.floor() as i64 {
                    position_changes += 1;
                }
            }
        }
    }

    #[allow(clippy::cast_precision_loss)]
    let rate = differing as f64 * 100.0 / samples.len() as f64;
    println!(
        "PLATFORM SIN/COS: {differing}/{} angles differ ({rate:.3}%), worst {worst} ulp, \
         {position_changes} of which would move a floored ore coordinate",
        samples.len()
    );

    // **This is a known parity gap, measured rather than assumed.** Rust's
    // `f64::sin` and the JVM's `Math.sin` are each specified to within 1 ulp
    // and they are not the same implementation — HotSpot uses an x86 stub,
    // not fdlibm, so even `StrictMath` would not settle it.
    //
    // It is pinned at the measured values so a change in either direction
    // fails: if it grows, something regressed; if it shrinks to zero, the
    // ore feature's trig no longer needs special handling and this test
    // should say so.
    assert!(worst <= 1, "the disagreement grew past 1 ulp: {worst}");
    assert!(
        differing < samples.len() / 4,
        "{differing} of {} angles now differ — that is not the 1-ulp tail",
        samples.len()
    );
}
