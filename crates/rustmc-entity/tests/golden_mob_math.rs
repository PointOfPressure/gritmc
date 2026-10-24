//! `Mth.atan2` and the head-turn helpers against the JVM.
//!
//! Vectors: `crates/rustmc-entity/golden/mob_math_vectors.json`, from
//! `ci/mob-math-golden.sh`.
//!
//! Everything here is bit-exact or the test fails. The one place that can
//! legitimately go red on a new platform is the pair of 257-entry tables,
//! built with the platform's `asin`/`cos` against the JVM's fdlibm; if it
//! does, the fix is to embed the JVM's bits, not to widen the tolerance.

use std::path::Path;

use rustmc_entity::ai::goal::positive_ceil_div;
use rustmc_entity::mth::{
    approach_degrees, atan2, degrees_difference, rotate_if_necessary, sqrt_f32, wrap_degrees,
    ATAN2_TABLES, FRAC_BIAS,
};

const VECTORS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/golden/mob_math_vectors.json");

fn bits64(v: &serde_json::Value) -> u64 {
    v.as_i64().expect("i64 bits") as u64
}

fn bits32(v: &serde_json::Value) -> u32 {
    v.as_i64().expect("i64 bits") as i32 as u32
}

#[test]
fn mob_math_matches_the_jvm_bit_for_bit() {
    let path = Path::new(VECTORS);
    if !rustmc_data::fixture::available(path, "mob-math extract", "ci/mob-math-golden.sh") {
        return;
    }
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("readable")).expect("json");

    assert_eq!(FRAC_BIAS.to_bits(), bits64(&doc["frac_bias"]));

    let asin = doc["asin_tab"].as_array().expect("asin_tab");
    let cos = doc["cos_tab"].as_array().expect("cos_tab");
    assert_eq!(asin.len(), 257);
    assert_eq!(cos.len(), 257);
    let tables = &*ATAN2_TABLES;
    for i in 0..257 {
        assert_eq!(
            tables.asin[i].to_bits(),
            bits64(&asin[i]),
            "ASIN_TAB[{i}] differs from the JVM's Math.asin({i}/256)"
        );
        assert_eq!(
            tables.cos[i].to_bits(),
            bits64(&cos[i]),
            "COS_TAB[{i}] differs from the JVM's Math.cos(asin({i}/256))"
        );
    }

    let mut n = 0;
    for row in doc["atan2"].as_array().expect("atan2") {
        let y = f64::from_bits(bits64(&row[0]));
        let x = f64::from_bits(bits64(&row[1]));
        let want = f64::from_bits(bits64(&row[2]));
        let got = atan2(y, x);
        assert_eq!(
            got.to_bits(),
            want.to_bits(),
            "Mth.atan2({y}, {x}): got {got}, jar {want}"
        );
        n += 1;
    }
    assert!(n >= 2000, "only {n} atan2 vectors");

    for row in doc["wrap"].as_array().expect("wrap") {
        let f = f32::from_bits(bits32(&row[0]));
        let want = f32::from_bits(bits32(&row[1]));
        assert_eq!(
            wrap_degrees(f).to_bits(),
            want.to_bits(),
            "wrapDegrees({f})"
        );
    }
    for row in doc["rotate"].as_array().expect("rotate") {
        let a = f32::from_bits(bits32(&row[0]));
        let b = f32::from_bits(bits32(&row[1]));
        let c = f32::from_bits(bits32(&row[2]));
        let want = f32::from_bits(bits32(&row[3]));
        assert_eq!(
            rotate_if_necessary(a, b, c).to_bits(),
            want.to_bits(),
            "rotateIfNecessary({a}, {b}, {c})"
        );
    }
    for row in doc["approach"].as_array().expect("approach") {
        let a = f32::from_bits(bits32(&row[0]));
        let b = f32::from_bits(bits32(&row[1]));
        let c = f32::from_bits(bits32(&row[2]));
        let want = f32::from_bits(bits32(&row[3]));
        assert_eq!(
            approach_degrees(a, b, c).to_bits(),
            want.to_bits(),
            "approachDegrees({a}, {b}, {c})"
        );
    }
    for row in doc["diff"].as_array().expect("diff") {
        let a = f32::from_bits(bits32(&row[0]));
        let b = f32::from_bits(bits32(&row[1]));
        let want = f32::from_bits(bits32(&row[2]));
        assert_eq!(
            degrees_difference(a, b).to_bits(),
            want.to_bits(),
            "degreesDifference({a}, {b})"
        );
    }
    for row in doc["ceil_div"].as_array().expect("ceil_div") {
        let n = row[0].as_i64().expect("n") as i32;
        let want = row[1].as_i64().expect("want") as i32;
        assert_eq!(positive_ceil_div(n, 2), want, "positiveCeilDiv({n}, 2)");
    }
    for row in doc["sqrt_f"].as_array().expect("sqrt_f") {
        let f = f32::from_bits(bits32(&row[0]));
        let want = f32::from_bits(bits32(&row[1]));
        assert_eq!(sqrt_f32(f).to_bits(), want.to_bits(), "Mth.sqrt({f})");
    }
}
