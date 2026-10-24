//! `DispenserBlock`, `DropperBlock` and the dispense behaviours — the block
//! half of a dispenser, and what it does with what it holds.
//!
//! Transcribed from the pinned 26.2 jar: `DispenserBlock.neighborChanged`
//! (offsets 0-113), `.tick` (0-7), `.dispenseFrom` (0-134),
//! `.getDispensePosition` (both overloads), `DropperBlock.dispenseFrom`
//! (0-202), `DispenserBlockEntity.getRandomSlot` (0-65),
//! `DefaultDispenseItemBehavior` whole, and `HopperBlock.checkPoweredState`
//! (0-55), which lives here because it is the same "a neighbour's signal
//! rewrites one boolean with flags 2" shape and has no other home.
//!
//! # The trigger and the fire are four ticks apart, and both are observable
//!
//! `neighborChanged` computes `hasNeighborSignal(pos) ||
//! hasNeighborSignal(pos.above())` — the **quasi-connectivity** read, the
//! block above counts — and on a rising edge does two things in this order:
//! `scheduleTick(pos, this, 4)` and `setBlock(pos, triggered=true, 2)`. Flags
//! 2 is `UPDATE_CLIENTS` alone, so the client sees `triggered` flip in the
//! lever's own tick and no neighbour hears about it. The falling edge only
//! clears `triggered`. Nothing fires until the tick, and the tick does not
//! re-read the signal: a lever flipped on and off inside four ticks still
//! dispenses.
//!
//! # What a dispense puts on the wire
//!
//! `DefaultDispenseItemBehavior.dispense` is `execute`, then `playSound`,
//! then `playAnimation`. `execute` splits one item off and `spawnItem`s it —
//! `addFreshEntity` pairs synchronously, so the `add_entity` (and the item's
//! `set_entity_data`) leave first. `playSound` is `levelEvent(1000, pos, 0)`
//! and `playAnimation` is `levelEvent(2000, pos,
//! facing.get3DDataValue())`, the smoke, whose data is the facing's 3-D
//! index (down 0, up 1, north 2, south 3, west 4, east 5).
//!
//! An empty dispenser — `getRandomSlot` returned -1 — sends
//! `levelEvent(1001)`, the dry click, and a game event no client sees. An
//! empty **dropper** sends the same 1001 and no game event.
//!
//! # `getRandomSlot` draws from the level's random, and that is unreproducible
//!
//! It is reservoir sampling over the non-empty slots: `nextInt(j++) == 0`
//! for each in turn, so with exactly **one** non-empty slot the answer is that
//! slot whatever the stream holds (`nextInt(1)` is always 0). With two or more
//! it depends on `ServerLevel.getRandom()`, which is shared by everything in
//! the level and unseeded — so no implementation reproduces the choice, and
//! this one draws from a generator of the same kind and does not pretend
//! otherwise. Every corpus check built on this loads a single slot.
//!
//! # The dropped item's numbers
//!
//! `spawnItem` puts the item at `getDispensePosition` — the block centre plus
//! 0.7 along the facing — lowered by **0.125** for a vertical facing and
//! **0.15625** for a horizontal one, and those are deterministic and compared.
//! Its velocity is `triangle` draws on the level random and its spawn yaw a
//! draw on the item's own; both are unreproducible and the oracle's projection
//! drops both, exactly as it does for a thrown item (`crate::dropitem`).

use crate::edits::Blocks;
use crate::interact::{BlockPos, Face};
use crate::item::ItemStack;
use crate::power::Redstone;
use crate::shapes::{Effects, ShapeTable, RECURSION_LIMIT, UPDATE_CLIENTS};
use rustmc_worldgen::rng::{legacy::LegacyRandomSource, RandomSource};

/// `DispenserBlock.neighborChanged` offset 54: `iconst_4`.
pub const TRIGGER_DELAY: i64 = 4;

/// `DefaultDispenseItemBehavior.playDefaultSound` — `sipush 1000`.
pub const LEVEL_EVENT_DISPENSE: i32 = 1000;
/// `DispenserBlock.dispenseFrom` offset 65 — `sipush 1001`, the dry click.
pub const LEVEL_EVENT_FAIL: i32 = 1001;
/// `DefaultDispenseItemBehavior.playDefaultAnimation` — `sipush 2000`.
pub const LEVEL_EVENT_SMOKE: i32 = 2000;

/// `DefaultDispenseItemBehavior.DEFAULT_ACCURACY`.
pub const DEFAULT_ACCURACY: i32 = 6;

/// `DispenserBlock.getDispensePosition(BlockSource)` — `ldc2_w 0.7`.
const DISPENSE_REACH: f64 = 0.7;

/// The block id, without properties.
fn block_of(state: &str) -> &str {
    state.split('[').next().unwrap_or(state)
}

/// `DispenserBlock` and its one subclass, `DropperBlock`.
#[must_use]
pub fn is_dispenser(state: &str) -> bool {
    matches!(block_of(state), "minecraft:dispenser" | "minecraft:dropper")
}

/// `CrafterBlock` — a separate class, not a `DispenserBlock`, with its own
/// `neighborChanged` and `dispenseFrom`.
#[must_use]
pub fn is_crafter(state: &str) -> bool {
    block_of(state) == "minecraft:crafter"
}

