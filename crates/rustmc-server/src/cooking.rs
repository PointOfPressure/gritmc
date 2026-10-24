//! `AbstractFurnaceBlockEntity.serverTick` — the furnace, blast furnace and
//! smoker, and the cooking recipes all three read.
//!
//! Transcribed from `net/minecraft/world/level/block/entity/
//! AbstractFurnaceBlockEntity.serverTick`, offsets 0-414, in the pinned 26.2
//! server jar. The three block entities differ in exactly one field — the
//! `RecipeType` their constructor hands the superclass — so there is one
//! ticker here and a [`CookingKind`] beside it.
//!
//! # The four fields and the order they move in
//!
//! `litTimeRemaining` is decremented **first**, before anything else is read
//! (offsets 3-35), and `wasLit`/`isLit` are sampled around that decrement.
//! The pair is compared again at the very end (offsets 369-373) and a
//! difference is what writes `AbstractFurnaceBlock.LIT` with flag 3. So a
//! furnace whose fuel runs out this tick sends its block update on the tick
//! the fuel ran out, not the tick after.
//!
//! `cookingTimer` is the one with four different resets, and which one fires
//! is the whole shape of the method:
//!
//! * **not lit and cannot light** (offset 344) — `Mth.clamp(timer - 2, 0,
//!   cookingTotalTime)`, the only place the progress *decays* rather than
//!   resetting. Minus **two** per tick, so a furnace loses progress twice as
//!   fast as it gains it;
//! * **no input** (offset 336) — straight to 0;
//! * **input but no recipe** — the timer is **not touched at all**; control
//!   falls from the null check at 156 to 333 past every write. An
//!   implementation that zeroed it here would be wrong for the one tick
//!   between taking a smeltable out and putting an unsmeltable in;
//! * **recipe but the result will not fit** (offset 328) — 0;
//! * **lit, cooking, and the timer reaches `cookingTotalTime`** (offset 277)
//!   — 0, and then the recipe's own `cookingTime()` becomes the new total.
//!
//! The completion test at offset 270 is `if_icmpne`, so it is **equality**
//! and not `>=`. That matters because `cookingTotalTime` is rewritten on
//! every completion: a timer that overshot would never equal it again.

use crate::fuel::FuelValues;
use crate::item::ItemStack;
use crate::recipes::Recipes;

/// `Container.getMaxStackSize()` — the interface default, `bipush 99`.
///
/// Neither `AbstractFurnaceBlockEntity` nor `BaseContainerBlockEntity`
/// overrides it, checked with `javap -p` on both. It reaches `canBurn` as the
/// left half of a `Math.min` against the assembled stack's own max, so for
/// every vanilla item the effective cap is 64 and this constant is invisible —
/// which is exactly why it is written down rather than folded away.
pub const CONTAINER_MAX_STACK_SIZE: i32 = 99;

/// Which `RecipeType` a block entity cooks with.
///
/// # There is one cooking table on this server now, and it is the matcher's
///
/// This module used to carry a second parser over the same 116 documents:
/// `CookingRecipes`, its own `CookingRecipe`, its own `parse_result` and its
/// own `find`. That is a table checked against a second copy of itself —
/// `crate::recipes` reads the identical file, and the two could disagree
/// about which recipe wins with nothing on either side to say so. The four
/// defects `ci/recipe-match-golden.sh` found in the crafting half are the
/// argument: an ingredient written as a JSON **array** is legal wherever a
/// bare id is, a `#tag` may name another tag, and `getRecipeFor` is
/// `findFirst` over a bucket sorted by recipe id — so the winner is a
/// property of the whole table's order and has no local symptom. A furnace
/// running its own parser would have had to re-find all four.
///
/// So the furnace family looks up through `crate::recipes::Recipes::
/// find_single`, which is `RecipeManager.getRecipeFor(type, new
/// SingleRecipeInput(stack), level)` for the five one-slot types, and is
/// gated against vanilla's own answer over 3,122 vectors.
pub use crate::recipes::CookingKind;

