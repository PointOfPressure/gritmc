//! Where the player is, and the packets that say so.
//!
//! Reference (26.2): `ServerboundMovePlayerPacket` and its four subclasses,
//! read from the jar. Ids come from Mojang's own packet report
//! (`generated/reports/packets.json`), never from memory.
//!
//! # Why this exists
//!
//! The play loop drained the client's traffic and dropped it. A connection
//! stayed open and a player could walk around, but the server had no idea
//! where they were — so nothing downstream of position could be built:
//! no chunk streaming as they move, no tick that depends on who is near
//! what, no second player who can be told about the first.
//!
//! # The trailing byte is flags, not a bool
//!
//! Older protocol notes end every movement packet with an `onGround`
//! boolean. In 26.2 it is an **unsigned byte of flags**:
//!
//! ```text
//!   FLAG_ON_GROUND            = 1
//!   FLAG_HORIZONTAL_COLLISION = 2
//! ```
//!
//! read from `ServerboundMovePlayerPacket`'s constants. A port that reads a
//! bool gets the right answer for a player standing still and the wrong one
//! for a player walking into a wall, which is the kind of difference that
//! shows up as a desync days later.

use std::io;

use rustmc_proto::wire;

/// `FLAG_ON_GROUND`.
const FLAG_ON_GROUND: u8 = 1;
/// `FLAG_HORIZONTAL_COLLISION`.
const FLAG_HORIZONTAL_COLLISION: u8 = 2;

/// The translation key `handleAcceptTeleportPacket` disconnects with at offset
/// 31, and `handleMovePlayer` at offset 44.
///
/// A **translation key**, not a message: vanilla wraps it in
/// `Component.translatable`, so what goes on the wire is
/// `{translate: "..."}` and never `{text: "..."}`. The two serialise to
/// different bytes and the corpus compares bytes.
pub const INVALID_PLAYER_MOVEMENT: &str = "multiplayer.disconnect.invalid_player_movement";

/// Serverbound play ids this module decodes, from the packet report.
pub mod ids {
    pub const ACCEPT_TELEPORTATION: i32 = 0;
    pub const MOVE_PLAYER_POS: i32 = 30;
    pub const MOVE_PLAYER_POS_ROT: i32 = 31;
    pub const MOVE_PLAYER_ROT: i32 = 32;
    pub const MOVE_PLAYER_STATUS_ONLY: i32 = 33;
    /// `minecraft:player_input` — the seven movement keys as one byte, of
    /// which this server reads the sneak bit. Not
    /// `ServerboundPlayerCommandPacket$Action`, which is where every account
    /// older than 26.2 puts sneaking and which no longer carries it.
    pub const PLAYER_INPUT: i32 = 43;
    /// `minecraft:set_carried_item` — which hotbar slot is selected.
    pub const SET_CARRIED_ITEM: i32 = 53;
    /// `minecraft:set_creative_mode_slot` — a creative client putting an
    /// item somewhere, which is the only way anything gets into an inventory
    /// this server does not otherwise have.
    pub const SET_CREATIVE_MODE_SLOT: i32 = 56;
}

/// `InventoryMenu.USE_ROW_SLOT_START` — the container slot the hotbar starts
/// at, read from the class's own constants along with `USE_ROW_SLOT_END`
/// (45). A creative slot packet numbers slots in the *menu*, not the
/// inventory: 0 is the crafting result, 1-4 the grid, 5-8 armour, 9-35 the
/// main inventory, 36-44 the hotbar, 45 the offhand.
pub const HOTBAR_SLOT_START: i16 = 36;
/// `InventoryMenu`'s first main-inventory slot — the one after the four
/// armour slots. `Inventory`'s own index for the same stack is identical,
/// which is a coincidence of the two layouts agreeing on 9..36 and not
/// something to rely on for the hotbar, where they do not.
pub const MAIN_SLOT_START: i16 = 9;
/// The `Inventory` index the main inventory starts at — the same 9, named
/// separately because it is a different quantity that happens to be equal.
pub const MAIN_INVENTORY_START: usize = 9;
/// `Inventory.SELECTION_SIZE`.
pub const HOTBAR_SIZE: usize = 9;

/// `InventoryMenu.USE_ROW_SLOT_END` — the **menu** slot the offhand occupies,
/// one past the hotbar's last.
///
/// It is not an `Inventory` index and there is no `Inventory` index for it.
/// `Inventory.items` is 36 long and the offhand lives on the player's
/// `EntityEquipment` under `EquipmentSlot.OFFHAND`, reached from `Inventory`
/// only through `EQUIPMENT_SLOT_MAPPING`'s pseudo-index 40. This server holds
/// it as its own field on [`Hotbar`] for exactly that reason: a 37th array
/// cell would put it in `Inventory` order, where a container menu would then
/// show it as a 37th inventory slot.
pub const OFFHAND_SLOT: i16 = 45;

/// `Inventory.SLOT_OFFHAND` — the offhand's **`Inventory`** index, which is
/// not its menu slot ([`OFFHAND_SLOT`]). `getItem(40)` resolves it through
/// `EQUIPMENT_SLOT_MAPPING`.
pub const SLOT_OFFHAND: usize = 40;

/// Item name by network id, from `data/vanilla/items.json`.
///
/// The wire carries the id (`Item.STREAM_CODEC` is `holderRegistry`, which
/// resolves to `idMapper` and writes a plain varint), and the mapping is
/// registry data extracted from vanilla's own report — never transcribed.
/// See `ci/dump-items.sh`.
#[derive(Debug, Default)]
pub struct ItemIds(
    std::collections::BTreeMap<i32, String>,
    /// `minecraft:data_component_type` ids by name, from the same file's
    /// `data_component_types` object. Empty when the file predates it, and an
    /// empty table makes every component unreadable rather than misread.
    std::collections::BTreeMap<String, i32>,
);

