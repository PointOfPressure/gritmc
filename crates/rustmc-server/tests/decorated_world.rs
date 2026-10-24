//! The decorated world, end to end: generate it, encode it, look at it.
//!
//! `crates/rustmc-server/src/world.rs` has unit tests for the packing and the
//! air rules, and those would all pass over an empty canvas. What they cannot
//! see is whether the pipeline actually ran — whether a client is being sent
//! trees or a very well-encoded field of stone.
//!
//! So this gate asserts on the *content*: blocks that only decoration can
//! produce, a biome that came from the registry the client was sent, and
//! heightmaps that follow the terrain rather than sitting at the world floor.
//!
//! Skipped without the vanilla extract, like every gate that needs it: the
//! material is gitignored and a checkout that has never run the oracle
//! scripts must not fail here.

use rustmc_server::chunk::ContainerKind;
use rustmc_server::configuration::CapturedRegistries;
use rustmc_server::edits::Blocks;
use rustmc_server::terrain::StateIds;
use rustmc_server::world::World;
use rustmc_worldgen::pipeline::Area;

const SEED: i64 = 20260722;

fn repo(relative: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

/// The world, or `None` if the extract this needs is not on disk.
fn world() -> Option<World> {
    if !worldgen_assembly::have_extract() {
        eprintln!("skipped: no vanilla extract");
        return None;
    }
    let registries = CapturedRegistries::load(&repo("data/vanilla/registries.bin")).ok()?;
    let biomes = registries
        .entry_ids("minecraft:worldgen/biome")
        .expect("readable capture")
        .expect("the capture holds the biome registry");
    let ids = StateIds::load(&repo("data/vanilla/block_props.json")).expect("block table");
    // Radius zero: one chunk offered, nine generated with the margin. Enough
    // to prove the stages ran, cheap enough to be a gate.
    let tables = std::sync::Arc::new(
        rustmc_server::world::WorldTables::load(&biomes, ids).expect("block tables"),
    );
    Some(World::generate(SEED, Area::square(0, 0, 0), tables).expect("generated"))
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
            let id = i32::try_from(e["id"].as_i64()?).ok()?;
            let name = state.split('[').next().unwrap_or(state).to_string();
            Some((id, name))
        })
        .collect()
}

/// Every block name in a chunk.
fn block_names(
    chunk: &rustmc_server::chunk::LevelChunkWithLight,
) -> std::collections::BTreeSet<String> {
    let names = names_by_id();
    chunk
        .sections
        .iter()
        .flat_map(|s| s.states.values(ContainerKind::BlockStates))
        .map(|v| {
            names
                .get(&v)
                .cloned()
                .unwrap_or_else(|| panic!("state id {v} is not in the block table"))
        })
        .collect()
}

#[test]
fn a_served_chunk_holds_blocks_only_decoration_can_produce() {
    let Some(world) = world() else { return };
    let chunk = world.chunk(0, 0).expect("encoded");

    // The witnesses are chosen to be **biome-independent**, which the obvious
    // ones are not: a first draft asked for a grass block and failed because
    // chunk 0,0 at this seed is not grassland. What every overworld chunk has
    // regardless of biome:
    //
    //   bedrock    a surface rule — `bedrock_floor` in the overworld noise
    //              settings, not something the noise stage can place
    //   coal ore   the decoration step, and distinct from the ore *veins* the
    //              noise stage places, which are iron, copper, tuff, granite
    //              and the raw-metal blocks
    let present = block_names(&chunk);
    for (block, stage) in [
        ("minecraft:bedrock", "the surface rules"),
        ("minecraft:coal_ore", "decoration"),
    ] {
        assert!(
            present.contains(block),
            "no {block} in the chunk, so {stage} did not run. Present: {present:?}"
        );
    }
}

/// Every section must agree with itself about how full it is, because
/// `nonEmptyBlockCount` is what decides whether a client bothers to render
/// one. A section that is wrong here renders as thin air.
#[test]
fn the_non_empty_count_matches_the_blocks() {
    let Some(world) = world() else { return };
    let chunk = world.chunk(0, 0).expect("encoded");

    let ids = StateIds::load(&repo("data/vanilla/block_props.json")).expect("block table");
    let air = ids.get("minecraft:air").expect("air");
    let cave_air = ids.get("minecraft:cave_air").expect("cave air");
    let void_air = ids.get("minecraft:void_air").expect("void air");

    let mut total = 0usize;
    for (i, s) in chunk.sections.iter().enumerate() {
        let values = s.states.values(ContainerKind::BlockStates);
        assert_eq!(values.len(), 4096, "section {i} is not a full section");
        let solid = values
            .iter()
            .filter(|v| **v != air && **v != cave_air && **v != void_air)
            .count();
        assert_eq!(
            usize::try_from(s.non_empty_blocks).expect("non-negative"),
            solid,
            "section {i} counts {} but holds {solid}",
            s.non_empty_blocks
        );
        total += solid;
    }
    // A column of world 384 tall is 98,304 blocks; a surface chunk is mostly
    // stone. Anything near zero means the canvas was never filled.
    assert!(
        total > 20_000,
        "only {total} solid blocks — the noise stage did not run"
    );
}

