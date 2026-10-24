//! Scheduled block ticks: the first thing other than the clock that ticks.
//!
//! Reference (26.2), read from the jar: `ScheduledTick` and its
//! `DRAIN_ORDER`, `TickPriority`, `LevelChunkTicks.schedule`,
//! `Level.scheduleTick`, and `CoralBlock` — `getStateForPlacement`, `tick`
//! and `scanForWater`.
//!
//! # Why coral is the first one and not an arbitrary choice
//!
//! `Block.tick` takes a **`ServerLevel`**, which is a class rather than an
//! interface, so the trick that made `getStateForPlacement` and `updateShape`
//! extractable — a `java.lang.reflect.Proxy` over a level that answers two
//! positions and throws for a third — does not work on it. There is no cheap
//! oracle for "what does this block do when its tick fires", and inventing
//! one per block is the thing this project refuses to do.
//!
//! What there *is* is a list, already measured: **five blocks schedule a tick
//! from inside `getStateForPlacement`**, and `data/vanilla/placement.json`
//! carries it as `schedulesTickBlocks`. All five are `CoralBlock`, and
//! `CoralBlock` is small enough to read whole:
//!
//! ```text
//!   getStateForPlacement: if (!scanForWater(level, clickedPos))
//!                             level.scheduleTick(pos, this, 60 + random.nextInt(40));
//!                         return defaultBlockState();
//!   tick:                 if (!scanForWater(level, pos))
//!                             level.setBlock(pos, deadBlock.defaultBlockState(), 2);
//!   scanForWater:         any of Direction.values() whose getFluidState is in #water
//! ```
//!
//! So this is the *machinery* — a `LevelTicks` with vanilla's ordering, run
//! off the 20 Hz clock — plus the behaviours whose whole rule fits in a few
//! lines of bytecode. Everything else that ticks is blocked on an oracle, and
//! that is written down rather than worked around.
//!
//! # Two behaviours now, and the second one changed where the delay comes from
//!
//! `ButtonBlock` is the other. Its rule is as short as the coral's, but its
//! *delay* is not derivable at all: `ticksToStayPressed` is a constructor
//! argument, 20 for stone and polished blackstone and 30 for the eleven
//! wooden ones, with no rule connecting it to a block's name. So the delay is
//! **data** — `use_without_item.json` records what `Level.scheduleTick` was
//! called with, and [`on_used`] passes it through. A tick queue driven by a
//! constant would be right about one button in three.
//!
//! Which blocks a rule applies to is read from **vanilla's own class name**,
//! which `set_placed_by.json` carries for the whole registry. Never from the
//! block's id: "ends in `_button`" is a naming rule that holds until one
//! block breaks it, and the break is silent.
//!
//! # The ordering is three keys and the third one is not decorative
//!
//! `ScheduledTick.DRAIN_ORDER` is `triggerTick`, then `priority`, then
//! `subTickOrder` — the last being a monotonic counter stamped at schedule
//! time, which is what makes two ticks scheduled for the same tick at the
//! same priority run in the order they were *asked for* rather than in
//! whatever order a map iterates. Priority is the enum's own ordinal, so
//! `EXTREMELY_HIGH` (0) sorts first.
//!
//! A second schedule for a position that already has one pending is
//! **dropped**: `LevelChunkTicks.schedule` guards on a set hashed by
//! `ScheduledTick.UNIQUE_TICK_HASH`, which is `(type, pos)`. Without that a
//! block that reschedules itself on every neighbour change accumulates one
//! tick per change and then fires all of them.

use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::edits::Blocks;
use crate::interact::{BlockPos, Face};
use crate::shapes::{Effects, ShapeTable};

/// `CoralBlock`'s five instances and the block each dies into.
///
/// Read from the jar rather than derived from the name, because "add `dead_`"
/// is a rule and this is a constructor argument: `Blocks`' static initialiser
/// builds each with `new CoralBlock(Blocks.DEAD_X_CORAL_BLOCK, ...)`, and
/// reflecting `CoralBlock.deadBlock` over `BuiltInRegistries.BLOCK` returns
/// exactly these five pairs and no others.
pub const CORAL_DEAD: [(&str, &str); 5] = [
    (
        "minecraft:tube_coral_block",
        "minecraft:dead_tube_coral_block",
    ),
    (
        "minecraft:brain_coral_block",
        "minecraft:dead_brain_coral_block",
    ),
    (
        "minecraft:bubble_coral_block",
        "minecraft:dead_bubble_coral_block",
    ),
    (
        "minecraft:fire_coral_block",
        "minecraft:dead_fire_coral_block",
    ),
    (
        "minecraft:horn_coral_block",
        "minecraft:dead_horn_coral_block",
    ),
];

/// `CoralBlock.getStateForPlacement`'s delay: `60 + random.nextInt(40)`.
pub const CORAL_DELAY_BASE: i64 = 60;
/// The bound of the `nextInt` in that same expression.
pub const CORAL_DELAY_SPREAD: i64 = 40;

/// `TickPriority.NORMAL`, by its ordinal in the enum's own `$values`:
/// `EXTREMELY_HIGH, VERY_HIGH, HIGH, NORMAL, LOW, VERY_LOW, EXTREMELY_LOW`.
pub const PRIORITY_NORMAL: u8 = 3;

/// One entry of `LevelTicks`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledTick {
    pub pos: BlockPos,
    /// The block that asked to be woken. `ScheduledTick.type`, which for a
    /// block tick is the `Block` and not the state.
    pub block: String,
    pub trigger: i64,
    pub priority: u8,
    /// Stamped at schedule time. See the module docs.
    pub sub_tick: u64,
}

/// `ServerLevel`'s **two** `LevelTicks`, cut down to what one server thread
/// needs.
///
/// Shared between the connection threads, which schedule, and the clock
/// thread, which drains — so the locks are the point rather than an
/// implementation detail.
///
/// # Two queues, not one with a tag
///
/// `ServerLevel` holds `blockTicks` and `fluidTicks` as separate `LevelTicks`
/// and `ServerLevel.tick` drains them one after the other under the one
/// `getGameTime()` read at offset 240: `blockTicks.tick` at 269, then
/// `fluidTicks.tick` at 296. Each queue stamps its own `subTickOrder`, so
/// the counter carries no ordering across the two; **every fluid tick due at
/// `T` runs after every block tick due at `T`**, whatever order they were
/// asked for in. A single queue with a discriminant would order them by
/// request.
#[derive(Debug, Default)]
pub struct ScheduledTicks {
    inner: Mutex<Inner>,
    /// `ServerLevel.fluidTicks`. Keyed by the concrete fluid's registry id —
    /// `minecraft:water` and `minecraft:flowing_water` are separate entries
    /// at one position, because `Fluids.WATER` and `Fluids.FLOWING_WATER` are
    /// separate instances and `UNIQUE_TICK_HASH` hashes the instance.
    fluid: Mutex<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    /// Keyed by the whole `DRAIN_ORDER` so iteration *is* the drain order:
    /// trigger, then priority, then the sub-tick counter.
    pending: BTreeMap<(i64, u8, u64), ScheduledTick>,
    /// `LevelChunkTicks.ticksPerPosition` — `(type, pos)`, which is what
    /// `UNIQUE_TICK_HASH` hashes.
    unique: std::collections::BTreeSet<(BlockPos, String)>,
    next_sub_tick: u64,
    /// `LevelTicks.toRunThisTickSet` — the ticks collected for this tick
    /// that have not run yet. Vanilla builds it lazily out of the
    /// `toRunThisTick` queue on the first `willTickThisTick` call and
    /// `runCollectedTicks` removes each entry as it polls it, so at any
    /// moment it holds exactly the collected-but-not-yet-run set. Filled by
    /// [`ScheduledTicks::drain_due`], emptied one entry at a time by
    /// [`ScheduledTicks::mark_run`].
    this_tick: std::collections::BTreeSet<(BlockPos, String)>,
}

