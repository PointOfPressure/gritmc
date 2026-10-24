//! `RedStoneWireBlock`'s connection rule — the one shape rule in the game a
//! table of neighbour states cannot express.
//!
//! # Why this is the port that refuted the family's specification
//!
//! The neighbour-aware family was specified on 2026-08-05 as needing "a
//! `BlockGetter` over the six orthogonal neighbours", read off
//! `update_shape.json`'s `refused` list. That list is a **lower bound**:
//! `ShapeGolden`'s probe throws on the first read outside the two positions it
//! serves, so each bucket records the first offending read and nothing the
//! method would have gone on to do. This rule is the counterexample —
//! `getConnectingSide` reads two diagonals per horizontal direction:
//!
//! ```text
//!     0-5    npos = pos.relative(dir)
//!     7-15   getBlockState(npos)             orthogonal
//!    57-63   getBlockState(npos.above())     DIAGONAL
//!   117-123  getBlockState(npos.below())     DIAGONAL
//! ```
//!
//! **Access shape: thirteen positions, eight of them diagonal** — `pos.above()`
//! from the three-argument overload, and for each of the four horizontals
//! `pos.relative(d)`, `pos.relative(d).above()` and `pos.relative(d).below()`.
//!
//! # What reaching the wire took, and why it was three commits and not one
//!
//! `450-redstone-wire-climbs-the-block-beside-it` places a block **diagonally**
//! from the wire it expects to change. No six-face cascade reaches that
//! position, and `crate::shapes::set_block` walked only the six. Making that
//! check answer needed **three** separate things, and the first commit landed
//! only the first of them:
//!
//!   1. the connection rule — [`connection_state`] and everything under it,
//!      landed 2026-08-05 with the module doc below;
//!   2. `RedStoneWireBlock.updateShape` — [`update_shape`], the wire's own
//!      answer to "a neighbour changed". **Without it a notification arriving
//!      at a wire is discarded**: `update_shape.json`'s 1,296 wire rows are all
//!      `dir=DOWN -> air`, and neither the typed hook nor the stair hook knows
//!      the block. This is the half the previous link listed as a smaller
//!      second item; it is not smaller and it is not optional, because a
//!      cascade that nothing answers is inert;
//!   3. `updateIndirectNeighbourShapes` — the diagonal walk, in
//!      [`crate::shapes::update_indirect_neighbour_shapes`], called twice by
//!      `set_block`.
//!
//! Landing the rule first was deliberate. The check's first difference is a
//! connection shape at `power=0` on both sides, which means a wrong predicate
//! produces a plausible-looking change that reads as progress — so the
//! predicates came from a regenerated `block_props.json` before any of this was
//! written, and the two halves that let a green appear came after.
//!
//! # The two predicates, and why neither could be guessed
//!
//! `isRedstoneConductor` is a per-block `StatePredicate` on
//! `BlockBehaviour.Properties`; `block_props.json` gained a column for it on
//! 2026-08-05 and the table proves it was needed — `minecraft:glass` is full
//! on every collision face and is not a conductor.
//!
//! `shouldConnectTo` needs only `isSignalSource` from a table. Its other three
//! cases are block identity plus the neighbour's own `facing`:
//!
//! ```java
//! if (state.is(REDSTONE_WIRE)) return true;                //  0-11
//! if (state.is(REPEATER))                                  // 12-19
//!     return FACING == dir || FACING.getOpposite() == dir; // 22-50
//! if (state.is(OBSERVER)) return dir == FACING;            // 52-77
//! return state.isSignalSource() && dir != null;            // 79-95
//! ```
//!
//! A comparator has **no** facing rule here and reaches the last line like any
//! other source, which is the opposite of what the briefing for this port said.

use crate::interact::{BlockPos, Face};
use crate::placement::{property_of, with_property, Around};

/// `RedstoneSide`, the value each of the four horizontal properties takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedstoneSide {
    None,
    Side,
    Up,
}

impl RedstoneSide {
    /// `RedstoneSide.isConnected()` — everything but `NONE`.
    #[must_use]
    pub const fn is_connected(self) -> bool {
        !matches!(self, Self::None)
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Side => "side",
            Self::Up => "up",
        }
    }
}

/// The four horizontals in `Direction.Plane.HORIZONTAL`'s own order.
///
/// `Plane.HORIZONTAL` is built from `new Direction[]{NORTH, EAST, SOUTH,
/// WEST}`, not from `Direction.values()`. Every loop below sets one property
/// per direction independently, so the order cannot change the answer — it is
/// copied anyway, because the next rule that iterates this plane may not be
/// order-free and finding that out twice is the expensive way.
pub use crate::interact::HORIZONTAL;

fn block_of(state: &str) -> &str {
    state.split('[').next().unwrap_or(state)
}

