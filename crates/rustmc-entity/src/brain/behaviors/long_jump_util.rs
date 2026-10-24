//! `net.minecraft.world.entity.ai.behavior.LongJumpUtil`, from the 26.2
//! jar: the ballistic solver behind `LongJumpToRandomPos` and
//! `LongJumpToPreferredBlock`.
//!
//! ```text
//! calculateJumpVectorForAngle(mob, target, maxVelocity, angleDeg, requireClearTransition)  (0..445)
//!   pos  = mob.position()
//!   v6   = new Vec3(target.x - pos.x, 0, target.z - pos.z).normalize().scale(0.5)
//!   v7   = target.subtract(v6);   v8 = v7.subtract(pos)
//!   f9   = (float) angleDeg * 3.1415927f / 180.0f                   <- FLOAT arithmetic, then widened
//!   d10  = Math.atan2(v8.z, v8.x)
//!   d12  = v8.subtract(0, v8.y, 0).lengthSqr();   d14 = sqrt(d12);   d16 = v8.y
//!   d18  = mob.getGravity()
//!   d20  = sin(2f * f9);  d22 = pow(cos(f9), 2);  d24 = sin(f9);  d26 = cos(f9)
//!   d28  = sin(d10);      d30 = cos(d10)
//!   d32  = d12 * d18 / (d14 * d20 - 2.0 * d16 * d22)
//!   if d32 < 0:            empty                                    (201..211)
//!   d34  = sqrt(d32)
//!   if d34 > maxVelocity:  empty                                    (219..230)
//!   d36  = d34 * d26;   d38 = d34 * d24
//!   if requireClearTransition: walk the arc (below)
//!   return Optional.of(new Vec3(d36 * d30, d38, d36 * d28).scale(0.949999988079071))
//!
//! the arc walk (245..416):
//!   n40 = Mth.ceil(d14 / d36) * 2;   d41 = 0.0;   prev = null
//!   dims = mob.getDimensions(Pose.LONG_JUMPING)
//!   for i in 0 .. n40 - 1 (exclusive):                              <- `i < n40 - 1`, so n40-1 steps
//!       d41 += d14 / n40
//!       d46 = (d24 / d26) * d41 - (pow(d41, 2) * d18) / (2.0 * d32 * pow(d26, 2))
//!       cur = new Vec3(pos.x + d41 * d30, pos.y + d46, pos.z + d41 * d28)
//!       if prev != null && !isClearTransition(mob, dims, prev, cur): empty
//!       prev = cur
//!
//! isClearTransition(mob, dims, from, to)  (0..111)
//!   v4 = to.subtract(from);   d5 = (double) Math.min(dims.width(), dims.height())
//!   n7 = Mth.ceil(v4.length() / d5);   v8 = v4.normalize();   v9 = from
//!   for i in 0 .. n7:
//!       v9 = (i == n7 - 1) ? to : v9.add(v8.scale(d5 * 0.8999999761581421))
//!       if !level.noCollision(mob, dims.makeBoundingBox(v9)): false
//!   true
//! ```
//!
//! **The angle is computed in `float` and then widened.** 62..70 is
//! `i2f; ldc 3.1415927f; fmul; ldc 180.0f; fdiv; fstore`, so `f9` is an
//! `f32` and every `sin`/`cos`/`pow` of it takes the widened `f32` value,
//! not the `f64` one — `65.0 * PI / 180.0` in `f64` is a different number.
//! Reproduced as `f64::from(angle as f32 * PI_F32 / 180.0)`.
//!
//! **The scale is `0.949999988079071`, the widened `float` 0.95f**, not
//! `0.95`. `ldc2_w` at 436 carries that exact double.
//!
//! The arc walk runs `n40 - 1` steps and the first has `prev == null`, so
//! it checks `n40 - 2` transitions; the last of them ends at the sampled
//! arc point rather than at the target, and `isClearTransition`'s own last
//! step snaps to `to`.

use crate::ai::mob_ai::MobAi;
use crate::host::{MobHost, PathLevel};
use crate::phys::Aabb;
use crate::phys::Vec3;

