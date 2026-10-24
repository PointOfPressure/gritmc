//! `ContainerOpenersCounter` and the block-event queue it feeds.
//!
//! # Why an open chest is a repeating packet and not a single one
//!
//! This is the mechanism behind the only two entries
//! `ci/checks/gameplay-unstable.toml` carries for containers, and it is worth
//! reading before either check is believed in any direction.
//!
//! `ContainerOpenersCounter.CHECK_TICK_DELAY` is 5.
//! `incrementOpeners` schedules a recheck on the first opener;
//! `recheckOpeners` re-arms it at the end whenever the count is still above
//! zero; and — the part that surprises — it calls `openerCountChanged`
//! **unconditionally**, at offset 167, on the path where the count did not
//! change as well as the path where it did. `ChestBlockEntity`'s
//! `openerCountChanged` is `signalOpenCount`, which is
//! `level.blockEvent(pos, block, 1, openCount)`.
//!
//! So an open chest emits the same `block_event(1, 1)` every 250 ms, for as
//! long as it is open, forever. How many of those land inside a check's
//! `settle_ms` depends on where the window boundary falls relative to a
//! 5-tick cadence, which is why vanilla disagrees with *itself* on
//! `chest-opens-a-screen` 1 time in 25 and on `a-bystander-sees-a-chest-open`
//! 3 times in 25 — the two checks are the same mechanism sampled through
//! 1,000 ms and 2,000 ms windows, and the rates are in the ratio the window
//! lengths predict.
//!
//! **A run that reports one of those two as `diverge` has not necessarily
//! found a defect**, and a run that reports `match` has not proved the
//! cadence right. The cadence is what to check, from the recording, and not
//! the verdict.
//!
//! # Which containers have one of these
//!
//! `ChestBlockEntity` overrides `startOpen`/`stopOpen` and owns a counter.
//! `AbstractFurnaceBlockEntity` does **not** — it declares neither, so a
//! furnace has no openers count, no lid and no block event, and the check
//! that opens one should see none. The crafting table has no block entity at
//! all. That three-way split is exactly what checks 090, 470 and 480 exist to
//! separate, so getting it wrong in the tidy direction — one code path for
//! "container opened" — passes one of them and fails the other two.
//!
//! # What is not modelled here, named rather than hidden
//!
//! * **The close sound.** `ChestBlockEntity$1.onClose` plays
//!   `block.chest.close` when the last opener leaves. The *open* sound now
//!   exists — `crate::world_send::open_menu_for` plays it under the same
//!   `openCount++ == 0` guard vanilla puts `onOpen` behind — but nothing here
//!   observes a container being closed, so the close half is written down
//!   rather than guessed at. Its pitch is a fresh RNG draw like the open
//!   one's, which is why `tools/oracle-runner` stopped comparing the pitch
//!   field: the same event was measured at five different pitches in one
//!   corpus run.
//! * **`Level.gameEvent(CONTAINER_OPEN)`.** Vibration only; nothing listens
//!   in this world.
//! * **`shouldTickBlocksAt`.** Vanilla defers a block event in a chunk that
//!   is not ticking and retries it next tick. Everything this server serves
//!   ticks, so the branch has no false side to take here.

use crate::interact::BlockPos;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// `ContainerOpenersCounter.CHECK_TICK_DELAY`.
pub const CHECK_TICK_DELAY: i64 = 5;

/// `Attributes.BLOCK_INTERACTION_RANGE`'s default value, which is what
/// `Player.getContainerInteractionRange()` returns for a player carrying no
/// modifiers.
///
/// A `RangedAttribute` registered with default 4.5, min 0 and max 64. This
/// server has no attribute system, so every player has the default; the
/// constant is named after the attribute rather than after the number so that
/// the day one exists there is one place to look.
pub const BLOCK_INTERACTION_RANGE: f64 = 4.5;

/// The constant `getEntitiesWithContainerOpen` adds to `maxInteractionRange`
/// before inflating the search box.
const OPENER_SEARCH_PADDING: f64 = 4.0;

/// One queued `BlockEventData`.
///
/// `block` is the block **name**; it is resolved to a registry id only at
/// broadcast time, because `doBlockEvent` re-reads the world and drops the
/// event if the block has changed, so the name is what has to survive the
/// wait.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockEventData {
    pub pos: BlockPos,
    pub block: String,
    pub b0: i32,
    pub b1: i32,
}

/// `ServerLevel.blockEvents` — an `ObjectLinkedOpenHashSet`.
///
/// A **set**, and the set-ness is load-bearing: two identical events queued
/// in one tick collapse into one, because `add` on an equal element returns
/// false and does not move it. Insertion-ordered, which a plain `HashSet`
/// would not be, so this is a `Vec` with a membership test rather than a
/// `HashSet` — the queue is never longer than a handful of entries and the
/// order is what has to be right.
#[derive(Debug, Default)]
pub struct BlockEvents {
    queue: Mutex<Vec<BlockEventData>>,
}

impl BlockEvents {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The process-wide queue.
    #[must_use]
    pub fn shared() -> &'static Self {
        static Q: OnceLock<BlockEvents> = OnceLock::new();
        Q.get_or_init(BlockEvents::new)
    }

    /// `Level.blockEvent` on the server side: queue, do not send.
    pub fn queue(&self, event: BlockEventData) {
        let Ok(mut q) = self.queue.lock() else { return };
        if !q.contains(&event) {
            q.push(event);
        }
    }

    /// Take everything queued, in insertion order.
    pub fn take(&self) -> Vec<BlockEventData> {
        let Ok(mut q) = self.queue.lock() else {
            return Vec::new();
        };
        std::mem::take(&mut *q)
    }
}

