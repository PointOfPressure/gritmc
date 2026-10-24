//! **A chunk, once delivered, is never written again.**
//!
//! `docs/DETERMINISM.md` §32.6, and it is a *different* property from the one
//! `tests/tiled_world.rs` asks for. That gate asks whether a delivered chunk
//! is independent of how big the area around it was, and §32.3/§32.4 measured
//! that the answer cannot be yes: vanilla's features read their neighbours'
//! features, a generator that forbids that read is 8,762 -> 42,873 mismatches
//! further from vanilla on the seed-20260722 r=7 window at (64,64), and two
//! unmodified vanilla runs of one seed differ by 32,127 blocks at that seam
//! anyway. Extent-independence is retired, not owed.
//!
//! What a player can actually feel is weaker and reachable: the world must not
//! change behind them as they walk. That needs the canvas to be **one canvas**
//! and a chunk, once handed out, never to be written into again. The pyramid
//! makes it cheap — `FEATURES` has `blockStateWriteRadius(1)` and no later
//! step writes at all, so a chunk whose own 3x3 has decorated can never be
//! reached again.
//!
//! # The two arms, in one binary and on one instrument
//!
//! Both arms grow a square from radius `r` to `r + 1`, and both are scored
//! over the same chunks with the same block-by-block comparison:
//!
//! * **`regenerate`** — what the tree does today. [`crate::tiles`] gives every
//!   tile its own generation of its own square, so growing means generating
//!   the bigger square from scratch. This arm is the before.
//! * **`grow`** — [`GrowingWorld`]: one canvas, a status per chunk, each chunk
//!   upgraded at most once, and the delivered set read off
//!   `ChunkStep.getAccumulatedRadiusOf(FEATURES)` rather than off the request.
//!
//! # And the injection is in the same measurement, not beside it
//!
//! A gate whose whole content is a zero is the shape AGENT-BRIEF's rule 7 has
//! paid out on fourteen times, and §32.5 is the most recent: eight zeroes is
//! also what a generator that decorated nothing would print. So the delivered
//! square is not the only thing watched here. The square one ring wider — the
//! chunks `GrowingWorld::delivering_early(1)` would have handed out, whose own
//! 3x3 has **not** finished decorating — is snapshotted in the same pass and
//! scored separately. The property says the first count is zero. The injection
//! says the second is not, off the same grown bytes, so a run that has stopped
//! measuring anything cannot look like a pass.
//!
//! # It has been red, and here is what it took
//!
//! Both injections were applied to the tree that produces the zeroes, run, and
//! reverted (`docs/DETERMINISM.md` §33.2):
//!
//! * scoring the ring one chunk early as delivered — §32.6's own named failure
//!   — reddens all eight rungs, **50,378** blocks;
//! * dropping the generate-once filter on the decoration targets, so a
//!   decorated chunk decorates again on the next growth, reddens all eight,
//!   **2,971** blocks — **including both rungs at seed 20260722, where
//!   `tiled_world.rs` reads 0**. That is the reason this file is not a
//!   re-spelling of that one.

use std::collections::HashMap;

use rustmc_server::growing::GrowingWorld;
use rustmc_server::terrain::StateIds;
use rustmc_server::world::{World, WorldTables};
use rustmc_worldgen::pipeline::Area;

/// One ladder: a seed, the chunk the squares are centred on, and what the
/// **regenerate** arm reads on each rung.
///
/// The seeds and centres are `tests/tiled_world.rs`'s, deliberately: that gate
/// already measures the regenerate arm, and an arm that disagreed with it
/// would mean this file's instrument and that one's are measuring different
/// things. `20260722` is where every parity number in the project is taken,
/// `12345` is the seed `docs/DETERMINISM.md` §17 adjudicates on, `0` is what
/// `rustmc-server run` uses, and (48,54) is the chunk §30.3 named.
#[derive(Clone, Copy)]
struct Ladder {
    seed: i64,
    center: (i32, i32),
    /// `Some(n)` is asserted to the block, `None` only to "some blocks moved"
    /// — the same split `tiled_world.rs` makes and for the same reason. The
    /// zeroes at 20260722 and §30.3's 23 are quoted; the rest are real,
    /// unquoted, and pinning them would redden every branch that legitimately
    /// moves a block at seed 12345 or 0.
    regenerate: [Option<usize>; 2],
}

