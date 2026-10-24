//! `ComparatorBlock` and the `ComparatorBlockEntity` it keeps its output in.
//!
//! # Why this is not the repeater with a different delay
//!
//! `ComparatorBlock extends DiodeBlock` and overrides **five** of the methods
//! `DiodeBlock.neighborChanged` reaches — `getInputSignal`, `shouldTurnOn`,
//! `checkTickOnNeighbor`, `tick` and `getOutputSignal` — plus `updateShape`
//! and `useWithoutItem`. `crate::power`'s diode rule serves the repeater and
//! refuses the comparator by name; this module is the comparator's own
//! transcription. Every body below is read from the pinned jar's
//! `ComparatorBlock` and `DiodeBlock`, offsets in the doc of each function.
//!
//! # The block entity is a single `int`, and it lives here
//!
//! `ComparatorBlockEntity` holds `output` and nothing else (`<init>` writes
//! `iconst_0` to it; `getOutputSignal`/`setOutputSignal` are its whole API).
//! It is never sent to a client — no update packet, no update tag — so the
//! wire cannot see where it is kept. It is kept in [`Outputs`] here, keyed by
//! position, rather than in `crate::blockentity`, because that file belongs
//! to another lane and a comparator's output is a redstone value. When the
//! block-entity registry grows a comparator kind, the map moves and nothing
//! else changes.
//!
//! # The two signals a comparator reads
//!
//! ```text
//! getInputSignal (ComparatorBlock, offsets 1-168):
//!     i     = DiodeBlock.getInputSignal(level, pos, state)      the block behind, as any diode reads it
//!     back  = pos.relative(FACING)                              FACING points at the INPUT side
//!     s1    = getBlockState(back)
//!     if (s1.hasAnalogOutputSignal())        return s1.getAnalogOutputSignal(level, back, FACING.opposite)
//!     if (i < 15 && s1.isRedstoneConductor(level, back)) {
//!         back2 = back.relative(FACING); s2 = getBlockState(back2)
//!         j = itemFrameAt(back2) ? frame.getAnalogOutput() : MIN_VALUE
//!         k = s2.hasAnalogOutputSignal() ? s2.getAnalogOutputSignal(level, back2, FACING.opposite) : MIN_VALUE
//!         l = max(j, k); if (l != MIN_VALUE) i = l
//!     }
//!     return i
//!
//! getAlternateSignal (DiodeBlock, offsets 1-67):
//!     max(getControlInputSignal(pos.relative(FACING.clockwise), FACING.clockwise, sideInputDiodesOnly),
//!         getControlInputSignal(pos.relative(FACING.counterClockwise), ..., sideInputDiodesOnly))
//!     sideInputDiodesOnly() is FALSE for the comparator (DiodeBlock's default; only RepeaterBlock says true)
//!
//! SignalGetter.getControlInputSignal (offsets 2-92):
//!     s = getBlockState(pos)
//!     if (diodesOnly)            return DiodeBlock.isDiode(s) ? getDirectSignal(pos, dir) : 0
//!     if (s.is(REDSTONE_BLOCK))  return 15
//!     if (s.is(REDSTONE_WIRE))   return s.POWER
//!     return s.isSignalSource() ? getDirectSignal(pos, dir) : 0
//! ```
//!
//! **No item frames.** `getItemFrame` is `getEntitiesOfClass(ItemFrame, ...)`
//! and there is no item-frame entity on this server; the frame's arm is the
//! `MIN_VALUE` branch, which is what vanilla takes when no frame hangs there.
//! Named here so the first item frame does not read as a comparator bug.
//!
//! # Analog outputs: which blocks, and which are refused
//!
//! `hasAnalogOutputSignal` is overridden — always to `true` — by exactly the
//! 28 block classes in [`ANALOG_CLASSES`], found by `javap` over every class
//! under `net/minecraft/world/level/block/` in the pinned jar. Membership is
//! tested by class through `set_placed_by.json`'s class table, so a
//! `trapped_chest` (a `ChestBlock`) and a `dropper` (a `DispenserBlock`)
//! answer without being listed. The **values** split four ways:
//!
//! * read off the state — cake, candle cake, beehive, composter, copper bulb,
//!   copper golem statue, end portal frame, the cauldrons, respawn anchor —
//!   ported whole;
//! * `AbstractContainerMenu.getRedstoneSignalFromContainer` over a container
//!   this server keeps — chest (single and double), the furnace family, the
//!   barrel, hopper, dispenser, dropper, shulker box and brewing stand —
//!   ported whole, with `Container.getMaxStackSize(stack)` being
//!   `min(99, stack.getMaxStackSize())`;
//! * the crafter, `CrafterBlockEntity.getRedstoneSignal` — the count of
//!   slots that are empty or disabled — since branch `be-hopper-dispense`;
//! * everything else whose value sits in a block entity this server does not
//!   model (jukebox, lectern, command block, chiseled bookshelf,
//!   decorated pot, sculk sensor, creaking heart, shelf, detector rail) —
//!   **refused**, once per state on stderr, answering 0. That is what an
//!   empty one answers too, so an idle one is right by accident and a busy
//!   one is loudly wrong.
//!
//! # When a comparator hears a container
//!
//! `BlockEntity.setChanged(level, pos, state)` ends in
//! `Level.updateNeighbourForOutputSignal(pos, block)` (offsets 5-18), so a
//! container's contents reach a comparator two ways besides a block write: a
//! block-entity ticker that marks itself (`crate::behaviour`'s walk — the
//! hopper moving a stack, a furnace burning one) and a menu click that
//! writes into the container (`crate::world_send::handle_container_click`).
//! Without them a comparator read a chest only when something *else* made
//! it look, and stayed dark beside one a hopper had just filled.

use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::edits::Blocks;
use crate::interact::{BlockPos, Face};
use crate::placedby::PlacedByTable;
use crate::power::{Redstone, SignalTable};
use crate::shapes::{Effects, ShapeTable, TickRequest, RECURSION_LIMIT, UPDATE_CLIENTS};

