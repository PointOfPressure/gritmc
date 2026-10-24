//! Mineshafts — the layout, not the blocks.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `MineshaftStructure`, `MineshaftPieces` and its four piece
//! classes, `StructurePiecesBuilder`.
//!
//! One mineshaft is ~45% of everything still wrong in the whole-world diff
//! (BENCH.md), so this is the single biggest item left. It is also the one
//! structure with a free oracle: vanilla saves every piece's box, facing and
//! recursion depth into the chunk NBT, so a generated region proves the
//! layout without a JVM harness — `golden_structure_starts` reads 659 real
//! mineshaft pieces out of the world the parity diff already uses.
//!
//! **Blocks are a separate stage.** This decides which pieces exist and
//! where; what gets written inside them is `postProcess`, with a different
//! oracle (the region's blocks) and its own gate. Splitting them is what
//! makes this half checkable on its own.
//!
//! # The shape
//!
//! ```text
//! random.nextDouble()                     // drawn and DISCARDED
//! room = MineShaftRoom(depth 0, ...)      // 3 draws for its size
//! builder.add(room)
//! room.add_children(parent = room)        // the whole recursion
//! builder.move_below_sea_level(...)       // 0 or 1 draw
//! ```
//!
//! # Four things that are easy to write plausibly and get wrong
//!
//! **The `nextDouble` at the top is discarded.** `findGenerationPoint` draws
//! one and never uses it. Skipping it shifts every value in the mineshaft.
//!
//! **The ±80 bound is measured from the *room*, always.** `generateAndAddPiece`
//! takes a `parent` and checks the candidate against `parent`'s box — and
//! then calls `piece.addChildren(parent, ..)`, passing the *same* parent on
//! rather than the piece it just made. So the bound never moves as the
//! recursion descends: it is the mine's radius about its room, not a limit
//! on how far each step may travel. Passing the new piece instead grows
//! mines without limit and still looks like a mine.
//!
//! **A collision retry is not a redraw.** `findCorridorSize` draws its length
//! once, then shortens by one and *retries the same value* until it fits or
//! runs out. Redrawing per attempt is the natural way to write it and
//! consumes a variable number of extra values.
//!
//! **`&&` short-circuits into the stream.** A corridor's `spiderCorridor` is
//! `!hasRails && random.nextInt(23) == 0`, so one corridor in three never
//! draws the 23. Likewise `findStairs` draws *nothing at all*, where the
//! corridor and crossing finders each draw once — so which piece kind the
//! `nextInt(100)` picks changes how many values the next piece sees.

// Read once, not per corridor and not per section of one. See
// `crate::trace_switch`.
crate::trace_switch!(trace_mineshaft, "RUSTMC_TRACE_MINESHAFT");
crate::trace_switch!(trace_mineshaft_phases, "RUSTMC_TRACE_MINESHAFT_PHASES");
use super::pieces::{BoundingBox, Facing, Piece, PiecesBuilder};
use super::writer::{StructureWorld, Writer};
use crate::rng::RandomSource;

/// `MineshaftStructure.Type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MineshaftType {
    /// `minecraft:mineshaft`.
    Normal,
    /// `minecraft:mineshaft_mesa` — same layout, different vertical
    /// placement and different woods.
    Mesa,
}

/// The ids vanilla writes into the chunk NBT, which is what the layout gate
/// compares against.
const ID_ROOM: &str = "minecraft:msroom";
const ID_CORRIDOR: &str = "minecraft:mscorridor";
const ID_CROSSING: &str = "minecraft:mscrossing";
const ID_STAIRS: &str = "minecraft:msstairs";

/// `MineshaftPieces.MAX_DEPTH` — the walk stops *past* this, so a piece may
/// be created at depth 9.
const MAX_DEPTH: i32 = 8;

/// How far a piece may sit from the room, on either horizontal axis.
const MAX_RADIUS: i32 = 80;

/// A piece mid-generation: what it is, plus the little state its children
/// need.
///
/// No `Room`: the room is built by `generate` before the recursion starts
/// and is never a *result* of one, so giving it a variant here would be a
/// state the walk can never reach.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The corridor's constructor decides three things from the stream and
    /// the box, and `postProcess` needs all three — so the layout keeps
    /// them rather than recomputing, which would draw again.
    Corridor {
        has_rails: bool,
        spider: bool,
        sections: i32,
    },
    Crossing {
        two_floored: bool,
    },
    Stairs,
    /// The room, which is built before the recursion starts.
    Room,
}

/// The recursion's working state.
struct Generator<'a, R: RandomSource + ?Sized> {
    builder: &'a mut PiecesBuilder,
    random: &'a mut R,
    /// One entry per piece added, in the same order.
    kinds: &'a mut Vec<Kind>,
    /// Where the room's children broke through its walls. The room's
    /// `postProcess` hollows these out, so they are layout output rather
    /// than bookkeeping.
    entrances: &'a mut Vec<BoundingBox>,
}

