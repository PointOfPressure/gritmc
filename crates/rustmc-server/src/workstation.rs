//! The workstation menus: what each one *computes* once it is open.
//!
//! `crate::menu` opens all seven of these — anvil, grindstone, loom, smithing
//! table, cartography table, stonecutter, enchanting table — and
//! `crate::click` moves items in and out of them. Neither knows that putting a
//! stone into a stonecutter is supposed to make anything happen. This module
//! is that half: each menu's `slotsChanged`, its result slot's `mayPlace` and
//! `onTake`, its `clickMenuButton`, and its data slots, transcribed from the
//! menu classes in the pinned `server-26.2.jar` (`javap -p -c -constants`,
//! JDK 25).
//!
//! # When `slotsChanged` runs, which is not when the crafting table's does
//!
//! Every one of these menus builds its inputs on an anonymous `SimpleContainer`
//! subclass whose `setChanged` calls `menu.slotsChanged(this)` —
//! `StonecutterMenu$1.setChanged` is `super.setChanged(); slotsChanged(this);
//! slotUpdateListener.run()`. `SimpleContainer.setItem` calls `setChanged` on
//! every write and `removeItem` calls it when it removed anything. So the
//! recompute is **synchronous with the write**, inside `doClick`, and a later
//! read in the same click sees the recomputed state. [`crate::click::Slots`]
//! calls [`slots_changed`] from its storage write for that reason, rather than
//! deferring it to the end of the click the way the crafting grid's direct
//! send is deferred.
//!
//! The result slots are different containers. A stonecutter's result is a
//! `ResultContainer`, whose `setChanged` is empty, so writing the result never
//! recomputes anything — which is what lets `setupResultSlot` write it from
//! inside a recompute without recursing.
//!
//! # Packets
//!
//! Nothing here sends. Every slot change these menus make reaches the client
//! through `broadcastChanges`' diff of slots, carried stack and **data slots**
//! — the last of which this server did not emit at all before this module,
//! because no menu it could open ever moved one off its initial value. What
//! does leave the menu immediately — a level event, a sound — is collected in
//! [`Env::effects`] for the caller, which sends it before the slot replies,
//! because `Level.levelEvent` and `Level.playSound` broadcast at the call and
//! `broadcastChanges` runs at the end of the handler.

use crate::interact::BlockPos;
use crate::item::ItemStack;

/// Which workstation a menu is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Station {
    Anvil,
    Grindstone,
    Loom,
    Smithing,
    Cartography,
    Stonecutter,
    Enchanting,
}

impl Station {
    /// The menu slot index of the result slot, or `None` for a menu that has
    /// none.
    ///
    /// Read off each constructor's `addSlot` order: `StonecutterMenu` adds the
    /// input then `StonecutterMenu$2` over its `ResultContainer`;
    /// `ItemCombinerMenu` adds the slot definition's inputs and then its
    /// `resultSlotIndex` (2 for the anvil's two inputs, 3 for the smithing
    /// table's three); `GrindstoneMenu` adds two inputs then `GrindstoneMenu$4`;
    /// `LoomMenu` banner, dye, pattern, result; `CartographyTableMenu` map,
    /// additional, result. `EnchantmentMenu` has no output slot at all — it
    /// enchants the item in place.
    #[must_use]
    pub const fn result_slot(self) -> Option<usize> {
        match self {
            Self::Stonecutter => Some(1),
            Self::Anvil | Self::Grindstone | Self::Cartography => Some(2),
            Self::Smithing | Self::Loom => Some(3),
            Self::Enchanting => None,
        }
    }

    /// Whether menu slot `i` (a container slot) is the result.
    #[must_use]
    pub fn is_result(self, i: usize) -> bool {
        self.result_slot() == Some(i)
    }

    /// The data slots' values on a freshly constructed menu, before
    /// `sendAllDataToRemote` reads them.
    ///
    /// `DataSlot.standalone()` starts at 0, and an `int[]` from `newarray`
    /// is zero-filled, so every slot is 0 except where the constructor writes
    /// one: `EnchantmentMenu` fills `enchantClue` and `levelClue` with `-1`
    /// (`iconst_m1; iastore`, offsets 43-77) and sets the seed slot to
    /// `player.getEnchantmentSeed()` (offsets 179-192).
    #[must_use]
    pub fn initial_data(self, len: usize, enchantment_seed: i32) -> Vec<i32> {
        let mut out = vec![0; len];
        if self == Self::Enchanting && len == 10 {
            out[3] = enchantment_seed;
            for v in &mut out[4..10] {
                *v = -1;
            }
        }
        out
    }
}

/// Something a menu did that leaves it at once rather than through
/// `broadcastChanges`.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// `Level.levelEvent(null, event, pos, data)` — broadcast to every player
    /// within 64 blocks, the actor included.
    LevelEvent {
        event: i32,
        pos: BlockPos,
        data: i32,
    },
    /// `Level.playSound(null, pos, event, source, volume, pitch)`.
    Sound(crate::sounds::Sound),
}

/// What a workstation reads from outside its own menu.
///
/// Gathered on [`crate::click::Slots`] so the transcribed methods can reach
/// it without every caller threading another argument. `Default` is a menu
/// that can compute nothing that needs the world, which is what the unit
/// tests of the storage half want.
#[derive(Debug, Default)]
pub struct Env<'a> {
    /// The block the menu was opened on — `ContainerLevelAccess`'s position.
    /// `None` is `ContainerLevelAccess.NULL`, whose `execute` does nothing.
    pub pos: Option<BlockPos>,
    /// `Level.getGameTime()`.
    pub game_time: i64,
    /// `Player.hasInfiniteMaterials()`.
    pub creative: bool,
    /// Effects to send before the slot replies, in call order.
    pub effects: Vec<Effect>,
    /// The enchantment registry, for the anvil, the grindstone and the
    /// enchanting table.
    pub enchantments: Option<&'a crate::enchantment::Registry>,
    /// `Player.enchantmentSeed`, which outlives every menu.
    pub enchantment_seed: Option<&'a mut i32>,
    /// How many of `EnchantingTableBlock.BOOKSHELF_OFFSETS` pass
    /// `isValidBookShelf` around [`Self::pos`], counted by the caller.
    pub bookshelves: i32,
    /// Set when `ServerPlayer.onEnchantmentPerformed` ran: it writes
    /// `lastSentExp = -1` (offsets 6-8), so the next player tick sends a
    /// `set_experience` whether or not the numbers moved.
    pub experience_dirty: bool,
    /// The `levels` that `onEnchantmentPerformed` took, when
    /// [`Self::experience_dirty`] is set — applied to the player's one
    /// experience store (`crate::experience`) by the caller.
    pub levels_spent: i32,
    /// Packets a menu sent **directly** from inside `clicked`, in send order —
    /// ahead of everything `broadcastChanges` sends at the end of the handler.
    ///
    /// Only `CraftingMenu.slotChangedCraftingGrid` sends this way, and the
    /// one caller that reaches it from inside a click rather than after it is
    /// `ResultSlot.onTake`: every cell it consumes is a
    /// `TransientCraftingContainer.removeItem` (or `setItem`), and each of
    /// those calls `slotsChanged` on the spot. So taking a crafted result
    /// sends one result-slot packet **per cell consumed**, each on its own
    /// state id, before the click's own corrections.
    pub direct: Vec<crate::click::Reply>,
}

/// The fields each menu keeps beyond its slots and data slots.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    /// `StonecutterMenu.input` — a copy of the input stack as of the last
    /// time its *item* changed. Only the item is ever compared.
    pub stonecutter_input: ItemStack,
    /// `StonecutterMenu.recipesForInput`, as recipe ids in button order.
    pub stonecutter_recipes: Vec<String>,
    /// `StonecutterMenu.lastSoundTime`, a `long` field defaulted to 0.
    pub last_sound_time: i64,
    /// `AnvilMenu.itemName`. `None` is Java's `null`.
    pub anvil_item_name: Option<String>,
    /// `AnvilMenu.repairItemCountCost`.
    pub anvil_repair_item_count_cost: i32,
    /// `AnvilMenu.onlyRenaming`.
    pub anvil_only_renaming: bool,
    /// `LoomMenu.selectablePatterns`, as pattern ids.
    pub loom_patterns: Vec<String>,
}

/// `UI_STONECUTTER_TAKE_RESULT`'s registry name.
const STONECUTTER_TAKE_RESULT: &str = "minecraft:ui.stonecutter.take_result";

/// `menu.slotsChanged(container)` for a write to one of a workstation's
/// **input** containers. Called by [`crate::click::Slots`] immediately after
/// the write, for the module header's reason.
pub fn slots_changed(slots: &mut crate::click::Slots<'_>) {
    match slots.menu.spec.station {
        Some(Station::Stonecutter) => stonecutter_slots_changed(slots),
        Some(Station::Anvil) => anvil_create_result(slots),
        Some(Station::Grindstone) => grindstone_create_result(slots),
        Some(Station::Enchanting) => enchanting_slots_changed(slots),
        Some(Station::Smithing) => smithing_slots_changed(slots),
        Some(Station::Loom) => loom_slots_changed(slots),
        // The others are ported in their own commits; until then a write to
        // their inputs computes nothing, which is what this server did
        // before this module.
        _ => {}
    }
}

