//! The container channel: `ServerPlayer.openMenu` and the two packets it
//! sends.
//!
//! # The counter that makes these checks one item
//!
//! `ServerPlayer.containerCounter` is a field on the **player**, not on the
//! menu or the block, and `nextContainerCounter` is
//!
//! ```text
//!   containerCounter = containerCounter % 100 + 1
//! ```
//!
//! — one `putfield`, in one method, called from `openMenu`,
//! `openHorseInventory` and `openNautilusInventory` and nowhere else. It is
//! **never reset**: not on close (`doCloseContainer` touches
//! `containerMenu`, not the counter), not on respawn (`restoreFrom` does not
//! mention it), so it runs for the whole life of the connection and its range
//! is 1..=100.
//!
//! The gameplay corpus logs one actor in and plays every scenario down that
//! one connection, so the counter runs across the **whole corpus**: the four
//! menu checks see window ids 1, 2, 3 and 4 in corpus order and no other
//! scenario touches it. That is why they are one item. A server that
//! implemented only the crafting table would send `window=1` where vanilla
//! sends `window=3` and diverge on a field that has nothing to do with
//! crafting tables.
//!
//! **The hazard runs the other way too, and it is the one to watch.** The
//! counter advances once per `openMenu` call, *including* the call whose
//! `createMenu` returns null — vanilla increments before it builds. So any
//! path on which this server opens a menu vanilla does not, or declines one
//! vanilla opens, shifts every later window id in the run and produces a wall
//! of unrelated failures downstream of the real defect. [`menu_for_state`]
//! carries the list of blocks that open menus and the note about what is
//! missing from it, because that list is where such a divergence would be
//! introduced.
//!
//! # The order the two packets go out in
//!
//! `openMenu` sends `ClientboundOpenScreenPacket` (offset 99) and *then*
//! calls `initMenu` (offset 104), which sets the synchronizer, which calls
//! `sendAllDataToRemote`, which calls `sendInitialData` — the
//! `ClientboundContainerSetContentPacket`. So **`open_screen` precedes
//! `container_set_content`**, always.
//!
//! The content packet's state id is `menu.incrementStateId()`, evaluated
//! *inside* the packet constructor's argument list, and a fresh
//! `AbstractContainerMenu` has `stateId = 0`. Every menu this server opens is
//! fresh, so **the first content packet of a menu always carries state 1** —
//! not because 1 was chosen but because `(0 + 1) & 32767` is 1.
//!
//! Note also that a chest's lid `blockEvent` is queued *before* either packet
//! — `ChestMenu`'s constructor calls `container.startOpen`, which runs during
//! `createMenu` at offset 37, well before the send at 99. It still reaches
//! the client last, because `ServerLevel.blockEvent` only queues and
//! `runBlockEvents` drains at the tick boundary. Two different orders, and
//! only the wire one is observable.

use crate::interact::BlockPos;
use rustmc_proto::varint::write_varint;
use rustmc_world::nbt::Nbt;
use std::collections::BTreeMap;
use std::io;

/// What a block opens.
///
/// Read from each block class's `useWithoutItem`/`openContainer` and from the
/// menu constructor it reaches. `slots` is the **total** slot count of the
/// menu, container slots plus the 36 the player's own inventory contributes
/// through `addStandardInventorySlots`, because that is what
/// `sendAllDataToRemote` walks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuSpec {
    /// The `minecraft:menu` registry name, resolved to a protocol id through
    /// [`crate::protocol_ids`].
    pub menu_type: &'static str,
    /// How the menu's own slots are laid out — see [`MenuKind`].
    pub kind: MenuKind,
    /// `AbstractContainerMenu.slots.size()`.
    pub slots: usize,
    /// How many container slots come before the player's inventory. The
    /// player's 36 always occupy the *last* 36 indices — except in
    /// `CraftingMenu`, where they still do, because its result slot is added
    /// first and its grid second.
    pub container_slots: usize,
    /// `AbstractContainerMenu.dataSlots.size()` — the `container_set_data`
    /// values that follow the content packet.
    pub data_slots: usize,
    /// The `MenuProvider.getDisplayName()` translate key for a container with
    /// no custom name.
    pub title: &'static str,
    /// Which `quickMoveStack` this menu has — see [`QuickMove`].
    pub quick_move: QuickMove,
    /// Which workstation's `slotsChanged`, result slot and `clickMenuButton`
    /// this menu runs, or `None` for a menu with no computed output — see
    /// [`crate::workstation`].
    pub station: Option<crate::workstation::Station>,
}

/// Which `AbstractContainerMenu.quickMoveStack(Player, int)` a menu carries.
///
/// **The method is `abstract` on `AbstractContainerMenu`** — offset 820 of the
/// class file declares it with no body — so there is no generic rule to
/// implement and no default to fall back on. Every menu overrides it, and the
/// overrides disagree about more than slot ranges:
///
/// * `ChestMenu.quickMoveStack` (offsets 0-129) splits on
///   `index < containerRows * 9` and calls `moveItemStackTo` once, in one of
///   two directions.
/// * `CraftingMenu.quickMoveStack` (offsets 0-232) has **four** arms, calls
///   `Item.onCraftedBy` and `Slot.onQuickCraft` on the result slot, and splits
///   the player's own 36 into hotbar (37-46) and main (10-37) with a
///   *fallback* from one to the other — the arm at offsets 112-153.
/// * `AbstractFurnaceMenu.quickMoveStack` (offsets 0-258) routes a player-side
///   stack by asking `canSmelt` and then `isFuel`, so where a stack lands is a
///   function of the recipe book and the fuel table rather than of the slot
///   index.
///
/// A menu whose override has not been transcribed declines the click rather
/// than borrowing the chest's rule. That is the same choice the `QUICK_MOVE`
/// arm's own doc makes and for the same reason: a wrong quick move puts a
/// stack in a plausible-looking wrong slot, and every later assertion in the
/// run is then measuring the wrong inventory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickMove {
    /// `ChestMenu.quickMoveStack`, transcribed in
    /// [`crate::click::Slots::quick_move_stack`].
    ChestMenu,
    /// `AbstractFurnaceMenu.quickMoveStack`, offsets 0-259, for a menu whose
    /// `acceptedInputs` is the given cooking type's `RecipePropertySet`.
    ///
    /// The cooking kind is part of the rule rather than of the block,
    /// because `canSmelt` is the property set the **menu** was built with:
    /// raw beef shift-clicked into a blast furnace is not smeltable there and
    /// falls through to the inventory-shuffle arms.
    Furnace(crate::recipes::CookingKind),
    /// `BrewingStandMenu.quickMoveStack`, offsets 0-311.
    BrewingStand,
    /// The menu's own override, in [`crate::workstation::quick_move_stack`].
    Workstation,
    /// `CraftingMenu.quickMoveStack`, in
    /// [`crate::click::Slots::crafting_quick_move_stack`].
    Crafting,
    /// Not transcribed. The click is declined and nothing moves.
    NotImplemented,
}

/// How a menu's own slots are laid out, which decides where a click lands.
///
/// **Not cosmetic, and not derivable from the slot count.** A chest's 27 and a
/// crafting table's 10 are both "the container's own slots", but the crafting
/// table's slot 0 is an *output* that refuses a placement and whose contents
/// are computed rather than stored, and its 1..10 are a grid that lives on the
/// menu rather than in the world. A layout-blind implementation puts a log in
/// the result slot and then runs the recipe over an empty grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuKind {
    /// Every slot is storage, backed by a block entity. Chest, furnace.
    Container,
    /// `CraftingMenu`: the result slot is added **first**, then the 3x3 grid.
    /// Both live on the menu — `CraftingTableBlock` has no block entity — and
    /// `CraftingMenu.removed` hands the grid back to the player on close.
    Crafting,
    /// Every container slot lives on the **menu**, with no block entity and no
    /// result-first ordering: the slots are added in their own order and slot
    /// 0 is whatever that menu's first slot is.
    ///
    /// This is what the six `SimpleMenuProvider` menus are — anvil,
    /// grindstone, loom, smithing, cartography table, stonecutter. None of
    /// their blocks is a `BaseEntityBlock`; each menu constructs its own
    /// container (`ItemCombinerMenu`'s `inputSlots` for the anvil and the
    /// smithing table, a plain `SimpleContainer` for the other four) and that
    /// container dies with the menu.
    ///
    /// **Held empty, deliberately.** Opening one of these is the whole of what
    /// this server does with it: no scenario puts an item into one, so the
    /// contents are unmeasured, and an unmeasured write is how a "close
    /// enough" slot layout gets built. `quick_move` is `NotImplemented` for
    /// all six for the same reason. What *is* shared with
    /// [`MenuKind::Crafting`] is the storage — [`crate::click::SlotRef::Grid`]
    /// — so `close_open_menu` already hands the contents back on close, which
    /// is `clearContainer` and is what vanilla does here too.
    MenuLocal,
    /// `CrafterMenu`: the block entity's nine slots, **then** the player's 36,
    /// **then** a result slot — the only menu here whose own slots do not all
    /// come before the inventory.
    ///
    /// `CrafterMenu.addSlots` (offsets 0-102) adds the 3x3 of
    /// `CrafterSlot`s over the block entity, calls
    /// `addStandardInventorySlots` at offset 72, and only then adds the
    /// `NonInteractiveResultSlot` at offset 93, over the menu's own
    /// `ResultContainer`. So slot 45 is the result and the hotbar is 36..45,
    /// exactly where a chest-shaped layout of nine would put it — which is why
    /// a nine-slot container with one extra slot tacked on the front (the
    /// crafting table's shape) is the wrong model and this is its own kind.
    ContainerThenResult,
    /// `LecternMenu`: one slot over the lectern's `bookAccess` and **no
    /// player inventory at all** — the constructor never calls
    /// `addStandardInventorySlots` — so the whole menu is its container.
    ContainerOnly,
}

