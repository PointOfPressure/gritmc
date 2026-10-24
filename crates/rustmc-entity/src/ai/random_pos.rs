//! `net.minecraft.world.entity.ai.util.{RandomPos, LandRandomPos,
//! DefaultRandomPos, GoalUtils}`, from the 26.2 jar — where the stroll
//! and panic goals get their targets, and where their random draws are.
//!
//! # Draw order
//!
//! Every draw is on `mob.getRandom()` and is named at the method that
//! makes it:
//!
//! * `RandomPos.generateRandomDirection(random, h, v)` — three
//!   `nextInt(2h+1)`, `nextInt(2v+1)`, `nextInt(2h+1)`, in that order
//!   (offsets 0..42), so **x, y, z**.
//! * `RandomPos.generateRandomDirectionWithinRadians(random, ...)` —
//!   `nextFloat()` at offset 17 (the angle), `nextDouble()` at 33 (the
//!   radius), then `nextInt(2v+1)` at 103 for y — but only if the
//!   `|dx| <= maxDist && |dz| <= maxDist` test at 74..91 passes; a
//!   rejected direction has drawn two and not the third.
//! * `RandomPos.generateRandomPosTowardDirection(mob, d, random, pos)` —
//!   for a homed mob with `d > 1`, one `nextDouble()` for x (offset 52 or
//!   72, whichever side of home) and one for z (103 or 123).
//! * `LandRandomPos.getPosAway(mob, ...)` — two `nextDouble()` (29 and 43)
//!   only when the away vector has zero length.
//! * `RandomPos.generateRandomPos(supplier, scorer)` — ten attempts
//!   (`RANDOM_POS_ATTEMPTS`), each one supplier call, keeping the highest
//!   score with a **strict** `>` (offset 49: `dcmpl; ifle`), so an equal
//!   score does not replace the earlier candidate; `null` candidates are
//!   skipped but still count as attempts.
//!
//! The scorer for every stroll goal is `PathfinderMob::getWalkTargetValue`,
//! `0.0f` by default, so with the default the first non-null candidate
//! wins.

use rustmc_data::BlockPos;
use rustmc_worldgen::rng::RandomSource;

use super::mob_ai::MobAi;
use crate::host::{MobHost, PathLevel};
use crate::mth::{atan2, lerp, SQRT_OF_TWO};
use crate::pathfinder::context::get_path_type_static;
use crate::pathfinder::{NodeEvaluator, PathfindingContext};
use crate::phys::{above, block_pos_containing, closer_to_center_than, Vec3};

/// `RandomPos.RANDOM_POS_ATTEMPTS`.
pub const RANDOM_POS_ATTEMPTS: i32 = 10;

/// `RandomPos.generateRandomDirection(RandomSource, int, int)`.
#[must_use]
pub fn generate_random_direction<R: RandomSource>(random: &mut R, h: i32, v: i32) -> BlockPos {
    let x = random.next_i32_bound(2 * h + 1) - h;
    let y = random.next_i32_bound(2 * v + 1) - v;
    let z = random.next_i32_bound(2 * h + 1) - h;
    BlockPos { x, y, z }
}

/// `RandomPos.moveUpOutOfSolid(BlockPos, int maxY, Predicate isSolid)`:
/// from a solid block, step up until not solid or past `maxY`.
pub fn move_up_out_of_solid(
    pos: BlockPos,
    max_y: i32,
    mut is_solid: impl FnMut(BlockPos) -> bool,
) -> BlockPos {
    if is_solid(pos) {
        let mut m = above(pos);
        while m.y <= max_y && is_solid(m) {
            m = above(m);
        }
        return m;
    }
    pos
}

/// `RandomPos.generateRandomPos(Supplier<BlockPos>, ToDoubleFunction<BlockPos>)`,
/// offsets 0..80: ten candidates, best strict score, as a bottom-centre
/// vector.
pub fn generate_random_pos(
    mut supplier: impl FnMut() -> Option<BlockPos>,
    mut scorer: impl FnMut(BlockPos) -> f64,
) -> Option<Vec3> {
    let mut best_score = f64::NEG_INFINITY;
    let mut best: Option<BlockPos> = None;
    for _ in 0..RANDOM_POS_ATTEMPTS {
        let Some(candidate) = supplier() else {
            continue;
        };
        let score = scorer(candidate);
        if score > best_score {
            best_score = score;
            best = Some(candidate);
        }
    }
    best.map(Vec3::at_bottom_center_of)
}

