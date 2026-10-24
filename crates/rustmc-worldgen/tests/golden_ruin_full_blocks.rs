//! Every block the ruin writes, against the server's world.
//!
//! The gates so far check two subsets: the five suspicious sand and the one
//! chest. Both were chosen because they are unambiguous — nothing else in the
//! world makes suspicious sand, and there is exactly one chest in two chunks.
//! Neither says anything about the other 27 blocks, which are ordinary
//! sandstone and stairs that terrain and other features also produce.
//!
//! It was written as a probe, expecting to have to separate real errors from
//! blocks a later feature legitimately overwrote. It found neither: **33 of 33
//! agree**, so it is a gate.
//!
//! It caught one real defect on its first run, which is the reason a subset
//! gate is not enough. The five suspicious sand were being written as bare
//! `minecraft:suspicious_sand` where the server has
//! `minecraft:suspicious_sand[dusted=0]` — the rule replaces with
//! `SUSPICIOUS_SAND.defaultBlockState()` and that default carries `dusted`.
//! The dedicated suspicious-sand gate compared with `starts_with` and passed
//! happily; only a full state-for-state comparison saw it. A block that reads
//! right to a human and wrong to a diff is exactly the failure this project
//! keeps finding, and the lesson is the same each time: compare canonical
//! states, over the whole set, against a server.

use rustmc_world::anvil::read_region;
use std::collections::BTreeMap;

#[test]
fn how_many_of_the_ruins_blocks_the_server_agrees_with() {
    let templates = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .join("oracle-work/vanilla-26.2/generated-data/data/minecraft/structure/underwater_ruin");
    let world = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .join("oracle-work/van-s2-c62");
    // Two fixtures, asked for separately: a compound `||` would name only one
    // of them in the failure, and the one it named would be whichever the
    // author wrote first rather than whichever is missing.
    if !rustmc_data::fixture::available(
        &templates,
        "underwater_ruin templates",
        "ci/vanilla-server.sh data",
    ) {
        return;
    }
    if !rustmc_data::fixture::available(&world, "server world", "ci/gen-vanilla-region.sh") {
        return;
    }

    use rustmc_worldgen::rng::legacy::LegacyRandomSource;
    use rustmc_worldgen::rng::worldgen_random::WorldgenRandom;
    use rustmc_worldgen::structure::ocean_ruin::{marker_blocks, place_blocks, plan};

    const SEED: i64 = 31_415_926;
    let (cx, cz) = (67, 65);
    let mut random = WorldgenRandom::new(LegacyRandomSource::new(0));
    random.set_large_feature_seed(SEED, cx, cz);
    let p = plan(&mut random, cx, cz, 0.3, 0.9);

    let file = std::fs::File::open(templates.join(format!("{}.nbt", p.template))).expect("open");
    let mut reader = flate2::read::GzDecoder::new(file);
    let (_n, root) = rustmc_world::nbt::read_named_root(&mut reader).expect("gzipped NBT");
    let template =
        rustmc_worldgen::structure::template::Template::from_nbt(&root).expect("template");

    let mut ours = place_blocks(&template, &p, 48, SEED);
    ours.extend(marker_blocks(&template, &p, 48, |_| true));

    // The server's blocks, indexed by position.
    let mut server: BTreeMap<(i32, i32, i32), String> = BTreeMap::new();
    for entry in std::fs::read_dir(&world).expect("read the world") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("mca") {
            continue;
        }
        for chunk in read_region(&path).expect("read a region file") {
            if !(66..=67).contains(&chunk.x) || chunk.z != 65 {
                continue;
            }
            for section in &chunk.sections {
                let base_y = i32::from(section.y) * 16;
                if !(32..=64).contains(&base_y) {
                    continue;
                }
                // `blocks` is 4096 palette indices in y*256 + z*16 + x order;
                // empty means every cell is palette entry 0.
                for i in 0..4096usize {
                    let index = section.blocks.get(i).copied().unwrap_or(0) as usize;
                    let Some(state) = section.palette.get(index) else {
                        continue;
                    };
                    let (x, y, z) = (i & 15, (i >> 8) & 15, (i >> 4) & 15);
                    server.insert(
                        (
                            chunk.x * 16 + x as i32,
                            base_y + y as i32,
                            chunk.z * 16 + z as i32,
                        ),
                        state.clone(),
                    );
                }
            }
        }
    }
    assert!(!server.is_empty(), "read no server blocks at all");

    let mut agree = 0;
    let mut disagree = Vec::new();
    for (pos, state) in &ours {
        match server.get(pos) {
            Some(theirs) if theirs == state => agree += 1,
            Some(theirs) => disagree.push((*pos, state.clone(), theirs.clone())),
            None => disagree.push((*pos, state.clone(), "<not read>".to_string())),
        }
    }

    println!(
        "ruin writes {} blocks; {agree} agree with the server",
        ours.len()
    );
    for (pos, mine, theirs) in &disagree {
        println!("  {pos:?}  ours {mine}  server {theirs}");
    }

    assert!(
        disagree.is_empty(),
        "{} of the ruin's {} blocks disagree with the server",
        disagree.len(),
        ours.len()
    );
    assert_eq!(agree, 33, "the ruin is 33 blocks and all of them are gated");
}
