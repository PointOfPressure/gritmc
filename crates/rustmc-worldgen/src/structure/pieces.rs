//! Structure pieces: the boxes, and the builder that collects them.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `BoundingBox`, `StructurePiece`,
//! `net.minecraft.world.level.levelgen.structure.pieces.StructurePiecesBuilder`.
//!
//! Every structure in the game is a list of these. A piece is a box, a
//! facing, and a recursion depth; a structure's *layout* is which pieces
//! exist and where, decided before a single block is written. Vanilla saves
//! exactly that much into each chunk's NBT, which is what makes the layout
//! independently checkable — see `golden_structure_starts`.
//!
//! # Boxes are inclusive on both ends
//!
//! `BoundingBox(minX, minY, minZ, maxX, maxY, maxZ)` contains both corners,
//! so a 5-block corridor runs `min..=min+4` and its span is
//! `max - min + 1`. Reading them as half-open makes every piece one block
//! short in all three axes and every collision test slightly too permissive
//! — which shows up as structures that *nearly* fit together.
//!
//! # Collision is what makes the layout finite
//!
//! Pieces are added one at a time, and a candidate box that intersects any
//! already-added piece is rejected. So the layout depends on **insertion
//! order**, and insertion order is the recursion order — which is why the
//! order this port adds pieces in is behaviour rather than bookkeeping.

/// `net.minecraft.world.level.levelgen.structure.BoundingBox` — inclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundingBox {
    pub min_x: i32,
    pub min_y: i32,
    pub min_z: i32,
    pub max_x: i32,
    pub max_y: i32,
    pub max_z: i32,
}

impl BoundingBox {
    #[must_use]
    pub const fn new(
        min_x: i32,
        min_y: i32,
        min_z: i32,
        max_x: i32,
        max_y: i32,
        max_z: i32,
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

    /// `move` — vanilla mutates in place and callers discard the return, so
    /// this is `&mut self` rather than a builder method that could be
    /// forgotten at a call site.
    pub const fn shift(&mut self, dx: i32, dy: i32, dz: i32) {
        self.min_x += dx;
        self.min_y += dy;
        self.min_z += dz;
        self.max_x += dx;
        self.max_y += dy;
        self.max_z += dz;
    }

    /// `getYSpan` — inclusive, so a one-block-tall box spans 1.
    #[must_use]
    pub const fn y_span(self) -> i32 {
        self.max_y - self.min_y + 1
    }

    /// `intersects` — inclusive on every face, so boxes that merely touch
    /// **do** collide. Vanilla relies on that: corridors are placed one
    /// block outside their parent so that touching is the normal case and
    /// overlapping is the rejected one.
    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.max_x >= other.min_x
            && self.min_x <= other.max_x
            && self.max_z >= other.min_z
            && self.min_z <= other.max_z
            && self.max_y >= other.min_y
            && self.min_y <= other.max_y
    }

    /// `getCenter` — integer division of the span, so it rounds towards the
    /// low corner for even spans.
    ///
    /// **Not** `(min + max) / 2`, which is what `isInInvalidLocation` uses
    /// and is a different number on an even span: for `0..=3` this gives 2
    /// and that gives 1. Both appear in the mineshaft path — this centre is
    /// where a mesa mine samples its surface height, the midpoint is where a
    /// piece asks whether its biome blocks mineshafts — so they stay two
    /// expressions rather than one helper that is right half the time.
    #[must_use]
    pub const fn center(self) -> (i32, i32, i32) {
        (
            self.min_x + (self.max_x - self.min_x + 1) / 2,
            self.min_y + (self.max_y - self.min_y + 1) / 2,
            self.min_z + (self.max_z - self.min_z + 1) / 2,
        )
    }

    /// `StructurePiece.createBoundingBox` — the union, or `None` for an
    /// empty list. Vanilla throws on empty; a caller here has to say what it
    /// wants instead, because "no pieces" is a real state during generation.
    #[must_use]
    pub fn encapsulating(boxes: &[Self]) -> Option<Self> {
        let mut it = boxes.iter().copied();
        let first = it.next()?;
        Some(it.fold(first, |a, b| Self {
            min_x: a.min_x.min(b.min_x),
            min_y: a.min_y.min(b.min_y),
            min_z: a.min_z.min(b.min_z),
            max_x: a.max_x.max(b.max_x),
            max_y: a.max_y.max(b.max_y),
            max_z: a.max_z.max(b.max_z),
        }))
    }