/// One position's `ContainerOpenersCounter`.
///
/// `openers` is this server's stand-in for vanilla's
/// `ContainerUser.hasContainerOpen(counter, pos)` — the predicate half of
/// `getEntitiesWithContainerOpen`. Vanilla asks each nearby entity whether
/// its `containerMenu` is this container; a menu lives on a connection thread
/// here, so the answer is recorded when the menu opens instead of asked for
/// later. The set is of **entity ids**, so the recheck can look the opener's
/// current position up in the level entity list and apply the distance filter
/// against where it is *now* — which is the whole reason a chest whose opener
/// has walked to the next workspace shuts.
#[derive(Debug, Default, Clone)]
struct Counter {
    open_count: i32,
    max_interaction_range: f64,
    openers: std::collections::BTreeSet<i32>,
}

/// Every container position that has ever been opened.
///
/// **This used to say "because this server has no block entities yet", and
/// that stopped being true** — [`crate::blockentity`] exists. It is still
/// keyed by position, and the choice is now deliberate rather than forced:
/// vanilla hangs the counter off `ChestBlockEntity`, so folding it in would
/// be more faithful, and it would also rewrite the mechanism behind four
/// checks currently at `match` (090, 470, 480, 490) to buy nothing any check
/// can see.
///
/// The consequence is unchanged and is worth keeping in front of whoever
/// moves it: the lifetimes differ the first time a container is **broken
/// while open**. The block entity goes with the block and this does not, so
/// the position keeps an openers count for a chest that is no longer there.
/// Nothing in the corpus breaks an open container.
#[derive(Debug, Default)]
pub struct Openers {
    counters: Mutex<HashMap<BlockPos, Counter>>,
}

/// Whether an opener is inside the box `getEntitiesWithContainerOpen`
/// searches: `AABB(pos).inflate(maxInteractionRange + 4.0)`.
///
/// **Vanilla intersects the opener's own bounding box with that region and
/// this tests its position.** The two answers can differ only for an opener
/// standing within half its own width of the boundary — which is 8.5 blocks
/// from the container for a player with no range modifiers, and nothing in
/// the corpus opens a container and then stands anywhere near that far from
/// it. Named here rather than left as a silent simplification: closing this
/// needs entity bounding boxes, and it belongs with them.
#[must_use]
fn within_search_box(pos: BlockPos, max_range: f64, opener: [f64; 3]) -> bool {
    let pad = max_range + OPENER_SEARCH_PADDING;
    let (x, y, z) = (f64::from(pos.0), f64::from(pos.1), f64::from(pos.2));
    // `new AABB(BlockPos)` is the unit cube from the corner, so the upper
    // bound is one block further out than the lower one.
    let inside = |v: f64, lo: f64| v >= lo - pad && v <= lo + 1.0 + pad;
    inside(opener[0], x) && inside(opener[1], y) && inside(opener[2], z)
}

impl Openers {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The process-wide counters.
    #[must_use]
    pub fn shared() -> &'static Self {
        static O: OnceLock<Openers> = OnceLock::new();
        O.get_or_init(Openers::new)
    }

    /// `ContainerOpenersCounter.incrementOpeners`.
    ///
    /// Returns whether the caller should schedule a recheck 5 ticks out —
    /// true exactly when this was the first opener, which is when vanilla
    /// calls `scheduleRecheck`.
    ///
    /// The block event goes out on **every** open, not only the first: the
    /// `openerCountChanged` call at offset 42 sits after the `if`, not inside
    /// it.
    pub fn increment(
        &self,
        events: &BlockEvents,
        pos: BlockPos,
        block: &str,
        opener: i32,
        range: f64,
    ) -> bool {
        let Ok(mut counters) = self.counters.lock() else {
            return false;
        };
        let c = counters.entry(pos).or_default();
        let old = c.open_count;
        c.open_count += 1;
        let now = c.open_count;
        let first = old == 0;
        c.openers.insert(opener);
        c.max_interaction_range = c.max_interaction_range.max(range);
        drop(counters);
        signal_open_count(events, pos, block, now);
        first
    }

    /// `ContainerOpenersCounter.decrementOpeners`.
    pub fn decrement(&self, events: &BlockEvents, pos: BlockPos, block: &str, opener: i32) {
        let Ok(mut counters) = self.counters.lock() else {
            return;
        };
        let Some(c) = counters.get_mut(&pos) else {
            return;
        };
        c.open_count -= 1;
        c.openers.remove(&opener);
        let now = c.open_count;
        if now == 0 {
            c.max_interaction_range = 0.0;
        }
        drop(counters);
        signal_open_count(events, pos, block, now);
    }

    /// `ContainerOpenersCounter.recheckOpeners`, as reached from
    /// `ChestBlock.tick` -> `ChestBlockEntity.recheckOpen`.
    ///
    /// `openers` is every entity that currently has *this* container open,
    /// before the distance filter — the predicate half of
    /// `getEntitiesWithContainerOpen`. Returns whether to re-arm, which
    /// vanilla does whenever the recounted total is above zero.
    ///
    /// The count is recomputed from the world rather than trusted, which is
    /// what makes a chest whose opener has walked away eventually shut.
    pub fn recheck(
        &self,
        events: &BlockEvents,
        pos: BlockPos,
        block: &str,
        locate: &dyn Fn(i32) -> Option<[f64; 3]>,
    ) -> bool {
        let Ok(mut counters) = self.counters.lock() else {
            return false;
        };
        let Some(c) = counters.get_mut(&pos) else {
            // Nothing was ever opened here. Vanilla would have no scheduled
            // tick either, so this is a lost race rather than a state, and it
            // must not queue a `block_event(1, 0)` for a chest nobody opened.
            return false;
        };
        // `maxInteractionRange` is zeroed *before* the walk and rebuilt from
        // it, so the box shrinks back as openers leave rather than staying at
        // its high-water mark. The box used for *this* walk is still the old
        // value, which is why it is read before the reset.
        let previous = c.max_interaction_range;
        let mut max_range: f64 = 0.0;
        let mut count = 0;
        let mut still_open = std::collections::BTreeSet::new();
        for id in &c.openers {
            let Some(at) = locate(*id) else { continue };
            if within_search_box(pos, previous, at) {
                max_range = max_range.max(BLOCK_INTERACTION_RANGE);
                still_open.insert(*id);
                count += 1;
            }
        }
        c.max_interaction_range = max_range;
        c.open_count = count;
        // An opener out of range is not counted, and vanilla does not forget
        // it either — its `containerMenu` is still this container, so walking
        // back into range re-opens the lid without a second click. The set is
        // therefore *not* replaced by `still_open`; that variable exists to
        // make the distinction explicit rather than accidental.
        let _ = still_open;
        drop(counters);
        // Unconditional — this is the call that makes an open chest repeat.
        signal_open_count(events, pos, block, count);
        count > 0
    }

    /// How many openers a position currently has. Test and diagnostic use.
    #[must_use]
    pub fn count(&self, pos: BlockPos) -> i32 {
        self.counters
            .lock()
            .ok()
            .and_then(|c| c.get(&pos).map(|c| c.open_count))
            .unwrap_or(0)
    }
}

