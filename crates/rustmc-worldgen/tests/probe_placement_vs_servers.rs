//! Which seed each saved vanilla world actually has, decided by placement.
//!
//! The region directories carry no `level.dat` — only `r.*.mca` — so a world's
//! seed is recorded nowhere but its directory name. That was fine until the two
//! disagreed: the world named `van-s2-c62` holds an `ocean_ruin_warm` start at
//! chunk (67,65), and this port's ocean-ruin placement selects (67,65) at seed
//! **31415926**, while at seed **20260722** it selects nothing in that window.
//!
//! One of three things is true, and the ocean ruin cannot be built until it is
//! known which:
//!
//!   1. the port's `random_spread` is wrong, or
//!   2. `van-s2-c62` is not the seed its name says, or
//!   3. the ocean-ruin *set* needs something the port does not do, and the
//!      chunk answer is collateral.
//!
//! The control that separates them is the **shipwreck**. It is a different
//! structure set with its own salt, it is placed by the same `random_spread`
//! code, and both servers saved one:
//!
//! ```text
//!   vanilla-r7    minecraft:shipwreck start (57, 65)
//!   van-rows-c64  minecraft:shipwreck start (57, 65)   <- same seed as above
//!   van-s2-c62    minecraft:shipwreck start (54, 59)
//! ```
//!
//! Two known answers at two seeds, from servers, for machinery shared with the
//! ocean ruin. If shipwrecks land correctly at both seeds then `random_spread`
//! is sound and the seed labels are honest, which leaves (3) — and (3) is the
//! structure-set `structures` list this port ignores.

use rustmc_worldgen::structure::placement::StructurePlacement;

const SEED_1: i64 = 31_415_926;
const SEED_2: i64 = 20_260_722;

fn load(set: &str) -> Option<StructurePlacement> {
    let path = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .join("oracle-work/vanilla-26.2/generated-data/data/minecraft/worldgen/structure_set")
        .join(format!("{set}.json"));
    if !rustmc_data::fixture::available(&path, "datagen extract", "ci/vanilla-server.sh data") {
        return None;
    }
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("read")).expect("json");
    Some(StructurePlacement::parse(&doc["placement"]).expect("placement"))
}

fn hits(placement: &StructurePlacement, seed: i64, lo: i32, hi: i32) -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    for z in lo..=hi {
        for x in lo..=hi {
            if placement.is_placement_chunk(seed, x, z).expect("supported") {
                out.push((x, z));
            }
        }
    }
    out
}

#[test]
fn shipwreck_is_the_control() {
    let Some(ship) = load("shipwrecks") else {
        return;
    };
    let Some(ruin) = load("ocean_ruins") else {
        return;
    };

    // The window every one of these worlds covers.
    let (lo, hi) = (48, 72);

    for (name, seed) in [("31415926", SEED_1), ("20260722", SEED_2)] {
        println!("seed {name}:");
        println!("  shipwrecks  {:?}", hits(&ship, seed, lo, hi));
        println!("  ocean_ruins {:?}", hits(&ruin, seed, lo, hi));
    }

    println!("servers saved:");
    println!("  vanilla-r7   (seed1 by name) shipwreck (57,65), no ocean ruin");
    println!("  van-rows-c64 (seed1 by name) shipwreck (57,65), no ocean ruin");
    println!("  van-s2-c62   (seed2 by name) shipwreck (54,59), ocean_ruin_warm (67,65)");

    let s1 = hits(&ship, SEED_1, lo, hi);
    let s2 = hits(&ship, SEED_2, lo, hi);
    let verdict = |v: &[(i32, i32)], want: (i32, i32)| v.contains(&want);

    println!(
        "shipwreck seed1 contains (57,65): {}",
        verdict(&s1, (57, 65))
    );
    println!(
        "shipwreck seed2 contains (54,59): {}",
        verdict(&s2, (54, 59))
    );
    println!(
        "CROSSED? seed1 contains (54,59): {}   seed2 contains (57,65): {}",
        verdict(&s1, (54, 59)),
        verdict(&s2, (57, 65))
    );
}