impl ScheduledTicks {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The one queue this process runs, so the connection threads and the
    /// clock thread are talking about the same ticks.
    pub fn shared() -> &'static Self {
        static TICKS: std::sync::OnceLock<ScheduledTicks> = std::sync::OnceLock::new();
        TICKS.get_or_init(Self::new)
    }

    /// `Level.scheduleTick(pos, block, delay, priority)`.
    ///
    /// Returns whether it was accepted — `false` means this position already
    /// has a tick pending for this block, which vanilla drops rather than
    /// queues twice.
    ///
    /// # Panics
    /// If the lock is poisoned, which means a thread died holding it.
    pub fn schedule(&self, pos: BlockPos, block: &str, now: i64, delay: i64, priority: u8) -> bool {
        Self::push(&self.inner, pos, block, now, delay, priority)
    }

    /// `Level.scheduleTick(pos, Fluid, delay)` — the **fluid** queue.
    ///
    /// No priority argument because no call site has one: every
    /// `scheduleTick` reaching `fluidTicks` in 26.2 is the three-argument
    /// overload, `TickPriority.NORMAL`.
    ///
    /// # Panics
    /// If the lock is poisoned.
    pub fn schedule_fluid(&self, pos: BlockPos, fluid: &str, now: i64, delay: i64) -> bool {
        Self::push(&self.fluid, pos, fluid, now, delay, PRIORITY_NORMAL)
    }

    fn push(
        queue: &Mutex<Inner>,
        pos: BlockPos,
        block: &str,
        now: i64,
        delay: i64,
        priority: u8,
    ) -> bool {
        let mut inner = queue.lock().expect("tick queue");
        if !inner.unique.insert((pos, block.to_string())) {
            return false;
        }
        let sub_tick = inner.next_sub_tick;
        inner.next_sub_tick += 1;
        inner.pending.insert(
            (now + delay, priority, sub_tick),
            ScheduledTick {
                pos,
                block: block.to_string(),
                trigger: now + delay,
                priority,
                sub_tick,
            },
        );
        true
    }

    /// Everything due at or before `now`, in `DRAIN_ORDER`.
    ///
    /// Drained rather than peeked: vanilla collects the tick's work into a
    /// list first and runs it after, so a tick that schedules another one
    /// cannot make the same pass run it.
    ///
    /// # Panics
    /// If the lock is poisoned.
    pub fn drain_due(&self, now: i64) -> Vec<ScheduledTick> {
        Self::take(&self.inner, now)
    }

    /// The same for the **fluid** queue, which `ServerLevel.tick` drains
    /// second.
    ///
    /// # Panics
    /// If the lock is poisoned.
    pub fn drain_due_fluid(&self, now: i64) -> Vec<ScheduledTick> {
        Self::take(&self.fluid, now)
    }

    fn take(queue: &Mutex<Inner>, now: i64) -> Vec<ScheduledTick> {
        let mut inner = queue.lock().expect("tick queue");
        let due: Vec<(i64, u8, u64)> = inner
            .pending
            .range(..=(now, u8::MAX, u64::MAX))
            .map(|(k, _)| *k)
            .collect();
        let mut out = Vec::with_capacity(due.len());
        for key in due {
            if let Some(tick) = inner.pending.remove(&key) {
                inner.unique.remove(&(tick.pos, tick.block.clone()));
                inner.this_tick.insert((tick.pos, tick.block.clone()));
                out.push(tick);
            }
        }
        out
    }

    /// Every pending tick whose position falls in chunk `(cx, cz)`, in
    /// `DRAIN_ORDER`, **without draining them**.
    ///
    /// For the save path. `LevelChunkTicks.save` writes exactly this set into
    /// the chunk's `block_ticks`, and a save that drained would run the
    /// world's clock forward by writing it down.
    ///
    /// # Panics
    /// If the lock is poisoned.
    #[must_use]
    pub fn pending_in_chunk(&self, cx: i32, cz: i32) -> Vec<ScheduledTick> {
        Self::pending_in(&self.inner, cx, cz)
    }

    /// [`Self::pending_in_chunk`] for the fluid queue — what
    /// `LevelChunkTicks.save` writes into the chunk's `fluid_ticks`.
    ///
    /// # Panics
    /// If the lock is poisoned.
    #[must_use]
    pub fn pending_fluid_in_chunk(&self, cx: i32, cz: i32) -> Vec<ScheduledTick> {
        Self::pending_in(&self.fluid, cx, cz)
    }

    fn pending_in(queue: &Mutex<Inner>, cx: i32, cz: i32) -> Vec<ScheduledTick> {
        queue
            .lock()
            .expect("tick queue")
            .pending
            .values()
            .filter(|t| (t.pos.0 >> 4, t.pos.2 >> 4) == (cx, cz))
            .cloned()
            .collect()
    }

    /// `LevelTicks.runCollectedTicks`'s `toRunThisTickSet.remove(tick)` —
    /// call it for each drained tick **before** running it, which is where
    /// vanilla's poll sits relative to the `BiConsumer.accept`. A block whose
    /// tick is running is no longer one that *will* tick this tick.
    pub fn mark_run(&self, tick: &ScheduledTick) {
        let mut inner = self.inner.lock().expect("tick queue");
        inner.this_tick.remove(&(tick.pos, tick.block.clone()));
    }

    /// [`Self::mark_run`] for the fluid queue.
    ///
    /// # Panics
    /// If the lock is poisoned.
    pub fn mark_run_fluid(&self, tick: &ScheduledTick) {
        let mut inner = self.fluid.lock().expect("tick queue");
        inner.this_tick.remove(&(tick.pos, tick.block.clone()));
    }

    /// `LevelTicks.willTickThisTick`: collected for this tick and not run
    /// yet. `DiodeBlock.checkTickOnNeighbor` and the comparator's override
    /// both guard on it, because the whole tick's worth was polled out of
    /// `ticksPerPosition` before any of it ran — so without this a neighbour
    /// change arriving mid-tick would book a second tick for a diode whose
    /// first is still in the queue.
    #[must_use]
    pub fn will_tick_this_tick(&self, pos: BlockPos, block: &str) -> bool {
        let inner = self.inner.lock().expect("tick queue");
        inner.this_tick.contains(&(pos, block.to_string()))
    }

    /// `LevelTicks.hasScheduledTick`: a pending tick for `(pos, block)`, the
    /// `UNIQUE_TICK_HASH` key. **Pending**, not collected — a tick drained
    /// this tick has already left `ticksPerPosition`.
    #[must_use]
    pub fn has_scheduled_tick(&self, pos: BlockPos, block: &str) -> bool {
        let inner = self.inner.lock().expect("tick queue");
        inner.unique.contains(&(pos, block.to_string()))
    }

    /// How many ticks are waiting.
    ///
    /// # Panics
    /// If the lock is poisoned.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.lock().expect("tick queue").pending.len()
    }

    /// How many **fluid** ticks are waiting.
    ///
    /// # Panics
    /// If the lock is poisoned.
    #[must_use]
    pub fn fluid_len(&self) -> usize {
        self.fluid.lock().expect("tick queue").pending.len()
    }

    /// Both queues empty.
    ///
    /// # Panics
    /// If the lock is poisoned.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0 && self.fluid_len() == 0
    }
}

