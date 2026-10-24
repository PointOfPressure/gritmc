//! Strongholds: the piece walk (`StrongholdPieces` and
//! `StrongholdStructure.generatePieces`), from the 26.2 bytecode.
//!
//! # The walk, and the state it keeps
//!
//! `StrongholdPieces` keeps its piece-weight table in **static** fields —
//! `currentPieces`, `totalWeight`, `imposedPiece` — reset by `resetPieces` at
//! the start of every attempt. [`Walk`] holds them per start instead, which is
//! the same thing for a single-threaded caller and the only sound thing for a
//! parallel one.
//!
//! `generatePieceFromSmallDoor` is the heart and its loop has one shape that is
//! easy to get wrong: a roll that lands on a piece type whose `createPiece`
//! refuses (no room) does **not** end the attempt — the iterator carries on
//! with the roll still negative, so the *next* type in table order is tried.
//! Only a type that is exhausted (`doPlace`) or was the previous piece ends
//! the attempt.

use super::pieces::{BoundingBox, Facing, Piece};
use crate::rng::RandomSource;

/// `StrongholdPiece.SmallDoorType`, in declaration order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Door {
    Opening,
    WoodDoor,
    Grates,
    IronDoor,
}

/// Which `StrongholdPiece` subclass, with the fields its constructor draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `StairsDown`; `source` is the start piece (`isSource`).
    StairsDown {
        source: bool,
    },
    Straight {
        left: bool,
        right: bool,
    },
    PrisonHall,
    LeftTurn,
    RightTurn,
    /// `RoomCrossing.type`, `nextInt(5)`.
    RoomCrossing {
        kind: i32,
    },
    StraightStairsDown,
    FiveCrossing {
        left_low: bool,
        left_high: bool,
        right_low: bool,
        right_high: bool,
    },
    ChestCorridor,
    /// `Library.isTall`.
    Library {
        tall: bool,
    },
    PortalRoom,
    /// `FillerCorridor.steps`.
    FillerCorridor {
        steps: i32,
    },
}

impl Kind {
    /// The `StructurePieceType` id vanilla saves.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::StairsDown { source: true } => "minecraft:shstart",
            Self::StairsDown { source: false } => "minecraft:shsd",
            Self::Straight { .. } => "minecraft:shs",
            Self::PrisonHall => "minecraft:shph",
            Self::LeftTurn => "minecraft:shlt",
            Self::RightTurn => "minecraft:shrt",
            Self::RoomCrossing { .. } => "minecraft:shrc",
            Self::StraightStairsDown => "minecraft:shssd",
            Self::FiveCrossing { .. } => "minecraft:sh5c",
            Self::ChestCorridor => "minecraft:shcc",
            Self::Library { .. } => "minecraft:shli",
            Self::PortalRoom => "minecraft:shpr",
            Self::FillerCorridor { .. } => "minecraft:shfc",
        }
    }
}

/// One stronghold piece: the layout half (`Piece`), its subclass, its entry
/// door, and the once-only flags its `postProcess` latches.
#[derive(Clone, Debug)]
pub struct ShPiece {
    pub piece: Piece,
    pub kind: Kind,
    pub door: Door,
    /// `ChestCorridor.hasPlacedChest`.
    pub placed_chest: bool,
    /// `PortalRoom.hasPlacedSpawner`.
    pub placed_spawner: bool,
}

/// The table rows of `STRONGHOLD_PIECE_WEIGHTS`, in `<clinit>` order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Type {
    Straight,
    PrisonHall,
    LeftTurn,
    RightTurn,
    RoomCrossing,
    StraightStairsDown,
    StairsDown,
    FiveCrossing,
    ChestCorridor,
    Library,
    PortalRoom,
}

#[derive(Clone, Copy, Debug)]
struct Weight {
    kind: Type,
    weight: i32,
    place_count: i32,
    max_place_count: i32,
}

impl Weight {
    /// `PieceWeight.doPlace`, with the Library's (`depth > 4`) and the portal
    /// room's (`depth > 5`) overrides.
    const fn do_place(&self, depth: i32) -> bool {
        let base = self.max_place_count == 0 || self.place_count < self.max_place_count;
        match self.kind {
            Type::Library => base && depth > 4,
            Type::PortalRoom => base && depth > 5,
            _ => base,
        }
    }

    const fn is_valid(&self) -> bool {
        self.max_place_count == 0 || self.place_count < self.max_place_count
    }
}

const WEIGHTS: [(Type, i32, i32); 11] = [
    (Type::Straight, 40, 0),
    (Type::PrisonHall, 5, 5),
    (Type::LeftTurn, 20, 0),
    (Type::RightTurn, 20, 0),
    (Type::RoomCrossing, 10, 6),
    (Type::StraightStairsDown, 5, 5),
    (Type::StairsDown, 5, 5),
    (Type::FiveCrossing, 5, 4),
    (Type::ChestCorridor, 5, 4),
    (Type::Library, 10, 2),
    (Type::PortalRoom, 20, 1),
];

