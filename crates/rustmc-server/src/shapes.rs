//! Cascading block updates: what falls down when the block under it goes.
//!
//! Reference (26.2), read from the jar rather than from wiki notes:
//! `Level.setBlock(BlockPos, BlockState, int, int)`,
//! `BlockBehaviour.BlockStateBase.updateNeighbourShapes`,
//! `BlockBehaviour.UPDATE_SHAPE_ORDER`, `NeighborUpdater.executeShapeUpdate`,
//! `Block.updateOrDestroy`, `Level.destroyBlock` and `Level.removeBlock`.
//!
//! # The two halves of a block update, and which one this is
//!
//! `Level.setBlock` does two different things after it writes, and they are
//! easy to confuse because both are called "a block update":
//!
//!   * **`updateNeighborsAt`**, gated on `UPDATE_NEIGHBORS` (flag 1), which
//!     calls `neighborChanged` on the six neighbours. That is the *redstone*
//!     path — signal strength, lamps, torches, repeaters, comparators,
//!     pistons. It lives in [`crate::power`] as of 2026-08-06; this module
//!     calls it and owns none of it. **Two of the twenty-eight blocks that
//!     override `neighborChanged` are ported** — the wire and the torch — and
//!     `crate::power::execute_update` names the other twenty-six. Until that
//!     date this paragraph said "not ported" and the flag was carried down to
//!     an empty branch.
//!
//!     **This sentence used to say "observers" and it was wrong, which cost a
//!     whole audit.** `ObserverBlock` has no `neighborChanged` at all; nor
//!     does `FallingBlock`. Both hang their behaviour off `updateShape` — the
//!     path below, which *is* ported — as a **side effect** that returns the
//!     block's own state, so the extracted table drops the pair as inert and
//!     the mechanism is invisible. Read from the pinned jar with `javap -c`,
//!     2026-08-04; the detail and the two zero-row measurements are on
//!     [`ShapeTable`]. A doc comment naming the wrong mechanism reads as
//!     evidence to everybody downstream: this one put nine gameplay checks in
//!     one group when the trace says seven belong there.
//!   * **`updateNeighbourShapes`**, gated on `UPDATE_KNOWN_SHAPE` (flag 16)
//!     being *clear*, which asks each neighbour what it becomes now that this
//!     block has changed. That is the path a torch falls down, and it is what
//!     this module implements.
//!
//! The order is vanilla's and it is not `Direction.values()`:
//! `UPDATE_SHAPE_ORDER` is `WEST, EAST, NORTH, SOUTH, DOWN, UP`. It is
//! observable — two blocks that would each destroy the other resolve
//! differently under a different order — so it is copied rather than
//! rationalised.
//!
//! # The direction argument points *back*
//!
//! `updateNeighbourShapes` calls
//! `neighborShapeChanged(d.getOpposite(), pos.relative(d), pos, state, ...)`,
//! and by the time that reaches `updateShape` the block being asked is at
//! `pos.relative(d)` and the direction it is handed is `d.getOpposite()`.
//! From the asked block's point of view the direction therefore points at the
//! block that changed — `neighborPos == pos.relative(direction)` — which is
//! the invariant `ShapeGolden` builds its table on. Getting this backwards
//! makes a wall torch fall off the block it is attached to and survive when
//! its own support goes, which looks like a table fault and is not.
//!
//! # Recursion, and why the limit is carried rather than assumed
//!
//! `setBlock`'s public form passes `recursionLeft = 512`, decrements it at
//! each shape update, and stops when it reaches zero. A column of torches
//! collapses one block per level of recursion, so the limit is a real bound
//! on how far a single dig propagates and not a safety net.

use std::collections::BTreeMap;
use std::io;

use crate::edits::Blocks;
use crate::interact::{BlockPos, Face};

/// `BlockBehaviour.UPDATE_SHAPE_ORDER`. Not `Direction.values()`.
pub const UPDATE_SHAPE_ORDER: [Face; 6] = [
    Face::West,
    Face::East,
    Face::North,
    Face::South,
    Face::Down,
    Face::Up,
];

/// `Block.UPDATE_NEIGHBORS` — run `neighborChanged`, which is
/// [`crate::power::update_neighbors_at`].
pub const UPDATE_NEIGHBORS: i32 = 1;
/// `Block.UPDATE_CLIENTS` — tell the players.
pub const UPDATE_CLIENTS: i32 = 2;
/// `Block.UPDATE_KNOWN_SHAPE` — the caller already did the shape updates.
pub const UPDATE_KNOWN_SHAPE: i32 = 16;
/// `Block.UPDATE_SUPPRESS_DROPS`.
pub const UPDATE_SUPPRESS_DROPS: i32 = 32;
/// `Block.UPDATE_MOVE_BY_PISTON`. Read by `LevelChunk.setBlockState` as
/// `bl2`, which is the `movedByPiston` every `affectNeighborsAfterRemoval`
/// and `onPlace` receives, and one of the two conditions under which the
/// removal hook runs at all.
pub const UPDATE_MOVE_BY_PISTON: i32 = 64;
/// `Block.UPDATE_SKIP_SHAPE_UPDATE_ON_WIRE`.
///
/// The guard is in `NeighborUpdater.executeShapeUpdate` at offsets 9-29 and it
/// is the *only* branch there: a shape update aimed at a `redstone_wire`
/// returns without asking the block anything. Nothing in this server sets it
/// today — vanilla's only setter is the wire power evaluator, which is not
/// ported — so [`execute_shape_update`] honouring it is a no-op that can be
/// measured to be one, and `wire_flag_128_skips_a_shape_update_on_wire` does
/// exactly that.
///
/// Written down rather than left out because leaving it out is invisible: an
/// unported *flag* looks identical to a flag nobody sets, and this one is
/// specifically about the block this link exists to make work.
pub const UPDATE_SKIP_SHAPE_UPDATE_ON_WIRE: i32 = 128;
/// `Block.UPDATE_SKIP_ON_PLACE`, flag 512.
///
/// `LevelChunk.setBlockState` guards its `onPlace` call on
/// `!isClientSide && (flags & 512) == 0` at offsets 514-542. Nothing in this
/// server sets it, so honouring it is a no-op that can be measured to be one —
/// the same reasoning [`UPDATE_SKIP_SHAPE_UPDATE_ON_WIRE`] carries, and the
/// same reason for writing it down: an unported *flag* is indistinguishable
/// from a flag nobody sets, and `onPlace` is where a wire works out its own
/// power.
pub const UPDATE_SKIP_ON_PLACE: i32 = 512;
/// `Block.UPDATE_ALL`, which is what a dig and a place both use.
pub const UPDATE_ALL: i32 = UPDATE_NEIGHBORS | UPDATE_CLIENTS;
/// `Level.setBlock`'s own `recursionLeft`.
pub const RECURSION_LIMIT: i32 = 512;

/// `LevelEvent.PARTICLES_DESTROY_BLOCK` — the block-break effect, carrying
/// `Block.getId(state)` as its data.
pub const LEVEL_EVENT_DESTROY: i32 = 2001;

/// Everything one `Level.setBlock` produced that a client has to be told
/// about.
///
/// # Why the level events travel beside the block updates and not with them
///
/// Vanilla sends the two through different machinery on different schedules
/// and this type exists so a caller cannot accidentally merge them.
/// `ChunkHolder.blockChanged` **records** a position and
/// `ServerChunkCache.tick` broadcasts it at the tick boundary — that is
/// [`Self::blocks`], drained into [`crate::broadcast::ChangedBlocks`].
/// `Level.levelEvent` goes straight to `PlayerList.broadcast` from inside the
/// handler, so it is on the wire **before** the block updates its own write
/// caused. Both halves of that ordering are in the recorded traces of
/// 2026-08-07 02:19: every cascade dig in the corpus shows the `level_event`
/// at or one millisecond before the `section_blocks_update` that carries the
/// same positions.
///
/// # The fields are public and there is no `len()`
///
/// Deliberately. A wrapper with `len`, `is_empty` and `iter` forwarding to the
/// block list would silently answer "nothing happened" for a `set_block` that
/// produced a level event and no visible block change, and every existing
/// `assert!(updates.blocks.is_empty())` in this file would keep passing while meaning
/// something narrower than it says. Naming the half at every call site costs a
/// word and cannot go quietly wrong.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Effects {
    /// `(position, state id)`, in the order vanilla would have sent them:
    /// `sendBlockUpdated` fires before the shape updates, so a caller that
    /// replays the list in order sends the cause before the consequence.
    pub blocks: Vec<(BlockPos, i32)>,
    /// `(event, position, data)` from the **three-argument**
    /// `Level.levelEvent(int, BlockPos, int)` — the overload that takes no
    /// entity and therefore excludes nobody. See [`destroy_block`] for why
    /// that is the only overload this server can produce today.
    pub level_events: Vec<(i32, BlockPos, i32)>,
    /// `Level.playSound`, each carrying its own exclusion. Nothing in this
    /// module produces one — a block's sound belongs to the block's own rule,
    /// not to `setBlock` — but a caller that both writes a block and plays a
    /// sound has one list to hand on rather than two, which is what keeps the
    /// two from being sent at different points of the same handler.
    pub sounds: Vec<crate::sounds::Sound>,
    /// `Level.scheduleTick(pos, block, delay[, priority])`, in the order the
    /// rules asked for it. See [`TickRequest`].
    pub ticks: Vec<TickRequest>,
    /// `FallingBlockEntity.fall`'s `addFreshEntity`, as a request.
    ///
    /// A separate channel from [`Self::blocks`] and **not** interleaved with
    /// it, because the two leave the server in different phases and the order
    /// they leave in is observable. `fall` writes the block first and adds the
    /// entity second, but `ChunkMap.addEntity` pairs *synchronously* while a
    /// block change waits for `broadcastChangedChunks` — so the client sees
    /// `add_entity` and then the `block_update`. The drain site is what
    /// realises that: it spawns everything in this list inside the
    /// scheduled-tick phase, before it flushes the block changes.
    /// `docs/PARITY-ROADMAP.md` §95.
    pub falls: Vec<crate::fallable::FallRequest>,
    /// A dispenser, dropper or crafter's scheduled tick, as a request:
    /// `DispenserBlock.tick` -> `dispenseFrom` and `CrafterBlock.tick` ->
    /// `dispenseFrom`.
    ///
    /// A request for the reason [`Self::falls`] is one: `dispenseFrom` reads
    /// and writes a block entity and adds an entity, and the scheduled-tick
    /// runner is handed only the block world. The drain site, which holds the
    /// block-entity store and the entity tracker, runs it in place — still
    /// inside the scheduled-tick phase, so its block writes coalesce with the
    /// tick's and its `level_event`s and `add_entity` leave immediately, as
    /// vanilla's do. See `crate::dispenser`.
    pub dispenses: Vec<BlockPos>,
    /// `CarvedPumpkinBlock.spawnGolemInWorld`'s `addFreshEntity`, as a
    /// request, for the same reason as [`Self::falls`]: the level events the
    /// clear sent have to leave first, and they are in this struct too. See
    /// [`crate::golem`].
    pub golems: Vec<crate::golem::GolemSpawn>,
    /// `Level.scheduleTick(pos, Fluid, delay)` — the **fluid** queue's
    /// requests, kept apart from [`Self::ticks`] rather than tagged.
    ///
    /// `Level.scheduleTick` is overloaded on `Block` and `Fluid` and the two
    /// registries collide by construction: `minecraft:water` is the id of a
    /// block *and* of a fluid, so a list routed on the string could not tell a
    /// `LiquidBlock` tick from a `WaterFluid` one. The two land in different
    /// queues with different drain positions — see
    /// [`crate::ticks::ScheduledTicks`] — and
    /// [`crate::ticks::drain_effects`] is what puts each list in its own.
    /// [`TickRequest::block`] holds the concrete fluid's id here, and
    /// `priority` is always `NORMAL`.
    pub fluid_ticks: Vec<TickRequest>,
    /// `Level.destroyBlock(pos, true, ...)`'s `Block.dropResources(state,
    /// level, pos, be, null, ItemStack.EMPTY)`, as a request: the block a
    /// cascade destroyed, with the state it had.
    ///
    /// Vanilla rolls it between the `2001` and the air write
    /// (`Level.destroyBlock` offsets 36-45 then 49-81 then 94), and both the
    /// level event and the item's pairing leave the server **synchronously**,
    /// so their order on the wire is the order of the calls. Each request
    /// therefore carries how many [`Self::level_events`] preceded it, and
    /// [`each_event_and_drop`] replays the two lists interleaved.
    pub drops: Vec<DropRequest>,
}

/// One cascade drop. See [`Effects::drops`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropRequest {
    pub pos: BlockPos,
    /// The destroyed state, read before the write — what the loot rolls on.
    pub state: String,
    /// `level_events.len()` when the drop was rolled.
    pub after_events: usize,
}

/// Replay `level_events` and `drops` in the order vanilla emitted them: a
/// drop goes out after the events that preceded it and before the next one.
pub fn each_event_and_drop(
    level_events: &[(i32, BlockPos, i32)],
    drops: &[DropRequest],
    mut event: impl FnMut(i32, BlockPos, i32),
    mut drop: impl FnMut(&DropRequest),
) {
    let mut pending = drops.iter().peekable();
    for (i, &(e, pos, data)) in level_events.iter().enumerate() {
        while let Some(d) = pending.next_if(|d| d.after_events <= i) {
            drop(d);
        }
        event(e, pos, data);
    }
    for d in pending {
        drop(d);
    }
}

/// One `Level.scheduleTick` a block rule asked for, before it has a game time.
///
/// # Why this is a request and not a `crate::ticks::ScheduledTick`
///
/// `ScheduledTick` is keyed on an **absolute** trigger tick, because
/// `ScheduledTick.DRAIN_ORDER` sorts on one. The rules that ask for a tick —
/// `RedstoneTorchBlock.neighborChanged`, `ObserverBlock.updateShape`,
/// `DiodeBlock.checkTickOnNeighbor` — are reached from
/// [`set_block`], which is called from chunk generation, from plugin edits and
/// from four connection handlers, none of which carry `Level.getGameTime()`.
/// Threading a clock into all of them to serve three rules would put a
/// server-lifecycle argument on every geometry function in this module.
///
/// So the delay travels with the request and the **drain site stamps the
/// time**, which is the one place that always has it: the connection handler
/// that owns `s.clock`, or the clock thread that was handed `now`. Vanilla's
/// own ordering survives that: `sub_tick` is stamped by
/// [`crate::ticks::ScheduledTicks::schedule`] in call order, this list is in
/// call order, and every request in one `Effects` is drained under one `now`.
///
/// # There is no `willTickThisTick` flag and that is not an omission
///
/// Three of vanilla's four call sites guard with
/// `LevelTickAccess.willTickThisTick(pos, block)` or `hasScheduledTick(pos,
/// block)` before scheduling. Both guards are redundant against
/// `LevelChunkTicks.schedule`, which drops a second tick for a `(type, pos)`
/// that already has one — `ScheduledTick.UNIQUE_TICK_HASH` — and
/// [`crate::ticks::ScheduledTicks::schedule`] implements exactly that and
/// returns `false` when it fires. So a guarded call and an unguarded one
/// leave the queue in the same state, and the guard is an early-out rather
/// than a condition. Nothing here may rely on that for a rule whose
/// *non-scheduling* half has a side effect; none of the three do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TickRequest {
    pub pos: BlockPos,
    /// The **block**, not the state: `ScheduledTick.type` is the `Block`, and
    /// it is what the queue's uniqueness is keyed on.
    pub block: String,
    /// Ticks from now.
    pub delay: i64,
    /// `TickPriority`'s own ordinal. See [`crate::ticks::PRIORITY_NORMAL`].
    pub priority: u8,
}