impl MenuSpec {
    /// The menu family whose `Slot` subclasses this menu installs, if it
    /// installs any — see [`crate::menuslots`].
    ///
    /// Read off [`Self::quick_move`] because the two are one fact: the menu
    /// *class* decides both its `quickMoveStack` and the slots its
    /// constructor adds, and a spec that carried them separately could pair a
    /// furnace's slots with a chest's quick move.
    #[must_use]
    pub const fn family(&self) -> Option<crate::menuslots::Family> {
        match self.quick_move {
            QuickMove::Furnace(_) => Some(crate::menuslots::Family::Furnace),
            QuickMove::BrewingStand => Some(crate::menuslots::Family::BrewingStand),
            QuickMove::ChestMenu
            | QuickMove::NotImplemented
            | QuickMove::Workstation
            | QuickMove::Crafting => None,
        }
    }

    /// How many crafting-grid slots the menu has: 9 for a crafting table and
    /// 0 for anything with no grid.
    ///
    /// Derived from `container_slots` rather than stored, because the two
    /// cannot disagree that way: `CraftingMenu`'s 10 is one result plus nine
    /// grid, and a spec that carried both numbers could claim otherwise.
    #[must_use]
    pub const fn grid_slots(&self) -> usize {
        match self.kind {
            MenuKind::Crafting => self.container_slots - 1,
            // No result slot is carved off the front the way `CraftingMenu`
            // carves one: these menus add their slots in their own order and
            // this server holds all of them as flat menu-local storage. Which
            // of them vanilla backs with `inputSlots` and which with a
            // `ResultContainer` is a distinction nothing here can observe
            // while they are all empty, and it is not asserted.
            MenuKind::MenuLocal => self.container_slots,
            // The crafter's nine are the block entity's, and its result slot
            // is [`crate::click::Menu::result`], not a grid cell.
            MenuKind::Container | MenuKind::ContainerThenResult | MenuKind::ContainerOnly => 0,
        }
    }

    /// The value data slot `id` carries in the `container_set_data` that
    /// `sendInitialData` sends when this menu is freshly opened.
    ///
    /// Zero for every data slot of every menu here **except the enchanting
    /// table's six clues**: `EnchantmentMenu`'s constructor fills
    /// `enchantClue` and `levelClue` with `iconst_m1` (offsets 48-76) before
    /// it shares them as data slots 4..10, so a fresh enchanting table says
    /// -1 there, "no clue", rather than 0, which is enchantment id 0.
    ///
    /// Everything else starts at its Java default: a fresh furnace family's
    /// four counters, a fresh brewing stand's `brewTime`/`fuel`, a fresh
    /// beacon's `levels` and its two powers (`BeaconMenu.encodeEffect(null)`
    /// is `iconst_0`, offsets 0-5), a fresh crafter's nine slot states and
    /// its `triggered`, and every combiner menu's single slot.
    ///
    /// **One value is not reproduced and cannot be from here:** data slot 3
    /// of the enchanting table is `player.getEnchantmentSeed()` (offset 189),
    /// which is `Player.enchantmentSeed` — drawn from the player's random at
    /// construction. This server keeps no per-player seed, so it sends 0.
    #[must_use]
    pub fn initial_data(&self, id: usize) -> i16 {
        if self.menu_type == crate::enchanting::menu_shape::MENU_TYPE && (4..10).contains(&id) {
            -1
        } else {
            0
        }
    }
}

