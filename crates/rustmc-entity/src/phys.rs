//! The `net.minecraft.world.phys.Vec3` / `AABB` and `net.minecraft.core`
//! arithmetic the mob AI calls, from the 26.2 jar.
//!
//! Only the methods the transcribed code reaches are here, each with the
//! bytecode it came from. `rustmc-server` has its own `Vec3`/`Aabb` for
//! collision; this crate cannot depend on that crate, so these are the
//! entity crate's own, and a later merge of the two is a rename.

use rustmc_data::BlockPos;

/// `net.minecraft.world.phys.Vec3`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// `Vec3.add(Vec3)`.
impl std::ops::Add for Vec3 {
    type Output = Vec3;
    fn add(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

impl Vec3 {
    /// `Vec3.ZERO`.
    pub const ZERO: Vec3 = Vec3 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    #[must_use]
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Vec3 { x, y, z }
    }

    /// `Vec3.atLowerCornerWithOffset(Vec3i, dx, dy, dz)`.
    #[must_use]
    pub fn at_lower_corner_with_offset(p: BlockPos, dx: f64, dy: f64, dz: f64) -> Self {
        Vec3::new(
            f64::from(p.x) + dx,
            f64::from(p.y) + dy,
            f64::from(p.z) + dz,
        )
    }

    /// `Vec3.atBottomCenterOf(Vec3i)`: offset `(0.5, 0.0, 0.5)`.
    #[must_use]
    pub fn at_bottom_center_of(p: BlockPos) -> Self {
        Self::at_lower_corner_with_offset(p, 0.5, 0.0, 0.5)
    }

    /// `Vec3.atCenterOf(Vec3i)`: offset `(0.5, 0.5, 0.5)`.
    #[must_use]
    pub fn at_center_of(p: BlockPos) -> Self {
        Self::at_lower_corner_with_offset(p, 0.5, 0.5, 0.5)
    }

    /// `subtract(Vec3)`.
    #[must_use]
    pub fn subtract(self, o: Vec3) -> Self {
        self.subtract3(o.x, o.y, o.z)
    }

    /// `subtract(double, double, double)`.
    #[must_use]
    pub fn subtract3(self, x: f64, y: f64, z: f64) -> Self {
        Vec3::new(self.x - x, self.y - y, self.z - z)
    }

    /// `add(double, double, double)`.
    #[must_use]
    pub fn add3(self, x: f64, y: f64, z: f64) -> Self {
        Vec3::new(self.x + x, self.y + y, self.z + z)
    }

    /// `scale(double)`: `multiply(s, s, s)`.
    #[must_use]
    pub fn scale(self, s: f64) -> Self {
        Vec3::new(self.x * s, self.y * s, self.z * s)
    }

    /// `dot(Vec3)`: `x*x' + y*y' + z*z'` in that order.
    #[must_use]
    pub fn dot(self, o: Vec3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    /// `lengthSqr()`: `x*x + y*y + z*z`.
    #[must_use]
    pub fn length_sqr(self) -> f64 {
        self.x * self.x + self.y * self.y + self.z * self.z
    }

    /// `length()`: `Math.sqrt(lengthSqr)`, with the sum written out the same
    /// way so the rounding matches.
    #[must_use]
    pub fn length(self) -> f64 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }

    /// `horizontalDistanceSqr()`: `x*x + z*z`.
    #[must_use]
    pub fn horizontal_distance_sqr(self) -> f64 {
        self.x * self.x + self.z * self.z
    }

    /// `normalize()`: `len < 1.0E-5 ? ZERO : (x/len, y/len, z/len)`.
    ///
    /// The threshold `ldc2_w` carries is `9.999999747378752E-6`, which is
    /// the float literal `1.0E-5f` widened to a double; the bytecode's
    /// constant is used verbatim rather than the decimal it came from.
    #[must_use]
    pub fn normalize(self) -> Self {
        let len = (self.x * self.x + self.y * self.y + self.z * self.z).sqrt();
        if len < 9.999_999_747_378_752E-6 {
            Vec3::ZERO
        } else {
            Vec3::new(self.x / len, self.y / len, self.z / len)
        }
    }

    /// `distanceTo(Vec3)`: `sqrt(dx*dx + dy*dy + dz*dz)` with `d = other - this`.
    #[must_use]
    pub fn distance_to(self, o: Vec3) -> f64 {
        let dx = o.x - self.x;
        let dy = o.y - self.y;
        let dz = o.z - self.z;
        (dx * dx + dy * dy + dz * dz).sqrt()
    }

    /// `distanceToSqr(Vec3)`.
    #[must_use]
    pub fn distance_to_sqr(self, o: Vec3) -> f64 {
        let dx = o.x - self.x;
        let dy = o.y - self.y;
        let dz = o.z - self.z;
        dx * dx + dy * dy + dz * dz
    }

    /// `distanceToSqr(double, double, double)`.
    #[must_use]
    pub fn distance_to_sqr3(self, x: f64, y: f64, z: f64) -> f64 {
        let dx = x - self.x;
        let dy = y - self.y;
        let dz = z - self.z;
        dx * dx + dy * dy + dz * dz
    }

    /// `closerThan(Position, double)`: `distanceToSqr(p) < d * d`.
    #[must_use]
    pub fn closer_than(self, p: Vec3, d: f64) -> bool {
        self.distance_to_sqr3(p.x, p.y, p.z) < d * d
    }
}

/// `net.minecraft.world.phys.AABB`.
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
    /// `AABB(double, double, double, double, double, double)`: the
    /// constructor takes `Math.min`/`Math.max` of each pair.
    #[must_use]
    pub fn new(x1: f64, y1: f64, z1: f64, x2: f64, y2: f64, z2: f64) -> Self {
        use rustmc_worldgen::math::{java_max, java_min};
        Aabb {
            min_x: java_min(x1, x2),
            min_y: java_min(y1, y2),
            min_z: java_min(z1, z2),
            max_x: java_max(x1, x2),
            max_y: java_max(y1, y2),
            max_z: java_max(z1, z2),
        }
    }

