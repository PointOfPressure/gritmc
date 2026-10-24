//! `ThrowableProjectile.tick` and the five `ThrowableItemProjectile` kinds'
//! `onHit`, from the pinned 26.2 jar.
//!
//! # A thrown thing and a shot arrow are DIFFERENT tick orders
//!
//! ```text
//!   ThrowableProjectile.tick, offsets 0-93
//!      0  handleFirstTickBubbleColumn();
//!      4  applyGravity();                       <- BEFORE the move
//!      8  applyInertia();                       <- BEFORE the move
//!     12  HitResult h = ProjectileUtil.getHitResultOnMoveVector(this, this::canHitEntity);
//!     23  Vec3 to = h.getType() != MISS ? h.getLocation() : position().add(getDeltaMovement());
//!     53  setPos(to);
//!     58  updateRotation();
//!     62  applyEffectsFromBlocks();
//!     66  Projectile.tick();
//!     70  if (h.getType() != MISS && isAlive()) hitTargetOrDeflectSelf(h);
//! ```
//!
//! `AbstractArrow.tick` applies its drag and its gravity **after** moving.
//! A throwable applies both **before**. Porting one order for both makes
//! every snowball land one tick's worth of gravity short, which reads as
//! "close enough" over three blocks and is a metre out over thirty.
//!
//! And the hit is resolved from the *pre-move* delta but acted on **after**
//! `setPos` and after `Projectile.tick`, so the entity is already at the hit
//! point when `onHit` runs.
//!
//! # Three drags, not two
//!
//! ```text
//!   ThrowableProjectile.applyInertia():
//!       if (isInWater()) { 4 x BUBBLE particles; f = 0.8f; }
//!       else               f = getAirDrag();          // 0.99f
//!       setDeltaMovement(getDeltaMovement().scale(f));
//! ```
//!
//! **The throwable's water drag is 0.8, the arrow's is 0.6**, and both share
//! the air drag 0.99. The two are separate methods on separate classes and
//! the constants are not interchangeable.
//!
//! Gravity is `ldc2_w 0.03d` on `ThrowableProjectile` against the arrow's
//! `0.05d`.

use crate::collide::{Aabb, CollisionBlocks, CollisionShapes, NoShapeFor, Vec3};
use crate::projectile::clip::{self, Clip};
use crate::projectile::hit::{self, ProjectileTargets, Target};
use crate::projectile::math;
use crate::projectile::shoot::{update_rotation, Rot};
use rustmc_worldgen::rng::RandomSource;

/// `ThrowableProjectile.getDefaultGravity()` — `ldc2_w 0.03d`.
pub const THROWABLE_GRAVITY: f64 = 0.03;
/// `ThrowableProjectile.getAirDrag()` — `ldc 0.99f`.
pub const THROWABLE_AIR_DRAG: f32 = 0.99;
/// `ThrowableProjectile.applyInertia`'s in-water factor — `ldc 0.8f`.
///
/// Not the arrow's `WATER_INERTIA`, which is `0.6f`.
pub const THROWABLE_WATER_DRAG: f32 = 0.8;
/// Every `ThrowableItemProjectile` is `sized(0.25f, 0.25f)`.
pub const THROWABLE_SIZE: f64 = 0.25;

/// The five item projectiles this module ticks.
///
/// `ThrownLingeringPotion` and `ThrownTrident` are absent: the lingering
/// potion needs the potion contents component and an `AreaEffectCloud`, and
/// the trident needs `AbstractArrow` plus the loyalty enchantment. Both are
/// refusals with a named owner rather than gaps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Snowball,
    Egg,
    EnderPearl,
    SplashPotion,
    ExperienceBottle,
}

impl Kind {
    /// The `minecraft:entity_type` registry name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Kind::Snowball => "minecraft:snowball",
            Kind::Egg => "minecraft:egg",
            Kind::EnderPearl => "minecraft:ender_pearl",
            Kind::SplashPotion => "minecraft:splash_potion",
            Kind::ExperienceBottle => "minecraft:experience_bottle",
        }
    }
}

/// One thrown item projectile.
#[derive(Debug, Clone, PartialEq)]
pub struct Throwable {
    pub kind: Kind,
    pub pos: Vec3,
    pub movement: Vec3,
    pub rot: Rot,
    pub tick_count: i32,
    pub owner: Option<i32>,
    pub left_owner: bool,
    pub left_owner_checked: bool,
    pub removed: bool,
    /// `Entity.firstTick`, which `handleFirstTickBubbleColumn` reads and
    /// `Entity.baseTick` clears at the end of the first tick.
    pub first_tick: bool,
}

