//! The status handshake — what a client sees in its multiplayer list.
//!
//! Reference (26.2): the `handshake` and `status` protocol states, with ids
//! from [`rustmc_proto::ids`], which are transcribed from Mojang's own
//! packet report rather than from memory.
//!
//! This is the first thing `rustmc run` does that a real client can observe,
//! and it is deliberately the *whole* of what this module does. A server list
//! entry is four packets:
//!
//! ```text
//!   C->S  handshake/intention   protocol, host, port, next state
//!   C->S  status/status_request  (empty)
//!   S->C  status/status_response JSON
//!   C->S  status/ping_request    i64
//!   S->C  status/pong_response   the same i64, echoed
//! ```
//!
//! # Why the ping is echoed and not generated
//!
//! The client measures its own latency by sending a number and timing how
//! long the same number takes to come back. Anything else — a timestamp of
//! ours, a counter — produces a plausible-looking connection with a nonsense
//! ping, which is the kind of wrong that shows up in a screenshot rather than
//! in a test.
//!
//! # The version block decides whether the client will even offer to connect
//!
//! `version.protocol` must match the client's exactly or the entry greys out
//! with "incompatible". It is not cosmetic, and it is the reason
//! [`rustmc_proto::GAME_VERSION`] is a single source of truth: a version bump
//! that misses this makes every client refuse a server that is otherwise
//! completely correct.

use std::io::{self, Read, Write};

use rustmc_proto::{frame, ids, varint::read_varint, wire};

/// The protocol number for [`rustmc_proto::GAME_VERSION`].
///
/// Read from the jar, not remembered:
///
/// ```text
/// javap -p -c -constants -cp server-26.2-unsigned.jar net.minecraft.SharedConstants
///   public static final int RELEASE_NETWORK_PROTOCOL_VERSION = 776;
/// ```
///
/// I first wrote 775 from memory and the tests passed, because they check our
/// encoder against our decoder and neither has an opinion about the number. A
/// real client does: one digit out and the server list entry greys to
/// "incompatible" without ever opening a connection. It took reading
/// `SharedConstants` to catch, which is the whole argument for this project's
/// rule about never guessing a constant.
pub const PROTOCOL_VERSION: i32 = 776;

/// `server.properties: max-players`, and `PlayerList.getMaxPlayers`.
///
/// **One number with two destinations**, which is why it is a constant rather
/// than a literal in each: it reaches a client in the status ping's JSON and
/// again as the second translation argument of `/list`'s reply
/// (`crate::commands`). A server that advertised 20 and answered `/list` with
/// something else would be wrong in a way only one of the two would show.
pub const MAX_PLAYERS: i32 = 20;

/// What to tell a client about this server.
#[derive(Debug, Clone)]
pub struct StatusInfo {
    pub description: String,
    pub max_players: i32,
    pub online_players: i32,
    /// `server.properties: view-distance`, in chunks.
    ///
    /// One number with three jobs, and this server used to have a different
    /// answer for each of them. Vanilla puts it in `ChunkMap.serverViewDistance`
    /// and reads it from `ClientboundLoginPacket`'s chunk radius, from the
    /// chunk sender, and from `ChunkMap.getPlayerViewDistance`, which is what
    /// bounds *entity tracking* — so a server that has no such setting cannot
    /// track entities at vanilla's distance by accident.
    pub view_distance: i32,
    /// `server.properties: simulation-distance`, in chunks.
    ///
    /// **A different number with a different job, and it was a literal 10 in
    /// the join packet until 2026-09-05.** It reaches the client in
    /// `ClientboundLoginPacket` and it sets the level of a player's
    /// `PLAYER_SIMULATION` ticket — `max(0, 31 - simulation-distance)`,
    /// [`crate::tickets::player_ticket_level`] — which is what decides how
    /// far mobs move, block entities tick and random ticks fire. Vanilla's
    /// `PlayerList` holds the two settings in separate fields and pushes each
    /// to `ServerChunkCache` separately; conflating them makes a server
    /// simulate exactly as far as it renders, which vanilla's defaults do
    /// not.
    pub simulation_distance: i32,
    /// `server.properties: entity-broadcast-range-percentage`.
    ///
    /// `MinecraftServer.getScaledTrackingDistance` is the identity and
    /// `DedicatedServer`'s override is `pct * range / 100`, so this is the
    /// only input that can move an entity's tracking range away from its
    /// type's. It reaches nothing else: it is not sent to a client and it
    /// does not touch chunk streaming. See [`crate::tracker::scaled_range`].
    pub entity_broadcast_range_percentage: i32,
}

/// `server.properties`' own default for `view-distance`.
///
/// **Ten, and the tempting number here is the wrong one.** This server's
/// tiles are `WORLD_RADIUS` chunks in each direction, and until 2026-08-03
/// the join packet carried that radius and the entity tracker used it as a
/// view distance — so how much world had been *generated* silently decided
/// how far a player could see another player. The two are unrelated
/// in vanilla: `view-distance` is an operator setting and the world is
/// infinite. Anything measuring against a vanilla server has to set both, and
/// `ci/t2-gameplay.sh` sets this one to 4 to match `ci/vanilla-server.sh`.
///
/// **`WORLD_RADIUS` is also 10 as of 2026-08-06 and that is a coincidence of
/// measurement, not an identity.** The sweep in its doc comment chose 10
/// because a tile that contains the shipped view means a standing player
/// forces no background generation; if either number moves the other does not
/// follow. They remain separate constants and separate flags — `--radius` and
/// `--view-distance` — and the bug this paragraph exists to prevent is
/// reintroducing one in terms of the other because they currently match.
pub const DEFAULT_VIEW_DISTANCE: i32 = 10;

/// `server.properties`' own default for `simulation-distance`, and
/// `DistanceManager.simulationDistance`'s initial value in the bytecode.
///
/// Equal to [`DEFAULT_VIEW_DISTANCE`] and not derived from it — see that
/// constant's note about `WORLD_RADIUS`, which is the same coincidence a
/// third time.
pub const DEFAULT_SIMULATION_DISTANCE: i32 = 10;

impl Default for StatusInfo {
    fn default() -> Self {
        Self {
            description: format!("rustmc {}", rustmc_proto::GAME_VERSION),
            max_players: MAX_PLAYERS,
            online_players: 0,
            view_distance: DEFAULT_VIEW_DISTANCE,
            simulation_distance: DEFAULT_SIMULATION_DISTANCE,
            entity_broadcast_range_percentage: crate::tracker::DEFAULT_BROADCAST_RANGE_PERCENTAGE,
        }
    }
}

/// `ChunkMap.getPlayerViewDistance` — how far *this* player tracks.
///
/// Both bounds matter and neither is decorative. The floor of two is why a
/// client asking for one still gets entities; the ceiling is why a client
/// asking for thirty-two does not get them at 512 blocks, which is what a
/// server that simply believes the client would do — `tools/oracle-runner`
/// sends 10 in its `client_information` and vanilla, at `view-distance=4`,
/// tracks it at 64 blocks and not at 160.
///
/// One line, delegating to [`crate::tickets::player_view_distance`], which is
/// where the ticket arithmetic that reads the same clamp lives. Two copies of
/// a clamp is two places for the `min > max` case to be answered differently.
#[must_use]
pub const fn player_view_distance(requested: i32, server: i32) -> i32 {
    crate::tickets::player_view_distance(requested, server)
}

/// `LivingEntity.<init>`'s closing `setHealth(getMaxHealth())`, for a player.
///
/// `getMaxHealth()` is `(float) getAttributeValue(Attributes.MAX_HEALTH)` and
/// a fresh player carries no modifiers, so the value is the base out of
/// `AttributeSupplier` — which is the `minecraft:player` row of
/// `data/vanilla/attributes.json`, not a number written here.
///
/// The fallback is the **accessor's own definition**, `1.0f`, and it is
/// chosen for a reason: without the table this server cannot know a player's
/// health, and `1.0f` is the one value that makes
/// `SynchedEntityData.getNonDefaultValues` leave health out of the payload
/// altogether. A missing table then costs a field rather than putting an
/// invented one on the wire.
#[must_use]
fn joining_health() -> f32 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "getMaxHealth()'s own d2f — the accessor is a float"
    )]
    crate::attributes::Attributes::shared()
        .and_then(|a| a.base("minecraft:player", crate::attributes::MAX_HEALTH))
        .map_or(1.0, |v| v as f32)
}

impl StatusInfo {
    /// The status JSON.
    ///
    /// Built by hand rather than through a serialiser: it is a fixed shape of
    /// four fields, and the escaping that matters is the description's, which
    /// is the only part a caller controls.
    #[must_use]
    pub fn to_json(&self) -> String {
        format!(
            r#"{{"version":{{"name":"{}","protocol":{}}},"players":{{"max":{},"online":{}}},"description":{{"text":"{}"}}}}"#,
            rustmc_proto::GAME_VERSION,
            PROTOCOL_VERSION,
            self.max_players,
            self.online_players,
            escape_json(&self.description),
        )
    }
}

/// The subset of JSON string escaping this needs.
///
/// Control characters are escaped as `\u00XX` rather than dropped, because a
/// description read from a config file is caller data and a raw control byte
/// makes the whole payload unparseable — the client's failure for that is a
/// blank server entry with no error, which is hard to diagnose from the far
/// side.
fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// What the handshake asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    Status,
    Login,
    Transfer,
    /// Anything else the client asked for; carried rather than rejected so a
    /// caller can log the actual number.
    Unknown(i32),
}

impl Intent {
    const fn from_wire(v: i32) -> Self {
        match v {
            1 => Self::Status,
            2 => Self::Login,
            3 => Self::Transfer,
            other => Self::Unknown(other),
        }
    }
}

/// A decoded `handshake/intention`.
#[derive(Debug, Clone)]
pub struct Handshake {
    pub protocol: i32,
    pub host: String,
    pub port: u16,
    pub intent: Intent,
}

/// Read the handshake a connection opens with.
///
/// # Errors
/// If the stream ends, or the first packet is not `intention`.
pub fn read_handshake(r: &mut impl Read) -> io::Result<Handshake> {
    let (id, body) = frame::read_frame(r, false)?;
    if id != ids::handshake::sb::INTENTION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "first packet was id {id:#04x}, expected intention {:#04x}",
                ids::handshake::sb::INTENTION
            ),
        ));
    }
    let mut cur = &body[..];
    let protocol = read_varint(&mut cur)?;
    let host = wire::read_string(&mut cur)?;
    let port = wire::read_u16(&mut cur)?;
    let intent = Intent::from_wire(read_varint(&mut cur)?);
    Ok(Handshake {
        protocol,
        host,
        port,
        intent,
    })
}

/// Serve the status exchange on a connection whose handshake said `Status`.
///
/// Returns once the client closes, which is what a client does after it has
/// its ping — the server list does not hold the socket open.
///
/// # Errors
/// On any I/O failure. A client vanishing mid-exchange is normal and surfaces
/// as `UnexpectedEof`; callers should treat that as success.
pub fn serve_status(stream: &mut (impl Read + Write), info: &StatusInfo) -> io::Result<()> {
    loop {
        let (id, body) = match frame::read_frame(stream, false) {
            Ok(v) => v,
            // The client hanging up is the normal end of a status exchange,
            // not a failure worth reporting up.
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(e),
        };
        match id {
            ids::status::sb::STATUS_REQUEST => {
                let mut out = Vec::new();
                wire::write_string(&mut out, &info.to_json())?;
                frame::write_frame(stream, None, ids::status::cb::STATUS_RESPONSE, &out)?;
            }
            ids::status::sb::PING_REQUEST => {
                // Echoed exactly. See the module docs.
                frame::write_frame(stream, None, ids::status::cb::PONG_RESPONSE, &body)?;
                return Ok(());
            }
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unexpected status packet {other:#04x}"),
                ));
            }
        }
    }
}

