//! `CombatRules` — how armour, toughness and protection turn a raw damage
//! figure into the figure that reaches health.
//!
//! Reference (26.2), read from the pinned jar with `javap -p -c -constants`:
//! `net.minecraft.world.damagesource.CombatRules` (`getDamageAfterAbsorb`,
//! `getDamageAfterMagicAbsorb`, and all five of its constants) and
//! `net.minecraft.util.Mth.clamp(float,float,float)`.
//!
//! # What main had before this file
//!
//! Nothing: there is no health field on this server, so no damage figure has
//! ever been reduced by anything. `crate::attack` reads attack attributes and
//! sends a swing; nothing on the receiving side existed.
//!
//! # The armour formula, and the two clamps that shape it
//!
//! ```text
//!   f5 = 2.0 + toughness / 4.0
//!   f6 = clamp(armor - damage / f5,  armor * 0.2,  20.0)
//!   f7 = f6 / 25.0
//!   return damage * (1.0 - f7)
//! ```
//!
//! The lower clamp is `armor * 0.2` and **not** a constant: it is why heavy
//! armour never falls below a fifth of its nominal protection however large
//! the hit. The upper clamp is `MAX_ARMOR`, 20.0, so 25 points of armour
//! protects exactly as much as 20 does.
//!
//! Toughness enters only through the divisor. At toughness 0 the divisor is
//! 2.0, so each point of damage eats half a point of effective armour; at
//! netherite's 3.0 it is 2.75.
//!
//! # `getWeaponItem()` and the enchantment hook are a refusal, not an omission
//!
//! Offsets 33..90 replace `f7` with
//! `clamp(EnchantmentHelper.modifyArmorEffectiveness(level, weapon, entity,
//! source, f7), 0.0, 1.0)` whenever the damage source carries a weapon stack
//! **and** the level is a `ServerLevel`. That call walks the weapon's
//! enchantments, and this server has no enchantment model at all.
//!
//! [`ArmorEffectiveness`] makes the two cases the caller's to state rather
//! than silently taking the unenchanted branch: `Unmodified` is vanilla's
//! `weapon == null` path and is exact, `Modified(f)` is for a caller that can
//! supply the hook's answer, and there is no third option that quietly
//! guesses. Everything on this server today is `Unmodified`, and that is a
//! true statement about a world with no enchantments rather than an
//! approximation of one that has them.

use crate::javamath;

/// `CombatRules.MAX_ARMOR` — the upper clamp on effective armour points.
pub const MAX_ARMOR: f32 = 20.0;
/// `CombatRules.ARMOR_PROTECTION_DIVIDER` — armour points per 100% reduction.
pub const ARMOR_PROTECTION_DIVIDER: f32 = 25.0;
/// `CombatRules.BASE_ARMOR_TOUGHNESS` — the constant term of the divisor.
pub const BASE_ARMOR_TOUGHNESS: f32 = 2.0;
/// `CombatRules.MIN_ARMOR_RATIO` — the floor, as a fraction of nominal armour.
pub const MIN_ARMOR_RATIO: f32 = 0.2;
/// `CombatRules.NUM_ARMOR_ITEMS`. Declared and unread by either method here;
/// carried so the class is transcribed whole.
pub const NUM_ARMOR_ITEMS: i32 = 4;

/// The result of `EnchantmentHelper.modifyArmorEffectiveness`, or its absence.
///
/// `getDamageAfterAbsorb` branches on whether the damage source has a weapon
/// stack in a server level. The two branches are different functions, so the
/// caller says which it is in rather than this module guessing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ArmorEffectiveness {
    /// `getWeaponItem()` was null, or the level is not a `ServerLevel`:
    /// offsets 86..88, `f8 = f7`. Exact.
    Unmodified,
    /// The enchantment hook ran and returned this. It is clamped to
    /// `[0.0, 1.0]` here, exactly as offsets 76..81 do.
    Modified(f32),
}

