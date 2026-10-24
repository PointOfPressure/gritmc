//! Vanilla's math helpers, ported where worldgen depends on their exact
//! shape.
//!
//! Reference: `net.minecraft.util.Mth`.
//!
//! These look trivial enough to "obviously" reimplement, which is the trap:
//! the *association order* of the lerps is observable in the last bits of
//! every noise sample, and floating-point addition is not associative. Every
//! function here is written in vanilla's order and must stay that way — no
//! algebraic simplification, ever (ORACLE.md §5).

/// `Mth#floor(double)` — `(int) Math.floor(v)`.
///
/// Java's narrowing cast saturates at the `int` bounds and maps NaN to 0;
/// Rust's `as` conversion has had exactly those semantics since 1.45, so the
/// cast is a faithful port rather than a lucky coincidence.
#[must_use]
pub fn floor(value: f64) -> i32 {
    value.floor() as i32
}

/// `Mth#lfloor(double)` — `(long) Math.floor(v)`.
#[must_use]
pub fn lfloor(value: f64) -> i64 {
    value.floor() as i64
}

/// `Mth#smoothstep` — the quintic 6t⁵−15t⁴+10t³, in Horner form.
#[must_use]
pub fn smoothstep(x: f64) -> f64 {
    x * x * x * (x * (x * 6.0 - 15.0) + 10.0)
}

/// `Mth#smoothstepDerivative`.
#[must_use]
pub fn smoothstep_derivative(x: f64) -> f64 {
    30.0 * x * x * (x - 1.0) * (x - 1.0)
}

/// `Mth#lerp`. Note the form is `p0 + alpha * (p1 - p0)`, **not**
/// `(1-alpha)*p0 + alpha*p1` — they differ in the last bits.
#[must_use]
pub fn lerp(alpha: f64, p0: f64, p1: f64) -> f64 {
    p0 + alpha * (p1 - p0)
}

/// `Mth#lerp2` — bilinear, as nested [`lerp`]s in vanilla's order.
#[must_use]
pub fn lerp2(alpha1: f64, alpha2: f64, x00: f64, x10: f64, x01: f64, x11: f64) -> f64 {
    lerp(alpha2, lerp(alpha1, x00, x10), lerp(alpha1, x01, x11))
}

/// `Mth#lerp3` — trilinear, as nested [`lerp2`]s in vanilla's order.
#[allow(clippy::too_many_arguments)] // Vanilla's signature; renaming the shape would obscure the port.
#[must_use]
pub fn lerp3(
    alpha1: f64,
    alpha2: f64,
    alpha3: f64,
    x000: f64,
    x100: f64,
    x010: f64,
    x110: f64,
    x001: f64,
    x101: f64,
    x011: f64,
    x111: f64,
) -> f64 {
    lerp(
        alpha3,
        lerp2(alpha1, alpha2, x000, x100, x010, x110),
        lerp2(alpha1, alpha2, x001, x101, x011, x111),
    )
}

/// `Mth#clamp(double, double, double)` — `value < min ? min : min(value, max)`.
///
/// Not `f64::clamp`: that panics when `min > max` and orders the comparisons
/// differently around NaN. The inner comparison is [`java_min`] for the same
/// reason — `f64::min` returns the non-NaN operand where Java propagates it.
#[must_use]
pub fn clamp(value: f64, min: f64, max: f64) -> f64 {
    if value < min {
        min
    } else {
        java_min(value, max)
    }
}

/// `Mth#clamp(float, float, float)`.
///
/// The **f32** overload. `Mth` has both and they are not interchangeable:
/// `getChanceOfStalagmiteOrStalactite` computes a chance in single precision
/// and only then widens it to a double to compare against `nextDouble()`, so a
/// port that stayed in `f64` throughout would land on a slightly different
/// number and flip the comparison for the values in between.
///
/// The inner comparison is [`java_min_f32`], not `f32::min`: the two disagree
/// on NaN, and `Mth.clamp` is a call site where a NaN arrives from a division
/// rather than being typed in. `IntProvider::ClampedNormal` had this right
/// inline and the shared helper did not — the wrong way round for a helper.
#[must_use]
pub fn clamp_f32(value: f32, min: f32, max: f32) -> f32 {
    if value < min {
        min
    } else {
        java_min_f32(value, max)
    }
}