/// Accept connections on `addr` and answer status queries, forever.
///
/// Single-threaded and sequential on purpose. A status query is four small
/// packets and returns immediately; the concurrency this eventually needs
/// belongs with the play state, not here, and a thread pool added before
/// there is anything to be concurrent *about* is a thing to get wrong for no
/// gain.
///
/// A login attempt is answered with a disconnect carrying a reason, rather
/// than a dropped socket: a client that is silently hung up on reports
/// "connection reset", which says nothing about why.
///
/// # `on_bound` is the server's readiness, and it is called once
///
/// It runs **after** the bind has succeeded, after the tick loop is spawned
/// and immediately before the first `accept` — never on any path that returns
/// `Err`. It is handed `listener.local_addr()`, the address the kernel
/// actually gave us, not the one that was asked for; the two differ whenever
/// port 0 was requested, and asking the listener is the difference between
/// reporting a bind and reporting an intention.
///
/// This exists because the announcement used to be an `eprintln!` in
/// `main.rs` *before* this function was called. A bind that failed therefore
/// printed `rustmc … listening on 0.0.0.0:26783` and then
/// `listen: Address already in use`, in that order — and three harness wait
/// loops proved the server was up by grepping for the first line. A server
/// that never bound read as ready and the harness recorded a corpus against
/// somebody else's port. Nothing downstream could tell, because the log said
/// what it always says. Announcing from here means the claim cannot outrun
/// the fact: there is no ordering of these statements that prints it early.
/// `ci/checks/liveness-proofs.toml` and `ci/readiness-falsify.sh`.
///
/// # Errors
/// If the listener cannot bind, or cannot report the address it bound to.
/// Per-connection failures are returned through `on_error` and never stop the
/// loop — one malformed client must not take the server down.
pub fn listen(
    addr: impl std::net::ToSocketAddrs,
    info: &StatusInfo,
    on_bound: impl FnOnce(&std::net::SocketAddr),
    on_error: impl Fn(&std::net::SocketAddr, &io::Error) + Send + Sync + Clone + 'static,
) -> io::Result<()> {
    // The plugins, before the socket opens.
    //
    // `grit_plugin_api::load` runs here, on the real list, on every boot —
    // not behind a flag and not only in a test. That is what stops the loader
    // being a fourth definition nobody calls: with the list empty it enables
    // nothing and prints nothing, and the moment a plugin is added to
    // `plugins::first_party` it is enabled by this line and by no other.
    //
    // Refusals are printed and never fatal. One plugin declaring a capability
    // this server does not back must leave the other plugins working and the
    // server up — six plugins with one incompatibility is five working
    // plugins and one named error, not a dead server.
    let plugins = crate::plugins::install_global(crate::plugins::first_party());
    if !plugins.loaded().is_empty() {
        eprintln!(
            "plugins: {} loaded — {}",
            plugins.loaded().len(),
            plugins
                .loaded()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    for refusal in plugins.refused() {
        eprintln!("plugins: refused {refusal}");
    }
    let listener = std::net::TcpListener::bind(addr)?;
    // One connection at a time meant the second player to try was not
    // refused, which would at least be honest — they simply waited behind
    // the first player's entire session. A thread each is the smallest thing
    // that makes a second player possible at all; a real scheduler is a
    // separate piece of work and belongs with ticking.
    //
    // The generated canvas is still shared and immutable; what a player
    // changes goes into an overlay beside it (`crate::edits`), which is
    // internally locked. So the contention this line used to warn about is
    // real now and is handled where the data is, rather than by serialising
    // whole sessions.
    let players = std::sync::Arc::new(crate::players::Players::new());
    // The console has been running since `crate::main` and had nobody to
    // broadcast to until now. `MinecraftServer` has the two the other way
    // round — the `PlayerList` exists before `DedicatedServer.initServer`
    // starts the console reader — and this hands the registry over instead of
    // reordering `listen`'s construction, which every caller of `listen`
    // would have to follow.
    crate::console::set_players(&players);
    // One clock for the server, started before the first connection so game
    // time is the same number for everyone who joins.
    let clock = crate::tick::GameClock::new();
    // The console tags each `/damage` with the tick it was read in; see
    // `crate::damagecmd::Pending::at_tick`.
    crate::console::set_clock(&clock);
    // `/time` reads the game time from whichever thread runs it.
    crate::cmdexec::set_clock(&clock);
    // The tick loop, and it is a *game* loop now rather than a counter: every
    // tick it drains whatever `crate::ticks` says is due and broadcasts what
    // changed. It runs on the clock's own thread, so a slow tick eats the
    // tick budget — which is the property a tick loop should have and the
    // reason `GameClock::run` skips a slot it has already missed.
    //
    // The world is prepared before the socket opens, so this closure can hold
    // a `&'static` to it. A checkout with no extract has no world and the
    // loop still runs; there is simply nothing in the queue.
    {
        let world = crate::tiles::Tiles::prepare(WORLD_SEED, WORLD_RADIUS).ok();
        // Captured for the tile sweep at the bottom of the tick: it is what
        // the chunk sender streams at, and retiring on anything smaller would
        // retire a tile still going out. See `retire_unwatched_tiles`.
        let server_view_distance = info.view_distance;
        let shapes = crate::shapes::shared_with_classes(std::path::Path::new("."));
        // The block-state argument table (`data/vanilla/block_props.json`,
        // 28.5 MB), loaded here rather than by the first command that parses
        // a block — `/fill`, `/setblock`, `/clone`, any `block_state`
        // argument — because that command runs on the console's reader
        // thread and was paying the parse inside its own tick. Vanilla's
        // block registry exists before `DedicatedServer.initServer` opens
        // anything. Measured 2026-10-03: `129-console-fill-a-hollow-box` as
        // the FIRST console fill of a server answered at 564-793 ms against
        // vanilla's 400 (four `rgc` subset runs, `~/rustmc-ranged2/oracle-
        // work/gameplay/rounds/g1*`), and 400 when an earlier `setblock` had
        // warmed the table; at the head of shard 1 of the 8-shard ranged
        // gate, under load, it did not answer inside the 1.9 s window at
        // all and the check read `absent`.
        let _ = crate::blockarg::shared();
        // The class map, for the same reason the shape table is here: a
        // button's tick fires on this thread and has to ask the extract what
        // class the block is rather than guess from its name.
        let classes = crate::placedby::PlacedByTable::shared(std::path::Path::new(
            "data/vanilla/set_placed_by.json",
        ))
        .ok();
        // For `crate::movement`'s gravity step — a falling block today, and
        // whatever else eventually needs `Profile::of`'s other kinds.
        // Loaded once here rather than per tick for the same reason `shapes`
        // and `classes` are: both are read-only extracts and neither changes
        // while the server runs.
        let collision_shapes =
            crate::collide::CollisionShapes::load(std::path::Path::new(crate::collide::TABLE_PATH))
                .map_err(|e| {
                    eprintln!("no collision shape table ({e}) — falling blocks will not move")
                })
                .ok();
        let movement_props = crate::movement_props::MovementProps::load_default()
            .map_err(|e| eprintln!("no movement props table ({e}) — falling blocks will not move"))
            .ok();
        // Max stack sizes, for an item on the floor deciding whether it can
        // merge. The same process-wide copy every session's inventory reads.
        let tick_item_props =
            crate::itemprops::ItemProps::shared(std::path::Path::new(crate::itemprops::TABLE_PATH))
                .ok();

        // The two tables the block-entity pass needs, loaded once for the
        // clock thread rather than per connection: `SessionTables` gives each
        // session its own `Recipes`, and the tick loop is not a session.
        // `None` for either is "the furnace family does not tick", which is
        // what a checkout with no extract has always been — not a furnace
        // that smelts nothing while reporting that it has.
        let tick_recipes = crate::recipes::Recipes::load(
            std::path::Path::new("data/vanilla/recipes.json"),
            std::path::Path::new("data/vanilla/tags_item.json"),
        )
        .ok();
        let tick_fuels =
            crate::fuel::FuelValues::shared(std::path::Path::new("data/vanilla/fuel_values.json"))
                .ok();
        // The item registry, for the per-tick menu sweep's
        // `container_set_slot` — the stacks a furnace's ticker writes have to
        // reach the client as protocol ids, and the sessions' own copies are
        // per connection. `None` sends a stack as empty, which is what every
        // other `to_wire` caller does without a registry.
        let tick_items =
            crate::player::ItemIds::load(std::path::Path::new("data/vanilla/items.json")).ok();
        let tick_songs = crate::jukebox::JukeboxSongs::shared(std::path::Path::new(
            "data/vanilla/jukebox_songs.json",
        ))
        .ok();
        let ticking = std::sync::Arc::clone(&players);
        // `PistonBaseBlock.triggerEvent` plays its sound from inside the
        // block event, ahead of the event packet; the piston module has no
        // player list of its own, so it is handed one once.
        crate::pistonmove::install_sound_sink(Box::new({
            let players = std::sync::Arc::clone(&players);
            let ids = crate::protocol_ids::shared();
            let events = crate::sound_events::SoundEventTable::shared(std::path::Path::new(
                crate::sound_events::TABLE_PATH,
            ))
            .ok();
            move |sound| {
                crate::sounds::send(
                    &players,
                    ids,
                    events,
                    Some(crate::entities::Entities::shared()),
                    std::slice::from_ref(sound),
                );
            }
        }));
        let queue = crate::ticks::ScheduledTicks::shared();
        let changed = crate::broadcast::ChangedBlocks::shared();
        let entities = crate::entities::Entities::shared();
        let tracker = crate::tracker::Tracker::shared();
        // `DedicatedServer.setEntityBroadcastRangePercentage`, before any
        // player can join: `getEffectiveRange` reads it live, so a tracker
        // still on the default when the first entity is paired would pair it
        // at the wrong range and only correct itself on the next section
        // change.
        tracker.set_broadcast_range_percentage(info.entity_broadcast_range_percentage);
        let openers = crate::containers::Openers::shared();
        let block_events = crate::containers::BlockEvents::shared();
        // How far each sound carries. Loaded here rather than taken from
        // `SessionTables` for the same reason `shapes`, `classes` and
        // `protocol_ids` are: this closure outlives every connection and owns
        // nothing from one. All four are process-wide `OnceLock`s, so this is
        // the same table the connection threads hold, not a second copy — the
        // console line about it is printed once, by whichever asks first.
        let sound_events = crate::sound_events::SoundEventTable::shared(std::path::Path::new(
            crate::sound_events::TABLE_PATH,
        ))
        .ok();
        let protocol_ids = crate::protocol_ids::shared();
        clock.spawn(move |now| {
            // `ServerLevel.tick`'s FIRST step, before weather and before
            // everything else: offsets 32..48, `push("world border")` then
            // `getWorldBorder().tick()`. Ahead of the `world` guard below
            // because a border does not need a world — it is the server's,
            // not the level's, and a worldless server still owes a joining
            // client an up-to-date `initialize_border`.
            //
            // It broadcasts nothing. `WorldBorder.tick()` is
            // `extent = extent.update()` and reaches no `BorderChangeListener`;
            // the shrink a client draws is its own interpolation of the one
            // `set_border_lerp_size` it was sent. What this call is FOR is
            // that `getSize()` and a later joiner's `initialize_border` both
            // read the extent — see `crate::bordercmd`.
            crate::worldborder::shared()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .tick();
            let Some(world) = world else { return };
            // The tick, in `ServerLevel.tick`'s own order, and the order is
            // the point. Scheduled block ticks run first and only *record*
            // what they changed; `ServerChunkCache.tick` then broadcasts the
            // whole tick's worth, coalesced per section; and the
            // acknowledgements come last, from
            // `ServerGamePacketListenerImpl.tick`. A placement therefore
            // reaches the acting client as resync, resync, broadcast, ack —
            // which is what real vanilla was measured doing on 2026-08-03,
            // and what this server got wrong in all three positions.
            //
            // The lock is held across all three so a connection thread
            // cannot interleave a handler between them; vanilla gets that
            // from being single-threaded.
            let _tick = crate::broadcast::tick_lock();
            // `ServerLevel.tick`'s SECOND item, after the world border and
            // before everything else in this closure. The profiler strings
            // in that method give the order outright — `world border`,
            // `weather`, then the sleep check, `updateSkyBrightness`,
            // `tickTime`, `tickPending` (which is where scheduled block
            // ticks live), `chunkSource`, `blockEvents`, `entities` — so
            // weather runs *ahead* of the scheduled-tick drain below and
            // not, as would be easier, beside the block broadcast.
            //
            // Nothing else in this server ticks a world border, so the
            // first item is absent rather than reordered.
            for send in crate::weather::tick() {
                // Both `broadcastAll` overloads reach the same set here:
                // this server serves one dimension. `crate::weather::Audience`
                // carries which overload vanilla used so a second dimension
                // cannot silently inherit the wrong one.
                ticking.broadcast(rustmc_proto::ids::play::cb::GAME_EVENT, &send.body());
            }
            // Where an entity is, for the two things in this tick that ask:
            // the container recheck's distance filter and the block-event
            // broadcast's 64-block radius. Both take live positions, which is
            // what makes a chest shut when its opener walks off rather than
            // when it clicks something.
            let locate = |id: i32| entities.get(id).map(|e| [e.x, e.y, e.z]);
            // For a freshly spawned `FallingBlockEntity`'s immediate pairing
            // (`ChunkMap.addEntity` — `add_entity` must precede the block
            // change made beside it, `docs/PARITY-ROADMAP.md` §95) and for
            // `crate::movement`'s landing send. One snapshot serves both:
            // nothing moves a player's own entity under the `_tick` lock
            // above, so it is as fresh at the end of this closure as at the
            // start.
            let players_now = tracked_players(&ticking, entities);
            for tick in queue.drain_due(now) {
                // `runCollectedTicks` polls the tick out of
                // `toRunThisTickSet` before it accepts it, so from here on
                // the block's own rule sees `willTickThisTick == false`.
                queue.mark_run(&tick);
                // A scheduled tick reaches `Level.setBlock` like any other
                // write, so it can cascade into a `Level.destroyBlock` and
                // produce a `level_event` — and it can play a sound of its
                // own, which is how a button's un-press is heard. Both go out
                // from here, immediately, because `PlayerList.broadcast` has
                // no queue on either path; only the block positions wait for
                // the flush below. See `crate::shapes::Effects`.
                let effects = crate::ticks::run(&tick, world, shapes, classes, now);
                // A tick that books another tick — the observer's second half,
                // the repeater's pulse extension, the torch's burnout restart.
                // Stamped with the same `now` the drain used, so a delay of 2
                // lands two ticks from the tick that asked, which is what
                // `Level.scheduleTick` does with `getGameTime()`. It cannot be
                // re-drained by this pass: `drain_due` already took its
                // snapshot.
                crate::ticks::drain_effects(queue, &effects, now);
                crate::sounds::send(
                    &ticking,
                    protocol_ids,
                    sound_events,
                    Some(entities),
                    &effects.sounds,
                );
                // The 64-block radius, as above `record_updates`.
                let level_event_conns = ticking.connections();
                crate::shapes::each_event_and_drop(
                    &effects.level_events,
                    &effects.drops,
                    |event, pos, data| {
                        if let Ok(body) = crate::interact::level_event_body(event, pos, data, false)
                        {
                            crate::playerlist::broadcast_within(
                                &level_event_conns,
                                &crate::playerlist::entity_locator(Some(entities)),
                                None,
                                (f64::from(pos.0), f64::from(pos.1), f64::from(pos.2)),
                                crate::playerlist::LEVEL_EVENT_RANGE,
                                rustmc_proto::ids::play::cb::LEVEL_EVENT,
                                &body,
                            );
                        }
                    },
                    |drop| crate::dig::cascade_drop(&ticking, drop),
                );
                for (pos, _) in effects.blocks {
                    changed.record(pos);
                }
                // `FallingBlockEntity.fall`'s `addFreshEntity` half:
                // `crate::fallable::tick` already wrote the block gone (that
                // write is among `effects.blocks` above, in `fall`'s own
                // order — see its call site in `crate::ticks::run`), so this
                // is only the entity. `crate::fallable::Cargo` remembers the
                // state the block was carrying for the landing tick, which is
                // the one place that reads it back.
                if let Some(kinds) = crate::entities::shared_kind_ids() {
                    for fall in &effects.falls {
                        let state_id =
                            crate::edits::Blocks::state_id(world, &fall.state).unwrap_or(0);
                        let e = crate::fallable::spawn(entities, fall, state_id);
                        crate::fallable::Cargo::shared().set(e.id, fall.state.clone());
                        tracker.add(&e, &players_now, kinds);
                    }
                }
                // `DispenserBlock.tick` -> `dispenseFrom`, run here because
                // it needs the block-entity store and the tracker. In
                // vanilla's order: `addFreshEntity` pairs synchronously, so an
                // item's `add_entity` leaves before the click and the smoke.
                for &pos in &effects.dispenses {
                    let rules: &dyn crate::hopper::Rules = match tick_fuels {
                        Some(f) => f,
                        None => &crate::hopper::NoRules,
                    };
                    let Some(outcome) =
                        crate::dispenser::run_at(pos, world, rules, tick_recipes.as_ref())
                    else {
                        continue;
                    };
                    for o in outcome {
                        match o {
                            crate::dispenser::Outcome::Spawn(spawn) => {
                                let Some(kinds) = crate::entities::shared_kind_ids() else {
                                    continue;
                                };
                                let e = crate::dispenser::spawn_item_entity(
                                    entities,
                                    &spawn,
                                    tick_items.as_ref(),
                                );
                                tracker.add(&e, &players_now, kinds);
                            }
                            crate::dispenser::Outcome::SetBlock(at, next, flags) => {
                                let mut fx = crate::shapes::Effects::new();
                                crate::shapes::set_block(
                                    world,
                                    shapes,
                                    at,
                                    &next,
                                    flags,
                                    crate::shapes::RECURSION_LIMIT,
                                    &mut fx,
                                );
                                crate::ticks::drain_requests(queue, &fx.ticks, now);
                                for (p, _) in fx.blocks {
                                    changed.record(p);
                                }
                            }
                            crate::dispenser::Outcome::SetChanged(at) => {
                                let fx = crate::dispenser::set_changed(world, shapes, at);
                                crate::ticks::drain_requests(queue, &fx.ticks, now);
                                for (p, _) in fx.blocks {
                                    changed.record(p);
                                }
                            }
                            crate::dispenser::Outcome::LevelEvent(event, at, data) => {
                                if let Ok(body) =
                                    crate::interact::level_event_body(event, at, data, false)
                                {
                                    crate::playerlist::broadcast_within(
                                        &ticking.connections(),
                                        &crate::playerlist::entity_locator(Some(entities)),
                                        None,
                                        (f64::from(at.0), f64::from(at.1), f64::from(at.2)),
                                        crate::playerlist::LEVEL_EVENT_RANGE,
                                        rustmc_proto::ids::play::cb::LEVEL_EVENT,
                                        &body,
                                    );
                                }
                            }
                        }
                    }
                }
                // `ChestBlock.tick` -> `ChestBlockEntity.recheckOpen`. Same
                // queue, same drain, same `UNIQUE_TICK_HASH` deduplication as
                // every other block tick — see
                // `crate::ticks::schedule_container_recheck`.
                if crate::containers::has_openers_counter(&tick.block)
                    && crate::edits::Blocks::state_at(world, tick.pos)
                        .split('[')
                        .next()
                        == Some(tick.block.as_str())
                    && openers.recheck(block_events, tick.pos, &tick.block, &locate)
                {
                    crate::ticks::schedule_container_recheck(queue, tick.pos, &tick.block, now);
                }
            }
            // `fluidTicks.tick(now, 65536, ServerLevel::tickFluid)` —
            // `ServerLevel.tick` offset 296, straight after the block queue at
            // 269 and under the same `getGameTime()`. **The whole block queue
            // has drained before the first fluid tick runs**, whatever order
            // they were asked for in; see `crate::ticks::ScheduledTicks`.
            // A fluid tick's writes are recorded for this tick's broadcast
            // exactly as a block tick's are, and its level events (the lava
            // fizz) go out at once, as `Level.levelEvent` does.
            if let Ok(rules) =
                crate::fluid::Rules::shared(std::path::Path::new(crate::fluid::RULES_PATH))
            {
                for tick in queue.drain_due_fluid(now) {
                    queue.mark_run_fluid(&tick);
                    let effects =
                        crate::fluid::tick_fluid(rules, world, shapes, tick.pos, &tick.block);
                    crate::ticks::drain_effects(queue, &effects, now);
                    crate::sounds::send(
                        &ticking,
                        protocol_ids,
                        sound_events,
                        Some(entities),
                        &effects.sounds,
                    );
                    let level_event_conns = ticking.connections();
                    for &(event, pos, data) in &effects.level_events {
                        if let Ok(body) = crate::interact::level_event_body(event, pos, data, false)
                        {
                            crate::playerlist::broadcast_within(
                                &level_event_conns,
                                &crate::playerlist::entity_locator(Some(entities)),
                                None,
                                (f64::from(pos.0), f64::from(pos.1), f64::from(pos.2)),
                                crate::playerlist::LEVEL_EVENT_RANGE,
                                rustmc_proto::ids::play::cb::LEVEL_EVENT,
                                &body,
                            );
                        }
                    }
                    for (pos, _) in effects.blocks {
                        changed.record(pos);
                    }
                }
            }
            // Random ticks — `crate::randomtick::tick`, gated by
            // `random_tick_speed` and, for every scenario but one, a no-op:
            // see that module's own doc for the design and its stated
            // scope. Placed here, before the flush, so a converted grass
            // block coalesces into the same tick's `block_update`/
            // `section_blocks_update` as everything else this tick changed
            // — matching where vanilla's own random-tick pass sits, inside
            // `ServerChunkCache.tickChunks`, ahead of `chunkMap.tick()`'s
            // broadcast.
            // Published for the autosave thread's `level.dat`, which needs
            // the tick count and cannot hold the clock. One relaxed store.
            crate::levelstore::record_tick(now);
            let random_tick_speed = crate::gamerules::GameRules::shared().random_tick_speed();
            if random_tick_speed > 0 {
                let random_tick_players = tracked_players(&ticking, entities);
                let pass = crate::randomtick::tick(
                    world,
                    shapes,
                    classes,
                    &random_tick_players,
                    random_tick_speed,
                    now,
                );
                // A growth write cascades exactly like a scheduled tick's:
                // shape updates can book ticks and destroy blocks, so the
                // same three outputs are drained the same way.
                crate::ticks::drain_requests(queue, &pass.effects.ticks, now);
                crate::sounds::send(
                    &ticking,
                    protocol_ids,
                    sound_events,
                    Some(entities),
                    &pass.effects.sounds,
                );
                let level_event_conns = ticking.connections();
                for &(event, pos, data) in &pass.effects.level_events {
                    if let Ok(body) = crate::interact::level_event_body(event, pos, data, false) {
                        crate::playerlist::broadcast_within(
                            &level_event_conns,
                            &crate::playerlist::entity_locator(Some(entities)),
                            None,
                            (f64::from(pos.0), f64::from(pos.1), f64::from(pos.2)),
                            crate::playerlist::LEVEL_EVENT_RANGE,
                            rustmc_proto::ids::play::cb::LEVEL_EVENT,
                            &body,
                        );
                    }
                }
                for (pos, _) in pass.effects.blocks {
                    changed.record(pos);
                }
            }
            // `Level.tickBlockEntities` — the one block-entity ticker this
            // server has, `DaylightDetectorBlock.tickEntity`. It fires on
            // `getGameTime() % 20 == 0`, so it costs a modulo on the other
            // nineteen ticks.
            //
            // **Placed after the scheduled ticks and before the flush, and
            // the position between those two is what is measured rather than
            // the position within the tick.** Nothing in the corpus can see
            // whether a detector's write happens before or after a scheduled
            // tick — one detector cannot observe another's write, both land
            // in the same coalesced `changed.flush` below, and no scheduled
            // tick writes to a detector's position. Before the flush is the
            // half that IS observable and is not a choice: a write recorded
            // after it would arrive a tick late.
            let top_y = crate::sky::active().map_or(320, crate::sky::SkyModel::top_y);
            let writes = crate::sky::tick_detectors(
                now,
                &|pos| crate::edits::Blocks::state_at(world, pos),
                &|pos| {
                    crate::light::sky_light(
                        |p| crate::edits::Blocks::state_at(world, p),
                        pos,
                        top_y,
                    )
                },
            );
            for (pos, next) in writes {
                let mut effects = crate::shapes::Effects::new();
                crate::shapes::set_block(
                    world,
                    shapes,
                    pos,
                    &next,
                    crate::sky::UPDATE_SIGNAL_STRENGTH_FLAGS,
                    crate::shapes::RECURSION_LIMIT,
                    &mut effects,
                );
                crate::ticks::drain_effects(queue, &effects, now);
                for (p, _) in effects.blocks {
                    changed.record(p);
                }
            }
            // `ChunkHolder.broadcastChanges`, light half — and it is
            // **before** the block half, which is not a preference:
            //
            //   0: if (!hasChangesToBroadcast()) return;
            //  13: if (!skyChanged.isEmpty() || !blockChanged.isEmpty()) {
            //  57:     new ClientboundLightUpdatePacket(pos, engine, sky, block)
            //  86:     broadcast(players, packet)
            //  89:     skyChanged.clear(); blockChanged.clear();
            //     }
            // 103: if (!hasChangedSections) return;
            // 111: ... the block updates
            //
            // So a torch placed in the dark puts its light on the wire ahead
            // of the block that emits it. A client that got them the other
            // way round would render one frame of a lit torch in an unlit
            // room; ours is the vanilla order because the bytecode is.
            for u in world.light_updates() {
                if let Ok(body) = u.encode() {
                    ticking.broadcast(rustmc_proto::ids::play::cb::LIGHT_UPDATE, &body);
                }
            }
            changed.flush(&ticking, world);
            // `Entity.tick()`'s gravity/move half for the kinds this server
            // simulates locally — today, `FallingBlockEntity` only. In real
            // vanilla this runs *before* `ServerChunkCache.tick`'s broadcast,
            // inside `Level.tick`'s own entity-tick phase; here it runs after
            // the phase above, so a second, smaller `flush` puts a landing
            // this tick's `block_update` on the wire in the same tick as its
            // own `send_to_tracking_players` rather than one tick late. See
            // `crate::movement`'s module docs.
            crate::movement::tick(
                entities,
                world,
                &crate::movement::Tables {
                    shapes: collision_shapes.as_ref(),
                    props: movement_props.as_ref(),
                    support: crate::fallable::Support {
                        props: crate::fallable::props(),
                        classes,
                        tags: crate::fallable::tags(),
                    },
                },
                changed,
                tracker,
                &players_now,
            );
            // `ItemEntity.tick` for every item on the floor — gravity,
            // friction, the pickup delay, merging, the 6000-tick despawn. The
            // pickup itself is the player's, in the connection tick: see
            // `crate::pickup`.
            if let (Some(shapes), Some(props)) =
                (collision_shapes.as_ref(), movement_props.as_ref())
            {
                crate::itementity::tick(
                    crate::itementity::ItemEntities::shared(),
                    entities,
                    &crate::movement::WorldCollision(world),
                    &crate::itementity::Tables {
                        shapes,
                        props,
                        item_props: tick_item_props,
                    },
                    tracker,
                    &players_now,
                );
            }
            changed.flush(&ticking, world);
            // `ChunkMap.tick()`, entity half: membership on a section change,
            // `sendChanges` every tick.
            //
            // Placed after the block broadcast and before the acks, which is
            // where `ServerChunkCache.tick` calls `chunkMap.tick()` — between
            // `broadcastChanges` and the connection tick that flushes
            // acknowledgements. Nothing in the corpus separates the three
            // today, but the ack's position relative to a block broadcast was
            // already measured to matter once, so this one is placed rather
            // than left wherever it was convenient.
            // `LivingEntity.updateSwingTime`, run once per entity per tick.
            //
            // Vanilla reaches it from `LivingEntity.aiStep`, inside the
            // entity tick that `ServerLevel.tickNonPassenger` runs — so it is
            // in the level tick and **before** `ServerChunkCache.tick`'s
            // broadcast, which is where it sits here. That ordering is the
            // only thing that decides which side of the swallow a swing
            // arriving `n` ticks later falls on, so it is placed rather than
            // put wherever it fitted.
            //
            // Every entity, not only players: `swingTime` is a
            // `LivingEntity` field and a dropped item simply never has
            // `swinging` set, so the loop costs a branch per entity and
            // needs no kind test that could go stale.
            //
            // `update_all` rather than `all()` and an `update` each — one
            // lock and one pass, against a clone of the whole entity list and
            // a lock round trip per id, to touch three `i32`s.
            entities.update_all(crate::entities::update_swing_time);
            if let Some(kinds) = crate::entities::shared_kind_ids() {
                let players = tracked_players(&ticking, entities);
                tracker.tick(entities, &players, kinds);
                // `ServerLevel.tick` offset 471: `entityTickList.forEach`,
                // after `chunkSource.tick` (342) and `runBlockEvents`
                // (360). The mobs move here and are broadcast next tick.
                // The day time is the overworld clock's total ticks, which
                // this server drives from the game time.
                crate::mob::tick_all(now, now, entities, tracker, &players, server_view_distance);
                // `AbstractBoat.tick`, in the same entity-tick phase. See
                // `crate::boat` for what a passenger-less boat does.
                if let (Some(shapes), Some(props)) =
                    (collision_shapes.as_ref(), movement_props.as_ref())
                {
                    crate::boat::tick_all(world, shapes, props, entities);
                }
                // The orbs, in the same `entityTickList.forEach` phase.
                crate::xporb::tick_all(
                    &crate::xporb::Level {
                        entities,
                        tracker,
                        players: &players,
                        kinds,
                    },
                    world,
                    collision_shapes.as_ref(),
                    movement_props.as_ref(),
                );
            }
            // `ServerLevel.runBlockEvents`, at offset 360 — **after**
            // `ServerChunkCache.tick` (342) and therefore after both the
            // block broadcast and the entity half, and before the
            // acknowledgements. A chest lid queued during a right-click
            // handler thus reaches the client behind that tick's
            // `block_update`s, which is the order the corpus recorded from
            // real vanilla.
            crate::containers::run_block_events(
                block_events,
                &ticking,
                world,
                protocol_ids,
                &locate,
            );
            // `Level.tickBlockEntities`, offset 484 of `ServerLevel.tick` —
            // after `runBlockEvents` (360). The only block entities that
            // tick here are the pistons'. What the block events wrote and
            // what the entities write both land in `Pending` and are
            // recorded now, for the next tick's broadcast, exactly as a
            // scheduled tick's writes are above.
            let mut moved = crate::pistonmove::Pending::shared().take();
            crate::pistonmove::tick_all(world, crate::power::shared(), shapes, now, &mut moved);
            crate::ticks::drain_effects(queue, &moved, now);
            crate::sounds::send(
                &ticking,
                protocol_ids,
                sound_events,
                Some(entities),
                &moved.sounds,
            );
            crate::shapes::each_event_and_drop(
                &moved.level_events,
                &moved.drops,
                |event, pos, data| {
                    if let Ok(body) = crate::interact::level_event_body(event, pos, data, false) {
                        ticking.broadcast(rustmc_proto::ids::play::cb::LEVEL_EVENT, &body);
                    }
                },
                |drop| crate::dig::cascade_drop(&ticking, drop),
            );
            for (pos, _) in moved.blocks {
                changed.record(pos);
            }
            // `ServerPlayer.doTick` -> `this.containerMenu.broadcastChanges()`.
            // In the entity tick, so after the block broadcast above and
            // before the acknowledgements below — the order vanilla's own
            // recording of the flower pot carries. See
            // `crate::players::Connection::pending_menu`.
            ticking.flush_menu_sweeps();
            crate::world_send::sweep_open_menus(&ticking, tick_items.as_ref());
            // `ServerLevel.tick` -> `tickBlockEntities()`, and it is the LAST
            // of the level's own passes: the bytecode order is `LevelTicks
            // .tick` twice, `Raids.tick`, `ServerChunkCache.tick`,
            // `runBlockEvents`, `EnderDragonFight.tick`, `EntityTickList
            // .forEach`, then this. That is **after** the block broadcast,
            // not before it — a note on this branch said the opposite and the
            // bytecode refuted it — so a block a ticker rewrites here is
            // recorded now and flushed by the *next* tick's `changed.flush`,
            // one tick later than a scheduled tick's write. A furnace that
            // lights at tick N sends its `lit` update at N+1.
            {
                let sim = tracked_players(&ticking, entities);
                let mut chunks: std::collections::BTreeSet<(i32, i32)> =
                    std::collections::BTreeSet::new();
                for p in &sim {
                    let (cx, cz) = p.chunk;
                    for dx in -crate::randomtick::CHUNK_RADIUS..=crate::randomtick::CHUNK_RADIUS {
                        for dz in -crate::randomtick::CHUNK_RADIUS..=crate::randomtick::CHUNK_RADIUS
                        {
                            chunks.insert((cx + dx, cz + dz));
                        }
                    }
                }
                let tables = crate::behaviour::TickTables {
                    recipes: tick_recipes.as_ref(),
                    fuels: tick_fuels,
                    songs: tick_songs,
                    shapes,
                };
                let effects = crate::behaviour::tick_block_entities(
                    crate::blockentity::BlockEntities::shared(),
                    world,
                    &chunks,
                    &tables,
                    now,
                );
                for pos in effects.blocks {
                    changed.record(pos);
                }
                // A comparator a ticker's `setChanged` reached books its tick
                // against this tick's `getGameTime()`, as every cascade does.
                crate::ticks::drain_requests(queue, &effects.ticks, now);
                // `ItemEntity.discard()` from a hopper's suck. `removeEntity`
                // unpairs at once, so the `remove_entities` leaves inside the
                // block-entity pass and not at the next broadcast.
                for id in effects.discarded {
                    entities.remove(id);
                    tracker.remove(id, &sim);
                }
                crate::sounds::send(
                    &ticking,
                    protocol_ids,
                    sound_events,
                    Some(entities),
                    &effects.sounds,
                );
                for (event, pos, data) in effects.level_events {
                    if let Ok(body) = crate::interact::level_event_body(event, pos, data, false) {
                        ticking.broadcast(rustmc_proto::ids::play::cb::LEVEL_EVENT, &body);
                    }
                }
            }
            ticking.flush_acks();
            // `ServerGamePacketListenerImpl.tick` -> `tickPlayer` -> `doTick`,
            // the experience half (touch, `set_experience`), right behind the
            // ack it follows in vanilla; then `handleConsoleInputs`, which
            // `DedicatedServer.tickConnection` runs after every connection.
            // See `crate::experience`.
            if let Some(kinds) = crate::entities::shared_kind_ids() {
                let players = tracked_players(&ticking, entities);
                let ctx = crate::experience::Ctx {
                    players: &ticking,
                    protocol_ids,
                    sound_events,
                    level: crate::xporb::Level {
                        entities,
                        tracker,
                        players: &players,
                        kinds,
                    },
                };
                crate::experience::connection_tick(&ctx);
                crate::experience::run_console(&ctx);
            }
            // Last in the tick, and outside everything above it: retiring a
            // tile is not a game event and must not sit between two that are.
            // A tile removed here is still alive for any thread that is
            // mid-encode on it — that is what the `Arc` is for — so this
            // cannot interrupt a chunk already going out.
            if now % TILE_SWEEP_TICKS == 0 {
                retire_unwatched_tiles(world, &ticking, entities, server_view_distance);
            }
            // `ServerChunkCache.tickChunks`' habitation half, every tick.
            // `crate::inhabited` differences the game clock itself, so a
            // sweep that ran on a tick where nothing moved costs one lock and
            // credits the right amount; running it less often would still be
            // correct in total (the delta absorbs it) and would credit chunks
            // a player had already left, which is the wrong answer for the
            // right total.
            crate::inhabited::Inhabited::shared().sweep(
                now,
                world.resident_chunks(),
                &inhabitants(&ticking, entities),
            );
            // `MinecraftServer.tickServer`'s last act before the profiler's
            // `tallying` push: decrement `ticksUntilAutosave` and, at zero,
            // `autoSave()`.
            //
            // **In the tick, and blocking it, which is what vanilla does.**
            // `autoSave` runs on the server thread and is why "Autosave
            // started" and "Autosave finished" exist as log lines at all — a
            // save is a hitch, and moving it off the tick to hide the hitch
            // would be a behavioural change dressed as a performance one. It
            // is placed after the tile sweep for the same reason the sweep is
            // last: neither is a game event, and a save that ran between two
            // that are would be saving a half-applied tick.
            //
            // It is also placed after the sweep rather than before it so a
            // tile the sweep is about to retire is saved by the sweep's own
            // path rather than twice.
            if crate::autosave::due_now() {
                autosave_now(world);
            }
            // `runServer`'s loop leaving, and `stopServer` running on the
            // server thread behind it. Last in the tick, so the world this
            // saves is a world with no half-applied tick in it; and after the
            // autosave rather than before, so a stop that arrives on the same
            // tick as a scheduled save does not do the work twice.
            //
            // Does not return.
            if crate::shutdown::requested() {
                crate::shutdown::stop_server(world);
            }
        });
    }
    // Ready, and only now. The bind is done, the game clock is running and the
    // next statement is `accept`, so anything that reads this line as "the
    // server is up" is reading something that is true when it is printed.
    //
    // `local_addr` rather than the requested address: a caller that asked for
    // port 0 got an ephemeral one, and a readiness proof that reads the port
    // out of this line has to be reading the port that was bound.
    on_bound(&listener.local_addr()?);
    loop {
        let (mut stream, peer) = match listener.accept() {
            Ok(v) => v,
            Err(e) => {
                on_error(&"0.0.0.0:0".parse().expect("literal"), &e);
                continue;
            }
        };
        let on_error = on_error.clone();
        let players = std::sync::Arc::clone(&players);
        // The status the *server list* sees, with the live player count
        // rather than a constant. A status query is a whole connection, so
        // this is read once per query and never held.
        let clock = clock.clone();
        let mut info = info.clone();
        info.online_players = i32::try_from(players.count()).unwrap_or(i32::MAX);
        std::thread::spawn(move || {
            serve_connection(&mut stream, &peer, &info, &players, &clock, &on_error);
        });
    }
}

/// Every connected player's position, for `crate::inhabited`.
///
/// `ChunkMap.playerIsCloseEnoughForSpawning` filters spectators out first.
/// This server has no spectator mode — `game_type` is survival or creative —
/// so the filter is written and can never fire today. It is here rather than
/// omitted because omitting it makes the rule silently wrong on the day
/// spectator lands, and a habitation counter that credits chunks a spectator
/// is flying over is not a thing anybody would notice.
fn inhabitants(
    players: &crate::players::Players,
    entities: &crate::entities::Entities,
) -> Vec<(f64, f64, f64)> {
    const SPECTATOR: u8 = 3;
    if game_type() == SPECTATOR {
        return Vec::new();
    }
    players
        .connections()
        .iter()
        .filter_map(|c| entities.get(c.entity_id()))
        .map(|e| (e.x, e.y, e.z))
        .collect()
}

/// `MinecraftServer.autoSave()`'s body, once the counter has fired.
///
/// Everything it needs is process-wide — the store, the entity list, the
/// tiles — so this takes only the tile map, which the tick closure already
/// holds a reference to and which `Tiles::shared` would otherwise have to be
/// asked for with a radius this function has no business knowing.
///
/// Silent when there was nothing to write, and loud when there was: an idle
/// server should not print a line every five minutes, and a server that just
/// wrote four hundred chunks and nineteen players should say so. The two
/// counts are printed separately because they fail separately —
/// `crate::autosave::save_everything` returns them apart for that reason.
fn autosave_now(tiles: &crate::tiles::Tiles) {
    let Some(store) = crate::levelstore::shared() else {
        return;
    };
    let state = crate::levelstore::WorldState {
        seed: 0,
        ticks: crate::levelstore::ticks(),
        game_type: game_type(),
        epoch_millis: crate::levelstore::epoch_millis(),
        spawn: (8, 100, 8),
        brand: format!("rustmc {}", rustmc_proto::GAME_VERSION),
    };
    let (players, chunks) =
        crate::autosave::save_everything(store, crate::entities::Entities::shared(), tiles, &state);
    if players > 0 || chunks > 0 {
        eprintln!("autosave: wrote {chunks} chunk(s) and {players} player(s)");
    }
}

/// How often the tile sweep runs, in ticks. Once a second at 20 Hz.
///
/// Not every tick: the sweep takes two short locks and walks the resident
/// tile list, and what it is deciding — whether a tile has been unwatched for
/// thirty seconds — cannot change meaningfully inside 50 ms. Once a second
/// makes the cost invisible and still bounds how long memory is held past the
/// moment it stopped being needed.
const TILE_SWEEP_TICKS: i64 = 20;

/// Retire every tile no connected player can see — the sweep that makes this
/// server's resident memory fall as well as rise.
///
/// # It refuses to act on incomplete information
///
/// A connection whose entity is not in the level list has no position, so
/// there is no honest answer to "what can they see". That happens for one
/// short window on every join, between `players.join` and the `entities.insert`
/// a few lines later — and permanently, for every player, when the
/// `entity_type` registry capture is missing and nothing is ever inserted.
/// Treating an unlocatable player as wanting nothing would retire the tile
/// they are standing in, so a sweep that cannot locate everybody does
/// nothing at all instead. Memory held for one extra second is a cost; a
/// world that dissolves under a player is a defect.
/// # The view distance it uses is the SERVER's, and that is not a detail
///
/// A connection carries `player_view_distance(requested, server)` — the
/// client's own request, clamped down to the server's ceiling — while the
/// chunk sender is built with `info.view_distance`, the server's, unclamped:
/// see the `ChunkView::new` call in `serve_connection`. The two are equal for
/// a client that asks for at least as much as the server offers and
/// **different for one that asks for less**, which is a perfectly ordinary
/// client setting.
///
/// Retiring on the smaller of the two would retire tiles the sender is still
/// streaming into. So this takes the larger, which is what is actually being
/// sent. Keeping a tile nobody turns out to want costs one tile for thirty
/// seconds; retiring one that is being streamed costs the player a hole in
/// the world.
fn retire_unwatched_tiles(
    tiles: &crate::tiles::Tiles,
    players: &crate::players::Players,
    entities: &crate::entities::Entities,
    server_view_distance: i32,
) {
    let conns = players.connections();
    let mut wanted = std::collections::BTreeSet::new();
    let mut located = 0usize;
    for conn in &conns {
        if let Some(e) = entities.get(conn.entity_id()) {
            located += 1;
            let reach = conn.view_distance().max(server_view_distance);
            wanted.append(&mut tiles.tiles_in_view(e.chunk(), reach));
        }
    }
    if located < conns.len() {
        return;
    }
    tiles.retain_around(&wanted);
}

/// `ServerLevel.players()` as the entity tracker needs it: every connection
/// paired with where its player currently is.
///
/// A connection whose entity is not in the level list is skipped rather than
/// placed at the origin. That happens for exactly one window — between
/// `players.join` and the `entities.insert` a few lines later — and a player
/// briefly at (0, 0, 0) would be announced to everyone within 512 blocks of
/// spawn and then never corrected, because `updatePlayers` only re-runs on a
/// section change and the phantom never moves.
pub(crate) fn tracked_players(
    players: &crate::players::Players,
    entities: &crate::entities::Entities,
) -> Vec<crate::tracker::TrackedPlayer> {
    players
        .connections()
        .into_iter()
        .filter_map(|conn| {
            let e = entities.get(conn.entity_id())?;
            Some(crate::tracker::TrackedPlayer {
                entity_id: conn.entity_id(),
                pos: (e.x, e.y, e.z),
                chunk: e.chunk(),
                view_distance: conn.view_distance(),
                conn,
            })
        })
        .collect()
}

/// One accepted connection, on its own thread.
fn serve_connection(
    stream: &mut std::net::TcpStream,
    peer: &std::net::SocketAddr,
    info: &StatusInfo,
    players: &crate::players::Players,
    clock: &crate::tick::GameClock,
    on_error: &impl Fn(&std::net::SocketAddr, &io::Error),
) {
    // `ServerConnectionListener$1.initChannel` offsets 0..13:
    // `channel.config().setOption(ChannelOption.TCP_NODELAY, true)`. Without
    // it the kernel holds a small frame back until the client ACKs the one
    // before — Nagle against the client's delayed ACK — and a burst of small
    // packets reaches the client in two instalments ~60 ms apart. Measured
    // 2026-09-30 on `008`: the hurt sound written right after a
    // `damage_event` arrived 60 ms after it, in the harness's NEXT step,
    // where vanilla's arrives in the same millisecond.
    let _ = stream.set_nodelay(true);
    match serve_one(stream, info) {
        // The handshake reads must block: a client takes as long as it takes
        // to answer. Only once it is in play does a timeout become useful,
        // and then only so the keep-alive loop can get back to its clock
        // instead of blocking forever on a quiet client.
        //
        // Setting it up front instead — which is what this did first — makes
        // `configure` time out mid-exchange and the client never reaches play
        // at all.
        Ok(InPlay {
            playing: true,
            spawn,
            name,
            player: identity,
        }) => {
            // One tick, not 500 ms: the loop simulates the player's ticks when
            // it wakes, so a console line (a `/damage`) waited up to half a
            // second for its tick, and three lines 100 ms apart landed in one
            // batch — recorded 2026-09-30 on `008`, all at step 2 where
            // vanilla answers at steps 0 and 1.
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(50)));
            // The outbound half is cloned off the socket and put behind the
            // connection's own lock, because another player's thread now has
            // a reason to write here — a block they broke. The inbound half
            // stays on this thread, which is the only one that ever reads it.
            let Ok(out) = stream.try_clone() else {
                return;
            };
            // The fallback for a connection that reached play without an
            // identity. `view_distance` here was `WORLD_RADIUS` — the size of
            // a *tile*, which is the exact conflation `DEFAULT_VIEW_DISTANCE`
            // warns about two constants above, and it was invisible only
            // because both happened to be small. It is the server's view
            // distance, which is what every other path on this connection
            // uses.
            let identity = identity.unwrap_or(PlayerIdentity {
                entity_id: 0,
                uuid: 0,
                view_distance: info.view_distance,
                // FULL: a connection with no client information is one this
                // server never configured, and silencing it would be a
                // stronger claim than we have evidence for.
                chat_visibility: 0,
                // The definition of `DATA_PLAYER_MODE_CUSTOMISATION`, which
                // is what a connection that never configured would have.
                model_customisation: 0,
            });
            // The spawn the join sent, needed here because the connection
            // now carries this player's chunk view and that view has to open
            // on the centre `enter_world` already named. See
            // `ChunkView::announced_center`.
            let (sx, sy, sz) = spawn.unwrap_or((8.0, 100.0, 8.0));
            let mut player = crate::player::PlayerState::at(sx, sy, sz);
            // `enter_world` ended the join sequence with a
            // `ClientboundPlayerPositionPacket` carrying teleport id 1, and in
            // vanilla that write goes through `teleport(...)`, which arms
            // `awaitingTeleport`/`awaitingPositionFromClient` in the same
            // breath. Arming it here rather than inside `enter_world` because
            // that function does not own the player state — but the id is the
            // one it wrote, and if the two ever disagree the server drops
            // every acknowledgement it receives and no client can move.
            // [`crate::world_send::tests::the_join_teleport_id_is_the_one_the_player_state_awaits`]
            // is the gate on that agreement.
            player.awaiting(crate::world_send::JOIN_TELEPORT_ID, (sx, sy, sz));
            // The saved position, if any, is already in `spawn`: it is read
            // in `serve_one` before `enter_world` sends the teleport, because
            // the client is put where that packet says and nowhere else.
            let conn = std::sync::Arc::new(crate::players::Connection::with_entity(
                players.next_id(),
                &name,
                Box::new(out),
                Some(COMPRESSION_THRESHOLD),
                identity.entity_id,
                // The same number the join packet carried and the same number
                // the tracker clamps into: vanilla's chunk sender, its login
                // packet and `getPlayerViewDistance` all read
                // `serverViewDistance`.
                identity.view_distance,
                player.chunk(),
            ));
            // What the client asked to be shown, from `client_information`.
            // Recorded before `players.join` so no broadcast can reach this
            // connection while it still holds the default.
            conn.set_chat_visibility(identity.chat_visibility);
            // Registered only once actually in play: a server-list ping is a
            // connection too, and counting those would make the player number
            // flicker with every refresh of somebody's list.
            players.join(&conn);

            // `PlayerList.sendPlayerPermissionLevel`: the permission-level
            // entity event, then the whole command tree filtered for that
            // level. Vanilla sends both from `placeNewPlayer`, in this order,
            // and a client that receives neither offers no completions and
            // believes it has no permissions — which is what this server did
            // until 2026-08-06.
            let level = crate::ops::level_for(&name);
            if let Err(e) = crate::commands::send_commands(&conn, level) {
                eprintln!("could not send the command tree to {name}: {e}");
            }
            // `PlayerList.placeNewPlayer` -> `CustomBossEvents.onPlayerConnect`:
            // every `/bossbar` whose player set holds this UUID takes the
            // connection back as a holder and sends it ADD. Nothing on a
            // server where no bar names the player.
            crate::bossbar::shared()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .on_player_connect(&conn);

            // `WORLD_RADIUS` decides how big a *tile* is, which is a
            // question about generation and not about sight; the view
            // distance went onto the connection with the view itself.
            let tiles = crate::tiles::Tiles::prepare(0, WORLD_RADIUS).ok();
            // The join already sent the spawn tile, so the view starts owning
            // it — otherwise the first step would resend every chunk the
            // client already has.
            if let Some(tiles) = tiles {
                conn.with_chunk_view(|view| {
                    for c in tiles.spawn_area().chunks() {
                        view.mark_sent(c);
                    }
                });
            }

            // `ChunkMap.addEntity` for the joining player, in vanilla's own
            // two halves: the newcomer is shown to everyone already here, and
            // then everyone already here is shown to the newcomer. Doing only
            // the first is the asymmetry that would make one client's trace
            // diverge and the other's match.
            //
            // Order matters and is vanilla's: `updatePlayers` runs before the
            // sweep over `entityMap`, so a second player joining is announced
            // to the first *before* the first is announced to the second.
            let entities = crate::entities::Entities::shared();
            let tracker = crate::tracker::Tracker::shared();
            let kinds = crate::entities::shared_kind_ids();
            if let Some(kinds) = kinds {
                let me = crate::entities::Entity {
                    id: identity.entity_id,
                    uuid: identity.uuid,
                    kind: crate::entities::PLAYER,
                    x: sx,
                    y: sy,
                    z: sz,
                    y_rot: player.yaw,
                    x_rot: player.pitch,
                    y_head_rot: player.yaw,
                    movement: (0.0, 0.0, 0.0),
                    on_ground: player.on_ground,
                    data: 0,
                    // `LivingEntity`'s own initial values: not swinging, and a null
                    // `swingingArm`, which `getCurrentSwingDuration` reads as the main
                    // hand.
                    swinging: false,
                    swing_time: 0,
                    swinging_arm: None,
                    // `LivingEntity.<init>` ends `setHealth(getMaxHealth())`
                    // and `ServerPlayer.updateOptions` writes the client's
                    // skin-parts byte into the other accessor, so both of a
                    // fresh player's non-default values are settled before it
                    // is ever tracked.
                    synched: crate::synched::Synched::Avatar {
                        health: joining_health(),
                        model_customisation: identity.model_customisation,
                        score: crate::experience::Registry::shared()
                            .joining_score(identity.entity_id),
                    },
                };
                entities.insert(me.clone());
                let others = tracked_players(players, entities);
                tracker.add(&me, &others, kinds);
                // `updatePlayerStatus(player, true)`, at `addEntity` offsets
                // 124-128 — **between** `updatePlayers` and the sweep over
                // `entityMap`, and the position is load-bearing now that
                // `updatePlayer`'s third limb reads the joiner's own view.
                // A view still holding `ChunkTrackingView.EMPTY` contains
                // nothing, so a sweep run before this would show the newcomer
                // an empty world and correct itself only at the next section
                // change.
                //
                // It writes no bytes: `announced_center` is already the spawn
                // chunk `enter_world` named, and the old view being `EMPTY`
                // means `difference` has nothing to forget. The arrivals go
                // into the chunk sender's queue, which is where the join blob
                // is not, because `mark_sent` above took the spawn tile out
                // of it.
                conn.with_chunk_view(|v| {
                    let update = v.update_tracking(player.chunk());
                    debug_assert!(
                        update.moved_center.is_none() && update.forget.is_empty(),
                        "the join's first tracking update writes nothing"
                    );
                });
                tracker.show_world_to(
                    &crate::tracker::TrackedPlayer {
                        conn: std::sync::Arc::clone(&conn),
                        entity_id: identity.entity_id,
                        pos: (sx, sy, sz),
                        chunk: player.chunk(),
                        view_distance: identity.view_distance,
                    },
                    entities,
                    kinds,
                );
            } else {
                eprintln!(
                    "no entity_type registry capture — no player is visible to any other, \
                     and nothing this server spawns will appear"
                );
            }
            // `None` now means "not yet" as often as it means "nowhere": a
            // chunk outside every generated tile starts that tile generating
            // on its own thread and answers `None` until it is ready. The
            // client is told nothing either way — to it, a chunk it does not
            // have is a chunk it does not have — and the retry in
            // `keep_alive_loop` is what makes the difference show up as one
            // arriving a few seconds later.
            let chunk_at =
                |cx: i32, cz: i32| -> io::Result<Option<crate::chunk::LevelChunkWithLight>> {
                    match tiles {
                        Some(t) => t.chunk(cx, cz),
                        None => Ok(None),
                    }
                };
            // The clocks to drive, from the capture the client was sent — a
            // registry id only means anything against the same ordering the
            // client received.
            let clock_ids: Vec<i32> = crate::configuration::CapturedRegistries::load(
                std::path::Path::new("data/vanilla/registries.bin"),
            )
            .ok()
            .and_then(|r| r.entry_ids("minecraft:world_clock").ok().flatten())
            .map(|names| {
                names
                    .iter()
                    .enumerate()
                    .filter(|(_, n)| n.as_str() == "minecraft:overworld")
                    .filter_map(|(i, _)| i32::try_from(i).ok())
                    .collect()
            })
            .unwrap_or_default();
            // The item registry, for `set_creative_mode_slot`. Absent means a
            // player can dig and not build; said out loud, because "placement
            // is not implemented" and "the extract is missing" look identical
            // from a client.
            let items =
                crate::player::ItemIds::load(std::path::Path::new("data/vanilla/items.json"));
            if let Err(e) = &items {
                eprintln!("no item table ({e}) — this player can break blocks but not place them");
            }
            let items = items.ok();
            // The six optional tables the connection loop carries, loaded
            // through the one function that loads them — see
            // `crate::session_tables`. They used to be five separate blocks
            // right here, and that was the whole defect: this was the only
            // place in the tree that set any of them, so the wiring from a
            // loaded table to the loop that reads it was witnessed by nothing
            // but a box-wide corpus run.
            //
            // `.` because a server is booted from the repo root. The one
            // other caller, `tests/session_joint.rs`, passes the repo root it
            // computes from `CARGO_MANIFEST_DIR`; nothing else about the two
            // call sites differs, which is what makes that test a witness of
            // this joint rather than of its own fixture.
            let tables = crate::session_tables::SessionTables::load(std::path::Path::new("."));
            // `useWithoutItem` per state. Absent means every block declines a
            // right-click and the placement path runs instead — which is what
            // this server did before the table existed, and which is an
            // inversion rather than an omission: a lever with a block in hand
            // gets the block placed on it. One line on the console for the
            // same reason as the two above.
            let uses = crate::useblock::UseTable::shared(std::path::Path::new(
                "data/vanilla/use_without_item.json",
            ));
            match &uses {
                Ok(t) => eprintln!("use table: {} states answer a right-click", t.len()),
                Err(e) => eprintln!(
                    "no use table ({e}) — no block answers a right-click; \
                     a lever will take a block instead of flipping"
                ),
            }
            let uses = uses.ok();
            // `useItemOn` per state and held item, for the classes
            // `crate::useitemon::PORTED_CLASSES` names. Absent means every
            // block answers `TryEmptyHandInteraction`, which is
            // `BlockBehaviour`'s own default — so unlike the table above this
            // one's absence is not an inversion, it is the item-aware branch
            // simply not running. A flower pot will not fill, and so will
            // never empty either.
            let item_uses = crate::useitemon::UseItemOnTable::shared(std::path::Path::new(
                "data/vanilla/use_item_on.json",
            ));
            match &item_uses {
                Ok(t) => eprintln!(
                    "useItemOn table: {} blocks, {} states, {} pot pairings",
                    t.blocks(),
                    t.len(),
                    t.pairings()
                ),
                Err(e) => eprintln!(
                    "no useItemOn table ({e}) — no block answers a right-click with an \
                     item in hand; a flower pot will not fill"
                ),
            }
            let item_uses = item_uses.ok();
            // Item -> recipes it unlocks, and each recipe's captured
            // `recipe_book_add` entry bytes. `None` means holding an item
            // never adds anything to the client's recipe book — said out
            // loud for the same reason every table above is.
            let recipe_unlocks = crate::recipe_book::RecipeUnlocks::shared(std::path::Path::new(
                "data/vanilla/recipe_unlocks.json",
            ));
            match &recipe_unlocks {
                Ok(t) => eprintln!(
                    "recipe unlock table: {} items name {} recipes with captured entries",
                    t.item_count(),
                    t.recipe_count()
                ),
                Err(e) => eprintln!(
                    "no recipe unlock table ({e}) — holding an item never adds anything \
                     to the client's recipe book"
                ),
            }
            let recipe_unlocks = recipe_unlocks.ok();
            // Vanilla's 1,688 advancement documents. `None` means no
            // advancement is ever awarded and no `update_advancements` packet
            // is ever sent — which is what this server did before this table
            // existed, said out loud for the same reason every table above
            // is.
            let advancements = crate::advancements::Table::shared(std::path::Path::new(
                "data/vanilla/advancements.json",
            ));
            match &advancements {
                Ok(t) => {
                    let tree = crate::advancements::Tree::build(t);
                    eprintln!(
                        "advancement table: {} documents, {} roots, {} with a display",
                        t.len(),
                        tree.roots().len(),
                        t.iter().filter(|a| a.has_display).count()
                    );
                }
                Err(e) => eprintln!(
                    "no advancement table ({e}) — no advancement is awarded and no \
                     update_advancements packet is sent"
                ),
            }
            // The engine is built from the table above plus the item tags
            // `ci/loot-golden.sh` already writes. `Engine::shared` is what
            // makes both `'static`, so a per-connection
            // `PlayerAdvancements` can borrow them.
            let adv_engine = crate::advancements::Engine::shared().ok();
            drop(advancements);
            let mut hotbar = crate::player::Hotbar::new().with_item_props(tables.item_props);
            let mut menu = crate::menu::MenuState::new();
            let mut known_recipes = std::collections::BTreeSet::new();
            // Vanilla constructs one of these per player in
            // `ServerPlayer`'s constructor and loads
            // `advancements/<uuid>.json` into it. This server does not
            // persist progress, so every connection starts empty.
            let mut player_advancements = adv_engine.map(|e| e.new_player());
            // VANILLA'S FIRST TICK, REPRODUCED, AND IT IS NOT A NO-OP.
            //
            // `flushDirty` is the last statement of `ServerPlayer.tick`, so a
            // player who has done nothing still runs it once and offset 250
            // clears `isFirstPacket` whether or not a packet went out. This
            // server has no per-player tick hook in this loop and flushes at
            // the trigger instead, so without this call the FIRST packet a
            // player ever receives would carry `reset = true` where vanilla's
            // carries false. It returns `None` by construction — no save file
            // means nothing is dirty — and the assertion says so.
            if let Some(pa) = player_advancements.as_mut() {
                match pa.flush_dirty(true) {
                    Ok(None) => {}
                    Ok(Some(_)) => eprintln!(
                        "advancements: the first flush produced a packet, which means \
                         this player joined with progress — this server does not persist \
                         any, so that is a defect and the packet was dropped"
                    ),
                    Err(e) => eprintln!("advancements: first flush failed: {e}"),
                }
            }
            let mut session = crate::world_send::Session {
                changed: crate::broadcast::ChangedBlocks::shared(),
                // The same two the clock thread drains, because a right-click
                // queues a lid on one thread and the tick boundary sends it
                // from the other. Shared here and per-session in the unit
                // tests, for the reason written on `changed`.
                block_events: crate::containers::BlockEvents::shared(),
                openers: crate::containers::Openers::shared(),
                protocol_ids: crate::protocol_ids::shared(),
                conn: &conn,
                player: &mut player,
                clock,
                clock_ids: &clock_ids,
                players,
                blocks: tiles.map(|t| t as &dyn crate::edits::Blocks),
                hotbar: &mut hotbar,
                menu: &mut menu,
                items: items.as_ref(),
                // The six from `crate::session_tables`, read straight off the
                // struct rather than unpacked into six locals first. The
                // unpacking is what a second construction site would have to
                // duplicate, and duplicating it is how the two drift.
                placement: tables.placement.as_ref(),
                recipes: tables.recipes.as_ref(),
                recipe_unlocks,
                known_recipes: &mut known_recipes,
                advancements: player_advancements.as_mut(),
                adv_engine,
                uses,
                item_uses,
                survive: tables.survive,
                bonemeal: tables.bonemeal,
                placed_by: tables.placed_by,
                shapes: tables.shapes,
                sound_types: tables.sound_types,
                sound_events: tables.sound_events,
                waypoints: crate::waypoints::Waypoints::shared(),
                ticks: tables.ticks,
                fire_odds: tables.fire_odds,
                chunk_at: &chunk_at,
                // The set loaded above, shared by every connection. `global`
                // rather than the local binding so that the production
                // session reads the plugin set through exactly the accessor a
                // second entry point would have to use.
                plugins: crate::plugins::global(),
                creative: game_type() == CREATIVE,
                seed: WORLD_SEED,
            };
            if let Err(e) = crate::world_send::keep_alive_loop(stream, &mut session) {
                on_error(peer, &e);
            }
            // `ChunkMap.removeEntity`, and in vanilla's order: the departing
            // player stops being an *observer* of everything else first —
            // otherwise its connection id stays in every `seenBy` and the
            // tracker keeps writing to a socket nobody is reading — and only
            // then is it removed as an entity, which is what tells everyone
            // else it has gone.
            //
            // `players.leave` comes last of the three so the removal
            // broadcast still finds an audience: taking the connection out of
            // the registry first would make `tracked_players` empty and the
            // `remove_entities` reach nobody, leaving a frozen copy of the
            // departed player standing on every other client.
            // `ServerWaypointManager.removePlayer`, which
            // `ServerPlayer.setServerLevel`/`remove` reaches on the way out.
            // **Before `players.leave`**, for the reason the tracker calls
            // below are: the UNTRACK naming this player has to find the other
            // connections, and taking this one out of the registry first
            // would leave a waypoint for a player who has gone on every other
            // client's locator bar.
            crate::waypoints::send(
                players,
                &crate::waypoints::Waypoints::shared().remove_player(conn.id()),
            );
            // **Saved before the entity is removed, because the entity is
            // where the position lives.** `PlayerState` is a local of the
            // play loop and has gone by the time this arm runs; the tracker's
            // entity carries the last position the loop wrote, and
            // `entities.remove` below is what destroys it. Vanilla saves on
            // the same trigger — `PlayerList.remove` calls `save(player)`
            // before `level.removePlayerImmediately`.
            if let Some(store) = crate::levelstore::shared() {
                if let Some(entity) = entities.get(identity.entity_id) {
                    let snapshot = crate::levelstore::PlayerSnapshot {
                        uuid: identity.uuid,
                        pos: [entity.x, entity.y, entity.z],
                        yaw: entity.y_rot,
                        pitch: entity.x_rot,
                        on_ground: entity.on_ground,
                        // The player's own mode, which `/gamemode` may have
                        // moved off the server default.
                        game_type: i32::from(conn.game_type()),
                        xp: crate::experience::save_fields(identity.entity_id),
                    };
                    if let Err(e) = store.save_player(snapshot) {
                        eprintln!("could not save player data: {e}");
                    }
                }
            }
            tracker.forget_observer(conn.id());
            tracker.remove(identity.entity_id, &tracked_players(players, entities));
            entities.remove(identity.entity_id);
            // `PlayerList.remove` -> `CustomBossEvents.onPlayerDisconnect`.
            crate::bossbar::shared()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .on_player_disconnect(conn.id());
            players.leave(conn.id());
        }
        Ok(InPlay { playing: false, .. }) => {}
        Err(e) => on_error(peer, &e),
    }
}

