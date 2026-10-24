//! Instrument, not a gate: how wide does the context have to be before a
//! chunk stops changing?
//!
//! ORACLE.md rule 14 — widen the context until the answer stops changing.
//! `DECORATION_MARGIN` went from 1 to 2 on 2026-07-27 because one ring was
//! measurably wrong; this asks whether two is enough, by generating the same
//! chunk inside progressively larger areas and diffing it against the
//! previous one. Every ring past the second should change nothing.
//!
//! # What it found, 2026-07-28, seed 0, chunk (0,0)
//!
//! ```text
//!   context 2 rings: top at (8,8) = 74, blocks differing from previous = -
//!   context 3 rings: top at (8,8) = 74, blocks differing from previous = 24
//!   context 4 rings: top at (8,8) = 62, blocks differing from previous = 332
//!   context 5 rings: top at (8,8) = 74, blocks differing from previous = 332
//!   context 6 rings: top at (8,8) = 62, blocks differing from previous = 334
//!   context 7 rings: top at (8,8) = 62, blocks differing from previous = 61
//! ```
//!
//! It does not converge — it **oscillates**. That is not the signature of a
//! margin that is merely too small, which would settle once the widest
//! reaching feature was inside it. Something about the generated area itself
//! is reaching the chunk.
//!
//! This is why the discrepancy was noticed at all: `ci/dig-probe.py` spawned
//! a player at y 75 with a tile radius of 1 and y 63 with a radius of 2, in
//! the same world at the same column, and twelve blocks is about a tree.
//!
//! # RE-RUN 2026-08-12: the seed-0 oscillation is GONE, and seed 12345 is not
//!
//! Same command, same chunk, tip `d049ef5a`:
//!
//! ```text
//!   seed 0          top at (8,8) = 74 at every context;  moved  2, 0, 0, 0, 0
//!   seed 12345      top at (8,8) = 62 at every context;  moved 40, 56, 41, 29, 33
//! ```
//!
//! The generator has moved a long way since 2026-07-28 — icebergs, lakes,
//! leaf litter — and seed 0's column no longer oscillates or even changes
//! after the first rung. **Do not quote the 74, 74, 62, 74, 62, 62 above as
//! current**; it is kept as the dated reading that opened the item.
//!
//! What replaced it is worse rather than better, and it is at the seed
//! `docs/DETERMINISM.md` §17 adjudicates on. Seed 12345's chunk (0,0) moves
//! at every rung, and `tools/worldgen-assembly/tests/probe_margin_context.rs`
//! — a different instrument, reading the canvas rather than `World::block_at`
//! — gets **40, 56, 41, 29, 33 to the block** on the same rungs. Two
//! instruments, one number.
//!
//! **And the top block at (8,8) is a poor detector, which is the lesson.** It
//! is constant at both seeds above while hundreds of blocks move underneath
//! it. A column is one sample of a sparse, localised effect; the delivered
//! *square* is what a tile has to be right about. See `docs/DETERMINISM.md`
//! §31.
//!
//! # What it does and does not invalidate
//!
//! `tests/tiled_world.rs` shows the same comparison coming out at **0
//! differences** at seed 20260722, which is the seed every parity number in
//! this project is measured at. So this is not "tiling is broken": the fixed
//! square the server used to serve had exactly the same exposure, because it
//! too was one area chosen at one radius. What it means is that "a delivered
//! chunk is finished" is **seed-dependent**, and that both the server's tiles
//! and item 1's region diffs rest on it.
//!
//! That paragraph still holds for 20260722 and is now checked much harder —
//! 0 over 225 delivered chunks out to a 10-ring context (§31.2). What it did
//! not say, because nobody had run the ladder long enough to see it, is that
//! **no margin width converges anywhere**: a tile's rim is at depth
//! `DECORATION_MARGIN` below the outermost ring it decorated whatever the
//! margin is (§31.3).
//!
//! # Running it
//!
//! Env-gated because it generates six worlds up to 13x13 chunks and takes
//! about 90 seconds — too slow for `ci/t0.sh`, and it asserts nothing, which
//! is what makes it an instrument rather than a gate.
//!
//! ```sh
//!   RUSTMC_MARGIN_PROBE=1 cargo test --release -p rustmc-server \
//!       --test margin_probe -- --nocapture
//!   RUSTMC_MARGIN_PROBE=1 RUSTMC_MARGIN_SEED=20260722 ...   # another seed
//! ```

use rustmc_server::terrain::StateIds;
use rustmc_server::world::World;
use rustmc_worldgen::pipeline::Area;

fn repo(relative: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

#[test]
fn how_wide_until_the_chunk_stops_changing() {
    if std::env::var("RUSTMC_MARGIN_PROBE").is_err() {
        eprintln!("skipped: set RUSTMC_MARGIN_PROBE=1 — this is an instrument, not a gate");
        return;
    }
    if !worldgen_assembly::have_extract() {
        eprintln!("skipped: no vanilla extract");
        return;
    }
    // Seed 0 by default: the seed `rustmc-server run` uses, and the one the
    // discrepancy was seen in from a real client.
    let seed: i64 = std::env::var("RUSTMC_MARGIN_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let registries = rustmc_server::configuration::CapturedRegistries::load(&repo(
        "data/vanilla/registries.bin",
    ))
    .expect("registry capture");
    let biomes = registries
        .entry_ids("minecraft:worldgen/biome")
        .expect("readable capture")
        .expect("the capture holds the biome registry");
    let ids = StateIds::load(&repo("data/vanilla/block_props.json")).expect("block table");
    let tables = std::sync::Arc::new(
        rustmc_server::world::WorldTables::load(&biomes, ids).expect("block tables"),
    );

    let mut previous: Option<Vec<String>> = None;
    for radius in 0..=5 {
        let world = World::generate(
            seed,
            Area::square(0, 0, radius),
            std::sync::Arc::clone(&tables),
        )
        .expect("generated");
        let mut cells = Vec::with_capacity(16 * 16 * 384);
        let mut top = -64;
        for x in 0..16 {
            for z in 0..16 {
                for y in -64..320 {
                    let block = world.block_at((x, y, z));
                    if x == 8 && z == 8 && block != "minecraft:air" && block != "minecraft:cave_air"
                    {
                        top = y;
                    }
                    cells.push(block);
                }
            }
        }
        // The context is the area plus the pipeline's own margin, which is
        // what actually decides whether a reaching feature was generated.
        let rings = radius + rustmc_worldgen::pipeline::DECORATION_MARGIN;
        match previous.as_ref() {
            None => eprintln!("context {rings} rings: top at (8,8) = {top}"),
            Some(p) => {
                let moved = p.iter().zip(cells.iter()).filter(|(a, b)| a != b).count();
                eprintln!(
                    "context {rings} rings: top at (8,8) = {top}, \
                     blocks differing from the previous context = {moved}"
                );
            }
        }
        previous = Some(cells);
    }
    eprintln!(
        "seed {seed}: every line after the first should read 0. \
         A number that shrinks means the margin is too small; \
         one that oscillates means something else reaches the chunk."
    );
}