/// `RedStoneWireBlock.canSurviveOn(BlockGetter, BlockPos, BlockState)`,
/// offsets 0-27.
///
/// ```java
/// return state.isFaceSturdy(level, pos, Direction.UP) || state.is(HOPPER);
/// ```
///
/// The hopper is not an oversight to tidy away: its top face is not sturdy and
/// dust sits on it anyway, and it is the only block in the game with that
/// exemption.
///
/// The state is passed rather than read from `pos`, because
/// [`update_shape`]'s `DOWN` branch hands it the *neighbour state argument*
/// (offsets 8-17, `aload 6`/`aload 7`) and not a fresh read. The two agree on
/// every path this server has, but a rule that re-reads the world where
/// vanilla uses a value it was given is a different rule, and this is the one
/// place the distinction could ever bite.
///
/// **Only the hopper half of the `||` honours it, and that is a gap, not a
/// choice.** Vanilla's first term is `state.isFaceSturdy(level, pos, UP)` — a
/// method on the *passed* state — whereas [`Around::sturdy`] is keyed on
/// position alone and goes back to the world. Nothing can currently make the
/// two disagree, because both callers read the state at that position
/// immediately before calling, so this costs nothing today; it is written down
/// because the day something passes a stale state it will be silent.
fn can_survive_on(around: &Around, pos: BlockPos, state: &str) -> bool {
    (around.sturdy)(pos, Face::Up) || block_of(state) == "minecraft:hopper"
}

/// `RedStoneWireBlock.shouldConnectTo(BlockState, Direction)`, offsets 0-95.
///
/// `dir` is `None` for the one-argument overload, which passes a literal
/// `null` — and that `null` is load-bearing at offset 86, where the
/// fall-through is `isSignalSource() && dir != null`. A signal source is
/// therefore **not** connected to by the diagonal probes, which call the
/// one-argument form, but **is** connected to sideways. Collapsing the two
/// overloads loses exactly that.
fn should_connect_to(around: &Around, pos: BlockPos, dir: Option<Face>) -> bool {
    let state = (around.at)(pos);
    match block_of(&state) {
        "minecraft:redstone_wire" => true,
        "minecraft:repeater" => {
            let Some(facing) = property_of(&state, "facing").and_then(Face::from_name) else {
                return false;
            };
            dir.is_some_and(|d| facing == d || facing.opposite() == d)
        }
        "minecraft:observer" => {
            let Some(facing) = property_of(&state, "facing").and_then(Face::from_name) else {
                return false;
            };
            dir == Some(facing)
        }
        _ => (around.source)(pos) && dir.is_some(),
    }
}

/// `RedStoneWireBlock.getConnectingSide(BlockGetter, BlockPos, Direction,
/// boolean)`, offsets 0-141.
///
/// ```java
/// BlockPos   npos = pos.relative(dir);                        //   0-5
/// BlockState n    = level.getBlockState(npos);                //   7-15
/// if (canGoUp                                                 //  17, ifeq 97
///     && (n.getBlock() instanceof TrapDoorBlock               //  22-30
///         || canSurviveOn(level, npos, n))                    //  33-42
///     && shouldConnectTo(level.getBlockState(npos.above()))) {//  57-71
///     return n.isFaceSturdy(level, npos, dir.getOpposite())   //  74-83
///          ? RedstoneSide.UP : RedstoneSide.SIDE;             //  89 / 93
/// }
/// if (shouldConnectTo(n, dir))            return SIDE;        //  97-103 -> 134
/// if (n.isRedstoneConductor(level, npos)) return NONE;        // 106-114 -> 138
/// if (shouldConnectTo(level.getBlockState(npos.below())))     // 117-131
///                                         return SIDE;        //         -> 134
///                                         return NONE;        //            138
/// ```
///
/// # The trapdoor is an `||`, not a special case to skip
///
/// Offsets 22-30 accept a `TrapDoorBlock` **instead of** the sturdy-face test,
/// so dust climbs a trapdoor whose top face is not sturdy. Reproduced by name
/// rather than by class because the class map would need a fifth set for one
/// block; noted so the next reader knows it was a choice.
///
/// # The last three tests are an if/else chain and their order is the rule
///
/// A neighbour that is *both* a signal source and a conductor connects
/// sideways, because `shouldConnectTo` is asked first. Reordering them makes
/// a redstone block behave like stone.
fn connecting_side(around: &Around, pos: BlockPos, dir: Face, can_go_up: bool) -> RedstoneSide {
    let npos = dir.relative(pos);
    let n = (around.at)(npos);
    let is_trapdoor = block_of(&n).ends_with("_trapdoor");
    if can_go_up
        && (is_trapdoor || can_survive_on(around, npos, &n))
        && should_connect_to(around, above(npos), None)
    {
        return if (around.sturdy)(npos, dir.opposite()) {
            RedstoneSide::Up
        } else {
            RedstoneSide::Side
        };
    }
    if should_connect_to(around, npos, Some(dir)) {
        return RedstoneSide::Side;
    }
    if (around.conductor)(npos) {
        return RedstoneSide::None;
    }
    if should_connect_to(around, below(npos), None) {
        return RedstoneSide::Side;
    }
    RedstoneSide::None
}

const fn above((x, y, z): BlockPos) -> BlockPos {
    (x, y + 1, z)
}