/// `GameType.SURVIVAL`, ordinal 0 — the first constant of the enum's
/// `$values`.
pub const SURVIVAL: u8 = 0;

/// `GameType.CREATIVE`, read from the enum's own `$values` — the second
/// constant, ordinal 1.
pub const CREATIVE: u8 = 1;

/// The mode a joining player is put in **by default**.
///
/// Creative, and the reasons are the world's rather than a preference. It is
/// a fixed square with nothing beyond its edge and no light data of its own,
/// so a survival player walks off into a void they cannot fly back out of;
/// and the only way a client can hold a block to place is the creative
/// inventory, because there is no survival inventory to put one in.
///
/// This was `pub const GAME_TYPE` until 2026-08-18 and its doc comment ended
/// "this constant is the one line that changes when they do". It has changed,
/// and for the reason that comment anticipated — but as a *default* rather
/// than as the only possibility, because the thing that needed it was not a
/// grown world. It was that **the whole corpus is ungradeable for damage,
/// health, hunger and mining progress while every player is creative**:
/// `Player.hurtServer` reads `Abilities.invulnerable` at offset 15 and
/// returns false at 31-32, and `ServerPlayerGameMode.handleBlockBreakAction`
/// takes the `instabuild` branch at offset 178 and returns at 194 before
/// `destroyPos` is ever assigned. See `ci/substrate-gate-audit.py`, which
/// re-reads both out of the pinned jar and refuses if either has moved.
pub const DEFAULT_GAME_TYPE: u8 = CREATIVE;

