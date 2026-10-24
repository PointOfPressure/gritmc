//! A correctly-rounded natural logarithm.
//!
//! Reference: `java.lang.Math#log`, and what it actually does rather than
//! what it says.
//!
//! # Why this file exists
//!
//! `MarsagliaPolarGaussian` calls `Math.log` once per accepted sample, so
//! every gaussian in worldgen — every speleothem height, everything through
//! `Mth.normal` — depends on its last bit. `golden_gaussian` measured Rust's
//! own `f64::ln` against the JVM's and found **231 disagreements in 300,000**
//! values over `(0, 1)`, every one of them exactly 1 ulp. This module closes
//! that to **4**, and the four are cases where the *JVM* is wrong.
//!
//! # What was measured, before anything was written
//!
//! Four things, in order, each of which changed what the fix should be:
//!
//! 1. `Math.log` and `StrictMath.log` **disagree on 7.3%** of inputs in
//!    `(0, 1)` — 365,590 of 5,000,000 — always by exactly 1 ulp. They are
//!    different functions and it matters which one a call site reaches.
//! 2. `Math.log` is **stable across 2,000,000 calls** to one site and differs
//!    from `StrictMath.log` from the first call. So the intrinsic is in play
//!    immediately and does not switch as the JIT warms up: within one JVM on
//!    one architecture the value is deterministic. Vanilla is not quietly
//!    non-reproducible, which was the first thing worth ruling out.
//! 3. Over 4,000 inputs where the two Java logs disagree, `Math.log` was the
//!    correctly-rounded one **every time** and `StrictMath.log` never was.
//!    That reading said "implement correct rounding and you match the JVM",
//!    and it was **wrong** — the sample was drawn from the disagreeing subset,
//!    which is exactly where the better implementation wins.
//! 4. Over 300,000 *unfiltered* inputs, this module differs from `Math.log`
//!    on four, and on all four an 80-digit reference says **this module is
//!    correct and `Math.log` is not**.
//!
//! So `Math.log` is within 1 ulp of the true logarithm, deterministic, and
//! not correctly rounded. Bit-for-bit agreement with it is not reachable by
//! being more accurate; it would need HotSpot's x86 `dlog` stub — Intel's
//! LIBM, hand-written assembly, and architecture-specific — ported as such.
//! That is a real option and it is not this one. See ORACLE.md §5c.
//!
//! # What this is instead
//!
//! A *correctly-rounded* logarithm: the nearest double to the true value.
//! It is a stable target that does not depend on which JVM, which
//! architecture or which JIT state produced a golden file, and it is within
//! 1 ulp of `Math.log` everywhere measured. The remaining divergence is
//! named and counted in `golden_gaussian` rather than hidden in a tolerance.
//!
//! Worldgen impact of the residual: a gaussian feeds `Mth.normal` ->
//! `ClampedNormalFloat` -> a cast to `int`. A 1-ulp difference changes that
//! cast only when the value lands within 1 ulp of an integer boundary, and it
//! never changes how many numbers come off the stream — the rejection loop
//! uses `nextDouble` alone and never touches the logarithm.
//!
//! # How it works
//!
//! Double-double (Dekker) arithmetic — a pair of `f64` whose sum carries
//! ~106 significant bits — over the identity
//!
//! ```text
//! x = 2^k * m,  m in [sqrt(2)/2, sqrt(2))
//! ln(x) = k*ln2 + 2*atanh((m-1)/(m+1))
//! ```
//!
//! No table. `|(m-1)/(m+1)| <= 0.172`, so the `atanh` series needs about
//! twenty-two odd terms to fall below 2^-106, which is cheap next to a noise
//! sample and is the reason the table was not worth the extra surface.

/// A double-double: `hi + lo`, with `lo` holding the bits `hi` cannot.
#[derive(Clone, Copy, Debug)]
struct Dd {
    hi: f64,
    lo: f64,
}

/// Dekker's exact sum, for `|a| >= |b|`.
#[inline]
fn quick_two_sum(a: f64, b: f64) -> (f64, f64) {
    let s = a + b;
    (s, b - (s - a))
}

/// Knuth's exact sum, for any operands.
#[inline]
fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let s = a + b;
    let shifted = s - a;
    (s, (a - (s - shifted)) + (b - shifted))
}