impl Effects {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// `BlockState.isAir()` — the three air blocks, not just the one.
///
/// Carving replaces stone with `cave_air`, so a cascade that only recognised
/// `minecraft:air` would call a hollowed-out section solid and take the
/// "set a block" branch of `updateOrDestroy` where vanilla takes "destroy".
#[must_use]
pub fn is_air(state: &str) -> bool {
    matches!(
        state.split('[').next().unwrap_or(state),
        "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
    )
}

/// One `(state, direction)` pair the world can change.
#[derive(Debug, Clone)]
struct Row {
    /// What the state becomes. Air for everything in the table today; a
    /// string rather than a flag because `ShapeGolden` emits whatever vanilla
    /// returned and nothing here needs it to be air.
    to: String,
    /// Index into [`ShapeTable::classes`] of the neighbour states that
    /// produce [`Self::to`].
    class: usize,
}

/// `BlockState.updateShape`, from the table `ci/shape-golden.sh` extracted.
///
/// See `tools/goldengen/vanilla/ShapeGolden.java` for what it can and cannot
/// answer. The short version: it carries the pairs whose only alternative
/// answer is one other state, and refuses anything with several. Connection
/// properties with more than two values — a stair choosing an inner or outer
/// corner, a rail its curve, a chest its half — are counted in `connective`
/// (11,621 pairs) and not carried, which is the same gap
/// `data/vanilla/placement.json` names from the other side as its 195
/// neighbour-sensitive blocks.
///
/// # A fence is NOT one of them, and this comment said it was
///
/// Until 2026-08-04 the sentence above read "Connection properties (a fence
/// choosing which way to join, a stair its corner) are counted and not
/// carried". **The fence half was wrong, and wrong by this very rule.** A
/// fence's join is one boolean per direction, so a `(state, direction)` pair
/// answers with the block's own state and exactly one other — which is the
/// emission rule, not the refusal rule. `update_shape.json` carries **all 128
/// `oak_fence` pairs** and **zero `oak_stairs` pairs**, so
/// `140-fences-connect-to-each-other` matches and
/// `180-stairs-make-a-corner-when-two-meet` diverges, and the trace shows
/// both halves of a fence connection — the placed block's own `west=true` and
/// the existing neighbour's `east=true` — coming back byte-identical to
/// vanilla's.
///
/// The cost of the wrong example was not cosmetic. A static audit of the 22
/// diverging checks quoted it, grouped four diverging connection checks with
/// the passing fence one, and could not explain why the fence passed — because
/// there is no fence-specific code to find and the doc comment said the
/// generic path could not do it either. **A comment that describes an
/// extraction has to obey the extraction's own stated rule**, or it sends the
/// next reader looking for a mechanism that is not there.
///
/// ~~`ShapeGolden.java`'s own header carries the identical wrong example.~~
/// **Closed 2026-08-18** on `tables-refusal-ledger`, which regenerates the
/// table anyway, so the provenance stamp this deferral was protecting is
/// refreshed in the same breath. The header now carries the corrected example
/// and the reason the wrong one cost an audit.
///
/// # 79,113 refused is not 79,113 owed, and this comment used to imply it was
///
/// `refusedPairs` is the largest number the extraction produces and it is not
/// a measure of what this module owes. `ShapeGolden`'s discovery pass breaks
/// out of its candidate loop on the **first** throw, so a pair is recorded as
/// refused before anything establishes that its answer ever moves.
/// `ci/checks/shape-refusal-census.json` takes it apart, re-deriving the
/// refusal set with `ShapeGolden`'s own probe so the two cannot drift:
///
/// ```text
///   48081  60.8%  world         needs a world, not a wider harness
///   22010  27.8%  recoverable   one alternative, invariant under 3 fillers
///    5609   7.1%  connective    already excluded by design, counted twice
///    3089   3.9%  inert         confirmed over ALL 32,366 neighbour states
///     324   0.4%  refused       getLightEngine / environmentAttributes /
///                               registryAccess — a real level, not a wider one
/// ```
///
/// **The cascade this module cannot do is 48,405 pairs, not 79,113**, and it
/// is concentrated rather than diffuse: `WallBlock` is 41,472 of it on its own,
/// `StairBlock` 19,200 (14,400 of them recoverable), and with `FireBlock` and
/// `NoteBlock` that is 84% of the whole refusal population. 54,724 of the
/// 79,113 read exactly one position past their own neighbour and 43,021 read
/// `0,1,0` and nothing else, so the shape of the fix is a three-position
/// extract rather than a live level.
///
/// **Nothing here may open that census.** Its level invents the world past the
/// neighbour, and a row derived from an invented world is the plausible answer
/// this project refuses. It is a claim about the jar, not an extract.
///
/// # The 164 that were in no count at all, and 160 of them are chests
///
/// Those three counts partitioned what **discovery found**, and until
/// 2026-08-18 that was not the game. Discovery swept the 1,196 block defaults,
/// and a pair it found nothing for was dropped before the exhaustive pass — no
/// row, no refusal, no place in any count — so a reader adding
/// `carried + connective + refused` and getting `sensitive` would conclude the
/// table had accounted for everything. **164 pairs moved that the default
/// breadth dropped**: `CopperChestBlock` 64, `WeatheringCopperChestBlock` 64,
/// `ChestBlock` 16, `TrappedChestBlock` 16, `PointedDripstoneBlock` 2,
/// `SulfurSpikeBlock` 2. The comment above names "a chest its half" as a
/// connection property this module does not carry; it was worse than that,
/// because those pairs were not in the `connective` count either. Two chests
/// placed side by side becoming one double chest is a mechanic a player sees,
/// and the pair that decides it fires only against a neighbour state no
/// block's default carries.
///
/// **Discovery reads all 32,366 states now** and the 164 are in the counts:
/// 32 carried, 130 connective, 2 refused, by subtraction between the two
/// re-derived tables. `ShapeReadsGolden` still measures what the old breadth
/// would drop, as `missedAtDefaults`, because a number that can only be zero
/// once its cause is fixed has stopped being able to fail — and
/// `ci/refusal-ledger.py` now checks that `discoveryMissed` is 0, which is the
/// difference between a partition that balances and one that is exhaustive.
///
/// **The 2 refusals are the part worth reading.** `discoveryLateRefusal` was 0
/// and was read as "the refusal set will not move". Discovery stops at the
/// first candidate that moves the answer; resolution sweeps from the beginning
/// and can meet a throw discovery stopped short of, and a pair discovery never
/// found cannot reach resolution at all. A count of what one pass refuses is
/// not a bound on what a later pass will refuse.
///
/// That 2 was arrived at by **subtracting two re-derived tables**, which is a
/// weaker thing than it reads as: it needs both runs to exist, a tree holding
/// one cannot check it, and whatever else moved between the two runs is
/// charged to the widening without being asked. `ShapeReadsGolden` measures it
/// directly now, as `resolutionRefusalAtDefaults`, in one run — and it is
/// deliberately not on `ci/refusal-ledger.py`'s must-be-zero list, because it
/// is 2 and the count sitting beside it that genuinely is 0
/// (`lateRefusalAtDefaults`) is about the other pass. Filing them together is
/// the confusion this paragraph exists to correct.
///
/// # What the table cannot see at all: a shape answer that is a side effect
///
/// Discovery drops a `(state, direction)` pair "whose answer is never anything
/// but itself". That is right for a pure state function and it is **blind to a
/// `updateShape` override that returns its own state and schedules a tick**,
/// which is how two of vanilla's mechanics work:
///
///   * `ObserverBlock.updateShape` — `if (FACING == direction && !POWERED)
///     startSignal(...)`, then `return super.updateShape(...)`. Zero
///     `minecraft:observer` rows in the table.
///   * `FallingBlock.updateShape` — `scheduleTick(pos, this,
///     getDelayAfterPlace())`, then `return super.updateShape(...)`. Zero
///     `minecraft:sand` rows.
///
/// Both were read from the pinned jar with `javap -c` on 2026-08-04. Neither
/// block overrides `neighborChanged` at all, so
/// `440-an-observer-pulses-at-the-block-it-faces` and
/// `070-sand-falls-when-unsupported` are **not** waiting on the redstone path
/// described above — they are waiting on this hole plus a tick behaviour, and
/// 070 additionally on an entity broadcast that does not exist anywhere in
/// this server.
#[derive(Debug, Clone)]
pub struct ShapeTable {
    /// State to its six directions, indexed by [`Face::to_3d`].
    rows: BTreeMap<String, [Option<Row>; 6]>,
    /// Bitmaps over the global block-state id, one bit per state.
    classes: Vec<Vec<u8>>,
    states: usize,
    connective: usize,
    refused: usize,
    /// Every block id vanilla would answer `instanceof DoorBlock` for.
    ///
    /// A **set of ids**, resolved once from the extract's class hierarchy,
    /// rather than a class name compared per lookup. `DoorBlock.updateShape`
    /// tests `instanceof`, and the four unwaxed copper doors are
    /// `WeatheringCopperDoorBlock` — a simple-name compare gives those four no
    /// rule at all, and the corpus only ever places an oak door, so nothing
    /// would catch it.
    ///
    /// Empty until [`Self::with_classes`] is called, and then nothing typed
    /// fires.
    doors: std::collections::BTreeSet<String>,
    /// Every block id vanilla would answer `instanceof BedBlock` for.
    ///
    /// Separate from [`Self::doors`] because the two rules are **not** the
    /// same rule with a different property — see [`Self::typed_update_shape`].
    beds: std::collections::BTreeSet<String>,
    /// Every block id vanilla would answer `instanceof FallingBlock` for.
    ///
    /// Its rule is not a shape answer at all — `FallingBlock.updateShape`
    /// schedules a tick and returns `super`'s state — so it lives beside the
    /// observer's in [`execute_shape_update`] rather than in the table. See
    /// [`crate::fallable`].
    gravity_blocks: std::collections::BTreeSet<String>,
    /// Every block id vanilla answers `instanceof DoublePlantBlock` for,
    /// which includes the four `TallFlowerBlock`s.
    ///
    /// Used by [`player_will_destroy`] and **not** by
    /// [`Self::typed_update_shape`]: the double plant's `updateShape` is the
    /// one of the three the extracted table can hold, because its only
    /// non-self answer is air and a row holds that. What the table cannot
    /// hold is what happens *before* the block goes, which is not a shape
    /// update at all.
    double_plants: std::collections::BTreeSet<String>,
    /// Every block id vanilla answers `instanceof StairBlock` for.
    ///
    /// Held here as well as on [`crate::placement::PlacementTable`] rather
    /// than shared, for the reason `PlacedByTable::class_pairs` gives: a
    /// table that owns its class sets answers its own rules without reaching
    /// through a second table that could have been loaded and this one not.
    /// The two are built by the same `is_a` call from the same file.
    stairs: std::collections::BTreeSet<String>,
    /// Every block id vanilla answers `instanceof ChestBlock` for.
    chests: std::collections::BTreeSet<String>,
    /// Every block id vanilla answers `instanceof BaseRailBlock` for — the
    /// class half of `BaseRailBlock.isRail`. See `crate::rails` for the tag
    /// half, which is not carried.
    rails: std::collections::BTreeSet<String>,
    /// Of those, the ones whose `isStraight()` is **true** and which
    /// therefore never take a corner: everything but `RailBlock`.
    straight_rails: std::collections::BTreeSet<String>,
}

/// The block id a state string names, without its properties.
fn block_of(state: &str) -> &str {
    state.split('[').next().unwrap_or(state)
}

/// `DoorBlock`'s simple name, as `PlacedGolden` records it.
const DOOR_BLOCK: &str = "DoorBlock";
/// `BedBlock`'s. All 16 beds are plain `BedBlock`s in 26.2; matched with
/// `is_a` anyway, for the reason `crate::ticks` gives about the button.
const BED_BLOCK: &str = "BedBlock";
/// `DoublePlantBlock`'s. Matched with `is_a` and it **has** to be: of the
/// seven blocks in the family, `sunflower`, `lilac`, `rose_bush` and `peony`
/// are `TallFlowerBlock`, a subclass, and only `tall_grass`, `large_fern`
/// and `pitcher_plant` are the class itself.
const DOUBLE_PLANT_BLOCK: &str = "DoublePlantBlock";
/// `BaseRailBlock`'s. Matched with `is_a`, which is the point: all four rails
/// are subclasses and none is the class itself.
const RAIL_BLOCK: &str = "BaseRailBlock";
/// `RailBlock`'s — the plain rail, and the **only** one of the four whose
/// `isStraight()` is false. The straight set is `BaseRailBlock` minus this.
const PLAIN_RAIL_BLOCK: &str = "RailBlock";

/// The one shape table this process serves, with its class map attached.
///
/// Two loads in one place rather than at each of the two call sites, because
/// a table loaded *without* the classes answers every typed rule with `None`
/// and looks exactly like a table that has them — a door's upper half simply
/// stops following its lower one, silently, on whichever path forgot.
///
/// `root` is the directory `data/vanilla` sits under — `.` for a server
/// booted from the repo, `../..` for an integration test whose working
/// directory is the crate. It was a hardcoded relative path until
/// 2026-08-10, which meant the only code that could reach this table was code
/// running from the repo root, which meant no test could reach it at all.
/// See [`crate::session_tables`].
///
/// The table is still loaded exactly once per process: the first caller's
/// `root` wins, as it must for a `OnceLock`, and in both real configurations
/// there is only one caller.
#[must_use]
pub fn shared_with_classes(root: &std::path::Path) -> Option<&'static ShapeTable> {
    static TABLE: std::sync::OnceLock<Option<ShapeTable>> = std::sync::OnceLock::new();
    TABLE
        .get_or_init(|| {
            let table = ShapeTable::load(&root.join("data/vanilla/update_shape.json"));
            let classes = crate::placedby::PlacedByTable::shared(
                &root.join("data/vanilla/set_placed_by.json"),
            );
            match (table, classes) {
                (Ok(t), Ok(c)) => Some(t.with_classes(c)),
                (Ok(t), Err(e)) => {
                    eprintln!(
                        "shape table has no class map ({e}) — a door's upper half will not \
                         follow its lower one"
                    );
                    Some(t)
                }
                (Err(e), _) => {
                    eprintln!("no shape table ({e}) — nothing cascades; torches will float");
                    None
                }
            }
        })
        .as_ref()
}

