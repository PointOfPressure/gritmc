//! `ProjectileUtil`'s entity hit resolution, and the two predicates that
//! decide what a projectile is allowed to hit.
//!
//! # `computeMargin` is new in 26.2 and it is not a constant
//!
//! ```text
//!   ProjectileUtil.computeMargin(Entity e):
//!       Math.max(0.0f, Math.min(0.3f, (e.tickCount - 2) / 20.0f))
//! ```
//!
//! Every earlier version inflated the target's box by a flat `0.3`. In 26.2
//! the inflation **ramps from zero to 0.3 over the projectile's first eight
//! ticks** — `tickCount` 2 gives 0, `tickCount` 8 gives 0.3 — so a projectile
//! is exactly its own hitbox for its first two ticks and forgiving
//! afterwards. Porting the old constant makes a point-blank shot hit
//! something vanilla misses, which is not a rounding difference: it is a
//! different outcome on the very shot a player is most likely to notice.
//!
//! The subtraction is on the **int** `tickCount` and the division is float,
//! so `tickCount = 0` and `tickCount = 1` both clamp at zero through
//! `Math.max` rather than producing a negative inflation.
//!
//! # The margin arm is a THREE-clip probe, not an inflation
//!
//! `getManyEntityHitResult`'s per-entity body is, in order:
//!
//! ```text
//!   AABB raw = e.getBoundingBox();
//!   if (includeInside && raw.contains(from))      -> hit at `from`, next entity
//!   if (raw.clip(from, to).isPresent())           -> hit there,     next entity
//!   if (margin <= 0.0)                            -> next entity
//!   Optional<Vec3> loose = raw.inflate(margin).clip(from, to);
//!   if (loose.isEmpty())                          -> next entity
//!   Vec3 centre = raw.getCenter();
//!   BlockHitResult wall = level.clip(new ClipContext(loose, centre, block, NONE, proj));
//!   if (wall.getType() != MISS) centre = wall.getLocation();
//!   if (raw.clip(loose, centre).isPresent())      -> hit there
//! ```
//!
//! So the margin does **not** widen the hitbox. It widens the search, and a
//! near miss is then converted into a real hit only if a second ray, from
//! where the loose box was entered towards the target's centre, reaches the
//! raw box without a block in the way. A port that simply inflated the box
//! would hit through a wall, and would report the hit position on the
//! inflated surface rather than on the entity.
//!
//! # `getEntityHitResult` and `getManyEntityHitResult` are different functions
//!
//! The single-result form inflates the box by the margin and takes the
//! **nearest** hit with a strict `<`. The many-result form does the
//! three-clip probe above and returns everything, unsorted, in whatever order
//! `Level.getEntities` yielded. `AbstractArrow` uses the many form and sorts
//! the result itself, by the distance from the arrow's position to the target
//! **entity's position** — not to the hit point, which is a different order
//! whenever two hitboxes overlap the ray.

use crate::collide::{Aabb, CollisionBlocks, CollisionShapes, NoShapeFor, Vec3};
use crate::projectile::clip;
use crate::projectile::math;

/// One candidate target, as the hit path reads it.
///
/// A struct rather than a live entity handle because the entity kinds this
/// server holds are not the entity kinds vanilla's predicates are written
/// against, and the join is named here rather than assumed: `mob-in-world`
/// owns the mob, `entity-tracker` owns who sees it, and this module only
/// needs a box, an id and three booleans.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Target {
    /// `Entity.getId()`.
    pub id: i32,
    /// `Entity.getBoundingBox()`.
    pub box_: Aabb,
    /// `Entity.position()` — the sort key `AbstractArrow.stepMoveAndHit`
    /// uses, which is the entity's position and not its box centre.
    pub pos: Vec3,
    /// `Entity.canBeHitByProjectile()` — `isAlive() && isPickable() &&
    /// !isSpectator()` on `Entity`.
    pub can_be_hit_by_projectile: bool,
    /// `Entity.isPassengerOfSameVehicle(owner)`.
    pub shares_vehicle_with_owner: bool,
    /// `Player.canHarmPlayer(target)` when both the owner and the target are
    /// players; `true` for every other pairing, because
    /// `AbstractArrow.canHitEntity`'s first block only fires when both are.
    pub owner_may_harm: bool,
}

/// `Level.getEntities(Entity except, AABB, Predicate)`, as the projectile
/// path needs it.
///
/// **The iteration order is a parity subject and it is not this lane's.**
/// `docs/cold-start/entity-id-and-order-keyed.md` derives it: the section
/// walk is x numerically ascending then z and y as *unsigned* packed fields,
/// and within a section it is `ClassInstanceMultiMap`'s insertion order. The
/// only place the order reaches an outcome here is which of two targets at
/// the same distance is hit first, and this server's `Entities::all` sorts by
/// id. That is a named difference, not a silent one.
pub trait ProjectileTargets {
    /// Every entity whose bounding box intersects `box_`, excluding the
    /// projectile itself, in the level's own order.
    fn targets_in(&self, box_: Aabb) -> Vec<Target>;
}

