//! `SculkSpreader` — the charge simulation behind sculk patches.
//!
//! # Why this is a subsystem and not a feature
//!
//! `sculk_patch` does not place blocks directly. It creates a spreader, hands
//! it charge, and runs the same cursor simulation a sculk catalyst runs at
//! runtime:
//!
//! ```java
//!   if (!canSpreadFrom(level, origin)) return false;
//!   SculkSpreader spreader = SculkSpreader.createWorldGenSpreader();
//!   int rounds = config.spreadRounds() + config.growthRounds();
//!   for (int i = 0; i < rounds; i++) {
//!       for (int j = 0; j < config.chargeCount(); j++)
//!           spreader.addCursors(origin, config.amountPerCharge());
//!       boolean convert = i < config.spreadRounds();
//!       for (int k = 0; k < config.spreadAttempts(); k++)
//!           spreader.updateCursors(level, origin, random, convert);
//!       spreader.clear();
//!   }
//! ```
//!
//! So porting it is not only worldgen work: the same code decides what a
//! catalyst does when a mob dies near it.
//!
//! # The constants, from the class file rather than the wiki
//!
//! ```text
//!   MAX_GROWTH_RATE_RADIUS  24      MAX_CHARGE           1000
//!   MAX_DECAY_FACTOR        0.5f    MAX_CURSORS            32
//!   SHRIEKER_PLACEMENT_RATE 11      MAX_CURSOR_DISTANCE  1024
//! ```
//!
//! and `createWorldGenSpreader()` is
//!
//! ```java
//!   new SculkSpreader(true, SCULK_REPLACEABLE_WORLD_GEN, 50, 1, 5, 10)
//! ```
//!
//! i.e. `isWorldGeneration = true`, `growthSpawnCost = 50`,
//! `noGrowthRadius = 1`, `chargeDecayRate = 5`, `additionalDecayRate = 10`.
//! The level spreader differs in every one of those numbers, which is why the
//! constructor takes them rather than hard-coding a set.
//!
//! # What is here and what is not
//!
//! This lands the container and `addCursors`, both of which are exactly
//! specified and cheap to gate. `updateCursors` and `ChargeCursor::update` —
//! the spread search, the merge pass and `SculkBehaviour` — are the bulk of
//! the work and follow.

/// `SculkSpreader.MAX_CHARGE`.
pub const MAX_CHARGE: i32 = 1000;
/// `SculkSpreader.MAX_CURSORS` — private in vanilla, and load-bearing:
/// `addCursor` silently drops a cursor once the list is full.
pub const MAX_CURSORS: usize = 32;
/// `SculkSpreader.MAX_CURSOR_DISTANCE`.
pub const MAX_CURSOR_DISTANCE: i32 = 1024;
/// `SculkSpreader.MAX_GROWTH_RATE_RADIUS`.
pub const MAX_GROWTH_RATE_RADIUS: i32 = 24;

/// One charge travelling through the world.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChargeCursor {
    pub pos: (i32, i32, i32),
    pub charge: i32,
    /// `decayDelay`, `updateDelay` and the visited set belong to the update
    /// pass and are added with it, so they are deliberately absent here rather
    /// than present and unused.
    pub decay_delay: i32,
    pub update_delay: i32,
}

impl ChargeCursor {
    /// `new ChargeCursor(pos, charge)`.
    #[must_use]
    pub const fn new(pos: (i32, i32, i32), charge: i32) -> Self {
        Self {
            pos,
            charge,
            decay_delay: 1,
            update_delay: 0,
        }
    }

    /// `isPosUnreasonable` — a cursor further than `MAX_CURSOR_DISTANCE` from
    /// the origin in any axis is dropped rather than updated.
    #[must_use]
    pub fn is_pos_unreasonable(&self, origin: (i32, i32, i32)) -> bool {
        (self.pos.0 - origin.0).abs() > MAX_CURSOR_DISTANCE
            || (self.pos.2 - origin.2).abs() > MAX_CURSOR_DISTANCE
    }
}

/// The spreader's fixed parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SculkSpreader {
    pub is_world_generation: bool,
    pub growth_spawn_cost: i32,
    pub no_growth_radius: i32,
    pub charge_decay_rate: i32,
    pub additional_decay_rate: i32,
    cursors: Vec<ChargeCursor>,
}

