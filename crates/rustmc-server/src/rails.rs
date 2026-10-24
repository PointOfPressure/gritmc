//! `RailState` — the rail's auto-shaping rule, and the third hook in the game.
//!
//! # Why a rail needs a hook neither of the other two provides
//!
//! Every other block in the neighbour-aware family answers a *question*:
//! `getStateForPlacement` says what to place and `updateShape` says what a
//! block becomes when a neighbour changes. Both are pure — the caller writes
//! the answer.
//!
//! A rail does not work that way, and reading its `getStateForPlacement`
//! makes the point: it consults **no neighbour at all**.
//! `BaseRailBlock.getStateForPlacement`, offsets 0-94, is the fluid at the
//! clicked position and `getHorizontalDirection() == EAST || == WEST ?
//! EAST_WEST : NORTH_SOUTH`. That is entirely a function of the click, so
//! `data/vanilla/placement.json` already carries it exactly and nothing in
//! `crate::placement` needs changing for port `300`.
//!
//! `updateShape` is no better: `BaseRailBlock.updateShape` only schedules the
//! waterlog tick. So a table of neighbour states and a refinement on the
//! placement answer are **both** blind to the entire connective rule.
//!
//! The rule lives in a third place:
//!
//! ```text
//!   LevelChunk.setBlockState  514-542   onPlace(level, pos, oldState, moved)
//!   BaseRailBlock.onPlace       0-23    if (oldState.is(getBlock())) return;
//!                                       updateState(state, level, pos, moved)
//!   BaseRailBlock.updateState   0-27    updateDir(level, pos, state, true)
//!   BaseRailBlock.updateDir     9-47    new RailState(level, pos, state)
//!                                         .place(hasNeighborSignal(pos), true, oldShape)
//!                                         .getState()
//! ```
//!
//! and `RailState.place` **writes**: its own position, and every neighbouring
//! rail it decides to bend. That is the thing no existing hook can express.
//! `240`'s chest changes a block nobody clicked through the shape cascade,
//! which is a pure function replayed by `set_block`; a rail changes its
//! neighbours by calling `setBlock` on them itself, from inside the block
//! layer, recursively.
//!
//! # Where `onPlace` sits, which is not where it looks like it sits
//!
//! It is called from `LevelChunk.setBlockState`, **not** from `Level.setBlock`
//! — so it runs *before* `sendBlockUpdated` and *before*
//! `updateNeighbourShapes`, inside the write rather than after it. Read from
//! the pinned jar at offsets 514-542, gated on `!isClientSide` and on
//! `(flags & 512) == 0`. [`crate::shapes::set_block`] therefore calls it
//! immediately after the write lands and before it touches `updates` or the
//! cascade.
//!
//! # Why this terminates
//!
//! `place` writes rails over rails, and each of those writes re-enters
//! `onPlace`. The recursion is stopped by `onPlace`'s own first line —
//! `if (oldState.is(state.getBlock())) return;` at offsets 0-12 — because the
//! old state at every position `place` writes to is already a rail of the
//! same block. It is not a depth limit and not a visited set; it is that one
//! test, and a port that "helpfully" dropped it would loop until the
//! recursion guard caught it.
//!
//! # What is NOT ported, named rather than discovered later
//!
//!   * **`powered` is always `false`.** `updateDir` passes
//!     `level.hasNeighborSignal(pos)`, which is the redstone path
//!     `crate::shapes` documents as unported. It is not cosmetic: with three
//!     or more neighbours the powered and unpowered arms of `place` try the
//!     four corners in **opposite orders** (offsets 341-385 against
//!     390-423), so a rail junction beside a lit lever picks a different
//!     curve. With no redstone in the world `hasNeighborSignal` is false and
//!     this is exact, which is the case every corpus scenario is in.
//!   * **`isStraight` is read from the class, and only `RailBlock` is
//!     false.** `PoweredRailBlock`, `DetectorRailBlock` and
//!     `ActivatorRailBlock` pass `true` to `BaseRailBlock`'s constructor and
//!     so never take a corner at all — the `!isStraight` guards at offsets
//!     203-207 and 334-338 skip every curved answer for them.
//!   * **`isRail` is `state.is(BlockTags.RAILS) && block instanceof
//!     BaseRailBlock`**, a conjunction, and only the second half is carried
//!     here. Using one half of a conjunction is the *permissive* direction,
//!     so this is written down rather than assumed harmless: it would differ
//!     only for a `BaseRailBlock` outside the tag, of which 26.2 has none.

use crate::edits::Blocks;
use crate::interact::{BlockPos, Face};
use crate::placement::{property_of, with_property};

/// `RailShape`'s ten values, in the enum's own declaration order.
///
/// The order is load-bearing twice over: `updateConnections`' `tableswitch`
/// is indexed by it, and the two ascending blocks in `place` compare against
/// specific members. Read from the field order of
/// `net.minecraft.world.level.block.state.properties.RailShape`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RailShape {
    NorthSouth,
    EastWest,
    AscendingEast,
    AscendingWest,
    AscendingNorth,
    AscendingSouth,
    SouthEast,
    SouthWest,
    NorthWest,
    NorthEast,
}