/// `ComparatorBlock.getDelay`: `iconst_2`.
pub const DELAY: i64 = 2;

#[must_use]
pub fn block_of(state: &str) -> &str {
    state.split('[').next().unwrap_or(state)
}

#[must_use]
pub fn is_comparator(state: &str) -> bool {
    block_of(state) == "minecraft:comparator"
}

#[must_use]
fn facing_of(state: &str) -> Option<Face> {
    crate::placement::property_of(state, "facing").and_then(Face::from_name)
}

#[must_use]
fn is_subtract(state: &str) -> bool {
    crate::placement::property_of(state, "mode") == Some("subtract")
}

#[must_use]
fn is_powered(state: &str) -> bool {
    crate::placement::property_of(state, "powered") == Some("true")
}

fn shared_classes() -> Option<&'static PlacedByTable> {
    PlacedByTable::shared(std::path::Path::new("data/vanilla/set_placed_by.json")).ok()
}

// ---------------------------------------------------------------------------
// ComparatorBlockEntity.output
// ---------------------------------------------------------------------------

/// The `output` field of every `ComparatorBlockEntity` in the world.
#[derive(Debug, Default)]
pub struct Outputs {
    inner: Mutex<BTreeMap<BlockPos, i32>>,
}

impl Outputs {
    pub fn shared() -> &'static Self {
        static OUTPUTS: std::sync::OnceLock<Outputs> = std::sync::OnceLock::new();
        OUTPUTS.get_or_init(Self::default)
    }

    /// `ComparatorBlock.getOutputSignal`: the entity's `output`, or 0 when
    /// there is no `ComparatorBlockEntity` at the position.
    #[must_use]
    pub fn get(&self, pos: BlockPos) -> i32 {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&pos)
            .copied()
            .unwrap_or(0)
    }

    /// `ComparatorBlockEntity.setOutputSignal`.
    pub fn set(&self, pos: BlockPos, value: i32) {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(pos, value);
    }

    /// `LevelChunk.removeBlockEntity` — the comparator left, its entity with
    /// it. Called from the block-changed hook so a comparator placed again
    /// at the same position starts from `iconst_0`.
    pub fn remove(&self, pos: BlockPos) {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&pos);
    }
}

// ---------------------------------------------------------------------------
// Analog output signals
// ---------------------------------------------------------------------------

/// Every class in the pinned jar that overrides `hasAnalogOutputSignal`,
/// each returning `iconst_1`. Subclasses inherit through
/// [`PlacedByTable::is_a`].
pub const ANALOG_CLASSES: [&str; 28] = [
    "AbstractCauldronBlock",
    "AbstractFurnaceBlock",
    "BarrelBlock",
    "BeehiveBlock",
    "BrewingStandBlock",
    "CakeBlock",
    "CandleCakeBlock",
    "ChestBlock",
    "ChiseledBookShelfBlock",
    "CommandBlock",
    "ComposterBlock",
    "CopperBulbBlock",
    "CopperGolemStatueBlock",
    "CrafterBlock",
    "CreakingHeartBlock",
    "DecoratedPotBlock",
    "DetectorRailBlock",
    "DispenserBlock",
    "EndPortalFrameBlock",
    "HopperBlock",
    "JukeboxBlock",
    "LavaCauldronBlock",
    "LayeredCauldronBlock",
    "LecternBlock",
    "RespawnAnchorBlock",
    "SculkSensorBlock",
    "ShelfBlock",
    "ShulkerBoxBlock",
];

/// `BlockState.hasAnalogOutputSignal()`.
#[must_use]
pub fn has_analog_output_signal(classes: Option<&PlacedByTable>, state: &str) -> bool {
    let Some(classes) = classes else {
        return false;
    };
    let block = block_of(state);
    ANALOG_CLASSES.iter().any(|c| classes.is_a(block, c))
}

fn int_property(state: &str, name: &str) -> i32 {
    crate::placement::property_of(state, name)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

/// `Mth.lerpDiscrete(f, min, max)`, offsets 2-27:
/// `min + floor(f * (max - min - 1)) + (f > 0 ? 1 : 0)`.
#[must_use]
pub fn lerp_discrete(f: f32, min: i32, max: i32) -> i32 {
    let k = max - min;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "Mth.floor is (int) of a float already rounded down"
    )]
    let floored = (f * (k - 1) as f32).floor() as i32;
    min + floored + i32::from(f > 0.0)
}

/// `Container.getMaxStackSize()` — `bipush 99`, the interface default that
/// none of the chest, furnace or base container entities override.
pub const CONTAINER_MAX_STACK_SIZE: i32 = 99;

/// `AbstractContainerMenu.getRedstoneSignalFromContainer`, offsets 1-75,
/// over the slots of one or more block entities read as one container.
#[must_use]
pub fn redstone_signal_from_slots(slots: &[crate::item::ItemStack]) -> i32 {
    if slots.is_empty() {
        return 0;
    }
    let mut f = 0.0f32;
    for stack in slots {
        if !stack.is_empty() {
            #[expect(
                clippy::cast_precision_loss,
                reason = "vanilla's own i2f on a count and a stack size"
            )]
            let ratio =
                stack.count() as f32 / CONTAINER_MAX_STACK_SIZE.min(stack.max_stack_size()) as f32;
            f += ratio;
        }
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "vanilla's own i2f on the container size"
    )]
    let size = slots.len() as f32;
    lerp_discrete(f / size, 0, 15)
}

/// Say once, per state, that a comparator read an analog output this server
/// does not hold, and answered 0 for it.
fn analog_refused_once(state: &str, why: &str) {
    static SAID: std::sync::OnceLock<Mutex<std::collections::BTreeSet<String>>> =
        std::sync::OnceLock::new();
    let said = SAID.get_or_init(|| Mutex::new(std::collections::BTreeSet::new()));
    let mut said = said
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if said.insert(state.to_string()) {
        eprintln!("comparator: getAnalogOutputSignal({state}) is not ported — {why}; answering 0");
    }
}

