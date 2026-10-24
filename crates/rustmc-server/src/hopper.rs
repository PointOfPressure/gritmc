//! `HopperBlockEntity` — the item transfer half, transcribed from
//! `net/minecraft/world/level/block/entity/HopperBlockEntity` in the pinned
//! 26.2 server jar.
//!
//! # Why this is a view over slots rather than a `Container` trait
//!
//! Vanilla's `Container` is an interface implemented by block entities that
//! live in the level and are mutated in place. This server holds its block
//! entities in one shared map behind [`crate::blockentity::BlockEntities`],
//! and `addItem(from, to, …)` needs `from` readable and `to` writable at the
//! same instant — two `with_mut` closures nested on one lock, which that map
//! cannot give and which would deadlock if it could.
//!
//! So the transfer runs on [`ContainerView`]s that the caller has already
//! taken out of the map, and the caller writes back the ones the run reports
//! as changed. That is not a simplification of vanilla: vanilla's containers
//! are in-memory arrays too, `setChanged()` is a flag rather than a write,
//! and the only state the algorithm reads outside the two containers is the
//! `tickedGameTime` pair that [`ContainerView::hopper`] carries.
//!
//! Everything else here is the bytecode's order and constants:
//!
//! * `pushItemsTick` decrements the cooldown **before** it tests it (offsets
//!   0-7), so a hopper set to 8 moves again on the eighth tick, not the
//!   ninth;
//! * `tryMoveItems` runs `ejectItems` first and `suckInItems` second, and the
//!   `ior` at offset 66 means the suck runs **even when the eject already
//!   moved something** — a `||` there would be wrong;
//! * the cooldown it sets on success is `8` (`bipush 8` at offset 75), and
//!   `tryMoveInItem`'s `8 - isSameTickOrLater` at offsets 178-185 is the
//!   asymmetry that makes a chain of hoppers keep step;
//! * the eject loop restores the count it took **and puts the stack back only
//!   when the count was exactly 1** (offsets 110-121). At any larger count
//!   `setCount` on the borrowed reference is enough, because `removeItem`
//!   handed back a split rather than the slot's own stack.

use crate::interact::Face;
use crate::item::ItemStack;

/// `HopperBlockEntity.HOPPER_CONTAINER_SIZE`.
pub const HOPPER_CONTAINER_SIZE: usize = 5;

/// The cooldown `tryMoveItems` sets after a successful move — `bipush 8` at
/// offset 75.
pub const MOVE_ITEM_SPEED: i32 = 8;

/// `WorldlyContainer`, and the three block entities in 26.2 that implement it.
///
/// Established by walking every class under
/// `net/minecraft/world/level/block/entity/` and asking `javap` which declare
/// the interface: **`AbstractFurnaceBlockEntity`, `BrewingStandBlockEntity`
/// and `ShulkerBoxBlockEntity`, and no others.** A chest, a barrel, a dropper
/// and a hopper are all plain `Container`s, so `getSlots` falls through to
/// `createFlatSlots(getContainerSize())` for them and every face sees every
/// slot in index order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Worldly {
    /// Not a `WorldlyContainer`: `createFlatSlots(size)`, `canPlaceItem`
    /// answering `Container`'s default `true`.
    None,
    /// `AbstractFurnaceBlockEntity` — furnace, blast furnace, smoker.
    Furnace,
    /// `BrewingStandBlockEntity`.
    Brewing,
    /// `ShulkerBoxBlockEntity`.
    Shulker,
}

impl Worldly {
    /// `WorldlyContainer.getSlotsForFace(Direction)`, or `createFlatSlots` for
    /// [`Worldly::None`].
    ///
    /// The furnace arrays are its `static {}` verbatim: `SLOTS_FOR_UP = {0}`,
    /// **`SLOTS_FOR_DOWN = {2, 1}`** — two entries, result before fuel, in
    /// that order — and `SLOTS_FOR_SIDES = {1}`. The brewing stand's are
    /// `UP = {3}`, `DOWN = {0, 1, 2, 3}` and `SIDES = {0, 1, 2, 4}`; note that
    /// the sides array ends in **4**, the fuel slot, where the down array ends
    /// in 3, the ingredient. The shulker box answers `IntStream.range(0, 27)`
    /// for every face.
    ///
    /// Slot order here is the order `ejectItems` and `suckInItems` walk, so an
    /// array written the other way round moves a different item.
    #[must_use]
    pub fn slots_for_face(self, size: usize, face: Face) -> Vec<usize> {
        match self {
            Self::None => (0..size).collect(),
            Self::Furnace => match face {
                Face::Down => vec![2, 1],
                Face::Up => vec![0],
                _ => vec![1],
            },
            Self::Brewing => match face {
                Face::Up => vec![3],
                Face::Down => vec![0, 1, 2, 3],
                _ => vec![0, 1, 2, 4],
            },
            Self::Shulker => (0..27).collect(),
        }
    }

