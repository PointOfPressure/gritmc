//! `EnchantmentHelper.processDurabilityChange` — what the stack's
//! enchantments do to a durability change before `applyDamage` sees it.
//! Unbreaking, in 26.2, and nothing else: it is the only enchantment whose
//! definition carries a `minecraft:item_damage` component.
//!
//! # The chain
//!
//! ```text
//!   EnchantmentHelper.processDurabilityChange(level, stack, amount)
//!      0-6    f = new MutableFloat((float) amount)
//!     14-19   runIterationOnItem(stack, (holder, lvl) ->
//!                 holder.value().modifyDurabilityChange(level, lvl, stack, f))
//!     23      return f.intValue()                         <- truncation
//!
//!   Enchantment.modifyItemFilteredCount(ITEM_DAMAGE, level, lvl, stack, f)
//!      2-21   applyEffects(getEffects(ITEM_DAMAGE), itemContext(level, lvl, stack),
//!                 f, e -> e.process(lvl, level.getRandom(), f.floatValue()))
//!
//!   Enchantment.applyEffects(list, ctx, action)           0-50
//!             for each ConditionalEffect: if (matches(ctx)) action.apply(effect)
//! ```
//!
//! `matches` is the effect's `requirements`, a loot condition over a context
//! whose `TOOL` is the stack — Unbreaking's two arms are
//! `match_tool #enchantable/armor` and its inversion, so armour takes
//! `(2 + 2(L-1)) / (10 + 5(L-1))` and everything else `L / (L + 1)`. The
//! conditions are `rustmc_loot`'s, the same interpreter the block drops use.
//!
//! # One deviation, named
//!
//! `level.getRandom()` is the level's own stream, advanced by everything else
//! the world does; this draws from a fresh `crate::dropitem::unique_random`,
//! as every other level-random draw in this server does. So which digs an
//! Unbreaking tool shrugs off differs run to run — as it does in vanilla —
//! and no corpus check can grade a single dig of one.

use rustmc_worldgen::rng::legacy::LegacyRandomSource;
use rustmc_worldgen::rng::RandomSource;
use serde_json::Value;

/// `LevelBasedValue.calculate(level)` for the three forms an
/// `item_damage` effect is written in — a bare number (`Constant`),
/// `minecraft:linear` and `minecraft:fraction` — or `None` for one this
/// does not transcribe.
///
/// ```text
///   Linear.calculate     0-14   base + perLevelAboveFirst * (float)(level - 1)
///   Fraction.calculate   0-31   d = denominator.calculate(level)
///                               d == 0 ? 0 : numerator.calculate(level) / d
/// ```
#[must_use]
pub fn level_based(v: &Value, level: i32) -> Option<f32> {
    if let Some(n) = v.as_f64() {
        #[expect(clippy::cast_possible_truncation, reason = "a float in the datapack")]
        return Some(n as f32);
    }
    let f = |k: &str| {
        #[expect(clippy::cast_possible_truncation, reason = "a float in the datapack")]
        v.get(k).and_then(Value::as_f64).map(|x| x as f32)
    };
    match v.get("type").and_then(Value::as_str)? {
        "minecraft:linear" => {
            #[expect(clippy::cast_precision_loss, reason = "i2f, as the bytecode does")]
            let above = (level - 1) as f32;
            Some(f("base")? + f("per_level_above_first")? * above)
        }
        "minecraft:fraction" => {
            let d = level_based(v.get("denominator")?, level)?;
            if d == 0.0 {
                return Some(0.0);
            }
            Some(level_based(v.get("numerator")?, level)? / d)
        }
        _ => None,
    }
}

/// `RemoveBinomial.process(level, random, value)`, offsets 0-138.
///
/// The Gaussian arm needs a stack past 128 points of damage in one change,
/// which no dig produces, and it rests on `RandomSource::next_gaussian`,
/// which `rustmc_worldgen` marks unproven against vanilla's vectors.
///
/// ```text
///   chance = this.chance.calculate(level); removed = 0
///   if (value <= 128 || value * chance < 20 || value * (1 - chance) < 20)
///       for (i = 0; (float) i < value; i++)
///           if (random.nextFloat() < chance) removed++
///   else
///       removed = (int) Math.round(floor(value * chance)
///                     + random.nextGaussian() * sqrt(value * chance * (1 - chance)))
///       removed = Math.clamp((long) removed, 0, (int) value)
///   return value - removed
/// ```
///
pub fn remove_binomial(chance: f32, value: f32, random: &mut impl RandomSource) -> f32 {
    let mut removed: i32 = 0;
    // Three `fcmpg`s: `ifle`, `iflt`, `ifge`. Each is false on a NaN, so a
    // NaN falls through all three to the Gaussian arm.
    let loop_arm = value <= 128.0 || value * chance < 20.0 || value * (1.0 - chance) < 20.0;
    if loop_arm {
        let mut i: i32 = 0;
        #[expect(clippy::cast_precision_loss, reason = "i2f, as the bytecode does")]
        while (i as f32) < value {
            if random.next_f32() < chance {
                removed += 1;
            }
            i += 1;
        }
    } else {
        let mean = f64::from(value * chance).floor();
        let sd = f64::from(value * chance * (1.0 - chance)).sqrt();
        #[expect(clippy::cast_possible_truncation, reason = "Math.round(D)J; l2i")]
        let r = (mean + random.next_gaussian() * sd).round() as i64 as i32;
        #[expect(clippy::cast_possible_truncation, reason = "f2i, as the bytecode does")]
        let hi = value as i32;
        // `Math.clamp(long, int, int)`, offsets 122-128.
        removed = i64::from(r).clamp(0, i64::from(hi.max(0))) as i32;
    }
    #[expect(clippy::cast_precision_loss, reason = "i2f, as the bytecode does")]
    let removed = removed as f32;
    value - removed
}