/// `ChestBlock.getConnectedDirection`: `LEFT` looks clockwise from the
/// chest's facing, everything else counter-clockwise.
#[must_use]
fn chest_connected_direction(state: &str) -> Option<Face> {
    let facing = facing_of(state)?;
    if crate::placement::property_of(state, "type") == Some("left") {
        facing.clockwise()
    } else {
        facing.counter_clockwise()
    }
}

/// The container slots behind a chest state: one entity for `type=single`,
/// the `CompoundContainer` of both halves otherwise (`ChestBlock.getContainer`
/// with `bl = false`, which does not refuse a blocked or cat-occupied chest —
/// that flag is the menu path's).
fn chest_slots(blocks: &dyn Blocks, pos: BlockPos, state: &str) -> Vec<crate::item::ItemStack> {
    let entities = crate::blockentity::BlockEntities::shared();
    let own: Vec<_> = entities
        .get(pos)
        .map(|be| {
            (0..be.kind().container_size())
                .map(|i| be.item(i))
                .collect()
        })
        .unwrap_or_default();
    if crate::placement::property_of(state, "type") == Some("single") {
        return own;
    }
    let Some(dir) = chest_connected_direction(state) else {
        return own;
    };
    let other_pos = dir.relative(pos);
    let other_state = blocks.state_at(other_pos);
    if block_of(&other_state) != block_of(state) {
        return own;
    }
    let other: Vec<_> = entities
        .get(other_pos)
        .map(|be| {
            (0..be.kind().container_size())
                .map(|i| be.item(i))
                .collect()
        })
        .unwrap_or_default();
    // `CompoundContainer(first, second)`: `ChestBlock.getBlockType` makes the
    // RIGHT half `FIRST`, and `DoubleBlockCombiner.combineWithNeigbour`
    // (offsets 219-255) passes FIRST's entity first whichever half asked.
    // This read the other way round until branch `be-hopper-dispense`; the
    // sum does not care, a per-slot reader would.
    if crate::placement::property_of(state, "type") == Some("right") {
        [own, other].concat()
    } else {
        [other, own].concat()
    }
}

/// The six classes whose `getAnalogOutputSignal` is
/// `AbstractContainerMenu.getRedstoneSignalFromBlockEntity(level
/// .getBlockEntity(pos))` and nothing else — offsets 0-8 of each, read off
/// the pinned jar. `DropperBlock` extends `DispenserBlock` and the smoker
/// and blast furnace extend `AbstractFurnaceBlock`, so they answer through
/// [`PlacedByTable::is_a`] without being listed. `ChestBlock` is not one of
/// them: it goes through `ChestBlock.getContainer` so a double chest reads
/// both halves.
pub const FROM_BLOCK_ENTITY: [&str; 6] = [
    "AbstractFurnaceBlock",
    "BarrelBlock",
    "BrewingStandBlock",
    "DispenserBlock",
    "HopperBlock",
    "ShulkerBoxBlock",
];

/// `AbstractContainerMenu.getRedstoneSignalFromBlockEntity`, offsets 0-18:
/// `be instanceof Container c ? getRedstoneSignalFromContainer(c) : 0`.
///
/// The entity is whatever stands at the position, not the one the state
/// implies — vanilla asks `level.getBlockEntity(pos)` — and "is a
/// `Container`" is `container_size() > 0` here, the same test
/// `crate::behaviour`'s hopper uses for `getContainerAt`.
#[must_use]
pub fn redstone_signal_from_block_entity(pos: BlockPos) -> i32 {
    let slots: Vec<_> = crate::blockentity::BlockEntities::shared()
        .get(pos)
        .map(|be| {
            (0..be.kind().container_size())
                .map(|i| be.item(i))
                .collect()
        })
        .unwrap_or_default();
    redstone_signal_from_slots(&slots)
}

/// `BlockState.getAnalogOutputSignal(level, pos, dir)` for a state that
/// answered [`has_analog_output_signal`].
#[must_use]
pub fn analog_output_signal(
    blocks: &dyn Blocks,
    classes: &PlacedByTable,
    pos: BlockPos,
    state: &str,
) -> i32 {
    let block = block_of(state);
    let is = |class: &str| classes.is_a(block, class);
    if is("CakeBlock") {
        // `CakeBlock.getOutputSignal(bites)`: `(7 - bites) * 2`.
        return (7 - int_property(state, "bites")) * 2;
    }
    if is("CandleCakeBlock") {
        // `CakeBlock.FULL_CAKE_SIGNAL` = `getOutputSignal(0)`.
        return 14;
    }
    if is("BeehiveBlock") {
        return int_property(state, "honey_level");
    }
    if is("ComposterBlock") {
        return int_property(state, "level");
    }
    if is("CopperBulbBlock") {
        return if crate::placement::property_of(state, "lit") == Some("true") {
            15
        } else {
            0
        };
    }
    if is("CopperGolemStatueBlock") {
        // `POSE.ordinal() + 1`; the enum is STANDING, SITTING, RUNNING, STAR.
        return match crate::placement::property_of(state, "pose") {
            Some("standing") => 1,
            Some("sitting") => 2,
            Some("running") => 3,
            Some("star") => 4,
            _ => 1,
        };
    }
    if is("EndPortalFrameBlock") {
        return if crate::placement::property_of(state, "eye") == Some("true") {
            15
        } else {
            0
        };
    }
    if is("LavaCauldronBlock") {
        return 3;
    }
    if is("LayeredCauldronBlock") {
        return int_property(state, "level");
    }
    if is("AbstractCauldronBlock") {
        // `CauldronBlock` does not override; `Block.getAnalogOutputSignal`
        // is `iconst_0`.
        return 0;
    }
    if is("RespawnAnchorBlock") {
        // `getScaledChargeLevel(state, 15)`: `floor((charge - 0) / 4.0f * 15)`.
        #[expect(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            reason = "vanilla's own float arithmetic on a 0..4 property"
        )]
        let scaled = ((int_property(state, "charges") as f32) / 4.0 * 15.0).floor() as i32;
        return scaled;
    }
    if is("ChestBlock") {
        return redstone_signal_from_slots(&chest_slots(blocks, pos, state));
    }
    if FROM_BLOCK_ENTITY.iter().any(|c| is(c)) {
        return redstone_signal_from_block_entity(pos);
    }
    if is("CrafterBlock") {
        // `be instanceof CrafterBlockEntity ? getRedstoneSignal() : 0` —
        // the count of slots that are empty or disabled, not a fill
        // fraction. The slots are the block entity's own; the disabled
        // flags are its `containerData`.
        let Some(be) = crate::blockentity::BlockEntities::shared().get(pos) else {
            return 0;
        };
        let Some(crate::behaviour::Behaviour::Crafter(c)) = be.behaviour() else {
            return 0;
        };
        let mut c = c.clone();
        c.items = (0..crate::crafter::CRAFTER_SLOTS)
            .map(|i| be.item(i))
            .collect();
        return c.redstone_signal();
    }
    analog_refused_once(
        state,
        "its value sits in a block entity or container this server does not keep",
    );
    0
}