    /// `WorldlyContainer.canTakeItemThroughFace(int, ItemStack, Direction)`.
    ///
    /// The furnace's is the only one with a *negative* clause: pulling out of
    /// slot 1 (fuel) through `DOWN` succeeds only for a bucket or a water
    /// bucket, which is how a hopper under a furnace gets the empty bucket a
    /// lava bucket left behind and nothing else. Every other slot and every
    /// other face is `true` (offsets 38-39).
    ///
    /// The brewing stand refuses slot 3 (the ingredient) unless the stack is a
    /// glass bottle — offsets 0-12 — and the shulker box is unconditionally
    /// `true`.
    #[must_use]
    pub fn can_take_through_face(self, slot: usize, stack: &ItemStack, face: Face) -> bool {
        match self {
            Self::None | Self::Shulker => true,
            Self::Furnace => {
                if face == Face::Down && slot == 1 {
                    stack.item() == "minecraft:water_bucket" || stack.item() == "minecraft:bucket"
                } else {
                    true
                }
            }
            Self::Brewing => {
                if slot == 3 {
                    stack.item() == "minecraft:glass_bottle"
                } else {
                    true
                }
            }
        }
    }
}

/// One container, taken out of the world for the length of one transfer.
///
/// `changed` is `Container.setChanged()`: a flag the algorithm sets and the
/// caller reads, rather than a write that would need the map lock.
#[derive(Clone, Debug)]
pub struct ContainerView {
    /// `NonNullList<ItemStack>`; empty slots are [`ItemStack::empty`], never
    /// absent, so [`ContainerView::item`] is total the way `getItem` is.
    pub slots: Vec<ItemStack>,
    /// Which `WorldlyContainer`, if any, this is.
    pub worldly: Worldly,
    /// The `HopperBlockEntity` fields `tryMoveInItem` reads at offsets
    /// 124-185, for a container that is one. `None` for everything else, which
    /// is exactly what its `instanceof` test at offset 125 answers.
    pub hopper: Option<HopperClock>,
    /// `Container.setChanged()` was called on this view.
    pub changed: bool,
}

/// The two hopper fields the cooldown asymmetry reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HopperClock {
    /// `HopperBlockEntity.tickedGameTime`, stamped from `Level.getGameTime()`
    /// at `pushItemsTick` offset 12 — **before** the cooldown is tested, so a
    /// hopper on cooldown still stamps it.
    pub ticked_game_time: i64,
    /// `HopperBlockEntity.cooldownTime`.
    pub cooldown: i32,
    /// `isOnCustomCooldown()` — `cooldownTime > 8`. A hopper a redstone signal
    /// or a minecart put on a longer cooldown is not re-cooled by an insert.
    pub custom: bool,
}

impl ContainerView {
    /// A view with `n` empty slots and no special rules.
    #[must_use]
    pub fn empty(n: usize) -> Self {
        Self {
            slots: vec![ItemStack::empty(); n],
            worldly: Worldly::None,
            hopper: None,
            changed: false,
        }
    }

    /// `Container.getContainerSize()`.
    #[must_use]
    pub fn size(&self) -> usize {
        self.slots.len()
    }

    /// `Container.getItem(int)`.
    #[must_use]
    pub fn item(&self, slot: usize) -> ItemStack {
        self.slots
            .get(slot)
            .cloned()
            .unwrap_or_else(ItemStack::empty)
    }

    /// `Container.setItem(int, ItemStack)`.
    pub fn set_item(&mut self, slot: usize, stack: ItemStack) {
        if let Some(s) = self.slots.get_mut(slot) {
            *s = stack;
        }
    }