/// `Slot.onTake(player, stack)` for a workstation's result slot.
pub fn on_take(slots: &mut crate::click::Slots<'_>, i: usize, taken: &ItemStack) {
    let Some(station) = slots.menu.spec.station else {
        return;
    };
    if !station.is_result(i) {
        return;
    }
    match station {
        Station::Stonecutter => stonecutter_on_take(slots, taken),
        Station::Anvil => anvil_on_take(slots),
        Station::Grindstone => grindstone_on_take(slots),
        Station::Smithing => smithing_on_take(slots),
        Station::Loom => loom_on_take(slots),
        _ => {}
    }
}

/// `AbstractContainerMenu.clickMenuButton(player, id)`. `false` — the
/// default body, `iconst_0; ireturn` — for every menu that has no buttons.
pub fn click_menu_button(slots: &mut crate::click::Slots<'_>, id: i32) -> bool {
    match slots.menu.spec.station {
        Some(Station::Stonecutter) => stonecutter_click_menu_button(slots, id),
        Some(Station::Enchanting) => enchanting_click_menu_button(slots, id),
        Some(Station::Loom) => loom_click_menu_button(slots, id),
        _ => false,
    }
}

/// `Slot.mayPlace(stack)` for a workstation's own slots. The result slot is
/// refused by [`crate::click::Slots`] before this is asked.
///
/// `props` is the item table, for the grindstone's `isDamageableItem`.
#[must_use]
pub fn may_place(
    slots: &crate::click::Slots<'_>,
    station: Station,
    i: usize,
    stack: &ItemStack,
) -> bool {
    let props = slots.inventory.item_props();
    match station {
        // `SmithingMenu.createInputSlotDefinitions`, offsets 30-101: slot 0
        // tests `RecipePropertySet.SMITHING_TEMPLATE`, slot 1
        // `SMITHING_BASE`, slot 2 `SMITHING_ADDITION` — each the union of
        // that ingredient over every smithing recipe.
        Station::Smithing => {
            let key = match i {
                0 => crate::recipes::PropertySetKey::SmithingTemplate,
                1 => crate::recipes::PropertySetKey::SmithingBase,
                _ => crate::recipes::PropertySetKey::SmithingAddition,
            };
            slots
                .recipes
                .is_some_and(|r| r.property_set(key).iter().any(|n| n == stack.item()))
        }
        // `GrindstoneMenu$2.mayPlace` and `$3.mayPlace`, both offsets 0-19:
        // `isDamageableItem() || EnchantmentHelper.hasAnyEnchantments(stack)`.
        Station::Grindstone => {
            props.is_some_and(|p| p.is_damageable_item(stack.item())) || has_any_enchantments(stack)
        }
        // `LoomMenu$3`: `getItem() instanceof BannerItem`; `$4`: `isDyeItem`;
        // `$5`: `isPatternItem`.
        Station::Loom => match i {
            LOOM_BANNER => {
                props.and_then(|p| p.class_of(stack.item()))
                    == Some("net.minecraft.world.item.BannerItem")
            }
            LOOM_DYE => dye_color(stack.item()).is_some(),
            _ => pattern_item_patterns(stack.item()).is_some(),
        },
        // `CartographyTableMenu$3.mayPlace` is `stack.has(MAP_ID)` — which no
        // stack here can carry, so the map slot refuses everything and the
        // table's `setupResultSlot` (every arm of which needs a map's saved
        // data) is unreachable; `$4.mayPlace` is paper, an empty map or a
        // glass pane.
        Station::Cartography => {
            i == 1
                && matches!(
                    stack.item(),
                    "minecraft:paper" | "minecraft:map" | "minecraft:glass_pane"
                )
        }
        // `EnchantmentMenu$3.mayPlace`: `stack.is(Items.LAPIS_LAZULI)`.
        // `EnchantmentMenu$2` does not override it.
        Station::Enchanting => i != ENCHANTING_LAPIS || stack.item() == "minecraft:lapis_lazuli",
        // `AnvilMenu.createInputSlotDefinitions`' two predicates are both
        // `iconst_1; ireturn` (`lambda$createInputSlotDefinitions$0`/`$1`).
        _ => true,
    }
}

/// `Slot.mayPickup(player)` for a workstation slot. `None` is the default
/// `return true`.
#[must_use]
pub fn may_pickup(slots: &crate::click::Slots<'_>, i: usize) -> Option<bool> {
    let station = slots.menu.spec.station?;
    if !station.is_result(i) {
        return None;
    }
    match station {
        // `ItemCombinerMenu$3.mayPickup` -> `AnvilMenu.mayPickup(player,
        // hasItem)`, offsets 0-36: `(hasInfiniteMaterials() || experienceLevel
        // >= cost) && cost > 0`. This server keeps no experience level, so a
        // survival player is read as level 0.
        Station::Anvil => {
            let cost = slots.data(ANVIL_COST);
            Some((slots.env.creative || 0 >= cost) && cost > 0)
        }
        _ => None,
    }
}

/// `Slot.getMaxStackSize()` for a workstation slot, where a menu overrides
/// it. `None` is the default, the container's own maximum.
#[must_use]
pub fn slot_max_stack_size(station: Station, i: usize) -> Option<i32> {
    // `EnchantmentMenu$2.getMaxStackSize` is `iconst_1; ireturn`: one item
    // on the table at a time.
    (station == Station::Enchanting && i == ENCHANTING_ITEM).then_some(1)
}

/// `EnchantmentHelper.hasAnyEnchantments(stack)` — either map non-empty.
fn has_any_enchantments(stack: &ItemStack) -> bool {
    use crate::item::ComponentKind;
    !stack
        .enchantments_of(ComponentKind::Enchantments)
        .is_empty()
        || !stack
            .enchantments_of(ComponentKind::StoredEnchantments)
            .is_empty()
}

/// Whether `item`'s prototype carries `DAMAGE`, for component writes.
fn prototype_damage(slots: &crate::click::Slots<'_>, item: &str) -> bool {
    slots
        .inventory
        .item_props()
        .is_some_and(|p| p.has_prototype_damage(item))
}

// ---------------------------------------------------------------------------
// StonecutterMenu
// ---------------------------------------------------------------------------

const STONECUTTER_INPUT: usize = 0;
const STONECUTTER_RESULT: usize = 1;
/// `selectedRecipeIndex`, the menu's only data slot.
const STONECUTTER_SELECTED: usize = 0;

/// `StonecutterMenu.slotsChanged`, offsets 0-35.
///
/// ```text
///    0-7    stack = inputSlot.getItem()
///    8-19   if (!stack.is(this.input.getItem()))
///   22-27       this.input = stack.copy()
///   30-32       setupRecipeList(stack)
/// ```
///
/// **An item comparison, not a stack comparison.** Adding a second stone to a
/// stone, or taking one away, changes nothing: the recipe list, the selection
/// and the result all survive. Only a different item — or emptying the slot,
/// since `EMPTY.getItem()` is air — resets them.
fn stonecutter_slots_changed(slots: &mut crate::click::Slots<'_>) {
    let stack = slots.peek(STONECUTTER_INPUT);
    if ItemStack::is_same_item(&stack, &slots.menu.ws.stonecutter_input) {
        return;
    }
    slots.menu.ws.stonecutter_input = stack.clone();
    stonecutter_setup_recipe_list(slots, &stack);
}

/// `StonecutterMenu.setupRecipeList`, offsets 0-55.
///
/// The selection goes to `-1` and the result to `EMPTY` **before** the list is
/// rebuilt, and both happen whether or not the new input has any recipes.
fn stonecutter_setup_recipe_list(slots: &mut crate::click::Slots<'_>, stack: &ItemStack) {
    slots.set_data(STONECUTTER_SELECTED, -1);
    slots.put(STONECUTTER_RESULT, ItemStack::empty());
    slots.menu.ws.stonecutter_recipes = if stack.is_empty() {
        Vec::new()
    } else {
        slots.recipes.map_or_else(Vec::new, |r| {
            r.stonecutter_recipes_for(stack)
                .into_iter()
                .map(|x| x.id.clone())
                .collect()
        })
    };
}

/// `StonecutterMenu.isValidRecipeIndex`, offsets 0-20.
fn stonecutter_is_valid_recipe_index(slots: &crate::click::Slots<'_>, id: i32) -> bool {
    usize::try_from(id).is_ok_and(|i| i < slots.menu.ws.stonecutter_recipes.len())
}

/// `StonecutterMenu.clickMenuButton`, offsets 0-35.
///
/// ```text
///    0-12   if (selectedRecipeIndex.get() == id) return false
///   13-18   if (isValidRecipeIndex(id))
///   21-26       selectedRecipeIndex.set(id)
///   29-31       setupResultSlot(id)
///   34-35   return true
/// ```
///
/// So re-pressing the selected button is the one press that is answered with
/// nothing, and an out-of-range press returns **true** having changed nothing
/// — `handleContainerButtonClick` then broadcasts, and finds nothing to send.
fn stonecutter_click_menu_button(slots: &mut crate::click::Slots<'_>, id: i32) -> bool {
    if slots.data(STONECUTTER_SELECTED) == id {
        return false;
    }
    if stonecutter_is_valid_recipe_index(slots, id) {
        slots.set_data(STONECUTTER_SELECTED, id);
        stonecutter_setup_result_slot(slots, id);
    }
    true
}