impl<R: RandomSource + ?Sized> Generator<'_, R> {
    /// `MineshaftPieces.generateAndAddPiece`.
    ///
    /// `parent` is the box every candidate is measured against, and it does
    /// **not** change as the recursion descends — see the module docs.
    fn generate_and_add(
        &mut self,
        parent: BoundingBox,
        x: i32,
        y: i32,
        z: i32,
        facing: Facing,
        depth: i32,
    ) -> Option<BoundingBox> {
        if depth > MAX_DEPTH {
            return None;
        }
        if (x - parent.min_x).abs() > MAX_RADIUS || (z - parent.min_z).abs() > MAX_RADIUS {
            return None;
        }
        let (kind, bb) = self.create_random_piece(x, y, z, facing)?;
        let id = match kind {
            Kind::Corridor { .. } => ID_CORRIDOR,
            Kind::Crossing { .. } => ID_CROSSING,
            Kind::Stairs => ID_STAIRS,
            Kind::Room => ID_ROOM,
        };
        self.builder.add(Piece {
            id,
            bounding_box: bb,
            // A crossing keeps its direction in a field of its own and never
            // calls `setOrientation`, so it saves no `O` — while still using
            // the facing to decide where its children go.
            facing: match kind {
                Kind::Crossing { .. } => None,
                _ => Some(facing),
            },
            depth: depth + 1,
        });
        self.kinds.push(kind);
        self.add_children(kind, bb, facing, depth + 1, parent);
        Some(bb)
    }

    /// `MineshaftPieces.createRandomShaftPiece`.
    ///
    /// The roll comes first and is always spent; what follows differs in how
    /// many further values it takes — a crossing and a corridor draw once
    /// each, stairs not at all.
    fn create_random_piece(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        facing: Facing,
    ) -> Option<(Kind, BoundingBox)> {
        let roll = self.random.next_i32_bound(100);
        if roll >= 80 {
            let bb = self.find_crossing(x, y, z, facing)?;
            Some((
                Kind::Crossing {
                    // `isTwoFloored` is decided from the box, so it follows
                    // the height the finder drew rather than a second draw.
                    two_floored: bb.y_span() > 3,
                },
                bb,
            ))
        } else if roll >= 70 {
            let bb = self.find_stairs(x, y, z, facing)?;
            Some((Kind::Stairs, bb))
        } else {
            let bb = self.find_corridor(x, y, z, facing)?;
            // The corridor's constructor draws, and it draws *after* the
            // box is settled.
            let has_rails = self.random.next_i32_bound(3) == 0;
            let spider = !has_rails && self.random.next_i32_bound(23) == 0;
            // `numSections` is the box's length along the corridor's axis,
            // divided by five — the same five the layout stepped by.
            let sections = match facing {
                Facing::North | Facing::South => (bb.max_z - bb.min_z + 1) / 5,
                Facing::West | Facing::East => (bb.max_x - bb.min_x + 1) / 5,
            };
            Some((
                Kind::Corridor {
                    has_rails,
                    spider,
                    sections,
                },
                bb,
            ))
        }
    }

    /// `MineShaftCorridor.findCorridorSize` — one draw, then shorten and
    /// retry without drawing again.
    fn find_corridor(&mut self, x: i32, y: i32, z: i32, facing: Facing) -> Option<BoundingBox> {
        let mut sections = self.random.next_i32_bound(3) + 2;
        while sections > 0 {
            let length = sections * 5;
            let mut bb = match facing {
                Facing::North => BoundingBox::new(0, 0, -(length - 1), 2, 2, 0),
                Facing::South => BoundingBox::new(0, 0, 0, 2, 2, length - 1),
                Facing::West => BoundingBox::new(-(length - 1), 0, 0, 0, 2, 2),
                Facing::East => BoundingBox::new(0, 0, 0, length - 1, 2, 2),
            };
            bb.shift(x, y, z);
            if !self.builder.collides(bb) {
                return Some(bb);
            }
            sections -= 1;
        }
        None
    }

    /// `MineShaftCrossing.findCrossing` — one draw for the height, and no
    /// retry: a collision is a refusal.
    fn find_crossing(&mut self, x: i32, y: i32, z: i32, facing: Facing) -> Option<BoundingBox> {
        let height = if self.random.next_i32_bound(4) == 0 {
            6
        } else {
            2
        };
        let mut bb = match facing {
            Facing::North => BoundingBox::new(-1, 0, -4, 3, height, 0),
            Facing::South => BoundingBox::new(-1, 0, 0, 3, height, 4),
            Facing::West => BoundingBox::new(-4, 0, -1, 0, height, 3),
            Facing::East => BoundingBox::new(0, 0, -1, 4, height, 3),
        };
        bb.shift(x, y, z);
        (!self.builder.collides(bb)).then_some(bb)
    }

    /// `MineShaftStairs.findStairs` — **no draw**. It takes a `RandomSource`
    /// and never uses it, which is why the stairs branch of the roll leaves
    /// the stream in a different place from the other two.
    fn find_stairs(&mut self, x: i32, y: i32, z: i32, facing: Facing) -> Option<BoundingBox> {
        let mut bb = match facing {
            Facing::North => BoundingBox::new(0, -5, -8, 2, 2, 0),
            Facing::South => BoundingBox::new(0, -5, 0, 2, 2, 8),
            Facing::West => BoundingBox::new(-8, -5, 0, 0, 2, 2),
            Facing::East => BoundingBox::new(0, -5, 0, 8, 2, 2),
        };
        bb.shift(x, y, z);
        (!self.builder.collides(bb)).then_some(bb)
    }

    fn add_children(
        &mut self,
        kind: Kind,
        bb: BoundingBox,
        facing: Facing,
        depth: i32,
        parent: BoundingBox,
    ) {
        match kind {
            Kind::Corridor { .. } => self.corridor_children(bb, facing, depth, parent),
            Kind::Room => self.room_children(bb, depth, parent),
            Kind::Crossing { two_floored } => {
                self.crossing_children(bb, facing, two_floored, depth, parent);
            }
            Kind::Stairs => self.stairs_children(bb, facing, depth, parent),
        }
    }

    /// `MineShaftCorridor.addChildren`.
    ///
    /// One draw picks which way the corridor continues, then one more for
    /// the child's vertical offset — both spent whichever branch is taken.
    fn corridor_children(
        &mut self,
        bb: BoundingBox,
        facing: Facing,
        depth: i32,
        parent: BoundingBox,
    ) {
        let branch = self.random.next_i32_bound(4);
        let dy = |g: &mut Self| bb.min_y - 1 + g.random.next_i32_bound(3);

        match facing {
            Facing::North => {
                if branch <= 1 {
                    let y = dy(self);
                    let _ = self.generate_and_add(parent, bb.min_x, y, bb.min_z - 1, facing, depth);
                } else if branch == 2 {
                    let y = dy(self);
                    let _ = self.generate_and_add(
                        parent,
                        bb.min_x - 1,
                        y,
                        bb.min_z,
                        Facing::West,
                        depth,
                    );
                } else {
                    let y = dy(self);
                    let _ = self.generate_and_add(
                        parent,
                        bb.max_x + 1,
                        y,
                        bb.min_z,
                        Facing::East,
                        depth,
                    );
                }
            }
            Facing::South => {
                if branch <= 1 {
                    let y = dy(self);
                    let _ = self.generate_and_add(parent, bb.min_x, y, bb.max_z + 1, facing, depth);
                } else if branch == 2 {
                    let y = dy(self);
                    let _ = self.generate_and_add(
                        parent,
                        bb.min_x - 1,
                        y,
                        bb.max_z - 3,
                        Facing::West,
                        depth,
                    );
                } else {
                    let y = dy(self);
                    let _ = self.generate_and_add(
                        parent,
                        bb.max_x + 1,
                        y,
                        bb.max_z - 3,
                        Facing::East,
                        depth,
                    );
                }
            }
            Facing::West => {
                if branch <= 1 {
                    let y = dy(self);
                    let _ = self.generate_and_add(parent, bb.min_x - 1, y, bb.min_z, facing, depth);
                } else if branch == 2 {
                    let y = dy(self);
                    let _ = self.generate_and_add(
                        parent,
                        bb.min_x,
                        y,
                        bb.min_z - 1,
                        Facing::North,
                        depth,
                    );
                } else {
                    let y = dy(self);
                    let _ = self.generate_and_add(
                        parent,
                        bb.min_x,
                        y,
                        bb.max_z + 1,
                        Facing::South,
                        depth,
                    );
                }
            }
            Facing::East => {
                if branch <= 1 {
                    let y = dy(self);
                    let _ = self.generate_and_add(parent, bb.max_x + 1, y, bb.min_z, facing, depth);
                } else if branch == 2 {
                    let y = dy(self);
                    let _ = self.generate_and_add(
                        parent,
                        bb.max_x - 3,
                        y,
                        bb.min_z - 1,
                        Facing::North,
                        depth,
                    );
                } else {
                    let y = dy(self);
                    let _ = self.generate_and_add(
                        parent,
                        bb.max_x - 3,
                        y,
                        bb.max_z + 1,
                        Facing::South,
                        depth,
                    );
                }
            }
        }

        // Side passages, one candidate every five blocks. These pass
        // `depth + 1` where the continuation above passes `depth`, so a
        // branch counts as a step deeper and a straight run does not.
        if depth >= MAX_DEPTH {
            return;
        }
        match facing {
            Facing::North | Facing::South => {
                let mut z = bb.min_z + 3;
                while z + 3 <= bb.max_z {
                    let roll = self.random.next_i32_bound(5);
                    if roll == 0 {
                        let _ = self.generate_and_add(
                            parent,
                            bb.min_x - 1,
                            bb.min_y,
                            z,
                            Facing::West,
                            depth + 1,
                        );
                    } else if roll == 1 {
                        let _ = self.generate_and_add(
                            parent,
                            bb.max_x + 1,
                            bb.min_y,
                            z,
                            Facing::East,
                            depth + 1,
                        );
                    }
                    z += 5;
                }
            }
            Facing::West | Facing::East => {
                let mut x = bb.min_x + 3;
                while x + 3 <= bb.max_x {
                    let roll = self.random.next_i32_bound(5);
                    if roll == 0 {
                        let _ = self.generate_and_add(
                            parent,
                            x,
                            bb.min_y,
                            bb.min_z - 1,
                            Facing::North,
                            depth + 1,
                        );
                    } else if roll == 1 {
                        let _ = self.generate_and_add(
                            parent,
                            x,
                            bb.min_y,
                            bb.max_z + 1,
                            Facing::South,
                            depth + 1,
                        );
                    }
                    x += 5;
                }
            }
        }
    }

    /// `MineShaftCrossing.addChildren` — three ways out, plus four coins for
    /// the upper floor when there is one. Every coin is spent whether or not
    /// the piece it would make fits.
    fn crossing_children(
        &mut self,
        bb: BoundingBox,
        facing: Facing,
        two_floored: bool,
        depth: i32,
        parent: BoundingBox,
    ) {
        match facing {
            Facing::North => {
                self.generate_and_add(
                    parent,
                    bb.min_x + 1,
                    bb.min_y,
                    bb.min_z - 1,
                    Facing::North,
                    depth,
                );
                self.generate_and_add(
                    parent,
                    bb.min_x - 1,
                    bb.min_y,
                    bb.min_z + 1,
                    Facing::West,
                    depth,
                );
                self.generate_and_add(
                    parent,
                    bb.max_x + 1,
                    bb.min_y,
                    bb.min_z + 1,
                    Facing::East,
                    depth,
                );
            }
            Facing::South => {
                self.generate_and_add(
                    parent,
                    bb.min_x + 1,
                    bb.min_y,
                    bb.max_z + 1,
                    Facing::South,
                    depth,
                );
                self.generate_and_add(
                    parent,
                    bb.min_x - 1,
                    bb.min_y,
                    bb.min_z + 1,
                    Facing::West,
                    depth,
                );
                self.generate_and_add(
                    parent,
                    bb.max_x + 1,
                    bb.min_y,
                    bb.min_z + 1,
                    Facing::East,
                    depth,
                );
            }
            Facing::West => {
                self.generate_and_add(
                    parent,
                    bb.min_x + 1,
                    bb.min_y,
                    bb.min_z - 1,
                    Facing::North,
                    depth,
                );
                self.generate_and_add(
                    parent,
                    bb.min_x + 1,
                    bb.min_y,
                    bb.max_z + 1,
                    Facing::South,
                    depth,
                );
                self.generate_and_add(
                    parent,
                    bb.min_x - 1,
                    bb.min_y,
                    bb.min_z + 1,
                    Facing::West,
                    depth,
                );
            }
            Facing::East => {
                self.generate_and_add(
                    parent,
                    bb.min_x + 1,
                    bb.min_y,
                    bb.min_z - 1,
                    Facing::North,
                    depth,
                );
                self.generate_and_add(
                    parent,
                    bb.min_x + 1,
                    bb.min_y,
                    bb.max_z + 1,
                    Facing::South,
                    depth,
                );
                self.generate_and_add(
                    parent,
                    bb.max_x + 1,
                    bb.min_y,
                    bb.min_z + 1,
                    Facing::East,
                    depth,
                );
            }
        }

        if !two_floored {
            return;
        }
        // `minY + 3 + 1` — the upper floor sits four above the lower.
        let upper = bb.min_y + 3 + 1;
        if self.random.next_bool() {
            let _ = self.generate_and_add(
                parent,
                bb.min_x + 1,
                upper,
                bb.min_z - 1,
                Facing::North,
                depth,
            );
        }
        if self.random.next_bool() {
            let _ = self.generate_and_add(
                parent,
                bb.min_x - 1,
                upper,
                bb.min_z + 1,
                Facing::West,
                depth,
            );
        }
        if self.random.next_bool() {
            let _ = self.generate_and_add(
                parent,
                bb.max_x + 1,
                upper,
                bb.min_z + 1,
                Facing::East,
                depth,
            );
        }
        if self.random.next_bool() {
            let _ = self.generate_and_add(
                parent,
                bb.min_x + 1,
                upper,
                bb.max_z + 1,
                Facing::South,
                depth,
            );
        }
    }

    /// `MineShaftStairs.addChildren` — carry straight on, and draw nothing.
    fn stairs_children(
        &mut self,
        bb: BoundingBox,
        facing: Facing,
        depth: i32,
        parent: BoundingBox,
    ) {
        match facing {
            Facing::North => {
                let _ =
                    self.generate_and_add(parent, bb.min_x, bb.min_y, bb.min_z - 1, facing, depth);
            }
            Facing::South => {
                let _ =
                    self.generate_and_add(parent, bb.min_x, bb.min_y, bb.max_z + 1, facing, depth);
            }
            Facing::West => {
                let _ =
                    self.generate_and_add(parent, bb.min_x - 1, bb.min_y, bb.min_z, facing, depth);
            }
            Facing::East => {
                let _ =
                    self.generate_and_add(parent, bb.max_x + 1, bb.min_y, bb.min_z, facing, depth);
            }
        }
    }

    /// `MineShaftRoom.addChildren` — four walls, each walked in steps of
    /// four *plus a random jump*.
    ///
    /// The jump is drawn before the bound is tested, so the iteration that
    /// decides to stop has already spent a value. Testing first and drawing
    /// second is the natural way round and takes one fewer.
    fn room_children(&mut self, bb: BoundingBox, depth: i32, parent: BoundingBox) {
        let span = {
            let s = bb.y_span() - 3 - 1;
            if s <= 0 {
                1
            } else {
                s
            }
        };
        let x_span = bb.max_x - bb.min_x + 1;
        let z_span = bb.max_z - bb.min_z + 1;

        let mut i = 0;
        while i < x_span {
            i += self.random.next_i32_bound(x_span);
            if i + 3 > x_span {
                break;
            }
            let y = bb.min_y + self.random.next_i32_bound(span) + 1;
            if let Some(c) =
                self.generate_and_add(parent, bb.min_x + i, y, bb.min_z - 1, Facing::North, depth)
            {
                self.entrances.push(BoundingBox::new(
                    c.min_x,
                    c.min_y,
                    bb.min_z,
                    c.max_x,
                    c.max_y,
                    bb.min_z + 1,
                ));
            }
            i += 4;
        }

        let mut i = 0;
        while i < x_span {
            i += self.random.next_i32_bound(x_span);
            if i + 3 > x_span {
                break;
            }
            let y = bb.min_y + self.random.next_i32_bound(span) + 1;
            if let Some(c) =
                self.generate_and_add(parent, bb.min_x + i, y, bb.max_z + 1, Facing::South, depth)
            {
                self.entrances.push(BoundingBox::new(
                    c.min_x,
                    c.min_y,
                    bb.max_z - 1,
                    c.max_x,
                    c.max_y,
                    bb.max_z,
                ));
            }
            i += 4;
        }

        let mut i = 0;
        while i < z_span {
            i += self.random.next_i32_bound(z_span);
            if i + 3 > z_span {
                break;
            }
            let y = bb.min_y + self.random.next_i32_bound(span) + 1;
            if let Some(c) =
                self.generate_and_add(parent, bb.min_x - 1, y, bb.min_z + i, Facing::West, depth)
            {
                self.entrances.push(BoundingBox::new(
                    bb.min_x,
                    c.min_y,
                    c.min_z,
                    bb.min_x + 1,
                    c.max_y,
                    c.max_z,
                ));
            }
            i += 4;
        }

        let mut i = 0;
        while i < z_span {
            i += self.random.next_i32_bound(z_span);
            if i + 3 > z_span {
                break;
            }
            let y = bb.min_y + self.random.next_i32_bound(span) + 1;
            if let Some(c) =
                self.generate_and_add(parent, bb.max_x + 1, y, bb.min_z + i, Facing::East, depth)
            {
                self.entrances.push(BoundingBox::new(
                    bb.max_x - 1,
                    c.min_y,
                    c.min_z,
                    bb.max_x,
                    c.max_y,
                    c.max_z,
                ));
            }
            i += 4;
        }
    }
}

