//! Why `DECORATION_MARGIN` does not converge at seed 0 — which stage moves,
//! and where in the area the movement is.
//!
//! `crates/rustmc-server/tests/margin_probe.rs` found that chunk (0,0) at
//! seed 0 reads a different top block depending on how big the area around it
//! was — 74, 74, 62, 74, 62, 62 over contexts of two to seven rings. It
//! oscillates rather than settling, which no "the margin is one ring too
//! small" story explains. `RUSTMC_TRACE_WRITE=8,74,8` names the writer:
//!
//! ```text
//!   WRITE (8,74,8) <- minecraft:oak_leaves   by minecraft:trees_birch_and_oak_leaf_litter
//!                                            from chunk (0, 0)
//! ```
//!
//! **The chunk's own feature**, not a reaching neighbour. So the question is
//! not what writes into (0,0) from far away; it is what makes (0,0)'s own
//! decoration decide differently, and the only context-dependent input a
//! feature has is the blocks its predecessors already wrote.
//!
//! This probe asks two things the server-side one cannot:
//!
//! 1. **Does the terrain move too?** Same chunk, same widening contexts,
//!    stopping at `Through::Carvers`. If that column is all zeroes the
//!    oscillation is decoration and nothing else — noise, biomes, surface and
//!    carvers are per-chunk pure and the margin is irrelevant to them.
//! 2. **Where does the movement live?** Consecutive contexts are compared
//!    over every chunk they share, not just (0,0). The pipeline decorates in
//!    x-then-z order, so if the cause is a chain of decisions running through
//!    already-decorated predecessors, the differences must be worst at the
//!    low-x edge of the shared square and fade inward.
//!
//! # What it answered, 2026-07-28
//!
//! ```text
//!   seed         Through::Carvers       Through::Decoration
//!   0            0, 0, 0, 0             24, 3760, 6352, 13306
//!   20260722     0, 0, 0, 0             0, 0, 0, 0
//!   31415926     0, 0, 0, 0             0, 0, 0, 0
//! ```
//!
//! Terrain is per-chunk pure everywhere. Both seeds this project measures
//! parity at are byte-stable through decoration as well, over 49 delivered
//! chunks from two rings to six — so no parity number in `docs/WORKLIST.md`
//! rests on the seed-0 finding.
//!
//! # RE-MEASURED 2026-08-12, and the decoration column no longer reproduces
//!
//! The `Carvers` column does, at every seed and out to a 14-ring context. The
//! decoration column does not — seed 0 reads **2, 81, 2736, 7383** where it
//! read 24, 3760, 6352, 13306 — because the generator has moved a long way
//! since (icebergs, lakes, leaf litter). The numbers above are kept as the
//! dated reading they were; do not quote them as current.
//!
//! **And the zeroes above are four rungs long, which is not the same as
//! zero.** `31415926` reads `0, 0, 0, 0` over the rungs this probe used to
//! run and **833, 1917, 2916, 2439** over the four after them. The ladder
//! stopped before the effect started, and a ladder that stops early reports a
//! clean seed. Of the three seeds this header called stable, only 20260722
//! survives the longer one. Seed **12345** — which `docs/DETERMINISM.md` §17
//! adjudicates on and which did not exist as a parity seed when that line was
//! written — moves at the very first rung.
//!
//! ```text
//!   seed        rungs, Through::Decoration, delivered square, 2026-08-12
//!   0           2, 81, 2736, 7383, 14719, 20217, 45349, 45145
//!   12345       40, 1397, 3406, 6501, 11127, 11797, 17194, 309, 11, 1005, 5514, 1937
//!   31415926    0, 0, 0, 0, 833, 1917, 2916, 2439
//!   20260722    0, 0, 0, 0, 0, 0, 0, 0
//! ```
//!
//! Read `docs/DETERMINISM.md` §31 before quoting anything from this
//! instrument: no margin width converges, the delivered square at seed 12345
//! is still moving at a 14-ring context, and the reason is structural.
//!
//! At seed 0 the movement **grows** with the area and spreads across the
//! whole delivered square, which is the opposite of a margin that is one ring
//! short. The cause is that decoration both writes across chunk boundaries
//! and *reads* the world, so a chunk's decoration depends on what its
//! already-decorated predecessors left; `Pipeline::run_stage` walks x-then-z,
//! so that chain runs inward from the edge of the generated area without
//! limit. `DECORATION_MARGIN` bounds a feature's direct **write** reach, not
//! its **decision** reach — raising it to 3 will not settle this. Vanilla has
//! the same property (see WORKLIST, "Where item 1 actually stands": two
//! vanilla worlds of one seed differ purely over which chunk decorated
//! first), and generate-once status-based generation is the only fix.
//!
//! **"Raising it to 3 will not settle this" was right and was an argument.**
//! It is now a measurement: injected at margins 1, 2 and 4 the strengthened
//! gate in `crates/rustmc-server/tests/tiled_world.rs` reads 5,753, 2,201 and
//! 1,130 — decreasing, never zero. The reason no width works is that a tile's
//! rim is at depth `m` below the outermost decorated ring **for every m**.
//! `docs/DETERMINISM.md` §31.3.
//!
//! Env-gated: it generates ten worlds up to 15x15 chunks by default, and
//! `RUSTMC_MARGIN_MAX_RADIUS` takes it further at `(2r + 5)^2` chunks a rung.
//! An instrument, not a gate.
//!
//! ```sh
//!   RUSTMC_MARGIN_CONTEXT=1 cargo test --release -p worldgen-assembly \
//!       --test probe_margin_context -- --nocapture
//!   RUSTMC_MARGIN_CONTEXT=1 RUSTMC_MARGIN_SEED=20260722 ...
//!   RUSTMC_MARGIN_CONTEXT=1 RUSTMC_MARGIN_MAX_RADIUS=12 ...   # to depth 14
//! ```