impl ShapeTable {
    /// Load `data/vanilla/update_shape.json`.
    ///
    /// # Errors
    /// If the file is missing, is not the expected shape, or was dumped in a
    /// direction order this build does not index by.
    pub fn load(path: &std::path::Path) -> io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let doc: serde_json::Value = serde_json::from_str(&text).map_err(io::Error::other)?;
        // The column order is checked, not assumed. A dump in
        // `Direction.values()` order and a port indexing by `get3DDataValue`
        // agree today because those are the same six; a transposition would
        // be silent everywhere except in which face a torch clings to.
        let order: Vec<&str> = doc
            .get("directionOrder")
            .and_then(serde_json::Value::as_array)
            .map(|a| a.iter().filter_map(serde_json::Value::as_str).collect())
            .unwrap_or_default();
        if order != ["DOWN", "UP", "NORTH", "SOUTH", "WEST", "EAST"] {
            return Err(io::Error::other(format!(
                "update_shape.json is in direction order {order:?}, not Direction.values()"
            )));
        }
        let states = doc
            .get("states")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| io::Error::other("update_shape.json has no `states`"))?;
        let states = usize::try_from(states).map_err(io::Error::other)?;

        let mut classes = Vec::new();
        for entry in doc
            .get("classes")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| io::Error::other("update_shape.json has no `classes`"))?
        {
            let encoded = entry
                .as_str()
                .ok_or_else(|| io::Error::other("a neighbour set is not a string"))?;
            let bits = decode_base64(encoded)
                .ok_or_else(|| io::Error::other("a neighbour set is not base64"))?;
            if bits.len() * 8 < states {
                return Err(io::Error::other(format!(
                    "a neighbour set covers {} states, not {states}",
                    bits.len() * 8
                )));
            }
            classes.push(bits);
        }

        let mut rows: BTreeMap<String, [Option<Row>; 6]> = BTreeMap::new();
        for entry in doc
            .get("table")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| io::Error::other("update_shape.json has no `table`"))?
        {
            let (Some(state), Some(dir), Some(to), Some(class)) = (
                entry.get("state").and_then(serde_json::Value::as_str),
                entry.get("dir").and_then(serde_json::Value::as_u64),
                entry.get("to").and_then(serde_json::Value::as_str),
                entry.get("class").and_then(serde_json::Value::as_u64),
            ) else {
                return Err(io::Error::other("an update_shape row is missing a field"));
            };
            let dir = usize::try_from(dir).map_err(io::Error::other)?;
            let class = usize::try_from(class).map_err(io::Error::other)?;
            if dir >= 6 || class >= classes.len() {
                return Err(io::Error::other(format!(
                    "row {state} names direction {dir} and class {class}"
                )));
            }
            rows.entry(state.to_string()).or_default()[dir] = Some(Row {
                to: to.to_string(),
                class,
            });
        }
        if rows.is_empty() {
            return Err(io::Error::other("update_shape.json carries no rows"));
        }
        Ok(Self {
            rows,
            classes,
            states,
            doors: std::collections::BTreeSet::new(),
            beds: std::collections::BTreeSet::new(),
            double_plants: std::collections::BTreeSet::new(),
            gravity_blocks: std::collections::BTreeSet::new(),
            stairs: std::collections::BTreeSet::new(),
            chests: std::collections::BTreeSet::new(),
            rails: std::collections::BTreeSet::new(),
            straight_rails: std::collections::BTreeSet::new(),
            connective: usize::try_from(
                doc.get("connective")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0),
            )
            .unwrap_or(0),
            refused: usize::try_from(
                doc.get("refusedPairs")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0),
            )
            .unwrap_or(0),
        })
    }

    /// The one table this process serves, loaded once.
    ///
    /// Three megabytes of JSON, and the play loop is entered per connection —
    /// so this is a `OnceLock` rather than a load per join, which is what the
    /// placement table does and what a second player would have paid for.
    ///
    /// # Errors
    /// Whatever [`Self::load`] returned the first time, repeated.
    pub fn shared(path: &std::path::Path) -> Result<&'static Self, String> {
        static TABLE: std::sync::OnceLock<Result<ShapeTable, String>> = std::sync::OnceLock::new();
        TABLE
            .get_or_init(|| Self::load(path).map_err(|e| e.to_string()))
            .as_ref()
            .map_err(Clone::clone)
    }

    /// Attach vanilla's class-per-block map, which `set_placed_by.json`
    /// carries for the whole registry.
    ///
    /// Without it [`Self::typed_update_shape`] answers nothing, and a door's
    /// upper half neither follows its lower one nor goes when it goes.
    #[must_use]
    pub fn with_classes(self, blocks: &crate::placedby::PlacedByTable) -> Self {
        let ids = blocks.class_pairs();
        let of = |class: &str| -> std::collections::BTreeSet<String> {
            ids.keys()
                .filter(|b| blocks.is_a(b, class))
                .cloned()
                .collect()
        };
        self.with_doors(of(DOOR_BLOCK))
            .with_beds(of(BED_BLOCK))
            .with_double_plants(of(DOUBLE_PLANT_BLOCK))
            .with_stairs(of(crate::placement::STAIR_BLOCK))
            .with_chests(of(crate::placement::CHEST_BLOCK))
            .with_gravity_blocks(of(crate::fallable::FALLING_BLOCK_CLASS))
            .with_rails(
                of(RAIL_BLOCK),
                of(RAIL_BLOCK)
                    .difference(&of(PLAIN_RAIL_BLOCK))
                    .cloned()
                    .collect(),
            )
    }

    /// As [`Self::with_doors`], for `StairBlock`.
    #[must_use]
    pub fn with_stairs(mut self, stairs: std::collections::BTreeSet<String>) -> Self {
        self.stairs = stairs;
        self
    }

    /// `StairBlock.isStairs(BlockState)`.
    #[must_use]
    pub fn is_stairs(&self, state: &str) -> bool {
        self.stairs.contains(block_of(state))
    }

    /// As [`Self::with_doors`], for `FallingBlock`.
    ///
    /// A precomputed set for the same reason the doors are one: sand is a
    /// `SandBlock`, gravel a `ColoredFallingBlock` and concrete powder a
    /// `ConcretePowderBlock`, so a simple-name compare against `FallingBlock`
    /// gives **no** gravity block a rule at all. See [`crate::fallable`].
    #[must_use]
    pub fn with_gravity_blocks(mut self, blocks: std::collections::BTreeSet<String>) -> Self {
        self.gravity_blocks = blocks;
        self
    }

    /// Whether the block a state names is a `FallingBlock`.
    ///
    /// Empty when the table was loaded without classes, which makes this
    /// `false` for everything — the same "a block we cannot ask about is not
    /// that class" convention as every other typed rule here, and the safe
    /// direction for a rule that removes a block from the world.
    #[must_use]
    pub fn is_gravity_block(&self, state: &str) -> bool {
        self.gravity_blocks.contains(block_of(state))
    }

    /// As [`Self::with_stairs`], for `ChestBlock`.
    #[must_use]
    pub fn with_chests(mut self, chests: std::collections::BTreeSet<String>) -> Self {
        self.chests = chests;
        self
    }

    /// Whether the block a state names is a `ChestBlock`.
    #[must_use]
    pub fn is_chest(&self, state: &str) -> bool {
        self.chests.contains(block_of(state))
    }

    /// As [`Self::with_stairs`], for `BaseRailBlock` and the straight subset.
    #[must_use]
    pub fn with_rails(
        mut self,
        rails: std::collections::BTreeSet<String>,
        straight: std::collections::BTreeSet<String>,
    ) -> Self {
        self.rails = rails;
        self.straight_rails = straight;
        self
    }

    /// The class half of `BaseRailBlock.isRail(BlockState)`.
    #[must_use]
    pub fn is_rail(&self, state: &str) -> bool {
        self.rails.contains(block_of(state))
    }

    /// `BaseRailBlock.isStraight()` — true for every rail but `RailBlock`.
    #[must_use]
    pub fn is_straight_rail(&self, state: &str) -> bool {
        self.straight_rails.contains(block_of(state))
    }

    /// `updateShape` for the blocks that go back to **the level** for their
    /// answer instead of deciding it from the neighbour state they were
    /// handed.
    ///
    /// Both tenants read more of the world than one neighbour; only the stair
    /// ignores the handed state entirely. The wire's `DOWN` branch does use
    /// it — see the second tenant below — which is why the pair is passed
    /// rather than dropped from the signature.
    ///
    /// # Why this is a third hook and not a widening of the other two
    ///
    /// [`Self::update_shape`] is a table keyed on `(state, direction,
    /// neighbour-state)` and [`Self::typed_update_shape`] is a hand port with
    /// the same three inputs. `StairBlock.updateShape` has a different
    /// signature in the only sense that matters: it **ignores the neighbour
    /// state it is handed** — `aload 7` is never read — and calls
    /// `getStairsShape(state, level, pos)` at offsets 49-53, which goes back
    /// to the world for two *different* neighbours. No function of one
    /// neighbour can express it, which is why `update_shape.json` carries
    /// zero `oak_stairs` rows and why widening the extractor would emit rows
    /// that are wrong rather than missing.
    ///
    /// ```java
    /// // StairBlock.updateShape, offsets 0-80
    /// if (state.getValue(WATERLOGGED))                       //  0-13
    ///     scheduleTick(pos, WATER, WATER.getTickDelay(level));  // 16-33
    /// if (direction.getAxis().isHorizontal())                // 34-42
    ///     return state.setValue(SHAPE,                       // 45-62
    ///         getStairsShape(state, level, pos));
    /// return super.updateShape(...);                         // 63-80
    /// ```
    ///
    /// The vertical case falls through to `Block.updateShape`, which returns
    /// the state unchanged — so `None` here, and a stair does not re-corner
    /// when the block above or below it changes. That asymmetry is vanilla's
    /// and is the difference between a stair and a wall.
    ///
    /// The waterlogged fluid tick at offsets 16-33 is **not** ported here.
    /// It is `crate::ticks`' queue and this hook returns a state; scheduling
    /// it from a function typed to answer "what does this block become" would
    /// put the effect on the wrong side of the seam. Named rather than left
    /// to be discovered: a waterlogged stair does not re-tick its water here.
    ///
    /// # The second tenant, and why it needed two more arguments
    ///
    /// `RedStoneWireBlock.updateShape` is the other rule in this shape, and it
    /// wants more of the call than the stair does: its `DOWN` branch asks
    /// `canSurviveOn(level, neighbourPos, neighbourState)` about the block that
    /// changed, not about itself. Both are therefore passed rather than
    /// re-derived.
    ///
    /// `neighbour_pos` is always `direction.relative(pos)` on every path this
    /// server has — the invariant the module docs open with, and it holds for
    /// the diagonal walk in [`update_indirect_neighbour_shapes`] too, which was
    /// checked rather than assumed: its `DOWN` probe notifies `pos+d+DOWN` from
    /// `pos+DOWN`, and its `UP` probe `pos+d+UP` from `pos+UP`, each of which is
    /// one step along the direction handed over. It is passed anyway because
    /// vanilla passes it, and a rule that re-derives an argument it was given is
    /// a rule that cannot be wrong in the same way vanilla is.
    #[must_use]
    pub fn level_update_shape(
        &self,
        blocks: &dyn Blocks,
        pos: BlockPos,
        state: &str,
        direction: Face,
        neighbour_pos: BlockPos,
        neighbour_state: &str,
    ) -> Option<String> {
        // `WallBlock.updateShape` — the one rule here that also reads the
        // block **above** and its collision shape. See `crate::wall`.
        let wall = crate::wall::live(&|s| blocks.state_id(s), |f| {
            crate::wall::is_wall(f, state).then(|| {
                let around = crate::placement::Around {
                    pos,
                    at: &|p| blocks.state_at(p),
                    sturdy: &|p, f| blocks.face_sturdy(p, f),
                    conductor: &|p| blocks.redstone_conductor(p),
                    source: &|p| blocks.signal_source(p),
                    instrument: &|p| blocks.note_block_instrument(p),
                    state_id: &|s| blocks.state_id(s),
                };
                crate::wall::update_shape(f, state, direction, neighbour_state, &around)
            })
        })
        .flatten();
        if let Some(answer) = wall {
            let _ = neighbour_pos;
            return answer;
        }
        // `NoteBlock.updateShape`, offsets 0-31: a change **above or below**
        // re-runs `setInstrument(level, pos, state)` — which is how a mob head
        // placed on top becomes the instrument; a side change falls to
        // `Block.updateShape` and keeps the state. `setInstrument` reads the
        // two vertical neighbours, which is what
        // `Blocks::note_block_instrument` answers; `None` from it leaves the
        // state alone. Measured on `890`: vanilla rewrites the note block to
        // `instrument=skeleton` in the same `section_blocks_update` as the
        // skull; this server left it `harp`.
        if crate::noteblock::is_note_block(state) {
            if !matches!(direction, Face::Up | Face::Down) {
                return None;
            }
            let instrument = blocks.note_block_instrument(pos)?;
            let to = crate::placement::with_property(state, "instrument", &instrument);
            return (to != state).then_some(to);
        }
        let stairs = self.is_stairs(state) && direction.axis_is_horizontal();
        let wire = crate::wire::is_wire(state);
        if !stairs && !wire {
            return None;
        }
        let around = crate::placement::Around {
            pos,
            at: &|p| blocks.state_at(p),
            sturdy: &|p, f| blocks.face_sturdy(p, f),
            conductor: &|p| blocks.redstone_conductor(p),
            source: &|p| blocks.signal_source(p),
            instrument: &|p| blocks.note_block_instrument(p),
            state_id: &|s| blocks.state_id(s),
        };
        if wire {
            return crate::wire::update_shape(
                &around,
                pos,
                state,
                direction,
                neighbour_pos,
                neighbour_state,
            );
        }
        let shape = crate::placement::stairs_shape(&|s| self.is_stairs(s), state, &around)?;
        let to = crate::placement::with_property(state, "shape", shape);
        (to != state).then_some(to)
    }

    /// The same, from the resolved set.
    #[must_use]
    pub fn with_doors(mut self, doors: std::collections::BTreeSet<String>) -> Self {
        self.doors = doors;
        self
    }

    /// As [`Self::with_doors`], for `BedBlock`.
    #[must_use]
    pub fn with_beds(mut self, beds: std::collections::BTreeSet<String>) -> Self {
        self.beds = beds;
        self
    }

    /// As [`Self::with_doors`], for `DoublePlantBlock` and its subclasses.
    #[must_use]
    pub fn with_double_plants(mut self, plants: std::collections::BTreeSet<String>) -> Self {
        self.double_plants = plants;
        self
    }

    /// Whether the block a state names is a `DoublePlantBlock`, subclasses
    /// included.
    #[must_use]
    pub fn is_double_plant(&self, state: &str) -> bool {
        self.double_plants.contains(block_of(state))
    }

    /// The rules a row structurally cannot hold.
    ///
    /// # Why this is not in the table
    ///
    /// A row is **one `to` per neighbour class**: given the subject state and
    /// a set of neighbour state ids, one answer. That expresses "become air"
    /// and "become this one other state", which is the destruction family and
    /// most of the game. It cannot express an answer that is a *function of
    /// the neighbour's state*, and `DoorBlock.updateShape` is exactly that:
    ///
    /// ```text
    ///   half = state.getValue(HALF)
    ///   if (dir.getAxis() == Y && (half == LOWER) == (dir == UP)):
    ///       return nb.getBlock() instanceof DoorBlock && nb.getValue(HALF) != half
    ///            ? nb.setValue(HALF, half)      // the WHOLE neighbour state
    ///            : AIR
    /// ```
    ///
    /// Measured, not assumed: `update_shape.json` carries **32 rows** for
    /// `oak_door`, all `half=lower, dir=DOWN` — the lost-its-support rule —
    /// and **zero** for `half=upper`. Nor is the pair in the dump's `refused`
    /// list, whose eleven buckets name no door at all, so nothing in the
    /// extract's own diagnostics says the gap is there. It reads as silence.
    ///
    /// # Why one class and not a family
    ///
    /// Because the family disagrees, which was read rather than guessed:
    ///
    /// ```text
    ///   DoorBlock         nb.setValue(HALF, half)     — the whole state
    ///   DoublePlantBlock  AIR if the other half is gone, else no change
    ///   BedBlock          copies only OCCUPIED, else AIR
    /// ```
    ///
    /// A single "double block" rule would be right about one of the three. So
    /// this carries the door and names the other two rather than generalising
    /// over them.
    ///
    /// Note `instanceof DoorBlock`, **not** "the same block": vanilla lets an
    /// oak upper half take a spruce lower half's state, and the result is the
    /// *neighbour's* block. Reproduced rather than tidied.
    #[must_use]
    pub fn typed_update_shape(
        &self,
        state: &str,
        direction: Face,
        neighbour: &str,
    ) -> Option<String> {
        if self.beds.contains(block_of(state)) {
            return self.bed_update_shape(state, direction, neighbour);
        }
        if self.is_chest(state) {
            return Self::chest_update_shape(state, direction, neighbour);
        }
        // `BambooSaplingBlock.updateShape`, offsets 17-42: a sapling with
        // bamboo **above** it becomes `BAMBOO.defaultBlockState()` — which is
        // how a bone-mealed or grown sapling turns into the stalk's foot. The
        // `canSurvive` arm before it (offsets 0-16) is the table's `DOWN`
        // row; this arm is neighbour-dependent, so the table cannot hold it.
        if block_of(state) == "minecraft:bamboo_sapling"
            && direction == Face::Up
            && block_of(neighbour) == "minecraft:bamboo"
        {
            return Some("minecraft:bamboo[age=0,leaves=none,stage=0]".to_string());
        }
        // `GrowingPlantHeadBlock.updateShape`, offsets 86-128: a head whose
        // neighbour in its growth direction is the same head or its body
        // becomes `getBodyBlock().defaultBlockState()`, passed through
        // `updateBodyAfterConvertedFromHead` — identity, except
        // `CaveVinesBlock`'s, which carries `BERRIES` across.
        if let Some((dir, _, body)) = crate::growth::plant_head(block_of(state)) {
            let toward = if dir > 0 { Face::Up } else { Face::Down };
            if direction == toward
                && (block_of(neighbour) == block_of(state) || block_of(neighbour) == body)
            {
                return Some(if body == "minecraft:cave_vines_plant" {
                    let berries =
                        crate::placement::property_of(state, "berries").unwrap_or("false");
                    format!("{body}[berries={berries}]")
                } else {
                    body.to_string()
                });
            }
        }
        if !self.is_door(state) {
            return None;
        }
        let half = crate::placement::property_of(state, "half")?;
        // `(half == LOWER) == (direction == UP)`, and the direction here
        // already points at the block that changed.
        if !matches!(direction, Face::Up | Face::Down) {
            return None;
        }
        if (half == "lower") != (direction == Face::Up) {
            return None;
        }
        let other = crate::placement::property_of(neighbour, "half");
        if self.is_door(neighbour) && other.is_some_and(|o| o != half) {
            return Some(crate::placement::with_property(neighbour, "half", half));
        }
        Some("minecraft:air".to_string())
    }

    /// `ChestBlock.updateShape`, offsets 0-178 — the half of check `240`
    /// that rewrites the chest **nobody clicked**.
    ///
    /// ```java
    /// if (chestCanConnectTo(nb) && direction.getAxis().isHorizontal()) {  // 34-51
    ///     ChestType nbType = nb.getValue(TYPE);                           // 54-65
    ///     if (state.getValue(TYPE) == SINGLE                              // 67-77
    ///         && nbType != SINGLE                                         // 80-85
    ///         && state.getValue(FACING) == nb.getValue(FACING)            // 88-103
    ///         && getConnectedDirection(nb) == direction.getOpposite())    // 106-116
    ///         return state.setValue(TYPE, nbType.getOpposite());          // 119-134
    /// } else if (getConnectedDirection(state) == direction) {             // 138-144
    ///     return state.setValue(TYPE, SINGLE);                            // 147-160
    /// }
    /// return super.updateShape(...);                                      // 161-178
    /// ```
    ///
    /// # This one IS a function of the neighbour state, and that is why it is here
    ///
    /// Every input above comes from `aload 7` or from the subject's own
    /// state: `chestCanConnectTo`, the neighbour's `TYPE`, its `FACING` and
    /// its `getConnectedDirection`. Nothing reads the level. So the chest
    /// belongs beside the door and the bed in this hook, and **not** in
    /// [`Self::level_update_shape`] where the stair went — the two look alike
    /// from the check list and are different in the only way that matters.
    ///
    /// It is here rather than in the extracted table for the reason
    /// `update_shape.json` states about itself: a `SINGLE` chest's answer to
    /// one neighbour is `LEFT` *or* `RIGHT` depending on that neighbour's
    /// own type, and a `Row` holds one `to` per class. Two alternatives is
    /// `connective`, and connective pairs are counted and refused.
    ///
    /// **`ChestType.getOpposite()` is `LEFT <-> RIGHT` and `SINGLE ->
    /// SINGLE`**, read from the enum's own tableswitch (ordinal 0 SINGLE,
    /// 1 LEFT, 2 RIGHT; the arms return SINGLE, RIGHT, LEFT). The
    /// `SINGLE` arm is unreachable from here — offset 80 has already
    /// excluded it — and is written out anyway so the function is total.
    ///
    /// The `else` branch is what un-pairs a chest whose partner was dug out:
    /// a half whose `getConnectedDirection` points at the direction that
    /// changed goes back to `SINGLE`. It is reached when the neighbour is
    /// **not** a connectable chest, which is exactly the case where the
    /// partner has just become air.
    fn chest_update_shape(state: &str, direction: Face, neighbour: &str) -> Option<String> {
        // `chestCanConnectTo(nb)` is `nb.is(this)` — the same block, so a
        // trapped chest is not a candidate for a plain one.
        if block_of(neighbour) == block_of(state) && direction.axis_is_horizontal() {
            let nb_type = crate::placement::property_of(neighbour, "type")?;
            if crate::placement::property_of(state, "type") == Some("single")
                && nb_type != "single"
                && crate::placement::property_of(state, "facing")
                    == crate::placement::property_of(neighbour, "facing")
                && crate::placement::chest_connected_direction(neighbour)
                    == Some(direction.opposite())
            {
                let paired = match nb_type {
                    "left" => "right",
                    "right" => "left",
                    other => other,
                };
                return Some(crate::placement::with_property(state, "type", paired));
            }
        } else if crate::placement::chest_connected_direction(state) == Some(direction) {
            return Some(crate::placement::with_property(state, "type", "single"));
        }
        None
    }

    /// `BedBlock.updateShape`, and it is **not** the door rule with a
    /// different property.
    ///
    /// ```text
    ///   if (dir == getNeighbourDirection(state.PART, state.FACING)):
    ///       return nb.is(this) && nb.getValue(PART) != state.getValue(PART)
    ///            ? state.setValue(OCCUPIED, nb.getValue(OCCUPIED))
    ///            : AIR
    ///   getNeighbourDirection(part, facing) =
    ///       part == FOOT ? facing : facing.getOpposite()
    /// ```
    ///
    /// Three differences from the door, each of which a rule "generalised"
    /// across the two would get wrong:
    ///
    ///   * the direction is **horizontal and per state**, from `FACING` and
    ///     `PART`, not the Y axis;
    ///   * the answer is the **subject's** state with `OCCUPIED` copied, not
    ///     the neighbour's state with a property put back;
    ///   * it is `nb.is(this)` — the **same block** — not `instanceof`. A red
    ///     bed's foot beside a blue bed's head becomes air rather than
    ///     inheriting anything, and reproducing that is the whole reason the
    ///     two rules are written out separately.
    fn bed_update_shape(&self, state: &str, direction: Face, neighbour: &str) -> Option<String> {
        let part = crate::placement::property_of(state, "part")?;
        let facing = crate::placement::property_of(state, "facing")?;
        let facing = Face::from_name(facing)?;
        let toward = if part == "foot" {
            facing
        } else {
            facing.opposite()
        };
        if direction != toward {
            return None;
        }
        let same_block = block_of(neighbour) == block_of(state);
        let other = crate::placement::property_of(neighbour, "part");
        if same_block && other.is_some_and(|o| o != part) {
            let occupied = crate::placement::property_of(neighbour, "occupied")?;
            return Some(crate::placement::with_property(state, "occupied", occupied));
        }
        Some("minecraft:air".to_string())
    }

    /// Whether the block a state names is a `DoorBlock`, subclasses included.
    fn is_door(&self, state: &str) -> bool {
        self.doors.contains(block_of(state))
    }

    /// Whether the block a state names is a `BedBlock`, subclasses included.
    ///
    /// Used by [`player_will_destroy`], which is why it is `pub(crate)` where
    /// [`Self::is_door`] is not.
    #[must_use]
    pub fn is_bed(&self, state: &str) -> bool {
        self.beds.contains(block_of(state))
    }

    /// `state.updateShape(..., direction, ...)` where the block at
    /// `pos.relative(direction)` has just become the state with id `changed`.
    ///
    /// `None` is "nothing happens", which covers three cases the caller
    /// treats alike: the pair is not in the table, this direction cannot
    /// affect it, or this particular neighbour is one it tolerates.
    #[must_use]
    pub fn update_shape(&self, state: &str, direction: Face, changed: i32) -> Option<&str> {
        let index = usize::try_from(direction.to_3d()).ok()?;
        let row = self.rows.get(state)?[index].as_ref()?;
        let id = usize::try_from(changed).ok()?;
        let byte = *self.classes[row.class].get(id >> 3)?;
        ((byte >> (id & 7)) & 1 == 1).then_some(row.to.as_str())
    }

    /// How many `(state, direction)` pairs are carried.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.values().flatten().flatten().count()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The pairs the dump found and did not carry: connection properties and
    /// rules that read more of the world than one neighbour. Reported at
    /// startup, because "no cascade here" and "no table" look the same from a
    /// client.
    #[must_use]
    pub const fn gaps(&self) -> (usize, usize, usize) {
        (self.states, self.connective, self.refused)
    }
}