/// `RandomPos.generateRandomPosTowardDirection(PathfinderMob, double, RandomSource, BlockPos)`,
/// offsets 0..165: for a homed mob with `d > 1`, pull the x and z
/// offsets half a random step toward home; then the mob's position plus
/// the offset, floored.
pub fn generate_random_pos_toward_direction<M: MobHost>(
    mob: &mut M,
    d: f64,
    dir: BlockPos,
) -> BlockPos {
    let mut x = f64::from(dir.x);
    let mut z = f64::from(dir.z);
    if mob.has_home() && d > 1.0 {
        let home = mob.home_position();
        if mob.x() > f64::from(home.x) {
            x -= mob.random().next_f64() * d / 2.0;
        } else {
            x += mob.random().next_f64() * d / 2.0;
        }
        if mob.z() > f64::from(home.z) {
            z -= mob.random().next_f64() * d / 2.0;
        } else {
            z += mob.random().next_f64() * d / 2.0;
        }
    }
    block_pos_containing(x + mob.x(), f64::from(dir.y) + mob.y(), z + mob.z())
}

/// `GoalUtils.mobRestricted(PathfinderMob, double)`:
/// `hasHome() && homePosition.closerToCenterThan(position(), homeRadius + d + 1.0)`.
#[must_use]
pub fn mob_restricted<M: MobHost>(mob: &M, d: f64) -> bool {
    mob.has_home()
        && closer_to_center_than(
            mob.home_position(),
            mob.position(),
            f64::from(mob.home_radius()) + d + 1.0,
        )
}

/// `GoalUtils.isOutsideLimits(BlockPos, PathfinderMob)`: `level.isOutsideBuildHeight(pos.y)`.
#[must_use]
pub fn is_outside_limits<M: MobHost>(pos: BlockPos, mob: &M) -> bool {
    mob.level().is_outside_build_height(pos.y)
}

/// `GoalUtils.isRestricted(boolean, PathfinderMob, BlockPos)`:
/// `restricted && !mob.isWithinHome(pos)`.
#[must_use]
pub fn is_restricted<M: MobHost>(restricted: bool, mob: &M, pos: BlockPos) -> bool {
    restricted && !mob.is_within_home_pos(pos)
}

/// `GoalUtils.isNotStable(PathNavigation, BlockPos)`: `!nav.isStableDestination(pos)`.
#[must_use]
pub fn is_not_stable<M: MobHost>(ai: &MobAi<M>, pos: BlockPos) -> bool {
    !ai.navigation.is_stable_destination(&ai.mob, pos)
}

/// `GoalUtils.isWater(PathfinderMob, BlockPos)`: `level.getFluidState(pos).is(WATER)`.
#[must_use]
pub fn is_water<M: MobHost>(mob: &M, pos: BlockPos) -> bool {
    mob.level().block_facts(pos).fluid_is_water
}

/// `GoalUtils.hasMalus(PathfinderMob, BlockPos)`:
/// `mob.getPathfindingMalus(WalkNodeEvaluator.getPathTypeStatic(mob, pos)) != 0.0f`.
#[must_use]
pub fn has_malus<M: MobHost>(mob: &M, pos: BlockPos) -> bool {
    let level = mob.level();
    let ctx = PathfindingContext::new(mob, level, level.path_type_cache());
    let t = get_path_type_static(&ctx, pos.x, pos.y, pos.z);
    mob.pathfinding_malus(t) != 0.0
}

/// `GoalUtils.isSolid(PathfinderMob, BlockPos)`: `getBlockState(pos).isSolid()`.
#[must_use]
pub fn is_solid<M: MobHost>(mob: &M, pos: BlockPos) -> bool {
    mob.level().block_facts(pos).is_solid
}

/// `GoalUtils.hasGroundPathNavigation(Mob)`: `getNavigation().canNavigateGround()`.
#[must_use]
pub fn has_ground_path_navigation<M: MobHost>(ai: &MobAi<M>) -> bool {
    ai.navigation.can_navigate_ground()
}

/// `DefaultRandomPos.getPos(PathfinderMob, int, int)`.
pub fn default_get_pos<M: MobHost>(ai: &mut MobAi<M>, h: i32, v: i32) -> Option<Vec3> {
    let restricted = mob_restricted(&ai.mob, f64::from(h));
    generate_random_pos_mob(ai, |ai| {
        let dir = generate_random_direction(ai.mob.random(), h, v);
        default_generate_random_pos_toward_direction(ai, h, restricted, dir)
    })
}