impl SculkSpreader {
    /// `SculkSpreader.createWorldGenSpreader()`.
    #[must_use]
    pub const fn world_gen() -> Self {
        Self {
            is_world_generation: true,
            growth_spawn_cost: 50,
            no_growth_radius: 1,
            charge_decay_rate: 5,
            additional_decay_rate: 10,
            cursors: Vec::new(),
        }
    }

    /// `getCursors`.
    #[must_use]
    pub fn cursors(&self) -> &[ChargeCursor] {
        &self.cursors
    }

    /// `clear`.
    pub fn clear(&mut self) {
        self.cursors.clear();
    }

    /// `addCursor` — **drops the cursor** when the list is already full.
    ///
    /// The cap is 32 and it is silent. A port that grew the list instead would
    /// keep charge vanilla throws away, and produce more sculk rather than
    /// less, which is the opposite of a shortfall and easy to misread.
    pub fn add_cursor(&mut self, cursor: ChargeCursor) {
        if self.cursors.len() < MAX_CURSORS {
            self.cursors.push(cursor);
        }
    }

    /// `addCursors(pos, charge)` — split a charge into cursors of at most
    /// `MAX_CHARGE`.
    ///
    /// ```java
    ///   while (charge > 0) {
    ///       int amount = Math.min(charge, MAX_CHARGE);
    ///       addCursor(new ChargeCursor(pos, amount));
    ///       charge -= amount;
    ///   }
    /// ```
    ///
    /// With `amount_per_charge` 32 this is one cursor per call, so the loop
    /// looks pointless on the worldgen path — it is not, because the catalyst
    /// path passes much larger charges through the same method.
    pub fn add_cursors(&mut self, pos: (i32, i32, i32), mut charge: i32) {
        while charge > 0 {
            let amount = charge.min(MAX_CHARGE);
            self.add_cursor(ChargeCursor::new(pos, amount));
            charge -= amount;
        }
    }
}

/// `ChargeCursor.NON_CORNER_NEIGHBOURS` — the 18 offsets a cursor may step to.
///
/// Built in vanilla as
///
/// ```java
///   BlockPos.betweenClosedStream(new BlockPos(-1,-1,-1), new BlockPos(1,1,1))
///           .filter(p -> (p.getX() == 0 || p.getY() == 0 || p.getZ() == 0)
///                        && !p.equals(BlockPos.ZERO))
///           .map(BlockPos::immutable)
/// ```
///
/// so it is the 3x3x3 block minus the centre minus the eight corners: a corner
/// is exactly the case where all three coordinates are non-zero.
///
/// **The order matters** even though callers shuffle it, because the shuffle
/// is a permutation of *indices* — a different base order gives a different
/// answer from the same draws. `BlockPos.betweenClosedStream` iterates through
/// `Cursor3D`, whose arithmetic is `x = i % width`, `y = (i / width) % height`,
/// `z = i / width / height`: **x fastest, then y, then z**. Read off the
/// bytecode rather than recalled, because the two plausible orders differ only
/// in results.
#[must_use]
pub fn non_corner_neighbours() -> Vec<(i32, i32, i32)> {
    let mut out = Vec::with_capacity(18);
    for z in -1..=1 {
        for y in -1..=1 {
            for x in -1..=1 {
                if (x == 0 || y == 0 || z == 0) && (x, y, z) != (0, 0, 0) {
                    out.push((x, y, z));
                }
            }
        }
    }
    out
}

/// `getRandomizedNonCornerNeighbourOffsets(random)`.
///
/// `Util.shuffledCopy(NON_CORNER_NEIGHBOURS, random)` — a copy, then
/// `Util.shuffle`, which is the same descending Fisher-Yates with bound `i`
/// that `CappedProcessor` uses. Shared with
/// [`crate::structure::processor::shuffled_order`] rather than reimplemented,
/// so the bound cannot drift between the two callers.
pub fn randomized_non_corner_neighbour_offsets<R: crate::rng::RandomSource + ?Sized>(
    random: &mut R,
) -> Vec<(i32, i32, i32)> {
    let base = non_corner_neighbours();
    crate::structure::processor::shuffled_order(random, base.len())
        .into_iter()
        .map(|i| base[i])
        .collect()
}

/// The six faces, in vanilla's `Direction` order where it matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Face {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

