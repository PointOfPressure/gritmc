//! `AbstractContainerMenu.doClick` and the `broadcastChanges` that answers it.
//!
//! # The trap this module is shaped around
//!
//! `ServerboundContainerClickPacket` carries `changedSlots` and `carriedItem`,
//! and they are **the client's prediction of the result**, not a request.
//! `handleContainerClick` runs the click, writes those claims into the menu's
//! remote-belief table, and then calls `broadcastChanges`, which sends a
//! `container_set_slot` *only where the server's truth differs from what the
//! client is believed to think*.
//!
//! So a correct server answers a correct prediction with **silence** — and a
//! server with no containers at all also answers with silence. The two are
//! indistinguishable on the wire. That is ORACLE rule 27, and it is why
//! `tools/oracle-runner`'s `container_click` verb sends an empty `changedSlots`
//! map and an empty `carriedItem`: the harness models no inventory, so the
//! only honest claim it can make is "I predict nothing", and vanilla therefore
//! has to correct every slot the click moved.
//!
//! The same trap points inwards. **A container implementation that produces
//! silence is indistinguishable from one that does nothing.** Every test below
//! asserts a packet that a do-nothing server would not send, or asserts a
//! *specific* silence that an over-eager server would break — and `561` in the
//! gameplay corpus is the second of those made into a check.
//!
//! # Why this is pure
//!
//! Nothing here touches a socket. [`handle_click`] returns a `Vec<Reply>` and
//! the caller writes them out. That is not tidiness: a click's whole
//! observable behaviour is *which packets, in which order, with which state
//! ids*, and a function that sent them itself could only be tested by standing
//! up a server and reading a recording. As a value it can be tested against
//! every branch of `doClick` in a unit test, which is what defect class #19
//! demands — eight rail tests once passed under four injected bugs because the
//! scenario never reached a whole branch of the method.
//!
//! # The order, from `ServerGamePacketListenerImpl.handleContainerClick`
//!
//! ```text
//!   containerId != menu.containerId          -> return, silently
//!   isSpectator || isDeadOrDying             -> sendAllDataToRemote, return
//!   !menu.stillValid(player)                 -> return, silently
//!   !menu.isValidSlotIndex(slotNum)          -> return, silently
//!   mismatch = packet.stateId != menu.stateId
//!   menu.suppressRemoteUpdates()
//!   menu.clicked(slotNum, button, input, player)
//!   for (k, v) in packet.changedSlots: menu.setRemoteSlotUnsafe(k, v)
//!   menu.setRemoteCarried(packet.carriedItem)
//!   menu.resumeRemoteUpdates()
//!   mismatch ? menu.broadcastFullState() : menu.broadcastChanges()
//! ```
//!
//! Four silent early returns before anything happens, and they are silent in
//! four different ways that all look the same on the wire. The `mismatch`
//! branch is the one worth naming: a client whose state id has drifted is
//! answered with the **whole menu** rather than a correction, which is a
//! different packet on a different channel, and it is reachable from a
//! scenario that clicks with a stale state id.

use crate::blockentity::BlockEntity;
use crate::item::ItemStack;
use crate::menu::{increment_state_id, MenuKind, MenuSpec, QuickMove};
use crate::player::{Hotbar, HOTBAR_SIZE, MAIN_INVENTORY_START};

/// `ContainerInput` — the enum that used to be `ClickType`.
///
/// The rename matters more than it looks: the seven ids equal the seven
/// ordinals, so a server that read the old enum would decode every value
/// correctly and would still be wrong about what the field *is*.
/// `ByIdMap.continuous(..., OutOfBoundsStrategy.ZERO)` means an out-of-range
/// value decodes as `PICKUP` rather than being refused, so a bad value is
/// never a decode error — it is a legal click of a different kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerInput {
    Pickup,
    QuickMove,
    Swap,
    Clone,
    Throw,
    QuickCraft,
    PickupAll,
}

impl ContainerInput {
    /// `ContainerInput.BY_ID`, which is `ByIdMap.continuous` with
    /// `OutOfBoundsStrategy.ZERO` — so an unknown id is `PICKUP`, and this
    /// reproduces that rather than refusing, because refusing would make this
    /// server drop a click vanilla would act on.
    #[must_use]
    pub const fn from_id(id: i32) -> Self {
        match id {
            1 => Self::QuickMove,
            2 => Self::Swap,
            3 => Self::Clone,
            4 => Self::Throw,
            5 => Self::QuickCraft,
            6 => Self::PickupAll,
            _ => Self::Pickup,
        }
    }
}

/// `ClickAction` — which of the two a button number means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClickAction {
    Primary,
    Secondary,
}

/// `AbstractContainerMenu.SLOT_CLICKED_OUTSIDE`.
pub const SLOT_CLICKED_OUTSIDE: i32 = -999;

/// One packet the click owes the client.
///
/// `SetCursor` is `ClientboundSetCursorItemPacket` and is **not** a
/// `container_set_slot`: `ServerPlayer`'s synchronizer sends a different
/// packet for the carried stack, and — the part that decides packet counts —
/// it does **not** call `incrementStateId`. A server that answered the cursor
/// with a `container_set_slot` would put the state id one ahead of vanilla's
/// on every later packet of that menu, and `container_set_slot`'s projection
/// carries the state id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// `ClientboundContainerSetSlotPacket(containerId, incrementStateId(), slot, stack)`.
    SetSlot {
        container_id: i32,
        state_id: i32,
        slot: i16,
        stack: ItemStack,
    },
    /// `ClientboundSetCursorItemPacket(stack)`.
    SetCursor { stack: ItemStack },
    /// `ClientboundContainerSetContentPacket` — `broadcastFullState`.
    SetContent {
        container_id: i32,
        state_id: i32,
        items: Vec<ItemStack>,
        carried: ItemStack,
    },
    /// `ClientboundContainerSetDataPacket(containerId, id, value)` —
    /// `ServerPlayer`'s synchronizer `sendDataChange`. Carries **no state
    /// id**: `synchronizeDataSlotToRemote` never calls `incrementStateId`, so
    /// a furnace burning for a minute sends 1,200 of these and moves the
    /// menu's state id by none of them.
    SetData {
        container_id: i32,
        id: i16,
        value: i32,
    },
}

/// The mutable half of an open menu — everything `AbstractContainerMenu` holds
/// that is not a slot's own storage.
#[derive(Debug, Clone)]
pub struct Menu {
    pub container_id: i32,
    pub state_id: i32,
    pub spec: MenuSpec,
    /// `AbstractContainerMenu.carried`.
    pub carried: ItemStack,
    /// `AbstractContainerMenu.remoteSlots` — what the server believes the
    /// client thinks each slot holds.
    ///
    /// Modelled as plain stacks compared by equality, which is
    /// `RemoteSlot.Synchronized.matches`. Vanilla's other implementation,
    /// `RemoteSlot.Hash`, is installed by `setRemoteSlotUnsafe` from the
    /// packet's `changedSlots` and compares a `HashedStack` instead — and
    /// **nothing reaches it here**, because the harness sends an empty map.
    /// Named rather than silently conflated: a real client fills that map on
    /// every click, so the day this server faces one, this is the field that
    /// is wrong.
    pub remote_slots: Vec<ItemStack>,
    /// Whether [`Self::remote_slots`] actually knows what the client thinks.
    ///
    /// **The honest half of the `RemoteSlot.Hash` gap.** `setRemoteSlotUnsafe`
    /// installs a belief built from a `HashedStack` — a *hash* of the client's
    /// stack, not the stack — and matching it needs vanilla's own hashing of
    /// the item and its component patch, which this server does not implement.
    /// Where a click claims a slot this server cannot resolve, the belief is
    /// marked unknown and [`broadcast_changes`] corrects the slot
    /// unconditionally.
    ///
    /// That errs towards **sending** rather than towards staying silent, and
    /// the direction is deliberate: an unnecessary correction is a wrong
    /// packet, and a missing one is a client left rendering an item that is
    /// not there. It is also unreachable from the gameplay corpus, whose
    /// `container_click` verb sends an empty map on purpose — so nothing
    /// measures it today, and this comment is the only thing standing between
    /// the gap and a future reader who assumes it was modelled.
    pub belief_known: Vec<bool>,
    /// `AbstractContainerMenu.remoteCarried`.
    pub remote_carried: ItemStack,
    /// `AbstractContainerMenu.remoteDataSlots` — the last value of each data
    /// slot this player was sent. A fresh menu believes each slot holds its
    /// fresh value ([`crate::menu::MenuSpec::initial_data`]: 0, or -1 for an
    /// enchanting table's clues), and `broadcastChanges` sends exactly the
    /// slots that have moved off it.
    pub remote_data: Vec<i32>,
    /// A crafting menu's 3x3, which lives on the **menu** and not on any
    /// block entity — `CraftingTableBlock` is not an `EntityBlock`. Empty for
    /// a container menu.
    pub grid: Vec<ItemStack>,
    /// A crafting menu's result slot.
    pub result: ItemStack,
    /// `AbstractContainerMenu.dataSlots` a menu holds itself, by value, in
    /// `addDataSlot` order — a workstation's cost, selection or clues. A slot
    /// a block entity backs reads the block entity instead (see
    /// `data_values_of`).
    pub data: Vec<i32>,
    /// A workstation's own fields — see [`crate::workstation::State`].
    pub ws: crate::workstation::State,
}

impl Menu {
    /// A freshly opened menu: `stateId` 0, nothing carried, and a remote
    /// belief table sized to the menu.
    #[must_use]
    pub fn new(container_id: i32, spec: MenuSpec) -> Self {
        Self {
            container_id,
            state_id: 0,
            spec,
            carried: ItemStack::empty(),
            remote_slots: vec![ItemStack::empty(); spec.slots],
            belief_known: vec![true; spec.slots],
            remote_carried: ItemStack::empty(),
            grid: vec![ItemStack::empty(); spec.grid_slots()],
            result: ItemStack::empty(),
            remote_data: (0..spec.data_slots)
                .map(|i| i32::from(spec.initial_data(i)))
                .collect(),
            data: spec.station.map_or_else(
                || vec![0; spec.data_slots],
                |st| st.initial_data(spec.data_slots, 0),
            ),
            ws: crate::workstation::State::default(),
        }
    }

    /// `AbstractContainerMenu.incrementStateId`, as the packet constructors
    /// call it: bump, then use the new value.
    fn next_state_id(&mut self) -> i32 {
        self.state_id = increment_state_id(self.state_id);
        self.state_id
    }

    /// `AbstractContainerMenu.isValidSlotIndex`.
    #[must_use]
    pub fn is_valid_slot_index(&self, slot: i32) -> bool {
        slot == SLOT_CLICKED_OUTSIDE || (slot >= 0 && (slot as usize) < self.spec.slots)
    }
}

/// Which storage a menu slot index actually names.
///
/// **The single most likely place in this module for an off-by-27.** Every
/// menu numbers its own slots first and then appends the player's 36 through
/// `addStandardInventorySlots`, which adds three rows of nine *main
/// inventory* and then the nine *hotbar* — while `Inventory` itself numbers
/// the hotbar 0..9 and the main inventory 9..36. The two orders are reversed
/// with respect to each other, so the conversion is not a shift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlotRef {
    /// The block entity's own storage.
    Container(usize),
    /// A crafting menu's grid, menu-local.
    Grid(usize),
    /// A crafting menu's result, menu-local.
    Result,
    /// An `Inventory` index — hotbar 0..9, main 9..36.
    Inventory(usize),
}

/// Where menu slot `i` lives.
fn slot_ref(spec: MenuSpec, i: usize) -> Option<SlotRef> {
    if i >= spec.slots {
        return None;
    }
    if i < spec.container_slots {
        return Some(match spec.kind {
            // `CraftingMenu` adds the **result first**, then the 3x3 grid, so
            // slot 0 is the result and 1..10 the grid. Every other menu here
            // is a flat container.
            MenuKind::Crafting => {
                if i == 0 {
                    SlotRef::Result
                } else {
                    SlotRef::Grid(i - 1)
                }
            }
            // The six `SimpleMenuProvider` menus add their slots in their own
            // order with no result-first rule, and every one of them is
            // menu-local — so slot `i` is grid slot `i`, sharing
            // `MenuKind::Crafting`'s storage without its layout. Nothing
            // recomputes: `slots_changed_crafting_grid` returns `None` for
            // any kind but `Crafting`, so a write here fills a slot and does
            // not invent a result.
            MenuKind::MenuLocal => SlotRef::Grid(i),
            MenuKind::Container | MenuKind::ContainerThenResult | MenuKind::ContainerOnly => {
                SlotRef::Container(i)
            }
        });
    }
    let j = i - spec.container_slots;
    // `CrafterMenu` adds its result slot **after** the player's 36, so the
    // index one past the inventory is the result. For every other kind that
    // index is past `spec.slots` and was refused above.
    if j >= crate::player::INVENTORY_SIZE {
        return (spec.kind == MenuKind::ContainerThenResult && j == crate::player::INVENTORY_SIZE)
            .then_some(SlotRef::Result);
    }
    // Three rows of nine, then the hotbar — `addStandardInventorySlots`.
    if j < INVENTORY_ROWS {
        Some(SlotRef::Inventory(MAIN_INVENTORY_START + j))
    } else {
        Some(SlotRef::Inventory(j - INVENTORY_ROWS))
    }
}

/// The 27 main-inventory slots `addInventoryExtendedSlots` adds before the
/// hotbar.
const INVENTORY_ROWS: usize = crate::player::INVENTORY_SIZE - HOTBAR_SIZE;

/// `AbstractContainerMenu.transferState(AbstractContainerMenu)` — offsets
/// 0-212 — reduced to the half of it this server can hold.
///
/// Vanilla builds a `HashBasedTable` keyed by **(`Slot.container`,
/// `Slot.getContainerSlot()`)** over the *closing* menu, then walks the
/// *receiving* menu and copies `lastSlots` and `remoteSlots` across wherever
/// both menus name the same storage cell. It is a join on the storage, not a
/// shift of indices — which is what makes it correct across two menus that
/// number their slots differently, and is exactly the case here: the log the
/// player picked up left `CraftingMenu` slot 37 and lands in `InventoryMenu`
/// slot 36, both of which are `(playerInventory, 0)`.
///
/// The join is transcribed as a scan of `menu`'s slots rather than as an
/// inverse of [`slot_ref`], because the scan **is** vanilla's algorithm and an
/// inverse would be a second thing to keep true.
///
/// # What is not carried, and why that is the whole of it
///
/// Only the 36 player-inventory cells are shared between any menu this server
/// opens and the player's own. `InventoryMenu`'s other ten — its result slot,
/// its 2x2 grid, four armour slots and the offhand — have no counterpart in a
/// crafting or container menu, so vanilla's `Table.get` returns null for them
/// and it copies nothing. This server does not model those ten at all, which
/// makes the two behaviours agree by construction rather than by accident.
///
/// The returned table is indexed by **`Inventory` index** — hotbar 0..9, main
/// 9..36 — and holds what the client is believed to think each cell contains.
#[must_use]
pub fn transferred_inventory_belief(menu: &Menu) -> Vec<ItemStack> {
    let mut belief = vec![ItemStack::empty(); crate::player::INVENTORY_SIZE];
    for i in 0..menu.spec.slots {
        if let Some(SlotRef::Inventory(index)) = slot_ref(menu.spec, i) {
            if let (Some(cell), Some(remote)) = (belief.get_mut(index), menu.remote_slots.get(i)) {
                *cell = remote.clone();
            }
        }
    }
    belief
}

/// How many slots `InventoryMenu` has: a result, a 2x2 grid, four armour
/// slots, `addStandardInventorySlots`' 36, and the offhand.
pub const INVENTORY_MENU_SLOTS: usize = 46;

/// The `Inventory` index an `InventoryMenu` slot names — [`slot_ref`]'s tail
/// for the one menu this server does not build a [`MenuSpec`] for.
///
/// `InventoryMenu` puts its result slot at 0, its 2x2 grid at 1..5 and armour
/// at 5..9, then calls the same `addStandardInventorySlots` every other menu
/// does — three rows of **main** inventory at 9..36, then the hotbar at
/// 36..45 — and finishes with the offhand at 45.
///
/// So `Inventory`'s two halves arrive in the opposite order to their own
/// numbering, which is why this is not a shift: menu 36 is `Inventory` 0.
///
/// `None` for the ten slots that name no `Inventory` cell — 0..9 and 45. This
/// server models none of them, and vanilla's `transferState` copies nothing
/// into them either, because no menu it can open shares that storage.
#[must_use]
pub fn inventory_index_for_menu_slot(slot: usize) -> Option<usize> {
    if (MAIN_INVENTORY_START..crate::player::INVENTORY_SIZE).contains(&slot) {
        Some(slot)
    } else if (crate::player::INVENTORY_SIZE..crate::player::INVENTORY_SIZE + HOTBAR_SIZE)
        .contains(&slot)
    {
        Some(slot - crate::player::INVENTORY_SIZE)
    } else {
        None
    }
}

/// The `InventoryMenu` slot an `Inventory` index sits in — the inverse of
/// [`inventory_index_for_menu_slot`], and total, because every one of the 36
/// indices is in that menu.
///
/// `broadcastChanges` walks slots and this walks indices, so a caller that has
/// just written an index needs this rather than a search. `None` only for an
/// index that is not an `Inventory` index at all.
#[must_use]
pub fn menu_slot_for_inventory_index(index: usize) -> Option<usize> {
    if index < HOTBAR_SIZE {
        Some(index + crate::player::INVENTORY_SIZE)
    } else if index < crate::player::INVENTORY_SIZE {
        Some(index)
    } else {
        None
    }
}

/// Everything a click can read and write, gathered so the transcribed method
/// below reads like `doClick` rather than like plumbing.
pub struct Slots<'a> {
    pub menu: &'a mut Menu,
    pub inventory: &'a mut Hotbar,
    /// The block entity behind a container menu. `None` for a crafting table,
    /// which has none, and also for a container whose block entity has gone —
    /// a chest broken while open — in which case every container slot reads
    /// empty and refuses a placement, rather than panicking.
    pub container: Option<&'a mut BlockEntity>,
    /// The **second** half of a `CompoundContainer` — a double chest's other
    /// block entity — or `None` for every menu over one container.
    ///
    /// `CompoundContainer.getItem(i)` is `i >= container1.getContainerSize()
    /// ? container2.getItem(i - size1) : container1.getItem(i)`, and
    /// `setItem` splits the same way, so container slot `i` names `container`
    /// below the first half's size and this above it. Which half is first is
    /// the combiner's decision and the caller's: see
    /// `crate::menu::double_chest`.
    pub second: Option<&'a mut BlockEntity>,
    /// The crafting recipes, for a menu that has a grid.
    ///
    /// `None` means a crafting table opens and computes nothing — which is
    /// what this server did before [`crate::recipes`] existed. It still
    /// *writes* an empty result and still sends the packet, because
    /// `slotChangedCraftingGrid` sets the result slot unconditionally and the
    /// miss branch sets it to `EMPTY`; a table with no recipes is a table that
    /// never matches, not one that never answers.
    pub recipes: Option<&'a crate::recipes::Recipes>,
    /// What a workstation menu reads from outside itself, and the effects it
    /// leaves for the caller to send. Empty for every other menu.
    pub env: crate::workstation::Env<'a>,
}