/// An enchantment id as the registry keys it: `ItemStack` keeps its ids
/// bare (`silk_touch`), the registry and its tags namespaced.
#[must_use]
pub fn namespaced(id: &str) -> String {
    if id.contains(':') {
        id.to_string()
    } else {
        format!("minecraft:{id}")
    }
}

/// What a context needs to answer an effect's `requirements`.
pub struct Requirements<'a> {
    /// The stack's item id — `match_tool`'s subject.
    pub item: &'a str,
    /// The item tags `#minecraft:enchantable/armor` resolves through.
    pub item_tags: std::sync::Arc<std::collections::BTreeMap<String, Vec<String>>>,
}

/// One enchantment's `modifyDurabilityChange` on `f`: every conditional
/// `item_damage` effect whose requirements hold, in datapack order.
///
/// A requirement or an effect this does not transcribe is a refusal: it is
/// logged and leaves `f` as it was, rather than guessing.
pub fn modify_durability_change(
    effects: &[Value],
    level: i32,
    req: &Requirements<'_>,
    f: f32,
    random: &mut LegacyRandomSource,
) -> f32 {
    let mut f = f;
    for conditional in effects {
        if let Some(cond) = conditional.get("requirements") {
            let mut ctx = rustmc_loot::Ctx {
                block: None,
                tool: Some(req.item.to_string()),
                explosion_radius: None,
                luck: 0.0,
                enchants: std::collections::BTreeMap::new(),
                item_tags: std::sync::Arc::clone(&req.item_tags),
                tables: std::sync::Arc::new(std::collections::BTreeMap::new()),
                faults: rustmc_loot::Faults::default(),
                refused: None,
                ..Default::default()
            };
            match rustmc_loot::test_condition(cond, &mut ctx, random) {
                Some(true) => {}
                Some(false) => continue,
                None => {
                    eprintln!("item_damage: unanswerable requirement {:?}", ctx.refused);
                    continue;
                }
            }
        }
        let Some(effect) = conditional.get("effect") else {
            continue;
        };
        match effect.get("type").and_then(Value::as_str) {
            Some("minecraft:remove_binomial") => {
                let Some(chance) = effect.get("chance").and_then(|c| level_based(c, level)) else {
                    eprintln!("item_damage: unported chance {effect}");
                    continue;
                };
                f = remove_binomial(chance, f, random);
            }
            other => eprintln!("item_damage: unported effect {other:?}"),
        }
    }
    f
}

