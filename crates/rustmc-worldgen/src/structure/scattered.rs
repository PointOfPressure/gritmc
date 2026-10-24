//! `ScatteredFeaturePiece` and the three structures built from one: the swamp
//! hut, the jungle temple and the desert pyramid.
//!
//! Reference (26.2 bytecode, behaviour ported — never transcribed):
//! `ScatteredFeaturePiece`, `SwampHutPiece`, `JungleTemplePiece`,
//! `DesertPyramidPiece`, and the `StructurePiece` helpers they call.
//!
//! # One piece, seated on the first chunk that writes it
//!
//! Each of these is a single piece built at `(chunkMinX, 64, chunkMinZ)` with
//! a random horizontal facing. Its real height is decided in `postProcess`,
//! from the **live** `MOTION_BLOCKING_NO_LEAVES` map of the world being
//! decorated, and latched: `heightPosition` starts at -1, the first call that
//! finds ground sets it and moves the box, and every later call returns early
//! from the seating code. So the height is a function of which chunk the piece
//! is first written into, exactly like vanilla — whose chunk order is the
//! harvest's — and the latch lives in [`ScatteredState`] behind a mutex shared
//! by every chunk the piece reaches.
//!
//! The two seating rules differ and both are here:
//!
//! * `updateAverageGroundHeight` (hut, temple) averages the map over the
//!   columns of the piece that lie **inside this chunk's box**, at y 64, and
//!   refuses (no placement in this chunk, no latch) when there are none;
//! * `updateHeightPositionToLowestGroundHeight` (pyramid) takes the minimum
//!   over the **whole** piece and never refuses unless the piece has no
//!   columns at all.

use std::sync::Mutex;

use super::pieces::{BoundingBox, Facing, Piece};
use super::writer::{is_air, StructureWorld, Writer};
use crate::rng::RandomSource;

/// Which of the three.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    SwampHut,
    JungleTemple,
    DesertPyramid,
}

impl Kind {
    /// The piece id vanilla saves.
    #[must_use]
    pub const fn piece_id(self) -> &'static str {
        match self {
            Self::SwampHut => "minecraft:tesh",
            Self::JungleTemple => "minecraft:tejp",
            Self::DesertPyramid => "minecraft:tedp",
        }
    }

    /// `(width, height, depth)` as each constructor passes them.
    #[must_use]
    pub const fn size(self) -> (i32, i32, i32) {
        match self {
            Self::SwampHut => (7, 7, 9),
            Self::JungleTemple => (12, 10, 15),
            Self::DesertPyramid => (21, 15, 21),
        }
    }
}

/// The per-start mutable half: the latch and every once-only flag.
#[derive(Clone, Debug)]
pub struct ScatteredState {
    /// The piece, with the box `updateXGroundHeight` may already have moved.
    pub piece: Piece,
    /// `heightPosition`, -1 until latched.
    pub height_position: i32,
    /// `DesertPyramidPiece.hasPlacedChest`, by `get2DDataValue`.
    pub pyramid_chests: [bool; 4],
    /// `JungleTemplePiece.placedMainChest / placedHiddenChest /
    /// placedTrap1 / placedTrap2`.
    pub jungle_flags: [bool; 4],
    /// `DesertPyramidPiece.potentialSuspiciousSandWorldPositions`, appended
    /// to on every pass that reaches a sand cell, in call order.
    pub suspicious_sand: Vec<(i32, i32, i32)>,
    /// `DesertPyramidPiece.randomCollapsedRoofPos`.
    pub collapsed_roof: (i32, i32, i32),
    /// `SwampHutPiece.spawnedWitch` and `spawnedCat`.
    pub hut_spawned: [bool; 2],
}

/// One scattered start: its chunk, its kind and its latched state.
#[derive(Debug)]
pub struct ScatteredStart {
    pub kind: Kind,
    pub start_chunk: (i32, i32),
    /// The box as `generatePieces` built it — the y of this never survives
    /// the first `postProcess`, but the x/z decide which chunks hold a
    /// reference.
    pub initial: BoundingBox,
    pub state: Mutex<ScatteredState>,
}

/// `StructurePiece.getRandomHorizontalDirection` —
/// `Direction.Plane.HORIZONTAL` is `[NORTH, EAST, SOUTH, WEST]` and the pick is
/// one `nextInt(4)`.
pub fn random_horizontal<R: RandomSource + ?Sized>(random: &mut R) -> Facing {
    match random.next_i32_bound(4) {
        0 => Facing::North,
        1 => Facing::East,
        2 => Facing::South,
        _ => Facing::West,
    }
}

/// `StructurePiece.makeBoundingBox(x, y, z, direction, w, h, d)`.
#[must_use]
pub const fn make_bounding_box(
    x: i32,
    y: i32,
    z: i32,
    facing: Facing,
    width: i32,
    height: i32,
    depth: i32,
) -> BoundingBox {
    match facing {
        Facing::North | Facing::South => {
            BoundingBox::new(x, y, z, x + width - 1, y + height - 1, z + depth - 1)
        }
        Facing::West | Facing::East => {
            BoundingBox::new(x, y, z, x + depth - 1, y + height - 1, z + width - 1)
        }
    }
}

impl ScatteredStart {
    /// `new XPiece(random, chunkMinX, chunkMinZ)` — the one draw the layout
    /// spends, on the structure's own `setLargeFeatureSeed` random.
    pub fn new<R: RandomSource + ?Sized>(
        kind: Kind,
        start_chunk: (i32, i32),
        random: &mut R,
    ) -> Self {
        let (w, h, d) = kind.size();
        let facing = random_horizontal(random);
        let bb = make_bounding_box(start_chunk.0 * 16, 64, start_chunk.1 * 16, facing, w, h, d);
        let piece = Piece {
            id: kind.piece_id(),
            bounding_box: bb,
            facing: Some(facing),
            depth: 0,
        };
        Self {
            kind,
            start_chunk,
            initial: bb,
            state: Mutex::new(ScatteredState {
                piece,
                height_position: -1,
                pyramid_chests: [false; 4],
                hut_spawned: [false; 2],
                jungle_flags: [false; 4],
                suspicious_sand: Vec::new(),
                collapsed_roof: (0, 0, 0),
            }),
        }
    }
}

fn plain(state: &str) -> &str {
    state.split_once('[').map_or(state, |(n, _)| n)
}

/// `ScatteredFeaturePiece.updateAverageGroundHeight(level, box, offset)`.
///
/// The positions are `(x, 64, z)` over the piece's own box, and only those
/// `box.isInside` keeps are averaged — so the y of the test is 64 whatever
/// the chunk box is, and with the writable box spanning the world height
/// that is never the reason a column is dropped.
pub fn update_average_ground_height<W: StructureWorld + ?Sized>(
    world: &mut W,
    state: &mut ScatteredState,
    clip: BoundingBox,
    offset: i32,
) -> bool {
    if state.height_position >= 0 {
        return true;
    }
    let bb = state.piece.bounding_box;
    let (mut sum, mut count) = (0i32, 0i32);
    for z in bb.min_z..=bb.max_z {
        for x in bb.min_x..=bb.max_x {
            if !clip.is_inside(x, 64, z) {
                continue;
            }
            let Some(h) = world.motion_blocking_no_leaves_height(x, z) else {
                return false;
            };
            sum += h;
            count += 1;
        }
    }
    if count == 0 {
        return false;
    }
    state.height_position = sum / count;
    let dy = state.height_position - bb.min_y + offset;
    state.piece.bounding_box.shift(0, dy, 0);
    true
}

