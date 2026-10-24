//! `Entity.move`'s geometry: `collide`, `collideBoundingBox`, the three-axis
//! sweep and auto-step, over per-state collision shapes.
//!
//! Every line here is transcribed from `javap -p -c -constants` on the pinned
//! 26.2 server jar. The shapes it sweeps against come from
//! `data/vanilla/collision_shapes.json`, dumped by
//! `tools/goldengen/vanilla/CollisionShapesGolden.java`, and the whole module
//! is gated against vanilla's own numbers by
//! `crates/rustmc-server/tests/entity_move.rs`.
//!
//! # What this is NOT
//!
//! It is not `Entity.move`. `move` is 327 instructions of which the geometry
//! is the first third; the rest is `setPos`, the four collision flags, step
//! sounds, `checkSupportingBlock`, `applyEffectsFromBlocks` and fall damage,
//! and every one of those reaches a subsystem this tree does not have.
//! Neither is it player physics: `aiStep`, `travel`, gravity, drag, friction
//! and the attribute system are all upstream of the delta vector this module
//! is handed, and `LivingEntity.getDefaultGravity()` is an attribute lookup
//! rather than a constant, so faking it is the one-way door `crate::dropitem`
//! already refuses in writing.
//!
//! What is here is the part that has one right answer and can be measured
//! against vanilla producing it: given a bounding box, a delta and a world,
//! where does the entity actually end up.
//!
//! **Nothing calls this yet**, and that is deliberate rather than unfinished.
//! Three corpus checks — 070 sand-falls, 500 dropping-an-item, 540
//! player-resync — bottom out in `Entity.move`, and wiring a geometry engine
//! under them before the physics above it exists would put the checks on
//! *some* motion rather than vanilla's and make three honest `diverge`
//! verdicts look measured.
//!
//! # The three representation findings, because they are counter-intuitive
//!
//! 1. **A box list per state is not enough.** `VoxelShape.collideX` scans the
//!    shape's voxel *grid* starting at the column after the one the box's
//!    leading face sits in, so an entity already inside solid reports the gap
//!    to the next occupied grid line — and which line that is depends on how
//!    the shape was subdivided, not on the solid's outline. Merging the boxes
//!    erases interior lines. Measured at 345,705 disagreements in 16,736,112
//!    probes, all of them with the box already inside the shape.
//! 2. **The candidate step heights come from the grid, not the boxes.**
//!    `collectCandidateStepUpHeights` reads `getCoords(Axis.Y)`. On 65 of the
//!    318 distinct shapes those are not the box corners.
//! 3. **Admission is not strict overlap.** A block joins the collider list
//!    through `joinIsNotEmpty(..., AND)`, whose merged coordinate list treats
//!    values within `1e-7` as one, so an overlap thinner than that does not
//!    exist. Measured at 68,551 disagreements in 1,046,007 probes.
//!
//! All three are recorded in `CollisionShapesGolden.java`'s header with the
//! sweeps that establish them, and the sweeps run on every regeneration.

use std::io;

use rustmc_data::BlockPos;

/// The epsilon `VoxelShape.collideX`, `BlockCollisions` and `IndirectMerger`
/// all use. Written once because a second copy is a second thing to get
/// wrong.
const EPS: f64 = 1.0E-7;

/// `Mth.equal`'s epsilon, which `Entity.collide` expands the step-up probe
/// box downwards by.
///
/// **This is `1.0E-5f` promoted to double, not `1.0E-5`.** The constant pool
/// holds `-9.999999747378752E-6`; `-1e-5_f64` is a different number and would
/// not bit-match. Same class as the `mul_add` trap recorded in `tracker.rs`.
const MTH_EQUAL_EPSILON: f64 = -(1.0E-5_f32 as f64);

/// A point, in vanilla's `Vec3`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    #[must_use]
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    /// `Vec3.get(Axis)`.
    #[must_use]
    pub const fn get(&self, axis: Axis) -> f64 {
        match axis {
            Axis::X => self.x,
            Axis::Y => self.y,
            Axis::Z => self.z,
        }
    }

    /// `Vec3.with(Axis, double)`.
    #[must_use]
    pub const fn with(&self, axis: Axis, v: f64) -> Self {
        match axis {
            Axis::X => Self { x: v, ..*self },
            Axis::Y => Self { y: v, ..*self },
            Axis::Z => Self { z: v, ..*self },
        }
    }

    /// `Vec3.lengthSqr()`.
    #[must_use]
    pub fn length_sqr(&self) -> f64 {
        self.x * self.x + self.y * self.y + self.z * self.z
    }

    /// `Vec3.horizontalDistanceSqr()` — x and z only.
    #[must_use]
    pub fn horizontal_distance_sqr(&self) -> f64 {
        self.x * self.x + self.z * self.z
    }
}

/// `Direction.Axis`, in `values()` order, which is also the index order of
/// the coordinate lists in the extract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    X = 0,
    Y = 1,
    Z = 2,
}

/// `Direction.axisStepOrder(Vec3)`.
///
/// **The larger horizontal magnitude is resolved first, not the smaller.**
/// `abs(x) < abs(z) ? YZX_AXIS_ORDER : YXZ_AXIS_ORDER`, and `YZX` is
/// `ImmutableList.of(Y, Z, X)` — so `|x| < |z|` puts Z second, and Z is the
/// bigger one. Y is always first either way. A port that resolves X, then Z,
/// then Y unconditionally agrees on every axis-aligned move and disagrees
/// diagonally into a corner, which needs a diagonal scenario to show at all.
#[must_use]
pub fn axis_step_order(v: Vec3) -> [Axis; 3] {
    if v.x.abs() < v.z.abs() {
        [Axis::Y, Axis::Z, Axis::X]
    } else {
        [Axis::Y, Axis::X, Axis::Z]
    }
}

/// Vanilla's `AABB`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    pub min_x: f64,
    pub min_y: f64,
    pub min_z: f64,
    pub max_x: f64,
    pub max_y: f64,
    pub max_z: f64,
}

