//! **The correction teleport, on the wire, through the real play loop.**
//!
//! # What this witnesses that no module test can
//!
//! `crate::move_player` decides whether a move is refused and
//! `crate::playermove` runs the collision, and both have module tests. What
//! neither can see is the **joint**: that `keep_alive_loop` builds a
//! `PlayerMovement`, adopts the join teleport's id, drives `tick()` off the
//! game clock, calls `on_move` before `player::apply` rather than after, and
//! turns a `Corrected` outcome into a `ClientboundPlayerPositionPacket` on
//! this connection.
//!
//! That is the same shape as `session_joint.rs`: six mechanisms were ported
//! and gated, and nothing anywhere asked whether setting the field did
//! anything. Here the risk is sharper, because a refusal that never reaches
//! the socket looks exactly like a server that accepted the move.
//!
//! # Why the pair, and not just the refusal
//!
//! A single refused packet shows that *a* correction exists and says nothing
//! about the budget. This corpus has fourteen recorded cases of a check that
//! pinned one value and so could not tell a mechanism from a constant. The
//! two claims here differ only in distance — twenty blocks and five — so
//! together they measure the threshold rather than the existence of the code.
//!
//! ```text
//!   handleMovePlayer, offsets 460..489
//!     460: f2 = isFallFlying ? 300.0f : 100.0f
//!     476: if (distanceSq - deltaMovementLengthSq > (double)(f2 * (float)i))
//!     577:     teleport(getX(), getY(), getZ(), f, f1)
//! ```
//!
//! Twenty blocks is `distanceSq = 400` against a budget of `100.0f * 1`;
//! five is `25` against the same budget.
//!
//! # The teleport id is an assertion, not a detail
//!
//! `trace.rs`'s `PLAYER_POSITION` arm exposes `teleport` as a **compared
//! field** of the gameplay corpus, so getting the counter wrong diverges
//! every correction check on the counter rather than on the correction.
//! Vanilla's join teleport goes through the same `teleport(
//! PositionMoveRotation, Set)` this server models, so its counter is already
//! 1 when the first move arrives and the first correction is **2**.

use std::io::Write;
use std::sync::{Arc, Mutex};

use rustmc_proto::varint::{read_varint, write_varint};
use rustmc_proto::{frame, ids, wire};
use rustmc_server::edits::Blocks;
use rustmc_server::interact::{BlockPos, Face};

/// Where the player starts. A whole number of chunks from the origin so the
/// walk does not stray out of the floor this test lays.
const SPAWN: (f64, f64, f64) = (8.0, 64.0, 8.0);

/// The state ids come from `data/vanilla/block_props.json`.
///
/// **Not a constant and not a counter.** The id is the key into
/// `collision_shapes.json`, so a made-up one silently gives the floor
/// whichever shape happens to hold that number — which for a test about
/// collision is the difference between standing on stone and standing in it.
struct Flat {
    ids: std::collections::HashMap<String, i32>,
}

fn repo(relative: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

impl Flat {
    fn new() -> Self {
        let doc: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(repo("data/vanilla/block_props.json"))
                .expect("data/vanilla/block_props.json — run ci/gen-gameplay-tables.sh"),
        )
        .expect("block_props.json parses");
        let mut ids = std::collections::HashMap::new();
        for row in doc["table"].as_array().expect("table") {
            if let (Some(state), Some(id)) = (row["state"].as_str(), row["id"].as_i64()) {
                ids.insert(state.to_string(), id as i32);
            }
        }
        Self { ids }
    }
}

