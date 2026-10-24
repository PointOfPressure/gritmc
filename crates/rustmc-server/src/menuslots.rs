//! The slot classes of `AbstractFurnaceMenu` and `BrewingStandMenu` — what
//! each container slot of those two menus will accept, and how much of it.
//!
//! `crate::click::Slots` asks two questions of a slot, `Slot.mayPlace` and
//! `Slot.getMaxStackSize`, and for a chest the answer is the base class's:
//! anything, up to the stack's own maximum. These two menus install their own
//! `Slot` subclasses and the answers stop being uniform. Read from the pinned
//! 26.2 jar:
//!
//! | menu slot | class | `mayPlace` | `getMaxStackSize` |
//! |---|---|---|---|
//! | furnace 0 | `Slot` | `true` | base |
//! | furnace 1 | `FurnaceFuelSlot` | `menu.isFuel(stack) \|\| isBucket(stack)` | `isBucket ? 1 : base` |
//! | furnace 2 | `FurnaceResultSlot` | `false` | base |
//! | brewing 0-2 | `BrewingStandMenu$PotionSlot` | potion, splash, lingering or glass bottle | `1` |
//! | brewing 3 | `BrewingStandMenu$IngredientsSlot` | `potionBrewing.isIngredient(stack)` | base |
//! | brewing 4 | `BrewingStandMenu$FuelSlot` | `stack.is(ItemTags.BREWING_FUEL)` | base |
//!
//! These are pure functions of the slot, the stack and the tables, so that
//! each row can be tested without a menu, a block entity or a click.
//!
//! **What a wrong row costs is not a refused click, it is a wrong inventory.**
//! A fuel slot that accepted stone would take it out of the player's hand and
//! keep it, and every later assertion in the run would be measuring a stone
//! that vanilla left on the cursor.

use crate::brewing::Brewing;
use crate::fuel::FuelValues;
use crate::item::ItemStack;
use crate::recipes::ItemTags;

/// Which slot subclass a container slot is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotClass {
    /// `Slot` itself — a furnace's ingredient slot, and every slot of a menu
    /// that installs no subclass.
    Plain,
    /// `FurnaceFuelSlot`.
    FurnaceFuel,
    /// `FurnaceResultSlot`.
    FurnaceResult,
    /// `BrewingStandMenu$PotionSlot`.
    BrewingPotion,
    /// `BrewingStandMenu$IngredientsSlot`.
    BrewingIngredient,
    /// `BrewingStandMenu$FuelSlot`.
    BrewingFuel,
}

/// The menu families that install slot subclasses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// `AbstractFurnaceMenu` — the furnace, the blast furnace and the smoker.
    Furnace,
    /// `BrewingStandMenu`.
    BrewingStand,
}

/// The class of container slot `slot` in a menu of `family`.
///
/// The constructor order is the slot order: `AbstractFurnaceMenu` adds a
/// `Slot` (0), a `FurnaceFuelSlot` (1) and a `FurnaceResultSlot` (2);
/// `BrewingStandMenu` adds three `PotionSlot`s (0-2), an `IngredientsSlot`
/// (3) and a `FuelSlot` (4). Anything past those is the player's inventory
/// and is never asked here.
#[must_use]
pub const fn class_of(family: Family, slot: usize) -> SlotClass {
    match (family, slot) {
        (Family::Furnace, 1) => SlotClass::FurnaceFuel,
        (Family::Furnace, 2) => SlotClass::FurnaceResult,
        (Family::BrewingStand, 0..=2) => SlotClass::BrewingPotion,
        (Family::BrewingStand, 3) => SlotClass::BrewingIngredient,
        (Family::BrewingStand, 4) => SlotClass::BrewingFuel,
        _ => SlotClass::Plain,
    }
}