// ---------------------------------------------------------------------------
// The signals
// ---------------------------------------------------------------------------

/// `DiodeBlock.getSignal(state, level, pos, dir)` (offsets 1-23) for a
/// comparator: `FACING == dir ? ownSignal : 0`, where `ownSignal` (offsets
/// 1-27) is `POWERED ? getOutputSignal(level, pos, state) : 0` and
/// `ComparatorBlock.getOutputSignal` is the block entity's `output`.
/// `getDirectSignal` is the same call. `signal.json` refuses every
/// comparator state for exactly this reason — "getSignal read a block
/// entity" — so `crate::power::Redstone` asks here instead of the table.
/// Without this the comparator turned `powered=true` and the dust in front
/// of it stayed dark: the first recording of `455`-`457` diverged at that.
#[must_use]
pub fn own_signal(pos: BlockPos, state: &str, dir: Face) -> u8 {
    if facing_of(state) != Some(dir) || !is_powered(state) {
        return 0;
    }
    u8::try_from(Outputs::shared().get(pos)).unwrap_or(0)
}

/// `SignalGetter.getControlInputSignal(pos, dir, diodesOnly)`.
#[must_use]
pub fn control_input_signal(
    blocks: &dyn Blocks,
    power: &Redstone<'_>,
    pos: BlockPos,
    dir: Face,
    diodes_only: bool,
) -> i32 {
    let state = blocks.state_at(pos);
    let block = block_of(&state);
    if diodes_only {
        return if matches!(block, "minecraft:repeater" | "minecraft:comparator") {
            i32::from(power.get_direct_signal(pos, dir))
        } else {
            0
        };
    }
    if block == "minecraft:redstone_block" {
        return 15;
    }
    if crate::wire::is_wire(&state) {
        return int_property(&state, "power");
    }
    if blocks.signal_source(pos) {
        return i32::from(power.get_direct_signal(pos, dir));
    }
    0
}

/// `DiodeBlock.getAlternateSignal` with `sideInputDiodesOnly() == false`.
#[must_use]
pub fn alternate_signal(
    blocks: &dyn Blocks,
    power: &Redstone<'_>,
    pos: BlockPos,
    state: &str,
) -> i32 {
    let Some(facing) = facing_of(state) else {
        return 0;
    };
    let (Some(cw), Some(ccw)) = (facing.clockwise(), facing.counter_clockwise()) else {
        return 0;
    };
    let a = control_input_signal(blocks, power, cw.relative(pos), cw, false);
    let b = control_input_signal(blocks, power, ccw.relative(pos), ccw, false);
    a.max(b)
}

/// `ComparatorBlock.getInputSignal`, offsets 1-168 (transcribed in the
/// module doc).
#[must_use]
pub fn input_signal(
    blocks: &dyn Blocks,
    power: &Redstone<'_>,
    classes: Option<&PlacedByTable>,
    pos: BlockPos,
    state: &str,
) -> i32 {
    let mut i = i32::from(crate::power::diode_input_signal(blocks, power, pos, state));
    let Some(facing) = facing_of(state) else {
        return i;
    };
    let back = facing.relative(pos);
    let s1 = blocks.state_at(back);
    if has_analog_output_signal(classes, &s1) {
        if let Some(classes) = classes {
            return analog_output_signal(blocks, classes, back, &s1);
        }
    }
    if i < 15 && blocks.redstone_conductor(back) {
        let back2 = facing.relative(back);
        let s2 = blocks.state_at(back2);
        // `j` is the item frame's output and there is no item frame here.
        let j = i32::MIN;
        let k = if has_analog_output_signal(classes, &s2) {
            classes.map_or(i32::MIN, |c| analog_output_signal(blocks, c, back2, &s2))
        } else {
            i32::MIN
        };
        let l = j.max(k);
        if l != i32::MIN {
            i = l;
        }
    }
    i
}

/// `ComparatorBlock.calculateOutputSignal`, offsets 4-55.
#[must_use]
pub fn calculate_output_signal(
    blocks: &dyn Blocks,
    power: &Redstone<'_>,
    classes: Option<&PlacedByTable>,
    pos: BlockPos,
    state: &str,
) -> i32 {
    let i = input_signal(blocks, power, classes, pos, state);
    if i == 0 {
        return 0;
    }
    let j = alternate_signal(blocks, power, pos, state);
    if j > i {
        return 0;
    }
    if is_subtract(state) {
        i - j
    } else {
        i
    }
}

/// `ComparatorBlock.shouldTurnOn`, offsets 4-59.
#[must_use]
pub fn should_turn_on(
    blocks: &dyn Blocks,
    power: &Redstone<'_>,
    classes: Option<&PlacedByTable>,
    pos: BlockPos,
    state: &str,
) -> bool {
    let i = input_signal(blocks, power, classes, pos, state);
    if i == 0 {
        return false;
    }
    let j = alternate_signal(blocks, power, pos, state);
    i > j || (i == j && !is_subtract(state))
}

