//! `BlockGetter.clip` — the ray a projectile casts down its own delta each
//! tick, and the only reason it ever stops.
//!
//! `AbstractArrow.tick` builds
//! `new ClipContext(pos, pos.add(delta), COLLIDER, NONE, this)` and hands it
//! to `Level.clipIncludingBorder`. Everything below is that call resolved:
//! `traverseBlocks`'s DDA walk, `VoxelShape.clip` per visited block, and
//! `AABB.clip` per box of the shape.
//!
//! # `Fluid.NONE` is why there is one arm and not two
//!
//! `BlockGetter.clip`'s per-block lambda clips the block shape and the fluid
//! shape and takes whichever is nearer. `ClipContext.Fluid.NONE`'s shape
//! function is `Shapes.empty()` for every state, so the fluid arm is always
//! `null` and the block arm always wins. Every projectile in this module
//! passes `NONE`, so the fluid arm is absent here rather than stubbed — and
//! a kind that passes `ANY` or `SOURCE_ONLY` would need it back.
//!
//! # `clipIncludingBorder` is `clip` plus a border test we cannot reach
//!
//! `Level.clipIncludingBorder` runs `clip` and then, only when the result is
//! a MISS **and** the segment crosses the world border, returns a border hit.
//! The default border is 59,999,968 wide; nothing this server places is
//! within thirty million blocks of it. The border arm is refused rather than
//! approximated — see [`ClipMiss`].
//!
//! # The interaction-shape override is a no-op for `COLLIDER`
//!
//! `clipWithInteractionOverride` re-clips against
//! `BlockState.getInteractionShape` and keeps the nearer hit's **direction**
//! only if the interaction hit is strictly nearer. `getInteractionShape`'s
//! `BlockBehaviour` default is `Shapes.empty()` and the four blocks that
//! override it are all shapes this server cannot place a projectile beside
//! yet. It is not implemented and the omission is named here rather than
//! left as a difference nobody wrote down.

use crate::collide::{Aabb, Axis, CollisionBlocks, CollisionShapes, NoShapeFor, Shape, Vec3};
use crate::projectile::math::{self, Face};
use rustmc_data::BlockPos;
use rustmc_entity::mth::{floor, lerp};

/// A `BlockHitResult` with `getType() == BLOCK`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockHit {
    /// `BlockHitResult.getLocation()`.
    pub location: Vec3,
    /// `BlockHitResult.getDirection()`.
    pub direction: Face,
    /// `BlockHitResult.getBlockPos()`.
    pub pos: BlockPos,
    /// `BlockHitResult.isInside()` — set by `VoxelShape.clip`'s fast path
    /// when the ray's origin is already inside the shape.
    pub inside: bool,
}

/// A `BlockHitResult` with `getType() == MISS`.
///
/// `BlockHitResult.miss(to, Direction.getApproximateNearest(from - to),
/// BlockPos.containing(to))`. The direction is the **reverse** of travel:
/// `lambda$clip$1` computes `from.subtract(to)` and takes the nearest axis of
/// that.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipMiss {
    pub location: Vec3,
    pub direction: Face,
    pub pos: BlockPos,
}

/// What `Level.clipIncludingBorder` can answer, minus the border.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Clip {
    Block(BlockHit),
    Miss(ClipMiss),
}

impl Clip {
    /// `HitResult.getLocation()` — defined on both arms, which is what
    /// `AbstractArrow.stepMoveAndHit` reads without testing the type first.
    #[must_use]
    pub const fn location(&self) -> Vec3 {
        match self {
            Clip::Block(h) => h.location,
            Clip::Miss(m) => m.location,
        }
    }

    #[must_use]
    pub const fn is_miss(&self) -> bool {
        matches!(self, Clip::Miss(_))
    }
}

