//! The `Vec3` and `AABB` operations the projectile path uses and
//! [`crate::collide`] does not already carry.
//!
//! `crate::collide::Vec3` and `crate::collide::Aabb` exist for the block
//! sweep, which needs `get`/`with`/`lengthSqr` and nothing else. The
//! projectile path needs `normalize`, `add`, `scale`, `multiply`,
//! `horizontalDistance`, `distanceToSqr`, `contains`, `getCenter`,
//! `inflate` and `clip`. They are added here as free functions over those
//! same types rather than as methods on them, so that a lane reading
//! `collide.rs` for the collision sweep still sees exactly the surface that
//! sweep uses.
//!
//! Every one is transcribed from `net.minecraft.world.phys.Vec3` and
//! `net.minecraft.world.phys.AABB` in the pinned 26.2 jar.

use crate::collide::{Aabb, Vec3};
use rustmc_data::BlockPos;
use rustmc_entity::mth::lerp;

/// `Vec3.add(double, double, double)`.
#[must_use]
pub fn add3(v: Vec3, x: f64, y: f64, z: f64) -> Vec3 {
    Vec3::new(v.x + x, v.y + y, v.z + z)
}

/// `Vec3.add(Vec3)`.
#[must_use]
pub fn add(a: Vec3, b: Vec3) -> Vec3 {
    add3(a, b.x, b.y, b.z)
}

/// `Vec3.subtract(Vec3)`.
#[must_use]
pub fn subtract(a: Vec3, b: Vec3) -> Vec3 {
    add3(a, -b.x, -b.y, -b.z)
}

/// `Vec3.scale(double)` — which is `multiply(d, d, d)`.
#[must_use]
pub fn scale(v: Vec3, s: f64) -> Vec3 {
    multiply(v, s, s, s)
}

/// `Vec3.multiply(double, double, double)`.
#[must_use]
pub fn multiply(v: Vec3, x: f64, y: f64, z: f64) -> Vec3 {
    Vec3::new(v.x * x, v.y * y, v.z * z)
}

/// `Vec3.length()`.
#[must_use]
pub fn length(v: Vec3) -> f64 {
    (v.x * v.x + v.y * v.y + v.z * v.z).sqrt()
}

/// `Vec3.horizontalDistance()` — `sqrt(x*x + z*z)`.
///
/// Not `horizontalDistanceSqr`, which [`crate::collide::Vec3`] already has:
/// `shoot` and `updateRotation` both feed the **rooted** value to
/// `Mth.atan2` as its second argument and the square would be a different
/// angle, not a scaled one.
#[must_use]
pub fn horizontal_distance(v: Vec3) -> f64 {
    v.horizontal_distance_sqr().sqrt()
}

/// `Vec3.normalize()`.
///
/// The `ldc2_w` guard is `9.999999747378752E-6` — the float literal `1.0E-5f`
/// widened — and the bytecode's constant is used rather than the decimal it
/// was written as.
#[must_use]
pub fn normalize(v: Vec3) -> Vec3 {
    let len = length(v);
    if len < 9.999_999_747_378_752E-6 {
        Vec3::ZERO
    } else {
        Vec3::new(v.x / len, v.y / len, v.z / len)
    }
}

/// `Vec3.distanceToSqr(Vec3)`.
#[must_use]
pub fn distance_to_sqr(a: Vec3, b: Vec3) -> f64 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let dz = b.z - a.z;
    dx * dx + dy * dy + dz * dz
}

/// `AABB.contains(Vec3)` — half-open on the max faces, closed on the min.
///
/// ```text
///   x >= minX && x < maxX && y >= minY && y < maxY && z >= minZ && z < maxZ
/// ```
#[must_use]
pub fn contains(b: &Aabb, v: Vec3) -> bool {
    v.x >= b.min_x
        && v.x < b.max_x
        && v.y >= b.min_y
        && v.y < b.max_y
        && v.z >= b.min_z
        && v.z < b.max_z
}

/// `AABB.getCenter()` — `Mth.lerp(0.5, min, max)` per axis.
///
/// `Mth.lerp(double, double, double)` is `start + delta * (end - start)`,
/// **not** `(start + end) / 2`; the two differ in the last bit for many
/// inputs and this value is fed to a ray clip.
#[must_use]
pub fn get_center(b: &Aabb) -> Vec3 {
    Vec3::new(
        lerp(0.5, b.min_x, b.max_x),
        lerp(0.5, b.min_y, b.max_y),
        lerp(0.5, b.min_z, b.max_z),
    )
}

/// `AABB.inflate(double)` — the same scalar on all six faces.
#[must_use]
pub fn inflate(b: &Aabb, d: f64) -> Aabb {
    Aabb::new(
        b.min_x - d,
        b.min_y - d,
        b.min_z - d,
        b.max_x + d,
        b.max_y + d,
        b.max_z + d,
    )
}