    /// `BoundingBox.isInside(int, int, int)` — inclusive on every face, like
    /// every other predicate on this type.
    #[must_use]
    pub const fn is_inside(self, x: i32, y: i32, z: i32) -> bool {
        x >= self.min_x
            && x <= self.max_x
            && y >= self.min_y
            && y <= self.max_y
            && z >= self.min_z
            && z <= self.max_z
    }

    /// `BoundingBox.inflatedBy(int)` — grow by `n` on all six faces.
    ///
    /// `Beardifier.forStructuresInChunk` calls this with **24**, not with the
    /// kernel radius of 12. The difference is inert — every contribution is
    /// already zero beyond 12, because the kernel guard refuses and the bury
    /// map has run out at a length of 6 — but 24 is the number in the
    /// bytecode at offset 413 and using 12 would be a port that happens to
    /// agree rather than one that matches.
    #[must_use]
    pub const fn inflated_by(self, n: i32) -> Self {
        Self {
            min_x: self.min_x - n,
            min_y: self.min_y - n,
            min_z: self.min_z - n,
            max_x: self.max_x + n,
            max_y: self.max_y + n,
            max_z: self.max_z + n,
        }
    }
}

/// `Direction`, as the four horizontals a structure piece can face.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Facing {
    North,
    South,
    West,
    East,
}

impl Facing {
    /// `Direction.get2DDataValue` — what a structure piece saves as `O`.
    ///
    /// **Not the `values()` ordinal.** `data2d` is a separate field on the
    /// enum, numbering only the horizontals and in a different order:
    /// SOUTH 0, WEST 1, NORTH 2, EAST 3, against ordinals 3, 4, 2, 5. The
    /// two agree on nothing except NORTH, which is exactly enough for a
    /// wrong port to look right in a spot check. The layout gate caught it
    /// on real data.
    #[must_use]
    pub const fn saved_value(self) -> i32 {
        match self {
            Self::South => 0,
            Self::West => 1,
            Self::North => 2,
            Self::East => 3,
        }
    }

    /// From a saved `O`, or `None` for `-1` — which is what a piece that
    /// never called `setOrientation` writes.
    #[must_use]
    pub const fn from_saved_value(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::South),
            1 => Some(Self::West),
            2 => Some(Self::North),
            3 => Some(Self::East),
            _ => None,
        }
    }

    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::North => Self::South,
            Self::South => Self::North,
            Self::West => Self::East,
            Self::East => Self::West,
        }
    }

    /// `Direction.Plane.HORIZONTAL.getRandomDirection` order — the order
    /// `values()` lists them, which is what an index into it means.
    #[must_use]
    pub const fn all() -> [Self; 4] {
        [Self::North, Self::South, Self::West, Self::East]
    }

    /// `StructurePiece.setOrientation` — the mirror and rotation a piece
    /// applies to **every state it places**.
    ///
    /// This is the part of a piece's frame that is not about coordinates. A
    /// corridor describes its fence as connecting west; facing west, the
    /// piece mirrors and rotates that into a fence connecting south. Skip it
    /// and every directional block in the structure points the wrong way
    /// while sitting in exactly the right place — which is what a
    /// block-state diff catches and a bounding-box diff never would.
    #[must_use]
    pub const fn frame(self) -> (Mirror, Rotation) {
        match self {
            Self::North => (Mirror::None, Rotation::None),
            Self::South => (Mirror::LeftRight, Rotation::None),
            Self::West => (Mirror::LeftRight, Rotation::Clockwise90),
            Self::East => (Mirror::None, Rotation::Clockwise90),
        }
    }
}

/// `Mirror`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mirror {
    None,
    /// Mirrors the **Z** axis: north and south swap, east and west do not.
    LeftRight,
    /// Mirrors the X axis.
    FrontBack,
}

