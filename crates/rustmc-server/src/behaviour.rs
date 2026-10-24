//! The per-type behaviour slot on a block entity, and the table of which
//! types vanilla ticks on the **server**.
//!
//! `crate::blockentity` models what a block entity *saves*: forty-nine kinds,
//! their container sizes and their NBT keys. This module models what one
//! *does* between saves, and it exists as a separate file because the two
//! answers have different shapes. Saved state is a table over all forty-nine;
//! ticking state is a small struct that only twelve of them have, and the
//! other thirty-seven must not pay a byte for it.
//!
//! # The table is three-valued, and that is the point
//!
//! `Level.tickBlockEntities` walks a list of `TickingBlockEntity` and every
//! entry on it got there from `EntityBlock.getTicker` returning non-null on
//! the *server*. Thirty-two block classes in the pinned jar override
//! `getTicker`; the rest inherit the interface default's `aconst_null`. So a
//! type is in exactly one of three states, and collapsing the last two into
//! "we do not tick it" is how a gap stops being visible:
//!
//! * [`Ticker::Ported`] — vanilla ticks it on the server and so do we, and
//!   the variant names the vanilla method the port is of.
//! * [`Ticker::Unported`] — **vanilla ticks it on the server and we do not.**
//!   The string names the method we are not running. Fifteen kinds are here.
//! * [`Ticker::None`] — vanilla hands out no server ticker for it at all. A
//!   chest is here, and its `ChestBlockEntity.lidAnimateTick` is the reason
//!   the test below cannot be written against the declared-method column
//!   alone: the method exists, and `ChestBlock.getTicker`'s server arm is
//!   `aconst_null`.
//!
//! # What the golden can and cannot falsify
//!
//! `ci/block-entity-golden.sh` emits, per type, the names of every declared
//! static `(Level, BlockPos, BlockState, T)V` on its class chain and whether
//! its first block's class declares `getTicker` at all. That is a
//! **declaration** walk, for the reason the `updatePacket` column is one:
//! asking `getTicker` for its answer needs a `Level` and there is none in a
//! bootstrapped-only JVM, and the first version of that column asked anyway
//! and came back false for all forty-nine.
//!
//! So the cross-check below can go red on:
//!
//! * a kind we tick whose entity declares no ticker-shaped method and whose
//!   block declares no `getTicker` — a ticker invented out of nothing;
//! * a kind we name a method for that its class does not declare — a
//!   transcription that has drifted or a method renamed by a version bump;
//! * a kind vanilla gives a ticker and we record as [`Ticker::None`] — a gap
//!   that has stopped being written down.
//!
//! It cannot see **which side** a declared ticker is handed to. That residue
//! is closed one row at a time from each block's `getTicker` branch, quoted
//! at the row, and it is why every [`Ticker::None`] row whose class declares
//! a ticker method carries the offset that makes it null on the server.

use crate::blockentity::Kind;

/// Whether vanilla ticks a kind on the server, and whether we do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ticker {
    /// `EntityBlock.getTicker` returns null on the server for every block of
    /// this type.
    None,
    /// Vanilla ticks it and this server does not. The string is the vanilla
    /// method, so a grep for a name finds the gap rather than nothing.
    Unported(&'static str),
    /// Vanilla ticks it and `BlockEntities::tick`
    /// dispatches to a port of the named method.
    Ported(&'static str),
}

impl Ticker {
    /// Whether this kind is on `Level.blockEntityTickers` **in this server**.
    #[must_use]
    pub const fn runs(self) -> bool {
        matches!(self, Ticker::Ported(_))
    }

    /// Whether **vanilla** puts this kind on `blockEntityTickers` server-side.
    #[must_use]
    pub const fn vanilla_ticks(self) -> bool {
        !matches!(self, Ticker::None)
    }

    /// The vanilla method name, for either of the two ticking answers.
    #[must_use]
    pub const fn method(self) -> Option<&'static str> {
        match self {
            Ticker::None => None,
            Ticker::Unported(m) | Ticker::Ported(m) => Some(m),
        }
    }
}

/// Vanilla's server-side ticker for a kind.
///
/// Every row is read off the block class's `getTicker`, which is the only
/// place the answer exists — a `BlockEntityType` knows nothing about whether
/// anything ticks it.
#[must_use]
#[allow(clippy::match_same_arms)]
pub const fn server_ticker(kind: Kind) -> Ticker {
    match kind {
        // ---------------- ported here ----------------
        //
        // `AbstractFurnaceBlock.createFurnaceTicker` — the one ticker in the
        // registry built by a helper rather than inline. It returns null when
        // `level` is not a `ServerLevel` and otherwise a lambda whose body is
        // `AbstractFurnaceBlockEntity.serverTick(ServerLevel, BlockPos,
        // BlockState, AbstractFurnaceBlockEntity)`. `FurnaceBlock`,
        // `SmokerBlock` and `BlastFurnaceBlock` each call it with their own
        // type, which is why three kinds share one method.
        Kind::Furnace | Kind::Smoker | Kind::BlastFurnace => {
            Ticker::Ported("AbstractFurnaceBlockEntity.serverTick")
        }
        // `HopperBlock.getTicker` offset 4: `isClientSide; ifeq 11`, so the
        // *client* falls through to `aconst_null` at 7 and the server takes
        // the branch. Server-only.
        Kind::Hopper => Ticker::Ported("HopperBlockEntity.pushItemsTick"),
        // `CampfireBlock.getTicker` is two nested tests: `ifeq 67` on
        // `level instanceof ServerLevel`, then `LIT`. Lit and on the server
        // is `cookTick`, unlit and on the server is `cooldownTick`, lit on
        // the client is `particleTick`, and unlit on the client is null.
        // `BlockEntities::tick` picks between the two
        // server arms from the state, which is why one row covers both.
        Kind::Campfire => Ticker::Ported("CampfireBlockEntity.cookTick"),
        // `BellBlock.getTicker` offset 8: `ifeq 19`, client gets
        // `clientTick`, server gets `serverTick`. Both sides tick.
        Kind::Bell => Ticker::Ported("BellBlockEntity.serverTick"),
        // `JukeboxBlock.getTicker` has **no** `isClientSide` test at all: it
        // reads `HAS_RECORD` off the state and returns the same
        // `JukeboxBlockEntity.tick` to both sides, or null. A jukebox with no
        // disc is not on the ticker list.
        Kind::Jukebox => Ticker::Ported("JukeboxBlockEntity.tick"),
        // Server-only, same `ifeq`/`aconst_null` shape as the hopper.
        Kind::BrewingStand => Ticker::Ported("BrewingStandBlockEntity.serverTick"),
        // `SpawnerBlock.getTicker` offset 8: `ifeq 19`, both sides.
        Kind::MobSpawner => Ticker::Ported("SpawnerBlockEntity.serverTick"),
        // `BeaconBlock.getTicker` has no branch: one `BeaconBlockEntity.tick`
        // for both sides.
        Kind::Beacon => Ticker::Ported("BeaconBlockEntity.tick"),
        // `ConduitBlock.getTicker` offset 8: `ifeq 19`, both sides.
        Kind::Conduit => Ticker::Ported("ConduitBlockEntity.serverTick"),
        // Server-only, same shape as the hopper.
        Kind::Crafter => Ticker::Ported("CrafterBlockEntity.serverTick"),

        // ---------------- vanilla ticks, we do not ----------------
        //
        // `SignBlock`, `CeilingHangingSignBlock` and `WallHangingSignBlock`
        // all return `SignBlockEntity.tick` with no side test. It is what
        // clears a sign's edit lock after a timeout; nothing this server
        // sends depends on it yet.
        Kind::Sign | Kind::HangingSign => Ticker::Unported("SignBlockEntity.tick"),
        // `ShulkerBoxBlock.getTicker`, no side test. The lid animation is
        // client-visible and the *collision* half is not modelled here.
        Kind::ShulkerBox => Ticker::Unported("ShulkerBoxBlockEntity.tick"),
        // Server-only. Bees are not ported.
        Kind::Beehive => Ticker::Unported("BeehiveBlockEntity.serverTick"),
        // Server-only and gated on a state property.
        Kind::CreakingHeart => Ticker::Unported("CreakingHeartBlockEntity.serverTick"),
        // Server-only. The charge spread needs the game-event dispatch this
        // server has none of — `crate::sculk` carries the frequency table and
        // the comparator arithmetic and says so.
        Kind::SculkCatalyst => Ticker::Unported("SculkCatalystBlockEntity.serverTick"),
        // `SculkSensorBlock`/`CalibratedSculkSensorBlock`/
        // `SculkShriekerBlock` offset 4: `ifne 20`, so the **client** gets
        // null and the server gets `VibrationSystem.Ticker.tick` — an
        // interface default, not a static on the entity, which is why the
        // golden's declared-method column is empty for all three and
        // `blockTicker` is true.
        Kind::SculkSensor | Kind::CalibratedSculkSensor | Kind::SculkShrieker => {
            Ticker::Unported("VibrationSystem.Ticker.tick")
        }
        // `EndGatewayBlock.getTicker` offset 8: `ifeq 19`, server gets
        // `portalTick`.
        Kind::EndGateway => Ticker::Unported("TheEndGatewayBlockEntity.portalTick"),
        // `TrialSpawnerBlock` and `VaultBlock` bind their tickers through
        // captured lambdas rather than named statics, so both report an empty
        // declared-method column with `blockTicker` true.
        Kind::TrialSpawner => Ticker::Unported("TrialSpawnerBlockEntity SERVER_TICKER"),
        Kind::Vault => Ticker::Unported("VaultBlockEntity SERVER_TICKER"),
        // The one ticker declared on the **block** rather than on the entity:
        // `DaylightDetectorBlock.tickEntity`. Its own column is therefore
        // empty and `blockTicker` is true.
        Kind::DaylightDetector => Ticker::Unported("DaylightDetectorBlock.tickEntity"),
        // Static lambdas on `PotentSulfurBlockEntity`, selected by
        // `PotentSulfurBlock.getTicker`'s five-way state test.
        Kind::PotentSulfur => Ticker::Unported("PotentSulfurBlockEntity static tickers"),
        // **`MovingPistonBlock` is not in `net.minecraft.world.level.block`,
        // it is in `…block.piston`, and a sweep of the first package alone
        // reported this type as untickable.** The golden's `blockTicker`
        // column caught it on the first run, which is the whole point of
        // having one: `getTicker` here has no side test at all and hands
        // `PistonMovingBlockEntity.tick` to both sides unconditionally.
        //
        // That method is what finishes a push — it advances `progress`,
        // places the moved state at the destination and removes the moving
        // block entity. `crate::containers::run_piston_event` already
        // delivers the `block_event`, so this server starts a push and never
        // ends one.
        Kind::Piston => Ticker::Unported("PistonMovingBlockEntity.tick"),

        // ---------------- vanilla hands out no server ticker ----------------
        //
        // These three declare `lidAnimateTick` and their blocks DO override
        // `getTicker` — with the server arm `aconst_null` (`ChestBlock`
        // offset 4 `ifeq 23`, `EnderChestBlock` offset 4 `ifeq 22`). They are
        // the reason this table cannot be derived from the declared-method
        // column: the method is there and the server never gets it.
        Kind::Chest | Kind::TrappedChest | Kind::EnderChest => Ticker::None,
        // `EnchantingTableBlock.getTicker` offset 4 `ifeq 22` — client only
        // (`bookAnimationTick`).
        Kind::EnchantingTable => Ticker::None,
        // `AbstractSkullBlock.getTicker` offset 4 `ifeq 72` — the *server*
        // arm is the `aconst_null` at 72 and the client gets `animation`,
        // gated further on the skull being a modelled one.
        Kind::Skull => Ticker::None,
        // Everything else inherits `EntityBlock.getTicker`'s default and is
        // never on the list on either side.
        Kind::Banner
        | Kind::BrushableBlock
        | Kind::CopperGolemStatue
        | Kind::DecoratedPot
        | Kind::Shelf
        | Kind::Dispenser
        | Kind::Dropper
        | Kind::EndPortal
        | Kind::Comparator
        | Kind::StructureBlock
        | Kind::CommandBlock
        | Kind::Barrel
        | Kind::Lectern
        | Kind::Jigsaw
        | Kind::ChiseledBookshelf
        | Kind::TestBlock
        | Kind::TestInstanceBlock => Ticker::None,
    }
}