    /// `Container.isEmpty()`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(ItemStack::is_empty)
    }

    /// `Container.removeItem(int, int)` — `ContainerHelper.removeItem`, which
    /// splits the slot's stack and leaves the remainder in place.
    pub fn remove_item(&mut self, slot: usize, count: i32) -> ItemStack {
        match self.slots.get_mut(slot) {
            Some(s) if !s.is_empty() && count > 0 => s.split(count),
            _ => ItemStack::empty(),
        }
    }

    /// `getSlots(Container, Direction)` — offsets 0-67.
    #[must_use]
    pub fn slots_for(&self, face: Face) -> Vec<usize> {
        self.worldly.slots_for_face(self.size(), face)
    }

    /// `Container.canPlaceItem(int, ItemStack)`, for the kinds whose rule is a
    /// property of the container alone.
    ///
    /// The furnace's rule is here in full and is the one place a hopper needs
    /// a table outside the two containers: slot 2 (result) refuses
    /// everything, slot 1 (fuel) accepts a fuel **or** a bucket-when-the-fuel-
    /// slot-does-not-already-hold-a-bucket, and slot 0 (ingredient) accepts
    /// anything. `is_fuel` is the caller's `FuelValues.isFuel`; a caller with
    /// no fuel table passes one that always answers false, which makes the
    /// hopper refuse to fuel a furnace rather than fuel it with the wrong
    /// thing.
    ///
    /// The brewing stand's rule reads `PotionBrewing.isIngredient` and the
    /// `brewing_fuel` item tag; neither is a property of the container either,
    /// so both arrive through [`Rules`] for the same reason.
    #[must_use]
    pub fn can_place_item(&self, slot: usize, stack: &ItemStack, rules: &dyn Rules) -> bool {
        match self.worldly {
            // `Container.canPlaceItem`'s default body is `iconst_1; ireturn`.
            Worldly::None => true,
            // `AbstractFurnaceBlockEntity.canPlaceItem` offsets 0-65.
            Worldly::Furnace => match slot {
                2 => false,
                1 => {
                    rules.is_fuel(stack)
                        || (stack.item() == "minecraft:bucket"
                            && self.item(1).item() != "minecraft:bucket")
                }
                _ => true,
            },
            // `BrewingStandBlockEntity.canPlaceItem` offsets 0-101.
            Worldly::Brewing => match slot {
                3 => rules.is_brewing_ingredient(stack),
                4 => rules.is_brewing_fuel(stack),
                // Slots 0-2, the bottles: one of the four potion-shaped items
                // **and** the slot has to be empty already. That second
                // conjunct at offsets 85-93 is what stops a hopper topping a
                // bottle up mid-brew.
                _ => {
                    matches!(
                        stack.item(),
                        "minecraft:potion"
                            | "minecraft:splash_potion"
                            | "minecraft:lingering_potion"
                            | "minecraft:glass_bottle"
                    ) && self.item(slot).is_empty()
                }
            },
            // `ShulkerBoxBlockEntity` does not override `canPlaceItem`; its
            // refusal of nested shulker boxes is in
            // `canPlaceItemThroughFace`, which is a different call site.
            Worldly::Shulker => true,
        }
    }

    /// `canPlaceItemInContainer(Container, ItemStack, int, Direction)` —
    /// `canPlaceItem` **and then**, only for a `WorldlyContainer`,
    /// `canPlaceItemThroughFace`.
    ///
    /// Two of the three through-face overrides delegate straight back to
    /// `canPlaceItem` (`invokevirtual canPlaceItem` at their offset 3), so the
    /// only one that adds anything is the shulker box's refusal of a stack
    /// whose `Block.byItem` is a `ShulkerBoxBlock`.
    #[must_use]
    pub fn can_place_in(
        &self,
        stack: &ItemStack,
        slot: usize,
        face: Option<Face>,
        rules: &dyn Rules,
    ) -> bool {
        if !self.can_place_item(slot, stack, rules) {
            return false;
        }
        // `face` is unread by all three overrides in 26.2: the furnace's and
        // the brewing stand's delegate to `canPlaceItem` and the shulker
        // box's tests only the stack. It is a parameter so that the first
        // override that does read it has somewhere to go, and so that a
        // caller cannot forget to have one.
        let _ = face;
        match self.worldly {
            Worldly::Shulker => !stack.item().ends_with("shulker_box"),
            _ => true,
        }
    }

    /// `canTakeItemFromContainer(Container from, Container to, ItemStack, int,
    /// Direction)`.
    ///
    /// `Container.canTakeItem`'s default is `true` and nothing among the
    /// hopper's reachable sources overrides it, so this reduces to the
    /// `WorldlyContainer` half.
    #[must_use]
    pub fn can_take_from(&self, stack: &ItemStack, slot: usize, face: Face) -> bool {
        self.worldly.can_take_through_face(slot, stack, face)
    }
}

/// The predicates `canPlaceItem` needs that live outside the container.
///
/// A trait rather than a table because two of the three are registry lookups
/// this server does not have yet, and a hard-coded list would be a table that
/// nothing can go red against.
pub trait Rules {
    /// `FuelValues.isFuel(ItemStack)`.
    fn is_fuel(&self, stack: &ItemStack) -> bool;
    /// `PotionBrewing.isIngredient(ItemStack)`.
    fn is_brewing_ingredient(&self, stack: &ItemStack) -> bool;
    /// `ItemStack.is(ItemTags.BREWING_FUEL)`.
    fn is_brewing_fuel(&self, stack: &ItemStack) -> bool;
}

/// The rules a server with none of the three tables can honestly answer.
///
/// **Every answer is `false`, and that is a refusal rather than a guess.** A
/// hopper over a furnace with these rules will not put coal in the fuel slot;
/// it will also never put the *wrong* thing there, and the day
/// `data/vanilla/fuel_values.json` lands the first answer becomes real without
/// anything else moving.
pub struct NoRules;

impl Rules for NoRules {
    fn is_fuel(&self, _stack: &ItemStack) -> bool {
        false
    }
    fn is_brewing_ingredient(&self, _stack: &ItemStack) -> bool {
        false
    }
    fn is_brewing_fuel(&self, _stack: &ItemStack) -> bool {
        false
    }
}

/// The rules the clock thread can answer: the server's fuel table, the item
/// tags, and `PotionBrewing` for the stacks this server holds.
///
/// `BrewingStandBlockEntity.canPlaceItem` (offsets 0-101) asks
/// `level.potionBrewing().isIngredient(stack)` for slot 3 and
/// `stack.is(ItemTags.BREWING_FUEL)` for slot 4; both were refused here while
/// `crate::brewing` refused the mix table, so a hopper over a brewing stand
/// filled the bottles and never the ingredient or the fuel. Each table that is
/// missing still answers `false`, the refusal direction.
pub struct VanillaRules<'a> {
    /// `Level.fuelValues()`.
    pub fuels: Option<&'a crate::fuel::FuelValues>,
    /// The item tags, for `BREWING_FUEL`.
    pub tags: Option<&'a crate::recipes::ItemTags>,
}

impl Rules for VanillaRules<'_> {
    fn is_fuel(&self, stack: &ItemStack) -> bool {
        self.fuels.is_some_and(|f| f.is_fuel(stack))
    }
    fn is_brewing_ingredient(&self, stack: &ItemStack) -> bool {
        crate::brewing::Brewing::is_ingredient(&crate::brewing::VanillaBrewing, stack)
    }
    fn is_brewing_fuel(&self, stack: &ItemStack) -> bool {
        crate::menuslots::brewing_fuel_may_place(stack, self.tags)
    }
}

