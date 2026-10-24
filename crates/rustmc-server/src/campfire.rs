//! `CampfireBlockEntity` — four cooking slots, two tickers and a drop.
//!
//! Transcribed from `CampfireBlockEntity.cookTick` (offsets 0-198),
//! `.cooldownTick` (0-75) and `.placeFood` (0-148) in the pinned 26.2 jar.
//!
//! # Which ticker runs is a property of the block state, not of the entity
//!
//! `CampfireBlock.getTicker` reads `LIT` and hands back **`cookTick` when it
//! is true and `cooldownTick` when it is false** (offsets 13-53 against the
//! `ifeq 54`). So a campfire that is doused stops cooking and starts *losing*
//! progress on the same tick its state changes, and neither ticker ever tests
//! `LIT` itself. A single ticker with a `lit` branch inside it would behave
//! the same and is not what the jar does; keeping them apart is what makes
//! [`cooldown_tick`] callable on a soul campfire, a doused one and a waterlogged
//! one without any of them needing a flag.
//!
//! # The two loops are not the same loop
//!
//! `cookTick` completes on `progress >= time` — the `if_icmplt 181` at offset
//! 71 *skips* the completion when progress is less, so equality completes.
//! `AbstractFurnaceBlockEntity` uses `if_icmpne`, strict equality, one class
//! away. They are different tests and only the furnace's can be overshot.
//!
//! `cooldownTick` decays by **two** and clamps into `[0, cookingTime[i]]`,
//! which is the same arithmetic the furnace's own decay branch uses.
//!
//! # What a completed campfire slot does that a furnace does not
//!
//! It **drops the item on the floor** — `Containers.dropItemStack` at the
//! block's own x, y, z — empties the slot, sends a block update with flag 3,
//! and fires a `BLOCK_CHANGE` game event. There is no result slot. The drop
//! and the game event are not implemented here and are reported instead: see
//! [`CampfireTick::drops`].

use crate::item::ItemStack;
use crate::recipes::{RecipeType, Recipes};

/// `CampfireBlockEntity`'s `NonNullList` length, and the length of both
/// `cookingProgress` and `cookingTime`.
///
/// Four, from `NonNullList.withSize(4, ItemStack.EMPTY)` in the constructor.
/// The three arrays are indexed together and `cookTick` walks `items.size()`,
/// so a mismatch between them would be an out-of-bounds in vanilla and is a
/// silent truncation here — which is why [`CampfireState`] holds them as one
/// fixed-length array of triples rather than three loose vectors.
pub const CAMPFIRE_SLOTS: usize = 4;

/// One campfire's cooking state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CampfireState {
    /// `cookingProgress[i]`.
    pub progress: [i32; CAMPFIRE_SLOTS],
    /// `cookingTime[i]` — set from the recipe when the food is placed, and
    /// **not** recomputed afterwards. A campfire whose recipe changed under
    /// it keeps the time it was loaded with, which is vanilla.
    pub time: [i32; CAMPFIRE_SLOTS],
}

impl Default for CampfireState {
    fn default() -> Self {
        Self {
            progress: [0; CAMPFIRE_SLOTS],
            time: [0; CAMPFIRE_SLOTS],
        }
    }
}

/// What one campfire tick did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CampfireTick {
    /// `setChanged(level, pos, state)` ran — local 5 in `cookTick`, local 4
    /// in `cooldownTick`.
    ///
    /// **`cookTick` sets it for any non-empty slot**, not only for one that
    /// finished (offset 43, inside the `isEmpty` continue). A campfire with
    /// food on it is `setChanged` every tick it burns.
    pub changed: bool,
    /// Stacks `Containers.dropItemStack` was called with, in slot order.
    ///
    /// Vanilla drops them into the world at the block's own position. This
    /// server has `crate::dropitem`, and wiring the two together is the
    /// caller's job; reporting them is what makes a finished cook observable
    /// rather than inferred from an emptied slot.
    pub drops: Vec<ItemStack>,
    /// Slots that finished this tick and so need `sendBlockUpdated(pos,
    /// state, state, 3)` and a `BLOCK_CHANGE` game event.
    pub finished: Vec<usize>,
}