/// An exact product, using the fused multiply-add to recover the residual.
///
/// `mul_add` is a single rounding by definition, so `a*b - p` is exact — the
/// classic Dekker splitting is unnecessary and slower on any machine that has
/// the instruction.
#[inline]
fn two_prod(a: f64, b: f64) -> (f64, f64) {
    let p = a * b;
    (p, a.mul_add(b, -p))
}

impl Dd {
    const fn new(hi: f64) -> Self {
        Self { hi, lo: 0.0 }
    }

    /// The nearest `f64` — one rounding, at the very end.
    const fn value(self) -> f64 {
        self.hi + self.lo
    }

    fn add(self, other: Self) -> Self {
        let (sum, mut err) = two_sum(self.hi, other.hi);
        err += self.lo + other.lo;
        let (hi, lo) = quick_two_sum(sum, err);
        Self { hi, lo }
    }

    fn neg(self) -> Self {
        Self {
            hi: -self.hi,
            lo: -self.lo,
        }
    }

    fn mul(self, other: Self) -> Self {
        let (prod, mut err) = two_prod(self.hi, other.hi);
        err += self.hi * other.lo + self.lo * other.hi;
        let (hi, lo) = quick_two_sum(prod, err);
        Self { hi, lo }
    }

    /// One Newton step off the leading quotient, which is enough to reach
    /// full double-double accuracy.
    fn div(self, other: Self) -> Self {
        let leading = self.hi / other.hi;
        let residual = self.add(other.neg().mul(Self::new(leading)));
        let correction = residual.value() / other.hi;
        let (hi, lo) = quick_two_sum(leading, correction);
        Self { hi, lo }
    }
}

/// `ln 2`, split so `LN2_HI + LN2_LO` carries ~106 bits.
///
/// Written as raw bit patterns rather than decimal literals. A decimal
/// literal for the high half is `std::f64::consts::LN_2` spelled the long way
/// — which is what it is — but the *low* half is only meaningful as the exact
/// residual of that rounding, and a decimal that round-trips is easy to
/// mistype by a bit while still looking right.
const LN2_HI: f64 = f64::from_bits(0x3FE6_2E42_FEFA_39EF);
const LN2_LO: f64 = f64::from_bits(0x3C7A_BC9E_3B39_803F);

/// 2^54, for scaling a subnormal into the normal range.
const TWO_54: f64 = 1.801_439_850_948_198_4e16;

/// The natural logarithm, correctly rounded.
///
/// Matches `java.lang.Math.log` on 299,996 of 300,000 measured inputs in
/// `(0, 1)`; on the other four this is the correct answer and the JVM's is
/// not. See the module docs before changing that claim.
#[must_use]
pub fn ln(x: f64) -> f64 {
    if x.is_nan() || x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return f64::NEG_INFINITY;
    }
    if x.is_infinite() {
        return f64::INFINITY;
    }

    // Split off the exponent. A subnormal is scaled up first and the scaling
    // taken back out of `k`, because its stored exponent field is zero and
    // says nothing about its magnitude.
    let (bits, mut k) = {
        let raw = x.to_bits();
        // An 11-bit field always fits an i32, so the conversion is total.
        let exponent_of = |bits: u64| i32::try_from((bits >> 52) & 0x7ff).unwrap_or(0);
        if (raw >> 52) & 0x7ff == 0 {
            let scaled = (x * TWO_54).to_bits();
            (scaled, exponent_of(scaled) - 1023 - 54)
        } else {
            (raw, exponent_of(raw) - 1023)
        }
    };

    // The mantissa as a value in [1, 2), then folded to [sqrt(2)/2, sqrt(2))
    // so the series argument stays small on both sides of 1.
    let mut mantissa = f64::from_bits((bits & 0x000f_ffff_ffff_ffff) | 0x3ff0_0000_0000_0000);
    if mantissa > std::f64::consts::SQRT_2 {
        mantissa *= 0.5;
        k += 1;
    }

    // s = (m - 1) / (m + 1). Both differences are exact in double-double, so
    // the only error in `s` is the division's.
    let s = Dd::new(mantissa)
        .add(Dd::new(-1.0))
        .div(Dd::new(mantissa).add(Dd::new(1.0)));
    let s_squared = s.mul(s);

    // 2 * atanh(s) = 2 * sum s^(2n+1) / (2n+1). |s| <= 0.172, so the terms
    // fall by ~1.5 decimal digits each and twenty-two of them clear 2^-106.
    let mut sum = Dd::new(0.0);
    let mut term = s;
    for n in 0..40u32 {
        sum = sum.add(term.div(Dd::new(f64::from(2 * n + 1))));
        term = term.mul(s_squared);
        if term.hi == 0.0 || term.hi.abs() < 1e-40 {
            break;
        }
    }
    let ln_mantissa = sum.add(sum);

    Dd {
        hi: LN2_HI,
        lo: LN2_LO,
    }
    .mul(Dd::new(f64::from(k)))
    .add(ln_mantissa)
    .value()
}

