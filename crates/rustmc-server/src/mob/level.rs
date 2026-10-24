//! The `ServerLevel` a mob stands in: `PathLevel` for the AI, the block and
//! collision reads the tick makes, and `Level.clip` for line of sight.
//!
//! Reference (26.2): `BlockGetter.traverseBlocks`, `BlockGetter.clip`,
//! `VoxelShape.clip`, `AABB.clip(Iterable, Vec3, Vec3, BlockPos)`,
//! `AABB.getDirection` / `clipPoint`, `LivingEntity.hasLineOfSight`,
//! `CollisionGetter.noCollision`, `Level.getFluidState`,
//! `ServerLevel.updateSkyBrightness`.
//!
//! # What the level answers from, and the two things it cannot
//!
//! Blocks come from [`crate::world::World`] as state strings, resolved to
//! global state ids and then to [`crate::path_facts::PathFacts`] rows and
//! [`crate::collide::CollisionShapes`] grids. Fluids are the same rows.
//!
//! **There is no light engine in this server**, and two `Level` reads are
//! light reads: `canSeeSky(pos)` is `getBrightness(SKY, pos) >= 15`, and
//! `getPathfindingCostFromLightLevels` is a brightness table lookup. Both
//! are answered here from the `WORLD_SURFACE` heightmap — sky-exposed above
//! it, dark below — which agrees with the light engine everywhere except
//! under a block of zero opacity (glass, leaves let 14 through, not 15).
//! Neither mob in this lane reads either on the corpus's superflat, and the
//! gap is named here rather than in the host so the first caller finds it.
//!
//! `noCollision`'s entity limb — boats and shulkers, the only entities that
//! `canBeCollidedWith` — is absent because no such entity exists here.
use crate::collide::{self, Axis, CollisionBlocks, CollisionShapes, Shape};
use crate::movement_props::MovementProps;
use crate::path_facts::PathFacts;
use crate::tiles::Tiles;
use rustmc_data::BlockPos;
use rustmc_entity::host::{BlockFacts, PathLevel};
use rustmc_entity::phys::{Aabb, Vec3};

/// The level as one mob sees it.
#[derive(Clone, Copy)]
pub struct MobLevel {
    /// The tiled world; every block read goes through its `Blocks` impl
    /// and every heightmap, biome or bound read through the tile that
    /// owns the position.
    pub world: &'static Tiles,
    pub shapes: &'static CollisionShapes,
    pub facts: &'static PathFacts,
    pub props: &'static MovementProps,
    /// `getGameTime()`, set by the tick driver before each mob ticks.
    pub game_time: i64,
    /// `getDayTime()`.
    pub day_time: i64,
    air_state: u32,
    min_y: i32,
    max_y: i32,
}

impl MobLevel {
    /// The level over the shared tables, or `None` when an extract is
    /// absent — in which case no mob can be spawned, which is the honest
    /// answer to a missing table.
    pub fn new(world: &'static Tiles) -> Option<Self> {
        let shapes = CollisionShapes::shared()?;
        let facts = PathFacts::shared()?;
        let props = MovementProps::shared()?;
        let air_state =
            u32::try_from(crate::edits::Blocks::state_id(world, "minecraft:air")?).ok()?;
        let spawn = world.spawn_area();
        let tile = world.tile(spawn.min_chunk_x, spawn.min_chunk_z)?;
        let (min_y, max_y) = (tile.min_y(), tile.max_y());
        Some(Self {
            world,
            shapes,
            facts,
            props,
            game_time: 0,
            day_time: 0,
            air_state,
            min_y,
            max_y,
        })
    }

    /// `getBiome(pos)`'s registry id, from the tile that owns the column;
    /// `minecraft:plains` for a position in no tile, which no spawn
    /// reaches (an egg needs a block to click).
    #[must_use]
    pub fn biome_at(&self, pos: BlockPos) -> String {
        self.world.tile(pos.x >> 4, pos.z >> 4).map_or_else(
            || "minecraft:plains".to_string(),
            |w| w.biome_at(pos.x, pos.y, pos.z),
        )
    }

