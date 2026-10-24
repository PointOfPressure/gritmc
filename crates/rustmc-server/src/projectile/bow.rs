//! `BowItem.releaseUsing` and `CrossbowItem`'s charge and shoot path.
//!
//! # The bow's power curve is quadratic and its threshold is on the POWER
//!
//! ```text
//!   BowItem.getPowerForTime(int charge):
//!       float f = charge / 20.0f;
//!       f = (f * f + f * 2.0f) / 3.0f;
//!       if (f > 1.0f) f = 1.0f;
//!       return f;
//!
//!   BowItem.releaseUsing(stack, level, shooter, remaining):
//!       if (!(shooter instanceof Player p)) return false;
//!       ItemStack ammo = p.getProjectile(stack);
//!       if (ammo.isEmpty()) return false;
//!       int charge = getUseDuration(stack, shooter) - remaining;
//!       float power = getPowerForTime(charge);
//!       if (power < 0.1) return false;                    <- (double) 0.1, on POWER
//!       List<ItemStack> drawn = draw(stack, ammo, p);
//!       if (server && !drawn.isEmpty())
//!           shoot(level, p, p.getUsedItemHand(), stack, drawn,
//!                 power * 3.0f, 1.0f, power == 1.0f, null);
//!       level.playSound(null, p.getX(), p.getY(), p.getZ(), ARROW_SHOOT, PLAYERS,
//!                       1.0f, 1.0f / (level.random.nextFloat() * 0.4f + 1.2f)
//!                            + power * 0.5f);
//!       p.awardStat(Stats.ITEM_USED.get(this));
//!       return true;
//! ```
//!
//! Three readings that are wrong. **The 0.1 cut is on the power, not on the
//! charge ticks** — the charge that first clears it is 1 tick, because
//! `(0.05² + 0.1)/3 = 0.0342` fails and 2 ticks gives `0.0733`, and 3 gives
//! `0.1225`. **`power == 1.0f` is an exact float equality** and it is what
//! makes an arrow critical, so a port comparing `>= 0.99` fires crits
//! vanilla does not. And **the sound is played whether or not anything was
//! shot**, including on a dedicated server where the `drawn` list came back
//! empty, so a bow released with no arrow at all still makes the noise as
//! long as the power cleared 0.1.
//!
//! # The arrow does not start at the eye
//!
//! `AbstractArrow(EntityType, LivingEntity shooter, Level, ItemStack,
//! ItemStack)` places it at
//! `(shooter.getX(), shooter.getEyeY() - 0.10000000149011612, shooter.getZ())`
//! — the constant is the float `0.1f` widened, not the double `0.1` — and
//! then calls `setOwner(shooter)`. A tenth of a block below the eye, and
//! centred on the shooter's own x and z rather than pushed out along the
//! view.
//!
//! # The crossbow
//!
//! ```text
//!   MAX_CHARGE_DURATION 1.25f     ARROW_POWER 3.15f    FIREWORK_POWER 1.6f
//!   getChargeDuration = Mth.floor(1.25f * 20.0f) = 25   (before enchantments)
//!   getPowerForTime(charge, ..) = min(1.0f, charge / getChargeDuration)
//!   getShotPitch(random, index):
//!       if (index == 0) return 1.0f;
//!       boolean odd = (index & 1) == 1;
//!       return getRandomShotPitch(odd, random);
//!   getRandomShotPitch(high, random):
//!       (high ? 0.63f : 0.43f) + 1.0f / (random.nextFloat() * 0.5f + 1.8f);
//! ```
//!
//! **The first projectile of a volley takes no draw at all** — `index == 0`
//! returns the literal `1.0f` before `getRandomShotPitch` is reached — so a
//! single-bolt crossbow shot costs zero `nextFloat`s where a bow shot costs
//! one. Getting that wrong displaces every later draw in the tick.
//!
//! # Refused, with the reason
//!
//! * **Multishot's fan.** `CrossbowItem.shootProjectile` rotates the view
//!   vector by a `org.joml.Quaternionf.setAngleAxis(angle * 0.017453292f,
//!   upVector)` and `Vector3f.rotate`, in float. That is a separate
//!   transcription of JOML's quaternion arithmetic, and the fan only exists
//!   under the Multishot enchantment, which this server has no table for.
//!   [`crossbow_shot_direction`] handles the `angle == 0` case, where the
//!   quaternion is the identity and the shot vector is the view vector
//!   exactly.
//! * **The mob-targeted arm.** `shootProjectile`'s first branch aims at a
//!   `LivingEntity target`, which only `Piglin` and `Pillager` pass; no mob
//!   in this server shoots.
//! * **`ProjectileWeaponItem.draw`, `useAmmo` and `hurtAndBreak`** are the
//!   inventory and durability lanes'. [`Release`] reports what would be
//!   drawn and consumed and consumes nothing.