/// `canMergeItems(ItemStack, ItemStack)` — offsets 0-24.
///
/// Note the `if_icmpgt`: the test is `a.getCount() <= a.getMaxStackSize()`,
/// **not** `<`. A full stack merges with an empty-room target and moves zero
/// items, which is why `tryMoveInItem` has its own `moved > 0` test at offset
/// 102 rather than trusting this one.
#[must_use]
pub fn can_merge_items(a: &ItemStack, b: &ItemStack) -> bool {
    a.count() <= a.max_stack_size() && ItemStack::is_same_item_same_components(a, b)
}

/// `tryMoveInItem(Container from, Container to, ItemStack, int slot,
/// Direction)` — offsets 0-195. Returns what is left of `stack`.
///
/// The cooldown block at offsets 119-185 fires only when **all** of: something
/// moved, the target `was empty before this call` (`isEmpty` is sampled at
/// offset 23, before the write), the target is a hopper, and that hopper is
/// not on a custom cooldown. `same_tick_or_later` is 1 when the source is also
/// a hopper whose `tickedGameTime` is `>=` the target's, so the target is set
/// to 7 instead of 8 and the two stay a tick apart forever rather than
/// beating together.
pub fn try_move_in_item(
    from_clock: Option<HopperClock>,
    to: &mut ContainerView,
    mut stack: ItemStack,
    slot: usize,
    face: Option<Face>,
    rules: &dyn Rules,
) -> ItemStack {
    let existing = to.item(slot);
    if !to.can_place_in(&stack, slot, face, rules) {
        return stack;
    }
    let mut moved = false;
    let was_empty = to.is_empty();
    if existing.is_empty() {
        to.set_item(slot, stack);
        stack = ItemStack::empty();
        moved = true;
    } else if can_merge_items(&existing, &stack) {
        let room = stack.max_stack_size() - existing.count();
        let n = stack.count().min(room);
        stack.shrink(n);
        let mut grown = existing;
        grown.grow(n);
        to.set_item(slot, grown);
        moved = n > 0;
    }
    if moved {
        if was_empty {
            if let Some(target) = to.hopper.as_mut() {
                if !target.custom {
                    let same_tick_or_later = i32::from(
                        from_clock
                            .is_some_and(|src| target.ticked_game_time >= src.ticked_game_time),
                    );
                    target.cooldown = MOVE_ITEM_SPEED - same_tick_or_later;
                }
            }
        }
        to.changed = true;
    }
    stack
}

/// `addItem(Container from, Container to, ItemStack, Direction)` — offsets
/// 0-109. Returns the remainder.
///
/// The `Direction` is nullable and the null case is not the same as "no
/// worldly container": at offset 13 a `null` face on a `WorldlyContainer`
/// falls through to the **flat** loop over `getContainerSize()`, ignoring
/// `getSlotsForFace` entirely. `suckInItems` is the caller that passes null,
/// which is why a hopper pulling into itself walks its own five slots in
/// index order.
pub fn add_item(
    from_clock: Option<HopperClock>,
    to: &mut ContainerView,
    mut stack: ItemStack,
    face: Option<Face>,
    rules: &dyn Rules,
) -> ItemStack {
    let slots: Vec<usize> = match (to.worldly, face) {
        (Worldly::None, _) | (_, None) => (0..to.size()).collect(),
        (w, Some(f)) => w.slots_for_face(to.size(), f),
    };
    for slot in slots {
        if stack.is_empty() {
            break;
        }
        stack = try_move_in_item(from_clock, to, stack, slot, face, rules);
    }
    stack
}

/// `isFullContainer(Container, Direction)` — offsets 0-60.
///
/// `getCount() >= getMaxStackSize()` on every slot the face exposes. An
/// **empty** slot answers `0 >= 64`, false, so a container with one empty slot
/// is not full whatever else it holds.
#[must_use]
pub fn is_full_container(c: &ContainerView, face: Face) -> bool {
    c.slots_for(face)
        .into_iter()
        .all(|s| c.item(s).count() >= c.item(s).max_stack_size())
}

/// `HopperBlockEntity.inventoryFull()` — offsets 0-51.
///
/// Not the same predicate as [`is_full_container`]: this one tests
/// `getCount() != getMaxStackSize()` and treats an **empty** slot as not full
/// via the `isEmpty` short-circuit at offset 28. The two agree on every
/// reachable state and are written separately because the bytecode has them
/// separately.
#[must_use]
pub fn inventory_full(h: &ContainerView) -> bool {
    h.slots
        .iter()
        .all(|s| !s.is_empty() && s.count() == s.max_stack_size())
}

