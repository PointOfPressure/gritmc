//! `BucketItem` — filling a bucket from a fluid and emptying one into the
//! world — and the ray trace both start from.
//!
//! Every line here is read from `javap -p -c -constants` on the pinned
//! `server-26.2.jar`: `BucketItem.use`, `emptyContents`, `playEmptySound`,
//! `getFluidContext`; `Item.getPlayerPOVHitResult`; `Entity.
//! calculateViewVector`; `BlockGetter.clip`/`traverseBlocks`;
//! `VoxelShape.clip`; `AABB.clip`; `LiquidBlock.pickupBlock` and
//! `SimpleWaterloggedBlock.pickupBlock`/`placeLiquid`.
//!
//! # A bucket is not a `BlockItem`, and that is the whole reason this exists
//!
//! `BucketItem` overrides `use`, not `useOn`: it is reached through the
//! `use_item` packet, which carries **no hit result**. The server casts its
//! own ray from the player's eyes, along the rotation the packet carries, for
//! `blockInteractionRange()` — 4.5, plus the 0.5 of `ServerPlayer`'s
//! `creative_mode_block_range` modifier in creative. A right-click *on a face*
//! (`use_item_on`) with a bucket reaches `Item.useOn`, which is `PASS`, and
//! then this.
//!
//! # What the ray hits, and the one thing it approximates
//!
//! `ClipContext.Block.OUTLINE` clips against `getShape`, the outline. This
//! server carries the **collision** shape per state
//! (`data/vanilla/collision_shapes.json`), and the two are the same geometry
//! for every full block, slab, stair and the superflat floor. They differ
//! for blocks with an outline and no collision (short grass, flowers,
//! torches — the ray passes through here and stops on the block in vanilla)
//! and for the tall collision boxes of fences and walls. Named rather than
//! hidden: neither is reachable from a scenario on bare superflat.
//!
//! The fluid half is exact: `ClipContext.Fluid.SOURCE_ONLY` for an empty
//! bucket, `NONE` for a full one, against `FlowingFluid.getShape` — a box of
//! the fluid's own height.

use crate::fluid::{FluidState, Kind};
use crate::interact::{BlockPos, Face};

/// `Attributes.BLOCK_INTERACTION_RANGE`'s default.
pub use crate::containers::BLOCK_INTERACTION_RANGE as REACH;

/// `ServerPlayer.CREATIVE_BLOCK_INTERACTION_RANGE_MODIFIER` — `ldc2_w 0.5`,
/// `ADD_VALUE`, applied by `updatePlayerAttributes` while `isCreative()`.
pub const CREATIVE_REACH_BONUS: f64 = 0.5;

/// `Player`'s standing eye height, `1.62f` widened — `getEyePosition` adds the
/// float, so the double is `1.6200000047683716`.
pub const EYE_HEIGHT: f64 = 1.62_f32 as f64;

/// `Entity.calculateViewVector(xRot, yRot)`:
///
/// ```text
///   f = xRot * 0.017453292f;  g = -yRot * 0.017453292f;     // float
///   h = Mth.cos(g); i = Mth.sin(g); j = Mth.cos(f); k = Mth.sin(f);
///   return new Vec3(i * j, -k, h * j);                      // float products
/// ```
///
/// `Mth.sin`/`cos` are the 65,536-entry table, not `f64::sin`, which is why
/// this goes through `rustmc_worldgen::math`.
#[must_use]
pub fn view_vector(x_rot: f32, y_rot: f32) -> [f64; 3] {
    let f = x_rot * 0.017_453_292_f32;
    let g = -y_rot * 0.017_453_292_f32;
    let h = rustmc_worldgen::math::cos(f64::from(g));
    let i = rustmc_worldgen::math::sin(f64::from(g));
    let j = rustmc_worldgen::math::cos(f64::from(f));
    let k = rustmc_worldgen::math::sin(f64::from(f));
    [f64::from(i * j), f64::from(-k), f64::from(h * j)]
}

/// A `BlockHitResult` of type `BLOCK`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    pub pos: BlockPos,
    pub face: Face,
    pub location: [f64; 3],
}

/// What the ray is asked to stop on at one position: the boxes of the block's
/// shape and, when the context picks fluids, the fluid's own box.
pub struct Shapes<'a> {
    /// The block's shape at a position, as boxes in block-local coordinates.
    pub block: &'a dyn Fn(BlockPos) -> Vec<[f64; 6]>,
    /// The fluid's shape at a position, already filtered by the
    /// `ClipContext.Fluid` — empty when the context does not pick it.
    pub fluid: &'a dyn Fn(BlockPos) -> Vec<[f64; 6]>,
}

/// `Mth.lerp(delta, a, b)`.
fn lerp(delta: f64, a: f64, b: f64) -> f64 {
    a + delta * (b - a)
}

/// `Mth.frac(v)` — `v - lfloor(v)`.
fn frac(v: f64) -> f64 {
    v - v.floor()
}

/// `Mth.floor(double)`.
#[expect(
    clippy::cast_possible_truncation,
    reason = "block coordinates, bounded by the reach"
)]
fn floor(v: f64) -> i32 {
    v.floor() as i32
}

fn sign(v: f64) -> i32 {
    if v > 0.0 {
        1
    } else if v < 0.0 {
        -1
    } else {
        0
    }
}