/// `CrafterBlock.neighborChanged`, offsets 0-128.
///
/// Three differences from the dispenser's, each read off the bytecode:
/// **no** quasi-connectivity (`hasNeighborSignal(pos)` alone, offset 2); the
/// rising edge also sets the block entity's `triggered` data (offsets
/// 67-71), which is what the crafter menu's data slot 9 shows; and the
/// falling edge clears **`CRAFTING` as well as `TRIGGERED`** in one
/// flags-2 write (offsets 87-117), so a crafter whose lever goes off inside
/// its six crafting ticks stops glowing at once.
pub fn crafter_neighbor_changed(
    power: &Redstone<'_>,
    blocks: &dyn Blocks,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    state: &str,
    updates: &mut Effects,
) {
    let powered = power.has_neighbor_signal(pos);
    let triggered = crate::placement::property_of(state, "triggered") == Some("true");
    let has_be = crate::blockentity::BlockEntities::shared()
        .get(pos)
        .is_some();
    let set_be = |on: bool| {
        crate::blockentity::BlockEntities::shared().with_mut(pos, |be| {
            if let Some(crate::behaviour::Behaviour::Crafter(c)) = be.behaviour_mut() {
                c.triggered = on;
            }
        });
    };
    if powered && !triggered {
        updates.ticks.push(crate::shapes::TickRequest {
            pos,
            block: block_of(state).to_string(),
            delay: TRIGGER_DELAY,
            priority: crate::ticks::PRIORITY_NORMAL,
        });
        let to = crate::placement::with_property(state, "triggered", "true");
        crate::shapes::set_block(
            blocks,
            shapes,
            pos,
            &to,
            UPDATE_CLIENTS,
            RECURSION_LIMIT,
            updates,
        );
        if has_be {
            set_be(true);
        }
    } else if !powered && triggered {
        let to = crate::placement::with_property(state, "triggered", "false");
        let to = crate::placement::with_property(&to, "crafting", "false");
        crate::shapes::set_block(
            blocks,
            shapes,
            pos,
            &to,
            UPDATE_CLIENTS,
            RECURSION_LIMIT,
            updates,
        );
        if has_be {
            set_be(false);
        }
    }
}

/// `CrafterBlock.dispenseFrom` (offsets 0-235) and `dispenseItem` (0-297),
/// over a copy of the crafter's nine slots.
///
/// No recipe, or a recipe whose `assemble` is empty, is `levelEvent(1050)`
/// and nothing else — both arms send the same packet. A craft writes
/// `CRAFTING=true` with flags 2, hands the result to `dispenseItem`, then the
/// recipe's remainders (each through `dispenseItem` too), and only then
/// shrinks every non-empty slot by one.
///
/// `dispenseItem` offers the result to the container in front, if any —
/// one item at a time into another crafter or when the stack exceeds the
/// container's limit, the whole stack at once otherwise — and spawns what is
/// left at the block centre plus 0.7 along the front with
/// `DefaultDispenseItemBehavior.spawnItem`, followed by `levelEvent(1049)` and
/// `levelEvent(2010, front.get3DDataValue())`. The two events are sent **only**
/// when something was spawned.
///
/// **Not ported: remainders.** `getRemainingItems` is a bucket left by a cake
/// recipe and the like; this server's recipe model carries none, so a
/// remainder is never dispensed. Named here because it is a real difference
/// for exactly those recipes.
#[allow(clippy::too_many_arguments)]
pub fn crafter_dispense_from(
    state: &str,
    pos: BlockPos,
    slots: &mut [ItemStack],
    recipes: Option<&crate::recipes::Recipes>,
    target: Option<&mut crate::hopper::ContainerView>,
    target_is_crafter: bool,
    rules: &dyn crate::hopper::Rules,
    random: &mut impl RandomSource,
) -> Vec<Outcome> {
    let mut out = Vec::new();
    let input = crate::recipes::CraftingInput::of(3, 3, slots);
    let result = recipes.map_or_else(ItemStack::empty, |r| r.assemble(&input));
    if result.is_empty() {
        out.push(Outcome::LevelEvent(LEVEL_EVENT_CRAFTER_FAIL, pos, 0));
        return out;
    }
    let crafting = crate::placement::with_property(state, "crafting", "true");
    out.push(Outcome::SetBlock(pos, crafting, UPDATE_CLIENTS));
    let Some(front) = crate::placement::property_of(state, "orientation")
        .and_then(|o| o.split('_').next())
        .and_then(Face::from_name)
    else {
        return out;
    };
    let mut stack = result;
    if let Some(to) = target {
        if target_is_crafter || stack.count() > stack.max_stack_size() {
            while !stack.is_empty() {
                let left = crate::hopper::add_item(
                    None,
                    to,
                    stack.copy_with_count(1),
                    Some(front.opposite()),
                    rules,
                );
                if !left.is_empty() {
                    break;
                }
                to.changed = true;
                stack.shrink(1);
            }
        } else {
            while !stack.is_empty() {
                let before = stack.count();
                stack = crate::hopper::add_item(None, to, stack, Some(front.opposite()), rules);
                if stack.count() != before {
                    to.changed = true;
                }
                if before == stack.count() {
                    break;
                }
            }
        }
    }
    if !stack.is_empty() {
        out.push(Outcome::Spawn(spawn_item(
            stack,
            DEFAULT_ACCURACY,
            front,
            dispense_position(pos, front),
            random,
        )));
        out.push(Outcome::LevelEvent(LEVEL_EVENT_CRAFTER_CRAFT, pos, 0));
        out.push(Outcome::LevelEvent(
            LEVEL_EVENT_CRAFTER_SMOKE,
            pos,
            data_value_3d(front),
        ));
    }
    // Offsets 217-227: `getItems().forEach(stack -> { if (!stack.isEmpty())
    // stack.shrink(1); })`.
    for s in slots.iter_mut() {
        if !s.is_empty() {
            s.shrink(1);
        }
    }
    out
}

/// `CrafterBlock.dispenseFrom` offset 50 and 95 — `sipush 1050`.
pub const LEVEL_EVENT_CRAFTER_FAIL: i32 = 1050;
/// `CrafterBlock.dispenseItem` offset 276 — `sipush 1049`.
pub const LEVEL_EVENT_CRAFTER_CRAFT: i32 = 1049;
/// `CrafterBlock.dispenseItem` offset 285 — `sipush 2010`.
pub const LEVEL_EVENT_CRAFTER_SMOKE: i32 = 2010;