/// `Mth#inverseLerp(float, float, float)`.
#[must_use]
pub fn inverse_lerp_f32(value: f32, min: f32, max: f32) -> f32 {
    (value - min) / (max - min)
}

/// `Mth#clampedLerp(float, float, float)`.
#[must_use]
pub fn clamped_lerp_f32(factor: f32, min: f32, max: f32) -> f32 {
    if factor < 0.0 {
        min
    } else if factor > 1.0 {
        max
    } else {
        lerp_f32(factor, min, max)
    }
}

/// `Mth#clampedMap(float, float, float, float, float)`.
#[must_use]
pub fn clamped_map_f32(value: f32, from_min: f32, from_max: f32, to_min: f32, to_max: f32) -> f32 {
    clamped_lerp_f32(inverse_lerp_f32(value, from_min, from_max), to_min, to_max)
}

/// `Mth#normal` — a mean plus a gaussian scaled by a deviation.
///
/// The cast is where it is on purpose: `nextGaussian` returns a **double** and
/// the reference narrows it to `float` *before* multiplying by the deviation,
/// so the whole expression is single precision. Keeping it in `f64` and
/// rounding at the end gives a value that is very nearly right, which is the
/// worst kind.
pub fn normal<R: crate::rng::RandomSource + ?Sized>(
    random: &mut R,
    mean: f32,
    deviation: f32,
) -> f32 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "`(float)random.nextGaussian()` — the narrowing is the reference's"
    )]
    let gaussian = random.next_gaussian() as f32;
    NORMAL_CALLS.with(|c| c.set(c.get() + 1));
    mean + gaussian * deviation
}

