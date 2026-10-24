//! `AnvilMenu.createResult` — the cost model, the enchantment combination,
//! the two repairs, the rename and the 40-level refusal.
//!
//! Transcribed from `AnvilMenu.createResult` (offsets 0..1077) in the pinned
//! `server-26.2.jar`, `javap -p -c -constants` under JDK 25, plus
//! `calculateIncreasedRepairCost`, `EnchantmentHelper.getEnchantmentsForCrafting`
//! and `Enchantment.areCompatible`.
//!
//! # This is the arithmetic, not the menu
//!
//! `crate::menu` opens three menus and `minecraft:anvil` is not one of them;
//! `Player.openMenu` and `AbstractContainerMenu.broadcastChanges` belong to
//! the container-menu lane. So [`create_result`] is a pure function over a
//! description of the two input slots, and nothing in this server calls it
//! yet. It is written now because the cost model is the part that is hard to
//! get right and easy to check, and because a later menu lane should find it
//! already derived rather than re-derive it from the same bytecode.
//!
//! It also does not use [`crate::item::ItemStack`], which carries an item and
//! a count and no components. Damage, repair cost, enchantments and a custom
//! name are all components, and adding them is the stacks lane's change, not
//! this one's. [`Input`] is this module's own description and says so.
//!
//! # The cost model, with the constants named as the class names them
//!
//! ```text
//!   COST_BASE                1     cost starts here, before anything is added
//!   COST_REPAIR_MATERIAL     1     per material unit consumed
//!   COST_REPAIR_SACRIFICE    2     for a same-item combine that heals
//!   COST_INCOMPATIBLE_PENALTY 1    per incompatible pair encountered
//!   COST_RENAME              1
//!   COST_FAIL                0
//! ```
//!
//! and the published cost is
//!
//! ```text
//!   846  i2 = (i <= 0) ? 0 : (int) Mth.clamp(j + i, 0, Integer.MAX_VALUE)
//! ```
//!
//! where `j` is the **sum of both inputs' `REPAIR_COST`** — added as a `long`,
//! clamped as a `long`, and only then narrowed. Two items each carrying a
//! prior-work penalty near `Integer.MAX_VALUE` overflow an `int` sum, and the
//! clamp is what stops that being negative; doing the addition in `i32` gives
//! a *cheap* anvil at the top of the range, which is the wrong direction.
//!
//! # `prior_work_count` and where `2^n - 1` comes from
//!
//! ```text
//!   public static int calculateIncreasedRepairCost(int);
//!      0: iload_0; i2l; ldc2_w 2l; lmul; lconst_1; ladd
//!      8: ldc2_w 2147483647l; Math.min(JJ); l2i; ireturn
//! ```
//!
//! One step is `r -> 2r + 1`, saturating at `Integer.MAX_VALUE`. A fresh item
//! has `REPAIR_COST` absent, which `getOrDefault` reads as 0, so after `n`
//! anvil operations the penalty is `2^n - 1`: 0, 1, 3, 7, 15, 31, 63… The
//! familiar "too expensive after six uses" is that sequence meeting the
//! 40-level refusal, not a counter compared against 6.
//!
//! **It is not applied on a pure rename.** Offsets 1021..1039:
//! `if (k != i) increase; else if (k != 0) skip; else increase` — so the step
//! is skipped exactly when the rename cost is the *whole* cost and is
//! non-zero. Renaming a tool forever therefore never makes it too expensive,
//! which is a deliberate vanilla behaviour and reads as a bug in any port
//! that applies the doubling unconditionally.
//!
//! # The 40-level refusal, and the one thing it is not
//!
//! ```text
//!   886  if (k == i && k > 0 && cost >= 40) { cost = 39; onlyRenaming = true }
//!   923  if (cost >= 40 && !player.hasInfiniteMaterials()) out = EMPTY
//! ```
//!
//! `>= 40`, not `> 40`. And the rename escape runs **first**, so a rename that
//! would have cost 40 or more is re-priced at 39 and allowed. Creative mode
//! bypasses the refusal but not the re-pricing.

use crate::enchantment::Registry;

/// The prior-work penalty step, `calculateIncreasedRepairCost`.
#[must_use]
pub fn increased_repair_cost(cost: i32) -> i32 {
    let doubled = i64::from(cost) * 2 + 1;
    i32::try_from(doubled.min(i64::from(i32::MAX))).unwrap_or(i32::MAX)
}