/// `AABB.clip(boxes, from, to, pos)` and the `clipPoint` ladder under it:
/// for each box moved to `pos`, X faces then Y then Z, each accepted only if
/// `0 < t < best` and the crossing lies within the other two ranges padded by
/// `1e-7`. `best` starts at 1.0, so a hit past `to` is no hit.
fn clip_boxes(boxes: &[[f64; 6]], from: [f64; 3], to: [f64; 3], pos: BlockPos) -> Option<Hit> {
    let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    let mut best = 1.0f64;
    let mut face = None;
    let off = [f64::from(pos.0), f64::from(pos.1), f64::from(pos.2)];
    for b in boxes {
        let lo = [b[0] + off[0], b[1] + off[1], b[2] + off[2]];
        let hi = [b[3] + off[0], b[4] + off[1], b[5] + off[2]];
        // (axis, plane, face)
        let mut try_plane = |axis: usize, plane: f64, f: Face| {
            let t = (plane - from[axis]) / d[axis];
            let (a, c) = ((axis + 1) % 3, (axis + 2) % 3);
            let pa = from[a] + t * d[a];
            let pc = from[c] + t * d[c];
            if 0.0 < t
                && t < best
                && lo[a] - 1.0e-7 < pa
                && pa < hi[a] + 1.0e-7
                && lo[c] - 1.0e-7 < pc
                && pc < hi[c] + 1.0e-7
            {
                best = t;
                face = Some(f);
            }
        };
        if d[0] > 1.0e-7 {
            try_plane(0, lo[0], Face::West);
        } else if d[0] < -1.0e-7 {
            try_plane(0, hi[0], Face::East);
        }
        if d[1] > 1.0e-7 {
            try_plane(1, lo[1], Face::Down);
        } else if d[1] < -1.0e-7 {
            try_plane(1, hi[1], Face::Up);
        }
        if d[2] > 1.0e-7 {
            try_plane(2, lo[2], Face::North);
        } else if d[2] < -1.0e-7 {
            try_plane(2, hi[2], Face::South);
        }
    }
    face.map(|face| Hit {
        pos,
        face,
        location: [
            from[0] + best * d[0],
            from[1] + best * d[1],
            from[2] + best * d[2],
        ],
    })
}

/// `VoxelShape.clip(from, to, pos)`:
///
/// ```text
///   if (isEmpty()) return null;
///   vec = to - from; if (vec.lengthSqr() < 1.0E-7) return null;
///   v2 = from + vec * 0.001;
///   if (shape.isFullWide(cell of v2 - pos))
///       return new BlockHitResult(v2, Direction.getApproximateNearest(vec).getOpposite(), pos, true);
///   return AABB.clip(toAabbs(), from, to, pos);
/// ```
fn clip_shape(boxes: &[[f64; 6]], from: [f64; 3], to: [f64; 3], pos: BlockPos) -> Option<Hit> {
    if boxes.is_empty() {
        return None;
    }
    let v = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    if v[0] * v[0] + v[1] * v[1] + v[2] * v[2] < 1.0e-7 {
        return None;
    }
    let v2 = [
        from[0] + v[0] * 0.001,
        from[1] + v[1] * 0.001,
        from[2] + v[2] * 0.001,
    ];
    let local = [
        v2[0] - f64::from(pos.0),
        v2[1] - f64::from(pos.1),
        v2[2] - f64::from(pos.2),
    ];
    let inside = boxes.iter().any(|b| {
        (b[0]..b[3]).contains(&local[0])
            && (b[1]..b[4]).contains(&local[1])
            && (b[2]..b[5]).contains(&local[2])
    });
    if inside {
        return Some(Hit {
            pos,
            face: approximate_nearest(v).opposite(),
            location: v2,
        });
    }
    clip_boxes(boxes, from, to, pos)
}

/// `Direction.getApproximateNearest(x, y, z)` — the `Direction.values()`
/// entry with the largest dot product, the first winning a tie.
fn approximate_nearest(v: [f64; 3]) -> Face {
    let mut best = Face::North;
    let mut best_dot = f64::MIN;
    for f in [
        Face::Down,
        Face::Up,
        Face::North,
        Face::South,
        Face::West,
        Face::East,
    ] {
        let n = f.normal();
        let dot = v[0] * f64::from(n.0) + v[1] * f64::from(n.1) + v[2] * f64::from(n.2);
        if dot > best_dot {
            best_dot = dot;
            best = f;
        }
    }
    best
}