/// `Mth.PI` as the `float` constant the bytecode loads (`ldc 3.1415927f`
/// at 64). It is `std::f32::consts::PI`, written through the standard
/// constant so clippy's `approx_constant` is satisfied; the point of the
/// name is that this is the **f32** one.
pub const PI_F32: f32 = std::f32::consts::PI;
/// The `0.95f` at 436, widened exactly as the `ldc2_w` carries it.
pub const RESULT_SCALE: f64 = 0.949_999_988_079_071;
/// The `0.9f` inside `isClearTransition`, likewise widened.
pub const TRANSITION_STEP_SCALE: f64 = 0.899_999_976_158_142_1;

/// `Mth.ceil(double)`: `(int) Math.ceil(d)`. Rust's `as` saturates and
/// maps NaN to 0 exactly as `d2i` does.
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn mth_ceil(d: f64) -> i32 {
    d.ceil() as i32
}

/// `isClearTransition(mob, dims, from, to)`.
#[must_use]
pub fn is_clear_transition<M: MobHost>(
    cx: &MobAi<M>,
    width: f32,
    height: f32,
    from: Vec3,
    to: Vec3,
) -> bool {
    let v4 = to.subtract(from);
    let d5 = f64::from(width.min(height));
    let n7 = mth_ceil(v4.length() / d5);
    let v8 = v4.normalize();
    let mut v9 = from;
    for i in 0..n7 {
        v9 = if i == n7 - 1 {
            to
        } else {
            let s = v8.scale(d5 * TRANSITION_STEP_SCALE);
            Vec3 {
                x: v9.x + s.x,
                y: v9.y + s.y,
                z: v9.z + s.z,
            }
        };
        let bb = make_bounding_box(width, height, v9);
        if !cx.mob.level().no_collision(cx.mob.id(), &bb) {
            return false;
        }
    }
    true
}

/// `EntityDimensions.makeBoundingBox(Vec3)`: the width-square centred on
/// the position, rising by the height. **The half-width is computed in
/// `float`** — `getfield width; fconst_2; fdiv; fstore` at 0..6 — and only
/// then widened, so `f64::from(width) / 2.0` would be a different number
/// for a width whose half is not exactly representable.
#[must_use]
pub fn make_bounding_box(width: f32, height: f32, p: Vec3) -> Aabb {
    let hw = f64::from(width / 2.0);
    Aabb {
        min_x: p.x - hw,
        min_y: p.y,
        min_z: p.z - hw,
        max_x: p.x + hw,
        max_y: p.y + f64::from(height),
        max_z: p.z + hw,
    }
}