impl Aabb {
    #[must_use]
    pub const fn new(
        min_x: f64,
        min_y: f64,
        min_z: f64,
        max_x: f64,
        max_y: f64,
        max_z: f64,
    ) -> Self {
        Self {
            min_x,
            min_y,
            min_z,
            max_x,
            max_y,
            max_z,
        }
    }

    #[must_use]
    pub const fn min(&self, axis: Axis) -> f64 {
        match axis {
            Axis::X => self.min_x,
            Axis::Y => self.min_y,
            Axis::Z => self.min_z,
        }
    }

    #[must_use]
    pub const fn max(&self, axis: Axis) -> f64 {
        match axis {
            Axis::X => self.max_x,
            Axis::Y => self.max_y,
            Axis::Z => self.max_z,
        }
    }

    /// `AABB.move(double, double, double)`.
    #[must_use]
    pub fn shifted(&self, dx: f64, dy: f64, dz: f64) -> Self {
        Self {
            min_x: self.min_x + dx,
            min_y: self.min_y + dy,
            min_z: self.min_z + dz,
            max_x: self.max_x + dx,
            max_y: self.max_y + dy,
            max_z: self.max_z + dz,
        }
    }

    /// `AABB.inflate(double)`: every face moved out by `d`.
    #[must_use]
    pub fn inflate(&self, d: f64) -> Self {
        Self::new(
            self.min_x - d,
            self.min_y - d,
            self.min_z - d,
            self.max_x + d,
            self.max_y + d,
            self.max_z + d,
        )
    }

    /// `AABB.expandTowards(double, double, double)`: a negative component
    /// moves the min face, a positive one moves the max face, and **zero
    /// moves neither** — the two comparisons are independent branches, not an
    /// if/else.
    #[must_use]
    pub fn expand_towards(&self, dx: f64, dy: f64, dz: f64) -> Self {
        let mut b = *self;
        if dx < 0.0 {
            b.min_x += dx;
        }
        if dx > 0.0 {
            b.max_x += dx;
        }
        if dy < 0.0 {
            b.min_y += dy;
        }
        if dy > 0.0 {
            b.max_y += dy;
        }
        if dz < 0.0 {
            b.min_z += dz;
        }
        if dz > 0.0 {
            b.max_z += dz;
        }
        b
    }

    /// `AABB.intersects(AABB)` — strict on every face, no epsilon.
    ///
    /// ```text
    ///   minX < o.maxX && maxX > o.minX
    ///   && minY < o.maxY && maxY > o.minY
    ///   && minZ < o.maxZ && maxZ > o.minZ
    /// ```
    ///
    /// Touching faces do NOT intersect. This is not the epsilon-margined
    /// admission test [`Shape::overlaps`] makes for the collision sweep, and
    /// the two must not be confused: this one is what
    /// `Projectile.isOutsideOwnerCollisionRange` and `Level.noCollision`'s
    /// box filter use.
    #[must_use]
    pub fn intersects(&self, o: &Aabb) -> bool {
        self.min_x < o.max_x
            && self.max_x > o.min_x
            && self.min_y < o.max_y
            && self.max_y > o.min_y
            && self.min_z < o.max_z
            && self.max_z > o.min_z
    }

    /// `AABB.intersects(double x6)` against a unit cube at `(x, y, z)`.
    /// Strict on both sides, no epsilon — this is the test the
    /// `Shapes.block()` fast path in `BlockCollisions.computeNext` makes, and
    /// it is deliberately *not* the epsilon-margined one [`Shape::overlaps`]
    /// uses for every other shape.
    #[must_use]
    pub fn intersects_cell(&self, x: i32, y: i32, z: i32) -> bool {
        let (x, y, z) = (f64::from(x), f64::from(y), f64::from(z));
        self.min_x < x + 1.0
            && self.max_x > x
            && self.min_y < y + 1.0
            && self.max_y > y
            && self.min_z < z + 1.0
            && self.max_z > z
    }
}

/// One collision shape: three grid coordinate lists and one occupancy bit per
/// cell.
///
/// This is `VoxelShape` reduced to what collision reads. None of vanilla's
/// shape *construction* is here — no `DiscreteVoxelShape`, no `IndexMerger`,
/// no `BooleanOp`, no `optimize` — because the constructed grids are what the
/// extract carries, and 32,366 states collapse to a few hundred of them.
#[derive(Debug, Clone)]
pub struct Shape {
    /// Grid lines per axis, ascending. `coords[a].len() == size[a] + 1` for a
    /// non-empty shape.
    coords: [Vec<f64>; 3],
    size: [usize; 3],
    /// Cell `(x, y, z)` at bit index `(x * ny + y) * nz + z`.
    full: Vec<bool>,
    /// `shape == Shapes.block()` by identity in vanilla, which selects the
    /// cheap `AABB.intersects` admission test.
    is_block: bool,
    /// `BlockStateBase.hasLargeCollisionShape` — any axis reaching outside
    /// `0..1`. Blocks in the one-cell shell around the query box are only
    /// consulted when this is set.
    large: bool,
}

impl Shape {
    /// `Shapes.box(min, max)` for a box that is not on a power-of-two grid —
    /// one cell between the two corners, which is what `Shapes.create` builds
    /// (an `ArrayVoxelShape` with the corners as its coordinates) when
    /// `findBits` finds no resolution. A fluid's `box(0, 0, 0, 1, h, 1)` is
    /// the reader. Not for collision admission: `is_block` is only set for
    /// the unit cube.
    #[must_use]
    pub fn single_box(min: [f64; 3], max: [f64; 3]) -> Self {
        let is_block = min == [0.0; 3] && max == [1.0; 3];
        let large = (0..3).any(|a| min[a] < 0.0 || max[a] > 1.0);
        Self {
            coords: [
                vec![min[0], max[0]],
                vec![min[1], max[1]],
                vec![min[2], max[2]],
            ],
            size: [1, 1, 1],
            full: vec![true],
            is_block,
            large,
        }
    }