/// `BlockGetter.clip(ctx)` over `traverseBlocks`, with the per-position
/// lambda that clips the block shape and the fluid shape and keeps the
/// nearer (`d <= e ? blockHit : fluidHit` — the block wins a tie).
///
/// `traverseBlocks` nudges both ends by `1e-7` — the end toward the start,
/// the start away from the end — before it floors anything, and visits the
/// starting cell first. The DDA ties go to Z over X when `v == x`, and to Z
/// over Y when `w == x`: `if (v < w) { v < x ? X : Z } else { w < x ? Y : Z }`.
#[must_use]
pub fn clip(from: [f64; 3], to: [f64; 3], shapes: &Shapes<'_>) -> Option<Hit> {
    if from == to {
        return None;
    }
    let end = [
        lerp(-1.0e-7, to[0], from[0]),
        lerp(-1.0e-7, to[1], from[1]),
        lerp(-1.0e-7, to[2], from[2]),
    ];
    let start = [
        lerp(-1.0e-7, from[0], to[0]),
        lerp(-1.0e-7, from[1], to[1]),
        lerp(-1.0e-7, from[2], to[2]),
    ];
    let visit = |pos: BlockPos| -> Option<Hit> {
        let block = clip_shape(&(shapes.block)(pos), from, to, pos);
        let fluid = clip_shape(&(shapes.fluid)(pos), from, to, pos);
        let dist = |h: &Option<Hit>| {
            h.as_ref().map_or(f64::MAX, |h| {
                let l = h.location;
                (l[0] - from[0]).powi(2) + (l[1] - from[1]).powi(2) + (l[2] - from[2]).powi(2)
            })
        };
        if dist(&block) <= dist(&fluid) {
            block
        } else {
            fluid
        }
    };
    let (mut j, mut k, mut l) = (floor(start[0]), floor(start[1]), floor(start[2]));
    if let Some(h) = visit((j, k, l)) {
        return Some(h);
    }
    let m = end[0] - start[0];
    let n = end[1] - start[1];
    let o = end[2] - start[2];
    let (p, q, r) = (sign(m), sign(n), sign(o));
    let s = if p == 0 { f64::MAX } else { f64::from(p) / m };
    let t = if q == 0 { f64::MAX } else { f64::from(q) / n };
    let u = if r == 0 { f64::MAX } else { f64::from(r) / o };
    let mut v = s * if p > 0 {
        1.0 - frac(start[0])
    } else {
        frac(start[0])
    };
    let mut w = t * if q > 0 {
        1.0 - frac(start[1])
    } else {
        frac(start[1])
    };
    let mut x = u * if r > 0 {
        1.0 - frac(start[2])
    } else {
        frac(start[2])
    };
    while v <= 1.0 || w <= 1.0 || x <= 1.0 {
        if v < w {
            if v < x {
                j += p;
                v += s;
            } else {
                l += r;
                x += u;
            }
        } else if w < x {
            k += q;
            w += t;
        } else {
            l += r;
            x += u;
        }
        if let Some(h) = visit((j, k, l)) {
            return Some(h);
        }
    }
    None
}

/// `ClipContext.Fluid` — which fluids a ray stops on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FluidMode {
    /// `NONE` — a full bucket's ray passes through every fluid.
    None,
    /// `SOURCE_ONLY` — an empty bucket's ray stops on a source.
    SourceOnly,
}

/// `BucketItem.getFluidContext()`: `content == EMPTY ? SOURCE_ONLY : NONE`.
#[must_use]
pub const fn fluid_context(content: Kind) -> FluidMode {
    match content {
        Kind::Empty => FluidMode::SourceOnly,
        _ => FluidMode::None,
    }
}

/// `FlowingFluid.getShape(state, level, pos)` filtered by the clip context:
/// `amount == 9 && hasSameAbove ? block() : box(0, 0, 0, 1, getHeight, 1)`.
/// `getAmount` never reaches 9, so it is always the height box; the height is
/// 1.0 under the same liquid and `amount / 9` otherwise.
#[must_use]
pub fn fluid_boxes(mode: FluidMode, here: FluidState, above: FluidState) -> Vec<[f64; 6]> {
    if mode == FluidMode::None || here.is_empty() || !here.is_source() {
        return Vec::new();
    }
    let h = if here.is_same(above) {
        1.0
    } else {
        f64::from(f32::from(here.amount()) / 9.0)
    };
    vec![[0.0, 0.0, 0.0, 1.0, h, 1.0]]
}

/// What a bucket holds, by item id. `None` for anything that is not a plain
/// `BucketItem`.
///
/// **The six mob buckets are refused, not mapped to water.** `MobBucketItem`
/// overrides `checkExtraContent` to create the mob (`EntityType.create(...,
/// EntitySpawnReason.BUCKET)`, `loadFromBucketTag`, `setFromBucket(true)`,
/// `addFreshEntityWithPassengers`, `playAmbientSound`) and `playEmptySound` to
/// play its own sound at `SoundSource.NEUTRAL`. Emptying their water here with
/// `BUCKET_EMPTY` would be a packet vanilla never sends and a fish that never
/// appears; this server does not tick fish, so the mob half is out of reach
/// and the item stays a `PASS`.
#[must_use]
pub fn content_of(item: &str) -> Option<Kind> {
    Some(match item {
        "minecraft:bucket" => Kind::Empty,
        "minecraft:water_bucket" => Kind::Water,
        "minecraft:lava_bucket" => Kind::Lava,
        _ => return None,
    })
}

/// `SoundEvents.BUCKET_EMPTY` and `BUCKET_EMPTY_LAVA` —
/// `BucketItem.playEmptySound` picks by `content.is(FluidTags.LAVA)`.
#[must_use]
pub const fn empty_sound(content: Kind) -> &'static str {
    match content {
        Kind::Lava => "minecraft:item.bucket.empty_lava",
        _ => "minecraft:item.bucket.empty",
    }
}

/// `getPickupSound()` of what was picked up: `WaterFluid` and every
/// `SimpleWaterloggedBlock` answer `BUCKET_FILL`, `LavaFluid`
/// `BUCKET_FILL_LAVA`.
#[must_use]
pub const fn fill_sound(picked: Kind) -> &'static str {
    match picked {
        Kind::Lava => "minecraft:item.bucket.fill_lava",
        _ => "minecraft:item.bucket.fill",
    }
}

/// `BlockState.canBeReplaced(Fluid)` → `BlockBehaviour.canBeReplaced(state,
/// fluid)`: `state.canBeReplaced() || !state.isSolid()`. The fluid argument
/// is not read; only `EndPortalBlock` and `EndGatewayBlock` override it, to
/// `false`.
fn can_be_replaced_by_fluid(state: &str, replaceable: bool, solid: bool) -> bool {
    if matches!(
        state.split('[').next().unwrap_or(state),
        "minecraft:end_portal" | "minecraft:end_gateway"
    ) {
        return false;
    }
    replaceable || !solid
}