/// `DefaultRandomPos.getPosTowards(PathfinderMob, int, int, Vec3, double)`.
pub fn default_get_pos_towards<M: MobHost>(
    ai: &mut MobAi<M>,
    h: i32,
    v: i32,
    toward: Vec3,
    max_angle: f64,
) -> Option<Vec3> {
    let dir = toward.subtract3(ai.mob.x(), ai.mob.y(), ai.mob.z());
    let restricted = mob_restricted(&ai.mob, f64::from(h));
    generate_random_pos_mob(ai, |ai| {
        let d = generate_random_direction_within_radians_impl(
            ai.mob.random(),
            0.0,
            f64::from(h),
            v,
            0,
            dir.x,
            dir.z,
            max_angle,
        )?;
        default_generate_random_pos_toward_direction(ai, h, restricted, d)
    })
}

/// `DefaultRandomPos.getPosAway(PathfinderMob, int, int, Vec3)`.
pub fn default_get_pos_away<M: MobHost>(
    ai: &mut MobAi<M>,
    h: i32,
    v: i32,
    away_from: Vec3,
) -> Option<Vec3> {
    let dir = ai.mob.position().subtract(away_from);
    let restricted = mob_restricted(&ai.mob, f64::from(h));
    generate_random_pos_mob(ai, |ai| {
        let d = generate_random_direction_within_radians_impl(
            ai.mob.random(),
            0.0,
            f64::from(h),
            v,
            0,
            dir.x,
            dir.z,
            1.570_796_370_506_286_6,
        )?;
        default_generate_random_pos_toward_direction(ai, h, restricted, d)
    })
}

/// `DefaultRandomPos.generateRandomPosTowardDirection(PathfinderMob, int, boolean, BlockPos)`:
/// the shared one, then refused when outside the build height, outside a
/// restricted home, not a stable destination, or carrying a malus.
fn default_generate_random_pos_toward_direction<M: MobHost>(
    ai: &mut MobAi<M>,
    h: i32,
    restricted: bool,
    dir: BlockPos,
) -> Option<BlockPos> {
    let pos = generate_random_pos_toward_direction(&mut ai.mob, f64::from(h), dir);
    if is_outside_limits(pos, &ai.mob)
        || is_restricted(restricted, &ai.mob, pos)
        || is_not_stable(ai, pos)
        || has_malus(&ai.mob, pos)
    {
        return None;
    }
    Some(pos)
}

/// `LandRandomPos.getPos(PathfinderMob, int, int)`: the mob's own
/// `getWalkTargetValue` as the scorer.
pub fn land_get_pos<M: MobHost>(ai: &mut MobAi<M>, h: i32, v: i32) -> Option<Vec3> {
    let restricted = mob_restricted(&ai.mob, f64::from(h));
    generate_random_pos_mob(ai, |ai| {
        let dir = generate_random_direction(ai.mob.random(), h, v);
        let pos = land_generate_random_pos_toward_direction(ai, f64::from(h), restricted, dir)?;
        land_move_pos_up_out_of_solid(&ai.mob, pos)
    })
}

/// `LandRandomPos.getPosTowards(PathfinderMob, int, int, Vec3)`.
pub fn land_get_pos_towards<M: MobHost>(
    ai: &mut MobAi<M>,
    h: i32,
    v: i32,
    toward: Vec3,
) -> Option<Vec3> {
    let dir = toward.subtract3(ai.mob.x(), ai.mob.y(), ai.mob.z());
    let restricted = mob_restricted(&ai.mob, f64::from(h));
    land_get_pos_in_direction(ai, 0.0, f64::from(h), v, dir, restricted)
}

/// `LandRandomPos.getPosAway(PathfinderMob, int, int, Vec3)`, and the
/// `(PathfinderMob, double, double, int, Vec3)` form it forwards to: when
/// the mob stands exactly on the point, a random direction (two
/// `nextDouble()` at offsets 29 and 43) stands in.
pub fn land_get_pos_away<M: MobHost>(
    ai: &mut MobAi<M>,
    h: i32,
    v: i32,
    away_from: Vec3,
) -> Option<Vec3> {
    land_get_pos_away_dist(ai, 0.0, f64::from(h), v, away_from)
}