/// `CombatRules.getDamageAfterAbsorb(LivingEntity, float, DamageSource,
/// float, float)`.
///
/// The `LivingEntity` and `DamageSource` arguments are only read to reach the
/// enchantment hook, which [`ArmorEffectiveness`] stands in for, so they are
/// not parameters here.
#[must_use]
pub fn damage_after_absorb(
    damage: f32,
    armor: f32,
    toughness: f32,
    effectiveness: ArmorEffectiveness,
) -> f32 {
    let divisor = BASE_ARMOR_TOUGHNESS + toughness / 4.0;
    let effective_armor =
        javamath::clamp_f(armor - damage / divisor, armor * MIN_ARMOR_RATIO, MAX_ARMOR);
    let ratio = effective_armor / ARMOR_PROTECTION_DIVIDER;
    let applied = match effectiveness {
        ArmorEffectiveness::Unmodified => ratio,
        ArmorEffectiveness::Modified(modified) => javamath::clamp_f(modified, 0.0, 1.0),
    };
    damage * (1.0 - applied)
}

/// `CombatRules.getDamageAfterMagicAbsorb(float, float)` — the Protection
/// enchantment's reduction, and the shape `getDamageAfterMagicAbsorb` on
/// `LivingEntity` reuses.
///
/// Note the divider is the same 25.0 as armour's but the clamp ceiling is
/// `MAX_ARMOR` rather than a separate constant, so 20 points of protection is
/// the ceiling here too.
#[must_use]
pub fn damage_after_magic_absorb(damage: f32, protection: f32) -> f32 {
    let clamped = javamath::clamp_f(protection, 0.0, MAX_ARMOR);
    damage * (1.0 - clamped / ARMOR_PROTECTION_DIVIDER)
}

#[cfg(test)]
mod tests {
    use super::{
        damage_after_absorb, damage_after_magic_absorb, ArmorEffectiveness, MAX_ARMOR,
        MIN_ARMOR_RATIO,
    };

    const UNMOD: ArmorEffectiveness = ArmorEffectiveness::Unmodified;

    #[test]
    fn no_armour_reduces_nothing() {
        assert!((damage_after_absorb(10.0, 0.0, 0.0, UNMOD) - 10.0).abs() < f32::EPSILON);
    }

    #[test]
    fn a_full_iron_set_against_a_small_hit() {
        // armour 15, toughness 0, damage 4:
        //   divisor 2.0, armor - 4/2 = 13, clamp(13, 3, 20) = 13,
        //   13/25 = 0.52, 4 * 0.48 = 1.92
        let d = damage_after_absorb(4.0, 15.0, 0.0, UNMOD);
        assert!((d - 1.92).abs() < 1e-5, "{d}");
    }

    #[test]
    fn a_large_hit_eats_the_armour_down_to_the_ratio_floor() {
        // armour 20, toughness 0, damage 200: 20 - 100 = -80, floored at
        // 20 * 0.2 = 4, so 4/25 = 0.16 and 200 * 0.84 = 168.
        let d = damage_after_absorb(200.0, 20.0, 0.0, UNMOD);
        assert!((d - 168.0).abs() < 1e-3, "{d}");
    }

    #[test]
    fn the_floor_is_a_fraction_of_the_armour_and_not_a_constant() {
        // Half the armour, same enormous hit: the floor halves with it.
        let d = damage_after_absorb(200.0, 10.0, 0.0, UNMOD);
        let floor_ratio = 10.0 * MIN_ARMOR_RATIO / 25.0;
        assert!((d - 200.0 * (1.0 - floor_ratio)).abs() < 1e-3, "{d}");
    }

    #[test]
    fn armour_above_twenty_protects_no_further() {
        // Both must actually REACH the ceiling for the comparison to be
        // about the ceiling: at armour 20 and a 1.0 hit the effective value
        // is 19.96, still under it. 25 and 30 both clamp down to 20.0.
        let at_25 = damage_after_absorb(1.0, 25.0, 100.0, UNMOD);
        let at_30 = damage_after_absorb(1.0, 30.0, 100.0, UNMOD);
        assert!((at_25 - at_30).abs() < 1e-9, "{at_25} {at_30}");
        // ...and 20 does not, which is what makes the previous line a claim
        // about the clamp rather than about float noise.
        let at_20 = damage_after_absorb(1.0, MAX_ARMOR, 100.0, UNMOD);
        assert!(at_20 > at_25, "{at_20} {at_25}");
    }

