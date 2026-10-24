//! Ocean monuments: `OceanMonumentStructure` and `OceanMonumentPieces`,
//! transcribed from the 26.2 jar's bytecode.
//!
//! One piece is saved — `MonumentBuilding`, 58 x 23 x 58 at y 39 — and every
//! room lives inside it as a child the building's own `postProcess` walks.
//! The layout is built on the start's `setLargeFeatureSeed` random, in this
//! order:
//!
//! 1. `Direction.Plane.HORIZONTAL.getRandomDirection` — the facing;
//! 2. `generateRoomGraph`: `nextInt(4)` picks the core room's column, the 75
//!    grid cells (less the ones a 5 x 5 x 3 lattice leaves out) are shuffled
//!    with `Util.shuffle`, and each cell then closes up to two of its
//!    openings, drawing `nextInt(6)` at most five times, keeping a closure
//!    only if both sides can still reach the source room;
//! 3. the room fitters, in their fixed order, claim the cells; a
//!    `FitSimpleRoom` draws `nextInt(3)` for its design;
//! 4. one `nextInt()` whose low bit, after `+1` and `+2`, picks the two wing
//!    designs.
//!
//! `postProcess` then draws from the chunk's decoration random only in two
//! rooms: a `SimpleRoom` whose design is not 0 spends a `nextBoolean`, and
//! a `SimpleTopRoom` spends `nextInt(3)` per floor column plus a `nextInt(4)`
//! when the sponge is placed.
//!
//! The elder guardians `spawnElder` adds are made by the placer
//! (`worldgen_assembly::generate::spawn_monument_elders`). Creating one
//! spends nothing on the structure's stream, but its `finalizeSpawn` draws
//! three values from the chunk's level random — which is why their saved
//! `follow_range` bonus is deterministic, and checked.

use crate::rng::RandomSource;
use crate::structure::pieces::{BoundingBox, Facing, Piece};
use crate::structure::scattered::{fill_column_down, make_bounding_box, random_horizontal};
use crate::structure::writer::{StructureWorld, Writer};

/// `Direction.get3DDataValue` order: DOWN, UP, NORTH, SOUTH, WEST, EAST.
const DOWN: usize = 0;
const UP: usize = 1;
const NORTH: usize = 2;
const SOUTH: usize = 3;
const WEST: usize = 4;
const EAST: usize = 5;

/// `Direction.values()` as `(data3d, dx, dy, dz)`.
const DIRECTIONS: [(usize, i32, i32, i32); 6] = [
    (DOWN, 0, -1, 0),
    (UP, 0, 1, 0),
    (NORTH, 0, 0, -1),
    (SOUTH, 0, 0, 1),
    (WEST, -1, 0, 0),
    (EAST, 1, 0, 0),
];

const fn opposite(d: usize) -> usize {
    match d {
        DOWN => UP,
        UP => DOWN,
        NORTH => SOUTH,
        SOUTH => NORTH,
        WEST => EAST,
        _ => WEST,
    }
}

/// `OceanMonumentPiece.getRoomIndex(x, y, z)`.
const fn room_index(x: i32, y: i32, z: i32) -> i32 {
    y * 25 + z * 5 + x
}

const SOURCE_INDEX: i32 = room_index(2, 0, 0);
const TOP_CONNECT_INDEX: i32 = room_index(2, 2, 0);
const LEFTWING_CONNECT_INDEX: i32 = room_index(0, 1, 0);
const RIGHTWING_CONNECT_INDEX: i32 = room_index(4, 1, 0);

/// `OceanMonumentPieces.RoomDefinition`.
#[derive(Clone, Debug)]
pub struct RoomDef {
    pub index: i32,
    pub connections: [Option<usize>; 6],
    pub has_opening: [bool; 6],
    pub claimed: bool,
    pub is_source: bool,
    scan_index: i32,
}

impl RoomDef {
    const fn new(index: i32) -> Self {
        Self {
            index,
            connections: [None; 6],
            has_opening: [false; 6],
            claimed: false,
            is_source: false,
            scan_index: 0,
        }
    }

    /// `isSpecial` — the three rooms outside the grid.
    #[must_use]
    pub const fn is_special(&self) -> bool {
        self.index >= 75
    }

    fn count_openings(&self) -> i32 {
        self.has_opening.iter().filter(|o| **o).count() as i32
    }
}

fn set_connection(defs: &mut [RoomDef], from: usize, dir: usize, to: usize) {
    defs[from].connections[dir] = Some(to);
    defs[to].connections[opposite(dir)] = Some(from);
}

fn update_openings(defs: &mut [RoomDef], room: usize) {
    for i in 0..6 {
        defs[room].has_opening[i] = defs[room].connections[i].is_some();
    }
}

/// `RoomDefinition.findSource`, recursive exactly as vanilla's is.
fn find_source(defs: &mut [RoomDef], room: usize, scan: i32) -> bool {
    if defs[room].is_source {
        return true;
    }
    defs[room].scan_index = scan;
    for i in 0..6 {
        if let Some(next) = defs[room].connections[i] {
            if defs[room].has_opening[i]
                && defs[next].scan_index != scan
                && find_source(defs, next, scan)
            {
                return true;
            }
        }
    }
    false
}

/// What a room is, and the design it drew.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomKind {
    Entry,
    Core,
    DoubleX,
    DoubleXY,
    DoubleY,
    DoubleYZ,
    DoubleZ,
    SimpleTop,
    Simple { main_design: i32 },
    Wing { main_design: i32 },
    Penthouse,
}

/// One child piece of the building.
#[derive(Clone, Debug)]
pub struct Room {
    pub kind: RoomKind,
    pub piece: Piece,
    /// Index into [`Monument::defs`]; `None` for the wings and the
    /// penthouse, which are built from boxes rather than grid cells.
    pub def: Option<usize>,
}

/// A monument start: the building and its rooms in `childPieces` order.
#[derive(Clone, Debug)]
pub struct Monument {
    pub start_chunk: (i32, i32),
    pub building: Piece,
    pub rooms: Vec<Room>,
    pub defs: Vec<RoomDef>,
}

/// `OceanMonumentPiece.makeBoundingBox(direction, definition, w, h, d)`.
fn room_box(facing: Facing, index: i32, w: i32, h: i32, d: i32) -> BoundingBox {
    let x = index % 5;
    let z = (index / 5) % 5;
    let y = index / 25;
    let mut b = make_bounding_box(0, 0, 0, facing, w * 8, h * 4, d * 8);
    match facing {
        Facing::North => b.shift(x * 8, y * 4, -(z + d) * 8 + 1),
        Facing::South => b.shift(x * 8, y * 4, z * 8),
        Facing::West => b.shift(-(z + d) * 8 + 1, y * 4, x * 8),
        Facing::East => b.shift(z * 8, y * 4, x * 8),
    }
    b
}

fn from_corners(a: (i32, i32, i32), b: (i32, i32, i32)) -> BoundingBox {
    BoundingBox::new(
        a.0.min(b.0),
        a.1.min(b.1),
        a.2.min(b.2),
        a.0.max(b.0),
        a.1.max(b.1),
        a.2.max(b.2),
    )
}

/// `MonumentBuilding.generateRoomGraph`. Returns the arena, the shuffled
/// list with the three specials appended, the source and the core.
fn generate_room_graph<R: RandomSource + ?Sized>(
    random: &mut R,
) -> (Vec<RoomDef>, Vec<usize>, usize, usize) {
    let mut defs: Vec<RoomDef> = Vec::new();
    let mut grid: [Option<usize>; 75] = [None; 75];
    let add = |defs: &mut Vec<RoomDef>, grid: &mut [Option<usize>; 75], idx: i32| {
        defs.push(RoomDef::new(idx));
        grid[idx as usize] = Some(defs.len() - 1);
    };
    for x in 0..5 {
        for z in 0..4 {
            add(&mut defs, &mut grid, room_index(x, 0, z));
        }
    }
    for x in 0..5 {
        for z in 0..4 {
            add(&mut defs, &mut grid, room_index(x, 1, z));
        }
    }
    for x in 1..4 {
        for z in 0..2 {
            add(&mut defs, &mut grid, room_index(x, 2, z));
        }
    }
    let source = grid[SOURCE_INDEX as usize].expect("the source cell exists");

    for x in 0..5 {
        for z in 0..5 {
            for y in 0..3 {
                let Some(here) = grid[room_index(x, y, z) as usize] else {
                    continue;
                };
                for (dir, dx, dy, dz) in DIRECTIONS {
                    let (nx, ny, nz) = (x + dx, y + dy, z + dz);
                    if !(0..5).contains(&nx) || !(0..5).contains(&nz) || !(0..3).contains(&ny) {
                        continue;
                    }
                    let Some(there) = grid[room_index(nx, ny, nz) as usize] else {
                        continue;
                    };
                    // The grid's +z is the monument's north: a step that
                    // moves in z connects through the opposite face.
                    if nz == z {
                        set_connection(&mut defs, here, dir, there);
                    } else {
                        set_connection(&mut defs, here, opposite(dir), there);
                    }
                }
            }
        }
    }

    defs.push(RoomDef::new(1003));
    let top = defs.len() - 1;
    defs.push(RoomDef::new(1001));
    let left = defs.len() - 1;
    defs.push(RoomDef::new(1002));
    let right = defs.len() - 1;
    let cell = |i: i32| grid[i as usize].expect("a connect cell exists");
    set_connection(&mut defs, cell(TOP_CONNECT_INDEX), UP, top);
    set_connection(&mut defs, cell(LEFTWING_CONNECT_INDEX), SOUTH, left);
    set_connection(&mut defs, cell(RIGHTWING_CONNECT_INDEX), SOUTH, right);
    defs[top].claimed = true;
    defs[left].claimed = true;
    defs[right].claimed = true;
    defs[source].is_source = true;

    let core = grid[room_index(random.next_i32_bound(4), 0, 2) as usize].expect("a core cell");
    let conn = |defs: &[RoomDef], r: usize, d: usize| defs[r].connections[d].expect("a core link");
    defs[core].claimed = true;
    let e = conn(&defs, core, EAST);
    let n = conn(&defs, core, NORTH);
    let en = conn(&defs, e, NORTH);
    let u = conn(&defs, core, UP);
    let eu = conn(&defs, e, UP);
    let nu = conn(&defs, n, UP);
    let enu = conn(&defs, en, UP);
    for r in [e, n, en, u, eu, nu, enu] {
        defs[r].claimed = true;
    }

    // Every non-null grid cell, in index order, then the top room's openings.
    let mut list: Vec<usize> = Vec::new();
    for slot in grid.iter().flatten() {
        update_openings(&mut defs, *slot);
        list.push(*slot);
    }
    update_openings(&mut defs, top);
    crate::structure::jigsaw::shuffle(&mut list, random);

    let mut scan = 1;
    for &room in &list {
        let mut closed = 0;
        let mut tries = 0;
        while closed < 2 && tries < 5 {
            tries += 1;
            let k = random.next_i32_bound(6) as usize;
            if !defs[room].has_opening[k] {
                continue;
            }
            let back = opposite(k);
            let other = defs[room].connections[k].expect("an opening has a connection");
            defs[room].has_opening[k] = false;
            defs[other].has_opening[back] = false;
            scan += 1;
            if find_source(&mut defs, room, scan) && {
                scan += 1;
                find_source(&mut defs, other, scan)
            } {
                closed += 1;
            } else {
                defs[room].has_opening[k] = true;
                defs[other].has_opening[back] = true;
            }
        }
    }
    list.push(top);
    list.push(left);
    list.push(right);
    (defs, list, source, core)
}