/// `DispenserBlock.neighborChanged`, offsets 0-113.
///
/// The read is `hasNeighborSignal(pos) || hasNeighborSignal(pos.above())` —
/// the block above counts, which is the quasi-connectivity every dispenser
/// has had since the beginning. The schedule is the three-argument
/// `scheduleTick(pos, block, 4)`, so `TickPriority.NORMAL`, and it is asked
/// for **before** the `triggered` write, which matters only to the order the
/// request list holds them in.
pub fn neighbor_changed(
    power: &Redstone<'_>,
    blocks: &dyn Blocks,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    state: &str,
    updates: &mut Effects,
) {
    let powered =
        power.has_neighbor_signal(pos) || power.has_neighbor_signal((pos.0, pos.1 + 1, pos.2));
    let triggered = crate::placement::property_of(state, "triggered") == Some("true");
    if powered && !triggered {
        updates.ticks.push(crate::shapes::TickRequest {
            pos,
            block: block_of(state).to_string(),
            delay: TRIGGER_DELAY,
            priority: crate::ticks::PRIORITY_NORMAL,
        });
        let to = crate::placement::with_property(state, "triggered", "true");
        crate::shapes::set_block(
            blocks,
            shapes,
            pos,
            &to,
            UPDATE_CLIENTS,
            RECURSION_LIMIT,
            updates,
        );
    } else if !powered && triggered {
        let to = crate::placement::with_property(state, "triggered", "false");
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
}

/// `HopperBlock.checkPoweredState`, offsets 0-55 — reached from both
/// `neighborChanged` and `onPlace`.
///
/// `enabled = !hasNeighborSignal(pos)`, and the state is rewritten with
/// flags 2 only when that differs from what it holds. **No** quasi-
/// connectivity here: a hopper reads its own position only.
pub fn hopper_check_powered_state(
    power: &Redstone<'_>,
    blocks: &dyn Blocks,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    state: &str,
    updates: &mut Effects,
) {
    let enabled = !power.has_neighbor_signal(pos);
    let is = crate::placement::property_of(state, "enabled") != Some("false");
    if enabled == is {
        return;
    }
    let to =
        crate::placement::with_property(state, "enabled", if enabled { "true" } else { "false" });
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

/// `DispenserBlockEntity.getRandomSlot(RandomSource)`, offsets 5-65.
///
/// Reservoir sampling: each non-empty slot replaces the pick with
/// probability `1/j`, `j` counting from 1. With one non-empty slot the
/// `nextInt(1)` is 0 and the answer is fixed; see the module doc.
pub fn random_slot(slots: &[ItemStack], random: &mut impl RandomSource) -> Option<usize> {
    let mut pick = None;
    let mut j = 1;
    for (k, s) in slots.iter().enumerate() {
        if s.is_empty() {
            continue;
        }
        let draw = random.next_i32_bound(j);
        j += 1;
        if draw == 0 {
            pick = Some(k);
        }
    }
    pick
}

/// `RandomSource.triangle(mode, deviation)` — `mode + deviation *
/// (nextDouble() - nextDouble())`, left operand drawn first.
pub fn triangle(random: &mut impl RandomSource, mode: f64, deviation: f64) -> f64 {
    let a = random.next_f64();
    let b = random.next_f64();
    mode + deviation * (a - b)
}

/// `DispenserBlock.getDispensePosition(source)` — the block centre plus 0.7
/// along the facing.
#[must_use]
pub fn dispense_position(pos: BlockPos, facing: Face) -> (f64, f64, f64) {
    let (dx, dy, dz) = facing.normal();
    (
        f64::from(pos.0) + 0.5 + DISPENSE_REACH * f64::from(dx),
        f64::from(pos.1) + 0.5 + DISPENSE_REACH * f64::from(dy),
        f64::from(pos.2) + 0.5 + DISPENSE_REACH * f64::from(dz),
    )
}

/// `Direction.get3DDataValue()` — the order of `Direction`'s constants:
/// down, up, north, south, west, east.
#[must_use]
pub const fn data_value_3d(face: Face) -> i32 {
    match face {
        Face::Down => 0,
        Face::Up => 1,
        Face::North => 2,
        Face::South => 3,
        Face::West => 4,
        Face::East => 5,
    }
}

/// One `ItemEntity` a dispense asked for, before it is an entity.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemSpawn {
    /// Where `spawnItem` put it, after the 0.125 / 0.15625 drop.
    pub at: (f64, f64, f64),
    /// `setDeltaMovement`'s three `triangle`s. Unreproducible; see the
    /// module doc.
    pub movement: (f64, f64, f64),
    pub stack: ItemStack,
}

/// `DefaultDispenseItemBehavior.spawnItem(level, stack, accuracy, facing,
/// position)`, offsets 0-166, minus the `addFreshEntity`.
///
/// The `nextDouble` that sets the speed is drawn first, then the three
/// `triangle`s in x, y, z order — six more draws. The y component's mode is a
/// constant **0.2** whatever the facing, which is why a dispenser pointing
/// down still throws its item slightly upwards.
pub fn spawn_item(
    stack: ItemStack,
    accuracy: i32,
    facing: Face,
    at: (f64, f64, f64),
    random: &mut impl RandomSource,
) -> ItemSpawn {
    let (x, mut y, z) = at;
    if matches!(facing, Face::Up | Face::Down) {
        y -= 0.125;
    } else {
        y -= 0.156_25;
    }
    let speed = random.next_f64() * 0.1 + 0.2;
    let (dx, _, dz) = facing.normal();
    let spread = 0.017_227_5 * f64::from(accuracy);
    let mx = triangle(random, f64::from(dx) * speed, spread);
    let my = triangle(random, 0.2, spread);
    let mz = triangle(random, f64::from(dz) * speed, spread);
    ItemSpawn {
        at: (x, y, z),
        movement: (mx, my, mz),
        stack,
    }
}

/// What one dispense did, in the order it did it.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// `Level.addFreshEntity` of an item.
    Spawn(ItemSpawn),
    /// `Level.levelEvent(event, pos, data)`.
    LevelEvent(i32, BlockPos, i32),
    /// `Level.setBlock(pos, state, flags)` — the crafter's `CRAFTING` write.
    SetBlock(BlockPos, String, i32),
    /// `BlockEntity.setChanged()` on the block entity at this position —
    /// see [`set_changed`].
    SetChanged(BlockPos),
}