/// The prior-work penalty after `n` anvil operations on a fresh item: the
/// closed form of [`increased_repair_cost`] iterated from 0.
///
/// Saturating, and the saturation is the point: `2^31 - 1` is reached at
/// n = 31 and every later operation leaves it there, matching the `Math.min`
/// in the bytecode rather than overflowing.
#[must_use]
pub fn prior_work_penalty(n: u32) -> i32 {
    let mut r = 0;
    for _ in 0..n {
        r = increased_repair_cost(r);
    }
    r
}

/// One input slot, described in the terms `createResult` reads it in.
///
/// Not [`crate::item::ItemStack`] — see the module header. Every field maps to
/// one component or accessor:
///
/// | field | vanilla |
/// |---|---|
/// | `item` | `ItemStack.getItem()` |
/// | `count` | `ItemStack.getCount()` |
/// | `damage` | `ItemStack.getDamageValue()` |
/// | `repair_cost` | `getOrDefault(REPAIR_COST, 0)` |
/// | `enchantments` | `ENCHANTMENTS`, or `STORED_ENCHANTMENTS` on an enchanted book |
/// | `custom_name` | `CUSTOM_NAME` |
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Input {
    pub item: String,
    pub count: i32,
    pub damage: i32,
    pub repair_cost: i32,
    /// `ItemEnchantments.entrySet()` order. Vanilla's is an
    /// `Object2IntOpenHashMap` wrapped by `ItemEnchantments`, and the anvil
    /// only ever *sums* over it, so order changes no published number — but it
    /// is a `Vec` rather than a map so that a future caller that does care
    /// gets to choose.
    pub enchantments: Vec<(String, i32)>,
    pub custom_name: Option<String>,
}

impl Input {
    /// An empty slot — `ItemStack.EMPTY`.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// `ItemStack.isEmpty()`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.item.is_empty() || self.count <= 0
    }

    /// `ItemStack.is(Items.ENCHANTED_BOOK)`.
    #[must_use]
    pub fn is_enchanted_book(&self) -> bool {
        self.item == "minecraft:enchanted_book"
    }

    /// `ItemStack.has(DataComponents.STORED_ENCHANTMENTS)`.
    ///
    /// Only an enchanted book carries that component, and
    /// `getEnchantmentsForCrafting` reads it in place of `ENCHANTMENTS` for
    /// exactly that item — `getComponentType` is one `is(ENCHANTED_BOOK)`
    /// test. So the two questions collapse into one here and the flag the
    /// bytecode calls `bl` is this.
    #[must_use]
    pub fn has_stored_enchantments(&self) -> bool {
        self.is_enchanted_book()
    }

    /// `EnchantmentHelper.canStoreEnchantments(stack)` — `stack.has(type)`.
    ///
    /// `ENCHANTMENTS` is in `DataComponents.COMMON_ITEM_COMPONENTS` (read out
    /// of `DataComponents.<clinit>`, between `LORE` and `REPAIR_COST`), so
    /// **every** item has it and this is true for everything except an
    /// enchanted book with no stored enchantments. It is written out rather
    /// than folded into `true` because the guard it stands in is the one that
    /// makes the anvil produce nothing at all, and a reader deserves to see
    /// why it almost never fires.
    #[must_use]
    pub fn can_store_enchantments(&self) -> bool {
        !self.is_enchanted_book() || !self.enchantments.is_empty()
    }

    /// `ItemStack.isDamageableItem()` — has `MAX_DAMAGE` and is not stacked.
    #[must_use]
    pub fn is_damageable(&self, registry: &Registry) -> bool {
        registry.max_damage(&self.item).is_some()
    }

    /// `ItemStack.getMaxDamage()`.
    #[must_use]
    pub fn max_damage(&self, registry: &Registry) -> i32 {
        registry.max_damage(&self.item).unwrap_or(0)
    }
}

/// What `createResult` leaves behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// `resultSlots.setItem(0, EMPTY)` and `cost.set(0)`. Five distinct
    /// branches reach this and they are not the same refusal — see
    /// [`Refusal`].
    None(Refusal),
    /// A result and the level cost the client is shown.
    Result(Result_),
}

