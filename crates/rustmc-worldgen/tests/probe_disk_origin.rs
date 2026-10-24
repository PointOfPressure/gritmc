//! Which feature index would put `disk_gravel`'s origin where the *server*
//! put it.
//!
//! # The question
//!
//! On the 121-chunk window, chunk (63,68) at seed 20260722 holds a 16-block
//! gravel disk at y 45, centred near (1013, 1098), that this port does not
//! place. Everything upstream of it has been cleared by measurement rather
//! than by argument:
//!
//!   * our surface stage for that chunk is **bit-exact** against
//!     `SurfaceChunkGolden` — 98,304 blocks, 0 wrong — and both agree on
//!     177 sand / 79 gravel;
//!   * both *server-generated* vanilla worlds agree with each other on
//!     161 sand / 95 gravel, so the disk is deterministic in a server;
//!   * `DecorateGolden` for that chunk puts `disk_gravel`'s origin at
//!     (1018, 47, 1093) and records it writing **nothing**, and our pipeline
//!     reproduces that origin exactly.
//!
//! So two isolated oracles agree with us and both differ from two servers.
//! The origin the server used, (1013, ·, 1098), is inside chunk (63,68) — a
//! disk's origin always is, because `in_square` scatters within the chunk —
//! so it is the *same feature in the same chunk* drawing from a different
//! stream.
//!
//! `setFeatureSeed(populationSeed, index, step)` is the only input to that
//! stream besides the chunk. `step` is fixed at 6. That leaves **index**,
//! which is the feature's position in the step's list and therefore depends
//! on which biomes the chunk is judged to contain.
//!
//! # What this proves and what it does not
//!
//! If some index in range yields (1013, ·, 1098), the isolated harness and
//! the server disagree about feature *indices*, and every number this project
//! has taken from `DecorateGolden` inherits that. If no index does, the
//! difference is not the seed and the search moves to the chain's world reads
//! — `heightmap OCEAN_FLOOR_WG` and `block_predicate_filter matching_fluids`.
//!
//! Either way it is one cheap arithmetic sweep against a known answer, which
//! is why it is a probe and not an argument.

use rustmc_worldgen::feature::decorate::{population_seed, ChunkOrigin};
use rustmc_worldgen::rng::xoroshiro::XoroshiroRandomSource;
use rustmc_worldgen::rng::{RandomSource, WorldgenRandom};

/// The chunk, seed and step the whole question is about.
const SEED: i64 = 20_260_722;
const CHUNK_X: i32 = 63;
const CHUNK_Z: i32 = 68;
const STEP: i32 = 6;

/// `DecorateGolden`'s index for `disk_gravel` in that chunk, and the origin
/// it produced — reproduced exactly by this port.
const ISOLATED_INDEX: i32 = 32;
const ISOLATED_XZ: (i32, i32) = (1018, 1093);

/// Where the server's disk is centred, read off the 16 mismatched blocks.
const SERVER_XZ: (i32, i32) = (1013, 1098);

/// `in_square` — x then z, each `nextInt(16)` off the chunk's corner.
fn in_square(index: i32) -> (i32, i32) {
    let origin = ChunkOrigin {
        chunk_x: CHUNK_X,
        chunk_z: CHUNK_Z,
        min_section_y: -4,
    };
    let population = population_seed(origin, SEED);
    let mut random = WorldgenRandom::new(XoroshiroRandomSource::from_seed(0));
    random.set_feature_seed(population, index, STEP);
    let block = origin.block();
    let x = random.next_i32_bound(16) + block.x;
    let z = random.next_i32_bound(16) + block.z;
    (x, z)
}

#[test]
fn which_feature_index_puts_the_disk_where_the_server_put_it() {
    if !std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../oracle-work/vanilla-26.2"
    ))
    .exists()
    {
        eprintln!("skipped: no vanilla extract");
        return;
    }

    // The control: the index the isolated oracle used must reproduce the
    // isolated origin, or this probe is measuring something else entirely.
    let control = in_square(ISOLATED_INDEX);
    println!("index {ISOLATED_INDEX} (DecorateGolden's) -> {control:?}, expected {ISOLATED_XZ:?}");
    assert_eq!(
        control, ISOLATED_XZ,
        "the probe does not reproduce the origin both the oracle and the \
         pipeline agree on, so its arithmetic is wrong and nothing below it \
         means anything"
    );

    let mut hits = Vec::new();
    for index in 0..96 {
        let xz = in_square(index);
        if xz == SERVER_XZ {
            hits.push(index);
        }
    }

    println!("indices 0..96 yielding the server's {SERVER_XZ:?}: {hits:?}");
    for index in 0..40 {
        println!("  index {index:3} -> {:?}", in_square(index));
    }

    // Not an assertion about which answer is right — both outcomes are
    // findings, and the printed sweep is the deliverable.
    if hits.is_empty() {
        println!(
            "NO INDEX REPRODUCES THE SERVER ORIGIN — the difference is not \
             setFeatureSeed's index, so it is the chain's world reads."
        );
    } else {
        println!(
            "THE SERVER'S ORIGIN IS AN INDEX SHIFT — DecorateGolden used \
             {ISOLATED_INDEX}, the server behaves like {hits:?}."
        );
    }
}
