//! The cluster, against a server that has one.
//!
//! Two of the three warm ruins sampled near the origin are clusters, so this
//! is the common shape rather than an edge case:
//!
//! ```text
//!   (-57,42)  ocean_ruin_warm  7 pieces  union -942,37,657..-898,56,692
//!   (-58,28)  ocean_ruin_warm  6 pieces  union -960,43,417..-923,58,462
//! ```
//!
//! A cluster is the main piece plus its satellites, so 7 pieces means the main
//! ruin plus 6 placed satellites. `addClusterRuins` rolls `Mth.nextInt(4, 8)`
//! candidates and skips any whose probe box overlaps the main ruin, so the
//! placed count is at most that roll and usually less.
//!
//! The piece count is read from the server's own saved start — one of the few
//! numbers a server records about a structure without inspecting its blocks,
//! and a strict one: it is the joint outcome of the large coin, the cluster
//! coin, `allPositions`' sixteen draws, `Mth.nextInt(4, 8)`, one removal draw
//! and one rotation draw per candidate, and the overlap test that decides
//! whether the template draw happens at all.
//!
//! All three matched on the first run, with nothing tuned — every span and
//! bound was read off the bytecode before this was written.

use rustmc_worldgen::rng::legacy::LegacyRandomSource;
use rustmc_worldgen::rng::worldgen_random::WorldgenRandom;
use rustmc_worldgen::structure::ocean_ruin::{cluster, plan};

const SEED: i64 = 31_415_926;

fn have_extract() -> bool {
    // Local rather than `common::have_extract`, because this gate reads the
    // ocean-ruin structure set specifically. Converted for the same reason:
    // the bare `.exists()` behind it made the one test in this file pass on a
    // tree with no extract, and print nothing while doing it.
    rustmc_data::fixture::available(
        &std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
            .join("oracle-work/vanilla-26.2/generated-data/data/minecraft/worldgen/structure_set/ocean_ruins.json"),
        "ocean-ruin structure set",
        "ci/vanilla-server.sh data",
    )
}

#[test]
fn cluster_piece_counts_against_the_servers_saved_starts() {
    if !have_extract() {
        return;
    }

    // (chunk, pieces the server saved)
    for (cx, cz, server_pieces) in [(-57, 42, 7), (-58, 28, 6), (64, 47, 1)] {
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(0));
        random.set_large_feature_seed(SEED, cx, cz);
        let p = plan(&mut random, cx, cz, 0.3, 0.9);

        let satellites = if p.clustered {
            cluster(&mut random, &p)
        } else {
            Vec::new()
        };
        let placed = satellites.iter().filter(|s| s.placed).count();
        let ours = 1 + placed;

        println!(
            "({cx},{cz}) large={} clustered={} template={} rot={:?} \
             candidates={} placed={placed} -> {ours} pieces, server {server_pieces}",
            p.large,
            p.clustered,
            p.template,
            p.rotation,
            satellites.len()
        );
        assert_eq!(
            ours, server_pieces,
            "({cx},{cz}): the server saved {server_pieces} pieces and this \
             produces {ours}; the piece count is the joint outcome of both \
             coins, allPositions' sixteen draws, the 4..8 roll, and the \
             overlap test, so a mismatch names the whole chain at once"
        );
        for s in &satellites {
            println!(
                "    {} {:?} at {:?} {}",
                if s.placed { "PLACE" } else { "skip " },
                s.plan.rotation,
                s.plan.origin,
                s.plan.template
            );
        }
    }
}