/// The menu a block opens when it is right-clicked, or `None`.
///
/// # What is here, and the much longer list that is not
///
/// Twenty-one block classes in 26.2 reach `Player.openMenu`: `AnvilBlock`,
/// `BarrelBlock`, `BeaconBlock`, `BlastFurnaceBlock`, `BrewingStandBlock`,
/// `CartographyTableBlock`, `ChestBlock`, `CrafterBlock`,
/// `CraftingTableBlock`, `DispenserBlock`, `EnchantingTableBlock`,
/// `EnderChestBlock`, `FurnaceBlock`, `GrindstoneBlock`, `HopperBlock`,
/// `LecternBlock`, `LoomBlock`, `ShulkerBoxBlock`, `SmithingTableBlock`,
/// `SmokerBlock` and `StonecutterBlock`, plus the subclasses that inherit one
/// — `TrappedChestBlock`, `CopperChestBlock` and `WeatheringCopperChestBlock`
/// from `ChestBlock`, `DropperBlock` from `DispenserBlock`.
///
/// **Eighteen of them are implemented and three are not — `BarrelBlock`,
/// `EnderChestBlock` and `LecternBlock` — and it is a plain feature gap.**
/// (Nine until the `diverge-burndown` branch added the dispenser, dropper,
/// hopper, shulker box, blast furnace, smoker, brewing stand, beacon,
/// enchanting table and crafter, each against a corpus check that recorded
/// vanilla opening it and this server opening nothing.) This paragraph used to say it was "a counter hazard rather
/// than a feature gap" — that a scenario right-clicking a barrel opens a menu
/// on vanilla and none here, so vanilla's `containerCounter` advances and this
/// server's does not, and *every* later menu check then reports a window id
/// one too low, pointing at four healthy checks instead of the one missing
/// block.
///
/// **That was true, it stopped being true at the `worldgen-iceberg-912`
/// repair, and it is now measured rather than argued.**
/// `compare::rebase_session_counters` rebases the `window` field onto the
/// scenario, and `ci/window-coupling.py` compares a real 109-scenario vanilla
/// recording against a copy of itself with the whole session counter shifted:
/// at +1, +2, +7 and +99 it moves **zero** checks, declared or strict, across
/// the 34 scenarios that carry an allocated window id. Not four. Zero.
/// `docs/PARITY-ROADMAP.md` §42 has the table and the four injected
/// container-id defects that do still get caught, which is what makes the zero
/// quotable.
///
/// So a missing opener below costs exactly the check that opens it, nothing
/// else in the run inherits anything from it, and menu coverage scales by
/// adding scenarios in any order. `ShulkerBoxBlock` was priced at 17 blocks
/// and refused on the retracted sentence; it is free.
///
/// Not implemented and deliberately named rather than approximated:
///
/// * **the double chest.** `ChestBlock.getMenuProvider` runs a combiner: two
///   halves give `MenuType.GENERIC_9x6`, 90 slots and the title
///   `container.chestDouble`, and the openers counter of *both* halves is
///   incremented. `240-chest-pairs-with-its-neighbour` builds one and never
///   opens it, so nothing measures this today.
/// * ~~**a custom-named container.**~~ — **closed 2026-10-01.** A container
///   placed from a stack carrying `minecraft:custom_name` keeps it
///   (`crate::blockentity::BlockEntity::custom_name`), and the menu title is
///   that name rather than `getDefaultName()`. `945` is the scenario.
/// * ~~**`isSecondaryUseActive`**~~ — **closed 2026-08-10.** A sneaking
///   player holding something places against a chest rather than opening it,
///   and that is now what happens: the gate is in
///   `crate::world_send::handle_use_item_on`, in front of the whole
///   block-interaction arm, so `open_menu_for` is never reached on a sneaking
///   click. `609` is the scenario. What is left of the gap is the conjunct —
///   `holdingSomething` reads both hands and this server has no offhand — so
///   a sneaking player with an empty main hand still opens the chest here,
///   which is also what vanilla does unless the offhand is full.
/// * **`AbstractContainerMenu.stillValid`**, and this one is a *behaviour*
///   gap rather than a missing block. `ServerPlayer.tick` closes a menu whose
///   `stillValid` has gone false, and a container's is
///   `player.distanceToSqr(pos + 0.5) <= 64.0` — eight blocks. So on vanilla
///   the actor's chest menu closes by itself the moment the corpus walks it
///   to the next workspace, with a `ClientboundContainerClosePacket`; here it
///   stays open until the next `openMenu` closes it.
///
///   **It cannot shift the counter, which is the only reason it is safe to
///   defer.** `closeContainer` and `doCloseContainer` do not touch
///   `containerCounter`, so the sequence is identical whether the previous
///   menu was closed early by distance or late by the next open. What differs
///   is one packet nothing in the corpus observes: `container_close` is
///   absent from vanilla's own recording of all 62 scenarios, because every
///   one of those closes lands in the un-measured settle between scenarios.
///   The first check that watches that channel will see it, and should be
///   believed over this note.
///
/// Takes a **state**, not a block name, and splits it here: every caller has
/// a state string in hand, and a lookup keyed on the whole of
/// `minecraft:chest[facing=north,type=single,waterlogged=false]` silently
/// matches nothing, which reads as "chests do not open" rather than as a
/// mistake.
#[must_use]
pub fn menu_for_state(state: &str) -> Option<MenuSpec> {
    match state.split('[').next().unwrap_or(state) {
        // `ChestMenu.threeRows` -> `MenuType.GENERIC_9x3`, `containerRows` 3.
        // 27 chest slots from `addChestGrid`, then 36 from
        // `addStandardInventorySlots`. Title `container.chest`, out of
        // `ChestBlockEntity.DEFAULT_NAME`.
        "minecraft:chest" => Some(MenuSpec {
            menu_type: "minecraft:generic_9x3",
            kind: MenuKind::Container,
            slots: 63,
            container_slots: 27,
            data_slots: 0,
            title: "container.chest",
            quick_move: QuickMove::ChestMenu,
            station: None,
        }),
        // `AbstractFurnaceMenu`: ingredient, fuel, result, then 36. Four data
        // slots — `SimpleContainerData(4)` — and they are the reason a
        // furnace is not a chest with a different number in it.
        "minecraft:furnace" => Some(MenuSpec {
            menu_type: "minecraft:furnace",
            kind: MenuKind::Container,
            slots: 39,
            container_slots: 3,
            data_slots: 4,
            title: "container.furnace",
            // `AbstractFurnaceMenu.quickMoveStack` asks `canSmelt` and
            // `isFuel` before it picks a destination range.
            quick_move: QuickMove::Furnace(crate::recipes::CookingKind::Smelting),
            station: None,
        }),
        // `BlastFurnaceMenu` and `SmokerMenu` are `AbstractFurnaceMenu` with a
        // different `MenuType`, `RecipePropertySet` and `RecipeBookType` — the
        // same three slots, the same four data slots, the same 36. What tells
        // them apart on the wire is the registry id and the title, which is
        // `BlastFurnaceBlockEntity.getDefaultName` -> `container.blast_furnace`
        // and `SmokerBlockEntity`'s -> `container.smoker`. `744` and `745`.
        "minecraft:blast_furnace" => Some(MenuSpec {
            menu_type: "minecraft:blast_furnace",
            kind: MenuKind::Container,
            slots: 39,
            container_slots: 3,
            data_slots: 4,
            title: "container.blast_furnace",
            quick_move: QuickMove::Furnace(crate::recipes::CookingKind::Blasting),
            station: None,
        }),
        "minecraft:smoker" => Some(MenuSpec {
            menu_type: "minecraft:smoker",
            kind: MenuKind::Container,
            slots: 39,
            container_slots: 3,
            data_slots: 4,
            title: "container.smoker",
            quick_move: QuickMove::Furnace(crate::recipes::CookingKind::Smoking),
            station: None,
        }),
        // `BrewingStandMenu(int, Inventory, Container, ContainerData)`:
        // `checkContainerSize(container, 5)` and `checkContainerDataCount(data,
        // 2)`, then three `PotionSlot`s (container 0..2), an `IngredientsSlot`
        // (3), a `FuelSlot` (4), `addDataSlots` and `addStandardInventorySlots`
        // — so 5 + 36 = 41. The data slots are added **before** the player's
        // inventory, which moves no slot index because data slots are not
        // slots. Title `container.brewing`, out of
        // `BrewingStandBlockEntity.getDefaultName`. `746`.
        "minecraft:brewing_stand" => Some(MenuSpec {
            menu_type: "minecraft:brewing_stand",
            kind: MenuKind::Container,
            slots: 41,
            container_slots: 5,
            data_slots: 2,
            title: "container.brewing",
            quick_move: QuickMove::BrewingStand,
            station: None,
        }),
        // `CraftingMenu`: the **result slot is added first**, so it is slot
        // 0, then the 3x3 grid, then 36. No block entity anywhere on the
        // path — the provider is a `SimpleMenuProvider` built in
        // `CraftingTableBlock.getMenuProvider` — which is the whole point of
        // the check that covers it.
        "minecraft:crafting_table" => Some(MenuSpec {
            menu_type: "minecraft:crafting",
            kind: MenuKind::Crafting,
            slots: 46,
            container_slots: 10,
            data_slots: 0,
            title: "container.crafting",
            // `CraftingMenu.quickMoveStack`: the result arm crafts repeatedly
            // under `doClick`'s loop; the player-side arm tries the grid
            // first and then falls back between hotbar and main inventory.
            quick_move: QuickMove::Crafting,
            station: None,
        }),

        // ------------------------------------------------------------------
        // The six `SimpleMenuProvider` menus. Every one of these is a block
        // with no block entity whose `getMenuProvider` builds the provider in
        // the block class, which is the path `480` already proves works — so
        // what each one costs is this row and nothing else.
        //
        // Slot counts are `addSlot` calls on the menu's own constructor path
        // plus the 36 `addStandardInventorySlots` adds (27 through
        // `addInventoryExtendedSlots`, 9 through `addInventoryHotbarSlots`).
        // Data slots are `addDataSlot` calls in the same constructor.
        //
        // **Two titles are not named after their block and were read rather
        // than guessed**: `AnvilBlock.CONTAINER_TITLE` is `container.repair`
        // and `SmithingTableBlock`'s is `container.upgrade`.
        //
        // **And only two of the three "item combiner" menus are one.**
        // `AnvilMenu` and `SmithingMenu` extend `ItemCombinerMenu` and declare
        // their slots through `createInputSlotDefinitions`; `GrindstoneMenu`
        // extends `AbstractContainerMenu` directly and calls `addSlot` three
        // times itself. `docs/cold-start/gameplay-container-menus.md` says
        // three, and three was the reason this tier was priced as risky.
        // ------------------------------------------------------------------

        // `AnvilMenu extends ItemCombinerMenu`; `createInputSlotDefinitions`
        // is two `withSlot` and one `withResultSlot`. One data slot — `cost`.
        // All three anvil states are `AnvilBlock`; only `minecraft:anvil` is
        // measured by a scenario, and the other two are here because refusing
        // them would be a behaviour this server invented.
        "minecraft:anvil" | "minecraft:chipped_anvil" | "minecraft:damaged_anvil" => {
            Some(MenuSpec {
                menu_type: "minecraft:anvil",
                kind: MenuKind::MenuLocal,
                slots: 39,
                container_slots: 3,
                data_slots: 1,
                title: "container.repair",
                quick_move: QuickMove::Workstation,
                station: Some(crate::workstation::Station::Anvil),
            })
        }
        // `GrindstoneMenu extends AbstractContainerMenu` — three bare
        // `addSlot` calls, no `addDataSlot` anywhere in the class.
        "minecraft:grindstone" => Some(MenuSpec {
            menu_type: "minecraft:grindstone",
            kind: MenuKind::MenuLocal,
            slots: 39,
            container_slots: 3,
            data_slots: 0,
            title: "container.grindstone_title",
            quick_move: QuickMove::Workstation,
            station: Some(crate::workstation::Station::Grindstone),
        }),
        // `LoomMenu`: banner, dye, pattern, result — four `addSlot`. One data
        // slot, `selectedBannerPatternIndex`.
        "minecraft:loom" => Some(MenuSpec {
            menu_type: "minecraft:loom",
            kind: MenuKind::MenuLocal,
            slots: 40,
            container_slots: 4,
            data_slots: 1,
            title: "container.loom",
            quick_move: QuickMove::Workstation,
            station: Some(crate::workstation::Station::Loom),
        }),
        // `SmithingMenu extends ItemCombinerMenu`: three `withSlot` and one
        // `withResultSlot`, so **four** where the anvil has three — the two
        // `ItemCombinerMenu` subclasses genuinely disagree and the superclass
        // does not decide it. One data slot, `hasRecipeError`.
        "minecraft:smithing_table" => Some(MenuSpec {
            menu_type: "minecraft:smithing",
            kind: MenuKind::MenuLocal,
            slots: 40,
            container_slots: 4,
            data_slots: 1,
            title: "container.upgrade",
            quick_move: QuickMove::Workstation,
            station: Some(crate::workstation::Station::Smithing),
        }),
        // `CartographyTableMenu`: map, additional, result. No data slots.
        "minecraft:cartography_table" => Some(MenuSpec {
            menu_type: "minecraft:cartography_table",
            kind: MenuKind::MenuLocal,
            slots: 39,
            container_slots: 3,
            data_slots: 0,
            title: "container.cartography_table",
            quick_move: QuickMove::Workstation,
            station: Some(crate::workstation::Station::Cartography),
        }),
        // `StonecutterMenu`: input and result only — the narrowest of the six.
        // One data slot, `selectedRecipeIndex`.
        "minecraft:stonecutter" => Some(MenuSpec {
            menu_type: "minecraft:stonecutter",
            kind: MenuKind::MenuLocal,
            slots: 38,
            container_slots: 2,
            data_slots: 1,
            title: "container.stonecutter",
            quick_move: QuickMove::Workstation,
            station: Some(crate::workstation::Station::Stonecutter),
        }),
        // `EnchantingTableBlock` is a `BaseEntityBlock` and its
        // `getMenuProvider` reads the block entity's display name, but nothing
        // in the menu reads the block entity after that: `EnchantmentMenu`'s
        // two slots live on its own `enchantSlots` container, which
        // `removed` hands back through `clearContainer`, exactly as the six
        // above. Two `addSlot`, then `addStandardInventorySlots`; ten
        // `addDataSlot` — three costs, the seed, three clues, three levels.
        "minecraft:enchanting_table" => Some(MenuSpec {
            menu_type: crate::enchanting::menu_shape::MENU_TYPE,
            kind: MenuKind::MenuLocal,
            slots: crate::enchanting::menu_shape::SLOTS,
            container_slots: crate::enchanting::menu_shape::CONTAINER_SLOTS,
            data_slots: crate::enchanting::menu_shape::DATA_SLOTS,
            title: crate::enchanting::menu_shape::TITLE,
            quick_move: QuickMove::Workstation,
            station: Some(crate::workstation::Station::Enchanting),
        }),

        // ------------------------------------------------------------------
        // The block-entity menus beyond the chest and the furnace. Each title
        // is the `ldc` in its block entity's `DEFAULT_NAME`/`getDefaultName`,
        // read out of the class rather than guessed: `container.shulkerBox`
        // is camel-case, `container.blast_furnace` is snake-case, and the
        // enchanting table's is `container.enchant`.
        // ------------------------------------------------------------------

        // `DispenserMenu`: `checkContainerSize(9)`, a 3x3 over the block
        // entity, then 36. `MenuType.GENERIC_3x3`. `DropperBlock extends
        // DispenserBlock` and `DropperBlockEntity extends
        // DispenserBlockEntity`, so the dropper opens the same menu under its
        // own name.
        "minecraft:dispenser" => Some(MenuSpec {
            menu_type: "minecraft:generic_3x3",
            kind: MenuKind::Container,
            slots: 45,
            container_slots: 9,
            data_slots: 0,
            title: "container.dispenser",
            quick_move: QuickMove::NotImplemented,
            station: None,
        }),
        "minecraft:dropper" => Some(MenuSpec {
            menu_type: "minecraft:generic_3x3",
            kind: MenuKind::Container,
            slots: 45,
            container_slots: 9,
            data_slots: 0,
            title: "container.dropper",
            quick_move: QuickMove::NotImplemented,
            station: None,
        }),
        // `HopperMenu`: `checkContainerSize(5)`, one row of five, then 36.
        "minecraft:hopper" => Some(MenuSpec {
            menu_type: "minecraft:hopper",
            kind: MenuKind::Container,
            slots: 41,
            container_slots: 5,
            data_slots: 0,
            title: "container.hopper",
            quick_move: QuickMove::NotImplemented,
            station: None,
        }),
        // `ShulkerBoxMenu`: 27 over the block entity, then 36 — a chest's
        // count and a different registry entry, `MenuType.SHULKER_BOX`. All
        // seventeen colours are `ShulkerBoxBlock` with one block entity type.
        // Whether it opens at all is `ShulkerBoxBlock.canOpen`, which is the
        // caller's: see `crate::world_send`.
        b if crate::blockentity::for_state(b) == Some(crate::blockentity::Kind::ShulkerBox) => {
            Some(MenuSpec {
                menu_type: "minecraft:shulker_box",
                kind: MenuKind::Container,
                slots: 63,
                container_slots: 27,
                data_slots: 0,
                title: "container.shulkerBox",
                quick_move: QuickMove::NotImplemented,
                station: None,
            })
        }
        // `BeaconMenu`: the one payment slot is over the menu's **own**
        // `SimpleContainer(1)`, not the block entity — a beacon is not a
        // `Container` — then 36, then the block entity's three data slots
        // (`levels`, primary, secondary).
        "minecraft:beacon" => Some(MenuSpec {
            menu_type: "minecraft:beacon",
            kind: MenuKind::MenuLocal,
            slots: 37,
            container_slots: 1,
            data_slots: 3,
            title: "container.beacon",
            quick_move: QuickMove::NotImplemented,
            station: None,
        }),
        // `CrafterMenu`: nine over the block entity, 36, then the result —
        // see [`MenuKind::ContainerThenResult`] — and `addDataSlots` of the
        // block entity's ten (nine slot states and `triggered`).
        // `MenuType.CRAFTER_3x3`.
        "minecraft:crafter" => Some(MenuSpec {
            menu_type: "minecraft:crafter_3x3",
            kind: MenuKind::ContainerThenResult,
            slots: 46,
            container_slots: 9,
            data_slots: 10,
            title: "container.crafter",
            quick_move: QuickMove::NotImplemented,
            station: None,
        }),
        // `LecternMenu(int, Container, ContainerData)`: `checkContainerSize(1)`,
        // `checkContainerDataCount(1)`, one `addSlot` over the book and
        // `addDataSlots` of the page — nothing else. `LecternBlock
        // .useWithoutItem` reaches `openScreen` only when `HAS_BOOK`; a
        // lectern without one consumes the click and opens nothing (offsets
        // 0-38), so an empty lectern answers `None` here.
        "minecraft:lectern" if crate::placement::property_of(state, "has_book") == Some("true") => {
            Some(MenuSpec {
                menu_type: "minecraft:lectern",
                kind: MenuKind::ContainerOnly,
                slots: 1,
                container_slots: 1,
                data_slots: 1,
                title: "container.lectern",
                quick_move: QuickMove::NotImplemented,
                station: None,
            })
        }
        _ => None,
    }
}