/// The mutable state one ticking block entity keeps between ticks.
///
/// One variant per [`Ticker::Ported`] kind and no variant for anything else,
/// which is the invariant [`Behaviour::for_kind`] and its test enforce: a
/// state struct nothing ticks is a field that can only ever be wrong.
///
/// **Boxed at the use site, not here.** `BlockEntity` holds
/// `Option<Box<Behaviour>>` for the reason it holds `Option<Box<Sign>>`: this
/// struct is one per ticking block entity and the thirty-seven kinds that
/// never tick must pay one pointer, not the width of the largest variant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Behaviour {
    /// `AbstractFurnaceBlockEntity`'s four counters.
    Furnace(crate::cooking::FurnaceState),
    /// `HopperBlockEntity.cooldownTime` and `tickedGameTime`.
    Hopper(crate::hopper::HopperClock),
    /// `CampfireBlockEntity`'s two four-int arrays.
    Campfire(crate::campfire::CampfireState),
    /// `BellBlockEntity`'s ring counter and shake direction.
    Bell(crate::bell::BellState),
    /// `JukeboxBlockEntity.songPlayer`.
    Jukebox(crate::jukebox::SongPlayer),
    /// `BrewingStandBlockEntity`'s brew timer, fuel and bottle bits.
    Brewing(crate::brewing::BrewingStand),
    /// `BaseSpawner`'s delay.
    Spawner(crate::spawner::Spawner),
    /// `BeaconBlockEntity.levels`, and the last levels broadcast.
    Beacon(BeaconState),
    /// `ConduitBlockEntity.isActive` and its tick counter.
    Conduit(ConduitState),
    /// `CrafterBlockEntity`'s disabled-slot mask and craft timer.
    Crafter(crate::crafter::Crafter),
}

/// `BeaconBlockEntity`'s two persistent numbers.
///
/// A struct rather than a bare `i32` because `lastCheckY` is what makes the
/// pyramid scan resume where it stopped rather than restart, and a scan that
/// restarts every tick is a different cost and a different answer at the
/// moment the pyramid changes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BeaconState {
    /// `levels` — the pyramid height under the beacon, 0..=4.
    pub levels: i32,
    /// `lastCheckY`.
    pub last_check_y: i32,
}

/// `ConduitBlockEntity`'s tick counter and activation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ConduitState {
    /// `tickCount`, which every one of the conduit's four sub-behaviours is
    /// keyed on a different residue of.
    pub tick_count: i32,
    /// `isActive`.
    pub active: bool,
}

impl Behaviour {
    /// The behaviour a freshly placed block entity of `kind` starts with, or
    /// `None` for a kind nothing ticks.
    ///
    /// Every default here is the vanilla constructor's, not a zero: a fresh
    /// spawner's `spawnDelay` is 20 and a fresh hopper's `cooldownTime` is
    /// -1, and the two structs carry those in their own `Default`.
    #[must_use]
    pub fn for_kind(kind: Kind) -> Option<Self> {
        if !server_ticker(kind).runs() {
            return None;
        }
        Some(match kind {
            Kind::Furnace | Kind::Smoker | Kind::BlastFurnace => {
                Behaviour::Furnace(crate::cooking::FurnaceState::default())
            }
            Kind::Hopper => Behaviour::Hopper(crate::hopper::HopperClock {
                // `HopperBlockEntity`'s constructor is `iconst_m1;
                // putfield cooldownTime`, which is the same -1 the fresh
                // save writes as `TransferCooldown`.
                cooldown: -1,
                ..crate::hopper::HopperClock::default()
            }),
            Kind::Campfire => Behaviour::Campfire(crate::campfire::CampfireState::default()),
            Kind::Bell => Behaviour::Bell(crate::bell::BellState::default()),
            Kind::Jukebox => Behaviour::Jukebox(crate::jukebox::SongPlayer::default()),
            Kind::BrewingStand => Behaviour::Brewing(crate::brewing::BrewingStand::default()),
            Kind::MobSpawner => Behaviour::Spawner(crate::spawner::Spawner::default()),
            Kind::Beacon => Behaviour::Beacon(BeaconState::default()),
            Kind::Conduit => Behaviour::Conduit(ConduitState::default()),
            Kind::Crafter => Behaviour::Crafter(crate::crafter::Crafter::default()),
            // Unreachable: `server_ticker(kind).runs()` is the same match.
            // Returning `None` rather than panicking keeps a future
            // `Ticker::Ported` row that forgets this arm as a block entity
            // that does not tick, not as a crashed tick loop.
            _ => return None,
        })
    }
}

// ---------------------------------------------------------------------------
// The pass
// ---------------------------------------------------------------------------

/// The tables a tick needs, none of which it can load itself.
///
/// Every field is optional and every arm treats `None` as "this kind does not
/// tick this tick" rather than as a zero. A furnace with no recipe table
/// would otherwise smelt nothing while reporting that it had — which is the
/// shape of a green that cannot go red, one level down from the checks.
pub struct TickTables<'a> {
    /// The one cooking table, for the furnace family and the campfire.
    pub recipes: Option<&'a crate::recipes::Recipes>,
    /// `FuelValues`, for the furnace family's `burn_duration`.
    pub fuels: Option<&'a crate::fuel::FuelValues>,
    /// `JukeboxSong`'s length, for the tick that stops a finished disc.
    pub songs: Option<&'a crate::jukebox::JukeboxSongs>,
    /// The shape table a comparator's signal read walks through, for the
    /// `setChanged` -> `updateNeighbourForOutputSignal` half of a ticker.
    /// `None` reads every block as a non-conductor, which is what the unit
    /// tests below want and the live tick never passes.
    pub shapes: Option<&'a crate::shapes::ShapeTable>,
}

/// Everything one pass of `Level.tickBlockEntities` wants to put on the wire.
///
/// Collected and returned rather than sent from inside the walk, because the
/// walk holds the block-entity map lock one position at a time and a send
/// inside it would hold it across a socket write.
#[derive(Debug, Default)]
pub struct TickEffects {
    /// Positions whose **block state** a ticker rewrote. The caller records
    /// them on `crate::broadcast::ChangedBlocks`; the write itself has
    /// already happened through `Blocks::set_block`.
    pub blocks: Vec<crate::interact::BlockPos>,
    /// `level_event(id, pos, data)`, in the order they were produced.
    pub level_events: Vec<(i32, crate::interact::BlockPos, i32)>,
    /// Sounds, for the tickers that play one.
    pub sounds: Vec<crate::sounds::Sound>,
    /// Stacks a ticker dropped into the world, at the block's own position.
    pub drops: Vec<(crate::interact::BlockPos, crate::item::ItemStack)>,
    /// Positions whose block entity was `setChanged`, for the save path.
    pub changed: Vec<crate::interact::BlockPos>,
    /// `Level.scheduleTick` requests the walk's `setChanged` calls made — a
    /// comparator beside a container that moved, through
    /// `Level.updateNeighbourForOutputSignal`. The caller drains them with
    /// this tick's game time, as every other cascade does.
    pub ticks: Vec<crate::shapes::TickRequest>,
    /// Item entities a hopper absorbed whole — `ItemEntity.discard()` at
    /// `HopperBlockEntity.addItem(Container, ItemEntity)` offset 37. The
    /// caller removes them from the tracker, which is what puts the
    /// `remove_entities` on the wire.
    pub discarded: Vec<i32>,
}