const LADDERS: [Ladder; 4] = [
    Ladder {
        seed: 20260722,
        center: (0, 0),
        regenerate: [Some(0), Some(0)],
    },
    Ladder {
        seed: 12345,
        center: (0, 0),
        regenerate: [None, None],
    },
    Ladder {
        seed: 12345,
        center: (48, 54),
        regenerate: [Some(23), None],
    },
    Ladder {
        seed: 0,
        center: (0, 0),
        regenerate: [None, None],
    },
];

/// The widest square either arm grows to. Two, so the rungs are `0 -> 1` and
/// `1 -> 2` — the same ladder `tiled_world.rs` walks, so the regenerate arm's
/// eight numbers are directly comparable with its eight.
const MAX_RADIUS: i32 = 2;

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

/// Block names, numbered, so a whole chunk's before-state costs 384 KB
/// instead of ~100,000 `String`s.
///
/// The comparison has to survive the growth that happens between the two
/// readings — that is the difference between measuring a growth and measuring
/// two generations — so the before-state is held in memory rather than
/// re-read, and holding it as strings is what makes the widest rung
/// unaffordable.
#[derive(Default)]
struct Names {
    ids: HashMap<String, u32>,
}

impl Names {
    fn intern(&mut self, state: &str) -> u32 {
        if let Some(id) = self.ids.get(state) {
            return *id;
        }
        let id = u32::try_from(self.ids.len()).expect("fewer than 4 billion block states");
        self.ids.insert(state.to_string(), id);
        id
    }
}

/// Every block of one chunk, floor to build limit, numbered.
///
/// x-then-z-then-y, and the index arithmetic in [`position_of`] is its
/// inverse, so a difference can name the block it happened at.
fn chunk_cells(world: &GrowingWorld, names: &mut Names, chunk_x: i32, chunk_z: i32) -> Vec<u32> {
    let mut out = Vec::with_capacity(16 * 16 * 384);
    for x in 0..16 {
        for z in 0..16 {
            for y in -64..320 {
                let id = names.intern(world.block_at(chunk_x * 16 + x, y, chunk_z * 16 + z));
                out.push(id);
            }
        }
    }
    out
}

/// The same, out of a finished [`World`] — the regenerate arm's reading.
///
/// Through `World::block_at`, which is what a client is ultimately served, so
/// a difference this finds is a difference a player would see.
fn world_cells(world: &World, names: &mut Names, chunk_x: i32, chunk_z: i32) -> Vec<u32> {
    let mut out = Vec::with_capacity(16 * 16 * 384);
    for x in 0..16 {
        for z in 0..16 {
            for y in -64..320 {
                let id = names.intern(&world.block_at((chunk_x * 16 + x, y, chunk_z * 16 + z)));
                out.push(id);
            }
        }
    }
    out
}

/// The block a flat index in [`chunk_cells`] names.
fn position_of(chunk_x: i32, chunk_z: i32, index: usize) -> (i32, i32, i32) {
    let y = -64 + i32::try_from(index % 384).expect("in range");
    let z = i32::try_from((index / 384) % 16).expect("in range");
    let x = i32::try_from(index / 384 / 16).expect("in range");
    (chunk_x * 16 + x, y, chunk_z * 16 + z)
}

/// How many cells differ, and the first one.
fn differences(
    before: &[u32],
    after: &[u32],
    chunk_x: i32,
    chunk_z: i32,
) -> (usize, Option<(i32, i32, i32)>) {
    let mut moved = 0;
    let mut first = None;
    for (index, (was, now)) in before.iter().zip(after).enumerate() {
        if was != now {
            moved += 1;
            if first.is_none() {
                first = Some(position_of(chunk_x, chunk_z, index));
            }
        }
    }
    (moved, first)
}

/// Chessboard distance from the centre, in chunks — which ring a chunk is on.
const fn ring(center: (i32, i32), chunk_x: i32, chunk_z: i32) -> i32 {
    let dx = (chunk_x - center.0).abs();
    let dz = (chunk_z - center.1).abs();
    if dx > dz {
        dx
    } else {
        dz
    }
}

/// What one rung of the growing arm read.
struct Rung {
    /// Blocks that moved inside the honest delivered square. The property.
    delivered: usize,
    /// Blocks that moved in the ring one further out — the chunks
    /// `delivering_early(1)` would have handed out. The injection.
    one_ring_early: usize,
}

