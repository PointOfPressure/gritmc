//! Woodland mansions: `WoodlandMansionStructure` and `WoodlandMansionPieces`,
//! transcribed from the 26.2 jar's bytecode.
//!
//! The layout is two passes on one `RandomSource` — the start's
//! `setLargeFeatureSeed` random, after `Rotation.getRandom` has spent its one
//! draw:
//!
//! 1. [`MansionGrid::new`] lays an 11x11 cell plan: four random corridors out
//!    of a fixed entrance block, `cleanEdges` until stable, rooms identified
//!    on floors one and two (each `identifyRooms` shuffles its candidate cells
//!    with `Util.shuffle` and draws two booleans per room), then a third
//!    floor grown from one randomly chosen 1x2 room.
//! 2. [`place_pieces`] (`MansionPiecePlacer.createMansion`) walks that plan
//!    into template pieces: outer walls, roofs, corridor floors and carpets,
//!    indoor walls and doors, and one room template per room, every room
//!    template name being one more draw.
//!
//! Every piece is a `TemplateStructurePiece` with `BlockIgnoreProcessor
//! .STRUCTURE_BLOCK`, a rotation and a mirror, pivot at the origin. Nothing in
//! the piece's placement reads a random for a block; the stream spent in a
//! chunk goes to chest loot seeds only.
//!
//! The evokers, vindicators and allays the `Mage`, `Warrior` and `Group of
//! Allays` data markers spawn are made by the placer
//! (`worldgen_assembly::generate::place_mansions_in_chunk`, compounds from
//! [`crate::structure::spawned`]); their marker block becomes air.

use crate::rng::RandomSource;
use crate::structure::template::{Mirror, Rotation};

/// A horizontal `Direction`, plus `UP` — the one non-horizontal value the
/// placer assigns, meaning "a room with no corridor door, reached from
/// below".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    North,
    East,
    South,
    West,
    Up,
}

impl Dir {
    /// `Direction.Plane.HORIZONTAL`, in its iteration order.
    pub const HORIZONTAL: [Self; 4] = [Self::North, Self::East, Self::South, Self::West];

    /// `Direction.from2DDataValue`: SOUTH, WEST, NORTH, EAST.
    #[must_use]
    pub const fn from_2d(value: i32) -> Self {
        match value & 3 {
            0 => Self::South,
            1 => Self::West,
            2 => Self::North,
            _ => Self::East,
        }
    }

    #[must_use]
    pub const fn step_x(self) -> i32 {
        match self {
            Self::East => 1,
            Self::West => -1,
            _ => 0,
        }
    }

    #[must_use]
    pub const fn step_z(self) -> i32 {
        match self {
            Self::South => 1,
            Self::North => -1,
            _ => 0,
        }
    }

    /// `getClockWise`. `UP` is never turned.
    #[must_use]
    pub const fn clockwise(self) -> Self {
        match self {
            Self::North => Self::East,
            Self::East => Self::South,
            Self::South => Self::West,
            Self::West => Self::North,
            Self::Up => Self::Up,
        }
    }

    #[must_use]
    pub const fn counter_clockwise(self) -> Self {
        match self {
            Self::North => Self::West,
            Self::West => Self::South,
            Self::South => Self::East,
            Self::East => Self::North,
            Self::Up => Self::Up,
        }
    }

    #[must_use]
    pub const fn opposite(self) -> Self {
        self.clockwise().clockwise()
    }

    /// `Rotation.rotate(Direction)` for a horizontal direction.
    #[must_use]
    pub const fn rotated(self, rotation: Rotation) -> Self {
        match rotation {
            Rotation::None => self,
            Rotation::Clockwise90 => self.clockwise(),
            Rotation::Clockwise180 => self.opposite(),
            Rotation::Counterclockwise90 => self.counter_clockwise(),
        }
    }
}

/// `BlockPos.relative(direction, n)` for a horizontal direction.
const fn relative(p: (i32, i32, i32), d: Dir, n: i32) -> (i32, i32, i32) {
    (p.0 + d.step_x() * n, p.1, p.2 + d.step_z() * n)
}

/// `pos.relative(rotation.rotate(dir), n)`, the placer's one idiom.
const fn rel(p: (i32, i32, i32), rotation: Rotation, d: Dir, n: i32) -> (i32, i32, i32) {
    relative(p, d.rotated(rotation), n)
}

const fn above(p: (i32, i32, i32), n: i32) -> (i32, i32, i32) {
    (p.0, p.1 + n, p.2)
}

/// `WoodlandMansionPieces.SimpleGrid` — `int[width][height]` with a value
/// answered for every out-of-range read.
#[derive(Clone, Debug)]
pub struct SimpleGrid {
    pub width: i32,
    pub height: i32,
    value_if_outside: i32,
    grid: Vec<i32>,
}

impl SimpleGrid {
    #[must_use]
    pub fn new(width: i32, height: i32, value_if_outside: i32) -> Self {
        Self {
            width,
            height,
            value_if_outside,
            grid: vec![0; (width * height) as usize],
        }
    }

    const fn inside(&self, x: i32, y: i32) -> bool {
        x >= 0 && x < self.width && y >= 0 && y < self.height
    }

    pub fn set(&mut self, x: i32, y: i32, value: i32) {
        if self.inside(x, y) {
            self.grid[(x * self.height + y) as usize] = value;
        }
    }

    /// The five-argument `set`: an inclusive rectangle, row by row.
    pub fn set_box(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, value: i32) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                self.set(x, y, value);
            }
        }
    }

    #[must_use]
    pub fn get(&self, x: i32, y: i32) -> i32 {
        if self.inside(x, y) {
            self.grid[(x * self.height + y) as usize]
        } else {
            self.value_if_outside
        }
    }

    pub fn setif(&mut self, x: i32, y: i32, expected: i32, value: i32) {
        if self.get(x, y) == expected {
            self.set(x, y, value);
        }
    }

    #[must_use]
    pub fn edges_to(&self, x: i32, y: i32, value: i32) -> bool {
        self.get(x - 1, y) == value
            || self.get(x + 1, y) == value
            || self.get(x, y + 1) == value
            || self.get(x, y - 1) == value
    }
}

