//! `BrewingStandBlockEntity.serverTick` — the timing, the fuel and the three
//! bottle bits.
//!
//! Transcribed from `.serverTick` (offsets 0-305), `.isBrewable` (0-75),
//! `.doBrew` (0-127) and `.getPotionBits` (0-39) in the pinned 26.2 jar.
//!
//! # What is here, and the one refusal that is left
//!
//! Everything about *when* a brewing stand brews is transcribed: the fuel
//! rule, the 400-tick timer, the two different ways the timer can be
//! cancelled, and the block-state write. *What* the bottles turn into is
//! [`VanillaBrewing`], `PotionBrewing.addVanillaMixes` answered for the stacks
//! this server can hold.
//!
//! This header used to refuse the mix table outright, on the argument that a
//! potion is `minecraft:potion` plus a `POTION_CONTENTS` component and this
//! server carries none. That argument is right about a water bottle and wrong
//! about the stack the server actually holds: a component-less potion is a
//! potion with the item's **default** contents, `PotionContents.EMPTY`, and
//! `PotionBrewing` answers every question about it without reading a potion
//! — see [`VanillaBrewing`]. What remains refused is a stack this server
//! cannot represent (an awkward potion, a water bottle), and that refusal
//! belongs to `crate::item::ItemStack`, not here. [`NoBrewing`] stays as the
//! answer for a caller with no table.
//!
//! # The timer has two cancel paths and they are not the same
//!
//! Offsets 140-159, taken when the timer is running and did **not** hit zero:
//! the timer is reset to 0 if the mix has stopped being brewable **or** the
//! ingredient slot no longer holds the item the brew started with. The second
//! test is `stack.is(this.ingredient)` against a stored `Item`, not against
//! the slot's current stack, so swapping one valid ingredient for another
//! valid ingredient mid-brew cancels it.
//!
//! # The block state write is flag 2 and it is gated on a diff
//!
//! `getPotionBits` is three booleans, one per bottle slot, `!isEmpty()`. The
//! state is only rewritten when they differ from `lastPotionCount`
//! (`Arrays.equals` at offset 227), and then with `setBlock(..., 2)` — no
//! neighbour update. So a brewing stand whose bottles do not change sends
//! nothing however long it runs.

use crate::item::ItemStack;
use crate::recipes::ItemTags;

/// `BrewingStandBlockEntity`'s `NonNullList` length: three bottles, an
/// ingredient and a fuel slot.
pub const BREWING_SLOTS: usize = 5;

/// How many bottle slots there are — the `iconst_3` in `getPotionBits` and
/// in both loops of `isBrewable` and `doBrew`.
pub const BOTTLE_SLOTS: usize = 3;

/// The ingredient slot.
pub const INGREDIENT_SLOT: usize = 3;

/// The fuel slot.
pub const FUEL_SLOT: usize = 4;

/// How many brews one piece of fuel buys — `bipush 20` at serverTick offset
/// 32.
pub const FUEL_PER_ITEM: i32 = 20;

/// How long a brew takes — `sipush 400` at offset 194.
pub const BREW_TIME: i32 = 400;

/// The `level_event` `doBrew` ends with — `sipush 1035` at offset 119, data
/// 0.
pub const BREW_LEVEL_EVENT: i32 = 1035;

/// The tag the fuel slot accepts — `ItemTags.BREWING_FUEL`, whose one member
/// in 26.2 is `minecraft:blaze_powder`. Read from the tag table rather than
/// written down, for the reason `crate::bookshelf` records.
pub const BREWING_FUEL_TAG: &str = "brewing_fuel";

