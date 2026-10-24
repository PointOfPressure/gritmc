//! `Entity.move` — everything above [`crate::collide`] and below a per-kind
//! `tick`.
//!
//! Reference (26.2), read from the pinned jar with `javap -p -c -constants`:
//! `Entity.move`, `Entity.applyGravity`, `Entity.getGravity`,
//! `Entity.isNoGravity`, `Entity.setOnGroundWithMovement`,
//! `Entity.checkSupportingBlock`, `Entity.getOnPos`,
//! `Entity.getBlockPosBelowThatAffectsMyMovement`,
//! `Entity.getBlockSpeedFactor`, `Entity.restituteMovementAfterCollisions`,
//! `Entity.getBlockBounciness`, `Entity.getEntityBounciness`,
//! `Entity.getAirDrag`, `EntityDimensions.makeBoundingBox`,
//! `CollisionGetter.findSupportingBlock`, `Mth.equal`.
//!
//! # What this covers, said before anything else
//!
//! **`Entity.move` as reached by a server-authoritative entity**, which in
//! this server is an item and a falling block. The limbs a *client*-
//! authoritative entity takes are different, they are named in [`Profile`],
//! and they are refused rather than approximated — see
//! [`Profile::client_authoritative`].
//!
//! Four limbs of `Entity.move` are **not** ported and each is unreachable
//! here rather than skipped:
//!
//! * `MoverType.PISTON`'s `limitPistonMovement` — nothing in this server
//!   moves a piston, and [`MoverType::Piston`] is refused at the top of
//!   [`move_entity`] rather than falling through to the normal path;
//! * `stuckSpeedMultiplier` — set only by cobweb, powder snow, sweet berries
//!   and honey, none of which any ported mechanic places or ticks;
//! * `maybeBackOffFromEdge` — `Entity`'s is the identity and only `Player`
//!   overrides it (the sneak-at-an-edge rule), so for the two kinds here it
//!   is `return vec`;
//! * the `fallDistance != 0 && lengthSqr >= 1` clip that resets fall damage,
//!   `checkFallDamage`, and `applyMovementEmissionAndPlaySound` — fall damage
//!   and sound are not modelled at all, and both are stated in
//!   `docs/SERVER.md` rather than silently absent.
//!
//! The rest is ported whole, including the two things a plausible
//! reimplementation gets wrong:
//!
//! * **the collision flags are `Mth.equal`, not `!=`.** `horizontalCollision`
//!   is `!Mth.equal(vec.x, moved.x) || !Mth.equal(vec.z, moved.z)`, and
//!   `Mth.equal(double,double)` is `Math.abs(b - a) < 9.999999747378752E-6` —
//!   the *float* `1.0E-5f` widened. `verticalCollision` on the same line is a
//!   plain `!=`. Using one test for all three axes is wrong in both
//!   directions and only shows on a clamp smaller than a hundred-thousandth
//!   of a block;
//! * **`setOnGroundWithMovement` is gated on `Math.abs(vec.y) > 0.0`** (or on
//!   local authority). A purely horizontal move does not update the ground
//!   flag at all, which is why a walking player's flag comes from its packet
//!   and not from the geometry.

use crate::attributes::Attributes;
use crate::collide::{self, Aabb, CollisionBlocks, CollisionShapes, NoShapeFor, Vec3};
use crate::movement_props::MovementProps;
use rustmc_data::BlockPos;

/// `Mth.equal(double, double)` — `Math.abs(b - a) < 9.999999747378752E-6`.
///
/// **The epsilon is `1.0E-5f` widened, not `1.0E-5`.** The constant pool holds
/// `9.999999747378752E-6`; writing `1e-5_f64` is a different number and the
/// difference is exactly the width of the band this predicate defines.
/// [`crate::collide`] already carries the negated form of the same constant
/// for the step-up probe box; this is the positive one and they are derived
/// the same way rather than from each other.
#[must_use]
pub fn mth_equal(a: f64, b: f64) -> bool {
    (b - a).abs() < f64::from(1.0E-5_f32)
}

/// `net.minecraft.world.entity.MoverType`.
///
/// Present in full rather than trimmed to the two values this server produces,
/// because `Entity.move`'s first two branches test it by identity and a
/// two-variant enum would make the piston branch unrepresentable rather than
/// unreached — which is the difference between a gap you can see and one you
/// cannot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoverType {
    SelfMove,
    Player,
    Piston,
    ShulkerBox,
    Shulker,
}

/// The per-kind constants `Entity.move` and the tick above it consult.
///
/// Built by [`Profile::of`] from the entity kind, so that a kind nobody has
/// written a profile for is a `None` at the call site rather than a set of
/// plausible defaults.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Profile {
    /// `EntityType.width` — `EntityDimensions.makeBoundingBox` halves it.
    pub width: f64,
    /// `EntityType.height`.
    pub height: f64,
    /// `Entity.maxUpStep()`. Zero for anything that is not a `LivingEntity`;
    /// a `LivingEntity`'s is `(float) getAttributeValue(STEP_HEIGHT)` and
    /// therefore comes through [`crate::attributes`].
    pub max_up_step: f32,
    /// `Entity.getGravity()` — `isNoGravity() ? 0 : getDefaultGravity()`.
    ///
    /// A literal for the two kinds here (`ldc2_w 0.04d` in both
    /// `ItemEntity.getDefaultGravity` and
    /// `FallingBlockEntity.getDefaultGravity`) and an attribute read for a
    /// `LivingEntity`. Which it was is not recoverable from this field, which
    /// is why [`Profile::of`] is the only thing allowed to fill it.
    pub gravity: f64,
    /// `Entity.getAirDrag()` — `0.98f` on `Entity`, and neither `ItemEntity`
    /// nor `FallingBlockEntity` overrides it.
    pub air_drag: f32,
    /// `Entity.getEntityBounciness()` — `dconst_0` on `Entity`.
    /// `LivingEntity` overrides it with `getAttributeValue(BOUNCINESS)`.
    pub entity_bounciness: f64,
    /// `Entity.isClientAuthoritative()`.
    ///
    /// On the server `isLocalInstanceAuthoritative()` is
    /// `!isClientAuthoritative()`, and `Entity.move` gates two things on it:
    /// the ground flag on a purely horizontal move (offset 416) and
    /// `checkFallDamage` (526..546). `Player.isClientAuthoritative` returns
    /// `iconst_1`.
    pub client_authoritative: bool,
    /// `Entity.canSimulateMovement()` on the server.
    ///
    /// **Not the negation of [`Self::client_authoritative`] for a player,
    /// and this field exists because it was read as one until 2026-09-30.**
    /// `Entity.canSimulateMovement` is `isLocalInstanceAuthoritative()`, but
    /// `Player` overrides it with `!level.isClientSide() || isLocalPlayer()`
    /// (offsets 0..22) — TRUE on the server. So a `ServerPlayer` both is
    /// client-authoritative AND simulates: `Entity.move` restitutes its
    /// velocity after collisions (offset 564) and `LivingEntity.aiStep` runs
    /// `travel` for it every tick (601..617). The first `/damage` recording
    /// shows it: a standing vanilla player's `set_entity_motion` is
    /// `(0, -0.0784, 0)`, which is one tick of `travelInAir`'s gravity and
    /// drag, where a server with no player physics sends zero.
    pub can_simulate_movement: bool,
    /// `entity instanceof LivingEntity`, which `Entity.getBlockBounciness`
    /// tests (`* 1` against `* 0.8`) and which selects
    /// `LivingEntity.getBlockSpeedFactor`'s `MOVEMENT_EFFICIENCY` lerp over
    /// `Entity`'s plain factor.
    pub living: bool,
}

impl Profile {
    /// `EntityTypes.ITEM`: `.sized(0.25, 0.25)`, no `AttributeSupplier`,
    /// `getDefaultGravity` `ldc2_w 0.04d`, `getAirDrag` inherited `0.98f`.
    pub const ITEM: Self = Self {
        width: 0.25,
        height: 0.25,
        max_up_step: 0.0,
        gravity: 0.04,
        air_drag: 0.98,
        entity_bounciness: 0.0,
        client_authoritative: false,
        can_simulate_movement: true,
        living: false,
    };

    /// `EntityTypes.FALLING_BLOCK`: `.sized(0.98, 0.98)`, no
    /// `AttributeSupplier`, `getDefaultGravity` `ldc2_w 0.04d`.
    pub const FALLING_BLOCK: Self = Self {
        width: 0.98,
        height: 0.98,
        max_up_step: 0.0,
        gravity: 0.04,
        air_drag: 0.98,
        entity_bounciness: 0.0,
        client_authoritative: false,
        can_simulate_movement: true,
        living: false,
    };