/// `ChestBlock.getConnectedDirection` — the horizontal step from one half of
/// a double chest to the other: `LEFT` is `facing.getClockWise()`, anything
/// else `getCounterClockWise()` (offsets 0-36).
fn chest_connected_step(state: &str) -> Option<(i32, i32)> {
    let facing = crate::placement::property_of(state, "facing")?;
    let left = crate::placement::property_of(state, "type") == Some("left");
    // (dx, dz) of north/east/south/west, and their clockwise successor.
    let (dx, dz) = match facing {
        "north" => (0, -1),
        "east" => (1, 0),
        "south" => (0, 1),
        "west" => (-1, 0),
        _ => return None,
    };
    // Clockwise seen from above: north -> east -> south -> west.
    Some(if left { (-dz, dx) } else { (dz, -dx) })
}

/// `ChestBlock.combine` for a menu: the `(first, second)` block positions of
/// the `CompoundContainer` a double chest opens, or `None` for a chest that
/// opens alone.
///
/// `DoubleBlockCombiner.combineWithNeigbour` — a `SINGLE` chest is single;
/// otherwise the neighbour at `getConnectedDirection` must be the **same
/// block**, the other `BlockType`, and the same `FACING`, or the chest is
/// single after all. `getBlockType` makes `RIGHT` the `FIRST` half and
/// `LEFT` the `SECOND` (offsets 0-36), and `MENU_PROVIDER_COMBINER
/// .acceptDouble(first, second)` builds `new CompoundContainer(first,
/// second)` in that order — so slots 0..27 are the right half's.
///
/// Measured on `742`: two chests placed east of one another, the west one
/// `type=left`; vanilla's lid events go out for the east half first and the
/// west second, which is `CompoundContainer.startOpen` walking first then
/// second.
#[must_use]
pub fn double_chest(
    state: &str,
    pos: BlockPos,
    at: &dyn Fn(BlockPos) -> String,
) -> Option<(BlockPos, BlockPos)> {
    let block = state.split('[').next().unwrap_or(state);
    let ty = crate::placement::property_of(state, "type")?;
    if ty == "single" {
        return None;
    }
    let (dx, dz) = chest_connected_step(state)?;
    let other = (pos.0 + dx, pos.1, pos.2 + dz);
    let neighbour = at(other);
    let other_ty = crate::placement::property_of(&neighbour, "type");
    let paired = neighbour.split('[').next() == Some(block)
        && other_ty.is_some_and(|t| t != "single" && t != ty)
        && crate::placement::property_of(&neighbour, "facing")
            == crate::placement::property_of(state, "facing");
    if !paired {
        return None;
    }
    Some(if ty == "right" {
        (pos, other)
    } else {
        (other, pos)
    })
}

/// `ChestMenu.sixRows` over a `CompoundContainer` —
/// `MenuType.GENERIC_9x6`, 54 container slots and 36, and the title
/// `container.chestDouble` that `ChestBlock`'s `MENU_PROVIDER_COMBINER`
/// gives an unnamed pair.
pub const DOUBLE_CHEST: MenuSpec = MenuSpec {
    menu_type: "minecraft:generic_9x6",
    kind: MenuKind::Container,
    slots: 90,
    container_slots: 54,
    data_slots: 0,
    title: "container.chestDouble",
    quick_move: QuickMove::ChestMenu,
    station: None,
};

/// `ServerPlayer.nextContainerCounter` — the whole of it.
///
/// Kept as a named function rather than inlined because the modulo is not
/// where anyone expects it: it is `% 100 + 1`, so the sequence is
/// 1..=100 and then 1 again, and there is no 0. Window 0 is the player's own
/// `InventoryMenu`, which is why the counter must never produce it.
#[must_use]
pub const fn next_container_counter(counter: i32) -> i32 {
    counter % 100 + 1
}

/// `AbstractContainerMenu.incrementStateId` — `(stateId + 1) & 32767`.
#[must_use]
pub const fn increment_state_id(state_id: i32) -> i32 {
    (state_id + 1) & 32767
}

