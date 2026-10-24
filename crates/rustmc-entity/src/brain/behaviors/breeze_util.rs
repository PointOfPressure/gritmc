//! `net.minecraft.world.entity.monster.breeze.BreezeUtil`, the two static
//! helpers the breeze's four behaviours share, from the 26.2 jar.
//!
//! ```text
//! MAX_LINE_OF_SIGHT_TEST_RANGE = 50.0d
//!
//! randomPointBehindTarget(LivingEntity target, RandomSource random)  (0..61)
//!    0  int MAX_ANGLE = 90                                    // bipush 90, unused after
//!    3  float yRot = target.yHeadRot + 180.0f
//!               + (float) random.nextGaussian() * 90.0f / 2.0f
//!   24  float dist = Mth.lerp(random.nextFloat(), 4.0f, 8.0f)
//!   39  Vec3 v = Vec3.directionFromRotation(0.0f, yRot).scale(dist)
//!   52  return target.position().add(v)
//!
//! hasLineOfSight(Breeze breeze, Vec3 to)  (0..72)
//!    0  Vec3 from = new Vec3(breeze.getX(), breeze.getY(), breeze.getZ())
//!   20  if to.distanceTo(from) > getMaxLineOfSightTestRange(breeze): return false
//!   35  return breeze.level().clip(new ClipContext(from, to, COLLIDER, NONE, breeze))
//!               .getType() == HitResult.Type.MISS
//!
//! getMaxLineOfSightTestRange(Breeze)  (0..13)
//!    0  return Math.max(50.0d, breeze.getAttributeValue(FOLLOW_RANGE))
//! ```
//!
//! **The `from` of the sight test is the breeze's feet, not its eyes** — a
//! bare `new Vec3(getX(), getY(), getZ())`, which is what `position()`
//! answers, rather than `getEyePosition()`. Every ray the breeze casts to
//! choose a slide spot or a jump landing therefore starts on the ground.
//!
//! **The draw order in `randomPointBehindTarget` is gaussian then float**,
//! and both come from the mob's own `RandomSource` — the caller passes
//! `breeze.getRandom()` at all three call sites (`Slide.start` offset 125,
//! `LongJump.canRun` offset 110). Two draws, in that order, on every call.
//!
//! **The `MAX_ANGLE` local is dead.** `bipush 90` is stored at offset 0 and
//! never read; the `90.0f` at offset 17 is a separate `ldc`. So the spread
//! is `nextGaussian() * 90 / 2` degrees about the direction the target's
//! head faces, reversed.
//!
//! `Vec3.directionFromRotation(0.0f, yRot)` is
//! [`crate::mth::direction_from_rotation`], the same function
//! `LongJumpUtil` uses.

use crate::ai::mob_ai::MobAi;
use crate::host::{MobHost, PathLevel};
use crate::mth;
use crate::phys::Vec3;
use rustmc_worldgen::rng::RandomSource;

/// `BreezeUtil.MAX_LINE_OF_SIGHT_TEST_RANGE`.
pub const MAX_LINE_OF_SIGHT_TEST_RANGE: f64 = 50.0;

/// `getMaxLineOfSightTestRange(Breeze)`.
#[must_use]
pub fn max_line_of_sight_test_range<M: MobHost>(mob: &M) -> f64 {
    MAX_LINE_OF_SIGHT_TEST_RANGE.max(mob.follow_range())
}

/// `BreezeUtil.hasLineOfSight(Breeze, Vec3)`.
pub fn has_line_of_sight<M: MobHost>(cx: &mut MobAi<M>, to: Vec3) -> bool {
    let from = Vec3::new(cx.mob.x(), cx.mob.y(), cx.mob.z());
    if to.distance_to(from) > max_line_of_sight_test_range(&cx.mob) {
        return false;
    }
    let id = cx.mob.id();
    cx.mob.level().clip_misses(id, from, to, false)
}

/// `BreezeUtil.randomPointBehindTarget(LivingEntity, RandomSource)`.
///
/// `target_y_head_rot` is the target's `yHeadRot` field and
/// `target_position` its `position()`; the random is the **mob's**, drawn
/// gaussian first and then float.
pub fn random_point_behind_target<M: MobHost>(
    cx: &mut MobAi<M>,
    target_y_head_rot: f32,
    target_position: Vec3,
) -> Vec3 {
    let gaussian = cx.mob.random().next_gaussian() as f32;
    let y_rot = target_y_head_rot + 180.0 + gaussian * 90.0 / 2.0;
    let next_float = cx.mob.random().next_f32();
    let dist = mth::lerp_f32(next_float, 4.0, 8.0);
    let v = mth::direction_from_rotation(0.0, y_rot).scale(f64::from(dist));
    target_position + v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{GridLevel, TestMob};
    use rustmc_worldgen::rng::LegacyRandomSource;

    #[test]
    fn the_point_behind_a_target_draws_gaussian_then_float_from_the_mob() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.0, 1.0, 0.0, 42));
        // A second, independent replay of the same two draws in the same
        // order — a cross-check on the draw order, not a jar golden.
        let mut twin = LegacyRandomSource::new(42);
        let g = twin.next_gaussian() as f32;
        let f = twin.next_f32();
        let y_rot = 30.0f32 + 180.0 + g * 90.0 / 2.0;
        let expect = Vec3::new(5.0, 1.0, 5.0)
            + mth::direction_from_rotation(0.0, y_rot).scale(f64::from(mth::lerp_f32(f, 4.0, 8.0)));

        let got = random_point_behind_target(&mut cx, 30.0, Vec3::new(5.0, 1.0, 5.0));
        assert_eq!(got, expect);
    }

    #[test]
    fn the_sight_range_is_the_larger_of_fifty_and_the_follow_range() {
        let mut mob = TestMob::new(GridLevel::new(), 0.0, 1.0, 0.0, 1);
        mob.follow_range = 24.0;
        assert_eq!(max_line_of_sight_test_range(&mob), 50.0);
        mob.follow_range = 64.0;
        assert_eq!(max_line_of_sight_test_range(&mob), 64.0);
    }

    #[test]
    fn a_point_beyond_the_range_is_refused_without_a_ray() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.0, 1.0, 0.0, 1));
        cx.mob.follow_range = 24.0;
        assert!(!has_line_of_sight(&mut cx, Vec3::new(0.0, 1.0, 51.0)));
        assert!(has_line_of_sight(&mut cx, Vec3::new(0.0, 1.0, 49.0)));
    }
}