/// Which of `createResult`'s refusals fired.
///
/// Kept apart because they mean different things to a caller and because a
/// test that could only see "no result" would pass while the wrong branch
/// fired. Every one of them sets the same two fields on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Offset 45: the left slot is empty, or cannot store enchantments.
    NoInput,
    /// Offset 200: a material repair whose input is already undamaged —
    /// `Math.min(damage, maxDamage / 4) <= 0`.
    NothingToRepair,
    /// Offset 316: the right slot is not a book, and is neither the same item
    /// nor is the left damageable.
    Incompatible,
    /// Offset 736: every enchantment on the right was refused and none was
    /// accepted (`hadIncompatible && !hadCompatible`).
    AllEnchantmentsRefused,
    /// Offset 877: the total cost came out at zero or less, so there is
    /// nothing to do.
    NoWorkToDo,
    /// Offset 923: `cost >= 40` and the player is not in creative.
    TooExpensive,
}

/// A produced result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Result_ {
    /// `this.cost.get()` — the number in the level bubble.
    pub cost: i32,
    /// `this.repairItemCountCost` — how many of the right stack are consumed
    /// by a material repair. Zero for every other path.
    pub repair_item_count_cost: i32,
    /// `this.onlyRenaming`, set when the rename escape re-priced a 40+ rename
    /// to 39.
    pub only_renaming: bool,
    /// The output stack.
    pub output: Input,
}