/// The player's half of the container channel.
///
/// **Not on [`crate::player::PlayerState`], and the reason is worth keeping.**
/// That struct is `Copy` and `PartialEq`, and it is copied per movement packet
/// and compared to decide whether anything moved. Hanging an owned `String`
/// off it would have cost both derives — and, worse, would have made a menu
/// part of the value a movement comparison looks at. Vanilla keeps both on
/// `ServerPlayer` because Java has no such distinction; here the split is the
/// honest one.
///
/// Lives beside the `Hotbar`, for the same connection lifetime, which is what
/// `ServerPlayer`'s own fields have.
#[derive(Debug, Default)]
pub struct MenuState {
    /// `ServerPlayer.containerCounter`. Starts at 0 because a fresh
    /// `ServerPlayer` is a fresh Java object, so the first
    /// `nextContainerCounter` yields 1; nothing ever returns it to 0. The
    /// whole corpus runs down one connection, so this carries state between
    /// scenarios by design.
    pub counter: i32,
    /// `ServerPlayer.containerMenu`, when it is not the inventory menu.
    ///
    /// **Shared with the clock thread**, because vanilla touches this object
    /// from two places and this server runs them on two threads: the click
    /// handler (the connection thread here) and `ServerPlayer.doTick`'s
    /// `containerMenu.broadcastChanges()` (the clock thread here), which is
    /// what carries a furnace's own writes — fuel consumed, a result made,
    /// the progress bar — to the player looking at it. The same `Arc` is
    /// registered on the [`crate::players::Connection`] while the menu is
    /// open. Both sides lock it under [`crate::broadcast::tick_lock`], so the
    /// two can never interleave inside one menu.
    pub open: Option<SharedMenu>,
    /// `InventoryMenu.stateId` — the state id of **window 0**, the player's
    /// own menu.
    ///
    /// Separate from every [`crate::click::Menu::state_id`] because it belongs
    /// to a menu that is never opened and never closed: vanilla builds one
    /// `InventoryMenu` in the `ServerPlayer` constructor and it outlives every
    /// container the player opens. Like [`Self::counter`], and for the same
    /// reason, this carries across scenarios — the whole corpus runs down one
    /// connection, so a counter that restarted per scenario would be a
    /// different counter from vanilla's from the second scenario onwards.
    ///
    /// Starts at 0: `AbstractContainerMenu.incrementStateId` bumps *then*
    /// returns, so the first packet sent on this menu carries 1.
    ///
    /// # This server advances it in one place only, and vanilla does not
    ///
    /// `ServerPlayer.tick` calls `containerMenu.broadcastChanges()` every
    /// tick, and `containerMenu` **is** the inventory menu whenever nothing
    /// else is open — so vanilla sweeps window 0 continuously and corrects any
    /// cell the server changed behind the client's back. This server has no
    /// such sweep: the only thing that advances this counter is
    /// `doCloseContainer`'s handback (see `close_open_menu` in
    /// [`crate::world_send`]).
    ///
    /// That is a real gap and it is measurable, so it is written here rather
    /// than left to be inferred: any window-0 correction vanilla emits from
    /// somewhere other than a menu close is one this server does not emit —
    /// the state id counts packets sent, not changes made. Two such sources
    /// are already in the corpus and are both absent here: picking a dropped
    /// item up, and swapping to the offhand.
    ///
    /// **What that gap now costs in the corpus, corrected 2026-08-11.** This
    /// used to say each miss leaves the counter "permanently one behind
    /// vanilla's", and that is still true of the counter and is no longer the
    /// measurable consequence. `compare::rebase_session_counters` rebases
    /// `container_set_slot.state` onto the scenario, so a *standing* offset
    /// inherited from an earlier scenario is discarded — deliberately, because
    /// vanilla cannot reproduce it against itself either. **The missing
    /// packet is still caught**, on its own scenario, as an observation
    /// vanilla has and we do not, and so is the changed gap between two
    /// packets on one menu. So the gap is not excused; it is charged where it
    /// happens instead of everywhere afterwards.
    pub inventory_state_id: i32,
    /// `Player.enchantmentSeed`.
    ///
    /// **0 for a new player, and that is the whole reason an enchanting
    /// table's offers are reproducible.** The `Player` constructor stores
    /// `iconst_0` at offset 63; `readAdditionalSaveData` replaces it with
    /// `random.nextInt()` only when a saved `XpSeed` reads back as 0 (offsets
    /// 110-139), and a player with no save file never reaches that method.
    /// Every corpus run boots a fresh world, so the actor's seed is 0 until
    /// `onEnchantmentPerformed` draws a new one — which is the first random
    /// thing an enchanting table does.
    pub enchantment_seed: i32,
}

impl MenuState {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// An open menu as both threads hold it — see [`MenuState::open`].
pub type SharedMenu = std::sync::Arc<std::sync::Mutex<OpenMenu>>;

/// An open menu, for as long as the player has it open.
///
/// The menu's own mutable state — the carried stack, the remote-belief table,
/// a crafting grid — lives on [`crate::click::Menu`]. This struct is what the
/// *connection* has to remember besides that: where the block is, so closing
/// can decrement the right openers counter and a click can find the right
/// block entity.
#[derive(Debug, Clone)]
pub struct OpenMenu {
    /// `ServerPlayer.containerMenu`.
    pub menu: crate::click::Menu,
    /// Where the block that opened it is, so closing can decrement the right
    /// openers counter.
    pub pos: BlockPos,
    pub block: String,
    /// The other half of a double chest — `CompoundContainer`'s second
    /// container, with [`Self::pos`] its first — or `None`.
    pub second: Option<BlockPos>,
}

impl OpenMenu {
    #[must_use]
    pub const fn container_id(&self) -> i32 {
        self.menu.container_id
    }

    #[must_use]
    pub const fn spec(&self) -> MenuSpec {
        self.menu.spec
    }
}

/// `ClientboundOpenScreenPacket` — container id, menu type, title.
///
/// The title is a `Component` under `ComponentSerialization`'s **trusted**
/// stream codec, which is network NBT and not JSON. For every menu this
/// server can open the component is a single `translate` key, so the
/// compound has one entry and the sorted-versus-insertion order question that
/// bites multi-key compounds elsewhere does not arise here.
///
/// # Errors
/// Only on a write to the buffer, which cannot fail for a `Vec`.
pub fn open_screen_body(container_id: i32, menu_id: i32, title_key: &str) -> io::Result<Vec<u8>> {
    let mut c = BTreeMap::new();
    c.insert("translate".to_string(), Nbt::String(title_key.to_string()));
    open_screen_titled(container_id, menu_id, &Nbt::Compound(c))
}

/// [`open_screen_body`] with the title already a component — a container's
/// `BaseContainerBlockEntity.name`, which `getDisplayName` returns in place
/// of the default `translate` key whenever the block was placed from a
/// stack carrying `minecraft:custom_name`. Written as the NBT
/// `ComponentSerialization.CODEC` made of it: a bare string for plain text.
///
/// # Errors
/// Only on a write to the buffer, which cannot fail for a `Vec`.
pub fn open_screen_titled(container_id: i32, menu_id: i32, title: &Nbt) -> io::Result<Vec<u8>> {
    let mut body = Vec::new();
    write_varint(&mut body, container_id)?;
    write_varint(&mut body, menu_id)?;
    write_network_nbt(&mut body, title)?;
    Ok(body)
}

/// A `Component` on the wire: the network form, with no root name.
///
/// The same shape as `crate::configuration`'s own writer. Duplicated as three
/// lines rather than made public there because the two are the same encoding
/// for different reasons — one is a registry payload, one is a chat
/// component — and a shared helper would make a later divergence between them
/// look like a bug in whichever caller noticed first.
fn write_network_nbt(out: &mut impl std::io::Write, value: &Nbt) -> io::Result<()> {
    let mut named = Vec::new();
    rustmc_world::nbt::write_named_root(&mut named, "", value)?;
    // `write_named_root` emits type, name length, name, payload. The network
    // form drops the name and its length and keeps the type byte.
    out.write_all(&named[..1])?;
    out.write_all(&named[3..])?;
    Ok(())
}

/// A stack as the wire carries it: a registry id and a count.
///
/// The name is resolved to an id by the caller, because [`crate::item`] is the
/// model and this file is the encoding, and the item table is not always
/// present — a checkout without `data/vanilla/items.json` has none, and the
/// honest answer there is an empty slot rather than a guessed id.
///
/// `patch` is the stack's `DataComponentPatch` already encoded — `None` for
/// the empty one, which is two zero varints. Encoded at [`to_wire`] because
/// that is where the item table is, and kept as bytes so a stack cloned into
/// three packets is encoded once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireStack {
    pub id: i32,
    pub count: i32,
    pub patch: Option<std::sync::Arc<[u8]>>,
}

impl WireStack {
    /// A prototype stack: id, count, empty patch.
    #[must_use]
    pub const fn plain(id: i32, count: i32) -> Self {
        Self {
            id,
            count,
            patch: None,
        }
    }
}

/// A stack plus the components it differs from its item's prototype by.
///
/// A separate type from [`WireStack`] because the patch is not `Copy` and
/// every existing caller wants the cheap one: 39 of the 40 stacks in an
/// inventory sweep are prototypes, and making them all carry an allocation
/// for the one that is not would be a cost paid on every menu packet.
#[derive(Debug, Clone, PartialEq)]
pub struct PatchedStack {
    pub stack: WireStack,
    pub patch: crate::patch::Patch,
}

/// One slot of `ClientboundContainerSetContentPacket`'s item list.
///
/// `ItemStack.OPTIONAL_STREAM_CODEC`: an empty stack is the single varint 0
/// and nothing else; a present one is count, item registry id, then the
/// `DataComponentPatch` — two varints, added and removed, both 0 for a stack
/// carrying only its item's prototype components.
///
/// **The count used to be a hardcoded 1** and was right by luck: nothing could
/// put a stack of more than one anywhere, because `set_creative_mode_slot` is
/// the only way anything entered this server's world and
/// `tools/oracle-runner` writes `1` into every one it sends. A container click
/// can now make a stack of any size, and the count reaches the client's
/// `container_set_slot` digest, so it is written for real.
fn write_item(out: &mut Vec<u8>, item: Option<WireStack>) -> io::Result<()> {
    match item {
        None => write_varint(out, 0),
        Some(s) => {
            write_varint(out, s.count)?;
            write_varint(out, s.id)?;
            match &s.patch {
                Some(bytes) => {
                    out.extend_from_slice(bytes);
                    Ok(())
                }
                // `DataComponentPatch.EMPTY` — and it means "exactly this
                // item's prototype components", not "none". See
                // [`crate::patch`] for why the difference matters.
                None => {
                    write_varint(out, 0)?;
                    write_varint(out, 0)
                }
            }
        }
    }
}

