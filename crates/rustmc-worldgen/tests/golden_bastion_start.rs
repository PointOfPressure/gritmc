//! `bastion_remnant`, gated against vanilla's own saved `Children[]`.
//!
//! Reference: `nether_complexes.json`, `bastion_remnant.json`, `fortress.json`,
//! `JigsawStructure.findGenerationPoint`, `JigsawPlacement.addPieces`,
//! `NetherFortressStructure.findGenerationPoint`, `StructureStart.placeInChunk`.
//!
//! # Why a LAYOUT gate and not only a block one
//!
//! The r=16 nether window measures blocks and closes the two starts to 27,
//! which is a strong result and a weak gate: it is one window, it takes
//! minutes to take, and by the time it moves it cannot say whether the
//! assembly changed or the placement did. This fixture is the other half —
//! element, rotation, template position, ground level delta, projection and
//! bounding box, piece for piece, out of vanilla's own saved starts.
//!
//! The fixture is a **vanilla region directory**, gitignored and jar-derived.
//! A missing one goes through `rustmc_data::fixture::available`, which
//! **panics by default** and skips only under `RUSTMC_FIXTURES=optional` —
//! the same contract `anvil_bridge`'s vanilla-region test uses, and it is
//! here because the first version of this file wrote its own `eprintln!` and
//! `return`, pointed at a path with an extra `region/` in it, and passed
//! green while comparing nothing. That is the exact shape
//! `ci/structure-surface.py --fixtures` counts three of elsewhere in this
//! tree; writing a fourth while porting the structure that needs it would
//! have been remarkable.
//!
//! It fails rather than skips when the directory is present and the starts
//! inside it are not, because "the reference has no bastion" and "we could
//! not read the reference" are different facts and only one is a reason to
//! be quiet.

use std::path::Path;

/// The r=16 nether reference, whose four region files hold **five**
/// `bastion_remnant` starts and not the two the r=16 square contains.
///
/// That is not a surprise once stated: a region file is 32x32 chunks, the
/// four of them cover chunks 32..95, and vanilla saves `structures.starts`
/// into every chunk it touched — including the border ring it generated only
/// far enough to serve the square. The two the block diff at `--bbox 48 48 80
/// 80` sees are (55,63) and (70,49); the other three are (45,43), (87,68) and
/// (93,46).
const REF: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../oracle-work/gen-wide-20260722-r16-nether-a"
);
const SEED: i64 = 20_260_722;

/// **The square this gate compares over is the one WE search, not the one the
/// diff measures.**
///
/// `bastion_starts` walks `area` widened by six chunks, because a 105-piece
/// start spans seven chunks and its own placement chunk is not the one it is
/// widest in. For `Area::square(64, 64, 16)` that is chunks 48..80 widened to
/// **42..86**. Vanilla's region files happen to cover 32..95, so inside
/// 42..86 both sides have data and the comparison is total; outside it, one
/// side or the other is silent for a reason that is not a defect.
///
/// Getting this wrong in either direction produces a green gate on a broken
/// port. Compare over 48..80 and the port's third start is unexamined;
/// compare over 32..95 and a correct port fails for not having searched
/// chunks it was never asked about.
const MIN_CHUNK: i32 = 48 - 6;
const MAX_CHUNK: i32 = 80 + 6;

