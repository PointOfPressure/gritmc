//! Print our End city walk in `worldgen-dump jigsaw-starts`' own format.
//!
//! `cargo test --release -p rustmc-worldgen --test probe_end_city_rows --
//! --nocapture`, then diff against
//! `./target/release/worldgen-dump jigsaw-starts oracle-work/van-end-20260722-c8362`.
//!
//! A block diff cannot separate a wrong element from a wrong rotation from a
//! wrong stream position — they all read as blocks in the wrong place. These
//! rows can, so they are the gate between the `GD` integer and the blocks.
//! Milliseconds, and no vanilla run: the reference is already harvested.

use std::sync::Arc;

use rustmc_worldgen::rng::legacy::LegacyRandomSource;
use rustmc_worldgen::rng::worldgen_random::WorldgenRandom;
use rustmc_worldgen::structure::end_city;
use rustmc_worldgen::structure::template::{Rotation, Template};

struct Extract {
    dir: std::path::PathBuf,
    cache: std::collections::BTreeMap<String, Option<Arc<Template>>>,
}

impl end_city::Templates for Extract {
    fn get(&mut self, name: &str) -> Option<Arc<Template>> {
        if let Some(hit) = self.cache.get(name) {
            return hit.clone();
        }
        let path = self.dir.join(format!("{name}.nbt"));
        let loaded = std::fs::File::open(&path).ok().and_then(|file| {
            let mut reader = flate2::read::GzDecoder::new(file);
            let (_n, root) = rustmc_world::nbt::read_named_root(&mut reader).ok()?;
            Template::from_nbt(&root).ok().map(Arc::new)
        });
        self.cache.insert(name.to_string(), loaded.clone());
        loaded
    }
}

const fn rot_name(rotation: Rotation) -> &'static str {
    match rotation {
        Rotation::None => "NONE",
        Rotation::Clockwise90 => "CLOCKWISE_90",
        Rotation::Clockwise180 => "CLOCKWISE_180",
        Rotation::Counterclockwise90 => "COUNTERCLOCKWISE_90",
    }
}

#[test]
fn print_rows() {
    let seed: i64 = std::env::var("PROBE_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20_260_722);
    let cx: i32 = std::env::var("PROBE_CX")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(83);
    let cz: i32 = std::env::var("PROBE_CZ")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(62);
    // Vanilla's own stub height for this chunk. Overridable, because the
    // height gate needs a generated world and this probe deliberately does
    // not build one.
    let stub_y: i32 = std::env::var("PROBE_Y")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);

    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/vanilla/structure/end_city");
    if !rustmc_data::fixture::available(&dir, "end_city templates", "ci/vanilla-server.sh data") {
        return;
    }
    let mut extract = Extract {
        dir,
        cache: std::collections::BTreeMap::new(),
    };
    let mut random = WorldgenRandom::new(LegacyRandomSource::new(0));
    random.set_large_feature_seed(seed, cx, cz);
    let rotation = rustmc_worldgen::structure::jigsaw::random_rotation(&mut random);
    let pieces = end_city::start_house_tower(
        &mut extract,
        (cx * 16 + 7, stub_y, cz * 16 + 7),
        rotation,
        &mut random,
    )
    .expect("every end_city template is in the extract");

    println!(
        "START minecraft:end_city chunk ({cx}, {cz}) — {} template pieces (not a jigsaw)",
        pieces.len()
    );
    for (i, piece) in pieces.iter().enumerate() {
        println!(
            "{i:5}  {:<24} rot={:<20} TP={},{},{:<8} GD={:<6} BB=[{} {} {} .. {} {} {}]",
            piece.template,
            rot_name(piece.rotation),
            piece.pos.0,
            piece.pos.1,
            piece.pos.2,
            piece.gen_depth,
            piece.bbox.min_x,
            piece.bbox.min_y,
            piece.bbox.min_z,
            piece.bbox.max_x,
            piece.bbox.max_y,
            piece.bbox.max_z,
        );
    }
}