    /// `AABB(BlockPos)` (0..40): the unit cube at the block's low corner,
    /// `(x, y, z)` to `(x+1, y+1, z+1)`, through the min/max constructor.
    #[must_use]
    pub fn of_block(p: BlockPos) -> Self {
        Aabb::new(
            f64::from(p.x),
            f64::from(p.y),
            f64::from(p.z),
            f64::from(p.x + 1),
            f64::from(p.y + 1),
            f64::from(p.z + 1),
        )
    }

    /// `AABB.ofSize(Vec3, double, double, double)` (0..69): the box of the
    /// given sizes centred on the point — each axis is `c - s/2 .. c + s/2`,
    /// through the min/max constructor.
    #[must_use]
    pub fn of_size(c: Vec3, xs: f64, ys: f64, zs: f64) -> Self {
        Aabb::new(
            c.x - xs / 2.0,
            c.y - ys / 2.0,
            c.z - zs / 2.0,
            c.x + xs / 2.0,
            c.y + ys / 2.0,
            c.z + zs / 2.0,
        )
    }

    /// `AABB.move(BlockPos)` (0..67): every coordinate plus the block's,
    /// widened to `double`. Unlike [`Aabb::inflate`] this one *does* go
    /// through the min/max constructor in the jar, but a translation can
    /// never invert a box so the result is the same either way.
    #[must_use]
    pub fn moved_by_block(&self, p: BlockPos) -> Self {
        self.moved(f64::from(p.x), f64::from(p.y), f64::from(p.z))
    }

    /// `AABB.contains(double, double, double)` (0..61): `min <= v < max`
    /// on all three axes — inclusive at the low corner, exclusive at the
    /// high one.
    #[must_use]
    pub fn contains(&self, x: f64, y: f64, z: f64) -> bool {
        x >= self.min_x
            && x < self.max_x
            && y >= self.min_y
            && y < self.max_y
            && z >= self.min_z
            && z < self.max_z
    }

    /// `getXsize()`.
    #[must_use]
    pub fn x_size(&self) -> f64 {
        self.max_x - self.min_x
    }

    /// `getYsize()`.
    #[must_use]
    pub fn y_size(&self) -> f64 {
        self.max_y - self.min_y
    }

    /// `getZsize()`.
    #[must_use]
    pub fn z_size(&self) -> f64 {
        self.max_z - self.min_z
    }