/// `AnvilMenu.createResult()`.
///
/// `item_name` is the menu's `itemName` field — `None` when the player has
/// typed nothing, `Some("")` when they cleared the box (which `isBlank`
/// treats the same way).
///
/// `creative` is `player.hasInfiniteMaterials()`. It does two things and they
/// are not the same: it makes every enchantment on the right applicable
/// (offset 554) and it bypasses the 40-level refusal (offset 935).
#[allow(clippy::too_many_lines)]
#[must_use]
pub fn create_result(
    registry: &Registry,
    left: &Input,
    right: &Input,
    item_name: Option<&str>,
    creative: bool,
) -> Outcome {
    if left.is_empty() || !left.can_store_enchantments() {
        return Outcome::None(Refusal::NoInput);
    }

    // `out = left.copy()`, and the merged enchantment map is seeded from the
    // COPY rather than from `left` — identical today, and it is the copy that
    // is written back at offset 1053.
    let mut out = left.clone();
    let mut merged: Vec<(String, i32)> = left.enchantments.clone();

    // `j`, offsets 97..137. A long, and see the module header for why.
    let base_repair_cost = i64::from(left.repair_cost) + i64::from(right.repair_cost);
    let mut cost = 0i32; // `i` — everything added to COST_BASE
    let mut repair_item_count_cost = 0;

    if !right.is_empty() {
        let right_is_book = right.has_stored_enchantments();

        if out.is_damageable(registry) && registry.is_valid_repair_item(&left.item, &right.item) {
            // --- material repair, offsets 178..287 ---------------------------
            //
            // Each unit heals `min(damage, maxDamage / 4)` and costs 1, and
            // the loop stops on the FIRST of "fully repaired" and "ran out of
            // material". `repairItemCountCost` is the units consumed, which is
            // what `onTake` shrinks the right stack by.
            let max = out.max_damage(registry);
            let mut heal = out.damage.min(max / 4);
            if heal <= 0 {
                return Outcome::None(Refusal::NothingToRepair);
            }
            let mut used = 0;
            while heal > 0 && used < right.count {
                out.damage -= heal;
                cost += 1;
                heal = out.damage.min(max / 4);
                used += 1;
            }
            repair_item_count_cost = used;
        } else {
            if !right_is_book && (out.item != right.item || !out.is_damageable(registry)) {
                return Outcome::None(Refusal::Incompatible);
            }

            if out.is_damageable(registry) && !right_is_book {
                // --- combine repair, offsets 349..433 -----------------------
                //
                // `left_left + right_left + 12% of max`, capped at max. The
                // bonus is `maxDamage * 12 / 100` — integer division of the
                // product, so it is not `0.12 * max` and rounds differently.
                let left_max = registry.max_damage(&left.item).unwrap_or(0);
                let right_max = registry.max_damage(&right.item).unwrap_or(0);
                let out_max = out.max_damage(registry);
                let left_durability = left_max - left.damage;
                let right_durability = right_max - right.damage;
                let contributed = right_durability + out_max * 12 / 100;
                let total = left_durability + contributed;
                let new_damage = (out_max - total).max(0);
                if new_damage < out.damage {
                    out.damage = new_damage;
                    cost += 2;
                }
            }

            // --- enchantment merge, offsets 434..726 ------------------------
            let mut had_compatible = false;
            let mut had_incompatible = false;
            for (id, right_level) in &right.enchantments {
                let existing = level_in(&merged, id);
                // 511: equal levels combine to one higher; otherwise the
                // higher of the two wins. `max` alone loses the whole
                // "two Sharpness III make a IV" rule.
                let mut level = if existing == *right_level {
                    right_level + 1
                } else {
                    (*right_level).max(existing)
                };

                // `Enchantment.canEnchant(stack)` is `supportedItems.contains`
                // — the SUPPORTED set, not the primary one, so the anvil
                // applies enchantments an enchanting table never offers on
                // that item.
                let mut applicable = registry
                    .get(id)
                    .is_some_and(|e| e.is_supported_item(&left.item));
                if creative || left.is_enchanted_book() {
                    applicable = true;
                }

                // The penalty is per incompatible PAIR and it is charged even
                // though the enchantment is then refused — offsets 611..636.
                for (held, _) in &merged {
                    if held != id && !registry.are_compatible(id, held) {
                        applicable = false;
                        cost += 1;
                    }
                }

                if !applicable {
                    had_incompatible = true;
                    continue;
                }
                had_compatible = true;
                if let Some(e) = registry.get(id) {
                    level = level.min(e.max_level);
                }
                set_level(&mut merged, id, level);

                let mut anvil_cost = registry.get(id).map_or(0, |e| e.anvil_cost);
                if right_is_book {
                    anvil_cost = (anvil_cost / 2).max(1);
                }
                cost += anvil_cost * level;
                // 712: a stacked left input costs a flat 40 — an assignment,
                // not an addition, so it also discards everything charged so
                // far and is then refused by the 40-level gate below.
                if left.count > 1 {
                    cost = 40;
                }
            }
            if had_incompatible && !had_compatible {
                return Outcome::None(Refusal::AllEnchantmentsRefused);
            }
        }
    }

    // --- rename, offsets 756..846 -------------------------------------------
    let mut rename_cost = 0;
    match item_name {
        None => {
            if left.custom_name.is_some() {
                rename_cost = 1;
                cost += rename_cost;
                out.custom_name = None;
            }
        }
        Some(name) if name.trim().is_empty() => {
            // `StringUtil.isBlank` — the same arm as a null name.
            if left.custom_name.is_some() {
                rename_cost = 1;
                cost += rename_cost;
                out.custom_name = None;
            }
        }
        Some(name) => {
            // `getHoverName().getString()` is the custom name when there is
            // one and the item's own display name otherwise; only the former
            // is modelled, so a rename to the item's vanilla name is charged
            // here and is free in vanilla. Named rather than guessed: the
            // display name needs the language file, which this server has no
            // reader for.
            if left.custom_name.as_deref() != Some(name) {
                rename_cost = 1;
                cost += rename_cost;
                out.custom_name = Some(name.to_string());
            }
        }
    }

    // --- publish, offsets 846..1077 ----------------------------------------
    //
    // Vanilla does not return here: at 877 it empties the output and carries
    // on through the rename escape and the 40 gate. Both are unreachable with
    // `i <= 0` — the escape needs `k > 0` and `k == i` — so the observable
    // state is the same pair of fields either way, and returning says which
    // branch produced them.
    if cost <= 0 {
        return Outcome::None(Refusal::NoWorkToDo);
    }
    let mut published =
        i32::try_from((base_repair_cost + i64::from(cost)).clamp(0, i64::from(i32::MAX)))
            .unwrap_or(i32::MAX);
    let mut only_renaming = false;
    if rename_cost == cost && rename_cost > 0 && published >= 40 {
        published = 39;
        only_renaming = true;
    }
    if published >= 40 && !creative {
        return Outcome::None(Refusal::TooExpensive);
    }

    let mut new_repair_cost = out.repair_cost.max(right.repair_cost);
    if rename_cost != cost || rename_cost == 0 {
        new_repair_cost = increased_repair_cost(new_repair_cost);
    }
    out.repair_cost = new_repair_cost;
    out.enchantments = merged;

    Outcome::Result(Result_ {
        cost: published,
        repair_item_count_cost,
        only_renaming,
        output: out,
    })
}

fn level_in(list: &[(String, i32)], id: &str) -> i32 {
    list.iter().find(|(k, _)| k == id).map_or(0, |(_, v)| *v)
}

fn set_level(list: &mut Vec<(String, i32)>, id: &str, level: i32) {
    if let Some(e) = list.iter_mut().find(|(k, _)| k == id) {
        e.1 = level;
    } else {
        list.push((id.to_string(), level));
    }
}
