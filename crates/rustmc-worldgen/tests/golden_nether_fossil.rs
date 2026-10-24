//! `nether_fossil` placement, against the starts vanilla saved into its own
//! chunks.
//!
//! Vectors: `crates/rustmc-worldgen/golden/nether_fossil_starts.json`, from
//! `ci/nether-fossil-golden.sh`, which reshapes
//! `structures.starts.minecraft:nether_fossil.Children[]` out of
//! `oracle-work/van-nether-20260722-r7`.
//!
//! # This structure has an unusually complete oracle, and it is worth saying
//! # why that is rare
//!
//! Most structures in this port are gated by the blocks they end up writing,
//! which conflates the placement, the piece layout and the block rules into
//! one number. `nether_fossil` is different: vanilla *saves its decision* —
//! the chunk, the template it chose, the rotation it drew, the position it
//! settled on and the resulting box — and that record is in the reference
//! world already, needing no JVM to re-run and no assumption to read.
//!
//! So every link in the chain is checked here at once and separately from
//! the blocks:
//!
//! ```text
//!   which chunks      random_spread, spacing 2 / separation 1 / salt
//!   which biome       #minecraft:has_structure/nether_fossil
//!   what the ground   getBaseColumn -> the ledge walk
//!     looks like
//!   how high          uniform [absolute 32, below_top 2]
//!   which fossil      FOSSILS[nextInt(14)], AFTER the rotation draw
//!   which way round   Rotation.values()[nextInt(4)]
//!   how big           template.getBoundingBox(rotation, pos)
//! ```
//!
//! A failure in any one of them moves a position, a name or a box, and the
//! comparison below is exact on all four.
//!
//! # What this does NOT check
//!
//! The blocks. A fossil's bones are written by the template placer with
//! `BlockIgnoreProcessor.STRUCTURE_AND_AIR`, and nothing here places one.
//! This gate says the right fossil is decided in the right place; it says
//! nothing about what lands there.

use std::collections::BTreeMap;
use std::path::Path;

const VECTORS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/golden/nether_fossil_starts.json"
);

#[test]
fn our_nether_fossil_starts_are_vanillas() {
    let path = Path::new(VECTORS);
    assert!(
        path.exists(),
        "no nether fossil vectors at {VECTORS} — run ci/nether-fossil-golden.sh"
    );
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("readable")).expect("json");
    let seed = doc["seed"].as_i64().expect("seed");
    let wanted = doc["starts"].as_array().expect("starts");
    assert!(
        wanted.len() >= 30,
        "the reference should carry thirty starts, not {}",
        wanted.len()
    );

    // The reference's own extent. Ours is generated over a window and the
    // reference covers a wider square, so the comparison is restricted to
    // the box both instruments actually see — computed from the golden
    // rather than hardcoded, so a regenerated reference does not silently
    // narrow the gate.
    let (mut lo_x, mut hi_x, mut lo_z, mut hi_z) = (i64::MAX, i64::MIN, i64::MAX, i64::MIN);
    for start in wanted {
        let chunk = start["chunk"].as_array().expect("chunk");
        let (x, z) = (chunk[0].as_i64().expect("x"), chunk[1].as_i64().expect("z"));
        lo_x = lo_x.min(x);
        hi_x = hi_x.max(x);
        lo_z = lo_z.min(z);
        hi_z = hi_z.max(z);
    }

    #[allow(clippy::cast_possible_truncation)]
    let area = rustmc_worldgen::pipeline::Area::square(64, 64, 7);
    let ours = worldgen_assembly::generate::nether_fossil_report(seed, area)
        .expect("the nether fossil report");

    let mut mine: BTreeMap<(i64, i64), String> = BTreeMap::new();
    for fossil in &ours {
        let (cx, cz) = (
            i64::from(fossil.start_chunk.0),
            i64::from(fossil.start_chunk.1),
        );
        if cx < lo_x || cx > hi_x || cz < lo_z || cz > hi_z {
            continue;
        }
        let b = fossil.bbox;
        mine.insert(
            (cx, cz),
            format!(
                "{} {:?} {},{},{} [{} {} {} {} {} {}]",
                fossil.start.template,
                fossil.start.rotation,
                fossil.start.pos.0,
                fossil.start.pos.1,
                fossil.start.pos.2,
                b.min_x,
                b.min_y,
                b.min_z,
                b.max_x,
                b.max_y,
                b.max_z
            ),
        );
    }

    let mut theirs: BTreeMap<(i64, i64), String> = BTreeMap::new();
    for start in wanted {
        let chunk = start["chunk"].as_array().expect("chunk");
        let (cx, cz) = (chunk[0].as_i64().expect("x"), chunk[1].as_i64().expect("z"));
        let pos = start["pos"].as_array().expect("pos");
        let bbox = start["bbox"].as_array().expect("bbox");
        // Vanilla spells the enum `CLOCKWISE_180`; this port spells it
        // `Clockwise180`. Normalised here rather than in the golden, so the
        // golden stays a faithful transcription of vanilla's own bytes.
        let rotation = match start["rotation"].as_str().expect("rotation") {
            "NONE" => "None",
            "CLOCKWISE_90" => "Clockwise90",
            "CLOCKWISE_180" => "Clockwise180",
            "COUNTERCLOCKWISE_90" => "Counterclockwise90",
            other => panic!("unknown rotation {other}"),
        };
        assert_eq!(
            start["ground_level_delta"].as_i64(),
            Some(0),
            "a nether fossil is a plain TemplateStructurePiece, so vanilla \
             saves GD=0; a non-zero one here means the piece is not the kind \
             this port assumes and the beard's `PieceShape::Plain` arm is wrong"
        );
        theirs.insert(
            (cx, cz),
            format!(
                "{} {} {},{},{} [{} {} {} {} {} {}]",
                start["template"].as_str().expect("template"),
                rotation,
                pos[0].as_i64().expect("x"),
                pos[1].as_i64().expect("y"),
                pos[2].as_i64().expect("z"),
                bbox[0].as_i64().expect("b"),
                bbox[1].as_i64().expect("b"),
                bbox[2].as_i64().expect("b"),
                bbox[3].as_i64().expect("b"),
                bbox[4].as_i64().expect("b"),
                bbox[5].as_i64().expect("b"),
            ),
        );
    }

    let missing: Vec<_> = theirs.keys().filter(|k| !mine.contains_key(k)).collect();
    let extra: Vec<_> = mine.keys().filter(|k| !theirs.contains_key(k)).collect();
    assert!(
        missing.is_empty(),
        "vanilla places a nether fossil in {} chunk(s) we do not: {missing:?}",
        missing.len()
    );
    assert!(
        extra.is_empty(),
        "we place a nether fossil in {} chunk(s) vanilla does not: {extra:?}",
        extra.len()
    );

    let mismatches: Vec<String> = theirs
        .iter()
        .filter(|(chunk, want)| &&mine[*chunk] != want)
        .map(|(chunk, want)| {
            format!(
                "chunk {chunk:?}\n    vanilla {want}\n    ours    {}",
                mine[chunk]
            )
        })
        .collect();
    assert!(
        mismatches.is_empty(),
        "{} of {} starts differ from vanilla's saved decision:\n  {}",
        mismatches.len(),
        theirs.len(),
        mismatches
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}
