//! `Projectile.getMovementToShoot`, `shoot`, `shootFromRotation` and the two
//! rotation updates, transcribed from the pinned 26.2 jar.
//!
//! # The inaccuracy is a TRIANGLE, not a Gaussian
//!
//! Every note this project carried said `shoot`'s spread was
//! `nextGaussian() * 0.0075 * inaccuracy` — the pre-1.19 shape. In 26.2
//! `Projectile.getMovementToShoot` is
//!
//! ```text
//!    0: new Vec3(x, y, z)
//!   11: .normalize()
//!   14..65: .add(random.triangle(0.0, 0.0172275 * inaccuracy),   <- x
//!                random.triangle(0.0, 0.0172275 * inaccuracy),   <- y
//!                random.triangle(0.0, 0.0172275 * inaccuracy))   <- z
//!   68: .scale((double) velocity)
//! ```
//!
//! and `RandomSource.triangle(double d, double e)` is
//! `d + e * (nextDouble() - nextDouble())`. So the draw count per shot is
//! **six `nextDouble`s, not three `nextGaussian`s**, the constant is
//! `0.0172275` and there is no divisor of any kind. A port carrying the old
//! shape draws the wrong number of values from the shared entity RNG and
//! every later draw in that tick is displaced, which is a defect no
//! trajectory test can see and every RNG-sensitive one can.
//!
//! The multiply is `0.0172275 * (double) inaccuracy` — the float is widened
//! first — and the three `triangle` calls are evaluated left to right, so the
//! draw order is x-lo, x-hi, y-lo, y-hi, z-lo, z-hi.
//!
//! # `57.2957763671875` is not `180 / PI`, and it is not `(float)(180 / PI)`
//!
//! Both rotations go through `Mth.atan2(..) * 57.2957763671875`. Three
//! candidates and only one of them is this number, measured rather than
//! assumed:
//!
//! ```text
//!   (double) 180 / Math.PI                        57.29577951308232
//!   (double)(float)(180 / Math.PI)                57.295780181884766
//!   (double)(float)(180.0f / (float) Math.PI)     57.2957763671875   <- this one
//! ```
//!
//! So the division is done in **float, with a float PI**, and only then
//! widened. The obvious reading — the mathematical constant — is 1.9e-6 away,
//! which moves the packed rotation byte by one step for a band either side of
//! every boundary.

use crate::collide::Vec3;
use crate::projectile::math::{add3, horizontal_distance, normalize, scale};
use rustmc_entity::mth::{cos, sin};
use rustmc_worldgen::rng::RandomSource;

/// `Projectile.getMovementToShoot`'s spread scale — `ldc2_w 0.0172275d`.
pub const INACCURACY_SCALE: f64 = 0.0172275;

/// The `ldc2_w` in `shoot`, `AbstractArrow.tick` and `updateRotation`:
/// `(double) (180.0f / (float) Math.PI)`.
pub const RAD_TO_DEG: f64 = 57.2957763671875;

/// `Entity.calculateViewVector`'s degrees-to-radians float — `ldc 0.017453292f`.
pub const DEG_TO_RAD: f32 = 0.017_453_292_f32;

/// `RandomSource.triangle(double d, double e)`:
/// `d + e * (nextDouble() - nextDouble())`.
///
/// Two draws, and **the subtrahend is drawn second**. Swapping them gives a
/// distribution with the same shape and a different sample for every seed.
pub fn triangle<R: RandomSource>(rng: &mut R, d: f64, e: f64) -> f64 {
    let a = rng.next_f64();
    let b = rng.next_f64();
    d + e * (a - b)
}

/// `Projectile.getMovementToShoot(double x, double y, double z, float
/// velocity, float inaccuracy)`.
///
/// Six `nextDouble` draws, in x, y, z order. Returns the delta movement the
/// caller then assigns.
pub fn movement_to_shoot<R: RandomSource>(
    rng: &mut R,
    x: f64,
    y: f64,
    z: f64,
    velocity: f32,
    inaccuracy: f32,
) -> Vec3 {
    let spread = INACCURACY_SCALE * f64::from(inaccuracy);
    let n = normalize(Vec3::new(x, y, z));
    let dx = triangle(rng, 0.0, spread);
    let dy = triangle(rng, 0.0, spread);
    let dz = triangle(rng, 0.0, spread);
    scale(add3(n, dx, dy, dz), f64::from(velocity))
}

/// The rotation pair `shoot` and `updateRotation` set, as vanilla's floats.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rot {
    /// `Entity.getYRot()`.
    pub y_rot: f32,
    /// `Entity.getXRot()`.
    pub x_rot: f32,
}