/// `Level.setBlock(pos, state, flags, recursionLeft)`.
///
/// Every change made — the write itself and everything that cascaded from it
/// — is appended to `updates` as `(position, state id)`, in the order vanilla
/// would have sent them: `sendBlockUpdated` fires before the shape updates,
/// so a caller that replays the list in order sends the cause before the
/// consequence.
///
/// Returns whether anything was written, which is `setBlock`'s own return.
pub fn set_block(
    blocks: &dyn Blocks,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    state: &str,
    flags: i32,
    recursion: i32,
    updates: &mut Effects,
) -> bool {
    // `LevelChunk.setBlockState` returns null when nothing changed, and
    // `setBlock` returns false without touching a neighbour — which is also
    // what stops a cascade that would otherwise rewrite the same block for
    // ever.
    let old = blocks.state_at(pos);
    let Some(id) = blocks.set_block(pos, state) else {
        return false;
    };
    // `LevelChunk.setBlockState` offsets 440-490: the removed state's
    // `affectNeighborsAfterRemoval`, when the block changed and either
    // `UPDATE_NEIGHBORS` or `UPDATE_MOVE_BY_PISTON` is set — before
    // `onPlace` (514-542) and before `Level.setBlock`'s own
    // `updateNeighborsAt`. See `crate::removal` for the bodies.
    // `LevelChunk.setBlockState` offsets 375-435: a changed block takes its
    // block entity with it. The comparator's is `crate::comparator::Outputs`.
    if block_of(&old) != block_of(state) && crate::comparator::is_comparator(&old) {
        crate::comparator::Outputs::shared().remove(pos);
    }
    if block_of(&old) != block_of(state) && flags & (UPDATE_NEIGHBORS | UPDATE_MOVE_BY_PISTON) != 0
    {
        crate::removal::affect_neighbors_after_removal(
            blocks,
            crate::power::shared(),
            shapes,
            crate::placedby::PlacedByTable::shared(std::path::Path::new(
                "data/vanilla/set_placed_by.json",
            ))
            .ok(),
            pos,
            &old,
            flags & UPDATE_MOVE_BY_PISTON != 0,
            updates,
        );
    }
    // **The block-entity ticker list is maintained here and nowhere else.**
    // `DaylightDetectorBlock.getTicker` gives the level a ticker for every
    // detector in a sky-lit dimension, and vanilla's `blockEntityTickers` is
    // kept in step by `LevelChunk.setBlockState` — this is the same funnel
    // and the same moment. Registering it at the placement call site instead
    // would miss every detector a cascade, a piston or a plugin wrote.
    crate::sky::note_block(pos, state);
    // **`onPlace` runs HERE**, not after the broadcast and not after the
    // cascade. It is called from `LevelChunk.setBlockState` at offsets
    // 514-542 — inside the write — so a rail has already re-shaped itself and
    // bent its neighbours by the time `Level.setBlock` gets to
    // `sendBlockUpdated`. See `crate::rails`.
    //
    // That the rail rewrites *this* position after `id` was taken does not
    // make `id` stale in any way that reaches a client: `record_updates`
    // keeps only the position and reads the state back at the tick boundary,
    // which is what `ChunkHolder.blockChanged` does with its
    // `ShortOpenHashSet`. Two writes to one position collapse to one entry
    // there exactly as they do in vanilla.
    //
    // **Both `onPlace` arms are one dispatch in vanilla and two calls here**,
    // and they are mutually exclusive by block: `BaseRailBlock.onPlace` and
    // `RedStoneWireBlock.onPlace` are overrides on different classes and no
    // state is both. The 512 guard is `LevelChunk.setBlockState`'s own and
    // covers the pair, which is why it is written here once rather than inside
    // either of them.
    if flags & UPDATE_SKIP_ON_PLACE == 0 {
        if let Some(shapes) = shapes {
            let rails = crate::rails::Rails {
                is_rail: &|s| shapes.is_rail(s),
                is_straight: &|s| shapes.is_straight_rail(s),
                shapes: Some(shapes),
            };
            crate::rails::on_place(&rails, blocks, pos, state, &old, updates);
        }
        crate::power::on_place(
            blocks,
            crate::power::shared(),
            shapes,
            pos,
            state,
            &old,
            updates,
        );
        // `FallingBlock.onPlace` — the third `onPlace` override that reaches
        // here, and one line: `scheduleTick(pos, this, getDelayAfterPlace())`.
        // Mutually exclusive with the two above by block, as they are with each
        // other. A gravity block placed with nothing under it therefore falls
        // two ticks later without anything having to notice at place time.
        if let Some(shapes) = shapes {
            crate::fallable::on_place(shapes.is_gravity_block(state), pos, state, updates);
        }
        // The fourth arm, and it is exclusive with the other three by block:
        // `FireBlock.onPlace` is an override on a class no rail, no wire and
        // no falling block shares. It is what makes a fire tick at all —
        // nothing else in this server ever schedules one — and it is also the
        // arm that removes a fire placed where it cannot survive, which is
        // `BaseFireBlock.onPlace`'s own last three instructions.
        crate::fire::on_place(blocks, shapes, pos, state, &old, updates);
        // The fifth: `CarvedPumpkinBlock.onPlace` -> `trySpawnGolem`. It
        // rewrites this very position to air, so the `id` taken above is
        // stale for it, which `record_updates` absorbs exactly as it does a
        // rail's rewrite (it keeps the position and reads the state back).
        crate::golem::on_place(blocks, shapes, pos, state, &old, updates);
        // `LiquidBlock.onPlace` — the fourth arm of the same dispatch and the
        // one that starts every flow. An override on a class no other state
        // shares, so it is exclusive with the three above. For lava it is
        // also where `shouldSpreadLiquid` turns a pour beside water into
        // obsidian or cobblestone, inside the write that placed it.
        crate::fluid::on_place_or_neighbor_changed(blocks, shapes, pos, state, updates);
    }
    // **`Level.setBlock` offsets 51-64 and 241: the whole rest of this
    // function is guarded on the write having survived `onPlace`.**
    //
    // ```java
    //   BlockState old = chunk.setBlockState(pos, state, flags);   // 44, onPlace inside
    //   if (old == null) return false;                              // 51
    //   BlockState now = getBlockState(pos);                        // 56
    //   if (now == state) {                                         // 64: if_acmpne 241
    //       ... sendBlockUpdated, updateNeighborsAt, updateNeighbourShapes ...
    //   }
    //   return true;                                                // 241
    // ```
    //
    // When `onPlace` rewrote its own position — lava poured beside water
    // turning to obsidian inside `LiquidBlock.onPlace`, a rail bending in
    // `BaseRailBlock.onPlace` — the inner write did its own broadcast and its
    // own neighbour and shape updates, and the outer write does **none**: no
    // client update for the stale state, no `neighborChanged` naming it, no
    // shape update carrying it. Until 2026-10-01 this function ran all three
    // with the stale state.
    //
    // Equal strings are the same state; unequal strings are still the same
    // state when their ids agree, because a caller's property order is not
    // the table's. Only a position that differs by both is rewritten.
    let now = blocks.state_at(pos);
    if now != state && blocks.state_id(&now) != Some(id) {
        return true;
    }
    if flags & UPDATE_CLIENTS != 0 {
        updates.blocks.push((pos, id));
    }
    // `updateNeighborsAt`, offsets 146-153, and **the block it names is the
    // OLD state's**: `aload 7` is `LevelChunk.setBlockState`'s return value,
    // not `aload_2` (the state just written) and not `aload 6` (the new block,
    // computed four instructions earlier and used on the very next line for
    // `updateNeighbourForOutputSignal`). Both are in scope and vanilla picks
    // the old one.
    if flags & UPDATE_NEIGHBORS != 0 {
        crate::power::update_neighbors_at(
            blocks,
            crate::power::shared(),
            shapes,
            pos,
            block_of(&old),
            updates,
        );
        // `Level.setBlock` offsets 157-174: `if (!isClientSide &&
        // state.hasAnalogOutputSignal()) updateNeighbourForOutputSignal(pos,
        // state.getBlock())` — the NEW state, right behind the flag-1 sweep.
        // A cake losing a bite tells the comparator two blocks away.
        if let Some(signals) = crate::power::shared() {
            let classes = crate::placedby::PlacedByTable::shared(std::path::Path::new(
                "data/vanilla/set_placed_by.json",
            ))
            .ok();
            if crate::comparator::has_analog_output_signal(classes, state) {
                crate::comparator::update_neighbour_for_output_signal(
                    blocks, signals, shapes, pos, updates,
                );
            }
        }
    }
    if flags & UPDATE_KNOWN_SHAPE == 0 && recursion > 0 {
        // `flags & -34`, which clears UPDATE_SUPPRESS_DROPS and
        // UPDATE_NEIGHBORS: a shape update is not itself a neighbour update,
        // and vanilla stops it from becoming one at every level.
        let inner = flags & !(UPDATE_SUPPRESS_DROPS | UPDATE_NEIGHBORS);
        if let Some(shapes) = shapes {
            // `Level.setBlock` offsets 177-229 — **three** calls, not one, and
            // the loop we already had is the middle one:
            //
            // ```java
            //   oldState.updateIndirectNeighbourShapes(this, pos, inner, r-1);
            //   newState.updateNeighbourShapes        (this, pos, inner, r-1);
            //   newState.updateIndirectNeighbourShapes(this, pos, inner, r-1);
            // ```
            //
            // `aload 7` is the **old** state and `aload_2` the new one, so the
            // order is old-indirect, new-direct, new-indirect. The old state's
            // call is not symmetry for its own sake: it is how a wire that has
            // just been removed tells the diagonals that were connected to it,
            // and a port with only the new state's call fixes placing and not
            // breaking. `removing_a_wire_notifies_the_diagonal_it_left` is the
            // test that separates them.
            update_indirect_neighbour_shapes(
                blocks,
                shapes,
                &old,
                pos,
                inner,
                recursion - 1,
                updates,
            );
            for d in UPDATE_SHAPE_ORDER {
                // The direction handed to `updateShape` points *back* at the
                // block that changed — see the module docs.
                execute_shape_update(
                    blocks,
                    shapes,
                    d.opposite(),
                    d.relative(pos),
                    pos,
                    state,
                    Some(id),
                    inner,
                    recursion - 1,
                    updates,
                );
            }
            // `state`, the argument — `aload_2`. It cannot be stale here: the
            // `now == state` guard above returned before this point if
            // `onPlace` had rewritten the position. (This said vanilla walks a
            // stale state when a rail bends itself; offset 64's `if_acmpne`
            // says it never gets here in that case.)
            update_indirect_neighbour_shapes(
                blocks,
                shapes,
                state,
                pos,
                inner,
                recursion - 1,
                updates,
            );
        }
    }
    true
}

/// `NeighborUpdater.executeShapeUpdate(level, direction, pos, neighbourPos,
/// neighbourState, flags, recursionLevel)`.
///
/// Hoisted out of [`set_block`]'s six-face loop when
/// [`update_indirect_neighbour_shapes`] became a second caller. Both paths now
/// go through one body, which is the point: the flag-128 guard below was
/// missing from the inlined copy for the whole life of the loop, and a second
/// inlined copy would have been a second place to miss it.
///
/// `direction` points *back* at the block that changed, so the block being
/// asked is the one at `pos` and `neighbour_pos` is `pos.relative(direction)`.
///
/// `neighbour_id` is `None` when the changed state has no network id. The table
/// is keyed on the neighbour's id through its class bitmaps, so there is
/// nothing to look up; the two hand ports below take the state string and are
/// unaffected. Passing a sentinel id instead would silently probe class 0.
#[expect(
    clippy::too_many_arguments,
    reason = "vanilla's own signature; splitting it would hide which flag went where"
)]
fn execute_shape_update(
    blocks: &dyn Blocks,
    shapes: &ShapeTable,
    direction: Face,
    pos: BlockPos,
    neighbour_pos: BlockPos,
    neighbour_state: &str,
    neighbour_id: Option<i32>,
    flags: i32,
    recursion: i32,
    updates: &mut Effects,
) {
    let current = blocks.state_at(pos);
    // Offsets 9-29: `(flags & 128) != 0 && state.is(REDSTONE_WIRE)` returns
    // before the block is asked anything at all.
    if flags & UPDATE_SKIP_SHAPE_UPDATE_ON_WIRE != 0 && crate::wire::is_wire(&current) {
        return;
    }
    // **A shape answer that is a side effect**, which the extracted table is
    // structurally blind to — this module's own header has said so since
    // 2026-08-04 and named this block. `ObserverBlock.updateShape` offsets
    // 0-33:
    //
    // ```java
    // if (state.getValue(FACING) == direction && !state.getValue(POWERED))
    //     startSignal(level, tickAccess, pos);
    // return super.updateShape(...);
    // ```
    //
    // It runs **before** the super call, so it is here rather than after the
    // lookup below, and it returns the observer's own state — which is why the
    // table holds zero `minecraft:observer` rows and why nothing below will
    // fire for one.
    let to = shape_after(
        blocks,
        shapes,
        pos,
        &current,
        direction,
        neighbour_pos,
        neighbour_state,
        neighbour_id,
        updates,
    );
    let Some(to) = to else { return };
    update_or_destroy(
        blocks,
        Some(shapes),
        &current,
        &to,
        pos,
        flags,
        recursion,
        updates,
    );
}

/// `BlockStateBase.updateIndirectNeighbourShapes(level, pos, flags,
/// recursionLeft)` — which dispatches to `Block.updateIndirectNeighbourShapes`,
/// whose body is `return`.
///
/// **`RedStoneWireBlock` is the only block in the pinned jar that overrides
/// it.** Established by grepping the constant pool of every class under
/// `net/minecraft/world/level/block/` for the method name and reading the four
/// hits: `BlockBehaviour` declares the empty default, `BlockStateBase` is the
/// dispatch, `PistonBaseBlock` only *calls* it, and `RedStoneWireBlock`
/// overrides. So this is a wire rule wearing a general name, and it is written
/// as one branch rather than as a hook table nothing else would ever join.
///
/// # The walk, offsets 0-233
///
/// ```java
/// MutableBlockPos m = new MutableBlockPos();                       //   0-7
/// for (Direction d : Plane.HORIZONTAL) {                           //   9-37
///     if (state.getValue(PROPERTY_BY_DIRECTION.get(d)) == NONE)    //  39-66
///         continue;
///     m.setWithOffset(pos, d);
///     if (level.getBlockState(m).is(this)) continue;               //  78-87
///     m.move(Direction.DOWN);                                      //  90-98
///     if (level.getBlockState(m).is(this)) {                       //  99-115
///         BlockPos from = m.relative(d.getOpposite());             // 118-128
///         level.neighborShapeChanged(d.getOpposite(), m, from,     // 130-152
///             level.getBlockState(from), flags, recursion);
///     }
///     m.setWithOffset(pos, d).move(Direction.UP);                  // 157-171
///     if (level.getBlockState(m).is(this)) {                       // 172-188
///         BlockPos from = m.relative(d.getOpposite());             // 191-201
///         level.neighborShapeChanged(d.getOpposite(), m, from,     // 203-225
///             level.getBlockState(from), flags, recursion);
///     }
/// }
/// ```
///
/// Three things in it a summary would lose:
///
///   * **the diagonals are only probed when the orthogonal neighbour is NOT a
///     wire** (offset 87 is `ifne`, straight to the next direction). A wire
///     beside a wire is already served by the six-face cascade, so this
///     override adds exactly the cases that cascade cannot reach and no others.
///     Reading it as `ifeq` inverts the whole method into a duplicate of the
///     cascade it exists to complement;
///   * **it is gated on this wire's own connection being non-`NONE`.** A dust
///     reaches out along the sides it claims to use, not along all four;
///   * it notifies **the diagonal**, from `d.getOpposite()`, with the state at
///     the position one step back from the diagonal — which works out to
///     `pos.below()` for the down probe and `pos.above()` for the up probe.
///     That is the block that changed, and it is not this one.
///
/// # Why the reads are interleaved with the writes
///
/// Each `getBlockState` above happens *after* whatever the previous
/// notification did to the world. Collecting the four directions' worth of
/// notifications first and executing them afterwards is a different function on
/// any arrangement where one shape update changes what the next probe sees, so
/// the walk runs them inline.
/// `BlockState.updateShape(level, ticks, pos, direction, neighbourPos,
/// neighbourState, random)` as a value: what `state` at `pos` becomes when
/// its neighbour in `direction` is `neighbour_state`, or `None` when nothing
/// happens. The three answerers are tried in the order
/// [`execute_shape_update`] always tried them — the id table, the typed
/// hook, the level-reading hook — and the observer's `updateShape` side
/// effect (a scheduled tick) rides along, because vanilla's `updateShape`
/// carries it whichever caller asked. `Block.updateFromNeighbourShapes`
/// walks `UPDATE_SHAPE_ORDER` through this without writing anything;
/// `NeighborUpdater.executeShapeUpdate` writes the answer.
#[expect(
    clippy::too_many_arguments,
    reason = "vanilla's own eight-argument updateShape, minus the random"
)]
#[must_use]
pub(crate) fn shape_after(
    blocks: &dyn Blocks,
    shapes: &ShapeTable,
    pos: BlockPos,
    state: &str,
    direction: Face,
    neighbour_pos: BlockPos,
    neighbour_state: &str,
    neighbour_id: Option<i32>,
    updates: &mut Effects,
) -> Option<String> {
    crate::power::observer_update_shape(state, direction, pos, updates);
    // `LiquidBlock.updateShape` offsets 0-39 — a source on either side books
    // a fluid tick here, and the block answers itself. See
    // [`crate::fluid::liquid_update_shape`].
    crate::fluid::liquid_update_shape(blocks, pos, state, neighbour_state, updates);
    // **The `if (WATERLOGGED) scheduleTick(pos, Fluids.WATER, 5)` head that
    // every waterloggable block's `updateShape` opens with**, run whether or
    // not the state then changes. Read off the property rather than the
    // extracted table, because the extractor drops a `(state, direction)`
    // whose answer is only itself — which is most of a waterlogged block's
    // updates — and so cannot say it scheduled. Checked against the jar
    // instead: all 49 classes in `net/minecraft/world/level/block/` that
    // implement `SimpleWaterloggedBlock` reach an `updateShape` that does
    // `getstatic Fluids.WATER ... scheduleTick` — 44 in their own override,
    // the four signs through `SignBlock.updateShape` after their survival
    // test, and `CrossCollisionBlock` through `FenceBlock`/`IronBarsBlock`.
    // Kelp and seagrass schedule the same tick by a different route
    // (`getFluidState`, no property) and are not ported here.
    if crate::placement::property_of(state, "waterlogged") == Some("true") {
        updates.fluid_ticks.push(TickRequest {
            pos,
            block: crate::fluid::WATER.to_string(),
            delay: crate::fluid::WATER_TICK_DELAY,
            priority: crate::ticks::PRIORITY_NORMAL,
        });
    }
    // **The second shape answer that is a side effect**, and the only other
    // one in the jar. `FallingBlock.updateShape` offsets 0-13:
    //
    // ```java
    // tickAccess.scheduleTick(pos, this, this.getDelayAfterPlace());
    // return super.updateShape(...);
    // ```
    //
    // Like the observer's it runs **before** the super call and returns
    // super's answer, so it is here rather than after the lookup below — and
    // for the same reason it cannot live in the extracted table, which records
    // the state a block answers with and has no column for a booked tick.
    //
    // Unlike the observer's it consults neither `direction` nor the block's own
    // state: there is no test of any kind, and the tick is where `isFree` gets
    // asked. See [`crate::fallable`].
    crate::fallable::update_shape(shapes.is_gravity_block(state), pos, state, updates);
    if let Some(locked) = crate::power::repeater_update_shape(
        blocks,
        crate::power::shared(),
        Some(shapes),
        pos,
        state,
        direction,
    ) {
        return Some(locked);
    }
    // After the repeater's LOCKED arm above: the table first, then the rules it structurally cannot hold, then the
    // rules that need the level. In that order because the table is the
    // measured answer for 12,748 pairs and the hand ports are a handful; a
    // typed rule that shadowed a table row would be a hand port quietly
    // overriding an extraction, which is the wrong way round for this project.
    // Each fallback is strictly wider than the one before it and none may
    // shadow it: `level_update_shape` runs last because a block with a measured
    // row is answered by the measurement, not by a hand port.
    neighbour_id
        .and_then(|id| shapes.update_shape(state, direction, id))
        .map(ToString::to_string)
        .or_else(|| shapes.typed_update_shape(state, direction, neighbour_state))
        .or_else(|| {
            shapes.level_update_shape(
                blocks,
                pos,
                state,
                direction,
                neighbour_pos,
                neighbour_state,
            )
        })
}