/// A laid-out mineshaft: the pieces, what each one is, where its room's
/// children broke through, and how far the whole thing was moved down.
#[derive(Clone, Debug)]
pub struct Layout {
    pub pieces: Vec<Piece>,
    /// One per piece, in the same order.
    pub kinds: Vec<Kind>,
    /// The room's entrances, already moved with the rest.
    pub entrances: Vec<BoundingBox>,
    /// What `moveBelowSeaLevel` returned.
    pub offset: i32,
}

/// This port cannot lay out that mineshaft, named.
#[derive(Debug, PartialEq, Eq)]
pub struct Unsupported(pub String);

/// `MineshaftStructure.findGenerationPoint`, layout only.
///
/// Returns the pieces and how far the whole thing was moved down.
///
/// # Errors
/// For a mesa mineshaft, whose vertical placement needs the chunk
/// generator's surface height rather than the sea level.
pub fn generate<R: RandomSource + ?Sized>(
    random: &mut R,
    chunk_x: i32,
    chunk_z: i32,
    kind: MineshaftType,
    sea_level: i32,
    min_y: i32,
) -> Result<(Vec<Piece>, i32), Unsupported> {
    generate_with_kinds(random, chunk_x, chunk_z, kind, sea_level, min_y)
        .map(|layout| (layout.pieces, layout.offset))
}

/// As [`generate`], and also the per-piece state `postProcess` needs.
///
/// # Errors
/// For a mesa mineshaft, as [`generate`].
pub fn generate_with_kinds<R: RandomSource + ?Sized>(
    random: &mut R,
    chunk_x: i32,
    chunk_z: i32,
    kind: MineshaftType,
    sea_level: i32,
    min_y: i32,
) -> Result<Layout, Unsupported> {
    // No surface source, so a mesa mine still refuses — by the same message
    // it always did, and now from one place.
    generate_with_kinds_using(random, chunk_x, chunk_z, kind, sea_level, min_y, |_, _| {
        None
    })
}

