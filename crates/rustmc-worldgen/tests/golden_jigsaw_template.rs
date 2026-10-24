//! The template half of the jigsaw, gated against vanilla's own piece list.
//!
//! `JigsawPlacement` asks a template three things: where its jigsaw blocks
//! are, which way they point, and what box the template occupies under a
//! rotation. Every one of those is a place a port can be quietly wrong — a
//! rotation applied about the wrong pivot translates a whole village, and a
//! jigsaw list in the wrong order changes every draw after it.
//!
//! All three can be checked **before a line of the assembler exists**, because
//! `golden/village_31415926.json` records, for each of 175 placed pieces, the
//! template it used, the rotation it took, the position it landed at and the
//! bounding box vanilla computed. Feeding the first three into
//! `Template::bounding_box` and requiring the fourth back is a 175-case gate on
//! the transform, the size convention and the corner rule at once.
//!
//! That is worth more than it looks. The pieces cover all four rotations and
//! templates from 1x2x1 to 19x20x19, so a formula that is right for square
//! templates and wrong for oblong ones — the classic pivot error — cannot pass.

use std::path::Path;

use rustmc_worldgen::structure::pieces::BoundingBox;
use rustmc_worldgen::structure::template::{Facing, Rotation, Template};

const VECTOR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/golden/village_31415926.json");

const TEMPLATES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../oracle-work/vanilla-26.2/generated-data/data/minecraft/structure"
);

fn rotation(name: &str) -> Rotation {
    match name {
        "NONE" => Rotation::None,
        "CLOCKWISE_90" => Rotation::Clockwise90,
        "CLOCKWISE_180" => Rotation::Clockwise180,
        "COUNTERCLOCKWISE_90" => Rotation::Counterclockwise90,
        other => panic!("unknown rotation {other}"),
    }
}