/// `Level.tickBlockEntities` — offsets 0-126.
///
/// # The three gates, in vanilla's order
///
/// 1. `pendingBlockEntityTickers` is appended and cleared, and dead entries
///    are dropped. Both are `BlockEntities::ticker_list`.
/// 2. `shouldTickBlocksAt(pos)` — vanilla's simulation-distance test, which
///    is `DistanceManager.inBlockTickingRange` over a ticket level this
///    server has no equivalent of. `ticking_chunks` stands in for it and is
///    built the same way `crate::randomtick` builds its own disc, so the two
///    passes agree about which chunks are simulated rather than each having
///    an opinion.
/// 3. `BlockEntityType.isValid(state)` — `BoundTickingBlockEntity.tick`
///    offset 72. A block entity under a state its type does not accept is
///    **not** ticked and is not removed either; vanilla logs once and carries
///    on. That is the case a furnace whose block was replaced by a chest is
///    in for exactly one tick, and skipping it is what stops a furnace ticker
///    running against a chest's three-slot list.
///
/// # Where this sits in the tick, which is not where it was expected to
///
/// `ServerLevel.tick` calls `tickBlockEntities()` **after**
/// `ServerChunkCache.tick` and after `runBlockEvents()`, not before them —
/// checked against the bytecode rather than taken from the note that said
/// otherwise. The order of the call sites is `LevelTicks.tick` (fluid, then
/// block), `Raids.tick`, `ServerChunkCache.tick`, `runBlockEvents`,
/// `EnderDragonFight.tick`, `EntityTickList.forEach`, `tickBlockEntities`.
///
/// That is not bookkeeping: the block broadcast lives inside
/// `ServerChunkCache.tick`, so a block a ticker rewrites this tick is
/// broadcast on the **next** one. A furnace lighting at tick N sends its
/// `lit` block update at N+1, and a pass placed before the flush would send
/// it at N. The caller therefore runs this after `changed.flush`.
#[must_use]
pub fn tick_block_entities(
    store: &crate::blockentity::BlockEntities,
    world: &dyn crate::edits::Blocks,
    ticking_chunks: &std::collections::BTreeSet<(i32, i32)>,
    tables: &TickTables<'_>,
    now: i64,
) -> TickEffects {
    let mut out = TickEffects::default();
    for pos in store.ticker_list() {
        if !ticking_chunks.contains(&(pos.0 >> 4, pos.2 >> 4)) {
            continue;
        }
        let state = world.state_at(pos);
        let valid_for = crate::blockentity::for_state(&state);
        // `BlockEntity.setChanged(level, pos, state)` runs inside the ticker
        // and ends in `Level.updateNeighbourForOutputSignal`; the positions a
        // ticker marked from here on are notified once it returns, before the
        // next entity ticks, so a comparator reads the container as this
        // ticker left it and not as a later one in the same pass did.
        let marked = out.changed.len();
        if valid_for == Some(Kind::Hopper) {
            hopper_tick(store, pos, &state, world, tables, now, &mut out);
            notify_output_signal(world, tables.shapes, marked, &mut out);
            continue;
        }
        // The ticker's `Level.setBlock` is collected under the block entity's
        // lock and applied after it is released. `World::set_block` ends in
        // `BlockEntities::on_block_changed`, which takes this same store's
        // write lock, so writing from inside `with_mut` re-entered an
        // `RwLock` on one thread: the tick thread hung the first time a
        // furnace lit or a jukebox stopped, and every tick-driven packet
        // (block acks included) stopped with it. Nothing reads the world
        // between the write and the end of the ticker, so the order a
        // client sees is unchanged.
        let mut writes: Vec<(crate::interact::BlockPos, String, i32)> = Vec::new();
        store.with_mut(pos, |be| {
            if valid_for != Some(be.kind()) {
                return;
            }
            tick_one(be, pos, &state, tables, &mut out, &mut writes);
        });
        apply_ticker_writes(world, tables.shapes, writes, &mut out);
        notify_output_signal(world, tables.shapes, marked, &mut out);
    }
    out
}

/// A ticker's `Level.setBlock(pos, state, flags)` calls, applied through
/// [`crate::shapes::set_block`] once the block entity's lock is released.
///
/// They used to be bare `Blocks::set_block` writes, which is `LevelChunk
/// .setBlockState` and none of `Level.setBlock`'s tail: no
/// `updateNeighborsAt` for the furnace's flags 3, no
/// `updateNeighbourForOutputSignal`, and — for every ticker, flags 2
/// included, since none of them sets `UPDATE_KNOWN_SHAPE` — no
/// `updateNeighbourShapes`. An observer watching a furnace never saw it
/// light. The flags are each ticker's own constant from the jar:
/// `AbstractFurnaceBlockEntity.serverTick` `iconst_3` (offset 398),
/// `BrewingStandBlockEntity.serverTick` `iconst_2` (offset 300),
/// `JukeboxBlockEntity` `iconst_2` (offset 51).
fn apply_ticker_writes(
    world: &dyn crate::edits::Blocks,
    shapes: Option<&crate::shapes::ShapeTable>,
    writes: Vec<(crate::interact::BlockPos, String, i32)>,
    out: &mut TickEffects,
) {
    for (at, next, flags) in writes {
        let mut effects = crate::shapes::Effects::new();
        crate::shapes::set_block(
            world,
            shapes,
            at,
            &next,
            flags,
            crate::shapes::RECURSION_LIMIT,
            &mut effects,
        );
        // `sendBlockUpdated` order: the ticker's own block first, then what
        // its neighbours answered, as the scheduled-tick drain records them.
        out.blocks.extend(effects.blocks.iter().map(|&(p, _)| p));
        out.level_events.extend(effects.level_events);
        out.sounds.extend(effects.sounds);
        out.ticks.extend(effects.ticks);
        debug_assert!(
            effects.falls.is_empty(),
            "a shape update books a fall as a tick, never directly"
        );
    }
}

/// The `Level.updateNeighbourForOutputSignal(pos, block)` half of
/// `BlockEntity.setChanged(level, pos, state)` (offsets 5-18), for every
/// position marked since `from`.
///
/// `setChanged` guards only on `!state.isAir()` and **not** on
/// `hasAnalogOutputSignal`, so a campfire's `setChanged` reaches the
/// comparators beside it too, which then read through it as they would
/// through any block with no analog output. A block entity always stands
/// under a block, so the guard cannot fail here and is not written.
///
/// A position marked twice in one ticker (a hopper is marked by
/// `tryMoveInItem` and again by `tryMoveItems`) is notified once: the second
/// `neighborChanged` would read the same container and book the same tick,
/// which `LevelTicks` refuses as a duplicate.
fn notify_output_signal(
    world: &dyn crate::edits::Blocks,
    shapes: Option<&crate::shapes::ShapeTable>,
    from: usize,
    out: &mut TickEffects,
) {
    let Some(signals) = crate::power::shared() else {
        return;
    };
    let mut effects = crate::shapes::Effects::new();
    let mut seen: Vec<crate::interact::BlockPos> = Vec::new();
    for &pos in &out.changed[from..] {
        if seen.contains(&pos) {
            continue;
        }
        seen.push(pos);
        crate::comparator::update_neighbour_for_output_signal(
            world,
            signals,
            shapes,
            pos,
            &mut effects,
        );
    }
    out.ticks.extend(effects.ticks);
}

/// One `BlockEntityTicker.tick(level, pos, state, be)`.
fn tick_one(
    be: &mut crate::blockentity::BlockEntity,
    pos: crate::interact::BlockPos,
    state: &str,
    tables: &TickTables<'_>,
    out: &mut TickEffects,
    writes: &mut Vec<(crate::interact::BlockPos, String, i32)>,
) {
    let kind = be.kind();
    match kind {
        Kind::Furnace | Kind::Smoker | Kind::BlastFurnace => {
            furnace_tick(be, pos, state, tables, out, writes);
        }
        Kind::Jukebox => jukebox_tick(be, pos, state, tables, out, writes),
        Kind::Campfire => campfire_tick(be, pos, state, tables, out),
        Kind::BrewingStand => brewing_tick(be, pos, state, tables, out, writes),
        Kind::Crafter => crafter_tick(be, pos, state, writes),
        // The hopper is dispatched by the pass itself, not from here: it
        // needs three block entities under one lock and `tick_one` holds only
        // its own. See `hopper_tick`.
        Kind::Hopper => {}
        // The other six ported arms land one at a time; until then a kind
        // with a behaviour and no arm is a block entity on the list that does
        // nothing, which is what it was before the list existed.
        _ => {}
    }
}

/// `CrafterBlockEntity.serverTick`, wired to the world: when the six
/// crafting ticks a craft set run out, `setBlock(pos, CRAFTING=false, 3)`.
fn crafter_tick(
    be: &mut crate::blockentity::BlockEntity,
    pos: crate::interact::BlockPos,
    state: &str,
    writes: &mut Vec<(crate::interact::BlockPos, String, i32)>,
) {
    let Some(Behaviour::Crafter(c)) = be.behaviour_mut() else {
        return;
    };
    if c.server_tick() {
        writes.push((
            pos,
            crate::placement::with_property(state, "crafting", "false"),
            crate::shapes::UPDATE_ALL,
        ));
    }
}

/// `AbstractFurnaceBlockEntity.serverTick`, wired to the world.
///
/// The block-state write is `setBlock(pos, state.setValue(LIT, isLit), 3)` at
/// offset 383, and it happens only when `wasLit != isLit` — which is why a
/// furnace that stays lit sends nothing at all and a scenario watching for a
/// `block_update` every tick would be watching for something vanilla does not
/// do.
///
/// **A furnace has no `block_entity_data`.** `AbstractFurnaceBlockEntity` does
/// not declare `getUpdatePacket`, so the four counters reach a client only
/// through the menu's data slots, and there is nothing to broadcast here. A
/// scenario that expected the cooking progress on the wire would be waiting
/// for a packet vanilla never sends.
fn furnace_tick(
    be: &mut crate::blockentity::BlockEntity,
    pos: crate::interact::BlockPos,
    state: &str,
    tables: &TickTables<'_>,
    out: &mut TickEffects,
    writes: &mut Vec<(crate::interact::BlockPos, String, i32)>,
) {
    let block = crate::blockentity::block_of(state);
    let (Some(cooking), Some(recipes), Some(fuels)) = (
        crate::cooking::CookingKind::for_furnace_block(block),
        tables.recipes,
        tables.fuels,
    ) else {
        return;
    };
    let Some(Behaviour::Furnace(mut furnace)) = be.behaviour().cloned() else {
        return;
    };
    let mut slots: Vec<crate::item::ItemStack> = be.items_mut().to_vec();
    let tick = crate::cooking::server_tick(&mut furnace, &mut slots, cooking, recipes, fuels);
    for (i, stack) in slots.into_iter().enumerate() {
        be.set_item(i, stack);
    }
    if let Some(Behaviour::Furnace(slot)) = be.behaviour_mut() {
        *slot = furnace;
    }
    if tick.changed {
        out.changed.push(pos);
    }
    // `setRecipeUsed` — the experience ledger the result slot pays out of.
    if let Some(id) = &tick.recipe_used {
        crate::xporb::FurnaceLedger::shared().record(pos, id);
    }
    if let Some(lit) = tick.lit_changed {
        let next =
            crate::placement::with_property(state, "lit", if lit { "true" } else { "false" });
        writes.push((pos, next, crate::shapes::UPDATE_ALL));
    }
}