impl Slots<'_> {
    /// Which half of a compound container a container slot lives in, and its
    /// index there: `(false, s)` for the first (or only) container, `(true,
    /// s - size1)` for the second. The split is the first container's own
    /// size; a first half that has gone is taken to be half the menu's
    /// container slots, which is what it was when the menu opened.
    fn split(&self, s: usize) -> (bool, usize) {
        if self.second.is_none() {
            return (false, s);
        }
        let size1 = self
            .container
            .as_ref()
            .map_or(self.menu.spec.container_slots / 2, |c| c.size());
        if s >= size1 {
            (true, s - size1)
        } else {
            (false, s)
        }
    }

    /// `Slot.getItem()`, for [`crate::workstation`].
    pub(crate) fn peek(&self, i: usize) -> ItemStack {
        self.get(i)
    }

    /// `Slot.set(stack)` — the storage write, `slotsChanged` included — for
    /// [`crate::workstation`].
    pub(crate) fn put(&mut self, i: usize, stack: ItemStack) {
        self.set(i, stack);
    }

    /// `dataSlots.get(i).get()`.
    pub(crate) fn data(&self, i: usize) -> i32 {
        self.menu.data.get(i).copied().unwrap_or(0)
    }

    /// `dataSlots.get(i).set(v)`.
    pub(crate) fn set_data(&mut self, i: usize, v: i32) {
        if let Some(d) = self.menu.data.get_mut(i) {
            *d = v;
        }
    }

    /// `Slot.getItem()`.
    fn get(&self, i: usize) -> ItemStack {
        match slot_ref(self.menu.spec, i) {
            Some(SlotRef::Container(s)) => {
                let (second, s) = self.split(s);
                let c = if second {
                    self.second.as_ref()
                } else {
                    self.container.as_ref()
                };
                c.map_or_else(ItemStack::empty, |c| c.item(s))
            }
            Some(SlotRef::Grid(s)) => self
                .menu
                .grid
                .get(s)
                .cloned()
                .unwrap_or_else(ItemStack::empty),
            Some(SlotRef::Result) => self.menu.result.clone(),
            Some(SlotRef::Inventory(s)) => self.inventory.stack(s),
            None => ItemStack::empty(),
        }
    }

    /// `Slot.set` / `Slot.setByPlayer` — the storage write. The two differ
    /// only in `setByPlayer`'s extra `onQuickCraft` hook, which no slot type
    /// here overrides.
    ///
    /// # A crafting grid recomputes on the write, not at the end of the click
    ///
    /// `TransientCraftingContainer.setItem` calls `menu.slotsChanged(this)`
    /// at offset 15 and `removeItem` at offset 22 (when it removed anything);
    /// `CraftingMenu.slotsChanged` is `slotChangedCraftingGrid`, which sets
    /// the result and **sends it directly**. Its `setChanged` — what `doClick`
    /// calls at the end of the click — is a bare `return`. So the recipe runs
    /// once per storage write, inside the click, and every run is one
    /// `container_set_slot` for slot 0 ahead of the click's own corrections:
    /// one for a placement, **two** for a take that empties a cell
    /// (`tryRemove`'s `remove`, then its `setByPlayer(EMPTY)`), none for a
    /// click that writes nothing. This replaced a flag recomputing once
    /// after the click, which was right for every click the corpus made and
    /// wrong for a take, a gather or a quick-move touching a grid cell twice.
    fn set(&mut self, i: usize, stack: ItemStack) {
        self.write(i, stack);
        if self.is_crafting_grid(i) {
            crafting_grid_changed(self);
        }
    }

    /// Whether menu slot `i` is a cell of a crafting table's grid.
    fn is_crafting_grid(&self, i: usize) -> bool {
        self.menu.spec.kind == MenuKind::Crafting
            && matches!(slot_ref(self.menu.spec, i), Some(SlotRef::Grid(_)))
    }

    /// The storage write with every hook a crafting grid has, minus the
    /// recompute: `ItemStack.setCount` on the slot's own object followed by
    /// `Slot.setChanged()`, which for a crafting grid does nothing — the
    /// shape of `moveItemStackTo`'s merge phase and of a quick-move's
    /// partial write-back. For every other slot it is [`Self::set`].
    fn set_in_place(&mut self, i: usize, stack: ItemStack) {
        if self.is_crafting_grid(i) {
            self.write(i, stack);
        } else {
            self.set(i, stack);
        }
    }

    /// The storage write and the workstation hook, without the crafting one.
    fn write(&mut self, i: usize, stack: ItemStack) {
        match slot_ref(self.menu.spec, i) {
            Some(SlotRef::Container(s)) => {
                let (second, s) = self.split(s);
                // `Slot.set` is `container.setItem`, and for the furnace
                // family that is the override that recomputes the cook total
                // — see `BlockEntity::set_item_from_menu`. A double chest's
                // second half is the compound's second container.
                let recipes = self.recipes;
                let c = if second {
                    self.second.as_mut()
                } else {
                    self.container.as_mut()
                };
                if let Some(c) = c {
                    c.set_item_from_menu(s, stack, recipes);
                }
            }
            Some(SlotRef::Grid(s)) => {
                if let Some(g) = self.menu.grid.get_mut(s) {
                    *g = stack;
                }
            }
            Some(SlotRef::Result) => self.menu.result = stack,
            Some(SlotRef::Inventory(s)) => self.inventory.set_stack(s, stack),
            None => {}
        }
        // A workstation's input container calls `menu.slotsChanged` from its
        // own `setChanged`, which `SimpleContainer.setItem` runs on every
        // write — synchronously, inside the click. Its result container is a
        // `ResultContainer`, whose `setChanged` is empty. See
        // `crate::workstation`'s header.
        if let Some(station) = self.menu.spec.station {
            if i < self.menu.spec.container_slots && !station.is_result(i) {
                crate::workstation::slots_changed(self);
            }
        }
    }

    /// `Slot.mayPlace(ItemStack)`.
    ///
    /// The default is `return true`. **`ResultSlot.mayPlace` is `return
    /// false`** — a crafting result is an output and nothing may be put into
    /// it — and that single override is the reason this is a method and not a
    /// constant. Without it a click carrying a log would deposit it in the
    /// result slot and the recipe would then be computed over a grid the log
    /// never reached.
    ///
    /// # The extra refusal that is not vanilla's, and why it is here
    ///
    /// A container slot whose block entity is **gone** also refuses. Vanilla
    /// has no such case: `Slot.container` is a Java reference to the
    /// `ChestBlockEntity`, so a chest broken while open leaves the menu
    /// writing into a detached object, and `stillValid` closes the menu on the
    /// next tick anyway. Here the store is a map and a missing entry has no
    /// object to write into — and the first version of this **silently
    /// destroyed the stack**: `safe_insert` split it off the cursor, handed it
    /// to `set`, and `set` dropped it on the floor because there was nowhere
    /// to put it. Refusing keeps the item on the cursor, which is the only
    /// answer that loses nothing.
    ///
    /// Named rather than hidden, because it is a place this server is
    /// deliberately *not* bit-identical to vanilla. Closing it properly needs
    /// `stillValid`, which is listed as a known gap in [`crate::menu`].
    fn may_place(&self, i: usize, stack: &ItemStack) -> bool {
        // A workstation's result slot is an anonymous `Slot` subclass whose
        // `mayPlace` is `iconst_0; ireturn` (`StonecutterMenu$2`,
        // `ItemCombinerMenu`'s result definition, `GrindstoneMenu$4`, ...),
        // and its inputs carry their own predicates.
        if let Some(station) = self.menu.spec.station {
            if i < self.menu.spec.container_slots {
                return !station.is_result(i)
                    && crate::workstation::may_place(self, station, i, stack);
            }
        }
        match slot_ref(self.menu.spec, i) {
            Some(SlotRef::Result) => false,
            // **The index has to be inside the container, not merely have
            // one.** Vanilla cannot express the disagreement — a
            // `ChestMenu`'s slot count comes from the container it was built
            // over — but this server holds the menu spec and the block entity
            // separately, so a position whose block entity has been replaced
            // by a *smaller* one leaves a menu addressing slots that no
            // longer exist.
            //
            // `Kind::Sign` made that reachable rather than theoretical: its
            // `container_size()` is 0, so every container slot of any menu is
            // outside it. Without this the click would pass `may_place`,
            // reach `safeInsert`, split the stack off the cursor and hand it
            // to `set`, whose `set_item` writes nowhere — the third instance
            // of an item silently ceasing to exist on this path, and the
            // first one no scenario can currently reach.
            //
            // Inside it, the menu's own `Slot` subclass decides — the fuel
            // slot, the result slot, the three bottle slots. See
            // `crate::menuslots`. A double chest's slots past the first half
            // are the second half's.
            Some(SlotRef::Container(s)) => {
                let (second, h) = self.split(s);
                let c = if second {
                    self.second.as_ref()
                } else {
                    self.container.as_ref()
                };
                c.is_some_and(|c| h < c.size())
                    && self.menu.spec.family().is_none_or(|family| {
                        crate::menuslots::may_place(
                            crate::menuslots::class_of(family, s),
                            stack,
                            &self.slot_tables(),
                        )
                    })
            }
            Some(_) => true,
            // **A slot that does not exist refuses**, and this was found by
            // the mutation sweep rather than by reading. With
            // `isValidSlotIndex` removed, a click on slot 63 of a 63-slot menu
            // used to reach `safeInsert`, which asked `may_place` — answered
            // `true` by a catch-all arm — split the stack off the cursor, and
            // handed it to `set`, which dropped it because there was no slot
            // to write to. The item ceased to exist. Two guards now have to
            // fail before that can happen instead of one.
            None => false,
        }
    }

    /// `Slot.mayPickup(Player)`. The default is `return true`. `ResultSlot`
    /// does **not** override it — a crafting result may be taken, it just may
    /// not be filled. **`NonInteractiveResultSlot` does**, with `iconst_0`:
    /// the crafter's result slot shows what the next pulse will craft and
    /// cannot be taken by hand. A workstation's slots answer through
    /// [`crate::workstation::may_pickup`].
    fn may_pickup(&self, i: usize) -> bool {
        if self.menu.spec.kind == MenuKind::ContainerThenResult
            && slot_ref(self.menu.spec, i) == Some(SlotRef::Result)
        {
            return false;
        }
        crate::workstation::may_pickup(self, i).unwrap_or(true)
    }

    /// `Slot.getMaxStackSize(ItemStack)` — offsets 0-11 of the one-argument
    /// form, which is `Math.min(this.getMaxStackSize(), stack.getMaxStackSize())`.
    ///
    /// The two halves are different quantities and only one of them was ever
    /// right here. `this.getMaxStackSize()` is
    /// `container.getMaxStackSize()`, 64 for every `Container` this server
    /// has, and [`crate::item::DEFAULT_MAX_STACK_SIZE`] is the honest name
    /// for it. `stack.getMaxStackSize()` is
    /// `getOrDefault(MAX_STACK_SIZE, 1)` off the item's components, and until
    /// `crate::itemprops` existed this server had no way to answer it, so it
    /// answered 64 — **wrong for 295 of the 1,537 items in the registry**,
    /// among them every tool, every filled bucket and every piece of armour.
    ///
    /// The table arrives through [`Hotbar`], which already carries it for the
    /// pickup path, so this needs no new field on [`Slots`] and no change at
    /// its thirty-nine construction sites. Without a table it is the old
    /// answer exactly, so a checkout with no vanilla extract behaves as it
    /// did.
    fn max_stack_size(&self, i: usize, stack: &ItemStack) -> i32 {
        // A patched `max_stack_size` wins over the prototype's.
        let stack_max = stack.patched_max_stack_size().unwrap_or_else(|| {
            self.inventory
                .item_props()
                .filter(|_| !stack.is_empty())
                .map_or_else(
                    || stack.max_stack_size(),
                    |p| p.max_stack_size(stack.item()),
                )
        });
        // A slot subclass that overrides the cap — `FurnaceFuelSlot`'s
        // bucket, `PotionSlot`'s 1 — replaces the whole rule, not one half of
        // the `min`: the fuel slot's override returns its literal 1 without
        // consulting the stack.
        if let (Some(SlotRef::Container(s)), Some(family)) =
            (slot_ref(self.menu.spec, i), self.menu.spec.family())
        {
            if let Some(cap) =
                crate::menuslots::max_stack_size(crate::menuslots::class_of(family, s), stack)
            {
                return cap.min(stack_max);
            }
        }
        // `this.getMaxStackSize()` is the slot's own, which a workstation
        // slot may override — `EnchantmentMenu$2`'s is 1.
        let slot_max = self
            .menu
            .spec
            .station
            .filter(|_| i < self.menu.spec.container_slots)
            .and_then(|st| crate::workstation::slot_max_stack_size(st, i))
            .unwrap_or(crate::item::DEFAULT_MAX_STACK_SIZE);
        slot_max.min(stack_max)
    }

    /// The tables [`crate::menuslots`] reads: the server's one fuel table,
    /// the item tags this session's recipes were expanded against, and the
    /// vanilla brewing table.
    fn slot_tables(&self) -> crate::menuslots::Tables<'_> {
        crate::menuslots::Tables {
            fuels: crate::fuel::FuelValues::loaded(),
            tags: self.recipes.map(crate::recipes::Recipes::tags),
            brewing: &crate::brewing::VanillaBrewing,
        }
    }

    /// `Slot.safeInsert(ItemStack, int)` — put up to `count` of `stack` in,
    /// returning what is left over.
    ///
    /// Transcribed from offsets 0..98. The two arms after the `n <= 0` guard
    /// are not interchangeable: an empty slot is *split into*, and an
    /// occupied one is grown only when the items match, and a mismatched
    /// occupied slot silently takes nothing and hands the whole stack back.
    fn safe_insert(&mut self, i: usize, mut stack: ItemStack, count: i32) -> ItemStack {
        if stack.is_empty() || !self.may_place(i, &stack) {
            return stack;
        }
        let existing = self.get(i);
        let n = count
            .min(stack.count())
            .min(self.max_stack_size(i, &stack) - existing.count());
        if n <= 0 {
            return stack;
        }
        if existing.is_empty() {
            let put = stack.split(n);
            self.set(i, put);
        } else if ItemStack::is_same_item_same_components(&existing, &stack) {
            stack.shrink(n);
            let mut grown = existing;
            grown.grow(n);
            self.set(i, grown);
        }
        stack
    }

    /// `Slot.tryRemove(int, int, Player)` — take up to `count`, but never
    /// more than `max_take`.
    ///
    /// Transcribed from offsets 0..84. `None` is vanilla's `Optional.empty()`
    /// and means nothing was taken; the caller must then leave the cursor
    /// alone, which is what `ifPresent` does.
    fn try_remove(&mut self, i: usize, count: i32, max_take: i32) -> Option<ItemStack> {
        if !self.may_pickup(i) {
            return None;
        }
        // `allowModification` is `mayPickup && mayPlace(getItem())`, so it is
        // false exactly for the result slot — and then a partial take is
        // refused: you take the whole crafted stack or none of it.
        let held = self.get(i);
        let allow_modification = self.may_pickup(i) && self.may_place(i, &held);
        if !allow_modification && max_take < held.count() {
            return None;
        }
        let take = count.min(max_take);
        let mut remaining = held;
        let taken = if self.removes_whole(i) {
            std::mem::replace(&mut remaining, ItemStack::empty())
        } else {
            remaining.split(take)
        };
        if taken.is_empty() {
            return None;
        }
        // `remove(amount)` is `container.removeItem`, which for a crafting
        // grid recomputes (it removed something); then offsets 61-76, `if
        // (getItem().isEmpty()) setByPlayer(EMPTY, taken)` — a `setItem`, and
        // a second recompute.
        let emptied = remaining.is_empty();
        self.set(i, remaining);
        if emptied && self.is_crafting_grid(i) {
            crafting_grid_changed(self);
        }
        Some(taken)
    }

    /// Whether `Slot.remove(amount)` on menu slot `i` ignores `amount`.
    ///
    /// `ResultContainer.removeItem(int, int)` is `ContainerHelper.takeItem(
    /// itemStacks, 0)` — offsets 0-8, the count argument never loaded — so a
    /// slot over one hands out its **whole** stack whatever was asked for. A
    /// right-click on four crafted planks takes four, not `(4 + 1) / 2`, and
    /// a single-item `THROW` throws all of them. The crafting table's result
    /// is one (`AbstractCraftingMenu`'s constructor), as are the stonecutter's
    /// and the cartography table's `resultContainer`, `ItemCombinerMenu`'s
    /// `resultSlots` (anvil, smithing table) and the grindstone's. **The
    /// loom's is not**: its `outputContainer` is `LoomMenu$2`, a
    /// `SimpleContainer`, which splits.
    fn removes_whole(&self, i: usize) -> bool {
        use crate::workstation::Station;
        if self.menu.spec.kind == MenuKind::Crafting {
            return matches!(slot_ref(self.menu.spec, i), Some(SlotRef::Result));
        }
        self.menu
            .spec
            .station
            .is_some_and(|st| st != Station::Loom && st.is_result(i))
    }

    /// `Slot.safeTake(int, int, Player)`, offsets 0-32.
    ///
    /// ```text
    ///   0- 7   optional = tryRemove(count, maxTake, player)
    ///   9-18   optional.ifPresent(stack -> onTake(player, stack))
    ///  21-32   return optional.orElse(ItemStack.EMPTY)
    /// ```
    ///
    /// So it is [`Self::try_remove`] with `None` flattened to an empty stack,
    /// plus [`Self::on_take`]. Until the crafting table's result consumed its
    /// grid the `onTake` here was left out as a no-op for every slot this
    /// server had; it is not one for a result slot, and `THROW` reaches a
    /// result slot through this call — so throwing a crafted item out of a
    /// table spends the grid, and throwing an anvil's result spends its
    /// inputs, exactly as taking them onto the cursor does.
    fn safe_take(&mut self, i: usize, count: i32, max_take: i32) -> ItemStack {
        match self.try_remove(i, count, max_take) {
            Some(taken) => {
                self.on_take(i, &taken);
                taken
            }
            None => ItemStack::empty(),
        }
    }

    /// `CraftingMenu.quickMoveStack(player, index)`, offsets 0-233.
    ///
    /// ```text
    ///   index 0 (result):  onCraftedBy; moveItemStackTo(10, 46, reverse)
    ///                      or EMPTY; slot.onQuickCraft(stack, copy)
    ///   index 10..46:      moveItemStackTo(1, 10) — the grid first — and
    ///                      failing that 37..46 from the main inventory or
    ///                      10..37 from the hotbar, or EMPTY
    ///   index 1..10:       moveItemStackTo(10, 46) or EMPTY
    ///  172-193  stack empty ? setByPlayer(EMPTY) : setChanged()
    ///  196-211  nothing moved -> EMPTY
    ///  214-217  slot.onTake(player, stack)
    ///  218-231  index 0: player.drop(stack, false)
    /// ```
    ///
    /// On the result that `onTake` spends the grid and recomputes it, and
    /// `doClick`'s loop calls this again while the result still holds the same
    /// item: a shift-click crafts until the grid or the inventory runs out.
    /// `onCraftedBy` and `onQuickCraft` feed statistics and the recipe award
    /// only. The `drop` of a result that did not fit is a gap for
    /// [`crafting_result_on_take`]'s reason.
    ///
    /// The write-back keeps vanilla's two arms apart because a grid tells them
    /// apart: `setByPlayer(EMPTY)` is a `setItem` and recomputes,
    /// `setChanged()` on a partly moved grid stack does not.
    pub(crate) fn crafting_quick_move_stack(&mut self, index: usize) -> ItemStack {
        const GRID: (usize, usize) = (1, 10);
        const INVENTORY: (usize, usize) = (10, 46);
        const HOTBAR_START: usize = 37;
        let mut stack = self.get(index);
        if stack.is_empty() {
            return ItemStack::empty();
        }
        let copy = stack.clone();
        let moved = if index == 0 {
            self.move_item_stack_to(&mut stack, INVENTORY.0, INVENTORY.1, true)
        } else if (INVENTORY.0..INVENTORY.1).contains(&index) {
            self.move_item_stack_to(&mut stack, GRID.0, GRID.1, false)
                || if index < HOTBAR_START {
                    self.move_item_stack_to(&mut stack, HOTBAR_START, INVENTORY.1, false)
                } else {
                    self.move_item_stack_to(&mut stack, INVENTORY.0, HOTBAR_START, false)
                }
        } else {
            self.move_item_stack_to(&mut stack, INVENTORY.0, INVENTORY.1, false)
        };
        if !moved {
            return ItemStack::empty();
        }
        if stack.is_empty() {
            self.set(index, ItemStack::empty());
        } else {
            self.set_in_place(index, stack.clone());
        }
        if stack.count() == copy.count() {
            return ItemStack::empty();
        }
        self.on_take(index, &stack);
        copy
    }

    /// `Slot.onTake(player, stack)` for menu slot `i`.
    ///
    /// `ResultSlot.onTake` for a crafting table's result — see
    /// [`crafting_result_on_take`] — the workstation's own override for a
    /// workstation's result, and for every other slot `Slot.onTake`, whose
    /// whole body is `setChanged()` and which does nothing here (see
    /// [`Slots::set`]).
    fn on_take(&mut self, i: usize, taken: &ItemStack) {
        if self.menu.spec.kind == MenuKind::Crafting
            && matches!(slot_ref(self.menu.spec, i), Some(SlotRef::Result))
        {
            crafting_result_on_take(self);
        } else {
            crate::workstation::on_take(self, i, taken);
        }
    }

    /// `AbstractContainerMenu.moveItemStackTo(ItemStack, int, int, boolean)`,
    /// offsets 0-330. Returns `flag` — whether anything moved.
    ///
    /// # It is two loops, and only the first one is a loop over destinations
    ///
    /// This is the half of `QUICK_MOVE` that decides *where* a stack lands,
    /// and its shape is not the obvious one:
    ///
    /// * **Phase 1, offsets 16-190 — merge.** Walks the whole range and grows
    ///   *every* slot already holding the same item, one after another, until
    ///   the source is empty or the range runs out. Guarded once, at offsets
    ///   16-20, by `stack.isStackable()`; the `isEmpty` test at offset 23 is
    ///   the loop condition, not part of that guard.
    /// * **Phase 2, offsets 193-328 — place.** Walks the range again looking
    ///   for an empty slot that `mayPlace` accepts, splits
    ///   `min(count, maxStackSize)` into it — and then **`goto 328`, out of
    ///   the method** (offset 308). One empty slot per call, never two.
    ///
    /// That asymmetry is the whole reason `doClick` wraps the call in a loop:
    /// a stack too large for one destination slot needs one `quickMoveStack`
    /// call per slot it ends up occupying. See [`do_click_quick_move`].
    ///
    /// **And the break at 308 is the one thing here no test can catch, which
    /// is said rather than left to be found.** Deleting it — letting phase 2
    /// fill every empty slot in one call — passes all 29 tests in this module,
    /// because the outer loop's next iteration re-runs phase 1 over slots the
    /// placement has just filled *to their maximum*, so the merge declines and
    /// the final inventory is identical. The two arrangements can only differ
    /// when two slots in one range have different `getMaxStackSize`, and
    /// [`Self::max_stack_size`] ignores its index today. Transcribed as
    /// vanilla writes it because the equivalence is a property of this
    /// server's current stack-size model and not of the method.
    ///
    /// `reverse` starts at `end - 1` and counts down. `ChestMenu` passes it
    /// `true` for the container-to-player direction, which is why a stone
    /// taken out of a chest lands in the **last** hotbar slot rather than the
    /// first free inventory slot.
    ///
    /// # Two places this is not a literal transcription
    ///
    /// Vanilla's `stack` argument is the source slot's own `ItemStack` object,
    /// mutated in place, so the source slot drains *while* the phases run.
    /// Here it is a `&mut` to a copy that [`Self::quick_move_stack`] writes
    /// back afterwards. The two are indistinguishable while the source slot
    /// lies outside `start..end`, which is true of both of `ChestMenu`'s
    /// calls and is the only reason the simpler arrangement is allowed.
    ///
    /// Vanilla's phase-1 write is `existing.setCount(..)` plus
    /// `slot.setChanged()`, which for a crafting container is a bare `return`
    /// and therefore does **not** run the recipe — which is why the merge
    /// writes through [`Self::set_in_place`] and only the placement through
    /// [`Self::set`]. `CraftingMenu.quickMoveStack` moves into the grid with
    /// this method, so the difference is a packet: a merge onto a grid cell
    /// sends no result, a placement into an empty one sends one.
    /// [`Self::move_item_stack_to`], for [`crate::workstation`].
    pub(crate) fn move_to(
        &mut self,
        stack: &mut ItemStack,
        start: usize,
        end: usize,
        reverse: bool,
    ) -> bool {
        self.move_item_stack_to(stack, start, end, reverse)
    }

    /// `Slot.mayPlace(stack)`, for [`crate::workstation`].
    pub(crate) fn accepts(&self, i: usize, stack: &ItemStack) -> bool {
        self.may_place(i, stack)
    }

    fn move_item_stack_to(
        &mut self,
        stack: &mut ItemStack,
        start: usize,
        end: usize,
        reverse: bool,
    ) -> bool {
        let mut moved = false;
        if start >= end {
            // Vanilla has no such guard; both of its loops simply never run
            // for an empty range and `flag` stays false. Written out so that
            // `end - 1` below cannot underflow.
            return false;
        }
        // Offsets 0-14: `i = reverse ? endIndex - 1 : startIndex`.
        let step: isize = if reverse { -1 } else { 1 };
        let first = if reverse { end - 1 } else { start };

        // Phase 1 — offsets 16-190.
        if stack.is_stackable() {
            let mut i = first;
            // Offset 23 is `stack.isEmpty()`; offsets 30-47 are the range
            // test, written the two ways round so that `reverse` compares
            // against `start` and forward against `end`.
            while !stack.is_empty() && i >= start && i < end {
                let existing = self.get(i);
                if !existing.is_empty() && ItemStack::is_same_item_same_components(stack, &existing)
                {
                    // Offsets 88-107.
                    let total = existing.count() + stack.count();
                    let max = self.max_stack_size(i, &existing);
                    if total <= max {
                        // Offsets 109-136 — it fits exactly or with room to
                        // spare, and the source is emptied outright.
                        stack.set_count(0);
                        let mut grown = existing;
                        grown.set_count(total);
                        self.set_in_place(i, grown);
                        moved = true;
                    } else if existing.count() < max {
                        // Offsets 139-174 — top the slot up and keep the rest.
                        // The `existing.getCount() < max` test is not
                        // redundant: without it a slot already at its maximum
                        // would `shrink` the source by a negative amount and
                        // *grow* it.
                        stack.shrink(max - existing.count());
                        let mut grown = existing;
                        grown.set_count(max);
                        self.set_in_place(i, grown);
                        moved = true;
                    }
                }
                // Offsets 176-190.
                let Some(next) = i.checked_add_signed(step) else {
                    break;
                };
                i = next;
            }
        }

        // Phase 2 — offsets 193-328.
        if !stack.is_empty() {
            let mut i = first;
            while i >= start && i < end {
                if self.get(i).is_empty() && self.may_place(i, stack) {
                    // Offsets 274-306. `Math.min` at offset 291 is what makes
                    // an over-sized stack leave a remainder behind rather
                    // than overfilling one slot.
                    let max = self.max_stack_size(i, stack);
                    let n = stack.count().min(max);
                    let put = stack.split(n);
                    self.set(i, put);
                    moved = true;
                    // Offset 308: `goto 328`. **One empty slot, then out.**
                    break;
                }
                // Offsets 311-325.
                let Some(next) = i.checked_add_signed(step) else {
                    break;
                };
                i = next;
            }
        }
        moved
    }

    /// `ChestMenu.quickMoveStack(Player, int)`, offsets 0-129.
    ///
    /// Returns what vanilla returns, which is **not** what was moved: offsets
    /// 37-42 copy the source stack *before* the move, so the return value is
    /// the stack as it was when the click arrived. [`do_click_quick_move`]
    /// compares that copy against what is left in the slot, so getting this
    /// wrong changes the loop's termination rather than any inventory.
    ///
    /// The direction test is offsets 43-51, `index < containerRows * 9`:
    ///
    /// * a **container** slot moves into `containerSlots..slots`, reversed —
    ///   offsets 54-72;
    /// * a **player** slot moves into `0..containerSlots`, forwards — offsets
    ///   82-94.
    ///
    /// `EMPTY` comes back on three paths, and they are three different facts
    /// that the caller cannot tell apart — which is exactly what makes the
    /// loop terminate: the slot was empty (offsets 22-27), or
    /// `moveItemStackTo` moved nothing (offsets 78-81 and 100-103).
    ///
    /// A menu whose [`QuickMove`] rule is not transcribed returns `EMPTY`
    /// here too, so its click is a no-op and its loop runs zero times.
    fn quick_move_stack(&mut self, index: usize) -> ItemStack {
        match self.menu.spec.quick_move {
            QuickMove::ChestMenu => self.chest_quick_move_stack(index),
            QuickMove::Furnace(kind) => self.furnace_quick_move_stack(index, kind),
            QuickMove::BrewingStand => self.brewing_quick_move_stack(index),
            QuickMove::Workstation => crate::workstation::quick_move_stack(self, index),
            QuickMove::Crafting => self.crafting_quick_move_stack(index),
            QuickMove::NotImplemented => ItemStack::empty(),
        }
    }

    /// `AbstractFurnaceMenu.quickMoveStack(Player, int)`, offsets 0-259.
    ///
    /// Six arms, tested in this order, each a `moveItemStackTo` whose failure
    /// returns `EMPTY` at once:
    ///
    /// | source | test | destination |
    /// |---|---|---|
    /// | 2, the result | — | `3..39`, **reversed** |
    /// | 0 or 1 | — | `3..39` |
    /// | player | `canSmelt` | `0..1` |
    /// | player | `isFuel` | `1..2` |
    /// | 3..30, main | — | `30..39`, the hotbar |
    /// | 30..39, hotbar | — | `3..30`, the main inventory |
    ///
    /// **Smeltable is asked before fuel**, so a log — both — goes to the
    /// ingredient slot. `canSmelt` is `acceptedInputs.test(stack)`, the
    /// menu's `RecipePropertySet` for its own cooking type, which is the set
    /// of items some recipe of that type accepts; for a single-ingredient
    /// recipe that is exactly "some recipe of that type matches the stack",
    /// which is the question `find_single` answers.
    ///
    /// Offsets 234-249 return `EMPTY` when the count did not move, which a
    /// chest's quick move does not do; `doClick`'s loop then stops. A
    /// `moveItemStackTo` that merged and placed nothing still returns
    /// `false`, so this is a second, independent guard and not a duplicate of
    /// the first.
    ///
    /// Not here: the result slot's `onQuickCraft` and `onTake`, which award
    /// the smelting experience (`FurnaceResultSlot.checkTakeAchievements` ->
    /// `awardUsedRecipesAndPopExperience`). The stack moves exactly as
    /// vanilla moves it; the experience orb does not appear.
    fn furnace_quick_move_stack(
        &mut self,
        index: usize,
        kind: crate::recipes::CookingKind,
    ) -> ItemStack {
        const RESULT: usize = 2;
        const INV_START: usize = 3;
        const HOTBAR_START: usize = 30;
        const END: usize = 39;
        let mut stack = self.get(index);
        if stack.is_empty() {
            return ItemStack::empty();
        }
        let copy = stack.clone();
        let moved = if index == RESULT {
            self.move_item_stack_to(&mut stack, INV_START, END, true)
        } else if index == 1 || index == 0 {
            self.move_item_stack_to(&mut stack, INV_START, END, false)
        } else if self
            .recipes
            .is_some_and(|r| r.find_single(kind.recipe_type(), &stack).is_some())
        {
            self.move_item_stack_to(&mut stack, 0, 1, false)
        } else if crate::fuel::FuelValues::loaded().is_some_and(|f| f.is_fuel(&stack)) {
            self.move_item_stack_to(&mut stack, 1, 2, false)
        } else if (INV_START..HOTBAR_START).contains(&index) {
            self.move_item_stack_to(&mut stack, HOTBAR_START, END, false)
        } else if (HOTBAR_START..END).contains(&index) {
            self.move_item_stack_to(&mut stack, INV_START, HOTBAR_START, false)
        } else {
            // Offsets 181-209 fall through to 210 for an index outside every
            // range — unreachable for a valid slot, and vanilla does not
            // return early, so neither does this.
            true
        };
        if !moved {
            return ItemStack::empty();
        }
        // Offsets 210-233: `setByPlayer(EMPTY)` or `setChanged()`, one write
        // here for the reason `chest_quick_move_stack` gives.
        let drained = stack.count();
        self.set(index, stack);
        // Offsets 234-249.
        if drained == copy.count() {
            return ItemStack::empty();
        }
        copy
    }

    /// `BrewingStandMenu.quickMoveStack(Player, int)`, offsets 0-311.
    ///
    /// | source | test | destination |
    /// |---|---|---|
    /// | 0-2, 3 or 4 | — | `5..41`, **reversed** |
    /// | player | `FuelSlot.mayPlaceItem` | `4..5`, then if that fails and the ingredient slot takes it, `3..4` |
    /// | player | ingredient slot's `mayPlace` | `3..4` |
    /// | player | `PotionSlot.mayPlaceItem` | `0..3` |
    /// | 5..32, main | — | `32..41`, the hotbar |
    /// | 32..41, hotbar | — | `5..32` |
    ///
    /// The fuel arm is the odd one (offsets 90-136). Blaze powder is both a
    /// fuel and an ingredient, so when the fuel slot is full the stack goes on
    /// to the ingredient slot; when *neither* takes it the method returns
    /// `EMPTY`. But a stack that went into the fuel slot skips the ingredient
    /// attempt entirely: the successful fuel move is `ifne 133`, and offset
    /// 133 is the `EMPTY` return — the same instruction a failed ingredient
    /// move reaches. **So a successful move into the fuel slot returns
    /// `EMPTY`**, without the count test or `onTake`, and `doClick`'s loop
    /// stops after one call; that is transcribed, not tidied.
    fn brewing_quick_move_stack(&mut self, index: usize) -> ItemStack {
        const INGREDIENT: usize = 3;
        const FUEL: usize = 4;
        const INV_START: usize = 5;
        const HOTBAR_START: usize = 32;
        const END: usize = 41;
        let mut stack = self.get(index);
        if stack.is_empty() {
            return ItemStack::empty();
        }
        let copy = stack.clone();
        let tables = self.slot_tables();
        let is_fuel = crate::menuslots::brewing_fuel_may_place(&copy, tables.tags);
        let is_ingredient = crate::menuslots::may_place(
            crate::menuslots::SlotClass::BrewingIngredient,
            &stack,
            &tables,
        );
        let is_potion = crate::menuslots::potion_slot_may_place(&copy);
        let moved = if index <= FUEL {
            // Offsets 43-87: `(index >= 0 && index <= 2) || index == 3 ||
            // index == 4`.
            self.move_item_stack_to(&mut stack, INV_START, END, true)
        } else if is_fuel {
            // Offsets 90-136. See the doc: both outcomes of a successful
            // fuel move and of a failed pair reach the `EMPTY` at 133.
            if self.move_item_stack_to(&mut stack, FUEL, FUEL + 1, false) {
                self.set(index, stack);
                return ItemStack::empty();
            }
            if !is_ingredient {
                // `ifeq 263`: the fuel move failed and the ingredient slot
                // refuses — fall to the write-back with nothing moved.
                return self.brewing_tail(index, stack, &copy);
            }
            if self.move_item_stack_to(&mut stack, INGREDIENT, INGREDIENT + 1, false) {
                return self.brewing_tail(index, stack, &copy);
            }
            return ItemStack::empty();
        } else if is_ingredient {
            self.move_item_stack_to(&mut stack, INGREDIENT, INGREDIENT + 1, false)
        } else if is_potion {
            self.move_item_stack_to(&mut stack, 0, 3, false)
        } else if (INV_START..HOTBAR_START).contains(&index) {
            self.move_item_stack_to(&mut stack, HOTBAR_START, END, false)
        } else if (HOTBAR_START..END).contains(&index) {
            self.move_item_stack_to(&mut stack, INV_START, HOTBAR_START, false)
        } else {
            self.move_item_stack_to(&mut stack, INV_START, END, false)
        };
        if !moved {
            return ItemStack::empty();
        }
        self.brewing_tail(index, stack, &copy)
    }

    /// `BrewingStandMenu.quickMoveStack` offsets 263-311: write the source
    /// back, `EMPTY` if the count did not move, else `onTake` and the copy.
    fn brewing_tail(&mut self, index: usize, stack: ItemStack, copy: &ItemStack) -> ItemStack {
        let left = stack.count();
        self.set(index, stack);
        if left == copy.count() {
            return ItemStack::empty();
        }
        copy.clone()
    }

    /// `ChestMenu.quickMoveStack(Player, int)` — the body documented on
    /// [`Self::quick_move_stack`].
    fn chest_quick_move_stack(&mut self, index: usize) -> ItemStack {
        let container_slots = self.menu.spec.container_slots;
        let total_slots = self.menu.spec.slots;
        // Offsets 22-35: `slot.hasItem()`, then `slot.getItem()`.
        let mut stack = self.get(index);
        if stack.is_empty() {
            return ItemStack::empty();
        }
        // Offsets 37-42.
        let result = stack.clone();
        let moved = if index < container_slots {
            self.move_item_stack_to(&mut stack, container_slots, total_slots, true)
        } else {
            self.move_item_stack_to(&mut stack, 0, container_slots, false)
        };
        if !moved {
            return ItemStack::empty();
        }
        // Offsets 104-127. Vanilla's two arms — `setByPlayer(EMPTY)` when the
        // stack drained and `setChanged()` when it did not — are one write
        // here, because vanilla's `stack` *is* the slot's object and ours is a
        // copy. The write-back is skipped on the `!moved` path above for the
        // same reason vanilla can skip it: `flag` is false only when no branch
        // touched the stack.
        self.set(index, stack);
        result
    }
}