    /// `WORLD_SURFACE` at a column, or the dimension's bottom in no tile.
    #[must_use]
    pub fn world_surface_at(&self, x: i32, z: i32) -> i32 {
        self.world
            .tile(x >> 4, z >> 4)
            .map_or(self.min_y, |w| w.world_surface_at(x, z))
    }

    /// The global state id at a position; air outside the world.
    #[must_use]
    pub fn state_id(&self, pos: BlockPos) -> u32 {
        let s = crate::edits::Blocks::state_at(self.world, (pos.x, pos.y, pos.z));
        crate::edits::Blocks::state_id(self.world, &s)
            .and_then(|i| u32::try_from(i).ok())
            .unwrap_or(self.air_state)
    }

    /// The state string at a position.
    #[must_use]
    pub fn state_at(&self, pos: BlockPos) -> String {
        crate::edits::Blocks::state_at(self.world, (pos.x, pos.y, pos.z))
    }

    /// The `path_facts` row at a position.
    #[must_use]
    pub fn row(&self, pos: BlockPos) -> Option<&'static crate::path_facts::Row> {
        self.facts.row(self.state_id(pos))
    }

    /// `getBlockState(pos).getBlock()`'s registry id.
    #[must_use]
    pub fn block_name(&self, pos: BlockPos) -> Option<&'static str> {
        self.facts.block_name(self.state_id(pos))
    }

    /// The collision shape at a position, `None` when the table has none
    /// (a dynamic-shape block) — see [`collide::NoShapeFor`].
    #[must_use]
    pub fn shape(&self, pos: BlockPos) -> Option<&'static Shape> {
        self.shapes.get(self.state_id(pos))
    }

    /// `ServerLevel.skyDarken`: `updateSkyBrightness` is
    /// `(int)(15.0F - environmentAttributes.getDimensionValue(SKY_LIGHT_LEVEL))`
    /// in 26.2, and the overworld's value of that attribute is its default
    /// `15.0F` through `timeline/day.json`'s `multiply` track, sampled at
    /// the overworld clock's total ticks — [`super::SkyLightTrack::sample`].
    /// `0` when the track is not loaded, which is noon.
    #[must_use]
    pub fn sky_darken(&self) -> i32 {
        super::mob_variants().map_or(0, |t| sky_darken_from(&t.sky_light_level, self.day_time))
    }
}

/// `(int)(15.0F - 15.0F * track.sample(dayTime))`: `AttributeTrackSampler.applyTimeBased`
/// with a `multiply` modifier over the attribute's default.
#[must_use]
pub fn sky_darken_from(track: &super::SkyLightTrack, day_time: i64) -> i32 {
    let v = 15.0_f32 * track.sample(day_time);
    #[expect(clippy::cast_possible_truncation, reason = "vanilla's own f2i")]
    {
        (15.0_f32 - v) as i32
    }
}

/// `Level.getTimeOfDay(float partialTick)` at partial tick 1:
/// `fractionalPart(dayTime / 24000 - 0.25)` bent through the same cosine
/// vanilla uses — `DimensionType.timeOfDay`:
///
/// ```text
///   d = frac(dayTime / 24000.0 - 0.25)
///   e = 0.5 - cos(d * π) / 2
///   (float) (d * 2 + e) / 3
/// ```
#[must_use]
pub fn time_of_day(day_time: i64) -> f32 {
    #[expect(clippy::cast_precision_loss, reason = "vanilla's own l2d")]
    let d = day_time as f64 / 24000.0 - 0.25;
    let d = d - d.floor();
    let e = 0.5 - (d * std::f64::consts::PI).cos() / 2.0;
    #[expect(clippy::cast_possible_truncation, reason = "vanilla's own d2f")]
    {
        ((d * 2.0 + e) / 3.0) as f32
    }
}

impl CollisionBlocks for MobLevel {
    fn state_id_at(&self, pos: BlockPos) -> u32 {
        self.state_id(pos)
    }
}

fn to_collide_aabb(a: &Aabb) -> collide::Aabb {
    collide::Aabb {
        min_x: a.min_x,
        min_y: a.min_y,
        min_z: a.min_z,
        max_x: a.max_x,
        max_y: a.max_y,
        max_z: a.max_z,
    }
}