/// `BlockState.updateNeighbourShapes(level, pos, flags, recursion)`: for each
/// direction of `UPDATE_SHAPE_ORDER`, `executeShapeUpdate` at the neighbour
/// with `d.getOpposite()` and this state as the changed one. The loop
/// [`set_block`] runs, exposed for the callers that run it on a state that
/// was not just written — `PistonBaseBlock.moveBlocks` runs it on `AIR` at
/// every vacated position and on the retracting `moving_piston`.
pub(crate) fn update_neighbour_shapes(
    blocks: &dyn Blocks,
    shapes: &ShapeTable,
    pos: BlockPos,
    state: &str,
    flags: i32,
    recursion: i32,
    updates: &mut Effects,
) {
    let id = blocks.state_id(state);
    for d in UPDATE_SHAPE_ORDER {
        execute_shape_update(
            blocks,
            shapes,
            d.opposite(),
            d.relative(pos),
            pos,
            state,
            id,
            flags,
            recursion - 1,
            updates,
        );
    }
}

pub(crate) fn update_indirect_neighbour_shapes(
    blocks: &dyn Blocks,
    shapes: &ShapeTable,
    state: &str,
    pos: BlockPos,
    flags: i32,
    recursion: i32,
    updates: &mut Effects,
) {
    if !crate::wire::is_wire(state) {
        return;
    }
    for d in crate::wire::HORIZONTAL {
        if !crate::wire::connects(state, d) {
            continue;
        }
        let side = d.relative(pos);
        if crate::wire::is_wire(&blocks.state_at(side)) {
            continue;
        }
        let back = d.opposite();
        for probe in [Face::Down, Face::Up] {
            let diagonal = probe.relative(side);
            if !crate::wire::is_wire(&blocks.state_at(diagonal)) {
                continue;
            }
            let from = back.relative(diagonal);
            let changed = blocks.state_at(from);
            let id = blocks.state_id(&changed);
            execute_shape_update(
                blocks, shapes, back, diagonal, from, &changed, id, flags, recursion, updates,
            );
        }
    }
}

/// `Block.playerWillDestroy(level, pos, state, player)` — the half of a dig
/// that runs **before** the block is removed.
///
/// # Why a dig is two steps and not one
///
/// `ServerPlayerGameMode.destroyBlock` calls `playerWillDestroy` and only
/// then `Level.removeBlock`. Everything this server did until 2026-08-04 was
/// the second call, so every rule vanilla puts in the first one was missing
/// — and the double plant's is the one the corpus can see.
///
/// `docs/SERVER.md` recorded the symptom on 2026-08-03 and attributed it to
/// the extracted table:
///
/// > Those rows are `dir=DOWN` only, so digging the *lower* half of a tall
/// > flower removes the upper [...] and digging the **upper** half leaves the
/// > lower standing, where vanilla removes it. [...] No check covers the
/// > upper-half dig; adding one would report `diverge` today.
///
/// The verdict it predicted is right and the mechanism it names is not, which
/// only reading `DoublePlantBlock` settles. Two *different* rules both remove
/// the other half, and which one fires depends on who broke it:
///
///   * `updateShape` is symmetric in the bytecode —
///     `dir.getAxis() == Y && (half == LOWER) == (dir == UP)` — and the
///     extract carries only the `dir=DOWN` direction of it. That asymmetry is
///     real, and a **dig cannot reach it**;
///   * `playerWillDestroy` -> `preventDropFromBottomPart` is what a dig
///     reaches, and it never consults `updateShape` at all. In creative
///     (`player.preventsBlockDrops()`) it is the whole of the behaviour.
///
/// So the table row is still missing and this function is not a substitute
/// for it; it is the rule the check actually measures. `remove_block` alone
/// left the lower half standing because nothing had ever run this.
///
/// # `preventDropFromBottomPart`, transcribed
///
/// ```text
///   half = state.getValue(HALF)
///   if half == UPPER:
///       below = pos.below()
///       nb    = level.getBlockState(below)
///       if nb.is(state.getBlock()) && nb.getValue(HALF) == LOWER:
///           to = nb.getFluidState().is(Fluids.WATER)
///                ? Blocks.WATER.defaultBlockState()
///                : Blocks.AIR.defaultBlockState()
///           level.setBlock(below, to, 35)
///           level.levelEvent(player, 2001, below, Block.getId(nb))
/// ```
///
/// Three things in that worth stating, because each is a way to get it wrong:
///
///   * it is `nb.is(state.getBlock())` — the **same block**, like the bed's
///     rule and unlike the door's `instanceof`. A sunflower's upper half over
///     a peony's lower half takes nothing with it;
///   * the flag is **35**, not `UPDATE_ALL`. 35 is
///     `UPDATE_NEIGHBORS | UPDATE_CLIENTS | UPDATE_SUPPRESS_DROPS`, and it
///     does **not** set `UPDATE_KNOWN_SHAPE`, so the write below cascades its
///     own shape updates normally;
///   * `Fluids.WATER` is the **source**. A flowing-water fluid state's type
///     is `FLOWING_WATER` and takes the air branch, which is why this
///     compares against water's default state rather than asking whether any
///     water is there.
///
/// **The `levelEvent` on the last line is still not sent from here, and that
/// is now a decision rather than a gap.** It is `2001` carrying the broken
/// state's id, and it is the **four**-argument
/// `Level.levelEvent(Entity, int, BlockPos, int)` — offsets 102-114 pass the
/// `Player` — so `PlayerList.broadcast` skips the digger. This server sends
/// the three-argument form only (see [`destroy_block`]), which is the one the
/// digger receives; emitting this one to everybody would put a packet on the
/// actor's stream that vanilla keeps off it, and `132`/`238` would go from
/// strict `match` to strict `diverge` on the spot.
///
/// What is therefore still missing is the *observer's* copy of this one:
/// vanilla tells a bystander about two or three destroyed blocks where this
/// server tells it about one or two — the dug block's own `2001` is sent to
/// everyone but the digger since 2026-10-01 (`crate::world_send`'s
/// `spawn_destroy_particles`, read by `970`, `971` and `974`), and the
/// `preventDropFromBottomPart` and bed ones are not.
/// `ci/checks/gameplay/238` deliberately does not project `level_event`, so
/// the check measures the rule and not the gap either way.
///
/// The other branch of `playerWillDestroy` — `dropResources` for a
/// non-creative player — needs item entities and is not reachable from this
/// corpus; see the survival-mining note in `ci/checks/gameplay/README.md`.
pub fn player_will_destroy(
    blocks: &dyn Blocks,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    updates: &mut Effects,
) {
    // The corpus positive control, the other half of `Kind::Structure`:
    // breaking one half of a two-block takes nothing with it. Armed by the
    // position being destroyed, because a destroy can be reached from the
    // tick loop as well as from a packet. See `crate::sabotage`.
    if crate::sabotage::fires_at(crate::sabotage::Kind::Structure, pos.0) {
        return;
    }
    let Some(shapes) = shapes else { return };
    let state = blocks.state_at(pos);
    // **Two classes reach `preventDropFromBottomPart`, not one.**
    // `DoorBlock.playerWillDestroy` calls
    // `DoublePlantBlock.preventDropFromBottomPart` — the same static method,
    // from a class that is not a double plant — and missing that would leave
    // the door half of this rule unported. The two guards differ and both are
    // true in creative, which is the only mode this corpus runs:
    //
    //   DoublePlantBlock   player.preventsBlockDrops()
    //   DoorBlock          player.preventsBlockDrops()
    //                      || !player.hasCorrectToolForDrops(state)
    //
    // `hasCorrectToolForDrops` needs a survival inventory to be anything but
    // constant here, so the union is exact for every state this server can be
    // in and the difference is named rather than silently collapsed.
    //
    // **The door half changes no packet today and is still not optional.**
    // Measured 2026-08-04: `132-digging-the-top-of-a-door-takes-the-bottom`
    // was `match` *before* this branch covered doors, because the typed
    // `updateShape` rule removed the lower half anyway and the resulting
    // `section_blocks_update` was byte-identical. What differs is which rule
    // did it and in what order, and that stops being invisible as soon as
    // drops exist: vanilla's write here carries `UPDATE_SUPPRESS_DROPS`,
    // where the shape cascade that stood in for it does not. Reproducing the
    // observable and not the mechanism is the one-way door this project
    // refuses to walk through.
    // **`BedBlock.playerWillDestroy` is a third rule, not a fourth caller of
    // the second.** It clears the *head* when the *foot* is broken, along an
    // axis the state names, and it writes plain air rather than the fluid.
    //
    // It was ported on 2026-08-07 because a measurement asked for it, and the
    // measurement is worth writing down: without it a bed still lost both
    // halves — `236-digging-a-bed-takes-both-halves` was `match` — because
    // `remove_block` at the foot cascaded into `BedBlock.updateShape` at the
    // head. Same blocks, same `section_blocks_update`, **different block
    // destroyed by the cascade**. Vanilla clears the head by hand and the
    // cascade then takes the *foot*, so its `level_event` names the foot at
    // the foot's position; ours took the head, and named that. One packet,
    // one position, one state id apart, and invisible until a `level_event`
    // existed to carry it.
    if shapes.is_bed(&state) {
        bed_will_destroy(blocks, shapes, pos, &state, updates);
        return;
    }
    if !shapes.is_double_plant(&state) && !shapes.is_door(&state) {
        return;
    }
    if crate::placement::property_of(&state, "half") != Some("upper") {
        return;
    }
    let below = (pos.0, pos.1 - 1, pos.2);
    let nb = blocks.state_at(below);
    if block_of(&nb) != block_of(&state) {
        return;
    }
    if crate::placement::property_of(&nb, "half") != Some("lower") {
        return;
    }
    // `nb.getFluidState().is(Fluids.WATER)` — the **source**. `legacy_fluid_at`
    // answers a water block with its own state, so flowing water comes back as
    // `water[level=3]` and correctly takes the air branch, exactly as
    // `FluidState.is(Fluids.WATER)` does against `FLOWING_WATER`. Comparing
    // against the source state is therefore the rule and not an approximation
    // of it; asking `water_at` instead would be the approximation, because
    // that is `is(FluidTags.WATER)` and answers true for flowing as well.
    let legacy = blocks.legacy_fluid_at(below);
    let to = if legacy == crate::placement::WATER_SOURCE {
        crate::placement::WATER_SOURCE.to_string()
    } else {
        "minecraft:air".to_string()
    };
    set_block(
        blocks,
        Some(shapes),
        below,
        &to,
        UPDATE_NEIGHBORS | UPDATE_CLIENTS | UPDATE_SUPPRESS_DROPS,
        RECURSION_LIMIT,
        updates,
    );
}

/// `BedBlock.playerWillDestroy`, offsets 0-118.
///
/// ```text
///   if (!level.isClientSide && player.preventsBlockDrops()) {
///       part = state.getValue(PART);
///       if (part == FOOT) {
///           p2 = pos.relative(getNeighbourDirection(part, state.getValue(FACING)));
///           s2 = level.getBlockState(p2);
///           if (s2.is(this) && s2.getValue(PART) == HEAD) {
///               level.setBlock(p2, Blocks.AIR.defaultBlockState(), 35);
///               level.levelEvent(player, 2001, p2, Block.getId(s2));
///           }
///       }
///   }
/// ```
///
/// Four things it does **not** share with `preventDropFromBottomPart`, each
/// of which a generalisation of that rule would get wrong:
///
///   * the neighbour is horizontal and the direction comes from the state.
///     `getNeighbourDirection(part, facing)` is `facing` for the foot and
///     `facing.getOpposite()` for the head (offsets 0-15), and only the
///     **foot** branch exists here — breaking the head runs nothing;
///   * it writes **`Blocks.AIR`**, flat, where the double plant checks for a
///     water source first. A waterlogged bed is not a state that exists, and
///     writing the fluid here would be a rule vanilla does not have;
///   * `s2.is(this)` is the same *block*, so a red bed's foot beside a white
///     bed's head takes nothing with it;
///   * the guard is `preventsBlockDrops()` alone, with no
///     `hasCorrectToolForDrops` clause — always true in creative, which is
///     the only mode this corpus runs, and the difference is named rather
///     than silently collapsed.
///
/// The `levelEvent` on the last line is the four-argument overload and
/// excludes the digger, so it is not emitted; the one the digger actually
/// receives comes from the cascade this write sets off. See [`destroy_block`].
fn bed_will_destroy(
    blocks: &dyn Blocks,
    shapes: &ShapeTable,
    pos: BlockPos,
    state: &str,
    updates: &mut Effects,
) {
    if crate::placement::property_of(state, "part") != Some("foot") {
        return;
    }
    let Some(facing) = crate::placement::property_of(state, "facing").and_then(Face::from_name)
    else {
        return;
    };
    // `part == FOOT` here by the guard above, so `getNeighbourDirection` is
    // `facing` itself and not its opposite.
    let head = facing.relative(pos);
    let nb = blocks.state_at(head);
    if block_of(&nb) != block_of(state) {
        return;
    }
    if crate::placement::property_of(&nb, "part") != Some("head") {
        return;
    }
    set_block(
        blocks,
        Some(shapes),
        head,
        "minecraft:air",
        UPDATE_NEIGHBORS | UPDATE_CLIENTS | UPDATE_SUPPRESS_DROPS,
        RECURSION_LIMIT,
        updates,
    );
}

/// `Level.removeBlock(pos, false)` — what a dig does.
///
/// Not "write air": vanilla writes
/// `getFluidState(pos).createLegacyBlock()`, so breaking a waterlogged
/// stair leaves water and breaking water leaves water, which is the same
/// state and therefore no change at all.
pub fn remove_block(
    blocks: &dyn Blocks,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    updates: &mut Effects,
) -> bool {
    let legacy = blocks.legacy_fluid_at(pos);
    set_block(
        blocks,
        shapes,
        pos,
        &legacy,
        UPDATE_ALL,
        RECURSION_LIMIT,
        updates,
    )
}

/// `Block.updateOrDestroy(state, newState, level, pos, flags, recursionLeft)`.
#[expect(
    clippy::too_many_arguments,
    reason = "vanilla's own signature; splitting it would hide which flag went where"
)]
pub(crate) fn update_or_destroy(
    blocks: &dyn Blocks,
    shapes: Option<&ShapeTable>,
    state: &str,
    new_state: &str,
    pos: BlockPos,
    flags: i32,
    recursion: i32,
    updates: &mut Effects,
) {
    if new_state == state {
        return;
    }
    if is_air(new_state) {
        destroy_block(
            blocks,
            shapes,
            pos,
            flags & UPDATE_SUPPRESS_DROPS == 0,
            recursion,
            updates,
        );
    } else {
        set_block(
            blocks,
            shapes,
            pos,
            new_state,
            flags & !UPDATE_SUPPRESS_DROPS,
            recursion,
            updates,
        );
    }
}

/// `Level.destroyBlock(pos, dropBlock, null, recursionLeft)`.
///
/// The drops are the part that is missing and it is the whole of what is
/// missing: no item entities exist, so `dropResources` has nowhere to put
/// them. Everything else is here — the air check that makes destroying air a
/// no-op, the `2001` level event, and the write of the fluid rather than of
/// air.
///
/// # The level event, and the exclusion rule that decides who receives it
///
/// This is **the one site in this server that may emit a `2001` to the player
/// who caused it**, and getting that wrong in either direction costs a
/// scenario, so the derivation is written out rather than summarised.
///
/// There are two `levelEvent` overloads on `Level` and the corpus can tell
/// them apart:
///
/// ```text
///   Level.levelEvent(Entity, int, BlockPos, int)
///     ServerLevel's implementation, offsets 0-63:
///       playerList.broadcast(entity instanceof Player p ? p : null, x, y, z,
///                            64.0, dimension(), new ClientboundLevelEventPacket(...))
///     PlayerList.broadcast(Player except, ...), offsets 33-39:
///       if (players.get(i) == except) continue;      <- THE EXCLUSION
///
///   Level.levelEvent(int, BlockPos, int)
///     the same thing with no entity, so nobody is excluded
/// ```
///
/// `Level.destroyBlock`, offsets 24-45, calls the **three-argument** one:
///
/// ```text
///   if (!(state.getBlock() instanceof BaseFireBlock))
///       levelEvent(2001, pos, Block.getId(state));
/// ```
///
/// and `Block.updateOrDestroy`'s air branch, offsets 21-39, reaches it with
/// `aconst_null` in the entity slot: `level.destroyBlock(pos, (flags & 32) == 0,
/// null, recursionLeft)`. So a block turned to air by a **shape cascade** is
/// announced to everybody, the acting player included.
///
/// The three sites that emit the *four*-argument overload all pass a real
/// player and are therefore **invisible to the actor**:
/// `Block.spawnDestroyParticles` (the block the player dug, reached from
/// `Block.playerWillDestroy`), `DoublePlantBlock.preventDropFromBottomPart`
/// offsets 102-114, and `BedBlock.playerWillDestroy` offsets 102-115. None of
/// them is emitted here, which is why `010-break-block` — whose whole content
/// is the actor's stream for a plain dig — sees no `level_event` on either
/// server and stays a strict `Match`. The first is emitted by the dig handler
/// itself, to everyone but the digger (`crate::world_send`'s
/// `spawn_destroy_particles`, since 2026-10-01); the other two are still not.
///
/// That reading is not only the bytecode's; it reproduces the recorded traces
/// of 2026-08-07 02:19 packet for packet. `010`'s actor gets none and its
/// observer gets one at the dug position. `260-digging-a-door-takes-both-halves`
/// digs the lower half: its actor gets exactly one, at the **upper** half's
/// position and carrying the upper half's state id, because that is the block
/// the cascade destroyed; its observer gets that one plus the dug block's own.
/// `132` and `238` dig the upper half, where `preventDropFromBottomPart` writes
/// air below and the cascade comes back up to destroy the upper half — one on
/// the actor's stream again, at the upper position. `236` digs a bed's foot,
/// `BedBlock` clears the head, and the cascade destroys the foot: one on the
/// actor's stream, at the foot.
///
/// # What is missing, named rather than left to be found
///
/// * **Two of the three four-argument sites are not emitted**, so an observer
///   is told about fewer destroyed blocks than vanilla tells it about. The
///   third, `Block.spawnDestroyParticles` for the dug block itself, is sent by
///   `crate::dig::destroy_block` since 2026-09-30, with the digger excluded;
///   `DoublePlantBlock.preventDropFromBottomPart`'s and `BedBlock`'s are not.
/// * **The 64-block radius is not applied.** `Players::broadcast` sends to
///   every connected player. It is no longer "for the same reason as the
///   block-update path": that path went through
///   `Players::players_for_chunk` on 2026-09-05 and is now
///   `ChunkMap.getPlayers(pos, false)`. This one is a *different* filter —
///   `PlayerList.broadcast`'s explicit distance test, not
///   `isChunkTracked` — and it is still unmodelled.
/// * **The drop is a request.** `dropBlock` puts the destroyed state on
///   [`Effects::drops`], after this block's `2001` and before the write —
///   the order of offsets 36-94 — and the drain site rolls the loot and
///   spawns the items. `spawnAfterBreak` (an ore's experience) is not
///   ported; nor is the block entity the loot context would carry.
pub(crate) fn destroy_block(
    blocks: &dyn Blocks,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    drop: bool,
    recursion: i32,
    updates: &mut Effects,
) -> bool {
    let state = blocks.state_at(pos);
    if is_air(&state) {
        return false;
    }
    // `!(state.getBlock() instanceof BaseFireBlock)`. `BaseFireBlock`'s
    // subclasses in 26.2 are `FireBlock` and `SoulFireBlock`, whose registered
    // blocks are these two; nothing else extends it. Written as the block test
    // vanilla performs rather than skipped, because a fire that announced its
    // own destruction is a difference no check would ever look for.
    if !matches!(block_of(&state), "minecraft:fire" | "minecraft:soul_fire") {
        // `Block.getId(state)` is the **global block-state** id — the same
        // number `block_update` carries, not the block's registry id. Read
        // before the write, because the write is what makes it stale.
        if let Some(id) = blocks.state_id(&state) {
            updates.level_events.push((LEVEL_EVENT_DESTROY, pos, id));
        }
    }
    if drop {
        updates.drops.push(DropRequest {
            pos,
            after_events: updates.level_events.len(),
            state: state.clone(),
        });
    }
    let legacy = blocks.legacy_fluid_at(pos);
    set_block(blocks, shapes, pos, &legacy, UPDATE_ALL, recursion, updates)
}