/// `ScatteredFeaturePiece.updateHeightPositionToLowestGroundHeight(level,
/// offset)` — the minimum over the whole piece, starting from
/// `getMaxY() + 1`.
pub fn update_height_position_to_lowest<W: StructureWorld + ?Sized>(
    world: &mut W,
    state: &mut ScatteredState,
    max_y: i32,
    offset: i32,
) -> bool {
    if state.height_position >= 0 {
        return true;
    }
    let bb = state.piece.bounding_box;
    let mut lowest = max_y + 1;
    let mut any = false;
    for z in bb.min_z..=bb.max_z {
        for x in bb.min_x..=bb.max_x {
            let Some(h) = world.motion_blocking_no_leaves_height(x, z) else {
                return false;
            };
            lowest = lowest.min(h);
            any = true;
        }
    }
    if !any {
        return false;
    }
    state.height_position = lowest;
    let dy = state.height_position - bb.min_y + offset;
    state.piece.bounding_box.shift(0, dy, 0);
    true
}

/// `StructurePiece.isReplaceableByStructures` — `isAir() || liquid() ||
/// GLOW_LICHEN || SEAGRASS || TALL_SEAGRASS`. `liquid()` is the block flag,
/// which `water`, `lava` and `bubble_column` carry.
#[must_use]
pub fn replaceable_by_structures(state: &str) -> bool {
    is_air(state)
        || matches!(
            plain(state),
            "minecraft:water"
                | "minecraft:lava"
                | "minecraft:bubble_column"
                | "minecraft:glow_lichen"
                | "minecraft:seagrass"
                | "minecraft:tall_seagrass"
        )
}

/// `StructurePiece.fillColumnDown` — from a piece-local position, write
/// `state` (unoriented: this is a raw `setBlock`) downwards while the world
/// holds something a structure may replace and the y is above
/// `getMinY() + 1`. Only the starting position is tested against the box.
pub fn fill_column_down<W: StructureWorld + ?Sized>(
    w: &mut Writer<'_, W>,
    state: &str,
    x: i32,
    y: i32,
    z: i32,
) {
    if !w.inside_box(x, y, z) {
        return;
    }
    let (wx, mut wy, wz) = w.piece.world_pos(x, y, z);
    let floor = w.world.min_y() + 1;
    while replaceable_by_structures(&w.world.block_at(wx, wy, wz)) && wy > floor {
        w.world.set_block(wx, wy, wz, state);
        wy -= 1;
    }
}

/// `Direction.Plane.HORIZONTAL` as `(name, dx, dz)`.
const HORIZONTAL: [(&str, i32, i32); 4] = [
    ("north", 0, -1),
    ("east", 1, 0),
    ("south", 0, 1),
    ("west", -1, 0),
];

/// `StructurePiece.reorient(level, pos, CHEST.defaultBlockState())` — the
/// facing a chest takes, or `None` for "a neighbouring chest returned the
/// state unchanged" (which is `facing=north`).
fn reorient_chest<W: StructureWorld + ?Sized>(
    world: &mut W,
    x: i32,
    y: i32,
    z: i32,
) -> Option<&'static str> {
    let mut found: Option<usize> = None;
    for (i, (_, dx, dz)) in HORIZONTAL.iter().enumerate() {
        let side = world.block_at(x + dx, y, z + dz);
        if plain(&side) == "minecraft:chest" {
            return None;
        }
        if world.state_is_solid_render(&side) {
            if found.is_none() {
                found = Some(i);
            } else {
                found = None;
                break;
            }
        }
    }
    let opposite = |d: usize| (d + 2) % 4;
    if let Some(i) = found {
        return Some(HORIZONTAL[opposite(i)].0);
    }
    let mut dir = 0usize;
    let solid_at = |world: &mut W, d: usize| {
        let (_, dx, dz) = HORIZONTAL[d];
        let s = world.block_at(x + dx, y, z + dz);
        world.state_is_solid_render(&s)
    };
    if solid_at(world, dir) {
        dir = opposite(dir);
    }
    if solid_at(world, dir) {
        dir = (dir + 1) % 4;
    }
    if solid_at(world, dir) {
        dir = opposite(dir);
    }
    Some(HORIZONTAL[dir].0)
}

/// `StructurePiece.createChest(level, box, random, x, y, z, lootTable)` with
/// no explicit state: inside the box and not already a chest, a reoriented
/// default chest goes in and `setLootTable(lootTable, random.nextLong())`
/// gives it its table and seed, recorded for the block-entity channel.
///
/// `chest` is the default state spelled canonically; its `facing` is replaced.
#[allow(clippy::too_many_arguments)]
pub fn create_chest<W: StructureWorld + ?Sized, R: RandomSource + ?Sized>(
    w: &mut Writer<'_, W>,
    random: &mut R,
    chest: &str,
    x: i32,
    y: i32,
    z: i32,
    loot_table: &str,
) -> bool {
    if !w.inside_box(x, y, z) {
        return false;
    }
    let (wx, wy, wz) = w.piece.world_pos(x, y, z);
    if plain(&w.world.block_at(wx, wy, wz)) == "minecraft:chest" {
        return false;
    }
    let state = match reorient_chest(w.world, wx, wy, wz) {
        Some(facing) => with_prop(chest, "facing", facing),
        None => chest.to_string(),
    };
    w.world.set_block(wx, wy, wz, &state);
    // `getBlockEntity(pos) instanceof ChestBlockEntity` holds in a
    // `WorldGenRegion`, so the seed is drawn.
    let seed = random.next_i64();
    w.world
        .record_block_entity(wx, wy, wz, loot_record(loot_table, seed));
    true
}

/// What `setLootTable(table, seed)` leaves on a container, for the channel.
#[must_use]
pub fn loot_record(table: &str, seed: i64) -> crate::block_entity::Record {
    crate::block_entity::Record {
        nbt: std::collections::BTreeMap::from([(
            "LootTable".to_string(),
            rustmc_world::nbt::Nbt::String(table.to_string()),
        )]),
        loot_seed: Some(seed),
    }
}

/// `BlockPos.asLong`: x in the top 26 bits, z in the next 26, y in the low
/// 12 (`X_OFFSET` 38, `Z_OFFSET` 12, `Y_OFFSET` 0).
#[must_use]
pub fn block_pos_as_long(x: i32, y: i32, z: i32) -> i64 {
    let (x, y, z) = (i64::from(x), i64::from(y), i64::from(z));
    ((x & 0x3FF_FFFF) << 38) | ((z & 0x3FF_FFFF) << 12) | (y & 0xFFF)
}

/// `state` with `key` set to `value`.
#[must_use]
pub fn with_prop(state: &str, key: &str, value: &str) -> String {
    let Some((name, rest)) = state.split_once('[') else {
        return format!("{state}[{key}={value}]");
    };
    let props: Vec<String> = rest
        .trim_end_matches(']')
        .split(',')
        .map(|p| match p.split_once('=') {
            Some((k, _)) if k == key => format!("{k}={value}"),
            _ => p.to_string(),
        })
        .collect();
    format!("{name}[{}]", props.join(","))
}