    /// `VoxelShape.move(x, y, z)`: every grid line plus the offset — one
    /// `double` addition each, which is `OffsetDoubleList.getDouble`.
    #[must_use]
    pub fn moved(&self, d: [f64; 3]) -> Self {
        let mut out = self.clone();
        for (a, c) in out.coords.iter_mut().enumerate() {
            for v in c.iter_mut() {
                *v += d[a];
            }
        }
        out.large = (0..3).any(|a| {
            out.coords[a].first().is_some_and(|v| *v < 0.0)
                || out.coords[a].last().is_some_and(|v| *v > 1.0)
        });
        out.is_block = false;
        out
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        !self.full.iter().any(|f| *f)
    }

    #[must_use]
    pub const fn is_full_block(&self) -> bool {
        self.is_block
    }

    /// `Block.isShapeFullBlock(shape)` — `!joinIsNotEmpty(block(), shape,
    /// NOT_SAME)`: the shape **is** the unit cube, whatever grid it is
    /// carried on. What `BlockStateBase.isCollisionShapeFullBlock` caches.
    ///
    /// Not [`Self::is_full_block`], which is the identity test
    /// `BlockCollisions` uses to pick its fast path: a cube carried on a
    /// finer grid is full here and not there.
    #[must_use]
    pub fn is_full_cube(&self) -> bool {
        if self.is_block {
            return true;
        }
        if self.full.is_empty() || !self.full.iter().all(|f| *f) {
            return false;
        }
        self.coords.iter().all(|c| {
            c.first().is_some_and(|v| v.abs() < EPS)
                && c.last().is_some_and(|v| (v - 1.0).abs() < EPS)
        })
    }

    #[must_use]
    pub const fn is_large(&self) -> bool {
        self.large
    }

    /// The grid lines on one axis, which is what
    /// `collectCandidateStepUpHeights` reads and **not** the box corners.
    #[must_use]
    pub fn coords(&self, axis: Axis) -> &[f64] {
        &self.coords[axis as usize]
    }

    fn at(&self, x: usize, y: usize, z: usize) -> bool {
        self.full[(x * self.size[1] + y) * self.size[2] + z]
    }

    /// Every occupied grid cell as a box `[x0, y0, z0, x1, y1, z1]` in
    /// block-local coordinates — what a ray is clipped against
    /// (`crate::bucket::clip`). Unmerged: `VoxelShape.toAabbs` merges runs of
    /// cells, and a ray entering from outside meets an exterior face of
    /// either form at the same `t`.
    #[must_use]
    pub fn cell_boxes(&self) -> Vec<[f64; 6]> {
        let mut out = Vec::new();
        for x in 0..self.size[0] {
            for y in 0..self.size[1] {
                for z in 0..self.size[2] {
                    if self.at(x, y, z) {
                        out.push([
                            self.coords[0][x],
                            self.coords[1][y],
                            self.coords[2][z],
                            self.coords[0][x + 1],
                            self.coords[1][y + 1],
                            self.coords[2][z + 1],
                        ]);
                    }
                }
            }
        }
        out
    }

    /// `DiscreteVoxelShape.getSize(axis)`: the number of cells along an axis,
    /// one less than the coordinate list.
    #[must_use]
    pub const fn size(&self, axis: Axis) -> usize {
        self.size[axis as usize]
    }

    /// `DiscreteVoxelShape.isFullWide(x, y, z)`: occupancy, with an index
    /// outside the grid reading `false` rather than panicking — the form
    /// `VoxelShape.clip` asks with a `findIndex` result that can be -1 or
    /// `size`.
    #[must_use]
    pub fn is_full_wide(&self, x: i64, y: i64, z: i64) -> bool {
        let Ok(x) = usize::try_from(x) else {
            return false;
        };
        let Ok(y) = usize::try_from(y) else {
            return false;
        };
        let Ok(z) = usize::try_from(z) else {
            return false;
        };
        if x >= self.size[0] || y >= self.size[1] || z >= self.size[2] {
            return false;
        }
        self.at(x, y, z)
    }

    /// `VoxelShape.findIndex(Axis, double)` against a shape at the origin.
    ///
    /// Public because `VoxelShape.clip` needs it and subtracts the block
    /// position from the query point rather than offsetting the coordinate
    /// list — which is what the bytecode does there, in contrast to the
    /// collision sweep, and the reason the private [`Shape::find_index`]
    /// takes an offset and this does not.
    #[must_use]
    pub fn find_index_at(&self, axis: Axis, v: f64) -> i64 {
        Self::find_index(self.coords(axis), 0.0, v)
    }

    /// Occupancy with out-of-range indices reading false, which is
    /// `DiscreteVoxelShape.isFullWide`.
    fn full_wide(&self, a: usize, i: i64, j: i64, k: i64) -> bool {
        // `collideX` reaches its three axes through `AxisCycle`. Resolved,
        // the triple is a forward rotation: sweeping X indexes (X, Y, Z),
        // sweeping Y indexes (Y, Z, X), sweeping Z indexes (Z, X, Y). The
        // shape sweep in `CollisionShapesGolden` asserts this against
        // vanilla on 16.7M probes over 318 shapes, most of them asymmetric,
        // so a wrong rotation could not survive it.
        let (x, y, z) = match a {
            0 => (i, j, k),
            1 => (k, i, j),
            _ => (j, k, i),
        };
        if x < 0 || y < 0 || z < 0 {
            return false;
        }
        let (x, y, z) = (x as usize, y as usize, z as usize);
        if x >= self.size[0] || y >= self.size[1] || z >= self.size[2] {
            return false;
        }
        self.at(x, y, z)
    }