/// As [`generate_with_kinds`], with the surface height a mesa mine needs.
///
/// `base_height(x, z)` is `ChunkGenerator#getBaseHeight` for
/// `Heightmap.Types.WORLD_SURFACE_WG` — **one above** the highest non-air
/// block of the noise column, which is what `iterateNoiseColumn` returns
/// (`OptionalInt.of(y + 1)`). It answers `None` where the caller cannot
/// supply it, and that is refused rather than guessed: a mesa mine placed
/// against the wrong height is a whole mineshaft in the wrong place.
///
/// # Errors
/// For a mesa mineshaft whose `base_height` answers `None`.
pub fn generate_with_kinds_using<R, F>(
    random: &mut R,
    chunk_x: i32,
    chunk_z: i32,
    kind: MineshaftType,
    sea_level: i32,
    min_y: i32,
    base_height: F,
) -> Result<Layout, Unsupported>
where
    R: RandomSource + ?Sized,
    F: FnOnce(i32, i32) -> Option<i32>,
{
    // Drawn and thrown away. See the module docs.
    let _ = random.next_f64();

    let mut builder = PiecesBuilder::new();
    // `ChunkPos.getBlockX(2)` / `getBlockZ(2)` — two blocks into the chunk.
    let room_x = chunk_x * 16 + 2;
    let room_z = chunk_z * 16 + 2;

    // `MineShaftRoom`'s constructor: three draws, and the order is the
    // argument order — x extent, then y, then z.
    let max_x = room_x + 7 + random.next_i32_bound(6);
    let max_y = 54 + random.next_i32_bound(6);
    let max_z = room_z + 7 + random.next_i32_bound(6);
    let room = BoundingBox::new(room_x, 50, room_z, max_x, max_y, max_z);
    builder.add(Piece {
        id: ID_ROOM,
        bounding_box: room,
        // The room records no orientation, and vanilla writes none.
        facing: None,
        depth: 0,
    });

    let mut kinds = vec![Kind::Room];
    let mut entrances = Vec::new();
    let mut generator = Generator {
        builder: &mut builder,
        random,
        kinds: &mut kinds,
        entrances: &mut entrances,
    };
    generator.room_children(room, 0, room);

    // `MineshaftStructure#generatePiecesAndAdjust` branches here, and this is
    // the **only** place the type changes behaviour. Inside the pieces it is
    // three block states and nothing else: every reference to the `type`
    // field in `MineShaftPiece`, `MineShaftCorridor` and `MineShaftCrossing`
    // is `getWoodState`, `getPlanksState`, `getFenceState` or `ordinal()` for
    // the `MST` tag, and no piece compares it against `MESA`.
    let dy = match kind {
        MineshaftType::Normal => builder.move_below_sea_level(sea_level, min_y, random, 10),
        MineshaftType::Mesa => {
            // A mesa mine hangs off the surface instead of the sea level.
            // The sample point is the **centre of the union box** — `min +
            // span / 2`, not the `(min + max) / 2` midpoint used elsewhere in
            // this file.
            let Some(bounds) = builder.bounding_box() else {
                return Ok(Layout {
                    pieces: Vec::new(),
                    kinds,
                    entrances,
                    offset: 0,
                });
            };
            let (cx, cy, cz) = bounds.center();
            let Some(surface) = base_height(cx, cz) else {
                return Err(Unsupported(
                    "a mesa mineshaft needs WORLD_SURFACE_WG at its union-box centre, and \
                     the centre is outside the generated area"
                        .to_string(),
                ));
            };
            // `Mth.randomBetweenInclusive(random, seaLevel, surface)` is
            // `nextInt(surface - seaLevel + 1) + seaLevel`, and it is only
            // reached when the surface is above sea level — below it the
            // mine sits at sea level and **no value is drawn at all**.
            let target = if surface <= sea_level {
                sea_level
            } else {
                random.next_i32_bound(surface - sea_level + 1) + sea_level
            };
            let dy = target - cy;
            builder.offset_vertically(dy);
            dy
        }
    };
    // The entrances were recorded before the whole mine moved, so they move
    // with it — they are boxes in the same space as the pieces. Vanilla does
    // this inside `MineShaftRoom.move`, which both paths go through.
    for entrance in &mut entrances {
        entrance.shift(0, dy, 0);
    }
    Ok(Layout {
        pieces: builder.pieces().to_vec(),
        kinds,
        entrances,
        offset: dy,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::legacy::LegacyRandomSource;
    use crate::rng::worldgen_random::WorldgenRandom;

    fn at(seed: i64, cx: i32, cz: i32) -> (Vec<Piece>, i32) {
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(0));
        random.set_large_feature_seed(seed, cx, cz);
        generate(&mut random, cx, cz, MineshaftType::Normal, 63, -64).expect("normal")
    }

    /// A mineshaft is a room plus corridors, and it is not small.
    #[test]
    fn a_mineshaft_is_a_room_and_a_lot_of_corridor() {
        let (pieces, _) = at(20_260_722, 70, 62);
        assert_eq!(pieces[0].id, ID_ROOM, "the room comes first");
        assert_eq!(pieces[0].facing, None, "the room records no facing");
        assert!(pieces.len() > 20, "only {} pieces", pieces.len());
        let corridors = pieces.iter().filter(|p| p.id == ID_CORRIDOR).count();
        assert!(
            corridors > pieces.len() / 3,
            "{corridors} corridors of {} pieces — corridors should dominate",
            pieces.len()
        );
    }

    /// The depth cap is on the *piece*, and a piece may exist at 9 because
    /// the check is `> 8` before the increment.
    #[test]
    fn no_piece_is_deeper_than_nine() {
        for chunk in [(70, 62), (67, 61), (71, 55), (-1, 13)] {
            let (pieces, _) = at(20_260_722, chunk.0, chunk.1);
            let deepest = pieces.iter().map(|p| p.depth).max().expect("pieces");
            assert!(deepest <= 9, "a piece at depth {deepest} in {chunk:?}");
            assert!(deepest >= 2, "only reached depth {deepest} in {chunk:?}");
        }
    }

    /// Every piece is within 80 blocks of the room on both axes — the bound
    /// is measured from the room and never moves.
    #[test]
    fn the_mine_stays_within_eighty_blocks_of_its_room() {
        let (pieces, _) = at(20_260_722, 70, 62);
        let room = pieces[0].bounding_box;
        for piece in &pieces[1..] {
            // The bound is on the *origin* the child was proposed at, not
            // its far corner, so a piece may extend a little past it.
            let overshoot_x = (piece.bounding_box.min_x - room.min_x).abs();
            let overshoot_z = (piece.bounding_box.min_z - room.min_z).abs();
            assert!(
                overshoot_x <= MAX_RADIUS + 16 && overshoot_z <= MAX_RADIUS + 16,
                "{} at {:?} is {overshoot_x}/{overshoot_z} from the room",
                piece.id,
                piece.bounding_box
            );
        }
    }

    /// No two pieces overlap — the collision test is what makes the layout
    /// finite, and a port that skipped it would still terminate on the depth
    /// cap while producing a tangle.
    #[test]
    fn no_two_pieces_overlap() {
        let (pieces, _) = at(20_260_722, 70, 62);
        for (i, a) in pieces.iter().enumerate() {
            for b in &pieces[i + 1..] {
                assert!(
                    !a.bounding_box.intersects(b.bounding_box),
                    "{} {:?} overlaps {} {:?}",
                    a.id,
                    a.bounding_box,
                    b.id,
                    b.bounding_box
                );
            }
        }
    }

    /// The whole mine ends up under the sea.
    #[test]
    fn the_mine_is_moved_below_sea_level() {
        let (pieces, dy) = at(20_260_722, 70, 62);
        assert!(dy < 0, "the mine moved by {dy}, expected downwards");
        let top = pieces
            .iter()
            .map(|p| p.bounding_box.max_y)
            .max()
            .expect("pieces");
        assert!(top < 63, "the mine tops out at {top}, above sea level");
    }

    /// Two runs of one seed agree, and different chunks disagree.
    #[test]
    fn the_layout_is_a_function_of_the_seed_and_the_chunk() {
        assert_eq!(at(20_260_722, 70, 62).0, at(20_260_722, 70, 62).0);
        assert_ne!(at(20_260_722, 70, 62).0, at(20_260_722, 67, 61).0);
        assert_ne!(at(20_260_722, 70, 62).0, at(1, 70, 62).0);
    }

    /// The discarded `nextDouble` is really discarded — removing it changes
    /// the mine. This pins it by construction: a stream advanced by one
    /// double before entering must produce a *different* layout.
    #[test]
    fn the_leading_double_is_load_bearing() {
        let mut a = WorldgenRandom::new(LegacyRandomSource::new(0));
        a.set_large_feature_seed(20_260_722, 70, 62);
        let with = generate(&mut a, 70, 62, MineshaftType::Normal, 63, -64).expect("ok");

        let mut b = WorldgenRandom::new(LegacyRandomSource::new(0));
        b.set_large_feature_seed(20_260_722, 70, 62);
        let _ = b.next_f64(); // stand where `generate` would leave it
        let mut c = WorldgenRandom::new(LegacyRandomSource::new(0));
        c.set_large_feature_seed(20_260_722, 70, 62);
        let _ = c.next_f64();
        let _ = c.next_f64();
        let shifted = generate(&mut c, 70, 62, MineshaftType::Normal, 63, -64).expect("ok");

        assert_ne!(
            with.0, shifted.0,
            "one extra draw before the room must change the layout"
        );
    }

    /// Mesa is refused when no surface height can be supplied, rather than
    /// quietly falling back to the sea-level rule.
    #[test]
    fn a_mesa_mineshaft_without_a_surface_is_refused_by_name() {
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(0));
        let err = generate(&mut random, 0, 0, MineshaftType::Mesa, 63, -64)
            .expect_err("no surface source, so no mesa layout");
        assert!(err.0.contains("mesa"), "{}", err.0);
    }

    fn mesa(cx: i32, cz: i32, surface: i32) -> Layout {
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(0));
        random.set_large_feature_seed(20_260_722, cx, cz);
        generate_with_kinds_using(&mut random, cx, cz, MineshaftType::Mesa, 63, -64, |_, _| {
            Some(surface)
        })
        .expect("a surface was supplied")
    }

    /// The type changes the *palette* and the vertical rule, and nothing
    /// about the walk. `generatePiecesAndAdjust` runs the identical room
    /// constructor and `addChildren` on the identical stream before it
    /// branches, so the two layouts differ by a constant y offset and by
    /// nothing else.
    #[test]
    fn a_mesa_mine_has_the_same_shape_as_a_normal_one() {
        let (normal, dy_n) = at(20_260_722, 70, 62);
        let m = mesa(70, 62, 120);
        assert_eq!(normal.len(), m.pieces.len(), "same piece count");
        for (n, p) in normal.iter().zip(&m.pieces) {
            assert_eq!(n.id, p.id);
            assert_eq!(n.bounding_box.min_x, p.bounding_box.min_x);
            assert_eq!(n.bounding_box.min_z, p.bounding_box.min_z);
            assert_eq!(n.bounding_box.max_x, p.bounding_box.max_x);
            assert_eq!(n.bounding_box.max_z, p.bounding_box.max_z);
            assert_eq!(
                n.bounding_box.min_y - dy_n,
                p.bounding_box.min_y - m.offset,
                "the two differ by their own offsets and nothing else"
            );
        }
    }

    /// `Mth.randomBetweenInclusive` is only reached when the surface is above
    /// sea level. At or below it the mine sits at sea level and **no value is
    /// drawn at all**, so a later draw would land one place early.
    #[test]
    fn a_mesa_mine_at_or_below_sea_level_spends_no_value() {
        // Two surfaces that both take the no-draw branch must leave the
        // stream in the same place, so the layouts are identical.
        assert_eq!(mesa(70, 62, 63).offset, mesa(70, 62, 10).offset);

        // And the offset is exactly sea level minus the union centre's y.
        let m = mesa(70, 62, 63);
        let centre_y = {
            let (_, y, _) = BoundingBox::encapsulating(
                &mesa(70, 62, 63)
                    .pieces
                    .iter()
                    .map(|p| p.bounding_box)
                    .collect::<Vec<_>>(),
            )
            .expect("pieces")
            .center();
            // The centre is measured after the move, so undo it.
            y - m.offset
        };
        assert_eq!(m.offset, 63 - centre_y);
    }

    /// Above sea level the draw is taken, and the result stays inside
    /// `[seaLevel, surface]` — `nextInt(surface - seaLevel + 1) + seaLevel`.
    #[test]
    fn a_mesa_mine_above_sea_level_lands_between_sea_level_and_the_surface() {
        for surface in [64, 90, 120, 200] {
            let m = mesa(70, 62, surface);
            let boxes: Vec<BoundingBox> = m.pieces.iter().map(|p| p.bounding_box).collect();
            let (_, centre_y, _) = BoundingBox::encapsulating(&boxes).expect("pieces").center();
            assert!(
                (63..=surface).contains(&centre_y),
                "surface {surface}: centre landed at {centre_y}"
            );
        }
    }
}