/// The mode this process puts joining players in, settled once at startup.
///
/// # Why a process-wide cell and not a per-player field
///
/// A per-player game mode is the right end state and this is not it. Vanilla
/// keeps the mode on `ServerPlayerGameMode`, changes it with a permission-2
/// command, and announces the change with a `game_event`. None of that exists
/// here: this server has no console, no argument nodes in its command tree
/// (see `crate::commands`) and no per-player ability model
/// (`ci/checks/server-unported-inventory.json` carries `abilities-model` as a
/// GAP for exactly that). Building a per-player mode *first* would mean
/// building all three before a single damage packet could be compared.
///
/// So the unit is the **server**, matching how the harness already boots two
/// of them, and the honest cost is stated rather than discovered: a scenario
/// cannot change mode part-way, so a survival scenario cannot use
/// `Step::Hold` — `ServerGamePacketListenerImpl.handleSetCreativeModeSlot`
/// refuses the whole packet when `!player.isCreative()`, silently, with
/// nothing in the log. `ci/t2-gameplay-survival.sh` runs a separate pass over
/// scenarios written not to need one, which is why that directory is separate
/// from `ci/checks/gameplay` rather than a flag on it.
///
/// **Set before the listener opens, read after.** `set_game_type` is called
/// from `main` while the socket is still closed, so no connection can observe
/// it changing; every read afterwards is `Relaxed` because there is no other
/// memory being published alongside it.
static GAME_TYPE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(DEFAULT_GAME_TYPE);