    /// `VoxelShape.findIndex(Axis, double)`: the last index whose coordinate
    /// is `<= v`, and `-1` when `v` precedes the list. Vanilla writes it as
    /// `Mth.binarySearch(0, size + 1, i -> v < get(axis, i)) - 1`.
    ///
    /// `off` is the block position the shape sits at. It is added to each
    /// coordinate **here, at the point of comparison**, because that is where
    /// vanilla adds it: `VoxelShape.move` wraps the coordinate lists in
    /// `OffsetDoubleList`, whose `getDouble` is `delegate.getDouble(i) +
    /// offset`. Shifting the query box by `-off` instead is the same value in
    /// exact arithmetic and **not the same double**: `(c + o) - h` and
    /// `c - (h - o)` differ by an ulp often enough to matter, and everything
    /// downstream of this is bit-compared.
    fn find_index(coords: &[f64], off: f64, v: f64) -> i64 {
        let mut lo = 0usize;
        let mut hi = coords.len();
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if v < coords[mid] + off {
                hi = mid;
            } else {
                lo = mid + 1;
            }
        }
        lo as i64 - 1
    }

    /// `VoxelShape.collide(Axis, AABB, double)` by way of `collideX`.
    ///
    /// The two `t >= -EPS` / `t <= EPS` guards are transcribed even though
    /// they are unreachable: `i` starts past `findIndex(hi - EPS)`, so
    /// `coords[i] > hi - EPS` and `t > -EPS` always. They are cheap, they are
    /// what the bytecode says, and a later version could move the scan start.
    #[must_use]
    pub fn collide(&self, axis: Axis, box_: Aabb, d: f64, off: [f64; 3]) -> f64 {
        if self.is_empty() {
            return d;
        }
        if d.abs() < EPS {
            return 0.0;
        }
        let a = axis as usize;
        let b = (a + 1) % 3;
        let c = (a + 2) % 3;
        let hi = box_.max(axis);
        let lo = box_.min(axis);
        let axis_b = AXES[b];
        let axis_c = AXES[c];

        let i0 = Self::find_index(&self.coords[a], off[a], lo + EPS);
        let i1 = Self::find_index(&self.coords[a], off[a], hi - EPS);
        let j0 = Self::find_index(&self.coords[b], off[b], box_.min(axis_b) + EPS).max(0);
        let j1 = (Self::find_index(&self.coords[b], off[b], box_.max(axis_b) - EPS) + 1)
            .min(self.size[b] as i64);
        let k0 = Self::find_index(&self.coords[c], off[c], box_.min(axis_c) + EPS).max(0);
        let k1 = (Self::find_index(&self.coords[c], off[c], box_.max(axis_c) - EPS) + 1)
            .min(self.size[c] as i64);

        if d > 0.0 {
            let mut i = i1 + 1;
            while i < self.size[a] as i64 {
                let mut j = j0;
                while j < j1 {
                    let mut k = k0;
                    while k < k1 {
                        if self.full_wide(a, i, j, k) {
                            let t = (self.coords[a][i as usize] + off[a]) - hi;
                            return if t >= -EPS { d.min(t) } else { d };
                        }
                        k += 1;
                    }
                    j += 1;
                }
                i += 1;
            }
        } else if d < 0.0 {
            let mut i = i0 - 1;
            while i >= 0 {
                let mut j = j0;
                while j < j1 {
                    let mut k = k0;
                    while k < k1 {
                        if self.full_wide(a, i, j, k) {
                            let t = (self.coords[a][i as usize + 1] + off[a]) - lo;
                            return if t <= EPS { d.max(t) } else { d };
                        }
                        k += 1;
                    }
                    j += 1;
                }
                i -= 1;
            }
        }
        d
    }

    /// `Shapes.joinIsNotEmpty(Shapes.create(box), this, BooleanOp.AND)` — the
    /// admission test in `BlockCollisions.computeNext`.
    ///
    /// The `- EPS` on each far face is not a fudge. `IndirectMerger` builds
    /// the merged coordinate list that `joinIsNotEmpty` walks and treats two
    /// coordinates within `1e-7` as one, so a cell thinner than that does not
    /// exist. Strict `<` disagrees with vanilla on 68,551 of 1,046,007
    /// probes, always by admitting a block vanilla does not.
    #[must_use]
    pub fn overlaps(&self, box_: Aabb, off: [f64; 3]) -> bool {
        for x in 0..self.size[0] {
            for y in 0..self.size[1] {
                for z in 0..self.size[2] {
                    if !self.at(x, y, z) {
                        continue;
                    }
                    if span_overlaps(
                        self.coords[0][x] + off[0],
                        self.coords[0][x + 1] + off[0],
                        box_.min_x,
                        box_.max_x,
                    ) && span_overlaps(
                        self.coords[1][y] + off[1],
                        self.coords[1][y + 1] + off[1],
                        box_.min_y,
                        box_.max_y,
                    ) && span_overlaps(
                        self.coords[2][z] + off[2],
                        self.coords[2][z + 1] + off[2],
                        box_.min_z,
                        box_.max_z,
                    ) {
                        return true;
                    }
                }
            }
        }
        false
    }
}

const AXES: [Axis; 3] = [Axis::X, Axis::Y, Axis::Z];

/// One axis of the admission test, written once so there is one thing to get
/// wrong instead of six.
///
/// **Not `cell_min < box_max && box_min < cell_max`.** `IndirectMerger`
/// builds the merged coordinate list `joinIsNotEmpty` walks and treats two
/// coordinates within `1e-7` as one, so an overlap thinner than that does not
/// exist rather than being merely small. Measured: plain strict `<` disagrees
/// with vanilla on 68,551 of 1,046,007 probes, always by admitting a block
/// vanilla does not.
fn span_overlaps(cell_min: f64, cell_max: f64, box_min: f64, box_max: f64) -> bool {
    box_min < cell_max - EPS && cell_min < box_max - EPS
}

/// A shape placed at a block position. `VoxelShape.move(Vec3i)` is an offset
/// and nothing else, so the offset is carried rather than the coordinates
/// being rewritten — 318 shapes are shared by 32,366 states and copying one
/// per touched block per tick is the allocation this avoids.
#[derive(Debug, Clone, Copy)]
pub struct PlacedShape<'a> {
    pub shape: &'a Shape,
    pub pos: BlockPos,
}

impl PlacedShape<'_> {
    fn offset(&self) -> [f64; 3] {
        [
            f64::from(self.pos.x),
            f64::from(self.pos.y),
            f64::from(self.pos.z),
        ]
    }

    fn collide(&self, axis: Axis, box_: Aabb, d: f64) -> f64 {
        self.shape.collide(axis, box_, d, self.offset())
    }

    /// The shape's y grid lines in world space, for
    /// `collectCandidateStepUpHeights`. `c + offset` in that order, which is
    /// `OffsetDoubleList.getDouble`.
    fn coords_y(&self) -> impl Iterator<Item = f64> + '_ {
        let dy = f64::from(self.pos.y);
        self.shape.coords(Axis::Y).iter().map(move |c| c + dy)
    }
}