/// `Rotation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rotation {
    None,
    Clockwise90,
    Clockwise180,
    CounterClockwise90,
}

impl Rotation {
    /// Where a horizontal direction ends up.
    #[must_use]
    pub const fn apply(self, facing: Facing) -> Facing {
        match self {
            Self::None => facing,
            Self::Clockwise90 => match facing {
                Facing::North => Facing::East,
                Facing::East => Facing::South,
                Facing::South => Facing::West,
                Facing::West => Facing::North,
            },
            Self::Clockwise180 => facing.opposite(),
            Self::CounterClockwise90 => match facing {
                Facing::North => Facing::West,
                Facing::West => Facing::South,
                Facing::South => Facing::East,
                Facing::East => Facing::North,
            },
        }
    }
}

impl Mirror {
    /// Where a horizontal direction ends up.
    #[must_use]
    pub const fn apply(self, facing: Facing) -> Facing {
        match self {
            Self::None => facing,
            Self::LeftRight => match facing {
                Facing::North => Facing::South,
                Facing::South => Facing::North,
                other => other,
            },
            Self::FrontBack => match facing {
                Facing::East => Facing::West,
                Facing::West => Facing::East,
                other => other,
            },
        }
    }
}

/// A placed piece, as far as the layout is concerned.
///
/// No blocks: this is what vanilla saves and what the layout gate compares.
/// Writing the blocks is a separate stage with a separate oracle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Piece {
    /// The `id` vanilla saves, lowercased as it appears in the NBT —
    /// `minecraft:mscorridor` and so on.
    pub id: &'static str,
    pub bounding_box: BoundingBox,
    /// `O`, or `None` for a piece that records no orientation.
    ///
    /// Two mineshaft pieces have none: the room, which has no facing at all,
    /// and the **crossing**, which keeps its direction in a field of its own
    /// and never calls `setOrientation`. So a crossing knows which way it
    /// faces while it generates its children and does not save it.
    pub facing: Option<Facing>,
    /// `GD` — the recursion depth. Load-bearing: the walk stops past 8, so
    /// a port that counts depth differently builds a different-sized mine.
    pub depth: i32,
}

impl Piece {
    /// `StructurePiece.getWorldX` — a piece's local x, in world coordinates.
    ///
    /// Local coordinates are *rotated* by the facing, so both `x` and `z` are
    /// needed to resolve either axis. A piece writes its blocks in its own
    /// frame — "three along, two up, one in" — and the same code produces a
    /// corridor running north or east depending only on this.
    ///
    /// A piece with **no facing passes coordinates straight through**, and
    /// that is not a fallback: the mineshaft crossing deliberately keeps no
    /// orientation, so its `postProcess` works in absolute coordinates while
    /// every other piece works in local ones. Treating a missing facing as
    /// "north" would put every crossing's contents somewhere else.
    #[must_use]
    pub const fn world_x(&self, x: i32, z: i32) -> i32 {
        let bb = self.bounding_box;
        match self.facing {
            None => x,
            Some(Facing::North | Facing::South) => bb.min_x + x,
            Some(Facing::West) => bb.max_x - z,
            Some(Facing::East) => bb.min_x + z,
        }
    }

    /// `StructurePiece.getWorldY` — local y is only offset, never rotated.
    #[must_use]
    pub const fn world_y(&self, y: i32) -> i32 {
        match self.facing {
            None => y,
            Some(_) => y + self.bounding_box.min_y,
        }
    }

    /// `StructurePiece.getWorldZ`. See [`Self::world_x`].
    #[must_use]
    pub const fn world_z(&self, x: i32, z: i32) -> i32 {
        let bb = self.bounding_box;
        match self.facing {
            None => z,
            Some(Facing::North) => bb.max_z - z,
            Some(Facing::South) => bb.min_z + z,
            Some(Facing::West | Facing::East) => bb.min_z + x,
        }
    }

    /// `getWorldPos` — the three together.
    #[must_use]
    pub const fn world_pos(&self, x: i32, y: i32, z: i32) -> (i32, i32, i32) {
        (self.world_x(x, z), self.world_y(y), self.world_z(x, z))
    }
}