fn load(name: &str) -> Template {
    let rel = name.strip_prefix("minecraft:").expect("namespaced");
    let path = Path::new(TEMPLATES).join(format!("{rel}.nbt"));
    let file = std::fs::File::open(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut reader = flate2::read::GzDecoder::new(file);
    let (_n, root) = rustmc_world::nbt::read_named_root(&mut reader).expect("template NBT");
    Template::from_nbt(&root).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

/// Every placed piece's bounding box, recomputed from its template.
#[test]
fn the_rotated_bounding_box_is_the_one_vanilla_saved() {
    if !rustmc_data::fixture::available(
        Path::new(TEMPLATES),
        "structure templates",
        "ci/vanilla-server.sh data",
    ) {
        return;
    }
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(VECTOR).expect("vector")).expect("json");
    let mut checked = 0usize;
    let mut expanded = 0usize;
    let mut rotations = std::collections::BTreeSet::new();
    for piece in doc["pieces"].as_array().expect("pieces") {
        // A `feature_pool_element` has no template and no size; its box is a
        // single block at its position, which says nothing about a rotation.
        let Some(name) = piece["element"].as_str() else {
            continue;
        };
        let rot = piece["rotation"].as_str().expect("rotation");
        rotations.insert(rot.to_string());
        let pos = piece["pos"].as_array().expect("pos");
        let origin = (
            pos[0].as_i64().unwrap() as i32,
            pos[1].as_i64().unwrap() as i32,
            pos[2].as_i64().unwrap() as i32,
        );
        let bb = piece["bb"].as_array().expect("bb");
        let want = BoundingBox::new(
            bb[0].as_i64().unwrap() as i32,
            bb[1].as_i64().unwrap() as i32,
            bb[2].as_i64().unwrap() as i32,
            bb[3].as_i64().unwrap() as i32,
            bb[4].as_i64().unwrap() as i32,
            bb[5].as_i64().unwrap() as i32,
        );
        let got = load(name).bounding_box(origin, rotation(rot));
        // Five of the six faces are pure transform, and they are asserted
        // exactly. The **top** is not: `Placer.tryPlacingChildren` runs the
        // expansion hack on a village — `useExpansionHack` is true for
        // exactly the five villages and the pillager outpost — and where it
        // fires it calls `box.encapsulate(minX, minY + h, minZ)` *before* the
        // piece is saved. So the sixth face of the file is the template's
        // box plus the hack, and requiring equality there would be requiring
        // this test to implement the assembler.
        //
        // This was found by measurement, not by reading: the first run of
        // this gate failed on `crossroad_02` with a top two blocks low, which
        // is the hack and nothing else.
        assert_eq!(
            (got.min_x, got.min_y, got.min_z, got.max_x, got.max_z),
            (want.min_x, want.min_y, want.min_z, want.max_x, want.max_z),
            "{name} at {origin:?} {rot}"
        );
        assert!(
            got.max_y <= want.max_y,
            "{name} at {origin:?} {rot}: the template is taller than the box vanilla saved"
        );
        if got.max_y < want.max_y {
            expanded += 1;
        }
        checked += 1;
    }
    assert_eq!(checked, 175, "175 template-backed pieces in the reference");
    assert_eq!(rotations.len(), 4, "all four rotations are exercised");
    // A ratchet on the hack itself, measured rather than guessed — the
    // first run of this line said 49 against a guess of 82. Forty-nine of the
    // 175 pieces were grown upward by `tryPlacingChildren`, so an assembler
    // that never expands anything is wrong 49 times, and this number says so
    // before any block diff does.
    assert_eq!(expanded, 49, "pieces the expansion hack grew");
}

/// The jigsaw list of the village's own start piece, in the order the shuffle
/// will be handed.
///
/// Six blocks, and the assertion that matters is that they come out sorted by
/// `(y, x, z)` — `buildInfoList`'s comparator — and not in file order. The
/// pool, the target and the front facing are asserted for the first of them so
/// that a parser which read the payload from the wrong key would fail here and
/// not four hundred draws later.
#[test]
fn the_start_template_lists_its_jigsaws_in_load_order() {
    if !rustmc_data::fixture::available(
        Path::new(TEMPLATES),
        "structure templates",
        "ci/vanilla-server.sh data",
    ) {
        return;
    }
    let template = load("minecraft:village/savanna/zombie/town_centers/savanna_meeting_point_4");
    assert_eq!(template.size, (9, 6, 9));

    let jigsaws = template.jigsaws((0, 0, 0), Rotation::None);
    assert_eq!(jigsaws.len(), 6);
    let order: Vec<(i32, i32, i32)> = jigsaws.iter().map(|j| j.pos).collect();
    let mut sorted = order.clone();
    sorted.sort_by_key(|p| (p.1, p.0, p.2));
    assert_eq!(order, sorted, "jigsaws come out sorted by (y, x, z)");

    let first = &jigsaws[0];
    assert_eq!(first.pos, (7, 0, 8));
    assert_eq!(first.front, Facing::Up);
    assert_eq!(first.top, Facing::North);
    assert_eq!(first.nbt.pool, "minecraft:village/common/cats");
    assert_eq!(first.nbt.name, "minecraft:bottom");
    assert_eq!(first.nbt.target, "minecraft:bottom");
    assert!(first.nbt.rollable, "the bottom jigsaws are rollable");
    assert_eq!(first.nbt.final_state, "minecraft:grass_block");

    // The street jigsaws are the horizontal ones, and they are `aligned` —
    // which is what makes a street's rotation depend on its neighbour's.
    let street = jigsaws
        .iter()
        .find(|j| j.nbt.name == "minecraft:street")
        .expect("a street jigsaw");
    assert!(!street.nbt.rollable);
    assert!(matches!(
        street.front,
        Facing::North | Facing::South | Facing::East | Facing::West
    ));

    // Rotating the template turns both halves of every `orientation`, and
    // moves the block. CLOCKWISE_180 about the pivot (0,0) sends (7,0,8) to
    // (-7,0,-8) and leaves a vertical front alone while turning its top.
    let turned = template.jigsaws((0, 0, 0), Rotation::Clockwise180);
    let same = turned
        .iter()
        .find(|j| j.pos == (-7, 0, -8))
        .expect("the first jigsaw, rotated");
    assert_eq!(same.front, Facing::Up);
    assert_eq!(same.top, Facing::South);
}
