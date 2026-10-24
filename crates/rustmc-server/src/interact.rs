//! Block interaction: the packets a player breaks and places with.
//!
//! Reference (26.2), all read from the jar rather than from protocol notes:
//! `ServerboundPlayerActionPacket`, `ServerboundUseItemOnPacket`,
//! `ClientboundBlockUpdatePacket`, `ClientboundBlockChangedAckPacket`,
//! `BlockPos`, `Direction` and `FriendlyByteBuf.readBlockHitResult`.
//!
//! # The sequence number is not optional
//!
//! Every serverbound block action carries one, and the client holds its own
//! *predicted* result of that action until the matching
//! `block_changed_ack` comes back. A server that never acks leaves the
//! prediction standing forever: the block looks broken to the player who
//! broke it and to nobody else, and no later correction can dislodge it
//! because the client is still treating its own guess as authoritative.
//! Vanilla records the highest sequence during the handler and sends the ack
//! at the end of the tick — after whatever `block_update` the action
//! produced — which is the order used here.
//!
//! # `BlockPos` is packed x-then-z-then-y, and the widths are derived
//!
//! `BlockPos`'s static initialiser computes them rather than writing them
//! down: `PACKED_HORIZONTAL_LENGTH = 1 + Mth.log2(smallestEncompassingPowerOfTwo(30000000))`
//! is 26, `PACKED_Y_LENGTH = 64 - 2 * 26` is 12, `Z_OFFSET` is 12 and
//! `X_OFFSET` is 38. So the long is `x:26 | z:26 | y:12` from the top, and
//! all three are **signed** — a naive mask-and-shift reads y = 4095 for a
//! block at y = −1 and puts every dig underground somewhere near the build
//! limit.

use std::io::{self, Write};

use rustmc_proto::{frame, ids, varint::read_varint, varint::write_varint, wire};

/// `BlockPos.PACKED_HORIZONTAL_LENGTH`, derived as vanilla derives it.
const HORIZONTAL_BITS: u32 = 26;
/// `BlockPos.PACKED_Y_LENGTH` — whatever is left of the long.
const Y_BITS: u32 = 64 - 2 * HORIZONTAL_BITS;
/// `BlockPos.Z_OFFSET`.
const Z_OFFSET: u32 = Y_BITS;
/// `BlockPos.X_OFFSET`.
const X_OFFSET: u32 = Y_BITS + HORIZONTAL_BITS;

/// A block position, in block coordinates.
pub type BlockPos = (i32, i32, i32);

/// `BlockPos.asLong`.
#[must_use]
pub const fn pack_pos((x, y, z): BlockPos) -> i64 {
    let x = (x as i64) & ((1 << HORIZONTAL_BITS) - 1);
    let y = (y as i64) & ((1 << Y_BITS) - 1);
    let z = (z as i64) & ((1 << HORIZONTAL_BITS) - 1);
    (x << X_OFFSET) | (z << Z_OFFSET) | y
}

/// `BlockPos.of(long)` — the three `getX`/`getY`/`getZ` shifts, which are
/// arithmetic and therefore sign-extending.
#[must_use]
pub const fn unpack_pos(v: i64) -> BlockPos {
    // Each is "shift the field up to the top of the long, then shift it back
    // down arithmetically", which is vanilla's own sign extension.
    let x = (v << (64 - X_OFFSET - HORIZONTAL_BITS)) >> (64 - HORIZONTAL_BITS);
    let y = (v << (64 - Y_BITS)) >> (64 - Y_BITS);
    let z = (v << (64 - Z_OFFSET - HORIZONTAL_BITS)) >> (64 - HORIZONTAL_BITS);
    (x as i32, y as i32, z as i32)
}

/// `Direction.values()` by name — the column order every extracted table
/// writes, which a loader checks against rather than assumes.
pub const FACE_NAMES: [&str; 6] = ["DOWN", "UP", "NORTH", "SOUTH", "WEST", "EAST"];

/// `Direction.Plane.HORIZONTAL` — `NORTH, EAST, SOUTH, WEST`, which is **not**
/// ordinal order. `FlowingFluid.getSpread` scans in this order and returns an
/// `EnumMap`, whose iteration is ordinal (`NORTH, SOUTH, WEST, EAST`): two
/// orders in one mechanic. Re-exported by `crate::wire`.
pub const HORIZONTAL: [Face; 4] = [Face::North, Face::East, Face::South, Face::West];