/// The world, as collision reads it: a global block-state id per position.
///
/// **Ids, not state strings.** A single `Entity.move` touches every block in
/// the swept box plus a one-cell shell, three times over in the step-up path;
/// `crate::edits::Blocks::state_at` returns a `String` and allocating one per
/// position per entity per tick is the wrong shape for this even though it is
/// the tree's usual accessor.
pub trait CollisionBlocks {
    /// The global block-state id at a position — `Block.BLOCK_STATE_REGISTRY`
    /// order, the same number the chunk sections store and the same number
    /// `collision_shapes.json` is indexed by.
    ///
    /// **Deliberately not defaulted.** A default returning air would make
    /// every unimplemented world silently frictionless, which is the
    /// "check that cannot fail" shape this corpus has found sixteen of.
    fn state_id_at(&self, pos: BlockPos) -> u32;
}

/// A position whose block has no answer in the table.
///
/// Raised rather than guessed. The 23 blocks with `hasDynamicShape()` —
/// the seventeen shulker boxes, `bamboo`, `moving_piston`,
/// `pointed_dripstone`, `powder_snow`, `scaffolding` and `sulfur_spike` —
/// have a collision shape that is not a function of their state, which is
/// exactly why vanilla gives them no `BlockStateBase.Cache`. Treating them as
/// air would drop an entity through a shulker box; treating them as a full
/// block would stand it on powder snow. Both are wrong and both look like
/// working physics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoShapeFor {
    pub pos: BlockPos,
    pub state_id: u32,
}

impl std::fmt::Display for NoShapeFor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "no collision shape for state id {} at {:?} — the block has a dynamic shape \
             and the state-keyed table has no answer for it",
            self.state_id, self.pos
        )
    }
}

impl std::error::Error for NoShapeFor {}

/// The extract: the distinct shapes, and one shape index per global state id.
#[derive(Debug, Clone)]
pub struct CollisionShapes {
    shapes: Vec<Shape>,
    /// `-1` for a state whose block has a dynamic shape.
    rows: Vec<i32>,
    /// The global state ids of `minecraft:moving_piston`.
    ///
    /// Needed for one line: `BlockCollisions.computeNext` consults an *edge*
    /// cell only for that block. It cannot be inferred from `rows` — every
    /// dynamic-shape block has row `-1` and only one of them is the piston,
    /// so inferring would refuse movement next to a shulker box that vanilla
    /// walks straight past.
    moving_piston: Vec<u32>,
}

/// Where the extract lives, relative to the server's working directory.
///
/// **This literal is load-bearing outside Rust.** `ci/gen-gameplay-tables.sh`
/// derives the set of tables a tree needs by grepping
/// `crates/rustmc-server/src` for `"data/vanilla/<name>.json"`, so a table
/// becomes required by being opened and stops being required by not being
/// opened. Spelling this path any other way -- joining a directory constant
/// to a file name, say -- makes the gate stop regenerating it and it goes
/// stale in every tree at once.
///
/// **No zero-argument `load_default()` here, deliberately** — `docs/
/// PARITY-ROADMAP.md` §34 found one, bare and cwd-relative like
/// `crate::protocol_ids::shared()` was before its own fix, and unused by
/// anything in the tree (`crates/rustmc-server/tests/entity_move.rs` already
/// calls [`CollisionShapes::load`] with its own `CARGO_MANIFEST_DIR`-anchored
/// path, the correctly-parameterized shape). Deleted rather than fixed: a
/// second, unused loader is exactly the trap that produced this fault the
/// first time, and the caller that eventually wires collision into the boot
/// path should resolve its own root the way `entity_move.rs` and
/// `SessionTables::load` already do, not reach for a convenience wrapper
/// that would need this same fix redone.
pub const TABLE_PATH: &str = "data/vanilla/collision_shapes.json";