/// `BrewingStandBlockEntity.serverTick`, wired to the world.
///
/// What a client sees of it is three things, and only one is a packet the
/// ticker sends:
///
/// * the `has_bottle_0..2` block state, rewritten with `setBlock(pos, state,
///   2)` only when `getPotionBits` differs from `lastPotionCount` (offsets
///   220-302) — flag 2 is "send to clients" with no neighbour update, so it
///   reaches the wire through the next tick's block broadcast exactly as the
///   furnace's `lit` does;
/// * `level_event(1035, pos, 0)` when a brew completes — `doBrew`'s last
///   instruction;
/// * the menu's two data slots and the slots it consumed, which reach an
///   open menu through `crate::click::sweep_container`.
///
/// The brewing table is `crate::brewing::VanillaBrewing`, which is exact for
/// every stack this server can hold. The `BREWING_FUEL` tag is read from the
/// cooking table's own tag document; with no table the stand does not tick,
/// for the reason [`TickTables`] gives.
///
/// The state write is skipped for a block that is not a brewing stand —
/// offset 238's `instanceof BrewingStandBlock` return — which `isValid` has
/// already guaranteed for the ticker to have been reached at all.
///
/// `doBrew`'s remainder drop (dragon's breath over a stack of more than one)
/// is collected on [`TickEffects::drops`] and, like every other entry there,
/// not yet spawned by the caller.
fn brewing_tick(
    be: &mut crate::blockentity::BlockEntity,
    pos: crate::interact::BlockPos,
    state: &str,
    tables: &TickTables<'_>,
    out: &mut TickEffects,
    writes: &mut Vec<(crate::interact::BlockPos, String, i32)>,
) {
    let Some(recipes) = tables.recipes else {
        return;
    };
    let Some(Behaviour::Brewing(mut stand)) = be.behaviour().cloned() else {
        return;
    };
    let mut slots: Vec<crate::item::ItemStack> = be.items_mut().to_vec();
    let tick = crate::brewing::server_tick(
        &mut stand,
        &mut slots,
        &crate::brewing::VanillaBrewing,
        recipes.tags(),
    );
    for (i, stack) in slots.into_iter().enumerate() {
        be.set_item(i, stack);
    }
    if let Some(Behaviour::Brewing(slot)) = be.behaviour_mut() {
        *slot = stand;
    }
    if tick.changed {
        out.changed.push(pos);
    }
    if tick.brewed {
        out.level_events
            .push((crate::brewing::BREW_LEVEL_EVENT, pos, 0));
    }
    for stack in tick.drops {
        out.drops.push((pos, stack));
    }
    if let Some(bits) = tick.bottle_bits {
        if crate::blockentity::block_of(state) == "minecraft:brewing_stand" {
            let mut next = state.to_string();
            for (i, on) in bits.iter().enumerate() {
                next = crate::placement::with_property(
                    &next,
                    &format!("has_bottle_{i}"),
                    if *on { "true" } else { "false" },
                );
            }
            writes.push((pos, next, crate::shapes::UPDATE_CLIENTS));
        }
    }
}

/// `JukeboxBlockEntity.tick` -> `JukeboxSongPlayer.tick`, wired to the world.
///
/// The whole of what a playing jukebox does on the server is count, and then
/// once — at `ticksSinceSongStarted >= length + 20` — stop: clear the item,
/// write `HAS_RECORD` false and send `level_event(1011, pos, 0)`.
///
/// **The finish test runs before the counter is incremented**, so a song of
/// `n` ticks stops on the tick where the counter has already reached `n + 20`
/// rather than on the one where it reaches it.
///
/// `JukeboxBlock.getTicker` has no `isClientSide` test at all — it reads
/// `HAS_RECORD` off the state and hands the same ticker to both sides — so a
/// jukebox with no disc is simply not on `blockEntityTickers`. That gate is
/// applied here rather than by leaving the entry off the list, because the
/// list is keyed on the position and the state under it changes.
fn jukebox_tick(
    be: &mut crate::blockentity::BlockEntity,
    pos: crate::interact::BlockPos,
    state: &str,
    tables: &TickTables<'_>,
    out: &mut TickEffects,
    writes: &mut Vec<(crate::interact::BlockPos, String, i32)>,
) {
    if crate::placement::property_of(state, "has_record") != Some("true") {
        return;
    }
    let Some(songs) = tables.songs else { return };
    let Some(Behaviour::Jukebox(mut player)) = be.behaviour().cloned() else {
        return;
    };
    let mut effects = crate::jukebox::JukeboxEffects::default();
    player.tick(songs, &mut effects);
    if let Some(Behaviour::Jukebox(cell)) = be.behaviour_mut() {
        *cell = player;
    }
    // `JukeboxBlockEntity.tick` offsets 12-24: the stop path clears the item
    // through `setTheItem(ItemStack.EMPTY)`, which is what writes the state.
    // `SongPlayer::stop` reports the `level_event` and this is the half that
    // owns the block entity, so the two are applied together.
    if effects.stop_event {
        be.set_item(0, crate::item::ItemStack::empty());
        out.changed.push(pos);
        let next = crate::placement::with_property(state, "has_record", "false");
        writes.push((pos, next, crate::shapes::UPDATE_CLIENTS));
    }
    for (event, data) in effects.level_events {
        out.level_events.push((event, pos, data));
    }
}

/// `HopperBlockEntity.pushItemsTick` — offsets 0-45 — and the two containers
/// it reaches for.
///
/// # Why this is not an arm of `tick_one`
///
/// A hopper transfer touches **three** block entities: the hopper, the
/// container it faces (`getAttachedContainer`) and the container above it
/// (`getSourceContainer`). `tick_one` is handed one `&mut BlockEntity` under
/// the store's write lock, and asking for a second inside that borrow is the
/// deadlock this store's `with_mut` doc warns about. So the three are copied
/// out, the transfer runs on the copies, and only the ones the transfer
/// actually marked `setChanged` are written back — which is also what makes a
/// hopper pointing at itself do nothing rather than double-count.
///
/// # Vanilla's own order, and both halves run
///
/// `tryMoveItems` is `ejectItems(...) | suckInItems(...)` joined by `ior` and
/// not by a short circuit, so a hopper both empties into the chest below and
/// fills from the chest above in the same tick. `pushItemsTick` decrements
/// the cooldown **before** it tests it and stamps `tickedGameTime`
/// unconditionally, so a hopper on cooldown still records the tick.
///
/// # What reaches the wire
///
/// Nothing, directly. A container write sends no packet of its own — a chest
/// declares no `getUpdatePacket` — so a hopper transfer is visible only
/// through an open menu, which is `AbstractContainerMenu.broadcastChanges`
/// and reaches the wire as `container_set_slot`.
fn hopper_tick(
    store: &crate::blockentity::BlockEntities,
    pos: crate::interact::BlockPos,
    state: &str,
    world: &dyn crate::edits::Blocks,
    tables: &TickTables<'_>,
    now: i64,
    out: &mut TickEffects,
) {
    let Some(facing) =
        crate::placement::property_of(state, "facing").and_then(crate::interact::Face::from_name)
    else {
        return;
    };
    // `HopperBlock.ENABLED` — `enabled` is false while the hopper is powered,
    // and `pushItemsTick` skips the whole transfer without clearing the
    // cooldown it has already zeroed.
    let enabled = crate::placement::property_of(state, "enabled") != Some("false");
    let (dx, dy, dz) = facing.normal();
    let attached_pos = (pos.0 + dx, pos.1 + dy, pos.2 + dz);
    let above_pos = (pos.0, pos.1 + 1, pos.2);
    let Some(mut hopper) = view_at(store, world, pos) else {
        return;
    };
    let mut attached = view_at(store, world, attached_pos);
    let mut above = view_at(store, world, above_pos);
    // `suckInItems` offsets 103-145: with no container above, the item
    // entities in `SUCK_AABB` — unless the block above is a full collision
    // cube that is not `#does_not_block_hoppers`.
    let mut items = if above.is_none() && !blocks_hopper(world, above_pos) {
        Some(items_at_and_above(pos))
    } else {
        None
    };
    let rules = crate::hopper::VanillaRules {
        fuels: tables.fuels,
        tags: tables.recipes.map(crate::recipes::Recipes::tags),
    };
    let rules: &dyn crate::hopper::Rules = &rules;
    let tick = crate::hopper::push_items_tick_with_items(
        &mut hopper,
        now,
        enabled,
        facing,
        attached.as_mut(),
        above.as_mut(),
        items.as_mut(),
        rules,
    );
    if let Some(items) = items {
        let stacks = crate::dispenser::ItemEntityStacks::shared();
        for e in items {
            if e.stack.is_empty() {
                stacks.remove(e.id);
                out.discarded.push(e.id);
            } else {
                // A partial absorption: `setItem(remaining)`. The
                // `DATA_ITEM` change it makes is not re-sent here; see
                // `crate::synched::Synched::Item`.
                stacks.set(e.id, e.stack);
            }
        }
    }
    // Written back in the order vanilla's `setChanged` calls first reach
    // each container — `ejectItems` marks the attached one, `tryMoveInItem`
    // the hopper as a destination, `tryTakeInItemFromSlot` the one above —
    // because each is also an `updateNeighbourForOutputSignal`, and two
    // comparators booked for the same tick fire in the order they were asked.
    if let Some(v) = &attached {
        write_back(store, world, attached_pos, v, out);
    }
    write_back(store, world, pos, &hopper, out);
    if let Some(v) = &above {
        write_back(store, world, above_pos, v, out);
    }
    // The hopper's own clock is written back whatever else happened: the
    // decrement and the `tickedGameTime` stamp are unconditional.
    store.with_mut(pos, |be| {
        if let Some(Behaviour::Hopper(clock)) = be.behaviour_mut() {
            clock.cooldown = tick.cooldown;
            clock.ticked_game_time = now;
        }
    });
}