/// What `emptyContents` needs to know about a state beyond its string.
pub struct StateFacts<'a> {
    /// The no-argument `canBeReplaced()` — `block_props.json`'s
    /// `replaceable`.
    pub replaceable: &'a dyn Fn(&str) -> bool,
    /// `isSolid()`.
    pub solid: &'a dyn Fn(&str) -> bool,
}

/// `BucketItem.emptyContents(entity, level, pos, hit)`, offsets 0-463.
///
/// ```text
///   state = level.getBlockState(pos); block = state.getBlock();
///   canBeReplaced = state.canBeReplaced(content);
///   shift = entity != null && entity.isShiftKeyDown();
///   canPlace = canBeReplaced
///           || block instanceof LiquidBlockContainer c && c.canPlaceLiquid(entity, level, pos, state, content);
///   ok = state.isAir() || canPlace && (!shift || hit == null);
///   if (!ok) return hit != null && emptyContents(entity, level, hit.pos.relative(hit.dir), null);
///   if (WATER_EVAPORATES at pos && content is water) { fizz sound + smoke; return true; }
///   if (block instanceof LiquidBlockContainer c && content == WATER) {
///       c.placeLiquid(level, pos, state, getSource(false)); playEmptySound; return true;
///   }
///   if (canBeReplaced && !state.liquid()) level.destroyBlock(pos, true);
///   if (level.setBlock(pos, content.defaultFluidState().createLegacyBlock(), 11)
///           || state.getFluidState().isSource()) { playEmptySound; return true; }
///   return false;
/// ```
///
/// `WATER_EVAPORATES` is the nether's environment attribute; this server's
/// block world is the overworld, where it is false. `destroyBlock(pos, true)`
/// writes the state's legacy fluid with level event 2001 — what
/// `crate::shapes::destroy_block` does — and its drops are not ported (no
/// block destruction drops anything in this server).
///
/// Returns the position the fluid went to, for the caller's sound.
#[expect(clippy::too_many_arguments, reason = "the bytecode's own signature")]
pub fn empty_contents(
    blocks: &dyn crate::edits::Blocks,
    shapes: Option<&crate::shapes::ShapeTable>,
    facts: &StateFacts<'_>,
    content: Kind,
    pos: BlockPos,
    hit: Option<Hit>,
    shift: bool,
    out: &mut crate::shapes::Effects,
) -> Option<BlockPos> {
    let state = blocks.state_at(pos);
    let can_be_replaced =
        can_be_replaced_by_fluid(&state, (facts.replaceable)(&state), (facts.solid)(&state));
    let container = crate::fluid::is_liquid_container(&state);
    // `SimpleWaterloggedBlock.canPlaceLiquid` — `fluid == Fluids.WATER`.
    let can_place = can_be_replaced || (container && content == Kind::Water);
    let ok = crate::shapes::is_air(&state) || (can_place && (!shift || hit.is_none()));
    if !ok {
        let h = hit?;
        return empty_contents(
            blocks,
            shapes,
            facts,
            content,
            h.face.relative(h.pos),
            None,
            shift,
            out,
        );
    }
    // `WATER_EVAPORATES` (offsets 184-342): the nether. A fizz sound and eight
    // smoke particles, `true`, and no block. The pitch is
    // `2.6f + (r.nextFloat() - r.nextFloat()) * 0.8f` from the level random,
    // which this server does not share — reproducible in range only.
    if content == Kind::Water && blocks.water_evaporates() {
        #[expect(clippy::cast_precision_loss, reason = "a 24-bit draw into a float")]
        let r = |n: u32| (n % (1 << 24)) as f32 / (1 << 24) as f32;
        let (a, b) = (
            r(crate::fluid::level_random_next_int(1 << 24)),
            r(crate::fluid::level_random_next_int(1 << 24)),
        );
        out.sounds.push(block_sound(
            "minecraft:block.fire.extinguish",
            pos,
            0.5,
            2.6 + (a - b) * 0.8,
        ));
        return Some(pos);
    }
    if container && content == Kind::Water {
        crate::fluid::place_liquid(
            blocks,
            shapes,
            pos,
            &state,
            FluidState::source(Kind::Water, false),
            out,
        );
        out.sounds
            .push(block_sound(empty_sound(content), pos, 1.0, 1.0));
        return Some(pos);
    }
    let existing = blocks.fluid_of_state(&state);
    if can_be_replaced && !crate::fluid::is_liquid_block(&state) && !crate::shapes::is_air(&state) {
        // `BucketItem.emptyContents`: `level.destroyBlock(pos, true)` — the
        // replaced block drops.
        crate::shapes::destroy_block(
            blocks,
            shapes,
            pos,
            true,
            crate::shapes::RECURSION_LIMIT,
            out,
        );
    }
    let written = crate::shapes::set_block(
        blocks,
        shapes,
        pos,
        &FluidState::source(content, false).legacy_block(),
        crate::shapes::UPDATE_NEIGHBORS | crate::shapes::UPDATE_CLIENTS | UPDATE_IMMEDIATE,
        crate::shapes::RECURSION_LIMIT,
        out,
    );
    if !(written || existing.is_source()) {
        return None;
    }
    out.sounds
        .push(block_sound(empty_sound(content), pos, 1.0, 1.0));
    Some(pos)
}

/// `level.playSound(entity, pos, sound, SoundSource.BLOCKS, volume, pitch)`
/// — at the block's centre. `except` is the caller's: every bucket sound
/// excludes the player who used it, and only the session knows who that is.
fn block_sound(event: &str, pos: BlockPos, volume: f32, pitch: f32) -> crate::sounds::Sound {
    crate::sounds::Sound {
        event: event.to_string(),
        source: crate::sounds::SOURCE_BLOCKS,
        at: crate::sounds::Eighths::block(pos),
        pos: crate::sounds::block_centre(pos),
        volume,
        pitch,
        except: None,
    }
}

