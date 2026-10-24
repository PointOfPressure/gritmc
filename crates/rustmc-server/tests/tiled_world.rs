//! Is a world made of separately-generated tiles the same world?
//!
//! The server generates one square up front and a player can walk off its
//! edge into nothing. The cheapest way to grow it is to **tile**: generate
//! another square beside the first, each with its own decoration margin, and
//! serve whichever one holds the chunk asked for. That is only allowed if the
//! seam is invisible — if the chunks either side of it hold exactly what one
//! big generation covering both would have produced.
//!
//! There is an argument that it must: `DECORATION_MARGIN` exists precisely so
//! that a delivered chunk has had every chunk that can reach it decorated,
//! and decoration is position-seeded, so a chunk generated as tile A's margin
//! and the same chunk generated as tile B's interior should be identical.
//! This project does not ship arguments (rule 9), so this measures it.
//!
//! **It holds at seed 20260722 and it does not hold everywhere.** Both tests
//! below pass — 0 differences — and `tests/margin_probe.rs` then widened the
//! context at **seed 0** and found chunk (0,0) never settling: 74, 74, 62,
//! 74, 62, 62 as the ring count goes 2 to 7, with hundreds of blocks moving
//! each time. So what these tests establish is narrower than it looks: at the
//! seed every parity number in this project is measured at, a tile is a
//! faithful slice of one pass. At another seed it is not, and the fault is
//! `DECORATION_MARGIN` rather than tiling — the same fixed square the server
//! served before had exactly the same exposure.
//!
//! Skipped without the vanilla extract, like every gate that needs it.

use rustmc_server::terrain::StateIds;
use rustmc_server::world::{World, WorldTables};
use rustmc_worldgen::pipeline::Area;

const SEED: i64 = 20260722;

/// One widening comparison: a seed and the chunk the tile is centred on.
///
/// A pair rather than a bare seed because the effect is **localised**, and a
/// centre is as much a condition of it as the seed is. `docs/DETERMINISM.md`
/// §30.3 put the seed-12345 movement in chunk (48,54) of a square centred
/// (64,64) — 23 blocks in one of the 33 chunks of a rim column — so a probe
/// that only ever sits at the origin is sampling one draw from a sparse
/// distribution and calling the miss a pass.
#[derive(Clone, Copy)]
struct Widening {
    seed: i64,
    center: (i32, i32),
    /// What the two rungs must come out at, measured 2026-08-12 at `d049ef5a`.
    ///
    /// `Some(n)` is asserted to the block and `None` only to "some blocks
    /// moved", and which of the two a case gets is a decision about what the
    /// number is *for*:
    ///
    /// * the zeroes at seed 20260722 are exact because every parity figure
    ///   this project publishes is taken at that seed on a square whose
    ///   delivered chunks must not move when the square grows. One block
    ///   there is a finding;
    /// * the 23 at (48,54) is exact because it is `docs/DETERMINISM.md`
    ///   §30.3's 23, reproduced from a 5x5 generation instead of a 37x37
    ///   region dump, down to the first position. It is the tie between this
    ///   gate and that measurement and it is not free to drift;
    /// * the rest are `None`. They are real and they are not quoted anywhere,
    ///   and pinning them would redden every branch that legitimately moves a
    ///   block at seed 12345 or 0 for a number nobody reads.
    ///
    /// A `None` that comes out **zero** is the loud one, and it does not mean
    /// the gate is fine: it means the margin's decision reach was fixed while
    /// nobody was looking. Record the before/after in `docs/DETERMINISM.md`
    /// §31 and make it exact, do not delete the case.
    ///
    /// # 2026-08-12: that zero happened, and this file is deliberately
    /// unchanged
    ///
    /// `docs/DETERMINISM.md` §32 built the fix §31 named — generate-once
    /// status-based generation over one canvas — and under it **every one of
    /// these eight rungs reads 0**. The paragraph above says to make them
    /// exact. They are not, and the reason is the finding rather than an
    /// exception to it: that model is measurably further from vanilla
    /// (8,712 -> 42,873 mismatches against `oracle-work/van-s1-c62` on the
    /// seed-20260722 r=7 window at (64,64); the End city window's
    /// 100.000000% down to 99.998861%), so it ships behind
    /// `RUSTMC_DECORATE_ISOLATED=1` and is **not** what this gate measures.
    ///
    /// What did ship is the pyramid's extent — one ring fewer decorated over
    /// the same terrain — and under it these cases read
    /// **0 / 0 / 66 / 608 / 23 / 986 / 12 / 2,364**, against
    /// 0 / 0 / 40 / 1,397 / 23 / 658 / 2 / 81 before. Every assertion below
    /// still holds without being touched, including the exact 23, so the
    /// gate is left exactly as it was built. **If you are here because you
    /// have made these eight numbers zero, read §32.3 before you celebrate:
    /// zero here has already been bought once and handed back.**
    expect: [Option<usize>; 2],
}