/// `HopperBlockEntity.getItemsAtAndAbove(level, hopper)` — offsets 0-51.
///
/// `Hopper.SUCK_AABB` is `Block.column(16, 11, 32)`: the hopper's whole
/// footprint from 11/16 up to two blocks above its base. `getEntitiesOfClass`
/// keeps an entity whose bounding box **strictly** intersects it
/// (`AABB.intersects` is `<`/`>` on every axis), and an `ItemEntity`'s box is
/// 0.25 wide and 0.25 tall, standing on its position.
///
/// Only items this server knows the stack of are candidates — every item
/// entity it creates registers one in `crate::dispenser::ItemEntityStacks`.
/// The walk is in entity-id order, which is the order they were added; vanilla
/// walks its section storage, which for entities in one section is the same.
fn items_at_and_above(pos: crate::interact::BlockPos) -> Vec<crate::hopper::ItemAbove> {
    const HALF_WIDTH: f64 = 0.125;
    const HEIGHT: f64 = 0.25;
    let (min_x, max_x) = (f64::from(pos.0), f64::from(pos.0) + 1.0);
    let (min_y, max_y) = (f64::from(pos.1) + 11.0 / 16.0, f64::from(pos.1) + 2.0);
    let (min_z, max_z) = (f64::from(pos.2), f64::from(pos.2) + 1.0);
    let stacks = crate::dispenser::ItemEntityStacks::shared();
    let mut found: Vec<crate::hopper::ItemAbove> = crate::entities::Entities::shared()
        .all()
        .into_iter()
        .filter(|e| e.kind.name == crate::entities::ITEM.name)
        .filter(|e| {
            e.x - HALF_WIDTH < max_x
                && e.x + HALF_WIDTH > min_x
                && e.y < max_y
                && e.y + HEIGHT > min_y
                && e.z - HALF_WIDTH < max_z
                && e.z + HALF_WIDTH > min_z
        })
        .filter_map(|e| {
            stacks
                .get(e.id)
                .map(|stack| crate::hopper::ItemAbove { id: e.id, stack })
        })
        .collect();
    found.sort_by_key(|e| e.id);
    found
}

/// `suckInItems` offsets 103-136: `isGridAligned()` (always, for a block
/// hopper) `&& isCollisionShapeFullBlock && !is(DOES_NOT_BLOCK_HOPPERS)`.
///
/// The full-block read is the collision table's `Shapes.block()` identity,
/// which is what `isCollisionShapeFullBlock` resolves to for every cube in
/// the registry. With no collision table the answer is "not blocked", which
/// errs toward sucking — and air, the case every hopper in the corpus has
/// above it, is not blocked under either answer.
fn blocks_hopper(world: &dyn crate::edits::Blocks, pos: crate::interact::BlockPos) -> bool {
    let state = world.state_at(pos);
    if crate::shapes::is_air(&state) {
        return false;
    }
    let block = state.split('[').next().unwrap_or(&state);
    if crate::fallable::tags()
        .and_then(|t| t.get("does_not_block_hoppers"))
        .is_some_and(|set| set.contains(block))
    {
        return false;
    }
    let Some(shapes) = crate::collide::CollisionShapes::shared() else {
        return false;
    };
    let Some(id) = world.state_id(&state) else {
        return false;
    };
    u32::try_from(id)
        .ok()
        .and_then(|id| shapes.get(id))
        .is_some_and(crate::collide::Shape::is_full_block)
}

/// `HopperBlockEntity.getContainerAt(level, pos)`, for a block entity.
///
/// `None` for a position with no block entity and for one whose type has no
/// container — a sign, a bell — which is `instanceof Container` in vanilla
/// and `container_size() > 0` here. The entity half of `getContainerAt`, the
/// `getEntityContainer` scan for a minecart or a boat with a chest, is not
/// ported; a hopper under a chest minecart moves nothing.
///
/// # A double chest is one container of 54
///
/// `getBlockContainer` asks `ChestBlock.getContainer(chest, state, level,
/// pos, true)` for a chest, which runs the same `DoubleBlockCombiner` the
/// menu does: a paired chest answers `new CompoundContainer(first, second)`
/// where `getBlockType` makes the `type=right` half `FIRST` (offsets 0-36)
/// and `combineWithNeigbour` passes the FIRST half's entity first (offsets
/// 219-255) whichever half was asked. So a hopper filling either half of a
/// double chest fills the RIGHT half's slot 0 first. `crate::menu::
/// double_chest` is that combiner and is reused rather than re-derived.
pub(crate) fn view_at(
    store: &crate::blockentity::BlockEntities,
    world: &dyn crate::edits::Blocks,
    pos: crate::interact::BlockPos,
) -> Option<crate::hopper::ContainerView> {
    let state = world.state_at(pos);
    let kind = crate::blockentity::for_state(&state)?;
    if kind.container_size() == 0 {
        return None;
    }
    let be = store.get(pos)?;
    if be.kind() != kind {
        return None;
    }
    if let [first, second] = container_parts(store, world, pos)[..] {
        let mut slots = Vec::new();
        for part in [first, second] {
            let be = store.get(part)?;
            slots.extend((0..be.size()).map(|i| be.item(i)));
        }
        return Some(crate::hopper::ContainerView {
            slots,
            worldly: crate::hopper::Worldly::None,
            hopper: None,
            changed: false,
        });
    }
    let slots: Vec<crate::item::ItemStack> = (0..be.size()).map(|i| be.item(i)).collect();
    let hopper = match be.behaviour() {
        Some(Behaviour::Hopper(clock)) => Some(*clock),
        _ => None,
    };
    Some(crate::hopper::ContainerView {
        slots,
        worldly: worldly_of(kind),
        hopper,
        changed: false,
    })
}

/// Which `WorldlyContainer`, if any, a kind is.
///
/// Three classes declare the interface and no others, which is
/// `crate::hopper::Worldly`'s own census: `AbstractFurnaceBlockEntity`,
/// `BrewingStandBlockEntity` and `ShulkerBoxBlockEntity`. A chest, a barrel,
/// a dropper and a hopper are plain `Container`s and every face sees every
/// slot in index order.
const fn worldly_of(kind: Kind) -> crate::hopper::Worldly {
    match kind {
        Kind::Furnace | Kind::Smoker | Kind::BlastFurnace => crate::hopper::Worldly::Furnace,
        Kind::BrewingStand => crate::hopper::Worldly::Brewing,
        Kind::ShulkerBox => crate::hopper::Worldly::Shulker,
        _ => crate::hopper::Worldly::None,
    }
}

/// Write a view's slots back, but **only if the transfer marked it changed**.
///
/// `Container.setChanged()` is the flag vanilla's own algorithm sets, and
/// gating the write on it is what stops a hopper pointing into its own
/// position from writing the same list twice and what keeps an untouched
/// container out of `TickEffects::changed`.
fn write_back(
    store: &crate::blockentity::BlockEntities,
    world: &dyn crate::edits::Blocks,
    pos: crate::interact::BlockPos,
    view: &crate::hopper::ContainerView,
    out: &mut TickEffects,
) {
    if !view.changed {
        return;
    }
    out.changed.extend(store_view(store, world, pos, view));
}

/// The block entities behind the container at `pos`, in `CompoundContainer`
/// order: both halves of a paired chest, otherwise `pos` alone.
pub(crate) fn container_parts(
    store: &crate::blockentity::BlockEntities,
    world: &dyn crate::edits::Blocks,
    pos: crate::interact::BlockPos,
) -> Vec<crate::interact::BlockPos> {
    let state = world.state_at(pos);
    let block = crate::blockentity::block_of(&state);
    if block.ends_with("chest") && block != "minecraft:ender_chest" {
        if let Some((first, second)) =
            crate::menu::double_chest(&state, pos, &|p| world.state_at(p))
        {
            if store.get(first).is_some() && store.get(second).is_some() {
                return vec![first, second];
            }
        }
    }
    vec![pos]
}

/// Write a container view back to the block entities behind it — split
/// across both halves for a double chest — and return the positions written,
/// each of which vanilla's `CompoundContainer.setChanged` marks changed.
pub(crate) fn store_view(
    store: &crate::blockentity::BlockEntities,
    world: &dyn crate::edits::Blocks,
    pos: crate::interact::BlockPos,
    view: &crate::hopper::ContainerView,
) -> Vec<crate::interact::BlockPos> {
    let parts = container_parts(store, world, pos);
    let mut offset = 0;
    for &part in &parts {
        store.with_mut(part, |be| {
            let n = be.size();
            for i in 0..n {
                if let Some(stack) = view.slots.get(offset + i) {
                    be.set_item(i, stack.clone());
                }
            }
            offset += n;
        });
    }
    parts
}