// ---------------------------------------------------------------------------
// The hooks
// ---------------------------------------------------------------------------

/// `DiodeBlock.neighborChanged` (offsets 2-29) into
/// `ComparatorBlock.checkTickOnNeighbor` (offsets 1-117).
///
/// The `canSurvive` false branch — `dropResources`, `removeBlock`, six
/// `updateNeighborsAt` — is not taken here: `ComparatorBlock.updateShape`
/// answers the same loss of support through `update_shape.json`'s `dir=DOWN
/// -> air` row, and this server has nothing to drop into. That is the same
/// choice the repeater made and the same gap.
pub fn neighbor_changed(
    blocks: &dyn Blocks,
    signals: &SignalTable,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    state: &str,
    updates: &mut Effects,
) {
    if !is_comparator(&blocks.state_at(pos)) {
        return;
    }
    // `willTickThisTick` first — before any signal is read.
    if crate::ticks::ScheduledTicks::shared().will_tick_this_tick(pos, block_of(state)) {
        return;
    }
    let classes = shared_classes();
    let power = Redstone::new(blocks, signals, shapes);
    let i = calculate_output_signal(blocks, &power, classes, pos, state);
    let j = Outputs::shared().get(pos);
    if i != j || is_powered(state) != should_turn_on(blocks, &power, classes, pos, state) {
        let priority = if crate::power::diode_should_prioritize(blocks, pos, state) {
            crate::power::PRIORITY_HIGH
        } else {
            crate::ticks::PRIORITY_NORMAL
        };
        updates.ticks.push(TickRequest {
            pos,
            block: block_of(state).to_string(),
            delay: DELAY,
            priority,
        });
    }
}

/// `ComparatorBlock.refreshOutputState`, offsets 4-164 — also the whole of
/// `ComparatorBlock.tick`.
///
/// ```java
/// int i = calculateOutputSignal(level, pos, state);
/// BlockEntity be = level.getBlockEntity(pos);
/// int j = be instanceof ComparatorBlockEntity c ? c.getOutputSignal() : 0;
/// if (be instanceof ComparatorBlockEntity c) c.setOutputSignal(i);
/// if (j != i || state.getValue(MODE) == ComparatorMode.COMPARE) {
///     boolean on = shouldTurnOn(level, pos, state);
///     boolean powered = state.getValue(POWERED);
///     if (powered && !on)       level.setBlock(pos, state.setValue(POWERED, false), 2);
///     else if (!powered && on)  level.setBlock(pos, state.setValue(POWERED, true), 2);
///     updateNeighborsInFront(level, pos, state);
/// }
/// ```
pub fn refresh_output_state(
    blocks: &dyn Blocks,
    signals: &SignalTable,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    state: &str,
    updates: &mut Effects,
) {
    let classes = shared_classes();
    let power = Redstone::new(blocks, signals, shapes);
    let i = calculate_output_signal(blocks, &power, classes, pos, state);
    let outputs = Outputs::shared();
    let j = outputs.get(pos);
    outputs.set(pos, i);
    if j != i || !is_subtract(state) {
        let on = should_turn_on(blocks, &power, classes, pos, state);
        let powered = is_powered(state);
        if powered && !on {
            let to = crate::placement::with_property(state, "powered", "false");
            crate::shapes::set_block(
                blocks,
                shapes,
                pos,
                &to,
                UPDATE_CLIENTS,
                RECURSION_LIMIT,
                updates,
            );
        } else if !powered && on {
            let to = crate::placement::with_property(state, "powered", "true");
            crate::shapes::set_block(
                blocks,
                shapes,
                pos,
                &to,
                UPDATE_CLIENTS,
                RECURSION_LIMIT,
                updates,
            );
        }
        crate::power::diode_update_neighbors_in_front(blocks, signals, shapes, pos, state, updates);
    }
}

/// `ComparatorBlock.tick`: `refreshOutputState` and nothing else.
pub fn tick(
    blocks: &dyn Blocks,
    signals: &SignalTable,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    state: &str,
    updates: &mut Effects,
) {
    refresh_output_state(blocks, signals, shapes, pos, state, updates);
}

/// The `onPlace` half of a comparator write, from inside
/// [`crate::shapes::set_block`].
///
/// `DiodeBlock.onPlace` is `updateNeighborsInFront` with no guard. On top of
/// it, `ComparatorBlock.useWithoutItem` (offsets 16-92) cycles `MODE`,
/// writes it with flags 2 and then calls `refreshOutputState` if the block
/// is still a comparator. `MODE` has exactly one writer in the jar — that
/// method — so a write whose old state is a comparator with a different
/// `mode` *is* the click, and the refresh rides here rather than in the
/// use path, which is a file another lane is rewriting. A `/setblock` that
/// changed the mode would refresh where vanilla's would not; no scenario
/// drives one and the caveat is written down here.
pub fn on_place(
    blocks: &dyn Blocks,
    signals: &SignalTable,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    state: &str,
    old: &str,
    updates: &mut Effects,
) {
    crate::power::diode_update_neighbors_in_front(blocks, signals, shapes, pos, state, updates);
    if is_comparator(old)
        && crate::placement::property_of(old, "mode")
            != crate::placement::property_of(state, "mode")
        && is_comparator(&blocks.state_at(pos))
    {
        refresh_output_state(blocks, signals, shapes, pos, state, updates);
    }
}

