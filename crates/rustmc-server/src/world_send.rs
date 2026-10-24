//! Getting a client into the world: the play-state send loop.
//!
//! Reference (26.2): the packet order a real server uses after
//! `finish_configuration`, ids from [`rustmc_proto::ids`].
//!
//! ```text
//!   S->C  play/login                 the join packet
//!   S->C  game_event 13              "start waiting for level chunks"
//!   S->C  set_chunk_cache_center     which chunk the client should keep
//!   S->C  chunk_batch_start
//!   S->C  level_chunk_with_light     one per chunk
//!   S->C  chunk_batch_finished       how many were in the batch
//!   S->C  player_position            a teleport, with an id to acknowledge
//! ```
//!
//! # The order is not decorative
//!
//! A client that receives chunks before `game_event 13` discards them and
//! sits on the loading screen: the event is what puts it into the state where
//! chunk packets mean anything. And `player_position` must come *after* the
//! chunks covering that position, or the client falls through a world that
//! has not arrived and is teleported back, repeatedly.
//!
//! # What this sends
//!
//! A flat world — stone to y = 63, air above — built here rather than taken
//! from the generator. That is a deliberate seam and it is worth being clear
//! about: this module proves the **transport**, that a client can be brought
//! from a TCP connect to standing on ground. Feeding it
//! `rustmc-worldgen`'s output is the next piece and a separate one, because
//! it needs the chunk pipeline that currently only exists inside the
//! whole-world dump tool.
//!
//! The blocks are named through [`rustmc_data`]'s state ids so that the seam
//! is the *source of the blocks* and not the encoding of them.

use std::io::{self, Read, Write};

use rustmc_proto::{frame, ids, varint::write_varint, wire};

use crate::chunk::{
    ContainerKind, Heightmap, LevelChunkWithLight, LightData, PalettedContainer, Section,
};

/// How many 16-block sections an overworld chunk has: −64 to 320.
pub const OVERWORLD_SECTIONS: usize = 24;

/// A block state id, as the network numbers them.
///
/// Not a name: the wire carries the *global palette index*, and the mapping
/// lives in the extracted block properties. Passed in rather than looked up
/// here so this module has no opinion about where the registry comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateId(pub i32);

/// Build one flat chunk: `floor` below `height`, `air` above.
#[must_use]
pub fn flat_chunk(
    chunk_x: i32,
    chunk_z: i32,
    floor: StateId,
    air: StateId,
    height: i32,
    plains_biome: i32,
) -> LevelChunkWithLight {
    let mut sections = Vec::with_capacity(OVERWORLD_SECTIONS);
    for s in 0..OVERWORLD_SECTIONS {
        // Section `s` covers y in [-64 + s*16, -64 + s*16 + 16).
        let base = -64 + (s as i32) * 16;
        let mut values = vec![air.0; 4096];
        let mut solid = 0i16;
        for y in 0..16 {
            if base + y < height {
                for i in 0..256 {
                    values[y as usize * 256 + i] = floor.0;
                }
                solid += 256;
            }
        }
        sections.push(Section {
            non_empty_blocks: solid,
            // No fluids. The field exists whether or not it is used, and
            // omitting it is what desyncs the buffer — see `chunk::Section`.
            fluid_count: 0,
            states: PalettedContainer::from_values(ContainerKind::BlockStates, &values),
            biomes: PalettedContainer::from_values(ContainerKind::Biomes, &[plains_biome; 64]),
        });
    }

    LevelChunkWithLight {
        chunk_x,
        chunk_z,
        // The three heightmaps with `sendToClient`. A flat world's surface is
        // the same everywhere, and nine bits a column, seven to a long, is 37
        // longs — the same shape every capture shows.
        heightmaps: [1, 4, 5]
            .into_iter()
            .map(|kind| Heightmap {
                kind,
                data: packed_heightmap(height + 64),
            })
            .collect(),
        sections,
        // Stone below, air above, and therefore genuinely no block entities
        // — not a stub. `flat_chunk` is reached only from its own two unit
        // tests; the live paths are `World::chunk` (which builds a real list,
        // PARITY-ROADMAP §32) and `Terrain::chunk`. See §70.
        block_entities: Vec::new(),
        // No light: every mask empty. A client lights an unlit chunk itself
        // rather than refusing it, which is the whole reason this is a
        // separable problem.
        light: LightData::default(),
    }
}

/// 256 columns of the same value, nine bits each, seven to a long.
fn packed_heightmap(value: i32) -> Vec<i64> {
    let v = i64::from(value) & 0x1ff;
    let mut out = Vec::with_capacity(37);
    let mut col = 0;
    while col < 256 {
        let mut word = 0i64;
        for slot in 0..7 {
            if col >= 256 {
                break;
            }
            word |= v << (slot * 9);
            col += 1;
        }
        out.push(word);
    }
    out
}

/// Bring a client from `finish_configuration` to standing on ground.
///
/// `weather` is [`crate::weather::Weather::join_events`] — the three
/// `game_event`s `PlayerList.sendLevelInfo` sends a player joining a raining
/// world, and empty otherwise. They go out **before**
/// `LEVEL_CHUNKS_LOAD_START`, which is where they sit in that method
/// (offsets 55-124 against 124).
///
/// # Errors
/// On I/O failure.
/// The teleport id the join sequence's `player_position` carries.
///
/// A named constant because two files have to agree on it: this one writes it
/// to the wire and `crate::status` arms `PlayerState::awaiting_teleport` with
/// it. A disagreement is silent and total — every acknowledgement fails the
/// `if_icmpne` at `handleAcceptTeleportPacket` offset 20, so the awaiting
/// position is never cleared and the player can never move.
pub const JOIN_TELEPORT_ID: i32 = 1;

pub fn enter_world(
    stream: &mut (impl Read + Write),
    join: &crate::play::JoinInfo,
    chunks: &[LevelChunkWithLight],
    spawn: (f64, f64, f64),
    comp: Option<i32>,
    weather: &[(crate::weather::Event, f32)],
) -> io::Result<()> {
    join.send(stream, comp)?;

    // `PlayerList.sendLevelInfo` — the world border, and it is FIRST in that
    // method, ahead of the full time sync, the default spawn position, the
    // weather events and the `LEVEL_CHUNKS_LOAD_START` game event
    // (`sendLevelInfo` offsets 9, 31, 41, 66..118, 128 in the pinned jar).
    // Of the four in between this server sends only the weather events at
    // join, so vanilla's own ordering places `initialize_border` exactly here:
    // after the login packet and before the weather arm and the game event
    // below.
    //
    // A client that never receives one draws no border at all and applies no
    // clamp, which is indistinguishable from a border at its default size —
    // and that is why this is a wiring commit with a byte comparison rather
    // than a call added and eyeballed. `tests::the_join_border_is_vanillas`
    // holds the 39 bytes vanilla's own `STREAM_CODEC` produced for a fresh
    // border.
    {
        let mut body = Vec::new();
        crate::worldborder::packets::InitializeBorder::of(
            &crate::worldborder::shared().lock().unwrap_or_else(|e| {
                // A border mutator that panicked would otherwise take every
                // later join down with it. `into_inner` keeps the last
                // consistent state, which for this lock is a whole extent.
                e.into_inner()
            }),
        )
        .write(&mut body)?;
        frame::write_frame(stream, comp, ids::play::cb::INITIALIZE_BORDER, &body)?;
    }

    // `PlayerList.sendLevelInfo`'s weather arm. Nothing is sent to a player
    // joining a dry world — no `STOP_RAINING`, no zero levels — because a
    // fresh client is already dry.
    for &(event, param) in weather {
        let mut ev = Vec::new();
        wire::write_u8(&mut ev, event as u8)?;
        wire::write_f32(&mut ev, param)?;
        frame::write_frame(stream, comp, ids::play::cb::GAME_EVENT, &ev)?;
    }

    // `GameEvent 13` — START_WAITING_FOR_LEVEL_CHUNKS. Sent *before* the
    // chunks: a client that gets chunks first discards them and never leaves
    // the loading screen.
    let mut ev = Vec::new();
    wire::write_u8(&mut ev, 13)?;
    wire::write_f32(&mut ev, 0.0)?;
    frame::write_frame(stream, comp, ids::play::cb::GAME_EVENT, &ev)?;

    let mut centre = Vec::new();
    write_varint(&mut centre, spawn.0 as i32 >> 4)?;
    write_varint(&mut centre, spawn.2 as i32 >> 4)?;
    frame::write_frame(stream, comp, ids::play::cb::SET_CHUNK_CACHE_CENTER, &centre)?;

    frame::write_frame(stream, comp, ids::play::cb::CHUNK_BATCH_START, &[])?;
    for c in chunks {
        frame::write_frame(
            stream,
            comp,
            ids::play::cb::LEVEL_CHUNK_WITH_LIGHT,
            &c.encode()?,
        )?;
    }
    let mut fin = Vec::new();
    write_varint(&mut fin, i32::try_from(chunks.len()).unwrap_or(i32::MAX))?;
    frame::write_frame(stream, comp, ids::play::cb::CHUNK_BATCH_FINISHED, &fin)?;

    // The teleport comes last, after the chunks covering it. Otherwise the
    // client falls through a world that has not arrived and gets teleported
    // back, over and over.
    let pos = player_position_body(JOIN_TELEPORT_ID, spawn, 0.0, 0.0)?;
    frame::write_frame(stream, comp, ids::play::cb::PLAYER_POSITION, &pos)?;
    Ok(())
}

/// Move a client from one dimension to another: the packets vanilla sends, in
/// vanilla's order, for the ones this server has.
///
/// # The order, from `ServerPlayer.teleport(TeleportTransition)`
///
/// The whole clientbound sequence of the dimension-changing branch, by
/// bytecode offset. It is not the join sequence rearranged — two things sit in
/// the opposite order from the one `enter_world` uses:
///
/// ```text
///    16  game_event NO_RESPAWN_BLOCK_AVAILABLE   only if missingRespawnBlock
///   115  respawn(createCommonSpawnInfo(newLevel), 3)      <- FIRST
///   135  change_difficulty(getDifficulty, isDifficultyLocked)
///   172  PlayerList.sendPlayerPermissionLevel             -> entity_event
///   253  connection.teleport(PositionMoveRotation.of(t), t.relatives())
///   272  connection.resetPosition()
///   296  player_abilities
///   314  PlayerList.sendLevelInfo(player, newLevel)       <- the border, LAST
///   321  PlayerList.sendAllPlayerInfo
///   327  PlayerList.sendActivePlayerEffects
///   334  postTeleportTransition.onTransition(entity)
/// ```
///
/// **`sendLevelInfo` is at 314 here and at the very start of a join.** At join
/// `PlayerList.placeNewPlayer` calls it before the game event; on a dimension
/// change the border arrives *after* the teleport. A wiring that reused the
/// join order would put the border ahead of the respawn, and a client applies
/// a border to the level it thinks it is in.
///
/// `dataToKeep` is **3** — `iconst_3` at offset 128, i.e.
/// [`crate::play::KEEP_ALL_DATA`] — on every dimension change, so a
/// travelling player keeps both their attribute modifiers and their entity
/// data.
///
/// # What is here and what is refused
///
/// Sent: the `respawn`, the `START_WAITING_FOR_LEVEL_CHUNKS` game event, the
/// chunk batch for the new dimension, the chunk-cache centre, the teleport,
/// and the new dimension's border.
///
/// **Not sent, each because this server has no id for the packet rather than
/// because anything decided it was unimportant:** `change_difficulty` (offset
/// 135), `entity_event` for the permission level (172), `player_abilities`
/// (296), `player_info_update` (321) and the effects (327). Adding any of them
/// is one constant in `crates/rustmc-proto/src/ids.rs` and a body; naming them
/// here rather than leaving the sequence looking complete is the point.
///
/// The `game_event` + batch + teleport tail is `enter_world`'s, and is here
/// rather than shared with it because the two sequences differ in the border's
/// position and sharing them would have to be parameterised by the thing that
/// differs.
///
/// # Errors
/// On I/O failure.
pub fn change_dimension(
    stream: &mut impl Write,
    to: &crate::play::JoinInfo,
    chunks: &[LevelChunkWithLight],
    arrive: (f64, f64, f64),
    rotation: (f32, f32),
    comp: Option<i32>,
) -> io::Result<()> {
    // 115: the respawn, first, with KEEP_ALL_DATA.
    to.send_respawn(stream, comp, crate::play::KEEP_ALL_DATA)?;

    // The client throws away its whole level on a respawn, so it needs the
    // same "chunks are coming" event a join gets before any arrive.
    let mut ev = Vec::new();
    wire::write_u8(&mut ev, 13)?;
    wire::write_f32(&mut ev, 0.0)?;
    frame::write_frame(stream, comp, ids::play::cb::GAME_EVENT, &ev)?;

    let mut centre = Vec::new();
    write_varint(&mut centre, arrive.0 as i32 >> 4)?;
    write_varint(&mut centre, arrive.2 as i32 >> 4)?;
    frame::write_frame(stream, comp, ids::play::cb::SET_CHUNK_CACHE_CENTER, &centre)?;

    frame::write_frame(stream, comp, ids::play::cb::CHUNK_BATCH_START, &[])?;
    for c in chunks {
        frame::write_frame(
            stream,
            comp,
            ids::play::cb::LEVEL_CHUNK_WITH_LIGHT,
            &c.encode()?,
        )?;
    }
    let mut fin = Vec::new();
    write_varint(&mut fin, i32::try_from(chunks.len()).unwrap_or(i32::MAX))?;
    frame::write_frame(stream, comp, ids::play::cb::CHUNK_BATCH_FINISHED, &fin)?;

    // 253: the teleport, after the chunks that cover it — same argument as
    // `enter_world`'s, and the same reason it is not first.
    let mut pos = Vec::new();
    write_varint(&mut pos, 1)?;
    wire::write_f64(&mut pos, arrive.0)?;
    wire::write_f64(&mut pos, arrive.1)?;
    wire::write_f64(&mut pos, arrive.2)?;
    for _ in 0..3 {
        wire::write_f64(&mut pos, 0.0)?;
    }
    wire::write_f32(&mut pos, rotation.0)?;
    wire::write_f32(&mut pos, rotation.1)?;
    wire::write_i32(&mut pos, 0)?;
    frame::write_frame(stream, comp, ids::play::cb::PLAYER_POSITION, &pos)?;

    // 314: `sendLevelInfo`, and its border, LAST — not first as at join.
    {
        let mut body = Vec::new();
        crate::worldborder::packets::InitializeBorder::of(
            &crate::worldborder::shared()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
        .write(&mut body)?;
        frame::write_frame(stream, comp, ids::play::cb::INITIALIZE_BORDER, &body)?;
    }
    Ok(())
}

/// Everything one connection's play loop works on.
///
/// A struct rather than eight arguments, and the grouping is not cosmetic:
/// half of these appeared together the moment a block could change, and a
/// call site that can pass the wrong player's `ChunkView` alongside the right
/// player's `Connection` is a bug waiting for a second player to find it.
pub struct Session<'a> {
    /// Where this connection's packets go. Every write after the join goes
    /// through it, because another player's thread can now write here too.
    pub conn: &'a crate::players::Connection,
    pub player: &'a mut crate::player::PlayerState,
    pub clock: &'a crate::tick::GameClock,
    /// `world_clock` registry ids, from the captured registries. Empty means
    /// the client's time advances and its sun does not move — an id only
    /// means anything against the ordering the client itself received.
    pub clock_ids: &'a [i32],
    /// Everyone else, for the packets that are not this player's business
    /// alone.
    pub players: &'a crate::players::Players,
    /// The world this connection may change, when there is one. `None` is
    /// the noise-stage fallback: a player there can walk but not build.
    pub blocks: Option<&'a dyn crate::edits::Blocks>,
    /// What the player is holding, which is the half of a placement that
    /// `use_item_on` does not carry.
    pub hotbar: &'a mut crate::player::Hotbar,
    /// `ServerPlayer.containerCounter` and `containerMenu`. Beside the
    /// hotbar because it has the same lifetime and the same reason to
    /// exist: state vanilla keeps on `ServerPlayer` that this server has
    /// nowhere else to put.
    pub menu: &'a mut crate::menu::MenuState,
    /// The item registry the creative-slot packet's ids resolve against.
    /// `None` means a player can dig but not build — which is what a
    /// checkout without `data/vanilla/items.json` gets, said out loud rather
    /// than shown as placements that quietly do nothing.
    pub items: Option<&'a crate::player::ItemIds>,
    /// The crafting recipes. `None` means a crafting table opens and its
    /// result slot never fills — see [`crate::recipes`].
    pub recipes: Option<&'a crate::recipes::Recipes>,
    /// Item -> recipes it unlocks, and each recipe's captured
    /// `recipe_book_add` entry bytes — see [`crate::recipe_book`]. `None`
    /// means holding an item never adds anything to the client's recipe
    /// book, which is what this server did before the table existed.
    pub recipe_unlocks: Option<&'a crate::recipe_book::RecipeUnlocks>,
    /// `ServerRecipeBook.known` for this connection: recipe ids already sent
    /// to this client, so a second furnace sends nothing.
    pub known_recipes: &'a mut std::collections::BTreeSet<String>,
    /// This connection's `PlayerAdvancements`. `None` means no advancement is
    /// ever awarded and no `update_advancements` packet is ever sent, which
    /// is what this server did before `crate::advancements` existed.
    pub advancements: Option<&'a mut crate::advancements::player::PlayerAdvancements<'static>>,
    /// The tables that state reads. `None` alongside a `Some` above is not a
    /// reachable pair — [`crate::advancements::Engine`] produces both — but
    /// the types keep them separate because the engine is `Copy` and the
    /// state is not.
    pub adv_engine: Option<crate::advancements::Engine>,
    /// `getStateForPlacement` per block, extracted from the jar. `None` means
    /// every placement takes the block's default state, which is uniformly
    /// unfinished rather than wrong — a stair will not face the placer, and
    /// no block will face the wrong way.
    pub placement: Option<&'a crate::placement::PlacementTable>,
    /// `useWithoutItem` per state, extracted from the jar. `None` means every
    /// block declines a right-click, so the placement path always runs —
    /// which is an inversion of vanilla's order rather than an omission of
    /// it, and is said out loud rather than shown as a lever that takes a
    /// block instead of flipping.
    pub uses: Option<&'a crate::useblock::UseTable>,
    /// `useItemOn` per state and held item, extracted from the jar, for the
    /// classes [`crate::useitemon::PORTED_CLASSES`] names. `None` means every
    /// block answers `TryEmptyHandInteraction` — which is
    /// `BlockBehaviour`'s own default and exactly what this server did before
    /// the table existed, so its absence costs the item-aware branch and
    /// nothing else: a flower pot will not fill, and will therefore never
    /// empty either.
    pub item_uses: Option<&'a crate::useitemon::UseItemOnTable>,
    /// Which `canSurvive` rule each of the 1,115 blocks a `BlockItem` names
    /// reaches, extracted from the jar. `None` means `BlockItem.canPlace`'s
    /// survive gate is not asked at all and every placement is allowed —
    /// which is what this server did before the table existed, and is said
    /// out loud rather than shown as a torch that hangs on air.
    pub survive: Option<&'a crate::itemsurvive::SurviveClasses>,
    /// Who `BoneMealItem.useOn` can grow. `None` means bone meal is inert —
    /// which is what this server did before the table existed, and is said
    /// out loud rather than shown as a crop that will not grow.
    pub bonemeal: Option<&'a crate::bonemeal::BonemealTable>,
    /// `setPlacedBy` per state, extracted from the jar. `None` means a
    /// placement writes exactly the one block `getStateForPlacement`
    /// described, so a door has no top and a bed no head — which is what
    /// this server did before the table existed and is said out loud rather
    /// than shown as half a door.
    pub placed_by: Option<&'a crate::placedby::PlacedByTable>,
    /// `updateShape` per state and direction, extracted from the jar. `None`
    /// means nothing cascades: a torch keeps hanging on the air where its
    /// support used to be, which is what this server did before the table
    /// existed and is said out loud rather than left to be discovered.
    pub shapes: Option<&'a crate::shapes::ShapeTable>,
    /// The scheduled-tick queue the clock thread drains. `None` means a
    /// placement that would ask to be woken later is simply never woken —
    /// a coral put down out of water stays alive for ever.
    pub ticks: Option<&'a crate::ticks::ScheduledTicks>,
    /// `FireBlock.igniteOdds` / `burnOdds`, extracted from the jar. `None`
    /// means a flint-and-steel refuses rather than guessing a fire's
    /// connection booleans — see `crate::itemuse::can_be_placed_at`.
    pub fire_odds: Option<&'a crate::fire::FireOdds>,
    /// The tick's worth of block changes this session records into. Not a
    /// global: `cargo test` runs a module's tests on parallel threads in one
    /// process, and a process-wide buffer would let one test's placement land
    /// in another's flush — the same shape as the shared temp path that made
    /// `player::tests` fail 21 runs in 40.
    pub changed: &'a crate::broadcast::ChangedBlocks,
    /// `Registries.MENU` and `Registries.BLOCK` protocol ids. `None` means
    /// no container can be opened and no block event can be addressed —
    /// which is the right answer for a session with no extract, and is
    /// printed once rather than defaulted, because an id defaulted to 0
    /// would open a `generic_9x1` for every container and name `air` in
    /// every block event.
    pub protocol_ids: Option<&'a crate::protocol_ids::ProtocolIds>,
    /// The tick's worth of block events. Same reason as `changed`: a
    /// process-wide queue would let one test's chest lid land in another
    /// test's flush.
    pub block_events: &'a crate::containers::BlockEvents,
    /// The per-position `ContainerOpenersCounter`s.
    pub openers: &'a crate::containers::Openers,
    pub chunk_at: &'a dyn Fn(i32, i32) -> io::Result<Option<LevelChunkWithLight>>,
    /// The plugins this server loaded, or `None` for a session with no
    /// plugin host at all.
    ///
    /// A borrow rather than a global, for the same reason `changed` is one:
    /// `cargo test` runs a module's tests on parallel threads in one process,
    /// and a process-wide plugin registry would let one test's plugin deny
    /// another test's dig. Production passes [`crate::plugins::global`], which
    /// is installed once at boot; a test passes its own
    /// [`crate::plugins::Runtime`].
    ///
    /// `None` and "installed with no plugins" are deliberately different
    /// values and both cost the same on the dig path — one branch — because
    /// the alternative is a server that boots without a plugin host having a
    /// *different* code path from one that boots with an empty one, which is
    /// two things to keep behaving identically instead of one.
    pub plugins: Option<&'a crate::plugins::Runtime>,
    /// `SoundType` per block state, extracted from the jar. `None` means a
    /// placed block makes no sound and a walk is silent — which is what this
    /// server did before the table existed, and is said out loud rather than
    /// shown as a world where nothing you build can be heard.
    pub sound_types: Option<&'a crate::sound_types::SoundTypes>,
    /// `SoundEvent.fixedRange()` per registry entry — how far each sound
    /// carries. `None` means every sound takes the 16-block default, which is
    /// the right answer for every event this server emits today and the wrong
    /// one for the first loud event it emits tomorrow.
    pub sound_events: Option<&'a crate::sound_events::SoundEventTable>,
    /// The locator-bar connections. A borrow rather than a global for the
    /// reason `changed` is one: `cargo test` runs a module's tests on
    /// parallel threads in one process, and a process-wide manager would let
    /// one test's join create a connection another test then sees a move on.
    /// Production passes [`crate::waypoints::Waypoints::shared`].
    pub waypoints: &'a crate::waypoints::Waypoints,
    /// The world seed, for `/seed`.
    ///
    /// `SeedCommand` reads `CommandSourceStack.getLevel().getSeed()`, which
    /// is the **real** seed — not the hashed one `crate::play`'s `JoinInfo`
    /// sends to the client. The two are different numbers and only one of
    /// them is the answer to `/seed`.
    pub seed: i64,
    /// Whether the join packet put this player in creative.
    ///
    /// It decides *which* dig packet destroys the block, and the two are not
    /// interchangeable: a creative client destroys on `START_DESTROY_BLOCK`
    /// and never sends `STOP`, a survival client sends `START` when it begins
    /// mining and `STOP` when its own progress reaches one. Acting on both
    /// would make survival mining instant; acting on neither, depending on
    /// the mode, does nothing at all.
    pub creative: bool,
}

/// `ClientboundPlayerPositionPacket.of(id, PositionMoveRotation, Set)`.
///
/// `handleMovePlayer`'s correction teleport is
/// `teleport(x, y, z, yRot, xRot)`, which builds a
/// `PositionMoveRotation(new Vec3(x, y, z), Vec3.ZERO, yRot, xRot)` and an
/// **empty** relative set — offsets 0-32 of `teleport(DDDFF)`. So the delta
/// movement is three zeros and the relative bitmask is zero: every field is
/// absolute, and a correction that left the client's velocity alone would
/// rubber-band it straight back out again.
///
/// # Errors
/// Only from the in-memory writer, which cannot fail.
fn player_position_body(
    id: i32,
    pos: (f64, f64, f64),
    yaw: f32,
    pitch: f32,
) -> io::Result<Vec<u8>> {
    let mut b = Vec::new();
    write_varint(&mut b, id)?;
    wire::write_f64(&mut b, pos.0)?;
    wire::write_f64(&mut b, pos.1)?;
    wire::write_f64(&mut b, pos.2)?;
    for _ in 0..3 {
        wire::write_f64(&mut b, 0.0)?; // `Vec3.ZERO` delta movement
    }
    wire::write_f32(&mut b, yaw)?;
    wire::write_f32(&mut b, pitch)?;
    wire::write_i32(&mut b, 0)?; // `Collections.emptySet()` — all absolute
    Ok(b)
}

/// `ClientboundSetHealthPacket`'s wire body — `write` offsets 0..23.
///
/// `writeFloat(health)`, `writeVarInt(food)`, `writeFloat(saturation)`. The
/// **constructor** takes `(F, I, F)` in that same order, which is a
/// coincidence and not a rule: `ClientboundSetExperiencePacket`'s does not.
fn set_health_body(h: crate::playersync::SetHealth) -> io::Result<Vec<u8>> {
    let mut b = Vec::new();
    wire::write_f32(&mut b, h.health)?;
    write_varint(&mut b, h.food)?;
    wire::write_f32(&mut b, h.saturation)?;
    Ok(b)
}

/// `ClientboundSetExperiencePacket`'s wire body — `write` offsets 0..23.
///
/// **`writeFloat(progress)`, `writeVarInt(LEVEL)`, `writeVarInt(TOTAL)`** —
/// while the constructor is `(progress, total, level)`. The two orders differ
/// and writing the packet from the constructor's is one of the field-order
/// traps `docs/SERVER.md` names for the join packet.
/// [`crate::playersync::SetExperience`] carries the constructor's order
/// because that is the order `doTick` builds it in; this function is where it
/// is reordered, once.
fn set_experience_body(x: crate::playersync::SetExperience) -> io::Result<Vec<u8>> {
    let mut b = Vec::new();
    wire::write_f32(&mut b, x.progress)?;
    write_varint(&mut b, x.level)?;
    write_varint(&mut b, x.total)?;
    Ok(b)
}

/// `ClientboundEntityEventPacket`'s wire body — `write` offsets 0..14.
///
/// `writeInt(entityId)` — a **raw big-endian int, not a varint** — then one
/// byte. Every other entity-addressed packet in this file writes a varint id,
/// which is exactly why this one is a function with the offsets on it.
fn entity_event_body(entity_id: i32, event: u8) -> io::Result<Vec<u8>> {
    let mut b = Vec::new();
    wire::write_i32(&mut b, entity_id)?;
    wire::write_u8(&mut b, event)?;
    Ok(b)
}

/// `Block.fallOn`'s default arm — `entity.causeFallDamage(fallDistance, 1.0f,
/// damageSources().fall())` — reached from `Entity.checkFallDamage` offsets
/// 25..98 whenever a landing had a positive fall distance.
///
/// **The damage is `crate::playermove`'s, not this function's.** That module
/// already reads `safe_fall_distance` and `fall_damage_multiplier` off
/// `data/vanilla/attributes.json`; recomputing it here from
/// [`crate::falldamage`] would be a second answer to one question. The two
/// transcriptions are checked against each other in
/// `crates/rustmc-server/tests/fall_damage_agreement.rs` rather than merged,
/// because two independent readings of the same bytecode agreeing is worth
/// more than one reading used twice.
///
/// Nothing is sent from here. `hurtServer` moves the health and the next
/// `doTick` decides whether the client is told — which is vanilla's own
/// one-tick lag and not an artefact of this shape.
fn apply_landing(
    s: &mut Session<'_>,
    live: &mut crate::playertick::LivePlayer,
    mv: &mut crate::playermove::PlayerMovement,
    landing: crate::fall::Landing,
    facts: crate::playertick::LevelFacts,
) -> io::Result<()> {
    if !landing.hit_ground || landing.distance <= 0.0 {
        return Ok(());
    }
    let damage = mv.fall_damage(landing);
    if damage <= 0 {
        return Ok(());
    }
    let Some(ty) =
        crate::damagecmd::registry().and_then(|r| r.get(crate::falldamage::fall_damage_type_id()))
    else {
        live.refuse_hurt();
        return Ok(());
    };
    // `LivingEntity.causeFallDamage` offsets 83..112, in order: the fall
    // sound, the block's fall sound, THEN the hurt — so both sounds are on the
    // wire ahead of the `damage_event`, as the first recording shows them.
    fall_sounds(s, mv.position(), damage);
    #[expect(
        clippy::cast_precision_loss,
        reason = "vanilla's own i2f at LivingEntity.causeFallDamage offset 107"
    )]
    let amount = damage as f32;
    // `Entity.hurt(source, amount)` -> `hurtServer`: the whole chain, the
    // Player layers included, as a console hit takes it.
    let Some(outcome) = live.hurt_player(
        amount,
        ty,
        false,
        false,
        facts,
        crate::gamerules::GameRules::shared(),
    ) else {
        return Ok(());
    };
    if outcome.refused.is_some() {
        return Ok(());
    }
    hurt_level_half(
        s,
        live,
        Some(mv),
        ty,
        crate::falldamage::fall_damage_type_id(),
        &outcome,
        None,
        None,
        None,
    )
}

/// `LivingEntity.causeFallDamage`'s two sounds.
///
/// ```text
///    94  playSound(getFallDamageSound(dmg), 1.0f, 1.0f)
///          getFallDamageSound: dmg > 4 ? fallSounds.big() : fallSounds.small()
///          Player.getFallSounds: (PLAYER_SMALL_FALL, PLAYER_BIG_FALL)
///   103  playBlockFallSound()
///          the state at (floor x, floor(y - 0.2), floor z); if not air:
///          playSound(soundType.getFallSound(), volume * 0.5f, pitch * 0.75f)
/// ```
///
/// Both go through `Player.playSound`, which excludes the player itself.
fn fall_sounds(s: &Session<'_>, at: (f64, f64, f64), damage: i32) {
    // The body's position after the landing move — `s.player` still holds
    // the one before it at this point of `handleMovePlayer`.
    let (x, y, z) = at;
    let mut sounds = vec![crate::sounds::Sound {
        event: if damage > 4 {
            "minecraft:entity.player.big_fall".to_string()
        } else {
            "minecraft:entity.player.small_fall".to_string()
        },
        source: crate::sounds::SOURCE_PLAYERS,
        at: crate::sounds::Eighths::exact(x, y, z),
        pos: (x, y, z),
        volume: 1.0,
        pitch: 1.0,
        except: Some(s.conn.id()),
    }];
    if let (Some(blocks), Some(types)) = (s.blocks, s.sound_types) {
        let under = crate::playerdeath::block_position(x, y - 0.200_000_002_980_232_24, z);
        let state = blocks.state_at(under);
        let is_air = matches!(
            state.split('[').next(),
            Some("minecraft:air" | "minecraft:cave_air" | "minecraft:void_air")
        );
        if !is_air {
            if let Some(row) = blocks
                .state_id(&state)
                .and_then(|id| u32::try_from(id).ok())
                .and_then(|id| types.row(id))
            {
                sounds.push(crate::sounds::Sound {
                    event: row.fall_sound.clone(),
                    source: crate::sounds::SOURCE_PLAYERS,
                    at: crate::sounds::Eighths::exact(x, y, z),
                    pos: (x, y, z),
                    volume: row.volume * 0.5,
                    pitch: row.pitch * 0.75,
                    except: Some(s.conn.id()),
                });
            }
        }
    }
    crate::sounds::send(
        s.players,
        s.protocol_ids,
        s.sound_events,
        Some(crate::entities::Entities::shared()),
        &sounds,
    );
}

/// One console `/damage` against this session's player:
/// `ServerPlayer.hurtServer` through `LivingEntity.hurtServer`, and then what
/// the level does with the outcome.
///
/// ```text
///   LivingEntity.hurtServer, the level half (offsets 285..423, all under fullHit)
///     324  level.broadcastDamageEvent(this, source)     -> damage_event, tracking + self
///     349  markHurt()                                   -> set_entity_motion next tick
///     367  dealDefaultKnockback(source, damage, blocked)
///             direction = sourcePosition - position (x, z)
///             knockback(0.4, dx, dz)                    -> velocity, needsSync
///             indicateDamage(dx, dz)                    -> hurt_animation, self only
///     392  died:  makeSound(getDeathSound()) + die(source)
///     418  else:  playHurtSound(source)                 -> sound, everyone but self
/// ```
///
/// `broadcastDamageEvent` is `ServerChunkCache.sendToTrackingPlayersAndSelf`;
/// the sound is `Player.playSound`, which passes the player as the excluded
/// listener because the hurt client plays its own.
///
/// **The death arm is refused, not faked.** `ServerPlayer.die` is
/// `crate::death`'s decision half and nothing here wires it to a live
/// session; a lethal console hit moves health to zero, sends what precedes
/// `die`, and says on stdout that the rest did not happen.
fn apply_console_damage(
    s: &mut Session<'_>,
    live: &mut crate::playertick::LivePlayer,
    body: Option<&mut crate::playermove::PlayerMovement>,
    hit: &crate::damagecmd::Pending,
    facts: crate::playertick::LevelFacts,
) -> io::Result<()> {
    use crate::damagecmd::{self, Source};
    let Some(ty) = damagecmd::registry().and_then(|r| r.get(&hit.damage_type)) else {
        eprintln!("console: damage — no damage type {}", hit.damage_type);
        return Ok(());
    };
    let (direct, causing, at) = match hit.source {
        Source::Plain => (None, None, None),
        Source::At(p) => (None, None, Some(p)),
        Source::By { direct, causing } => (Some(direct), Some(causing), None),
    };
    // Every entity the console can name is a player, so a causing entity is a
    // player and never a living non-player.
    let Some(outcome) = live.hurt_player(
        hit.amount,
        ty,
        causing.is_some(),
        false,
        facts,
        crate::gamerules::GameRules::shared(),
    ) else {
        // `DamageCommand.damage` offset 27: ERROR_INVULNERABLE, to the
        // console and to operators — never to a client.
        println!(
            "console: damage — commands.damage.invulnerable ({})",
            hit.damage_type
        );
        return Ok(());
    };
    if outcome.refused.is_some() {
        println!(
            "console: damage — commands.damage.invulnerable ({})",
            hit.damage_type
        );
        return Ok(());
    }
    // `hurtServer` offsets 274..282: `resolvePlayerResponsibleForDamage` —
    // a hit whose causing entity is a player remembers them for 100 ticks,
    // which is what `getKillCredit` reads when a later death has no entity.
    if let Some(by) = causing {
        if let Some(n) = player_name_of(s.players, by) {
            live.last_hurt_by_player = Some((by, n));
            live.last_hurt_by_player_memory = 100;
        }
    }
    println!(
        "console: damage — commands.damage.success {} {}",
        hit.amount,
        s.conn.name()
    );
    hurt_level_half(
        s,
        live,
        body,
        ty,
        &hit.damage_type,
        &outcome,
        causing,
        direct,
        at,
    )
}

/// What the level does with a `hurtServer` outcome — everything after the
/// health moved: the broadcast, the impulse, the knockback, the sounds and
/// the death.
///
/// Shared by the console's `/damage` and a fall, because vanilla's two routes
/// meet in the same `LivingEntity.hurtServer` and diverge again only above
/// it.
#[expect(
    clippy::too_many_arguments,
    reason = "one argument per part of the DamageSource and the outcome it produced"
)]
fn hurt_level_half(
    s: &mut Session<'_>,
    live: &mut crate::playertick::LivePlayer,
    mut body: Option<&mut crate::playermove::PlayerMovement>,
    ty: &crate::damage_types::DamageType,
    type_name: &str,
    outcome: &crate::hurt::HurtOutcome,
    causing: Option<i32>,
    direct: Option<i32>,
    at: Option<[f64; 3]>,
) -> io::Result<()> {
    use crate::damagecmd;
    use rustmc_worldgen::rng::RandomSource;
    // `actuallyHurt`'s `recordDamage`, inside `hurtServer` and so ahead of
    // the `damage_event` below: `player_combat_enter` (or a lapsed combat's
    // `player_combat_end`) to this player alone.
    for p in std::mem::take(&mut live.combat_out) {
        send_combat_packet(s, p)?;
    }
    let entities = crate::entities::Entities::shared();
    let me = s.conn.entity_id();
    let tracked = crate::status::tracked_players(s.players, entities);
    let tracker = crate::tracker::Tracker::shared();
    if outcome.broadcast_damage_event {
        let Some(type_id) =
            damagecmd::type_ids().and_then(|ids| damagecmd::wire_id(ids, type_name))
        else {
            eprintln!(
                "console: damage — {} is not in the captured damage_type registry; \
                 no damage_event sent rather than a number invented",
                type_name
            );
            return Ok(());
        };
        // `DamageSource.sourcePosition()` goes on the wire only when the
        // source was built with one — `getSourcePosition()` would fall back
        // to the direct entity, the packet's field does not.
        let body = damagecmd::damage_event_body(me, type_id, causing, direct, at)?;
        s.conn.send(ids::play::cb::DAMAGE_EVENT, &body)?;
        tracker.send_to_tracking_players(me, &tracked, ids::play::cb::DAMAGE_EVENT, &body);
    }
    if outcome.mark_hurt {
        damagecmd::mark_hurt(me);
    }
    let mut body_now = entities.get(me);
    // Where the player is NOW: the server body's position when there is one.
    // A fall is hurt inside `handleMovePlayer` after the move and before this
    // server copies the position to the entity store, so the store still
    // holds the pre-landing position there (the first fall recording put the
    // hurt sound five blocks up).
    if let (Some(e), Some(mv)) = (body_now.as_mut(), body.as_deref()) {
        (e.x, e.y, e.z) = mv.position();
    }
    let mut rng = crate::dropitem::unique_random();
    if outcome.knockback {
        // `DamageSource.getSourcePosition()`: the explicit position, else the
        // direct entity's. No projectile can be named here, so offsets 7..52
        // never apply.
        let source_pos = at.or_else(|| {
            direct
                .and_then(|id| entities.get(id))
                .map(|e| [e.x, e.y, e.z])
        });
        let (x, z) = match (source_pos, body_now.as_ref()) {
            (Some(p), Some(e)) => (p[0] - e.x, p[2] - e.z),
            _ => (0.0, 0.0),
        };
        if let Some(e) = body_now.as_ref() {
            let resistance = crate::playermove::shared()
                .and_then(|t| {
                    t.attributes
                        .base("minecraft:player", "minecraft:knockback_resistance")
                })
                .unwrap_or(0.0);
            let mut redraw = || rng.next_f64();
            // The server's own body when there is one: its velocity is what
            // `travel` left, and `onGround` is the body's.
            let (velocity, on_ground) = body.as_deref().map_or((e.movement, e.on_ground), |mv| {
                (mv.velocity(), mv.body_on_ground())
            });
            if let Some(v) = damagecmd::knockback(
                velocity,
                on_ground,
                KNOCKBACK_STRENGTH,
                resistance,
                x,
                z,
                &mut redraw,
            ) {
                if let Some(mv) = body.as_mut() {
                    mv.set_velocity(v);
                }
                entities.update(me, |e| e.movement = v);
                damagecmd::mark_needs_sync(me);
            }
            // `dealDefaultKnockback` offsets 103..112: `!wasBlocked` ->
            // `indicateDamage(d, e)`, with the direction as computed and not
            // as `knockback` re-rolled it.
            if !outcome.was_blocked {
                let yaw = damagecmd::hurt_dir(x, z, e.y_rot);
                s.conn.send(
                    ids::play::cb::HURT_ANIMATION,
                    &damagecmd::hurt_animation_body(me, yaw)?,
                )?;
            }
        }
    }
    let sound = if outcome.died {
        Some("minecraft:entity.player.death")
    } else if outcome.full_hit {
        damagecmd::player_hurt_sound(&ty.effects)
    } else {
        None
    };
    if let (Some(event), Some(e)) = (sound, body_now.as_ref()) {
        // `LivingEntity.getVoicePitch()`: `(nextFloat() - nextFloat()) * 0.2f
        // + 1.0f` for an adult, off the entity's own unseeded random.
        let pitch = (rng.next_f32() - rng.next_f32()) * 0.2 + 1.0;
        crate::sounds::send(
            s.players,
            s.protocol_ids,
            s.sound_events,
            Some(entities),
            &[crate::sounds::Sound {
                event: event.to_string(),
                source: crate::sounds::SOURCE_PLAYERS,
                at: crate::sounds::Eighths::exact(e.x, e.y, e.z),
                pos: (e.x, e.y, e.z),
                volume: 1.0,
                pitch,
                except: Some(s.conn.id()),
            }],
        );
    }
    if outcome.died {
        server_player_die(s, live, ty, causing.or(direct), &tracked)?;
    }
    Ok(())
}

/// `ServerPlayer.onEnterCombat` / `onLeaveCombat`: one packet to this
/// player's own connection.
fn send_combat_packet(s: &Session<'_>, p: crate::combat_tracker::CombatPacket) -> io::Result<()> {
    match p {
        crate::combat_tracker::CombatPacket::Enter => {
            s.conn.send(ids::play::cb::PLAYER_COMBAT_ENTER, &[])
        }
        crate::combat_tracker::CombatPacket::End { duration } => s.conn.send(
            ids::play::cb::PLAYER_COMBAT_END,
            &crate::combat_tracker::combat_end_body(duration)?,
        ),
    }
}

/// The name of the connected player whose entity this is.
fn player_name_of(players: &crate::players::Players, entity: i32) -> Option<String> {
    players
        .connections()
        .into_iter()
        .find(|c| c.entity_id() == entity)
        .map(|c| c.name().to_string())
}

/// `Player.attack` from its offset 44 to `hurtOrSimulate`, for a target that
/// is another connected player: the strength scale off this player's own
/// ticker, [`crate::attack::melee`]'s plan, `onAttack()`'s reset (offset
/// 76), and the rest of the method handed to the target — see
/// [`crate::pvp`].
///
/// A target id that names no connection (it logged out between the reach
/// test and here) is `getEntityOrPart` returning null: nothing happens.
fn attack_a_player(
    s: &Session<'_>,
    target_id: i32,
    strength: &mut crate::pvp::Strength,
    movement: Option<&crate::playermove::PlayerMovement>,
) {
    let Some(target_conn) = s
        .players
        .connections()
        .into_iter()
        .find(|c| c.entity_id() == target_id)
        .map(|c| c.id())
    else {
        return;
    };
    // `canCriticalAttack` reads the attacker's body. The server's own body
    // when there is one; `onClimbable`, `isInWater`, blindness and riding
    // have no answer on this base and read as their idle values, which a
    // player standing on open ground holds anyway.
    let (on_ground, fall_distance) =
        movement.map_or((true, 0.0), |mv| (mv.body_on_ground(), mv.fall_distance()));
    let swing = crate::attack::Swing {
        attack_strength_ticker: strength.ticker,
        attack_speed: crate::attack::PLAYER_ATTACK_SPEED,
        attack_damage: crate::attack::PLAYER_ATTACK_DAMAGE,
        attack_knockback: crate::attack::PLAYER_ATTACK_KNOCKBACK,
        item_damage_bonus: 0.0,
        // No `ServerboundPlayerCommand` handling on this base: a player is
        // never sprinting here. Named in `crate::pvp`.
        sprinting: false,
        crit: crate::attack::CritConditions {
            fall_distance,
            on_ground,
            on_climbable: false,
            in_water: false,
            mobility_restricted: false,
            passenger: false,
            target_is_living: true,
            sprinting: false,
        },
        sweep: crate::attack::SweepConditions {
            strong: false,
            crit: false,
            knockback_sprint: false,
            on_ground,
            known_movement_horizontal_sqr: 0.0,
            speed: 0.0,
            holding_sword: false,
        },
    };
    let plan = crate::attack::melee(&swing);
    // 76: `onAttack()` -> `resetOnlyAttackStrengthTicker()`.
    strength.reset();
    crate::pvp::submit(
        target_conn,
        crate::pvp::PendingAttack {
            attacker: s.conn.entity_id(),
            attacker_conn: s.conn.id(),
            attacker_pos: (s.player.x, s.player.y, s.player.z),
            attacker_y_rot: s.player.yaw,
            plan,
            at_tick: s.clock.ticks() + 1,
        },
    );
}

/// `Player.playServerSideSound(sound)` for an attacker who is not this
/// session: `level.playSound(null, x, y, z, sound, PLAYERS, 1.0f, 1.0f)` at
/// the attacker's feet, heard by everyone in range, the attacker included.
fn play_attacker_sound(s: &Session<'_>, at: (f64, f64, f64), event: &str) {
    crate::sounds::send(
        s.players,
        s.protocol_ids,
        s.sound_events,
        Some(crate::entities::Entities::shared()),
        &[crate::sounds::Sound {
            event: event.to_string(),
            source: crate::sounds::SOURCE_PLAYERS,
            at: crate::sounds::Eighths::exact(at.0, at.1, at.2),
            pos: at,
            volume: 1.0,
            pitch: 1.0,
            except: None,
        }],
    );
}

/// The target's half of `Player.attack`, offsets 130-348, run on the
/// target's loop in the method's own order.
fn apply_player_attack(
    s: &mut Session<'_>,
    live: &mut crate::playertick::LivePlayer,
    mut body: Option<&mut crate::playermove::PlayerMovement>,
    attack: &crate::pvp::PendingAttack,
    facts: crate::playertick::LevelFacts,
) -> io::Result<()> {
    use crate::damagecmd;
    const TYPE: &str = "minecraft:player_attack";
    let (early, late) = crate::pvp::split_sounds(&attack.plan);
    // 130-133: the sprint-knockback sound, before the hit.
    for e in early {
        play_attacker_sound(s, attack.attacker_pos, e);
    }
    let Some(ty) = damagecmd::registry().and_then(|r| r.get(TYPE)) else {
        eprintln!("attack: no damage type {TYPE}; the hit is refused rather than typed");
        return Ok(());
    };
    let me = s.conn.entity_id();
    // 232-235: the target's velocity before the hit, which
    // `causeExtraKnockback` restores for a player target.
    let old_velocity = body.as_deref().map_or_else(
        || {
            crate::entities::Entities::shared()
                .get(me)
                .map_or((0.0, 0.0, 0.0), |e| e.movement)
        },
        crate::playermove::PlayerMovement::velocity,
    );
    // 242: `hurtOrSimulate(source, damage)` -> `ServerPlayer.hurtServer`.
    let outcome = live.hurt_player(
        attack.plan.damage,
        ty,
        true,
        false,
        facts,
        crate::gamerules::GameRules::shared(),
    );
    let Some(outcome) = outcome.filter(|o| o.hurt) else {
        // 345-348.
        play_attacker_sound(s, attack.attacker_pos, crate::attack::ATTACK_NODAMAGE_SOUND);
        return Ok(());
    };
    // `hurtServer` 274..282, as for the console's `/damage`.
    if let Some(n) = player_name_of(s.players, attack.attacker) {
        live.last_hurt_by_player = Some((attack.attacker, n));
        live.last_hurt_by_player_memory = 100;
    }
    hurt_level_half(
        s,
        live,
        body.as_deref_mut(),
        ty,
        TYPE,
        &outcome,
        Some(attack.attacker),
        Some(attack.attacker),
        None,
    )?;
    // 252-281: `causeExtraKnockback`.
    if let Some((strength, x, z)) =
        crate::pvp::extra_knockback_args(&attack.plan, attack.attacker_y_rot)
    {
        use rustmc_worldgen::rng::RandomSource;
        let mut rng = crate::dropitem::unique_random();
        let mut redraw = || rng.next_f64();
        let (velocity, on_ground) = body.as_deref().map_or((old_velocity, true), |mv| {
            (mv.velocity(), mv.body_on_ground())
        });
        let resistance = crate::playermove::shared()
            .and_then(|t| {
                t.attributes
                    .base("minecraft:player", "minecraft:knockback_resistance")
            })
            .unwrap_or(0.0);
        if let Some(v) =
            damagecmd::knockback(velocity, on_ground, strength, resistance, x, z, &mut redraw)
        {
            if let Some(mv) = body.as_deref_mut() {
                mv.set_velocity(v);
            }
            crate::entities::Entities::shared().update(me, |e| e.movement = v);
            damagecmd::mark_needs_sync(me);
        }
    }
    // `causeExtraKnockback` 124-166: a `ServerPlayer` target that is
    // `hurtMarked` is sent its own velocity now, the mark is cleared — so
    // `sendChanges` never broadcasts it — and the velocity is put back.
    let marked = damagecmd::take_hurt_mark(me);
    let marked_self = damagecmd::take_self_hurt_mark(me);
    if marked || marked_self {
        let v = body.as_deref().map_or_else(
            || {
                crate::entities::Entities::shared()
                    .get(me)
                    .map_or((0.0, 0.0, 0.0), |e| e.movement)
            },
            crate::playermove::PlayerMovement::velocity,
        );
        s.conn.send(
            ids::play::cb::SET_ENTITY_MOTION,
            &crate::tracker::set_entity_motion_body(me, v)?,
        )?;
        if let Some(mv) = body {
            mv.set_velocity(old_velocity);
        }
        crate::entities::Entities::shared().update(me, |e| e.movement = old_velocity);
    }
    // 299-312: `attackVisualEffects` — the crit and strong/weak sounds.
    for e in late {
        play_attacker_sound(s, attack.attacker_pos, e);
    }
    // 335: `causeFoodExhaustion(0.1f)`, the attacker's.
    crate::pvp::owe_exhaustion(attack.attacker_conn, attack.plan.exhaustion);
    Ok(())
}

/// `ServerPlayer.die(DamageSource)`, the arms a harness player reaches.
///
/// ```text
///      0  gameEvent(ENTITY_DIE)                              (no packet)
///     11  show_death_messages ->
///     43    player_combat_kill(id, getDeathMessage()) to self
///     99    broadcastSystemMessage(message, false)          -> system_chat, everyone
///   else
///    161    player_combat_kill(id, EMPTY) to self
///    209  !isSpectator -> dropAllDeathLoot                  (refused if non-empty)
///    282  broadcastEntityEvent(this, 3)                     -> tracking + self
///    358  lastDeathLocation = blockPosition()
///    365  markClientUnloadedAfterDeath()
/// ```
///
/// Team visibility is `ALWAYS` for a player on no team, and this server puts
/// no player on a team, so offsets 69..154 always take the broadcast.
fn server_player_die(
    s: &mut Session<'_>,
    live: &mut crate::playertick::LivePlayer,
    ty: &crate::damage_types::DamageType,
    attacker: Option<i32>,
    tracked: &[crate::tracker::TrackedPlayer],
) -> io::Result<()> {
    let me = s.conn.entity_id();
    let name = s.conn.name().to_string();
    let show = !matches!(
        crate::gamerules::GameRules::shared().get("show_death_messages"),
        Some(crate::gamerules::GameRuleValue::Bool(false))
    );
    if show {
        let attacker_name = attacker.and_then(|e| player_name_of(s.players, e));
        let credit = live.last_hurt_by_player.as_ref().map(|(_, n)| n.clone());
        match crate::playerdeath::death_message(
            &name,
            ty,
            attacker_name.as_deref(),
            false,
            credit.as_deref(),
        ) {
            Some(message) => {
                s.conn.send(
                    ids::play::cb::PLAYER_COMBAT_KILL,
                    &crate::death::encode_player_combat_kill_message(me, &message)?,
                )?;
                s.players.broadcast(
                    ids::play::cb::SYSTEM_CHAT,
                    &crate::commands::system_chat_body(&message)?,
                );
            }
            None => eprintln!(
                "console: {name} died to {} — its death message ({}) is a refused arm; \
                 no player_combat_kill and no chat line were sent",
                ty.message_id, ty.death_message_type
            ),
        }
    } else {
        s.conn.send(
            ids::play::cb::PLAYER_COMBAT_KILL,
            &crate::death::encode_player_combat_kill_message(
                me,
                &crate::playerdeath::empty_component(),
            )?,
        )?;
    }
    let carrying = s.hotbar.slots().iter().any(Option::is_some)
        || !s.hotbar.offhand().is_empty()
        || s.conn.with_experience(|e| e.total) > 0;
    if carrying {
        eprintln!(
            "console: {name} died carrying items or experience; dropAllDeathLoot is not \
             wired, so nothing was dropped"
        );
    }
    let ev = crate::playerdeath::entity_event_body(me, crate::playerdeath::ENTITY_EVENT_DEATH)?;
    s.conn.send(ids::play::cb::ENTITY_EVENT, &ev)?;
    crate::tracker::Tracker::shared().send_to_tracking_players(
        me,
        tracked,
        ids::play::cb::ENTITY_EVENT,
        &ev,
    );
    live.death_location = Some(crate::playerdeath::block_position(
        s.player.x, s.player.y, s.player.z,
    ));
    Ok(())
}

/// The self half of `ServerEntity.sendChanges` for this player:
/// `sendDirtyEntityData` (offset 1093) and then the `hurtMarked` velocity
/// (1202..1235), both `broadcastAndSend`/`sendToTrackingPlayersAndSelf`.
/// The tracking players' half is `crate::tracker::Tracker::tick`'s.
///
/// Recorded 2026-09-30: vanilla's actor stream after a hit is
/// `set_entity_data`, `set_entity_motion`, `set_health` — the level tick's
/// packets ahead of `doTick`'s — and this server sent `set_health` first,
/// because the self half went out from the clock thread and raced the loop.
fn send_changes_to_self(
    s: &Session<'_>,
    live: &crate::playertick::LivePlayer,
    mv: Option<&crate::playermove::PlayerMovement>,
    last_health: &mut f32,
) -> io::Result<()> {
    let me = s.conn.entity_id();
    // The velocity `ChunkMap.tick` would read this tick: what the last
    // `doTick`'s travel left. Published here, before this tick's travel,
    // so the tracker's half reads the same value the self half sends.
    if let Some(v) = mv.map(crate::playermove::PlayerMovement::velocity) {
        crate::entities::Entities::shared().update(me, |e| e.movement = v);
    }
    let h = live.health.health();
    if h.to_bits() != last_health.to_bits() {
        *last_health = h;
        let values = [crate::synched::DataValue {
            id: 9,
            serializer: crate::synched::serializer::FLOAT,
            value: h.to_be_bytes().to_vec(),
        }];
        s.conn.send(
            ids::play::cb::SET_ENTITY_DATA,
            &crate::synched::set_entity_data_body(me, &values)?,
        )?;
    }
    if crate::damagecmd::take_self_hurt_mark(me) {
        let v = mv.map_or_else(
            || {
                crate::entities::Entities::shared()
                    .get(me)
                    .map_or((0.0, 0.0, 0.0), |e| e.movement)
            },
            crate::playermove::PlayerMovement::velocity,
        );
        s.conn.send(
            ids::play::cb::SET_ENTITY_MOTION,
            &crate::tracker::set_entity_motion_body(me, v)?,
        )?;
    }
    Ok(())
}

/// One tick of the server's own player travel. It moves the velocity only;
/// see `PlayerMovement::server_travel`. The velocity reaches the entity store
/// at the start of the NEXT tick, in [`send_changes_to_self`], which is where
/// vanilla's `sendChanges` reads it.
fn server_travel_tick(
    s: &mut Session<'_>,
    mv: &mut crate::playermove::PlayerMovement,
    t: &crate::playermove::Tables,
    blocks: &dyn crate::edits::Blocks,
    live: &crate::playertick::LivePlayer,
    walk: &mut crate::step::Walk,
    refusals: &mut u64,
) {
    let facts = crate::playermove::TravelFacts {
        shift_key_down: s.player.shift_key_down,
        flying: false,
        levitation: live
            .effect("minecraft:levitation")
            .map(crate::effects::MobEffectInstance::amplifier),
        slow_falling: live.has_effect("minecraft:slow_falling"),
        friction_modifier: crate::physics::friction_modifier(&t.attributes).unwrap_or(1.0),
        air_drag_modifier: crate::physics::air_drag_modifier(&t.attributes).unwrap_or(1.0),
        climbable_tag: crate::fallable::tags(),
    };
    match mv.server_travel(blocks, t, facts) {
        Ok(Some(step)) => step_sound_at(s, walk, step.from, step.to, step.on_ground),
        Ok(None) => {}
        Err(why) => {
            if *refusals == 0 {
                eprintln!(
                    "{}: server-side player travel refused ({why:?}); the velocity is left \
                     as it was — counted from here on",
                    s.conn.name()
                );
            }
            *refusals = refusals.saturating_add(1);
        }
    }
}

/// The `set_time` body this server sends once a second — see the comment at
/// its call site in [`keep_alive_loop`] for what it does and does not model.
fn set_time_now(clock_ids: &[i32], now: i64) -> io::Result<Vec<u8>> {
    let day = crate::sky::day_time(now);
    let rate = if crate::sky::advances() { 1.0 } else { 0.0 };
    let states: Vec<(i32, ClockState)> = clock_ids
        .iter()
        .map(|id| {
            (
                *id,
                ClockState {
                    total_ticks: day,
                    partial_tick: 0.0,
                    rate,
                },
            )
        })
        .collect();
    time_body(now, &states)
}

/// `ServerGamePacketListenerImpl.handleClientCommand`, `PERFORM_RESPAWN`,
/// and `PlayerList.respawn(player, false, KILLED)`.
///
/// ```text
///   handleClientCommand
///    139  getHealth() > 0                       -> return   (nothing sent)
///         playerList.respawn(player, false, KILLED)
///   PlayerList.respawn
///     35  level.removePlayerImmediately(old)                  (if tickDeath has not)
///     45  new ServerPlayer(...); restoreFrom(old, false); setId(old.getId())
///    249  respawn(createCommonSpawnInfo, dataToKeep 0)         -> respawn
///    303  connection.teleport(x, y, z, yRot, xRot)             -> player_position
///    323  set_default_spawn_position
///    352  change_difficulty                                    (no id here: not sent)
///    382  set_experience
///    388  sendActivePlayerEffects                              (a new player has none)
///    396  sendLevelInfo: border, time, spawn position, weather, game event 13
///    402  sendPlayerPermissionLevel: entity_event 24+level, commands
///    409  addRespawnedPlayer                                   -> tracked again, chunks
/// ```
///
/// **The entity id is kept** (`setId(old.getId())` at offsets 87..90), which
/// is why this server can respawn on the same connection without renumbering
/// anything. `restoreFrom(old, false)` copies nothing a harness player holds.
///
/// Returns whether a respawn happened.
///
/// The respawn point is the position the session joined at: this server
/// stores no level respawn data, and `status::serve_one` places a joining
/// player at the spawn it computes, which is the one a respawn would find.
fn perform_respawn(
    s: &mut Session<'_>,
    live: &mut crate::playertick::LivePlayer,
    movement: &mut Option<crate::playermove::PlayerMovement>,
    respawn_at: (f64, f64, f64),
    template: Option<&crate::entities::Entity>,
) -> io::Result<bool> {
    if live.health.health() > 0.0 {
        return Ok(false);
    }
    let me = s.conn.entity_id();
    let entities = crate::entities::Entities::shared();
    let tracker = crate::tracker::Tracker::shared();
    // Offsets 45..90: `new ServerPlayer(...)` draws an id from the counter in
    // `Entity.<init>`, and `setId(old.getId())` then stamps the old one over
    // it — one number burned per respawn. Every entity created after a death
    // has the opposite parity to one created on a server that skipped this,
    // and parity is what `Mob.serverAiStep`'s goal phase reads.
    let _burned = entities.next_id();
    if !live.removed {
        let tracked = crate::status::tracked_players(s.players, entities);
        tracker.remove(me, &tracked);
        tracker.untrack_observer(s.conn);
        entities.remove(me);
    }
    let died_at = live
        .death_location
        .unwrap_or_else(|| crate::playerdeath::block_position(s.player.x, s.player.y, s.player.z));
    *live = crate::playertick::LivePlayer::from_attributes(
        crate::playermove::shared().map(|t| &t.attributes),
    );

    match crate::playerdeath::respawn_info(me, died_at) {
        Some(info) => s.conn.send(
            ids::play::cb::RESPAWN,
            &crate::respawn::encode_respawn(&info, false)?,
        )?,
        None => eprintln!(
            "{}: no join spawn info remembered — respawn packet not sent",
            s.conn.name()
        ),
    }
    // 303: the teleport, on the connection's own counter.
    let tp = movement
        .as_mut()
        .map_or(JOIN_TELEPORT_ID, |mv| mv.respawn(respawn_at));
    s.player.x = respawn_at.0;
    s.player.y = respawn_at.1;
    s.player.z = respawn_at.2;
    s.player.yaw = 0.0;
    s.player.pitch = 0.0;
    s.player.awaiting(tp, respawn_at);
    s.conn.send(
        ids::play::cb::PLAYER_POSITION,
        &player_position_body(tp, respawn_at, 0.0, 0.0)?,
    )?;
    let spawn_block = crate::playerdeath::block_position(respawn_at.0, respawn_at.1, respawn_at.2);
    let dimension = crate::playerdeath::respawn_info(me, died_at)
        .map_or_else(|| "minecraft:overworld".to_string(), |i| i.dimension);
    let spawn_pos = crate::respawn::encode_set_default_spawn_position(
        &dimension,
        spawn_block.0,
        spawn_block.1,
        spawn_block.2,
        0.0,
        0.0,
    )?;
    s.conn
        .send(ids::play::cb::SET_DEFAULT_SPAWN_POSITION, &spawn_pos)?;
    s.conn.send(
        ids::play::cb::SET_EXPERIENCE,
        &crate::respawn::encode_set_experience(0.0, 0, 0)?,
    )?;
    // 396: `sendLevelInfo`.
    {
        let mut body = Vec::new();
        crate::worldborder::packets::InitializeBorder::of(
            &crate::worldborder::shared()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
        .write(&mut body)?;
        s.conn.send(ids::play::cb::INITIALIZE_BORDER, &body)?;
    }
    s.conn.send(
        ids::play::cb::SET_TIME,
        &set_time_now(s.clock_ids, s.clock.ticks())?,
    )?;
    s.conn
        .send(ids::play::cb::SET_DEFAULT_SPAWN_POSITION, &spawn_pos)?;
    let weather = crate::weather::shared()
        .lock()
        .map_or_else(|e| e.into_inner().join_events(), |w| w.join_events());
    for (event, param) in weather {
        let mut ev = Vec::new();
        wire::write_u8(&mut ev, event as u8)?;
        wire::write_f32(&mut ev, param)?;
        s.conn.send(ids::play::cb::GAME_EVENT, &ev)?;
    }
    {
        let mut ev = Vec::new();
        wire::write_u8(
            &mut ev,
            crate::playerdeath::GAME_EVENT_LEVEL_CHUNKS_LOAD_START,
        )?;
        wire::write_f32(&mut ev, 0.0)?;
        s.conn.send(ids::play::cb::GAME_EVENT, &ev)?;
    }
    // 402: `sendPlayerPermissionLevel`.
    crate::commands::send_commands(s.conn, crate::ops::level_for(s.conn.name()))?;
    // 409: `addRespawnedPlayer` — tracked by the others again, and the world
    // shown to the new player: its client dropped every entity and chunk
    // when the respawn packet arrived.
    if let (Some(t), Some(kinds)) = (template, crate::entities::shared_kind_ids()) {
        let mut e = t.clone();
        e.x = respawn_at.0;
        e.y = respawn_at.1;
        e.z = respawn_at.2;
        e.y_rot = 0.0;
        e.x_rot = 0.0;
        e.y_head_rot = 0.0;
        e.movement = (0.0, 0.0, 0.0);
        if let crate::synched::Synched::Avatar { health, .. } = &mut e.synched {
            *health = live.health.health();
        }
        entities.insert(e.clone());
        let others = crate::status::tracked_players(s.players, entities);
        tracker.add(&e, &others, kinds);
        if let Some(me_tracked) = others.iter().find(|p| p.entity_id == me) {
            tracker.show_world_to(me_tracked, entities, kinds);
        }
    }
    // A respawned player is a new `ServerPlayer`: its tracking view starts
    // `EMPTY` and the client has dropped its chunk cache with the respawn
    // packet, so the next tracking update sends the centre and queues the
    // whole view again.
    let centre = s.player.chunk();
    s.conn.with_chunk_view(ChunkView::forget_everything);
    update_chunk_tracking(s, centre)?;
    Ok(true)
}

/// `LivingEntity.tickDeath`'s twentieth tick: the poof, then
/// `remove(KILLED)` — which for a player still connected is
/// `ChunkMap.removeEntity`: everyone tracking it is told it is gone, and it
/// stops tracking everything it could see.
fn remove_dead_player(s: &Session<'_>) -> io::Result<()> {
    let me = s.conn.entity_id();
    let entities = crate::entities::Entities::shared();
    let tracked = crate::status::tracked_players(s.players, entities);
    let tracker = crate::tracker::Tracker::shared();
    let ev = crate::playerdeath::entity_event_body(me, crate::playerdeath::ENTITY_EVENT_POOF)?;
    s.conn.send(ids::play::cb::ENTITY_EVENT, &ev)?;
    tracker.send_to_tracking_players(me, &tracked, ids::play::cb::ENTITY_EVENT, &ev);
    tracker.remove(me, &tracked);
    tracker.untrack_observer(s.conn);
    entities.remove(me);
    Ok(())
}

/// `dealDefaultKnockback` offset 91: `ldc2_w 0.4000000059604645d` — the float
/// `0.4f` widened, not the double `0.4`.
const KNOCKBACK_STRENGTH: f64 = 0.400_000_005_960_464_5;

/// `ServerPlayer.checkMovementStatistics(dx, dy, dz)` and, when the move was a
/// jump, `ServerPlayer.jumpFromGround()`.
///
/// **Five of the eight predicates are `false` here and every one of them is a
/// stated gap, not a guess.** `isSwimming`, `isEyeInFluid(WATER)` and
/// `isInWater` need a fluid query this server's player does not have;
/// `onClimbable` needs the `BlockTags.CLIMBABLE` table
/// `origin/player-movement` refused for want of a generator; `isSprinting`
/// needs the `SynchedEntityData` dirty path the same branch refused, and
/// `isFallFlying` needs elytra. So the only reachable arm on the ground is
/// the walking one, which charges `0.0f * i * 0.01f` — a real call with a
/// computed zero, exactly as vanilla makes it. The **jump** is the one
/// exhaustion source this server can actually drive, at `0.05f` a jump.
fn apply_movement_exhaustion(
    live: &mut crate::playertick::LivePlayer,
    mv: &crate::playermove::PlayerMovement,
    movement: (f64, f64, f64),
    jumped: bool,
    creative: bool,
) {
    let ctx = crate::movestats::MoveContext {
        on_ground: mv.on_ground(),
        ..crate::movestats::MoveContext::default()
    };
    let charge =
        crate::movestats::check_movement_statistics(movement.0, movement.1, movement.2, ctx);
    if let Some(amount) = charge.exhaustion {
        crate::movestats::cause_food_exhaustion(&mut live.food, creative, amount);
    }
    if jumped {
        // `isSprinting()` is `false` for the reason above, so this is always
        // the 0.05f arm today. Routed through `jump_from_ground` anyway so
        // that the sprint arm arrives with the flag rather than needing to be
        // found.
        crate::movestats::cause_food_exhaustion(
            &mut live.food,
            creative,
            crate::movestats::jump_from_ground(false),
        );
    }
}

/// `ClientboundUpdateMobEffectPacket`'s wire body — `write` offsets 0..49.
///
/// A varint entity id, then `MobEffect.STREAM_CODEC` — which is
/// `ByteBufCodecs.holderRegistry(Registries.MOB_EFFECT)`, i.e.
/// `ByteBufCodecs$28.encode`, a **plain** `VarInt.write(rawId)` with no `+1`
/// and no inline-holder escape — then a varint amplifier, a varint duration
/// and one byte of flags.
fn update_mob_effect_body(
    entity_id: i32,
    e: crate::playertick::EffectPacket,
) -> io::Result<Vec<u8>> {
    let mut b = Vec::new();
    write_varint(&mut b, entity_id)?;
    write_varint(&mut b, e.registry_id)?;
    write_varint(&mut b, e.amplifier)?;
    write_varint(&mut b, e.duration)?;
    wire::write_u8(&mut b, e.flags)?;
    Ok(b)
}

/// `ClientboundRemoveMobEffectPacket`'s wire body.
///
/// Its `STREAM_CODEC` composes `ByteBufCodecs.VAR_INT` with
/// `MobEffect.STREAM_CODEC`, so it is two varints and nothing else — no
/// trailing flags, which is the difference a reader that shares the update
/// packet's decoder would not notice until the next packet in the stream
/// desynchronised.
fn remove_mob_effect_body(entity_id: i32, registry_id: i32) -> io::Result<Vec<u8>> {
    let mut b = Vec::new();
    write_varint(&mut b, entity_id)?;
    write_varint(&mut b, registry_id)?;
    Ok(b)
}

/// How long a frame that has started arriving may take to finish before the
/// connection is given up on: vanilla's own keep-alive allowance.
const FRAME_PATIENCE: std::time::Duration = std::time::Duration::from_secs(15);

/// Hold a connection open: keep-alives out, client traffic handled.
///
/// # Why a server that does nothing still has to do this
///
/// A client disconnects itself after 30 seconds without a `keep_alive`, and
/// a server disconnects a client after 15 seconds without a reply. Neither
/// timeout says anything useful when it fires — the player sees "Timed out",
/// which is the same message a broken world would produce. Sleeping and
/// dropping the socket, which is what this did before, is indistinguishable
/// from a crash from the far side.
///
/// The interval is vanilla's: one every 15 seconds, carrying an i64 the
/// client echoes. The id is the tick it was sent at in vanilla; here it is a
/// counter, because nothing ticks yet and inventing a clock to look authentic
/// would be worse than a number that is honestly just a sequence.
///
/// # Errors
/// On I/O failure. A client that hangs up surfaces as `UnexpectedEof` and is
/// returned as success — that is a player quitting, not a fault.
pub fn keep_alive_loop(reader: &mut impl Read, s: &mut Session<'_>) -> io::Result<()> {
    let compressed = s.conn.compressed();
    let mut counter: i64 = 1;
    let mut last = std::time::Instant::now();
    // Vanilla sends the time once a second, not once a tick — the client
    // interpolates between them, so twenty a second is twenty times the
    // bandwidth for no visible difference.
    let mut last_time = std::time::Instant::now();
    // The last tick on which `send_next_chunks` ran, so it runs once a tick
    // rather than once an iteration. `-1` rather than the current tick so the
    // first pass through the loop serves whatever the join left queued.
    let mut last_chunk_tick: i64 = -1;
    // Where the player was when we last said anything about it, so the log
    // reports a *move* rather than every packet — a walking client sends
    // twenty a second.
    let mut reported = s.player.chunk();
    // `ChunkMap.updatePlayerStatus(player, true)`, whose last act is
    // `updateChunkTracking(player)`. Without it the view stays `EMPTY` until
    // the player crosses a chunk border, so a player who joins and stands
    // still is sent the join blob and nothing else, for ever — which is what
    // `ci/chunk-stream-probe.sh` measured as 25 chunks in 1 batch before this
    // line existed.
    update_chunk_tracking(s, reported)?;
    // The connection's game mode as this loop last applied it to `s.creative`.
    let mut seen_game_type = s.conn.game_type();
    // The last `/time` change this loop has sent; see `crate::sky::sync_generation`.
    let mut seen_time_generation = crate::sky::sync_generation();
    // `Entity.moveDist` and `Entity.nextStep` for this player, and the
    // locator-bar connections it is one end of. Both are established here
    // rather than at the join packet because this is the first point at which
    // the player has the position the client will build on: `Session::player`
    // holds the spawn until the first move packet, and a waypoint made from a
    // position the client has not acknowledged would have to be corrected by
    // the very next one.
    let mut walk = crate::step::Walk::new();
    // `ServerGamePacketListenerImpl`'s movement fields for this connection,
    // and the player's `Entity` body. `None` when `data/vanilla` is absent —
    // see `crate::playermove::shared` — in which case this loop keeps
    // exactly the behaviour it had before that module existed: the client's
    // position is written onto the player and believed.
    let tables = crate::playermove::shared();
    let mut movement = tables.and_then(|t| {
        let spawn = (s.player.x, s.player.y, s.player.z);
        let mut mv = crate::playermove::PlayerMovement::new(spawn, &t.attributes)?;
        // `enter_world` sent teleport id 1 before this loop started, and
        // vanilla's `PlayerList.placeNewPlayer` reaches the same `teleport`
        // method, so vanilla's counter is already 1 and its
        // `awaitingPositionFromClient` is already the spawn. The next
        // correction is id 2 on both, and until the client acknowledges the
        // join teleport every move is swallowed — which is
        // `updateAwaitingTeleport` at offsets 123-140 and is what makes a
        // correction stick.
        mv.adopt_join_teleport(1, spawn);
        Some(mv)
    });
    crate::waypoints::send(
        s.players,
        &s.waypoints.add_player(
            s.conn.id(),
            player_uuid(s),
            crate::step::block_position(s.player.x, s.player.y, s.player.z),
        ),
    );
    // `ServerGamePacketListenerImpl.tickPlayer` runs once per server tick,
    // and its movement half — `resetPosition()`, `tickCount++`,
    // `knownMovePacketCount = receivedMovePacketCount` — is what makes the
    // speed budget per-tick rather than per-connection. This loop has no tick
    // callback, so it watches the clock it already reads for the time packet.
    //
    // **Not once per packet.** With the anchor reset on every packet the
    // budget is per-packet and nothing is ever refused; with it never reset
    // a player is rubber-banded eleven blocks from spawn. Either mistake
    // produces a server that looks like it is checking movement.
    let mut last_tick = s.clock.ticks();
    // `ServerPlayer`'s own per-player state: health, food, experience, air
    // and the two `lastSent*` change detectors. Unlike `movement` this is
    // never `None` — a player starves and drowns whether or not the
    // collision tables are present — but the damage half of it is inert
    // without `data/vanilla/damage_types.json`, and
    // `LivePlayer::hurts_refused` counts how often that mattered.
    let mut live = crate::playertick::LivePlayer::from_attributes(tables.map(|t| &t.attributes));
    // The health the player's own client was last sent in `set_entity_data`
    // — `packDirty`'s baseline for the self half of `sendChanges`.
    let mut self_synched_health = live.health.health();
    // `Player.attackStrengthTicker` and the main-hand item it resets on —
    // see `crate::pvp::Strength`.
    let mut strength = crate::pvp::Strength::default();
    // How many ticks the server's player travel refused an arm it has no
    // port for — printed once, counted always, so a refusal is not silence.
    let mut travel_refusals: u64 = 0;
    // What a respawn needs from the join: where it put the player, and the
    // entity it announced — see [`perform_respawn`].
    let respawn_at = (s.player.x, s.player.y, s.player.z);
    let player_template = crate::entities::Entities::shared().get(s.conn.entity_id());
    // What `ServerLevel.getDifficulty()` and the game rules answer for this
    // session. Rebuilt every tick rather than captured: `/gamerule` can move
    // `natural_health_regeneration` underneath a live connection, and a
    // snapshot taken at join would make the rule take effect on the next
    // login instead of the next tick.
    let level_facts = |creative: bool| crate::playertick::LevelFacts {
        // `Player.causeFoodExhaustion` offsets 0..10 and `Player.hurtServer`
        // offsets 15..32 both read `abilities.invulnerable`, and
        // `GameType.CREATIVE` is what sets it. Of the four gates
        // `ci/substrate-gate-audit.py` names, this is the one this server
        // already knows about.
        abilities_invulnerable: creative,
        natural_regeneration: matches!(
            crate::gamerules::GameRules::shared().get("natural_health_regeneration"),
            Some(crate::gamerules::GameRuleValue::Bool(true))
        ),
        // **This server has no difficulty.** `ServerLevel.getDifficulty()`
        // has no counterpart here — nothing reads `server.properties`'
        // `difficulty` and no packet carries it — so the value is the one
        // both corpus substrates set, and it is named rather than defaulted.
        // `ci/t2-gameplay.sh` and `ci/t2-gameplay-survival.sh` both write
        // `difficulty=peaceful`, and on peaceful `FoodData.tick`'s starve arm
        // never fires (offset 335: `difficulty != PEACEFUL`). A server that
        // guessed NORMAL here would starve a player vanilla does not.
        difficulty: crate::playertick::SUBSTRATE_DIFFICULTY,
        // The fluid state a breath tick reads. This server's player has no
        // eye-in-fluid query yet, so the air never falls; see
        // `crate::playertick`'s header for what that costs and what it does
        // not.
        breath: crate::playertick::DRY_BREATH,
    };
    // The `OXYGEN_BONUS` draw. `crate::breath::decrease_air_supply` takes it
    // only when the bonus is positive, and nothing on this server makes it
    // so, which is why this panics rather than returning a number: an
    // invented draw off a stream nothing else agrees with is the divergence
    // that would never be found. `crate::playertick`'s unit tests exercise
    // the real path.
    let mut next_double =
        || -> f64 { unreachable!("OXYGEN_BONUS is never positive on this server") };
    // The caller sets the socket's read timeout — see `status::listen`.
    // `ServerPlayerGameMode` lives as long as the player does, and its
    // `tick()` runs once per game tick — caught up here on every wake, which
    // the one-tick read timeout makes every tick. See `crate::mining`.
    let mut game_mode = crate::mining::GameMode::new();
    let mut dig_tick = s.clock.ticks();
    // `Player.aiStep`'s touch loop runs once per server tick, from the
    // connection tick (`tickPlayer` -> `doTick`). See `crate::pickup`.
    let mut touched_tick = s.clock.ticks();
    loop {
        // `ServerPlayer.tick` -> `gameMode.tick()`, before the connection's
        // `doTick` below: the level ticks its players before the connections
        // tick (`MinecraftServer.tickChildren`). See `crate::mining`.
        let now = s.clock.ticks();
        for _ in 0..now.saturating_sub(dig_tick).min(64) {
            crate::dig::tick(s, &mut game_mode);
        }
        dig_tick = now;
        {
            let now = s.clock.ticks();
            // Caught up one tick at a time rather than collapsed to one
            // call: `tickCount` is what `updateAwaitingTeleport`'s 20-tick
            // resend counts, so collapsing a gap would delay a resend by the
            // size of the gap. Bounded so that a clock that jumps cannot
            // spin here.
            let behind = now.saturating_sub(last_tick).min(64);
            let facts = level_facts(s.creative);
            for step in 0..behind {
                // The tick this iteration simulates.
                let tick = last_tick + step + 1;
                // The packet phase of this tick, before any `doTick`: an
                // attack on this player read while the clock read `tick - 1`
                // is handled by vanilla now, and its `set_health` goes out in
                // this tick's `doTick` below. See `crate::pvp`.
                for attack in crate::pvp::take(s.conn.id(), tick) {
                    apply_player_attack(s, &mut live, movement.as_mut(), &attack, facts)?;
                }
                // `causeFoodExhaustion(0.1f)` owed by this player for a hit it
                // landed on someone else's loop.
                for e in crate::pvp::take_exhaustion(s.conn.id()) {
                    crate::movestats::cause_food_exhaustion(
                        &mut live.food,
                        facts.abilities_invulnerable,
                        e,
                    );
                }
                if let Some(mv) = movement.as_mut() {
                    mv.tick();
                }
                // `MinecraftServer.tickChildren` runs every level's
                // `ChunkMap.tick` before the connections, so this tick's
                // `sendChanges` reaches the player ahead of its `doTick`.
                send_changes_to_self(s, &live, movement.as_ref(), &mut self_synched_health)?;
                // `ServerPlayer.doTick` offset 15, `Player.tick` ->
                // `LivingEntity.aiStep` -> `travel`: the server's own
                // simulation of a player with no input. See
                // `PlayerMovement::server_travel` for why it exists.
                if let (Some(mv), Some(t), Some(blocks)) = (movement.as_mut(), tables, s.blocks) {
                    server_travel_tick(s, mv, t, blocks, &live, &mut walk, &mut travel_refusals);
                }
                // `ServerPlayer.doTick`, and the two packets it sends. This
                // is the only place either leaves this server.
                // `Player.tick` offsets 230-288: the strength ticker and the
                // main-hand reset. Only the attack reads either.
                strength.advance_to(tick, s.hotbar.held());
                for emit in live.do_tick(facts, &mut next_double) {
                    match emit {
                        crate::playertick::Emit::Health(h) => {
                            s.conn
                                .send(ids::play::cb::SET_HEALTH, &set_health_body(h)?)?;
                        }
                        crate::playertick::Emit::Experience(x) => {
                            s.conn
                                .send(ids::play::cb::SET_EXPERIENCE, &set_experience_body(x)?)?;
                        }
                        crate::playertick::Emit::EntityEvent(event) => {
                            s.conn.send(
                                ids::play::cb::ENTITY_EVENT,
                                &entity_event_body(s.conn.entity_id(), event)?,
                            )?;
                        }
                        crate::playertick::Emit::UpdateMobEffect(e) => {
                            s.conn.send(
                                ids::play::cb::UPDATE_MOB_EFFECT,
                                &update_mob_effect_body(s.conn.entity_id(), e)?,
                            )?;
                        }
                        crate::playertick::Emit::RemoveMobEffect { registry_id } => {
                            s.conn.send(
                                ids::play::cb::REMOVE_MOB_EFFECT,
                                &remove_mob_effect_body(s.conn.entity_id(), registry_id)?,
                            )?;
                        }
                        crate::playertick::Emit::DeathRemoval => remove_dead_player(s)?,
                        crate::playertick::Emit::Combat(p) => send_combat_packet(s, p)?,
                    }
                }
                // `doTick`'s experience half (offsets 487..532) is the clock
                // thread's `crate::experience::connection_tick`, off the one
                // store, with the orb touches it decides.
                // `DedicatedServer.tickConnection`: `super.tickConnection()`
                // (every `ServerPlayer.doTick`, above) and THEN
                // `handleConsoleInputs()`. So a console `/damage` lands after
                // this tick's `set_health` check and its health change goes
                // out on the next one — vanilla's own one-tick lag.
                for act in crate::damagecmd::take(s.conn.id(), tick) {
                    match act {
                        crate::damagecmd::Act::Hurt(hit) => {
                            apply_console_damage(s, &mut live, movement.as_mut(), &hit, facts)?;
                        }
                        crate::damagecmd::Act::Effect(e) => {
                            // `LivingEntity.addEffect` -> `onEffectAdded` ->
                            // `update_mob_effect`, with blend.
                            let instance = crate::effects::MobEffectInstance::with_flags(
                                e.effect,
                                e.duration,
                                e.amplifier,
                                false,
                                e.visible,
                                e.visible,
                            );
                            if let Some(crate::playertick::Emit::UpdateMobEffect(p)) =
                                live.add_effect(instance)
                            {
                                s.conn.send(
                                    ids::play::cb::UPDATE_MOB_EFFECT,
                                    &update_mob_effect_body(s.conn.entity_id(), p)?,
                                )?;
                            }
                        }
                    }
                }
                // `LivingEntity.setHealth` is `entityData.set(DATA_HEALTH_ID,
                // ...)`: the synched copy follows the health wherever it
                // moved this tick, and `ServerEntity.sendChanges` sends it on
                // as `set_entity_data` when it differs from what it last sent.
                let h = live.health.health();
                crate::entities::Entities::shared().update(s.conn.entity_id(), |e| {
                    if let crate::synched::Synched::Avatar { health, .. } = &mut e.synched {
                        *health = h;
                    }
                });
            }
            last_tick = now;
        }
        crate::dig::receive_gives(s);
        let tick_now = s.clock.ticks();
        if tick_now > touched_tick {
            touched_tick = tick_now;
            touch_items(s);
        }
        if last.elapsed() >= std::time::Duration::from_secs(15) {
            let mut b = Vec::new();
            wire::write_i64(&mut b, counter)?;
            s.conn.send(ids::play::cb::KEEP_ALIVE, &b)?;
            counter += 1;
            last = std::time::Instant::now();
        }
        // A `/time` change is sent at once, not at the next beat.
        let time_changed = crate::sky::sync_generation() != seen_time_generation;
        if time_changed || last_time.elapsed() >= std::time::Duration::from_secs(1) {
            seen_time_generation = crate::sky::sync_generation();
            let now = s.clock.ticks();
            // **The clock states are the DAY time and the day's rate, not the
            // game time and a hardcoded 1.0.** `ClockManager.getTotalTicks` is
            // what `AttributeTrackSampler` samples the sun's angle at, so this
            // is the same quantity `crate::sky` resolves, and a client told
            // `rate: 1.0` while `advance_time` is false animates a sun the
            // server has stopped.
            //
            // Two things this does NOT do, said rather than left:
            //
            // * it sends the same state for **every** clock id, which is what
            //   this line already did. There are two `WorldClock` entries in
            //   26.2 — `minecraft:overworld` and `minecraft:the_end` — and
            //   `clock_ids` carries ids without names, so telling them apart
            //   needs the registry threaded through `Session`. The end's rule
            //   is unported either way, so this is one wrong clock rather than
            //   two;
            // * `partial_tick` stays 0.0. This packet goes out once a second,
            //   not once a tick, so a partial that meant anything would need
            //   the send moved.
            //
            // Nothing in the corpus observes `set_time` — no scenario lists it
            // — so this is a correctness change with no measurement behind it,
            // which is why it is scoped to what the bytecode plainly says.
            s.conn
                .send(ids::play::cb::SET_TIME, &set_time_now(s.clock_ids, now)?)?;
            last_time = std::time::Instant::now();
        }
        // `ServerGamePacketListenerImpl.tick()` -> `chunkSender.sendNextChunks`,
        // once per server tick and no more often. The old code offered chunks
        // only when the player crossed a chunk border, plus a once-a-second
        // retry for whatever was still missing; both are subsumed here,
        // because a chunk stays in the sender's queue until it is both
        // generated and within quota. A player who walks to the edge of the
        // world and stops is still served: the tile finishes, the next tick
        // finds it available, and the batch goes out.
        //
        // Gated on the tick counter rather than run every iteration: the
        // quota arithmetic in `PlayerChunkSender` is per call, so a loop that
        // spins faster than 20 Hz would stream faster than vanilla by exactly
        // that ratio.
        let now_tick = s.clock.ticks();
        if now_tick != last_chunk_tick {
            last_chunk_tick = now_tick;
            send_next_chunks(s)?;
        }
        // What this player holds, as of the frames handled so far, for a
        // mob's `TemptGoal` on the clock thread.
        crate::mob::publish_hands(
            s.conn.entity_id(),
            s.hotbar.held(),
            Some(s.hotbar.offhand())
                .filter(|o| !o.is_empty())
                .map(|o| o.item().to_string())
                .as_deref(),
        );
        // `/gamemode` changes the mode on the connection from whichever
        // thread ran the command; every mechanic in this loop reads it through
        // `s.creative`, so a CHANGE is carried over before the next frame.
        // Only a change: the session's own starting value is the join's and is
        // not overwritten by a connection that was never told otherwise.
        let game_type = s.conn.game_type();
        if game_type != seen_game_type {
            seen_game_type = game_type;
            s.creative = game_type == crate::status::CREATIVE;
        }
        // A `/teleport` aimed at this player from any thread lands here.
        for t in s.conn.take_teleports() {
            command_teleport(s, t)?;
            if s.player.chunk() != reported {
                reported = s.player.chunk();
                update_chunk_tracking(s, reported)?;
            }
        }
        // `read_frame_whole`, not `read_frame`: the 50 ms read timeout may
        // land between a frame's length and its body, and a plain read
        // then loses the frame and desynchronises the stream for good. See
        // `rustmc_proto::frame::read_frame_whole`.
        match frame::read_frame_whole(reader, compressed, FRAME_PATIENCE) {
            Ok((id, body)) => {
                // Which scenario's workspace this connection is standing in.
                // Several corpus positive controls sit in code with no block
                // position — `useWithoutItem` takes a state and nothing else —
                // and plumbing one through to serve a test would change the
                // shape of the thing under test. Set once per frame, from the
                // acting player, which is exact: the recorder walks both
                // players into the workspace before the scenario starts and
                // neither leaves it. A no-op without `corpus-sabotage`.
                crate::sabotage::here_is(s.player.x);
                // `handleChunkBatchReceived` — the client's answer to
                // `chunk_batch_finished`, carrying the chunks per tick it
                // wants. Until it arrives `maxUnacknowledgedBatches` is 1, so
                // ignoring this packet does not merely skip a throttle: it
                // delivers one batch and then stops for ever.
                if id == ids::play::sb::CHUNK_BATCH_RECEIVED {
                    if body.len() >= 4 {
                        let bits = u32::from_be_bytes([body[0], body[1], body[2], body[3]]);
                        s.conn
                            .with_chunk_view(|v| v.on_batch_received(f32::from_bits(bits)));
                    }
                    continue;
                }
                if id == ids::play::sb::PLAYER_ACTION {
                    handle_player_action(s, &mut game_mode, &body)?;
                    continue;
                }
                if id == ids::play::sb::USE_ITEM_ON {
                    handle_use_item_on(s, &body)?;
                    continue;
                }
                // `handleUseItem` — the right click with nothing aimed at a
                // block. Its own packet (`minecraft:use_item`, id 67) and a
                // different body from `use_item_on`: hand, sequence, two
                // floats, no hit result at all.
                if id == ids::play::sb::USE_ITEM {
                    handle_use_item(s, &body)?;
                    continue;
                }
                if id == ids::play::sb::CONTAINER_CLICK {
                    handle_container_click(s, &body)?;
                    continue;
                }
                if id == ids::play::sb::CONTAINER_CLOSE {
                    handle_container_close(s)?;
                    continue;
                }
                if id == ids::play::sb::CONTAINER_BUTTON_CLICK {
                    handle_container_button_click(s, &body)?;
                    continue;
                }
                if id == ids::play::sb::RENAME_ITEM {
                    handle_rename_item(s, &body)?;
                    continue;
                }
                if id == ids::play::sb::SIGN_UPDATE {
                    handle_sign_update(s, &body)?;
                    continue;
                }
                if id == ids::play::sb::SWING {
                    // Tick `T` has run in vanilla before a packet read at `T`
                    // is handled — see `crate::pvp::Strength`.
                    strength.advance_to(s.clock.ticks(), s.hotbar.held());
                    handle_swing(s, &body);
                    // `ServerPlayer.swing` 2-6: `resetAttackStrengthTicker()`.
                    strength.reset();
                    continue;
                }
                // `handleAttack`. Its own packet in 26.2 — `minecraft:attack`
                // is protocol id 1 and `minecraft:interact` is 26, and each
                // has its own arm. See `crate::attack`.
                if id == ids::play::sb::ATTACK {
                    strength.advance_to(s.clock.ticks(), s.hotbar.held());
                    handle_attack(s, &body, &mut strength, movement.as_ref());
                    continue;
                }
                // `handleInteract`: a right click on an entity. See
                // `crate::mob::interact`.
                if id == ids::play::sb::INTERACT {
                    handle_interact(s, &body)?;
                    continue;
                }
                // `handlePlayerInput` stores one bit on the player and sends
                // nothing back — see `crate::player::input_shift`. It is an
                // arm here rather than a case inside `crate::player::apply`
                // because `apply`'s non-`Ignored` answers all mean "the
                // player moved", and the caller below broadcasts on them; a
                // sneak moves nobody, and vanilla's `setShiftKeyDown` reaches
                // no packet on this path either.
                if let Some(shift) = crate::player::input_shift(id, &body)? {
                    s.player.shift_key_down = shift;
                    continue;
                }
                // `ServerGamePacketListenerImpl.handleChatCommand` ->
                // `performUnsignedChatCommand` -> the dispatcher. Answered on
                // this thread rather than queued for the tick loop, which is
                // where vanilla's own answer comes from too: `sendSuccess`
                // writes straight to the source's connection.
                if id == ids::play::sb::CHAT_COMMAND {
                    crate::commands::handle(s, &body)?;
                    continue;
                }
                // `handleClientCommand`: `PERFORM_RESPAWN` (ordinal 0) is the
                // one action with an answer here; the other two are refused
                // in `crate::respawn::client_command_blocker`.
                if id == ids::play::sb::CLIENT_COMMAND {
                    let mut r = body.as_slice();
                    let action = rustmc_proto::varint::read_varint(&mut r)?;
                    if crate::respawn::ClientCommandAction::from_ordinal(action)
                        == Some(crate::respawn::ClientCommandAction::PerformRespawn)
                        && perform_respawn(
                            s,
                            &mut live,
                            &mut movement,
                            respawn_at,
                            player_template.as_ref(),
                        )?
                    {
                        // The new `ServerPlayer`'s health was set after
                        // its data was defined (`PlayerList.respawn`
                        // offset 453), so it is dirty, and the next
                        // `sendChanges` sends it to the player itself as
                        // well — the recording shows `set_entity_data`
                        // after the permission event. Its own client has
                        // never been sent the new entity's values.
                        self_synched_health = 1.0;
                    }
                    continue;
                }
                // `ServerGamePacketListenerImpl.handleChat` -> `tryHandleChat`
                // -> `broadcastChatMessage`. Answered on this thread for the
                // same reason the command is: vanilla hops to the main thread
                // via `server.execute`, but every packet that reaches a
                // client comes from `PlayerList.broadcastChatMessage` writing
                // straight to each connection, and this server has no shared
                // chat state for a tick boundary to protect.
                if id == ids::play::sb::CHAT {
                    crate::chat::handle(s, &body)?;
                    continue;
                }
                // Dropped **deliberately**, not by falling through. Each is
                // inert on an offline-mode server — see `crate::chat`'s
                // header — and without these arms they would reach
                // `crate::player::apply`, which decodes by packet id and
                // would read a chat acknowledgement as movement.
                if id == ids::play::sb::CHAT_ACK
                    || id == ids::play::sb::CHAT_SESSION_UPDATE
                    || id == ids::play::sb::CHAT_COMMAND_SIGNED
                {
                    continue;
                }
                if crate::player::apply_hand(id, &body, s.hotbar, s.items)? {
                    // `ServerRecipeBook.addRecipes`, narrowed to the item this
                    // packet just put somewhere — see `crate::recipe_book`.
                    // Beside `apply_hand` rather than inside it: this reads
                    // the same body for a different question, and does so
                    // whether or not `apply_hand`'s own writes land anywhere
                    // this player can select.
                    if let Some(item) = crate::player::creative_slot_item(id, &body, s.items)? {
                        if let Some(unlocks) = s.recipe_unlocks {
                            if let Some(recipe_body) = unlocks.hold(&item, s.known_recipes) {
                                s.conn.send(ids::play::cb::RECIPE_BOOK_ADD, &recipe_body)?;
                            }
                        }
                        // `CriteriaTriggers.INVENTORY_CHANGED`, fired from
                        // the same event and immediately after, because
                        // vanilla's route to the packet above runs THROUGH
                        // this trigger: `ServerPlayer$2.slotChanged` fires
                        // it, an `inventory_changed` criterion completes a
                        // `minecraft:recipes/*` advancement, and its
                        // `rewards.recipes` reach `ServerRecipeBook.addRecipes`.
                        // `crate::recipe_book`'s generated table is the
                        // short circuit across that path and it stays: both
                        // share `known_recipes`, so a recipe the short
                        // circuit has already sent is not sent twice, and
                        // the corpus check that grades it does not move.
                        //
                        // The flush is here rather than on a tick because
                        // this loop has no per-player tick hook. Vanilla's
                        // is the last statement of `ServerPlayer.tick`, so
                        // the packet lands in the same tick as the trigger
                        // either way; what would differ is a tick in which
                        // two triggers fired, which no scenario reaches.
                        if let (Some(pa), Some(engine)) =
                            (s.advancements.as_deref_mut(), s.adv_engine)
                        {
                            let now = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
                            let fired = crate::advancements::triggers::inventory_changed(
                                pa,
                                engine.table,
                                engine.tags,
                                &item,
                                now,
                            );
                            for r in &fired.rewards.recipes {
                                s.known_recipes.insert(r.clone());
                            }
                            // `showAdvancements` is `iconst_1` at
                            // `ServerPlayer.tick` offset 191.
                            if let Some(adv_body) = pa.flush_dirty(true)? {
                                s.conn.send(ids::play::cb::UPDATE_ADVANCEMENTS, &adv_body)?;
                            }
                        }
                    }
                    continue;
                }
                // Decoded rather than dropped. Until this, a player could
                // walk around and the server had no idea where they were,
                // which blocks everything downstream of position.
                //
                // The position *before* the packet, because
                // `handleMovePlayer` moves by a delta:
                // `player.move(PLAYER, new Vec3(x - lastGoodX, …))` at offsets
                // 590-689. `lastGood` is where the server last agreed the
                // player was, which for a server that does no collision is
                // simply the previous packet's position.
                let before = (s.player.x, s.player.y, s.player.z);
                // `handleMovePlayer` proper, when the tables and a world are
                // both present. It runs BEFORE `apply` writes the packet onto
                // the player, because a correction has to be decided against
                // where the server had them and not against where the client
                // says they are.
                //
                // A `Corrected` outcome is the only one that changes what
                // this loop does with the packet: the client's position is
                // refused, the teleport goes out, and `apply` is skipped so
                // the player entity keeps the position the server just
                // rubber-banded them to.
                if let (Some(mv), Some(blocks), Some(t)) = (movement.as_mut(), s.blocks, tables) {
                    if id == crate::player::ids::ACCEPT_TELEPORTATION {
                        let mut cur = &body[..];
                        let tp = rustmc_proto::varint::read_varint(&mut cur)?;
                        if mv.accept_teleport(tp).is_err() {
                            // `disconnect("multiplayer.disconnect.invalid_player_movement")`,
                            // which writes the packet before it closes, as the
                            // `Update::Disconnect` arm below does.
                            let body = crate::play::disconnect_translatable_body(
                                crate::player::INVALID_PLAYER_MOVEMENT,
                            )?;
                            s.conn.send(ids::play::cb::DISCONNECT, &body)?;
                            return Ok(());
                        }
                    } else if let Some(pkt) = crate::player::decode_move(id, &body)? {
                        let gates = crate::move_player::Gates {
                            creative: s.creative,
                            ..crate::move_player::Gates::default()
                        };
                        match mv.on_move(pkt, gates, blocks, &t.shapes, &t.props) {
                            crate::playermove::Outcome::Disconnect => return Ok(()),
                            crate::playermove::Outcome::Corrected {
                                to,
                                yaw,
                                pitch,
                                id: tp,
                                landing,
                                ..
                            } => {
                                // Offsets 926-966: the rubber-band path runs
                                // its OWN `doCheckFallDamage` before the
                                // teleport. A port that only checks the
                                // accepted path lets a player refused mid-fall
                                // land for free.
                                apply_landing(s, &mut live, mv, landing, level_facts(s.creative))?;
                                s.player.x = to.0;
                                s.player.y = to.1;
                                s.player.z = to.2;
                                s.player.yaw = yaw;
                                s.player.pitch = pitch;
                                // `teleport(PositionMoveRotation, Set)` holds
                                // the position for `handleAcceptTeleportPacket`,
                                // so `crate::player::apply`'s gate sees this
                                // teleport exactly as it sees the join's.
                                s.player.awaiting(tp, to);
                                s.conn.send(
                                    ids::play::cb::PLAYER_POSITION,
                                    &player_position_body(tp, to, yaw, pitch)?,
                                )?;
                                continue;
                            }
                            // `absSnapRotationTo(f, f1); return;` — offsets
                            // 130-140. The rotation is taken and the
                            // POSITION IS NOT, which is what makes a
                            // correction stick: until the client
                            // acknowledges the teleport, every move it sends
                            // moves nothing. Falling through to `apply` here
                            // would let a rubber-banded client keep walking,
                            // and it did until a wire-level test asked.
                            crate::playermove::Outcome::RotationOnly { yaw, pitch } => {
                                s.player.yaw = yaw;
                                s.player.pitch = pitch;
                                crate::entities::Entities::shared().update(
                                    s.conn.entity_id(),
                                    |e| {
                                        e.y_rot = yaw;
                                        e.x_rot = pitch;
                                        e.y_head_rot = yaw;
                                    },
                                );
                                continue;
                            }
                            // `wonGame` and `!hasClientLoaded` are plain
                            // `return`s at offsets 62-91.
                            crate::playermove::Outcome::Ignored => continue,
                            // `NoShape` falls through to the existing path.
                            // Refusing to move a player because a shulker box
                            // is on the path would be a worse divergence than
                            // the one it avoids, and it is recorded rather
                            // than acted on until a scenario measures it.
                            //
                            // The fall-through moves the player, so the
                            // tracker's body has to follow or it is left
                            // behind for good — see `adopt_unchecked_move`.
                            crate::playermove::Outcome::NoShape { .. } => {
                                if let Some(to) = pkt.pos {
                                    mv.adopt_unchecked_move(to);
                                }
                            }
                            // **The join `origin/player-systems` said could
                            // not be made.** It ported `causeFallDamage` and
                            // declined to wire it because "the server never
                            // knows the player fell"; `landing` is that
                            // knowledge, computed by `Entity.checkFallDamage`
                            // at `handleMovePlayer` offsets 1163-1188, and
                            // `movement` is the vector
                            // `checkMovementStatistics` takes at 1146-1162.
                            //
                            // Both run HERE and not on the tick, because
                            // vanilla runs both inside `handleMovePlayer`.
                            // The health they move is on the wire at the next
                            // `doTick`, which is the one-tick lag vanilla has
                            // too.
                            crate::playermove::Outcome::Moved {
                                movement,
                                landing,
                                jumped,
                                ..
                            } => {
                                let facts = level_facts(s.creative);
                                apply_landing(s, &mut live, mv, landing, facts)?;
                                apply_movement_exhaustion(
                                    &mut live, mv, movement, jumped, s.creative,
                                );
                            }
                        }
                    }
                }
                let update = crate::player::apply(id, &body, s.player)?;
                // `ServerCommonPacketListenerImpl.disconnect` writes the packet
                // and closes the channel, so the loop ends here rather than
                // carrying on with a player vanilla has already given up on.
                // Returning `Ok` and not an error: a refusal the protocol
                // prescribes is not a fault of this server.
                if let crate::player::Update::Disconnect(key) = update {
                    let body = crate::play::disconnect_translatable_body(key)?;
                    s.conn.send(ids::play::cb::DISCONNECT, &body)?;
                    return Ok(());
                }
                if update != crate::player::Update::Ignored {
                    // `handlePlayerKnownMovement` (handleMovePlayer 1189-1192):
                    // the delta of the move just applied, which a leashed
                    // mob's elastic pull reads as the holder's movement.
                    crate::mob::publish_known_movement(
                        s.conn.entity_id(),
                        (
                            s.player.x - before.0,
                            s.player.y - before.1,
                            s.player.z - before.2,
                        ),
                    );
                    // A player *is* an entity, and this is where it moves.
                    // `ServerGamePacketListenerImpl.handleMovePlayer` writes
                    // the client's position onto the player entity and does
                    // no broadcasting of its own; every packet another client
                    // sees comes from `ServerEntity.sendChanges` on the next
                    // tick, reading what was written here.
                    //
                    // The head rotation follows the yaw because a player has
                    // no independent head — `Avatar.getYHeadRot` returns
                    // `yHeadRot`, which the movement handler sets from the
                    // same value it sets `yRot` from.
                    crate::entities::Entities::shared().update(s.conn.entity_id(), |e| {
                        e.x = s.player.x;
                        e.y = s.player.y;
                        e.z = s.player.z;
                        e.y_rot = s.player.yaw;
                        e.x_rot = s.player.pitch;
                        e.y_head_rot = s.player.yaw;
                        e.on_ground = s.player.on_ground;
                    });
                    // `Entity.setPosRaw`, offsets 214-296, which runs
                    // **inside** `move` and therefore before the step sound
                    // the same `move` plays. The recording shows that order
                    // on the wire — waypoint, then sound, then the tick's
                    // `move_entity` — and a version that emitted the sound
                    // first would be right in content and wrong in sequence,
                    // which the strict comparison reads as a divergence just
                    // the same.
                    crate::waypoints::send(
                        s.players,
                        &s.waypoints.moved(
                            s.conn.id(),
                            crate::step::block_position(s.player.x, s.player.y, s.player.z),
                        ),
                    );
                    step_sound(s, &mut walk, before);
                    // `ChunkMap.move(player)` -> `updateChunkTracking`. On
                    // a section change, which for the chunk view is a chunk
                    // change. It queues and forgets; nothing is sent here.
                    if s.player.chunk() != reported {
                        reported = s.player.chunk();
                        update_chunk_tracking(s, reported)?;
                    }
                }
            }
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
            // A read timeout is the normal case: it is how this loop gets
            // back to the clock without blocking forever on a quiet client.
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(e) => return Err(e),
        }
    }
}

/// `Entity.move`'s step sound, for the move that has just been applied.
///
/// `before` is where the player was, so the delta is
/// `handleMovePlayer`'s `x - lastGoodX`. Vanilla runs the whole of `move`
/// here — collision, fall damage, block effects — and this server runs the one
/// part of it that reaches the wire; see [`crate::step`] for exactly which of
/// vanilla's gates are and are not reproduced.
///
/// **The player who took the step does not hear it.** That is
/// `Player.playSound`'s override, and it is the reason this can be added at
/// all: a step sound broadcast to everybody would land on the actor's stream
/// of every scenario that walks, which is most of them.
fn step_sound(s: &Session<'_>, walk: &mut crate::step::Walk, before: (f64, f64, f64)) {
    step_sound_at(
        s,
        walk,
        before,
        (s.player.x, s.player.y, s.player.z),
        s.player.on_ground,
    );
}

/// [`step_sound`] for a move that is not the packet's: the server's own
/// `travel` of the player, which reaches the same
/// `applyMovementEmissionAndPlaySound` from the same tail of `Entity.move`
/// (offsets 601..648 — `!level.isClientSide()` is enough, the entity need not
/// be authoritative). Recorded 2026-09-30 on `013`: a knocked-back vanilla
/// player's first travel tick plays a grass step sound to the observer.
fn step_sound_at(
    s: &Session<'_>,
    walk: &mut crate::step::Walk,
    before: (f64, f64, f64),
    at: (f64, f64, f64),
    on_ground: bool,
) {
    let (Some(blocks), Some(types)) = (s.blocks, s.sound_types) else {
        return;
    };
    // `getOnPosLegacy()` — the block whose sound type a step takes. Read
    // **after** the move, because `applyMovementEmissionAndPlaySound` is
    // called from the tail of `move` with a `BlockPos` taken at offset 509,
    // after the position has been written.
    let under = crate::step::on_pos(at.0, at.1, at.2, crate::step::LEGACY_OFFSET);
    let state = blocks.state_at(under);
    // `!onState.isAir()`. Asked of the state string rather than of a
    // `block_props` lookup because that is the one property this path needs
    // and the three air blocks are the whole of the answer.
    let solid = !matches!(
        state.split('[').next().unwrap_or(&state),
        "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
    );
    if !walk.advance_emitting(
        at.0 - before.0,
        at.2 - before.2,
        on_ground,
        solid,
        // `Player.getMovementEmission()`. `flying` is `Abilities.flying`,
        // which arrives on `ServerboundPlayerAbilitiesPacket` and is not
        // decoded here, so it is passed as false and named as a gap in
        // `crate::step::emits_sounds` rather than folded into the expression.
        crate::step::emits_sounds(false, on_ground, s.player.shift_key_down),
    ) {
        return;
    }
    let Some(row) = blocks
        .state_id(&state)
        .and_then(|id| u32::try_from(id).ok())
        .and_then(|id| types.row(id))
    else {
        return;
    };
    crate::sounds::send(
        s.players,
        s.protocol_ids,
        s.sound_events,
        Some(crate::entities::Entities::shared()),
        &[crate::sounds::player_step(
            row,
            at.0,
            at.1,
            at.2,
            s.conn.id(),
        )],
    );
}

/// A dig packet: destroy the block, tell everyone, then acknowledge.
///
/// The order is vanilla's and it is the order the client needs. The client
/// holds its own predicted result of the dig until the `block_changed_ack`
/// for that sequence arrives, and applies server updates underneath the
/// prediction; vanilla records the sequence during the handler and sends the
/// ack at the end of the tick, so every `block_update` the dig produced is
/// already on the wire when the prediction ends.
///
/// The ack is sent whether or not anything changed. A refused dig is exactly
/// the case the prediction exists for: without the ack the client keeps its
/// own guess that the block is gone, and no later packet can talk it out of
/// that.
fn handle_player_action(
    s: &mut Session<'_>,
    game_mode: &mut crate::mining::GameMode,
    body: &[u8],
) -> io::Result<()> {
    use crate::interact::{decode_player_action, Action};

    // Held for the whole handler: vanilla's game loop is single-threaded, so
    // a tick's broadcasts can never land between a handler's world edit and
    // the acknowledgement it owes. See `crate::broadcast::tick_lock`.
    let _tick = crate::broadcast::tick_lock();
    let action = decode_player_action(body)?;
    // `DROP_ITEM` reaches `Player.drop(stack, false)` — the *aimed* branch —
    // and nothing else. It names no block, carries `BlockPos.ZERO`, and is
    // never acknowledged, so it returns before the sequence is recorded.
    //
    // `DROP_ALL_ITEMS` is the same call with the whole stack — offsets
    // 241-259, `drop(true)`.
    if action.action == Action::DropItem || action.action == Action::DropAllItems {
        drop_held_item(s, action.action == Action::DropAllItems);
        return Ok(());
    }
    // `SWAP_ITEM_WITH_OFFHAND`, offsets 160-221. Like `DROP_ITEM` it names no
    // block, carries `BlockPos.ZERO` and is never acknowledged: the arm ends
    // in `return`, above the `ackBlockChangesUpTo` at the tail of the
    // `START/ABORT/STOP_DESTROY_BLOCK` arm.
    if action.action == Action::SwapItemWithOffhand {
        swap_to_offhand(s)?;
        return Ok(());
    }
    // `handlePlayerAction`'s tableswitch sends exactly the three digging
    // arms to `handleBlockBreakAction` and then `ackBlockChangesUpTo`; every
    // other arm — `RELEASE_USE_ITEM`, `STAB`, `DROP_ALL_ITEMS` — returns
    // without an ack. This server used to ack all of them.
    let dig = match action.action {
        Action::StartDestroyBlock => crate::mining::Dig::Start,
        Action::AbortDestroyBlock => crate::mining::Dig::Abort,
        Action::StopDestroyBlock => crate::mining::Dig::Stop,
        _ => return Ok(()),
    };
    crate::dig::block_break_action(s, game_mode, dig, action.pos);
    s.conn.ack_block_changes_up_to(action.sequence);
    Ok(())
}

/// `handlePlayerAction`'s `SWAP_ITEM_WITH_OFFHAND` arm, and the window-0
/// sweep that shows the result.
///
/// # The scenario's name is the requirement and its reference refutes half of
/// it
///
/// `510-swapping-to-the-offhand-shows-on-both-channels` says the change lands
/// on the equipment channel *and* the inventory channel, and its `observe`
/// list is `["set_equipment", "container_set_slot", "container_set_content"]`.
/// **Vanilla sends no `set_equipment` for this action.** Measured across five
/// independent recordings of this corpus (`rustmc-batch`, `-batch2`, `-cov3`,
/// `-iceberg`, `-joint`): the actor is sent exactly two `container_set_slot`
/// on window 0 — the selected hotbar slot, then slot 45 — and the observer is
/// sent one `entity_event`, which is not in the projection. `set_equipment`
/// is a kind the recorder does decode and does see elsewhere in the same
/// runs, so its absence here is a fact about the action and not about the
/// instrument.
///
/// **And the mechanism is in the jar, so this is a derivation and not just an
/// observation.** `LivingEntity.detectEquipmentUpdates` is
///
/// ```text
///   Map changed = collectEquipmentChanges(lastEquipmentItems);
///   if (changed != null) {
///       handleHandSwap(changed);
///       if (!changed.isEmpty()) handleEquipmentChanges(changed);
///   }
/// ```
///
/// and `handleHandSwap`, offsets 0-156, fires when both hands changed **and**
/// the new main hand `ItemStack.matches` the last known offhand and the new
/// offhand matches the last known main hand — an exchange rather than two
/// independent writes. It then broadcasts `ClientboundEntityEventPacket`
/// with **`EntityEvent.SWAP_HANDS`, byte 55**, `remove`s both slots from the
/// changed map and copies both into `lastEquipmentItems`. So the map handed
/// to `handleEquipmentChanges` is empty and the `set_equipment` never
/// happens: **vanilla substitutes one entity event for two equipment slots
/// and lets the client do the exchange itself.**
///
/// So the equipment channel does exist for this action and it is not the one
/// the check names. Both channels are reproduced here — the inventory one,
/// which is what `510` projects, and the entity-event one, which only the
/// strict projection can see. `set_equipment` remains what neither server
/// sends.
///
/// # Why the two packets and not a full sweep
///
/// `AbstractContainerMenu.broadcastChanges` walks the menu in **menu-slot**
/// order and sends where `RemoteSlot.matches` is false, taking a fresh state
/// id per packet. The swap can only have moved two cells, and menu slot
/// `36 + selected` is below 45, so the order is fixed. Slots whose content
/// did not change are skipped, which is `RemoteSlot.matches` being true: a
/// swap between two identical stacks writes nothing to the wire, and that is
/// vanilla's behaviour rather than an optimisation.
///
/// The state id is [`crate::menu::MenuState::inventory_state_id`], whose doc
/// named this exact action as one of the two window-0 corrections this server
/// did not emit. It emits one now.
///
/// # Errors
/// If a send fails.
fn swap_to_offhand(s: &mut Session<'_>) -> io::Result<()> {
    let selected = s.hotbar.selected();
    let before_main = s.hotbar.stack(selected);
    let before_offhand = s.hotbar.offhand();
    let (main_index, offhand_slot) = s.hotbar.swap_offhand();
    let after_main = s.hotbar.stack(main_index);
    let after_offhand = s.hotbar.offhand();

    // `handleHandSwap`'s guard, which for this handler is exactly "the two
    // stacks were not already identical": the swap writes the old offhand
    // into the main hand and the old main hand into the offhand, so the
    // `matches` pair in the bytecode holds by construction whenever
    // `collectEquipmentChanges` found anything at all. Two identical stacks
    // change nothing, `collectEquipmentChanges` returns null, and vanilla
    // sends neither the event nor any slot.
    if before_main != before_offhand {
        let id = s.conn.entity_id();
        let mut ev = Vec::new();
        // `ClientboundEntityEventPacket.write` — a big-endian i32 entity id,
        // **not** a varint, then one byte. The same encoding
        // `crate::commands` writes for the op-level event.
        rustmc_proto::wire::write_i32(&mut ev, id)?;
        rustmc_proto::wire::write_u8(&mut ev, SWAP_HANDS_ENTITY_EVENT)?;
        // `ServerChunkCache.sendToTrackingPlayers`, which is `seenBy` and so
        // excludes the swapper. The recorded trace agrees: the event is on
        // the observer's stream and not the actor's.
        let tracked =
            crate::status::tracked_players(s.players, crate::entities::Entities::shared());
        crate::tracker::Tracker::shared().send_to_tracking_players(
            id,
            &tracked,
            ids::play::cb::ENTITY_EVENT,
            &ev,
        );
    }

    let main_slot =
        crate::player::HOTBAR_SLOT_START.saturating_add(i16::try_from(main_index).unwrap_or(0));
    for (slot, before, after) in [
        (main_slot, &before_main, &after_main),
        (offhand_slot, &before_offhand, &after_offhand),
    ] {
        if before == after {
            continue;
        }
        s.menu.inventory_state_id = crate::menu::increment_state_id(s.menu.inventory_state_id);
        s.conn.send(
            ids::play::cb::CONTAINER_SET_SLOT,
            &crate::menu::container_set_slot_stack_body(
                INVENTORY_MENU_CONTAINER_ID,
                s.menu.inventory_state_id,
                slot,
                after,
                &encoding(s),
            )?,
        )?;
    }
    Ok(())
}

/// `Player.aiStep` offsets 77-252 for this connection's player — see
/// [`crate::pickup`] — and the `container_set_slot`s the next
/// `ServerPlayer.tick`'s `broadcastChanges` sends for what it changed.
///
/// Under [`crate::broadcast::tick_lock`], so it runs between two clock
/// ticks and never inside one, which is where vanilla's connection tick is.
fn touch_items(s: &mut Session<'_>) {
    let _tick = crate::broadcast::tick_lock();
    let entities = crate::entities::Entities::shared();
    let Some(me) = entities.get(s.conn.entity_id()) else {
        return;
    };
    let players = crate::status::tracked_players(s.players, entities);
    let who = crate::pickup::Toucher {
        entity: &me,
        uuid: me.uuid,
        crouching: s.player.shift_key_down,
        infinite_materials: s.creative,
        spectator: false,
    };
    let touched = crate::pickup::touch(
        crate::itementity::ItemEntities::shared(),
        &who,
        s.hotbar,
        entities,
        crate::tracker::Tracker::shared(),
        &players,
    );
    if !touched.slots.is_empty() {
        queue_inventory_sweep(s, &touched.slots);
    }
}

/// `AbstractContainerMenu.broadcastChanges()` for the slots a tick-side
/// inventory change wrote, deferred to the menu sweep.
///
/// With nothing open, `containerMenu` is the `InventoryMenu`: one
/// `container_set_slot` per changed slot on window 0, in **menu-slot**
/// order (the main inventory 9..36, then the hotbar at 36..45, then the
/// offhand at 45), each with the next state id. With a container open, it is
/// that menu's own sweep — `crate::click::broadcast_changes` over its belief
/// table — and its window and state id.
fn queue_inventory_sweep(s: &mut Session<'_>, changed: &[usize]) {
    if let Some(shared) = s.menu.open.clone() {
        let mut guard = shared.lock().expect("open menu lock");
        let open = &mut *guard;
        let (pos, second_pos) = (open.pos, open.second);
        let replies = crate::blockentity::BlockEntities::shared().with_menu_pair(
            pos,
            second_pos,
            |be, second| {
                let mut slots = crate::click::Slots {
                    menu: &mut open.menu,
                    inventory: s.hotbar,
                    container: be,
                    recipes: s.recipes,
                    second,
                    env: crate::workstation::Env::default(),
                };
                crate::click::broadcast_changes(&mut slots)
            },
        );
        drop(guard);
        for reply in replies {
            if let crate::click::Reply::SetSlot {
                container_id,
                state_id,
                slot,
                stack,
            } = reply
            {
                if let Ok(body) = crate::menu::container_set_slot_body(
                    container_id,
                    state_id,
                    slot,
                    crate::menu::to_wire(&stack, s.items),
                ) {
                    s.conn
                        .defer_to_menu_sweep(ids::play::cb::CONTAINER_SET_SLOT, body);
                }
            }
        }
        return;
    }
    let mut menu_slots: Vec<(i16, usize)> = changed
        .iter()
        .filter_map(|&index| {
            let slot = if index == crate::player::SLOT_OFFHAND {
                crate::player::OFFHAND_SLOT
            } else {
                i16::try_from(crate::click::menu_slot_for_inventory_index(index)?).ok()?
            };
            Some((slot, index))
        })
        .collect();
    menu_slots.sort_unstable();
    for (slot, index) in menu_slots {
        s.menu.inventory_state_id = crate::menu::increment_state_id(s.menu.inventory_state_id);
        let wire = crate::menu::to_wire(&s.hotbar.get_item(index), s.items);
        if let Ok(body) = crate::menu::container_set_slot_body(
            INVENTORY_MENU_CONTAINER_ID,
            s.menu.inventory_state_id,
            slot,
            wire,
        ) {
            s.conn
                .defer_to_menu_sweep(ids::play::cb::CONTAINER_SET_SLOT, body);
        }
    }
}

/// `handleAttack` — see [`crate::attack`] for the ladder and for what stops
/// where.
///
/// Nothing here is `?`: an attack owes no acknowledgement (the handler's arms
/// all `return` before `ackBlockChangesUpTo`) and a malformed body is a
/// packet vanilla's own decoder would refuse, so every failure is a silent
/// return rather than a killed connection.
fn handle_attack(
    s: &Session<'_>,
    body: &[u8],
    strength: &mut crate::pvp::Strength,
    movement: Option<&crate::playermove::PlayerMovement>,
) {
    // Held for the same reason every other handler holds it: vanilla's game
    // loop is single-threaded, so the sound this may play cannot be
    // interleaved with a tick's broadcasts.
    let _tick = crate::broadcast::tick_lock();
    let Ok(target_id) = crate::attack::decode(body) else {
        return;
    };
    let entities = crate::entities::Entities::shared();
    // A mob takes the whole of `Player.attack`, not just the reach ladder.
    //
    // The mob arm is tried FIRST and returns, rather than being folded into
    // the `Target` below, because the two paths differ before they converge:
    // a mob's `invulnerable` is false where every player here is creative, so
    // `resolve` would answer `Outcome::Hurt` for it — the variant main
    // documents as "unreachable today and deliberately silent". Reaching it
    // through the same call and then branching on the variant would leave
    // that comment true and this code unreachable, which is worse than
    // either.
    if crate::mob::is_mob(target_id) {
        attack_a_mob(s, target_id, entities);
        return;
    }
    // `ServerLevel.getEntityOrPart(entityId)`. The parts half of that name is
    // the ender dragon's, which this server does not have.
    let target = entities.get(target_id).map(|e| {
        let is_item = e.kind.name == crate::entities::ITEM.name;
        crate::attack::Target {
            id: e.id,
            // Offset 118's first case. `ExperienceOrb` and `AbstractArrow`
            // are the other two kinds it names and this server creates
            // neither, so a dropped stack is the whole of the reachable set.
            invalid_to_attack: is_item,
            aabb: if is_item {
                // `EntityTypes.ITEM`'s `.sized(0.25, 0.25)`.
                crate::attack::Aabb::of_entity(e.x, e.y, e.z, 0.25, 0.25)
            } else {
                crate::attack::Aabb::of_player(e.x, e.y, e.z)
            },
            // `Player.hurtServer` offsets 11-32 read `abilities.invulnerable`,
            // which `GameType.CREATIVE` sets. The target's abilities are this
            // session's — read through the session's own flag rather than
            // through a constant, which is what let `--gamemode survival`
            // land on 2026-08-18 as a startup flag and nothing else. When a
            // PER-PLAYER mode exists this is still the line that changes, and
            // it is still a field to plumb rather than a constant to find.
            invulnerable: !is_item && s.creative,
        }
    });
    let attacker = crate::attack::Attacker {
        entity_id: s.conn.entity_id(),
        // `Entity.getEyePosition()`. The eye height is the widened `1.62f`
        // that `crate::dropitem` had to get exactly right for a different
        // reason; the reach test is far less sensitive to it, but there is
        // only one player eye height and it lives there.
        eye: (
            s.player.x,
            s.player.y + crate::dropitem::PLAYER_EYE_HEIGHT,
            s.player.z,
        ),
        creative: s.creative,
        // No connection here is ever a spectator: `crate::status::game_type()`
        // answers `creative` or `survival` and nothing else — `set_game_type`
        // refuses the other two names rather than defaulting them, precisely
        // so this line stays true.
        spectator: false,
    };
    let outcome = crate::attack::resolve(&attacker, target.as_ref());
    if outcome == crate::attack::Outcome::Hurt {
        attack_a_player(s, target_id, strength, movement);
        return;
    }
    if outcome != crate::attack::Outcome::NoDamageSound {
        return;
    }
    // `Player.playServerSideSound(SoundEvent)`, offsets 0-27:
    //
    //   level().playSound(null, getX(), getY(), getZ(), sound,
    //                     getSoundSource(), 1.0f, 1.0f)
    //
    // `aconst_null` in the entity slot, so **the attacker hears it too** —
    // the opposite of `Player.playSound`'s `aload_0`, which is what makes a
    // step sound reach everyone but the walker. Both `550` and `551` observe
    // the actor, so an implementation that excluded the attacker would
    // produce silence on the only stream those checks read.
    //
    // The position is the attacker's feet, not the eye and not the target:
    // `getX/getY/getZ`. The recorded vanilla trace has it at the attacker's
    // walk destination.
    crate::sounds::send(
        s.players,
        s.protocol_ids,
        s.sound_events,
        Some(crate::entities::Entities::shared()),
        &[crate::sounds::Sound {
            event: crate::attack::ATTACK_NODAMAGE_SOUND.to_string(),
            source: crate::sounds::SOURCE_PLAYERS,
            at: crate::sounds::Eighths::exact(s.player.x, s.player.y, s.player.z),
            pos: (s.player.x, s.player.y, s.player.z),
            volume: 1.0,
            pitch: 1.0,
            except: None,
        }],
    );
}

/// `handleAttack`'s mob arm: the reach ladder, then the whole of
/// `Player.attack`.
///
/// The reach test is `crate::attack::resolve`'s, run against the mob's own
/// bounding box, so `551-attacking-from-out-of-range` grades a mob exactly as
/// it grades a player. What differs is only the target's box — a pig is
/// `sized(0.9, 0.9)` where a player is `(0.6, 1.8)` — and that comes off
/// `MobKind` rather than from a constant here.
///
/// The `NoDamageSound` fallback is the one at `Player.attack` offset 344, and
/// it fires for a real reason on this path: a hit inside the invulnerability
/// cooldown that is no larger than the last one returns false from
/// `hurtOrSimulate`, which is the branch main's `crate::attack` could not
/// reach with only creative players in the world.
fn attack_a_mob(s: &Session<'_>, target_id: i32, entities: &crate::entities::Entities) {
    let Some(facts) = crate::mob::mob_box(target_id) else {
        return;
    };
    let attacker = crate::attack::Attacker {
        entity_id: s.conn.entity_id(),
        eye: (
            s.player.x,
            s.player.y + crate::dropitem::PLAYER_EYE_HEIGHT,
            s.player.z,
        ),
        creative: s.creative,
        spectator: false,
    };
    let target = crate::attack::Target {
        id: target_id,
        invalid_to_attack: false,
        aabb: facts,
        // `Mob` has no `abilities.invulnerable`; only a `Player` does.
        invulnerable: false,
    };
    if crate::attack::resolve(&attacker, Some(&target)) != crate::attack::Outcome::Hurt {
        return;
    }
    let tracker = crate::tracker::Tracker::shared();
    let Some(kinds) = crate::entities::shared_kind_ids() else {
        return;
    };
    let Some(damage_type) = crate::mob::combat::damage_type_id(crate::mob::combat::PLAYER_ATTACK)
    else {
        // No capture, no registry id, no packet. Sending a zero here would be
        // a plausible wrong damage type rather than a missing one.
        return;
    };
    let players = crate::status::tracked_players(s.players, entities);
    let swing = crate::attack::Swing {
        // `Player.attackStrengthTicker` is not modelled per connection yet, so
        // a swing is always fully charged. That is an ABSENCE with a named
        // value rather than a guess: `PLAYER_TICKER_FULL` is the smallest
        // ticker that clamps the scale to 1.0, so every hit this server deals
        // is a strong one and the weak/crit/sweep branches are unreachable
        // from a packet. `crate::attack`'s unit tests reach all of them, and
        // the corpus scenario grades only the strong case, which is what a
        // player who waits between swings produces in vanilla too.
        attack_strength_ticker: PLAYER_TICKER_FULL,
        attack_speed: crate::attack::PLAYER_ATTACK_SPEED,
        attack_damage: crate::attack::PLAYER_ATTACK_DAMAGE,
        attack_knockback: crate::attack::PLAYER_ATTACK_KNOCKBACK,
        item_damage_bonus: 0.0,
        sprinting: false,
        crit: crate::attack::CritConditions {
            fall_distance: 0.0,
            on_ground: true,
            on_climbable: false,
            in_water: false,
            mobility_restricted: false,
            passenger: false,
            target_is_living: true,
            sprinting: false,
        },
        sweep: crate::attack::SweepConditions {
            strong: false,
            crit: false,
            knockback_sprint: false,
            on_ground: true,
            known_movement_horizontal_sqr: 0.0,
            speed: 0.0,
            holding_sword: false,
        },
    };
    let outcome = crate::mob::attack_mob(
        target_id,
        s.conn.entity_id(),
        (s.player.x, s.player.y, s.player.z),
        &swing,
        damage_type,
        entities,
        tracker,
        &players,
        kinds,
    );
    if outcome == crate::mob::MobAttack::NoDamage {
        crate::sounds::send(
            s.players,
            s.protocol_ids,
            s.sound_events,
            Some(entities),
            &[crate::sounds::Sound {
                event: crate::attack::ATTACK_NODAMAGE_SOUND.to_string(),
                source: crate::sounds::SOURCE_PLAYERS,
                at: crate::sounds::Eighths::exact(s.player.x, s.player.y, s.player.z),
                pos: (s.player.x, s.player.y, s.player.z),
                volume: 1.0,
                pitch: 1.0,
                except: None,
            }],
        );
    }
}

/// The `attackStrengthTicker` this server hands `Player.attack`.
///
/// `getAttackStrengthScale(0.5f)` is `clamp((ticker + 0.5) / delay, 0, 1)` and
/// the delay is 5.0 at the player's `ATTACK_SPEED` of 4.0, so any ticker at or
/// above 5 clamps to 1.0. Five, not twenty: a larger number would say the same
/// thing less clearly, and this one is the boundary
/// `crate::attack::melee_tests::the_cooldown_is_five_ticks_and_four_is_not_yet_strong`
/// pins.
const PLAYER_TICKER_FULL: i32 = 5;

/// `ServerPlayer.drop(boolean)` — offsets 0-55 — throw what is held: one of
/// it for `DROP_ITEM`, the whole stack for `DROP_ALL_ITEMS`.
///
/// The `add_entity` goes out **from this handler**, not from the next tick.
///
/// That was written the other way round first, as a guess by analogy with
/// block changes — which really are recorded now and broadcast at the tick
/// boundary — and the bytecode refutes it.
/// `Level.addFreshEntity` reaches `ServerLevel$EntityCallbacks.onTrackingStart`,
/// whose first instruction is `getChunkSource().addEntity(entity)`, and
/// `ChunkMap.addEntity` calls `updatePlayers(level.players())` before it
/// returns. There is no queue. The two paths differ because
/// `ChunkHolder.blockChanged` deliberately *defers* and `onTrackingStart`
/// deliberately does not.
///
/// It still happens under `tick_lock`, held by the caller, which is what
/// stands in for vanilla's single game thread.
///
/// # The slot changes and **no `container_set_slot` is sent**
///
/// `Inventory.removeFromSelected` takes the stack off the selected slot,
/// and the very next statement is
/// `containerMenu.findSlot(inventory, selected).ifPresent(i ->
/// containerMenu.setRemoteSlot(i, inventory.getSelectedItem()))` — the
/// menu's belief is moved to the new contents, so the next
/// `broadcastChanges` has nothing to correct. The client predicted the
/// drop itself. Until 2026-10-01 this server left the slot alone, because
/// its hotbar had no counts when the drop was written; with counts, a slot
/// that is not emptied is a stack that comes back doubled when it is picked
/// up.
///
/// A player holding nothing drops nothing: `LivingEntity.drop` returns null
/// for an empty stack before it constructs anything. A spectator drops
/// nothing either — `handlePlayerAction` offsets 222-259 — and this server
/// has no spectators.
fn drop_held_item(s: &mut Session<'_>, all: bool) {
    let stack = s.hotbar.remove_from_selected(all);
    if stack.is_empty() {
        return;
    }
    spawn_and_announce_thrown_item(s, stack, true);
}

/// `Player.drop(stack, true)` reduced to what this server can carry: build the
/// item entity, insert it, and announce it to everybody tracking the thrower.
///
/// Shared by the two callers vanilla shares it between — the DROP_ITEM arm of
/// `handlePlayerAction` ([`drop_held_item`]) and `doClick`'s `THROW` arm
/// ([`handle_container_click`]) — because they really are the same call:
/// `Player.drop(ItemStack, boolean)` is `drop(itemStack, false, bl)` (offsets
/// 0-7), and both sites pass `true`, so both reach `LivingEntity.drop` with
/// `dropAround = false` and `traceItem = true`. That is the *aimed* branch,
/// which is the one [`crate::dropitem::spawn_thrown_item`] transcribes:
/// `setPickUpDelay(40)` and, for `traceItem` (`traced`), `setThrower(this)`.
/// The two no-room paths pass `drop(stack, false)` — untraced — and reach
/// the same spawner with the aimed velocity; that gap is named at both.
///
/// The stack is registered in [`crate::itementity::ItemEntities`], which is
/// what lets the item tick and be picked up again.
///
/// **The stack is not in the pairing, and that is a gap rather than a
/// simplification.** `ItemEntity`'s `DATA_ITEM` accessor moves off its
/// default the moment the constructor sets the stack, so vanilla follows
/// every item `add_entity` with a `set_entity_data` and this server sends
/// none — which is why `605`, `606` and `500` stay strict-`diverge` after
/// this. Named in [`crate::dropitem`] and in `docs/SERVER.md`.
///
/// Silent on two conditions, both of which are "this server does not know
/// enough to build the entity" rather than "vanilla would not have dropped":
/// no entity-type table, and no thrower in the entity store. Both are
/// impossible on a joined session and neither is worth taking a connection
/// down for.
fn spawn_and_announce_thrown_item(s: &Session<'_>, stack: crate::item::ItemStack, traced: bool) {
    let Some(kinds) = crate::entities::shared_kind_ids() else {
        return;
    };
    let entities = crate::entities::Entities::shared();
    let Some(thrower) = entities.get(s.conn.entity_id()) else {
        return;
    };
    let mut rng = crate::dropitem::unique_random();
    let item = crate::dropitem::spawn_thrown_item(entities, &thrower, &mut rng);
    let item_id = s.items.and_then(|t| t.id_of(stack.item()));
    let mut data = crate::itementity::ItemData::new(stack, item_id);
    data.pickup_delay = crate::itementity::THROWN_PICKUP_DELAY;
    // `createItemStackToDrop` offsets 50-57: `if (traceItem) setThrower(this)`.
    if traced {
        data.thrower = Some(thrower.uuid);
    }
    crate::itementity::ItemEntities::shared().insert(item.id, data);
    crate::tracker::Tracker::shared().add(
        &item,
        &crate::status::tracked_players(s.players, entities),
        kinds,
    );
}

/// `ServerPlayer.openMenu` — the whole of it, for the blocks that have one.
///
/// Vanilla's order, and every step of it is observable somewhere:
///
/// ```text
///   if (containerMenu != inventoryMenu) closeContainer();
///   nextContainerCounter();
///   menu = provider.createMenu(containerCounter, inventory, this);
///   if (menu == null) return;                       // counter already spent
///   send(ClientboundOpenScreenPacket(id, type, title));
///   initMenu(menu);                                 // -> the content packet
///   containerMenu = menu;
/// ```
///
/// **The counter is spent before `createMenu`, so a menu that fails to build
/// still consumes a window id.** This server has no failing provider today —
/// vanilla's is the locked-chest and blocked-chest case — but the increment
/// is placed where vanilla places it rather than where it would be tidy,
/// because the two differ exactly when something goes wrong and that is when
/// the difference would be hardest to read.
///
/// A container with an openers counter also has `startOpen` called from
/// inside its menu's constructor, which is why the increment happens here
/// between the counter and the packet rather than after the sends.
fn open_menu_for(
    s: &mut Session<'_>,
    pos: crate::interact::BlockPos,
    state: &str,
) -> io::Result<()> {
    let Some(spec) = crate::menu::menu_for_state(state) else {
        return Ok(());
    };
    // `ChestBlock.getMenuProvider` runs the combiner: a chest with a matching
    // neighbour opens one `CompoundContainer` over both halves, with a
    // different menu type, size and title. See [`crate::menu::double_chest`].
    let pair = if state.split('[').next() == Some("minecraft:chest") {
        s.blocks
            .and_then(|b| crate::menu::double_chest(state, pos, &|p| b.state_at(p)))
    } else {
        None
    };
    let spec = if pair.is_some() {
        crate::menu::DOUBLE_CHEST
    } else {
        spec
    };
    let Some(ids) = s.protocol_ids else {
        return Ok(());
    };
    let Some(menu_id) = ids.menu(spec.menu_type) else {
        eprintln!("no protocol id for menu {} — not opening", spec.menu_type);
        return Ok(());
    };
    let block = state.split('[').next().unwrap_or(state);
    let shulker = is_shulker_box(block);

    // `ShulkerBoxBlock.useWithoutItem` (offsets 35-43) asks `canOpen` and
    // skips `Player.openMenu` altogether when it says no — so a blocked box
    // spends **no** window id, unlike a provider that returns null. The
    // interaction is still `SUCCESS` either way (offset 70), which is the
    // use table's answer and is why this is reached at all.
    if shulker && !shulker_box_can_open(s, pos, state) {
        return Ok(());
    }

    // `closeContainer` on the previous menu. Nothing in the corpus opens two
    // menus without a disconnect between them, but the counter's sequence
    // depends on this running, so it is here rather than assumed away.
    //
    // **`openMenu` reaches `closeContainer`, not `doCloseContainer`** — its
    // first three offsets are `if (containerMenu != inventoryMenu)
    // closeContainer()` — so vanilla *does* send a
    // `ClientboundContainerClosePacket` on this path, unlike the serverbound
    // one. That send is still not written: no scenario opens a second menu
    // without a disconnect first, so nothing has measured it, and adding an
    // unmeasured send here would be a packet no check could confirm. Named,
    // because the two callers of `close_open_menu` are now genuinely
    // different in vanilla and the difference is one packet.
    close_open_menu(s)?;

    s.menu.counter = crate::menu::next_container_counter(s.menu.counter);
    let container_id = s.menu.counter;

    // `ChestMenu`'s constructor calls `container.startOpen`, which is what
    // makes the lid move for everybody else. A furnace's block entity does
    // not override `startOpen` and a crafting table has no block entity at
    // all, so this fires for the chest and for nothing else — which is the
    // three-way split checks 090, 470 and 480 exist to separate.
    //
    // A double chest's `CompoundContainer.startOpen` is `container1.startOpen`
    // then `container2.startOpen`: each half runs its own counter, lid event,
    // sound and recheck, first half first.
    let halves: Vec<(crate::interact::BlockPos, String)> = match (pair, s.blocks) {
        (Some((a, b)), Some(blocks)) => vec![(a, blocks.state_at(a)), (b, blocks.state_at(b))],
        _ => vec![(pos, state.to_string())],
    };
    // `RandomizableContainerBlockEntity.createMenu` offset 13:
    // `unpackLootTable(player)` runs before the menu is built, and so before
    // `ChestMenu`'s constructor calls `startOpen` below. A double chest's
    // provider unpacks each half, first half first.
    let props = s.hotbar.item_props();
    let max_stack = |id: &str| {
        props.map_or(crate::item::DEFAULT_MAX_STACK_SIZE, |p| {
            p.max_stack_size(id)
        })
    };
    for (half, _) in &halves {
        if let Some(Err(e)) = crate::blockentity::BlockEntities::shared().with_mut(*half, |be| {
            crate::lootroll::unpack(be, 0.0, &max_stack, props)
        }) {
            eprintln!("loot: the container at {half:?} was not filled: {e}");
        }
    }
    for (pos, state) in &halves {
        let (pos, state) = (*pos, state.as_str());
        if !crate::containers::has_openers_counter(block) {
            break;
        }
        let first = s.openers.increment(
            s.block_events,
            pos,
            block,
            s.conn.entity_id(),
            crate::containers::BLOCK_INTERACTION_RANGE,
        );
        // `scheduleRecheck` runs only for the first opener; a second player
        // joining an already-open chest re-arms nothing, because the pending
        // tick is already there.
        //
        // **So does the sound, and for the same reason.**
        // `ContainerOpenersCounter.incrementOpeners` guards `onOpen`,
        // `gameEvent` and `scheduleRecheck` together on `openCount++ == 0`
        // (offsets 13-39) and only `openerCountChanged` runs unconditionally
        // — which is why the lid signal goes out on every open and the sound
        // does not.
        //
        // It is played here rather than inside `Openers::increment` for the
        // reason that method takes no `Players`: the counter is a piece of
        // block-entity state and the connection registry is not its business.
        // The ordering that matters is against `open_screen` below, and the
        // recorded traces put the sound first — `490` at 6 ms for the sound
        // and 6 ms for the screen, in that order on both streams.
        if first {
            if let Some(sound) = chest_sound(pos, state) {
                crate::sounds::send(
                    s.players,
                    s.protocol_ids,
                    s.sound_events,
                    Some(crate::entities::Entities::shared()),
                    &[sound],
                );
            }
            if let Some(ticks) = s.ticks {
                crate::ticks::schedule_container_recheck(ticks, pos, state, s.clock.ticks());
            }
        }
    }

    // `ShulkerBoxBlockEntity.startOpen` (offsets 19-137) is its own counter,
    // not a `ContainerOpenersCounter`: `openCount++`, then
    // `blockEvent(pos, block, 1, openCount)` on every open, then — only when
    // the count is now 1 — the open sound. No recheck is scheduled: a
    // shulker box's lid is driven by its own ticker, not by a re-count.
    // The count shares [`crate::containers::Openers`]' storage because the
    // two have the same increment and the same wire signal, `(1, count)`.
    if shulker {
        let first = s.openers.increment(
            s.block_events,
            pos,
            block,
            s.conn.entity_id(),
            crate::containers::BLOCK_INTERACTION_RANGE,
        );
        if first {
            crate::sounds::send(
                s.players,
                s.protocol_ids,
                s.sound_events,
                Some(crate::entities::Entities::shared()),
                &[crate::sounds::shulker_box_open(
                    pos,
                    crate::sounds::nine_tenths_pitch(),
                )],
            );
        }
    }

    // The corpus positive control: the screen never opens, which is the
    // server before any menu existed. A check whose claim is "this block opens
    // a screen" must go red here. See `crate::sabotage`.
    if crate::sabotage::fires_here(crate::sabotage::Kind::Menu) {
        return Ok(());
    }
    // `getDisplayName`: the container's custom name when it has one
    // (`BaseContainerBlockEntity.getName`, offsets 1-13), else the default.
    let named = crate::blockentity::BlockEntities::shared()
        .get(pos)
        .and_then(|be| be.custom_name().cloned());
    let body = match &named {
        Some(title) => crate::menu::open_screen_titled(container_id, menu_id, title)?,
        None => crate::menu::open_screen_body(container_id, menu_id, spec.title)?,
    };
    s.conn.send(ids::play::cb::OPEN_SCREEN, &body)?;

    // `initMenu` -> `setSynchronizer` -> `sendAllDataToRemote` ->
    // `sendInitialData`. The state id is `incrementStateId()` on a menu whose
    // `stateId` is 0, so it is 1.
    //
    // The slot vector is read out of the **real storage** — the block entity
    // behind this position, the crafting grid if this is one, and all 36 of
    // the player's inventory slots. It used to be synthesised from the hotbar
    // alone with everything else empty, which was indistinguishable from the
    // truth for as long as nothing could put an item anywhere else.
    let mut menu = crate::click::Menu::new(container_id, spec);
    // `EnchantmentMenu`'s constructor, offsets 179-192: the seed slot is set
    // from `player.getEnchantmentSeed()` before `initMenu` reads it.
    if let Some(station) = spec.station {
        menu.data = station.initial_data(spec.data_slots, s.menu.enchantment_seed);
    }
    let (first_pos, second_pos) = pair.map_or((pos, None), |(a, b)| (a, Some(b)));
    let (content, data) = crate::blockentity::BlockEntities::shared().with_menu_pair(
        first_pos,
        second_pos,
        |be, second| {
            let mut slots = crate::click::Slots {
                menu: &mut menu,
                inventory: s.hotbar,
                container: be,
                recipes: s.recipes,
                second,
                env: crate::workstation::Env::default(),
            };
            let content = crate::click::send_all_data_to_remote(&mut slots);
            (content, crate::click::full_state_data(&mut slots))
        },
    );
    send_reply(s, &content)?;

    // `sendInitialData`'s tail: one `ClientboundContainerSetDataPacket` per
    // data slot, in index order, carrying the block entity's **live** values
    // — a cold furnace's four zeros, a burning one's counters, an enchanting
    // table's fresh -1 clues. A chest and a crafting table have no data slots
    // and send none.
    for reply in &data {
        send_reply(s, reply)?;
    }

    let shared = std::sync::Arc::new(std::sync::Mutex::new(crate::menu::OpenMenu {
        menu,
        pos: first_pos,
        second: second_pos,
        block: state.split('[').next().unwrap_or(state).to_string(),
    }));
    s.conn.set_open_menu(Some(std::sync::Arc::clone(&shared)));
    s.menu.open = Some(shared);
    Ok(())
}

/// The tables a stack with components needs on the wire — see
/// [`crate::menu::Encoding`].
fn encoding<'a>(s: &Session<'a>) -> crate::menu::Encoding<'a> {
    crate::menu::Encoding {
        items: s.items,
        props: s.hotbar.item_props(),
        enchantments: crate::enchantment::Registry::shared(),
    }
}

/// Write one [`crate::click::Reply`] to this connection.
///
/// The three arms are three different packets and the middle one is the trap:
/// the cursor is a `ClientboundSetCursorItemPacket`, not a
/// `container_set_slot`, and it carries no state id. See
/// [`crate::menu::set_cursor_item_body`].
fn send_reply(s: &Session<'_>, reply: &crate::click::Reply) -> io::Result<()> {
    send_reply_to(s.conn, s.items, reply)
}

/// `ServerPlayer.doTick`'s `containerMenu.broadcastChanges()` for every
/// player with a block-entity menu open — the clock thread's half.
///
/// Called from the tick beside `Players::flush_menu_sweeps`, which is the
/// same vanilla call reached from the other side: those are packets a
/// connection-thread handler queued, these are differences a block entity's
/// ticker made. Both land after the tick's block broadcast and before the
/// acknowledgements, which is the order vanilla's own recording of
/// `797-a-fed-furnace-lights-on-the-next-tick` carries — `block_update`
/// (`lit=true`), then `container_set_slot` for the fuel the same tick burned,
/// then the data slots.
///
/// Every menu with a block entity behind it is swept — a chest (both halves
/// of a double one), a hopper, a dispenser, a crafter, a lectern, the furnace
/// family. A crafting table and the six menu-local menus have no block
/// entity, and nothing but a click changes them.
pub fn sweep_open_menus(players: &crate::players::Players, items: Option<&crate::player::ItemIds>) {
    for conn in players.connections() {
        let Some(shared) = conn.open_menu() else {
            continue;
        };
        let replies = {
            let mut open = shared.lock().expect("open menu lock");
            if matches!(
                open.menu.spec.kind,
                crate::menu::MenuKind::Crafting | crate::menu::MenuKind::MenuLocal
            ) {
                continue;
            }
            let (pos, second) = (open.pos, open.second);
            crate::blockentity::BlockEntities::shared().with_menu_pair(pos, second, |be, other| {
                crate::click::sweep_container(&mut open.menu, be.as_deref(), other.as_deref())
            })
        };
        for reply in &replies {
            // A dead socket is dropped here like every other tick-side send;
            // the connection thread notices and tears the session down.
            let _ = send_reply_to(&conn, items, reply);
        }
    }
}

/// [`send_reply`] for a caller with no session — the clock thread's sweep.
///
/// # Errors
/// If the send fails.
pub fn send_reply_to(
    conn: &crate::players::Connection,
    wire_items: Option<&crate::player::ItemIds>,
    reply: &crate::click::Reply,
) -> io::Result<()> {
    use crate::click::Reply;
    let wire = |stack: &crate::item::ItemStack| crate::menu::to_wire(stack, wire_items);
    match reply {
        Reply::SetSlot {
            container_id,
            state_id,
            slot,
            stack,
        } => conn.send(
            ids::play::cb::CONTAINER_SET_SLOT,
            &crate::menu::container_set_slot_body(*container_id, *state_id, *slot, wire(stack))?,
        ),
        Reply::SetCursor { stack } => conn.send(
            ids::play::cb::SET_CURSOR_ITEM,
            &crate::menu::set_cursor_item_body(wire(stack))?,
        ),
        Reply::SetContent {
            container_id,
            state_id,
            items,
            carried,
        } => {
            let items = crate::menu::to_wire_all(items, wire_items);
            conn.send(
                ids::play::cb::CONTAINER_SET_CONTENT,
                &crate::menu::container_set_content_body(
                    *container_id,
                    *state_id,
                    &items,
                    wire(carried),
                )?,
            )
        }
        // `writeShort(value)`: the int is truncated to its low sixteen bits
        // on the wire, which is what the client then reads back.
        #[allow(clippy::cast_possible_truncation)]
        Reply::SetData {
            container_id,
            id,
            value,
        } => conn.send(
            ids::play::cb::CONTAINER_SET_DATA,
            &crate::menu::container_set_data_body(*container_id, *id, *value as i16)?,
        ),
    }
}

/// `ServerGamePacketListenerImpl.handleContainerClick`.
///
/// The decision, the storage write and the reply all happen under the block
/// entity's lock and under [`crate::broadcast::tick_lock`], because vanilla's
/// game loop is single-threaded: a second player clicking the same chest
/// between this handler's read and its write would duplicate the stack.
///
/// # The throw's item entity goes out **before** the slot correction
///
/// `doClick`'s `THROW` arm calls `Player.drop` at offset 1524, inside
/// `clicked`, and `Level.addFreshEntity` broadcasts from there with no queue
/// — see [`drop_held_item`] for the `onTrackingStart` chain that settles it.
/// `broadcastChanges` runs afterwards, at the end of `handleContainerClick`.
/// So the order on the wire is `add_entity`, then the `container_set_slot`
/// for the slot the throw emptied, and vanilla's own recording of
/// `605-container-click-throw-drops-one-item` shows exactly that, both in the
/// same millisecond.
///
/// The loop below therefore runs before the reply loop rather than after it.
/// Reversed, both scenarios would still send both packets and would still
/// diverge, on an ordering nothing else in the corpus can see.
fn handle_container_click(s: &mut Session<'_>, body: &[u8]) -> io::Result<()> {
    let _tick = crate::broadcast::tick_lock();
    let click = crate::click::decode_click(body)?;
    // A click with no menu open is dropped. Vanilla compares against
    // `containerMenu`, which is never null there — it is the player's own
    // `InventoryMenu` when nothing else is open — and this server does not
    // model that menu, so a click naming window 0 has nowhere to land. Said
    // out loud rather than shown as an inventory that silently does nothing.
    // The corpus positive control: a click that changes nothing, which is the
    // server before `handleContainerClick` moved an item. Deliberately placed
    // after the decode so that a malformed click is still a protocol error
    // under the control — the thing being removed is the effect, not the
    // parse. See `crate::sabotage`.
    if crate::sabotage::fires_here(crate::sabotage::Kind::Click) {
        return Ok(());
    }
    let Some(shared) = s.menu.open.clone() else {
        return Ok(());
    };
    let mut guard = shared.lock().expect("open menu lock");
    let open = &mut *guard;
    let pos = open.pos;
    let second_pos = open.second;
    let station = open.menu.spec.station;
    let game_time = s.clock.ticks();
    let creative = s.creative;
    let bookshelves = bookshelves_at(s, pos, station);
    let seed = &mut s.menu.enchantment_seed;
    // `FurnaceResultSlot`: every way a click removes from container slot 2
    // of a furnace-family menu reaches `checkTakeAchievements` — `onTake`
    // for PICKUP, SWAP and THROW, `onQuickCraft` then `onTake` for
    // QUICK_MOVE — and nothing can be *placed* there (`mayPlace` is false).
    // So a fall in that slot's count is exactly a take (xp-orbs).
    let furnace = matches!(
        open.menu.spec.menu_type,
        "minecraft:furnace" | "minecraft:blast_furnace" | "minecraft:smoker"
    );
    let mut result_before = 0;
    let mut result_after = 0;
    let ((replies, set_changed), effects) = crate::blockentity::BlockEntities::shared()
        .with_menu_pair(pos, second_pos, |be, second| {
            if furnace {
                result_before = be.as_deref().map_or(0, |b| b.item(2).count());
            }
            // A double chest is one `CompoundContainer`: both halves' slots,
            // first half first, and its `setChanged` marks both.
            let contents =
                |a: Option<&crate::blockentity::BlockEntity>,
                 b: Option<&crate::blockentity::BlockEntity>| {
                    a.map(|a| {
                        let mut v: Vec<crate::item::ItemStack> =
                            (0..a.size()).map(|i| a.item(i)).collect();
                        if let Some(b) = b {
                            v.extend((0..b.size()).map(|i| b.item(i)));
                        }
                        v
                    })
                };
            let before = contents(be.as_deref(), second.as_deref());
            let mut slots = crate::click::Slots {
                menu: &mut open.menu,
                inventory: s.hotbar,
                container: be,
                recipes: s.recipes,
                second,
                env: crate::workstation::Env {
                    pos: Some(pos),
                    game_time,
                    creative,
                    enchantment_seed: Some(seed),
                    enchantments: crate::enchantment::Registry::shared(),
                    bookshelves,
                    ..crate::workstation::Env::default()
                },
            };
            let replies = crate::click::handle_click(&mut slots, &click);
            if furnace {
                result_after = slots.container.as_deref().map_or(0, |b| b.item(2).count());
            }
            let after = contents(slots.container.as_deref(), slots.second.as_deref());
            let set_changed = match (before, after) {
                (Some(before), Some(after)) => {
                    usize::try_from(click.slot_num).is_ok_and(|n| n < after.len())
                        || before != after
                }
                _ => false,
            };
            let effects = std::mem::take(&mut slots.env.effects);
            ((replies, set_changed), effects)
        });
    drop(guard);
    // `Slot.setChanged` -> `Container.setChanged` -> `BlockEntity.setChanged`,
    // whose offsets 5-18 are `Level.updateNeighbourForOutputSignal(pos,
    // block)`: a comparator reading the container hears the click. `doClick`
    // ends in `slot.setChanged()` on the slot it was given, and a quick move
    // or a swap that wrote into the container marks it through `set` /
    // `setByPlayer`; so the container is marked when the click named one of
    // its slots or when its contents moved, and not when only the player's
    // own slots did. Under the tick lock this handler already holds, which
    // is what a cascade from a handler runs under everywhere else.
    if set_changed {
        if let (Some(blocks), Some(signals)) = (s.blocks, crate::power::shared()) {
            let mut updates = crate::shapes::Effects::new();
            for at in std::iter::once(pos).chain(second_pos) {
                crate::comparator::update_neighbour_for_output_signal(
                    blocks,
                    signals,
                    s.shapes,
                    at,
                    &mut updates,
                );
            }
            record_updates(s, &updates);
        }
    }
    if result_after < result_before {
        award_furnace_take(s, pos);
    }
    for stack in &replies.dropped {
        spawn_and_announce_thrown_item(s, stack.clone(), true);
    }
    send_effects(s, &effects);
    for reply in &replies.replies {
        send_reply(s, reply)?;
    }
    Ok(())
}

/// Send what a workstation did that leaves the menu at once — see
/// [`crate::workstation::Effect`]. Before the slot replies, because
/// `Level.levelEvent` and `Level.playSound` broadcast at the call and
/// `broadcastChanges` runs when the handler is done.
fn send_effects(s: &Session<'_>, effects: &[crate::workstation::Effect]) {
    use crate::workstation::Effect;
    for e in effects {
        match e {
            Effect::Sound(sound) => crate::sounds::send(
                s.players,
                s.protocol_ids,
                s.sound_events,
                Some(crate::entities::Entities::shared()),
                std::slice::from_ref(sound),
            ),
            Effect::LevelEvent { event, pos, data } => {
                let conns = s.players.connections();
                if let Ok(body) = crate::interact::level_event_body(*event, *pos, *data, false) {
                    crate::playerlist::broadcast_within(
                        &conns,
                        &crate::playerlist::entity_locator(Some(
                            crate::entities::Entities::shared(),
                        )),
                        None,
                        (f64::from(pos.0), f64::from(pos.1), f64::from(pos.2)),
                        crate::playerlist::LEVEL_EVENT_RANGE,
                        ids::play::cb::LEVEL_EVENT,
                        &body,
                    );
                }
            }
        }
    }
}

/// `ServerGamePacketListenerImpl.handleRenameItem`, offsets 12-69.
///
/// One `readUtf()`. Acts only when the open menu is an anvil
/// (`instanceof AnvilMenu`, offset 21), through `AnvilMenu.setItemName`;
/// the `broadcastChanges` at the end of the `createResult` that runs is
/// performed here, and only when the name was accepted.
fn handle_rename_item(s: &mut Session<'_>, body: &[u8]) -> io::Result<()> {
    let _tick = crate::broadcast::tick_lock();
    let mut cur = body;
    let len = usize::try_from(rustmc_proto::varint::read_varint(&mut cur)?).unwrap_or(0);
    let name = String::from_utf8_lossy(cur.get(..len).unwrap_or(cur)).into_owned();
    if crate::sabotage::fires_here(crate::sabotage::Kind::Click) {
        return Ok(());
    }
    let Some(shared) = s.menu.open.clone() else {
        return Ok(());
    };
    let mut guard = shared.lock().expect("open menu lock");
    let open = &mut *guard;
    let pos = open.pos;
    let game_time = s.clock.ticks();
    let creative = s.creative;
    let replies = crate::blockentity::BlockEntities::shared().with_menu(pos, |be| {
        let mut slots = crate::click::Slots {
            menu: &mut open.menu,
            inventory: s.hotbar,
            container: be,
            recipes: s.recipes,
            second: None,
            env: crate::workstation::Env {
                pos: Some(pos),
                game_time,
                creative,
                enchantments: crate::enchantment::Registry::shared(),
                ..crate::workstation::Env::default()
            },
        };
        if crate::workstation::anvil_set_item_name(&mut slots, &name) {
            crate::click::broadcast_changes(&mut slots)
        } else {
            Vec::new()
        }
    });
    drop(guard);
    for reply in &replies {
        send_reply(s, reply)?;
    }
    Ok(())
}

/// `ServerGamePacketListenerImpl.handleContainerButtonClick`.
///
/// The body is `ByteBufCodecs.CONTAINER_ID` then `ByteBufCodecs.VAR_INT`.
/// Everything after the decode is [`crate::click::handle_button_click`].
fn handle_container_button_click(s: &mut Session<'_>, body: &[u8]) -> io::Result<()> {
    let _tick = crate::broadcast::tick_lock();
    let mut cur = body;
    let container_id = rustmc_proto::varint::read_varint(&mut cur)?;
    let button = rustmc_proto::varint::read_varint(&mut cur)?;
    // The same positive control as a click: the press changes nothing.
    if crate::sabotage::fires_here(crate::sabotage::Kind::Click) {
        return Ok(());
    }
    let Some(shared) = s.menu.open.clone() else {
        return Ok(());
    };
    let mut guard = shared.lock().expect("open menu lock");
    let open = &mut *guard;
    let (pos, station) = (open.pos, open.menu.spec.station);
    let game_time = s.clock.ticks();
    let creative = s.creative;
    let bookshelves = bookshelves_at(s, pos, station);
    let seed = &mut s.menu.enchantment_seed;
    let (replies, effects) = crate::blockentity::BlockEntities::shared().with_menu(pos, |be| {
        let mut slots = crate::click::Slots {
            menu: &mut open.menu,
            inventory: s.hotbar,
            container: be,
            recipes: s.recipes,
            second: None,
            env: crate::workstation::Env {
                pos: Some(pos),
                game_time,
                creative,
                enchantment_seed: Some(seed),
                enchantments: crate::enchantment::Registry::shared(),
                bookshelves,
                ..crate::workstation::Env::default()
            },
        };
        let out = crate::click::handle_button_click(&mut slots, container_id, button);
        (
            out,
            (
                std::mem::take(&mut slots.env.effects),
                slots.env.experience_dirty.then_some(slots.env.levels_spent),
            ),
        )
    });
    drop(guard);
    let (effects, levels_spent) = effects;
    send_effects(s, &effects);
    for reply in &replies {
        send_reply(s, reply)?;
    }
    if let Some(levels) = levels_spent {
        s.conn
            .with_experience(|e| e.on_enchantment_performed(levels));
        send_experience(s)?;
    }
    Ok(())
}

/// `AbstractFurnaceBlockEntity.awardUsedRecipesAndPopExperience(player)` —
/// the orbs it pops, at the player's feet, inside the click and therefore
/// **before** `broadcastChanges`' slot corrections, as a throw's item is.
fn award_furnace_take(s: &Session<'_>, pos: crate::interact::BlockPos) {
    let (Some(kinds), Some(recipes)) = (crate::entities::shared_kind_ids(), s.recipes) else {
        return;
    };
    let entities = crate::entities::Entities::shared();
    let Some(me) = entities.get(s.conn.entity_id()) else {
        return;
    };
    let players = crate::status::tracked_players(s.players, entities);
    crate::xporb::award_furnace(
        &crate::xporb::Level {
            entities,
            tracker: crate::tracker::Tracker::shared(),
            players: &players,
            kinds,
        },
        pos,
        crate::collide::Vec3::new(me.x, me.y, me.z),
        recipes,
    );
}

/// `ClientboundSetExperiencePacket` out of the player's one experience store,
/// sent at once after an enchant: `onEnchantmentPerformed`'s `lastSentExp =
/// -1` guarantees the next `doTick` sends it, and the menu's scenarios were
/// recorded with it beside the button's replies. A creative player at level
/// 0 is clamped back to `(0.0, 0, 0)` — the bytes this sent before the store
/// was the player's.
fn send_experience(s: &Session<'_>) -> io::Result<()> {
    match s.conn.with_experience(crate::experience::Experience::sync) {
        Some((progress, level, total)) => s.conn.send(
            ids::play::cb::SET_EXPERIENCE,
            &crate::experience::set_experience_body(progress, total, level)?,
        ),
        None => Ok(()),
    }
}

/// The enchanting table's bookshelf count around `pos`, or 0 for any other
/// menu — see [`crate::workstation::count_bookshelves`].
fn bookshelves_at(
    s: &Session<'_>,
    pos: crate::interact::BlockPos,
    station: Option<crate::workstation::Station>,
) -> i32 {
    match (station, s.blocks) {
        (Some(crate::workstation::Station::Enchanting), Some(blocks)) => {
            crate::workstation::count_bookshelves(pos, &|p| blocks.state_at(p))
        }
        _ => 0,
    }
}

/// `ServerGamePacketListenerImpl.handleAnimate`, offsets 0-30.
///
/// The handler is `resetLastActionTime` and `player.swing(packet.getHand())`,
/// and everything interesting is inside `swing`:
///
/// * the **swallow** — a second swing inside the first half of the running
///   animation puts nothing on the wire, see [`crate::entities::swing`];
/// * the send is `sendToTrackingPlayers`, **not** `...AndSelf`, so the
///   swinging player never sees their own `animate`;
/// * the action is 0 for the main hand and **3** for the off hand, while the
///   hand's own ordinal on the serverbound packet is 0 and 1.
///
/// Under the tick lock, because the swallow reads `swingTime`, which the tick
/// loop writes.
///
/// A body that does not decode is dropped rather than propagated: this is the
/// one packet in this loop whose only effect is an animation, and taking the
/// connection down for a malformed one would cost every later scenario on it.
fn handle_swing(s: &Session<'_>, body: &[u8]) {
    let _tick = crate::broadcast::tick_lock();
    let Ok(hand) = rustmc_proto::varint::read_varint(&mut io::Cursor::new(body)) else {
        return;
    };
    // `readEnum` on `InteractionHand`, which has two constants. Anything else
    // is a packet vanilla's own decoder would refuse.
    let Ok(hand) = u8::try_from(hand) else { return };
    if hand > 1 {
        return;
    }
    let id = s.conn.entity_id();
    let swung = crate::entities::Entities::shared()
        .modify(id, |e| crate::entities::swing(e, hand))
        .unwrap_or(false);
    if !swung {
        return;
    }
    let Ok(body) = crate::tracker::animate_body(id, crate::entities::animate_action_for_hand(hand))
    else {
        return;
    };
    let players = crate::status::tracked_players(s.players, crate::entities::Entities::shared());
    crate::tracker::Tracker::shared().send_to_tracking_players(
        id,
        &players,
        ids::play::cb::ANIMATE,
        &body,
    );
}

/// `ServerPlayer.teleportTo` on the same level, for a teleport a command
/// queued: `ServerGamePacketListenerImpl.teleport(PositionMoveRotation,
/// relatives)`.
///
/// ```text
///    0-5   awaitingTeleportTime = tickCount
///   10-27  if (++awaitingTeleport == Integer.MAX_VALUE) awaitingTeleport = 0
///   31-36  player.teleportSetPosition(pos, relatives)
///   41-47  awaitingPositionFromClient = player.position()
///   52-60  send(ClientboundPlayerPositionPacket.of(awaitingTeleport, pos, relatives))
/// ```
///
/// `teleportSetPosition` is `PositionMoveRotation.calculateAbsolute`: a
/// relative axis is the current value plus the delta, and the pitch is
/// clamped to [-90, 90]. The packet carries the values as given — deltas on
/// the relative axes — and the bitmask, so the client does the same
/// arithmetic on its side. Delta movement is `Vec3.ZERO`, which a
/// `TeleportTransition` built by `Entity.teleportTo` always carries.
///
/// The entity store takes the new position at once, as `setPos` does, so the
/// tracker's next pass tells everyone else.
fn command_teleport(s: &mut Session<'_>, t: crate::players::PendingTeleport) -> io::Result<()> {
    use crate::cmdexec::relative as rel;
    if t.rotation_only {
        return command_rotation(s, t);
    }
    let abs = |bit: i32, v: f64, cur: f64| if t.relatives & bit != 0 { cur + v } else { v };
    s.player.x = abs(rel::X, t.pos[0], s.player.x);
    s.player.y = abs(rel::Y, t.pos[1], s.player.y);
    s.player.z = abs(rel::Z, t.pos[2], s.player.z);
    let yaw = if t.relatives & rel::Y_ROT != 0 {
        s.player.yaw + t.y_rot
    } else {
        t.y_rot
    };
    let pitch = if t.relatives & rel::X_ROT != 0 {
        s.player.pitch + t.x_rot
    } else {
        t.x_rot
    };
    s.player.yaw = yaw;
    s.player.pitch = pitch.clamp(-90.0, 90.0);
    let id = if s.player.awaiting_teleport.wrapping_add(1) == i32::MAX {
        0
    } else {
        s.player.awaiting_teleport.wrapping_add(1)
    };
    s.player.awaiting(id, (s.player.x, s.player.y, s.player.z));
    let mut b = Vec::new();
    write_varint(&mut b, id)?;
    for v in t.pos {
        wire::write_f64(&mut b, v)?;
    }
    for _ in 0..3 {
        wire::write_f64(&mut b, 0.0)?;
    }
    wire::write_f32(&mut b, t.y_rot)?;
    wire::write_f32(&mut b, t.x_rot)?;
    wire::write_i32(&mut b, t.relatives)?;
    s.conn.send(ids::play::cb::PLAYER_POSITION, &b)?;
    crate::entities::Entities::shared().update(s.conn.entity_id(), |e| {
        e.x = s.player.x;
        e.y = s.player.y;
        e.z = s.player.z;
        e.y_rot = s.player.yaw;
        e.x_rot = s.player.pitch;
        e.y_head_rot = s.player.yaw;
    });
    Ok(())
}

/// `ServerPlayer.forceSetRotation(yRot, yRelative, xRot, xRelative)` — the
/// `/rotate` half of [`command_teleport`]'s queue.
///
/// ```text
///   Player.forceSetRotation -> Entity.forceSetRotation:
///     relatives = Relative.rotation(yRelative, xRelative)
///     abs = PositionMoveRotation.calculateAbsolute(of(this),
///             of(this).withRotation(yRot, xRot), relatives)
///     setYRot(abs.yRot)        // yRot % 360
///     setYHeadRot(abs.yRot)
///     setXRot(abs.xRot)        // Math.clamp(xRot % 360, -90, 90)
///     setOldRot()
///   connection.send(new ClientboundPlayerRotationPacket(yRot, yRelative,
///                                                       xRot, xRelative))
/// ```
///
/// `calculateAbsolute` adds the change to the current value on a relative
/// axis and takes it as it stands otherwise. The packet carries the angles as
/// the command computed them — deltas on the relative axes — so the client
/// repeats the same arithmetic. No teleport id is spent: this is not
/// `ServerGamePacketListenerImpl.teleport`.
fn command_rotation(s: &mut Session<'_>, t: crate::players::PendingTeleport) -> io::Result<()> {
    use crate::cmdexec::relative as rel;
    let y_rel = t.relatives & rel::Y_ROT != 0;
    let x_rel = t.relatives & rel::X_ROT != 0;
    let yaw = if y_rel {
        s.player.yaw + t.y_rot
    } else {
        t.y_rot
    };
    let pitch = if x_rel {
        s.player.pitch + t.x_rot
    } else {
        t.x_rot
    };
    s.player.yaw = yaw % 360.0;
    s.player.pitch = (pitch % 360.0).clamp(-90.0, 90.0);
    let mut b = Vec::new();
    wire::write_f32(&mut b, t.y_rot)?;
    b.push(u8::from(y_rel));
    wire::write_f32(&mut b, t.x_rot)?;
    b.push(u8::from(x_rel));
    s.conn.send(ids::play::cb::PLAYER_ROTATION, &b)?;
    crate::entities::Entities::shared().update(s.conn.entity_id(), |e| {
        e.y_rot = s.player.yaw;
        e.x_rot = s.player.pitch;
        e.y_head_rot = s.player.yaw;
    });
    Ok(())
}

/// `ServerGamePacketListenerImpl.handleInteract`, offsets 12-220, and
/// `Player.interactOn` (31-207) for the one kind of entity this server can
/// answer: a mob.
///
/// ```text
///    20  entity = level.getEntityOrPart(packet.entityId())
///    37  player.resetLastActionTime()
///    44  player.setShiftKeyDown(packet.usingSecondaryAction())
///    55  entity == null || outside the world border  -> return
///    74  !player.isWithinEntityInteractionRange(entity.getBoundingBox(), 3.0)
///                                                    -> return
///   119  the stack in hand is not enabled            -> return
///   139  result = player.interactOn(entity, hand, location)
///   153  result is Success: PLAYER_INTERACTED_WITH_ENTITY (no advancements
///        here), and swingSource == SERVER -> player.swing(hand, true)
/// ```
///
/// `interactOn` for a non-spectator: `entity.interact(...)`; a consuming
/// result restores a creative player's count (64-101), and anything else
/// falls to `stack.interactLivingEntity` (107-203), which answers `PASS` for
/// every item a mob here can be clicked with — see `crate::mob::interact`'s
/// refusals.
fn handle_interact(s: &mut Session<'_>, body: &[u8]) -> io::Result<()> {
    let _tick = crate::broadcast::tick_lock();
    let packet = crate::mob::interact::decode_interact(body)?;
    // 44-52: the sneak bit, before anything can return.
    s.player.shift_key_down = packet.secondary;
    let entities = crate::entities::Entities::shared();
    let Some(target) = entities.get(packet.entity_id) else {
        return Ok(());
    };
    // 74-95: `AABB.distanceToSqr(getEyePosition()) < (range + 3.0)^2`.
    let Some(facts) = crate::mob::facts_of_entity(target.id).filter(|f| !f.is_player) else {
        // A player or a dropped item: `Player.interact` and
        // `ItemEntity`'s are `Entity.interact`, the leash branch this
        // server refuses. PASS, and nothing on the wire.
        return Ok(());
    };
    let eye = (
        s.player.x,
        s.player.y + crate::dropitem::PLAYER_EYE_HEIGHT,
        s.player.z,
    );
    let aabb = crate::attack::Aabb {
        min: (facts.bb.min_x, facts.bb.min_y, facts.bb.min_z),
        max: (facts.bb.max_x, facts.bb.max_y, facts.bb.max_z),
    };
    let reach = crate::attack::ENTITY_INTERACTION_RANGE + 3.0;
    if aabb.distance_to_sqr(eye) >= reach * reach {
        return Ok(());
    }
    let main = s.hotbar.held().map(str::to_string);
    let off = Some(s.hotbar.offhand())
        .filter(|o| !o.is_empty())
        .map(|o| o.item().to_string());
    let item = if packet.hand == 1 { off } else { main };
    let who = crate::mob::interact::Interactor {
        entity_id: s.conn.entity_id(),
        item: item.as_deref(),
        secondary: packet.secondary,
        creative: s.creative,
    };
    let players = crate::status::tracked_players(s.players, entities);
    let Some(answer) = crate::mob::interact::interact(target.id, &who, &players) else {
        return Ok(());
    };
    // `ItemStack.consume`: a creative player keeps the stack.
    if answer.used > 0 && !s.creative && packet.hand == 0 {
        let slot = s.hotbar.selected();
        let mut in_hand = s.hotbar.stack(slot);
        in_hand.shrink(answer.used);
        s.hotbar.set_stack(slot, in_hand);
    }
    if let Some(fill) = answer.fill {
        create_filled_result(s, packet.hand, fill);
    }
    if answer.outcome == crate::mob::interact::Outcome::Success(crate::mob::interact::Swing::Server)
    {
        swing_server(s, packet.hand, &players);
    }
    Ok(())
}

/// `ItemUtils.createFilledResult(stack, player, result, limit)`, offsets
/// 0-71, then `player.setItemInHand(hand, returned)`:
///
/// ```text
///    0  creative && limit:   !inventory.contains(result) -> inventory.add(result);
///                            return stack                (the hand is unchanged)
///   37  stack.consume(1, player)                         (no shrink in creative)
///   43  stack.isEmpty()   -> return result               (the hand becomes it)
///   52  !inventory.add(result) -> player.drop(result, false)
///   70  return stack
/// ```
///
/// `Inventory.add` sends nothing itself: the touched slot reaches the client
/// from the next `broadcastChanges` sweep — `player_add_item`'s deferral. A
/// hand that changed is swept the same way.
fn create_filled_result(s: &mut Session<'_>, hand: u8, fill: crate::mob::cow::Fill) {
    let result = crate::item::ItemStack::one(fill.result);
    if s.creative && fill.limit_creative {
        if !inventory_contains(s, &result) {
            player_add_item(s, fill.result);
        }
        return;
    }
    let mut in_hand = if hand == 1 {
        s.hotbar.offhand()
    } else {
        s.hotbar.stack(s.hotbar.selected())
    };
    if !s.creative {
        in_hand.shrink(1);
    }
    if in_hand.is_empty() {
        set_hand_and_sweep(s, hand, result);
        return;
    }
    set_hand_and_sweep(s, hand, in_hand);
    player_add_item(s, fill.result);
}

/// `Inventory.contains(ItemStack)`: any non-empty slot with the same item
/// and components — the 36 storage slots and the offhand here (no armour is
/// modelled).
fn inventory_contains(s: &Session<'_>, stack: &crate::item::ItemStack) -> bool {
    (0..crate::player::INVENTORY_SIZE)
        .map(|i| s.hotbar.stack(i))
        .chain(std::iter::once(s.hotbar.offhand()))
        .any(|x| !x.is_empty() && crate::item::ItemStack::is_same_item_same_components(&x, stack))
}

/// `player.setItemInHand(hand, stack)`, and the slot's `container_set_slot`
/// on the next window-0 sweep when it differs from what was there.
fn set_hand_and_sweep(s: &mut Session<'_>, hand: u8, stack: crate::item::ItemStack) {
    let (before, slot) = if hand == 1 {
        let b = s.hotbar.offhand();
        s.hotbar.set_offhand(stack.clone());
        (b, usize::try_from(crate::player::OFFHAND_SLOT).ok())
    } else {
        let i = s.hotbar.selected();
        let b = s.hotbar.stack(i);
        s.hotbar.set_stack(i, stack.clone());
        (b, crate::click::menu_slot_for_inventory_index(i))
    };
    let Some(slot) = slot else { return };
    if before == stack {
        return;
    }
    s.menu.inventory_state_id = crate::menu::increment_state_id(s.menu.inventory_state_id);
    let wire = crate::menu::to_wire(&stack, s.items);
    let Ok(body) = crate::menu::container_set_slot_body(
        INVENTORY_MENU_CONTAINER_ID,
        s.menu.inventory_state_id,
        i16::try_from(slot).unwrap_or(i16::MAX),
        wire,
    ) else {
        return;
    };
    s.conn
        .defer_to_menu_sweep(ids::play::cb::CONTAINER_SET_SLOT, body);
}

/// `LivingEntity.swing(hand, true)` for this session's player: the swallow,
/// then `animate` to every tracker **and to the player** —
/// `ServerChunkCache.sendToTrackingPlayersAndSelf`, trackers first.
fn swing_server(s: &Session<'_>, hand: u8, players: &[crate::tracker::TrackedPlayer]) {
    let id = s.conn.entity_id();
    let swung = crate::entities::Entities::shared()
        .modify(id, |e| crate::entities::swing(e, hand))
        .unwrap_or(false);
    if !swung {
        return;
    }
    let Ok(body) = crate::tracker::animate_body(id, crate::entities::animate_action_for_hand(hand))
    else {
        return;
    };
    crate::tracker::Tracker::shared().send_to_tracking_players(
        id,
        players,
        ids::play::cb::ANIMATE,
        &body,
    );
    let _ = s.conn.send(ids::play::cb::ANIMATE, &body);
}

/// The uuid this connection's player is known by.
///
/// `Profile::offline` is a pure function of the name, so it is recomputed
/// here rather than carried on the session: the login already derived it the
/// same way, and a second copy that could drift is worse than a hash.
fn player_uuid(s: &Session<'_>) -> u128 {
    rustmc_proto::wire::offline_uuid(s.conn.name())
}

/// `ServerGamePacketListenerImpl.updateSignText`, offsets 0-71, and the
/// block-entity half it delegates to.
///
/// # Three routes to silence, and only one of them is a missing feature
///
/// Vanilla returns without a byte on the wire when the chunk is not loaded
/// (offset 27), when the position holds no `SignBlockEntity` (offset 43), and
/// — inside `SignBlockEntity.updateSignText` — when the sign is waxed or the
/// sender is not `getPlayerWhoMayEdit()`. The last of those is the whole of
/// what `571` measures, and it is why this handler must not answer a packet
/// it accepted a moment ago.
///
/// The accepted path ends in `sendBlockUpdated(pos, state, state, 3)`, which
/// is `ChangedBlocks::record` here: **nothing is sent from this handler.**
/// The `block_update` and the `block_entity_data` both come out of
/// [`crate::broadcast::ChangedBlocks::flush`] on the next tick, in that
/// order, exactly as `ChunkHolder.broadcastChanges` produces them.
///
/// `markUpdated()` inside `setFrontText` calls `sendBlockUpdated` a second
/// time with the same three arguments. Recording the position twice in one
/// tick collapses to one entry in the section's `ShortSet`, so vanilla emits
/// one pair of packets from two calls and so does this — but the second call
/// is real and is named here rather than left looking like an omission.
fn handle_sign_update(s: &mut Session<'_>, body: &[u8]) -> io::Result<()> {
    let _tick = crate::broadcast::tick_lock();
    let update = crate::sign::decode_sign_update(body)?;
    let by = player_uuid(s);
    // `hasChunkAt` at offset 24. A position in no tile has no block entity
    // either, so the store answers `None` and the effect is the same; the
    // test is written on the world rather than skipped, because "no chunk"
    // and "no sign" are different reasons and only one of them is a gap.
    if s.blocks.is_none() {
        return Ok(());
    }
    let accepted = crate::blockentity::BlockEntities::shared().with_menu(update.pos, |be| {
        // `instanceof SignBlockEntity` at offset 40: a chest here is not an
        // error, it is a return.
        let Some(sign) = be.and_then(crate::blockentity::BlockEntity::sign_mut) else {
            return false;
        };
        sign.update_sign_text(by, update.front, &update.lines) == crate::sign::Edit::Accepted
    });
    if accepted {
        crate::broadcast::ChangedBlocks::shared().record(update.pos);
    }
    Ok(())
}

/// `SignBlock.openTextEdit` and the two packets `ServerPlayer.openTextEdit`
/// sends after it, in that order.
///
/// ```text
///   setAllowedPlayerEditor(player.getUUID())
///   send(ClientboundBlockUpdatePacket(level, sign.getBlockPos()))
///   send(ClientboundOpenSignEditorPacket(sign.getBlockPos(), isFront))
/// ```
///
/// Both go to **this connection only** — they are `connection.send`, not a
/// broadcast — which is why they are here and not in `crate::broadcast`.
///
/// The block update looks redundant beside the one the tick will broadcast
/// and is not: it is sent immediately, on the packet thread, before the tick
/// boundary, and a client that has just placed a sign needs the state before
/// it is asked to open an editor on it.
fn open_sign_editor(
    s: &Session<'_>,
    pos: crate::interact::BlockPos,
    front: bool,
) -> io::Result<()> {
    let uuid = player_uuid(s);
    let opened = crate::blockentity::BlockEntities::shared().with_menu(pos, |be| {
        match be.and_then(crate::blockentity::BlockEntity::sign_mut) {
            Some(sign) => {
                sign.set_allowed_player_editor(Some(uuid));
                true
            }
            None => false,
        }
    });
    if !opened {
        return Ok(());
    }
    if let Some(blocks) = s.blocks {
        let state = blocks.state_at(pos);
        if let Some(id) = blocks.state_id(&state) {
            s.conn.send(
                ids::play::cb::BLOCK_UPDATE,
                &crate::interact::block_update_body(pos, id)?,
            )?;
        }
    }
    let mut out = Vec::new();
    rustmc_proto::wire::write_i64(&mut out, crate::interact::pack_pos(pos))?;
    rustmc_proto::wire::write_bool(&mut out, front)?;
    s.conn.send(ids::play::cb::OPEN_SIGN_EDITOR, &out)
}

/// `ServerGamePacketListenerImpl.handleContainerClose` — offsets 0-19.
///
/// After the thread hop it is one call: `player.doCloseContainer()`.
///
/// **The packet's `containerId` is read by nothing**, and that is worth
/// stating because the handler two methods below it does the opposite:
/// `handleContainerClick` compares `containerMenu.containerId` against the
/// packet's and returns early on a mismatch (offsets 23-36), and this one has
/// no such guard. So a close naming the wrong window still closes the open
/// menu. Decoding the id to check it would be this server disagreeing with
/// vanilla about which packets take effect, so the body is not read at all.
///
/// It runs under [`crate::broadcast::tick_lock`] for
/// [`handle_container_click`]'s reason: the handback below is a read of the
/// player's inventory followed by a write to it.
///
/// # Errors
/// If a send fails.
fn handle_container_close(s: &mut Session<'_>) -> io::Result<()> {
    let _tick = crate::broadcast::tick_lock();
    close_open_menu(s)
}

/// `ServerPlayer.doCloseContainer` — offsets 0-27, all three of them.
///
/// ```text
///   containerMenu.removed(this);
///   inventoryMenu.transferState(containerMenu);
///   containerMenu = inventoryMenu;
/// ```
///
/// **No `ClientboundContainerClosePacket` is sent from here, and that is
/// vanilla rather than an omission.** That packet belongs to
/// `closeContainer` — offsets 0-18, which sends it and *then* calls this —
/// and vanilla calls `closeContainer` only when the **server** closes a menu.
/// A client closing its own screen sends `ServerboundContainerClosePacket`,
/// whose handler is `doCloseContainer()` alone, and gets nothing back.
/// `611-closing-a-crafting-table-returns-the-grid-contents` measures exactly
/// that route and observes no `container_close` on either side.
///
/// # `removed` is two methods and the order between them is observable
///
/// `AbstractContainerMenu.removed(Player)` (offsets 0-32) hands back the
/// **carried** stack and clears the cursor. `CraftingMenu.removed` calls that
/// first, then runs `access.execute((level, pos) -> clearContainer(player,
/// this.craftSlots))` — so the cursor is returned before the grid is, and a
/// full inventory would therefore lose grid contents rather than the cursor.
///
/// `craftSlots` is the 3x3 and **not** the result slot: `ResultContainer` is a
/// separate container that no `removed` clears, which is why a crafting result
/// vanishes on close and its ingredients do not.
///
/// A `ChestMenu` reaches the same `AbstractContainerMenu.removed` and has no
/// `clearContainer` call at all — its items live in the block entity and stay
/// there. That falls out here for free: [`crate::click::Menu::grid`] is empty
/// for a container menu, so the walk below is a no-op over it.
///
/// # Errors
/// If a send fails.
fn close_open_menu(s: &mut Session<'_>) -> io::Result<()> {
    let Some(shared) = s.menu.open.take() else {
        return Ok(());
    };
    // `containerMenu = inventoryMenu` happens last in vanilla, but nothing
    // between here and there can tick: this runs under the tick lock, so the
    // clock thread's sweep cannot see a half-closed menu either way.
    s.conn.set_open_menu(None);
    let mut open = shared.lock().expect("open menu lock").clone();

    // `AbstractContainerMenu.removed`, offsets 8-32: the carried stack, then
    // `setCarried(EMPTY)`. Taken before the handback rather than after,
    // because `dropOrPlaceInInventory` can write into the very slot the
    // cursor came from.
    let carried = std::mem::replace(&mut open.menu.carried, crate::item::ItemStack::empty());
    drop_or_place_in_inventory(s, carried)?;

    // `clearContainer(player, container)`, offsets 0-29: every index of the
    // container, in order, through `removeItemNoUpdate`. **Empty slots go
    // through it too** — vanilla does not test for them — and
    // `placeItemBackInInventory`'s `while (!stack.isEmpty())` is what makes
    // that free. Transcribed as the unconditional walk it is, so the shape
    // stays readable against the bytecode.
    for i in 0..open.menu.grid.len() {
        // A workstation's result slot is a different container from the ones
        // `removed` clears — `StonecutterMenu.removed` throws its result away
        // with `resultContainer.removeItemNoUpdate`, and `ItemCombinerMenu`,
        // `GrindstoneMenu`, `LoomMenu` and `CartographyTableMenu` clear only
        // their input containers — so a result left in the slot vanishes on
        // close instead of being handed back.
        if open.menu.spec.station.is_some_and(|st| st.is_result(i)) {
            continue;
        }
        let stack = std::mem::replace(&mut open.menu.grid[i], crate::item::ItemStack::empty());
        drop_or_place_in_inventory(s, stack)?;
    }

    // `InventoryMenu.transferState(containerMenu)` and then the swap to the
    // inventory menu. The state the client is left believing is the closing
    // menu's, joined onto window 0 by storage cell; whatever the handback
    // above wrote that disagrees with it is what goes out now.
    broadcast_inventory_changes(s, &open.menu)?;

    if crate::containers::has_openers_counter(&open.block) {
        // `CompoundContainer.stopOpen`: first half, then second.
        for p in std::iter::once(open.pos).chain(open.second) {
            s.openers
                .decrement(s.block_events, p, &open.block, s.conn.entity_id());
        }
    }
    // `ShulkerBoxBlockEntity.stopOpen` (offsets 0-117): `openCount--`,
    // `blockEvent(pos, block, 1, openCount)`, and once the count is at or
    // below zero the close sound — the half of the lid a chest here does not
    // play yet (see `crate::containers`), played here because the shulker's
    // is guarded by a plain count and needs nothing this server lacks.
    if is_shulker_box(&open.block) {
        s.openers
            .decrement(s.block_events, open.pos, &open.block, s.conn.entity_id());
        if s.openers.count(open.pos) <= 0 {
            crate::sounds::send(
                s.players,
                s.protocol_ids,
                s.sound_events,
                Some(crate::entities::Entities::shared()),
                &[crate::sounds::shulker_box_close(
                    open.pos,
                    crate::sounds::nine_tenths_pitch(),
                )],
            );
        }
    }
    Ok(())
}

/// `ChestBlockEntity.playSound` for one half, offsets 0-137: a `LEFT` half
/// is silent (`return` at offset 20), a `RIGHT` half sounds from the middle
/// of the pair — its centre moved half a block along
/// `ChestBlock.getConnectedDirection` — and a single chest from its own
/// centre. So a double chest makes one sound, not two, and not at either
/// block. Measured on `742`: vanilla's `chest.open` is at x = 2.0, the seam
/// between the chests at x = 1 and x = 2; this server played it at 1.5.
fn chest_sound(pos: crate::interact::BlockPos, state: &str) -> Option<crate::sounds::Sound> {
    let ty = crate::placement::property_of(state, "type");
    if ty == Some("left") {
        return None;
    }
    let mut sound = crate::sounds::chest_open(pos, crate::sounds::nine_tenths_pitch());
    if ty == Some("right") {
        // Counter-clockwise of `facing` — `getConnectedDirection` for RIGHT.
        let (dx, dz) = match crate::placement::property_of(state, "facing") {
            Some("north") => (-0.5, 0.0),
            Some("east") => (0.0, -0.5),
            Some("south") => (0.5, 0.0),
            Some("west") => (0.0, 0.5),
            _ => (0.0, 0.0),
        };
        let (x, y, z) = sound.pos;
        sound.pos = (x + dx, y, z + dz);
        sound.at = crate::sounds::Eighths::exact(x + dx, y, z + dz);
    }
    Some(sound)
}

/// Whether a block name is one of the seventeen `ShulkerBoxBlock`s — keyed on
/// the block entity type, which all seventeen share.
fn is_shulker_box(block: &str) -> bool {
    crate::blockentity::for_state(block) == Some(crate::blockentity::Kind::ShulkerBox)
}

/// `ShulkerBoxBlock.canOpen` — offsets 0-47.
///
/// ```text
///    0  if (be.getAnimationStatus() != CLOSED) return true;
///   12  AABB box = Shulker.getProgressDeltaAabb(1.0f, facing, 0.0f, 0.5f,
///                                                Vec3.atBottomCenterOf(pos))
///                        .deflate(1.0E-6);
///   41  return level.noCollision(box);
/// ```
///
/// `getProgressDeltaAabb` with those arguments is the unit box expanded half
/// a block towards `facing` and then contracted by one whole block from the
/// other side: the half-cell directly in front of the lid. So a box opens
/// when the half-block its lid would sweep holds no collision shape.
///
/// **Two gaps, both named.** The animation status is not modelled — every box
/// here is `CLOSED` between openings, which is what a box nobody has open for
/// a second is on vanilla too. And `noCollision` also asks for *entity*
/// collisions (`Entity.canBeCollidedWith`: boats, minecarts, shulkers) and the
/// world border; only blocks are consulted here. A block this server's shape
/// table cannot answer for (a neighbouring shulker box, say) refuses the
/// open rather than guessing either way.
fn shulker_box_can_open(s: &Session<'_>, pos: crate::interact::BlockPos, state: &str) -> bool {
    let (Some(blocks), Some(shapes)) = (s.blocks, crate::collide::CollisionShapes::shared()) else {
        return true;
    };
    let facing = crate::placement::property_of(state, "facing").unwrap_or("up");
    let (dx, dy, dz) = match facing {
        "down" => (0, -1, 0),
        "north" => (0, 0, -1),
        "south" => (0, 0, 1),
        "west" => (-1, 0, 0),
        "east" => (1, 0, 0),
        _ => (0, 1, 0),
    };
    // The half-cell in front of the lid, in world coordinates.
    let half = |d: i32, p: i32| -> (f64, f64) {
        let p = f64::from(p);
        match d {
            1 => (p + 1.0, p + 1.5),
            -1 => (p - 0.5, p),
            _ => (p, p + 1.0),
        }
    };
    let (x0, x1) = half(dx, pos.0);
    let (y0, y1) = half(dy, pos.1);
    let (z0, z1) = half(dz, pos.2);
    let e = 1.0e-6;
    let box_ = crate::collide::Aabb {
        min_x: x0 + e,
        min_y: y0 + e,
        min_z: z0 + e,
        max_x: x1 - e,
        max_y: y1 - e,
        max_z: z1 - e,
    };
    // The box itself is a face cell of the region's search cursor, and a
    // shulker box is one of the dynamic-shape blocks with no row in the
    // table — so it would refuse its own opening. Vanilla computes it: a box
    // whose lid is closed is `Shapes.block()` (`ShulkerBoxBlock.getShape`
    // with progress 0), which is what stands in for it here. Any *other*
    // position the table cannot answer still refuses.
    struct SelfAsBlock<'a> {
        world: crate::movement::WorldCollision<'a, dyn crate::edits::Blocks + 'a>,
        me: rustmc_data::BlockPos,
        full: u32,
    }
    impl crate::collide::CollisionBlocks for SelfAsBlock<'_> {
        fn state_id_at(&self, pos: rustmc_data::BlockPos) -> u32 {
            if pos == self.me {
                self.full
            } else {
                self.world.state_id_at(pos)
            }
        }
    }
    let Some(full) = blocks
        .state_id("minecraft:stone")
        .and_then(|id| u32::try_from(id).ok())
    else {
        return true;
    };
    let world = SelfAsBlock {
        world: crate::movement::WorldCollision(blocks),
        me: rustmc_data::BlockPos {
            x: pos.0,
            y: pos.1,
            z: pos.2,
        },
        full,
    };
    matches!(
        crate::collide::collect_block_collisions(shapes, &world, box_),
        Ok(v) if v.is_empty()
    )
}

/// `AbstractContainerMenu.dropOrPlaceInInventory(Player, ItemStack)` —
/// offsets 0-83.
///
/// Two booleans decide between dropping and handing back:
///
/// ```text
///   removed      = player.isRemoved() && getRemovalReason() != CHANGED_DIMENSION
///   disconnected = player instanceof ServerPlayer && hasDisconnected()
///   if (removed || disconnected) player.drop(stack, false);
///   else if (player instanceof ServerPlayer) getInventory().placeItemBackInInventory(stack);
/// ```
///
/// **Both are false on every path that reaches here today**, and the reason is
/// worth stating rather than assuming: this runs from a packet handler, so the
/// player is alive and still connected by construction. The day a *disconnect*
/// path calls `close_open_menu` — which vanilla does, through
/// `PlayerList.remove` — the first disjunct becomes live and this is the
/// function that has to grow it. Written as the branch it is, with both flags
/// pinned to false, so that growth is an edit to one line.
///
/// # Errors
/// If a send fails.
fn drop_or_place_in_inventory(
    s: &mut Session<'_>,
    stack: crate::item::ItemStack,
) -> io::Result<()> {
    let placed = s.hotbar.place_item_back_in_inventory(stack);
    // `placeItemBackInInventory` offsets 95-105: one
    // `ClientboundSetPlayerInventoryPacket` per slot written, sent from
    // inside the loop — so these go out **before** the window-0 corrections
    // that `broadcastChanges` produces afterwards, and vanilla's own
    // recording of `611` shows `packet:0x6C` one millisecond ahead of the
    // `container_set_slot`.
    for index in placed.touched {
        let stack = s.hotbar.stack(index);
        let slot = i32::try_from(index).unwrap_or(0);
        s.conn.send(
            ids::play::cb::SET_PLAYER_INVENTORY,
            &crate::menu::set_player_inventory_stack_body(slot, &stack, &encoding(s))?,
        )?;
    }
    if !placed.leftover.is_empty() {
        // Vanilla's no-room answer is `player.drop(stack, false)` — offsets
        // 28-38 — and this is the one place in this function that is **not**
        // a transcription.
        //
        // `drop(stack, false)` is `drop(stack, false, false)`: `dropAround`
        // false and **`traceItem` false**, which is a different velocity
        // branch from the one [`crate::dropitem::spawn_thrown_item`]
        // transcribes — that one is the aimed `traceItem = true` throw both
        // of its existing callers pass. Reaching for it here would put the
        // wrong velocity on the wire, and inventing the other branch off no
        // bytecode would be worse.
        //
        // So the item entity is spawned by the helper that exists, and the
        // gap is named: on a full inventory the returned stack appears in the
        // world with a thrown item's velocity rather than a dropped one's,
        // and — as [`spawn_and_announce_thrown_item`] already documents — it
        // carries no `set_entity_data`, so it is not visibly *that* item.
        // Losing the stack instead would be the one outcome that cannot be
        // corrected later.
        //
        // **Nothing in the gameplay corpus can reach this.** `oracle-runner`'s
        // `hold` verb sends `count = 1`, so no scenario can build a stack big
        // enough to fill 36 slots; it needs a unit test, and has one.
        spawn_and_announce_thrown_item(s, placed.leftover.clone(), false);
    }
    Ok(())
}

/// `InventoryMenu.transferState(closing)` followed by the sweep of window 0
/// that `ServerPlayer.tick` would next run — the two together, because this
/// server has no tick loop for the inventory menu and they are only ever
/// observed as one thing.
///
/// `broadcastChanges` walks the menu **in menu-slot order** and sends a
/// `container_set_slot` wherever `RemoteSlot.matches` is false, taking a fresh
/// state id per packet from `incrementStateId`. Menu-slot order is not
/// `Inventory` order — `InventoryMenu` puts main inventory at 9..36 and the
/// hotbar at 36..45 — so this iterates slots and maps each one back, rather
/// than iterating the inventory. With one changed cell the difference is
/// invisible; with two it is the order of two packets.
///
/// Slots 0..9 and 45 are skipped: they are the result slot, the 2x2 grid, the
/// armour and the offhand, none of which this server models, and
/// [`crate::click::inventory_menu_slot`] has no `Inventory` index for them.
///
/// # Errors
/// If a send fails.
fn broadcast_inventory_changes(
    s: &mut Session<'_>,
    closing: &crate::click::Menu,
) -> io::Result<()> {
    let believed = crate::click::transferred_inventory_belief(closing);
    for slot in 0..crate::click::INVENTORY_MENU_SLOTS {
        let Some(index) = crate::click::inventory_index_for_menu_slot(slot) else {
            continue;
        };
        let actual = s.hotbar.stack(index);
        if believed.get(index).is_some_and(|b| *b == actual) {
            continue;
        }
        s.menu.inventory_state_id = crate::menu::increment_state_id(s.menu.inventory_state_id);
        s.conn.send(
            ids::play::cb::CONTAINER_SET_SLOT,
            &crate::menu::container_set_slot_stack_body(
                INVENTORY_MENU_CONTAINER_ID,
                s.menu.inventory_state_id,
                i16::try_from(slot).unwrap_or(i16::MAX),
                &actual,
                &encoding(s),
            )?,
        )?;
    }
    Ok(())
}

/// `AbstractContainerMenu.containerId` for `InventoryMenu`, which is
/// **0** — `InventoryMenu`'s constructor passes it literally, and it is the
/// one container id `nextContainerCounter` never returns.
const INVENTORY_MENU_CONTAINER_ID: i32 = 0;

/// `EntityEvent.SWAP_HANDS`, read from that class's own
/// `public static final byte SWAP_HANDS = 55;`.
///
/// `LivingEntity.handleHandSwap` broadcasts it in place of the two
/// `set_equipment` entries an exchanged pair of hands would otherwise
/// produce. See [`swap_to_offhand`].
const SWAP_HANDS_ENTITY_EVENT: u8 = 55;

/// A right-click on a block face: place what the player is holding.
///
/// The rules are `BlockPlaceContext`'s, in its own order and cut down to the
/// two that decide *where*:
///
///  * click a **replaceable** block — tall grass, water, snow — and the
///    placement takes that block's own position rather than the one against
///    the face. Without this, a player who clicks the grass they are
///    standing in gets a block one step away from where they aimed;
///  * otherwise it goes one step along the clicked face's normal, and that
///    position must itself be replaceable.
///
/// What is deliberately not here: `Level.isUnobstructed`, minus the one case
/// a player hits immediately. Placing on the top face of the block you are
/// standing on targets your own feet, and vanilla refuses it; the rest of
/// entity collision needs entities.
///
/// The ack is sent either way, for the reason in [`handle_player_action`].
/// `ServerGamePacketListenerImpl.handleUseItem` — offsets 12-189.
///
/// ```text
///     12-19   if (!hasClientLoaded()) return
///     20-25   ackBlockChangesUpTo(packet.getSequence())
///     51-57   player.resetLastActionTime()
///     58-78   if (stack.isEmpty() || !stack.isItemEnabled(...)) return
///     79-95   yRot = wrapDegrees(packet.getYRot()); xRot = wrapDegrees(...)
///    97-131   if (xRot != player.getXRot() || yRot != player.getYRot())
///                 player.absSnapRotationTo(yRot, xRot)
///   134-152   result = gameMode.useItem(player, level, stack, hand)
///   154-186   if (result is Success && swingSource == SERVER)
///                 player.swing(hand, true)
/// ```
///
/// **The ack is unconditional and comes before the empty-hand check**, so a
/// right-click into the air with an empty hand still acknowledges the
/// client's block-change prediction. That is the whole of what this server
/// answers today and it is not nothing: without it a client that mispredicted
/// a block and then right-clicked air is never corrected.
///
/// # What is dispatched and what is refused
///
/// [`crate::itemprops::ItemProps::use_arm`] is the four-component ladder of
/// `Item.use`, already transcribed and asserted — 1,411 Pass, 75 Equippable,
/// 43 Consumable, 7 KineticWeapon, 1 BlocksAttacks. Every arm but `Pass`
/// needs a subsystem this server does not have: `Consumable` needs the
/// hunger and effect model that `origin/player-systems` carries (its
/// `finishUsingItem` belongs to that branch's `FoodData` and this is the join
/// point), `Equippable` needs an equipment model, and the other two need a
/// use-duration clock that ticks between `startUsingItem` and `releaseUsing`.
///
/// So the arms are **named and counted** rather than half-done. A
/// `Consumable` that swung and played no sound and fed nobody would be a
/// packet vanilla does not send, which is strictly worse than the silence.
fn handle_use_item(s: &mut Session<'_>, body: &[u8]) -> io::Result<()> {
    let _tick = crate::broadcast::tick_lock();
    let use_item = crate::interact::decode_use_item(body)?;
    // offsets 20-25, above everything else.
    s.conn.ack_block_changes_up_to(use_item.sequence);
    // offsets 79-131. `absSnapRotationTo` is not a teleport and sends no
    // packet — it writes the server's copy of the rotation — so this is
    // state and not output. It matters anyway: `getPlayerPOVHitResult`,
    // which every arm below would use, ray-traces from these.
    let y_rot = crate::interact::wrap_degrees(use_item.y_rot);
    let x_rot = crate::interact::wrap_degrees(use_item.x_rot);
    if (x_rot - s.player.pitch).abs() > f32::EPSILON || (y_rot - s.player.yaw).abs() > f32::EPSILON
    {
        s.player.yaw = y_rot;
        s.player.pitch = x_rot;
    }
    // offsets 58-78: an empty hand returns before `useItem` is reached.
    let Some(item) = s.hotbar.held() else {
        return Ok(());
    };
    // `BoatItem.use` — an `Item.use` override, not a component arm, so it
    // is dispatched by the item before the component ladder below.
    if let Some(kind) = crate::boat::kind_for_item(item) {
        use_boat(s, kind);
        return Ok(());
    }
    // `BucketItem.use` — the one `Item.use` override ported here, because it
    // needs nothing this server lacks: a ray, the world and the fluid queue.
    // See `crate::bucket`.
    if let Some(content) = crate::bucket::content_of(item) {
        use_bucket(s, content);
        return Ok(());
    }
    // `EggItem.use` — an `Item.use` override, so the component ladder below
    // reads it as `Pass`; it answers `SUCCESS` (the client swings) after
    // throwing. See `crate::mob::egg`.
    if crate::mob::egg::is_egg(item) {
        let item = item.to_string();
        let world =
            crate::tiles::Tiles::shared(crate::status::WORLD_SEED, crate::status::WORLD_RADIUS);
        if let Ok(world) = world {
            let players =
                crate::status::tracked_players(s.players, crate::entities::Entities::shared());
            let thrower = crate::mob::egg::Thrower {
                entity_id: s.conn.entity_id(),
                x: s.player.x,
                y: s.player.y,
                z: s.player.z,
                x_rot: s.player.pitch,
                y_rot: s.player.yaw,
                on_ground: s.player.on_ground,
                eye_height: crate::dropitem::PLAYER_EYE_HEIGHT,
            };
            crate::mob::breed_with_players(&players, || {
                crate::mob::egg::use_egg(world, &item, thrower, &players)
            });
            // 92-96: `stack.consume(1, player)`.
            if !s.creative {
                shrink_held(s, 1);
            }
        }
        return Ok(());
    }
    let arm = s
        .hotbar
        .item_props()
        .map_or(crate::itemprops::UseArm::Pass, |p| p.use_arm(item));
    crate::itemprops::note_use_arm(arm);
    Ok(())
}

/// `BoatItem.use` through `addFreshEntity`: see [`crate::boat::use_item`].
/// A refusal of any kind sends nothing — `PASS` and `FAIL` both leave the
/// world alone and the handler has already acknowledged the sequence.
fn use_boat(s: &Session<'_>, kind: crate::entities::EntityKind) {
    let (Some(blocks), Some(collision), Some(kinds)) = (
        s.blocks,
        crate::collide::CollisionShapes::shared(),
        crate::entities::shared_kind_ids(),
    ) else {
        return;
    };
    let entities = crate::entities::Entities::shared();
    let viewer = crate::boat::Viewer {
        entity_id: s.conn.entity_id(),
        x: s.player.x,
        y: s.player.y,
        z: s.player.z,
        y_rot: s.player.yaw,
        x_rot: s.player.pitch,
        creative: s.creative,
    };
    let Ok(boat) = crate::boat::use_item(blocks, collision, kind, viewer, entities) else {
        return;
    };
    entities.insert(boat.clone());
    crate::boat::track(&boat);
    let players = crate::status::tracked_players(s.players, entities);
    crate::tracker::Tracker::shared().add(&boat, &players, kinds);
}

/// The session half of `BucketItem.use`: the eye, the reach, the shape and
/// props tables, and what to send. The rule itself is
/// [`crate::bucket::use_bucket`].
fn use_bucket(s: &mut Session<'_>, content: crate::fluid::Kind) {
    let Some(blocks) = s.blocks else { return };
    let props = blocks.place_props();
    let replaceable = |state: &str| {
        props
            .and_then(|t| t.get(state))
            .is_some_and(|p| p.replaceable)
    };
    let solid = |state: &str| blocks.solid_of_state(state);
    let facts = crate::bucket::StateFacts {
        replaceable: &replaceable,
        solid: &solid,
    };
    let collision = crate::collide::CollisionShapes::shared();
    let boxes = |p: crate::interact::BlockPos| -> Vec<[f64; 6]> {
        let state = blocks.state_at(p);
        let (Some(table), Some(id)) = (collision, blocks.state_id(&state)) else {
            return Vec::new();
        };
        u32::try_from(id)
            .ok()
            .and_then(|id| table.get(id))
            .map_or_else(Vec::new, crate::collide::Shape::cell_boxes)
    };
    let reach = crate::bucket::REACH
        + if s.creative {
            crate::bucket::CREATIVE_REACH_BONUS
        } else {
            0.0
        };
    let used = crate::bucket::use_bucket(
        blocks,
        s.shapes,
        &facts,
        &boxes,
        content,
        [
            s.player.x,
            s.player.y + crate::bucket::EYE_HEIGHT,
            s.player.z,
        ],
        (s.player.pitch, s.player.yaw),
        reach,
        s.player.shift_key_down,
        s.creative,
    );
    // The level events (a fizz, a 2001 from a replaced plant) and the block
    // positions, through the one path every handler uses.
    record_updates(s, &used.effects);
    let mut sounds = used.effects.sounds.clone();
    if let Some(event) = used.fill_sound {
        // `player.playSound(sound, 1.0f, 1.0f)` is `level.playSound(this,
        // getX(), getY(), getZ(), sound, getSoundSource(), ...)` — at the
        // player's own position, `SoundSource.PLAYERS`.
        sounds.push(crate::sounds::Sound {
            event: event.to_string(),
            source: crate::sounds::SOURCE_PLAYERS,
            at: crate::sounds::Eighths::exact(s.player.x, s.player.y, s.player.z),
            pos: (s.player.x, s.player.y, s.player.z),
            volume: 1.0,
            pitch: 1.0,
            except: None,
        });
    }
    for sound in &mut sounds {
        sound.except = Some(s.conn.id());
    }
    crate::sounds::send(
        s.players,
        s.protocol_ids,
        s.sound_events,
        Some(crate::entities::Entities::shared()),
        &sounds,
    );
    if let Some(item) = used.held_after {
        let slot = s.hotbar.selected();
        s.hotbar.set(slot, Some(item));
    }
    if let Some(item) = used.creative_result {
        creative_filled_result(s, item);
    }
}

/// `ItemUtils.createFilledResult(stack, player, result)` for a player with
/// infinite materials (offsets 1-36): `if (!inventory.contains(result))
/// inventory.add(result)`, and the hand keeps the stack it used.
///
/// `Inventory.contains` is `isSameItemSameComponents` over every slot; no
/// bucket this server hands out carries a component, so the item id is the
/// whole comparison. The slot reaches the client through
/// [`player_add_item`]'s deferred sweep, as the flower pot's hand-back does.
fn creative_filled_result(s: &mut Session<'_>, result: &str) {
    let held = |st: crate::item::ItemStack| !st.is_empty() && st.item() == result;
    let contains = (0..crate::player::INVENTORY_SIZE).any(|i| held(s.hotbar.stack(i)))
        || held(s.hotbar.offhand());
    if !contains {
        player_add_item(s, result);
    }
}

fn handle_use_item_on(s: &mut Session<'_>, body: &[u8]) -> io::Result<()> {
    let _tick = crate::broadcast::tick_lock();
    let use_on = crate::interact::decode_use_item_on(body)?;
    // **The block gets first refusal, and the item only afterwards.**
    // `ServerPlayerGameMode.useItemOn` runs `useItemOn` then
    // `useWithoutItem` on the clicked block and only reaches the placement
    // path if both declined — see `crate::useblock` for the whole order and
    // for the three gates around it that are not obvious. This handler used
    // to place immediately, which is not a missing feature but an inversion:
    // right-clicking a lever with a block in hand flips the lever on vanilla
    // and placed the block here.
    //
    // # THE FIRST OF THOSE TWO IS NOT IMPLEMENTED, AND IT IS NOT AN ABSENCE
    //
    // Everything below asks `useWithoutItem` — through `use_gate`, then
    // `hand_ported`, then `use_without_item` off `use_without_item.json`.
    // **`BlockState.useItemOn`, offset 145 and the one that runs first, has
    // no arm here at all.** `data/vanilla/use_item_on.json` exists and no
    // module opens it.
    //
    // For most blocks that costs nothing, because the rule declines and
    // vanilla falls through to the same place this does. For one it is a
    // wrong write rather than a missing one:
    //
    // ```text
    //   right-click minecraft:tnt holding minecraft:flint_and_steel
    //     vanilla   TntBlock.useItemOn primes the TNT — the block becomes
    //               air, a PrimedTnt spawns, the item takes damage, SUCCESS,
    //               and offset 223 is never reached
    //     here      no useItemOn arm; `minecraft:tnt` has no row in
    //               use_without_item.json either, so the block passes and
    //               `crate::itemuse::flint_and_steel` runs — which places
    //               minecraft:fire on the face that was clicked
    // ```
    //
    // So the TNT survives and there is a fire sitting on it. The item rule is
    // ported correctly and wins by default, which is what makes this worse
    // than the un-ported cases: shears on a pumpkin does nothing here, and
    // nothing is at least not a lie about the world.
    //
    // **And the table would not have told you.** `TntBlock.useItemOn` is
    // gated on `instanceof ServerLevel` twice over — once in its own body and
    // once inside `TntBlock.prime`, which the bytecode walk in
    // `ci/server-level-gates.py` cannot see from the caller — so the row
    // reads `result: SUCCESS, consumes: true, writes: []`. A port built from
    // that row would consume the click and write nothing, which stops the
    // spurious fire and still does not prime the TNT.
    // `ci/checks/server-level-gates.toml` carries the `TntBlock` entry and
    // the `via` that names `prime`.
    //
    // **And the block gets first refusal only when you are not sneaking with
    // something in your hands.** `ServerPlayerGameMode.useItemOn`:
    //
    // ```text
    //    74-99   holdingSomething = !(mainHand.isEmpty() && offhand.isEmpty())
    //   101-118  secondary = isSecondaryUseActive() && holdingSomething
    //   126      if (secondary) goto 223
    //   131-199  useItemOn, then useWithoutItem
    //   223      the placement path
    // ```
    //
    // `Player.isSecondaryUseActive` is `isShiftKeyDown()`, three
    // instructions, so the whole gate is one bit off the input packet. The
    // jump at 126 skips **both** block calls, which is why this is a guard on
    // the whole arm rather than a test inside it: a sneaking placement never
    // asks the block anything, so it can neither be consumed by it nor sound
    // it nor open its menu.
    //
    // **`holdingSomething` reads both hands and this server has one.**
    // `Hotbar` carries the nine selection slots; there is no offhand stack
    // anywhere in this crate, so the conjunct is evaluated on the main hand
    // alone. The two disagree for exactly one case — empty main hand, full
    // offhand, sneaking — where vanilla skips the block and this does not.
    // Stated rather than approximated silently: closing it needs an offhand
    // slot, not a change here.
    let holding_something = s.hotbar.held().is_some();
    let secondary_use = s.player.shift_key_down && holding_something;
    let consumed = if let (false, Some(blocks)) = (secondary_use, s.blocks) {
        let was = blocks.state_at(use_on.pos);
        // **`useItemOn` runs first and it is the one holding the stack.**
        // `ServerPlayerGameMode.useItemOn` offset 145 asks the block with the
        // item; only a `TryEmptyHandInteraction` from that call reaches
        // `useWithoutItem` at 196, and anything that consumes returns straight
        // out without the empty-hand method being asked at all. See
        // `crate::useitemon` for which classes are answered here and why it is
        // not all 163 the table measures.
        //
        // This is the step that was missing entirely until now, and its
        // absence was not a quiet one: a flower pot could never be *filled*,
        // so the emptying below could never fire either and `790` diverged on
        // a mechanism whose second half was already in the tree.
        // **The jukebox answers before the table, because what it writes is
        // a block entity and a `level_event` rather than a replacement
        // state.**
        //
        // `JukeboxBlock.useItemOn` offsets 0-19 return `TRY_WITH_EMPTY_HAND`
        // when `HAS_RECORD` is already true — a full jukebox is *ejected* by
        // the empty-hand path, not filled again — and otherwise hand the
        // stack to `JukeboxPlayable.tryInsertIntoJukebox`, whose guards at
        // 12-59 are: the stack carries a `JUKEBOX_PLAYABLE` component, the
        // block is `minecraft:jukebox`, and `HAS_RECORD` is false. Then, on
        // the server only, `consumeAndReturn(1, player)` and
        // `JukeboxBlockEntity.setTheItem` on the returned single-item copy.
        //
        // Only the insert half is here. The eject half is
        // `JukeboxBlockEntity.popOutTheItem`, which spawns an item entity,
        // and it is left to the table rather than half-ported.
        if note_block_passes_to_placement(s, &use_on, &was) {
            false
        } else if let Some(handled) = cauldron_bucket_step(s, blocks, &use_on, &was) {
            handled
        } else if let Some(handled) = lectern_place_book_step(s, blocks, &use_on, &was) {
            handled
        } else if let Some(handled) = campfire_place_food_step(s, blocks, &use_on, &was) {
            handled
        } else if let Some(handled) = jukebox_insert_step(s, blocks, &use_on, &was) {
            handled
        } else {
            match crate::useitemon::use_item_on(s.item_uses, &was, s.hotbar.held()) {
                crate::useitemon::UsedItem::Consumed(c) => {
                    item_use_consumed(s, blocks, use_on.pos, &c);
                    true
                }
                // `PASS` or `FAIL`. The gate at 196 is `instanceof
                // TryEmptyHandInteraction` and not "did not consume", so a block
                // that declines this way skips `useWithoutItem` and goes to the
                // placement path.
                crate::useitemon::UsedItem::Pass => false,
                crate::useitemon::UsedItem::TryWithEmptyHand => {
                    use_without_item_step(s, blocks, &use_on, &was)?
                }
            }
        }
    } else {
        false
    };
    // **`ItemStack.useOn(ctx)` — offset 275, after both block calls.**
    //
    // Vanilla reaches the two ported items and `BlockItem` alike by virtual
    // dispatch on the stack; this handler had only the `BlockItem` arm,
    // inlined below, so a hoe or a flint-and-steel arrived at a placement
    // that had no block to place and wrote nothing. `crate::itemuse` is the
    // rest of that dispatch and returns `Pass` for every item it does not
    // port, which is `Item.useOn`'s own body — so the placement below runs
    // exactly when it used to, minus the two items that now consume.
    //
    // `Fail` is not `Pass`: a flint-and-steel aimed where fire cannot go
    // returns `InteractionResult.FAIL` at offset 266 and must not fall
    // through to a placement, or an item that is not a `BlockItem` would
    // start placing blocks.
    // **`BoneMealItem.useOn` — the same virtual dispatch, one arm earlier.**
    //
    // It is here rather than inside `crate::itemuse::use_on` because its
    // success is three things and that module's `Effect` is one write: bone
    // meal writes a block, *shrinks the stack*, and sends a `level_event`.
    // Widening `Effect` for it would put two fields on six other items that
    // never set them.
    //
    // The order is `growCrop`'s and is observable: the block write is inside
    // `performBonemeal` and the event is two calls above it in `useOn`, so
    // `block_update` precedes `level_event` on the wire.
    let bone_mealed = if !consumed && s.hotbar.held() == Some(crate::bonemeal::BONE_MEAL) {
        bone_meal(s, blocks_or_none(s), use_on.pos)
    } else {
        false
    };
    let consumed = consumed || bone_mealed;
    let item_used = match (consumed, s.blocks, use_on.face, s.hotbar.held()) {
        (false, Some(blocks), Some(face), Some(item)) => {
            let around = crate::itemuse::At {
                state_at: &|p| blocks.state_at(p),
                face_sturdy: &|p, f| blocks.face_sturdy(p, f),
                default_state: &|b| blocks.default_state(b),
                fire_odds: s.fire_odds,
            };
            crate::itemuse::use_on(item, use_on.pos, face, s.conn.id(), &around)
        }
        _ => crate::itemuse::UseOn::Pass,
    };
    // `SpawnEggItem.useOn`, reached by the same virtual dispatch as the
    // two items above: `crate::mob` answers `Ok(None)` for anything that
    // is not one of its eggs, which is `Item.useOn`'s PASS.
    let item_used = if matches!(item_used, crate::itemuse::UseOn::Pass) {
        spawn_egg_step(s, &use_on)
            .or_else(|| minecart_step(s, &use_on))
            .or_else(|| end_crystal_step(s, &use_on))
            .or_else(|| item_frame_step(s, &use_on))
            .unwrap_or(item_used)
    } else {
        item_used
    };
    // `ArmorStandItem.useOn`, the same dispatch.
    let item_used = if matches!(item_used, crate::itemuse::UseOn::Pass) {
        armor_stand_step(s, &use_on).unwrap_or(item_used)
    } else {
        item_used
    };
    let item_consumed = !matches!(item_used, crate::itemuse::UseOn::Pass);
    if let (crate::itemuse::UseOn::Success(effect), Some(blocks)) = (item_used, s.blocks) {
        // `Level.setBlock(pos, state, flags)` with the flags the call site
        // carries — 11 at both — rather than `UPDATE_ALL`. The extra bit is
        // `UPDATE_IMMEDIATE`, which is a client-side re-render hint, but the
        // number is passed through rather than masked so that a call site
        // with different flags cannot silently acquire these.
        //
        // Guarded on `has_state` for the reason `crate::edits::Blocks`
        // documents: a state with no network id fails the next whole-chunk
        // encode rather than this write.
        //
        // **Sound first.** `HoeItem.useOn` plays at offset 89 and writes at
        // 102, and `FlintAndSteelItem.useOn` plays at 185 and writes at 203 —
        // both sound before they set the block, and neither order is
        // observable on the wire (a `playSound` broadcasts immediately while
        // a block change waits for the tick boundary). Written in the jar's
        // order anyway, because the alternative is a line nobody can check.
        if let Some(sound) = effect.sound {
            crate::sounds::send(
                s.players,
                s.protocol_ids,
                s.sound_events,
                Some(crate::entities::Entities::shared()),
                &[sound],
            );
        }
        if blocks.has_state(&effect.state) {
            let mut updates = crate::shapes::Effects::new();
            crate::shapes::set_block(
                blocks,
                s.shapes,
                effect.at,
                &effect.state,
                effect.flags,
                crate::shapes::RECURSION_LIMIT,
                &mut updates,
            );
            record_updates(s, &updates);
        }
    }
    if let (false, false, Some(blocks), Some(face), Some(item)) = (
        consumed,
        item_consumed,
        s.blocks,
        use_on.face,
        s.hotbar.held(),
    ) {
        // `BlockItem.useOn` — the arm of the dispatch above that stayed
        // inlined here, because it is the one every other block item takes.
        //
        // `BlockPlaceContext`'s constructor, offsets 11-47, and it is the
        // seeded `replaceClicked = true` at offset 12 that makes this two
        // contexts rather than one flag:
        //
        // ```text
        //   this.replaceClicked = true;                       // 11-13
        //   this.relativePos    = hit.getBlockPos().relative(hit.getDirection());
        //   this.replaceClicked = level.getBlockState(hit.getBlockPos())
        //                              .canBeReplaced(this);  // 33-47
        // ```
        //
        // So the clicked block is asked while `replacingClickedOnBlock()`
        // still reads **true**, and both `SlabBlock` and `SnowLayerBlock`
        // branch on it. Computing the answer first and passing it in would
        // take the wrong arm of both.
        //
        // `cursor_y` here is the raw wire offset within the clicked block,
        // which is what `getClickLocation().y - getClickedPos().getY()` is
        // during that evaluation — not `Click::cursor_y`, which is measured
        // from the block being placed into and is only decided afterwards.
        let clicked_ctx = crate::placement::Replacing {
            item: Some(item),
            face,
            cursor_y: f64::from(use_on.cursor.1),
            replacing_clicked: true,
            secondary_use: s.player.shift_key_down,
            table: s.placement,
        };
        let replace_clicked = blocks.replaceable(use_on.pos, &clicked_ctx);
        let target = if replace_clicked {
            use_on.pos
        } else {
            face.relative(use_on.pos)
        };
        // `BlockPlaceContext.canPlace()`, offsets 0-30:
        //
        // ```text
        //   return this.replaceClicked
        //       || level.getBlockState(getClickedPos()).canBeReplaced(this);
        // ```
        //
        // The short-circuit is reproduced rather than flattened. It is not an
        // optimisation: with `replaceClicked` false the context that reaches
        // `canBeReplaced` has it false too, and that is a different question
        // from the one the constructor asked.
        let can_place = replace_clicked
            || blocks.replaceable(
                target,
                &crate::placement::Replacing {
                    replacing_clicked: false,
                    ..clicked_ctx
                },
            );
        if can_place {
            // What the item places, which is **not** its own name for 14 of
            // the 1,054 block items — `minecraft:redstone` places
            // `redstone_wire`. Read from `BlockItem.getBlock()` via the
            // placement extract; without it, the old guess, which is right
            // for the other 1,040. An item absent from the map places
            // nothing, which is the right answer for a sword and is now a
            // statement rather than an accident of the name not matching.
            // A torch, a sign, a banner or a head places a **different
            // block** on a wall than on the floor, and `BlockItem.getBlock()`
            // names only the floor one. `StandingAndWallBlockItem` walks the
            // player's nearest looking directions and takes the wall block
            // for a horizontal one; the observable part of that, and the part
            // a clicked face already decides, is simply: a side face gets the
            // wall variant. Its `facing` then comes out of the placement
            // table like any other state, because `WallTorchBlock` sets it
            // from the clicked face.
            //
            // Not ported: vanilla's fallback when the wall block cannot
            // survive where it was aimed, which walks on to the next looking
            // direction and can end up placing the standing block instead.
            // That needs `Direction.orderedByNearest`, which is the same
            // pitch-sensitive family `placement.json` cannot hold, and is
            // named here rather than approximated.
            let block = match s.placement {
                Some(t) => {
                    let on_a_wall = face.axis_is_horizontal();
                    on_a_wall
                        .then(|| t.wall_block_for_item(item))
                        .flatten()
                        .or_else(|| t.block_for_item(item))
                }
                None => Some(item),
            };
            // **Two `BlockItem` subclasses do not place from `useOn` at
            // all**, and this server ran the inlined placement arm for every
            // item that names a block. `crate::blockitem::places_from_use_on`
            // has the bytecode; the short version is that
            // `PlaceOnWaterBlockItem.useOn` is an unconditional `PASS` and
            // `GameMasterBlockItem.getPlacementState` returns `null` for a
            // player who cannot use game-master blocks.
            //
            // `false` for `game_master` is the same hardcoded answer
            // `use_gate` is given further down, and for the same reason: this
            // server has no per-player ability model. The two sites now agree
            // rather than differing by omission.
            let block = block.filter(|_| {
                crate::blockitem::places_from_use_on(s.hotbar.item_props(), item, false)
            });
            if let Some(default) = block.and_then(|b| blocks.default_state(b)) {
                // `getStateForPlacement`, looked up per block in the table
                // `ci/placement-golden.sh` extracted — see `crate::placement`
                // for what it can and cannot answer.
                //
                // The cursor is measured from the corner of the block being
                // placed **into**, not of the one that was clicked: the wire
                // carries an offset within the clicked block
                // (`readBlockHitResult` adds it to that position), while
                // `StairBlock` compares against `getClickedPos()`. When the
                // placement steps along a vertical face those are different
                // blocks, and using the wrong one puts every stair placed
                // against a wall upside down.
                let click = crate::placement::Click {
                    face,
                    yaw: s.player.yaw,
                    cursor_y: f64::from(use_on.pos.1) + f64::from(use_on.cursor.1)
                        - f64::from(target.1),
                };
                // The world reader the ported `getStateForPlacement` rules
                // need. `blocks` was already in scope four lines below this;
                // what was missing was a way to hand it to `placement`.
                let around = crate::placement::Around {
                    pos: target,
                    at: &|p| blocks.state_at(p),
                    sturdy: &|p, f| blocks.face_sturdy(p, f),
                    conductor: &|p| blocks.redstone_conductor(p),
                    source: &|p| blocks.signal_source(p),
                    instrument: &|p| blocks.note_block_instrument(p),
                    state_id: &|s| blocks.state_id(s),
                };
                let state = crate::placement::state_for_placement(
                    s.placement,
                    &default,
                    click,
                    &blocks.state_at(target),
                    &|s| blocks.has_state(s),
                    Some(&around),
                );
                // The ten look-facing blocks the table flags and answers with
                // their default: `getNearestLookingDirection()` needs the
                // pitch as well as the yaw. See `crate::placement::look_facing`.
                let state = state.map(|st| {
                    crate::placement::look_facing(&st, s.player.pitch, s.player.yaw, &|x| {
                        blocks.has_state(x)
                    })
                    .unwrap_or(st)
                });
                // `CrafterBlock.getStateForPlacement` offsets 105-122:
                // `TRIGGERED = level.hasNeighborSignal(context.getClickedPos())`.
                let state = state.map(|st| match crate::power::shared() {
                    Some(signals) if crate::placement::is_crafter(&st) => {
                        let lit = crate::power::Redstone::new(blocks, signals, s.shapes)
                            .has_neighbor_signal(target);
                        crate::placement::with_property(
                            &st,
                            "triggered",
                            if lit { "true" } else { "false" },
                        )
                    }
                    _ => st,
                });
                // `None` is vanilla refusing the placement — a door with no
                // room above it, a crop on no farmland. Nothing is placed and
                // nothing is broadcast; the ack below still goes out, because
                // a refusal is exactly what the client's prediction has to be
                // corrected against.
                // **THE PLUGIN HOOK, placement half.** After
                // `getStateForPlacement` has decided *what* would be written,
                // so the event carries the state a plugin can inspect rather
                // than the item in the player's hand, and before it is
                // written, so a denial leaves the world alone.
                //
                // Both halves exist because a protection plugin that stopped
                // breaking but not placing would let anyone wall a claim in —
                // and because `grit:event.block_place` is published in
                // `crate::plugins::capabilities`, and publishing a capability
                // with no production call path is the one thing capability
                // negotiation cannot survive.
                // **`BlockItem.canPlace`, first half** — offsets 5-24:
                // `mustSurvive() && !state.canSurvive(level, clickedPos)`
                // refuses the placement outright. This server has never asked
                // it, so a torch clicked onto air was placed and a sign was
                // hung on nothing.
                //
                // The class the rule lives on comes off
                // `data/vanilla/item_survive.json` (`crate::itemsurvive`) and
                // the rule itself is
                // `rustmc_behavior::place_survive`, which is checked against
                // 640,532 of vanilla's own answers. A class it has not
                // transcribed answers `Unported`, which
                // `vanilla_would_place` reads as **allow** — refusing what
                // cannot be adjudicated would stop a scaffold or a chorus
                // flower being placed at all, and a placement that silently
                // does not happen is indistinguishable on the wire from a
                // client that never clicked.
                //
                // `isUnobstructed` — the second half, offsets 27-43 — is the
                // entity sweep in `crate::obstruct`, asked of the chosen
                // state's collision shape. It replaced a floor-column test
                // that refused a torch in your own feet (vanilla places it:
                // empty shape) and placed a block overlapping your box from
                // the next column (vanilla refuses it).
                // `FaceAttachedHorizontalDirectionalBlock.getStateForPlacement`
                // (levers, every button) walks the player's look and keeps
                // the first attachment that survives; the sampled table only
                // holds the clicked face's answer in open air — where every
                // lever and button is refused — so the walk replaces it,
                // refusal included.
                let state = if face_attached(s, &default) {
                    face_attached_placement(s, blocks, target, &default, face, replace_clicked)
                } else {
                    state
                };
                let cannot_survive = !place_survive_gate(s, blocks, target, state.as_deref());
                let obstructed = state
                    .as_deref()
                    .is_some_and(|st| !placement_unobstructed(s, blocks, target, st));
                let denied = cannot_survive
                    || obstructed
                    || state.as_deref().is_some_and(|st| {
                        crate::plugins::block_place_denied(s, target, st).is_some()
                    });
                if let Some(st) = state.filter(|_| !denied) {
                    let mut updates = crate::shapes::Effects::new();
                    // `DoubleHighBlockItem.placeBlock` writes air (or
                    // water) at the position above **before** the lower half
                    // goes down — 28 items, every door and every tall flower.
                    // The whole override, and why it is a wire difference
                    // rather than a world difference, is in
                    // `crate::blockitem::clear_above`.
                    if let Some((above, block)) = crate::blockitem::clear_above(
                        s.hotbar.item_props(),
                        item,
                        target,
                        // `Level.isWaterAt` is
                        // `getFluidState(pos).is(FluidTags.WATER)`, and
                        // `Blocks::water_at` is that question answered off
                        // `block_props.json`'s per-state `fluid` field — the
                        // only reading that gets a **waterlogged** block
                        // right, and right for both members of the tag.
                        // `FluidTags.WATER` is `{water, flowing_water}`, two
                        // **fluid** ids over the one `minecraft:water` block;
                        // the block registry has no `flowing_water`.
                        blocks.water_at((target.0, target.1 + 1, target.2)),
                    ) {
                        if let Some(fill) = blocks.default_state(block) {
                            crate::shapes::set_block(
                                blocks,
                                s.shapes,
                                above,
                                &fill,
                                crate::blockitem::CLEAR_FLAGS,
                                crate::shapes::RECURSION_LIMIT,
                                &mut updates,
                            );
                        }
                    }
                    // `BlockItem.placeBlock` writes with 11 and
                    // `BedItem.placeBlock` with 26 — no `UPDATE_NEIGHBORS`,
                    // and `UPDATE_KNOWN_SHAPE` set, so a bed's foot runs
                    // neither the neighbour update nor the shape cascade. See
                    // `crate::blockitem::place_flags` for why the general
                    // answer stays `UPDATE_ALL`.
                    let placed = crate::shapes::set_block(
                        blocks,
                        s.shapes,
                        target,
                        &st,
                        crate::blockitem::place_flags(s.hotbar.item_props(), item),
                        crate::shapes::RECURSION_LIMIT,
                        &mut updates,
                    );
                    // `aload 8` — `level.getBlockState(clickedPos)` at
                    // offset 101, read **before** `updateCustomBlockEntityTag`
                    // and `setPlacedBy` and used two hundred offsets later as
                    // the sound's `SoundType`. Captured here rather than read
                    // again at the sound, because `setPlacedBy` is entitled to
                    // write the clicked position itself and vanilla's sound
                    // would not see that write.
                    let mut sounded = blocks.state_at(target);
                    // `updateBlockStateFromTag` (offsets 106-130): only while
                    // the position still holds the placed block, the stack's
                    // `minecraft:block_state` is applied and, when it changed
                    // anything, written again with flag 2. `aload 8` is
                    // re-stored with the result, so `setPlacedBy` and the
                    // sound below both see the updated state.
                    let mut st = st;
                    if placed
                        && crate::comparator::block_of(&sounded) == crate::comparator::block_of(&st)
                    {
                        let held = s.hotbar.stack(s.hotbar.selected());
                        if let Some(props) = held.block_state_props() {
                            let updated =
                                crate::blockitem::apply_block_state_props(&sounded, &props, &|c| {
                                    blocks.has_state(c)
                                });
                            if updated != sounded {
                                crate::shapes::set_block(
                                    blocks,
                                    s.shapes,
                                    target,
                                    &updated,
                                    crate::blockitem::BLOCK_STATE_FLAGS,
                                    crate::shapes::RECURSION_LIMIT,
                                    &mut updates,
                                );
                                sounded = blocks.state_at(target);
                                st = sounded.clone();
                            }
                        }
                    }
                    // `BlockItem.place` offset 143:
                    // `updateCustomBlockEntityTag`, which for a sign item is
                    // `SignItem`'s override and ends in
                    // `SignBlock.openTextEdit(player, sign, true)` —
                    // **before** `setPlacedBy` at offset 171. The `true` is
                    // `iconst_1` at offset 81 and is not a function of where
                    // the player is standing: placing a sign always opens the
                    // front.
                    //
                    // This is the **only** thing in this server that hands
                    // out a sign's edit token, which is what makes
                    // `570`/`571`/`572` measure the verb rather than the
                    // placement: without it every `sign_update` is refused
                    // and this server is silent for the same reason vanilla
                    // is silent at a position with no sign.
                    if placed {
                        // The general arm of `updateCustomBlockEntityTag`: a
                        // stack carrying `block_entity_data` loads it into
                        // the block entity just made. See `crate::lootroll`.
                        let held = s.hotbar.stack(s.hotbar.selected());
                        if let Some((be_type, tag)) = held.block_entity_data() {
                            let ids = s.protocol_ids;
                            crate::blockentity::BlockEntities::shared().with_mut(target, |be| {
                                let kind_type =
                                    ids.and_then(|i| i.block_entity_type(be.kind().id()));
                                crate::lootroll::apply_block_entity_data(
                                    be, be_type, kind_type, tag,
                                )
                            });
                        }
                        // `BlockItem.updateBlockEntityComponents` ->
                        // `applyComponentsFromItemStack` ->
                        // `applyImplicitComponents`: a container takes the
                        // stack's `minecraft:custom_name`.
                        if let Some(name) = held.custom_name() {
                            crate::blockentity::BlockEntities::shared()
                                .with_mut(target, |be| be.set_custom_name(Some(name)));
                        }
                        // ... and its `minecraft:container`, through
                        // `ItemContainerContents.copyInto` (offsets 0-27:
                        // every slot of the block's list, from the listed
                        // position or empty).
                        if let Some(stacks) = s.items.and_then(|t| held.container_items(t)) {
                            crate::blockentity::BlockEntities::shared().with_mut(target, |be| {
                                if be.kind().is_nameable_container() {
                                    for i in 0..be.size() {
                                        be.set_item(
                                            i,
                                            stacks
                                                .get(i)
                                                .cloned()
                                                .unwrap_or_else(crate::item::ItemStack::empty),
                                        );
                                    }
                                } else if be.kind() == crate::blockentity::Kind::DecoratedPot {
                                    // `DecoratedPotBlockEntity`: `copyOne()`,
                                    // the first listed slot (offsets 26-43).
                                    be.set_item(
                                        0,
                                        stacks
                                            .first()
                                            .cloned()
                                            .unwrap_or_else(crate::item::ItemStack::empty),
                                    );
                                }
                            });
                        }
                        // ... and a pot's `minecraft:pot_decorations` (7-21).
                        if let Some(names) = s.items.and_then(|t| held.pot_decorations(t)) {
                            crate::blockentity::BlockEntities::shared()
                                .with_mut(target, |be| be.set_sherds(&names));
                        }
                        open_sign_editor(s, target, true)?;
                    }
                    // `BlockItem.place` calls `setPlacedBy` **after** the
                    // write and only if the level reads back the block it
                    // just put down, so this sits inside `placed` and after
                    // the `set_block` above rather than beside it. A door's
                    // upper half and a bed's head are written here; nothing
                    // in `getStateForPlacement` describes them. See
                    // `crate::placedby` — in particular for why the offset
                    // comes from the table instead of being `above`.
                    // The corpus positive control: `setPlacedBy` writes
                    // nothing, so a door is one block and a bed is one block.
                    // See `crate::sabotage`.
                    let structural = crate::sabotage::fires_here(crate::sabotage::Kind::Structure);
                    if placed && !structural {
                        for w in s.placed_by.map(|t| t.writes_for(&st)).unwrap_or_default() {
                            let at = (target.0 + w.at.0, target.1 + w.at.1, target.2 + w.at.2);
                            crate::shapes::set_block(
                                blocks,
                                s.shapes,
                                at,
                                &w.state,
                                w.flags,
                                crate::shapes::RECURSION_LIMIT,
                                &mut updates,
                            );
                        }
                        // `DiodeBlock.setPlacedBy`: a repeater or comparator
                        // placed with its input already live books a tick
                        // in 1. `set_placed_by.json` cannot carry it — the
                        // condition reads the level — so it is asked here.
                        if let (Some(signals), Some(ticks)) = (crate::power::shared(), s.ticks) {
                            if crate::comparator::diode_set_placed_by(
                                blocks, signals, s.shapes, target, &st,
                            ) {
                                ticks.schedule(
                                    target,
                                    crate::comparator::block_of(&st),
                                    s.clock.ticks(),
                                    crate::comparator::SET_PLACED_BY_DELAY,
                                    crate::ticks::PRIORITY_NORMAL,
                                );
                            }
                        }
                    }
                    record_updates(s, &updates);
                    // `BlockItem.place` offsets 201-240, after `setPlacedBy`
                    // at 171 and before the game event at 257.
                    //
                    // **The sound type comes from the state read back out of
                    // the level (`aload 8`), not from `st`.** The two differ
                    // whenever `updateBlockStateFromTag` changed the state or
                    // the level stored something other than what was asked
                    // for, and reading back is both what the bytecode does
                    // and one fewer thing to keep in step.
                    //
                    // Inside `if placed` because vanilla returns FAIL from
                    // offset 69 when `placeBlock` says no, and everything
                    // from 73 down — including the sound — is on the far side
                    // of that return.
                    if placed {
                        place_sound(s, target, &sounded);
                    }
                    // `BlockItem.place`, after the sound: `itemstack.consume(1,
                    // player)` — `shrink(1)` unless the player has infinite
                    // materials. Every survival rig in `ci/checks/survival`
                    // that places one block and then takes a tool from the
                    // console depends on this: `Inventory.add` fills the
                    // first free slot, and while the placed block's stack was
                    // never shrunk the tool landed in slot 1 and the digger
                    // dug bare-handed.
                    if placed && !s.creative {
                        let slot = s.hotbar.selected();
                        let mut held = s.hotbar.stack(slot);
                        held.shrink(1);
                        s.hotbar.set_stack(slot, held);
                    }
                    // `getStateForPlacement` is where a coral asks to be
                    // woken — see `crate::ticks`. After the write, because
                    // the queue is keyed by position and the position has to
                    // hold the block by the time the tick fires.
                    if placed {
                        if let Some(ticks) = s.ticks {
                            crate::ticks::on_placed(ticks, blocks, target, &st, s.clock.ticks());
                        }
                    }
                }
            }
        }
    }
    // The two resyncs vanilla sends by hand, from the tail of
    // `ServerGamePacketListenerImpl.handleUseItemOn` (offsets 585-618):
    //
    //     send(ClientboundBlockUpdatePacket(level, blockpos));
    //     send(ClientboundBlockUpdatePacket(level, blockpos.relative(dir)));
    //
    // Unconditional, on every path that reaches the end of the handler, and
    // sent whether or not anything changed — a refusal is precisely the case
    // the client's prediction has to be corrected against. The clicked
    // position first, then the one the placement would have gone into.
    //
    // Note this is the *current* state of both, read after the interaction,
    // not a record of what was written: for a refused placement both come
    // back unchanged, which is what tells the client to drop its guess.
    if let (Some(blocks), Some(face)) = (s.blocks, use_on.face) {
        for pos in [use_on.pos, face.relative(use_on.pos)] {
            let state = blocks.state_at(pos);
            if let Some(id) = blocks.state_id(&state) {
                s.conn.send(
                    ids::play::cb::BLOCK_UPDATE,
                    &crate::interact::block_update_body(pos, id)?,
                )?;
            }
        }
    }
    s.conn.ack_block_changes_up_to(use_on.sequence);
    Ok(())
}

/// The `ArmorStandItem.useOn` arm of `ItemStack.useOn`. `None` is not an
/// armor stand; otherwise the item took the click, whether or not a stand
/// came of it, and — like the spawn egg — nothing is placed as a block, so
/// the answer is `Fail` in both cases (the caller's block arm must not run).
///
/// `BlockPlaceContext.getClickedPos()` is resolved here because it needs
/// the replaceability table: the clicked position when its block
/// `canBeReplaced` with `replacingClickedOnBlock()` still true (offsets
/// 11-47 of the constructor), else the one the face points at.
fn armor_stand_step(
    s: &Session<'_>,
    use_on: &crate::interact::UseItemOn,
) -> Option<crate::itemuse::UseOn> {
    let item = s.hotbar.held()?;
    if item != "minecraft:armor_stand" {
        return None;
    }
    let (blocks, face) = (s.blocks?, use_on.face?);
    let clicked_ctx = crate::placement::Replacing {
        item: Some(item),
        face,
        cursor_y: f64::from(use_on.cursor.1),
        replacing_clicked: true,
        secondary_use: s.player.shift_key_down,
        table: s.placement,
    };
    let pos = if blocks.replaceable(use_on.pos, &clicked_ctx) {
        use_on.pos
    } else {
        face.relative(use_on.pos)
    };
    let world =
        crate::tiles::Tiles::shared(crate::status::WORLD_SEED, crate::status::WORLD_RADIUS).ok()?;
    let entities = crate::entities::Entities::shared();
    let kinds = crate::entities::shared_kind_ids()?;
    let players = crate::status::tracked_players(s.players, entities);
    let _ = crate::mob::use_armor_stand(
        world,
        rustmc_data::BlockPos {
            x: pos.0,
            y: pos.1,
            z: pos.2,
        },
        face == crate::interact::Face::Down,
        s.player.yaw,
        entities,
        crate::tracker::Tracker::shared(),
        &players,
        kinds,
    );
    Some(crate::itemuse::UseOn::Fail)
}

/// `MinecartItem.useOn`, offsets 0-266, for `minecraft:minecart`.
///
/// ```text
///    12  if (!state.is(BlockTags.RAILS)) return FAIL;
///    40  shape = block instanceof BaseRailBlock ? state.getValue(shapeProperty) : NORTH_SOUTH;
///    76  d = shape.isSlope() ? 0.5 : 0.0;
///    92  at = (x + 0.5, y + 0.0625 + d, z + 0.5);
///   160  m = AbstractMinecart.createMinecart(level, at, type, DISPENSER, stack, player);
///   175  if (useExperimentalMovement(level)) { ...refuse onto another cart... }
///        level.addFreshEntity(m); gameEvent(ENTITY_PLACE); stack.shrink(1); return SUCCESS;
/// ```
///
/// `createMinecart` sets the position and nothing else a client sees: the
/// cart is added at yaw 0 and every synched value at its definition, so the
/// pairing bundle is the `add_entity` alone — which is what vanilla's
/// recording of `a-minecart-sits-on-a-rail` shows.
///
/// **Not ported:** the experimental-movement arm (`minecart_improvements` is
/// off in a vanilla world), the `ENTITY_PLACE` game event (vibrations — no
/// listener here), and every other cart type (chest, furnace, hopper, TNT,
/// command block), each of which is this same path with a different
/// `EntityType` and its own synched data. `None` hands anything that is not
/// the plain minecart back to the caller.
///
/// **And the cart does not tick.** A cart on a flat rail with no push does
/// not move and vanilla sends nothing after the pairing; a cart that should
/// roll down a slope sits here. Measured on the check: vanilla's `add_entity`
/// at (1.5, 0.0625, -0.5) kind 85; this server sent nothing at all.
fn minecart_step(
    s: &Session<'_>,
    use_on: &crate::interact::UseItemOn,
) -> Option<crate::itemuse::UseOn> {
    if s.hotbar.held()? != "minecraft:minecart" {
        return None;
    }
    let blocks = s.blocks?;
    let state = blocks.state_at(use_on.pos);
    let block = state.split('[').next().unwrap_or(&state);
    if crate::blocktags::is_in("rails", block) != Some(true) {
        return Some(crate::itemuse::UseOn::Fail);
    }
    let base_rail = crate::placedby::PlacedByTable::shared(std::path::Path::new(
        "data/vanilla/set_placed_by.json",
    ))
    .is_ok_and(|c| c.is_a(block, "BaseRailBlock"));
    let shape = if base_rail {
        crate::placement::property_of(&state, "shape").unwrap_or("north_south")
    } else {
        "north_south"
    };
    let slope = if shape.starts_with("ascending_") {
        0.5
    } else {
        0.0
    };
    let entities = crate::entities::Entities::shared();
    let kinds = crate::entities::shared_kind_ids()?;
    let mut rng = crate::dropitem::unique_random();
    let cart = crate::entities::Entity {
        id: entities.next_id(),
        uuid: crate::dropitem::insecure_uuid(&mut rng),
        kind: crate::entities::MINECART,
        x: f64::from(use_on.pos.0) + 0.5,
        y: f64::from(use_on.pos.1) + 0.0625 + slope,
        z: f64::from(use_on.pos.2) + 0.5,
        y_rot: 0.0,
        x_rot: 0.0,
        y_head_rot: 0.0,
        movement: (0.0, 0.0, 0.0),
        on_ground: false,
        data: 0,
        swinging: false,
        swing_time: 0,
        swinging_arm: None,
        synched: crate::synched::Synched::EntityOnly,
    };
    entities.insert(cart.clone());
    crate::tracker::Tracker::shared().add(
        &cart,
        &crate::status::tracked_players(s.players, entities),
        kinds,
    );
    // `stack.shrink(1)` is not done here: this handler holds the session
    // shared, and the corpus actor is creative, for whom
    // `ServerPlayerGameMode.useItemOn` puts the count back. A survival
    // player keeps the cart — a named gap, the same one the spawn egg path
    // has.
    Some(crate::itemuse::UseOn::Fail)
}

/// `EndCrystalItem.useOn`, offsets 0-227.
///
/// ```text
///    12  if (!state.is(OBSIDIAN) && !state.is(BEDROCK)) return FAIL;
///    46  above = pos.above(); if (!level.isEmptyBlock(above)) return FAIL;
///   104  if (!level.getEntities(null, new AABB(x, y, z, x + 1, y + 2, z + 1)).isEmpty()) return FAIL;
///   160  c = new EndCrystal(level, x + 0.5, y, z + 0.5);
///   173  c.setShowBottom(false);
///   178  level.addFreshEntity(c); gameEvent(ENTITY_PLACE); dragonFight?.tryRespawn();
///   218  stack.shrink(1); return SUCCESS;
/// ```
///
/// `setShowBottom(false)` makes the value both non-default — so the pairing
/// bundle carries it — and dirty, so the crystal's first `sendChanges` sends
/// it again; [`crate::tracker::mark_data_dirty`] is that second half.
/// Measured on `an-end-crystal-sits-on-obsidian`: vanilla's bundle
/// `add_entity` kind 45 at (1.5, 1.0, -0.5) with one `set_entity_data`, and
/// a second after the block resyncs; this server sent neither.
///
/// **Not ported:** the game event, the dragon fight (the overworld has
/// none), the stack shrink (see [`minecart_step`]). `isEmptyBlock` is
/// `isAir()`. The entity query intersects [`crate::entities::dimensions`]
/// boxes, which covers every kind this server can put in the world.
fn end_crystal_step(
    s: &Session<'_>,
    use_on: &crate::interact::UseItemOn,
) -> Option<crate::itemuse::UseOn> {
    if s.hotbar.held()? != "minecraft:end_crystal" {
        return None;
    }
    let blocks = s.blocks?;
    let state = blocks.state_at(use_on.pos);
    let block = state.split('[').next().unwrap_or(&state);
    if block != "minecraft:obsidian" && block != "minecraft:bedrock" {
        return Some(crate::itemuse::UseOn::Fail);
    }
    let (x, y, z) = (use_on.pos.0, use_on.pos.1 + 1, use_on.pos.2);
    if !crate::shapes::is_air(&blocks.state_at((x, y, z))) {
        return Some(crate::itemuse::UseOn::Fail);
    }
    let (x0, y0, z0) = (f64::from(x), f64::from(y), f64::from(z));
    let entities = crate::entities::Entities::shared();
    // `AABB.intersects`: strict on both sides of every axis.
    let occupied = entities.all().iter().any(|e| {
        let Some((w, h)) = crate::entities::dimensions(e.kind) else {
            return false;
        };
        let hw = w / 2.0;
        e.x - hw < x0 + 1.0
            && e.x + hw > x0
            && e.y < y0 + 2.0
            && e.y + h > y0
            && e.z - hw < z0 + 1.0
            && e.z + hw > z0
    });
    if occupied {
        return Some(crate::itemuse::UseOn::Fail);
    }
    let kinds = crate::entities::shared_kind_ids()?;
    let mut rng = crate::dropitem::unique_random();
    let crystal = crate::entities::Entity {
        id: entities.next_id(),
        uuid: crate::dropitem::insecure_uuid(&mut rng),
        kind: crate::entities::END_CRYSTAL,
        x: x0 + 0.5,
        y: y0,
        z: z0 + 0.5,
        y_rot: 0.0,
        x_rot: 0.0,
        y_head_rot: 0.0,
        movement: (0.0, 0.0, 0.0),
        on_ground: false,
        data: 0,
        swinging: false,
        swing_time: 0,
        swinging_arm: None,
        synched: crate::synched::Synched::EndCrystal { show_bottom: false },
    };
    entities.insert(crystal.clone());
    if let Some(values) = crystal.synched.non_default_values() {
        if let Ok(body) = crate::synched::set_entity_data_body(crystal.id, &values) {
            crate::tracker::mark_data_dirty(crystal.id, body);
        }
    }
    crate::tracker::Tracker::shared().add(
        &crystal,
        &crate::status::tracked_players(s.players, entities),
        kinds,
    );
    Some(crate::itemuse::UseOn::Fail)
}

/// `HangingEntityItem.useOn` for `minecraft:item_frame` (an `ItemFrameItem`),
/// offsets 0-231, with `ItemFrame(level, pos, direction)`,
/// `ItemFrame.setDirection`, `createBoundingBox` and `survives`.
///
/// ```text
///    place = clicked.relative(face)
///    ItemFrameItem.mayPlace: level.isInsideBuildHeight(place) && player.mayUseItemAt(...)
///    frame = new ItemFrame(level, place, face)
///        setDirection: horizontal -> xRot 0, yRot 90 * get2DDataValue
///                      vertical   -> xRot -90 * step, yRot 0
///        position = atCenterOf(place).relative(face, -0.46875)
///    if (frame.survives()) { playPlacementSound(); gameEvent; addFreshEntity; shrink; SUCCESS }
///    else CONSUME
/// ItemFrame.survives: fixed || (!hasLevelCollision(popBox)
///        && (support.isSolid() || horizontal && DiodeBlock.isDiode(support))
///        && canCoexist)
/// ```
///
/// `setDirection` writes `DATA_DIRECTION`, so the value is non-default in
/// the pairing bundle and dirty for the frame's first `sendChanges` — the
/// two `set_entity_data` vanilla sends. The `add_entity` carries the block
/// position, see [`crate::entities::hanging_block_pos`].
///
/// Measured on `an-item-frame-hangs-on-a-blocks-side`: vanilla sounds
/// `entity.item_frame.place` at x 2.0 (the frame's centre, 1/32 off the
/// stone, truncated toward zero on a negative workspace), then the bundle
/// `add_entity` kind 73 at (1, 0, -1) data 4 yaw 64 and a `set_entity_data`,
/// then the second after the resyncs; this server sent nothing.
///
/// **Approximated, and named:** `mayUseItemAt` is taken as true (a creative
/// or unrestricted player), the build-height test is not made, `canCoexist`
/// is "no other frame hangs in the same block facing the same way", and a
/// collision the shape table cannot answer refuses the frame.
fn item_frame_step(
    s: &Session<'_>,
    use_on: &crate::interact::UseItemOn,
) -> Option<crate::itemuse::UseOn> {
    use crate::interact::Face;
    if s.hotbar.held()? != "minecraft:item_frame" {
        return None;
    }
    let blocks = s.blocks?;
    let face = use_on.face?;
    let place = face.relative(use_on.pos);
    let (sx, sy, sz) = face.relative((0, 0, 0));
    let inset = crate::entities::ITEM_FRAME_INSET;
    let (x, y, z) = (
        f64::from(place.0) + 0.5 - f64::from(sx) * inset,
        f64::from(place.1) + 0.5 - f64::from(sy) * inset,
        f64::from(place.2) + 0.5 - f64::from(sz) * inset,
    );
    let horizontal = !matches!(face, Face::Up | Face::Down);
    let (x_rot, y_rot) = match face {
        Face::South => (0.0, 0.0),
        Face::West => (0.0, 90.0),
        Face::North => (0.0, 180.0),
        Face::East => (0.0, 270.0),
        Face::Up => (-90.0, 0.0),
        Face::Down => (90.0, 0.0),
    };
    // `survives`: the support is the clicked block.
    let support = blocks.state_at(use_on.pos);
    let solid = blocks
        .place_props()
        .and_then(|t| t.get(&support))
        .is_some_and(|p| p.solid);
    let diode = horizontal
        && crate::placedby::PlacedByTable::shared(std::path::Path::new(
            "data/vanilla/set_placed_by.json",
        ))
        .is_ok_and(|c| c.is_a(support.split('[').next().unwrap_or(&support), "DiodeBlock"));
    // `hasLevelCollision(getPopBox())`: the 0.75 x 0.75 x 0.0625 box.
    let half = |step: i32| if step == 0 { 0.375 } else { 0.03125 };
    let box_ = crate::collide::Aabb {
        min_x: x - half(sx),
        min_y: y - half(sy),
        min_z: z - half(sz),
        max_x: x + half(sx),
        max_y: y + half(sy),
        max_z: z + half(sz),
    };
    let clear = crate::collide::CollisionShapes::shared().is_none_or(|shapes| {
        matches!(
            crate::collide::collect_block_collisions(shapes, &crate::movement::WorldCollision(blocks), box_),
            Ok(v) if v.is_empty()
        )
    });
    let entities = crate::entities::Entities::shared();
    let data = face.to_3d();
    let coexists = !entities.all().iter().any(|e| {
        e.kind == crate::entities::ITEM_FRAME
            && e.data == data
            && crate::entities::hanging_block_pos(e)
                == Some((f64::from(place.0), f64::from(place.1), f64::from(place.2)))
    });
    if !(clear && (solid || diode) && coexists) {
        return Some(crate::itemuse::UseOn::Fail);
    }
    // `playPlacementSound()` is before `addFreshEntity`, at the frame's own
    // position, on `getSoundSource()` — NEUTRAL.
    crate::sounds::send(
        s.players,
        s.protocol_ids,
        s.sound_events,
        Some(entities),
        &[crate::sounds::Sound {
            event: "minecraft:entity.item_frame.place".into(),
            source: crate::sounds::SOURCE_NEUTRAL,
            at: crate::sounds::Eighths::exact(x, y, z),
            pos: (x, y, z),
            volume: 1.0,
            pitch: 1.0,
            except: None,
        }],
    );
    let kinds = crate::entities::shared_kind_ids()?;
    let mut rng = crate::dropitem::unique_random();
    let frame = crate::entities::Entity {
        id: entities.next_id(),
        uuid: crate::dropitem::insecure_uuid(&mut rng),
        kind: crate::entities::ITEM_FRAME,
        x,
        y,
        z,
        y_rot,
        x_rot,
        y_head_rot: 0.0,
        movement: (0.0, 0.0, 0.0),
        on_ground: false,
        data,
        swinging: false,
        swing_time: 0,
        swinging_arm: None,
        synched: crate::synched::Synched::ItemFrame {
            direction: u8::try_from(data).unwrap_or(3),
            item: None,
        },
    };
    entities.insert(frame.clone());
    if let Some(values) = frame.synched.non_default_values() {
        if let Ok(body) = crate::synched::set_entity_data_body(frame.id, &values) {
            crate::tracker::mark_data_dirty(frame.id, body);
        }
    }
    crate::tracker::Tracker::shared().add(
        &frame,
        &crate::status::tracked_players(s.players, entities),
        kinds,
    );
    Some(crate::itemuse::UseOn::Fail)
}

/// The `SpawnEggItem.useOn` arm of `ItemStack.useOn`: a spawn egg on a
/// block puts a mob in the world and returns `SUCCESS` (nothing to place),
/// or `FAIL` when the type cannot spawn. `None` is not-an-egg.
fn spawn_egg_step(
    s: &Session<'_>,
    use_on: &crate::interact::UseItemOn,
) -> Option<crate::itemuse::UseOn> {
    let item = s.hotbar.held()?;
    crate::mob::kind_for_spawn_egg(item)?;
    let world =
        crate::tiles::Tiles::shared(crate::status::WORLD_SEED, crate::status::WORLD_RADIUS).ok()?;
    let entities = crate::entities::Entities::shared();
    let tracker = crate::tracker::Tracker::shared();
    let kinds = crate::entities::shared_kind_ids()?;
    let players = crate::status::tracked_players(s.players, entities);
    let clicked = rustmc_data::BlockPos {
        x: use_on.pos.0,
        y: use_on.pos.1,
        z: use_on.pos.2,
    };
    let face_is_up = use_on.face == Some(crate::interact::Face::Up);
    let entity_data = s
        .hotbar
        .held_stack()
        .and_then(crate::item::ItemStack::entity_data);
    match crate::mob::use_spawn_egg(
        world,
        item,
        entity_data,
        clicked,
        face_is_up,
        entities,
        tracker,
        &players,
        kinds,
    ) {
        Ok(Some(_)) => Some(crate::itemuse::UseOn::Fail),
        Ok(None) => None,
        Err(_) => Some(crate::itemuse::UseOn::Fail),
    }
}

/// `NoteBlock.useItemOn`, offsets 0-24: a stack in
/// `ItemTags.NOTE_BLOCK_TOP_INSTRUMENTS` clicked on the **top** face returns
/// `PASS`, so the block is not asked again and the click goes on to place the
/// head on top of the note block — which is how a mob head becomes the
/// block's instrument. Every other stack and face falls to `super`,
/// `TRY_WITH_EMPTY_HAND`, and plays the note.
///
/// Hand-ported rather than read from `use_item_on.json`: the extract has the
/// row, but marks `NoteBlock` `faceSensitive`, and `crate::useitemon` refuses
/// a face-sensitive class outright because its cells carry no face. Measured
/// on `890`: vanilla placed the skull and retuned nothing; this server played
/// the note and placed nothing.
///
/// With no tag table this answers `false` — the previous behaviour — rather
/// than guessing membership.
fn note_block_passes_to_placement(
    s: &Session<'_>,
    use_on: &crate::interact::UseItemOn,
    was: &str,
) -> bool {
    use_on.face == Some(crate::interact::Face::Up)
        && crate::noteblock::is_note_block(was)
        && s.hotbar.held().is_some_and(|item| {
            crate::itemtags::is_in(crate::itemtags::NOTE_BLOCK_TOP_INSTRUMENTS, item) == Some(true)
        })
}

/// `LecternBlock.useItemOn`'s book arm, offsets 20-52, and the `placeBook`
/// it reaches.
///
/// ```text
///    0  if (state.getValue(HAS_BOOK)) return TRY_WITH_EMPTY_HAND;
///   20  if (stack.is(ItemTags.LECTERN_BOOKS))
///   37      return tryPlaceBook(...) ? SUCCESS : PASS;
/// placeBook:
///         be.setBook(stack.consumeAndReturn(1, entity));
///         resetBookState(entity, level, pos, state, true);   // setBlock(POWERED=false, HAS_BOOK=true, 3)
///         level.playSound(null, pos, BOOK_PUT, BLOCKS, 1.0f, 1.0f);
/// ```
///
/// `None` hands everything else back to the tables: a lectern that already
/// has a book (`TRY_WITH_EMPTY_HAND`, which opens it), and any stack that is
/// not a lectern book. Measured on `756`: the setup's writable book reached
/// the lectern on vanilla (`has_book=true` in the step's resync) and not here
/// (`has_book=false`), so the menu the check is about could not open.
///
/// **Not ported: `resetBookState`'s tail**, `updateBelow` — an
/// `updateNeighborsAt(pos.below())` so a block under the lectern hears it —
/// and the `BLOCK_CHANGE` game event. The `setBlock` itself carries
/// `UPDATE_NEIGHBORS` and runs the ordinary neighbour pass.
fn lectern_place_book_step(
    s: &mut Session<'_>,
    blocks: &dyn crate::edits::Blocks,
    use_on: &crate::interact::UseItemOn,
    was: &str,
) -> Option<bool> {
    if was.split('[').next() != Some("minecraft:lectern")
        || crate::placement::property_of(was, "has_book") != Some("false")
    {
        return None;
    }
    let held = s.hotbar.held()?.to_string();
    if crate::itemtags::is_in(crate::itemtags::LECTERN_BOOKS, &held) != Some(true) {
        return None;
    }
    // `consumeAndReturn(1, entity)`: the lectern gets a one-item copy and the
    // hand shrinks by one, except in creative — the corpus actor's mode.
    if !s.creative {
        let slot = s.hotbar.selected();
        let mut in_hand = s.hotbar.stack(slot);
        in_hand.shrink(1);
        s.hotbar.set_stack(slot, in_hand);
    }
    crate::blockentity::BlockEntities::shared().with_mut(use_on.pos, |be| {
        if be.kind() == crate::blockentity::Kind::Lectern {
            // `setBook` zeroes the page (`LecternBlockEntity.setBook`, see
            // `crate::lectern`).
            be.set_item(0, crate::item::ItemStack::one(&held));
            be.set_extra("Page", rustmc_world::nbt::Nbt::Int(0));
        }
    });
    let next = crate::placement::with_property(
        &crate::placement::with_property(was, "powered", "false"),
        "has_book",
        "true",
    );
    let mut updates = crate::shapes::Effects::new();
    crate::shapes::set_block(
        blocks,
        s.shapes,
        use_on.pos,
        &next,
        crate::shapes::UPDATE_ALL,
        crate::shapes::RECURSION_LIMIT,
        &mut updates,
    );
    record_updates(s, &updates);
    crate::sounds::send(
        s.players,
        s.protocol_ids,
        s.sound_events,
        Some(crate::entities::Entities::shared()),
        &[crate::sounds::Sound {
            event: "minecraft:item.book.put".into(),
            volume: 1.0,
            ..crate::sounds::chest_open(use_on.pos, 1.0)
        }],
    );
    Some(true)
}

/// `CampfireBlock.useItemOn` — offsets 0-98.
///
/// Four guards and no `LIT` test among them: the block entity is a
/// `CampfireBlockEntity` (offset 13), the held stack tests true against
/// `RecipePropertySet.CAMPFIRE_INPUT` (46), the level is a `ServerLevel` (56)
/// and `placeFood` found a free slot (76). **Food goes onto an unlit campfire
/// as readily as a lit one** — it simply never cooks, because
/// `cooldownTick` only unwinds progress that is already above zero.
///
/// The property set is the set of ingredients of every `campfire_cooking`
/// recipe, so `Recipes::find_single(CampfireCooking, stack).is_some()` is the
/// same predicate over the same table and not a second reading of it.
///
/// # This is the first block entity in this branch whose contents reach the
/// wire
///
/// `placeFood` ends in `markUpdated`, which is `setChanged()` plus
/// `Level.sendBlockUpdated(pos, state, state, 3)` — the same state on both
/// sides. That marks the position in `ChunkHolder.changedBlocksPerSection`,
/// and `broadcastChanges` then sends both the `block_update` **and**
/// `broadcastBlockEntityIfNeeded`. `CampfireBlockEntity` is one of the
/// nineteen types that declare `getUpdatePacket`, so the food shows up as a
/// `block_entity_data` beside a `block_update` that changes nothing.
///
/// Recording the position on `ChangedBlocks` is therefore the whole of the
/// wire effect, and it is why this returns without calling
/// `crate::shapes::set_block`: there is no state change to cascade.
fn campfire_place_food_step(
    s: &mut Session<'_>,
    blocks: &dyn crate::edits::Blocks,
    use_on: &crate::interact::UseItemOn,
    was: &str,
) -> Option<bool> {
    if was.split('[').next() != Some("minecraft:campfire") {
        return None;
    }
    let held = s.hotbar.held()?.to_string();
    let recipes = s.recipes?;
    let stack = crate::item::ItemStack::one(&held);
    recipes.find_single(crate::recipes::RecipeType::CampfireCooking, &stack)?;
    let mut placed = false;
    crate::blockentity::BlockEntities::shared().with_mut(use_on.pos, |be| {
        if be.kind() != crate::blockentity::Kind::Campfire {
            return;
        }
        let Some(crate::behaviour::Behaviour::Campfire(mut st)) = be.behaviour().cloned() else {
            return;
        };
        let mut items: Vec<crate::item::ItemStack> = (0..be.size()).map(|i| be.item(i)).collect();
        placed = crate::campfire::place_food(&mut st, &mut items, &stack, recipes);
        if placed {
            for (i, item) in items.into_iter().enumerate() {
                be.set_item(i, item);
            }
            if let Some(crate::behaviour::Behaviour::Campfire(cell)) = be.behaviour_mut() {
                *cell = st;
            }
        }
    });
    if !placed {
        // `CONSUME` at offset 91: the click is eaten either way once the
        // stack tests true, so a campfire with four occupied slots still
        // blocks the placement behind it.
        let _ = blocks;
        return Some(true);
    }
    // `consumeAndReturn(1, entity)` inside `placeFood`, same creative split
    // as every other consumer here.
    if !s.creative {
        let slot = s.hotbar.selected();
        let mut in_hand = s.hotbar.stack(slot);
        in_hand.shrink(1);
        s.hotbar.set_stack(slot, in_hand);
    }
    crate::broadcast::ChangedBlocks::shared().record(use_on.pos);
    Some(true)
}

/// `JukeboxPlayable.tryInsertIntoJukebox` — offsets 0-128.
///
/// `None` means "this is not a jukebox insert, ask the table"; `Some(true)`
/// means the click was consumed.
///
/// # The component this server does not have, and what stands in for it
///
/// Vanilla's first guard is `stack.get(DataComponents.JUKEBOX_PLAYABLE)`, and
/// `crate::item::ItemStack` carries no components at all.
/// `JukeboxSongs::from_stack` is the stand-in: `data/vanilla/jukebox_songs
/// .json` is keyed by the item that carries the component, so it is exact for
/// every item in the pinned jar and stops being so the day a datapack adds a
/// playable item. Stated rather than approximated silently — the same
/// boundary `crate::jukebox`'s header records for the song id.
///
/// # What reaches the wire, and what does not
///
/// `setTheItem` writes `HAS_RECORD` with `setBlock(..., 2)` **before** it
/// starts the player, so the block turns on and only then does the disc play.
/// Flag 2 is `UPDATE_CLIENTS` with no neighbour notification, which is why
/// the write goes through `Blocks::set_block` and `ChangedBlocks` rather than
/// through `crate::shapes::set_block`: there is no cascade to run.
///
/// There is **no `block_entity_data`**. `JukeboxBlockEntity` does not declare
/// `getUpdatePacket`, so the disc in the block reaches a client as the state
/// change and the `level_event` and by nothing else.
/// `AbstractCauldronBlock.useItemOn` for a bucket — see `crate::cauldron`.
///
/// `None` when the block is not a cauldron or the dispatcher has no entry
/// for the held bucket (`TRY_WITH_EMPTY_HAND`), so the existing path answers
/// exactly as it did. Every bucket entry consumes the action.
fn cauldron_bucket_step(
    s: &mut Session<'_>,
    blocks: &dyn crate::edits::Blocks,
    use_on: &crate::interact::UseItemOn,
    was: &str,
) -> Option<bool> {
    let held = s.hotbar.held()?.to_string();
    let p = use_on.pos;
    let water_above = blocks.fluid_at((p.0, p.1 + 1, p.2)).kind.is_water();
    let i = crate::cauldron::interact(was, &held, water_above)?;
    let mut effects = crate::shapes::Effects::new();
    if let Some(next) = i.write {
        crate::shapes::set_block(
            blocks,
            s.shapes,
            p,
            next,
            crate::shapes::UPDATE_ALL,
            crate::shapes::RECURSION_LIMIT,
            &mut effects,
        );
    }
    record_updates(s, &effects);
    if let Some(event) = i.sound {
        crate::sounds::send(
            s.players,
            s.protocol_ids,
            s.sound_events,
            Some(crate::entities::Entities::shared()),
            &[crate::sounds::Sound {
                event: event.to_string(),
                source: crate::sounds::SOURCE_BLOCKS,
                at: crate::sounds::Eighths::block(p),
                pos: crate::sounds::block_centre(p),
                volume: 1.0,
                pitch: 1.0,
                // `playSound(null, pos, ...)` — nobody is excluded.
                except: None,
            }],
        );
    }
    // `player.setItemInHand(hand, createFilledResult(stack, player, result))`.
    // In creative `createFilledResult` returns the stack that was used and
    // adds the result to the inventory unless it is already there.
    if s.creative {
        creative_filled_result(s, i.hand_after);
    } else {
        let slot = s.hotbar.selected();
        s.hotbar.set(slot, Some(i.hand_after));
    }
    Some(true)
}

fn jukebox_insert_step(
    s: &mut Session<'_>,
    blocks: &dyn crate::edits::Blocks,
    use_on: &crate::interact::UseItemOn,
    was: &str,
) -> Option<bool> {
    if was.split('[').next() != Some("minecraft:jukebox") {
        return None;
    }
    // Offsets 0-19 of `useItemOn`: a jukebox that already holds a record
    // declines with `TRY_WITH_EMPTY_HAND` and is answered by the eject path,
    // which is not ported. Falling through to the table is what this server
    // did before and is not made worse by being reached from here.
    if crate::placement::property_of(was, "has_record") != Some("false") {
        return None;
    }
    let held = s.hotbar.held()?.to_string();
    let songs =
        crate::jukebox::JukeboxSongs::shared(std::path::Path::new(JUKEBOX_SONGS_PATH)).ok()?;
    let stack = crate::item::ItemStack::one(&held);
    songs.from_stack(&stack)?;
    // `consumeAndReturn(1, player)` at offset 70: the stack the block entity
    // gets is a **single-item copy**, and the player's own stack shrinks by
    // one — except in creative, where `ItemStack.consume` skips the shrink
    // for a holder with infinite materials. Same split as
    // [`item_use_consumed`], and the corpus actor is creative.
    if !s.creative {
        let slot = s.hotbar.selected();
        let mut in_hand = s.hotbar.stack(slot);
        in_hand.shrink(1);
        s.hotbar.set_stack(slot, in_hand);
    }
    let mut effects = crate::jukebox::JukeboxEffects::default();
    crate::blockentity::BlockEntities::shared().with_mut(use_on.pos, |be| {
        if be.kind() != crate::blockentity::Kind::Jukebox {
            return;
        }
        let mut slot = be.item(0);
        let Some(crate::behaviour::Behaviour::Jukebox(mut player)) = be.behaviour().cloned() else {
            return;
        };
        effects = crate::jukebox::set_the_item(&mut player, &mut slot, stack, songs, true);
        be.set_item(0, slot);
        if let Some(crate::behaviour::Behaviour::Jukebox(cell)) = be.behaviour_mut() {
            *cell = player;
        }
    });
    if let Some(has_record) = effects.has_record {
        let next = crate::placement::with_property(
            was,
            "has_record",
            if has_record { "true" } else { "false" },
        );
        if blocks.set_block(use_on.pos, &next).is_some() {
            crate::broadcast::ChangedBlocks::shared().record(use_on.pos);
        }
    }
    for (event, data) in effects.level_events {
        if let Ok(body) = crate::interact::level_event_body(event, use_on.pos, data, false) {
            s.players
                .broadcast(rustmc_proto::ids::play::cb::LEVEL_EVENT, &body);
        }
    }
    Some(true)
}

/// Where `jukebox_songs.json` is, relative to the server's working directory.
///
/// Spelled as one quoted `data/vanilla/...` literal because
/// `ci/gen-gameplay-tables.sh` derives the set of tables a tree needs by
/// grepping for exactly that shape — a path assembled any other way is
/// invisible to it and its table is never generated in a gate dir. That cost
/// `block-entities-2` a red T0; see `crate::blockentity`'s own fixture loader.
const JUKEBOX_SONGS_PATH: &str = "data/vanilla/jukebox_songs.json";

/// The consuming branch of `BlockState.useItemOn`: what it wrote, and what it
/// took out of your hand.
///
/// Only the classes `crate::useitemon::PORTED_CLASSES` names reach this, and
/// what that list holds today decides what is *not* here: `FlowerPotBlock`
/// plays no sound, opens no menu and schedules no tick, and the table loader
/// refuses a kept row that does any of the three rather than letting this
/// function drop it. A class added to the list without checking that is how a
/// block starts half working.
fn item_use_consumed(
    s: &mut Session<'_>,
    blocks: &dyn crate::edits::Blocks,
    pos: crate::interact::BlockPos,
    c: &crate::useitemon::Consumed,
) {
    if let Some((next, flags)) = &c.write {
        let mut updates = crate::shapes::Effects::new();
        // The flags are the table's, read at the call site vanilla wrote them
        // at — 3 for both of `FlowerPotBlock`'s — for the same reason
        // `useWithoutItem`'s are: `Level.setBlock` gates the shape cascade on
        // them and one constant for every class is wrong for some of them.
        crate::shapes::set_block(
            blocks,
            s.shapes,
            pos,
            next,
            *flags,
            crate::shapes::RECURSION_LIMIT,
            &mut updates,
        );
        record_updates(s, &updates);
    }
    // `ItemStack.consume(int, LivingEntity)`:
    //
    // ```text
    //   if (entity == null || !entity.hasInfiniteMaterials()) this.shrink(n);
    // ```
    //
    // So the creative branch is *here*, in the consumer, and not in the
    // table: `use_item_on.json` marks `minecraft:flower_pot` probe-dependent
    // on the `game_master` axis for exactly this, and that axis moves the
    // stack and never the block write. The corpus actor is creative, so the
    // pot fills and the flower stays in hand.
    if c.consume_one && !s.creative {
        let slot = s.hotbar.selected();
        let mut held = s.hotbar.stack(slot);
        held.shrink(1);
        s.hotbar.set_stack(slot, held);
    }
}

/// `BlockState.useWithoutItem` at `ServerPlayerGameMode.useItemOn` offset 196,
/// and everything vanilla's classes do from inside it.
///
/// Returns whether the click was consumed, which is what stops the placement
/// path. Split out of [`handle_use_item_on`] when `useItemOn` gained a step in
/// front of it.
///
/// **The `mayBuild`/`canUseGameMasterBlocks` gate lives HERE and not in the
/// caller, and that is a bytecode reading rather than a tidiness choice.** It
/// is offset 0 of the *block's* `useWithoutItem` — `ComparatorBlock`:
/// `Player.getAbilities`, `Abilities.mayBuild`, `ifne 15`, `PASS` — so it is
/// reached only once `ServerPlayerGameMode.useItemOn` has already asked
/// `BlockState.useItemOn` at offset 145 and been handed a
/// `TryEmptyHandInteraction` at 176. Hoisting it above the item step would
/// gate a call vanilla makes unconditionally.
///
/// # Errors
/// If a send fails.
fn use_without_item_step(
    s: &mut Session<'_>,
    blocks: &dyn crate::edits::Blocks,
    use_on: &crate::interact::UseItemOn,
    was: &str,
) -> io::Result<bool> {
    // **The `mayBuild`/`canUseGameMasterBlocks` gate answers first and wins
    // over everything below**, exactly as it does in the bytecode — offset 0
    // of the block's own `useWithoutItem`, before any state is read, on all
    // five blocks it covers. `may_build`/`game_master` are hardcoded: this
    // server has no per-player ability model yet, so this is a no-op on every
    // call today. See `crate::useblock::use_gate`'s own doc.
    let may_build = true;
    let game_master = false;
    // **The bell answers before everything below, and it is the one block
    // here that consumes a click and writes no state.**
    //
    // `BellBlock.useWithoutItem` is `onHit(level, state, hit, player, true)`,
    // and a proper hit reaches `attemptToRing`, whose whole output is a
    // `Level.blockEvent(pos, block, 1, direction.get3DDataValue())` and a
    // `playSound(null, pos, BELL_BLOCK, BLOCKS, 2.0f, 1.0f)`. Neither
    // `use_without_item.json` nor `hand_ported` can express that: the table's
    // rows are a replacement state and `Ported` is a state plus flags, and a
    // bell has no state to write. So it is answered here, before the gate,
    // and returns `Ok(true)` without touching `set_block`.
    //
    // `has_block_entity` is vanilla's `getBlockEntity(pos) instanceof
    // BellBlockEntity`. Every `minecraft:bell` block has one — `BellBlock`
    // implements `EntityBlock` and its `newBlockEntity` never returns null —
    // so the block's own name is the same predicate today. It stops being so
    // the day this server can hold a bell block whose block entity was
    // evicted, and `crate::bell::attempt_to_ring` takes the boolean rather
    // than deriving it for exactly that reason.
    if was.split('[').next() == Some("minecraft:bell") {
        let attach = crate::placement::property_of(was, "attachment")
            .and_then(crate::bell::BellAttach::from_name);
        let facing =
            crate::placement::property_of(was, "facing").and_then(crate::interact::Face::from_name);
        if let (Some(attach), Some(facing), Some(hit_face)) = (attach, facing, use_on.face) {
            let (handled, ring) = crate::bell::on_hit(
                true,
                facing,
                attach,
                hit_face,
                f64::from(use_on.cursor.1),
                true,
            );
            if handled {
                if let Some(ring) = ring {
                    // The order is vanilla's: `onHit` queues the block event
                    // first (its offsets 30-47) and `attemptToRing` plays the
                    // sound second (57-68). The event waits for the tick's own
                    // `run_block_events` sweep and the sound goes out now, so
                    // the wire order is sound-then-event either way — the same
                    // note `use_sound` carries above.
                    crate::containers::BlockEvents::shared().queue(
                        crate::containers::BlockEventData {
                            pos: use_on.pos,
                            block: "minecraft:bell".to_string(),
                            b0: ring.event.0,
                            b1: ring.event.1,
                        },
                    );
                    crate::sounds::send(
                        s.players,
                        s.protocol_ids,
                        s.sound_events,
                        Some(crate::entities::Entities::shared()),
                        &[crate::sounds::bell_ring(use_on.pos)],
                    );
                }
                // `true` whether or not it rang: `BellBlock.onHit` returns
                // true for any proper hit (offsets 48-85), so an unringable
                // bell still consumes the click and blocks the placement.
                return Ok(true);
            }
        }
    }
    // **`SweetBerryBushBlock.useWithoutItem`, offsets 0-129 — picking
    // berries.** `use_without_item.json` records `consumes: true, writes: []`
    // for `age > 1`, because the whole body sits behind `instanceof
    // ServerLevel` and the harness is not one (`ci/checks/server-level-gates.toml`,
    // the `SweetBerryBushBlock` gate). Vanilla, inside the gate:
    //
    //   dropFromBlockInteractLootTable(HARVEST_SWEET_BERRY_BUSH)  — not ported
    //   playSound(null, pos, PICK_BERRIES, BLOCKS, 1.0, 0.8 + nextFloat() * 0.4)
    //   setBlock(pos, AGE=1, 2); gameEvent(BLOCK_CHANGE)
    //
    // and `SUCCESS` outside it. The pitch draw is `level.getRandom()`.
    if was.split('[').next() == Some("minecraft:sweet_berry_bush")
        && crate::placement::property_of(was, "age")
            .and_then(|a| a.parse::<i32>().ok())
            .is_some_and(|a| a > 1)
    {
        let pitch = {
            use rustmc_worldgen::rng::RandomSource;
            let mut level = crate::weather::level_random()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            0.8 + level.next_f32() * 0.4
        };
        crate::sounds::send(
            s.players,
            s.protocol_ids,
            s.sound_events,
            Some(crate::entities::Entities::shared()),
            &[crate::sounds::Sound {
                event: "minecraft:block.sweet_berry_bush.pick_berries".into(),
                source: crate::sounds::SOURCE_BLOCKS,
                at: crate::sounds::Eighths::block(use_on.pos),
                pos: crate::sounds::block_centre(use_on.pos),
                volume: 1.0,
                pitch,
                except: None,
            }],
        );
        let picked = crate::placement::with_property(was, "age", "1");
        let mut updates = crate::shapes::Effects::new();
        crate::shapes::set_block(
            blocks,
            s.shapes,
            use_on.pos,
            &picked,
            crate::shapes::UPDATE_CLIENTS,
            crate::shapes::RECURSION_LIMIT,
            &mut updates,
        );
        record_updates(s, &updates);
        return Ok(true);
    }
    let gate = crate::useblock::use_gate(s.placed_by, was, may_build, game_master);
    // **Two more classes are answered by hand and they win over the table.**
    // `FenceGateBlock.useWithoutItem` reads `player.getDirection()` and
    // `DaylightDetectorBlock.useWithoutItem` is `refused` in every one of its
    // 32 rows; neither is a shape `use_without_item.json` has, so
    // `crate::useblock::hand_ported` is consulted next and supplies its own
    // `Level.setBlock` flags. See that module's header.
    let facing =
        crate::placement::HORIZONTAL_BY_2D_DATA[crate::placement::from_y_rot(s.player.yaw) % 4];
    // `flag_override` is `Some` exactly when the answer did not come from the
    // table, which is the role `ported` played before the gate existed.
    let (answer, flag_override) = match &gate {
        Some(crate::useblock::Gate::Pass) => (crate::useblock::Used::Pass, None),
        Some(crate::useblock::Gate::Write(p)) => {
            (crate::useblock::Used::Consumed(p.to.clone()), Some(p.flags))
        }
        None => {
            let ported = crate::useblock::hand_ported(s.placed_by, was, facing);
            match &ported {
                Some(p) => (crate::useblock::Used::Consumed(p.to.clone()), Some(p.flags)),
                None => (crate::useblock::use_without_item(s.uses, was), None),
            }
        }
    };
    match answer {
        // `Consumed(None)` is a block that took the click and wrote
        // nothing — an already-pressed button, and 891 states like it.
        // It still stops the placement, which is the whole reason the
        // two are distinguished.
        crate::useblock::Used::Consumed(next) => {
            if let Some(next) = next {
                // **`FlowerPotBlock.useWithoutItem` offsets 11-42, which
                // `use_without_item.json` has no column for.** That table
                // records the block write — every one of the 38 potted states
                // writes `minecraft:flower_pot` with flags 3 — and has no way
                // to say that the pot also creates `new ItemStack(this.potted)`
                // and hands it to `player.addItem`. Only those 38 states have
                // a pairing, so for every other block in the table this is one
                // failed map lookup.
                //
                // Before the write, which is vanilla's order: `addItem` is at
                // offset 24 and `setBlock` at 43.
                let table = s.item_uses;
                if let Some(item) = crate::useitemon::given_back(table, was) {
                    player_add_item(s, item);
                }
                let mut updates = crate::shapes::Effects::new();
                // The flags come from the table, not from a constant.
                // `DoorBlock.useWithoutItem` writes with 10 and
                // `ButtonBlock.press` with 3, and `Level.setBlock` gates
                // the shape cascade on them — which is how a door's
                // upper half follows its lower one.
                let flags = flag_override.unwrap_or_else(|| {
                    s.uses
                        .and_then(|t| t.write_flags(was))
                        .unwrap_or(crate::shapes::UPDATE_ALL)
                });
                crate::shapes::set_block(
                    blocks,
                    s.shapes,
                    use_on.pos,
                    &next,
                    flags,
                    crate::shapes::RECURSION_LIMIT,
                    &mut updates,
                );
                // **The sound is the block's own rule.** Reading it off
                // `next` rather than off `was` is what makes a lever
                // going *on* sound different from one going off.
                //
                // It is sent after the write here and vanilla's three
                // classes disagree about that order — `LeverBlock.pull`
                // and `ButtonBlock.press` set the block and then sound,
                // `ComparatorBlock.useWithoutItem` sounds and then sets.
                // Neither order is observable: a `playSound` reaches
                // `PlayerList.broadcast` with nothing queued in front of
                // it, while the block change waits for the tick boundary
                // in `crate::broadcast`, so the sound is ahead of the
                // update on the wire whichever way round the two calls
                // go.
                //
                // Four transcribed families, then the table. Since
                // 2026-08-17 `useblock`'s extract carries the event id,
                // the category, the volume and who is excluded, so this
                // no longer has to guess which sound — for the 1,509
                // states whose pitch is a constant. The 2,944 whose pitch
                // vanilla draws still stay silent, because a door playing
                // a lever's click is worse than a door playing none and a
                // door playing a *made-up pitch* is no better.
                // See `crate::sounds` and `use_sound`.
                if let Some(sound) =
                    use_sound(s.placed_by, s.uses, s.conn.id(), was, &next, use_on.pos)
                {
                    crate::sounds::send(
                        s.players,
                        s.protocol_ids,
                        s.sound_events,
                        Some(crate::entities::Entities::shared()),
                        &[sound],
                    );
                }
                // **`DaylightDetectorBlock.useWithoutItem` offset 67 — the
                // half that was transcribed and not run until 2026-08-18.**
                // The `inverted` cycle above wrote with flag 2, deliberately
                // telling no neighbour; this is the line that does the
                // neighbour half, with its own flag 3, and only when the
                // recomputed strength differs.
                //
                // It is inside the `Consumed` arm and after the first write
                // because that is where vanilla has it — between `gameEvent`
                // and the `return SUCCESS`. Both writes reach the client as
                // one state at the tick boundary, so their order is not
                // observable for a lone detector; what IS observable is that
                // the second one happens at all, and for two years of this
                // server it did not.
                //
                // The detector test comes FIRST and that is not tidiness.
                // `crate::light::sky_light` walks the whole column above the
                // position — 380 blocks in the overworld — and this arm runs
                // on every consumed right-click in the game. A lever flip must
                // not pay for a daylight detector's sky.
                if crate::sky::is_daylight_detector(&blocks.state_at(use_on.pos)) {
                    let after = blocks.state_at(use_on.pos);
                    let top_y = crate::sky::active().map_or(320, crate::sky::SkyModel::top_y);
                    let lit = crate::light::sky_light(|p| blocks.state_at(p), use_on.pos, top_y);
                    if let Some(recomputed) = crate::sky::signal_strength_write(
                        &after,
                        crate::sky::day_time(s.clock.ticks()),
                        lit,
                    ) {
                        crate::shapes::set_block(
                            blocks,
                            s.shapes,
                            use_on.pos,
                            &recomputed,
                            crate::sky::UPDATE_SIGNAL_STRENGTH_FLAGS,
                            crate::shapes::RECURSION_LIMIT,
                            &mut updates,
                        );
                    }
                }
                // The fourth family, and the one that does NOT sound here.
                // `NoteBlock.playNote` calls `Level.blockEvent`, which queues;
                // the sound comes out of `runBlockEvents` at the tick
                // boundary, so it lands *behind* the block update rather than
                // ahead of it. See `crate::noteblock`.
                queue_note_block_event(s, use_on.pos, &next);
                // **`LeverBlock.pull` and `ButtonBlock.press` end in a
                // `updateNeighbours` that is TWO `updateNeighborsAt` calls,
                // and `Level.setBlock`'s `UPDATE_NEIGHBORS` flag is only the
                // first.** The second is at `pos.relative(back)` — the block
                // the lever is stuck to — and is how a strongly powered block
                // passes a signal on to something that is not beside the
                // lever at all.
                //
                // Scenario 400 is exactly that and nothing else: its torch
                // hangs on the stone the lever stands on, two positions from
                // the lever, so the first call can never reach it. See
                // `crate::power::face_attached_update_neighbours`.
                if crate::power::attaches_and_updates_twice(s.placed_by, &next) {
                    crate::power::face_attached_update_neighbours(
                        blocks,
                        crate::power::shared(),
                        s.shapes,
                        use_on.pos,
                        &next,
                        &mut updates,
                    );
                }
                record_updates(s, &updates);
            }
            // `ButtonBlock.press` schedules its own un-press, and the
            // delay is the table's — 20 ticks for stone, 30 for wood —
            // rather than one number for every button. Scheduled after
            // the write, because the queue is keyed by position and the
            // position has to hold the block by the time it fires.
            if let (Some(ticks), Some(tick)) = (s.ticks, s.uses.and_then(|t| t.tick_for(was))) {
                crate::ticks::on_used(
                    ticks,
                    use_on.pos,
                    &blocks.state_at(use_on.pos),
                    tick,
                    s.clock.ticks(),
                );
            }
            // `ChestBlock.useWithoutItem` is `getMenuProvider` then
            // `Player.openMenu`, and `CraftingTableBlock`'s and
            // `FurnaceBlock`'s are the same two steps through a different
            // provider. It belongs **here**, inside the consumed arm and
            // before the handler's tail: vanilla opens the menu from
            // inside the block's own `useWithoutItem`, so the two screen
            // packets precede the two resyncs at the foot of
            // `handleUseItemOn`.
            open_menu_for(s, use_on.pos, was)?;
            Ok(true)
        }
        crate::useblock::Used::Pass => Ok(false),
    }
}

/// `BoneMealItem.useOn`'s crop arm, end to end.
///
/// Returns whether the bone meal was taken — which is `growCrop`'s own return
/// value, and stops the placement arm below from running for the same click.
///
/// **The RNG is `Level.getRandom()`, and this server does not have one that
/// vanilla's is comparable to.** The draw is taken from the session's own
/// source so that the *number of draws* matches — a torchflower takes none
/// and the other four take one, which is what
/// [`crate::bonemeal::AgeIncrease::draw_bound`] exists to say — but the
/// *value* is this server's and not vanilla's. So a crop grows by a legal
/// amount rather than by vanilla's amount, and the two agree on which states
/// are reachable and not on which one is reached. Said here rather than left
/// for a corpus run to discover.
fn bone_meal(
    s: &mut Session<'_>,
    blocks: Option<&dyn crate::edits::Blocks>,
    pos: crate::interact::BlockPos,
) -> bool {
    let (Some(blocks), Some(table)) = (blocks, s.bonemeal) else {
        return false;
    };
    let state = blocks.state_at(pos);
    // The blocks whose growth `crate::growth` owns answer first: sweet
    // berries, cocoa, a bamboo sapling, a pitcher crop and the two stems.
    // Their draws come off `Level.getRandom()`, which is what `growCrop`
    // passes — the same generator the random-tick pass uses.
    {
        let top_y = crate::sky::active().map_or(320, crate::sky::SkyModel::top_y);
        let light = crate::brightness::Light::new(blocks, top_y, true, 0);
        let mut updates = crate::shapes::Effects::new();
        let mut level = crate::weather::level_random()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut cx = crate::growth::Ctx {
            world: blocks,
            shapes: s.shapes,
            light: &light,
            rng: &mut *level,
            fx: &mut updates,
            raining: false,
        };
        match crate::growth::bone_meal(&mut cx, pos, &state) {
            crate::growth::Meal::NotMine => {}
            crate::growth::Meal::Pass => return false,
            crate::growth::Meal::Grown => {
                drop(level);
                record_updates(s, &updates);
                shrink_held(s, table.shrink());
                s.players.broadcast(
                    rustmc_proto::ids::play::cb::LEVEL_EVENT,
                    &crate::bonemeal::level_event_body(
                        table.level_event(),
                        pos,
                        table.level_event_data(),
                    ),
                );
                return true;
            }
        }
    }
    // The draw comes first and is taken only where vanilla takes one, so a
    // torchflower does not advance the source.
    let roll = match crate::bonemeal::draw_bound(Some(table), &state) {
        Some(bound) => {
            use rustmc_worldgen::rng::RandomSource;
            crate::dropitem::unique_random().next_i32_bound(bound)
        }
        None => 0,
    };
    match crate::bonemeal::grow_crop(Some(table), &state, roll) {
        // `growCrop` returned false at offset 17 or 37 — `useOn` falls
        // through to the water-plant arm, which this server does not have,
        // and then to `PASS`. The stack is untouched.
        crate::bonemeal::Grow::Pass => false,
        // A valid target this port cannot grow. **Also false**, and
        // deliberately not "consumed": pretending to take the item while
        // writing nothing would be strictly worse than declining, because it
        // is the one outcome vanilla never produces.
        crate::bonemeal::Grow::Unported => false,
        crate::bonemeal::Grow::Crop { state } => {
            let mut updates = crate::shapes::Effects::new();
            crate::shapes::set_block(
                blocks,
                s.shapes,
                pos,
                &state,
                table.set_block_flags(),
                crate::shapes::RECURSION_LIMIT,
                &mut updates,
            );
            record_updates(s, &updates);
            // `growCrop` offset 86, after `performBonemeal` and before
            // `useOn` sends anything.
            shrink_held(s, table.shrink());
            // `Level.levelEvent(1505, clickedPos, 15)` — broadcast to
            // everyone within 64 blocks and excluding **nobody**, because
            // the three-argument overload passes a null entity. The acting
            // player receives their own particles, which is what makes this
            // observable from the actor.
            s.players.broadcast(
                rustmc_proto::ids::play::cb::LEVEL_EVENT,
                &crate::bonemeal::level_event_body(
                    table.level_event(),
                    pos,
                    table.level_event_data(),
                ),
            );
            true
        }
    }
}

/// `s.blocks` as a trait object, so the two gates below can share a helper
/// signature.
fn blocks_or_none<'a>(s: &Session<'a>) -> Option<&'a dyn crate::edits::Blocks> {
    s.blocks
}

/// `ItemStack.shrink(n)` on the selected hotbar slot.
///
/// **A creative player is not exempt.** `growCrop` calls `shrink` outright;
/// the creative check is `Player.hasInfiniteMaterials()` inside
/// `ItemStack.consume`, and `growCrop` does not go through `consume`. So bone
/// meal is spent in creative mode, which is the one place this differs from
/// every other item on this path.
fn shrink_held(s: &mut Session<'_>, by: i32) {
    let slot = s.hotbar.selected();
    let mut stack = s.hotbar.stack(slot);
    let left = stack.count().saturating_sub(by);
    if left <= 0 {
        s.hotbar.set_stack(slot, crate::item::ItemStack::empty());
    } else {
        stack.set_count(left);
        s.hotbar.set_stack(slot, stack);
    }
}

/// `state.canSurvive(level, clickedPos)`, answered through the world this
/// session is holding.
///
/// `true` means place it, which is what `BlockItem.canPlace`'s first half
/// answers for a state with no rule, for a class this port has not
/// transcribed, and for **any world with no `block_props` table** — the nine
/// fake worlds in this crate's tests included. See
/// [`crate::edits::Blocks::place_props`] for why that default fails open.
fn place_survive_gate(
    s: &Session<'_>,
    blocks: &dyn crate::edits::Blocks,
    target: crate::interact::BlockPos,
    state: Option<&str>,
) -> bool {
    let (Some(state), Some(props), Some(classes)) = (state, blocks.place_props(), s.survive) else {
        return true;
    };
    let Some(rule) = classes.rule(state) else {
        return true;
    };
    let world = GateWorld { blocks };
    let n = rustmc_behavior::place_survive::Neighbourhood::new(&world, props, target);
    rustmc_behavior::place_survive::vanilla_would_place(
        &n,
        state,
        rule.can_survive.as_deref(),
        rule.may_place_on.as_deref(),
    )
}

/// Whether `state`'s `canSurvive` is `FaceAttachedHorizontalDirectionalBlock`'s
/// own — a lever or a button. The grindstone extends the class and overrides
/// `canSurvive` (always true), so it is not one: its table answer stands.
fn face_attached(s: &Session<'_>, state: &str) -> bool {
    // The extract names the class in full
    // (`net.minecraft.world.level.block.FaceAttached...`).
    s.survive
        .and_then(|c| c.rule(state))
        .and_then(|r| r.can_survive.as_deref())
        .and_then(|class| class.rsplit('.').next())
        == Some("FaceAttachedHorizontalDirectionalBlock")
}

/// `FaceAttachedHorizontalDirectionalBlock.getStateForPlacement`, offsets
/// 0-140:
///
/// ```text
///   for (Direction d : ctx.getNearestLookingDirections())
///     s = d.axis == Y ? default.FACE(d == UP ? CEILING : FLOOR)
///                              .FACING(ctx.getHorizontalDirection())
///                     : default.FACE(WALL).FACING(d.getOpposite())
///     if (s.canSurvive(level, clickedPos)) return s
///   return null
/// ```
///
/// The clicked face only reorders the walk (its opposite goes first,
/// `BlockPlaceContext.getNearestLookingDirections`), so a lever clicked onto
/// the side of a hopper — whose side is not sturdy — lands on the floor in
/// front of it facing the player's way, and one with nowhere to hang is
/// refused. `None` is that refusal.
fn face_attached_placement(
    s: &Session<'_>,
    blocks: &dyn crate::edits::Blocks,
    target: crate::interact::BlockPos,
    state: &str,
    clicked: crate::interact::Face,
    replace_clicked: bool,
) -> Option<String> {
    use rustmc_behavior::place_survive as ps;
    let face_index = match clicked {
        crate::interact::Face::Down => ps::DOWN,
        crate::interact::Face::Up => ps::UP,
        crate::interact::Face::North => ps::NORTH,
        crate::interact::Face::South => ps::SOUTH,
        crate::interact::Face::West => ps::WEST,
        crate::interact::Face::East => ps::EAST,
    };
    let name = |d: usize| match d {
        ps::DOWN => "down",
        ps::UP => "up",
        ps::NORTH => "north",
        ps::SOUTH => "south",
        ps::WEST => "west",
        _ => "east",
    };
    let horizontal =
        crate::placement::HORIZONTAL_BY_2D_DATA[crate::placement::from_y_rot(s.player.yaw)];
    for d in
        ps::nearest_looking_directions(s.player.pitch, s.player.yaw, face_index, replace_clicked)
    {
        let candidate = if d == ps::UP || d == ps::DOWN {
            let attach = if d == ps::UP { "ceiling" } else { "floor" };
            crate::placement::with_property(
                &crate::placement::with_property(state, "face", attach),
                "facing",
                horizontal,
            )
        } else {
            crate::placement::with_property(
                &crate::placement::with_property(state, "face", "wall"),
                "facing",
                name(ps::opposite(d)),
            )
        };
        if place_survive_gate(s, blocks, target, Some(&candidate)) {
            return Some(candidate);
        }
    }
    None
}

/// The world a `canSurvive` rule reads, over this session's blocks.
struct GateWorld<'a> {
    blocks: &'a dyn crate::edits::Blocks,
}

impl rustmc_behavior::place_survive::PlaceWorld for GateWorld<'_> {
    fn state_at(&self, x: i32, y: i32, z: i32) -> String {
        self.blocks.state_at((x, y, z))
    }

    /// **A constant, and the constant is stated rather than sampled.**
    ///
    /// This server has no light engine — nothing under `crates/` computes a
    /// block or sky light level — so there is no reading to take. 15 is the
    /// value at the surface of a daylit world, which is where every gameplay
    /// scenario runs, and it is the value that makes the two rules reading it
    /// answer the way they do above ground: a crop needs `>= 8`
    /// (`CropBlock.hasSufficientLight`) and a mushroom needs `< 13`
    /// (`MushroomBlock.canSurvive`), so at 15 wheat may be planted on
    /// farmland and a mushroom may not be placed on stone.
    ///
    /// What that is wrong about is named rather than left to be found: a
    /// mushroom in a dark cave, which vanilla allows and this gate refuses.
    /// Six blocks of 1,115 read this at all — the two mushrooms and the four
    /// crops — and the fix is a light engine, not a better guess here.
    fn raw_brightness(&self, _x: i32, _y: i32, _z: i32) -> i32 {
        DAYLIT
    }
}

/// See [`GateWorld::raw_brightness`].
const DAYLIT: i32 = 15;

/// `Player.addItem(ItemStack)` — `Inventory.add`, and the window-0 sweep that
/// carries it to the client.
///
/// **The packet is deferred and that is the observable half.** `Inventory.add`
/// sends nothing itself; the changed slot reaches the client from
/// `ServerPlayer.doTick`'s `containerMenu.broadcastChanges()`, inside the
/// entity tick — after `ServerChunkCache.tick` has flushed the block updates
/// and before the connection tick sends the acknowledgement. Vanilla's own
/// recording of `790` is exactly that: three `block_update`s, then
/// `container_set_slot`, then `block_changed_ack`. Sending it from this
/// handler would put it second, in front of the tick's own broadcast.
///
/// It is also what tells this apart from `placeItemBackInInventory`, which
/// sends a `set_player_inventory` per touched slot from inside its own loop.
/// The recording carries none, so this is `add` and not that.
fn player_add_item(s: &mut Session<'_>, item: &str) {
    let placed = s.hotbar.add_item(crate::item::ItemStack::one(item));
    for index in &placed.touched {
        let Some(slot) = crate::click::menu_slot_for_inventory_index(*index) else {
            continue;
        };
        // `incrementStateId` per packet, taken inside `broadcastChanges`.
        // Taken here because this is where the packet is built; the counter is
        // per session and nothing else touches it between now and the sweep.
        s.menu.inventory_state_id = crate::menu::increment_state_id(s.menu.inventory_state_id);
        let Ok(body) = crate::menu::container_set_slot_stack_body(
            INVENTORY_MENU_CONTAINER_ID,
            s.menu.inventory_state_id,
            i16::try_from(slot).unwrap_or(i16::MAX),
            &s.hotbar.stack(*index),
            &encoding(s),
        ) else {
            continue;
        };
        s.conn
            .defer_to_menu_sweep(ids::play::cb::CONTAINER_SET_SLOT, body);
    }
    if !placed.leftover.is_empty() {
        // `FlowerPotBlock.useWithoutItem` offsets 31-42:
        // `if (!player.addItem(out)) player.drop(out, false)`. This server has
        // one item-entity spawner and it is the *thrown* one, so the gap is
        // the same one `handle_container_click` already names: on a full
        // inventory the flower appears with a thrown item's velocity rather
        // than a dropped one's, and carries no `set_entity_data`. Losing it
        // would be the one outcome that cannot be corrected later.
        //
        // **Unreachable from the gameplay corpus.** Its `hold` verb sends
        // `count = 1`, so no scenario can fill 36 slots.
        spawn_and_announce_thrown_item(s, placed.leftover.clone(), false);
    }
}

/// `BlockItem.place`'s sound, for a block that has just gone down.
///
/// # The exclusion is the whole risk, and it is measured
///
/// `aload 6` at offset 210 is the placing player, and `PlayerList.broadcast`
/// skips it. In the corpus of 2026-08-08, **32 scenarios carry a `*.place`
/// sound on vanilla and in all 32 it is on the observer's stream only** —
/// twenty-one of them a strict `match`. Broadcasting this to everybody
/// converts nothing and costs those twenty-one, which is why
/// [`crate::sounds::block_place`] takes the placer as a plain `u64` and not an
/// `Option`.
fn place_sound(s: &Session<'_>, at: crate::interact::BlockPos, state: &str) {
    let (Some(blocks), Some(types)) = (s.blocks, s.sound_types) else {
        return;
    };
    let Some(row) = blocks
        .state_id(state)
        .and_then(|id| u32::try_from(id).ok())
        .and_then(|id| types.row(id))
    else {
        return;
    };
    crate::sounds::send(
        s.players,
        s.protocol_ids,
        s.sound_events,
        Some(crate::entities::Entities::shared()),
        &[crate::sounds::block_place(row, at, s.conn.id())],
    );
}

/// `NoteBlock.playNote`, offsets 0-40 — queue the block event, or do not.
///
/// ```text
///   if (state.getValue(INSTRUMENT).worksAboveNoteBlock()
///       || level.getBlockState(pos.above()).isAir())
///       level.blockEvent(pos, this, 0, 0);
/// ```
///
/// **`state` is the state after the `NOTE` cycle**, which is what
/// `useWithoutItem` passes at offset 29 — it reassigns its local before
/// calling. Reading the instrument off the pre-click state would give the
/// same answer today, because the cycle only touches `NOTE`; reading the
/// *note* off it would be wrong by one, which is why the sound reads the
/// world again at drain time rather than carrying the note along.
///
/// Silent when the instrument table is missing, like every other table-driven
/// sound here.
pub(crate) fn queue_note_block_event(s: &Session<'_>, pos: crate::interact::BlockPos, next: &str) {
    if !crate::noteblock::is_note_block(next) {
        return;
    }
    let Ok(instruments) =
        crate::noteblock::Instruments::shared(std::path::Path::new(crate::noteblock::TABLE_PATH))
    else {
        return;
    };
    let Some(blocks) = s.blocks else { return };
    let above = (pos.0, pos.1 + 1, pos.2);
    let above_is_air = crate::blockentity::block_of(&blocks.state_at(above)) == "minecraft:air";
    if crate::noteblock::should_queue(instruments, next, above_is_air) {
        crate::containers::BlockEvents::shared().queue(crate::containers::BlockEventData {
            pos,
            block: "minecraft:note_block".to_string(),
            b0: 0,
            b1: 0,
        });
    }
}

/// `instanceof NoteBlock` — one block, and the one whose use sound is not the
/// use table's to play. See [`use_sound`].
const NOTE_BLOCK_CLASS: &str = "NoteBlock";

/// The sound a `useWithoutItem` played, if it is one of the three whose rule
/// has been read out of the bytecode.
///
/// Takes the two things it needs rather than a whole `Session` — the class
/// table for the button's `instanceof`, and the clicking connection's id for
/// the two families that exclude the clicker. It is a rule, not a handler,
/// and a rule that cannot be called without building a session is a rule
/// whose 184-block silence nothing ever checks.
///
/// `was` is the state before the write and `next` the state after; all three
/// classes pass the **new** state to their sound, which is what makes a lever
/// going on sound different from one going off, and a comparator switching
/// *to* subtract different from one switching back.
///
/// # Why three blocks and not a table
///
/// `crate::useblock`'s extract records a `sound` boolean for every state that
/// plays one — 184 blocks — and no event id, because `UseGolden`'s probe
/// counts the call rather than reading its argument. A table-driven version
/// would therefore have to pick the event by some other rule, and there is no
/// such rule: a door, a lever and a button share nothing but the boolean.
/// Playing the wrong sound is worse than playing none, so the families whose
/// classes have actually been transcribed are the ones that sound, and
/// `docs/WORKLIST.md` carries the extraction that would generalise it.
///
/// **Each family added here is added beside the others, never over them.**
/// Adding the comparator moved no existing answer: the lever and the button
/// keep the arms and the pitches they had, and the other 181 blocks with a
/// `sound` boolean still reach `None`. Two of them are checked —
/// `470`'s furnace and `480`'s crafting table both consume a click, write
/// nothing, and must stay silent here — because the way to break this
/// function is not to get a pitch wrong, it is to start emitting where
/// vanilla does not.
/// The sound a right-click plays, for the families whose rule has been read.
///
/// **A note block is deliberately not here and must not be added here.**
/// `use_without_item.json` marks `minecraft:note_block` `sound: true`, so a
/// table-driven arm over that column would pick it up — and it would be
/// wrong twice. Vanilla plays a note from `NoteBlock.triggerEvent`, inside
/// the block-event drain at the tick boundary, which is **after** the
/// `block_update`s and immediately **before** the `BLOCK_EVENT` packet; a
/// sound emitted from the click lands ahead of all of them. And it is
/// `RECORDS` at volume 3.0, where every sound this function returns is
/// `BLOCKS` at 1.0.
///
/// The note block's sound is `crate::containers::trigger_event`'s, and a
/// second one from here would be a duplicate on the wire rather than a
/// near-miss. See `docs/PARITY-ROADMAP.md` §54. **The table arm at the foot
/// of this function reintroduced exactly that duplicate** — it was added
/// after this paragraph and keyed on the column this paragraph names — until
/// the explicit `NoteBlock` refusal in front of it.
fn use_sound(
    classes: Option<&crate::placedby::PlacedByTable>,
    uses: Option<&crate::useblock::UseTable>,
    clicker: u64,
    was: &str,
    next: &str,
    pos: crate::interact::BlockPos,
) -> Option<crate::sounds::Sound> {
    let block = was.split('[').next().unwrap_or(was);
    let powered = crate::placement::property_of(next, "powered") == Some("true");
    if block == "minecraft:lever" {
        // `LeverBlock.useWithoutItem` -> `pull(state, level, pos, null)`, and
        // the `null` is `aconst_null` at offset 51 — the flipper hears it.
        return Some(crate::sounds::lever_click(pos, powered));
    }
    // `ComparatorBlock.useWithoutItem` — the third family, and the first one
    // whose sound the clicker does not hear. Keyed on `MODE` off the state
    // being written, which is the cycled one vanilla reads its pitch from;
    // see `crate::sounds::comparator_click` for why that is not the same as
    // reading `POWERED`.
    //
    // **Added beside the other two, not in place of them.** The two arms
    // below are unchanged and 184 blocks still reach `None` — a comparator
    // is a third answer, not a replacement for the lever's or the button's.
    if block == "minecraft:comparator" {
        let subtract = crate::placement::property_of(next, "mode") == Some("subtract");
        return Some(crate::sounds::comparator_click(
            pos,
            subtract,
            Some(clicker),
        ));
    }
    // `FenceGateBlock`, the fourth family, and the second one whose sound the
    // clicker does not hear: offsets 117-162 pass `player` to `playSound`.
    // Keyed on `OPEN` off the state being **written** — `astore_1` has already
    // replaced the local by offset 102 — so a gate going open sounds `open`.
    //
    // **This buys no verdict and is not expected to.** `700` was `match` on
    // both projections with `missing_kinds: ["sound"]` beside it before this
    // arm existed, because both fence-gate checks `observe_from` the actor and
    // the actor is the one player vanilla excludes. It is here because the
    // mechanism has been read, which is the same reason the comparator's is.
    if classes.is_some_and(|c| c.is_a(block, crate::useblock::FENCE_GATE_BLOCK)) {
        let open = crate::placement::property_of(next, "open") == Some("true");
        return Some(crate::sounds::fence_gate(
            block,
            pos,
            open,
            Some(clicker),
            crate::sounds::nine_tenths_pitch(),
        ));
    }
    // `instanceof ButtonBlock`, out of the class extract, never "the name ends
    // in `_button`" — the same rule `crate::ticks` applies to decide whether a
    // scheduled tick is a button's.
    if classes.is_some_and(|c| c.is_a(block, crate::ticks::BUTTON_BLOCK)) {
        // `ButtonBlock.press` ends with `playSound(player, level, pos, true)`
        // — `iconst_1` at offset 43 — so the press is the one sound in this
        // server that excludes the player who caused it. A button that was
        // already pressed reaches `Used::Consumed(None)`, writes nothing and
        // never gets here, which is why this does not have to re-check
        // `powered` on `was`.
        return Some(crate::sounds::button_click(
            block,
            pos,
            powered,
            Some(clicker),
        ));
    }
    // **The table, for everything whose class nobody has transcribed.**
    //
    // Last, not first, and the four arms above are unchanged. Each of them
    // knows something the table does not: the comparator and the lever key
    // their pitch on the state being *written*, where the table's row is
    // keyed on the state that was clicked, and the fence gate's pitch is a
    // draw the table refuses to publish. A row that disagrees with a
    // transcribed class is the transcription's to win.
    //
    // `usable()` is what keeps this honest. The extract names an event for
    // 4,453 states and can fully describe 1,509 of them; the rest sound with
    // a pitch vanilla drew, and this returns `None` for those — the same
    // silence they have now. **That is the finding, not a limitation of the
    // wiring**: the roadmap priced this row as "the column is a boolean", and
    // an event id alone closes only the states whose pitch is a constant.
    // The others need their class read, exactly as before.
    // **Not a note block: its sound is a block event's, and it already has
    // one.** `NoteBlock.useWithoutItem` reaches `playNote`, whose only effect
    // is `level.blockEvent(pos, this, 0, 0)`; the sound comes out of
    // `NoteBlock.triggerEvent` -> `playSeededSound` when `runBlockEvents`
    // drains at the tick boundary — which is [`queue_note_block_event`]'s
    // path here. The use table's harness level delivers block events
    // synchronously, so it recorded that same sound against the click, and
    // playing the row as well sent the note **twice**: once ahead of the
    // block update and once after it. Measured on `430`: vanilla six actor
    // packets, this server seven, the extra one a `sound` at 0 ms.
    if classes.is_some_and(|c| c.is_a(block, NOTE_BLOCK_CLASS)) {
        return None;
    }
    let s = uses?.sound_for(was)?;
    // **Doors and trapdoors: the table's event, the shared formula's pitch.**
    //
    // These are the whole of the remainder. Every one of the 2,944 states the
    // table cannot fully describe is a door (1,280), a trapdoor (1,280) or a
    // fence gate (384) — measured, not assumed, and the fence gate already has
    // its own arm above. All three carry volume 1.0 and exclude the actor, and
    // all three draw their pitch from the identical
    // `nextFloat() * 0.1F + 0.9F`.
    //
    // So the split this function makes is not "table" against "hand-ported".
    // It is **which half of the packet each source can answer for**: the
    // extract knows the event, which varies per block set and would have cost
    // 2,560 transcribed constants; the bytecode knows the pitch, which is one
    // formula shared by three classes and which the extract must not publish
    // because the harness fixes its seed.
    //
    // The pitch is a fresh draw here and cannot match vanilla's. That is not a
    // regression against emitting nothing, and it is not gradeable either way
    // — `oracle-runner` drops `sound.pitch`. It IS a real packet where there
    // was silence.
    if !s.usable()
        && classes.is_some_and(|c| {
            c.is_a(block, crate::useblock::DOOR_BLOCK)
                || c.is_a(block, crate::useblock::TRAPDOOR_BLOCK)
        })
    {
        return Some(crate::sounds::Sound {
            event: s.event.clone(),
            source: s.source,
            at: crate::sounds::Eighths::block(pos),
            pos: crate::sounds::block_centre(pos),
            volume: s.volume,
            pitch: crate::sounds::nine_tenths_pitch(),
            except: if s.excludes_actor {
                Some(clicker)
            } else {
                None
            },
        });
    }
    if !s.usable() {
        return None;
    }
    Some(crate::sounds::Sound {
        event: s.event.clone(),
        source: s.source,
        at: crate::sounds::Eighths::block(pos),
        pos: crate::sounds::block_centre(pos),
        volume: s.volume,
        pitch: s.pitch?,
        // The first argument to `Level.playSound` is the entity that does
        // *not* hear it, which the extract records per row.
        except: if s.excludes_actor {
            Some(clicker)
        } else {
            None
        },
    })
}

/// Record every block that changed, and send the level events that came with
/// them.
///
/// # The two halves are on different schedules and that is the whole point
///
/// `Level.setBlock` reaches `ChunkHolder.blockChanged`, which only writes the
/// position into a per-section set. The packets are a **tick-boundary**
/// event, sent by `ServerChunkCache.tick`, coalesced per section, and
/// followed by the acknowledgement. Sending here instead put this server's
/// broadcast *before* the two resyncs vanilla sends by hand and made every
/// placement check diverge at index 0. See `crate::broadcast`.
///
/// A level event is the opposite: `ServerLevel.levelEvent` calls
/// `PlayerList.broadcast` directly, with no queue anywhere on the path, so it
/// is on the wire the moment the block was destroyed — **before** the block
/// update for the same position, which is still sitting in the section set.
/// Sending it here reproduces that order, and the recorded traces show it:
/// every cascade dig in the corpus has the `level_event` at or one
/// millisecond before its `section_blocks_update`.
///
/// Anything that fails to encode is dropped rather than propagated, for the
/// reason every other broadcast here does: this runs inside a handler that
/// still owes the client an acknowledgement, and an error return would skip
/// it.
pub(crate) fn record_updates(s: &Session<'_>, updates: &crate::shapes::Effects) {
    // The scheduled ticks a rule asked for on this handler's cascade — the
    // torch that has to go out in two, the repeater that has to pass its
    // signal on in four, the observer that has to pulse. `s.clock.ticks()` is
    // `Level.getGameTime()`; see `crate::ticks::drain_requests` for why one
    // reading covers the whole batch.
    if let Some(ticks) = s.ticks {
        crate::ticks::drain_effects(ticks, updates, s.clock.ticks());
    }
    // `ServerLevel.levelEvent` broadcasts at a **constant** 64 blocks —
    // `ldc2_w double 64.0d` at its offset 41 — and at the block's raw
    // coordinates, `getX(); i2d` with no half-block. That is not the sound
    // packet's rule: a sound at a block sits at the block's *centre* and
    // carries `getRange(volume)`. Two channels, two conventions, and reading
    // one off the other is how a level event ends up half a block out.
    let conns = s.players.connections();
    crate::shapes::each_event_and_drop(
        &updates.level_events,
        &updates.drops,
        |event, pos, data| {
            if let Ok(body) = crate::interact::level_event_body(event, pos, data, false) {
                crate::playerlist::broadcast_within(
                    &conns,
                    &crate::playerlist::entity_locator(Some(crate::entities::Entities::shared())),
                    None,
                    (f64::from(pos.0), f64::from(pos.1), f64::from(pos.2)),
                    crate::playerlist::LEVEL_EVENT_RANGE,
                    ids::play::cb::LEVEL_EVENT,
                    &body,
                );
            }
        },
        |drop| crate::dig::cascade_drop(s.players, drop),
    );
    // `spawnGolemInWorld`'s `addFreshEntity`, after the clear's level events
    // and before the placement sound the caller plays next — the order the
    // pumpkin's `onPlace` produced them in. Pairing is synchronous
    // (`ChunkMap.addEntity` -> `updatePlayers`), so the `add_entity` and the
    // golem's data leave here, not at the tick boundary.
    if !updates.golems.is_empty() {
        let entities = crate::entities::Entities::shared();
        if let (Ok(world), Some(kinds)) = (
            crate::tiles::Tiles::shared(crate::status::WORLD_SEED, crate::status::WORLD_RADIUS),
            crate::entities::shared_kind_ids(),
        ) {
            let players = crate::status::tracked_players(s.players, entities);
            for req in &updates.golems {
                crate::mob::spawn_golem(
                    world,
                    req,
                    entities,
                    crate::tracker::Tracker::shared(),
                    &players,
                    kinds,
                );
            }
        }
    }
    for &(pos, _) in &updates.blocks {
        s.changed.record(pos);
    }
}

/// `BlockItem.canPlace`'s second half — `Level.isUnobstructed(state, pos,
/// CollisionContext.placementContext(player))` — over the entity store.
/// See [`crate::obstruct`].
///
/// A state with no collision-shape row (one of the 23 dynamic shapes) or a
/// checkout with no table is **allowed**: refusing what cannot be adjudicated
/// would stop a scaffold being placed at all.
fn placement_unobstructed(
    s: &Session<'_>,
    blocks: &dyn crate::edits::Blocks,
    target: crate::interact::BlockPos,
    state: &str,
) -> bool {
    let Some(table) = crate::collide::CollisionShapes::shared() else {
        return true;
    };
    let Some(shape) = blocks
        .state_id(state)
        .and_then(|id| u32::try_from(id).ok())
        .and_then(|id| table.get(id))
    else {
        return true;
    };
    if shape.is_empty() {
        return true;
    }
    // The placer's own box comes from the connection's own position — what
    // `ServerPlayer.getBoundingBox()` is — rather than the entity store's
    // copy of it, which is written from the same packet; everyone else's
    // from the store.
    let me = s.conn.entity_id();
    let all: Vec<crate::entities::Entity> = crate::entities::Entities::shared()
        .all()
        .into_iter()
        .filter(|e| e.id != me)
        .collect();
    let own = crate::pickup::make_bounding_box(
        if s.player.shift_key_down {
            crate::pickup::CROUCHING
        } else {
            crate::pickup::STANDING
        },
        s.player.x,
        s.player.y,
        s.player.z,
    );
    if !crate::obstruct::is_unobstructed_by_box(shape, target, &own) {
        return false;
    }
    crate::obstruct::is_unobstructed(shape, target, &all, |e| {
        crate::obstruct::blocking_box(e, me, false)
    })
}

/// The player crossed a chunk border: move the cache centre, send what is
/// new, forget what is behind.
/// `ChunkMap.updateChunkTracking` + `applyChunkTrackingView` on the wire.
///
/// Writes `set_chunk_cache_center` **only when the centre moved**, then the
/// `forget_level_chunk` packets for the departures, in `difference` order.
/// The arrivals produce no bytes here: they go into the chunk sender's queue
/// and leave in [`send_next_chunks`]'s batches.
///
/// This is what stream_chunks used to be, and the difference from it is not
/// cosmetic. The old version sent the centre packet on every call, walked a
/// Chebyshev square, and pushed the whole delta out in one unbounded batch.
fn update_chunk_tracking(s: &mut Session<'_>, centre: (i32, i32)) -> io::Result<()> {
    let update = s.conn.with_chunk_view(|v| v.update_tracking(centre));
    if let Some(c) = update.moved_center {
        let mut buf = Vec::new();
        write_varint(&mut buf, c.0)?;
        write_varint(&mut buf, c.1)?;
        s.conn.send(ids::play::cb::SET_CHUNK_CACHE_CENTER, &buf)?;
    }
    for c in update.forget {
        s.conn
            .send(ids::play::cb::FORGET_LEVEL_CHUNK, &forget_body(c)?)?;
    }
    Ok(())
}

/// `PlayerChunkSender.sendNextChunks` — one tick's batch, written out.
///
/// Called once per server tick, which is where
/// `ServerGamePacketListenerImpl.tick` calls it. Calling it once per read
/// loop iteration instead would send faster than vanilla by whatever ratio
/// this loop spins above 20 Hz, and the quota arithmetic is per-call.
fn send_next_chunks(s: &mut Session<'_>) -> io::Result<()> {
    let centre = s.player.chunk();
    // The chunk bodies, resolved once. `available` has to answer
    // `getChunkToSend(pos) != null` *before* the batch is chosen, and the
    // chosen chunks then have to be encoded; asking the world twice would
    // either double the work or, for a tile that finishes generating between
    // the two calls, disagree with itself.
    let resolved: std::cell::RefCell<
        std::collections::BTreeMap<(i32, i32), Option<LevelChunkWithLight>>,
    > = std::cell::RefCell::new(std::collections::BTreeMap::new());
    // A world error must not be reported as "that chunk is not there yet":
    // the first is a fault and the second is normal, and conflating them is
    // how a broken world reads as a slow one. Stashed and re-raised below.
    let failed: std::cell::RefCell<Option<io::Error>> = std::cell::RefCell::new(None);
    // The chunk view's lock is held across `available`, which reaches
    // `Tiles::chunk`. That is bounded and deliberate: a tile that is not
    // ready answers `None` and *starts* generating on its own thread rather
    // than blocking here, so the longest this holds is a map lookup and a
    // chunk encode. Nothing in the closure takes `broadcast::tick_lock` —
    // see `Connection::chunk_view` for why that rule is what keeps this a
    // leaf lock.
    let batch = {
        let Session { conn, chunk_at, .. } = &mut *s;
        conn.with_chunk_view(|view| {
            view.take_batch(centre, |c| {
                let mut m = resolved.borrow_mut();
                let e = m.entry(c).or_insert_with(|| match chunk_at(c.0, c.1) {
                    Ok(v) => v,
                    Err(e) => {
                        *failed.borrow_mut() = Some(e);
                        None
                    }
                });
                e.is_some()
            })
        })
    };
    if let Some(e) = failed.into_inner() {
        return Err(e);
    }
    if batch.is_empty() {
        return Ok(());
    }
    s.conn.send(ids::play::cb::CHUNK_BATCH_START, &[])?;
    let resolved = resolved.into_inner();
    for c in &batch {
        let chunk = resolved
            .get(c)
            .and_then(Option::as_ref)
            .expect("take_batch only returns chunks `available` said were there");
        s.conn
            .send(ids::play::cb::LEVEL_CHUNK_WITH_LIGHT, &chunk.encode()?)?;
    }
    let mut fin = Vec::new();
    write_varint(&mut fin, i32::try_from(batch.len()).unwrap_or(i32::MAX))?;
    s.conn.send(ids::play::cb::CHUNK_BATCH_FINISHED, &fin)?;
    eprintln!(
        "player at ({:.1}, {:.1}, {:.1}) chunk {centre:?} — sent {} chunks, {} still queued",
        s.player.x,
        s.player.y,
        s.player.z,
        batch.len(),
        s.conn.with_chunk_view(|v| v.pending_len())
    );
    Ok(())
}

#[cfg(test)]
/// A test connection's view, opened the way a real join opens one.
///
/// `enter_world` writes the spawn blob straight onto the socket and then
/// tells the view about it through `mark_sent`; the read loop's first act
/// is `ChunkMap.updatePlayerStatus(player, true)`, whose tail is
/// `updateChunkTracking`. A connection that has had neither still holds
/// `ChunkTrackingView.EMPTY`, and vanilla withholds every block update
/// from a client in that state — so a harness that skipped this would
/// measure silence and read it as a defect.
///
/// The square is `view_distance + 2` on a side because the tracked set is
/// a disc padded by two: a plain `view_distance` square would leave the
/// axis chunks at `+3` pending, and `isChunkTracked` is false for a
/// pending chunk.
fn open_test_view(conn: &crate::players::Connection) {
    conn.with_chunk_view(|v| {
        for dx in -4..=4 {
            for dz in -4..=4 {
                v.mark_sent((dx, dz));
            }
        }
        v.update_tracking((0, 0));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flat_chunk_has_ground_below_and_air_above() {
        let c = flat_chunk(0, 0, StateId(1), StateId(0), 64, 0);
        assert_eq!(c.sections.len(), OVERWORLD_SECTIONS);

        // y = -64..63 is stone: sections 0..7 full, section 8 (y 64..79) empty.
        assert_eq!(c.sections[0].non_empty_blocks, 4096, "bottom section solid");
        assert_eq!(c.sections[8].non_empty_blocks, 0, "above the floor is air");

        let values = c.sections[0].states.values(ContainerKind::BlockStates);
        assert!(values.iter().all(|v| *v == 1), "all floor");
        assert_eq!(
            c.sections[0].fluid_count, 0,
            "the field must be present even when zero"
        );
    }

    /// Nine bits a column, seven to a long, 256 columns — the shape every
    /// captured heightmap has.
    #[test]
    fn the_heightmap_packs_to_thirty_seven_longs() {
        let h = packed_heightmap(128);
        assert_eq!(h.len(), 37);
        // The first column is the low nine bits of the first long.
        assert_eq!(h[0] & 0x1ff, 128);
        // And the second sits nine bits up, not eight or sixteen.
        assert_eq!((h[0] >> 9) & 0x1ff, 128);
    }

    /// The order is the part that goes wrong silently, so it is asserted.
    #[test]
    fn the_packets_go_out_in_the_order_a_client_needs() {
        let mut out = Vec::new();
        let chunks = vec![flat_chunk(0, 0, StateId(1), StateId(0), 64, 0)];
        enter_world(
            &mut Pipe(&mut out),
            &crate::play::JoinInfo::default(),
            &chunks,
            (8.0, 65.0, 8.0),
            None,
            &[],
        )
        .expect("sent");

        let mut cur = &out[..];
        let mut ids_seen = Vec::new();
        while let Ok((id, _)) = frame::read_frame(&mut cur, false) {
            ids_seen.push(id);
        }
        assert_eq!(
            ids_seen,
            vec![
                ids::play::cb::LOGIN,
                ids::play::cb::INITIALIZE_BORDER,
                ids::play::cb::GAME_EVENT,
                ids::play::cb::SET_CHUNK_CACHE_CENTER,
                ids::play::cb::CHUNK_BATCH_START,
                ids::play::cb::LEVEL_CHUNK_WITH_LIGHT,
                ids::play::cb::CHUNK_BATCH_FINISHED,
                ids::play::cb::PLAYER_POSITION,
            ],
            "the border before the game event, the game event before the chunks, \
             the teleport after them"
        );
    }

    /// `PlayerList.sendLevelInfo`'s weather arm lands between the join
    /// packet and `LEVEL_CHUNKS_LOAD_START`, and its bodies are the one-byte
    /// id plus the big-endian float `ClientboundGameEventPacket.write`
    /// writes.
    #[test]
    fn a_player_joining_a_raining_world_gets_the_weather_before_the_chunks() {
        use crate::weather::Event;
        let mut out = Vec::new();
        let chunks = vec![flat_chunk(0, 0, StateId(1), StateId(0), 64, 0)];
        enter_world(
            &mut Pipe(&mut out),
            &crate::play::JoinInfo::default(),
            &chunks,
            (8.0, 65.0, 8.0),
            None,
            &[
                (Event::StartRaining, 0.0),
                (Event::RainLevelChange, 0.5),
                (Event::ThunderLevelChange, 0.25),
            ],
        )
        .expect("sent");

        let mut cur = &out[..];
        let mut seen = Vec::new();
        while let Ok((id, body)) = frame::read_frame(&mut cur, false) {
            seen.push((id, body));
        }
        let events: Vec<(u8, f32)> = seen
            .iter()
            .filter(|(id, _)| *id == ids::play::cb::GAME_EVENT)
            .map(|(_, b)| (b[0], f32::from_be_bytes([b[1], b[2], b[3], b[4]])))
            .collect();
        assert_eq!(
            events,
            vec![(1, 0.0), (7, 0.5), (8, 0.25), (13, 0.0)],
            "START_RAINING, RAIN_LEVEL_CHANGE, THUNDER_LEVEL_CHANGE, then 13"
        );
        // And the join packet is still first, so nothing was inserted ahead
        // of it.
        assert_eq!(seen[0].0, ids::play::cb::LOGIN);
    }

    /// The join writes a teleport id to the wire and `crate::status` arms the
    /// player state with one. **If they disagree, no client can ever move**,
    /// and nothing else in this repository would say so: every
    /// acknowledgement would fail the `if_icmpne` at
    /// `handleAcceptTeleportPacket` offset 20, the awaiting position would
    /// never clear, and every movement packet would be discarded by a server
    /// that looked otherwise healthy and was answering keep-alives.
    ///
    /// So the id is read back off the bytes `enter_world` actually produced,
    /// rather than compared to the constant — a test that quoted the constant
    /// on both sides would pass on any pair of values.
    #[test]
    fn the_join_teleport_id_is_the_one_the_player_state_awaits() {
        let mut out = Vec::new();
        let spawn = (8.0, 65.0, 8.0);
        let chunks = vec![flat_chunk(0, 0, StateId(1), StateId(0), 64, 0)];
        enter_world(
            &mut Pipe(&mut out),
            &crate::play::JoinInfo::default(),
            &chunks,
            spawn,
            None,
            &[],
        )
        .expect("sent");

        let mut cur = &out[..];
        let mut on_the_wire = None;
        while let Ok((id, body)) = frame::read_frame(&mut cur, false) {
            if id == ids::play::cb::PLAYER_POSITION {
                let mut r = body.as_slice();
                on_the_wire = rustmc_proto::varint::read_varint(&mut r).ok();
            }
        }
        let on_the_wire = on_the_wire.expect("the join sent a player_position");

        // Exactly what `crate::status` does after `enter_world` returns.
        let mut player = crate::player::PlayerState::at(spawn.0, spawn.1, spawn.2);
        player.awaiting(JOIN_TELEPORT_ID, spawn);

        let mut ack = Vec::new();
        write_varint(&mut ack, on_the_wire).expect("buf");
        assert_eq!(
            crate::player::apply(crate::player::ids::ACCEPT_TELEPORTATION, &ack, &mut player)
                .expect("decoded"),
            crate::player::Update::Teleport(on_the_wire),
            "the id on the wire ({on_the_wire}) is not the id the player awaits \
             ({JOIN_TELEPORT_ID}) — every client would be frozen at spawn"
        );
        assert_eq!(player.awaiting_position, None);
    }

    struct Pipe<'a>(&'a mut Vec<u8>);
    impl Read for Pipe<'_> {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Ok(0)
        }
    }
    impl Write for Pipe<'_> {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            self.0.write(b)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
}

#[cfg(test)]
mod dig_tests {
    use super::*;
    use crate::edits::Blocks;
    use crate::interact::{block_changed_ack_body, pack_pos, BlockPos};
    use std::sync::{Arc, Mutex};

    /// A writer whose bytes survive the `Connection` that owns it.
    #[derive(Clone, Default)]
    pub(super) struct Recorder(pub(super) Arc<Mutex<Vec<u8>>>);
    impl Write for Recorder {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            self.0.lock().expect("recorder").extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// A world that records what it was asked to write. Standing in for the
    /// real one so this tests the *loop*, which needs no vanilla extract.
    ///
    /// `solid` are the positions that are **not** replaceable; everything
    /// else is. That is the one world fact placement branches on, so it is
    /// the one this fake carries.
    #[derive(Default)]
    pub(super) struct FakeWorld {
        wrote: Mutex<Vec<(BlockPos, String)>>,
        solid: Vec<BlockPos>,
        /// Positions holding a chest, so a click can reach a block that
        /// opens a menu. Kept separate from `solid` rather than folded into
        /// it: `state_at` answering one string for everything is what makes
        /// the rest of these fixtures readable, and a chest is the first
        /// block whose *identity* a handler branches on.
        chests: Vec<BlockPos>,
        /// Arbitrary states, for the placements whose rule reads the state
        /// it is being placed *into* rather than whether it is solid — a
        /// slab merging into a slab, a snow layer deepening.
        ///
        /// Searched before `chests` and `solid`, so a fixture can put
        /// anything anywhere without the other two growing a case for it.
        at: Vec<(BlockPos, String)>,
    }
    impl Blocks for FakeWorld {
        fn state_at(&self, pos: BlockPos) -> String {
            // A write sticks: `Level.setBlock` reads the position back at
            // offset 56 and skips its own updates when `onPlace` rewrote it,
            // so a world whose writes vanish would look rewritten after
            // every one. The latest write wins.
            if let Some((_, s)) = self
                .wrote
                .lock()
                .expect("fake world")
                .iter()
                .rev()
                .find(|(p, _)| *p == pos)
            {
                return s.clone();
            }
            // Everything solid is stone; everything else is air. The one
            // thing placement reads a neighbour's state for is
            // waterlogging, which this fake never triggers.
            if let Some((_, s)) = self.at.iter().find(|(p, _)| *p == pos) {
                s.clone()
            } else if self.chests.contains(&pos) {
                "minecraft:chest[facing=north,type=single,waterlogged=false]".into()
            } else if self.solid.contains(&pos) {
                "minecraft:stone".into()
            } else {
                "minecraft:air".into()
            }
        }
        fn has_state(&self, _: &str) -> bool {
            true
        }
        /// A test double: anything that is not air is sturdy on every face.
        /// Closer to true than a blanket `false`, which would make every
        /// ported placement rule refuse and pass for the wrong reason.
        fn face_sturdy(&self, pos: BlockPos, _: crate::interact::Face) -> bool {
            self.state_at(pos) != "minecraft:air"
        }
        /// A test double. `false` is the honest answer for a world of air
        /// and stone that no test asks a redstone question of; the moment one
        /// does, it overrides this rather than inheriting a default that
        /// cannot be seen.
        fn redstone_conductor(&self, _: BlockPos) -> bool {
            false
        }
        fn signal_source(&self, _: BlockPos) -> bool {
            false
        }
        fn state_id(&self, _: &str) -> Option<i32> {
            Some(1)
        }
        fn set_block(&self, pos: BlockPos, state: &str) -> Option<i32> {
            self.wrote
                .lock()
                .expect("fake world")
                .push((pos, state.to_string()));
            // A non-air id, so a caller that broadcast the *old* state would
            // be visible in the assertion below.
            Some(7)
        }
        fn default_state(&self, block: &str) -> Option<String> {
            // Everything named like a block is one, except this, which
            // stands in for a sword: something a client can be holding that
            // has no block to place.
            (block != "minecraft:not_a_block").then(|| format!("{block}[placed=true]"))
        }
        fn replaceable(&self, pos: BlockPos, ctx: &crate::placement::Replacing<'_>) -> bool {
            // Read off `state_at` rather than off `solid` directly. It used
            // to be `!self.solid.contains(&pos)`, which called a **chest**
            // replaceable — the one position in this fake that is neither
            // air nor `solid`. No test could see it while every chest click
            // was consumed by the block and never reached the placement
            // path; a sneaking click reaches it, and would have placed stone
            // into the chest.
            //
            // The flag is the props table's no-argument `canBeReplaced()`
            // for the kinds of block this fake can hold, read off the real
            // `block_props.json`: air, every `snow[layers=N]` and tall grass
            // yes; stone, chest and every slab state no. The rule on top of
            // it is the production one, so a fixture cannot pass by having
            // its own idea of what replaces what.
            //
            // **`tall_grass` is in this list because leaving it out made a
            // check unable to fail.** Without it the item-aware case in
            // `a_placement_goes_against_the_face_unless_the_click_was_replaceable`
            // passed by tall grass not being replaceable at all rather than
            // by the held item making it so, and it went on passing with the
            // rule's second conjunct deleted.
            let state = self.state_at(pos);
            let flag = state == "minecraft:air"
                || state.starts_with("minecraft:snow[")
                || state.starts_with("minecraft:tall_grass");
            crate::placement::can_be_replaced(&state, flag, ctx)
        }
        fn legacy_fluid_at(&self, _: BlockPos) -> String {
            // No water in this fake, so removing anything leaves air.
            "minecraft:air".to_string()
        }
        fn water_at(&self, _: BlockPos) -> bool {
            false
        }
    }

    pub(super) fn dig(action: i32, pos: BlockPos, sequence: i32) -> Vec<u8> {
        let mut body = Vec::new();
        write_varint(&mut body, action).expect("buf");
        wire::write_i64(&mut body, pack_pos(pos)).expect("buf");
        wire::write_u8(&mut body, 1).expect("buf");
        write_varint(&mut body, sequence).expect("buf");
        let mut frame = Vec::new();
        frame::write_frame(&mut frame, None, ids::play::sb::PLAYER_ACTION, &body).expect("buf");
        frame
    }

    /// The frames a connection was sent, as (id, body).
    type Sent = Vec<(i32, Vec<u8>)>;
    /// The writes the world was asked for.
    type Wrote = Vec<(BlockPos, String)>;

    /// Feed a stream of client packets through the loop and report what went
    /// out and what the world was asked to do.
    ///
    /// The player stands at (8, 64, 8), so blocks (8, 64, 8) and (8, 65, 8)
    /// are their own body.
    pub(super) fn play(input: &[u8], creative: bool, world: &FakeWorld) -> (Sent, Wrote) {
        play_with(input, creative, world, None, None)
    }

    /// The same loop with the two tables the container channel needs. Split
    /// out rather than added to every caller: `uses` decides whether a click
    /// reaches the block at all, and without `protocol_ids` no menu can name
    /// its type, so a fixture that wants neither should not have to say so.
    fn play_with(
        input: &[u8],
        creative: bool,
        world: &FakeWorld,
        uses: Option<&crate::useblock::UseTable>,
        protocol_ids: Option<&crate::protocol_ids::ProtocolIds>,
    ) -> (Sent, Wrote) {
        play_tables(
            input,
            creative,
            world,
            uses,
            protocol_ids,
            None,
            None,
            None,
            None,
        )
    }

    /// The same loop again with the hotbar filled directly and with the two
    /// tables the **recipe book** channel needs.
    ///
    /// One helper rather than two because the three extra arguments arrived
    /// from two directions for two reasons and both reasons are live. It is
    /// separate from [`play_with`] for the same reason `play_with` is
    /// separate from [`play`] — a fixture that wants none of this should not
    /// have to say so three times — and each argument exists because a
    /// fixture without it cannot *reach* a branch:
    ///
    /// * `held` — `items` is `None` in every fixture on the front doors
    ///   above, so their `hold` frames resolve to nothing and every one of
    ///   them has an empty hand. `isSecondaryUseActive` is a conjunction
    ///   with `holdingSomething`, so a fixture that cannot hold anything
    ///   cannot reach the sneaking branch at all, and a test written on
    ///   [`play_with`] would have passed by never arriving.
    /// * `recipe_unlocks` — until this existed every fixture in this module
    ///   passed `recipe_unlocks: None`, so the `ServerRecipeBook.addRecipes`
    ///   arm in [`keep_alive_loop`] was reached by **no test in the tree**:
    ///   `data/vanilla/recipe_unlocks.json` is gitignored and generated, and
    ///   the corpus — a fifty-minute instrument — was the sole witness to
    ///   that line.
    /// * `items` — not optional decoration beside it.
    ///   `crate::player::creative_slot_item` resolves the numeric item id
    ///   through it and returns `None` without it, so a fixture that forgets
    ///   the item table gets silence and reads it as a passing negative
    ///   control.
    ///
    /// **`held` and `items` are two different ways to fill a hand and are
    /// not interchangeable.** `held` writes the hotbar slot directly and
    /// needs no table; a `hold` frame goes through `creative_slot_item` and
    /// therefore *is* the test of `items`. So a caller reaching the recipe
    /// book passes `items` and leaves `held` alone, and a caller that only
    /// needs a full hand passes `held` and leaves `items` alone — swapping
    /// them silently deletes what the other one was proving.
    ///
    /// * `item_uses` — `useItemOn` runs **in front of** `useWithoutItem`, so
    ///   a fixture without it cannot reach the item-aware branch at all and a
    ///   flower pot cannot be filled. See [`crate::useitemon`].
    ///
    /// # Nine arguments, and the count is load-bearing
    ///
    /// `clippy::too_many_arguments` warns **above** seven, not at it, and an
    /// `#[expect]` that is never fulfilled is itself a hard error under
    /// `-D unfulfilled-lint-expectations` — implied by `-D warnings`. So the
    /// annotation below is wrong at seven arguments and right at eight or
    /// more, and both halves of that have been paid for: the seven-argument
    /// version of this helper carried this same `#[expect]` and T0 rejected
    /// it, exit 101 at `world_send.rs:1920`, `skipping: no vanilla` = 0 on
    /// that run. Adding `held` took it to eight and `item_uses` to nine; the
    /// lint fires, the expectation is fulfilled.
    ///
    /// **Count before you change this list in either direction.** Dropping
    /// back to seven re-breaks the build exactly as surely as adding one
    /// without an annotation would, and neither failure names the argument
    /// that caused it.
    #[expect(
        clippy::too_many_arguments,
        reason = "one argument per table the loop reads; a struct here would \
                  only move the same list somewhere a caller cannot see it"
    )]
    pub(super) fn play_tables(
        input: &[u8],
        creative: bool,
        world: &FakeWorld,
        uses: Option<&crate::useblock::UseTable>,
        protocol_ids: Option<&crate::protocol_ids::ProtocolIds>,
        held: Option<&str>,
        items: Option<&crate::player::ItemIds>,
        recipe_unlocks: Option<&crate::recipe_book::RecipeUnlocks>,
        item_uses: Option<&crate::useitemon::UseItemOnTable>,
    ) -> (Sent, Wrote) {
        play_tables_adv(
            input,
            creative,
            world,
            uses,
            protocol_ids,
            held,
            items,
            recipe_unlocks,
            item_uses,
            None,
        )
    }

    /// [`play_tables`] plus the advancement engine, which is a **tenth**
    /// argument and therefore a second function rather than a tenth
    /// parameter on the first: fourteen existing call sites want none of it,
    /// and the argument this file's own header makes about `recipe_unlocks`
    /// — a table nothing in the tree could reach, leaving a fifty-minute
    /// corpus as the sole witness to one line — is exactly why the arm needs
    /// a unit test at all.
    ///
    /// The engine borrows `'static`, which in a test means leaking a table.
    /// That is deliberate and cheap: `cargo test` is one process per binary
    /// and the tables are a few hundred bytes each.
    #[expect(
        clippy::too_many_arguments,
        reason = "one argument per table the loop reads; a struct here would \
                  only move the same list somewhere a caller cannot see it"
    )]
    fn play_tables_adv(
        input: &[u8],
        creative: bool,
        world: &FakeWorld,
        uses: Option<&crate::useblock::UseTable>,
        protocol_ids: Option<&crate::protocol_ids::ProtocolIds>,
        held: Option<&str>,
        items: Option<&crate::player::ItemIds>,
        recipe_unlocks: Option<&crate::recipe_book::RecipeUnlocks>,
        item_uses: Option<&crate::useitemon::UseItemOnTable>,
        adv_engine: Option<crate::advancements::Engine>,
    ) -> (Sent, Wrote) {
        let rec = Recorder::default();
        let players = crate::players::Players::new();
        let conn = Arc::new(crate::players::Connection::with_entity(
            players.next_id(),
            "Tester",
            Box::new(rec.clone()),
            None,
            0,
            2,
            (0, 0),
        ));
        players.join(&conn);
        // The join's own two steps, which is what puts this connection into
        // the recipient list of `ChunkHolder.broadcastChanges`:
        // `enter_world` writes the spawn blob and tells the view
        // (`mark_sent`), then `ChunkMap.updatePlayerStatus(player, true)`
        // runs `updateChunkTracking`. A view that has had neither is `EMPTY`,
        // and vanilla withholds every block update from a client in that
        // state.
        open_test_view(&conn);
        let clock = crate::tick::GameClock::new();
        let changed = crate::broadcast::ChangedBlocks::new();
        let block_events = crate::containers::BlockEvents::new();
        let openers = crate::containers::Openers::new();
        let mut player = crate::player::PlayerState::at(8.0, 64.0, 8.0);
        let mut hotbar = crate::player::Hotbar::new();
        let mut menu = crate::menu::MenuState::new();
        let mut known_recipes = std::collections::BTreeSet::new();
        let mut player_advancements = adv_engine.map(|e| e.new_player());
        // `crate::status` does this once per connection to reproduce
        // `ServerPlayer.tick`'s first, empty flush, which is what clears
        // `isFirstPacket`. Without it every fixture's first packet would
        // carry `reset = true` where a real one carries false.
        if let Some(pa) = player_advancements.as_mut() {
            assert!(
                pa.flush_dirty(true).expect("write").is_none(),
                "a fresh player has nothing to flush"
            );
        }
        hotbar.set(0, held);
        let chunk_at = |_: i32, _: i32| Ok(None);
        // A private manager per test: `cargo test` runs these on
        // parallel threads in one process, and the shared one would
        // let one test's join make a connection another test's move
        // then reports on.
        let wp = crate::waypoints::Waypoints::new();
        let mut session = Session {
            conn: &conn,
            player: &mut player,
            clock: &clock,
            clock_ids: &[],
            players: &players,
            blocks: Some(world),
            hotbar: &mut hotbar,
            menu: &mut menu,
            items,
            placement: None,
            recipes: None,
            recipe_unlocks,
            known_recipes: &mut known_recipes,
            advancements: player_advancements.as_mut(),
            adv_engine,
            uses,
            item_uses,
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
            protocol_ids,
            chunk_at: &chunk_at,
            // No plugins. These fixtures assert what the *loop* does, and a
            // plugin in one of them would be a second thing able to change
            // the answer. The plugin hook has its own module below, with its
            // own world, because `FakeWorld` records writes rather than
            // applying them and so cannot answer "is the block still there".
            plugins: None,
            creative,
            seed: 0,
        };
        // The reader ends after the packets, which the loop treats as the
        // player quitting.
        let mut cursor = std::io::Cursor::new(input.to_vec());
        keep_alive_loop(&mut cursor, &mut session).expect("the loop ends on EOF");
        // The tick boundary the clock thread supplies in a real server, and
        // the reason these fixtures see anything at all: a handler only
        // *records* what it changed and only *remembers* the sequence it
        // owes. Broadcasts first, acknowledgements after — `ServerLevel.tick`
        // reaches `ServerChunkCache.tick` before
        // `ServerGamePacketListenerImpl.tick`, and the gameplay oracle
        // measured real vanilla in exactly that order.
        changed.flush(&players, world);
        players.flush_menu_sweeps();
        players.flush_acks();

        let bytes = rec.0.lock().expect("recorder").clone();
        let mut cur = &bytes[..];
        let mut out = Vec::new();
        while let Ok(frame) = frame::read_frame(&mut cur, false) {
            out.push(frame);
        }
        // `set_time` is the loop's once-a-WALL-second beat, and a sky change
        // anywhere in the process (`crate::sky::sync_generation`, which other
        // tests in this binary move) sends one at once. Neither is anything a
        // scenario here does, and both land in the middle of an exact packet
        // list whenever the session runs past a second or overlaps such a
        // test: measured on nether-parity 2026-10-04, `dig_tests` failed 9 of
        // 15 runs under a loaded box with a stray 113 in front, and that is
        // what turned a T1 gate red. Dropped here, where every caller's list
        // is made; the beat itself is pinned elsewhere (`send_time`'s tests).
        out.retain(|(id, _)| *id != ids::play::cb::SET_TIME);
        let wrote = world.wrote.lock().expect("fake world").clone();
        (out, wrote)
    }

    /// A minimal `use_without_item.json` and `protocol_ids.json`, on disk,
    /// because both loaders take a path.
    ///
    /// **Unique per caller, deliberately.** `cargo test` runs a module's
    /// tests on parallel threads in one process and `std::fs::write`
    /// truncates before it writes, so a read that lands inside another
    /// test's truncate gets an empty file and a parse error. That exact race
    /// made `player::tests` fail one run in three; see `ItemIds::parse`.
    fn menu_tables(
        tag: &str,
    ) -> (
        std::path::PathBuf,
        crate::useblock::UseTable,
        crate::protocol_ids::ProtocolIds,
    ) {
        let dir = std::env::temp_dir().join(format!("rustmc-menu-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("tmp");
        let uses_path = dir.join("use_without_item.json");
        // A chest consumes the click and writes nothing, which is what the
        // real extract records for all 24 of its states.
        std::fs::write(
            &uses_path,
            r#"{"table":[{"block":"minecraft:chest","states":[
                 {"from":"minecraft:chest[facing=north,type=single,waterlogged=false]",
                  "result":"Success","consumes":true,"schedulesTick":false,"writes":[]}]}]}"#,
        )
        .expect("write");
        let ids_path = dir.join("protocol_ids.json");
        std::fs::write(
            &ids_path,
            r#"{"menus":{"minecraft:generic_9x3":2},
                "blocks":{"minecraft:chest":201},
                "block_entity_types":{"minecraft:chest":1,"minecraft:sign":7},
                "sounds":{"minecraft:block.chest.open":349},
                "attributes":{}}"#,
        )
        .expect("write");
        (
            dir,
            crate::useblock::UseTable::load(&uses_path).expect("use table"),
            crate::protocol_ids::ProtocolIds::load(&ids_path).expect("protocol ids"),
        )
    }

    /// The three tables the flower pot needs, on disk, cut down to one pot and
    /// one flower. Unique per caller for the reason [`menu_tables`] is.
    ///
    /// The rows are the real extract's, copied in shape rather than in bulk:
    /// `flower_pot` + `dandelion` writes `potted_dandelion` with flags 3 and
    /// empties a one-count stack, `potted_dandelion`'s `useWithoutItem` writes
    /// `flower_pot` with flags 3, and everything else falls through.
    fn pot_tables(
        tag: &str,
    ) -> (
        std::path::PathBuf,
        crate::useblock::UseTable,
        crate::useitemon::UseItemOnTable,
        crate::player::ItemIds,
    ) {
        let dir = std::env::temp_dir().join(format!("rustmc-pot-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("tmp");
        let uses_path = dir.join("use_without_item.json");
        std::fs::write(
            &uses_path,
            r#"{"table":[{"block":"minecraft:potted_dandelion","states":[
                 {"from":"minecraft:potted_dandelion","result":"Success","consumes":true,
                  "schedulesTick":false,
                  "writes":[{"at":[0,0,0],"state":"minecraft:flower_pot","flags":3}]}]}]}"#,
        )
        .expect("write");
        let item_uses_path = dir.join("use_item_on.json");
        std::fs::write(
            &item_uses_path,
            r#"{"table":[
                 {"block":"minecraft:flower_pot","class":"FlowerPotBlock","random":false,
                  "faceSensitive":false,"handSensitive":false,"probeDependent":true,
                  "probeAxes":["game_master"],"probeUnknown":[],
                  "states":[{"from":"minecraft:flower_pot",
                    "default":{"result":"TRY_WITH_EMPTY_HAND","consumes":false,
                               "schedulesTick":false,"sound":false,"writes":[]},
                    "except":[{"items":["minecraft:dandelion"],
                      "cell":{"result":"SUCCESS","consumes":true,"schedulesTick":false,
                              "sound":false,"stackAfter":"empty",
                              "writes":[{"at":[0,0,0],"state":"minecraft:potted_dandelion",
                                         "flags":3}]}}]}]},
                 {"block":"minecraft:potted_dandelion","class":"FlowerPotBlock","random":false,
                  "faceSensitive":false,"handSensitive":false,"probeDependent":false,
                  "probeAxes":[],"probeUnknown":[],
                  "states":[{"from":"minecraft:potted_dandelion",
                    "default":{"result":"TRY_WITH_EMPTY_HAND","consumes":false,
                               "schedulesTick":false,"sound":false,"writes":[]},
                    "except":[{"items":["minecraft:dandelion"],
                      "cell":{"result":"CONSUME","consumes":true,"schedulesTick":false,
                              "sound":false,"writes":[]}}]}]}]}"#,
        )
        .expect("write");
        let items_path = dir.join("items.json");
        std::fs::write(
            &items_path,
            r#"{"items":{"minecraft:dandelion":141,"minecraft:stick":842,
                         "minecraft:flower_pot":1076}}"#,
        )
        .expect("write");
        (
            dir,
            crate::useblock::UseTable::load(&uses_path).expect("use table"),
            crate::useitemon::UseItemOnTable::load(&item_uses_path).expect("useItemOn table"),
            crate::player::ItemIds::load(&items_path).expect("items"),
        )
    }

    /// **`useItemOn` runs before `useWithoutItem`, and the pot fills.**
    ///
    /// This is the step that did not exist: `790`'s setup pots a dandelion,
    /// this server consumed the click and wrote nothing, and the emptying half
    /// — which *was* in the tree, in `use_without_item.json` — could therefore
    /// never fire because the pot was empty when the check clicked it.
    ///
    /// The placement path must not run either. A dandelion is a `BlockItem`
    /// and the click is on an `UP` face, so a server that let the click fall
    /// through would put a dandelion in the air above the pot.
    #[test]
    fn a_dandelion_pots_instead_of_being_placed_beside_the_pot() {
        let (dir, uses, item_uses, items) = pot_tables("fill");
        let world = FakeWorld {
            at: vec![((3, 70, 5), "minecraft:flower_pot".to_string())],
            ..FakeWorld::default()
        };
        let (_, wrote) = play_tables(
            &place((3, 70, 5), 1, 4),
            true,
            &world,
            Some(&uses),
            None,
            Some("minecraft:dandelion"),
            Some(&items),
            None,
            Some(&item_uses),
        );
        assert_eq!(
            wrote,
            vec![((3, 70, 5), "minecraft:potted_dandelion".to_string())],
            "the pot fills at the clicked block and nothing is placed above it"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The same click without the `useItemOn` table falls through to the
    /// placement path, which is what this server did before this branch.
    ///
    /// **A negative control, and the one that makes the test above mean
    /// something**: without it a fixture in which nothing at all happened
    /// would pass the assertion that no dandelion was placed.
    #[test]
    fn without_the_table_the_same_click_places_the_flower_instead() {
        let (dir, uses, _item_uses, items) = pot_tables("fill-control");
        let world = FakeWorld {
            at: vec![((3, 70, 5), "minecraft:flower_pot".to_string())],
            ..FakeWorld::default()
        };
        let (_, wrote) = play_tables(
            &place((3, 70, 5), 1, 4),
            true,
            &world,
            Some(&uses),
            None,
            Some("minecraft:dandelion"),
            Some(&items),
            None,
            None,
        );
        assert_eq!(
            wrote,
            vec![((3, 71, 5), "minecraft:dandelion[placed=true]".to_string())],
            "the old behaviour: the block never gets first refusal from the item"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// **The emptying half, and the packet order that is the whole of it.**
    ///
    /// Vanilla's own recording of `790`, actor stream, 2026-08-12:
    ///
    /// ```text
    ///   block_update [1,0,0] flower_pot     the resync of the clicked block
    ///   block_update [1,1,0] air            the resync of the relative one
    ///   block_update [1,0,0] flower_pot     the tick's broadcast
    ///   container_set_slot window 0 slot 38 the flower handed back
    ///   block_changed_ack
    /// ```
    ///
    /// The slot is **after** the tick's block broadcast and **before** the
    /// acknowledgement, because `Inventory.add` sends nothing itself and the
    /// slot goes out from `ServerPlayer.doTick`'s `broadcastChanges`. A server
    /// that sent it from the handler would put it second. That is what
    /// `crate::players::Connection::pending_menu` exists for and this is its
    /// only fast witness — the corpus is a fifty-minute instrument.
    ///
    /// The slot here is 37 rather than 38 because this fixture's inventory
    /// holds only the stick in hand, so `getFreeSlot` answers `Inventory` index
    /// 1. In the corpus the actor has played 92 scenarios first.
    #[test]
    fn emptying_a_pot_hands_the_flower_back_after_the_block_update() {
        let (dir, uses, item_uses, items) = pot_tables("empty");
        let world = FakeWorld {
            at: vec![((3, 70, 5), "minecraft:potted_dandelion".to_string())],
            ..FakeWorld::default()
        };
        let (sent, wrote) = play_tables(
            &place((3, 70, 5), 1, 4),
            true,
            &world,
            Some(&uses),
            None,
            Some("minecraft:stick"),
            Some(&items),
            None,
            Some(&item_uses),
        );
        assert_eq!(
            wrote,
            vec![((3, 70, 5), "minecraft:flower_pot".to_string())],
            "a stick is not pottable, so the click reaches useWithoutItem"
        );
        assert_eq!(
            sent.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec![
                ids::play::cb::BLOCK_UPDATE,
                ids::play::cb::BLOCK_UPDATE,
                ids::play::cb::BLOCK_UPDATE,
                ids::play::cb::CONTAINER_SET_SLOT,
                ids::play::cb::BLOCK_CHANGED_ACK
            ],
            "two resyncs, the tick's broadcast, the slot, then the ack"
        );
        // container id 0 (`InventoryMenu`), state id 1, slot 37 big-endian.
        assert_eq!(&sent[3].1[..4], &[0, 1, 0, 37]);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A pottable item on a pot that is already full is `CONSUME` with no
    /// write: the pot keeps what it had, nothing is placed, and no flower is
    /// handed back. It is the branch a name-based guess gets wrong, and the
    /// one that separates "consumed" from "consumed and wrote".
    #[test]
    fn a_second_flower_on_a_full_pot_changes_nothing() {
        let (dir, uses, item_uses, items) = pot_tables("full");
        let world = FakeWorld {
            at: vec![((3, 70, 5), "minecraft:potted_dandelion".to_string())],
            ..FakeWorld::default()
        };
        let (sent, wrote) = play_tables(
            &place((3, 70, 5), 1, 4),
            true,
            &world,
            Some(&uses),
            None,
            Some("minecraft:dandelion"),
            Some(&items),
            None,
            Some(&item_uses),
        );
        assert!(
            wrote.is_empty(),
            "the pot keeps its flower and nothing is placed"
        );
        assert_eq!(
            sent.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec![
                ids::play::cb::BLOCK_UPDATE,
                ids::play::cb::BLOCK_UPDATE,
                ids::play::cb::BLOCK_CHANGED_ACK
            ],
            "the two resyncs and the ack, and no slot"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// **The order `ServerPlayer.openMenu` puts on the wire, and the counter
    /// that makes the four menu checks one item.**
    ///
    /// Vanilla's own recording of `090-chest-opens-a-screen`, actor stream,
    /// 2026-08-04:
    ///
    /// ```text
    ///   sound, open_screen, container_set_content,
    ///   block_update, block_update, block_event, block_changed_ack
    /// ```
    ///
    /// The two screen packets come from inside the block's `useWithoutItem`
    /// and therefore precede the two resyncs at the foot of
    /// `handleUseItemOn`; the `block_event` is a tick-boundary broadcast and
    /// comes after them. This asserts the part a connection thread produces —
    /// the lid is `crate::containers`' and is tested there.
    ///
    /// It exists because the corpus takes fifty minutes and two of the four
    /// menu checks are declared unstable in vanilla, so the wire order had no
    /// fast signal at all.
    #[test]
    fn opening_a_menu_sends_the_screen_then_its_contents_then_the_resyncs() {
        let (dir, uses, ids) = menu_tables("order");
        let world = FakeWorld {
            chests: vec![(3, 70, -5)],
            ..FakeWorld::default()
        };
        let (sent, wrote) = play_with(
            &place((3, 70, -5), 1, 4),
            true,
            &world,
            Some(&uses),
            Some(&ids),
        );
        assert!(wrote.is_empty(), "a chest click places nothing");
        assert_eq!(
            sent.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec![
                ids::play::cb::SOUND,
                ids::play::cb::OPEN_SCREEN,
                ids::play::cb::CONTAINER_SET_CONTENT,
                ids::play::cb::BLOCK_UPDATE,
                ids::play::cb::BLOCK_UPDATE,
                ids::play::cb::BLOCK_CHANGED_ACK
            ],
            "the open sound, screen, contents, the two resyncs, then the ack"
        );
        // **The sound is first and it is sent to the opener.**
        // `ContainerOpenersCounter.incrementOpeners` runs `onOpen` inside the
        // `openCount++ == 0` guard, before anything the menu does, and
        // `ChestBlockEntity`'s override passes `null` as the excluded player —
        // so the player who opened the chest hears it. Vanilla's own recording
        // of `090-chest-opens-a-screen` has `sound` at the same millisecond as
        // `open_screen` and ahead of it.
        //
        // The holder varint is `registry id + 1` = 350, which is two bytes,
        // then the source ordinal 4 (`BLOCKS`).
        assert_eq!(
            &sent[0].1[..3],
            &[0xDE, 0x02, 0x04],
            "sound 349 + 1, BLOCKS"
        );
        // window 1 and menu 2 (`generic_9x3`), then window 1 and state 1.
        assert_eq!(&sent[1].1[..2], &[1, 2], "container id 1, menu type 2");
        assert_eq!(&sent[2].1[..2], &[1, 1], "container id 1, state id 1");
        // 63 slots, all empty, then an empty carried stack: the count is a
        // varint 63 and every stack is a single zero byte.
        assert_eq!(sent[2].1[2], 63, "ChestMenu.threeRows has 63 slots");
        assert_eq!(sent[2].1.len(), 2 + 1 + 63 + 1, "one byte per empty stack");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The counter does not restart per menu, which is the whole reason the
    /// four menu checks cannot be closed separately.
    #[test]
    fn a_second_menu_on_one_connection_gets_the_next_window_id() {
        let (dir, uses, ids) = menu_tables("counter");
        let world = FakeWorld {
            chests: vec![(3, 70, -5)],
            ..FakeWorld::default()
        };
        let mut input = place((3, 70, -5), 1, 4);
        input.extend(place((3, 70, -5), 1, 5));
        let (sent, _) = play_with(&input, true, &world, Some(&uses), Some(&ids));
        let screens: Vec<&Vec<u8>> = sent
            .iter()
            .filter(|(id, _)| *id == ids::play::cb::OPEN_SCREEN)
            .map(|(_, b)| b)
            .collect();
        assert_eq!(screens.len(), 2, "two clicks, two screens");
        assert_eq!(screens[0][0], 1, "the first menu is window 1");
        assert_eq!(
            screens[1][0], 2,
            "and the second is window 2 — `containerCounter` is on the player \
             and nothing resets it, so the corpus sees 1, 2, 3, 4 across four \
             separate scenarios"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// `ServerPlayerGameMode.useItemOn` 101-126, all three of its cases.
    ///
    /// The gate is a conjunction and the jump it guards skips the *whole*
    /// block-interaction section, so there are exactly three answers and this
    /// asserts each at its own assertion:
    ///
    /// ```text
    ///   sneaking + something held  ->  no menu, the block is placed   (609)
    ///   not sneaking + something   ->  the menu opens                 (560)
    ///   sneaking + empty hands     ->  the menu opens                 (holdingSomething)
    /// ```
    ///
    /// The third is the one that is easy to lose. `isSecondaryUseActive() &&
    /// holdingSomething` is a conjunction at offsets 101-118, not a test of
    /// the sneak bit alone, and a server that dropped the second conjunct
    /// would still pass the first two lines above. It is also the line that
    /// says why `crate::useblock`'s "sneaking with both hands empty still
    /// runs the interaction" is a statement about *this* branch rather than
    /// about the block's own.
    #[test]
    fn sneaking_with_something_held_skips_the_block_and_places_instead() {
        let (dir, uses, ids) = menu_tables("sneak");
        // **A fresh world per case.** `FakeWorld.wrote` accumulates and
        // `play_tables` hands back the whole of it, so a shared fixture
        // would report the first case's placement as the second case's — a
        // check reading the neighbouring case's evidence, which is the
        // failure `553` was briefly killed by in the corpus.
        let chest = || FakeWorld {
            chests: vec![(3, 70, -5)],
            ..FakeWorld::default()
        };
        let opened = |sent: &Sent| sent.iter().any(|(id, _)| *id == ids::play::cb::OPEN_SCREEN);

        // 609: sneak, then click the chest holding a placeable block.
        //
        // **`items` and `recipe_unlocks` are `None` in all four cases below
        // and that is the whole of what this fixture wants.** The hand is
        // filled by `held`, which writes the hotbar slot directly and needs
        // no item table; passing `items` as well would change nothing here
        // — no `hold` frame is on the wire — and passing `recipe_unlocks`
        // would add `recipe_book_add` packets to a `Sent` that four
        // assertions scan for `OPEN_SCREEN` and `CONTAINER_SET_CONTENT`.
        // The recipe-book arm has its own fixtures further down; this one is
        // the `useItemOn` gate and nothing else.
        let mut input = sneak(true);
        input.extend(place((3, 70, -5), 1, 4));
        let (sent, wrote) = play_tables(
            &input,
            true,
            &chest(),
            Some(&uses),
            Some(&ids),
            Some("minecraft:stone"),
            None, // items: the hand is filled by `held`, not by a `hold` frame
            None, // recipe_unlocks: not this fixture's channel
            None, // item_uses: no block here has an item-aware rule
        );
        assert!(
            !opened(&sent),
            "a sneaking click never reaches the chest's own useWithoutItem, so \
             no menu — the jump at 126 is above the call, not inside it"
        );
        assert!(
            !sent
                .iter()
                .any(|(id, _)| *id == ids::play::cb::CONTAINER_SET_CONTENT),
            "and nothing to put in a menu that was not opened"
        );
        assert_eq!(
            wrote,
            vec![((3, 71, -5), "minecraft:stone[placed=true]".to_string())],
            "the placement runs instead, against the clicked face — the \
             positive content of 609, and what stops this passing on a server \
             that had simply stopped answering clicks"
        );

        // 553's half of the pair, on a chest: the same click without the
        // sneak still opens.
        let (sent, wrote) = play_tables(
            &place((3, 70, -5), 1, 4),
            true,
            &chest(),
            Some(&uses),
            Some(&ids),
            Some("minecraft:stone"),
            None, // items
            None, // recipe_unlocks
            None, // item_uses: no block here has an item-aware rule
        );
        assert!(
            opened(&sent),
            "not sneaking: the block gets first refusal and the chest takes it"
        );
        assert!(wrote.is_empty(), "and the stone stays in hand");

        // The conjunct. Sneaking with nothing held is not a secondary use.
        let mut input = sneak(true);
        input.extend(place((3, 70, -5), 1, 4));
        let (sent, _) = play_tables(
            &input,
            true,
            &chest(),
            Some(&uses),
            Some(&ids),
            None, // held: empty hands — the conjunct under test
            None, // items
            None, // recipe_unlocks
            None, // item_uses: no block here has an item-aware rule
        );
        assert!(
            opened(&sent),
            "sneaking with empty hands still runs the interaction — \
             holdingSomething is the second conjunct at offsets 108-110"
        );

        // And the flag is a state that survives its packet: releasing it puts
        // the click back on the block. Without this a server could pass the
        // first case by treating any input packet as a secondary use.
        let mut input = sneak(true);
        input.extend(sneak(false));
        input.extend(place((3, 70, -5), 1, 4));
        let (sent, _) = play_tables(
            &input,
            true,
            &chest(),
            Some(&uses),
            Some(&ids),
            Some("minecraft:stone"),
            None, // items
            None, // recipe_unlocks
            None, // item_uses: no block here has an item-aware rule
        );
        assert!(
            opened(&sent),
            "the released key clears the flag — handlePlayerInput stores what \
             it is sent, so `sneak = false` is a real packet and not a no-op"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A crafting table, its `use_without_item` row and the two protocol ids
    /// opening one needs. Separate from [`menu_tables`] because that fixture's
    /// table names a chest and this one must not grow a second block into a
    /// table five other tests read.
    fn crafting_tables(
        tag: &str,
    ) -> (
        std::path::PathBuf,
        crate::useblock::UseTable,
        crate::protocol_ids::ProtocolIds,
    ) {
        let dir = std::env::temp_dir().join(format!("rustmc-craft-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("tmp");
        let uses_path = dir.join("use_without_item.json");
        std::fs::write(
            &uses_path,
            r#"{"table":[{"block":"minecraft:crafting_table","states":[
                 {"from":"minecraft:crafting_table",
                  "result":"Success","consumes":true,"schedulesTick":false,"writes":[]}]}]}"#,
        )
        .expect("write");
        let ids_path = dir.join("protocol_ids.json");
        std::fs::write(
            &ids_path,
            r#"{"menus":{"minecraft:crafting":12},
                "blocks":{"minecraft:crafting_table":5310},
                "block_entity_types":{},
                "sounds":{},
                "attributes":{}}"#,
        )
        .expect("write");
        (
            dir,
            crate::useblock::UseTable::load(&uses_path).expect("use table"),
            crate::protocol_ids::ProtocolIds::load(&ids_path).expect("protocol ids"),
        )
    }

    /// `ServerboundContainerClickPacket`, cut down to the shape
    /// `oracle-runner`'s `container_click` verb sends: an empty changed-slots
    /// map and an empty carried stack.
    pub(super) fn click(container_id: i32, state_id: i32, slot: i16) -> Vec<u8> {
        let mut body = Vec::new();
        write_varint(&mut body, container_id).expect("buf");
        write_varint(&mut body, state_id).expect("buf");
        body.extend_from_slice(&slot.to_be_bytes());
        wire::write_u8(&mut body, 0).expect("buf"); // button: left
        write_varint(&mut body, 0).expect("buf"); // PICKUP
        write_varint(&mut body, 0).expect("buf"); // no hashed slots
        wire::write_bool(&mut body, false).expect("buf"); // carried: empty
        let mut frame = Vec::new();
        frame::write_frame(&mut frame, None, ids::play::sb::CONTAINER_CLICK, &body).expect("buf");
        frame
    }

    /// `ServerboundContainerClosePacket` — a container id and nothing else.
    pub(super) fn close(container_id: i32) -> Vec<u8> {
        let mut body = Vec::new();
        write_varint(&mut body, container_id).expect("buf");
        let mut frame = Vec::new();
        frame::write_frame(&mut frame, None, ids::play::sb::CONTAINER_CLOSE, &body).expect("buf");
        frame
    }

    /// **`611-closing-a-crafting-table-returns-the-grid-contents`, end to
    /// end, through the real packet loop.**
    ///
    /// The scenario in miniature: put a log in the hotbar, open a crafting
    /// table, pick the log off menu slot 37 and drop it into grid slot 1,
    /// then close. Vanilla hands it back, and the observable is
    /// `container_set_slot` on **window 0** at slot 36 — the hotbar slot the
    /// log can be seen leaving from.
    ///
    /// What would make this fail: a server that treats close as a bare
    /// acknowledgement and clears its menu state without walking the grid.
    /// That is precisely what this server did until this test existed — the
    /// serverbound `CONTAINER_CLOSE` had no dispatch arm at all, so the
    /// packet was read off the wire and discarded, and `close_open_menu` was
    /// an `Option::take` and an openers decrement.
    #[test]
    fn closing_a_crafting_table_hands_the_grid_back() {
        let (dir, uses, ids) = crafting_tables("close611");
        let world = FakeWorld {
            at: vec![((3, 70, -5), "minecraft:crafting_table".to_string())],
            ..FakeWorld::default()
        };
        // The click's state id has to be the one the menu is on, or
        // `doClick` answers with a full-state resync instead of a pickup.
        // Opening sends the content packet at state 1.
        let mut input = place((3, 70, -5), 1, 4);
        input.extend(click(1, 1, 37)); // pick the log off the hotbar
        input.extend(click(1, 2, 1)); // put it in the grid
        input.extend(close(1));
        let (sent, _) = play_tables(
            &input,
            true,
            &world,
            Some(&uses),
            Some(&ids),
            Some("minecraft:oak_log"),
            None, // items: the hand is filled by `held`
            None, // recipe_unlocks
            None, // item_uses: no block here has an item-aware rule
        );

        let inventory: Vec<&(i32, Vec<u8>)> = sent
            .iter()
            .filter(|(id, _)| *id == ids::play::cb::SET_PLAYER_INVENTORY)
            .collect();
        assert_eq!(
            inventory.len(),
            1,
            "placeItemBackInInventory sends one per slot it writes, and it \
             wrote one: the hotbar slot the log came out of"
        );

        // The window-0 correction. `container_set_slot`'s body is
        // `(containerId, stateId, slot)`, so the first varint being 0 is the
        // whole claim: this is the player's own menu and not the crafting
        // table's.
        let window0: Vec<&(i32, Vec<u8>)> = sent
            .iter()
            .filter(|(id, body)| {
                *id == ids::play::cb::CONTAINER_SET_SLOT && body.first() == Some(&0)
            })
            .collect();
        assert_eq!(
            window0.len(),
            1,
            "one slot changed against what the closing menu had told the \
             client, so one correction goes out"
        );
        assert_eq!(
            window0[0].1,
            crate::menu::container_set_slot_body(
                0,
                1,
                36,
                crate::menu::to_wire(&crate::item::ItemStack::new("minecraft:oak_log", 1), None)
            )
            .expect("buf"),
            "window 0, the first state id this menu has ever sent, slot 36 — \
             which is Inventory 0 in InventoryMenu's numbering"
        );

        // Order: the inventory write is sent from inside
        // `placeItemBackInInventory`'s loop and the correction from the sweep
        // after it, so `set_player_inventory` precedes `container_set_slot`.
        // Vanilla's own recording of `611` has them one millisecond apart in
        // that order.
        //
        // **The window-0 packet has to be found by its body, not by its id.**
        // The two clicks before the close each answered with a
        // `container_set_slot` of their own on window 1, so a `position` on
        // the id alone finds the first of *those* and the assertion holds
        // whatever the close does — a check that cannot fail, and this one
        // was, until it was run and said so.
        let wrote_back = sent
            .iter()
            .position(|(id, _)| *id == ids::play::cb::SET_PLAYER_INVENTORY)
            .expect("the handback was sent");
        let corrected = sent
            .iter()
            .position(|(id, body)| {
                *id == ids::play::cb::CONTAINER_SET_SLOT && body.first() == Some(&0)
            })
            .expect("the correction was sent");
        assert!(
            wrote_back < corrected,
            "the handback is written before the sweep reports it"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The other half of the same walk, and the one no gameplay scenario can
    /// reach: **the cursor is handed back too**, before the grid.
    ///
    /// `AbstractContainerMenu.removed` returns `carried` at offsets 8-32 and
    /// `CraftingMenu.removed` only then runs `clearContainer`. A close with a
    /// stack still on the cursor therefore loses nothing — and closing
    /// mid-click is exactly what a real client does when the player presses
    /// Escape.
    #[test]
    fn closing_with_a_stack_on_the_cursor_hands_that_back_too() {
        let (dir, uses, ids) = crafting_tables("cursor611");
        let world = FakeWorld {
            at: vec![((3, 70, -5), "minecraft:crafting_table".to_string())],
            ..FakeWorld::default()
        };
        // Pick the log up and stop there: it is on the cursor, not in the
        // grid, when the close arrives.
        let mut input = place((3, 70, -5), 1, 4);
        input.extend(click(1, 1, 37));
        input.extend(close(1));
        let (sent, _) = play_tables(
            &input,
            true,
            &world,
            Some(&uses),
            Some(&ids),
            Some("minecraft:oak_log"),
            None,
            None,
            None, // item_uses: no block here has an item-aware rule
        );
        assert_eq!(
            sent.iter()
                .filter(|(id, _)| *id == ids::play::cb::SET_PLAYER_INVENTORY)
                .count(),
            1,
            "the cursor's stack went back into the inventory"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Doors and trapdoors sound with the table's event and the shared
    /// nine-tenths pitch, and a block that is neither still does not.
    ///
    /// # What this is really guarding
    ///
    /// The whole of the extract's unusable half is doors (1,280), trapdoors
    /// (1,280) and fence gates (384) — measured by `ci/sound-coverage.py`,
    /// which refuses a fourth family precisely because a fourth family would
    /// reach neither this arm nor the table arm and would be **silently
    /// silent**. This is the same fact from the other side: it asserts the
    /// arm fires for the two classes it claims, and does not fire for a class
    /// that merely happens to sound.
    ///
    /// The pitch is a fresh draw and cannot equal vanilla's, so it is checked
    /// for its *range* rather than its value — `nextFloat() * 0.1F + 0.9F` is
    /// `[0.9, 1.0)`. That is the strongest available assertion and it is
    /// weaker than the ones above it, which is worth knowing rather than
    /// papering over: a bug that replaced this pitch with a different draw
    /// inside the same range would pass here and pass the corpus too, because
    /// `oracle-runner` drops `sound.pitch`.
    #[test]
    fn a_door_sounds_the_tables_event_at_the_shared_nine_tenths_pitch() {
        let cp = std::env::temp_dir().join(format!("rustmc-doorcls-{}.json", std::process::id()));
        std::fs::write(
            &cp,
            r#"{"classes": {"minecraft:oak_door": "DoorBlock",
                            "minecraft:copper_door": "WeatheringCopperDoorBlock",
                            "minecraft:oak_trapdoor": "TrapDoorBlock",
                            "minecraft:note_block": "NoteBlock"},
                "superclasses": {"DoorBlock": ["Block"], "TrapDoorBlock": ["Block"],
                                 "WeatheringCopperDoorBlock": ["DoorBlock", "Block"],
                                 "NoteBlock": ["Block"]},
                "table": []}"#,
        )
        .expect("write");
        let classes = crate::placedby::PlacedByTable::load(&cp).expect("load");
        std::fs::remove_file(&cp).ok();

        let tp = std::env::temp_dir().join(format!("rustmc-doortbl-{}.json", std::process::id()));
        std::fs::write(
            &tp,
            r#"{"table": [
              {"block": "minecraft:oak_door", "random": false, "faceSensitive": false,
               "states": [
                {"from": "minecraft:oak_door[open=true]", "result": "Success",
                 "consumes": true, "schedulesTick": false, "sound": true,
                 "soundEvent": "minecraft:block.wooden_door.close", "soundSource": "block",
                 "soundVolume": 1.0, "soundCount": 1, "soundPitchDrawn": true,
                 "soundExcludesActor": true,
                 "writes": [{"at": [0, 0, 0], "state": "minecraft:oak_door[open=false]",
                             "flags": 10}]}
               ]},
              {"block": "minecraft:copper_door", "random": false, "faceSensitive": false,
               "states": [
                {"from": "minecraft:copper_door[open=false]", "result": "Success",
                 "consumes": true, "schedulesTick": false, "sound": true,
                 "soundEvent": "minecraft:block.copper_door.open", "soundSource": "block",
                 "soundVolume": 1.0, "soundCount": 1, "soundPitchDrawn": true,
                 "soundExcludesActor": true,
                 "writes": [{"at": [0, 0, 0], "state": "minecraft:copper_door[open=true]",
                             "flags": 10}]}
               ]}
            ]}"#,
        )
        .expect("write");
        let uses = crate::useblock::UseTable::load(&tp).expect("loads");
        std::fs::remove_file(&tp).ok();

        for (state, event) in [
            (
                "minecraft:oak_door[open=true]",
                "minecraft:block.wooden_door.close",
            ),
            // Through a subclass, which is the case a name-suffix rule would
            // have got wrong: `WeatheringCopperDoorBlock` is not `DoorBlock`
            // and `is_a` walks the chain.
            (
                "minecraft:copper_door[open=false]",
                "minecraft:block.copper_door.open",
            ),
        ] {
            let got = use_sound(Some(&classes), Some(&uses), 7, state, state, (1, 0, 0))
                .unwrap_or_else(|| panic!("{state} must sound off the table"));
            assert_eq!(got.event, event);
            assert_eq!(got.source, crate::sounds::SOURCE_BLOCKS);
            assert!((got.volume - 1.0).abs() < 1e-6);
            assert_eq!(
                got.except,
                Some(7),
                "the opener does not hear their own door"
            );
            assert!(
                (0.9..1.0).contains(&got.pitch),
                "nextFloat()*0.1+0.9 is [0.9, 1.0), got {}",
                got.pitch
            );
        }

        // Without the classes table there is no way to know it is a door, and
        // guessing from the name is exactly what this arm refuses to do.
        assert_eq!(
            use_sound(
                None,
                Some(&uses),
                7,
                "minecraft:oak_door[open=true]",
                "minecraft:oak_door[open=true]",
                (1, 0, 0)
            ),
            None,
        );
    }

    /// The table arm emits what the extract says, and stays silent when the
    /// extract cannot say.
    ///
    /// # This is the control the corpus does not have
    ///
    /// The fallback's failure mode is not "no sound" — it is **a sound the
    /// table could not justify**, and nothing downstream would catch one.
    /// `oracle-runner` drops `sound.pitch` (`trace.rs`, `let _pitch`), so a
    /// wrong pitch is invisible to every scenario in the corpus in both
    /// directions; and only `430` observes a use-path sound at all. So the
    /// two assertions below are the whole of the evidence that this arm is
    /// wired to the extract rather than to a guess.
    ///
    /// Both rows are copied from the shipped `use_without_item.json`, and
    /// the note block's two surprises are why a fixture written by hand
    /// would have been worse than useless:
    ///
    /// * its source is **`record`, ordinal 2**, not `block`/4. Every other
    ///   sound this function emits is a `BLOCKS` sound and a hand-port would
    ///   have reached for `SOURCE_BLOCKS` without looking.
    /// * its pitch is keyed on the note **being written**, not the one
    ///   clicked: a click on `note=0` sounds `2^((1-12)/12) = 0.5297316`,
    ///   because vanilla cycles the note and then plays it. The row is the
    ///   answer for that click, so keying `sound_for` on the clicked state
    ///   is right — but it is right for a reason worth writing down.
    #[test]
    fn the_table_arm_emits_the_extracts_sound_and_only_when_it_can() {
        let dir = std::env::temp_dir().join(format!("rustmc-usesound-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("tmp");
        let path = dir.join("use_without_item.json");
        std::fs::write(
            &path,
            r#"{"table": [
              {"block": "minecraft:note_block", "random": false, "faceSensitive": false,
               "states": [
                {"from": "minecraft:note_block[instrument=harp,note=0,powered=true]",
                 "result": "Success", "consumes": true, "schedulesTick": false, "sound": true,
                 "soundEvent": "minecraft:block.note_block.harp", "soundSource": "record",
                 "soundVolume": 3.0, "soundCount": 1, "soundPitchDrawn": false,
                 "soundExcludesActor": false, "soundPitch": 0.5297316,
                 "writes": [{"at": [0, 0, 0],
                   "state": "minecraft:note_block[instrument=harp,note=1,powered=true]",
                   "flags": 3}]}
               ]},
              {"block": "minecraft:oak_door", "random": false, "faceSensitive": false,
               "states": [
                {"from": "minecraft:oak_door[facing=north,half=upper,hinge=left,open=true,powered=true]",
                 "result": "Success", "consumes": true, "schedulesTick": false, "sound": true,
                 "soundEvent": "minecraft:block.wooden_door.close", "soundSource": "block",
                 "soundVolume": 1.0, "soundCount": 1, "soundPitchDrawn": true,
                 "soundExcludesActor": true,
                 "writes": [{"at": [0, 0, 0],
                   "state": "minecraft:oak_door[facing=north,half=upper,hinge=left,open=false,powered=true]",
                   "flags": 10}]}
               ]}
            ]}"#,
        )
        .expect("write");
        let uses = crate::useblock::UseTable::load(&path).expect("loads");
        std::fs::remove_file(&path).ok();

        let note = "minecraft:note_block[instrument=harp,note=0,powered=true]";
        let got = use_sound(None, Some(&uses), 7, note, note, (1, 0, 0))
            .expect("the note block sounds off the table");
        assert_eq!(got.event, "minecraft:block.note_block.harp");
        assert_eq!(got.source, 2, "note blocks are a RECORDS sound, not BLOCKS");
        assert!((got.volume - 3.0).abs() < 1e-6);
        assert!((got.pitch - 0.529_731_6).abs() < 1e-6);
        assert_eq!(got.except, None, "the clicker hears their own note block");

        // The same click with no table loaded is the pre-2026-08-17 server,
        // and it must still be silent — otherwise this arm is emitting from
        // somewhere other than the extract.
        assert_eq!(
            use_sound(None, None, 7, note, note, (1, 0, 0)),
            None,
            "with no table there is nothing to emit from"
        );

        // A drawn pitch is the refusal, not an excuse to invent one.
        let door = "minecraft:oak_door[facing=north,half=upper,hinge=left,open=true,powered=true]";
        assert_eq!(
            use_sound(None, Some(&uses), 7, door, door, (1, 0, 0)),
            None,
            "the door's pitch is a draw, so the table must not emit it"
        );

        // And a state the extract has never heard of stays silent whatever
        // else is in the table.
        assert_eq!(
            use_sound(
                None,
                Some(&uses),
                7,
                "minecraft:stone",
                "minecraft:stone",
                (1, 0, 0)
            ),
            None,
        );
    }

    /// Which blocks `use_sound` answers for, which is the half of it that a
    /// wrong pitch cannot break and a stray arm can.
    ///
    /// `crate::sounds`' own tests pin the constants. This pins the routing:
    /// four families sound and the other 180 blocks with a `sound` boolean
    /// in `use_without_item.json` stay silent. **That is the direction this
    /// function fails in.** Adding a family is a two-line arm and the way to
    /// get it wrong is to make it fire somewhere else — a door playing a
    /// comparator's click is a worse regression than a comparator playing the
    /// wrong pitch, because no scenario in the corpus would attribute it.
    ///
    /// # The corpus cannot check this and that is why it is here
    ///
    /// Four of the 90 scenarios observe `sound` at all — `420`, `430`, `550`
    /// and `551`. `290` and `330` sound a lever and a button in vanilla and
    /// **do not observe it**; `470` and `480` do not observe it either. So
    /// the named negative controls for a use-sound change are, at corpus
    /// level, blind on the axis they are supposed to control, and only `430`
    /// — the note block — could catch this function starting to emit
    /// somewhere new. The silence below is the control that does not exist
    /// out there.
    #[test]
    fn only_four_families_sound_and_the_rest_stay_silent() {
        let comparator = |from: &str, to: &str| {
            use_sound(
                None,
                None,
                7,
                &format!("minecraft:comparator[facing=north,mode={from},powered=false]"),
                &format!("minecraft:comparator[facing=north,mode={to},powered=false]"),
                (1, 0, 0),
            )
        };
        let to_subtract = comparator("compare", "subtract").expect("a comparator sounds");
        assert_eq!(to_subtract.event, "minecraft:block.comparator.click");
        assert!(
            (to_subtract.pitch - 0.55).abs() < 1e-6,
            "the pitch is read off the state being written, which is the \
             cycled one vanilla reads it from"
        );
        assert_eq!(to_subtract.except, Some(7), "the clicker is excluded");
        assert!(
            (comparator("subtract", "compare")
                .expect("still sounds")
                .pitch
                - 0.5)
                .abs()
                < 1e-6,
            "and back the other way it is 0.5f — MODE, not POWERED"
        );

        // 290's block, unchanged. The comparator arm sits in front of the
        // button's and behind the lever's, so this is the assertion that
        // catches an arm added in the wrong place.
        let lever = use_sound(
            None,
            None,
            7,
            "minecraft:lever[face=floor,facing=north,powered=false]",
            "minecraft:lever[face=floor,facing=north,powered=true]",
            (1, 0, 0),
        )
        .expect("a lever still sounds");
        assert_eq!(lever.event, "minecraft:block.lever.click");
        assert!((lever.pitch - 0.6).abs() < 1e-6, "the lever kept its 0.6f");
        assert_eq!(lever.except, None, "and the flipper still hears it");

        // The silence. Every one of these is in `use_without_item.json` with
        // `sound: true`, and every one of them must produce nothing *here*.
        //
        // `note_block` is still on this list and its meaning has changed: it
        // does now make a sound, and not through this function. Vanilla's is
        // `NoteBlock.playNote` -> `Level.blockEvent` -> `triggerEvent`, so it
        // comes out of `crate::containers::run_block_events` at the tick
        // boundary and lands *behind* the block update. An arm here would put
        // it ahead of one, which `430` reads as a divergence in the order
        // rather than in the sound. See `crate::noteblock`.
        for block in [
            "minecraft:note_block[instrument=harp,note=0,powered=false]",
            "minecraft:furnace[facing=north,lit=false]",
            "minecraft:crafting_table",
            "minecraft:oak_door[facing=north,half=lower,hinge=left,open=false,powered=false]",
            "minecraft:oak_trapdoor[facing=north,half=bottom,open=false,powered=false,waterlogged=false]",
            "minecraft:chest[facing=north,type=single,waterlogged=false]",
            // Not a comparator, and the arm is an equality test rather than
            // a prefix or a `contains` — which is how it would have caught
            // this one.
            "minecraft:comparator_but_not_really",
        ] {
            assert_eq!(
                use_sound(None, None, 7, block, block, (1, 0, 0)),
                None,
                "{block} has a `sound` boolean in the extract and no \
                 transcribed rule, so it stays silent"
            );
        }
    }

    /// The fourth family, and the two ways its arm could be wrong: firing on
    /// the wrong class, or reading `OPEN` off the state that was clicked
    /// rather than the one that was written.
    ///
    /// `is_a`, not a name suffix — `crate::useblock::FENCE_GATE_BLOCK` is the
    /// class column of `set_placed_by.json`, so a hypothetical block called
    /// `minecraft:garden_fence_gate` that is not a `FenceGateBlock` gets no
    /// sound, exactly as `comparator_but_not_really` gets none above.
    #[test]
    fn a_fence_gate_sounds_open_or_close_and_excludes_the_clicker() {
        let p = std::env::temp_dir().join(format!("rustmc-fgsound-{}.json", std::process::id()));
        std::fs::write(
            &p,
            r#"{"classes": {"minecraft:oak_fence_gate": "FenceGateBlock",
                            "minecraft:crimson_fence_gate": "FenceGateBlock",
                            "minecraft:garden_fence_gate": "Block",
                            "minecraft:oak_door": "DoorBlock"},
                "superclasses": {"FenceGateBlock": ["Block"], "DoorBlock": ["Block"]},
                "table": []}"#,
        )
        .expect("write");
        let classes = crate::placedby::PlacedByTable::load(&p).expect("load");
        std::fs::remove_file(&p).ok();

        let gate = |block: &str, from: &str, to: &str| {
            use_sound(
                Some(&classes),
                None,
                7,
                &format!("{block}[facing=south,in_wall=false,open={from},powered=false]"),
                &format!("{block}[facing=south,in_wall=false,open={to},powered=false]"),
                (1, 0, 0),
            )
            .expect("a fence gate sounds")
        };

        let opened = gate("minecraft:oak_fence_gate", "false", "true");
        assert_eq!(opened.event, "minecraft:block.fence_gate.open");
        assert_eq!(opened.volume, 1.0);
        assert_eq!(
            opened.except,
            Some(7),
            "`playSound(player, …)` excludes the clicker, so this is a \
             bystander's observation only"
        );
        assert!(
            (0.9..1.0).contains(&opened.pitch),
            "nextFloat() * 0.1f + 0.9f, in shape rather than pinned"
        );
        assert_eq!(
            gate("minecraft:oak_fence_gate", "true", "false").event,
            "minecraft:block.fence_gate.close",
            "`OPEN` is read off the written state, not the clicked one"
        );
        assert_eq!(
            gate("minecraft:crimson_fence_gate", "false", "true").event,
            "minecraft:block.nether_wood_fence_gate.open",
            "crimson and warped share one WoodType sound pair"
        );
        for block in [
            "minecraft:garden_fence_gate[facing=south,in_wall=false,open=false,powered=false]",
            "minecraft:oak_door[facing=north,half=lower,hinge=left,open=false,powered=false]",
        ] {
            assert_eq!(
                use_sound(Some(&classes), None, 7, block, block, (1, 0, 0)),
                None,
                "{block} is not a FenceGateBlock and must stay silent"
            );
        }
    }

    /// One dig packet through a world with nothing solid in it.
    ///
    /// The position is beside the fixture player's feet — (8, 64, 8) —
    /// because `handleBlockBreakAction` refuses anything outside
    /// `isWithinBlockInteractionRange` before it looks at the action, and
    /// sends nothing when it does. The (3, 70, -5) these tests dug before
    /// the range check existed is 13 blocks away.
    fn run(action: i32, creative: bool) -> (Sent, Wrote) {
        play(&dig(action, NEAR, 9), creative, &FakeWorld::default())
    }

    const NEAR: BlockPos = (9, 64, 9);

    /// A creative dig sets the block to air, broadcasts it to everyone
    /// including the digger, and acknowledges the sequence.
    ///
    /// This assertion has now been written three ways and it is worth saying
    /// which two were wrong and why, because both were wrong for the same
    /// reason — a client that was not being told things, mistaken for a
    /// server that was not saying them.
    ///
    /// The second version asserted the ack **alone**, on a measurement that
    /// showed vanilla sending the digging client nothing else. That
    /// measurement was taken by a harness that never acknowledged a chunk
    /// batch, so vanilla had stopped sending it chunks after the first nine
    /// and `ChunkMap.isChunkTracked` was false for every chunk it dug in.
    /// `ChunkHolder.broadcast` is `list.forEach(send)` with no exclusion of
    /// anyone; the digger was missing from the *list*, not skipped by a rule.
    ///
    /// Re-measured 2026-08-03 with a conforming client: a placement puts
    /// four things on the actor's socket — resync of the clicked block,
    /// resync of the block placed into, the broadcast, then the ack.
    #[test]
    fn a_creative_dig_tells_everyone_including_the_digger_and_acks() {
        let (sent, wrote) = run(0, true);
        assert_eq!(wrote, vec![(NEAR, "minecraft:air".to_string())]);
        assert_eq!(
            sent.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec![
                ids::play::cb::BLOCK_UPDATE,
                ids::play::cb::BLOCK_CHANGED_ACK
            ],
            "the broadcast reaches the digger too, and the ack follows it"
        );
        assert_eq!(sent[1].1, block_changed_ack_body(9).expect("buf"));
    }

    /// A creative client sends `START_DESTROY_BLOCK` and never a `STOP`, and
    /// the START destroys. A survival START on anything with a destroy time
    /// only starts `ServerPlayerGameMode`'s clock, and a survival STOP that
    /// names a position no START named — `destroyPos` is `BlockPos.ZERO` on a
    /// fresh player — is ignored outright. This test used to assert that a
    /// survival STOP on its own digs: that was the instant survival mining
    /// `004` recorded vanilla refusing.
    #[test]
    fn survival_needs_a_start_and_the_servers_own_clock() {
        assert!(run(0, false).1.is_empty(), "survival START digs nothing");
        assert!(
            run(2, false).1.is_empty(),
            "a STOP no START named digs nothing"
        );
        assert_eq!(run(0, true).1.len(), 1, "creative START digs");
        assert!(
            run(2, true).1.is_empty(),
            "creative sends no STOP to act on"
        );
        // START then STOP on the same air block in one breath: the air arm
        // of START shows stage 10 and never destroys, and STOP on air is
        // nothing.
        let mut both = dig(0, NEAR, 1);
        both.extend(dig(2, NEAR, 2));
        assert!(play(&both, false, &FakeWorld::default()).1.is_empty());
    }

    /// A dig outside `isWithinBlockInteractionRange(pos, 1.0)` is dropped
    /// before the action is read — no destroy — and it is still
    /// acknowledged, because the ack is `handlePlayerAction`'s and comes
    /// after `handleBlockBreakAction` returns whatever it did.
    #[test]
    fn a_dig_out_of_reach_is_refused_but_acknowledged() {
        let (sent, wrote) = play(&dig(0, (3, 70, -5), 4), true, &FakeWorld::default());
        assert!(wrote.is_empty(), "13 blocks away is out of reach");
        assert_eq!(
            sent.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec![ids::play::cb::BLOCK_CHANGED_ACK]
        );
    }

    /// `RELEASE_USE_ITEM` (ordinal 5) returns from `handlePlayerAction`
    /// without reaching the ack at the tail of the digging arms.
    #[test]
    fn only_the_digging_actions_are_acknowledged() {
        let (sent, _) = run(5, false);
        assert!(
            sent.iter()
                .all(|(id, _)| *id != ids::play::cb::BLOCK_CHANGED_ACK),
            "no ack for releasing a use"
        );
        let (sent, _) = run(1, false);
        assert!(
            sent.iter()
                .any(|(id, _)| *id == ids::play::cb::BLOCK_CHANGED_ACK),
            "ABORT is a digging arm and is acknowledged"
        );
    }

    /// The ack goes out whether or not anything changed. A refused dig is
    /// exactly what the client's prediction is for: with no ack it keeps its
    /// own guess that the block is gone, and nothing later can dislodge it.
    #[test]
    fn a_refused_dig_is_still_acknowledged() {
        let (sent, wrote) = run(2, true);
        assert!(wrote.is_empty());
        assert_eq!(
            sent.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec![ids::play::cb::BLOCK_CHANGED_ACK]
        );
    }

    /// A creative client putting an item in a hotbar slot, then selecting it.
    ///
    /// `set_creative_mode_slot` numbers slots in the **menu**: 36 is the
    /// first hotbar slot (`InventoryMenu.USE_ROW_SLOT_START`), not 0.
    fn hold(item_id: i32, menu_slot: i16, hotbar_slot: i16) -> Vec<u8> {
        let mut give = Vec::new();
        #[expect(clippy::cast_sign_loss, reason = "a short goes out as its bits")]
        wire::write_u16(&mut give, menu_slot as u16).expect("buf");
        write_varint(&mut give, 1).expect("buf"); // count
        write_varint(&mut give, item_id).expect("buf");
        write_varint(&mut give, 0).expect("buf"); // components added
        write_varint(&mut give, 0).expect("buf"); // components removed

        let mut select = Vec::new();
        #[expect(clippy::cast_sign_loss, reason = "a short goes out as its bits")]
        wire::write_u16(&mut select, hotbar_slot as u16).expect("buf");

        let mut out = Vec::new();
        frame::write_frame(
            &mut out,
            None,
            crate::player::ids::SET_CREATIVE_MODE_SLOT,
            &give,
        )
        .expect("buf");
        frame::write_frame(
            &mut out,
            None,
            crate::player::ids::SET_CARRIED_ITEM,
            &select,
        )
        .expect("buf");
        out
    }

    /// The advancement engine's ONE call site in this loop, and the only
    /// test in the tree that reaches it.
    ///
    /// This is the `recipe_unlocks` lesson applied before it costs anything:
    /// that arm was reachable by no test at all and a fifty-minute corpus was
    /// its sole witness. `data/vanilla/advancements.json` is gitignored and
    /// generated, so the fixture builds a two-advancement table inline
    /// instead — the mechanism under test is the wiring, not the extract.
    ///
    /// Three assertions and a negative control:
    ///
    ///   * a `hold` of the item a criterion names produces exactly one
    ///     `update_advancements`, whose `reset` byte is **false** because the
    ///     first, empty flush already cleared `isFirstPacket`;
    ///   * a `hold` of an item no criterion names produces none, so the first
    ///     result is the predicate matching and not the loop emitting on
    ///     every inventory change;
    ///   * the same fixture with `adv_engine: None` produces none, so the
    ///     packet comes from the engine and not from somewhere else in the
    ///     loop.
    #[test]
    fn holding_an_item_a_criterion_names_sends_one_update_advancements() {
        let table: &'static crate::advancements::Table = Box::leak(Box::new(
            crate::advancements::Table::parse(&serde_json::json!({
                "advancements": {
                    "t:root": {"parent": null, "requirements": [["d"]],
                        "criteria_triggers": {"d": "minecraft:inventory_changed"},
                        "document": {"criteria": {"d": {"trigger": "minecraft:inventory_changed",
                            "conditions": {"items": [{"items": "minecraft:furnace"}]}}}},
                        "display": {"hidden": false}, "experience": 0,
                        "reward_recipes": [], "reward_loot": [],
                        "reward_function": false, "holder_wire": "aabbcc"}
                }
            }))
            .expect("fixture table"),
        ));
        let tree: &'static crate::advancements::Tree =
            Box::leak(Box::new(crate::advancements::Tree::build(table)));
        let engine = crate::advancements::Engine {
            table,
            tree,
            tags: None,
        };
        let items = crate::player::ItemIds::parse(&serde_json::json!({
            "items": {"minecraft:furnace": 1, "minecraft:stone": 2}
        }))
        .expect("item ids");
        let world = FakeWorld::default();

        let bodies = |sent: &Sent| -> Vec<Vec<u8>> {
            sent.iter()
                .filter(|(id, _)| *id == ids::play::cb::UPDATE_ADVANCEMENTS)
                .map(|(_, b)| b.clone())
                .collect()
        };

        let (hit, _) = play_tables_adv(
            &hold(1, 36, 0),
            true,
            &world,
            None,
            None,
            None,
            Some(&items),
            None,
            None,
            Some(engine),
        );
        let got = bodies(&hit);
        assert_eq!(got.len(), 1, "one hold, one packet");
        assert_eq!(
            got[0][0], 0,
            "reset is false — the empty first flush cleared isFirstPacket"
        );
        assert!(
            got[0].windows(3).any(|w| w == [0xaa, 0xbb, 0xcc]),
            "the added list carries the table's captured holder bytes"
        );

        let (miss, _) = play_tables_adv(
            &hold(2, 36, 0),
            true,
            &world,
            None,
            None,
            None,
            Some(&items),
            None,
            None,
            Some(engine),
        );
        assert!(
            bodies(&miss).is_empty(),
            "an item no criterion names sends nothing"
        );

        let (off, _) = play_tables(
            &hold(1, 36, 0),
            true,
            &world,
            None,
            None,
            None,
            Some(&items),
            None,
            None,
        );
        assert!(
            bodies(&off).is_empty(),
            "without the engine the same input sends nothing"
        );
    }

    /// `ServerboundPlayerInputPacket`: the seven movement keys as one byte,
    /// of which only `FLAG_SHIFT` (32) is ever set — the same byte
    /// `tools/oracle-runner`'s `sneak` verb puts on the wire, so a unit test
    /// and a corpus scenario are driving the identical packet.
    fn sneak(on: bool) -> Vec<u8> {
        let mut body = Vec::new();
        wire::write_u8(&mut body, u8::from(on) * 32).expect("buf");
        let mut frame = Vec::new();
        frame::write_frame(&mut frame, None, ids::play::sb::PLAYER_INPUT, &body).expect("buf");
        frame
    }

    /// The two tables `040-stairs-and-furnace-face-apart` runs on, in memory.
    ///
    /// The real ones are generated — `ci/dump-items.sh` and
    /// `ci/dump-recipe-unlocks.sh` — and gitignored, so a fixture that loaded
    /// them from `data/vanilla` would be a test that silently skips on any
    /// tree that has not generated yet. These are the same *shape* with two
    /// items in them, and the entry bytes are deliberately **not** vanilla's:
    /// what is under test here is the loop's decision to send, not the
    /// payload, which `recipe_book::tests` and the corpus both cover.
    fn recipe_tables() -> (crate::player::ItemIds, crate::recipe_book::RecipeUnlocks) {
        let items = crate::player::ItemIds::parse(&serde_json::json!({
            "items": {"minecraft:oak_stairs": 1, "minecraft:furnace": 2}
        }))
        .expect("items parse");
        let unlocks = crate::recipe_book::RecipeUnlocks::parse(&serde_json::json!({
            "by_item": {
                "minecraft:furnace": ["minecraft:smoker"],
                // 040 step 0's control, and it is in the table on purpose:
                // an item that is *absent* would prove only that a lookup
                // missed, not that a lookup can hit and still send nothing.
                "minecraft:oak_stairs": [],
            },
            "displays": {"minecraft:smoker": {"entry_hex": "aabbcc"}},
        }))
        .expect("unlocks parse");
        (items, unlocks)
    }

    fn recipe_book_bodies(sent: &Sent) -> Vec<Vec<u8>> {
        sent.iter()
            .filter(|(id, _)| *id == ids::play::cb::RECIPE_BOOK_ADD)
            .map(|(_, b)| b.clone())
            .collect()
    }

    /// **`ServerRecipeBook.addRecipes` through the connection loop, which no
    /// other test in this tree reaches.**
    ///
    /// `recipe_book::tests` covers [`crate::recipe_book::RecipeUnlocks::hold`]
    /// in isolation; this covers the arm in [`keep_alive_loop`] that decides
    /// to call it at all, and it is one assertion because the three
    /// behaviours are three steps of one connection: `known` is per-player
    /// state, so a second furnace can only be shown to send nothing on the
    /// same session that already sent something.
    ///
    /// Vanilla, offsets 45-56 and 115-137: one packet on the first furnace,
    /// nothing on the second, and — 040's own step 0 — nothing for
    /// `oak_stairs`, which unlocks no recipe.
    ///
    /// **This is a control that can fire, checked rather than assumed.**
    /// Deleting the `known.contains` skip in `RecipeUnlocks::hold` turns the
    /// second assertion red (2 bodies, not 1); returning the whole candidate
    /// list regardless of the item turns the third red.
    #[test]
    fn a_furnace_unlocks_the_smoker_once_and_stairs_unlock_nothing() {
        let (items, unlocks) = recipe_tables();
        let world = FakeWorld::default();

        // Step 0 of 040: `oak_stairs`, item id 1.
        //
        // **`held` is `None` here on purpose and it is the argument this
        // test would be broken by.** The hand is filled by the `hold` frame,
        // which resolves the numeric id through `items` in
        // `crate::player::creative_slot_item` — so `items` is what is under
        // test and the `hold` frame is how it gets exercised. Filling the
        // slot directly with `held` would put the same item in the same hand
        // while never reading the table, and all three assertions below
        // would still pass on a server that had stopped resolving item ids
        // altogether.
        let (stairs, _) = play_tables(
            &hold(1, 36, 0),
            true,
            &world,
            None, // uses: no block is clicked
            None, // protocol_ids: no menu is opened
            None, // held: see above — the hand is filled through `items`
            Some(&items),
            Some(&unlocks),
            None, // item_uses: no block here has an item-aware rule
        );
        assert!(
            recipe_book_bodies(&stairs).is_empty(),
            "040 step 0: oak_stairs unlocks nothing, so nothing goes out"
        );

        // Step 2 of 040: `furnace`, item id 2 — then a second furnace on the
        // same connection, which vanilla's `known` set silences.
        let mut twice = hold(2, 36, 0);
        twice.extend(hold(2, 37, 1));
        let (furnace, _) = play_tables(
            &twice,
            true,
            &world,
            None, // uses
            None, // protocol_ids
            None, // held: the hand is filled through `items`, as above
            Some(&items),
            Some(&unlocks),
            None, // item_uses: no block here has an item-aware rule
        );
        assert_eq!(
            recipe_book_bodies(&furnace),
            // count=1, the captured entry, then `replace = false` —
            // `iconst_0` at offset 133.
            vec![vec![1u8, 0xaa, 0xbb, 0xcc, 0]],
            "one packet for the first furnace, none for the second"
        );
    }

    /// The other half of the control: the same two furnaces with no table
    /// send nothing, so the packet above is caused by the table and not by
    /// holding an item.
    ///
    /// This is the shape every other fixture in this module is in
    /// (`recipe_unlocks: None`), stated as an assertion rather than left as
    /// an absence — an absence that nobody asserts is how a dead code path
    /// reads as a passing negative control.
    #[test]
    fn without_the_table_a_furnace_sends_no_recipe_book_add() {
        let (items, _) = recipe_tables();
        let world = FakeWorld::default();
        let (sent, _) = play_tables(
            &hold(2, 36, 0),
            true,
            &world,
            None, // uses
            None, // protocol_ids
            None, // held: the hand is filled through `items`, as above
            Some(&items),
            None, // recipe_unlocks: the variable under test in this one
            None, // item_uses: no block here has an item-aware rule
        );
        assert!(recipe_book_bodies(&sent).is_empty());
    }

    /// `use_item_on`: hand, then a `BlockHitResult`, then the sequence.
    pub(super) fn place(pos: BlockPos, face: i32, sequence: i32) -> Vec<u8> {
        place_at(pos, face, sequence, 1.0)
    }

    /// `minecraft:use_item` — hand, sequence, two floats. **No hit result**,
    /// which is the whole difference from [`place`] above.
    pub(super) fn use_item(sequence: i32, y_rot: f32, x_rot: f32) -> Vec<u8> {
        let mut body = Vec::new();
        write_varint(&mut body, 0).expect("buf"); // main hand
        write_varint(&mut body, sequence).expect("buf");
        wire::write_f32(&mut body, y_rot).expect("buf");
        wire::write_f32(&mut body, x_rot).expect("buf");
        let mut frame = Vec::new();
        frame::write_frame(&mut frame, None, ids::play::sb::USE_ITEM, &body).expect("buf");
        frame
    }

    /// The same with the cursor's height inside the clicked block spelled
    /// out. `SlabBlock.canBeReplaced` and `120`'s `type` both read it, and
    /// `place` above pins it at the top of the block, which is the only
    /// value a click on an `UP` face can honestly have.
    pub(super) fn place_at(pos: BlockPos, face: i32, sequence: i32, cursor_y: f32) -> Vec<u8> {
        let mut body = Vec::new();
        write_varint(&mut body, 0).expect("buf"); // main hand
        wire::write_i64(&mut body, pack_pos(pos)).expect("buf");
        write_varint(&mut body, face).expect("buf");
        for v in [0.5f32, cursor_y, 0.5] {
            wire::write_f32(&mut body, v).expect("buf");
        }
        wire::write_bool(&mut body, false).expect("buf"); // inside
        wire::write_bool(&mut body, false).expect("buf"); // world border
        write_varint(&mut body, sequence).expect("buf");
        let mut frame = Vec::new();
        frame::write_frame(&mut frame, None, ids::play::sb::USE_ITEM_ON, &body).expect("buf");
        frame
    }

    /// Give the player an item, select it, and right-click the top face of a
    /// solid block: the block goes one step up and everyone is told.
    ///
    /// `items: None` in the harness, so the creative slot resolves to
    /// nothing — which is the point of the first assertion: without an item
    /// table a placement is silently a no-op, and that must not read as
    /// "placement works".
    ///
    /// The two `block_update`s still go out, and that is not a bug: vanilla
    /// resyncs the clicked position and the position placed into at the end
    /// of `handleUseItemOn` unconditionally, on every path that reaches the
    /// end, whether or not anything changed. A refusal is precisely the case
    /// the client's prediction has to be corrected against, so a refused
    /// placement is the one that needs them most.
    #[test]
    fn a_placement_without_an_item_table_places_nothing() {
        let world = FakeWorld {
            solid: vec![(3, 70, -5)],
            ..FakeWorld::default()
        };
        let mut input = hold(1, 36, 0);
        input.extend(place((3, 70, -5), 1, 4));
        let (sent, wrote) = play(&input, true, &world);
        assert!(wrote.is_empty(), "nothing to place with");
        assert_eq!(
            sent.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec![
                ids::play::cb::BLOCK_UPDATE,
                ids::play::cb::BLOCK_UPDATE,
                ids::play::cb::BLOCK_CHANGED_ACK
            ],
            "the two resyncs go out even though nothing was placed, then the ack"
        );
    }

    /// The placement rules that decide *where*, with the hotbar filled
    /// directly so the test does not depend on the extract.
    #[test]
    fn a_placement_goes_against_the_face_unless_the_click_was_replaceable() {
        // Clicked block solid: the placement goes one step along the normal.
        let solid = FakeWorld {
            solid: vec![(3, 70, -5)],
            ..FakeWorld::default()
        };
        assert_eq!(
            placed(&solid, (3, 70, -5), 1),
            Some(((3, 71, -5), "minecraft:stone[placed=true]".into())),
            "UP from the clicked block"
        );
        // Clicked block replaceable — tall grass, water: the placement takes
        // that block's own position, not the one against the face.
        //
        // **THIS ASSERTION USED TO BE THE WHOLE RULE AND IT WAS NOT.** It
        // said, with the comment above naming tall grass, that replaceability
        // is a property of the position: click something replaceable and the
        // block lands there, whatever you are holding. Vanilla's
        // `BlockBehaviour.canBeReplaced(state, ctx)` is
        // `replaceable && (item.isEmpty() || !item.is(asItem()))`, so the
        // held item is half of it, and the case immediately below — the one
        // that could not be written before — is the half that was missing.
        // The line above is still true for the item it happens to hold.
        let grass = FakeWorld::default();
        assert_eq!(
            placed(&grass, (3, 70, -5), 1),
            Some(((3, 70, -5), "minecraft:stone[placed=true]".into())),
            "into the replaceable block itself, holding something else"
        );
        // The same click, the same replaceable block, holding *that block*.
        let tall_grass = FakeWorld {
            at: vec![((3, 70, -5), "minecraft:tall_grass[half=lower]".into())],
            ..FakeWorld::default()
        };
        let t = crate::placement::PlacementTable::default().with_items(
            std::iter::once((
                "minecraft:tall_grass".to_string(),
                "minecraft:tall_grass".to_string(),
            ))
            .collect(),
        );
        assert_eq!(
            placed_holding(
                &tall_grass,
                (3, 70, -5),
                1,
                1.0,
                "minecraft:tall_grass",
                Some(&t)
            ),
            Some(((3, 71, -5), "minecraft:tall_grass[placed=true]".into())),
            "AGAINST THE FACE, not into it: `!item.is(asItem())` is false, so              the clicked block is not replaceable by this placement and              `getClickedPos()` steps along the normal"
        );
        // The target is solid: refused.
        let blocked = FakeWorld {
            solid: vec![(3, 70, -5), (3, 71, -5)],
            ..FakeWorld::default()
        };
        assert_eq!(placed(&blocked, (3, 70, -5), 1), None);
        // The target is the player's own feet: refused. The player stands at
        // (8, 64, 8), so clicking the top of the block below them aims there.
        let floor = FakeWorld {
            solid: vec![(8, 63, 8)],
            ..FakeWorld::default()
        };
        assert_eq!(placed(&floor, (8, 63, 8), 1), None, "into your own feet");
    }

    /// `170` and `250` end to end, and the two controls that must not move.
    ///
    /// Both scenarios are one click on a block that is already there, and
    /// both need the two halves together: [`crate::placement::can_be_replaced`]
    /// decides the write lands **at the clicked position** rather than above
    /// it, and `merged_into` decides what it writes when it does. Either half
    /// alone reproduces neither — with only the first, a second slab
    /// overwrites the first with another bottom slab and the trace carries no
    /// change at all.
    ///
    /// The controls are the other direction. `120` is a slab clicked on its
    /// side, where the cursor rather than the face decides, and `020` is a
    /// plain block onto a solid one; neither may start merging.
    #[test]
    fn a_second_slab_and_a_second_snow_layer_merge_into_the_first() {
        let table = || {
            let items = [
                ("minecraft:oak_slab", "minecraft:oak_slab"),
                ("minecraft:spruce_slab", "minecraft:spruce_slab"),
                ("minecraft:snow", "minecraft:snow"),
                ("minecraft:stone", "minecraft:stone"),
            ]
            .into_iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect();
            crate::placement::PlacementTable::default()
                .with_items(items)
                .with_slabs(
                    ["minecraft:oak_slab", "minecraft:spruce_slab"]
                        .into_iter()
                        .map(String::from)
                        .collect(),
                )
                .with_snow_layers(std::iter::once("minecraft:snow".to_string()).collect())
        };
        let t = table();
        let world = |state: &str| FakeWorld {
            at: vec![((3, 70, -5), state.to_string())],
            ..FakeWorld::default()
        };

        // 170. The click is the top face, so `f == UP` decides it and the
        // cursor is not consulted.
        assert_eq!(
            placed_holding(
                &world("minecraft:oak_slab[type=bottom,waterlogged=false]"),
                (3, 70, -5),
                1,
                1.0,
                "minecraft:oak_slab",
                Some(&t)
            ),
            Some((
                (3, 70, -5),
                "minecraft:oak_slab[type=double,waterlogged=false]".into()
            )),
            "at the clicked position and as a DOUBLE — no rule about faces              and cursors can produce this, which is 170's own `about`"
        );

        // 250. Two layers, at the position that was clicked.
        assert_eq!(
            placed_holding(
                &world("minecraft:snow[layers=1]"),
                (3, 70, -5),
                1,
                1.0,
                "minecraft:snow",
                Some(&t)
            ),
            Some(((3, 70, -5), "minecraft:snow[layers=2]".into())),
            "snow deepens in place"
        );

        // The control inside 170's own rig: a different wood is a different
        // block, so it stacks. Without this, "merge whenever a slab is
        // clicked" passes the assertion above.
        assert_eq!(
            placed_holding(
                &world("minecraft:oak_slab[type=bottom,waterlogged=false]"),
                (3, 70, -5),
                1,
                1.0,
                "minecraft:spruce_slab",
                Some(&t)
            ),
            Some(((3, 71, -5), "minecraft:spruce_slab[placed=true]".into())),
            "spruce onto oak goes above it"
        );

        // 120's axis: clicked on the SIDE, below the halfway line. A bottom
        // slab is not replaced from there, so the placement steps along the
        // face exactly as it did before any of this existed.
        assert_eq!(
            placed_holding(
                &world("minecraft:oak_slab[type=bottom,waterlogged=false]"),
                (3, 70, -5),
                2,
                0.3,
                "minecraft:oak_slab",
                Some(&t)
            ),
            Some(((3, 70, -6), "minecraft:oak_slab[placed=true]".into())),
            "north face, cursor below the line: against the face, not merged"
        );
        // And above the line on the same face, it does merge — the half of
        // `120`'s quantity that proves the cursor is actually being read
        // rather than the face alone.
        assert_eq!(
            placed_holding(
                &world("minecraft:oak_slab[type=bottom,waterlogged=false]"),
                (3, 70, -5),
                2,
                0.7,
                "minecraft:oak_slab",
                Some(&t)
            ),
            Some((
                (3, 70, -5),
                "minecraft:oak_slab[type=double,waterlogged=false]".into()
            )),
            "same face, cursor above the line: merged"
        );

        // 020: a plain block onto a solid one, with the table present. The
        // whole rule has to stay out of the way of the ordinary case.
        let solid = FakeWorld {
            solid: vec![(3, 70, -5)],
            ..FakeWorld::default()
        };
        assert_eq!(
            placed_holding(&solid, (3, 70, -5), 1, 1.0, "minecraft:stone", Some(&t)),
            Some(((3, 71, -5), "minecraft:stone[placed=true]".into())),
            "020 is unmoved"
        );
    }

    /// Run one placement with `minecraft:stone` already in hand and report
    /// the single write it produced, if any.
    fn placed(world: &FakeWorld, pos: BlockPos, face: i32) -> Option<(BlockPos, String)> {
        placed_holding(world, pos, face, 1.0, "minecraft:stone", None)
    }

    /// The same with the three things `canBeReplaced` reads spelled out: the
    /// cursor's height in the clicked block, the item in hand, and the table
    /// that resolves `Block.asItem()` and the two class overrides.
    ///
    /// `placed` above passes `placement: None`, which is why it kept working
    /// unchanged when `replaceable` became item-aware: with no table every
    /// `is(asItem())` is false and the rule collapses to the flag it used to
    /// be. That is the honest degradation and it is also a blind spot, so the
    /// cases that need a table say so.
    fn placed_holding(
        world: &FakeWorld,
        pos: BlockPos,
        face: i32,
        cursor_y: f32,
        held: &str,
        placement: Option<&crate::placement::PlacementTable>,
    ) -> Option<(BlockPos, String)> {
        let rec = Recorder::default();
        let players = crate::players::Players::new();
        let conn = Arc::new(crate::players::Connection::with_entity(
            players.next_id(),
            "Tester",
            Box::new(rec),
            None,
            0,
            2,
            (0, 0),
        ));
        players.join(&conn);
        open_test_view(&conn);
        let clock = crate::tick::GameClock::new();
        let changed = crate::broadcast::ChangedBlocks::new();
        let block_events = crate::containers::BlockEvents::new();
        let openers = crate::containers::Openers::new();
        let mut player = crate::player::PlayerState::at(8.0, 64.0, 8.0);
        let mut hotbar = crate::player::Hotbar::new();
        let mut menu = crate::menu::MenuState::new();
        let mut known_recipes = std::collections::BTreeSet::new();
        hotbar.set(0, Some(held));
        let chunk_at = |_: i32, _: i32| Ok(None);
        // A private manager per test: `cargo test` runs these on
        // parallel threads in one process, and the shared one would
        // let one test's join make a connection another test's move
        // then reports on.
        let wp = crate::waypoints::Waypoints::new();
        let mut session = Session {
            conn: &conn,
            player: &mut player,
            clock: &clock,
            clock_ids: &[],
            players: &players,
            blocks: Some(world),
            hotbar: &mut hotbar,
            menu: &mut menu,
            items: None,
            placement,
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
            plugins: None,
            creative: true,
            seed: 0,
        };
        let mut cursor = std::io::Cursor::new(place_at(pos, face, 1, cursor_y));
        keep_alive_loop(&mut cursor, &mut session).expect("the loop ends on EOF");
        // The tick boundary the clock thread supplies in a real server, and
        // the reason these fixtures see anything at all: a handler only
        // *records* what it changed and only *remembers* the sequence it
        // owes. Broadcasts first, acknowledgements after — `ServerLevel.tick`
        // reaches `ServerChunkCache.tick` before
        // `ServerGamePacketListenerImpl.tick`, and the gameplay oracle
        // measured real vanilla in exactly that order.
        changed.flush(&players, world);
        players.flush_menu_sweeps();
        players.flush_acks();
        world.wrote.lock().expect("fake world").first().cloned()
    }
}

/// A chunk position, in chunk coordinates.
pub type ChunkPos = (i32, i32);

/// What a view move implies on the wire, before any chunk data goes out.
///
/// `ChunkMap.applyChunkTrackingView` does three things in this order and the
/// order is on the wire: it sends `set_chunk_cache_center` *if the centre
/// moved*, then walks `ChunkTrackingView.difference`, queueing the arrivals
/// into `PlayerChunkSender` and sending `forget_level_chunk` for the
/// departures as it goes. Since the arrivals produce no bytes at this point,
/// every forget packet for one move precedes every chunk packet for it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrackingUpdate {
    /// `Some(centre)` when `set_chunk_cache_center` must be written.
    ///
    /// **Not on every move.** Vanilla sends it only when the new view is
    /// `Positioned` and the old one was either `EMPTY` or centred somewhere
    /// else — so a view-distance change with the player standing still moves
    /// chunks and sends no centre packet at all.
    pub moved_center: Option<ChunkPos>,
    /// The chunks to `forget_level_chunk`, in `difference` order: x-major,
    /// both axes ascending, over the union of the two bounding boxes.
    ///
    /// Chunks that were still queued when they left the view are **not**
    /// here — they are dequeued silently, because the client never had them.
    pub forget: Vec<ChunkPos>,
}

/// Which chunks the client has, which are queued for it, and what changes
/// when the player moves.
///
/// This is `ServerPlayer.chunkTrackingView` and
/// `ServerGamePacketListenerImpl.chunkSender` in one place, because in this
/// server they have one owner and one lifetime. The two halves are
/// [`crate::trackingview`] and [`crate::chunksender`] and both are ports; the
/// only thing here is the wiring and the `sent` set below.
///
/// # This replaced a Chebyshev square, and that was not a rounding difference
///
/// Until 2026-09-05 this type walked `dx, dz in -radius..=radius` and sorted
/// by Chebyshev ring. Vanilla's tracked set is
/// `ChunkTrackingView$Positioned`, a Euclidean disc padded by two inside a
/// box of `viewDistance + 1`, and its send order is by `distanceSquared`.
/// At view distance 10 the square is 441 chunks and the disc is 473, and
/// neither contains the other. See [`crate::trackingview`].
#[derive(Debug, Clone)]
pub struct ChunkView {
    /// `ServerPlayer.chunkTrackingView`. Starts `EMPTY`, exactly as
    /// `ChunkMap.updatePlayerStatus` sets it before the first
    /// `updateChunkTracking`.
    tracking: crate::trackingview::TrackingView,
    /// `ServerGamePacketListenerImpl.chunkSender`.
    sender: crate::chunksender::ChunkSender,
    /// Chunks this client has actually been sent.
    ///
    /// # Vanilla has no such set, and the reason it needs one here is local
    ///
    /// Vanilla's join goes through the same `PlayerChunkSender` as every
    /// later chunk, so "already sent" is simply "no longer pending". This
    /// server's [`enter_world`] writes the spawn square directly onto the
    /// socket before the play loop exists, and then tells this view about it
    /// through [`ChunkView::mark_sent`]. Without the set, the first
    /// `update_tracking` would `difference(EMPTY, disc)` and queue those
    /// chunks a second time.
    ///
    /// It is also what [`ChunkView::holds`] answers, which is what the
    /// session tests assert on.
    sent: std::collections::BTreeSet<ChunkPos>,
    /// `ChunkMap.getPlayerViewDistance(player)` for this connection, already
    /// clamped. Held rather than passed in because
    /// `ChunkMap.updateChunkTracking` re-reads it on every move and a caller
    /// that passed a different one each time would silently rebuild the view.
    view_distance: i32,
    /// The centre the client was last told about.
    ///
    /// # Vanilla reads this off the tracking view and here it cannot
    ///
    /// `applyChunkTrackingView` compares `oldView.center()` with
    /// `newView.center()`, which works because in vanilla every
    /// `set_chunk_cache_center` is written by that method. This server's
    /// [`enter_world`] writes one directly, before the play loop exists and
    /// while the tracking view is still `EMPTY` — so comparing against the
    /// view would send a second, identical centre packet on the first move.
    /// Held separately for that one reason; the comparison is otherwise
    /// vanilla's.
    announced_center: ChunkPos,
}

impl ChunkView {
    /// A view for a player who has just joined.
    ///
    /// The tracking view starts `EMPTY` whatever `center` says — that is
    /// `updatePlayerStatus`'s `setChunkTrackingView(ChunkTrackingView.EMPTY)`
    /// — and `center` is remembered only so that [`Self::center`] has an
    /// answer before the first move.
    #[must_use]
    pub fn new(center: ChunkPos, view_distance: i32) -> Self {
        Self {
            tracking: crate::trackingview::TrackingView::Empty,
            sender: crate::chunksender::ChunkSender::new(false),
            sent: std::collections::BTreeSet::new(),
            view_distance,
            announced_center: center,
        }
    }

    /// Back to a fresh view, the client holding nothing: what a respawn
    /// leaves (`PlayerList.respawn` builds a new `ServerPlayer`, whose view
    /// is `EMPTY`). The centre is forgotten too, so the next update announces
    /// it, as `applyChunkTrackingView` does from `EMPTY`.
    pub fn forget_everything(&mut self) {
        *self = Self::new((i32::MIN, i32::MIN), self.view_distance);
    }

    /// Record that the client already has this chunk — for the square
    /// [`enter_world`] wrote before this view existed. See the `sent` field.
    pub fn mark_sent(&mut self, chunk: ChunkPos) {
        self.sent.insert(chunk);
    }

    /// Where the client's chunk cache is centred.
    #[must_use]
    pub fn center(&self) -> ChunkPos {
        match self.tracking {
            crate::trackingview::TrackingView::Positioned { center, .. } => center,
            crate::trackingview::TrackingView::Empty => self.announced_center,
        }
    }

    /// Has this client been sent this chunk?
    #[must_use]
    pub fn holds(&self, chunk: ChunkPos) -> bool {
        self.sent.contains(&chunk)
    }

    /// `ChunkMap.isChunkTracked(player, x, z)` —
    /// `getChunkTrackingView().contains(x, z) && !chunkSender.isPending(...)`.
    ///
    /// **The `isPending` half is not bookkeeping.** It is the only filter on
    /// `ChunkHolder.broadcastChanges`'s recipient list, so a block change in
    /// a chunk that is tracked but still queued is withheld from that client
    /// entirely — not deferred, withheld. That is what made vanilla look
    /// nondeterministic to the gameplay harness for a day
    /// (`tools/oracle-runner/src/scenario.rs`, `From`).
    #[must_use]
    pub fn is_chunk_tracked(&self, chunk: ChunkPos) -> bool {
        self.tracking.contains(chunk) && !self.sender.is_pending(chunk)
    }

    /// How many chunks are queued but not yet sent.
    #[must_use]
    pub fn pending_len(&self) -> usize {
        self.sender.pending_len()
    }

    /// Is any tracked chunk still waiting to go out?
    #[must_use]
    pub fn missing(&self) -> bool {
        self.sender.pending_len() > 0
    }

    /// `ServerGamePacketListenerImpl.handleChunkBatchReceived` — the client's
    /// answer, carrying the rate it wants.
    pub fn on_batch_received(&mut self, desired_chunks_per_tick: f32) {
        self.sender.on_batch_received(desired_chunks_per_tick);
    }

    /// `ChunkMap.updateChunkTracking` followed by `applyChunkTrackingView`.
    ///
    /// Returns what has to be written, and queues the arrivals. Writing
    /// nothing when the view has not changed is vanilla's own early return
    /// and is what stops a stationary player being re-sent a world twenty
    /// times a second.
    pub fn update_tracking(&mut self, centre: ChunkPos) -> TrackingUpdate {
        let new = crate::trackingview::TrackingView::of(centre, self.view_distance);
        if self.tracking == new {
            return TrackingUpdate::default();
        }
        let moved_center = if self.announced_center == centre {
            None
        } else {
            self.announced_center = centre;
            Some(centre)
        };
        let mut add = Vec::new();
        let mut forget = Vec::new();
        crate::trackingview::difference(&self.tracking, &new, |c| add.push(c), |c| forget.push(c));
        for c in add {
            // The `sent` guard is this server's, not vanilla's — see the
            // field. `difference` already guarantees the chunk was outside
            // the old view, so without the join square this branch would
            // never be taken.
            if !self.sent.contains(&c) {
                self.sender.mark_pending(c);
            }
        }
        forget.retain(|c| {
            self.sent.remove(c);
            self.sender.drop_chunk(*c)
        });
        self.tracking = new;
        TrackingUpdate {
            moved_center,
            forget,
        }
    }

    /// `PlayerChunkSender.sendNextChunks` — one tick's batch, in send order.
    ///
    /// Every returned chunk is recorded as sent. `available` answers
    /// `ChunkMap.getChunkToSend(pos) != null`.
    pub fn take_batch(
        &mut self,
        centre: ChunkPos,
        available: impl Fn(ChunkPos) -> bool,
    ) -> Vec<ChunkPos> {
        let batch = self.sender.send_next_chunks(centre, available);
        for c in &batch {
            self.sent.insert(*c);
        }
        batch
    }
}

/// `forget_level_chunk`'s body.
///
/// # Errors
/// On a write failure, which for a `Vec` cannot happen.
pub fn forget_body(chunk: (i32, i32)) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    // `FriendlyByteBuf.writeChunkPos` is `writeLong(ChunkPos.pack())`, and
    // `pack` puts **x in the low 32 bits and z in the high** — the opposite
    // of the reading order, and the kind of thing that silently transposes a
    // world.
    let packed = (i64::from(chunk.1) << 32) | i64::from(chunk.0 as u32);
    wire::write_i64(&mut out, packed)?;
    Ok(out)
}

/// Tell the client to drop a chunk.
///
/// # Errors
/// On I/O failure.
pub fn forget_chunk(
    stream: &mut impl Write,
    chunk: (i32, i32),
    comp: Option<i32>,
) -> io::Result<()> {
    frame::write_frame(
        stream,
        comp,
        ids::play::cb::FORGET_LEVEL_CHUNK,
        &forget_body(chunk)?,
    )
}

#[cfg(test)]
mod view_tests {
    use super::ChunkView;

    fn drain(view: &mut ChunkView, centre: (i32, i32)) -> Vec<(i32, i32)> {
        let mut all = Vec::new();
        loop {
            let b = view.take_batch(centre, |_| true);
            if b.is_empty() {
                break;
            }
            all.extend(b);
            view.on_batch_received(64.0);
        }
        all
    }

    /// A fresh view is `EMPTY` and its first update queues the whole disc,
    /// nearest first once the sender has ordered it.
    ///
    /// 49 rather than 25: at view distance 2 the padded disc swallows its
    /// whole `(2 + 1)`-wide bounding box. The old Chebyshev square gave 25,
    /// and every chunk of the ring between them is one vanilla sends and it
    /// did not.
    #[test]
    fn a_new_view_queues_its_whole_disc_nearest_first() {
        let mut view = ChunkView::new((0, 0), 2);
        let u = view.update_tracking((0, 0));
        assert_eq!(
            u.moved_center, None,
            "the join already wrote this centre; see `announced_center`"
        );
        assert!(u.forget.is_empty());
        assert_eq!(view.pending_len(), 49);
        let sent = drain(&mut view, (0, 0));
        assert_eq!(sent.len(), 49);
        assert_eq!(sent[0], (0, 0), "the centre first");
        let d = |c: (i32, i32)| c.0 * c.0 + c.1 * c.1;
        assert!(
            sent.windows(2).all(|w| d(w[0]) <= d(w[1])),
            "batches are nearest-first and batches go out in order"
        );
    }

    /// `548`'s crossing, the recording that refuted the square: at view
    /// distance 4, stepping from chunk `(0, 0)` to `(0, 1)` forgets exactly
    /// the eleven chunks vanilla forgot and sends eleven.
    #[test]
    fn crossing_one_boundary_at_view_distance_four_moves_eleven_chunks() {
        let mut view = ChunkView::new((0, 0), 4);
        view.update_tracking((0, 0));
        drain(&mut view, (0, 0));
        let u = view.update_tracking((0, 1));
        let mut forget = u.forget;
        forget.sort_unstable();
        let mut vanilla: Vec<(i32, i32)> = (-4..=4).map(|x| (x, -5)).collect();
        vanilla.extend([(-5, -4), (5, -4)]);
        vanilla.sort_unstable();
        assert_eq!(forget, vanilla);
        assert_eq!(view.pending_len(), 11);
    }

    /// The send order is vanilla's metric, and this test exists to separate
    /// it from the one this path used before PARITY-ROADMAP #70.
    ///
    /// `PlayerChunkSender.collectChunksToSend` sorts on
    /// `ChunkPos.distanceSquared` — `dx*dx + dz*dz`. Chebyshev,
    /// `max(|dx|,|dz|)`, agrees with it completely out to radius 3: the first
    /// radius at which any pair is ordered differently is **4**, so a test at
    /// the corpus's own radius passes under both and proves nothing.
    ///
    /// (3,3) and (4,0) are the cheapest witness. Chebyshev ranks (3,3) at 3
    /// and (4,0) at 4; vanilla ranks them 18 and 16 and sends (4,0) first.
    #[test]
    fn the_send_order_is_distance_squared_and_not_chebyshev() {
        let mut view = ChunkView::new((0, 0), 4);
        view.update_tracking((0, 0));
        let send = drain(&mut view, (0, 0));
        let at = |c: (i32, i32)| {
            send.iter()
                .position(|x| *x == c)
                .unwrap_or_else(|| panic!("{c:?} is inside a radius-4 disc"))
        };
        assert!(
            at((4, 0)) < at((3, 3)),
            "vanilla sends (4,0) at d^2=16 before (3,3) at d^2=18; Chebyshev \
             would put (3,3) first at ring 3 against ring 4"
        );
        let d2 = |c: (i32, i32)| c.0 * c.0 + c.1 * c.1;
        for w in send.windows(2) {
            assert!(
                d2(w[0]) <= d2(w[1]),
                "{:?} (d^2={}) sent before {:?} (d^2={})",
                w[0],
                d2(w[0]),
                w[1],
                d2(w[1])
            );
        }
    }

    /// The centre packet is sent when the centre moves and **not** when only
    /// the queue changes. Vanilla's `applyChunkTrackingView` compares the two
    /// centres; the version this replaced sent it on every call.
    #[test]
    fn the_centre_packet_only_goes_out_when_the_centre_moves() {
        let mut view = ChunkView::new((0, 0), 2);
        assert_eq!(
            view.update_tracking((0, 0)).moved_center,
            None,
            "the join wrote (0, 0) already"
        );
        assert_eq!(
            view.update_tracking((0, 0)),
            super::TrackingUpdate::default(),
            "an unchanged view is vanilla's early return: no packets at all"
        );
        assert_eq!(view.update_tracking((1, 0)).moved_center, Some((1, 0)));
        assert_eq!(
            view.update_tracking((1, 0)),
            super::TrackingUpdate::default()
        );
        assert_eq!(view.update_tracking((0, 0)).moved_center, Some((0, 0)));
    }

    /// One step east moves a crescent, not a column, because the tracked set
    /// is a disc. At view distance 10 that is 23 chunks each way; the
    /// Chebyshev square this replaced moved 21.
    #[test]
    fn one_step_moves_a_crescent() {
        let mut view = ChunkView::new((0, 0), 10);
        view.update_tracking((0, 0));
        drain(&mut view, (0, 0));
        assert_eq!(view.pending_len(), 0);

        let u = view.update_tracking((1, 0));
        assert_eq!(u.forget.len(), 23, "21 under a Chebyshev square");
        assert_eq!(view.pending_len(), 23);
        assert!(
            u.forget.windows(2).all(|w| w[0] <= w[1]),
            "difference sweeps x-major ascending, which is lexicographic"
        );
    }

    /// A chunk that entered the view and left again before its batch went out
    /// gets **no** `forget_level_chunk`: the client never had it. That is
    /// `PlayerChunkSender.dropChunk`'s `if (!pendingChunks.remove(pos))`.
    #[test]
    fn a_chunk_that_never_went_out_is_forgotten_silently() {
        let mut view = ChunkView::new((0, 0), 2);
        view.update_tracking((0, 0));
        // Nothing drained, so everything is still pending.
        let u = view.update_tracking((40, 40));
        assert!(
            u.forget.is_empty(),
            "the whole old view was queued and never sent, so nothing to forget"
        );
        assert_eq!(
            view.pending_len(),
            49,
            "and the new view replaced the queue"
        );
    }

    /// A view that never forgot would grow without bound as a player walks.
    #[test]
    fn walking_in_a_line_does_not_grow_the_view() {
        let mut view = ChunkView::new((0, 0), 2);
        for step in 0..40 {
            let u = view.update_tracking((step, 0));
            drain(&mut view, (step, 0));
            for c in &u.forget {
                assert!(!view.holds(*c), "forgotten chunks are dropped");
            }
        }
        let held = (-40..=45)
            .flat_map(|x| (-40..=45).map(move |z| (x, z)))
            .filter(|c| view.holds(*c))
            .count();
        assert_eq!(
            held, 49,
            "exactly the current disc is held, forty chunks after the start"
        );
    }

    /// `ChunkMap.isChunkTracked`: a queued chunk is *not* tracked, which is
    /// what withholds its block changes from that client.
    #[test]
    fn a_queued_chunk_is_not_yet_tracked() {
        let mut view = ChunkView::new((0, 0), 2);
        view.update_tracking((0, 0));
        assert!(
            !view.is_chunk_tracked((0, 0)),
            "in the view but still pending"
        );
        drain(&mut view, (0, 0));
        assert!(view.is_chunk_tracked((0, 0)));
        assert!(
            !view.is_chunk_tracked((40, 40)),
            "outside the view entirely"
        );
    }

    /// The join square this server writes directly is not queued a second
    /// time. There is no vanilla analogue — see `ChunkView::sent`.
    #[test]
    fn a_chunk_already_written_by_the_join_is_not_requeued() {
        let mut view = ChunkView::new((0, 0), 2);
        for x in -2..=2 {
            for z in -2..=2 {
                view.mark_sent((x, z));
            }
        }
        view.update_tracking((0, 0));
        assert_eq!(
            view.pending_len(),
            49 - 25,
            "the 25 the join wrote are held, the rest of the disc is queued"
        );
        assert!(view.holds((0, 0)));
    }
}

/// `play/set_time` — the game clock, as the client sees it.
///
/// Reference (26.2): `ClientboundSetTimePacket` is a **raw i64** game time
/// followed by a map of `WorldClock` holders to `ClockNetworkState`. Older
/// protocol notes describe three fields — game time, day time, and a
/// do-daylight-cycle flag — and that packet no longer exists; sending it
/// would desynchronise everything after it in the frame.
///
/// Each clock update is a `WorldClock` holder and a `ClockNetworkState`.
/// The holder is a **plain varint registry id** — `WorldClock.STREAM_CODEC`
/// is `holderRegistry`, which resolves to `idMapper` and writes
/// `VarInt.write(buf, id)`. It is *not* the `holder(...)` form with its
/// `id + 1` and a zero meaning an inline value; the two are a byte apart and
/// only one of them is this packet.
///
/// # Errors
/// On I/O failure.
pub fn send_time(
    stream: &mut impl Write,
    game_time: i64,
    clocks: &[(i32, ClockState)],
    comp: Option<i32>,
) -> io::Result<()> {
    frame::write_frame(
        stream,
        comp,
        ids::play::cb::SET_TIME,
        &time_body(game_time, clocks)?,
    )
}

/// `set_time`'s body.
///
/// # Errors
/// On a write failure, which for a `Vec` cannot happen.
pub fn time_body(game_time: i64, clocks: &[(i32, ClockState)]) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    // A raw i64, not a varlong — `ByteBufCodecs.LONG`.
    wire::write_i64(&mut out, game_time)?;
    // `ByteBufCodecs.map` is a varint count then the pairs.
    write_varint(&mut out, i32::try_from(clocks.len()).unwrap_or(i32::MAX))?;
    for (id, state) in clocks {
        write_varint(&mut out, *id)?;
        rustmc_proto::varint::write_varlong(&mut out, state.total_ticks)?;
        wire::write_f32(&mut out, state.partial_tick)?;
        wire::write_f32(&mut out, state.rate)?;
    }
    Ok(out)
}

/// `ClockNetworkState` — varlong ticks, then two floats.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClockState {
    pub total_ticks: i64,
    /// How far into the current tick, for the client to interpolate from.
    pub partial_tick: f32,
    /// Ticks of this clock per game tick. 1.0 is normal time; 0.0 stops it,
    /// which is what a frozen or fixed-time world looks like.
    pub rate: f32,
}

#[cfg(test)]
mod time_tests {
    use super::{send_time, ClockState};
    use rustmc_proto::frame;

    /// The body is a raw i64, a varint count, then per clock a **plain**
    /// varint id and the three `ClockNetworkState` fields. The id is the
    /// byte most easily got wrong: `holderRegistry` resolves to `idMapper`
    /// and writes the id as-is, while the other holder form writes `id + 1`
    /// and reserves 0 for an inline value.
    #[test]
    fn a_clock_update_is_a_plain_id_then_varlong_and_two_floats() {
        let mut out = Vec::new();
        send_time(
            &mut out,
            0x0102_0304_0506_0708,
            &[(
                7,
                ClockState {
                    total_ticks: 300,
                    partial_tick: 0.0,
                    rate: 1.0,
                },
            )],
            None,
        )
        .expect("buf");

        let (id, body) = frame::read_frame(&mut &out[..], false).expect("frame");
        assert_eq!(id, rustmc_proto::ids::play::cb::SET_TIME);
        assert_eq!(
            &body[..8],
            &[0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08],
            "game time is a raw big-endian i64, not a varlong"
        );
        assert_eq!(body[8], 1, "one clock");
        assert_eq!(body[9], 7, "the registry id, written as-is");
        // 300 as a varlong is two bytes.
        assert_eq!(&body[10..12], &[0xAC, 0x02]);
        assert_eq!(
            &body[12..16],
            &0.0f32.to_bits().to_be_bytes(),
            "partial_tick comes before rate"
        );
        assert_eq!(&body[16..20], &1.0f32.to_bits().to_be_bytes(), "then rate");
        assert_eq!(body.len(), 20, "no trailing bytes");
    }

    #[test]
    fn no_clocks_is_a_zero_count() {
        let mut out = Vec::new();
        send_time(&mut out, 0, &[], None).expect("buf");
        let (_, body) = frame::read_frame(&mut &out[..], false).expect("frame");
        assert_eq!(body.len(), 9, "eight bytes of time and a zero count");
        assert_eq!(body[8], 0);
    }
}

/// **The plugin hook, end to end.**
///
/// A real [`grit_plugin_api::Plugin`], enabled by the real
/// [`grit_plugin_api::load`], refusing a dig that arrives as a real
/// `player_action` frame on a real [`Session`] — and the block still standing
/// afterwards according to [`crate::edits::Blocks::state_at`], which is the
/// server's own read and not the plugin API agreeing with itself.
///
/// # Why this module has its own world instead of using `FakeWorld`
///
/// `FakeWorld` **records** writes; its `state_at` answers from a fixed list
/// and never changes. Every assertion here would pass against it whether or
/// not the denial worked, because the block it claims is "still stone" was
/// never capable of being anything else. A check that cannot fail is the
/// defect this project keeps finding in itself, so the world here is
/// [`crate::edits::BlockEdits`] — the server's own overlay, the same one
/// `crate::world::World` writes into — and a removal really does turn the
/// position to air.
///
/// # What would make each test here come back red
///
/// * `a_protected_block_survives_a_dig_and_an_unprotected_one_does_not` —
///   the hook not being called, the verdict not being read, or the removal
///   happening anyway. Its second half is the one that catches a plugin (or
///   a hook) that refuses *everything*, which a one-sided test would call a
///   pass.
/// * `without_a_plugin_the_same_position_breaks` — the control. Without it,
///   a bug making that position unwritable for any unrelated reason would
///   read as a working protection plugin.
/// * `the_plugin_is_actually_loaded` — the loader silently dropping the
///   plugin. Without it the two above are only distinguishable if something
///   is listening at all.
/// * `a_plugin_missing_a_capability_is_refused_and_the_dig_goes_through` —
///   a refusal that leaves handlers registered anyway, and the *variant*
///   assertion catches a refusal coming from the registration backstop
///   rather than from the manifest check.
#[cfg(test)]
mod plugin_hook_tests {
    use super::dig_tests::{dig, place, Recorder};
    use super::*;
    use crate::edits::{BlockEdits, Blocks};
    use crate::interact::BlockPos;
    use crate::plugins::Runtime;
    use grit_plugin_api::event::{BlockBreak, BlockPlace, Priority, Verdict};
    use grit_plugin_api::{
        BlockPos as ApiPos, CapabilitySet, Ctx, Manifest, Plugin, PluginError, PluginId, Region,
        Setup, WorldId, API_VERSION,
    };
    use std::sync::{Arc, Mutex};

    /// A world whose writes are visible to the next read.
    ///
    /// `BlockEdits` is the server's real overlay. Everything else here is the
    /// smallest block table two states need — the point of this fixture is
    /// the *storage*, not the registry.
    struct EditsWorld {
        edits: BlockEdits,
    }

    impl EditsWorld {
        fn new() -> Self {
            Self {
                edits: BlockEdits::new(),
            }
        }
    }

    const STATES: [&str; 3] = ["minecraft:air", "minecraft:stone", "minecraft:dirt"];

    impl Blocks for EditsWorld {
        fn state_at(&self, pos: BlockPos) -> String {
            self.edits
                .get(pos)
                .unwrap_or_else(|| "minecraft:air".to_owned())
        }
        fn set_block(&self, pos: BlockPos, state: &str) -> Option<i32> {
            if !self.has_state(state) {
                return None;
            }
            // `LevelChunk.setBlockState` returns null when nothing changed,
            // and `Level.setBlock` then returns false without touching a
            // neighbour. Reproducing that is what stops a cascade, and it is
            // what makes `SetOutcome::Unchanged` mean something.
            if self.state_at(pos) == state {
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
        fn replaceable(&self, pos: BlockPos, _: &crate::placement::Replacing<'_>) -> bool {
            self.state_at(pos) == "minecraft:air"
        }
        fn face_sturdy(&self, pos: BlockPos, _: crate::interact::Face) -> bool {
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

    /// A region-protection plugin, written against `grit-plugin-api` and
    /// nothing else.
    ///
    /// This is `grit:protect`'s shape, deliberately: the same manifest
    /// requirements, the same two registrations, the same `Priority::Normal`.
    /// It is not that plugin — that one lives in the `grit-plugins` repo and
    /// this repo does not depend on it — but it is a plugin by the contract's
    /// definition and it is loaded by the contract's loader, so what this
    /// proves about the server is the same thing.
    struct Protect {
        claims: Arc<Mutex<Vec<(Region, String)>>>,
    }

    impl Protect {
        fn new() -> Self {
            Self {
                claims: Arc::new(Mutex::new(Vec::new())),
            }
        }
        fn claim(&self, region: Region, reason: &str) {
            self.claims
                .lock()
                .expect("claims")
                .push((region, reason.to_owned()));
        }
    }

    fn refusal(
        claims: &Arc<Mutex<Vec<(Region, String)>>>,
        world: &WorldId,
        pos: ApiPos,
    ) -> Option<String> {
        claims
            .lock()
            .ok()?
            .iter()
            .find(|(r, _)| r.contains(world, pos))
            .map(|(_, why)| why.clone())
    }

    impl Plugin for Protect {
        fn manifest(&self) -> Manifest {
            use grit_plugin_api::capability::well_known as cap;
            Manifest::new(
                PluginId::parse("grit:testprotect").expect("literal"),
                API_VERSION,
            )
            .requires(
                CapabilitySet::from_strs([cap::EVENT_BLOCK_BREAK, cap::EVENT_BLOCK_PLACE])
                    .expect("literals"),
            )
        }

        fn enable(&mut self, setup: &mut Setup<'_>) -> Result<(), PluginError> {
            let breaks = Arc::clone(&self.claims);
            let places = Arc::clone(&self.claims);
            setup.events().on::<BlockBreak, _>(
                Priority::Normal,
                move |ev: &BlockBreak, v: &mut Verdict, _c: &mut Ctx<'_>| {
                    if let Some(why) = refusal(&breaks, &ev.world, ev.pos) {
                        v.deny(why);
                    }
                },
            )?;
            setup.events().on::<BlockPlace, _>(
                Priority::Normal,
                move |ev: &BlockPlace, v: &mut Verdict, _c: &mut Ctx<'_>| {
                    if let Some(why) = refusal(&places, &ev.world, ev.pos) {
                        v.deny(why);
                    }
                },
            )?;
            Ok(())
        }
    }

    /// A plugin needing something this server does not provide.
    struct NeedsCommands;

    impl Plugin for NeedsCommands {
        fn manifest(&self) -> Manifest {
            use grit_plugin_api::capability::well_known as cap;
            Manifest::new(
                PluginId::parse("grit:needscommands").expect("literal"),
                API_VERSION,
            )
            .requires(CapabilitySet::from_strs([cap::COMMAND_REGISTER]).expect("literal"))
        }
        fn enable(&mut self, _setup: &mut Setup<'_>) -> Result<(), PluginError> {
            panic!("enable must not run for a plugin refused at the manifest check");
        }
    }

    fn world_id() -> WorldId {
        WorldId::parse("minecraft:overworld").expect("literal")
    }

    /// A 5x5x5 claim at the origin.
    fn protector() -> Protect {
        let p = Protect::new();
        p.claim(
            Region::from_corners(world_id(), ApiPos::new(0, 0, 0), ApiPos::new(4, 4, 4)),
            "this area is protected",
        );
        p
    }

    /// Drive real client frames through the real play loop with `rt`
    /// installed, and report the frames that went back out.
    ///
    /// Everything between the packet and the world is production code:
    /// `keep_alive_loop` -> `handle_player_action` / `handle_use_item_on` ->
    /// `crate::plugins` -> `shapes::remove_block` / `shapes::set_block`.
    fn drive(
        input: &[u8],
        world: &EditsWorld,
        rt: Option<&Runtime>,
        held: Option<&str>,
    ) -> Vec<(i32, Vec<u8>)> {
        let rec = Recorder::default();
        let players = crate::players::Players::new();
        let conn = Arc::new(crate::players::Connection::with_entity(
            players.next_id(),
            "Tester",
            Box::new(rec.clone()),
            None,
            0,
            2,
            (0, 0),
        ));
        players.join(&conn);
        // The join's own two steps, which is what puts this connection into
        // the recipient list of `ChunkHolder.broadcastChanges`:
        // `enter_world` writes the spawn blob and tells the view
        // (`mark_sent`), then `ChunkMap.updatePlayerStatus(player, true)`
        // runs `updateChunkTracking`. A view that has had neither is `EMPTY`,
        // and vanilla withholds every block update from a client in that
        // state.
        open_test_view(&conn);
        let clock = crate::tick::GameClock::new();
        let changed = crate::broadcast::ChangedBlocks::new();
        let block_events = crate::containers::BlockEvents::new();
        let openers = crate::containers::Openers::new();
        // Between the claimed (2, 2, 2) and the unclaimed (9, 2, 2), inside
        // `isWithinBlockInteractionRange` of both: a dig out of reach is
        // refused before any plugin is asked, so a player standing at the
        // old (8, 64, 8) would have tested nothing about the hook.
        let mut player = crate::player::PlayerState::at(5.5, 2.0, 2.5);
        let mut hotbar = crate::player::Hotbar::new();
        hotbar.set(0, held);
        let mut menu = crate::menu::MenuState::new();
        let mut known_recipes = std::collections::BTreeSet::new();
        let chunk_at = |_: i32, _: i32| Ok(None);
        // A private manager per test: `cargo test` runs these on
        // parallel threads in one process, and the shared one would
        // let one test's join make a connection another test's move
        // then reports on.
        let wp = crate::waypoints::Waypoints::new();
        let mut session = Session {
            conn: &conn,
            player: &mut player,
            clock: &clock,
            clock_ids: &[],
            players: &players,
            blocks: Some(world),
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
            plugins: rt,
            creative: true,
            // Written out rather than defaulted, like every other field here.
            // `Session` cannot derive `Default` — five of its fields are
            // borrows — and a partial default would be worse than the missing
            // one: `0` is a legal seed and the seed this server actually
            // ships (`status::WORLD_SEED`), so a `/seed` answered from a
            // forgotten field would be indistinguishable from a correct one.
            // The compiler refusing this literal is what turned "two branches
            // added a field at the same anchor" into a build error instead of
            // a silent merge, which is the guard worth keeping.
            seed: 0,
        };
        let mut cursor = std::io::Cursor::new(input.to_vec());
        keep_alive_loop(&mut cursor, &mut session).expect("the loop ends on EOF");
        changed.flush(&players, world);
        players.flush_menu_sweeps();
        players.flush_acks();
        let bytes = rec.0.lock().expect("recorder").clone();
        let mut cur = &bytes[..];
        let mut out = Vec::new();
        while let Ok(frame) = frame::read_frame(&mut cur, false) {
            out.push(frame);
        }
        out
    }

    /// A creative client destroys on `START_DESTROY_BLOCK`.
    const START: i32 = 0;

    fn stone_at(world: &EditsWorld, positions: &[BlockPos]) {
        for &p in positions {
            world.edits.set(p, "minecraft:stone");
        }
    }

    /// The plugin is loaded and listening. Without this the two behavioural
    /// tests below could both pass with no plugin present at all.
    #[test]
    fn the_plugin_is_actually_loaded() {
        let rt = Runtime::install(vec![Box::new(protector())]);
        assert_eq!(
            rt.loaded()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            vec!["grit:testprotect"],
        );
        assert!(rt.refused().is_empty(), "{:?}", rt.refused());
        assert_eq!(rt.break_listeners(), 1);
        assert_eq!(rt.place_listeners(), 1);
    }

    /// **The slice.** Inside the claim the block survives a real dig packet;
    /// outside it, the real server removes it.
    #[test]
    fn a_protected_block_survives_a_dig_and_an_unprotected_one_does_not() {
        let world = EditsWorld::new();
        stone_at(&world, &[(2, 2, 2), (9, 2, 2)]);
        let rt = Runtime::install(vec![Box::new(protector())]);

        // Precondition, read through the server's own trait.
        assert_eq!(Blocks::state_at(&world, (2, 2, 2)), "minecraft:stone");
        assert_eq!(Blocks::state_at(&world, (9, 2, 2)), "minecraft:stone");

        let mut input = dig(START, (2, 2, 2), 1);
        input.extend(dig(START, (9, 2, 2), 2));
        drive(&input, &world, Some(&rt), None);

        assert_eq!(
            Blocks::state_at(&world, (2, 2, 2)),
            "minecraft:stone",
            "the protected block is still there, in the server's own world"
        );
        assert_eq!(
            Blocks::state_at(&world, (9, 2, 2)),
            "minecraft:air",
            "outside the claim `shapes::remove_block` took it, as it always did"
        );
    }

    /// The control: the very same position, the very same packet, no plugin.
    ///
    /// Without this, a bug that made (2,2,2) unwritable for any reason at all
    /// would read as a working protection plugin.
    #[test]
    fn without_a_plugin_the_same_position_breaks() {
        for rt in [None, Some(Runtime::install(Vec::new()))] {
            let world = EditsWorld::new();
            stone_at(&world, &[(2, 2, 2)]);
            drive(&dig(START, (2, 2, 2), 1), &world, rt.as_ref(), None);
            assert_eq!(
                Blocks::state_at(&world, (2, 2, 2)),
                "minecraft:air",
                "with nothing listening the same position is removed"
            );
        }
    }

    /// A refused player is told why, on the wire, in a `system_chat`.
    ///
    /// A protection plugin that silently swallowed the action would be
    /// indistinguishable from lag.
    #[test]
    fn a_refused_player_is_told_the_reason() {
        let world = EditsWorld::new();
        stone_at(&world, &[(2, 2, 2)]);
        let rt = Runtime::install(vec![Box::new(protector())]);
        let sent = drive(&dig(START, (2, 2, 2), 1), &world, Some(&rt), None);
        let chat: Vec<&(i32, Vec<u8>)> = sent
            .iter()
            .filter(|(id, _)| *id == ids::play::cb::SYSTEM_CHAT)
            .collect();
        assert_eq!(chat.len(), 1, "one message, from the handler that refused");
        let text = String::from_utf8_lossy(&chat[0].1);
        assert!(
            text.contains("this area is protected"),
            "the reason reaches the player: {text:?}"
        );
    }

    /// The acknowledgement still goes out when a plugin refuses.
    ///
    /// This is the one thing a denial must not break. The client is holding
    /// its own prediction that the block is gone, and the ack is the only
    /// packet that can talk it out of that — a refused dig is precisely the
    /// case the prediction exists for.
    #[test]
    fn a_refused_dig_is_still_acknowledged() {
        let world = EditsWorld::new();
        stone_at(&world, &[(2, 2, 2)]);
        let rt = Runtime::install(vec![Box::new(protector())]);
        let sent = drive(&dig(START, (2, 2, 2), 7), &world, Some(&rt), None);
        let acks: Vec<&(i32, Vec<u8>)> = sent
            .iter()
            .filter(|(id, _)| *id == ids::play::cb::BLOCK_CHANGED_ACK)
            .collect();
        assert_eq!(acks.len(), 1, "the refused dig is acknowledged");
        assert_eq!(
            acks[0].1,
            crate::interact::block_changed_ack_body(7).expect("buf"),
            "and with the sequence the client sent"
        );
    }

    /// Placing inside a claim is refused too, and outside it the real
    /// placement path still writes.
    ///
    /// A protection plugin that stopped breaking but not placing would let
    /// anyone wall a claim in.
    #[test]
    fn placing_inside_a_claim_is_refused_and_outside_it_is_not() {
        let rt = Runtime::install(vec![Box::new(protector())]);

        // Inside: click the top of (3,2,3) so the placement aims at (3,3,3).
        let inside = EditsWorld::new();
        stone_at(&inside, &[(3, 2, 3)]);
        drive(
            &place((3, 2, 3), 1, 1),
            &inside,
            Some(&rt),
            Some("minecraft:dirt"),
        );
        assert_eq!(
            Blocks::state_at(&inside, (3, 3, 3)),
            "minecraft:air",
            "nothing was written inside the claim"
        );

        // Outside: the same packet shape at (20,2,3) -> (20,3,3).
        let outside = EditsWorld::new();
        stone_at(&outside, &[(20, 2, 3)]);
        drive(
            &place((20, 2, 3), 1, 1),
            &outside,
            Some(&rt),
            Some("minecraft:dirt"),
        );
        assert_eq!(
            Blocks::state_at(&outside, (20, 3, 3)),
            "minecraft:dirt",
            "outside the claim the placement path wrote through as it always did"
        );
    }

    /// A plugin requiring something this server does not back is refused by
    /// name, registers nothing, and does not stop a dig.
    ///
    /// **Which defence fired matters**, and asserting only "it was refused"
    /// is what let the plugins repo's own falsification sweep pass a deleted
    /// manifest requirement: the registration check inside `EventBus::on` had
    /// caught it instead, after `enable` had already started running. The
    /// manifest check is the one that matters, so the variant is asserted —
    /// and `NeedsCommands::enable` panics, so a refusal that let it run at
    /// all fails here rather than passing quietly.
    #[test]
    fn a_plugin_missing_a_capability_is_refused_and_the_dig_goes_through() {
        let rt = Runtime::install(vec![Box::new(NeedsCommands)]);
        assert!(rt.loaded().is_empty());
        assert_eq!(rt.refused().len(), 1);
        assert!(
            rt.refused()[0].contains("grit:needscommands")
                && rt.refused()[0].contains("grit:command.register"),
            "the refusal names the plugin and the missing capability: {:?}",
            rt.refused()
        );
        assert_eq!(rt.break_listeners(), 0, "nothing was registered");

        let world = EditsWorld::new();
        stone_at(&world, &[(2, 2, 2)]);
        drive(&dig(START, (2, 2, 2), 1), &world, Some(&rt), None);
        assert_eq!(
            Blocks::state_at(&world, (2, 2, 2)),
            "minecraft:air",
            "a refused plugin protects nothing"
        );
    }

    /// A handler's own write goes through `Level.setBlock` and reaches the
    /// world — the same call the dig path makes.
    ///
    /// Without this, `WorldBackend::set_block` could be a no-op and every
    /// other test here would still pass, because they only ever assert that
    /// the server did **not** write.
    #[test]
    fn a_handler_can_write_through_the_host_and_the_world_changes() {
        struct Mason;
        impl Plugin for Mason {
            fn manifest(&self) -> Manifest {
                use grit_plugin_api::capability::well_known as cap;
                Manifest::new(PluginId::parse("grit:mason").expect("literal"), API_VERSION)
                    .requires(
                        CapabilitySet::from_strs([cap::EVENT_BLOCK_BREAK, cap::WORLD_WRITE])
                            .expect("literals"),
                    )
            }
            fn enable(&mut self, setup: &mut Setup<'_>) -> Result<(), PluginError> {
                setup.events().on::<BlockBreak, _>(
                    Priority::Normal,
                    |ev: &BlockBreak, _v: &mut Verdict, c: &mut Ctx<'_>| {
                        // **Eighteen blocks up, not one**, and the distance is
                        // the whole of what makes the broadcast assertion
                        // below able to fail. `ChangedBlocks` coalesces per
                        // 16-block section, so a write inside the dig's own
                        // section is flushed in the same packet as the dig and
                        // is indistinguishable from it. The falsification
                        // sweep found exactly that: with the handler writing
                        // to (2,3,2), deleting `changed.record` from
                        // `plugins::Host::write` left this test green, because
                        // the dig's own removal had already put a block_update
                        // on the wire.
                        let above = ApiPos::new(ev.pos.x, ev.pos.y + 18, ev.pos.z);
                        // `grit:world.write` is in the manifest above, so the
                        // runtime negotiation check cannot refuse this. An
                        // `expect` naming the manifest row is the honest
                        // handling of a branch the loader has ruled out.
                        let w = c.world().expect("grit:world.write is declared");
                        let dirt = w.resolve("minecraft:dirt").expect("known state");
                        w.set_block(above, dirt);
                    },
                )?;
                Ok(())
            }
        }

        let world = EditsWorld::new();
        stone_at(&world, &[(2, 2, 2)]);
        let rt = Runtime::install(vec![Box::new(Mason)]);
        let sent = drive(&dig(START, (2, 2, 2), 1), &world, Some(&rt), None);
        assert_eq!(
            Blocks::state_at(&world, (2, 2, 2)),
            "minecraft:air",
            "the dig was not denied"
        );
        assert_eq!(
            Blocks::state_at(&world, (2, 20, 2)),
            "minecraft:dirt",
            "the handler's own write reached the server's world"
        );
        // **And a client was told about that position specifically.**
        //
        // Without `changed.record` in `plugins::Host::write` a plugin's edits
        // land in the world and no client is told until the chunk is
        // reloaded. The assertion names the packet *body* rather than its id
        // because the dig's own removal also puts a `block_update` on the
        // wire, so "some block_update was sent" is satisfied by the dig
        // alone — which is the weaker form the falsification sweep caught.
        let expected = crate::interact::block_update_body(
            (2, 20, 2),
            world.state_id("minecraft:dirt").expect("known state"),
        )
        .expect("buf");
        assert!(
            sent.iter()
                .any(|(id, body)| *id == ids::play::cb::BLOCK_UPDATE && *body == expected),
            "the handler's own write was broadcast, not just stored: {:?}",
            sent.iter().map(|(id, _)| *id).collect::<Vec<_>>()
        );
    }

    // `a_handler_runs_with_the_tick_lock_held` used to live here and now lives
    // in `tests/plugin_tick_lock.rs`, in a test binary of its own.
    //
    // **It was flaky here and the falsification sweep is what found it.** The
    // tick lock is process-wide and `cargo test` runs this module's tests on
    // parallel threads, so "is the tick lock held while my handler runs" was
    // being answered `true` by whichever *other* test happened to be mid-
    // handler at that instant. Deleting the `tick_lock()` line from
    // `handle_player_action` (sweep M13) was caught on one run and missed on
    // the next: the right answer arriving from the wrong mechanism.

    /// The event carries the state that is about to be removed, not the air
    /// that replaces it. A handler branching on the state would otherwise
    /// branch on the wrong one.
    #[test]
    fn the_break_event_carries_the_state_being_removed() {
        struct Watcher(Arc<Mutex<Vec<String>>>);
        impl Plugin for Watcher {
            fn manifest(&self) -> Manifest {
                use grit_plugin_api::capability::well_known as cap;
                // **`grit:world.read` was missing here**, and the handler
                // below has always called `world_ref()`. The one-directional
                // check could not see it: the host publishes the capability,
                // so the call worked, and nothing compared the *use* against
                // the declaration. Now it does, and this manifest had to
                // grow a row — found in merged code on the check's first run.
                Manifest::new(
                    PluginId::parse("grit:watcher").expect("literal"),
                    API_VERSION,
                )
                .requires(
                    CapabilitySet::from_strs([cap::EVENT_BLOCK_BREAK, cap::WORLD_READ])
                        .expect("literals"),
                )
            }
            fn enable(&mut self, setup: &mut Setup<'_>) -> Result<(), PluginError> {
                let seen = Arc::clone(&self.0);
                setup.events().on::<BlockBreak, _>(
                    Priority::Normal,
                    move |ev: &BlockBreak, _v: &mut Verdict, c: &mut Ctx<'_>| {
                        let text = c
                            .world_ref()
                            .expect("grit:world.read is declared")
                            .text(ev.state)
                            .map(|s| s.as_str().to_owned())
                            .unwrap_or_default();
                        seen.lock().expect("seen").push(text);
                    },
                )?;
                Ok(())
            }
        }

        let seen = Arc::new(Mutex::new(Vec::new()));
        let world = EditsWorld::new();
        stone_at(&world, &[(2, 2, 2)]);
        let rt = Runtime::install(vec![Box::new(Watcher(Arc::clone(&seen)))]);
        drive(&dig(START, (2, 2, 2), 1), &world, Some(&rt), None);
        assert_eq!(
            *seen.lock().expect("seen"),
            vec!["minecraft:stone".to_owned()],
            "the handler saw the block that was about to go"
        );
    }
}

/// **The `canSurvive` gate's wiring**, which is a different question from
/// whether the rule is right.
///
/// `crates/rustmc-behavior/tests/golden_item_survive.rs` settles the rules
/// against 640,532 of vanilla's own answers. Nothing there says this server
/// ever *calls* them, and the failure mode of a correct rule nobody asks is
/// exactly the shape this project keeps finding: a green that measures the
/// rule and not the placement. These three tests are about the call.
#[cfg(test)]
mod survive_gate_tests {
    use super::*;

    fn repo(name: &str) -> std::path::PathBuf {
        let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.pop();
        path.pop();
        path.push("data/vanilla");
        path.push(name);
        path
    }

    /// A world of air with one block placed by the test, and a real
    /// `block_props` table behind it.
    struct Ground {
        props: Option<rustmc_behavior::block_props::BlockPropsTable>,
        under: Option<String>,
    }

    impl crate::edits::Blocks for Ground {
        fn state_at(&self, pos: crate::interact::BlockPos) -> String {
            match (&self.under, pos) {
                (Some(state), (0, -1, 0)) => state.clone(),
                _ => "minecraft:air".to_string(),
            }
        }
        fn set_block(&self, _pos: crate::interact::BlockPos, _state: &str) -> Option<i32> {
            None
        }
        fn replaceable(
            &self,
            _pos: crate::interact::BlockPos,
            _ctx: &crate::placement::Replacing<'_>,
        ) -> bool {
            true
        }
        fn legacy_fluid_at(&self, _pos: crate::interact::BlockPos) -> String {
            "minecraft:air".to_string()
        }
        fn default_state(&self, block: &str) -> Option<String> {
            Some(block.to_string())
        }
        fn has_state(&self, _state: &str) -> bool {
            true
        }
        fn state_id(&self, _state: &str) -> Option<i32> {
            None
        }
        fn face_sturdy(
            &self,
            _pos: crate::interact::BlockPos,
            _face: crate::interact::Face,
        ) -> bool {
            false
        }
        fn redstone_conductor(&self, _pos: crate::interact::BlockPos) -> bool {
            false
        }
        fn signal_source(&self, _pos: crate::interact::BlockPos) -> bool {
            false
        }
        fn water_at(&self, _pos: crate::interact::BlockPos) -> bool {
            false
        }
        fn place_props(&self) -> Option<&rustmc_behavior::block_props::BlockPropsTable> {
            self.props.as_ref()
        }
    }

    fn tables() -> Option<(
        rustmc_behavior::block_props::BlockPropsTable,
        crate::itemsurvive::SurviveClasses,
    )> {
        let props =
            rustmc_behavior::block_props::BlockPropsTable::load(&repo("block_props.json")).ok()?;
        let classes = crate::itemsurvive::SurviveClasses::load(&repo("item_survive.json")).ok()?;
        Some((props, classes))
    }

    /// The gate is reached and its answer is used: a torch over air is
    /// refused and the same torch over stone is not.
    ///
    /// Both halves, because a gate that refused *everything* would pass the
    /// first assertion on its own — which is what a one-sided test of a
    /// refusal always misses.
    #[test]
    fn a_torch_over_air_is_refused_and_one_over_stone_is_not() {
        let Some((props, classes)) = tables() else {
            return;
        };
        let over_air = Ground {
            props: Some(props),
            under: None,
        };
        assert!(
            !gate(&over_air, &classes, "minecraft:torch"),
            "BlockItem.canPlace refuses a torch whose support is air"
        );
        let props = rustmc_behavior::block_props::BlockPropsTable::load(&repo("block_props.json"))
            .expect("block_props");
        let over_stone = Ground {
            props: Some(props),
            under: Some("minecraft:stone".to_string()),
        };
        assert!(
            gate(&over_stone, &classes, "minecraft:torch"),
            "and allows one over stone — without this half a gate that refused \
             everything would read as correct"
        );
    }

    /// A block with no rule is never refused, over air or anything else.
    /// 804 of the 1,115 are in this arm and a gate that caught them would
    /// stop the game.
    #[test]
    fn a_block_with_no_rule_is_placed_on_nothing() {
        let Some((props, classes)) = tables() else {
            return;
        };
        let over_air = Ground {
            props: Some(props),
            under: None,
        };
        assert!(gate(&over_air, &classes, "minecraft:stone"));
    }

    /// **The fail-open default.** A world with no `block_props` table asks
    /// no gate at all, which is what every fake world in this crate's tests
    /// gets and what this server did before the gate existed. Without this,
    /// adding the gate would silently have refused every torch, sign and
    /// rail in nine test fixtures.
    #[test]
    fn a_world_without_the_table_is_not_gated() {
        let Some((_, classes)) = tables() else {
            return;
        };
        let no_table = Ground {
            props: None,
            under: None,
        };
        assert!(gate(&no_table, &classes, "minecraft:torch"));
    }

    /// The gate without a `Session` around it: the same three arguments
    /// `place_survive_gate` assembles, so these tests exercise the rule
    /// lookup and the world adapter rather than a re-implementation.
    fn gate(
        blocks: &dyn crate::edits::Blocks,
        classes: &crate::itemsurvive::SurviveClasses,
        state: &str,
    ) -> bool {
        let Some(props) = blocks.place_props() else {
            return true;
        };
        let Some(rule) = classes.rule(state) else {
            return true;
        };
        let world = GateWorld { blocks };
        let n = rustmc_behavior::place_survive::Neighbourhood::new(&world, props, (0, 0, 0));
        rustmc_behavior::place_survive::vanilla_would_place(
            &n,
            state,
            rule.can_survive.as_deref(),
            rule.may_place_on.as_deref(),
        )
    }
}

/// **`minecraft:use_item` reaches the loop**, which is a different claim from
/// the decoder being right.
///
/// `crate::interact::use_item_tests` settles the body. Nothing there says the
/// frame loop ever dispatches id 67 — and a decoder that is never called is
/// the shape this project keeps finding. These tests drive real frames.
#[cfg(test)]
mod use_item_loop_tests {
    use super::dig_tests::{use_item, FakeWorld};
    use super::*;

    /// The ack is unconditional and above the empty-hand return, so a
    /// right-click into the air with nothing held still acknowledges the
    /// client's block-change prediction.
    ///
    /// This is the packet's whole observable answer on this server today, and
    /// it is not nothing: without it a client that mispredicted a block and
    /// then right-clicked air is never corrected.
    #[test]
    fn a_right_click_on_air_is_acknowledged() {
        let world = FakeWorld::default();
        let (sent, _) = super::dig_tests::play(&use_item(21, 90.0, 0.0), true, &world);
        let acks: Vec<&(i32, Vec<u8>)> = sent
            .iter()
            .filter(|(id, _)| *id == ids::play::cb::BLOCK_CHANGED_ACK)
            .collect();
        assert_eq!(acks.len(), 1, "handleUseItem offsets 20-25, unconditional");
        assert_eq!(
            acks[0].1,
            crate::interact::block_changed_ack_body(21).expect("buf"),
            "with the sequence the client sent"
        );
    }

    /// **The negative control for the assertion above.** Without it, a loop
    /// that acknowledged every frame it did not recognise would pass — which
    /// is exactly what this server did before id 67 was dispatched, and would
    /// make the test above measure nothing.
    #[test]
    fn an_unknown_packet_is_not_acknowledged() {
        let mut frame = Vec::new();
        let mut body = Vec::new();
        write_varint(&mut body, 0).expect("buf");
        write_varint(&mut body, 21).expect("buf");
        wire::write_f32(&mut body, 90.0).expect("buf");
        wire::write_f32(&mut body, 0.0).expect("buf");
        // An id no `sb` constant uses. If this ever becomes a real packet the
        // test fails loudly, which is the right outcome.
        frame::write_frame(&mut frame, None, 120, &body).expect("buf");
        let world = FakeWorld::default();
        let (sent, _) = super::dig_tests::play(&frame, true, &world);
        assert!(
            !sent
                .iter()
                .any(|(id, _)| *id == ids::play::cb::BLOCK_CHANGED_ACK),
            "an unrecognised frame acknowledges nothing — so the ack above is id 67's"
        );
    }

    /// The arm is reached and named. `Pass` is what an empty hand and every
    /// block item come to, and counting it is what makes the other four
    /// meaningful as a ratio rather than a bare tally.
    #[test]
    fn the_use_arm_is_reached_and_counted() {
        let before = crate::itemprops::use_arms_reached(crate::itemprops::UseArm::Pass);
        let world = FakeWorld::default();
        // Held is set through `play_tables`; `play` leaves the hand empty,
        // which returns at offsets 58-78 *before* the arm — so this drives
        // the held path instead.
        let (_, _) = super::dig_tests::play_tables(
            &use_item(22, 0.0, 0.0),
            true,
            &world,
            None,
            None,
            Some("minecraft:stone"),
            None,
            None,
            None,
        );
        let after = crate::itemprops::use_arms_reached(crate::itemprops::UseArm::Pass);
        assert!(
            after > before,
            "the Pass arm was not reached: {before} -> {after}. The counter is \
             process-wide and monotonic and only this module's tests move it, so an \
             increase here is this frame's."
        );
    }
}
