//! Nether portal frames — `net.minecraft.world.level.portal.PortalShape`,
//! transcribed from 26.2 bytecode.
//!
//! # The search is three walks, and none of them is a perimeter scan
//!
//! Vanilla does not test the ring of blocks around the opening. It:
//!
//! 1. walks **down** from the clicked position while the block below is
//!    "empty", stopping at `max(level.minY, clicked.y - 21)`;
//! 2. walks **left** along the portal's axis counting positions that are
//!    empty *and* stand on a frame block, then reads the same walk rightwards
//!    for the width;
//! 3. walks **up** the two side columns together, and only then checks the
//!    row above the top for frame blocks.
//!
//! The corners are never read. A frame with all four corner blocks knocked
//! out is a valid portal, and a transcription that scans the perimeter refuses
//! it — `portal_shape.json`'s `no-corners-3x4` row is that frame.
//!
//! # `numPortalBlocks` is counted during the height walk, not afterwards
//!
//! `getDistanceUntilTop` increments the counter every time it steps over a
//! block that already `is(Blocks.NETHER_PORTAL)`. It is what separates a lit
//! portal from an unlit frame of the same size, because `isComplete()` is
//! `isValid() && numPortalBlocks == width * height`. A count taken by a second
//! pass over the finished rectangle would agree on both of the golden's
//! extreme rows and disagree on `min-2x3-x-half-lit`.
//!
//! # "Empty" is three things and one of them is a tag
//!
//! `isEmpty(state)` is `isAir() || is(BlockTags.FIRE) || is(NETHER_PORTAL)`.
//! The tag matters: lighting a portal works because the fire the flint and
//! steel places is *inside* the opening and does not stop the search.
//! `BlockTags.FIRE` is `minecraft:fire` and `minecraft:soul_fire` in the 26.2
//! datapack, and [`FIRE_TAG`] is that list. It is spelled out here rather than
//! read from the tag extract because this crate has no tag loader; the extract
//! is what [`tests`] checks it against.
//!
//! # Evidence
//!
//! `crates/rustmc-server/golden/portal_shape.json`: eighteen frames run
//! through vanilla's own `findAnyShape` against an in-memory `BlockGetter`,
//! recording width, height, `numPortalBlocks`, the bottom-left corner, the
//! right direction and both booleans. `ci/portal-golden.sh` refuses a capture
//! taken in a JVM where `BlockTags.FIRE` came back unbound — which is what it
//! did on the first run, and is why the generator now reloads that one tag.

use rustmc_data::BlockPos;

/// `BaseFireBlock.isPortal(level, pos, direction)` — whether right-clicking
/// with flint and steel lights a **portal** instead of placing fire.
///
/// This is the branch `crate::itemuse::flint_and_steel` does not take, and the
/// three things in it are all decided before any shape is looked for:
///
/// ```text
///     0  if (!inPortalDimension(level)) return false
///    17  for (Direction d : Direction.values())
///           if (level.getBlockState(pos.relative(d)).is(OBSIDIAN)) { found = true; break; }
///    79  if (!found) return false
///    86  axis = direction.getAxis().isHorizontal()
///             ? direction.getCounterClockWise().getAxis()
///             : Direction.Plane.HORIZONTAL.getRandomAxis(level.getRandom())
///   118  return PortalShape.findEmptyPortalShape(level, pos, axis).isPresent()
/// ```
///
/// # A portal cannot be lit in the End
///
/// `inPortalDimension` is `dimension() == OVERWORLD || dimension() == NETHER`,
/// and nothing else. Flint and steel in the End places fire and never a
/// portal, whatever the frame around it looks like.
///
/// # The axis is an RNG DRAW when the clicked face is up or down
///
/// A horizontal face fixes the axis — `direction.getCounterClockWise().getAxis()`
/// — but clicking the **top** of a block inside a frame draws one:
/// `Direction.Plane.HORIZONTAL.getRandomAxis(level.getRandom())`, off the
/// level's own random source, at that point in the tick. So lighting a portal
/// from above consumes a level-RNG draw and can pick the axis the frame is not
/// on, in which case `findEmptyPortalShape` fails and ordinary fire is placed.
/// A transcription that always derived the axis from the frame would light
/// portals vanilla refuses and would consume one draw fewer.
///
/// # The obsidian check is a neighbour scan, not a frame test
///
/// It walks `Direction.values()` — DOWN, UP, NORTH, SOUTH, WEST, EAST, in
/// `Direction`'s own ordinal order — and stops at the **first** obsidian
/// neighbour. It is a cheap pre-filter, not the shape search; the shape search
/// is `findEmptyPortalShape` and runs after it.
#[must_use]
pub const fn in_portal_dimension(dimension: crate::dimension::Dimension) -> bool {
    matches!(
        dimension,
        crate::dimension::Dimension::Overworld | crate::dimension::Dimension::Nether
    )
}