fn open_unclaimed(defs: &[RoomDef], r: usize, d: usize) -> bool {
    defs[r].has_opening[d] && defs[r].connections[d].is_some_and(|c| !defs[c].claimed)
}

fn link(defs: &[RoomDef], r: usize, d: usize) -> usize {
    defs[r].connections[d].expect("a fitted room's partner exists")
}

impl Monument {
    /// `new MonumentBuilding(random, chunkMinX - 29, chunkMinZ - 29,
    /// getRandomDirection(random))`.
    pub fn new<R: RandomSource + ?Sized>(start_chunk: (i32, i32), random: &mut R) -> Self {
        let (x, z) = (start_chunk.0 * 16 - 29, start_chunk.1 * 16 - 29);
        let facing = random_horizontal(random);
        let building = Piece {
            id: "minecraft:omb",
            bounding_box: make_bounding_box(x, 39, z, facing, 58, 23, 58),
            facing: Some(facing),
            depth: 0,
        };
        let (mut defs, list, source, core) = generate_room_graph(random);
        defs[source].claimed = true;

        let piece = |id: &'static str, bb: BoundingBox| Piece {
            id,
            bounding_box: bb,
            facing: Some(facing),
            depth: 1,
        };
        let mut rooms = vec![
            Room {
                kind: RoomKind::Entry,
                piece: piece(
                    "minecraft:omentry",
                    room_box(facing, defs[source].index, 1, 1, 1),
                ),
                def: Some(source),
            },
            Room {
                kind: RoomKind::Core,
                piece: piece(
                    "minecraft:omcr",
                    room_box(facing, defs[core].index, 2, 2, 2),
                ),
                def: Some(core),
            },
        ];

        for &r in &list {
            if defs[r].claimed || defs[r].is_special() {
                continue;
            }
            // The fitters, in `MonumentBuilding`'s list order; the first that
            // fits claims the cell.
            let room = if open_unclaimed(&defs, r, EAST)
                && open_unclaimed(&defs, r, UP)
                && open_unclaimed(&defs, link(&defs, r, EAST), UP)
            {
                let e = link(&defs, r, EAST);
                let (u, eu) = (link(&defs, r, UP), link(&defs, e, UP));
                for c in [r, e, u, eu] {
                    defs[c].claimed = true;
                }
                (RoomKind::DoubleXY, "minecraft:omdxyr", r, (2, 2, 1))
            } else if open_unclaimed(&defs, r, NORTH)
                && open_unclaimed(&defs, r, UP)
                && open_unclaimed(&defs, link(&defs, r, NORTH), UP)
            {
                let n = link(&defs, r, NORTH);
                let (u, nu) = (link(&defs, r, UP), link(&defs, n, UP));
                for c in [r, n, u, nu] {
                    defs[c].claimed = true;
                }
                (RoomKind::DoubleYZ, "minecraft:omdyzr", r, (1, 2, 2))
            } else if open_unclaimed(&defs, r, NORTH) {
                // `FitDoubleZRoom.create` re-tests the north side and, when it
                // is gone, anchors on the room to the south instead.
                let anchor = if defs[r].has_opening[NORTH] && !defs[link(&defs, r, NORTH)].claimed {
                    r
                } else {
                    link(&defs, r, SOUTH)
                };
                let n = link(&defs, anchor, NORTH);
                defs[anchor].claimed = true;
                defs[n].claimed = true;
                (RoomKind::DoubleZ, "minecraft:omdzr", anchor, (1, 1, 2))
            } else if open_unclaimed(&defs, r, EAST) {
                let e = link(&defs, r, EAST);
                defs[r].claimed = true;
                defs[e].claimed = true;
                (RoomKind::DoubleX, "minecraft:omdxr", r, (2, 1, 1))
            } else if open_unclaimed(&defs, r, UP) {
                let u = link(&defs, r, UP);
                defs[r].claimed = true;
                defs[u].claimed = true;
                (RoomKind::DoubleY, "minecraft:omdyr", r, (1, 2, 1))
            } else if !defs[r].has_opening[WEST]
                && !defs[r].has_opening[EAST]
                && !defs[r].has_opening[NORTH]
                && !defs[r].has_opening[SOUTH]
                && !defs[r].has_opening[UP]
            {
                defs[r].claimed = true;
                (RoomKind::SimpleTop, "minecraft:omsimplet", r, (1, 1, 1))
            } else {
                defs[r].claimed = true;
                let main_design = random.next_i32_bound(3);
                (
                    RoomKind::Simple { main_design },
                    "minecraft:omsimple",
                    r,
                    (1, 1, 1),
                )
            };
            let (kind, id, anchor, (w, h, d)) = room;
            rooms.push(Room {
                kind,
                piece: piece(id, room_box(facing, defs[anchor].index, w, h, d)),
                def: Some(anchor),
            });
        }

        // `getBoundingBox().move(getWorldPos(9, 0, 22))` on every grid room.
        let (ox, oy, oz) = building.world_pos(9, 0, 22);
        for room in &mut rooms {
            room.piece.bounding_box.shift(ox, oy, oz);
        }

        let left = from_corners(building.world_pos(1, 1, 1), building.world_pos(23, 8, 21));
        let right = from_corners(building.world_pos(34, 1, 1), building.world_pos(56, 8, 21));
        let top = from_corners(
            building.world_pos(22, 13, 22),
            building.world_pos(35, 17, 35),
        );
        // `new WingRoom(dir, box, i++)` twice — the post-increment hands the
        // first wing the drawn value itself, so the two designs are always
        // opposite and the first is the draw's low bit.
        let mut design = random.next_i32();
        rooms.push(Room {
            kind: RoomKind::Wing {
                main_design: design & 1,
            },
            piece: piece("minecraft:omwr", left),
            def: None,
        });
        design = design.wrapping_add(1);
        rooms.push(Room {
            kind: RoomKind::Wing {
                main_design: design & 1,
            },
            piece: piece("minecraft:omwr", right),
            def: None,
        });
        rooms.push(Room {
            kind: RoomKind::Penthouse,
            piece: piece("minecraft:ompenthouse", top),
            def: None,
        });

        Self {
            start_chunk,
            building,
            rooms,
            defs,
        }
    }
}

/// The block states every piece uses, resolved once.
pub struct Palette {
    pub gray: String,
    pub light: String,
    pub black: String,
    pub lamp: String,
    pub fill: String,
    pub air: String,
    pub gold: String,
    pub sponge: String,
}

impl Palette {
    /// `s` resolves a block name to its default state, as every other
    /// structure in this port does.
    pub fn new(s: &dyn Fn(&str) -> String) -> Self {
        Self {
            gray: s("minecraft:prismarine"),
            light: s("minecraft:prismarine_bricks"),
            black: s("minecraft:dark_prismarine"),
            lamp: s("minecraft:sea_lantern"),
            fill: s("minecraft:water"),
            air: s("minecraft:air"),
            gold: s("minecraft:gold_block"),
            sponge: s("minecraft:wet_sponge"),
        }
    }
}

fn plain(state: &str) -> &str {
    state.split_once('[').map_or(state, |(n, _)| n)
}

/// The `OceanMonumentPiece` helpers over one piece's writer.
struct Om<'w, 'a, W: StructureWorld + ?Sized> {
    w: &'w mut Writer<'a, W>,
    p: &'w Palette,
    sea_level: i32,
}

impl<W: StructureWorld + ?Sized> Om<'_, '_, W> {
    #[expect(clippy::too_many_arguments, reason = "the reference's own signature")]
    fn bx(&mut self, x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32, state: &str) {
        self.w
            .generate_box(x0, y0, z0, x1, y1, z1, state, state, false);
    }

    fn put(&mut self, state: &str, x: i32, y: i32, z: i32) {
        self.w.place_block(state, x, y, z);
    }