/// What `Projectile.shoot` produces: the new delta movement and the rotations
/// it derives from it.
///
/// `shoot` also sets `needsSync = true` and copies both rotations into
/// `yRotO`/`xRotO`. The `O` copies are the previous-tick values the client
/// interpolates from and this server does not hold them; the `needsSync` flag
/// is the tracker's and is set by the caller, because it is the one field of
/// `shoot` whose effect is on the wire rather than on the entity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shot {
    pub movement: Vec3,
    pub rot: Rot,
}

/// `Projectile.shoot(double x, double y, double z, float velocity, float
/// inaccuracy)`, offsets 0-89.
///
/// ```text
///   Vec3 v = getMovementToShoot(x, y, z, velocity, inaccuracy);
///   setDeltaMovement(v);
///   needsSync = true;
///   double h = v.horizontalDistance();
///   setYRot((float)(Mth.atan2(v.x, v.z) * 57.2957763671875));
///   setXRot((float)(Mth.atan2(v.y, h)   * 57.2957763671875));
///   yRotO = getYRot(); xRotO = getXRot();
/// ```
///
/// **`atan2(v.x, v.z)`, in that order** — `Mth.atan2(y, x)`'s first argument
/// is the numerator, so yaw is taken with x as the numerator and z as the
/// denominator, and the transposed reading gives a heading reflected about
/// the 45-degree line.
pub fn shoot<R: RandomSource>(
    rng: &mut R,
    x: f64,
    y: f64,
    z: f64,
    velocity: f32,
    inaccuracy: f32,
) -> Shot {
    let v = movement_to_shoot(rng, x, y, z, velocity, inaccuracy);
    let h = horizontal_distance(v);
    Shot {
        movement: v,
        rot: Rot {
            #[expect(clippy::cast_possible_truncation, reason = "vanilla's d2f")]
            y_rot: (rustmc_entity::mth::atan2(v.x, v.z) * RAD_TO_DEG) as f32,
            #[expect(clippy::cast_possible_truncation, reason = "vanilla's d2f")]
            x_rot: (rustmc_entity::mth::atan2(v.y, h) * RAD_TO_DEG) as f32,
        },
    }
}

/// `Projectile.shootFromRotation(Entity shooter, float xRot, float yRot,
/// float zRot, float velocity, float inaccuracy)`, offsets 0-113.
///
/// ```text
///   f = -sin(yRot * D) * cos(xRot * D)
///   g = -sin((xRot + zRot) * D)
///   h =  cos(yRot * D) * cos(xRot * D)
///   shoot(f, g, h, velocity, inaccuracy);
///   Vec3 k = shooter.getKnownMovement();
///   setDeltaMovement(getDeltaMovement()
///       .add(k.x, shooter.onGround() ? 0.0 : k.y, k.z));
/// ```
///
/// Three things a reading gets wrong. `zRot` is added to the **pitch**, not
/// to a roll — it is the crossbow's multishot fan and the trident's zero.
/// The products are computed in `float` and only widened at the `shoot` call,
/// so the whole direction is float-precision. And the shooter's own motion is
/// added **after** the inaccuracy draw, with the y component suppressed while
/// the shooter is on the ground; an arrow fired by a walking player therefore
/// inherits the walk and one fired by a falling player does not inherit the
/// fall.
#[expect(
    clippy::too_many_arguments,
    reason = "vanilla's shootFromRotation arity"
)]
pub fn shoot_from_rotation<R: RandomSource>(
    rng: &mut R,
    x_rot: f32,
    y_rot: f32,
    z_rot: f32,
    velocity: f32,
    inaccuracy: f32,
    shooter_movement: Vec3,
    shooter_on_ground: bool,
) -> Shot {
    let f = -sin(f64::from(y_rot * DEG_TO_RAD)) * cos(f64::from(x_rot * DEG_TO_RAD));
    let g = -sin(f64::from((x_rot + z_rot) * DEG_TO_RAD));
    let h = cos(f64::from(y_rot * DEG_TO_RAD)) * cos(f64::from(x_rot * DEG_TO_RAD));
    let shot = shoot(
        rng,
        f64::from(f),
        f64::from(g),
        f64::from(h),
        velocity,
        inaccuracy,
    );
    let ky = if shooter_on_ground {
        0.0
    } else {
        shooter_movement.y
    };
    Shot {
        movement: add3(shot.movement, shooter_movement.x, ky, shooter_movement.z),
        rot: shot.rot,
    }
}