    /// `getSize()`: `(xsize + ysize + zsize) / 3.0`.
    #[must_use]
    pub fn size(&self) -> f64 {
        (self.x_size() + self.y_size() + self.z_size()) / 3.0
    }

    /// `move(double, double, double)`.
    #[must_use]
    pub fn moved(&self, dx: f64, dy: f64, dz: f64) -> Self {
        Aabb {
            min_x: self.min_x + dx,
            min_y: self.min_y + dy,
            min_z: self.min_z + dz,
            max_x: self.max_x + dx,
            max_y: self.max_y + dy,
            max_z: self.max_z + dz,
        }
    }

    /// `move(Vec3)`.
    #[must_use]
    pub fn moved_by(&self, v: Vec3) -> Self {
        self.moved(v.x, v.y, v.z)
    }

    /// `inflate(double, double, double)` (0..69): the six shrunk/grown
    /// coordinates are computed into locals and then handed to the
    /// `AABB(DDDDDD)` **constructor**, so a deflation large enough to cross
    /// the box is re-ordered rather than left inverted. Going straight to
    /// the struct skipped that min/max and is the one case where the two
    /// disagree.
    #[must_use]
    pub fn inflate(&self, dx: f64, dy: f64, dz: f64) -> Self {
        Aabb::new(
            self.min_x - dx,
            self.min_y - dy,
            self.min_z - dz,
            self.max_x + dx,
            self.max_y + dy,
            self.max_z + dz,
        )
    }

    /// `intersects(AABB)`: strict on every axis.
    #[must_use]
    pub fn intersects(&self, o: &Aabb) -> bool {
        self.min_x < o.max_x
            && self.max_x > o.min_x
            && self.min_y < o.max_y
            && self.max_y > o.min_y
            && self.min_z < o.max_z
            && self.max_z > o.min_z
    }
}

/// `BlockPos.containing(double, double, double)`: `Mth.floor` of each.
#[must_use]
pub fn block_pos_containing(x: f64, y: f64, z: f64) -> BlockPos {
    use rustmc_worldgen::math::floor;
    BlockPos {
        x: floor(x),
        y: floor(y),
        z: floor(z),
    }
}

/// `BlockPos.containing(Position)`.
#[must_use]
pub fn block_pos_containing_vec(v: Vec3) -> BlockPos {
    block_pos_containing(v.x, v.y, v.z)
}

/// `Vec3i.distToCenterSqr(double, double, double)`: squared distance from
/// the block's centre `(x+0.5, y+0.5, z+0.5)`.
#[must_use]
pub fn dist_to_center_sqr(p: BlockPos, x: f64, y: f64, z: f64) -> f64 {
    let dx = f64::from(p.x) + 0.5 - x;
    let dy = f64::from(p.y) + 0.5 - y;
    let dz = f64::from(p.z) + 0.5 - z;
    dx * dx + dy * dy + dz * dz
}

/// `Vec3i.closerToCenterThan(Position, double)`: `distToCenterSqr < d*d`.
#[must_use]
pub fn closer_to_center_than(p: BlockPos, v: Vec3, d: f64) -> bool {
    dist_to_center_sqr(p, v.x, v.y, v.z) < crate::mth::square(d)
}

/// `BlockPos.asLong(int, int, int)` with 26/12/26-bit packing:
/// `PACKED_HORIZONTAL_LENGTH = 1 + log2(smallestEncompassingPowerOfTwo(30000000)) = 26`,
/// `PACKED_Y_LENGTH = 64 - 52 = 12`, `Z_OFFSET = 12`, `X_OFFSET = 38`.
#[must_use]
pub fn block_pos_as_long(x: i32, y: i32, z: i32) -> i64 {
    const X_MASK: i64 = (1 << 26) - 1;
    const Y_MASK: i64 = (1 << 12) - 1;
    const Z_MASK: i64 = (1 << 26) - 1;
    let mut l = 0i64;
    l |= (i64::from(x) & X_MASK) << 38;
    l |= i64::from(y) & Y_MASK;
    l |= (i64::from(z) & Z_MASK) << 12;
    l
}