/// `SwampHutPiece.postProcess`. The witch and then the cat spawn once each,
/// at local (2,2,5) when that is inside this chunk's box, and draw from the
/// level random (`finalizeSpawn`), never from the structure's stream.
pub fn swamp_hut<W: StructureWorld + ?Sized>(
    world: &mut W,
    state: &mut ScatteredState,
    clip: BoundingBox,
    s: &dyn Fn(&str) -> String,
) {
    if !update_average_ground_height(world, state, clip, 0) {
        return;
    }
    let piece = state.piece.clone();
    let mut w = Writer {
        world,
        piece: &piece,
        clip,
        protected: &[],
    };
    let w = &mut w;
    let planks = s("minecraft:spruce_planks");
    let log = s("minecraft:oak_log");
    w.generate_box(1, 1, 1, 5, 1, 7, &planks, &planks, false);
    w.generate_box(1, 4, 2, 5, 4, 7, &planks, &planks, false);
    w.generate_box(2, 1, 0, 4, 1, 0, &planks, &planks, false);
    w.generate_box(2, 2, 2, 3, 3, 2, &planks, &planks, false);
    w.generate_box(1, 2, 3, 1, 3, 6, &planks, &planks, false);
    w.generate_box(5, 2, 3, 5, 3, 6, &planks, &planks, false);
    w.generate_box(2, 2, 7, 4, 3, 7, &planks, &planks, false);
    w.generate_box(1, 0, 2, 1, 3, 2, &log, &log, false);
    w.generate_box(5, 0, 2, 5, 3, 2, &log, &log, false);
    w.generate_box(1, 0, 7, 1, 3, 7, &log, &log, false);
    w.generate_box(5, 0, 7, 5, 3, 7, &log, &log, false);
    let fence = s("minecraft:oak_fence");
    let air = s("minecraft:air");
    w.place_block(&fence, 2, 3, 2);
    w.place_block(&fence, 3, 3, 7);
    w.place_block(&air, 1, 3, 4);
    w.place_block(&air, 5, 3, 4);
    w.place_block(&air, 5, 3, 5);
    w.place_block(&s("minecraft:potted_red_mushroom"), 1, 3, 5);
    w.place_block(&s("minecraft:crafting_table"), 3, 2, 6);
    w.place_block(&s("minecraft:cauldron"), 4, 2, 6);
    w.place_block(&fence, 1, 2, 1);
    w.place_block(&fence, 5, 2, 1);
    let north = s("minecraft:spruce_stairs[facing=north]");
    let east = s("minecraft:spruce_stairs[facing=east]");
    let west = s("minecraft:spruce_stairs[facing=west]");
    let south = s("minecraft:spruce_stairs[facing=south]");
    w.generate_box(0, 4, 1, 6, 4, 1, &north, &north, false);
    w.generate_box(0, 4, 2, 0, 4, 7, &east, &east, false);
    w.generate_box(6, 4, 2, 6, 4, 7, &west, &west, false);
    w.generate_box(0, 4, 8, 6, 4, 8, &south, &south, false);
    w.place_block(&with_prop(&north, "shape", "outer_right"), 0, 4, 1);
    w.place_block(&with_prop(&north, "shape", "outer_left"), 6, 4, 1);
    w.place_block(&with_prop(&south, "shape", "outer_left"), 0, 4, 8);
    w.place_block(&with_prop(&south, "shape", "outer_right"), 6, 4, 8);
    // Offsets 731..777: z from 2 to 7 step 5, x from 1 to 5 step 4.
    let mut z = 2;
    while z <= 7 {
        let mut x = 1;
        while x <= 5 {
            fill_column_down(w, &log, x, -1, z);
            x += 4;
        }
        z += 5;
    }
    // Offsets 781..905: `spawnedWitch`, then `spawnCat`.
    use crate::structure::spawned::{finalize_spawn, mob_compound, uuid, Mob};
    for (flag, mob) in [(0usize, Mob::Witch), (1, Mob::Cat)] {
        if state.hut_spawned[flag] || !w.inside_box(2, 2, 5) {
            continue;
        }
        state.hut_spawned[flag] = true;
        let at = w.piece.world_pos(2, 2, 5);
        let Some(level) = w.world.level_random() else {
            continue;
        };
        let finalized = finalize_spawn(mob, level);
        w.world
            .add_fresh_entity(mob_compound(mob, at, finalized, uuid(mob.id(), at, 0)));
    }
}

/// `StructurePiece.generateBox(.., skipAir=false, random,
/// JungleTemplePiece.MossStoneSelector)` — one `nextFloat` per position in
/// y, x, z order, **whether or not the position is inside the chunk box**:
/// the selector runs before `placeBlock` clips. Below 0.4 is cobblestone,
/// otherwise mossy cobblestone; the edge flag is computed and ignored.
#[expect(clippy::too_many_arguments, reason = "the reference's own signature")]
fn moss_box<W: StructureWorld + ?Sized, R: RandomSource + ?Sized>(
    w: &mut Writer<'_, W>,
    random: &mut R,
    cobble: &str,
    mossy: &str,
    x0: i32,
    y0: i32,
    z0: i32,
    x1: i32,
    y1: i32,
    z1: i32,
) {
    for y in y0..=y1 {
        for x in x0..=x1 {
            for z in z0..=z1 {
                let state = if random.next_f32() < 0.4 {
                    cobble
                } else {
                    mossy
                };
                w.place_block(state, x, y, z);
            }
        }
    }
}

/// `StructurePiece.createDispenser` — inside the box and not already a
/// dispenser: a dispenser facing `facing` (oriented by `placeBlock`), and the
/// loot seed costs one `nextLong`.
fn create_dispenser<W: StructureWorld + ?Sized, R: RandomSource + ?Sized>(
    w: &mut Writer<'_, W>,
    random: &mut R,
    dispenser: &str,
    x: i32,
    y: i32,
    z: i32,
    facing: &str,
) -> bool {
    if !w.inside_box(x, y, z) {
        return false;
    }
    if plain(&w.world_block(x, y, z)) == "minecraft:dispenser" {
        return false;
    }
    w.place_block(&with_prop(dispenser, "facing", facing), x, y, z);
    // `setLootTable(JUNGLE_TEMPLE_DISPENSER, random.nextLong())`; the jungle
    // temple is the only caller.
    let seed = random.next_i64();
    let (wx, wy, wz) = w.piece.world_pos(x, y, z);
    w.world.record_block_entity(
        wx,
        wy,
        wz,
        loot_record("minecraft:chests/jungle_temple_dispenser", seed),
    );
    true
}

/// `BuiltInLootTables.JUNGLE_TEMPLE`.
const JUNGLE_TEMPLE: &str = "minecraft:chests/jungle_temple";