/// One stack under `ItemStack.OPTIONAL_STREAM_CODEC`, on its own — the
/// value an `ItemEntity`'s `DATA_ITEM` carries.
#[must_use]
pub fn item_stack_bytes(item: Option<WireStack>) -> Vec<u8> {
    let mut out = Vec::new();
    // A `Vec` write cannot fail.
    let _ = write_item(&mut out, item);
    out
}

/// The same slot with a real [`crate::patch::Patch`].
///
/// Kept beside [`write_item`] rather than replacing it: the two produce
/// identical bytes for an empty patch — asserted in this module's tests — so
/// the split costs nothing and every existing call site keeps the shape it
/// was written with.
///
/// # Errors
/// If a component in the patch has no registry id, which
/// [`crate::patch::Patch::write`] refuses rather than skipping.
pub fn write_patched_item(
    out: &mut Vec<u8>,
    item: Option<&PatchedStack>,
    type_id: &dyn Fn(&str) -> Option<i32>,
) -> io::Result<()> {
    match item {
        None => write_varint(out, 0),
        Some(p) => {
            write_varint(out, p.stack.count)?;
            write_varint(out, p.stack.id)?;
            p.patch.write(out, type_id)
        }
    }
}

/// `ClientboundContainerSetSlotPacket` — container id, state id, slot, stack.
///
/// The slot is a **short**, and the container id and state id are varints. The
/// carried-item form of this packet is `containerId = -1`, `slot = -1`; it is
/// not produced here, because in 26.2 the cursor has its own packet — see
/// [`set_cursor_item_body`].
///
/// # Errors
/// Only on a write to the buffer, which cannot fail for a `Vec`.
pub fn container_set_slot_body(
    container_id: i32,
    state_id: i32,
    slot: i16,
    item: Option<WireStack>,
) -> io::Result<Vec<u8>> {
    let mut body = Vec::new();
    write_varint(&mut body, container_id)?;
    write_varint(&mut body, state_id)?;
    body.extend_from_slice(&slot.to_be_bytes());
    write_item(&mut body, item)?;
    Ok(body)
}

/// `ClientboundSetPlayerInventoryPacket` — one `Inventory` cell, on the
/// player's inventory rather than on a menu.
///
/// Its `STREAM_CODEC` is a two-element composite: `ByteBufCodecs.VAR_INT` for
/// the slot, then `ItemStack.OPTIONAL_STREAM_CODEC` — the same stack encoding
/// every container packet here uses.
///
/// **The slot is a varint and it is an `Inventory` index**, not a menu slot:
/// `createInventoryUpdatePacket(int)` reads `getItem(slot)` straight off
/// `Inventory`, and its one caller — `placeItemBackInInventory` — passes the
/// index it just wrote. So the hotbar is 0..9 here where the same cell is
/// 36..45 in a menu, and the two must not be confused.
///
/// **There is no state id**, which is the whole reason this is a separate
/// writer rather than a flag on [`container_set_slot_body`]: this packet is
/// not a menu packet and neither reads nor advances any menu's counter.
///
/// # Errors
/// Only on a write to the buffer, which cannot fail for a `Vec`.
pub fn set_player_inventory_body(slot: i32, item: Option<WireStack>) -> io::Result<Vec<u8>> {
    let mut body = Vec::new();
    write_varint(&mut body, slot)?;
    write_item(&mut body, item)?;
    Ok(body)
}

/// `ClientboundSetCursorItemPacket` — the stack on the cursor, alone.
///
/// **This is not a `container_set_slot` and the difference decides packet
/// counts.** `ServerPlayer`'s `ContainerSynchronizer.sendCarriedChange`
/// constructs a `ClientboundSetCursorItemPacket` and — unlike
/// `sendSlotChange` — does **not** call `incrementStateId`. A server that
/// answered the cursor with a slot packet would run one state id ahead of
/// vanilla for the rest of the menu's life.
///
/// **Since 2026-08-11 the corpus catches that as an extra packet rather than
/// as a standing offset**, and it is worth knowing which:
/// `compare::rebase_session_counters` rebases `container_set_slot.state` onto
/// the scenario, so the inherited offset is discarded by design while the
/// spurious packet itself — present on one side, absent on the other — still
/// moves the projection, as does the gap it opens between two packets on one
/// menu. The heading above is therefore still exactly right: the difference
/// decides packet counts, and packet counts are what is compared.
///
/// # Errors
/// Only on a write to the buffer, which cannot fail for a `Vec`.
pub fn set_cursor_item_body(item: Option<WireStack>) -> io::Result<Vec<u8>> {
    let mut body = Vec::new();
    write_item(&mut body, item)?;
    Ok(body)
}

/// `ClientboundContainerSetContentPacket`.
///
/// `items` is every slot of the menu in menu order, `carried` is the stack on
/// the cursor.
///
/// # Errors
/// Only on a write to the buffer, which cannot fail for a `Vec`.
pub fn container_set_content_body(
    container_id: i32,
    state_id: i32,
    items: &[Option<WireStack>],
    carried: Option<WireStack>,
) -> io::Result<Vec<u8>> {
    let mut body = Vec::new();
    write_varint(&mut body, container_id)?;
    write_varint(&mut body, state_id)?;
    write_varint(&mut body, i32::try_from(items.len()).unwrap_or(0))?;
    for item in items {
        write_item(&mut body, item.clone())?;
    }
    write_item(&mut body, carried)?;
    Ok(body)
}

/// `ClientboundContainerSetDataPacket` — container id varint, then **two
/// shorts**, not varints.
///
/// The three container packets do not agree with each other about this:
/// `containerId` is a varint in all of them, `container_set_content`'s state
/// id is a varint, and this packet's `id` and `value` are 16-bit. Read from
/// `ClientboundContainerSetDataPacket.write`, offsets 0..26.
///
/// # Errors
/// Only on a write to the buffer, which cannot fail for a `Vec`.
pub fn container_set_data_body(container_id: i32, id: i16, value: i16) -> io::Result<Vec<u8>> {
    let mut body = Vec::new();
    write_varint(&mut body, container_id)?;
    body.extend_from_slice(&id.to_be_bytes());
    body.extend_from_slice(&value.to_be_bytes());
    Ok(body)
}

/// A model stack as the wire carries it, or `None` for an empty slot.
///
/// **`None` for an item the table does not know, and that is a deliberate
/// silence rather than a fallback.** Without `data/vanilla/items.json` there
/// are no ids at all, and inventing one would put a wrong item on the client's
/// screen; an empty slot is at least honestly empty. The same path already
/// makes a player able to dig but not build, and says so in
/// [`crate::world_send::Session`]'s `items` field.
///
/// This replaced `menu_items`, which synthesised a slot vector out of the
/// hotbar alone and filled everything else with `None`. That was right while
/// nothing could put an item anywhere else; the slot vector now comes from the
/// real storage — [`crate::click::Slots`] reads the block entity, the crafting
/// grid and all 36 inventory slots — so this only converts.
#[must_use]
pub fn to_wire(
    stack: &crate::item::ItemStack,
    items: Option<&crate::player::ItemIds>,
) -> Option<WireStack> {
    if stack.is_empty() {
        return None;
    }
    let id = items?.id_of(stack.item())?;
    let patch = stack.patch();
    let patch = if patch.is_empty() {
        None
    } else {
        encode_patch(patch, items?)
    };
    Some(WireStack {
        id,
        count: stack.count(),
        patch,
    })
}

/// A non-empty patch as the bytes `DataComponentPatch.STREAM_CODEC` writes.
///
/// **`None` sends the prototype, and says so once on stderr.** It happens
/// only when a table is missing — a component type or a holder the tables
/// do not name — and the choice is between a prototype on the client and a
/// packet whose remaining bytes the client reads as the next field. The
/// first is wrong about one stack; the second disconnects the player.
fn encode_patch(
    patch: &crate::patch::Patch,
    items: &crate::player::ItemIds,
) -> Option<std::sync::Arc<[u8]>> {
    let holders = crate::patch::HolderIds::shared();
    let mut out = Vec::new();
    match patch.write_with(&mut out, &|n| items.component_type(n), &|r, n| {
        holders
            .and_then(|h| h.id(r, n))
            .or_else(|| crate::workstation::holder_id(r, n))
    }) {
        Ok(()) => Some(out.into()),
        Err(e) => {
            static SAID: std::sync::Once = std::sync::Once::new();
            SAID.call_once(|| eprintln!("a component patch was sent as its prototype: {e}"));
            None
        }
    }
}

/// `ItemStack.OPTIONAL_STREAM_CODEC` for a model stack, patch included —
/// the value of every clientbound stack field outside a menu packet, such as
/// an `ItemEntity`'s `DATA_ITEM` (`EntityDataSerializers.ITEM_STACK`) or an
/// equipment slot. An empty stack, or one whose item the table does not
/// know, is the single varint 0, as [`to_wire`] decides.
///
/// # Errors
/// Only on a write to the buffer, which cannot fail for a `Vec`.
pub fn write_optional_stack(
    out: &mut Vec<u8>,
    stack: &crate::item::ItemStack,
    items: Option<&crate::player::ItemIds>,
) -> io::Result<()> {
    write_item(out, to_wire(stack, items))
}