/// `LandRandomPos.getPosAway(PathfinderMob, double, double, int, Vec3)`.
pub fn land_get_pos_away_dist<M: MobHost>(
    ai: &mut MobAi<M>,
    min_dist: f64,
    max_dist: f64,
    v: i32,
    away_from: Vec3,
) -> Option<Vec3> {
    let mut dir = ai.mob.position().subtract(away_from);
    if dir.length() == 0.0 {
        let rx = ai.mob.random().next_f64() - 0.5;
        let rz = ai.mob.random().next_f64() - 0.5;
        dir = Vec3::new(rx, 0.0, rz);
    }
    let restricted = mob_restricted(&ai.mob, max_dist);
    land_get_pos_in_direction(ai, min_dist, max_dist, v, dir, restricted)
}

/// `LandRandomPos.getPosInDirection(...)`: `RandomPos.generateRandomPos(mob, supplier)`
/// with the radians direction, the shared toward-direction step, then
/// `movePosUpOutOfSolid`.
fn land_get_pos_in_direction<M: MobHost>(
    ai: &mut MobAi<M>,
    min_dist: f64,
    max_dist: f64,
    v: i32,
    dir: Vec3,
    restricted: bool,
) -> Option<Vec3> {
    generate_random_pos_mob(ai, |ai| {
        let d = generate_random_direction_within_radians_impl(
            ai.mob.random(),
            min_dist,
            max_dist,
            v,
            0,
            dir.x,
            dir.z,
            1.570_796_370_506_286_6,
        )?;
        let pos = land_generate_random_pos_toward_direction(ai, max_dist, restricted, d)?;
        land_move_pos_up_out_of_solid(&ai.mob, pos)
    })
}

/// `LandRandomPos.movePosUpOutOfSolid(PathfinderMob, BlockPos)`: up out
/// of solid to at most `level.getMaxY()`, then refused if in water or
/// carrying a malus.
pub fn land_move_pos_up_out_of_solid<M: MobHost>(mob: &M, pos: BlockPos) -> Option<BlockPos> {
    let max_y = mob.level().max_y();
    let pos = move_up_out_of_solid(pos, max_y, |p| is_solid(mob, p));
    if is_water(mob, pos) || has_malus(mob, pos) {
        return None;
    }
    Some(pos)
}

/// `LandRandomPos.generateRandomPosTowardDirection(PathfinderMob, double, boolean, BlockPos)`:
/// the shared one, refused outside the build height, a restricted home,
/// or an unstable destination — no malus test here, that comes after
/// `movePosUpOutOfSolid`.
fn land_generate_random_pos_toward_direction<M: MobHost>(
    ai: &mut MobAi<M>,
    d: f64,
    restricted: bool,
    dir: BlockPos,
) -> Option<BlockPos> {
    let pos = generate_random_pos_toward_direction(&mut ai.mob, d, dir);
    if is_outside_limits(pos, &ai.mob)
        || is_restricted(restricted, &ai.mob, pos)
        || is_not_stable(ai, pos)
    {
        return None;
    }
    Some(pos)
}

/// `RandomPos.generateRandomPos(PathfinderMob, Supplier<BlockPos>)`:
/// `generateRandomPos(supplier, mob::getWalkTargetValue)`.
fn generate_random_pos_mob<M: MobHost>(
    ai: &mut MobAi<M>,
    mut supplier: impl FnMut(&mut MobAi<M>) -> Option<BlockPos>,
) -> Option<Vec3> {
    let mut best_score = f64::NEG_INFINITY;
    let mut best: Option<BlockPos> = None;
    for _ in 0..RANDOM_POS_ATTEMPTS {
        let Some(candidate) = supplier(ai) else {
            continue;
        };
        let score = f64::from(ai.mob.walk_target_value(candidate));
        if score > best_score {
            best_score = score;
            best = Some(candidate);
        }
    }
    best.map(Vec3::at_bottom_center_of)
}