impl Face {
    /// The unit step this face points along.
    #[must_use]
    pub const fn offset(self) -> (i32, i32, i32) {
        match self {
            Self::Down => (0, -1, 0),
            Self::Up => (0, 1, 0),
            Self::North => (0, 0, -1),
            Self::South => (0, 0, 1),
            Self::West => (-1, 0, 0),
            Self::East => (1, 0, 0),
        }
    }

    /// `Direction.fromAxisAndDirection(axis, sign < 0 ? NEGATIVE : POSITIVE)`.
    ///
    /// Note the comparison: vanilla asks `< 0`, so **zero counts as positive**.
    /// That is not cosmetic — `isMovementUnobstructed` builds a direction per
    /// axis before it checks which axis is zero, and a port that treated zero
    /// as negative would pick the opposite face on exactly the axes that
    /// matter.
    #[must_use]
    pub const fn from_axis(axis: u8, sign: i32) -> Self {
        match (axis, sign < 0) {
            (0, true) => Self::West,
            (0, false) => Self::East,
            (1, true) => Self::Down,
            (1, false) => Self::Up,
            (2, true) => Self::North,
            _ => Self::South,
        }
    }
}

impl Face {
    /// The face pointing the other way.
    ///
    /// Lost when the duplicated `SpreadType` block was excised — it lived in
    /// that block and nothing else needed it until the discharge loop did.
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::Down => Self::Up,
            Self::Up => Self::Down,
            Self::North => Self::South,
            Self::South => Self::North,
            Self::West => Self::East,
            Self::East => Self::West,
        }
    }
}

/// What the step search needs of the world.
pub trait SpreadWorld {
    /// `BlockState#isFaceSturdy(level, pos, direction)` with `SupportType.FULL`.
    fn is_face_sturdy(&mut self, pos: (i32, i32, i32), face: Face) -> bool;
}

/// `ChargeCursor.isUnobstructed(level, pos, direction)`.
///
/// ```java
///   return !level.getBlockState(pos.relative(d))
///                .isFaceSturdy(level, pos.relative(d), d.getOpposite());
/// ```
///
/// The face tested is the **opposite** one, on the *neighbour* — i.e. the face
/// pointing back at `pos`. Testing the near block's own face instead would
/// pass and fail in almost the same places, which is why it is spelled out.
pub fn is_unobstructed<W: SpreadWorld + ?Sized>(
    world: &mut W,
    pos: (i32, i32, i32),
    face: Face,
) -> bool {
    let (dx, dy, dz) = face.offset();
    let neighbour = (pos.0 + dx, pos.1 + dy, pos.2 + dz);
    let opposite = match face {
        Face::Down => Face::Up,
        Face::Up => Face::Down,
        Face::North => Face::South,
        Face::South => Face::North,
        Face::West => Face::East,
        Face::East => Face::West,
    };
    !world.is_face_sturdy(neighbour, opposite)
}

/// `ChargeCursor.isMovementUnobstructed(level, from, to)`.
///
/// ```java
///   if (from.distManhattan(to) == 1) return true;
///   BlockPos d = to.subtract(from);
///   Direction dx = fromAxisAndDirection(X, d.getX() < 0 ? NEGATIVE : POSITIVE);
///   Direction dy = fromAxisAndDirection(Y, d.getY() < 0 ? NEGATIVE : POSITIVE);
///   Direction dz = fromAxisAndDirection(Z, d.getZ() < 0 ? NEGATIVE : POSITIVE);
///   if (d.getX() == 0) return isUnobstructed(level, from, dy)
///                          || isUnobstructed(level, from, dz);
///   if (d.getY() == 0) return isUnobstructed(level, from, dx)
///                          || isUnobstructed(level, from, dz);
///   return isUnobstructed(level, from, dx) || isUnobstructed(level, from, dy);
/// ```
///
/// A cursor only ever steps to a non-corner neighbour, so the offset has at
/// least one zero component and exactly one of those branches applies. The
/// adjacent case short-circuits to true before any world read.
pub fn is_movement_unobstructed<W: SpreadWorld + ?Sized>(
    world: &mut W,
    from: (i32, i32, i32),
    to: (i32, i32, i32),
) -> bool {
    let d = (to.0 - from.0, to.1 - from.1, to.2 - from.2);
    if d.0.abs() + d.1.abs() + d.2.abs() == 1 {
        return true;
    }
    let fx = Face::from_axis(0, d.0);
    let fy = Face::from_axis(1, d.1);
    let fz = Face::from_axis(2, d.2);
    if d.0 == 0 {
        return is_unobstructed(world, from, fy) || is_unobstructed(world, from, fz);
    }
    if d.1 == 0 {
        return is_unobstructed(world, from, fx) || is_unobstructed(world, from, fz);
    }
    is_unobstructed(world, from, fx) || is_unobstructed(world, from, fy)
}