/// `Level.updateNeighbourForOutputSignal(pos, block)`, offsets 0-130: for
/// each of `Direction.Plane.HORIZONTAL` (NORTH, EAST, SOUTH, WEST), the
/// neighbour if it is a comparator, else the block beyond it if the
/// neighbour is a conductor and that block is a comparator, gets
/// `neighborChanged(pos, block, null, false)` — the comparator's own body,
/// not a `NeighborUpdater` round.
pub fn update_neighbour_for_output_signal(
    blocks: &dyn Blocks,
    signals: &SignalTable,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    updates: &mut Effects,
) {
    for d in crate::wire::HORIZONTAL {
        let p2 = d.relative(pos);
        let s2 = blocks.state_at(p2);
        if is_comparator(&s2) {
            neighbor_changed(blocks, signals, shapes, p2, &s2, updates);
        } else if blocks.redstone_conductor(p2) {
            let p3 = d.relative(p2);
            let s3 = blocks.state_at(p3);
            if is_comparator(&s3) {
                neighbor_changed(blocks, signals, shapes, p3, &s3, updates);
            }
        }
    }
}

/// `DiodeBlock.setPlacedBy`, offsets 4-17: `if (shouldTurnOn(...))
/// level.scheduleTick(pos, this, 1)` — for both diodes, each with its own
/// `shouldTurnOn`. Returns whether the tick was asked for.
#[must_use]
pub fn diode_set_placed_by(
    blocks: &dyn Blocks,
    signals: &SignalTable,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    state: &str,
) -> bool {
    let power = Redstone::new(blocks, signals, shapes);
    if is_comparator(state) {
        should_turn_on(blocks, &power, shared_classes(), pos, state)
    } else if block_of(state) == "minecraft:repeater" {
        crate::power::diode_should_turn_on(blocks, &power, pos, state)
    } else {
        false
    }
}