/// `StonecutterMenu.setupResultSlot`, offsets 0-70, with both lambdas.
///
/// A valid index assembles that recipe over **the input container's slot 0
/// as it is now** (`lambda$setupResultSlot$0`, offsets 19-39) and writes it to
/// the result; anything else writes `EMPTY` (`lambda$setupResultSlot$1`).
/// `StonecutterRecipe.assemble` is `SingleItemRecipe.assemble`, the result
/// template's own stack — the input's components do not travel.
///
/// The trailing `broadcastChanges()` at offset 66 is a send when this runs
/// from a button press and a no-op inside a click, where
/// `suppressRemoteUpdates` is on; the caller's own broadcast covers both.
fn stonecutter_setup_result_slot(slots: &mut crate::click::Slots<'_>, id: i32) {
    let recipe = if !slots.menu.ws.stonecutter_recipes.is_empty()
        && stonecutter_is_valid_recipe_index(slots, id)
    {
        usize::try_from(id)
            .ok()
            .and_then(|i| slots.menu.ws.stonecutter_recipes.get(i).cloned())
    } else {
        None
    };
    let result = recipe
        .and_then(|rid| {
            slots
                .recipes
                .and_then(|r| r.by_id(&rid))
                .map(|r| r.result.clone())
        })
        .unwrap_or_else(ItemStack::empty);
    slots.put(STONECUTTER_RESULT, result);
}

/// `StonecutterMenu$2.onTake`, offsets 0-81.
///
/// ```text
///    0-6    stack.onCraftedBy(player, stack.getCount())     stats only
///   10-21   resultContainer.awardUsedRecipes(player, ...)   recipe book only
///   25-35   removed = inputSlot.remove(1)
///   36-57   if (!removed.isEmpty()) setupResultSlot(selectedRecipeIndex.get())
///   61-70   access.execute(lambda$onTake$0)                 the sound
///   76-78   super.onTake(player, stack)
/// ```
///
/// `inputSlot.remove(1)` runs `slotsChanged` itself, so by offset 54 an input
/// that has just run out has already reset the selection to `-1`, and
/// `setupResultSlot(-1)` writes `EMPTY`. An input with stone left keeps its
/// selection and re-assembles the same result.
///
/// The sound (`lambda$onTake$0`) plays only when `lastSoundTime` differs from
/// the level's game time, at the block, `BLOCKS`, volume 1 and pitch 1 —
/// nothing random — and stamps the time.
fn stonecutter_on_take(slots: &mut crate::click::Slots<'_>, _taken: &ItemStack) {
    let mut input = slots.peek(STONECUTTER_INPUT);
    let removed = input.split(1);
    if !removed.is_empty() {
        slots.put(STONECUTTER_INPUT, input);
        let selected = slots.data(STONECUTTER_SELECTED);
        stonecutter_setup_result_slot(slots, selected);
    }
    if let Some(pos) = slots.env.pos {
        let now = slots.env.game_time;
        if slots.menu.ws.last_sound_time != now {
            slots.env.effects.push(Effect::Sound(crate::sounds::Sound {
                event: STONECUTTER_TAKE_RESULT.into(),
                source: crate::sounds::SOURCE_BLOCKS,
                at: crate::sounds::Eighths::block(pos),
                pos: crate::sounds::block_centre(pos),
                volume: 1.0,
                pitch: 1.0,
                except: None,
            }));
            slots.menu.ws.last_sound_time = now;
        }
    }
}

// ---------------------------------------------------------------------------
// AnvilMenu
// ---------------------------------------------------------------------------

const ANVIL_LEFT: usize = 0;
const ANVIL_RIGHT: usize = 1;
const ANVIL_RESULT: usize = 2;
/// `cost`, the anvil's only data slot.
const ANVIL_COST: usize = 0;
/// `AnvilMenu.MAX_NAME_LENGTH`, the `bipush 50` in `validateName`.
const ANVIL_MAX_NAME_LENGTH: usize = 50;

/// The anvil's view of a stack — `crate::anvil::Input` — read off its
/// components. `getEnchantmentsForCrafting` reads `STORED_ENCHANTMENTS` on an
/// enchanted book and `ENCHANTMENTS` on anything else.
fn anvil_input(stack: &ItemStack) -> crate::anvil::Input {
    use crate::item::ComponentKind;
    if stack.is_empty() {
        return crate::anvil::Input::empty();
    }
    let kind = if stack.item() == "minecraft:enchanted_book" {
        ComponentKind::StoredEnchantments
    } else {
        ComponentKind::Enchantments
    };
    crate::anvil::Input {
        item: stack.item().to_string(),
        count: stack.count(),
        damage: stack.damage_value(),
        repair_cost: stack.repair_cost(),
        enchantments: stack.enchantments_of(kind),
        custom_name: stack.custom_name_text(),
    }
}

/// `AnvilMenu.createResult`, offsets 0-1077, through
/// [`crate::anvil::create_result`], which holds the arithmetic.
///
/// What this adds is the **stack** — the output as `createResult` builds it on
/// `left.copy()`, so the component patch comes out in the order vanilla's
/// `Reference2ObjectArrayMap` does: the left input's own entries where they
/// were, then `setDamageValue` from the repair (offsets 178-433), the rename's
/// `set`/`remove(CUSTOM_NAME)` (756-846), `set(REPAIR_COST)` (1049) and
/// `setEnchantments` (1060), each appended only if new. Then
/// `resultSlots.setItem(0, out)` and `cost.set(..)`.
fn anvil_create_result(slots: &mut crate::click::Slots<'_>) {
    use crate::item::{ComponentKind, ComponentValue};
    let left = slots.peek(ANVIL_LEFT);
    let right = slots.peek(ANVIL_RIGHT);
    let Some(registry) = slots.env.enchantments else {
        // No registry: an anvil that computes nothing, which is what this
        // server did before it had one.
        return;
    };
    let outcome = crate::anvil::create_result(
        registry,
        &anvil_input(&left),
        &anvil_input(&right),
        slots.menu.ws.anvil_item_name.as_deref(),
        slots.env.creative,
    );
    slots.menu.ws.anvil_only_renaming = false;
    slots.menu.ws.anvil_repair_item_count_cost = 0;
    let (result, cost) = match outcome {
        crate::anvil::Outcome::None(_) => (ItemStack::empty(), 0),
        crate::anvil::Outcome::Result(r) => {
            let damageable = prototype_damage(slots, left.item());
            let mut out = left.clone();
            let o = &r.output;
            if o.damage != left.damage_value() {
                let max = registry.max_damage(left.item()).unwrap_or(0);
                out.set_component(
                    ComponentKind::Damage,
                    ComponentValue::Int(o.damage.clamp(0, max)),
                    damageable,
                );
            }
            if o.custom_name != left.custom_name_text() {
                match &o.custom_name {
                    Some(n) => out.set_component(
                        ComponentKind::CustomName,
                        ComponentValue::Text(n.clone()),
                        damageable,
                    ),
                    None => out.remove_component(ComponentKind::CustomName, damageable),
                }
            }
            out.set_component(
                ComponentKind::RepairCost,
                ComponentValue::Int(o.repair_cost),
                damageable,
            );
            let kind = if left.item() == "minecraft:enchanted_book" {
                ComponentKind::StoredEnchantments
            } else {
                ComponentKind::Enchantments
            };
            out.set_component(
                kind,
                ComponentValue::Enchantments(o.enchantments.clone()),
                damageable,
            );
            slots.menu.ws.anvil_only_renaming = r.only_renaming;
            slots.menu.ws.anvil_repair_item_count_cost = r.repair_item_count_cost;
            (out, r.cost)
        }
    };
    slots.set_data(ANVIL_COST, cost);
    slots.put(ANVIL_RESULT, result);
}

/// `AnvilMenu.onTake`, offsets 0-213.
///
/// ```text
///    0-16   if (!hasInfiniteMaterials) giveExperienceLevels(-cost)
///   19-90   if (repairItemCountCost > 0): shrink the right stack by it, or
///           empty it when it has no more than that
///   93-108  else if (!onlyRenaming): empty the right slot
///  113-118  cost.set(0)
///  121-184  the rename's text filter — no effect on any packet
///  185-193  empty the left slot
///  199-208  access.execute(lambda$onTake$0)
/// ```
///
/// `lambda$onTake$0` is `levelEvent(1030, pos, 0)` — `ANVIL_USED` — for a
/// player with infinite materials, and otherwise a 12% chance of damaging the
/// anvil (`player.getRandom().nextFloat() < 0.12F`, offsets 24-36) with 1029
/// or 1030. Only the creative arm is deterministic and only it is written: a
/// survival take raises nothing here, which is a named gap rather than a
/// guessed draw.
fn anvil_on_take(slots: &mut crate::click::Slots<'_>) {
    let rcc = slots.menu.ws.anvil_repair_item_count_cost;
    if rcc > 0 {
        let mut right = slots.peek(ANVIL_RIGHT);
        if !right.is_empty() && right.count() > rcc {
            right.shrink(rcc);
            slots.put(ANVIL_RIGHT, right);
        } else {
            slots.put(ANVIL_RIGHT, ItemStack::empty());
        }
    } else if !slots.menu.ws.anvil_only_renaming {
        slots.put(ANVIL_RIGHT, ItemStack::empty());
    }
    slots.set_data(ANVIL_COST, 0);
    slots.put(ANVIL_LEFT, ItemStack::empty());
    if let (Some(pos), true) = (slots.env.pos, slots.env.creative) {
        slots.env.effects.push(Effect::LevelEvent {
            event: ANVIL_USED,
            pos,
            data: 0,
        });
    }
}