/// `BoundingBox.orientBox(x, y, z, offX, offY, offZ, width, height, depth,
/// direction)`.
#[must_use]
#[expect(clippy::too_many_arguments, reason = "the reference's own signature")]
pub const fn orient_box(
    x: i32,
    y: i32,
    z: i32,
    off_x: i32,
    off_y: i32,
    off_z: i32,
    width: i32,
    height: i32,
    depth: i32,
    facing: Facing,
) -> BoundingBox {
    match facing {
        Facing::North => BoundingBox::new(
            x + off_x,
            y + off_y,
            z - depth + 1 + off_z,
            x + width - 1 + off_x,
            y + height - 1 + off_y,
            z + off_z,
        ),
        Facing::West => BoundingBox::new(
            x - depth + 1 + off_z,
            y + off_y,
            z + off_x,
            x + off_z,
            y + height - 1 + off_y,
            z + width - 1 + off_x,
        ),
        Facing::East => BoundingBox::new(
            x + off_z,
            y + off_y,
            z + off_x,
            x + depth - 1 + off_z,
            y + height - 1 + off_y,
            z + width - 1 + off_x,
        ),
        Facing::South => BoundingBox::new(
            x + off_x,
            y + off_y,
            z + off_z,
            x + width - 1 + off_x,
            y + height - 1 + off_y,
            z + depth - 1 + off_z,
        ),
    }
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

/// `StrongholdPiece.randomSmallDoor`: `nextInt(5)`, 0 and 1 an opening.
fn random_small_door<R: RandomSource + ?Sized>(random: &mut R) -> Door {
    match random.next_i32_bound(5) {
        0 | 1 => Door::Opening,
        2 => Door::WoodDoor,
        3 => Door::Grates,
        _ => Door::IronDoor,
    }
}

/// `StrongholdPiece.isOkBox` — above y 10.
const fn is_ok_box(bb: BoundingBox) -> bool {
    bb.min_y > 10
}

/// One attempt's walk state: the static fields of `StrongholdPieces` and the
/// start piece's `previousPiece`, `portalRoomPiece` and `pendingChildren`.
struct Walk {
    current: Vec<Weight>,
    total_weight: i32,
    imposed: Option<Type>,
    previous: Option<Type>,
    portal_room: bool,
    pieces: Vec<ShPiece>,
    pending: Vec<usize>,
}

impl Walk {
    fn new() -> Self {
        Self {
            current: WEIGHTS
                .iter()
                .map(|&(kind, weight, max_place_count)| Weight {
                    kind,
                    weight,
                    place_count: 0,
                    max_place_count,
                })
                .collect(),
            total_weight: 0,
            imposed: None,
            previous: None,
            portal_room: false,
            pieces: Vec::new(),
            pending: Vec::new(),
        }
    }

    fn collides(&self, bb: BoundingBox) -> bool {
        self.pieces
            .iter()
            .any(|p| p.piece.bounding_box.intersects(bb))
    }

    /// `updatePieceWeight`.
    fn update_piece_weight(&mut self) -> bool {
        let mut any = false;
        self.total_weight = 0;
        for w in &self.current {
            if w.max_place_count > 0 && w.place_count < w.max_place_count {
                any = true;
            }
            self.total_weight += w.weight;
        }
        any
    }

    /// `findAndCreatePieceFactory` — each type's `createPiece`, then its
    /// constructor's draws: `randomSmallDoor` (every type but the portal
    /// room), then the type's own fields.
    #[expect(clippy::too_many_arguments, reason = "the reference's own signature")]
    fn create<R: RandomSource + ?Sized>(
        &self,
        kind: Type,
        random: &mut R,
        x: i32,
        y: i32,
        z: i32,
        facing: Facing,
        depth: i32,
    ) -> Option<ShPiece> {
        let ok = |bb: BoundingBox| is_ok_box(bb) && !self.collides(bb);
        let bb = match kind {
            Type::Straight | Type::ChestCorridor => orient_box(x, y, z, -1, -1, 0, 5, 5, 7, facing),
            Type::PrisonHall => orient_box(x, y, z, -1, -1, 0, 9, 5, 11, facing),
            Type::LeftTurn | Type::RightTurn => orient_box(x, y, z, -1, -1, 0, 5, 5, 5, facing),
            Type::RoomCrossing => orient_box(x, y, z, -4, -1, 0, 11, 7, 11, facing),
            Type::StraightStairsDown => orient_box(x, y, z, -1, -7, 0, 5, 11, 8, facing),
            Type::StairsDown => orient_box(x, y, z, -1, -7, 0, 5, 11, 5, facing),
            Type::FiveCrossing => orient_box(x, y, z, -4, -3, 0, 10, 9, 11, facing),
            Type::Library => {
                // Tall first, then the short one, each with its own test.
                let tall = orient_box(x, y, z, -4, -1, 0, 14, 11, 15, facing);
                if ok(tall) {
                    tall
                } else {
                    orient_box(x, y, z, -4, -1, 0, 14, 6, 15, facing)
                }
            }
            Type::PortalRoom => orient_box(x, y, z, -4, -1, 0, 11, 8, 16, facing),
        };
        if !ok(bb) {
            return None;
        }
        let door = if kind == Type::PortalRoom {
            Door::Opening
        } else {
            random_small_door(random)
        };
        let kind = match kind {
            Type::Straight => {
                let left = random.next_i32_bound(2) == 0;
                let right = random.next_i32_bound(2) == 0;
                Kind::Straight { left, right }
            }
            Type::PrisonHall => Kind::PrisonHall,
            Type::LeftTurn => Kind::LeftTurn,
            Type::RightTurn => Kind::RightTurn,
            Type::RoomCrossing => Kind::RoomCrossing {
                kind: random.next_i32_bound(5),
            },
            Type::StraightStairsDown => Kind::StraightStairsDown,
            Type::StairsDown => Kind::StairsDown { source: false },
            Type::FiveCrossing => {
                let left_low = random.next_bool();
                let left_high = random.next_bool();
                let right_low = random.next_bool();
                let right_high = random.next_i32_bound(3) > 0;
                Kind::FiveCrossing {
                    left_low,
                    left_high,
                    right_low,
                    right_high,
                }
            }
            Type::ChestCorridor => Kind::ChestCorridor,
            Type::Library => Kind::Library {
                tall: bb.y_span() > 6,
            },
            Type::PortalRoom => Kind::PortalRoom,
        };
        Some(ShPiece {
            piece: Piece {
                id: kind.id(),
                bounding_box: bb,
                facing: Some(facing),
                depth,
            },
            kind,
            door,
            placed_chest: false,
            placed_spawner: false,
        })
    }

    /// `generatePieceFromSmallDoor`.
    fn piece_from_small_door<R: RandomSource + ?Sized>(
        &mut self,
        random: &mut R,
        x: i32,
        y: i32,
        z: i32,
        facing: Facing,
        depth: i32,
    ) -> Option<ShPiece> {
        if !self.update_piece_weight() {
            return None;
        }
        if let Some(imposed) = self.imposed.take() {
            if let Some(piece) = self.create(imposed, random, x, y, z, facing, depth) {
                return Some(piece);
            }
        }
        for _ in 0..5 {
            let mut roll = random.next_i32_bound(self.total_weight);
            let mut i = 0;
            while i < self.current.len() {
                let w = self.current[i];
                roll -= w.weight;
                if roll < 0 {
                    if !w.do_place(depth) || self.previous == Some(w.kind) {
                        break;
                    }
                    if let Some(piece) = self.create(w.kind, random, x, y, z, facing, depth) {
                        self.current[i].place_count += 1;
                        self.previous = Some(w.kind);
                        if !self.current[i].is_valid() {
                            self.current.remove(i);
                        }
                        return Some(piece);
                    }
                }
                i += 1;
            }
        }
        // `FillerCorridor.findPieceBox`, and only above y 1.
        let bb = self.filler_box(x, y, z, facing)?;
        if bb.min_y <= 1 {
            return None;
        }
        let steps = match facing {
            Facing::North | Facing::South => bb.max_z - bb.min_z + 1,
            Facing::West | Facing::East => bb.max_x - bb.min_x + 1,
        };
        Some(ShPiece {
            piece: Piece {
                id: Kind::FillerCorridor { steps }.id(),
                bounding_box: bb,
                facing: Some(facing),
                depth,
            },
            kind: Kind::FillerCorridor { steps },
            door: Door::Opening,
            placed_chest: false,
            placed_spawner: false,
        })
    }

    /// `FillerCorridor.findPieceBox`: a 5x5x4 box must collide with
    /// something at its own floor height; then the longest of 3, 2, 1 steps
    /// that does not, plus one.
    fn filler_box(&self, x: i32, y: i32, z: i32, facing: Facing) -> Option<BoundingBox> {
        let probe = orient_box(x, y, z, -1, -1, 0, 5, 5, 4, facing);
        let hit = self
            .pieces
            .iter()
            .find(|p| p.piece.bounding_box.intersects(probe))?
            .piece
            .bounding_box;
        if hit.min_y != probe.min_y {
            return None;
        }
        let mut steps = 2;
        while steps >= 1 {
            let bb = orient_box(x, y, z, -1, -1, 0, 5, 5, steps, facing);
            if !hit.intersects(bb) {
                return Some(orient_box(x, y, z, -1, -1, 0, 5, 5, steps + 1, facing));
            }
            steps -= 1;
        }
        None
    }

    /// `generateAndAddPiece`.
    fn add<R: RandomSource + ?Sized>(
        &mut self,
        random: &mut R,
        x: i32,
        y: i32,
        z: i32,
        facing: Facing,
        depth: i32,
    ) {
        if depth > 50 {
            return;
        }
        let start = self.pieces[0].piece.bounding_box;
        if (x - start.min_x).abs() > 112 || (z - start.min_z).abs() > 112 {
            return;
        }
        if let Some(piece) = self.piece_from_small_door(random, x, y, z, facing, depth + 1) {
            if piece.kind == Kind::PortalRoom {
                // `PortalRoom.addChildren` records itself on the start when
                // its turn comes; it has no children, so marking it here is
                // the same fact a step early and changes no draw.
            }
            self.pieces.push(piece);
            self.pending.push(self.pieces.len() - 1);
        }
    }

    /// `generateSmallDoorChildForward(start, accessor, random, a, b)`.
    fn forward<R: RandomSource + ?Sized>(&mut self, random: &mut R, at: usize, a: i32, b: i32) {
        let p = self.pieces[at].piece.clone();
        let bb = p.bounding_box;
        let Some(f) = p.facing else { return };
        match f {
            Facing::North => self.add(random, bb.min_x + a, bb.min_y + b, bb.min_z - 1, f, p.depth),
            Facing::South => self.add(random, bb.min_x + a, bb.min_y + b, bb.max_z + 1, f, p.depth),
            Facing::West => self.add(random, bb.min_x - 1, bb.min_y + b, bb.min_z + a, f, p.depth),
            Facing::East => self.add(random, bb.max_x + 1, bb.min_y + b, bb.min_z + a, f, p.depth),
        }
    }

    /// `generateSmallDoorChildLeft(start, accessor, random, yOff, hOff)`.
    fn left<R: RandomSource + ?Sized>(&mut self, random: &mut R, at: usize, a: i32, b: i32) {
        let p = self.pieces[at].piece.clone();
        let bb = p.bounding_box;
        let Some(f) = p.facing else { return };
        match f {
            Facing::North | Facing::South => {
                self.add(
                    random,
                    bb.min_x - 1,
                    bb.min_y + a,
                    bb.min_z + b,
                    Facing::West,
                    p.depth,
                );
            }
            Facing::West | Facing::East => {
                self.add(
                    random,
                    bb.min_x + b,
                    bb.min_y + a,
                    bb.min_z - 1,
                    Facing::North,
                    p.depth,
                );
            }
        }
    }

    /// `generateSmallDoorChildRight(start, accessor, random, yOff, hOff)`.
    fn right<R: RandomSource + ?Sized>(&mut self, random: &mut R, at: usize, a: i32, b: i32) {
        let p = self.pieces[at].piece.clone();
        let bb = p.bounding_box;
        let Some(f) = p.facing else { return };
        match f {
            Facing::North | Facing::South => {
                self.add(
                    random,
                    bb.max_x + 1,
                    bb.min_y + a,
                    bb.min_z + b,
                    Facing::East,
                    p.depth,
                );
            }
            Facing::West | Facing::East => {
                self.add(
                    random,
                    bb.min_x + b,
                    bb.min_y + a,
                    bb.max_z + 1,
                    Facing::South,
                    p.depth,
                );
            }
        }
    }

    /// Each type's `addChildren`.
    fn add_children<R: RandomSource + ?Sized>(&mut self, random: &mut R, at: usize) {
        let kind = self.pieces[at].kind;
        let facing = self.pieces[at].piece.facing;
        match kind {
            Kind::StairsDown { source } => {
                if source {
                    self.imposed = Some(Type::FiveCrossing);
                }
                self.forward(random, at, 1, 1);
            }
            Kind::Straight { left, right } => {
                self.forward(random, at, 1, 1);
                if left {
                    self.left(random, at, 1, 2);
                }
                if right {
                    self.right(random, at, 1, 2);
                }
            }
            Kind::PrisonHall | Kind::StraightStairsDown | Kind::ChestCorridor => {
                self.forward(random, at, 1, 1);
            }
            Kind::LeftTurn => {
                if matches!(facing, Some(Facing::North | Facing::East)) {
                    self.left(random, at, 1, 1);
                } else {
                    self.right(random, at, 1, 1);
                }
            }
            Kind::RightTurn => {
                if matches!(facing, Some(Facing::North | Facing::East)) {
                    self.right(random, at, 1, 1);
                } else {
                    self.left(random, at, 1, 1);
                }
            }
            Kind::RoomCrossing { .. } => {
                self.forward(random, at, 4, 1);
                self.left(random, at, 1, 4);
                self.right(random, at, 1, 4);
            }
            Kind::FiveCrossing {
                left_low,
                left_high,
                right_low,
                right_high,
            } => {
                let (mut low, mut high) = (3, 5);
                if matches!(facing, Some(Facing::West | Facing::North)) {
                    low = 8 - low;
                    high = 8 - high;
                }
                self.forward(random, at, 5, 1);
                if left_low {
                    self.left(random, at, low, 1);
                }
                if left_high {
                    self.left(random, at, high, 7);
                }
                if right_low {
                    self.right(random, at, low, 1);
                }
                if right_high {
                    self.right(random, at, high, 7);
                }
            }
            Kind::PortalRoom => self.portal_room = true,
            Kind::Library { .. } | Kind::FillerCorridor { .. } => {}
        }
    }
}

/// A stronghold start: its chunk and its pieces, already lowered.
#[derive(Clone, Debug)]
pub struct Stronghold {
    pub start_chunk: (i32, i32),
    pub pieces: Vec<ShPiece>,
}

/// `StrongholdStructure.generatePieces` on the structure's own
/// `GenerationContext.random` — `setLargeFeatureSeed(seed + i, x, z)` for
/// attempt `i` until one builds a portal room.
#[must_use]
pub fn generate(seed: i64, chunk: (i32, i32), sea_level: i32, min_y: i32) -> Stronghold {
    use crate::rng::legacy::LegacyRandomSource;
    use crate::rng::worldgen_random::WorldgenRandom;
    let mut random = WorldgenRandom::new(LegacyRandomSource::new(0));
    let random = &mut random;
    let mut attempt: i64 = 0;
    loop {
        random.set_large_feature_seed(seed.wrapping_add(attempt), chunk.0, chunk.1);
        attempt += 1;
        let mut walk = Walk::new();
        // `new StartPiece(random, chunk.getBlockX(2), chunk.getBlockZ(2))`:
        // a `StairsDown` source at y 64, its facing drawn first.
        let facing = random_horizontal(random);
        let bb = super::scattered::make_bounding_box(
            chunk.0 * 16 + 2,
            64,
            chunk.1 * 16 + 2,
            facing,
            5,
            11,
            5,
        );
        walk.pieces.push(ShPiece {
            piece: Piece {
                id: Kind::StairsDown { source: true }.id(),
                bounding_box: bb,
                facing: Some(facing),
                depth: 0,
            },
            kind: Kind::StairsDown { source: true },
            door: Door::Opening,
            placed_chest: false,
            placed_spawner: false,
        });
        walk.add_children(random, 0);
        while !walk.pending.is_empty() {
            let len = i32::try_from(walk.pending.len()).unwrap_or(i32::MAX);
            let index = usize::try_from(random.next_i32_bound(len)).unwrap_or(0);
            let at = walk.pending.remove(index);
            walk.add_children(random, at);
        }
        // `moveBelowSeaLevel(seaLevel, minY, random, 10)` over the builder.
        let boxes: Vec<BoundingBox> = walk.pieces.iter().map(|p| p.piece.bounding_box).collect();
        if let Some(bounds) = BoundingBox::encapsulating(&boxes) {
            let ceiling = sea_level - 10;
            let mut top = bounds.y_span() + min_y + 1;
            if top < ceiling {
                top += random.next_i32_bound(ceiling - top);
            }
            let dy = top - bounds.max_y;
            for p in &mut walk.pieces {
                p.piece.bounding_box.shift(0, dy, 0);
            }
        }
        if !walk.pieces.is_empty() && walk.portal_room {
            return Stronghold {
                start_chunk: chunk,
                pieces: walk.pieces,
            };
        }
    }
}

/// `ChunkGeneratorStructureState.generateRingPositions` up to the biome
/// search: each stronghold's ring-relative chunk and the forked random its
/// search will use, in generation order. The search itself
/// ([`find_biome_horizontal`]) is the expensive half and is run only for the
/// positions a caller can reach.
#[must_use]
pub fn ring_candidates(
    seed: i64,
    distance: i32,
    spread: i32,
    count: i32,
) -> Vec<((i32, i32), i64)> {
    use crate::rng::legacy::LegacyRandomSource;
    let mut random = LegacyRandomSource::new(seed);
    let mut out = Vec::new();
    let tau = std::f64::consts::PI * 2.0;
    let mut angle = random.next_f64() * std::f64::consts::PI * 2.0;
    let mut position_in_ring = 0;
    let mut ring = 0;
    let mut spread = spread;
    for i in 0..count {
        let dist = f64::from(4 * distance + distance * ring * 6)
            + (random.next_f64() - 0.5) * (f64::from(distance) * 2.5);
        // `Math.round(double)` is `floor(x + 0.5)`, not Rust's half-away.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "`l2i` of a chunk coordinate"
        )]
        let cx = (angle.cos() * dist + 0.5).floor() as i64 as i32;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "`l2i` of a chunk coordinate"
        )]
        let cz = (angle.sin() * dist + 0.5).floor() as i64 as i32;
        // `random.fork()` on a legacy source is a new one seeded `nextLong()`.
        let fork_seed = random.next_i64();
        out.push(((cx, cz), fork_seed));
        angle += tau / f64::from(spread);
        position_in_ring += 1;
        if position_in_ring == spread {
            ring += 1;
            position_in_ring = 0;
            spread += 2 * spread / (ring + 1);
            spread = spread.min(count - i);
            angle += random.next_f64() * std::f64::consts::PI * 2.0;
        }
    }
    out
}