/// `AbstractContainerMenu.doClick`, the `PICKUP`, `QUICK_MOVE`, `SWAP` and
/// `THROW` arms.
///
/// Returns whether a slot's `setChanged` ran, which is what drives
/// `slotsChanged` and therefore the crafting recipe.
///
/// # What is implemented and what is not
///
/// `PICKUP` in full, both buttons; `QUICK_MOVE` for the menus whose
/// `quickMoveStack` is transcribed — see [`QuickMove`] — and, since the guard
/// at 575 is shared, only for buttons 0 and 1; `SWAP` for a hotbar
/// button in range, except the one sub-path named in [`do_click_swap`]'s own
/// doc; and `THROW`, both buttons, which is the only arm with an effect
/// outside the menu. `QUICK_CRAFT` and `CLONE` are **not** implemented and
/// are declined rather than approximated, because a wrong move puts a stack
/// in a plausible-looking wrong slot and every later assertion is then
/// measuring the wrong inventory. `tools/oracle-runner`'s `container_click`
/// can send all seven, so a scenario naming one of the others will show it
/// immediately as a divergence rather than as a subtly wrong inventory.
///
/// **The arms are dispatched in the order the bytecode tests them.** From
/// `javap -p -c -constants` on `AbstractContainerMenu` in 26.2, the private
/// `doClick(int, int, ContainerInput, Player)`:
///
/// ```text
///     0- 11  QUICK_CRAFT           -> 547     not implemented
///   547-572  PICKUP or QUICK_MOVE  -> 1079    both, and QUICK_MOVE is
///                                             re-tested inside at 664
///  1080-1083 SWAP                  -> 1385    implemented below, see its doc
///  1386-1389 CLONE                 -> 1449    not implemented
///  1449-1608 THROW                 -> 1611    implemented below
///  1612-1615 PICKUP_ALL            -> 1873    implemented below
/// ```
///
/// The doc block this replaced said "`PICKUP` first (offset 0), `QUICK_MOVE`
/// at 663", which is two offsets from the *inside* of the shared arm read as
/// if they were the dispatch. The behaviour was right — the arms are mutually
/// exclusive on one enum value, so their order cannot matter — and the map
/// was wrong, which is the kind of comment that stops the next reader
/// looking. Corrected against the bytecode while adding `THROW`.
///
/// `dropped` collects the stacks `Player.drop` was called with, in call
/// order. It is an out-parameter rather than a return value because
/// `THROW`'s loop can call `drop` more than once and because this module
/// stays pure: an item entity needs a `Level`, an id allocator and a tracker,
/// none of which belong here. See [`Outcome::dropped`].
fn do_click(
    slots: &mut Slots<'_>,
    slot_num: i32,
    button: i8,
    input: ContainerInput,
    dropped: &mut Vec<ItemStack>,
) -> bool {
    // **Offsets 561-581: the guard belongs to the shared arm, not to
    // `PICKUP` alone.** 561-572 enter ONE arm for either `PICKUP` or
    // `QUICK_MOVE` (`if_acmpeq 575` on the first, `if_acmpne 1079` on the
    // second), and 575-581 — `if (button != 0 && button != 1) return` — sit
    // *inside* it, ahead of the re-test at 664 that separates the two. So a
    // `QUICK_MOVE` whose button is neither 0 nor 1 returns without touching a
    // slot, exactly as a `PICKUP` with that button does.
    //
    // This server had the guard **below** the `QUICK_MOVE` dispatch, where it
    // could only ever run for `PICKUP` — and `do_click_quick_move` does not
    // take `button` at all, so there was nothing further down to catch it
    // either. Every out-of-range button shift-clicked a stack that vanilla
    // leaves alone.
    //
    // **No scenario could have caught this**, and that is the point of the
    // arrangement rather than an excuse for it: `ci/click-surface.py` reports
    // the corpus asking 6 of the 28 `(input, button)` classes `doClick` acts
    // on, and every `quick_move` step in `ci/checks/gameplay` sends button 0.
    // A closed loop — the corpus asks what the recorder sends, the recorder
    // sends what this file names — cannot produce the one value that differs.
    if matches!(input, ContainerInput::Pickup | ContainerInput::QuickMove)
        && button != 0
        && button != 1
    {
        return false;
    }
    // **NAMED GAP: `tryItemClickBehaviourOverride`, and it is exactly
    // seventeen items wide.** `doClick` asks the slot's stack
    // `overrideStackedOnOther(slot, action, player)` and then the carried
    // stack `overrideOtherStackedOnMe(other, slot, action, player,
    // createCarriedSlotAccess())`, and returns before the ordinary PICKUP
    // arithmetic if either says yes (offsets 19-31 and 42-60 of that helper).
    // Nothing in this file calls either.
    //
    // `Item.overrideStackedOnOther` and `Item.overrideOtherStackedOnMe` are
    // both `iconst_0; ireturn`, so the omission is a no-op for every item
    // that does not override them — and the class that does is `BundleItem`,
    // **17 of the 1,537 items in the registry** (class column of
    // `data/vanilla/item_props.json`). Clicking a stack onto a bundle, or a
    // bundle onto a stack, is the whole of what is missing.
    //
    // It is not ported because a bundle is its `minecraft:bundle_contents`
    // component — a list of stacks inside a stack — and no stack on this
    // server carries a component patch at all. The gap is written here rather
    // than left to be discovered because an absent override is
    // indistinguishable from an override that returned false, which is the
    // shape of defect this project keeps paying for.
    if input == ContainerInput::QuickMove {
        return do_click_quick_move(slots, slot_num);
    }
    if input == ContainerInput::Swap {
        return do_click_swap(slots, slot_num, button);
    }
    if input == ContainerInput::Throw {
        return do_click_throw(slots, slot_num, button, dropped);
    }
    if input == ContainerInput::PickupAll {
        return do_click_pickup_all(slots, slot_num, button);
    }
    if input != ContainerInput::Pickup {
        return false;
    }
    // 584-597. The guard itself is above, with `QUICK_MOVE`, because the
    // bytecode applies it to both.
    let action = if button == 0 {
        ClickAction::Primary
    } else {
        ClickAction::Secondary
    };

    if slot_num == SLOT_CLICKED_OUTSIDE {
        // Dropping into the world. `Player.drop` needs an item entity, which
        // belongs with `crate::dropitem`; what is faithful here is that the
        // cursor loses the stack either way.
        if slots.menu.carried.is_empty() {
            return false;
        }
        match action {
            ClickAction::Primary => slots.menu.carried = ItemStack::empty(),
            ClickAction::Secondary => {
                let _dropped = slots.menu.carried.split(1);
            }
        }
        return false;
    }

    let Ok(i) = usize::try_from(slot_num) else {
        // `if (slotNum < 0) return` at offset 744 — a negative that is not
        // `SLOT_CLICKED_OUTSIDE` falls out here having done nothing.
        return false;
    };

    let slot_stack = slots.get(i);
    let carried = slots.menu.carried.clone();

    if slot_stack.is_empty() {
        if !carried.is_empty() {
            let n = match action {
                ClickAction::Primary => carried.count(),
                ClickAction::Secondary => 1,
            };
            slots.menu.carried = slots.safe_insert(i, carried, n);
        }
    } else if slots.may_pickup(i) {
        if carried.is_empty() {
            let n = match action {
                ClickAction::Primary => slot_stack.count(),
                // `(count + 1) / 2` — a secondary click takes the larger half.
                ClickAction::Secondary => (slot_stack.count() + 1) / 2,
            };
            if let Some(taken) = slots.try_remove(i, n, i32::MAX) {
                // `lambda$doClick$0`: `setCarried(stack)`, then
                // `slot.onTake(player, stack)` — the cursor first.
                slots.menu.carried = taken.clone();
                slots.on_take(i, &taken);
            }
        } else if slots.may_place(i, &carried) {
            if ItemStack::is_same_item_same_components(&slot_stack, &carried) {
                let n = match action {
                    ClickAction::Primary => carried.count(),
                    ClickAction::Secondary => 1,
                };
                slots.menu.carried = slots.safe_insert(i, carried, n);
            } else if carried.count() <= slots.max_stack_size(i, &carried) {
                // The swap. Note the order: the slot's old stack goes to the
                // cursor *before* the cursor's goes to the slot, because both
                // reads happened at the top.
                slots.menu.carried = slot_stack;
                slots.set(i, carried);
            }
        } else if ItemStack::is_same_item_same_components(&slot_stack, &carried) {
            // The slot refuses a placement but holds the same item — taking
            // from a crafting result onto a cursor already holding some of it.
            let room = carried.max_stack_size() - carried.count();
            if let Some(taken) = slots.try_remove(i, slot_stack.count(), room) {
                // `lambda$doClick$1`: `carried.grow(count)`, then
                // `slot.onTake(player, taken)`.
                let mut grown = carried;
                grown.grow(taken.count());
                slots.menu.carried = grown;
                slots.on_take(i, &taken);
            }
        }
    }
    // `slot.setChanged()` at offset 1071 — unconditional on this path. It is
    // reached, and in 26.2 it does **nothing**: both crafting containers'
    // `setChanged` bodies are a bare `return`. The recipe runs on the
    // storage writes instead; see [`Slots::set`].
    true
}

/// `doClick`'s `QUICK_MOVE` arm, offsets 664-741 — **and 708-738 is a loop**.
///
/// ```text
///   664-667  if (input != QUICK_MOVE) goto 744
///   670-674  if (slotIndex < 0) return
///   675-686  slot = slots.get(slotIndex)
///   688-698  if (!slot.mayPickup(player)) return
///   699-706  moved = quickMoveStack(player, slotIndex)
///   708-713  if (moved.isEmpty()) goto 741                  <- exit 1
///   716-726  if (!isSameItem(slot.getItem(), moved)) goto 741 <- exit 2
///   729-736  moved = quickMoveStack(player, slotIndex)
///   738      goto 708
///   741      goto 1076 — past the `slot.setChanged()` at 1071
/// ```
///
/// # Why the loop is the whole item
///
/// [`Slots::move_item_stack_to`]'s placement phase fills **one** empty slot
/// and returns (offset 308). So one `quickMoveStack` call moves at most one
/// destination slot's worth beyond whatever it could merge, and a stack larger
/// than a destination slot has to be handed over across several calls. A
/// one-shot implementation — call `quickMoveStack` once and stop — is the
/// difference between *"shift-click moves an item"* and *"shift-click empties
/// the slot"*, and it passes corpus scenarios 601 and 603 because both move a
/// stack of exactly one.
///
/// # The two exits, and why neither is "until the slot is empty"
///
/// Exit 1 fires when `quickMoveStack` returns `EMPTY`, which it does when the
/// slot is empty *or* when nothing could be moved — a full destination, or a
/// menu with no rule. Exit 2 fires when what is left in the slot is no longer
/// `isSameItem` with what the last call reported, which covers the ordinary
/// case of the slot draining to nothing (`Items.AIR` is not the same item as
/// stone) and also the case of a slot whose remainder is a different item
/// entirely, which no menu here can produce but which the bytecode guards
/// against.
///
/// **Termination.** Every iteration past exit 1 moved something, and both of
/// `move_item_stack_to`'s write branches strictly reduce the source stack's
/// count — phase 1 by `max - existing.count() >= 1` or to zero outright, phase
/// 2 by `min(count, max) >= 1` given `max >= 1`. `max` is
/// [`crate::item::DEFAULT_MAX_STACK_SIZE`] `.min(stack.max_stack_size())`,
/// which is 64, so the count falls by at least one per iteration and the loop
/// runs at most `count` times. The day `max_stack_size` can return zero, this
/// argument fails and the loop does not stop — which is why the bound is
/// stated here rather than left to be rediscovered.
///
/// Returns `false` always: offset 741 jumps to 1076, **past** the
/// `slot.setChanged()` at 1071 that the `PICKUP` arm ends on. `setChanged` is
/// still called, but by `moveItemStackTo` on each destination it wrote — and
/// what drives a crafting recompute is the storage write itself, in
/// [`Slots::set`], not this flag.
fn do_click_quick_move(slots: &mut Slots<'_>, slot_num: i32) -> bool {
    // 670-674. `SLOT_CLICKED_OUTSIDE` is -999 and falls out here, so a
    // shift-click into the world does nothing — not even a drop.
    let Ok(i) = usize::try_from(slot_num) else {
        return false;
    };
    // 675-686. `handle_click`'s `isValidSlotIndex` has already run; a direct
    // caller has not, and `slots.get` on a missing slot answers empty rather
    // than panicking, so this only guards `may_pickup`'s catch-all.
    if i >= slots.menu.spec.slots {
        return false;
    }
    // 688-698.
    if !slots.may_pickup(i) {
        return false;
    }
    // 699-706.
    let mut moved = slots.quick_move_stack(i);
    // 708-738.
    while !moved.is_empty() && ItemStack::is_same_item(&slots.get(i), &moved) {
        moved = slots.quick_move_stack(i);
    }
    false
}

/// `doClick`'s `SWAP` arm, offsets 1080-1382 — a hotbar number key pressed
/// over a slot.
///
/// ```text
///   1086-1099  bounds-check button: 0..9 (hotbar) or ==40 (offhand)  -> 1385 (decline)
///   1102-1108  hotbarStack = inventory.getItem(button)
///   1110-1128  slot = menu.slots.get(slotNum); slotStack = slot.getItem()
///   1130-1146  both empty                                            -> 1382 (no-op)
///   1149-1202  hotbarStack empty, slotStack not: pickup into hotbar
///   1205-1273  slotStack empty, hotbarStack not: place into slot,
///              splitting at the slot's max if hotbarStack overflows it
///   1276-1382  both non-empty: the exchange, splitting the same way
/// ```
///
/// Both reads (`hotbarStack`, `slotStack`) happen before either write, same
/// as the `PICKUP` swap arm above. `slot` here is `menu.slots.get(slotNum)`
/// — [`Slots::get`]/[`Slots::set`] at `slotNum`, exactly what `PICKUP` reads
/// and writes — but `hotbarStack` is read and written straight off
/// `Player.getInventory()`, bypassing `Slot` entirely. [`Slots::inventory`]
/// is `Inventory`'s own 36-slot array in `Inventory`'s own index order (see
/// [`crate::player::Hotbar`]'s doc), so `button` indexes it directly with no
/// translation through [`slot_ref`].
///
/// # The offhand arm is declined, not approximated
///
/// `button == 40` is `SLOT_OFFHAND`, outside `Inventory`'s 36 and reachable
/// in vanilla through `EQUIPMENT_SLOT_MAPPING` onto an `EntityEquipment` this
/// server does not model — the same gap
/// [`crate::player::Hotbar::get_slot_with_remaining_space`]'s doc names and
/// the gameplay corpus cannot reach today (`hold` cannot put anything in an
/// offhand). Declined here for the same reason the whole enum's other four
/// values are: a wrong slot is worse than no move.
///
/// # The oversized two-way exchange is declined too
///
/// Offsets 1315-1355 — both slots hold a stack, and the hotbar stack does
/// not fit whole into the container slot's max — call `Inventory.add`
/// (spread the container's *old* stack back over the rest of the inventory)
/// and, failing that, `Player.drop`. That is a new mechanism this arm has no
/// scenario driving: nothing in the gameplay corpus swaps a stack that
/// overflows the target slot, and no `container-click-swap` check exercises
/// it. Declined for the same reason as the offhand arm rather than wired up
/// unmeasured. `container-click-swap-exchanges-a-hotbar-slot` clicks two
/// single-item stacks, which takes the plain exchange at 1358-1382.
fn do_click_swap(slots: &mut Slots<'_>, slot_num: i32, button: i8) -> bool {
    // 1086-1099.
    let Ok(button) = usize::try_from(button) else {
        return false;
    };
    if button >= HOTBAR_SIZE {
        return false;
    }
    let Ok(i) = usize::try_from(slot_num) else {
        return false;
    };
    if i >= slots.menu.spec.slots {
        return false;
    }

    // 1102-1128. Both reads before either write.
    let hotbar_stack = slots.inventory.stack(button);
    let slot_stack = slots.get(i);

    if hotbar_stack.is_empty() && slot_stack.is_empty() {
        // 1130-1146.
        return false;
    }

    if hotbar_stack.is_empty() {
        // 1149-1202: pickup into the hotbar.
        if !slots.may_pickup(i) {
            return false;
        }
        // 1172-1199: `inventory.setItem`, `onSwapCraft` (statistics only),
        // `slot.setByPlayer(EMPTY)`, then `slot.onTake(player, slotStack)` —
        // which on a crafting result spends the grid.
        slots.inventory.set_stack(button, slot_stack.clone());
        slots.set(i, ItemStack::empty());
        slots.on_take(i, &slot_stack);
        return true;
    }

    if slot_stack.is_empty() {
        // 1205-1273: place into the slot.
        if !slots.may_place(i, &hotbar_stack) {
            return false;
        }
        let max = slots.max_stack_size(i, &hotbar_stack);
        let mut hotbar_stack = hotbar_stack;
        if hotbar_stack.count() > max {
            let put = hotbar_stack.split(max);
            slots.inventory.set_stack(button, hotbar_stack);
            slots.set(i, put);
        } else {
            slots.inventory.set_stack(button, ItemStack::empty());
            slots.set(i, hotbar_stack);
        }
        return true;
    }

    // 1276-1382: both non-empty. The exchange `604` measures.
    if !slots.may_pickup(i) || !slots.may_place(i, &hotbar_stack) {
        return false;
    }
    let max = slots.max_stack_size(i, &hotbar_stack);
    if hotbar_stack.count() > max {
        // 1305-1355 — declined; see the doc above.
        return false;
    }
    // 1358-1382.
    // 1363-1379: the exchange ends in `slot.onTake(player, slotStack)` too.
    slots.inventory.set_stack(button, slot_stack.clone());
    slots.set(i, hotbar_stack);
    slots.on_take(i, &slot_stack);
    true
}

/// `doClick`'s `THROW` arm, offsets 1449-1608 — pressing Q over a slot.
///
/// ```text
///  1449-1453  if (input != THROW) goto 1611
///  1456-1463  if (!getCarried().isEmpty()) goto 1611     <- the cursor guard
///  1466-1467  if (slotNum < 0) goto 1611
///  1470-1481  slot = slots.get(slotNum)
///  1483-1499  n = (button == 0) ? 1 : slot.getItem().getCount()
///  1501-1509  if (!player.canDropItems()) return
///  1510-1522  taken = slot.safeTake(n, Integer.MAX_VALUE, player)
///  1524-1532  player.drop(taken, true)
///  1533-1539  player.handleCreativeModeItemDrop(taken)
///  1540-1542  if (button != 1) goto 1608
///  1545-1550  LOOP: if (taken.isEmpty()) goto 1608
///  1553-1563  if (!isSameItem(slot.getItem(), taken)) goto 1608
///  1566-1574  if (!player.canDropItems()) return
///  1575-1587  taken = slot.safeTake(n, Integer.MAX_VALUE, player)
///  1589-1597  player.drop(taken, true)
///  1598-1604  player.handleCreativeModeItemDrop(taken)
///  1605       goto 1545
///  1608       goto 1873 — past the `slot.setChanged()` at 1071
/// ```
///
/// # `n` is computed once, and the loop re-uses it
///
/// Offsets 1491-1499 read `slot.getItem().getCount()` **before** the first
/// `safeTake`, and the loop at 1575 passes that same local. So a secondary
/// throw of a stack of two is one `safeTake(2, MAX)` — one call, one item
/// entity of two — and the loop exits immediately because the slot is now
/// empty and `Items.AIR` is not `isSameItem` with stone. It is a loop for the
/// case `safeTake` cannot satisfy in one go, which needs a slot whose
/// `getMaxStackSize` is below its own count; the transcription keeps it
/// because that is what the bytecode does and because `605`/`606` cannot tell
/// a loop from a one-shot.
///
/// This is exactly the branch `605-container-click-throw-drops-one-item` and
/// `606-container-click-throw-drops-the-whole-stack` exist to pin: `605`
/// sends `button = 0` and must leave one stone behind, `606` sends
/// `button = 1` and must empty the slot, and a server that hardcoded either
/// answer would pass one and fail the other.
///
/// # The three guards that are silence, not refusal
///
/// A non-empty cursor falls through to `PICKUP_ALL` — vanilla does **not**
/// throw the carried stack, it ignores the click — and so does a negative
/// slot. `SLOT_CLICKED_OUTSIDE` is -999 and therefore lands in that second
/// one: Q over the void is not a throw. All three end in the same `goto`,
/// which is why they read the same on the wire and are written out here.
///
/// # `canDropItems` and `handleCreativeModeItemDrop`
///
/// `Player.canDropItems()` is `iconst_1; ireturn` and `ServerPlayer` does not
/// override it, so the two `return`s at 1509 and 1574 are unreachable on this
/// server and the test is omitted rather than transcribed as `if (true)`.
/// `Player.handleCreativeModeItemDrop` is a bare `return`; `ServerPlayer`
/// does not override that either. Both are named because their absence would
/// otherwise read as a missed line of the arm.
///
/// # Termination
///
/// Stated for the same reason [`do_click_quick_move`]'s is. Every iteration
/// past the `isEmpty` test took a non-empty stack, and [`Slots::safe_take`]
/// with `max_take = i32::MAX` removes `min(n, slot count) >= 1` — `n` is
/// `slot.getItem().getCount()` on the `button == 1` path and therefore at
/// least 1 whenever the slot is non-empty. So the slot's count strictly falls
/// and the loop runs at most that many times. A slot that answered a take
/// without shrinking would not stop, which is a property of [`Slots::set`]
/// and not of this method.
///
/// Returns `false`: offset 1608 jumps past the `slot.setChanged()` at 1071,
/// exactly as the `QUICK_MOVE` arm does.
fn do_click_throw(
    slots: &mut Slots<'_>,
    slot_num: i32,
    button: i8,
    dropped: &mut Vec<ItemStack>,
) -> bool {
    // 1456-1463.
    if !slots.menu.carried.is_empty() {
        return false;
    }
    // 1466-1467. `SLOT_CLICKED_OUTSIDE` is negative and falls out here.
    let Ok(i) = usize::try_from(slot_num) else {
        return false;
    };
    // 1470-1481. `handle_click`'s `isValidSlotIndex` has already run for a
    // click off the wire; a direct caller has not.
    if i >= slots.menu.spec.slots {
        return false;
    }
    // 1483-1499. Read once, before anything is taken.
    let n = if button == 0 { 1 } else { slots.get(i).count() };
    // 1510-1532. `LivingEntity.drop` returns null for an empty stack before
    // it constructs anything, so an empty take is a call that spawns nothing
    // — the click on an empty slot, and the loop's own exhaustion.
    let mut taken = slots.safe_take(i, n, i32::MAX);
    if !taken.is_empty() {
        dropped.push(taken.clone());
    }
    // 1540-1542.
    if button != 1 {
        return false;
    }
    // 1545-1605.
    while !taken.is_empty() && ItemStack::is_same_item(&slots.get(i), &taken) {
        taken = slots.safe_take(i, n, i32::MAX);
        if !taken.is_empty() {
            dropped.push(taken.clone());
        }
    }
    false
}

/// `AbstractContainerMenu.canItemQuickReplace(Slot, ItemStack, boolean)`,
/// offsets 0-66, specialised to its one caller here.
///
/// The published shape is a three-argument static, but `PICKUP_ALL` is the
/// only caller in this server and it passes `stackSizeMatters = true`, which
/// deletes a term: offsets 39-51 add `stackSizeMatters ? 0 : stack.getCount()`
/// to the slot's count before the comparison, so with `true` the sum is the
/// slot's count alone. Writing the general form and then calling it one way
/// would be a branch no test could reach.
///
/// The `slot == null || !slot.hasItem()` arm at offsets 0-16 returns **true**
/// — an empty slot is always quick-replaceable — and it is unreachable from
/// here, because the sweep tests `hasItem` before it asks. That is why this
/// takes the slot's stack rather than the slot: the caller has already proved
/// it non-empty.
fn can_item_quick_replace(slot_item: &ItemStack, carried: &ItemStack) -> bool {
    // 21-29 then 32-56, with the `stackSizeMatters` term collapsed to 0.
    ItemStack::is_same_item_same_components(carried, slot_item)
        && slot_item.count() <= carried.max_stack_size()
}

/// `canTakeItemForPickAll(ItemStack, Slot)` — whether a double-click may
/// sweep menu slot `i` onto the cursor.
///
/// `AbstractContainerMenu`'s body is `iconst_1; ireturn`, and five menus in
/// the pinned jar override it to refuse their result slot:
/// `CraftingMenu` and `InventoryMenu` (`slot.container != resultSlots`),
/// `SmithingMenu` (`resultSlots`), `StonecutterMenu` and
/// `CartographyTableMenu` (`resultContainer`) — each `if_acmpeq` to
/// `iconst_0` and otherwise the super call. `MerchantMenu` returns
/// `iconst_0` outright and this server opens none.
///
/// **The anvil, the grindstone and the loom do not override it**:
/// `ItemCombinerMenu` has no `canTakeItemForPickAll` and neither do
/// `AnvilMenu`, `GrindstoneMenu` or `LoomMenu`. A double-click with a
/// matching stack on the cursor sweeps their result like any other slot,
/// through `safeTake`, and so through the result's `onTake`. Read off every
/// `*Menu.class` in `net/minecraft/world/inventory/` with `javap -c`, not
/// inferred from which menus have a result slot.
///
/// This said "no menu this server opens overrides it" until the crafting
/// table, which it has been able to open since before this function existed,
/// was checked: a gather onto a cursor of planks took the table's planks.
fn can_take_item_for_pick_all(slots: &Slots<'_>, i: usize) -> bool {
    use crate::workstation::Station;
    if slots.menu.spec.kind == MenuKind::Crafting {
        return !matches!(slot_ref(slots.menu.spec, i), Some(SlotRef::Result));
    }
    match slots.menu.spec.station {
        Some(st @ (Station::Smithing | Station::Stonecutter | Station::Cartography)) => {
            !st.is_result(i)
        }
        _ => true,
    }
}

/// `doClick`'s `PICKUP_ALL` arm, offsets 1611-1873 — the second half of a
/// double-click, which gathers every matching stack in the menu onto the
/// cursor.
///
/// # The entry condition is three tests and the middle one is the point
///
/// ```text
///   1618-1619  slotNum < 0                     -> return
///   1641-1646  carried.isEmpty()               -> return
///   1649-1654  !slot.hasItem()                 -> STRAIGHT TO THE SWEEP
///   1657-1664  slot.hasItem() && mayPickup     -> return
/// ```
///
/// So the sweep runs on a clicked slot that is **empty**, which is exactly
/// what a real double-click produces: the client sends `PICKUP` first,
/// lifting that slot's stack onto the cursor, and then `PICKUP_ALL` against
/// the slot it has just emptied. A scenario that clicks a slot still holding
/// a matching stack takes the 1664 exit and measures nothing — which is how
/// `607` spent its whole life unable to fail.
///
/// # Two passes, and the first one will not break a full stack
///
/// ```text
///   1667-1684  start = button == 0 ? 0 : size - 1
///   1686-1695  step  = button == 0 ? 1 : -1
///   1697-1703  for pass in 0..2
///   1727-1737  carried at its maximum          -> end this pass
///   1801-1819  pass == 0 && stack is full      -> skip this slot
///   1822-1854  safeTake(count, max - carried.count) then carried.grow(taken)
/// ```
///
/// The first pass draining partial stacks before the second one touches full
/// ones is not an optimisation: it decides *which* stacks a gather empties
/// when the cursor cannot hold all of them, and therefore what the menu looks
/// like afterwards.
///
/// **`getMaxStackSize` here is the ItemStack's own, not the slot's** (offsets
/// 1734, 1813 and 1831 all read it off a stack), so [`Slots::max_stack_size`]
/// — which folds in the container's limit — is deliberately not used.
fn do_click_pickup_all(slots: &mut Slots<'_>, slot_num: i32, button: i8) -> bool {
    // 1618-1619. `SLOT_CLICKED_OUTSIDE` is -999 and falls out here with it.
    let Ok(i) = usize::try_from(slot_num) else {
        return false;
    };
    // 1635-1646.
    if slots.menu.carried.is_empty() {
        return false;
    }
    let size = slots.menu.spec.slots;
    // 1649-1664. `slots.get` answers empty for a slot that does not exist, so
    // the bounds test is what keeps a click past the end of the menu out of
    // the sweep rather than into it through a false `hasItem`.
    if i < size && !slots.get(i).is_empty() && slots.may_pickup(i) {
        return false;
    }
    // 1667-1695.
    let (start, step): (i64, i64) = if button == 0 {
        (0, 1)
    } else {
        (i64::try_from(size).unwrap_or(0) - 1, -1)
    };
    // 1697-1703, 1867-1870.
    for pass in 0..2 {
        let mut j = start;
        // 1710-1724.
        while j >= 0 && usize::try_from(j).is_ok_and(|k| k < size) {
            // 1727-1737. A full cursor ends the pass rather than the sweep;
            // the outer loop then re-enters and falls straight back out here,
            // which is what the bytecode does too.
            let carried_count = slots.menu.carried.count();
            let carried_max = slots.menu.carried.max_stack_size();
            if carried_count >= carried_max {
                break;
            }
            #[allow(clippy::cast_sign_loss)]
            let k = j as usize;
            // 1740-1791.
            let item = slots.get(k);
            if !item.is_empty()
                && can_item_quick_replace(&item, &slots.menu.carried)
                && slots.may_pickup(k)
                && can_take_item_for_pick_all(slots, k)
                // 1801-1819.
                && !(pass == 0 && item.count() == item.max_stack_size())
            {
                // 1822-1854.
                let taken = slots.safe_take(k, item.count(), carried_max - carried_count);
                slots.menu.carried.grow(taken.count());
            }
            // 1857-1864.
            j += step;
        }
    }
    // The arm ends at 1873 with `return`, before the `slot.setChanged()` that
    // the `PICKUP` arm's tail runs — so a gather does not recompute a
    // crafting grid. Same answer as `THROW` and `QUICK_MOVE`, for the same
    // reason. See `Outcome::slots_changed`.
    false
}