/// `LevelEvent.SOUND_ANVIL_USED`, the `sipush 1030` in `lambda$onTake$0`.
const ANVIL_USED: i32 = 1030;

/// `StringUtil.filterText` then the 50-character cap — `validateName`,
/// offsets 0-17. `None` is its `null`.
fn anvil_validate_name(name: &str) -> Option<String> {
    // `filterText` drops `§` and every code point under 32 or equal to 127 —
    // `StringUtil.isAllowedChatCharacter`.
    let filtered: String = name
        .chars()
        .filter(|&c| c != '\u{a7}' && c >= ' ' && c != '\u{7f}')
        .collect();
    (filtered.encode_utf16().count() <= ANVIL_MAX_NAME_LENGTH).then_some(filtered)
}

/// `AnvilMenu.setItemName`, offsets 0-82 — `handleRenameItem`'s whole effect.
///
/// A name that does not validate, or that equals the current one, returns
/// `false` and nothing is recomputed or sent. Otherwise the name is stored,
/// the result stack (slot 2, if it holds anything) has its `CUSTOM_NAME` set
/// or removed in place, and `createResult` runs — ending in a
/// `broadcastChanges` the caller performs.
pub fn anvil_set_item_name(slots: &mut crate::click::Slots<'_>, name: &str) -> bool {
    use crate::item::{ComponentKind, ComponentValue};
    if slots.menu.spec.station != Some(Station::Anvil) {
        return false;
    }
    let Some(valid) = anvil_validate_name(name) else {
        return false;
    };
    if slots.menu.ws.anvil_item_name.as_deref() == Some(valid.as_str()) {
        return false;
    }
    slots.menu.ws.anvil_item_name = Some(valid.clone());
    let mut result = slots.peek(ANVIL_RESULT);
    if !result.is_empty() {
        let damageable = prototype_damage(slots, result.item());
        if valid.trim().is_empty() {
            result.remove_component(ComponentKind::CustomName, damageable);
        } else {
            result.set_component(
                ComponentKind::CustomName,
                ComponentValue::Text(valid),
                damageable,
            );
        }
        slots.put(ANVIL_RESULT, result);
    }
    anvil_create_result(slots);
    true
}

// ---------------------------------------------------------------------------
// GrindstoneMenu
// ---------------------------------------------------------------------------

const GRINDSTONE_TOP: usize = 0;
const GRINDSTONE_BOTTOM: usize = 1;
const GRINDSTONE_RESULT: usize = 2;
/// `LevelEvent.SOUND_GRINDSTONE_USED`, the `sipush 1042` in
/// `GrindstoneMenu$4.lambda$onTake$0`.
const GRINDSTONE_USED: i32 = 1042;

/// `GrindstoneMenu.createResult`, offsets 0-38: the result slot is
/// `computeResult(top, bottom)`.
fn grindstone_create_result(slots: &mut crate::click::Slots<'_>) {
    let top = slots.peek(GRINDSTONE_TOP);
    let bottom = slots.peek(GRINDSTONE_BOTTOM);
    let result = grindstone_compute_result(slots, &top, &bottom);
    slots.put(GRINDSTONE_RESULT, result);
}

/// `GrindstoneMenu.computeResult`, offsets 0-116.
///
/// ```text
///    0-27   neither slot holds anything            -> EMPTY
///   28-47   either slot holds more than one        -> EMPTY
///   48-109  only one holds anything: EMPTY unless it is enchanted, and then
///           removeNonCursesFrom(copy)
///  110-116  both: mergeItems(top, bottom)
/// ```
fn grindstone_compute_result(
    slots: &crate::click::Slots<'_>,
    top: &ItemStack,
    bottom: &ItemStack,
) -> ItemStack {
    if top.is_empty() && bottom.is_empty() {
        return ItemStack::empty();
    }
    if top.count() > 1 || bottom.count() > 1 {
        return ItemStack::empty();
    }
    if top.is_empty() || bottom.is_empty() {
        let only = if top.is_empty() { bottom } else { top };
        if !has_any_enchantments(only) {
            return ItemStack::empty();
        }
        return grindstone_remove_non_curses(slots, only.clone());
    }
    grindstone_merge_items(slots, top, bottom)
}

/// `GrindstoneMenu.mergeItems`, offsets 0-151.
///
/// Durability is pooled the anvil's way with a **5%** bonus where the anvil
/// gives 12 — `max * 5 / 100`, integer division of the product — and the
/// result is built on `top.copyWithCount(n)`, so it keeps the top stack's
/// patch. `set(MAX_DAMAGE, max)` (offsets 112-124) equals the prototype for
/// two stacks of one item and so adds nothing to the patch.
fn grindstone_merge_items(
    slots: &crate::click::Slots<'_>,
    top: &ItemStack,
    bottom: &ItemStack,
) -> ItemStack {
    use crate::item::{ComponentKind, ComponentValue};
    if !ItemStack::is_same_item(top, bottom) {
        return ItemStack::empty();
    }
    let props = slots.inventory.item_props();
    let max_of = |s: &ItemStack| props.map_or(0, |p| p.max_damage(s.item()));
    let max = max_of(top).max(max_of(bottom));
    let top_left = max_of(top) - top.damage_value();
    let bottom_left = max_of(bottom) - bottom.damage_value();
    let pooled = top_left + bottom_left + max * 5 / 100;
    let damageable_item = props.is_some_and(|p| p.is_damageable_item(top.item()));
    let mut count = 1;
    if !damageable_item {
        // Offsets 70-93: a stackable item that matches exactly merges into a
        // stack of two; anything else is refused.
        let stackable = props.map_or(64, |p| p.max_stack_size(top.item())) >= 2;
        if !stackable || top != bottom {
            return ItemStack::empty();
        }
        count = 2;
    }
    let mut out = top.copy_with_count(count);
    let damageable = prototype_damage(slots, top.item());
    if damageable_item {
        out.set_component(
            ComponentKind::Damage,
            ComponentValue::Int((max - pooled).max(0).clamp(0, max)),
            damageable,
        );
    }
    // `mergeEnchantsFrom(out, bottom)`: every enchantment of the bottom stack
    // that is a curse, or that the top stack lacks, is upserted at the
    // higher level; then `removeNonCursesFrom` strips everything but curses.
    // With the curses all that survives, the merge reduces to "the union of
    // both stacks' curses at their higher level".
    let kind = if out.item() == "minecraft:enchanted_book" {
        ComponentKind::StoredEnchantments
    } else {
        ComponentKind::Enchantments
    };
    let mut merged = out.enchantments_of(kind);
    for (id, lvl) in bottom.enchantments_of(kind) {
        match merged.iter_mut().find(|(k, _)| *k == id) {
            Some(e) => e.1 = e.1.max(lvl),
            None => merged.push((id, lvl)),
        }
    }
    out.set_component(kind, ComponentValue::Enchantments(merged), damageable);
    grindstone_remove_non_curses(slots, out)
}

/// `GrindstoneMenu.removeNonCursesFrom`, offsets 0-73: keep only the curses,
/// turn an enchanted book left with nothing into a plain book, and reprice the
/// prior-work penalty as one anvil step per surviving enchantment.
fn grindstone_remove_non_curses(slots: &crate::click::Slots<'_>, mut out: ItemStack) -> ItemStack {
    use crate::item::{ComponentKind, ComponentValue};
    let damageable = prototype_damage(slots, out.item());
    let kind = if out.item() == "minecraft:enchanted_book" {
        ComponentKind::StoredEnchantments
    } else {
        ComponentKind::Enchantments
    };
    let curses: Vec<(String, i32)> = out
        .enchantments_of(kind)
        .into_iter()
        .filter(|(id, _)| {
            slots
                .env
                .enchantments
                .and_then(|r| r.tag("minecraft:curse"))
                .is_some_and(|t| t.iter().any(|c| c == id))
        })
        .collect();
    let n = curses.len();
    out.set_component(kind, ComponentValue::Enchantments(curses), damageable);
    if out.item() == "minecraft:enchanted_book" && n == 0 {
        out = out.transmute_copy("minecraft:book", out.count(), false);
    }
    let mut cost = 0;
    for _ in 0..n {
        cost = crate::anvil::increased_repair_cost(cost);
    }
    out.set_component(
        ComponentKind::RepairCost,
        ComponentValue::Int(cost),
        damageable,
    );
    out
}

/// `GrindstoneMenu$4.onTake`, offsets 0-47.
///
/// The level-access lambda first — `ExperienceOrb.award` of
/// `getExperienceAmount`, then `levelEvent(1042, pos, 0)` — and then both
/// inputs emptied, each through `setItem` and so each through
/// `createResult`. The experience is the sum of the non-curse enchantments'
/// minimum costs; with none it is 0 and `award` spawns nothing. An enchanted
/// input would need the orb split and a `nextInt` draw that is not written
/// here, and no harness stack can carry an enchantment today.
fn grindstone_on_take(slots: &mut crate::click::Slots<'_>) {
    if let Some(pos) = slots.env.pos {
        slots.env.effects.push(Effect::LevelEvent {
            event: GRINDSTONE_USED,
            pos,
            data: 0,
        });
    }
    slots.put(GRINDSTONE_TOP, ItemStack::empty());
    slots.put(GRINDSTONE_BOTTOM, ItemStack::empty());
}