impl CookingKind {
    /// Which kind a block cooks with, from its `RecipeType` field.
    ///
    /// `None` for anything that is not one of the three furnaces, which is
    /// what stops a chest being ticked as one. A campfire is deliberately
    /// absent: `CampfireBlock.getTicker` hands out `CampfireBlockEntity
    /// .cookTick`, a different ticker with a different completion test, and
    /// `crate::campfire` names its own kind.
    #[must_use]
    pub fn for_furnace_block(block: &str) -> Option<Self> {
        Some(match block {
            "minecraft:furnace" => Self::Smelting,
            "minecraft:blast_furnace" => Self::Blasting,
            "minecraft:smoker" => Self::Smoking,
            _ => return None,
        })
    }
}

/// The four ints an `AbstractFurnaceBlockEntity` carries.
///
/// Named as vanilla names them. `litTimeRemaining` and `litTotalTime` are two
/// fields and not one: the second is only ever written beside the first
/// (offsets 219-228) and exists so the client can draw the flame at the right
/// height, which is why a furnace refuelled early keeps a full flame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FurnaceState {
    /// `litTimeRemaining`.
    pub lit_time_remaining: i32,
    /// `litTotalTime`.
    pub lit_total_time: i32,
    /// `cookingTimer`.
    pub cooking_timer: i32,
    /// `cookingTotalTime`.
    pub cooking_total_time: i32,
}

/// What one `serverTick` did, for a caller that has to write it back.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FurnaceTick {
    /// `setChanged(level, pos, state)` was called — local 4.
    pub changed: bool,
    /// `AbstractFurnaceBlock.LIT` changed and `setBlock(pos, state, 3)` ran.
    /// `None` when it did not; the flag-3 write is what the client sees.
    pub lit_changed: Option<bool>,
    /// The id of the recipe `setRecipeUsed` recorded this tick, if one
    /// completed. Nothing consumes it yet — the experience ledger is not
    /// ported — and it is here so a completion is observable rather than
    /// inferred from the items.
    pub recipe_used: Option<String>,
}

/// `AbstractFurnaceBlockEntity.serverTick(level, pos, state, be)` — offsets
/// 0-414.
///
/// `slots` is the three-slot `NonNullList`: 0 ingredient, 1 fuel, 2 result.
/// `lit` is the block state's current `LIT`, which vanilla reads off the
/// state rather than off the block entity and which is why a state and a
/// block entity that disagree resynchronise on the next tick.
#[allow(clippy::too_many_lines)]
pub fn server_tick(
    state: &mut FurnaceState,
    slots: &mut [ItemStack],
    kind: CookingKind,
    recipes: &Recipes,
    fuels: &FuelValues,
) -> FurnaceTick {
    let mut out = FurnaceTick::default();
    if slots.len() < 3 {
        return out;
    }
    // Offsets 3-45. `wasLit` is "was it lit when this tick began", `is_lit`
    // is "is it lit after the decrement" — and the decrement happens before
    // either is used for anything else.
    let was_lit;
    let mut is_lit;
    if state.lit_time_remaining > 0 {
        was_lit = true;
        state.lit_time_remaining -= 1;
        is_lit = state.lit_time_remaining > 0;
    } else {
        was_lit = false;
        is_lit = false;
    }

    let fuel = slots[1].clone();
    let input = slots[0].clone();
    let has_input = !input.is_empty();
    let has_fuel = !fuel.is_empty();

    // Offsets 102-114: `isLit || (hasFuel && hasInput)`. Note the operand
    // order — vanilla tests `hasFuel` first — which is only observable if one
    // of them had a side effect, and neither does.
    if is_lit || (has_fuel && has_input) {
        if has_input {
            let recipe = recipes.find_single(kind.recipe_type(), &input).cloned();
            if let Some(recipe) = recipe {
                let assembled = recipe.result.clone();
                if !assembled.is_empty() && can_burn(slots, CONTAINER_MAX_STACK_SIZE, &assembled) {
                    // Offsets 202-249: light it, but only if it is not lit
                    // already. A burn duration of 0 lights nothing **and
                    // consumes nothing** — the `ifle 251` at offset 231 skips
                    // both `consumeFuel` and the two flag writes, while
                    // `litTimeRemaining` and `litTotalTime` have already been
                    // set to that 0 at offsets 219-228.
                    if !is_lit {
                        let burn = fuels.burn_duration(&fuel);
                        state.lit_time_remaining = burn;
                        state.lit_total_time = burn;
                        if burn > 0 {
                            consume_fuel(slots);
                            is_lit = true;
                            out.changed = true;
                        }
                    }
                    if is_lit {
                        state.cooking_timer += 1;
                        // `if_icmpne` — equality, not `>=`.
                        if state.cooking_timer == state.cooking_total_time {
                            state.cooking_timer = 0;
                            state.cooking_total_time = recipe.cooking_time();
                            burn_result(slots, &assembled);
                            out.recipe_used = Some(recipe.id.clone());
                            out.changed = true;
                        }
                    } else {
                        state.cooking_timer = 0;
                    }
                } else {
                    state.cooking_timer = 0;
                }
            }
            // No recipe: offset 156's `ifnull` jumps to 333 and the timer is
            // **not** written. Deliberately no `else` here.
        } else {
            state.cooking_timer = 0;
        }
    } else if state.cooking_timer > 0 {
        // Offsets 344-366. Minus two, clamped into `[0, cookingTotalTime]`.
        state.cooking_timer = (state.cooking_timer - 2).clamp(0, state.cooking_total_time);
    }

    if was_lit != is_lit {
        out.changed = true;
        out.lit_changed = Some(is_lit);
    }
    out
}