/// `Block.UPDATE_IMMEDIATE` — flag 8, which `emptyContents` ORs into its
/// `setBlock` (`bipush 11`). It changes when a client re-renders and nothing
/// a server sends.
pub const UPDATE_IMMEDIATE: i32 = 8;

/// `BucketPickup.pickupBlock(entity, level, pos, state)` for the pickups
/// this server holds:
///
/// ```text
///   LiquidBlock:             if (state.LEVEL == 0) { setBlock(pos, AIR, 11); return new ItemStack(fluid.getBucket()); }
///                            return EMPTY;
///   SimpleWaterloggedBlock:  if (WATERLOGGED) { setBlock(pos, state.setValue(WATERLOGGED, false), 3);
///                                if (!state.canSurvive(level, pos)) level.destroyBlock(pos, true);
///                                return new ItemStack(WATER_BUCKET); }
///                            return EMPTY;
/// ```
///
/// Returns what was picked up. The waterlogged `canSurvive` re-check is not
/// ported: a block that could stand waterlogged and cannot stand dry is
/// nothing a bucket meets on superflat.
pub fn pickup_block(
    blocks: &dyn crate::edits::Blocks,
    shapes: Option<&crate::shapes::ShapeTable>,
    pos: BlockPos,
    out: &mut crate::shapes::Effects,
) -> Option<Kind> {
    let state = blocks.state_at(pos);
    if crate::fluid::is_liquid_block(&state) {
        let f = blocks.fluid_of_state(&state);
        if !f.is_source() {
            return None;
        }
        crate::shapes::set_block(
            blocks,
            shapes,
            pos,
            "minecraft:air",
            crate::shapes::UPDATE_NEIGHBORS | crate::shapes::UPDATE_CLIENTS | UPDATE_IMMEDIATE,
            crate::shapes::RECURSION_LIMIT,
            out,
        );
        return Some(f.kind);
    }
    if crate::placement::property_of(&state, "waterlogged") == Some("true") {
        let dry = crate::placement::with_property(&state, "waterlogged", "false");
        crate::shapes::set_block(
            blocks,
            shapes,
            pos,
            &dry,
            crate::shapes::UPDATE_ALL,
            crate::shapes::RECURSION_LIMIT,
            out,
        );
        return Some(Kind::Water);
    }
    None
}

/// The item a bucket of `kind` is.
#[must_use]
pub const fn bucket_of(kind: Kind) -> &'static str {
    match kind {
        Kind::Water => "minecraft:water_bucket",
        Kind::Lava => "minecraft:lava_bucket",
        Kind::Empty => "minecraft:bucket",
    }
}

/// What one `BucketItem.use` did, for the caller to send.
#[derive(Debug, Default)]
pub struct Used {
    /// The writes, the level events and the sounds. Every sound in it
    /// excludes the player who used the bucket — `playEmptySound` passes the
    /// entity and the fill goes through `player.playSound` — and is left with
    /// `except: None` for the session to fill in. A fill's sound is at the
    /// player and carries source `PLAYERS`; the session places it.
    pub effects: crate::shapes::Effects,
    /// `Some` when a fill happened: its sound, to be played at the player.
    pub fill_sound: Option<&'static str>,
    /// What the hand should now hold, when the item changed. `None` in
    /// creative, where `ItemUtils.createFilledResult` and
    /// `getEmptySuccessItem` both hand back the stack that was used.
    pub held_after: Option<&'static str>,
    /// In creative, the filled bucket `createFilledResult` adds to the
    /// inventory **if the inventory does not already contain one** (offsets
    /// 1-36: `hasInfiniteMaterials && limitCreativeStackSize`, then
    /// `Inventory.contains`, then `Inventory.add`). An emptying hands the
    /// used stack back to itself, which the inventory always contains, so it
    /// is only ever a fill.
    pub creative_result: Option<&'static str>,
}