/// `ChestBlockEntity.signalOpenCount` — `blockEvent(pos, block, 1, count)`.
///
/// `b0` is the literal 1 and `b1` is the **new** count, read off `iload 5`,
/// the fifth parameter of `openerCountChanged`, not the old one at `iload 4`.
fn signal_open_count(events: &BlockEvents, pos: BlockPos, block: &str, count: i32) {
    events.queue(BlockEventData {
        pos,
        block: block.to_string(),
        b0: 1,
        b1: count,
    });
}

/// Whether a block's block entity owns a `ContainerOpenersCounter`.
///
/// **Deliberately a different question from [`trigger_event`], even though
/// the answer happens to be the same set today.** One asks whether opening
/// this container makes a lid move for everybody; the other asks whether a
/// queued block event survives `doBlockEvent`. A piston answers yes to the
/// second and no to the first, so folding them into one predicate would be
/// right by coincidence and wrong as soon as pistons land.
///
/// `ChestBlockEntity` overrides `startOpen`/`stopOpen` and holds a counter.
/// `AbstractFurnaceBlockEntity` declares neither — a furnace has no lid.
/// `CraftingTableBlock` has no block entity at all.
///
/// # Three classes hold a `ContainerOpenersCounter` and only two belong here
///
/// Counted off the jar rather than recalled: `ChestBlockEntity`,
/// `BarrelBlockEntity` and `EnderChestBlockEntity` are the only three that
/// declare one, and **the barrel's does something else entirely**. Its
/// anonymous subclass calls `BarrelBlockEntity.updateBlockState(state,
/// boolean)` — a `setBlock` writing the `open` property — where the chest's
/// and the ender chest's call `signalOpenCount`/`Level.blockEvent`. So a
/// barrel's opening is a block *state* change and produces no `block_event`
/// at all, and putting it in this predicate would queue an event vanilla
/// never sends the day a barrel menu lands.
///
/// # Keyed on the block entity, so the chest family comes with it
///
/// `TrappedChestBlockEntity extends ChestBlockEntity` and the eight copper
/// chests are `ChestBlockEntity` outright, so all ten signal their lid on
/// vanilla; keying this on the literal `"minecraft:chest"` said no for nine
/// of them. **It is inert today and named rather than left to be found**:
/// [`crate::menu::menu_for_state`] answers for `minecraft:chest` alone, so
/// nothing else can be opened here to reach this. That is the join point with
/// the container-menu work — when `menu_for_state` grows the other nine, this
/// predicate is already right for them.
#[must_use]
pub fn has_openers_counter(block: &str) -> bool {
    use crate::blockentity::Kind;
    matches!(
        crate::blockentity::for_state(block),
        Some(Kind::Chest | Kind::TrappedChest | Kind::EnderChest)
    )
}