/// The block states a mineshaft is built from.
///
/// Supplied by the caller rather than spelled here, because these are
/// *states* and not names — a fence carries four connection properties and a
/// rail carries a shape. Oracle fault 7 was exactly this mistake made once
/// already.
#[derive(Clone, Debug)]
pub struct MineshaftBlocks {
    /// `type.getWoodState()` — oak/dark oak log.
    pub wood: String,
    /// `type.getPlanksState()`.
    pub planks: String,
    /// `type.getFenceState()`, with every connection false.
    pub fence: String,
    pub cave_air: String,
    pub cobweb: String,
    /// `minecraft:rail`, default shape.
    pub rail: String,
    /// `minecraft:wall_torch`, default facing.
    pub wall_torch: String,
    pub iron_chain: String,
}

impl MineshaftBlocks {
    /// The vanilla states, for the two mineshaft types.
    ///
    /// `states` resolves a name to its default state, so this is the
    /// datapack's spelling rather than a guess.
    #[must_use]
    pub fn vanilla(
        kind: MineshaftType,
        states: &dyn crate::feature::state_spec::DefaultStates,
    ) -> Option<Self> {
        let wood = if kind == MineshaftType::Mesa {
            "minecraft:dark_oak_log"
        } else {
            "minecraft:oak_log"
        };
        let planks = if kind == MineshaftType::Mesa {
            "minecraft:dark_oak_planks"
        } else {
            "minecraft:oak_planks"
        };
        let fence = if kind == MineshaftType::Mesa {
            "minecraft:dark_oak_fence"
        } else {
            "minecraft:oak_fence"
        };
        Some(Self {
            wood: states.default_state(wood)?.to_string(),
            planks: states.default_state(planks)?.to_string(),
            fence: states.default_state(fence)?.to_string(),
            cave_air: states.default_state("minecraft:cave_air")?.to_string(),
            cobweb: states.default_state("minecraft:cobweb")?.to_string(),
            rail: states.default_state("minecraft:rail")?.to_string(),
            wall_torch: states.default_state("minecraft:wall_torch")?.to_string(),
            iron_chain: states.default_state("minecraft:iron_chain")?.to_string(),
        })
    }
}

/// `MineShaftCorridor.postProcess` — the blocks, for one chunk.
///
/// Roughly 2,150 of the ~2,370 mineshaft blocks in the whole-world diff are
/// the plain `cave_air` hollowing at the top of this; everything after it is
/// the furniture. That is worth knowing when reading a parity number after
/// this lands: most of the win is the first two calls.
///
/// The draw order is the whole of the behaviour, and three places in here
/// spend values conditionally — `placeSupport`'s `nextInt(4)`, the two chest
/// coins, and the rails — so an early return anywhere leaves the stream
/// somewhere different.
pub fn post_process_corridor<W: StructureWorld + ?Sized, R: RandomSource + ?Sized>(
    world: &mut W,
    piece: &Piece,
    kind: Kind,
    blocks: &MineshaftBlocks,
    clip: BoundingBox,
    random: &mut R,
    // `MineShaftCorridor.hasPlacedSpider` — **piece state, not call state.**
    //
    // A corridor is post-processed once per chunk it overlaps, and this
    // field carries over between those calls: vanilla checks it *before* the
    // `nextInt(3)` that picks the spawner's z, so a corridor that already
    // placed its cave-spider spawner in one chunk spends no value on it in
    // the next. Resetting it per call redraws, and every draw in the rest of
    // that chunk's mineshaft pass lands one place late.
    //
    // Vanilla saves it as `hps` in the piece NBT, which is what settles that
    // it is meant to persist rather than being an accident of object reuse.
    placed_spider: &mut bool,
) {
    let Kind::Corridor {
        has_rails,
        spider,
        sections,
    } = kind
    else {
        return;
    };
    let protected = protected_blocks(blocks);
    let mut w = Writer {
        world,
        piece,
        clip,
        protected: &protected,
    };
    if w.is_in_invalid_location() {
        return;
    }
    let length = sections * 5 - 1;

    // The tunnel, then the ceiling. This is the part that matters.
    w.generate_box(
        0,
        0,
        0,
        2,
        1,
        length,
        &blocks.cave_air,
        &blocks.cave_air,
        false,
    );
    w.generate_maybe_box(
        random,
        0.8,
        0,
        2,
        0,
        2,
        2,
        length,
        &blocks.cave_air,
        &blocks.cave_air,
        false,
        false,
    );
    if spider {
        w.generate_maybe_box(
            random,
            0.6,
            0,
            0,
            0,
            2,
            1,
            length,
            &blocks.cobweb,
            &blocks.cave_air,
            false,
            true,
        );
    }

    // `RUSTMC_TRACE_MINESHAFT=1` also reports the corridor's draw budget.
    // The ceiling pass above is the first thing that draws and its count is
    // fixed by `length`, so if two implementations disagree on the blocks it
    // writes while agreeing on this number, the streams differ on entry
    // rather than inside. That is the whole question for this piece.
    if trace_mineshaft() {
        println!(
            "MINESHAFT corridor box {},{},{}..{},{},{} sections {sections} length {length} \
             ceiling draws {}",
            piece.bounding_box.min_x,
            piece.bounding_box.min_y,
            piece.bounding_box.min_z,
            piece.bounding_box.max_x,
            piece.bounding_box.max_y,
            piece.bounding_box.max_z,
            3 * (length + 1),
        );
    }

    // `RUSTMC_TRACE_MINESHAFT_PHASES=1` reports, per section, which of the
    // conditional draws were actually spent. `mineshaft-ceiling` localises a
    // draw-count difference to a corridor; this localises it inside one.
    let phases = trace_mineshaft_phases();
    for section in 0..sections {
        let z = 2 + section * 5;
        if phases {
            // Behind the flag, because these are world reads and this runs
            // per section of every corridor in the world.
            let supported = w.is_supporting_box(0, 2, 2, z);
            let webs: Vec<bool> = [
                (0, z - 1),
                (2, z - 1),
                (0, z + 1),
                (2, z + 1),
                (0, z - 2),
                (2, z - 2),
                (0, z + 2),
                (2, z + 2),
            ]
            .iter()
            .map(|&(x, wz)| w.is_interior(x, 2, wz))
            .collect();
            println!(
                "  MINESHAFT section {section} z {z}: support {supported} \
                 interior {webs:?}"
            );
        }
        place_support(&mut w, blocks, random, z);
        maybe_cobweb(&mut w, blocks, random, 0.1, 0, 2, z - 1);
        maybe_cobweb(&mut w, blocks, random, 0.1, 2, 2, z - 1);
        maybe_cobweb(&mut w, blocks, random, 0.1, 0, 2, z + 1);
        maybe_cobweb(&mut w, blocks, random, 0.1, 2, 2, z + 1);
        maybe_cobweb(&mut w, blocks, random, 0.05, 0, 2, z - 2);
        maybe_cobweb(&mut w, blocks, random, 0.05, 2, 2, z - 2);
        maybe_cobweb(&mut w, blocks, random, 0.05, 0, 2, z + 2);
        maybe_cobweb(&mut w, blocks, random, 0.05, 2, 2, z + 2);
        // The chest coins are always spent; the chest itself is conditional
        // on the position, and draws one more for the rail's shape.
        if random.next_i32_bound(100) == 0 {
            create_chest(&mut w, blocks, random, 2, 0, z - 1);
        }
        if random.next_i32_bound(100) == 0 {
            create_chest(&mut w, blocks, random, 0, 0, z + 1);
        }
        if spider && !*placed_spider {
            let spawner_z = z - 1 + random.next_i32_bound(3);
            // `box.isInside(getWorldPos(1, 0, spawnerZ))` comes **before**
            // `isInterior`, and the two ask about different y — `isInterior`
            // tests one block up. This gate does not cost a draw itself, but
            // it decides `hasPlacedSpider`, which suppresses the `nextInt(3)`
            // in every later section of the corridor.
            if w.inside_box(1, 0, spawner_z) && w.is_interior(1, 0, spawner_z) {
                *placed_spider = true;
                w.place_block("minecraft:spawner", 1, 0, spawner_z);
                // `MineShaftCorridor.postProcess` follows the write with
                // `getstatic EntityTypes.CAVE_SPIDER` into
                // `SpawnerBlockEntity.setEntityId(EntityType, RandomSource)`,
                // read off the pinned jar. **A constant, and no draw** —
                // `setEntityId` reaches `getOrCreateNextSpawnData`, whose
                // `spawnPotentials.getRandom(random)` returns empty without
                // drawing while the list is unbuilt, which it always is for a
                // spawner placed a moment ago.
                //
                // A constant that is nonetheless NOT a function of the block
                // state, which is the whole reason this needs the side
                // channel: the block is `minecraft:spawner` here and in a
                // dungeon, and the two save different mobs. Corroborated
                // positionally rather than assumed — of the nineteen
                // `mob_spawner` entries in `gen-wide-12345-r16-ow-c`, exactly
                // the six the census attributes to `minecraft:mineshaft`
                // carry `cave_spider` and none of the thirteen
                // `<feature>`-owned ones do.
                w.record_block_entity(
                    1,
                    0,
                    spawner_z,
                    crate::block_entity::Record {
                        nbt: std::collections::BTreeMap::from([(
                            "SpawnData".to_string(),
                            rustmc_world::nbt::Nbt::Compound(std::collections::BTreeMap::from([(
                                "entity".to_string(),
                                rustmc_world::nbt::Nbt::Compound(std::collections::BTreeMap::from(
                                    [(
                                        "id".to_string(),
                                        rustmc_world::nbt::Nbt::String(
                                            "minecraft:cave_spider".to_string(),
                                        ),
                                    )],
                                )),
                            )])),
                        )]),
                        loot_seed: None,
                    },
                );
            }
        }
    }

    // The floor. `setPlanksBlock` bypasses `placeBlock`, so it ignores the
    // protected list — a plank floor may be laid over an older piece's.
    for x in 0..=2 {
        for z in 0..=length {
            set_planks_block(&mut w, blocks, x, -1, z);
        }
    }

    place_double_support(&mut w, blocks, 0, -1, 2);
    if sections > 1 {
        place_double_support(&mut w, blocks, 0, -1, length - 2);
    }

    if has_rails {
        let rail = with_property(&blocks.rail, "shape", "north_south");
        for z in 0..=length {
            let below = w.get_block(1, -1, z);
            if !super::writer::is_air(&below) && w.state_is_solid_render(&below) {
                // 0.7 underground, 0.9 in the open — and the draw only
                // happens where there is something to lay track on.
                let chance = if w.is_interior(1, 0, z) { 0.7 } else { 0.9 };
                w.maybe_generate_block(random, chance, 1, 0, z, &rail);
            }
        }
    }
}