/// `StructurePiecesBuilder`.
///
/// A plain `Vec` and not a spatial index on purpose. The collision test is
/// a linear scan in vanilla too, and — more importantly — it scans in
/// **insertion order** and returns the *first* hit. Any structure that
/// changed the scan order would still reject the same candidates, but a
/// future caller that cares *which* piece it collided with would silently
/// get a different one.
#[derive(Debug, Default)]
pub struct PiecesBuilder {
    pieces: Vec<Piece>,
}

impl PiecesBuilder {
    #[must_use]
    pub const fn new() -> Self {
        Self { pieces: Vec::new() }
    }

    pub fn add(&mut self, piece: Piece) {
        self.pieces.push(piece);
    }

    #[must_use]
    pub fn pieces(&self) -> &[Piece] {
        &self.pieces
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.pieces.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pieces.is_empty()
    }

    /// `findCollisionPiece` — the first already-added piece the candidate
    /// intersects.
    #[must_use]
    pub fn collides(&self, candidate: BoundingBox) -> bool {
        self.pieces
            .iter()
            .any(|p| p.bounding_box.intersects(candidate))
    }

    /// `getBoundingBox` — the union of every piece.
    #[must_use]
    pub fn bounding_box(&self) -> Option<BoundingBox> {
        let boxes: Vec<BoundingBox> = self.pieces.iter().map(|p| p.bounding_box).collect();
        BoundingBox::encapsulating(&boxes)
    }

    /// `offsetPiecesVertically`.
    pub fn offset_vertically(&mut self, dy: i32) {
        for piece in &mut self.pieces {
            piece.bounding_box.shift(0, dy, 0);
        }
    }