/// `Projectile.lerpRotation(float current, float target)`, offsets 0-46.
///
/// ```text
///   while (target - current < -180.0f) current -= 360.0f;
///   while (target - current >= 180.0f) current += 360.0f;
///   return Mth.lerp(0.2f, current, target);
/// ```
///
/// The two loops normalise the **current** angle towards the target, not the
/// target towards the current, and the second is `>=` where the first is `<`.
/// A single `wrapDegrees` of the difference gives the same answer for every
/// input that terminates and a different one at exactly 180.
#[must_use]
pub fn lerp_rotation(current: f32, target: f32) -> f32 {
    let mut c = current;
    while target - c < -180.0_f32 {
        c -= 360.0_f32;
    }
    while target - c >= 180.0_f32 {
        c += 360.0_f32;
    }
    // `Mth.lerp(float delta, float start, float end)` is
    // `start + delta * (end - start)`, in float.
    c + 0.2_f32 * (target - c)
}

/// `Projectile.updateRotation()`, offsets 0-61.
///
/// Note that it lerps from `xRotO`/`yRotO` — the **previous tick's** angles —
/// where `AbstractArrow.tick` lerps from the live `getXRot()`/`getYRot()`.
/// The two are the same value for anything whose rotation is only ever
/// written here, which is every `ThrowableProjectile`, and the distinction is
/// kept so that a kind which writes its rotation elsewhere is not silently
/// given the wrong base.
#[must_use]
pub fn update_rotation(prev: Rot, movement: Vec3) -> Rot {
    let h = horizontal_distance(movement);
    #[expect(clippy::cast_possible_truncation, reason = "vanilla's d2f")]
    let x = (rustmc_entity::mth::atan2(movement.y, h) * RAD_TO_DEG) as f32;
    #[expect(clippy::cast_possible_truncation, reason = "vanilla's d2f")]
    let y = (rustmc_entity::mth::atan2(movement.x, movement.z) * RAD_TO_DEG) as f32;
    Rot {
        x_rot: lerp_rotation(prev.x_rot, x),
        y_rot: lerp_rotation(prev.y_rot, y),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmc_worldgen::rng::legacy::LegacyRandomSource;

    /// A counting wrapper: the draw count is the thing the Gaussian
    /// correction is about, and it is invisible in the resulting vector.
    struct Counting {
        inner: LegacyRandomSource,
        doubles: u32,
        floats: u32,
        gaussians: u32,
    }

    impl Counting {
        fn new(seed: i64) -> Self {
            Self {
                inner: LegacyRandomSource::new(seed),
                doubles: 0,
                floats: 0,
                gaussians: 0,
            }
        }
    }

    impl RandomSource for Counting {
        fn set_seed(&mut self, seed: i64) {
            self.inner.set_seed(seed);
        }
        fn next_i32(&mut self) -> i32 {
            self.inner.next_i32()
        }
        fn next_i32_bound(&mut self, bound: i32) -> i32 {
            self.inner.next_i32_bound(bound)
        }
        fn next_i64(&mut self) -> i64 {
            self.inner.next_i64()
        }
        fn next_bool(&mut self) -> bool {
            self.inner.next_bool()
        }
        fn next_f32(&mut self) -> f32 {
            self.floats += 1;
            self.inner.next_f32()
        }
        fn next_f64(&mut self) -> f64 {
            self.doubles += 1;
            self.inner.next_f64()
        }
        fn next_gaussian(&mut self) -> f64 {
            self.gaussians += 1;
            self.inner.next_gaussian()
        }
    }

    #[test]
    fn a_shot_costs_six_nextdouble_and_no_gaussian() {
        let mut r = Counting::new(42);
        let _ = movement_to_shoot(&mut r, 0.0, 0.0, 1.0, 3.0, 1.0);
        assert_eq!(r.doubles, 6, "triangle is two nextDouble, three times");
        assert_eq!(r.gaussians, 0, "26.2 does not draw a Gaussian here");
        assert_eq!(r.floats, 0);
    }

    #[test]
    fn zero_inaccuracy_still_costs_six_draws() {
        // The spread scale is zero but `triangle` is still called, so the
        // stream still advances. A port that short-circuits on
        // `inaccuracy == 0` is bit-identical in the vector and wrong in
        // every later draw of the tick.
        let mut r = Counting::new(7);
        let v = movement_to_shoot(&mut r, 0.0, 0.0, 1.0, 2.0, 0.0);
        assert_eq!(r.doubles, 6);
        assert_eq!(v, Vec3::new(0.0, 0.0, 2.0));
    }

    #[test]
    fn shoot_takes_yaw_from_x_over_z() {
        let mut r = LegacyRandomSource::new(1);
        let s = shoot(&mut r, 1.0, 0.0, 0.0, 1.0, 0.0);
        // Straight +X is yaw +90 in vanilla's convention.
        assert!((s.rot.y_rot - 90.0).abs() < 1e-4, "{}", s.rot.y_rot);
        let mut r = LegacyRandomSource::new(1);
        let s = shoot(&mut r, 0.0, 0.0, 1.0, 1.0, 0.0);
        assert!(s.rot.y_rot.abs() < 1e-4, "{}", s.rot.y_rot);
    }

    #[test]
    fn the_degree_constant_is_a_float_divide_by_a_float_pi() {
        // The three candidates, all three evaluated, so the test says which
        // one the jar carries rather than only that it is not the obvious one.
        let maths = 180.0_f64 / std::f64::consts::PI;
        #[expect(clippy::cast_possible_truncation, reason = "the candidate under test")]
        let narrowed = f64::from(maths as f32);
        let float_pi = f64::from(180.0_f32 / std::f32::consts::PI);
        assert_ne!(RAD_TO_DEG, maths);
        assert_ne!(RAD_TO_DEG, narrowed);
        assert_eq!(RAD_TO_DEG, float_pi);
        assert_eq!(RAD_TO_DEG, 57.295_776_367_187_5);
    }

    #[test]
    fn shoot_from_rotation_puts_zrot_on_the_pitch() {
        let mut r = LegacyRandomSource::new(3);
        let straight = shoot_from_rotation(&mut r, 0.0, 0.0, 0.0, 1.0, 0.0, Vec3::ZERO, true);
        let mut r = LegacyRandomSource::new(3);
        let fanned = shoot_from_rotation(&mut r, 0.0, 0.0, 10.0, 1.0, 0.0, Vec3::ZERO, true);
        // A non-zero zRot changes y and leaves the horizontal heading alone.
        assert!(fanned.movement.y < straight.movement.y - 0.1);
        assert!((fanned.movement.x - straight.movement.x).abs() < 1e-9);
    }

    #[test]
    fn the_shooters_fall_is_not_inherited_and_its_walk_is() {
        let walk = Vec3::new(0.2, -0.5, 0.0);
        let mut r = LegacyRandomSource::new(5);
        let grounded = shoot_from_rotation(&mut r, 0.0, 0.0, 0.0, 1.0, 0.0, walk, true);
        let mut r = LegacyRandomSource::new(5);
        let falling = shoot_from_rotation(&mut r, 0.0, 0.0, 0.0, 1.0, 0.0, walk, false);
        assert_eq!(grounded.movement.x, falling.movement.x);
        assert!((grounded.movement.y - falling.movement.y - 0.5).abs() < 1e-12);
    }

    #[test]
    fn the_shooters_motion_is_added_after_the_rotation_is_taken() {
        // `shoot` sets the rotation from the pre-addition vector, so two
        // shots that differ only in the shooter's motion carry the SAME
        // yaw and pitch. A port that added the motion first would not.
        let mut r = LegacyRandomSource::new(9);
        let still = shoot_from_rotation(&mut r, 0.0, 0.0, 0.0, 1.0, 0.0, Vec3::ZERO, true);
        let mut r = LegacyRandomSource::new(9);
        let moving = shoot_from_rotation(
            &mut r,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
            Vec3::new(0.4, 0.0, 0.0),
            true,
        );
        assert_eq!(still.rot, moving.rot);
        assert!((moving.movement.x - still.movement.x - 0.4).abs() < 1e-12);
    }

    #[test]
    fn lerp_rotation_normalises_the_current_angle_across_the_seam() {
        // 350 -> 10 is a 20-degree turn, and vanilla walks `current` DOWN to
        // -10 first, so the answer is -6 and not the congruent 354. The
        // sign matters: this value is written straight into `yRot` and then
        // packed, and -6 and 354 pack to the same byte but read back
        // differently the next time `lerpRotation` normalises from it.
        let out = lerp_rotation(350.0, 10.0);
        assert!((out - -6.0).abs() < 1e-3, "{out}");
        // Positive control: dropping the wrap loops gives 282.0.
        assert!((out - (350.0 + 0.2 * (10.0 - 350.0))).abs() > 1.0);
    }

    #[test]
    fn lerp_rotation_at_exactly_180_takes_the_ge_branch() {
        // The second loop is `>=`, so a target exactly 180 ahead sends
        // `current` UP to 360 and the answer is 324 rather than 36. This is
        // the one input where a `wrapDegrees(target - current)` rewrite
        // gives a different number.
        let out = lerp_rotation(0.0, 180.0);
        assert!((out - 324.0).abs() < 1e-3, "{out}");
        assert!((out - 36.0).abs() > 1.0);
    }

    #[test]
    fn update_rotation_lerps_from_the_previous_angles() {
        let prev = Rot {
            y_rot: 0.0,
            x_rot: 0.0,
        };
        let r = update_rotation(prev, Vec3::new(1.0, 0.0, 0.0));
        // One fifth of the way from 0 to 90.
        assert!((r.y_rot - 18.0).abs() < 1e-3, "{}", r.y_rot);
    }
}
