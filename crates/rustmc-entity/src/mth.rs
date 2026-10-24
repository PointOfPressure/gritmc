//! The `net.minecraft.util.Mth` methods the mob AI calls that
//! `rustmc_worldgen::math` does not already carry, from the 26.2 jar.
//!
//! Every function here is one `Mth` method. The ones worldgen already has —
//! `sin`, `cos`, `floor`, `clamp_f32`, `fast_inv_sqrt`, `java_min_f32`,
//! `java_max_f32` — are re-exported rather than written twice.
//!
//! # `atan2`
//!
//! `Mth.atan2` is not `Math.atan2`. It is a table-driven approximation over
//! `fastInvSqrt`, two 257-entry tables and a cubic correction, and every
//! head-turn in the game goes through it, so the bytecode is transcribed
//! offset by offset below rather than replaced by the platform's `atan2`.
//! The tables are built the way `Mth.<clinit>` builds them
//! (offsets 261..319 of the static initialiser):
//!
//! ```text
//! FRAC_BIAS = Double.longBitsToDouble(4805340802404319232L)
//! for i in 0..257:
//!     d          = i / 256.0
//!     asin       = Math.asin(d)
//!     COS_TAB[i] = Math.cos(asin)
//!     ASIN_TAB[i]= asin
//! ```
//!
//! `Math.asin` and `Math.cos` on the JVM are fdlibm; Rust's are the
//! platform libm, and they do not agree: `COS_TAB[61]` came out one ulp
//! apart on the Threadripper (JDK 25 against glibc), measured by
//! `tools/goldengen/vanilla/MobMathGolden.java` the first time it ran. So
//! the tables are the JVM's bits, embedded in [`crate::mth_tables`], and
//! `tests/golden_mob_math.rs` asserts on every run that the embedded
//! tables still equal the vectors and that `atan2` then matches on 2014
//! inputs.

use std::sync::LazyLock;

pub use rustmc_worldgen::math::{
    clamp_f32, cos, fast_inv_sqrt, floor, java_max_f32, java_min_f32, sin,
};

/// `Entity.calculateViewVector(float xRot, float yRot)`, offsets 0..67:
///
/// ```text
/// f = xRot * 0.017453292f; g = -yRot * 0.017453292f
/// h = Mth.cos(g); i = Mth.sin(g); j = Mth.cos(f); k = Mth.sin(f)
/// Vec3(i * j, -k, h * j)         -- each product in float, then widened
/// ```
#[must_use]
pub fn calculate_view_vector(x_rot: f32, y_rot: f32) -> crate::phys::Vec3 {
    let f = x_rot * 0.017_453_292_f32;
    let g = -y_rot * 0.017_453_292_f32;
    let h = cos(f64::from(g));
    let i = sin(f64::from(g));
    let j = cos(f64::from(f));
    let k = sin(f64::from(f));
    crate::phys::Vec3::new(f64::from(i * j), f64::from(-k), f64::from(h * j))
}

/// `Vec3.directionFromRotation(float xRot, float yRot)`, offsets 0..66.
/// **Not the same as `calculate_view_vector`**: the yaw term carries an
/// extra `- 3.1415927f` and the pitch cosine is negated, so
/// `directionFromRotation(0, 0)` is `(0, 0, -1)` where
/// `calculateViewVector(0, 0)` is `(0, 0, 1)`.
///
/// ```text
/// g  = -yRot * 0.017453292f - 3.1415927f
/// f2 = Mth.cos(g);  f3 = Mth.sin(g)          <- the float-returning table lookups
/// h  = -xRot * 0.017453292f
/// f4 = -Mth.cos(h); f5 = Mth.sin(h)
/// Vec3(f3 * f4, f5, f2 * f4)                 <- each product in float, then widened
/// ```
#[must_use]
pub fn direction_from_rotation(x_rot: f32, y_rot: f32) -> crate::phys::Vec3 {
    // `ldc 3.1415927f` at 5, which is `std::f32::consts::PI`.
    let g = -y_rot * 0.017_453_292_f32 - std::f32::consts::PI;
    let f2 = cos(f64::from(g));
    let f3 = sin(f64::from(g));
    let h = -x_rot * 0.017_453_292_f32;
    let f4 = -cos(f64::from(h));
    let f5 = sin(f64::from(h));
    crate::phys::Vec3::new(f64::from(f3 * f4), f64::from(f5), f64::from(f2 * f4))
}