/// `canBurn(NonNullList, int maxStackSize, ItemStack assembled)` — offsets
/// 0-61.
///
/// An empty result slot accepts anything (offsets 9-17). Otherwise the two
/// stacks must be the same item with the same components, and the **sum** of
/// the counts must be `<= min(maxStackSize, assembled.getMaxStackSize())`.
/// The `if_icmpgt` makes it `<=` and not `<`, so a result slot that ends up
/// exactly full is still a legal burn.
#[must_use]
pub fn can_burn(slots: &[ItemStack], max_stack_size: i32, assembled: &ItemStack) -> bool {
    let result = &slots[2];
    if result.is_empty() {
        return true;
    }
    if !ItemStack::is_same_item_same_components(result, assembled) {
        return false;
    }
    let total = result.count() + assembled.count();
    total <= max_stack_size.min(assembled.max_stack_size())
}

/// `consumeFuel(NonNullList, ItemStack)` — offsets 0-42.
///
/// Shrinks the fuel stack by one and, **only when that empties it**, replaces
/// slot 1 with the item's crafting remainder. That is how a lava bucket
/// leaves an empty bucket behind.
///
/// The remainder is [`fuel_remainder`]: of the five items `Items`'s
/// `static {}` registers with `craftRemainder`, the lava bucket is the only
/// one that burns.
pub fn consume_fuel(slots: &mut [ItemStack]) {
    let item = slots[1].item().to_string();
    slots[1].shrink(1);
    if slots[1].is_empty() {
        slots[1] = fuel_remainder(&item).map_or_else(ItemStack::empty, ItemStack::one);
    }
}

/// `Item.getCraftingRemainder()` for the fuels that have one.
///
/// Read from `Items`'s `static {}`: `Item.Properties.craftRemainder` is
/// called five times — the water, lava and milk buckets (`BUCKET`), the
/// honey bottle and dragon's breath (`GLASS_BOTTLE`) — and of those only
/// `LAVA_BUCKET` is in `FuelValues`. The milk and water buckets are not
/// fuel, so a furnace can never consume them.
#[must_use]
pub fn fuel_remainder(item: &str) -> Option<&'static str> {
    (item == "minecraft:lava_bucket").then_some("minecraft:bucket")
}