/// `BucketItem.use(level, player, hand)`, offsets 0-366.
///
/// ```text
///   hit = getPlayerPOVHitResult(level, player, getFluidContext());
///   if (hit.getType() == MISS) return PASS;
///   if (hit.getType() != BLOCK) return PASS;
///   pos = hit.getBlockPos(); dir = hit.getDirection(); rel = pos.relative(dir);
///   if (!level.mayInteract(player, pos) || !player.mayUseItemAt(rel, dir, stack)) return FAIL;
///   if (content == EMPTY) {
///       state = level.getBlockState(pos);
///       if (state.getBlock() instanceof BucketPickup p) {
///           ItemStack r = p.pickupBlock(player, level, pos, state);
///           if (!r.isEmpty()) { ...; p.getPickupSound().ifPresent(s -> player.playSound(s, 1, 1)); ...;
///               return SUCCESS.heldItemTransformedTo(createFilledResult(stack, player, r)); }
///       }
///       return FAIL;
///   }
///   target = state.getBlock() instanceof LiquidBlockContainer && content == WATER ? pos : rel;
///   if (emptyContents(player, level, target, hit)) { ...;
///       return SUCCESS.heldItemTransformedTo(createFilledResult(stack, player, getEmptySuccessItem(stack, player))); }
///   return FAIL;
/// ```
///
/// `mayInteract` is the spawn-protection and world-border test; this server
/// runs `spawn-protection=0` and every workspace is inside the border.
#[expect(clippy::too_many_arguments, reason = "the bytecode's own inputs")]
pub fn use_bucket(
    blocks: &dyn crate::edits::Blocks,
    shapes: Option<&crate::shapes::ShapeTable>,
    facts: &StateFacts<'_>,
    block_boxes: &dyn Fn(BlockPos) -> Vec<[f64; 6]>,
    content: Kind,
    eye: [f64; 3],
    rot: (f32, f32),
    reach: f64,
    shift: bool,
    creative: bool,
) -> Used {
    let mut used = Used::default();
    let view = view_vector(rot.0, rot.1);
    let to = [
        eye[0] + view[0] * reach,
        eye[1] + view[1] * reach,
        eye[2] + view[2] * reach,
    ];
    let mode = fluid_context(content);
    let fluid = |p: BlockPos| {
        fluid_boxes(
            mode,
            blocks.fluid_at(p),
            blocks.fluid_at((p.0, p.1 + 1, p.2)),
        )
    };
    let Some(hit) = clip(
        eye,
        to,
        &Shapes {
            block: block_boxes,
            fluid: &fluid,
        },
    ) else {
        return used;
    };
    if content == Kind::Empty {
        if let Some(picked) = pickup_block(blocks, shapes, hit.pos, &mut used.effects) {
            used.fill_sound = Some(fill_sound(picked));
            if creative {
                used.creative_result = Some(bucket_of(picked));
            } else {
                used.held_after = Some(bucket_of(picked));
            }
        }
        return used;
    }
    let clicked = blocks.state_at(hit.pos);
    let target = if crate::fluid::is_liquid_container(&clicked) && content == Kind::Water {
        hit.pos
    } else {
        hit.face.relative(hit.pos)
    };
    let emptied = empty_contents(
        blocks,
        shapes,
        facts,
        content,
        target,
        Some(hit),
        shift,
        &mut used.effects,
    )
    .is_some();
    if emptied && !creative {
        used.held_after = Some(bucket_of(Kind::Empty));
    }
    used
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f64; 3], b: [f64; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6)
    }

    /// `calculateViewVector` — yaw 0 is south (+z), 90 west, 180 north, 270
    /// east; pitch 90 is straight down. **Pitch is the first argument.**
    #[test]
    fn the_view_vector_is_vanillas_and_pitch_comes_first() {
        assert!(close(view_vector(0.0, 0.0), [0.0, 0.0, 1.0]));
        assert!(close(view_vector(0.0, 90.0), [-1.0, 0.0, 0.0]));
        assert!(close(view_vector(0.0, 180.0), [0.0, 0.0, -1.0]));
        assert!(close(view_vector(0.0, 270.0), [1.0, 0.0, 0.0]));
        assert!(close(view_vector(90.0, 0.0), [0.0, -1.0, 0.0]));
        assert!(close(view_vector(-90.0, 0.0), [0.0, 1.0, 0.0]));
    }

    fn cube(_: BlockPos) -> Vec<[f64; 6]> {
        vec![[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]]
    }

    fn nothing(_: BlockPos) -> Vec<[f64; 6]> {
        Vec::new()
    }

    /// Looking straight down from a block centre lands on the floor's top
    /// face, and the bucket empties into the cell above it.
    #[test]
    fn looking_down_hits_the_floor_through_its_up_face() {
        let floor = |p: BlockPos| if p.1 <= 63 { cube(p) } else { nothing(p) };
        let eye = [0.5, 64.0 + EYE_HEIGHT, 0.5];
        let v = view_vector(90.0, 0.0);
        let to = [
            eye[0] + v[0] * 4.5,
            eye[1] + v[1] * 4.5,
            eye[2] + v[2] * 4.5,
        ];
        let hit = clip(
            eye,
            to,
            &Shapes {
                block: &floor,
                fluid: &nothing,
            },
        )
        .expect("the floor is in reach");
        assert_eq!(hit.pos, (0, 63, 0));
        assert_eq!(hit.face, Face::Up);
        assert_eq!(hit.face.relative(hit.pos), (0, 64, 0));
    }

    /// A floor beyond the reach is a miss — `AABB.clip`'s `best` starts at
    /// 1.0, the end of the ray.
    #[test]
    fn a_floor_out_of_reach_is_a_miss() {
        let deep = |p: BlockPos| if p.1 <= 59 { cube(p) } else { nothing(p) };
        let eye = [0.5, 64.0 + EYE_HEIGHT, 0.5];
        let to = [0.5, eye[1] - 4.5, 0.5];
        assert_eq!(
            clip(
                eye,
                to,
                &Shapes {
                    block: &deep,
                    fluid: &nothing
                }
            ),
            None
        );
    }

    /// A ray entering a wall horizontally hits the face it came through.
    #[test]
    fn a_horizontal_ray_enters_through_the_facing_side() {
        let wall = |p: BlockPos| if p.0 >= 3 { cube(p) } else { nothing(p) };
        let eye = [0.5, 64.0 + EYE_HEIGHT, 0.5];
        let to = [5.0, eye[1], 0.5];
        let hit = clip(
            eye,
            to,
            &Shapes {
                block: &wall,
                fluid: &nothing,
            },
        )
        .expect("the wall is in reach");
        assert_eq!(hit.pos, (3, 65, 0));
        assert_eq!(hit.face, Face::West);
    }

    /// A bottom slab is hit on its own top face at half height — the shape,
    /// not the cell.
    #[test]
    fn a_slab_is_hit_on_its_own_top_face() {
        let slab = |p: BlockPos| {
            if p == (0, 64, 0) {
                vec![[0.0, 0.0, 0.0, 1.0, 0.5, 1.0]]
            } else if p.1 <= 63 {
                cube(p)
            } else {
                nothing(p)
            }
        };
        let eye = [0.5, 65.0 + EYE_HEIGHT, 0.5];
        let to = [0.5, eye[1] - 4.5, 0.5];
        let hit = clip(
            eye,
            to,
            &Shapes {
                block: &slab,
                fluid: &nothing,
            },
        )
        .expect("the slab is in reach");
        assert_eq!(hit.pos, (0, 64, 0));
        assert_eq!(hit.face, Face::Up);
        assert!((hit.location[1] - 64.5).abs() < 1e-9);
    }

    /// `SOURCE_ONLY` stops on a source's own surface; `NONE` passes through
    /// it to the floor; a flowing block is never picked.
    #[test]
    fn an_empty_bucket_stops_on_a_source_and_a_full_one_does_not() {
        let src = FluidState::source(Kind::Water, false);
        let flowing = FluidState::flowing(Kind::Water, 7, false);
        assert_eq!(
            fluid_boxes(FluidMode::SourceOnly, src, crate::fluid::NOTHING),
            vec![[0.0, 0.0, 0.0, 1.0, f64::from(8.0f32 / 9.0), 1.0]]
        );
        assert!(fluid_boxes(FluidMode::None, src, crate::fluid::NOTHING).is_empty());
        assert!(fluid_boxes(FluidMode::SourceOnly, flowing, crate::fluid::NOTHING).is_empty());
        assert_eq!(
            fluid_boxes(FluidMode::SourceOnly, src, src)[0][4],
            1.0,
            "the same liquid above makes it a full block"
        );
    }
}