impl Blocks for Flat {
    /// Stone below `y = 64`, air at and above it. A whole plane, so nothing
    /// in this test is about the edge of a rig.
    fn state_at(&self, pos: BlockPos) -> String {
        if pos.1 < 64 {
            "minecraft:stone".to_string()
        } else {
            "minecraft:air".to_string()
        }
    }
    fn set_block(&self, _: BlockPos, _: &str) -> Option<i32> {
        None
    }
    fn default_state(&self, block: &str) -> Option<String> {
        Some(block.to_string())
    }
    fn has_state(&self, state: &str) -> bool {
        self.ids.contains_key(state)
    }
    fn state_id(&self, state: &str) -> Option<i32> {
        self.ids.get(state).copied()
    }
    fn replaceable(&self, _: BlockPos, _: &rustmc_server::placement::Replacing<'_>) -> bool {
        true
    }
    fn face_sturdy(&self, pos: BlockPos, _: Face) -> bool {
        pos.1 < 64
    }
    fn redstone_conductor(&self, _: BlockPos) -> bool {
        false
    }
    fn signal_source(&self, _: BlockPos) -> bool {
        false
    }
    fn legacy_fluid_at(&self, _: BlockPos) -> String {
        "minecraft:air".to_string()
    }
    fn water_at(&self, _: BlockPos) -> bool {
        false
    }
}

#[derive(Clone, Default)]
struct Recorder(Arc<Mutex<Vec<u8>>>);
impl Write for Recorder {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("recorder").extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl Recorder {
    fn frames(&self) -> Vec<(i32, Vec<u8>)> {
        let bytes = self.0.lock().expect("recorder").clone();
        let mut cur = &bytes[..];
        let mut out = Vec::new();
        while let Ok(f) = frame::read_frame(&mut cur, false) {
            out.push(f);
        }
        out
    }
}

/// One `move_player_pos` claiming an absolute position, on the ground.
fn claim(x: f64, y: f64, z: f64) -> Vec<u8> {
    let mut body = Vec::new();
    wire::write_f64(&mut body, x).expect("buf");
    wire::write_f64(&mut body, y).expect("buf");
    wire::write_f64(&mut body, z).expect("buf");
    // Flags, not a bool: bit 0 is `FLAG_ON_GROUND`.
    wire::write_u8(&mut body, 1).expect("buf");
    let mut out = Vec::new();
    frame::write_frame(&mut out, None, ids::play::sb::MOVE_PLAYER_POS, &body).expect("buf");
    out
}

/// `accept_teleportation` for a teleport id.
fn ack(id: i32) -> Vec<u8> {
    let mut body = Vec::new();
    write_varint(&mut body, id).expect("buf");
    let mut out = Vec::new();
    frame::write_frame(
        &mut out,
        None,
        rustmc_server::player::ids::ACCEPT_TELEPORTATION,
        &body,
    )
    .expect("buf");
    out
}

/// Every `player_position` frame the actor received, as `(teleport id, at)`.
fn corrections(rec: &Recorder) -> Vec<(i32, (f64, f64, f64))> {
    let mut out = Vec::new();
    for (id, body) in rec.frames() {
        if id != ids::play::cb::PLAYER_POSITION {
            continue;
        }
        let mut cur = &body[..];
        let tp = read_varint(&mut cur).expect("teleport id");
        let x = wire::read_f64(&mut cur).expect("x");
        let y = wire::read_f64(&mut cur).expect("y");
        let z = wire::read_f64(&mut cur).expect("z");
        out.push((tp, (x, y, z)));
    }
    out
}

/// A position on the wire, or the one the player ended at.
type At = (f64, f64, f64);

/// One `player_position` frame: the teleport id and where it sent the player.
type Correction = (i32, At);

/// Drive a packet stream through the real loop and report the corrections and
/// where the player ended up.
fn run(input: Vec<u8>) -> (Vec<Correction>, At) {
    let world = Flat::new();
    let rec = Recorder::default();
    let players = rustmc_server::players::Players::new();
    let conn = Arc::new(rustmc_server::players::Connection::new(
        players.next_id(),
        "Actor",
        Box::new(rec.clone()),
        None,
    ));
    players.join(&conn);

    let clock = rustmc_server::tick::GameClock::new();
    let changed = rustmc_server::broadcast::ChangedBlocks::new();
    let block_events = rustmc_server::containers::BlockEvents::new();
    let openers = rustmc_server::containers::Openers::new();
    let waypoints = rustmc_server::waypoints::Waypoints::new();
    let mut player = rustmc_server::player::PlayerState::at(SPAWN.0, SPAWN.1, SPAWN.2);
    let mut hotbar = rustmc_server::player::Hotbar::new();
    let mut menu = rustmc_server::menu::MenuState::new();
    let mut known_recipes = std::collections::BTreeSet::new();
    let chunk_at = |_: i32, _: i32| Ok(None);

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
        known_recipes: &mut known_recipes,
        advancements: None,
        adv_engine: None,
        recipe_unlocks: None,
        uses: None,
        item_uses: None,
        survive: None,
        bonemeal: None,
        fire_odds: None,
        protocol_ids: None,
        waypoints: &waypoints,
        changed: &changed,
        block_events: &block_events,
        openers: &openers,
        chunk_at: &chunk_at,
        plugins: None,
        // Creative, like the gameplay corpus. It exempts the "moved wrongly"
        // geometry check at offsets 804-821 and does NOT exempt the speed
        // check — `shouldCheckPlayerMovement` never asks — which is the whole
        // reason this test is about the speed check.
        creative: true,
        seed: 0,
        placement: None,
        placed_by: None,
        shapes: None,
        sound_types: None,
        sound_events: None,
        ticks: None,
        recipes: None,
    };