/// The tables the slot rules read, none of which a slot can load itself.
///
/// `None` is a refusal in every row that reads it: a fuel slot with no fuel
/// table accepts only a bucket, and a brewing fuel slot with no tags accepts
/// nothing. That is the direction that leaves the stack on the cursor.
pub struct Tables<'a> {
    /// `Level.fuelValues()`.
    pub fuels: Option<&'a FuelValues>,
    /// The item tags, for `ItemTags.BREWING_FUEL`.
    pub tags: Option<&'a ItemTags>,
    /// `Level.potionBrewing()`.
    pub brewing: &'a dyn Brewing,
}

/// `FurnaceFuelSlot.isBucket(ItemStack)` — `stack.is(Items.BUCKET)`.
#[must_use]
pub fn is_bucket(stack: &ItemStack) -> bool {
    stack.item() == "minecraft:bucket"
}

/// `BrewingStandMenu$PotionSlot.mayPlaceItem` — four `ItemStack.is` tests in
/// the order the bytecode makes them, `POTION`, `SPLASH_POTION`,
/// `LINGERING_POTION`, `GLASS_BOTTLE`.
#[must_use]
pub fn potion_slot_may_place(stack: &ItemStack) -> bool {
    matches!(
        stack.item(),
        "minecraft:potion"
            | "minecraft:splash_potion"
            | "minecraft:lingering_potion"
            | "minecraft:glass_bottle"
    )
}

/// `BrewingStandMenu$FuelSlot.mayPlaceItem` — `stack.is(BREWING_FUEL)`.
#[must_use]
pub fn brewing_fuel_may_place(stack: &ItemStack, tags: Option<&ItemTags>) -> bool {
    !stack.is_empty()
        && tags.is_some_and(|t| t.contains(crate::brewing::BREWING_FUEL_TAG, stack.item()))
}

/// `Slot.mayPlace(ItemStack)` for one slot class.
#[must_use]
pub fn may_place(class: SlotClass, stack: &ItemStack, tables: &Tables<'_>) -> bool {
    match class {
        SlotClass::Plain => true,
        // Offsets 0-23: `menu.isFuel(stack) || isBucket(stack)`. `isFuel` is
        // `level.fuelValues().isFuel(stack)`, a bare `containsKey`.
        SlotClass::FurnaceFuel => {
            tables.fuels.is_some_and(|f| f.is_fuel(stack)) || is_bucket(stack)
        }
        // `iconst_0; ireturn`.
        SlotClass::FurnaceResult => false,
        SlotClass::BrewingPotion => potion_slot_may_place(stack),
        SlotClass::BrewingIngredient => tables.brewing.is_ingredient(stack),
        SlotClass::BrewingFuel => brewing_fuel_may_place(stack, tables.tags),
    }
}