/// `BucketItem.use` end to end over a small world: the ray, the target, the
/// write, the sound and the hand.
#[cfg(test)]
mod use_tests {
    use super::*;
    use crate::edits::Blocks;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    /// Grass at y <= 63, air above, plus whatever a test writes. Full cubes
    /// for grass, dirt and stone; a half box for a bottom slab; nothing for
    /// air and liquids — the collision shapes the real table has.
    #[derive(Default)]
    struct World {
        blocks: Mutex<BTreeMap<BlockPos, String>>,
        nether: bool,
    }

    impl World {
        fn with(at: &[(BlockPos, &str)]) -> Self {
            let w = Self::default();
            for (p, s) in at {
                w.blocks.lock().expect("w").insert(*p, (*s).to_string());
            }
            w
        }
        fn boxes(&self, p: BlockPos) -> Vec<[f64; 6]> {
            let s = self.state_at(p);
            if s.starts_with("minecraft:oak_slab[type=bottom") {
                return vec![[0.0, 0.0, 0.0, 1.0, 0.5, 1.0]];
            }
            if crate::shapes::is_air(&s) || crate::fluid::is_liquid_block(&s) {
                return Vec::new();
            }
            vec![[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]]
        }
    }

    impl Blocks for World {
        fn state_at(&self, pos: BlockPos) -> String {
            if let Some(s) = self.blocks.lock().expect("w").get(&pos) {
                return s.clone();
            }
            if pos.1 <= 63 {
                "minecraft:grass_block[snowy=false]".into()
            } else {
                "minecraft:air".into()
            }
        }
        fn set_block(&self, pos: BlockPos, state: &str) -> Option<i32> {
            if self.state_at(pos) == state {
                return None;
            }
            self.blocks
                .lock()
                .expect("w")
                .insert(pos, state.to_string());
            Some(1)
        }
        fn default_state(&self, block: &str) -> Option<String> {
            Some(block.to_string())
        }
        fn has_state(&self, _: &str) -> bool {
            true
        }
        fn state_id(&self, _: &str) -> Option<i32> {
            Some(1)
        }
        fn replaceable(&self, pos: BlockPos, _: &crate::placement::Replacing<'_>) -> bool {
            crate::shapes::is_air(&self.state_at(pos))
        }
        fn face_sturdy(&self, pos: BlockPos, _: Face) -> bool {
            !self.boxes(pos).is_empty()
        }
        fn redstone_conductor(&self, _: BlockPos) -> bool {
            false
        }
        fn signal_source(&self, _: BlockPos) -> bool {
            false
        }
        fn water_at(&self, pos: BlockPos) -> bool {
            self.fluid_at(pos).kind.is_water()
        }
        fn legacy_fluid_at(&self, pos: BlockPos) -> String {
            self.fluid_at(pos).legacy_block()
        }
        fn water_evaporates(&self) -> bool {
            self.nether
        }
    }

    /// `canBeReplaced()` and `isSolid()` as the props table answers them for
    /// the states these tests hold: liquids and air replaceable, the full
    /// cubes and the slab solid.
    fn facts() -> (impl Fn(&str) -> bool, impl Fn(&str) -> bool) {
        (
            |s: &str| crate::shapes::is_air(s) || crate::fluid::is_liquid_block(s),
            |s: &str| !crate::shapes::is_air(s) && !crate::fluid::is_liquid_block(s),
        )
    }

    /// From the feet at (0, 64, 0), aim at a point and use the bucket.
    fn use_at(world: &World, content: Kind, target: [f64; 3], creative: bool) -> Used {
        let eye = [0.0, 64.0 + EYE_HEIGHT, 0.0];
        let d = [target[0] - eye[0], target[1] - eye[1], target[2] - eye[2]];
        #[expect(clippy::cast_possible_truncation, reason = "angles in degrees")]
        let yaw = (-d[0]).atan2(d[2]).to_degrees() as f32;
        #[expect(clippy::cast_possible_truncation, reason = "angles in degrees")]
        let pitch = (-d[1]).atan2(d[0].hypot(d[2])).to_degrees() as f32;
        let (replaceable, solid) = facts();
        let facts = StateFacts {
            replaceable: &replaceable,
            solid: &solid,
        };
        let boxes = |p: BlockPos| world.boxes(p);
        use_bucket(
            world,
            None,
            &facts,
            &boxes,
            content,
            eye,
            (pitch, yaw),
            REACH + CREATIVE_REACH_BONUS,
            false,
            creative,
        )
    }