/// The mode joining players are put in. See [`GAME_TYPE`].
#[must_use]
pub fn game_type() -> u8 {
    GAME_TYPE.load(std::sync::atomic::Ordering::Relaxed)
}

/// Settle the mode joining players are put in.
///
/// Called from `main` before the listening socket is opened. Returns `Err`
/// with the offending string for anything that is not a mode this server can
/// honestly serve — the two it names are the two it has a `creative` boolean
/// for, and adventure or spectator would need the ability model that
/// `ci/unported-inventory.py --open` still lists as a GAP.
///
/// # Errors
///
/// The name is not `creative` or `survival`.
pub fn set_game_type(name: &str) -> Result<u8, String> {
    let v = match name {
        "creative" => CREATIVE,
        "survival" => SURVIVAL,
        other => return Err(other.to_string()),
    };
    GAME_TYPE.store(v, std::sync::atomic::Ordering::Relaxed);
    Ok(v)
}

/// Vanilla's default `network-compression-threshold`: frames at least this
/// large are deflated. Negotiated in login and applies to every frame after
/// it, in both directions — the status exchange happens before login and so
/// stays uncompressed.
pub const COMPRESSION_THRESHOLD: i32 = 256;

/// How big a **tile** is, in chunks from its centre — see [`crate::tiles`].
///
/// The pipeline generates a `DECORATION_MARGIN` of 2 around a tile and offers
/// only the inside, so a tile of side `n` generates `(n + 4)^2` chunks to
/// deliver `n^2` and the difference is thrown away. **That discard is the
/// whole argument for this number**, and it is paid on every tile the server
/// ever makes rather than once at startup.
///
/// # Ten, and it is tied to [`DEFAULT_VIEW_DISTANCE`] on purpose
///
/// It was 2 until 2026-08-06. `docs/BENCH.md`'s radius sweep — noise world,
/// seed 0, view-distance 10, n=3, Threadripper 1920X, with tile generation
/// capped at `tiles::MAX_CONCURRENT_GENERATIONS` — measured this across
/// 2..12:
///
/// ```text
///   r    side   delivers   generates   wasted   boot     chunks/s   view at 6 s
///   2      5       25          81       69%     3.33 s      8.1      75 / 441
///   3      7       49         121       60%     4.07 s     12.9     147 / 441
///   4      9       81         169       52%     4.84 s     17.6      81 / 441
///   5     11      121         225       46%     5.82 s     22.0     121 / 441
///   6     13      169         289       41%     6.83 s     26.4     169 / 441
///   8     17      289         441       34%     9.33 s     32.8     289 / 441
///  10     21      441         625       29%    12.38 s     38.0     441 / 441
/// ```
///
/// Two things are monotone and one is not. Boot time and generation
/// throughput both move steadily with the radius; **resident memory does
/// not**, and after the concurrency cap it is no longer a function of this
/// number at all — the memory column of that sweep is a reading of how many
/// generations happened to be in flight when it was sampled, which is why it
/// is not quoted here.
///
/// So the trade is throughput against boot, and ten wins it:
///
/// * **69% of all generation work was being discarded at radius 2** — 81
///   chunks put through every stage to deliver 25. At radius 10 it is 29%.
///   That is 4.7x the useful chunks per second (8.1 to 38.0) and it is paid
///   forever, while the boot cost is paid once per server start;
/// * **ten is the first radius at which one tile contains a view-distance-10
///   player's whole view**, so a player who joins and stands still needs no
///   background generation at all. Every smaller radius left the view
///   incomplete after six seconds; this one delivers all 441.
///
/// The price is stated rather than buried: boot on the noise world goes from
/// 3.33 s to 12.38 s, against vanilla's 14.02 s, so the boot result narrows
/// from 4.2x to 1.13x. On superflat it goes from 0.64 s to 1.30 s against
/// vanilla's 9.47 s, so that headline stays comfortable at 7.3x. An operator
/// who wants the old trade back has `--radius`, and `main.rs` says why the
/// choice is theirs.
pub const WORLD_RADIUS: i32 = 10;