/// The room-kind field of a `floorRooms` cell.
const ROOM_MASK: i32 = 983_040;
const ROOM_1X1: i32 = 65_536;
const ROOM_1X2: i32 = 131_072;
const ROOM_2X2: i32 = 262_144;
/// The room's id, unique per floor.
const ID_MASK: i32 = 65_535;
/// The cell of a room that carries its door (or its stairs from below).
const ROOM_ORIGIN: i32 = 1_048_576;
/// The door cell touches a corridor.
const ROOM_DOOR: i32 = 2_097_152;
/// The 1x2 room the third floor's staircase rises from.
const ROOM_STAIRS: i32 = 4_194_304;
/// A cell the entrance or the third floor's stairs occupies.
const ROOM_CORRIDOR: i32 = 8_388_608;

/// `MansionGrid.isHouse`: a corridor (1), a room (2), the entrance (3) or the
/// third floor's staircase cell (4).
#[must_use]
pub fn is_house(grid: &SimpleGrid, x: i32, y: i32) -> bool {
    matches!(grid.get(x, y), 1..=4)
}

/// `WoodlandMansionPieces.MansionGrid`.
pub struct MansionGrid {
    pub base_grid: SimpleGrid,
    pub third_floor_grid: SimpleGrid,
    pub floor_rooms: [SimpleGrid; 3],
    pub entrance_x: i32,
    pub entrance_y: i32,
}

impl MansionGrid {
    /// The constructor, which is the whole plan.
    pub fn new<R: RandomSource + ?Sized>(random: &mut R) -> Self {
        let entrance_x = 7;
        let entrance_y = 4;
        let mut base = SimpleGrid::new(11, 11, 5);
        base.set_box(entrance_x, entrance_y, entrance_x + 1, entrance_y + 1, 3);
        base.set_box(
            entrance_x - 1,
            entrance_y,
            entrance_x - 1,
            entrance_y + 1,
            2,
        );
        base.set_box(
            entrance_x + 2,
            entrance_y - 2,
            entrance_x + 3,
            entrance_y + 3,
            5,
        );
        base.set_box(
            entrance_x + 1,
            entrance_y - 2,
            entrance_x + 1,
            entrance_y - 1,
            1,
        );
        base.set_box(
            entrance_x + 1,
            entrance_y + 2,
            entrance_x + 1,
            entrance_y + 3,
            1,
        );
        base.set(entrance_x - 1, entrance_y - 1, 1);
        base.set(entrance_x - 1, entrance_y + 2, 1);
        base.set_box(0, 0, 11, 1, 5);
        base.set_box(0, 9, 11, 11, 5);
        recursive_corridor(random, &mut base, entrance_x, entrance_y - 2, Dir::West, 6);
        recursive_corridor(random, &mut base, entrance_x, entrance_y + 3, Dir::West, 6);
        recursive_corridor(
            random,
            &mut base,
            entrance_x - 2,
            entrance_y - 1,
            Dir::West,
            3,
        );
        recursive_corridor(
            random,
            &mut base,
            entrance_x - 2,
            entrance_y + 2,
            Dir::West,
            3,
        );
        while clean_edges(&mut base) {}

        let mut floor_rooms = [
            SimpleGrid::new(11, 11, 5),
            SimpleGrid::new(11, 11, 5),
            SimpleGrid::new(11, 11, 5),
        ];
        identify_rooms(random, &base, &mut floor_rooms[0]);
        identify_rooms(random, &base, &mut floor_rooms[1]);
        floor_rooms[0].set_box(
            entrance_x + 1,
            entrance_y,
            entrance_x + 1,
            entrance_y + 1,
            ROOM_CORRIDOR,
        );
        floor_rooms[1].set_box(
            entrance_x + 1,
            entrance_y,
            entrance_x + 1,
            entrance_y + 1,
            ROOM_CORRIDOR,
        );
        let third = SimpleGrid::new(base.width, base.height, 5);
        let mut grid = Self {
            base_grid: base,
            third_floor_grid: third,
            floor_rooms,
            entrance_x,
            entrance_y,
        };
        grid.setup_third_floor(random);
        let third = grid.third_floor_grid.clone();
        identify_rooms(random, &third, &mut grid.floor_rooms[2]);
        grid
    }

    /// `isRoomId` — the grid argument is never read.
    #[must_use]
    pub fn is_room_id(&self, x: i32, y: i32, floor: usize, id: i32) -> bool {
        (self.floor_rooms[floor].get(x, y) & ID_MASK) == id
    }

    /// `get1x2RoomDirection`: the first horizontal neighbour carrying the
    /// same room id.
    #[must_use]
    pub fn get_1x2_room_direction(&self, x: i32, y: i32, floor: usize, id: i32) -> Option<Dir> {
        Dir::HORIZONTAL
            .into_iter()
            .find(|d| self.is_room_id(x + d.step_x(), y + d.step_z(), floor, id))
    }