    /// `EntityTypes.PLAYER`: `.sized(0.6, 1.8)`.
    ///
    /// **Every field here that comes from an attribute is `None` without the
    /// table**, and that is why this is a constructor rather than a constant:
    /// `max_up_step` is `STEP_HEIGHT`, `gravity` is `GRAVITY` and
    /// `entity_bounciness` is `BOUNCINESS`, all three read through
    /// [`crate::attributes`]. Returning `None` when the extract is missing is
    /// a refusal to move the entity, which is the honest failure — a default
    /// of 0.6 would be a transcribed number with nothing checking it.
    ///
    /// `handleMovePlayer`'s `player.move(MoverType.PLAYER, vec)` and the
    /// server's own `travel` both move a player with this profile; see
    /// [`Self::can_simulate_movement`] for why a client-authoritative entity
    /// is simulated at all.
    #[must_use]
    pub fn player(attributes: &Attributes) -> Option<Self> {
        use crate::attributes::{BOUNCINESS, GRAVITY, STEP_HEIGHT};
        const PLAYER: &str = "minecraft:player";
        #[allow(clippy::cast_possible_truncation)]
        let step = attributes.value(PLAYER, STEP_HEIGHT, &[])? as f32;
        Some(Self {
            width: 0.6,
            height: 1.8,
            max_up_step: step,
            gravity: attributes.value(PLAYER, GRAVITY, &[])?,
            // `LivingEntity.getAirDrag()` is
            // `computeModifiedFriction(0.98f, AIR_DRAG_MODIFIER)`; see
            // [`compute_modified_friction`].
            air_drag: compute_modified_friction(0.98, air_drag_modifier(attributes)?),
            entity_bounciness: attributes.value(PLAYER, BOUNCINESS, &[])?,
            client_authoritative: true,
            can_simulate_movement: true,
            living: true,
        })
    }

    /// The profile for an entity kind, or `None` for a kind whose movement
    /// this server does not simulate.
    ///
    /// **This is the coverage statement and it is deliberately a `match` with
    /// no wildcard arm that guesses.** Every kind not named here gets `None`,
    /// and [`crate::movement`]'s tick treats `None` as "do not move this
    /// entity" rather than as "move it like an item". A 1,296-row table in
    /// which every row said `dir=DOWN -> air` is the defect this shape exists
    /// to prevent.
    #[must_use]
    pub fn of(kind: &str, attributes: Option<&Attributes>) -> Option<Self> {
        match kind {
            "minecraft:item" => Some(Self::ITEM),
            "minecraft:falling_block" => Some(Self::FALLING_BLOCK),
            "minecraft:player" => attributes.and_then(Self::player),
            _ => None,
        }
    }

    /// `EntityDimensions.makeBoundingBox(x, y, z)`:
    /// `new AABB(x - w, y, z - w, x + w, y + h, z + w)` with `w = width / 2`.
    ///
    /// **`width` is halved and `height` is not**, and the box starts at the
    /// entity's `y` rather than being centred on it — an entity's position is
    /// its feet.
    #[must_use]
    pub fn bounding_box(&self, pos: Vec3) -> Aabb {
        let w = self.width / 2.0;
        Aabb::new(
            pos.x - w,
            pos.y,
            pos.z - w,
            pos.x + w,
            pos.y + self.height,
            pos.z + w,
        )
    }
}

/// The mutable half of an entity, as `Entity.move` reads and writes it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Body {
    /// `Entity.position()` — feet, centred in x and z.
    pub pos: Vec3,
    /// `Entity.getDeltaMovement()`.
    pub movement: Vec3,
    /// `Entity.onGround()`.
    pub on_ground: bool,
    pub horizontal_collision: bool,
    pub vertical_collision: bool,
    pub vertical_collision_below: bool,
    /// `Entity.mainSupportingBlockPos`, which `getOnPos` reads and
    /// `checkSupportingBlock` writes. `None` is vanilla's `Optional.empty()`.
    pub supporting_block: Option<BlockPos>,
    /// `Entity.onGroundNoBlocks`, which `checkSupportingBlock` both reads and
    /// writes. It is not a cache of `!on_ground`: it records that the entity
    /// was on the ground with *no* supporting block found, and it is what
    /// makes the second, shifted probe run only once.
    pub on_ground_no_blocks: bool,
    /// `Entity.isSuppressingBounce()` — `Player`'s is `isShiftKeyDown()`,
    /// everything else here answers `false`. Held on the body so that
    /// [`move_entity`] can read it without a parameter every caller but the
    /// player would pass as `false`.
    pub suppressing_bounce: bool,
}

impl Body {
    /// A body at rest at a position.
    #[must_use]
    pub const fn at(pos: Vec3) -> Self {
        Self {
            pos,
            movement: Vec3::ZERO,
            on_ground: false,
            horizontal_collision: false,
            vertical_collision: false,
            vertical_collision_below: false,
            supporting_block: None,
            on_ground_no_blocks: false,
            suppressing_bounce: false,
        }
    }
}

/// `LivingEntity.computeModifiedFriction(float friction, float modifier)`:
/// `Mth.clamp(1.0f - (1.0f - friction) * modifier, 0.0f, 1.0f)`, in float.
///
/// With a modifier of `1.0f` it gives the friction back exactly for the three
/// constants that reach it here (`0.98f`, `0.91f` and a block's `0.6f`):
/// `1.0f - 0.98f` is `0.01999998f` and `1.0f` minus that rounds to `0.98f`.
/// Computed rather than assumed, because a modifier other than 1 does move
/// it and the attribute is a table read.
#[must_use]
pub fn compute_modified_friction(friction: f32, modifier: f32) -> f32 {
    (1.0_f32 - (1.0_f32 - friction) * modifier).clamp(0.0, 1.0)
}

/// `minecraft:air_drag_modifier`'s value for a player, narrowed `d2f` as
/// `travelInAir` offset 194 narrows it.
#[must_use]
pub fn air_drag_modifier(attributes: &Attributes) -> Option<f32> {
    #[allow(clippy::cast_possible_truncation)]
    attributes
        .value("minecraft:player", "minecraft:air_drag_modifier", &[])
        .map(|v| v as f32)
}

/// `minecraft:friction_modifier`'s value for a player, narrowed `d2f` as
/// `travelInAir` offset 33 narrows it.
#[must_use]
pub fn friction_modifier(attributes: &Attributes) -> Option<f32> {
    #[allow(clippy::cast_possible_truncation)]
    attributes
        .value("minecraft:player", "minecraft:friction_modifier", &[])
        .map(|v| v as f32)
}

/// `Entity.applyGravity()`.
///
/// ```text
///   double g = getGravity();
///   if (g != 0) setDeltaMovement(getDeltaMovement().add(0, -g, 0));
/// ```
///
/// The `!= 0` guard is not an optimisation and is kept: `add(0, -0.0, 0)`
/// would turn a `-0.0` y into `-0.0` and a `+0.0` into `+0.0`, which is the
/// same value, but the guard is also what `isNoGravity()` reaches — and
/// `getGravity()` is `isNoGravity() ? 0 : getDefaultGravity()`, so the
/// no-gravity flag is expressed here as a zero rather than as a second branch.
pub fn apply_gravity(body: &mut Body, gravity: f64) {
    if gravity != 0.0 {
        body.movement = Vec3::new(body.movement.x, body.movement.y - gravity, body.movement.z);
    }
}