/// `ProjectileUtil.computeMargin(Entity)`.
///
/// `Math.max(0.0f, Math.min(0.3f, (tickCount - 2) / 20.0f))`, in float.
#[must_use]
pub fn compute_margin(tick_count: i32) -> f32 {
    #[expect(
        clippy::cast_precision_loss,
        reason = "vanilla's i2f on the int tickCount"
    )]
    let ramp = (tick_count - 2) as f32 / 20.0_f32;
    0.0_f32.max(0.3_f32.min(ramp))
}

/// `Projectile.canHitEntity(Entity)` with `AbstractArrow`'s override in front
/// of it.
///
/// ```text
///   AbstractArrow: if (target instanceof Player p && getOwner() instanceof
///                      Player o && !o.canHarmPlayer(p)) return false;
///                  return super.canHitEntity(target)
///                      && (piercingIgnoreEntityIds == null
///                          || !piercingIgnoreEntityIds.contains(target.getId()));
///   Projectile:    if (!target.canBeHitByProjectile()) return false;
///                  Entity owner = getOwner();
///                  return owner == null || leftOwner
///                      || !owner.isPassengerOfSameVehicle(target);
/// ```
///
/// **`leftOwner` is the second disjunct, not a guard on the whole test.** A
/// projectile that has left its owner's collision range may hit the owner's
/// own vehicle mates; one that has not may not. And `leftOwner` is latched:
/// `checkLeftOwner` only ever sets it true.
#[must_use]
pub fn can_hit_entity(t: &Target, has_owner: bool, left_owner: bool, pierced: &[i32]) -> bool {
    if !t.owner_may_harm {
        return false;
    }
    if !t.can_be_hit_by_projectile {
        return false;
    }
    if has_owner && !left_owner && t.shares_vehicle_with_owner {
        return false;
    }
    !pierced.contains(&t.id)
}

/// An `EntityHitResult`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityHit {
    pub target: Target,
    /// `EntityHitResult.getLocation()`.
    pub location: Vec3,
}

/// `ProjectileUtil.getManyEntityHitResult(level, projectile, from, to, box,
/// filter, margin, blockShape, includeInside)`.
///
/// Returns the hits in the order the level yielded the entities; the caller
/// sorts. `include_inside` is `false` for every arrow call
/// (`AbstractArrow.findHitEntities` passes `iconst_0`) and is carried because
/// `AbstractWindCharge` passes true.
///
/// # Errors
///
/// [`NoShapeFor`] from the line-of-sight clip, which walks real blocks.
#[expect(clippy::too_many_arguments, reason = "vanilla's arity")]
pub fn many_entity_hit_result<W, T>(
    shapes: &CollisionShapes,
    world: &W,
    targets: &T,
    from: Vec3,
    to: Vec3,
    search: Aabb,
    margin: f32,
    include_inside: bool,
    accept: &dyn Fn(&Target) -> bool,
) -> Result<Vec<EntityHit>, NoShapeFor>
where
    W: CollisionBlocks,
    T: ProjectileTargets,
{
    let mut out = Vec::new();
    for t in targets.targets_in(search) {
        if !accept(&t) {
            continue;
        }
        let raw = t.box_;
        if include_inside && math::contains(&raw, from) {
            out.push(EntityHit {
                target: t,
                location: from,
            });
            continue;
        }
        if let Some((at, _)) = math::clip(&raw, from, to) {
            out.push(EntityHit {
                target: t,
                location: at,
            });
            continue;
        }
        if f64::from(margin) <= 0.0 {
            continue;
        }
        let Some((loose, _)) = math::clip(&math::inflate(&raw, f64::from(margin)), from, to) else {
            continue;
        };
        let mut centre = math::get_center(&raw);
        let wall = clip::clip(shapes, world, loose, centre)?;
        if !wall.is_miss() {
            centre = wall.location();
        }
        if let Some((at, _)) = math::clip(&raw, loose, centre) {
            out.push(EntityHit {
                target: t,
                location: at,
            });
        }
    }
    Ok(out)
}

