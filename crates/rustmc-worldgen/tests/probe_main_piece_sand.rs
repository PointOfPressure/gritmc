//! Why the big ruin at (-58,28) yields four suspicious sand and not five.
//!
//! Its piece box matches the server exactly (`x -943..-928, y 43..58,
//! z 433..448`), and four of its five sand are in the right place. Only the
//! fifth, at (-928,43,436), is missing.
//!
//! Four-of-five is the informative part. `CappedProcessor` shuffles the whole
//! survivor list, so a list of a different *length* would produce an entirely
//! different permutation and all five would move. Sharing four means the
//! permutation is the same and the walk stopped one short — which points at
//! the number of sand candidates, not at the shuffle.

use rustmc_worldgen::rng::legacy::LegacyRandomSource;
use rustmc_worldgen::rng::worldgen_random::WorldgenRandom;
use rustmc_worldgen::structure::ocean_ruin::{place_blocks, plan};
use rustmc_worldgen::structure::template::{ignored_by_structure_and_air, survives_rot, Mirror};

const SEED: i64 = 31_415_926;

#[test]
fn the_big_ruins_survivor_list() {
    let dir = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .join("oracle-work/vanilla-26.2/generated-data/data/minecraft/structure/underwater_ruin");
    if !rustmc_data::fixture::available(&dir, "templates", "ci/vanilla-server.sh data") {
        return;
    }

    let (cx, cz) = (-58, 28);
    let mut random = WorldgenRandom::new(LegacyRandomSource::new(0));
    random.set_large_feature_seed(SEED, cx, cz);
    let p = plan(&mut random, cx, cz, 0.3, 0.9);

    let file = std::fs::File::open(dir.join(format!("{}.nbt", p.template))).expect("open");
    let mut reader = flate2::read::GzDecoder::new(file);
    let (_n, root) = rustmc_world::nbt::read_named_root(&mut reader).expect("nbt");
    let template =
        rustmc_worldgen::structure::template::Template::from_nbt(&root).expect("template");

    // The y the server's own piece box gives.
    let y = 43;
    let origin = (p.origin.0, y, p.origin.2);

    let mut survivors = 0;
    let mut sand = 0;
    let mut rotted_sand = 0;
    let mut total_sand = 0;
    for (pos, state) in template.blocks_rotated(origin, Mirror::None, p.rotation, (0, 0)) {
        let bare = state.split('[').next().unwrap_or(&state);
        if bare == "minecraft:sand" {
            total_sand += 1;
        }
        if !survives_rot(pos, p.integrity) {
            if bare == "minecraft:sand" {
                rotted_sand += 1;
            }
            continue;
        }
        if ignored_by_structure_and_air(&state) {
            continue;
        }
        survivors += 1;
        if bare == "minecraft:sand" {
            sand += 1;
        }
    }

    println!("{} at {origin:?} rot {:?}", p.template, p.rotation);
    println!("  template sand      {total_sand}");
    println!("  rotted away        {rotted_sand}");
    println!("  survivors          {survivors}");
    println!("  sand in survivors  {sand}");

    let blocks = place_blocks(&template, &p, y, SEED);
    let suspicious = blocks
        .iter()
        .filter(|(_, s)| s.starts_with("minecraft:suspicious_sand["))
        .count();
    println!("  suspicious placed  {suspicious}   (server has 5 here)");
}