/// `BlockEntity.setChanged()`'s level half: `setChanged(level, pos, state)`
/// is `level.blockEntityChanged(pos)` and then, unless the state is air,
/// `level.updateNeighbourForOutputSignal(pos, state.getBlock())` — so a
/// comparator beside a hopper or a dispenser re-reads it the moment its
/// contents change, not when a block next to the comparator next moves.
///
/// `updateNeighbourForOutputSignal` itself does not ask whether the block
/// has an analog output; it walks the four horizontal neighbours for
/// comparators unconditionally, and each comparator's `neighborChanged`
/// decides. Returns the effects for the caller to drain (ticks, writes).
#[must_use]
pub fn set_changed(world: &dyn Blocks, shapes: Option<&ShapeTable>, pos: BlockPos) -> Effects {
    let mut fx = Effects::new();
    if crate::shapes::is_air(&world.state_at(pos)) {
        return fx;
    }
    if let Some(signals) = crate::power::shared() {
        crate::comparator::update_neighbour_for_output_signal(world, signals, shapes, pos, &mut fx);
    }
    fx
}

/// The level's own random, standing in for `ServerLevel.getRandom()`.
///
/// One generator for the whole server, as vanilla has one per level, seeded
/// the way every unseeded `RandomSource` here is (`crate::dropitem::unique_random`).
/// Its draws are not vanilla's and cannot be; see the module doc.
pub fn level_random() -> std::sync::MutexGuard<'static, LegacyRandomSource> {
    static RANDOM: std::sync::OnceLock<std::sync::Mutex<LegacyRandomSource>> =
        std::sync::OnceLock::new();
    RANDOM
        .get_or_init(|| std::sync::Mutex::new(crate::dropitem::unique_random()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// `DispenserBlock.dispenseFrom` and `DropperBlock.dispenseFrom`, over a
/// copy of the block entity's slots.
///
/// `slots` is the dispenser's nine; `target` is `getContainerAt` for the
/// block in front, already resolved by the caller, and is read only by a
/// dropper. Returns what went out, in order, and leaves `slots` (and
/// `target`) holding what vanilla's `setItem` writes back.
#[expect(
    clippy::too_many_arguments,
    reason = "dispenseFrom reads the block, its slots, the container in front, the hopper rules, the world and the bone-meal table; a struct would only rename the list"
)]
pub fn dispense_from(
    state: &str,
    pos: BlockPos,
    slots: &mut [ItemStack],
    target: Option<&mut crate::hopper::ContainerView>,
    rules: &dyn crate::hopper::Rules,
    world: Option<&crate::itemuse::At<'_>>,
    bonemeal: Option<&crate::bonemeal::BonemealTable>,
    random: &mut impl RandomSource,
) -> Vec<Outcome> {
    let mut out = Vec::new();
    let dropper = block_of(state) == "minecraft:dropper";
    let Some(slot) = random_slot(slots, random) else {
        // Offsets 64-70 of both: `levelEvent(1001, pos, 0)`. The dispenser
        // also fires `GameEvent.BLOCK_ACTIVATE`, which reaches no client.
        out.push(Outcome::LevelEvent(LEVEL_EVENT_FAIL, pos, 0));
        return out;
    };
    let Some(facing) = crate::placement::property_of(state, "facing").and_then(Face::from_name)
    else {
        return out;
    };
    let stack = slots[slot].clone();
    if dropper {
        // `DropperBlock.dispenseFrom` offsets 92-199. A container in front
        // takes one item through its face opposite the dropper's facing, with
        // no sound and no smoke; a refusal leaves the slot as it was.
        if let Some(to) = target {
            let left = crate::hopper::add_item(
                None,
                to,
                stack.copy_with_count(1),
                Some(facing.opposite()),
                rules,
            );
            if left.is_empty() {
                to.changed = true;
                let mut rest = stack;
                rest.shrink(1);
                slots[slot] = rest;
            }
            return out;
        }
        let left = default_dispense(stack, pos, facing, random, &mut out);
        slots[slot] = left;
        return out;
    }
    // `getDispenseMethod`: `DISPENSER_REGISTRY.get(item)`, then the item's
    // own default. One registry entry is ported — the flint and steel — and
    // every other item takes `DefaultDispenseItemBehavior`, which is
    // vanilla's answer for every item the registry does not name and is
    // **wrong** for the ones it does: an arrow, a bucket, a shulker box or
    // bone meal is dropped as an item here where vanilla fires, pours,
    // places or grows it. Named rather than guessed; see the module doc.
    let left = match (stack.item(), world) {
        ("minecraft:flint_and_steel", Some(w)) => flint_and_steel(stack, pos, facing, w, &mut out),
        ("minecraft:bone_meal", Some(w)) => {
            bone_meal(stack, pos, facing, w, bonemeal, random, &mut out)
        }
        _ => default_dispense(stack, pos, facing, random, &mut out),
    };
    slots[slot] = left;
    out
}

/// `DispenseItemBehavior$5.execute` — the bone meal entry, an
/// `OptionalDispenseItemBehavior` (offsets 0-80).
///
/// `setSuccess(true)`; then `BoneMealItem.growCrop(stack, level, front)` —
/// the same method a player's click reaches, so it is `crate::bonemeal`'s
/// [`crate::bonemeal::grow_crop`] here, its draw taken only where vanilla
/// takes one — or else `growWaterPlant(stack, level, front, null)`. Either
/// returning true sends `levelEvent(1505, front, 15)`; both false is
/// `setSuccess(false)`, so the sound is **1001** instead of 1000. The smoke
/// follows either way.
///
/// Two arms answer false where vanilla can answer true, both named: a valid
/// bone-meal target `crate::bonemeal` does not grow (`Grow::Unported`, a
/// sapling or grass — the player path declines the same set, for the reason
/// it gives), and `growWaterPlant`, which this server does not have. The
/// stack is shrunk only on a grown crop, inside `growCrop` as vanilla does.
fn bone_meal(
    mut stack: ItemStack,
    pos: BlockPos,
    facing: Face,
    w: &crate::itemuse::At<'_>,
    table: Option<&crate::bonemeal::BonemealTable>,
    random: &mut impl RandomSource,
    out: &mut Vec<Outcome>,
) -> ItemStack {
    let (dx, dy, dz) = facing.normal();
    let front = (pos.0 + dx, pos.1 + dy, pos.2 + dz);
    let state = (w.state_at)(front);
    let roll = crate::bonemeal::draw_bound(table, &state).map_or(0, |b| random.next_i32_bound(b));
    let success = match (crate::bonemeal::grow_crop(table, &state, roll), table) {
        (crate::bonemeal::Grow::Crop { state }, Some(t)) => {
            out.push(Outcome::SetBlock(front, state, t.set_block_flags()));
            stack.shrink(t.shrink());
            out.push(Outcome::LevelEvent(LEVEL_EVENT_BONE_MEAL, front, 15));
            true
        }
        _ => false,
    };
    let sound = if success {
        LEVEL_EVENT_DISPENSE
    } else {
        LEVEL_EVENT_FAIL
    };
    out.push(Outcome::LevelEvent(sound, pos, 0));
    out.push(Outcome::LevelEvent(
        LEVEL_EVENT_SMOKE,
        pos,
        data_value_3d(facing),
    ));
    stack
}