    /// `moveBelowSeaLevel` — drop the whole structure to a random depth
    /// under the sea, and return how far it moved.
    ///
    /// The draw is **conditional**: a structure already tall enough to reach
    /// the ceiling does not draw at all. That is the trap here — a port that
    /// always drew would stay plausible (mines would still be underground)
    /// and would desynchronise every structure generated afterwards in the
    /// same chunk.
    pub fn move_below_sea_level<R: crate::rng::RandomSource + ?Sized>(
        &mut self,
        sea_level: i32,
        min_y: i32,
        random: &mut R,
        margin: i32,
    ) -> i32 {
        let ceiling = sea_level - margin;
        let Some(bounds) = self.bounding_box() else {
            return 0;
        };
        let mut top = bounds.y_span() + min_y + 1;
        if top < ceiling {
            top += random.next_i32_bound(ceiling - top);
        }
        let dy = top - bounds.max_y;
        self.offset_vertically(dy);
        dy
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::legacy::LegacyRandomSource;
    use crate::rng::worldgen_random::WorldgenRandom;
    use crate::rng::RandomSource;

    fn piece(bb: BoundingBox) -> Piece {
        Piece {
            id: "test",
            bounding_box: bb,
            facing: None,
            depth: 0,
        }
    }

    /// Both corners are inside, so the span is one more than the difference.
    #[test]
    fn a_box_is_inclusive_at_both_ends() {
        let bb = BoundingBox::new(0, 0, 0, 0, 4, 0);
        assert_eq!(bb.y_span(), 5, "a box from 0 to 4 is five blocks tall");
        assert_eq!(BoundingBox::new(0, 7, 0, 0, 7, 0).y_span(), 1);
    }

    /// Touching counts as intersecting, which is what stops two corridors
    /// being placed flush against each other.
    #[test]
    fn boxes_that_only_touch_still_collide() {
        let a = BoundingBox::new(0, 0, 0, 2, 2, 2);
        let flush = BoundingBox::new(2, 0, 0, 4, 2, 2);
        let clear = BoundingBox::new(3, 0, 0, 5, 2, 2);
        assert!(a.intersects(flush), "sharing a face is a collision");
        assert!(!a.intersects(clear));
        // ...and it is symmetric, which a hand-written comparison chain is
        // an easy place to get wrong on one axis.
        assert_eq!(a.intersects(flush), flush.intersects(a));
        assert_eq!(a.intersects(clear), clear.intersects(a));
    }

    /// A collision on **any** axis is a collision; disagreeing on one axis
    /// is not.
    #[test]
    fn a_box_stacked_above_another_does_not_collide() {
        let a = BoundingBox::new(0, 0, 0, 2, 2, 2);
        assert!(!a.intersects(BoundingBox::new(0, 3, 0, 2, 5, 2)), "above");
        assert!(
            !a.intersects(BoundingBox::new(0, 0, 3, 2, 2, 5)),
            "beyond z"
        );
        assert!(
            a.intersects(BoundingBox::new(1, 1, 1, 1, 1, 1)),
            "contained"
        );
    }

    #[test]
    fn the_union_covers_every_box_and_an_empty_list_has_none() {
        assert_eq!(BoundingBox::encapsulating(&[]), None);
        let union = BoundingBox::encapsulating(&[
            BoundingBox::new(0, 0, 0, 1, 1, 1),
            BoundingBox::new(-5, 3, 2, -4, 9, 2),
        ])
        .expect("two boxes");
        assert_eq!(union, BoundingBox::new(-5, 0, 0, 1, 9, 2));
    }

    /// `getCenter` rounds towards the high corner for an even span, because
    /// it adds the span *then* halves it.
    #[test]
    fn the_centre_is_vanillas_and_not_a_midpoint() {
        assert_eq!(BoundingBox::new(0, 0, 0, 3, 3, 3).center(), (2, 2, 2));
        assert_eq!(BoundingBox::new(0, 0, 0, 4, 4, 4).center(), (2, 2, 2));
    }

    /// The saved `O` is `get2DDataValue`, not the `values()` ordinal. They
    /// agree on NORTH and on nothing else, which is why a spot check on one
    /// direction is not evidence.
    #[test]
    fn a_facing_round_trips_through_the_saved_value() {
        for facing in Facing::all() {
            assert_eq!(Facing::from_saved_value(facing.saved_value()), Some(facing));
            assert_eq!(facing.opposite().opposite(), facing);
        }
        assert_eq!(Facing::South.saved_value(), 0);
        assert_eq!(Facing::West.saved_value(), 1);
        assert_eq!(
            Facing::North.saved_value(),
            2,
            "the one the ordinal agrees on"
        );
        assert_eq!(Facing::East.saved_value(), 3);
        assert_eq!(
            Facing::from_saved_value(-1),
            None,
            "no orientation recorded"
        );
        assert_eq!(Facing::from_saved_value(4), None);
    }

    /// The four rotations, checked at a corner that distinguishes them.
    ///
    /// Local (0,0,0) is the piece's "near left" and lands on a different
    /// world corner per facing — so a port that used one formula for all
    /// four would place three quarters of a mine inside out.
    #[test]
    fn local_coordinates_rotate_with_the_facing() {
        let bb = BoundingBox::new(10, 64, 20, 14, 66, 24);
        let of = |facing| Piece {
            id: "test",
            bounding_box: bb,
            facing: Some(facing),
            depth: 0,
        };
        // North runs towards -Z, so local z counts down from the far edge.
        assert_eq!(of(Facing::North).world_pos(0, 0, 0), (10, 64, 24));
        assert_eq!(of(Facing::North).world_pos(1, 2, 3), (11, 66, 21));
        // South counts up from the near edge.
        assert_eq!(of(Facing::South).world_pos(0, 0, 0), (10, 64, 20));
        assert_eq!(of(Facing::South).world_pos(1, 2, 3), (11, 66, 23));
        // West and East swap the axes: local x becomes world z.
        assert_eq!(of(Facing::West).world_pos(0, 0, 0), (14, 64, 20));
        assert_eq!(of(Facing::West).world_pos(1, 2, 3), (11, 66, 21));
        assert_eq!(of(Facing::East).world_pos(0, 0, 0), (10, 64, 20));
        assert_eq!(of(Facing::East).world_pos(1, 2, 3), (13, 66, 21));
    }

    /// A piece with no facing is in world coordinates already — and `y` is
    /// not offset either, which is the part that looks like an oversight and
    /// is not.
    #[test]
    fn a_piece_without_a_facing_passes_coordinates_through() {
        let unoriented = Piece {
            id: "test",
            bounding_box: BoundingBox::new(10, 64, 20, 14, 66, 24),
            facing: None,
            depth: 0,
        };
        assert_eq!(unoriented.world_pos(1, 2, 3), (1, 2, 3));
        assert_eq!(unoriented.world_y(0), 0, "y is not offset either");
    }

    #[test]
    fn a_candidate_collides_with_any_added_piece() {
        let mut builder = PiecesBuilder::new();
        assert!(!builder.collides(BoundingBox::new(0, 0, 0, 1, 1, 1)));
        builder.add(piece(BoundingBox::new(0, 0, 0, 4, 4, 4)));
        builder.add(piece(BoundingBox::new(10, 0, 0, 14, 4, 4)));
        assert!(
            builder.collides(BoundingBox::new(4, 4, 4, 6, 6, 6)),
            "touches the first"
        );
        assert!(
            builder.collides(BoundingBox::new(12, 0, 0, 13, 1, 1)),
            "inside the second"
        );
        assert!(
            !builder.collides(BoundingBox::new(6, 0, 0, 8, 1, 1)),
            "between them"
        );
    }

    #[test]
    fn offsetting_moves_every_piece_and_the_union_with_them() {
        let mut builder = PiecesBuilder::new();
        builder.add(piece(BoundingBox::new(0, 10, 0, 1, 12, 1)));
        builder.add(piece(BoundingBox::new(5, 20, 5, 6, 21, 6)));
        builder.offset_vertically(-30);
        assert_eq!(
            builder.bounding_box(),
            Some(BoundingBox::new(0, -20, 0, 6, -9, 6))
        );
        for p in builder.pieces() {
            assert!(p.bounding_box.min_y < 0);
        }
    }

    /// The draw is conditional. A structure that already reaches the ceiling
    /// is moved without consulting the stream at all.
    #[test]
    fn a_structure_that_reaches_the_ceiling_does_not_draw() {
        // min_y -64, span big enough that -64 + span + 1 >= 63 - 10.
        let mut tall = PiecesBuilder::new();
        tall.add(piece(BoundingBox::new(0, 0, 0, 1, 120, 1)));
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(5));
        let mut probe = WorldgenRandom::new(LegacyRandomSource::new(5));
        tall.move_below_sea_level(63, -64, &mut random, 10);
        assert_eq!(
            random.next_i64(),
            probe.next_i64(),
            "a structure taller than the space above it must not draw"
        );
    }