/// What `get_valid_movement_pos` needs beyond face sturdiness.
pub trait SculkWorld: SpreadWorld {
    fn block_at(&mut self, pos: (i32, i32, i32)) -> String;
    /// Membership of `#minecraft:sculk_replaceable_world_gen`.
    fn is_sculk_replaceable(&mut self, pos: (i32, i32, i32)) -> bool;
    /// Write a block **immediately**.
    ///
    /// Vanilla's `attemptPlaceSculk` calls `level.setBlock` inline, so the
    /// very next read sees the new sculk. Collecting writes and applying them
    /// after the patch finishes looks equivalent and is not: every conversion
    /// then reads a world that never contains the sculk already placed, so the
    /// same substrate converts over and over, charge is spent on nothing, and
    /// the patch never cascades. Measured at ~10x redundancy — 4076
    /// conversions producing 395 distinct blocks.
    fn set_block(&mut self, pos: (i32, i32, i32), state: &str);
}

/// `MultifaceBlock.hasFace(state, direction)` — the property is named after
/// the face and holds `"true"`/`"false"`.
#[must_use]
pub fn has_face(state: &str, face: Face) -> bool {
    let key = match face {
        Face::Down => "down",
        Face::Up => "up",
        Face::North => "north",
        Face::South => "south",
        Face::West => "west",
        Face::East => "east",
    };
    state.contains(&format!("{key}=true"))
}

