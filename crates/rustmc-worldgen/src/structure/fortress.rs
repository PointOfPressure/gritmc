//! Nether fortresses: the piece walk (`NetherFortressPieces` and
//! `NetherFortressStructure.generatePieces`), from the 26.2 bytecode.
//!
//! # The walk
//!
//! ```text
//!   random = WorldgenRandom(LegacyRandomSource(0)); setLargeFeatureSeed(seed, cx, cz)
//!   start  = BridgeCrossing at (cx*16 + 2, 64, cz*16 + 2), facing getRandomHorizontalDirection
//!   start.addChildren
//!   while pendingChildren not empty:
//!       pendingChildren.remove(random.nextInt(size)).addChildren
//!   moveInsideHeights(random, 48, 70)
//! ```
//!
//! Each child slot asks `generateAndAddPiece`, which does three things in an
//! order that is easy to get wrong:
//!
//! 1. **Too far from the start** (|x - start.minX| or |z - start.minZ| over
//!    112) makes a `BridgeEndFiller` and returns it **without adding it**.
//!    The filler's constructor still draws `nextInt()` for its seed when its
//!    box fits, so a dropped filler still moves the stream.
//! 2. Otherwise `generatePiece` rolls the bridge or castle weight table: up to
//!    five rolls, and within one roll a type whose `createPiece` refuses (no
//!    room) does **not** end the roll — the walk carries on down the table
//!    with the roll still negative. A type at its cap, or the previous type
//!    when it may not repeat, ends the roll. After five rolls, or when every
//!    capped type is exhausted, it falls back to a `BridgeEndFiller`.
//! 3. Whatever came back, filler included, is added and queued.
//!
//! The weight tables are **static** in vanilla and reset by `StartPiece`'s
//! constructor; [`Walk`] keeps them per start, which is the same thing for one
//! start at a time and the only sound thing for several.
//!
//! # What is saved, and so what is gated
//!
//! Each piece is saved with its id, `BB`, `O` and `GD`; a filler adds `Seed`
//! (its bare `nextInt()`), and the two corridor turns add `Chest`
//! (`nextInt(3) == 0`). The seeds pin the stream at every dead end of the
//! walk, so a layout that matches piece for piece *and* seed for seed has
//! consumed the stream exactly as vanilla did.

use super::pieces::{BoundingBox, Facing, Piece};
use super::scattered::{create_chest, fill_column_down, make_bounding_box};
use super::stronghold::orient_box;
use super::writer::{StructureWorld, Writer};
use crate::rng::legacy::LegacyRandomSource;
use crate::rng::worldgen_random::WorldgenRandom;
use crate::rng::RandomSource;

/// `NetherFortressPieces$*` — one variant per piece class.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Start,
    BridgeStraight,
    BridgeEndFiller,
    BridgeCrossing,
    RoomCrossing,
    StairsRoom,
    MonsterThrone,
    CastleEntrance,
    CastleSmallCorridor,
    CastleSmallCorridorCrossing,
    CastleSmallCorridorRightTurn,
    CastleSmallCorridorLeftTurn,
    CastleCorridorStairs,
    CastleCorridorTBalcony,
    CastleStalkRoom,
}

impl Kind {
    /// The id `StructurePieceType` registers, as the NBT saves it.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            // `StartPiece(random, x, z)` calls `BridgeCrossing(int, int,
            // Direction)`, which passes `NETHER_FORTRESS_BRIDGE_CROSSING`;
            // `NETHER_FORTRESS_START` is only the type a start *loaded from
            // NBT* is given. So a freshly generated start saves as a
            // crossing, and vanilla's first child reads `nebcr`.
            Self::Start => "minecraft:nebcr",
            Self::BridgeStraight => "minecraft:nebs",
            Self::BridgeEndFiller => "minecraft:nebef",
            Self::BridgeCrossing => "minecraft:nebcr",
            Self::RoomCrossing => "minecraft:nerc",
            Self::StairsRoom => "minecraft:nesr",
            Self::MonsterThrone => "minecraft:nemt",
            Self::CastleEntrance => "minecraft:nece",
            Self::CastleSmallCorridor => "minecraft:nesc",
            Self::CastleSmallCorridorCrossing => "minecraft:nescsc",
            Self::CastleSmallCorridorRightTurn => "minecraft:nescrt",
            Self::CastleSmallCorridorLeftTurn => "minecraft:nesclt",
            Self::CastleCorridorStairs => "minecraft:neccs",
            Self::CastleCorridorTBalcony => "minecraft:nectb",
            Self::CastleStalkRoom => "minecraft:necsr",
        }
    }
}

/// One placed piece: the saved layout plus the two fields a fortress adds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FortressPiece {
    pub kind: Kind,
    pub piece: Piece,
    /// `BridgeEndFiller.selfSeed`; 0 on every other piece.
    pub seed: i32,
    /// `isNeedingChest` on the two corridor turns; false elsewhere. Cleared
    /// by `postProcess` once the chest is in.
    pub chest: bool,
    /// `MonsterThrone.hasPlacedSpawner`; set by `postProcess`.
    pub placed_spawner: bool,
}

/// `PieceWeight`: type, weight, cap (0 = none), and whether it may follow
/// itself. Table order is the arrays' order in `NetherFortressPieces.<clinit>`.
const BRIDGE_WEIGHTS: [(Kind, i32, i32, bool); 6] = [
    (Kind::BridgeStraight, 30, 0, true),
    (Kind::BridgeCrossing, 10, 4, false),
    (Kind::RoomCrossing, 10, 4, false),
    (Kind::StairsRoom, 10, 3, false),
    (Kind::MonsterThrone, 5, 2, false),
    (Kind::CastleEntrance, 5, 1, false),
];

const CASTLE_WEIGHTS: [(Kind, i32, i32, bool); 7] = [
    (Kind::CastleSmallCorridor, 25, 0, true),
    (Kind::CastleSmallCorridorCrossing, 15, 5, false),
    (Kind::CastleSmallCorridorRightTurn, 5, 10, false),
    (Kind::CastleSmallCorridorLeftTurn, 5, 10, false),
    (Kind::CastleCorridorStairs, 10, 3, true),
    (Kind::CastleCorridorTBalcony, 7, 2, false),
    (Kind::CastleStalkRoom, 5, 2, false),
];

#[derive(Clone, Copy, Debug)]
struct Weight {
    kind: Kind,
    weight: i32,
    place_count: i32,
    max_place_count: i32,
    allow_in_row: bool,
}

impl Weight {
    /// `doPlace` and `isValid` are the same test; the depth argument
    /// `doPlace` takes is not read.
    const fn can_place(&self) -> bool {
        self.max_place_count == 0 || self.place_count < self.max_place_count
    }
}

fn table(rows: &[(Kind, i32, i32, bool)]) -> Vec<Weight> {
    rows.iter()
        .map(|&(kind, weight, max_place_count, allow_in_row)| Weight {
            kind,
            weight,
            place_count: 0,
            max_place_count,
            allow_in_row,
        })
        .collect()
}

/// `StructurePiece.getRandomHorizontalDirection`: `[N, E, S, W][nextInt(4)]`.
fn random_horizontal<R: RandomSource + ?Sized>(random: &mut R) -> Facing {
    match random.next_i32_bound(4) {
        0 => Facing::North,
        1 => Facing::East,
        2 => Facing::South,
        _ => Facing::West,
    }
}

/// `NetherBridgePiece.isOkBox`.
const fn is_ok_box(bb: BoundingBox) -> bool {
    bb.min_y > 10
}