/// `BiomeSource.findBiomeHorizontal(x, y, z, radius, 1, predicate, random,
/// false, sampler)`: the whole `(2r+1)^2` quart square, z outer and x inner,
/// keeping a matching cell by reservoir sampling — the first match without a
/// draw, each later one with `nextInt(found + 1) == 0`.
pub fn find_biome_horizontal(
    x: i32,
    y: i32,
    z: i32,
    radius: i32,
    fork_seed: i64,
    matches: &mut dyn FnMut(i32, i32, i32) -> bool,
) -> Option<(i32, i32, i32)> {
    use crate::rng::legacy::LegacyRandomSource;
    let mut random = LegacyRandomSource::new(fork_seed);
    let (qx, qz, qr, qy) = (x >> 2, z >> 2, radius >> 2, y >> 2);
    let mut result = None;
    let mut found = 0;
    let r = qr;
    let mut dz = -r;
    while dz <= r {
        let mut dx = -r;
        while dx <= r {
            let (bx, bz) = (qx + dx, qz + dz);
            if matches(bx, qy, bz) {
                if result.is_none() || random.next_i32_bound(found + 1) == 0 {
                    result = Some((bx << 2, y, bz << 2));
                }
                found += 1;
            }
            dx += 1;
        }
        dz += 1;
    }
    result
}

