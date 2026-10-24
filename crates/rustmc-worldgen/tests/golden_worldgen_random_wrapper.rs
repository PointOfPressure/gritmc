//! `WorldgenRandom.nextLong()` over a Xoroshiro source — the composition, not
//! the generator.
//!
//! # Why this exists
//!
//! `golden/xoroshiro128pp_seed_*.json` pin `XoroshiroRandomSource.nextLong()`
//! for five seeds. Nothing pinned the **wrapper**, and the wrapper is what
//! every feature draws from: `WorldgenRandom extends LegacyRandomSource
//! implements BitRandomSource`, so its `nextLong()` is
//!
//! ```text
//!   ((long) next(32) << 32) + next(32)
//! ```
//!
//! with `XoroshiroRandomSource.next(bits)` being
//! `(int)(nextLong() >>> (64 - bits))`. One wrapper `nextLong()` therefore
//! consumes **two** generator draws and keeps only the top 32 bits of each.
//!
//! That gap cost most of a day. An independently written Xoroshiro, verified
//! against all five goldens, still disagreed with this port about a chunk's
//! population seed — because it called the generator's `nextLong()` once per
//! value where the wrapper calls `next(32)` twice. The disagreement looked for
//! several hours like evidence that the population seed was wrong in both the
//! port and `DecorateGolden`.
//!
//! The port already implements the composition correctly, so this passes on
//! the first run. That is the point of it: it pins behaviour no existing
//! vector pins, so the next reimplementation does not have to rediscover it.
//!
//! # The expected values
//!
//! Derived from the composition applied to `xoroshiro128pp_seed_0.json`'s own
//! outputs — vanilla-produced numbers, recombined by the documented rule
//! rather than by running this port. A vector taken from the thing it is
//! meant to check would prove nothing.

use rustmc_worldgen::rng::xoroshiro::XoroshiroRandomSource;
use rustmc_worldgen::rng::{RandomSource, WorldgenRandom};

/// `(int)(v >>> (64 - 32))` — the top 32 bits, as a signed int.
fn next32(v: i64) -> i64 {
    i64::from(((v as u64) >> 32) as u32 as i32)
}

#[test]
fn the_wrapper_composes_two_generator_draws_per_long() {
    let golden = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/golden/xoroshiro128pp_seed_0.json"
    ))
    .expect("the seed-0 xoroshiro golden");
    let doc: serde_json::Value = serde_json::from_str(&golden).expect("json");
    let raw: Vec<i64> = doc["outputs"]
        .as_array()
        .expect("outputs")
        .iter()
        .map(|v| v.as_str().expect("string").parse().expect("i64"))
        .collect();
    let seed: i64 = doc["seed"].as_str().map_or_else(
        || doc["seed"].as_i64().expect("seed"),
        |s| s.parse().expect("i64"),
    );

    // The wrapper's Nth long, built from vanilla's own raw outputs.
    let expected: Vec<i64> = raw
        .chunks_exact(2)
        .take(16)
        .map(|pair| (next32(pair[0]) << 32).wrapping_add(next32(pair[1])))
        .collect();

    let mut random = WorldgenRandom::new(XoroshiroRandomSource::from_seed(0));
    random.set_seed(seed);
    let got: Vec<i64> = (0..expected.len()).map(|_| random.next_i64()).collect();

    assert_eq!(
        got, expected,
        "`WorldgenRandom.nextLong()` must consume two generator draws and keep \
         the top 32 bits of each — not call the source's `nextLong()` once"
    );
}