/// The blocks a mineshaft will not overwrite — `MineShaftPiece.canBeReplaced`.
fn protected_blocks(blocks: &MineshaftBlocks) -> Vec<&str> {
    vec![
        plain_name(&blocks.planks),
        plain_name(&blocks.wood),
        plain_name(&blocks.fence),
        "minecraft:iron_chain",
    ]
}

fn plain_name(state: &str) -> &str {
    state.split_once('[').map_or(state, |(n, _)| n)
}

/// `state.setValue(property, value)` on a canonical state spelling.
fn with_property(state: &str, property: &str, value: &str) -> String {
    let Some((name, rest)) = state.split_once('[') else {
        return format!("{state}[{property}={value}]");
    };
    let mut pairs: Vec<String> = rest
        .trim_end_matches(']')
        .split(',')
        .map(|pair| match pair.split_once('=') {
            Some((k, _)) if k == property => format!("{property}={value}"),
            _ => pair.to_string(),
        })
        .collect();
    if !pairs.iter().any(|p| p.starts_with(&format!("{property}="))) {
        pairs.push(format!("{property}={value}"));
        pairs.sort();
    }
    format!("{name}[{}]", pairs.join(","))
}

// `BlockState#isSolidRender` used to be answered here from a list of block
// names. It is now [`StructureWorld::state_is_solid_render`], off the
// occlusion column of the props table, because the list was wrong for every
// block a mine puts under its own floor — `oak_fence`, `iron_chain`,
// `spawner` and `pointed_dripstone` are all named as if they occluded and
// none of them do — and the answer gates a `maybeGenerateBlock` draw.

/// `MineShaftPiece.setPlanksBlock` — a plank under anything the floor cannot
/// stand on, and **only** where the position is interior.
///
/// Writes with the world's own setter rather than `placeBlock`, so it is not
/// subject to `canBeReplaced`.
fn set_planks_block<W: StructureWorld + ?Sized>(
    w: &mut Writer<'_, W>,
    blocks: &MineshaftBlocks,
    x: i32,
    y: i32,
    z: i32,
) {
    if !w.is_interior(x, y, z) {
        return;
    }
    // `state.isFaceSturdy(level, pos, UP)`, off the props table. It used to
    // be approximated by "not air and not a fluid", which answers the wrong
    // way for everything a cave leaves lying on a corridor floor — a
    // pointed dripstone, a rail, a torch, an amethyst cluster all present no
    // sturdy up-face and all read as sturdy under a name test, so the plank
    // that should cover them was never laid. The floor then feeds the rail
    // pass, which draws only where it finds something to lay track on.
    if !w.is_sturdy_face_up(x, y, z) {
        w.place_block_ignoring_protection(&blocks.planks, x, y, z);
    }
}

/// `MineShaftCorridor.maybePlaceCobWeb`.
///
/// The interior test comes **before** the draw here — the opposite order
/// from `generateMaybeBox`, in the same file.
fn maybe_cobweb<W: StructureWorld + ?Sized, R: RandomSource + ?Sized>(
    w: &mut Writer<'_, W>,
    blocks: &MineshaftBlocks,
    random: &mut R,
    chance: f32,
    x: i32,
    y: i32,
    z: i32,
) {
    if !w.is_interior(x, y, z) {
        return;
    }
    if random.next_f32() >= chance {
        return;
    }
    if w.has_sturdy_neighbours(x, y, z, 2) {
        w.place_block(&blocks.cobweb, x, y, z);
    }
}

/// `MineShaftCorridor.placeSupport`.
fn place_support<W: StructureWorld + ?Sized, R: RandomSource + ?Sized>(
    w: &mut Writer<'_, W>,
    blocks: &MineshaftBlocks,
    random: &mut R,
    z: i32,
) {
    if !w.is_supporting_box(0, 2, 2, z) {
        return;
    }
    // `RUSTMC_TRACE_MINESHAFT_PHASES=1` also reports the support that is
    // actually written. The guard's answer differs between a piece's two
    // per-chunk passes — normal, since vanilla's guards do the same — so the
    // question is which pass's *write* survives the clip.
    if trace_mineshaft_phases() {
        let ((wx, wy, wz), admitted) = w.traced_target(0, 0, z);
        println!(
            "  MINESHAFT support WRITE local (0,0,{z}) world ({wx},{wy},{wz}) \
             clip_admits={admitted}"
        );
    }
    let west_fence = with_property(&blocks.fence, "west", "true");
    let east_fence = with_property(&blocks.fence, "east", "true");
    w.generate_box(0, 0, z, 0, 1, z, &west_fence, &blocks.cave_air, false);
    w.generate_box(2, 0, z, 2, 1, z, &east_fence, &blocks.cave_air, false);
    if random.next_i32_bound(4) == 0 {
        w.generate_box(0, 2, z, 0, 2, z, &blocks.planks, &blocks.cave_air, false);
        w.generate_box(2, 2, z, 2, 2, z, &blocks.planks, &blocks.cave_air, false);
    } else {
        w.generate_box(0, 2, z, 2, 2, z, &blocks.planks, &blocks.cave_air, false);
        let south_torch = with_property(&blocks.wall_torch, "facing", "south");
        let north_torch = with_property(&blocks.wall_torch, "facing", "north");
        w.maybe_generate_block(random, 0.05, 1, 2, z - 1, &south_torch);
        w.maybe_generate_block(random, 0.05, 1, 2, z + 1, &north_torch);
    }
}

/// `MineShaftCorridor.placeDoubleLowerOrUpperSupport` — a pillar down or a
/// chain up, under each end of a plank floor.
fn place_double_support<W: StructureWorld + ?Sized>(
    w: &mut Writer<'_, W>,
    blocks: &MineshaftBlocks,
    x: i32,
    y: i32,
    z: i32,
) {
    let planks = plain_name(&blocks.planks).to_string();
    if plain_name(&w.get_block(x, y, z)) == planks {
        w.fill_pillar_down_or_chain_up(&blocks.wood, &blocks.fence, &blocks.iron_chain, x, y, z);
    }
    if plain_name(&w.get_block(x + 2, y, z)) == planks {
        w.fill_pillar_down_or_chain_up(
            &blocks.wood,
            &blocks.fence,
            &blocks.iron_chain,
            x + 2,
            y,
            z,
        );
    }
}