/// `CollisionGetter.findSupportingBlock(entity, aabb)`.
///
/// The block under the entity that actually holds it up: of every position
/// whose collision shape intersects the probe box, the one nearest the
/// entity's position, ties broken by `BlockPos.compareTo`.
///
/// ```text
///   BlockPos best = null; double bestDist = Double.MAX_VALUE;
///   for (BlockPos p : new BlockCollisions<>(this, entity, aabb, false, (pos, shape) -> pos)) {
///       double d = p.distToCenterSqr(entity.position());
///       if (d < bestDist || (d == bestDist && (best == null || best.compareTo(p) < 0)))
///           { best = p.immutable(); bestDist = d; }
///   }
/// ```
///
/// **The tie-break keeps the *larger* `BlockPos`**, because the update runs
/// when `best.compareTo(p) < 0`. `BlockPos.compareTo` is `Vec3i`'s: y first,
/// then z, then x. Getting that backwards picks a different neighbour under an
/// entity standing exactly on a seam, which changes which block's friction it
/// slides on.
///
/// # Errors
/// [`NoShapeFor`], from collider collection.
pub fn find_supporting_block<W: CollisionBlocks>(
    shapes: &CollisionShapes,
    world: &W,
    pos: Vec3,
    probe: Aabb,
) -> Result<Option<BlockPos>, NoShapeFor> {
    let mut best: Option<BlockPos> = None;
    let mut best_dist = f64::MAX;
    for placed in collide::collect_block_collisions(shapes, world, probe)? {
        let p = placed.pos;
        // `BlockPos.distToCenterSqr(Position)` — the distance to the block's
        // **centre**, so each coordinate is `p + 0.5`. Using the corner picks
        // the wrong neighbour for an entity standing near a seam.
        let dx = f64::from(p.x) + 0.5 - pos.x;
        let dy = f64::from(p.y) + 0.5 - pos.y;
        let dz = f64::from(p.z) + 0.5 - pos.z;
        let d = dx * dx + dy * dy + dz * dz;
        let take = if d < best_dist {
            true
        } else if d == best_dist {
            best.is_none_or(|b| block_pos_cmp(b, p) == std::cmp::Ordering::Less)
        } else {
            false
        };
        if take {
            best = Some(p);
            best_dist = d;
        }
    }
    Ok(best)
}

/// `Vec3i.compareTo` — **y, then z, then x**, which is not the order anybody
/// writes by hand.
fn block_pos_cmp(a: BlockPos, b: BlockPos) -> std::cmp::Ordering {
    a.y.cmp(&b.y).then(a.z.cmp(&b.z)).then(a.x.cmp(&b.x))
}

/// `Entity.checkSupportingBlock(boolean onGround, Vec3 movement)`.
///
/// ```text
///   if (onGround) {
///       AABB bb = getBoundingBox();
///       AABB probe = new AABB(bb.minX, bb.minY - 1.0E-6, bb.minZ, bb.maxX, bb.minY, bb.maxZ);
///       Optional<BlockPos> found = level.findSupportingBlock(this, probe);
///       if (found.isPresent() || this.onGroundNoBlocks) {
///           this.mainSupportingBlockPos = found;
///       } else if (movement != null) {
///           AABB back = probe.move(-movement.x, 0, -movement.z);
///           this.mainSupportingBlockPos = level.findSupportingBlock(this, back);
///       }
///       this.onGroundNoBlocks = found.isEmpty();
///   } else {
///       this.onGroundNoBlocks = false;
///       if (this.mainSupportingBlockPos.isPresent()) this.mainSupportingBlockPos = Optional.empty();
///   }
/// ```
///
/// The second, *backwards-shifted* probe is the part worth naming: an entity
/// that walked off the edge of a block this tick has nothing under it now, so
/// vanilla looks under where it came from. It runs only when the first probe
/// found nothing **and** the entity was not already in that state, which is
/// the whole job of `onGroundNoBlocks`.
///
/// # Errors
/// [`NoShapeFor`].
pub fn check_supporting_block<W: CollisionBlocks>(
    shapes: &CollisionShapes,
    world: &W,
    profile: &Profile,
    body: &mut Body,
    on_ground: bool,
    movement: Option<Vec3>,
) -> Result<(), NoShapeFor> {
    if !on_ground {
        body.on_ground_no_blocks = false;
        body.supporting_block = None;
        return Ok(());
    }
    let bb = profile.bounding_box(body.pos);
    let probe = Aabb::new(
        bb.min_x,
        bb.min_y - 1.0E-6,
        bb.min_z,
        bb.max_x,
        bb.min_y,
        bb.max_z,
    );
    let found = find_supporting_block(shapes, world, body.pos, probe)?;
    if found.is_some() || body.on_ground_no_blocks {
        body.supporting_block = found;
    } else if let Some(m) = movement {
        let back = probe.shifted(-m.x, 0.0, -m.z);
        body.supporting_block = find_supporting_block(shapes, world, body.pos, back)?;
    }
    body.on_ground_no_blocks = found.is_none();
    Ok(())
}

/// `Entity.getOnPos(float offset)`.
///
/// ```text
///   if (mainSupportingBlockPos.isPresent()) {
///       BlockPos p = mainSupportingBlockPos.get();
///       if (offset <= 1.0E-5f) return p;
///       BlockState st = level.getBlockState(p);
///    38: if (offset > 0.5) skip the fence test
///    47: if (st.is(BlockTags.FENCES))       return p;
///    57: if (st.is(BlockTags.WALLS))        return p;
///    67: if (block instanceof FenceGateBlock) return p;
///    79: return p.atY(Mth.floor(position.y - offset));
///   }
///   return new BlockPos(floor(x), floor(y - offset), floor(z));
/// ```
///
/// **THE FENCE TEST IS GUARDED BY `offset <= 0.5` AND THE OTHER TWO ARE NOT**,
/// and that is not a detail: `Entity.move` calls this at **0.2**
/// (`getOnPosLegacy`, for the block state the restitution reads) and at
/// **0.500001** (`getBlockPosBelowThatAffectsMyMovement`, for the friction),
/// one either side of the comparison. A fence therefore returns *itself* at
/// 0.2 and the *column below* at 0.500001.
///
/// This function collapsed all three arms into one boolean until a review pass
/// against the bytecode caught it, which would have made a fence behave the
/// same at both offsets — wrong at both, and invisible to anything that never
/// stands an entity on a fence.
///
/// The exception exists at all because a fence, a wall and a gate are taller
/// than their own block, so an entity on one is a whole block above the
/// position `floor(y - offset)` would name.
#[must_use]
pub fn on_pos(
    body: &Body,
    offset: f32,
    is_fence: impl Fn(BlockPos) -> bool,
    is_wall_or_gate: impl Fn(BlockPos) -> bool,
) -> BlockPos {
    #[allow(clippy::cast_possible_truncation)]
    let below_y = (body.pos.y - f64::from(offset)).floor() as i32;
    match body.supporting_block {
        Some(p) => {
            if offset <= 1.0E-5_f32 {
                return p;
            }
            if (offset <= 0.5 && is_fence(p)) || is_wall_or_gate(p) {
                return p;
            }
            BlockPos {
                x: p.x,
                y: below_y,
                z: p.z,
            }
        }
        None => {
            #[allow(clippy::cast_possible_truncation)]
            let (x, z) = (body.pos.x.floor() as i32, body.pos.z.floor() as i32);
            BlockPos { x, y: below_y, z }
        }
    }
}

/// `Entity.getOnPosLegacy()` — `getOnPos(0.2f)`.
///
/// The position whose block state `Entity.move` passes to
/// `restituteMovementAfterCollisions` and `checkFallDamage`. **A different
/// offset from [`block_pos_below_that_affects_my_movement`]'s `0.500001f`**,
/// and the two are used one line apart in `move`, so a port that computed one
/// of them and reused it would read the wrong block for one of the two.
/// 0.2 is on the *near* side of `getOnPos`'s `<= 0.5` comparison, so a fence
/// takes the fence arm here and not there.
#[must_use]
pub fn on_pos_legacy(
    body: &Body,
    is_fence: impl Fn(BlockPos) -> bool,
    is_wall_or_gate: impl Fn(BlockPos) -> bool,
) -> BlockPos {
    on_pos(body, 0.2_f32, is_fence, is_wall_or_gate)
}

/// `Entity.getBlockPosBelowThatAffectsMyMovement()` — `getOnPos(0.500001f)`.
///
/// **`0.500001f`, not `0.5f`.** The offset is compared against `0.5` inside
/// `getOnPos` to decide whether a fence counts, and this constant is on the
/// far side of that comparison on purpose.
#[must_use]
pub fn block_pos_below_that_affects_my_movement(
    body: &Body,
    is_fence: impl Fn(BlockPos) -> bool,
    is_wall_or_gate: impl Fn(BlockPos) -> bool,
) -> BlockPos {
    on_pos(body, 0.500_001_f32, is_fence, is_wall_or_gate)
}

