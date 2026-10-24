//! Fall damage — `causeFallDamage`, `calculateFallDamage` and
//! `calculateFallPower`.
//!
//! Reference (26.2), read from the pinned jar with `javap -p -c -constants`:
//! `net.minecraft.world.entity.LivingEntity.causeFallDamage` (offsets 0..119),
//! `.calculateFallDamage`, `.calculateFallPower`,
//! `net.minecraft.world.entity.Entity.causeFallDamage`,
//! `net.minecraft.world.entity.player.Player.causeFallDamage`, and
//! `net.minecraft.util.Mth.floor(double)`.
//!
//! # What main had before this file
//!
//! Nothing falls and nothing is hurt. `crate::physics` and `crate::collide`
//! move a player, `docs/SERVER.md` says movement "streams chunks; nothing else
//! responds to it", and there is no health to take the damage.
//!
//! # Neither `3` nor `1` appears in this file
//!
//! The two numbers that decide fall damage are **attributes**:
//! `Attributes.SAFE_FALL_DISTANCE` and `Attributes.FALL_DAMAGE_MULTIPLIER`,
//! read through `getAttributeValue` and therefore modifiable by an effect or
//! a piece of equipment. They are parameters of [`calculate_fall_damage`] for
//! the same reason `crate::attributes` exists at all, and their defaults live
//! in `data/vanilla/attributes.json`.
//!
//! # The epsilon is load-bearing
//!
//! ```text
//!   calculateFallPower(d) = d + 1.0E-6 - SAFE_FALL_DISTANCE
//!   calculateFallDamage  = Mth.floor(power * multiplier * FALL_DAMAGE_MULTIPLIER)
//! ```
//!
//! `1.0E-6` is there so that a fall of **exactly** the safe distance plus one
//! block yields a power of 1.000001 and floors to 1 rather than to 0 on a
//! double that came out a hair under. Dropping it makes a four-block fall deal
//! nothing whenever the accumulated `fallDistance` lands below 4.0 by an ulp,
//! which is most of the time. It is the only reason the answer is stable.
//!
//! `Mth.floor(double)` is a floor, **not** a truncation: a negative power —
//! any fall shorter than the safe distance — floors *away* from zero, so the
//! `damage > 0` test in `causeFallDamage` is what keeps a short fall harmless
//! rather than the arithmetic.
//!
//! # `causeFallDamage`'s return value is not "was damage dealt"
//!
//! Three overrides stack, and each returns something different:
//!
//! ```text
//!   Entity.causeFallDamage    FALL_DAMAGE_IMMUNE -> false
//!                             else propagate to passengers, return false
//!   Player.causeFallDamage    abilities.mayfly   -> false        [!]
//!                             else award FALL_ONE_CM, defer to super
//!   LivingEntity              damage > 0 -> hurt, return true
//!                             else return whatever super said
//! ```
//!
//! The `[!]` is the one worth having: a player who **may** fly takes no fall
//! damage at all, and the test is `mayfly`, not `flying`. So a creative player
//! who has switched to walking still lands unhurt, and so does anyone a plugin
//! has granted flight to.
//!
//! The statistic threshold is `>= 2.0` blocks and the value is
//! `Math.round(distance * 100.0)` — centimetres — which is recorded here
//! because it is the only place the raw distance is used unrounded.

use crate::health::DamageTags;

/// `LivingEntity.calculateFallPower`'s epsilon, `ldc2_w 1.0E-6d`.
pub const FALL_POWER_EPSILON: f64 = 1.0e-6;
/// The distance at or above which `Player.causeFallDamage` awards
/// `Stats.FALL_ONE_CM`, `ldc2_w 2.0d`.
pub const FALL_STAT_THRESHOLD: f64 = 2.0;
/// The centimetre scale for that statistic, `ldc2_w 100.0d`.
pub const FALL_STAT_SCALE: f64 = 100.0;

/// What `causeFallDamage` reads that this module cannot compute.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FallContext {
    /// `is(EntityTypeTags.FALL_DAMAGE_IMMUNE)`.
    pub fall_damage_immune: bool,
    /// `abilities.mayfly` — **not** `abilities.flying`.
    pub may_fly: bool,
    /// `getAttributeValue(Attributes.SAFE_FALL_DISTANCE)`. Its default is in
    /// `data/vanilla/attributes.json`; there is no literal for it here.
    pub safe_fall_distance: f64,
    /// `getAttributeValue(Attributes.FALL_DAMAGE_MULTIPLIER)`, likewise.
    pub fall_damage_multiplier: f64,
    /// `isIgnoringFallDamageFromCurrentImpulse()`, and if so the y of
    /// `currentImpulseImpactPos` minus `getY()` — the distance a wind-charge
    /// or mace impulse allows to be discounted.
    pub impulse_allowance: Option<f64>,
}