/// `Direction`, by its `get3DDataValue` — which is its ordinal for all six.
///
/// The normals are read from the enum's own constructor arguments, not
/// assumed: `DOWN` is `(0,-1,0)`, `UP` `(0,1,0)`, `NORTH` `(0,0,-1)`,
/// `SOUTH` `(0,0,1)`, `WEST` `(-1,0,0)`, `EAST` `(1,0,0)`.
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
    /// `Direction.from3DDataValue`.
    #[must_use]
    pub const fn from_3d(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Down),
            1 => Some(Self::Up),
            2 => Some(Self::North),
            3 => Some(Self::South),
            4 => Some(Self::West),
            5 => Some(Self::East),
            _ => None,
        }
    }

    /// `Direction.get3DDataValue` — the inverse of [`Self::from_3d`], and
    /// also the index of this face in `Direction.values()`, which is the
    /// column order every extracted table uses.
    #[must_use]
    pub const fn to_3d(self) -> i32 {
        match self {
            Self::Down => 0,
            Self::Up => 1,
            Self::North => 2,
            Self::South => 3,
            Self::West => 4,
            Self::East => 5,
        }
    }

    /// The unit normal — where a block placed *against* this face goes.
    #[must_use]
    pub const fn normal(self) -> BlockPos {
        match self {
            Self::Down => (0, -1, 0),
            Self::Up => (0, 1, 0),
            Self::North => (0, 0, -1),
            Self::South => (0, 0, 1),
            Self::West => (-1, 0, 0),
            Self::East => (1, 0, 0),
        }
    }

    /// The `Direction` a block state property names, as vanilla spells it.
    ///
    /// `None` for anything that is not one of the six, which is how a
    /// property that turned out not to be a direction declines instead of
    /// being guessed at.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "down" => Self::Down,
            "up" => Self::Up,
            "north" => Self::North,
            "south" => Self::South,
            "west" => Self::West,
            "east" => Self::East,
            _ => return None,
        })
    }

    /// `Direction.getOpposite()`.
    ///
    /// Needed by the shape-update path, which hands each neighbour the
    /// direction that points *back* at the block that changed.
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

    /// `Direction.getAxis().isHorizontal()` — the four sides, not the top or
    /// the bottom. What decides whether a torch stands on a floor or hangs on
    /// a wall.
    ///
    /// (The doc comment "The position one step along this face's normal from
    /// `pos`" sat on this function until 2026-08-05, describing
    /// [`Self::relative`] below it. Two `#[must_use]` attributes and two doc
    /// blocks had been interleaved by an edit.)
    #[must_use]
    pub const fn axis_is_horizontal(self) -> bool {
        matches!(self, Self::North | Self::South | Self::West | Self::East)
    }

    /// `Direction.getClockWise()` — a quarter turn about the **Y** axis.
    ///
    /// # `None` is vanilla throwing, not vanilla returning `this`
    ///
    /// The no-argument form is a `tableswitch` over the ordinal covering
    /// **2 to 5 only** — `NORTH`, `SOUTH`, `WEST`, `EAST` — and its `default`
    /// branch builds an `IllegalStateException` and throws it. `DOWN` and
    /// `UP` are ordinals 0 and 1, so both land there.
    ///
    /// That is worth stating because the two-argument
    /// `getClockWise(Axis.Y)` — a *different* method — does return `this` for
    /// `UP` and `DOWN` (offsets 67-82: `if this == UP || this == DOWN return
    /// this`, else delegate to the no-argument form). A port that remembered
    /// "vertical is a fixed point" would be describing that overload while
    /// calling this one, and would silently answer where vanilla refuses.
    /// Every caller ported so far passes a `FACING` that is horizontal by
    /// construction, so `None` is unreachable today and is returned rather
    /// than asserted so that the first caller for which it is reachable finds
    /// out.
    ///
    /// Read from the pinned 26.2 jar, `javap -p -c -constants` on
    /// `net.minecraft.core.Direction`, 2026-08-05.
    #[must_use]
    pub const fn clockwise(self) -> Option<Self> {
        Some(match self {
            Self::North => Self::East,
            Self::East => Self::South,
            Self::South => Self::West,
            Self::West => Self::North,
            Self::Down | Self::Up => return None,
        })
    }

    /// `Direction.getCounterClockWise()`. Same shape as [`Self::clockwise`],
    /// same `IllegalStateException` on the vertical pair, opposite sense.
    #[must_use]
    pub const fn counter_clockwise(self) -> Option<Self> {
        Some(match self {
            Self::North => Self::West,
            Self::West => Self::South,
            Self::South => Self::East,
            Self::East => Self::North,
            Self::Down | Self::Up => return None,
        })
    }

    /// The position one step along this face's normal from `pos`.
    #[must_use]
    pub const fn relative(self, (x, y, z): BlockPos) -> BlockPos {
        let (dx, dy, dz) = self.normal();
        (x + dx, y + dy, z + dz)
    }
}

