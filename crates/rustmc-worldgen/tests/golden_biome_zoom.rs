//! `BiomeManager`'s block-resolution biome lookup, against real vanilla.
//!
//! This is the fuzz that makes biome borders wobble instead of stepping
//! along a visible 4-block grid: a position resolves to whichever of eight
//! surrounding quart cells is nearest *after* a seeded perturbation.
//!
//! Worth a golden of its own because the port has three places to go wrong
//! that all still produce plausible output — the -2 centring shift, the
//! unusual `rval *= rval * A + B` LCG, and `floorMod` on a negative state —
//! and because nothing downstream would obviously fail if it were wrong.
//!
//! The vectors record which quart cell vanilla *asked its biome source for*,
//! captured by handing `BiomeManager` a source that records its argument.
//! That isolates the lookup from the biome table entirely.

use rustmc_worldgen::biome::manager::{obfuscate_seed, quart_for_block};
use serde_json::Value;

const GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/golden/biome_zoom_vectors.json"
);

#[test]
fn the_biome_zoom_matches_vanilla() {
    let text = std::fs::read_to_string(GOLDEN).expect("golden vectors");
    let doc: Value = serde_json::from_str(&text).expect("json");

    let mut total = 0usize;
    let mut wrong = Vec::new();

    for (seed_text, cells) in doc.as_object().expect("seed map") {
        let seed: i64 = seed_text.parse().expect("seed");
        let zoom = obfuscate_seed(seed);
        for (position, expected) in cells.as_object().expect("cell map") {
            let (x, z) = position.split_once(',').expect("x,z");
            let (x, z): (i32, i32) = (x.parse().expect("x"), z.parse().expect("z"));
            let want: Vec<i32> = expected
                .as_array()
                .expect("[qx, qy, qz]")
                .iter()
                .map(|v| v.as_i64().expect("quart") as i32)
                .collect();

            let got = quart_for_block(zoom, x, 70, z);
            total += 1;
            if got != (want[0], want[1], want[2]) {
                if wrong.len() < 8 {
                    wrong.push(format!(
                        "seed={seed} block=({x}, 70, {z}): want {want:?}, got {got:?}"
                    ));
                } else {
                    wrong.push(String::new());
                }
            }
        }
    }

    assert!(total > 0, "no positions were checked");
    let bad = wrong.len();
    let exactness = 100.0 * (total - bad) as f64 / total as f64;
    println!("BIOME ZOOM EXACTNESS {exactness:.6}% over {total} positions ({bad} wrong)");
    assert!(
        bad == 0,
        "{bad} of {total} lookups differ ({exactness:.6}% exact)\n  {}",
        wrong
            .iter()
            .filter(|w| !w.is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}