/// `Direction.values()` in ordinal order — the order
/// [`in_portal_dimension`]'s caller scans for an obsidian neighbour in, and
/// the order that decides which neighbour a frame with obsidian on two sides
/// is found by.
///
/// **Y first, then Z, then X.** `Direction`'s ordinal groups the axes that
/// way, and it is the same ordering that once made a golden test assert the
/// `DOWN` face's cursor lay on the *x* face (`docs/AGENT-BRIEF.md`, "Every
/// gate").
pub const DIRECTION_VALUES: [(i32, i32, i32); 6] = [
    (0, -1, 0),
    (0, 1, 0),
    (0, 0, -1),
    (0, 0, 1),
    (-1, 0, 0),
    (1, 0, 0),
];

/// `PortalShape.MIN_WIDTH`.
pub const MIN_WIDTH: i32 = 2;
/// `PortalShape.MAX_WIDTH`.
pub const MAX_WIDTH: i32 = 21;
/// `PortalShape.MIN_HEIGHT`.
pub const MIN_HEIGHT: i32 = 3;
/// `PortalShape.MAX_HEIGHT`.
pub const MAX_HEIGHT: i32 = 21;

/// `BlockTags.FIRE` as of 26.2 — `data/minecraft/tags/block/fire.json`.
pub const FIRE_TAG: [&str; 2] = ["minecraft:fire", "minecraft:soul_fire"];

/// The frame material. `PortalShape.FRAME` is
/// `(state, level, pos) -> state.is(Blocks.OBSIDIAN)` — one block, no tag, and
/// nothing about the level or position, so the predicate is a name comparison.
pub const FRAME_BLOCK: &str = "minecraft:obsidian";

/// The block the opening is filled with.
pub const PORTAL_BLOCK: &str = "minecraft:nether_portal";

/// `Direction.Axis`, restricted to the two a portal can lie on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    X,
    Z,
}

impl Axis {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::X => "x",
            Self::Z => "z",
        }
    }

    /// `findAnyShape`'s `rightDir`: `WEST` for an X-axis portal, `SOUTH` for a
    /// Z-axis one. "Right" is from the point of view of someone walking
    /// *through*, and the two are not mirror images of each other — WEST is
    /// negative x, SOUTH is **positive** z.
    #[must_use]
    pub fn right_dir(self) -> &'static str {
        match self {
            Self::X => "west",
            Self::Z => "south",
        }
    }

    /// One step in `rightDir`.
    fn right_step(self) -> (i32, i32) {
        match self {
            Self::X => (-1, 0),
            Self::Z => (0, 1),
        }
    }
}

/// `PortalShape`'s six fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortalShape {
    pub axis: Axis,
    pub num_portal_blocks: i32,
    pub bottom_left: BlockPos,
    pub width: i32,
    pub height: i32,
}

impl PortalShape {
    /// `isValid()` — the size bounds, both inclusive on both sides.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.width >= MIN_WIDTH
            && self.width <= MAX_WIDTH
            && self.height >= MIN_HEIGHT
            && self.height <= MAX_HEIGHT
    }

    /// `isComplete()`.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.is_valid() && self.num_portal_blocks == self.width * self.height
    }

    /// `rightDir`, which is a function of the axis alone.
    #[must_use]
    pub fn right_dir(&self) -> &'static str {
        self.axis.right_dir()
    }
}

/// A state's block id — everything before the `[`.
fn name_of(state: &str) -> &str {
    state.split_once('[').map_or(state, |(b, _)| b)
}