    fn setup_third_floor<R: RandomSource + ?Sized>(&mut self, random: &mut R) {
        let mut candidates: Vec<(i32, i32)> = Vec::new();
        {
            let second = &self.floor_rooms[1];
            for y in 0..self.third_floor_grid.height {
                for x in 0..self.third_floor_grid.width {
                    let v = second.get(x, y);
                    if (v & ROOM_MASK) == ROOM_1X2 && (v & ROOM_DOOR) == ROOM_DOOR {
                        candidates.push((x, y));
                    }
                }
            }
        }
        if candidates.is_empty() {
            let (w, h) = (self.third_floor_grid.width, self.third_floor_grid.height);
            self.third_floor_grid.set_box(0, 0, w, h, 5);
            return;
        }
        let pick = candidates[random.next_i32_bound(candidates.len() as i32) as usize];
        let v = self.floor_rooms[1].get(pick.0, pick.1);
        self.floor_rooms[1].set(pick.0, pick.1, v | ROOM_STAIRS);
        let Some(dir) = self.get_1x2_room_direction(pick.0, pick.1, 1, v & ID_MASK) else {
            // vanilla dereferences the null and throws; a 1x2 room always has
            // its partner cell, so this is unreachable on a grid the
            // constructor built.
            return;
        };
        let ox = pick.0 + dir.step_x();
        let oy = pick.1 + dir.step_z();
        for y in 0..self.third_floor_grid.height {
            for x in 0..self.third_floor_grid.width {
                if !is_house(&self.base_grid, x, y) {
                    self.third_floor_grid.set(x, y, 5);
                } else if x == pick.0 && y == pick.1 {
                    self.third_floor_grid.set(x, y, 3);
                } else if x == ox && y == oy {
                    self.third_floor_grid.set(x, y, 3);
                    self.floor_rooms[2].set(x, y, ROOM_CORRIDOR);
                }
            }
        }
        let mut open: Vec<Dir> = Vec::new();
        for d in Dir::HORIZONTAL {
            if self.third_floor_grid.get(ox + d.step_x(), oy + d.step_z()) == 0 {
                open.push(d);
            }
        }
        if open.is_empty() {
            let (w, h) = (self.third_floor_grid.width, self.third_floor_grid.height);
            self.third_floor_grid.set_box(0, 0, w, h, 5);
            self.floor_rooms[1].set(pick.0, pick.1, v);
            return;
        }
        let d = open[random.next_i32_bound(open.len() as i32) as usize];
        recursive_corridor(
            random,
            &mut self.third_floor_grid,
            ox + d.step_x(),
            oy + d.step_z(),
            d,
            4,
        );
        while clean_edges(&mut self.third_floor_grid) {}
    }
}

fn recursive_corridor<R: RandomSource + ?Sized>(
    random: &mut R,
    grid: &mut SimpleGrid,
    x: i32,
    y: i32,
    heading: Dir,
    depth: i32,
) {
    if depth <= 0 {
        return;
    }
    grid.set(x, y, 1);
    grid.setif(x + heading.step_x(), y + heading.step_z(), 0, 1);
    for _ in 0..8 {
        let d = Dir::from_2d(random.next_i32_bound(4));
        if d == heading.opposite() {
            continue;
        }
        if d == Dir::East && random.next_bool() {
            continue;
        }
        let nx = x + heading.step_x();
        let ny = y + heading.step_z();
        if grid.get(nx + d.step_x(), ny + d.step_z()) == 0
            && grid.get(nx + d.step_x() * 2, ny + d.step_z() * 2) == 0
        {
            recursive_corridor(
                random,
                grid,
                x + heading.step_x() + d.step_x(),
                y + heading.step_z() + d.step_z(),
                d,
                depth - 1,
            );
            break;
        }
    }
    let cw = heading.clockwise();
    let ccw = heading.counter_clockwise();
    grid.setif(x + cw.step_x(), y + cw.step_z(), 0, 2);
    grid.setif(x + ccw.step_x(), y + ccw.step_z(), 0, 2);
    grid.setif(
        x + heading.step_x() + cw.step_x(),
        y + heading.step_z() + cw.step_z(),
        0,
        2,
    );
    grid.setif(
        x + heading.step_x() + ccw.step_x(),
        y + heading.step_z() + ccw.step_z(),
        0,
        2,
    );
    grid.setif(x + heading.step_x() * 2, y + heading.step_z() * 2, 0, 2);
    grid.setif(x + cw.step_x() * 2, y + cw.step_z() * 2, 0, 2);
    grid.setif(x + ccw.step_x() * 2, y + ccw.step_z() * 2, 0, 2);
}

fn clean_edges(grid: &mut SimpleGrid) -> bool {
    let mut changed = false;
    for y in 0..grid.height {
        for x in 0..grid.width {
            if grid.get(x, y) != 0 {
                continue;
            }
            let sides = i32::from(is_house(grid, x + 1, y))
                + i32::from(is_house(grid, x - 1, y))
                + i32::from(is_house(grid, x, y + 1))
                + i32::from(is_house(grid, x, y - 1));
            if sides >= 3 {
                grid.set(x, y, 2);
                changed = true;
            } else if sides == 2 {
                let corners = i32::from(is_house(grid, x + 1, y + 1))
                    + i32::from(is_house(grid, x - 1, y + 1))
                    + i32::from(is_house(grid, x + 1, y - 1))
                    + i32::from(is_house(grid, x - 1, y - 1));
                if corners <= 1 {
                    grid.set(x, y, 2);
                    changed = true;
                }
            }
        }
    }
    changed
}

