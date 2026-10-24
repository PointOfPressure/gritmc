//! `crate::protocol_ids::shared()` reads `data/vanilla/protocol_ids.json`
//! relative to the process's **working directory**, and every `cargo test`
//! binary in this workspace runs with `crates/rustmc-server` as that
//! directory rather than the repo root — a package's own manifest dir, not
//! the workspace root. Before `protocol_ids::shared` grew a compile-time
//! (`CARGO_MANIFEST_DIR`) fallback for the cwd-relative lookup failing, this
//! made the table read as `None` on **every** `cargo test` invocation
//! regardless of what was on disk: `World::chunk`'s (future) block-entity
//! list and `broadcast::broadcast_block_entity_if_needed` (already live —
//! see `crates/rustmc-server/src/broadcast.rs`) both resolve a block
//! entity's wire type id through this table, and a correct id and a wrong
//! one produced the identical `None` under any unit test. Not a check that
//! could fail — a function no check could reach.
//!
//! This is the witness. It calls the exact zero-argument function that was
//! unreachable, cross-checks the id it returns against an independent read
//! of the same generated file, and does so alongside a real, end-to-end
//! `World::chunk` call built the same way `tests/decorated_world.rs` builds
//! one — not a fixture standing in for the pipeline.
//!
//! `World::chunk` does not yet build its `block_entities` field from
//! `crate::blockentity::BlockEntities` — `docs/PARITY-ROADMAP.md` §32
//! (`world-send-chunk-block-entities`, unmerged as of this test) is the
//! branch that wires that in. So the chunk-payload assertion below is
//! honest about what it can see today: the list is empty because nothing
//! reads the store into it yet, not because nothing was placed. What this
//! file proves is that the two pieces §32 will need —
//! `BlockEntities::in_chunk` and `protocol_ids::shared().block_entity_type`
//! — are both reachable and both correct under the same working directory
//! that made the second of them invisible.
//!
//! Skipped without the vanilla extract, like every gate that needs it.

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
    let tables = std::sync::Arc::new(
        rustmc_server::world::WorldTables::load(&biomes, ids).expect("block tables"),
    );
    Some(World::generate(SEED, Area::square(0, 0, 0), tables).expect("generated"))
}

/// `minecraft:chest`'s `Registries.BLOCK_ENTITY_TYPE` id, read directly from
/// `data/vanilla/protocol_ids.json` — independent of
/// `crate::protocol_ids::ProtocolIds` entirely, so a test that compares
/// against this is not the loader agreeing with itself.
fn chest_type_id_from_the_raw_table() -> i32 {
    let path = repo("data/vanilla/protocol_ids.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} — run ci/gen-gameplay-tables.sh ({e})", path.display()));
    let doc: serde_json::Value = serde_json::from_str(&text).expect("json");
    let id = doc["block_entity_types"]["minecraft:chest"]
        .as_i64()
        .expect("minecraft:chest has a block_entity_types entry");
    i32::try_from(id).expect("a block-entity type id fits in i32")
}

/// The function this item exists to unlock, called exactly as its two real
/// callers call it: no path, no override, nothing this test hands it.
///
/// This is not gated on `have_extract()` the way the other test in this file
/// is — it needs only `data/vanilla/protocol_ids.json`, which
/// `ci/gen-gameplay-tables.sh` produces from `ci/dump-protocol-ids.sh`
/// without booting a `World` at all — but it is still skipped when the table
/// itself is absent, for the same reason every other golden-backed test in
/// this crate skips rather than fails a checkout that has never run the
/// generator.
#[test]
fn protocol_ids_shared_resolves_under_the_cargo_test_working_directory() {
    if !repo("data/vanilla/protocol_ids.json").exists() {
        eprintln!("skipped: no data/vanilla/protocol_ids.json — run ci/gen-gameplay-tables.sh");
        return;
    }
    let ids = rustmc_server::protocol_ids::shared().expect(
        "protocol ids unreachable under cargo test's own working directory — \
         crate::protocol_ids::shared()'s cwd-relative lookup, and its \
         CARGO_MANIFEST_DIR fallback, both failed",
    );
    let want = chest_type_id_from_the_raw_table();
    assert_eq!(
        ids.block_entity_type("minecraft:chest"),
        Some(want),
        "a wrong id here decodes cleanly on the wire and names a different \
         block entity type"
    );
}

/// `World::chunk`, end to end, alongside the block-entity store and the id
/// table its future wiring needs — see the module doc for what this can and
/// cannot see before `docs/PARITY-ROADMAP.md` §32 lands.
#[test]
fn world_chunk_runs_end_to_end_beside_the_block_entity_store_and_its_ids() {
    let Some(world) = world() else { return };

    // Place a chest for real, through the same `World::set_block` production
    // uses. This already feeds `crate::blockentity::BlockEntities` today —
    // `world.rs`'s `Blocks::set_block` calls `on_block_changed` on every
    // write — so this half of the pipeline needs no fix and this test does
    // not claim to have made one.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a surface y inside a 384-tall world"
    )]
    let target_y = world.surface_at(4, 4) as i32;
    let state = world
        .default_state("minecraft:chest")
        .expect("chest has a default state");
    world
        .set_block((4, target_y, 4), &state)
        .expect("the block changed");

    // The store side: the real function, over the real chunk, no fixture.
    let stored = rustmc_server::blockentity::BlockEntities::shared().in_chunk(0, 0);
    assert!(
        stored.iter().any(|(pos, be)| *pos == (4, target_y, 4)
            && be.kind() == rustmc_server::blockentity::Kind::Chest),
        "the chest just placed is not in its own chunk's block-entity store: {stored:?}"
    );

    // The id side: the function this item exists to unlock, cross-checked
    // against an independent read rather than trusted.
    let ids = rustmc_server::protocol_ids::shared().expect("protocol ids reachable");
    assert_eq!(
        ids.block_entity_type("minecraft:chest"),
        Some(chest_type_id_from_the_raw_table())
    );

    // `World::chunk` itself, called end to end over the real encoder and the
    // real canvas. Its `block_entities` field DOES read the store above now:
    // `world_send: World::chunk sends a chest's chunk-payload block-entity
    // entry` (94fe1efbb) built it from `BlockEntities::shared().in_chunk`
    // through `blockentity::chunk_payload_entry`. This assertion used to pin
    // the field empty and said, in its own message, that a change building it
    // from the store had to update it alongside — that is this update, and it
    // asserts the entry's content rather than merely that one exists, because
    // "non-empty" would go green on an entry at the wrong position, of the
    // wrong type, or carrying the wrong tag.
    let chunk = world.chunk(0, 0).expect("encoded");
    let want_xz = ((4 & 15) << 4) | (4 & 15);
    let entry = chunk
        .block_entities
        .iter()
        .find(|be| be.packed_xz == want_xz)
        .unwrap_or_else(|| {
            panic!(
                "the chest placed at (4, {target_y}, 4) has no chunk-payload \
                 entry at packed_xz {want_xz}: {:?}",
                chunk.block_entities
            )
        });
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a surface y inside a 384-tall world"
    )]
    let want_y = target_y as i16;
    assert_eq!(
        entry.y, want_y,
        "the entry is at the y the chest was placed"
    );
    assert_eq!(
        entry.kind,
        chest_type_id_from_the_raw_table(),
        "the entry carries the chest's own registry id, not another type's"
    );
    assert_eq!(
        entry.nbt,
        vec![0u8],
        "a chest's getUpdateTag() is null, which encodes as the single END byte"
    );
}