impl RailShape {
    /// The property value vanilla writes into a state string.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::NorthSouth => "north_south",
            Self::EastWest => "east_west",
            Self::AscendingEast => "ascending_east",
            Self::AscendingWest => "ascending_west",
            Self::AscendingNorth => "ascending_north",
            Self::AscendingSouth => "ascending_south",
            Self::SouthEast => "south_east",
            Self::SouthWest => "south_west",
            Self::NorthWest => "north_west",
            Self::NorthEast => "north_east",
        }
    }

    /// The inverse. `None` for anything that is not one of the ten, which is
    /// how a `shape` property belonging to a different block declines.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "north_south" => Self::NorthSouth,
            "east_west" => Self::EastWest,
            "ascending_east" => Self::AscendingEast,
            "ascending_west" => Self::AscendingWest,
            "ascending_north" => Self::AscendingNorth,
            "ascending_south" => Self::AscendingSouth,
            "south_east" => Self::SouthEast,
            "south_west" => Self::SouthWest,
            "north_west" => Self::NorthWest,
            "north_east" => Self::NorthEast,
            _ => return None,
        })
    }
}

/// The four horizontal steps `place` and `connectTo` both name, in the order
/// their bytecode computes them: north, south, west, east.
const fn around(pos: BlockPos) -> [BlockPos; 4] {
    [
        Face::North.relative(pos),
        Face::South.relative(pos),
        Face::West.relative(pos),
        Face::East.relative(pos),
    ]
}

const fn above((x, y, z): BlockPos) -> BlockPos {
    (x, y + 1, z)
}

const fn below((x, y, z): BlockPos) -> BlockPos {
    (x, y - 1, z)
}

/// The world a `RailState` reads and writes, plus the two class questions it
/// asks that no state string can answer.
///
/// A struct rather than four closures because `place` recurses through
/// `connectTo`, and every level of that recursion needs the same four —
/// passing them separately made the signatures longer than the rule.
pub struct Rails<'a> {
    /// `BaseRailBlock.isRail(BlockState)`, minus the tag half. See the
    /// module docs.
    pub is_rail: &'a dyn Fn(&str) -> bool,
    /// `BaseRailBlock.isStraight()` for the block a state names — `false`
    /// only for `RailBlock`.
    pub is_straight: &'a dyn Fn(&str) -> bool,
    /// The shape table the writes below pass on, so that
    /// `level.setBlock(pos, state, 3)` keeps running `updateNeighbourShapes`
    /// the way vanilla's does. Nothing a rail writes has a shape row today,
    /// but dropping the table here would make that a property of this port
    /// rather than of the table.
    pub shapes: Option<&'a crate::shapes::ShapeTable>,
}

/// One rail, its position, and the positions its shape says it joins to.
///
/// Vanilla's `RailState` is a mutable object with a `connections` list that
/// `removeSoftConnections` edits in place and `connectTo` appends to. That
/// mutability is not incidental — `hasConnection` is asked *between* the
/// edits — so it is reproduced rather than turned into a pure function.
pub struct RailState {
    pos: BlockPos,
    state: String,
    is_straight: bool,
    connections: Vec<BlockPos>,
}

impl RailState {
    /// `new RailState(Level, BlockPos, BlockState)`, offsets 0-70.
    ///
    /// The constructor ends by calling `updateConnections(shape)` with the
    /// state's *current* shape, so a freshly built `RailState` already knows
    /// where the block it names claims to point.
    #[must_use]
    pub fn new(rails: &Rails, pos: BlockPos, state: &str) -> Self {
        let mut me = Self {
            pos,
            state: state.to_string(),
            is_straight: (rails.is_straight)(state),
            connections: Vec::new(),
        };
        if let Some(shape) = property_of(state, "shape").and_then(RailShape::from_name) {
            me.update_connections(shape);
        }
        me
    }

    /// The state this rail ended up with — `getState()`.
    #[must_use]
    pub fn state(&self) -> &str {
        &self.state
    }

    /// `updateConnections(RailShape)`, offsets 0-451.
    ///
    /// A `tableswitch` over the shape into ten two-element lists. The
    /// **ascending** four are the ones worth reading twice: the raised end
    /// is the one that gets `.above()`, so `ASCENDING_EAST` joins `west` and
    /// `east.above()` — the rail climbs towards the east.
    fn update_connections(&mut self, shape: RailShape) {
        let [north, south, west, east] = around(self.pos);
        self.connections.clear();
        let pair = match shape {
            RailShape::NorthSouth => [north, south],
            RailShape::EastWest => [west, east],
            RailShape::AscendingEast => [west, above(east)],
            RailShape::AscendingWest => [above(west), east],
            RailShape::AscendingNorth => [above(north), south],
            RailShape::AscendingSouth => [north, above(south)],
            RailShape::SouthEast => [east, south],
            RailShape::SouthWest => [west, south],
            RailShape::NorthWest => [west, north],
            RailShape::NorthEast => [east, north],
        };
        self.connections.extend_from_slice(&pair);
    }