struct Walk {
    bridge: Vec<Weight>,
    castle: Vec<Weight>,
    previous: Option<Kind>,
    pieces: Vec<FortressPiece>,
    pending: Vec<usize>,
}

impl Walk {
    fn collides(&self, bb: BoundingBox) -> bool {
        self.pieces
            .iter()
            .any(|p| p.piece.bounding_box.intersects(bb))
    }

    /// Each class's `createPiece`: the box, the room test, then the
    /// constructor's own draws.
    #[expect(clippy::too_many_arguments, reason = "the reference's own signature")]
    fn create<R: RandomSource + ?Sized>(
        &self,
        kind: Kind,
        random: &mut R,
        x: i32,
        y: i32,
        z: i32,
        facing: Facing,
        depth: i32,
    ) -> Option<FortressPiece> {
        let (ox, oy, oz, w, h, d) = match kind {
            Kind::BridgeStraight => (-1, -3, 0, 5, 10, 19),
            Kind::BridgeEndFiller => (-1, -3, 0, 5, 10, 8),
            Kind::BridgeCrossing => (-8, -3, 0, 19, 10, 19),
            Kind::RoomCrossing => (-2, 0, 0, 7, 9, 7),
            Kind::StairsRoom => (-2, 0, 0, 7, 11, 7),
            Kind::MonsterThrone => (-2, 0, 0, 7, 8, 9),
            Kind::CastleEntrance | Kind::CastleStalkRoom => (-5, -3, 0, 13, 14, 13),
            Kind::CastleSmallCorridor
            | Kind::CastleSmallCorridorCrossing
            | Kind::CastleSmallCorridorRightTurn
            | Kind::CastleSmallCorridorLeftTurn => (-1, 0, 0, 5, 7, 5),
            Kind::CastleCorridorStairs => (-1, -7, 0, 5, 14, 10),
            Kind::CastleCorridorTBalcony => (-3, 0, 0, 9, 7, 9),
            Kind::Start => unreachable!("the start is not made by a factory"),
        };
        let bb = orient_box(x, y, z, ox, oy, oz, w, h, d, facing);
        if !is_ok_box(bb) || self.collides(bb) {
            return None;
        }
        let mut seed = 0;
        let mut chest = false;
        match kind {
            Kind::BridgeEndFiller => seed = random.next_i32(),
            Kind::CastleSmallCorridorRightTurn | Kind::CastleSmallCorridorLeftTurn => {
                chest = random.next_i32_bound(3) == 0;
            }
            _ => {}
        }
        Some(FortressPiece {
            kind,
            piece: Piece {
                id: kind.id(),
                bounding_box: bb,
                facing: Some(facing),
                depth,
            },
            seed,
            chest,
            placed_spawner: false,
        })
    }

    /// `NetherBridgePiece.generatePiece`.
    #[expect(clippy::too_many_arguments, reason = "the reference's own signature")]
    fn generate_piece<R: RandomSource + ?Sized>(
        &mut self,
        castle: bool,
        random: &mut R,
        x: i32,
        y: i32,
        z: i32,
        facing: Facing,
        depth: i32,
    ) -> Option<FortressPiece> {
        // `updatePieceWeight`: the total of every weight, or -1 when no
        // capped type has room left.
        let (total, any) = {
            let list = if castle { &self.castle } else { &self.bridge };
            let any = list
                .iter()
                .any(|w| w.max_place_count > 0 && w.place_count < w.max_place_count);
            (list.iter().map(|w| w.weight).sum::<i32>(), any)
        };
        let weight = if any { total } else { -1 };
        let ok = weight > 0 && depth <= 30;
        let mut attempts = 0;
        while attempts < 5 && ok {
            attempts += 1;
            let mut roll = random.next_i32_bound(weight);
            let len = if castle {
                self.castle.len()
            } else {
                self.bridge.len()
            };
            for i in 0..len {
                let entry = if castle {
                    self.castle[i]
                } else {
                    self.bridge[i]
                };
                roll -= entry.weight;
                if roll >= 0 {
                    continue;
                }
                if !entry.can_place() {
                    break;
                }
                if self.previous == Some(entry.kind) && !entry.allow_in_row {
                    break;
                }
                let Some(piece) = self.create(entry.kind, random, x, y, z, facing, depth) else {
                    continue;
                };
                let list = if castle {
                    &mut self.castle
                } else {
                    &mut self.bridge
                };
                list[i].place_count += 1;
                self.previous = Some(entry.kind);
                if !list[i].can_place() {
                    list.remove(i);
                }
                return Some(piece);
            }
        }
        self.create(Kind::BridgeEndFiller, random, x, y, z, facing, depth)
    }

    /// `NetherBridgePiece.generateAndAddPiece`.
    #[expect(clippy::too_many_arguments, reason = "the reference's own signature")]
    fn generate_and_add<R: RandomSource + ?Sized>(
        &mut self,
        random: &mut R,
        x: i32,
        y: i32,
        z: i32,
        facing: Facing,
        depth: i32,
        castle: bool,
    ) {
        let start = self.pieces[0].piece.bounding_box;
        if (x - start.min_x).abs() > 112 || (z - start.min_z).abs() > 112 {
            // Made and dropped: never added, never queued — but its seed
            // was drawn if its box fit.
            let _ = self.create(Kind::BridgeEndFiller, random, x, y, z, facing, depth);
            return;
        }
        if let Some(piece) = self.generate_piece(castle, random, x, y, z, facing, depth + 1) {
            self.pieces.push(piece);
            self.pending.push(self.pieces.len() - 1);
        }
    }

    /// `generateChildForward(start, pieces, random, a, b, castle)`.
    fn forward<R: RandomSource + ?Sized>(
        &mut self,
        random: &mut R,
        at: usize,
        a: i32,
        b: i32,
        castle: bool,
    ) {
        let p = &self.pieces[at].piece;
        let (bb, depth) = (p.bounding_box, p.depth);
        let Some(facing) = p.facing else { return };
        let (x, y, z) = match facing {
            Facing::North => (bb.min_x + a, bb.min_y + b, bb.min_z - 1),
            Facing::South => (bb.min_x + a, bb.min_y + b, bb.max_z + 1),
            Facing::West => (bb.min_x - 1, bb.min_y + b, bb.min_z + a),
            Facing::East => (bb.max_x + 1, bb.min_y + b, bb.min_z + a),
        };
        self.generate_and_add(random, x, y, z, facing, depth, castle);
    }

    /// `generateChildLeft(start, pieces, random, a, b, castle)`: `a` is the
    /// height offset, `b` the horizontal one.
    fn left<R: RandomSource + ?Sized>(
        &mut self,
        random: &mut R,
        at: usize,
        a: i32,
        b: i32,
        castle: bool,
    ) {
        let p = &self.pieces[at].piece;
        let (bb, depth) = (p.bounding_box, p.depth);
        let Some(facing) = p.facing else { return };
        let (x, y, z, f) = match facing {
            Facing::North | Facing::South => {
                (bb.min_x - 1, bb.min_y + a, bb.min_z + b, Facing::West)
            }
            Facing::West | Facing::East => {
                (bb.min_x + b, bb.min_y + a, bb.min_z - 1, Facing::North)
            }
        };
        self.generate_and_add(random, x, y, z, f, depth, castle);
    }