    let mut cursor = std::io::Cursor::new(input);
    rustmc_server::world_send::keep_alive_loop(&mut cursor, &mut session).expect("ends on EOF");
    let at = (player.x, player.y, player.z);
    (corrections(&rec), at)
}

/// Twenty blocks in one packet is refused; five is not. The pair pins the
/// budget rather than the existence of a correction.
#[test]
fn a_move_past_the_speed_budget_is_teleported_back_and_a_short_one_is_not() {
    // The client acknowledges the join teleport (id 1, sent by `enter_world`
    // before this loop), then claims twenty blocks.
    let mut input = ack(1);
    input.extend(claim(SPAWN.0, SPAWN.1, SPAWN.2 + 20.0));
    let (corrections, at) = run(input);
    assert_eq!(
        corrections.len(),
        1,
        "exactly one correction, got {corrections:?}"
    );
    let (tp, to) = corrections[0];
    assert_eq!(tp, 2, "the join teleport was 1, so the correction is 2");
    assert!(
        (to.0 - SPAWN.0).abs() < 1.0e-9
            && (to.1 - SPAWN.1).abs() < 1.0e-9
            && (to.2 - SPAWN.2).abs() < 1.0e-9,
        "the correction sends the player where the SERVER had them, got {to:?}"
    );
    assert!(
        (at.2 - SPAWN.2).abs() < 1.0e-9,
        "and the player entity did not take the claimed position, got {at:?}"
    );

    // The control: five blocks from the same anchor, nothing sent back.
    let mut input = ack(1);
    input.extend(claim(SPAWN.0, SPAWN.1, SPAWN.2 + 5.0));
    let (corrections, at) = run(input);
    assert!(
        corrections.is_empty(),
        "five blocks is inside the budget, got {corrections:?}"
    );
    assert!(
        (at.2 - (SPAWN.2 + 5.0)).abs() < 1.0e-9,
        "and the player moved there, got {at:?}"
    );
}

/// Until the client acknowledges the join teleport, every move is swallowed.
///
/// `updateAwaitingTeleport` at offsets 123-140 returns before the position is
/// even clamped, and the same branch is what makes a correction stick. A
/// server that skipped it would let a refused client keep walking.
#[test]
fn a_move_before_the_join_teleport_is_acknowledged_moves_nothing() {
    let (corrections, at) = run(claim(SPAWN.0, SPAWN.1, SPAWN.2 + 5.0));
    assert!(corrections.is_empty(), "nothing is sent back");
    assert!(
        (at.2 - SPAWN.2).abs() < 1.0e-9,
        "the move is swallowed, got {at:?}"
    );

    // The control is the test above: the identical packet after an `ack(1)`
    // moves the player five blocks.
}