/// `ejectItems(Level, BlockPos, HopperBlockEntity)` — offsets 0-131.
///
/// `to` is the attached container, `face` the hopper's **facing**; the
/// direction handed to `isFullContainer` and `addItem` is its opposite
/// (offset 17), because a hopper pointing down inserts through the target's
/// `UP` face.
pub fn eject_items(
    hopper: &mut ContainerView,
    to: &mut ContainerView,
    facing: Face,
    rules: &dyn Rules,
) -> bool {
    let insert_face = facing.opposite();
    if is_full_container(to, insert_face) {
        return false;
    }
    for slot in 0..hopper.size() {
        if hopper.item(slot).is_empty() {
            continue;
        }
        let before = hopper.item(slot);
        let count = before.count();
        let clock = hopper.hopper;
        let one = hopper.remove_item(slot, 1);
        let left = add_item(clock, to, one, Some(insert_face), rules);
        if left.is_empty() {
            to.changed = true;
            return true;
        }
        // Offsets 103-121 restore the count and then `setItem` **only** when
        // the original count was 1. That `if` is a Java aliasing detail and
        // not a rule: `removeItem` splits, so above 1 the slot still holds the
        // very object `setCount` was called on and the write is redundant,
        // while at exactly 1 the slot was emptied and the write is the whole
        // restore. Here `before` is a copy either way, so both arms have to
        // write and the `count == 1` test would distinguish nothing. Written
        // out rather than silently dropped, because the reader who checks the
        // bytecode will look for it.
        let mut restored = before;
        restored.set_count(count);
        hopper.set_item(slot, restored);
    }
    false
}

/// `tryTakeInItemFromSlot(Hopper, Container, int, Direction)` — offsets 0-91.
pub fn try_take_in_item_from_slot(
    hopper: &mut ContainerView,
    source: &mut ContainerView,
    slot: usize,
    face: Face,
    rules: &dyn Rules,
) -> bool {
    let before = source.item(slot);
    if before.is_empty() || !source.can_take_from(&before, slot, face) {
        return false;
    }
    let count = before.count();
    let clock = source.hopper;
    let one = source.remove_item(slot, 1);
    // The `aconst_null` at offset 46 is the `Direction`: a hopper sucking in
    // ignores its own faces, so this is the flat loop over its five slots.
    let left = add_item(clock, hopper, one, None, rules);
    if left.is_empty() {
        source.changed = true;
        return true;
    }
    let mut restored = before;
    restored.set_count(count);
    source.set_item(slot, restored);
    false
}

/// `suckInItems(Level, Hopper)` — the container half, offsets 0-102.
///
/// The item-entity half (offsets 103-192) needs `getItemsAtAndAbove` and a
/// live entity list and is **not** here; see this module's tests for what that
/// costs. `source` is the container one block **above** the hopper
/// (`getLevelY() + 1` at offsets 12-13), and the face is always `DOWN`
/// (offset 44) whatever way the hopper points.
pub fn suck_in_items(
    hopper: &mut ContainerView,
    source: &mut ContainerView,
    rules: &dyn Rules,
) -> bool {
    for slot in source.slots_for(Face::Down) {
        if try_take_in_item_from_slot(hopper, source, slot, Face::Down, rules) {
            return true;
        }
    }
    false
}

/// What one `pushItemsTick` did, for a caller that has to write it back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TickOutcome {
    /// Something moved, so `tryMoveItems` set the cooldown and called
    /// `setChanged`.
    pub moved: bool,
    /// The hopper's `cooldownTime` after the tick.
    pub cooldown: i32,
}

/// `pushItemsTick` and `tryMoveItems` together — offsets 0-45 and 0-89.
///
/// `enabled` is `state.getValue(HopperBlock.ENABLED)`; `attached` is
/// `getAttachedContainer` and `above` is `getSourceContainer`, either of which
/// may be absent. The order is fixed and both halves run: eject, then suck,
/// joined by `ior` and not by a short circuit.
pub fn push_items_tick(
    hopper: &mut ContainerView,
    now: i64,
    enabled: bool,
    facing: Face,
    attached: Option<&mut ContainerView>,
    above: Option<&mut ContainerView>,
    rules: &dyn Rules,
) -> TickOutcome {
    push_items_tick_with_items(hopper, now, enabled, facing, attached, above, None, rules)
}

/// One `ItemEntity` inside `Hopper.SUCK_AABB`: its entity id and its stack,
/// which [`suck_in_item_entities`] rewrites in place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemAbove {
    pub id: i32,
    pub stack: ItemStack,
}

/// `suckInItems` offsets 103-192 — the item-entity half, reached only when
/// `getSourceContainer` found no container above.
///
/// For each entity in `getItemsAtAndAbove` order, `addItem(Container,
/// ItemEntity)`: offer a **copy** of its whole stack to the hopper's five
/// slots through no face. If all of it went in the entity is emptied and
/// discarded and the suck **returns true at once**; if only some went in the
/// entity keeps the rest and the walk **continues** — and in that case the
/// method returns false even though the hopper changed, so `tryMoveItems`
/// sets no cooldown. Both halves are offsets 177-188.
///
/// The `isGridAligned && isCollisionShapeFullBlock && !DOES_NOT_BLOCK_HOPPERS`
/// test at offsets 103-136 is the caller's: it decides whether `items` is
/// handed over at all.
pub fn suck_in_item_entities(
    hopper: &mut ContainerView,
    items: &mut [ItemAbove],
    rules: &dyn Rules,
) -> bool {
    for e in items.iter_mut() {
        if e.stack.is_empty() {
            continue;
        }
        let left = add_item(None, hopper, e.stack.clone(), None, rules);
        if left.is_empty() {
            e.stack = ItemStack::empty();
            return true;
        }
        e.stack = left;
    }
    false
}