thread_local! {
    /// How many times [`normal`] has drawn a gaussian.
    ///
    /// The parity is the interesting part. `MarsagliaPolarGaussian` computes
    /// two at a time and returns the second on the next call, so after an odd
    /// number of calls a value is cached and the next one costs no underlying
    /// draw. Two runs can therefore agree on every draw *count* and still
    /// disagree on a gaussian's value if they entered a feature with the
    /// cache in different phases — which is the only mechanism left for the
    /// sulfur divergence, every piece of its arithmetic having been read
    /// against the bytecode and matched.
    static NORMAL_CALLS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Gaussian draws taken through [`normal`] so far on this thread.
#[must_use]
pub fn normal_calls() -> u64 {
    NORMAL_CALLS.with(std::cell::Cell::get)
}

/// `Math.min(double, double)` — **not** `f64::min`.
///
/// The two disagree exactly where it matters: Java propagates NaN, Rust
/// returns the non-NaN operand. Java also guarantees `min(-0.0, 0.0)` is
/// `-0.0`, which Rust leaves unspecified. Density functions run `min`/`max`
/// over noise that can produce either, so this is a correctness shim, not
/// pedantry.
#[must_use]
pub fn java_min(a: f64, b: f64) -> f64 {
    if a.is_nan() {
        return a;
    }
    if a == 0.0 && b == 0.0 && b.is_sign_negative() {
        return b;
    }
    if a <= b {
        a
    } else {
        b
    }
}

/// `Math.max(double, double)`. See [`java_min`].
#[must_use]
pub fn java_max(a: f64, b: f64) -> f64 {
    if a.is_nan() {
        return a;
    }
    if a == 0.0 && b == 0.0 && a.is_sign_negative() {
        return b;
    }
    if a >= b {
        a
    } else {
        b
    }
}

/// `Math.min(float, float)`. See [`java_min`] — same NaN/-0.0 rules.
#[must_use]
pub fn java_min_f32(a: f32, b: f32) -> f32 {
    if a.is_nan() {
        return a;
    }
    if a == 0.0 && b == 0.0 && b.is_sign_negative() {
        return b;
    }
    if a <= b {
        a
    } else {
        b
    }
}

/// `Math.max(float, float)`. See [`java_max`].
#[must_use]
pub fn java_max_f32(a: f32, b: f32) -> f32 {
    if a.is_nan() {
        return a;
    }
    if a == 0.0 && b == 0.0 && a.is_sign_negative() {
        return b;
    }
    if a >= b {
        a
    } else {
        b
    }
}

/// `Mth#lerp(float, float, float)`.
///
/// The **f32** overload, and the distinction matters: splines evaluate
/// entirely in single precision even though every layer beneath them is
/// `f64`. Computing a spline in `f64` and narrowing at the end gives a
/// different answer.
#[must_use]
pub fn lerp_f32(alpha: f32, p0: f32, p1: f32) -> f32 {
    p0 + alpha * (p1 - p0)
}

/// `Mth#binarySearch` — the index of the first element for which
/// `is_target_before_or_at` holds, or `max` if none does.
///
/// Ported rather than delegated to `slice::partition_point` because the
/// midpoint arithmetic decides which element a NaN or a boundary-equal
/// input lands on, and that is observable in spline output.
#[must_use]
pub fn binary_search(min: i32, max: i32, is_target_before_or_at: impl Fn(i32) -> bool) -> i32 {
    let mut min = min;
    let mut remaining = max - min;
    while remaining > 0 {
        let half = remaining / 2;
        let mid = min + half;
        if is_target_before_or_at(mid) {
            remaining = half;
        } else {
            min = mid + 1;
            remaining -= half + 1;
        }
    }
    min
}

/// `Mth#inverseLerp`.
#[must_use]
pub fn inverse_lerp(value: f64, min: f64, max: f64) -> f64 {
    (value - min) / (max - min)
}

/// `Mth#clampedMap`.
#[must_use]
pub fn clamped_map(value: f64, from_min: f64, from_max: f64, to_min: f64, to_max: f64) -> f64 {
    clamped_lerp(inverse_lerp(value, from_min, from_max), to_min, to_max)
}

/// `Mth#map` — [`clamped_map`] without the clamp, so it extrapolates past
/// both ends of the source range. The aquifer relies on that: its
/// floodedness thresholds are read off a `map` whose input is already known
/// to be in range, and clamping would silently flatten the ends.
#[must_use]
pub fn map(value: f64, from_min: f64, from_max: f64, to_min: f64, to_max: f64) -> f64 {
    lerp(inverse_lerp(value, from_min, from_max), to_min, to_max)
}

/// `Mth#quantize` — round `value` down to a multiple of `resolution`.
///
/// Floors rather than truncates, so it steps *down* on both sides of zero;
/// truncation would make the step around the origin twice as wide.
#[must_use]
pub fn quantize(value: f64, resolution: i32) -> i32 {
    floor(value / f64::from(resolution)).wrapping_mul(resolution)
}

/// `Mth#clampedLerp`.
#[must_use]
pub fn clamped_lerp(factor: f64, min: f64, max: f64) -> f64 {
    if factor < 0.0 {
        min
    } else if factor > 1.0 {
        max
    } else {
        lerp(factor, min, max)
    }
}

/// `Mth#lengthSquared(double, double, double)`.
#[must_use]
pub fn length_squared(x: f64, y: f64, z: f64) -> f64 {
    x * x + y * y + z * z
}

/// `Mth#length(double, double, double)` — the true square root of
/// [`length_squared`], and deliberately not [`fast_inv_sqrt`]'s reciprocal.
/// Vanilla uses both, a few lines apart in `Beardifier`, and they are
/// different numbers.
#[must_use]
pub fn length(x: f64, y: f64, z: f64) -> f64 {
    length_squared(x, y, z).sqrt()
}

/// `Mth#fastInvSqrt` — the Quake reciprocal-square-root, in doubles, with one
/// Newton step.
///
/// # This is not an optimisation and must not be "fixed"
///
/// `feature/geode.rs` carries the opposite helper — `Mth#invSqrt`, a plain
/// `1.0 / x.sqrt()` — with a note that "a fast inverse square root here would
/// be a different function and this port forbids fast math". Both notes are
/// right, because vanilla calls **both functions** and they disagree by
/// roughly a part in a thousand.
///
/// `Beardifier.getBeardContribution` calls this one, at offset 72. Replacing
/// it with `1.0 / x.sqrt()` would leave every beard value slightly wrong and
/// every gate that reads decimals would still pass — which is why the golden
/// compares raw bits.
///
/// The magic constant and the shift are read off the bytecode:
/// `6910469410427058090L - (bits >> 1)`, an **arithmetic** shift, matching
/// the JVM's `lshr` on a long.
#[must_use]
pub fn fast_inv_sqrt(value: f64) -> f64 {
    let half = 0.5 * value;
    #[allow(clippy::cast_possible_wrap)]
    let bits = value.to_bits() as i64;
    let bits = 6_910_469_410_427_058_090_i64 - (bits >> 1);
    #[allow(clippy::cast_sign_loss)]
    let approximation = f64::from_bits(bits as u64);
    approximation * (1.5 - half * approximation * approximation)
}

/// `Mth.SIN` — the 65,536-entry sine lookup table.
///
/// Vanilla does **not** call `Math.sin` at runtime. It builds this table once
/// and indexes it, so every carver tunnel, every ellipsoid radius and every
/// rotation in the game is a table lookup rather than a transcendental. A
/// port that calls `f64::sin` produces a *different, smoother* world: the
/// table quantises the input to one of 65,536 angles before doing anything.
///
/// # Built here, proved against the JVM
///
/// The entries are `(float) Math.sin(i / 10430.378350470453)`. `Math.sin` is
/// only specified to within 1 ulp, so computing the table here rather than
/// committing 256 KB of it is a bet — one that `golden_mth_sin` settles by
/// comparing a SHA-256 of the whole table against the JVM's. The bet is a
/// good one because the double result is then narrowed to `f32`, discarding
/// 29 bits: a 1-ulp disagreement only survives if the exact value sits within
/// one double-ulp of a float rounding boundary. If the hash ever fails, the
/// fix is to commit the differing indices, not to widen a tolerance.
static SIN_TABLE: std::sync::LazyLock<Box<[f32; SIN_QUANTIZATION]>> =
    std::sync::LazyLock::new(|| {
        let mut table = Box::new([0.0f32; SIN_QUANTIZATION]);
        for (i, slot) in table.iter_mut().enumerate() {
            #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
            {
                *slot = (i as f64 / SIN_SCALE).sin() as f32;
            }
        }
        table
    });

const SIN_QUANTIZATION: usize = 65_536;
const SIN_MASK: i64 = 65_535;
const COS_OFFSET: f64 = 16_384.0;
/// `65536 / (2 * PI)`, written as vanilla writes it. Recomputing it from
/// `PI` gives a different last bit and therefore a different table index at
/// the boundaries.
const SIN_SCALE: f64 = 10_430.378_350_470_453;

/// `Mth#sin`.
///
/// **Takes a `f64` in 26.2** — it took a `float` in earlier versions, so a
/// port written against an older decompile computes `i * SIN_SCALE` in the
/// wrong width at every call site. Every carver call passes a `float`
/// expression, which Java widens at the call.
#[must_use]
pub fn sin(i: f64) -> f32 {
    SIN_TABLE[table_index(i * SIN_SCALE)]
}

/// `Mth#cos` — the same table, offset a quarter turn.
#[must_use]
pub fn cos(i: f64) -> f32 {
    SIN_TABLE[table_index(i * SIN_SCALE + COS_OFFSET)]
}

/// `(int) ((long) v & 65535L)`.
///
/// The narrowing to `long` truncates toward zero and saturates, which is why
/// a negative angle does not index out of bounds: `-1.5` becomes `-1`, and
/// `-1 & 65535` is `65535`. Doing the mask in `i32` instead would be the same
/// answer here but not for angles past 2^31, and rotations accumulate.
fn table_index(scaled: f64) -> usize {
    #[allow(clippy::cast_possible_truncation)]
    let truncated = scaled as i64;
    (truncated & SIN_MASK) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_is_a_table_lookup_not_a_transcendental() {
        // The observable consequence: inputs that differ by less than one
        // table step give *identical* results. A port calling f64::sin would
        // separate these, and every carver tunnel would drift.
        let step = 1.0 / SIN_SCALE;
        assert_eq!(sin(0.3), sin(0.3 + step * 0.4));
        assert_ne!(sin(0.3), sin(0.3 + step * 1.6));
    }

    #[test]
    fn cosine_is_sine_a_quarter_turn_along() {
        // Not `sin(x + PI/2)`: the offset is applied to the *index*, after
        // scaling, so it is exact rather than an angle addition.
        for i in 0..64 {
            let angle = f64::from(i) * 0.1 - 3.2;
            assert_eq!(cos(angle), sin(angle + COS_OFFSET / SIN_SCALE));
        }
    }

    #[test]
    fn a_negative_angle_wraps_instead_of_panicking() {
        // `(long)` truncates toward zero and the mask is on the i64, so
        // negative indices wrap to the top of the table.
        for i in 1..2000 {
            let angle = -f64::from(i) * 0.37;
            let _ = sin(angle);
            let _ = cos(angle);
        }
        assert_eq!(table_index(-1.5), 65_535);
        assert_eq!(table_index(-0.5), 0);
    }

    #[test]
    fn the_table_covers_a_full_turn_and_no_more() {
        // Index 0 is sin(0); the table should come back near zero at the end,
        // having gone through one full period. If SIN_SCALE were wrong this
        // would land somewhere else entirely.
        assert_eq!(SIN_TABLE[0], 0.0);
        assert!(SIN_TABLE[16_384] > 0.999, "quarter turn should be ~1");
        assert!(
            SIN_TABLE[49_152] < -0.999,
            "three-quarter turn should be ~-1"
        );
        assert!(SIN_TABLE[32_768].abs() < 1.0e-4, "half turn should be ~0");
    }

    #[test]
    fn smoothstep_pins_the_endpoints() {
        assert_eq!(smoothstep(0.0), 0.0);
        assert_eq!(smoothstep(1.0), 1.0);
        assert_eq!(smoothstep(0.5), 0.5);
    }

    #[test]
    fn lerp_uses_vanillas_association_order() {
        // `p0 + alpha*(p1 - p0)` and `(1-alpha)*p0 + alpha*p1` are equal in
        // exact arithmetic and not in floating point. Rather than assume a
        // particular triple disagrees, find one — then pin that `lerp`
        // computes vanilla's form and not the other.
        let alphas = [0.1_f64, 0.3, 0.7, 0.9];
        let pairs = [
            (0.1_f64, 0.2_f64),
            (1.0, 3.0),
            (-1.0, 1.0),
            (1.0e16, 1.0e16 + 2.0),
            (1.0e-8, 1.0),
        ];

        let disagreement = alphas.iter().flat_map(|&alpha| {
            pairs.iter().map(move |&(p0, p1)| {
                let vanilla = p0 + alpha * (p1 - p0);
                let other = (1.0 - alpha) * p0 + alpha * p1;
                (alpha, p0, p1, vanilla, other)
            })
        });

        let (alpha, p0, p1, vanilla, other) = disagreement
            .into_iter()
            .find(|(_, _, _, vanilla, other)| vanilla.to_bits() != other.to_bits())
            .expect("no sampled triple distinguishes the two lerp forms");

        assert_eq!(
            lerp(alpha, p0, p1).to_bits(),
            vanilla.to_bits(),
            "lerp({alpha}, {p0}, {p1}) must use vanilla's association order \
             (got the {other} form)"
        );
    }

    #[test]
    fn floor_saturates_like_a_java_narrowing_cast() {
        assert_eq!(floor(-0.5), -1);
        assert_eq!(floor(0.5), 0);
        assert_eq!(floor(f64::NAN), 0);
        assert_eq!(floor(f64::INFINITY), i32::MAX);
        assert_eq!(floor(f64::NEG_INFINITY), i32::MIN);
    }
}