impl CollisionShapes {
    /// Load `data/vanilla/collision_shapes.json`.
    ///
    /// # Errors
    /// If the file is missing, malformed, or reports a non-zero
    /// `gridEquivalence.mismatches` — that field is the dumper's own sweep of
    /// this representation against `VoxelShape.collide`, and a non-zero one
    /// means a shape exists in this version that a coordinate grid cannot
    /// carry. Refusing to load is the only response that does not put wrong
    /// physics under a green test.
    /// The process-wide table, loaded once from [`TABLE_PATH`] relative to
    /// the working directory (or the workspace root under `cargo test`);
    /// `None` when the extract is absent.
    pub fn shared() -> Option<&'static Self> {
        static TABLE: std::sync::OnceLock<Option<CollisionShapes>> = std::sync::OnceLock::new();
        TABLE
            .get_or_init(|| {
                let cwd_relative = std::path::Path::new(TABLE_PATH);
                let path: std::path::PathBuf = if cwd_relative.exists() {
                    cwd_relative.to_path_buf()
                } else {
                    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("../..")
                        .join(TABLE_PATH)
                };
                Self::load(&path).ok()
            })
            .as_ref()
    }

    pub fn load(path: &std::path::Path) -> io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let doc: serde_json::Value = serde_json::from_str(&text).map_err(io::Error::other)?;
        Self::from_json(&doc)
    }

    /// # Errors
    /// As [`Self::load`].
    pub fn from_json(doc: &serde_json::Value) -> io::Result<Self> {
        let mismatches = doc
            .get("gridEquivalence")
            .and_then(|g| g.get("mismatches"))
            .and_then(serde_json::Value::as_i64)
            .ok_or_else(|| io::Error::other("collision_shapes.json has no gridEquivalence"))?;
        if mismatches != 0 {
            return Err(io::Error::other(format!(
                "collision_shapes.json reports {mismatches} grid-equivalence mismatches — \
                 a coordinate grid does not carry every shape in this version, so this \
                 module would move entities to the wrong place"
            )));
        }
        let probes = doc["gridEquivalence"]["probes"]
            .as_i64()
            .unwrap_or_default();
        if probes < 1_000_000 {
            return Err(io::Error::other(format!(
                "collision_shapes.json reports only {probes} equivalence probes — \
                 its zero mismatches would be measuring nothing"
            )));
        }
        Self::from_json_carrier(doc)
    }

    /// The carrier alone — `shapes`, `rows`, `movingPistonStates` — without
    /// the collision sweep's proof.
    ///
    /// For `outline_shapes.json`, whose reader is a **clip**, not a collide:
    /// the grid is the `VoxelShape`'s own coordinate lists and occupancy, the
    /// same carrier the collision table proves, and a clip over it is
    /// [`crate::projectile::clip::shape_clip`]'s own equivalence argument
    /// (merged faces are interior, the inside probe runs first). The
    /// collision sweep's probes are collide queries and would not speak for
    /// a clip; asking for them here would be a check measuring the wrong
    /// reader.
    ///
    /// # Errors
    /// As [`Self::load`].
    pub fn from_json_carrier(doc: &serde_json::Value) -> io::Result<Self> {
        let raw = doc
            .get("shapes")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| io::Error::other("collision_shapes.json has no `shapes` array"))?;
        let mut shapes = Vec::with_capacity(raw.len());
        for s in raw {
            shapes.push(Self::shape_from_json(s)?);
        }

        let rows_raw = doc
            .get("rows")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| io::Error::other("collision_shapes.json has no `rows` array"))?;
        let mut rows = Vec::with_capacity(rows_raw.len());
        for r in rows_raw {
            let v = r
                .as_i64()
                .ok_or_else(|| io::Error::other("a row is not an integer"))?;
            let v = i32::try_from(v).map_err(io::Error::other)?;
            if v >= 0 && (v as usize) >= shapes.len() {
                return Err(io::Error::other(format!(
                    "row points at shape {v} of {}",
                    shapes.len()
                )));
            }
            rows.push(v);
        }
        if rows.is_empty() {
            return Err(io::Error::other("collision_shapes.json has no rows"));
        }

        let mut moving_piston = Vec::new();
        let listed = doc
            .get("movingPistonStates")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| io::Error::other("collision_shapes.json has no movingPistonStates"))?;
        for v in listed {
            let id = v
                .as_u64()
                .ok_or_else(|| io::Error::other("a movingPistonStates entry is not an id"))?;
            moving_piston.push(u32::try_from(id).map_err(io::Error::other)?);
        }
        if moving_piston.is_empty() {
            // The block exists in every version this targets, so an empty
            // list means the dumper stopped finding it rather than that there
            // is nothing to find — and the edge-cell rule would then be dead
            // code that always takes the skip.
            return Err(io::Error::other(
                "collision_shapes.json lists no moving_piston states — the edge-cell \
                 rule in collect_block_collisions would never fire",
            ));
        }
        Ok(Self {
            shapes,
            rows,
            moving_piston,
        })
    }

    fn shape_from_json(s: &serde_json::Value) -> io::Result<Shape> {
        let size_arr = s
            .get("size")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| io::Error::other("shape has no size"))?;
        let mut size = [0usize; 3];
        for (i, v) in size_arr.iter().take(3).enumerate() {
            size[i] = usize::try_from(v.as_u64().unwrap_or_default()).map_err(io::Error::other)?;
        }
        let coords_arr = s
            .get("coords")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| io::Error::other("shape has no coords"))?;
        // `[Vec::new(); 3]` needs Copy; three explicit slots keeps this const
        // in the reader's head and avoids a clone.
        let mut coords = [Vec::new(), Vec::new(), Vec::new()];
        for (a, list) in coords_arr.iter().take(3).enumerate() {
            let list = list
                .as_array()
                .ok_or_else(|| io::Error::other("a coord list is not an array"))?;
            for v in list {
                // Doubles are carried as raw IEEE-754 bits. A decimal
                // rendering round-trips only through a correctly rounded
                // parser and a reader cannot tell the difference by looking;
                // bits cannot be a half-ulp out.
                let bits = v.as_i64().ok_or_else(|| {
                    io::Error::other("a coordinate is not an integer bit pattern")
                })?;
                coords[a].push(f64::from_bits(bits as u64));
            }
            if coords[a].len() != size[a] + 1 && !(size[a] == 0 && coords[a].is_empty()) {
                return Err(io::Error::other(format!(
                    "axis {a} has {} coordinates for {} cells",
                    coords[a].len(),
                    size[a]
                )));
            }
        }
        let cells = size[0] * size[1] * size[2];
        let hex = s
            .get("voxels")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| io::Error::other("shape has no voxels"))?;
        let mut full = Vec::with_capacity(cells);
        'outer: for ch in hex.chars() {
            let nibble = ch
                .to_digit(16)
                .ok_or_else(|| io::Error::other("voxels is not hex"))?;
            for bit in (0..4).rev() {
                if full.len() == cells {
                    break 'outer;
                }
                full.push((nibble >> bit) & 1 == 1);
            }
        }
        if full.len() != cells {
            return Err(io::Error::other(format!(
                "voxels carries {} cells, the size says {cells}",
                full.len()
            )));
        }
        Ok(Shape {
            coords,
            size,
            full,
            is_block: s
                .get("full")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or_default(),
            large: s
                .get("large")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or_default(),
        })
    }

    /// The shape for a global state id, or `None` when the table has no
    /// answer — a dynamic-shape block, or an id past the end of the table.
    #[must_use]
    pub fn get(&self, state_id: u32) -> Option<&Shape> {
        let row = *self.rows.get(state_id as usize)?;
        if row < 0 {
            return None;
        }
        self.shapes.get(row as usize)
    }

    /// Whether a global state id is `minecraft:moving_piston`.
    #[must_use]
    pub fn is_moving_piston(&self, state_id: u32) -> bool {
        self.moving_piston.contains(&state_id)
    }

    #[must_use]
    pub fn shape_count(&self) -> usize {
        self.shapes.len()
    }

    #[must_use]
    pub fn state_count(&self) -> usize {
        self.rows.len()
    }
}