/// The real `generateRandomDirectionWithinRadians`, offsets 0..126:
///
/// ```text
/// base   = Mth.atan2(dirZ, dirX) - (double) 1.5707964f       (0..11)
/// angle  = base + (double)(2.0f * nextFloat() - 1.0f) * maxAngle   (13..30)
/// radius = sqrt(nextDouble()) * Mth.lerp(minDist, maxDist... )
/// ```
///
/// where offsets 32..51 are `sqrt(nextDouble())`, then
/// `Mth.lerp(that, minDist, maxDist)`, then `* (double) SQRT_OF_TWO`:
/// the random square root is the lerp's **delta**. Then
/// `dx = -radius * sin(angle)`, `dz = radius * cos(angle)`; refuse when
/// `|dx| > maxDist || |dz| > maxDist`; else `y = nextInt(2*yRange+1) - yRange + yOffset`
/// and `BlockPos.containing(dx, y, dz)`.
#[allow(clippy::too_many_arguments)]
fn generate_random_direction_within_radians_impl<R: RandomSource>(
    random: &mut R,
    min_dist: f64,
    max_dist: f64,
    y_range: i32,
    y_offset: i32,
    dir_x: f64,
    dir_z: f64,
    max_angle: f64,
) -> Option<BlockPos> {
    let base = atan2(dir_z, dir_x) - 1.570_796_370_506_286_6;
    let angle = base + f64::from(2.0f32 * random.next_f32() - 1.0f32) * max_angle;
    let t = random.next_f64().sqrt();
    let radius = lerp(t, min_dist, max_dist) * f64::from(SQRT_OF_TWO);
    let dx = -radius * angle.sin();
    let dz = radius * angle.cos();
    if dx.abs() > max_dist || dz.abs() > max_dist {
        return None;
    }
    let y = random.next_i32_bound(2 * y_range + 1) - y_range + y_offset;
    Some(block_pos_containing(dx, f64::from(y), dz))
}

/// The public name for the radians direction; see the `_impl` for the
/// bytecode.
#[allow(clippy::too_many_arguments)]
pub fn random_direction_within_radians<R: RandomSource>(
    random: &mut R,
    min_dist: f64,
    max_dist: f64,
    y_range: i32,
    y_offset: i32,
    dir_x: f64,
    dir_z: f64,
    max_angle: f64,
) -> Option<BlockPos> {
    generate_random_direction_within_radians_impl(
        random, min_dist, max_dist, y_range, y_offset, dir_x, dir_z, max_angle,
    )
}

/// `NodeEvaluator.getPathType(Mob, BlockPos)` through a navigation's
/// evaluator, as `GoalUtils.hasMalus` needs it without one.
#[must_use]
pub fn path_type_static_for<M: MobHost>(mob: &M, pos: BlockPos) -> crate::pathfinder::PathType {
    let level = mob.level();
    let ctx = PathfindingContext::new(mob, level, level.path_type_cache());
    let eval = crate::pathfinder::WalkNodeEvaluator::new();
    NodeEvaluator::<M, M::Level>::get_path_type(&eval, &ctx, pos.x, pos.y, pos.z)
}

/// `RandomPos.moveUpToAboveSolid(BlockPos, int aboveSolidAmount, int maxY, Predicate isSolid)`,
/// offsets 0..139: from a solid block, climb out of the solid, then up to
/// `aboveSolidAmount` more while under `maxY`, stepping back down one on
/// meeting a solid.
pub fn move_up_to_above_solid(
    pos: BlockPos,
    above_solid_amount: i32,
    max_y: i32,
    mut is_solid: impl FnMut(BlockPos) -> bool,
) -> BlockPos {
    assert!(
        above_solid_amount >= 0,
        "aboveSolidAmount was {above_solid_amount}, expected >= 0"
    );
    if !is_solid(pos) {
        return pos;
    }
    let mut m = above(pos);
    while m.y <= max_y && is_solid(m) {
        m = above(m);
    }
    let k = m.y;
    while m.y <= max_y && m.y - k < above_solid_amount {
        m = above(m);
        if is_solid(m) {
            m = crate::phys::below(m);
            break;
        }
    }
    m
}

/// `HoverRandomPos.getPos(PathfinderMob, int h, int v, double dirX, double dirZ, float maxAngle, int yMin, int yMax)`:
/// per attempt a direction within the angle (three draws), the land
/// candidate toward it, lifted `nextInt(yMax - yMin + 1) + yMin` above
/// the solid; water or a malus refuses it.
#[allow(clippy::too_many_arguments)]
pub fn hover_get_pos<M: MobHost>(
    ai: &mut MobAi<M>,
    h: i32,
    v: i32,
    dir_x: f64,
    dir_z: f64,
    max_angle: f32,
    y_min: i32,
    y_max: i32,
) -> Option<Vec3> {
    let restricted = mob_restricted(&ai.mob, f64::from(h));
    generate_random_pos_mob(ai, |ai| {
        let dir = generate_random_direction_within_radians_impl(
            ai.mob.random(),
            0.0,
            f64::from(h),
            v,
            0,
            dir_x,
            dir_z,
            f64::from(max_angle),
        )?;
        let pos = land_generate_random_pos_toward_direction(ai, f64::from(h), restricted, dir)?;
        let lift = ai.mob.random().next_i32_bound(y_max - y_min + 1) + y_min;
        let max_y = ai.mob.level().max_y();
        let pos = move_up_to_above_solid(pos, lift, max_y, |p| is_solid(&ai.mob, p));
        if is_water(&ai.mob, pos) || has_malus(&ai.mob, pos) {
            return None;
        }
        Some(pos)
    })
}