/// `ServerboundPlayerActionPacket.Action`, by ordinal from its `$values`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    StartDestroyBlock,
    AbortDestroyBlock,
    StopDestroyBlock,
    DropAllItems,
    DropItem,
    ReleaseUseItem,
    SwapItemWithOffhand,
    Stab,
    /// An ordinal this build does not know, carried rather than rejected.
    Unknown(i32),
}

impl Action {
    const fn from_ordinal(v: i32) -> Self {
        match v {
            0 => Self::StartDestroyBlock,
            1 => Self::AbortDestroyBlock,
            2 => Self::StopDestroyBlock,
            3 => Self::DropAllItems,
            4 => Self::DropItem,
            5 => Self::ReleaseUseItem,
            6 => Self::SwapItemWithOffhand,
            7 => Self::Stab,
            other => Self::Unknown(other),
        }
    }
}

/// A decoded `player_action`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerAction {
    pub action: Action,
    pub pos: BlockPos,
    /// `Direction.from3DDataValue` of a byte, which can be out of range on a
    /// hostile client — hence the `Option` rather than a panic.
    pub face: Option<Face>,
    pub sequence: i32,
}

/// Decode `player_action`: enum, position, face byte, sequence.
///
/// # Errors
/// If the body is truncated.
pub fn decode_player_action(body: &[u8]) -> io::Result<PlayerAction> {
    let mut cur = body;
    let action = Action::from_ordinal(read_varint(&mut cur)?);
    let pos = unpack_pos(wire::read_i64(&mut cur)?);
    let face = Face::from_3d(i32::from(wire::read_u8(&mut cur)?));
    let sequence = read_varint(&mut cur)?;
    Ok(PlayerAction {
        action,
        pos,
        face,
        sequence,
    })
}

/// A decoded `use_item_on` — a `BlockHitResult` and the hand that made it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UseItemOn {
    /// `InteractionHand` — 0 main hand, 1 off hand.
    pub hand: i32,
    pub pos: BlockPos,
    pub face: Option<Face>,
    /// Where on the face the cursor was, relative to the block's corner.
    pub cursor: (f32, f32, f32),
    /// `BlockHitResult.isInside` — the player's eyes are inside the block.
    pub inside: bool,
    /// The trailing boolean `readBlockHitResult` reads after `inside`.
    pub world_border_hit: bool,
    pub sequence: i32,
}

/// Decode `use_item_on`.
///
/// The body is the hand, then `readBlockHitResult` — position, face,
/// three floats, **two** booleans — then the sequence. The second boolean is
/// the one worth naming: older notes stop at `isInside`, and a decoder that
/// does too reads the sequence out of the wrong byte.
///
/// # Errors
/// If the body is truncated.
pub fn decode_use_item_on(body: &[u8]) -> io::Result<UseItemOn> {
    let mut cur = body;
    let hand = read_varint(&mut cur)?;
    let pos = unpack_pos(wire::read_i64(&mut cur)?);
    let face = Face::from_3d(read_varint(&mut cur)?);
    let cursor = (
        wire::read_f32(&mut cur)?,
        wire::read_f32(&mut cur)?,
        wire::read_f32(&mut cur)?,
    );
    let inside = wire::read_bool(&mut cur)?;
    let world_border_hit = wire::read_bool(&mut cur)?;
    let sequence = read_varint(&mut cur)?;
    Ok(UseItemOn {
        hand,
        pos,
        face,
        cursor,
        inside,
        world_border_hit,
        sequence,
    })
}

/// A decoded `use_item` — the right-click with nothing aimed at.
///
/// **Not a `BlockHitResult`.** `ServerboundUseItemPacket`'s reader is four
/// fields — `readEnum(InteractionHand)`, `readVarInt()`, `readFloat()`,
/// `readFloat()` — and nothing else; the hit result belongs to
/// [`UseItemOn`], which is a different packet. A decoder written from the
/// wrong one would consume 22 bytes of a 13-byte body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UseItem {
    /// `InteractionHand` — 0 main hand, 1 off hand.
    pub hand: i32,
    /// The same block-change acknowledgement `use_item_on` carries.
    /// `handleUseItem` calls `ackBlockChangesUpTo(sequence)` at offset 25,
    /// **before** it looks at the item at all.
    pub sequence: i32,
    /// The client's rotation at the moment of the click, in degrees.
    ///
    /// Two floats and not a byte pair: this is not the `player_rot` encoding.
    /// `handleUseItem` wraps both with `Mth.wrapDegrees` (offsets 79-95) and
    /// snaps the player to them if either differs from the server's copy, so
    /// they are authoritative rather than advisory.
    pub y_rot: f32,
    pub x_rot: f32,
}