impl PathLevel for MobLevel {
    fn block_facts(&self, pos: BlockPos) -> BlockFacts {
        let id = self.state_id(pos);
        let Some(mut f) = self.facts.block_facts(id) else {
            return BlockFacts::AIR;
        };
        // The two block-entity facts the table cannot carry.
        if f.block_id == Some("furnace") {
            let s = self.state_at(pos);
            f.furnace_lit = Some(s.contains("lit=true"));
        }
        if f.block_id == Some("chest") {
            f.chest_open_count =
                Some(crate::containers::Openers::shared().count((pos.x, pos.y, pos.z)));
        }
        f
    }

    fn min_y(&self) -> i32 {
        self.min_y
    }

    fn max_y(&self) -> i32 {
        self.max_y
    }

    fn collision_shape_max_y(&self, pos: BlockPos) -> Option<f64> {
        let shape = self.shape(pos)?;
        if shape.is_empty() {
            return None;
        }
        // `VoxelShape.max(Axis.Y)`: the last coordinate of the last occupied
        // cell along y — `getCoords(axis).getDouble(lastFull(axis))`.
        let ys = shape.coords(Axis::Y);
        let (sx, sy, sz) = (
            shape.size(Axis::X),
            shape.size(Axis::Y),
            shape.size(Axis::Z),
        );
        let mut last = None;
        for y in (0..sy).rev() {
            let any = (0..sx).any(|x| {
                (0..sz).any(|z| {
                    shape.is_full_wide(
                        i64::try_from(x).unwrap_or(0),
                        i64::try_from(y).unwrap_or(0),
                        i64::try_from(z).unwrap_or(0),
                    )
                })
            });
            if any {
                last = Some(y);
                break;
            }
        }
        last.map(|y| ys[y + 1])
    }

    fn game_time(&self) -> i64 {
        self.game_time
    }

    fn is_bright_outside(&self) -> bool {
        // `!dimensionType().hasFixedTime() && skyDarken < 4`; the overworld
        // has no fixed time.
        self.sky_darken() < 4
    }

    fn can_see_sky(&self, pos: BlockPos) -> bool {
        // NOT `getBrightness(SKY, pos) >= 15`: no light engine — see the
        // module docs for where the heightmap answer differs.
        pos.y >= self.world_surface_at(pos.x, pos.z)
    }

    fn is_chunk_loaded(&self, chunk_x: i32, chunk_z: i32) -> bool {
        // `getChunkNow(cx, cz) != null`: a generated tile owns it.
        self.world
            .tile(chunk_x, chunk_z)
            .is_some_and(|w| w.area().contains(chunk_x, chunk_z))
    }

    fn no_collision(&self, _mob_id: i32, aabb: &Aabb) -> bool {
        // A block whose shape the table cannot answer is treated as
        // colliding: the alternative admits a mob into a shulker box.
        match collide::collect_block_collisions(self.shapes, self, to_collide_aabb(aabb)) {
            Ok(v) => v.is_empty(),
            Err(_) => false,
        }
    }

    fn clip_misses(&self, _mob_id: i32, from: Vec3, to: Vec3, fluids: bool) -> bool {
        clip_misses(self, from, to, fluids)
    }
}

/// `Level.getPathfindingCostFromLightLevels(pos)`:
/// `getLightLevelDependentMagicValue(pos) - 0.5F`, where the magic value is
/// the dimension's brightness table at `getMaxLocalRawBrightness(pos)` —
/// `max(sky - skyDarken, block)` — and the overworld's table is
/// `(1 - ambientLight) * f / (4 - 3f) + ambientLight` with `f = i / 15`
/// and `ambientLight = 0`, so brightness 15 → 1.0 → cost 0.5.
///
/// **No light engine**: a sky-exposed position (above `WORLD_SURFACE`) is
/// charged at sky light 15 less `skyDarken`; a covered one at light 0.
/// `Animal.getWalkTargetValue` reaches this only for a non-grass floor,
/// which the corpus's superflat never presents.
#[must_use]
pub fn pathfinding_cost_from_light_levels(level: &MobLevel, pos: BlockPos) -> f32 {
    let raw = if level.can_see_sky(pos) {
        (15 - level.sky_darken()).max(0)
    } else {
        0
    };
    #[expect(clippy::cast_precision_loss, reason = "0..=15")]
    let f = raw as f32 / 15.0;
    let magic = f / (4.0 - 3.0 * f);
    magic - 0.5
}