impl ItemIds {
    /// # Errors
    /// If the file is missing or not the expected shape.
    pub fn load(path: &std::path::Path) -> io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let doc: serde_json::Value = serde_json::from_str(&text).map_err(io::Error::other)?;
        Self::parse(&doc)
    }

    /// The same table from an already-parsed document.
    ///
    /// Split out from [`Self::load`] so a caller that has the JSON does not
    /// have to put it on disk to be read straight back — which is what the
    /// tests below did, all of them through **one** path, and it made this
    /// module's tests fail one run in three. `std::fs::write` truncates
    /// before it writes, `cargo test` runs a module's tests on parallel
    /// threads in one process, and a read that lands inside another test's
    /// truncate gets an empty file and a parse error. The race was in the
    /// harness, not in the code under test, which is the worst place for it:
    /// it made a green gate a coin flip for all three tracks.
    ///
    /// # Errors
    /// If the document is not the expected shape.
    pub fn parse(doc: &serde_json::Value) -> io::Result<Self> {
        let map = doc
            .get("items")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| io::Error::other("items.json has no `items` object"))?;
        let mut by_id = std::collections::BTreeMap::new();
        for (name, id) in map {
            let id = i32::try_from(
                id.as_i64()
                    .ok_or_else(|| io::Error::other(format!("{name} has no numeric id")))?,
            )
            .map_err(io::Error::other)?;
            by_id.insert(id, name.clone());
        }
        if by_id.is_empty() {
            return Err(io::Error::other("no items in items.json"));
        }
        let components = doc
            .get("data_component_types")
            .and_then(serde_json::Value::as_object)
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| Some((k.clone(), i32::try_from(v.as_i64()?).ok()?)))
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self(by_id, components))
    }

    /// A data component type's registry id, by name.
    #[must_use]
    pub fn component_type(&self, name: &str) -> Option<i32> {
        self.1.get(name).copied()
    }

    #[must_use]
    pub fn name(&self, id: i32) -> Option<&str> {
        self.0.get(&id).map(String::as_str)
    }

    /// A data component type's name, by registry id — the reverse of
    /// [`Self::component_type`], a scan for the reason [`Self::id_of`] is.
    #[must_use]
    pub fn component_type_name(&self, id: i32) -> Option<&str> {
        self.1
            .iter()
            .find_map(|(n, i)| (*i == id).then_some(n.as_str()))
    }

    /// The registry id of a name — the reverse of [`Self::name`].
    ///
    /// A scan rather than a second map: this runs once per slot of a menu
    /// that has just been opened, which is a few dozen lookups over ~1,500
    /// entries at human pace, and a second index would be a second thing that
    /// can disagree with the first.
    #[must_use]
    pub fn id_of(&self, name: &str) -> Option<i32> {
        self.0.iter().find_map(|(id, n)| (n == name).then_some(*id))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// `Inventory.getContainerSize()`'s main half: nine hotbar slots and the
/// twenty-seven above them.
///
/// The armour slots and the offhand are **not** here. `Inventory` has 41 and
/// this has 36, because `addStandardInventorySlots` — the method every
/// container menu calls — adds exactly these 36 and no others. The offhand is
/// reachable through `swap_offhand`, which this server answers from
/// elsewhere; armour has no path into it at all yet. Named rather than left
/// as an off-by-five.
pub const INVENTORY_SIZE: usize = 36;

/// What the player is carrying: `Inventory`'s 36 slots, in its own index
/// order — **hotbar first**.
///
/// # Why it is 36 slots now and was 9
///
/// It began as the hotbar alone, because `use_item_on` says *where* a player
/// clicked and never *what* with, and nine slots is everything a player can
/// select without opening anything. A container menu is the first thing that
/// can reach the other twenty-seven: `addStandardInventorySlots` puts all 36
/// into every menu, `broadcastChanges` walks all of them, and a click can
/// move a stack into one. Twenty-seven slots that did not exist would read to
/// the client as twenty-seven slots that were empty, and the first thing a
/// player did with them would be silently undone.
///
/// # The index order is vanilla's and it is not the menu's
///
/// `Inventory` numbers the **hotbar 0..9** and the main inventory 9..36.
/// Every menu numbers them the other way round, main inventory first — see
/// [`crate::menu`]. Two orders for the same 36 stacks, and the conversion
/// between them is the single most likely place for an off-by-27, so it lives
/// in exactly one function and is tested against both menu shapes.
///
/// # Counts
///
/// Stacks, not names. The count was read off `set_creative_mode_slot` and
/// thrown away for as long as nothing could use it; `doClick`'s PICKUP branch
/// is almost entirely arithmetic on counts, so a container built on names
/// could not half-stack, overflow or merge. See [`crate::item`].
#[derive(Debug, Clone)]
pub struct Hotbar {
    items: [crate::item::ItemStack; INVENTORY_SIZE],
    selected: usize,
    /// `EntityEquipment`'s `OFFHAND` entry — outside `items` because it is
    /// outside `Inventory.items` in vanilla too. See [`OFFHAND_SLOT`].
    offhand: crate::item::ItemStack,
    /// Every item's default components, or `None` for a checkout with no
    /// vanilla extract.
    ///
    /// **Not an ambient lookup.** CRATES.md rule 1 forbids ambient
    /// singletons, so the table arrives through
    /// [`Hotbar::with_item_props`] at the one production construction site
    /// (`crate::status::listen`) exactly as `shapes`, `placed_by`,
    /// `sound_types` and `ticks` arrive at a `Session`. `None` is the
    /// [`Default`] and every existing test keeps it, which is why adding this
    /// field changed no call site: those tests hold cobblestone, and
    /// cobblestone stacks to 64 either way.
    props: Option<&'static crate::itemprops::ItemProps>,
}

impl Default for Hotbar {
    fn default() -> Self {
        Self {
            items: std::array::from_fn(|_| crate::item::ItemStack::empty()),
            selected: 0,
            offhand: crate::item::ItemStack::empty(),
            props: None,
        }
    }
}

impl Hotbar {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Install the item-props table. See [`Self::props`].
    #[must_use]
    pub fn with_item_props(mut self, props: Option<&'static crate::itemprops::ItemProps>) -> Self {
        self.props = props;
        self
    }

    /// The item-props table this inventory was built with, for a caller that
    /// needs a column of it and has a `Hotbar` in scope but no `Session`
    /// field. `crate::world_send`'s placement path is the one such caller.
    #[must_use]
    pub const fn item_props(&self) -> Option<&'static crate::itemprops::ItemProps> {
        self.props
    }

    /// `ItemStack.getMaxStackSize()` for a stack in **this** inventory.
    ///
    /// `ItemInstance.getMaxStackSize()` is
    /// `getOrDefault(DataComponents.MAX_STACK_SIZE, 1)` and a stack's
    /// components are its item's defaults until something patches them, so
    /// with the table this is the whole of vanilla's answer. Without it, the
    /// old constant — which is 64 for everything and wrong for 295 of the
    /// 1,537 items in the registry.
    ///
    /// **The empty stack is 64 and that is not a special case being invented
    /// here.** `ItemStack.EMPTY` is `Items.AIR`, and `minecraft:air` carries
    /// `max_stack_size = 64` in the extract (checked, not assumed). This
    /// crate represents empty as a stack with no name, so the name has to be
    /// supplied; getting it wrong would make `is_stackable` false for an
    /// empty stack, and `moveItemStackTo` tests `isStackable` *once, outside*
    /// its merge loop.
    fn max_stack_size(&self, stack: &crate::item::ItemStack) -> i32 {
        let Some(props) = self.props else {
            return stack.max_stack_size();
        };
        if stack.is_empty() {
            return props.max_stack_size("minecraft:air");
        }
        // A patched `max_stack_size` wins over the prototype's.
        stack
            .patched_max_stack_size()
            .unwrap_or_else(|| props.max_stack_size(stack.item()))
    }

    /// `ItemStack.isStackable()` — offsets 0-27, with the table's stack size.
    ///
    /// The second conjunct stays vacuously true for the reason
    /// [`crate::item::ItemStack::is_stackable`] gives: nothing this server
    /// produces carries a `DataComponents.DAMAGE` *patch*, so no stack is
    /// damaged, so `!isDamaged()` holds however damageable the item is. The
    /// table makes `isDamageableItem` answerable — see
    /// [`crate::itemprops::ItemProps::is_damageable_item`] — and it is
    /// deliberately not consulted here, because doing so would change the
    /// answer for every tool from vanilla's to a wrong one.
    fn is_stackable(&self, stack: &crate::item::ItemStack) -> bool {
        self.max_stack_size(stack) > 1
    }

    /// The item in the selected slot, by name — what a placement needs.
    ///
    /// `None` for an empty slot, which is what every existing caller reads it
    /// as. The count is not part of this question: `use_item_on` places one
    /// block and creative does not consume.
    #[must_use]
    pub fn held(&self) -> Option<&str> {
        let s = &self.items[self.selected];
        (!s.is_empty()).then(|| s.item())
    }

    /// The selected hotbar stack itself, components and all, or `None` when
    /// the slot is empty.
    #[must_use]
    pub fn held_stack(&self) -> Option<&crate::item::ItemStack> {
        let s = &self.items[self.selected];
        (!s.is_empty()).then_some(s)
    }

    #[must_use]
    pub const fn selected(&self) -> usize {
        self.selected
    }

    /// Put a named item in a hotbar slot, one of it.
    ///
    /// Kept taking a name so the existing creative-slot path and its tests
    /// read unchanged; [`Self::set_stack`] is the one that carries a count.
    pub fn set(&mut self, slot: usize, item: Option<&str>) {
        self.set_stack(
            slot,
            item.map_or_else(crate::item::ItemStack::empty, crate::item::ItemStack::one),
        );
    }

    /// The stack at an **`Inventory`** index — hotbar 0..9, main 9..36.
    #[must_use]
    pub fn stack(&self, slot: usize) -> crate::item::ItemStack {
        self.items
            .get(slot)
            .cloned()
            .unwrap_or_else(crate::item::ItemStack::empty)
    }

    /// Write an **`Inventory`** index. Out of range is dropped rather than
    /// wrapped: a bad index is a bug and putting the stack somewhere else
    /// would hide it.
    pub fn set_stack(&mut self, slot: usize, stack: crate::item::ItemStack) {
        if let Some(s) = self.items.get_mut(slot) {
            *s = stack;
        }
    }

    /// `Player.getItemInHand(OFF_HAND)`.
    #[must_use]
    pub fn offhand(&self) -> crate::item::ItemStack {
        self.offhand.clone()
    }

    /// `Player.setItemInHand(OFF_HAND, stack)`.
    pub fn set_offhand(&mut self, stack: crate::item::ItemStack) {
        self.offhand = stack;
    }

    /// `handlePlayerAction`'s `SWAP_ITEM_WITH_OFFHAND` arm, offsets 160-221:
    ///
    /// ```text
    ///   ItemStack tmp = player.getItemInHand(OFF_HAND);
    ///   player.setItemInHand(OFF_HAND, player.getItemInHand(MAIN_HAND));
    ///   player.setItemInHand(MAIN_HAND, tmp);
    ///   player.stopUsingItem();
    /// ```
    ///
    /// `getItemInHand(MAIN_HAND)` is `Inventory.getSelectedItem()`, so the
    /// hotbar cell this touches is the **selected** one and not slot 0.
    ///
    /// `stopUsingItem` has nothing to stop here: this server models no item
    /// use duration, and `LivingEntity.stopUsingItem` on a player who is not
    /// using an item clears two fields and sends nothing.
    ///
    /// Returns the two `Inventory`-side handles that changed, for a caller
    /// that has to turn them into `container_set_slot` packets — the selected
    /// hotbar index, then the offhand. Both are returned even when one of
    /// them did not really change; deciding that is
    /// `RemoteSlot.matches`' job and it belongs at the packet layer, where
    /// the belief lives.
    pub fn swap_offhand(&mut self) -> (usize, i16) {
        let selected = self.selected;
        let tmp = self.offhand.clone();
        self.offhand = self.stack(selected);
        self.set_stack(selected, tmp);
        (selected, OFFHAND_SLOT)
    }

    /// The hotbar's nine, by name, in `Inventory` order.
    ///
    /// `addStandardInventorySlots` puts the hotbar **last** in any menu, so
    /// this is what fills the final nine slots of a container's item list.
    #[must_use]
    pub fn slots(&self) -> [Option<&str>; HOTBAR_SIZE] {
        std::array::from_fn(|i| {
            let s = &self.items[i];
            (!s.is_empty()).then(|| s.item())
        })
    }

    /// `Inventory.hasRemainingSpaceForItem(ItemStack, ItemStack)` — offsets
    /// 0-39, four conjuncts, in vanilla's order.
    ///
    /// **The first conjunct is the one that carries the method.** `existing`
    /// must be non-empty: a free slot is [`Self::get_free_slot`]'s answer and
    /// not this one, and merging the two predicates would let
    /// [`Self::place_item_back_in_inventory`] "top up" an empty slot and so
    /// skip the `copyWithCount(0)` inside [`Self::add`] that gives that slot
    /// its item in the first place.
    ///
    /// Both the count and the maximum are read off `existing`, not off the
    /// arriving stack — offsets 22-31 are `aload_1` twice.
    fn has_remaining_space_for_item(
        &self,
        existing: &crate::item::ItemStack,
        stack: &crate::item::ItemStack,
    ) -> bool {
        !existing.is_empty()
            && crate::item::ItemStack::is_same_item_same_components(existing, stack)
            && self.is_stackable(existing)
            && existing.count() < self.max_stack_size(existing)
    }

    /// `Inventory.getFreeSlot()` — the first empty slot, or `None` for `-1`.
    #[must_use]
    pub fn get_free_slot(&self) -> Option<usize> {
        self.items.iter().position(crate::item::ItemStack::is_empty)
    }

    /// `Inventory.getSlotWithRemainingSpace(ItemStack)` — offsets 0-79.
    ///
    /// Three probes, and **the order is observable**: the *selected* slot
    /// first, then the offhand, then a forward scan from 0. A scan-only
    /// implementation would put a returning stack in the lowest matching slot
    /// rather than back in the hand it came out of.
    ///
    /// The second probe is `getItem(40)` (offsets 21-37), and 40 is
    /// [`SLOT_OFFHAND`] — outside `Inventory.items`, which is 36 long, and
    /// resolved through `EQUIPMENT_SLOT_MAPPING` onto the offhand this type
    /// carries beside `items`. So the answer can be `Some(40)`, and every
    /// caller reads and writes it through [`Self::get_item`] /
    /// [`Self::set_item`], which route 40 there. Until 2026-10-01 this probe
    /// was missing and a stack vanilla tops up in the offhand was topped up by
    /// the scan instead; `swap_offhand` makes the offhand reachable from the
    /// corpus.
    #[must_use]
    pub fn get_slot_with_remaining_space(&self, stack: &crate::item::ItemStack) -> Option<usize> {
        if self.has_remaining_space_for_item(&self.items[self.selected], stack) {
            return Some(self.selected);
        }
        if self.has_remaining_space_for_item(&self.offhand, stack) {
            return Some(SLOT_OFFHAND);
        }
        self.items
            .iter()
            .position(|existing| self.has_remaining_space_for_item(existing, stack))
    }

    /// `Inventory.getItem(int)` — 0..36 are `items`, and 40 is the offhand
    /// through `EQUIPMENT_SLOT_MAPPING`. The armour indices 36..40 have no
    /// storage here and read empty.
    #[must_use]
    pub fn get_item(&self, slot: usize) -> crate::item::ItemStack {
        if slot == SLOT_OFFHAND {
            return self.offhand.clone();
        }
        self.stack(slot)
    }

    /// `Inventory.setItem(int, ItemStack)`, with the same routing as
    /// [`Self::get_item`].
    pub fn set_item(&mut self, slot: usize, stack: crate::item::ItemStack) {
        if slot == SLOT_OFFHAND {
            self.offhand = stack;
            return;
        }
        self.set_stack(slot, stack);
    }

    /// `Inventory.removeFromSelected(boolean)` — offsets 0-36:
    /// `getSelectedItem().isEmpty() ? EMPTY : removeItem(selected, all ?
    /// count : 1)`, and `removeItem` on a hotbar index is
    /// `ContainerHelper.removeItem`, a `split` of the slot's own stack.
    ///
    /// What `ServerPlayer.drop(boolean)` throws. It sends nothing: the
    /// `findSlot(...).ifPresent(setRemoteSlot)` that follows it in `drop`
    /// moves the menu's belief to the new contents, so the next
    /// `broadcastChanges` finds nothing to correct.
    pub fn remove_from_selected(&mut self, all: bool) -> crate::item::ItemStack {
        let selected = self.selected;
        let mut held = self.stack(selected);
        if held.is_empty() {
            return crate::item::ItemStack::empty();
        }
        let amount = if all { held.count() } else { 1 };
        let taken = held.split(amount);
        self.set_stack(selected, held);
        taken
    }

    /// `Inventory.addResource(int, ItemStack)` — offsets 0-82. Returns what
    /// is left of `count` after topping up `slot`.
    ///
    /// The empty-slot `copyWithCount(0)` step is written as the arriving item
    /// with a count of zero carried beside it, for the reason
    /// [`Self::add`]'s body gives: a zero-count named stack is not
    /// representable in [`crate::item::ItemStack`].
    fn add_resource_at(&mut self, slot: usize, stack: &crate::item::ItemStack) -> i32 {
        let count = stack.count();
        let existing = self.get_item(slot);
        let (item, have) = if existing.is_empty() {
            (stack.item().to_string(), 0)
        } else {
            (existing.item().to_string(), existing.count())
        };
        // `getMaxStackSize(itemStack)` is `Inventory`'s two-argument
        // `Container` default, `min(getMaxStackSize() = 99,
        // stack.getMaxStackSize())`, read off the slot's (copied) stack —
        // which is the arriving item either way.
        let room = self.max_stack_size(stack).min(99) - have;
        let k = count.min(room);
        if k == 0 {
            // Offsets 56-62: `return i` — and the `setItem` of the empty
            // copy at 27-31 has already happened in vanilla. A zero-count
            // stack is empty, so leaving the slot untouched is the same slot.
            return count;
        }
        self.set_item(slot, crate::item::ItemStack::new(&item, have + k));
        count - k
    }

    /// `Inventory.addResource(ItemStack)` — offsets 0-32: the slot with room,
    /// else the first free one, else nothing moves.
    fn add_resource(&mut self, stack: &crate::item::ItemStack) -> (i32, Option<usize>) {
        let Some(slot) = self
            .get_slot_with_remaining_space(stack)
            .or_else(|| self.get_free_slot())
        else {
            return (stack.count(), None);
        };
        (self.add_resource_at(slot, stack), Some(slot))
    }

    /// `Inventory.add(ItemStack)`, which is `add(-1, stack)` — offsets
    /// 0-101 — **mutating `stack` in place**, exactly as vanilla's does to
    /// the stack an `ItemEntity` holds.
    ///
    /// Returns what the bytecode returns, and that is not "did anything
    /// move". `i` is re-read at the top of every pass of the `do/while`
    /// (offset 16, `istore_3` inside the loop), and the loop exits on the
    /// first pass that makes no progress. So:
    ///
    /// * a stack that fits completely returns `true`;
    /// * a stack of which **some** fits and the rest does not returns
    ///   **`false`** in survival — the last pass moved nothing, so
    ///   `getCount() < i` is false — even though the first pass moved items.
    ///   `ItemEntity.playerTouch` then sends no `take_item_entity` and does
    ///   not discard, while the inventory has grown and the entity's stack
    ///   has shrunk underneath it;
    /// * the creative tail, offsets 63-87: when the last pass moved nothing
    ///   and `hasInfiniteMaterials()`, the stack is set to 0 and the answer
    ///   is `true` — what did not fit is destroyed.
    ///
    /// The second element is the `Inventory` indices written, in write
    /// order, for a caller that has to turn them into `container_set_slot`.
    ///
    /// The `isDamaged()` branch (offsets 102-164) is unreachable: nothing
    /// this server produces carries a `minecraft:damage` patch — see
    /// [`crate::item::ItemStack::is_same_item_same_components`].
    pub fn add_mut(
        &mut self,
        stack: &mut crate::item::ItemStack,
        has_infinite_materials: bool,
    ) -> (bool, Vec<usize>) {
        let mut touched = Vec::new();
        if stack.is_empty() {
            return (false, touched);
        }
        let mut i;
        loop {
            i = stack.count();
            let (left, slot) = self.add_resource(stack);
            if let Some(slot) = slot {
                if left != i && !touched.contains(&slot) {
                    touched.push(slot);
                }
            }
            if left == 0 {
                *stack = crate::item::ItemStack::empty();
                break;
            }
            stack.set_count(left);
            if stack.count() >= i {
                break;
            }
        }
        if stack.count() == i && has_infinite_materials {
            *stack = crate::item::ItemStack::empty();
            return (true, touched);
        }
        (stack.count() < i, touched)
    }

    /// `Inventory.add(int, ItemStack)` — offsets 0-101 — over the one shape
    /// [`Self::place_item_back_in_inventory`] can hand it.
    ///
    /// Vanilla's `add` wraps `addResource` in a `do/while` that re-enters
    /// only while the stack is both non-empty and still shrinking, which is
    /// how a stack larger than one slot's room gets spread over several.
    /// **It cannot re-enter from this caller**: `placeItemBackInInventory`
    /// has already `split` the stack down to exactly the room `slot` has, so
    /// the first pass consumes all of it. Written as the single pass it is,
    /// rather than as a loop whose second iteration is dead.
    ///
    /// The `isDamaged()` branch above it — which routes a damaged stack to
    /// `addResource(-1, …)` — is unreachable for the reason
    /// [`crate::item::ItemStack::is_same_item_same_components`] gives: nothing
    /// this server produces carries a component patch, so no stack is damaged.
    ///
    /// `setPopTime(5)` is dropped. It is a client-side render bob and reaches
    /// no packet.
    ///
    /// Returns whether anything moved, which is vanilla's `count` comparison
    /// at offsets 88-101 and is what decides whether a
    /// `ClientboundSetPlayerInventoryPacket` goes out.
    fn add(&mut self, slot: usize, stack: &crate::item::ItemStack) -> bool {
        if stack.is_empty() {
            return false;
        }
        // `addResource(int, ItemStack)` offsets 12-33: an empty target slot
        // first becomes a **zero-count copy of what is arriving**
        // (`copyWithCount(0)` and `setItem`), so that the grow below has an
        // item to grow.
        //
        // **That step cannot be transcribed literally here, and the reason is
        // this crate's own invariant rather than a shortcut.**
        // [`crate::item::ItemStack::new`] collapses a count of zero to
        // [`crate::item::ItemStack::empty`] on purpose — so that `is_empty`
        // and `item` can never disagree about whether there is anything there
        // — and `copy_with_count(0)` therefore returns a stack with **no
        // item**. Growing that yields a count of 1 of nothing, which is
        // exactly what it did before this comment existed: the free-slot test
        // below caught `ItemStack { item: "", count: 1 }`.
        //
        // So the empty case is written as what vanilla's two steps come to
        // once a zero-count named stack is unrepresentable: the slot becomes
        // `take` of the arriving item. The name and the count are carried
        // separately for the same reason.
        // The name and the count are carried with the arriving stack's
        // COMPONENTS as a template: rebuilding the cell from the item name
        // dropped a given tool's `minecraft:damage` and enchantments on the
        // way in — survival 048 recorded a pickaxe that never broke and 050 a
        // Silk Touch shovel with no Silk Touch (2026-10-01).
        let existing = self.stack(slot);
        let (template, have) = if existing.is_empty() {
            (stack.clone(), 0)
        } else {
            let have = existing.count();
            (existing, have)
        };
        // Vanilla reads the maximum off the *target* stack — which after the
        // `copyWithCount` is the arriving item either way, so reading it off
        // the arriving one agrees with vanilla and does not depend on the
        // maximum being a constant. It no longer is: [`Self::max_stack_size`]
        // reads the extract, so a bucket tops out at 16 here and a filled one
        // at 1.
        let room = self.max_stack_size(stack) - have;
        let take = stack.count().min(room);
        if take == 0 {
            return false;
        }
        self.set_stack(slot, template.copy_with_count(have + take));
        true
    }

    /// `Inventory.placeItemBackInInventory(ItemStack)` — which is the
    /// two-argument form with `sendPacket = true` (offsets 0-6).
    ///
    /// The loop is vanilla's, offsets 0-111: while the stack is non-empty,
    /// find a slot with room, else a free one, else **drop what is left** and
    /// stop; otherwise move `maxStackSize - getItem(slot).getCount()` of it
    /// into that slot and go round again.
    ///
    /// # What comes back
    ///
    /// [`PlacedBack::touched`] is the `Inventory` indices written, in the
    /// order they were written — one per `ClientboundSetPlayerInventoryPacket`
    /// vanilla sends from offsets 95-105, and the caller sends them because
    /// this type has no connection to send them on.
    ///
    /// [`PlacedBack::leftover`] is what did not fit. Vanilla's answer at
    /// offsets 28-38 is `player.drop(stack, false)`; deciding that is the
    /// caller's, and returning it rather than discarding it is what keeps
    /// this method from being able to destroy an item.
    pub fn place_item_back_in_inventory(
        &mut self,
        mut stack: crate::item::ItemStack,
    ) -> PlacedBack {
        let mut touched = Vec::new();
        while !stack.is_empty() {
            let Some(slot) = self
                .get_slot_with_remaining_space(&stack)
                .or_else(|| self.get_free_slot())
            else {
                return PlacedBack {
                    touched,
                    leftover: stack,
                };
            };
            let room = self.max_stack_size(&stack) - self.get_item(slot).count();
            let moved = stack.split(room);
            // **Not vanilla's, and here so the loop is provably finite rather
            // than finite by argument.** Both probes above guarantee room > 0
            // — one returns only a slot whose count is under the maximum, the
            // other only an empty slot — so `moved` cannot be empty and this
            // never fires. Vanilla relies on exactly that and would spin here
            // too; a spin inside a packet handler is not a failure worth
            // reproducing.
            if moved.is_empty() {
                return PlacedBack {
                    touched,
                    leftover: stack,
                };
            }
            if self.add(slot, &moved) {
                touched.push(slot);
            }
        }
        PlacedBack {
            touched,
            leftover: crate::item::ItemStack::empty(),
        }
    }

    /// `Inventory.add(ItemStack)`, which is `add(-1, stack)` — what
    /// `Player.addItem` calls.
    ///
    /// # It is the same loop as [`Self::place_item_back_in_inventory`], and
    /// the difference that matters is the caller's
    ///
    /// `add(-1, …)` is a `do/while` over `addResource(stack)`, which is
    /// `getSlotWithRemainingSpace` then `getFreeSlot` then move
    /// `min(room, count)`; `placeItemBackInInventory` is a `while` over the
    /// same two probes that `split`s exactly the room it found. Both probes
    /// only ever return a slot with room, so the split always consumes and
    /// both loops write the same slots in the same order for any input. The
    /// two are therefore one implementation here rather than a transcription
    /// and a near-copy of it.
    ///
    /// **What is not shared is what goes on the wire.**
    /// `placeItemBackInInventory` sends a `ClientboundSetPlayerInventoryPacket`
    /// per touched slot from inside its own loop, at offsets 95-105;
    /// `add` sends nothing at all, and the slot reaches the client from the
    /// next `broadcastChanges` sweep of the player's own menu instead. That is
    /// how the two were told apart in the recording of the flower pot:
    /// vanilla's `container_set_slot` there arrives with **no**
    /// `set_player_inventory` beside it.
    ///
    /// # Two branches of vanilla's `add` are unreachable here
    ///
    /// * `isDamaged()`, which routes to a free slot with no merging. Nothing
    ///   this server produces carries a component patch — see
    ///   [`crate::item::ItemStack::is_same_item_same_components`] — so no
    ///   stack is damaged.
    /// * the creative tail, `if (count == before && hasInfiniteMaterials())
    ///   { setCount(0); return true; }`. It fires only when **nothing** moved,
    ///   which needs 36 full slots, and it makes the leftover vanish rather
    ///   than be dropped. [`PlacedBack::leftover`] therefore over-reports by
    ///   one case: a creative player with a full inventory. The gameplay
    ///   corpus cannot build one — its `hold` verb sends `count = 1` — and
    ///   naming it is cheaper than modelling abilities in this type.
    pub fn add_item(&mut self, stack: crate::item::ItemStack) -> PlacedBack {
        self.place_item_back_in_inventory(stack)
    }
}