/// The three questions `PotionBrewing` answers.
///
/// A trait rather than a table: see the module header. Every one of them
/// needs a data component this server does not have.
pub trait Brewing {
    /// `PotionBrewing.isIngredient(ItemStack)`.
    fn is_ingredient(&self, stack: &ItemStack) -> bool;
    /// `PotionBrewing.hasMix(ItemStack input, ItemStack ingredient)`.
    fn has_mix(&self, input: &ItemStack, ingredient: &ItemStack) -> bool;
    /// `PotionBrewing.mix(ItemStack ingredient, ItemStack input)`.
    fn mix(&self, ingredient: &ItemStack, input: &ItemStack) -> ItemStack;
}

/// The honest answer for a server with no data components.
///
/// Every question is refused, so a brewing stand fuels, shows its bottles and
/// never starts a brew. **That is a refusal, not a guess**: the alternative,
/// an id-keyed mix table, would turn every potion in the game into every
/// other one because they are all `minecraft:potion`.
pub struct NoBrewing;

impl Brewing for NoBrewing {
    fn is_ingredient(&self, _stack: &ItemStack) -> bool {
        false
    }
    fn has_mix(&self, _input: &ItemStack, _ingredient: &ItemStack) -> bool {
        false
    }
    fn mix(&self, _ingredient: &ItemStack, input: &ItemStack) -> ItemStack {
        // `mix` with no table leaves the bottle alone rather than emptying
        // it. `doBrew` writes this straight back into the slot, so returning
        // an empty stack here would delete the player's bottles.
        input.clone()
    }
}

/// `PotionBrewing.addVanillaMixes` — answered for the stacks this server can
/// hold, which is every stack **with its default components**.
///
/// # Why the refusal above was too wide, and what is left of it
///
/// [`NoBrewing`] refuses every question on the argument that a potion is
/// `minecraft:potion` plus a `POTION_CONTENTS` component and this server has
/// no components. That is true of a *water bottle* and false of the stack this
/// server actually holds: a component-less `minecraft:potion` is a potion
/// whose `POTION_CONTENTS` is the item's default, `PotionContents.EMPTY`, and
/// vanilla answers every question about that stack without consulting a
/// single potion:
///
/// * `isIngredient` (offsets 0-21) is `isContainerIngredient ||
///   isPotionIngredient` — two walks over `Mix.ingredient`, an item test, no
///   component read;
/// * `hasMix` (0-33) is `isContainer(input) && (hasContainerMix ||
///   hasPotionMix)`. `hasContainerMix` (0-60) tests `input.is(mix.from)` and
///   the ingredient — items again. `hasPotionMix` opens with
///   `getOrDefault(POTION_CONTENTS, EMPTY).potion()` and **returns false on
///   an empty `Optional`** (offsets 0-25), so for a component-less stack it
///   is false without reading the table;
/// * `mix` (0-35) returns `potion` unchanged on an empty stack **and on an
///   empty `potion()`**, before either list is walked.
///
/// So for every stack this server can represent, brewing is exact: a
/// component-less potion over gunpowder *is* brewable (a container mix), the
/// stand runs its full 400 ticks, consumes the gunpowder, sends `1035` — and
/// the bottle comes out unchanged, because that is what `mix` does with no
/// potion to carry across. What stays refused is the stack this server cannot
/// hold — a water bottle, an awkward potion — and that refusal is the
/// `ItemStack` model's, not this table's.
///
/// The three lists are transcribed from `addVanillaMixes` in call order. Every
/// `addMix`/`addStartMix`/`addContainerRecipe` there is feature-gated by
/// `Item.isEnabled` / `Potion.isEnabled`, and in 26.2 every item and potion it
/// names is in the vanilla feature set, so none is dropped.
pub struct VanillaBrewing;

/// `addContainer` — the three items `isContainer` accepts.
pub const CONTAINERS: [&str; 3] = [
    "minecraft:potion",
    "minecraft:splash_potion",
    "minecraft:lingering_potion",
];

/// `addContainerRecipe(from, ingredient, to)` — the two container mixes, in
/// call order.
pub const CONTAINER_MIXES: [(&str, &str, &str); 2] = [
    (
        "minecraft:potion",
        "minecraft:gunpowder",
        "minecraft:splash_potion",
    ),
    (
        "minecraft:splash_potion",
        "minecraft:dragon_breath",
        "minecraft:lingering_potion",
    ),
];