/// `Vec3i.distSqr(Vec3i)`: `distToLowCornerSqr(x, y, z)`, the squared
/// distance between the two corners in doubles.
#[must_use]
pub fn dist_sqr(a: BlockPos, b: BlockPos) -> f64 {
    let dx = f64::from(a.x) - f64::from(b.x);
    let dy = f64::from(a.y) - f64::from(b.y);
    let dz = f64::from(a.z) - f64::from(b.z);
    dx * dx + dy * dy + dz * dz
}

/// `BlockPos.withinManhattan(BlockPos, int, int, int)`: the positions
/// within the Manhattan reach, nearest first, in the order `BlockPos$3`
/// produces them.
///
/// `computeNext` (offsets 0..255) walks depth 0, 1, 2 ... up to
/// `xr + yr + zr`; within a depth, `x` from `-min(xr, depth)` up, `y` from
/// `-min(yr, depth - |x|)` up, and for each `(x, y)` the `+z` position
/// followed (on the next call, via `zMirror`) by the `-z` one when `z` is
/// not zero. `findClosestMatch` is the first of these a predicate accepts.
#[must_use]
pub fn within_manhattan(origin: BlockPos, xr: i32, yr: i32, zr: i32) -> Vec<BlockPos> {
    let max_depth = xr + yr + zr;
    let mut out = Vec::new();
    let mut depth = 0;
    let mut max_x = 0;
    let mut max_y = 0;
    let mut x = 0;
    let mut y = 0;
    loop {
        if y > max_y {
            x += 1;
            if x > max_x {
                depth += 1;
                if depth > max_depth {
                    return out;
                }
                max_x = xr.min(depth);
                x = -max_x;
            }
            max_y = yr.min(depth - x.abs());
            y = -max_y;
        }
        let dz = depth - x.abs() - y.abs();
        if dz <= zr {
            out.push(BlockPos {
                x: origin.x + x,
                y: origin.y + y,
                z: origin.z + dz,
            });
            if dz != 0 {
                out.push(BlockPos {
                    x: origin.x + x,
                    y: origin.y + y,
                    z: origin.z - dz,
                });
            }
        }
        y += 1;
    }
}

/// `BlockPos.findClosestMatch(BlockPos, int, int, Predicate)`: the first
/// position of `withinManhattan(pos, xz, y, xz)` the predicate accepts.
pub fn find_closest_match(
    origin: BlockPos,
    xz: i32,
    y: i32,
    mut pred: impl FnMut(BlockPos) -> bool,
) -> Option<BlockPos> {
    within_manhattan(origin, xz, y, xz)
        .into_iter()
        .find(|p| pred(*p))
}

/// `BlockPos.offset(int, int, int)`.
#[must_use]
pub fn offset(p: BlockPos, dx: i32, dy: i32, dz: i32) -> BlockPos {
    BlockPos {
        x: p.x + dx,
        y: p.y + dy,
        z: p.z + dz,
    }
}

/// `BlockPos.below()`.
#[must_use]
pub fn below(p: BlockPos) -> BlockPos {
    offset(p, 0, -1, 0)
}

/// `BlockPos.above()`.
#[must_use]
pub fn above(p: BlockPos) -> BlockPos {
    offset(p, 0, 1, 0)
}

/// `net.minecraft.core.Direction`, in ordinal order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

impl Direction {
    /// `Direction.Plane.HORIZONTAL`'s facings array, which is the order its
    /// iterator walks: `NORTH, EAST, SOUTH, WEST` (from
    /// `Direction$Plane.<clinit>` offsets 12..31).
    pub const HORIZONTAL: [Direction; 4] = [
        Direction::North,
        Direction::East,
        Direction::South,
        Direction::West,
    ];

    /// `getStepX()`: the constructor's normal, from `Direction.<clinit>`.
    #[must_use]
    pub const fn step_x(self) -> i32 {
        match self {
            Direction::West => -1,
            Direction::East => 1,
            _ => 0,
        }
    }

    /// `getStepY()`.
    #[must_use]
    pub const fn step_y(self) -> i32 {
        match self {
            Direction::Down => -1,
            Direction::Up => 1,
            _ => 0,
        }
    }

