//! `BellBlock` and `BellBlockEntity` — the one block-entity behaviour that
//! puts two packets on the wire for one click.
//!
//! Transcribed from `BellBlock.onHit` (offsets 0-87), `.isProperHit`
//! (0-129), `.attemptToRing(Entity, Level, BlockPos, Direction)` (0-83),
//! `BellBlockEntity.onHit(Direction)` (0-50), `.triggerEvent(int, int)`
//! (0-40) and `.tick` (0-128), in the pinned 26.2 jar.
//!
//! # The mechanic does not exist without the block entity
//!
//! `attemptToRing`'s guard at offsets 7-19 is
//! `!level.isClientSide() && getBlockEntity(pos) instanceof BellBlockEntity`.
//! A bell block with no block entity rings nothing, plays nothing and returns
//! false, which is why this lives here rather than beside the other block
//! interactions.
//!
//! # What one successful ring emits, in this order
//!
//! 1. `BellBlockEntity.onHit(direction)`, whose last act is
//!    `Level.blockEvent(pos, block, 1, direction.get3DDataValue())` — a
//!    `block_event` packet with `b0 = 1` and `b1` the face's 3D data value;
//! 2. `Level.playSound(null, pos, SoundEvents.BELL_BLOCK, BLOCKS, 2.0f,
//!    1.0f)` — `fconst_2` and `fconst_1` at offsets 66-67, so **volume 2 and
//!    pitch 1**, not the 1.0/1.0 a guess would give;
//! 3. `Level.gameEvent(entity, GameEvent.BLOCK_CHANGE, pos)`.
//!
//! The sound is sent with a `null` source entity, so it reaches the ringing
//! player too — a bell is not one of the "everyone but you" sounds.
//!
//! # `isProperHit` is skipped for anything but a player's own click
//!
//! `BellBlock.onHit` takes a boolean (`isHand` at local 5) and the whole
//! `isProperHit` test is behind it (offsets 12-14). `useWithoutItem` passes
//! `true`; a projectile, an explosion and a redstone signal all reach
//! `attemptToRing` directly and ring a bell from any angle.

use crate::interact::Face;

/// `SoundEvents.BELL_BLOCK`'s volume at `attemptToRing` — `fconst_2`.
pub const RING_VOLUME: f32 = 2.0;

/// Its pitch — `fconst_1`.
pub const RING_PITCH: f32 = 1.0;

/// The `b0` of the `block_event` a ring queues — `iconst_1` at
/// `BellBlockEntity.onHit` offset 42, and the only value
/// `BellBlockEntity.triggerEvent` accepts (its `if_icmpne 34` falls through
/// to `BlockEntity.triggerEvent`, whose body is `iconst_0; ireturn`).
pub const RING_EVENT: i32 = 1;

/// How long the bell shakes, in ticks — `bipush 50` at `tick` offset 21.
pub const SHAKE_TICKS: i32 = 50;

/// How long after a ring the resonation may start — `iconst_5` at offset 40.
pub const RESONATE_AFTER: i32 = 5;

/// How long a resonation lasts — `bipush 40` at offset 92.
pub const RESONATION_TICKS: i32 = 40;

/// The y-coordinate within the block below which a side hit does not count —
/// `ldc2_w 0.8123999834060669d` at `isProperHit` offset 11.
///
/// Written as the exact double the constant pool holds, not as `0.8124`. The
/// two differ in the last bits and the comparison is `dcmpl` against a
/// `hit.y - pos.getY()`, so a rounded constant moves the boundary.
pub const PROPER_HIT_MAX_Y: f64 = 0.812_399_983_406_066_9;

/// `BellAttachType`, in declaration order.
///
/// The order is load-bearing: `BellBlock$1`'s synthetic switch map assigns
/// `FLOOR -> 1`, `SINGLE_WALL -> 2`, `DOUBLE_WALL -> 3`, `CEILING -> 4`, and
/// `isProperHit`'s `tableswitch` branches 1, {2,3} and 4 to three different
/// answers. Read off `BellBlock$1`'s `static {}` rather than off the enum's
/// own ordinals, because the two need not agree and only the map's numbering
/// is what the switch indexes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BellAttach {
    /// Standing on the floor.
    Floor,
    /// Hung on one wall.
    SingleWall,
    /// Hung between two walls.
    DoubleWall,
    /// Hung from the ceiling.
    Ceiling,
}