/// `PortalShape.isEmpty(BlockState)`.
#[must_use]
pub fn is_empty(state: &str) -> bool {
    let n = name_of(state);
    n == "minecraft:air" || FIRE_TAG.contains(&n) || n == PORTAL_BLOCK
}

/// `PortalShape.FRAME.test(...)`.
#[must_use]
pub fn is_frame(state: &str) -> bool {
    name_of(state) == FRAME_BLOCK
}

fn offset(p: BlockPos, d: (i32, i32), n: i32) -> BlockPos {
    BlockPos {
        x: p.x + d.0 * n,
        y: p.y,
        z: p.z + d.1 * n,
    }
}

fn up(p: BlockPos, n: i32) -> BlockPos {
    BlockPos { y: p.y + n, ..p }
}

/// `PortalShape.findAnyShape(BlockGetter, BlockPos, Axis)`.
///
/// `get` is the level's `getBlockState`; `min_y` is `BlockGetter.getMinY()`.
/// A shape is always returned — vanilla returns a zero-sized one rather than
/// an `Option`, and its `bottomLeft` in that case is whichever position the
/// search gave up at, which is not always the one handed in.
pub fn find_any_shape<F>(get: &F, min_y: i32, from: BlockPos, axis: Axis) -> PortalShape
where
    F: Fn(BlockPos) -> String,
{
    let right = axis.right_step();
    let Some(bottom_left) = calculate_bottom_left(get, min_y, right, from) else {
        return PortalShape {
            axis,
            num_portal_blocks: 0,
            bottom_left: from,
            width: 0,
            height: 0,
        };
    };
    let width = calculate_width(get, bottom_left, right);
    if width == 0 {
        return PortalShape {
            axis,
            num_portal_blocks: 0,
            bottom_left,
            width: 0,
            height: 0,
        };
    }
    let mut portal_blocks = 0;
    let height = calculate_height(get, bottom_left, right, width, &mut portal_blocks);
    PortalShape {
        axis,
        num_portal_blocks: portal_blocks,
        bottom_left,
        width,
        height,
    }
}

/// `calculateBottomLeft`.
///
/// The floor of the downward walk is `max(level.getMinY(), pos.getY() - 21)`,
/// and the walk stops when the block *below* is not empty — so it lands on the
/// lowest empty block of the column, not below it.
fn calculate_bottom_left<F>(
    get: &F,
    min_y: i32,
    right: (i32, i32),
    mut pos: BlockPos,
) -> Option<BlockPos>
where
    F: Fn(BlockPos) -> String,
{
    let floor = min_y.max(pos.y - MAX_HEIGHT);
    while pos.y > floor && is_empty(&get(up(pos, -1))) {
        pos = up(pos, -1);
    }
    // The walk is in the direction OPPOSITE `rightDir`, then one step back.
    let left = (-right.0, -right.1);
    let d = distance_until_edge_above_frame(get, pos, left) - 1;
    if d < 0 {
        return None;
    }
    Some(offset(pos, left, d))
}

/// `calculateWidth` — the raw distance, zeroed unless it is in 2..=21.
fn calculate_width<F>(get: &F, bottom_left: BlockPos, right: (i32, i32)) -> i32
where
    F: Fn(BlockPos) -> String,
{
    let d = distance_until_edge_above_frame(get, bottom_left, right);
    if !(MIN_WIDTH..=MAX_WIDTH).contains(&d) {
        return 0;
    }
    d
}

/// `getDistanceUntilEdgeAboveFrame`.
///
/// Two conditions per step, and both matter: the block itself must be empty
/// (or the walk stops, returning the count if that block is a frame and 0
/// otherwise) **and** the block *below* it must be a frame. A walk that only
/// looked at the floor would run past the end of a wall.
///
/// The loop bound is `i <= 21`, so a 22-wide opening is measured as 22 and
/// then rejected by the range check rather than clipped to 21.
fn distance_until_edge_above_frame<F>(get: &F, from: BlockPos, dir: (i32, i32)) -> i32
where
    F: Fn(BlockPos) -> String,
{
    for i in 0..=MAX_WIDTH {
        let p = offset(from, dir, i);
        let state = get(p);
        if !is_empty(&state) {
            return if is_frame(&state) { i } else { 0 };
        }
        if !is_frame(&get(up(p, -1))) {
            return 0;
        }
    }
    0
}