use super::scattered::create_chest;
use super::writer::{StructureWorld, Writer};

/// `StrongholdPieces.SmoothStoneSelector.next`: an edge position draws one
/// `nextFloat` — below 0.2 cracked, 0.5 mossy, 0.55 infested, else plain
/// stone bricks; an interior one is cave air and draws nothing.
fn smooth_box<W: StructureWorld + ?Sized, R: RandomSource + ?Sized>(
    w: &mut Writer<'_, W>,
    random: &mut R,
    s: &dyn Fn(&str) -> String,
    bounds: [i32; 6],
    skip_air: bool,
) {
    let [x0, y0, z0, x1, y1, z1] = bounds;
    for y in y0..=y1 {
        for x in x0..=x1 {
            for z in z0..=z1 {
                if skip_air && super::writer::is_air(&w.get_block(x, y, z)) {
                    continue;
                }
                let edge = y == y0 || y == y1 || x == x0 || x == x1 || z == z0 || z == z1;
                let state = if edge {
                    let f = random.next_f32();
                    if f < 0.2 {
                        "minecraft:cracked_stone_bricks"
                    } else if f < 0.5 {
                        "minecraft:mossy_stone_bricks"
                    } else if f < 0.55 {
                        "minecraft:infested_stone_bricks"
                    } else {
                        "minecraft:stone_bricks"
                    }
                } else {
                    "minecraft:cave_air"
                };
                w.place_block(&s(state), x, y, z);
            }
        }
    }
}