    /// `generateChildRight(start, pieces, random, a, b, castle)`.
    fn right<R: RandomSource + ?Sized>(
        &mut self,
        random: &mut R,
        at: usize,
        a: i32,
        b: i32,
        castle: bool,
    ) {
        let p = &self.pieces[at].piece;
        let (bb, depth) = (p.bounding_box, p.depth);
        let Some(facing) = p.facing else { return };
        let (x, y, z, f) = match facing {
            Facing::North | Facing::South => {
                (bb.max_x + 1, bb.min_y + a, bb.min_z + b, Facing::East)
            }
            Facing::West | Facing::East => {
                (bb.min_x + b, bb.min_y + a, bb.max_z + 1, Facing::South)
            }
        };
        self.generate_and_add(random, x, y, z, f, depth, castle);
    }

    /// Each class's `addChildren`.
    fn add_children<R: RandomSource + ?Sized>(&mut self, random: &mut R, at: usize) {
        match self.pieces[at].kind {
            Kind::Start | Kind::BridgeCrossing => {
                self.forward(random, at, 8, 3, false);
                self.left(random, at, 3, 8, false);
                self.right(random, at, 3, 8, false);
            }
            Kind::BridgeStraight => self.forward(random, at, 1, 3, false),
            Kind::RoomCrossing => {
                self.forward(random, at, 2, 0, false);
                self.left(random, at, 0, 2, false);
                self.right(random, at, 0, 2, false);
            }
            Kind::StairsRoom => self.right(random, at, 6, 2, false),
            Kind::CastleEntrance => self.forward(random, at, 5, 3, true),
            Kind::CastleSmallCorridor | Kind::CastleCorridorStairs => {
                self.forward(random, at, 1, 0, true);
            }
            Kind::CastleSmallCorridorCrossing => {
                self.forward(random, at, 1, 0, true);
                self.left(random, at, 0, 1, true);
                self.right(random, at, 0, 1, true);
            }
            Kind::CastleSmallCorridorRightTurn => self.right(random, at, 0, 1, true),
            Kind::CastleSmallCorridorLeftTurn => self.left(random, at, 0, 1, true),
            Kind::CastleCorridorTBalcony => {
                let offset = match self.pieces[at].piece.facing {
                    Some(Facing::West | Facing::North) => 5,
                    _ => 1,
                };
                // The `castle` argument is `nextInt(8) > 0`, evaluated as
                // an argument — before any draw the child makes.
                let castle = random.next_i32_bound(8) > 0;
                self.left(random, at, 0, offset, castle);
                let castle = random.next_i32_bound(8) > 0;
                self.right(random, at, 0, offset, castle);
            }
            Kind::CastleStalkRoom => {
                self.forward(random, at, 5, 3, true);
                self.forward(random, at, 5, 11, true);
            }
            Kind::MonsterThrone | Kind::BridgeEndFiller => {}
        }
    }
}

/// `NetherFortressStructure.generatePieces` for the start at `chunk`.
#[must_use]
pub fn generate(seed: i64, chunk: (i32, i32)) -> Vec<FortressPiece> {
    let mut random = WorldgenRandom::new(LegacyRandomSource::new(0));
    random.set_large_feature_seed(seed, chunk.0, chunk.1);

    let facing = random_horizontal(&mut random);
    let start = FortressPiece {
        kind: Kind::Start,
        piece: Piece {
            id: Kind::Start.id(),
            bounding_box: make_bounding_box(
                chunk.0 * 16 + 2,
                64,
                chunk.1 * 16 + 2,
                facing,
                19,
                10,
                19,
            ),
            facing: Some(facing),
            depth: 0,
        },
        seed: 0,
        chest: false,
        placed_spawner: false,
    };
    let mut walk = Walk {
        bridge: table(&BRIDGE_WEIGHTS),
        castle: table(&CASTLE_WEIGHTS),
        previous: None,
        pieces: vec![start],
        pending: Vec::new(),
    };
    walk.add_children(&mut random, 0);
    while !walk.pending.is_empty() {
        let i = random.next_i32_bound(walk.pending.len() as i32) as usize;
        let at = walk.pending.remove(i);
        walk.add_children(&mut random, at);
    }

    // `StructurePiecesBuilder.moveInsideHeights(random, 48, 70)`.
    let mut pieces = walk.pieces;
    let boxes: Vec<BoundingBox> = pieces.iter().map(|p| p.piece.bounding_box).collect();
    if let Some(all) = BoundingBox::encapsulating(&boxes) {
        let span = 70 - 48 + 1 - (all.max_y - all.min_y + 1);
        let y = if span > 1 {
            48 + random.next_i32_bound(span)
        } else {
            48
        };
        let dy = y - all.min_y;
        for p in &mut pieces {
            p.piece.bounding_box.shift(0, dy, 0);
        }
    }
    pieces
}

// ---------------------------------------------------------------------------
// postProcess: the blocks
//
// Every body below is the piece class's `postProcess`, call for call, from
// the same listing: `generateBox`, `placeBlock` and `fillColumnDown` with
// their local coordinates, and each block state as the bytecode builds it
// (`defaultBlockState()` then `setValue` per fence side) in its canonical
// spelling. `placeBlock` mirrors and rotates the state into the piece's frame;
// `fillColumnDown` and the spawner are raw `setBlock`s and do not.

const BRICKS: &str = "minecraft:nether_bricks";
const AIR: &str = "minecraft:air";
const CHEST: &str = "minecraft:chest[facing=north,type=single,waterlogged=false]";

/// A spawner's block entity, as `setEntityId` leaves it: `SpawnData.entity.id`.
fn spawner_of(mob: &str) -> crate::block_entity::Record {
    use rustmc_world::nbt::Nbt;
    use std::collections::BTreeMap;
    crate::block_entity::Record {
        nbt: BTreeMap::from([(
            "SpawnData".to_string(),
            Nbt::Compound(BTreeMap::from([(
                "entity".to_string(),
                Nbt::Compound(BTreeMap::from([(
                    "id".to_string(),
                    Nbt::String(mob.to_string()),
                )])),
            )])),
        )]),
        loot_seed: None,
    }
}

/// `StructureStart.placeInChunk` for one piece: its `postProcess` against the
/// chunk's writable box.
///
/// `random` is the structure's decoration stream; only the two corridor
/// turns draw from it, one `nextLong` per chest for the loot seed.
pub fn post_process<W: StructureWorld + ?Sized, R: RandomSource + ?Sized>(
    world: &mut W,
    p: &mut FortressPiece,
    clip: BoundingBox,
    random: &mut R,
) {
    let piece = p.piece.clone();
    let mut w = Writer {
        world,
        piece: &piece,
        clip,
        protected: &[],
    };
    let w = &mut w;
    match p.kind {
        Kind::Start | Kind::BridgeCrossing => bridge_crossing(w),
        Kind::BridgeStraight => bridge_straight(w),
        Kind::BridgeEndFiller => bridge_end_filler(w, p.seed),
        Kind::RoomCrossing => room_crossing(w),
        Kind::StairsRoom => stairs_room(w),
        Kind::MonsterThrone => monster_throne(w, &mut p.placed_spawner),
        Kind::CastleEntrance => castle_entrance(w),
        Kind::CastleSmallCorridor => castle_small_corridor(w),
        Kind::CastleSmallCorridorCrossing => castle_small_corridor_crossing(w),
        Kind::CastleSmallCorridorRightTurn => {
            castle_small_corridor_right_turn(w, random, &mut p.chest);
        }
        Kind::CastleSmallCorridorLeftTurn => {
            castle_small_corridor_left_turn(w, random, &mut p.chest);
        }
        Kind::CastleCorridorStairs => castle_corridor_stairs(w),
        Kind::CastleCorridorTBalcony => castle_corridor_t_balcony(w),
        Kind::CastleStalkRoom => castle_stalk_room(w),
    }
}