/// The world seed.
///
/// One number with one source, for the reason [`MAX_PLAYERS`] carries: it
/// reaches `Tiles::prepare` to decide what is generated **and** `/seed` to
/// tell a player what was generated, and a seed with two sources is a seed
/// that will eventually disagree with itself — a server that answers `/seed`
/// with a number its own terrain was not made from.
///
/// Not yet an operator setting. `server.properties`' `level-seed` is not
/// read by this server at all, so this is the whole of the world's identity
/// and changing it is a recompile. Said plainly rather than left to be
/// discovered by an operator who edits the properties file and sees nothing
/// change.
pub const WORLD_SEED: i64 = 0;

static LEVEL_SEED: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(WORLD_SEED);

/// `ServerLevel.getSeed()` as `getRandomSequence(id)` reads it —
/// `RandomSequences.get(id, getSeed())`, the stream every loot table with a
/// `random_sequence` draws from (`crate::mob::drops`). [`WORLD_SEED`] until
/// `--level-seed` says otherwise.
///
/// **Only the random sequences read it.** The generator is built at
/// [`WORLD_SEED`], and `/seed` and the login packet's hashed seed still
/// answer it too; on the flat preset, which is the only world the corpus
/// runs, no generator reads a seed at all, so the one place the two can
/// disagree observably is those two answers. Named here rather than
/// widened: moving them is a change to what every other check sees.
#[must_use]
pub fn level_seed() -> i64 {
    LEVEL_SEED.load(std::sync::atomic::Ordering::Relaxed)
}