/// `ProjectileUtil.getEntityHitResult(level, entity, from, to, box, filter,
/// margin)` — the single-nearest form.
///
/// Every candidate's box is inflated by `margin` unconditionally here (there
/// is no three-clip probe on this path), clipped, and the strictly nearest
/// crossing by `from.distanceToSqr(hit)` wins. Ties keep the **earlier**
/// entity, because the comparison is `<` against the running best.
#[must_use]
pub fn nearest_entity_hit<T: ProjectileTargets>(
    targets: &T,
    from: Vec3,
    to: Vec3,
    search: Aabb,
    margin: f32,
    accept: &dyn Fn(&Target) -> bool,
) -> Option<EntityHit> {
    let mut best: Option<EntityHit> = None;
    let mut best_d = f64::MAX;
    for t in targets.targets_in(search) {
        if !accept(&t) {
            continue;
        }
        let inflated = math::inflate(&t.box_, f64::from(margin));
        if let Some((at, _)) = math::clip(&inflated, from, to) {
            let d = math::distance_to_sqr(from, at);
            if d < best_d {
                best_d = d;
                best = Some(EntityHit {
                    target: t,
                    location: at,
                });
            }
        }
    }
    best
}

/// The search box both forms are given: the projectile's own box expanded by
/// this tick's delta and then inflated by 1.0 on every face.
///
/// `AbstractArrow.findHitEntities` and `Projectile.isOutsideOwnerCollisionRange`
/// build the identical expression, `getBoundingBox().expandTowards(
/// getDeltaMovement()).inflate(1.0)`.
#[must_use]
pub fn search_box(box_: Aabb, delta: Vec3) -> Aabb {
    math::inflate(&box_.expand_towards(delta.x, delta.y, delta.z), 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_margin_ramps_over_the_first_eight_ticks() {
        assert_eq!(compute_margin(0), 0.0);
        assert_eq!(compute_margin(1), 0.0);
        assert_eq!(compute_margin(2), 0.0);
        assert_eq!(compute_margin(3), 0.05);
        assert_eq!(compute_margin(8), 0.3);
        assert_eq!(compute_margin(1000), 0.3);
        // Positive control for the pre-26.2 reading: a flat 0.3 would give
        // 0.3 on the first tick, and it does not.
        assert!(compute_margin(2) < 0.3);
    }

    fn t(id: i32) -> Target {
        Target {
            id,
            box_: Aabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0),
            pos: Vec3::new(0.5, 0.0, 0.5),
            can_be_hit_by_projectile: true,
            shares_vehicle_with_owner: false,
            owner_may_harm: true,
        }
    }

    #[test]
    fn a_target_that_cannot_be_hit_by_a_projectile_is_refused() {
        let mut x = t(1);
        assert!(can_hit_entity(&x, true, false, &[]));
        x.can_be_hit_by_projectile = false;
        assert!(!can_hit_entity(&x, true, false, &[]));
    }

    #[test]
    fn left_owner_reopens_the_owners_vehicle_mates() {
        let mut x = t(1);
        x.shares_vehicle_with_owner = true;
        assert!(!can_hit_entity(&x, true, false, &[]));
        assert!(can_hit_entity(&x, true, true, &[]));
        // With no owner at all the vehicle test never runs.
        assert!(can_hit_entity(&x, false, false, &[]));
    }

    #[test]
    fn a_pierced_id_is_refused_and_a_different_one_is_not() {
        let x = t(7);
        assert!(!can_hit_entity(&x, true, true, &[3, 7]));
        assert!(can_hit_entity(&x, true, true, &[3, 9]));
    }

    struct Fixed(Vec<Target>);
    impl ProjectileTargets for Fixed {
        fn targets_in(&self, _box_: Aabb) -> Vec<Target> {
            self.0.clone()
        }
    }

    #[test]
    fn nearest_keeps_the_first_of_two_at_the_same_distance() {
        let mut a = t(1);
        let mut b = t(2);
        a.box_ = Aabb::new(1.0, 0.0, 0.0, 2.0, 1.0, 1.0);
        b.box_ = Aabb::new(1.0, 0.0, 0.0, 2.0, 1.0, 1.0);
        let f = Fixed(vec![a, b]);
        let hit = nearest_entity_hit(
            &f,
            Vec3::new(0.0, 0.5, 0.5),
            Vec3::new(3.0, 0.5, 0.5),
            Aabb::new(-9.0, -9.0, -9.0, 9.0, 9.0, 9.0),
            0.0,
            &|_| true,
        )
        .unwrap();
        assert_eq!(hit.target.id, 1, "a strict < keeps the earlier entity");
    }

    #[test]
    fn the_search_box_is_expanded_then_inflated() {
        let b = Aabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
        let s = search_box(b, Vec3::new(2.0, 0.0, 0.0));
        assert_eq!(s.min_x, -1.0);
        assert_eq!(s.max_x, 4.0);
        // Inflating first and expanding second would give max_x 4.0 too but
        // min_x -1.0 either way; the y faces are the discriminator.
        assert_eq!(s.min_y, -1.0);
        assert_eq!(s.max_y, 2.0);
    }
}
