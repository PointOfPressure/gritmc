//! Does a player's build survive a restart?
//!
//! The server holds player writes in an in-memory overlay
//! ([`rustmc_server::edits`]) and, until this branch, never wrote them
//! anywhere. A block placed, a disconnect, a restart, and the block was gone —
//! not lost on the way to disk, but never on a path to disk at all.
//!
//! This is the round trip at the level a player experiences it: change a
//! block, save, throw the world away, generate it again from the same seed,
//! load, and ask whether the block is there.
//!
//! # Why the second world is generated rather than reused
//!
//! Reusing the first `World` would leave its overlay populated, and every
//! assertion below would pass on a `save_to` that wrote nothing and a
//! `load_from` that read nothing. The restart has to be real for the test to
//! be about anything, so the second world is built from the seed with an
//! empty overlay — which is exactly what a restarted process has.
//!
//! # The pair
//!
//! Arm 1 — "the sentinel is back" — passes on its own on a `load_from` that
//! indiscriminately copies every block of the saved chunk into the overlay.
//! That would "work" and would be wrong twice over: it destroys the invariant
//! that the overlay means *how this world differs from its seed*, and it would
//! make a reloaded world carry a 98,304-entry overlay per chunk.
//!
//! Arm 2 — "exactly one block was restored" — passes on its own on a save that
//! wrote nothing at all, because zero restored blocks is trivially "no
//! extras"... except that arm 1 is then red. That is the pairing: arm 2 bounds
//! the count from above and arm 1 from below, so the only way through both is
//! a save that really wrote the column and a load that really diffed it.
//!
//! Measured, not argued: with the overlay deliberately not applied in
//! `World::save_to`, arm 1 fails with "the block a player placed did not
//! survive the restart" and the second test still passes — which is the
//! degenerate case this pairing exists to catch.
//!
//! Skipped without the vanilla extract, like every gate that needs it.

use rustmc_server::edits::Blocks;
use rustmc_server::terrain::StateIds;
use rustmc_server::world::{World, WorldTables};
use rustmc_worldgen::pipeline::Area;

const SEED: i64 = 20260722;
/// A block no generator stage here places, so finding it can only mean the
/// save carried it.
const SENTINEL: &str = "minecraft:diamond_block";

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

fn world(tables: &std::sync::Arc<WorldTables>) -> World {
    World::generate(SEED, Area::square(0, 0, 1), std::sync::Arc::clone(tables)).expect("generate")
}

/// A scratch region directory that cleans up after itself.
struct Scratch(std::path::PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn scratch(tag: &str) -> Scratch {
    let dir = std::env::temp_dir().join(format!("rustmc-save-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    Scratch(dir)
}

/// Both arms in one generation pass, because generating two decorated worlds
/// costs seconds and doing it twice to make two `#[test]`s would double it
/// for no extra evidence.
#[test]
fn a_placed_block_survives_a_restart_and_brings_nothing_else_with_it() {
    let Some(tables) = inputs() else {
        return;
    };
    let dir = scratch("round-trip");

    // --- before the restart -------------------------------------------------
    let before = world(&tables);
    // Somewhere in the middle of the square, high enough to be air.
    let pos = (5, 100, 5);
    let generated = before.block_at(pos);
    assert_ne!(
        generated, SENTINEL,
        "the generator already puts {SENTINEL} at {pos:?}; the sentinel proves nothing there"
    );
    // A neighbour we never touch. Read now, on the pristine world, so the
    // comparison after the restart is against generation rather than against
    // whatever the reload happened to produce.
    let control_pos = (6, 100, 5);
    let control = before.block_at(control_pos);

    assert!(
        before.set_block(pos, SENTINEL).is_some(),
        "{SENTINEL} is not a state this server knows"
    );
    assert_eq!(before.block_at(pos), SENTINEL, "the overlay took the write");

    let chunks = before.save_to(&dir.0).expect("save");
    assert_eq!(chunks, 1, "one edited chunk, so one chunk written");

    // --- the restart --------------------------------------------------------
    drop(before);
    let after = world(&tables);
    assert_eq!(
        after.block_at(pos),
        generated,
        "a freshly generated world must not already carry the edit, or the \
         load below is unfalsifiable"
    );

    let restored = after.load_from(&dir.0).expect("load");

    // Arm 1: the build is back.
    assert_eq!(
        after.block_at(pos),
        SENTINEL,
        "the block a player placed did not survive the restart"
    );
    // Arm 2: and nothing else came with it. One edit in, one block out.
    assert_eq!(
        restored, 1,
        "the reload put {restored} blocks into the overlay for a single placed \
         block; the overlay means `how this world differs from its seed`, and \
         a load that copies the whole chunk destroys that"
    );
    // Arm 3: the untouched neighbour still reads as the generator made it.
    assert_eq!(
        after.block_at(control_pos),
        control,
        "a block nobody edited changed across the save/load round trip"
    );
}

/// Saving a world nobody has touched writes nothing, and loading it back is a
/// no-op.
///
/// The boring case, and it is here because the interesting one cannot see it:
/// a `save_to` that wrote every chunk of the square would pass every arm
/// above and would turn a first boot into a region directory the size of the
/// world.
#[test]
fn an_untouched_world_saves_nothing() {
    let Some(tables) = inputs() else {
        return;
    };
    let dir = scratch("untouched");

    let w = world(&tables);
    assert_eq!(w.save_to(&dir.0).expect("save"), 0, "nothing was edited");
    assert_eq!(
        w.load_from(&dir.0).expect("load"),
        0,
        "nothing was saved, so nothing can be restored"
    );
}