    /// `generateWaterBox`: keep ice and water, write water below the sea
    /// level and air at or above it.
    fn water(&mut self, x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    let here = self.w.get_block(x, y, z);
                    if matches!(
                        plain(&here),
                        "minecraft:ice"
                            | "minecraft:packed_ice"
                            | "minecraft:blue_ice"
                            | "minecraft:water"
                    ) {
                        continue;
                    }
                    if self.w.piece.world_y(y) >= self.sea_level && here != self.p.fill {
                        let air = self.p.air.clone();
                        self.put(&air, x, y, z);
                    } else {
                        let fill = self.p.fill.clone();
                        self.put(&fill, x, y, z);
                    }
                }
            }
        }
    }

    /// `generateDefaultFloor`.
    fn floor(&mut self, x: i32, z: i32, open_below: bool) {
        let (g, l) = (self.p.gray.clone(), self.p.light.clone());
        if open_below {
            self.bx(x, 0, z, x + 2, 0, z + 7, &g);
            self.bx(x + 5, 0, z, x + 7, 0, z + 7, &g);
            self.bx(x + 3, 0, z, x + 4, 0, z + 2, &g);
            self.bx(x + 3, 0, z + 5, x + 4, 0, z + 7, &g);
            self.bx(x + 3, 0, z + 2, x + 4, 0, z + 2, &l);
            self.bx(x + 3, 0, z + 5, x + 4, 0, z + 5, &l);
            self.bx(x + 2, 0, z + 3, x + 2, 0, z + 4, &l);
            self.bx(x + 5, 0, z + 3, x + 5, 0, z + 4, &l);
        } else {
            self.bx(x, 0, z, x + 7, 0, z + 7, &g);
        }
    }

    /// `generateBoxOnFillOnly` — overwrite only the water.
    #[expect(clippy::too_many_arguments, reason = "the reference's own signature")]
    fn on_fill(&mut self, x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32, state: &str) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    if self.w.get_block(x, y, z) == self.p.fill {
                        self.put(state, x, y, z);
                    }
                }
            }
        }
    }

    /// `chunkIntersects` — the 2D test over two local corners.
    fn chunk_intersects(&self, x0: i32, z0: i32, x1: i32, z1: i32) -> bool {
        let piece = self.w.piece;
        let (ax, az) = (piece.world_x(x0, z0), piece.world_z(x0, z0));
        let (bx, bz) = (piece.world_x(x1, z1), piece.world_z(x1, z1));
        let c = self.w.clip;
        c.max_x >= ax.min(bx)
            && c.min_x <= ax.max(bx)
            && c.max_z >= az.min(bz)
            && c.min_z <= az.max(bz)
    }
}

/// `MonumentBuilding.postProcess` for one chunk, children included.
pub fn place<W: StructureWorld + ?Sized, R: RandomSource + ?Sized>(
    world: &mut W,
    monument: &Monument,
    clip: BoundingBox,
    random: &mut R,
    palette: &Palette,
    sea_level: i32,
) {
    {
        let mut w = Writer {
            world: &mut *world,
            piece: &monument.building,
            clip,
            protected: &[],
        };
        let mut om = Om {
            w: &mut w,
            p: palette,
            sea_level,
        };
        building(&mut om);
    }
    for room in &monument.rooms {
        if !room.piece.bounding_box.intersects(clip) {
            continue;
        }
        let mut w = Writer {
            world: &mut *world,
            piece: &room.piece,
            clip,
            protected: &[],
        };
        let mut om = Om {
            w: &mut w,
            p: palette,
            sea_level,
        };
        let defs = &monument.defs;
        match room.kind {
            RoomKind::Entry => entry(&mut om, &defs[room.def.expect("a grid room")]),
            RoomKind::Core => core(&mut om),
            RoomKind::DoubleX => double_x(&mut om, defs, room.def.expect("a grid room")),
            RoomKind::DoubleXY => double_xy(&mut om, defs, room.def.expect("a grid room")),
            RoomKind::DoubleY => double_y(&mut om, defs, room.def.expect("a grid room")),
            RoomKind::DoubleYZ => double_yz(&mut om, defs, room.def.expect("a grid room")),
            RoomKind::DoubleZ => double_z(&mut om, defs, room.def.expect("a grid room")),
            RoomKind::SimpleTop => {
                simple_top(&mut om, &defs[room.def.expect("a grid room")], random)
            }
            RoomKind::Simple { main_design } => {
                simple(
                    &mut om,
                    &defs[room.def.expect("a grid room")],
                    main_design,
                    random,
                );
            }
            RoomKind::Wing { main_design } => wing_room(&mut om, main_design),
            RoomKind::Penthouse => penthouse(&mut om),
        }
    }
}

fn building<W: StructureWorld + ?Sized>(om: &mut Om<'_, '_, W>) {
    let min_y = om.w.piece.bounding_box.min_y;
    let top = om.sea_level.max(64) - min_y;
    om.water(0, 0, 0, 58, top, 58);
    wing(om, false, 0);
    wing(om, true, 33);
    entrance_archs(om);
    entrance_wall(om);
    roof_piece(om);
    lower_wall(om);
    middle_wall(om);
    upper_wall(om);

    let light = om.p.light.clone();
    for i in 0..7 {
        let mut j = 0;
        while j < 7 {
            if j == 0 && i == 3 {
                j = 6;
            }
            let (x0, z0) = (i * 9, j * 9);
            for k in 0..4 {
                for l in 0..4 {
                    om.put(&light, x0 + k, 0, z0 + l);
                    fill_column_down(om.w, &light, x0 + k, -1, z0 + l);
                }
            }
            if i == 0 || i == 6 {
                j += 1;
            } else {
                j += 6;
            }
        }
    }

    for i in 0..5 {
        om.water(-1 - i, i * 2, -1 - i, -1 - i, 23, 58 + i);
        om.water(58 + i, i * 2, -1 - i, 58 + i, 23, 58 + i);
        om.water(-i, i * 2, -1 - i, 57 + i, 23, -1 - i);
        om.water(-i, i * 2, 58 + i, 57 + i, 23, 58 + i);
    }
}

fn wing<W: StructureWorld + ?Sized>(om: &mut Om<'_, '_, W>, is_left: bool, x: i32) {
    if !om.chunk_intersects(x, 0, x + 23, 20) {
        return;
    }
    let (g, l, dot) = (om.p.gray.clone(), om.p.light.clone(), om.p.light.clone());
    om.bx(x, 0, 0, x + 24, 0, 20, &g);
    om.water(x, 1, 0, x + 24, 10, 20);
    for i in 0..4 {
        om.bx(x + i, i + 1, i, x + i, i + 1, 20, &l);
        om.bx(x + i + 7, i + 5, i + 7, x + i + 7, i + 5, 20, &l);
        om.bx(x + 17 - i, i + 5, i + 7, x + 17 - i, i + 5, 20, &l);
        om.bx(x + 24 - i, i + 1, i, x + 24 - i, i + 1, 20, &l);
        om.bx(x + i + 1, i + 1, i, x + 23 - i, i + 1, i, &l);
        om.bx(x + i + 8, i + 5, i + 7, x + 16 - i, i + 5, i + 7, &l);
    }
    om.bx(x + 4, 4, 4, x + 6, 4, 20, &g);
    om.bx(x + 7, 4, 4, x + 17, 4, 6, &g);
    om.bx(x + 18, 4, 4, x + 20, 4, 20, &g);
    om.bx(x + 11, 8, 11, x + 13, 8, 20, &g);
    om.put(&dot, x + 12, 9, 12);
    om.put(&dot, x + 12, 9, 15);
    om.put(&dot, x + 12, 9, 18);
    let a = x + if is_left { 19 } else { 5 };
    let b = x + if is_left { 5 } else { 19 };
    let mut z = 20;
    while z >= 5 {
        om.put(&dot, a, 5, z);
        z -= 3;
    }
    let mut z = 19;
    while z >= 7 {
        om.put(&dot, b, 5, z);
        z -= 3;
    }
    for i in 0..4 {
        let px = if is_left {
            x + 24 - (17 - i * 3)
        } else {
            x + 17 - i * 3
        };
        om.put(&dot, px, 5, 5);
    }
    om.put(&dot, b, 5, 5);
    om.bx(x + 11, 1, 12, x + 13, 7, 12, &g);
    om.bx(x + 12, 1, 11, x + 12, 7, 13, &g);
}

fn entrance_archs<W: StructureWorld + ?Sized>(om: &mut Om<'_, '_, W>) {
    if !om.chunk_intersects(22, 5, 35, 17) {
        return;
    }
    let (g, l, lamp) = (om.p.gray.clone(), om.p.light.clone(), om.p.lamp.clone());
    om.water(25, 0, 0, 32, 8, 20);
    for i in 0..4 {
        let z = 5 + i * 4;
        om.bx(24, 2, z, 24, 4, z, &l);
        om.bx(22, 4, z, 23, 4, z, &l);
        om.put(&l, 25, 5, z);
        om.put(&l, 26, 6, z);
        om.put(&lamp, 26, 5, z);
        om.bx(33, 2, z, 33, 4, z, &l);
        om.bx(34, 4, z, 35, 4, z, &l);
        om.put(&l, 32, 5, z);
        om.put(&l, 31, 6, z);
        om.put(&lamp, 31, 5, z);
        om.bx(27, 6, z, 30, 6, z, &g);
    }
}