/// `Mth.lerp(double, double, double)`: `start + delta * (end - start)`.
fn lerp(delta: f64, start: f64, end: f64) -> f64 {
    start + delta * (end - start)
}

/// `Mth.frac(double)`: `d - lfloor(d)`.
fn frac(d: f64) -> f64 {
    d - d.floor()
}

/// `Mth.sign(double)`.
fn sign(d: f64) -> i32 {
    if d == 0.0 {
        0
    } else if d > 0.0 {
        1
    } else {
        -1
    }
}

/// `Mth.floor(double)`.
fn floor(d: f64) -> i32 {
    #[expect(clippy::cast_possible_truncation, reason = "a block coordinate")]
    {
        d.floor() as i32
    }
}

/// `level.clip(new ClipContext(from, to, COLLIDER, fluids ? ANY : NONE,
/// mob)).getType() == MISS`.
///
/// `BlockGetter.clip` walks `traverseBlocks` with a tester that clips the
/// block's collision shape (and, under `Fluid.ANY`, the fluid's shape) and
/// returns the first hit; only whether *any* block is hit matters here, so
/// the hit's location and face are not computed. `clipWithInteractionOverride`
/// changes a hit's face and never its existence.
///
/// **A block the shape table cannot answer blocks the ray.** The 23
/// dynamic-shape blocks (shulker boxes, powder snow, bamboo, dripstone,
/// scaffolding, moving pistons, sulfur spikes) have no state-keyed grid;
/// most of them are opaque to a ray, powder snow is not, and neither guess
/// is the bytecode. Refused as a hit rather than a miss so a mob never sees
/// through a shulker box.
pub fn clip_misses(level: &MobLevel, from: Vec3, to: Vec3, fluids: bool) -> bool {
    traverse_blocks(from, to, |pos| {
        block_hit(level, from, to, pos, fluids).then_some(())
    })
    .is_none()
}

/// `BlockGetter.traverseBlocks(from, to, ctx, tester, miss)`, offsets
/// 0-491: the blocks the segment passes through, in order, until `tester`
/// answers; `None` is the miss.
fn traverse_blocks<T>(
    from: Vec3,
    to: Vec3,
    mut tester: impl FnMut(BlockPos) -> Option<T>,
) -> Option<T> {
    if from == to {
        return None;
    }
    let d0 = lerp(-1.0E-7, to.x, from.x);
    let d1 = lerp(-1.0E-7, to.y, from.y);
    let d2 = lerp(-1.0E-7, to.z, from.z);
    let d3 = lerp(-1.0E-7, from.x, to.x);
    let d4 = lerp(-1.0E-7, from.y, to.y);
    let d5 = lerp(-1.0E-7, from.z, to.z);
    let mut i = floor(d3);
    let mut j = floor(d4);
    let mut k = floor(d5);
    if let Some(t) = tester(BlockPos { x: i, y: j, z: k }) {
        return Some(t);
    }
    let d6 = d0 - d3;
    let d7 = d1 - d4;
    let d8 = d2 - d5;
    let l = sign(d6);
    let i1 = sign(d7);
    let j1 = sign(d8);
    let d9 = if l == 0 { f64::MAX } else { f64::from(l) / d6 };
    let d10 = if i1 == 0 {
        f64::MAX
    } else {
        f64::from(i1) / d7
    };
    let d11 = if j1 == 0 {
        f64::MAX
    } else {
        f64::from(j1) / d8
    };
    let mut d12 = d9 * if l > 0 { 1.0 - frac(d3) } else { frac(d3) };
    let mut d13 = d10 * if i1 > 0 { 1.0 - frac(d4) } else { frac(d4) };
    let mut d14 = d11 * if j1 > 0 { 1.0 - frac(d5) } else { frac(d5) };
    while d12 <= 1.0 || d13 <= 1.0 || d14 <= 1.0 {
        if d12 < d13 {
            if d12 < d14 {
                i += l;
                d12 += d9;
            } else {
                k += j1;
                d14 += d11;
            }
        } else if d13 < d14 {
            j += i1;
            d13 += d10;
        } else {
            k += j1;
            d14 += d11;
        }
        if let Some(t) = tester(BlockPos { x: i, y: j, z: k }) {
            return Some(t);
        }
    }
    None
}

