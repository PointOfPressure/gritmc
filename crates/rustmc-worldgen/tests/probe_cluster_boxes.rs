//! The server's own per-piece boxes for a cluster, against ours.
//!
//! At (-58,28) every suspicious sand this port places is in the right spot and
//! the server has exactly **one** more. Counting alone cannot say why: a piece
//! at the wrong place, a piece the right place with one fewer sand candidate,
//! or a capped selection that changed four blocks where vanilla changed five.
//!
//! A saved `StructureStart` carries each child's `BB`, so the server states
//! where every piece is without any inference. Comparing those boxes to this
//! port's satellite origins separates "wrong piece position" from "right
//! position, wrong contents" — and only the second would be a processor bug.

use rustmc_world::anvil::read_region;

const SEED: i64 = 31_415_926;

/// **Neither world exists on this box, so this has never compared anything.**
///
/// `oracle-work/van-ruin--58_28` and `oracle-work/van-ruin--57_42` are not in
/// the shared `~/rustmc/oracle-work` and not in any of the 250 gate
/// directories. The loop `continue`d past both on every run and the test
/// reported `ok. 1 passed` — the whole binary, comparing nothing, for its
/// entire life.
///
/// `#[ignore]` rather than a conversion, because this is an **instrument**:
/// its own module docs describe it as the thing that separates "wrong piece
/// position" from "right position, wrong contents" when the count at (-58,28)
/// disagrees. It is a diagnostic to reach for, not a claim to gate on, and
/// `ci/checks/t1-known-red.toml` already carries four of those with
/// `max_ignored`. This is the ninth, which that file's header calls a
/// deliberate edit that has to name the new instrument.
///
/// The `continue` is now an assertion: if either world is ever generated, the
/// run must have visited at least one, so this cannot go back to being a
/// silent no-op the day somebody removes the `#[ignore]`.
#[test]
#[ignore = "diagnostic; needs oracle-work/van-ruin--58_28, which no tree on this box has"]
fn the_servers_piece_boxes_against_ours() {
    let mut visited = 0usize;
    for (cx, cz, dir) in [
        (-58, 28, "oracle-work/van-ruin--58_28"),
        (-57, 42, "oracle-work/van-ruin--57_42"),
    ] {
        let world = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")).join(dir);
        if !rustmc_data::fixture::available(&world, dir, "ci/gen-vanilla-region.sh") {
            continue;
        }
        visited += 1;

        let mut server_boxes = Vec::new();
        for entry in std::fs::read_dir(&world).expect("read the world") {
            let path = entry.expect("entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("mca") {
                continue;
            }
            for chunk in read_region(&path).expect("read a region file") {
                for start in &chunk.structure_starts {
                    if start.id != "minecraft:ocean_ruin_warm"
                        || (start.chunk_x, start.chunk_z) != (cx, cz)
                    {
                        continue;
                    }
                    if server_boxes.is_empty() {
                        for piece in &start.pieces {
                            server_boxes.push(piece.bounding_box);
                        }
                    }
                }
            }
        }
        server_boxes.sort_unstable();

        use rustmc_worldgen::rng::legacy::LegacyRandomSource;
        use rustmc_worldgen::rng::worldgen_random::WorldgenRandom;
        use rustmc_worldgen::structure::ocean_ruin::{cluster, plan};

        let mut random = WorldgenRandom::new(LegacyRandomSource::new(0));
        random.set_large_feature_seed(SEED, cx, cz);
        let p = plan(&mut random, cx, cz, 0.3, 0.9);
        let satellites = if p.clustered {
            cluster(&mut random, &p)
        } else {
            Vec::new()
        };

        println!("=== ({cx},{cz}) ===");
        println!(
            "  main {} {:?} origin {:?}",
            p.template, p.rotation, p.origin
        );
        for s in satellites.iter().filter(|s| s.placed) {
            println!(
                "  sat  {} {:?} origin {:?}",
                s.plan.template, s.plan.rotation, s.plan.origin
            );
        }
        println!("  server pieces ({}):", server_boxes.len());
        for b in &server_boxes {
            println!(
                "    x {}..{}  y {}..{}  z {}..{}",
                b[0], b[3], b[1], b[4], b[2], b[5]
            );
        }
    }
    assert!(
        visited > 0,
        "neither ruin world is present, so this probe compared nothing — which \
         is what it did silently before 2026-08-18"
    );
}