    /// `getStepZ()`.
    #[must_use]
    pub const fn step_z(self) -> i32 {
        match self {
            Direction::North => -1,
            Direction::South => 1,
            _ => 0,
        }
    }

    /// `get2DDataValue()`: the constructor's `data2d` — `SOUTH 0, WEST 1,
    /// NORTH 2, EAST 3`; the vertical two carry `-1`.
    #[must_use]
    pub const fn data_2d(self) -> i32 {
        match self {
            Direction::South => 0,
            Direction::West => 1,
            Direction::North => 2,
            Direction::East => 3,
            Direction::Down | Direction::Up => -1,
        }
    }

    /// `getClockWise()`: `NORTH -> EAST, EAST -> SOUTH, SOUTH -> WEST, WEST -> NORTH`
    /// (the `tableswitch` at offsets 4..77); vertical throws in the jar and
    /// is unreachable from `Plane.HORIZONTAL` here.
    #[must_use]
    pub const fn clockwise(self) -> Direction {
        match self {
            Direction::North => Direction::East,
            Direction::East => Direction::South,
            Direction::South => Direction::West,
            Direction::West => Direction::North,
            Direction::Down | Direction::Up => self,
        }
    }
}

/// `SectionPos.blockToSectionCoord(int)`: an arithmetic right shift by 4.
#[must_use]
pub fn block_to_section_coord(v: i32) -> i32 {
    v >> 4
}

/// `ChunkPos.containing(BlockPos)` (0..21) as an `(x, z)` pair:
/// `blockToSectionCoord` of the block's x and z. The y is not read.
#[must_use]
pub fn chunk_pos_containing(p: BlockPos) -> (i32, i32) {
    (block_to_section_coord(p.x), block_to_section_coord(p.z))
}