/// `JungleTemplePiece.postProcess`.
#[expect(
    clippy::too_many_lines,
    reason = "one vanilla method, kept in its order"
)]
pub fn jungle_temple<W: StructureWorld + ?Sized, R: RandomSource + ?Sized>(
    world: &mut W,
    state: &mut ScatteredState,
    clip: BoundingBox,
    random: &mut R,
    s: &dyn Fn(&str) -> String,
) {
    if !update_average_ground_height(world, state, clip, 0) {
        return;
    }
    let piece = state.piece.clone();
    let (width, depth) = (12, 15);
    let flags = &mut state.jungle_flags;
    let mut w = Writer {
        world,
        piece: &piece,
        clip,
        protected: &[],
    };
    let w = &mut w;
    let c = s("minecraft:cobblestone");
    let m = s("minecraft:mossy_cobblestone");
    let air = s("minecraft:air");
    macro_rules! moss {
        ($x0:expr, $y0:expr, $z0:expr, $x1:expr, $y1:expr, $z1:expr) => {
            moss_box(w, random, &c, &m, $x0, $y0, $z0, $x1, $y1, $z1)
        };
    }
    moss!(0, -4, 0, width - 1, 0, depth - 1);
    moss!(2, 1, 2, 9, 2, 2);
    moss!(2, 1, 12, 9, 2, 12);
    moss!(2, 1, 3, 2, 2, 11);
    moss!(9, 1, 3, 9, 2, 11);
    moss!(1, 3, 1, 10, 6, 1);
    moss!(1, 3, 13, 10, 6, 13);
    moss!(1, 3, 2, 1, 6, 12);
    moss!(10, 3, 2, 10, 6, 12);
    moss!(2, 3, 2, 9, 3, 12);
    moss!(2, 6, 2, 9, 6, 12);
    moss!(3, 7, 3, 8, 7, 11);
    moss!(4, 8, 4, 7, 8, 10);
    w.generate_air_box(3, 1, 3, 8, 2, 11);
    w.generate_air_box(4, 3, 6, 7, 3, 9);
    w.generate_air_box(2, 4, 2, 9, 5, 12);
    w.generate_air_box(4, 6, 5, 7, 6, 9);
    w.generate_air_box(5, 7, 6, 6, 7, 8);
    w.generate_air_box(5, 1, 2, 6, 2, 2);
    w.generate_air_box(5, 2, 12, 6, 2, 12);
    w.generate_air_box(5, 5, 1, 6, 5, 1);
    w.generate_air_box(5, 5, 13, 6, 5, 13);
    w.place_block(&air, 1, 5, 5);
    w.place_block(&air, 10, 5, 5);
    w.place_block(&air, 1, 5, 9);
    w.place_block(&air, 10, 5, 9);
    // Offsets 513..614: z 0 and 14.
    let mut z = 0;
    while z <= 14 {
        moss!(2, 4, z, 2, 5, z);
        moss!(4, 4, z, 4, 5, z);
        moss!(7, 4, z, 7, 5, z);
        moss!(9, 4, z, 9, 5, z);
        z += 14;
    }
    moss!(5, 6, 0, 6, 6, 0);
    // Offsets 639..739: x 0 and 11; inner z 2..=12 step 2.
    let mut x = 0;
    while x <= 11 {
        let mut z = 2;
        while z <= 12 {
            moss!(x, 4, z, x, 5, z);
            z += 2;
        }
        moss!(x, 6, 5, x, 6, 5);
        moss!(x, 6, 9, x, 6, 9);
        x += 11;
    }
    moss!(2, 7, 2, 2, 9, 2);
    moss!(9, 7, 2, 9, 9, 2);
    moss!(2, 7, 12, 2, 9, 12);
    moss!(9, 7, 12, 9, 9, 12);
    moss!(4, 9, 4, 4, 9, 4);
    moss!(7, 9, 4, 7, 9, 4);
    moss!(4, 9, 10, 4, 9, 10);
    moss!(7, 9, 10, 7, 9, 10);
    moss!(5, 9, 7, 6, 9, 7);
    let north = s("minecraft:cobblestone_stairs[facing=north]");
    let south = s("minecraft:cobblestone_stairs[facing=south]");
    let east = s("minecraft:cobblestone_stairs[facing=east]");
    let west = s("minecraft:cobblestone_stairs[facing=west]");
    w.place_block(&north, 5, 9, 6);
    w.place_block(&north, 6, 9, 6);
    w.place_block(&south, 5, 9, 8);
    w.place_block(&south, 6, 9, 8);
    w.place_block(&north, 4, 0, 0);
    w.place_block(&north, 5, 0, 0);
    w.place_block(&north, 6, 0, 0);
    w.place_block(&north, 7, 0, 0);
    w.place_block(&north, 4, 1, 8);
    w.place_block(&north, 4, 2, 9);
    w.place_block(&north, 4, 3, 10);
    w.place_block(&north, 7, 1, 8);
    w.place_block(&north, 7, 2, 9);
    w.place_block(&north, 7, 3, 10);
    moss!(4, 1, 9, 4, 1, 9);
    moss!(7, 1, 9, 7, 1, 9);
    moss!(4, 1, 10, 7, 2, 10);
    moss!(5, 4, 5, 6, 4, 5);
    w.place_block(&east, 4, 4, 5);
    w.place_block(&west, 7, 4, 5);
    // Offsets 1330..1409.
    for i in 0..4 {
        w.place_block(&south, 5, -i, 6 + i);
        w.place_block(&south, 6, -i, 6 + i);
        w.generate_air_box(5, -i, 7 + i, 6, -i, 9 + i);
    }
    w.generate_air_box(1, -3, 12, 10, -1, 13);
    w.generate_air_box(1, -3, 1, 3, -1, 13);
    w.generate_air_box(1, -3, 1, 9, -1, 5);
    let mut z = 1;
    while z <= 13 {
        moss!(1, -3, z, 1, -2, z);
        z += 2;
    }
    let mut z = 2;
    while z <= 12 {
        moss!(1, -1, z, 3, -1, z);
        z += 2;
    }
    moss!(2, -2, 1, 5, -2, 1);
    moss!(7, -2, 1, 9, -2, 1);
    moss!(6, -3, 1, 6, -3, 1);
    moss!(6, -1, 1, 6, -1, 1);
    w.place_block(
        &s("minecraft:tripwire_hook[facing=east,attached=true]"),
        1,
        -3,
        8,
    );
    w.place_block(
        &s("minecraft:tripwire_hook[facing=west,attached=true]"),
        4,
        -3,
        8,
    );
    let wire_ew = s("minecraft:tripwire[east=true,west=true,attached=true]");
    w.place_block(&wire_ew, 2, -3, 8);
    w.place_block(&wire_ew, 3, -3, 8);
    let red_ns = s("minecraft:redstone_wire[north=side,south=side]");
    for z in [7, 6, 5, 4, 3, 2] {
        w.place_block(&red_ns, 5, -3, z);
    }
    w.place_block(
        &s("minecraft:redstone_wire[north=side,west=side]"),
        5,
        -3,
        1,
    );
    let red_ew = s("minecraft:redstone_wire[east=side,west=side]");
    w.place_block(&red_ew, 4, -3, 1);
    w.place_block(&m, 3, -3, 1);
    let dispenser = s("minecraft:dispenser");
    if !flags[2] {
        flags[2] = create_dispenser(w, random, &dispenser, 3, -2, 1, "north");
    }
    w.place_block(&s("minecraft:vine[south=true]"), 3, -2, 2);
    w.place_block(
        &s("minecraft:tripwire_hook[facing=north,attached=true]"),
        7,
        -3,
        1,
    );
    w.place_block(
        &s("minecraft:tripwire_hook[facing=south,attached=true]"),
        7,
        -3,
        5,
    );
    let wire_ns = s("minecraft:tripwire[north=true,south=true,attached=true]");
    w.place_block(&wire_ns, 7, -3, 2);
    w.place_block(&wire_ns, 7, -3, 3);
    w.place_block(&wire_ns, 7, -3, 4);
    w.place_block(&red_ew, 8, -3, 6);
    w.place_block(
        &s("minecraft:redstone_wire[west=side,south=side]"),
        9,
        -3,
        6,
    );
    w.place_block(&s("minecraft:redstone_wire[north=side,south=up]"), 9, -3, 5);
    w.place_block(&m, 9, -3, 4);
    w.place_block(&red_ns, 9, -2, 4);
    if !flags[3] {
        flags[3] = create_dispenser(w, random, &dispenser, 9, -2, 3, "west");
    }
    let vine_e = s("minecraft:vine[east=true]");
    w.place_block(&vine_e, 8, -1, 3);
    w.place_block(&vine_e, 8, -2, 3);
    let chest = s("minecraft:chest");
    if !flags[0] {
        flags[0] = create_chest(w, random, &chest, 8, -3, 3, JUNGLE_TEMPLE);
    }
    w.place_block(&m, 9, -3, 2);
    w.place_block(&m, 8, -3, 1);
    w.place_block(&m, 4, -3, 5);
    w.place_block(&m, 5, -2, 5);
    w.place_block(&m, 5, -1, 5);
    w.place_block(&m, 6, -3, 5);
    w.place_block(&m, 7, -2, 5);
    w.place_block(&m, 7, -1, 5);
    w.place_block(&m, 8, -3, 5);
    moss!(9, -1, 1, 9, -1, 5);
    w.generate_air_box(8, -3, 8, 10, -1, 10);
    let chiseled = s("minecraft:chiseled_stone_bricks");
    w.place_block(&chiseled, 8, -2, 11);
    w.place_block(&chiseled, 9, -2, 11);
    w.place_block(&chiseled, 10, -2, 11);
    let lever = s("minecraft:lever[facing=north,face=wall]");
    w.place_block(&lever, 8, -2, 12);
    w.place_block(&lever, 9, -2, 12);
    w.place_block(&lever, 10, -2, 12);
    moss!(8, -3, 8, 8, -3, 10);
    moss!(10, -3, 8, 10, -3, 10);
    w.place_block(&m, 10, -2, 9);
    w.place_block(&red_ns, 8, -2, 9);
    w.place_block(&red_ns, 8, -2, 10);
    w.place_block(
        &s("minecraft:redstone_wire[north=side,south=side,east=side,west=side]"),
        10,
        -1,
        9,
    );
    w.place_block(&s("minecraft:sticky_piston[facing=up]"), 9, -2, 8);
    let piston_w = s("minecraft:sticky_piston[facing=west]");
    w.place_block(&piston_w, 10, -2, 8);
    w.place_block(&piston_w, 10, -1, 8);
    w.place_block(&s("minecraft:repeater[facing=north]"), 10, -2, 10);
    if !flags[1] {
        flags[1] = create_chest(w, random, &chest, 9, -3, 10, JUNGLE_TEMPLE);
    }
}