/// `burn(NonNullList, ItemStack input, ItemStack assembled)` — offsets 0-99.
///
/// Puts the result in slot 2 (copy if empty, `grow` if not), then the wet
/// sponge special case, then shrinks the input by one. **The input shrink is
/// last**, after the sponge branch has read slot 1.
///
/// The sponge case is transcribed in full because it is the one place a
/// furnace *writes the fuel slot* during a burn: smelting a wet sponge over a
/// bucket replaces that bucket with a water bucket.
pub fn burn_result(slots: &mut [ItemStack], assembled: &ItemStack) {
    if slots[2].is_empty() {
        slots[2] = assembled.clone();
    } else {
        let n = assembled.count();
        slots[2].grow(n);
    }
    if slots[0].item() == "minecraft:wet_sponge"
        && !slots[1].is_empty()
        && slots[1].item() == "minecraft:bucket"
    {
        slots[1] = ItemStack::one("minecraft:water_bucket");
    }
    slots[0].shrink(1);
}

/// `getTotalCookTime(ServerLevel, AbstractFurnaceBlockEntity)` — offsets
/// 0-47.
///
/// The recipe's `cookingTime()`, or **200** when nothing matches — a literal
/// `sipush 200` in the `orElse`, not the per-type codec default. So a smoker
/// holding an unsmokable item carries a total of 200 and not 100, which is
/// invisible until something reads the total while no recipe matches.
#[must_use]
pub fn total_cook_time(recipes: &Recipes, kind: CookingKind, input: &ItemStack) -> i32 {
    recipes
        .find_single(kind.recipe_type(), input)
        .map_or(200, crate::recipes::Recipe::cooking_time)
}