/// `calculateHeight` — the walk, then the top row, then the range check.
fn calculate_height<F>(
    get: &F,
    bottom_left: BlockPos,
    right: (i32, i32),
    width: i32,
    portal_blocks: &mut i32,
) -> i32
where
    F: Fn(BlockPos) -> String,
{
    let h = distance_until_top(get, bottom_left, right, width, portal_blocks);
    if !(MIN_HEIGHT..=MAX_HEIGHT).contains(&h) || !has_top_frame(get, bottom_left, right, width, h)
    {
        return 0;
    }
    h
}

/// `getDistanceUntilTop`.
///
/// Per row: the block one step LEFT of the row must be a frame, then the block
/// `width` steps right of it must be a frame, then every position in between
/// must be empty. The counter is incremented inside that innermost loop, for
/// each position that is already a portal block — which is why a half-lit
/// frame has a count that is neither 0 nor `width * height`.
///
/// Reaching 21 rows without a stop returns 21, and `calculateHeight`'s range
/// check accepts it: a 21-tall opening is legal, and a 22-tall one is only
/// rejected later, by `hasTopFrame` failing on the 22nd row.
fn distance_until_top<F>(
    get: &F,
    bottom_left: BlockPos,
    right: (i32, i32),
    width: i32,
    portal_blocks: &mut i32,
) -> i32
where
    F: Fn(BlockPos) -> String,
{
    let left = (-right.0, -right.1);
    for row in 0..MAX_HEIGHT {
        let base = up(bottom_left, row);
        if !is_frame(&get(offset(base, left, 1))) {
            return row;
        }
        if !is_frame(&get(offset(base, right, width))) {
            return row;
        }
        for col in 0..width {
            let p = offset(base, right, col);
            let state = get(p);
            if !is_empty(&state) {
                return row;
            }
            if name_of(&state) == PORTAL_BLOCK {
                *portal_blocks += 1;
            }
        }
    }
    MAX_HEIGHT
}

/// `hasTopFrame` — the row at `height` above the bottom-left, `width` blocks
/// of it, all frame.
fn has_top_frame<F>(
    get: &F,
    bottom_left: BlockPos,
    right: (i32, i32),
    width: i32,
    height: i32,
) -> bool
where
    F: Fn(BlockPos) -> String,
{
    (0..width).all(|col| is_frame(&get(offset(up(bottom_left, height), right, col))))
}

/// `createPortalBlocks` — the positions a lit portal writes, in vanilla's own
/// order.
///
/// **Not a column-by-column walk along `rightDir`.** Vanilla builds the two
/// opposite corners — `bottomLeft` and
/// `bottomLeft.above(height - 1).relative(rightDir, width - 1)` — and hands
/// them to `BlockPos.betweenClosed`, which takes the component-wise min and
/// max and then walks the box with **x fastest, then y, then z**. For a
/// WEST-facing (X-axis) portal that runs the horizontal axis in the *opposite*
/// direction from the search that found the frame; for a SOUTH-facing one it
/// does not. Every position here is a block update a client sees, so the order
/// is behaviour: `portal_shape.json` carries the list for each valid frame.
///
/// Returned rather than written so the caller owns the world access.
#[must_use]
pub fn portal_block_positions(shape: &PortalShape) -> Vec<BlockPos> {
    if !shape.is_valid() {
        return Vec::new();
    }
    let right = shape.axis.right_step();
    let far = offset(
        up(shape.bottom_left, shape.height - 1),
        right,
        shape.width - 1,
    );
    let (a, b) = (shape.bottom_left, far);
    let (min_x, max_x) = (a.x.min(b.x), a.x.max(b.x));
    let (min_y, max_y) = (a.y.min(b.y), a.y.max(b.y));
    let (min_z, max_z) = (a.z.min(b.z), a.z.max(b.z));
    let mut out = Vec::new();
    for z in min_z..=max_z {
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                out.push(BlockPos { x, y, z });
            }
        }
    }
    out
}

/// The block state a lit portal's opening is filled with.
#[must_use]
pub fn portal_state(axis: Axis) -> String {
    format!("{PORTAL_BLOCK}[axis={}]", axis.name())
}