fn entrance_wall<W: StructureWorld + ?Sized>(om: &mut Om<'_, '_, W>) {
    if !om.chunk_intersects(15, 20, 42, 21) {
        return;
    }
    let (g, l, k) = (om.p.gray.clone(), om.p.light.clone(), om.p.black.clone());
    om.bx(15, 0, 21, 42, 0, 21, &g);
    om.water(26, 1, 21, 31, 3, 21);
    om.bx(21, 12, 21, 36, 12, 21, &g);
    om.bx(17, 11, 21, 40, 11, 21, &g);
    om.bx(16, 10, 21, 41, 10, 21, &g);
    om.bx(15, 7, 21, 42, 9, 21, &g);
    om.bx(16, 6, 21, 41, 6, 21, &g);
    om.bx(17, 5, 21, 40, 5, 21, &g);
    om.bx(21, 4, 21, 36, 4, 21, &g);
    om.bx(22, 3, 21, 26, 3, 21, &g);
    om.bx(31, 3, 21, 35, 3, 21, &g);
    om.bx(23, 2, 21, 25, 2, 21, &g);
    om.bx(32, 2, 21, 34, 2, 21, &g);
    om.bx(28, 4, 20, 29, 4, 21, &l);
    om.put(&l, 27, 3, 21);
    om.put(&l, 30, 3, 21);
    om.put(&l, 26, 2, 21);
    om.put(&l, 31, 2, 21);
    om.put(&l, 25, 1, 21);
    om.put(&l, 32, 1, 21);
    for i in 0..7 {
        om.put(&k, 28 - i, 6 + i, 21);
        om.put(&k, 29 + i, 6 + i, 21);
    }
    for i in 0..4 {
        om.put(&k, 28 - i, 9 + i, 21);
        om.put(&k, 29 + i, 9 + i, 21);
    }
    om.put(&k, 28, 12, 21);
    om.put(&k, 29, 12, 21);
    for i in 0..3 {
        om.put(&k, 22 - i * 2, 8, 21);
        om.put(&k, 22 - i * 2, 9, 21);
        om.put(&k, 35 + i * 2, 8, 21);
        om.put(&k, 35 + i * 2, 9, 21);
    }
    om.water(15, 13, 21, 42, 15, 21);
    om.water(15, 1, 21, 15, 6, 21);
    om.water(16, 1, 21, 16, 5, 21);
    om.water(17, 1, 21, 20, 4, 21);
    om.water(21, 1, 21, 21, 3, 21);
    om.water(22, 1, 21, 22, 2, 21);
    om.water(23, 1, 21, 24, 1, 21);
    om.water(42, 1, 21, 42, 6, 21);
    om.water(41, 1, 21, 41, 5, 21);
    om.water(37, 1, 21, 40, 4, 21);
    om.water(36, 1, 21, 36, 3, 21);
    om.water(33, 1, 21, 34, 1, 21);
    om.water(35, 1, 21, 35, 2, 21);
}

fn roof_piece<W: StructureWorld + ?Sized>(om: &mut Om<'_, '_, W>) {
    if !om.chunk_intersects(21, 21, 36, 36) {
        return;
    }
    let (g, l, lamp) = (om.p.gray.clone(), om.p.light.clone(), om.p.lamp.clone());
    om.bx(21, 0, 22, 36, 0, 36, &g);
    om.water(21, 1, 22, 36, 23, 36);
    for i in 0..4 {
        om.bx(21 + i, 13 + i, 21 + i, 36 - i, 13 + i, 21 + i, &l);
        om.bx(21 + i, 13 + i, 36 - i, 36 - i, 13 + i, 36 - i, &l);
        om.bx(21 + i, 13 + i, 22 + i, 21 + i, 13 + i, 35 - i, &l);
        om.bx(36 - i, 13 + i, 22 + i, 36 - i, 13 + i, 35 - i, &l);
    }
    om.bx(25, 16, 25, 32, 16, 32, &g);
    om.bx(25, 17, 25, 25, 19, 25, &l);
    om.bx(32, 17, 25, 32, 19, 25, &l);
    om.bx(25, 17, 32, 25, 19, 32, &l);
    om.bx(32, 17, 32, 32, 19, 32, &l);
    om.put(&l, 26, 20, 26);
    om.put(&l, 27, 21, 27);
    om.put(&lamp, 27, 20, 27);
    om.put(&l, 26, 20, 31);
    om.put(&l, 27, 21, 30);
    om.put(&lamp, 27, 20, 30);
    om.put(&l, 31, 20, 31);
    om.put(&l, 30, 21, 30);
    om.put(&lamp, 30, 20, 30);
    om.put(&l, 31, 20, 26);
    om.put(&l, 30, 21, 27);
    om.put(&lamp, 30, 20, 27);
    om.bx(28, 21, 27, 29, 21, 27, &g);
    om.bx(27, 21, 28, 27, 21, 29, &g);
    om.bx(28, 21, 30, 29, 21, 30, &g);
    om.bx(30, 21, 28, 30, 21, 29, &g);
}

fn lower_wall<W: StructureWorld + ?Sized>(om: &mut Om<'_, '_, W>) {
    let (g, l, dot) = (om.p.gray.clone(), om.p.light.clone(), om.p.light.clone());
    if om.chunk_intersects(0, 21, 6, 58) {
        om.bx(0, 0, 21, 6, 0, 57, &g);
        om.water(0, 1, 21, 6, 7, 57);
        om.bx(4, 4, 21, 6, 4, 53, &g);
        for i in 0..4 {
            om.bx(i, i + 1, 21, i, i + 1, 57 - i, &l);
        }
        let mut z = 23;
        while z < 53 {
            om.put(&dot, 5, 5, z);
            z += 3;
        }
        om.put(&dot, 5, 5, 52);
        for i in 0..4 {
            om.bx(i, i + 1, 21, i, i + 1, 57 - i, &l);
        }
        om.bx(4, 1, 52, 6, 3, 52, &g);
        om.bx(5, 1, 51, 5, 3, 53, &g);
    }
    if om.chunk_intersects(51, 21, 58, 58) {
        om.bx(51, 0, 21, 57, 0, 57, &g);
        om.water(51, 1, 21, 57, 7, 57);
        om.bx(51, 4, 21, 53, 4, 53, &g);
        for i in 0..4 {
            om.bx(57 - i, i + 1, 21, 57 - i, i + 1, 57 - i, &l);
        }
        let mut z = 23;
        while z < 53 {
            om.put(&dot, 52, 5, z);
            z += 3;
        }
        om.put(&dot, 52, 5, 52);
        om.bx(51, 1, 52, 53, 3, 52, &g);
        om.bx(52, 1, 51, 52, 3, 53, &g);
    }
    if om.chunk_intersects(0, 51, 57, 57) {
        om.bx(7, 0, 51, 50, 0, 57, &g);
        om.water(7, 1, 51, 50, 10, 57);
        for i in 0..4 {
            om.bx(i + 1, i + 1, 57 - i, 56 - i, i + 1, 57 - i, &l);
        }
    }
}

fn middle_wall<W: StructureWorld + ?Sized>(om: &mut Om<'_, '_, W>) {
    let (g, l, dot) = (om.p.gray.clone(), om.p.light.clone(), om.p.light.clone());
    if om.chunk_intersects(7, 21, 13, 50) {
        om.bx(7, 0, 21, 13, 0, 50, &g);
        om.water(7, 1, 21, 13, 10, 50);
        om.bx(11, 8, 21, 13, 8, 53, &g);
        for i in 0..4 {
            om.bx(i + 7, i + 5, 21, i + 7, i + 5, 54, &l);
        }
        let mut z = 21;
        while z <= 45 {
            om.put(&dot, 12, 9, z);
            z += 3;
        }
    }
    if om.chunk_intersects(44, 21, 50, 54) {
        om.bx(44, 0, 21, 50, 0, 50, &g);
        om.water(44, 1, 21, 50, 10, 50);
        om.bx(44, 8, 21, 46, 8, 53, &g);
        for i in 0..4 {
            om.bx(50 - i, i + 5, 21, 50 - i, i + 5, 54, &l);
        }
        let mut z = 21;
        while z <= 45 {
            om.put(&dot, 45, 9, z);
            z += 3;
        }
    }
    if om.chunk_intersects(8, 44, 49, 54) {
        om.bx(14, 0, 44, 43, 0, 50, &g);
        om.water(14, 1, 44, 43, 10, 50);
        let mut x = 12;
        while x <= 45 {
            om.put(&dot, x, 9, 45);
            om.put(&dot, x, 9, 52);
            if matches!(x, 12 | 18 | 24 | 33 | 39 | 45) {
                om.put(&dot, x, 9, 47);
                om.put(&dot, x, 9, 50);
                om.put(&dot, x, 10, 45);
                om.put(&dot, x, 10, 46);
                om.put(&dot, x, 10, 51);
                om.put(&dot, x, 10, 52);
                om.put(&dot, x, 11, 47);
                om.put(&dot, x, 11, 50);
                om.put(&dot, x, 12, 48);
                om.put(&dot, x, 12, 49);
            }
            x += 3;
        }
        for i in 0..3 {
            om.bx(8 + i, 5 + i, 54, 49 - i, 5 + i, 54, &g);
        }
        om.bx(11, 8, 54, 46, 8, 54, &l);
        om.bx(14, 8, 44, 43, 8, 53, &g);
    }
}

fn upper_wall<W: StructureWorld + ?Sized>(om: &mut Om<'_, '_, W>) {
    let (g, l, dot) = (om.p.gray.clone(), om.p.light.clone(), om.p.light.clone());
    if om.chunk_intersects(14, 21, 20, 43) {
        om.bx(14, 0, 21, 20, 0, 43, &g);
        om.water(14, 1, 22, 20, 14, 43);
        om.bx(18, 12, 22, 20, 12, 39, &g);
        om.bx(18, 12, 21, 20, 12, 21, &l);
        for i in 0..4 {
            om.bx(i + 14, i + 9, 21, i + 14, i + 9, 43 - i, &l);
        }
        let mut z = 23;
        while z <= 39 {
            om.put(&dot, 19, 13, z);
            z += 3;
        }
    }
    if om.chunk_intersects(37, 21, 43, 43) {
        om.bx(37, 0, 21, 43, 0, 43, &g);
        om.water(37, 1, 22, 43, 14, 43);
        om.bx(37, 12, 22, 39, 12, 39, &g);
        om.bx(37, 12, 21, 39, 12, 21, &l);
        for i in 0..4 {
            om.bx(43 - i, i + 9, 21, 43 - i, i + 9, 43 - i, &l);
        }
        let mut z = 23;
        while z <= 39 {
            om.put(&dot, 38, 13, z);
            z += 3;
        }
    }
    if om.chunk_intersects(15, 37, 42, 43) {
        om.bx(21, 0, 37, 36, 0, 43, &g);
        om.water(21, 1, 37, 36, 14, 43);
        om.bx(21, 12, 37, 36, 12, 39, &g);
        for i in 0..4 {
            om.bx(15 + i, i + 9, 43 - i, 42 - i, i + 9, 43 - i, &l);
        }
        let mut x = 21;
        while x <= 36 {
            om.put(&dot, x, 13, 38);
            x += 3;
        }
    }
}