/// Biomes are indices into the client's own registry, so the only useful
/// assertion is that they resolve to a real biome there — a container full of
/// zero is `the_void` and looks exactly like a working world until the
/// foliage comes out grey.
#[test]
fn biomes_resolve_against_the_registry_the_client_was_sent() {
    let Some(world) = world() else { return };
    let chunk = world.chunk(0, 0).expect("encoded");
    let registries =
        CapturedRegistries::load(&repo("data/vanilla/registries.bin")).expect("capture");
    let names = registries
        .entry_ids("minecraft:worldgen/biome")
        .expect("readable")
        .expect("biome registry");

    let mut seen = std::collections::BTreeSet::new();
    for s in &chunk.sections {
        for v in s.biomes.values(ContainerKind::Biomes) {
            let i = usize::try_from(v).expect("a biome index is not negative");
            seen.insert(names.get(i).cloned().unwrap_or_else(|| {
                panic!(
                    "biome index {i} is past the end of a {}-entry registry",
                    names.len()
                )
            }));
        }
    }
    assert!(!seen.is_empty(), "no biomes at all");
    assert!(
        !seen.contains("minecraft:the_void") || seen.len() > 1,
        "every cell is the_void: the biome stage did not run"
    );
}

/// The heightmaps a client is sent, and the one property that makes them
/// worth sending: they follow the ground.
#[test]
fn the_heightmaps_follow_the_terrain() {
    let Some(world) = world() else { return };
    let chunk = world.chunk(0, 0).expect("encoded");

    // WORLD_SURFACE, MOTION_BLOCKING, MOTION_BLOCKING_NO_LEAVES — the three
    // with `usage == CLIENT`, read from the jar.
    let kinds: Vec<i32> = chunk.heightmaps.iter().map(|h| h.kind).collect();
    assert_eq!(kinds, vec![1, 4, 5]);

    for h in &chunk.heightmaps {
        assert_eq!(h.data.len(), 37, "256 columns, nine bits, seven to a long");
        // Exactly 256 columns, seven to a long. The last long holds four and
        // its remaining three slots are padding — reading those as columns
        // is what a first draft of this test did, and zero is what padding
        // looks like.
        for column in 0..256usize {
            let v = (h.data[column / 7] >> ((column % 7) * 9)) & 0x1ff;
            assert!(
                (1..=384).contains(&v),
                "column {column} of heightmap {} reads {v}, which is not a surface",
                h.kind
            );
        }
    }

    // And the surface the spawn uses has to be one a player can stand on.
    let y = world.surface_at(8, 8);
    assert!(
        y > -64.0 && y < 320.0,
        "spawn surface at y={y} is outside the world"
    );
}

/// Breaking a block changes the world a client is sent, not just the packet
/// the breaker gets.
///
/// This is the whole claim of the mutable-world step and it is not provable
/// from `edits.rs` alone: that module round-trips a map, and a map that is
/// never consulted by the encoder would pass every one of its own tests
/// while a player who walked away and back found their hole filled in.
#[test]
fn a_broken_block_is_gone_from_the_next_chunk_a_client_is_sent() {
    let Some(world) = world() else { return };
    let ids = StateIds::load(&repo("data/vanilla/block_props.json")).expect("block table");
    let air = ids.get("minecraft:air").expect("air");

    // The surface at the spawn column, which is a block a real player is
    // standing on rather than one chosen for being convenient.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a surface y inside a 384-tall world"
    )]
    let top = world.surface_at(8, 8) as i32 - 1;
    let before = world.chunk(0, 0).expect("encoded");

    // Section and offset of (8, top, 8) in the canvas's own y-then-z-then-x
    // order, which is the order a paletted container reads its cells in.
    let flat = usize::try_from((top + 64) * 256 + 8 * 16 + 8).expect("a positive index");
    let (section, offset) = (flat / 4096, flat % 4096);
    let was = before.sections[section]
        .states
        .values(ContainerKind::BlockStates)[offset];
    assert_ne!(
        was, air,
        "the surface block is already air; nothing to break"
    );

    let new_id = world
        .set_block((8, top, 8), "minecraft:air")
        .expect("the block changed");
    assert_eq!(new_id, air, "set_block reports the state now there");
    assert_eq!(
        world.set_block((8, top, 8), "minecraft:air"),
        None,
        "breaking the same block twice is not a second change"
    );

    let after = world.chunk(0, 0).expect("re-encoded");
    let now = after.sections[section]
        .states
        .values(ContainerKind::BlockStates)[offset];
    assert_eq!(now, air, "the re-sent chunk still has the block");
    assert_eq!(
        after.sections[section].non_empty_blocks,
        before.sections[section].non_empty_blocks - 1,
        "the section's own count has to follow, or the client renders it as \
         thin air or as solid rock"
    );

    // And the heightmap for that column drops, because the block removed was
    // the top one. Nine bits a column, seven to a long, stored as an offset
    // from the world floor.
    let column = 8 * 16 + 8;
    let read = |c: &rustmc_server::chunk::LevelChunkWithLight| {
        // MOTION_BLOCKING is kind 4, the second of the three sent.
        let h = c.heightmaps.iter().find(|h| h.kind == 4).expect("kind 4");
        (h.data[column / 7] >> ((column % 7) * 9)) & 0x1ff
    };
    assert_eq!(
        read(&after),
        read(&before) - 1,
        "the column's motion-blocking height did not follow the dig"
    );
}