const fn below((x, y, z): BlockPos) -> BlockPos {
    (x, y - 1, z)
}

/// `RedStoneWireBlock.getMissingConnections(BlockGetter, BlockState,
/// BlockPos)`, offsets 0-120.
///
/// ```java
/// boolean canGoUp = !level.getBlockState(pos.above())
///                         .isRedstoneConductor(level, pos);   // 0-23
/// for (Direction d : Plane.HORIZONTAL)
///     if (!state.getValue(PROPERTY_BY_DIRECTION.get(d)).isConnected())
///         state = state.setValue(..., getConnectingSide(level, pos, d, canGoUp));
/// ```
///
/// **The `isRedstoneConductor` call is passed `pos`, not `pos.above()`** —
/// offsets 10-12 are `aload_1` then `aload_3`. The state comes from one block
/// up and the position from where the wire is. Position-independent for us
/// because the column is per state, so it changes nothing here; written down
/// because it looks like a transcription slip and is vanilla's.
fn missing_connections(around: &Around, pos: BlockPos, state: &str) -> String {
    let can_go_up = !(around.conductor)(above(pos));
    let mut state = state.to_string();
    for d in HORIZONTAL {
        let name = face_property(d);
        let already = property_of(&state, name).is_some_and(|v| v != "none");
        if !already {
            let side = connecting_side(around, pos, d, can_go_up);
            state = with_property(&state, name, side.name());
        }
    }
    state
}

/// The wire's property name for a horizontal direction — vanilla's
/// `PROPERTY_BY_DIRECTION`.
const fn face_property(f: Face) -> &'static str {
    match f {
        Face::North => "north",
        Face::South => "south",
        Face::West => "west",
        Face::East => "east",
        // Not in `PROPERTY_BY_DIRECTION` at all; the loops above never reach
        // it and a lookup that did would be a bug, so it names itself.
        Face::Up | Face::Down => "north",
    }
}

/// `isDot(BlockState)`, offsets 0-69 — none of the four sides connected.
fn is_dot(state: &str) -> bool {
    HORIZONTAL
        .into_iter()
        .all(|d| property_of(state, face_property(d)).is_none_or(|v| v == "none"))
}

/// `RedStoneWireBlock.getConnectionState(BlockGetter, BlockState, BlockPos)`,
/// offsets 0-241 — and `getStateForPlacement` is one line of it.
///
/// ```java
/// boolean wasDot = isDot(state);                              //   0-4
/// state = getMissingConnections(level,                        //   6-35
///           defaultBlockState().setValue(POWER, state.POWER), pos);
/// if (wasDot && isDot(state)) return state;                   //  36-49
/// boolean n = state.NORTH.isConnected();  // ... s, e, w      //  50-108
/// boolean noNS = !n && !s;                                    // 110-125
/// boolean noEW = !e && !w;                                    // 127-142
/// if (!w && noNS) state = state.setValue(WEST,  SIDE);        // 144-167
/// if (!e && noNS) state = state.setValue(EAST,  SIDE);        // 168-191
/// if (!n && noEW) state = state.setValue(NORTH, SIDE);        // 192-215
/// if (!s && noEW) state = state.setValue(SOUTH, SIDE);        // 216-239
/// return state;                                               //     240
/// ```
///
/// # The four `if`s at the end are what makes a lone wire a cross
///
/// A dust with one connection to the north has `noEW` true, so east and west
/// are forced to `SIDE` and the result is a T rather than a stub. A dust with
/// none of the four keeps being a dot, which is what the early return at 36-49
/// preserves: **only** a state that was already a dot is allowed to stay one.
/// Drop that guard and every isolated dust becomes a cross.
///
/// The `POWER` carried across is the *old* state's, not the default's — this
/// function never computes power, and porting it as if it did would put a
/// redstone signal in a placement rule.
#[must_use]
pub fn connection_state(around: &Around, pos: BlockPos, state: &str) -> String {
    let was_dot = is_dot(state);
    let power = property_of(state, "power").unwrap_or("0").to_string();
    // `defaultBlockState().setValue(POWER, state.getValue(POWER))` — every
    // side starts at `none`, which is what makes `getMissingConnections`
    // compute all four.
    let base =
        format!("minecraft:redstone_wire[east=none,north=none,power={power},south=none,west=none]");
    let state = missing_connections(around, pos, &base);
    if was_dot && is_dot(&state) {
        return state;
    }
    let connected =
        |s: &str, d: Face| property_of(s, face_property(d)).is_some_and(|v| v != "none");
    let (n, s_, e, w) = (
        connected(&state, Face::North),
        connected(&state, Face::South),
        connected(&state, Face::East),
        connected(&state, Face::West),
    );
    let no_ns = !n && !s_;
    let no_ew = !e && !w;
    let mut state = state;
    if !w && no_ns {
        state = with_property(&state, "west", "side");
    }
    if !e && no_ns {
        state = with_property(&state, "east", "side");
    }
    if !n && no_ew {
        state = with_property(&state, "north", "side");
    }
    if !s_ && no_ew {
        state = with_property(&state, "south", "side");
    }
    state
}