/// A decoded `ServerboundContainerClickPacket`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClickPacket {
    pub container_id: i32,
    pub state_id: i32,
    pub slot_num: i32,
    pub button: i8,
    pub input: ContainerInput,
    /// Slots whose claimed contents this server could resolve. Always empty
    /// today — see [`Self::hashed_slots`].
    pub changed_slots: Vec<(usize, ItemStack)>,
    /// Slots the client made a claim about that this server cannot check,
    /// because the claim is a `HashedStack`. See [`Menu::belief_known`].
    pub hashed_slots: Vec<usize>,
    /// The client's claimed cursor. A `HashedStack`, so only its
    /// *emptiness* is readable here — which is all the corpus ever sends and
    /// is the only part `synchronizeCarriedToRemote` can be compared against
    /// without vanilla's hashing.
    pub carried: ItemStack,
}

/// Decode `container_click`.
///
/// Wire form, from `STREAM_CODEC`'s `composite` in argument order. Two fields
/// are easy to get wrong and both were read rather than assumed: `containerId`
/// is a **varint** (`ByteBufCodecs.CONTAINER_ID` -> `writeContainerId` ->
/// `VarInt.write`, not a byte), and `slotNum` is a **two-byte** big-endian
/// short — `ByteBufCodecs.SHORT` writes `writeShort` while `INT`, four lines
/// away in the same static initialiser, writes `writeInt`. Reading the wrong
/// one puts four bytes where two belong and desyncs the rest of the packet.
///
/// # Errors
/// If the packet is truncated.
pub fn decode_click(body: &[u8]) -> std::io::Result<ClickPacket> {
    use rustmc_proto::varint::read_varint;
    use rustmc_proto::wire;
    let mut cur = body;
    let container_id = read_varint(&mut cur)?;
    let state_id = read_varint(&mut cur)?;
    #[expect(clippy::cast_possible_wrap, reason = "writeShort is signed")]
    let slot_num = i32::from(wire::read_u16(&mut cur)? as i16);
    #[expect(clippy::cast_possible_wrap, reason = "writeByte is signed")]
    let button = wire::read_u8(&mut cur)? as i8;
    let input = ContainerInput::from_id(read_varint(&mut cur)?);
    let n = read_varint(&mut cur)?;
    let mut hashed_slots = Vec::new();
    for _ in 0..n.max(0) {
        #[expect(clippy::cast_possible_wrap, reason = "the key is a short")]
        let key = wire::read_u16(&mut cur)? as i16;
        // The value is a `HashedStack`, which this server cannot resolve into
        // a stack. Its bytes are consumed so the rest of the packet stays in
        // step, and the slot is recorded as unresolvable rather than dropped
        // — dropping it would silently claim the client said nothing.
        skip_hashed_stack(&mut cur)?;
        if let Ok(k) = usize::try_from(key) {
            hashed_slots.push(k);
        }
    }
    // The carried claim. Only its presence is readable, so an absent one is
    // an empty cursor and a present one is "something, and this server cannot
    // say what" — recorded as a non-empty stack of a name no registry has, so
    // it compares unequal to every real stack and forces the correction.
    let carried = if read_optional_flag(&mut cur)? {
        skip_hashed_stack_body(&mut cur)?;
        ItemStack::new(UNRESOLVED_ITEM, 1)
    } else {
        ItemStack::empty()
    };
    Ok(ClickPacket {
        container_id,
        state_id,
        slot_num,
        button,
        input,
        changed_slots: Vec::new(),
        hashed_slots,
        carried,
    })
}

/// The name given to a client claim this server cannot resolve.
///
/// No registry contains it, so it is never equal to a real stack and never
/// reaches the wire: [`crate::menu::to_wire`] looks it up, fails, and emits an
/// empty slot. Its only job is to compare unequal so the correction is sent.
const UNRESOLVED_ITEM: &str = "rustmc:unresolved_client_claim";

/// `ByteBufCodecs.optional`'s leading flag.
fn read_optional_flag(cur: &mut &[u8]) -> std::io::Result<bool> {
    Ok(rustmc_proto::wire::read_u8(cur)? != 0)
}

/// A whole `HashedStack`, flag included.
fn skip_hashed_stack(cur: &mut &[u8]) -> std::io::Result<()> {
    if read_optional_flag(cur)? {
        skip_hashed_stack_body(cur)?;
    }
    Ok(())
}

/// A present `HashedStack`'s body: item, count, then a `HashedPatchMap` —
/// two maps of component id to a 32-bit hash, then a set of removed ids.
fn skip_hashed_stack_body(cur: &mut &[u8]) -> std::io::Result<()> {
    use rustmc_proto::varint::read_varint;
    let _item = read_varint(cur)?;
    let _count = read_varint(cur)?;
    // `HashedPatchMap`: a map of added components to their hashes, then a set
    // of removed component ids.
    let added = read_varint(cur)?;
    for _ in 0..added.max(0) {
        let _id = read_varint(cur)?;
        let _hash = rustmc_proto::wire::read_i32(cur)?;
    }
    let removed = read_varint(cur)?;
    for _ in 0..removed.max(0) {
        let _id = read_varint(cur)?;
    }
    Ok(())
}

/// The result of handling one `container_click`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Outcome {
    /// The packets to write, in order.
    pub replies: Vec<Reply>,
    /// Whether a slot changed and the menu should recompute anything hung off
    /// `slotsChanged` — the crafting recipe.
    pub slots_changed: bool,
    /// The stacks `Player.drop(stack, true)` was called with, in call order.
    ///
    /// Only `THROW` fills this today. It is a value rather than a side effect
    /// for the reason the module header gives for `replies`: an item entity
    /// needs a `Level`, an entity-id allocator and a tracker, and a function
    /// that reached for those could only be tested by standing up a server.
    /// As a list it is asserted against `doClick`'s branch directly — one
    /// stack of one for `button = 0`, one stack of the slot's whole count for
    /// `button = 1`.
    ///
    /// **Empty stacks are not recorded.** `LivingEntity.drop` returns null
    /// before it constructs anything for an empty stack, so a `THROW` on an
    /// empty slot — and the loop iteration that finally empties one — calls
    /// `drop` and spawns nothing. Recording them would make the caller emit
    /// an `add_entity` vanilla never sends.
    ///
    /// The caller spawns one item entity per element **before** it writes any
    /// of [`Self::replies`], because that is the order vanilla puts them on
    /// the wire: `Player.drop` reaches `Level.addFreshEntity` inside
    /// `clicked`, and `ChunkMap.addEntity` broadcasts without a queue, while
    /// `broadcastChanges` runs afterwards. Vanilla's own recording of `605`
    /// shows `add_entity` then `container_set_slot`, in that order, in the
    /// same millisecond.
    pub dropped: Vec<ItemStack>,
}

/// `handleContainerClick`, whole.
///
/// `state_id` is the client's claimed state id; `changed_slots` and `carried`
/// are its prediction. See the module header for why a *correct* prediction
/// is answered with silence.
///
/// # Panics
/// Never. Every index is bounds-checked before use.
#[must_use]
pub fn handle_click(slots: &mut Slots<'_>, click: &ClickPacket) -> Outcome {
    let ClickPacket {
        container_id,
        state_id,
        slot_num,
        button,
        input,
        changed_slots,
        hashed_slots,
        carried,
    } = click;
    let (container_id, state_id, slot_num, button, input) =
        (*container_id, *state_id, *slot_num, *button, *input);
    let mut out = Outcome::default();
    // 1. A click naming a menu this player does not have open is dropped in
    //    silence. Not an error, not a resync — vanilla returns.
    if container_id != slots.menu.container_id {
        return out;
    }
    // 4. An out-of-range slot is dropped in silence too. `isValidSlotIndex`
    //    accepts `SLOT_CLICKED_OUTSIDE`, which is why it is not just a range
    //    test — reading it as one makes dropping an item impossible.
    if !slots.menu.is_valid_slot_index(slot_num) {
        return out;
    }
    // 5. Computed *before* the click, because `clicked` will move the state
    //    id on and comparing afterwards would never mismatch.
    let mismatch = state_id != slots.menu.state_id;

    // 6-7. `suppressRemoteUpdates` then `clicked`. Suppression is why nothing
    //      is emitted from inside `do_click`: it returns a flag and the
    //      packets are decided below.
    out.slots_changed = do_click(slots, slot_num, button, input, &mut out.dropped);

    //  7b. Every write to a crafting grid inside `clicked` —
    //      `TransientCraftingContainer.setItem`/`removeItem` ->
    //      `menu.slotsChanged` -> `CraftingMenu.slotsChanged` ->
    //      `slotChangedCraftingGrid` — sent the result slot. **Still inside
    //      `clicked`**, and
    //      therefore inside the suppression — which does not stop it, because
    //      it sends directly rather than through the synchronizer. So the
    //      result slot's packet PRECEDES the correction for the grid slot the
    //      click just filled.
    //      `ResultSlot.onTake` reaches the same send once per cell it
    //      consumes, from inside the take; those went out first.
    out.replies.append(&mut slots.env.direct);

    // 8-9. The client's prediction overwrites the belief table. An empty
    //      `changedSlots` — which is what the harness sends — leaves the
    //      belief at whatever was last *sent*, so every slot the click moved
    //      now differs and is answered.
    for (i, stack) in changed_slots {
        if let Some(s) = slots.menu.remote_slots.get_mut(*i) {
            *s = stack.clone();
        }
        if let Some(k) = slots.menu.belief_known.get_mut(*i) {
            *k = true;
        }
    }
    for i in hashed_slots {
        if let Some(k) = slots.menu.belief_known.get_mut(*i) {
            *k = false;
        }
    }
    slots.menu.remote_carried = carried.clone();

    // 11. Either the whole menu or the differences.
    if mismatch {
        out.replies.push(broadcast_full_state(slots));
        out.replies.extend(full_state_data(slots));
    } else {
        out.replies.extend(broadcast_changes(slots));
    }
    out
}

/// `ResultSlot.onTake(player, stack)`, offsets 0-268 — taking a crafted result
/// spends the grid.
///
/// ```text
///    0-  2  checkTakeAchievements(stack)       stats, recipe award: see below
///    5- 31  positioned = craftSlots.asPositionedCraftInput()
///   33- 43  remaining = getRemainingItems(input, level)
///   45-265  for y < input.height, for x < input.width:
///             slot = x + left + (y + top) * craftSlots.getWidth()
///             item = craftSlots.getItem(slot); rem = remaining[x + y * width]
///  128-160    if !item.isEmpty: craftSlots.removeItem(slot, 1); re-read item
///  162-191    if !rem.isEmpty and item.isEmpty: craftSlots.setItem(slot, rem)
///  194-227    else if sameItemSameComponents(item, rem):
///               rem.grow(item.count); craftSlots.setItem(slot, rem)
///  230-255    else if !inventory.add(rem): player.drop(rem, false)
/// ```
///
/// **Every `removeItem` and `setItem` there recomputes the recipe and sends
/// the result slot**, because `TransientCraftingContainer.removeItem`
/// (offsets 10-22, when the removed stack is non-empty) and `setItem`
/// (offsets 10-15) both call `menu.slotsChanged`. The grid is written
/// directly here rather than through [`Slots::set`] so that the recompute
/// runs where vanilla runs it — per cell, inside the take — instead of once
/// after the click; the sends land in [`crate::workstation::Env::direct`].
///
/// `getRemainingItems` (offsets 0-45) is the recipe's own `getRemainingItems`
/// when one matches — `CraftingRecipe.defaultCraftingReminder` for every
/// recipe kind this server parses; only `BannerDuplicateRecipe` and
/// `BookCloningRecipe` override it, and neither is parsed — and
/// `copyAllInputItems` when none does. That one returns the grid's **own**
/// stacks, not copies, so the remainder of a cell is the cell itself after the
/// removal: an empty cell is skipped and a non-empty one is grown by its own
/// count. Unreachable while the result slot holds only what the last
/// recompute matched, and transcribed rather than dropped.
///
/// Two halves are not here. `checkTakeAchievements` awards the craft statistic
/// and `awardUsedRecipes`, neither of which reaches a packet unless the
/// recipe was not yet known; and a remainder that neither fits its cell nor
/// the inventory is `Player.drop(rem, false)`, an item entity this function
/// has no level to spawn. Both are gaps, named.
fn crafting_result_on_take(slots: &mut Slots<'_>) {
    let (input, left, top) = crate::recipes::CraftingInput::positioned(
        CRAFTING_GRID_WIDTH,
        CRAFTING_GRID_HEIGHT,
        &slots.menu.grid,
    );
    let matched = slots.recipes.and_then(|r| r.find(&input)).is_some();
    for y in 0..input.height {
        for x in 0..input.width {
            let cell = x + left + (y + top) * CRAFTING_GRID_WIDTH;
            let Some(mut item) = slots.menu.grid.get(cell).cloned() else {
                continue;
            };
            let mut remainder = if matched {
                input
                    .items
                    .get(x + y * input.width)
                    .and_then(|s| crate::recipes::crafting_remainder(s.item()))
                    .map_or_else(ItemStack::empty, ItemStack::one)
            } else {
                ItemStack::empty()
            };
            if !item.is_empty() {
                item.shrink(1);
                slots.menu.grid[cell] = item.clone();
                crafting_grid_changed(slots);
            }
            if !matched {
                // `copyAllInputItems`: the remainder *is* the cell.
                remainder = item.clone();
            }
            if remainder.is_empty() {
                continue;
            }
            if item.is_empty() {
                slots.menu.grid[cell] = remainder;
                crafting_grid_changed(slots);
            } else if ItemStack::is_same_item_same_components(&item, &remainder) {
                remainder.grow(item.count());
                slots.menu.grid[cell] = remainder;
                crafting_grid_changed(slots);
            } else {
                let _leftover = slots.inventory.add_item(remainder);
            }
        }
    }
}

/// `menu.slotsChanged(craftSlots)` from inside [`crafting_result_on_take`].
fn crafting_grid_changed(slots: &mut Slots<'_>) {
    if let Some(reply) = slots_changed_crafting_grid(slots) {
        slots.env.direct.push(reply);
    }
}

/// `CraftingMenu.slotChangedCraftingGrid`, offsets 0..147.
///
/// Runs the recipe over the grid, writes the result slot, **updates the remote
/// belief for slot 0**, and sends that slot directly.
///
/// The `setRemoteSlot(0, result)` at offset 118 is the line that stops
/// `broadcastChanges` sending slot 0 a second time immediately afterwards.
/// Without it this server would emit two packets where vanilla emits one, and
/// since the harness follows the state id through `container_set_slot`, every
/// later click in the run would be answered on the wrong channel.
///
/// Returns `None` for a menu with no grid, which is every menu but the
/// crafting table.
#[must_use]
pub fn slots_changed_crafting_grid(slots: &mut Slots<'_>) -> Option<Reply> {
    if slots.menu.spec.kind != MenuKind::Crafting {
        return None;
    }
    // A grid with no recipe table computes nothing — but it still writes
    // `EMPTY`, exactly as the miss branch does, so a result left over from a
    // previous grid cannot survive.
    let input = crate::recipes::CraftingInput::of(
        CRAFTING_GRID_WIDTH,
        CRAFTING_GRID_HEIGHT,
        &slots.menu.grid,
    );
    let result = slots
        .recipes
        .map_or_else(ItemStack::empty, |r| r.assemble(&input));

    slots.menu.result = result.clone();
    // `setRemoteSlot(0, result)` — the belief, before the send.
    if let Some(b) = slots.menu.remote_slots.get_mut(0) {
        *b = result.clone();
    }
    if let Some(k) = slots.menu.belief_known.get_mut(0) {
        *k = true;
    }
    let state_id = slots.menu.next_state_id();
    Some(Reply::SetSlot {
        container_id: slots.menu.container_id,
        state_id,
        slot: 0,
        stack: result,
    })
}

/// `CraftingMenu`'s grid is 3x3 — `CraftingMenu(int, Inventory)` builds a
/// `TransientCraftingContainer(this, 3, 3)`.
const CRAFTING_GRID_WIDTH: usize = 3;
const CRAFTING_GRID_HEIGHT: usize = 3;

/// `AbstractContainerMenu.broadcastChanges`.
///
/// Slots in index order, then the carried stack, then the data slots — see
/// [`data_changes`].
#[must_use]
pub fn broadcast_changes(slots: &mut Slots<'_>) -> Vec<Reply> {
    let mut out = Vec::new();
    for i in 0..slots.menu.spec.slots {
        let actual = slots.get(i);
        let believed = slots
            .menu
            .remote_slots
            .get(i)
            .cloned()
            .unwrap_or_else(ItemStack::empty);
        let known = slots.menu.belief_known.get(i).copied().unwrap_or(true);
        if known && believed == actual {
            continue;
        }
        if let Some(s) = slots.menu.remote_slots.get_mut(i) {
            *s = actual.clone();
        }
        if let Some(k) = slots.menu.belief_known.get_mut(i) {
            *k = true;
        }
        let state_id = slots.menu.next_state_id();
        out.push(Reply::SetSlot {
            container_id: slots.menu.container_id,
            state_id,
            slot: i16::try_from(i).unwrap_or(i16::MAX),
            stack: actual,
        });
    }
    // `synchronizeCarriedToRemote`, after the loop and before the data slots.
    if slots.menu.remote_carried != slots.menu.carried {
        slots.menu.remote_carried = slots.menu.carried.clone();
        out.push(Reply::SetCursor {
            stack: slots.menu.carried.clone(),
        });
    }
    out.extend(data_changes(slots));
    out
}

/// The current value of every data slot of this menu, in data-slot order.
///
/// Read from the block entity behind it — a furnace's four counters, a
/// brewing stand's two, a lectern's page — and the menu's fresh value
/// ([`crate::menu::MenuSpec::initial_data`]) for a slot nothing here backs:
/// 0 for a menu-local menu (an anvil's cost, a loom's selection), -1 for an
/// enchanting table's six clue slots. Those are the values a fresh menu
/// already believes, so they are never sent as changes; they are sent only by
/// [`full_state_data`], where vanilla sends them too.
fn data_values(slots: &Slots<'_>) -> Vec<i32> {
    data_values_of(
        slots.menu.spec,
        slots.container.as_deref(),
        &slots.menu.data,
    )
}

/// [`data_values`] without a [`Slots`] — for the sweep, which has no player
/// inventory in hand.
fn data_values_of(spec: MenuSpec, container: Option<&BlockEntity>, data: &[i32]) -> Vec<i32> {
    // `LecternBlockEntity.dataAccess.get(0)` is `page`.
    if spec.kind == MenuKind::ContainerOnly {
        if let Some(page) = container.and_then(|b| b.extra_i32("Page")) {
            return vec![page];
        }
    }
    let backed = container
        .map(BlockEntity::container_data)
        .unwrap_or_default();
    (0..spec.data_slots)
        .map(|i| {
            backed
                .get(i)
                .copied()
                .or_else(|| data.get(i).copied())
                .unwrap_or_else(|| i32::from(spec.initial_data(i)))
        })
        .collect()
}

/// Send the data slots whose value has moved off the remote belief, and
/// record them. The shared body of [`data_changes`] and [`sweep_container`].
fn data_changes_of(menu: &mut Menu, values: Vec<i32>) -> Vec<Reply> {
    let mut out = Vec::new();
    for (i, value) in values.into_iter().enumerate() {
        let Some(remote) = menu.remote_data.get_mut(i) else {
            continue;
        };
        if *remote == value {
            continue;
        }
        *remote = value;
        out.push(Reply::SetData {
            container_id: menu.container_id,
            id: i16::try_from(i).unwrap_or(i16::MAX),
            value,
        });
    }
    out
}

/// `ServerPlayer.doTick`'s `containerMenu.broadcastChanges()`, run by the
/// clock thread for a menu backed by a block entity.
///
/// **This is what carries a block entity's own writes to the player looking
/// at it.** A furnace consuming its fuel, finishing an ingot or advancing its
/// progress bar does so inside `serverTick`, which sends nothing; the client
/// learns of it from the next tick's `broadcastChanges`, which finds the slot
/// or the data value no longer matching what it last sent. Without this the
/// furnace's slots changed on the server and stayed frozen on the screen.
///
/// The same three phases as [`broadcast_changes`] — slots, the carried
/// stack, the data slots — over the **container** slots only. The player's
/// own 36 are not walked here, and the omission is bounded rather than
/// forgotten: nothing on the clock thread writes the player's inventory while
/// a container menu is open, and every connection-thread write already ends
/// in its own `broadcastChanges`. The block entity is read under the store's
/// lock and the menu under its own, both inside the tick lock.
#[must_use]
pub fn sweep_container(
    menu: &mut Menu,
    container: Option<&BlockEntity>,
    second: Option<&BlockEntity>,
) -> Vec<Reply> {
    let mut out = Vec::new();
    // A double chest's `CompoundContainer`: the first half's slots, then the
    // second's — the same split as `Slots::split`.
    let first_size = container.map_or(menu.spec.container_slots / 2, BlockEntity::size);
    for i in 0..menu.spec.container_slots {
        let Some(SlotRef::Container(s)) = slot_ref(menu.spec, i) else {
            continue;
        };
        let actual = match second {
            Some(b) if s >= first_size => b.item(s - first_size),
            _ => container.map_or_else(ItemStack::empty, |c| c.item(s)),
        };
        let believed = menu
            .remote_slots
            .get(i)
            .cloned()
            .unwrap_or_else(ItemStack::empty);
        let known = menu.belief_known.get(i).copied().unwrap_or(true);
        if known && believed == actual {
            continue;
        }
        if let Some(r) = menu.remote_slots.get_mut(i) {
            *r = actual.clone();
        }
        if let Some(k) = menu.belief_known.get_mut(i) {
            *k = true;
        }
        let state_id = menu.next_state_id();
        out.push(Reply::SetSlot {
            container_id: menu.container_id,
            state_id,
            slot: i16::try_from(i).unwrap_or(i16::MAX),
            stack: actual,
        });
    }
    if menu.remote_carried != menu.carried {
        menu.remote_carried = menu.carried.clone();
        out.push(Reply::SetCursor {
            stack: menu.carried.clone(),
        });
    }
    let values = data_values_of(menu.spec, container, &menu.data);
    out.extend(data_changes_of(menu, values));
    out
}

/// The data-slot half of `broadcastChanges`: offsets after
/// `synchronizeCarriedToRemote`, one `synchronizeDataSlotToRemote(i, value)`
/// per slot in index order, which sends only when the value differs from
/// `remoteDataSlots[i]` and then records it.
///
/// This is the whole of how a furnace's progress bar and flame reach a
/// client: `AbstractFurnaceBlockEntity` declares no `getUpdatePacket`, so a
/// furnace burning with nobody looking at it puts nothing on the wire but its
/// `lit` block state.
///
/// Called from a click's `broadcastChanges`; the per-tick sweep
/// `ServerPlayer.doTick` runs is [`sweep_container`].
#[must_use]
pub fn data_changes(slots: &mut Slots<'_>) -> Vec<Reply> {
    let values = data_values(slots);
    data_changes_of(slots.menu, values)
}

/// The data-slot half of `broadcastFullState`: every data slot's value is
/// copied into `remoteDataSlots` **and sent**, changed or not, after the
/// content packet — `sendInitialData`'s tail.
///
/// So a menu opened on a cold furnace sends four zeros, and one opened on a
/// burning furnace sends its four live counters.
#[must_use]
pub fn full_state_data(slots: &mut Slots<'_>) -> Vec<Reply> {
    let values = data_values(slots);
    slots.menu.remote_data.clone_from(&values);
    values
        .into_iter()
        .enumerate()
        .map(|(i, value)| Reply::SetData {
            container_id: slots.menu.container_id,
            id: i16::try_from(i).unwrap_or(i16::MAX),
            value,
        })
        .collect()
}

/// [`full_state_data`] by the name `crate::workstation`'s tests use.
#[must_use]
pub fn send_all_data_slots(slots: &mut Slots<'_>) -> Vec<Reply> {
    full_state_data(slots)
}

/// `handleContainerButtonClick`, offsets 12-120, after the thread hop.
///
/// A press naming a menu that is not open is dropped (offsets 19-33), as is
/// one from a spectator (36-43). `stillValid` (47-86) is not modelled here —
/// see `crate::menu`'s note on it. Then `clickMenuButton`, and
/// `broadcastChanges` **only if it returned true** (106-117).
#[must_use]
pub fn handle_button_click(slots: &mut Slots<'_>, container_id: i32, button: i32) -> Vec<Reply> {
    if container_id != slots.menu.container_id {
        return Vec::new();
    }
    if crate::workstation::click_menu_button(slots, button) {
        broadcast_changes(slots)
    } else {
        Vec::new()
    }
}

/// `AbstractContainerMenu.broadcastFullState` — the answer to a click whose
/// state id had drifted.
///
/// It forces the whole belief table rather than only sending, which is what
/// stops the *next* click re-sending everything.
#[must_use]
pub fn broadcast_full_state(slots: &mut Slots<'_>) -> Reply {
    let items: Vec<ItemStack> = (0..slots.menu.spec.slots).map(|i| slots.get(i)).collect();
    slots.menu.remote_slots = items.clone();
    slots.menu.belief_known = vec![true; items.len()];
    slots.menu.remote_carried = slots.menu.carried.clone();
    let state_id = slots.menu.next_state_id();
    Reply::SetContent {
        container_id: slots.menu.container_id,
        state_id,
        items,
        carried: slots.menu.carried.clone(),
    }
}

