//! **A fall, the health it costs, and the `set_health` that follows — through
//! the real play loop.**
//!
//! # What this witnesses that no module test can
//!
//! Five ported mechanisms have to meet for one packet to leave this server,
//! and every one of them has its own unit tests that pass without the others:
//!
//! ```text
//!   crate::playermove   handleMovePlayer -> Entity.move -> checkFallDamage
//!   crate::fall         fallDistance, calculateFallPower/Damage
//!   crate::hurt         hurtServer's cooldown, the NaN substitution
//!   crate::health       setHealth's clamp
//!   crate::playersync   doTick's lastSentHealth change detector
//! ```
//!
//! `origin/player-systems` ported the last three and wired none of them,
//! recording that "the server never knows the player fell";
//! `origin/player-movement` ported the first two and recorded that the fall
//! damage it computes "is not applied". This file is the assertion that the
//! two halves are now one thing, and it is a *wire* assertion rather than a
//! state one because a health change that never reaches the socket looks
//! exactly like a server that models no damage — which is what this server
//! was until this branch.
//!
//! # The clock is driven, and that is not a convenience
//!
//! `keep_alive_loop`'s tick catch-up reads `GameClock::ticks()`, and a test
//! feeding it a `Cursor` would see zero ticks elapse and therefore never call
//! `doTick` at all — the test would pass against a server that computes the
//! damage and never sends anything, which is precisely the state this file
//! exists to distinguish from. [`Ticking`] advances the clock exactly once
//! per frame, which is what a client sending twenty packets a second does.
//!
//! # Why the trailing packet
//!
//! `doTick` runs at the top of the loop, before the read. The landing is
//! applied while handling packet *n*, so the `set_health` it causes goes out
//! on packet *n+1*'s tick. That is vanilla's own one-tick lag — `hurtServer`
//! writes the health field and `ServerPlayer.doTick` decides whether the
//! client is told — and the trailing zero-delta move is what lets this test
//! observe it rather than assert it away.

use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use rustmc_proto::varint::{read_varint, write_varint};
use rustmc_proto::{frame, ids, wire};
use rustmc_server::edits::Blocks;
use rustmc_server::interact::{BlockPos, Face};
use rustmc_server::tick::GameClock;

/// Where the player starts: standing on the plane of stone this test lays.
const SPAWN: (f64, f64, f64) = (8.0, 64.0, 8.0);

/// How far up the player claims before dropping back.
///
/// Five blocks against a `safe_fall_distance` of 3.0 is **two** damage, and
/// the number is deliberately not the smallest one that hurts: four blocks is
/// one damage, which a server that dealt a flat one-heart fall would also
/// produce. Two damage from five blocks and one from four is the pair that
/// separates the mechanism from a constant, and both are asserted.
const RISE: f64 = 5.0;

/// The state ids come from `data/vanilla/block_props.json` — see
/// `tests/move_correction.rs` for why a made-up id is not harmless here.
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
                ids.insert(state.to_string(), id.try_into().expect("state id fits"));
            }
        }
        Self { ids }
    }
}

impl Blocks for Flat {
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

/// A reader that advances the game clock once per frame it hands out.
///
/// It never returns bytes past the next frame boundary, so `read_frame`'s own
/// several small reads all fall inside one frame and the clock advances
/// exactly once for each.
struct Ticking {
    bytes: Vec<u8>,
    pos: usize,
    /// Byte offsets where a frame starts, in order, excluding zero.
    boundaries: std::collections::VecDeque<usize>,
    clock: GameClock,
}

impl Ticking {
    fn new(frames: &[Vec<u8>], clock: GameClock) -> Self {
        let mut bytes = Vec::new();
        let mut boundaries = std::collections::VecDeque::new();
        for f in frames {
            if !bytes.is_empty() {
                boundaries.push_back(bytes.len());
            }
            bytes.extend_from_slice(f);
        }
        // The first frame gets a tick too, so the join `set_health` is sent
        // before any move is handled — which is what a real connection does.
        clock.advance();
        Self {
            bytes,
            pos: 0,
            boundaries,
            clock,
        }
    }
}

impl Read for Ticking {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.pos >= self.bytes.len() {
            return Ok(0);
        }
        if self.boundaries.front() == Some(&self.pos) {
            self.boundaries.pop_front();
            self.clock.advance();
        }
        let stop = self
            .boundaries
            .front()
            .copied()
            .unwrap_or(self.bytes.len())
            .min(self.pos + buf.len());
        let n = stop - self.pos;
        buf[..n].copy_from_slice(&self.bytes[self.pos..stop]);
        self.pos = stop;
        Ok(n)
    }
}