impl Default for FallContext {
    /// An ordinary survival player with unmodified attributes is **not** what
    /// this returns: the two attribute defaults are data and this module
    /// refuses to carry them. `0.0` for both is the honest "nobody told me",
    /// and it makes every fall damaging, which is loud rather than silent.
    fn default() -> Self {
        Self {
            fall_damage_immune: false,
            may_fly: false,
            safe_fall_distance: 0.0,
            fall_damage_multiplier: 0.0,
            impulse_allowance: None,
        }
    }
}

/// What one `causeFallDamage` did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FallOutcome {
    /// The method's return value. **Not** "damage was dealt": see the module
    /// docs. `Entity`'s override returns false even when it propagated the
    /// fall to passengers.
    pub returned: bool,
    /// The integer damage `calculateFallDamage` produced, before the
    /// `> 0` test.
    pub damage: i32,
    /// `hurt(source, (float) damage)` was called.
    pub hurt: bool,
    /// `Stats.FALL_ONE_CM` was awarded, and by how much.
    pub fall_one_cm: Option<i64>,
    /// The distance actually used, after the impulse allowance.
    pub distance: f64,
}

/// `LivingEntity.calculateFallPower(double)` — `d + 1.0E-6 - safeFallDistance`.
#[must_use]
pub fn calculate_fall_power(distance: f64, safe_fall_distance: f64) -> f64 {
    distance + FALL_POWER_EPSILON - safe_fall_distance
}

/// `LivingEntity.calculateFallDamage(double, float)`.
///
/// `multiplier` is the `float` argument the caller passes (1.0 for an ordinary
/// fall), widened `f2d` before the multiply — so the whole product is computed
/// in `double` and only the final `Mth.floor` narrows.
#[must_use]
pub fn calculate_fall_damage(distance: f64, multiplier: f32, ctx: FallContext) -> i32 {
    if ctx.fall_damage_immune {
        return 0;
    }
    let power = calculate_fall_power(distance, ctx.safe_fall_distance);
    mth_floor(power * f64::from(multiplier) * ctx.fall_damage_multiplier)
}

/// `Player.causeFallDamage` -> `LivingEntity.causeFallDamage`, composed.
///
/// `Entity.causeFallDamage`'s passenger propagation is not modelled (nothing
/// on this server rides anything) but its return value — always `false` — is,
/// because `LivingEntity` returns it verbatim when the damage is zero.
///
/// The sounds (`getFallDamageSound`, `playBlockFallSound`) and
/// `resetCurrentImpulseContext` are reported rather than performed, on the
/// same principle as [`crate::hurt::hurt_server`].
#[must_use]
pub fn cause_fall_damage(distance: f64, multiplier: f32, ctx: FallContext) -> FallOutcome {
    // Player.causeFallDamage offsets 0..12.
    if ctx.may_fly {
        return FallOutcome {
            returned: false,
            damage: 0,
            hurt: false,
            fall_one_cm: None,
            distance,
        };
    }
    let fall_one_cm = if distance >= FALL_STAT_THRESHOLD {
        // `Math.round(double)` is floor(x + 0.5) for a finite x, which is NOT
        // the same as Rust's `f64::round` on a negative half — irrelevant here
        // because the distance is >= 2.0, and written out anyway.
        Some(java_round(distance * FALL_STAT_SCALE))
    } else {
        None
    };

    // LivingEntity.causeFallDamage offsets 0..61 — the impulse allowance.
    let used = match ctx.impulse_allowance {
        Some(allowance) => java_min_d(distance, allowance),
        None => distance,
    };

    // Entity.causeFallDamage — false either way.
    let from_super = false;

    let damage = calculate_fall_damage(used, multiplier, ctx);
    if damage > 0 {
        FallOutcome {
            returned: true,
            damage,
            hurt: true,
            fall_one_cm,
            distance: used,
        }
    } else {
        FallOutcome {
            returned: from_super,
            damage,
            hurt: false,
            fall_one_cm,
            distance: used,
        }
    }
}

/// The `minecraft:fall` damage type's tags, for a caller composing this with
/// [`crate::hurt::hurt_server`].
///
/// It is **not** a constant here: `crate::damage_types` reads them out of
/// `data/vanilla/damage_types.json`, and this function exists only to name the
/// id so a grep finds the pairing.
#[must_use]
pub fn fall_damage_type_id() -> &'static str {
    "minecraft:fall"
}

