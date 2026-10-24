//! `LightEngine.QueueEntry` — the long that rides beside every position in
//! the light engine's two queues.
//!
//! Transcribed from `LightEngine$QueueEntry` in 26.2's server jar. Every
//! constant below is a `ldc2_w` in that class and is asserted against its
//! decimal value in the tests, so a transcription slip is a red rather than a
//! silently wrong world.
//!
//! ```text
//!   FROM_LEVEL_BITS             = 4
//!   DIRECTION_BITS              = 6
//!   LEVEL_MASK                  = 15
//!   DIRECTIONS_MASK             = 1008      // bits 4..=9
//!   FLAG_FROM_EMPTY_SHAPE       = 1024      // bit 10
//!   FLAG_INCREASE_FROM_EMISSION = 2048      // bit 11
//! ```
//!
//! The direction bit for `d` is `1 << (d.ordinal() + 4)`, so the six bits sit
//! in `Direction.values()` order: DOWN, UP, NORTH, SOUTH, WEST, EAST.
//!
//! # Why this is a type and not six free functions
//!
//! Two of the six fields are *flags that change what a dequeue means*, and
//! the engine reads them at a distance from where they were set:
//! `isIncreaseFromEmission` decides in `propagateIncreases` whether a stored
//! level is overwritten rather than merely compared, and `isFromEmptyShape`
//! decides in `propagateIncrease` whether the occlusion test uses the source
//! block's real state or air. Both are the kind of bit that a port drops and
//! nothing notices until a slab roof leaks.

/// `LightEngine.QueueEntry.LEVEL_MASK`.
const LEVEL_MASK: u64 = 15;
/// `LightEngine.QueueEntry.DIRECTIONS_MASK` — bits 4..=9.
const DIRECTIONS_MASK: u64 = 1008;
/// `LightEngine.QueueEntry.FLAG_FROM_EMPTY_SHAPE` — bit 10.
const FLAG_FROM_EMPTY_SHAPE: u64 = 1024;
/// `LightEngine.QueueEntry.FLAG_INCREASE_FROM_EMISSION` — bit 11.
const FLAG_INCREASE_FROM_EMISSION: u64 = 2048;
/// `LightEngine.QueueEntry.FROM_LEVEL_BITS`.
const FROM_LEVEL_BITS: u32 = 4;

/// The six `Direction.values()` in ordinal order. The ordinal *is* the wire
/// position of the direction bit, so this order is load-bearing and not a
/// convenience.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(usize)]
pub enum Dir {
    Down = 0,
    Up = 1,
    North = 2,
    South = 3,
    West = 4,
    East = 5,
}

/// `LightEngine.PROPAGATION_DIRECTIONS`, which the static initialiser sets to
/// `Direction.values()` — so it is all six, in ordinal order, and the engines
/// iterate it in exactly this order.
pub const PROPAGATION_DIRECTIONS: [Dir; 6] = [
    Dir::Down,
    Dir::Up,
    Dir::North,
    Dir::South,
    Dir::West,
    Dir::East,
];

impl Dir {
    /// `Direction.ordinal()`.
    #[must_use]
    pub const fn ordinal(self) -> usize {
        self as usize
    }

    /// `Direction.getOpposite()`. `Direction.values()` pairs each axis
    /// adjacently — DOWN/UP, NORTH/SOUTH, WEST/EAST — so the opposite is the
    /// ordinal with its low bit flipped.
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

    /// `(dx, dy, dz)` — `Direction`'s normal.
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
}

/// A packed `QueueEntry`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct QueueEntry(pub u64);

impl QueueEntry {
    /// `withLevel(long, int)` — `(e & ~15) | (level & 15)`.
    #[must_use]
    const fn with_level(self, level: i32) -> Self {
        #[allow(clippy::cast_sign_loss)]
        Self((self.0 & !LEVEL_MASK) | ((level as u64) & LEVEL_MASK))
    }

    /// `withDirection(long, Direction)`.
    #[must_use]
    const fn with_direction(self, d: Dir) -> Self {
        Self(self.0 | (1u64 << (d.ordinal() as u32 + FROM_LEVEL_BITS)))
    }

    /// `withoutDirection(long, Direction)`.
    #[must_use]
    const fn without_direction(self, d: Dir) -> Self {
        Self(self.0 & !(1u64 << (d.ordinal() as u32 + FROM_LEVEL_BITS)))
    }

    /// `decreaseSkipOneDirection(level, dir)`.
    #[must_use]
    pub const fn decrease_skip_one_direction(level: i32, d: Dir) -> Self {
        Self(DIRECTIONS_MASK).without_direction(d).with_level(level)
    }