/// `NetherFortressPieces$BridgeStraight.postProcess`.
fn bridge_straight<W: StructureWorld + ?Sized>(w: &mut Writer<'_, W>) {
    w.generate_box(0, 3, 0, 4, 4, 18, BRICKS, BRICKS, false);
    w.generate_box(1, 5, 0, 3, 7, 18, AIR, AIR, false);
    w.generate_box(0, 5, 0, 0, 5, 18, BRICKS, BRICKS, false);
    w.generate_box(4, 5, 0, 4, 5, 18, BRICKS, BRICKS, false);
    w.generate_box(0, 2, 0, 4, 2, 5, BRICKS, BRICKS, false);
    w.generate_box(0, 2, 13, 4, 2, 18, BRICKS, BRICKS, false);
    w.generate_box(0, 0, 0, 4, 1, 3, BRICKS, BRICKS, false);
    w.generate_box(0, 0, 15, 4, 1, 18, BRICKS, BRICKS, false);
    for i8 in 0..=4 {
        for i9 in 0..=2 {
            fill_column_down(w, BRICKS, i8, -1, i9);
            fill_column_down(w, BRICKS, i8, -1, 18 - i9);
        }
    }
    w.generate_box(0, 1, 1, 0, 4, 1, "minecraft:nether_brick_fence[east=true,north=true,south=true,waterlogged=false,west=false]", "minecraft:nether_brick_fence[east=true,north=true,south=true,waterlogged=false,west=false]", false);
    w.generate_box(0, 3, 4, 0, 4, 4, "minecraft:nether_brick_fence[east=true,north=true,south=true,waterlogged=false,west=false]", "minecraft:nether_brick_fence[east=true,north=true,south=true,waterlogged=false,west=false]", false);
    w.generate_box(0, 3, 14, 0, 4, 14, "minecraft:nether_brick_fence[east=true,north=true,south=true,waterlogged=false,west=false]", "minecraft:nether_brick_fence[east=true,north=true,south=true,waterlogged=false,west=false]", false);
    w.generate_box(0, 1, 17, 0, 4, 17, "minecraft:nether_brick_fence[east=true,north=true,south=true,waterlogged=false,west=false]", "minecraft:nether_brick_fence[east=true,north=true,south=true,waterlogged=false,west=false]", false);
    w.generate_box(4, 1, 1, 4, 4, 1, "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=true]", "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=true]", false);
    w.generate_box(4, 3, 4, 4, 4, 4, "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=true]", "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=true]", false);
    w.generate_box(4, 3, 14, 4, 4, 14, "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=true]", "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=true]", false);
    w.generate_box(4, 1, 17, 4, 4, 17, "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=true]", "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=true]", false);
}

/// `NetherFortressPieces$BridgeEndFiller.postProcess`.
fn bridge_end_filler<W: StructureWorld + ?Sized>(w: &mut Writer<'_, W>, seed: i32) {
    // `RandomSource.createThreadLocalInstance(selfSeed)`: a fresh
    // `SingleThreadedRandomSource`, which is the legacy LCG. Its draws are the
    // filler's own and touch no other stream.
    let mut random = LegacyRandomSource::new(i64::from(seed));
    for i9 in 0..=4 {
        for i10 in 3..=4 {
            let i11 = random.next_i32_bound(8);
            w.generate_box(i9, i10, 0, i9, i10, i11, BRICKS, BRICKS, false);
        }
    }
    let i9 = random.next_i32_bound(8);
    w.generate_box(0, 5, 0, 0, 5, i9, BRICKS, BRICKS, false);
    let i9 = random.next_i32_bound(8);
    w.generate_box(4, 5, 0, 4, 5, i9, BRICKS, BRICKS, false);
    for i9 in 0..=4 {
        let i10 = random.next_i32_bound(5);
        w.generate_box(i9, 2, 0, i9, 2, i10, BRICKS, BRICKS, false);
    }
    for i9 in 0..=4 {
        for i10 in 0..=1 {
            let i11 = random.next_i32_bound(3);
            w.generate_box(i9, i10, 0, i9, i10, i11, BRICKS, BRICKS, false);
        }
    }
}

/// `NetherFortressPieces$BridgeCrossing.postProcess`.
fn bridge_crossing<W: StructureWorld + ?Sized>(w: &mut Writer<'_, W>) {
    w.generate_box(7, 3, 0, 11, 4, 18, BRICKS, BRICKS, false);
    w.generate_box(0, 3, 7, 18, 4, 11, BRICKS, BRICKS, false);
    w.generate_box(8, 5, 0, 10, 7, 18, AIR, AIR, false);
    w.generate_box(0, 5, 8, 18, 7, 10, AIR, AIR, false);
    w.generate_box(7, 5, 0, 7, 5, 7, BRICKS, BRICKS, false);
    w.generate_box(7, 5, 11, 7, 5, 18, BRICKS, BRICKS, false);
    w.generate_box(11, 5, 0, 11, 5, 7, BRICKS, BRICKS, false);
    w.generate_box(11, 5, 11, 11, 5, 18, BRICKS, BRICKS, false);
    w.generate_box(0, 5, 7, 7, 5, 7, BRICKS, BRICKS, false);
    w.generate_box(11, 5, 7, 18, 5, 7, BRICKS, BRICKS, false);
    w.generate_box(0, 5, 11, 7, 5, 11, BRICKS, BRICKS, false);
    w.generate_box(11, 5, 11, 18, 5, 11, BRICKS, BRICKS, false);
    w.generate_box(7, 2, 0, 11, 2, 5, BRICKS, BRICKS, false);
    w.generate_box(7, 2, 13, 11, 2, 18, BRICKS, BRICKS, false);
    w.generate_box(7, 0, 0, 11, 1, 3, BRICKS, BRICKS, false);
    w.generate_box(7, 0, 15, 11, 1, 18, BRICKS, BRICKS, false);
    for i8 in 7..=11 {
        for i9 in 0..=2 {
            fill_column_down(w, BRICKS, i8, -1, i9);
            fill_column_down(w, BRICKS, i8, -1, 18 - i9);
        }
    }
    w.generate_box(0, 2, 7, 5, 2, 11, BRICKS, BRICKS, false);
    w.generate_box(13, 2, 7, 18, 2, 11, BRICKS, BRICKS, false);
    w.generate_box(0, 0, 7, 3, 1, 11, BRICKS, BRICKS, false);
    w.generate_box(15, 0, 7, 18, 1, 11, BRICKS, BRICKS, false);
    for i8 in 0..=2 {
        for i9 in 7..=11 {
            fill_column_down(w, BRICKS, i8, -1, i9);
            fill_column_down(w, BRICKS, 18 - i8, -1, i9);
        }
    }
}