/// Every distinct `Item` an `addMix` or `addStartMix` names as its
/// ingredient — what `isPotionIngredient` walks. `addStartMix(item, potion)`
/// is two `addMix` calls on the same item (from `WATER` to `MUNDANE` and from
/// `AWKWARD` to `potion`), so it contributes one ingredient here.
pub const POTION_INGREDIENTS: [&str; 19] = [
    "minecraft:glowstone_dust",
    "minecraft:redstone",
    "minecraft:nether_wart",
    "minecraft:breeze_rod",
    "minecraft:slime_block",
    "minecraft:stone",
    "minecraft:cobweb",
    "minecraft:golden_carrot",
    "minecraft:fermented_spider_eye",
    "minecraft:magma_cream",
    "minecraft:rabbit_foot",
    "minecraft:turtle_helmet",
    "minecraft:sugar",
    "minecraft:pufferfish",
    "minecraft:glistering_melon_slice",
    "minecraft:spider_eye",
    "minecraft:ghast_tear",
    "minecraft:blaze_powder",
    "minecraft:phantom_membrane",
];

impl Brewing for VanillaBrewing {
    fn is_ingredient(&self, stack: &ItemStack) -> bool {
        let item = stack.item();
        CONTAINER_MIXES.iter().any(|(_, i, _)| *i == item) || POTION_INGREDIENTS.contains(&item)
    }
    fn has_mix(&self, input: &ItemStack, ingredient: &ItemStack) -> bool {
        if !CONTAINERS.contains(&input.item()) {
            return false;
        }
        // `hasContainerMix`; `hasPotionMix` is false for a component-less
        // stack, see the type's doc.
        CONTAINER_MIXES
            .iter()
            .any(|(from, i, _)| *from == input.item() && *i == ingredient.item())
    }
    fn mix(&self, _ingredient: &ItemStack, input: &ItemStack) -> ItemStack {
        // Offsets 0-34: an empty stack, then an empty `potion()`, both return
        // `potion` itself. Every non-empty stack here has the default
        // contents, so the second return is the one taken.
        input.clone()
    }
}

/// `Item.getCraftingRemainder()` for the brewing ingredients that have one.
///
/// Read from `Items`'s `static {}`: five items are registered with
/// `Item.Properties.craftRemainder`, and of the twenty-one brewing
/// ingredients only `DRAGON_BREATH` is among them, with `GLASS_BOTTLE`.
#[must_use]
pub fn crafting_remainder(item: &str) -> Option<&'static str> {
    (item == "minecraft:dragon_breath").then_some("minecraft:glass_bottle")
}

/// The brewing stand's own fields.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BrewingStand {
    /// `brewTime`.
    pub brew_time: i32,
    /// `fuel`.
    pub fuel: i32,
    /// `ingredient` — the **item** the current brew started with, not the
    /// stack.
    pub ingredient: Option<String>,
    /// `lastPotionCount`.
    pub last_potion_bits: [bool; BOTTLE_SLOTS],
}

/// What one brewing tick did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BrewTick {
    /// `setChanged(level, pos, state)` ran.
    pub changed: bool,
    /// A brew completed: `level_event(1035, pos, 0)`.
    pub brewed: bool,
    /// The three `has_bottle_N` booleans, when they changed and the block
    /// state was rewritten with `setBlock(..., 2)`.
    pub bottle_bits: Option<[bool; BOTTLE_SLOTS]>,
    /// Stacks `Containers.dropItemStack` was called with by `doBrew`'s
    /// crafting-remainder branch.
    pub drops: Vec<ItemStack>,
}

/// `getPotionBits()` — offsets 0-39.
#[must_use]
pub fn potion_bits(slots: &[ItemStack]) -> [bool; BOTTLE_SLOTS] {
    std::array::from_fn(|i| !slots[i].is_empty())
}