/// What [`Hotbar::place_item_back_in_inventory`] did, for a caller that has to
/// turn it into packets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacedBack {
    /// `Inventory` indices written, in write order. One
    /// `ClientboundSetPlayerInventoryPacket` each.
    pub touched: Vec<usize>,
    /// What would not fit anywhere. Empty unless the inventory is full.
    pub leftover: crate::item::ItemStack,
}

/// Decode the two packets that decide what a player is holding.
///
/// Returns whether the packet was one of them. `items` is the registry the
/// ids resolve against; without it a creative slot is *cleared* rather than
/// guessed, because a wrong item silently places the wrong block.
///
/// # Errors
/// If a packet this handles is truncated.
pub fn apply_hand(
    id: i32,
    body: &[u8],
    hotbar: &mut Hotbar,
    items: Option<&ItemIds>,
) -> io::Result<bool> {
    let mut cur = body;
    match id {
        ids::SET_CARRIED_ITEM => {
            // A **short**, not a varint — `FriendlyByteBuf.readShort` in the
            // packet's own constructor.
            #[expect(clippy::cast_possible_wrap, reason = "readShort is signed")]
            let slot = wire::read_u16(&mut cur)? as i16;
            if (0..HOTBAR_SIZE as i16).contains(&slot) {
                hotbar.selected = usize::try_from(slot).unwrap_or(0);
            }
            Ok(true)
        }
        ids::SET_CREATIVE_MODE_SLOT => {
            #[expect(clippy::cast_possible_wrap, reason = "readShort is signed")]
            let slot = wire::read_u16(&mut cur)? as i16;
            // `ItemStack`'s optional codec: a varint count, and zero or less
            // means empty and nothing follows. Otherwise the item id, then a
            // component patch this server has no use for and does not read —
            // safe only because the stack is the last thing in the body.
            let count = rustmc_proto::varint::read_varint(&mut cur)?;
            let stack = if count > 0 {
                let item_id = rustmc_proto::varint::read_varint(&mut cur)?;
                // **The count is kept now.** It was read and dropped for as
                // long as nothing could use it; a container click is
                // arithmetic on counts, so dropping it here would make every
                // stack in the world a stack of one however it arrived.
                let stack = items
                    .and_then(|t| t.name(item_id))
                    .map_or_else(crate::item::ItemStack::empty, |name| {
                        crate::item::ItemStack::new(name, count)
                    });
                match items.and_then(|t| read_untrusted_patch(&mut cur, t)) {
                    Some((patch, bed, ed)) => {
                        let stack = stack.with_patch(patch);
                        let stack = match bed {
                            Some((type_id, tag)) => stack.with_block_entity_data(type_id, tag),
                            None => stack,
                        };
                        match ed {
                            Some(d) => stack.with_entity_data(d),
                            None => stack,
                        }
                    }
                    None => stack,
                }
            } else {
                crate::item::ItemStack::empty()
            };
            // Two ranges, because a creative client can fill the main
            // inventory as well as the hotbar and the menu numbers them
            // apart: `InventoryMenu` slots 9..36 are `Inventory` 9..36, and
            // slots 36..45 are `Inventory` 0..9. Only the second used to be
            // handled, which was right when there were only nine slots to
            // land in and would now silently discard anything put in the
            // other twenty-seven.
            if (HOTBAR_SLOT_START..HOTBAR_SLOT_START + HOTBAR_SIZE as i16).contains(&slot) {
                let index = usize::try_from(slot - HOTBAR_SLOT_START).unwrap_or(0);
                hotbar.set_stack(index, stack);
            } else if (MAIN_SLOT_START..HOTBAR_SLOT_START).contains(&slot) {
                let index = usize::try_from(slot).unwrap_or(MAIN_INVENTORY_START);
                hotbar.set_stack(index, stack);
            } else if slot == OFFHAND_SLOT {
                // A third range, added with the offhand itself. It used to
                // fall through and be discarded, which was honest while
                // nothing could read an offhand and is not any more: a
                // creative client can drop a stack into slot 45 and a
                // `SWAP_ITEM_WITH_OFFHAND` would then swap the wrong thing
                // out of it. `EQUIPMENT_SLOT_MAPPING` is what makes this a
                // separate arm rather than a wider range — 45 is not
                // `Inventory` index 45, it is not an `Inventory` index at
                // all.
                hotbar.set_offhand(stack);
            }
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// A serverbound `ItemStack`'s component patch, kept whole, and
/// `minecraft:block_entity_data` out of it if it carries one.
///
/// `ServerboundSetCreativeModeSlotPacket` reads its stack through
/// `ItemStack.OPTIONAL_UNTRUSTED_STREAM_CODEC`, which is built on
/// `DataComponentPatch.DELIMITED_STREAM_CODEC`: a varint count of added
/// components, a varint count of removed ones, then each added one as its
/// type id and **a varint byte length** followed by that many bytes of the
/// type's own codec (`ByteBufCodecs.registryFriendlyLengthPrefixed`), then the
/// removed type ids.
///
/// **Every component is kept, decoded or not.** The length prefix is the
/// only framing the delimited form adds; strip it and what is left is
/// exactly what `DataComponentPatch.STREAM_CODEC` writes clientbound for the
/// same value, because both are that type's own stream codec. So a stack a
/// creative client puts in a slot goes back out in every content packet that
/// shows the slot — a damaged sword, an enchanted book, a named item —
/// without this server knowing any of their codecs. Vanilla decodes and
/// re-encodes instead; the two agree wherever re-encoding is the identity,
/// which it is for every codec here except a hash-ordered map with more than
/// one entry (`ItemEnchantments` with two enchantments, a compound with two
/// keys), whose order the client and the server may walk differently.
///
/// The one component decoded is `TypedEntityData.streamCodec`: the
/// block-entity type through `ByteBufCodecs.registry(BLOCK_ENTITY_TYPE)` (a
/// varint) and then `ByteBufCodecs.COMPOUND_TAG` — nameless network NBT, kept
/// as bytes for placement.
///
/// `minecraft:entity_data` is decoded too, to [`crate::item::EntityData`]:
/// `TypedEntityData.streamCodec(EntityType.STREAM_CODEC)` is the entity
/// type's registry id, then a network compound whose `Silent` byte is the one
/// key a spawn egg here applies.
#[expect(
    clippy::type_complexity,
    reason = "three independent reads of one patch"
)]
fn read_untrusted_patch(
    cur: &mut &[u8],
    items: &ItemIds,
) -> Option<(
    crate::patch::Patch,
    Option<(i32, Vec<u8>)>,
    Option<crate::item::EntityData>,
)> {
    let bed = items.component_type("minecraft:block_entity_data");
    let ed = items.component_type("minecraft:entity_data");
    let mut entity_data = None;
    let added = rustmc_proto::varint::read_varint(cur).ok()?;
    let removed = rustmc_proto::varint::read_varint(cur).ok()?;
    // `None` once a type id the table does not name is met: the value can
    // still be stepped over, but it cannot be written back under a name, and
    // half a patch is a different stack — so the patch is dropped whole and
    // the stack goes out as its prototype. `block_entity_data` is still read.
    let mut patch = Some(crate::patch::Patch::default());
    let mut found = None;
    for _ in 0..added {
        let ty = rustmc_proto::varint::read_varint(cur).ok()?;
        let len = usize::try_from(rustmc_proto::varint::read_varint(cur).ok()?).ok()?;
        if cur.len() < len {
            return None;
        }
        let (value, rest) = cur.split_at(len);
        *cur = rest;
        patch = match (patch, items.component_type_name(ty)) {
            (Some(p), Some(name)) => Some(p.with_raw(name, value.to_vec())),
            _ => None,
        };
        if Some(ty) == bed {
            let mut v = value;
            let be_type = rustmc_proto::varint::read_varint(&mut v).ok()?;
            found = Some((be_type, v.to_vec()));
        }
        if Some(ty) == ed {
            entity_data = read_entity_data(value).or(entity_data);
        }
    }
    for _ in 0..removed {
        let Ok(ty) = rustmc_proto::varint::read_varint(cur) else {
            // Truncated after the additions: what was read is kept as far
            // as `block_entity_data`, which is all the old reader took.
            return Some((crate::patch::Patch::default(), found, entity_data));
        };
        patch = match (patch, items.component_type_name(ty)) {
            (Some(p), Some(name)) => Some(p.without(name)),
            _ => None,
        };
    }
    Some((patch.unwrap_or_default(), found, entity_data))
}

/// One `minecraft:entity_data` value, its length prefix already stripped:
/// the entity type's registry id, then `ByteBufCodecs.COMPOUND_TAG`. `None`
/// for a type this server cannot create or a value that does not parse — the
/// raw bytes still travel in the patch.
fn read_entity_data(mut value: &[u8]) -> Option<crate::item::EntityData> {
    let entity_id = rustmc_proto::varint::read_varint(&mut value).ok()?;
    let entity = crate::entities::shared_kind_ids()?.name_of(entity_id)?;
    let tag = rustmc_world::nbt::read_network_tag(&mut value).ok()?;
    let flag = |key: &str| {
        tag.as_ref()
            .and_then(|t| t.get(key))
            .and_then(rustmc_world::nbt::Nbt::as_i32)
            .map(|v| v != 0)
    };
    Some(crate::item::EntityData {
        entity: entity.to_string(),
        silent: flag("Silent"),
        left_handed: flag("LeftHanded"),
    })
}

/// The item a `set_creative_mode_slot` packet just put somewhere, if it put
/// a non-empty stack anywhere at all — beside [`apply_hand`] rather than
/// inside it, because this reads the same body for a different question
/// ("what item is this about") and [`apply_hand`] is not the only caller who
/// needs the answer.
///
/// `set_carried_item` (selecting an already-filled slot) answers `None`: it
/// changes no stack, so nothing was newly *put into* an inventory and
/// vanilla's `InventoryChangeTrigger` has nothing to fire on.
///
/// # Errors
/// If the body is one this decodes and it is truncated.
pub fn creative_slot_item(
    id: i32,
    body: &[u8],
    items: Option<&ItemIds>,
) -> io::Result<Option<String>> {
    if id != ids::SET_CREATIVE_MODE_SLOT {
        return Ok(None);
    }
    let mut cur = body;
    // Mirrors `apply_hand`'s own read of this packet exactly, because the
    // two have to agree about where the item id lives in the body or one of
    // them is reading the wrong bytes.
    let _slot = wire::read_u16(&mut cur)?;
    let count = rustmc_proto::varint::read_varint(&mut cur)?;
    if count <= 0 {
        return Ok(None);
    }
    let item_id = rustmc_proto::varint::read_varint(&mut cur)?;
    Ok(items.and_then(|t| t.name(item_id)).map(str::to_string))
}

/// `Input`'s `FLAG_SHIFT` — the sneak bit of the one byte
/// `ServerboundPlayerInputPacket` carries.
///
/// `Input$1.encode` ors seven `private static final byte` flags into a single
/// `writeByte`: `FORWARD` 1, `BACKWARD` 2, `LEFT` 4, `RIGHT` 8, `JUMP` 16,
/// `SHIFT` 32, `SPRINT` 64. Only this one is read here, because
/// `handlePlayerInput` is the only handler that touches the packet and the
/// only field it forwards is `shift()`; the other six reach
/// `setLastClientInput` and nothing in this server reads that yet. They are
/// named rather than left out so that the gap is visible.
const FLAG_SHIFT: u8 = 32;

/// `ServerGamePacketListenerImpl.handlePlayerInput`: the sneak bit, or `None`
/// for any other packet.
///
/// ```text
///   handlePlayerInput, javap -p -c -constants on the pinned jar
///      9: PacketUtils.ensureRunningOnSameThread(...)
///     16: player.setLastClientInput(packet.input())
///     23: hasClientLoaded : ifeq 51 -> 51: return
///     34: player.resetLastActionTime()
///     42: packet.input().shift()
///     48: player.setShiftKeyDown(...)
/// ```
///
/// **`setLastClientInput` happens before the `hasClientLoaded` gate and
/// `setShiftKeyDown` after it**, which is a real difference and not a
/// transcription artefact: a client that has not finished loading still
/// updates the stored input and does *not* update the shift flag. This server
/// has no `hasClientLoaded` — a connection reaches the play loop only after
/// the join sequence — so the gate is unconditionally true here and is
/// recorded rather than reproduced.
///
/// Returned as an `Option<bool>` rather than written through a `&mut` so the
/// dispatch arm reads like the other decoders in this module, and so a packet
/// that is not this one cannot be mistaken for a released sneak key.
///
/// # Errors
/// If the body is this packet and is empty.
pub fn input_shift(id: i32, body: &[u8]) -> io::Result<Option<bool>> {
    if id != ids::PLAYER_INPUT {
        return Ok(None);
    }
    let mut cur = body;
    let flags = wire::read_u8(&mut cur)?;
    Ok(Some(flags & FLAG_SHIFT != 0))
}

/// What the server believes about a player.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerState {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
    pub horizontal_collision: bool,
    /// `Entity.isShiftKeyDown()` — the sneak key, as the client last reported
    /// it on `ServerboundPlayerInputPacket`.
    ///
    /// **A state, not an event.** `handlePlayerInput` stores what it is sent
    /// and holds it until another input packet says otherwise, so this stays
    /// set across every later packet on the connection. See [`input_shift`]
    /// for the read, and `crate::world_send::handle_use_item_on` for the one
    /// branch that consults it.
    pub shift_key_down: bool,
    /// The last teleport id the client acknowledged.
    ///
    /// **This field is a record, not a gate**, and saying so is the point: the
    /// gate is [`Self::awaiting_position`]. Until 2026-08-13 this was the only
    /// thing `ACCEPT_TELEPORTATION` wrote, nothing anywhere read it, and its
    /// doc comment claimed that an unacknowledged client's "positions are not
    /// yet authoritative" — which described a behaviour no line of this server
    /// implemented. `docs/PARITY-ROADMAP.md` §41.
    pub acknowledged_teleport: Option<i32>,
    /// `ServerGamePacketListenerImpl.awaitingTeleport` — the id of the
    /// teleport this connection last sent and is waiting to have acknowledged.
    ///
    /// **Never cleared by the acknowledgement.** Only
    /// [`Self::awaiting_position`] is (`handleAcceptTeleportPacket` offset
    /// 125), and that asymmetry is the whole mechanism: a second
    /// acknowledgement of the same id still matches at offset 20, finds the
    /// position already taken at 27, and disconnects.
    pub awaiting_teleport: i32,
    /// `awaitingPositionFromClient` — where the server put the player, held
    /// until the client acknowledges that teleport.
    ///
    /// `Some` means a teleport is outstanding. `handleMovePlayer` consults it
    /// through `updateAwaitingTeleport` at offset 123 and **discards the
    /// claimed position** while it is set, which is why a client that never
    /// acknowledges cannot move.
    pub awaiting_position: Option<(f64, f64, f64)>,
}

