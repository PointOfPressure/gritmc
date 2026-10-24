//! The plugin host: what makes `grit-plugin-api` live inside this server.
//!
//! # What this is, and what it deliberately is not
//!
//! `grit-plugin-api` is the contract, and it lives in the `grit-plugins`
//! repo beside this one. This module is the *host* side of it: the traits
//! `grit_plugin_api::host` asks a server to implement, implemented over this
//! server's own block layer, player registry and clock, plus the one thing
//! that turns the contract from satisfiable into live — a plugin actually
//! being asked, from the production packet handler, before a block is
//! written.
//!
//! It is **not** a fourth frozen interface. This project has three already —
//! `rustmc_api::{Plugin, Server}`, [`crate::Scheduler`] and
//! [`crate::RegionMessage`] — and all three were frozen before anything ever
//! called them. The test that distinguishes this module from those is written
//! down so it can be applied: `world_send::handle_player_action` calls
//! [`block_break_denied`] on every dig, and `handle_use_item_on` calls
//! [`block_place_denied`] on every placement, unconditionally; and
//! `world_send::plugin_hook_tests` drives a real dig packet through the real
//! handler, into a real [`Plugin`] loaded by the real
//! [`grit_plugin_api::load`], and asserts through this server's own
//! [`crate::edits::Blocks`] trait that the block is still there. Remove any
//! link in that chain and the test goes red — which is the property the three
//! frozen interfaces never had.
//!
//! # Zero plugins costs one load and one untaken branch
//!
//! A server with no plugins must behave exactly as it did before this module
//! existed. That is not a promise, it is the shape of the code: the hook
//! entry points open on `s.plugins`, an `Option<&Runtime>` already in the
//! `Session` the handler is holding, and then on a plain `usize` count of
//! registered handlers. Nothing is allocated, no lock is taken, no state
//! string is interned and no event is built until a plugin has actually
//! registered for that event.
//!
//! # The threading rule is satisfied by construction, not by care
//!
//! `docs/CONTRACT.md` §3 promises exactly one plugin callback at any instant,
//! server-wide, and enforces the half of it that can be enforced by making
//! `World` and `Ctx` `!Send + !Sync`. This server can keep the other half
//! without doing anything new: both hook sites already run underneath
//! [`crate::broadcast::tick_lock`], the single `Mutex<()>` this server holds
//! across the whole tick body *and* across every connection-thread packet
//! handler. So a callback runs with the tick lock held, on whichever
//! connection thread read the packet, and never concurrently with another.
//! The `Ctx` is built on that thread's stack and dropped before the handler
//! returns; it is never stored, never moved, and could not be.
//!
//! The `Mutex` around [`Runtime::bus`] is therefore uncontended in practice.
//! It is there because [`EventBus::dispatch`] needs `&mut`, and a `&mut` to a
//! process-shared registry is a thing Rust will not hand out on a promise.
//!
//! # The registry is per-`Session`, not a global
//!
//! [`Runtime`] is a value. Production installs one and hands out
//! `&'static` references to it ([`install_global`], [`global`]); a test builds
//! its own and puts it in its own `Session`. That is deliberate and it is the
//! same lesson `Session::changed` carries: `cargo test` runs a module's tests
//! on parallel threads in one process, and a process-wide plugin registry
//! would let one test's plugin deny another test's dig.

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock, PoisonError};

use grit_plugin_api::capability::well_known as cap;
use grit_plugin_api::event::{BlockBreak, BlockPlace, EventBus};
use grit_plugin_api::host::{
    BatchSummary, BatchToken, HostServices, PlayerBackend, SetOutcome, StateHandle, WorldBackend,
};
use grit_plugin_api::{
    load, BlockPos as ApiPos, CapabilitySet, Ctx, LoadReport, PlayerId, Plugin, PluginId, WorldId,
};

use crate::edits::Blocks;
use crate::interact::BlockPos;

/// The one world this server serves.
///
/// `docs/DECISIONS.md` D12: every position in the API carries a world even
/// though there is only one, because adding a world parameter later changes
/// the type of every position in every signature. The cost here is a refcount
/// bump per event.
fn overworld() -> WorldId {
    static ID: OnceLock<WorldId> = OnceLock::new();
    ID.get_or_init(|| WorldId::parse("minecraft:overworld").expect("literal is a valid id"))
        .clone()
}