/// `isBrewable(PotionBrewing, NonNullList)` — offsets 0-75.
///
/// Three gates in order: the ingredient slot is non-empty, it **is** an
/// ingredient, and at least one bottle slot has a mix with it. The bottle
/// loop returns on the **first** hit, so one brewable bottle is enough for
/// all three to brew.
#[must_use]
pub fn is_brewable(brewing: &dyn Brewing, slots: &[ItemStack]) -> bool {
    let ingredient = &slots[INGREDIENT_SLOT];
    if ingredient.is_empty() || !brewing.is_ingredient(ingredient) {
        return false;
    }
    (0..BOTTLE_SLOTS).any(|i| !slots[i].is_empty() && brewing.has_mix(&slots[i], ingredient))
}

/// `doBrew(Level, BlockPos, NonNullList)` — offsets 0-127.
///
/// Mixes **all three** bottle slots, including empty ones — the loop has no
/// `isEmpty` guard, so `mix` is asked about an empty bottle and its answer is
/// written back. Then the ingredient shrinks by one and, if it has a crafting
/// remainder, the remainder either takes the slot (when the shrink emptied
/// it) or is dropped on the floor.
pub fn do_brew(brewing: &dyn Brewing, slots: &mut [ItemStack], out: &mut BrewTick) {
    let ingredient = slots[INGREDIENT_SLOT].clone();
    for bottle in slots.iter_mut().take(BOTTLE_SLOTS) {
        *bottle = brewing.mix(&ingredient, bottle);
    }
    slots[INGREDIENT_SLOT].shrink(1);
    // Offsets 69-112: the remainder takes the slot when the shrink emptied
    // it, and is dropped at the stand otherwise. Dragon's breath is the one
    // vanilla ingredient with one — see [`crafting_remainder`].
    if let Some(rest) = crafting_remainder(ingredient.item()) {
        if slots[INGREDIENT_SLOT].is_empty() {
            slots[INGREDIENT_SLOT] = ItemStack::one(rest);
        } else {
            out.drops.push(ItemStack::one(rest));
        }
    }
    out.brewed = true;
}