impl PlayerState {
    #[must_use]
    pub const fn at(x: f64, y: f64, z: f64) -> Self {
        Self {
            x,
            y,
            z,
            yaw: 0.0,
            pitch: 0.0,
            on_ground: false,
            horizontal_collision: false,
            shift_key_down: false,
            acknowledged_teleport: None,
            // No teleport is outstanding until one is sent.
            // `crate::world_send::enter_world` writes id 1 and
            // `crate::status` arms the pair below with the same spawn, so a
            // real connection is awaiting from the moment it reaches play.
            awaiting_teleport: 0,
            awaiting_position: None,
        }
    }

    /// Arm the awaiting-teleport state for a teleport just written to the wire.
    ///
    /// `ServerGamePacketListenerImpl.teleport` sets both fields together, and
    /// they are set together here for the same reason: an `awaiting_teleport`
    /// without an `awaiting_position` is exactly the state that disconnects a
    /// client, so a call site that sets one and forgets the other would drop
    /// the next acknowledgement it receives.
    pub const fn awaiting(&mut self, id: i32, at: (f64, f64, f64)) {
        self.awaiting_teleport = id;
        self.awaiting_position = Some(at);
    }

    /// The chunk the player is standing in.
    ///
    /// Floor division, not truncation: block −1 is in chunk −1, and `as i32`
    /// would put it in chunk 0 and make every chunk west or north of the
    /// origin off by one.
    #[must_use]
    pub fn chunk(&self) -> (i32, i32) {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a block coordinate inside the world's extent"
        )]
        let (bx, bz) = (self.x.floor() as i32, self.z.floor() as i32);
        (bx >> 4, bz >> 4)
    }
}