/// [`push_items_tick`] with the item-entity half of `suckInItems` armed.
///
/// `items` is `Some` exactly when vanilla would walk `getItemsAtAndAbove`:
/// there is no container above **and** the block above does not block
/// hoppers. A caller passing both `above` and `items` gets the container,
/// which is the order `suckInItems` tests them in.
#[allow(clippy::too_many_arguments)]
pub fn push_items_tick_with_items(
    hopper: &mut ContainerView,
    now: i64,
    enabled: bool,
    facing: Face,
    attached: Option<&mut ContainerView>,
    above: Option<&mut ContainerView>,
    items: Option<&mut Vec<ItemAbove>>,
    rules: &dyn Rules,
) -> TickOutcome {
    let clock = hopper.hopper.get_or_insert_with(HopperClock::default);
    clock.cooldown -= 1;
    clock.ticked_game_time = now;
    let on_cooldown = clock.cooldown > 0;
    if on_cooldown {
        return TickOutcome {
            moved: false,
            cooldown: clock.cooldown,
        };
    }
    // Offset 26: `setCooldown(0)` before the move, so the `isOnCooldown`
    // inside `tryMoveItems` is false however the decrement landed.
    clock.cooldown = 0;
    if !enabled {
        return TickOutcome {
            moved: false,
            cooldown: 0,
        };
    }
    let mut moved = false;
    if !hopper.is_empty() {
        if let Some(to) = attached {
            moved = eject_items(hopper, to, facing, rules);
        }
    }
    if !inventory_full(hopper) {
        if let Some(src) = above {
            moved |= suck_in_items(hopper, src, rules);
        } else if let Some(items) = items {
            let before = hopper.slots.clone();
            moved |= suck_in_item_entities(hopper, items, rules);
            // A partial absorption returns false and still wrote the
            // hopper's slots; the caller writes back on `changed`.
            if hopper.slots != before {
                hopper.changed = true;
            }
        }
    }
    if moved {
        // `setCooldown(8)` at offset 75 — and note this overwrites whatever
        // `tryMoveInItem` may have set on *this* hopper as an insert target
        // during the same call.
        if let Some(clock) = hopper.hopper.as_mut() {
            clock.cooldown = MOVE_ITEM_SPEED;
        }
        hopper.changed = true;
    }
    TickOutcome {
        moved,
        cooldown: hopper.hopper.map_or(0, |c| c.cooldown),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hopper() -> ContainerView {
        let mut c = ContainerView::empty(HOPPER_CONTAINER_SIZE);
        c.hopper = Some(HopperClock::default());
        c
    }

    fn chest() -> ContainerView {
        ContainerView::empty(27)
    }

    fn furnace() -> ContainerView {
        let mut c = ContainerView::empty(3);
        c.worldly = Worldly::Furnace;
        c
    }

    /// `SLOTS_FOR_DOWN = {2, 1}` and not `{1, 2}`, straight off the `static
    /// {}` block. The order decides which slot a hopper under a furnace
    /// empties first, so a test that only checked membership would pass on the
    /// wrong table.
    #[test]
    fn the_furnace_down_face_offers_the_result_slot_before_the_fuel_slot() {
        assert_eq!(
            Worldly::Furnace.slots_for_face(3, Face::Down),
            vec![2usize, 1]
        );
        assert_eq!(Worldly::Furnace.slots_for_face(3, Face::Up), vec![0usize]);
        assert_eq!(
            Worldly::Furnace.slots_for_face(3, Face::North),
            vec![1usize]
        );
        // Positive control: a plain container's faces are all the flat list,
        // so if `slots_for_face` were ignoring its argument the two asserts
        // above would still have to disagree with this one.
        assert_eq!(
            Worldly::None.slots_for_face(3, Face::Down),
            Worldly::None.slots_for_face(3, Face::Up)
        );
    }

    /// The brewing stand's side face ends in the **fuel** slot and its down
    /// face in the **ingredient** slot. Transposing them is the plausible
    /// error and this is the assert that catches it.
    #[test]
    fn the_brewing_stand_side_and_down_faces_differ_in_their_last_slot() {
        assert_eq!(
            Worldly::Brewing.slots_for_face(5, Face::Down),
            vec![0usize, 1, 2, 3]
        );
        assert_eq!(
            Worldly::Brewing.slots_for_face(5, Face::East),
            vec![0usize, 1, 2, 4]
        );
        assert_eq!(Worldly::Brewing.slots_for_face(5, Face::Up), vec![3usize]);
    }

    /// One item, hopper to chest, in one tick — and the cooldown it leaves.
    #[test]
    fn a_hopper_moves_one_item_into_the_chest_below_and_cools_for_eight() {
        let mut h = hopper();
        h.set_item(0, ItemStack::new("minecraft:stone", 3));
        let mut c = chest();
        let out = push_items_tick(&mut h, 100, true, Face::Down, Some(&mut c), None, &NoRules);
        assert!(out.moved);
        assert_eq!(out.cooldown, MOVE_ITEM_SPEED);
        assert_eq!(h.item(0).count(), 2, "one item left, not the whole stack");
        assert_eq!(c.item(0).item(), "minecraft:stone");
        assert_eq!(c.item(0).count(), 1);
        assert!(c.changed && h.changed);
    }

    /// The negative control for the test above: on cooldown, nothing moves and
    /// the cooldown counts down by exactly one.
    #[test]
    fn a_hopper_on_cooldown_moves_nothing_and_decrements_by_one() {
        let mut h = hopper();
        h.hopper = Some(HopperClock {
            cooldown: 8,
            ..HopperClock::default()
        });
        h.set_item(0, ItemStack::new("minecraft:stone", 3));
        let mut c = chest();
        let out = push_items_tick(&mut h, 100, true, Face::Down, Some(&mut c), None, &NoRules);
        assert!(!out.moved);
        assert_eq!(out.cooldown, 7);
        assert!(c.is_empty());
    }

    /// `cooldownTime` is decremented **before** `isOnCooldown` is tested, so a
    /// hopper sitting at 1 moves on this tick. Off by one here is the
    /// difference between eight ticks and nine between transfers.
    #[test]
    fn a_cooldown_of_one_is_spent_by_the_decrement_and_the_move_happens() {
        let mut h = hopper();
        h.hopper = Some(HopperClock {
            cooldown: 1,
            ..HopperClock::default()
        });
        h.set_item(0, ItemStack::one("minecraft:stone"));
        let mut c = chest();
        let out = push_items_tick(&mut h, 100, true, Face::Down, Some(&mut c), None, &NoRules);
        assert!(out.moved, "a cooldown of 1 does not survive its decrement");
    }

    /// `HopperBlock.ENABLED` false is checked in `tryMoveItems`, which is
    /// **after** the decrement and the `setCooldown(0)`. So a powered hopper
    /// still runs its clock down to zero and stays there.
    #[test]
    fn a_disabled_hopper_still_zeroes_its_cooldown_but_moves_nothing() {
        let mut h = hopper();
        h.hopper = Some(HopperClock {
            cooldown: 1,
            ..HopperClock::default()
        });
        h.set_item(0, ItemStack::one("minecraft:stone"));
        let mut c = chest();
        let out = push_items_tick(&mut h, 100, false, Face::Down, Some(&mut c), None, &NoRules);
        assert!(!out.moved);
        assert_eq!(out.cooldown, 0);
        assert!(c.is_empty());
    }

    /// Eject and suck are joined by `ior`, not `||`: a hopper that pushed an
    /// item down also pulls one in on the same tick.
    #[test]
    fn both_halves_run_in_one_tick() {
        let mut h = hopper();
        h.set_item(0, ItemStack::new("minecraft:stone", 2));
        let mut below = chest();
        let mut above = chest();
        above.set_item(0, ItemStack::new("minecraft:dirt", 5));
        let out = push_items_tick(
            &mut h,
            100,
            true,
            Face::Down,
            Some(&mut below),
            Some(&mut above),
            &NoRules,
        );
        assert!(out.moved);
        assert_eq!(below.item(0).count(), 1, "the eject half ran");
        assert_eq!(above.item(0).count(), 4, "the suck half ran too");
        assert_eq!(h.item(0).count(), 1);
        assert_eq!(h.item(1).item(), "minecraft:dirt");
    }

    /// A hopper feeding a hopper: the target's cooldown is 7 rather than 8
    /// when the source has already ticked this game time, which is the
    /// `8 - isSameTickOrLater` at offsets 178-185. Both hoppers here carry the
    /// same `tickedGameTime`, so the comparison is `>=` and answers true.
    #[test]
    fn a_hopper_inserting_into_an_empty_hopper_sets_seven_not_eight() {
        let mut src = hopper();
        src.hopper = Some(HopperClock {
            ticked_game_time: 40,
            ..HopperClock::default()
        });
        src.set_item(0, ItemStack::new("minecraft:stone", 2));
        let mut dst = hopper();
        dst.hopper = Some(HopperClock {
            ticked_game_time: 40,
            ..HopperClock::default()
        });
        assert!(eject_items(&mut src, &mut dst, Face::Down, &NoRules));
        assert_eq!(dst.hopper.unwrap().cooldown, 7);
    }

    /// The same insert into a hopper that is **not** empty sets no cooldown at
    /// all, because `isEmpty` is sampled before the write.
    #[test]
    fn inserting_into_a_non_empty_hopper_sets_no_cooldown() {
        let mut src = hopper();
        src.set_item(0, ItemStack::new("minecraft:stone", 2));
        let mut dst = hopper();
        dst.set_item(4, ItemStack::one("minecraft:dirt"));
        assert!(eject_items(&mut src, &mut dst, Face::Down, &NoRules));
        assert_eq!(dst.hopper.unwrap().cooldown, 0);
    }

    /// `canPlaceItem` refuses the furnace result slot outright and refuses the
    /// fuel slot too while no fuel table is loaded — which is
    /// [`NoRules`]' refusal, not a claim about coal.
    #[test]
    fn a_furnace_refuses_its_result_slot_and_norules_refuses_its_fuel_slot() {
        let f = furnace();
        let coal = ItemStack::one("minecraft:coal");
        assert!(!f.can_place_item(2, &coal, &NoRules));
        assert!(!f.can_place_item(1, &coal, &NoRules));
        assert!(f.can_place_item(0, &coal, &NoRules));
        // A bucket reaches the fuel slot without any table, from the second
        // disjunct — and stops reaching it once a bucket is already there.
        let bucket = ItemStack::one("minecraft:bucket");
        assert!(f.can_place_item(1, &bucket, &NoRules));
        let mut with_bucket = furnace();
        with_bucket.set_item(1, ItemStack::one("minecraft:bucket"));
        assert!(!with_bucket.can_place_item(1, &bucket, &NoRules));
    }

    /// `BrewingStandBlockEntity.canPlaceItem` under the rules the clock thread
    /// now passes: an ingredient into slot 3, blaze powder into slot 4, and a
    /// stick into neither. Under [`NoRules`] both answers were `false`, which
    /// is what a hopper over a brewing stand did on main.
    #[test]
    fn a_hopper_can_feed_a_brewing_stand_its_ingredient_and_its_fuel() {
        let mut stand = ContainerView::empty(5);
        stand.worldly = Worldly::Brewing;
        let tags = crate::recipes::ItemTags::parse(&serde_json::json!({
            "brewing_fuel": {"values": ["minecraft:blaze_powder"]}
        }))
        .expect("tags");
        let rules = VanillaRules {
            fuels: None,
            tags: Some(&tags),
        };
        let wart = ItemStack::one("minecraft:nether_wart");
        let blaze = ItemStack::one("minecraft:blaze_powder");
        let stick = ItemStack::one("minecraft:stick");
        assert!(stand.can_place_item(3, &wart, &rules));
        assert!(stand.can_place_item(4, &blaze, &rules));
        assert!(!stand.can_place_item(3, &stick, &rules));
        assert!(!stand.can_place_item(4, &wart, &rules));
        assert!(!stand.can_place_item(3, &wart, &NoRules));
        assert!(!stand.can_place_item(4, &blaze, &NoRules));
    }

    /// A hopper under a furnace may take the result but not the fuel, unless
    /// the fuel slot holds a bucket.
    #[test]
    fn the_furnace_down_face_yields_the_result_and_only_buckets_from_the_fuel() {
        let f = furnace();
        let iron = ItemStack::one("minecraft:iron_ingot");
        assert!(f.can_take_from(&iron, 2, Face::Down));
        assert!(!f.can_take_from(&iron, 1, Face::Down));
        assert!(f.can_take_from(&ItemStack::one("minecraft:bucket"), 1, Face::Down));
        // Positive control: the same slot through a side face is unrestricted,
        // so this is a face rule and not a slot rule.
        assert!(f.can_take_from(&iron, 1, Face::North));
    }

    /// `isFullContainer` is `>=` per slot over the face's slots. A chest with
    /// twenty-six full stacks and one empty slot is not full.
    #[test]
    fn one_empty_slot_makes_a_container_not_full() {
        let mut c = chest();
        for i in 0..26 {
            c.set_item(i, ItemStack::new("minecraft:stone", 64));
        }
        assert!(!is_full_container(&c, Face::Up));
        c.set_item(26, ItemStack::new("minecraft:stone", 64));
        assert!(is_full_container(&c, Face::Up));
    }

    /// A merge that has no room moves nothing and reports nothing moved — the
    /// `moved > 0` test at offset 102, which `canMergeItems` alone does not
    /// give because its own count test is `<=`.
    #[test]
    fn a_merge_with_no_room_moves_nothing() {
        let mut to = chest();
        to.set_item(0, ItemStack::new("minecraft:stone", 64));
        let left = try_move_in_item(
            None,
            &mut to,
            ItemStack::new("minecraft:stone", 5),
            0,
            Some(Face::Up),
            &NoRules,
        );
        assert_eq!(left.count(), 5);
        assert!(!to.changed);
    }

    /// The item-entity half: a whole stack goes in, the entity is emptied
    /// (the caller discards it) and the suck reports a move.
    #[test]
    fn a_hopper_with_nothing_above_absorbs_a_whole_item_entity() {
        let mut h = hopper();
        let mut items = vec![ItemAbove {
            id: 7,
            stack: ItemStack::one("minecraft:coal"),
        }];
        let out = push_items_tick_with_items(
            &mut h,
            100,
            true,
            Face::Down,
            None,
            None,
            Some(&mut items),
            &NoRules,
        );
        assert!(out.moved);
        assert_eq!(out.cooldown, MOVE_ITEM_SPEED);
        assert!(items[0].stack.is_empty());
        assert_eq!(h.item(0).item(), "minecraft:coal");
        // Positive control: the same hopper with the entity half disarmed
        // (a full block above) moves nothing.
        let mut h = hopper();
        let out =
            push_items_tick_with_items(&mut h, 100, true, Face::Down, None, None, None, &NoRules);
        assert!(!out.moved);
    }

    /// A partial absorption keeps the remainder on the entity, walks on, and
    /// returns false — so no cooldown — while the hopper did change.
    #[test]
    fn a_partly_absorbed_item_entity_sets_no_cooldown() {
        let mut h = hopper();
        for i in 0..HOPPER_CONTAINER_SIZE {
            h.set_item(i, ItemStack::new("minecraft:coal", 63));
        }
        let mut items = vec![ItemAbove {
            id: 1,
            stack: ItemStack::new("minecraft:coal", 10),
        }];
        let out = push_items_tick_with_items(
            &mut h,
            100,
            true,
            Face::Down,
            None,
            None,
            Some(&mut items),
            &NoRules,
        );
        assert!(!out.moved);
        assert_eq!(out.cooldown, 0);
        assert_eq!(items[0].stack.count(), 5);
        assert!(h.changed);
    }
}