/// One `move_player_pos` claiming an absolute position.
fn claim(x: f64, y: f64, z: f64, on_ground: bool) -> Vec<u8> {
    let mut body = Vec::new();
    wire::write_f64(&mut body, x).expect("buf");
    wire::write_f64(&mut body, y).expect("buf");
    wire::write_f64(&mut body, z).expect("buf");
    // Flags, not a bool: bit 0 is `FLAG_ON_GROUND`.
    wire::write_u8(&mut body, u8::from(on_ground)).expect("buf");
    let mut out = Vec::new();
    frame::write_frame(&mut out, None, ids::play::sb::MOVE_PLAYER_POS, &body).expect("buf");
    out
}

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

/// Every `set_health` frame, decoded as `(health, food, saturation)`.
fn healths(rec: &Recorder) -> Vec<(f32, i32, f32)> {
    let mut out = Vec::new();
    for (id, body) in rec.frames() {
        if id != ids::play::cb::SET_HEALTH {
            continue;
        }
        let mut cur = &body[..];
        let health = wire::read_f32(&mut cur).expect("health");
        let food = read_varint(&mut cur).expect("food");
        let saturation = wire::read_f32(&mut cur).expect("saturation");
        assert!(cur.is_empty(), "set_health had trailing bytes: {body:?}");
        out.push((health, food, saturation));
    }
    out
}

/// Every `set_experience` frame, as `(progress, level, total)` — the WIRE
/// order, which is not the constructor's.
fn experiences(rec: &Recorder) -> Vec<(f32, i32, i32)> {
    let mut out = Vec::new();
    for (id, body) in rec.frames() {
        if id != ids::play::cb::SET_EXPERIENCE {
            continue;
        }
        let mut cur = &body[..];
        let progress = wire::read_f32(&mut cur).expect("progress");
        let level = read_varint(&mut cur).expect("level");
        let total = read_varint(&mut cur).expect("total");
        assert!(
            cur.is_empty(),
            "set_experience had trailing bytes: {body:?}"
        );
        out.push((progress, level, total));
    }
    out
}

/// Drive a frame stream through the real loop in **survival** and report the
/// packets and where the player ended up.
fn run(frames: &[Vec<u8>]) -> (Recorder, (f64, f64, f64)) {
    let world = Flat::new();
    let rec = Recorder::default();
    let players = rustmc_server::players::Players::new();
    let conn = Arc::new(rustmc_server::players::Connection::new(
        players.next_id(),
        "Faller",
        Box::new(rec.clone()),
        None,
    ));
    players.join(&conn);

    let clock = GameClock::new();
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
        // **Survival, and this is the whole substrate of the test.**
        // `Player.hurtServer` offsets 15..32 read `abilities.invulnerable`
        // and return `false` when it is set, and `GameType.CREATIVE` is what
        // sets it. With `creative: true` every assertion below would measure
        // an early return.
        creative: false,
        seed: 0,
        placement: None,
        placed_by: None,
        shapes: None,
        sound_types: None,
        sound_events: None,
        ticks: None,
        recipes: None,
    };

    let mut reader = Ticking::new(frames, clock.clone());
    rustmc_server::world_send::keep_alive_loop(&mut reader, &mut session).expect("ends on EOF");
    let at = (player.x, player.y, player.z);
    (rec, at)
}

/// The frame stream for a rise of `rise` blocks and a drop back to the floor.
fn fall_of(rise: f64) -> Vec<Vec<u8>> {
    vec![
        ack(1),
        // Up. `distanceSq` is `rise * rise` against `handleMovePlayer`'s
        // `100.0f * i` budget, so anything under ten blocks is accepted.
        claim(SPAWN.0, SPAWN.1 + rise, SPAWN.2, false),
        // Down, landing. `Entity.checkFallDamage` accumulates
        // `fallDistance -= (double)(float)dy` and then, because the packet
        // says on-ground, calls `Block.fallOn`.
        claim(SPAWN.0, SPAWN.1, SPAWN.2, true),
        // The trailing tick — see the module header.
        claim(SPAWN.0, SPAWN.1, SPAWN.2, true),
    ]
}