/// The slot's own cap on `Slot.getMaxStackSize(ItemStack)`, or `None` where
/// the class does not override it and the base rule stands.
///
/// * `FurnaceFuelSlot.getMaxStackSize(ItemStack)` — `isBucket ? 1 :
///   super.getMaxStackSize(stack)`, offsets 0-16. An empty bucket stacks to
///   16 and the fuel slot still takes one, which is what stops a furnace
///   holding sixteen buckets that would each want to become a water bucket.
/// * `PotionSlot.getMaxStackSize()` — the **no-argument** form, `iconst_1`.
///   The one-argument form is the base `Math.min(getMaxStackSize(),
///   stack.getMaxStackSize())`, so it is 1 for anything, including a stack of
///   sixteen glass bottles.
#[must_use]
pub fn max_stack_size(class: SlotClass, stack: &ItemStack) -> Option<i32> {
    match class {
        SlotClass::FurnaceFuel if is_bucket(stack) => Some(1),
        SlotClass::BrewingPotion => Some(1),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brewing::{NoBrewing, VanillaBrewing};

    fn fuels() -> FuelValues {
        FuelValues::parse(&serde_json::json!({
            "table": [
                {"item": "minecraft:coal", "burnDuration": 1600},
                {"item": "minecraft:lava_bucket", "burnDuration": 20000}
            ]
        }))
        .expect("fuel table parses")
    }

    fn tags() -> ItemTags {
        ItemTags::parse(&serde_json::json!({
            "brewing_fuel": {"values": ["minecraft:blaze_powder"]}
        }))
        .expect("tags parse")
    }

    /// The furnace's three slots answer three different ways, and the fuel
    /// slot's bucket exception is an `||`, not a fuel-table entry.
    #[test]
    fn a_furnace_takes_anything_in_0_fuel_or_a_bucket_in_1_and_nothing_in_2() {
        let f = fuels();
        let t = Tables {
            fuels: Some(&f),
            tags: None,
            brewing: &NoBrewing,
        };
        let stone = ItemStack::one("minecraft:stone");
        let coal = ItemStack::one("minecraft:coal");
        let bucket = ItemStack::one("minecraft:bucket");
        assert!(may_place(class_of(Family::Furnace, 0), &stone, &t));
        assert!(!may_place(class_of(Family::Furnace, 1), &stone, &t));
        assert!(may_place(class_of(Family::Furnace, 1), &coal, &t));
        assert!(
            may_place(class_of(Family::Furnace, 1), &bucket, &t),
            "an empty bucket is not fuel and is still accepted"
        );
        assert!(!may_place(class_of(Family::Furnace, 2), &coal, &t));
        // No fuel table: the bucket is the only thing left.
        let none = Tables {
            fuels: None,
            tags: None,
            brewing: &NoBrewing,
        };
        assert!(!may_place(SlotClass::FurnaceFuel, &coal, &none));
        assert!(may_place(SlotClass::FurnaceFuel, &bucket, &none));
    }

    /// The two stack-size overrides, and the classes that have none.
    #[test]
    fn a_bucket_in_the_fuel_slot_and_anything_in_a_bottle_slot_stack_to_one() {
        let bucket = ItemStack::one("minecraft:bucket");
        let coal = ItemStack::one("minecraft:coal");
        assert_eq!(max_stack_size(SlotClass::FurnaceFuel, &bucket), Some(1));
        assert_eq!(max_stack_size(SlotClass::FurnaceFuel, &coal), None);
        assert_eq!(max_stack_size(SlotClass::Plain, &bucket), None);
        let bottles = ItemStack::new("minecraft:glass_bottle", 16);
        assert_eq!(max_stack_size(SlotClass::BrewingPotion, &bottles), Some(1));
        assert_eq!(max_stack_size(SlotClass::BrewingFuel, &bottles), None);
    }

    /// The brewing stand's five slots, against the vanilla table.
    #[test]
    fn a_brewing_stand_sorts_bottles_ingredients_and_blaze_powder() {
        let t_tags = tags();
        let t = Tables {
            fuels: None,
            tags: Some(&t_tags),
            brewing: &VanillaBrewing,
        };
        let potion = ItemStack::one("minecraft:potion");
        let bottle = ItemStack::one("minecraft:glass_bottle");
        let wart = ItemStack::one("minecraft:nether_wart");
        let gunpowder = ItemStack::one("minecraft:gunpowder");
        let blaze = ItemStack::one("minecraft:blaze_powder");
        let stick = ItemStack::one("minecraft:stick");
        for slot in 0..3 {
            let c = class_of(Family::BrewingStand, slot);
            assert!(may_place(c, &potion, &t));
            assert!(may_place(c, &bottle, &t));
            assert!(!may_place(c, &wart, &t));
        }
        let ing = class_of(Family::BrewingStand, 3);
        assert!(may_place(ing, &wart, &t));
        assert!(may_place(ing, &gunpowder, &t), "a container-mix ingredient");
        assert!(
            may_place(ing, &blaze, &t),
            "blaze powder is an ingredient as well as a fuel"
        );
        assert!(!may_place(ing, &stick, &t));
        let fuel = class_of(Family::BrewingStand, 4);
        assert!(may_place(fuel, &blaze, &t));
        assert!(!may_place(fuel, &wart, &t));
        // No tags: nothing is fuel.
        let none = Tables {
            fuels: None,
            tags: None,
            brewing: &VanillaBrewing,
        };
        assert!(!may_place(fuel, &blaze, &none));
    }
}
