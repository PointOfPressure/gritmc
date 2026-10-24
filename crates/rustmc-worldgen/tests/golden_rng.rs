//! Golden-vector conformance for the RNG layer (ORACLE.md §5 rule 1, G1).
//!
//! Replays streams exported from the JVM oracle (`golden/`, provenance in
//! its README) against the Rust ports. These are not "does it look random"
//! tests — every value must match bit-for-bit, because everything above
//! this layer is a pure function of these streams.
//!
//! Failures print the first divergent index and both values in hex: a
//! mismatch at index 0 means the seeding pipeline is wrong, while a
//! mismatch deep in a stream means the state update is wrong. That
//! distinction is most of the debugging.

use rustmc_worldgen::rng::{
    block_seed, mix_stafford13, LegacyRandomSource, PositionalRandomFactory, RandomSource,
    WorldgenBacking, WorldgenRandom, XoroshiroRandomSource,
};
use serde_json::Value;

const GOLDEN_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/golden");

fn load(name: &str) -> Value {
    let path = format!("{GOLDEN_DIR}/{name}");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("golden vector {path} unreadable: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("golden vector {path} is not JSON: {e}"))
}

/// i64s are JSON strings — a JSON number cannot hold the full range.
fn as_i64(v: &Value) -> i64 {
    v.as_str()
        .unwrap_or_else(|| panic!("expected an i64-as-string, got {v}"))
        .parse()
        .unwrap_or_else(|e| panic!("bad i64 {v}: {e}"))
}

fn as_i64s(v: &Value) -> Vec<i64> {
    v.as_array()
        .expect("expected an array")
        .iter()
        .map(as_i64)
        .collect()
}

fn as_i32(v: &Value) -> i32 {
    i32::try_from(
        v.as_i64()
            .unwrap_or_else(|| panic!("expected an int, got {v}")),
    )
    .expect("i32")
}

#[track_caller]
fn assert_stream(label: &str, expected: &[i64], mut next: impl FnMut() -> i64) {
    for (index, &want) in expected.iter().enumerate() {
        let got = next();
        assert_eq!(
            got, want,
            "{label}: diverged at draw {index}\n  expected {want} (0x{want:016x})\n  got      {got} (0x{got:016x})"
        );
    }
}

// ---------------------------------------------------------------------------
// Raw streams: 10,000 nextLong draws per seed.
// ---------------------------------------------------------------------------

const STREAM_SEEDS: [&str; 5] = ["0", "1", "m1", "123456789", "20260708"];

#[test]
fn xoroshiro_next_long_streams() {
    for suffix in STREAM_SEEDS {
        let doc = load(&format!("xoroshiro128pp_seed_{suffix}.json"));
        let seed = as_i64(&doc["seed"]);
        let expected = as_i64s(&doc["outputs"]);
        assert_eq!(expected.len(), 10_000, "expected a 10k stream");

        let mut rng = XoroshiroRandomSource::from_seed(seed);
        assert_stream(&format!("xoroshiro seed {seed}"), &expected, || {
            rng.next_i64()
        });
    }
}

#[test]
fn legacy_next_long_streams() {
    for suffix in STREAM_SEEDS {
        let doc = load(&format!("legacy_lcg_seed_{suffix}.json"));
        let seed = as_i64(&doc["seed"]);
        let expected = as_i64s(&doc["outputs"]);
        assert_eq!(expected.len(), 10_000, "expected a 10k stream");

        let mut rng = LegacyRandomSource::new(seed);
        assert_stream(&format!("legacy seed {seed}"), &expected, || rng.next_i64());
    }
}

// ---------------------------------------------------------------------------
// Derived operations.
//
// Each case is a fresh source seeded from the case's `seed`, then 100 draws
// of the named operation.
// ---------------------------------------------------------------------------

/// Checks the per-op cases shared by all three sources. Returns whether the
/// case name was recognised, so the callers can assert full coverage — a
/// silently skipped case is a test that proves nothing.
fn check_common_op<R: RandomSource>(
    case: &Value,
    name: &str,
    mut build: impl FnMut(i64) -> R,
) -> bool {
    let seed_of = |case: &Value| as_i64(&case["seed"]);

    match name {
        "nextInt.bounded" => {
            let bound = as_i32(&case["bound"]);
            let expected = as_i64s(&case["outputs"]);
            let mut rng = build(seed_of(case));
            assert_stream(&format!("nextInt({bound})"), &expected, || {
                i64::from(rng.next_i32_bound(bound))
            });
        }
        "nextIntBetweenInclusive" => {
            let min = as_i32(&case["min"]);
            let max = as_i32(&case["max"]);
            let expected = as_i64s(&case["outputs"]);
            let mut rng = build(seed_of(case));
            assert_stream(
                &format!("nextIntBetweenInclusive({min},{max})"),
                &expected,
                || i64::from(rng.next_i32_between_inclusive(min, max)),
            );
        }
        "nextDouble" => {
            // Compare IEEE-754 bits, never the decimal repr.
            let expected = as_i64s(&case["output_bits"]);
            let mut rng = build(seed_of(case));
            assert_stream("nextDouble (bits)", &expected, || {
                rng.next_f64().to_bits() as i64
            });
        }
        "nextFloat" => {
            let expected = as_i64s(&case["output_bits"]);
            let mut rng = build(seed_of(case));
            assert_stream("nextFloat (bits)", &expected, || {
                i64::from(rng.next_f32().to_bits() as i32)
            });
        }
        "nextBoolean" => {
            let expected: Vec<bool> = case["outputs"]
                .as_array()
                .expect("outputs")
                .iter()
                .map(|v| v.as_bool().expect("bool"))
                .collect();
            let mut rng = build(seed_of(case));
            for (index, &want) in expected.iter().enumerate() {
                assert_eq!(
                    rng.next_bool(),
                    want,
                    "nextBoolean: diverged at draw {index}"
                );
            }
        }
        _ => return false,
    }
    true
}

/// The seeding-ladder cases, shared by the two `WorldgenRandom` flavours.
fn check_worldgen_op<R: WorldgenBacking>(
    case: &Value,
    name: &str,
    mut build: impl FnMut(i64) -> WorldgenRandom<R>,
) -> bool {
    match name {
        "setDecorationSeed" => {
            let world_seed = as_i64(&case["worldSeed"]);
            // Block coordinates here — see WorldgenRandom::set_decoration_seed.
            let min_block_x = as_i32(&case["minBlockX"]);
            let min_block_z = as_i32(&case["minBlockZ"]);
            let expected_seed = as_i64(&case["decorationSeed"]);
            let expected = as_i64s(&case["outputs"]);

            let mut rng = build(0);
            let derived = rng.set_decoration_seed(world_seed, min_block_x, min_block_z);
            assert_eq!(
                derived, expected_seed,
                "setDecorationSeed({world_seed}, {min_block_x}, {min_block_z}) returned the wrong seed"
            );
            assert_stream("setDecorationSeed stream", &expected, || rng.next_i64());
        }
        "setFeatureSeed" => {
            let decoration_seed = as_i64(&case["decorationSeed"]);
            let index = as_i32(&case["index"]);
            let step = as_i32(&case["step"]);
            let expected = as_i64s(&case["outputs"]);

            let mut rng = build(0);
            rng.set_feature_seed(decoration_seed, index, step);
            assert_stream("setFeatureSeed stream", &expected, || rng.next_i64());
        }
        "setLargeFeatureSeed" => {
            let world_seed = as_i64(&case["worldSeed"]);
            // Chunk coordinates here, unlike setDecorationSeed.
            let chunk_x = as_i32(&case["chunkX"]);
            let chunk_z = as_i32(&case["chunkZ"]);
            let expected = as_i64s(&case["outputs"]);

            let mut rng = build(0);
            rng.set_large_feature_seed(world_seed, chunk_x, chunk_z);
            assert_stream("setLargeFeatureSeed stream", &expected, || rng.next_i64());
        }
        "setLargeFeatureWithSalt" => {
            let world_seed = as_i64(&case["worldSeed"]);
            let x = as_i32(&case["x"]);
            let z = as_i32(&case["z"]);
            let salt = as_i32(&case["salt"]);
            let expected = as_i64s(&case["outputs"]);

            let mut rng = build(0);
            rng.set_large_feature_with_salt(world_seed, x, z, salt);
            assert_stream("setLargeFeatureWithSalt stream", &expected, || {
                rng.next_i64()
            });
        }
        _ => return false,
    }
    true
}

#[test]
fn xoroshiro_derived_vectors() {
    let doc = load("xoroshiro128pp_derived.json");
    let mut seen = 0usize;

    for case in doc["cases"].as_array().expect("cases") {
        let name = case["case"].as_str().expect("case name");
        seen += 1;

        match name {
            "mixStafford13" => {
                let inputs = as_i64s(&case["inputs"]);
                let outputs = as_i64s(&case["outputs"]);
                for (input, want) in inputs.iter().zip(&outputs) {
                    assert_eq!(mix_stafford13(*input), *want, "mixStafford13({input})");
                }
            }
            "blockSeed" => {
                let outputs = as_i64s(&case["outputs"]);
                for (coords, want) in case["inputs"]
                    .as_array()
                    .expect("inputs")
                    .iter()
                    .zip(&outputs)
                {
                    let c = coords.as_array().expect("[x,y,z]");
                    let (x, y, z) = (as_i32(&c[0]), as_i32(&c[1]), as_i32(&c[2]));
                    assert_eq!(block_seed(x, y, z), *want, "blockSeed({x},{y},{z})");
                }
            }
            "fork" => {
                let expected = as_i64s(&case["outputs"]);
                let mut child = XoroshiroRandomSource::from_seed(as_i64(&case["seed"])).fork();
                assert_stream("fork", &expected, || child.next_i64());
            }
            "forkPositional.at" => {
                let (x, y, z) = (as_i32(&case["x"]), as_i32(&case["y"]), as_i32(&case["z"]));
                let expected = as_i64s(&case["outputs"]);
                let factory =
                    XoroshiroRandomSource::from_seed(as_i64(&case["seed"])).fork_positional();
                let mut child = factory.at(x, y, z);
                assert_stream(
                    &format!("forkPositional.at({x},{y},{z})"),
                    &expected,
                    || child.next_i64(),
                );
            }
            "forkPositional.fromHashOf" => {
                let name = case["name"].as_str().expect("name");
                let expected = as_i64s(&case["outputs"]);
                let factory =
                    XoroshiroRandomSource::from_seed(as_i64(&case["seed"])).fork_positional();
                let mut child = factory.from_hash_of(name);
                assert_stream(
                    &format!("forkPositional.fromHashOf({name})"),
                    &expected,
                    || child.next_i64(),
                );
            }
            "forkPositional.fromSeed" => {
                let from_seed = as_i64(&case["fromSeed"]);
                let expected = as_i64s(&case["outputs"]);
                let factory =
                    XoroshiroRandomSource::from_seed(as_i64(&case["seed"])).fork_positional();
                let mut child = factory.from_seed(from_seed);
                assert_stream(
                    &format!("forkPositional.fromSeed({from_seed})"),
                    &expected,
                    || child.next_i64(),
                );
            }
            other => assert!(
                check_common_op(case, other, XoroshiroRandomSource::from_seed),
                "unhandled xoroshiro case {other:?} — a skipped case proves nothing"
            ),
        }
    }

    assert!(
        seen >= 20,
        "expected the full xoroshiro case set, saw {seen}"
    );
}

#[test]
fn legacy_derived_vectors() {
    let doc = load("legacy_lcg_derived.json");
    let mut seen = 0usize;

    for case in doc["cases"].as_array().expect("cases") {
        let name = case["case"].as_str().expect("case name");
        seen += 1;

        let handled = check_worldgen_op(case, name, |seed| {
            WorldgenRandom::new(LegacyRandomSource::new(seed))
        }) || check_common_op(case, name, LegacyRandomSource::new);

        assert!(handled, "unhandled legacy case {name:?}");
    }

    assert!(seen >= 18, "expected the full legacy case set, saw {seen}");
}

#[test]
fn xoroshiro_worldgen_random_derived_vectors() {
    let doc = load("xwr_derived.json");
    let mut seen = 0usize;

    for case in doc["cases"].as_array().expect("cases") {
        let name = case["case"].as_str().expect("case name");
        seen += 1;

        let handled = check_worldgen_op(case, name, |seed| {
            WorldgenRandom::new(XoroshiroRandomSource::from_seed(seed))
        }) || check_common_op(case, name, |seed| {
            WorldgenRandom::new(XoroshiroRandomSource::from_seed(seed))
        });

        assert!(handled, "unhandled xwr case {name:?}");
    }

    assert!(seen >= 15, "expected the full xwr case set, saw {seen}");
}

/// `WorldgenRandom` over Xoroshiro must take `BitRandomSource`'s
/// `nextFloat` — which reads the same 24 high bits of the same first draw as
/// the raw source, so the two agree — while `nextDouble` costs two draws on
/// one and one draw on the other, so they must *disagree*. Both directions
/// are pinned because getting either backwards is a silent, plausible bug.
#[test]
fn worldgen_random_float_agrees_but_double_diverges() {
    let seed = 20260708;

    let raw_float = XoroshiroRandomSource::from_seed(seed).next_f32();
    let wrapped_float = WorldgenRandom::new(XoroshiroRandomSource::from_seed(seed)).next_f32();
    assert_eq!(raw_float.to_bits(), wrapped_float.to_bits());

    let raw_double = XoroshiroRandomSource::from_seed(seed).next_f64();
    let wrapped_double = WorldgenRandom::new(XoroshiroRandomSource::from_seed(seed)).next_f64();
    assert_ne!(raw_double.to_bits(), wrapped_double.to_bits());
}
