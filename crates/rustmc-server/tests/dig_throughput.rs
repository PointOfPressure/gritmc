//! **What the plugin hook costs a server with no plugins.**
//!
//! `#[ignore]`d, so it is not in any gate: it is an instrument, and its number
//! only means anything beside the same number taken from a control tree on the
//! same box in the same minute.
//!
//! ```bash
//! cargo test --release -p rustmc-server --test dig_throughput -- --ignored --nocapture
//! ```
//!
//! # The control, and why this file is written to compile without the hook
//!
//! The honest question is not "is `plugins: None` as fast as
//! `plugins: Some(&empty)`" — both of those are *this* tree, and both pay
//! whatever the hook costs. It is "is this tree as fast as the tree before the
//! hook existed", and answering it needs the same loop built in both.
//!
//! So the two lines here that a pre-hook tree cannot compile are marked
//! `// HOOK-ONLY`, and `ci/plugin-hook-cost.sh` deletes exactly those lines
//! when it plants this file in a `git archive` checkout of the control commit.
//! Everything else is byte-identical: same world, same packets, same iteration
//! count, same compiler, same box.
//!
//! The subject measures the **production** configuration and not a convenient
//! one: `install_global(first_party())` runs, exactly as `status::listen` runs
//! it, so `global()` answers `Some(&Runtime)` with zero handlers — which is
//! what a shipped server with no plugins actually has, and is one `usize` load
//! more expensive than the `None` a test would get for free.
//!
//! # Why a dig loop rather than a microbenchmark of the gate
//!
//! Timing `block_break_denied` alone would measure a branch predictor being
//! right two million times in a row and would say nothing about a server. The
//! loop below is the whole production path a dig takes: frame decode, tick
//! lock, `player_will_destroy`, `Level.setBlock`, the neighbour pass, the
//! change record and the acknowledgement.

use std::io::Write;
use std::sync::{Arc, Mutex};

use rustmc_server::edits::{BlockEdits, Blocks};
use rustmc_server::interact::BlockPos;

const DIGS: usize = 200_000;

/// A world whose writes are visible to the next read, so a dig does the work
/// a dig does rather than being deduplicated away.
struct World {
    edits: BlockEdits,
}

const STATES: [&str; 2] = ["minecraft:air", "minecraft:stone"];

impl Blocks for World {
    fn state_at(&self, pos: BlockPos) -> String {
        self.edits
            .get(pos)
            .unwrap_or_else(|| "minecraft:air".to_owned())
    }
    fn set_block(&self, pos: BlockPos, state: &str) -> Option<i32> {
        if !self.has_state(state) || self.state_at(pos) == state {
            return None;
        }
        self.edits.set(pos, state);
        self.state_id(state)
    }
    fn default_state(&self, block: &str) -> Option<String> {
        self.has_state(block).then(|| block.to_owned())
    }
    fn has_state(&self, state: &str) -> bool {
        STATES.contains(&state)
    }
    fn state_id(&self, state: &str) -> Option<i32> {
        STATES
            .iter()
            .position(|s| *s == state)
            .and_then(|i| i32::try_from(i).ok())
    }
    fn replaceable(&self, pos: BlockPos, _: &rustmc_server::placement::Replacing<'_>) -> bool {
        self.state_at(pos) == "minecraft:air"
    }
    fn face_sturdy(&self, pos: BlockPos, _: rustmc_server::interact::Face) -> bool {
        self.state_at(pos) != "minecraft:air"
    }
    fn redstone_conductor(&self, pos: BlockPos) -> bool {
        self.state_at(pos) != "minecraft:air"
    }
    fn signal_source(&self, _: BlockPos) -> bool {
        false
    }
    fn water_at(&self, _: BlockPos) -> bool {
        false
    }
    fn legacy_fluid_at(&self, _: BlockPos) -> String {
        "minecraft:air".to_owned()
    }
}