    #[test]
    fn toughness_only_widens_the_divisor() {
        // Netherite toughness 3.0: divisor 2.75 rather than 2.0.
        let plain = damage_after_absorb(20.0, 20.0, 0.0, UNMOD);
        let tough = damage_after_absorb(20.0, 20.0, 3.0, UNMOD);
        assert!(tough < plain, "{tough} {plain}");
        let expect = {
            let divisor = 2.0 + 3.0 / 4.0;
            let eff = 20.0_f32 - 20.0 / divisor;
            20.0 * (1.0 - eff / 25.0)
        };
        assert!((tough - expect).abs() < 1e-5, "{tough} {expect}");
    }

    #[test]
    fn the_enchantment_hook_result_is_clamped_to_one() {
        let all = damage_after_absorb(10.0, 20.0, 0.0, ArmorEffectiveness::Modified(5.0));
        assert!(all.abs() < f32::EPSILON, "{all}");
        let none = damage_after_absorb(10.0, 20.0, 0.0, ArmorEffectiveness::Modified(-5.0));
        assert!((none - 10.0).abs() < f32::EPSILON, "{none}");
    }

    #[test]
    fn magic_absorb_is_four_percent_a_point_and_caps_at_twenty() {
        assert!((damage_after_magic_absorb(10.0, 0.0) - 10.0).abs() < f32::EPSILON);
        assert!((damage_after_magic_absorb(10.0, 5.0) - 8.0).abs() < 1e-6);
        let at_20 = damage_after_magic_absorb(10.0, 20.0);
        let at_25 = damage_after_magic_absorb(10.0, 25.0);
        assert!((at_20 - 2.0).abs() < 1e-6, "{at_20}");
        assert!((at_20 - at_25).abs() < f32::EPSILON);
    }

    #[test]
    fn negative_protection_is_clamped_up_rather_than_amplifying() {
        assert!((damage_after_magic_absorb(10.0, -40.0) - 10.0).abs() < f32::EPSILON);
    }

    /// The positive control. Three misreadings of the armour formula, each
    /// written out and each shown disagreeing with the transcription.
    ///
    /// Every test above would pass against a formula that used a constant
    /// floor of 0.0 for small hits, so the floor's *shape* has to be shown to
    /// be separable and not merely asserted.
    #[test]
    fn each_misreading_of_the_armour_formula_is_separable() {
        // 1. The lower clamp read as the constant 0.0 rather than
        //    `armor * 0.2`. Only a hit large enough to drive the effective
        //    armour negative can tell the two apart, which is why the test
        //    above uses 200 damage.
        let real = damage_after_absorb(200.0, 20.0, 0.0, UNMOD);
        let zero_floor = {
            let eff = crate::javamath::clamp_f(20.0 - 200.0 / 2.0, 0.0, MAX_ARMOR);
            200.0 * (1.0 - eff / 25.0)
        };
        assert_ne!(real, zero_floor);

        // 2. Toughness added to the armour rather than to the divisor.
        let real_t = damage_after_absorb(20.0, 20.0, 3.0, UNMOD);
        let added = {
            let eff =
                crate::javamath::clamp_f(20.0 + 3.0 - 20.0 / 2.0, (20.0 + 3.0) * 0.2, MAX_ARMOR);
            20.0 * (1.0 - eff / 25.0)
        };
        assert_ne!(real_t, added);

        // 3. The divider read as 20.0 (the armour cap) rather than 25.0 — the
        //    two constants sit next to each other in the class.
        let real_d = damage_after_absorb(4.0, 15.0, 0.0, UNMOD);
        let wrong_divider = {
            let eff = crate::javamath::clamp_f(15.0 - 4.0 / 2.0, 3.0, MAX_ARMOR);
            4.0 * (1.0 - eff / 20.0)
        };
        assert_ne!(real_d, wrong_divider);
    }
}