/// `DispenseItemBehavior$5` offset 61 — `sipush 1505`, the bone-meal sparkle,
/// with data 15.
pub const LEVEL_EVENT_BONE_MEAL: i32 = 1505;

/// `FlintAndSteelDispenseItemBehavior.execute` (offsets 0-188), then
/// `OptionalDispenseItemBehavior.playSound` and the default animation.
///
/// `setSuccess(true)` first; then, for the block in front:
/// `tryIgniteExplosiveEntities` (an entity scan for primable minecarts and
/// the like — none of which this server spawns, so it is false), then
/// `BaseFireBlock.canBePlacedAt` -> `setBlockAndUpdate(pos, getState)`;
/// else the lit-campfire/candle arm; else a `TntBlock`; else
/// `setSuccess(false)`. A success damages the stack by one
/// (`hurtAndBreak`) — this server's stacks carry no damage, so the stack is
/// returned unchanged — and the sound is 1000 on success and **1001** on
/// failure, the smoke 2000 either way.
///
/// The campfire/candle and TNT arms are refused: they are block-tag reads
/// and a prime, and this answers them as a failure rather than writing a
/// guessed state. `crate::itemuse::flint_and_steel` refuses the same
/// campfire arm for the same reason.
fn flint_and_steel(
    stack: ItemStack,
    pos: BlockPos,
    facing: Face,
    w: &crate::itemuse::At<'_>,
    out: &mut Vec<Outcome>,
) -> ItemStack {
    let (dx, dy, dz) = facing.normal();
    let front = (pos.0 + dx, pos.1 + dy, pos.2 + dz);
    let success = match crate::itemuse::can_be_placed_at(front, w) {
        Some(fire) => {
            out.push(Outcome::SetBlock(front, fire, crate::shapes::UPDATE_ALL));
            true
        }
        None => false,
    };
    let sound = if success {
        LEVEL_EVENT_DISPENSE
    } else {
        LEVEL_EVENT_FAIL
    };
    out.push(Outcome::LevelEvent(sound, pos, 0));
    out.push(Outcome::LevelEvent(
        LEVEL_EVENT_SMOKE,
        pos,
        data_value_3d(facing),
    ));
    stack
}

/// `DefaultDispenseItemBehavior.dispense`: `execute`, `playSound`,
/// `playAnimation`.
fn default_dispense(
    mut stack: ItemStack,
    pos: BlockPos,
    facing: Face,
    random: &mut impl RandomSource,
    out: &mut Vec<Outcome>,
) -> ItemStack {
    let one = stack.split(1);
    out.push(Outcome::Spawn(spawn_item(
        one,
        DEFAULT_ACCURACY,
        facing,
        dispense_position(pos, facing),
        random,
    )));
    out.push(Outcome::LevelEvent(LEVEL_EVENT_DISPENSE, pos, 0));
    out.push(Outcome::LevelEvent(
        LEVEL_EVENT_SMOKE,
        pos,
        data_value_3d(facing),
    ));
    stack
}

/// `dispenseFrom` against the live block-entity store: read the nine slots,
/// resolve the container in front for a dropper, run [`dispense_from`], and
/// write back what changed.
///
/// `None` is `getBlockEntity(pos, DISPENSER).orElse(null) == null` — offsets
/// 17-33, a logged warning and nothing else.
pub fn run_at(
    pos: BlockPos,
    world: &dyn Blocks,
    rules: &dyn crate::hopper::Rules,
    recipes: Option<&crate::recipes::Recipes>,
) -> Option<Vec<Outcome>> {
    let store = crate::blockentity::BlockEntities::shared();
    let state = world.state_at(pos);
    if is_crafter(&state) {
        return run_crafter_at(pos, &state, world, rules, recipes);
    }
    if !is_dispenser(&state) {
        return None;
    }
    let be = store.get(pos)?;
    let mut slots: Vec<ItemStack> = (0..be.size()).map(|i| be.item(i)).collect();
    let before = slots.clone();
    let facing = crate::placement::property_of(&state, "facing").and_then(Face::from_name);
    let front = facing.map(|f| {
        let (dx, dy, dz) = f.normal();
        (pos.0 + dx, pos.1 + dy, pos.2 + dz)
    });
    let mut target = if block_of(&state) == "minecraft:dropper" {
        front.and_then(|p| crate::behaviour::view_at(store, world, p))
    } else {
        None
    };
    let around = crate::itemuse::At {
        state_at: &|p| world.state_at(p),
        face_sturdy: &|p, f| world.face_sturdy(p, f),
        default_state: &|b| world.default_state(b),
        // A dispenser here only bone-meals; no item it uses places fire.
        fire_odds: None,
    };
    let out = {
        let mut random = level_random();
        dispense_from(
            &state,
            pos,
            &mut slots,
            target.as_mut(),
            rules,
            Some(&around),
            crate::bonemeal::BonemealTable::shared(std::path::Path::new(
                "data/vanilla/bonemeal.json",
            ))
            .ok(),
            &mut *random,
        )
    };
    let wrote = slots != before;
    store.with_mut(pos, |be| {
        for (i, s) in slots.iter().enumerate() {
            be.set_item(i, s.clone());
        }
    });
    let mut out = out;
    // `DispenserBlockEntity.setItem` -> `setChanged()`: reached whenever
    // `dispenseFrom` got past the empty-slot return, which is exactly when
    // the slots moved (a dropper's refused insert writes the same stack back
    // and is also a `setItem`, so it is included by `slot_written`).
    if wrote || slot_written(&out) {
        out.push(Outcome::SetChanged(pos));
    }
    if let (Some(t), Some(p)) = (target, front) {
        if t.changed {
            for at in crate::behaviour::store_view(store, world, p, &t) {
                out.push(Outcome::SetChanged(at));
            }
        }
    }
    Some(out)
}