/// `AbstractFurnaceBlockEntity.setItem(int, ItemStack)` — offsets 0-102.
///
/// Recomputing the cook time is gated on **three** things at once: the slot is
/// 0, and the new stack is not "the same item and components as what was
/// already there", and the level is a `ServerLevel`. The middle one is why
/// topping up the ingredient slot with more of the same ore does not restart
/// the progress bar, and why swapping it for a different ore does.
///
/// Returns whether the cook time was recomputed, which is also whether
/// `setChanged()` ran.
pub fn set_item(
    state: &mut FurnaceState,
    slots: &mut [ItemStack],
    slot: usize,
    stack: ItemStack,
    kind: CookingKind,
    recipes: &Recipes,
) -> bool {
    if slot >= slots.len() {
        return false;
    }
    let existing = slots[slot].clone();
    let same = !stack.is_empty() && ItemStack::is_same_item_same_components(&existing, &stack);
    slots[slot] = stack;
    if slot == 0 && !same {
        state.cooking_total_time = total_cook_time(recipes, kind, &slots[0]);
        state.cooking_timer = 0;
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipes::RecipeType;

    fn recipes() -> Recipes {
        Recipes::parse(
            &serde_json::json!({
                "iron_ingot_from_smelting_iron_ore": {
                    "type": "minecraft:smelting",
                    "experience": 0.7,
                    "ingredient": "minecraft:iron_ore",
                    "result": {"id": "minecraft:iron_ingot"}
                },
                "iron_ingot_from_blasting_iron_ore": {
                    "type": "minecraft:blasting",
                    "experience": 0.7,
                    "ingredient": "minecraft:iron_ore",
                    "result": {"id": "minecraft:iron_ingot"}
                },
                "baked_potato_from_campfire_cooking": {
                    "type": "minecraft:campfire_cooking",
                    "cookingtime": 600,
                    "ingredient": "minecraft:potato",
                    "result": {"id": "minecraft:baked_potato"}
                },
                "oak_planks": {
                    "type": "minecraft:crafting_shapeless",
                    "ingredients": ["minecraft:oak_log"],
                    "result": {"id": "minecraft:oak_planks", "count": 4}
                }
            }),
            &serde_json::json!({}),
        )
        .expect("recipes parse")
    }

    fn fuels() -> FuelValues {
        FuelValues::parse(&serde_json::json!({
            "table": [{"item": "minecraft:coal", "burnDuration": 1600}]
        }))
        .expect("fuel table parses")
    }

    fn slots(input: &str, fuel: &str) -> Vec<ItemStack> {
        vec![
            if input.is_empty() {
                ItemStack::empty()
            } else {
                ItemStack::one(input)
            },
            if fuel.is_empty() {
                ItemStack::empty()
            } else {
                ItemStack::one(fuel)
            },
            ItemStack::empty(),
        ]
    }

    /// A recipe document that omits `cookingtime` takes its **type's** codec
    /// default, and the four types do not share one. This is the assert that
    /// separates 200 from 100.
    #[test]
    fn an_omitted_cooking_time_takes_the_types_own_default() {
        let r = recipes();
        assert_eq!(
            r.find_single(RecipeType::Smelting, &ItemStack::one("minecraft:iron_ore"))
                .expect("smelting recipe")
                .cooking_time(),
            200
        );
        assert_eq!(
            r.find_single(RecipeType::Blasting, &ItemStack::one("minecraft:iron_ore"))
                .expect("blasting recipe")
                .cooking_time(),
            100
        );
        // And a document that states the field keeps it, so the defaults are
        // not being applied unconditionally.
        assert_eq!(
            r.find_single(
                RecipeType::CampfireCooking,
                &ItemStack::one("minecraft:potato")
            )
            .expect("campfire recipe")
            .cooking_time(),
            600
        );
    }

    /// The kinds do not share a recipe list. A blast furnace fed a potato
    /// finds nothing even though the campfire list holds one.
    #[test]
    fn a_recipe_belongs_to_one_kind_only() {
        let r = recipes();
        assert!(r
            .find_single(RecipeType::Blasting, &ItemStack::one("minecraft:potato"))
            .is_none());
        assert!(r
            .find_single(
                RecipeType::CampfireCooking,
                &ItemStack::one("minecraft:iron_ore")
            )
            .is_none());
        // Positive control: the crafting recipe in the same document is not
        // in any cooking list, so the parser is filtering on type rather than
        // taking everything.
        assert_eq!(r.len_of(RecipeType::Smelting), 1);
        assert_eq!(r.len_of(RecipeType::Smoking), 0);
    }

    /// One tick of a cold furnace with ore and coal: it lights, consumes the
    /// coal, records `lit` going true, and both lit fields take the coal's
    /// 1600.
    #[test]
    fn a_cold_furnace_with_ore_and_coal_lights_on_the_first_tick() {
        let mut st = FurnaceState {
            cooking_total_time: 200,
            ..FurnaceState::default()
        };
        let mut s = slots("minecraft:iron_ore", "minecraft:coal");
        let out = server_tick(&mut st, &mut s, CookingKind::Smelting, &recipes(), &fuels());
        assert_eq!(out.lit_changed, Some(true));
        assert!(out.changed);
        assert_eq!(st.lit_time_remaining, 1600);
        assert_eq!(st.lit_total_time, 1600);
        assert_eq!(
            st.cooking_timer, 1,
            "the cook advances on the lighting tick"
        );
        assert!(s[1].is_empty(), "the coal was consumed");
    }

    /// The negative control for the test above: the same furnace with no fuel
    /// lights nothing, cooks nothing, and reports no state change.
    #[test]
    fn a_furnace_with_no_fuel_does_nothing_at_all() {
        let mut st = FurnaceState {
            cooking_total_time: 200,
            ..FurnaceState::default()
        };
        let mut s = slots("minecraft:iron_ore", "");
        let out = server_tick(&mut st, &mut s, CookingKind::Smelting, &recipes(), &fuels());
        assert_eq!(out.lit_changed, None);
        assert!(!out.changed);
        assert_eq!(st.lit_time_remaining, 0);
        assert_eq!(st.cooking_timer, 0);
    }

    /// The completion tick: the timer reaches the total exactly, the result
    /// appears, the input shrinks and the timer resets to zero.
    #[test]
    fn the_cook_completes_when_the_timer_equals_the_total() {
        let mut st = FurnaceState {
            lit_time_remaining: 100,
            lit_total_time: 1600,
            cooking_timer: 199,
            cooking_total_time: 200,
        };
        let mut s = slots("minecraft:iron_ore", "");
        s[0] = ItemStack::new("minecraft:iron_ore", 3);
        let out = server_tick(&mut st, &mut s, CookingKind::Smelting, &recipes(), &fuels());
        assert_eq!(
            out.recipe_used.as_deref(),
            Some("iron_ingot_from_smelting_iron_ore")
        );
        assert_eq!(st.cooking_timer, 0);
        assert_eq!(s[2].item(), "minecraft:iron_ingot");
        assert_eq!(s[2].count(), 1);
        assert_eq!(s[0].count(), 2, "one ore consumed");
        assert_eq!(st.lit_time_remaining, 99);
        assert_eq!(out.lit_changed, None, "still lit, so no block update");
    }

    /// The progress **decays** by two a tick when the furnace can neither
    /// stay lit nor light, and stops at zero rather than going negative.
    #[test]
    fn an_unlit_furnace_loses_two_ticks_of_progress_and_clamps_at_zero() {
        let mut st = FurnaceState {
            cooking_timer: 3,
            cooking_total_time: 200,
            ..FurnaceState::default()
        };
        let mut s = slots("minecraft:iron_ore", "");
        for want in [1, 0, 0] {
            server_tick(&mut st, &mut s, CookingKind::Smelting, &recipes(), &fuels());
            assert_eq!(st.cooking_timer, want);
        }
    }

    /// The branch with **no** write: input present, lit, and no recipe for it.
    /// Vanilla's `ifnull` jumps past every `cookingTimer` store, so a stale
    /// progress value survives the tick.
    #[test]
    fn an_input_with_no_recipe_leaves_the_timer_untouched() {
        let mut st = FurnaceState {
            lit_time_remaining: 50,
            lit_total_time: 1600,
            cooking_timer: 42,
            cooking_total_time: 200,
        };
        let mut s = slots("minecraft:diamond", "");
        server_tick(&mut st, &mut s, CookingKind::Smelting, &recipes(), &fuels());
        assert_eq!(st.cooking_timer, 42, "no recipe writes no timer");
        // Positive control: an **empty** input on the same lit furnace does
        // reach a write, and zeroes it. If both branches behaved alike this
        // pair could not both hold.
        st.cooking_timer = 42;
        let mut empty = slots("", "");
        server_tick(
            &mut st,
            &mut empty,
            CookingKind::Smelting,
            &recipes(),
            &fuels(),
        );
        assert_eq!(st.cooking_timer, 0);
    }

    /// Fuel runs out on the tick the last lit tick is spent, and the block
    /// update goes out on that same tick.
    #[test]
    fn the_lit_state_falls_on_the_tick_the_fuel_runs_out() {
        let mut st = FurnaceState {
            lit_time_remaining: 1,
            lit_total_time: 1600,
            cooking_timer: 10,
            cooking_total_time: 200,
        };
        let mut s = slots("minecraft:iron_ore", "");
        let out = server_tick(&mut st, &mut s, CookingKind::Smelting, &recipes(), &fuels());
        assert_eq!(out.lit_changed, Some(false));
        assert!(out.changed);
        assert_eq!(st.lit_time_remaining, 0);
        // AND THE PROGRESS DECAYS ON THIS TICK RATHER THAN BEING ZEROED.
        // This assert read `0` when it was first written, from the reasonable
        // guess that going unlit takes the `else` at offset 320. It does not:
        // `isLit` is sampled **after** the decrement, so by the time the test
        // at offset 102 runs it is already false, `hasFuel` is false too, and
        // the whole `if` is skipped for the decay branch at 344. 10 - 2 = 8.
        // The offset-320 zeroing needs a furnace that is unlit *and* could
        // have lit — fuel and a recipe present, burn duration 0.
        assert_eq!(st.cooking_timer, 8);
    }

    /// `canBurn` is `<=`, so a result slot that ends exactly full still burns.
    #[test]
    fn a_burn_that_exactly_fills_the_result_slot_is_allowed() {
        let mut s = slots("minecraft:iron_ore", "");
        s[2] = ItemStack::new("minecraft:iron_ingot", 63);
        let one = ItemStack::one("minecraft:iron_ingot");
        assert!(can_burn(&s, CONTAINER_MAX_STACK_SIZE, &one));
        s[2] = ItemStack::new("minecraft:iron_ingot", 64);
        assert!(!can_burn(&s, CONTAINER_MAX_STACK_SIZE, &one));
        // A different item in the result slot refuses whatever the counts.
        s[2] = ItemStack::one("minecraft:gold_ingot");
        assert!(!can_burn(&s, CONTAINER_MAX_STACK_SIZE, &one));
    }

    /// The wet sponge writes the fuel slot, which nothing else in a burn
    /// does, and it reads that slot **before** the input is shrunk.
    #[test]
    fn smelting_a_wet_sponge_over_a_bucket_fills_the_bucket() {
        let mut s = vec![
            ItemStack::one("minecraft:wet_sponge"),
            ItemStack::one("minecraft:bucket"),
            ItemStack::empty(),
        ];
        burn_result(&mut s, &ItemStack::one("minecraft:sponge"));
        assert_eq!(s[1].item(), "minecraft:water_bucket");
        assert_eq!(s[2].item(), "minecraft:sponge");
        assert!(s[0].is_empty());
        // Negative control: a bucket under anything else is left alone.
        let mut other = vec![
            ItemStack::one("minecraft:iron_ore"),
            ItemStack::one("minecraft:bucket"),
            ItemStack::empty(),
        ];
        burn_result(&mut other, &ItemStack::one("minecraft:iron_ingot"));
        assert_eq!(other[1].item(), "minecraft:bucket");
    }

    /// `getTotalCookTime`'s fallback is a literal 200 and **not** the type's
    /// codec default, so a smoker with an unsmokable item carries 200.
    #[test]
    fn the_total_cook_time_falls_back_to_two_hundred_for_every_kind() {
        let r = recipes();
        let junk = ItemStack::one("minecraft:diamond");
        assert_eq!(total_cook_time(&r, CookingKind::Smoking, &junk), 200);
        assert_eq!(total_cook_time(&r, CookingKind::Blasting, &junk), 200);
        // And a real blasting recipe still gives 100, so the fallback is a
        // fallback rather than the only answer.
        assert_eq!(
            total_cook_time(
                &r,
                CookingKind::Blasting,
                &ItemStack::one("minecraft:iron_ore")
            ),
            100
        );
    }

    /// Topping the ingredient slot up with more of the same item does not
    /// reset the progress; a different item does.
    #[test]
    fn setting_the_same_item_again_keeps_the_progress() {
        let r = recipes();
        let mut st = FurnaceState {
            cooking_timer: 90,
            cooking_total_time: 200,
            ..FurnaceState::default()
        };
        let mut s = slots("minecraft:iron_ore", "");
        assert!(!set_item(
            &mut st,
            &mut s,
            0,
            ItemStack::new("minecraft:iron_ore", 5),
            CookingKind::Smelting,
            &r
        ));
        assert_eq!(st.cooking_timer, 90);
        assert!(set_item(
            &mut st,
            &mut s,
            0,
            ItemStack::one("minecraft:diamond"),
            CookingKind::Smelting,
            &r
        ));
        assert_eq!(st.cooking_timer, 0);
        assert_eq!(st.cooking_total_time, 200);
    }
}