/// `DiodeBlock.setPlacedBy`'s delay: `iconst_1`.
pub const SET_PLACED_BY_DELAY: i64 = 1;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap as Map;

    struct World {
        states: Mutex<Map<BlockPos, String>>,
    }

    impl World {
        fn new(initial: &[(BlockPos, &str)]) -> Self {
            Self {
                states: Mutex::new(
                    initial
                        .iter()
                        .map(|(p, s)| (*p, (*s).to_string()))
                        .collect(),
                ),
            }
        }
        fn at(&self, pos: BlockPos) -> String {
            self.states
                .lock()
                .unwrap()
                .get(&pos)
                .cloned()
                .unwrap_or_else(|| "minecraft:air".to_string())
        }
    }

    impl Blocks for World {
        fn state_at(&self, pos: BlockPos) -> String {
            self.at(pos)
        }
        fn set_block(&self, pos: BlockPos, state: &str) -> Option<i32> {
            let mut map = self.states.lock().unwrap();
            if map.get(&pos).map(String::as_str) == Some(state) {
                return None;
            }
            map.insert(pos, state.to_string());
            Some(1)
        }
        fn default_state(&self, block: &str) -> Option<String> {
            Some(block.to_string())
        }
        fn has_state(&self, _state: &str) -> bool {
            true
        }
        fn state_id(&self, _state: &str) -> Option<i32> {
            Some(1)
        }
        fn replaceable(&self, pos: BlockPos, _: &crate::placement::Replacing<'_>) -> bool {
            crate::shapes::is_air(&self.at(pos))
        }
        fn face_sturdy(&self, pos: BlockPos, _face: Face) -> bool {
            block_of(&self.at(pos)) == "minecraft:stone"
        }
        fn redstone_conductor(&self, pos: BlockPos) -> bool {
            block_of(&self.at(pos)) == "minecraft:stone"
        }
        fn signal_source(&self, pos: BlockPos) -> bool {
            matches!(
                block_of(&self.at(pos)),
                "minecraft:redstone_block"
                    | "minecraft:redstone_torch"
                    | "minecraft:redstone_wire"
                    | "minecraft:repeater"
                    | "minecraft:comparator"
            )
        }
        fn water_at(&self, _pos: BlockPos) -> bool {
            false
        }
        fn legacy_fluid_at(&self, _pos: BlockPos) -> String {
            "minecraft:air".to_string()
        }
    }

    fn table() -> SignalTable {
        let json = r#"{
          "rng": "signal", "states": 5, "quiet": 1, "sources": 4, "refusedStates": 0,
          "directionOrder": ["DOWN", "UP", "NORTH", "SOUTH", "WEST", "EAST"],
          "refused": [], "unanswerable": [],
          "table": [
            {"state": "minecraft:redstone_block", "id": 1, "source": true,
             "signal": [15,15,15,15,15,15], "direct": [0,0,0,0,0,0]},
            {"state": "minecraft:redstone_torch[lit=true]", "id": 2, "source": true,
             "signal": [15,0,15,15,15,15], "direct": [15,0,0,0,0,0]},
            {"state": "minecraft:repeater[delay=1,facing=east,locked=false,powered=true]", "id": 3, "source": true,
             "signal": [0,0,0,0,0,15], "direct": [0,0,0,0,0,15]},
            {"state": "minecraft:repeater[delay=1,facing=west,locked=false,powered=true]", "id": 4, "source": true,
             "signal": [0,0,0,0,15,0], "direct": [0,0,0,0,15,0]}
          ]
        }"#;
        let dir = std::env::temp_dir().join(format!(
            "rustmc-comparator-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("signal.json");
        std::fs::write(&path, json).unwrap();
        let out = SignalTable::load(&path).expect("fixture loads");
        let _ = std::fs::remove_file(&path);
        out
    }

    fn classes() -> PlacedByTable {
        let json = r#"{"classes": {
            "minecraft:cake": "CakeBlock",
            "minecraft:candle_cake": "CandleCakeBlock",
            "minecraft:chest": "ChestBlock",
            "minecraft:trapped_chest": "TrappedChestBlock",
            "minecraft:water_cauldron": "LayeredCauldronBlock",
            "minecraft:cauldron": "CauldronBlock",
            "minecraft:respawn_anchor": "RespawnAnchorBlock",
            "minecraft:barrel": "BarrelBlock",
            "minecraft:hopper": "HopperBlock",
            "minecraft:stone": "Block"
          },
          "superclasses": {
            "TrappedChestBlock": ["ChestBlock", "AbstractChestBlock", "BaseEntityBlock", "Block"],
            "LayeredCauldronBlock": ["AbstractCauldronBlock", "Block"],
            "CauldronBlock": ["AbstractCauldronBlock", "Block"]
          },
          "table": []}"#;
        let p = std::env::temp_dir().join(format!(
            "rustmc-comparator-classes-{}-{:?}.json",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(&p, json).expect("write");
        let t = PlacedByTable::load(&p).expect("load");
        let _ = std::fs::remove_file(&p);
        t
    }

    const CMP_N: &str = "minecraft:comparator[facing=north,mode=compare,powered=false]";
    const SUB_N: &str = "minecraft:comparator[facing=north,mode=subtract,powered=false]";

    /// The dust south of a north-facing comparator asks it with NORTH and
    /// gets the entity's output; asked any other way, or unpowered, it is 0.
    #[test]
    fn a_powered_comparator_answers_its_output_only_along_its_facing() {
        let pos = (900, 0, 0);
        let on = "minecraft:comparator[facing=north,mode=compare,powered=true]";
        Outputs::shared().set(pos, 14);
        assert_eq!(own_signal(pos, on, Face::North), 14);
        assert_eq!(own_signal(pos, on, Face::South), 0);
        assert_eq!(own_signal(pos, on, Face::East), 0);
        assert_eq!(own_signal(pos, CMP_N, Face::North), 0, "unpowered");
        let t = table();
        let w = World::new(&[(pos, on)]);
        let p = Redstone::new(&w, &t, None);
        assert_eq!(
            p.get_signal(pos, Face::North),
            14,
            "through Redstone, not the table"
        );
        assert_eq!(p.get_direct_signal(pos, Face::North), 14);
        Outputs::shared().remove(pos);
    }

    #[test]
    fn lerp_discrete_is_floor_of_fourteen_plus_one_when_anything_is_there() {
        assert_eq!(lerp_discrete(0.0, 0, 15), 0);
        assert_eq!(
            lerp_discrete(1.0 / 64.0 / 27.0, 0, 15),
            1,
            "one stone in a chest"
        );
        assert_eq!(lerp_discrete(0.5, 0, 15), 8);
        assert_eq!(lerp_discrete(1.0, 0, 15), 15);
    }

    #[test]
    fn a_redstone_block_behind_a_comparator_is_fifteen_out_and_turns_it_on() {
        let t = table();
        let w = World::new(&[((0, 0, 0), CMP_N), ((0, 0, -1), "minecraft:redstone_block")]);
        let p = Redstone::new(&w, &t, None);
        assert_eq!(input_signal(&w, &p, None, (0, 0, 0), CMP_N), 15);
        assert_eq!(alternate_signal(&w, &p, (0, 0, 0), CMP_N), 0);
        assert_eq!(calculate_output_signal(&w, &p, None, (0, 0, 0), CMP_N), 15);
        assert!(should_turn_on(&w, &p, None, (0, 0, 0), CMP_N));

        let mut fx = Effects::new();
        neighbor_changed(&w, &t, None, (0, 0, 0), CMP_N, &mut fx);
        assert_eq!(fx.ticks.len(), 1);
        assert_eq!(fx.ticks[0].delay, 2);
        assert_eq!(fx.ticks[0].priority, crate::ticks::PRIORITY_NORMAL);

        let mut fx = Effects::new();
        tick(&w, &t, None, (0, 0, 0), CMP_N, &mut fx);
        assert_eq!(
            w.at((0, 0, 0)),
            "minecraft:comparator[facing=north,mode=compare,powered=true]"
        );
        assert_eq!(Outputs::shared().get((0, 0, 0)), 15);
        Outputs::shared().remove((0, 0, 0));
    }

    /// Fifteen from behind, fifteen from the side: compare mode stays on
    /// (`i == j && COMPARE`), subtract mode outputs 0 and turns off.
    ///
    /// The side repeater sits WEST of the comparator and faces WEST: a
    /// diode's `FACING` names its input side, its output goes the other
    /// way, and `DiodeBlock.getSignal` answers only when asked with a
    /// direction equal to `FACING` — the comparator asks its west neighbour
    /// with WEST. (An east-facing repeater there would be fed BY the
    /// comparator and answer 0.)
    #[test]
    fn an_equal_side_input_keeps_compare_on_and_zeroes_subtract() {
        let t = table();
        let side = "minecraft:repeater[delay=1,facing=west,locked=false,powered=true]";
        let w = World::new(&[
            ((0, 0, 0), CMP_N),
            ((0, 0, -1), "minecraft:redstone_block"),
            ((-1, 0, 0), side),
        ]);
        let p = Redstone::new(&w, &t, None);
        assert_eq!(
            alternate_signal(&w, &p, (0, 0, 0), CMP_N),
            15,
            "the repeater's direct signal, asked with WEST"
        );
        let wrong_way = "minecraft:repeater[delay=1,facing=east,locked=false,powered=true]";
        let w2 = World::new(&[
            ((0, 0, 0), CMP_N),
            ((0, 0, -1), "minecraft:redstone_block"),
            ((-1, 0, 0), wrong_way),
        ]);
        let p2 = Redstone::new(&w2, &t, None);
        assert_eq!(
            alternate_signal(&w2, &p2, (0, 0, 0), CMP_N),
            0,
            "a repeater whose input faces the comparator feeds it nothing"
        );
        assert!(should_turn_on(&w, &p, None, (0, 0, 0), CMP_N));
        assert_eq!(calculate_output_signal(&w, &p, None, (0, 0, 0), CMP_N), 15);
        assert!(!should_turn_on(&w, &p, None, (0, 0, 0), SUB_N));
        assert_eq!(calculate_output_signal(&w, &p, None, (0, 0, 0), SUB_N), 0);
    }

    #[test]
    fn a_cake_behind_a_comparator_reads_fourteen_and_a_bitten_one_less() {
        let t = table();
        let c = classes();
        let w = World::new(&[((0, 0, 0), CMP_N), ((0, 0, -1), "minecraft:cake[bites=0]")]);
        let p = Redstone::new(&w, &t, None);
        assert_eq!(input_signal(&w, &p, Some(&c), (0, 0, 0), CMP_N), 14);
        let w = World::new(&[((0, 0, 0), CMP_N), ((0, 0, -1), "minecraft:cake[bites=3]")]);
        assert_eq!(input_signal(&w, &p, Some(&c), (0, 0, 0), CMP_N), 8);
        assert!(has_analog_output_signal(
            Some(&c),
            "minecraft:trapped_chest[facing=north,type=single,waterlogged=false]"
        ));
        assert!(!has_analog_output_signal(Some(&c), "minecraft:stone"));
        assert!(!has_analog_output_signal(None, "minecraft:cake[bites=0]"));
    }

    /// A conductor between the comparator and the cake: the analog read
    /// goes one block further, and only when the direct input is under 15.
    #[test]
    fn a_comparator_reads_a_cake_through_a_solid_block() {
        let t = table();
        let c = classes();
        let w = World::new(&[
            ((0, 0, 0), CMP_N),
            ((0, 0, -1), "minecraft:stone"),
            ((0, 0, -2), "minecraft:cake[bites=1]"),
        ]);
        let p = Redstone::new(&w, &t, None);
        assert_eq!(input_signal(&w, &p, Some(&c), (0, 0, 0), CMP_N), 12);
    }

    #[test]
    fn the_cauldron_family_and_the_anchor_read_their_properties() {
        let c = classes();
        let w = World::new(&[]);
        let v = |s: &str| analog_output_signal(&w, &c, (0, 0, 0), s);
        assert_eq!(v("minecraft:water_cauldron[level=2]"), 2);
        assert_eq!(v("minecraft:cauldron"), 0);
        assert_eq!(v("minecraft:respawn_anchor[charges=0]"), 0);
        assert_eq!(v("minecraft:respawn_anchor[charges=1]"), 3);
        assert_eq!(v("minecraft:respawn_anchor[charges=2]"), 7);
        assert_eq!(v("minecraft:respawn_anchor[charges=3]"), 11);
        assert_eq!(v("minecraft:respawn_anchor[charges=4]"), 15);
        assert_eq!(v("minecraft:candle_cake[lit=false]"), 14);
        assert_eq!(
            v("minecraft:barrel[facing=up,open=false]"),
            0,
            "no block entity at the position is not a Container: 0"
        );
    }

    /// `getRedstoneSignalFromBlockEntity` over the entity actually standing
    /// there: a hopper's five slots and a barrel's twenty-seven, one full
    /// stack each — `lerpDiscrete(1/5)` is 3, `lerpDiscrete(1/27)` is 1.
    /// Before the six container classes were ported both read 0, refused.
    #[test]
    fn a_hopper_and_a_barrel_read_their_own_slots() {
        let c = classes();
        let w = World::new(&[]);
        let store = crate::blockentity::BlockEntities::shared();
        for (pos, kind, state, expected) in [
            (
                (7001, 0, 0),
                crate::blockentity::Kind::Hopper,
                "minecraft:hopper[enabled=true,facing=down]",
                3,
            ),
            (
                (7002, 0, 0),
                crate::blockentity::Kind::Barrel,
                "minecraft:barrel[facing=up,open=false]",
                1,
            ),
        ] {
            store.insert(pos, crate::blockentity::BlockEntity::new(kind));
            assert_eq!(analog_output_signal(&w, &c, pos, state), 0, "{state} empty");
            store.with_mut(pos, |be| {
                be.set_item(0, crate::item::ItemStack::new("minecraft:stone", 64));
            });
            assert_eq!(
                analog_output_signal(&w, &c, pos, state),
                expected,
                "{state}"
            );
        }
    }

    #[test]
    fn one_stone_in_twenty_seven_slots_is_one() {
        let mut slots = vec![crate::item::ItemStack::empty(); 27];
        assert_eq!(redstone_signal_from_slots(&slots), 0);
        slots[0] = crate::item::ItemStack::new("minecraft:stone", 1);
        assert_eq!(redstone_signal_from_slots(&slots), 1);
        for s in &mut slots {
            *s = crate::item::ItemStack::new("minecraft:stone", 64);
        }
        assert_eq!(redstone_signal_from_slots(&slots), 15);
        assert_eq!(
            redstone_signal_from_slots(&[]),
            0,
            "a non-container answers 0"
        );
    }

    #[test]
    fn a_left_chest_pairs_clockwise_and_a_right_one_counter_clockwise() {
        assert_eq!(
            chest_connected_direction("minecraft:chest[facing=north,type=left,waterlogged=false]"),
            Some(Face::East)
        );
        assert_eq!(
            chest_connected_direction("minecraft:chest[facing=north,type=right,waterlogged=false]"),
            Some(Face::West)
        );
    }

    /// Two comparators, one already booked for this tick: only the other
    /// asks for one.
    #[test]
    fn a_comparator_that_will_tick_this_tick_books_nothing() {
        let t = table();
        let w = World::new(&[((5, 0, 0), CMP_N), ((5, 0, -1), "minecraft:redstone_block")]);
        let q = crate::ticks::ScheduledTicks::shared();
        assert!(q.schedule((5, 0, 0), "minecraft:comparator", 1_000_000, 0, 3));
        let due = q.drain_due(1_000_000);
        assert!(due.iter().any(|d| d.pos == (5, 0, 0)));
        let mut fx = Effects::new();
        neighbor_changed(&w, &t, None, (5, 0, 0), CMP_N, &mut fx);
        assert!(fx.ticks.is_empty());
        for d in &due {
            q.mark_run(d);
        }
        let mut fx = Effects::new();
        neighbor_changed(&w, &t, None, (5, 0, 0), CMP_N, &mut fx);
        assert_eq!(fx.ticks.len(), 1);
    }
}