/// `sendAllDataToRemote` — the content packet a freshly opened menu sends,
/// and the belief table it establishes.
///
/// **This is what makes the first click answerable at all.** Without forcing
/// the belief table here, every slot would read as differing from an empty
/// belief on the first `broadcastChanges` and the menu would resend itself.
#[must_use]
pub fn send_all_data_to_remote(slots: &mut Slots<'_>) -> Reply {
    broadcast_full_state(slots)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blockentity::{BlockEntity, Kind};
    use crate::menu::menu_for_state;

    const CHEST_MENU_HOTBAR_0: usize = 54;
    const CRAFTING_MENU_HOTBAR_0: usize = 37;

    /// The two directions of the `Inventory`/`InventoryMenu` numbering agree
    /// on all 36 indices, and the hotbar really does move — an identity map
    /// would pass a round trip and put every hand-back 27 slots out.
    #[test]
    fn the_menu_slot_and_the_inventory_index_invert_each_other() {
        for index in 0..crate::player::INVENTORY_SIZE {
            let slot = menu_slot_for_inventory_index(index).expect("every index is in the menu");
            assert_eq!(inventory_index_for_menu_slot(slot), Some(index));
        }
        assert_eq!(menu_slot_for_inventory_index(0), Some(36));
        assert_eq!(menu_slot_for_inventory_index(2), Some(38));
        assert_eq!(menu_slot_for_inventory_index(9), Some(9));
        assert_eq!(menu_slot_for_inventory_index(35), Some(35));
        assert_eq!(menu_slot_for_inventory_index(36), None);
    }

    /// A plain left-click, with the empty prediction the gameplay harness
    /// sends — which is what makes the server answer at all.
    fn click(container_id: i32, state_id: i32, slot_num: i32, button: i8) -> ClickPacket {
        ClickPacket {
            container_id,
            state_id,
            slot_num,
            button,
            input: ContainerInput::Pickup,
            changed_slots: Vec::new(),
            hashed_slots: Vec::new(),
            carried: ItemStack::empty(),
        }
    }

    fn chest_spec() -> MenuSpec {
        menu_for_state("minecraft:chest").expect("chest")
    }

    fn crafting_spec() -> MenuSpec {
        menu_for_state("minecraft:crafting_table").expect("crafting")
    }

    /// Open a chest menu with `stone` in hotbar slot 0, exactly as scenario
    /// 560 leaves it, and return the state after `sendAllDataToRemote`.
    fn opened_chest() -> (Menu, Hotbar, BlockEntity) {
        let spec = chest_spec();
        let mut menu = Menu::new(5, spec);
        let mut inv = Hotbar::new();
        inv.set_stack(0, ItemStack::one("minecraft:stone"));
        let mut be = BlockEntity::new(Kind::Chest);
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: Some(&mut be),
            recipes: None,
            second: None,
            env: crate::workstation::Env::default(),
        };
        let opened = send_all_data_to_remote(&mut slots);
        // The open costs exactly one state id, and it is 1 on a fresh menu.
        assert!(matches!(opened, Reply::SetContent { state_id: 1, .. }));
        (menu, inv, be)
    }

    /// **A menu addressing more slots than its block entity has must not eat
    /// the stack**, and `Kind::Sign` is what made that reachable.
    ///
    /// A sign's `container_size()` is 0, so every container slot of a chest
    /// menu is outside it. Before the bounds test in `may_place` this click
    /// passed `mayPlace`, reached `safeInsert`, split the stone off the
    /// cursor and handed it to `set`, whose `set_item` writes nowhere — the
    /// third item-vanishing bug on this path and the first that no scenario
    /// can reach, because nothing opens a menu on a sign.
    ///
    /// It is a unit test rather than a check for exactly that reason, and the
    /// assertion is on the **cursor**, not on the container: a container that
    /// swallowed the stone and a container that refused it look identical
    /// from the container's side, since both leave it empty.
    #[test]
    fn a_container_smaller_than_its_menu_refuses_rather_than_swallowing() {
        let (mut menu, mut inv, _) = opened_chest();
        // Pick the stone up first, so there is something on the cursor.
        let mut chest = BlockEntity::new(Kind::Chest);
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: Some(&mut chest),
            recipes: None,
            second: None,
            env: crate::workstation::Env::default(),
        };
        let _ = handle_click(&mut slots, &click(5, 1, CHEST_MENU_HOTBAR_0 as i32, 0));
        assert_eq!(
            menu.carried,
            ItemStack::one("minecraft:stone"),
            "the stone is on the cursor"
        );

        // The block entity is now a sign — nought slots — under the same
        // open chest menu.
        let mut sign = BlockEntity::new(Kind::Sign);
        assert_eq!(sign.size(), 0);
        let state_id = menu.state_id;
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: Some(&mut sign),
            recipes: None,
            second: None,
            env: crate::workstation::Env::default(),
        };
        let _ = handle_click(&mut slots, &click(5, state_id, 0, 0));
        assert_eq!(
            menu.carried,
            ItemStack::one("minecraft:stone"),
            "the click was refused and the stone is still on the cursor"
        );
        assert!(sign.item(0).is_empty(), "and nothing was written into it");
    }

    /// **Scenario 560, end to end, as a unit test.** This is the claim the
    /// whole item exists to make, and it is written so that a server with no
    /// block entities cannot pass it: the second click's packet names slot 0
    /// and carries a stone, and there is nowhere but a block entity for that
    /// stone to be.
    #[test]
    fn storing_an_item_in_a_chest_answers_two_corrections() {
        let (mut menu, mut inv, mut be) = opened_chest();
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: Some(&mut be),
            recipes: None,
            second: None,
            env: crate::workstation::Env::default(),
        };

        // Click 54: pick the stone up off the hotbar.
        let first = handle_click(&mut slots, &click(5, 1, CHEST_MENU_HOTBAR_0 as i32, 0));
        assert_eq!(
            first.replies,
            vec![
                Reply::SetSlot {
                    container_id: 5,
                    state_id: 2,
                    slot: 54,
                    stack: ItemStack::empty(),
                },
                Reply::SetCursor {
                    stack: ItemStack::one("minecraft:stone"),
                },
            ],
            "the emptied hotbar slot is corrected, then the cursor — and the \
             cursor is a set_cursor_item, which costs no state id"
        );

        // Click 0: put it in the chest.
        //
        // **The state id echoed here is 2, not 1**, and that is the client
        // following the server rather than a detail of the test.
        // `oracle_runner::client` updates its tracked `state_id` from every
        // `container_set_content` *and* every `container_set_slot`, so the
        // second click echoes what the first click's packet stamped. Echoing a
        // stale 1 makes `mismatch` true and answers with the whole menu — the
        // branch `a_stale_state_id_is_answered_with_the_whole_menu` covers.
        //
        // The consequence is worth stating once: **a server that sends a
        // different NUMBER of packets than vanilla desynchronises this
        // counter**, and the next click is then answered on a different
        // channel entirely. Packet count is not merely observable here, it is
        // load-bearing for every later click in the run.
        let st = slots.menu.state_id;
        let second = handle_click(&mut slots, &click(5, st, 0, 0));
        assert_eq!(
            second.replies,
            vec![Reply::SetSlot {
                container_id: 5,
                state_id: 3,
                slot: 0,
                stack: ItemStack::one("minecraft:stone"),
            }],
            "one correction naming the chest slot and the stone in it, and no \
             cursor packet because the client already predicted an empty cursor"
        );
        // And the stone is actually in the chest, which is the half the wire
        // cannot show.
        assert_eq!(be.item(0), ItemStack::one("minecraft:stone"));
        assert!(inv.stack(0).is_empty(), "and no longer in the hotbar");
    }

    /// **Scenario 561: the same two clicks with the first moved onto an empty
    /// slot must produce NOTHING.**
    ///
    /// This is the test that forbids the cheap fix. A server that answered
    /// every container click with a correction naming the clicked slot would
    /// pass the test above and fail this one, which is exactly the
    /// relationship 560 and 561 have in the corpus.
    #[test]
    fn clicking_an_empty_chest_slot_twice_is_answered_with_silence() {
        let (mut menu, mut inv, mut be) = opened_chest();
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: Some(&mut be),
            recipes: None,
            second: None,
            env: crate::workstation::Env::default(),
        };
        for _ in 0..2 {
            let out = handle_click(&mut slots, &click(5, 1, 0, 0));
            assert!(
                out.replies.is_empty(),
                "an empty slot clicked with an empty cursor moves nothing, so \
                 broadcastChanges finds nothing to correct: {:?}",
                out.replies
            );
        }
        // The click ran — it was not rejected early — and the state id did
        // not move, because nothing was sent.
        assert_eq!(menu.state_id, 1, "no packet, no state id");
    }

    /// **The prediction half of rule 27, made into a test.** The same click
    /// as 560's first, but with the client correctly predicting the result,
    /// must be answered with silence. Without this, "we send a correction"
    /// and "we send a correction *only when the client is wrong*" are
    /// indistinguishable.
    #[test]
    fn a_correct_client_prediction_is_answered_with_silence() {
        let (mut menu, mut inv, mut be) = opened_chest();
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: Some(&mut be),
            recipes: None,
            second: None,
            env: crate::workstation::Env::default(),
        };
        let out = handle_click(
            &mut slots,
            &ClickPacket {
                // The client says: slot 54 is now empty...
                changed_slots: vec![(CHEST_MENU_HOTBAR_0, ItemStack::empty())],
                // ...and I am holding the stone.
                carried: ItemStack::one("minecraft:stone"),
                ..click(5, 1, CHEST_MENU_HOTBAR_0 as i32, 0)
            },
        );
        assert!(
            out.replies.is_empty(),
            "a client that predicted correctly is told nothing: {:?}",
            out.replies
        );
        // The click still happened. This is the assertion that separates
        // "silent because correct" from "silent because nothing ran".
        assert!(inv.stack(0).is_empty());
        assert_eq!(menu.carried, ItemStack::one("minecraft:stone"));
    }

    /// A stale state id is answered with the whole menu instead of a
    /// correction — `broadcastFullState`. A branch no corpus scenario reaches
    /// today, which is exactly why it needs a unit test.
    #[test]
    fn a_stale_state_id_is_answered_with_the_whole_menu() {
        let (mut menu, mut inv, mut be) = opened_chest();
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: Some(&mut be),
            recipes: None,
            second: None,
            env: crate::workstation::Env::default(),
        };
        let out = handle_click(
            &mut slots,
            &click(
                5, // The menu's state id is 1; the client claims 0.
                0,
                CHEST_MENU_HOTBAR_0 as i32,
                0,
            ),
        );
        assert_eq!(out.replies.len(), 1);
        match &out.replies[0] {
            Reply::SetContent {
                container_id,
                state_id,
                items,
                carried,
            } => {
                assert_eq!((*container_id, *state_id), (5, 2));
                assert_eq!(items.len(), 63);
                assert_eq!(
                    carried,
                    &ItemStack::one("minecraft:stone"),
                    "the click still ran before the full state was sent"
                );
            }
            other => panic!("expected a full state, got {other:?}"),
        }
    }

    /// The four silent early returns are silent, and — the part that matters
    /// — they are silent *without running the click*.
    #[test]
    fn a_click_naming_the_wrong_window_or_slot_does_nothing_at_all() {
        for (window, slot) in [(6, 54), (5, 63), (5, -5)] {
            let (mut menu, mut inv, mut be) = opened_chest();
            let mut slots = Slots {
                menu: &mut menu,
                inventory: &mut inv,
                container: Some(&mut be),
                recipes: None,
                second: None,
                env: crate::workstation::Env::default(),
            };
            let out = handle_click(&mut slots, &click(window, 1, slot, 0));
            assert!(out.replies.is_empty(), "window {window} slot {slot}");
            assert!(
                !inv.stack(0).is_empty(),
                "window {window} slot {slot}: the click must not have run — \
                 the stone is still in the hotbar"
            );
        }
    }

    /// **The same refusal, but with something on the cursor — and this is the
    /// version that can fail.**
    ///
    /// The test above clicks with an empty cursor, and with an empty cursor
    /// every arm of `doClick` is a no-op anyway: deleting the
    /// `isValidSlotIndex` guard entirely changes nothing it can see. The
    /// mutation sweep reported exactly that, which is defect class #19 in
    /// miniature — a test paired with a scenario that never reaches the
    /// branch.
    ///
    /// With a stack on the cursor the branch is live: the click reaches
    /// `safeInsert`, which asks `may_place` about a slot that does not exist.
    /// Answer `true` and the stack is split off the cursor and handed to a
    /// `set` with nowhere to put it — **the item is destroyed**.
    #[test]
    fn an_out_of_range_slot_does_not_swallow_the_cursor() {
        for slot in [63i32, 999, -5] {
            let spec = chest_spec();
            let mut menu = Menu::new(5, spec);
            let mut inv = Hotbar::new();
            let mut be = BlockEntity::new(Kind::Chest);
            let mut slots = Slots {
                menu: &mut menu,
                inventory: &mut inv,
                container: Some(&mut be),
                recipes: None,
                second: None,
                env: crate::workstation::Env::default(),
            };
            slots.menu.carried = ItemStack::new("minecraft:stone", 3);
            let _ = send_all_data_to_remote(&mut slots);
            let st = slots.menu.state_id;
            let out = handle_click(&mut slots, &click(5, st, slot, 0));
            // **`slots_changed` must stay false**, and this is what the
            // `isValidSlotIndex` guard is still for now that `may_place`
            // refuses a slot that does not exist. `doClick`'s tail calls
            // `slot.setChanged()` unconditionally on the `slotNum >= 0` path,
            // and that is what drives `slotsChanged` and therefore the
            // crafting recipe. Without the guard, clicking slot 999 of a
            // crafting table would recompute the grid.
            assert!(
                !out.slots_changed,
                "slot {slot}: a slot that does not exist did not change"
            );
            assert_eq!(
                menu.carried,
                ItemStack::new("minecraft:stone", 3),
                "slot {slot}: the cursor kept its whole stack rather than \
                 losing part of it into a slot that does not exist"
            );
            assert!(be.is_empty(), "slot {slot}: and nothing reached the chest");
        }
    }

    /// The secondary button takes the larger half, and a second secondary
    /// click puts one back. Two branches of `doClick` no corpus scenario
    /// reaches — `container_click`'s `button` defaults to 0 — and both are
    /// arithmetic that a name-only inventory could not have expressed.
    #[test]
    fn a_right_click_halves_a_stack_and_places_one_at_a_time() {
        let spec = chest_spec();
        let mut menu = Menu::new(5, spec);
        let mut inv = Hotbar::new();
        let mut be = BlockEntity::new(Kind::Chest);
        be.set_item(0, ItemStack::new("minecraft:stone", 7));
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: Some(&mut be),
            recipes: None,
            second: None,
            env: crate::workstation::Env::default(),
        };
        let _ = send_all_data_to_remote(&mut slots);

        // Right-click slot 0 with an empty cursor: (7 + 1) / 2 = 4.
        let _ = handle_click(&mut slots, &click(5, 1, 0, 1));
        assert_eq!(menu.carried.count(), 4, "the larger half");
        assert_eq!(be.item(0).count(), 3);

        // Right-click slot 1 with those four: exactly one goes down.
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: Some(&mut be),
            recipes: None,
            second: None,
            env: crate::workstation::Env::default(),
        };
        let st = slots.menu.state_id;
        let _ = handle_click(&mut slots, &click(5, st, 1, 1));
        assert_eq!(be.item(1).count(), 1, "one at a time");
        assert_eq!(menu.carried.count(), 3);
    }

    /// `safeInsert` caps at the slot's maximum and hands back the remainder
    /// rather than deleting it. A branch reachable only with counts, and the
    /// one that would silently destroy items if the cap were applied to the
    /// stack instead of to the insert.
    #[test]
    fn inserting_past_a_full_stack_leaves_the_remainder_on_the_cursor() {
        let spec = chest_spec();
        let mut menu = Menu::new(5, spec);
        let mut inv = Hotbar::new();
        let mut be = BlockEntity::new(Kind::Chest);
        be.set_item(0, ItemStack::new("minecraft:stone", 60));
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: Some(&mut be),
            recipes: None,
            second: None,
            env: crate::workstation::Env::default(),
        };
        let _ = send_all_data_to_remote(&mut slots);
        slots.menu.carried = ItemStack::new("minecraft:stone", 10);

        let _ = handle_click(&mut slots, &click(5, 1, 0, 0));
        assert_eq!(be.item(0).count(), 64, "filled to the maximum");
        assert_eq!(
            menu.carried.count(),
            6,
            "and the six that did not fit are still on the cursor, not gone"
        );
    }

    /// Two different items swap, and the swap is refused when the cursor
    /// holds more than the slot could take. Both arms of the `mayPlace`
    /// branch.
    #[test]
    fn different_items_swap_and_an_oversized_cursor_does_not() {
        let spec = chest_spec();
        let mut menu = Menu::new(5, spec);
        let mut inv = Hotbar::new();
        let mut be = BlockEntity::new(Kind::Chest);
        be.set_item(0, ItemStack::one("minecraft:oak_log"));
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: Some(&mut be),
            recipes: None,
            second: None,
            env: crate::workstation::Env::default(),
        };
        let _ = send_all_data_to_remote(&mut slots);
        slots.menu.carried = ItemStack::new("minecraft:stone", 5);
        let _ = handle_click(&mut slots, &click(5, 1, 0, 0));
        assert_eq!(be.item(0), ItemStack::new("minecraft:stone", 5));
        assert_eq!(menu.carried, ItemStack::one("minecraft:oak_log"));

        // A cursor holding more than the slot's maximum refuses the swap and
        // leaves both where they were.
        let slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: Some(&mut be),
            recipes: None,
            second: None,
            env: crate::workstation::Env::default(),
        };
        slots.menu.carried = ItemStack::new("minecraft:oak_log", 65);
        let before = be.item(0);
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: Some(&mut be),
            recipes: None,
            second: None,
            env: crate::workstation::Env::default(),
        };
        let st = slots.menu.state_id;
        let _ = handle_click(&mut slots, &click(5, st, 0, 0));
        assert_eq!(be.item(0), before, "the swap was refused");
        assert_eq!(menu.carried.count(), 65, "and the cursor kept its stack");
    }

    /// **The slot-index map, against both menu shapes.** The hotbar is menu
    /// slot 54 in a chest and 37 in a crafting table, and both map to
    /// `Inventory` index 0. An implementation that shifted rather than
    /// reversed would put it at index 27.
    #[test]
    fn the_hotbar_is_slot_fifty_four_in_a_chest_and_thirty_seven_in_a_grid() {
        let chest = chest_spec();
        assert_eq!(
            slot_ref(chest, CHEST_MENU_HOTBAR_0),
            Some(SlotRef::Inventory(0))
        );
        assert_eq!(slot_ref(chest, 62), Some(SlotRef::Inventory(8)));
        assert_eq!(slot_ref(chest, 27), Some(SlotRef::Inventory(9)));
        assert_eq!(slot_ref(chest, 53), Some(SlotRef::Inventory(35)));
        assert_eq!(slot_ref(chest, 0), Some(SlotRef::Container(0)));
        assert_eq!(slot_ref(chest, 26), Some(SlotRef::Container(26)));
        assert_eq!(slot_ref(chest, 63), None, "past the end of the menu");

        let crafting = crafting_spec();
        assert_eq!(
            slot_ref(crafting, CRAFTING_MENU_HOTBAR_0),
            Some(SlotRef::Inventory(0))
        );
        assert_eq!(slot_ref(crafting, 0), Some(SlotRef::Result));
        assert_eq!(slot_ref(crafting, 1), Some(SlotRef::Grid(0)));
        assert_eq!(slot_ref(crafting, 9), Some(SlotRef::Grid(8)));
        assert_eq!(slot_ref(crafting, 10), Some(SlotRef::Inventory(9)));
        assert_eq!(slot_ref(crafting, 45), Some(SlotRef::Inventory(8)));
        assert_eq!(slot_ref(crafting, 46), None);
    }

    /// The crafter is the one menu whose result slot comes **after** the
    /// inventory: its nine, then 36, then slot 45. The vanilla recording of
    /// `755` has the held stick at slots 36 and 37 of a 46-slot menu, which
    /// is only possible if the hotbar starts at 36 — a result-first layout
    /// like the crafting table's would put it at 37.
    #[test]
    fn the_crafter_puts_its_result_after_the_inventory() {
        let crafter = menu_for_state("minecraft:crafter").expect("crafter");
        assert_eq!(slot_ref(crafter, 0), Some(SlotRef::Container(0)));
        assert_eq!(slot_ref(crafter, 8), Some(SlotRef::Container(8)));
        assert_eq!(slot_ref(crafter, 9), Some(SlotRef::Inventory(9)));
        assert_eq!(slot_ref(crafter, 36), Some(SlotRef::Inventory(0)));
        assert_eq!(slot_ref(crafter, 44), Some(SlotRef::Inventory(8)));
        assert_eq!(slot_ref(crafter, 45), Some(SlotRef::Result));
        assert_eq!(slot_ref(crafter, 46), None);
        // And no other kind reaches a result past the inventory.
        let dispenser = menu_for_state("minecraft:dispenser").expect("dispenser");
        assert_eq!(slot_ref(dispenser, 44), Some(SlotRef::Inventory(8)));
        assert_eq!(slot_ref(dispenser, 45), None);
    }

    /// Nothing may be put into a crafting result slot. `ResultSlot.mayPlace`
    /// is the single override that makes this true, and without it a click
    /// carrying a log would drop it into slot 0 and the recipe would run over
    /// a grid the log never reached.
    #[test]
    fn a_crafting_result_slot_refuses_a_placement() {
        let spec = crafting_spec();
        let mut menu = Menu::new(7, spec);
        let mut inv = Hotbar::new();
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: None,
            recipes: None,
            second: None,
            env: crate::workstation::Env::default(),
        };
        // The cursor is primed **before** the belief table is established, so
        // `remote_carried` starts out agreeing with it and this test measures
        // the click rather than the priming.
        slots.menu.carried = ItemStack::one("minecraft:oak_log");
        let _ = send_all_data_to_remote(&mut slots);
        let st = slots.menu.state_id;
        let out = handle_click(&mut slots, &click(7, st, 0, 0));
        assert!(menu.result.is_empty(), "the log did not go into the result");
        assert_eq!(
            menu.carried,
            ItemStack::one("minecraft:oak_log"),
            "it is still on the cursor"
        );
        // **No slot was corrected**, which is the claim: the click reached
        // `doClick`, ran the whole PICKUP branch, and moved nothing.
        assert!(
            !out.replies
                .iter()
                .any(|r| matches!(r, Reply::SetSlot { .. })),
            "no slot moved, so no slot was corrected: {:?}",
            out.replies
        );
        // The one packet that *is* owed is the cursor, and it is owed because
        // the harness claimed an empty one. That is not the click doing
        // something — it is `synchronizeCarriedToRemote` refusing a wrong
        // prediction, and asserting it here is what stops this test passing
        // for the wrong reason if the result slot ever started accepting.
        assert_eq!(
            out.replies,
            vec![Reply::SetCursor {
                stack: ItemStack::one("minecraft:oak_log")
            }]
        );
    }

    /// **Scenario 562, end to end.** Two clicks: pick the log off the hotbar,
    /// put it in the grid. The second must produce **two** packets in this
    /// order — the recipe result for slot 0, then the correction for the grid
    /// slot the click filled.
    ///
    /// The order is not cosmetic. `slotChangedCraftingGrid` sends directly
    /// from inside `clicked()`, before `broadcastChanges` runs at all, so the
    /// result precedes the correction; and its `setRemoteSlot(0, result)` is
    /// what stops `broadcastChanges` sending slot 0 a **second** time.
    #[test]
    fn a_log_in_the_grid_fills_the_result_slot_with_four_planks() {
        let spec = crafting_spec();
        let mut menu = Menu::new(7, spec);
        let mut inv = Hotbar::new();
        inv.set_stack(0, ItemStack::one("minecraft:oak_log"));
        let recipes = crate::recipes::Recipes::parse(
            &serde_json::json!({
                "oak_planks": {
                    "type": "minecraft:crafting_shapeless",
                    "ingredients": ["#minecraft:oak_logs"],
                    "result": { "count": 4, "id": "minecraft:oak_planks" }
                }
            }),
            &serde_json::json!({ "oak_logs": { "values": ["minecraft:oak_log"] } }),
        )
        .expect("a one-recipe table");
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: None,
            recipes: Some(&recipes),
            second: None,
            env: crate::workstation::Env::default(),
        };
        let _ = send_all_data_to_remote(&mut slots);

        // Click 37: the hotbar slot holding the log.
        let st = slots.menu.state_id;
        let first = handle_click(&mut slots, &click(7, st, CRAFTING_MENU_HOTBAR_0 as i32, 0));
        assert_eq!(
            first.replies,
            vec![
                Reply::SetSlot {
                    container_id: 7,
                    state_id: 2,
                    slot: 37,
                    stack: ItemStack::empty(),
                },
                Reply::SetCursor {
                    stack: ItemStack::one("minecraft:oak_log"),
                },
            ],
            "picking the log up touches no grid cell, so no recipe runs"
        );

        // Click 1: the first grid cell.
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: None,
            recipes: Some(&recipes),
            second: None,
            env: crate::workstation::Env::default(),
        };
        let st = slots.menu.state_id;
        let second = handle_click(&mut slots, &click(7, st, 1, 0));
        assert_eq!(
            second.replies,
            vec![
                Reply::SetSlot {
                    container_id: 7,
                    state_id: 3,
                    slot: 0,
                    stack: ItemStack::new("minecraft:oak_planks", 4),
                },
                Reply::SetSlot {
                    container_id: 7,
                    state_id: 4,
                    slot: 1,
                    stack: ItemStack::one("minecraft:oak_log"),
                },
            ],
            "the recipe result comes first, then the grid correction — and \
             slot 0 is sent exactly ONCE, because slotChangedCraftingGrid \
             updated the belief before sending it"
        );
        assert_eq!(menu.result, ItemStack::new("minecraft:oak_planks", 4));
        assert_eq!(menu.grid[0], ItemStack::one("minecraft:oak_log"));
    }

    /// A one-recipe table — one oak log, shapeless, makes four planks — and a
    /// second recipe that leaves a remainder: a milk bucket makes a cake here,
    /// shapeless, which no vanilla recipe does but which exercises the
    /// reminder without a six-cell grid.
    fn planks_and_milk() -> crate::recipes::Recipes {
        crate::recipes::Recipes::parse(
            &serde_json::json!({
                "oak_planks": {
                    "type": "minecraft:crafting_shapeless",
                    "ingredients": ["#minecraft:oak_logs"],
                    "result": { "count": 4, "id": "minecraft:oak_planks" }
                },
                "cake": {
                    "type": "minecraft:crafting_shapeless",
                    "ingredients": ["minecraft:milk_bucket"],
                    "result": { "count": 1, "id": "minecraft:cake" }
                }
            }),
            &serde_json::json!({ "oak_logs": { "values": ["minecraft:oak_log"] } }),
        )
        .expect("a two-recipe table")
    }

    /// A crafting table with `cell` holding `grid` and the result already
    /// computed and believed, as the click that filled the cell leaves it.
    fn table_holding(grid: ItemStack, cell: usize, recipes: &crate::recipes::Recipes) -> Menu {
        let mut menu = Menu::new(7, crafting_spec());
        let mut inv = Hotbar::new();
        menu.grid[cell] = grid;
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: None,
            recipes: Some(recipes),
            second: None,
            env: crate::workstation::Env::default(),
        };
        let _ = slots_changed_crafting_grid(&mut slots);
        let _ = send_all_data_to_remote(&mut slots);
        menu
    }

    /// **Taking the planks spends the log** — `ResultSlot.onTake`. The
    /// removal recomputes on the spot, so the emptied result is sent
    /// *directly*, ahead of the grid correction and the cursor; slot 0 is not
    /// sent a second time by `broadcastChanges`, because the direct send
    /// updated the belief. Before the port this click left the log in the
    /// grid and answered with slot 0 and the cursor only.
    #[test]
    fn taking_crafted_planks_spends_the_log() {
        let recipes = planks_and_milk();
        let mut menu = table_holding(ItemStack::one("minecraft:oak_log"), 0, &recipes);
        let mut inv = Hotbar::new();
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: None,
            recipes: Some(&recipes),
            second: None,
            env: crate::workstation::Env::default(),
        };
        let st = slots.menu.state_id;
        let out = handle_click(&mut slots, &click(7, st, 0, 0));
        assert_eq!(
            out.replies,
            vec![
                Reply::SetSlot {
                    container_id: 7,
                    state_id: st + 1,
                    slot: 0,
                    stack: ItemStack::empty(),
                },
                Reply::SetSlot {
                    container_id: 7,
                    state_id: st + 2,
                    slot: 1,
                    stack: ItemStack::empty(),
                },
                Reply::SetCursor {
                    stack: ItemStack::new("minecraft:oak_planks", 4),
                },
            ]
        );
        assert!(menu.grid[0].is_empty());
    }

    /// Two logs in the cell: one is spent, the recipe still matches, and the
    /// result is sent **full** again — directly — before the grid correction.
    #[test]
    fn taking_planks_from_two_logs_leaves_one_and_refills_the_result() {
        let recipes = planks_and_milk();
        let mut menu = table_holding(ItemStack::new("minecraft:oak_log", 2), 4, &recipes);
        let mut inv = Hotbar::new();
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: None,
            recipes: Some(&recipes),
            second: None,
            env: crate::workstation::Env::default(),
        };
        let st = slots.menu.state_id;
        let out = handle_click(&mut slots, &click(7, st, 0, 0));
        assert_eq!(
            out.replies,
            vec![
                Reply::SetSlot {
                    container_id: 7,
                    state_id: st + 1,
                    slot: 0,
                    stack: ItemStack::new("minecraft:oak_planks", 4),
                },
                Reply::SetSlot {
                    container_id: 7,
                    state_id: st + 2,
                    slot: 5,
                    stack: ItemStack::one("minecraft:oak_log"),
                },
                Reply::SetCursor {
                    stack: ItemStack::new("minecraft:oak_planks", 4),
                },
            ]
        );
    }

    /// A remainder is a second write to the cell and so a **second** direct
    /// send: the removal empties the cell and the result (send one), then
    /// `setItem(bucket)` recomputes again (send two, still empty).
    #[test]
    fn a_milk_bucket_leaves_its_bucket_and_recomputes_twice() {
        let recipes = planks_and_milk();
        let mut menu = table_holding(ItemStack::one("minecraft:milk_bucket"), 0, &recipes);
        assert_eq!(menu.result, ItemStack::one("minecraft:cake"));
        let mut inv = Hotbar::new();
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: None,
            recipes: Some(&recipes),
            second: None,
            env: crate::workstation::Env::default(),
        };
        let st = slots.menu.state_id;
        let out = handle_click(&mut slots, &click(7, st, 0, 0));
        let slot0: Vec<_> = out
            .replies
            .iter()
            .filter(|r| matches!(r, Reply::SetSlot { slot: 0, .. }))
            .collect();
        assert_eq!(slot0.len(), 2, "{:?}", out.replies);
        assert_eq!(menu.grid[0], ItemStack::one("minecraft:bucket"));
    }

    /// `ResultContainer.removeItem` ignores the count: a right-click on four
    /// planks asks for `(4 + 1) / 2 = 3` and gets all four.
    #[test]
    fn a_right_click_on_the_crafting_result_takes_the_whole_stack() {
        let recipes = planks_and_milk();
        let mut menu = table_holding(ItemStack::one("minecraft:oak_log"), 0, &recipes);
        let mut inv = Hotbar::new();
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: None,
            recipes: Some(&recipes),
            second: None,
            env: crate::workstation::Env::default(),
        };
        let st = slots.menu.state_id;
        let _ = handle_click(&mut slots, &click(7, st, 0, 1));
        assert_eq!(menu.carried, ItemStack::new("minecraft:oak_planks", 4));
        assert!(menu.result.is_empty());
        assert!(menu.grid[0].is_empty());
    }

    /// **A shift-click on the result crafts until the grid runs out** —
    /// `CraftingMenu.quickMoveStack`'s index-0 arm under `doClick`'s loop.
    /// Two logs make eight planks, all in the last hotbar slot (the move is
    /// reversed over 10..46, and the second batch merges onto the first), and
    /// the grid ends empty.
    #[test]
    fn shift_clicking_the_result_crafts_until_the_grid_runs_out() {
        let recipes = planks_and_milk();
        let mut menu = table_holding(ItemStack::new("minecraft:oak_log", 2), 0, &recipes);
        let mut inv = Hotbar::new();
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: None,
            recipes: Some(&recipes),
            second: None,
            env: crate::workstation::Env::default(),
        };
        let st = slots.menu.state_id;
        let out = handle_click(&mut slots, &quick_move(7, st, 0));
        assert_eq!(inv.stack(8), ItemStack::new("minecraft:oak_planks", 8));
        assert!(menu.grid[0].is_empty());
        assert!(menu.result.is_empty());
        // One direct result send per consumed log, then the corrections.
        let slot0: Vec<_> = out
            .replies
            .iter()
            .filter_map(|r| match r {
                Reply::SetSlot { slot: 0, stack, .. } => Some(stack.count()),
                _ => None,
            })
            .collect();
        assert_eq!(slot0, vec![4, 0], "{:?}", out.replies);
    }

    /// A shift-clicked log goes into the **grid** first — offsets 99-109 —
    /// and its placement there is a `setItem`, so the result is sent before
    /// the corrections, exactly as a `PICKUP` placement is.
    #[test]
    fn a_shift_clicked_log_goes_into_the_grid_and_crafts() {
        let recipes = planks_and_milk();
        let mut menu = Menu::new(7, crafting_spec());
        let mut inv = Hotbar::new();
        inv.set_stack(0, ItemStack::one("minecraft:oak_log"));
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: None,
            recipes: Some(&recipes),
            second: None,
            env: crate::workstation::Env::default(),
        };
        let _ = send_all_data_to_remote(&mut slots);
        let st = slots.menu.state_id;
        let out = handle_click(
            &mut slots,
            &quick_move(7, st, CRAFTING_MENU_HOTBAR_0 as i32),
        );
        assert_eq!(
            out.replies,
            vec![
                Reply::SetSlot {
                    container_id: 7,
                    state_id: st + 1,
                    slot: 0,
                    stack: ItemStack::new("minecraft:oak_planks", 4),
                },
                Reply::SetSlot {
                    container_id: 7,
                    state_id: st + 2,
                    slot: 1,
                    stack: ItemStack::one("minecraft:oak_log"),
                },
                Reply::SetSlot {
                    container_id: 7,
                    state_id: st + 3,
                    slot: CRAFTING_MENU_HOTBAR_0 as i16,
                    stack: ItemStack::empty(),
                },
            ]
        );
    }

    /// Taking a whole stack out of a grid cell recomputes **twice**:
    /// `tryRemove`'s `remove` is a `removeItem` that removed something, and
    /// the slot then being empty, its `setByPlayer(EMPTY)` is a `setItem`.
    #[test]
    fn taking_the_log_back_out_of_the_grid_sends_the_result_twice() {
        let recipes = planks_and_milk();
        let mut menu = table_holding(ItemStack::one("minecraft:oak_log"), 0, &recipes);
        let mut inv = Hotbar::new();
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: None,
            recipes: Some(&recipes),
            second: None,
            env: crate::workstation::Env::default(),
        };
        let st = slots.menu.state_id;
        let out = handle_click(&mut slots, &click(7, st, 1, 0));
        assert_eq!(
            out.replies,
            vec![
                Reply::SetSlot {
                    container_id: 7,
                    state_id: st + 1,
                    slot: 0,
                    stack: ItemStack::empty(),
                },
                Reply::SetSlot {
                    container_id: 7,
                    state_id: st + 2,
                    slot: 0,
                    stack: ItemStack::empty(),
                },
                Reply::SetSlot {
                    container_id: 7,
                    state_id: st + 3,
                    slot: 1,
                    stack: ItemStack::empty(),
                },
                Reply::SetCursor {
                    stack: ItemStack::one("minecraft:oak_log"),
                },
            ]
        );
    }

    /// A number key over the result is a take: `doClick`'s `SWAP` arm calls
    /// `slot.onTake` at offset 1199, so the planks land in the hotbar and the
    /// log is spent.
    #[test]
    fn a_number_key_on_the_result_spends_the_grid() {
        let recipes = planks_and_milk();
        let mut menu = table_holding(ItemStack::one("minecraft:oak_log"), 0, &recipes);
        let mut inv = Hotbar::new();
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: None,
            recipes: Some(&recipes),
            second: None,
            env: crate::workstation::Env::default(),
        };
        let st = slots.menu.state_id;
        let mut c = click(7, st, 0, 2);
        c.input = ContainerInput::Swap;
        let _ = handle_click(&mut slots, &c);
        assert_eq!(inv.stack(2), ItemStack::new("minecraft:oak_planks", 4));
        assert!(menu.grid[0].is_empty());
    }

    /// `CraftingMenu.canTakeItemForPickAll` refuses the result slot: a
    /// double-click with planks on the cursor leaves the table's planks, and
    /// the log, where they are.
    #[test]
    fn a_gather_does_not_sweep_the_crafting_result() {
        let recipes = planks_and_milk();
        let mut menu = table_holding(ItemStack::one("minecraft:oak_log"), 0, &recipes);
        menu.carried = ItemStack::new("minecraft:oak_planks", 4);
        menu.remote_carried = menu.carried.clone();
        let mut inv = Hotbar::new();
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: None,
            recipes: Some(&recipes),
            second: None,
            env: crate::workstation::Env::default(),
        };
        let st = slots.menu.state_id;
        let mut c = click(7, st, CRAFTING_MENU_HOTBAR_0 as i32, 0);
        c.input = ContainerInput::PickupAll;
        let out = handle_click(&mut slots, &c);
        // Only the cursor, re-sent because the click's own `carried` claimed
        // an empty one: no slot moved.
        assert_eq!(
            out.replies,
            vec![Reply::SetCursor {
                stack: ItemStack::new("minecraft:oak_planks", 4)
            }]
        );
        assert_eq!(menu.result, ItemStack::new("minecraft:oak_planks", 4));
        assert_eq!(menu.grid[0], ItemStack::one("minecraft:oak_log"));
    }

    /// **`slots_changed_crafting_grid` refuses a menu with no grid**, called
    /// directly rather than through a click.
    ///
    /// Through a click this branch is unreachable: the recompute is only ever
    /// reached by a write to a crafting grid cell, and only a
    /// `MenuKind::Crafting` menu has any. The mutation sweep reported exactly that — deleting the
    /// guard changed nothing any scenario could see — which makes it an
    /// equivalent mutant *from the outside* and a live branch from the inside.
    ///
    /// It is pinned rather than deleted because the guard is what a container
    /// menu that later grows a grid would fall foul of, and because a chest
    /// reaching it would push a `SetSlot` for slot 0 and bump the state id:
    /// one extra packet on the chest path, which is the failure mode this
    /// whole item is most exposed to.
    #[test]
    fn a_menu_with_no_grid_computes_no_recipe() {
        let spec = chest_spec();
        let mut menu = Menu::new(5, spec);
        let mut inv = Hotbar::new();
        let mut be = BlockEntity::new(Kind::Chest);
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: Some(&mut be),
            recipes: None,
            second: None,
            env: crate::workstation::Env::default(),
        };
        let _ = send_all_data_to_remote(&mut slots);
        let before = slots.menu.state_id;
        assert_eq!(
            slots_changed_crafting_grid(&mut slots),
            None,
            "a chest has no grid and must not be handed a result slot"
        );
        assert_eq!(
            menu.state_id, before,
            "and it must not spend a state id doing nothing"
        );
    }

    /// Taking the log back out **clears** the result. A recipe engine that
    /// only ever wrote a result on a match would leave four planks in the
    /// output slot for ever, and no scenario in the corpus would notice.
    #[test]
    fn emptying_the_grid_clears_the_result_slot() {
        let spec = crafting_spec();
        let mut menu = Menu::new(7, spec);
        let mut inv = Hotbar::new();
        let recipes = crate::recipes::Recipes::parse(
            &serde_json::json!({
                "oak_planks": {
                    "type": "minecraft:crafting_shapeless",
                    "ingredients": ["#minecraft:oak_logs"],
                    "result": { "count": 4, "id": "minecraft:oak_planks" }
                }
            }),
            &serde_json::json!({ "oak_logs": { "values": ["minecraft:oak_log"] } }),
        )
        .expect("table");
        menu.grid[0] = ItemStack::one("minecraft:oak_log");
        menu.result = ItemStack::new("minecraft:oak_planks", 4);
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: None,
            recipes: Some(&recipes),
            second: None,
            env: crate::workstation::Env::default(),
        };
        let _ = send_all_data_to_remote(&mut slots);
        // Pick the log back out of the grid.
        let st = slots.menu.state_id;
        let out = handle_click(&mut slots, &click(7, st, 1, 0));
        assert!(menu.grid[0].is_empty());
        assert!(
            menu.result.is_empty(),
            "the result slot still holds {:?} after the grid was emptied",
            menu.result
        );
        assert!(
            out.replies.iter().any(|r| matches!(
                r,
                Reply::SetSlot { slot: 0, stack, .. } if stack.is_empty()
            )),
            "and the client was told slot 0 is now empty: {:?}",
            out.replies
        );
    }

    /// A container menu whose block entity has gone — a chest broken while
    /// open — reads empty and refuses to store, rather than panicking on a
    /// missing map entry.
    #[test]
    fn a_menu_whose_block_entity_vanished_stores_nothing() {
        let spec = chest_spec();
        let mut menu = Menu::new(5, spec);
        let mut inv = Hotbar::new();
        inv.set_stack(0, ItemStack::one("minecraft:stone"));
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: None,
            recipes: None,
            second: None,
            env: crate::workstation::Env::default(),
        };
        let _ = send_all_data_to_remote(&mut slots);
        let _ = handle_click(&mut slots, &click(5, 1, CHEST_MENU_HOTBAR_0 as i32, 0));
        let out = {
            let st = slots.menu.state_id;
            handle_click(&mut slots, &click(5, st, 0, 0))
        };
        // **Nothing was stored**, which is the claim. There is no block entity
        // to store into, and the first version of this code answered that by
        // splitting the stack off the cursor and dropping it — the item simply
        // ceased to exist. `may_place` refusing a container slot with no block
        // entity is what stops that, and this assertion is what would catch it
        // coming back.
        assert!(
            !out.replies
                .iter()
                .any(|r| matches!(r, Reply::SetSlot { .. })),
            "nothing was stored, so no slot was corrected: {:?}",
            out.replies
        );
        assert_eq!(
            menu.carried,
            ItemStack::one("minecraft:stone"),
            "the stone is still on the cursor rather than destroyed"
        );
    }

    /// The two still-unimplemented `ContainerInput` values decline rather
    /// than falling through to `PICKUP`. `ContainerInput::from_id` maps an
    /// unknown id to `PICKUP` because vanilla's own mapper does — those are
    /// different questions and conflating them would make a `SWAP` behave as a
    /// pickup.
    ///
    /// **`QUICK_MOVE` used to be in this list and is not any more, `THROW`
    /// left it after that, and `SWAP` has now left it too.** Removing a name
    /// is the visible half of the change that implements the arm: this test
    /// asserted that a `SWAP` click on `CHEST_MENU_HOTBAR_0` moved nothing,
    /// and it would now pass for the wrong reason — `CHEST_MENU_HOTBAR_0`
    /// and `button = 0` both name `Inventory` index 0, so the arm swaps that
    /// cell with itself and the assertions below cannot tell that from a
    /// decline. See [`a_swap_exchanges_two_different_slots`] for the real
    /// claim. Kept as the record that the two claims cannot both be made, and
    /// as the count that has to be decremented deliberately rather than by
    /// editing a list until the suite is green.
    #[test]
    fn the_other_inputs_are_declined_and_an_unknown_id_is_pickup() {
        assert_eq!(ContainerInput::from_id(0), ContainerInput::Pickup);
        assert_eq!(ContainerInput::from_id(1), ContainerInput::QuickMove);
        assert_eq!(ContainerInput::from_id(4), ContainerInput::Throw);
        assert_eq!(ContainerInput::from_id(6), ContainerInput::PickupAll);
        // Out of range decodes as PICKUP — `OutOfBoundsStrategy.ZERO`.
        assert_eq!(ContainerInput::from_id(99), ContainerInput::Pickup);
        assert_eq!(ContainerInput::from_id(-1), ContainerInput::Pickup);

        // `PickupAll` was in this list until it was implemented. It is out
        // rather than kept-and-passing on purpose: with an empty cursor the
        // arm takes its 1646 exit and moves nothing, so it would have gone on
        // asserting "declined" from a state in which the *implemented* verb
        // also does nothing — a test that had stopped being able to fail.
        for input in [ContainerInput::Clone, ContainerInput::QuickCraft] {
            let (mut menu, mut inv, mut be) = opened_chest();
            let mut slots = Slots {
                menu: &mut menu,
                inventory: &mut inv,
                container: Some(&mut be),
                recipes: None,
                second: None,
                env: crate::workstation::Env::default(),
            };
            let out = handle_click(
                &mut slots,
                &ClickPacket {
                    input,
                    ..click(5, 1, CHEST_MENU_HOTBAR_0 as i32, 0)
                },
            );
            assert!(out.replies.is_empty(), "{input:?} answered a packet");
            assert!(
                !inv.stack(0).is_empty(),
                "{input:?}: a declined input moves nothing rather than \
                 behaving as a pickup"
            );
            assert!(be.is_empty(), "{input:?}: and nothing reached the chest");
        }
    }

    // ------------------------------------------------------------------
    // SWAP — `doClick` offsets 1080-1382.
    //
    // The chest's own menu numbers hotbar index `b` at menu slot
    // `27 + 27 + b` = `54 + b` (`chest_spec().container_slots` is 27, and the
    // main-inventory rows before the hotbar are another 27 — see
    // [`slot_ref`]). `CHEST_MENU_HOTBAR_0` is `54`; `55` is hotbar index 1,
    // which is `container-click-swap-exchanges-a-hotbar-slot`'s own `button
    // = 1` and the `slot 55` its `about` field names.
    // ------------------------------------------------------------------

    /// `604`'s own claim: a hotbar key pressed over a chest slot exchanges
    /// the two stacks in one click, and vanilla corrects **both** — the
    /// chest slot first (menu index order), then the hotbar slot, each with
    /// its own state id bump.
    #[test]
    fn a_swap_exchanges_two_different_slots() {
        let (mut menu, mut inv, mut be) = opened_chest_over(
            &[(1, ItemStack::one("minecraft:dirt"))],
            &[(0, ItemStack::one("minecraft:stone"))],
        );
        let st = menu.state_id;
        let out = click_chest(
            &mut menu,
            &mut inv,
            &mut be,
            &ClickPacket {
                input: ContainerInput::Swap,
                ..click(5, st, 0, 1)
            },
        );
        assert_eq!(
            out.replies,
            vec![
                Reply::SetSlot {
                    container_id: 5,
                    state_id: 2,
                    slot: 0,
                    stack: ItemStack::one("minecraft:dirt"),
                },
                Reply::SetSlot {
                    container_id: 5,
                    state_id: 3,
                    slot: 55,
                    stack: ItemStack::one("minecraft:stone"),
                },
            ],
            "the chest slot is corrected before the hotbar slot, in menu \
             index order"
        );
        assert_eq!(be.item(0), ItemStack::one("minecraft:dirt"));
        assert_eq!(inv.stack(1), ItemStack::one("minecraft:stone"));
    }

    /// The hotbar slot is empty: the click moves the chest stack into it
    /// wholesale rather than declining for want of a cursor step. Offsets
    /// 1149-1202.
    #[test]
    fn a_swap_onto_an_empty_hotbar_slot_moves_the_stack_there() {
        let (mut menu, mut inv, mut be) =
            opened_chest_over(&[], &[(0, ItemStack::one("minecraft:stone"))]);
        let st = menu.state_id;
        let out = click_chest(
            &mut menu,
            &mut inv,
            &mut be,
            &ClickPacket {
                input: ContainerInput::Swap,
                ..click(5, st, 0, 1)
            },
        );
        assert!(be.item(0).is_empty());
        assert_eq!(inv.stack(1), ItemStack::one("minecraft:stone"));
        assert!(
            out.replies
                .iter()
                .any(|r| matches!(r, Reply::SetSlot { slot: 0, .. })),
            "the now-empty chest slot is corrected: {:?}",
            out.replies
        );
    }

    /// The chest slot is empty: the hotbar stack moves in. Offsets
    /// 1205-1273, the non-overflow half — the slot's maximum (64) is well
    /// above the single item this test moves.
    #[test]
    fn a_swap_into_an_empty_chest_slot_moves_the_hotbar_stack_in() {
        let (mut menu, mut inv, mut be) =
            opened_chest_over(&[(1, ItemStack::one("minecraft:dirt"))], &[]);
        let st = menu.state_id;
        let _ = click_chest(
            &mut menu,
            &mut inv,
            &mut be,
            &ClickPacket {
                input: ContainerInput::Swap,
                ..click(5, st, 0, 1)
            },
        );
        assert_eq!(be.item(0), ItemStack::one("minecraft:dirt"));
        assert!(inv.stack(1).is_empty());
    }

    /// Both empty: offsets 1130-1146 goto straight past every write. No
    /// state id moves and no packet answers a click that changed nothing.
    #[test]
    fn a_swap_between_two_empty_slots_is_silent() {
        let (mut menu, mut inv, mut be) = opened_chest_over(&[], &[]);
        let st = menu.state_id;
        let out = click_chest(
            &mut menu,
            &mut inv,
            &mut be,
            &ClickPacket {
                input: ContainerInput::Swap,
                ..click(5, st, 0, 1)
            },
        );
        assert_eq!(out.replies, Vec::new());
        assert_eq!(menu.state_id, st, "no state id was spent");
    }

    /// `button` outside `0..9` and not `40` is declined at offsets
    /// 1086-1099, same as a `button` naming the offhand — this server's
    /// `Inventory` has no armour or offhand storage to swap into (see
    /// `crate::player::Hotbar`'s doc), so `40` is declined rather than
    /// approximated. Both read identically on the wire: nothing moves and
    /// nothing is sent.
    #[test]
    fn a_swap_with_an_out_of_range_or_offhand_button_is_declined() {
        for button in [9_i8, 40, -1, 127] {
            let (mut menu, mut inv, mut be) = opened_chest_over(
                &[(1, ItemStack::one("minecraft:dirt"))],
                &[(0, ItemStack::one("minecraft:stone"))],
            );
            let st = menu.state_id;
            let out = click_chest(
                &mut menu,
                &mut inv,
                &mut be,
                &ClickPacket {
                    input: ContainerInput::Swap,
                    ..click(5, st, 0, button)
                },
            );
            assert_eq!(out.replies, Vec::new(), "button {button}");
            assert_eq!(be.item(0), ItemStack::one("minecraft:stone"));
            assert_eq!(inv.stack(1), ItemStack::one("minecraft:dirt"));
        }
    }

    /// The oversized two-way exchange — offsets 1305-1355, where the hotbar
    /// stack does not fit the slot even after splitting to its maximum — is
    /// declined rather than wired to `Inventory.add`/`Player.drop`. Named in
    /// [`do_click_swap`]'s own doc as a gap rather than left to look covered.
    ///
    /// [`crate::item::ItemStack::max_stack_size`] is one constant for every
    /// item this server knows — see its own doc — so the only way to build a
    /// stack this branch's `count() > max` sees is a count over 64, which is
    /// what `hold`'s `count = 1` cannot build and no scenario reaches.
    #[test]
    fn a_swap_the_slot_cannot_hold_whole_is_declined() {
        let (mut menu, mut inv, mut be) = opened_chest_over(
            &[(1, ItemStack::new("minecraft:dirt", 100))],
            &[(0, ItemStack::new("minecraft:stone", 1))],
        );
        let st = menu.state_id;
        let out = click_chest(
            &mut menu,
            &mut inv,
            &mut be,
            &ClickPacket {
                input: ContainerInput::Swap,
                ..click(5, st, 0, 1)
            },
        );
        assert_eq!(out.replies, Vec::new());
        assert_eq!(be.item(0), ItemStack::new("minecraft:stone", 1));
        assert_eq!(inv.stack(1), ItemStack::new("minecraft:dirt", 100));
    }

    // ------------------------------------------------------------------
    // PICKUP_ALL — `doClick` offsets 1611-1873.
    //
    // **The corpus can see that a gather happens and almost nothing else.**
    // `oracle-runner`'s `hold` sends `set_creative_mode_slot` with `count = 1`,
    // so every stack a scenario can build is a stack of one: the two-pass
    // structure, the cursor's maximum and the backward sweep are all
    // unreachable from `607`. They are measured here or nowhere, which is the
    // same division `do_click_quick_move`'s test block sets out above.
    // ------------------------------------------------------------------

    /// Cursor holding one stone, clicked slot empty, two matching donors: the
    /// sweep drains both. This is `607`'s assertion, in a unit test.
    #[test]
    fn a_gather_drains_every_matching_stack_onto_the_cursor() {
        let (mut menu, mut inv, mut be) = opened_chest_over(
            &[],
            &[
                (1, ItemStack::one("minecraft:stone")),
                (2, ItemStack::one("minecraft:stone")),
            ],
        );
        menu.carried = ItemStack::one("minecraft:stone");
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: Some(&mut be),
            recipes: None,
            second: None,
            env: crate::workstation::Env::default(),
        };
        // Chest slot 0 is empty — the state a real double-click produces.
        assert!(!do_click_pickup_all(&mut slots, 0, 0));
        assert_eq!(
            menu.carried,
            ItemStack::new("minecraft:stone", 3),
            "the cursor gathered both donors"
        );
        assert!(be.is_empty(), "and both donor slots were emptied");
    }

    /// Offsets 1657-1664: a clicked slot that still holds a takeable item
    /// leaves the arm having done nothing. This is the exit the old `607`
    /// took for its whole life.
    #[test]
    fn a_gather_declines_when_the_clicked_slot_still_holds_a_takeable_item() {
        let (mut menu, mut inv, mut be) = opened_chest_over(
            &[],
            &[
                (0, ItemStack::one("minecraft:stone")),
                (1, ItemStack::one("minecraft:stone")),
            ],
        );
        menu.carried = ItemStack::one("minecraft:stone");
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: Some(&mut be),
            recipes: None,
            second: None,
            env: crate::workstation::Env::default(),
        };
        assert!(!do_click_pickup_all(&mut slots, 0, 0));
        assert_eq!(
            menu.carried,
            ItemStack::one("minecraft:stone"),
            "the cursor was left alone"
        );
        assert_eq!(be.item(0), ItemStack::one("minecraft:stone"));
        assert_eq!(be.item(1), ItemStack::one("minecraft:stone"));
    }

    /// Offsets 1641-1646: nothing to gather onto.
    #[test]
    fn an_empty_cursor_gathers_nothing() {
        let (mut menu, mut inv, mut be) =
            opened_chest_over(&[], &[(1, ItemStack::one("minecraft:stone"))]);
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: Some(&mut be),
            recipes: None,
            second: None,
            env: crate::workstation::Env::default(),
        };
        assert!(!do_click_pickup_all(&mut slots, 0, 0));
        assert!(menu.carried.is_empty());
        assert_eq!(be.item(1), ItemStack::one("minecraft:stone"));
    }

    /// Offsets 1727-1737: the cursor stops at its own maximum, and what it
    /// could not take stays where it was.
    #[test]
    fn a_gather_stops_at_the_cursors_maximum() {
        let (mut menu, mut inv, mut be) = opened_chest_over(
            &[],
            &[
                (1, ItemStack::new("minecraft:stone", 40)),
                (2, ItemStack::new("minecraft:stone", 40)),
            ],
        );
        menu.carried = ItemStack::one("minecraft:stone");
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: Some(&mut be),
            recipes: None,
            second: None,
            env: crate::workstation::Env::default(),
        };
        assert!(!do_click_pickup_all(&mut slots, 0, 0));
        assert_eq!(
            menu.carried,
            ItemStack::new("minecraft:stone", 64),
            "the cursor filled to 64 and no further"
        );
        assert_eq!(
            be.item(1).count() + be.item(2).count(),
            17,
            "and the 17 it could not take are still in the chest"
        );
    }

    /// Offsets 1801-1819. **The load-bearing one for the two-pass structure.**
    /// A full stack and a partial one: the first pass must drain the partial
    /// and leave the full stack whole, so the cursor ends at 1 + 20 = 21 with
    /// the 64 untouched. A one-pass implementation takes the full stack first
    /// and ends at 64 with the partial left behind — a different menu.
    #[test]
    fn the_first_pass_leaves_a_full_stack_alone() {
        let (mut menu, mut inv, mut be) = opened_chest_over(
            &[],
            &[
                (1, ItemStack::new("minecraft:stone", 64)),
                (2, ItemStack::new("minecraft:stone", 20)),
            ],
        );
        menu.carried = ItemStack::one("minecraft:stone");
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: Some(&mut be),
            recipes: None,
            second: None,
            env: crate::workstation::Env::default(),
        };
        assert!(!do_click_pickup_all(&mut slots, 0, 0));
        assert_eq!(
            menu.carried,
            ItemStack::new("minecraft:stone", 64),
            "the cursor filled"
        );
        assert_eq!(
            be.item(2),
            ItemStack::empty(),
            "the partial stack went first"
        );
        assert_eq!(
            be.item(1),
            ItemStack::new("minecraft:stone", 21),
            "and only the remainder came out of the full one"
        );
    }

    /// Offsets 1667-1695: a non-zero button sweeps from the last slot
    /// backwards. Two donors, one near each end of the chest, and room on the
    /// cursor for only one of them — which one moves is the whole observable
    /// difference between the two directions.
    #[test]
    fn a_non_zero_button_sweeps_backwards() {
        for (button, expect_low_taken) in [(0i8, true), (1i8, false)] {
            let (mut menu, mut inv, mut be) = opened_chest_over(
                &[],
                &[
                    (1, ItemStack::new("minecraft:stone", 63)),
                    (26, ItemStack::new("minecraft:stone", 63)),
                ],
            );
            menu.carried = ItemStack::one("minecraft:stone");
            let mut slots = Slots {
                menu: &mut menu,
                inventory: &mut inv,
                container: Some(&mut be),
                recipes: None,
                second: None,
                env: crate::workstation::Env::default(),
            };
            assert!(!do_click_pickup_all(&mut slots, 0, button));
            assert_eq!(
                menu.carried,
                ItemStack::new("minecraft:stone", 64),
                "button {button}: the cursor filled either way"
            );
            let low_taken = be.item(1).count() < 63;
            assert_eq!(
                low_taken,
                expect_low_taken,
                "button {button}: the sweep started from the {} end",
                if expect_low_taken { "near" } else { "far" }
            );
        }
    }

    // ------------------------------------------------------------------
    // QUICK_MOVE — `doClick` offsets 664-741, and the loop at 708-738.
    //
    // **The corpus cannot tell a one-shot from the loop and never will.**
    // `tools/oracle-runner`'s `hold` verb sends `set_creative_mode_slot` with
    // `count = 1` (`client.rs`, the `Step::Hold` arm), so every stack any
    // gameplay scenario can quick-move is a stack of one, and one call always
    // finishes it. 601, 602 and 603 are therefore *necessary* and nowhere near
    // sufficient: they measure that the verb acts, in the right direction, and
    // stays silent on an empty slot. Nothing but a unit test measures that it
    // keeps going, which is why the multi-slot cases below are here rather
    // than as scenarios.
    //
    // FALSIFICATION, measured rather than asserted. Five mutations were
    // injected into the code above and the module re-run against each:
    //
    //   one_shot                  drop the loop at 708-738, one call only
    //                             -> 2 failed  (splits_an_oversized_stack,
    //                                out_of_a_chest_merges_then_fills_backwards)
    //   no_merge_phase            skip offsets 16-190 entirely
    //                             -> 4 failed
    //   merge_first_partial_only  break after the first slot merged into
    //                             -> 1 failed  (merges_into_every_partial…)
    //   not_reversed              container->player scans forwards
    //                             -> 2 failed  (retrieves_a_stored_stone…,
    //                                out_of_a_chest_merges_then_fills_backwards)
    //   phase2_fills_every_empty  delete the break at offset 308
    //                             -> 0 failed, and see `move_item_stack_to`
    //
    // Four of five caught, and the survivor is *provably* undetectable while
    // every slot shares one maximum stack size — recorded as a known limit of
    // this module rather than closed with a test that would pass for a reason
    // it did not state.
    // ------------------------------------------------------------------

    /// Open a chest menu over a given inventory and container.
    ///
    /// `inv_items` is indexed the way `Inventory` is — hotbar 0..9, main
    /// 9..36 — and `chest_items` by the block entity's own slot. Both are the
    /// storage indices, not menu indices, so a test that means "hotbar 0" says
    /// 0 and the menu-index conversion stays where `slot_ref` can be wrong
    /// about it.
    fn opened_chest_over(
        inv_items: &[(usize, ItemStack)],
        chest_items: &[(usize, ItemStack)],
    ) -> (Menu, Hotbar, BlockEntity) {
        let mut menu = Menu::new(5, chest_spec());
        let mut inv = Hotbar::new();
        for (i, s) in inv_items {
            inv.set_stack(*i, s.clone());
        }
        let mut be = BlockEntity::new(Kind::Chest);
        for (i, s) in chest_items {
            be.set_item(*i, s.clone());
        }
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: Some(&mut be),
            recipes: None,
            second: None,
            env: crate::workstation::Env::default(),
        };
        let opened = send_all_data_to_remote(&mut slots);
        assert!(matches!(opened, Reply::SetContent { state_id: 1, .. }));
        (menu, inv, be)
    }

    /// The gameplay corpus's `container_click` with `input = "quick_move"`:
    /// button 0, an empty prediction, and a state id that matches.
    fn quick_move(container_id: i32, state_id: i32, slot_num: i32) -> ClickPacket {
        ClickPacket {
            input: ContainerInput::QuickMove,
            ..click(container_id, state_id, slot_num, 0)
        }
    }

    fn click_chest(
        menu: &mut Menu,
        inv: &mut Hotbar,
        be: &mut BlockEntity,
        packet: &ClickPacket,
    ) -> Outcome {
        let mut slots = Slots {
            menu,
            inventory: inv,
            container: Some(be),
            recipes: None,
            second: None,
            env: crate::workstation::Env::default(),
        };
        handle_click(&mut slots, packet)
    }

    /// A stone carrying `minecraft:enchantments` — what 942 and 943 put in a
    /// chest beside plain ones.
    fn enchanted_stone() -> ItemStack {
        ItemStack::one("minecraft:stone").with_patch(crate::patch::Patch::default().with(
            crate::patch::Component::Enchantments(vec![("minecraft:mending".into(), 1)]),
        ))
    }

    /// 942 in miniature: `PICKUP_ALL`'s sweep takes a slot only through
    /// `canItemQuickReplace`, which is `isSameItemSameComponents`. The plain
    /// stone joins the cursor; the enchanted one stays where it is.
    #[test]
    fn pickup_all_leaves_a_differently_enchanted_stack_behind() {
        let (mut menu, mut inv, mut be) = opened_chest_over(
            &[],
            &[
                (1, ItemStack::one("minecraft:stone")),
                (2, enchanted_stone()),
            ],
        );
        menu.carried = ItemStack::one("minecraft:stone");
        let st = menu.state_id;
        let _ = click_chest(
            &mut menu,
            &mut inv,
            &mut be,
            &ClickPacket {
                input: ContainerInput::PickupAll,
                ..click(5, st, 0, 0)
            },
        );
        assert_eq!(menu.carried, ItemStack::new("minecraft:stone", 2));
        assert!(be.item(1).is_empty(), "the plain donor was gathered");
        assert_eq!(be.item(2), enchanted_stone(), "the enchanted one was not");
    }

    /// 943 in miniature: `moveItemStackTo`'s merge pass skips a slot whose
    /// stack differs in components, so the enchanted stone takes the first
    /// empty slot and slot 0 stays a single plain stone.
    #[test]
    fn quick_move_does_not_merge_into_a_differently_enchanted_stack() {
        let (mut menu, mut inv, mut be) = opened_chest_over(
            &[(2, enchanted_stone())],
            &[(0, ItemStack::one("minecraft:stone"))],
        );
        let st = menu.state_id;
        // Hotbar 2 is menu slot 56 in a 63-slot chest menu.
        let _ = click_chest(&mut menu, &mut inv, &mut be, &quick_move(5, st, 56));
        assert_eq!(be.item(0), ItemStack::one("minecraft:stone"));
        assert_eq!(
            be.item(1),
            enchanted_stone(),
            "patch and all, in the next slot"
        );
        assert!(inv.stack(2).is_empty());
    }

    /// 944 in miniature: a stack shift-clicked out of a chest keeps its patch
    /// in the inventory and in the `container_set_slot` that reports it.
    #[test]
    fn a_shift_clicked_enchanted_stack_keeps_its_patch() {
        let (mut menu, mut inv, mut be) = opened_chest_over(&[], &[(2, enchanted_stone())]);
        let st = menu.state_id;
        let out = click_chest(&mut menu, &mut inv, &mut be, &quick_move(5, st, 2));
        assert!(be.item(2).is_empty());
        let moved: Vec<&ItemStack> = out
            .replies
            .iter()
            .filter_map(|r| match r {
                Reply::SetSlot { stack, .. } if !stack.is_empty() => Some(stack),
                _ => None,
            })
            .collect();
        assert_eq!(
            moved,
            vec![&enchanted_stone()],
            "one correction, the stack whole"
        );
    }

    /// 948 in miniature: a patched `minecraft:max_stack_size` of 1 is the
    /// stack's maximum, so a second such stone clicked onto the first is not
    /// inserted (`safeInsert`'s room is 1 - 1) and stays on the cursor.
    #[test]
    fn a_patched_stack_size_of_one_does_not_merge() {
        let single = || {
            ItemStack::one("minecraft:stone").with_patch(
                crate::patch::Patch::default().with_raw("minecraft:max_stack_size", vec![1]),
            )
        };
        assert_eq!(single().max_stack_size(), 1);
        // With and without the item table: with it, stone's prototype says
        // 64, and the patch has to win over that.
        for props in [None, Some(props_bucket_16())] {
            let (mut menu, inv, mut be) = opened_chest_over(&[], &[(0, single())]);
            let mut inv = inv.with_item_props(props);
            menu.carried = single();
            let st = menu.state_id;
            let _ = click_chest(&mut menu, &mut inv, &mut be, &click(5, st, 0, 0));
            assert_eq!(be.item(0), single(), "still one in the slot");
            assert_eq!(menu.carried, single(), "and one on the cursor");
        }
    }

    /// The `container_set_slot`s an outcome carries, in wire order.
    fn corrections(out: &Outcome) -> Vec<(i16, String, i32)> {
        out.replies
            .iter()
            .filter_map(|r| match r {
                Reply::SetSlot { slot, stack, .. } => {
                    Some((*slot, stack.item().to_string(), stack.count()))
                }
                _ => None,
            })
            .collect()
    }

    /// The same, with the state id each correction carries.
    ///
    /// Separate from [`corrections`] and used only where a **vanilla
    /// recording** supplies the ground truth, because a state id asserted
    /// against nothing is a number this module chose and then agreed with.
    fn corrections_with_state(out: &Outcome) -> Vec<(i16, String, i32, i32)> {
        out.replies
            .iter()
            .filter_map(|r| match r {
                Reply::SetSlot {
                    slot,
                    stack,
                    state_id,
                    ..
                } => Some((*slot, stack.item().to_string(), stack.count(), *state_id)),
                _ => None,
            })
            .collect()
    }

    fn stone(n: i32) -> ItemStack {
        ItemStack::new("minecraft:stone", n)
    }

    /// **Scenario 601 as a unit test.** A stone in hotbar 0, one click, and it
    /// is in the chest — where `560` needs two.
    ///
    /// The assertion is on the two corrections, because those are what the
    /// check reads: `container_set_slot 0` carrying a stone is a claim only a
    /// server with a block entity can make, and `container_set_slot 54`
    /// carrying nothing is the hotbar being emptied in the same click. A
    /// server that answered one and not the other would be moving an item and
    /// not taking it, which is the shape a cursor-based transcription of this
    /// arm produces.
    ///
    /// **Slots and state ids are vanilla's own, read off a recording rather
    /// than derived here.** From `oracle-work/gameplay/vanilla.json`, this
    /// scenario's measured step: `container_set_slot slot 0 state 2` then
    /// `container_set_slot slot 54 state 3`, window 8, after a
    /// `container_set_content` at state 1. Two packets, index order, one state
    /// id each — which is what pins that neither the cursor packet nor a
    /// second correction sneaks into the sequence.
    #[test]
    fn quick_move_stores_a_hotbar_stone_in_the_first_chest_slot() {
        let (mut menu, mut inv, mut be) = opened_chest_over(&[(0, stone(1))], &[]);
        let st = menu.state_id;
        let out = click_chest(
            &mut menu,
            &mut inv,
            &mut be,
            &quick_move(5, st, CHEST_MENU_HOTBAR_0 as i32),
        );
        assert_eq!(
            corrections_with_state(&out),
            vec![
                (0, "minecraft:stone".to_string(), 1, 2),
                (54, String::new(), 0, 3),
            ],
            "vanilla's recording: slot 0 state 2, slot 54 state 3"
        );
        assert_eq!(be.item(0), stone(1));
        assert!(inv.stack(0).is_empty());
        // Offsets 664-741 never mention `carried`. A `SetCursor` here would be
        // this arm borrowing `PICKUP`'s machinery.
        assert!(
            !out.replies
                .iter()
                .any(|r| matches!(r, Reply::SetCursor { .. })),
            "the QUICK_MOVE arm does not touch the cursor"
        );
        assert!(menu.carried.is_empty());
    }

    /// **Scenario 603 as a unit test, and the slot number is the point.**
    ///
    /// `ChestMenu.quickMoveStack` passes `reverse = true` for the
    /// container-to-player direction (offsets 54-72), so `moveItemStackTo`
    /// starts at `slots.size() - 1` and counts *down*. The stone therefore
    /// lands in menu slot **62** — hotbar slot 8 — and not in menu slot 27,
    /// which is where a forward scan would put it and which is the first free
    /// slot by every intuition except vanilla's.
    ///
    /// A one-directional stub — the plausible one, since storing is the
    /// direction every earlier container check needed — moves nothing here at
    /// all.
    ///
    /// **Slot 62 is measured, not merely derived.** From a vanilla
    /// `oracle-work/gameplay/vanilla.json`, this scenario's four
    /// `container_set_slot`s across its three clicks are `slot 54 state 2`,
    /// `slot 0 state 3`, and then at the measured step `slot 0 state 4` and
    /// **`slot 62 state 5`**. All three clicks are replayed here rather than
    /// short-cutting to a pre-filled chest, so those state ids are comparable
    /// to the recording — a test that opened the chest with the stone already
    /// in it would agree about the slot and say nothing about the counter.
    #[test]
    fn quick_move_retrieves_a_stored_stone_into_the_last_hotbar_slot() {
        // Steps 1 and 2 are 560's two PICKUP clicks, which is how the scenario
        // gets the stone into the chest without using the verb under test.
        let (mut menu, mut inv, mut be) = opened_chest_over(&[(0, stone(1))], &[]);
        let st = menu.state_id;
        let take = click_chest(
            &mut menu,
            &mut inv,
            &mut be,
            &click(5, st, CHEST_MENU_HOTBAR_0 as i32, 0),
        );
        assert_eq!(
            corrections_with_state(&take),
            vec![(54, String::new(), 0, 2)],
            "vanilla: slot 54 state 2"
        );
        let put_packet = click(5, menu.state_id, 0, 0);
        let put = click_chest(&mut menu, &mut inv, &mut be, &put_packet);
        assert_eq!(
            corrections_with_state(&put),
            vec![(0, "minecraft:stone".to_string(), 1, 3)],
            "vanilla: slot 0 state 3"
        );

        // Step 3 — the measured click.
        let packet = quick_move(5, menu.state_id, 0);
        let out = click_chest(&mut menu, &mut inv, &mut be, &packet);
        assert_eq!(
            corrections_with_state(&out),
            vec![
                (0, String::new(), 0, 4),
                (62, "minecraft:stone".to_string(), 1, 5),
            ],
            "vanilla: slot 0 state 4, slot 62 state 5"
        );
        assert_eq!(
            inv.stack(8),
            stone(1),
            "hotbar 8, because the scan reverses"
        );
        assert!(
            inv.stack(9).is_empty(),
            "and not main-inventory slot 9, which menu slot 27 names"
        );
        assert!(be.item(0).is_empty());
    }

    /// **Scenario 602, both of its measured steps, in the order it plays
    /// them.** Step 2 must stay silent and step 3 must speak, and the whole
    /// value of that check is that one server cannot do both wrongly.
    ///
    /// The scenario places a chest fresh so its slot 0 is empty by
    /// construction; here the empty slot is empty for the same reason and the
    /// stone sits in hotbar 0 exactly as the file's third `setup` puts it.
    ///
    /// **Measured, and it was a prediction first.** Every vanilla recording
    /// that was on disk when this was written predates `e894224` and carries
    /// the two-step 602, so the third step had no reference anywhere and these
    /// state ids went in as this server's own claim, labelled as one. A subset
    /// recording of the three-step file then settled it, and vanilla agrees on
    /// both halves: its measured step contributes **nothing at all** to the
    /// projection, and the step after it answers `container_set_slot slot 0
    /// state 2` then `slot 54 state 3` — the silence spending no state id
    /// being exactly what makes the second pair start at 2 rather than at 4.
    #[test]
    fn quick_move_is_silent_on_an_empty_slot_and_not_on_the_next_one() {
        let (mut menu, mut inv, mut be) = opened_chest_over(&[(0, stone(1))], &[]);

        // Step 2 — `container_click slot 0`, a slot guaranteed empty.
        let st = menu.state_id;
        let step2 = click_chest(&mut menu, &mut inv, &mut be, &quick_move(5, st, 0));
        assert!(
            step2.replies.is_empty(),
            "an empty slot has nothing to move, so nothing is corrected: {:?}",
            step2.replies
        );
        assert_eq!(
            menu.state_id, st,
            "and no state id was spent, which is what makes the silence \
             indistinguishable from vanilla's rather than merely quiet"
        );

        // Step 3 — `container_click slot 54`, the positive control.
        let step3_packet = quick_move(5, menu.state_id, CHEST_MENU_HOTBAR_0 as i32);
        let step3 = click_chest(&mut menu, &mut inv, &mut be, &step3_packet);
        assert_eq!(
            corrections_with_state(&step3),
            vec![
                (0, "minecraft:stone".to_string(), 1, 2),
                (54, String::new(), 0, 3),
            ],
            "the same verb, on a slot that is not empty, has to speak — and \
             at states 2 and 3, because step 2's silence spent nothing"
        );
    }

    /// **The merge phase runs over every partial stack before any empty slot
    /// is used** — `moveItemStackTo` offsets 16-190, which is a loop over the
    /// whole range, and only then offsets 193-328.
    ///
    /// Two chest slots hold 60 stone each and twenty more arrive. Vanilla tops
    /// up both — 4 into each — and puts the remaining 12 in the first empty
    /// slot. Three implementations fail this and all three are plausible:
    ///
    /// * "place into the first empty slot" leaves both partials at 60 and puts
    ///   20 into slot 2;
    /// * "merge into the first partial, then place" leaves slot 1 at 60 and
    ///   puts 16 into slot 2;
    /// * "merge everywhere, never place" leaves 12 in the hotbar.
    #[test]
    fn quick_move_merges_into_every_partial_stack_before_it_uses_an_empty_one() {
        let (mut menu, mut inv, mut be) =
            opened_chest_over(&[(0, stone(20))], &[(0, stone(60)), (1, stone(60))]);
        let st = menu.state_id;
        let out = click_chest(
            &mut menu,
            &mut inv,
            &mut be,
            &quick_move(5, st, CHEST_MENU_HOTBAR_0 as i32),
        );
        assert_eq!(be.item(0), stone(64), "the first partial was topped up");
        assert_eq!(be.item(1), stone(64), "and so was the second");
        assert_eq!(
            be.item(2),
            stone(12),
            "the remainder went to the empty slot"
        );
        assert!(inv.stack(0).is_empty(), "and the hotbar slot is empty");
        assert_eq!(
            corrections(&out),
            vec![
                (0, "minecraft:stone".to_string(), 64),
                (1, "minecraft:stone".to_string(), 64),
                (2, "minecraft:stone".to_string(), 12),
                (54, String::new(), 0),
            ]
        );
        assert_eq!(
            be.item(0).count() + be.item(1).count() + be.item(2).count(),
            140,
            "60 + 60 + 20, and nothing was created or destroyed"
        );
    }

    /// **A destination that fills exactly takes the other branch**, and the
    /// difference is observable as a slot that is *not* written.
    ///
    /// Offsets 109-136 handle `existing + incoming <= max` by zeroing the
    /// source outright; offsets 139-174 handle the overflow by topping the
    /// slot up to `max`. Four stone onto sixty is the first branch, and after
    /// it the source is empty, so the placement phase at offset 193 is skipped
    /// entirely and chest slot 1 is never touched.
    ///
    /// An implementation that always runs the placement phase — or that
    /// treats "merged" as "partially merged" — writes an empty stack into slot
    /// 1 and emits a correction for it.
    #[test]
    fn quick_move_that_fills_a_stack_exactly_touches_no_empty_slot() {
        let (mut menu, mut inv, mut be) = opened_chest_over(&[(0, stone(4))], &[(0, stone(60))]);
        let st = menu.state_id;
        let out = click_chest(
            &mut menu,
            &mut inv,
            &mut be,
            &quick_move(5, st, CHEST_MENU_HOTBAR_0 as i32),
        );
        assert_eq!(be.item(0), stone(64));
        assert!(be.item(1).is_empty(), "the empty slot was never reached");
        assert!(inv.stack(0).is_empty());
        assert_eq!(
            corrections(&out),
            vec![
                (0, "minecraft:stone".to_string(), 64),
                (54, String::new(), 0),
            ],
            "exactly two slots changed"
        );
    }

    /// **THE LOOP, forwards.** A stack that needs three destination slots.
    ///
    /// `moveItemStackTo`'s placement phase fills **one** empty slot and
    /// returns (offset 308's `goto 328`). So 150 stone cannot leave the hotbar
    /// in one `quickMoveStack` call whatever else is true, and it takes the
    /// three calls `doClick`'s loop at 708-738 makes.
    ///
    /// **A one-shot implementation fails here and nowhere in the corpus.**
    /// Call `quickMoveStack` once and stop and the chest holds 64 in slot 0,
    /// nothing in slots 1 and 2, and 86 stone stays in the hotbar — which is
    /// "shift-click moves an item" instead of "shift-click empties the slot".
    #[test]
    fn quick_move_splits_an_oversized_stack_across_as_many_slots_as_it_needs() {
        let (mut menu, mut inv, mut be) = opened_chest_over(&[(0, stone(150))], &[]);
        let st = menu.state_id;
        let out = click_chest(
            &mut menu,
            &mut inv,
            &mut be,
            &quick_move(5, st, CHEST_MENU_HOTBAR_0 as i32),
        );
        assert_eq!(be.item(0), stone(64));
        assert_eq!(be.item(1), stone(64));
        assert_eq!(be.item(2), stone(22));
        assert!(
            inv.stack(0).is_empty(),
            "the source slot is emptied, not merely reduced"
        );
        assert_eq!(
            corrections(&out),
            vec![
                (0, "minecraft:stone".to_string(), 64),
                (1, "minecraft:stone".to_string(), 64),
                (2, "minecraft:stone".to_string(), 22),
                (54, String::new(), 0),
            ]
        );
    }

    /// **THE LOOP, backwards, with a partial in the way.** The same claim in
    /// the direction `603` clicks, where the scan runs from the last menu slot
    /// down.
    ///
    /// A hundred stone come out of the chest onto a hotbar whose slot 8
    /// already holds thirty. Vanilla tops slot 8 up to 64 first (34 of the
    /// hundred), places 64 in slot 7, and the second `quickMoveStack` call
    /// places the last 2 in slot 6. Three destination slots, in descending
    /// order, and the merge before any of the placements.
    ///
    /// A one-shot leaves 2 stone in the chest and slot 6 empty.
    #[test]
    fn quick_move_out_of_a_chest_merges_then_fills_backwards() {
        let (mut menu, mut inv, mut be) = opened_chest_over(&[(8, stone(30))], &[(0, stone(100))]);
        let st = menu.state_id;
        let out = click_chest(&mut menu, &mut inv, &mut be, &quick_move(5, st, 0));
        assert_eq!(inv.stack(8), stone(64), "hotbar 8 was topped up first");
        assert_eq!(inv.stack(7), stone(64), "then hotbar 7 was filled");
        assert_eq!(inv.stack(6), stone(2), "and the remainder went to hotbar 6");
        assert!(be.item(0).is_empty(), "the chest slot is empty");
        assert_eq!(
            corrections(&out),
            vec![
                (0, String::new(), 0),
                (60, "minecraft:stone".to_string(), 2),
                (61, "minecraft:stone".to_string(), 64),
                (62, "minecraft:stone".to_string(), 64),
            ],
            "corrections go out in menu-slot order, whatever order the moves \
             happened in"
        );
        assert_eq!(
            inv.stack(6).count() + inv.stack(7).count() + inv.stack(8).count(),
            130,
            "30 + 100"
        );
    }

    /// **A destination with no room at all is answered with silence, and the
    /// loop stops.**
    ///
    /// `moveItemStackTo` returns `false`, `quickMoveStack` returns `EMPTY`
    /// (offsets 78-81), and `doClick`'s first exit at 708-713 fires without a
    /// second call. The source keeps its whole stack.
    ///
    /// This is the case that makes the loop's termination argument load
    /// bearing rather than decorative: without the `EMPTY` return the
    /// condition at 716-726 would be true forever, because the slot still
    /// holds the item it started with.
    #[test]
    fn quick_move_into_a_full_container_changes_nothing_and_terminates() {
        let full: Vec<(usize, ItemStack)> = (0..27)
            .map(|i| (i, ItemStack::new("minecraft:dirt", 64)))
            .collect();
        let (mut menu, mut inv, mut be) = opened_chest_over(&[(0, stone(5))], &full);
        let st = menu.state_id;
        let out = click_chest(
            &mut menu,
            &mut inv,
            &mut be,
            &quick_move(5, st, CHEST_MENU_HOTBAR_0 as i32),
        );
        assert!(out.replies.is_empty());
        assert_eq!(inv.stack(0), stone(5), "the stack stayed where it was");
        assert_eq!(menu.state_id, st, "and no state id was spent");
    }

    /// **A partially full destination takes what fits and hands the rest
    /// back** — the loop stops on its second exit rather than its first.
    ///
    /// Twenty-six chest slots hold dirt and slot 26 holds 60 stone. Ten stone
    /// arrive: 4 merge, 6 have nowhere to go. The second `quickMoveStack` call
    /// moves nothing and returns `EMPTY`, so the loop ends with 6 stone still
    /// in the hotbar — a quick move that is *supposed* to leave the source
    /// non-empty, which is the case a "loop until the slot is empty"
    /// implementation hangs on.
    #[test]
    fn quick_move_leaves_behind_what_does_not_fit() {
        let mut chest: Vec<(usize, ItemStack)> = (0..26)
            .map(|i| (i, ItemStack::new("minecraft:dirt", 64)))
            .collect();
        chest.push((26, stone(60)));
        let (mut menu, mut inv, mut be) = opened_chest_over(&[(0, stone(10))], &chest);
        let st = menu.state_id;
        let out = click_chest(
            &mut menu,
            &mut inv,
            &mut be,
            &quick_move(5, st, CHEST_MENU_HOTBAR_0 as i32),
        );
        assert_eq!(be.item(26), stone(64));
        assert_eq!(inv.stack(0), stone(6), "six had nowhere to go");
        assert_eq!(
            corrections(&out),
            vec![
                (26, "minecraft:stone".to_string(), 64),
                (54, "minecraft:stone".to_string(), 6),
            ]
        );
    }

    /// **A menu whose `quickMoveStack` has not been transcribed declines.**
    ///
    /// `quickMoveStack` is abstract, and a menu whose override is not
    /// transcribed must not borrow `ChestMenu`'s — see [`QuickMove`].
    ///
    /// Checked by clicking, not by reading the field: a click on a live menu
    /// is the only thing that shows the decline reaching the wire.
    #[test]
    fn a_menu_with_no_transcribed_rule_declines_the_click() {
        // A furnace: three container slots, so hotbar 0 is menu slot 30.
        // The furnace's rule is transcribed (`furnace_tests`), and so is the
        // crafting table's; a spec with the rule taken away stands in for a
        // menu that has none.
        let mut furnace_spec = menu_for_state("minecraft:furnace").expect("furnace");
        furnace_spec.quick_move = QuickMove::NotImplemented;
        let mut menu = Menu::new(8, furnace_spec);
        let mut inv = Hotbar::new();
        inv.set_stack(0, stone(1));
        let mut be = BlockEntity::new(Kind::Furnace);
        be.set_item(0, ItemStack::one("minecraft:oak_log"));
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: Some(&mut be),
            recipes: None,
            second: None,
            env: crate::workstation::Env::default(),
        };
        let _ = send_all_data_to_remote(&mut slots);
        let st = slots.menu.state_id;
        for slot in [0, 3 + 27] {
            let out = handle_click(&mut slots, &quick_move(8, st, slot));
            assert!(
                out.replies.is_empty(),
                "furnace slot {slot} answered a packet"
            );
        }
        assert_eq!(be.item(0), ItemStack::one("minecraft:oak_log"));
        assert_eq!(inv.stack(0), stone(1));
    }

    /// A `QUICK_MOVE` naming `SLOT_CLICKED_OUTSIDE` or any other negative slot
    /// returns at offsets 670-674, before `slots.get`. It does **not** drop
    /// the cursor the way `PICKUP` does — the arm never mentions the cursor at
    /// all — and this is the only place the two verbs' handling of -999
    /// differs.
    #[test]
    fn quick_move_outside_the_window_drops_nothing() {
        let (mut menu, mut inv, mut be) = opened_chest_over(&[(0, stone(1))], &[]);
        menu.carried = stone(3);
        let st = menu.state_id;
        let out = click_chest(&mut menu, &mut inv, &mut be, &quick_move(5, st, -999));
        assert_eq!(
            menu.carried,
            stone(3),
            "PICKUP drops the cursor into the world here; QUICK_MOVE does not"
        );
        assert_eq!(inv.stack(0), stone(1));
        assert!(
            out.replies
                .iter()
                .all(|r| !matches!(r, Reply::SetSlot { .. })),
            "and no slot was corrected"
        );
    }

    /// A button that is neither 0 nor 1 is refused at offset 575 before
    /// anything happens.
    #[test]
    fn a_button_outside_zero_and_one_is_refused() {
        let (mut menu, mut inv, mut be) = opened_chest();
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: Some(&mut be),
            recipes: None,
            second: None,
            env: crate::workstation::Env::default(),
        };
        let out = handle_click(&mut slots, &click(5, 1, CHEST_MENU_HOTBAR_0 as i32, 4));
        assert!(out.replies.is_empty());
        assert!(!inv.stack(0).is_empty());
    }

    /// The same guard, the same instruction, for `QUICK_MOVE`.
    ///
    /// Offsets 561-572 enter one arm for either input and 575-581 sit inside
    /// it, above the re-test at 664 that separates them, so vanilla drops a
    /// shift-click whose button is out of range without reading a slot. This
    /// server dispatched `QUICK_MOVE` above the guard, and
    /// [`do_click_quick_move`] does not take `button` at all, so there was
    /// nothing below to catch it either.
    ///
    /// **The control at the end is not decoration.** The refusals alone would
    /// pass just as well against a server whose `QUICK_MOVE` moves nothing,
    /// which is what this one was before `602` grew its third step — the same
    /// trap, in the same arm, one layer down. So the last block runs the
    /// identical click with the one button that is inside the guard and
    /// requires the stone to leave the hotbar.
    #[test]
    fn a_quick_move_with_a_button_outside_zero_and_one_is_refused() {
        for button in [2_i8, 3, 40, -1, 127] {
            let (mut menu, mut inv, mut be) = opened_chest_over(&[(0, stone(1))], &[]);
            let st = menu.state_id;
            let out = click_chest(
                &mut menu,
                &mut inv,
                &mut be,
                &ClickPacket {
                    input: ContainerInput::QuickMove,
                    ..click(5, st, CHEST_MENU_HOTBAR_0 as i32, button)
                },
            );
            assert_eq!(corrections(&out), Vec::new(), "button {button} answered");
            assert_eq!(inv.stack(0), stone(1), "button {button} moved the stone");
            assert!(be.item(0).is_empty(), "button {button} filled the chest");
        }

        let (mut menu, mut inv, mut be) = opened_chest_over(&[(0, stone(1))], &[]);
        let st = menu.state_id;
        let out = click_chest(
            &mut menu,
            &mut inv,
            &mut be,
            &quick_move(5, st, CHEST_MENU_HOTBAR_0 as i32),
        );
        assert!(
            !corrections(&out).is_empty(),
            "button 0 must still move it, or the five refusals above prove nothing"
        );
        assert_eq!(be.item(0), stone(1), "button 0 stored the stone");
        assert!(inv.stack(0).is_empty(), "button 0 emptied the hotbar slot");
    }

    // ------------------------------------------------------------------
    // THROW — `doClick` offsets 1449-1608.
    //
    // The corpus pins the branch and nothing else. `605` and `606` are the
    // same rig with `button = 0` and `button = 1` over a chest slot holding
    // two stone, and between them they catch a server that hardcodes either
    // answer. What they cannot see, and what the tests below are for:
    //
    //   * the cursor guard at 1456 — no scenario clicks THROW with a stack on
    //     the cursor, and a server that threw the *carried* stack would look
    //     identical on both of them;
    //   * `n` being read once, before the first take, rather than re-read
    //     inside the loop;
    //   * the loop existing at all, which needs a slot whose `safeTake`
    //     cannot satisfy `n` in one call;
    //   * `SLOT_CLICKED_OUTSIDE`, which is a legal slot number for PICKUP and
    //     is silence here.
    // ------------------------------------------------------------------

    /// A `THROW` click with `button = 0` over a chest slot takes exactly one.
    ///
    /// This is `605`'s branch. Offsets 1483-1488: `button == 0` loads
    /// `iconst_1` and skips the `getCount()`, so a stack of two loses one and
    /// the client is corrected to the remainder.
    #[test]
    fn a_primary_throw_takes_one_item_and_leaves_the_rest() {
        let (mut menu, mut inv, mut be) =
            opened_chest_over(&[], &[(0, ItemStack::new("minecraft:stone", 2))]);
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: Some(&mut be),
            recipes: None,
            second: None,
            env: crate::workstation::Env::default(),
        };
        let st = slots.menu.state_id;
        let out = handle_click(
            &mut slots,
            &ClickPacket {
                input: ContainerInput::Throw,
                ..click(5, st, 0, 0)
            },
        );
        assert_eq!(
            out.dropped,
            vec![ItemStack::one("minecraft:stone")],
            "one item entity, of one stone"
        );
        assert_eq!(
            be.item(0),
            ItemStack::one("minecraft:stone"),
            "and one stone is still in the chest"
        );
        assert!(
            out.replies.iter().any(|r| matches!(
                r,
                Reply::SetSlot { slot: 0, stack, .. }
                    if *stack == ItemStack::one("minecraft:stone")
            )),
            "the client is corrected to the remainder: {:?}",
            out.replies
        );
    }

    /// A `THROW` click with `button = 1` empties the slot in **one** drop.
    ///
    /// This is `606`'s branch, and the assertion that matters is the *count*
    /// of dropped stacks. Offsets 1491-1499 read `slot.getItem().getCount()`
    /// once, so `safeTake(2, MAX)` takes both stone at once and vanilla
    /// spawns a single `ItemEntity` carrying two — not two entities of one.
    /// A transcription that looped `safeTake(1, MAX)` would leave the same
    /// inventory behind and put an extra `add_entity` on the wire.
    #[test]
    fn a_secondary_throw_empties_the_slot_in_one_item_entity() {
        let (mut menu, mut inv, mut be) =
            opened_chest_over(&[], &[(0, ItemStack::new("minecraft:stone", 2))]);
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: Some(&mut be),
            recipes: None,
            second: None,
            env: crate::workstation::Env::default(),
        };
        let st = slots.menu.state_id;
        let out = handle_click(
            &mut slots,
            &ClickPacket {
                input: ContainerInput::Throw,
                ..click(5, st, 0, 1)
            },
        );
        assert_eq!(
            out.dropped,
            vec![ItemStack::new("minecraft:stone", 2)],
            "one item entity, of two stone"
        );
        assert!(be.item(0).is_empty(), "the chest slot is empty");
        assert!(
            out.replies.iter().any(|r| matches!(
                r,
                Reply::SetSlot { slot: 0, stack, .. } if stack.is_empty()
            )),
            "and the client is told so: {:?}",
            out.replies
        );
    }

    /// **A `THROW` with something on the cursor does nothing at all.**
    ///
    /// Offsets 1456-1463: `if (!getCarried().isEmpty()) goto 1611`, which is
    /// the `PICKUP_ALL` test and then the end of the method. Vanilla does not
    /// throw the carried stack and does not throw the slot either.
    ///
    /// No scenario in the corpus can see this — every `container_click` the
    /// harness sends for a throw arrives with an empty cursor — so a server
    /// that threw the slot regardless would pass `605` and `606` both. That
    /// is exactly the case defect class #19 is about.
    #[test]
    fn a_throw_with_a_full_cursor_is_silence() {
        let (mut menu, mut inv, mut be) =
            opened_chest_over(&[], &[(0, ItemStack::new("minecraft:stone", 2))]);
        menu.carried = ItemStack::one("minecraft:stick");
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: Some(&mut be),
            recipes: None,
            second: None,
            env: crate::workstation::Env::default(),
        };
        let st = slots.menu.state_id;
        let out = handle_click(
            &mut slots,
            &ClickPacket {
                input: ContainerInput::Throw,
                ..click(5, st, 0, 0)
            },
        );
        assert!(out.dropped.is_empty(), "nothing was thrown");
        assert_eq!(
            be.item(0),
            ItemStack::new("minecraft:stone", 2),
            "and the slot is untouched"
        );
        assert_eq!(
            menu.carried,
            ItemStack::one("minecraft:stick"),
            "and so is the cursor"
        );
    }

    /// `SLOT_CLICKED_OUTSIDE` is silence, not a drop.
    ///
    /// -999 is a legal slot number that `isValidSlotIndex` accepts — it has
    /// to be, or `PICKUP`'s drop-into-the-world branch would be unreachable —
    /// and offsets 1466-1467 then reject it as `slotNum < 0`. So Q over the
    /// void throws nothing, and an empty slot throws nothing either:
    /// `safeTake` returns `EMPTY` and `LivingEntity.drop` builds no entity
    /// for it.
    #[test]
    fn a_throw_at_nothing_drops_nothing() {
        for slot in [SLOT_CLICKED_OUTSIDE, 1] {
            let (mut menu, mut inv, mut be) =
                opened_chest_over(&[], &[(0, ItemStack::new("minecraft:stone", 2))]);
            let mut slots = Slots {
                menu: &mut menu,
                inventory: &mut inv,
                container: Some(&mut be),
                recipes: None,
                second: None,
                env: crate::workstation::Env::default(),
            };
            let st = slots.menu.state_id;
            let out = handle_click(
                &mut slots,
                &ClickPacket {
                    input: ContainerInput::Throw,
                    ..click(5, st, slot, 1)
                },
            );
            assert!(out.dropped.is_empty(), "slot {slot} threw something");
            assert_eq!(
                be.item(0),
                ItemStack::new("minecraft:stone", 2),
                "slot {slot} moved the stack"
            );
        }
    }

    /// **The loop at 1545-1605 is real, and this is the only thing that can
    /// see it.**
    ///
    /// One `safeTake` cannot always satisfy `n`: `Slot.tryRemove` takes
    /// `min(count, maxTake)` out of the slot, and a slot holding more than
    /// its own stack maximum hands over one maximum at a time. So a
    /// `button = 1` throw of an over-full slot is several `drop` calls with
    /// the same `n`, and a one-shot transcription would leave the rest of the
    /// stack in the chest.
    ///
    /// The rig is a stack of 5 in a slot whose maximum is 2 — an egg, whose
    /// `max_stack_size` is 16 in vanilla, is not extreme enough to force it
    /// through `crate::item::DEFAULT_MAX_STACK_SIZE`, so the case is built by
    /// hand out of a component-carrying stack instead. If no such stack can
    /// be built, this asserts the shape it *would* take rather than passing
    /// silently: see the fallback branch.
    #[test]
    fn a_secondary_throw_keeps_taking_until_the_slot_is_empty() {
        // `n` is read once and is the slot's whole count, so the loop can
        // only run when a single take is capped below it. With every stack
        // maximum equal — which is this server's model today, see
        // `Slots::max_stack_size` — `safeTake(n, MAX)` always empties the
        // slot in one call and the loop's second iteration is unreachable.
        //
        // That is asserted rather than left implicit: the claim is "one call
        // per throw **because** nothing caps it", and the day a per-slot
        // maximum arrives this test fails and the loop starts earning its
        // place.
        let (mut menu, mut inv, mut be) =
            opened_chest_over(&[], &[(0, ItemStack::new("minecraft:stone", 5))]);
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: Some(&mut be),
            recipes: None,
            second: None,
            env: crate::workstation::Env::default(),
        };
        let st = slots.menu.state_id;
        let out = handle_click(
            &mut slots,
            &ClickPacket {
                input: ContainerInput::Throw,
                ..click(5, st, 0, 1)
            },
        );
        assert!(be.item(0).is_empty(), "the slot is empty either way");
        assert_eq!(
            out.dropped.iter().map(ItemStack::count).sum::<i32>(),
            5,
            "and every one of the five reached an item entity: {:?}",
            out.dropped
        );
        assert_eq!(
            out.dropped.len(),
            1,
            "with one stack maximum for every slot, one take empties it — \
             if this now fails, `max_stack_size` has gained a per-slot \
             answer and the loop at 1545-1605 is finally reachable: {:?}",
            out.dropped
        );
    }

    /// A `THROW` on the player's own half of the menu works the same way.
    ///
    /// `doClick`'s arm reads `slots.get(slotIndex)` and knows nothing about
    /// which container is behind it, so hotbar slot 0 — menu slot 54 in a
    /// three-row chest — throws exactly as a chest slot does. Written because
    /// [`slot_ref`] is the single most likely place in this module for an
    /// off-by-27 and because no scenario throws from the inventory half.
    #[test]
    fn a_throw_from_the_players_own_slots_drops_too() {
        let (mut menu, mut inv, mut be) =
            opened_chest_over(&[(0, ItemStack::new("minecraft:stone", 3))], &[]);
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: Some(&mut be),
            recipes: None,
            second: None,
            env: crate::workstation::Env::default(),
        };
        let st = slots.menu.state_id;
        let out = handle_click(
            &mut slots,
            &ClickPacket {
                input: ContainerInput::Throw,
                ..click(5, st, CHEST_MENU_HOTBAR_0 as i32, 0)
            },
        );
        assert_eq!(out.dropped, vec![ItemStack::one("minecraft:stone")]);
        assert_eq!(
            inv.stack(0),
            ItemStack::new("minecraft:stone", 2),
            "one off the hotbar stack, and out of the hotbar, not the chest"
        );
        assert!(be.is_empty(), "nothing reached the chest");
    }

    /// A synthetic props table: `air` 64, `bucket` 16.
    ///
    /// Leaked because [`Hotbar::with_item_props`] takes a `&'static` —
    /// production's is a `OnceLock` and lives for the process.
    fn props_bucket_16() -> &'static crate::itemprops::ItemProps {
        Box::leak(Box::new(
            crate::itemprops::ItemProps::parse(
                &serde_json::from_str(
                    r#"{"table":[
                        {"item":"minecraft:air","components":{"minecraft:max_stack_size":64}},
                        {"item":"minecraft:stone","components":{"minecraft:max_stack_size":64}},
                        {"item":"minecraft:bucket","components":{"minecraft:max_stack_size":16}}
                    ]}"#,
                )
                .expect("test json"),
            )
            .expect("parse"),
        ))
    }

    /// **`safeInsert` caps at the STACK's maximum, and until the item-props
    /// table existed it capped at 64 for everything.**
    ///
    /// Written as a pair over the same insert, because the claim is the
    /// difference: with no table a stack of 24 buckets goes wholly into one
    /// slot, and with the table 16 go in and 8 come back. A single assertion
    /// of 16 would pass against an implementation that had hard-coded it.
    #[test]
    fn a_container_slot_caps_a_bucket_at_sixteen_only_with_the_table() {
        for (props, want_in, want_left) in [(None, 24, 0), (Some(props_bucket_16()), 16, 8)] {
            let mut menu = Menu::new(7, chest_spec());
            let mut inv = Hotbar::new().with_item_props(props);
            let mut be = BlockEntity::new(Kind::Chest);
            let mut slots = Slots {
                menu: &mut menu,
                inventory: &mut inv,
                container: Some(&mut be),
                recipes: None,
                second: None,
                env: crate::workstation::Env::default(),
            };
            let left = slots.safe_insert(0, ItemStack::new("minecraft:bucket", 24), 24);
            assert_eq!(
                slots.get(0).count(),
                want_in,
                "props present: {}",
                props.is_some()
            );
            assert_eq!(
                left.count(),
                want_left,
                "props present: {}",
                props.is_some()
            );
        }
    }

    /// The container's own 64 is the other half of `Math.min` and still
    /// binds: a stack that stacks to 64 is capped at 64 either way, and an
    /// **empty** stack answers 64 rather than 1 because
    /// `ItemStack.EMPTY.getMaxStackSize()` is `Items.AIR`'s.
    #[test]
    fn the_containers_own_maximum_still_binds_and_empty_is_air() {
        let mut menu = Menu::new(7, chest_spec());
        let mut inv = Hotbar::new().with_item_props(Some(props_bucket_16()));
        let mut be = BlockEntity::new(Kind::Chest);
        let slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: Some(&mut be),
            recipes: None,
            second: None,
            env: crate::workstation::Env::default(),
        };
        assert_eq!(
            slots.max_stack_size(0, &ItemStack::new("minecraft:stone", 1)),
            64
        );
        assert_eq!(slots.max_stack_size(0, &ItemStack::empty()), 64);
        assert_eq!(
            slots.max_stack_size(0, &ItemStack::new("minecraft:bucket", 1)),
            16
        );
    }
}