/// Exactly the capabilities this server backs with a live call path.
///
/// **This list is the whole honesty mechanism** (`docs/DECISIONS.md` D19):
/// a plugin declares what it cannot work without, and one it declares that is
/// not in here means the plugin does not load and the refusal names the
/// capability. Publishing one whose call path does not exist converts that
/// refusal back into a silent no-op, which is this project's recurring defect
/// wearing a new costume.
///
/// It is the same five `grit-plugins/host-proof` publishes, and that is worth
/// keeping true: the host-proof host and this one are two implementations of
/// the same trait against the same block layer, so a capability one publishes
/// and the other does not is a discrepancy somebody should have to explain.
///
/// **`grit:storage.dir` is a sixth, and it is deliberately not written here.**
/// It is derived from [`Host::storage_root`] by
/// `HostServices::published_capabilities`, which adds it when a root exists
/// and *strips* it when one does not — so a line added here would be removed
/// again, and the capability cannot be published without being backed. The set
/// this function returns is therefore no longer the whole published set;
/// `published_capabilities()` is, and it is what `load` checks against and
/// what the bus is given.
///
/// Deliberately **not** here, though this module now implements the route:
/// `grit:player.message`. [`Host::send_message`] does reach a named online
/// player, so the capability has become backable — but that is a change to
/// the published contract table in `grit-plugins/docs/CONTRACT.md` §4, and
/// this repo does not get to make it by adding a line here.
fn capabilities() -> &'static CapabilitySet {
    static CAPS: OnceLock<CapabilitySet> = OnceLock::new();
    CAPS.get_or_init(|| {
        CapabilitySet::from_strs([
            cap::WORLD_READ,
            cap::WORLD_WRITE,
            cap::WORLD_BULK_WRITE,
            cap::EVENT_BLOCK_BREAK,
            cap::EVENT_BLOCK_PLACE,
        ])
        .expect("literal capabilities")
    })
}

// ---------------------------------------------------------------------------
// The block-state interner.
// ---------------------------------------------------------------------------

/// Block states, interned once and named by index thereafter.
///
/// `grit_plugin_api::host` places this cost on the host on purpose: a
/// world-editing plugin reads millions of blocks in one command, and an
/// allocation per read makes that impossible at any API design. The set is
/// bounded — the block-state table is tens of thousands of entries — so this
/// is a fixed cost and not a leak, which is exactly why the strings may be
/// leaked to get a `&'static str` out the other side.
///
/// Only states this server's own block table admits are ever interned:
/// [`Interner::resolve`] asks [`Blocks::has_state`] first, so a plugin cannot
/// grow this table by asking about nonsense.
///
/// A `BTreeMap` rather than a `HashMap`, for the reason `crate::edits` gives
/// for the same choice: nothing iterates it into anything observable today,
/// and a hash order would make that a defect the day something does.
#[derive(Default)]
struct Interner {
    inner: Mutex<InternerInner>,
}

#[derive(Default)]
struct InternerInner {
    by_text: BTreeMap<&'static str, u32>,
    by_index: Vec<&'static str>,
}

impl Interner {
    /// Intern a state string, minting a handle the first time it is seen.
    fn intern(&self, state: &str) -> StateHandle {
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(i) = inner.by_text.get(state) {
            return StateHandle::from_host_index(*i);
        }
        let leaked: &'static str = Box::leak(state.to_owned().into_boxed_str());
        let i = u32::try_from(inner.by_index.len()).unwrap_or(u32::MAX);
        inner.by_index.push(leaked);
        inner.by_text.insert(leaked, i);
        StateHandle::from_host_index(i)
    }

    /// The state behind a handle, or `None` for a handle this host never
    /// issued.
    fn text(&self, handle: StateHandle) -> Option<&'static str> {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .by_index
            .get(handle.host_index() as usize)
            .copied()
    }
}

/// The process's interner.
///
/// Process-wide rather than per-[`Runtime`] because a [`StateHandle`] is
/// documented as process-local, and two runtimes in one process issuing
/// different handles for the same state would make that sentence false. It is
/// created on first use, which — since the only things that intern are the
/// hook entry points and the world methods a handler calls — means a server
/// with no plugins never builds one.
fn interner() -> &'static Interner {
    static INTERNER: OnceLock<Interner> = OnceLock::new();
    INTERNER.get_or_init(Interner::default)
}

