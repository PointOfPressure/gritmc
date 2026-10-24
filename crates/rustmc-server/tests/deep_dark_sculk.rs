//! The deep dark against vanilla, block for block, where vanilla agrees with
//! itself.
//!
//! Its own binary because it sets `RUSTMC_DECORATE_ORDER` for the whole
//! process: the measurement it pins was taken in `zx` order, the order the
//! harvests' `HARVEST-ORDER` stamp names, and a process-wide variable must not
//! leak into tests pinned in another order.

use rustmc_worldgen::pipeline::{Area, Through};

/// The deep dark around seed 12345's ancient city, block for block, in the
/// seven chunks where the two clamped vanilla runs (`sl2-city1-{a,b}`, rows,
/// centre (32,38) r=4) agree on every sculk-family block: (35,41), (35,42),
/// (36,37), (36,38), (36,40), (36,41), (36,42). Both read 1,056 positions with
/// FNV-1a 64 `0xb8c062890f40f006` over the sorted `x,y,z,state\n` lines.
///
/// Generated the way the measurement was: r=7 around (32,38), decoration
/// order `zx` (the harvests' stamped `dump_order`). Before the faithful
/// `SculkPatchFeature` port these chunks read 4, 268, 14 and 33 mismatches
/// where they now read 0 (`docs/cold-start/structures-2.md`).
#[test]
fn the_deep_dark_matches_vanilla_where_vanilla_agrees_with_itself() {
    if !worldgen_assembly::have_extract() {
        return;
    }
    std::env::set_var("RUSTMC_DECORATE_ORDER", "zx");
    let area = Area::square(32, 38, 7);
    let generated = worldgen_assembly::generate::generate(12_345, area, Through::Decoration)
        .expect("generates");
    let chunks = [
        (35, 41),
        (35, 42),
        (36, 37),
        (36, 38),
        (36, 40),
        (36, 41),
        (36, 42),
    ];
    let mut rows: Vec<(i32, i32, i32, String)> = Vec::new();
    for (cx, cz) in chunks {
        for y in -64..320 {
            for z in cz * 16..cz * 16 + 16 {
                for x in cx * 16..cx * 16 + 16 {
                    let state = generated.canvas.block_at(x, y, z);
                    if state.split('[').next().unwrap_or("").contains("sculk") {
                        rows.push((x, y, z, state.to_string()));
                    }
                }
            }
        }
    }
    rows.sort();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for (x, y, z, state) in &rows {
        for byte in format!("{x},{y},{z},{state}\n").bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    assert_eq!(
        (rows.len(), format!("{hash:#x}")),
        (1056, "0xb8c062890f40f006".to_string()),
        "sculk-family positions in the seven chunks vanilla agrees on"
    );
}