use rustmc_worldgen::canvas::Canvas;
use rustmc_worldgen::pipeline::{Area, Through};

const MIN_Y: i32 = -64;
const MAX_Y: i32 = 320;

/// Blocks of chunk `(cx, cz)` that differ between two canvases.
fn chunk_diff(a: &Canvas, b: &Canvas, cx: i32, cz: i32) -> usize {
    let mut moved = 0;
    for x in cx * 16..cx * 16 + 16 {
        for z in cz * 16..cz * 16 + 16 {
            for y in MIN_Y..MAX_Y {
                if a.block_at(x, y, z) != b.block_at(x, y, z) {
                    moved += 1;
                }
            }
        }
    }
    moved
}

fn seed() -> i64 {
    std::env::var("RUSTMC_MARGIN_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

/// How far the ladder climbs, from `RUSTMC_MARGIN_MAX_RADIUS`.
///
/// Four by default, which is what this probe ran at from 2026-07-28 to
/// 2026-08-12 and is the reason the ladder's *shape* was never read: four
/// steps of a growing sequence look like four steps of any other. A bad value
/// is a refusal rather than a fallback to the default — ORACLE rule 30, an
/// unrecognised knob is not a turned one, and a silent fallback here would
/// print a ladder shorter than the one that was asked for while saying
/// nothing about it.
fn max_radius() -> i32 {
    let Ok(raw) = std::env::var("RUSTMC_MARGIN_MAX_RADIUS") else {
        return 4;
    };
    let parsed: i32 = raw
        .parse()
        .unwrap_or_else(|_| panic!("RUSTMC_MARGIN_MAX_RADIUS={raw} is not an integer"));
    assert!(
        parsed >= 1,
        "RUSTMC_MARGIN_MAX_RADIUS={parsed} leaves no pair to compare"
    );
    parsed
}

#[test]
fn which_stage_moves_when_the_context_widens() {
    if std::env::var("RUSTMC_MARGIN_CONTEXT").is_err() {
        eprintln!("skipped: set RUSTMC_MARGIN_CONTEXT=1 — an instrument, not a gate");
        return;
    }
    if !worldgen_assembly::have_extract() {
        eprintln!("skipped: no vanilla extract");
        return;
    }
    let seed = seed();
    let max_radius = max_radius();
    eprintln!("seed {seed}, ladder to radius {max_radius}");

    for through in [Through::Carvers, Through::Decoration] {
        eprintln!("\n=== through {through:?}, seed {seed} ===");
        let mut previous: Option<(i32, Canvas)> = None;
        for radius in 0..=max_radius {
            let world =
                worldgen_assembly::generate::generate(seed, Area::square(0, 0, radius), through)
                    .expect("generates");
            let rings = radius + rustmc_worldgen::pipeline::DECORATION_MARGIN;
            match previous.take() {
                None => eprintln!("context {rings} rings: the baseline"),
                Some((prev_radius, prev)) => {
                    // Every chunk the smaller run *delivered* — its inner
                    // area, the part it claims is finished.
                    let mut per_chunk: Vec<((i32, i32), usize)> = Area::square(0, 0, prev_radius)
                        .chunks()
                        .map(|(cx, cz)| ((cx, cz), chunk_diff(&prev, &world.canvas, cx, cz)))
                        .filter(|(_, n)| *n > 0)
                        .collect();
                    let total: usize = per_chunk.iter().map(|(_, n)| n).sum();
                    // The centre chunk on its own line, every line, because it
                    // is the only chunk whose distance to the outermost
                    // decorated ring is exactly the context width — so its
                    // column, and only its column, is the margin ladder. The
                    // aggregate is not: the delivered set grows with the
                    // radius, so a rising total can be more chunks rather than
                    // a wider dependence.
                    let centre = per_chunk
                        .iter()
                        .find(|((cx, cz), _)| *cx == 0 && *cz == 0)
                        .map_or(0, |(_, n)| *n);
                    per_chunk.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
                    eprintln!(
                        "context {rings} rings: {total} blocks moved in the {} \
                         delivered chunks of the {}-ring context, across {} of them \
                         — CENTRE (0,0) {centre}",
                        Area::square(0, 0, prev_radius).chunk_count(),
                        prev_radius + rustmc_worldgen::pipeline::DECORATION_MARGIN,
                        per_chunk.len()
                    );
                    for ((cx, cz), n) in per_chunk.iter().take(9) {
                        eprintln!("    chunk ({cx:3},{cz:3})  {n:6}");
                    }
                }
            }
            previous = Some((radius, world.canvas));
        }
    }
    eprintln!(
        "\nA zero column under `Carvers` means terrain is per-chunk pure and the \
         instability is decoration alone. Differences concentrated at low x are \
         the x-then-z decoration order carrying a decision inward."
    );
}