/// The furnace and brewing-stand menus: their slot classes, their
/// `quickMoveStack`s, their data slots and the clock thread's sweep.
#[cfg(test)]
mod furnace_tests {
    use super::*;
    use crate::blockentity::Kind;
    use crate::menu::menu_for_state;

    /// A furnace's hotbar 0 — three container slots, then 27, then the bar.
    const FURNACE_HOTBAR_0: i32 = 30;
    /// A brewing stand's hotbar 0 — five container slots, then 27.
    const BREWING_HOTBAR_0: i32 = 32;

    fn recipes() -> crate::recipes::Recipes {
        crate::recipes::Recipes::parse(
            &serde_json::json!({
                "iron_ingot_from_smelting_raw_iron": {
                    "type": "minecraft:smelting",
                    "experience": 0.7,
                    "ingredient": "minecraft:raw_iron",
                    "result": {"id": "minecraft:iron_ingot"}
                }
            }),
            &serde_json::json!({
                "brewing_fuel": {"values": ["minecraft:blaze_powder"]}
            }),
        )
        .expect("recipes parse")
    }

    fn click(
        container_id: i32,
        state_id: i32,
        slot_num: i32,
        input: ContainerInput,
    ) -> ClickPacket {
        ClickPacket {
            container_id,
            state_id,
            slot_num,
            button: 0,
            input,
            changed_slots: Vec::new(),
            hashed_slots: Vec::new(),
            carried: ItemStack::empty(),
        }
    }