// ---------------------------------------------------------------------------
// The host.
// ---------------------------------------------------------------------------

const fn to_api(pos: BlockPos) -> ApiPos {
    ApiPos::new(pos.0, pos.1, pos.2)
}

const fn to_server(pos: ApiPos) -> BlockPos {
    (pos.x, pos.y, pos.z)
}

/// This server, wearing `grit_plugin_api::host`'s traits, for the duration of
/// one dispatch.
///
/// Built on the stack of the connection thread that read the packet and
/// dropped before the handler returns. Every field is a borrow of something
/// the [`crate::world_send::Session`] already holds, so nothing here can
/// outlive the handler and there is no lifetime for a plugin to capture — see
/// the module docs on threading.
///
/// The `Option`s are what makes one type serve both the dispatch site and
/// [`Runtime::install`], which needs a `&dyn HostServices` before any player
/// exists. A detached host answers "no world, nobody online", which is the
/// truth at boot; nothing in `Plugin::enable` may read the world, because
/// `Setup` is all it is given.
pub struct Host<'a> {
    blocks: Option<&'a dyn Blocks>,
    shapes: Option<&'a crate::shapes::ShapeTable>,
    changed: Option<&'a crate::broadcast::ChangedBlocks>,
    players: Option<&'a crate::players::Players>,
    tick: u64,
    world: WorldId,
    batches: Mutex<BTreeMap<u64, BatchSummary>>,
    next_batch: Mutex<u64>,
}

impl<'a> Host<'a> {
    /// A host with no world and no players, for load time.
    #[must_use]
    pub fn detached() -> Self {
        Self {
            blocks: None,
            shapes: None,
            changed: None,
            players: None,
            tick: 0,
            world: overworld(),
            batches: Mutex::new(BTreeMap::new()),
            next_batch: Mutex::new(0),
        }
    }

    /// A host over one connection's session.
    #[must_use]
    pub fn for_session(s: &'a crate::world_send::Session<'_>) -> Self {
        Self {
            blocks: s.blocks,
            shapes: s.shapes,
            changed: Some(s.changed),
            players: Some(s.players),
            // The clock counts in `i64` and the API in `u64`. A negative tick
            // is not a thing this server produces, so the saturating case is
            // unreachable rather than lossy.
            tick: u64::try_from(s.clock.ticks()).unwrap_or(0),
            world: overworld(),
            batches: Mutex::new(BTreeMap::new()),
            next_batch: Mutex::new(0),
        }
    }

    /// The world this host serves.
    #[must_use]
    pub fn world_id(&self) -> WorldId {
        self.world.clone()
    }

    /// **The real server write**, and the reason a plugin's edit is not a
    /// second-class one.
    ///
    /// `crate::shapes::set_block` is `Level.setBlock(pos, state, flags,
    /// recursionLeft)` — the same function `handle_player_action` and
    /// `handle_use_item_on` call. Everything it cascades runs here exactly as
    /// it does for a player's dig: `onPlace`, the rail shape cascade, the
    /// redstone neighbour pass, the shape update.
    ///
    /// The positions it knocked down are recorded into the session's
    /// [`crate::broadcast::ChangedBlocks`], which is what the tick boundary
    /// flushes. Without that a plugin's write would land in the world and no
    /// client would ever be told — visible only to the next player to load
    /// the chunk, which is the shape of bug that gets blamed on the network
    /// for a week.
    fn write(&self, pos: BlockPos, state: &str) -> SetOutcome {
        let Some(blocks) = self.blocks else {
            return SetOutcome::OutOfBounds;
        };
        if !blocks.has_state(state) {
            return SetOutcome::UnknownState;
        }
        let mut updates = crate::shapes::Effects::new();
        let wrote = crate::shapes::set_block(
            blocks,
            self.shapes,
            pos,
            state,
            crate::shapes::UPDATE_ALL,
            crate::shapes::RECURSION_LIMIT,
            &mut updates,
        );
        if let Some(changed) = self.changed {
            for &(p, _) in &updates.blocks {
                changed.record(p);
            }
        }
        // A plugin's write can cascade into a `Level.destroyBlock` exactly as
        // a player's can, and the level events it produces have nowhere to go
        // from here: `Ctx` holds no connection registry, by design — see
        // `grit-plugins/docs/CONTRACT.md` §3 on what a plugin may reach. Named
        // rather than dropped silently; the first plugin that knocks a torch
        // off a wall will be one `level_event` short on every client, and this
        // comment is where that is written down.
        let _unsent = &updates.level_events;
        if wrote {
            SetOutcome::Changed
        } else {
            SetOutcome::Unchanged
        }
    }