fn entry<W: StructureWorld + ?Sized>(om: &mut Om<'_, '_, W>, def: &RoomDef) {
    let l = om.p.light.clone();
    om.bx(0, 3, 0, 2, 3, 7, &l);
    om.bx(5, 3, 0, 7, 3, 7, &l);
    om.bx(0, 2, 0, 1, 2, 7, &l);
    om.bx(6, 2, 0, 7, 2, 7, &l);
    om.bx(0, 1, 0, 0, 1, 7, &l);
    om.bx(7, 1, 0, 7, 1, 7, &l);
    om.bx(0, 1, 7, 7, 3, 7, &l);
    om.bx(1, 1, 0, 2, 3, 0, &l);
    om.bx(5, 1, 0, 6, 3, 0, &l);
    if def.has_opening[NORTH] {
        om.water(3, 1, 7, 4, 2, 7);
    }
    if def.has_opening[WEST] {
        om.water(0, 1, 3, 1, 2, 4);
    }
    if def.has_opening[EAST] {
        om.water(6, 1, 3, 7, 2, 4);
    }
}

fn core<W: StructureWorld + ?Sized>(om: &mut Om<'_, '_, W>) {
    let (g, l, k, lamp, gold) = (
        om.p.gray.clone(),
        om.p.light.clone(),
        om.p.black.clone(),
        om.p.lamp.clone(),
        om.p.gold.clone(),
    );
    om.on_fill(1, 8, 0, 14, 8, 14, &g);
    om.bx(0, 7, 0, 0, 7, 15, &l);
    om.bx(15, 7, 0, 15, 7, 15, &l);
    om.bx(1, 7, 0, 15, 7, 0, &l);
    om.bx(1, 7, 15, 14, 7, 15, &l);
    for y in 1..=6 {
        let s = if y == 2 || y == 6 { &g } else { &l };
        let mut x = 0;
        while x <= 15 {
            om.bx(x, y, 0, x, y, 1, s);
            om.bx(x, y, 6, x, y, 9, s);
            om.bx(x, y, 14, x, y, 15, s);
            x += 15;
        }
        om.bx(1, y, 0, 1, y, 0, s);
        om.bx(6, y, 0, 9, y, 0, s);
        om.bx(14, y, 0, 14, y, 0, s);
        om.bx(1, y, 15, 14, y, 15, s);
    }
    om.bx(6, 3, 6, 9, 6, 9, &k);
    om.bx(7, 4, 7, 8, 5, 8, &gold);
    let mut y = 3;
    while y <= 6 {
        let mut x = 6;
        while x <= 9 {
            om.put(&lamp, x, y, 6);
            om.put(&lamp, x, y, 9);
            x += 3;
        }
        y += 3;
    }
    om.bx(5, 1, 6, 5, 2, 6, &l);
    om.bx(5, 1, 9, 5, 2, 9, &l);
    om.bx(10, 1, 6, 10, 2, 6, &l);
    om.bx(10, 1, 9, 10, 2, 9, &l);
    om.bx(6, 1, 5, 6, 2, 5, &l);
    om.bx(9, 1, 5, 9, 2, 5, &l);
    om.bx(6, 1, 10, 6, 2, 10, &l);
    om.bx(9, 1, 10, 9, 2, 10, &l);
    om.bx(5, 2, 5, 5, 6, 5, &l);
    om.bx(5, 2, 10, 5, 6, 10, &l);
    om.bx(10, 2, 5, 10, 6, 5, &l);
    om.bx(10, 2, 10, 10, 6, 10, &l);
    om.bx(5, 7, 1, 5, 7, 6, &l);
    om.bx(10, 7, 1, 10, 7, 6, &l);
    om.bx(5, 7, 9, 5, 7, 14, &l);
    om.bx(10, 7, 9, 10, 7, 14, &l);
    om.bx(1, 7, 5, 6, 7, 5, &l);
    om.bx(1, 7, 10, 6, 7, 10, &l);
    om.bx(9, 7, 5, 14, 7, 5, &l);
    om.bx(9, 7, 10, 14, 7, 10, &l);
    om.bx(2, 1, 2, 2, 1, 3, &l);
    om.bx(3, 1, 2, 3, 1, 2, &l);
    om.bx(13, 1, 2, 13, 1, 3, &l);
    om.bx(12, 1, 2, 12, 1, 2, &l);
    om.bx(2, 1, 12, 2, 1, 13, &l);
    om.bx(3, 1, 13, 3, 1, 13, &l);
    om.bx(13, 1, 12, 13, 1, 13, &l);
    om.bx(12, 1, 13, 12, 1, 13, &l);
}

fn double_x<W: StructureWorld + ?Sized>(om: &mut Om<'_, '_, W>, defs: &[RoomDef], me: usize) {
    let (g, l, lamp) = (om.p.gray.clone(), om.p.light.clone(), om.p.lamp.clone());
    let a = &defs[me];
    let b = &defs[link(defs, me, EAST)];
    if a.index / 25 > 0 {
        om.floor(8, 0, b.has_opening[DOWN]);
        om.floor(0, 0, a.has_opening[DOWN]);
    }
    if a.connections[UP].is_none() {
        om.on_fill(1, 4, 1, 7, 4, 6, &g);
    }
    if b.connections[UP].is_none() {
        om.on_fill(8, 4, 1, 14, 4, 6, &g);
    }
    om.bx(0, 3, 0, 0, 3, 7, &l);
    om.bx(15, 3, 0, 15, 3, 7, &l);
    om.bx(1, 3, 0, 15, 3, 0, &l);
    om.bx(1, 3, 7, 14, 3, 7, &l);
    om.bx(0, 2, 0, 0, 2, 7, &g);
    om.bx(15, 2, 0, 15, 2, 7, &g);
    om.bx(1, 2, 0, 15, 2, 0, &g);
    om.bx(1, 2, 7, 14, 2, 7, &g);
    om.bx(0, 1, 0, 0, 1, 7, &l);
    om.bx(15, 1, 0, 15, 1, 7, &l);
    om.bx(1, 1, 0, 15, 1, 0, &l);
    om.bx(1, 1, 7, 14, 1, 7, &l);
    om.bx(5, 1, 0, 10, 1, 4, &l);
    om.bx(6, 2, 0, 9, 2, 3, &g);
    om.bx(5, 3, 0, 10, 3, 4, &l);
    om.put(&lamp, 6, 2, 3);
    om.put(&lamp, 9, 2, 3);
    if a.has_opening[SOUTH] {
        om.water(3, 1, 0, 4, 2, 0);
    }
    if a.has_opening[NORTH] {
        om.water(3, 1, 7, 4, 2, 7);
    }
    if a.has_opening[WEST] {
        om.water(0, 1, 3, 0, 2, 4);
    }
    if b.has_opening[SOUTH] {
        om.water(11, 1, 0, 12, 2, 0);
    }
    if b.has_opening[NORTH] {
        om.water(11, 1, 7, 12, 2, 7);
    }
    if b.has_opening[EAST] {
        om.water(15, 1, 3, 15, 2, 4);
    }
}

fn double_xy<W: StructureWorld + ?Sized>(om: &mut Om<'_, '_, W>, defs: &[RoomDef], me: usize) {
    let (g, l, lamp) = (om.p.gray.clone(), om.p.light.clone(), om.p.lamp.clone());
    let e = link(defs, me, EAST);
    let (a, b) = (&defs[me], &defs[e]);
    let up = &defs[link(defs, me, UP)];
    let eup = &defs[link(defs, e, UP)];
    if a.index / 25 > 0 {
        om.floor(8, 0, b.has_opening[DOWN]);
        om.floor(0, 0, a.has_opening[DOWN]);
    }
    if up.connections[UP].is_none() {
        om.on_fill(1, 8, 1, 7, 8, 6, &g);
    }
    if eup.connections[UP].is_none() {
        om.on_fill(8, 8, 1, 14, 8, 6, &g);
    }
    for y in 1..=7 {
        let s = if y == 2 || y == 6 { &g } else { &l };
        om.bx(0, y, 0, 0, y, 7, s);
        om.bx(15, y, 0, 15, y, 7, s);
        om.bx(1, y, 0, 15, y, 0, s);
        om.bx(1, y, 7, 14, y, 7, s);
    }
    om.bx(2, 1, 3, 2, 7, 4, &l);
    om.bx(3, 1, 2, 4, 7, 2, &l);
    om.bx(3, 1, 5, 4, 7, 5, &l);
    om.bx(13, 1, 3, 13, 7, 4, &l);
    om.bx(11, 1, 2, 12, 7, 2, &l);
    om.bx(11, 1, 5, 12, 7, 5, &l);
    om.bx(5, 1, 3, 5, 3, 4, &l);
    om.bx(10, 1, 3, 10, 3, 4, &l);
    om.bx(5, 7, 2, 10, 7, 5, &l);
    om.bx(5, 5, 2, 5, 7, 2, &l);
    om.bx(10, 5, 2, 10, 7, 2, &l);
    om.bx(5, 5, 5, 5, 7, 5, &l);
    om.bx(10, 5, 5, 10, 7, 5, &l);
    om.put(&l, 6, 6, 2);
    om.put(&l, 9, 6, 2);
    om.put(&l, 6, 6, 5);
    om.put(&l, 9, 6, 5);
    om.bx(5, 4, 3, 6, 4, 4, &l);
    om.bx(9, 4, 3, 10, 4, 4, &l);
    om.put(&lamp, 5, 4, 2);
    om.put(&lamp, 5, 4, 5);
    om.put(&lamp, 10, 4, 2);
    om.put(&lamp, 10, 4, 5);
    if a.has_opening[SOUTH] {
        om.water(3, 1, 0, 4, 2, 0);
    }
    if a.has_opening[NORTH] {
        om.water(3, 1, 7, 4, 2, 7);
    }
    if a.has_opening[WEST] {
        om.water(0, 1, 3, 0, 2, 4);
    }
    if b.has_opening[SOUTH] {
        om.water(11, 1, 0, 12, 2, 0);
    }
    if b.has_opening[NORTH] {
        om.water(11, 1, 7, 12, 2, 7);
    }
    if b.has_opening[EAST] {
        om.water(15, 1, 3, 15, 2, 4);
    }
    if up.has_opening[SOUTH] {
        om.water(3, 5, 0, 4, 6, 0);
    }
    if up.has_opening[NORTH] {
        om.water(3, 5, 7, 4, 6, 7);
    }
    if up.has_opening[WEST] {
        om.water(0, 5, 3, 0, 6, 4);
    }
    if eup.has_opening[SOUTH] {
        om.water(11, 5, 0, 12, 6, 0);
    }
    if eup.has_opening[NORTH] {
        om.water(11, 5, 7, 12, 6, 7);
    }
    if eup.has_opening[EAST] {
        om.water(15, 5, 3, 15, 6, 4);
    }
}