/// `RedStoneWireBlock.getStateForPlacement(BlockPlaceContext)`, offsets 0-12 —
/// literally `getConnectionState(level, defaultBlockState(), clickedPos)`.
#[must_use]
pub fn placement(around: &Around, pos: BlockPos, default: &str) -> String {
    connection_state(around, pos, default)
}

/// Whether a state is `minecraft:redstone_wire`.
///
/// Written as a name rather than resolved from the class map like
/// [`crate::shapes::ShapeTable`]'s five sets, because `RedStoneWireBlock` has
/// exactly one instance in the registry — a set would have one member and
/// would only add a way for it to be empty. [`should_connect_to`] already
/// matches the same literal.
#[must_use]
pub fn is_wire(state: &str) -> bool {
    block_of(state) == "minecraft:redstone_wire"
}

/// `state.getValue(PROPERTY_BY_DIRECTION.get(d)).isConnected()` — one side of a
/// wire, by direction.
///
/// A missing property reads as **not** connected, which is the safe direction
/// here: the one caller outside this module is
/// [`crate::shapes::update_indirect_neighbour_shapes`], whose gate is
/// `!= NONE`, and a state with no such property is not a wire at all.
#[must_use]
pub fn connects(state: &str, d: Face) -> bool {
    property_of(state, face_property(d)).is_some_and(|v| v != "none")
}

/// `RedStoneWireBlock.isCross(BlockState)`, offsets 0-69 — all four sides
/// connected, which is `&&` and not "three or more".
fn is_cross(state: &str) -> bool {
    HORIZONTAL.into_iter().all(|d| connects(state, d))
}

/// The `crossState` field, built once in the constructor at offsets 107-158 as
/// `defaultBlockState()` with all four sides `SIDE`, and carrying the subject
/// state's `POWER` because [`update_shape`] sets it at offsets 124-140.
///
/// It is **not** `getConnectionState`'s `base`: that one starts every side at
/// `none`, this one starts every side at `side`. Swapping them makes the
/// re-derivation below start from a dot and lose every connection the wire
/// already had.
fn cross_state(power: &str) -> String {
    format!("minecraft:redstone_wire[east=side,north=side,power={power},south=side,west=side]")
}

/// `RedStoneWireBlock.getConnectingSide(BlockGetter, BlockPos, Direction)` —
/// the **three**-argument overload, offsets 0-30.
///
/// ```java
/// return getConnectingSide(level, pos, dir,
///     !level.getBlockState(pos.above()).isRedstoneConductor(level, pos));
/// ```
///
/// Same `pos`-not-`pos.above()` quirk in the `isRedstoneConductor` argument as
/// [`missing_connections`] — see its doc. This overload exists so
/// [`update_shape`] can be called without a `canGoUp` in hand; the four-argument
/// form is the one [`missing_connections`] uses, which computes `canGoUp` once
/// for all four directions instead of four times.
fn connecting_side_here(around: &Around, pos: BlockPos, dir: Face) -> RedstoneSide {
    connecting_side(around, pos, dir, !(around.conductor)(above(pos)))
}