    /// The interned state at a position, for building an event.
    fn handle_at(&self, pos: BlockPos) -> Option<StateHandle> {
        let blocks = self.blocks?;
        Some(interner().intern(&blocks.state_at(pos)))
    }
}

impl WorldBackend for Host<'_> {
    fn worlds(&self) -> Vec<WorldId> {
        vec![self.world.clone()]
    }

    fn state_at(&self, _world: &WorldId, pos: ApiPos) -> Option<StateHandle> {
        self.handle_at(to_server(pos))
    }

    fn state_text(&self, handle: StateHandle) -> Option<&str> {
        interner().text(handle)
    }

    fn resolve_state(&self, state: &str) -> Option<StateHandle> {
        let blocks = self.blocks?;
        // `has_state` first, and it is the guard that keeps the table
        // bounded: without it a plugin asking about a typo in a loop would
        // leak a string per call.
        blocks.has_state(state).then(|| interner().intern(state))
    }

    fn default_state(&self, block: &str) -> Option<StateHandle> {
        let blocks = self.blocks?;
        blocks.default_state(block).map(|s| interner().intern(&s))
    }

    fn set_block(&self, _world: &WorldId, pos: ApiPos, state: StateHandle) -> SetOutcome {
        let Some(text) = interner().text(state) else {
            return SetOutcome::UnknownState;
        };
        self.write(to_server(pos), text)
    }

    fn batch_begin(&self, _world: &WorldId) -> Option<BatchToken> {
        self.blocks?;
        let mut n = self.next_batch.lock().ok()?;
        let token = *n;
        *n += 1;
        self.batches
            .lock()
            .ok()?
            .insert(token, BatchSummary::default());
        Some(BatchToken::from_host_index(token))
    }

    fn batch_set(&self, token: BatchToken, pos: ApiPos, state: StateHandle) -> SetOutcome {
        let out = self.set_block(&self.world, pos, state);
        if let Ok(mut b) = self.batches.lock() {
            if let Some(s) = b.get_mut(&token.host_index()) {
                match out {
                    SetOutcome::Changed => s.changed += 1,
                    SetOutcome::Unchanged => s.unchanged += 1,
                    _ => s.refused += 1,
                }
            }
        }
        out
    }

    fn batch_commit(&self, token: BatchToken) -> BatchSummary {
        // Per-block client packets are already coalesced by
        // `ChangedBlocks`, which is a per-section set drained at the tick
        // boundary — so the bracket has nothing further to defer, and
        // committing is closing the tally. Said rather than left implied:
        // the coalescing `docs/DECISIONS.md` D8 asks a host for is the
        // broadcast one, and this server has always done it that way.
        self.batches
            .lock()
            .ok()
            .and_then(|mut b| b.remove(&token.host_index()))
            .unwrap_or_default()
    }
}