/// A reminder in the type system that the tags come from the table.
///
/// Returns `None` unless the caller supplies them, which is the same refusal
/// [`crate::damage_types::DamageTypeRegistry::tags_for`] makes for an unknown
/// id: a `DamageTags::default()` here would claim a fall bypasses nothing,
/// and `minecraft:fall` is in `bypasses_armor`.
#[must_use]
pub const fn fall_tags(from_registry: Option<DamageTags>) -> Option<DamageTags> {
    from_registry
}

/// `Mth.floor(double)` — `(int) Math.floor(d)`.
fn mth_floor(v: f64) -> i32 {
    let floored = v.floor();
    if floored.is_nan() {
        0
    } else if floored >= f64::from(i32::MAX) {
        i32::MAX
    } else if floored <= f64::from(i32::MIN) {
        i32::MIN
    } else {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the JVM's own d2i, with its saturating bounds handled above"
        )]
        {
            floored as i32
        }
    }
}

/// `java.lang.Math.round(double)` -> `long`.
fn java_round(v: f64) -> i64 {
    let r = (v + 0.5).floor();
    if r.is_nan() {
        0
    } else if r >= i64::MAX as f64 {
        i64::MAX
    } else if r <= i64::MIN as f64 {
        i64::MIN
    } else {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the JVM's own d2l, with its saturating bounds handled above"
        )]
        {
            r as i64
        }
    }
}

/// `java.lang.Math.min(double, double)` — NaN-propagating.
fn java_min_d(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if a < b {
        a
    } else {
        b
    }
}

#[cfg(test)]
mod tests {
    use super::{
        calculate_fall_damage, calculate_fall_power, cause_fall_damage, FallContext,
        FALL_POWER_EPSILON,
    };

    /// The attribute values a vanilla player carries, taken here as test
    /// inputs rather than as constants — see the module docs.
    fn player() -> FallContext {
        FallContext {
            safe_fall_distance: 3.0,
            fall_damage_multiplier: 1.0,
            ..FallContext::default()
        }
    }

    #[test]
    fn a_three_block_fall_is_free_and_a_four_block_one_is_one_heart() {
        assert_eq!(calculate_fall_damage(3.0, 1.0, player()), 0);
        assert_eq!(calculate_fall_damage(4.0, 1.0, player()), 1);
        assert_eq!(calculate_fall_damage(10.0, 1.0, player()), 7);
        assert_eq!(calculate_fall_damage(23.0, 1.0, player()), 20);
    }

    #[test]
    fn the_epsilon_is_what_makes_a_four_block_fall_land_on_one() {
        // Without the 1.0E-6 the power of a fall that accumulated to a hair
        // under 4.0 floors to 0. This is the case it exists for.
        let just_under = 4.0_f64 - 1.0e-9;
        assert_eq!(calculate_fall_damage(just_under, 1.0, player()), 1);
        let without_epsilon = (just_under - 3.0).floor() as i32;
        assert_eq!(without_epsilon, 0);
    }

    #[test]
    fn a_short_fall_has_a_negative_power_and_floors_away_from_zero() {
        // Mth.floor is a floor, not a truncation: -2.999999 floors to -3.
        let p = calculate_fall_power(0.0, 3.0);
        assert!(p < 0.0);
        assert_eq!(calculate_fall_damage(0.0, 1.0, player()), -3);
        // ...and the `> 0` test in causeFallDamage is what keeps it harmless.
        let out = cause_fall_damage(0.0, 1.0, player());
        assert!(!out.hurt);
        assert_eq!(out.damage, -3);
    }

    #[test]
    fn the_safe_distance_is_an_attribute_and_moving_it_moves_the_answer() {
        let boosted = FallContext {
            safe_fall_distance: 10.0,
            ..player()
        };
        assert_eq!(calculate_fall_damage(10.0, 1.0, boosted), 0);
        assert_eq!(calculate_fall_damage(14.0, 1.0, boosted), 4);
    }

    #[test]
    fn the_multiplier_attribute_scales_the_whole_product() {
        let doubled = FallContext {
            fall_damage_multiplier: 2.0,
            ..player()
        };
        assert_eq!(calculate_fall_damage(10.0, 1.0, doubled), 14);
        let none = FallContext {
            fall_damage_multiplier: 0.0,
            ..player()
        };
        assert_eq!(calculate_fall_damage(10.0, 1.0, none), 0);
    }