fn double_y<W: StructureWorld + ?Sized>(om: &mut Om<'_, '_, W>, defs: &[RoomDef], me: usize) {
    let (g, l) = (om.p.gray.clone(), om.p.light.clone());
    let a = &defs[me];
    if a.index / 25 > 0 {
        om.floor(0, 0, a.has_opening[DOWN]);
    }
    let up = link(defs, me, UP);
    if defs[up].connections[UP].is_none() {
        om.on_fill(1, 8, 1, 6, 8, 6, &g);
    }
    om.bx(0, 4, 0, 0, 4, 7, &l);
    om.bx(7, 4, 0, 7, 4, 7, &l);
    om.bx(1, 4, 0, 6, 4, 0, &l);
    om.bx(1, 4, 7, 6, 4, 7, &l);
    om.bx(2, 4, 1, 2, 4, 2, &l);
    om.bx(1, 4, 2, 1, 4, 2, &l);
    om.bx(5, 4, 1, 5, 4, 2, &l);
    om.bx(6, 4, 2, 6, 4, 2, &l);
    om.bx(2, 4, 5, 2, 4, 6, &l);
    om.bx(1, 4, 5, 1, 4, 5, &l);
    om.bx(5, 4, 5, 5, 4, 6, &l);
    om.bx(6, 4, 5, 6, 4, 5, &l);
    let mut def = me;
    let mut y = 1;
    while y <= 5 {
        let d = &defs[def];
        for (open, z) in [(d.has_opening[SOUTH], 0), (d.has_opening[NORTH], 7)] {
            if open {
                om.bx(2, y, z, 2, y + 2, z, &l);
                om.bx(5, y, z, 5, y + 2, z, &l);
                om.bx(3, y + 2, z, 4, y + 2, z, &l);
            } else {
                om.bx(0, y, z, 7, y + 2, z, &l);
                om.bx(0, y + 1, z, 7, y + 1, z, &g);
            }
        }
        for (open, x) in [(d.has_opening[WEST], 0), (d.has_opening[EAST], 7)] {
            if open {
                om.bx(x, y, 2, x, y + 2, 2, &l);
                om.bx(x, y, 5, x, y + 2, 5, &l);
                om.bx(x, y + 2, 3, x, y + 2, 4, &l);
            } else {
                om.bx(x, y, 0, x, y + 2, 7, &l);
                om.bx(x, y + 1, 0, x, y + 1, 7, &g);
            }
        }
        def = up;
        y += 4;
    }
}

fn double_yz<W: StructureWorld + ?Sized>(om: &mut Om<'_, '_, W>, defs: &[RoomDef], me: usize) {
    let (g, l, k, lamp) = (
        om.p.gray.clone(),
        om.p.light.clone(),
        om.p.black.clone(),
        om.p.lamp.clone(),
    );
    let n = link(defs, me, NORTH);
    let (a, b) = (&defs[me], &defs[n]);
    let nup = &defs[link(defs, n, UP)];
    let up = &defs[link(defs, me, UP)];
    if a.index / 25 > 0 {
        om.floor(0, 8, b.has_opening[DOWN]);
        om.floor(0, 0, a.has_opening[DOWN]);
    }
    if up.connections[UP].is_none() {
        om.on_fill(1, 8, 1, 6, 8, 7, &g);
    }
    if nup.connections[UP].is_none() {
        om.on_fill(1, 8, 8, 6, 8, 14, &g);
    }
    for y in 1..=7 {
        let s = if y == 2 || y == 6 { &g } else { &l };
        om.bx(0, y, 0, 0, y, 15, s);
        om.bx(7, y, 0, 7, y, 15, s);
        om.bx(1, y, 0, 6, y, 0, s);
        om.bx(1, y, 15, 6, y, 15, s);
    }
    for y in 1..=7 {
        let s = if y == 2 || y == 6 { &lamp } else { &k };
        om.bx(3, y, 7, 4, y, 8, s);
    }
    if a.has_opening[SOUTH] {
        om.water(3, 1, 0, 4, 2, 0);
    }
    if a.has_opening[EAST] {
        om.water(7, 1, 3, 7, 2, 4);
    }
    if a.has_opening[WEST] {
        om.water(0, 1, 3, 0, 2, 4);
    }
    if b.has_opening[NORTH] {
        om.water(3, 1, 15, 4, 2, 15);
    }
    if b.has_opening[WEST] {
        om.water(0, 1, 11, 0, 2, 12);
    }
    if b.has_opening[EAST] {
        om.water(7, 1, 11, 7, 2, 12);
    }
    if up.has_opening[SOUTH] {
        om.water(3, 5, 0, 4, 6, 0);
    }
    if up.has_opening[EAST] {
        om.water(7, 5, 3, 7, 6, 4);
        om.bx(5, 4, 2, 6, 4, 5, &l);
        om.bx(6, 1, 2, 6, 3, 2, &l);
        om.bx(6, 1, 5, 6, 3, 5, &l);
    }
    if up.has_opening[WEST] {
        om.water(0, 5, 3, 0, 6, 4);
        om.bx(1, 4, 2, 2, 4, 5, &l);
        om.bx(1, 1, 2, 1, 3, 2, &l);
        om.bx(1, 1, 5, 1, 3, 5, &l);
    }
    if nup.has_opening[NORTH] {
        om.water(3, 5, 15, 4, 6, 15);
    }
    if nup.has_opening[WEST] {
        om.water(0, 5, 11, 0, 6, 12);
        om.bx(1, 4, 10, 2, 4, 13, &l);
        om.bx(1, 1, 10, 1, 3, 10, &l);
        om.bx(1, 1, 13, 1, 3, 13, &l);
    }
    if nup.has_opening[EAST] {
        om.water(7, 5, 11, 7, 6, 12);
        om.bx(5, 4, 10, 6, 4, 13, &l);
        om.bx(6, 1, 10, 6, 3, 10, &l);
        om.bx(6, 1, 13, 6, 3, 13, &l);
    }
}

fn double_z<W: StructureWorld + ?Sized>(om: &mut Om<'_, '_, W>, defs: &[RoomDef], me: usize) {
    let (g, l, lamp) = (om.p.gray.clone(), om.p.light.clone(), om.p.lamp.clone());
    let n = link(defs, me, NORTH);
    let (a, b) = (&defs[me], &defs[n]);
    if a.index / 25 > 0 {
        om.floor(0, 8, b.has_opening[DOWN]);
        om.floor(0, 0, a.has_opening[DOWN]);
    }
    if a.connections[UP].is_none() {
        om.on_fill(1, 4, 1, 6, 4, 7, &g);
    }
    if b.connections[UP].is_none() {
        om.on_fill(1, 4, 8, 6, 4, 14, &g);
    }
    om.bx(0, 3, 0, 0, 3, 15, &l);
    om.bx(7, 3, 0, 7, 3, 15, &l);
    om.bx(1, 3, 0, 7, 3, 0, &l);
    om.bx(1, 3, 15, 6, 3, 15, &l);
    om.bx(0, 2, 0, 0, 2, 15, &g);
    om.bx(7, 2, 0, 7, 2, 15, &g);
    om.bx(1, 2, 0, 7, 2, 0, &g);
    om.bx(1, 2, 15, 6, 2, 15, &g);
    om.bx(0, 1, 0, 0, 1, 15, &l);
    om.bx(7, 1, 0, 7, 1, 15, &l);
    om.bx(1, 1, 0, 7, 1, 0, &l);
    om.bx(1, 1, 15, 6, 1, 15, &l);
    om.bx(1, 1, 1, 1, 1, 2, &l);
    om.bx(6, 1, 1, 6, 1, 2, &l);
    om.bx(1, 3, 1, 1, 3, 2, &l);
    om.bx(6, 3, 1, 6, 3, 2, &l);
    om.bx(1, 1, 13, 1, 1, 14, &l);
    om.bx(6, 1, 13, 6, 1, 14, &l);
    om.bx(1, 3, 13, 1, 3, 14, &l);
    om.bx(6, 3, 13, 6, 3, 14, &l);
    om.bx(2, 1, 6, 2, 3, 6, &l);
    om.bx(5, 1, 6, 5, 3, 6, &l);
    om.bx(2, 1, 9, 2, 3, 9, &l);
    om.bx(5, 1, 9, 5, 3, 9, &l);
    om.bx(3, 2, 6, 4, 2, 6, &l);
    om.bx(3, 2, 9, 4, 2, 9, &l);
    om.bx(2, 2, 7, 2, 2, 8, &l);
    om.bx(5, 2, 7, 5, 2, 8, &l);
    om.put(&lamp, 2, 2, 5);
    om.put(&lamp, 5, 2, 5);
    om.put(&lamp, 2, 2, 10);
    om.put(&lamp, 5, 2, 10);
    om.put(&l, 2, 3, 5);
    om.put(&l, 5, 3, 5);
    om.put(&l, 2, 3, 10);
    om.put(&l, 5, 3, 10);
    if a.has_opening[SOUTH] {
        om.water(3, 1, 0, 4, 2, 0);
    }
    if a.has_opening[EAST] {
        om.water(7, 1, 3, 7, 2, 4);
    }
    if a.has_opening[WEST] {
        om.water(0, 1, 3, 0, 2, 4);
    }
    if b.has_opening[NORTH] {
        om.water(3, 1, 15, 4, 2, 15);
    }
    if b.has_opening[WEST] {
        om.water(0, 1, 11, 0, 2, 12);
    }
    if b.has_opening[EAST] {
        om.water(7, 1, 11, 7, 2, 12);
    }
}