impl PlayerBackend for Host<'_> {
    fn online(&self) -> Vec<PlayerId> {
        self.players
            .map(|p| {
                p.connections()
                    .into_iter()
                    .map(|c| player_id(c.name()))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn name_of(&self, player: PlayerId) -> Option<String> {
        self.players?
            .connections()
            .into_iter()
            .find(|c| player_id(c.name()) == player)
            .map(|c| c.name().to_owned())
    }

    fn send_message(&self, player: PlayerId, text: &str) -> bool {
        let Some(players) = self.players else {
            return false;
        };
        let Some(conn) = players
            .connections()
            .into_iter()
            .find(|c| player_id(c.name()) == player)
        else {
            return false;
        };
        let Ok(body) = crate::commands::plain_text_body(text) else {
            return false;
        };
        conn.send(rustmc_proto::ids::play::cb::SYSTEM_CHAT, &body)
            .is_ok()
    }
}

impl HostServices for Host<'_> {
    fn tick(&self) -> u64 {
        self.tick
    }

    fn monotonic_nanos(&self) -> u64 {
        static START: OnceLock<std::time::Instant> = OnceLock::new();
        u64::try_from(
            START
                .get_or_init(std::time::Instant::now)
                .elapsed()
                .as_nanos(),
        )
        .unwrap_or(u64::MAX)
    }

    fn capabilities(&self) -> &CapabilitySet {
        capabilities()
    }

    /// Where plugins keep their files: `<world>/plugins`.
    ///
    /// # This is the whole of what this server says about storage
    ///
    /// `grit:storage.dir` is **derived** from this by
    /// `HostServices::published_capabilities` — it is deliberately absent from
    /// [`capabilities`] above, and writing it there by hand would be stripped
    /// straight back out. So the capability cannot be published without being
    /// backed, or backed without being published, which is what the previous
    /// shape (a hand-written capability beside an independent `storage_dir`)
    /// could not promise. The per-plugin subdirectory beneath this —
    /// `<world>/plugins/grit/protect` — is derived and created by
    /// `grit_plugin_api::plugin::load`, so every host lays out the same tree.
    ///
    /// # `None` when the operator did not ask for persistence
    ///
    /// Rooted under the operator's own `--world` directory rather than under a
    /// second flag, because they are the same decision. A server run without
    /// `--world` persists no chunks, and it would be dishonest for it to
    /// persist a plugin's claims — so it publishes no storage capability, and
    /// `grit:protect` is refused **by name** on such a server rather than
    /// loading and quietly losing every claim at the next restart. That is
    /// `docs/DECISIONS.md` D19 applied to this server's own configuration and
    /// not only to its feature set.
    fn storage_root(&self) -> Option<std::path::PathBuf> {
        // The sabotage arm answers `Some` unconditionally — the shape of this
        // method before the capability gate above was written, when a server
        // with no `--world` published `grit:storage.dir` and lost every claim
        // at the next restart. `rustmc_data::sabotage`.
        if rustmc_data::sabotage::fires(rustmc_data::sabotage::Arm::Storage) {
            return Some(std::path::PathBuf::from("/nonexistent/sabotage/plugins"));
        }
        crate::tiles::Tiles::configured_world_dir().map(|d| d.join("plugins"))
    }
}

/// A player's identity, as the API defines it: their UUID, never their name
/// and never their entity id.
///
/// `docs/DECISIONS.md` D3 — an entity id is a per-session integer that
/// changes on every join, so a permissions or economy plugin keyed on one
/// loses every record when a player reconnects. This server already computes
/// the offline-mode UUID during login; it simply was not being used as an
/// identity.
fn player_id(name: &str) -> PlayerId {
    PlayerId::from_u128(rustmc_proto::wire::offline_uuid(name))
}

// ---------------------------------------------------------------------------
// The runtime.
// ---------------------------------------------------------------------------

/// The loaded plugins and their handler registry.
///
/// A value, not a global. See the module docs for why.
pub struct Runtime {
    bus: Mutex<EventBus>,
    loaded: Vec<PluginId>,
    refused: Vec<String>,
    /// How many handlers are registered for `BlockBreak`. Read on the dig
    /// path before anything else happens, so a server with plugins that do
    /// not care about breaking pays nothing for them.
    breaks: usize,
    /// The same, for `BlockPlace`.
    places: usize,
}

impl Runtime {
    /// Load a set of plugins against this server.
    ///
    /// Everything about *how* — sorted by plugin id, API version checked,
    /// capabilities checked by name, one refusal never stopping another — is
    /// [`grit_plugin_api::load`]'s and not reimplemented here. A host that
    /// wrote its own loader would eventually get the ordering subtly
    /// different from the one the contract documents.
    #[must_use]
    pub fn install(plugins: Vec<Box<dyn Plugin>>) -> Self {
        let host = Host::detached();
        let mut bus = EventBus::new(capabilities().clone());
        let report: LoadReport = load(plugins, &host, &mut bus);
        let breaks = bus.listeners::<BlockBreak>().len();
        let places = bus.listeners::<BlockPlace>().len();
        Self {
            bus: Mutex::new(bus),
            loaded: report.loaded,
            refused: report.refused.iter().map(ToString::to_string).collect(),
            breaks,
            places,
        }
    }

    /// The plugins that enabled, in the order they were enabled.
    #[must_use]
    pub fn loaded(&self) -> &[PluginId] {
        &self.loaded
    }

    /// The plugins that were refused, each with the reason and the name of
    /// what was missing.
    #[must_use]
    pub fn refused(&self) -> &[String] {
        &self.refused
    }

    /// How many handlers listen for a block break.
    #[must_use]
    pub const fn break_listeners(&self) -> usize {
        self.breaks
    }

    /// How many handlers listen for a block place.
    #[must_use]
    pub const fn place_listeners(&self) -> usize {
        self.places
    }

    /// Dispatch one cancellable event and return the denial reason, if any.
    ///
    /// The lock is recovered rather than propagated on poison. A poisoned bus
    /// means a handler panicked; the registry itself is intact, and the two
    /// alternatives are both worse — failing open silently un-protects every
    /// claim on the server, and failing closed makes the world read-only for
    /// ever because one plugin has a bug.
    fn deny_reason<E: grit_plugin_api::Cancellable>(
        &self,
        event: &E,
        ctx: &mut Ctx<'_>,
    ) -> Option<String> {
        let mut bus = self.bus.lock().unwrap_or_else(PoisonError::into_inner);
        bus.dispatch(event, ctx).reason().map(ToOwned::to_owned)
    }
}

/// The process's plugins, installed once at boot.
static GLOBAL: OnceLock<Runtime> = OnceLock::new();

/// Install the server's plugin set. Called once, from `status::listen`.
///
/// # Panics
/// Never; a second call returns the first runtime rather than replacing it,
/// because two plugin sets in one process would make dispatch order depend on
/// which call won.
pub fn install_global(plugins: Vec<Box<dyn Plugin>>) -> &'static Runtime {
    GLOBAL.get_or_init(|| Runtime::install(plugins))
}