/// `StrongholdPiece.generateSmallDoor`.
fn small_door<W: StructureWorld + ?Sized>(
    w: &mut Writer<'_, W>,
    s: &dyn Fn(&str) -> String,
    door: Door,
    x: i32,
    y: i32,
    z: i32,
) {
    let bricks = s("minecraft:stone_bricks");
    let frame = |w: &mut Writer<'_, W>| {
        w.place_block(&bricks, x, y, z);
        w.place_block(&bricks, x, y + 1, z);
        w.place_block(&bricks, x, y + 2, z);
        w.place_block(&bricks, x + 1, y + 2, z);
        w.place_block(&bricks, x + 2, y + 2, z);
        w.place_block(&bricks, x + 2, y + 1, z);
        w.place_block(&bricks, x + 2, y, z);
    };
    match door {
        Door::Opening => {
            let air = s("minecraft:cave_air");
            w.generate_box(x, y, z, x + 2, y + 2, z, &air, &air, false);
        }
        Door::WoodDoor => {
            frame(w);
            w.place_block(&s("minecraft:oak_door"), x + 1, y, z);
            w.place_block(&s("minecraft:oak_door[half=upper]"), x + 1, y + 1, z);
        }
        Door::Grates => {
            let air = s("minecraft:cave_air");
            w.place_block(&air, x + 1, y, z);
            w.place_block(&air, x + 1, y + 1, z);
            let west = s("minecraft:iron_bars[west=true]");
            let both = s("minecraft:iron_bars[east=true,west=true]");
            let east = s("minecraft:iron_bars[east=true]");
            w.place_block(&west, x, y, z);
            w.place_block(&west, x, y + 1, z);
            w.place_block(&both, x, y + 2, z);
            w.place_block(&both, x + 1, y + 2, z);
            w.place_block(&both, x + 2, y + 2, z);
            w.place_block(&east, x + 2, y + 1, z);
            w.place_block(&east, x + 2, y, z);
        }
        Door::IronDoor => {
            frame(w);
            w.place_block(&s("minecraft:iron_door"), x + 1, y, z);
            w.place_block(&s("minecraft:iron_door[half=upper]"), x + 1, y + 1, z);
            w.place_block(
                &s("minecraft:stone_button[facing=north]"),
                x + 2,
                y + 1,
                z + 1,
            );
            w.place_block(
                &s("minecraft:stone_button[facing=south]"),
                x + 2,
                y + 1,
                z - 1,
            );
        }
    }
}