/// `Entity.getBlockSpeedFactor()`.
///
/// ```text
///   BlockState in = level.getBlockState(blockPosition());
///   float f = in.getBlock().getSpeedFactor();
///   if (in.is(WATER) || in.is(BUBBLE_COLUMN)) return f;
///   return f == 1.0 ? level.getBlockState(getBlockPosBelowThatAffectsMyMovement())
///                          .getBlock().getSpeedFactor()
///                   : f;
/// ```
///
/// The water/bubble-column short-circuit comes **before** the fallback, so an
/// entity standing in water over soul sand is not slowed by the soul sand.
#[must_use]
pub fn block_speed_factor<W: CollisionBlocks>(
    props: &MovementProps,
    world: &W,
    body: &Body,
) -> f32 {
    #[allow(clippy::cast_possible_truncation)]
    let here = BlockPos {
        x: body.pos.x.floor() as i32,
        y: body.pos.y.floor() as i32,
        z: body.pos.z.floor() as i32,
    };
    let id = world.state_id_at(here);
    let f = props.speed_factor(id);
    if props.is_water_or_bubble_column(id) {
        return f;
    }
    if f == 1.0 {
        let below = block_pos_below_that_affects_my_movement(
            body,
            |p| props.is_fence(world.state_id_at(p)),
            |p| props.is_wall_or_gate(world.state_id_at(p)),
        );
        props.speed_factor(world.state_id_at(below))
    } else {
        f
    }
}

/// `Entity.restituteMovementAfterCollisions(BlockState, boolean xCollided,
/// boolean zCollided, Vec3 moved)`, for an entity whose bounciness is zero.
///
/// With `getEntityBounciness()` zero and the supporting block's
/// `getBounceRestitution()` zero — which is every block in the gameplay corpus
/// and every entity kind this server simulates — the method reduces to
/// **zeroing each collided axis**:
///
/// ```text
///   b = isSuppressingBounce() ? 0 : getEntityBounciness()
///   out = delta
///   if (xCollided) out = out.with(X, -delta.x * b)
///   if (zCollided) out = out.with(Z, -delta.z * b)
///   if (verticalCollision) {
///       if (verticalCollisionBelow) b = (-delta.y < getEffectiveGravity()
///                                        || isSuppressingBounce()
///                                        || state.is(SUPPRESSES_BOUNCE))
///                                       ? 0 : max(b, getBlockBounciness(block));
///       if (b > 0) { r = moved.y / delta.y; a = r * gravity; c = lerp(r, 1, airDrag); }
///       else       { a = 0; c = 1; }
///       out = out.with(Y, (a - delta.y) * c * b)
///   }
///   setDeltaMovement(out)
/// ```
///
/// **The reduction is computed here rather than written down as the rule.**
/// The bounciness is a parameter, the block's restitution is read from
/// `movement_props.json`, and the `b > 0` arm is implemented — so a slime
/// block placed by anything later bounces rather than absorbing. Writing
/// "zero the collided axis" as the rule would be an approximation that reads
/// as complete, which is this project's most expensive defect.
#[allow(clippy::too_many_arguments)]
pub fn restitute_movement_after_collisions(
    body: &mut Body,
    profile: &Profile,
    block_restitution: f64,
    suppresses_bounce: bool,
    effective_gravity: f64,
    x_collided: bool,
    z_collided: bool,
    moved: Vec3,
) {
    // `isSuppressingBounce()` is `isShiftKeyDown()`; nothing here sneaks, so
    // it is a parameter rather than a constant and the caller says so.
    let mut b = if suppresses_bounce {
        0.0
    } else {
        profile.entity_bounciness
    };
    let delta = body.movement;
    let mut out = delta;
    if x_collided {
        out = out.with(collide::Axis::X, -delta.x * b);
    }
    if z_collided {
        out = out.with(collide::Axis::Z, -delta.z * b);
    }
    if body.vertical_collision {
        if body.vertical_collision_below {
            b = if -delta.y < effective_gravity || suppresses_bounce || suppresses_bounce_tag() {
                0.0
            } else {
                b.max(block_restitution)
            };
        }
        let (a, c) = if b > 0.0 {
            let r = moved.y / delta.y;
            // `Mth.lerp(delta, start, end)` is `start + delta * (end - start)`.
            let drag = f64::from(profile.air_drag);
            (r * effective_gravity, 1.0 + r * (drag - 1.0))
        } else {
            (0.0, 1.0)
        };
        out = out.with(collide::Axis::Y, (a - delta.y) * c * b);
    }
    body.movement = out;
}

/// `BlockTags.SUPPRESSES_BOUNCE` membership for the supporting block.
///
/// **Always `false`, and that is a measurement rather than a stub**: the tag
/// holds only `minecraft:sculk` and `minecraft:sculk_shrieker` in 26.2, and it
/// is consulted only on the arm where `b` is already about to be raised from a
/// *bouncy* block's restitution. A sculk block has restitution 0, so the two
/// conditions cannot both matter at once and the answer this returns cannot
/// change any result reachable here. Written as a named function so that the
/// day a tag table exists there is one place to wire it.
const fn suppresses_bounce_tag() -> bool {
    false
}