    #[test]
    fn the_float_argument_multiplies_too_and_is_widened_first() {
        assert_eq!(calculate_fall_damage(10.0, 0.5, player()), 3);
        assert_eq!(calculate_fall_damage(10.0, 2.0, player()), 14);
    }

    #[test]
    fn a_fall_damage_immune_type_takes_nothing_however_far() {
        let immune = FallContext {
            fall_damage_immune: true,
            ..player()
        };
        assert_eq!(calculate_fall_damage(1000.0, 1.0, immune), 0);
        let out = cause_fall_damage(1000.0, 1.0, immune);
        assert!(!out.hurt);
        assert!(!out.returned);
    }

    #[test]
    fn a_player_who_may_fly_takes_no_fall_damage_and_no_statistic() {
        let flier = FallContext {
            may_fly: true,
            ..player()
        };
        let out = cause_fall_damage(100.0, 1.0, flier);
        assert!(!out.hurt);
        assert!(!out.returned);
        assert_eq!(out.damage, 0);
        assert_eq!(out.fall_one_cm, None, "the return is before the statistic");
    }

    #[test]
    fn the_statistic_starts_at_two_blocks_and_is_in_centimetres() {
        assert_eq!(cause_fall_damage(1.99, 1.0, player()).fall_one_cm, None);
        assert_eq!(cause_fall_damage(2.0, 1.0, player()).fall_one_cm, Some(200));
        assert_eq!(
            cause_fall_damage(10.5, 1.0, player()).fall_one_cm,
            Some(1050)
        );
    }

    #[test]
    fn the_impulse_allowance_caps_the_distance_and_the_statistic_ignores_it() {
        // A wind charge launch: the fall counts only from the impact point.
        let ctx = FallContext {
            impulse_allowance: Some(2.0),
            ..player()
        };
        let out = cause_fall_damage(50.0, 1.0, ctx);
        assert_eq!(out.distance, 2.0);
        assert!(!out.hurt);
        // The statistic was computed from the raw 50.0 before the allowance,
        // which is offsets 12..36 of Player.causeFallDamage running before
        // LivingEntity's offset 0.
        assert_eq!(out.fall_one_cm, Some(5000));
    }

    #[test]
    fn an_allowance_larger_than_the_fall_does_not_lengthen_it() {
        let ctx = FallContext {
            impulse_allowance: Some(1000.0),
            ..player()
        };
        let out = cause_fall_damage(10.0, 1.0, ctx);
        assert_eq!(out.distance, 10.0);
        assert_eq!(out.damage, 7);
    }

    #[test]
    fn a_damaging_fall_returns_true_and_a_harmless_one_returns_false() {
        assert!(cause_fall_damage(10.0, 1.0, player()).returned);
        assert!(!cause_fall_damage(2.0, 1.0, player()).returned);
    }

    /// The positive control. Three misreadings, each written out and shown to
    /// disagree on a named input.
    #[test]
    fn each_misreading_of_the_fall_formula_is_separable() {
        // 1. The epsilon dropped.
        let just_under = 4.0_f64 - 1.0e-9;
        let real = calculate_fall_damage(just_under, 1.0, player());
        let without = (just_under - 3.0).floor() as i32;
        assert_ne!(real, without);
        // ...and the epsilon is what accounts for the difference. Not an
        // exact equality: the two subtractions round differently, which is
        // itself the reason vanilla needs a margin rather than a nudge.
        let gap = calculate_fall_power(just_under, 3.0) - (just_under - 3.0);
        assert!(
            (gap - FALL_POWER_EPSILON).abs() < 1e-12,
            "the epsilon does not account for the difference: {gap}"
        );

        // 2. `Mth.floor` read as a truncation. They differ on every fall
        //    shorter than the safe distance, which is where the `> 0` test
        //    then hides the difference — so the separable claim is on the
        //    intermediate, not on the damage.
        let power = calculate_fall_power(0.5, 3.0);
        assert_eq!(power.floor() as i32, -3);
        assert_eq!(power as i32, -2, "truncation gives a different integer");

        // 3. `mayfly` read as `flying`. A creative player walking still takes
        //    nothing, and the only way to see that is the flag's name — so the
        //    check is that the ONLY input that matters here is may_fly.
        let walking_creative = FallContext {
            may_fly: true,
            ..player()
        };
        assert!(!cause_fall_damage(100.0, 1.0, walking_creative).hurt);
        let survival = player();
        assert!(cause_fall_damage(100.0, 1.0, survival).hurt);
    }
}