// ---------------------------------------------------------------------------
// EnchantmentMenu
// ---------------------------------------------------------------------------

const ENCHANTING_ITEM: usize = 0;
const ENCHANTING_LAPIS: usize = 1;
/// The ten data slots, in `addDataSlot` order — see
/// `crate::enchanting::menu_shape`.
const ENCHANTING_SEED: usize = 3;

/// `ItemStack.isEnchantable()`, offsets 0-39: the item has `ENCHANTABLE` and
/// its `ENCHANTMENTS` is present and empty.
fn is_enchantable(registry: &crate::enchantment::Registry, stack: &ItemStack) -> bool {
    use crate::item::ComponentKind;
    registry.enchantability(stack.item()).is_some()
        && stack
            .enchantments_of(ComponentKind::Enchantments)
            .is_empty()
}

/// `EnchantmentMenu.slotsChanged`, offsets 0-83, and
/// `lambda$slotsChanged$0` (0-277) through [`crate::enchanting::offers`].
///
/// An empty or unenchantable item zeroes the costs and sets every clue to
/// `-1` (offsets 30-61) without touching the random source. Otherwise the
/// menu's own `random` is reseeded from the **data slot** `enchantmentSeed` —
/// not from the player, so a seed change reaches the offers only once it has
/// been copied into the menu — and the three offers are drawn.
fn enchanting_slots_changed(slots: &mut crate::click::Slots<'_>) {
    let item = slots.peek(ENCHANTING_ITEM);
    let registry = slots.env.enchantments;
    let enchantable = registry.is_some_and(|r| !item.is_empty() && is_enchantable(r, &item));
    let rows = match (registry, enchantable) {
        (Some(registry), true) => {
            let mut random = rustmc_worldgen::rng::LegacyRandomSource::new(0);
            crate::enchanting::offers(
                &mut random,
                registry,
                slots.data(ENCHANTING_SEED),
                slots.env.bookshelves,
                item.item(),
            )
        }
        _ => [crate::enchanting::Offer::EMPTY; 3],
    };
    for (i, o) in rows.iter().enumerate() {
        slots.set_data(i, o.cost);
        slots.set_data(4 + i, o.enchant_clue);
        slots.set_data(7 + i, o.level_clue);
    }
}

/// `EnchantmentMenu.clickMenuButton`, offsets 0-153, and
/// `lambda$clickMenuButton$0` (0-244).
///
/// ```text
///    0-27   an id outside 0..2 is refused
///   51-82   lapis short of id + 1, without infinite materials: refused
///   84-125  costs[id] > 0, an item, and the level or infinite materials
///  129-151  access.execute(lambda) and return true
///
///   lambda:
///     list = getEnchantmentList(registryAccess, item, id, costs[id])
///     if list is empty: nothing at all
///     player.onEnchantmentPerformed(item, id + 1)   new seed; level - (id+1)
///     a book becomes an enchanted book (transmuteCopy), set into slot 0
///     every instance applied with ItemStack.enchant
///     lapis.consume(id + 1, player)                 not for infinite materials
///     enchantSlots.setChanged()                     -> slotsChanged
///     enchantmentSeed.set(player.getEnchantmentSeed())
///     slotsChanged(enchantSlots)
///     playSound(ENCHANTMENT_TABLE_USE, pitch 0.9 + nextFloat() * 0.1)
/// ```
///
/// The sound's pitch is a draw on the level's random, which no recording of
/// vanilla can reproduce; the event, source, position and volume are not.
/// The new seed is drawn here from the
/// same kind of source vanilla uses — `Player.random`, which is unseeded — so
/// it is a number no two runs agree on, vanilla against itself included.
fn enchanting_click_menu_button(slots: &mut crate::click::Slots<'_>, id: i32) -> bool {
    use crate::item::{ComponentKind, ComponentValue};
    let Ok(slot) = usize::try_from(id) else {
        return false;
    };
    if slot >= 3 {
        return false;
    }
    let item = slots.peek(ENCHANTING_ITEM);
    let lapis = slots.peek(ENCHANTING_LAPIS);
    let levels = id + 1;
    if (lapis.is_empty() || lapis.count() < levels) && !slots.env.creative {
        return false;
    }
    let cost = slots.data(slot);
    // No experience level is kept here: survival reads as level 0, so only
    // infinite materials can pay.
    if cost <= 0 || item.is_empty() || !slots.env.creative {
        return false;
    }
    let Some(registry) = slots.env.enchantments else {
        return true;
    };
    let mut random = rustmc_worldgen::rng::LegacyRandomSource::new(0);
    let list = crate::enchanting::enchantment_list(
        &mut random,
        registry,
        slots.data(ENCHANTING_SEED),
        id,
        cost,
        item.item(),
    );
    if list.is_empty() {
        return true;
    }
    // `Player.onEnchantmentPerformed`: the level drops by `levels` (clamped
    // at 0) and the seed is redrawn from the player's unseeded random.
    let fresh = fresh_seed();
    if let Some(seed) = slots.env.enchantment_seed.as_deref_mut() {
        *seed = fresh;
    }
    slots.env.experience_dirty = true;
    slots.env.levels_spent = levels;
    let mut out = item;
    if out.item() == "minecraft:book" {
        out = out.transmute_copy("minecraft:enchanted_book", out.count(), false);
    }
    let kind = if out.item() == "minecraft:enchanted_book" {
        ComponentKind::StoredEnchantments
    } else {
        ComponentKind::Enchantments
    };
    let mut held = out.enchantments_of(kind);
    for inst in &list {
        // `ItemEnchantments.Mutable.upgrade`: the higher of the two levels.
        match held.iter_mut().find(|(k, _)| *k == inst.enchantment) {
            Some(e) => e.1 = e.1.max(inst.level),
            None => held.push((inst.enchantment.clone(), inst.level)),
        }
    }
    let damageable = prototype_damage(slots, out.item());
    out.set_component(kind, ComponentValue::Enchantments(held), damageable);
    slots.put(ENCHANTING_ITEM, out);
    if !slots.env.creative {
        let mut l = lapis;
        l.shrink(levels);
        slots.put(
            ENCHANTING_LAPIS,
            if l.is_empty() { ItemStack::empty() } else { l },
        );
    }
    slots.set_data(ENCHANTING_SEED, fresh);
    enchanting_slots_changed(slots);
    // `ENCHANTMENT_TABLE_USE`, `BLOCKS`, volume 1, pitch
    // `0.9 + level.random.nextFloat() * 0.1` (offsets 213-241). The pitch is a
    // draw no recording reproduces; the trace's `sound` arm does not compare
    // it, and the value here is the midpoint of vanilla's range.
    if let Some(pos) = slots.env.pos {
        slots.env.effects.push(Effect::Sound(crate::sounds::Sound {
            event: "minecraft:block.enchantment_table.use".into(),
            source: crate::sounds::SOURCE_BLOCKS,
            at: crate::sounds::Eighths::block(pos),
            pos: crate::sounds::block_centre(pos),
            volume: 1.0,
            pitch: 0.95,
            except: None,
        }));
    }
    true
}

/// A seed no run can predict, which is what `Player.random.nextInt()` is.
fn fresh_seed() -> i32 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| {
                u64::try_from(d.as_nanos() & u128::from(u64::MAX)).unwrap_or(0)
            }),
    );
    #[expect(
        clippy::cast_possible_truncation,
        reason = "any 32 bits of a random 64"
    )]
    let v = h.finish() as i32;
    v
}

/// `EnchantingTableBlock.BOOKSHELF_OFFSETS` counted the way
/// `lambda$slotsChanged$0` counts them (offsets 19-70).
///
/// `BOOKSHELF_OFFSETS` is `BlockPos.betweenClosedStream(-2, 0, -2, 2, 1, 2)`
/// filtered to `|x| == 2 || |z| == 2`, so 32 positions, and
/// `isValidBookShelf` wants the position in `#enchantment_power_provider`
/// (only `minecraft:bookshelf`) and the block halfway to it —
/// `offset(x / 2, y, z / 2)`, Java's truncating division — in
/// `#enchantment_power_transmitter`, which is `#replaceable`.
#[must_use]
pub fn count_bookshelves(table: BlockPos, state_at: &dyn Fn(BlockPos) -> String) -> i32 {
    let tags = crate::fallable::tags();
    let block = |p: BlockPos| {
        let s = state_at(p);
        s.split('[').next().unwrap_or(&s).to_string()
    };
    let mut n = 0;
    for x in -2i32..=2 {
        for y in 0..=1 {
            for z in -2i32..=2 {
                if x.abs() != 2 && z.abs() != 2 {
                    continue;
                }
                let at = (table.0 + x, table.1 + y, table.2 + z);
                if block(at) != "minecraft:bookshelf" {
                    continue;
                }
                let mid = block((table.0 + x / 2, table.1 + y, table.2 + z / 2));
                let transmits = tags.map_or(mid == "minecraft:air", |t| {
                    t.contains("minecraft:replaceable", &mid)
                });
                if transmits {
                    n += 1;
                }
            }
        }
    }
    n
}