/// `DesertPyramidPiece.WIDTH` and `DEPTH`.
const WIDTH: i32 = 21;
const DEPTH: i32 = 21;

/// `DesertPyramidPiece.postProcess`, then `DesertPyramidStructure.afterPlace`
/// for the same chunk — `StructureStart.placeInChunk` runs the structure's
/// `afterPlace` once after its pieces, with the same box.
///
/// Three randoms, and only one of them is the structure's stream:
///
/// * `random` (the per-structure `setFeatureSeed` stream): the seating
///   offset `nextInt(3)`, drawn on **every** pass before the latch is
///   consulted, and one `nextLong` per chest placed;
/// * the `WorldGenRegion`'s own random (`level.getRandom()`): the cellar
///   stair's sand-or-sandstone coin and one `nextFloat` per collapsed-roof
///   cell — so [`StructureWorld::level_random_next_bool`] must be live or the
///   pyramid declines to place;
/// * positional legacy streams off the world seed for the roof cell and the
///   suspicious-sand shuffle.
#[expect(
    clippy::too_many_lines,
    reason = "one vanilla method, kept in its order"
)]
pub fn desert_pyramid<W: StructureWorld + ?Sized, R: RandomSource + ?Sized>(
    world: &mut W,
    state: &mut ScatteredState,
    clip: BoundingBox,
    random: &mut R,
    world_seed: i64,
    s: &dyn Fn(&str) -> String,
) {
    let offset = -random.next_i32_bound(3);
    let max_y = clip.max_y;
    if !update_height_position_to_lowest(world, state, max_y, offset) {
        return;
    }
    let piece = state.piece.clone();
    {
        let mut w = Writer {
            world: &mut *world,
            piece: &piece,
            clip,
            protected: &[],
        };
        let w = &mut w;
        w.generate_box(
            0,
            -4,
            0,
            WIDTH - 1,
            0,
            DEPTH - 1,
            &s("minecraft:sandstone"),
            &s("minecraft:sandstone"),
            false,
        );
        for i8 in 1..=9 {
            w.generate_box(
                i8,
                i8,
                i8,
                (WIDTH - 1) - i8,
                i8,
                (DEPTH - 1) - i8,
                &s("minecraft:sandstone"),
                &s("minecraft:sandstone"),
                false,
            );
            w.generate_box(
                i8 + 1,
                i8,
                i8 + 1,
                (WIDTH - 2) - i8,
                i8,
                (DEPTH - 2) - i8,
                &s("minecraft:air"),
                &s("minecraft:air"),
                false,
            );
        }
        for i8 in 0..WIDTH {
            for i9 in 0..DEPTH {
                fill_column_down(w, &s("minecraft:sandstone"), i8, -5, i9);
            }
        }
        w.generate_box(
            0,
            0,
            0,
            4,
            9,
            4,
            &s("minecraft:sandstone"),
            &s("minecraft:air"),
            false,
        );
        w.generate_box(
            1,
            10,
            1,
            3,
            10,
            3,
            &s("minecraft:sandstone"),
            &s("minecraft:sandstone"),
            false,
        );
        w.place_block(&s("minecraft:sandstone_stairs[facing=north]"), 2, 10, 0);
        w.place_block(&s("minecraft:sandstone_stairs[facing=south]"), 2, 10, 4);
        w.place_block(&s("minecraft:sandstone_stairs[facing=east]"), 0, 10, 2);
        w.place_block(&s("minecraft:sandstone_stairs[facing=west]"), 4, 10, 2);
        w.generate_box(
            WIDTH - 5,
            0,
            0,
            WIDTH - 1,
            9,
            4,
            &s("minecraft:sandstone"),
            &s("minecraft:air"),
            false,
        );
        w.generate_box(
            WIDTH - 4,
            10,
            1,
            WIDTH - 2,
            10,
            3,
            &s("minecraft:sandstone"),
            &s("minecraft:sandstone"),
            false,
        );
        w.place_block(
            &s("minecraft:sandstone_stairs[facing=north]"),
            WIDTH - 3,
            10,
            0,
        );
        w.place_block(
            &s("minecraft:sandstone_stairs[facing=south]"),
            WIDTH - 3,
            10,
            4,
        );
        w.place_block(
            &s("minecraft:sandstone_stairs[facing=east]"),
            WIDTH - 5,
            10,
            2,
        );
        w.place_block(
            &s("minecraft:sandstone_stairs[facing=west]"),
            WIDTH - 1,
            10,
            2,
        );
        w.generate_box(
            8,
            0,
            0,
            12,
            4,
            4,
            &s("minecraft:sandstone"),
            &s("minecraft:air"),
            false,
        );
        w.generate_box(
            9,
            1,
            0,
            11,
            3,
            4,
            &s("minecraft:air"),
            &s("minecraft:air"),
            false,
        );
        w.place_block(&s("minecraft:cut_sandstone"), 9, 1, 1);
        w.place_block(&s("minecraft:cut_sandstone"), 9, 2, 1);
        w.place_block(&s("minecraft:cut_sandstone"), 9, 3, 1);
        w.place_block(&s("minecraft:cut_sandstone"), 10, 3, 1);
        w.place_block(&s("minecraft:cut_sandstone"), 11, 3, 1);
        w.place_block(&s("minecraft:cut_sandstone"), 11, 2, 1);
        w.place_block(&s("minecraft:cut_sandstone"), 11, 1, 1);
        w.generate_box(
            4,
            1,
            1,
            8,
            3,
            3,
            &s("minecraft:sandstone"),
            &s("minecraft:air"),
            false,
        );
        w.generate_box(
            4,
            1,
            2,
            8,
            2,
            2,
            &s("minecraft:air"),
            &s("minecraft:air"),
            false,
        );
        w.generate_box(
            12,
            1,
            1,
            16,
            3,
            3,
            &s("minecraft:sandstone"),
            &s("minecraft:air"),
            false,
        );
        w.generate_box(
            12,
            1,
            2,
            16,
            2,
            2,
            &s("minecraft:air"),
            &s("minecraft:air"),
            false,
        );
        w.generate_box(
            5,
            4,
            5,
            WIDTH - 6,
            4,
            DEPTH - 6,
            &s("minecraft:sandstone"),
            &s("minecraft:sandstone"),
            false,
        );
        w.generate_box(
            9,
            4,
            9,
            11,
            4,
            11,
            &s("minecraft:air"),
            &s("minecraft:air"),
            false,
        );
        w.generate_box(
            8,
            1,
            8,
            8,
            3,
            8,
            &s("minecraft:cut_sandstone"),
            &s("minecraft:cut_sandstone"),
            false,
        );
        w.generate_box(
            12,
            1,
            8,
            12,
            3,
            8,
            &s("minecraft:cut_sandstone"),
            &s("minecraft:cut_sandstone"),
            false,
        );
        w.generate_box(
            8,
            1,
            12,
            8,
            3,
            12,
            &s("minecraft:cut_sandstone"),
            &s("minecraft:cut_sandstone"),
            false,
        );
        w.generate_box(
            12,
            1,
            12,
            12,
            3,
            12,
            &s("minecraft:cut_sandstone"),
            &s("minecraft:cut_sandstone"),
            false,
        );
        w.generate_box(
            1,
            1,
            5,
            4,
            4,
            11,
            &s("minecraft:sandstone"),
            &s("minecraft:sandstone"),
            false,
        );
        w.generate_box(
            WIDTH - 5,
            1,
            5,
            WIDTH - 2,
            4,
            11,
            &s("minecraft:sandstone"),
            &s("minecraft:sandstone"),
            false,
        );
        w.generate_box(
            6,
            7,
            9,
            6,
            7,
            11,
            &s("minecraft:sandstone"),
            &s("minecraft:sandstone"),
            false,
        );
        w.generate_box(
            WIDTH - 7,
            7,
            9,
            WIDTH - 7,
            7,
            11,
            &s("minecraft:sandstone"),
            &s("minecraft:sandstone"),
            false,
        );
        w.generate_box(
            5,
            5,
            9,
            5,
            7,
            11,
            &s("minecraft:cut_sandstone"),
            &s("minecraft:cut_sandstone"),
            false,
        );
        w.generate_box(
            WIDTH - 6,
            5,
            9,
            WIDTH - 6,
            7,
            11,
            &s("minecraft:cut_sandstone"),
            &s("minecraft:cut_sandstone"),
            false,
        );
        w.place_block(&s("minecraft:air"), 5, 5, 10);
        w.place_block(&s("minecraft:air"), 5, 6, 10);
        w.place_block(&s("minecraft:air"), 6, 6, 10);
        w.place_block(&s("minecraft:air"), WIDTH - 6, 5, 10);
        w.place_block(&s("minecraft:air"), WIDTH - 6, 6, 10);
        w.place_block(&s("minecraft:air"), WIDTH - 7, 6, 10);
        w.generate_box(
            2,
            4,
            4,
            2,
            6,
            4,
            &s("minecraft:air"),
            &s("minecraft:air"),
            false,
        );
        w.generate_box(
            WIDTH - 3,
            4,
            4,
            WIDTH - 3,
            6,
            4,
            &s("minecraft:air"),
            &s("minecraft:air"),
            false,
        );
        w.place_block(&s("minecraft:sandstone_stairs[facing=north]"), 2, 4, 5);
        w.place_block(&s("minecraft:sandstone_stairs[facing=north]"), 2, 3, 4);
        w.place_block(
            &s("minecraft:sandstone_stairs[facing=north]"),
            WIDTH - 3,
            4,
            5,
        );
        w.place_block(
            &s("minecraft:sandstone_stairs[facing=north]"),
            WIDTH - 3,
            3,
            4,
        );
        w.generate_box(
            1,
            1,
            3,
            2,
            2,
            3,
            &s("minecraft:sandstone"),
            &s("minecraft:sandstone"),
            false,
        );
        w.generate_box(
            WIDTH - 3,
            1,
            3,
            WIDTH - 2,
            2,
            3,
            &s("minecraft:sandstone"),
            &s("minecraft:sandstone"),
            false,
        );
        w.place_block(&s("minecraft:sandstone"), 1, 1, 2);
        w.place_block(&s("minecraft:sandstone"), WIDTH - 2, 1, 2);
        w.place_block(&s("minecraft:sandstone_slab"), 1, 2, 2);
        w.place_block(&s("minecraft:sandstone_slab"), WIDTH - 2, 2, 2);
        w.place_block(&s("minecraft:sandstone_stairs[facing=west]"), 2, 1, 2);
        w.place_block(
            &s("minecraft:sandstone_stairs[facing=east]"),
            WIDTH - 3,
            1,
            2,
        );
        w.generate_box(
            4,
            3,
            5,
            4,
            3,
            17,
            &s("minecraft:sandstone"),
            &s("minecraft:sandstone"),
            false,
        );
        w.generate_box(
            WIDTH - 5,
            3,
            5,
            WIDTH - 5,
            3,
            17,
            &s("minecraft:sandstone"),
            &s("minecraft:sandstone"),
            false,
        );
        w.generate_box(
            3,
            1,
            5,
            4,
            2,
            16,
            &s("minecraft:air"),
            &s("minecraft:air"),
            false,
        );
        w.generate_box(
            WIDTH - 6,
            1,
            5,
            WIDTH - 5,
            2,
            16,
            &s("minecraft:air"),
            &s("minecraft:air"),
            false,
        );
        let mut i12 = 5;
        while i12 <= 17 {
            w.place_block(&s("minecraft:cut_sandstone"), 4, 1, i12);
            w.place_block(&s("minecraft:chiseled_sandstone"), 4, 2, i12);
            w.place_block(&s("minecraft:cut_sandstone"), WIDTH - 5, 1, i12);
            w.place_block(&s("minecraft:chiseled_sandstone"), WIDTH - 5, 2, i12);
            i12 += 2;
        }
        w.place_block(&s("minecraft:orange_terracotta"), 10, 0, 7);
        w.place_block(&s("minecraft:orange_terracotta"), 10, 0, 8);
        w.place_block(&s("minecraft:orange_terracotta"), 9, 0, 9);
        w.place_block(&s("minecraft:orange_terracotta"), 11, 0, 9);
        w.place_block(&s("minecraft:orange_terracotta"), 8, 0, 10);
        w.place_block(&s("minecraft:orange_terracotta"), 12, 0, 10);
        w.place_block(&s("minecraft:orange_terracotta"), 7, 0, 10);
        w.place_block(&s("minecraft:orange_terracotta"), 13, 0, 10);
        w.place_block(&s("minecraft:orange_terracotta"), 9, 0, 11);
        w.place_block(&s("minecraft:orange_terracotta"), 11, 0, 11);
        w.place_block(&s("minecraft:orange_terracotta"), 10, 0, 12);
        w.place_block(&s("minecraft:orange_terracotta"), 10, 0, 13);
        w.place_block(&s("minecraft:blue_terracotta"), 10, 0, 10);
        let mut i12 = 0;
        while i12 < WIDTH {
            w.place_block(&s("minecraft:cut_sandstone"), i12, 2, 1);
            w.place_block(&s("minecraft:orange_terracotta"), i12, 2, 2);
            w.place_block(&s("minecraft:cut_sandstone"), i12, 2, 3);
            w.place_block(&s("minecraft:cut_sandstone"), i12, 3, 1);
            w.place_block(&s("minecraft:orange_terracotta"), i12, 3, 2);
            w.place_block(&s("minecraft:cut_sandstone"), i12, 3, 3);
            w.place_block(&s("minecraft:orange_terracotta"), i12, 4, 1);
            w.place_block(&s("minecraft:chiseled_sandstone"), i12, 4, 2);
            w.place_block(&s("minecraft:orange_terracotta"), i12, 4, 3);
            w.place_block(&s("minecraft:cut_sandstone"), i12, 5, 1);
            w.place_block(&s("minecraft:orange_terracotta"), i12, 5, 2);
            w.place_block(&s("minecraft:cut_sandstone"), i12, 5, 3);
            w.place_block(&s("minecraft:orange_terracotta"), i12, 6, 1);
            w.place_block(&s("minecraft:chiseled_sandstone"), i12, 6, 2);
            w.place_block(&s("minecraft:orange_terracotta"), i12, 6, 3);
            w.place_block(&s("minecraft:orange_terracotta"), i12, 7, 1);
            w.place_block(&s("minecraft:orange_terracotta"), i12, 7, 2);
            w.place_block(&s("minecraft:orange_terracotta"), i12, 7, 3);
            w.place_block(&s("minecraft:cut_sandstone"), i12, 8, 1);
            w.place_block(&s("minecraft:cut_sandstone"), i12, 8, 2);
            w.place_block(&s("minecraft:cut_sandstone"), i12, 8, 3);
            i12 += WIDTH - 1;
        }
        let mut i12 = 2;
        while i12 <= WIDTH - 3 {
            w.place_block(&s("minecraft:cut_sandstone"), i12 - 1, 2, 0);
            w.place_block(&s("minecraft:orange_terracotta"), i12, 2, 0);
            w.place_block(&s("minecraft:cut_sandstone"), i12 + 1, 2, 0);
            w.place_block(&s("minecraft:cut_sandstone"), i12 - 1, 3, 0);
            w.place_block(&s("minecraft:orange_terracotta"), i12, 3, 0);
            w.place_block(&s("minecraft:cut_sandstone"), i12 + 1, 3, 0);
            w.place_block(&s("minecraft:orange_terracotta"), i12 - 1, 4, 0);
            w.place_block(&s("minecraft:chiseled_sandstone"), i12, 4, 0);
            w.place_block(&s("minecraft:orange_terracotta"), i12 + 1, 4, 0);
            w.place_block(&s("minecraft:cut_sandstone"), i12 - 1, 5, 0);
            w.place_block(&s("minecraft:orange_terracotta"), i12, 5, 0);
            w.place_block(&s("minecraft:cut_sandstone"), i12 + 1, 5, 0);
            w.place_block(&s("minecraft:orange_terracotta"), i12 - 1, 6, 0);
            w.place_block(&s("minecraft:chiseled_sandstone"), i12, 6, 0);
            w.place_block(&s("minecraft:orange_terracotta"), i12 + 1, 6, 0);
            w.place_block(&s("minecraft:orange_terracotta"), i12 - 1, 7, 0);
            w.place_block(&s("minecraft:orange_terracotta"), i12, 7, 0);
            w.place_block(&s("minecraft:orange_terracotta"), i12 + 1, 7, 0);
            w.place_block(&s("minecraft:cut_sandstone"), i12 - 1, 8, 0);
            w.place_block(&s("minecraft:cut_sandstone"), i12, 8, 0);
            w.place_block(&s("minecraft:cut_sandstone"), i12 + 1, 8, 0);
            i12 += WIDTH - 3 - 2;
        }
        w.generate_box(
            8,
            4,
            0,
            12,
            6,
            0,
            &s("minecraft:cut_sandstone"),
            &s("minecraft:cut_sandstone"),
            false,
        );
        w.place_block(&s("minecraft:air"), 8, 6, 0);
        w.place_block(&s("minecraft:air"), 12, 6, 0);
        w.place_block(&s("minecraft:orange_terracotta"), 9, 5, 0);
        w.place_block(&s("minecraft:chiseled_sandstone"), 10, 5, 0);
        w.place_block(&s("minecraft:orange_terracotta"), 11, 5, 0);
        w.generate_box(
            8,
            -14,
            8,
            12,
            -11,
            12,
            &s("minecraft:cut_sandstone"),
            &s("minecraft:cut_sandstone"),
            false,
        );
        w.generate_box(
            8,
            -10,
            8,
            12,
            -10,
            12,
            &s("minecraft:chiseled_sandstone"),
            &s("minecraft:chiseled_sandstone"),
            false,
        );
        w.generate_box(
            8,
            -9,
            8,
            12,
            -9,
            12,
            &s("minecraft:cut_sandstone"),
            &s("minecraft:cut_sandstone"),
            false,
        );
        w.generate_box(
            8,
            -8,
            8,
            12,
            -1,
            12,
            &s("minecraft:sandstone"),
            &s("minecraft:sandstone"),
            false,
        );
        w.generate_box(
            9,
            -11,
            9,
            11,
            -1,
            11,
            &s("minecraft:air"),
            &s("minecraft:air"),
            false,
        );
        w.place_block(&s("minecraft:stone_pressure_plate"), 10, -11, 10);
        w.generate_box(
            9,
            -13,
            9,
            11,
            -13,
            11,
            &s("minecraft:tnt"),
            &s("minecraft:air"),
            false,
        );
        w.place_block(&s("minecraft:air"), 8, -11, 10);
        w.place_block(&s("minecraft:air"), 8, -10, 10);
        w.place_block(&s("minecraft:chiseled_sandstone"), 7, -10, 10);
        w.place_block(&s("minecraft:cut_sandstone"), 7, -11, 10);
        w.place_block(&s("minecraft:air"), 12, -11, 10);
        w.place_block(&s("minecraft:air"), 12, -10, 10);
        w.place_block(&s("minecraft:chiseled_sandstone"), 13, -10, 10);
        w.place_block(&s("minecraft:cut_sandstone"), 13, -11, 10);
        w.place_block(&s("minecraft:air"), 10, -11, 8);
        w.place_block(&s("minecraft:air"), 10, -10, 8);
        w.place_block(&s("minecraft:chiseled_sandstone"), 10, -10, 7);
        w.place_block(&s("minecraft:cut_sandstone"), 10, -11, 7);
        w.place_block(&s("minecraft:air"), 10, -11, 12);
        w.place_block(&s("minecraft:air"), 10, -10, 12);
        w.place_block(&s("minecraft:chiseled_sandstone"), 10, -10, 13);
        w.place_block(&s("minecraft:cut_sandstone"), 10, -11, 13); // Offsets 3767..3862: `Direction.Plane.HORIZONTAL`, a chest two out
                                                                   // from the centre each way, once per direction for the life of the
                                                                   // start.
        let chest = s("minecraft:chest");
        for (data2d, dx, dz) in [(2usize, 0, -1), (3, 1, 0), (0, 0, 1), (1, -1, 0)] {
            if !state.pyramid_chests[data2d] {
                state.pyramid_chests[data2d] = create_chest(
                    w,
                    random,
                    &chest,
                    10 + dx * 2,
                    -11,
                    10 + dz * 2,
                    "minecraft:chests/desert_pyramid",
                );
            }
        }
        if !add_cellar(w, state, world_seed, s) {
            return;
        }
    }
    after_place(world, state, clip, world_seed, s);
}