/// `Level.clip(new ClipContext(from, to, COLLIDER, NONE, entity))` →
/// `getLocation()` of the `BlockHitResult`: the first block along the
/// segment whose collision shape the segment enters, and where. `None` is
/// `MISS`.
///
/// `VoxelShape.clip(from, to, pos)`: an empty shape or a segment shorter than
/// `sqrt(1e-7)` misses; a start point (`from + d * 0.001`) already inside a
/// full cell hits AT that start point; otherwise `AABB.clip(toAabbs(), from,
/// to, pos)` — the least parameter over every box's entry face, `from + d *
/// t`. The fluid half of `BlockGetter.clip` is `Fluid.NONE`'s empty shape.
pub fn clip_location(level: &MobLevel, from: Vec3, to: Vec3) -> Option<Vec3> {
    traverse_blocks(from, to, |pos| {
        let id = level.state_id(pos);
        let shape = level.shapes.get(id)?;
        shape_clip_location(shape, from, to, pos)
    })
}

fn shape_clip_location(shape: &Shape, from: Vec3, to: Vec3, pos: BlockPos) -> Option<Vec3> {
    if shape.is_empty() {
        return None;
    }
    let d = to.subtract(from);
    if d.length_sqr() < 1.0E-7 {
        return None;
    }
    let start = from + d.scale(0.001);
    let ix = find_index(shape, Axis::X, start.x - f64::from(pos.x));
    let iy = find_index(shape, Axis::Y, start.y - f64::from(pos.y));
    let iz = find_index(shape, Axis::Z, start.z - f64::from(pos.z));
    if shape.is_full_wide(ix, iy, iz) {
        return Some(start);
    }
    let mut min = [1.0_f64];
    let (sx, sy, sz) = (
        shape.size(Axis::X),
        shape.size(Axis::Y),
        shape.size(Axis::Z),
    );
    let (cx, cy, cz) = (
        shape.coords(Axis::X),
        shape.coords(Axis::Y),
        shape.coords(Axis::Z),
    );
    let mut hit = false;
    for x in 0..sx {
        for y in 0..sy {
            for z in 0..sz {
                if !shape.is_full_wide(
                    i64::try_from(x).unwrap_or(0),
                    i64::try_from(y).unwrap_or(0),
                    i64::try_from(z).unwrap_or(0),
                ) {
                    continue;
                }
                hit |= aabb_clips(
                    (cx[x], cy[y], cz[z], cx[x + 1], cy[y + 1], cz[z + 1]),
                    from,
                    to,
                    pos,
                    &mut min,
                );
            }
        }
    }
    hit.then(|| from + d.scale(min[0]))
}

/// The tester `BlockGetter.clip` hands `traverseBlocks`, reduced to "did
/// this block's collision shape (or, under `ANY`, its fluid) stop the ray".
fn block_hit(level: &MobLevel, from: Vec3, to: Vec3, pos: BlockPos, fluids: bool) -> bool {
    let id = level.state_id(pos);
    let Some(shape) = level.shapes.get(id) else {
        return true;
    };
    if shape_clips(shape, from, to, pos) {
        return true;
    }
    if fluids {
        // `ClipContext.Fluid.ANY.getFluidShape`: `fluid.getShape(level, pos)`
        // — a box `height` tall, empty when the fluid is. `FluidState.getShape`
        // is `Shapes.box(0, 0, 0, 1, getHeight(level, pos), 1)`, the
        // full-block shape when the height is 1.
        if let Some(row) = level.facts.row(id) {
            if row.fluid_kind != 0 {
                let above = level.facts.row(level.state_id(BlockPos {
                    x: pos.x,
                    y: pos.y + 1,
                    z: pos.z,
                }));
                let same_above = above.is_some_and(|a| a.fluid_kind == row.fluid_kind);
                let h = if same_above {
                    1.0
                } else {
                    f64::from(row.fluid_own_height)
                };
                return aabb_clips((0.0, 0.0, 0.0, 1.0, h, 1.0), from, to, pos, &mut [1.0]);
            }
        }
    }
    false
}

