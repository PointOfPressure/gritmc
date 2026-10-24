//! The wired path, driven end to end.
//!
//! `crates/rustmc-server/tests/light_golden.rs` grades the engine against
//! vanilla. This file grades something different and just as necessary: that
//! the engine is actually *connected* to the packet a client receives.
//!
//! The distinction is this project's `ACCEPT_TELEPORTATION` defect — a value
//! decoded correctly into a field no behaviour reads, behind a doc comment
//! describing a gate no line implemented. A light engine that is exact
//! against vanilla and never runs in the server is the same defect with a
//! better test suite attached, so this test runs the server's own
//! `World::chunk` and reads the light off the packet body.
//!
//! It is its own integration test binary, and that is deliberate:
//! `RUSTMC_SECTION_LIGHT` is process-global, so setting it in a file that
//! shares a process with other tests would make their results depend on the
//! order the harness happened to run them in.

use rustmc_server::chunk::LightData;
use rustmc_server::configuration::CapturedRegistries;
use rustmc_server::terrain::StateIds;
use rustmc_server::world::{World, WorldTables};
use rustmc_worldgen::pipeline::Area;

const SEED: i64 = 20260722;

fn repo(relative: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn world() -> Option<World> {
    if !worldgen_assembly::have_extract() {
        eprintln!("skipped: no vanilla extract");
        return None;
    }
    let registries = CapturedRegistries::load(&repo("data/vanilla/registries.bin")).ok()?;
    let biomes = registries
        .entry_ids("minecraft:worldgen/biome")
        .expect("readable capture")
        .expect("the capture holds the biome registry");
    let ids = StateIds::load(&repo("data/vanilla/block_props.json")).expect("block table");
    let tables = std::sync::Arc::new(WorldTables::load(&biomes, ids).expect("block tables"));
    // Radius 1: a 3x3, which is the smallest area in which the centre chunk
    // has all eight of the neighbours the stored-section rule reads.
    Some(World::generate(SEED, Area::square(0, 0, 1), tables).expect("generated"))
}

#[test]
fn the_flag_is_what_stands_between_us_and_zero_of_twenty_six() {
    let Some(w) = world() else { return };

    // Off — the pre-2026-09-05 wire, still reachable for a bisect. The flag
    // inverted on that date, so this half now needs the variable SET; before
    // it, it needed the variable absent. A test that kept the old polarity
    // would pass on the new default while measuring nothing, because the
    // dark case would then be the lit one.
    // SAFETY: this test binary runs one test and touches no other thread.
    std::env::set_var("RUSTMC_SECTION_LIGHT", "0");
    let dark = w.chunk(0, 0).expect("chunk encodes");
    std::env::remove_var("RUSTMC_SECTION_LIGHT");
    assert_eq!(
        dark.light,
        LightData::default(),
        "with the flag off the packet must carry exactly what it carried before \
         the flip — four empty bitsets and two empty array lists"
    );

    // A second `World`, because the light is computed at most once per world
    // and the first one has already cached the answer for the flag-off case.
    let Some(w) = world() else { return };
    // Nothing is set: this is the DEFAULT, which is the whole point of the
    // flip. A `set_var("1")` here would pass equally on a tree whose default
    // was still off, and would therefore grade nothing.
    std::env::remove_var("RUSTMC_SECTION_LIGHT");
    let lit = w.chunk(0, 0).expect("chunk encodes");

    assert_ne!(
        lit.light,
        LightData::default(),
        "with no flag set the packet must carry real section light — if this \
         fails either the default is off again or the engine is not wired to \
         the packet at all, which is the ACCEPT_TELEPORTATION defect and the \
         reason this file exists"
    );

    // Count what actually reached the wire, in the units the item is named
    // for: sections classified out of the dimension's light section count.
    let classified = [
        &lit.light.sky_mask,
        &lit.light.block_mask,
        &lit.light.empty_sky_mask,
        &lit.light.empty_block_mask,
    ]
    .iter()
    .map(|m| m.iter().map(|w| w.count_ones()).sum::<u32>())
    .sum::<u32>();
    eprintln!(
        "wired: {classified} section/layer classifications on chunk (0,0), \
         {} sky arrays and {} block arrays",
        lit.light.sky.len(),
        lit.light.block.len()
    );
    assert!(
        classified > 0,
        "the flag is on and no section landed in any mask, which means the \
         region was built empty rather than lit"
    );

    // Every array is a whole `DataLayer` and every mask bit that claims one
    // has one. A packet that fails this does not decode at the client.
    let sky_bits: u32 = lit.light.sky_mask.iter().map(|w| w.count_ones()).sum();
    let block_bits: u32 = lit.light.block_mask.iter().map(|w| w.count_ones()).sum();
    assert_eq!(
        sky_bits as usize,
        lit.light.sky.len(),
        "sky mask claims {sky_bits} sections and {} arrays follow",
        lit.light.sky.len()
    );
    assert_eq!(block_bits as usize, lit.light.block.len());
    for a in lit.light.sky.iter().chain(lit.light.block.iter()) {
        assert_eq!(a.len(), 2048, "a DataLayer on the wire is 2048 bytes");
    }

    // And the packet must still round-trip, which is the one check that
    // covers the field ORDER as well as the contents: `encode` writes the
    // four masks and then the two array lists, and a client reads them in
    // that order or not at all.
    let body = lit.encode().expect("encodes");
    let back = rustmc_server::chunk::LevelChunkWithLight::decode(&body, lit.sections.len())
        .expect("a chunk packet carrying real light must decode");
    assert_eq!(
        back.light, lit.light,
        "light did not survive the round trip"
    );
}