/// `NetherFortressPieces$RoomCrossing.postProcess`.
fn room_crossing<W: StructureWorld + ?Sized>(w: &mut Writer<'_, W>) {
    w.generate_box(0, 0, 0, 6, 1, 6, BRICKS, BRICKS, false);
    w.generate_box(0, 2, 0, 6, 7, 6, AIR, AIR, false);
    w.generate_box(0, 2, 0, 1, 6, 0, BRICKS, BRICKS, false);
    w.generate_box(0, 2, 6, 1, 6, 6, BRICKS, BRICKS, false);
    w.generate_box(5, 2, 0, 6, 6, 0, BRICKS, BRICKS, false);
    w.generate_box(5, 2, 6, 6, 6, 6, BRICKS, BRICKS, false);
    w.generate_box(0, 2, 0, 0, 6, 1, BRICKS, BRICKS, false);
    w.generate_box(0, 2, 5, 0, 6, 6, BRICKS, BRICKS, false);
    w.generate_box(6, 2, 0, 6, 6, 1, BRICKS, BRICKS, false);
    w.generate_box(6, 2, 5, 6, 6, 6, BRICKS, BRICKS, false);
    w.generate_box(2, 6, 0, 4, 6, 0, BRICKS, BRICKS, false);
    w.generate_box(2, 5, 0, 4, 5, 0, "minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", "minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", false);
    w.generate_box(2, 6, 6, 4, 6, 6, BRICKS, BRICKS, false);
    w.generate_box(2, 5, 6, 4, 5, 6, "minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", "minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", false);
    w.generate_box(0, 6, 2, 0, 6, 4, BRICKS, BRICKS, false);
    w.generate_box(0, 5, 2, 0, 5, 4, "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", false);
    w.generate_box(6, 6, 2, 6, 6, 4, BRICKS, BRICKS, false);
    w.generate_box(6, 5, 2, 6, 5, 4, "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", false);
    for i10 in 0..=6 {
        for i11 in 0..=6 {
            fill_column_down(w, BRICKS, i10, -1, i11);
        }
    }
}

/// `NetherFortressPieces$StairsRoom.postProcess`.
fn stairs_room<W: StructureWorld + ?Sized>(w: &mut Writer<'_, W>) {
    w.generate_box(0, 0, 0, 6, 1, 6, BRICKS, BRICKS, false);
    w.generate_box(0, 2, 0, 6, 10, 6, AIR, AIR, false);
    w.generate_box(0, 2, 0, 1, 8, 0, BRICKS, BRICKS, false);
    w.generate_box(5, 2, 0, 6, 8, 0, BRICKS, BRICKS, false);
    w.generate_box(0, 2, 1, 0, 8, 6, BRICKS, BRICKS, false);
    w.generate_box(6, 2, 1, 6, 8, 6, BRICKS, BRICKS, false);
    w.generate_box(1, 2, 6, 5, 8, 6, BRICKS, BRICKS, false);
    w.generate_box(0, 3, 2, 0, 5, 4, "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", false);
    w.generate_box(6, 3, 2, 6, 5, 2, "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", false);
    w.generate_box(6, 3, 4, 6, 5, 4, "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", false);
    w.place_block(BRICKS, 5, 2, 5);
    w.generate_box(4, 2, 5, 4, 3, 5, BRICKS, BRICKS, false);
    w.generate_box(3, 2, 5, 3, 4, 5, BRICKS, BRICKS, false);
    w.generate_box(2, 2, 5, 2, 5, 5, BRICKS, BRICKS, false);
    w.generate_box(1, 2, 5, 1, 6, 5, BRICKS, BRICKS, false);
    w.generate_box(1, 7, 1, 5, 7, 4, BRICKS, BRICKS, false);
    w.generate_box(6, 8, 2, 6, 8, 4, AIR, AIR, false);
    w.generate_box(2, 6, 0, 4, 8, 0, BRICKS, BRICKS, false);
    w.generate_box(2, 5, 0, 4, 5, 0, "minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", "minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", false);
    for i10 in 0..=6 {
        for i11 in 0..=6 {
            fill_column_down(w, BRICKS, i10, -1, i11);
        }
    }
}

/// `NetherFortressPieces$MonsterThrone.postProcess`.
fn monster_throne<W: StructureWorld + ?Sized>(w: &mut Writer<'_, W>, placed_spawner: &mut bool) {
    w.generate_box(0, 2, 0, 6, 7, 7, AIR, AIR, false);
    w.generate_box(1, 0, 0, 5, 1, 7, BRICKS, BRICKS, false);
    w.generate_box(1, 2, 1, 5, 2, 7, BRICKS, BRICKS, false);
    w.generate_box(1, 3, 2, 5, 3, 7, BRICKS, BRICKS, false);
    w.generate_box(1, 4, 3, 5, 4, 7, BRICKS, BRICKS, false);
    w.generate_box(1, 2, 0, 1, 4, 2, BRICKS, BRICKS, false);
    w.generate_box(5, 2, 0, 5, 4, 2, BRICKS, BRICKS, false);
    w.generate_box(1, 5, 2, 1, 5, 3, BRICKS, BRICKS, false);
    w.generate_box(5, 5, 2, 5, 5, 3, BRICKS, BRICKS, false);
    w.generate_box(0, 5, 3, 0, 5, 8, BRICKS, BRICKS, false);
    w.generate_box(6, 5, 3, 6, 5, 8, BRICKS, BRICKS, false);
    w.generate_box(1, 5, 8, 5, 5, 8, BRICKS, BRICKS, false);
    w.place_block("minecraft:nether_brick_fence[east=false,north=false,south=false,waterlogged=false,west=true]", 1, 6, 3);
    w.place_block("minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=false]", 5, 6, 3);
    w.place_block("minecraft:nether_brick_fence[east=true,north=true,south=false,waterlogged=false,west=false]", 0, 6, 3);
    w.place_block("minecraft:nether_brick_fence[east=false,north=true,south=false,waterlogged=false,west=true]", 6, 6, 3);
    w.generate_box(0, 6, 4, 0, 6, 7, "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", false);
    w.generate_box(6, 6, 4, 6, 6, 7, "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", false);
    w.place_block("minecraft:nether_brick_fence[east=true,north=false,south=true,waterlogged=false,west=false]", 0, 6, 8);
    w.place_block("minecraft:nether_brick_fence[east=false,north=false,south=true,waterlogged=false,west=true]", 6, 6, 8);
    w.generate_box(1, 6, 8, 5, 6, 8, "minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", "minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", false);
    w.place_block("minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=false]", 1, 7, 8);
    w.generate_box(2, 7, 8, 4, 7, 8, "minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", "minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", false);
    w.place_block("minecraft:nether_brick_fence[east=false,north=false,south=false,waterlogged=false,west=true]", 5, 7, 8);
    w.place_block("minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=false]", 2, 8, 8);
    w.place_block("minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", 3, 8, 8);
    w.place_block("minecraft:nether_brick_fence[east=false,north=false,south=false,waterlogged=false,west=true]", 4, 8, 8);
    // `hasPlacedSpawner`, then the box test on the spawner's own position,
    // then a raw `setBlock` (no orientation, no replaceability test).
    if !*placed_spawner && w.inside_box(3, 5, 5) {
        *placed_spawner = true;
        let (x, y, z) = w.piece.world_pos(3, 5, 5);
        w.world.set_block(x, y, z, "minecraft:spawner");
        // `SpawnerBlockEntity.setEntityId(EntityTypes.BLAZE, random)`: a
        // constant, and no draw — the spawn-potentials list of a spawner
        // placed a moment ago is unbuilt, so `getRandom` returns empty
        // without drawing.
        w.record_block_entity(3, 5, 5, spawner_of("minecraft:blaze"));
    }
    for i10 in 0..=6 {
        for i11 in 0..=6 {
            fill_column_down(w, BRICKS, i10, -1, i11);
        }
    }
}