#[test]
fn a_five_block_fall_costs_two_health_and_the_client_is_told() {
    let (rec, at) = run(&fall_of(RISE));
    assert!(
        (at.1 - SPAWN.1).abs() < 1.0e-9,
        "the player should be back on the floor, got {at:?}"
    );

    let healths = healths(&rec);
    // The join packet: `ServerPlayer.<init>`'s four sentinels make the first
    // `doTick` send unconditionally, so this is not the fall.
    assert!(
        !healths.is_empty(),
        "no set_health at all — the tick never ran"
    );
    assert!(
        (healths[0].0 - 20.0).abs() < f32::EPSILON,
        "the join packet should carry full health, got {:?}",
        healths[0]
    );
    assert_eq!(healths[0].1, 20, "and a full food bar");

    let after = *healths.last().expect("at least one");
    assert!(
        (after.0 - 18.0).abs() < f32::EPSILON,
        "five blocks is two damage: expected 18.0, got {after:?} from {healths:?}"
    );
    assert_eq!(
        healths.len(),
        2,
        "exactly two: the join and the fall, not one per tick — {healths:?}"
    );
}

#[test]
fn the_damage_tracks_the_distance_rather_than_being_a_constant() {
    // The pair that separates the mechanism from a flat one-heart fall.
    for (rise, want) in [(4.0_f64, 19.0_f32), (5.0, 18.0), (10.0, 13.0)] {
        let (rec, _) = run(&fall_of(rise));
        let after = *healths(&rec).last().expect("a set_health");
        assert!(
            (after.0 - want).abs() < f32::EPSILON,
            "a {rise}-block fall should leave {want} health, got {after:?}"
        );
    }

    // The negative control, and it is the one that would catch a server
    // dealing damage for every landing: three blocks is the safe distance
    // and costs nothing, so the ONLY set_health is the join packet.
    let (rec, _) = run(&fall_of(3.0));
    let healths = healths(&rec);
    assert_eq!(
        healths.len(),
        1,
        "a three-block fall is free and must send nothing after the join — {healths:?}"
    );
    assert!((healths[0].0 - 20.0).abs() < f32::EPSILON);
}

#[test]
fn set_experience_goes_out_once_on_the_join_and_never_again() {
    // `doTick` offsets 487..532 compare `totalExperience` against
    // `lastSentExp`, whose sentinel is -99999999. Since `train-g` the
    // comparison runs on the clock thread (`crate::experience::
    // connection_tick`, the one store `/experience`, the orbs and the
    // enchanting table write), so the play loop this rig drives sends none,
    // and the store's own detector sends a fresh bar once and then nothing.
    let (rec, _) = run(&fall_of(RISE));
    let xp = experiences(&rec);
    assert!(
        xp.is_empty(),
        "the play loop sends no set_experience — {xp:?}"
    );
    let mut x = rustmc_server::experience::Experience::new();
    assert_eq!(x.sync(), Some((0.0, 0, 0)), "a fresh player's bar, once");
    assert_eq!(x.sync(), None, "and never again while nothing moves");
}

#[test]
fn a_creative_player_falls_the_same_distance_and_is_never_hurt() {
    // The substrate control for the whole file. Same packets, same physics,
    // and `abilities.invulnerable` swallows the hurt at `hurtServer` offset
    // 31 — which is exactly why `ci/checks/gameplay`'s creative corpus has
    // never recorded a `set_health` and why `ci/t2-gameplay-survival.sh`
    // exists.
    let frames = fall_of(RISE);
    let world = Flat::new();
    let rec = Recorder::default();
    let players = rustmc_server::players::Players::new();
    let conn = Arc::new(rustmc_server::players::Connection::new(
        players.next_id(),
        "Creative",
        Box::new(rec.clone()),
        None,
    ));
    players.join(&conn);
    let clock = GameClock::new();
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
    let mut reader = Ticking::new(&frames, clock.clone());
    rustmc_server::world_send::keep_alive_loop(&mut reader, &mut session).expect("ends on EOF");

    let healths = healths(&rec);
    assert_eq!(
        healths.len(),
        1,
        "creative sends only the join packet — {healths:?}"
    );
    assert!((healths[0].0 - 20.0).abs() < f32::EPSILON);
    assert!(
        (player.y - SPAWN.1).abs() < 1.0e-9,
        "and the physics still ran: the player is back on the floor"
    );
}