    /// `decreaseAllDirections(level)`.
    #[must_use]
    pub const fn decrease_all_directions(level: i32) -> Self {
        Self(DIRECTIONS_MASK).with_level(level)
    }

    /// `increaseLightFromEmission(level, fromEmptyShape)`.
    #[must_use]
    pub const fn increase_light_from_emission(level: i32, from_empty_shape: bool) -> Self {
        let mut e = DIRECTIONS_MASK | FLAG_INCREASE_FROM_EMISSION;
        if from_empty_shape {
            e |= FLAG_FROM_EMPTY_SHAPE;
        }
        Self(e).with_level(level)
    }

    /// `increaseSkipOneDirection(level, fromEmptyShape, dir)`.
    ///
    /// Note the asymmetry with the emission form: this one does **not** set
    /// `FLAG_INCREASE_FROM_EMISSION`, which is what stops a propagated level
    /// from overwriting a stored one in `propagateIncreases`.
    #[must_use]
    pub const fn increase_skip_one_direction(level: i32, from_empty_shape: bool, d: Dir) -> Self {
        let e = Self(DIRECTIONS_MASK).without_direction(d);
        let e = if from_empty_shape {
            Self(e.0 | FLAG_FROM_EMPTY_SHAPE)
        } else {
            e
        };
        e.with_level(level)
    }

    /// `increaseOnlyOneDirection(level, fromEmptyShape, dir)` — starts from
    /// **zero**, not from `DIRECTIONS_MASK`, so exactly one direction bit is
    /// set.
    #[must_use]
    pub const fn increase_only_one_direction(level: i32, from_empty_shape: bool, d: Dir) -> Self {
        let e = if from_empty_shape {
            Self(FLAG_FROM_EMPTY_SHAPE)
        } else {
            Self(0)
        };
        e.with_direction(d).with_level(level)
    }

    /// `increaseSkySourceInDirections(down, north, south, west, east)` —
    /// level 15, and note there is no UP: a sky source never propagates
    /// upward, because everything above it is already a source.
    #[must_use]
    pub const fn increase_sky_source_in_directions(
        down: bool,
        north: bool,
        south: bool,
        west: bool,
        east: bool,
    ) -> Self {
        let mut e = Self(0).with_level(15);
        if down {
            e = e.with_direction(Dir::Down);
        }
        if north {
            e = e.with_direction(Dir::North);
        }
        if south {
            e = e.with_direction(Dir::South);
        }
        if west {
            e = e.with_direction(Dir::West);
        }
        if east {
            e = e.with_direction(Dir::East);
        }
        e
    }

    /// `getFromLevel(long)`.
    #[must_use]
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    pub const fn from_level(self) -> i32 {
        (self.0 & LEVEL_MASK) as i32
    }

    /// `isFromEmptyShape(long)`.
    #[must_use]
    pub const fn is_from_empty_shape(self) -> bool {
        self.0 & FLAG_FROM_EMPTY_SHAPE != 0
    }

    /// `isIncreaseFromEmission(long)`.
    #[must_use]
    pub const fn is_increase_from_emission(self) -> bool {
        self.0 & FLAG_INCREASE_FROM_EMISSION != 0
    }

    /// `shouldPropagateInDirection(long, Direction)`.
    #[must_use]
    pub const fn should_propagate_in_direction(self, d: Dir) -> bool {
        self.0 & (1u64 << (d.ordinal() as u32 + FROM_LEVEL_BITS)) != 0
    }
}

/// `LightEngine.PULL_LIGHT_IN_ENTRY` — `decreaseAllDirections(1)`.
///
/// The name is the mechanism: a decrease entry at level 1 propagates into no
/// neighbour (every stored level is `> 0`, and the test is
/// `stored <= fromLevel - 1`, i.e. `stored <= 0`), so every neighbour takes
/// the `else` arm and is re-queued as an *increase* from its own stored
/// level. It is how a node that lost nothing still asks its neighbours to
/// push their light back into it.
pub const PULL_LIGHT_IN_ENTRY: QueueEntry = QueueEntry::decrease_all_directions(1);

#[cfg(test)]
mod tests {
    use super::*;

    /// The six constants, exactly as `javap -p -c -constants` prints them.
    #[test]
    fn the_packing_constants_are_the_jars() {
        assert_eq!(LEVEL_MASK, 15);
        assert_eq!(DIRECTIONS_MASK, 1008);
        assert_eq!(FLAG_FROM_EMPTY_SHAPE, 1024);
        assert_eq!(FLAG_INCREASE_FROM_EMISSION, 2048);
        assert_eq!(FROM_LEVEL_BITS, 4);
        // DIRECTIONS_MASK is exactly the six direction bits and nothing else.
        let mut m = 0u64;
        for d in PROPAGATION_DIRECTIONS {
            m |= 1u64 << (d.ordinal() as u32 + FROM_LEVEL_BITS);
        }
        assert_eq!(m, DIRECTIONS_MASK);
    }