/// `AABB.move(BlockPos)`.
#[must_use]
pub fn moved_by_block(b: &Aabb, p: BlockPos) -> Aabb {
    b.shifted(f64::from(p.x), f64::from(p.y), f64::from(p.z))
}

/// The face `AABB.clip` reports, in `Direction`'s own declaration order.
///
/// Carried because `BlockHitResult` carries it and `Projectile.onHitBlock`'s
/// callers read it; the clip itself only ever compares it against `null`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Face {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

/// `AABB.clip(Vec3 from, Vec3 to)`.
///
/// Returns the point at which the segment `from -> to` first crosses into the
/// box, or `None` if it does not. The `1.0E-7` margins are vanilla's and they
/// are not symmetric with the axis test: `clipPoint` widens the box by 1e-7
/// on **both** faces of the two axes it is not clipping, and `getDirection`
/// only considers an axis whose delta exceeds 1e-7 in magnitude.
///
/// ```text
///   getDirection: for each of x, y, z, in that order
///       if  delta >  1.0E-7  clipPoint against the MIN face, dir WEST/DOWN/NORTH
///       if  delta < -1.0E-7  clipPoint against the MAX face, dir EAST/UP/SOUTH
///   clipPoint(t[], best, dDrive, dA, dB, faceDrive, minA, maxA, minB, maxB,
///             dir, fromDrive, fromA, fromB):
///       h = (faceDrive - fromDrive) / dDrive
///       a = fromA + h * dA;  b = fromB + h * dB
///       if 0 < h && h < t[0]
///          && minA - 1e-7 < a && a < maxA + 1e-7
///          && minB - 1e-7 < b && b < maxB + 1e-7   then t[0] = h, return dir
///       else return best
/// ```
///
/// `t[0]` starts at `1.0`, so a crossing beyond the end of the segment is not
/// a hit, and the strict `h < t[0]` is what makes the **first** face win when
/// two are crossed at the same parameter.
///
/// # `h` must be STRICTLY positive, and this lane had it as `>=`
///
/// The guard at `clipPoint` offsets 29-33 is `dconst_0; dload h; dcmpg;
/// ifge 101` — it jumps to `return best` when `0.0 >= h`, so a crossing at
/// `h == 0.0` is **not** a hit. A segment whose start lies exactly on a face
/// and whose end is inside the box therefore misses.
///
/// This was written as `0.0 <= h` from a reading of the bytecode and
/// `ci/projectile-golden.sh` caught it on the first run: `AABB(0,0,0,1,1,1)`
/// against the segment `(0.0, 0.5, 0.5) -> (0.5, 0.5, 0.5)` is a miss in
/// vanilla and was a hit here. It is the difference between a projectile
/// spawned flush against a hitbox passing through it and stopping dead.
#[must_use]
pub fn clip(b: &Aabb, from: Vec3, to: Vec3) -> Option<(Vec3, Face)> {
    let mut t = 1.0_f64;
    let face = clip_shared(b, from, to, &mut t)?;
    Some((
        add3(
            from,
            t * (to.x - from.x),
            t * (to.y - from.y),
            t * (to.z - from.z),
        ),
        face,
    ))
}

/// `AABB.getDirection(minX..maxZ, from, t[], best, dx, dy, dz)` with the
/// shared `t` exposed.
///
/// `AABB.clip(Iterable<AABB>, ...)` runs this over every box of a shape with
/// **one** `t` array, so a later box can only win by crossing strictly
/// nearer. `Some` here means this box moved `t`; `None` means it did not,
/// which is not the same as "did not intersect" — a box crossed further away
/// than the running best also returns `None`.
#[must_use]
pub fn clip_shared(b: &Aabb, from: Vec3, to: Vec3, t: &mut f64) -> Option<Face> {
    let mut face: Option<Face> = None;
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let dz = to.z - from.z;

    if dx > 1.0E-7 {
        face = clip_point(
            t,
            face,
            dx,
            dy,
            dz,
            b.min_x,
            b.min_y,
            b.max_y,
            b.min_z,
            b.max_z,
            Face::West,
            from.x,
            from.y,
            from.z,
        );
    } else if dx < -1.0E-7 {
        face = clip_point(
            t,
            face,
            dx,
            dy,
            dz,
            b.max_x,
            b.min_y,
            b.max_y,
            b.min_z,
            b.max_z,
            Face::East,
            from.x,
            from.y,
            from.z,
        );
    }
    if dy > 1.0E-7 {
        face = clip_point(
            t,
            face,
            dy,
            dz,
            dx,
            b.min_y,
            b.min_z,
            b.max_z,
            b.min_x,
            b.max_x,
            Face::Down,
            from.y,
            from.z,
            from.x,
        );
    } else if dy < -1.0E-7 {
        face = clip_point(
            t,
            face,
            dy,
            dz,
            dx,
            b.max_y,
            b.min_z,
            b.max_z,
            b.min_x,
            b.max_x,
            Face::Up,
            from.y,
            from.z,
            from.x,
        );
    }
    if dz > 1.0E-7 {
        face = clip_point(
            t,
            face,
            dz,
            dx,
            dy,
            b.min_z,
            b.min_x,
            b.max_x,
            b.min_y,
            b.max_y,
            Face::North,
            from.z,
            from.x,
            from.y,
        );
    } else if dz < -1.0E-7 {
        face = clip_point(
            t,
            face,
            dz,
            dx,
            dy,
            b.max_z,
            b.min_x,
            b.max_x,
            b.min_y,
            b.max_y,
            Face::South,
            from.z,
            from.x,
            from.y,
        );
    }
    face
}

