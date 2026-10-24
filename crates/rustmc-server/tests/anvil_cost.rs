//! `AnvilMenu.createResult` — one bucket per branch, and a control per bucket.
//!
//! # Why the refusals are tested by name
//!
//! Six distinct branches produce "no result, cost 0" and they mean six
//! different things. A test that only checked "the anvil produced nothing"
//! would pass while the wrong one fired — for instance, an incompatible-item
//! refusal standing in for a too-expensive one, which is a port that has
//! silently lost the entire enchantment merge. `Refusal` names them and every
//! assertion here names the one it expects.
//!
//! # These are arithmetic goldens, not parity measurements
//!
//! Nothing here has been compared against a vanilla anvil. `crate::menu`
//! carries no `minecraft:anvil` arm, so no scenario in the gameplay corpus
//! can open one, and the numbers below are the bytecode's arithmetic applied
//! to inputs described by `data/vanilla/enchantments.json`. What they catch is
//! a change to that arithmetic. Read them as that.

use rustmc_server::anvil::{
    create_result, increased_repair_cost, prior_work_penalty, Input, Outcome, Refusal,
};
use rustmc_server::enchantment::{Registry, TABLE_PATH};

fn repo(rel: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

fn registry() -> Option<Registry> {
    let p = repo(TABLE_PATH);
    if !rustmc_data::fixture::available(
        &p,
        "data/vanilla/enchantments.json",
        "ci/enchantments-golden.sh",
    ) {
        return None;
    }
    Some(Registry::load(&p).expect("enchantments.json"))
}

fn stack(item: &str) -> Input {
    Input {
        item: item.to_string(),
        count: 1,
        ..Input::default()
    }
}

fn book(enchantments: &[(&str, i32)]) -> Input {
    Input {
        item: "minecraft:enchanted_book".to_string(),
        count: 1,
        enchantments: enchantments
            .iter()
            .map(|(k, v)| ((*k).to_string(), *v))
            .collect(),
        ..Input::default()
    }
}

fn result(o: &Outcome) -> &rustmc_server::anvil::Result_ {
    match o {
        Outcome::Result(r) => r,
        Outcome::None(refusal) => panic!("expected a result, got {refusal:?}"),
    }
}

// ---------------------------------------------------------------------------
// calculateIncreasedRepairCost and the 2^n - 1 sequence
// ---------------------------------------------------------------------------

/// `r -> 2r + 1`, saturating at `Integer.MAX_VALUE` because the multiply is
/// done as a `long` and passed through `Math.min`.
#[test]
fn the_repair_cost_step_is_two_r_plus_one_and_saturates() {
    assert_eq!(increased_repair_cost(0), 1);
    assert_eq!(increased_repair_cost(1), 3);
    assert_eq!(increased_repair_cost(3), 7);
    assert_eq!(increased_repair_cost(i32::MAX), i32::MAX);
    // The control: an i32 multiply here would wrap to -1 rather than saturate,
    // and a negative prior-work penalty makes the anvil CHEAPER at the top of
    // the range. `2 * MAX + 1` as an i32 is exactly that.
    assert_eq!(i32::MAX.wrapping_mul(2).wrapping_add(1), -1);
}

/// The closed form, which is where "too expensive after six uses" comes from:
/// it is this sequence meeting the 40-level gate, not a counter tested
/// against 6.
#[test]
fn the_prior_work_penalty_is_two_to_the_n_minus_one() {
    let want = [0, 1, 3, 7, 15, 31, 63, 127];
    for (n, w) in want.iter().enumerate() {
        assert_eq!(
            prior_work_penalty(u32::try_from(n).expect("small")),
            *w,
            "after {n} operations"
        );
    }
    assert_eq!(prior_work_penalty(31), i32::MAX);
    assert_eq!(prior_work_penalty(60), i32::MAX, "and it stays there");
}

// ---------------------------------------------------------------------------
// The refusals, each by name
// ---------------------------------------------------------------------------

#[test]
fn an_empty_left_slot_refuses_with_no_input() {
    let Some(reg) = registry() else { return };
    let got = create_result(
        &reg,
        &Input::empty(),
        &stack("minecraft:diamond"),
        None,
        false,
    );
    assert_eq!(got, Outcome::None(Refusal::NoInput));
}

#[test]
fn an_enchanted_book_with_nothing_stored_cannot_store_enchantments() {
    let Some(reg) = registry() else { return };
    let empty_book = book(&[]);
    assert!(!empty_book.can_store_enchantments());
    let got = create_result(&reg, &empty_book, &Input::empty(), Some("x"), false);
    assert_eq!(got, Outcome::None(Refusal::NoInput));
    // The control: the same book WITH a stored enchantment gets past the gate.
    let real_book = book(&[("minecraft:sharpness", 1)]);
    assert!(real_book.can_store_enchantments());
    assert!(matches!(
        create_result(&reg, &real_book, &Input::empty(), Some("x"), false),
        Outcome::Result(_)
    ));
}

#[test]
fn an_undamaged_tool_over_its_repair_material_refuses_with_nothing_to_repair() {
    let Some(reg) = registry() else { return };
    let got = create_result(
        &reg,
        &stack("minecraft:diamond_sword"),
        &stack("minecraft:diamond"),
        None,
        false,
    );
    assert_eq!(got, Outcome::None(Refusal::NothingToRepair));
}

#[test]
fn two_unrelated_items_refuse_with_incompatible() {
    let Some(reg) = registry() else { return };
    let got = create_result(
        &reg,
        &stack("minecraft:diamond_sword"),
        &stack("minecraft:oak_planks"),
        None,
        false,
    );
    assert_eq!(got, Outcome::None(Refusal::Incompatible));
}

/// Every enchantment on the book refused and none accepted. Aqua affinity is
/// a helmet enchantment; a sword's supported set does not hold it.
#[test]
fn a_book_of_wholly_inapplicable_enchantments_refuses_by_name() {
    let Some(reg) = registry() else { return };
    let got = create_result(
        &reg,
        &stack("minecraft:diamond_sword"),
        &book(&[("minecraft:aqua_affinity", 1)]),
        None,
        false,
    );
    assert_eq!(got, Outcome::None(Refusal::AllEnchantmentsRefused));
    // The control: the SAME book on a helmet is accepted, so the refusal is
    // about applicability and not about books.
    assert!(matches!(
        create_result(
            &reg,
            &stack("minecraft:diamond_helmet"),
            &book(&[("minecraft:aqua_affinity", 1)]),
            None,
            false,
        ),
        Outcome::Result(_)
    ));
    // And in creative every enchantment is applicable (offset 554), so the
    // same call produces a result rather than a refusal.
    assert!(matches!(
        create_result(
            &reg,
            &stack("minecraft:diamond_sword"),
            &book(&[("minecraft:aqua_affinity", 1)]),
            None,
            true,
        ),
        Outcome::Result(_)
    ));
}

#[test]
fn a_left_item_alone_with_no_rename_has_no_work_to_do() {
    let Some(reg) = registry() else { return };
    let got = create_result(
        &reg,
        &stack("minecraft:diamond_sword"),
        &Input::empty(),
        None,
        false,
    );
    assert_eq!(got, Outcome::None(Refusal::NoWorkToDo));
}

/// `cost >= 40`, not `> 40`. Both sides of the boundary are asserted, because
/// an off-by-one here is the difference between a usable anvil and one that
/// refuses the last affordable combine.
#[test]
fn the_refusal_boundary_is_at_exactly_forty() {
    let Some(reg) = registry() else { return };
    // A prior-work penalty on the left carries straight into the published
    // cost, so it is the cheapest way to land on an exact total. A rename
    // costs 1, so a penalty of 38 gives 39 and 39 gives 40.
    let mut left = stack("minecraft:diamond_sword");
    left.repair_cost = 38;
    let ok = create_result(&reg, &left, &Input::empty(), Some("nine"), false);
    assert_eq!(result(&ok).cost, 39);

    left.repair_cost = 39;
    let refused = create_result(&reg, &left, &Input::empty(), Some("nine"), false);
    // A pure rename escapes the gate: see the next test. Force a non-rename
    // cost by adding a compatible book so `rename_cost != cost`.
    assert!(
        matches!(refused, Outcome::Result(_)),
        "a pure rename escapes"
    );

    let mut left2 = stack("minecraft:diamond_sword");
    left2.repair_cost = 39;
    let got = create_result(
        &reg,
        &left2,
        &book(&[("minecraft:sharpness", 1)]),
        None,
        false,
    );
    assert_eq!(got, Outcome::None(Refusal::TooExpensive));
    // Creative bypasses it.
    assert!(matches!(
        create_result(
            &reg,
            &left2,
            &book(&[("minecraft:sharpness", 1)]),
            None,
            true
        ),
        Outcome::Result(_)
    ));
}

/// The rename escape: a cost that is ENTIRELY rename is re-priced to 39 and
/// allowed, and `only_renaming` is set. This is why renaming an item forever
/// never makes it too expensive.
#[test]
fn a_pure_rename_is_repriced_to_thirty_nine_and_never_doubles() {
    let Some(reg) = registry() else { return };
    let mut left = stack("minecraft:diamond_sword");
    left.repair_cost = 1000;
    let got = create_result(&reg, &left, &Input::empty(), Some("still cheap"), false);
    let r = result(&got);
    assert_eq!(r.cost, 39);
    assert!(r.only_renaming);
    // The prior-work penalty is NOT increased — offsets 1021..1039.
    assert_eq!(r.output.repair_cost, 1000);
    assert_eq!(r.output.custom_name.as_deref(), Some("still cheap"));

    // The control: add any non-rename cost and both halves flip. A book takes
    // `rename_cost != cost`, so the doubling applies and the gate refuses.
    let got = create_result(
        &reg,
        &left,
        &book(&[("minecraft:sharpness", 1)]),
        Some("expensive now"),
        false,
    );
    assert_eq!(got, Outcome::None(Refusal::TooExpensive));
}

/// Clearing the name removes `CUSTOM_NAME` and still costs 1.
#[test]
fn clearing_a_name_costs_one_and_removes_the_component() {
    let Some(reg) = registry() else { return };
    let mut left = stack("minecraft:diamond_sword");
    left.custom_name = Some("Old Name".to_string());
    for typed in [None, Some(""), Some("   ")] {
        let got = create_result(&reg, &left, &Input::empty(), typed, false);
        let r = result(&got);
        assert_eq!(r.cost, 1, "typed {typed:?}");
        assert_eq!(r.output.custom_name, None);
        // A rename is the whole cost, so the penalty does not double.
        assert_eq!(r.output.repair_cost, 0);
    }
}

// ---------------------------------------------------------------------------
// The two repairs
// ---------------------------------------------------------------------------

/// Each material unit heals `min(damage, maxDamage / 4)` and costs 1, and the
/// loop stops at whichever of "repaired" and "out of material" comes first.
#[test]
fn a_material_repair_charges_one_per_unit_and_reports_the_count() {
    let Some(reg) = registry() else { return };
    let max = reg
        .max_damage("minecraft:diamond_sword")
        .expect("damageable");
    assert_eq!(max, 1561);

    // Fully damaged: 1561 needs ceil(1561 / 390) = 5 diamonds, and the last
    // one heals the remainder rather than a quarter.
    let mut left = stack("minecraft:diamond_sword");
    left.damage = max;
    let mut material = stack("minecraft:diamond");
    material.count = 64;
    let got = create_result(&reg, &left, &material, None, false);
    let r = result(&got);
    assert_eq!(r.repair_item_count_cost, 5);
    assert_eq!(r.output.damage, 0);
    // COST_BASE is not part of `i`; the published cost is the 5 units plus
    // both prior-work penalties, which are 0 here.
    assert_eq!(r.cost, 5);
    // Prior work DOES double on a repair.
    assert_eq!(r.output.repair_cost, 1);

    // Out of material first: one diamond heals one quarter and no more.
    let mut one = stack("minecraft:diamond");
    one.count = 1;
    let got = create_result(&reg, &left, &one, None, false);
    let r = result(&got);
    assert_eq!(r.repair_item_count_cost, 1);
    assert_eq!(r.output.damage, max - max / 4);
    assert_eq!(r.cost, 1);
}

/// Combining two of the same tool: `left_left + right_left + 12% of max`,
/// capped, and it costs COST_REPAIR_SACRIFICE = 2.
#[test]
fn combining_two_tools_adds_twelve_percent_and_costs_two() {
    let Some(reg) = registry() else { return };
    let max = reg
        .max_damage("minecraft:diamond_sword")
        .expect("damageable");
    let mut left = stack("minecraft:diamond_sword");
    left.damage = 1000;
    let mut right = stack("minecraft:diamond_sword");
    right.damage = 1200;
    let got = create_result(&reg, &left, &right, None, false);
    let r = result(&got);

    // 561 + 361 + 1561 * 12 / 100 (= 187) = 1109 restored, from damage 1000.
    let bonus = max * 12 / 100;
    assert_eq!(
        bonus, 187,
        "integer division of the product, not 0.12 * max"
    );
    let restored = (max - 1000) + (max - 1200) + bonus;
    assert_eq!(r.output.damage, (max - restored).max(0));
    assert_eq!(r.cost, 2);
}

// ---------------------------------------------------------------------------
// The enchantment merge
// ---------------------------------------------------------------------------

/// Equal levels combine to one higher; unequal levels take the higher. `max`
/// alone loses the first rule and every "two of the same make one better"
/// combine with it.
#[test]
fn equal_levels_combine_one_higher_and_unequal_take_the_max() {
    let Some(reg) = registry() else { return };
    let mut left = stack("minecraft:diamond_sword");
    left.enchantments = vec![("minecraft:sharpness".to_string(), 3)];

    let got = create_result(
        &reg,
        &left,
        &book(&[("minecraft:sharpness", 3)]),
        None,
        false,
    );
    assert_eq!(result(&got).output.enchantments[0].1, 4, "3 + 3 -> 4");

    let got = create_result(
        &reg,
        &left,
        &book(&[("minecraft:sharpness", 1)]),
        None,
        false,
    );
    assert_eq!(result(&got).output.enchantments[0].1, 3, "3 + 1 -> 3");

    // And the max_level cap, which is 5.
    let mut five = stack("minecraft:diamond_sword");
    five.enchantments = vec![("minecraft:sharpness".to_string(), 5)];
    let got = create_result(
        &reg,
        &five,
        &book(&[("minecraft:sharpness", 5)]),
        None,
        false,
    );
    assert_eq!(
        result(&got).output.enchantments[0].1,
        5,
        "capped at max_level"
    );
}

/// A book halves the anvil cost per level, floored at 1 — `Math.max(1, c / 2)`.
///
/// Sharpness has `anvil_cost` 1, so a book charges `max(1, 0)` = 1 rather than
/// 0: the floor is what stops a cheap enchantment becoming free.
#[test]
fn a_book_halves_the_anvil_cost_with_a_floor_of_one() {
    let Some(reg) = registry() else { return };
    let s = reg.get("minecraft:sharpness").expect("sharpness");
    assert_eq!(s.anvil_cost, 1);
    let got = create_result(
        &reg,
        &stack("minecraft:diamond_sword"),
        &book(&[("minecraft:sharpness", 1)]),
        None,
        false,
    );
    // level 1 * max(1, 1/2) = 1
    assert_eq!(result(&got).cost, 1);

    // Unbreaking's anvil_cost is 2, so a book charges 1 per level and a
    // non-book charges 2 — which is the arm that would vanish if the halving
    // were unconditional.
    let u = reg.get("minecraft:unbreaking").expect("unbreaking");
    assert_eq!(u.anvil_cost, 2);
    let got = create_result(
        &reg,
        &stack("minecraft:diamond_sword"),
        &book(&[("minecraft:unbreaking", 3)]),
        None,
        false,
    );
    assert_eq!(result(&got).cost, 3, "3 levels * max(1, 2/2)");

    let mut other_sword = stack("minecraft:diamond_sword");
    other_sword.enchantments = vec![("minecraft:unbreaking".to_string(), 3)];
    let got = create_result(
        &reg,
        &stack("minecraft:diamond_sword"),
        &other_sword,
        None,
        false,
    );
    assert_eq!(result(&got).cost, 6, "3 levels * 2, no halving");
}

/// The incompatible-pair penalty is charged even though the enchantment is
/// then refused, and the pair still leaves `had_compatible` set when some
/// other enchantment on the same book was applied.
#[test]
fn an_incompatible_pair_is_charged_and_then_refused() {
    let Some(reg) = registry() else { return };
    assert!(!reg.are_compatible("minecraft:sharpness", "minecraft:smite"));
    let mut left = stack("minecraft:diamond_sword");
    left.enchantments = vec![("minecraft:sharpness".to_string(), 1)];

    // Smite alone: refused, and every enchantment on the book was refused.
    let got = create_result(&reg, &left, &book(&[("minecraft:smite", 1)]), None, false);
    assert_eq!(got, Outcome::None(Refusal::AllEnchantmentsRefused));

    // Smite plus unbreaking: unbreaking lands, smite is refused, and the
    // published cost carries the penalty for the pair.
    let got = create_result(
        &reg,
        &left,
        &book(&[("minecraft:smite", 1), ("minecraft:unbreaking", 1)]),
        None,
        false,
    );
    let r = result(&got);
    let has_smite = r
        .output
        .enchantments
        .iter()
        .any(|(k, _)| k == "minecraft:smite");
    assert!(!has_smite, "smite must not land");
    // 1 for the smite/sharpness pair, then unbreaking at max(1, 2/2) * 1.
    assert_eq!(r.cost, 2);
}

/// A stacked left input ASSIGNS `cost = 40`, discarding what was charged, and
/// the 40-level gate then refuses it.
#[test]
fn a_stacked_left_input_is_flattened_to_forty_and_refused() {
    let Some(reg) = registry() else { return };
    let mut left = stack("minecraft:diamond_sword");
    left.count = 2;
    let got = create_result(
        &reg,
        &left,
        &book(&[("minecraft:sharpness", 1)]),
        None,
        false,
    );
    assert_eq!(got, Outcome::None(Refusal::TooExpensive));
    // In creative it survives the gate, at exactly 40 — showing the 40 is an
    // assignment rather than an addition to the 1 the book would have cost.
    let got = create_result(
        &reg,
        &left,
        &book(&[("minecraft:sharpness", 1)]),
        None,
        true,
    );
    assert_eq!(result(&got).cost, 40);
}

/// The two prior-work penalties are summed, and summed as a `long`.
#[test]
fn both_prior_work_penalties_are_added_and_the_sum_is_clamped() {
    let Some(reg) = registry() else { return };
    let mut left = stack("minecraft:diamond_sword");
    left.repair_cost = 7;
    let mut right = book(&[("minecraft:sharpness", 1)]);
    right.repair_cost = 3;
    let got = create_result(&reg, &left, &right, None, true);
    // 1 for the enchantment, plus 7 + 3.
    assert_eq!(result(&got).cost, 11);

    // Near the top of the range the long arithmetic is what keeps the answer
    // positive; an i32 sum would wrap.
    left.repair_cost = i32::MAX;
    right.repair_cost = i32::MAX;
    let got = create_result(&reg, &left, &right, None, true);
    assert_eq!(result(&got).cost, i32::MAX);
    assert!(
        i32::MAX.wrapping_add(i32::MAX) < 0,
        "which is what an i32 sum would have produced"
    );
}