/// **The property.** One canvas, grown a ring at a time, and nothing already
/// delivered moves.
///
/// Every rung also reads what an early delivery would have cost, off the same
/// grown bytes — see the module docs for why a bare zero would not be evidence
/// of anything.
#[test]
fn a_delivered_chunk_is_never_written_again() {
    if inputs().is_none() {
        return;
    }
    let mut wrong: Vec<String> = Vec::new();
    let mut delivered_total = 0usize;
    let mut early_total = 0usize;

    for ladder in LADDERS {
        let (cx, cz) = ladder.center;
        let mut world = GrowingWorld::new(ladder.seed, ladder.center);
        let mut names = Names::default();
        let mut carved = world
            .grow_to(0)
            .expect("the first square of the growing world generated");

        for radius in 0..MAX_RADIUS {
            // The delivered set is derived from the status map and the
            // pyramid, never from the radius that was asked for — so this
            // assertion is a real check that the two agree rather than a
            // restatement of the request.
            let claimed: Vec<(i32, i32)> = Area::square(cx, cz, radius).chunks().collect();
            assert_eq!(
                world.delivered(),
                claimed,
                "seed {} centre ({cx},{cz}) radius {radius}: the pyramid's \
                 delivered set is not the square that was asked for",
                ladder.seed
            );

            // Watched: the delivered square **and** the ring outside it, which
            // is exactly the set `delivering_early(1)` would hand out.
            let watched: Vec<(i32, i32)> = Area::square(cx, cz, radius + 1).chunks().collect();
            let before: Vec<Vec<u32>> = watched
                .iter()
                .map(|&(x, z)| chunk_cells(&world, &mut names, x, z))
                .collect();

            carved += world.grow_to(radius + 1).expect("the world grew");

            let mut rung = Rung {
                delivered: 0,
                one_ring_early: 0,
            };
            let mut first_delivered = None;
            let mut first_early = None;
            for (&(x, z), was) in watched.iter().zip(&before) {
                let now = chunk_cells(&world, &mut names, x, z);
                let (moved, first) = differences(was, &now, x, z);
                if moved == 0 {
                    continue;
                }
                if ring(ladder.center, x, z) <= radius {
                    rung.delivered += moved;
                    if first_delivered.is_none() {
                        first_delivered = first.map(|p| format!("chunk ({x},{z}) at {p:?}"));
                    }
                } else {
                    rung.one_ring_early += moved;
                    if first_early.is_none() {
                        first_early = first.map(|p| format!("chunk ({x},{z}) at {p:?}"));
                    }
                }
            }
            eprintln!(
                "seed {} centre ({cx},{cz}): growing {radius} -> {} moves {} blocks in the \
                 {} delivered chunks{}, and {} in the ring one further out{}",
                ladder.seed,
                radius + 1,
                rung.delivered,
                claimed.len(),
                first_delivered
                    .map(|f| format!(" — first {f}"))
                    .unwrap_or_default(),
                rung.one_ring_early,
                first_early
                    .map(|f| format!(" — first {f}"))
                    .unwrap_or_default(),
            );
            delivered_total += rung.delivered;
            early_total += rung.one_ring_early;
            if rung.delivered != 0 {
                wrong.push(format!(
                    "seed {} centre ({cx},{cz}) radius {radius} -> {}: {} blocks moved in \
                     chunks that had already been delivered",
                    ladder.seed,
                    radius + 1,
                    rung.delivered
                ));
            }
        }
        // The cost claim, measured rather than argued. A tile regenerates its
        // whole square every time it grows; this ladder carves each chunk
        // once, so the total is the widest square generated and not the sum of
        // every square on the way to it.
        let widest = rustmc_worldgen::pipeline::extents_for(Area::square(cx, cz, MAX_RADIUS))
            .1
            .chunk_count();
        eprintln!(
            "seed {} centre ({cx},{cz}): the ladder to radius {MAX_RADIUS} carved \
             {carved} chunks; the widest square alone is {widest}",
            ladder.seed,
        );
        assert_eq!(
            carved, widest,
            "seed {} centre ({cx},{cz}): a growing world carved {carved} chunks to \
             reach radius {MAX_RADIUS}, and the widest square it ever held is \
             {widest} — a chunk was carved twice",
            ladder.seed,
        );
    }

    assert!(
        wrong.is_empty(),
        "a chunk that had been delivered was written again — {}\n(docs/DETERMINISM.md \
         §32.6; the canvas is one canvas and a chunk whose own 3x3 has decorated cannot \
         be reached by anything left in the pyramid, so a non-zero here is either a \
         delivery rule that runs ahead of the decoration or a stage that ran twice)",
        wrong.join("; ")
    );
    assert_eq!(delivered_total, 0, "the property, summed over every rung");
    // **The injection, and it is not decoration.** Growing the canvas has to
    // write *somewhere*, or the zero above is a generator that stopped
    // generating. These are the blocks a world delivering one ring early
    // would have handed a player and then silently changed underneath them.
    assert!(
        early_total > 0,
        "growing the canvas moved no block anywhere, in any of the four \
         ladders — the zero above is the instrument, not the property \
         (docs/AGENT-BRIEF.md rule 7)"
    );
    eprintln!(
        "GROWING WORLD: 0 blocks moved in delivered chunks over 8 rungs; \
         {early_total} moved in the ring a one-ring-early delivery would have \
         handed out"
    );
}

