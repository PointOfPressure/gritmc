//! Does what is **inside** a chest survive a restart?
//!
//! `save_round_trip.rs` asks whether a placed block comes back. This asks the
//! harder half: a chest is two things, a block state and twenty-seven item
//! stacks held somewhere else entirely, and the block half was already
//! covered. A chest that comes back empty is worse than no chest at all —
//! it looks like it worked.
//!
//! # Why this is its own test binary
//!
//! [`rustmc_server::blockentity::BlockEntities::shared`] is a process-wide
//! static, and `cargo test` runs the tests in one binary on parallel threads.
//! A block entity created here would be visible to `save_round_trip.rs`'s
//! `save_to`, whose first arm asserts that **one** chunk was written — so
//! putting this in that file would make two unrelated tests flake against
//! each other depending on which thread got there first. A separate
//! integration test is a separate process and therefore a separate static.
//!
//! For the same reason this file holds exactly **one** `#[test]`. Two would
//! share the static with each other.
//!
//! # The arms, and why no one of them is enough
//!
//! The failure this is really guarding against is a save path that carries
//! the *block* and drops the *block entity*, because that is the shape the
//! tree already had: `ChunkColumn` grew a `block_entities` field, and nothing
//! in `World::save_to` filled it in. Every arm below is red under that bug
//! except the ones that are red under its opposite.
//!
//! * **Arm 1, the chest block is back.** Red if the save path broke
//!   generally. Passes on its own on a server that saves blocks and forgets
//!   contents — which is the bug — so it is necessary and nowhere near
//!   sufficient.
//! * **Arm 2, the stone is in slot 3 of it.** The claim. Red on a save that
//!   wrote no `block_entities`, red on a load that read none, and red on a
//!   round trip that lost the slot index.
//! * **Arm 3, the count is 7 and not 1.** A separate field with a separate
//!   way of being wrong: `ItemStackWithSlot`'s codec defaults a missing
//!   `count` to 1, so a writer that omitted it would come back as a single
//!   stone and arms 1 and 2 would both still pass.
//! * **Arm 4, slot 0 is still empty.** Bounds the reload from above. A load
//!   that filled every slot, or that dropped the `Slot` key and appended in
//!   order, satisfies arms 1 to 3 and fails this.
//! * **Arm 5, a chest that was never filled comes back empty and present.**
//!   The degenerate case. `ContainerHelper.saveAllItems` skips empty stacks,
//!   so an empty chest's `Items` list is empty — and a reader that treated an
//!   empty list as "no block entity" would drop the chest itself.
//!
//! # The restart is real
//!
//! The second world is generated from the seed rather than reused, and the
//! block-entity store is cleared, because a store that still held the chest
//! would make the load unfalsifiable — every arm would pass against a
//! `load_from` that read nothing at all. That is the same trap
//! `save_round_trip.rs` documents for the overlay, and it applies with more
//! force here because the store is a static and does not go away when the
//! `World` does.
//!
//! Skipped without the vanilla extract, like every gate that needs it.

use rustmc_server::blockentity::{BlockEntities, BlockEntity, Kind};
use rustmc_server::edits::Blocks;
use rustmc_server::item::ItemStack;
use rustmc_server::terrain::StateIds;
use rustmc_server::world::{World, WorldTables};
use rustmc_worldgen::pipeline::Area;

const SEED: i64 = 20260722;
/// Where the filled chest goes. High enough to be air in the generated world.
const CHEST: (i32, i32, i32) = (5, 100, 5);
/// And an empty one, in the same chunk so the chunk count stays predictable.
const EMPTY_CHEST: (i32, i32, i32) = (7, 100, 5);
/// The slot the stone goes in — deliberately **not** 0, so a reader that
/// ignored the `Slot` key and appended in order would put it in the wrong
/// place and arm 4 would catch it.
const SLOT: usize = 3;
/// And deliberately **not** 1, so a writer that omitted `count` would come
/// back as a single stone.
const COUNT: i32 = 7;

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

/// The block half of a state string — `minecraft:chest[facing=north,...]`
/// is a chest.
fn block_of(state: &str) -> &str {
    state.split('[').next().unwrap_or(state)
}

fn world(tables: &std::sync::Arc<WorldTables>) -> World {
    World::generate(SEED, Area::square(0, 0, 1), std::sync::Arc::clone(tables)).expect("generate")
}