/// Every piece's `postProcess`, for one chunk's writable box.
#[expect(
    clippy::too_many_lines,
    reason = "twelve vanilla methods, kept in order"
)]
pub fn post_process<W: StructureWorld + ?Sized, R: RandomSource + ?Sized>(
    world: &mut W,
    p: &mut ShPiece,
    clip: BoundingBox,
    random: &mut R,
    s: &dyn Fn(&str) -> String,
) {
    let piece = p.piece.clone();
    let mut w = Writer {
        world,
        piece: &piece,
        clip,
        protected: &[],
    };
    let w = &mut w;
    let air = s("minecraft:cave_air");
    let bricks = s("minecraft:stone_bricks");
    match p.kind {
        Kind::Straight { left, right } => {
            smooth_box(w, random, s, [0, 0, 0, 4, 4, 6], true);
            small_door(w, s, p.door, 1, 1, 0);
            small_door(w, s, Door::Opening, 1, 1, 6);
            let east = s("minecraft:wall_torch[facing=east]");
            let west = s("minecraft:wall_torch[facing=west]");
            w.maybe_generate_block(random, 0.1, 1, 2, 1, &east);
            w.maybe_generate_block(random, 0.1, 3, 2, 1, &west);
            w.maybe_generate_block(random, 0.1, 1, 2, 5, &east);
            w.maybe_generate_block(random, 0.1, 3, 2, 5, &west);
            if left {
                w.generate_box(0, 1, 2, 0, 3, 4, &air, &air, false);
            }
            if right {
                w.generate_box(4, 1, 2, 4, 3, 4, &air, &air, false);
            }
        }
        Kind::ChestCorridor => {
            smooth_box(w, random, s, [0, 0, 0, 4, 4, 6], true);
            small_door(w, s, p.door, 1, 1, 0);
            small_door(w, s, Door::Opening, 1, 1, 6);
            w.generate_box(3, 1, 2, 3, 1, 4, &bricks, &bricks, false);
            let slab = s("minecraft:stone_brick_slab");
            w.place_block(&slab, 3, 1, 1);
            w.place_block(&slab, 3, 1, 5);
            w.place_block(&slab, 3, 2, 2);
            w.place_block(&slab, 3, 2, 4);
            for z in 2..=4 {
                w.place_block(&slab, 2, 1, z);
            }
            if !p.placed_chest && w.inside_box(3, 2, 3) {
                p.placed_chest = true;
                create_chest(
                    w,
                    random,
                    &s("minecraft:chest"),
                    3,
                    2,
                    3,
                    "minecraft:chests/stronghold_corridor",
                );
            }
        }
        Kind::PrisonHall => {
            smooth_box(w, random, s, [0, 0, 0, 8, 4, 10], true);
            small_door(w, s, p.door, 1, 1, 0);
            w.generate_box(1, 1, 10, 3, 3, 10, &air, &air, false);
            smooth_box(w, random, s, [4, 1, 1, 4, 3, 1], false);
            smooth_box(w, random, s, [4, 1, 3, 4, 3, 3], false);
            smooth_box(w, random, s, [4, 1, 7, 4, 3, 7], false);
            smooth_box(w, random, s, [4, 1, 9, 4, 3, 9], false);
            let ns = s("minecraft:iron_bars[north=true,south=true]");
            let nse = s("minecraft:iron_bars[east=true,north=true,south=true]");
            let we = s("minecraft:iron_bars[east=true,west=true]");
            for y in 1..=3 {
                w.place_block(&ns, 4, y, 4);
                w.place_block(&nse, 4, y, 5);
                w.place_block(&ns, 4, y, 6);
                w.place_block(&we, 5, y, 5);
                w.place_block(&we, 6, y, 5);
                w.place_block(&we, 7, y, 5);
            }
            w.place_block(&ns, 4, 3, 2);
            w.place_block(&ns, 4, 3, 8);
            let lower = s("minecraft:iron_door[facing=west]");
            let upper = s("minecraft:iron_door[facing=west,half=upper]");
            w.place_block(&lower, 4, 1, 2);
            w.place_block(&upper, 4, 2, 2);
            w.place_block(&lower, 4, 1, 8);
            w.place_block(&upper, 4, 2, 8);
        }
        Kind::LeftTurn | Kind::RightTurn => {
            smooth_box(w, random, s, [0, 0, 0, 4, 4, 4], true);
            small_door(w, s, p.door, 1, 1, 0);
            let north_or_east = matches!(piece.facing, Some(Facing::North | Facing::East));
            let low_side = (kind_is_left(p.kind)) == north_or_east;
            if low_side {
                w.generate_box(0, 1, 1, 0, 3, 3, &air, &air, false);
            } else {
                w.generate_box(4, 1, 1, 4, 3, 3, &air, &air, false);
            }
        }
        Kind::RoomCrossing { kind } => {
            smooth_box(w, random, s, [0, 0, 0, 10, 6, 10], true);
            small_door(w, s, p.door, 4, 1, 0);
            w.generate_box(4, 1, 10, 6, 3, 10, &air, &air, false);
            w.generate_box(0, 1, 4, 0, 3, 6, &air, &air, false);
            w.generate_box(10, 1, 4, 10, 3, 6, &air, &air, false);
            room_crossing_body(w, random, s, kind);
        }
        Kind::StraightStairsDown => {
            smooth_box(w, random, s, [0, 0, 0, 4, 10, 7], true);
            small_door(w, s, p.door, 1, 7, 0);
            small_door(w, s, Door::Opening, 1, 1, 7);
            let stairs = s("minecraft:cobblestone_stairs[facing=south]");
            for i in 0..6 {
                w.place_block(&stairs, 1, 6 - i, 1 + i);
                w.place_block(&stairs, 2, 6 - i, 1 + i);
                w.place_block(&stairs, 3, 6 - i, 1 + i);
                if i < 5 {
                    w.place_block(&bricks, 1, 5 - i, 1 + i);
                    w.place_block(&bricks, 2, 5 - i, 1 + i);
                    w.place_block(&bricks, 3, 5 - i, 1 + i);
                }
            }
        }
        Kind::StairsDown { .. } => {
            smooth_box(w, random, s, [0, 0, 0, 4, 10, 4], true);
            small_door(w, s, p.door, 1, 7, 0);
            small_door(w, s, Door::Opening, 1, 1, 4);
            let slab = s("minecraft:smooth_stone_slab");
            w.place_block(&bricks, 2, 6, 1);
            w.place_block(&bricks, 1, 5, 1);
            w.place_block(&slab, 1, 6, 1);
            w.place_block(&bricks, 1, 5, 2);
            w.place_block(&bricks, 1, 4, 3);
            w.place_block(&slab, 1, 5, 3);
            w.place_block(&bricks, 2, 4, 3);
            w.place_block(&bricks, 3, 3, 3);
            w.place_block(&slab, 3, 4, 3);
            w.place_block(&bricks, 3, 3, 2);
            w.place_block(&bricks, 3, 2, 1);
            w.place_block(&slab, 3, 3, 1);
            w.place_block(&bricks, 2, 2, 1);
            w.place_block(&bricks, 1, 1, 1);
            w.place_block(&slab, 1, 2, 1);
            w.place_block(&bricks, 1, 1, 2);
            w.place_block(&slab, 1, 1, 3);
        }
        Kind::FiveCrossing {
            left_low,
            left_high,
            right_low,
            right_high,
        } => {
            smooth_box(w, random, s, [0, 0, 0, 9, 8, 10], true);
            small_door(w, s, p.door, 4, 3, 0);
            if left_low {
                w.generate_box(0, 3, 1, 0, 5, 3, &air, &air, false);
            }
            if right_low {
                w.generate_box(9, 3, 1, 9, 5, 3, &air, &air, false);
            }
            if left_high {
                w.generate_box(0, 5, 7, 0, 7, 9, &air, &air, false);
            }
            if right_high {
                w.generate_box(9, 5, 7, 9, 7, 9, &air, &air, false);
            }
            w.generate_box(5, 1, 10, 7, 3, 10, &air, &air, false);
            smooth_box(w, random, s, [1, 2, 1, 8, 2, 6], false);
            smooth_box(w, random, s, [4, 1, 5, 4, 4, 9], false);
            smooth_box(w, random, s, [8, 1, 5, 8, 4, 9], false);
            smooth_box(w, random, s, [1, 4, 7, 3, 4, 9], false);
            smooth_box(w, random, s, [1, 3, 5, 3, 3, 6], false);
            let slab = s("minecraft:smooth_stone_slab");
            let double = s("minecraft:smooth_stone_slab[type=double]");
            w.generate_box(1, 3, 4, 3, 3, 4, &slab, &slab, false);
            w.generate_box(1, 4, 6, 3, 4, 6, &slab, &slab, false);
            smooth_box(w, random, s, [5, 1, 7, 7, 1, 8], false);
            w.generate_box(5, 1, 9, 7, 1, 9, &slab, &slab, false);
            w.generate_box(5, 2, 7, 7, 2, 7, &slab, &slab, false);
            w.generate_box(4, 5, 7, 4, 5, 9, &slab, &slab, false);
            w.generate_box(8, 5, 7, 8, 5, 9, &slab, &slab, false);
            w.generate_box(5, 5, 7, 7, 5, 9, &double, &double, false);
            w.place_block(&s("minecraft:wall_torch[facing=south]"), 6, 5, 6);
        }
        Kind::Library { tall } => library(w, random, s, p.door, tall),
        Kind::PortalRoom => portal_room(w, random, s, &mut p.placed_spawner),
        Kind::FillerCorridor { steps } => {
            for z in 0..steps {
                for x in 0..=4 {
                    w.place_block(&bricks, x, 0, z);
                }
                for y in 1..=3 {
                    w.place_block(&bricks, 0, y, z);
                    w.place_block(&air, 1, y, z);
                    w.place_block(&air, 2, y, z);
                    w.place_block(&air, 3, y, z);
                    w.place_block(&bricks, 4, y, z);
                }
                for x in 0..=4 {
                    w.place_block(&bricks, x, 4, z);
                }
            }
        }
    }
}

const fn kind_is_left(kind: Kind) -> bool {
    matches!(kind, Kind::LeftTurn)
}