/// `AABB.clipPoint`. The driving axis is the one being crossed; `a` and `b`
/// are the other two, in the rotation vanilla's three call pairs use
/// (x -> (y, z), y -> (z, x), z -> (x, y)).
#[expect(clippy::too_many_arguments, reason = "vanilla's clipPoint arity")]
fn clip_point(
    t: &mut f64,
    best: Option<Face>,
    d_drive: f64,
    d_a: f64,
    d_b: f64,
    face_drive: f64,
    min_a: f64,
    max_a: f64,
    min_b: f64,
    max_b: f64,
    dir: Face,
    from_drive: f64,
    from_a: f64,
    from_b: f64,
) -> Option<Face> {
    let h = (face_drive - from_drive) / d_drive;
    let a = from_a + h * d_a;
    let b = from_b + h * d_b;
    if 0.0 < h
        && h < *t
        && min_a - 1.0E-7 < a
        && a < max_a + 1.0E-7
        && min_b - 1.0E-7 < b
        && b < max_b + 1.0E-7
    {
        *t = h;
        Some(dir)
    } else {
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_matches_the_widened_float_guard() {
        // Just under `1.0E-5f` widened: the guard fires and the answer is ZERO.
        let v = Vec3::new(9.0E-6, 0.0, 0.0);
        assert_eq!(normalize(v), Vec3::ZERO);
        // Just over: it does not, and a unit vector comes back.
        let v = Vec3::new(1.1E-5, 0.0, 0.0);
        assert!((normalize(v).x - 1.0).abs() < 1e-12);
    }

    #[test]
    fn get_center_is_lerp_not_a_midpoint() {
        // A box whose faces sum inexactly: `(a+b)/2` and `a + 0.5*(b-a)`
        // differ in the last bit, and this is the value a hit ray is aimed at.
        let b = Aabb::new(0.1, 0.0, 0.0, 0.300_000_000_000_000_04, 1.0, 1.0);
        let want: f64 = 0.1 + 0.5 * (0.300_000_000_000_000_04 - 0.1);
        assert_eq!(get_center(&b).x.to_bits(), want.to_bits());
    }

    #[test]
    fn contains_is_closed_below_and_open_above() {
        let b = Aabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
        assert!(contains(&b, Vec3::new(0.0, 0.0, 0.0)));
        assert!(!contains(&b, Vec3::new(1.0, 0.5, 0.5)));
        assert!(contains(&b, Vec3::new(0.999_999, 0.5, 0.5)));
    }

    #[test]
    fn clip_takes_the_near_face_and_reports_it() {
        let b = Aabb::new(1.0, 0.0, 0.0, 2.0, 1.0, 1.0);
        let (hit, face) = clip(&b, Vec3::new(0.0, 0.5, 0.5), Vec3::new(3.0, 0.5, 0.5)).unwrap();
        assert_eq!(face, Face::West);
        assert!((hit.x - 1.0).abs() < 1e-12);
        // Positive control: the same box from the other side reports EAST.
        let (_, face) = clip(&b, Vec3::new(3.0, 0.5, 0.5), Vec3::new(0.0, 0.5, 0.5)).unwrap();
        assert_eq!(face, Face::East);
    }

    #[test]
    fn clip_refuses_a_crossing_beyond_the_segment() {
        // `t[0]` starts at 1.0, so the box at x=5 is not hit by a segment
        // that stops at x=3 even though the ray would reach it.
        let b = Aabb::new(5.0, 0.0, 0.0, 6.0, 1.0, 1.0);
        assert!(clip(&b, Vec3::new(0.0, 0.5, 0.5), Vec3::new(3.0, 0.5, 0.5)).is_none());
    }

    #[test]
    fn clip_ignores_an_axis_whose_delta_is_under_the_epsilon() {
        // dx is 5e-8, under 1.0E-7, so the x faces are never considered and
        // a box the segment only enters through them is missed.
        let b = Aabb::new(1.0, 0.0, 0.0, 2.0, 1.0, 1.0);
        assert!(clip(
            &b,
            Vec3::new(0.999_999_99, 0.5, 0.5),
            Vec3::new(1.000_000_04, 0.5, 0.5)
        )
        .is_none());
    }

    #[test]
    fn horizontal_distance_is_rooted() {
        let v = Vec3::new(3.0, 100.0, 4.0);
        assert_eq!(horizontal_distance(v), 5.0);
        assert_eq!(v.horizontal_distance_sqr(), 25.0);
    }
}