    /// `hasConnection(BlockPos)`, offsets 0-60.
    ///
    /// **X and Z only — the Y is deliberately not compared.** That is what
    /// lets a flat rail recognise a connection to an ascending neighbour one
    /// block up or down without a second case for it, and copying the
    /// comparison as a whole-position equality would silently break every
    /// slope.
    fn has_connection(&self, pos: BlockPos) -> bool {
        self.connections
            .iter()
            .any(|c| c.0 == pos.0 && c.2 == pos.2)
    }

    /// `countPotentialConnections()`, offsets 0-50 — how many of the four
    /// horizontal neighbours have a rail at, above or below them.
    ///
    /// ```java
    /// // RailBlock.updateState(state, level, pos, neighbourBlock), 0-37
    /// if (neighbourBlock.defaultBlockState().isSignalSource()
    ///     && new RailState(level, pos, state).countPotentialConnections() == 3)
    ///     updateDir(level, pos, state, false);   // alwaysConnect FALSE
    /// ```
    ///
    /// That is the only use of this count in the game: a three-way rail
    /// junction re-picks its curve when a redstone component beside it
    /// changes. It arrives through `neighborChanged`, which is unported, so
    /// nothing here calls it yet. Note the `false` — that call is the one
    /// place `alwaysConnect` is not `true`, so a junction re-shaped this way
    /// writes only if its shape actually moved.
    #[must_use]
    pub fn count_potential_connections(&self, rails: &Rails, blocks: &dyn Blocks) -> usize {
        around(self.pos)
            .into_iter()
            .filter(|p| self.has_rail(rails, blocks, *p))
            .count()
    }

    /// `connectsTo(RailState)`, offsets 0-8.
    fn connects_to(&self, other: &Self) -> bool {
        self.has_connection(other.pos)
    }

    /// `canConnectTo(RailState)`, offsets 0-26.
    ///
    /// `!connectsTo(other) || connections.size() != 2` — a rail will accept a
    /// new neighbour if it is not already pointing at it, **or** if it has
    /// room. Both arms matter and the `||` is easy to read as `&&`.
    fn can_connect_to(&self, other: &Self) -> bool {
        !self.connects_to(other) || self.connections.len() != 2
    }

    /// `hasRail(BlockPos)`, offsets 0-44 — a rail at the position, one above
    /// it, or one below it.
    ///
    /// `pub` and unreachable from this module, deliberately: its only caller
    /// is [`Self::count_potential_connections`], whose only caller is
    /// `RailBlock.updateState(state, level, pos, Block)` on the
    /// `neighborChanged` path — which this server does not port. Ported and
    /// exposed rather than dropped so the gap is one missing entry point
    /// instead of a missing rule.
    pub fn has_rail(&self, rails: &Rails, blocks: &dyn Blocks, pos: BlockPos) -> bool {
        (rails.is_rail)(&blocks.state_at(pos))
            || (rails.is_rail)(&blocks.state_at(above(pos)))
            || (rails.is_rail)(&blocks.state_at(below(pos)))
    }

    /// `getRail(BlockPos)`, offsets 0-103 — the same three positions as
    /// [`Self::has_rail`], in the same order, returning the first that holds
    /// a rail. The order is the answer when a slope has rails both above and
    /// below: the level one wins.
    fn get_rail(&self, rails: &Rails, blocks: &dyn Blocks, pos: BlockPos) -> Option<Self> {
        for p in [pos, above(pos), below(pos)] {
            let state = blocks.state_at(p);
            if (rails.is_rail)(&state) {
                return Some(Self::new(rails, p, &state));
            }
        }
        None
    }

    /// `removeSoftConnections()`, offsets 0-83.
    ///
    /// Drops every connection that is not answered from the other end, and
    /// **rewrites the ones that are** to the neighbour's real position —
    /// which is how a connection recorded at the flat position becomes a
    /// connection to the rail one block up.
    ///
    /// The index arithmetic is vanilla's: on a removal it does
    /// `remove(i); i--` and then the loop's own `i++`, so the index stays
    /// put. Written as a `while` for that reason; a `retain` would visit the
    /// same elements but could not do the rewrite in the same pass.
    fn remove_soft_connections(&mut self, rails: &Rails, blocks: &dyn Blocks) {
        let mut i = 0;
        while i < self.connections.len() {
            match self.get_rail(rails, blocks, self.connections[i]) {
                Some(r) if r.connects_to(self) => {
                    self.connections[i] = r.pos;
                    i += 1;
                }
                _ => {
                    self.connections.remove(i);
                }
            }
        }
    }

    /// `hasNeighborRail(BlockPos)`, offsets 0-21.
    ///
    /// Not a plain "is there a rail there": it fetches the neighbour, makes
    /// **the neighbour** drop its own stale connections, and then asks
    /// whether the neighbour would accept this rail. So the answer depends on
    /// how full the neighbour already is, which is what stops a rail from
    /// bending into a junction that has no room.
    fn has_neighbor_rail(&self, rails: &Rails, blocks: &dyn Blocks, pos: BlockPos) -> bool {
        let Some(mut r) = self.get_rail(rails, blocks, pos) else {
            return false;
        };
        r.remove_soft_connections(rails, blocks);
        r.can_connect_to(self)
    }