/// `RoomCrossing.postProcess`'s switch on `type`: 0 a pillar with torches,
/// 1 a fountain, 2 a two-storey room with a chest; 3 and 4 nothing.
fn room_crossing_body<W: StructureWorld + ?Sized, R: RandomSource + ?Sized>(
    w: &mut Writer<'_, W>,
    random: &mut R,
    s: &dyn Fn(&str) -> String,
    kind: i32,
) {
    let bricks = s("minecraft:stone_bricks");
    match kind {
        0 => {
            w.place_block(&bricks, 5, 1, 5);
            w.place_block(&bricks, 5, 2, 5);
            w.place_block(&bricks, 5, 3, 5);
            w.place_block(&s("minecraft:wall_torch[facing=west]"), 4, 3, 5);
            w.place_block(&s("minecraft:wall_torch[facing=east]"), 6, 3, 5);
            w.place_block(&s("minecraft:wall_torch[facing=south]"), 5, 3, 4);
            w.place_block(&s("minecraft:wall_torch[facing=north]"), 5, 3, 6);
            let slab = s("minecraft:smooth_stone_slab");
            for (x, z) in [
                (4, 4),
                (4, 5),
                (4, 6),
                (6, 4),
                (6, 5),
                (6, 6),
                (5, 4),
                (5, 6),
            ] {
                w.place_block(&slab, x, 1, z);
            }
        }
        1 => {
            for i in 0..5 {
                w.place_block(&bricks, 3, 1, 3 + i);
                w.place_block(&bricks, 7, 1, 3 + i);
                w.place_block(&bricks, 3 + i, 1, 3);
                w.place_block(&bricks, 3 + i, 1, 7);
            }
            w.place_block(&bricks, 5, 1, 5);
            w.place_block(&bricks, 5, 2, 5);
            w.place_block(&bricks, 5, 3, 5);
            w.place_block(&s("minecraft:water"), 5, 4, 5);
        }
        2 => {
            let cobble = s("minecraft:cobblestone");
            for z in 1..=9 {
                w.place_block(&cobble, 1, 3, z);
                w.place_block(&cobble, 9, 3, z);
            }
            for x in 1..=9 {
                w.place_block(&cobble, x, 3, 1);
                w.place_block(&cobble, x, 3, 9);
            }
            for (x, y, z) in [
                (5, 1, 4),
                (5, 1, 6),
                (5, 3, 4),
                (5, 3, 6),
                (4, 1, 5),
                (6, 1, 5),
                (4, 3, 5),
                (6, 3, 5),
            ] {
                w.place_block(&cobble, x, y, z);
            }
            for y in 1..=3 {
                w.place_block(&cobble, 4, y, 4);
                w.place_block(&cobble, 6, y, 4);
                w.place_block(&cobble, 4, y, 6);
                w.place_block(&cobble, 6, y, 6);
            }
            w.place_block(&s("minecraft:wall_torch"), 5, 3, 5);
            let planks = s("minecraft:oak_planks");
            for z in 2..=8 {
                w.place_block(&planks, 2, 3, z);
                w.place_block(&planks, 3, 3, z);
                // Offsets 979..1077: the branch for `z <= 3 || z >= 7`
                // places 4..6 and FALLS THROUGH into the 7, 8 every row
                // places — the `z < 7` jump lands on the 7, not past the 8.
                if z <= 3 || z >= 7 {
                    for x in 4..=6 {
                        w.place_block(&planks, x, 3, z);
                    }
                }
                w.place_block(&planks, 7, 3, z);
                w.place_block(&planks, 8, 3, z);
            }
            let ladder = s("minecraft:ladder[facing=west]");
            w.place_block(&ladder, 9, 1, 3);
            w.place_block(&ladder, 9, 2, 3);
            w.place_block(&ladder, 9, 3, 3);
            create_chest(
                w,
                random,
                &s("minecraft:chest"),
                3,
                4,
                8,
                "minecraft:chests/stronghold_crossing",
            );
        }
        _ => {}
    }
}

/// `Library.postProcess`.
fn library<W: StructureWorld + ?Sized, R: RandomSource + ?Sized>(
    w: &mut Writer<'_, W>,
    random: &mut R,
    s: &dyn Fn(&str) -> String,
    door: Door,
    tall: bool,
) {
    let height = if tall { 11 } else { 6 };
    smooth_box(w, random, s, [0, 0, 0, 13, height - 1, 14], true);
    small_door(w, s, door, 4, 1, 0);
    let cobweb = s("minecraft:cobweb");
    w.generate_maybe_box(
        random, 0.07, 2, 1, 1, 11, 4, 13, &cobweb, &cobweb, false, false,
    );
    let planks = s("minecraft:oak_planks");
    let shelf = s("minecraft:bookshelf");
    for z in 1..=13 {
        if (z - 1) % 4 == 0 {
            w.generate_box(1, 1, z, 1, 4, z, &planks, &planks, false);
            w.generate_box(12, 1, z, 12, 4, z, &planks, &planks, false);
            w.place_block(&s("minecraft:wall_torch[facing=east]"), 2, 3, z);
            w.place_block(&s("minecraft:wall_torch[facing=west]"), 11, 3, z);
            if tall {
                w.generate_box(1, 6, z, 1, 9, z, &planks, &planks, false);
                w.generate_box(12, 6, z, 12, 9, z, &planks, &planks, false);
            }
        } else {
            w.generate_box(1, 1, z, 1, 4, z, &shelf, &shelf, false);
            w.generate_box(12, 1, z, 12, 4, z, &shelf, &shelf, false);
            if tall {
                w.generate_box(1, 6, z, 1, 9, z, &shelf, &shelf, false);
                w.generate_box(12, 6, z, 12, 9, z, &shelf, &shelf, false);
            }
        }
    }
    let mut z = 3;
    while z < 12 {
        w.generate_box(3, 1, z, 4, 3, z, &shelf, &shelf, false);
        w.generate_box(6, 1, z, 7, 3, z, &shelf, &shelf, false);
        w.generate_box(9, 1, z, 10, 3, z, &shelf, &shelf, false);
        z += 2;
    }
    if tall {
        w.generate_box(1, 5, 1, 3, 5, 13, &planks, &planks, false);
        w.generate_box(10, 5, 1, 12, 5, 13, &planks, &planks, false);
        w.generate_box(4, 5, 1, 9, 5, 2, &planks, &planks, false);
        w.generate_box(4, 5, 12, 9, 5, 13, &planks, &planks, false);
        w.place_block(&planks, 9, 5, 11);
        w.place_block(&planks, 8, 5, 11);
        w.place_block(&planks, 9, 5, 10);
        let ns = s("minecraft:oak_fence[north=true,south=true]");
        let we = s("minecraft:oak_fence[east=true,west=true]");
        w.generate_box(3, 6, 3, 3, 6, 11, &ns, &ns, false);
        w.generate_box(10, 6, 3, 10, 6, 9, &ns, &ns, false);
        w.generate_box(4, 6, 2, 9, 6, 2, &we, &we, false);
        w.generate_box(4, 6, 12, 7, 6, 12, &we, &we, false);
        w.place_block(&s("minecraft:oak_fence[east=true,north=true]"), 3, 6, 2);
        w.place_block(&s("minecraft:oak_fence[east=true,south=true]"), 3, 6, 12);
        w.place_block(&s("minecraft:oak_fence[north=true,west=true]"), 10, 6, 2);
        for i in 0..=2 {
            w.place_block(
                &s("minecraft:oak_fence[south=true,west=true]"),
                8 + i,
                6,
                12 - i,
            );
            if i != 2 {
                w.place_block(
                    &s("minecraft:oak_fence[east=true,north=true]"),
                    8 + i,
                    6,
                    11 - i,
                );
            }
        }
        let ladder = s("minecraft:ladder[facing=south]");
        for y in 1..=7 {
            w.place_block(&ladder, 10, y, 13);
        }
        let e = s("minecraft:oak_fence[east=true]");
        let wst = s("minecraft:oak_fence[west=true]");
        let all = s("minecraft:oak_fence[east=true,north=true,south=true,west=true]");
        w.place_block(&e, 6, 9, 7);
        w.place_block(&wst, 7, 9, 7);
        w.place_block(&e, 6, 8, 7);
        w.place_block(&wst, 7, 8, 7);
        w.place_block(&all, 6, 7, 7);
        w.place_block(&all, 7, 7, 7);
        w.place_block(&e, 5, 7, 7);
        w.place_block(&wst, 8, 7, 7);
        w.place_block(&s("minecraft:oak_fence[east=true,north=true]"), 6, 7, 6);
        w.place_block(&s("minecraft:oak_fence[east=true,south=true]"), 6, 7, 8);
        w.place_block(&s("minecraft:oak_fence[north=true,west=true]"), 7, 7, 6);
        w.place_block(&s("minecraft:oak_fence[south=true,west=true]"), 7, 7, 8);
        let torch = s("minecraft:torch");
        for (x, y, z) in [
            (5, 8, 7),
            (8, 8, 7),
            (6, 8, 6),
            (6, 8, 8),
            (7, 8, 6),
            (7, 8, 8),
        ] {
            w.place_block(&torch, x, y, z);
        }
    }
    create_chest(
        w,
        random,
        &s("minecraft:chest"),
        3,
        3,
        5,
        "minecraft:chests/stronghold_library",
    );
    if tall {
        w.place_block(&s("minecraft:cave_air"), 12, 9, 1);
        create_chest(
            w,
            random,
            &s("minecraft:chest"),
            12,
            8,
            1,
            "minecraft:chests/stronghold_library",
        );
    }
}