fn identify_rooms<R: RandomSource + ?Sized>(
    random: &mut R,
    plan: &SimpleGrid,
    rooms: &mut SimpleGrid,
) {
    let mut cells: Vec<(i32, i32)> = Vec::new();
    for y in 0..plan.height {
        for x in 0..plan.width {
            if plan.get(x, y) == 2 {
                cells.push((x, y));
            }
        }
    }
    crate::structure::jigsaw::shuffle(&mut cells, random);
    let mut id = 10;
    for (x, y) in cells {
        if rooms.get(x, y) != 0 {
            continue;
        }
        let (mut min_x, mut max_x, mut min_y, mut max_y) = (x, x, y, y);
        let free = |rooms: &SimpleGrid, dx: i32, dy: i32| {
            rooms.get(x + dx, y + dy) == 0 && plan.get(x + dx, y + dy) == 2
        };
        let kind;
        if rooms.get(x + 1, y) == 0
            && rooms.get(x, y + 1) == 0
            && rooms.get(x + 1, y + 1) == 0
            && plan.get(x + 1, y) == 2
            && plan.get(x, y + 1) == 2
            && plan.get(x + 1, y + 1) == 2
        {
            max_x += 1;
            max_y += 1;
            kind = ROOM_2X2;
        } else if rooms.get(x - 1, y) == 0
            && rooms.get(x, y + 1) == 0
            && rooms.get(x - 1, y + 1) == 0
            && plan.get(x - 1, y) == 2
            && plan.get(x, y + 1) == 2
            && plan.get(x - 1, y + 1) == 2
        {
            min_x -= 1;
            max_y += 1;
            kind = ROOM_2X2;
        } else if rooms.get(x - 1, y) == 0
            && rooms.get(x, y - 1) == 0
            && rooms.get(x - 1, y - 1) == 0
            && plan.get(x - 1, y) == 2
            && plan.get(x, y - 1) == 2
            && plan.get(x - 1, y - 1) == 2
        {
            min_x -= 1;
            min_y -= 1;
            kind = ROOM_2X2;
        } else if free(rooms, 1, 0) {
            max_x += 1;
            kind = ROOM_1X2;
        } else if free(rooms, 0, 1) {
            max_y += 1;
            kind = ROOM_1X2;
        } else if free(rooms, -1, 0) {
            min_x -= 1;
            kind = ROOM_1X2;
        } else if free(rooms, 0, -1) {
            min_y -= 1;
            kind = ROOM_1X2;
        } else {
            kind = ROOM_1X1;
        }
        let mut door_x = if random.next_bool() { min_x } else { max_x };
        let mut door_y = if random.next_bool() { min_y } else { max_y };
        let mut door = ROOM_DOOR;
        if !plan.edges_to(door_x, door_y, 1) {
            door_x = if door_x == min_x { max_x } else { min_x };
            door_y = if door_y == min_y { max_y } else { min_y };
            if !plan.edges_to(door_x, door_y, 1) {
                door_y = if door_y == min_y { max_y } else { min_y };
                if !plan.edges_to(door_x, door_y, 1) {
                    door_x = if door_x == min_x { max_x } else { min_x };
                    door_y = if door_y == min_y { max_y } else { min_y };
                    if !plan.edges_to(door_x, door_y, 1) {
                        door = 0;
                        door_x = min_x;
                        door_y = min_y;
                    }
                }
            }
        }
        for yy in min_y..=max_y {
            for xx in min_x..=max_x {
                if xx == door_x && yy == door_y {
                    rooms.set(xx, yy, ROOM_ORIGIN | door | kind | id);
                } else {
                    rooms.set(xx, yy, kind | id);
                }
            }
        }
        id += 1;
    }
}

/// One `WoodlandMansionPiece`: a template under `woodland_mansion/`, its
/// `templatePosition`, rotation and mirror.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MansionPiece {
    pub name: String,
    pub pos: (i32, i32, i32),
    pub rotation: Rotation,
    pub mirror: Mirror,
}

/// `FloorRoomCollection` and its three subclasses, by floor.
fn room_1x1<R: RandomSource + ?Sized>(floor: usize, random: &mut R) -> String {
    if floor == 0 {
        format!("1x1_a{}", random.next_i32_bound(5) + 1)
    } else {
        format!("1x1_b{}", random.next_i32_bound(5) + 1)
    }
}

fn room_1x1_secret<R: RandomSource + ?Sized>(_floor: usize, random: &mut R) -> String {
    format!("1x1_as{}", random.next_i32_bound(4) + 1)
}

fn room_1x2_side<R: RandomSource + ?Sized>(floor: usize, random: &mut R, stairs: bool) -> String {
    if floor == 0 {
        format!("1x2_a{}", random.next_i32_bound(9) + 1)
    } else if stairs {
        "1x2_c_stairs".to_string()
    } else {
        format!("1x2_c{}", random.next_i32_bound(4) + 1)
    }
}

fn room_1x2_front<R: RandomSource + ?Sized>(floor: usize, random: &mut R, stairs: bool) -> String {
    if floor == 0 {
        format!("1x2_b{}", random.next_i32_bound(5) + 1)
    } else if stairs {
        "1x2_d_stairs".to_string()
    } else {
        format!("1x2_d{}", random.next_i32_bound(5) + 1)
    }
}

fn room_1x2_secret<R: RandomSource + ?Sized>(floor: usize, random: &mut R) -> String {
    if floor == 0 {
        format!("1x2_s{}", random.next_i32_bound(2) + 1)
    } else {
        format!("1x2_se{}", random.next_i32_bound(1) + 1)
    }
}

fn room_2x2<R: RandomSource + ?Sized>(floor: usize, random: &mut R) -> String {
    if floor == 0 {
        format!("2x2_a{}", random.next_i32_bound(4) + 1)
    } else {
        format!("2x2_b{}", random.next_i32_bound(5) + 1)
    }
}

/// `StructureTemplate.getZeroPositionWithTransform(zero, NONE, rotation, 7,
/// 7)` followed by `BlockPos.rotate(outer)` — `addRoom1x1`'s offset.
const fn room_1x1_offset(rotation: Rotation, outer: Rotation) -> (i32, i32) {
    // size 7x7, so sizeX - 1 = sizeZ - 1 = 6; zero is (1, 0, 0); no mirror.
    let (x, z) = match rotation {
        Rotation::Counterclockwise90 => (1, 6),
        Rotation::Clockwise90 => (1 + 6, 0),
        Rotation::Clockwise180 => (1 + 6, 6),
        Rotation::None => (1, 0),
    };
    match outer {
        Rotation::None => (x, z),
        Rotation::Clockwise90 => (-z, x),
        Rotation::Clockwise180 => (-x, -z),
        Rotation::Counterclockwise90 => (z, -x),
    }
}