/// `Level.getBlockCollisions(entity, box)` — every block shape the box could
/// touch, in `Cursor3D` order.
///
/// # The one-cell shell is not padding
///
/// `BlockCollisions`'s cursor spans `floor(min - 1e-7) - 1 ..= floor(max +
/// 1e-7) + 1`, one cell wider than the box on every side, and then filters by
/// `Cursor3D.getNextType()`: 0 is the interior and always considered, 1 is a
/// face cell and is considered **only for blocks with a large collision
/// shape**, 2 is an edge cell and only for `moving_piston`, 3 is a corner and
/// is skipped outright. So the shell is not a safety margin — it is how a
/// block whose shape reaches outside its own cell still collides.
///
/// `moving_piston` is in the dynamic set and therefore has no row, so an edge
/// cell holding one raises rather than being silently skipped.
///
/// # Errors
/// [`NoShapeFor`] when a touched position holds a block the table cannot
/// answer for.
pub fn collect_block_collisions<'a, W: CollisionBlocks>(
    shapes: &'a CollisionShapes,
    world: &W,
    box_: Aabb,
) -> Result<Vec<PlacedShape<'a>>, NoShapeFor> {
    let x0 = floor(box_.min_x - EPS) - 1;
    let x1 = floor(box_.max_x + EPS) + 1;
    let y0 = floor(box_.min_y - EPS) - 1;
    let y1 = floor(box_.max_y + EPS) + 1;
    let z0 = floor(box_.min_z - EPS) - 1;
    let z1 = floor(box_.max_z + EPS) + 1;

    let mut out = Vec::new();
    // **z outermost, x innermost.** `Cursor3D.advance` computes
    // `x = index % width`, `y = (index / width) % height`,
    // `z = index / width / height`, so x is the fastest-varying coordinate.
    // The order is load-bearing, not cosmetic: `Shapes.collide` folds a
    // clamp-to-zero over this list and the clamp is not commutative, so a
    // list in a different order can produce a different resolved delta from
    // the same set of blocks.
    for z in z0..=z1 {
        for y in y0..=y1 {
            for x in x0..=x1 {
                let cell_type = usize::from(x == x0 || x == x1)
                    + usize::from(y == y0 || y == y1)
                    + usize::from(z == z0 || z == z1);
                // `Cursor3D.TYPE_CORNER`. Skipped before the block is even
                // read, which is why a corner holding a block with no row
                // does not raise.
                if cell_type == 3 {
                    continue;
                }
                let pos = BlockPos { x, y, z };
                let state_id = world.state_id_at(pos);
                if cell_type == 2 {
                    // `TYPE_EDGE`: vanilla consults the block only when it
                    // `is(Blocks.MOVING_PISTON)`, which is an identity test
                    // and needs no shape. Everything else at an edge is
                    // skipped without the shape being asked for, so a shulker
                    // box sitting diagonally past a corner must NOT raise
                    // here — it would be a refusal vanilla never makes.
                    if shapes.is_moving_piston(state_id) {
                        return Err(NoShapeFor { pos, state_id });
                    }
                    continue;
                }
                let Some(shape) = shapes.get(state_id) else {
                    // `TYPE_FACE` reaches here as well as `TYPE_INSIDE`, and
                    // deliberately. `BlockStateBase.hasLargeCollisionShape`
                    // is `cache != null ? cache.largeCollisionShape : true` —
                    // **a state with no cache reports a large shape** — so
                    // every dynamic-shape block passes the face filter and
                    // vanilla goes on to compute its shape. Skipping it here
                    // would be the wrong answer, not a cheaper one.
                    return Err(NoShapeFor { pos, state_id });
                };
                if cell_type == 1 && !shape.is_large() {
                    continue;
                }
                if shape.is_full_block() {
                    // The `shape == Shapes.block()` identity branch, whose
                    // admission test is a plain strict `AABB.intersects` with
                    // no epsilon — a different test from the one every other
                    // shape gets, and one that admits a 1e-8 overlap the
                    // other rejects.
                    if box_.intersects_cell(x, y, z) {
                        out.push(PlacedShape { shape, pos });
                    }
                    continue;
                }
                if shape.is_empty() {
                    continue;
                }
                if shape.overlaps(box_, [f64::from(x), f64::from(y), f64::from(z)]) {
                    out.push(PlacedShape { shape, pos });
                }
            }
        }
    }
    Ok(out)
}

/// `Mth.floor(double)`.
fn floor(v: f64) -> i32 {
    // `as i32` truncates toward zero; vanilla's `Mth.floor` is
    // `(int) Math.floor(d)`, which is the other rounding for negatives.
    let f = v.floor();
    f as i32
}

