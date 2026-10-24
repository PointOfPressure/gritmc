//! `java.lang.Math` and `net.minecraft.util.Mth` primitives, written out.
//!
//! Reference (26.2), read from the pinned jar with `javap -p -c -constants`:
//! `net.minecraft.util.Mth.clamp(int,int,int)` and `clamp(float,float,float)`,
//! and the JLS definitions of `Math.min`/`Math.max` that the `invokestatic`s
//! beside them reach.
//!
//! # Why these are not the Rust standard library's
//!
//! Three differences, all reachable from the player systems that call these:
//!
//! * **`f32::clamp` panics when `min > max`.** `Mth.clamp` does not: it is
//!   `value < min ? min : Math.min(value, max)`, so an inverted range returns
//!   `max` and the caller carries on. `CombatRules.getDamageAfterAbsorb`
//!   passes `armor * 0.2` as the low bound and `20.0` as the high one, and an
//!   armour value above 100 inverts them.
//! * **`f32::min` returns the non-NaN operand; `Math.min` propagates NaN.**
//!   `LivingEntity.hurtServer` explicitly turns a NaN damage into
//!   `Float.MAX_VALUE` at offsets 164..181, which is only correct if NaN has
//!   survived everything before it.
//! * **The sign of a zero.** `Math.min(-0.0f, 0.0f)` is `-0.0f` and
//!   `Math.max` is `+0.0f`; Rust leaves the tie unspecified. `FoodData.add`
//!   clamps saturation against a `0.0f` ceiling for a starving player, so the
//!   tie is live.
//!
//! None of the three has been *observed* to change an outcome on this server
//! yet. They are written this way because the cost is a branch and the
//! alternative is a difference nobody would find.

/// `net.minecraft.util.Mth.clamp(float, float, float)`.
///
/// `value < min ? min : Math.min(value, max)` — a `fcmpg` and a `Math.min`,
/// so a NaN `value` takes the second branch and comes back NaN.
#[must_use]
pub fn clamp_f(value: f32, lo: f32, hi: f32) -> f32 {
    if value < lo {
        lo
    } else {
        min_f(value, hi)
    }
}

/// `net.minecraft.util.Mth.clamp(int, int, int)` — `min(max(v, lo), hi)`.
#[must_use]
pub const fn clamp_i(value: i32, lo: i32, hi: i32) -> i32 {
    min_i(max_i(value, lo), hi)
}

/// `java.lang.Math.min(float, float)`.
#[must_use]
pub fn min_f(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        return f32::NAN;
    }
    if a == 0.0 && b == 0.0 {
        return if a.is_sign_negative() { a } else { b };
    }
    if a < b {
        a
    } else {
        b
    }
}

/// `java.lang.Math.max(float, float)`.
#[must_use]
pub fn max_f(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        return f32::NAN;
    }
    if a == 0.0 && b == 0.0 {
        return if a.is_sign_positive() { a } else { b };
    }
    if a > b {
        a
    } else {
        b
    }
}

/// `java.lang.Math.min(int, int)`.
#[must_use]
pub const fn min_i(a: i32, b: i32) -> i32 {
    if a <= b {
        a
    } else {
        b
    }
}

/// `java.lang.Math.max(int, int)`.
#[must_use]
pub const fn max_i(a: i32, b: i32) -> i32 {
    if a >= b {
        a
    } else {
        b
    }
}