/// `addCellar` — `addCellarStairs` then `addCellarRoom`, both at local
/// `(16, -4, 13)`. `false` when the world has no level random to give.
fn add_cellar<W: StructureWorld + ?Sized>(
    w: &mut Writer<'_, W>,
    state: &mut ScatteredState,
    world_seed: i64,
    s: &dyn Fn(&str) -> String,
) -> bool {
    let (x, y, z) = (16, -4, 13);
    // addCellarStairs.
    let stairs = s("minecraft:sandstone_stairs[facing=west]");
    w.place_block(&stairs, 13, -1, 17);
    w.place_block(&stairs, 14, -2, 17);
    w.place_block(&stairs, 15, -3, 17);
    let sand = s("minecraft:sand");
    let sandstone = s("minecraft:sandstone");
    let Some(coin) = w.world.level_random_next_bool() else {
        return false;
    };
    w.place_block(&sand, x - 4, y + 4, z + 4);
    w.place_block(&sand, x - 3, y + 4, z + 4);
    w.place_block(&sand, x - 2, y + 4, z + 4);
    w.place_block(&sand, x - 1, y + 4, z + 4);
    w.place_block(&sand, x, y + 4, z + 4);
    w.place_block(&sand, x - 2, y + 3, z + 4);
    w.place_block(if coin { &sand } else { &sandstone }, x - 1, y + 3, z + 4);
    w.place_block(if coin { &sandstone } else { &sand }, x, y + 3, z + 4);
    w.place_block(&sand, x - 1, y + 2, z + 4);
    w.place_block(&sandstone, x, y + 2, z + 4);
    w.place_block(&sand, x, y + 1, z + 4);
    // addCellarRoom.
    let cut = s("minecraft:cut_sandstone");
    let chiseled = s("minecraft:chiseled_sandstone");
    w.generate_box(x - 3, y + 1, z - 3, x - 3, y + 1, z + 2, &cut, &cut, true);
    w.generate_box(x + 3, y + 1, z - 3, x + 3, y + 1, z + 2, &cut, &cut, true);
    w.generate_box(x - 3, y + 1, z - 3, x + 3, y + 1, z - 2, &cut, &cut, true);
    w.generate_box(x - 3, y + 1, z + 3, x + 3, y + 1, z + 3, &cut, &cut, true);
    w.generate_box(
        x - 3,
        y + 2,
        z - 3,
        x - 3,
        y + 2,
        z + 2,
        &chiseled,
        &chiseled,
        true,
    );
    w.generate_box(
        x + 3,
        y + 2,
        z - 3,
        x + 3,
        y + 2,
        z + 2,
        &chiseled,
        &chiseled,
        true,
    );
    w.generate_box(
        x - 3,
        y + 2,
        z - 3,
        x + 3,
        y + 2,
        z - 2,
        &chiseled,
        &chiseled,
        true,
    );
    w.generate_box(
        x - 3,
        y + 2,
        z + 3,
        x + 3,
        y + 2,
        z + 3,
        &chiseled,
        &chiseled,
        true,
    );
    w.generate_box(x - 3, y + 3, z - 3, x - 3, y + 3, z + 2, &cut, &cut, true);
    w.generate_box(x + 3, y + 3, z - 3, x + 3, y + 3, z + 2, &cut, &cut, true);
    w.generate_box(x - 3, y + 3, z - 3, x + 3, y + 3, z - 2, &cut, &cut, true);
    w.generate_box(x - 3, y + 3, z + 3, x + 3, y + 3, z + 3, &cut, &cut, true);
    place_sand_box(w, state, x - 2, y + 1, z - 2, x + 2, y + 3, z + 2);
    if !place_collapsed_roof(w, state, world_seed, x - 2, y + 4, z - 2, x + 2, z + 2, s) {
        return false;
    }
    let orange = s("minecraft:orange_terracotta");
    w.place_block(&s("minecraft:blue_terracotta"), x, y, z);
    w.place_block(&orange, x + 1, y, z - 1);
    w.place_block(&orange, x + 1, y, z + 1);
    w.place_block(&orange, x - 1, y, z - 1);
    w.place_block(&orange, x - 1, y, z + 1);
    w.place_block(&orange, x + 2, y, z);
    w.place_block(&orange, x - 2, y, z);
    w.place_block(&orange, x, y, z + 2);
    w.place_block(&orange, x, y, z - 2);
    w.place_block(&orange, x + 3, y, z);
    place_sand(w, state, x + 3, y + 1, z);
    place_sand(w, state, x + 3, y + 2, z);
    w.place_block(&cut, x + 4, y + 1, z);
    w.place_block(&chiseled, x + 4, y + 2, z);
    w.place_block(&orange, x - 3, y, z);
    place_sand(w, state, x - 3, y + 1, z);
    place_sand(w, state, x - 3, y + 2, z);
    w.place_block(&cut, x - 4, y + 1, z);
    w.place_block(&chiseled, x - 4, y + 2, z);
    w.place_block(&orange, x, y, z + 3);
    place_sand(w, state, x, y + 1, z + 3);
    place_sand(w, state, x, y + 2, z + 3);
    w.place_block(&orange, x, y, z - 3);
    place_sand(w, state, x, y + 1, z - 3);
    place_sand(w, state, x, y + 2, z - 3);
    w.place_block(&cut, x, y + 1, z - 4);
    w.place_block(&chiseled, x, y + 2, z - 4);
    true
}