/// `BlockState.triggerEvent` — whether a queued event is delivered at all.
///
/// `doBlockEvent` returns this, and `runBlockEvents` sends the packet only
/// when it is true. The base `Block.triggerEvent` is `return false`, so a
/// block that does not handle its own event produces **no packet**; only
/// `BaseEntityBlock.triggerEvent`'s hand-off to a block entity can make one.
/// Defaulting the other way would invent packets for every block this server
/// has not modelled.
///
/// # `ChestBlockEntity` does **not** accept unconditionally — corrected 2026-09-05
///
/// This paragraph said `if (b0 == 1) { lid.shouldBeOpen(b1 > 0); } return
/// true`, and that it "accepts unconditionally and only *acts* on `b0 == 1`".
/// The `if_icmpne` at offset 2 jumps to offset 23, which is the `super` call:
///
/// ```text
///    0: iload_1
///    1: iconst_1
///    2: if_icmpne     23          <- b0 != 1 leaves here
///    5-18: chestLidController.shouldBeOpen(b1 > 0)
///   21: iconst_1
///   22: ireturn                   <- only b0 == 1 returns true
///   23-26: super.triggerEvent(b0, b1)
///   29: ireturn
/// ```
///
/// and `RandomizableContainerBlockEntity` does not override it, so the super
/// chain reaches `BlockEntity.triggerEvent`, whose whole body is `iconst_0;
/// ireturn`. A chest returns **false** for every `b0` but 1, and the version
/// of this function that ignored `b0` sent a packet vanilla suppresses.
///
/// # The seven types that override it, and the one rule they share
///
/// `BellBlockEntity`, `ChestBlockEntity`, `DecoratedPotBlockEntity`,
/// `EnderChestBlockEntity`, `ShulkerBoxBlockEntity`, `SpawnerBlockEntity` and
/// `TheEndGatewayBlockEntity` — and no others; every remaining type inherits
/// the base `return false`. All seven gate on `b0 == 1` and only the pot looks
/// at `b1`: `level != null && b0 == 1 && b1 >= 0 && b1 < WobbleStyle.values()
/// .length`, and that enum has two constants.
///
/// `SpawnerBlockEntity`'s route is `BaseSpawner.onEventTriggered(level, b0)`,
/// which returns `true` on `b0 == 1` and uses `isClientSide` only to decide
/// whether to reset `spawnDelay` — so the *return* is side-independent even
/// though the effect is not.
///
/// # Dispatched on the block ENTITY, not on the block name
///
/// `triggerEvent` is a method on the block entity, so the key is
/// [`crate::blockentity::for_state`]'s answer. Keying on the block name is
/// how the previous version came to answer only for `minecraft:chest`: a
/// trapped chest, a copper chest and the seventeen shulker boxes are all
/// `ChestBlockEntity`/`ShulkerBoxBlockEntity` and every one of them signalled
/// its lid on vanilla and not here.
///
/// # This function cannot answer for a piston, and that is not an oversight
///
/// `PistonBaseBlock.triggerEvent` re-reads the world — it calls
/// `getNeighborSignal` and then `moveBlocks`, and returns `false` in three
/// world-dependent cases. Its answer is a function of the world at the moment
/// the queue drains, not of `(block, b0, b1)`, so it is **not representable
/// here** and is reached through [`crate::piston::trigger_event`] instead.
/// [`run_block_events`] routes pistons there before it consults this.
///
/// Widening this `matches!` to include `minecraft:piston` would have made the
/// discarded-event case go away while sending a packet in the two cases
/// vanilla suppresses — a green that could not be turned red by any test that
/// only ever powers the piston.
#[must_use]
pub fn trigger_event(block: &str, b0: i32, b1: i32) -> bool {
    use crate::blockentity::Kind;
    match crate::blockentity::for_state(block) {
        Some(
            Kind::Chest
            | Kind::TrappedChest
            | Kind::EnderChest
            | Kind::ShulkerBox
            | Kind::Bell
            | Kind::MobSpawner
            | Kind::EndGateway,
        ) => b0 == 1,
        // `WobbleStyle` has two constants, POSITIVE and NEGATIVE, and the
        // index is used as an array subscript — the bounds test is the guard
        // on that `aaload` and not decoration.
        Some(Kind::DecoratedPot) => b0 == 1 && (0..WOBBLE_STYLES).contains(&b1),
        // Every other block entity, and every block without one, inherits
        // `BlockEntity.triggerEvent`'s `iconst_0; ireturn`. Defaulting the
        // other way would invent a packet for every block this server has not
        // modelled.
        _ => false,
    }
}

/// `DecoratedPotBlockEntity.WobbleStyle.values().length` — POSITIVE and
/// NEGATIVE.
const WOBBLE_STYLES: i32 = 2;

/// Run a piston's `triggerEvent` and apply its writes.
///
/// Returns `triggerEvent`'s own return value — whether the `block_event`
/// packet is sent.
///
/// Only the writes carrying `Block.UPDATE_CLIENTS` are recorded for
/// broadcast. Every `moving_piston` placement is deliberately not, which is
/// what makes this server's wire traffic match vanilla's rather than
/// exceeding it; see [`crate::piston`]'s header.
fn run_piston_event(
    blocks: &dyn crate::edits::Blocks,
    pos: BlockPos,
    state: &str,
    b0: i32,
    b1: i32,
) -> (bool, Option<crate::sounds::Sound>) {
    let Some(signals) = crate::power::shared() else {
        // No signal table means `getNeighborSignal` cannot be answered, and a
        // piston whose power is unknown must not be guessed at in either
        // direction. Refusing loses the event; inventing one would put a
        // packet on the wire off a reading nobody took. The sound goes with
        // it: a click with no movement is worse than silence.
        return (false, None);
    };
    let redstone = crate::power::Redstone::new(blocks, signals, None);
    let world = crate::piston::LevelPiston::new(
        blocks,
        &redstone,
        crate::piston::shared_push_table(),
        crate::piston::WORLD_MIN_Y,
        crate::piston::WORLD_MAX_Y,
    );
    let outcome = crate::piston::trigger_event(&world, state, pos, b0, b1);
    for w in &outcome.writes {
        if !blocks.has_state(&w.state) {
            crate::piston::unknown_state_once(&w.state);
            continue;
        }
        if blocks.set_block(w.pos, &w.state).is_some() && w.reaches_clients() {
            crate::broadcast::ChangedBlocks::shared().record(w.pos);
        }
    }
    (outcome.deliver, outcome.sound)
}