/// `java.lang.Math.round(float)` -> `int`.
///
/// **Not `(f + 0.5).floor() as i32`.** That formulation is the one every
/// reimplementation reaches for and it is wrong twice over: it rounds
/// `0x1.fffffep-2` (the float immediately below 0.5) *up* to 1, because the
/// addition itself rounds to exactly 1.0f, and it answers `i32::MIN` rather
/// than 0 for a NaN. Java's own implementation, since 8, is the bit
/// manipulation below, which never adds anything.
///
/// The reason it matters here is `ServerPlayer.checkMovementStatistics`:
/// every one of its six arms is `Math.round(distance * 100.0f)`, the result
/// is tested `> 0`, and the exhaustion charged is proportional to it. A
/// player moving `0.00499` of a block per tick is charged nothing by vanilla
/// and one unit of movement by the naive version, every tick, forever.
///
/// Transcribed from `java.lang.Math.round(float)`, whose constants are
/// `FloatConsts.SIGNIFICAND_WIDTH = 24`, `EXP_BIAS = 127`,
/// `EXP_BIT_MASK = 0x7F80_0000` and `SIGNIF_BIT_MASK = 0x007F_FFFF`.
#[must_use]
pub fn round_f(a: f32) -> i32 {
    const SIGNIFICAND_WIDTH: i32 = 24;
    const EXP_BIAS: i32 = 127;
    const EXP_BIT_MASK: u32 = 0x7F80_0000;
    const SIGNIF_BIT_MASK: u32 = 0x007F_FFFF;

    let int_bits = a.to_bits();
    #[expect(
        clippy::cast_possible_wrap,
        reason = "the exponent field is 8 bits, so the shift result is 0..=255"
    )]
    let biased_exp = ((int_bits & EXP_BIT_MASK) >> (SIGNIFICAND_WIDTH - 1)) as i32;
    let shift = (SIGNIFICAND_WIDTH - 2 + EXP_BIAS) - biased_exp;
    // `(shift & -32) == 0` is Java's own way of writing `0 <= shift < 32`.
    if shift & -32 == 0 {
        #[expect(
            clippy::cast_possible_wrap,
            reason = "the mask leaves 24 bits, so this cannot reach the sign bit"
        )]
        let mut r = ((int_bits & SIGNIF_BIT_MASK) | (SIGNIF_BIT_MASK + 1)) as i32;
        // `intBits < 0` — the raw bit pattern read as a signed int, which is
        // the sign bit and nothing else.
        #[expect(clippy::cast_possible_wrap, reason = "Java reads the same bits signed")]
        if (int_bits as i32) < 0 {
            r = -r;
        }
        ((r >> shift) + 1) >> 1
    } else {
        // Java's `(int) a`, which saturates and sends NaN to 0.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the JVM's f2i, whose saturation Rust's `as` matches exactly"
        )]
        {
            a as i32
        }
    }
}