/// `placeSand` — records the world position and writes nothing; the block is
/// decided in `afterPlace`.
fn place_sand<W: StructureWorld + ?Sized>(
    w: &Writer<'_, W>,
    state: &mut ScatteredState,
    x: i32,
    y: i32,
    z: i32,
) {
    state.suspicious_sand.push(w.piece.world_pos(x, y, z));
}

/// `placeSandBox` — y outermost, then x, then z.
#[expect(clippy::too_many_arguments, reason = "the reference's own signature")]
fn place_sand_box<W: StructureWorld + ?Sized>(
    w: &Writer<'_, W>,
    state: &mut ScatteredState,
    x0: i32,
    y0: i32,
    z0: i32,
    x1: i32,
    y1: i32,
    z1: i32,
) {
    for y in y0..=y1 {
        for x in x0..=x1 {
            for z in z0..=z1 {
                place_sand(w, state, x, y, z);
            }
        }
    }
}

/// `placeCollapsedRoof` — a 0.33 sandstone-else-sand coin per cell off the
/// level random, then one cell picked by a positional stream at the corner.
#[expect(clippy::too_many_arguments, reason = "the reference's own signature")]
fn place_collapsed_roof<W: StructureWorld + ?Sized>(
    w: &mut Writer<'_, W>,
    state: &mut ScatteredState,
    world_seed: i64,
    x0: i32,
    y: i32,
    z0: i32,
    x1: i32,
    z1: i32,
    s: &dyn Fn(&str) -> String,
) -> bool {
    use crate::rng::PositionalRandomFactory;
    let sandstone = s("minecraft:sandstone");
    let sand = s("minecraft:sand");
    for x in x0..=x1 {
        for z in z0..=z1 {
            let Some(roll) = w.world.level_random_next_f32() else {
                return false;
            };
            w.place_block(if roll < 0.33 { &sandstone } else { &sand }, x, y, z);
        }
    }
    let (cx, cy, cz) = w.piece.world_pos(x0, y, z0);
    let mut positional = crate::rng::legacy::LegacyRandomSource::new(world_seed)
        .fork_positional()
        .at(cx, cy, cz);
    let rx = x0 + positional.next_i32_bound(x1 - x0 + 1);
    let rz = z0 + positional.next_i32_bound(z1 - z0 + 1);
    state.collapsed_roof = w.piece.world_pos(rx, y, rz);
    true
}

