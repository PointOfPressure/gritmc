//! **The plugin contract's threading guarantee, observed — in a process where
//! nothing else can be holding the lock.**
//!
//! `grit-plugins/docs/CONTRACT.md` §3 promises a plugin author that exactly one
//! callback runs at any instant, server-wide, and that a plugin therefore needs
//! no locking of its own. This server keeps that promise entirely by holding
//! `broadcast::tick_lock` across every connection-thread packet handler. It is
//! a guarantee with no type to enforce it, so it gets a test that can see it.
//!
//! # Why this is a separate test binary, and it is not tidiness
//!
//! It lived in `world_send`'s unit tests first, and the falsification sweep
//! caught it red-handed: `ci/plugin-hook-falsify.sh` M13 deletes the
//! `tick_lock()` line from `handle_player_action`, and the test **caught it on
//! one run and missed it on the next**.
//!
//! The tick lock is process-wide, `cargo test` runs a module's tests on
//! parallel threads in one process, and dozens of other tests in that binary
//! take the same lock. So "is the tick lock held while my handler runs" was
//! being answered `true` by whichever *other* test happened to be mid-handler
//! at that instant — the right answer arriving from the wrong mechanism, which
//! is precisely the shape this project's falsification passes exist to find.
//!
//! An integration test is its own binary. This file holds exactly one test, so
//! the only thing that can be holding the tick lock while it runs is the call
//! it made itself. **Do not add a second test to this file**; a second one
//! would restore the contamination.

use std::io::Write;
use std::sync::{Arc, Mutex};

use grit_plugin_api::event::{BlockBreak, Priority, Verdict};
use grit_plugin_api::{
    CapabilitySet, Ctx, Manifest, Plugin, PluginError, PluginId, Setup, API_VERSION,
};
use rustmc_server::edits::{BlockEdits, Blocks};
use rustmc_server::interact::BlockPos;
use rustmc_server::plugins::Runtime;

const STATES: [&str; 2] = ["minecraft:air", "minecraft:stone"];

struct World {
    edits: BlockEdits,
}

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
        self.0.lock().expect("sink").extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A plugin whose only job is to report, from a thread that is not this one,
/// whether the tick lock is held while its callback runs.
struct Prober(Arc<Mutex<Vec<bool>>>);

impl Plugin for Prober {
    fn manifest(&self) -> Manifest {
        use grit_plugin_api::capability::well_known as cap;
        Manifest::new(
            PluginId::parse("grit:prober").expect("literal"),
            API_VERSION,
        )
        .requires(CapabilitySet::from_strs([cap::EVENT_BLOCK_BREAK]).expect("literal"))
    }
    fn enable(&mut self, setup: &mut Setup<'_>) -> Result<(), PluginError> {
        let seen = Arc::clone(&self.0);
        setup.events().on::<BlockBreak, _>(
            Priority::Normal,
            move |_ev: &BlockBreak, _v: &mut Verdict, _c: &mut Ctx<'_>| {
                // Another thread, because asking a `std::sync::Mutex` whether
                // *you* hold it is unspecified behaviour rather than an
                // observation.
                let held = std::thread::spawn(rustmc_server::broadcast::tick_lock_is_held)
                    .join()
                    .expect("prober thread");
                seen.lock().expect("seen").push(held);
            },
        )?;
        Ok(())
    }
}

/// One `player_action` frame: creative destroy at `pos`.
fn dig(pos: BlockPos) -> Vec<u8> {
    let mut body = Vec::new();
    rustmc_proto::varint::write_varint(&mut body, 0).expect("buf");
    let packed = (i64::from(pos.0) & 0x3FF_FFFF) << 38
        | (i64::from(pos.2) & 0x3FF_FFFF) << 12
        | (i64::from(pos.1) & 0xFFF);
    rustmc_proto::wire::write_i64(&mut body, packed).expect("buf");
    rustmc_proto::wire::write_u8(&mut body, 1).expect("buf");
    rustmc_proto::varint::write_varint(&mut body, 1).expect("buf");
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
fn a_plugin_callback_runs_with_the_tick_lock_held() {
    // The control, and it is what makes the assertion below mean something:
    // in this process, at this moment, nothing holds the tick lock. If this
    // fails, the test after it could pass for a reason that has nothing to do
    // with the handler — which is exactly how it failed before.
    assert!(
        !rustmc_server::broadcast::tick_lock_is_held(),
        "nothing else in this binary may hold the tick lock — do not add a second test here"
    );

    let world = World {
        edits: BlockEdits::new(),
    };
    world.edits.set((2, 2, 2), "minecraft:stone");

    let seen = Arc::new(Mutex::new(Vec::new()));
    let rt = Runtime::install(vec![Box::new(Prober(Arc::clone(&seen)))]);
    assert_eq!(rt.break_listeners(), 1, "the prober is listening");

    let players = rustmc_server::players::Players::new();
    let conn = Arc::new(rustmc_server::players::Connection::with_entity(
        players.next_id(),
        "Tester",
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
    // Beside the dug block: `handleBlockBreakAction` drops a dig outside
    // `isWithinBlockInteractionRange` before any plugin is asked.
    let mut player = rustmc_server::player::PlayerState::at(3.5, 2.0, 3.5);
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
        plugins: Some(&rt),
        creative: true,
        // Not under test here — this fixture sends one dig packet. Written
        // out because `Session` has no `Default`; see `world_send`.
        seed: 0,
    };
    let mut cursor = std::io::Cursor::new(dig((2, 2, 2)));
    rustmc_server::world_send::keep_alive_loop(&mut cursor, &mut session).expect("ends on EOF");

    assert_eq!(
        *seen.lock().expect("seen"),
        vec![true],
        "the callback ran with the tick lock held, so no other callback could \
         have been running at the same instant"
    );
    assert!(
        !rustmc_server::broadcast::tick_lock_is_held(),
        "and the handler released it"
    );
}