/// `CampfireBlockEntity.cookTick` — offsets 0-198. The ticker a **lit**
/// campfire gets.
pub fn cook_tick(
    state: &mut CampfireState,
    items: &mut [ItemStack],
    recipes: &Recipes,
) -> CampfireTick {
    let mut out = CampfireTick::default();
    for i in 0..items.len().min(CAMPFIRE_SLOTS) {
        if items[i].is_empty() {
            continue;
        }
        // Offset 43: `changed` is set for a non-empty slot whether or not
        // anything finishes.
        out.changed = true;
        state.progress[i] += 1;
        if state.progress[i] < state.time[i] {
            continue;
        }
        // `getRecipeFor(...).map(assemble).orElse(theRawStack)` — offsets
        // 85-110. **A slot whose recipe has gone away drops the raw food**
        // rather than staying on the fire, because the `orElse` hands back
        // the input. That is a real difference from the furnace, which does
        // nothing at all without a recipe.
        let assembled = recipes
            .find_single(RecipeType::CampfireCooking, &items[i])
            .map_or_else(|| items[i].clone(), |r| r.result.clone());
        // `isItemEnabled(level.enabledFeatures())` at offsets 115-124. Every
        // item this server can produce is enabled under `DEFAULT_FLAGS`, so
        // the test is transcribed as its answer and named rather than left
        // out: the day an experimental item reaches a campfire, this is the
        // guard that should have refused it.
        out.drops.push(assembled);
        items[i] = ItemStack::empty();
        out.finished.push(i);
    }
    out
}

/// `CampfireBlockEntity.cooldownTick` — offsets 0-75. The ticker an **unlit**
/// campfire gets.
///
/// Only a slot with progress `> 0` is touched — the `ifle 58` at offset 25 —
/// so a campfire holding raw food it never started cooking reports no change
/// at all, which is what stops an unlit campfire marking its chunk dirty
/// forever.
pub fn cooldown_tick(state: &mut CampfireState, slots: usize) -> CampfireTick {
    let mut out = CampfireTick::default();
    for i in 0..slots.min(CAMPFIRE_SLOTS) {
        if state.progress[i] <= 0 {
            continue;
        }
        out.changed = true;
        state.progress[i] = (state.progress[i] - 2).clamp(0, state.time[i]);
    }
    out
}