/// `DesertPyramidStructure.afterPlace` for one chunk.
fn after_place<W: StructureWorld + ?Sized>(
    world: &mut W,
    state: &mut ScatteredState,
    clip: BoundingBox,
    world_seed: i64,
    s: &dyn Fn(&str) -> String,
) {
    use crate::rng::PositionalRandomFactory;
    // `SortedArraySet` under `Vec3i.compareTo`: y, then z, then x.
    let mut set: Vec<(i32, i32, i32)> = state.suspicious_sand.clone();
    set.sort_by_key(|&(x, y, z)| (y, z, x));
    set.dedup();
    let suspicious = s("minecraft:suspicious_sand");
    let sand = s("minecraft:sand");
    // `placeSuspiciousSand(box, level, piece.getRandomCollapsedRoofPos())`.
    let place_suspicious = |world: &mut W, (x, y, z): (i32, i32, i32)| {
        if clip.is_inside(x, y, z) {
            world.set_block(x, y, z, &suspicious);
            // `getBlockEntity(pos, BRUSHABLE_BLOCK).ifPresent(be ->
            // be.setLootTable(DESERT_PYRAMID_ARCHAEOLOGY, pos.asLong()))`:
            // the seed is the position, packed, and costs no draw.
            world.record_block_entity(
                x,
                y,
                z,
                crate::block_entity::Record {
                    nbt: std::collections::BTreeMap::from([
                        (
                            "LootTable".to_string(),
                            rustmc_world::nbt::Nbt::String(
                                "minecraft:archaeology/desert_pyramid".into(),
                            ),
                        ),
                        (
                            "LootTableSeed".to_string(),
                            rustmc_world::nbt::Nbt::Long(block_pos_as_long(x, y, z)),
                        ),
                    ]),
                    loot_seed: None,
                },
            );
        }
    };
    place_suspicious(world, state.collapsed_roof);
    let mut list = set.clone();
    let bb = state.piece.bounding_box;
    let (cx, cy, cz) = bb.center();
    let mut positional = crate::rng::legacy::LegacyRandomSource::new(world_seed)
        .fork_positional()
        .at(cx, cy, cz);
    // `Util.shuffle`.
    let mut i = list.len();
    while i > 1 {
        let j = usize::try_from(positional.next_i32_bound(i32::try_from(i).unwrap_or(i32::MAX)))
            .unwrap_or(0);
        list.swap(i - 1, j);
        i -= 1;
    }
    // `Math.min(set.size(), random.nextInt(5, 8))`.
    let mut remaining =
        (5 + positional.next_i32_bound(3)).min(i32::try_from(set.len()).unwrap_or(i32::MAX));
    for pos in list {
        if remaining > 0 {
            remaining -= 1;
            place_suspicious(world, pos);
        } else if clip.is_inside(pos.0, pos.1, pos.2) {
            world.set_block(pos.0, pos.1, pos.2, &sand);
        }
    }
}