/// `AirAndWaterRandomPos.getPos(PathfinderMob, int h, int v, int yOffset, double dirX, double dirZ, double maxAngle)`.
pub fn air_and_water_get_pos<M: MobHost>(
    ai: &mut MobAi<M>,
    h: i32,
    v: i32,
    y_offset: i32,
    dir_x: f64,
    dir_z: f64,
    max_angle: f64,
) -> Option<Vec3> {
    let restricted = mob_restricted(&ai.mob, f64::from(h));
    generate_random_pos_mob(ai, |ai| {
        air_and_water_generate_random_pos(ai, h, v, y_offset, dir_x, dir_z, max_angle, restricted)
    })
}

/// `AirAndWaterRandomPos.generateRandomPos(...)`, offsets 0..96.
#[allow(clippy::too_many_arguments)]
pub fn air_and_water_generate_random_pos<M: MobHost>(
    ai: &mut MobAi<M>,
    h: i32,
    v: i32,
    y_offset: i32,
    dir_x: f64,
    dir_z: f64,
    max_angle: f64,
    restricted: bool,
) -> Option<BlockPos> {
    let dir = generate_random_direction_within_radians_impl(
        ai.mob.random(),
        0.0,
        f64::from(h),
        v,
        y_offset,
        dir_x,
        dir_z,
        max_angle,
    )?;
    let pos = generate_random_pos_toward_direction(&mut ai.mob, f64::from(h), dir);
    if is_outside_limits(pos, &ai.mob) || is_restricted(restricted, &ai.mob, pos) {
        return None;
    }
    let max_y = ai.mob.level().max_y();
    let pos = move_up_out_of_solid(pos, max_y, |p| is_solid(&ai.mob, p));
    if has_malus(&ai.mob, pos) {
        return None;
    }
    Some(pos)
}

/// `BehaviorUtils.getRandomSwimmablePos(PathfinderMob, int, int)`, offsets
/// 0..55: `DefaultRandomPos.getPos(h, v)`, re-drawn up to ten more times
/// while the block it lands in is not `isPathfindable(WATER)`; the
/// eleventh answer is returned unchecked.
pub fn get_random_swimmable_pos<M: MobHost>(ai: &mut MobAi<M>, h: i32, v: i32) -> Option<Vec3> {
    let mut v3 = default_get_pos(ai, h, v);
    let mut i = 0;
    while let Some(p) = v3 {
        if ai
            .mob
            .level()
            .block_facts(crate::phys::block_pos_containing_vec(p))
            .is_pathfindable_water
        {
            break;
        }
        let n = i;
        i += 1;
        if n >= 10 {
            break;
        }
        v3 = default_get_pos(ai, h, v);
    }
    v3
}

/// `LandRandomPos.getPos(PathfinderMob, int, int, ToDoubleFunction<BlockPos>)`:
/// the same ten land candidates as [`land_get_pos`], scored by the given
/// function instead of the mob's `getWalkTargetValue`; best strict score.
pub fn land_get_pos_scored<M: MobHost>(
    ai: &mut MobAi<M>,
    h: i32,
    v: i32,
    mut scorer: impl FnMut(&MobAi<M>, BlockPos) -> f64,
) -> Option<Vec3> {
    let restricted = mob_restricted(&ai.mob, f64::from(h));
    let mut best_score = f64::NEG_INFINITY;
    let mut best: Option<BlockPos> = None;
    for _ in 0..RANDOM_POS_ATTEMPTS {
        let dir = generate_random_direction(ai.mob.random(), h, v);
        let Some(pos) =
            land_generate_random_pos_toward_direction(ai, f64::from(h), restricted, dir)
        else {
            continue;
        };
        let Some(candidate) = land_move_pos_up_out_of_solid(&ai.mob, pos) else {
            continue;
        };
        let score = scorer(ai, candidate);
        if score > best_score {
            best_score = score;
            best = Some(candidate);
        }
    }
    best.map(Vec3::at_bottom_center_of)
}