/// Decode `use_item` — `minecraft:use_item`, id
/// [`rustmc_proto::ids::play::sb::USE_ITEM`].
///
/// # Errors
/// If the body is truncated.
pub fn decode_use_item(body: &[u8]) -> io::Result<UseItem> {
    let mut cur = body;
    let hand = read_varint(&mut cur)?;
    let sequence = read_varint(&mut cur)?;
    let y_rot = wire::read_f32(&mut cur)?;
    let x_rot = wire::read_f32(&mut cur)?;
    Ok(UseItem {
        hand,
        sequence,
        y_rot,
        x_rot,
    })
}

/// `Mth.wrapDegrees(float)` — `f % 360`, then `+360` if under `-180` and
/// `-360` if over `180`.
///
/// Java's `%` on floats keeps the sign of the dividend, which Rust's `%`
/// also does, so the remainder is the same in both and only the two
/// corrections need writing.
#[must_use]
pub fn wrap_degrees(value: f32) -> f32 {
    let mut wrapped = value % 360.0;
    if wrapped >= 180.0 {
        wrapped -= 360.0;
    }
    if wrapped < -180.0 {
        wrapped += 360.0;
    }
    wrapped
}

/// `block_update`'s body: the packed position, then the global state id.
///
/// # Errors
/// On a write failure, which for a `Vec` cannot happen.
pub fn block_update_body(pos: BlockPos, state_id: i32) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    wire::write_i64(&mut out, pack_pos(pos))?;
    write_varint(&mut out, state_id)?;
    Ok(out)
}

/// `level_event`'s body.
///
/// `ClientboundLevelEventPacket.write`, from the bytecode: `writeInt(type)`,
/// `writeBlockPos(pos)`, `writeInt(data)`, `writeBoolean(globalEvent)`.
///
/// **The two ints are big-endian `int`s and not varints**, which is the trap
/// here: every other id this server writes is a varint, and a varint `2001`
/// decodes as a different number in a field the client reads as four bytes.
///
/// `globalEvent` is the packet's fourth field and is `false` for everything
/// this server produces — it is what makes an event audible across the whole
/// dimension rather than within 64 blocks, and only the ender-dragon and
/// wither events set it. Passed rather than defaulted so the flag is visible
/// at the one call site that could ever want the other value.
///
/// # Errors
/// On a write failure, which for a `Vec` cannot happen.
pub fn level_event_body(event: i32, pos: BlockPos, data: i32, global: bool) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    wire::write_i32(&mut out, event)?;
    wire::write_i64(&mut out, pack_pos(pos))?;
    wire::write_i32(&mut out, data)?;
    wire::write_bool(&mut out, global)?;
    Ok(out)
}

/// `block_changed_ack`'s body: one varint.
///
/// # Errors
/// On a write failure, which for a `Vec` cannot happen.
pub fn block_changed_ack_body(sequence: i32) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    write_varint(&mut out, sequence)?;
    Ok(out)
}