/// `MineShaftCorridor.createChest` — a rail, and a chest minecart this port
/// does not spawn.
///
/// The `nextBoolean` for the rail's axis is drawn **only** when the position
/// is air over something solid, so a chest coin that passes does not always
/// cost a second value.
fn create_chest<W: StructureWorld + ?Sized, R: RandomSource + ?Sized>(
    w: &mut Writer<'_, W>,
    blocks: &MineshaftBlocks,
    random: &mut R,
    x: i32,
    y: i32,
    z: i32,
) {
    // `box.isInside(pos)`, then two `level.getBlockState(..).isAir()` — the
    // real world, not `getBlock(.., box)`, and `isAir()` rather than a name
    // comparison.
    //
    // Both details were wrong here and both cost draws. A chest sits at
    // local y 0, inside the tunnel the corridor has just carved, so the
    // block there is **`cave_air`**: comparing the name against
    // `minecraft:air` made this return before every chest in the game, and
    // with it the two values the chest spends.
    if !w.inside_box(x, y, z) {
        return;
    }
    if !super::writer::is_air(&w.world_block(x, y, z)) {
        return;
    }
    if super::writer::is_air(&w.world_block(x, y - 1, z)) {
        return;
    }
    let shape = if random.next_bool() {
        "north_south"
    } else {
        "east_west"
    };
    w.place_block(&with_property(&blocks.rail, "shape", shape), x, y, z);
    // `MinecartChest.setLootTable(ABANDONED_MINESHAFT, random.nextLong())`,
    // then `addFreshEntity`. `EntityType.create` returns non-null on a real
    // level, so vanilla always spends this value.
    let loot_seed = random.next_i64();
    let at = w.piece.world_pos(x, y, z);
    w.world
        .add_fresh_entity(crate::structure::spawned::chest_minecart(
            at,
            "minecraft:chests/abandoned_mineshaft",
            loot_seed,
            crate::structure::spawned::uuid("minecraft:chest_minecart", at, 0),
        ));
}

/// `MineShaftStairs.postProcess` — three boxes and a staircase of five.
///
/// Draws nothing at all.
pub fn post_process_stairs<W: StructureWorld + ?Sized>(
    world: &mut W,
    piece: &Piece,
    blocks: &MineshaftBlocks,
    clip: BoundingBox,
) {
    let protected = protected_blocks(blocks);
    let mut w = Writer {
        world,
        piece,
        clip,
        protected: &protected,
    };
    if w.is_in_invalid_location() {
        return;
    }
    let air = blocks.cave_air.clone();
    w.generate_box(0, 5, 0, 2, 7, 1, &air, &air, false);
    w.generate_box(0, 0, 7, 2, 2, 8, &air, &air, false);
    for step in 0..5 {
        // The floor drops one per step except the last, which is the flat
        // landing at the bottom.
        let drop = i32::from(step < 4);
        w.generate_box(
            0,
            5 - step - drop,
            2 + step,
            2,
            7 - step,
            2 + step,
            &air,
            &air,
            false,
        );
    }
}

/// `MineShaftCrossing.postProcess`.
///
/// Draws nothing either, and works in **absolute** coordinates — a crossing
/// records no orientation, so its local frame is the world's.
pub fn post_process_crossing<W: StructureWorld + ?Sized>(
    world: &mut W,
    piece: &Piece,
    two_floored: bool,
    blocks: &MineshaftBlocks,
    clip: BoundingBox,
) {
    let protected = protected_blocks(blocks);
    let bb = piece.bounding_box;
    let mut w = Writer {
        world,
        piece,
        clip,
        protected: &protected,
    };
    if w.is_in_invalid_location() {
        return;
    }
    let air = blocks.cave_air.clone();
    if two_floored {
        w.generate_box(
            bb.min_x + 1,
            bb.min_y,
            bb.min_z,
            bb.max_x - 1,
            bb.min_y + 3 - 1,
            bb.max_z,
            &air,
            &air,
            false,
        );
        w.generate_box(
            bb.min_x,
            bb.min_y,
            bb.min_z + 1,
            bb.max_x,
            bb.min_y + 3 - 1,
            bb.max_z - 1,
            &air,
            &air,
            false,
        );
        w.generate_box(
            bb.min_x + 1,
            bb.max_y - 2,
            bb.min_z,
            bb.max_x - 1,
            bb.max_y,
            bb.max_z,
            &air,
            &air,
            false,
        );
        w.generate_box(
            bb.min_x,
            bb.max_y - 2,
            bb.min_z + 1,
            bb.max_x,
            bb.max_y,
            bb.max_z - 1,
            &air,
            &air,
            false,
        );
        // The hole between the two floors.
        w.generate_box(
            bb.min_x + 1,
            bb.min_y + 3,
            bb.min_z + 1,
            bb.max_x - 1,
            bb.min_y + 3,
            bb.max_z - 1,
            &air,
            &air,
            false,
        );
    } else {
        w.generate_box(
            bb.min_x + 1,
            bb.min_y,
            bb.min_z,
            bb.max_x - 1,
            bb.max_y,
            bb.max_z,
            &air,
            &air,
            false,
        );
        w.generate_box(
            bb.min_x,
            bb.min_y,
            bb.min_z + 1,
            bb.max_x,
            bb.max_y,
            bb.max_z - 1,
            &air,
            &air,
            false,
        );
    }
    for (x, z) in [
        (bb.min_x + 1, bb.min_z + 1),
        (bb.min_x + 1, bb.max_z - 1),
        (bb.max_x - 1, bb.min_z + 1),
        (bb.max_x - 1, bb.max_z - 1),
    ] {
        // `placeSupportPillar`: only where there is something overhead —
        // `getBlock(level, x, height + 1, z, box).isAir()`, and a pillar is
        // placed only when that is **false**.
        //
        // **`isAir()`, not a name comparison, and this is the second time
        // that distinction has been paid for in this file.** A crossing sits
        // in a corridor system the carvers have usually opened, so the block
        // over its corners is `cave_air` far more often than it is `air`;
        // `!= "minecraft:air"` reads `cave_air` as "something overhead" and
        // raises four plank pillars into an open cave where vanilla raises
        // none. Six such blocks on the r=16 window at seed 20260722, at
        // (1158..1160, -38..-36, 894), every one of them `cave_air` in both
        // vanilla references.
        //
        // `place_chest` thirty lines up carries a comment about exactly this
        // — same block, same reason, found and fixed there and not carried
        // the length of one file.
        if !super::writer::is_air(&w.get_block(x, bb.max_y + 1, z)) {
            w.generate_box(x, bb.min_y, z, x, bb.max_y, z, &blocks.planks, &air, false);
        }
    }
    let floor_y = bb.min_y - 1;
    for x in bb.min_x..=bb.max_x {
        for z in bb.min_z..=bb.max_z {
            set_planks_block(&mut w, blocks, x, floor_y, z);
        }
    }
}

/// `MineShaftRoom.postProcess` — a hollow with a domed ceiling, plus the
/// entrances its children broke through.
pub fn post_process_room<W: StructureWorld + ?Sized>(
    world: &mut W,
    piece: &Piece,
    entrances: &[BoundingBox],
    blocks: &MineshaftBlocks,
    clip: BoundingBox,
) {
    let protected = protected_blocks(blocks);
    let bb = piece.bounding_box;
    let mut w = Writer {
        world,
        piece,
        clip,
        protected: &protected,
    };
    if w.is_in_invalid_location() {
        return;
    }
    let air = blocks.cave_air.clone();
    w.generate_box(
        bb.min_x,
        bb.min_y + 1,
        bb.min_z,
        bb.max_x,
        (bb.min_y + 3).min(bb.max_y),
        bb.max_z,
        &air,
        &air,
        false,
    );
    for e in entrances {
        w.generate_box(
            e.min_x,
            e.max_y - 2,
            e.min_z,
            e.max_x,
            e.max_y,
            e.max_z,
            &air,
            &air,
            false,
        );
    }
    w.generate_upper_half_sphere(
        bb.min_x,
        bb.min_y + 4,
        bb.min_z,
        bb.max_x,
        bb.max_y,
        bb.max_z,
        &air,
        false,
    );
}

#[cfg(test)]
mod block_tests {
    use super::*;
    use crate::rng::legacy::LegacyRandomSource;
    use crate::rng::worldgen_random::WorldgenRandom;
    use std::collections::BTreeMap;

    struct Rock {
        blocks: BTreeMap<(i32, i32, i32), String>,
        surface: i32,
        /// The block-entity side channel, collected rather than dropped, so a
        /// spawner that stopped recording its mob shows up as a failing test
        /// instead of as a world whose blocks are all correct.
        records: Vec<((i32, i32, i32), crate::block_entity::Record)>,
    }