use crate::collide::Vec3;
use rustmc_entity::mth::calculate_view_vector;
use rustmc_worldgen::rng::RandomSource;

/// `BowItem.MAX_DRAW_DURATION`.
pub const MAX_DRAW_DURATION: i32 = 20;
/// `BowItem.DEFAULT_RANGE`.
pub const BOW_DEFAULT_RANGE: i32 = 15;
/// `CrossbowItem.MAX_CHARGE_DURATION`.
pub const CROSSBOW_MAX_CHARGE_DURATION: f32 = 1.25;
/// `CrossbowItem.ARROW_POWER`.
pub const CROSSBOW_ARROW_POWER: f32 = 3.15;
/// `CrossbowItem.FIREWORK_POWER`.
pub const CROSSBOW_FIREWORK_POWER: f32 = 1.6;
/// `CrossbowItem.MOB_ARROW_POWER`.
pub const CROSSBOW_MOB_ARROW_POWER: f32 = 1.6;
/// `CrossbowItem.DEFAULT_RANGE`.
pub const CROSSBOW_DEFAULT_RANGE: i32 = 8;
/// The `dsub` in `AbstractArrow(EntityType, LivingEntity, ...)` — the float
/// `0.1f` widened to a double, **not** the double `0.1`.
pub const ARROW_SPAWN_BELOW_EYE: f64 = 0.100_000_001_490_116_12;

/// `BowItem.getPowerForTime(int)`.
#[must_use]
pub fn bow_power_for_time(charge: i32) -> f32 {
    #[expect(clippy::cast_precision_loss, reason = "vanilla's i2f")]
    let mut f = charge as f32 / 20.0_f32;
    // `f * f + f * 2.0f` with TWO roundings, which is what the bytecode
    // does (`fmul`, `fmul`, `fadd`). A fused multiply-add here rounds once
    // and gives a different float for most inputs.
    f = (f * f + f * 2.0_f32) / 3.0_f32;
    if f > 1.0 {
        f = 1.0;
    }
    f
}

/// `CrossbowItem.getChargeDuration(stack, shooter)` with no enchantment:
/// `Mth.floor(1.25f * 20.0f)`.
#[must_use]
pub fn crossbow_charge_duration() -> i32 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "vanilla's Mth.floor(float)"
    )]
    let d = (CROSSBOW_MAX_CHARGE_DURATION * 20.0_f32).floor() as i32;
    d
}

/// `CrossbowItem.getPowerForTime(int charge, stack, shooter)`.
#[must_use]
pub fn crossbow_power_for_time(charge: i32) -> f32 {
    #[expect(clippy::cast_precision_loss, reason = "vanilla's i2f")]
    let f = charge as f32 / crossbow_charge_duration() as f32;
    if f > 1.0 {
        1.0
    } else {
        f
    }
}

/// `CrossbowItem.getShotPitch(RandomSource, int index)`.
///
/// **Index 0 takes no draw.**
pub fn crossbow_shot_pitch<R: RandomSource>(rng: &mut R, index: i32) -> f32 {
    if index == 0 {
        return 1.0;
    }
    let base = if index & 1 == 1 { 0.63_f32 } else { 0.43_f32 };
    base + 1.0_f32 / (rng.next_f32() * 0.5_f32 + 1.8_f32)
}