/// `RUSTMC_TRACE_TICKS=1` — every schedule and every fire, with what the
/// position actually held when the tick ran.
///
/// The instrument this module was missing on its first run: a tick that never
/// fires and a tick that fires and finds the wrong block are the same silence
/// from a client.
fn trace(message: impl FnOnce() -> String) {
    if std::env::var_os("RUSTMC_TRACE_TICKS").is_some() {
        eprintln!("tick: {}", message());
    }
}

/// The block name out of a state string.
fn name_of(state: &str) -> &str {
    state.split('[').next().unwrap_or(state)
}

/// `CoralBlock.scanForWater` — any of the six neighbours whose fluid is in
/// `#minecraft:water`.
///
/// Note what it does **not** read: the block's own state. A waterlogged coral
/// with dry neighbours still dies.
#[must_use]
pub fn scan_for_water(blocks: &dyn Blocks, pos: BlockPos) -> bool {
    FACES.into_iter().any(|d| blocks.water_at(d.relative(pos)))
}

/// `ButtonBlock`'s simple name, as `UseGolden` records it.
///
/// Every button in 26.2 is an instance of this one class — stone, polished
/// blackstone and the eleven wooden ones differ only in constructor
/// arguments (`BlockSetType` and `ticksToStayPressed`), so there is no
/// subclass to also match.
pub(crate) const BUTTON_BLOCK: &str = "ButtonBlock";

/// `Direction.values()`.
const FACES: [Face; 6] = [
    Face::Down,
    Face::Up,
    Face::North,
    Face::South,
    Face::West,
    Face::East,
];

/// What a freshly placed block asks the tick loop for.
///
/// Today that is `CoralBlock.getStateForPlacement` and nothing else, which is
/// not a simplification: `placement.json`'s `schedulesTickBlocks` is those
/// five blocks. Returns the delay it scheduled, for a caller that wants to
/// say so.
pub fn on_placed(
    ticks: &ScheduledTicks,
    blocks: &dyn Blocks,
    pos: BlockPos,
    state: &str,
    now: i64,
) -> Option<i64> {
    let block = name_of(state);
    if !CORAL_DEAD.iter().any(|(live, _)| *live == block) {
        return None;
    }
    // Traced *before* the water check, not after: the first run of this
    // scheduled nothing and the trace could not say whether the placement
    // never reached here or reached here and found water.
    let wet = scan_for_water(blocks, pos);
    trace(|| {
        let sides: Vec<String> = FACES
            .iter()
            .map(|d| {
                let at = d.relative(pos);
                format!("{d:?}={}", blocks.state_at(at))
            })
            .collect();
        format!(
            "placed {block} at {pos:?} at tick {now}, water {wet} — {}",
            sides.join(" ")
        )
    });
    if wet {
        return None;
    }
    let delay = CORAL_DELAY_BASE + next_spread(now, pos);
    let taken = ticks.schedule(pos, block, now, delay, PRIORITY_NORMAL);
    trace(|| {
        format!(
            "schedule {block} at {pos:?} for tick {} ({taken})",
            now + delay
        )
    });
    taken.then_some(delay)
}

/// `ContainerOpenersCounter.scheduleRecheck` —
/// `level.scheduleTick(pos, block, 5)`.
///
/// The **same** queue every other block tick goes into, which is the point of
/// putting it here rather than giving containers a timer of their own: the
/// recheck is an ordinary block tick on the container's own block, it runs in
/// `LevelTicks.tick` with everything else, and `UNIQUE_TICK_HASH` therefore
/// deduplicates a second schedule at the same position exactly as vanilla
/// does. A parallel timer would have had its own drain order and its own
/// idea of "already scheduled".
///
/// Returns whether it was taken, which is `false` when one is already
/// pending — the case `recheckOpeners` re-arming into an unfired tick would
/// hit.
pub fn schedule_container_recheck(
    ticks: &ScheduledTicks,
    pos: BlockPos,
    state: &str,
    now: i64,
) -> bool {
    ticks.schedule(
        pos,
        name_of(state),
        now,
        crate::containers::CHECK_TICK_DELAY,
        PRIORITY_NORMAL,
    )
}

/// What a right-clicked block asks the tick loop for.
///
/// The delay and the priority come from `use_without_item.json`, which
/// records what `Level.scheduleTick` was actually called with — **not** from
/// a number here. Buttons are the reason: stone and polished blackstone stay
/// pressed 20 ticks and every wooden one 30, and `ticksToStayPressed` is a
/// constructor argument with no rule connecting it to the block's name.
///
/// Returns whether the queue took it. A second tick for a position that
/// already has one pending is dropped by `LevelChunkTicks.schedule`'s
/// `UNIQUE_TICK_HASH` guard, which [`ScheduledTicks::schedule`] reproduces.
pub fn on_used(
    ticks: &ScheduledTicks,
    pos: BlockPos,
    state: &str,
    tick: crate::placedby::Tick,
    now: i64,
) -> bool {
    // **Block ticks only.** A waterlogged trapdoor's right-click schedules a
    // *fluid* tick — `TrapDoorBlock.toggle` calls
    // `scheduleTick(pos, Fluids.WATER, ...)` and `TrapDoorBlock` has no `tick`
    // override — and this server has no fluid queue. Queueing it as a block
    // tick would fire a tick nothing answers, and `UNIQUE_TICK_HASH` would
    // then make that entry block a real block tick at the same position.
    //
    // It goes in the **fluid** queue now. The fluid is water, and that is
    // read off the file rather than assumed:
    // `every_fluid_tick_in_the_use_table_is_waterlogged_water` asserts that
    // every `tickKind = "fluid"` row carries `waterlogged=true`, which is only
    // ever a water source, at `WATER.getTickDelay` = 5.
    if !tick.is_block {
        let taken = ticks.schedule_fluid(pos, crate::fluid::WATER, now, tick.delay);
        trace(|| {
            format!(
                "schedule fluid water at {pos:?} for tick {} ({taken})",
                now + tick.delay
            )
        });
        return taken;
    }
    let block = name_of(state);
    let taken = ticks.schedule(pos, block, now, tick.delay, tick.priority);
    trace(|| {
        format!(
            "schedule {block} at {pos:?} for tick {} ({taken})",
            now + tick.delay
        )
    });
    taken
}

/// `random.nextInt(40)`, from a source that is not vanilla's.
///
/// Vanilla draws from `level.getRandom()`, the server level's shared
/// `RandomSource`, which this server does not have — nothing else in it
/// consumes that stream, so reproducing the stream would mean inventing every
/// other draw that shares it. Named rather than hidden: the *delay* is not
/// reproducible against vanilla, only its range is. Everything about the tick
/// that a client can see — that it fires, and what it does — is.
fn next_spread(now: i64, (x, y, z): BlockPos) -> i64 {
    let mut h = (now as u64)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add((x as i64 as u64).wrapping_mul(0x517C_C1B7_2722_0A95))
        .wrapping_add((y as i64 as u64).wrapping_mul(0x2545_F491_4F6C_DD1D))
        .wrapping_add((z as i64 as u64).wrapping_mul(0xD6E8_FEB8_6659_FD93));
    h ^= h >> 33;
    #[expect(
        clippy::cast_possible_wrap,
        reason = "masked into 0..CORAL_DELAY_SPREAD before it is used"
    )]
    let spread = (h % CORAL_DELAY_SPREAD as u64) as i64;
    spread
}