/// `Entity.getInputVector(Vec3 input, float speed, float yRot)`, offsets 0..110:
///
/// ```text
/// d = input.lengthSqr(); if d < 1.0E-7 return ZERO
/// v = (d > 1.0 ? input.normalize() : input).scale((double) speed)
/// s = Mth.sin(yRot * 0.017453292f); c = Mth.cos(yRot * 0.017453292f)
/// Vec3(v.x * c - v.z * s, v.y, v.z * c + v.x * s)
/// ```
#[must_use]
pub fn get_input_vector(input: crate::phys::Vec3, speed: f32, y_rot: f32) -> crate::phys::Vec3 {
    let d = input.length_sqr();
    if d < 1.0E-7 {
        return crate::phys::Vec3::ZERO;
    }
    let v = if d > 1.0 { input.normalize() } else { input }.scale(f64::from(speed));
    let s = f64::from(sin(f64::from(y_rot * 0.017_453_292_f32)));
    let c = f64::from(cos(f64::from(y_rot * 0.017_453_292_f32)));
    crate::phys::Vec3::new(v.x * c - v.z * s, v.y, v.z * c + v.x * s)
}

/// `Mth.rotLerp(float delta, float a, float b)`: `a + delta * wrapDegrees(b - a)`.
#[must_use]
pub fn rot_lerp(delta: f32, a: f32, b: f32) -> f32 {
    a + delta * wrap_degrees(b - a)
}

/// `Mth.wrapDegrees(float)`.
///
/// ```text
/// 0: fload_0; 1: ldc 360.0f; 3: frem; 4: fstore_1
/// 5..16:  if (f >= 180.0f) f -= 360.0f
/// 17..28: if (f < -180.0f) f += 360.0f
/// ```
#[must_use]
pub fn wrap_degrees(v: f32) -> f32 {
    let mut f = v % 360.0;
    if f >= 180.0 {
        f -= 360.0;
    }
    if f < -180.0 {
        f += 360.0;
    }
    f
}

/// `Mth.wrapDegrees(double)`, the same shape at double width.
#[must_use]
pub fn wrap_degrees_f64(v: f64) -> f64 {
    let mut d = v % 360.0;
    if d >= 180.0 {
        d -= 360.0;
    }
    if d < -180.0 {
        d += 360.0;
    }
    d
}

/// `Mth.wrapDegrees(int)`.
#[must_use]
pub fn wrap_degrees_i32(v: i32) -> i32 {
    let mut i = v % 360;
    if i >= 180 {
        i -= 360;
    }
    if i < -180 {
        i += 360;
    }
    i
}

/// `Mth.degreesDifference(float a, float b)`: `wrapDegrees(b - a)`.
#[must_use]
pub fn degrees_difference(a: f32, b: f32) -> f32 {
    wrap_degrees(b - a)
}

/// `Mth.degreesDifferenceAbs(float, float)`: `abs(degreesDifference(a, b))`.
#[must_use]
pub fn degrees_difference_abs(a: f32, b: f32) -> f32 {
    degrees_difference(a, b).abs()
}

/// `Mth.rotateIfNecessary(float current, float target, float max)`:
///
/// ```text
/// d = degreesDifference(current, target)
/// c = clamp(d, -max, max)
/// return target - c
/// ```
#[must_use]
pub fn rotate_if_necessary(current: f32, target: f32, max: f32) -> f32 {
    let d = degrees_difference(current, target);
    let c = clamp_f32(d, -max, max);
    target - c
}

/// `Mth.approach(float from, float to, float step)`:
///
/// ```text
/// step = abs(step)
/// return from < to ? clamp(from + step, from, to) : clamp(from - step, to, from)
/// ```
#[must_use]
pub fn approach(from: f32, to: f32, step: f32) -> f32 {
    let step = step.abs();
    if from < to {
        clamp_f32(from + step, from, to)
    } else {
        clamp_f32(from - step, to, from)
    }
}

/// `Mth.approachDegrees(float from, float to, float step)`:
/// `approach(from, from + degreesDifference(from, to), step)`.
#[must_use]
pub fn approach_degrees(from: f32, to: f32, step: f32) -> f32 {
    let d = degrees_difference(from, to);
    approach(from, from + d, step)
}

/// `Mth.clamp(int, int, int)`: `Math.min(Math.max(v, min), max)`.
#[must_use]
pub fn clamp_i32(v: i32, min: i32, max: i32) -> i32 {
    v.max(min).min(max)
}