/// `CampfireBlockEntity.cookTick` and `cooldownTick`, picked by `LIT`.
///
/// `CampfireBlock.getTicker` is two nested tests: `level instanceof
/// ServerLevel` and then `LIT`. Lit on the server is `cookTick`, unlit is
/// `cooldownTick`, and the two client arms are `particleTick` and null. So
/// one entry on the ticker list runs one of two different methods depending
/// on a block state that can change under it, which is why the choice is made
/// here rather than when the entry was armed.
///
/// # The three ways this differs from the furnace one class away
///
/// * the completion test is `if_icmplt` — `>=` — where
///   `AbstractFurnaceBlockEntity` uses `if_icmpne`. A campfire whose progress
///   overshot still finishes; a furnace whose `cookingTimer` overshot never
///   equals `cookingTotalTime` again;
/// * a slot whose recipe has gone away drops the **raw** food, from
///   `orElse(theRawStack)`. A furnace in the same state does nothing at all;
/// * `changed` is set for any non-empty slot, not only for one that finished,
///   so a burning campfire marks its chunk every tick.
///
/// A finish reaches the wire. `cookTick`'s tail is `markUpdated` — the same
/// `setChanged` plus `sendBlockUpdated(pos, state, state, 3)` that
/// `placeFood` ends with — so the position goes on `ChangedBlocks` and the
/// broadcast carries both the `block_update` and, because
/// `CampfireBlockEntity` declares `getUpdatePacket`, a `block_entity_data`.
///
/// **The drops are reported, not spawned.** `Containers.dropItemStack` makes
/// an item entity at the block's own position with vanilla's own velocity
/// draws, and `crate::dropitem` owns that; a half-port that spawned one with
/// a guessed velocity would put a wrong `add_entity` on the wire where today
/// there is none.
fn campfire_tick(
    be: &mut crate::blockentity::BlockEntity,
    pos: crate::interact::BlockPos,
    state: &str,
    tables: &TickTables<'_>,
    out: &mut TickEffects,
) {
    let lit = crate::placement::property_of(state, "lit") == Some("true");
    let Some(Behaviour::Campfire(mut st)) = be.behaviour().cloned() else {
        return;
    };
    let mut items: Vec<crate::item::ItemStack> = (0..be.size()).map(|i| be.item(i)).collect();
    let tick = if lit {
        let Some(recipes) = tables.recipes else {
            return;
        };
        crate::campfire::cook_tick(&mut st, &mut items, recipes)
    } else {
        crate::campfire::cooldown_tick(&mut st, items.len())
    };
    if let Some(Behaviour::Campfire(cell)) = be.behaviour_mut() {
        *cell = st;
    }
    for (i, item) in items.into_iter().enumerate() {
        be.set_item(i, item);
    }
    if tick.changed {
        out.changed.push(pos);
    }
    if !tick.finished.is_empty() {
        // `markUpdated` — one write per finishing tick, not one per slot.
        out.blocks.push(pos);
    }
    for stack in tick.drops {
        out.drops.push((pos, stack));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every kind, for the exhaustive sweeps below.
    fn all_kinds() -> Vec<Kind> {
        // `Kind::from_id` round-trips in `crate::blockentity`'s own test, so
        // the registry ids are the honest enumeration: an added variant with
        // no id would fail there, and one with an id appears here.
        let doc = golden();
        match doc {
            Some(d) => d
                .get("table")
                .and_then(|t| t.as_array())
                .expect("table")
                .iter()
                .map(|r| {
                    let id = r.get("type").and_then(|v| v.as_str()).expect("type");
                    Kind::from_id(id).unwrap_or_else(|| panic!("unmodelled type {id}"))
                })
                .collect(),
            None => Vec::new(),
        }
    }

    fn golden() -> Option<serde_json::Value> {
        const REL: &str = "data/vanilla/block_entities.json";
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(REL);
        if !rustmc_data::fixture::available(
            &path,
            "block entity extract",
            "ci/block-entity-golden.sh",
        ) {
            return None;
        }
        let text = std::fs::read_to_string(&path).expect("read block_entities.json");
        Some(serde_json::from_str(&text).expect("parse block_entities.json"))
    }

    /// The ticker table against vanilla's own declarations.
    ///
    /// Three assertions, and each fires on a different way to be wrong:
    ///
    /// 1. a kind we say ticks whose entity declares no ticker-shaped static
    ///    **and** whose block declares no `getTicker` is a ticker invented
    ///    from nothing;
    /// 2. a method name we quote that the class does not declare is a
    ///    transcription that has drifted — this is the assertion a version
    ///    bump that renames `serverTick` fires;
    /// 3. a kind vanilla gives a named ticker that we record as
    ///    [`Ticker::None`] is a gap that stopped being written down.
    ///
    /// Assertion 3 has three sanctioned exceptions and they are named, not
    /// skipped: `ChestBlockEntity`, `EnderChestBlockEntity` and
    /// `EnchantingTableBlockEntity` declare a ticker their block hands only
    /// to the client, and `SkullBlockEntity` does the same. The golden cannot
    /// see the side, so the exception list is where that residue lives — and
    /// it is a fixed list, so a *fourth* type acquiring a client-only ticker
    /// reds this test rather than joining them quietly.
    #[test]
    fn the_ticker_table_agrees_with_vanillas_own_declarations() {
        let Some(doc) = golden() else { return };
        let table = doc.get("table").and_then(|t| t.as_array()).expect("table");
        assert_eq!(table.len(), 49, "the extract is not the whole registry");
        /// Types whose only declared ticker is handed to the client.
        const CLIENT_ONLY: [&str; 5] = [
            "minecraft:chest",
            "minecraft:trapped_chest",
            "minecraft:ender_chest",
            "minecraft:enchanting_table",
            "minecraft:skull",
        ];
        let mut invented = Vec::new();
        let mut drifted = Vec::new();
        let mut undeclared_gap = Vec::new();
        for row in table {
            let id = row.get("type").and_then(|v| v.as_str()).expect("type");
            let kind = Kind::from_id(id).unwrap_or_else(|| panic!("unmodelled type {id}"));
            let declared: Vec<&str> = row
                .get("tickers")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(serde_json::Value::as_str).collect())
                .unwrap_or_default();
            let block_ticker = row
                .get("blockTicker")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            let ours = server_ticker(kind);
            match ours.method() {
                Some(m) => {
                    if declared.is_empty() && !block_ticker {
                        invented.push(format!("{id} -> {m}"));
                    }
                    // The method name we quote is `Class.method`, or a phrase
                    // for the four whose ticker is not a named static on the
                    // entity. Only the first form is checkable here, and only
                    // when the class in it is the entity's own.
                    if let Some((_, name)) = m.rsplit_once('.') {
                        // `cookTick` is the lit arm and `cooldownTick` the
                        // unlit one; the row names the first and the golden
                        // declares both.
                        let names_it = declared.contains(&name)
                            || (name == "cookTick" && declared.contains(&"cooldownTick"));
                        if !declared.is_empty() && !names_it {
                            drifted.push(format!(
                                "{id}: we say {name}, vanilla declares {declared:?}"
                            ));
                        }
                    }
                }
                None => {
                    if (!declared.is_empty() || block_ticker) && !CLIENT_ONLY.contains(&id) {
                        undeclared_gap.push(format!("{id}: vanilla declares {declared:?}"));
                    }
                }
            }
        }
        assert!(
            invented.is_empty(),
            "ticker invented from nothing: {invented:?}"
        );
        assert!(drifted.is_empty(), "ticker name drifted: {drifted:?}");
        assert!(
            undeclared_gap.is_empty(),
            "vanilla ticks these and the table says None: {undeclared_gap:?}"
        );
    }

    /// The positive control for the test above: the same comparison run
    /// against a *deliberately wrong* table must fail all three ways.
    ///
    /// Without this, a golden that came back with an empty `tickers` column
    /// on every row — the `updatePacket` defect one column over — would make
    /// every assertion above vacuously true and the test would pass on a
    /// table that measures nothing.
    #[test]
    fn the_cross_check_fails_on_a_wrong_table() {
        let Some(doc) = golden() else { return };
        let table = doc.get("table").and_then(|t| t.as_array()).expect("table");
        let mut any_declared = 0;
        let mut any_block_ticker = 0;
        for row in table {
            if row
                .get("tickers")
                .and_then(|v| v.as_array())
                .is_some_and(|a| !a.is_empty())
            {
                any_declared += 1;
            }
            if row
                .get("blockTicker")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
            {
                any_block_ticker += 1;
            }
        }
        // If either column is uniform the check above distinguishes nothing.
        assert!(
            any_declared > 0 && any_declared < table.len(),
            "the tickers column is uniform ({any_declared} of {} rows), so the check measures nothing",
            table.len()
        );
        assert!(
            any_block_ticker > 0 && any_block_ticker < table.len(),
            "the blockTicker column is uniform ({any_block_ticker} of {} rows)",
            table.len()
        );
    }

    /// A behaviour exists for exactly the kinds this server ticks.
    ///
    /// Both directions: a `Ported` row with no `Behaviour` arm is a tick that
    /// would find nowhere to keep its counters, and a `Behaviour` for a kind
    /// nothing ticks is a field that can only ever be wrong.
    #[test]
    fn a_behaviour_exists_for_exactly_the_ported_tickers() {
        let kinds = all_kinds();
        if kinds.is_empty() {
            return;
        }
        let mut ported = 0;
        for k in kinds {
            let runs = server_ticker(k).runs();
            assert_eq!(
                Behaviour::for_kind(k).is_some(),
                runs,
                "{}: behaviour {:?} against ticker {:?}",
                k.id(),
                Behaviour::for_kind(k).is_some(),
                server_ticker(k)
            );
            if runs {
                ported += 1;
            }
        }
        assert_eq!(ported, 12, "the ported ticker count moved without a note");
    }

    /// A fresh hopper starts at `cooldownTime = -1`, not 0.
    ///
    /// The saved-state table already records `TransferCooldown: -1`; this is
    /// the same constant reaching the *behaviour*, and it matters because
    /// `pushItemsTick` decrements before it tests, so a hopper starting at 0
    /// would go to -1 and behave identically for one tick and differently
    /// forever after.
    #[test]
    fn a_fresh_hopper_starts_on_minus_one() {
        let Some(Behaviour::Hopper(h)) = Behaviour::for_kind(Kind::Hopper) else {
            panic!("a hopper has a behaviour")
        };
        assert_eq!(h.cooldown, -1);
        // The positive control: a fresh furnace's counters really are zero,
        // so "everything starts at -1" is not what is being asserted.
        let Some(Behaviour::Furnace(f)) = Behaviour::for_kind(Kind::Furnace) else {
            panic!("a furnace has a behaviour")
        };
        assert_eq!(f.lit_time_remaining, 0);
    }

    /// A chest is not ticked, and the reason is the one thing the golden
    /// cannot see.
    #[test]
    fn a_chest_declares_a_ticker_and_still_does_not_tick() {
        assert_eq!(server_ticker(Kind::Chest), Ticker::None);
        assert_eq!(server_ticker(Kind::EnderChest), Ticker::None);
        assert!(Behaviour::for_kind(Kind::Chest).is_none());
        // And the shape it is being distinguished from: a hopper's block
        // nulls the *client* arm instead.
        assert!(server_ticker(Kind::Hopper).runs());
    }

    /// The fourteen gaps are named rather than absent.
    #[test]
    fn every_unported_ticker_names_the_method_it_is_not_running() {
        let kinds = all_kinds();
        if kinds.is_empty() {
            return;
        }
        let gaps: Vec<&str> = kinds
            .iter()
            .filter_map(|k| match server_ticker(*k) {
                Ticker::Unported(m) => Some(m),
                _ => None,
            })
            .collect();
        assert_eq!(gaps.len(), 15, "the unported ticker count moved: {gaps:?}");
        assert!(gaps.iter().all(|m| !m.is_empty()));
    }

    // ---------------------------------------------------------------------
    // The pass
    // ---------------------------------------------------------------------

    use crate::interact::BlockPos;
    use crate::item::ItemStack;

    /// A world of named states and a record of every write.
    ///
    /// Only the four methods a block-entity ticker calls do anything; the
    /// other seven answer the way an empty world would. A ticker that started
    /// asking one of them would get a constant, so each is written as the
    /// honest answer for a world of air rather than as a `todo!` that would
    /// take the test down.
    #[derive(Default)]
    struct Fake {
        at: std::sync::Mutex<std::collections::BTreeMap<BlockPos, String>>,
        wrote: std::sync::Mutex<Vec<(BlockPos, String)>>,
    }

    impl Fake {
        fn with(states: &[(BlockPos, &str)]) -> Self {
            let f = Self::default();
            for (p, st) in states {
                f.at.lock().expect("fake").insert(*p, (*st).to_string());
            }
            f
        }
        fn writes(&self) -> Vec<(BlockPos, String)> {
            self.wrote.lock().expect("fake").clone()
        }
    }

    impl crate::edits::Blocks for Fake {
        fn state_at(&self, pos: BlockPos) -> String {
            self.at
                .lock()
                .expect("fake")
                .get(&pos)
                .cloned()
                .unwrap_or_else(|| "minecraft:air".to_string())
        }
        fn set_block(&self, pos: BlockPos, state: &str) -> Option<i32> {
            let mut at = self.at.lock().expect("fake");
            if at.get(&pos).map(String::as_str) == Some(state) {
                return None;
            }
            at.insert(pos, state.to_string());
            self.wrote
                .lock()
                .expect("fake")
                .push((pos, state.to_string()));
            Some(7)
        }
        fn default_state(&self, block: &str) -> Option<String> {
            Some(block.to_string())
        }
        fn has_state(&self, _: &str) -> bool {
            true
        }
        fn state_id(&self, _: &str) -> Option<i32> {
            Some(1)
        }
        fn replaceable(&self, pos: BlockPos, _: &crate::placement::Replacing<'_>) -> bool {
            self.state_at(pos) == "minecraft:air"
        }
        fn face_sturdy(&self, pos: BlockPos, _: crate::interact::Face) -> bool {
            self.state_at(pos) != "minecraft:air"
        }
        fn redstone_conductor(&self, _: BlockPos) -> bool {
            false
        }
        fn signal_source(&self, _: BlockPos) -> bool {
            false
        }
        fn water_at(&self, _: BlockPos) -> bool {
            false
        }
        fn legacy_fluid_at(&self, _: BlockPos) -> String {
            "minecraft:empty".to_string()
        }
    }

    /// A store with the given kinds at the given positions, armed.
    fn store(at: &[(BlockPos, Kind)]) -> crate::blockentity::BlockEntities {
        let s = crate::blockentity::BlockEntities::new();
        for (pos, kind) in at {
            s.insert(*pos, crate::blockentity::BlockEntity::new(*kind));
        }
        s
    }

    fn all_chunks() -> std::collections::BTreeSet<(i32, i32)> {
        let mut c = std::collections::BTreeSet::new();
        for x in -1..=1 {
            for z in -1..=1 {
                c.insert((x, z));
            }
        }
        c
    }

    fn cooking_tables() -> (crate::recipes::Recipes, crate::fuel::FuelValues) {
        let recipes = crate::recipes::Recipes::parse(
            &serde_json::json!({
                "iron_ingot_from_smelting_iron_ore": {
                    "type": "minecraft:smelting",
                    "experience": 0.7,
                    "ingredient": "minecraft:raw_iron",
                    "result": {"id": "minecraft:iron_ingot"}
                }
            }),
            &serde_json::json!({}),
        )
        .expect("recipes");
        let fuels = crate::fuel::FuelValues::parse(&serde_json::json!({
            "table": [{"item": "minecraft:coal", "burnDuration": 1600}]
        }))
        .expect("fuels");
        (recipes, fuels)
    }

    /// The whole join, end to end: a furnace with fuel and an ingredient
    /// lights, and the `lit` block update is the one thing that reaches the
    /// wire.
    ///
    /// The positive control is the second half — the same furnace with **no
    /// fuel** writes nothing at all — so a pass that wrote `lit=true`
    /// unconditionally would fail here even though the first assertion is
    /// about a write happening.
    #[test]
    fn a_fuelled_furnace_lights_on_its_first_tick_and_an_unfuelled_one_does_not() {
        let (recipes, fuels) = cooking_tables();
        let tables = TickTables {
            recipes: Some(&recipes),
            fuels: Some(&fuels),
            songs: None,
            shapes: None,
        };
        for (fuel, expected) in [("minecraft:coal", 1_usize), ("minecraft:air", 0)] {
            let pos = (0, 64, 0);
            let world = Fake::with(&[(pos, "minecraft:furnace[facing=north,lit=false]")]);
            let s = store(&[(pos, Kind::Furnace)]);
            s.with_mut(pos, |be| {
                be.set_item(0, ItemStack::one("minecraft:raw_iron"));
                if fuel != "minecraft:air" {
                    be.set_item(1, ItemStack::one(fuel));
                }
            });
            let out = tick_block_entities(&s, &world, &all_chunks(), &tables, 1);
            assert_eq!(
                out.blocks.len(),
                expected,
                "fuel {fuel}: block writes {:?}",
                world.writes()
            );
            if expected == 1 {
                assert_eq!(
                    world.writes()[0].1,
                    "minecraft:furnace[facing=north,lit=true]"
                );
            }
        }
    }

    /// A block write from a ticker reaches `World::set_block`, which ends in
    /// `BlockEntities::on_block_changed` on the **same** store the pass is
    /// ticking. Writing from inside `with_mut` re-entered that store's
    /// `RwLock` on one thread and hung the server's tick thread the first time
    /// a furnace lit (found on `train-a`, 2026-09-30: every tick-driven packet
    /// stopped after `797`, and the recorder's next `block_changed_ack` wait
    /// timed out). `ReEnter` does what `World` does, but asks the lock first
    /// so the old code fails here instead of hanging the test.
    #[test]
    fn a_ticker_block_write_is_applied_after_the_block_entity_lock_is_released() {
        struct ReEnter<'a> {
            inner: Fake,
            store: &'a crate::blockentity::BlockEntities,
            held: std::sync::atomic::AtomicBool,
        }
        impl crate::edits::Blocks for ReEnter<'_> {
            fn state_at(&self, pos: BlockPos) -> String {
                self.inner.state_at(pos)
            }
            fn set_block(&self, pos: BlockPos, state: &str) -> Option<i32> {
                if !self.store.write_lock_is_free() {
                    self.held.store(true, std::sync::atomic::Ordering::SeqCst);
                    return self.inner.set_block(pos, state);
                }
                let was = self.inner.state_at(pos);
                let id = self.inner.set_block(pos, state);
                self.store.on_block_changed(pos, &was, state);
                id
            }
            fn default_state(&self, block: &str) -> Option<String> {
                self.inner.default_state(block)
            }
            fn has_state(&self, s: &str) -> bool {
                self.inner.has_state(s)
            }
            fn state_id(&self, s: &str) -> Option<i32> {
                self.inner.state_id(s)
            }
            fn replaceable(&self, pos: BlockPos, r: &crate::placement::Replacing<'_>) -> bool {
                self.inner.replaceable(pos, r)
            }
            fn face_sturdy(&self, pos: BlockPos, f: crate::interact::Face) -> bool {
                self.inner.face_sturdy(pos, f)
            }
            fn redstone_conductor(&self, pos: BlockPos) -> bool {
                self.inner.redstone_conductor(pos)
            }
            fn signal_source(&self, pos: BlockPos) -> bool {
                self.inner.signal_source(pos)
            }
            fn water_at(&self, pos: BlockPos) -> bool {
                self.inner.water_at(pos)
            }
            fn legacy_fluid_at(&self, pos: BlockPos) -> String {
                self.inner.legacy_fluid_at(pos)
            }
        }
        let (recipes, fuels) = cooking_tables();
        let tables = TickTables {
            recipes: Some(&recipes),
            fuels: Some(&fuels),
            songs: None,
            shapes: None,
        };
        let pos = (0, 64, 0);
        let s = store(&[(pos, Kind::Furnace)]);
        s.with_mut(pos, |be| {
            be.set_item(0, ItemStack::one("minecraft:raw_iron"));
            be.set_item(1, ItemStack::one("minecraft:coal"));
        });
        let world = ReEnter {
            inner: Fake::with(&[(pos, "minecraft:furnace[facing=north,lit=false]")]),
            store: &s,
            held: std::sync::atomic::AtomicBool::new(false),
        };
        let out = tick_block_entities(&s, &world, &all_chunks(), &tables, 1);
        assert!(
            !world.held.load(std::sync::atomic::Ordering::SeqCst),
            "the furnace's lit write ran while the pass held the block-entity lock"
        );
        assert_eq!(out.blocks, vec![pos], "and the write still happened");
        assert_eq!(
            world.inner.writes(),
            vec![(pos, "minecraft:furnace[facing=north,lit=true]".to_string())]
        );
    }

    /// The `shouldTickBlocksAt` gate: the same furnace outside the simulated
    /// disc is not ticked.
    ///
    /// Without this the gate would be a line no test could turn red, which is
    /// the class of defect `docs/AGENT-BRIEF.md` opens with.
    #[test]
    fn a_furnace_outside_the_simulated_chunks_does_not_tick() {
        let (recipes, fuels) = cooking_tables();
        let tables = TickTables {
            recipes: Some(&recipes),
            fuels: Some(&fuels),
            songs: None,
            shapes: None,
        };
        let pos = (1000, 64, 1000);
        let world = Fake::with(&[(pos, "minecraft:furnace[facing=north,lit=false]")]);
        let s = store(&[(pos, Kind::Furnace)]);
        s.with_mut(pos, |be| {
            be.set_item(0, ItemStack::one("minecraft:raw_iron"));
            be.set_item(1, ItemStack::one("minecraft:coal"));
        });
        let out = tick_block_entities(&s, &world, &all_chunks(), &tables, 1);
        assert!(out.blocks.is_empty(), "{:?}", world.writes());
        // And the control: the identical furnace inside the disc does light,
        // so this is measuring the gate and not a broken fixture.
        let near = (0, 64, 0);
        let world2 = Fake::with(&[(near, "minecraft:furnace[facing=north,lit=false]")]);
        let s2 = store(&[(near, Kind::Furnace)]);
        s2.with_mut(near, |be| {
            be.set_item(0, ItemStack::one("minecraft:raw_iron"));
            be.set_item(1, ItemStack::one("minecraft:coal"));
        });
        let out2 = tick_block_entities(&s2, &world2, &all_chunks(), &tables, 1);
        assert_eq!(out2.blocks, vec![near]);
    }

    /// `BlockEntityType.isValid(state)`: a furnace block entity under a chest
    /// is skipped rather than ticked as a furnace.
    #[test]
    fn a_block_entity_under_the_wrong_block_is_not_ticked() {
        let (recipes, fuels) = cooking_tables();
        let tables = TickTables {
            recipes: Some(&recipes),
            fuels: Some(&fuels),
            songs: None,
            shapes: None,
        };
        let pos = (0, 64, 0);
        let world = Fake::with(&[(pos, "minecraft:chest[facing=north,type=single]")]);
        let s = store(&[(pos, Kind::Furnace)]);
        s.with_mut(pos, |be| {
            be.set_item(0, ItemStack::one("minecraft:raw_iron"));
            be.set_item(1, ItemStack::one("minecraft:coal"));
        });
        let out = tick_block_entities(&s, &world, &all_chunks(), &tables, 1);
        assert!(out.blocks.is_empty());
        assert!(out.changed.is_empty());
    }

    /// A hopper over a chest moves one item down, and the chest is the one
    /// marked changed.
    ///
    /// The control is a hopper pointing at **air**: the same item stays put,
    /// so this is measuring the transfer and not the write-back.
    #[test]
    fn a_hopper_pushes_one_item_into_the_container_it_faces() {
        let tables = TickTables {
            recipes: None,
            fuels: None,
            songs: None,
            shapes: None,
        };
        let hopper = (0, 65, 0);
        let chest = (0, 64, 0);
        for (below, moved) in [
            ("minecraft:chest[facing=north,type=single]", true),
            ("minecraft:air", false),
        ] {
            let world = Fake::with(&[
                (hopper, "minecraft:hopper[enabled=true,facing=down]"),
                (chest, below),
            ]);
            let mut placed = vec![(hopper, Kind::Hopper)];
            if moved {
                placed.push((chest, Kind::Chest));
            }
            let s = store(&placed);
            s.with_mut(hopper, |be| {
                be.set_item(0, ItemStack::one("minecraft:stone"));
                // `pushItemsTick` decrements before it tests, so a hopper
                // fresh out of `Behaviour::for_kind` at -1 goes to -2 and is
                // not on cooldown. Zeroed here so the test asserts the
                // transfer rather than the constructor.
                if let Some(Behaviour::Hopper(c)) = be.behaviour_mut() {
                    c.cooldown = 0;
                }
            });
            let out = tick_block_entities(&s, &world, &all_chunks(), &tables, 5);
            let in_hopper = s.get(hopper).expect("hopper").item(0);
            if moved {
                assert!(
                    in_hopper.is_empty(),
                    "the stone should have left the hopper"
                );
                assert_eq!(
                    s.get(chest).expect("chest").item(0).item(),
                    "minecraft:stone"
                );
                assert!(out.changed.contains(&chest));
            } else {
                assert_eq!(in_hopper.item(), "minecraft:stone");
                assert!(out.changed.is_empty());
            }
        }
    }

    /// A hopper's transfer into a chest is the chest's `setChanged`, and that
    /// is `updateNeighbourForOutputSignal`: the comparator reading the chest
    /// is asked again and books its tick. The comparator's stored output is
    /// set stale (7 against an empty-or-one-stone chest that reads at most
    /// 1), so any re-read books one; what is measured is whether it was
    /// re-read at all.
    ///
    /// The control is the same hopper pointing at air: nothing moves, the
    /// chest is not marked, and the stale comparator is left alone — which
    /// is what this server did for every transfer before the walk notified.
    #[test]
    fn a_hopper_filling_a_chest_wakes_the_comparator_reading_it() {
        assert!(
            crate::power::shared().is_some(),
            "data/vanilla/signal.json is this test's fixture"
        );
        let tables = TickTables {
            recipes: None,
            fuels: None,
            songs: None,
            shapes: None,
        };
        let chest = (13, 64, 13);
        let cmp = (13, 64, 14);
        let cmp_state = "minecraft:comparator[facing=north,mode=compare,powered=false]";
        for (facing, expect_tick) in [("west", false), ("down", true)] {
            let hopper = (13, 65, 13);
            let world = Fake::with(&[
                (
                    hopper,
                    &*format!("minecraft:hopper[enabled=true,facing={facing}]"),
                ),
                (chest, "minecraft:chest[facing=north,type=single]"),
                (cmp, cmp_state),
            ]);
            let s = store(&[(hopper, Kind::Hopper), (chest, Kind::Chest)]);
            s.with_mut(hopper, |be| {
                be.set_item(0, ItemStack::one("minecraft:stone"));
                if let Some(Behaviour::Hopper(c)) = be.behaviour_mut() {
                    c.cooldown = 0;
                }
            });
            crate::comparator::Outputs::shared().set(cmp, 7);
            let out = tick_block_entities(&s, &world, &all_chunks(), &tables, 5);
            let booked = out
                .ticks
                .iter()
                .any(|t| t.pos == cmp && t.block == "minecraft:comparator" && t.delay == 2);
            assert_eq!(
                booked, expect_tick,
                "facing={facing}: changed={:?} ticks={:?}",
                out.changed, out.ticks
            );
        }
    }

    /// The hopper's clock is written back whatever else happened, because
    /// `pushItemsTick` stamps `tickedGameTime` before it tests the cooldown.
    #[test]
    fn a_hopper_on_cooldown_still_stamps_the_tick() {
        let tables = TickTables {
            recipes: None,
            fuels: None,
            songs: None,
            shapes: None,
        };
        let hopper = (0, 65, 0);
        let world = Fake::with(&[(hopper, "minecraft:hopper[enabled=true,facing=down]")]);
        let s = store(&[(hopper, Kind::Hopper)]);
        s.with_mut(hopper, |be| {
            if let Some(Behaviour::Hopper(c)) = be.behaviour_mut() {
                c.cooldown = 8;
            }
        });
        let _ = tick_block_entities(&s, &world, &all_chunks(), &tables, 41);
        let Some(Behaviour::Hopper(c)) = s.get(hopper).expect("hopper").behaviour().cloned() else {
            panic!("a hopper has a clock")
        };
        assert_eq!(c.cooldown, 7, "decremented");
        assert_eq!(c.ticked_game_time, 41, "stamped even on cooldown");
    }

    /// A block entity placed this tick first ticks on the next one, because
    /// `pendingBlockEntityTickers` is drained at the top of the pass.
    #[test]
    fn the_ticker_list_takes_a_new_entry_from_pending() {
        let s = crate::blockentity::BlockEntities::new();
        assert_eq!(s.ticker_count(), 0);
        s.insert(
            (0, 64, 0),
            crate::blockentity::BlockEntity::new(Kind::Furnace),
        );
        // Still zero: the entry is in `pending` until a pass drains it.
        assert_eq!(s.ticker_count(), 0);
        assert_eq!(s.ticker_list(), vec![(0, 64, 0)]);
        assert_eq!(s.ticker_count(), 1);
        // A chest is never armed at all — `getTicker`'s server arm is null.
        s.insert(
            (1, 64, 0),
            crate::blockentity::BlockEntity::new(Kind::Chest),
        );
        assert_eq!(s.ticker_list(), vec![(0, 64, 0)]);
    }

    /// A lit campfire with food cooks; the same campfire unlit does not.
    ///
    /// The control is the whole point: `cooldownTick` and `cookTick` are two
    /// different methods off one entry on the ticker list, chosen by a block
    /// state that can change under it, and a pass that always ran one of them
    /// would pass a test that only ever lit the fire.
    #[test]
    fn a_campfire_cooks_only_while_it_is_lit() {
        let recipes = crate::recipes::Recipes::parse(
            &serde_json::json!({
                "baked_potato_from_campfire_cooking": {
                    "type": "minecraft:campfire_cooking",
                    "cookingtime": 3,
                    "ingredient": "minecraft:potato",
                    "result": {"id": "minecraft:baked_potato"}
                }
            }),
            &serde_json::json!({}),
        )
        .expect("recipes");
        let tables = TickTables {
            recipes: Some(&recipes),
            fuels: None,
            songs: None,
            shapes: None,
        };
        for lit in [true, false] {
            let pos = (0, 64, 0);
            let world = Fake::with(&[(
                pos,
                if lit {
                    "minecraft:campfire[facing=north,lit=true]"
                } else {
                    "minecraft:campfire[facing=north,lit=false]"
                },
            )]);
            let s = store(&[(pos, Kind::Campfire)]);
            s.with_mut(pos, |be| {
                be.set_item(0, ItemStack::one("minecraft:potato"));
                if let Some(Behaviour::Campfire(c)) = be.behaviour_mut() {
                    c.time[0] = 3;
                }
            });
            let mut finished = None;
            for t in 1..=4 {
                let out = tick_block_entities(&s, &world, &all_chunks(), &tables, t);
                if !out.drops.is_empty() {
                    finished = Some((t, out.drops[0].1.item().to_string(), out.blocks.clone()));
                    break;
                }
            }
            if lit {
                let (t, item, blocks) = finished.expect("a lit campfire finishes");
                assert_eq!(t, 3, "progress reaches cookingTime on the third tick");
                assert_eq!(item, "minecraft:baked_potato");
                assert_eq!(blocks, vec![pos], "markUpdated writes the position once");
                assert!(s.get(pos).expect("campfire").item(0).is_empty());
            } else {
                assert!(finished.is_none(), "an unlit campfire cooks nothing");
                assert_eq!(
                    s.get(pos).expect("campfire").item(0).item(),
                    "minecraft:potato"
                );
            }
        }
    }

    /// A jukebox stops its song at `length + 20` and not before, and the stop
    /// is a `level_event(1011)` plus `has_record=false`.
    ///
    /// The control is the tick one earlier, which produces nothing: the
    /// finish test runs **before** the increment, so an off-by-one in either
    /// direction moves which tick this fires on.
    #[test]
    fn a_jukebox_stops_twenty_ticks_after_the_song_ends() {
        let songs = crate::jukebox::JukeboxSongs::parse(&serde_json::json!({
            "table": [{
                "song": "minecraft:cat",
                "alphabeticalRank": 0,
                "lengthInTicks": 3,
                "comparatorOutput": 2
            }]
        }))
        .expect("songs");
        let tables = TickTables {
            recipes: None,
            fuels: None,
            songs: Some(&songs),
            shapes: None,
        };
        let pos = (0, 64, 0);
        let world = Fake::with(&[(pos, "minecraft:jukebox[has_record=true]")]);
        let s = store(&[(pos, Kind::Jukebox)]);
        s.with_mut(pos, |be| {
            be.set_item(0, ItemStack::one("minecraft:music_disc_cat"));
            if let Some(Behaviour::Jukebox(p)) = be.behaviour_mut() {
                p.song = Some("minecraft:cat".to_string());
                p.ticks = 22;
            }
        });
        // 22 < 3 + 20, so this tick counts and says nothing.
        let quiet = tick_block_entities(&s, &world, &all_chunks(), &tables, 1);
        assert!(quiet.level_events.is_empty(), "{quiet:?}");
        assert!(quiet.blocks.is_empty());
        // 23 >= 23: the stop.
        let out = tick_block_entities(&s, &world, &all_chunks(), &tables, 2);
        assert_eq!(out.level_events, vec![(1011, pos, 0)]);
        assert_eq!(out.blocks, vec![pos]);
        assert_eq!(
            world.writes().last().expect("a write").1,
            "minecraft:jukebox[has_record=false]"
        );
        assert!(s.get(pos).expect("jukebox").item(0).is_empty());
    }
}