/// `Direction.getApproximateNearest(double x, double y, double z)`.
///
/// The largest-magnitude component wins; ties go to the earlier axis in the
/// x, y, z scan, and the sign chooses the face. Vanilla's loop is over
/// `Direction.values()` scoring `x*stepX + y*stepY + z*stepZ` and keeping the
/// strict maximum, so the first `Direction` in declaration order wins a tie —
/// and the order is DOWN, UP, NORTH, SOUTH, WEST, EAST.
#[must_use]
pub fn approximate_nearest(x: f64, y: f64, z: f64) -> Face {
    let mut best = Face::Down;
    let mut best_score = f64::MIN;
    for (face, score) in [
        (Face::Down, -y),
        (Face::Up, y),
        (Face::North, -z),
        (Face::South, z),
        (Face::West, -x),
        (Face::East, x),
    ] {
        if score > best_score {
            best_score = score;
            best = face;
        }
    }
    best
}

/// `Direction.getOpposite()`.
#[must_use]
pub const fn opposite(f: Face) -> Face {
    match f {
        Face::Down => Face::Up,
        Face::Up => Face::Down,
        Face::North => Face::South,
        Face::South => Face::North,
        Face::West => Face::East,
        Face::East => Face::West,
    }
}

/// `VoxelShape.clip(Vec3 from, Vec3 to, BlockPos pos)`.
///
/// ```text
///   if (isEmpty()) return null;
///   Vec3 d = to.subtract(from);
///   if (d.lengthSqr() < 1.0E-7) return null;
///   Vec3 probe = from.add(d.scale(0.001));
///   if (shape.isFullWide(findIndex(X, probe.x - pos.x),
///                        findIndex(Y, probe.y - pos.y),
///                        findIndex(Z, probe.z - pos.z)))
///       return new BlockHitResult(probe,
///           Direction.getApproximateNearest(d.x, d.y, d.z).getOpposite(),
///           pos, true);
///   return AABB.clip(toAabbs(), from, to, pos);
/// ```
///
/// **The inside probe is nudged 0.001 of the whole segment forward**, not
/// normalised, so how far it moves depends on the length of this tick's
/// delta. That is what makes an arrow launched from inside a block report
/// `inside = true` with a location a thousandth of a step along.
///
/// # `toAabbs()` is replaced by the occupied cells, and the result is the same
///
/// This server stores a shape as the grid rather than as a merged box list —
/// `docs/cold-start/entity-move-collision.md` says why, and it is a
/// correctness argument about the *collision* sweep, not a shortcut. For a
/// **clip** the two agree exactly: `AABB.clip(Iterable, ...)` shares one `t`
/// across all the boxes and keeps the strictly-nearest crossing, and any face
/// that merging removes is interior to the union, so a ray reaching it has
/// already crossed an outer face at a smaller `t`. Merging can only delete
/// candidates that were already losing. The `inside` fast path above catches
/// the one case where that argument does not hold — a ray starting within the
/// union — before any box is considered.
pub fn shape_clip(shape: &Shape, pos: BlockPos, from: Vec3, to: Vec3) -> Option<BlockHit> {
    if shape.is_empty() {
        return None;
    }
    let d = math::subtract(to, from);
    if d.length_sqr() < 1.0E-7 {
        return None;
    }
    let probe = math::add(from, math::scale(d, 0.001));
    let ix = shape.find_index_at(Axis::X, probe.x - f64::from(pos.x));
    let iy = shape.find_index_at(Axis::Y, probe.y - f64::from(pos.y));
    let iz = shape.find_index_at(Axis::Z, probe.z - f64::from(pos.z));
    if shape.is_full_wide(ix, iy, iz) {
        return Some(BlockHit {
            location: probe,
            direction: opposite(approximate_nearest(d.x, d.y, d.z)),
            pos,
            inside: true,
        });
    }

    // `AABB.clip(Iterable<AABB>, from, to, pos)`: one shared `t`, every box
    // moved by `pos`, the nearest crossing kept.
    let mut t = 1.0_f64;
    let mut face: Option<Face> = None;
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let dz = to.z - from.z;
    for cell in occupied_cells(shape) {
        let moved = math::moved_by_block(&cell, pos);
        if let Some(f) = math::clip_shared(&moved, from, to, &mut t) {
            face = Some(f);
        }
    }
    face.map(|f| BlockHit {
        location: math::add3(from, t * dx, t * dy, t * dz),
        direction: f,
        pos,
        inside: false,
    })
}