/// `NetherFortressPieces$CastleEntrance.postProcess`.
fn castle_entrance<W: StructureWorld + ?Sized>(w: &mut Writer<'_, W>) {
    w.generate_box(0, 3, 0, 12, 4, 12, BRICKS, BRICKS, false);
    w.generate_box(0, 5, 0, 12, 13, 12, AIR, AIR, false);
    w.generate_box(0, 5, 0, 1, 12, 12, BRICKS, BRICKS, false);
    w.generate_box(11, 5, 0, 12, 12, 12, BRICKS, BRICKS, false);
    w.generate_box(2, 5, 11, 4, 12, 12, BRICKS, BRICKS, false);
    w.generate_box(8, 5, 11, 10, 12, 12, BRICKS, BRICKS, false);
    w.generate_box(5, 9, 11, 7, 12, 12, BRICKS, BRICKS, false);
    w.generate_box(2, 5, 0, 4, 12, 1, BRICKS, BRICKS, false);
    w.generate_box(8, 5, 0, 10, 12, 1, BRICKS, BRICKS, false);
    w.generate_box(5, 9, 0, 7, 12, 1, BRICKS, BRICKS, false);
    w.generate_box(2, 11, 2, 10, 12, 10, BRICKS, BRICKS, false);
    w.generate_box(5, 8, 0, 7, 8, 0, "minecraft:nether_brick_fence[east=false,north=false,south=false,waterlogged=false,west=false]", "minecraft:nether_brick_fence[east=false,north=false,south=false,waterlogged=false,west=false]", false);
    for i10 in (1..=11).step_by(2) {
        w.generate_box(i10, 10, 0, i10, 11, 0, "minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", "minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", false);
        w.generate_box(i10, 10, 12, i10, 11, 12, "minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", "minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", false);
        w.generate_box(0, 10, i10, 0, 11, i10, "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", false);
        w.generate_box(12, 10, i10, 12, 11, i10, "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", false);
        w.place_block(BRICKS, i10, 13, 0);
        w.place_block(BRICKS, i10, 13, 12);
        w.place_block(BRICKS, 0, 13, i10);
        w.place_block(BRICKS, 12, 13, i10);
        if i10 != 11 {
            w.place_block("minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", i10 + 1, 13, 0);
            w.place_block("minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", i10 + 1, 13, 12);
            w.place_block("minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", 0, 13, i10 + 1);
            w.place_block("minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", 12, 13, i10 + 1);
        }
    }
    w.place_block("minecraft:nether_brick_fence[east=true,north=true,south=false,waterlogged=false,west=false]", 0, 13, 0);
    w.place_block("minecraft:nether_brick_fence[east=true,north=false,south=true,waterlogged=false,west=false]", 0, 13, 12);
    w.place_block("minecraft:nether_brick_fence[east=false,north=false,south=true,waterlogged=false,west=true]", 12, 13, 12);
    w.place_block("minecraft:nether_brick_fence[east=false,north=true,south=false,waterlogged=false,west=true]", 12, 13, 0);
    for i10 in (3..=9).step_by(2) {
        w.generate_box(1, 7, i10, 1, 8, i10, "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=true]", "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=true]", false);
        w.generate_box(11, 7, i10, 11, 8, i10, "minecraft:nether_brick_fence[east=true,north=true,south=true,waterlogged=false,west=false]", "minecraft:nether_brick_fence[east=true,north=true,south=true,waterlogged=false,west=false]", false);
    }
    w.generate_box(4, 2, 0, 8, 2, 12, BRICKS, BRICKS, false);
    w.generate_box(0, 2, 4, 12, 2, 8, BRICKS, BRICKS, false);
    w.generate_box(4, 0, 0, 8, 1, 3, BRICKS, BRICKS, false);
    w.generate_box(4, 0, 9, 8, 1, 12, BRICKS, BRICKS, false);
    w.generate_box(0, 0, 4, 3, 1, 8, BRICKS, BRICKS, false);
    w.generate_box(9, 0, 4, 12, 1, 8, BRICKS, BRICKS, false);
    for i10 in 4..=8 {
        for i11 in 0..=2 {
            fill_column_down(w, BRICKS, i10, -1, i11);
            fill_column_down(w, BRICKS, i10, -1, 12 - i11);
        }
    }
    for i10 in 0..=2 {
        for i11 in 4..=8 {
            fill_column_down(w, BRICKS, i10, -1, i11);
            fill_column_down(w, BRICKS, 12 - i10, -1, i11);
        }
    }
    w.generate_box(5, 5, 5, 7, 5, 7, BRICKS, BRICKS, false);
    w.generate_box(6, 1, 6, 6, 4, 6, AIR, AIR, false);
    w.place_block(BRICKS, 6, 0, 6);
    w.place_block("minecraft:lava[level=0]", 6, 5, 6);
    // `scheduleTick(pos, LAVA, 0)` when the lava is inside the box: a fluid
    // tick, which writes no block during generation.
}

/// `NetherFortressPieces$CastleSmallCorridorPiece.postProcess`.
fn castle_small_corridor<W: StructureWorld + ?Sized>(w: &mut Writer<'_, W>) {
    w.generate_box(0, 0, 0, 4, 1, 4, BRICKS, BRICKS, false);
    w.generate_box(0, 2, 0, 4, 5, 4, AIR, AIR, false);
    w.generate_box(0, 2, 0, 0, 5, 4, BRICKS, BRICKS, false);
    w.generate_box(4, 2, 0, 4, 5, 4, BRICKS, BRICKS, false);
    w.generate_box(0, 3, 1, 0, 4, 1, "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", false);
    w.generate_box(0, 3, 3, 0, 4, 3, "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", false);
    w.generate_box(4, 3, 1, 4, 4, 1, "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", false);
    w.generate_box(4, 3, 3, 4, 4, 3, "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", false);
    w.generate_box(0, 6, 0, 4, 6, 4, BRICKS, BRICKS, false);
    for i9 in 0..=4 {
        for i10 in 0..=4 {
            fill_column_down(w, BRICKS, i9, -1, i10);
        }
    }
}

/// `NetherFortressPieces$CastleSmallCorridorCrossingPiece.postProcess`.
fn castle_small_corridor_crossing<W: StructureWorld + ?Sized>(w: &mut Writer<'_, W>) {
    w.generate_box(0, 0, 0, 4, 1, 4, BRICKS, BRICKS, false);
    w.generate_box(0, 2, 0, 4, 5, 4, AIR, AIR, false);
    w.generate_box(0, 2, 0, 0, 5, 0, BRICKS, BRICKS, false);
    w.generate_box(4, 2, 0, 4, 5, 0, BRICKS, BRICKS, false);
    w.generate_box(0, 2, 4, 0, 5, 4, BRICKS, BRICKS, false);
    w.generate_box(4, 2, 4, 4, 5, 4, BRICKS, BRICKS, false);
    w.generate_box(0, 6, 0, 4, 6, 4, BRICKS, BRICKS, false);
    for i8 in 0..=4 {
        for i9 in 0..=4 {
            fill_column_down(w, BRICKS, i8, -1, i9);
        }
    }
}

/// `NetherFortressPieces$CastleSmallCorridorRightTurnPiece.postProcess`.
fn castle_small_corridor_right_turn<W: StructureWorld + ?Sized, R: RandomSource + ?Sized>(
    w: &mut Writer<'_, W>,
    random: &mut R,
    needs_chest: &mut bool,
) {
    w.generate_box(0, 0, 0, 4, 1, 4, BRICKS, BRICKS, false);
    w.generate_box(0, 2, 0, 4, 5, 4, AIR, AIR, false);
    w.generate_box(0, 2, 0, 0, 5, 4, BRICKS, BRICKS, false);
    w.generate_box(0, 3, 1, 0, 4, 1, "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", false);
    w.generate_box(0, 3, 3, 0, 4, 3, "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", false);
    w.generate_box(4, 2, 0, 4, 5, 0, BRICKS, BRICKS, false);
    w.generate_box(1, 2, 4, 4, 5, 4, BRICKS, BRICKS, false);
    w.generate_box(1, 3, 4, 1, 4, 4, "minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", "minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", false);
    w.generate_box(3, 3, 4, 3, 4, 4, "minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", "minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", false);
    if *needs_chest && w.inside_box(1, 2, 3) {
        *needs_chest = false;
        // `BuiltInLootTables.NETHER_BRIDGE`, `ldc "chests/nether_bridge"`.
        create_chest(w, random, CHEST, 1, 2, 3, "minecraft:chests/nether_bridge");
    }
    w.generate_box(0, 6, 0, 4, 6, 4, BRICKS, BRICKS, false);
    for i10 in 0..=4 {
        for i11 in 0..=4 {
            fill_column_down(w, BRICKS, i10, -1, i11);
        }
    }
}