    impl crate::block_entity::BlockEntityRecorder for Rock {
        fn record_block_entity(
            &mut self,
            x: i32,
            y: i32,
            z: i32,
            record: crate::block_entity::Record,
        ) {
            self.records.push(((x, y, z), record));
        }
    }

    impl StructureWorld for Rock {
        fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
            self.blocks
                .get(&(x, y, z))
                .cloned()
                .unwrap_or_else(|| "minecraft:stone".to_string())
        }
        fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
            self.blocks.insert((x, y, z), state.to_string());
        }
        fn ocean_floor_wg(&mut self, _: i32, _: i32) -> i32 {
            self.surface
        }
        fn biome_blocks_mineshafts(&mut self, _: i32, _: i32, _: i32) -> bool {
            false
        }
        fn is_liquid(&mut self, _: i32, _: i32, _: i32) -> bool {
            false
        }
        fn is_sturdy_face(&mut self, x: i32, y: i32, z: i32, _: i32, _: i32, _: i32) -> bool {
            self.block_at(x, y, z) == "minecraft:stone"
        }
        fn can_hang_chain_below(&mut self, x: i32, y: i32, z: i32) -> bool {
            self.block_at(x, y, z) == "minecraft:stone"
        }
        fn state_is_solid_render(&self, state: &str) -> bool {
            state == "minecraft:stone"
        }
    }

    fn blocks() -> MineshaftBlocks {
        MineshaftBlocks {
            wood: "minecraft:oak_log[axis=y]".to_string(),
            planks: "minecraft:oak_planks".to_string(),
            fence: "minecraft:oak_fence[east=false,north=false,south=false,waterlogged=false,west=false]"
                .to_string(),
            cave_air: "minecraft:cave_air".to_string(),
            cobweb: "minecraft:cobweb".to_string(),
            rail: "minecraft:rail[shape=north_south,waterlogged=false]".to_string(),
            wall_torch: "minecraft:wall_torch[facing=north]".to_string(),
            iron_chain: "minecraft:iron_chain[axis=y,waterlogged=false]".to_string(),
        }
    }

    /// A corridor hollows itself out — the part that carries almost all of
    /// the parity weight.
    #[test]
    fn a_corridor_hollows_its_tunnel() {
        let piece = Piece {
            id: ID_CORRIDOR,
            bounding_box: BoundingBox::new(0, 40, 0, 2, 42, 9),
            facing: Some(Facing::South),
            depth: 1,
        };
        let mut world = Rock {
            blocks: BTreeMap::new(),
            surface: 200,
            records: Vec::new(),
        };
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(3));
        post_process_corridor(
            &mut world,
            &piece,
            Kind::Corridor {
                has_rails: false,
                spider: false,
                sections: 2,
            },
            &blocks(),
            BoundingBox::new(-64, -64, -64, 64, 320, 64),
            &mut random,
            &mut false,
        );
        // The tunnel proper: local y 0..1 over the whole length.
        assert_eq!(world.block_at(1, 40, 3), "minecraft:cave_air");
        assert_eq!(world.block_at(1, 41, 3), "minecraft:cave_air");
        // Below the floor is planks or untouched stone, never cave air.
        assert_ne!(world.block_at(1, 39, 3), "minecraft:cave_air");
    }

    /// A spider corridor records `cave_spider` at the spawner it places, and
    /// records it at the position the spawner block actually landed on.
    ///
    /// The position is the half worth testing: `record_block_entity` takes
    /// piece-local coordinates and converts them, and a payload one
    /// `world_pos` away from its block is dropped by the emitter as an orphan
    /// — which looks exactly like a class nobody ported.
    #[test]
    fn a_spider_corridor_records_its_cave_spider_where_the_spawner_landed() {
        let piece = Piece {
            id: ID_CORRIDOR,
            bounding_box: BoundingBox::new(0, 40, 0, 2, 42, 9),
            facing: Some(Facing::South),
            depth: 1,
        };
        let mut world = Rock {
            blocks: BTreeMap::new(),
            surface: 200,
            records: Vec::new(),
        };
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(3));
        post_process_corridor(
            &mut world,
            &piece,
            Kind::Corridor {
                has_rails: false,
                spider: true,
                sections: 2,
            },
            &blocks(),
            BoundingBox::new(-64, -64, -64, 64, 320, 64),
            &mut random,
            &mut false,
        );
        let spawners: Vec<&(i32, i32, i32)> = world
            .blocks
            .iter()
            .filter(|(_, state)| state.as_str() == "minecraft:spawner")
            .map(|(pos, _)| pos)
            .collect();
        assert_eq!(spawners.len(), 1, "one spawner: {spawners:?}");
        assert_eq!(
            world.records.len(),
            1,
            "one record, at the spawner: {:?}",
            world.records
        );
        assert_eq!(
            &world.records[0].0, spawners[0],
            "the record must sit on the block"
        );
        let Some(rustmc_world::nbt::Nbt::Compound(data)) = world.records[0].1.nbt.get("SpawnData")
        else {
            panic!("SpawnData: {:?}", world.records[0].1)
        };
        let Some(rustmc_world::nbt::Nbt::Compound(entity)) = data.get("entity") else {
            panic!("entity: {data:?}")
        };
        assert_eq!(
            entity.get("id"),
            Some(&rustmc_world::nbt::Nbt::String(
                "minecraft:cave_spider".to_string()
            ))
        );
        assert_eq!(world.records[0].1.loot_seed, None);
    }

    /// The ceiling maybe-box draws once per position whatever it writes, so
    /// the stream position after a corridor does not depend on how much of
    /// its roof fell in.
    #[test]
    fn the_ceiling_costs_the_same_however_it_lands() {
        let piece = Piece {
            id: ID_CORRIDOR,
            bounding_box: BoundingBox::new(0, 40, 0, 2, 42, 9),
            facing: Some(Facing::South),
            depth: 1,
        };
        let run = |seed: i64| {
            let mut world = Rock {
                blocks: BTreeMap::new(),
                surface: 200,
                records: Vec::new(),
            };
            let mut random = WorldgenRandom::new(LegacyRandomSource::new(seed));
            post_process_corridor(
                &mut world,
                &piece,
                Kind::Corridor {
                    has_rails: false,
                    spider: false,
                    sections: 2,
                },
                &blocks(),
                BoundingBox::new(-64, -64, -64, 64, 320, 64),
                &mut random,
                &mut false,
            );
            let air = world
                .blocks
                .values()
                .filter(|b| *b == "minecraft:cave_air")
                .count();
            (air, random)
        };
        let (air_a, mut a) = run(11);
        let (air_b, mut b) = run(12);
        assert_ne!(air_a, air_b, "different seeds should hollow differently");
        // Both consumed the same number of values up to the ceiling box; the
        // later conditionals differ, so this only checks they both advanced.
        assert_ne!(a.next_i64(), 0);
        assert_ne!(b.next_i64(), 0);
    }

    /// A corridor in a blocking location writes nothing and draws nothing.
    #[test]
    fn an_invalid_location_costs_nothing() {
        struct Blocked(Rock);

        impl crate::block_entity::BlockEntityRecorder for Blocked {
            fn record_block_entity(
                &mut self,
                x: i32,
                y: i32,
                z: i32,
                record: crate::block_entity::Record,
            ) {
                self.0.record_block_entity(x, y, z, record);
            }
        }
        impl StructureWorld for Blocked {
            fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
                self.0.block_at(x, y, z)
            }
            fn set_block(&mut self, x: i32, y: i32, z: i32, s: &str) {
                self.0.set_block(x, y, z, s);
            }
            fn ocean_floor_wg(&mut self, x: i32, z: i32) -> i32 {
                self.0.ocean_floor_wg(x, z)
            }
            fn biome_blocks_mineshafts(&mut self, _: i32, _: i32, _: i32) -> bool {
                true
            }
            fn is_liquid(&mut self, _: i32, _: i32, _: i32) -> bool {
                false
            }
            fn is_sturdy_face(&mut self, _: i32, _: i32, _: i32, _: i32, _: i32, _: i32) -> bool {
                true
            }
            fn can_hang_chain_below(&mut self, _: i32, _: i32, _: i32) -> bool {
                true
            }
            fn state_is_solid_render(&self, state: &str) -> bool {
                self.0.state_is_solid_render(state)
            }
        }
        let piece = Piece {
            id: ID_CORRIDOR,
            bounding_box: BoundingBox::new(0, 40, 0, 2, 42, 9),
            facing: Some(Facing::South),
            depth: 1,
        };
        let mut world = Blocked(Rock {
            blocks: BTreeMap::new(),
            surface: 200,
            records: Vec::new(),
        });
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(3));
        let mut probe = WorldgenRandom::new(LegacyRandomSource::new(3));
        post_process_corridor(
            &mut world,
            &piece,
            Kind::Corridor {
                has_rails: false,
                spider: false,
                sections: 2,
            },
            &blocks(),
            BoundingBox::new(-64, -64, -64, 64, 320, 64),
            &mut random,
            &mut false,
        );
        assert!(world.0.blocks.is_empty(), "nothing should be written");
        assert_eq!(random.next_i64(), probe.next_i64(), "and nothing drawn");
    }
}