    /// ...and a short one does draw, exactly once.
    #[test]
    fn a_short_structure_draws_once_and_lands_under_the_sea() {
        let mut short = PiecesBuilder::new();
        short.add(piece(BoundingBox::new(0, 0, 0, 1, 4, 1)));
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(5));
        let dy = short.move_below_sea_level(63, -64, &mut random, 10);

        let mut probe = WorldgenRandom::new(LegacyRandomSource::new(5));
        let ceiling = 63 - 10;
        let top = 5 + -64 + 1;
        probe.next_i32_bound(ceiling - top);
        assert_eq!(random.next_i64(), probe.next_i64(), "exactly one draw");

        let bounds = short.bounding_box().expect("a piece");
        assert!(
            bounds.max_y < 63,
            "the structure should end up below sea level, not at {}",
            bounds.max_y
        );
        assert_eq!(dy, bounds.max_y - 4, "the offset is what it moved by");
    }

    /// An empty builder has no box and moves nothing — vanilla would throw
    /// here, and a caller in the middle of generation should not.
    #[test]
    fn an_empty_builder_is_a_no_op_rather_than_a_panic() {
        let mut empty = PiecesBuilder::new();
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(1));
        assert_eq!(empty.bounding_box(), None);
        assert_eq!(empty.move_below_sea_level(63, -64, &mut random, 10), 0);
        assert!(empty.is_empty());
    }
}
