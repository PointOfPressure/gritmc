//! The End city walk, gated against a vanilla world before a block is placed.
//!
//! The reference is `oracle-work/van-end-20260722-c8362` — seed 20260722,
//! centre chunk (83, 62) — read by `worldgen-dump jigsaw-starts`, which prints
//! a `TemplateStructurePiece`'s `Template`, `Rot`, `TPX/TPY/TPZ` and `GD`.
//! Vanilla wrote 70 pieces there. Both figures below come from that world and
//! from its independently generated rule-6 twin, which agrees piece for piece.
//!
//! # Why `GD` is the gate and not an afterthought
//!
//! `recursiveChildren` draws an **unbounded** `nextInt()` after its generator
//! returns and stamps it over its whole sublist, so the outermost call
//! overwrites every nested one. The value vanilla recorded is therefore the
//! stream value at position `N`, where `N` is the total number of draws the
//! entire walk consumed — a single integer that pins element order, every
//! rotation and every branch taken. A block diff cannot separate a wrong
//! element from a wrong rotation from a wrong stream position; this can, and
//! it costs milliseconds and no vanilla run.
//!
//! This test needs no heightmap. Vanilla's own piece 0 records `TPY = 60`,
//! which is the `getLowestYIn5by5BoxOffset7Blocks` answer for that chunk, so
//! the walk is driven from the recorded stub and the height gate is tested
//! separately by `generation_point`'s own arithmetic.

use std::sync::Arc;

use rustmc_worldgen::rng::legacy::LegacyRandomSource;
use rustmc_worldgen::rng::worldgen_random::WorldgenRandom;
use rustmc_worldgen::structure::end_city;
use rustmc_worldgen::structure::template::{Rotation, Template};

const SEED: i64 = 20_260_722;
const CHUNK: (i32, i32) = (83, 62);
/// Vanilla's `TPY` for piece 0 — the stub height the height gate returned.
const STUB_Y: i32 = 60;
/// The stamp every one of the 66 recursive pieces carries in the reference.
const GEN_DEPTH: i32 = 1_154_021_824;

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

fn walk() -> Option<(Rotation, Vec<end_city::Piece>)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/vanilla/structure/end_city");
    if !rustmc_data::fixture::available(&dir, "end_city templates", "ci/vanilla-server.sh data") {
        return None;
    }
    let mut extract = Extract {
        dir,
        cache: std::collections::BTreeMap::new(),
    };
    let mut random = WorldgenRandom::new(LegacyRandomSource::new(0));
    random.set_large_feature_seed(SEED, CHUNK.0, CHUNK.1);
    // `EndCityStructure.findGenerationPoint` draws this *first*, before the
    // height is taken, so it is the walk's draw 0.
    let rotation = rustmc_worldgen::structure::jigsaw::random_rotation(&mut random);
    let pieces = end_city::start_house_tower(
        &mut extract,
        (CHUNK.0 * 16 + 7, STUB_Y, CHUNK.1 * 16 + 7),
        rotation,
        &mut random,
    )
    .expect("every end_city template is in the extract");
    Some((rotation, pieces))
}

/// The cheapest gate in the item: one integer, and it pins the draw count of
/// the whole walk.
#[test]
fn gen_depth_matches_vanilla() {
    let Some((rotation, pieces)) = walk() else {
        return;
    };
    assert_eq!(
        rotation,
        Rotation::None,
        "vanilla's four stub pieces all record rot=NONE"
    );
    assert_eq!(
        pieces.len(),
        70,
        "vanilla wrote 70 pieces in chunk (83, 62)"
    );
    // The four `startHouseTower` pieces are added straight to the outer list
    // and are never in anybody's sublist, so nothing ever stamps them.
    for piece in &pieces[..4] {
        assert_eq!(piece.gen_depth, 0, "{} is a stub piece", piece.template);
    }
    for piece in &pieces[4..] {
        assert_eq!(
            piece.gen_depth, GEN_DEPTH,
            "{} carries the outermost stamp",
            piece.template
        );
    }
}

/// The four pieces `startHouseTower` places itself, element for element and
/// block for block. They fix the connected-position arithmetic — which is
/// `transform(offset, parentRotation, pivot ZERO)` and **not**
/// `getZeroPositionWithTransform`, a size-dependent formula that would move
/// every rotated piece.
#[test]
fn the_stub_matches_vanilla() {
    let Some((_, pieces)) = walk() else {
        return;
    };
    let expected = [
        ("base_floor", (1335, 60, 999)),
        ("second_floor_1", (1334, 60, 998)),
        ("third_floor_1", (1333, 64, 997)),
        ("third_roof", (1332, 72, 996)),
    ];
    for (piece, (name, pos)) in pieces.iter().zip(expected) {
        assert_eq!(piece.template, name);
        assert_eq!(piece.pos, pos, "{name} template position");
        assert_eq!(piece.rotation, Rotation::None);
    }
}

/// This city has no ship, which the reference's census confirms — zero
/// `end_gateway`, zero shulker boxes and zero `ship` pieces. The branch is
/// ported from bytecode but **this reference cannot test it**, and saying so
/// here is cheaper than someone later reading a green bar as proof.
#[test]
fn this_reference_holds_no_ship() {
    let Some((_, pieces)) = walk() else {
        return;
    };
    assert_eq!(pieces.iter().filter(|p| p.template == "ship").count(), 0);
}

/// The height gate's arithmetic, which needs no world: the sample box's sign
/// follows the rotation, the answer is the **minimum** of four corners, and
/// anything under y=60 is refused outright.
#[test]
fn the_height_gate_takes_the_lowest_corner() {
    let mut asked: Vec<(i32, i32)> = Vec::new();
    let point = end_city::generation_point(83, 62, Rotation::Clockwise90, |x, z| {
        asked.push((x, z));
        Some(if (x, z) == (1330, 1004) { 61 } else { 70 })
    });
    assert_eq!(point, Some((1335, 61, 999)), "the lowest corner wins");
    assert_eq!(
        asked,
        vec![(1335, 999), (1335, 1004), (1330, 999), (1330, 1004)],
        "CLOCKWISE_90 flips dx and leaves dz alone"
    );
    assert_eq!(
        end_city::generation_point(83, 62, Rotation::None, |_, _| Some(59)),
        None,
        "y < 60 is refused"
    );
}