/// The tables a stack needs on the wire. Only `items` is read by the
/// encoder ([`to_wire`]); the component and holder ids come from the item
/// table and [`crate::patch::HolderIds`]. `props` and `enchantments` are what
/// a workstation reads beside it.
#[derive(Clone, Copy, Default)]
pub struct Encoding<'a> {
    pub items: Option<&'a crate::player::ItemIds>,
    pub props: Option<&'a crate::itemprops::ItemProps>,
    pub enchantments: Option<&'a crate::enchantment::Registry>,
}

impl std::fmt::Debug for Encoding<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Encoding").finish_non_exhaustive()
    }
}

/// `ItemStack.OPTIONAL_STREAM_CODEC` for a real stack, components included.
///
/// # Errors
/// Only on a write to the buffer.
pub fn write_stack(
    out: &mut Vec<u8>,
    stack: &crate::item::ItemStack,
    enc: &Encoding<'_>,
) -> io::Result<()> {
    // One encoder: [`to_wire`] writes the stack's patch through
    // [`crate::patch::HolderIds`], whatever set it — a workstation, a loot
    // roll or a creative client.
    write_item(out, to_wire(stack, enc.items))
}

/// [`container_set_slot_body`] over a real stack.
///
/// # Errors
/// Only on a write to the buffer.
pub fn container_set_slot_stack_body(
    container_id: i32,
    state_id: i32,
    slot: i16,
    stack: &crate::item::ItemStack,
    enc: &Encoding<'_>,
) -> io::Result<Vec<u8>> {
    let mut body = Vec::new();
    write_varint(&mut body, container_id)?;
    write_varint(&mut body, state_id)?;
    body.extend_from_slice(&slot.to_be_bytes());
    write_stack(&mut body, stack, enc)?;
    Ok(body)
}

/// [`set_cursor_item_body`] over a real stack.
///
/// # Errors
/// Only on a write to the buffer.
pub fn set_cursor_item_stack_body(
    stack: &crate::item::ItemStack,
    enc: &Encoding<'_>,
) -> io::Result<Vec<u8>> {
    let mut body = Vec::new();
    write_stack(&mut body, stack, enc)?;
    Ok(body)
}

/// [`set_player_inventory_body`] over a real stack.
///
/// # Errors
/// Only on a write to the buffer.
pub fn set_player_inventory_stack_body(
    slot: i32,
    stack: &crate::item::ItemStack,
    enc: &Encoding<'_>,
) -> io::Result<Vec<u8>> {
    let mut body = Vec::new();
    write_varint(&mut body, slot)?;
    write_stack(&mut body, stack, enc)?;
    Ok(body)
}

/// [`container_set_content_body`] over real stacks.
///
/// # Errors
/// Only on a write to the buffer.
pub fn container_set_content_stack_body(
    container_id: i32,
    state_id: i32,
    items: &[crate::item::ItemStack],
    carried: &crate::item::ItemStack,
    enc: &Encoding<'_>,
) -> io::Result<Vec<u8>> {
    let mut body = Vec::new();
    write_varint(&mut body, container_id)?;
    write_varint(&mut body, state_id)?;
    write_varint(&mut body, i32::try_from(items.len()).unwrap_or(0))?;
    for item in items {
        write_stack(&mut body, item, enc)?;
    }
    write_stack(&mut body, carried, enc)?;
    Ok(body)
}