/// `--level-seed <long>`.
pub fn set_level_seed(seed: i64) {
    LEVEL_SEED.store(seed, std::sync::atomic::Ordering::Relaxed);
}

/// Whether a connection reached the play state and wants holding open.
// No `Eq`: the spawn is floating point, and a derived `Eq` on it would be a
// promise this type cannot keep.
#[derive(Debug, Clone, PartialEq)]
pub struct InPlay {
    /// Whether the connection reached the play state.
    pub playing: bool,
    /// The name the client logged in with, for the player registry. Empty
    /// when the connection never got as far as a hello.
    pub name: String,
    /// Where the join packet put the player, when it did.
    ///
    /// Carried rather than recomputed by the caller: the spawn is the world's
    /// surface at the spawn column and only `serve_one` has the world in
    /// hand. A caller that guessed it would seed the player somewhere the
    /// client is not, and every position-derived decision would be wrong
    /// until the first movement packet corrected it.
    pub spawn: Option<(f64, f64, f64)>,
    /// Who the joining player *is as an entity*, when it joined.
    ///
    /// Allocated in `serve_one` rather than by the caller because the entity
    /// id has to be in the join packet — a client is told its own id there
    /// and nowhere else — so by the time the caller sees the connection the
    /// number is already on the wire and can only be reported, not chosen.
    pub player: Option<PlayerIdentity>,
}

/// The player's identity as an entity: what the join packet told the client
/// about itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerIdentity {
    /// The entity id, from `crate::entities::Entities::next_id`.
    ///
    /// **This used to be the constant 1 for everybody.**
    /// `JoinInfo::default().player_id` is 1 and no call site ever overrode
    /// it, so two connected players were both entity 1 and neither could
    /// ever be told about the other coherently. Nothing observed it while
    /// entities did not exist; everything would have, the moment they did.
    pub entity_id: i32,
    pub uuid: u128,
    /// The view distance the client was told to use, which is also the
    /// radius `ChunkMap.getPlayerViewDistance` clamps into the tracking
    /// range test.
    pub view_distance: i32,
    /// `ChatVisiblity`'s ordinal from `client_information`.
    ///
    /// Carried through here for the same reason `view_distance` is: it is
    /// read during configuration, in a different function from the one that
    /// builds the connection, and it decides whether that connection is ever
    /// sent a `player_chat` at all.
    pub chat_visibility: i32,
    /// `ClientInformation.modelCustomisation()`, carried because it is the
    /// only thing the configuration exchange learns that the player's
    /// *entity* has to know: `ServerPlayer.updateOptions` puts it into
    /// `Avatar.DATA_PLAYER_MODE_CUSTOMISATION`, and every pairing bundle for
    /// this player then carries it.
    pub model_customisation: u8,
}