/// `EnchantmentHelper.processDurabilityChange(level, stack, amount)` over
/// the stack's `(enchantment, level)` pairs.
///
/// `runIterationOnItem` walks the `ItemEnchantments` map in its own order;
/// with one enchantment carrying `item_damage` in 26.2 that order cannot
/// change the answer, so the stack's own order is used.
pub fn process_durability_change(
    registry: &crate::enchantment::Registry,
    enchantments: &[(String, i32)],
    req: &Requirements<'_>,
    amount: i32,
    random: &mut LegacyRandomSource,
) -> i32 {
    #[expect(clippy::cast_precision_loss, reason = "i2f, as the bytecode does")]
    let mut f = amount as f32;
    for (id, level) in enchantments {
        if let Some(e) = registry.get(&namespaced(id)) {
            f = modify_durability_change(&e.item_damage, *level, req, f, random);
        }
    }
    // `MutableFloat.intValue()` — `(int) value`: truncation, NaN to 0.
    #[expect(clippy::cast_possible_truncation, reason = "f2i")]
    let out = f as i32;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unbreaking_chance(level: i32, armour: bool) -> f32 {
        let v: Value = if armour {
            serde_json::json!({"type": "minecraft:fraction",
                "denominator": {"type": "minecraft:linear", "base": 10.0, "per_level_above_first": 5.0},
                "numerator": {"type": "minecraft:linear", "base": 2.0, "per_level_above_first": 2.0}})
        } else {
            serde_json::json!({"type": "minecraft:fraction",
                "denominator": {"type": "minecraft:linear", "base": 2.0, "per_level_above_first": 1.0},
                "numerator": {"type": "minecraft:linear", "base": 1.0, "per_level_above_first": 1.0}})
        };
        level_based(&v, level).expect("transcribed")
    }

    /// Unbreaking's two arms: a tool keeps `L / (L + 1)` of its hits off,
    /// armour `(2 + 2(L-1)) / (10 + 5(L-1))`.
    #[test]
    fn unbreaking_chances_are_the_datapack_fractions() {
        assert!((unbreaking_chance(1, false) - 0.5).abs() < 1e-7);
        assert!((unbreaking_chance(3, false) - 0.75).abs() < 1e-7);
        assert!((unbreaking_chance(1, true) - 0.2).abs() < 1e-7);
        assert!((unbreaking_chance(3, true) - 0.3).abs() < 1e-7);
        assert_eq!(
            level_based(
                &serde_json::json!({"type": "minecraft:fraction",
                "numerator": 1.0, "denominator": 0.0}),
                1
            ),
            Some(0.0),
            "a zero denominator is 0, not inf"
        );
        assert_eq!(
            level_based(&serde_json::json!({"type": "minecraft:lookup"}), 1),
            None
        );
    }

    /// The loop arm draws one float per point of damage, and a point is
    /// removed when the draw falls under the chance — so chance 1 removes
    /// all, chance 0 none, and the draw count is the value.
    #[test]
    fn the_loop_arm_draws_once_per_point() {
        let mut r = LegacyRandomSource::new(7);
        assert_eq!(remove_binomial(1.0, 3.0, &mut r), 0.0);
        assert_eq!(remove_binomial(0.0, 3.0, &mut r), 3.0);
        let mut a = LegacyRandomSource::new(42);
        let mut b = LegacyRandomSource::new(42);
        let out = remove_binomial(0.5, 4.0, &mut a);
        let kept = (0..4).filter(|_| b.next_f32() >= 0.5).count();
        #[expect(clippy::cast_precision_loss, reason = "a count under five")]
        let kept = kept as f32;
        assert_eq!(out, kept);
        assert_eq!(a.next_i64(), b.next_i64(), "same number of draws");
    }

    /// Past 128 points with both tails at least 20, one Gaussian replaces the
    /// loop: a single draw, clamped to the value.
    #[test]
    fn the_gaussian_arm_draws_once() {
        let mut a = LegacyRandomSource::new(9);
        let mut b = LegacyRandomSource::new(9);
        let out = remove_binomial(0.5, 200.0, &mut a);
        let g = b.next_gaussian();
        let removed = (100.0 + g * 50.0_f64.sqrt()).round().clamp(0.0, 200.0);
        #[expect(clippy::cast_possible_truncation, reason = "test arithmetic")]
        let expect = 200.0 - removed as f32;
        assert_eq!(out, expect);
        assert_eq!(a.next_i64(), b.next_i64());
    }

    /// The real Unbreaking definition, its requirements answered by the loot
    /// interpreter over the jar's item tags: a pickaxe takes the tool arm
    /// (3/4 at level III), a chestplate the armour arm (3/10).
    #[test]
    fn unbreaking_picks_its_arm_by_the_armour_tag() {
        let root = {
            let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            p.pop();
            p.pop();
            p
        };
        let reg_path = root.join(crate::enchantment::TABLE_PATH);
        let tags_path = root.join("data/vanilla/item_tags.json");
        if !rustmc_data::fixture::available(
            &reg_path,
            "enchantment table",
            "ci/gen-gameplay-tables.sh",
        ) || !rustmc_data::fixture::available(
            &tags_path,
            "item tags",
            "ci/gen-gameplay-tables.sh",
        ) {
            return;
        }
        let reg = crate::enchantment::Registry::load(&reg_path).expect("registry");
        let tags: std::collections::BTreeMap<String, Vec<String>> =
            serde_json::from_str(&std::fs::read_to_string(&tags_path).expect("tags"))
                .expect("json");
        let tags = std::sync::Arc::new(tags);
        // Bare, as `ItemStack::with_enchantment` stores it.
        let ench = vec![("unbreaking".to_string(), 3)];
        for (item, chance) in [
            ("minecraft:diamond_pickaxe", 0.75_f32),
            ("minecraft:diamond_chestplate", 0.3),
        ] {
            let req = Requirements {
                item,
                item_tags: std::sync::Arc::clone(&tags),
            };
            for seed in 0..32 {
                let mut a = LegacyRandomSource::new(seed);
                let mut b = LegacyRandomSource::new(seed);
                let got = process_durability_change(&reg, &ench, &req, 5, &mut a);
                #[expect(clippy::cast_possible_truncation, reason = "f2i")]
                let want = remove_binomial(chance, 5.0, &mut b) as i32;
                assert_eq!(got, want, "{item} seed {seed}");
            }
        }
        let none: Vec<(String, i32)> = Vec::new();
        let req = Requirements {
            item: "minecraft:diamond_pickaxe",
            item_tags: tags,
        };
        assert_eq!(
            process_durability_change(&reg, &none, &req, 5, &mut LegacyRandomSource::new(1)),
            5,
            "no enchantment, no change"
        );
    }
}