#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<u8>>>);
impl Write for Sink {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        // Length only. Keeping the bytes would make this measure a `Vec`
        // growing to a few hundred megabytes rather than a server digging.
        self.0.lock().expect("sink").clear();
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// One `player_action` frame: creative destroy at `pos`.
/// `MOVE_PLAYER_POS` to the top of `pos`, on the ground.
fn stand_on(pos: BlockPos) -> Vec<u8> {
    let mut body = Vec::new();
    rustmc_proto::wire::write_f64(&mut body, f64::from(pos.0) + 0.5).expect("buf");
    rustmc_proto::wire::write_f64(&mut body, f64::from(pos.1) + 1.0).expect("buf");
    rustmc_proto::wire::write_f64(&mut body, f64::from(pos.2) + 0.5).expect("buf");
    rustmc_proto::wire::write_u8(&mut body, 1).expect("buf");
    let mut frame = Vec::new();
    rustmc_proto::frame::write_frame(
        &mut frame,
        None,
        rustmc_proto::ids::play::sb::MOVE_PLAYER_POS,
        &body,
    )
    .expect("buf");
    frame
}

fn dig(pos: BlockPos, sequence: i32) -> Vec<u8> {
    let mut body = Vec::new();
    rustmc_proto::varint::write_varint(&mut body, 0).expect("buf");
    let packed = (i64::from(pos.0) & 0x3FF_FFFF) << 38
        | (i64::from(pos.2) & 0x3FF_FFFF) << 12
        | (i64::from(pos.1) & 0xFFF);
    rustmc_proto::wire::write_i64(&mut body, packed).expect("buf");
    rustmc_proto::wire::write_u8(&mut body, 1).expect("buf");
    rustmc_proto::varint::write_varint(&mut body, sequence).expect("buf");
    let mut frame = Vec::new();
    rustmc_proto::frame::write_frame(
        &mut frame,
        None,
        rustmc_proto::ids::play::sb::PLAYER_ACTION,
        &body,
    )
    .expect("buf");
    frame
}

#[test]
#[ignore = "an instrument, not a gate: run it with --ignored beside a control tree"]
fn dig_throughput_with_no_plugins() {
    let world = World {
        edits: BlockEdits::new(),
    };
    // A column of stone per dig, so no dig is a no-op and none of them share
    // a position with another.
    let mut input = Vec::new();
    for i in 0..DIGS {
        let pos = (
            i32::try_from(i % 512).expect("small") - 256,
            i32::try_from(i / 512).expect("small") % 128,
            8,
        );
        world.edits.set(pos, "minecraft:stone");
        // Stand on the block first. `handleBlockBreakAction` refuses a dig
        // outside `isWithinBlockInteractionRange` before any hook is asked,
        // so a fixed player would have timed 199,990 refusals. The same
        // packet goes into the control tree's copy, so the pair still
        // differs only by the `// HOOK-ONLY` lines.
        input.extend(stand_on(pos));
        input.extend(dig(pos, i32::try_from(i).expect("small")));
    }

    // Exactly what `status::listen` does at boot, so the measured path is the
    // shipped one.
    rustmc_server::plugins::install_global(rustmc_server::plugins::first_party()); // HOOK-ONLY

    let players = rustmc_server::players::Players::new();
    let conn = Arc::new(rustmc_server::players::Connection::with_entity(
        players.next_id(),
        "Bench",
        Box::new(Sink::default()),
        None,
        0,
        2,
        (0, 0),
    ));
    players.join(&conn);
    // The join's own two steps, which is what puts this connection into the
    // recipient list of `ChunkHolder.broadcastChanges`: `enter_world` writes
    // the spawn blob and tells the view (`mark_sent`), then
    // `ChunkMap.updatePlayerStatus(player, true)` runs `updateChunkTracking`.
    // A view that has had neither is `EMPTY`, and vanilla withholds every
    // block update from a client in that state.
    conn.with_chunk_view(|v| {
        for dx in -4..=4 {
            for dz in -4..=4 {
                v.mark_sent((dx, dz));
            }
        }
        v.update_tracking((0, 0));
    });
    let clock = rustmc_server::tick::GameClock::new();
    let changed = rustmc_server::broadcast::ChangedBlocks::new();
    let block_events = rustmc_server::containers::BlockEvents::new();
    let openers = rustmc_server::containers::Openers::new();
    let mut player = rustmc_server::player::PlayerState::at(8.0, 64.0, 8.0);
    let mut hotbar = rustmc_server::player::Hotbar::new();
    let mut menu = rustmc_server::menu::MenuState::new();
    let mut known_recipes = std::collections::BTreeSet::new();
    let chunk_at = |_: i32, _: i32| Ok(None);
    // A private manager per test: `cargo test` runs these on
    // parallel threads in one process, and the shared one would
    // let one test's join make a connection another test's move
    // then reports on.
    let wp = rustmc_server::waypoints::Waypoints::new();
    let mut session = rustmc_server::world_send::Session {
        conn: &conn,
        player: &mut player,
        clock: &clock,
        clock_ids: &[],
        players: &players,
        blocks: Some(&world),
        hotbar: &mut hotbar,
        menu: &mut menu,
        items: None,
        placement: None,
        recipes: None,
        recipe_unlocks: None,
        known_recipes: &mut known_recipes,
        advancements: None,
        adv_engine: None,
        uses: None,
        item_uses: None,
        survive: None,
        bonemeal: None,
        placed_by: None,
        shapes: None,
        sound_types: None,
        sound_events: None,
        waypoints: &wp,
        ticks: None,
        fire_odds: None,
        changed: &changed,
        block_events: &block_events,
        openers: &openers,
        protocol_ids: None,
        chunk_at: &chunk_at,
        plugins: rustmc_server::plugins::global(), // HOOK-ONLY
        creative: true,
        // This fixture sends dig packets and never a command, so the value is
        // not under test; it is written out because `Session` has no
        // `Default` and deliberately should not gain one. See the note on the
        // same field in `world_send`'s plugin-hook fixture.
        seed: 0,
    };

    let start = std::time::Instant::now();
    let mut cursor = std::io::Cursor::new(input);
    rustmc_server::world_send::keep_alive_loop(&mut cursor, &mut session).expect("ends on EOF");
    let elapsed = start.elapsed();

    #[allow(clippy::cast_precision_loss)]
    let per = elapsed.as_nanos() as f64 / DIGS as f64;
    println!("dig_throughput digs={DIGS} wall={elapsed:?} ns_per_dig={per:.1}");
    assert_eq!(
        Blocks::state_at(&world, (-256, 0, 8)),
        "minecraft:air",
        "the loop really dug"
    );
}