/// `Mth.clamp(double, double, double)`: `v < min ? min : Math.min(v, max)`.
#[must_use]
pub fn clamp_f64(v: f64, min: f64, max: f64) -> f64 {
    if v < min {
        min
    } else {
        rustmc_worldgen::math::java_min(v, max)
    }
}

/// `Mth.sqrt(float)`: `(float) Math.sqrt((double) f)`.
#[must_use]
pub fn sqrt_f32(f: f32) -> f32 {
    f64::from(f).sqrt() as f32
}

/// `Mth.floor(float)`: `(int) Math.floor((double) f)`.
#[must_use]
pub fn floor_f32(f: f32) -> i32 {
    floor(f64::from(f))
}

/// `Mth.ceil(double)`: `(int) Math.ceil(d)`, with Java's saturating `d2i`.
#[must_use]
pub fn ceil(d: f64) -> i32 {
    d.ceil() as i32
}

/// `Mth.square(double)`.
#[must_use]
pub fn square(d: f64) -> f64 {
    d * d
}

/// `Mth.sign(double d)`: `d == 0.0 ? 0 : (d > 0.0 ? 1 : -1)`.
#[must_use]
pub fn sign(d: f64) -> i32 {
    if d == 0.0 {
        0
    } else if d > 0.0 {
        1
    } else {
        -1
    }
}

/// `Mth.lerp(float delta, float start, float end)` (offsets 0..7):
/// `start + delta * (end - start)`, in **float** arithmetic — a separate
/// overload from the double one below, and the rounding differs.
#[must_use]
pub fn lerp_f32(delta: f32, start: f32, end: f32) -> f32 {
    start + delta * (end - start)
}

/// `Mth.lerp(double delta, double start, double end)`: `start + delta * (end - start)`.
#[must_use]
pub fn lerp(delta: f64, start: f64, end: f64) -> f64 {
    start + delta * (end - start)
}

/// `Mth.SQRT_OF_TWO`: `sqrt(2.0f)` as a float.
pub const SQRT_OF_TWO: f32 = std::f32::consts::SQRT_2;

/// `Mth.FRAC_BIAS`: `Double.longBitsToDouble(4805340802404319232L)`.
pub const FRAC_BIAS: f64 = f64::from_bits(4_805_340_802_404_319_232);

/// The two 257-entry tables `Mth.<clinit>` builds for `atan2`.
pub struct Atan2Tables {
    pub asin: [f64; 257],
    pub cos: [f64; 257],
}

/// The JVM's tables, from their embedded bits; bit-compared against the
/// vectors by the mob-math golden.
pub static ATAN2_TABLES: LazyLock<Atan2Tables> = LazyLock::new(|| {
    let mut t = Atan2Tables {
        asin: [0.0; 257],
        cos: [0.0; 257],
    };
    for i in 0..257 {
        t.asin[i] = f64::from_bits(crate::mth_tables::ASIN_TAB_BITS[i]);
        t.cos[i] = f64::from_bits(crate::mth_tables::COS_TAB_BITS[i]);
    }
    t
});