    fn sets(out: &[Reply]) -> Vec<(i16, String, i32)> {
        out.iter()
            .filter_map(|r| match r {
                Reply::SetSlot { slot, stack, .. } => {
                    Some((*slot, stack.item().to_string(), stack.count()))
                }
                _ => None,
            })
            .collect()
    }

    fn data(out: &[Reply]) -> Vec<(i16, i32)> {
        out.iter()
            .filter_map(|r| match r {
                Reply::SetData { id, value, .. } => Some((*id, *value)),
                _ => None,
            })
            .collect()
    }

    /// Open `state`'s menu over `be` with `held` in hotbar 0, and return the
    /// menu, the inventory and the opening data packets.
    fn open(
        state: &str,
        be: &mut BlockEntity,
        held: &str,
        recipes: Option<&crate::recipes::Recipes>,
    ) -> (Menu, Hotbar, Vec<Reply>) {
        let mut menu = Menu::new(9, menu_for_state(state).expect("menu"));
        let mut inv = Hotbar::new();
        inv.set_stack(0, ItemStack::one(held));
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: Some(be),
            recipes,
            second: None,
            env: crate::workstation::Env::default(),
        };
        let _ = send_all_data_to_remote(&mut slots);
        let opening = full_state_data(&mut slots);
        (menu, inv, opening)
    }

    fn run(
        menu: &mut Menu,
        inv: &mut Hotbar,
        be: &mut BlockEntity,
        recipes: Option<&crate::recipes::Recipes>,
        slot: i32,
        input: ContainerInput,
    ) -> Vec<Reply> {
        let st = menu.state_id;
        let mut slots = Slots {
            menu,
            inventory: inv,
            container: Some(be),
            recipes,
            second: None,
            env: crate::workstation::Env::default(),
        };
        handle_click(&mut slots, &click(9, st, slot, input)).replies
    }

    /// `sendInitialData`'s tail: a cold furnace's four data slots are sent,
    /// as zeros, and a brewing stand's two.
    #[test]
    fn opening_sends_every_data_slot_once() {
        let mut furnace = BlockEntity::new(Kind::Furnace);
        let (_, _, opening) = open("minecraft:furnace", &mut furnace, "minecraft:stick", None);
        assert_eq!(data(&opening), vec![(0, 0), (1, 0), (2, 0), (3, 0)]);
        let mut stand = BlockEntity::new(Kind::BrewingStand);
        let (_, _, opening) = open(
            "minecraft:brewing_stand",
            &mut stand,
            "minecraft:stick",
            None,
        );
        assert_eq!(data(&opening), vec![(0, 0), (1, 0)]);
    }

    /// **Scenario 830.** Raw iron into slot 0 goes through
    /// `AbstractFurnaceBlockEntity.setItem`, which sets the cook total, and
    /// the click's `broadcastChanges` sends it as data slot 3 after the slot.
    #[test]
    fn raw_iron_into_the_ingredient_slot_sends_a_cook_total_of_200() {
        let r = recipes();
        let mut be = BlockEntity::new(Kind::Furnace);
        let (mut menu, mut inv, _) =
            open("minecraft:furnace", &mut be, "minecraft:raw_iron", Some(&r));
        let _ = run(
            &mut menu,
            &mut inv,
            &mut be,
            Some(&r),
            FURNACE_HOTBAR_0,
            ContainerInput::Pickup,
        );
        let out = run(
            &mut menu,
            &mut inv,
            &mut be,
            Some(&r),
            0,
            ContainerInput::Pickup,
        );
        assert_eq!(sets(&out), vec![(0, "minecraft:raw_iron".to_string(), 1)]);
        assert_eq!(data(&out), vec![(3, 200)]);
        // The negative control: the same click with no recipe table writes
        // the slot through the plain path and moves no data slot.
        let mut be = BlockEntity::new(Kind::Furnace);
        let (mut menu, mut inv, _) = open("minecraft:furnace", &mut be, "minecraft:raw_iron", None);
        let _ = run(
            &mut menu,
            &mut inv,
            &mut be,
            None,
            FURNACE_HOTBAR_0,
            ContainerInput::Pickup,
        );
        let out = run(
            &mut menu,
            &mut inv,
            &mut be,
            None,
            0,
            ContainerInput::Pickup,
        );
        assert_eq!(data(&out), vec![], "no table, no total — and nothing sent");
    }

    /// **Scenarios 836 and 837.** Stone is refused by the fuel slot and coal
    /// by the result slot; neither click moves a container slot. The fuel
    /// table is not loaded in a unit test, which is the refusal direction
    /// for coal — so the result-slot half uses coal precisely because the
    /// fuel slot would have refused it for a different reason.
    #[test]
    fn the_fuel_and_result_slots_refuse_what_they_do_not_take() {
        for (held, slot) in [("minecraft:stone", 1), ("minecraft:coal", 2)] {
            let mut be = BlockEntity::new(Kind::Furnace);
            let (mut menu, mut inv, _) = open("minecraft:furnace", &mut be, held, None);
            let _ = run(
                &mut menu,
                &mut inv,
                &mut be,
                None,
                FURNACE_HOTBAR_0,
                ContainerInput::Pickup,
            );
            let out = run(
                &mut menu,
                &mut inv,
                &mut be,
                None,
                slot,
                ContainerInput::Pickup,
            );
            assert_eq!(sets(&out), vec![], "{held} into slot {slot}");
            assert!(be.item(usize::try_from(slot).expect("slot")).is_empty());
            assert_eq!(menu.carried.item(), held, "still on the cursor");
        }
    }

    /// **Scenario 835.** A smeltable shift-clicked from the hotbar goes to
    /// slot 0 — `canSmelt` — and the write is a `setItem`, so the same click
    /// sends the cook total.
    #[test]
    fn a_smeltable_quick_moves_into_the_ingredient_slot() {
        let r = recipes();
        let mut be = BlockEntity::new(Kind::Furnace);
        let (mut menu, mut inv, _) =
            open("minecraft:furnace", &mut be, "minecraft:raw_iron", Some(&r));
        let out = run(
            &mut menu,
            &mut inv,
            &mut be,
            Some(&r),
            FURNACE_HOTBAR_0,
            ContainerInput::QuickMove,
        );
        assert_eq!(
            sets(&out),
            vec![
                (0, "minecraft:raw_iron".to_string(), 1),
                (30, String::new(), 0)
            ]
        );
        assert_eq!(data(&out), vec![(3, 200)]);
    }

    /// Neither smeltable nor fuel: the hotbar arm moves it into the main
    /// inventory, **not** into the furnace — the arm a chest's rule has no
    /// equivalent of.
    #[test]
    fn a_stone_quick_moves_from_the_hotbar_into_the_main_inventory() {
        let r = recipes();
        let mut be = BlockEntity::new(Kind::Furnace);
        let (mut menu, mut inv, _) =
            open("minecraft:furnace", &mut be, "minecraft:stone", Some(&r));
        let out = run(
            &mut menu,
            &mut inv,
            &mut be,
            Some(&r),
            FURNACE_HOTBAR_0,
            ContainerInput::QuickMove,
        );
        assert_eq!(
            sets(&out),
            vec![
                (3, "minecraft:stone".to_string(), 1),
                (30, String::new(), 0)
            ]
        );
        assert!(be.item(0).is_empty() && be.item(1).is_empty());
    }

    /// **Scenario 841.** Blaze powder is a fuel and an ingredient; the fuel
    /// slot is tested first, and the successful move returns `EMPTY`.
    #[test]
    fn blaze_powder_quick_moves_into_the_brewing_fuel_slot() {
        let r = recipes();
        let mut be = BlockEntity::new(Kind::BrewingStand);
        let (mut menu, mut inv, _) = open(
            "minecraft:brewing_stand",
            &mut be,
            "minecraft:blaze_powder",
            Some(&r),
        );
        let out = run(
            &mut menu,
            &mut inv,
            &mut be,
            Some(&r),
            BREWING_HOTBAR_0,
            ContainerInput::QuickMove,
        );
        assert_eq!(
            sets(&out),
            vec![
                (4, "minecraft:blaze_powder".to_string(), 1),
                (32, String::new(), 0)
            ]
        );
        assert!(be.item(3).is_empty(), "not the ingredient slot");
    }

    /// The clock thread's sweep: what a ticker wrote reaches the menu as a
    /// slot correction with the next state id, then the data slots that
    /// moved — and a second sweep with nothing changed sends nothing.
    #[test]
    fn the_sweep_sends_what_the_ticker_wrote_and_then_nothing() {
        let mut be = BlockEntity::new(Kind::Furnace);
        be.set_item(1, ItemStack::one("minecraft:coal"));
        let (mut menu, _, _) = open("minecraft:furnace", &mut be, "minecraft:stick", None);
        let before = menu.state_id;
        // What one lighting tick does: the coal is consumed and the counters
        // move.
        be.set_item(1, ItemStack::empty());
        if let Some(crate::behaviour::Behaviour::Furnace(f)) = be.behaviour_mut() {
            f.lit_time_remaining = 1600;
            f.lit_total_time = 1600;
            f.cooking_timer = 1;
            f.cooking_total_time = 200;
        }
        let out = sweep_container(&mut menu, Some(&be), None);
        assert_eq!(sets(&out), vec![(1, String::new(), 0)]);
        assert!(matches!(out[0], Reply::SetSlot { state_id, .. } if state_id == before + 1));
        assert_eq!(data(&out), vec![(0, 1600), (1, 1600), (2, 1), (3, 200)]);
        assert!(sweep_container(&mut menu, Some(&be), None).is_empty());
        // One tick on: two counters move, and only those two are sent.
        if let Some(crate::behaviour::Behaviour::Furnace(f)) = be.behaviour_mut() {
            f.lit_time_remaining = 1599;
            f.cooking_timer = 2;
        }
        assert_eq!(
            data(&sweep_container(&mut menu, Some(&be), None)),
            vec![(0, 1599), (2, 2)]
        );
    }

    /// A burning furnace saves its counters under vanilla's four short keys
    /// and loads them back onto the behaviour the ticker reads — before
    /// this, it saved the constructor's zeros and came back cold.
    #[test]
    fn a_burning_furnace_saves_and_reloads_its_counters() {
        let mut be = BlockEntity::new(Kind::Smoker);
        if let Some(crate::behaviour::Behaviour::Furnace(f)) = be.behaviour_mut() {
            f.lit_time_remaining = 1234;
            f.lit_total_time = 1600;
            f.cooking_timer = 77;
            f.cooking_total_time = 100;
        }
        let nbt = be.to_nbt((1, 2, 3));
        let c = nbt.as_compound().expect("compound");
        assert_eq!(
            c.get("lit_time_remaining"),
            Some(&rustmc_world::nbt::Nbt::Short(1234))
        );
        assert_eq!(
            c.get("cooking_time_spent"),
            Some(&rustmc_world::nbt::Nbt::Short(77))
        );
        let (_, back) = BlockEntity::from_nbt(&nbt).expect("round trip");
        assert_eq!(back.container_data(), vec![1234, 1600, 77, 100]);
    }
}