/// `CampfireBlockEntity.placeFood(ServerLevel, LivingEntity, ItemStack)` —
/// offsets 0-148.
///
/// Walks the slots in index order and takes the **first empty one**; a
/// campfire with slots 0 and 2 free puts the food in 0. Returns whether it was
/// placed.
///
/// Two things happen in an order that matters: `cookingTime[i]` is set from
/// the recipe *before* `cookingProgress[i]` is zeroed and before the item is
/// written. And **the recipe lookup happens inside the loop**, so a food with
/// no campfire recipe returns false from the first empty slot rather than
/// walking on — which is only observable through the `markUpdated` that does
/// not happen.
pub fn place_food(
    state: &mut CampfireState,
    items: &mut [ItemStack],
    stack: &ItemStack,
    recipes: &Recipes,
) -> bool {
    for i in 0..items.len().min(CAMPFIRE_SLOTS) {
        if !items[i].is_empty() {
            continue;
        }
        let Some(recipe) = recipes.find_single(RecipeType::CampfireCooking, stack) else {
            // `Optional.isEmpty()` at offset 60 returns **false from the
            // method**, not `continue` to the next slot.
            return false;
        };
        state.time[i] = recipe.cooking_time();
        state.progress[i] = 0;
        // `stack.consumeAndReturn(1, entity)` — one item off the player's
        // stack, and the campfire gets a single-item copy of it.
        items[i] = stack.copy_with_count(1);
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recipes() -> Recipes {
        Recipes::parse(
            &serde_json::json!({
                "baked_potato_from_campfire_cooking": {
                    "type": "minecraft:campfire_cooking",
                    "cookingtime": 600,
                    "ingredient": "minecraft:potato",
                    "result": {"id": "minecraft:baked_potato"}
                }
            }),
            &serde_json::json!({}),
        )
        .expect("recipes parse")
    }

    fn empty_items() -> Vec<ItemStack> {
        vec![ItemStack::empty(); CAMPFIRE_SLOTS]
    }

    /// Placing food takes the first empty slot and stamps that slot's time
    /// from the recipe.
    #[test]
    fn placing_food_takes_the_first_empty_slot_and_stamps_its_cooking_time() {
        let mut st = CampfireState::default();
        let mut items = empty_items();
        items[0] = ItemStack::one("minecraft:beef");
        let potato = ItemStack::new("minecraft:potato", 9);
        assert!(place_food(&mut st, &mut items, &potato, &recipes()));
        assert_eq!(items[1].item(), "minecraft:potato");
        assert_eq!(items[1].count(), 1, "one item, not the whole stack");
        assert_eq!(st.time[1], 600);
        assert_eq!(st.time[0], 0, "the occupied slot is untouched");
    }

    /// The negative control: food with no campfire recipe is refused.
    #[test]
    fn food_with_no_recipe_is_refused() {
        let mut st = CampfireState::default();
        let mut items = empty_items();
        assert!(!place_food(
            &mut st,
            &mut items,
            &ItemStack::one("minecraft:diamond"),
            &recipes()
        ));
        assert!(items[0].is_empty());
    }

    /// The completion test is `>=`, so the tick on which progress reaches the
    /// time is the tick the food drops. One less and nothing happens.
    #[test]
    fn a_cook_completes_on_the_tick_progress_reaches_the_time() {
        let r = recipes();
        let mut st = CampfireState::default();
        let mut items = empty_items();
        place_food(&mut st, &mut items, &ItemStack::one("minecraft:potato"), &r);
        st.progress[0] = 598;
        let out = cook_tick(&mut st, &mut items, &r);
        assert!(out.drops.is_empty(), "599 of 600 is not done");
        assert!(out.changed, "a burning slot still marks changed");
        let out = cook_tick(&mut st, &mut items, &r);
        assert_eq!(out.drops.len(), 1);
        assert_eq!(out.drops[0].item(), "minecraft:baked_potato");
        assert_eq!(out.finished, vec![0usize]);
        assert!(items[0].is_empty());
    }

    /// A finished slot whose recipe has gone away drops the **raw** item,
    /// from the `orElse(theRawStack)` at offset 107. The furnace does nothing
    /// in the same situation; this is the difference.
    #[test]
    fn a_finished_slot_with_no_recipe_drops_the_raw_food() {
        let r = recipes();
        let empty_recipes =
            Recipes::parse(&serde_json::json!({}), &serde_json::json!({})).expect("empty");
        let mut st = CampfireState::default();
        let mut items = empty_items();
        place_food(&mut st, &mut items, &ItemStack::one("minecraft:potato"), &r);
        st.progress[0] = 600;
        let out = cook_tick(&mut st, &mut items, &empty_recipes);
        assert_eq!(out.drops.len(), 1);
        assert_eq!(out.drops[0].item(), "minecraft:potato");
    }

    /// An empty campfire reports no change, so it does not mark its chunk
    /// dirty every tick.
    #[test]
    fn an_empty_campfire_changes_nothing() {
        let mut st = CampfireState::default();
        let mut items = empty_items();
        let out = cook_tick(&mut st, &mut items, &recipes());
        assert!(!out.changed);
        assert!(out.drops.is_empty());
    }

    /// The cooldown decays by two, clamps at zero, and skips a slot that was
    /// never started.
    #[test]
    fn the_cooldown_decays_by_two_and_leaves_an_unstarted_slot_alone() {
        let mut st = CampfireState::default();
        st.progress[0] = 3;
        st.time[0] = 600;
        let out = cooldown_tick(&mut st, CAMPFIRE_SLOTS);
        assert!(out.changed);
        assert_eq!(st.progress[0], 1);
        cooldown_tick(&mut st, CAMPFIRE_SLOTS);
        assert_eq!(st.progress[0], 0);
        // At zero it stops reporting a change at all.
        let out = cooldown_tick(&mut st, CAMPFIRE_SLOTS);
        assert!(!out.changed);
    }

    /// The clamp's upper bound is that slot's own `cookingTime`, so a
    /// progress somehow above it is pulled **down** rather than left alone.
    #[test]
    fn the_cooldown_clamps_down_to_the_slots_own_cooking_time() {
        let mut st = CampfireState::default();
        st.progress[0] = 900;
        st.time[0] = 100;
        cooldown_tick(&mut st, CAMPFIRE_SLOTS);
        assert_eq!(st.progress[0], 100);
    }
}