/// `Shapes.collide(Axis, AABB, Iterable<VoxelShape>, double)`.
///
/// The `abs(d) < 1e-7` test is at the **top of the loop body**, so it is made
/// once per shape before that shape is consulted and never after the last
/// one: an empty list returns `d` untouched however tiny it is, and a list
/// whose last shape leaves `d` at 5e-8 returns 5e-8 rather than zero.
#[must_use]
pub fn shapes_collide(axis: Axis, box_: Aabb, shapes: &[PlacedShape<'_>], d: f64) -> f64 {
    let mut d = d;
    for s in shapes {
        if d.abs() < EPS {
            return 0.0;
        }
        d = s.collide(axis, box_, d);
    }
    d
}

/// `Entity.collideWithShapes(Vec3, AABB, List<VoxelShape>)`.
///
/// Each axis is resolved against the box **already moved by the axes
/// resolved before it**, which is what makes the order matter.
#[must_use]
pub fn collide_with_shapes(v: Vec3, box_: Aabb, shapes: &[PlacedShape<'_>]) -> Vec3 {
    if shapes.is_empty() {
        return v;
    }
    let mut acc = Vec3::ZERO;
    for axis in axis_step_order(v) {
        let d = v.get(axis);
        if d == 0.0 {
            continue;
        }
        let moved = box_.shifted(acc.x, acc.y, acc.z);
        let r = shapes_collide(axis, moved, shapes, d);
        acc = acc.with(axis, r);
    }
    acc
}

/// `Entity.collideBoundingBox(Entity, Vec3, AABB, Level, List<VoxelShape>)`.
///
/// The entity-collision list vanilla passes in is empty here: nothing in this
/// tree has entity-vs-entity collision, and the golden this is gated against
/// records vanilla producing an empty one for the same scenarios rather than
/// assuming it.
///
/// # Errors
/// [`NoShapeFor`], from the collider collection.
pub fn collide_bounding_box<W: CollisionBlocks>(
    shapes: &CollisionShapes,
    world: &W,
    v: Vec3,
    box_: Aabb,
) -> Result<Vec3, NoShapeFor> {
    let colliders = collect_block_collisions(shapes, world, box_.expand_towards(v.x, v.y, v.z))?;
    Ok(collide_with_shapes(v, box_, &colliders))
}

/// `Entity.collectCandidateStepUpHeights(AABB, List<VoxelShape>, float, float)`.
///
/// Reads the shapes' **y grid lines**, not their box corners. Heights below
/// zero are skipped, a height equal to `start_y` is skipped, and a height
/// above `max_up_step` **breaks out of that shape's coordinate list and moves
/// to the next shape** — it does not abandon the whole scan, which matters
/// because the lists are per shape and ascending only within one.
///
/// Vanilla accumulates into a `FloatArraySet` and then `unstableSort`s, so
/// the result is ascending and duplicate-free; insertion order does not
/// survive and is not reproduced.
#[must_use]
pub fn collect_candidate_step_up_heights(
    box_: Aabb,
    shapes: &[PlacedShape<'_>],
    max_up_step: f32,
    start_y: f32,
) -> Vec<f32> {
    let mut set: Vec<f32> = Vec::with_capacity(4);
    for s in shapes {
        for c in s.coords_y() {
            #[allow(clippy::cast_possible_truncation)]
            let h = (c - box_.min_y) as f32;
            if h < 0.0 {
                continue;
            }
            if h == start_y {
                continue;
            }
            if h > max_up_step {
                break;
            }
            if !set.contains(&h) {
                set.push(h);
            }
        }
    }
    set.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    set
}

/// `Entity.collide(Vec3)` — the sweep, then auto-step.
///
/// `max_up_step` is `Entity.maxUpStep()` and `on_ground` is `Entity.onGround()`;
/// both are the caller's, because both come from the physics layer above this
/// module and neither is derivable here.
///
/// # Errors
/// [`NoShapeFor`], from either collider collection.
pub fn collide<W: CollisionBlocks>(
    shapes: &CollisionShapes,
    world: &W,
    v: Vec3,
    box_: Aabb,
    max_up_step: f32,
    on_ground: bool,
) -> Result<Vec3, NoShapeFor> {
    let resolved = if v.length_sqr() == 0.0 {
        v
    } else {
        collide_bounding_box(shapes, world, v, box_)?
    };
    let x_changed = v.x != resolved.x;
    let y_changed = v.y != resolved.y;
    let z_changed = v.z != resolved.z;
    // Vanilla's local 8: y was clamped AND the move was downwards. Named
    // `grounded` here because that is what it means — the entity has just
    // landed on something this tick — but it is not `onGround()`, which is
    // tested separately below.
    let grounded = y_changed && v.y < 0.0;

    if max_up_step > 0.0 && (grounded || on_ground) && (x_changed || z_changed) {
        // **`resolved.y`, not `v.y`.** The step box starts from where the
        // vertical sweep actually left the entity.
        let step_box = if grounded {
            box_.shifted(0.0, resolved.y, 0.0)
        } else {
            box_
        };
        let mut expanded = step_box.expand_towards(v.x, f64::from(max_up_step), v.z);
        if !grounded {
            expanded = expanded.expand_towards(0.0, MTH_EQUAL_EPSILON, 0.0);
        }
        let colliders = collect_block_collisions(shapes, world, expanded)?;
        // Also `resolved.y`, narrowed to f32. A port reading `v.y` here
        // filters out the wrong candidate height and the difference only
        // shows when something is standing exactly one candidate away.
        #[allow(clippy::cast_possible_truncation)]
        let start_y = resolved.y as f32;
        let heights = collect_candidate_step_up_heights(step_box, &colliders, max_up_step, start_y);
        for h in heights {
            let stepped =
                collide_with_shapes(Vec3::new(v.x, f64::from(h), v.z), step_box, &colliders);
            if stepped.horizontal_distance_sqr() > resolved.horizontal_distance_sqr() {
                let dy = box_.min_y - step_box.min_y;
                return Ok(Vec3::new(stepped.x, stepped.y - dy, stepped.z));
            }
        }
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The constant in `Entity.collide`'s second `expandTowards` is the float
    /// `1.0E-5f` promoted, and the constant pool says so exactly. Typing
    /// `1e-5` would be a different number and the difference is invisible in
    /// any output that is not bit-compared.
    #[test]
    fn the_step_up_epsilon_is_a_promoted_float() {
        assert_eq!(MTH_EQUAL_EPSILON, -9.999999747378752E-6);
        assert_ne!(MTH_EQUAL_EPSILON, -1.0E-5);
    }

    /// `abs(x) < abs(z)` resolves the **larger** horizontal axis second, not
    /// the smaller, and Y is always first.
    #[test]
    fn the_axis_order_puts_the_bigger_horizontal_first() {
        assert_eq!(
            axis_step_order(Vec3::new(0.1, 0.0, 0.9)),
            [Axis::Y, Axis::Z, Axis::X]
        );
        assert_eq!(
            axis_step_order(Vec3::new(0.9, 0.0, 0.1)),
            [Axis::Y, Axis::X, Axis::Z]
        );
        // Equal magnitudes take the `ifge` branch.
        assert_eq!(
            axis_step_order(Vec3::new(0.5, 0.0, -0.5)),
            [Axis::Y, Axis::X, Axis::Z]
        );
    }

    /// `Mth.floor` rounds towards negative infinity; `as i32` alone truncates
    /// towards zero and would put the cursor one cell short on the negative
    /// side of every axis.
    #[test]
    fn floor_is_not_truncation() {
        assert_eq!(floor(-0.5), -1);
        assert_eq!(floor(-1.0), -1);
        assert_eq!(floor(0.5), 0);
    }
}