/// What `BowItem.releaseUsing` decided.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Release {
    /// `getPowerForTime(useDuration - remaining)`.
    pub power: f32,
    /// The `power * 3.0f` handed to `shoot` as the velocity.
    pub velocity: f32,
    /// The `1.0f` inaccuracy the bow always passes.
    pub inaccuracy: f32,
    /// `power == 1.0f` — an exact float equality.
    pub crit: bool,
    /// `SoundEvents.ARROW_SHOOT`'s pitch:
    /// `1.0f / (nextFloat() * 0.4f + 1.2f) + power * 0.5f`.
    pub sound_pitch: f32,
}

/// `BowItem.releaseUsing`'s arithmetic, from the charge and one `nextFloat`.
///
/// Returns `None` on the `power < 0.1` refusal, which is the whole of the
/// early-return path this server can evaluate: the ammunition and player
/// checks above it are the inventory lane's.
///
/// **The draw happens only when the power clears the cut**, because the
/// `return false` at offset 64 is before the sound.
pub fn bow_release<R: RandomSource>(charge: i32, rng: &mut R) -> Option<Release> {
    let power = bow_power_for_time(charge);
    if f64::from(power) < 0.1 {
        return None;
    }
    let pitch = 1.0_f32 / (rng.next_f32() * 0.4_f32 + 1.2_f32) + power * 0.5_f32;
    Some(Release {
        power,
        velocity: power * 3.0_f32,
        inaccuracy: 1.0,
        crit: power == 1.0_f32,
        sound_pitch: pitch,
    })
}

/// The position `AbstractArrow`'s shooter constructor puts the arrow at.
#[must_use]
pub fn arrow_spawn_position(shooter_pos: Vec3, eye_height: f64) -> Vec3 {
    Vec3::new(
        shooter_pos.x,
        shooter_pos.y + eye_height - ARROW_SPAWN_BELOW_EYE,
        shooter_pos.z,
    )
}