// ---------------------------------------------------------------------------
// SmithingMenu
// ---------------------------------------------------------------------------

const SMITHING_TEMPLATE: usize = 0;
const SMITHING_BASE: usize = 1;
const SMITHING_ADDITION: usize = 2;
const SMITHING_RESULT: usize = 3;
/// `hasRecipeError`, the smithing table's only data slot.
const SMITHING_ERROR: usize = 0;
/// `LevelEvent.SOUND_SMITHING_TABLE_USED`, the `sipush 1044` in
/// `lambda$onTake$0`.
const SMITHING_USED: i32 = 1044;

/// `SmithingMenu.slotsChanged`, offsets 0-84: `ItemCombinerMenu.slotsChanged`
/// runs `createResult` first, then `hasRecipeError` is set to 1 exactly when
/// all three inputs hold something and the result is still empty.
fn smithing_slots_changed(slots: &mut crate::click::Slots<'_>) {
    smithing_create_result(slots);
    let full = [SMITHING_TEMPLATE, SMITHING_BASE, SMITHING_ADDITION]
        .iter()
        .all(|&i| !slots.peek(i).is_empty());
    let error = full && slots.peek(SMITHING_RESULT).is_empty();
    slots.set_data(SMITHING_ERROR, i32::from(error));
}

/// `SmithingMenu.createResult`, offsets 0-62, with both lambdas.
///
/// `getRecipeFor(SMITHING, input)` and, on a hit, `SmithingRecipe.assemble`;
/// otherwise EMPTY. A transform's `assemble` is
/// `TransmuteRecipe.createWithOriginalComponents(result, base)`: the result
/// item with the **base's** patch applied, so a worn diamond sword becomes an
/// equally worn netherite one. A trim's is [`smithing_apply_trim`].
fn smithing_create_result(slots: &mut crate::click::Slots<'_>) {
    let input = crate::recipes::SmithingInput {
        template: slots.peek(SMITHING_TEMPLATE),
        base: slots.peek(SMITHING_BASE),
        addition: slots.peek(SMITHING_ADDITION),
    };
    let result = match slots
        .recipes
        .and_then(|r| r.find_smithing(&input))
        .map(|recipe| (&recipe.kind, recipe.result.clone()))
    {
        Some((crate::recipes::RecipeKind::SmithingTransform { .. }, template)) => {
            let damageable = prototype_damage(slots, template.item());
            input
                .base
                .transmute_copy(template.item(), template.count(), damageable)
        }
        Some((crate::recipes::RecipeKind::SmithingTrim { pattern, .. }, _)) => {
            let damageable = prototype_damage(slots, input.base.item());
            smithing_apply_trim(&input.base, &input.addition, pattern, damageable)
        }
        _ => ItemStack::empty(),
    };
    slots.put(SMITHING_RESULT, result);
}

/// `SmithingTrimRecipe.applyTrim(base, addition, pattern)`, offsets 0-76.
///
/// ```text
///    0- 12  material = addition.get(PROVIDES_TRIM_MATERIAL); null -> EMPTY
///   15- 36  existing = base.get(TRIM); trim = new ArmorTrim(material, pattern)
///   38- 51  Objects.equals(existing, trim) -> EMPTY
///   52- 72  out = base.copyWithCount(1); out.set(TRIM, trim); return out
/// ```
///
/// So re-applying the trim an item already carries makes nothing — and with
/// all three inputs full, `hasRecipeError` then reads 1 although a recipe
/// matched. `set` appends `TRIM` to the base's patch, after whatever the base
/// already carried.
fn smithing_apply_trim(
    base: &ItemStack,
    addition: &ItemStack,
    pattern: &str,
    damageable: bool,
) -> ItemStack {
    use crate::item::{ComponentKind, ComponentValue};
    let Some(material) = provides_trim_material(addition.item()) else {
        return ItemStack::empty();
    };
    let trim = ComponentValue::Trim {
        material: material.to_string(),
        pattern: pattern.to_string(),
    };
    if base.get_component(ComponentKind::Trim, damageable).as_ref() == Some(&trim) {
        return ItemStack::empty();
    }
    let mut out = base.copy_with_count(1);
    out.set_component(ComponentKind::Trim, trim, damageable);
    out
}

/// The `PROVIDES_TRIM_MATERIAL` an item is registered with —
/// `Item.Properties.trimMaterial(key)` in `Items.<clinit>`, eleven call sites
/// (offsets 7256, 9685, 9707, 9729, 9751, 9773, 9804, 9835, 9866, 9891 and
/// 15835), one per item of `#minecraft:trim_materials`.
#[must_use]
pub fn provides_trim_material(item: &str) -> Option<&'static str> {
    Some(match item {
        "minecraft:redstone" => "minecraft:redstone",
        "minecraft:diamond" => "minecraft:diamond",
        "minecraft:emerald" => "minecraft:emerald",
        "minecraft:lapis_lazuli" => "minecraft:lapis",
        "minecraft:quartz" => "minecraft:quartz",
        "minecraft:amethyst_shard" => "minecraft:amethyst",
        "minecraft:iron_ingot" => "minecraft:iron",
        "minecraft:copper_ingot" => "minecraft:copper",
        "minecraft:gold_ingot" => "minecraft:gold",
        "minecraft:netherite_ingot" => "minecraft:netherite",
        "minecraft:resin_brick" => "minecraft:resin",
        _ => return None,
    })
}

/// The `minecraft:trim_material` registry in network order — synced and
/// key-sorted like [`BANNER_PATTERNS`]: the 11 files of
/// `data/minecraft/trim_material/` in the pinned jar, sorted.
/// `tests/workstation_menus.rs` holds it against the capture.
pub const TRIM_MATERIALS: [&str; 11] = [
    "minecraft:amethyst",
    "minecraft:copper",
    "minecraft:diamond",
    "minecraft:emerald",
    "minecraft:gold",
    "minecraft:iron",
    "minecraft:lapis",
    "minecraft:netherite",
    "minecraft:quartz",
    "minecraft:redstone",
    "minecraft:resin",
];

/// The `minecraft:trim_pattern` registry in network order: the 18 files of
/// `data/minecraft/trim_pattern/`, sorted.
pub const TRIM_PATTERNS: [&str; 18] = [
    "minecraft:bolt",
    "minecraft:coast",
    "minecraft:dune",
    "minecraft:eye",
    "minecraft:flow",
    "minecraft:host",
    "minecraft:raiser",
    "minecraft:rib",
    "minecraft:sentry",
    "minecraft:shaper",
    "minecraft:silence",
    "minecraft:snout",
    "minecraft:spire",
    "minecraft:tide",
    "minecraft:vex",
    "minecraft:ward",
    "minecraft:wayfinder",
    "minecraft:wild",
];

/// A trim material's network id.
#[must_use]
pub fn trim_material_id(material: &str) -> Option<i32> {
    TRIM_MATERIALS
        .iter()
        .position(|m| *m == material)
        .and_then(|i| i32::try_from(i).ok())
}

/// A holder id out of the registries this module carries in registry order —
/// banner patterns, trim materials, trim patterns — for the one patch
/// encoder (`crate::menu`), behind [`crate::patch::HolderIds`]' captured
/// registries.
#[must_use]
pub fn holder_id(registry: &str, name: &str) -> Option<i32> {
    match registry {
        "minecraft:banner_pattern" => banner_pattern_id(name),
        "minecraft:trim_material" => trim_material_id(name),
        "minecraft:trim_pattern" => trim_pattern_id(name),
        _ => None,
    }
}

/// A trim pattern's network id.
#[must_use]
pub fn trim_pattern_id(pattern: &str) -> Option<i32> {
    TRIM_PATTERNS
        .iter()
        .position(|p| *p == pattern)
        .and_then(|i| i32::try_from(i).ok())
}

/// `SmithingMenu.onTake`, offsets 0-50: one of each input consumed, in slot
/// order, each through `setItem` and so each through `slotsChanged`; then
/// `levelEvent(1044, pos, 0)`.
fn smithing_on_take(slots: &mut crate::click::Slots<'_>) {
    for i in [SMITHING_TEMPLATE, SMITHING_BASE, SMITHING_ADDITION] {
        let mut stack = slots.peek(i);
        if !stack.is_empty() {
            stack.shrink(1);
            slots.put(i, stack);
        }
    }
    if let Some(pos) = slots.env.pos {
        slots.env.effects.push(Effect::LevelEvent {
            event: SMITHING_USED,
            pos,
            data: 0,
        });
    }
}

// ---------------------------------------------------------------------------
// LoomMenu
// ---------------------------------------------------------------------------

const LOOM_BANNER: usize = 0;
const LOOM_DYE: usize = 1;
const LOOM_PATTERN: usize = 2;
const LOOM_RESULT: usize = 3;
/// `selectedBannerPatternIndex`, the loom's only data slot.
const LOOM_SELECTED: usize = 0;
/// `BannerPatternLayers.Layer`'s cap that `slotsChanged` tests — `bipush 6`
/// at offset 246: a banner already carrying six layers gets no result.
const LOOM_MAX_LAYERS: usize = 6;