struct PlacementData {
    position: (i32, i32, i32),
    rotation: Rotation,
    wall_type: &'static str,
}

struct Placer<'r, R: RandomSource + ?Sized> {
    random: &'r mut R,
    start_x: i32,
    start_y: i32,
    out: Vec<MansionPiece>,
}

impl<R: RandomSource + ?Sized> Placer<'_, R> {
    fn add(&mut self, name: &str, pos: (i32, i32, i32), rotation: Rotation, mirror: Mirror) {
        self.out.push(MansionPiece {
            name: name.to_string(),
            pos,
            rotation,
            mirror,
        });
    }

    fn cell(&self, base: (i32, i32, i32), rotation: Rotation, x: i32, y: i32) -> (i32, i32, i32) {
        let p = rel(base, rotation, Dir::South, 8 + (y - self.start_y) * 8);
        rel(p, rotation, Dir::East, (x - self.start_x) * 8)
    }

    fn entrance(&mut self, data: &mut PlacementData) {
        let west = Dir::West.rotated(data.rotation);
        self.add(
            "entrance",
            relative(data.position, west, 9),
            data.rotation,
            Mirror::None,
        );
        data.position = rel(data.position, data.rotation, Dir::South, 16);
    }

    fn traverse_wall_piece(&mut self, data: &mut PlacementData) {
        let at = rel(data.position, data.rotation, Dir::East, 7);
        self.add(data.wall_type, at, data.rotation, Mirror::None);
        data.position = rel(data.position, data.rotation, Dir::South, 8);
    }

    fn traverse_turn(&mut self, data: &mut PlacementData) {
        data.position = rel(data.position, data.rotation, Dir::South, -1);
        self.add("wall_corner", data.position, data.rotation, Mirror::None);
        data.position = rel(data.position, data.rotation, Dir::South, -7);
        data.position = rel(data.position, data.rotation, Dir::West, -6);
        data.rotation = data.rotation.get_rotated(Rotation::Clockwise90);
    }

    fn traverse_inner_turn(data: &mut PlacementData) {
        data.position = rel(data.position, data.rotation, Dir::South, 6);
        data.position = rel(data.position, data.rotation, Dir::East, 8);
        data.rotation = data.rotation.get_rotated(Rotation::Counterclockwise90);
    }

    #[expect(clippy::too_many_arguments, reason = "the bytecode's own signature")]
    fn traverse_outer_walls(
        &mut self,
        data: &mut PlacementData,
        grid: &SimpleGrid,
        mut dir: Dir,
        start_x: i32,
        start_y: i32,
        end_x: i32,
        end_y: i32,
    ) {
        let mut x = start_x;
        let mut y = start_y;
        let first = dir;
        loop {
            if !is_house(grid, x + dir.step_x(), y + dir.step_z()) {
                self.traverse_turn(data);
                dir = dir.clockwise();
                if x != end_x || y != end_y || first != dir {
                    self.traverse_wall_piece(data);
                }
            } else if is_house(
                grid,
                x + dir.step_x() + dir.counter_clockwise().step_x(),
                y + dir.step_z() + dir.counter_clockwise().step_z(),
            ) {
                Self::traverse_inner_turn(data);
                x += dir.step_x();
                y += dir.step_z();
                dir = dir.counter_clockwise();
            } else {
                x += dir.step_x();
                y += dir.step_z();
                if x != end_x || y != end_y || first != dir {
                    self.traverse_wall_piece(data);
                }
            }
            if x == end_x && y == end_y && first == dir {
                break;
            }
        }
    }

    fn create_roof(
        &mut self,
        pos: (i32, i32, i32),
        rotation: Rotation,
        grid: &SimpleGrid,
        upper: Option<&SimpleGrid>,
    ) {
        use Rotation::{Clockwise180 as R180, Clockwise90 as R90, Counterclockwise90 as R270};
        let n = Mirror::None;
        for i in 0..grid.height {
            for j in 0..grid.width {
                let p = self.cell(pos, rotation, j, i);
                let covered = upper.is_some_and(|u| is_house(u, j, i));
                if is_house(grid, j, i) && !covered {
                    self.add("roof", above(p, 3), rotation, n);
                    if !is_house(grid, j + 1, i) {
                        self.add("roof_front", rel(p, rotation, Dir::East, 6), rotation, n);
                    }
                    if !is_house(grid, j - 1, i) {
                        let q = rel(rel(p, rotation, Dir::East, 0), rotation, Dir::South, 7);
                        self.add("roof_front", q, rotation.get_rotated(R180), n);
                    }
                    if !is_house(grid, j, i - 1) {
                        let q = rel(p, rotation, Dir::West, 1);
                        self.add("roof_front", q, rotation.get_rotated(R270), n);
                    }
                    if !is_house(grid, j, i + 1) {
                        let q = rel(rel(p, rotation, Dir::East, 6), rotation, Dir::South, 6);
                        self.add("roof_front", q, rotation.get_rotated(R90), n);
                    }
                }
            }
        }
        if let Some(upper) = upper {
            for i in 0..grid.height {
                for j in 0..grid.width {
                    let p = self.cell(pos, rotation, j, i);
                    if !(is_house(grid, j, i) && is_house(upper, j, i)) {
                        continue;
                    }
                    if !is_house(grid, j + 1, i) {
                        self.add("small_wall", rel(p, rotation, Dir::East, 7), rotation, n);
                    }
                    if !is_house(grid, j - 1, i) {
                        let q = rel(rel(p, rotation, Dir::West, 1), rotation, Dir::South, 6);
                        self.add("small_wall", q, rotation.get_rotated(R180), n);
                    }
                    if !is_house(grid, j, i - 1) {
                        let q = rel(rel(p, rotation, Dir::West, 0), rotation, Dir::North, 1);
                        self.add("small_wall", q, rotation.get_rotated(R270), n);
                    }
                    if !is_house(grid, j, i + 1) {
                        let q = rel(rel(p, rotation, Dir::East, 6), rotation, Dir::South, 7);
                        self.add("small_wall", q, rotation.get_rotated(R90), n);
                    }
                    if !is_house(grid, j + 1, i) {
                        if !is_house(grid, j, i - 1) {
                            let q = rel(rel(p, rotation, Dir::East, 7), rotation, Dir::North, 2);
                            self.add("small_wall_corner", q, rotation, n);
                        }
                        if !is_house(grid, j, i + 1) {
                            let q = rel(rel(p, rotation, Dir::East, 8), rotation, Dir::South, 7);
                            self.add("small_wall_corner", q, rotation.get_rotated(R90), n);
                        }
                    }
                    if !is_house(grid, j - 1, i) {
                        if !is_house(grid, j, i - 1) {
                            let q = rel(rel(p, rotation, Dir::West, 2), rotation, Dir::North, 1);
                            self.add("small_wall_corner", q, rotation.get_rotated(R270), n);
                        }
                        if !is_house(grid, j, i + 1) {
                            let q = rel(rel(p, rotation, Dir::West, 1), rotation, Dir::South, 8);
                            self.add("small_wall_corner", q, rotation.get_rotated(R180), n);
                        }
                    }
                }
            }
        }
        for i in 0..grid.height {
            for j in 0..grid.width {
                let p = self.cell(pos, rotation, j, i);
                let covered = upper.is_some_and(|u| is_house(u, j, i));
                if !is_house(grid, j, i) || covered {
                    continue;
                }
                if !is_house(grid, j + 1, i) {
                    let e = rel(p, rotation, Dir::East, 6);
                    if !is_house(grid, j, i + 1) {
                        self.add("roof_corner", rel(e, rotation, Dir::South, 6), rotation, n);
                    } else if is_house(grid, j + 1, i + 1) {
                        let q = rel(e, rotation, Dir::South, 5);
                        self.add("roof_inner_corner", q, rotation, n);
                    }
                    if !is_house(grid, j, i - 1) {
                        self.add("roof_corner", e, rotation.get_rotated(R270), n);
                    } else if is_house(grid, j + 1, i - 1) {
                        let q = rel(rel(p, rotation, Dir::East, 9), rotation, Dir::North, 2);
                        self.add("roof_inner_corner", q, rotation.get_rotated(R90), n);
                    }
                }
                if !is_house(grid, j - 1, i) {
                    let w = rel(rel(p, rotation, Dir::East, 0), rotation, Dir::South, 0);
                    if !is_house(grid, j, i + 1) {
                        let q = rel(w, rotation, Dir::South, 6);
                        self.add("roof_corner", q, rotation.get_rotated(R90), n);
                    } else if is_house(grid, j - 1, i + 1) {
                        let q = rel(rel(w, rotation, Dir::South, 8), rotation, Dir::West, 3);
                        self.add("roof_inner_corner", q, rotation.get_rotated(R270), n);
                    }
                    if !is_house(grid, j, i - 1) {
                        self.add("roof_corner", w, rotation.get_rotated(R180), n);
                    } else if is_house(grid, j - 1, i - 1) {
                        let q = rel(w, rotation, Dir::South, 1);
                        self.add("roof_inner_corner", q, rotation.get_rotated(R180), n);
                    }
                }
            }
        }
    }

    fn add_room_1x1(
        &mut self,
        pos: (i32, i32, i32),
        rotation: Rotation,
        door: Option<Dir>,
        floor: usize,
    ) {
        let mut r = Rotation::None;
        let mut name = room_1x1(floor, self.random);
        match door {
            Some(Dir::East) => {}
            Some(Dir::North) => r = r.get_rotated(Rotation::Counterclockwise90),
            Some(Dir::West) => r = r.get_rotated(Rotation::Clockwise180),
            Some(Dir::South) => r = r.get_rotated(Rotation::Clockwise90),
            _ => name = room_1x1_secret(floor, self.random),
        }
        let off = room_1x1_offset(r, rotation);
        let r = r.get_rotated(rotation);
        self.add(
            &name,
            (pos.0 + off.0, pos.1, pos.2 + off.1),
            r,
            Mirror::None,
        );
    }

    fn add_room_1x2(
        &mut self,
        pos: (i32, i32, i32),
        rotation: Rotation,
        room: Option<Dir>,
        door: Dir,
        floor: usize,
        stairs: bool,
    ) {
        use Dir::{East as E, North as N, South as S, Up as U, West as W};
        use Rotation::{Clockwise180 as R180, Clockwise90 as R90, Counterclockwise90 as R270};
        let r = rotation;
        let Some(room) = room else { return };
        let (name, at, rot, mirror) = match (door, room) {
            (E, S) => {
                let at = rel(pos, r, E, 1);
                (
                    room_1x2_side(floor, self.random, stairs),
                    at,
                    r,
                    Mirror::None,
                )
            }
            (E, N) => {
                let at = rel(rel(pos, r, E, 1), r, S, 6);
                (
                    room_1x2_side(floor, self.random, stairs),
                    at,
                    r,
                    Mirror::LeftRight,
                )
            }
            (W, N) => {
                let at = rel(rel(pos, r, E, 7), r, S, 6);
                (
                    room_1x2_side(floor, self.random, stairs),
                    at,
                    r.get_rotated(R180),
                    Mirror::None,
                )
            }
            (W, S) => {
                let at = rel(pos, r, E, 7);
                (
                    room_1x2_side(floor, self.random, stairs),
                    at,
                    r,
                    Mirror::FrontBack,
                )
            }
            (S, E) => {
                let at = rel(pos, r, E, 1);
                (
                    room_1x2_side(floor, self.random, stairs),
                    at,
                    r.get_rotated(R90),
                    Mirror::LeftRight,
                )
            }
            (S, W) => {
                let at = rel(pos, r, E, 7);
                (
                    room_1x2_side(floor, self.random, stairs),
                    at,
                    r.get_rotated(R90),
                    Mirror::None,
                )
            }
            (N, W) => {
                let at = rel(rel(pos, r, E, 7), r, S, 6);
                (
                    room_1x2_side(floor, self.random, stairs),
                    at,
                    r.get_rotated(R90),
                    Mirror::FrontBack,
                )
            }
            (N, E) => {
                let at = rel(rel(pos, r, E, 1), r, S, 6);
                (
                    room_1x2_side(floor, self.random, stairs),
                    at,
                    r.get_rotated(R270),
                    Mirror::None,
                )
            }
            (S, N) => {
                let at = rel(rel(pos, r, E, 1), r, N, 8);
                (
                    room_1x2_front(floor, self.random, stairs),
                    at,
                    r,
                    Mirror::None,
                )
            }
            (N, S) => {
                let at = rel(rel(pos, r, E, 7), r, S, 14);
                (
                    room_1x2_front(floor, self.random, stairs),
                    at,
                    r.get_rotated(R180),
                    Mirror::None,
                )
            }
            (W, E) => {
                let at = rel(pos, r, E, 15);
                (
                    room_1x2_front(floor, self.random, stairs),
                    at,
                    r.get_rotated(R90),
                    Mirror::None,
                )
            }
            (E, W) => {
                let at = rel(rel(pos, r, W, 7), r, S, 6);
                (
                    room_1x2_front(floor, self.random, stairs),
                    at,
                    r.get_rotated(R270),
                    Mirror::None,
                )
            }
            (U, E) => {
                let at = rel(pos, r, E, 15);
                (
                    room_1x2_secret(floor, self.random),
                    at,
                    r.get_rotated(R90),
                    Mirror::None,
                )
            }
            (U, S) => {
                let at = rel(rel(pos, r, E, 1), r, N, 0);
                (room_1x2_secret(floor, self.random), at, r, Mirror::None)
            }
            _ => return,
        };
        self.add(&name, at, rot, mirror);
    }

    fn add_room_2x2(
        &mut self,
        pos: (i32, i32, i32),
        rotation: Rotation,
        room: Dir,
        door: Dir,
        floor: usize,
    ) {
        use Dir::{East as E, North as N, South as S, West as W};
        let (mut dx, mut dz, mut rot, mut mirror) = (0, 0, rotation, Mirror::None);
        match (door, room) {
            (E, S) => dx = -7,
            (E, N) => {
                dx = -7;
                dz = 6;
                mirror = Mirror::LeftRight;
            }
            (N, E) => {
                dx = 1;
                dz = 14;
                rot = rotation.get_rotated(Rotation::Counterclockwise90);
            }
            (N, W) => {
                dx = 7;
                dz = 14;
                rot = rotation.get_rotated(Rotation::Counterclockwise90);
                mirror = Mirror::LeftRight;
            }
            (S, W) => {
                dx = 7;
                dz = -8;
                rot = rotation.get_rotated(Rotation::Clockwise90);
            }
            (S, E) => {
                dx = 1;
                dz = -8;
                rot = rotation.get_rotated(Rotation::Clockwise90);
                mirror = Mirror::LeftRight;
            }
            (W, N) => {
                dx = 15;
                dz = 6;
                rot = rotation.get_rotated(Rotation::Clockwise180);
            }
            (W, S) => {
                dx = 15;
                mirror = Mirror::FrontBack;
            }
            _ => {}
        }
        let at = rel(rel(pos, rotation, E, dx), rotation, S, dz);
        let name = room_2x2(floor, self.random);
        self.add(&name, at, rot, mirror);
    }

    fn add_room_2x2_secret(&mut self, pos: (i32, i32, i32), rotation: Rotation) {
        let at = rel(pos, rotation, Dir::East, 1);
        self.add("2x2_s1", at, rotation, Mirror::None);
    }

    fn create_mansion(&mut self, pos: (i32, i32, i32), rotation: Rotation, grid: &MansionGrid) {
        let mut data = PlacementData {
            position: pos,
            rotation,
            wall_type: "wall_flat",
        };
        self.entrance(&mut data);
        let mut data2 = PlacementData {
            position: above(data.position, 8),
            rotation: data.rotation,
            wall_type: "wall_window",
        };
        let base = &grid.base_grid;
        let third = &grid.third_floor_grid;
        self.start_x = grid.entrance_x + 1;
        self.start_y = grid.entrance_y + 1;
        let end_x = grid.entrance_x + 1;
        let end_y = grid.entrance_y;
        let (sx, sy) = (self.start_x, self.start_y);
        self.traverse_outer_walls(&mut data, base, Dir::South, sx, sy, end_x, end_y);
        self.traverse_outer_walls(&mut data2, base, Dir::South, sx, sy, end_x, end_y);
        let mut data3 = PlacementData {
            position: above(data.position, 19),
            rotation: data.rotation,
            wall_type: "wall_window",
        };
        'found: for k in 0..third.height {
            for l in (0..third.width).rev() {
                if is_house(third, l, k) {
                    data3.position = rel(
                        data3.position,
                        rotation,
                        Dir::South,
                        8 + (k - self.start_y) * 8,
                    );
                    data3.position =
                        rel(data3.position, rotation, Dir::East, (l - self.start_x) * 8);
                    self.traverse_wall_piece(&mut data3);
                    self.traverse_outer_walls(&mut data3, third, Dir::South, l, k, l, k);
                    break 'found;
                }
            }
        }
        self.create_roof(above(pos, 16), rotation, base, Some(third));
        self.create_roof(above(pos, 27), rotation, third, None);

        for floor in 0..3usize {
            let floor_pos = above(pos, 8 * floor as i32 + if floor == 2 { 3 } else { 0 });
            let rooms = &grid.floor_rooms[floor];
            let g = if floor == 2 { third } else { base };
            let south_carpet = if floor == 0 {
                "carpet_south_1"
            } else {
                "carpet_south_2"
            };
            let west_carpet = if floor == 0 {
                "carpet_west_1"
            } else {
                "carpet_west_2"
            };
            let corridor = |x: i32, y: i32| {
                g.get(x, y) == 1 || (rooms.get(x, y) & ROOM_CORRIDOR) == ROOM_CORRIDOR
            };
            for y in 0..g.height {
                for x in 0..g.width {
                    if g.get(x, y) != 1 {
                        continue;
                    }
                    let p = self.cell(floor_pos, rotation, x, y);
                    self.add("corridor_floor", p, rotation, Mirror::None);
                    if corridor(x, y - 1) {
                        self.add(
                            "carpet_north",
                            above(rel(p, rotation, Dir::East, 1), 1),
                            rotation,
                            Mirror::None,
                        );
                    }
                    if corridor(x + 1, y) {
                        let q = above(
                            rel(rel(p, rotation, Dir::South, 1), rotation, Dir::East, 5),
                            1,
                        );
                        self.add("carpet_east", q, rotation, Mirror::None);
                    }
                    if corridor(x, y + 1) {
                        let q = rel(rel(p, rotation, Dir::South, 5), rotation, Dir::West, 1);
                        self.add(south_carpet, q, rotation, Mirror::None);
                    }
                    if corridor(x - 1, y) {
                        let q = rel(rel(p, rotation, Dir::West, 1), rotation, Dir::North, 1);
                        self.add(west_carpet, q, rotation, Mirror::None);
                    }
                }
            }
            let wall = if floor == 0 {
                "indoors_wall_1"
            } else {
                "indoors_wall_2"
            };
            let door_name = if floor == 0 {
                "indoors_door_1"
            } else {
                "indoors_door_2"
            };
            let mut doors: Vec<Dir> = Vec::new();
            for y in 0..g.height {
                for x in 0..g.width {
                    let mut stairs_cell = floor == 2 && g.get(x, y) == 3;
                    if g.get(x, y) != 2 && !stairs_cell {
                        continue;
                    }
                    let v = rooms.get(x, y);
                    let kind = v & ROOM_MASK;
                    let id = v & ID_MASK;
                    stairs_cell = stairs_cell && (v & ROOM_CORRIDOR) == ROOM_CORRIDOR;
                    doors.clear();
                    if (v & ROOM_DOOR) == ROOM_DOOR {
                        for d in Dir::HORIZONTAL {
                            if g.get(x + d.step_x(), y + d.step_z()) == 1 {
                                doors.push(d);
                            }
                        }
                    }
                    let door = if doors.is_empty() {
                        ((v & ROOM_ORIGIN) == ROOM_ORIGIN).then_some(Dir::Up)
                    } else {
                        Some(doors[self.random.next_i32_bound(doors.len() as i32) as usize])
                    };
                    let p = rel(floor_pos, rotation, Dir::South, 8 + (y - self.start_y) * 8);
                    let p = rel(p, rotation, Dir::East, -1 + (x - self.start_x) * 8);
                    let pick = |d: Dir| if door == Some(d) { door_name } else { wall };
                    if is_house(g, x - 1, y) && !grid.is_room_id(x - 1, y, floor, id) {
                        self.add(pick(Dir::West), p, rotation, Mirror::None);
                    }
                    if g.get(x + 1, y) == 1 && !stairs_cell {
                        self.add(
                            pick(Dir::East),
                            rel(p, rotation, Dir::East, 8),
                            rotation,
                            Mirror::None,
                        );
                    }
                    if is_house(g, x, y + 1) && !grid.is_room_id(x, y + 1, floor, id) {
                        let q = rel(rel(p, rotation, Dir::South, 7), rotation, Dir::East, 7);
                        self.add(
                            pick(Dir::South),
                            q,
                            rotation.get_rotated(Rotation::Clockwise90),
                            Mirror::None,
                        );
                    }
                    if g.get(x, y - 1) == 1 && !stairs_cell {
                        let q = rel(rel(p, rotation, Dir::North, 1), rotation, Dir::East, 7);
                        self.add(
                            pick(Dir::North),
                            q,
                            rotation.get_rotated(Rotation::Clockwise90),
                            Mirror::None,
                        );
                    }
                    if kind == ROOM_1X1 {
                        self.add_room_1x1(p, rotation, door, floor);
                    } else if kind == ROOM_1X2 && door.is_some() {
                        let room = grid.get_1x2_room_direction(x, y, floor, id);
                        let stairs = (v & ROOM_STAIRS) == ROOM_STAIRS;
                        self.add_room_1x2(
                            p,
                            rotation,
                            room,
                            door.unwrap_or(Dir::Up),
                            floor,
                            stairs,
                        );
                    } else if kind == ROOM_2X2 && door.is_some() && door != Some(Dir::Up) {
                        let d = door.unwrap_or(Dir::Up);
                        let mut side = d.clockwise();
                        if !grid.is_room_id(x + side.step_x(), y + side.step_z(), floor, id) {
                            side = side.opposite();
                        }
                        self.add_room_2x2(p, rotation, side, d, floor);
                    } else if kind == ROOM_2X2 && door == Some(Dir::Up) {
                        self.add_room_2x2_secret(p, rotation);
                    }
                }
            }
        }
    }
}

/// `WoodlandMansionPieces.generateMansion`: the grid, then the placer, on the
/// one random.
pub fn generate_mansion<R: RandomSource + ?Sized>(
    pos: (i32, i32, i32),
    rotation: Rotation,
    random: &mut R,
) -> Vec<MansionPiece> {
    let grid = MansionGrid::new(random);
    let mut placer = Placer {
        random,
        start_x: 0,
        start_y: 0,
        out: Vec::new(),
    };
    placer.create_mansion(pos, rotation, &grid);
    placer.out
}