    #[test]
    fn direction_ordinals_are_the_values_order() {
        let names = [
            Dir::Down,
            Dir::Up,
            Dir::North,
            Dir::South,
            Dir::West,
            Dir::East,
        ];
        for (i, d) in names.into_iter().enumerate() {
            assert_eq!(d.ordinal(), i);
            // `getOpposite()` is the low bit flipped, which is the property
            // `shapeOccludes` relies on when it indexes the face column.
            assert_eq!(d.opposite().ordinal(), i ^ 1);
        }
    }

    #[test]
    fn decrease_all_directions_propagates_in_all_six() {
        let e = QueueEntry::decrease_all_directions(7);
        assert_eq!(e.from_level(), 7);
        for d in PROPAGATION_DIRECTIONS {
            assert!(e.should_propagate_in_direction(d));
        }
        assert!(!e.is_from_empty_shape());
        assert!(!e.is_increase_from_emission());
    }

    #[test]
    fn decrease_skip_one_direction_clears_exactly_that_bit() {
        for skip in PROPAGATION_DIRECTIONS {
            let e = QueueEntry::decrease_skip_one_direction(11, skip);
            assert_eq!(e.from_level(), 11);
            for d in PROPAGATION_DIRECTIONS {
                assert_eq!(e.should_propagate_in_direction(d), d != skip);
            }
        }
    }

    /// The one flag `increaseSkipOneDirection` does NOT set, and the reason
    /// a port that copies the emission form into both places is wrong: the
    /// emission flag lets `propagateIncreases` raise a stored level, and a
    /// propagated entry must not.
    #[test]
    fn only_the_emission_form_carries_the_emission_flag() {
        assert!(QueueEntry::increase_light_from_emission(9, false).is_increase_from_emission());
        assert!(
            !QueueEntry::increase_skip_one_direction(9, false, Dir::Up).is_increase_from_emission()
        );
        assert!(
            !QueueEntry::increase_only_one_direction(9, false, Dir::Up).is_increase_from_emission()
        );
    }

    #[test]
    fn increase_only_one_direction_sets_exactly_one_bit() {
        for only in PROPAGATION_DIRECTIONS {
            let e = QueueEntry::increase_only_one_direction(4, true, only);
            assert_eq!(e.from_level(), 4);
            assert!(e.is_from_empty_shape());
            for d in PROPAGATION_DIRECTIONS {
                assert_eq!(e.should_propagate_in_direction(d), d == only);
            }
        }
    }

    /// A sky source pushes down and sideways and never up.
    #[test]
    fn a_sky_source_entry_never_propagates_up() {
        let e = QueueEntry::increase_sky_source_in_directions(true, true, true, true, true);
        assert_eq!(e.from_level(), 15);
        assert!(!e.should_propagate_in_direction(Dir::Up));
        for d in [Dir::Down, Dir::North, Dir::South, Dir::West, Dir::East] {
            assert!(e.should_propagate_in_direction(d));
        }
    }

    /// `SkyLightEngine`'s three static entries, from its `static {}` block.
    #[test]
    fn the_sky_engines_three_static_entries() {
        let remove_top = QueueEntry::decrease_all_directions(15);
        let remove = QueueEntry::decrease_skip_one_direction(15, Dir::Up);
        let add = QueueEntry::increase_skip_one_direction(15, false, Dir::Up);
        assert!(remove_top.should_propagate_in_direction(Dir::Up));
        assert!(!remove.should_propagate_in_direction(Dir::Up));
        assert!(!add.should_propagate_in_direction(Dir::Up));
        assert!(!add.is_increase_from_emission());
        assert_eq!(
            (
                remove_top.from_level(),
                remove.from_level(),
                add.from_level()
            ),
            (15, 15, 15)
        );
    }

    #[test]
    fn pull_light_in_entry_is_decrease_all_directions_at_one() {
        assert_eq!(PULL_LIGHT_IN_ENTRY, QueueEntry::decrease_all_directions(1));
        assert_eq!(PULL_LIGHT_IN_ENTRY.from_level(), 1);
    }

    /// The level field truncates to four bits, which is why a level of 16 is
    /// not expressible and `getLightDampeningInto`'s 16 never reaches here.
    #[test]
    fn the_level_field_is_four_bits() {
        assert_eq!(QueueEntry::decrease_all_directions(16).from_level(), 0);
        assert_eq!(QueueEntry::decrease_all_directions(15).from_level(), 15);
    }
}