/// Every stack of a menu, converted for the content packet.
#[must_use]
pub fn to_wire_all(
    stacks: &[crate::item::ItemStack],
    items: Option<&crate::player::ItemIds>,
) -> Vec<Option<WireStack>> {
    stacks.iter().map(|s| to_wire(s, items)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sequence the four menu checks see, and the two ends of the wrap.
    #[test]
    fn the_container_counter_runs_one_to_a_hundred_and_never_zero() {
        let mut c = 0;
        let mut seen = Vec::new();
        for _ in 0..4 {
            c = next_container_counter(c);
            seen.push(c);
        }
        assert_eq!(
            seen,
            vec![1, 2, 3, 4],
            "the four menu checks, in corpus order"
        );
        // The wrap. `100 % 100 + 1` is 1, so the sequence never yields 0 —
        // which matters because 0 is the player's own inventory menu.
        assert_eq!(next_container_counter(99), 100);
        assert_eq!(next_container_counter(100), 1);
    }

    /// A fresh menu's first content packet carries state 1, not state 0.
    #[test]
    fn a_fresh_menus_first_state_id_is_one() {
        assert_eq!(increment_state_id(0), 1);
        // And it wraps at 15 bits rather than overflowing.
        assert_eq!(increment_state_id(32767), 0);
    }

    #[test]
    fn open_screen_is_two_varints_and_a_network_nbt_component() {
        let b = open_screen_body(3, 12, "container.crafting").expect("buf");
        assert_eq!(b[0], 3, "container id");
        assert_eq!(b[1], 12, "menu type");
        // TAG_Compound, then TAG_String named `translate`.
        assert_eq!(b[2], 10, "the component is a compound, unnamed root");
        let rest = String::from_utf8_lossy(&b[3..]).to_string();
        assert!(rest.contains("translate"), "key: {rest}");
        assert!(rest.contains("container.crafting"), "value: {rest}");
    }

    /// The empty-stack encoding is one byte, and a present stack is four.
    #[test]
    fn an_empty_slot_is_a_single_zero() {
        let b = container_set_content_body(1, 1, &[None, None], None).expect("buf");
        // window, state, count, two empties, carried empty.
        assert_eq!(b, vec![1, 1, 2, 0, 0, 0]);
        let one = WireStack::plain(9, 1);
        let b = container_set_content_body(1, 1, &[Some(one)], None).expect("buf");
        assert_eq!(b, vec![1, 1, 1, 1, 9, 0, 0, 0]);
    }

    /// **The count reaches the wire.** It was a hardcoded 1 for as long as
    /// nothing could make a stack of more than one; a container click can, and
    /// the count lands inside the `rest` digest that every container check
    /// compares. A writer that still emitted 1 would be invisible to a slot
    /// number and a state id and wrong in the digest.
    #[test]
    fn a_stack_of_more_than_one_says_so_on_the_wire() {
        let four = WireStack::plain(9, 4);
        let b = container_set_slot_body(7, 3, 0, Some(four.clone())).expect("buf");
        // window 7, state 3, slot 0 as a short, then count 4, id 9, 0, 0.
        assert_eq!(b, vec![7, 3, 0, 0, 4, 9, 0, 0]);
        // The cursor packet is the stack alone — no window, no state id.
        let b = set_cursor_item_body(Some(four)).expect("buf");
        assert_eq!(b, vec![4, 9, 0, 0]);
        assert_eq!(set_cursor_item_body(None).expect("buf"), vec![0]);
    }

    /// A slot number is a **short**, so a negative one round-trips rather
    /// than becoming a large positive. `-1` is the carried-item form of the
    /// packet and a varint would encode it as five bytes of nonsense.
    #[test]
    fn the_slot_number_is_a_signed_short() {
        let b = container_set_slot_body(1, 1, -1, None).expect("buf");
        assert_eq!(b, vec![1, 1, 0xff, 0xff, 0]);
    }

    /// `container_set_data`'s two fields are shorts and its container id is
    /// not — the packet a uniform reading of the three would get wrong.
    #[test]
    fn container_set_data_is_a_varint_and_two_shorts() {
        let b = container_set_data_body(2, 1, 300).expect("buf");
        assert_eq!(b, vec![2, 0, 1, 1, 44], "varint 2, short 1, short 300");
    }

    /// An empty stack converts to an absent slot and a real one carries its
    /// own count, not a 1.
    #[test]
    fn conversion_to_the_wire_keeps_the_count_and_drops_empties() {
        let ids = crate::player::ItemIds::parse(&serde_json::json!({
            "items": { "minecraft:stone": 42 }
        }))
        .expect("a one-item table");
        assert_eq!(
            to_wire(
                &crate::item::ItemStack::new("minecraft:stone", 5),
                Some(&ids)
            ),
            Some(WireStack::plain(42, 5))
        );
        assert_eq!(to_wire(&crate::item::ItemStack::empty(), Some(&ids)), None);
        // An item the table has never heard of is an empty slot, not a
        // guessed id.
        assert_eq!(
            to_wire(
                &crate::item::ItemStack::one("minecraft:beehive"),
                Some(&ids)
            ),
            None
        );
        // And with no table at all, everything is empty rather than wrong.
        assert_eq!(
            to_wire(&crate::item::ItemStack::one("minecraft:stone"), None),
            None
        );
    }

    /// The grid split, which is what tells a crafting menu from a container
    /// of the same size.
    #[test]
    fn only_a_crafting_menu_has_grid_slots() {
        let chest = menu_for_state("minecraft:chest").expect("chest");
        let crafting = menu_for_state("minecraft:crafting_table").expect("crafting");
        assert_eq!(chest.kind, MenuKind::Container);
        assert_eq!(chest.grid_slots(), 0);
        assert_eq!(crafting.kind, MenuKind::Crafting);
        assert_eq!(
            crafting.grid_slots(),
            9,
            "ten container slots is one result and nine grid"
        );
    }

    /// The three slot counts, which are the field a server that hardcoded one
    /// container would get wrong.
    #[test]
    fn the_three_menus_differ_in_type_and_in_size() {
        let chest = menu_for_state("minecraft:chest").expect("chest");
        let furnace = menu_for_state("minecraft:furnace").expect("furnace");
        let crafting = menu_for_state("minecraft:crafting_table").expect("crafting");
        assert_eq!((chest.slots, chest.data_slots), (63, 0));
        assert_eq!((furnace.slots, furnace.data_slots), (39, 4));
        assert_eq!((crafting.slots, crafting.data_slots), (46, 0));
        assert_ne!(chest.menu_type, furnace.menu_type);
        assert_ne!(chest.title, crafting.title);
        // A block with no menu is not a menu with no block.
        assert_eq!(menu_for_state("minecraft:stone"), None);
        assert_eq!(menu_for_state("minecraft:barrel"), None);
    }

    /// The two furnace subclasses share the furnace's shape and differ in
    /// type and title; the brewing stand has a slot shape of its own.
    #[test]
    fn the_furnace_family_and_the_brewing_stand_each_open_their_own_menu() {
        let furnace = menu_for_state("minecraft:furnace").expect("furnace");
        for (state, ty, title) in [
            (
                "minecraft:blast_furnace[facing=north,lit=false]",
                "minecraft:blast_furnace",
                "container.blast_furnace",
            ),
            (
                "minecraft:smoker[facing=north,lit=true]",
                "minecraft:smoker",
                "container.smoker",
            ),
        ] {
            let spec = menu_for_state(state).expect(state);
            assert_eq!((spec.menu_type, spec.title), (ty, title));
            assert_eq!(
                (spec.slots, spec.container_slots, spec.data_slots),
                (furnace.slots, furnace.container_slots, furnace.data_slots)
            );
        }
        let brewing = menu_for_state("minecraft:brewing_stand").expect("brewing");
        assert_eq!(
            (brewing.slots, brewing.container_slots, brewing.data_slots),
            (41, 5, 2)
        );
        assert_eq!(brewing.title, "container.brewing");
    }

    /// `742`'s layout: chests at x = 1 (`left`) and x = 2 (`right`), both
    /// facing north. The right half is the compound's first container
    /// whichever half was clicked, and a neighbour that does not match —
    /// another block, another facing, the same type — leaves a chest single.
    #[test]
    fn a_double_chest_is_right_half_first() {
        let left = "minecraft:chest[facing=north,type=left,waterlogged=false]";
        let right = "minecraft:chest[facing=north,type=right,waterlogged=false]";
        let world = |p: BlockPos| -> String {
            match p {
                (1, 0, 0) => left.to_string(),
                (2, 0, 0) => right.to_string(),
                _ => "minecraft:air".to_string(),
            }
        };
        assert_eq!(
            double_chest(left, (1, 0, 0), &world),
            Some(((2, 0, 0), (1, 0, 0)))
        );
        assert_eq!(
            double_chest(right, (2, 0, 0), &world),
            Some(((2, 0, 0), (1, 0, 0)))
        );
        let lonely = |_: BlockPos| "minecraft:air".to_string();
        assert_eq!(double_chest(left, (1, 0, 0), &lonely), None);
        let twin = |_: BlockPos| left.to_string();
        assert_eq!(
            double_chest(left, (1, 0, 0), &twin),
            None,
            "two lefts do not pair"
        );
        let single = "minecraft:chest[facing=north,type=single,waterlogged=false]";
        assert_eq!(double_chest(single, (1, 0, 0), &world), None);
        assert_eq!((DOUBLE_CHEST.slots, DOUBLE_CHEST.container_slots), (90, 54));
    }

    /// Every row the corpus's `740`-`755` recorded from vanilla: registry
    /// name, total slots and data-slot count, as `open_screen`,
    /// `container_set_content` and the count of `container_set_data` packets
    /// in `oracle-work/gameplay/vanilla.json` on `train-a` (2026-09-30) have
    /// them.
    #[test]
    fn the_block_entity_menus_have_vanillas_shapes() {
        let rows = [
            (
                "minecraft:dispenser[facing=north,triggered=false]",
                "minecraft:generic_3x3",
                45,
                0,
            ),
            (
                "minecraft:hopper[enabled=true,facing=down]",
                "minecraft:hopper",
                41,
                0,
            ),
            (
                "minecraft:shulker_box[facing=up]",
                "minecraft:shulker_box",
                63,
                0,
            ),
            (
                "minecraft:red_shulker_box[facing=up]",
                "minecraft:shulker_box",
                63,
                0,
            ),
            (
                "minecraft:blast_furnace[facing=north,lit=false]",
                "minecraft:blast_furnace",
                39,
                4,
            ),
            (
                "minecraft:smoker[facing=north,lit=false]",
                "minecraft:smoker",
                39,
                4,
            ),
            ("minecraft:brewing_stand", "minecraft:brewing_stand", 41, 2),
            ("minecraft:beacon", "minecraft:beacon", 37, 3),
            (
                "minecraft:enchanting_table",
                "minecraft:enchantment",
                38,
                10,
            ),
            (
                "minecraft:crafter[crafting=false,orientation=north_up,triggered=false]",
                "minecraft:crafter_3x3",
                46,
                10,
            ),
        ];
        for (state, menu_type, slots, data) in rows {
            let spec = menu_for_state(state).unwrap_or_else(|| panic!("{state} opens nothing"));
            assert_eq!(
                (spec.menu_type, spec.slots, spec.data_slots),
                (menu_type, slots, data),
                "{state}"
            );
        }
        let enchant = menu_for_state("minecraft:enchanting_table").expect("enchant");
        let fresh: Vec<i16> = (0..10).map(|i| enchant.initial_data(i)).collect();
        assert_eq!(fresh, [0, 0, 0, 0, -1, -1, -1, -1, -1, -1]);
    }
}

/// The patched slot encoder against the unpatched one.
#[cfg(test)]
mod patched_item_tests {
    use super::*;

    /// **An empty patch produces the identical bytes.** This is what makes
    /// the two writers a split rather than a fork: if they diverged on the
    /// common case, every menu packet in the corpus would move.
    #[test]
    fn an_empty_patch_is_byte_identical_to_the_old_encoder() {
        for stack in [WireStack::plain(1, 1), WireStack::plain(900, 64)] {
            let mut old = Vec::new();
            write_item(&mut old, Some(stack.clone())).expect("writes");
            let mut new = Vec::new();
            write_patched_item(
                &mut new,
                Some(&PatchedStack {
                    stack: stack.clone(),
                    patch: crate::patch::Patch::EMPTY,
                }),
                &|_| Some(0),
            )
            .expect("writes");
            assert_eq!(old, new, "for {stack:?}");
        }
        let mut old = Vec::new();
        write_item(&mut old, None).expect("writes");
        let mut new = Vec::new();
        write_patched_item(&mut new, None, &|_| Some(0)).expect("writes");
        assert_eq!(old, new, "and an empty slot is the single varint 0");
    }

    /// A model stack carrying a patch reaches the wire through [`to_wire`]
    /// with the patch encoded, in a menu packet and in the bare optional
    /// form an entity's `DATA_ITEM` uses alike.
    #[test]
    fn a_patched_model_stack_writes_its_patch() {
        let ids = crate::player::ItemIds::parse(&serde_json::json!({
            "items": {"minecraft:iron_sword": 900},
            "data_component_types": {"minecraft:damage": 3}
        }))
        .expect("parsed");
        let worn = crate::item::ItemStack::one("minecraft:iron_sword")
            .with_patch(crate::patch::Patch::default().with(crate::patch::Component::Damage(224)));
        let mut b = Vec::new();
        write_optional_stack(&mut b, &worn, Some(&ids)).expect("writes");
        // count 1, id 900 (0x84 0x07), added 1, removed 0, damage, 224.
        assert_eq!(b, vec![1, 0x84, 0x07, 1, 0, 3, 0xE0, 0x01]);
        let mut fresh = Vec::new();
        let plain = crate::item::ItemStack::one("minecraft:iron_sword");
        write_optional_stack(&mut fresh, &plain, Some(&ids)).expect("writes");
        assert_eq!(fresh, vec![1, 0x84, 0x07, 0, 0]);
        assert!(!crate::item::ItemStack::is_same_item_same_components(
            &worn, &plain
        ));
        // `split` keeps the patch on both halves.
        let mut two = worn.copy_with_count(2);
        let one = two.split(1);
        assert_eq!(one.patch(), worn.patch());
        assert_eq!(two.patch(), worn.patch());
    }

    /// A damaged stack is not the prototype, and its bytes say so — which is
    /// the whole point: `container_set_slot`'s digest is what a corpus check
    /// on a worn shovel reads.
    #[test]
    fn a_damaged_stack_differs_from_a_fresh_one_on_the_wire() {
        let stack = WireStack::plain(900, 1);
        let mut fresh = Vec::new();
        write_item(&mut fresh, Some(stack.clone())).expect("writes");
        let mut worn = Vec::new();
        write_patched_item(
            &mut worn,
            Some(&PatchedStack {
                stack: stack.clone(),
                patch: crate::patch::Patch::default().with(crate::patch::Component::Damage(1)),
            }),
            &|n| (n == "minecraft:damage").then_some(3),
        )
        .expect("writes");
        assert_ne!(
            fresh, worn,
            "a shovel that has dug once must not encode as one that has not"
        );
        assert_eq!(worn, vec![1, 132, 7, 1, 0, 3, 1]);
    }
}