    #[test]
    fn a_water_bucket_empties_into_a_hole_and_keeps_its_stack_in_creative() {
        let world = World::with(&[((1, 63, 0), "minecraft:air")]);
        let used = use_at(&world, Kind::Water, [1.8, 63.0, 0.5], true);
        assert_eq!(world.state_at((1, 63, 0)), "minecraft:water[level=0]");
        assert_eq!(
            used.effects
                .sounds
                .iter()
                .map(|s| s.event.as_str())
                .collect::<Vec<_>>(),
            vec!["minecraft:item.bucket.empty"]
        );
        assert_eq!(used.effects.sounds[0].source, crate::sounds::SOURCE_BLOCKS);
        assert_eq!((used.held_after, used.creative_result), (None, None));
        // `LiquidBlock.onPlace` booked the first fluid tick inside the write.
        assert!(used
            .effects
            .fluid_ticks
            .iter()
            .any(|t| t.pos == (1, 63, 0) && t.block == crate::fluid::WATER));
    }

    #[test]
    fn a_survival_hand_ends_holding_the_other_bucket() {
        let world = World::with(&[((1, 63, 0), "minecraft:air")]);
        let used = use_at(&world, Kind::Lava, [1.8, 63.0, 0.5], false);
        assert_eq!(world.state_at((1, 63, 0)), "minecraft:lava[level=0]");
        assert_eq!(used.held_after, Some("minecraft:bucket"));
        assert_eq!(
            used.effects.sounds[0].event,
            "minecraft:item.bucket.empty_lava"
        );
        let back = use_at(&world, Kind::Empty, [1.8, 63.0, 0.5], false);
        assert_eq!(world.state_at((1, 63, 0)), "minecraft:air");
        assert_eq!(back.held_after, Some("minecraft:lava_bucket"));
        assert_eq!(back.fill_sound, Some("minecraft:item.bucket.fill_lava"));
    }

    /// The full bucket's ray is `Fluid.NONE` and passes through a source to
    /// the floor; the empty bucket's is `SOURCE_ONLY` and stops on it. Both
    /// land on the same cell here, and only one of them changes it.
    #[test]
    fn an_empty_bucket_stops_on_the_water_and_a_full_one_reaches_the_floor() {
        let world = World::with(&[((1, 63, 0), "minecraft:water[level=0]")]);
        let full = use_at(&world, Kind::Water, [1.8, 63.0, 0.5], true);
        assert!(
            full.effects.blocks.is_empty(),
            "a source replaced by itself writes nothing"
        );
        assert_eq!(
            full.effects.sounds.len(),
            1,
            "and still plays the sound: setBlock false, but the old state is a source"
        );
        let empty = use_at(&world, Kind::Empty, [1.8, 63.0, 0.5], true);
        assert_eq!(world.state_at((1, 63, 0)), "minecraft:air");
        assert_eq!(empty.creative_result, Some("minecraft:water_bucket"));
    }

    #[test]
    fn a_water_bucket_waterlogs_a_slab_in_place() {
        let world = World::with(&[(
            (1, 63, 0),
            "minecraft:oak_slab[type=bottom,waterlogged=false]",
        )]);
        let used = use_at(&world, Kind::Water, [1.8, 63.5, 0.5], true);
        assert_eq!(
            world.state_at((1, 63, 0)),
            "minecraft:oak_slab[type=bottom,waterlogged=true]"
        );
        assert_eq!(
            world.state_at((1, 64, 0)),
            "minecraft:air",
            "not the cell above"
        );
        assert_eq!(used.effects.sounds.len(), 1);
        // Lava does not waterlog: the target is the face's neighbour.
        let world = World::with(&[(
            (1, 63, 0),
            "minecraft:oak_slab[type=bottom,waterlogged=false]",
        )]);
        use_at(&world, Kind::Lava, [1.8, 63.5, 0.5], true);
        assert_eq!(world.state_at((1, 64, 0)), "minecraft:lava[level=0]");
    }

    #[test]
    fn water_evaporates_in_the_nether_with_a_fizz_and_no_block() {
        let world = World {
            nether: true,
            ..World::with(&[((1, 63, 0), "minecraft:air")])
        };
        let used = use_at(&world, Kind::Water, [1.8, 63.0, 0.5], false);
        assert_eq!(world.state_at((1, 63, 0)), "minecraft:air");
        assert_eq!(
            used.effects.sounds[0].event,
            "minecraft:block.fire.extinguish"
        );
        assert_eq!(
            used.held_after,
            Some("minecraft:bucket"),
            "it still succeeded"
        );
        // Lava does not evaporate.
        let used = use_at(&world, Kind::Lava, [1.8, 63.0, 0.5], false);
        assert_eq!(world.state_at((1, 63, 0)), "minecraft:lava[level=0]");
        assert_eq!(
            used.effects.sounds[0].event,
            "minecraft:item.bucket.empty_lava"
        );
    }

    #[test]
    fn a_ray_that_meets_nothing_does_nothing() {
        let world = World::default();
        let used = use_at(&world, Kind::Water, [0.0, 80.0, 3.0], false);
        assert!(used.effects.blocks.is_empty() && used.effects.sounds.is_empty());
        assert_eq!(used.held_after, None);
    }
}