/// One `AABB` per occupied cell of the grid.
fn occupied_cells(shape: &Shape) -> Vec<Aabb> {
    let xs = shape.coords(Axis::X);
    let ys = shape.coords(Axis::Y);
    let zs = shape.coords(Axis::Z);
    let mut out = Vec::new();
    for x in 0..shape.size(Axis::X) {
        for y in 0..shape.size(Axis::Y) {
            for z in 0..shape.size(Axis::Z) {
                #[expect(clippy::cast_possible_wrap, reason = "grid indices are small")]
                if shape.is_full_wide(x as i64, y as i64, z as i64) {
                    out.push(Aabb::new(
                        xs[x],
                        ys[y],
                        zs[z],
                        xs[x + 1],
                        ys[y + 1],
                        zs[z + 1],
                    ));
                }
            }
        }
    }
    out
}

/// `BlockGetter.clip(ClipContext)` with `Block.COLLIDER` and `Fluid.NONE`.
///
/// # Errors
///
/// [`NoShapeFor`] when the walk reaches a block whose collision shape is not
/// a function of its state — the same refusal the collision sweep makes, and
/// for the same reason: an arrow that flies through a shulker box and one
/// that stops on powder snow both look like working physics.
pub fn clip<W: CollisionBlocks>(
    shapes: &CollisionShapes,
    world: &W,
    from: Vec3,
    to: Vec3,
) -> Result<Clip, NoShapeFor> {
    let miss = || {
        let back = math::subtract(from, to);
        Clip::Miss(ClipMiss {
            location: to,
            direction: approximate_nearest(back.x, back.y, back.z),
            pos: containing(to),
        })
    };

    let mut hit: Option<BlockHit> = None;
    let mut err: Option<NoShapeFor> = None;
    let missed = traverse_blocks(
        from,
        to,
        &mut |pos| match at(shapes, world, pos, from, to) {
            Err(e) => {
                err = Some(e);
                false
            }
            Ok(None) => true,
            Ok(Some(h)) => {
                hit = Some(h);
                false
            }
        },
    );
    if let Some(e) = err {
        return Err(e);
    }
    if let Some(h) = hit {
        return Ok(Clip::Block(h));
    }
    debug_assert!(missed, "the walk ended with neither a hit nor an error");
    Ok(miss())
}