/// `Rotation`'s vanilla enum-constant name, which is what the NBT holds.
///
/// `format!("{:?}", ..).to_uppercase()` gives `CLOCKWISE90` and vanilla writes
/// `CLOCKWISE_90`. That one missing underscore is what the skipped first
/// version of this file was hiding: every other field of all 194 pieces
/// already matched.
fn rotation_name(rotation: rustmc_worldgen::structure::template::Rotation) -> &'static str {
    use rustmc_worldgen::structure::template::Rotation;
    match rotation {
        Rotation::None => "NONE",
        Rotation::Clockwise90 => "CLOCKWISE_90",
        Rotation::Clockwise180 => "CLOCKWISE_180",
        Rotation::Counterclockwise90 => "COUNTERCLOCKWISE_90",
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Piece {
    element: String,
    rotation: String,
    pos: (i32, i32, i32),
    ground_level_delta: i32,
    bbox: (i32, i32, i32, i32, i32, i32),
}

/// One saved start: its chunk and its pieces in the order vanilla wrote them.
type Start = ((i32, i32), Vec<Piece>);

/// Vanilla's saved starts, read out of the reference's own chunk NBT.
fn vanilla_starts() -> Option<Vec<Start>> {
    // The reference IS the region directory — the `.mca` files sit beside
    // `HARVEST-ORDER`, with no `region/` under it. `region-differ` takes it
    // that way too, and OUR dumps are the ones that carry a `region/`.
    let dir = Path::new(REF);
    if !rustmc_data::fixture::available(dir, "vanilla nether reference", "ci/gen-vanilla-region.sh")
    {
        return None;
    }
    assert!(dir.is_dir(), "{} is not a directory", dir.display());
    let mut out: Vec<Start> = Vec::new();
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .expect("read the reference region dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("mca"))
        .collect();
    files.sort();
    for path in files {
        for chunk in rustmc_world::anvil::read_region(&path).expect("read a region file") {
            for start in &chunk.structure_starts {
                if start.id != "minecraft:bastion_remnant" {
                    continue;
                }
                let pieces = start
                    .pieces
                    .iter()
                    .map(|p| Piece {
                        element: p.pool_element.clone().unwrap_or_default(),
                        rotation: p.rotation.clone().unwrap_or_default(),
                        pos: p.pos.map(|v| (v[0], v[1], v[2])).unwrap_or_default(),
                        ground_level_delta: p.ground_level_delta.unwrap_or(i32::MIN),
                        bbox: {
                            let b = p.bounding_box;
                            (b[0], b[1], b[2], b[3], b[4], b[5])
                        },
                    })
                    .collect();
                if !(MIN_CHUNK..=MAX_CHUNK).contains(&start.chunk_x)
                    || !(MIN_CHUNK..=MAX_CHUNK).contains(&start.chunk_z)
                {
                    continue;
                }
                out.push(((start.chunk_x, start.chunk_z), pieces));
            }
        }
    }
    out.sort_by_key(|(c, _)| *c);
    Some(out)
}

#[test]
fn the_bastions_match_vanillas_own_children() {
    let Some(want) = vanilla_starts() else { return };
    // The reference is present, so its content is now an assertion and not a
    // skip: a fixture whose shape has changed must fail here rather than be
    // silently compared against.
    assert_eq!(
        want.iter().map(|(c, p)| (*c, p.len())).collect::<Vec<_>>(),
        vec![((45, 43), 85), ((55, 63), 105), ((70, 49), 89)],
        "the reference does not hold the three starts this gate was written for"
    );

    if !worldgen_assembly::have_extract() {
        return;
    }
    let area = rustmc_worldgen::pipeline::Area::square(64, 64, 16);
    let mut got =
        worldgen_assembly::generate::bastion_report(SEED, area).expect("assemble the bastions");
    got.sort_by_key(|b| b.start_chunk);

    // Ours, bounded to the searched square — see MIN_CHUNK. Bounding rather
    // than asserting a total, because the WEIGHTED CONTENTION is what this
    // set tests: `nether_complexes` gives the bastion three weight against
    // the fortress's two, and running the bastion alone puts one on every
    // placement chunk. A port that skipped the fortress attempt would have
    // MORE starts here, not fewer, and would still match on every piece of
    // the ones it got right.
    let mine: Vec<_> = got
        .iter()
        .filter(|b| {
            (MIN_CHUNK..=MAX_CHUNK).contains(&b.start_chunk.0)
                && (MIN_CHUNK..=MAX_CHUNK).contains(&b.start_chunk.1)
        })
        .collect();
    assert_eq!(
        mine.iter()
            .map(|b| (b.start_chunk, b.pieces.len()))
            .collect::<Vec<_>>(),
        vec![((45, 43), 85), ((55, 63), 105), ((70, 49), 89)],
        "the set of starts inside the searched square differs from vanilla's"
    );

    for ((chunk, want_pieces), bastion) in want.iter().zip(mine.iter()) {
        assert_eq!(*chunk, bastion.start_chunk);
        for (i, (w, g)) in want_pieces.iter().zip(bastion.pieces.iter()).enumerate() {
            let element = match &g.element.kind {
                rustmc_worldgen::structure::jigsaw::ElementKind::Single { location, .. } => {
                    location.clone()
                }
                other => panic!("piece {i} of {chunk:?} is not a single element: {other:?}"),
            };
            let b = g.bbox;
            let mine = Piece {
                element,
                rotation: rotation_name(g.rotation).to_string(),
                pos: g.pos,
                ground_level_delta: g.ground_level_delta,
                bbox: (b.min_x, b.min_y, b.min_z, b.max_x, b.max_y, b.max_z),
            };
            assert_eq!(&mine, w, "piece {i} of the bastion at {chunk:?}");
        }
    }
}

/// The two starts vanilla saved that this port does **not** find, and why
/// that is correct.
///
/// (87,68) and (93,46) are inside the reference's region files and outside
/// the square `bastion_starts` searches for an `Area::square(64, 64, 16)`.
/// Neither can write a block into chunks 48..80: a bastion reaches at most
/// `max_distance_from_center` = 80 blocks, five chunks, so a start at chunk 87
/// cannot reach chunk 80's far edge. Asserted rather than reasoned, because
/// the reach constant and this claim have to move together.
#[test]
fn the_starts_beyond_the_searched_square_cannot_reach_the_measured_one() {
    let dir = Path::new(REF);
    if !rustmc_data::fixture::available(dir, "vanilla nether reference", "ci/gen-vanilla-region.sh")
    {
        return;
    }
    let mut beyond: Vec<(i32, i32)> = Vec::new();
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .expect("read the reference dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("mca"))
        .collect();
    files.sort();
    for path in files {
        for chunk in rustmc_world::anvil::read_region(&path).expect("read a region file") {
            for start in &chunk.structure_starts {
                if start.id != "minecraft:bastion_remnant" {
                    continue;
                }
                if !(MIN_CHUNK..=MAX_CHUNK).contains(&start.chunk_x)
                    || !(MIN_CHUNK..=MAX_CHUNK).contains(&start.chunk_z)
                {
                    beyond.push((start.chunk_x, start.chunk_z));
                }
            }
        }
    }
    beyond.sort_unstable();
    assert_eq!(beyond, vec![(87, 68), (93, 46)]);

    // 80 blocks is five chunks, and `bastion_starts` searches six beyond the
    // area, so nothing it declines to look for can reach the area at all.
    const MEASURED_MAX: i32 = 80;
    const REACH_CHUNKS: i32 = 80 / 16;
    for (cx, cz) in beyond {
        assert!(
            cx - REACH_CHUNKS > MEASURED_MAX || cz - REACH_CHUNKS > MEASURED_MAX,
            "the start at ({cx}, {cz}) could reach the measured square and is not being looked for"
        );
    }
}
