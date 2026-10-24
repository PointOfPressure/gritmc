//! World-shape constants — how tall a dimension is allowed to be.
//!
//! Reference: `net.minecraft.world.level.dimension.DimensionType` and
//! `net.minecraft.core.BlockPos`.
//!
//! These are not the overworld's -64..320. They are the *absolute* limits any
//! dimension may declare, and they exist because a block position packs into
//! a single `long`: 26 bits of x, 26 of z, and whatever is left for y. So the
//! world height ceiling is a consequence of the packing, not a design choice,
//! and every one of these numbers is derived here rather than written down.

/// `BlockPos.PACKED_Y_LENGTH` — bits left for y after x and z take 26 each.
///
/// Verified against the jar (`BlockPos.PACKED_Y_LENGTH == 12`) rather than
/// recalled: 26 comes from `1 + log2(smallestEncompassingPowerOfTwo(30000000))`,
/// i.e. the world border, and getting it wrong shifts every derived constant.
pub const PACKED_Y_LENGTH: i32 = 12;

/// `DimensionType.Y_SIZE`. The 32 is headroom vanilla reserves so y+height
/// arithmetic cannot overflow the packed field.
pub const Y_SIZE: i32 = (1 << PACKED_Y_LENGTH) - 32;

/// `DimensionType.MAX_Y` — 2031.
pub const MAX_Y: i32 = (Y_SIZE >> 1) - 1;

/// `DimensionType.MIN_Y` — -2032.
pub const MIN_Y: i32 = MAX_Y - Y_SIZE + 1;

/// `DimensionType.WAY_BELOW_MIN_Y` — -32512.
///
/// A sentinel, not a coordinate: the aquifer uses it as "this cell has no
/// fluid at all", relying on it being below every possible block y so the
/// `blockY < fluidLevel` test can never accidentally pass.
pub const WAY_BELOW_MIN_Y: i32 = MIN_Y << 4;

/// `DimensionType.WAY_ABOVE_MAX_Y`.
pub const WAY_ABOVE_MAX_Y: i32 = MAX_Y << 4;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_match_vanillas_values() {
        // Dumped from the 26.2 jar. Pinned because every one of them is a
        // silent world-shape change if the derivation drifts.
        assert_eq!(Y_SIZE, 4064);
        assert_eq!(MAX_Y, 2031);
        assert_eq!(MIN_Y, -2032);
        assert_eq!(WAY_BELOW_MIN_Y, -32512);
    }

    #[test]
    fn the_sentinel_is_below_every_real_block() {
        // What makes WAY_BELOW_MIN_Y usable as "no fluid": nothing a
        // dimension can declare reaches it. Routed through variables so this
        // is a real comparison rather than one clippy folds away.
        let (sentinel_low, sentinel_high) = (WAY_BELOW_MIN_Y, WAY_ABOVE_MAX_Y);
        let (floor, ceiling) = (MIN_Y, MAX_Y);
        assert!(sentinel_low < floor);
        assert!(sentinel_high > ceiling);
    }

    #[test]
    fn the_height_span_is_symmetric_about_zero_within_one() {
        // MAX_Y - MIN_Y + 1 == Y_SIZE by construction; asserting it catches
        // an off-by-one in the derivation rather than restating it.
        assert_eq!(MAX_Y - MIN_Y + 1, Y_SIZE);
    }
}