/// `ServerLevel.runBlockEvents` — drain the queue and broadcast what survives.
///
/// Two filters, in vanilla's order, and both of them fail closed:
///
/// * `doBlockEvent` re-reads the world and drops the event unless the block
///   is still the one that queued it, then asks
///   [`trigger_event`] whether the block handles it at all;
/// * `PlayerList.broadcast` sends to every player in the dimension within
///   **64.0 blocks** of the event's position, measured from the block's
///   integer corner rather than its centre — and with `exclude` **null**, so
///   the player who opened the chest gets the lid packet too. That is not a
///   detail: `090-chest-opens-a-screen` reads `block_event` off the *actor's*
///   stream, so a broadcast that skipped the actor would make that check look
///   like a server that never signalled at all.
///
/// It is *not* chunk tracking and *not* view distance. A player 60 blocks
/// away with the chunk unloaded still gets the packet.
pub fn run_block_events(
    events: &BlockEvents,
    players: &crate::players::Players,
    blocks: &dyn crate::edits::Blocks,
    ids: Option<&crate::protocol_ids::ProtocolIds>,
    locate: &dyn Fn(i32) -> Option<[f64; 3]>,
) {
    for event in events.take() {
        let state = blocks.state_at(event.pos);
        if state.split('[').next().unwrap_or(&state) != event.block {
            continue;
        }
        // `BlockState.triggerEvent`. A piston's answer depends on the world
        // and on `moveBlocks` succeeding, so it is dispatched rather than
        // looked up; every other block still answers from the constant
        // predicate.
        let (delivered, sound) = if crate::piston::is_piston(&event.block) {
            run_piston_event(blocks, event.pos, &state, event.b0, event.b1)
        } else if crate::noteblock::is_note_block(&event.block) {
            // `NoteBlock.triggerEvent` returns true and plays a sound whose
            // event and pitch come from the state, so it cannot be answered
            // by the constant predicate below. It returns FALSE — no packet,
            // no sound — for a `custom_head` whose sound id will not resolve,
            // which is the one arm `crate::noteblock` refuses.
            crate::noteblock::Instruments::shared(std::path::Path::new(
                crate::noteblock::TABLE_PATH,
            ))
            .ok()
            .and_then(|t| crate::noteblock::trigger_event(t, &state, event.pos))
            .map_or((false, None), |s| (true, Some(s)))
        } else {
            (trigger_event(&event.block, event.b0, event.b1), None)
        };
        // THE SOUND GOES OUT FIRST AND WHETHER OR NOT THE EVENT IS
        // DELIVERED, because that is where vanilla's is. Both of
        // `PistonBaseBlock.triggerEvent`'s `playSound` calls are inside it,
        // reaching `PlayerList.broadcast` with no queue on the path, while
        // `runBlockEvents` broadcasts the `block_event` only after
        // `doBlockEvent` returns true. A piston whose event is dropped
        // between those two points still made a noise. (It cannot be dropped
        // here today — the only thing between them is the id lookup — but
        // ordering the two by accident is how the next thing added in
        // between silently changes it.)
        if let Some(sound) = sound {
            crate::sounds::send(
                players,
                ids,
                crate::sound_events::SoundEventTable::shared(std::path::Path::new(
                    crate::sound_events::TABLE_PATH,
                ))
                .ok(),
                Some(crate::entities::Entities::shared()),
                &[sound],
            );
        }
        if !delivered {
            continue;
        }
        let Some(block_id) = ids.and_then(|t| t.block(&event.block)) else {
            continue;
        };
        let Ok(body) = block_event_body(event.pos, event.b0, event.b1, block_id) else {
            continue;
        };
        // ONE IMPLEMENTATION OF `PlayerList.broadcast`, NOT TWO. This loop
        // used to be written out here with its own squared-distance test, and
        // it differed from `crate::playerlist`'s in two ways that nothing
        // would have surfaced: it used a fused `mul_add` where vanilla rounds
        // twice, and it **skipped** a listener it could not locate where the
        // sound path sends to them. The second is a policy about the join
        // window and it should be one decision, not two; see
        // `crate::playerlist`'s header for which way it goes and why.
        crate::playerlist::broadcast_within(
            &players.connections(),
            locate,
            None,
            (
                f64::from(event.pos.0),
                f64::from(event.pos.1),
                f64::from(event.pos.2),
            ),
            BROADCAST_RADIUS,
            rustmc_proto::ids::play::cb::BLOCK_EVENT,
            &body,
        );
    }
}

/// The radius `ServerLevel.runBlockEvents` passes to `PlayerList.broadcast`,
/// `ldc2_w double 64.0d` at its offset 81.
///
/// It is its own constant and not `crate::playerlist::LEVEL_EVENT_RANGE`,
/// which is also 64.0 and comes from `ServerLevel.levelEvent` offset 41. Two
/// unrelated call sites that happen to agree; sharing one constant would be a
/// claim that they have to.
const BROADCAST_RADIUS: f64 = 64.0;