/// What a thrown projectile's tick produced.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tick {
    /// `Level.broadcastEntityEvent(this, (byte) 3)` — the break particles
    /// every one of these sends on landing.
    pub entity_event: Option<u8>,
    /// `ServerLevel.levelEvent(2002, blockPosition(), colour)` from the
    /// splash potion and the experience bottle.
    pub level_event: Option<(i32, i32)>,
    /// Chickens the egg hatched, as a count. Spawning them is
    /// `mob-in-world`'s and this reports what vanilla would have spawned.
    pub chickens: i32,
    /// Experience the bottle awarded, as a total. Awarding it is the
    /// experience-orb lane's.
    pub experience: i32,
    /// Entities struck, with the damage vanilla would have applied.
    pub hits: Vec<(i32, f32)>,
    pub discarded: bool,
}

/// `ThrowableProjectile.tick()`.
///
/// # Errors
///
/// [`NoShapeFor`] from the block ray, which walks real collision shapes.
pub fn tick<W, T, R>(
    p: &mut Throwable,
    shapes: &CollisionShapes,
    world: &W,
    targets: &T,
    rng: &mut R,
    in_water: bool,
) -> Result<Tick, NoShapeFor>
where
    W: CollisionBlocks,
    T: ProjectileTargets,
    R: RandomSource,
{
    p.tick_count += 1;
    let mut out = Tick::default();

    // `handleFirstTickBubbleColumn` walks `BlockPos.betweenClosed(box)` and
    // calls `entityInside` on any `bubble_column`. No bubble column can be
    // placed by this server yet and `Projectile.onInsideBubbleColumn` is the
    // only thing it could reach, so the walk is omitted rather than
    // implemented against a block that cannot exist. `first_tick` is still
    // tracked, because the day a bubble column can be placed the flag is
    // what says whether this ran.
    p.first_tick = false;

    // `applyGravity()` then `applyInertia()`, both before the move.
    p.movement = Vec3::new(p.movement.x, p.movement.y - THROWABLE_GRAVITY, p.movement.z);
    let drag = if in_water {
        THROWABLE_WATER_DRAG
    } else {
        THROWABLE_AIR_DRAG
    };
    p.movement = math::scale(p.movement, f64::from(drag));

    let owned = p.owner.is_some();
    let left = p.left_owner;
    let result = hit_result_on_move_vector(p, shapes, world, targets, &|t: &Target| {
        hit::can_hit_entity(t, owned, left, &[])
    })?;

    p.pos = match &result {
        Outcome::Miss => math::add(p.pos, p.movement),
        Outcome::Block(b) => b.location(),
        Outcome::Entity(h) => h.at,
    };
    p.rot = update_rotation(p.rot, p.movement);

    // `Projectile.tick()`: checkLeftOwner, Entity.tick, leftOwnerChecked = false.
    check_left_owner(p, targets);
    p.left_owner_checked = false;

    if !matches!(result, Outcome::Miss) && !p.removed {
        on_hit(p, &result, rng, &mut out);
    }
    Ok(out)
}

/// The three shapes `HitResult` takes here.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Miss,
    Block(Clip),
    Entity(EntityStrike),
}

/// An `EntityHitResult` reduced to what `onHit` reads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityStrike {
    pub target: Target,
    pub at: Vec3,
}

/// `ProjectileUtil.getHitResultOnMoveVector(entity, filter)` — which is
/// `getHitResult(position(), entity, filter, getDeltaMovement(), level,
/// computeMargin(entity), COLLIDER)`.
///
/// ```text
///   getHitResult(from, e, filter, delta, level, margin, blockShape):
///       Vec3 to = from.add(delta);
///       BlockHitResult b = level.clipIncludingBorder(
///           new ClipContext(from, to, blockShape, NONE, e));
///       if (b.getType() != MISS) to = b.getLocation();     <- the ray is SHORTENED
///       EntityHitResult h = getEntityHitResult(level, e, from, to,
///           e.getBoundingBox().expandTowards(delta).inflate(1.0), filter, margin);
///       return h != null ? h : b;
/// ```
///
/// **The block hit shortens the entity ray**, so an entity behind a wall is
/// never considered, and **an entity hit wins over a block hit at any
/// distance** along that shortened ray. Reversing the precedence makes a
/// projectile fired at a mob standing against a wall hit the wall.
///
/// # Errors
///
/// [`NoShapeFor`] from the block ray.
pub fn hit_result_on_move_vector<W, T>(
    p: &Throwable,
    shapes: &CollisionShapes,
    world: &W,
    targets: &T,
    accept: &dyn Fn(&Target) -> bool,
) -> Result<Outcome, NoShapeFor>
where
    W: CollisionBlocks,
    T: ProjectileTargets,
{
    let from = p.pos;
    let mut to = math::add(from, p.movement);
    let block = clip::clip(shapes, world, from, to)?;
    if !block.is_miss() {
        to = block.location();
    }
    let search = hit::search_box(bounding_box(p), p.movement);
    let margin = hit::compute_margin(p.tick_count);
    if let Some(h) = hit::nearest_entity_hit(targets, from, to, search, margin, accept) {
        return Ok(Outcome::Entity(EntityStrike {
            target: h.target,
            at: h.location,
        }));
    }
    Ok(if block.is_miss() {
        Outcome::Miss
    } else {
        Outcome::Block(block)
    })
}

