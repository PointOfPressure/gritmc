//! `fortress`, gated against vanilla's own saved `Children[]`.
//!
//! Reference: `NetherFortressStructure.generatePieces`, `NetherFortressPieces`
//! and its fifteen piece classes, `StructurePiecesBuilder.moveInsideHeights`.
//!
//! # What is compared
//!
//! Every piece, in the order vanilla saved it: the id, `BB`, `O`, `GD`, and
//! the two fields a fortress adds — a filler's `Seed`, which is the bare
//! `nextInt()` its constructor drew, and a corridor turn's `Chest`, which is
//! `nextInt(3) == 0`. The seeds are the sharp part: every dead end of the walk
//! records the stream there, so a walk that matches seed for seed consumed
//! the stream exactly as vanilla did, which no block diff can say.
//!
//! `Chest` is weaker than it looks: vanilla saves the start after its chunks
//! were decorated, and `postProcess` clears the flag once the chest is in, so
//! a saved false may be a placed chest. Only a saved true binds ours.
//!
//! # The two starts, and why two
//!
//! Seed 31415926 has a fortress at chunk (-189, 198) — 56 pieces, the one
//! whose bridges reach into the c(-200,200) forest window — and one at
//! (12, 3), 142 pieces. Each was harvested three times
//! (`van-nether-31415926-c-189_198-open-{a,b,c}`, `...-c12_3-open-{a,b,c}`).
//! The layout is a pure function of the seed and the chunk, so the three
//! harvests of a start must agree with each other exactly — asserted, because
//! a reference that disagreed with its twins would make this a comparison
//! against noise.
//!
//! The fixtures go through `rustmc_data::fixture::available`, which panics
//! when they are missing unless `RUSTMC_FIXTURES=optional`.

use std::path::Path;

use rustmc_worldgen::structure::fortress;

const SEED: i64 = 31_415_926;

/// (start chunk, reference prefix, piece count vanilla saved).
const STARTS: [((i32, i32), &str, usize); 2] = [
    ((-189, 198), "van-nether-31415926-c-189_198-open-", 56),
    ((12, 3), "van-nether-31415926-c12_3-open-", 142),
];

/// One saved piece, as the fields this gate compares.
type Row = (String, [i32; 6], i32, i32, Option<i32>, Option<bool>);

fn vanilla_start(dir: &Path, chunk: (i32, i32)) -> Vec<Row> {
    assert!(
        rustmc_data::fixture::available(
            dir,
            "vanilla fortress reference",
            "ci/gen-vanilla-region.sh"
        ),
        "missing {}",
        dir.display()
    );
    for entry in std::fs::read_dir(dir).expect("reference directory") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("mca") {
            continue;
        }
        for column in rustmc_world::anvil::read_region(&path).expect("read a region file") {
            for start in &column.structure_starts {
                if start.id != "minecraft:fortress" || (start.chunk_x, start.chunk_z) != chunk {
                    continue;
                }
                return start
                    .pieces
                    .iter()
                    .map(|p| {
                        (
                            p.id.to_lowercase(),
                            p.bounding_box,
                            p.orientation,
                            p.depth,
                            p.seed,
                            p.chest,
                        )
                    })
                    .collect();
            }
        }
    }
    panic!(
        "{} holds no fortress start at chunk {chunk:?} — the reference is present \
         and the start is not, which is not a reason to be quiet",
        dir.display()
    );
}

fn ours(chunk: (i32, i32)) -> Vec<Row> {
    fortress::generate(SEED, chunk)
        .into_iter()
        .map(|p| {
            let bb = p.piece.bounding_box;
            let seed = (p.kind == fortress::Kind::BridgeEndFiller).then_some(p.seed);
            let chest = matches!(
                p.kind,
                fortress::Kind::CastleSmallCorridorLeftTurn
                    | fortress::Kind::CastleSmallCorridorRightTurn
            )
            .then_some(p.chest);
            (
                p.piece.id.to_string(),
                [bb.min_x, bb.min_y, bb.min_z, bb.max_x, bb.max_y, bb.max_z],
                p.piece.facing.map_or(-1, |f| f.saved_value()),
                p.piece.depth,
                seed,
                chest,
            )
        })
        .collect()
}

#[test]
fn the_fortresses_match_vanillas_own_children() {
    let root = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../oracle-work"));
    for (chunk, prefix, count) in STARTS {
        let refs: Vec<Vec<Row>> = ["a", "b", "c"]
            .iter()
            .map(|t| vanilla_start(&root.join(format!("{prefix}{t}")), chunk))
            .collect();
        assert_eq!(
            refs[0].len(),
            count,
            "{prefix}a: the start's piece count moved"
        );
        for (i, other) in refs.iter().enumerate().skip(1) {
            assert_eq!(
                &refs[0], other,
                "{prefix}a and harvest {i} disagree about the start at {chunk:?}: the \
                 layout is supposed to be a pure function of seed and chunk"
            );
        }
        let vanilla = &refs[0];
        let ours = ours(chunk);
        let mut first_diff = None;
        let mut chests_placed = 0usize;
        for (i, (v, o)) in vanilla.iter().zip(&ours).enumerate() {
            // `Chest` is saved AFTER generation: `postProcess` clears
            // `isNeedingChest` once it has placed the chest, so vanilla's
            // false is "never needed one" or "already placed it". Only a
            // vanilla true is a claim about the draw.
            let chest_ok = match (v.5, o.5) {
                (Some(true), Some(false)) => false,
                (Some(false), Some(true)) => {
                    chests_placed += 1;
                    true
                }
                (a, b) => a == b,
            };
            if (&v.0, v.1, v.2, v.3, v.4) != (&o.0, o.1, o.2, o.3, o.4) || !chest_ok {
                first_diff = Some(format!("piece {i}:\n  vanilla {v:?}\n  ours    {o:?}"));
                break;
            }
        }
        assert!(
            first_diff.is_none() && vanilla.len() == ours.len(),
            "fortress at {chunk:?}: vanilla saved {} pieces, ours {}; first difference: {}",
            vanilla.len(),
            ours.len(),
            first_diff.unwrap_or_else(|| "none before the shorter list ends".to_string())
        );
        let fillers = vanilla.iter().filter(|r| r.4.is_some()).count();
        assert!(
            fillers > 0,
            "the start at {chunk:?} has no filler, so no seed pins the stream"
        );
        println!(
            "FORTRESS {chunk:?}: {} pieces equal to vanilla's, {fillers} filler seeds among them, \
             {chests_placed} chest(s) vanilla had already placed",
            ours.len()
        );
    }
}
