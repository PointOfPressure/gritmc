//! Is `settle_height` load-bearing for the ruin at (67,65), or a no-op?
//!
//! `postProcess` sets the y twice: first from the `OCEAN_FLOOR_WG` heightmap
//! at the template position, then from `getHeight`, which walks the footprint
//! and only lowers the piece when the ground under most of it is more than two
//! blocks below.
//!
//! The server put the ruin's sand at y **48**. If the heightmap alone already
//! answers 48 at (1072, 1040), then `getHeight` changed nothing here and the
//! block-exact agreement already gated says nothing about it — the height walk
//! would still be completely untested against terrain, and believing otherwise
//! is the same "correct for what was measured" trap as the biome container and
//! the beardifier.
//!
//! Read from our own generated world rather than vanilla's, because vanilla's
//! saved region already contains the ruin and its heightmap is post-decoration;
//! ours has the terrain the ruin would have been placed onto.

use rustmc_world::anvil::read_region;

/// **Its world is `/tmp/ruin-w/region`, so it has never run in any gate.**
///
/// That path is a scratch directory from whatever one-off run answered the
/// question in the module docs above. `/tmp` does not survive a reboot and no
/// gate directory has ever contained it, so the `if !dir.exists() { return }`
/// this replaces made the test report `ok. 1 passed` on every run since it was
/// written, having read nothing.
///
/// Found by converting the skip: `ci/t1-gate.sh` on this branch went red here
/// and nowhere else, which is the conversion earning its place on the first
/// full run after it.
///
/// `#[ignore]` rather than a fix, for the same reason as
/// `probe_cluster_boxes`: the module docs above are a **question** — is
/// `settle_height` load-bearing for the ruin at (67,65), or a no-op — and the
/// answer is a `println!` sweep. An instrument, not a gate. Reproduce the world
/// with `ci/dump-chunks.sh` into that path and lift the `#[ignore]`; the
/// tenth entry under `max_ignored` in `ci/checks/t1-known-red.toml` names it.
#[test]
#[ignore = "diagnostic; its world is /tmp/ruin-w/region, a scratch path no gate directory has"]
fn what_the_heightmap_says_under_the_ruin() {
    let dir = std::path::Path::new("/tmp/ruin-w/region");
    if !rustmc_data::fixture::available(dir, "generated world", "ci/dump-chunks.sh") {
        return;
    }

    let mut chunks = Vec::new();
    for entry in std::fs::read_dir(dir).expect("read the region directory") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|e| e.to_str()) == Some("mca") {
            chunks.extend(read_region(&path).expect("read a region file"));
        }
    }

    // The ruin's origin column, and the far corner of its footprint.
    for (label, wx, wz) in [
        ("origin", 1072, 1040),
        ("corner", 1066, 1045),
        ("sand-1", 1068, 1041),
        ("sand-5", 1067, 1044),
    ] {
        let (cx, cz) = (wx >> 4, wz >> 4);
        let Some(chunk) = chunks.iter().find(|c| c.x == cx && c.z == cz) else {
            println!("{label} ({wx},{wz}): chunk ({cx},{cz}) not generated");
            continue;
        };
        let index = ((wz & 15) * 16 + (wx & 15)) as usize;
        let mut line = format!("{label} ({wx},{wz}) chunk ({cx},{cz}):");
        if chunk.heightmaps.is_empty() {
            line.push_str(" no heightmaps saved");
        }
        // `pack_heightmap` stores `value - min_y`, and the reader hands back
        // the stored form, so a world y needs min_y adding again. Overworld
        // min_y is -64. Getting this wrong reads a seafloor at 112.
        const MIN_Y: i32 = -64;
        for (name, values) in &chunk.heightmaps {
            if let Some(v) = values.get(index) {
                line.push_str(&format!(" {name}={}", v + MIN_Y));
            }
        }
        println!("{line}");
    }
    println!("the server's ruin sand is at y 48");

    // The finding, asserted so it cannot quietly stop being true.
    let origin = chunks
        .iter()
        .find(|c| c.x == 67 && c.z == 65)
        .expect("the ruin's origin chunk");
    // Bound through variables: the literals fold to zero and clippy rejects
    // the constant form, but the expression is the column index and should
    // read as one.
    let (wx, wz) = (1072i32, 1040i32);
    let index = ((wz & 15) * 16 + (wx & 15)) as usize;
    let ocean_floor = origin.heightmaps["OCEAN_FLOOR"][index] - 64;
    assert_eq!(
        ocean_floor, 48,
        "OCEAN_FLOOR at the ruin's origin column, in world coordinates"
    );
    println!(
        "OCEAN_FLOOR at the origin column is already {ocean_floor}, which is \
         the y the server used, so `getHeight` lowered nothing here and is \
         STILL untested against terrain"
    );
}