/// `ClientboundBlockEventPacket` — packed position, two raw bytes, then a
/// varint **block** registry id.
///
/// The constructor takes `(pos, block, b0, b1)` and the wire order is
/// `pos, b0, b1, block`. They disagree, and writing the fields in
/// constructor order produces a packet that decodes into nonsense.
///
/// # Errors
/// Only on a write to the buffer, which cannot fail for a `Vec`.
pub fn block_event_body(
    pos: BlockPos,
    b0: i32,
    b1: i32,
    block_id: i32,
) -> std::io::Result<Vec<u8>> {
    let mut body = Vec::new();
    rustmc_proto::wire::write_i64(&mut body, crate::interact::pack_pos(pos))?;
    rustmc_proto::wire::write_u8(&mut body, u8::try_from(b0 & 0xff).unwrap_or(0))?;
    rustmc_proto::wire::write_u8(&mut body, u8::try_from(b1 & 0xff).unwrap_or(0))?;
    rustmc_proto::varint::write_varint(&mut body, block_id)?;
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACTOR: i32 = 7;

    #[test]
    fn the_first_open_signals_one_and_asks_for_a_recheck() {
        let events = BlockEvents::new();
        let openers = Openers::new();
        let pos = (1, 0, 0);
        assert!(
            openers.increment(
                &events,
                pos,
                "minecraft:chest",
                ACTOR,
                BLOCK_INTERACTION_RANGE
            ),
            "the first opener schedules the recheck"
        );
        let q = events.take();
        assert_eq!(q.len(), 1);
        assert_eq!((q[0].b0, q[0].b1), (1, 1), "b0 is 1, b1 is the new count");
    }

    /// The second opener still signals, and does **not** re-schedule.
    #[test]
    fn a_second_opener_signals_two_and_does_not_reschedule() {
        let events = BlockEvents::new();
        let openers = Openers::new();
        let pos = (1, 0, 0);
        openers.increment(
            &events,
            pos,
            "minecraft:chest",
            ACTOR,
            BLOCK_INTERACTION_RANGE,
        );
        let again = openers.increment(
            &events,
            pos,
            "minecraft:chest",
            ACTOR + 1,
            BLOCK_INTERACTION_RANGE,
        );
        assert!(!again, "only the first opener calls scheduleRecheck");
        let q = events.take();
        assert_eq!(q.len(), 2);
        assert_eq!(q[1].b1, 2, "the new count, not the old one");
    }

    /// The repeat that makes the two chest checks flap in vanilla.
    #[test]
    fn a_recheck_with_the_opener_still_there_signals_again_and_re_arms() {
        let events = BlockEvents::new();
        let openers = Openers::new();
        let pos = (1, 0, 0);
        openers.increment(
            &events,
            pos,
            "minecraft:chest",
            ACTOR,
            BLOCK_INTERACTION_RANGE,
        );
        events.take();
        let here = |_id: i32| Some([1.5, 0.0, 0.5]);
        // **Drained between rechecks, because that is what a tick does**, and
        // this test asserted three events out of one undrained queue on its
        // first run and got one. That is the set behaving correctly: three
        // identical `block_event(1, 1)` at one position collapse to a single
        // entry, exactly as `ObjectLinkedOpenHashSet.add` does. Vanilla never
        // sees the collapse for this mechanism because the rechecks are five
        // ticks apart and `runBlockEvents` empties the queue in between — so
        // the dedup is real, is per-tick, and only bites something that
        // signals twice *within one tick*.
        let mut sent = Vec::new();
        for _ in 0..3 {
            assert!(
                openers.recheck(&events, pos, "minecraft:chest", &here),
                "a still-open chest re-arms"
            );
            sent.extend(events.take());
        }
        assert_eq!(
            sent.len(),
            3,
            "one block event per recheck, unconditionally"
        );
        assert!(sent.iter().all(|e| (e.b0, e.b1) == (1, 1)));
    }

    /// **The cadence, measured as the GAP and not the count — and this test
    /// is the replacement coverage for what the two chest checks stopped
    /// testing when they declared `collapse_repeats = ["block_event"]`.**
    ///
    /// The two do not ship separately, and the reason is the rule this
    /// project keeps re-learning: an exemption that removes a signal without
    /// something else claiming the mechanism manufactures a check that cannot
    /// fail. `090-chest-opens-a-screen` and
    /// `490-a-bystander-sees-a-chest-open` still assert the lid's position,
    /// `b0`, `b1` and block, and they no longer assert that it repeats. A
    /// server that signalled once and never re-armed would pass both. This is
    /// what says it must not.
    ///
    /// It is the *gap* rather than the count because the count is exactly the
    /// quantity the wire cannot pin: it is decided by where a check's window
    /// edge falls against a 5-tick cadence. The gap is a property of the
    /// mechanism alone, and measuring it means driving the real tick queue —
    /// `ContainerOpenersCounter.scheduleRecheck` is
    /// `level.scheduleTick(pos, block, 5)` into the **same** `LevelTicks`
    /// every other block tick goes into, so the loop below is
    /// `crate::status`'s tick body with the world reads dropped.
    ///
    /// **The wire cadence itself remains unmeasured against vanilla**, and
    /// that is a real gap rather than a hidden one. What is pinned here is
    /// that this server re-arms at `CHECK_TICK_DELAY` and stops when the
    /// count reaches zero; that vanilla's own wire gaps are 195-265 ms
    /// against our exact 250 is recorded on the check files and in
    /// `docs/WORKLIST.md`, and closing it needs a timing comparison the
    /// corpus deliberately does not do.
    #[test]
    fn the_wire_cadence_is_one_signal_every_check_tick_delay_ticks() {
        let events = BlockEvents::new();
        let openers = Openers::new();
        let ticks = crate::ticks::ScheduledTicks::new();
        let pos = (1, 0, 0);
        const CHEST: &str = "minecraft:chest";

        // The open, at tick 0. `increment` returning true is
        // `scheduleRecheck` firing, and only the first opener gets it.
        assert!(openers.increment(&events, pos, CHEST, ACTOR, BLOCK_INTERACTION_RANGE));
        assert!(crate::ticks::schedule_container_recheck(
            &ticks, pos, CHEST, 0
        ));
        // Everything the open itself queued is drained here, so what the loop
        // collects is repeats and nothing else.
        assert_eq!(events.take().len(), 1, "the open signals once");

        // A second schedule while one is pending is refused — `UNIQUE_TICK_HASH`
        // — and that refusal is *why* the gap is exactly 5 rather than
        // compounding every time something asks.
        assert!(
            !crate::ticks::schedule_container_recheck(&ticks, pos, CHEST, 0),
            "a pending recheck is not queued twice"
        );

        // `crate::status`'s tick body: drain what is due, recheck, re-arm.
        let here = |_id: i32| Some([1.5, 0.0, 0.5]);
        let mut signalled: Vec<i64> = Vec::new();
        for now in 1..=40 {
            for tick in ticks.drain_due(now) {
                if has_openers_counter(&tick.block)
                    && openers.recheck(&events, tick.pos, &tick.block, &here)
                {
                    crate::ticks::schedule_container_recheck(&ticks, tick.pos, &tick.block, now);
                }
            }
            for _ in events.take() {
                signalled.push(now);
            }
        }

        // The claim, and it is about the gaps.
        let gaps: Vec<i64> = signalled.windows(2).map(|w| w[1] - w[0]).collect();
        assert!(
            !gaps.is_empty(),
            "an open chest repeats; it signalled at {signalled:?}"
        );
        assert!(
            gaps.iter().all(|g| *g == CHECK_TICK_DELAY),
            "every gap is CHECK_TICK_DELAY, not just the mean: {gaps:?}"
        );
        // And the phase: the first repeat is one delay after the open, not on
        // the next tick and not at some offset that happens to divide by 5.
        assert_eq!(signalled.first(), Some(&CHECK_TICK_DELAY));
    }

    /// The other half of the cadence: it **stops**. A timer that re-armed for
    /// ever would satisfy the gap test above and would still be wrong, so the
    /// two assertions belong to one claim.
    #[test]
    fn the_cadence_stops_the_tick_the_opener_is_out_of_range() {
        let events = BlockEvents::new();
        let openers = Openers::new();
        let ticks = crate::ticks::ScheduledTicks::new();
        let pos = (1, 0, 0);
        const CHEST: &str = "minecraft:chest";

        assert!(openers.increment(&events, pos, CHEST, ACTOR, BLOCK_INTERACTION_RANGE));
        assert!(crate::ticks::schedule_container_recheck(
            &ticks, pos, CHEST, 0
        ));
        events.take();

        // The opener walks off after tick 12, which is between the second and
        // the third recheck.
        let mut signalled: Vec<(i64, i32)> = Vec::new();
        for now in 1..=40 {
            let locate = |_id: i32| {
                Some(if now > 12 {
                    [33.5, 0.0, 0.5]
                } else {
                    [1.5, 0.0, 0.5]
                })
            };
            for tick in ticks.drain_due(now) {
                if has_openers_counter(&tick.block)
                    && openers.recheck(&events, tick.pos, &tick.block, &locate)
                {
                    crate::ticks::schedule_container_recheck(&ticks, tick.pos, &tick.block, now);
                }
            }
            for e in events.take() {
                signalled.push((now, e.b1));
            }
        }

        assert_eq!(
            signalled,
            vec![(5, 1), (10, 1), (15, 0)],
            "two repeats with the opener there, then the lid shuts once and \
             nothing is re-armed"
        );
        assert_eq!(ticks.len(), 0, "nothing left pending");
    }

    /// And it stops when the opener has walked away, which is the other half
    /// — and is what ends the repeat when the corpus moves to the next
    /// workspace 32 blocks along.
    #[test]
    fn a_recheck_with_the_opener_out_of_range_signals_zero_and_stops() {
        let events = BlockEvents::new();
        let openers = Openers::new();
        let pos = (1, 0, 0);
        openers.increment(
            &events,
            pos,
            "minecraft:chest",
            ACTOR,
            BLOCK_INTERACTION_RANGE,
        );
        events.take();
        let far = |_id: i32| Some([33.5, 0.0, 0.5]);
        assert!(!openers.recheck(&events, pos, "minecraft:chest", &far));
        let q = events.take();
        assert_eq!(q.len(), 1);
        assert_eq!(q[0].b1, 0, "the lid shuts");
        assert_eq!(openers.count(pos), 0);
    }

    /// A recheck at a position nobody ever opened produces nothing at all.
    /// Without this it would queue `block_event(1, 0)` for every chest in the
    /// world on the first tick after a restart.
    #[test]
    fn a_recheck_on_an_unopened_container_says_nothing() {
        let events = BlockEvents::new();
        let openers = Openers::new();
        let here = |_id: i32| Some([1.5, 0.0, 0.5]);
        assert!(!openers.recheck(&events, (4, 0, 0), "minecraft:chest", &here));
        assert!(events.take().is_empty());
    }

    /// The two predicates ask different questions and must not be merged.
    #[test]
    fn only_the_chest_has_a_lid_and_a_furnace_has_neither() {
        assert!(has_openers_counter("minecraft:chest"));
        assert!(!has_openers_counter("minecraft:furnace"));
        assert!(!has_openers_counter("minecraft:crafting_table"));
        // The whole `ChestBlockEntity` family, because a trapped chest
        // extends it and the eight copper chests *are* it. Keyed on the block
        // name this said no for nine of the ten.
        assert!(has_openers_counter("minecraft:trapped_chest"));
        assert!(has_openers_counter("minecraft:waxed_exposed_copper_chest"));
        assert!(has_openers_counter("minecraft:ender_chest"));
        // **And not the barrel**, which is the one that would look right.
        // `BarrelBlockEntity` holds a `ContainerOpenersCounter` like the other
        // two and its subclass calls `updateBlockState` — a `setBlock` on the
        // `open` property — where the chest's calls `signalOpenCount` and
        // `Level.blockEvent`. Opening a barrel writes a block state and sends
        // no block event at all.
        assert!(!has_openers_counter("minecraft:barrel"));
        assert!(!has_openers_counter("minecraft:red_shulker_box"));
    }

    /// `triggerEvent` gates on `b0 == 1` for all seven types that override it,
    /// and the previous version of this predicate ignored `b0` entirely.
    ///
    /// `ChestBlockEntity.triggerEvent`'s `if_icmpne` at offset 2 jumps to the
    /// `super` call at 23, and `RandomizableContainerBlockEntity` does not
    /// override the method, so a chest asked about any `b0` but 1 reaches
    /// `BlockEntity.triggerEvent`'s `iconst_0; ireturn`. A packet was being
    /// sent for events vanilla discards — invisible, because the only `b0` in
    /// this server's own queue is 1.
    #[test]
    fn trigger_event_gates_on_b0_and_the_pot_bounds_its_b1() {
        assert!(trigger_event("minecraft:chest", 1, 1));
        assert!(!trigger_event("minecraft:chest", 2, 1));
        assert!(!trigger_event("minecraft:chest", 0, 1));
        // The rest of the family and the other five overriders.
        for block in [
            "minecraft:trapped_chest",
            "minecraft:oxidized_copper_chest",
            "minecraft:ender_chest",
            "minecraft:lime_shulker_box",
            "minecraft:bell",
            "minecraft:spawner",
            "minecraft:end_gateway",
        ] {
            assert!(trigger_event(block, 1, 0), "{block} accepts b0 == 1");
            assert!(!trigger_event(block, 3, 0), "{block} refuses b0 == 3");
        }
        // `DecoratedPotBlockEntity` is the only one that reads `b1`: it is an
        // index into `WobbleStyle.values()`, which has two constants, and the
        // bounds test is the guard on that `aaload`.
        assert!(trigger_event("minecraft:decorated_pot", 1, 0));
        assert!(trigger_event("minecraft:decorated_pot", 1, 1));
        assert!(!trigger_event("minecraft:decorated_pot", 1, 2));
        assert!(!trigger_event("minecraft:decorated_pot", 1, -1));
        // Everything that does not override it inherits `return false`,
        // including block entities that exist and blocks that have none.
        assert!(!trigger_event("minecraft:furnace", 1, 1));
        assert!(!trigger_event("minecraft:barrel", 1, 1));
        assert!(!trigger_event("minecraft:stone", 1, 1));
    }

    /// 64 blocks, exclusive, from the block's integer corner.
    ///
    /// This used to call a `within_broadcast_radius` local to this module,
    /// which was a second implementation of `PlayerList.broadcast`'s distance
    /// test and disagreed with `crate::playerlist`'s on the fused
    /// multiply-add. The wrapper is gone rather than kept as a name, because a
    /// wrapper whose only caller is its own test is what "unused" means; the
    /// test survives, pointed at the one implementation, so `BROADCAST_RADIUS`
    /// is still exercised at its boundary.
    #[test]
    fn the_broadcast_radius_is_sixty_four_and_exclusive() {
        let at = |x: f64, z: f64| {
            crate::playerlist::within((0.0, 0.0, 0.0), Some((x, 0.0, z)), BROADCAST_RADIUS)
        };
        assert!(at(63.9, 0.0));
        assert!(!at(64.0, 0.0));
        // Euclidean, not per-axis: 40 on two axes is 56.6 and inside.
        assert!(at(40.0, 40.0));
        assert!(!at(50.0, 50.0));
    }

    /// The wire order is pos, b0, b1, block — not the constructor's order.
    #[test]
    fn the_block_event_body_puts_the_block_last() {
        let b = block_event_body((1, 0, 0), 1, 1, 201).expect("buf");
        assert_eq!(b.len(), 8 + 1 + 1 + 2, "long, byte, byte, two-byte varint");
        assert_eq!((b[8], b[9]), (1, 1), "b0 then b1");
        assert_eq!(&b[10..], &[0xC9, 0x01], "varint 201, last");
    }

    /// Two identical events in one tick are one event: the queue is a set.
    #[test]
    fn the_queue_dedups_and_keeps_insertion_order() {
        let events = BlockEvents::new();
        let a = BlockEventData {
            pos: (1, 0, 0),
            block: "minecraft:chest".into(),
            b0: 1,
            b1: 1,
        };
        let b = BlockEventData { b1: 2, ..a.clone() };
        events.queue(a.clone());
        events.queue(b.clone());
        events.queue(a.clone());
        let q = events.take();
        assert_eq!(q, vec![a, b], "deduped, and the first insertion wins");
    }

    /// The box is **not symmetric about the block**, which this test asserted
    /// wrongly on its first run: it expected 8.6 to be outside and it is not.
    /// `new AABB(BlockPos)` is the unit cube from the block's corner, so
    /// inflating by 8.5 gives −8.5 on the low side and **+9.5** on the high
    /// side. An opener standing at x = 9 is inside the box of the block at
    /// x = 0 and outside the box of the block at x = 18.
    #[test]
    fn the_search_box_is_the_interaction_range_plus_four() {
        let pos = (0, 0, 0);
        // The low side: the corner minus the padding.
        assert!(within_search_box(
            pos,
            BLOCK_INTERACTION_RANGE,
            [-8.5, 0.0, 0.0]
        ));
        assert!(!within_search_box(
            pos,
            BLOCK_INTERACTION_RANGE,
            [-8.6, 0.0, 0.0]
        ));
        // The high side is a whole block further out.
        assert!(within_search_box(
            pos,
            BLOCK_INTERACTION_RANGE,
            [9.5, 0.0, 0.0]
        ));
        assert!(!within_search_box(
            pos,
            BLOCK_INTERACTION_RANGE,
            [9.6, 0.0, 0.0]
        ));
    }

    /// A block with no block entity drops its own event rather than sending
    /// a packet for it.
    #[test]
    fn only_a_block_that_handles_its_event_produces_a_packet() {
        assert!(trigger_event("minecraft:chest", 1, 1));
        assert!(!trigger_event("minecraft:furnace", 1, 1));
        assert!(!trigger_event("minecraft:stone", 1, 1));
    }

    /// **The constant predicate must never learn about pistons.**
    ///
    /// The obvious fix for the discarded piston event was to widen the
    /// `matches!` above, and it is wrong: `PistonBaseBlock.triggerEvent`
    /// returns `false` when the piston has regained power and the event is a
    /// contract, and again when it has lost power and the event is an extend.
    /// A constant `true` here sends the packet in both, and no test that only
    /// ever powers a piston and waits could tell.
    ///
    /// So this asserts the *absence* of a piston arm, and it goes red the
    /// moment somebody adds one. The piston is reached through
    /// [`run_piston_event`] instead, which has a world to read.
    #[test]
    fn the_constant_predicate_does_not_answer_for_a_piston() {
        for b in ["minecraft:piston", "minecraft:sticky_piston"] {
            assert!(
                crate::piston::is_piston(b),
                "{b} is not recognised as a piston, so run_block_events will send it to the \
                 constant predicate"
            );
            for b0 in [0, 1, 2] {
                assert!(
                    !trigger_event(b, b0, 2),
                    "{b} was answered by the world-free predicate at b0={b0}; that predicate \
                     cannot see the redstone signal triggerEvent re-reads"
                );
            }
        }
    }
}