/// `ChunkPos.rangeClosed(ChunkPos, int)` (0..41) materialised in the order
/// its spliterator yields, which is what `Stream.toList()` produces.
///
/// The `(center, r)` overload delegates to `rangeClosed(from, to)` with
/// `from = (cx - r, cz - r)` and `to = (cx + r, cz + r)`, and that one
/// computes `xDiff = from.x < to.x ? 1 : -1` and `zDiff` likewise, then
/// hands `ChunkPos$2` to `StreamSupport.stream`.
///
/// `ChunkPos$2.tryAdvance` (0..114) is the whole order: the first call
/// emits `from`; after that, if `pos.x == to.x` it either stops (when
/// `pos.z == to.z` too) or emits `new ChunkPos(from.x, pos.z + zDiff)`,
/// and otherwise it emits `new ChunkPos(pos.x + xDiff, pos.z)`. So **x is
/// the inner axis and z the outer one**, both ascending for a positive
/// `r`, and the x of every row restarts at `from.x` rather than being
/// stepped back — which is why a `to` whose z differs but whose x has been
/// reached still advances the row.
///
/// `r == 0` degenerates: `from == to`, `xDiff` and `zDiff` are both `-1`
/// because `<` is false, and the walk stops after the single `from`.
#[must_use]
pub fn chunk_pos_range_closed(center: (i32, i32), r: i32) -> Vec<(i32, i32)> {
    let from = (center.0 - r, center.1 - r);
    let to = (center.0 + r, center.1 + r);
    let x_diff = if from.0 < to.0 { 1 } else { -1 };
    let z_diff = if from.1 < to.1 { 1 } else { -1 };
    let mut out = Vec::new();
    let mut pos: Option<(i32, i32)> = None;
    loop {
        pos = Some(match pos {
            None => from,
            Some((x, z)) => {
                if x == to.0 {
                    if z == to.1 {
                        break;
                    }
                    (from.0, z + z_diff)
                } else {
                    (x + x_diff, z)
                }
            }
        });
        out.push(pos.expect("just set"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_zeroes_below_the_threshold() {
        assert_eq!(Vec3::new(1e-6, 0.0, 0.0).normalize(), Vec3::ZERO);
        assert_eq!(
            Vec3::new(2.0, 0.0, 0.0).normalize(),
            Vec3::new(1.0, 0.0, 0.0)
        );
    }

    #[test]
    fn closer_than_is_strict() {
        let a = Vec3::new(0.0, 0.0, 0.0);
        assert!(!a.closer_than(Vec3::new(2.0, 0.0, 0.0), 2.0));
        assert!(a.closer_than(Vec3::new(1.9, 0.0, 0.0), 2.0));
        let p = BlockPos { x: 0, y: 0, z: 0 };
        assert!(closer_to_center_than(p, Vec3::new(0.5, 0.5, 0.5), 0.1));
        assert!(!closer_to_center_than(p, Vec3::new(1.5, 0.5, 0.5), 1.0));
    }

    #[test]
    fn as_long_packs_26_12_26() {
        assert_eq!(block_pos_as_long(0, 0, 0), 0);
        assert_eq!(block_pos_as_long(1, 0, 0), 1 << 38);
        assert_eq!(block_pos_as_long(0, 1, 0), 1);
        assert_eq!(block_pos_as_long(0, 0, 1), 1 << 12);
        assert_eq!(block_pos_as_long(-1, 0, 0), ((1i64 << 26) - 1) << 38);
        assert_eq!(block_pos_as_long(0, -1, 0), (1 << 12) - 1);
    }

    #[test]
    fn horizontal_plane_order_and_clockwise() {
        let d2: Vec<i32> = Direction::HORIZONTAL.iter().map(|d| d.data_2d()).collect();
        assert_eq!(d2, vec![2, 3, 0, 1]);
        assert_eq!(Direction::North.clockwise(), Direction::East);
        assert_eq!(Direction::West.clockwise(), Direction::North);
        assert_eq!((Direction::East.step_x(), Direction::East.step_z()), (1, 0));
        assert_eq!(
            (Direction::South.step_x(), Direction::South.step_z()),
            (0, 1)
        );
    }

    /// The first positions `BlockPos$3` yields: the origin, then depth 1
    /// as -x, -y, +z, -z, +y, +x; depth runs to xr + yr + zr, so a reach
    /// of 1 on every axis is the 27-block cube.
    #[test]
    fn within_manhattan_order_is_the_iterators() {
        let o = BlockPos { x: 0, y: 0, z: 0 };
        let got: Vec<(i32, i32, i32)> = within_manhattan(o, 1, 1, 1)
            .into_iter()
            .map(|p| (p.x, p.y, p.z))
            .collect();
        assert_eq!(got.len(), 27);
        assert_eq!(
            &got[..7],
            &[
                (0, 0, 0),
                (-1, 0, 0),
                (0, -1, 0),
                (0, 0, 1),
                (0, 0, -1),
                (0, 1, 0),
                (1, 0, 0)
            ]
        );
        assert_eq!(
            &got[7..11],
            &[(-1, -1, 0), (-1, 0, 1), (-1, 0, -1), (-1, 1, 0)]
        );
        assert_eq!(got[26], (1, 1, -1));
        // Reach limits: no y beyond 0 when yr is 0, depth still runs to 2.
        let got: Vec<(i32, i32, i32)> = within_manhattan(o, 1, 0, 1)
            .into_iter()
            .map(|p| (p.x, p.y, p.z))
            .collect();
        assert_eq!(
            got,
            vec![
                (0, 0, 0),
                (-1, 0, 0),
                (0, 0, 1),
                (0, 0, -1),
                (1, 0, 0),
                (-1, 0, 1),
                (-1, 0, -1),
                (1, 0, 1),
                (1, 0, -1)
            ]
        );
        assert_eq!(
            find_closest_match(o, 1, 0, |p| p.z == -1),
            Some(BlockPos { x: 0, y: 0, z: -1 })
        );
    }

    #[test]
    fn aabb_constructor_orders_its_corners() {
        let b = Aabb::new(1.0, 2.0, 3.0, 0.0, -1.0, 4.0);
        assert_eq!((b.min_x, b.max_x), (0.0, 1.0));
        assert_eq!((b.min_y, b.max_y), (-1.0, 2.0));
        assert_eq!((b.min_z, b.max_z), (3.0, 4.0));
        assert_eq!(b.size(), (1.0 + 3.0 + 1.0) / 3.0);
        assert!(b.intersects(&b.moved(0.5, 0.0, 0.0)));
        assert!(!b.intersects(&b.moved(1.0, 0.0, 0.0)));
    }
}
