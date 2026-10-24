//! Which warm-ruin template the server drew, read off the size of its union.
//!
//! The server saved this start, and it is the only evidence about the draw:
//!
//! ```text
//!   minecraft:ocean_ruin_warm start (67, 65) 1 pieces,
//!                                   union 1066,48,1040..1072,54,1045
//! ```
//!
//! A union is inclusive, so that is **7 x 7 x 6** in x, y, z. `addPiece`'s warm
//! branch picks one identifier with a single `nextInt(len)` — `Util.getRandom`
//! is `array[random.nextInt(array.length)]`, one draw, confirmed by javap — from
//! `WARM_RUINS` (warm_1..warm_8) when small, or `BIG_WARM_RUINS`
//! (big_warm_4..7) when large.
//!
//! A template's declared `size` is not automatically the union: rotation swaps
//! x and z, and the piece's bounding box is the *transformed* extent. So the
//! test is that some template matches under some rotation, and the printout
//! says which ones and under which.
//!
//! If exactly one candidate fits, the draw is determined without implementing
//! anything, and that pins both the `large` coin and the `nextInt` result
//! before a single block is written. That ordering matters: it is the same
//! discipline that caught `block_column`, where the measurement was taken
//! before the port was trusted.

use rustmc_worldgen::structure::template::Template;

/// The server's union, inclusive, as (x, y, z) extents.
const UNION: (i32, i32, i32) = (7, 7, 6);

fn dir() -> Option<std::path::PathBuf> {
    let d = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .join("oracle-work/vanilla-26.2/generated-data/data/minecraft/structure/underwater_ruin");
    if !rustmc_data::fixture::available(
        &d,
        "underwater_ruin templates",
        "ci/vanilla-server.sh data",
    ) {
        return None;
    }
    Some(d)
}

#[test]
fn which_warm_template_has_the_server_s_extent() {
    let Some(dir) = dir() else { return };

    let small: Vec<String> = (1..=8).map(|i| format!("warm_{i}")).collect();
    let big: Vec<String> = (4..=7).map(|i| format!("big_warm_{i}")).collect();

    for (label, names) in [("WARM_RUINS", &small), ("BIG_WARM_RUINS", &big)] {
        println!("{label}:");
        for (index, name) in names.iter().enumerate() {
            let path = dir.join(format!("{name}.nbt"));
            let Ok(file) = std::fs::File::open(&path) else {
                println!("  [{index}] {name}: unreadable");
                continue;
            };
            let mut reader = flate2::read::GzDecoder::new(file);
            let (_n, root) = rustmc_world::nbt::read_named_root(&mut reader).expect("gzipped NBT");
            match Template::from_nbt(&root) {
                Ok(t) => {
                    let (sx, sy, sz) = t.size;
                    // Under a 90-degree rotation the horizontal extents swap.
                    let upright = (sx, sy, sz) == UNION;
                    let turned = (sz, sy, sx) == UNION;
                    let mark = match (upright, turned) {
                        (true, true) => "  <== MATCHES (square, any rotation)",
                        (true, false) => "  <== MATCHES upright (0 or 180)",
                        (false, true) => "  <== MATCHES turned (90 or 270)",
                        _ => "",
                    };
                    println!(
                        "  [{index}] {name}: size {sx}x{sy}x{sz}, {} blocks{mark}",
                        t.blocks.len()
                    );
                }
                Err(e) => println!("  [{index}] {name}: malformed: {}", e.0),
            }
        }
    }
    println!("server union extent is {UNION:?} (inclusive)");
}