/// The installed plugin set, or `None` if [`install_global`] has not run.
#[must_use]
pub fn global() -> Option<&'static Runtime> {
    GLOBAL.get()
}

/// The plugins compiled into this server.
///
/// **Empty, and that is the honest state of it.** Loading is static
/// (`docs/DECISIONS.md` D14: Rust has no stable ABI, so there is no `dlopen`
/// and no plugin file dropped into a folder), which means adding a plugin is
/// a line here and a rebuild. The first-party plugins live in the
/// `grit-plugins` repo and none is depended on from here yet; when one is,
/// this is where it goes and nothing else about the wiring changes.
///
/// The list being empty does **not** make the loader dead: `install_global`
/// runs on every boot with it, so the load path is exercised by production
/// rather than only by a test, and a server prints how many plugins it
/// loaded rather than leaving the question open.
#[must_use]
pub fn first_party() -> Vec<Box<dyn Plugin>> {
    Vec::new()
}

// ---------------------------------------------------------------------------
// The hooks. These two functions are the whole point of this module.
// ---------------------------------------------------------------------------

/// Ask the plugins whether `player` may break the block at `pos`.
///
/// `Some(reason)` means a handler refused and the block must not be removed;
/// the reason has already been sent to the player, because a protection
/// plugin that silently swallowed the action would be indistinguishable from
/// lag.
///
/// # What it costs a server with no plugins
///
/// One `Option` discriminant test on a field the caller is already holding,
/// and one `usize` compare. No allocation, no lock, no interning, no event.
/// The `state_at` that builds the event is *inside* the branch, which matters
/// because this server's `Blocks::state_at` returns a `String`.
pub fn block_break_denied(s: &crate::world_send::Session<'_>, pos: BlockPos) -> Option<String> {
    let rt = s.plugins?;
    if rt.breaks == 0 {
        return None;
    }
    let host = Host::for_session(s);
    let state = host.handle_at(pos)?;
    let player = player_id(s.conn.name());
    let event = BlockBreak::new(player, host.world_id(), to_api(pos), state);
    let mut ctx = Ctx::new(&host, host.world_id());
    let reason = rt.deny_reason(&event, &mut ctx)?;
    // **The host's own message, and the denial does not depend on it.**
    //
    // `deny_reason` has already returned `Some`, so this function returns
    // `Some(reason)` — denied — whatever happens on this line. That ordering
    // is the point: a refusal here is about whether a chat line reached the
    // player, never about whether the plugin's veto stands.
    //
    // `dispatch` leaves the context unscoped when it returns, so the runtime
    // capability check has no declaration to hold this to — right, because
    // negotiation constrains a plugin against what it asked for and this
    // server is not a plugin. Discarded rather than propagated because a chat
    // line that did not land does not make the block break allowed.
    let _ = ctx.message(player, &reason);
    Some(reason)
}