fn simple_top<W: StructureWorld + ?Sized, R: RandomSource + ?Sized>(
    om: &mut Om<'_, '_, W>,
    def: &RoomDef,
    random: &mut R,
) {
    let (g, l, k, sponge) = (
        om.p.gray.clone(),
        om.p.light.clone(),
        om.p.black.clone(),
        om.p.sponge.clone(),
    );
    if def.index / 25 > 0 {
        om.floor(0, 0, def.has_opening[DOWN]);
    }
    if def.connections[UP].is_none() {
        om.on_fill(1, 4, 1, 6, 4, 6, &g);
    }
    for x in 1..=6 {
        for z in 1..=6 {
            if random.next_i32_bound(3) != 0 {
                let y = 2 + i32::from(random.next_i32_bound(4) != 0);
                om.bx(x, y, z, x, 3, z, &sponge);
            }
        }
    }
    om.bx(0, 1, 0, 0, 1, 7, &l);
    om.bx(7, 1, 0, 7, 1, 7, &l);
    om.bx(1, 1, 0, 6, 1, 0, &l);
    om.bx(1, 1, 7, 6, 1, 7, &l);
    om.bx(0, 2, 0, 0, 2, 7, &k);
    om.bx(7, 2, 0, 7, 2, 7, &k);
    om.bx(1, 2, 0, 6, 2, 0, &k);
    om.bx(1, 2, 7, 6, 2, 7, &k);
    om.bx(0, 3, 0, 0, 3, 7, &l);
    om.bx(7, 3, 0, 7, 3, 7, &l);
    om.bx(1, 3, 0, 6, 3, 0, &l);
    om.bx(1, 3, 7, 6, 3, 7, &l);
    om.bx(0, 1, 3, 0, 2, 4, &k);
    om.bx(7, 1, 3, 7, 2, 4, &k);
    om.bx(3, 1, 0, 4, 2, 0, &k);
    om.bx(3, 1, 7, 4, 2, 7, &k);
    if def.has_opening[SOUTH] {
        om.water(3, 1, 0, 4, 2, 0);
    }
}

fn simple<W: StructureWorld + ?Sized, R: RandomSource + ?Sized>(
    om: &mut Om<'_, '_, W>,
    def: &RoomDef,
    main_design: i32,
    random: &mut R,
) {
    let (g, l, k, lamp) = (
        om.p.gray.clone(),
        om.p.light.clone(),
        om.p.black.clone(),
        om.p.lamp.clone(),
    );
    if def.index / 25 > 0 {
        om.floor(0, 0, def.has_opening[DOWN]);
    }
    if def.connections[UP].is_none() {
        om.on_fill(1, 4, 1, 6, 4, 6, &g);
    }
    // `mainDesign != 0 && random.nextBoolean() && ...` — the draw is spent
    // only when the design is not 0, and before the opening tests.
    let pillar = main_design != 0
        && random.next_bool()
        && !def.has_opening[DOWN]
        && !def.has_opening[UP]
        && def.count_openings() > 1;
    match main_design {
        0 => {
            om.bx(0, 1, 0, 2, 1, 2, &l);
            om.bx(0, 3, 0, 2, 3, 2, &l);
            om.bx(0, 2, 0, 0, 2, 2, &g);
            om.bx(1, 2, 0, 2, 2, 0, &g);
            om.put(&lamp, 1, 2, 1);
            om.bx(5, 1, 0, 7, 1, 2, &l);
            om.bx(5, 3, 0, 7, 3, 2, &l);
            om.bx(7, 2, 0, 7, 2, 2, &g);
            om.bx(5, 2, 0, 6, 2, 0, &g);
            om.put(&lamp, 6, 2, 1);
            om.bx(0, 1, 5, 2, 1, 7, &l);
            om.bx(0, 3, 5, 2, 3, 7, &l);
            om.bx(0, 2, 5, 0, 2, 7, &g);
            om.bx(1, 2, 7, 2, 2, 7, &g);
            om.put(&lamp, 1, 2, 6);
            om.bx(5, 1, 5, 7, 1, 7, &l);
            om.bx(5, 3, 5, 7, 3, 7, &l);
            om.bx(7, 2, 5, 7, 2, 7, &g);
            om.bx(5, 2, 7, 6, 2, 7, &g);
            om.put(&lamp, 6, 2, 6);
            if def.has_opening[SOUTH] {
                om.bx(3, 3, 0, 4, 3, 0, &l);
            } else {
                om.bx(3, 3, 0, 4, 3, 1, &l);
                om.bx(3, 2, 0, 4, 2, 0, &g);
                om.bx(3, 1, 0, 4, 1, 1, &l);
            }
            if def.has_opening[NORTH] {
                om.bx(3, 3, 7, 4, 3, 7, &l);
            } else {
                om.bx(3, 3, 6, 4, 3, 7, &l);
                om.bx(3, 2, 7, 4, 2, 7, &g);
                om.bx(3, 1, 6, 4, 1, 7, &l);
            }
            if def.has_opening[WEST] {
                om.bx(0, 3, 3, 0, 3, 4, &l);
            } else {
                om.bx(0, 3, 3, 1, 3, 4, &l);
                om.bx(0, 2, 3, 0, 2, 4, &g);
                om.bx(0, 1, 3, 1, 1, 4, &l);
            }
            if def.has_opening[EAST] {
                om.bx(7, 3, 3, 7, 3, 4, &l);
            } else {
                om.bx(6, 3, 3, 7, 3, 4, &l);
                om.bx(7, 2, 3, 7, 2, 4, &g);
                om.bx(6, 1, 3, 7, 1, 4, &l);
            }
        }
        1 => {
            om.bx(2, 1, 2, 2, 3, 2, &l);
            om.bx(2, 1, 5, 2, 3, 5, &l);
            om.bx(5, 1, 5, 5, 3, 5, &l);
            om.bx(5, 1, 2, 5, 3, 2, &l);
            om.put(&lamp, 2, 2, 2);
            om.put(&lamp, 2, 2, 5);
            om.put(&lamp, 5, 2, 5);
            om.put(&lamp, 5, 2, 2);
            om.bx(0, 1, 0, 1, 3, 0, &l);
            om.bx(0, 1, 1, 0, 3, 1, &l);
            om.bx(0, 1, 7, 1, 3, 7, &l);
            om.bx(0, 1, 6, 0, 3, 6, &l);
            om.bx(6, 1, 7, 7, 3, 7, &l);
            om.bx(7, 1, 6, 7, 3, 6, &l);
            om.bx(6, 1, 0, 7, 3, 0, &l);
            om.bx(7, 1, 1, 7, 3, 1, &l);
            om.put(&g, 1, 2, 0);
            om.put(&g, 0, 2, 1);
            om.put(&g, 1, 2, 7);
            om.put(&g, 0, 2, 6);
            om.put(&g, 6, 2, 7);
            om.put(&g, 7, 2, 6);
            om.put(&g, 6, 2, 0);
            om.put(&g, 7, 2, 1);
            if !def.has_opening[SOUTH] {
                om.bx(1, 3, 0, 6, 3, 0, &l);
                om.bx(1, 2, 0, 6, 2, 0, &g);
                om.bx(1, 1, 0, 6, 1, 0, &l);
            }
            if !def.has_opening[NORTH] {
                om.bx(1, 3, 7, 6, 3, 7, &l);
                om.bx(1, 2, 7, 6, 2, 7, &g);
                om.bx(1, 1, 7, 6, 1, 7, &l);
            }
            if !def.has_opening[WEST] {
                om.bx(0, 3, 1, 0, 3, 6, &l);
                om.bx(0, 2, 1, 0, 2, 6, &g);
                om.bx(0, 1, 1, 0, 1, 6, &l);
            }
            if !def.has_opening[EAST] {
                om.bx(7, 3, 1, 7, 3, 6, &l);
                om.bx(7, 2, 1, 7, 2, 6, &g);
                om.bx(7, 1, 1, 7, 1, 6, &l);
            }
        }
        2 => {
            om.bx(0, 1, 0, 0, 1, 7, &l);
            om.bx(7, 1, 0, 7, 1, 7, &l);
            om.bx(1, 1, 0, 6, 1, 0, &l);
            om.bx(1, 1, 7, 6, 1, 7, &l);
            om.bx(0, 2, 0, 0, 2, 7, &k);
            om.bx(7, 2, 0, 7, 2, 7, &k);
            om.bx(1, 2, 0, 6, 2, 0, &k);
            om.bx(1, 2, 7, 6, 2, 7, &k);
            om.bx(0, 3, 0, 0, 3, 7, &l);
            om.bx(7, 3, 0, 7, 3, 7, &l);
            om.bx(1, 3, 0, 6, 3, 0, &l);
            om.bx(1, 3, 7, 6, 3, 7, &l);
            om.bx(0, 1, 3, 0, 2, 4, &k);
            om.bx(7, 1, 3, 7, 2, 4, &k);
            om.bx(3, 1, 0, 4, 2, 0, &k);
            om.bx(3, 1, 7, 4, 2, 7, &k);
            if def.has_opening[SOUTH] {
                om.water(3, 1, 0, 4, 2, 0);
            }
            if def.has_opening[NORTH] {
                om.water(3, 1, 7, 4, 2, 7);
            }
            if def.has_opening[WEST] {
                om.water(0, 1, 3, 0, 2, 4);
            }
            if def.has_opening[EAST] {
                om.water(7, 1, 3, 7, 2, 4);
            }
        }
        _ => {}
    }
    if pillar {
        om.bx(3, 1, 3, 4, 1, 4, &l);
        om.bx(3, 2, 3, 4, 2, 4, &g);
        om.bx(3, 3, 3, 4, 3, 4, &l);
    }
}