/// `VoxelShape.clip(from, to, pos) != null`, offsets 0-151.
fn shape_clips(shape: &Shape, from: Vec3, to: Vec3, pos: BlockPos) -> bool {
    if shape.is_empty() {
        return false;
    }
    let d = to.subtract(from);
    if d.length_sqr() < 1.0E-7 {
        return false;
    }
    let start = from + d.scale(0.001);
    let ix = find_index(shape, Axis::X, start.x - f64::from(pos.x));
    let iy = find_index(shape, Axis::Y, start.y - f64::from(pos.y));
    let iz = find_index(shape, Axis::Z, start.z - f64::from(pos.z));
    if shape.is_full_wide(ix, iy, iz) {
        return true;
    }
    // `AABB.clip(this.toAabbs(), from, to, pos)`: every box in turn, sharing
    // one `minDistance`. Only existence is asked here, and every occupied
    // cell is a box of the union `toAabbs` merges, so the cells are walked
    // directly — the same set of faces, in x, y, z order.
    let mut min = [1.0_f64];
    let (sx, sy, sz) = (
        shape.size(Axis::X),
        shape.size(Axis::Y),
        shape.size(Axis::Z),
    );
    let (cx, cy, cz) = (
        shape.coords(Axis::X),
        shape.coords(Axis::Y),
        shape.coords(Axis::Z),
    );
    let mut hit = false;
    for x in 0..sx {
        for y in 0..sy {
            for z in 0..sz {
                if !shape.is_full_wide(
                    i64::try_from(x).unwrap_or(0),
                    i64::try_from(y).unwrap_or(0),
                    i64::try_from(z).unwrap_or(0),
                ) {
                    continue;
                }
                if aabb_clips(
                    (cx[x], cy[y], cz[z], cx[x + 1], cy[y + 1], cz[z + 1]),
                    from,
                    to,
                    pos,
                    &mut min,
                ) {
                    hit = true;
                }
            }
        }
    }
    hit
}