/// Whether `dispenseFrom` reached its `setItem` — it did unless the only
/// thing it did was the empty-dispenser click.
fn slot_written(out: &[Outcome]) -> bool {
    !matches!(out, [Outcome::LevelEvent(LEVEL_EVENT_FAIL, _, _)])
}

/// [`run_at`] for a crafter: `getBlockEntity(pos) instanceof
/// CrafterBlockEntity` or return (offsets 0-25), then
/// [`crafter_dispense_from`], then the block entity's half of a craft —
/// `setCraftingTicksRemaining(6)` — and the slot and target writes.
fn run_crafter_at(
    pos: BlockPos,
    state: &str,
    world: &dyn Blocks,
    rules: &dyn crate::hopper::Rules,
    recipes: Option<&crate::recipes::Recipes>,
) -> Option<Vec<Outcome>> {
    let store = crate::blockentity::BlockEntities::shared();
    let be = store.get(pos)?;
    if be.kind() != crate::blockentity::Kind::Crafter {
        return None;
    }
    let mut slots: Vec<ItemStack> = (0..be.size()).map(|i| be.item(i)).collect();
    let front = crate::placement::property_of(state, "orientation")
        .and_then(|o| o.split('_').next())
        .and_then(Face::from_name)
        .map(|f| {
            let (dx, dy, dz) = f.normal();
            (pos.0 + dx, pos.1 + dy, pos.2 + dz)
        });
    let mut target = front.and_then(|p| crate::behaviour::view_at(store, world, p));
    let target_is_crafter = front.is_some_and(|p| is_crafter(&world.state_at(p)));
    let out = {
        let mut random = level_random();
        crafter_dispense_from(
            state,
            pos,
            &mut slots,
            recipes,
            target.as_mut(),
            target_is_crafter,
            rules,
            &mut *random,
        )
    };
    let crafted = out.iter().any(|o| matches!(o, Outcome::SetBlock(..)));
    store.with_mut(pos, |be| {
        for (i, s) in slots.iter().enumerate() {
            be.set_item(i, s.clone());
        }
        if crafted {
            if let Some(crate::behaviour::Behaviour::Crafter(c)) = be.behaviour_mut() {
                c.begin_craft();
            }
        }
    });
    let mut out = out;
    // `dispenseFrom` offset 232: `setChanged()` after the shrink, on a craft
    // only — the two 1050 returns leave before it.
    if crafted {
        out.push(Outcome::SetChanged(pos));
    }
    if let (Some(t), Some(p)) = (target, front) {
        if t.changed {
            for at in crate::behaviour::store_view(store, world, p, &t) {
                out.push(Outcome::SetChanged(at));
            }
        }
    }
    Some(out)
}

/// The stack each live `ItemEntity` holds — `ItemEntity.getItem()`.
///
/// Kept beside the entity table rather than on [`crate::entities::Entity`]
/// for the reason `crate::fallable::Cargo` is: the entity carries what the
/// wire needs (here, `DATA_ITEM`'s encoded bytes in
/// [`crate::synched::Synched::Item`]) and this carries what the game needs,
/// which a hopper's `suckInItems` reads and empties.
#[derive(Debug, Default)]
pub struct ItemEntityStacks(std::sync::Mutex<std::collections::BTreeMap<i32, ItemStack>>);

impl ItemEntityStacks {
    /// The process-wide table.
    #[must_use]
    pub fn shared() -> &'static Self {
        static SHARED: std::sync::OnceLock<ItemEntityStacks> = std::sync::OnceLock::new();
        SHARED.get_or_init(ItemEntityStacks::default)
    }

    /// Record what item entity `id` holds.
    ///
    /// # Panics
    /// If the lock is poisoned.
    pub fn set(&self, id: i32, stack: ItemStack) {
        self.0.lock().expect("item stacks").insert(id, stack);
    }

    /// What item entity `id` holds, if it is one this table knows.
    ///
    /// # Panics
    /// If the lock is poisoned.
    #[must_use]
    pub fn get(&self, id: i32) -> Option<ItemStack> {
        self.0.lock().expect("item stacks").get(&id).cloned()
    }

    /// Forget `id` — the entity was discarded.
    ///
    /// # Panics
    /// If the lock is poisoned.
    pub fn remove(&self, id: i32) {
        self.0.lock().expect("item stacks").remove(&id);
    }
}

/// Item entities whose `DATA_ITEM` is still dirty — every one this server
/// spawns, until the tracker's next `sendChanges`.
///
/// `ItemEntity.setItem` in the constructor marks `DATA_ITEM` dirty, and
/// `ServerEntity.sendPairingData` sends `getNonDefaultValues()` **without**
/// clearing the dirty flags. So the first `sendChanges` after the spawn —
/// `sendDirtyEntityData`, every tick, before the interval gate — sends the
/// same value a second time. Vanilla's recording of a dropper's item carries
/// exactly that: two `set_entity_data` with one digest, the second after the
/// tick's block broadcast.
fn item_dirty() -> &'static std::sync::Mutex<std::collections::BTreeSet<i32>> {
    static SET: std::sync::OnceLock<std::sync::Mutex<std::collections::BTreeSet<i32>>> =
        std::sync::OnceLock::new();
    SET.get_or_init(|| std::sync::Mutex::new(std::collections::BTreeSet::new()))
}

/// `SynchedEntityData.packDirty()` for an item entity this server spawned:
/// its `DATA_ITEM` once, then nothing. `None` for anything else.
#[must_use]
pub fn drain_item_dirty(e: &crate::entities::Entity) -> Option<Vec<u8>> {
    if !item_dirty()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&e.id)
    {
        return None;
    }
    e.synched
        .non_default_values()
        .and_then(|v| crate::synched::set_entity_data_body(e.id, &v).ok())
}

/// Whether an item entity this server spawned has an unsent `DATA_ITEM`.
#[must_use]
pub fn is_dirty_item(id: i32) -> bool {
    item_dirty()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .contains(&id)
}