/// `Mth.atan2(double y, double x)`, offsets 0..219 of the method.
///
/// ```text
/// 0..7:     len2 = x*x + y*y
/// 9..20:    if isNaN(len2) return NaN
/// 21..41:   negY = y < 0; if negY y = -y
/// 42..62:   negX = x < 0; if negX x = -x
/// 63..88:   swap = y > x; if swap (x, y) = (y, x)
/// 89..105:  inv = fastInvSqrt(len2); x *= inv; y *= inv
/// 106..119: biased = FRAC_BIAS + y; idx = (int) doubleToRawLongBits(biased)
/// 121..135: a = ASIN_TAB[idx]; c = COS_TAB[idx]
/// 137..143: frac = biased - FRAC_BIAS
/// 145..154: e = y*c - x*frac
/// 156..172: corr = (6.0 + e*e) * e * (1/6)
/// 174..179: r = a + corr
/// 181..192: if swap r = PI/2 - r
/// 194..205: if negX r = PI - r
/// 207..215: if negY r = -r
/// ```
///
/// `l2i` on the raw bits keeps the low 32 bits of the long, and Java's
/// `daload` with that int index is what the table lookup does; the bias
/// puts `y` in `[0, 1]` into the low bits of a double whose exponent makes
/// the low 32 bits exactly `round(y * 256)` — which is why the tables are
/// 257 long.
#[must_use]
pub fn atan2(y: f64, x: f64) -> f64 {
    let len2 = x * x + y * y;
    if len2.is_nan() {
        return f64::NAN;
    }
    let neg_y = y < 0.0;
    let y = if neg_y { -y } else { y };
    let neg_x = x < 0.0;
    let x = if neg_x { -x } else { x };
    let swap = y > x;
    let (x, y) = if swap { (y, x) } else { (x, y) };
    let inv = fast_inv_sqrt(len2);
    let x = x * inv;
    let y = y * inv;
    let biased = FRAC_BIAS + y;
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let idx = (biased.to_bits() as i64) as i32;
    let tables = &*ATAN2_TABLES;
    #[allow(clippy::cast_sign_loss)]
    let a = tables.asin[idx as usize];
    #[allow(clippy::cast_sign_loss)]
    let c = tables.cos[idx as usize];
    let frac = biased - FRAC_BIAS;
    let e = y * c - x * frac;
    let corr = (6.0 + e * e) * e * 0.166_666_666_666_666_66;
    let mut r = a + corr;
    if swap {
        r = std::f64::consts::FRAC_PI_2 - r;
    }
    if neg_x {
        r = std::f64::consts::PI - r;
    }
    if neg_y {
        r = -r;
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_degrees_edges() {
        assert_eq!(wrap_degrees(180.0), -180.0);
        assert_eq!(wrap_degrees(-180.0), -180.0);
        assert_eq!(wrap_degrees(540.0), -180.0);
        assert_eq!(wrap_degrees(-190.0), 170.0);
        assert_eq!(wrap_degrees(10.0), 10.0);
        assert_eq!(wrap_degrees_f64(360.0), 0.0);
        assert_eq!(wrap_degrees_i32(180), -180);
        assert_eq!(wrap_degrees_i32(-181), 179);
    }

    #[test]
    fn rotate_if_necessary_clamps_the_difference() {
        // current 0, target 90, max 10: d = 90, c = 10, target - c = 80.
        assert_eq!(rotate_if_necessary(0.0, 90.0, 10.0), 80.0);
        // current 0, target -90: d = -90, c = -10, -90 - -10 = -80.
        assert_eq!(rotate_if_necessary(0.0, -90.0, 10.0), -80.0);
        // within the cone: unchanged current is returned as target - d.
        assert_eq!(rotate_if_necessary(5.0, 8.0, 10.0), 5.0);
    }

    #[test]
    fn approach_degrees_takes_the_short_way() {
        // from 170 to -170 is a 20-degree difference the positive way.
        assert_eq!(approach_degrees(170.0, -170.0, 5.0), 175.0);
        assert_eq!(approach_degrees(170.0, -170.0, 50.0), 190.0);
        assert_eq!(approach(0.0, 10.0, -3.0), 3.0);
        assert_eq!(approach(10.0, 0.0, 3.0), 7.0);
    }

    /// The table index is `round(y * 256)`: the bias's exponent is 2^20, so
    /// the low 32 bits of `FRAC_BIAS + y` count 1/256ths.
    #[test]
    fn the_bias_puts_y_in_the_low_bits() {
        for (y, want) in [(0.0, 0), (0.5, 128), (1.0, 256), (0.25, 64)] {
            let biased = FRAC_BIAS + y;
            #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
            let idx = (biased.to_bits() as i64) as i32;
            assert_eq!(idx, want, "y = {y}");
        }
    }

    /// `atan2` on the axes and the diagonal. The approximation is exact at
    /// the table points, so the axes are exactly 0, PI/2, PI, -PI/2 and the
    /// diagonal is PI/4 to within the cubic's error, which for y = x is
    /// well under 1e-6.
    #[test]
    fn atan2_axes_and_diagonal() {
        assert_eq!(atan2(0.0, 1.0), 0.0);
        assert_eq!(atan2(1.0, 0.0), std::f64::consts::FRAC_PI_2);
        assert!((atan2(0.0, -1.0) - std::f64::consts::PI).abs() < 1e-12);
        assert_eq!(atan2(-1.0, 0.0), -std::f64::consts::FRAC_PI_2);
        assert!((atan2(1.0, 1.0) - std::f64::consts::FRAC_PI_4).abs() < 1e-6);
        assert!(atan2(f64::NAN, 1.0).is_nan());
    }
}