/// `VoxelShape.findIndex(axis, d)`:
/// `Mth.binarySearch(0, size + 1, i -> d < coords[i]) - 1`.
fn find_index(shape: &Shape, axis: Axis, d: f64) -> i64 {
    let coords = shape.coords(axis);
    let n = shape.size(axis) + 1;
    // `Mth.binarySearch(min, max, pred)`: the first index in `[min, max)`
    // satisfying `pred`, or `max`.
    let mut lo = 0usize;
    let mut hi = n;
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if d < coords[mid] {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    i64::try_from(lo).unwrap_or(0) - 1
}

/// One box of `AABB.clip(Iterable, from, to, pos)`: `getDirection(box.move(pos),
/// from, minDistance, dir, dx, dy, dz) != null`, i.e. one of the six
/// `clipPoint` tests passed.
fn aabb_clips(
    b: (f64, f64, f64, f64, f64, f64),
    from: Vec3,
    to: Vec3,
    pos: BlockPos,
    min: &mut [f64; 1],
) -> bool {
    let (min_x, min_y, min_z, max_x, max_y, max_z) = (
        b.0 + f64::from(pos.x),
        b.1 + f64::from(pos.y),
        b.2 + f64::from(pos.z),
        b.3 + f64::from(pos.x),
        b.4 + f64::from(pos.y),
        b.5 + f64::from(pos.z),
    );
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let dz = to.z - from.z;
    let mut hit = false;
    // `getDirection(minX, minY, minZ, maxX, maxY, maxZ, from, dists, dir, dx, dy, dz)`.
    if dx > 1.0E-7 {
        hit |= clip_point(
            min, dx, dy, dz, min_x, min_y, max_y, min_z, max_z, from.x, from.y, from.z,
        );
    } else if dx < -1.0E-7 {
        hit |= clip_point(
            min, dx, dy, dz, max_x, min_y, max_y, min_z, max_z, from.x, from.y, from.z,
        );
    }
    if dy > 1.0E-7 {
        hit |= clip_point(
            min, dy, dz, dx, min_y, min_z, max_z, min_x, max_x, from.y, from.z, from.x,
        );
    } else if dy < -1.0E-7 {
        hit |= clip_point(
            min, dy, dz, dx, max_y, min_z, max_z, min_x, max_x, from.y, from.z, from.x,
        );
    }
    if dz > 1.0E-7 {
        hit |= clip_point(
            min, dz, dx, dy, min_z, min_x, max_x, min_y, max_y, from.z, from.x, from.y,
        );
    } else if dz < -1.0E-7 {
        hit |= clip_point(
            min, dz, dx, dy, max_z, min_x, max_x, min_y, max_y, from.z, from.x, from.y,
        );
    }
    hit
}

/// `AABB.clipPoint(dists, dir, d, dA, dB, plane, minA, maxA, minB, maxB,
/// result, start, startA, startB)`, offsets 0-102: the slab test on one
/// face, tightening `dists[0]` when it passes.
#[expect(
    clippy::too_many_arguments,
    reason = "vanilla's own fourteen-argument static"
)]
fn clip_point(
    dists: &mut [f64; 1],
    d: f64,
    da: f64,
    db: f64,
    plane: f64,
    min_a: f64,
    max_a: f64,
    min_b: f64,
    max_b: f64,
    start: f64,
    start_a: f64,
    start_b: f64,
) -> bool {
    let t = (plane - start) / d;
    let pa = start_a + t * da;
    let pb = start_b + t * db;
    if 0.0 < t
        && t < dists[0]
        && min_a - 1.0E-7 < pa
        && pa < max_a + 1.0E-7
        && min_b - 1.0E-7 < pb
        && pb < max_b + 1.0E-7
    {
        dists[0] = t;
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_time_of_day_is_noon_at_6000_and_midnight_at_18000() {
        // `DimensionType.timeOfDay`: 0 is noon (the celestial angle), 0.5
        // midnight; dawn (0) bends forward through the cosine.
        assert!((time_of_day(6000) - 0.0).abs() < 1e-6);
        assert!((time_of_day(18000) - 0.5).abs() < 1e-6);
        let dawn = time_of_day(0);
        assert!(dawn > 0.75 && dawn < 0.8, "{dawn}");
    }

    #[test]
    fn find_index_places_a_point_in_its_cell_and_outside_the_grid() {
        // Against the real table: the full-block shape has one cell per axis.
        let Some(shapes) = CollisionShapes::shared() else {
            return;
        };
        let Some(s) = (0..u32::MAX)
            .take(40_000)
            .filter_map(|i| shapes.get(i))
            .find(|s| s.is_full_block())
        else {
            return;
        };
        assert_eq!(find_index(s, Axis::X, -0.1), -1);
        assert_eq!(find_index(s, Axis::X, 0.0), 0);
        assert_eq!(find_index(s, Axis::X, 0.99), 0);
        assert_eq!(find_index(s, Axis::X, 1.0), 1);
    }

    #[test]
    fn a_ray_through_a_full_cell_is_a_hit_and_one_past_it_is_not() {
        let mut min = [1.0];
        let from = Vec3::new(-1.0, 0.5, 0.5);
        let to = Vec3::new(2.0, 0.5, 0.5);
        let pos = BlockPos { x: 0, y: 0, z: 0 };
        assert!(aabb_clips(
            (0.0, 0.0, 0.0, 1.0, 1.0, 1.0),
            from,
            to,
            pos,
            &mut min
        ));
        assert!((min[0] - 1.0 / 3.0).abs() < 1e-12);
        let mut min = [1.0];
        let above = Vec3::new(-1.0, 1.5, 0.5);
        let above_to = Vec3::new(2.0, 1.5, 0.5);
        assert!(!aabb_clips(
            (0.0, 0.0, 0.0, 1.0, 1.0, 1.0),
            above,
            above_to,
            pos,
            &mut min
        ));
    }
}
