//! Which feature index would make the sulfur cluster do what a *server* does.
//!
//! # The question
//!
//! At chunk (63,67), seed 20260722, three `sulfur_spike[thickness=tip_merge]`
//! blocks differ from two independently generated server worlds, which both
//! hold `tip`. Everything upstream has been cleared by measurement:
//!
//!   * the pre-decoration column at (1016,·,1077) is **identical** between the
//!     dumper's world and a server's, block for block;
//!   * the port and `DecorateGolden` agree on **all 925 draws** of that
//!     chunk's `sulfur_spike_cluster`, verified against a labelled slice;
//!   * twelve pieces of the feature were read against the bytecode and matched,
//!     including the merge rule, the overlap split and both height branches;
//!   * nothing in generation can reshape a placed spike — the writes use flag
//!     2, speleothems are never marked for post-processing, and the reference
//!     server runs under `tick freeze`.
//!
//! So two harnesses agree with each other and both differ from two servers.
//! The remaining input they share is the feature *stream*:
//! `setFeatureSeed(populationSeed, index, step)` with `index = 2`, `step = 7`.
//! `index` is the feature's position in its step's list, which is built from
//! the biome source's possible set — a thing both harnesses compute the same
//! way and a server need not.
//!
//! # What this proves and what it does not
//!
//! The server's column is stalactite 2, stalagmite 0. This sweep asks whether
//! any index in range makes the cluster's *first* two body draws — the radii —
//! come out such that a server's answer becomes reachable. It is deliberately
//! a cheap arithmetic probe, not a full replay: a hit says the ordering is
//! worth doubting, a miss says the stream is not the difference and the
//! population seed is.
//!
//! Modelled on `probe_disk_origin.rs`, which asked the same question of the
//! sand/gravel family and answered it *no* — the cause there was the biome
//! container. A second no would be just as informative.

use rustmc_worldgen::rng::xoroshiro::XoroshiroRandomSource;
use rustmc_worldgen::rng::{RandomSource, WorldgenRandom};

/// From the vectors for chunk (63,67).
const POPULATION_SEED: i64 = -4_746_405_627_041_933_582;
const STEP: i32 = 7;
/// The index both harnesses use.
const HARNESS_INDEX: i32 = 2;

/// The cluster's first five draws: height, wetness, density, xRadius, zRadius.
///
/// Only the two radii matter for the question — they decide which columns
/// exist and therefore whether (1016,1077) is reachable at all.
fn radii(index: i32) -> (i32, i32) {
    let mut random = WorldgenRandom::new(XoroshiroRandomSource::from_seed(0));
    random.set_feature_seed(POPULATION_SEED, index, STEP);
    // height: uniform 1..=4
    let _height = random.next_i32_bound(4) + 1;
    // wetness: a constant 0.0 float provider draws nothing.
    // density: uniform 0.3..0.7 — one float.
    let _density = random.next_f32();
    let x = random.next_i32_bound(7) + 2;
    let z = random.next_i32_bound(7) + 2;
    (x, z)
}

#[test]
fn which_feature_index_gives_the_server_its_column() {
    let control = radii(HARNESS_INDEX);
    println!("index {HARNESS_INDEX} (both harnesses) -> radii {control:?}");
    for index in 0..24 {
        println!("  index {index:3} -> radii {:?}", radii(index));
    }
    // Not an assertion about which is right: the printed sweep is the
    // deliverable, exactly as in `probe_disk_origin`.
    println!(
        "the server's column at (1016,1077) is stalactite 2 / stalagmite 0; \
         an index whose radii cannot reach that column would explain it"
    );
}