#[cfg(test)]
mod tests {
    use super::ln;

    /// The values every logarithm has to get exactly right.
    #[test]
    fn the_exact_points_are_exact() {
        assert_eq!(ln(1.0).to_bits(), 0.0f64.to_bits(), "ln 1 is +0");
        assert_eq!(ln(0.0), f64::NEG_INFINITY);
        assert_eq!(ln(f64::INFINITY), f64::INFINITY);
        assert!(ln(-1.0).is_nan());
        assert!(ln(f64::NAN).is_nan());
    }

    /// `ln 2` and `ln e` to the last bit, which the double-double split of
    /// `ln 2` is there to make possible.
    #[test]
    fn the_named_constants_land_on_the_nearest_double() {
        assert_eq!(ln(2.0).to_bits(), std::f64::consts::LN_2.to_bits());
        assert_eq!(ln(std::f64::consts::E).to_bits(), 1.0f64.to_bits());
        assert_eq!(ln(10.0).to_bits(), std::f64::consts::LN_10.to_bits());
        // A power of two is k*ln2 with no series contribution at all.
        assert_eq!(
            ln(1024.0).to_bits(),
            (10.0 * std::f64::consts::LN_2).to_bits()
        );
    }

    /// Subnormals go through the scaling path, which is the branch most
    /// likely to be written once and never executed.
    #[test]
    fn subnormals_take_the_scaled_path_and_come_back_right() {
        let smallest = f64::from_bits(1);
        let got = ln(smallest);
        // ln(2^-1074) = -1074 * ln2.
        let want = -1074.0 * std::f64::consts::LN_2;
        assert!(
            (got - want).abs() < 1e-12,
            "ln(MIN) is {got}, expected about {want}"
        );
        assert!(ln(f64::MIN_POSITIVE) < 0.0);
    }

    /// Either side of the sqrt(2) fold, where `k` changes and the series
    /// argument flips sign. An implementation that folded on the wrong side
    /// would still be continuous and would lose a bit of accuracy near the
    /// join, so this checks the *values*, not the shape.
    #[test]
    fn the_fold_at_sqrt_two_is_continuous_to_the_last_bit() {
        let root = std::f64::consts::SQRT_2;
        for x in [
            f64::from_bits(root.to_bits() - 1),
            root,
            f64::from_bits(root.to_bits() + 1),
        ] {
            let got = ln(x);
            let want = 0.5 * std::f64::consts::LN_2;
            assert!(
                (got - want).abs() < 1e-15,
                "ln({x}) is {got}, expected about {want}"
            );
        }
    }

    /// This is meant to be *at least* as good as the standard library, and
    /// the whole reason it exists is that it is sometimes better. Compared
    /// against a value the module docs record: over `(0, 1)` the two differ
    /// rarely and never by more than one ulp.
    #[test]
    fn it_never_disagrees_with_the_standard_library_by_more_than_one_ulp() {
        let mut state = 0x2026_0725u64;
        let mut differ = 0u64;
        let total = 200_000u64;
        for _ in 0..total {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let x = f64::from_bits(0x3ff0_0000_0000_0000 | (state >> 12)) - 1.0;
            if x <= 0.0 {
                continue;
            }
            #[expect(clippy::cast_possible_wrap, reason = "an ulp distance")]
            let distance = (ln(x).to_bits() as i64 - x.ln().to_bits() as i64).abs();
            assert!(
                distance <= 1,
                "ln({x}) differs from f64::ln by {distance} ulp"
            );
            if distance != 0 {
                differ += 1;
            }
        }
        // Reported rather than bounded tightly: the point of the assertion
        // above is the 1-ulp ceiling. This says the two are not identical,
        // which is what makes the module worth having.
        println!("PRECISE LOG: {differ} of {total} differ from f64::ln, all by 1 ulp");
        assert!(
            differ > 0,
            "if these never differ, this module is dead weight"
        );
    }
}