/// `java.lang.Math.round(double)` -> `long`, the same construction one width up.
///
/// `DoubleConsts.SIGNIFICAND_WIDTH = 53`, `EXP_BIAS = 1023`,
/// `EXP_BIT_MASK = 0x7FF0_0000_0000_0000`,
/// `SIGNIF_BIT_MASK = 0x000F_FFFF_FFFF_FFFF`. Reached by
/// `checkMovementStatistics`' climb arm (`Math.round(dy * 100.0)`), which is
/// the one arm of the six that is a `double` round and an `l2i` rather than a
/// `float` one.
#[must_use]
pub fn round_d(a: f64) -> i64 {
    const SIGNIFICAND_WIDTH: i64 = 53;
    const EXP_BIAS: i64 = 1023;
    const EXP_BIT_MASK: u64 = 0x7FF0_0000_0000_0000;
    const SIGNIF_BIT_MASK: u64 = 0x000F_FFFF_FFFF_FFFF;

    let long_bits = a.to_bits();
    #[expect(
        clippy::cast_possible_wrap,
        reason = "the exponent field is 11 bits, so the shift result is 0..=2047"
    )]
    let biased_exp = ((long_bits & EXP_BIT_MASK) >> (SIGNIFICAND_WIDTH - 1)) as i64;
    let shift = (SIGNIFICAND_WIDTH - 2 + EXP_BIAS) - biased_exp;
    if shift & -64 == 0 {
        #[expect(
            clippy::cast_possible_wrap,
            reason = "the mask leaves 53 bits, so this cannot reach the sign bit"
        )]
        let mut r = ((long_bits & SIGNIF_BIT_MASK) | (SIGNIF_BIT_MASK + 1)) as i64;
        #[expect(clippy::cast_possible_wrap, reason = "Java reads the same bits signed")]
        if (long_bits as i64) < 0 {
            r = -r;
        }
        ((r >> shift) + 1) >> 1
    } else {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the JVM's d2l, whose saturation Rust's `as` matches exactly"
        )]
        {
            a as i64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{clamp_f, clamp_i, max_f, max_i, min_f, min_i, round_d, round_f};

    #[test]
    fn clamp_f_on_an_inverted_range_returns_the_high_bound_rather_than_panicking() {
        // `f32::clamp(5.0, 20.0, 3.0)` panics; Mth.clamp answers 20.0 because
        // the `value < min` test fires first.
        assert!((clamp_f(5.0, 20.0, 3.0) - 20.0).abs() < f32::EPSILON);
    }

    #[test]
    fn clamp_f_passes_nan_through_the_min_branch() {
        assert!(clamp_f(f32::NAN, 0.0, 1.0).is_nan());
    }

    #[test]
    fn min_and_max_propagate_nan_where_the_std_library_does_not() {
        assert!(min_f(f32::NAN, 1.0).is_nan());
        assert!(max_f(1.0, f32::NAN).is_nan());
        // The difference this exists for:
        assert!(!f32::min(f32::NAN, 1.0).is_nan());
    }

    #[test]
    fn the_sign_of_a_zero_tie_follows_java() {
        assert!(min_f(-0.0, 0.0).is_sign_negative());
        assert!(min_f(0.0, -0.0).is_sign_negative());
        assert!(max_f(-0.0, 0.0).is_sign_positive());
        assert!(max_f(0.0, -0.0).is_sign_positive());
    }

    #[test]
    fn the_ordinary_cases_agree_with_the_std_library() {
        for (a, b) in [(1.0_f32, 2.0_f32), (-3.5, 0.5), (7.0, 7.0)] {
            assert!((min_f(a, b) - a.min(b)).abs() < f32::EPSILON);
            assert!((max_f(a, b) - a.max(b)).abs() < f32::EPSILON);
        }
        assert_eq!(min_i(3, 9), 3);
        assert_eq!(max_i(3, 9), 9);
        assert_eq!(clamp_i(30, 0, 20), 20);
        assert_eq!(clamp_i(-1, 0, 20), 0);
        assert_eq!(clamp_i(7, 0, 20), 7);
    }

    #[test]
    fn clamp_i_on_an_inverted_range_takes_the_high_bound() {
        // min(max(5, 20), 3) = min(20, 3) = 3. Different from the float
        // version's answer on the same arguments, and both are vanilla's.
        assert_eq!(clamp_i(5, 20, 3), 3);
    }

    #[test]
    fn round_f_agrees_with_the_jvm_on_the_half_boundary_and_the_float_below_it() {
        // The three cells the naive `(f + 0.5).floor()` gets wrong.
        //
        // `0x1.fffffep-2` is the float immediately below 0.5. Adding 0.5f to
        // it rounds to exactly 1.0f, so the naive form answers 1; Java
        // answers 0.
        let just_under_half = f32::from_bits(0x3EFF_FFFF);
        assert!(just_under_half < 0.5);
        assert_eq!(round_f(just_under_half), 0);
        assert_eq!((just_under_half + 0.5).floor() as i32, 1, "the trap itself");

        assert_eq!(round_f(0.5), 1);
        assert_eq!(round_f(-0.5), 0, "half rounds toward positive infinity");
        assert_eq!(round_f(f32::NAN), 0);
    }

    #[test]
    fn round_f_matches_the_naive_form_everywhere_it_is_allowed_to() {
        // The positive control for the test above: on ordinary values the two
        // agree, so the three cells there are a real difference and not a
        // broken function.
        for i in -2000..2000 {
            let v = i as f32 / 7.0;
            assert_eq!(
                round_f(v),
                (v + 0.5).floor() as i32,
                "disagreed at {v}, which is not one of the three known cells"
            );
        }
    }

    #[test]
    fn round_f_is_what_the_movement_stat_arms_ask_for() {
        // `Math.round(distance * 100.0f) > 0` is the gate on every exhaustion
        // charge in `checkMovementStatistics`. A player who moved 0.00499 of
        // a block is under it and one who moved 0.005 is on it.
        assert_eq!(round_f(0.004_99_f32 * 100.0), 0);
        assert_eq!(round_f(0.005_f32 * 100.0), 1);
        assert_eq!(round_f(0.215_f32 * 100.0), 22);
    }

    #[test]
    fn round_d_agrees_with_java_on_the_boundary_and_on_nan() {
        assert_eq!(round_d(0.5), 1);
        assert_eq!(round_d(-0.5), 0);
        assert_eq!(round_d(f64::NAN), 0);
        assert_eq!(round_d(2.499_999_999_999_999_6), 2);
        for i in -2000..2000 {
            let v = f64::from(i) / 7.0;
            assert_eq!(round_d(v), (v + 0.5).floor() as i64, "disagreed at {v}");
        }
    }
}