fn wing_room<W: StructureWorld + ?Sized>(om: &mut Om<'_, '_, W>, main_design: i32) {
    let (l, k, lamp) = (om.p.light.clone(), om.p.black.clone(), om.p.lamp.clone());
    if main_design == 0 {
        for i in 0..4 {
            om.bx(10 - i, 3 - i, 20 - i, 12 + i, 3 - i, 20, &l);
        }
        om.bx(7, 0, 6, 15, 0, 16, &l);
        om.bx(6, 0, 6, 6, 3, 20, &l);
        om.bx(16, 0, 6, 16, 3, 20, &l);
        om.bx(7, 1, 7, 7, 1, 20, &l);
        om.bx(15, 1, 7, 15, 1, 20, &l);
        om.bx(7, 1, 6, 9, 3, 6, &l);
        om.bx(13, 1, 6, 15, 3, 6, &l);
        om.bx(8, 1, 7, 9, 1, 7, &l);
        om.bx(13, 1, 7, 14, 1, 7, &l);
        om.bx(9, 0, 5, 13, 0, 5, &l);
        om.bx(10, 0, 7, 12, 0, 7, &k);
        om.bx(8, 0, 10, 8, 0, 12, &k);
        om.bx(14, 0, 10, 14, 0, 12, &k);
        let mut z = 18;
        while z >= 7 {
            om.put(&lamp, 6, 3, z);
            om.put(&lamp, 16, 3, z);
            z -= 3;
        }
        om.put(&lamp, 10, 0, 10);
        om.put(&lamp, 12, 0, 10);
        om.put(&lamp, 10, 0, 12);
        om.put(&lamp, 12, 0, 12);
        om.put(&lamp, 8, 3, 6);
        om.put(&lamp, 14, 3, 6);
        om.put(&l, 4, 2, 4);
        om.put(&lamp, 4, 1, 4);
        om.put(&l, 4, 0, 4);
        om.put(&l, 18, 2, 4);
        om.put(&lamp, 18, 1, 4);
        om.put(&l, 18, 0, 4);
        om.put(&l, 4, 2, 18);
        om.put(&lamp, 4, 1, 18);
        om.put(&l, 4, 0, 18);
        om.put(&l, 18, 2, 18);
        om.put(&lamp, 18, 1, 18);
        om.put(&l, 18, 0, 18);
        om.put(&l, 9, 7, 20);
        om.put(&l, 13, 7, 20);
        om.bx(6, 0, 21, 7, 4, 21, &l);
        om.bx(15, 0, 21, 16, 4, 21, &l);
    } else if main_design == 1 {
        om.bx(9, 3, 18, 13, 3, 20, &l);
        om.bx(9, 0, 18, 9, 2, 18, &l);
        om.bx(13, 0, 18, 13, 2, 18, &l);
        for x in [9, 13] {
            om.put(&l, x, 6, 20);
            om.put(&lamp, x, 5, 20);
            om.put(&l, x, 4, 20);
        }
        om.bx(7, 3, 7, 15, 3, 14, &l);
        for x in [10, 12] {
            om.bx(x, 0, 10, x, 6, 10, &l);
            om.bx(x, 0, 12, x, 6, 12, &l);
            om.put(&lamp, x, 0, 10);
            om.put(&lamp, x, 0, 12);
            om.put(&lamp, x, 4, 10);
            om.put(&lamp, x, 4, 12);
        }
        for x in [8, 14] {
            om.bx(x, 0, 7, x, 2, 7, &l);
            om.bx(x, 0, 14, x, 2, 14, &l);
        }
        om.bx(8, 3, 8, 8, 3, 13, &k);
        om.bx(14, 3, 8, 14, 3, 13, &k);
    }
}

fn penthouse<W: StructureWorld + ?Sized>(om: &mut Om<'_, '_, W>) {
    let (g, l, k, lamp) = (
        om.p.gray.clone(),
        om.p.light.clone(),
        om.p.black.clone(),
        om.p.lamp.clone(),
    );
    om.bx(2, -1, 2, 11, -1, 11, &l);
    om.bx(0, -1, 0, 1, -1, 11, &g);
    om.bx(12, -1, 0, 13, -1, 11, &g);
    om.bx(2, -1, 0, 11, -1, 1, &g);
    om.bx(2, -1, 12, 11, -1, 13, &g);
    om.bx(0, 0, 0, 0, 0, 13, &l);
    om.bx(13, 0, 0, 13, 0, 13, &l);
    om.bx(1, 0, 0, 12, 0, 0, &l);
    om.bx(1, 0, 13, 12, 0, 13, &l);
    let mut i = 2;
    while i <= 11 {
        om.put(&lamp, 0, 0, i);
        om.put(&lamp, 13, 0, i);
        om.put(&lamp, i, 0, 0);
        i += 3;
    }
    om.bx(2, 0, 3, 4, 0, 9, &l);
    om.bx(9, 0, 3, 11, 0, 9, &l);
    om.bx(4, 0, 9, 9, 0, 11, &l);
    om.put(&l, 5, 0, 8);
    om.put(&l, 8, 0, 8);
    om.put(&l, 10, 0, 10);
    om.put(&l, 3, 0, 10);
    om.bx(3, 0, 3, 3, 0, 7, &k);
    om.bx(10, 0, 3, 10, 0, 7, &k);
    om.bx(6, 0, 10, 7, 0, 10, &k);
    for x in [3, 10] {
        let mut z = 2;
        while z <= 8 {
            om.bx(x, 0, z, x, 2, z, &l);
            z += 3;
        }
    }
    om.bx(5, 0, 10, 5, 2, 10, &l);
    om.bx(8, 0, 10, 8, 2, 10, &l);
    om.bx(6, -1, 7, 7, -1, 8, &k);
    om.water(6, -1, 3, 7, -1, 4);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::legacy::LegacyRandomSource;
    use crate::rng::worldgen_random::WorldgenRandom;

    fn start(seed: i64, cx: i32, cz: i32) -> Monument {
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(0));
        random.set_large_feature_seed(seed, cx, cz);
        Monument::new((cx, cz), &mut random)
    }

    /// The one piece vanilla saves, read off four clamped harvests
    /// (`region-differ starts <ref> --match monument`): the facing is the
    /// start random's first draw and the box is a pure function of it, so a
    /// wrong first draw or a wrong `makeBoundingBox` axis swap shows here.
    #[test]
    fn the_saved_building_is_vanillas() {
        for (seed, cx, cz, o, bb) in [
            (42, 49, -11, 3, [755, 39, -205, 812, 61, -148]),
            (8, 37, 102, 0, [563, 39, 1603, 620, 61, 1660]),
            (20_260_722, 14, -17, 1, [195, 39, -301, 252, 61, -244]),
            (30, 15, -80, 2, [211, 39, -1309, 268, 61, -1252]),
        ] {
            let m = start(seed, cx, cz);
            let b = m.building.bounding_box;
            assert_eq!(
                m.building.facing.map(Facing::saved_value),
                Some(o),
                "seed {seed} ({cx},{cz})"
            );
            assert_eq!(
                [b.min_x, b.min_y, b.min_z, b.max_x, b.max_y, b.max_z],
                bb,
                "seed {seed} ({cx},{cz})"
            );
        }
    }

    /// The graph's invariants, which hold for every start: the entry and the
    /// core come first and the two wings and the penthouse last, the wings'
    /// designs are opposite (`i++` twice on one draw), and every grid room is
    /// claimed by exactly one piece or by the core's eight.
    #[test]
    fn every_grid_cell_is_claimed_once() {
        for seed in 0..40 {
            let m = start(seed, 3, 7);
            assert_eq!(m.rooms[0].kind, RoomKind::Entry);
            assert_eq!(m.rooms[1].kind, RoomKind::Core);
            let n = m.rooms.len();
            let (RoomKind::Wing { main_design: a }, RoomKind::Wing { main_design: b }) =
                (m.rooms[n - 3].kind, m.rooms[n - 2].kind)
            else {
                panic!("seed {seed}: the wings are not third and second from last");
            };
            assert_ne!(a, b, "seed {seed}: the wing designs must be opposite");
            assert_eq!(m.rooms[n - 1].kind, RoomKind::Penthouse);
            assert!(
                m.defs.iter().all(|d| d.claimed),
                "seed {seed}: an unclaimed cell"
            );
        }
    }
}