/// A write outside the served square is refused rather than kept.
///
/// The margin is generated so the inside can be finished; it is not itself
/// finished and is never sent to anyone, so an edit there would be invisible
/// and permanent.
#[test]
fn a_write_outside_the_served_area_is_refused() {
    let Some(world) = world() else { return };
    // Radius zero: chunk (0,0) is the whole served area, and (1,0) is margin.
    assert_eq!(world.set_block((20, 64, 0), "minecraft:air"), None);
    // Above the world is refused too, and by the y bound rather than the
    // area one.
    assert_eq!(world.set_block((8, 400, 8), "minecraft:air"), None);
    assert!(
        world.edits().is_empty(),
        "a refused write left an edit behind"
    );
}

/// Placing a block: the other half of a mutable world, against the real one.
///
/// The three properties are the ones breaking already has, because they are
/// the ones a client actually reads: the re-encoded chunk, the section's own
/// count, and the column's heightmap.
#[test]
fn a_placed_block_appears_in_the_next_chunk_a_client_is_sent() {
    let Some(world) = world() else { return };
    let ids = StateIds::load(&repo("data/vanilla/block_props.json")).expect("block table");

    // One block above the surface, which is where a right-click on the top
    // face of the surface block lands.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a surface y inside a 384-tall world"
    )]
    let target_y = world.surface_at(4, 4) as i32;
    // `canBeReplaced(BlockPlaceContext)` needs a context. Holding stone,
    // which is what this test goes on to place, so the item-aware conjunct
    // is exercised rather than side-stepped with an empty hand: air is
    // replaceable by stone and stone is not.
    let holding_stone = rustmc_server::placement::Replacing {
        item: Some("minecraft:stone"),
        face: rustmc_server::interact::Face::Up,
        cursor_y: 1.0,
        replacing_clicked: true,
        secondary_use: false,
        table: None,
    };
    assert!(
        world.replaceable((4, target_y, 4), &holding_stone),
        "the block above the surface should be placeable into"
    );

    let state = world
        .default_state("minecraft:stone")
        .expect("stone has a default state");
    let before = world.chunk(0, 0).expect("encoded");
    let placed = world
        .set_block((4, target_y, 4), &state)
        .expect("the block changed");
    assert_eq!(placed, ids.get(&state).expect("a network id"));
    assert!(
        !world.replaceable((4, target_y, 4), &holding_stone),
        "stone is not replaceable, so a second placement there is refused"
    );

    let after = world.chunk(0, 0).expect("re-encoded");
    let flat = usize::try_from((target_y + 64) * 256 + 4 * 16 + 4).expect("a positive index");
    let (section, offset) = (flat / 4096, flat % 4096);
    assert_eq!(
        after.sections[section]
            .states
            .values(ContainerKind::BlockStates)[offset],
        placed,
        "the placed block is not in the re-sent chunk"
    );
    assert_eq!(
        after.sections[section].non_empty_blocks,
        before.sections[section].non_empty_blocks + 1
    );

    let column = 4 * 16 + 4;
    let read = |c: &rustmc_server::chunk::LevelChunkWithLight| {
        let h = c.heightmaps.iter().find(|h| h.kind == 4).expect("kind 4");
        (h.data[column / 7] >> ((column % 7) * 9)) & 0x1ff
    };
    assert_eq!(
        read(&after),
        read(&before) + 1,
        "the column's motion-blocking height did not follow the placement"
    );
}

/// The item table is what turns "the player clicked" into "place *this*", and
/// it is extracted rather than transcribed — so the thing worth asserting is
/// that the extract is the shape the loader expects and that a known id
/// resolves to the name vanilla's own report gives it.
#[test]
fn the_item_table_resolves_the_ids_the_wire_carries() {
    let path = repo("data/vanilla/items.json");
    if !path.exists() {
        eprintln!("skipped: no data/vanilla/items.json — run ci/dump-items.sh");
        return;
    }
    let items = rustmc_server::player::ItemIds::load(&path).expect("item table");
    assert!(items.len() > 1000, "only {} items", items.len());

    // Cross-checked against the report the table is generated from, so this
    // is not the loader agreeing with itself.
    let report = repo("oracle-work/vanilla-26.2/generated/reports/registries.json");
    if !report.exists() {
        return;
    }
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(report).expect("report")).expect("json");
    let entries = doc["minecraft:item"]["entries"]
        .as_object()
        .expect("item entries");
    for name in ["minecraft:stone", "minecraft:oak_planks", "minecraft:torch"] {
        let id = i32::try_from(entries[name]["protocol_id"].as_i64().expect("id")).expect("i32");
        assert_eq!(items.name(id), Some(name), "id {id} should be {name}");
    }
}