    /// The shape a set of four neighbour flags implies, shared by `place`
    /// (offsets 105-201 and 210-288) and `connectTo` (81-216).
    ///
    /// The two call sites compute the same eight booleans from different
    /// sources — `place` from `hasNeighborRail`, `connectTo` from
    /// `hasConnection` — and then run identical logic, so it is written once
    /// with the source passed in.
    fn straight_or_corner(
        n: bool,
        s: bool,
        w: bool,
        e: bool,
        is_straight: bool,
    ) -> Option<RailShape> {
        let north_south = n || s;
        let east_west = w || e;
        let mut shape = None;
        if north_south && !east_west {
            shape = Some(RailShape::NorthSouth);
        }
        if east_west && !north_south {
            shape = Some(RailShape::EastWest);
        }
        if !is_straight {
            // Each corner needs its two sides present and the other two
            // absent. The order is vanilla's and the last one to match wins,
            // which is only observable if two could match at once — they
            // cannot, because each excludes the other's sides.
            if s && e && !n && !w {
                shape = Some(RailShape::SouthEast);
            }
            if s && w && !n && !e {
                shape = Some(RailShape::SouthWest);
            }
            if n && w && !s && !e {
                shape = Some(RailShape::NorthWest);
            }
            if n && e && !s && !w {
                shape = Some(RailShape::NorthEast);
            }
        }
        shape
    }

    /// The two ascending fix-ups, offsets 428-524 of `place` and 218-312 of
    /// `connectTo`.
    ///
    /// A `NORTH_SOUTH` rail with a rail one block up to its north becomes
    /// `ASCENDING_NORTH`; the south test runs **after** and overwrites, so a
    /// rail with raised neighbours on both sides ascends south. Reproduced
    /// rather than turned into an `else`.
    fn ascend(
        rails: &Rails,
        blocks: &dyn Blocks,
        pos: BlockPos,
        shape: Option<RailShape>,
    ) -> Option<RailShape> {
        let [north, south, west, east] = around(pos);
        let mut shape = shape;
        let is_rail_at = |p: BlockPos| (rails.is_rail)(&blocks.state_at(p));
        if shape == Some(RailShape::NorthSouth) {
            if is_rail_at(above(north)) {
                shape = Some(RailShape::AscendingNorth);
            }
            if is_rail_at(above(south)) {
                shape = Some(RailShape::AscendingSouth);
            }
        }
        if shape == Some(RailShape::EastWest) {
            if is_rail_at(above(east)) {
                shape = Some(RailShape::AscendingEast);
            }
            if is_rail_at(above(west)) {
                shape = Some(RailShape::AscendingWest);
            }
        }
        shape
    }

    /// `connectTo(RailState)`, offsets 0-362 — run on the **neighbour**, with
    /// the rail that is being placed as the argument.
    ///
    /// It appends the other rail's position to its own connection list and
    /// then re-derives its whole shape from that list, which is why a rail
    /// placed at right angles bends *both* pieces. The write is
    /// `level.setBlock(pos, state, 3)`.
    fn connect_to(
        &mut self,
        rails: &Rails,
        blocks: &dyn Blocks,
        other: &Self,
        updates: &mut crate::shapes::Effects,
    ) {
        self.connections.push(other.pos);
        let [north, south, west, east] = around(self.pos);
        let shape = Self::straight_or_corner(
            self.has_connection(north),
            self.has_connection(south),
            self.has_connection(west),
            self.has_connection(east),
            self.is_straight,
        );
        let shape = Self::ascend(rails, blocks, self.pos, shape);
        // `if (shape == null) shape = NORTH_SOUTH` — offsets 312-320.
        let shape = shape.unwrap_or(RailShape::NorthSouth);
        self.state = with_property(&self.state, "shape", shape.name());
        crate::shapes::set_block(
            blocks,
            rails.shapes,
            self.pos,
            &self.state,
            crate::shapes::UPDATE_ALL,
            crate::shapes::RECURSION_LIMIT,
            updates,
        );
    }

