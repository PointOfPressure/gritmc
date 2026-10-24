//! The two `nextLong() | 1` values inside `setDecorationSeed`.
//!
//! An independently written Xoroshiro — verified against
//! `golden/xoroshiro128pp_seed_20260708.json`, which is vanilla-produced —
//! computes a different population seed for chunk (63,67) at seed 20260722
//! than both this port and `DecorateGolden`, which agree with each other:
//!
//! ```text
//!   referee  -2863772679376894862
//!   both     -4746405627041933582
//! ```
//!
//! Two implementations agreeing against one is normally the one being wrong,
//! but here the two share an author and the one is checked against a golden,
//! so the usual inference does not apply. What separates the cases is *where*
//! they diverge: the golden verifies `nextLong` from construction and nothing
//! after it, so if `a` and `b` agree the fault is in the referee's final
//! multiply-and-xor, and if they disagree it is in the reseed — which the port
//! and the dumper compute the same way and could therefore both have wrong.
//!
//! This prints them. It asserts nothing, because the point is the comparison
//! and not a verdict written in advance.

use rustmc_worldgen::rng::xoroshiro::XoroshiroRandomSource;
use rustmc_worldgen::rng::{RandomSource, WorldgenRandom};

#[test]
fn the_two_scales_inside_set_decoration_seed() {
    const WORLD_SEED: i64 = 20_260_722;
    const BLOCK_X: i32 = 1008;
    const BLOCK_Z: i32 = 1072;

    let mut random = WorldgenRandom::new(XoroshiroRandomSource::from_seed(0));
    random.set_seed(WORLD_SEED);
    let a = random.next_i64() | 1;
    let b = random.next_i64() | 1;
    println!("port    a = {a}");
    println!("port    b = {b}");
    println!("referee a = 6264548328051964541");
    println!("referee b = -1126660801248313201");

    let seed = i64::from(BLOCK_X)
        .wrapping_mul(a)
        .wrapping_add(i64::from(BLOCK_Z).wrapping_mul(b))
        ^ WORLD_SEED;
    println!("port    populationSeed = {seed}");
    println!("vectors populationSeed = -4746405627041933582");
}