/// `BrewingStandBlockEntity.serverTick` — offsets 0-305.
///
/// `slots` is the five-slot list. Returns what the tick emitted.
pub fn server_tick(
    stand: &mut BrewingStand,
    slots: &mut [ItemStack],
    brewing: &dyn Brewing,
    tags: &ItemTags,
) -> BrewTick {
    let mut out = BrewTick::default();
    if slots.len() < BREWING_SLOTS {
        return out;
    }
    // Offsets 13-46. **The fuel test is `fuel <= 0`, not `fuel == 0`**, and
    // it consumes one blaze powder for twenty brews. It runs before anything
    // else, so a stand refuelled this tick can start a brew on the same tick.
    if stand.fuel <= 0 && tags.contains(BREWING_FUEL_TAG, slots[FUEL_SLOT].item()) {
        stand.fuel = FUEL_PER_ITEM;
        slots[FUEL_SLOT].shrink(1);
        out.changed = true;
    }
    let brewable = is_brewable(brewing, slots);
    let brewing_now = stand.brew_time > 0;
    let ingredient = slots[INGREDIENT_SLOT].clone();
    if brewing_now {
        stand.brew_time -= 1;
        let done = stand.brew_time == 0;
        if done && brewable {
            do_brew(brewing, slots, &mut out);
        } else if !brewable || ingredient.item() != stand.ingredient.as_deref().unwrap_or("") {
            // Offsets 140-159. Note the second test compares the slot's
            // **item** against the `Item` stored when the brew began, so
            // swapping one valid ingredient for another cancels the brew.
            stand.brew_time = 0;
        }
        out.changed = true;
    } else if brewable && stand.fuel > 0 {
        stand.fuel -= 1;
        stand.brew_time = BREW_TIME;
        stand.ingredient = Some(ingredient.item().to_string());
        out.changed = true;
    }

    let bits = potion_bits(slots);
    if bits != stand.last_potion_bits {
        stand.last_potion_bits = bits;
        out.bottle_bits = Some(bits);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags() -> ItemTags {
        ItemTags::parse(&serde_json::json!({
            "brewing_fuel": {"values": ["minecraft:blaze_powder"]}
        }))
        .expect("tag parses")
    }

    /// A stub `PotionBrewing` for the timing tests: awkward flower is an
    /// ingredient and mixes with a water bottle. It exists so the ticker's
    /// control flow can be exercised; it is not a claim about vanilla's mix
    /// table, which this module refuses.
    struct StubBrewing;

    impl Brewing for StubBrewing {
        fn is_ingredient(&self, stack: &ItemStack) -> bool {
            matches!(stack.item(), "minecraft:nether_wart" | "minecraft:redstone")
        }
        fn has_mix(&self, input: &ItemStack, _ingredient: &ItemStack) -> bool {
            input.item() == "minecraft:potion"
        }
        fn mix(&self, _ingredient: &ItemStack, input: &ItemStack) -> ItemStack {
            if input.is_empty() {
                return input.clone();
            }
            ItemStack::one("minecraft:brewed")
        }
    }

    fn slots(bottles: usize, ingredient: &str, fuel: &str) -> Vec<ItemStack> {
        let mut v = vec![ItemStack::empty(); BREWING_SLOTS];
        for s in v.iter_mut().take(bottles) {
            *s = ItemStack::one("minecraft:potion");
        }
        if !ingredient.is_empty() {
            v[INGREDIENT_SLOT] = ItemStack::one(ingredient);
        }
        if !fuel.is_empty() {
            v[FUEL_SLOT] = ItemStack::one(fuel);
        }
        v
    }

    /// One blaze powder is twenty brews, and the fuel is taken on the tick
    /// the stand needs it — before the brew starts, so both happen at once.
    #[test]
    fn one_blaze_powder_fuels_twenty_and_the_brew_starts_the_same_tick() {
        let mut st = BrewingStand::default();
        let mut s = slots(1, "minecraft:nether_wart", "minecraft:blaze_powder");
        let out = server_tick(&mut st, &mut s, &StubBrewing, &tags());
        assert_eq!(st.fuel, FUEL_PER_ITEM - 1, "twenty, minus this brew");
        assert_eq!(st.brew_time, BREW_TIME);
        assert!(s[FUEL_SLOT].is_empty());
        assert!(out.changed);
    }

    /// The negative control: no fuel, no brew, and the timer stays at zero.
    #[test]
    fn a_stand_with_no_fuel_does_not_start() {
        let mut st = BrewingStand::default();
        let mut s = slots(1, "minecraft:nether_wart", "");
        server_tick(&mut st, &mut s, &StubBrewing, &tags());
        assert_eq!(st.brew_time, 0);
        assert_eq!(st.fuel, 0);
    }

    /// A non-fuel item in the fuel slot is left alone.
    #[test]
    fn only_the_tagged_fuel_is_consumed() {
        let mut st = BrewingStand::default();
        let mut s = slots(1, "minecraft:nether_wart", "minecraft:stone");
        server_tick(&mut st, &mut s, &StubBrewing, &tags());
        assert_eq!(st.fuel, 0);
        assert_eq!(s[FUEL_SLOT].item(), "minecraft:stone");
    }

    /// Four hundred ticks and the brew completes, mixing every bottle slot.
    #[test]
    fn the_brew_completes_after_four_hundred_ticks() {
        let mut st = BrewingStand::default();
        let mut s = slots(3, "minecraft:nether_wart", "minecraft:blaze_powder");
        let mut last = BrewTick::default();
        for _ in 0..=BREW_TIME {
            last = server_tick(&mut st, &mut s, &StubBrewing, &tags());
        }
        assert!(last.brewed);
        assert_eq!(st.brew_time, 0);
        for bottle in s.iter().take(BOTTLE_SLOTS) {
            assert_eq!(bottle.item(), "minecraft:brewed");
        }
        assert!(s[INGREDIENT_SLOT].is_empty(), "the ingredient was consumed");
    }

    /// Swapping one **valid** ingredient for another mid-brew cancels it,
    /// because the test is against the stored `Item` and not against
    /// `isIngredient`.
    #[test]
    fn swapping_a_valid_ingredient_mid_brew_cancels_it() {
        let mut st = BrewingStand::default();
        let mut s = slots(1, "minecraft:nether_wart", "minecraft:blaze_powder");
        server_tick(&mut st, &mut s, &StubBrewing, &tags());
        assert_eq!(st.brew_time, BREW_TIME);
        s[INGREDIENT_SLOT] = ItemStack::one("minecraft:redstone");
        server_tick(&mut st, &mut s, &StubBrewing, &tags());
        assert_eq!(
            st.brew_time, 0,
            "a different valid ingredient still cancels"
        );
    }

    /// Taking the bottles out mid-brew cancels it too — the other of the two
    /// cancel paths.
    #[test]
    fn removing_the_bottles_mid_brew_cancels_it() {
        let mut st = BrewingStand::default();
        let mut s = slots(1, "minecraft:nether_wart", "minecraft:blaze_powder");
        server_tick(&mut st, &mut s, &StubBrewing, &tags());
        s[0] = ItemStack::empty();
        server_tick(&mut st, &mut s, &StubBrewing, &tags());
        assert_eq!(st.brew_time, 0);
    }

    /// The bottle bits are reported only when they change, so a running
    /// stand whose bottles are steady sends no block update.
    #[test]
    fn the_bottle_bits_are_reported_only_on_a_change() {
        let mut st = BrewingStand::default();
        let mut s = slots(2, "minecraft:nether_wart", "minecraft:blaze_powder");
        let out = server_tick(&mut st, &mut s, &StubBrewing, &tags());
        assert_eq!(out.bottle_bits, Some([true, true, false]));
        let out = server_tick(&mut st, &mut s, &StubBrewing, &tags());
        assert_eq!(out.bottle_bits, None, "nothing changed, nothing sent");
        s[2] = ItemStack::one("minecraft:potion");
        let out = server_tick(&mut st, &mut s, &StubBrewing, &tags());
        assert_eq!(out.bottle_bits, Some([true, true, true]));
    }

    /// `NoBrewing` refuses everything, so a real stand with no potion table
    /// fuels and shows its bottles and never brews. And its `mix` returns the
    /// input rather than an empty stack, so the bottles survive.
    #[test]
    fn norules_brewing_fuels_but_never_brews() {
        let mut st = BrewingStand::default();
        let mut s = slots(3, "minecraft:nether_wart", "minecraft:blaze_powder");
        for _ in 0..500 {
            server_tick(&mut st, &mut s, &NoBrewing, &tags());
        }
        assert_eq!(st.brew_time, 0);
        assert_eq!(st.fuel, FUEL_PER_ITEM, "fuelled, never spent");
        for bottle in s.iter().take(BOTTLE_SLOTS) {
            assert_eq!(bottle.item(), "minecraft:potion", "the bottles survive");
        }
    }

    /// `isBrewable` needs one brewable bottle, not three.
    #[test]
    fn one_brewable_bottle_is_enough() {
        let s = slots(1, "minecraft:nether_wart", "");
        assert!(is_brewable(&StubBrewing, &s));
        let s = slots(0, "minecraft:nether_wart", "");
        assert!(!is_brewable(&StubBrewing, &s), "no bottles at all");
        let s = slots(3, "minecraft:stone", "");
        assert!(!is_brewable(&StubBrewing, &s), "not an ingredient");
    }
}