    /// `place(boolean powered, boolean alwaysConnect, RailShape oldShape)`,
    /// offsets 0-672.
    ///
    /// ```text
    ///    36-66   n/s/w/e = hasNeighborRail(each horizontal)
    ///   105-133  the two straight answers, each needing the other axis empty
    ///   203-288  the four corners, skipped entirely when isStraight
    ///   290-332  nothing decided yet: both axes -> KEEP THE OLD SHAPE,
    ///            one axis -> that axis, neither -> still nothing
    ///   334-423  a second corner pass, ordered by `powered`
    ///   428-524  the ascending fix-ups
    ///   526-532  still nothing -> NORTH_SOUTH
    ///   535-558  updateConnections(shape); state = state.setValue(SHAPE, shape)
    ///   562-596  alwaysConnect || the world disagrees -> setBlock(pos, state, 3)
    ///   600-671  for each connection: getRail, removeSoftConnections,
    ///            and if it canConnectTo us, connectTo us
    /// ```
    ///
    /// # The line that is easy to miss, at 305
    ///
    /// When neighbours exist on **both** axes and no corner matched, the
    /// answer is `aload_3` — the shape the rail already had. Not a default,
    /// not north-south: the old shape. A junction therefore keeps its
    /// orientation instead of snapping, and a port that fell through to the
    /// `NORTH_SOUTH` default at 526 would rotate every crossing.
    ///
    /// # The second corner pass is not the first one again
    ///
    /// Offsets 210-288 require the two *opposite* sides to be empty; offsets
    /// 341-423 do not test them at all and simply take the first corner whose
    /// two sides are present, in an order chosen by `powered`. The first pass
    /// is "an unambiguous corner", the second is "pick one". Collapsing them
    /// loses the tie-break.
    pub fn place(
        mut self,
        rails: &Rails,
        blocks: &dyn Blocks,
        powered: bool,
        always_connect: bool,
        old_shape: Option<RailShape>,
        updates: &mut crate::shapes::Effects,
    ) -> Self {
        let [north, south, west, east] = around(self.pos);
        let n = self.has_neighbor_rail(rails, blocks, north);
        let s = self.has_neighbor_rail(rails, blocks, south);
        let w = self.has_neighbor_rail(rails, blocks, west);
        let e = self.has_neighbor_rail(rails, blocks, east);
        let mut shape = Self::straight_or_corner(n, s, w, e, self.is_straight);
        let north_south = n || s;
        let east_west = w || e;
        if shape.is_none() {
            if north_south && east_west {
                shape = old_shape;
            } else if north_south {
                shape = Some(RailShape::NorthSouth);
            } else if east_west {
                shape = Some(RailShape::EastWest);
            }
        }
        if !self.is_straight {
            let ordered = if powered {
                [
                    (s && e, RailShape::SouthEast),
                    (s && w, RailShape::SouthWest),
                    (n && e, RailShape::NorthEast),
                    (n && w, RailShape::NorthWest),
                ]
            } else {
                [
                    (n && w, RailShape::NorthWest),
                    (n && e, RailShape::NorthEast),
                    (s && w, RailShape::SouthWest),
                    (s && e, RailShape::SouthEast),
                ]
            };
            for (matched, corner) in ordered {
                if matched {
                    shape = Some(corner);
                    break;
                }
            }
        }
        let shape = Self::ascend(rails, blocks, self.pos, shape).unwrap_or(RailShape::NorthSouth);
        self.update_connections(shape);
        self.state = with_property(&self.state, "shape", shape.name());
        if always_connect || blocks.state_at(self.pos) != self.state {
            crate::shapes::set_block(
                blocks,
                rails.shapes,
                self.pos,
                &self.state,
                crate::shapes::UPDATE_ALL,
                crate::shapes::RECURSION_LIMIT,
                updates,
            );
            // The list is walked by index because `connectTo` can append to
            // the *neighbour's* list, never to this one — but the neighbour
            // is a fresh `RailState` each time round, which is vanilla's
            // behaviour and not an optimisation to remove.
            let mut i = 0;
            while i < self.connections.len() {
                if let Some(mut r) = self.get_rail(rails, blocks, self.connections[i]) {
                    r.remove_soft_connections(rails, blocks);
                    if r.can_connect_to(&self) {
                        r.connect_to(rails, blocks, &self, updates);
                    }
                }
                i += 1;
            }
        }
        self
    }
}

/// `BaseRailBlock.onPlace(state, level, pos, oldState, movedByPiston)`,
/// offsets 0-23, and the `updateState`/`updateDir` pair it delegates to.
///
/// ```java
/// if (oldState.is(state.getBlock())) return;              //  0-12
/// updateState(state, level, pos, movedByPiston);          // 13-22
/// // updateState  -> updateDir(level, pos, state, true)
/// // updateDir    -> new RailState(level, pos, state)
/// //                   .place(level.hasNeighborSignal(pos), true, oldShape)
/// ```
///
/// The `alwaysConnect` argument is a literal `true` from `updateState`
/// (offset 4, `iconst_1`), so the write at the end of `place` is
/// unconditional and the rail is always re-broadcast even when its shape did
/// not change. That is deliberate on vanilla's part and is what makes the
/// `240`-style "the block nobody clicked" observation appear for `300` too.
///
/// Returns without doing anything for a block that is not a rail, which is
/// how [`crate::shapes::set_block`] can call it on every write.
pub fn on_place(
    rails: &Rails,
    blocks: &dyn Blocks,
    pos: BlockPos,
    state: &str,
    old_state: &str,
    updates: &mut crate::shapes::Effects,
) {
    if !(rails.is_rail)(state) {
        return;
    }
    // `oldState.is(state.getBlock())` — the same BLOCK, properties ignored.
    // This is the whole termination argument for the recursion; see the
    // module docs.
    let block_of = |s: &str| s.split('[').next().unwrap_or(s).to_string();
    if block_of(old_state) == block_of(state) {
        return;
    }
    let old_shape = property_of(state, "shape").and_then(RailShape::from_name);
    let me = RailState::new(rails, pos, state);
    // `powered` is `level.hasNeighborSignal(pos)`. That function exists now —
    // [`crate::power::Redstone::has_neighbor_signal`] — but the `false` here is
    // still `false`: `BaseRailBlock.onPlace` is not where vanilla reads it, and
    // wiring a powered rail up is the rail's own item rather than a line
    // changed in passing. See the module docs for what that costs and when.
    me.place(rails, blocks, false, true, old_shape, updates);
}