struct Scratch(std::path::PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn scratch(tag: &str) -> Scratch {
    let dir = std::env::temp_dir().join(format!("rustmc-be-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    Scratch(dir)
}

#[test]
fn a_chest_keeps_its_contents_across_a_restart() {
    let Some(tables) = inputs() else {
        return;
    };
    let dir = scratch("round-trip");
    let store = BlockEntities::shared();
    store.clear();

    // --- before the restart -------------------------------------------------
    let before = world(&tables);
    assert_ne!(
        block_of(&before.block_at(CHEST)),
        "minecraft:chest",
        "the generator already put a chest at {CHEST:?}; nothing below would prove anything"
    );

    // **The canonical state, not the block name.** `set_block` resolves
    // against the global state table, and `minecraft:chest` on its own is not
    // in it — every chest state carries `facing`, `type` and `waterlogged`.
    // Passing the bare name is refused rather than defaulted, which is
    // `World::set_block` declining to guess; the first version of this test
    // did exactly that and failed with "not a state this server knows".
    let chest = before
        .default_state("minecraft:chest")
        .expect("the block table knows what a chest is");

    // `set_block` is the path that creates the block entity — see
    // `World::set_block`'s call to `on_block_changed`. Going through the
    // overlay directly would not, which is exactly what `load_from` relies on.
    assert!(
        before.set_block(CHEST, &chest).is_some(),
        "{chest} is not a state this server knows"
    );
    assert!(before.set_block(EMPTY_CHEST, &chest).is_some());
    assert_eq!(
        store.len(),
        2,
        "placing two chests created two block entities"
    );

    store
        .with_mut(CHEST, |be| {
            be.set_item(SLOT, ItemStack::new("minecraft:stone", COUNT));
        })
        .expect("the chest we just placed has a block entity");

    let chunks = before.save_to(&dir.0).expect("save");
    assert!(chunks >= 1, "the edited chunk was written");

    // --- the restart --------------------------------------------------------
    drop(before);
    // **The static does not go away with the `World`.** Without this the
    // chest would still be in the store and every arm below would pass
    // against a `load_from` that read nothing.
    store.clear();
    assert_eq!(store.len(), 0);

    let after = world(&tables);
    assert_ne!(
        block_of(&after.block_at(CHEST)),
        "minecraft:chest",
        "a freshly generated world must not already carry the chest, or the \
         load below is unfalsifiable"
    );

    after.load_from(&dir.0).expect("load");

    // Arm 1: the block is back.
    assert_eq!(
        block_of(&after.block_at(CHEST)),
        "minecraft:chest",
        "the chest block itself did not survive the restart"
    );

    // Arm 2: and so is what was in it.
    let be = store.get(CHEST).expect(
        "the chest came back as a block, but with no block entity — \
                 its contents were dropped somewhere between save and load",
    );
    let back = be.item(SLOT);
    assert_eq!(
        back.item(),
        "minecraft:stone",
        "slot {SLOT} of the reloaded chest does not hold what was put in it"
    );

    // Arm 3: with the right count, which is a separate field.
    assert_eq!(
        back.count(),
        COUNT,
        "the stack came back as {} rather than {COUNT}; a missing `count` key \
         decodes as 1 and would look like this",
        back.count()
    );

    // Arm 4: and nothing else. The slot index round-tripped rather than the
    // stack being appended at 0.
    assert!(
        be.item(0).is_empty(),
        "slot 0 of the reloaded chest is not empty, so the `Slot` key was not \
         honoured and the stack landed wherever the list order put it"
    );
    assert_eq!(be.size(), Kind::Chest.container_size());

    // Arm 5: the chest nobody filled is still a chest, and still empty. An
    // empty container writes an empty `Items` list, and a reader that took
    // that for "no block entity here" would lose the chest itself.
    let empty = store
        .get(EMPTY_CHEST)
        .expect("the chest that was never filled was dropped by the round trip");
    assert_eq!(empty.kind(), Kind::Chest);
    assert!(empty.is_empty());

    // And the store holds those two and nothing invented.
    assert_eq!(store.len(), 2, "two chests in, two chests out");

    // --- the tile that is not this one ---------------------------------
    //
    // **The block-entity store is process-wide and a `World` is one tile.**
    // `crate::tiles` holds several and calls `save_to` on each in turn, so
    // every tile's save sees every other tile's chests. `save_to` turns a
    // chunk this world does not hold into an `Err` — right for an edit, which
    // should be impossible, and wrong for these, which are routine. Without
    // the `holds_chunk` filter the second tile a player visited would make
    // every save of the first one fail, and no test above would notice
    // because they all use one tile.
    store.insert((10_000, 64, 10_000), BlockEntity::new(Kind::Chest));
    after
        .save_to(&dir.0)
        .expect("a block entity in another tile's chunk must be skipped, not an error");

    store.clear();
}

/// A compile-time reminder that [`BlockEntity`] is reachable from here; the
/// test above uses it only through the store, and an unused import would be a
/// gate failure rather than a warning.
#[allow(dead_code)]
fn _uses_block_entity() -> BlockEntity {
    BlockEntity::new(Kind::Chest)
}