/// `LongJumpUtil.calculateJumpVectorForAngle(mob, target, maxVelocity,
/// angle, requireClearTransition)`.
#[must_use]
pub fn calculate_jump_vector_for_angle<M: MobHost>(
    cx: &MobAi<M>,
    target: Vec3,
    max_velocity: f32,
    angle: i32,
    require_clear_transition: bool,
) -> Option<Vec3> {
    let pos = cx.mob.position();
    let v6 = Vec3 {
        x: target.x - pos.x,
        y: 0.0,
        z: target.z - pos.z,
    }
    .normalize()
    .scale(0.5);
    let v7 = target.subtract(v6);
    let v8 = v7.subtract(pos);
    #[allow(clippy::cast_precision_loss)]
    let f9 = f64::from(angle as f32 * PI_F32 / 180.0);
    let d10 = v8.z.atan2(v8.x);
    let d12 = v8.subtract3(0.0, v8.y, 0.0).length_sqr();
    let d14 = d12.sqrt();
    let d16 = v8.y;
    let d18 = cx.mob.gravity();
    let d20 = (2.0 * f9).sin();
    let d22 = f9.cos().powf(2.0);
    let d24 = f9.sin();
    let d26 = f9.cos();
    let d28 = d10.sin();
    let d30 = d10.cos();
    let d32 = d12 * d18 / (d14 * d20 - 2.0 * d16 * d22);
    if d32 < 0.0 {
        return None;
    }
    let d34 = d32.sqrt();
    if d34 > f64::from(max_velocity) {
        return None;
    }
    let d36 = d34 * d26;
    let d38 = d34 * d24;
    if require_clear_transition {
        let n40 = mth_ceil(d14 / d36) * 2;
        let mut d41 = 0.0_f64;
        let mut prev: Option<Vec3> = None;
        let (width, height) = cx.mob.long_jumping_dimensions();
        for _ in 0..(n40 - 1) {
            d41 += d14 / f64::from(n40);
            let d46 = (d24 / d26) * d41 - (d41.powf(2.0) * d18) / (2.0 * d32 * d26.powf(2.0));
            let cur = Vec3 {
                x: pos.x + d41 * d30,
                y: pos.y + d46,
                z: pos.z + d41 * d28,
            };
            if let Some(p) = prev {
                if !is_clear_transition(cx, width, height, p, cur) {
                    return None;
                }
            }
            prev = Some(cur);
        }
    }
    Some(
        Vec3 {
            x: d36 * d30,
            y: d38,
            z: d36 * d28,
        }
        .scale(RESULT_SCALE),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{GridLevel, TestMob};

    /// The solver is pure in `(position, target, maxVelocity, angle,
    /// gravity)` when `requireClearTransition` is false, so the expected
    /// values here are a **second transcription of the same bytecode**
    /// evaluated independently in `f64` (a twin, not a jar golden — this
    /// class needs a `Mob` in a `ServerLevel` to run in the jar). What it
    /// catches is an operator-order or widening slip in the port.
    #[test]
    fn the_angle_is_computed_in_float_and_the_result_scaled_by_the_widened_0_95f() {
        let mut cx =
            crate::ai::mob_ai::MobAi::new(TestMob::new(GridLevel::new(), 0.0, 0.0, 0.0, 1));
        cx.mob.gravity = 0.08;
        let target = Vec3 {
            x: 6.0,
            y: 1.0,
            z: 0.0,
        };
        let v = calculate_jump_vector_for_angle(&cx, target, 10.0, 65, false).expect("solvable");
        // Independently evaluated from the bytecode, f32 angle widened:
        //   f9 = (float)65 * 3.1415927f / 180.0f = 1.1344640254974365
        //   v6 = (1,0,0)*0.5;  v8 = (5.5, 1, 0);  d12 = 30.25; d14 = 5.5
        //   d32 = 30.25*0.08 / (5.5*sin(2f9) - 2*1*cos(f9)^2) = 0.7009...
        let f9 = f64::from(65_f32 * PI_F32 / 180.0);
        let d12 = 30.25_f64;
        let d14 = 5.5_f64;
        let d32 = d12 * 0.08 / (d14 * (2.0 * f9).sin() - 2.0 * 1.0 * f9.cos().powf(2.0));
        let d34 = d32.sqrt();
        let expected = Vec3 {
            x: d34 * f9.cos() * 1.0,
            y: d34 * f9.sin(),
            z: d34 * f9.cos() * 0.0,
        }
        .scale(RESULT_SCALE);
        assert!((v.x - expected.x).abs() < 1e-12, "{v:?} vs {expected:?}");
        assert!((v.y - expected.y).abs() < 1e-12);
        assert!(v.z.abs() < 1e-12);
        // The float angle is not the double one: 65.0 * PI / 180.0 in f64
        // differs in the last bits, and that difference reaches the result.
        let f9_f64 = 65.0 * std::f64::consts::PI / 180.0;
        assert_ne!(f9, f9_f64, "the widened f32 angle is its own value");
    }

    #[test]
    fn a_negative_discriminant_and_an_over_fast_solution_both_refuse() {
        let mut cx =
            crate::ai::mob_ai::MobAi::new(TestMob::new(GridLevel::new(), 0.0, 0.0, 0.0, 1));
        cx.mob.gravity = 0.08;
        // A target far above: the denominator goes negative, d32 < 0.
        let up = Vec3 {
            x: 1.0,
            y: 40.0,
            z: 0.0,
        };
        assert_eq!(
            calculate_jump_vector_for_angle(&cx, up, 10.0, 80, false),
            None
        );
        // A reachable target refused only because maxVelocity is too small.
        let far = Vec3 {
            x: 12.0,
            y: 0.0,
            z: 0.0,
        };
        assert!(calculate_jump_vector_for_angle(&cx, far, 10.0, 65, false).is_some());
        assert_eq!(
            calculate_jump_vector_for_angle(&cx, far, 0.2, 65, false),
            None
        );
    }

    #[test]
    fn mth_ceil_rounds_towards_positive_infinity_including_negatives() {
        assert_eq!(mth_ceil(2.0), 2);
        assert_eq!(mth_ceil(2.000_001), 3);
        assert_eq!(mth_ceil(-2.5), -2);
        assert_eq!(mth_ceil(0.0), 0);
    }
}