/// `NetherFortressPieces$CastleSmallCorridorLeftTurnPiece.postProcess`.
fn castle_small_corridor_left_turn<W: StructureWorld + ?Sized, R: RandomSource + ?Sized>(
    w: &mut Writer<'_, W>,
    random: &mut R,
    needs_chest: &mut bool,
) {
    w.generate_box(0, 0, 0, 4, 1, 4, BRICKS, BRICKS, false);
    w.generate_box(0, 2, 0, 4, 5, 4, AIR, AIR, false);
    w.generate_box(4, 2, 0, 4, 5, 4, BRICKS, BRICKS, false);
    w.generate_box(4, 3, 1, 4, 4, 1, "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", false);
    w.generate_box(4, 3, 3, 4, 4, 3, "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", false);
    w.generate_box(0, 2, 0, 0, 5, 0, BRICKS, BRICKS, false);
    w.generate_box(0, 2, 4, 3, 5, 4, BRICKS, BRICKS, false);
    w.generate_box(1, 3, 4, 1, 4, 4, "minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", "minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", false);
    w.generate_box(3, 3, 4, 3, 4, 4, "minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", "minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", false);
    if *needs_chest && w.inside_box(3, 2, 3) {
        *needs_chest = false;
        // `BuiltInLootTables.NETHER_BRIDGE`, `ldc "chests/nether_bridge"`.
        create_chest(w, random, CHEST, 3, 2, 3, "minecraft:chests/nether_bridge");
    }
    w.generate_box(0, 6, 0, 4, 6, 4, BRICKS, BRICKS, false);
    for i10 in 0..=4 {
        for i11 in 0..=4 {
            fill_column_down(w, BRICKS, i10, -1, i11);
        }
    }
}

/// `NetherFortressPieces$CastleCorridorStairsPiece.postProcess`.
fn castle_corridor_stairs<W: StructureWorld + ?Sized>(w: &mut Writer<'_, W>) {
    for i10 in 0..=9 {
        let i11 = 1i32.max(7 - i10);
        let i12 = (i11 + 5).max(14 - i10).min(13);
        let i13 = i10;
        w.generate_box(0, 0, i13, 4, i11, i13, BRICKS, BRICKS, false);
        w.generate_box(1, i11 + 1, i13, 3, i12 - 1, i13, AIR, AIR, false);
        if i10 <= 6 {
            w.place_block("minecraft:nether_brick_stairs[facing=south,half=bottom,shape=straight,waterlogged=false]", 1, i11 + 1, i13);
            w.place_block("minecraft:nether_brick_stairs[facing=south,half=bottom,shape=straight,waterlogged=false]", 2, i11 + 1, i13);
            w.place_block("minecraft:nether_brick_stairs[facing=south,half=bottom,shape=straight,waterlogged=false]", 3, i11 + 1, i13);
        }
        w.generate_box(0, i12, i13, 4, i12, i13, BRICKS, BRICKS, false);
        w.generate_box(0, i11 + 1, i13, 0, i12 - 1, i13, BRICKS, BRICKS, false);
        w.generate_box(4, i11 + 1, i13, 4, i12 - 1, i13, BRICKS, BRICKS, false);
        if (i10 & 1) == 0 {
            w.generate_box(0, i11 + 2, i13, 0, i11 + 3, i13, "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", false);
            w.generate_box(4, i11 + 2, i13, 4, i11 + 3, i13, "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", false);
        }
        for i14 in 0..=4 {
            fill_column_down(w, BRICKS, i14, -1, i13);
        }
    }
}

/// `NetherFortressPieces$CastleCorridorTBalconyPiece.postProcess`.
fn castle_corridor_t_balcony<W: StructureWorld + ?Sized>(w: &mut Writer<'_, W>) {
    w.generate_box(0, 0, 0, 8, 1, 8, BRICKS, BRICKS, false);
    w.generate_box(0, 2, 0, 8, 5, 8, AIR, AIR, false);
    w.generate_box(0, 6, 0, 8, 6, 5, BRICKS, BRICKS, false);
    w.generate_box(0, 2, 0, 2, 5, 0, BRICKS, BRICKS, false);
    w.generate_box(6, 2, 0, 8, 5, 0, BRICKS, BRICKS, false);
    w.generate_box(1, 3, 0, 1, 4, 0, "minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", "minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", false);
    w.generate_box(7, 3, 0, 7, 4, 0, "minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", "minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", false);
    w.generate_box(0, 2, 4, 8, 2, 8, BRICKS, BRICKS, false);
    w.generate_box(1, 1, 4, 2, 2, 4, AIR, AIR, false);
    w.generate_box(6, 1, 4, 7, 2, 4, AIR, AIR, false);
    w.generate_box(1, 3, 8, 7, 3, 8, "minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", "minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", false);
    w.place_block("minecraft:nether_brick_fence[east=true,north=false,south=true,waterlogged=false,west=false]", 0, 3, 8);
    w.place_block("minecraft:nether_brick_fence[east=false,north=false,south=true,waterlogged=false,west=true]", 8, 3, 8);
    w.generate_box(0, 3, 6, 0, 3, 7, "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", false);
    w.generate_box(8, 3, 6, 8, 3, 7, "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", false);
    w.generate_box(0, 3, 4, 0, 5, 5, BRICKS, BRICKS, false);
    w.generate_box(8, 3, 4, 8, 5, 5, BRICKS, BRICKS, false);
    w.generate_box(1, 3, 5, 2, 5, 5, BRICKS, BRICKS, false);
    w.generate_box(6, 3, 5, 7, 5, 5, BRICKS, BRICKS, false);
    w.generate_box(1, 4, 5, 1, 5, 5, "minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", "minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", false);
    w.generate_box(7, 4, 5, 7, 5, 5, "minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", "minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", false);
    for i10 in 0..=5 {
        for i11 in 0..=8 {
            fill_column_down(w, BRICKS, i11, -1, i10);
        }
    }
}