/// The `minecraft:banner_pattern` registry in network order.
///
/// A synced data-driven registry, sent key-sorted — the capture in
/// `data/vanilla/registries.bin` is sorted, and `crate::chat` asserts the same
/// of `chat_type` — so this is the 43 files of `data/minecraft/banner_pattern/`
/// in the pinned jar, sorted by path. `tests/workstation_menus.rs` holds it
/// against the capture.
pub const BANNER_PATTERNS: [&str; 43] = [
    "minecraft:base",
    "minecraft:border",
    "minecraft:bricks",
    "minecraft:circle",
    "minecraft:creeper",
    "minecraft:cross",
    "minecraft:curly_border",
    "minecraft:diagonal_left",
    "minecraft:diagonal_right",
    "minecraft:diagonal_up_left",
    "minecraft:diagonal_up_right",
    "minecraft:flow",
    "minecraft:flower",
    "minecraft:globe",
    "minecraft:gradient",
    "minecraft:gradient_up",
    "minecraft:guster",
    "minecraft:half_horizontal",
    "minecraft:half_horizontal_bottom",
    "minecraft:half_vertical",
    "minecraft:half_vertical_right",
    "minecraft:mojang",
    "minecraft:piglin",
    "minecraft:rhombus",
    "minecraft:skull",
    "minecraft:small_stripes",
    "minecraft:square_bottom_left",
    "minecraft:square_bottom_right",
    "minecraft:square_top_left",
    "minecraft:square_top_right",
    "minecraft:straight_cross",
    "minecraft:stripe_bottom",
    "minecraft:stripe_center",
    "minecraft:stripe_downleft",
    "minecraft:stripe_downright",
    "minecraft:stripe_left",
    "minecraft:stripe_middle",
    "minecraft:stripe_right",
    "minecraft:stripe_top",
    "minecraft:triangle_bottom",
    "minecraft:triangle_top",
    "minecraft:triangles_bottom",
    "minecraft:triangles_top",
];

/// A banner pattern's network id.
#[must_use]
pub fn banner_pattern_id(pattern: &str) -> Option<i32> {
    BANNER_PATTERNS
        .iter()
        .position(|p| *p == pattern)
        .and_then(|i| i32::try_from(i).ok())
}

/// `#minecraft:no_item_required`, in the tag file's own order
/// (`data/minecraft/tags/banner_pattern/no_item_required.json`) — the list a
/// loom offers with its pattern slot empty, and so the meaning of a button.
const NO_ITEM_REQUIRED: [&str; 32] = [
    "minecraft:square_bottom_left",
    "minecraft:square_bottom_right",
    "minecraft:square_top_left",
    "minecraft:square_top_right",
    "minecraft:stripe_bottom",
    "minecraft:stripe_top",
    "minecraft:stripe_left",
    "minecraft:stripe_right",
    "minecraft:stripe_center",
    "minecraft:stripe_middle",
    "minecraft:stripe_downright",
    "minecraft:stripe_downleft",
    "minecraft:small_stripes",
    "minecraft:cross",
    "minecraft:straight_cross",
    "minecraft:triangle_bottom",
    "minecraft:triangle_top",
    "minecraft:triangles_bottom",
    "minecraft:triangles_top",
    "minecraft:diagonal_left",
    "minecraft:diagonal_up_right",
    "minecraft:diagonal_up_left",
    "minecraft:diagonal_right",
    "minecraft:circle",
    "minecraft:rhombus",
    "minecraft:half_vertical",
    "minecraft:half_horizontal",
    "minecraft:half_vertical_right",
    "minecraft:half_horizontal_bottom",
    "minecraft:border",
    "minecraft:gradient",
    "minecraft:gradient_up",
];

/// `isPatternItem` and its `PROVIDES_BANNER_PATTERNS` tag, for the ten items
/// of `#minecraft:loom_patterns`: each names a
/// `#minecraft:pattern_item/<x>` tag of exactly one pattern.
fn pattern_item_patterns(item: &str) -> Option<&'static str> {
    Some(match item {
        "minecraft:flower_banner_pattern" => "minecraft:flower",
        "minecraft:creeper_banner_pattern" => "minecraft:creeper",
        "minecraft:skull_banner_pattern" => "minecraft:skull",
        "minecraft:mojang_banner_pattern" => "minecraft:mojang",
        "minecraft:globe_banner_pattern" => "minecraft:globe",
        "minecraft:piglin_banner_pattern" => "minecraft:piglin",
        "minecraft:flow_banner_pattern" => "minecraft:flow",
        "minecraft:guster_banner_pattern" => "minecraft:guster",
        "minecraft:field_masoned_banner_pattern" => "minecraft:bricks",
        "minecraft:bordure_indented_banner_pattern" => "minecraft:curly_border",
        _ => return None,
    })
}

/// `isDyeItem`'s `DYE` component — the `DyeColor` id of a dye, in the enum's
/// declaration order, which `DyeColor.STREAM_CODEC`'s `idMapper` writes.
fn dye_color(item: &str) -> Option<i32> {
    const COLOURS: [&str; 16] = [
        "white",
        "orange",
        "magenta",
        "light_blue",
        "yellow",
        "lime",
        "pink",
        "gray",
        "light_gray",
        "cyan",
        "purple",
        "blue",
        "brown",
        "green",
        "red",
        "black",
    ];
    let colour = item.strip_prefix("minecraft:")?.strip_suffix("_dye")?;
    COLOURS
        .iter()
        .position(|c| *c == colour)
        .and_then(|i| i32::try_from(i).ok())
}

/// `getSelectablePatterns(patternStack)`, offsets 0-62: the no-item list for
/// an empty pattern slot, the item's own patterns otherwise.
fn loom_selectable(pattern: &ItemStack) -> Vec<&'static str> {
    if pattern.is_empty() {
        return NO_ITEM_REQUIRED.to_vec();
    }
    pattern_item_patterns(pattern.item()).into_iter().collect()
}

/// `LoomMenu.slotsChanged`, offsets 0-307.
///
/// ```text
///   26-64   no banner or no dye: result EMPTY, list empty, selection -1
///   66-95   i = selection; valid = isValidPatternIndex(i) against the OLD
///           list; then the list is rebuilt
///   99-134  a one-entry list selects entry 0
///  137-153  else an invalid old selection becomes -1
///  156-213  else the old pattern is looked up in the new list by identity
///  216-300  a pattern on a banner with six layers already: -1 and EMPTY;
///           otherwise setupResultSlot(pattern); no pattern: EMPTY
/// ```
fn loom_slots_changed(slots: &mut crate::click::Slots<'_>) {
    let banner = slots.peek(LOOM_BANNER);
    let dye = slots.peek(LOOM_DYE);
    let pattern = slots.peek(LOOM_PATTERN);
    if banner.is_empty() || dye.is_empty() {
        slots.put(LOOM_RESULT, ItemStack::empty());
        slots.menu.ws.loom_patterns.clear();
        slots.set_data(LOOM_SELECTED, -1);
        return;
    }
    let i = slots.data(LOOM_SELECTED);
    let old = slots.menu.ws.loom_patterns.clone();
    let valid = usize::try_from(i).is_ok_and(|i| i < old.len());
    let new: Vec<String> = loom_selectable(&pattern)
        .into_iter()
        .map(str::to_string)
        .collect();
    slots.menu.ws.loom_patterns.clone_from(&new);
    let chosen = if new.len() == 1 {
        slots.set_data(LOOM_SELECTED, 0);
        Some(new[0].clone())
    } else if !valid {
        slots.set_data(LOOM_SELECTED, -1);
        None
    } else {
        let held = usize::try_from(i).ok().and_then(|i| old.get(i).cloned());
        let at = held.as_ref().and_then(|h| new.iter().position(|p| p == h));
        match held.zip(at) {
            Some((h, j)) => {
                slots.set_data(LOOM_SELECTED, i32::try_from(j).unwrap_or(-1));
                Some(h)
            }
            None => {
                slots.set_data(LOOM_SELECTED, -1);
                None
            }
        }
    };
    match chosen {
        Some(p) => {
            if loom_layers(&banner).len() >= LOOM_MAX_LAYERS {
                slots.set_data(LOOM_SELECTED, -1);
                slots.put(LOOM_RESULT, ItemStack::empty());
            } else {
                loom_setup_result_slot(slots, &p);
            }
        }
        None => slots.put(LOOM_RESULT, ItemStack::empty()),
    }
}

/// The banner's layers, bottom first.
fn loom_layers(banner: &ItemStack) -> Vec<(String, i32)> {
    use crate::item::{ComponentKind, ComponentValue};
    match banner.get_component(ComponentKind::BannerPatterns, false) {
        Some(ComponentValue::Layers(l)) => l,
        _ => Vec::new(),
    }
}

/// `LoomMenu.setupResultSlot`, offsets 0-103: a copy of one banner with the
/// pattern added on top in the dye's colour (`ItemStack.update` with
/// `BannerPatternLayers.Builder.addAll(...).add(pattern, colour)`), written
/// only if it differs from what the result already holds.
fn loom_setup_result_slot(slots: &mut crate::click::Slots<'_>, pattern: &str) {
    use crate::item::{ComponentKind, ComponentValue};
    let banner = slots.peek(LOOM_BANNER);
    let dye = slots.peek(LOOM_DYE);
    let mut out = ItemStack::empty();
    if !banner.is_empty() && !dye.is_empty() {
        if let Some(colour) = dye_color(dye.item()) {
            out = banner.copy_with_count(1);
            let mut layers = loom_layers(&banner);
            layers.push((pattern.to_string(), colour));
            out.set_component(
                ComponentKind::BannerPatterns,
                ComponentValue::Layers(layers),
                false,
            );
        }
    }
    if out != slots.peek(LOOM_RESULT) {
        slots.put(LOOM_RESULT, out);
    }
}