/// `RedStoneWireBlock.updateShape(state, level, tickAccess, pos, direction,
/// neighbourPos, neighbourState, random)`, offsets 0-169 — **the whole of it**,
/// including the `isCross`/`crossState` fallback at 118-169 that was left
/// untranscribed by the previous link.
///
/// ```java
/// if (direction == DOWN)                                        //   0-5
///     return canSurviveOn(level, neighbourPos, neighbourState)  //   8-17
///          ? state : AIR.defaultBlockState();                   //  27 / 20-26
/// if (direction == UP)                                          //  29-34
///     return getConnectionState(level, state, pos);             //  37-45
/// RedstoneSide side = getConnectingSide(level, pos, direction); //  46-55
/// if (side.isConnected() ==                                     //  57-85
///         state.getValue(PROPERTY_BY_DIRECTION.get(direction)).isConnected()
///     && !isCross(state))                                       //  88-92
///     return state.setValue(PROPERTY_BY_DIRECTION.get(direction), side); // 95-117
/// return getConnectionState(level,                              // 118-169
///     crossState.setValue(POWER, state.getValue(POWER))
///               .setValue(PROPERTY_BY_DIRECTION.get(direction), side),
///     pos);
/// ```
///
/// # Why this is a level hook and not a table row, when the table has 1,296 of them
///
/// `update_shape.json` carries 1,296 `redstone_wire` subject rows and **every
/// one is `dir=DOWN` with `to=minecraft:air`** — the first branch above, and
/// only its destroying half. The extractor drops the rest as `connective`,
/// because the answer to a horizontal update is a function of eight positions
/// the probe never serves. So the table is not wrong here, it is partial in a
/// way that reads as silence, and this hook runs *after* it precisely so the
/// measured rows keep winning where they exist.
///
/// # The middle branch is the one that looks redundant and is not
///
/// `side.isConnected() == current.isConnected()` is a test on the **boolean**,
/// not on the value, so a side going `SIDE -> UP` satisfies it and is written
/// straight in. What it excludes is a side that gained or lost a connection
/// outright, which can change whether the wire is a cross and therefore has to
/// go the long way round through `getConnectionState`. `!isCross(state)` is the
/// second guard on the same thing from the other side: a wire that is *already*
/// a cross must re-derive, because the four forced `SIDE`s of a cross are not
/// connections it chose and one of them may now be wrong.
///
/// Returns `None` where vanilla returns the state unchanged — the convention
/// [`crate::shapes::ShapeTable::level_update_shape`] uses, and what
/// `Block.updateOrDestroy`'s `newState == state` early return does with it.
#[must_use]
pub fn update_shape(
    around: &Around,
    pos: BlockPos,
    state: &str,
    direction: Face,
    neighbour_pos: BlockPos,
    neighbour_state: &str,
) -> Option<String> {
    if direction == Face::Down {
        return (!can_survive_on(around, neighbour_pos, neighbour_state))
            .then(|| "minecraft:air".to_string());
    }
    if direction == Face::Up {
        let to = connection_state(around, pos, state);
        return (to != state).then_some(to);
    }
    let side = connecting_side_here(around, pos, direction);
    let name = face_property(direction);
    let currently = property_of(state, name).is_some_and(|v| v != "none");
    let to = if side.is_connected() == currently && !is_cross(state) {
        with_property(state, name, side.name())
    } else {
        let power = property_of(state, "power").unwrap_or("0");
        let base = with_property(&cross_state(power), name, side.name());
        connection_state(around, pos, &base)
    };
    (to != state).then_some(to)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world(states: &[(BlockPos, &str)]) -> Vec<(BlockPos, String)> {
        states.iter().map(|(p, s)| (*p, (*s).to_string())).collect()
    }

    /// A world where `sturdy`, `conductor` and `source` are decided by block
    /// name, so a test can build a conductor that is not a source and the
    /// other way round — which is the pair the rule turns on.
    fn with<R>(states: &[(BlockPos, &str)], f: impl FnOnce(&Around) -> R) -> R {
        let w = world(states);
        let at = |p: BlockPos| -> String {
            w.iter()
                .find(|(q, _)| *q == p)
                .map_or_else(|| "minecraft:air".to_string(), |(_, s)| s.clone())
        };
        // `glass` is the block that separates the two questions: full on
        // every collision face, and not a redstone conductor. It is why
        // `block_props.json` needed a column of its own.
        let solid = |p: BlockPos| {
            matches!(
                block_of(&at(p)),
                "minecraft:stone" | "minecraft:redstone_block" | "minecraft:glass"
            )
        };
        let sturdy = |p: BlockPos, _f: Face| solid(p);
        // `redstone_block` is a full block and NOT a conductor — measured, and
        // the reason this table needed its own column.
        let conductor = |p: BlockPos| block_of(&at(p)) == "minecraft:stone";
        let source = |p: BlockPos| {
            matches!(
                block_of(&at(p)),
                "minecraft:redstone_block" | "minecraft:lever" | "minecraft:comparator"
            )
        };
        let around = Around {
            pos: (0, 0, 0),
            at: &at,
            sturdy: &sturdy,
            conductor: &conductor,
            source: &source,
            instrument: &|_| None,
            state_id: &|_| None,
        };
        f(&around)
    }

    fn side(around: &Around, pos: BlockPos, d: Face, up: bool) -> RedstoneSide {
        connecting_side(around, pos, d, up)
    }

    /// **Check `450`'s geometry, at the level of the rule.** Dust at the
    /// origin, stone to its east, and dust on top of that stone. The wire
    /// must answer `UP` for east — it climbs the block beside it.
    ///
    /// Paired with the two below: remove the dust on top and the answer is
    /// `NONE` (the stone is a conductor and blocks); remove the stone's
    /// sturdiness by making it a lone dust at the same height and the answer
    /// is `SIDE`. One input cannot tell those three apart.
    #[test]
    fn dust_climbs_the_block_beside_it() {
        with(
            &[
                ((1, 0, 0), "minecraft:stone"),
                ((1, 1, 0), "minecraft:redstone_wire[power=0]"),
            ],
            |a| assert_eq!(side(a, (0, 0, 0), Face::East, true), RedstoneSide::Up),
        );
    }

    /// The degenerate: the same stone with nothing on top. `canGoUp` is still
    /// true and the block is still climbable, but there is nothing to connect
    /// to up there, so the rule falls through to the conductor test and
    /// answers `NONE`. This passes under a port that always returned `UP` for
    /// a sturdy neighbour.
    #[test]
    fn dust_does_not_climb_a_bare_block() {
        with(&[((1, 0, 0), "minecraft:stone")], |a| {
            assert_eq!(side(a, (0, 0, 0), Face::East, false), RedstoneSide::None);
            assert_eq!(side(a, (0, 0, 0), Face::East, true), RedstoneSide::None);
        });
    }

    /// **`canGoUp` is a real gate.** The same climbable arrangement with a
    /// conductor over the wire's own head: the whole first branch is skipped
    /// and the answer drops to `NONE`, because the stone beside it is a
    /// conductor.
    ///
    /// This is the assertion that fails if `canGoUp` is hard-coded true, which
    /// is the natural shortcut when the three-argument overload is not ported.
    #[test]
    fn a_conductor_overhead_stops_the_climb() {
        with(
            &[
                ((1, 0, 0), "minecraft:stone"),
                ((1, 1, 0), "minecraft:redstone_wire[power=0]"),
            ],
            |a| assert_eq!(side(a, (0, 0, 0), Face::East, false), RedstoneSide::None),
        );
    }

    /// Dust one block **down** and across connects sideways — the second
    /// diagonal, offsets 117-131. Its own degenerate is that the same
    /// position holding stone gives `NONE`.
    #[test]
    fn dust_reaches_down_the_step_beside_it() {
        with(&[((1, -1, 0), "minecraft:redstone_wire[power=0]")], |a| {
            assert_eq!(side(a, (0, 0, 0), Face::East, true), RedstoneSide::Side);
        });
        with(&[((1, -1, 0), "minecraft:stone")], |a| {
            assert_eq!(side(a, (0, 0, 0), Face::East, true), RedstoneSide::None);
        });
    }

    /// **A conductor and a source are different questions, and `glass` is why
    /// the column exists.** `redstone_block` is a full sturdy block that is
    /// not a conductor and *is* a source: the wire connects sideways to it.
    /// `stone` is a conductor and not a source: the wire does not.
    ///
    /// A port that used `collisionFullFaces` for `isRedstoneConductor` gets
    /// the first of these wrong, and nothing else in this module notices.
    #[test]
    fn a_source_that_is_not_a_conductor_connects_sideways() {
        with(&[((1, 0, 0), "minecraft:redstone_block")], |a| {
            assert_eq!(side(a, (0, 0, 0), Face::East, true), RedstoneSide::Side);
        });
        with(&[((1, 0, 0), "minecraft:stone")], |a| {
            assert_eq!(side(a, (0, 0, 0), Face::East, true), RedstoneSide::None);
        });
    }

    /// **The test the whole `block_props` column exists for, and the first
    /// version of this module did not have it.**
    ///
    /// Injecting "answer `isRedstoneConductor` from the collision/sturdy
    /// column" left every other wire test green. Both of the cases in
    /// `a_source_that_is_not_a_conductor_connects_sideways` reach their answer
    /// through `shouldConnectTo` at offset 97, *before* the conductor test at
    /// 106 is ever consulted, so the substitution is invisible there.
    ///
    /// This is the arrangement that reaches it: glass to the east — full on
    /// every collision face, **not** a conductor — with dust one step below
    /// it. The conductor test must fall through so that the second diagonal
    /// at offsets 117-131 can see the dust and answer `SIDE`. Under the
    /// substitution glass reads as a conductor, the rule returns `NONE` at
    /// 114, and the dust below is never looked at.
    ///
    /// Its degenerate is stone in the same place, which really is a conductor
    /// and really does answer `NONE` — same geometry, one property different.
    #[test]
    fn dust_runs_under_glass_because_glass_is_not_a_conductor() {
        with(
            &[
                ((1, 0, 0), "minecraft:glass"),
                ((1, -1, 0), "minecraft:redstone_wire[power=0]"),
            ],
            |a| assert_eq!(side(a, (0, 0, 0), Face::East, true), RedstoneSide::Side),
        );
        with(
            &[
                ((1, 0, 0), "minecraft:stone"),
                ((1, -1, 0), "minecraft:redstone_wire[power=0]"),
            ],
            |a| assert_eq!(side(a, (0, 0, 0), Face::East, true), RedstoneSide::None),
        );
    }

    /// A repeater connects only along its own axis — offsets 22-50 — and the
    /// mirror is that it does not connect across it. A comparator has **no**
    /// such rule and connects from any side, which is the pair that pins the
    /// difference the briefing got backwards.
    #[test]
    fn a_repeater_connects_along_its_axis_and_a_comparator_from_anywhere() {
        with(
            &[((1, 0, 0), "minecraft:repeater[facing=east,powered=false]")],
            |a| assert_eq!(side(a, (0, 0, 0), Face::East, true), RedstoneSide::Side),
        );
        with(
            &[((1, 0, 0), "minecraft:repeater[facing=north,powered=false]")],
            |a| assert_eq!(side(a, (0, 0, 0), Face::East, true), RedstoneSide::None),
        );
        with(
            &[((1, 0, 0), "minecraft:comparator[facing=north,mode=compare]")],
            |a| assert_eq!(side(a, (0, 0, 0), Face::East, true), RedstoneSide::Side),
        );
    }

    /// **A lone dust stays a dot; one neighbour makes it a straight line.**
    ///
    /// This assertion was written the other way round first — east and west
    /// forced to `SIDE`, making a T — and the port refuted it. With a
    /// connection to the north, `noNS` is **false**, so the two `if`s that
    /// would set `WEST` and `EAST` are both skipped; `noEW` is true, so
    /// `SOUTH` is set and the dust becomes a north-south line through itself.
    /// The four `if`s at offsets 144-239 straighten a stub rather than
    /// spreading it, and reading them as "fill in the empty axis" gets the
    /// axis backwards.
    ///
    /// The early return at 36-49 is what keeps the lone case a dot: only a
    /// state that was *already* a dot may stay one. Drop it and the first
    /// assertion becomes a cross.
    #[test]
    fn a_lone_dust_is_a_dot_and_one_neighbour_makes_it_a_line() {
        let lone = with(&[], |a| {
            connection_state(a, (0, 0, 0), "minecraft:redstone_wire[power=0]")
        });
        assert!(is_dot(&lone), "a dust with nothing round it stays a dot");

        let line = with(&[((0, 0, -1), "minecraft:redstone_wire[power=0]")], |a| {
            connection_state(a, (0, 0, 0), "minecraft:redstone_wire[power=0]")
        });
        assert_eq!(property_of(&line, "north"), Some("side"));
        assert_eq!(
            property_of(&line, "south"),
            Some("side"),
            "noEW is true, so the north stub is straightened through to the south"
        );
        assert_eq!(property_of(&line, "east"), Some("none"));
        assert_eq!(property_of(&line, "west"), Some("none"));
    }

    /// `POWER` is carried across from the old state and never computed here.
    #[test]
    fn the_power_is_carried_and_not_recomputed() {
        let out = with(&[], |a| {
            connection_state(a, (0, 0, 0), "minecraft:redstone_wire[power=9]")
        });
        assert_eq!(property_of(&out, "power"), Some("9"));
    }

    // ---- `updateShape`, one test per branch --------------------------------
    //
    // Four branches, and the corpus scenario that motivated the whole port
    // reaches exactly one of them. Each test below is written so that the
    // *other three* branches give a different answer, because a branch test
    // whose expected value both arms produce is not a branch test.

    fn dust(east: &str, north: &str, south: &str, west: &str) -> String {
        format!(
            "minecraft:redstone_wire[east={east},north={north},power=0,south={south},west={west}]"
        )
    }

    /// **The `DOWN` branch, both halves, and the hopper that makes the
    /// argument load-bearing.**
    ///
    /// Vanilla asks `canSurviveOn(level, neighbourPos, neighbourState)` — the
    /// *state it was handed*, not a fresh read. A hopper's top face is not
    /// sturdy and dust sits on it anyway; it is the only block in the game with
    /// that exemption, so it is also the only thing that can tell a port which
    /// of the two the `||` reaches.
    #[test]
    fn a_wire_falls_only_when_what_is_under_it_cannot_hold_it() {
        let state = dust("side", "none", "none", "side");
        let held = with(&[((0, -1, 0), "minecraft:stone")], |a| {
            update_shape(
                a,
                (0, 0, 0),
                &state,
                Face::Down,
                (0, -1, 0),
                "minecraft:stone",
            )
        });
        assert_eq!(
            held, None,
            "stone holds it, so vanilla returns it unchanged"
        );

        let dropped = with(&[], |a| {
            update_shape(
                a,
                (0, 0, 0),
                &state,
                Face::Down,
                (0, -1, 0),
                "minecraft:air",
            )
        });
        assert_eq!(dropped.as_deref(), Some("minecraft:air"));

        let hopper = with(&[((0, -1, 0), "minecraft:hopper")], |a| {
            update_shape(
                a,
                (0, 0, 0),
                &state,
                Face::Down,
                (0, -1, 0),
                "minecraft:hopper",
            )
        });
        assert_eq!(
            hopper, None,
            "the hopper is not sturdy on top and dust sits on it regardless"
        );

        // The argument and the world made to **disagree**, which is the only
        // way to show which of the two the rule reads. Vanilla's
        // `canSurviveOn(level, neighbourPos, neighbourState)` reads the state
        // it was handed; a port that re-reads the position instead is
        // identical on every path this server has, and this is the assertion
        // that separates them. The world here is air and the argument is a
        // hopper, so a re-reading port drops the dust.
        let handed = with(&[], |a| {
            update_shape(
                a,
                (0, 0, 0),
                &state,
                Face::Down,
                (0, -1, 0),
                "minecraft:hopper",
            )
        });
        assert_eq!(
            handed, None,
            "the state argument wins over a fresh read of the position"
        );
    }

    /// **The `UP` branch** — a whole `getConnectionState`, not a one-side
    /// edit, because the block above decides `canGoUp` for all four sides at
    /// once.
    ///
    /// A conductor arriving overhead takes away every upward connection the
    /// dust had. Falling through to the horizontal branch here would edit one
    /// side named by a vertical direction, which `PROPERTY_BY_DIRECTION` does
    /// not even have a key for.
    #[test]
    fn a_block_landing_on_top_of_a_wire_re_derives_all_four_sides() {
        let out = with(
            &[
                ((0, -1, 0), "minecraft:stone"),
                ((0, 1, 0), "minecraft:stone"),
                ((1, 0, 0), "minecraft:stone"),
                ((1, 1, 0), "minecraft:redstone_wire[power=0]"),
            ],
            |a| {
                update_shape(
                    a,
                    (0, 0, 0),
                    &dust("up", "none", "none", "side"),
                    Face::Up,
                    (0, 1, 0),
                    "minecraft:stone",
                )
            },
        );
        assert_eq!(
            out.as_deref(),
            Some(dust("side", "side", "side", "side").as_str()),
            "canGoUp is false under a conductor, so the climb is lost and the dot re-crosses"
        );
    }

    /// **The cheap horizontal branch at offsets 95-117**, which writes one side
    /// and leaves the other three exactly as they were.
    ///
    /// The subject's `north=side` is a connection the world does **not**
    /// support. That is the whole point: the cheap path keeps it and the
    /// `crossState` re-derivation would throw it away and put `west=side` in
    /// its place. An arrangement whose stored sides already agree with the
    /// world cannot tell the two apart, and every arrangement the corpus builds
    /// is one of those.
    #[test]
    fn a_side_that_stays_connected_is_written_in_place() {
        let out = with(
            &[
                ((0, -1, 0), "minecraft:stone"),
                ((1, 0, 0), "minecraft:stone"),
                ((1, 1, 0), "minecraft:redstone_wire[power=0]"),
            ],
            |a| {
                update_shape(
                    a,
                    (0, 0, 0),
                    &dust("side", "side", "none", "none"),
                    Face::East,
                    (1, 0, 0),
                    "minecraft:stone",
                )
            },
        );
        assert_eq!(
            out.as_deref(),
            Some(dust("up", "side", "none", "none").as_str()),
            "east goes side->up, both connected, so the other three are untouched"
        );
    }

    /// **The `crossState` fallback at 118-169, reached because the side
    /// *gained* a connection.** This is the branch check `450` runs, and the
    /// one the previous link left untranscribed.
    ///
    /// A dot whose east becomes `up` cannot take the cheap path — the boolean
    /// changed — so the state is rebuilt from `crossState` and pushed back
    /// through `getConnectionState`, which is what turns the lone connection
    /// into a line by forcing `west`.
    #[test]
    fn a_side_that_gains_a_connection_re_derives_from_the_cross_state() {
        let out = with(
            &[
                ((0, -1, 0), "minecraft:stone"),
                ((1, 0, 0), "minecraft:stone"),
                ((1, 1, 0), "minecraft:redstone_wire[power=0]"),
            ],
            |a| {
                update_shape(
                    a,
                    (0, 0, 0),
                    &dust("none", "none", "none", "none"),
                    Face::East,
                    (1, 0, 0),
                    "minecraft:stone",
                )
            },
        );
        assert_eq!(
            out.as_deref(),
            Some(dust("up", "none", "none", "side").as_str()),
            "one connection makes a line, so west is forced even though nothing is there"
        );
    }

    /// **The `!isCross(state)` half of the same guard**, which is the one a
    /// port drops without noticing.
    ///
    /// Here the boolean does **not** change — east was connected and stays
    /// connected — so the cheap path is reached on every count except that the
    /// subject is already a cross. A cross's four `SIDE`s are not connections
    /// it chose, so it must re-derive; keeping them would leave three sides
    /// pointing at air.
    #[test]
    fn a_cross_re_derives_even_when_the_side_stays_connected() {
        let out = with(
            &[
                ((0, -1, 0), "minecraft:stone"),
                ((1, 0, 0), "minecraft:stone"),
                ((1, 1, 0), "minecraft:redstone_wire[power=0]"),
            ],
            |a| {
                update_shape(
                    a,
                    (0, 0, 0),
                    &dust("side", "side", "side", "side"),
                    Face::East,
                    (1, 0, 0),
                    "minecraft:stone",
                )
            },
        );
        assert_eq!(
            out.as_deref(),
            Some(dust("up", "none", "none", "side").as_str()),
            "the cheap path would have kept north, south and west at `side`"
        );
    }

    /// `crossState` is the constructor's all-`SIDE` state carrying the
    /// subject's `POWER`, and it is **not** `getConnectionState`'s all-`none`
    /// base. Swapping them is invisible in every test above, because the
    /// re-derivation immediately overwrites all four sides — it shows up only
    /// in the `power` that rides along.
    #[test]
    fn the_cross_state_carries_the_power_and_starts_connected() {
        assert_eq!(
            cross_state("11"),
            "minecraft:redstone_wire[east=side,north=side,power=11,south=side,west=side]"
        );
        assert!(is_cross(&cross_state("0")));
        assert!(!is_dot(&cross_state("0")));
    }

    /// `isCross` is `&&` across all four, not "three or more" and not `isDot`
    /// negated — a dust with two opposite connections is neither.
    #[test]
    fn is_cross_needs_all_four_sides() {
        assert!(is_cross(&dust("side", "side", "side", "up")));
        assert!(!is_cross(&dust("side", "none", "side", "side")));
        assert!(!is_cross(&dust("side", "none", "none", "side")));
        assert!(!is_dot(&dust("side", "none", "none", "side")));
    }
}