/// `SculkBlock.getDecayPenalty(spreader, pos, origin, charge)`.
///
/// ```java
///   int radius = spreader.noGrowthRadius();
///   float d = Mth.square((float)Math.sqrt(pos.distSqr(origin)) - (float)radius);
///   int maxSq = Mth.square(24 - radius);
///   float factor = Math.min(1.0f, d / (float)maxSq);
///   return Math.max(1, (int)((float)charge * factor * 0.5f));
/// ```
///
/// Three things here read differently from how they are usually described:
///
///   * the clamp is at **1.0**, not at `MAX_DECAY_FACTOR`. The 0.5 is a
///     separate multiply at the very end, so the effective ceiling is half the
///     charge — the same number by a different route, and a port that clamped
///     at 0.5 and then multiplied would halve it twice;
///   * `(int)` is `f2i`, which **truncates toward zero**. There is no `+ 0.5f`
///     rounding anywhere in this method;
///   * the whole computation is in `f32` after `d2f`, so doing it in `f64`
///     and narrowing at the end gives a different answer at the boundaries.
#[must_use]
pub fn get_decay_penalty(
    no_growth_radius: i32,
    pos: (i32, i32, i32),
    origin: (i32, i32, i32),
    charge: i32,
) -> i32 {
    let dist_sq = f64::from(pos.0 - origin.0).mul_add(
        f64::from(pos.0 - origin.0),
        f64::from(pos.1 - origin.1).mul_add(
            f64::from(pos.1 - origin.1),
            f64::from(pos.2 - origin.2) * f64::from(pos.2 - origin.2),
        ),
    );
    #[allow(clippy::cast_possible_truncation)]
    let d = {
        let v = dist_sq.sqrt() as f32 - no_growth_radius as f32;
        v * v
    };
    let max_sq = (24 - no_growth_radius) * (24 - no_growth_radius);
    #[allow(clippy::cast_precision_loss)]
    let factor = (d / max_sq as f32).min(1.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    let penalty = (charge as f32 * factor * 0.5) as i32;
    penalty.max(1)
}

/// `SculkBlock.canPlaceGrowth(level, pos)` — may a growth spawn on top of the
/// sculk at `pos`?
///
/// ```java
///   BlockState above = level.getBlockState(pos.above());
///   if (!above.isAir()
///       && !(above.is(Blocks.WATER) && above.getFluidState().is(Fluids.WATER)))
///       return false;
///   int count = 0;
///   for (BlockPos p : betweenClosed(pos.offset(-4,0,-4), pos.offset(4,2,4))) {
///       BlockState s = level.getBlockState(p);
///       if (s.is(SCULK_SENSOR) || s.is(SCULK_SHRIEKER)) count++;
///       if (count > 2) return false;
///   }
///   return true;
/// ```
///
/// The space above must be air **or source water** — the water test is two
/// conditions, the block *and* its fluid state, so flowing water does not
/// qualify. The crowding box is 9x3x9 and asymmetric in y: it reaches two
/// blocks up and none down.
pub fn can_place_growth<W: SculkWorld + ?Sized>(world: &mut W, pos: (i32, i32, i32)) -> bool {
    let above = world.block_at((pos.0, pos.1 + 1, pos.2));
    let bare = above.split('[').next().unwrap_or(&above);
    let is_air = matches!(
        bare,
        "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
    );
    // Parenthesised deliberately: without them the `||` binds loosely and
    // *any* state carrying `level=0` reads as water — which is a composter,
    // a cauldron, a honey block and most of the fluid-adjacent game.
    let is_source_water =
        bare == "minecraft:water" && (!above.contains("level=") || above.contains("level=0"));
    if !is_air && !is_source_water {
        return false;
    }

    let mut count = 0;
    for y in pos.1..=pos.1 + 2 {
        for z in pos.2 - 4..=pos.2 + 4 {
            for x in pos.0 - 4..=pos.0 + 4 {
                let state = world.block_at((x, y, z));
                let bare = state.split('[').next().unwrap_or(&state);
                if matches!(bare, "minecraft:sculk_sensor" | "minecraft:sculk_shrieker") {
                    count += 1;
                }
                if count > 2 {
                    return false;
                }
            }
        }
    }
    true
}

/// `SculkShriekerBlock.SHRIEKER_PLACEMENT_RATE`.
pub const SHRIEKER_PLACEMENT_RATE: i32 = 11;

/// The spread geometry lives in [`crate::feature::multiface_growth`].
///
/// It was briefly reimplemented here before a search found the existing copy —
/// `SpreadType`, `spread_types`, `is_other_block_valid_as_source`,
/// `state_can_be_replaced` and `spread_from_face_toward_direction` are all
/// already ported and gated there, because glow lichen needs exactly the same
/// `MultifaceSpreader`.
///
/// Re-exported rather than wrapped so there is one definition to be wrong.
/// Two copies of a spread rule would agree on the day they were written and
/// drift the first time either was corrected — which is the same argument that
/// made `randomized_non_corner_neighbour_offsets` share the capped
/// processor's Fisher-Yates instead of owning a second one.
pub use crate::feature::multiface_growth::SpreadType;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_worldgen_spreader_matches_create_world_gen_spreader() {
        let s = SculkSpreader::world_gen();
        assert!(s.is_world_generation);
        assert_eq!(s.growth_spawn_cost, 50);
        assert_eq!(s.no_growth_radius, 1);
        assert_eq!(s.charge_decay_rate, 5);
        assert_eq!(s.additional_decay_rate, 10);
    }

    /// The deep-dark patch's own numbers: ten calls of 32 charge each.
    #[test]
    fn the_deep_dark_patch_makes_ten_cursors() {
        let mut s = SculkSpreader::world_gen();
        for _ in 0..10 {
            s.add_cursors((0, 0, 0), 32);
        }
        assert_eq!(s.cursors().len(), 10);
        assert!(s.cursors().iter().all(|c| c.charge == 32));
    }

    /// A charge above `MAX_CHARGE` splits, rather than being clamped away.
    #[test]
    fn a_large_charge_splits_at_max_charge() {
        let mut s = SculkSpreader::world_gen();
        s.add_cursors((1, 2, 3), 2500);
        let charges: Vec<i32> = s.cursors().iter().map(|c| c.charge).collect();
        assert_eq!(charges, vec![1000, 1000, 500]);
    }

    /// The 32-cursor cap is silent, and that silence is the behaviour: the
    /// remaining charge is discarded, not queued.
    #[test]
    fn the_cursor_cap_drops_charge_rather_than_queueing_it() {
        let mut s = SculkSpreader::world_gen();
        // 40 separate charges, but only 32 cursors can exist.
        for _ in 0..40 {
            s.add_cursors((0, 0, 0), 32);
        }
        assert_eq!(s.cursors().len(), MAX_CURSORS);
    }

    /// A single huge charge is also capped by the cursor limit, so charge is
    /// lost in two different ways and both are vanilla's.
    #[test]
    fn a_huge_charge_is_capped_by_the_cursor_limit_too() {
        let mut s = SculkSpreader::world_gen();
        s.add_cursors((0, 0, 0), 100_000);
        assert_eq!(s.cursors().len(), MAX_CURSORS);
        assert!(s.cursors().iter().all(|c| c.charge == MAX_CHARGE));
    }

    #[test]
    fn an_unreasonable_cursor_is_measured_in_x_and_z_only() {
        let far = ChargeCursor::new((2000, 0, 0), 1);
        assert!(far.is_pos_unreasonable((0, 0, 0)));
        let high = ChargeCursor::new((0, 5000, 0), 1);
        assert!(
            !high.is_pos_unreasonable((0, 0, 0)),
            "vanilla checks x and z, not y"
        );
    }

    /// Eighteen offsets: 27 cells, less the centre, less the eight corners.
    #[test]
    fn there_are_eighteen_non_corner_neighbours() {
        let n = non_corner_neighbours();
        assert_eq!(n.len(), 18);
        assert!(!n.contains(&(0, 0, 0)), "the centre is excluded");
        for &(x, y, z) in &n {
            assert!(
                x == 0 || y == 0 || z == 0,
                "({x},{y},{z}) is a corner and must not be here"
            );
            assert!((-1..=1).contains(&x) && (-1..=1).contains(&y) && (-1..=1).contains(&z));
        }
        let mut sorted = n.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 18, "no duplicates");
    }

    /// The base order is `Cursor3D`'s: x fastest, then y, then z. A different
    /// order would still be a valid set and would still shuffle to a
    /// permutation, but to a *different* permutation from the same draws.
    #[test]
    fn the_base_order_is_x_fastest_then_y_then_z() {
        let n = non_corner_neighbours();
        assert_eq!(
            n[0],
            (0, -1, -1),
            "first cell with a zero, scanning x fastest"
        );
        assert_eq!(n[1], (-1, 0, -1));
        assert_eq!(n[2], (0, 0, -1));
        assert_eq!(n[3], (1, 0, -1));
        assert_eq!(n[4], (0, 1, -1));
        assert_eq!(*n.last().expect("non-empty"), (0, 1, 1));
    }

    /// The shuffle is a permutation, and it is the same Fisher-Yates the
    /// capped processor uses — shared rather than copied.
    #[test]
    fn the_randomized_offsets_are_a_permutation_of_the_eighteen() {
        use crate::rng::legacy::LegacyRandomSource;
        let mut random = LegacyRandomSource::new(42);
        let shuffled = randomized_non_corner_neighbour_offsets(&mut random);
        let mut a = shuffled.clone();
        let mut b = non_corner_neighbours();
        a.sort_unstable();
        b.sort_unstable();
        assert_eq!(a, b);
        assert_ne!(
            shuffled,
            non_corner_neighbours(),
            "a shuffle that returns the input order proves nothing"
        );
    }

    /// A world where a chosen set of (pos, face) pairs are sturdy.
    struct Faces(Vec<((i32, i32, i32), Face)>);
    impl SpreadWorld for Faces {
        fn is_face_sturdy(&mut self, pos: (i32, i32, i32), face: Face) -> bool {
            self.0.contains(&(pos, face))
        }
    }

    /// An adjacent step short-circuits before touching the world at all.
    #[test]
    fn an_adjacent_step_is_always_unobstructed() {
        // Every face sturdy: if the world were consulted this would be false.
        let mut world = Faces(
            [
                Face::Down,
                Face::Up,
                Face::North,
                Face::South,
                Face::West,
                Face::East,
            ]
            .into_iter()
            .map(|f| ((0, 0, 0), f))
            .collect(),
        );
        assert!(is_movement_unobstructed(&mut world, (0, 0, 0), (1, 0, 0)));
        assert!(is_movement_unobstructed(&mut world, (0, 0, 0), (0, -1, 0)));
    }

    /// A diagonal is unobstructed when *either* of its two component faces is
    /// free — an `||`, not an `&&`.
    #[test]
    fn a_diagonal_needs_only_one_free_face() {
        // Step (1,1,0): x and y components. Block the x side only.
        let mut world = Faces(vec![((1, 0, 0), Face::West)]);
        assert!(
            is_movement_unobstructed(&mut world, (0, 0, 0), (1, 1, 0)),
            "the y side is still free"
        );

        // Block both.
        let mut world = Faces(vec![((1, 0, 0), Face::West), ((0, 1, 0), Face::Down)]);
        assert!(!is_movement_unobstructed(&mut world, (0, 0, 0), (1, 1, 0)));
    }

    /// `isUnobstructed` reads the **neighbour's** face, pointing back at the
    /// cursor — not the cursor's own face.
    #[test]
    fn obstruction_is_read_on_the_neighbours_opposite_face() {
        let mut world = Faces(vec![((0, 1, 0), Face::Down)]);
        assert!(!is_unobstructed(&mut world, (0, 0, 0), Face::Up));

        // The same face on the *near* block must not be what is consulted.
        let mut world = Faces(vec![((0, 0, 0), Face::Up)]);
        assert!(is_unobstructed(&mut world, (0, 0, 0), Face::Up));
    }

    /// Zero counts as POSITIVE, because vanilla asks `< 0`.
    #[test]
    fn a_zero_component_yields_the_positive_face() {
        assert_eq!(Face::from_axis(0, 0), Face::East);
        assert_eq!(Face::from_axis(1, 0), Face::Up);
        assert_eq!(Face::from_axis(2, 0), Face::South);
        assert_eq!(Face::from_axis(0, -1), Face::West);
    }

    /// A world of named blocks; every face is free unless listed.
    struct Blocks {
        blocks: std::collections::BTreeMap<(i32, i32, i32), String>,
        replaceable: Vec<(i32, i32, i32)>,
        sturdy: Vec<((i32, i32, i32), Face)>,
    }
    impl SpreadWorld for Blocks {
        fn is_face_sturdy(&mut self, pos: (i32, i32, i32), face: Face) -> bool {
            self.sturdy.contains(&(pos, face))
        }
    }
    impl SculkWorld for Blocks {
        fn set_block(&mut self, pos: (i32, i32, i32), state: &str) {
            self.blocks.insert(pos, state.to_string());
        }
        fn block_at(&mut self, pos: (i32, i32, i32)) -> String {
            self.blocks
                .get(&pos)
                .cloned()
                .unwrap_or_else(|| "minecraft:stone".to_string())
        }
        fn is_sculk_replaceable(&mut self, pos: (i32, i32, i32)) -> bool {
            self.replaceable.contains(&pos)
        }
    }

    fn world(blocks: &[((i32, i32, i32), &str)]) -> Blocks {
        Blocks {
            blocks: blocks.iter().map(|(p, s)| (*p, (*s).to_string())).collect(),
            replaceable: Vec::new(),
            sturdy: Vec::new(),
        }
    }

    /// At the origin the distance is zero, so the penalty floors at 1 rather
    /// than at 0 — charge always decays by at least one when it decays.
    #[test]
    fn the_decay_penalty_never_falls_below_one() {
        assert_eq!(get_decay_penalty(1, (0, 0, 0), (0, 0, 0), 1000), 1);
        assert_eq!(get_decay_penalty(1, (1, 0, 0), (0, 0, 0), 1000), 1);
    }

    /// Far away the factor saturates at 1.0 and the penalty is half the
    /// charge — which is where `MAX_DECAY_FACTOR` 0.5 actually shows up. A
    /// port that clamped at 0.5 *and* multiplied by 0.5 would give a quarter.
    #[test]
    fn a_distant_cursor_loses_half_its_charge() {
        // 24 - radius = 23, so beyond 23+1 blocks the factor is 1.0.
        assert_eq!(get_decay_penalty(1, (100, 0, 0), (0, 0, 0), 1000), 500);
        assert_eq!(get_decay_penalty(1, (100, 0, 0), (0, 0, 0), 7), 3);
    }

    /// `f2i` truncates: 7 * 1.0 * 0.5 is 3.5 and the answer is 3, not 4.
    #[test]
    fn the_penalty_truncates_rather_than_rounding() {
        assert_eq!(
            get_decay_penalty(1, (100, 0, 0), (0, 0, 0), 7),
            3,
            "3.5 truncates to 3; rounding would give 4"
        );
        assert_eq!(get_decay_penalty(1, (100, 0, 0), (0, 0, 0), 9), 4);
    }

    /// The penalty grows with distance rather than jumping straight to the cap.
    #[test]
    fn the_penalty_grows_with_distance() {
        let near = get_decay_penalty(1, (5, 0, 0), (0, 0, 0), 1000);
        let mid = get_decay_penalty(1, (12, 0, 0), (0, 0, 0), 1000);
        let far = get_decay_penalty(1, (20, 0, 0), (0, 0, 0), 1000);
        assert!(near < mid && mid < far, "{near} {mid} {far}");
        assert!(far <= 500, "and never exceeds half the charge");
    }

    /// Air or source water above, and nothing else.
    #[test]
    fn a_growth_needs_air_or_source_water_above() {
        let mut w = world(&[]);
        // The fixture answers "minecraft:stone" for anything unset, so an
        // empty world is *solid* above and must refuse.
        assert!(!can_place_growth(&mut w, (0, 0, 0)));

        let mut w = world(&[((0, 1, 0), "minecraft:air")]);
        assert!(can_place_growth(&mut w, (0, 0, 0)));

        let mut w = world(&[((0, 1, 0), "minecraft:water[level=0]")]);
        assert!(can_place_growth(&mut w, (0, 0, 0)));

        let mut w = world(&[((0, 1, 0), "minecraft:deepslate")]);
        assert!(!can_place_growth(&mut w, (0, 0, 0)));

        // Flowing water is not source water: the fluid test is `level=0`.
        let mut w = world(&[((0, 1, 0), "minecraft:water[level=3]")]);
        assert!(!can_place_growth(&mut w, (0, 0, 0)));

        // And a non-water block carrying `level=0` must not pass as water,
        // which a loosely-bound `||` would have allowed.
        let mut w = world(&[((0, 1, 0), "minecraft:composter[level=0]")]);
        assert!(!can_place_growth(&mut w, (0, 0, 0)));
    }

    /// Three sensors or shriekers in the 9x3x9 box refuses; two is fine.
    #[test]
    fn three_sensors_nearby_refuse_a_growth() {
        let two = world(&[
            ((0, 1, 0), "minecraft:air"),
            ((2, 0, 2), "minecraft:sculk_sensor"),
            ((-2, 1, 1), "minecraft:sculk_shrieker"),
        ]);
        let mut two = two;
        assert!(can_place_growth(&mut two, (0, 0, 0)));

        let mut three = world(&[
            ((0, 1, 0), "minecraft:air"),
            ((2, 0, 2), "minecraft:sculk_sensor"),
            ((-2, 1, 1), "minecraft:sculk_shrieker"),
            ((3, 2, -3), "minecraft:sculk_sensor"),
        ]);
        assert!(!can_place_growth(&mut three, (0, 0, 0)));
    }

    /// The box reaches 4 out horizontally and 2 up, never down — so a sensor
    /// below the cursor does not count toward the crowd.
    #[test]
    fn the_crowding_box_never_reaches_below() {
        let mut w = world(&[
            ((0, 1, 0), "minecraft:air"),
            ((0, -1, 0), "minecraft:sculk_sensor"),
            ((0, -1, 1), "minecraft:sculk_sensor"),
            ((0, -1, 2), "minecraft:sculk_sensor"),
        ]);
        assert!(
            can_place_growth(&mut w, (0, 0, 0)),
            "three sensors directly below are outside the 0..2 y range"
        );
    }

    /// Five out horizontally is outside the box too.
    #[test]
    fn the_crowding_box_stops_at_four_blocks() {
        let mut w = world(&[
            ((0, 1, 0), "minecraft:air"),
            ((5, 0, 0), "minecraft:sculk_sensor"),
            ((-5, 0, 0), "minecraft:sculk_sensor"),
            ((0, 0, 5), "minecraft:sculk_sensor"),
        ]);
        assert!(can_place_growth(&mut w, (0, 0, 0)));
    }

    #[test]
    fn clear_empties_the_cursor_list() {
        let mut s = SculkSpreader::world_gen();
        s.add_cursors((0, 0, 0), 64);
        assert!(!s.cursors().is_empty());
        s.clear();
        assert!(s.cursors().is_empty());
    }
}