impl BellAttach {
    /// The `attachment` block state value.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "floor" => Self::Floor,
            "single_wall" => Self::SingleWall,
            "double_wall" => Self::DoubleWall,
            "ceiling" => Self::Ceiling,
            _ => return None,
        })
    }
}

/// `BellBlock.isProperHit(BlockState, Direction, double)` — offsets 0-129.
///
/// `hit_y` is `hit.getLocation().y - pos.getY()`, the height of the click
/// within the block.
///
/// Two gates, in this order:
///
/// * **any hit on the top or bottom face is refused**, and so is any side hit
///   above [`PROPER_HIT_MAX_Y`] — offsets 0-19, one `if` with an `||`. Note
///   the y test is `> 0.8124` refuses, so exactly 0.8124 is accepted;
/// * then the attachment decides. A floor bell wants the hit **along** its
///   facing axis; a wall bell wants it **across**; a ceiling bell takes any
///   side hit at all.
#[must_use]
pub fn is_proper_hit(facing: Face, attach: BellAttach, hit_face: Face, hit_y: f64) -> bool {
    if !hit_face.axis_is_horizontal() || hit_y > PROPER_HIT_MAX_Y {
        return false;
    }
    let same_axis = axis_of(facing) == axis_of(hit_face);
    match attach {
        BellAttach::Floor => same_axis,
        BellAttach::SingleWall | BellAttach::DoubleWall => !same_axis,
        BellAttach::Ceiling => true,
    }
}

/// `Direction.getAxis()`, as the two letters that can differ here.
///
/// A private helper rather than a method on [`Face`]: `axis_is_horizontal`
/// already exists there and answers the other question this function needs,
/// and adding an `Axis` type for one comparison would be more surface than
/// the comparison is worth.
const fn axis_of(f: Face) -> u8 {
    match f {
        Face::Down | Face::Up => b'y',
        Face::North | Face::South => b'z',
        Face::West | Face::East => b'x',
    }
}

/// What a successful ring emits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ring {
    /// The `b0`/`b1` of the `block_event`: always [`RING_EVENT`], and the
    /// direction's `get3DDataValue`.
    pub event: (i32, i32),
    /// `SoundEvents.BELL_BLOCK` at [`RING_VOLUME`] and [`RING_PITCH`].
    pub volume: f32,
    /// The pitch.
    pub pitch: f32,
}

/// `BellBlock.attemptToRing(Entity, Level, BlockPos, Direction)` — offsets
/// 0-83.
///
/// `has_block_entity` is the `instanceof BellBlockEntity` at offset 16. A
/// bell without one produces `None`, which is a refusal and not a silent
/// success.
///
/// `direction` is nullable in vanilla and the null case is **not** "no
/// direction": offsets 29-48 replace it with the block's own `FACING`, which
/// is how an explosion rings a bell in the direction it happens to point.
#[must_use]
pub fn attempt_to_ring(
    has_block_entity: bool,
    facing: Face,
    direction: Option<Face>,
) -> Option<Ring> {
    if !has_block_entity {
        return None;
    }
    let dir = direction.unwrap_or(facing);
    Some(Ring {
        event: (RING_EVENT, dir.to_3d()),
        volume: RING_VOLUME,
        pitch: RING_PITCH,
    })
}

/// `BellBlock.onHit(Level, BlockState, BlockHitResult, Player, boolean)` —
/// offsets 0-87. The player-click entry point.
///
/// `is_hand` is the boolean `useWithoutItem` passes as `true`; when it is
/// false the [`is_proper_hit`] test is skipped entirely, which is the path a
/// projectile takes.
///
/// The return in vanilla is `true` whenever the hit was proper — **even when
/// `attemptToRing` returned false**, offsets 48-85, so a bell whose block
/// entity is missing still answers `InteractionResult.SUCCESS` to the click.
/// That is transcribed as the `bool` here being about the *hit* and the
/// `Option` being about the *ring*.
#[must_use]
pub fn on_hit(
    has_block_entity: bool,
    facing: Face,
    attach: BellAttach,
    hit_face: Face,
    hit_y: f64,
    is_hand: bool,
) -> (bool, Option<Ring>) {
    let proper = !is_hand || is_proper_hit(facing, attach, hit_face, hit_y);
    if !proper {
        return (false, None);
    }
    (
        true,
        attempt_to_ring(has_block_entity, facing, Some(hit_face)),
    )
}