/// `LoomMenu.clickMenuButton`, offsets 0-45: an index inside the current list
/// selects it and builds the result; anything else returns false.
fn loom_click_menu_button(slots: &mut crate::click::Slots<'_>, id: i32) -> bool {
    let Some(p) = usize::try_from(id)
        .ok()
        .and_then(|i| slots.menu.ws.loom_patterns.get(i).cloned())
    else {
        return false;
    };
    slots.set_data(LOOM_SELECTED, id);
    loom_setup_result_slot(slots, &p);
    true
}

/// `LoomMenu$6.onTake`, offsets 0-82: one banner and one dye removed (each a
/// `slotsChanged`), the selection reset if either ran out, and
/// `UI_LOOM_TAKE_RESULT` at the block — volume 1, pitch 1, once per game tick.
/// The pattern item is not consumed.
fn loom_on_take(slots: &mut crate::click::Slots<'_>) {
    for i in [LOOM_BANNER, LOOM_DYE] {
        let mut s = slots.peek(i);
        if !s.is_empty() {
            s.shrink(1);
            slots.put(i, s);
        }
    }
    if slots.peek(LOOM_BANNER).is_empty() || slots.peek(LOOM_DYE).is_empty() {
        slots.set_data(LOOM_SELECTED, -1);
    }
    if let Some(pos) = slots.env.pos {
        let now = slots.env.game_time;
        if slots.menu.ws.last_sound_time != now {
            slots.env.effects.push(Effect::Sound(crate::sounds::Sound {
                event: "minecraft:ui.loom.take_result".into(),
                source: crate::sounds::SOURCE_BLOCKS,
                at: crate::sounds::Eighths::block(pos),
                pos: crate::sounds::block_centre(pos),
                volume: 1.0,
                pitch: 1.0,
                except: None,
            }));
            slots.menu.ws.last_sound_time = now;
        }
    }
}

// ---------------------------------------------------------------------------
// quickMoveStack — one skeleton, seven routing tables
// ---------------------------------------------------------------------------

/// Where one shift-click sends the clicked stack.
enum Route {
    /// `moveItemStackTo(stack, start, end, reverse)`; on `false` the method
    /// returns EMPTY.
    Move(usize, usize, bool),
    /// `EnchantmentMenu.quickMoveStack` offsets 113-185: one item of the
    /// stack straight into slot 0 by `setByPlayer`, the source shrunk by one.
    OneIntoSlot0,
    /// The method returns EMPTY without moving anything.
    Refuse,
}

/// The routing every one of these overrides shares the shape of: the result
/// to the player's 36 **reversed**, a menu input to them forwards, a player
/// stack into the inputs that accept it, and otherwise main inventory and
/// hotbar into each other. Read per class:
///
/// * `StonecutterMenu.quickMoveStack` 0-269 — result 1, input 0; a stack
///   `stonecutterRecipes().acceptsInput` takes goes to `0..1`;
/// * `ItemCombinerMenu.quickMoveStack` 0-291 (anvil, smithing table) — a
///   player stack goes to `0..resultSlot` when `canMoveIntoInputSlots`
///   (always, for the anvil; for the smithing table the first of template,
///   base, addition whose test it passes and whose slot is empty);
/// * `GrindstoneMenu.quickMoveStack` 0-265 — a player stack goes to `0..2`
///   when either input is empty, and is shuffled otherwise;
/// * `EnchantmentMenu.quickMoveStack` 0-235 — both own slots to `2..38`
///   reversed, lapis to `1..2` reversed, anything else one item into an
///   empty slot 0 that accepts it, or nothing at all — this one never
///   shuffles the player's own slots;
/// * `LoomMenu.quickMoveStack` 0-356 — banners, dyes and pattern items each to
///   their own slot;
/// * `CartographyTableMenu.quickMoveStack` 0-297 — a map to slot 0, paper,
///   an empty map or a glass pane to slot 1.
fn route(slots: &crate::click::Slots<'_>, station: Station, i: usize, stack: &ItemStack) -> Route {
    let n = slots.menu.spec.container_slots;
    let (inv, hot, end) = (n, n + 27, n + 36);
    let shuffle = |i: usize| {
        if (inv..hot).contains(&i) {
            Route::Move(hot, end, false)
        } else if (hot..end).contains(&i) {
            Route::Move(inv, hot, false)
        } else {
            Route::Refuse
        }
    };
    if station.is_result(i) {
        return Route::Move(inv, end, true);
    }
    match station {
        Station::Enchanting => {
            if i < n {
                Route::Move(inv, end, true)
            } else if stack.item() == "minecraft:lapis_lazuli" {
                Route::Move(ENCHANTING_LAPIS, ENCHANTING_LAPIS + 1, true)
            } else if slots.peek(ENCHANTING_ITEM).is_empty()
                && slots.accepts(ENCHANTING_ITEM, stack)
            {
                Route::OneIntoSlot0
            } else {
                Route::Refuse
            }
        }
        _ if i < n => Route::Move(inv, end, false),
        Station::Stonecutter => {
            let accepts = slots
                .recipes
                .is_some_and(|r| !r.stonecutter_recipes_for(stack).is_empty());
            if accepts {
                Route::Move(STONECUTTER_INPUT, STONECUTTER_INPUT + 1, false)
            } else {
                shuffle(i)
            }
        }
        Station::Anvil => Route::Move(0, ANVIL_RESULT, false),
        Station::Smithing => {
            let into = [SMITHING_TEMPLATE, SMITHING_BASE, SMITHING_ADDITION]
                .iter()
                .any(|&s| slots.peek(s).is_empty() && slots.accepts(s, stack));
            if into {
                Route::Move(0, SMITHING_RESULT, false)
            } else {
                shuffle(i)
            }
        }
        Station::Grindstone => {
            if slots.peek(GRINDSTONE_TOP).is_empty() || slots.peek(GRINDSTONE_BOTTOM).is_empty() {
                Route::Move(0, GRINDSTONE_RESULT, false)
            } else {
                shuffle(i)
            }
        }
        Station::Loom => {
            if slots.accepts(LOOM_BANNER, stack) {
                Route::Move(LOOM_BANNER, LOOM_BANNER + 1, false)
            } else if slots.accepts(LOOM_DYE, stack) {
                Route::Move(LOOM_DYE, LOOM_DYE + 1, false)
            } else if slots.accepts(LOOM_PATTERN, stack) {
                Route::Move(LOOM_PATTERN, LOOM_PATTERN + 1, false)
            } else {
                shuffle(i)
            }
        }
        // `CartographyTableMenu.quickMoveStack` 0-297: a stack carrying
        // `MAP_ID` to `0..1` (offsets 115-139) — a component no stack on this
        // server carries, so that arm is never taken here, which is also
        // vanilla's answer for every stack the corpus can hold — then paper,
        // an empty map or a glass pane to `1..2` (142-188), and otherwise the
        // shuffle (190-246). Its tail adds a `broadcastChanges()` (292),
        // which runs under `clicked`'s `suppressRemoteUpdates` and so sends
        // and records nothing.
        Station::Cartography => {
            if matches!(
                stack.item(),
                "minecraft:paper" | "minecraft:map" | "minecraft:glass_pane"
            ) {
                Route::Move(CARTOGRAPHY_ADDITIONAL, CARTOGRAPHY_ADDITIONAL + 1, false)
            } else {
                shuffle(i)
            }
        }
    }
}

/// `CartographyTableMenu`'s second input, where paper, an empty map or a
/// glass pane goes.
const CARTOGRAPHY_ADDITIONAL: usize = 1;

/// The shared tail of all seven overrides, after the routing:
///
/// ```text
///   if stack.isEmpty(): slot.setByPlayer(EMPTY) else slot.setChanged()
///   if stack.getCount() == copy.getCount(): return EMPTY
///   slot.onTake(player, stack)
///   return copy
/// ```
///
/// `onQuickCraft` on the result is the default `Slot`'s, a no-op for every
/// slot here. `StonecutterMenu` alone drops what is left of a result that did
/// not fit (`player.drop(stack, false)`, offsets 252-263) — not written: it
/// needs a full inventory and an item entity, and no scenario fills one.
pub fn quick_move_stack(slots: &mut crate::click::Slots<'_>, index: usize) -> ItemStack {
    let Some(station) = slots.menu.spec.station else {
        return ItemStack::empty();
    };
    let mut stack = slots.peek(index);
    if stack.is_empty() {
        return ItemStack::empty();
    }
    let copy = stack.clone();
    match route(slots, station, index, &stack) {
        Route::Refuse => return ItemStack::empty(),
        Route::Move(start, end, reverse) => {
            if !slots.move_to(&mut stack, start, end, reverse) {
                return ItemStack::empty();
            }
        }
        Route::OneIntoSlot0 => {
            let one = stack.copy_with_count(1);
            stack.shrink(1);
            slots.put(ENCHANTING_ITEM, one);
        }
    }
    slots.put(index, stack.clone());
    if stack.count() == copy.count() {
        return ItemStack::empty();
    }
    on_take(slots, index, &stack);
    copy
}
