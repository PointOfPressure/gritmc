//! `OCEAN_FLOOR_WG` after carving, against vanilla.
//!
//! This is the one heightmap vanilla does **not** save into a region file, so
//! unlike the four live maps it has no free oracle and a whole-world diff
//! cannot see it at all. It is also the map a surprising amount of decoration
//! hangs off: `disk_sand`, `seagrass_warm`, `kelp_warm` and every ore chain
//! anchor to it, and `underwater_magma` filters on it through
//! `surface_relative_threshold_filter`.
//!
//! The values were already being shipped — `ore_vectors.json` carries
//! vanilla's `oceanFloorWg` per chunk — but only as an **input**: the ore and
//! decorate gates take vanilla's numbers and generate from them, precisely so
//! that a fault in this map cannot be mistaken for a fault in the feature
//! under test. That is the right call for those gates and it leaves this map
//! untested, which is what this file fixes. Nothing new had to be dumped.
//!
//! # Why "after carving" is the whole point
//!
//! `ProtoChunk#setBlockState` updates whatever
//! `getPersistedStatus().heightmapsAfter()` names, and the persisted status
//! lags the running step by one — so while CARVERS runs, the set is still
//! `{OCEAN_FLOOR_WG, WORLD_SURFACE_WG}`. The map therefore tracks noise,
//! surface *and* carving, and only becomes a fossil when decoration starts.
//! A port that froze it after the noise stage would pass a noise-only gate
//! and place a `disk_sand` four blocks up in open water.
//!
//! # What this gate does not cover, stated plainly
//!
//! The dumper's `ProtoChunk` never has a status set, so its
//! `heightmapsAfter()` is empty and nothing updates the map after the noise
//! fill. The shipped numbers are therefore the **noise-stage** values, and
//! our post-carve values match all 6,912 of them exactly — which means
//! carving lowered no column in these three worlds, not that the carve-update
//! path is proved. It plausibly did not: these worlds are land, where the map
//! sits at the surface and a cave cut underground never reaches it. The case
//! that does exercise it is an ocean floor thin enough for a carver to break
//! through, and covering that needs a dumper that sets a status.

use rustmc_worldgen::pipeline::{Area, Through};
use serde_json::Value;

const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/golden/ore_vectors.json");

fn read(path: &str) -> Value {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("golden vector {path} unreadable: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path} is not JSON: {e}"))
}

#[test]
fn the_carved_ocean_floor_wg_matches_vanilla() {
    let doc = read(GOLDEN);
    let seed: i64 = doc["seed"]
        .as_str()
        .expect("seed")
        .parse()
        .expect("seed is an i64");

    let worlds = doc["worlds"].as_array().expect("worlds");
    let mut compared = 0usize;
    let mut wrong = 0usize;
    // How far off, so a systematic shift reads differently from noise.
    let mut deltas: std::collections::BTreeMap<i32, usize> = std::collections::BTreeMap::new();
    let mut examples: Vec<String> = Vec::new();

    for world in worlds {
        let centre_x = i32::try_from(world["centerX"].as_i64().expect("centerX")).expect("i32");
        let centre_z = i32::try_from(world["centerZ"].as_i64().expect("centerZ")).expect("i32");

        // The vectors are a 3x3 around the centre, and the map is only
        // correct for a chunk whose neighbours were carved too — carvers
        // reach across a chunk border.
        let area = Area::square(centre_x, centre_z, 1);
        // See `golden_assembly_chunk.rs`: an absent extract is refused by
        // `have_extract`, so a failure here is data this code cannot read.
        let generated = worldgen_assembly::generate::generate(seed, area, Through::Carvers)
            .unwrap_or_else(|e| panic!("the assembly could not load its data: {e}"));

        for chunk in world["chunks"].as_array().expect("chunks") {
            let chunk_x = i32::try_from(chunk["chunkX"].as_i64().expect("chunkX")).expect("i32");
            let chunk_z = i32::try_from(chunk["chunkZ"].as_i64().expect("chunkZ")).expect("i32");
            let want = chunk["oceanFloorWg"].as_array().expect("oceanFloorWg");
            assert_eq!(want.len(), 256, "one entry per column");

            for (i, value) in want.iter().enumerate() {
                // **Two conventions, one number.** The dumper calls
                // `ChunkAccess#getHeight`, which is `getFirstAvailable() - 1`
                // — the y of the highest matching block. A *feature* never
                // sees that: it goes through `WorldGenRegion#getHeight`,
                // which adds the 1 back, so what a placement chain anchors to
                // is one **above** the highest matching block. Our canvas
                // stores the feature-visible form, so the shipped value is
                // converted rather than the canvas being bent to match a
                // convention nothing in the generator uses.
                let want = i32::try_from(value.as_i64().expect("height")).expect("i32") + 1;
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_possible_wrap,
                    reason = "an index bounded by 256"
                )]
                let flat = i as i32;
                let x = chunk_x * 16 + flat % 16;
                let z = chunk_z * 16 + flat / 16;
                let got = generated.canvas.ocean_floor_wg(x, z);
                compared += 1;
                if got != want {
                    wrong += 1;
                    *deltas.entry(got - want).or_default() += 1;
                    if examples.len() < 8 {
                        examples.push(format!("({x}, {z}) vanilla {want}, rustmc {got}"));
                    }
                }
            }
        }
    }

    assert!(compared > 0, "no columns compared — the vectors are empty");
    let exact = 100.0 * (1.0 - wrong as f64 / compared as f64);
    println!("OCEAN_FLOOR_WG after carving: {wrong}/{compared} wrong, {exact:.6}% exact");
    if !deltas.is_empty() {
        println!("  deltas (rustmc - vanilla): {deltas:?}");
    }
    for example in &examples {
        println!("  {example}");
    }
    assert_eq!(
        wrong, 0,
        "OCEAN_FLOOR_WG disagrees with vanilla on {wrong} of {compared} columns. \
         Nothing downstream of this can be trusted while it is wrong: disk_sand, \
         the aquatic features and every ore chain anchor to it, and a whole-world \
         block diff cannot see it because vanilla never writes it to disk."
    );
}