/// Base64, because the neighbour sets are 4,046-byte bitmaps and a JSON array
/// of 32,366 booleans is 130 KB each.
///
/// Written here rather than pulled in: it is twenty lines, the workspace has
/// no base64 dependency, and this is the only thing that needs one.
fn decode_base64(text: &str) -> Option<Vec<u8>> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for byte in text.bytes() {
        if byte == b'=' {
            break;
        }
        if byte.is_ascii_whitespace() {
            continue;
        }
        let value = ALPHABET.iter().position(|&c| c == byte)?;
        acc = (acc << 6) | u32::try_from(value).ok()?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((acc >> bits) & 0xff).ok()?);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap as Map;
    use std::sync::Mutex;

    /// A world of state strings with ids assigned in the order they are first
    /// seen, which is all the cascade reads.
    #[derive(Default)]
    struct Fake {
        blocks: Mutex<Map<BlockPos, String>>,
        ids: Mutex<Map<String, i32>>,
    }

    impl Fake {
        fn with(states: &[(BlockPos, &str)]) -> Self {
            let fake = Self::default();
            for (pos, state) in states {
                fake.blocks
                    .lock()
                    .expect("fake")
                    .insert(*pos, (*state).to_string());
                fake.id(state);
            }
            fake.id("minecraft:air");
            fake
        }
        fn id(&self, state: &str) -> i32 {
            let mut ids = self.ids.lock().expect("ids");
            let next = i32::try_from(ids.len()).expect("small");
            *ids.entry(state.to_string()).or_insert(next)
        }
    }

    impl Blocks for Fake {
        fn state_at(&self, pos: BlockPos) -> String {
            self.blocks
                .lock()
                .expect("fake")
                .get(&pos)
                .cloned()
                .unwrap_or_else(|| "minecraft:air".to_string())
        }
        fn set_block(&self, pos: BlockPos, state: &str) -> Option<i32> {
            if self.state_at(pos) == state {
                return None;
            }
            self.blocks
                .lock()
                .expect("fake")
                .insert(pos, state.to_string());
            Some(self.id(state))
        }
        fn default_state(&self, block: &str) -> Option<String> {
            Some(block.to_string())
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
        /// A test double, and a test **did** come and ask: the wire tests
        /// below reach `RedStoneWireBlock.getConnectingSide`, whose third
        /// branch is `isRedstoneConductor`. `false` for everything would make
        /// dust connect sideways through solid stone and every expected state
        /// in those tests would be a state vanilla never produces.
        ///
        /// Stone and only stone, which is the same double
        /// `crate::wire`'s own harness uses — deliberately, so a state
        /// computed in one file can be compared against the other. `glass` and
        /// `redstone_block` are full blocks that are **not** conductors, which
        /// is the distinction `block_props.json` grew a column for.
        fn redstone_conductor(&self, pos: BlockPos) -> bool {
            self.state_at(pos) == "minecraft:stone"
        }
        fn signal_source(&self, _: BlockPos) -> bool {
            false
        }
        /// The real per-state id, not a constant.
        ///
        /// It **was** `Some(1)` for everything, which made the table's whole
        /// keying inert through this method: every neighbour looked like the
        /// same block, so no test reaching the table this way could tell one
        /// from another. `the_indirect_walk_names_the_block_below_as_the_one_that_changed`
        /// is the test that needs it, and it is the only reason the walk's
        /// `from` position is observable at all.
        fn state_id(&self, state: &str) -> Option<i32> {
            Some(self.id(state))
        }
        fn replaceable(&self, pos: BlockPos, _: &crate::placement::Replacing<'_>) -> bool {
            is_air(&self.state_at(pos))
        }
        fn legacy_fluid_at(&self, _: BlockPos) -> String {
            "minecraft:air".to_string()
        }
        fn water_at(&self, _: BlockPos) -> bool {
            false
        }
    }

    /// A table with one rule in it: `state` in `direction` dies when the
    /// neighbour becomes any of `killers`.
    fn table(rules: &[(&str, Face, &[i32])]) -> ShapeTable {
        let mut rows: BTreeMap<String, [Option<Row>; 6]> = BTreeMap::new();
        let mut classes = Vec::new();
        for (state, face, killers) in rules {
            let mut bits = vec![0u8; 64];
            for id in *killers {
                let id = usize::try_from(*id).expect("id");
                bits[id >> 3] |= 1 << (id & 7);
            }
            classes.push(bits);
            let index = usize::try_from(face.to_3d()).expect("face");
            rows.entry((*state).to_string()).or_default()[index] = Some(Row {
                to: "minecraft:air".to_string(),
                class: classes.len() - 1,
            });
        }
        ShapeTable {
            rows,
            classes,
            states: 512,
            connective: 0,
            refused: 0,
            doors: std::collections::BTreeSet::new(),
            beds: std::collections::BTreeSet::new(),
            double_plants: std::collections::BTreeSet::new(),
            gravity_blocks: std::collections::BTreeSet::new(),
            stairs: std::collections::BTreeSet::new(),
            chests: std::collections::BTreeSet::new(),
            rails: std::collections::BTreeSet::new(),
            straight_rails: std::collections::BTreeSet::new(),
        }
    }

    /// The state string `oak_stairs` really has, property order included —
    /// `with_property` edits in place and a reordered string is a state
    /// nobody has heard of.
    fn stair(facing: &str, shape: &str) -> String {
        format!("minecraft:oak_stairs[facing={facing},half=bottom,shape={shape},waterlogged=false]")
    }

    /// **Check `180`'s scenario, driven through the real cascade.**
    ///
    /// `ci/checks/gameplay/180-stairs-make-a-corner-when-two-meet` looks at
    /// yaw 270 then yaw 180, which is `getHorizontalDirection()` `EAST` then
    /// `NORTH`, placing at `x` and `x+1`. The block the check watches is the
    /// **first** one, which nobody clicked: when the north-facing stair
    /// arrives to its east, the east-facing stair must stop being straight.
    ///
    /// This runs `set_block`, not `getStairsShape`, so it covers the parts
    /// the unit tests in `crate::placement` cannot: that the pair reaches
    /// `level_update_shape` at all, that the direction handed to it points
    /// back at the block that changed, and that the rewrite is appended to
    /// `updates` so a client is told about it.
    /// **`set_block` is the one place the block-entity ticker list is kept
    /// in step, so this asserts it from the outside.**
    ///
    /// `DaylightDetectorBlock.getTicker` gives the level a ticker per
    /// detector and vanilla keeps `blockEntityTickers` current from
    /// `LevelChunk.setBlockState`. Registering at the placement call site
    /// instead would miss every detector a cascade, a piston or a plugin
    /// wrote — which is a defect nothing else here would catch, because a
    /// detector that is never ticked looks exactly like one whose power has
    /// not changed.
    #[test]
    fn setting_a_daylight_detector_puts_it_on_the_ticker_list() {
        let pos = (11, 4, 11);
        let det = "minecraft:daylight_detector[inverted=false,power=0]";
        let fake = Fake::with(&[(pos, "minecraft:air")]);
        fake.id(det);
        let before = crate::sky::detector_count();
        let mut updates = Effects::new();
        assert!(set_block(
            &fake,
            None,
            pos,
            det,
            UPDATE_ALL,
            RECURSION_LIMIT,
            &mut updates
        ));
        assert_eq!(
            crate::sky::detector_count(),
            before + 1,
            "set_block did not register the detector"
        );
        let mut updates = Effects::new();
        assert!(set_block(
            &fake,
            None,
            pos,
            "minecraft:air",
            UPDATE_ALL,
            RECURSION_LIMIT,
            &mut updates
        ));
        assert_eq!(
            crate::sky::detector_count(),
            before,
            "breaking it did not take it off the list"
        );
    }
    #[test]
    fn a_stair_corners_when_a_second_one_arrives_beside_it() {
        let stairs = ["minecraft:oak_stairs"]
            .into_iter()
            .map(ToString::to_string)
            .collect();
        let shapes = table(&[]).with_stairs(stairs);
        let world = Fake::with(&[((0, 0, 0), &stair("east", "straight"))]);
        let mut updates = Effects::new();
        set_block(
            &world,
            Some(&shapes),
            (1, 0, 0),
            &stair("north", "straight"),
            UPDATE_ALL,
            RECURSION_LIMIT,
            &mut updates,
        );
        assert_eq!(
            world.state_at((0, 0, 0)),
            stair("east", "outer_left"),
            "the stair nobody clicked is the one that changes"
        );
        assert_eq!(
            world.state_at((1, 0, 0)),
            stair("north", "straight"),
            "the placed stair has no second neighbour and stays straight"
        );
        assert!(
            updates.blocks.iter().any(|(p, _)| *p == (0, 0, 0)),
            "the cascade has to be broadcast or the client never sees the corner"
        );
    }

    /// **The degenerate half of that pair.** A second stair facing the *same*
    /// way is not a corner, so nothing changes and nothing is broadcast for
    /// it. This passes under a `getClockWise` transcription, under a
    /// front/back swap, and under a `level_update_shape` that answered on the
    /// vertical axis as well — which is what makes the test above the gate
    /// and this one the control.
    #[test]
    fn a_stair_in_line_with_its_neighbour_does_not_corner() {
        let stairs = ["minecraft:oak_stairs"]
            .into_iter()
            .map(ToString::to_string)
            .collect();
        let shapes = table(&[]).with_stairs(stairs);
        let world = Fake::with(&[((0, 0, 0), &stair("east", "straight"))]);
        let mut updates = Effects::new();
        set_block(
            &world,
            Some(&shapes),
            (1, 0, 0),
            &stair("east", "straight"),
            UPDATE_ALL,
            RECURSION_LIMIT,
            &mut updates,
        );
        assert_eq!(world.state_at((0, 0, 0)), stair("east", "straight"));
        assert!(!updates.blocks.iter().any(|(p, _)| *p == (0, 0, 0)));
    }

    fn chest(facing: &str, kind: &str) -> String {
        format!("minecraft:chest[facing={facing},type={kind},waterlogged=false]")
    }

    fn chest_table() -> ShapeTable {
        table(&[]).with_chests(
            ["minecraft:chest", "minecraft:trapped_chest"]
                .into_iter()
                .map(ToString::to_string)
                .collect(),
        )
    }

    /// **Check `240`'s scenario: the block that changes is the one nobody
    /// clicked.** A north-facing single chest sits at the origin; a second
    /// arrives one step east already knowing it is the `RIGHT` half, and the
    /// first must become `LEFT`.
    ///
    /// The `getConnectedDirection(nb) == direction.getOpposite()` test at
    /// offsets 106-116 is the load-bearing one here: the neighbour's
    /// connected direction is `WEST` and the direction pointing back at it
    /// is `EAST`, so the two agree only when the pair really does face each
    /// other.
    #[test]
    fn a_chest_becomes_a_half_when_its_partner_arrives() {
        let shapes = chest_table();
        let world = Fake::with(&[((0, 0, 0), &chest("north", "single"))]);
        let mut updates = Effects::new();
        set_block(
            &world,
            Some(&shapes),
            (1, 0, 0),
            &chest("north", "right"),
            UPDATE_ALL,
            RECURSION_LIMIT,
            &mut updates,
        );
        assert_eq!(world.state_at((0, 0, 0)), chest("north", "left"));
        assert!(
            updates.blocks.iter().any(|(p, _)| *p == (0, 0, 0)),
            "the half nobody clicked has to be broadcast"
        );
    }

    /// **The degenerate half of that pair, and it is a real distinction.**
    /// The arriving chest faces the other way, so `state.FACING ==
    /// nb.FACING` fails at offsets 88-103 and nothing happens. This passes
    /// under a `ChestType.getOpposite` that returned its argument, under a
    /// missing `nbType != SINGLE` test, and under a swapped
    /// `getConnectedDirection` — every bug the test above catches.
    #[test]
    fn a_chest_facing_the_other_way_is_not_a_partner() {
        let shapes = chest_table();
        let world = Fake::with(&[((0, 0, 0), &chest("north", "single"))]);
        let mut updates = Effects::new();
        set_block(
            &world,
            Some(&shapes),
            (1, 0, 0),
            &chest("south", "right"),
            UPDATE_ALL,
            RECURSION_LIMIT,
            &mut updates,
        );
        assert_eq!(world.state_at((0, 0, 0)), chest("north", "single"));
        assert!(!updates.blocks.iter().any(|(p, _)| *p == (0, 0, 0)));
    }

    /// A chest already half of a double is not rewritten by a third one:
    /// `state.getValue(TYPE) == SINGLE` at offsets 67-77.
    #[test]
    fn a_chest_that_is_already_a_half_is_left_alone() {
        let shapes = chest_table();
        let world = Fake::with(&[((0, 0, 0), &chest("north", "left"))]);
        let mut updates = Effects::new();
        set_block(
            &world,
            Some(&shapes),
            (1, 0, 0),
            &chest("north", "right"),
            UPDATE_ALL,
            RECURSION_LIMIT,
            &mut updates,
        );
        assert_eq!(world.state_at((0, 0, 0)), chest("north", "left"));
    }

    /// The un-pairing branch, offsets 138-160, tested **directly** rather
    /// than through `set_block`.
    ///
    /// It has to be direct, because in production this branch is dead code:
    /// `update_shape.json` already carries all 32 of these pairs — every
    /// `LEFT`/`RIGHT` chest and trapped chest losing its partner along its
    /// own connected direction — and the table runs first. Driving it
    /// through `set_block` against the real table would measure the
    /// extraction and report it as this port, which is the sixteen-checks
    /// mistake in miniature. The rule is written out anyway so the port is
    /// the whole method, and this test says which half of it the corpus can
    /// actually see.
    #[test]
    fn a_half_that_loses_its_partner_goes_back_to_single() {
        // A LEFT chest facing north is connected to the EAST.
        assert_eq!(
            ShapeTable::chest_update_shape(&chest("north", "left"), Face::East, "minecraft:air")
                .as_deref(),
            Some(chest("north", "single").as_str()),
        );
        // ...and not in any other direction.
        assert_eq!(
            ShapeTable::chest_update_shape(&chest("north", "left"), Face::West, "minecraft:air"),
            None,
        );
    }

    /// A stair does not re-corner when the block above or below changes:
    /// `updateShape` gates the whole rule on
    /// `direction.getAxis().isHorizontal()` at offsets 34-42 and otherwise
    /// falls through to `Block.updateShape`, which returns the state
    /// unchanged.
    #[test]
    fn a_vertical_neighbour_does_not_reshape_a_stair() {
        let stairs = ["minecraft:oak_stairs"]
            .into_iter()
            .map(ToString::to_string)
            .collect();
        let shapes = table(&[]).with_stairs(stairs);
        // A cornering stair to the east, so the shape rule *would* have
        // something to say if the axis gate let it run.
        let world = Fake::with(&[
            ((0, 0, 0), &stair("east", "straight")),
            ((1, 0, 0), &stair("north", "straight")),
        ]);
        let mut updates = Effects::new();
        set_block(
            &world,
            Some(&shapes),
            (0, 1, 0),
            "minecraft:stone",
            UPDATE_ALL,
            RECURSION_LIMIT,
            &mut updates,
        );
        assert_eq!(
            world.state_at((0, 0, 0)),
            stair("east", "straight"),
            "a block placed above a stair is the vertical case and changes nothing"
        );
    }

    /// **The `2001` goes out for the cascade and never for the dug block**,
    /// which is the difference between buying five corpus scenarios and
    /// losing `010-break-block`.
    ///
    /// Both halves of the pinned trace are here. A plain dig — `remove_block`
    /// on a block with nothing above it — produces no level event at all,
    /// because the only site that would announce it is
    /// `Block.spawnDestroyParticles`, which passes the digger to the
    /// four-argument `Level.levelEvent` and is therefore invisible to the one
    /// client `010` reads. A dig that knocks something down produces exactly
    /// one, at the **knocked-down** block and carrying **its** state id,
    /// because `Block.updateOrDestroy` reaches `Level.destroyBlock` with
    /// `aconst_null` in the entity slot.
    ///
    /// The recorded numbers this reproduces, from 2026-08-07 02:19:
    /// `010-break-block` actor 0 / observer 1 at the dug position;
    /// `260-digging-a-door-takes-both-halves` actor 1, at `[1, 1, 0]` with the
    /// upper half's id, for a dig at `[1, 0, 0]`.
    #[test]
    fn only_a_cascaded_destroy_announces_itself_to_the_digger() {
        // A lone block: nothing above it, so nothing cascades.
        let world = Fake::with(&[((0, 63, 0), "minecraft:stone")]);
        let shapes = table(&[]);
        let mut updates = Effects::new();
        assert!(remove_block(
            &world,
            Some(&shapes),
            (0, 63, 0),
            &mut updates
        ));
        assert_eq!(updates.blocks.len(), 1, "the dug block, broadcast");
        assert!(
            updates.level_events.is_empty(),
            "a plain dig announces nothing to the digger — this is why 010 passes"
        );

        // The same dig with a torch on top: the torch is destroyed by the
        // cascade and that one *is* announced.
        let world = Fake::with(&[
            ((0, 63, 0), "minecraft:stone"),
            ((0, 64, 0), "minecraft:torch"),
        ]);
        let air = world.id("minecraft:air");
        let torch = world.id("minecraft:torch");
        let shapes = table(&[("minecraft:torch", Face::Down, &[air])]);
        let mut updates = Effects::new();
        assert!(remove_block(
            &world,
            Some(&shapes),
            (0, 63, 0),
            &mut updates
        ));
        assert_eq!(
            updates.level_events,
            vec![(LEVEL_EVENT_DESTROY, (0, 64, 0), torch)],
            "at the torch, not at the support, and carrying the torch's own id"
        );
        // `Block.updateOrDestroy` passes `(flags & 32) == 0` as `dropBlock`,
        // and `removeBlock`'s flags are 3: the torch drops, after its 2001.
        assert_eq!(
            updates.drops,
            vec![DropRequest {
                pos: (0, 64, 0),
                state: "minecraft:torch".into(),
                after_events: 1,
            }]
        );

        // Destroying air is a no-op in `Level.destroyBlock` before the level
        // event, so a cascade that reaches an already-empty position is
        // silent. Without the air check this would announce a destruction of
        // nothing at every position a cascade visited.
        let world = Fake::with(&[((0, 63, 0), "minecraft:stone")]);
        let mut updates = Effects::new();
        assert!(!destroy_block(
            &world,
            Some(&table(&[])),
            (0, 70, 0),
            true,
            RECURSION_LIMIT,
            &mut updates
        ));
        assert!(updates.level_events.is_empty());
        assert!(updates.drops.is_empty(), "air drops nothing");
    }

    /// The whole of a double-plant dig, both directions, against the packet
    /// counts the corpus measured.
    ///
    /// `237` digs the lower half and `238` the upper, and vanilla answers both
    /// with **one** level event on the actor's stream at the **upper**
    /// position — by two different routes, which is the point of testing both.
    /// Digging the lower half destroys the upper through `updateShape`;
    /// digging the upper half runs `preventDropFromBottomPart`, whose
    /// `setBlock(below, air, 35)` cascades back up and destroys the upper half
    /// itself. Neither route emits at the dug block.
    #[test]
    fn a_double_plant_dig_announces_the_upper_half_whichever_half_was_dug() {
        let plants: std::collections::BTreeSet<String> =
            ["minecraft:sunflower".to_string()].into_iter().collect();
        let upper = "minecraft:sunflower[half=upper]";
        let lower = "minecraft:sunflower[half=lower]";

        let dig = |at: BlockPos| -> Effects {
            let world = Fake::with(&[((0, 1, 0), upper), ((0, 0, 0), lower)]);
            let air = world.id("minecraft:air");
            // The one row the table really carries for a sunflower:
            // `half=upper`, `dir=DOWN`, dies when what is below becomes air.
            let t = table(&[(upper, Face::Down, &[air])]).with_double_plants(plants.clone());
            let mut updates = Effects::new();
            // `ServerPlayerGameMode.destroyBlock`'s two calls, in its order.
            player_will_destroy(&world, Some(&t), at, &mut updates);
            remove_block(&world, Some(&t), at, &mut updates);
            assert_eq!(world.state_at((0, 0, 0)), "minecraft:air");
            assert_eq!(world.state_at((0, 1, 0)), "minecraft:air");
            updates
        };

        let upper_id = Fake::with(&[((0, 1, 0), upper), ((0, 0, 0), lower)]).id(upper);
        for at in [(0, 0, 0), (0, 1, 0)] {
            let got = dig(at);
            assert_eq!(
                got.level_events,
                vec![(LEVEL_EVENT_DESTROY, (0, 1, 0), upper_id)],
                "digging {at:?}: one 2001, at the upper half"
            );
        }
    }

    /// `preventDropFromBottomPart`, and the three ways to get it wrong.
    ///
    /// This is the rule `ci/checks/gameplay/238` measures. It is tested here
    /// as well as there because the check can only see the creative path
    /// through a real client, and the discriminating cases below — a
    /// *different* double plant below, and the lower half broken rather than
    /// the upper — are ones no scenario in the corpus sets up.
    #[test]
    fn breaking_the_upper_half_of_a_double_plant_clears_the_lower_one() {
        let plants = ["minecraft:sunflower", "minecraft:peony"]
            .into_iter()
            .map(ToString::to_string)
            .collect();
        let t = table(&[]).with_double_plants(plants);
        let upper = "minecraft:sunflower[half=upper]";
        let lower = "minecraft:sunflower[half=lower]";

        // The rule itself: break the top, the bottom goes.
        let f = Fake::with(&[((0, 1, 0), upper), ((0, 0, 0), lower)]);
        let mut updates = Effects::new();
        player_will_destroy(&f, Some(&t), (0, 1, 0), &mut updates);
        assert_eq!(
            f.state_at((0, 0, 0)),
            "minecraft:air",
            "the lower half is cleared before the upper one is removed"
        );
        assert_eq!(updates.blocks.len(), 1, "and the client is told, once");

        // `nb.is(this)` is the SAME BLOCK, not `instanceof`. A peony's lower
        // half under a sunflower's upper half is untouched — the door's rule
        // would have taken it.
        let f = Fake::with(&[
            ((0, 1, 0), upper),
            ((0, 0, 0), "minecraft:peony[half=lower]"),
        ]);
        let mut updates = Effects::new();
        player_will_destroy(&f, Some(&t), (0, 1, 0), &mut updates);
        assert_eq!(
            f.state_at((0, 0, 0)),
            "minecraft:peony[half=lower]",
            "a different double plant below is not the other half of this one"
        );
        assert!(updates.blocks.is_empty());

        // Only UPPER fires. Breaking the lower half is `updateShape`'s job,
        // and a rule that fired both ways would clear the block under the
        // plant.
        let f = Fake::with(&[
            ((0, 1, 0), upper),
            ((0, 0, 0), lower),
            ((0, -1, 0), "minecraft:stone"),
        ]);
        let mut updates = Effects::new();
        player_will_destroy(&f, Some(&t), (0, 0, 0), &mut updates);
        assert_eq!(
            f.state_at((0, -1, 0)),
            "minecraft:stone",
            "breaking the lower half must not clear what it stands on"
        );
        assert!(updates.blocks.is_empty());

        // A block that is not a double plant at all keeps its neighbour.
        let f = Fake::with(&[((0, 1, 0), "minecraft:stone"), ((0, 0, 0), lower)]);
        let mut updates = Effects::new();
        player_will_destroy(&f, Some(&t), (0, 1, 0), &mut updates);
        assert_eq!(f.state_at((0, 0, 0)), lower);
        assert!(updates.blocks.is_empty());
    }

    /// `BedBlock.playerWillDestroy`, and the position its cascade destroys.
    ///
    /// The block outcome here was already right before this rule existed —
    /// `remove_block` at the foot cascaded through `BedBlock.updateShape` at
    /// the head and both halves went — so the only thing that separates the
    /// two arrangements is **which** block the cascade destroyed, and that is
    /// only visible in the level event. Vanilla clears the head by hand, so
    /// the cascade takes the foot; the arrangement without this rule cleared
    /// the foot, so the cascade took the head. Measured on 2026-08-07:
    /// `236-digging-a-bed-takes-both-halves` had the event at the head with
    /// the head's id where vanilla put it at the foot with the foot's.
    #[test]
    fn a_beds_foot_clears_its_head_and_the_cascade_destroys_the_foot() {
        let beds: std::collections::BTreeSet<String> = [
            "minecraft:red_bed".to_string(),
            "minecraft:white_bed".to_string(),
        ]
        .into_iter()
        .collect();
        let foot = "minecraft:red_bed[facing=south,occupied=false,part=foot]";
        let head = "minecraft:red_bed[facing=south,occupied=false,part=head]";
        // `facing=south` is `(0, 0, 1)`, so the head is one step in +z.
        let world = Fake::with(&[((0, 0, 0), foot), ((0, 0, 1), head)]);
        let air = world.id("minecraft:air");
        let foot_id = world.id(foot);
        let t = table(&[(foot, Face::South, &[air])]).with_beds(beds.clone());

        let mut updates = Effects::new();
        player_will_destroy(&world, Some(&t), (0, 0, 0), &mut updates);
        remove_block(&world, Some(&t), (0, 0, 0), &mut updates);
        assert_eq!(world.state_at((0, 0, 1)), "minecraft:air", "the head went");
        assert_eq!(world.state_at((0, 0, 0)), "minecraft:air", "and the foot");
        assert_eq!(
            updates.level_events,
            vec![(LEVEL_EVENT_DESTROY, (0, 0, 0), foot_id)],
            "at the FOOT with the foot's id — the head is cleared by hand and \
             its 2001 is the player-excluded overload"
        );

        // Breaking the HEAD runs nothing: `getNeighbourDirection` is only
        // consulted inside the `part == FOOT` branch.
        let world = Fake::with(&[((0, 0, 0), foot), ((0, 0, 1), head)]);
        let mut updates = Effects::new();
        player_will_destroy(&world, Some(&t), (0, 0, 1), &mut updates);
        assert!(updates.blocks.is_empty(), "the head branch does not exist");
        assert_eq!(world.state_at((0, 0, 0)), foot);

        // `s2.is(this)` is the same block. A white bed's head beside a red
        // bed's foot is not the other half of it.
        let other = "minecraft:white_bed[facing=south,occupied=false,part=head]";
        let world = Fake::with(&[((0, 0, 0), foot), ((0, 0, 1), other)]);
        let mut updates = Effects::new();
        player_will_destroy(&world, Some(&t), (0, 0, 0), &mut updates);
        assert_eq!(
            world.state_at((0, 0, 1)),
            other,
            "a different bed is untouched"
        );
        assert!(updates.blocks.is_empty());
    }

    /// The door rule, which no row can hold. Two neighbours differing in
    /// exactly one property must give two different answers — a rule that
    /// returned a *fixed* state would pass a test that only ever opened a
    /// door.
    #[test]
    fn a_door_half_takes_the_whole_state_of_the_other_half() {
        let t = table(&[]).with_doors(door_set());
        let upper =
            "minecraft:oak_door[facing=south,half=upper,hinge=left,open=false,powered=false]";
        for open in ["true", "false"] {
            let lower = format!(
                "minecraft:oak_door[facing=south,half=lower,hinge=left,open={open},powered=false]"
            );
            assert_eq!(
                t.typed_update_shape(upper, Face::Down, &lower).as_deref(),
                Some(
                    format!(
                        "minecraft:oak_door[facing=south,half=upper,hinge=left,open={open},powered=false]"
                    )
                    .as_str()
                ),
                "the upper half copies the lower's whole state with HALF put back"
            );
        }
        // Support gone: the upper half goes with it.
        assert_eq!(
            t.typed_update_shape(upper, Face::Down, "minecraft:air")
                .as_deref(),
            Some("minecraft:air")
        );
        // And the branch is gated on the axis and the half. Asked from any
        // direction that is not the one holding its other half, it declines
        // and the table's own answer stands.
        assert!(t
            .typed_update_shape(upper, Face::Up, "minecraft:air")
            .is_none());
        assert!(t
            .typed_update_shape(upper, Face::North, "minecraft:air")
            .is_none());
        let lower =
            "minecraft:oak_door[facing=south,half=lower,hinge=left,open=false,powered=false]";
        assert!(t
            .typed_update_shape(lower, Face::Down, "minecraft:air")
            .is_none());
        assert_eq!(
            t.typed_update_shape(lower, Face::Up, "minecraft:air")
                .as_deref(),
            Some("minecraft:air")
        );
    }

    /// Without the class map nothing typed fires, and it must decline rather
    /// than guess — the failure that costs a corpus run is a class lookup
    /// that silently answers nothing.
    #[test]
    fn no_class_map_means_no_typed_rule() {
        let t = table(&[]);
        let upper =
            "minecraft:oak_door[facing=south,half=upper,hinge=left,open=false,powered=false]";
        assert!(t
            .typed_update_shape(upper, Face::Down, "minecraft:air")
            .is_none());
    }

    /// The rule follows the resolved **set**, never the id, and the set is
    /// what carries `instanceof`.
    ///
    /// A block named like a door that the extract did not resolve to one gets
    /// nothing; a block named nothing like a door that it *did* resolve gets
    /// the rule. `WeatheringCopperDoorBlock` is the real case — four copper
    /// doors are subclasses, and every door check in the corpus places oak.
    #[test]
    fn the_door_rule_follows_the_resolved_set_and_not_the_name() {
        let named_but_not_resolved = table(&[]).with_doors(std::collections::BTreeSet::new());
        let upper =
            "minecraft:oak_door[facing=south,half=upper,hinge=left,open=false,powered=false]";
        assert!(named_but_not_resolved
            .typed_update_shape(upper, Face::Down, "minecraft:air")
            .is_none());

        let resolved =
            table(&[]).with_doors(["minecraft:copper_door".to_string()].into_iter().collect());
        let copper =
            "minecraft:copper_door[facing=south,half=upper,hinge=left,open=false,powered=false]";
        assert_eq!(
            resolved
                .typed_update_shape(copper, Face::Down, "minecraft:air")
                .as_deref(),
            Some("minecraft:air"),
            "a subclass door resolved into the set gets the rule"
        );
    }

    /// `BedBlock.updateShape`, and the three ways it differs from the door's.
    ///
    /// A rule generalised across the two would get every one of them wrong,
    /// which is why they are written out separately.
    #[test]
    fn a_bed_part_copies_only_occupied_and_only_from_its_own_block() {
        let beds: std::collections::BTreeSet<String> = [
            "minecraft:red_bed".to_string(),
            "minecraft:blue_bed".to_string(),
        ]
        .into_iter()
        .collect();
        let t = table(&[]).with_beds(beds);
        let foot = "minecraft:red_bed[facing=south,occupied=false,part=foot]";

        // 1. The direction is horizontal and comes from FACING and PART:
        //    a foot looks along `facing`, a head against it. Asked from any
        //    other side the rule declines and the table's answer stands.
        for d in [Face::North, Face::Up, Face::Down, Face::West] {
            assert!(
                t.typed_update_shape(foot, d, "minecraft:air").is_none(),
                "{d:?} is not the direction this foot's head lies in"
            );
        }
        let head = "minecraft:red_bed[facing=south,occupied=false,part=head]";
        assert!(t
            .typed_update_shape(head, Face::South, "minecraft:air")
            .is_none());
        assert_eq!(
            t.typed_update_shape(head, Face::North, "minecraft:air")
                .as_deref(),
            Some("minecraft:air"),
            "a head looks *against* its facing"
        );

        // 2. The answer is the SUBJECT's state with OCCUPIED copied, not the
        //    neighbour's state. The two differ in `part`, so a rule that
        //    returned the neighbour would be caught here.
        assert_eq!(
            t.typed_update_shape(
                foot,
                Face::South,
                "minecraft:red_bed[facing=south,occupied=true,part=head]"
            )
            .as_deref(),
            Some("minecraft:red_bed[facing=south,occupied=true,part=foot]")
        );

        // 3. `nb.is(this)` is the SAME BLOCK, not `instanceof`. A blue bed's
        //    head beside a red bed's foot is air, and this is the assertion
        //    that separates it from the door rule.
        assert_eq!(
            t.typed_update_shape(
                foot,
                Face::South,
                "minecraft:blue_bed[facing=south,occupied=true,part=head]"
            )
            .as_deref(),
            Some("minecraft:air"),
            "a different bed block is not this bed's other half"
        );
        assert_eq!(
            t.typed_update_shape(foot, Face::South, "minecraft:air")
                .as_deref(),
            Some("minecraft:air")
        );
    }

    fn door_set() -> std::collections::BTreeSet<String> {
        ["minecraft:oak_door".to_string()].into_iter().collect()
    }

    /// The order is vanilla's, and it is **not** `Direction.values()`.
    #[test]
    fn the_shape_order_is_west_east_north_south_down_up() {
        assert_eq!(
            UPDATE_SHAPE_ORDER.map(Face::to_3d),
            [4, 5, 2, 3, 0, 1],
            "UPDATE_SHAPE_ORDER, not the enum order 0..5"
        );
    }

    /// Breaking the block under a torch takes the torch with it, and the
    /// caller is told about both changes in the order they happened.
    #[test]
    fn a_torch_falls_when_its_support_goes() {
        let world = Fake::with(&[
            ((0, 63, 0), "minecraft:stone"),
            ((0, 64, 0), "minecraft:torch"),
        ]);
        let air = world.id("minecraft:air");
        let torch = world.id("minecraft:torch");
        let shapes = table(&[("minecraft:torch", Face::Down, &[air])]);

        let mut updates = Effects::new();
        assert!(remove_block(
            &world,
            Some(&shapes),
            (0, 63, 0),
            &mut updates
        ));

        assert_eq!(
            world.state_at((0, 64, 0)),
            "minecraft:air",
            "the torch went"
        );
        assert_eq!(
            updates.blocks,
            vec![((0, 63, 0), air), ((0, 64, 0), air)],
            "the cause is broadcast before the consequence"
        );
        // **The torch was destroyed by a cascade and the digger hears it.**
        // `Level.removeBlock` writes air at the support with no level event of
        // its own — `spawnDestroyParticles` is the four-argument overload and
        // is not produced here — and the torch above it is taken by
        // `Block.updateOrDestroy`, which reaches `Level.destroyBlock` with a
        // null entity. So exactly one event, at the torch and not at the
        // support. A version that emitted at both would take
        // `010-break-block` from strict `match` to strict `diverge`.
        assert_eq!(
            updates.level_events,
            vec![(LEVEL_EVENT_DESTROY, (0, 64, 0), torch)],
            "one 2001, at the cascaded block, carrying its own state id"
        );
    }

    /// A column of them goes all the way down, one shape update per level.
    #[test]
    fn a_column_of_torches_collapses() {
        let mut states: Vec<(BlockPos, &str)> = vec![((0, 63, 0), "minecraft:stone")];
        for y in 64..70 {
            states.push(((0, y, 0), "minecraft:torch"));
        }
        let world = Fake::with(&states);
        let air = world.id("minecraft:air");
        let shapes = table(&[("minecraft:torch", Face::Down, &[air])]);

        let mut updates = Effects::new();
        remove_block(&world, Some(&shapes), (0, 63, 0), &mut updates);
        assert_eq!(updates.blocks.len(), 7, "the support and six torches");
        for y in 64..70 {
            assert!(is_air(&world.state_at((0, y, 0))), "torch at {y} survived");
        }
    }

    /// The direction points *back* at what changed. A wall torch attached to
    /// the block to its north is destroyed by a change to the **north**, and
    /// a change to the south does nothing — a port that passed `d` instead of
    /// `d.getOpposite()` has these exactly the wrong way round.
    #[test]
    fn a_wall_torch_watches_the_block_it_hangs_on() {
        let world = Fake::with(&[
            ((0, 64, -1), "minecraft:stone"),
            ((0, 64, 0), "minecraft:wall_torch[facing=south]"),
            ((0, 64, 1), "minecraft:stone"),
        ]);
        let air = world.id("minecraft:air");
        let shapes = table(&[("minecraft:wall_torch[facing=south]", Face::North, &[air])]);

        let mut updates = Effects::new();
        remove_block(&world, Some(&shapes), (0, 64, 1), &mut updates);
        assert_eq!(
            world.state_at((0, 64, 0)),
            "minecraft:wall_torch[facing=south]",
            "the block to the south is not what holds it"
        );

        updates.blocks.clear();
        remove_block(&world, Some(&shapes), (0, 64, -1), &mut updates);
        assert!(is_air(&world.state_at((0, 64, 0))), "its support went");
    }

    /// Without a table nothing cascades and nothing panics — a checkout with
    /// no extract still runs, it simply leaves torches floating.
    #[test]
    fn no_table_means_no_cascade() {
        let world = Fake::with(&[
            ((0, 63, 0), "minecraft:stone"),
            ((0, 64, 0), "minecraft:torch"),
        ]);
        let mut updates = Effects::new();
        remove_block(&world, None, (0, 63, 0), &mut updates);
        assert_eq!(updates.blocks.len(), 1);
        assert_eq!(world.state_at((0, 64, 0)), "minecraft:torch");
    }

    /// A state the table does not carry, and a neighbour the class does not
    /// name, are both "nothing happens" rather than a panic.
    #[test]
    fn an_unknown_state_or_neighbour_is_nothing_happening() {
        let shapes = table(&[("minecraft:torch", Face::Down, &[3])]);
        assert_eq!(shapes.update_shape("minecraft:stone", Face::Down, 3), None);
        assert_eq!(shapes.update_shape("minecraft:torch", Face::Up, 3), None);
        assert_eq!(shapes.update_shape("minecraft:torch", Face::Down, 9), None);
        assert_eq!(
            shapes.update_shape("minecraft:torch", Face::Down, 3),
            Some("minecraft:air")
        );
        // Out of the bitmap entirely, which a hostile id would be.
        assert_eq!(shapes.update_shape("minecraft:torch", Face::Down, -1), None);
        assert_eq!(
            shapes.update_shape("minecraft:torch", Face::Down, 1_000_000),
            None
        );
    }

    #[test]
    fn base64_decodes_the_way_java_encoded_it() {
        assert_eq!(decode_base64(""), Some(vec![]));
        assert_eq!(decode_base64("AQ=="), Some(vec![1]));
        assert_eq!(decode_base64("AAE="), Some(vec![0, 1]));
        assert_eq!(decode_base64("/w=="), Some(vec![255]));
        assert_eq!(decode_base64("AAECAwQ="), Some(vec![0, 1, 2, 3, 4]));
        assert_eq!(decode_base64("!!!!"), None);
    }

    /// The three air blocks, because a cave is `cave_air` and a cascade that
    /// only knows `minecraft:air` takes the wrong branch underground.
    #[test]
    fn all_three_air_blocks_are_air() {
        assert!(is_air("minecraft:air"));
        assert!(is_air("minecraft:cave_air"));
        assert!(is_air("minecraft:void_air"));
        assert!(!is_air("minecraft:stone"));
        assert!(!is_air("minecraft:oak_log[axis=y]"));
    }

    // ---- `updateIndirectNeighbourShapes` -------------------------------
    //
    // **These test the walk, not check `450`.** The scenario in `450` reaches
    // one direction, one probe, one of `updateShape`'s four branches, and only
    // the *new* state's call. Eight rail tests passed under four injected bugs
    // last link for exactly that reason — a test built from a corpus scenario
    // exercises the path that scenario takes and no other. So the integration
    // test below is one of seven, and the other six drive
    // `update_indirect_neighbour_shapes` directly, where the cascade cannot
    // arrive at an observation by a second route and make a dead branch look
    // live.

    /// A dust state with its four sides named in the order the state string
    /// really has them, which is alphabetical and not `HORIZONTAL`'s.
    fn dust(east: &str, north: &str, south: &str, west: &str) -> String {
        format!(
            "minecraft:redstone_wire[east={east},north={north},power=0,south={south},west={west}]"
        )
    }

    /// A table that knows nothing, so every answer below comes from the ported
    /// rule rather than from an extracted row. `update_shape.json`'s 1,296 wire
    /// rows are all `dir=DOWN -> air` and would answer none of these.
    fn wire_table() -> ShapeTable {
        table(&[])
    }

    /// **Check `450`'s arrangement, driven through the real `set_block`** —
    /// the one test here that is allowed to look like the corpus, because its
    /// job is to prove the three ported pieces meet.
    ///
    /// Dust at `[1,0,0]` on stone, stone at `[2,0,0]`, then dust placed on top
    /// of that stone at `[2,1,0]`. `[1,0,0]` is **diagonal** from the write, so
    /// no six-face cascade reaches it; only the indirect walk does, and only
    /// `wire::update_shape` can answer once it arrives.
    ///
    /// The expected state is `east=up` **and** `west=side`: a dust with exactly
    /// one connection is a line, not a stub, because `getConnectionState`'s
    /// `noNS` forces the opposite side. Asserting only `east=up` would pass
    /// under a port that lost the second half of that rule.
    #[test]
    fn dust_climbs_a_block_when_dust_lands_on_top_of_it_diagonally() {
        let shapes = wire_table();
        let world = Fake::with(&[
            ((1, -1, 0), "minecraft:stone"),
            ((1, 0, 0), &dust("none", "none", "none", "none")),
            ((2, -1, 0), "minecraft:stone"),
            ((2, 0, 0), "minecraft:stone"),
        ]);
        let mut updates = Effects::new();
        set_block(
            &world,
            Some(&shapes),
            (2, 1, 0),
            &dust("side", "none", "none", "side"),
            UPDATE_ALL,
            RECURSION_LIMIT,
            &mut updates,
        );
        assert_eq!(
            world.state_at((1, 0, 0)),
            dust("up", "none", "none", "side"),
            "the dust two blocks away and one down is what this check watches"
        );
        assert!(
            updates.blocks.iter().any(|(p, _)| *p == (1, 0, 0)),
            "a diagonal that changes and is never broadcast is invisible to a client"
        );
    }

    /// **The gate at offsets 39-66.** A dust reaches out along the sides it
    /// claims to use, not along all four.
    ///
    /// The subject's `west` is connected and its `east` is not, and there is a
    /// dust diagonally below on *both* sides. West must be notified — which is
    /// what stops this test passing because the walk is dead — and east must
    /// not.
    /// The world must hold the subject state at the subject position, exactly
    /// as it does in vanilla: `Level.setBlock` writes first and calls this
    /// afterwards, and the walk's own probes read the level back. Two of these
    /// tests were written with the state passed only as the argument and both
    /// failed for that reason before they failed for any interesting one.
    #[test]
    fn the_indirect_walk_skips_a_side_the_wire_does_not_use() {
        let shapes = wire_table();
        let subject = dust("none", "none", "none", "side");
        let east_before = dust("side", "none", "none", "side");
        let world = Fake::with(&[
            ((1, -1, 0), "minecraft:stone"),
            ((1, 0, 0), &dust("none", "none", "none", "none")),
            ((2, -1, 0), "minecraft:stone"),
            ((2, 0, 0), "minecraft:stone"),
            ((2, 1, 0), &subject),
            ((3, -1, 0), "minecraft:stone"),
            ((3, 0, 0), &east_before),
        ]);
        let mut updates = Effects::new();
        update_indirect_neighbour_shapes(
            &world,
            &shapes,
            &subject,
            (2, 1, 0),
            UPDATE_ALL,
            RECURSION_LIMIT - 1,
            &mut updates,
        );
        assert_eq!(
            world.state_at((1, 0, 0)),
            dust("up", "none", "none", "side"),
            "west is connected, so the walk goes that way and the diagonal answers"
        );
        assert_eq!(
            world.state_at((3, 0, 0)),
            east_before,
            "east is `none`, so the diagonal below it is never asked — and it \
             would have gone to west=up if it had been"
        );
    }

    /// **The `ifne` at offset 87, which is the easiest instruction in the
    /// method to read backwards.** The diagonals are probed only when the
    /// orthogonal neighbour is **not** a wire, because a wire beside a wire is
    /// already served by the six-face cascade.
    ///
    /// Read as `ifeq` the method becomes a duplicate of the cascade it exists
    /// to complement, and every arrangement in which the orthogonal is air —
    /// which is every arrangement the corpus has — still passes.
    /// The arrangement is check `450`'s with **one block added** — a wire at
    /// `[1,1,0]`, the orthogonal neighbour. Everything that made `450` move is
    /// still here, so under the inverted branch the dust at `[1,0,0]` goes to
    /// `east=up` exactly as it does there. That is the point: without the
    /// added wire the inverted branch changes nothing and the test passes
    /// either way.
    #[test]
    fn the_indirect_walk_stops_when_the_orthogonal_neighbour_is_a_wire() {
        let shapes = wire_table();
        let subject = dust("none", "none", "none", "side");
        let world = Fake::with(&[
            ((1, -1, 0), "minecraft:stone"),
            ((1, 0, 0), &dust("none", "none", "none", "none")),
            ((1, 1, 0), &dust("none", "none", "none", "none")),
            ((2, -1, 0), "minecraft:stone"),
            ((2, 0, 0), "minecraft:stone"),
            ((2, 1, 0), &subject),
        ]);
        let mut updates = Effects::new();
        update_indirect_neighbour_shapes(
            &world,
            &shapes,
            &subject,
            (2, 1, 0),
            UPDATE_ALL,
            RECURSION_LIMIT - 1,
            &mut updates,
        );
        assert_eq!(
            world.state_at((1, 0, 0)),
            dust("none", "none", "none", "none"),
            "the orthogonal neighbour at [1,1,0] is a wire, so neither probe runs"
        );
        assert!(
            updates.blocks.is_empty(),
            "nothing was asked, so nothing can have been broadcast"
        );
    }

    /// **The UP probe, offsets 157-188.** Everything in the corpus and every
    /// test above reaches the DOWN probe at 90-115; a port that transcribed
    /// only the first half of the loop body passes all of them.
    /// The mirror of the arrangement above: the subject is at `[2,0,0]`, the
    /// orthogonal `[1,0,0]` is stone rather than a wire, and the dust sits one
    /// **up** and one across at `[1,1,0]`. Nothing is placed at `[1,-1,0]`, so
    /// the DOWN probe finds air and this observation has only one possible
    /// source.
    #[test]
    fn the_indirect_walk_probes_upwards_as_well_as_down() {
        let shapes = wire_table();
        let subject = dust("none", "none", "none", "side");
        let world = Fake::with(&[
            ((1, 0, 0), "minecraft:stone"),
            ((1, 1, 0), &dust("none", "none", "none", "none")),
            ((2, -1, 0), "minecraft:stone"),
            ((2, 0, 0), &subject),
            ((2, 1, 0), "minecraft:stone"),
            ((2, 2, 0), &dust("none", "none", "none", "none")),
        ]);
        let mut updates = Effects::new();
        update_indirect_neighbour_shapes(
            &world,
            &shapes,
            &subject,
            (2, 0, 0),
            UPDATE_ALL,
            RECURSION_LIMIT - 1,
            &mut updates,
        );
        assert_eq!(
            world.state_at((1, 1, 0)),
            dust("up", "none", "none", "side"),
            "the dust one up and one across is only reachable through the UP probe"
        );
    }

    /// **The old state's call, offsets 195-205 of `Level.setBlock`** — the
    /// half of the pair that has nothing to do with placing anything.
    ///
    /// A wire that is *removed* has to tell the diagonals that were connected
    /// to it. The new state here is air, so the new state's indirect call does
    /// nothing at all: a port that added only that call leaves the dust below
    /// still pointing `up` at a block that is no longer there, and every
    /// placing test above still passes.
    #[test]
    fn removing_a_wire_notifies_the_diagonal_it_left() {
        let shapes = wire_table();
        let world = Fake::with(&[
            ((1, -1, 0), "minecraft:stone"),
            ((1, 0, 0), &dust("up", "none", "none", "side")),
            ((2, -1, 0), "minecraft:stone"),
            ((2, 0, 0), "minecraft:stone"),
            ((2, 1, 0), &dust("side", "none", "none", "side")),
        ]);
        let mut updates = Effects::new();
        set_block(
            &world,
            Some(&shapes),
            (2, 1, 0),
            "minecraft:air",
            UPDATE_ALL,
            RECURSION_LIMIT,
            &mut updates,
        );
        assert_ne!(
            world.state_at((1, 0, 0)),
            dust("up", "none", "none", "side"),
            "the dust is still climbing a block that was taken away"
        );
        assert_eq!(
            world.state_at((1, 0, 0)),
            dust("side", "side", "side", "side"),
            "with nothing left to connect to it re-derives as a cross"
        );
    }

    /// **A non-wire subject reaches the empty default.**
    /// `Block.updateIndirectNeighbourShapes`'s whole body is `return`, and
    /// `RedStoneWireBlock` is the only override in the jar.
    ///
    /// The subject is a **fence**, and it has to be, because a stair proves
    /// nothing: `wire::connects` asks whether the side property is anything
    /// other than `none`, and a stair has no side properties at all, so the
    /// walk would fall out of its own gate one line later and the test would
    /// pass with `is_wire` deleted. A fence's `west=false` is *not* `none`, so
    /// it walks — which is a live hazard and not a contrived one, and the
    /// `is_wire` check is the only thing standing in front of it.
    ///
    /// The geometry is the UP-probe test's, so removing the gate produces the
    /// same visible climb that test asserts.
    #[test]
    fn a_block_that_is_not_a_wire_walks_no_diagonals() {
        let shapes = wire_table();
        let fence =
            "minecraft:oak_fence[east=false,north=false,south=false,waterlogged=false,west=false]";
        let before = dust("none", "none", "none", "none");
        let world = Fake::with(&[
            ((1, 0, 0), "minecraft:stone"),
            ((1, 1, 0), &before),
            ((2, -1, 0), "minecraft:stone"),
            ((2, 0, 0), fence),
            ((2, 1, 0), "minecraft:stone"),
            ((2, 2, 0), &dust("none", "none", "none", "none")),
        ]);
        let mut updates = Effects::new();
        update_indirect_neighbour_shapes(
            &world,
            &shapes,
            fence,
            (2, 0, 0),
            UPDATE_ALL,
            RECURSION_LIMIT - 1,
            &mut updates,
        );
        assert_eq!(
            world.state_at((1, 1, 0)),
            before,
            "a fence has no indirect walk, so the dust above the stone never hears anything"
        );
        assert!(updates.blocks.is_empty());
    }

    /// **The `from` position, offsets 118-128 and 191-201** — `m.relative(
    /// d.getOpposite())`, which works out to `pos.below()` for the DOWN probe.
    /// The walk tells the diagonal about the block *between* them, not about
    /// itself.
    ///
    /// This one needed building. Nothing in the wire rule reads the neighbour
    /// **state** on a horizontal update — `RedStoneWireBlock.updateShape` uses
    /// its `neighbourPos`/`neighbourState` arguments only in the `DOWN` branch,
    /// and the walk only ever notifies horizontally — so getting `from` wrong
    /// changes no wire answer anywhere, and the injected bug passed all twelve
    /// other tests. The one thing that does read it is the **extracted table**,
    /// which is keyed on the neighbour's state id, so the test hands the
    /// diagonal a row that fires for `minecraft:stone` and for nothing else. A
    /// wrong `from` names the diagonal itself, misses the row, and the dust
    /// survives.
    ///
    /// Worth knowing rather than hiding: on today's tree that argument is
    /// otherwise unobservable, which is exactly why it would have rotted.
    #[test]
    fn the_indirect_walk_names_the_block_below_as_the_one_that_changed() {
        let subject = dust("none", "none", "none", "side");
        let diagonal = dust("none", "none", "none", "none");
        let world = Fake::with(&[
            ((1, -1, 0), "minecraft:stone"),
            ((1, 0, 0), &diagonal),
            ((2, -1, 0), "minecraft:stone"),
            ((2, 0, 0), "minecraft:stone"),
            ((2, 1, 0), &subject),
        ]);
        let shapes = table(&[(&diagonal, Face::East, &[world.id("minecraft:stone")])]);
        let mut updates = Effects::new();
        update_indirect_neighbour_shapes(
            &world,
            &shapes,
            &subject,
            (2, 1, 0),
            UPDATE_ALL,
            RECURSION_LIMIT - 1,
            &mut updates,
        );
        assert_eq!(
            world.state_at((1, 0, 0)),
            "minecraft:air",
            "the row fires only for the stone at [2,0,0], which is pos.below() \
             and not the diagonal itself"
        );
    }

    /// **`UPDATE_SKIP_SHAPE_UPDATE_ON_WIRE`, offsets 9-29 of
    /// `executeShapeUpdate`** — the branch that was missing from the inlined
    /// copy in `set_block`'s loop for the whole life of that loop.
    ///
    /// Nothing sets flag 128 in this server yet, so this is the only thing that
    /// can hold the guard honest: the same call is made twice and only the
    /// flag differs. Without the guard both change the wire and the pair
    /// collapses into one assertion contradicting the other.
    #[test]
    fn flag_128_skips_a_shape_update_aimed_at_a_wire() {
        let shapes = wire_table();
        let before = dust("none", "none", "none", "none");
        let world = Fake::with(&[
            ((0, -1, 0), "minecraft:stone"),
            ((0, 0, 0), &before),
            ((1, 0, 0), "minecraft:stone"),
            ((1, 1, 0), &dust("none", "none", "none", "none")),
        ]);
        let mut updates = Effects::new();
        execute_shape_update(
            &world,
            &shapes,
            Face::East,
            (0, 0, 0),
            (1, 0, 0),
            "minecraft:stone",
            None,
            UPDATE_ALL | UPDATE_SKIP_SHAPE_UPDATE_ON_WIRE,
            RECURSION_LIMIT - 1,
            &mut updates,
        );
        assert_eq!(
            world.state_at((0, 0, 0)),
            before,
            "flag 128 returns before the block is asked anything"
        );
        execute_shape_update(
            &world,
            &shapes,
            Face::East,
            (0, 0, 0),
            (1, 0, 0),
            "minecraft:stone",
            None,
            UPDATE_ALL,
            RECURSION_LIMIT - 1,
            &mut updates,
        );
        assert_eq!(
            world.state_at((0, 0, 0)),
            dust("up", "none", "none", "side"),
            "without the flag the very same call changes it, so the guard is what did it"
        );
    }

    /// Drops interleave with level events by the count that preceded them.
    #[test]
    fn a_drop_goes_out_between_the_events_around_it() {
        let ev = |n: i32| (n, (0, 0, 0), 0);
        let dr = |n: usize| DropRequest {
            pos: (0, 0, 0),
            state: format!("s{n}"),
            after_events: n,
        };
        let mut out = Vec::new();
        let out_cell = std::cell::RefCell::new(&mut out);
        each_event_and_drop(
            &[ev(1), ev(2)],
            &[dr(0), dr(1), dr(1), dr(2)],
            |e, _, _| out_cell.borrow_mut().push(format!("e{e}")),
            |d| out_cell.borrow_mut().push(d.state.clone()),
        );
        assert_eq!(out, ["s0", "e1", "s1", "s1", "e2", "s2"]);
    }
}