/// Ask the plugins whether `player` may place `state` at `pos`.
///
/// Same shape as [`block_break_denied`]. Both halves exist because a
/// protection plugin that stopped breaking but not placing would let anyone
/// wall a claim in — and because publishing `grit:event.block_place` with no
/// call path would be exactly the dishonesty capability negotiation exists to
/// prevent.
pub fn block_place_denied(
    s: &crate::world_send::Session<'_>,
    pos: BlockPos,
    state: &str,
) -> Option<String> {
    let rt = s.plugins?;
    if rt.places == 0 {
        return None;
    }
    let host = Host::for_session(s);
    // Interned without a `has_state` check, unlike [`Host::resolve_state`],
    // and the reason is worth writing down because it is what keeps the
    // interner bounded. `state` here is `placement::state_for_placement`'s
    // answer, and that function is handed `Blocks::has_state` as its own
    // predicate — so it can only return a state this server's block table
    // knows. Checking again here would buy nothing except a path where an
    // unrecognised state silently *bypasses* the plugin, which is the worse
    // of the two failures.
    let placing = interner().intern(state);
    let replacing = host.handle_at(pos)?;
    let player = player_id(s.conn.name());
    let event = BlockPlace::new(player, host.world_id(), to_api(pos), placing, replacing);
    let mut ctx = Ctx::new(&host, host.world_id());
    let reason = rt.deny_reason(&event, &mut ctx)?;
    // **The host's own message, and the denial does not depend on it.**
    //
    // `deny_reason` has already returned `Some`, so this function returns
    // `Some(reason)` — denied — whatever happens on this line. That ordering
    // is the point: a refusal here is about whether a chat line reached the
    // player, never about whether the plugin's veto stands.
    //
    // `dispatch` leaves the context unscoped when it returns, so the runtime
    // capability check has no declaration to hold this to — right, because
    // negotiation constrains a plugin against what it asked for and this
    // server is not a plugin. Discarded rather than propagated because a chat
    // line that did not land does not make the block break allowed.
    let _ = ctx.message(player, &reason);
    Some(reason)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The interner is the one place a handle comes from, so a handle that
    /// did not round-trip would make every event's `state` field a lie.
    #[test]
    fn a_handle_round_trips_and_is_stable() {
        let i = Interner::default();
        let a = i.intern("minecraft:stone");
        let b = i.intern("minecraft:stone");
        assert_eq!(a, b, "the same state interns to the same handle");
        assert_eq!(i.text(a), Some("minecraft:stone"));
        let c = i.intern("minecraft:dirt");
        assert_ne!(a, c);
        assert_eq!(i.text(c), Some("minecraft:dirt"));
        assert_eq!(
            i.text(StateHandle::from_host_index(9999)),
            None,
            "a handle this interner never issued has no text"
        );
    }

    /// An empty plugin set still goes through the real loader, and the result
    /// is a runtime that answers zero to both hot-path questions.
    #[test]
    fn no_plugins_means_no_listeners() {
        let rt = Runtime::install(Vec::new());
        assert!(rt.loaded().is_empty());
        assert!(rt.refused().is_empty());
        assert_eq!(rt.break_listeners(), 0);
        assert_eq!(rt.place_listeners(), 0);
    }

    /// The published set is the one the contract's table says this server can
    /// back. A capability added here without a call path is the failure the
    /// whole mechanism exists to prevent, so the list is asserted rather than
    /// left to review.
    #[test]
    fn the_published_capabilities_are_exactly_the_five_with_call_paths() {
        let caps = capabilities();
        for c in [
            cap::WORLD_READ,
            cap::WORLD_WRITE,
            cap::WORLD_BULK_WRITE,
            cap::EVENT_BLOCK_BREAK,
            cap::EVENT_BLOCK_PLACE,
        ] {
            assert!(
                caps.provides(&grit_plugin_api::Capability::parse(c).expect("literal")),
                "{c} is published"
            );
        }
        for c in [
            cap::COMMAND_REGISTER,
            cap::EVENT_PLAYER_CHAT,
            cap::PLAYER_TELEPORT,
            cap::PLAYER_KICK,
            cap::PLAYER_MESSAGE,
            cap::EVENT_PLAYER_MOVE,
            cap::WORLD_MULTIPLE,
            cap::STORAGE_DIR,
            cap::SCHEDULE_TICK,
            cap::INVENTORY_ACCESS,
            cap::EVENT_PLAYER_CONNECT,
        ] {
            assert!(
                !caps.provides(&grit_plugin_api::Capability::parse(c).expect("literal")),
                "{c} has no production call path in this server and must not be published"
            );
        }
    }

    /// A detached host is what `enable` runs against, and it must answer
    /// honestly rather than plausibly: no world, nobody online, no storage.
    #[test]
    fn a_detached_host_has_no_world_and_no_players() {
        let h = Host::detached();
        assert_eq!(h.worlds().len(), 1, "the world exists; its blocks do not");
        assert_eq!(h.state_at(&h.world_id(), ApiPos::new(0, 0, 0)), None);
        assert_eq!(h.resolve_state("minecraft:stone"), None);
        assert!(h.online().is_empty());
        assert!(!h.send_message(player_id("nobody"), "hello"));
        assert_eq!(
            h.batch_begin(&h.world_id()),
            None,
            "no world means no bulk write to open"
        );
        // **The invariant, not the value.** `storage_root` now reads the
        // process-wide world directory, which another test in this binary
        // could have set, so asserting `None` here would be asserting on test
        // order — the exact flakiness the note above
        // `the_break_event_carries_the_state_being_removed` records. What must
        // hold either way is that the capability and the root agree, because
        // they are one fact now rather than two.
        let storage = grit_plugin_api::Capability::parse(cap::STORAGE_DIR).expect("literal");
        assert_eq!(
            h.published_capabilities().provides(&storage),
            h.storage_root().is_some(),
            "grit:storage.dir is published exactly when a root backs it"
        );
        assert!(
            !capabilities().provides(&storage),
            "and it is never hand-written into the static set"
        );
    }

    /// **The one line no behavioural test in this crate can reach.**
    ///
    /// Everything else about the hook is proved by driving real packets
    /// through the real handlers, but the handler reads its runtime out of
    /// `Session::plugins`, and the *production* session is built inside
    /// `status::listen` — behind a `TcpListener`, a login and a
    /// configuration phase. Delete `plugins: crate::plugins::global()` there
    /// and the shipped server is silently plugin-less while every test in
    /// this crate stays green. That is exactly the shape of hole this project
    /// keeps finding, so it gets the only check that can see it.
    ///
    /// **What this does and does not prove**, said plainly because a source
    /// grep dressed up as a behavioural test would be worse than none: it
    /// proves the two lines are *present*. It does not prove they are
    /// reached. Reaching them needs a client driven through login against a
    /// real socket, which is `ci/t2-gameplay.sh`'s job and not a unit test's.
    #[test]
    fn the_production_session_is_wired_to_the_installed_plugins() {
        let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/status.rs"))
            .expect("status.rs is beside this crate's manifest");
        assert!(
            src.contains("crate::plugins::install_global(crate::plugins::first_party())"),
            "status::listen must load the plugin set at boot"
        );
        assert!(
            src.contains("plugins: crate::plugins::global()"),
            "the production Session must carry the loaded plugin set"
        );
    }

    /// The offline UUID is the identity, and it is the one this server
    /// already computes at login rather than a second scheme.
    #[test]
    fn a_player_id_is_the_offline_uuid() {
        assert_eq!(
            player_id("Tester").as_u128(),
            rustmc_proto::wire::offline_uuid("Tester")
        );
        assert_ne!(player_id("Tester"), player_id("Tester2"));
    }
}