/// Stamp a batch of [`crate::shapes::TickRequest`]s with the game time and put
/// them on the queue.
///
/// **One `now` for the whole batch.** Every request in one `Effects` was
/// produced inside one `Level.setBlock` cascade, which in vanilla runs inside
/// one tick and reads one `getGameTime()`. The list is in the order the rules
/// asked, so `ScheduledTicks::schedule`'s `sub_tick` counter — the third and
/// last key of `ScheduledTick.DRAIN_ORDER` — comes out in vanilla's order too.
///
/// Requests are dropped silently when the queue already holds a tick for the
/// same `(pos, block)`; that is `LevelChunkTicks.schedule`'s own behaviour and
/// the reason none of the three callers ports its `willTickThisTick` guard.
/// See [`crate::shapes::TickRequest`].
pub fn drain_requests(queue: &ScheduledTicks, requests: &[crate::shapes::TickRequest], now: i64) {
    for r in requests {
        queue.schedule(r.pos, &r.block, now, r.delay, r.priority);
    }
}

/// [`drain_requests`] for both of an `Effects`' request lists — the block
/// ticks into the block queue and the fluid ticks into the fluid queue, each
/// in the order its rules asked.
pub fn drain_effects(queue: &ScheduledTicks, effects: &crate::shapes::Effects, now: i64) {
    drain_requests(queue, &effects.ticks, now);
    for r in &effects.fluid_ticks {
        queue.schedule_fluid(r.pos, &r.block, now, r.delay);
    }
}