/// `CrossbowItem.shootProjectile`'s untargeted arm at `angle == 0`.
///
/// `Quaternionf().setAngleAxis(0.0, up)` is the identity rotation and
/// `Vector3f.rotate(identity)` returns the vector unchanged, so the shot
/// direction is `shooter.getViewVector(1.0f)` exactly — which is
/// `Entity.calculateViewVector(getXRot(), getYRot())`, already ported in
/// `rustmc_entity::mth`.
///
/// # Panics
///
/// Never; the assertion documents the one input this function is defined for.
#[must_use]
pub fn crossbow_shot_direction(x_rot: f32, y_rot: f32, angle: f32) -> Option<Vec3> {
    if angle != 0.0 {
        return None;
    }
    let v = calculate_view_vector(x_rot, y_rot);
    Some(Vec3::new(v.x, v.y, v.z))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmc_worldgen::rng::legacy::LegacyRandomSource;

    #[test]
    fn the_power_cut_lands_at_three_charge_ticks() {
        let mut r = LegacyRandomSource::new(1);
        assert!(bow_release(0, &mut r).is_none());
        assert!(bow_release(1, &mut r).is_none());
        assert!(bow_release(2, &mut r).is_none());
        assert!(bow_release(3, &mut r).is_some(), "0.1225 clears 0.1");
    }

    #[test]
    fn a_refused_release_takes_no_draw() {
        // The `return false` is before the sound, so a bow released too
        // early does not advance the level RNG. A port that computed the
        // pitch first would displace every later draw in the tick.
        struct Counting(LegacyRandomSource, u32);
        impl RandomSource for Counting {
            fn set_seed(&mut self, s: i64) {
                self.0.set_seed(s);
            }
            fn next_i32(&mut self) -> i32 {
                self.0.next_i32()
            }
            fn next_i32_bound(&mut self, b: i32) -> i32 {
                self.0.next_i32_bound(b)
            }
            fn next_i64(&mut self) -> i64 {
                self.0.next_i64()
            }
            fn next_bool(&mut self) -> bool {
                self.0.next_bool()
            }
            fn next_f32(&mut self) -> f32 {
                self.1 += 1;
                self.0.next_f32()
            }
            fn next_f64(&mut self) -> f64 {
                self.0.next_f64()
            }
            fn next_gaussian(&mut self) -> f64 {
                self.0.next_gaussian()
            }
        }
        let mut r = Counting(LegacyRandomSource::new(1), 0);
        assert!(bow_release(1, &mut r).is_none());
        assert_eq!(r.1, 0);
        assert!(bow_release(20, &mut r).is_some());
        assert_eq!(r.1, 1);
    }

    #[test]
    fn a_full_draw_is_exactly_one_and_is_critical() {
        assert_eq!(bow_power_for_time(20), 1.0);
        let mut r = LegacyRandomSource::new(1);
        assert!(bow_release(20, &mut r).unwrap().crit);
        // The curve is quadratic, so the tick before a full draw is a long
        // way short of 1.0: `f = 0.95`, `(0.9025 + 1.9) / 3 = 0.93416667`.
        // A port that read the curve as linear would put this at 0.95, and
        // one that read the crit test as `>= 0.9` would fire a crit here.
        let p = bow_power_for_time(19);
        assert!((p - 0.934_166_67).abs() < 1e-6, "{p}");
        let mut r = LegacyRandomSource::new(1);
        assert!(!bow_release(19, &mut r).unwrap().crit);
    }

    #[test]
    fn the_power_saturates_and_the_velocity_is_three_times_it() {
        assert_eq!(bow_power_for_time(100), 1.0);
        let mut r = LegacyRandomSource::new(1);
        let rel = bow_release(20, &mut r).unwrap();
        assert_eq!(rel.velocity, 3.0);
        assert_eq!(rel.inaccuracy, 1.0);
    }

    #[test]
    fn the_crossbow_charge_duration_is_twenty_five() {
        assert_eq!(crossbow_charge_duration(), 25);
        assert_eq!(crossbow_power_for_time(25), 1.0);
        assert_eq!(crossbow_power_for_time(100), 1.0);
        assert!((crossbow_power_for_time(5) - 0.2).abs() < 1e-6);
    }

    #[test]
    fn the_first_bolt_of_a_volley_takes_no_draw() {
        struct Counting(LegacyRandomSource, u32);
        impl RandomSource for Counting {
            fn set_seed(&mut self, s: i64) {
                self.0.set_seed(s);
            }
            fn next_i32(&mut self) -> i32 {
                self.0.next_i32()
            }
            fn next_i32_bound(&mut self, b: i32) -> i32 {
                self.0.next_i32_bound(b)
            }
            fn next_i64(&mut self) -> i64 {
                self.0.next_i64()
            }
            fn next_bool(&mut self) -> bool {
                self.0.next_bool()
            }
            fn next_f32(&mut self) -> f32 {
                self.1 += 1;
                self.0.next_f32()
            }
            fn next_f64(&mut self) -> f64 {
                self.0.next_f64()
            }
            fn next_gaussian(&mut self) -> f64 {
                self.0.next_gaussian()
            }
        }
        let mut r = Counting(LegacyRandomSource::new(1), 0);
        assert_eq!(crossbow_shot_pitch(&mut r, 0), 1.0);
        assert_eq!(r.1, 0);
        let p1 = crossbow_shot_pitch(&mut r, 1);
        assert_eq!(r.1, 1);
        assert!(p1 > 0.63 && p1 < 1.2, "{p1}");
        let p2 = crossbow_shot_pitch(&mut r, 2);
        assert!(p2 > 0.43 && p2 < 1.0, "{p2}");
    }

    #[test]
    fn the_arrow_starts_a_float_tenth_below_the_eye() {
        let p = arrow_spawn_position(Vec3::new(1.0, 64.0, 2.0), 1.62);
        assert_eq!(p.x, 1.0);
        assert_eq!(p.z, 2.0);
        // The double 0.1 would give a different last bit.
        assert_eq!(
            p.y.to_bits(),
            (64.0 + 1.62 - ARROW_SPAWN_BELOW_EYE).to_bits()
        );
        assert_ne!(ARROW_SPAWN_BELOW_EYE, 0.1);
    }

    #[test]
    fn the_multishot_fan_is_refused_rather_than_approximated() {
        assert!(crossbow_shot_direction(0.0, 0.0, 10.0).is_none());
        let v = crossbow_shot_direction(0.0, 0.0, 0.0).unwrap();
        assert!((v.z - 1.0).abs() < 1e-6, "{v:?}");
    }
}