/// The client-visible state a bell carries between ticks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BellState {
    /// `shaking`.
    pub shaking: bool,
    /// `ticks`, counted only while shaking.
    pub ticks: i32,
    /// `resonating`.
    pub resonating: bool,
    /// `resonationTicks`.
    pub resonation_ticks: i32,
}

impl BellState {
    /// `BellBlockEntity.triggerEvent(int b0, int b1)` — offsets 0-40.
    ///
    /// Accepts **only** `b0 == 1`; anything else falls through to
    /// `BlockEntity.triggerEvent`, which is `iconst_0; ireturn`. On accepting
    /// it zeroes `resonationTicks` and `ticks` and sets `shaking`, in that
    /// order, and records the direction — so a bell hit again mid-shake
    /// restarts its animation rather than extending it.
    pub fn trigger_event(&mut self, b0: i32, _b1: i32) -> bool {
        if b0 != RING_EVENT {
            return false;
        }
        self.resonation_ticks = 0;
        self.ticks = 0;
        self.shaking = true;
        true
    }

    /// `BellBlockEntity.tick` — offsets 0-128, the half that needs no entity
    /// list.
    ///
    /// `raiders_nearby` is `areRaidersNearby(pos, nearbyEntities)` at offset
    /// 56. This server has no raiders, so a caller passes `false` and the
    /// resonation never starts; the branch is transcribed rather than dropped
    /// because its *absence* would be indistinguishable from a bell that
    /// never sees one.
    ///
    /// Returns whether `SoundEvents.BELL_RESONATE` should be played this
    /// tick, and whether the resonation's end action should run.
    pub fn tick(&mut self, raiders_nearby: bool) -> (bool, bool) {
        if self.shaking {
            self.ticks += 1;
        }
        // `>= 50` stops the shake, and note it also zeroes `ticks`, so the
        // two `if`s below see 0 rather than 50 on that tick.
        if self.ticks >= SHAKE_TICKS {
            self.shaking = false;
            self.ticks = 0;
        }
        let mut resonate = false;
        if self.ticks >= RESONATE_AFTER && self.resonation_ticks == 0 && raiders_nearby {
            self.resonating = true;
            resonate = true;
        }
        let mut ended = false;
        if self.resonating {
            if self.resonation_ticks < RESONATION_TICKS {
                self.resonation_ticks += 1;
            } else {
                ended = true;
                self.resonating = false;
            }
        }
        (resonate, ended)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A floor bell wants the hit along its facing axis; a wall bell wants it
    /// across. The same click is proper for one and not the other, which is
    /// what makes this a real test of the switch map rather than of a
    /// constant.
    #[test]
    fn a_floor_bell_and_a_wall_bell_want_opposite_axes() {
        // Facing north (z axis), hit on the north face (z axis).
        assert!(is_proper_hit(
            Face::North,
            BellAttach::Floor,
            Face::North,
            0.5
        ));
        assert!(!is_proper_hit(
            Face::North,
            BellAttach::SingleWall,
            Face::North,
            0.5
        ));
        // Hit on the east face (x axis) — across the facing.
        assert!(!is_proper_hit(
            Face::North,
            BellAttach::Floor,
            Face::East,
            0.5
        ));
        assert!(is_proper_hit(
            Face::North,
            BellAttach::SingleWall,
            Face::East,
            0.5
        ));
        assert!(is_proper_hit(
            Face::North,
            BellAttach::DoubleWall,
            Face::East,
            0.5
        ));
        // A ceiling bell takes either.
        assert!(is_proper_hit(
            Face::North,
            BellAttach::Ceiling,
            Face::East,
            0.5
        ));
        assert!(is_proper_hit(
            Face::North,
            BellAttach::Ceiling,
            Face::North,
            0.5
        ));
    }

    /// The top and bottom faces are refused for every attachment, including
    /// the ceiling one that accepts every side.
    #[test]
    fn a_vertical_face_is_never_a_proper_hit() {
        for attach in [
            BellAttach::Floor,
            BellAttach::SingleWall,
            BellAttach::DoubleWall,
            BellAttach::Ceiling,
        ] {
            assert!(!is_proper_hit(Face::North, attach, Face::Up, 0.5));
            assert!(!is_proper_hit(Face::North, attach, Face::Down, 0.5));
        }
    }

    /// The height boundary is `>` and not `>=`, so a hit exactly at the
    /// constant is accepted. One ulp above it is not.
    #[test]
    fn the_height_boundary_accepts_the_constant_itself() {
        assert!(is_proper_hit(
            Face::North,
            BellAttach::Ceiling,
            Face::East,
            PROPER_HIT_MAX_Y
        ));
        assert!(!is_proper_hit(
            Face::North,
            BellAttach::Ceiling,
            Face::East,
            PROPER_HIT_MAX_Y + f64::EPSILON
        ));
    }

    /// A ring is `block_event(1, face3d)` and a sound at volume 2 pitch 1.
    /// The volume is the part a guess gets wrong.
    #[test]
    fn a_ring_is_event_one_and_a_sound_at_volume_two() {
        let r = attempt_to_ring(true, Face::North, Some(Face::East)).expect("rings");
        assert_eq!(r.event, (1, Face::East.to_3d()));
        assert_eq!(r.event.1, 5, "east is 5 in Direction.values()");
        assert!((r.volume - 2.0).abs() < f32::EPSILON);
        assert!((r.pitch - 1.0).abs() < f32::EPSILON);
    }

    /// No block entity, no ring — and a bell with one rings, so this is a
    /// test of the guard rather than of the constructor.
    #[test]
    fn a_bell_with_no_block_entity_does_not_ring() {
        assert!(attempt_to_ring(false, Face::North, Some(Face::East)).is_none());
        assert!(attempt_to_ring(true, Face::North, Some(Face::East)).is_some());
    }

    /// A null direction is replaced by the block's own facing, not dropped.
    #[test]
    fn a_null_direction_becomes_the_blocks_facing() {
        let r = attempt_to_ring(true, Face::South, None).expect("rings");
        assert_eq!(r.event.1, Face::South.to_3d());
    }

    /// `isHand` false skips the proper-hit test entirely, so a projectile
    /// rings a floor bell from the wrong axis and a hand does not.
    #[test]
    fn a_projectile_rings_a_bell_a_hand_could_not() {
        let (hit, ring) = on_hit(true, Face::North, BellAttach::Floor, Face::East, 0.5, true);
        assert!(!hit);
        assert!(ring.is_none());
        let (hit, ring) = on_hit(true, Face::North, BellAttach::Floor, Face::East, 0.5, false);
        assert!(hit);
        assert!(ring.is_some());
    }

    /// A proper hit on a bell with no block entity still reports the hit as
    /// handled, which is what makes the click `SUCCESS` rather than `PASS`.
    #[test]
    fn a_proper_hit_is_handled_even_when_nothing_rings() {
        let (hit, ring) = on_hit(
            false,
            Face::North,
            BellAttach::Floor,
            Face::North,
            0.5,
            true,
        );
        assert!(hit);
        assert!(ring.is_none());
    }

    /// `triggerEvent` takes only `b0 == 1`, and taking it restarts the shake
    /// from zero rather than extending it.
    #[test]
    fn trigger_event_accepts_only_one_and_restarts_the_shake() {
        let mut s = BellState {
            shaking: true,
            ticks: 30,
            resonating: false,
            resonation_ticks: 12,
        };
        assert!(!s.trigger_event(0, 3));
        assert!(!s.trigger_event(2, 3));
        assert_eq!(s.ticks, 30, "a refused event changes nothing");
        assert!(s.trigger_event(1, 3));
        assert_eq!(s.ticks, 0);
        assert_eq!(s.resonation_ticks, 0);
        assert!(s.shaking);
    }

    /// The shake stops at fifty ticks and zeroes its counter on that tick, so
    /// the resonation test below it sees 0 and not 50.
    #[test]
    fn the_shake_stops_at_fifty_and_zeroes_the_counter() {
        let mut s = BellState::default();
        s.trigger_event(1, 0);
        for _ in 0..SHAKE_TICKS {
            s.tick(false);
        }
        assert!(!s.shaking);
        assert_eq!(s.ticks, 0);
    }

    /// With no raiders the resonation never starts, which is this server's
    /// state today. With raiders it does, five ticks in — so the branch is
    /// live and the `false` is a fact about the world rather than about the
    /// code.
    #[test]
    fn the_resonation_needs_raiders_and_five_ticks() {
        let mut s = BellState::default();
        s.trigger_event(1, 0);
        for _ in 0..10 {
            assert_eq!(s.tick(false), (false, false));
        }
        assert!(!s.resonating);

        let mut s = BellState::default();
        s.trigger_event(1, 0);
        for _ in 0..4 {
            assert!(!s.tick(true).0, "four ticks is not five");
        }
        assert!(s.tick(true).0);
        assert!(s.resonating);
    }
}