/// Every widening comparison this gate runs.
///
/// Three seeds because a one-seed gate cannot see a seed-dependent defect and
/// this one is seed-dependent; `20260722` is where every parity number in the
/// project is taken, `12345` is the seed `docs/DETERMINISM.md` §17 adjudicates
/// on, and `0` is what `rustmc-server run` uses. Two centres at 12345 because
/// §30.3 named a chunk and a gate that cannot be pointed at the chunk a
/// finding names is not testing that finding.
const WIDENINGS: [Widening; 4] = [
    Widening {
        seed: 20260722,
        center: (0, 0),
        expect: [Some(0), Some(0)],
    },
    Widening {
        seed: 12345,
        center: (0, 0),
        expect: [None, None],
    },
    Widening {
        seed: 12345,
        center: (48, 54),
        expect: [Some(23), None],
    },
    Widening {
        seed: 0,
        center: (0, 0),
        expect: [None, None],
    },
];

/// The widest tile the comparison generates.
///
/// Two: the cost is `(2r + 1 + 2 * DECORATION_MARGIN)^2` chunks per rung and
/// the whole ladder runs at every case, so this is 155 chunks a case and 620
/// in all.
const WIDENING_MAX_RADIUS: i32 = 2;

fn repo(relative: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn inputs() -> Option<std::sync::Arc<WorldTables>> {
    if !worldgen_assembly::have_extract() {
        eprintln!("skipped: no vanilla extract");
        return None;
    }
    let registries = rustmc_server::configuration::CapturedRegistries::load(&repo(
        "data/vanilla/registries.bin",
    ))
    .ok()?;
    let biomes = registries
        .entry_ids("minecraft:worldgen/biome")
        .expect("readable capture")
        .expect("the capture holds the biome registry");
    let ids = StateIds::load(&repo("data/vanilla/block_props.json")).expect("block table");
    Some(std::sync::Arc::new(
        WorldTables::load(&biomes, ids).expect("block tables"),
    ))
}

/// Every block of one chunk, floor to build limit, as a flat vector.
///
/// Read through `World::block_at`, which is what a client is ultimately
/// served, rather than out of the canvas — so a difference this finds is a
/// difference a player would see.
fn column_stack(world: &World, chunk_x: i32, chunk_z: i32) -> Vec<String> {
    let mut out = Vec::with_capacity(16 * 16 * 384);
    for x in 0..16 {
        for z in 0..16 {
            for y in -64..320 {
                out.push(world.block_at((chunk_x * 16 + x, y, chunk_z * 16 + z)));
            }
        }
    }
    out
}

/// Two tiles side by side against one generation covering both.
///
/// The tiles are 1x1 chunks each so the run is a gate rather than a
/// benchmark: with `DECORATION_MARGIN` of 2 each tile still generates 25
/// chunks, and the reference generates 30. The chunks compared are the two
/// tiles' own — `(0,0)` and `(1,0)` — which are the only ones any of the
/// three is entitled to serve.
#[test]
fn two_tiles_hold_what_one_generation_would_have() {
    let Some(tables) = inputs() else {
        return;
    };

    let left = World::generate(SEED, Area::square(0, 0, 0), std::sync::Arc::clone(&tables))
        .expect("the left tile generated");
    let right = World::generate(SEED, Area::square(1, 0, 0), std::sync::Arc::clone(&tables))
        .expect("the right tile generated");
    // One area covering both tiles' interiors, generated in a single pass —
    // the thing tiling has to be indistinguishable from.
    let whole = World::generate(
        SEED,
        Area {
            min_chunk_x: 0,
            max_chunk_x: 1,
            min_chunk_z: 0,
            max_chunk_z: 0,
        },
        std::sync::Arc::clone(&tables),
    )
    .expect("the reference generated");

    let mut differences = 0usize;
    let mut first = None;
    for (tile, chunk_x) in [(&left, 0), (&right, 1)] {
        let tiled = column_stack(tile, chunk_x, 0);
        let reference = column_stack(&whole, chunk_x, 0);
        assert_eq!(tiled.len(), reference.len(), "same number of cells");
        for (i, (a, b)) in tiled.iter().zip(reference.iter()).enumerate() {
            if a != b {
                differences += 1;
                if first.is_none() {
                    let y = -64 + i32::try_from(i % 384).expect("in range");
                    let z = i32::try_from((i / 384) % 16).expect("in range");
                    let x = i32::try_from(i / 384 / 16).expect("in range");
                    first = Some(format!(
                        "chunk ({chunk_x},0) at ({x},{y},{z}): tiled {a}, one pass {b}"
                    ));
                }
            }
        }
    }

    assert_eq!(
        differences,
        0,
        "a tiled world is not the world one pass produces — {} \
         (tiling is invalid; the world has to grow through status-based \
         chunk generation over one canvas instead)",
        first.unwrap_or_default()
    );
}

/// **Three of this gate's four cases are `None`, and `None` means "some
/// blocks moved" — so the non-zeroes are also the only evidence in this file
/// that decoration ran at all.**
///
/// That evidence very nearly went. `docs/DETERMINISM.md` §32 spent a day on
/// a model under which all eight rungs read 0, and eight zeroes is equally
/// what a generator that decorated *nothing* would print. AGENT-BRIEF's rule
/// 7 has paid out fourteen times and twice it was a gate that had stopped
/// looking, so the question is asked here directly rather than left as a
/// side effect of a number that may yet legitimately become zero.
///
/// It generates one area twice — through `Through::Carvers` and through
/// `Through::Decoration` — and counts the blocks decoration put there. A
/// pass that dropped its writes comes back 0 and red.
///
/// The bound is loose on purpose. What must not drift is the shape of the
/// claim, not a figure nobody quotes; pinning it exactly would redden every
/// branch that legitimately places one more flower. Measured 2026-08-12:
/// **122,560 blocks** over the 25 delivered chunks at seed 20260722.
#[test]
fn the_delivered_square_is_the_decorated_world_and_not_the_carved_one() {
    let Some(tables) = inputs() else {
        return;
    };
    let area = Area::square(0, 0, 2);
    let decorated = World::generate(SEED, area, std::sync::Arc::clone(&tables))
        .expect("the decorated world generated");
    let carved = World::generate_through(
        SEED,
        area,
        std::sync::Arc::clone(&tables),
        rustmc_worldgen::pipeline::Through::Carvers,
    )
    .expect("the carved world generated");

    let mut placed = 0usize;
    let mut example = None;
    for (cx, cz) in area.chunks() {
        let (n, first) = chunk_diff(&carved, &decorated, cx, cz);
        placed += n;
        if example.is_none() {
            example = first;
        }
    }
    eprintln!(
        "decoration wrote {placed} blocks into the 25 delivered chunks at seed \
         {SEED} — first {}",
        example.unwrap_or_default()
    );
    assert!(
        placed > 10_000,
        "decoration wrote {placed} blocks into 25 delivered chunks; the \
         widening gate's cases are only evidence about the margin if \
         decoration happened at all (docs/DETERMINISM.md §32.5)"
    );
}

/// Blocks of chunk `(cx, cz)` that differ between two worlds, and the first.
fn chunk_diff(narrower: &World, wider: &World, cx: i32, cz: i32) -> (usize, Option<String>) {
    let mut moved = 0;
    let mut first = None;
    for x in cx * 16..cx * 16 + 16 {
        for z in cz * 16..cz * 16 + 16 {
            for y in -64..320 {
                let (before, after) = (narrower.block_at((x, y, z)), wider.block_at((x, y, z)));
                if before != after {
                    moved += 1;
                    if first.is_none() {
                        first = Some(format!("({x},{y},{z}) narrower {before}, wider {after}"));
                    }
                }
            }
        }
    }
    (moved, first)
}

/// Blocks the narrower run's **delivered** chunks lose when the area widens,
/// one entry per rung of the ladder.
///
/// Every chunk the narrower run claims is finished, not one of them: the
/// narrower area is chunk-for-chunk interior to the wider one, so it is
/// entitled to be identical throughout.
fn widening_moves(case: Widening, tables: &std::sync::Arc<WorldTables>) -> Vec<usize> {
    let (cx, cz) = case.center;
    let mut rungs = Vec::new();
    let mut previous: Option<(i32, World)> = None;
    for radius in 0..=WIDENING_MAX_RADIUS {
        let world = World::generate(
            case.seed,
            Area::square(cx, cz, radius),
            std::sync::Arc::clone(tables),
        )
        .expect("the tile generated");
        if let Some((narrow_radius, narrow)) = previous.take() {
            let delivered = Area::square(cx, cz, narrow_radius);
            let (mut moved, mut chunks, mut first) = (0usize, 0usize, None);
            for (x, z) in delivered.chunks() {
                let (n, example) = chunk_diff(&narrow, &world, x, z);
                if n > 0 {
                    moved += n;
                    chunks += 1;
                    if first.is_none() {
                        first = Some(format!("chunk ({x},{z}) {}", example.unwrap_or_default()));
                    }
                }
            }
            eprintln!(
                "seed {} centre ({cx},{cz}): radius {narrow_radius} -> {radius} moves \
                 {moved} blocks in {chunks} of the {} chunks radius {narrow_radius} \
                 delivered{}",
                case.seed,
                delivered.chunk_count(),
                first.map(|f| format!(" — first {f}")).unwrap_or_default()
            );
            rungs.push(moved);
        }
        previous = Some((radius, world));
    }
    rungs
}

/// The same chunks, out of areas of different sizes.
///
/// This exists because of a discrepancy the probe found, not because of a
/// hunch: the spawn height at column (8,8) was 75 with a tile radius of 1 and
/// 63 with a radius of 2, and twelve blocks is about a tree. If a chunk's
/// contents depend on how big the area around it was, then `DECORATION_MARGIN`
/// is not big enough and **tiling is invalid** — the test above would only
/// have missed it because both its tiles were the same size.
///
/// # It was a gate that could not fail, and this is what was under-powered
///
/// Until 2026-08-12 this compared **chunk (0,0) alone, at seed 20260722
/// alone**, and passed with 0 differences over 196,608 blocks.
/// `docs/DETERMINISM.md` §30.3 measured the same property on region dumps and
/// got the opposite answer — widening our own generated extent at seed 12345
/// moves 23 blocks between r=16 and r=18 and 2,713 between r=18 and r=20 —
/// and the two could not both be right.
///
/// The scope was wrong on two axes, and only one of them had to be fixed for
/// the gate to start failing:
///
/// * **One seed.** The effect is seed-dependent and 20260722 is a seed it
///   does not reach. [`WIDENINGS`] carries three.
/// * **One chunk, and the interior one.** §30.3 put the movement in the rim
///   of the delivered square, which is where a chain of decisions running
///   inward from the edge of the generated area arrives first; the centre is
///   the last place it reaches. Every delivered chunk is compared now.
///
/// # Why the name is still true and the assertion is no longer `0`
///
/// Because there is no margin at which `0` is reachable, and that is measured
/// rather than conceded — `docs/DETERMINISM.md` §31.3. A tile of radius `r`
/// with margin `m` decorates out to `r + m` and delivers out to `r`, so its
/// rim sits at depth `m` below the outermost decorated ring **for every m**;
/// widening the margin moves the rim outward with it and leaves the distance
/// unchanged. Injected at `m` of 1, 2 and 4 the totals here are 5,753, 2,201
/// and 1,130 — a decreasing sequence with no zero in it, bought at
/// `(2r + 1 + 2m)^2` chunks generated per tile (§31.5).
///
/// So this asserts the measured shape of the dependence instead: exactly zero
/// where every parity number in the project is taken, non-zero where the
/// defect lives, and §30.3's 23 to the block. The old gate passed at margins
/// 1, 2 and 4 alike — it did not respond to the margin at all — and this one
/// fails at all three.
#[test]
fn a_chunk_does_not_depend_on_how_big_the_area_around_it_was() {
    let Some(tables) = inputs() else {
        return;
    };
    let mut wrong: Vec<String> = Vec::new();
    for case in WIDENINGS {
        let rungs = widening_moves(case, &tables);
        assert_eq!(rungs.len(), case.expect.len(), "one entry per rung");
        for (rung, (moved, expected)) in rungs.iter().zip(case.expect).enumerate() {
            let ok = match expected {
                Some(n) => *moved == n,
                None => *moved > 0,
            };
            if !ok {
                let want = match expected {
                    Some(n) => format!("exactly {n}"),
                    None => "some blocks to move — a zero here means the \
                             margin's decision reach was fixed; record the \
                             before/after in docs/DETERMINISM.md §31 and pin \
                             this case, do not delete it"
                        .to_string(),
                };
                wrong.push(format!(
                    "seed {} centre {:?} radius {rung} -> {}: {moved} moved, wanted {want}",
                    case.seed,
                    case.center,
                    rung + 1
                ));
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "the dependence of a delivered chunk on the size of the area it was \
         generated in has changed shape — {}\n(docs/DETERMINISM.md §31; the \
         zeroes at seed 20260722 are what every parity figure in this project \
         rests on, and the 23 at (48,54) is §30.3's 23)",
        wrong.join("; ")
    );
}