/// Run one scheduled tick, returning the block updates it made.
///
/// The state is re-read rather than carried: a tick is scheduled for a
/// *position*, and vanilla's own `LevelTicks` runs `getBlockState(pos).tick`,
/// so a block that was replaced between the schedule and the fire runs the
/// new block's rule or none at all.
pub fn run(
    tick: &ScheduledTick,
    blocks: &dyn Blocks,
    shapes: Option<&ShapeTable>,
    classes: Option<&crate::placedby::PlacedByTable>,
    now: i64,
) -> Effects {
    let mut updates = Effects::new();
    // The corpus positive control: a scheduled tick that never comes due,
    // which is the server before anything happened on a later tick than the
    // packet that caused it. A button that stays pressed, a torch that never
    // goes dark, a repeater with no delay. One site covers every check whose
    // claim is "and then, later" — see `crate::sabotage`.
    if crate::sabotage::fires_at(crate::sabotage::Kind::Sched, tick.pos.0) {
        return updates;
    }
    let state = blocks.state_at(tick.pos);
    trace(|| format!("fire {} at {:?} — found {state}", tick.block, tick.pos));
    if name_of(&state) != tick.block {
        return updates;
    }
    // The three redstone rules live in `crate::power` beside the neighbour
    // paths that schedule them — a `Block.tick` that reads `getSignal` and
    // ends in `updateNeighborsInFront` is a redstone rule wearing a scheduler's
    // name, and splitting it from `neighborChanged` would put the two halves of
    // one mechanism in two files. Each is gated on the block id here so that
    // this dispatch stays readable as a list of what ticks.
    if let Some(signals) = crate::power::shared() {
        match name_of(&state) {
            "minecraft:redstone_torch" | "minecraft:redstone_wall_torch" => {
                crate::power::torch_tick(
                    blocks,
                    signals,
                    shapes,
                    tick.pos,
                    &state,
                    now,
                    &mut updates,
                );
                return updates;
            }
            "minecraft:redstone_lamp" => {
                crate::power::lamp_tick(blocks, signals, shapes, tick.pos, &state, &mut updates);
                return updates;
            }
            // `DispenserBlock.tick` is `dispenseFrom` and nothing else —
            // no re-read of the signal — and `DropperBlock` inherits it. The
            // work needs the block entity, so it goes back as a request; see
            // `crate::shapes::Effects::dispenses`.
            "minecraft:dispenser" | "minecraft:dropper" | "minecraft:crafter" => {
                updates.dispenses.push(tick.pos);
                return updates;
            }
            "minecraft:observer" => {
                crate::power::observer_tick(
                    blocks,
                    signals,
                    shapes,
                    tick.pos,
                    &state,
                    &mut updates,
                );
                return updates;
            }
            "minecraft:comparator" => {
                crate::comparator::tick(blocks, signals, shapes, tick.pos, &state, &mut updates);
                return updates;
            }
            "minecraft:repeater" => {
                crate::power::repeater_tick(
                    blocks,
                    signals,
                    shapes,
                    tick.pos,
                    &state,
                    &mut updates,
                );
                return updates;
            }
            _ => {}
        }
    }
    // `FallingBlock.tick` — the gravity block, whose whole rule is a support
    // test and an entity. Gated on the class map through the shape table's
    // precomputed set, so a `SandBlock` three levels below `FallingBlock`
    // answers yes and `suspicious_sand`, which is a `BrushableBlock`, does
    // not. See `crate::fallable`.
    if let Some(shapes) = shapes {
        if shapes.is_gravity_block(&state) {
            let below = (tick.pos.0, tick.pos.1 - 1, tick.pos.2);
            let support = crate::fallable::Support {
                props: crate::fallable::props(),
                classes,
                tags: crate::fallable::tags(),
            };
            // `level.getMinY()`. `None` is a world with no height model, and
            // the refusal is deliberate — see `Blocks::level_min_y`.
            if let Some(min_y) = blocks.level_min_y() {
                if let Some(fall) = crate::fallable::tick(
                    &support,
                    &blocks.state_at(below),
                    tick.pos,
                    &state,
                    min_y,
                ) {
                    // `FallingBlockEntity.fall`, offsets 60-79, in order:
                    //
                    //   setBlock(pos, state.getFluidState().createLegacyBlock(), 3)
                    //   addFreshEntity(e)
                    //
                    // The write is `legacy_fluid_at` and **not air** for the
                    // same reason a dig is not: a waterlogged gravity block
                    // would have to leave water behind. None of the 23
                    // `FallingBlock`s in 26.2 is waterloggable, so this is air
                    // in every case that exists today — written as the fluid
                    // read anyway, because the alternative is a hard-coded
                    // `minecraft:air` that becomes wrong silently.
                    let leaves = blocks.legacy_fluid_at(tick.pos);
                    crate::shapes::set_block(
                        blocks,
                        Some(shapes),
                        tick.pos,
                        &leaves,
                        crate::shapes::UPDATE_ALL,
                        crate::shapes::RECURSION_LIMIT,
                        &mut updates,
                    );
                    // Pushed *after* the write, matching `fall`'s own order.
                    // That the client sees the reverse is the drain site's
                    // doing and not a reordering here — `Effects::falls`.
                    updates.falls.push(fall);
                }
            }
            return updates;
        }
    }
    // Vanilla's own class for this block, out of the extract — never "the
    // id ends in `_button`". `set_placed_by.json` carries it for the **whole
    // registry**, which is why it is the source and the use table is not:
    // that one names only the 184 blocks with a right-click behaviour, so it
    // would answer "not a button" for anything it happened not to carry.
    //
    // `is_a`, not a string compare: the test vanilla performs is
    // `instanceof`, and a class with subclasses answers differently. Every
    // button in 26.2 is a plain `ButtonBlock` today, so this costs nothing
    // now — but the doors next door are not, and the two rules must not
    // disagree about what "is this class" means.
    // `ButtonBlock.tick` -> `checkPressed`, read from the bytecode:
    //
    // ```text
    //   tick:          if (!state.getValue(POWERED)) return;
    //                  checkPressed(state, level, pos);
    //   checkPressed:  arrow = type.canButtonBeActivatedByArrows()
    //                          ? first AbstractArrow in the shape's bounds
    //                          : null
    //                  hasArrow = arrow != null
    //                  if (hasArrow != state.getValue(POWERED)):
    //                      setBlock(pos, state.setValue(POWERED, hasArrow), 3)
    //                      updateNeighbours(...); playSound(...); gameEvent(...)
    //                  if (hasArrow): scheduleTick(pos, this, ticksToStayPressed)
    // ```
    //
    // `hasArrow` is **false** here and that is a statement about this world,
    // not a simplification: there are no entities of any kind in it, so
    // `getEntitiesOfClass` would return an empty list whatever the button's
    // `BlockSetType` says. When arrows exist this needs the entity sweep and
    // the `canButtonBeActivatedByArrows` flag, and the shape of the rule
    // above is already the shape that wants them.
    //
    // So: a pressed button un-presses, an unpressed one returns at the first
    // line, and nothing reschedules. Flags **3**, from the bytecode, not
    // the coral's 2.
    if classes.is_some_and(|c| c.is_a(&tick.block, BUTTON_BLOCK)) {
        if crate::placement::property_of(&state, "powered") == Some("true") {
            let off = crate::placement::with_property(&state, "powered", "false");
            crate::shapes::set_block(
                blocks,
                shapes,
                tick.pos,
                &off,
                crate::shapes::UPDATE_ALL,
                crate::shapes::RECURSION_LIMIT,
                &mut updates,
            );
            // `checkPressed`'s own last two calls are `updateNeighbours` then
            // `playSound(null, level, pos, hasArrow)` — **`aconst_null` at
            // offset 116**, so the un-press is heard by everybody, the player
            // who pressed it included. `hasArrow` is the `false` argued for
            // above, so this is the `click_off` half of the pair.
            //
            // That asymmetry against the press is the whole reason
            // `crate::sounds::Sound` carries an exclusion at all, and it is
            // measurable: in the traces of 2026-08-07 02:19 the actor's stream
            // for `330-button-unpresses-itself` carries `click_off` and not
            // `click_on`, and the observer's carries both.
            updates.sounds.push(crate::sounds::button_click(
                &tick.block,
                tick.pos,
                false,
                None,
            ));
        }
        return updates;
    }
    // `FireBlock.tick` — the whole method, in `crate::fire`. Gated on the
    // block id here for the same reason the three redstone rules are: this
    // dispatch stays readable as a list of what ticks, and the rule itself
    // lives beside the odds table and the placement walk it shares.
    //
    // **`minecraft:soul_fire` is deliberately absent.** `SoulFireBlock` has
    // no `tick` override, so a soul fire never schedules and never burns;
    // adding it here would invent a mechanic.
    if tick.block == "minecraft:fire" {
        if let Some(odds) = crate::fire::shared_odds() {
            // Per draw, never a held guard: the tick's own `set_block`
            // re-enters the stream through `fire::on_place`. See
            // `crate::fire::LevelRandom`.
            crate::fire::tick(
                odds,
                blocks,
                shapes,
                tick.pos,
                &state,
                &mut crate::fire::LevelRandom,
                crate::fire::Env::default(),
                &mut updates,
            );
        }
        return updates;
    }
    // `CoralBlock.tick`.
    if let Some((_, dead)) = CORAL_DEAD.iter().find(|(live, _)| *live == tick.block) {
        if !scan_for_water(blocks, tick.pos) {
            if let Some(dead_state) = blocks.default_state(dead) {
                // Flags **2**, not 3: `UPDATE_CLIENTS` without
                // `UPDATE_NEIGHBORS`. `UPDATE_KNOWN_SHAPE` is still clear, so
                // the shape updates around it do run.
                crate::shapes::set_block(
                    blocks,
                    shapes,
                    tick.pos,
                    &dead_state,
                    crate::shapes::UPDATE_CLIENTS,
                    crate::shapes::RECURSION_LIMIT,
                    &mut updates,
                );
            }
        }
    }
    updates
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap as Map;

    #[derive(Default)]
    struct Fake {
        blocks: Mutex<Map<BlockPos, String>>,
    }
    impl Fake {
        fn with(states: &[(BlockPos, &str)]) -> Self {
            let f = Self::default();
            for (pos, state) in states {
                f.blocks
                    .lock()
                    .expect("fake")
                    .insert(*pos, (*state).to_string());
            }
            f
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
            Some(7)
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
        fn replaceable(&self, _: BlockPos, _: &crate::placement::Replacing<'_>) -> bool {
            true
        }
        fn legacy_fluid_at(&self, _: BlockPos) -> String {
            "minecraft:air".to_string()
        }
        fn water_at(&self, pos: BlockPos) -> bool {
            self.state_at(pos).starts_with("minecraft:water")
        }
    }

    /// **A fire tick that lights a second fire must come back.** The new
    /// fire is written by `set_block` inside `fire::tick`, that write reaches
    /// `fire::on_place`, and `on_place` draws `getFireTickDelay` from the
    /// level's stream — the same stream the tick is drawing from. This call
    /// site held that stream's `MutexGuard` across the tick until train-b,
    /// and the nested lock hung the clock thread for good: every scheduled
    /// tick in the world stopped, and the corpus read it as a server that
    /// would not let the player walk. So the assertion is a timeout.
    ///
    /// Driven through [`run`], not `fire::tick`, because the defect was the
    /// guard at this call site; and it counts the fires it saw appear,
    /// because a run in which nothing spread never re-entered the stream and
    /// would pass on the deadlocking code. Needs the real odds table: without
    /// it `run` skips the fire arm and `on_place` returns before it draws.
    #[test]
    fn a_fire_tick_that_spreads_returns_through_the_new_fires_on_place() {
        assert!(
            crate::fire::shared_odds().is_some(),
            "data/vanilla fire odds table: without it no fire ticks and this proves nothing"
        );
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut states: Vec<(BlockPos, &str)> = Vec::new();
            for x in -3..=3 {
                for z in -3..=3 {
                    states.push(((x, 64, z), "minecraft:stone"));
                }
            }
            for y in 65..=67 {
                states.push(((1, y, 0), "minecraft:oak_planks"));
                states.push(((-1, y, 0), "minecraft:oak_planks"));
                states.push(((0, y, 1), "minecraft:oak_planks"));
            }
            states.push((
                (0, 65, 0),
                "minecraft:fire[age=0,east=false,north=false,south=false,up=false,west=false]",
            ));
            let w = Fake::with(&states);
            let fires = |w: &Fake| -> Vec<BlockPos> {
                w.blocks
                    .lock()
                    .expect("fake")
                    .iter()
                    .filter(|(_, s)| s.starts_with("minecraft:fire"))
                    .map(|(p, _)| *p)
                    .collect()
            };
            let mut seen: std::collections::BTreeSet<BlockPos> = fires(&w).into_iter().collect();
            let mut lit = 0;
            for now in 0..400 {
                let Some(pos) = fires(&w).first().copied() else {
                    break;
                };
                let tick = ScheduledTick {
                    pos,
                    block: "minecraft:fire".to_string(),
                    trigger: now,
                    priority: PRIORITY_NORMAL,
                    sub_tick: 0,
                };
                let _ = run(&tick, &w, None, None, now);
                for p in fires(&w) {
                    if seen.insert(p) {
                        lit += 1;
                    }
                }
            }
            tx.send(lit).ok();
        });
        let lit = rx.recv_timeout(std::time::Duration::from_secs(60)).expect(
            "ticks::run did not return: a fire tick re-entered the level stream under a held guard",
        );
        assert!(
            lit > 0,
            "no tick lit a second fire, so on_place never drew inside a tick and this measured nothing"
        );
    }

    /// `DRAIN_ORDER` is trigger, then priority, then the counter — and the
    /// counter is what makes two ticks asked for at the same instant run in
    /// the order they were asked for.
    #[test]
    fn ticks_drain_in_trigger_then_priority_then_request_order() {
        let ticks = ScheduledTicks::new();
        ticks.schedule((0, 0, 0), "a", 0, 5, PRIORITY_NORMAL);
        ticks.schedule((1, 0, 0), "b", 0, 5, 0); // EXTREMELY_HIGH
        ticks.schedule((2, 0, 0), "c", 0, 5, PRIORITY_NORMAL);
        ticks.schedule((3, 0, 0), "d", 0, 1, PRIORITY_NORMAL);

        assert!(ticks.drain_due(0).is_empty(), "nothing is due yet");
        let early = ticks.drain_due(1);
        assert_eq!(early.len(), 1, "only the one at trigger 1");
        assert_eq!(early[0].block, "d");

        let drained = ticks.drain_due(9);
        let due: Vec<&str> = drained.iter().map(|t| t.block.as_str()).collect();
        assert_eq!(due, vec!["b", "a", "c"], "priority first, then who asked");
        assert!(ticks.is_empty());
    }

    /// A second schedule for the same position and block is dropped, which is
    /// `LevelChunkTicks.ticksPerPosition`. Without it a block that reschedules
    /// on every neighbour change fires once per change.
    #[test]
    fn one_tick_per_position_and_block() {
        let ticks = ScheduledTicks::new();
        assert!(ticks.schedule((0, 0, 0), "coral", 0, 10, PRIORITY_NORMAL));
        assert!(!ticks.schedule((0, 0, 0), "coral", 0, 20, PRIORITY_NORMAL));
        assert!(
            ticks.schedule((0, 0, 0), "other", 0, 10, PRIORITY_NORMAL),
            "a different block at the same position is a different tick"
        );
        assert_eq!(ticks.len(), 2);
        // And once it has fired, the position can be scheduled again.
        ticks.drain_due(100);
        assert!(ticks.schedule((0, 0, 0), "coral", 100, 10, PRIORITY_NORMAL));
    }

    /// The whole of `CoralBlock`, over the fake: placed dry it asks to be
    /// woken, and when it is woken still dry it dies.
    #[test]
    fn a_coral_placed_out_of_water_dies_when_its_tick_fires() {
        let world = Fake::with(&[((0, 64, 0), "minecraft:fire_coral_block")]);
        let ticks = ScheduledTicks::new();

        let delay = on_placed(&ticks, &world, (0, 64, 0), "minecraft:fire_coral_block", 0)
            .expect("a dry coral schedules a tick");
        assert!(
            (CORAL_DELAY_BASE..CORAL_DELAY_BASE + CORAL_DELAY_SPREAD).contains(&delay),
            "60 + nextInt(40), got {delay}"
        );

        assert!(ticks.drain_due(delay - 1).is_empty());
        let due = ticks.drain_due(delay);
        assert_eq!(due.len(), 1);
        let updates = run(&due[0], &world, None, None, 0);
        assert_eq!(updates.blocks.len(), 1, "one block changed");
        assert_eq!(
            world.state_at((0, 64, 0)),
            "minecraft:dead_fire_coral_block"
        );
    }

    /// A `PlacedByTable` carrying only a class map, which is all [`run`]
    /// asks it for.
    fn classes(rows: &[(&str, &str)]) -> crate::placedby::PlacedByTable {
        let pairs: Vec<String> = rows
            .iter()
            .map(|(block, class)| format!(r#""{block}": "{class}""#))
            .collect();
        let p = std::env::temp_dir().join(format!(
            "rustmc-ticks-classes-{}-{:?}.json",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(
            &p,
            format!(r#"{{"classes": {{{}}}, "table": []}}"#, pairs.join(",")),
        )
        .expect("write");
        let t = crate::placedby::PlacedByTable::load(&p).expect("load");
        let _ = std::fs::remove_file(&p);
        t
    }

    /// `ButtonBlock.tick` -> `checkPressed`: a pressed button un-presses.
    ///
    /// **The two halves differ in exactly one property.** `tick` opens with
    /// `if (!state.getValue(POWERED)) return;`, so the un-press is not "write
    /// `powered=false`" — it is conditional on the button being pressed, and a
    /// version that wrote unconditionally would pass a test that only ever
    /// ticked a pressed button. Both are ticked here and only one moves.
    #[test]
    fn a_pressed_button_unpresses_and_an_unpressed_one_does_nothing() {
        let uses = classes(&[("minecraft:stone_button", "ButtonBlock")]);
        for (start, want) in [
            (
                "powered=true",
                "minecraft:stone_button[face=floor,facing=south,powered=false]",
            ),
            (
                "powered=false",
                "minecraft:stone_button[face=floor,facing=south,powered=false]",
            ),
        ] {
            let from = format!("minecraft:stone_button[face=floor,facing=south,{start}]");
            let world = Fake::with(&[((0, 64, 0), from.as_str())]);
            let ticks = ScheduledTicks::new();
            on_used(
                &ticks,
                (0, 64, 0),
                &from,
                crate::placedby::Tick {
                    delay: 20,
                    priority: PRIORITY_NORMAL,
                    is_block: true,
                },
                0,
            );
            let due = ticks.drain_due(20);
            assert_eq!(due.len(), 1, "{start}: scheduled at the table's delay");
            let updates = run(&due[0], &world, None, Some(&uses), 0);
            assert_eq!(world.state_at((0, 64, 0)), want, "{start}");
            assert_eq!(
                updates.blocks.len(),
                usize::from(start == "powered=true"),
                "{start}: an unpressed button must write nothing at all, \
                 not write the same state back"
            );
        }
    }

    /// **A fluid tick is refused, and a block tick beside it is not.**
    ///
    /// Twenty-two of the 34 blocks whose right-click schedules anything are
    /// trapdoors, and `TrapDoorBlock` has no `tick` override: `toggle` ends
    /// with `scheduleTick(pos, Fluids.WATER, ...)` when the trapdoor is
    /// waterlogged. This server has no fluid queue, so taking one would put a
    /// tick nothing answers into the block queue — and `UNIQUE_TICK_HASH`
    /// would then make it block a real block tick at that position. The two
    /// calls here differ in exactly `is_block`.
    #[test]
    fn a_fluid_tick_is_not_queued_as_a_block_tick() {
        let state = "minecraft:oak_trapdoor[half=bottom,open=true,waterlogged=true]";
        let ticks = ScheduledTicks::new();
        assert!(on_used(
            &ticks,
            (0, 64, 0),
            state,
            crate::placedby::Tick {
                delay: 5,
                priority: PRIORITY_NORMAL,
                is_block: false,
            },
            0,
        ));
        assert_eq!(ticks.len(), 0, "nothing may enter the block queue");
        assert_eq!(ticks.fluid_len(), 1, "it goes in the fluid queue");
        assert!(on_used(
            &ticks,
            (0, 64, 0),
            state,
            crate::placedby::Tick {
                delay: 5,
                priority: PRIORITY_NORMAL,
                is_block: true,
            },
            0,
        ));
        assert_eq!(ticks.len(), 1);
        assert_eq!(ticks.fluid_len(), 1, "the fluid tick is still there");
        assert_eq!(ticks.drain_due(5)[0].block, "minecraft:oak_trapdoor");
        assert_eq!(ticks.drain_due_fluid(5)[0].block, crate::fluid::WATER);
    }

    /// **Every fluid tick at trigger `T` runs after every block tick at `T`**,
    /// and the two sub-tick counters are independent. `ServerLevel.tick`
    /// drains `blockTicks` at offset 269 and `fluidTicks` at 296 under the
    /// one `getGameTime()` of offset 240. The fluid tick is asked for first;
    /// both carry `sub_tick` 0, so nothing but the queue orders them.
    #[test]
    fn the_whole_block_queue_drains_before_the_fluid_queue() {
        let ticks = ScheduledTicks::new();
        assert!(ticks.schedule_fluid((0, 64, 0), crate::fluid::WATER, 0, 5));
        assert!(ticks.schedule((1, 64, 0), "minecraft:repeater", 0, 5, PRIORITY_NORMAL));
        let blocks = ticks.drain_due(5);
        let fluids = ticks.drain_due_fluid(5);
        assert_eq!(blocks.len(), 1);
        assert_eq!(fluids.len(), 1);
        assert_eq!(blocks[0].block, "minecraft:repeater");
        assert_eq!(fluids[0].block, crate::fluid::WATER);
        assert_eq!(fluids[0].sub_tick, 0, "the fluid queue's own counter");
        assert_eq!(blocks[0].sub_tick, 0, "the block queue's own counter");
    }

    /// A source tick and a flowing tick at one position are **two** entries:
    /// `Fluids.WATER` and `Fluids.FLOWING_WATER` are separate instances and
    /// `UNIQUE_TICK_HASH` hashes the instance. A second schedule of the same
    /// one is still dropped.
    #[test]
    fn a_source_tick_and_a_flowing_tick_at_one_position_are_two_entries() {
        let ticks = ScheduledTicks::new();
        assert!(ticks.schedule_fluid((0, 64, 0), crate::fluid::WATER, 0, 5));
        assert!(ticks.schedule_fluid((0, 64, 0), crate::fluid::FLOWING_WATER, 0, 5));
        assert!(!ticks.schedule_fluid((0, 64, 0), crate::fluid::WATER, 0, 5));
        assert_eq!(ticks.fluid_len(), 2);
        // And the block queue's namespace does not meet the fluid queue's:
        // `minecraft:water` is a block id as well as a fluid id.
        assert!(ticks.schedule((0, 64, 0), "minecraft:water", 0, 5, PRIORITY_NORMAL));
    }

    /// **The fluid a use-table tick names is water, measured against the
    /// file.** [`on_used`] schedules `crate::fluid::WATER` for every
    /// `tickKind = "fluid"` row; it may because every one of them carries
    /// `waterlogged=true`, at delay 5 and `NORMAL`.
    #[test]
    fn every_fluid_tick_in_the_use_table_is_waterlogged_water() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/vanilla/use_without_item.json");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{} — run ci/use-golden.sh ({e})", path.display()));
        let doc: serde_json::Value = serde_json::from_str(&text).expect("the table is json");
        let rows: Vec<&serde_json::Value> = doc["table"]
            .as_array()
            .expect("table")
            .iter()
            .flat_map(|b| b["states"].as_array().expect("states"))
            .filter(|c| c.get("tickDelay").is_some())
            .collect();
        let fluid: Vec<&&serde_json::Value> = rows
            .iter()
            .filter(|c| c["tickKind"].as_str() == Some("fluid"))
            .collect();
        assert!(!fluid.is_empty(), "the table names no fluid tick at all");
        for c in &fluid {
            let from = c["from"].as_str().expect("from");
            assert!(
                from.contains("waterlogged=true"),
                "{from} schedules a fluid tick and is not waterlogged"
            );
            assert_eq!(c["tickDelay"].as_i64(), Some(5), "{from}");
            assert_eq!(c["tickPriority"].as_str(), Some("NORMAL"), "{from}");
        }
    }

    /// The rule is keyed on vanilla's **class**, not on the block's name.
    ///
    /// A block called `..._button` that the extract does not call a
    /// `ButtonBlock` gets no un-press, and one that is a `ButtonBlock`
    /// under any name does. This is what stops the rule quietly becoming
    /// "the id ends in `_button`".
    #[test]
    fn the_button_rule_follows_the_class_and_not_the_name() {
        let named = "minecraft:stone_button[face=floor,facing=south,powered=true]";
        let world = Fake::with(&[((0, 64, 0), named)]);
        let ticks = ScheduledTicks::new();
        let tick = crate::placedby::Tick {
            delay: 1,
            priority: PRIORITY_NORMAL,
            is_block: true,
        };
        on_used(&ticks, (0, 64, 0), named, tick, 0);
        let due = ticks.drain_due(1);
        // The extract calls it something else, so no rule applies.
        let wrong = classes(&[("minecraft:stone_button", "LeverBlock")]);
        assert!(run(&due[0], &world, None, Some(&wrong), 0)
            .blocks
            .is_empty());
        assert_eq!(world.state_at((0, 64, 0)), named, "unchanged");
        // No table at all is the same answer, and for the same reason.
        assert!(run(&due[0], &world, None, None, 0).blocks.is_empty());
    }

    /// Water on any of the six sides and it neither schedules nor dies.
    #[test]
    fn a_coral_in_water_is_left_alone() {
        for side in [
            Face::Down,
            Face::Up,
            Face::North,
            Face::South,
            Face::West,
            Face::East,
        ] {
            let world = Fake::with(&[
                ((0, 64, 0), "minecraft:horn_coral_block"),
                (side.relative((0, 64, 0)), "minecraft:water[level=0]"),
            ]);
            let ticks = ScheduledTicks::new();
            assert_eq!(
                on_placed(&ticks, &world, (0, 64, 0), "minecraft:horn_coral_block", 0),
                None,
                "water on the {side:?} side"
            );
            // And a tick that fires anyway finds water and does nothing —
            // the check is in `tick` as well as in `getStateForPlacement`.
            let tick = ScheduledTick {
                pos: (0, 64, 0),
                block: "minecraft:horn_coral_block".to_string(),
                trigger: 0,
                priority: PRIORITY_NORMAL,
                sub_tick: 0,
            };
            assert!(run(&tick, &world, None, None, 0).blocks.is_empty());
            assert_eq!(world.state_at((0, 64, 0)), "minecraft:horn_coral_block");
        }
    }

    /// A tick fires against whatever is at the position *now*. A player who
    /// breaks the coral before it dries out leaves a tick behind, and it must
    /// not turn the block that replaced it into dead coral.
    #[test]
    fn a_tick_whose_block_went_away_does_nothing() {
        let world = Fake::with(&[((0, 64, 0), "minecraft:stone")]);
        let tick = ScheduledTick {
            pos: (0, 64, 0),
            block: "minecraft:fire_coral_block".to_string(),
            trigger: 0,
            priority: PRIORITY_NORMAL,
            sub_tick: 0,
        };
        assert!(run(&tick, &world, None, None, 0).blocks.is_empty());
        assert_eq!(world.state_at((0, 64, 0)), "minecraft:stone");
    }

    /// Only the five. Anything else placed dry schedules nothing, which is
    /// what `placement.json`'s `schedulesTickBlocks` says.
    #[test]
    fn only_coral_schedules_from_a_placement() {
        let world = Fake::with(&[((0, 64, 0), "minecraft:stone")]);
        let ticks = ScheduledTicks::new();
        assert_eq!(
            on_placed(&ticks, &world, (0, 64, 0), "minecraft:stone", 0),
            None
        );
        assert!(ticks.is_empty());
        assert_eq!(CORAL_DEAD.len(), 5);
    }
}

/// `SavedTick` — one entry of a chunk's `block_ticks` or `fluid_ticks` list.
///
/// Six keys, every one of them mandatory: `SavedTick.codec` builds its two
/// groups out of `Codec.fieldOf` and never `optionalFieldOf`, so nothing is
/// omitted at a default value.
///
/// ```text
/// i  String  the block id — `ScheduledTick.type`, the Block and not the state
/// t  Int     the DELAY, not the trigger tick: `toSavedTick(gameTime)` writes
///            `triggerTick - gameTime`, so a tick due in three ticks saves as
///            3 whatever the world clock says. `unpack(gameTime, ...)` adds it
///            back, which is what makes a saved queue survive a restart at a
///            different game time.
/// p  Int     `TickPriority`'s VALUE, not its ordinal. The enum runs
///            EXTREMELY_HIGH..EXTREMELY_LOW at ordinals 0..6 and values
///            -3..3, and `TickPriority.CODEC` is
///            `Codec.INT.xmap(byValue, getValue)` — so NORMAL, ordinal 3, is
///            written as 0. Writing the ordinal would make every saved tick
///            three priority levels too low.
/// x, y, z    Int, absolute block coordinates, flat siblings of the above.
/// ```
#[must_use]
pub fn saved_tick_nbt(tick: &ScheduledTick, game_time: i64) -> rustmc_world::nbt::Nbt {
    use rustmc_world::nbt::Nbt;
    let mut m = std::collections::BTreeMap::new();
    m.insert("i".to_string(), Nbt::String(tick.block.clone()));
    m.insert(
        "t".to_string(),
        Nbt::Int(i32::try_from(tick.trigger - game_time).unwrap_or(i32::MAX)),
    );
    m.insert(
        "p".to_string(),
        Nbt::Int(i32::from(tick.priority) - i32::from(PRIORITY_NORMAL)),
    );
    m.insert("x".to_string(), Nbt::Int(tick.pos.0));
    m.insert("y".to_string(), Nbt::Int(tick.pos.1));
    m.insert("z".to_string(), Nbt::Int(tick.pos.2));
    Nbt::Compound(m)
}

#[cfg(test)]
mod saved_tick_tests {
    use super::*;
    use rustmc_world::nbt::Nbt;

    /// The two conversions that are not identities, pinned separately because
    /// each has a plausible wrong answer that produces a file vanilla reads.
    #[test]
    fn a_saved_tick_carries_a_delay_and_a_priority_value() {
        let tick = ScheduledTick {
            pos: (10, 64, -3),
            block: "minecraft:water".into(),
            trigger: 1005,
            priority: PRIORITY_NORMAL,
            sub_tick: 0,
        };
        let nbt = saved_tick_nbt(&tick, 1000);
        assert_eq!(nbt.get("t"), Some(&Nbt::Int(5)), "a delay, not the trigger");
        assert_eq!(
            nbt.get("p"),
            Some(&Nbt::Int(0)),
            "NORMAL is ordinal 3 and value 0"
        );
        assert_eq!(nbt.get("i"), Some(&Nbt::String("minecraft:water".into())));
        assert_eq!(nbt.get("x"), Some(&Nbt::Int(10)));
        assert_eq!(nbt.get("y"), Some(&Nbt::Int(64)));
        assert_eq!(nbt.get("z"), Some(&Nbt::Int(-3)));
        assert_eq!(
            nbt.as_compound().map(std::collections::BTreeMap::len),
            Some(6),
            "six mandatory keys, none optional"
        );
    }

    /// The extremes of the ordinal-to-value map, so a sign error shows.
    #[test]
    fn the_priority_extremes_map_to_minus_three_and_three() {
        let at = |priority| {
            let tick = ScheduledTick {
                pos: (0, 0, 0),
                block: "minecraft:stone".into(),
                trigger: 0,
                priority,
                sub_tick: 0,
            };
            saved_tick_nbt(&tick, 0).get("p").cloned()
        };
        assert_eq!(at(0), Some(Nbt::Int(-3)), "EXTREMELY_HIGH");
        assert_eq!(at(6), Some(Nbt::Int(3)), "EXTREMELY_LOW");
    }

    /// A tick already overdue saves as a negative delay rather than being
    /// clamped: `unpack` adds `gameTime` back and a clamp would move it.
    #[test]
    fn an_overdue_tick_saves_a_negative_delay() {
        let tick = ScheduledTick {
            pos: (0, 0, 0),
            block: "minecraft:stone".into(),
            trigger: 90,
            priority: PRIORITY_NORMAL,
            sub_tick: 0,
        };
        assert_eq!(saved_tick_nbt(&tick, 100).get("t"), Some(&Nbt::Int(-10)));
    }

    /// `pending_in_chunk` does not drain, and it selects on the chunk the
    /// position is in — floor-shifted, so a negative x is not chunk 0.
    #[test]
    fn pending_in_chunk_selects_and_does_not_drain() {
        let queue = ScheduledTicks::new();
        assert!(queue.schedule((5, 64, 5), "minecraft:water", 0, 5, PRIORITY_NORMAL));
        assert!(queue.schedule((-1, 64, 5), "minecraft:water", 0, 5, PRIORITY_NORMAL));
        assert_eq!(queue.pending_in_chunk(0, 0).len(), 1);
        assert_eq!(queue.pending_in_chunk(-1, 0).len(), 1, "-1 >> 4 is -1");
        assert_eq!(queue.len(), 2, "peeking must not drain");
    }
}

#[cfg(test)]
mod this_tick_tests {
    use super::*;

    /// `toRunThisTickSet` holds the collected-not-yet-run set and nothing
    /// else: before the drain a scheduled tick is *pending* (visible to
    /// `hasScheduledTick`), after the drain it *will tick this tick*, and
    /// once marked run it is neither.
    #[test]
    fn will_tick_this_tick_is_collected_minus_run() {
        let q = ScheduledTicks::new();
        let a = (1, 0, 0);
        let b = (2, 0, 0);
        assert!(q.schedule(a, "minecraft:repeater", 10, 2, PRIORITY_NORMAL));
        assert!(q.schedule(b, "minecraft:comparator", 10, 2, PRIORITY_NORMAL));
        assert!(q.has_scheduled_tick(a, "minecraft:repeater"));
        assert!(!q.will_tick_this_tick(a, "minecraft:repeater"));

        let due = q.drain_due(12);
        assert_eq!(due.len(), 2);
        assert!(!q.has_scheduled_tick(a, "minecraft:repeater"));
        assert!(q.will_tick_this_tick(a, "minecraft:repeater"));
        assert!(q.will_tick_this_tick(b, "minecraft:comparator"));
        assert!(
            !q.will_tick_this_tick(a, "minecraft:comparator"),
            "keyed on (pos, block), the UNIQUE_TICK_HASH pair"
        );

        q.mark_run(&due[0]);
        assert!(!q.will_tick_this_tick(due[0].pos, &due[0].block));
        assert!(q.will_tick_this_tick(due[1].pos, &due[1].block));
        // Rescheduling a running tick is allowed again: `poll` removed it
        // from `ticksPerPosition`.
        assert!(q.schedule(due[0].pos, &due[0].block, 12, 2, PRIORITY_NORMAL));
    }
}