/// `NetherFortressPieces$CastleStalkRoom.postProcess`.
fn castle_stalk_room<W: StructureWorld + ?Sized>(w: &mut Writer<'_, W>) {
    w.generate_box(0, 3, 0, 12, 4, 12, BRICKS, BRICKS, false);
    w.generate_box(0, 5, 0, 12, 13, 12, AIR, AIR, false);
    w.generate_box(0, 5, 0, 1, 12, 12, BRICKS, BRICKS, false);
    w.generate_box(11, 5, 0, 12, 12, 12, BRICKS, BRICKS, false);
    w.generate_box(2, 5, 11, 4, 12, 12, BRICKS, BRICKS, false);
    w.generate_box(8, 5, 11, 10, 12, 12, BRICKS, BRICKS, false);
    w.generate_box(5, 9, 11, 7, 12, 12, BRICKS, BRICKS, false);
    w.generate_box(2, 5, 0, 4, 12, 1, BRICKS, BRICKS, false);
    w.generate_box(8, 5, 0, 10, 12, 1, BRICKS, BRICKS, false);
    w.generate_box(5, 9, 0, 7, 12, 1, BRICKS, BRICKS, false);
    w.generate_box(2, 11, 2, 10, 12, 10, BRICKS, BRICKS, false);
    for i12 in (1..=11).step_by(2) {
        w.generate_box(i12, 10, 0, i12, 11, 0, "minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", "minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", false);
        w.generate_box(i12, 10, 12, i12, 11, 12, "minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", "minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", false);
        w.generate_box(0, 10, i12, 0, 11, i12, "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", false);
        w.generate_box(12, 10, i12, 12, 11, i12, "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", false);
        w.place_block(BRICKS, i12, 13, 0);
        w.place_block(BRICKS, i12, 13, 12);
        w.place_block(BRICKS, 0, 13, i12);
        w.place_block(BRICKS, 12, 13, i12);
        if i12 != 11 {
            w.place_block("minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", i12 + 1, 13, 0);
            w.place_block("minecraft:nether_brick_fence[east=true,north=false,south=false,waterlogged=false,west=true]", i12 + 1, 13, 12);
            w.place_block("minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", 0, 13, i12 + 1);
            w.place_block("minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=false]", 12, 13, i12 + 1);
        }
    }
    w.place_block("minecraft:nether_brick_fence[east=true,north=true,south=false,waterlogged=false,west=false]", 0, 13, 0);
    w.place_block("minecraft:nether_brick_fence[east=true,north=false,south=true,waterlogged=false,west=false]", 0, 13, 12);
    w.place_block("minecraft:nether_brick_fence[east=false,north=false,south=true,waterlogged=false,west=true]", 12, 13, 12);
    w.place_block("minecraft:nether_brick_fence[east=false,north=true,south=false,waterlogged=false,west=true]", 12, 13, 0);
    for i12 in (3..=9).step_by(2) {
        w.generate_box(1, 7, i12, 1, 8, i12, "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=true]", "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=true]", false);
        w.generate_box(11, 7, i12, 11, 8, i12, "minecraft:nether_brick_fence[east=true,north=true,south=true,waterlogged=false,west=false]", "minecraft:nether_brick_fence[east=true,north=true,south=true,waterlogged=false,west=false]", false);
    }
    for i13 in 0..=6 {
        let i14 = i13 + 4;
        for i15 in 5..=7 {
            w.place_block("minecraft:nether_brick_stairs[facing=north,half=bottom,shape=straight,waterlogged=false]", i15, 5 + i13, i14);
        }
        if (5..=8).contains(&i14) {
            w.generate_box(5, 5, i14, 7, i13 + 4, i14, BRICKS, BRICKS, false);
        } else if (9..=10).contains(&i14) {
            w.generate_box(5, 8, i14, 7, i13 + 4, i14, BRICKS, BRICKS, false);
        }
        if i13 >= 1 {
            w.generate_box(5, 6 + i13, i14, 7, 9 + i13, i14, AIR, AIR, false);
        }
    }
    for i13 in 5..=7 {
        w.place_block("minecraft:nether_brick_stairs[facing=north,half=bottom,shape=straight,waterlogged=false]", i13, 12, 11);
    }
    w.generate_box(5, 6, 7, 5, 7, 7, "minecraft:nether_brick_fence[east=true,north=true,south=true,waterlogged=false,west=false]", "minecraft:nether_brick_fence[east=true,north=true,south=true,waterlogged=false,west=false]", false);
    w.generate_box(7, 6, 7, 7, 7, 7, "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=true]", "minecraft:nether_brick_fence[east=false,north=true,south=true,waterlogged=false,west=true]", false);
    w.generate_box(5, 13, 12, 7, 13, 12, AIR, AIR, false);
    w.generate_box(2, 5, 2, 3, 5, 3, BRICKS, BRICKS, false);
    w.generate_box(2, 5, 9, 3, 5, 10, BRICKS, BRICKS, false);
    w.generate_box(2, 5, 4, 2, 5, 8, BRICKS, BRICKS, false);
    w.generate_box(9, 5, 2, 10, 5, 3, BRICKS, BRICKS, false);
    w.generate_box(9, 5, 9, 10, 5, 10, BRICKS, BRICKS, false);
    w.generate_box(10, 5, 4, 10, 5, 8, BRICKS, BRICKS, false);
    w.place_block(
        "minecraft:nether_brick_stairs[facing=west,half=bottom,shape=straight,waterlogged=false]",
        4,
        5,
        2,
    );
    w.place_block(
        "minecraft:nether_brick_stairs[facing=west,half=bottom,shape=straight,waterlogged=false]",
        4,
        5,
        3,
    );
    w.place_block(
        "minecraft:nether_brick_stairs[facing=west,half=bottom,shape=straight,waterlogged=false]",
        4,
        5,
        9,
    );
    w.place_block(
        "minecraft:nether_brick_stairs[facing=west,half=bottom,shape=straight,waterlogged=false]",
        4,
        5,
        10,
    );
    w.place_block(
        "minecraft:nether_brick_stairs[facing=east,half=bottom,shape=straight,waterlogged=false]",
        8,
        5,
        2,
    );
    w.place_block(
        "minecraft:nether_brick_stairs[facing=east,half=bottom,shape=straight,waterlogged=false]",
        8,
        5,
        3,
    );
    w.place_block(
        "minecraft:nether_brick_stairs[facing=east,half=bottom,shape=straight,waterlogged=false]",
        8,
        5,
        9,
    );
    w.place_block(
        "minecraft:nether_brick_stairs[facing=east,half=bottom,shape=straight,waterlogged=false]",
        8,
        5,
        10,
    );
    w.generate_box(
        3,
        4,
        4,
        4,
        4,
        8,
        "minecraft:soul_sand",
        "minecraft:soul_sand",
        false,
    );
    w.generate_box(
        8,
        4,
        4,
        9,
        4,
        8,
        "minecraft:soul_sand",
        "minecraft:soul_sand",
        false,
    );
    w.generate_box(
        3,
        5,
        4,
        4,
        5,
        8,
        "minecraft:nether_wart[age=0]",
        "minecraft:nether_wart[age=0]",
        false,
    );
    w.generate_box(
        8,
        5,
        4,
        9,
        5,
        8,
        "minecraft:nether_wart[age=0]",
        "minecraft:nether_wart[age=0]",
        false,
    );
    w.generate_box(4, 2, 0, 8, 2, 12, BRICKS, BRICKS, false);
    w.generate_box(0, 2, 4, 12, 2, 8, BRICKS, BRICKS, false);
    w.generate_box(4, 0, 0, 8, 1, 3, BRICKS, BRICKS, false);
    w.generate_box(4, 0, 9, 8, 1, 12, BRICKS, BRICKS, false);
    w.generate_box(0, 0, 4, 3, 1, 8, BRICKS, BRICKS, false);
    w.generate_box(9, 0, 4, 12, 1, 8, BRICKS, BRICKS, false);
    for i15 in 4..=8 {
        for i16 in 0..=2 {
            fill_column_down(w, BRICKS, i15, -1, i16);
            fill_column_down(w, BRICKS, i15, -1, 12 - i16);
        }
    }
    for i15 in 0..=2 {
        for i16 in 4..=8 {
            fill_column_down(w, BRICKS, i15, -1, i16);
            fill_column_down(w, BRICKS, 12 - i15, -1, i16);
        }
    }
}