/// `Entity.move(MoverType, Vec3)`, for a server-authoritative entity.
///
/// Returns the vector the entity actually moved — vanilla's local 4 — so a
/// caller can compare it against what it asked for without recomputing.
///
/// # Errors
/// [`NoShapeFor`] when a swept position has a dynamic collision shape the
/// state-keyed table has no answer for. **Raised rather than guessed**: the
/// alternative is dropping an entity through a shulker box or standing it on
/// powder snow, and both look like working physics.
///
/// # Panics
/// Never. The `MoverType::Piston` arm returns without moving.
pub fn move_entity<W: CollisionBlocks>(
    shapes: &CollisionShapes,
    props: &MovementProps,
    world: &W,
    profile: &Profile,
    body: &mut Body,
    mover: MoverType,
    vec: Vec3,
) -> Result<Vec3, NoShapeFor> {
    // `limitPistonMovement` is not ported and nothing in this server pushes an
    // entity with a piston. Refusing here rather than falling through means a
    // future piston gets a visibly unmoved entity instead of one moved by the
    // wrong rule.
    if mover == MoverType::Piston {
        return Ok(Vec3::ZERO);
    }

    let box_ = profile.bounding_box(body.pos);
    // `maybeBackOffFromEdge` is `Entity`'s identity for both kinds here; only
    // `Player` overrides it. `stuckSpeedMultiplier` is always zero.
    let moved = collide::collide(
        shapes,
        world,
        vec,
        box_,
        profile.max_up_step,
        body.on_ground,
    )?;

    // `if (moved.lengthSqr() > 1.0E-7 || vec.lengthSqr() - moved.lengthSqr() < 1.0E-7) setPos(...)`
    // — vanilla's offsets 154-181, and the second disjunct is the one that
    // matters: a move fully absorbed by a collision still updates the position
    // (by nothing), while a move whose *loss* is large is treated as a
    // teleport-sized correction and skipped.
    let moved_len = moved.length_sqr();
    if moved_len > 1.0E-7 || vec.length_sqr() - moved_len < 1.0E-7 {
        body.pos = Vec3::new(
            body.pos.x + moved.x,
            body.pos.y + moved.y,
            body.pos.z + moved.z,
        );
    }

    let x_collided = !mth_equal(vec.x, moved.x);
    let z_collided = !mth_equal(vec.z, moved.z);
    body.horizontal_collision = x_collided || z_collided;

    // The gate the module docs name: a purely horizontal move leaves the
    // ground flag alone unless the entity is locally authoritative, which on
    // the server is everything that is not a player.
    let moved_vertically = vec.y.abs() > 0.0;
    if moved_vertically || !profile.client_authoritative {
        body.vertical_collision = vec.y != moved.y;
        body.vertical_collision_below = body.vertical_collision && vec.y < 0.0;
        // `setOnGroundWithMovement(verticalCollisionBelow, horizontalCollision,
        // moved)` — vanilla passes its own `horizontalCollision` field back
        // into the setter, so that assignment is a no-op and is not repeated
        // here. `onGround` and the supporting block are what it really does.
        let on_ground = body.vertical_collision_below;
        body.on_ground = on_ground;
        check_supporting_block(shapes, world, profile, body, on_ground, Some(moved))?;
    }

    // `restituteMovementAfterCollisions`, gated on `canSimulateMovement()`
    // (offset 564) — true for a player on the server; see the field.
    if profile.can_simulate_movement
        && ((moved_vertically && body.vertical_collision) || body.horizontal_collision)
    {
        // **`getOnPosLegacy()`, not the supporting block.** `Entity.move`
        // computes `BlockPos onPos = getOnPosLegacy()` at offset 508 and
        // passes `level.getBlockState(onPos)` to the restitution — a 0.2
        // offset, not the 0.500001 the friction lookup uses.
        let legacy = on_pos_legacy(
            body,
            |p| props.is_fence(world.state_id_at(p)),
            |p| props.is_wall_or_gate(world.state_id_at(p)),
        );
        let support = world.state_id_at(legacy);
        restitute_movement_after_collisions(
            body,
            profile,
            props.bounce_restitution_for_entity(support, profile.living),
            body.suppressing_bounce,
            profile.gravity,
            x_collided,
            z_collided,
            moved,
        );
    }

    // The last statement of `Entity.move`, and it runs on every path including
    // the ones that returned early above it in vanilla's source order:
    // `setDeltaMovement(getDeltaMovement().multiply(f, 1.0, f))`.
    let f = f64::from(block_speed_factor(props, world, body));
    body.movement = Vec3::new(body.movement.x * f, body.movement.y, body.movement.z * f);
    Ok(moved)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Mth.equal`'s epsilon is the widened *float*. A value just inside it
    /// compares equal and one just outside does not, and `1e-5_f64` would put
    /// the boundary in a different place.
    #[test]
    fn mth_equal_uses_the_widened_float_epsilon() {
        assert!(mth_equal(0.0, 9.0e-6));
        assert!(!mth_equal(0.0, 1.1e-5));
        // The exact boundary: the constant pool holds 9.999999747378752E-6,
        // and the test is `<`, so the epsilon itself is not equal.
        let eps = f64::from(1.0E-5_f32);
        assert!(!mth_equal(0.0, eps));
        assert!(mth_equal(0.0, eps * 0.999));
        assert_ne!(eps, 1.0e-5_f64);
    }

    #[test]
    fn compute_modified_friction_is_float_and_identity_at_one() {
        assert_eq!(compute_modified_friction(0.98, 1.0), 0.98_f32);
        assert_eq!(compute_modified_friction(0.91, 1.0), 0.91_f32);
        assert_eq!(compute_modified_friction(0.6, 1.0), 0.6_f32);
        // A doubled modifier doubles the loss, and the clamp holds the ends.
        assert!((compute_modified_friction(0.9, 2.0) - 0.8).abs() < 1e-6);
        assert_eq!(compute_modified_friction(0.1, 5.0), 0.0);
    }

    #[test]
    fn gravity_is_a_literal_for_the_two_kinds_that_fall() {
        assert_eq!(Profile::ITEM.gravity, 0.04);
        assert_eq!(Profile::FALLING_BLOCK.gravity, 0.04);
    }

    /// The coverage statement, as a test. A kind with no profile must be
    /// `None` rather than an item-shaped guess.
    #[test]
    fn an_unknown_kind_has_no_profile() {
        assert!(Profile::of("minecraft:zombie", None).is_none());
        assert!(Profile::of("minecraft:arrow", None).is_none());
        assert!(Profile::of("minecraft:item", None).is_some());
        assert!(Profile::of("minecraft:falling_block", None).is_some());
        // A player without the attribute table has no profile either: its
        // step height is an attribute read and there is no default to fall
        // back to.
        assert!(Profile::of("minecraft:player", None).is_none());
    }

    #[test]
    fn a_bounding_box_halves_the_width_and_stands_on_the_position() {
        let bb = Profile::ITEM.bounding_box(Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(bb.min_x, 1.0 - 0.125);
        assert_eq!(bb.max_x, 1.0 + 0.125);
        assert_eq!(bb.min_y, 2.0);
        assert_eq!(bb.max_y, 2.25);
    }

    #[test]
    fn gravity_subtracts_from_the_y_component_only() {
        let mut b = Body::at(Vec3::new(0.0, 0.0, 0.0));
        b.movement = Vec3::new(1.0, 0.0, -1.0);
        apply_gravity(&mut b, 0.04);
        assert_eq!(b.movement, Vec3::new(1.0, -0.04, -1.0));
        apply_gravity(&mut b, 0.04);
        assert_eq!(b.movement.y, -0.08);
    }

    /// `getGravity()` folds `isNoGravity()` into a zero, so a zero gravity
    /// must leave the vector untouched rather than adding `-0.0`.
    #[test]
    fn zero_gravity_does_nothing() {
        let mut b = Body::at(Vec3::ZERO);
        b.movement = Vec3::new(0.0, 0.0, 0.0);
        apply_gravity(&mut b, 0.0);
        assert_eq!(b.movement.y.to_bits(), 0.0_f64.to_bits());
    }

    /// A two-shape table: index 0 empty, index 1 a full block. Enough to put
    /// colliders under an entity and ask which one holds it up.
    ///
    /// Built by hand rather than loaded, because the point is the tie-break in
    /// [`find_supporting_block`] and not the extract — but through
    /// `CollisionShapes::from_json` rather than around it, so the fixture
    /// cannot drift from what the real table means.
    fn shapes_fixture() -> CollisionShapes {
        let bits = |v: f64| v.to_bits() as i64;
        let doc = serde_json::json!({
            "gridEquivalence": {"mismatches": 0, "probes": 16_736_112_i64},
            "shapes": [
                {"size": [0, 0, 0], "coords": [[], [], []], "voxels": "", "full": false},
                {"size": [1, 1, 1],
                 "coords": [[bits(0.0), bits(1.0)], [bits(0.0), bits(1.0)],
                            [bits(0.0), bits(1.0)]],
                 "voxels": "8", "full": true}
            ],
            // state 0 -> empty, states 1..=8 -> the full block
            "rows": [0, 1, 1, 1, 1, 1, 1, 1, 1],
            "movingPistonStates": [8]
        });
        CollisionShapes::from_json(&doc).expect("the fixture parses")
    }

    /// A world that is solid at a listed set of positions and air everywhere
    /// else.
    struct Solid(Vec<BlockPos>);
    impl CollisionBlocks for Solid {
        fn state_id_at(&self, pos: BlockPos) -> u32 {
            if self.0.contains(&pos) {
                1
            } else {
                0
            }
        }
    }

    /// A `MovementProps` with one ordinary block, one slow block and one
    /// water, indexed to match [`shapes_fixture`]'s state ids.
    ///
    /// State 1 is the solid ground everything below stands on; state 2 is
    /// soul-sand-like (speed 0.4) and state 3 is water. Only state 1 is solid
    /// in [`Solid`], so 2 and 3 are read for their properties without also
    /// being collided with — which is what `getBlockSpeedFactor` does.
    fn props_fixture() -> crate::movement_props::MovementProps {
        let row = |name: &str, fr: f32, sp: f32, bo: f32, water: bool| {
            serde_json::json!({
                "block": name,
                "frictionBits": fr.to_bits() as i64,
                "speedFactorBits": sp.to_bits() as i64,
                "bounceRestitutionBits": bo.to_bits() as i64,
                "fence": false, "wallOrGate": false, "waterlike": water,
            })
        };
        let doc = serde_json::json!({
            "table": [
                row("minecraft:air", 0.6, 1.0, 0.0, false),
                row("minecraft:stone", 0.6, 1.0, 0.0, false),
                row("minecraft:soul_sand", 0.6, 0.4, 0.0, false),
                row("minecraft:water", 0.6, 1.0, 0.0, true),
            ],
            // state 0 air, 1 stone, 2 soul sand, 3 water, 4..8 stone again
            "stateRows": [0, 1, 2, 3, 1, 1, 1, 1, 1]
        });
        crate::movement_props::MovementProps::from_json(&doc).expect("the fixture parses")
    }

    /// A world built from an explicit (position -> state id) map, so a test
    /// can put a non-solid block underfoot and still have it read.
    struct Map(std::collections::BTreeMap<(i32, i32, i32), u32>);
    impl CollisionBlocks for Map {
        fn state_id_at(&self, pos: BlockPos) -> u32 {
            *self.0.get(&(pos.x, pos.y, pos.z)).unwrap_or(&0)
        }
    }
    fn map(cells: &[((i32, i32, i32), u32)]) -> Map {
        Map(cells.iter().copied().collect())
    }

    /// **A falling item landing on a block**, which is the whole subsystem in
    /// one call: the sweep is clamped, the ground flag comes on, the support
    /// is found, and `verticalCollisionBelow` distinguishes landing from
    /// hitting a ceiling.
    ///
    /// Added because the falsification sweep found `move_entity` had no test
    /// at all — thirteen injections inside it went green.
    #[test]
    fn an_item_falling_onto_a_block_lands_on_it_and_gains_the_ground_flag() {
        let shapes = shapes_fixture();
        let props = props_fixture();
        let world = map(&[((0, 3, 0), 1)]);
        // Feet at y = 4.2, falling 0.5: the block's top is y = 4.
        let mut body = Body::at(Vec3::new(0.5, 4.2, 0.5));
        body.movement = Vec3::new(0.0, -0.5, 0.0);
        let v = body.movement;
        let moved = move_entity(
            &shapes,
            &props,
            &world,
            &Profile::ITEM,
            &mut body,
            MoverType::SelfMove,
            v,
        )
        .expect("no dynamic shapes");

        assert!(
            (moved.y - -0.2).abs() < 1e-12,
            "clamped to the block top, moved {}",
            moved.y
        );
        assert!((body.pos.y - 4.0).abs() < 1e-12, "resting on y = 4");
        assert!(body.vertical_collision, "the y sweep was clamped");
        assert!(body.vertical_collision_below, "and it was moving downwards");
        assert!(body.on_ground, "so the ground flag comes on");
        assert!(!body.horizontal_collision, "nothing sideways happened");
        assert_eq!(
            body.supporting_block,
            Some(BlockPos { x: 0, y: 3, z: 0 }),
            "checkSupportingBlock found the block underneath"
        );
        // restituteMovementAfterCollisions with zero bounciness zeroes the
        // collided axis, so a landed item has no downward velocity left.
        assert_eq!(body.movement.y, 0.0);
    }

    /// The other side of `verticalCollisionBelow`: a move **upwards** into a
    /// ceiling is a vertical collision and must NOT set the ground flag.
    /// Dropping the `vec.y < 0.0` test passes the landing case above and
    /// fails this one, which is why both exist.
    #[test]
    fn an_item_hitting_a_ceiling_does_not_become_grounded() {
        let shapes = shapes_fixture();
        let props = props_fixture();
        let world = map(&[((0, 6, 0), 1)]);
        let mut body = Body::at(Vec3::new(0.5, 5.0, 0.5));
        body.movement = Vec3::new(0.0, 0.9, 0.0);
        let v = body.movement;
        move_entity(
            &shapes,
            &props,
            &world,
            &Profile::ITEM,
            &mut body,
            MoverType::SelfMove,
            v,
        )
        .expect("no dynamic shapes");
        assert!(body.vertical_collision, "the ceiling clamped it");
        assert!(
            !body.vertical_collision_below,
            "upwards is not `below` — vec.y < 0.0 is false"
        );
        assert!(!body.on_ground, "and so the ground flag stays off");
    }

    /// A horizontal move into a wall sets `horizontalCollision`, and the
    /// clamp is measured with `Mth.equal` rather than `!=`: a move that is
    /// blocked by less than a hundred-thousandth of a block is **not** a
    /// collision. The second half is what separates the two predicates, and a
    /// port using `!=` for both passes the first assertion and fails this one.
    #[test]
    fn a_horizontal_clamp_is_measured_with_mth_equal() {
        let shapes = shapes_fixture();
        let props = props_fixture();
        let world = map(&[((0, 3, 0), 1), ((1, 4, 0), 1)]);

        // Blocked hard: asks for 0.9 east, the wall at x = 1 stops it.
        let mut body = Body::at(Vec3::new(0.5, 4.0, 0.5));
        body.on_ground = true;
        let v = Vec3::new(0.9, 0.0, 0.0);
        move_entity(
            &shapes,
            &props,
            &world,
            &Profile::ITEM,
            &mut body,
            MoverType::SelfMove,
            v,
        )
        .expect("no dynamic shapes");
        assert!(body.horizontal_collision, "0.9 east into a wall");

        // Blocked by less than Mth.equal's epsilon: the item's east face is
        // already at x = 0.875, so a request of 0.125 + 1e-6 is clamped by
        // 1e-6 — under the epsilon, therefore not a collision.
        let mut body = Body::at(Vec3::new(0.75, 4.0, 0.5));
        body.on_ground = true;
        let v = Vec3::new(0.125 + 1.0e-6, 0.0, 0.0);
        move_entity(
            &shapes,
            &props,
            &world,
            &Profile::ITEM,
            &mut body,
            MoverType::SelfMove,
            v,
        )
        .expect("no dynamic shapes");
        assert!(
            !body.horizontal_collision,
            "a clamp under 9.999999747378752E-6 is not a collision"
        );
    }

    /// `Entity.move`'s last statement scales **x and z only** by the block
    /// speed factor, and the factor comes from the block the entity is *in*,
    /// falling back to the one below only when that is 1.0.
    #[test]
    fn the_block_speed_factor_scales_x_and_z_and_falls_back_downwards() {
        let shapes = shapes_fixture();
        let props = props_fixture();
        // Soul sand underfoot, air at the entity's own position: the factor
        // at the entity is 1.0, so the fallback reads the block below.
        let world = map(&[((0, 3, 0), 1), ((0, 4, 0), 0), ((0, 3, 0), 1)]);
        let mut world = world;
        world.0.insert((0, 3, 0), 2); // soul sand, and still solid in shapes
        let mut body = Body::at(Vec3::new(0.5, 4.0, 0.5));
        body.movement = Vec3::new(1.0, 1.0, 1.0);
        body.on_ground = true;
        move_entity(
            &shapes,
            &props,
            &world,
            &Profile::ITEM,
            &mut body,
            MoverType::SelfMove,
            Vec3::ZERO,
        )
        .expect("no dynamic shapes");
        // **`f64::from(0.4_f32)`, not `0.4`.** `Block.getSpeedFactor()` is a
        // Java `float` and `Vec3.multiply` widens it, so the product is
        // 0.4000000059604645. Writing `0.4` here would be a different number
        // and the first draft of this test failed on exactly that — a
        // tolerance loose enough to hide it would also hide a real defect of
        // the same size.
        assert_eq!(
            body.movement.x,
            f64::from(0.4_f32),
            "x scaled by the widened float, not by the decimal 0.4"
        );
        assert_eq!(body.movement.z, f64::from(0.4_f32), "z the same");
        assert_eq!(
            body.movement.y, 1.0,
            "y is multiplied by 1.0, not by the factor"
        );
    }

    /// **A client-authoritative entity's ground flag is not touched by a
    /// purely horizontal move.**
    ///
    /// The gate is `Math.abs(vec.y) > 0.0 || isLocalInstanceAuthoritative()`,
    /// and for a player on the server the second disjunct is false. So a
    /// walking player's flag comes from its movement packet and not from the
    /// geometry — which is the whole mechanism behind
    /// `540-a-player-that-leaves-the-ground-is-resynced`. An item takes the
    /// branch either way, so only a client-authoritative profile can show it.
    #[test]
    fn a_horizontal_move_leaves_a_client_authoritative_ground_flag_alone() {
        let shapes = shapes_fixture();
        let props = props_fixture();
        let world = map(&[((0, 3, 0), 1)]);
        let player_like = Profile {
            client_authoritative: true,
            ..Profile::ITEM
        };
        let mut body = Body::at(Vec3::new(0.5, 4.0, 0.5));
        body.on_ground = true;
        body.supporting_block = Some(BlockPos { x: 0, y: 3, z: 0 });
        move_entity(
            &shapes,
            &props,
            &world,
            &player_like,
            &mut body,
            MoverType::Player,
            Vec3::new(0.1, 0.0, 0.0),
        )
        .expect("no dynamic shapes");
        assert!(body.on_ground, "the claim survives the move");
        assert_eq!(
            body.supporting_block,
            Some(BlockPos { x: 0, y: 3, z: 0 }),
            "and so does the support — checkSupportingBlock did not run"
        );

        // The control: the same move on a server-authoritative profile DOES
        // recompute the flag, and over open air it comes out false.
        let mut body = Body::at(Vec3::new(0.5, 4.0, 0.5));
        body.on_ground = true;
        move_entity(
            &shapes,
            &props,
            &world,
            &Profile::ITEM,
            &mut body,
            MoverType::SelfMove,
            Vec3::new(0.1, 0.0, 0.0),
        )
        .expect("no dynamic shapes");
        assert!(!body.on_ground, "an item's flag is recomputed and cleared");
    }

    /// **`verticalCollision` is a plain `!=`, unlike the two horizontal
    /// flags.** A downward clamp smaller than `Mth.equal`'s epsilon is still
    /// a vertical collision; the same clamp on x or z is not. Using one test
    /// for all three axes is wrong in both directions and this is the half
    /// that shows it.
    #[test]
    fn a_sub_epsilon_vertical_clamp_is_still_a_vertical_collision() {
        let shapes = shapes_fixture();
        let props = props_fixture();
        let world = map(&[((0, 3, 0), 1)]);
        // Feet five microns above the block top at y = 4, asked to fall ten.
        let mut body = Body::at(Vec3::new(0.5, 4.0 + 5.0e-6, 0.5));
        let v = Vec3::new(0.0, -1.0e-5, 0.0);
        move_entity(
            &shapes,
            &props,
            &world,
            &Profile::ITEM,
            &mut body,
            MoverType::SelfMove,
            v,
        )
        .expect("no dynamic shapes");
        assert!(
            body.vertical_collision,
            "clamped by 5e-6, which Mth.equal would call equal"
        );
        assert!(body.vertical_collision_below);
        assert!(body.on_ground);
    }

    /// `getBlockSpeedFactor`'s water short circuit fires **before** the
    /// downward fallback, so an entity in water over soul sand is not slowed
    /// by the soul sand.
    #[test]
    fn water_short_circuits_the_speed_factor_before_the_fallback() {
        let props = props_fixture();
        // state 3 water at the entity, state 2 soul sand below.
        let world = map(&[((0, 4, 0), 3), ((0, 3, 0), 2)]);
        let body = Body::at(Vec3::new(0.5, 4.0, 0.5));
        assert_eq!(
            block_speed_factor(&props, &world, &body),
            1.0,
            "water's own factor, not the soul sand below it"
        );
    }

    /// The fallback runs **only** when the entity's own block has factor 1.0.
    /// A block that slows you does so even with ordinary ground beneath it.
    #[test]
    fn the_speed_factor_falls_back_only_when_its_own_block_is_neutral() {
        let props = props_fixture();
        // Soul sand at the entity, stone below.
        let world = map(&[((0, 4, 0), 2), ((0, 3, 0), 1)]);
        let body = Body::at(Vec3::new(0.5, 4.0, 0.5));
        assert_eq!(
            block_speed_factor(&props, &world, &body),
            0.4_f32,
            "0.4 is not 1.0, so no fallback"
        );

        // Air at the entity, soul sand below: now it does fall back.
        let world = map(&[((0, 4, 0), 0), ((0, 3, 0), 2)]);
        assert_eq!(block_speed_factor(&props, &world, &body), 0.4_f32);
    }

    /// **The second disjunct of the `setPos` guard.**
    ///
    /// `if (moved.lengthSqr() > 1.0E-7 || vec.lengthSqr() - moved.lengthSqr() < 1.0E-7)`.
    /// A move too small to clear the first threshold still updates the
    /// position, because nothing was lost to a collision. Dropping the second
    /// disjunct freezes an entity that is drifting slowly.
    #[test]
    fn a_move_below_the_length_threshold_still_updates_the_position() {
        let shapes = shapes_fixture();
        let props = props_fixture();
        let world = map(&[]);
        let mut body = Body::at(Vec3::new(0.5, 4.0, 0.5));
        let v = Vec3::new(1.0e-5, 0.0, 0.0);
        assert!(
            v.length_sqr() < 1.0E-7,
            "the first disjunct must be false for this test to mean anything"
        );
        move_entity(
            &shapes,
            &props,
            &world,
            &Profile::ITEM,
            &mut body,
            MoverType::SelfMove,
            v,
        )
        .expect("no dynamic shapes");
        assert_eq!(body.pos.x, 0.5 + 1.0e-5, "the position moved anyway");
    }

    /// `MoverType.PISTON` returns without moving, because
    /// `limitPistonMovement` is not ported. A fall-through would move the
    /// entity by the full vector and look like working physics.
    #[test]
    fn the_piston_mover_refuses_rather_than_falling_through() {
        let shapes = shapes_fixture();
        let props = props_fixture();
        let world = map(&[]);
        let mut body = Body::at(Vec3::new(0.5, 4.0, 0.5));
        let before = body.pos;
        let moved = move_entity(
            &shapes,
            &props,
            &world,
            &Profile::ITEM,
            &mut body,
            MoverType::Piston,
            Vec3::new(1.0, 0.0, 0.0),
        )
        .expect("no dynamic shapes");
        assert_eq!(moved, Vec3::ZERO);
        assert_eq!(body.pos, before, "the entity did not move");
    }

    /// **The tie-break the falsification sweep found untested.**
    ///
    /// `findSupportingBlock` updates its answer when
    /// `best.compareTo(p) < 0`, so on an exact distance tie it keeps the
    /// **larger** `BlockPos` under `Vec3i`'s y-then-z-then-x ordering. An
    /// entity standing exactly on the seam between two blocks is equidistant
    /// from both, and which one wins decides whose friction it slides on.
    ///
    /// The probe here is the same box `checkSupportingBlock` builds: the
    /// entity's footprint, one micron deep, just under its feet. An item is
    /// 0.25 wide, so standing at x = 1.0 straddles the seam between x = 0 and
    /// x = 1 and the two candidates are exactly equidistant.
    #[test]
    fn find_supporting_block_keeps_the_larger_block_pos_on_a_tie() {
        let shapes = shapes_fixture();
        let world = Solid(vec![
            BlockPos { x: 0, y: 3, z: 0 },
            BlockPos { x: 1, y: 3, z: 0 },
        ]);
        let pos = Vec3::new(1.0, 4.0, 0.5);
        let bb = Profile::ITEM.bounding_box(pos);
        let probe = Aabb::new(
            bb.min_x,
            bb.min_y - 1.0E-6,
            bb.min_z,
            bb.max_x,
            bb.min_y,
            bb.max_z,
        );
        let got = find_supporting_block(&shapes, &world, pos, probe).expect("no dynamic shapes");
        // Both are at the same y and z, so `compareTo` falls through to x and
        // the larger x wins.
        assert_eq!(got, Some(BlockPos { x: 1, y: 3, z: 0 }));
    }

    /// **Centre, not corner**, and the two disagree on the answer here.
    ///
    /// `BlockPos.distToCenterSqr` measures to `p + 0.5` on each axis.
    /// Measuring to the corner is the same as measuring to a query point
    /// shifted by half a block, so it picks a different neighbour whenever
    /// that shift crosses the bisector — which the tie test above does not
    /// exercise, because there the two candidates are symmetric about the
    /// entity and both metrics agree. The falsification sweep found exactly
    /// that gap.
    ///
    /// A falling block (0.98 wide) at x = 0.6 spans blocks 0 and 1. By centre
    /// the near one is block 0 (0.26 against 1.06); by corner it is block 1
    /// (1.41 against 1.61).
    #[test]
    fn find_supporting_block_measures_to_the_block_centre_not_its_corner() {
        let shapes = shapes_fixture();
        let world = Solid(vec![
            BlockPos { x: 0, y: 3, z: 0 },
            BlockPos { x: 1, y: 3, z: 0 },
        ]);
        let pos = Vec3::new(0.6, 4.0, 0.5);
        let bb = Profile::FALLING_BLOCK.bounding_box(pos);
        let probe = Aabb::new(
            bb.min_x,
            bb.min_y - 1.0E-6,
            bb.min_z,
            bb.max_x,
            bb.min_y,
            bb.max_z,
        );
        let got = find_supporting_block(&shapes, &world, pos, probe).expect("no dynamic shapes");
        assert_eq!(
            got,
            Some(BlockPos { x: 0, y: 3, z: 0 }),
            "measuring to the corner would pick block 1"
        );
    }

    /// **The probe box is one micron deep, not one block.**
    ///
    /// `checkSupportingBlock` builds
    /// `new AABB(minX, minY - 1.0E-6, minZ, maxX, minY, maxZ)`. An entity
    /// hovering half a block above the ground has nothing supporting it, and
    /// a deeper probe would find the floor and call it grounded.
    #[test]
    fn the_supporting_block_probe_is_a_micron_deep() {
        let shapes = shapes_fixture();
        let props = props_fixture();
        let world = Solid(vec![BlockPos { x: 0, y: 3, z: 0 }]);
        // Feet at 4.5; the block's top face is at 4.0, half a block below.
        let mut body = Body::at(Vec3::new(0.5, 4.5, 0.5));
        let _ = &props;
        check_supporting_block(
            &shapes,
            &world,
            &Profile::ITEM,
            &mut body,
            true,
            Some(Vec3::ZERO),
        )
        .expect("no dynamic shapes");
        assert_eq!(
            body.supporting_block, None,
            "nothing within a micron of the feet — a one-block probe would find the floor"
        );
        assert!(body.on_ground_no_blocks, "and that fact is recorded");
    }

    /// **The second probe, shifted backwards along the movement.**
    ///
    /// An entity that walked off the edge of a block this tick has nothing
    /// under it now, so `checkSupportingBlock` looks under where it came
    /// from — `probe.move(-movement.x, 0, -movement.z)`. It runs only when
    /// the first probe found nothing and `onGroundNoBlocks` was not already
    /// set, which is the whole job of that field.
    #[test]
    fn check_supporting_block_looks_under_where_the_entity_came_from() {
        let shapes = shapes_fixture();
        let world = Solid(vec![BlockPos { x: 0, y: 3, z: 0 }]);
        // Standing over x = 1, which is air; arrived by moving +1 in x, so
        // the shifted probe lands over x = 0, which is solid.
        let mut body = Body::at(Vec3::new(1.5, 4.0, 0.5));
        check_supporting_block(
            &shapes,
            &world,
            &Profile::ITEM,
            &mut body,
            true,
            Some(Vec3::new(1.0, 0.0, 0.0)),
        )
        .expect("no dynamic shapes");
        assert_eq!(
            body.supporting_block,
            Some(BlockPos { x: 0, y: 3, z: 0 }),
            "the backwards-shifted probe found the block it walked off"
        );

        // Same position, no movement to shift by: nothing is found. This is
        // the control that stops the assertion above passing for the wrong
        // reason — if the first probe were finding the block, both would.
        let mut body = Body::at(Vec3::new(1.5, 4.0, 0.5));
        check_supporting_block(&shapes, &world, &Profile::ITEM, &mut body, true, None)
            .expect("no dynamic shapes");
        assert_eq!(body.supporting_block, None);
    }

    /// Not a tie: the nearer block wins outright, which is the arm above the
    /// tie-break and the one that would still pass if the comparator were
    /// reversed. Both are asserted so that neither alone can carry the test.
    #[test]
    fn find_supporting_block_takes_the_nearest_when_there_is_no_tie() {
        let shapes = shapes_fixture();
        let world = Solid(vec![
            BlockPos { x: 0, y: 3, z: 0 },
            BlockPos { x: 1, y: 3, z: 0 },
        ]);
        // Centred over x = 0 rather than on the seam.
        let pos = Vec3::new(0.5, 4.0, 0.5);
        let bb = Profile::ITEM.bounding_box(pos);
        let probe = Aabb::new(
            bb.min_x,
            bb.min_y - 1.0E-6,
            bb.min_z,
            bb.max_x,
            bb.min_y,
            bb.max_z,
        );
        let got = find_supporting_block(&shapes, &world, pos, probe).expect("no dynamic shapes");
        assert_eq!(got, Some(BlockPos { x: 0, y: 3, z: 0 }));
    }

    /// Nothing underneath is `None`, not a guessed column. `checkSupportingBlock`
    /// distinguishes the two and only re-probes when the first came back empty.
    #[test]
    fn find_supporting_block_over_air_is_none() {
        let shapes = shapes_fixture();
        let world = Solid(vec![]);
        let pos = Vec3::new(0.5, 4.0, 0.5);
        let bb = Profile::ITEM.bounding_box(pos);
        let probe = Aabb::new(
            bb.min_x,
            bb.min_y - 1.0E-6,
            bb.min_z,
            bb.max_x,
            bb.min_y,
            bb.max_z,
        );
        assert_eq!(
            find_supporting_block(&shapes, &world, pos, probe).expect("no dynamic shapes"),
            None
        );
    }

    /// `Vec3i.compareTo` is y, then z, then x. Written as a test because the
    /// order is not the one anybody assumes and it decides which block an
    /// entity on a seam slides on.
    #[test]
    fn block_pos_ordering_is_y_then_z_then_x() {
        let p = |x, y, z| BlockPos { x, y, z };
        assert_eq!(
            block_pos_cmp(p(9, 0, 0), p(0, 1, 0)),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            block_pos_cmp(p(9, 0, 0), p(0, 0, 1)),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            block_pos_cmp(p(1, 0, 0), p(0, 0, 0)),
            std::cmp::Ordering::Greater
        );
    }

    /// With no supporting block, `getOnPos` is the floor of the position with
    /// the offset taken off y.
    #[test]
    fn on_pos_without_a_support_is_the_floored_column() {
        let b = Body::at(Vec3::new(1.7, 4.0, -0.2));
        let p = on_pos(&b, 0.500_001_f32, |_| false, |_| false);
        assert_eq!(p, BlockPos { x: 1, y: 3, z: -1 });
    }

    /// With one, it is that block's column at `floor(y - offset)` — the same
    /// x and z as the support, not the entity's.
    #[test]
    fn on_pos_with_a_support_keeps_the_supports_column() {
        let mut b = Body::at(Vec3::new(1.7, 4.0, -0.2));
        b.supporting_block = Some(BlockPos { x: 2, y: 3, z: -1 });
        let p = on_pos(&b, 0.500_001_f32, |_| false, |_| false);
        assert_eq!(p, BlockPos { x: 2, y: 3, z: -1 });
    }

    /// **The asymmetry the bytecode guards with `offset <= 0.5`.** A wall is
    /// returned at both offsets; a fence is returned at 0.2 and *not* at
    /// 0.500001. This is the case a single `tall_support` boolean got wrong in
    /// both directions, and it is the reason the two predicates are separate
    /// arguments rather than one.
    #[test]
    fn a_fence_is_tall_only_below_half_a_block_and_a_wall_always_is() {
        let mut b = Body::at(Vec3::new(0.5, 6.4, 0.5));
        b.supporting_block = Some(BlockPos { x: 0, y: 4, z: 0 });
        let fence = |_: BlockPos| true;
        let wall = |_: BlockPos| true;
        let no = |_: BlockPos| false;

        // A fence at the legacy offset (0.2): the support itself.
        assert_eq!(on_pos(&b, 0.2, fence, no), BlockPos { x: 0, y: 4, z: 0 });
        // The same fence at the movement offset (0.500001): the column below,
        // because 0.500001 > 0.5 skips the fence arm entirely.
        assert_eq!(
            on_pos(&b, 0.500_001_f32, fence, no),
            BlockPos { x: 0, y: 5, z: 0 }
        );
        // A wall is unconditional, so it is the support at both.
        assert_eq!(on_pos(&b, 0.2, no, wall), BlockPos { x: 0, y: 4, z: 0 });
        assert_eq!(
            on_pos(&b, 0.500_001_f32, no, wall),
            BlockPos { x: 0, y: 4, z: 0 }
        );
    }

    /// An offset within `1.0E-5f` returns the support before any block test,
    /// which is the arm above the fence guard.
    #[test]
    fn a_tiny_offset_returns_the_support_without_consulting_the_block() {
        let mut b = Body::at(Vec3::new(0.5, 9.9, 0.5));
        b.supporting_block = Some(BlockPos { x: 0, y: 4, z: 0 });
        assert_eq!(
            on_pos(
                &b,
                0.0,
                |_| panic!("must not be consulted"),
                |_| panic!("must not be consulted")
            ),
            BlockPos { x: 0, y: 4, z: 0 }
        );
    }

    /// The two offsets are different constants and `Entity.move` uses both,
    /// one line apart. 0.2 is on the near side of `getOnPos`'s `<= 0.5`
    /// comparison and 0.500001 is on the far side, which is the whole reason
    /// the second one is not simply 0.5.
    #[test]
    fn the_legacy_offset_and_the_movement_offset_are_different_blocks() {
        let mut b = Body::at(Vec3::new(0.5, 6.4, 0.5));
        b.supporting_block = Some(BlockPos { x: 0, y: 4, z: 0 });
        assert_eq!(
            on_pos_legacy(&b, |_| false, |_| false),
            BlockPos { x: 0, y: 6, z: 0 },
            "floor(6.4 - 0.2) = 6"
        );
        assert_eq!(
            block_pos_below_that_affects_my_movement(&b, |_| false, |_| false),
            BlockPos { x: 0, y: 5, z: 0 },
            "floor(6.4 - 0.500001) = 5"
        );

        // **And the millionth, which is the whole reason the constant is not
        // 0.5.** At y = 6.5 the two offsets straddle an integer:
        // floor(6.5 - 0.5) = 6 and floor(6.5 - 0.500001) = 5. The
        // falsification sweep replaced 0.500001 with 0.5 and this test did not
        // notice, because 6.4 gives 5 either way.
        b.pos = Vec3::new(0.5, 6.5, 0.5);
        assert_eq!(
            block_pos_below_that_affects_my_movement(&b, |_| false, |_| false),
            BlockPos { x: 0, y: 5, z: 0 },
            "floor(6.5 - 0.500001) = 5, and floor(6.5 - 0.5) would be 6"
        );
    }
}