/// `PortalRoom.postProcess`.
fn portal_room<W: StructureWorld + ?Sized, R: RandomSource + ?Sized>(
    w: &mut Writer<'_, W>,
    random: &mut R,
    s: &dyn Fn(&str) -> String,
    placed_spawner: &mut bool,
) {
    smooth_box(w, random, s, [0, 0, 0, 10, 7, 15], false);
    small_door(w, s, Door::Grates, 4, 1, 0);
    smooth_box(w, random, s, [1, 6, 1, 1, 6, 14], false);
    smooth_box(w, random, s, [9, 6, 1, 9, 6, 14], false);
    smooth_box(w, random, s, [2, 6, 1, 8, 6, 2], false);
    smooth_box(w, random, s, [2, 6, 14, 8, 6, 14], false);
    smooth_box(w, random, s, [1, 1, 1, 2, 1, 4], false);
    smooth_box(w, random, s, [8, 1, 1, 9, 1, 4], false);
    let lava = s("minecraft:lava");
    w.generate_box(1, 1, 1, 1, 1, 3, &lava, &lava, false);
    w.generate_box(9, 1, 1, 9, 1, 3, &lava, &lava, false);
    smooth_box(w, random, s, [3, 1, 8, 7, 1, 12], false);
    w.generate_box(4, 1, 9, 6, 1, 11, &lava, &lava, false);
    let ns = s("minecraft:iron_bars[north=true,south=true]");
    let we = s("minecraft:iron_bars[east=true,west=true]");
    let mut z = 3;
    while z < 14 {
        w.generate_box(0, 3, z, 0, 4, z, &ns, &ns, false);
        w.generate_box(10, 3, z, 10, 4, z, &ns, &ns, false);
        z += 2;
    }
    let mut x = 2;
    while x < 9 {
        w.generate_box(x, 3, 15, x, 4, 15, &we, &we, false);
        x += 2;
    }
    smooth_box(w, random, s, [4, 1, 5, 6, 1, 7], false);
    smooth_box(w, random, s, [4, 2, 6, 6, 2, 7], false);
    smooth_box(w, random, s, [4, 3, 7, 6, 3, 7], false);
    let stairs = s("minecraft:stone_brick_stairs[facing=north]");
    for x in 4..=6 {
        w.place_block(&stairs, x, 1, 4);
        w.place_block(&stairs, x, 2, 5);
        w.place_block(&stairs, x, 3, 6);
    }
    let mut eyes = [false; 12];
    let mut all = true;
    for eye in &mut eyes {
        *eye = random.next_f32() > 0.9;
        all &= *eye;
    }
    let frames: [(&str, (i32, i32, i32)); 12] = [
        ("north", (4, 3, 8)),
        ("north", (5, 3, 8)),
        ("north", (6, 3, 8)),
        ("south", (4, 3, 12)),
        ("south", (5, 3, 12)),
        ("south", (6, 3, 12)),
        ("east", (3, 3, 9)),
        ("east", (3, 3, 10)),
        ("east", (3, 3, 11)),
        ("west", (7, 3, 9)),
        ("west", (7, 3, 10)),
        ("west", (7, 3, 11)),
    ];
    for (i, (facing, (x, y, z))) in frames.iter().enumerate() {
        let state = s(&format!(
            "minecraft:end_portal_frame[eye={},facing={facing}]",
            eyes[i]
        ));
        w.place_block(&state, *x, *y, *z);
    }
    if all {
        let portal = s("minecraft:end_portal");
        for z in 9..=11 {
            for x in 4..=6 {
                w.place_block(&portal, x, 3, z);
            }
        }
    }
    if !*placed_spawner && w.inside_box(5, 3, 6) {
        *placed_spawner = true;
        let (wx, wy, wz) = w.piece.world_pos(5, 3, 6);
        w.world.set_block(wx, wy, wz, &s("minecraft:spawner"));
        // `spawner.setEntityId(EntityTypes.SILVERFISH, random)`: the type
        // goes into `SpawnData.entity.id`, and `getOrCreateNextSpawnData`
        // draws nothing from an unbuilt `spawnPotentials` (see
        // `feature::monster_room`). The seven constants are `complete`'s.
        w.world.record_block_entity(
            wx,
            wy,
            wz,
            crate::block_entity::Record {
                nbt: std::collections::BTreeMap::from([(
                    "SpawnData".to_string(),
                    rustmc_world::nbt::Nbt::Compound(std::collections::BTreeMap::from([(
                        "entity".to_string(),
                        rustmc_world::nbt::Nbt::Compound(std::collections::BTreeMap::from([(
                            "id".to_string(),
                            rustmc_world::nbt::Nbt::String("minecraft:silverfish".into()),
                        )])),
                    )])),
                )]),
                loot_seed: None,
            },
        );
    }
}