/// Port 4 of 5, and the only one that needed a hook to exist first.
///
/// Every corner assertion is paired with its mirror, for the reason the
/// ladder's red established: a rule that chooses one of four corners cannot
/// be told apart from a constant by one input.
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    /// A world of state strings. Ids are handed out in first-seen order,
    /// which is all `set_block`'s return value is used for here.
    #[derive(Default)]
    struct Fake {
        blocks: Mutex<BTreeMap<BlockPos, String>>,
        ids: Mutex<BTreeMap<String, i32>>,
    }

    impl Fake {
        fn with(states: &[(BlockPos, &str)]) -> Self {
            let f = Self::default();
            for (p, s) in states {
                f.blocks.lock().expect("fake").insert(*p, (*s).to_string());
                f.id(s);
            }
            f
        }
        fn id(&self, state: &str) -> i32 {
            let mut ids = self.ids.lock().expect("ids");
            let n = i32::try_from(ids.len()).expect("small");
            *ids.entry(state.to_string()).or_insert(n)
        }
    }

    impl Blocks for Fake {
        fn state_at(&self, pos: BlockPos) -> String {
            self.blocks
                .lock()
                .expect("fake")
                .get(&pos)
                .cloned()
                .unwrap_or_else(|| "minecraft:air".to_string())
        }
        fn set_block(&self, pos: BlockPos, state: &str) -> Option<i32> {
            if self.state_at(pos) == state {
                return None;
            }
            self.blocks
                .lock()
                .expect("fake")
                .insert(pos, state.to_string());
            Some(self.id(state))
        }
        fn default_state(&self, b: &str) -> Option<String> {
            Some(b.to_string())
        }
        fn has_state(&self, _: &str) -> bool {
            true
        }
        fn face_sturdy(&self, pos: BlockPos, _: Face) -> bool {
            self.state_at(pos) != "minecraft:air"
        }
        /// A test double. `false` is the honest answer for a world of air
        /// and stone that no test asks a redstone question of; the moment one
        /// does, it overrides this rather than inheriting a default that
        /// cannot be seen.
        fn redstone_conductor(&self, _: BlockPos) -> bool {
            false
        }
        fn signal_source(&self, _: BlockPos) -> bool {
            false
        }
        fn state_id(&self, _: &str) -> Option<i32> {
            Some(1)
        }
        fn replaceable(&self, pos: BlockPos, _: &crate::placement::Replacing<'_>) -> bool {
            crate::shapes::is_air(&self.state_at(pos))
        }
        fn legacy_fluid_at(&self, _: BlockPos) -> String {
            "minecraft:air".to_string()
        }
        fn water_at(&self, _: BlockPos) -> bool {
            false
        }
    }

    fn rail(shape: &str) -> String {
        format!("minecraft:rail[shape={shape},waterlogged=false]")
    }

    fn powered(shape: &str) -> String {
        format!("minecraft:powered_rail[powered=false,shape={shape},waterlogged=false]")
    }

    fn is_rail(s: &str) -> bool {
        s.starts_with("minecraft:rail[") || s.starts_with("minecraft:powered_rail[")
    }

    /// Only `RailBlock` is curved; `PoweredRailBlock` and friends are
    /// straight.
    fn is_straight(s: &str) -> bool {
        s.starts_with("minecraft:powered_rail[")
    }

    /// Place a rail and run `onPlace`, returning the world.
    fn place_into(world: &Fake, pos: BlockPos, state: &str) -> crate::shapes::Effects {
        let rails = Rails {
            is_rail: &is_rail,
            is_straight: &is_straight,
            shapes: None,
        };
        let mut updates = crate::shapes::Effects::new();
        let old = world.state_at(pos);
        world.set_block(pos, state);
        on_place(&rails, world, pos, state, &old, &mut updates);
        updates
    }

    fn shape_at(world: &Fake, pos: BlockPos) -> String {
        property_of(&world.state_at(pos), "shape")
            .unwrap_or("none")
            .to_string()
    }

    /// **Check `300`'s scenario, exactly.** Three rails: two in a line along
    /// x, then a third south of the eastern one. The middle rail — which
    /// nobody clicked, and which was `east_west` a moment ago — must bend
    /// into `south_west`.
    ///
    /// Yaw 0 is `getHorizontalDirection() == SOUTH`, so every rail is placed
    /// as `north_south` and the shapes below are all `onPlace`'s work.
    #[test]
    fn a_third_rail_at_right_angles_bends_the_one_it_meets() {
        let world = Fake::with(&[]);
        place_into(&world, (0, 0, 0), &rail("north_south"));
        place_into(&world, (1, 0, 0), &rail("north_south"));
        assert_eq!(shape_at(&world, (0, 0, 0)), "east_west", "two in a line");
        assert_eq!(shape_at(&world, (1, 0, 0)), "east_west");

        let updates = place_into(&world, (1, 0, 1), &rail("north_south"));
        assert_eq!(
            shape_at(&world, (1, 0, 0)),
            "south_west",
            "the rail nobody clicked joins west to its old partner and south to the new one"
        );
        assert_eq!(shape_at(&world, (1, 0, 1)), "north_south");
        assert!(
            updates.blocks.iter().any(|(p, _)| *p == (1, 0, 0)),
            "the bend has to be broadcast"
        );
    }

    /// **The mirror.** Same two rails, third one to the NORTH instead, so the
    /// bend is `north_west`. A port that answered a constant corner, or that
    /// ran the four corner tests in the wrong order, passes the test above
    /// and fails this one.
    #[test]
    fn the_bend_follows_which_side_the_third_rail_is_on() {
        let world = Fake::with(&[]);
        place_into(&world, (0, 0, 0), &rail("north_south"));
        place_into(&world, (1, 0, 0), &rail("north_south"));
        place_into(&world, (1, 0, -1), &rail("north_south"));
        assert_eq!(shape_at(&world, (1, 0, 0)), "north_west");
    }

    /// **The degenerate control.** Two rails in a line have no corner to get
    /// wrong: `east_west` on both. It passes under every corner-order bug,
    /// under a swapped `can_connect_to`, and under a `has_connection` that
    /// compared the y as well — which is why the three tests around it exist.
    #[test]
    fn two_rails_in_a_line_are_straight() {
        let world = Fake::with(&[]);
        place_into(&world, (0, 0, 0), &rail("north_south"));
        place_into(&world, (1, 0, 0), &rail("north_south"));
        assert_eq!(shape_at(&world, (0, 0, 0)), "east_west");
        assert_eq!(shape_at(&world, (1, 0, 0)), "east_west");
    }

    /// **The slope, which is where `hasConnection`'s missing y matters.**
    ///
    /// A rail placed west of a rail sitting one block higher must climb
    /// towards it — `getRail` searches the position, then one above, then one
    /// below (offsets 0-103), and `place`'s ascending fix-up at 428-524 turns
    /// the `EAST_WEST` answer into `ASCENDING_EAST`.
    ///
    /// Its own degenerate is directly below it: the same placement with the
    /// neighbour on the level stays `east_west`.
    #[test]
    fn a_rail_climbs_towards_a_rail_one_block_up() {
        let world = Fake::with(&[((1, 1, 0), &rail("east_west"))]);
        place_into(&world, (0, 0, 0), &rail("north_south"));
        assert_eq!(shape_at(&world, (0, 0, 0)), "ascending_east");
    }

    #[test]
    fn a_rail_beside_a_level_rail_does_not_climb() {
        let world = Fake::with(&[((1, 0, 0), &rail("east_west"))]);
        place_into(&world, (0, 0, 0), &rail("north_south"));
        assert_eq!(shape_at(&world, (0, 0, 0)), "east_west");
    }

    /// **`isStraight` is a real branch, not a label.** The same L that bends
    /// a plain rail leaves a powered rail straight, because both corner
    /// passes are behind `if (!isStraight)` at offsets 203-207 and 334-338.
    ///
    /// This is the paired opposite of the two corner tests: identical
    /// geometry, one property different, and the answer must not be a corner.
    #[test]
    fn a_powered_rail_never_takes_a_corner() {
        let world = Fake::with(&[]);
        place_into(&world, (0, 0, 0), &powered("north_south"));
        place_into(&world, (1, 0, 0), &powered("north_south"));
        place_into(&world, (1, 0, 1), &powered("north_south"));
        let bent = shape_at(&world, (1, 0, 0));
        assert!(
            bent == "east_west" || bent == "north_south",
            "a powered rail has no corner to take, got {bent}"
        );
    }

    /// A lone rail keeps the shape its placement gave it. The `null` fall
    /// through at offsets 526-532 is `NORTH_SOUTH`, which is also what yaw 0
    /// placed, so this pins that `onPlace` does not invent a shape when there
    /// is nothing to connect to.
    #[test]
    fn a_lone_rail_is_left_alone() {
        let world = Fake::with(&[]);
        let updates = place_into(&world, (0, 0, 0), &rail("north_south"));
        assert_eq!(shape_at(&world, (0, 0, 0)), "north_south");
        assert!(
            !updates.blocks.iter().any(|(p, _)| *p == (0, 0, 0)),
            "nothing changed, so onPlace writes nothing"
        );
    }

    /// **`place`'s SECOND corner pass, which none of the tests above reach.**
    ///
    /// This was found by injecting bugs and watching every rail test stay
    /// green. The L-shaped scenarios bend their middle rail inside
    /// `connectTo`, which runs only the *first* corner pass — the one that
    /// needs the two opposite sides empty. `place`'s second pass, at offsets
    /// 341-423, is reached only when a rail is placed with neighbours on
    /// **both** axes and no unambiguous corner, and nothing in the corpus
    /// scenario does that.
    ///
    /// Three neighbours: north, south and west. Pass one finds nothing —
    /// every corner it tests requires the opposite two sides to be empty and
    /// they are not — so pass two picks the first pair in its unpowered
    /// order, `NORTH_WEST`.
    ///
    /// **Paired with the east version below**, because a pass that always
    /// answered `NORTH_WEST` would satisfy this one alone.
    #[test]
    fn a_rail_placed_into_a_three_way_junction_picks_the_first_corner() {
        let world = Fake::with(&[
            ((0, 0, -1), &rail("north_south")),
            ((0, 0, 1), &rail("north_south")),
            ((-1, 0, 0), &rail("east_west")),
        ]);
        place_into(&world, (0, 0, 0), &rail("north_south"));
        assert_eq!(shape_at(&world, (0, 0, 0)), "north_west");
    }

    #[test]
    fn the_three_way_junction_answer_follows_which_side_is_occupied() {
        let world = Fake::with(&[
            ((0, 0, -1), &rail("north_south")),
            ((0, 0, 1), &rail("north_south")),
            ((1, 0, 0), &rail("east_west")),
        ]);
        place_into(&world, (0, 0, 0), &rail("north_south"));
        assert_eq!(shape_at(&world, (0, 0, 0)), "north_east");
    }

    /// The same junction with a **straight** rail: both corner passes are
    /// behind `if (!isStraight)`, so offsets 290-332 have already set the
    /// shape to the one it arrived with and nothing overwrites it.
    ///
    /// This is the test that fails when the guard on the *second* pass is
    /// dropped. `a_powered_rail_never_takes_a_corner` above does not: its
    /// bend goes through `connectTo`, whose guard is a different one.
    #[test]
    fn a_straight_rail_in_a_three_way_junction_keeps_its_shape() {
        let world = Fake::with(&[
            ((0, 0, -1), &powered("north_south")),
            ((0, 0, 1), &powered("north_south")),
            ((-1, 0, 0), &powered("east_west")),
        ]);
        place_into(&world, (0, 0, 0), &powered("north_south"));
        assert_eq!(
            shape_at(&world, (0, 0, 0)),
            "north_south",
            "both axes are occupied, so offset 305 keeps the old shape"
        );
    }

    /// **`canConnectTo`'s `||`, which needs a rail that is already full.**
    ///
    /// Three rails in a line, then a fourth placed south of the middle one.
    /// The middle already has two connections, so the `connections.size() !=
    /// 2` arm is false and only `!connectsTo(other)` can carry it — and it
    /// does, because the middle is not yet pointing south. With `&&` in place
    /// of `||` the middle refuses and stays `east_west`.
    ///
    /// None of the L-shaped tests reach this: their middle rail has one
    /// connection, so `size() != 2` is true and the `||` short-circuits
    /// whatever the other arm says.
    #[test]
    fn a_full_rail_still_accepts_a_neighbour_it_is_not_pointing_at() {
        let world = Fake::with(&[]);
        place_into(&world, (-1, 0, 0), &rail("north_south"));
        place_into(&world, (0, 0, 0), &rail("north_south"));
        place_into(&world, (1, 0, 0), &rail("north_south"));
        assert_eq!(shape_at(&world, (0, 0, 0)), "east_west", "a full middle");
        place_into(&world, (0, 0, 1), &rail("north_south"));
        assert_ne!(
            shape_at(&world, (0, 0, 0)),
            "east_west",
            "the middle has to re-derive its shape from three connections"
        );
    }

    /// **`hasConnection` ignores the y, and only a slope can tell.**
    ///
    /// An `ASCENDING_EAST` rail at the origin connects to `east.above()`. The
    /// rail actually sitting there is `ASCENDING_WEST`, whose own connections
    /// are `west.above()` and `east` — and `west.above()` is `(0, 2, 0)`,
    /// two blocks above the origin. The connection is mutual **only because
    /// the y is not compared**: on x and z the two agree exactly.
    ///
    /// So `removeSoftConnections` must keep it, and rewrite it to the
    /// neighbour's real position. Comparing the whole `BlockPos` — the
    /// natural way to write this — drops every slope in the game, and every
    /// flat test in this module passes while it does.
    #[test]
    fn a_slope_connection_survives_because_the_y_is_not_compared() {
        let world = Fake::with(&[
            ((0, 0, 0), &rail("ascending_east")),
            ((1, 1, 0), &rail("ascending_west")),
        ]);
        let rails = Rails {
            is_rail: &is_rail,
            is_straight: &is_straight,
            shapes: None,
        };
        let mut me = RailState::new(&rails, (0, 0, 0), &rail("ascending_east"));
        assert_eq!(me.connections, vec![(-1, 0, 0), (1, 1, 0)]);
        me.remove_soft_connections(&rails, &world);
        assert_eq!(
            me.connections,
            vec![(1, 1, 0)],
            "the uphill neighbour answers back; the westward one is empty air"
        );
    }

    /// `onPlace` returns immediately when the block was already the same one
    /// — offsets 0-12 — and that early return is the whole reason the
    /// recursion in `place` terminates.
    #[test]
    fn replacing_a_rail_with_a_rail_does_not_re_enter() {
        let world = Fake::with(&[((0, 0, 0), &rail("east_west"))]);
        let rails = Rails {
            is_rail: &is_rail,
            is_straight: &is_straight,
            shapes: None,
        };
        let mut updates = crate::shapes::Effects::new();
        on_place(
            &rails,
            &world,
            (0, 0, 0),
            &rail("north_south"),
            &rail("east_west"),
            &mut updates,
        );
        assert!(
            updates.blocks.is_empty(),
            "the same block short-circuits onPlace"
        );
    }
}