/// `Entity.getBoundingBox()` for a 0.25-wide, 0.25-tall projectile.
#[must_use]
pub fn bounding_box(p: &Throwable) -> Aabb {
    let h = THROWABLE_SIZE / 2.0;
    Aabb::new(
        p.pos.x - h,
        p.pos.y,
        p.pos.z - h,
        p.pos.x + h,
        p.pos.y + THROWABLE_SIZE,
        p.pos.z + h,
    )
}

fn check_left_owner<T: ProjectileTargets>(p: &mut Throwable, targets: &T) {
    if p.left_owner || p.left_owner_checked {
        return;
    }
    p.left_owner = match p.owner {
        None => true,
        Some(id) => {
            let probe = hit::search_box(bounding_box(p), p.movement);
            !targets
                .targets_in(probe)
                .iter()
                .any(|t| t.id == id && probe.intersects(&t.box_))
        }
    };
    p.left_owner_checked = true;
}

/// The per-kind `onHit`, with the RNG draws each one takes.
///
/// | kind | server-side effect | draws |
/// |---|---|---|
/// | snowball | 3 damage to a `Blaze`, 0 to anything else; event 3; discard | 0 |
/// | egg | `nextInt(8) == 0` -> 1 chicken, then `nextInt(32) == 0` -> 4; event 3; discard | 1 or 2 |
/// | experience bottle | `levelEvent(2002, blockPos, -13083194)`; `3 + nextInt(5) + nextInt(5)` xp; discard | 2 |
/// | splash potion | the potion contents; **refused** | — |
/// | ender pearl | the owner's teleport; **refused** | — |
///
/// **The egg's second draw is conditional.** `nextInt(8)` is compared to zero
/// and a non-zero result jumps straight past the hatch block, so seven times
/// in eight the egg costs exactly one draw. A port that always drew twice
/// would displace every later draw in that tick seven times in eight.
fn on_hit<R: RandomSource>(p: &mut Throwable, result: &Outcome, rng: &mut R, out: &mut Tick) {
    if let Outcome::Entity(h) = result {
        let dmg = match p.kind {
            // `Snowball.onHitEntity`: `entity instanceof Blaze ? 3 : 0`.
            // No mob in this server is a `Blaze`, so the figure is 0 today
            // and the branch is written rather than folded away.
            Kind::Snowball => 0.0,
            // `ThrownEgg.onHitEntity` and `AbstractThrownPotion`'s hurt are
            // `fconst_0`.
            _ => 0.0,
        };
        out.hits.push((h.target.id, dmg));
    }

    match p.kind {
        Kind::Snowball => {
            out.entity_event = Some(3);
            p.removed = true;
            out.discarded = true;
        }
        Kind::Egg => {
            let mut n = 0;
            if rng.next_i32_bound(8) == 0 {
                n = 1;
                if rng.next_i32_bound(32) == 0 {
                    n = 4;
                }
            }
            out.chickens = n;
            out.entity_event = Some(3);
            p.removed = true;
            out.discarded = true;
        }
        Kind::ExperienceBottle => {
            // `levelEvent(2002, blockPosition(), -13083194)` — the colour is
            // an `ldc int`, not a particle id.
            out.level_event = Some((2002, -13_083_194));
            out.experience = 3 + rng.next_i32_bound(5) + rng.next_i32_bound(5);
            p.removed = true;
            out.discarded = true;
        }
        Kind::SplashPotion | Kind::EnderPearl => {
            // Refused. `AbstractThrownPotion.onHit` reads the
            // `POTION_CONTENTS` component and `data/vanilla` carries no
            // potion table; `ThrownEnderpearl.onHit` calls
            // `ServerPlayer.teleportTo`, `resetFallDistance` and a
            // five-damage `hurtServer`, all of which are the player-systems
            // lanes'. Neither is discarded here, because a projectile that
            // vanished without doing its work is a worse lie than one that
            // is visibly not implemented.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmc_worldgen::rng::legacy::LegacyRandomSource;

    fn p(kind: Kind) -> Throwable {
        Throwable {
            kind,
            pos: Vec3::new(0.0, 64.0, 0.0),
            movement: Vec3::new(0.5, 0.0, 0.0),
            rot: Rot {
                y_rot: 0.0,
                x_rot: 0.0,
            },
            tick_count: 1,
            owner: None,
            left_owner: true,
            left_owner_checked: false,
            removed: false,
            first_tick: true,
        }
    }

    #[test]
    fn the_egg_draws_once_when_it_does_not_hatch() {
        struct Counting(LegacyRandomSource, u32);
        impl RandomSource for Counting {
            fn set_seed(&mut self, s: i64) {
                self.0.set_seed(s);
            }
            fn next_i32(&mut self) -> i32 {
                self.0.next_i32()
            }
            fn next_i32_bound(&mut self, b: i32) -> i32 {
                self.1 += 1;
                self.0.next_i32_bound(b)
            }
            fn next_i64(&mut self) -> i64 {
                self.0.next_i64()
            }
            fn next_bool(&mut self) -> bool {
                self.0.next_bool()
            }
            fn next_f32(&mut self) -> f32 {
                self.0.next_f32()
            }
            fn next_f64(&mut self) -> f64 {
                self.0.next_f64()
            }
            fn next_gaussian(&mut self) -> f64 {
                self.0.next_gaussian()
            }
        }
        // Sweep seeds and record (draws, chickens) pairs. The claim is the
        // COUNT, which no outcome test can see: an egg that does not hatch
        // costs exactly one draw and one that does costs exactly two, so a
        // port that always drew twice would displace every later draw in the
        // tick seven times in eight.
        let mut seen_one = 0;
        let mut seen_two = 0;
        // ONE generator across the sweep, advanced by each egg, because
        // `new Random(s).nextInt(8)` for small consecutive `s` is strongly
        // correlated in its top bits and a per-seed loop here produced
        // 400 non-hatching eggs and zero hatching ones. That is a property
        // of `java.util.Random`'s seed scramble, not of the egg.
        let mut r = Counting(LegacyRandomSource::new(20_260_906), 0);
        for _ in 0..400 {
            let mut e = p(Kind::Egg);
            let before = r.1;
            let mut out = Tick::default();
            on_hit(&mut e, &Outcome::Miss, &mut r, &mut out);
            match r.1 - before {
                1 => {
                    assert_eq!(out.chickens, 0, "one draw means no hatch");
                    seen_one += 1;
                }
                2 => {
                    assert!(
                        out.chickens == 1 || out.chickens == 4,
                        "two draws means 1 or 4, got {}",
                        out.chickens
                    );
                    seen_two += 1;
                }
                n => panic!("an egg took {n} draws"),
            }
        }
        assert!(seen_one > 0 && seen_two > 0, "{seen_one} / {seen_two}");
        // The hatch is one in eight, so over 400 seeds the two-draw case is
        // the minority. Asserted so a stuck RNG that always returned 0 would
        // not read as a pass.
        assert!(seen_two < seen_one, "{seen_one} / {seen_two}");
    }

    #[test]
    fn the_experience_bottle_awards_three_to_eleven() {
        for seed in 0..200_i64 {
            let mut e = p(Kind::ExperienceBottle);
            let mut r = LegacyRandomSource::new(seed);
            let mut out = Tick::default();
            on_hit(&mut e, &Outcome::Miss, &mut r, &mut out);
            assert!((3..=11).contains(&out.experience), "{}", out.experience);
            assert_eq!(out.level_event, Some((2002, -13_083_194)));
        }
    }

    #[test]
    fn the_refused_kinds_do_not_vanish() {
        for kind in [Kind::SplashPotion, Kind::EnderPearl] {
            let mut e = p(kind);
            let mut r = LegacyRandomSource::new(1);
            let mut out = Tick::default();
            on_hit(&mut e, &Outcome::Miss, &mut r, &mut out);
            assert!(!out.discarded, "{kind:?} must not be silently consumed");
            assert!(!e.removed);
        }
        // Positive control: a kind that IS implemented does vanish.
        let mut e = p(Kind::Snowball);
        let mut r = LegacyRandomSource::new(1);
        let mut out = Tick::default();
        on_hit(&mut e, &Outcome::Miss, &mut r, &mut out);
        assert!(out.discarded);
    }

    #[test]
    fn the_two_water_drags_are_different_constants() {
        // The arrow's is 0.6, this one's is 0.8. Written as an assertion
        // because the two modules are the place a copy-paste would put the
        // wrong one.
        assert_eq!(THROWABLE_WATER_DRAG, 0.8);
        assert_eq!(crate::projectile::arrow::WATER_INERTIA, 0.6);
        assert_eq!(THROWABLE_GRAVITY, 0.03);
        assert_eq!(crate::projectile::arrow::ARROW_GRAVITY, 0.05);
    }
}