/// **The before.** Growing by regenerating the bigger square — what the tree
/// does today — measured on this file's instrument and over this file's
/// delivered sets.
///
/// It exists so the pair of numbers is one instrument's before and after
/// rather than two instruments agreeing. Its eight rungs must also reproduce
/// `tests/tiled_world.rs`'s eight, which is the cross-check that this file's
/// comparison and that one's are the same comparison.
#[test]
fn regenerating_the_square_instead_of_growing_it_rewrites_delivered_chunks() {
    let Some(tables) = inputs() else {
        return;
    };
    let mut wrong: Vec<String> = Vec::new();
    let mut total = 0usize;

    for ladder in LADDERS {
        let (cx, cz) = ladder.center;
        let mut names = Names::default();
        let mut narrower: Option<World> = Some(
            World::generate(
                ladder.seed,
                Area::square(cx, cz, 0),
                std::sync::Arc::clone(&tables),
            )
            .expect("the narrow square generated"),
        );
        for radius in 0..MAX_RADIUS {
            let narrow = narrower.take().expect("one world per rung");
            let delivered: Vec<(i32, i32)> = Area::square(cx, cz, radius).chunks().collect();
            let before: Vec<Vec<u32>> = delivered
                .iter()
                .map(|&(x, z)| world_cells(&narrow, &mut names, x, z))
                .collect();
            drop(narrow);
            let wide = World::generate(
                ladder.seed,
                Area::square(cx, cz, radius + 1),
                std::sync::Arc::clone(&tables),
            )
            .expect("the wide square generated");

            let mut moved_here = 0usize;
            let mut first = None;
            for (&(x, z), was) in delivered.iter().zip(&before) {
                let now = world_cells(&wide, &mut names, x, z);
                let (moved, where_) = differences(was, &now, x, z);
                moved_here += moved;
                if moved > 0 && first.is_none() {
                    first = where_.map(|p| format!("chunk ({x},{z}) at {p:?}"));
                }
            }
            eprintln!(
                "seed {} centre ({cx},{cz}): regenerating at {radius} -> {} moves {moved_here} \
                 blocks in the {} chunks radius {radius} delivered{}",
                ladder.seed,
                radius + 1,
                delivered.len(),
                first.map(|f| format!(" — first {f}")).unwrap_or_default(),
            );
            total += moved_here;
            let expected = ladder.regenerate[usize::try_from(radius).expect("a small radius")];
            let ok = match expected {
                Some(n) => moved_here == n,
                None => moved_here > 0,
            };
            if !ok {
                wrong.push(format!(
                    "seed {} centre ({cx},{cz}) radius {radius} -> {}: {moved_here} moved, \
                     wanted {}",
                    ladder.seed,
                    radius + 1,
                    match expected {
                        Some(n) => format!("exactly {n}"),
                        None => "some blocks to move".to_string(),
                    }
                ));
            }
            narrower = Some(wide);
        }
    }

    eprintln!("REGENERATE ARM: {total} blocks over 8 rungs");
    assert!(
        wrong.is_empty(),
        "the regenerate arm has changed shape and no longer agrees with \
         tests/tiled_world.rs — {}\n(both files measure the same eight rungs \
         over the same delivered squares; if this one moved, that one moved \
         too, and docs/DETERMINISM.md §31/§32 is where the before/after goes)",
        wrong.join("; ")
    );
    assert!(
        total > 0,
        "regenerating the square moved nothing at all, which is the property \
         docs/DETERMINISM.md §31.3 measured as unreachable at every margin \
         width — suspect the instrument (AGENT-BRIEF rule 7)"
    );
}
