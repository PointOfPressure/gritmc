//! Can a client stand in the nether and the End, and is what it is standing
//! on the same world the dump measured?
//!
//! # The claim this gate exists to make, and the one it deliberately does not
//!
//! `docs/PARITY.md` states the nether and the End against **vanilla**, and
//! those numbers were taken on `tools/worldgen-dump`'s output. They transfer
//! to the running server only if the server's chunks are the dump's chunks —
//! which is a statement about two code paths, not about vanilla, and is what
//! is measured here.
//!
//! It is measured rather than argued because "they call the same function" is
//! exactly the shape of argument this project has been wrong about before: the
//! server reaches `worldgen_assembly::generate::generate_nether` through
//! `World::generate_in`, which chooses the arm, checks the canvas height
//! against the dimension type and hands the canvas to a **chunk encoder that
//! was written for a 24-section overworld**. Every one of those steps can
//! disagree with the dump while the generator agrees with itself.
//!
//! So the comparison is over the encoded chunk, decoded back — the bytes a
//! client receives — against the canvas the generator produced, block for
//! block, with the section count taken from the dimension type rather than
//! from either side of the comparison.

use rustmc_server::chunk::ContainerKind;
use rustmc_server::dimension::Dimension;
use rustmc_server::terrain::StateIds;
use rustmc_server::world::{World, WorldTables};
use rustmc_worldgen::pipeline::{Area, Through};

const SEED: i64 = 20260722;