/// Send `block_changed_ack`.
///
/// # Errors
/// On I/O failure.
pub fn send_block_changed_ack(
    stream: &mut impl Write,
    sequence: i32,
    comp: Option<i32>,
) -> io::Result<()> {
    frame::write_frame(
        stream,
        comp,
        ids::play::cb::BLOCK_CHANGED_ACK,
        &block_changed_ack_body(sequence)?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The packing is `x:26 | z:26 | y:12`, and every field is signed. A
    /// mask-and-shift that forgets the sign reads y = −1 as 4095, which is
    /// above the build limit — so a dig underground writes into the sky.
    #[test]
    fn a_block_pos_round_trips_through_a_long_with_its_signs() {
        for pos in [
            (0, 0, 0),
            (1, 2, 3),
            (-1, -1, -1),
            (8, -64, 8),
            (-1000, 319, 2000),
            (30_000_000, -64, -30_000_000),
        ] {
            assert_eq!(unpack_pos(pack_pos(pos)), pos, "round trip of {pos:?}");
        }
    }

    /// The one that catches a naive decoder.
    #[test]
    fn y_is_sign_extended_not_masked() {
        let (_, y, _) = unpack_pos(pack_pos((0, -1, 0)));
        assert_eq!(y, -1, "a masked read would say 4095");
    }

    /// The normals come from `Direction`'s own constructor arguments.
    #[test]
    fn a_face_points_the_way_its_enum_says() {
        assert_eq!(Face::from_3d(0), Some(Face::Down));
        assert_eq!(Face::from_3d(1), Some(Face::Up));
        assert_eq!(Face::from_3d(5), Some(Face::East));
        assert_eq!(Face::from_3d(6), None, "out of range is not a face");
        assert_eq!(Face::Up.opposite(), Face::Down);
        assert_eq!(Face::North.opposite(), Face::South);
        assert_eq!(Face::East.opposite(), Face::West);
        for face in [
            Face::Down,
            Face::Up,
            Face::North,
            Face::South,
            Face::West,
            Face::East,
        ] {
            assert_eq!(face.opposite().opposite(), face);
            assert_eq!(
                face.opposite().relative(face.relative((0, 0, 0))),
                (0, 0, 0)
            );
        }
        assert_eq!(Face::Up.relative((4, 5, 6)), (4, 6, 6));
        assert_eq!(Face::North.relative((4, 5, 6)), (4, 5, 5));
        assert_eq!(Face::West.relative((4, 5, 6)), (3, 5, 6));
    }

    #[test]
    fn a_player_action_decodes_in_wire_order() {
        let mut body = Vec::new();
        write_varint(&mut body, 2).expect("buf"); // STOP_DESTROY_BLOCK
        wire::write_i64(&mut body, pack_pos((10, -3, -7))).expect("buf");
        wire::write_u8(&mut body, 1).expect("buf"); // UP
        write_varint(&mut body, 42).expect("buf");

        let a = decode_player_action(&body).expect("decoded");
        assert_eq!(a.action, Action::StopDestroyBlock);
        assert_eq!(a.pos, (10, -3, -7));
        assert_eq!(a.face, Some(Face::Up));
        assert_eq!(a.sequence, 42);
    }

    /// `readBlockHitResult` reads **two** booleans. Dropping the second
    /// leaves the sequence one byte off, which reads as a wildly wrong
    /// sequence and desynchronises every prediction after it.
    #[test]
    fn use_item_on_reads_both_trailing_booleans() {
        let mut body = Vec::new();
        write_varint(&mut body, 0).expect("buf"); // main hand
        wire::write_i64(&mut body, pack_pos((1, 64, 2))).expect("buf");
        write_varint(&mut body, 1).expect("buf"); // UP
        wire::write_f32(&mut body, 0.5).expect("buf");
        wire::write_f32(&mut body, 1.0).expect("buf");
        wire::write_f32(&mut body, 0.25).expect("buf");
        wire::write_bool(&mut body, false).expect("buf"); // inside
        wire::write_bool(&mut body, false).expect("buf"); // world border
        write_varint(&mut body, 300).expect("buf"); // two bytes as a varint

        let u = decode_use_item_on(&body).expect("decoded");
        assert_eq!(u.pos, (1, 64, 2));
        assert_eq!(u.face, Some(Face::Up));
        assert_eq!(
            u.sequence, 300,
            "one boolean short reads a different number"
        );
        assert!(!u.inside);
        // And placing against the top face goes one block up.
        assert_eq!(u.face.expect("a face").relative(u.pos), (1, 65, 2));
    }

    /// `level_event`'s body, pinned against a real recorded observation rather
    /// than against the writer that produced it.
    ///
    /// The wire order is `writeInt(type)`, `writeBlockPos(pos)`,
    /// `writeInt(data)`, `writeBoolean(globalEvent)` —
    /// `ClientboundLevelEventPacket.write` offsets 2-32. **Both ints are raw
    /// four-byte big-endian, not varints**, which is the mistake this test
    /// exists for: the type is 2001 and a varint of 2001 is two bytes, so a
    /// varint version produces a body six bytes short that decodes into
    /// nonsense from the position onward.
    ///
    /// The vector is `010-break-block`'s, from every vanilla recording on this
    /// box: `event 2001, at [1, -1, 0], data 9`, where 9 is the destroyed
    /// block's state id. 833 observations across 32 recordings and all of them
    /// are this event; see `docs/FINDINGS.md` 2026-09-05.
    #[test]
    fn a_level_event_is_two_raw_ints_around_a_packed_position() {
        let body = level_event_body(2001, (1, -1, 0), 9, false).expect("buf");
        assert_eq!(body.len(), 4 + 8 + 4 + 1);
        assert_eq!(&body[..4], &2001_i32.to_be_bytes(), "type, raw not varint");
        let packed = i64::from_be_bytes(body[4..12].try_into().expect("eight bytes"));
        assert_eq!(packed, pack_pos((1, -1, 0)));
        assert_eq!(&body[12..16], &9_i32.to_be_bytes(), "data, raw not varint");
        assert_eq!(body[16], 0, "globalEvent");
        // And the flag really is the last byte rather than a constant: the
        // global form is what `globalLevelEvent` sends when the
        // `global_sound_events` game rule is on, and it is a different packet
        // to a client.
        let global = level_event_body(2001, (1, -1, 0), 9, true).expect("buf");
        assert_eq!(global[16], 1);
        assert_eq!(&global[..16], &body[..16], "only the flag moved");
    }

    #[test]
    fn a_block_update_is_a_packed_long_then_a_varint_state() {
        let body = block_update_body((-1, -64, 300), 1234).expect("buf");
        let mut cur = &body[..];
        assert_eq!(
            unpack_pos(wire::read_i64(&mut cur).expect("long")),
            (-1, -64, 300)
        );
        assert_eq!(read_varint(&mut cur).expect("state"), 1234);
        assert!(cur.is_empty(), "no trailing bytes");
    }
}

#[cfg(test)]
mod use_item_tests {
    use super::*;

    /// The body is hand, sequence, then two **floats** — 4 + 4 bytes at the
    /// tail and not a byte pair. Two varints and eight bytes.
    ///
    /// The vector is built by hand rather than round-tripped, because a
    /// round-trip through this module's own encoder would pass whatever the
    /// decoder did.
    #[test]
    fn the_body_is_two_varints_and_two_floats() {
        let mut body = vec![0x00, 0x09];
        body.extend_from_slice(&90.0f32.to_be_bytes());
        body.extend_from_slice(&(-12.5f32).to_be_bytes());
        assert_eq!(body.len(), 10);
        let got = decode_use_item(&body).expect("decodes");
        assert_eq!(
            got,
            UseItem {
                hand: 0,
                sequence: 9,
                y_rot: 90.0,
                x_rot: -12.5,
            }
        );
    }

    /// **The `use_item_on` decoder must not be reached for this packet.**
    /// Its body starts with a hand and then reads a packed long, so given a
    /// 10-byte `use_item` body it either fails or invents a position — and a
    /// decoder that read the hit result would consume 22 bytes of a 10-byte
    /// body. This is the confusion the two names invite, pinned.
    #[test]
    fn the_use_item_on_decoder_cannot_read_a_use_item_body() {
        let mut body = vec![0x00, 0x09];
        body.extend_from_slice(&90.0f32.to_be_bytes());
        body.extend_from_slice(&(-12.5f32).to_be_bytes());
        assert!(
            decode_use_item_on(&body).is_err(),
            "a 10-byte use_item body is short for a use_item_on"
        );
    }

    #[test]
    fn a_truncated_body_is_an_error_and_not_a_zero() {
        assert!(decode_use_item(&[0x00, 0x09, 0x00]).is_err());
        assert!(decode_use_item(&[]).is_err());
    }

    /// `Mth.wrapDegrees` — `>= 180` subtracts a turn, then `< -180` adds one,
    /// in that order. The boundary is the whole content of the function:
    /// exactly 180 wraps to -180 and exactly -180 does not wrap.
    #[test]
    fn wrap_degrees_is_the_half_open_interval() {
        assert!((wrap_degrees(0.0) - 0.0).abs() < f32::EPSILON);
        assert!((wrap_degrees(179.0) - 179.0).abs() < f32::EPSILON);
        assert!((wrap_degrees(180.0) + 180.0).abs() < f32::EPSILON);
        assert!((wrap_degrees(-180.0) + 180.0).abs() < f32::EPSILON);
        assert!((wrap_degrees(190.0) + 170.0).abs() < f32::EPSILON);
        assert!((wrap_degrees(360.0) - 0.0).abs() < f32::EPSILON);
        assert!((wrap_degrees(-190.0) - 170.0).abs() < f32::EPSILON);
        // Two turns: `frem` leaves 10 and neither correction fires.
        assert!((wrap_degrees(730.0) - 10.0).abs() < f32::EPSILON);
    }
}