/// One connection, from handshake to the play state or a refusal.
///
/// # Errors
/// On a malformed handshake or an I/O failure.
pub fn serve_one(stream: &mut (impl Read + Write), info: &StatusInfo) -> io::Result<InPlay> {
    let hs = read_handshake(stream)?;
    match hs.intent {
        Intent::Status => serve_status(stream, info).map(|()| InPlay {
            playing: false,
            name: String::new(),
            spawn: None,
            player: None,
        }),
        // Not implemented yet, and said so rather than shown by a dropped
        // socket.
        Intent::Login => {
            // 256 is vanilla's default threshold. Turned on *before*
            // login_finished, which is vanilla's order, so that packet is
            // itself compressed — see `login::enable_compression`.
            let hello = crate::login::read_hello(stream, None)?;
            let profile = crate::login::Profile::offline(&hello.name);
            crate::login::enable_compression(stream, COMPRESSION_THRESHOLD)?;
            let comp = Some(COMPRESSION_THRESHOLD);
            crate::login::finish(stream, &profile, comp)?;

            // Registries come from a capture of a real vanilla server; see
            // `CapturedRegistries`. Without them the client cannot render a
            // world, so a missing capture is reported as itself rather than
            // as a protocol failure ten packets later.
            let path = std::path::Path::new("data/vanilla/registries.bin");
            let registries = match crate::configuration::CapturedRegistries::load(path) {
                Ok(r) => r,
                Err(e) => {
                    return crate::configuration::disconnect(
                        stream,
                        &format!("rustmc: no registry capture at {}: {e}", path.display()),
                        comp,
                    )
                    .map(|()| InPlay {
                        playing: false,
                        name: profile.name.clone(),
                        spawn: None,
                        player: None,
                    });
                }
            };
            // **The return value used to be dropped.** It carries the view
            // distance the client asked for, which is one of the two numbers
            // `ChunkMap.getPlayerViewDistance` clamps together, and without
            // it this server had no way to answer the question vanilla asks
            // on every visibility test.
            let client = crate::configuration::configure_with(stream, &registries, comp)?;

            // Into play. Two worlds are possible and which one a player gets
            // depends on what is on disk:
            //
            //   with the vanilla extract   the whole pipeline — surface
            //                              rules, carvers, decoration
            //   without it                 the noise stage alone
            //
            // The extract is gitignored jar-derived material, so a checkout
            // that has never run the oracle scripts still serves a world
            // rather than refusing. Which one it served is said out loud,
            // because "the trees are missing" and "trees are not implemented"
            // are very different bug reports.
            let chunks;
            let spawn;
            // The player's own entity id, drawn from the level's one
            // allocator so it cannot collide with an item's or another
            // player's. It has to be settled here because the join packet
            // carries it and the join packet is about to go out.
            let entity_id = crate::entities::Entities::shared().next_id();
            // Read once, here, rather than three times inside the literal:
            // vanilla reads `level.getGameRules()` into a local at offset 209
            // and takes all three booleans off it, so all three describe the
            // same instant even if a console command lands mid-join.
            let rules = crate::gamerules::GameRules::shared();
            // No longer `mut`: the one field that used to be patched after
            // construction was `chunk_radius`, and it is now settled here
            // from the server's own view distance.
            let join = crate::play::JoinInfo {
                // Not the default. See `GAME_TYPE`: the world is a fixed
                // square with no light of its own and there is no inventory
                // to hold a block in, and creative is what makes both
                // survivable. `JoinInfo::default` stays survival because the
                // captured vanilla packet it is checked against is.
                game_type: game_type(),
                player_id: entity_id,
                // `ClientboundLoginPacket`'s chunk radius is
                // `playerList.getViewDistance()` — **the server's setting**.
                //
                // This used to be `(area.max_chunk_x - area.min_chunk_x) / 2`,
                // the radius of the spawn tile, with a comment explaining that
                // the join packet wants a radius and not a diameter. The
                // arithmetic was right and the quantity was wrong: it made the
                // client's render distance, and through `PlayerIdentity` the
                // entity tracking range, a function of how much world had been
                // generated. `crate::play::JoinInfo`'s own field doc called it
                // "the client's own view distance, echoed back", which is a
                // third answer and was not what any of the code did.
                chunk_radius: info.view_distance,
                // Three of `ClientboundLoginPacket`'s booleans are game
                // rules, and all three were literals here until 2026-09-06.
                //
                // ```text
                //   PlayerList.placeNewPlayer, javap -p -c -constants
                //     211: immediateRespawn = rules.get(IMMEDIATE_RESPAWN)
                //     227: reducedDebugInfo = rules.get(REDUCED_DEBUG_INFO)
                //     243: limitedCrafting  = rules.get(LIMITED_CRAFTING)
                //     295: iload 14                       reducedDebugInfo
                //     297: iload 13 / ifne 306 / iconst_1 / goto 307
                //     306: iconst_0                       showDeathScreen
                //     307: iload 15                       doLimitedCrafting
                // ```
                //
                // **`showDeathScreen` is the NEGATION of
                // `immediate_respawn`** — offsets 297..307 — which is the
                // one place in this packet where a rule and its field have
                // opposite senses, and the reason this is transcribed
                // rather than remembered. It is also the client-side half of
                // the whole death path: with it clear the client skips the
                // death screen entirely and sends `PERFORM_RESPAWN` at once.
                //
                // `crate::gamerules::GameRules` has carried all 59 rules and
                // a live `/gamerule` since it landed, so setting
                // `immediate_respawn` on this server changed a value nothing
                // read. `JoinInfo::default()` keeps the literals, because the
                // captured vanilla packet it is gated against was taken on a
                // server at its defaults.
                show_death_screen: crate::death::show_death_screen(
                    rules.boolean("immediate_respawn"),
                ),
                reduced_debug_info: rules.boolean("reduced_debug_info"),
                do_limited_crafting: rules.boolean("limited_crafting"),
                // `playerList.getSimulationDistance()`. A literal 10 lived in
                // `JoinInfo::default` until this commit, so a server run with
                // any other setting told every client the wrong number.
                simulation_distance: info.simulation_distance,
                ..crate::play::JoinInfo::default()
            };
            // The respawn sends this block again; see `crate::playerdeath`.
            crate::playerdeath::remember_join(&join);
            // `prepare` is where the seconds go, and `main` has already paid
            // them before the socket was opened — see `World::prepare`. What
            // is left here is encoding chunks that already exist.
            if worldgen_assembly::have_extract() {
                // Whatever radius `main` prepared; the argument is ignored
                // after the first call, which is the point of one world.
                let tiles = crate::tiles::Tiles::prepare(0, WORLD_RADIUS)?;
                let area = tiles.spawn_area();
                let mut out = Vec::with_capacity(area.chunk_count());
                for (cx, cz) in area.chunks() {
                    out.push(
                        tiles
                            .chunk(cx, cz)?
                            .ok_or_else(|| io::Error::other("the spawn tile is not generated"))?,
                    );
                }
                // Stand on the ground rather than at a guessed altitude: a
                // fixed y is either inside a hill or a long fall, and both
                // look like the world failed to load.
                spawn = (8.0, tiles.surface_at(8, 8), 8.0);
                chunks = out;
            } else {
                let ids = crate::terrain::StateIds::load(std::path::Path::new(
                    "data/vanilla/block_props.json",
                ))?;
                let noises = crate::terrain::noise_parameters(std::path::Path::new(
                    "crates/rustmc-worldgen/golden/router_vectors.json",
                ))?;
                let terrain = crate::terrain::Terrain::new(0, &noises, ids)?;
                let mut out = Vec::new();
                // A 3x3 around the origin: enough to stand on, few enough to
                // generate synchronously while a client waits.
                for cz in -1..=1 {
                    for cx in -1..=1 {
                        out.push(terrain.chunk(cx, cz)?);
                    }
                }
                spawn = (8.0, 100.0, 8.0);
                chunks = out;
            }
            // **A player who has been here before joins where they left.**
            //
            // Here rather than in the play loop, and that is the whole of the
            // fix: `enter_world` is the packet that teleports the client, and
            // the play loop runs after it. Setting the server's belief later
            // left the client standing at the spawn while the server thought
            // otherwise — measured by `ci/t2-player-survives-a-restart.sh`,
            // which walked six blocks east, restarted, and came back to the
            // spawn with the correct position sitting in the file the whole
            // time.
            //
            // A parse failure is logged and the spawn is used: a corrupt
            // player file should cost that player their position, not their
            // login.
            //
            // **The chunk set is still the spawn tile's, and that is a known
            // limitation rather than an oversight.** `chunks` was collected
            // above, around the spawn; a player who logged out far from it
            // rejoins into chunks the join packet did not carry and sees void
            // until `Tiles` generates the tile they are standing in — which it
            // does, on the same trigger as walking there. Fixing it properly
            // means choosing the tile from the saved position before the chunk
            // sweep, which is a change to what `serve_one` generates
            // synchronously while a client waits, and is not free.
            let spawn =
                match crate::levelstore::shared().map(|store| store.load_player(profile.uuid)) {
                    Some(Ok(Some(saved))) => {
                        crate::experience::Registry::shared().preload(
                            entity_id,
                            saved.xp_level,
                            saved.xp_progress,
                            saved.xp_total,
                            saved.score,
                        );
                        eprintln!(
                            "{} rejoins at {:?} from players/data",
                            profile.name, saved.pos
                        );
                        (saved.pos[0], saved.pos[1], saved.pos[2])
                    }
                    Some(Err(e)) => {
                        eprintln!("{}: could not read saved player data: {e}", profile.name);
                        spawn
                    }
                    _ => spawn,
                };
            let joined = chunks.len();
            // `PlayerList.sendLevelInfo`'s weather arm, read at the moment
            // the player is placed. Taken with the lock held for exactly the
            // read: the tick thread advances the same state, and a join that
            // straddled a level step would tell the client a rain level no
            // tick ever held.
            let weather_events = crate::weather::shared()
                .lock()
                .map_or_else(|e| e.into_inner().join_events(), |w| w.join_events());
            crate::world_send::enter_world(stream, &join, &chunks, spawn, comp, &weather_events)?;
            // After `enter_world`, not before it, and for the same reason the
            // listening line moved past the bind. This said a player had
            // joined and then did the work of sending them the world; a join
            // that died in `enter_world` — a dropped socket, a chunk that
            // would not encode — left a log claiming it had succeeded, and
            // the only other record of it is an `on_error` line further down
            // naming a peer rather than a player.
            //
            // Nothing greps for this today (checked across `ci/`, `tools/`
            // and `crates/`, 2026-08-13), which is the whole reason to fix it
            // now: `listening on` was also read by nobody, right up until
            // three wait loops were written against it.
            eprintln!("{} joined at {spawn:?}, {joined} chunks", profile.name);
            // The caller takes it from here: it owns the socket and so can
            // give the keep-alive loop the read timeout it needs.
            Ok(InPlay {
                playing: true,
                name: profile.name,
                spawn: Some(spawn),
                player: Some(PlayerIdentity {
                    entity_id,
                    uuid: profile.uuid,
                    view_distance: player_view_distance(
                        i32::from(client.view_distance),
                        info.view_distance,
                    ),
                    chat_visibility: client.chat_visibility,
                    model_customisation: client.model_customisation,
                }),
            })
        }
        Intent::Transfer | Intent::Unknown(_) => crate::login::disconnect(
            stream,
            &format!(
                "rustmc {} accepts status and login",
                rustmc_proto::GAME_VERSION
            ),
            None,
        )
        .map(|()| InPlay {
            playing: false,
            name: String::new(),
            spawn: None,
            player: None,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bind that did not happen is never announced.
    ///
    /// This is the shipped defect, as a test that goes red if it returns.
    /// `main.rs` printed `rustmc … listening on 0.0.0.0:<port>` **before**
    /// calling `listen`, so a server whose bind was refused printed the
    /// readiness line and then `listen: Address already in use`, in that
    /// order. Three harness wait loops grep for the first of those two lines
    /// and broke out of their wait on it, then recorded a whole corpus
    /// against a port this process does not hold — against, in the failure
    /// that names itself, whichever server already had it.
    ///
    /// The property is not "the message is in the right place in the file",
    /// which a refactor would quietly lose. It is **`on_bound` is called zero
    /// times on every path that returns `Err`**, and it is checked by running
    /// the real `listen` against a port something else is holding, which is
    /// exactly the situation the log used to lie about.
    ///
    /// The other direction — that a bind which *did* happen is announced,
    /// with the address the kernel gave — cannot be a unit test, because a
    /// successful `listen` never returns. It is
    /// `ci/readiness-falsify.sh`'s positive control, taken against the real
    /// binary.
    #[test]
    fn a_refused_bind_is_never_announced() {
        let squatter = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("a free port");
        let addr = squatter.local_addr().expect("the squatter is bound");
        let announced = std::sync::atomic::AtomicUsize::new(0);
        let info = StatusInfo::default();
        let r = listen(
            addr,
            &info,
            |_| {
                announced.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            },
            |_, _| {},
        );
        let e = r.expect_err("binding a port another live listener holds must fail");
        assert_eq!(
            e.kind(),
            io::ErrorKind::AddrInUse,
            "expected the address-in-use refusal, got {e}"
        );
        assert_eq!(
            announced.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "listen announced a bind it did not get — the readiness line is a lie again \
             and every proof that greps for it is back to measuring a corpse"
        );
        drop(squatter);
    }

    /// Drive the exchange a client drives, over a pair of in-memory buffers.
    fn client_bytes(next_state: i32, ping: i64) -> Vec<u8> {
        let mut hs = Vec::new();
        rustmc_proto::varint::write_varint(&mut hs, PROTOCOL_VERSION).expect("buf");
        wire::write_string(&mut hs, "localhost").expect("buf");
        wire::write_u16(&mut hs, 25565).expect("buf");
        rustmc_proto::varint::write_varint(&mut hs, next_state).expect("buf");

        let mut out = Vec::new();
        frame::write_frame(&mut out, None, ids::handshake::sb::INTENTION, &hs).expect("buf");
        frame::write_frame(&mut out, None, ids::status::sb::STATUS_REQUEST, &[]).expect("buf");
        let mut pb = Vec::new();
        wire::write_i64(&mut pb, ping).expect("buf");
        frame::write_frame(&mut out, None, ids::status::sb::PING_REQUEST, &pb).expect("buf");
        out
    }

    struct Pipe {
        input: std::io::Cursor<Vec<u8>>,
        output: Vec<u8>,
    }
    impl Read for Pipe {
        fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
            self.input.read(b)
        }
    }
    impl Write for Pipe {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            self.output.write(b)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// The whole exchange, as a client performs it.
    #[test]
    fn a_client_gets_a_status_and_its_ping_back() {
        let ping = 0x0123_4567_89ab_cdefi64;
        let mut pipe = Pipe {
            input: std::io::Cursor::new(client_bytes(1, ping)),
            output: Vec::new(),
        };
        serve_one(&mut pipe, &StatusInfo::default()).expect("served");

        let mut cur = &pipe.output[..];
        let (id, body) = frame::read_frame(&mut cur, false).expect("status response");
        assert_eq!(id, ids::status::cb::STATUS_RESPONSE);
        let json = wire::read_string(&mut &body[..]).expect("json");
        assert!(
            json.contains(rustmc_proto::GAME_VERSION),
            "version name: {json}"
        );
        assert!(
            json.contains(&format!("\"protocol\":{PROTOCOL_VERSION}")),
            "protocol number decides whether a client will connect at all: {json}"
        );

        let (id, body) = frame::read_frame(&mut cur, false).expect("pong");
        assert_eq!(id, ids::status::cb::PONG_RESPONSE);
        // Echoed exactly, not regenerated — the client times its own number.
        assert_eq!(
            wire::read_i64(&mut &body[..]).expect("i64"),
            ping,
            "the pong must carry the client's own number back"
        );
    }

    /// A login now runs to completion and is then told where the work stops,
    /// rather than being hung up on at the handshake.
    #[test]
    fn a_login_completes_then_says_where_the_work_stops() {
        let mut hs = Vec::new();
        rustmc_proto::varint::write_varint(&mut hs, PROTOCOL_VERSION).expect("buf");
        wire::write_string(&mut hs, "localhost").expect("buf");
        wire::write_u16(&mut hs, 25565).expect("buf");
        rustmc_proto::varint::write_varint(&mut hs, 2).expect("buf");

        let mut hello = Vec::new();
        wire::write_string(&mut hello, "Notch").expect("buf");
        wire::write_u128(&mut hello, 0).expect("buf");

        let mut input = Vec::new();
        frame::write_frame(&mut input, None, ids::handshake::sb::INTENTION, &hs).expect("buf");
        frame::write_frame(&mut input, None, ids::login::sb::HELLO, &hello).expect("buf");
        // Compression is negotiated during login, so everything the client
        // sends *after* `hello` is compressed. Writing this frame plainly is
        // what the test did before, and it now fails — which is the point:
        // the fixture speaking the wrong form is exactly the bug a
        // half-threaded implementation would ship.
        frame::write_frame(
            &mut input,
            Some(COMPRESSION_THRESHOLD),
            ids::login::sb::LOGIN_ACKNOWLEDGED,
            &[],
        )
        .expect("buf");

        let mut pipe = Pipe {
            input: std::io::Cursor::new(input),
            output: Vec::new(),
        };
        serve_one(&mut pipe, &StatusInfo::default()).expect("served");

        let mut cur = &pipe.output[..];
        // Uncompressed: it is the packet that turns compression on.
        let (id, body) = frame::read_frame(&mut cur, false).expect("login_compression");
        assert_eq!(
            id,
            ids::login::cb::LOGIN_COMPRESSION,
            "compression is negotiated before login_finished"
        );
        assert_eq!(
            crate::login::peek_varint(&body).expect("threshold"),
            COMPRESSION_THRESHOLD
        );

        // And every frame after it is compressed, including this one.
        let (id, body) = frame::read_frame(&mut cur, true).expect("login_finished");
        assert_eq!(id, ids::login::cb::LOGIN_FINISHED);
        let mut b = &body[..];
        assert_eq!(
            wire::read_u128(&mut b).expect("uuid"),
            crate::login::Profile::offline("Notch").uuid
        );

        // What follows depends on whether the registry capture is present,
        // and the test asserts the part that is true either way: the client
        // is told, in the state it is actually in, why it is stopping.
        //
        // Without a capture that is a *configuration*-state disconnect naming
        // the missing file — not a login-state one, and not a dropped socket.
        // The distinction is load-bearing: the two states encode their
        // disconnect reason differently (JSON against NBT), so a server that
        // sends the wrong one shows the player a parse error instead.
        let (id, _) = frame::read_frame(&mut cur, true).expect("a disconnect follows");
        assert_eq!(
            id,
            ids::configuration::cb::DISCONNECT,
            "login succeeded, so the refusal comes from the configuration state"
        );
    }

    /// A description with a quote in it must not break the JSON.
    #[test]
    fn the_description_is_escaped() {
        let info = StatusInfo {
            description: "a \"quoted\" name\nwith a newline".into(),
            ..StatusInfo::default()
        };
        let json = info.to_json();
        assert!(
            json.contains(r#"a \"quoted\" name\nwith a newline"#),
            "{json}"
        );
        // And the whole thing is still parseable.
        serde_json::from_str::<serde_json::Value>(&json).expect("valid JSON");
    }

    /// `Mth.clamp(requested, 2, serverViewDistance)`, both bounds and the
    /// inversion.
    ///
    /// The middle case is the one that matters on the wire and it is the one
    /// this server got wrong: `oracle-runner` asks for 10 against a server
    /// set to 4, and the answer is 4. It is asserted here as well as in the
    /// corpus because a unit test says the arithmetic is right and
    /// `530-walking-away-drops-a-standing-player-from-view` says the bytes
    /// are, and neither is a substitute for the other.
    #[test]
    fn a_players_view_distance_is_clamped_between_two_and_the_servers() {
        assert_eq!(player_view_distance(10, 4), 4, "the server's ceiling binds");
        assert_eq!(player_view_distance(3, 10), 3, "the client asked for less");
        assert_eq!(player_view_distance(1, 10), 2, "the floor of two binds");
        assert_eq!(player_view_distance(0, 10), 2);
        assert_eq!(player_view_distance(-1, 10), 2, "the byte is signed");
        // `Math.min(Math.max(v, 2), max)` with `max < 2` returns `max`, and
        // Rust's `clamp` would panic here instead.
        assert_eq!(
            player_view_distance(10, 1),
            1,
            "an operator below the floor"
        );
        assert_eq!(player_view_distance(10, 10), 10);
    }

    /// The two names this server can honestly serve, what they refuse, and
    /// the ordinals they travel as.
    ///
    /// # One test, and that is not tidiness
    ///
    /// `game_type()` reads a process-wide static and `cargo test` runs the
    /// crate's tests as threads in ONE process. Split across two tests these
    /// assertions race each other — the split version passed eight times and
    /// would have failed under `--test-threads`; a flaky test in this repo is
    /// worse than no test, because it teaches a reader to re-run rather than
    /// to read. Nothing else in the crate writes this cell, so a single test
    /// that owns it outright has no race to lose.
    ///
    /// The ordinals are `GameType`'s own `$values` order — survival 0,
    /// creative 1 — and they travel raw in `ClientboundLoginPacket`, so a
    /// swap would put every joining player in the other mode with nothing
    /// else in the server disagreeing. The refusals matter for the same
    /// reason in reverse: adventure and spectator need the per-player ability
    /// model that `ci/unported-inventory.py --open` still lists as
    /// `abilities-model`, and quietly serving creative under one of those
    /// names would make a pass recorded under it a measurement of a server
    /// nobody asked for.
    #[test]
    fn the_game_mode_cell_serves_two_names_refuses_the_rest_and_keeps_vanillas_ordinals() {
        assert_eq!(SURVIVAL, 0);
        assert_eq!(CREATIVE, 1);

        assert_eq!(set_game_type("survival"), Ok(SURVIVAL));
        assert_eq!(game_type(), SURVIVAL);
        assert_eq!(set_game_type("creative"), Ok(CREATIVE));
        assert_eq!(game_type(), CREATIVE);

        // Refused, and the mode is left where it was rather than defaulted.
        for bad in ["adventure", "spectator", "Survival", ""] {
            assert_eq!(set_game_type(bad), Err(bad.to_string()));
            assert_eq!(game_type(), CREATIVE, "`{bad}` moved the mode");
        }

        // Put it back, so the cell is left as `main` would leave it unflagged.
        set_game_type("creative").expect("creative is servable");
    }

    /// The default is creative, because 88 recordings and every baseline row
    /// in `ci/checks/gameplay-baseline.toml` were taken under it.
    #[test]
    fn the_default_game_type_is_the_one_the_corpus_was_measured_under() {
        assert_eq!(DEFAULT_GAME_TYPE, CREATIVE);
    }
}