fn repo(relative: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn tables() -> Option<std::sync::Arc<WorldTables>> {
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

/// Network id to block name, over the whole state table.
fn names_by_id() -> std::collections::BTreeMap<i32, String> {
    let text = std::fs::read_to_string(repo("data/vanilla/block_props.json")).expect("block table");
    let doc: serde_json::Value = serde_json::from_str(&text).expect("json");
    doc["table"]
        .as_array()
        .expect("table")
        .iter()
        .filter_map(|e| {
            let state = e["state"].as_str()?;
            let id = e["id"].as_i64()? as i32;
            Some((id, state.split('[').next().unwrap_or(state).to_string()))
        })
        .collect()
}

/// One served chunk of one dimension, decoded back off the wire.
fn served(dim: Dimension) -> Option<(rustmc_server::chunk::LevelChunkWithLight, Vec<String>)> {
    let tables = tables()?;
    let world = World::generate_in(
        dim,
        SEED,
        Area::square(0, 0, 0),
        tables,
        Through::Decoration,
    )
    .expect("generated");
    let packet = world.chunk(0, 0).expect("encoded");

    // Round-tripped rather than read straight off the struct: the section
    // count is the one thing a second dimension gets wrong, and a decoder
    // handed the wrong number desyncs the buffer rather than returning a
    // short chunk. Encoding and decoding with the DIMENSION TYPE's count is
    // the check.
    let body = packet.encode().expect("encode");
    let back = rustmc_server::chunk::LevelChunkWithLight::decode(&body, dim.sections())
        .expect("decode at the dimension's own section count");
    let names = names_by_id();
    let mut seen = Vec::new();
    for s in &back.sections {
        if let rustmc_server::chunk::Palette::Indirect(p) = &s.states.palette {
            for id in p {
                let n = names.get(id).cloned().unwrap_or_else(|| format!("id {id}"));
                if !seen.contains(&n) {
                    seen.push(n);
                }
            }
        }
        if let rustmc_server::chunk::Palette::Single(id) = &s.states.palette {
            let n = names.get(id).cloned().unwrap_or_else(|| format!("id {id}"));
            if !seen.contains(&n) {
                seen.push(n);
            }
        }
    }
    Some((back, seen))
}

/// A nether chunk a client can be sent: sixteen sections, y 0..255, and
/// material only the nether's own surface rule and decoration produce.
#[test]
fn the_nether_serves_a_chunk_a_client_can_stand_in() {
    let Some((chunk, blocks)) = served(Dimension::Nether) else {
        return;
    };
    assert_eq!(
        chunk.sections.len(),
        16,
        "the nether stores 256 blocks, so a chunk packet is 16 sections — not the overworld's 24"
    );
    for want in [
        "minecraft:netherrack",
        "minecraft:bedrock",
        "minecraft:lava",
    ] {
        assert!(
            blocks.iter().any(|b| b == want),
            "a nether chunk must hold {want}; it holds {blocks:?}"
        );
    }
    // The overworld's default block must not be in it at all. Without this
    // the test would pass over a world the overworld generator made, since
    // an overworld chunk also holds bedrock.
    assert!(
        !blocks.iter().any(|b| b == "minecraft:grass_block"),
        "an overworld world got served as the nether: {blocks:?}"
    );
}

/// An End chunk a client can be sent.
#[test]
fn the_end_serves_a_chunk_a_client_can_stand_in() {
    let Some((chunk, blocks)) = served(Dimension::TheEnd) else {
        return;
    };
    assert_eq!(chunk.sections.len(), 16, "the End stores 256 blocks");
    assert!(
        blocks.iter().any(|b| b == "minecraft:end_stone"),
        "an End chunk must hold end_stone; it holds {blocks:?}"
    );
    assert!(
        !blocks.iter().any(|b| b == "minecraft:netherrack"),
        "the nether got served as the End: {blocks:?}"
    );
}

/// The overworld still serves 24, so the change did not make every dimension
/// the same one. The positive control for both tests above.
#[test]
fn the_overworld_still_serves_twenty_four_sections() {
    let Some((chunk, blocks)) = served(Dimension::Overworld) else {
        return;
    };
    assert_eq!(chunk.sections.len(), 24);
    assert!(blocks.iter().any(|b| b == "minecraft:stone"), "{blocks:?}");
}

/// **The transfer claim**: a served chunk of a dimension is the dump's chunk.
///
/// Block for block over the whole column, against the canvas
/// `worldgen_assembly::generate::generate_nether` / `_end` produced — the
/// same call `tools/worldgen-dump` makes, at the same seed and area. What is
/// under test is everything between that canvas and the wire: the section
/// slicing, the `min_y` offset, the palettes, the biome container.
///
/// A mismatch here means `docs/PARITY.md`'s nether and End figures are about
/// a world the server does not serve.
#[test]
fn a_served_chunk_is_the_dumps_chunk() {
    let Some(tables) = tables() else {
        return;
    };
    let area = Area::square(0, 0, 0);
    for dim in [Dimension::Nether, Dimension::TheEnd, Dimension::Overworld] {
        let reference = match dim {
            Dimension::Nether => {
                worldgen_assembly::generate::generate_nether(SEED, area, Through::Decoration)
            }
            Dimension::TheEnd => {
                worldgen_assembly::generate::generate_end(SEED, area, Through::Decoration)
            }
            Dimension::Overworld => {
                worldgen_assembly::generate::generate(SEED, area, Through::Decoration)
            }
        }
        .expect("the dump's own call");

        let world = World::generate_in(
            dim,
            SEED,
            area,
            std::sync::Arc::clone(&tables),
            Through::Decoration,
        )
        .expect("served");
        let packet = world.chunk(0, 0).expect("encoded");
        let body = packet.encode().expect("encode");
        let back = rustmc_server::chunk::LevelChunkWithLight::decode(&body, dim.sections())
            .expect("decode");

        let names = names_by_id();
        let (palette, cells) = reference
            .canvas
            .chunk_data(0, 0)
            .expect("the reference generated chunk (0,0)");

        let mut compared = 0usize;
        let mut differ = 0usize;
        let mut first: Option<String> = None;
        for (s, section) in back.sections.iter().enumerate() {
            let values = section.states.values(ContainerKind::BlockStates);
            for offset in 0..4096usize {
                let want = &palette[cells[s * 4096 + offset] as usize];
                let want = want.split('[').next().unwrap_or(want);
                let got_id = values[offset];
                let got = names
                    .get(&got_id)
                    .map_or_else(|| format!("id {got_id}"), Clone::clone);
                compared += 1;
                if got != want {
                    differ += 1;
                    if first.is_none() {
                        let y = dim.min_y() + (s * 16 + offset / 256) as i32;
                        first = Some(format!(
                            "{} at section {s} offset {offset} (y {y}): served {got}, dump {want}",
                            dim.id()
                        ));
                    }
                }
            }
        }
        assert_eq!(
            compared,
            dim.sections() * 4096,
            "{}: the whole column was compared",
            dim.id()
        );
        assert_eq!(
            differ,
            0,
            "{}: {differ} of {compared} blocks differ from the dump. first: {}",
            dim.id(),
            first.unwrap_or_default()
        );
    }
}

/// The **live** chunk source, not the generator: `Tiles::prepare_in` then
/// `Tiles::chunk`, which is exactly the pair `status.rs`'s `chunk_at` closure
/// calls for every chunk a connected client is sent.
///
/// The tests above go through `World::generate_in` directly. That leaves a
/// gap the size of `Tiles`: the per-dimension `OnceLock`, the tile grid, the
/// overlay attachment and the region directory all sit between a generated
/// world and a served chunk, and a `Tiles` that handed every dimension the
/// overworld's grid would satisfy every assertion in this file up to here.
///
/// `Tiles::shared_in` is one process-wide cell per dimension, so this test
/// fixes the seed and the radius for the whole binary — which is why it is
/// one test over all three dimensions rather than three tests that would race
/// each other into the same cells.
#[test]
fn the_tile_grid_serves_all_three_dimensions() {
    if !worldgen_assembly::have_extract() {
        eprintln!("skipped: no vanilla extract");
        return;
    }
    // `Tiles::build` reads `data/vanilla/block_props.json` and
    // `data/vanilla/registries.bin` by RELATIVE path — correct for a server
    // that runs from the world root, and the reason the live chunk source has
    // never been reachable from a test, whose cwd is the crate directory. So
    // this test moves cwd to the workspace root.
    //
    // That is a process-wide mutation and it is safe HERE and not in general:
    // every other test in this binary resolves its fixtures through `repo()`
    // or through `worldgen_assembly::DATA`, both of which are built from
    // `CARGO_MANIFEST_DIR` and are absolute. Nothing in this binary reads a
    // relative path except the code under test.
    std::env::set_current_dir(repo("")).expect("workspace root");

    // Radius 0: one chunk per tile, nine generated with the margin. Enough to
    // prove the grid ran the right generator, cheap enough to be a gate.
    let mut seen = Vec::new();
    for dim in [Dimension::Nether, Dimension::TheEnd, Dimension::Overworld] {
        let tiles = rustmc_server::tiles::Tiles::prepare_in(dim, SEED, 0)
            .unwrap_or_else(|e| panic!("{}: prepare: {e}", dim.id()));
        assert_eq!(tiles.dimension(), dim);

        let chunk = tiles
            .chunk(0, 0)
            .unwrap_or_else(|e| panic!("{}: chunk: {e}", dim.id()))
            .unwrap_or_else(|| panic!("{}: the spawn tile is prepared before this runs", dim.id()));
        assert_eq!(
            chunk.sections.len(),
            dim.sections(),
            "{}: the served chunk is the dimension's own height",
            dim.id()
        );

        // The three grids are separate cells: a `Tiles` that fell through to
        // the overworld's would have the overworld's section count, caught
        // above, AND the overworld's blocks, caught here.
        let names = names_by_id();
        let mut blocks = std::collections::BTreeSet::new();
        for s in &chunk.sections {
            for id in s.states.values(ContainerKind::BlockStates) {
                if let Some(n) = names.get(&id) {
                    blocks.insert(n.clone());
                }
            }
        }
        seen.push((dim, blocks));
    }

    let signature = |d: Dimension| match d {
        Dimension::Nether => "minecraft:netherrack",
        Dimension::TheEnd => "minecraft:end_stone",
        Dimension::Overworld => "minecraft:stone",
    };
    for (dim, blocks) in &seen {
        assert!(
            blocks.contains(signature(*dim)),
            "{}: served chunk holds no {}: {blocks:?}",
            dim.id(),
            signature(*dim)
        );
        // And no other dimension's signature block, which is what a shared
        // grid would show.
        for other in [Dimension::Nether, Dimension::TheEnd] {
            if other != *dim {
                assert!(
                    !blocks.contains(signature(other)),
                    "{} served {}'s terrain — the grids are not separate",
                    dim.id(),
                    other.id()
                );
            }
        }
    }
    assert_eq!(seen.len(), 3);
}