/// What a decoded packet told us.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Update {
    /// Position and/or rotation changed.
    Moved,
    /// Only the on-ground/collision flags changed.
    Status,
    /// The client acknowledged a teleport.
    Teleport(i32),
    /// The client must be disconnected, with this translation key as the
    /// reason.
    ///
    /// A variant rather than an `io::Error` because a refusal is not a
    /// failure: vanilla writes a `ClientboundDisconnectPacket` carrying a
    /// **translatable** component and closes cleanly, and the corpus compares
    /// that packet's bytes. An error return would close the socket with
    /// nothing on it and the two servers would then differ by one packet for
    /// a reason that had nothing to do with the mechanism.
    Disconnect(&'static str),
    /// Not a packet this module handles.
    Ignored,
}

/// Decode one serverbound play packet into `state`.
///
/// Unknown ids are [`Update::Ignored`] rather than an error: a real client
/// sends plenty this server does not implement yet, and refusing them would
/// drop the connection over an inventory click.
///
/// # Errors
/// If a packet this module *does* handle is truncated.
pub fn apply(id: i32, body: &[u8], state: &mut PlayerState) -> io::Result<Update> {
    let mut cur = body;
    if id == ids::ACCEPT_TELEPORTATION {
        let teleport = rustmc_proto::varint::read_varint(&mut cur)?;
        state.acknowledged_teleport = Some(teleport);
        // `handleAcceptTeleportPacket`, in its own order:
        //
        //   20: if_icmpne 128   an id we are not waiting for is ignored,
        //                       silently and without complaint, which is
        //                       what lets a client acknowledge a teleport
        //                       that has already been superseded;
        //   27: ifnonnull 41    the id matches and nothing is outstanding,
        //                       so this is a *second* acknowledgement of a
        //                       teleport already accepted -> disconnect;
        //   41..122             otherwise snap to the position the server
        //                       has been holding and clear it.
        if teleport != state.awaiting_teleport {
            return Ok(Update::Ignored);
        }
        let Some((x, y, z)) = state.awaiting_position.take() else {
            return Ok(Update::Disconnect(INVALID_PLAYER_MOVEMENT));
        };
        // `ServerPlayer.absSnapTo` — the position the server chose wins
        // over anything the client has claimed since.
        state.x = x;
        state.y = y;
        state.z = z;
        return Ok(Update::Teleport(teleport));
    }
    // The four movement packets share one vanilla handler, so they share
    // one gate. `handleMovePlayer` reads the whole body first (offsets
    // 12..121, each field defaulting to the player's current value when
    // this packet does not carry it) and only then, at offset 123, asks
    // `updateAwaitingTeleport`. If a teleport is outstanding it applies
    // **the rotation alone** — `absSnapRotationTo` at 137 — and returns at
    // 140, so the claimed position and the flags byte are thrown away
    // together. `decode_move` consumes the whole body before this decides.
    let Some(pkt) = decode_move(id, body)? else {
        return Ok(Update::Ignored);
    };
    if state.awaiting_position.is_some() {
        // With no rotation in the packet `absSnapRotationTo` is handed the
        // player's own rotation and writes nothing, so that is `Ignored`. A
        // rotation still lands and still reaches `ServerEntity.sendChanges`,
        // so the player turns where it stands and that is `Moved`.
        return Ok(match pkt.rot {
            Some((yaw, pitch)) => {
                state.yaw = yaw;
                state.pitch = pitch;
                Update::Moved
            }
            None => Update::Ignored,
        });
    }
    if let Some((x, y, z)) = pkt.pos {
        state.x = x;
        state.y = y;
        state.z = z;
    }
    if let Some((yaw, pitch)) = pkt.rot {
        state.yaw = yaw;
        state.pitch = pitch;
    }
    state.on_ground = pkt.on_ground;
    state.horizontal_collision = pkt.horizontal_collision;
    if pkt.pos.is_some() || pkt.rot.is_some() {
        Ok(Update::Moved)
    } else {
        Ok(Update::Status)
    }
}