/// A dispensed item entity as `crate::tracker::DirtySource`: its data is
/// dirty once (`DATA_ITEM`), it never needs a position sync of its own, and
/// nothing hurts it.
pub struct ItemSync<'a>(pub &'a crate::entities::Entity);

impl crate::tracker::DirtySource for ItemSync<'_> {
    fn flags(&self) -> (bool, bool) {
        (is_dirty_item(self.0.id), false)
    }
    fn drain(&self) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
        (drain_item_dirty(self.0), None)
    }
    fn take_hurt_marked(&self) -> bool {
        false
    }
}

/// `new ItemEntity(level, x, y, z, stack)` followed by `setDeltaMovement`,
/// as `spawnItem` does it: the entity's own draws first (uuid, bob phase,
/// spawn yaw, the constructor's delta that the dispense overwrites), in the
/// order `crate::dropitem::spawn_thrown_item` documents.
pub fn spawn_item_entity(
    entities: &crate::entities::Entities,
    spawn: &ItemSpawn,
    items: Option<&crate::player::ItemIds>,
) -> crate::entities::Entity {
    let mut item_rng = crate::dropitem::unique_random();
    let uuid = crate::dropitem::insecure_uuid(&mut item_rng);
    let _bob_offs = item_rng.next_f32() * std::f32::consts::PI * 2.0;
    let y_rot = item_rng.next_f32() * 360.0;
    let _ctor_dx = item_rng.next_f64() * 0.2 - 0.1;
    let _ctor_dz = item_rng.next_f64() * 0.2 - 0.1;
    let e = crate::entities::Entity {
        id: entities.next_id(),
        uuid,
        kind: crate::entities::ITEM,
        x: spawn.at.0,
        y: spawn.at.1,
        z: spawn.at.2,
        y_rot,
        x_rot: 0.0,
        y_head_rot: 0.0,
        movement: spawn.movement,
        on_ground: false,
        data: 0,
        swinging: false,
        swing_time: 0,
        swinging_arm: None,
        synched: crate::synched::Synched::Item {
            value: crate::menu::item_stack_bytes(crate::menu::to_wire(&spawn.stack, items)),
        },
    };
    entities.insert(e.clone());
    ItemEntityStacks::shared().set(e.id, spawn.stack.clone());
    item_dirty()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(e.id);
    e
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rng() -> LegacyRandomSource {
        LegacyRandomSource::new(12345)
    }

    #[test]
    fn one_loaded_slot_is_always_the_pick() {
        let mut slots = vec![ItemStack::empty(); 9];
        slots[4] = ItemStack::one("minecraft:coal");
        for seed in 0..50 {
            let mut r = LegacyRandomSource::new(seed);
            assert_eq!(random_slot(&slots, &mut r), Some(4));
        }
        // Positive control: an empty dispenser has no pick at all.
        assert_eq!(random_slot(&vec![ItemStack::empty(); 9], &mut rng()), None);
    }

    /// The horizontal drop is 0.15625 and the vertical one 0.125 — the two
    /// `ldc2_w`s at `spawnItem` offsets 39 and 50. A port that used one for
    /// both would put every dropper's item 1/32 of a block off in y.
    #[test]
    fn a_horizontal_dispense_drops_lower_than_a_vertical_one() {
        let north = spawn_item(
            ItemStack::one("minecraft:coal"),
            6,
            Face::North,
            dispense_position((3, 0, 2), Face::North),
            &mut rng(),
        );
        assert_eq!(north.at, (3.5, 0.5 - 0.156_25, 2.5 - 0.7));
        let down = spawn_item(
            ItemStack::one("minecraft:coal"),
            6,
            Face::Down,
            dispense_position((3, 2, 2), Face::Down),
            &mut rng(),
        );
        assert_eq!(down.at, (3.5, 2.5 - 0.7 - 0.125, 2.5));
    }

    #[test]
    fn a_dropper_with_one_coal_drops_it_clicks_and_smokes_in_that_order() {
        let mut slots = vec![ItemStack::empty(); 9];
        slots[0] = ItemStack::one("minecraft:coal");
        let out = dispense_from(
            "minecraft:dropper[facing=north,triggered=true]",
            (3, 0, 2),
            &mut slots,
            None,
            &crate::hopper::NoRules,
            None,
            None,
            &mut rng(),
        );
        assert!(matches!(out[0], Outcome::Spawn(ref s) if s.stack.item() == "minecraft:coal"));
        assert_eq!(out[1], Outcome::LevelEvent(1000, (3, 0, 2), 0));
        assert_eq!(out[2], Outcome::LevelEvent(2000, (3, 0, 2), 2));
        assert!(slots[0].is_empty());
    }

    #[test]
    fn an_empty_dispenser_only_clicks() {
        let mut slots = vec![ItemStack::empty(); 9];
        let out = dispense_from(
            "minecraft:dispenser[facing=north,triggered=true]",
            (0, 0, 0),
            &mut slots,
            None,
            &crate::hopper::NoRules,
            None,
            None,
            &mut rng(),
        );
        assert_eq!(out, vec![Outcome::LevelEvent(1001, (0, 0, 0), 0)]);
    }

    #[test]
    fn a_dropper_facing_a_chest_fills_it_silently() {
        let mut slots = vec![ItemStack::empty(); 9];
        slots[0] = ItemStack::new("minecraft:coal", 2);
        let mut chest = crate::hopper::ContainerView::empty(27);
        let out = dispense_from(
            "minecraft:dropper[facing=north,triggered=true]",
            (0, 0, 0),
            &mut slots,
            Some(&mut chest),
            &crate::hopper::NoRules,
            None,
            None,
            &mut rng(),
        );
        assert!(out.is_empty());
        assert_eq!(slots[0].count(), 1);
        assert_eq!(chest.item(0).item(), "minecraft:coal");
        assert!(chest.changed);
        // Positive control: the same dropper with nothing in front drops.
        let out = dispense_from(
            "minecraft:dropper[facing=north,triggered=true]",
            (0, 0, 0),
            &mut slots,
            None,
            &crate::hopper::NoRules,
            None,
            None,
            &mut rng(),
        );
        assert_eq!(out.len(), 3);
    }

    fn planks() -> crate::recipes::Recipes {
        crate::recipes::Recipes::parse(
            &serde_json::json!({
                "oak_planks": {
                    "type": "minecraft:crafting_shapeless",
                    "ingredients": ["#minecraft:oak_logs"],
                    "result": { "count": 4, "id": "minecraft:oak_planks" }
                }
            }),
            &serde_json::json!({ "oak_logs": { "values": ["minecraft:oak_log"] } }),
        )
        .expect("a one-recipe table")
    }

    /// The crafter's success order is `CRAFTING`, the item, 1049, then 2010 —
    /// and the log is consumed only after the result has gone out.
    #[test]
    fn a_crafter_with_a_log_glows_drops_four_planks_and_eats_the_log() {
        let mut slots = vec![ItemStack::empty(); 9];
        slots[0] = ItemStack::one("minecraft:oak_log");
        let out = crafter_dispense_from(
            "minecraft:crafter[crafting=false,orientation=north_up,triggered=true]",
            (3, 0, 2),
            &mut slots,
            Some(&planks()),
            None,
            false,
            &crate::hopper::NoRules,
            &mut rng(),
        );
        assert!(
            matches!(&out[0], Outcome::SetBlock((3, 0, 2), s, 2) if s.contains("crafting=true"))
        );
        assert!(
            matches!(&out[1], Outcome::Spawn(sp) if sp.stack.item() == "minecraft:oak_planks" && sp.stack.count() == 4)
        );
        assert_eq!(out[2], Outcome::LevelEvent(1049, (3, 0, 2), 0));
        assert_eq!(out[3], Outcome::LevelEvent(2010, (3, 0, 2), 2));
        assert!(slots[0].is_empty());
    }

    /// No recipe: 1050 and nothing else, and the stick stays.
    #[test]
    fn a_crafter_with_no_recipe_only_clicks() {
        let mut slots = vec![ItemStack::empty(); 9];
        slots[0] = ItemStack::one("minecraft:stick");
        let out = crafter_dispense_from(
            "minecraft:crafter[crafting=false,orientation=north_up,triggered=true]",
            (0, 0, 0),
            &mut slots,
            Some(&planks()),
            None,
            false,
            &crate::hopper::NoRules,
            &mut rng(),
        );
        assert_eq!(out, vec![Outcome::LevelEvent(1050, (0, 0, 0), 0)]);
        assert_eq!(slots[0].item(), "minecraft:stick");
    }

    /// A crafter facing a chest puts the planks in it and sends neither
    /// 1049 nor 2010 — those two are inside the spawn branch.
    #[test]
    fn a_crafter_facing_a_chest_fills_it_without_the_craft_sound() {
        let mut slots = vec![ItemStack::empty(); 9];
        slots[0] = ItemStack::one("minecraft:oak_log");
        let mut chest = crate::hopper::ContainerView::empty(27);
        let out = crafter_dispense_from(
            "minecraft:crafter[crafting=false,orientation=north_up,triggered=true]",
            (0, 0, 0),
            &mut slots,
            Some(&planks()),
            Some(&mut chest),
            false,
            &crate::hopper::NoRules,
            &mut rng(),
        );
        assert_eq!(out.len(), 1, "only the CRAFTING write: {out:?}");
        assert_eq!(chest.item(0).count(), 4);
    }

    /// Bone meal at air fails: 1001 and the smoke, no item, the stack kept.
    /// Positive control: the same dispenser holding coal throws it with 1000.
    #[test]
    fn bone_meal_at_nothing_clicks_dry_and_stays() {
        let air = |_: BlockPos| "minecraft:air".to_string();
        let w = crate::itemuse::At {
            state_at: &air,
            face_sturdy: &|_, _| true,
            default_state: &|b| Some(b.to_string()),
            fire_odds: None,
        };
        let mut slots = vec![ItemStack::empty(); 9];
        slots[0] = ItemStack::one("minecraft:bone_meal");
        let out = dispense_from(
            "minecraft:dispenser[facing=north,triggered=true]",
            (0, 0, 0),
            &mut slots,
            None,
            &crate::hopper::NoRules,
            Some(&w),
            None,
            &mut rng(),
        );
        assert_eq!(
            out,
            vec![
                Outcome::LevelEvent(1001, (0, 0, 0), 0),
                Outcome::LevelEvent(2000, (0, 0, 0), 2)
            ]
        );
        assert_eq!(slots[0].item(), "minecraft:bone_meal");
        slots[0] = ItemStack::one("minecraft:coal");
        let out = dispense_from(
            "minecraft:dispenser[facing=north,triggered=true]",
            (0, 0, 0),
            &mut slots,
            None,
            &crate::hopper::NoRules,
            Some(&w),
            None,
            &mut rng(),
        );
        assert!(matches!(out[0], Outcome::Spawn(_)));
        assert_eq!(out[1], Outcome::LevelEvent(1000, (0, 0, 0), 0));
    }

    /// Flint and steel over a sturdy face writes fire in front with
    /// `setBlockAndUpdate`'s flags 3, then 1000 and the smoke.
    #[test]
    fn flint_and_steel_lights_the_air_in_front() {
        let air = |_: BlockPos| "minecraft:air".to_string();
        let w = crate::itemuse::At {
            state_at: &air,
            face_sturdy: &|_, _| true,
            default_state: &|b| Some(b.to_string()),
            fire_odds: None,
        };
        let mut slots = vec![ItemStack::empty(); 9];
        slots[0] = ItemStack::one("minecraft:flint_and_steel");
        let out = dispense_from(
            "minecraft:dispenser[facing=north,triggered=true]",
            (0, 0, 0),
            &mut slots,
            None,
            &crate::hopper::NoRules,
            Some(&w),
            None,
            &mut rng(),
        );
        assert_eq!(
            out[0],
            Outcome::SetBlock((0, 0, -1), "minecraft:fire".into(), 3)
        );
        assert_eq!(out[1], Outcome::LevelEvent(1000, (0, 0, 0), 0));
        assert_eq!(out[2], Outcome::LevelEvent(2000, (0, 0, 0), 2));
    }
}