/// `BlockGetter.traverseBlocks(Vec3 from, Vec3 to, C ctx,
/// BiFunction<C, BlockPos, T> visitor, Function<C, T> miss)`, offsets 0-491.
///
/// The visitor returns `true` to keep walking, which is vanilla's `null`;
/// returning `false` is vanilla's non-null result and stops the walk. The
/// return value here is `true` when the walk ran out — vanilla's `miss`
/// branch at offset 483.
///
/// ```text
///   if (from.equals(to)) return miss.apply(ctx);
///   double ex = lerp(-1.0E-7, to.x, from.x);   // and ey, ez
///   double sx = lerp(-1.0E-7, from.x, to.x);   // and sy, sz
///   int cx = floor(sx), cy = floor(sy), cz = floor(sz);
///   T t = visitor.apply(ctx, mutable(cx, cy, cz));
///   if (t != null) return t;
///   double ddx = ex - sx, ddy = ey - sy, ddz = ez - sz;
///   int sgx = Mth.sign(ddx), sgy = ..., sgz = ...;
///   double stx = sgx == 0 ? Double.MAX_VALUE : sgx / ddx;   // and sty, stz
///   double tx = stx * (sgx > 0 ? 1.0 - frac(sx) : frac(sx));   // and ty, tz
///   while (tx <= 1.0 || ty <= 1.0 || tz <= 1.0) {
///       if (tx < ty) { if (tx < tz) { cx += sgx; tx += stx; }
///                      else         { cz += sgz; tz += stz; } }
///       else if (ty < tz) { cy += sgy; ty += sty; }
///       else              { cz += sgz; tz += stz; }
///       t = visitor.apply(ctx, mutable.set(cx, cy, cz));
///       if (t != null) return t;
///   }
///   return miss.apply(ctx);
/// ```
///
/// **Both endpoints are pulled 1e-7 towards each other before the walk.**
/// `Mth.lerp(-1.0E-7, a, b)` is `a + -1e-7 * (b - a)`, so `sx` is the *from*
/// end nudged towards *to* and `ex` is the *to* end nudged towards *from*.
/// That is what stops a segment ending exactly on a grid line from visiting
/// the block beyond it, and it is applied to the DDA's start cell as well as
/// to its extent.
///
/// **The loop condition is a disjunction of `> 1.0` on all three**, so the
/// walk continues while ANY axis still has ground to cover. Reading it as a
/// conjunction stops the walk at the first axis to run out and loses the
/// tail of every non-axis-aligned ray.
pub fn traverse_blocks(from: Vec3, to: Vec3, visit: &mut dyn FnMut(BlockPos) -> bool) -> bool {
    if from == to {
        return true;
    }
    let ex = lerp(-1.0E-7, to.x, from.x);
    let ey = lerp(-1.0E-7, to.y, from.y);
    let ez = lerp(-1.0E-7, to.z, from.z);
    let sx = lerp(-1.0E-7, from.x, to.x);
    let sy = lerp(-1.0E-7, from.y, to.y);
    let sz = lerp(-1.0E-7, from.z, to.z);

    let mut cx = floor(sx);
    let mut cy = floor(sy);
    let mut cz = floor(sz);
    if !visit(BlockPos {
        x: cx,
        y: cy,
        z: cz,
    }) {
        return false;
    }

    let ddx = ex - sx;
    let ddy = ey - sy;
    let ddz = ez - sz;
    let signx = sign(ddx);
    let signy = sign(ddy);
    let signz = sign(ddz);
    let stepx = if signx == 0 {
        f64::MAX
    } else {
        f64::from(signx) / ddx
    };
    let stepy = if signy == 0 {
        f64::MAX
    } else {
        f64::from(signy) / ddy
    };
    let stepz = if signz == 0 {
        f64::MAX
    } else {
        f64::from(signz) / ddz
    };
    let mut tx = stepx * if signx > 0 { 1.0 - frac(sx) } else { frac(sx) };
    let mut ty = stepy * if signy > 0 { 1.0 - frac(sy) } else { frac(sy) };
    let mut tz = stepz * if signz > 0 { 1.0 - frac(sz) } else { frac(sz) };

    while tx <= 1.0 || ty <= 1.0 || tz <= 1.0 {
        if tx < ty {
            if tx < tz {
                cx += signx;
                tx += stepx;
            } else {
                cz += signz;
                tz += stepz;
            }
        } else if ty < tz {
            cy += signy;
            ty += stepy;
        } else {
            cz += signz;
            tz += stepz;
        }
        if !visit(BlockPos {
            x: cx,
            y: cy,
            z: cz,
        }) {
            return false;
        }
    }
    true
}

fn at<W: CollisionBlocks>(
    shapes: &CollisionShapes,
    world: &W,
    pos: BlockPos,
    from: Vec3,
    to: Vec3,
) -> Result<Option<BlockHit>, NoShapeFor> {
    let state_id = world.state_id_at(pos);
    let shape = shapes.get(state_id).ok_or(NoShapeFor { pos, state_id })?;
    Ok(shape_clip(shape, pos, from, to))
}

/// `Mth.sign(double)` — `d == 0 ? 0 : (d > 0 ? 1 : -1)`.
const fn sign(d: f64) -> i32 {
    if d == 0.0 {
        0
    } else if d > 0.0 {
        1
    } else {
        -1
    }
}

/// `Mth.frac(double)` — `d - Mth.lfloor(d)`.
fn frac(d: f64) -> f64 {
    d - (d.floor())
}

/// `BlockPos.containing(Position)` — `Mth.floor` on each component.
fn containing(v: Vec3) -> BlockPos {
    BlockPos {
        x: floor(v.x),
        y: floor(v.y),
        z: floor(v.z),
    }
}