/// Decode one of the four `move_player` packets into the shape
/// [`crate::move_player`] works on, or `None` for any other id.
///
/// **The single decode.** [`apply`] is written on top of it rather than
/// beside it, because two decoders of the same four packets is two places
/// for the trailing flags byte to be read as a bool.
///
/// The `Option` fields are the packet's *presence*, not defaulted values:
/// `ServerboundMovePlayerPacket`'s `getX(double)` returns the argument when
/// the packet carries no position, and `handleMovePlayer` passes two
/// different defaults into the same getter at offsets 12-37 and 92-187. A
/// decoder that resolves the default once cannot serve both.
///
/// # Errors
/// If a move packet is truncated.
pub fn decode_move(id: i32, body: &[u8]) -> io::Result<Option<crate::move_player::MovePacket>> {
    use crate::move_player::MovePacket;
    let mut cur = body;
    let (pos, rot) = match id {
        ids::MOVE_PLAYER_POS => {
            let x = wire::read_f64(&mut cur)?;
            let y = wire::read_f64(&mut cur)?;
            let z = wire::read_f64(&mut cur)?;
            (Some((x, y, z)), None)
        }
        ids::MOVE_PLAYER_POS_ROT => {
            let x = wire::read_f64(&mut cur)?;
            let y = wire::read_f64(&mut cur)?;
            let z = wire::read_f64(&mut cur)?;
            let yaw = wire::read_f32(&mut cur)?;
            let pitch = wire::read_f32(&mut cur)?;
            (Some((x, y, z)), Some((yaw, pitch)))
        }
        ids::MOVE_PLAYER_ROT => {
            let yaw = wire::read_f32(&mut cur)?;
            let pitch = wire::read_f32(&mut cur)?;
            (None, Some((yaw, pitch)))
        }
        ids::MOVE_PLAYER_STATUS_ONLY => (None, None),
        _ => return Ok(None),
    };
    let flags = wire::read_u8(&mut cur)?;
    Ok(Some(MovePacket {
        pos,
        rot,
        on_ground: flags & FLAG_ON_GROUND != 0,
        horizontal_collision: flags & FLAG_HORIZONTAL_COLLISION != 0,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(parts: &[&[u8]]) -> Vec<u8> {
        parts.concat()
    }

    #[test]
    fn a_position_packet_moves_the_player() {
        let mut state = PlayerState::at(0.0, 0.0, 0.0);
        let mut b = Vec::new();
        wire::write_f64(&mut b, 1.5).expect("buf");
        wire::write_f64(&mut b, 70.0).expect("buf");
        wire::write_f64(&mut b, -2.5).expect("buf");
        wire::write_u8(&mut b, FLAG_ON_GROUND).expect("buf");

        assert_eq!(
            apply(ids::MOVE_PLAYER_POS, &b, &mut state).expect("decoded"),
            Update::Moved
        );
        assert!((state.x - 1.5).abs() < f64::EPSILON);
        assert!((state.z + 2.5).abs() < f64::EPSILON);
        assert!(state.on_ground);
        assert!(!state.horizontal_collision);
    }

    /// The trailing byte carries two flags. A port that reads it as a bool
    /// sees `on_ground` for a walking player and misses the collision bit.
    #[test]
    fn the_trailing_byte_is_flags_not_a_bool() {
        let mut state = PlayerState::at(0.0, 0.0, 0.0);
        let b = body(&[&[FLAG_ON_GROUND | FLAG_HORIZONTAL_COLLISION]]);
        assert_eq!(
            apply(ids::MOVE_PLAYER_STATUS_ONLY, &b, &mut state).expect("decoded"),
            Update::Status
        );
        assert!(state.on_ground);
        assert!(state.horizontal_collision);

        let b = body(&[&[FLAG_HORIZONTAL_COLLISION]]);
        apply(ids::MOVE_PLAYER_STATUS_ONLY, &b, &mut state).expect("decoded");
        assert!(!state.on_ground, "bit 0 clear means airborne");
        assert!(state.horizontal_collision);
    }

    /// Rotation-only must not disturb the position, which is the whole point
    /// of it being a separate packet.
    #[test]
    fn rotation_only_leaves_the_position_alone() {
        let mut state = PlayerState::at(10.0, 64.0, -3.0);
        let mut b = Vec::new();
        wire::write_f32(&mut b, 90.0).expect("buf");
        wire::write_f32(&mut b, -12.0).expect("buf");
        wire::write_u8(&mut b, 0).expect("buf");

        apply(ids::MOVE_PLAYER_ROT, &b, &mut state).expect("decoded");
        assert!((state.x - 10.0).abs() < f64::EPSILON);
        assert!((state.y - 64.0).abs() < f64::EPSILON);
        assert!((state.yaw - 90.0).abs() < f32::EPSILON);
        assert!((state.pitch + 12.0).abs() < f32::EPSILON);
    }

    #[test]
    fn an_unhandled_packet_is_ignored_not_an_error() {
        let mut state = PlayerState::at(0.0, 0.0, 0.0);
        assert_eq!(
            apply(0x7F, &[1, 2, 3], &mut state).expect("no error"),
            Update::Ignored
        );
    }

    /// Chunk coordinates are floor division. `as i32` truncates toward zero
    /// and would put block −1 in chunk 0.
    #[test]
    fn the_chunk_is_floor_divided() {
        assert_eq!(PlayerState::at(0.0, 0.0, 0.0).chunk(), (0, 0));
        assert_eq!(PlayerState::at(15.9, 0.0, 15.9).chunk(), (0, 0));
        assert_eq!(PlayerState::at(-0.5, 0.0, -0.5).chunk(), (-1, -1));
        assert_eq!(PlayerState::at(-16.0, 0.0, -17.0).chunk(), (-1, -2));
    }

    /// A creative slot is numbered in the **menu**, not the inventory:
    /// `InventoryMenu.USE_ROW_SLOT_START` is 36, so hotbar slot 0 arrives as
    /// 36. Treating the number as a hotbar index puts everything the player
    /// picks into a slot they can never select, and placement silently does
    /// nothing.
    #[test]
    fn a_creative_slot_is_offset_by_the_menus_first_hotbar_slot() {
        let items = table();
        let mut hotbar = Hotbar::new();
        assert!(apply_hand(
            ids::SET_CREATIVE_MODE_SLOT,
            &give(36, 1),
            &mut hotbar,
            Some(&items)
        )
        .expect("decoded"));
        assert_eq!(
            hotbar.held(),
            Some("minecraft:stone"),
            "slot 36 is hotbar 0"
        );

        // The main inventory (9-35) must not land in the hotbar — and since
        // this type grew from nine slots to `Inventory`'s 36, it is no longer
        // *discarded* either. It lands at `Inventory` index 9, which is where
        // a container menu will read it from. Asserting only that the hotbar
        // is unchanged would pass on both behaviours.
        apply_hand(
            ids::SET_CREATIVE_MODE_SLOT,
            &give(9, 2),
            &mut hotbar,
            Some(&items),
        )
        .expect("decoded");
        assert_eq!(
            hotbar.held(),
            Some("minecraft:stone"),
            "slot 9 is not hotbar 0"
        );
        assert_eq!(
            hotbar.stack(9),
            crate::item::ItemStack::one("minecraft:granite"),
            "menu slot 9 lands at Inventory index 9, not nowhere"
        );

        // The offhand (45) has no home in these 36 — it has a home of its
        // own now, and the assertion that matters is still that it is not in
        // the 36. It used to be discarded entirely; the second assertion is
        // what changed and is what would catch a regression back to that.
        apply_hand(
            ids::SET_CREATIVE_MODE_SLOT,
            &give(45, 2),
            &mut hotbar,
            Some(&items),
        )
        .expect("decoded");
        assert_eq!(
            hotbar.offhand(),
            crate::item::ItemStack::one("minecraft:granite"),
            "menu slot 45 is the offhand and is no longer discarded"
        );
        assert!(
            (0..INVENTORY_SIZE).all(|i| i == 0 || i == 9 || hotbar.stack(i).is_empty()),
            "the offhand went somewhere it should not have"
        );
    }

    /// Selecting a slot changes what is held, and the packet is a **short**.
    #[test]
    fn the_selected_slot_follows_set_carried_item() {
        let items = table();
        let mut hotbar = Hotbar::new();
        apply_hand(
            ids::SET_CREATIVE_MODE_SLOT,
            &give(36, 1),
            &mut hotbar,
            Some(&items),
        )
        .expect("decoded");
        apply_hand(
            ids::SET_CREATIVE_MODE_SLOT,
            &give(38, 2),
            &mut hotbar,
            Some(&items),
        )
        .expect("decoded");
        assert_eq!(hotbar.held(), Some("minecraft:stone"));

        let mut sel = Vec::new();
        wire::write_u16(&mut sel, 2).expect("buf");
        apply_hand(ids::SET_CARRIED_ITEM, &sel, &mut hotbar, Some(&items)).expect("decoded");
        assert_eq!(hotbar.selected(), 2);
        assert_eq!(hotbar.held(), Some("minecraft:granite"));

        // Out of range is ignored rather than panicking on a hostile client.
        let mut bad = Vec::new();
        wire::write_u16(&mut bad, 99).expect("buf");
        apply_hand(ids::SET_CARRIED_ITEM, &bad, &mut hotbar, Some(&items)).expect("decoded");
        assert_eq!(hotbar.selected(), 2);
    }

    /// A count of zero is an empty stack and nothing follows it — so an
    /// emptied slot clears rather than keeping what was there.
    #[test]
    fn an_empty_stack_clears_the_slot() {
        let items = table();
        let mut hotbar = Hotbar::new();
        apply_hand(
            ids::SET_CREATIVE_MODE_SLOT,
            &give(36, 1),
            &mut hotbar,
            Some(&items),
        )
        .expect("decoded");
        let mut empty = Vec::new();
        wire::write_u16(&mut empty, 36).expect("buf");
        rustmc_proto::varint::write_varint(&mut empty, 0).expect("buf");
        apply_hand(
            ids::SET_CREATIVE_MODE_SLOT,
            &empty,
            &mut hotbar,
            Some(&items),
        )
        .expect("decoded");
        assert_eq!(hotbar.held(), None);
    }

    /// Without an item table an id cannot be resolved, and the slot is
    /// cleared rather than guessed: a wrong item places the wrong block, and
    /// a client that sees the wrong block has no way to tell it from a
    /// broken world.
    #[test]
    fn no_item_table_means_no_item_rather_than_a_guess() {
        let mut hotbar = Hotbar::new();
        apply_hand(ids::SET_CREATIVE_MODE_SLOT, &give(36, 1), &mut hotbar, None).expect("decoded");
        assert_eq!(hotbar.held(), None);
    }

    #[test]
    fn a_movement_packet_is_not_a_hand_packet() {
        let mut hotbar = Hotbar::new();
        assert!(!apply_hand(ids::MOVE_PLAYER_POS, &[], &mut hotbar, None).expect("no error"));
    }

    /// Two entries, enough to tell one from the other.
    /// Two items, in memory. No file: see [`ItemIds::parse`] for what a
    /// shared temporary path cost.
    fn table() -> ItemIds {
        ItemIds::parse(&serde_json::json!({
            "items": {"minecraft:stone": 1, "minecraft:granite": 2}
        }))
        .expect("parsed")
    }

    /// `set_creative_mode_slot`: a short slot, then the optional item stack.
    fn give(slot: i16, item_id: i32) -> Vec<u8> {
        let mut b = Vec::new();
        #[expect(clippy::cast_sign_loss, reason = "a short goes out as its bits")]
        wire::write_u16(&mut b, slot as u16).expect("buf");
        rustmc_proto::varint::write_varint(&mut b, 1).expect("buf");
        rustmc_proto::varint::write_varint(&mut b, item_id).expect("buf");
        rustmc_proto::varint::write_varint(&mut b, 0).expect("buf");
        rustmc_proto::varint::write_varint(&mut b, 0).expect("buf");
        b
    }

    /// A creative stack of item 1 whose DELIMITED component patch is
    /// `added` (type id, value bytes) — each value length-prefixed, as
    /// `DataComponentPatch.DELIMITED_STREAM_CODEC` writes it.
    fn give_with(slot: i16, added: &[(i32, Vec<u8>)]) -> Vec<u8> {
        let mut b = Vec::new();
        #[expect(clippy::cast_sign_loss, reason = "a short goes out as its bits")]
        wire::write_u16(&mut b, slot as u16).expect("buf");
        rustmc_proto::varint::write_varint(&mut b, 1).expect("buf");
        rustmc_proto::varint::write_varint(&mut b, 1).expect("buf");
        rustmc_proto::varint::write_varint(&mut b, i32::try_from(added.len()).unwrap_or(0))
            .expect("buf");
        rustmc_proto::varint::write_varint(&mut b, 0).expect("buf");
        for (ty, v) in added {
            rustmc_proto::varint::write_varint(&mut b, *ty).expect("buf");
            rustmc_proto::varint::write_varint(&mut b, i32::try_from(v.len()).unwrap_or(0))
                .expect("buf");
            b.extend_from_slice(v);
        }
        b
    }

    /// `block_entity_data` is read off a creative stack, and a component in
    /// front of it that this server does not decode is stepped over by its
    /// length prefix rather than misread — the damage varint here, and an
    /// arbitrary three-byte value under an unknown type.
    #[test]
    fn a_creative_stack_keeps_its_block_entity_data_past_other_components() {
        let items = ItemIds::parse(&serde_json::json!({
            "items": {"minecraft:chest": 1},
            "data_component_types": {"minecraft:damage": 3, "minecraft:block_entity_data": 60}
        }))
        .expect("parsed");
        let tag = vec![10u8, 4, 0, 1, b'S', 0, 0, 0, 0, 0, 0, 0, 7, 0];
        let mut bed = vec![1u8];
        bed.extend_from_slice(&tag);
        let body = give_with(36, &[(3, vec![5]), (99, vec![1, 2, 3]), (60, bed)]);
        let mut hotbar = Hotbar::default();
        apply_hand(
            ids::SET_CREATIVE_MODE_SLOT,
            &body,
            &mut hotbar,
            Some(&items),
        )
        .expect("applied");
        let held = hotbar.stack(0);
        assert_eq!(held.item(), "minecraft:chest");
        assert_eq!(held.block_entity_data(), Some((1, tag.as_slice())));
        // With no block_entity_data the stack carries none, and a truncated
        // value is dropped rather than read past the end.
        let body = give_with(36, &[(3, vec![5])]);
        apply_hand(
            ids::SET_CREATIVE_MODE_SLOT,
            &body,
            &mut hotbar,
            Some(&items),
        )
        .expect("applied");
        assert_eq!(hotbar.stack(0).block_entity_data(), None);
        let mut cut = give_with(36, &[(60, vec![1, 10, 0])]);
        cut.truncate(cut.len() - 1);
        apply_hand(ids::SET_CREATIVE_MODE_SLOT, &cut, &mut hotbar, Some(&items)).expect("applied");
        assert_eq!(hotbar.stack(0).block_entity_data(), None);
    }

    /// What a creative client puts in a slot goes back out with every
    /// component it carried: the delimited patch loses its length prefixes
    /// and nothing else, removals included, so the clientbound stack is the
    /// trusted encoding of the same value. An unknown type drops the patch
    /// whole rather than writing half of it.
    #[test]
    fn a_creative_stack_echoes_its_whole_patch_clientbound() {
        let items = ItemIds::parse(&serde_json::json!({
            "items": {"minecraft:chest": 1},
            "data_component_types": {"minecraft:damage": 3, "minecraft:stored_enchantments": 34,
                                     "minecraft:repair_cost": 18}
        }))
        .expect("parsed");
        let mut body = give_with(36, &[(3, vec![5]), (34, vec![1, 21, 1])]);
        // One removal: repair_cost.
        let removals_at = 2 + 1 + 1;
        body[removals_at + 1] = 1;
        body.push(18);
        let mut hotbar = Hotbar::default();
        apply_hand(
            ids::SET_CREATIVE_MODE_SLOT,
            &body,
            &mut hotbar,
            Some(&items),
        )
        .expect("applied");
        let wire = crate::menu::to_wire(&hotbar.stack(0), Some(&items)).expect("a stack");
        assert_eq!(
            wire.patch.as_deref(),
            Some(&[2u8, 1, 3, 5, 34, 1, 21, 1, 18][..]),
            "added=2 removed=1, damage 5, stored_enchantments {{21: 1}}, then repair_cost"
        );
        let body = give_with(36, &[(3, vec![5]), (99, vec![1, 2, 3])]);
        apply_hand(
            ids::SET_CREATIVE_MODE_SLOT,
            &body,
            &mut hotbar,
            Some(&items),
        )
        .expect("applied");
        let wire = crate::menu::to_wire(&hotbar.stack(0), Some(&items)).expect("a stack");
        assert_eq!(wire.patch, None, "an unknown type sends the prototype");
    }

    /// `minecraft:entity_data` off a creative spawn egg: the entity type id
    /// and `Silent` are decoded for the egg to apply, and the raw value still
    /// rides in the patch so the client is shown the stack it sent.
    #[test]
    fn a_creative_egg_keeps_its_entity_data_silent() {
        let items = ItemIds::parse(&serde_json::json!({
            "items": {"minecraft:pig_spawn_egg": 1},
            "data_component_types": {"minecraft:damage": 3, "minecraft:entity_data": 71}
        }))
        .expect("parsed");
        let kinds = crate::entities::shared_kind_ids().expect("entity_types.json");
        let pig = (0..1024)
            .find(|&i| kinds.name_of(i) == Some("minecraft:pig"))
            .expect("the pig is a kind this server creates");
        let mut value = Vec::new();
        rustmc_proto::varint::write_varint(&mut value, pig).expect("buf");
        value.extend_from_slice(&[10, 1, 0, 6]);
        value.extend_from_slice(b"Silent");
        value.extend_from_slice(&[1, 0]);
        let body = give_with(36, &[(3, vec![5]), (71, value.clone())]);
        let mut hotbar = Hotbar::default();
        apply_hand(
            ids::SET_CREATIVE_MODE_SLOT,
            &body,
            &mut hotbar,
            Some(&items),
        )
        .expect("applied");
        let held = hotbar.stack(0);
        assert_eq!(
            held.entity_data(),
            Some(&crate::item::EntityData {
                entity: "minecraft:pig".into(),
                silent: Some(true),
                left_handed: None,
            })
        );
        let wire = crate::menu::to_wire(&held, Some(&items)).expect("a stack");
        let mut want = vec![2u8, 0, 3, 5, 71];
        want.extend_from_slice(&value);
        assert_eq!(wire.patch.as_deref(), Some(want.as_slice()));
        // `Silent:0b` reads as not silent, and an egg without the component
        // carries none.
        let n = value.len();
        value[n - 2] = 0;
        let body = give_with(36, &[(71, value)]);
        apply_hand(
            ids::SET_CREATIVE_MODE_SLOT,
            &body,
            &mut hotbar,
            Some(&items),
        )
        .expect("applied");
        assert_eq!(
            hotbar.stack(0).entity_data().and_then(|d| d.silent),
            Some(false)
        );
        assert_eq!(
            hotbar.stack(0).entity_data().and_then(|d| d.left_handed),
            None,
            "no LeftHanded key, no override"
        );
        // `LeftHanded` after `Silent` in the same compound: both are read.
        for left in [0u8, 1] {
            let mut lh = Vec::new();
            rustmc_proto::varint::write_varint(&mut lh, pig).expect("buf");
            lh.extend_from_slice(&[10, 1, 0, 6]);
            lh.extend_from_slice(b"Silent");
            lh.extend_from_slice(&[1, 1, 0, 10]);
            lh.extend_from_slice(b"LeftHanded");
            lh.extend_from_slice(&[left, 0]);
            let body = give_with(36, &[(71, lh)]);
            apply_hand(
                ids::SET_CREATIVE_MODE_SLOT,
                &body,
                &mut hotbar,
                Some(&items),
            )
            .expect("applied");
            assert_eq!(
                hotbar.stack(0).entity_data(),
                Some(&crate::item::EntityData {
                    entity: "minecraft:pig".into(),
                    silent: Some(true),
                    left_handed: Some(left == 1),
                })
            );
        }
        let body = give_with(36, &[(3, vec![5])]);
        apply_hand(
            ids::SET_CREATIVE_MODE_SLOT,
            &body,
            &mut hotbar,
            Some(&items),
        )
        .expect("applied");
        assert_eq!(hotbar.stack(0).entity_data(), None);
    }

    /// `minecraft:custom_name` off a creative chest reads back as the NBT
    /// string the client wrote — what a placed container takes as its name —
    /// and a value with bytes left over after its tag is not a name.
    #[test]
    fn a_creative_stacks_custom_name_reads_back_as_its_tag() {
        let items = ItemIds::parse(&serde_json::json!({
            "items": {"minecraft:chest": 1},
            "data_component_types": {"minecraft:custom_name": 6}
        }))
        .expect("parsed");
        let mut hotbar = Hotbar::default();
        let body = give_with(36, &[(6, vec![8, 0, 4, b'L', b'o', b'o', b't'])]);
        apply_hand(
            ids::SET_CREATIVE_MODE_SLOT,
            &body,
            &mut hotbar,
            Some(&items),
        )
        .expect("applied");
        assert_eq!(
            hotbar.stack(0).custom_name(),
            Some(rustmc_world::nbt::Nbt::String("Loot".into()))
        );
        let body = give_with(36, &[(6, vec![8, 0, 1, b'L', 9])]);
        apply_hand(
            ids::SET_CREATIVE_MODE_SLOT,
            &body,
            &mut hotbar,
            Some(&items),
        )
        .expect("applied");
        assert_eq!(hotbar.stack(0).custom_name(), None);
    }

    fn ack(id: i32) -> Vec<u8> {
        let mut b = Vec::new();
        rustmc_proto::varint::write_varint(&mut b, id).expect("buf");
        b
    }

    fn pos_packet(x: f64, y: f64, z: f64) -> Vec<u8> {
        let mut b = Vec::new();
        wire::write_f64(&mut b, x).expect("buf");
        wire::write_f64(&mut b, y).expect("buf");
        wire::write_f64(&mut b, z).expect("buf");
        wire::write_u8(&mut b, FLAG_ON_GROUND).expect("buf");
        b
    }

    #[test]
    fn a_teleport_acknowledgement_is_recorded() {
        let mut state = PlayerState::at(0.0, 0.0, 0.0);
        state.awaiting(7, (1.0, 2.0, 3.0));
        assert_eq!(
            apply(ids::ACCEPT_TELEPORTATION, &ack(7), &mut state).expect("decoded"),
            Update::Teleport(7)
        );
        assert_eq!(state.acknowledged_teleport, Some(7));
        // `absSnapTo` at offset 80 — the server's held position wins.
        assert_eq!((state.x, state.y, state.z), (1.0, 2.0, 3.0));
        // Offset 125 clears the position and **not** the id.
        assert_eq!(state.awaiting_position, None);
        assert_eq!(state.awaiting_teleport, 7);
    }

    /// `handleAcceptTeleportPacket` offset 20: `if_icmpne 128`.
    #[test]
    fn an_acknowledgement_of_an_id_we_are_not_waiting_for_is_ignored() {
        let mut state = PlayerState::at(0.0, 0.0, 0.0);
        state.awaiting(7, (1.0, 2.0, 3.0));
        assert_eq!(
            apply(ids::ACCEPT_TELEPORTATION, &ack(8), &mut state).expect("decoded"),
            Update::Ignored
        );
        // Nothing was consumed, so the real acknowledgement still works.
        assert_eq!(state.awaiting_position, Some((1.0, 2.0, 3.0)));
        assert_eq!((state.x, state.y, state.z), (0.0, 0.0, 0.0));
    }

    /// The whole reason `awaiting_teleport` is not cleared at offset 125:
    /// the second acknowledgement matches the id and finds nothing held.
    #[test]
    fn acknowledging_the_same_teleport_twice_disconnects() {
        let mut state = PlayerState::at(0.0, 0.0, 0.0);
        state.awaiting(7, (1.0, 2.0, 3.0));
        assert_eq!(
            apply(ids::ACCEPT_TELEPORTATION, &ack(7), &mut state).expect("decoded"),
            Update::Teleport(7)
        );
        assert_eq!(
            apply(ids::ACCEPT_TELEPORTATION, &ack(7), &mut state).expect("decoded"),
            Update::Disconnect(INVALID_PLAYER_MOVEMENT)
        );
    }

    /// `updateAwaitingTeleport` returning true at `handleMovePlayer` 123.
    #[test]
    fn movement_is_discarded_while_a_teleport_is_outstanding() {
        let mut state = PlayerState::at(0.0, 0.0, 0.0);
        state.awaiting(1, (8.0, 65.0, 8.0));
        assert_eq!(
            apply(
                ids::MOVE_PLAYER_POS,
                &pos_packet(50.0, 65.0, 50.0),
                &mut state
            )
            .expect("decoded"),
            Update::Ignored
        );
        assert_eq!((state.x, state.y, state.z), (0.0, 0.0, 0.0));
        // The flags travel in the same packet and are dropped with it.
        assert!(!state.on_ground);

        // Once acknowledged, the identical packet moves the player.
        apply(ids::ACCEPT_TELEPORTATION, &ack(1), &mut state).expect("decoded");
        assert_eq!(
            apply(
                ids::MOVE_PLAYER_POS,
                &pos_packet(50.0, 65.0, 50.0),
                &mut state
            )
            .expect("decoded"),
            Update::Moved
        );
        assert_eq!((state.x, state.y, state.z), (50.0, 65.0, 50.0));
        assert!(state.on_ground);
    }

    /// Offsets 130-137: the rotation is applied even while awaiting, because
    /// `absSnapRotationTo` runs *before* the return at 140.
    #[test]
    fn rotation_still_lands_while_a_teleport_is_outstanding() {
        let mut state = PlayerState::at(0.0, 0.0, 0.0);
        state.awaiting(1, (8.0, 65.0, 8.0));
        let mut b = Vec::new();
        wire::write_f64(&mut b, 50.0).expect("buf");
        wire::write_f64(&mut b, 65.0).expect("buf");
        wire::write_f64(&mut b, 50.0).expect("buf");
        wire::write_f32(&mut b, 90.0).expect("buf");
        wire::write_f32(&mut b, 45.0).expect("buf");
        wire::write_u8(&mut b, FLAG_ON_GROUND).expect("buf");
        assert_eq!(
            apply(ids::MOVE_PLAYER_POS_ROT, &b, &mut state).expect("decoded"),
            Update::Moved
        );
        assert!((state.yaw - 90.0).abs() < f32::EPSILON);
        assert!((state.pitch - 45.0).abs() < f32::EPSILON);
        assert_eq!((state.x, state.y, state.z), (0.0, 0.0, 0.0));
    }

    // ---------------------------------------------------------------------
    // `Inventory.placeItemBackInInventory` and the two probes under it.
    //
    // **These carry the whole of this method's stack arithmetic on their
    // own, and that is a structural fact rather than a preference.**
    // `oracle-runner`'s `hold` verb sends `count = 1`, so no gameplay
    // scenario can build a stack of more than one item; the spill across two
    // slots and the full-inventory leftover below are unreachable from the
    // corpus in principle, not merely untested by it. If these go, nothing
    // else is looking.
    // ---------------------------------------------------------------------

    /// The selected slot is probed **first**, ahead of the forward scan, so a
    /// stack that came out of the hand goes back into the hand.
    ///
    /// The lower matching slot is what makes this an ordering assertion
    /// rather than a merging one: a scan-only implementation passes every
    /// other test here and fails this one.
    #[test]
    fn a_returning_stack_goes_back_to_the_hand_it_came_from() {
        let mut inv = Hotbar::new();
        inv.set_stack(0, crate::item::ItemStack::new("minecraft:oak_log", 1));
        inv.set_stack(4, crate::item::ItemStack::new("minecraft:oak_log", 1));
        inv.selected = 4;

        let back =
            inv.place_item_back_in_inventory(crate::item::ItemStack::new("minecraft:oak_log", 1));

        assert_eq!(back.touched, vec![4], "the selected slot won the probe");
        assert!(back.leftover.is_empty());
        assert_eq!(inv.stack(4).count(), 2);
        assert_eq!(inv.stack(0).count(), 1, "slot 0 was not touched");
    }

    /// Nothing to merge with, so `getFreeSlot` answers — and it answers with
    /// the **first** empty slot, not the selected one.
    #[test]
    fn a_returning_stack_with_nothing_to_merge_takes_the_first_free_slot() {
        let mut inv = Hotbar::new();
        inv.set_stack(0, crate::item::ItemStack::new("minecraft:stick", 1));
        inv.selected = 5;

        let back =
            inv.place_item_back_in_inventory(crate::item::ItemStack::new("minecraft:oak_log", 1));

        assert_eq!(back.touched, vec![1]);
        assert_eq!(
            inv.stack(1),
            crate::item::ItemStack::new("minecraft:oak_log", 1)
        );
        assert!(
            inv.stack(5).is_empty(),
            "the selected slot had nothing to merge"
        );
    }

    /// `while (!stack.isEmpty())`: a stack too big for the slot with room
    /// tops that slot up and then takes a free one, in that order.
    #[test]
    fn a_returning_stack_spills_into_a_second_slot_in_order() {
        let mut inv = Hotbar::new();
        inv.set_stack(3, crate::item::ItemStack::new("minecraft:oak_log", 63));
        inv.selected = 3;

        let back =
            inv.place_item_back_in_inventory(crate::item::ItemStack::new("minecraft:oak_log", 5));

        assert_eq!(
            back.touched,
            vec![3, 0],
            "the partial stack is topped up first, then the first free slot"
        );
        assert!(back.leftover.is_empty());
        assert_eq!(inv.stack(3).count(), 64);
        assert_eq!(inv.stack(0).count(), 4);
    }

    /// **The item is handed back, never destroyed.** With every slot full of
    /// something that will not merge, both probes answer `-1` and vanilla
    /// drops what is left; this returns it so the caller can.
    #[test]
    fn a_full_inventory_hands_the_leftover_back_instead_of_destroying_it() {
        let mut inv = Hotbar::new();
        for i in 0..INVENTORY_SIZE {
            inv.set_stack(i, crate::item::ItemStack::new("minecraft:stone", 64));
        }

        let back =
            inv.place_item_back_in_inventory(crate::item::ItemStack::new("minecraft:oak_log", 3));

        assert!(back.touched.is_empty(), "nothing could be written");
        assert_eq!(
            back.leftover,
            crate::item::ItemStack::new("minecraft:oak_log", 3),
            "all three come back for the caller to drop"
        );
    }

    /// An empty stack is the common case — `clearContainer` walks every slot
    /// of the grid, and eight of a crafting table's nine are usually empty —
    /// and it must write nothing and send nothing.
    #[test]
    fn placing_an_empty_stack_back_writes_nothing() {
        let mut inv = Hotbar::new();
        let back = inv.place_item_back_in_inventory(crate::item::ItemStack::empty());
        assert!(back.touched.is_empty());
        assert!(back.leftover.is_empty());
        assert!(inv.get_free_slot() == Some(0), "the inventory is untouched");
    }

    /// A different item never merges, however much room the slot has.
    #[test]
    fn a_different_item_does_not_merge() {
        let mut inv = Hotbar::new();
        inv.set_stack(0, crate::item::ItemStack::new("minecraft:stick", 1));
        inv.selected = 0;

        let back =
            inv.place_item_back_in_inventory(crate::item::ItemStack::new("minecraft:oak_log", 1));

        assert_eq!(back.touched, vec![1], "it went past slot 0, not into it");
        assert_eq!(inv.stack(0).count(), 1);
    }

    /// This module's `mod ids` is a **second** copy of ids that
    /// `rustmc_proto::ids::play::sb` already holds, and nothing made the two
    /// agree. Found 2026-08-18 while censusing the serverbound surface for
    /// the chunk-send path: a `grep` for `sb::` across `crates/rustmc-server`
    /// reports 21 handled kinds and misses every `MOVE_PLAYER_*`, because
    /// these handlers reach them through the local table instead.
    ///
    /// The two agreed when this test was written — 0/30/31/32/33/43/53/56,
    /// checked entry by entry, not assumed. What did not exist was anything
    /// that would notice them diverging: a renumber in `ids.rs` would leave
    /// this module decoding the old numbers, and every packet would still
    /// decode, into the wrong arm.
    ///
    /// Deliberately a comparison and not a re-declaration: making `mod ids`
    /// re-export from `rustmc_proto` would be the better fix and is a wider
    /// change than the chunk-send item that found this. Until then, this is
    /// the thing that goes red.
    #[test]
    fn the_local_id_table_agrees_with_the_proto_one() {
        use rustmc_proto::ids::play::sb;
        assert_eq!(ids::ACCEPT_TELEPORTATION, sb::ACCEPT_TELEPORTATION);
        assert_eq!(ids::MOVE_PLAYER_POS, sb::MOVE_PLAYER_POS);
        assert_eq!(ids::MOVE_PLAYER_POS_ROT, sb::MOVE_PLAYER_POS_ROT);
        assert_eq!(ids::MOVE_PLAYER_ROT, sb::MOVE_PLAYER_ROT);
        assert_eq!(ids::MOVE_PLAYER_STATUS_ONLY, sb::MOVE_PLAYER_STATUS_ONLY);
        assert_eq!(ids::PLAYER_INPUT, sb::PLAYER_INPUT);
        assert_eq!(ids::SET_CARRIED_ITEM, sb::SET_CARRIED_ITEM);
        assert_eq!(ids::SET_CREATIVE_MODE_SLOT, sb::SET_CREATIVE_MODE_SLOT);
    }

    /// A table for the tests below. Leaked because [`Hotbar::with_item_props`]
    /// takes a `&'static` — production's copy is a `OnceLock` and lives for
    /// the process, and a test that borrowed a local would be testing a
    /// lifetime this code never has.
    fn props_16() -> &'static crate::itemprops::ItemProps {
        Box::leak(Box::new(
            crate::itemprops::ItemProps::parse(
                &serde_json::from_str(
                    r#"{"table":[
                        {"item":"minecraft:air","components":{"minecraft:max_stack_size":64}},
                        {"item":"minecraft:bucket","components":{"minecraft:max_stack_size":16}}
                    ]}"#,
                )
                .expect("test json"),
            )
            .expect("parse"),
        ))
    }

    /// **The discriminating pair.** With no table a bucket merges to 64,
    /// which is the bug `crate::itemprops` exists to remove; with the table
    /// it stops at 16 and the remaining 8 go to the next slot.
    ///
    /// Written as one test over two inventories rather than two tests,
    /// because the claim is the *difference*: a single assertion of 16 would
    /// pass against an implementation that had hard-coded 16 somewhere, and
    /// the 64 half is what says the table is being read at all.
    #[test]
    fn a_bucket_stacks_to_sixteen_only_with_the_table() {
        let mut without = Hotbar::new();
        let back = without
            .place_item_back_in_inventory(crate::item::ItemStack::new("minecraft:bucket", 24));
        assert_eq!(
            without.stack(0).count(),
            24,
            "without the table every item is DEFAULT_MAX_STACK_SIZE"
        );
        assert!(back.leftover.is_empty());

        let mut with = Hotbar::new().with_item_props(Some(props_16()));
        let back =
            with.place_item_back_in_inventory(crate::item::ItemStack::new("minecraft:bucket", 24));
        assert_eq!(with.stack(0).count(), 16, "Item.Properties.stacksTo(16)");
        assert_eq!(with.stack(1).count(), 8, "the rest went to the next slot");
        assert_eq!(back.touched, vec![0, 1]);
        assert!(back.leftover.is_empty());
    }

    /// `ItemStack.EMPTY.getMaxStackSize()` is `Items.AIR`'s 64, and this
    /// server represents empty as a stack with no name — so
    /// [`Hotbar::max_stack_size`] has to supply `minecraft:air` or an empty
    /// stack becomes unstackable and `moveItemStackTo`'s outside-the-loop
    /// `isStackable` test flips. A table that holds `minecraft:air` and
    /// nothing else useful is enough to catch a lookup that passed `""`,
    /// which would answer 1.
    #[test]
    fn an_empty_stack_is_air_and_stacks_to_sixty_four() {
        let inv = Hotbar::new().with_item_props(Some(props_16()));
        assert_eq!(inv.max_stack_size(&crate::item::ItemStack::empty()), 64);
        assert!(inv.is_stackable(&crate::item::ItemStack::empty()));
    }
}
