//! `ConduitBlockEntity.serverTick` — the frame, the range and the three
//! sounds.
//!
//! Transcribed from `.serverTick` (offsets 0-210), `.updateShape(Level,
//! BlockPos, List)` (0-295), `.updateHunting` (0-20) and `.applyEffects`
//! in the pinned 26.2 jar.
//!
//! # Two different periods and a random third
//!
//! * **every 40 ticks** (`ldc2_w 40l` at offset 24) the frame is recounted,
//!   the activate/deactivate sound fires on a change, hunting is updated and
//!   the effect is applied;
//! * **every 80 ticks** (offset 140) an active conduit plays
//!   `CONDUIT_AMBIENT`;
//! * and a third sound, `CONDUIT_AMBIENT_SHORT`, fires when the game time
//!   passes `nextAmbientSoundActivation`, which is then pushed to
//!   `now + 60 + nextInt(40)` (offsets 173-193). **That draw is from
//!   `level.getRandom()` and happens only on an active conduit**, so a
//!   conduit is a source of RNG consumption that a world without one does
//!   not have.
//!
//! # `updateShape` refuses before it counts
//!
//! Its first act is the **3x3x3 of water** around the conduit (offsets
//! 6-71): every one of the 27 positions must satisfy `Level.isWaterAt`, and
//! a single dry one returns `false` **without clearing anything it had
//! already found** — the `clear()` is at offset 0, before the water loop, so
//! the list is empty either way. A conduit lifted out of the water reports
//! no frame at all rather than a partial one.

/// The blocks a conduit frame is made of — `VALID_BLOCKS`, in its `static {}`
/// order.
pub const VALID_BLOCKS: [&str; 4] = [
    "minecraft:prismarine",
    "minecraft:prismarine_bricks",
    "minecraft:sea_lantern",
    "minecraft:dark_prismarine",
];

/// How many frame blocks activate a conduit — `bipush 16` at `updateShape`
/// offset 285.
pub const ACTIVATE_BLOCKS: usize = 16;

/// How many make it hunt — `bipush 42`, in `updateHunting` and again in
/// `serverTick`'s call to `updateAndAttackTarget`.
pub const HUNT_BLOCKS: usize = 42;

/// The frame recount period.
pub const SHAPE_PERIOD: i64 = 40;

/// The ambient sound period.
pub const AMBIENT_PERIOD: i64 = 80;

/// `MobEffectInstance`'s duration for conduit power — `sipush 260` in
/// `applyEffects`.
pub const EFFECT_DURATION: i32 = 260;

/// Whether `(dx, dy, dz)` is a frame position.
///
/// Offsets 138-190 as three clauses joined by `||`, each "one coordinate is
/// zero and another is ±2":
///
/// ```text
///   (dx == 0 && (|dy| == 2 || |dz| == 2))
/// || (dy == 0 && (|dx| == 2 || |dz| == 2))
/// || (dz == 0 && (|dx| == 2 || |dy| == 2))
/// ```
///
/// The scan itself walks `-2..=2` on all three axes, so this is the filter
/// that turns a 5x5x5 cube into the twelve edge lines a conduit frame is.
/// Its maximum is exactly [`HUNT_BLOCKS`], which is the check the count
/// threshold can be read against rather than trusted.
#[must_use]
pub const fn is_frame_offset(dx: i32, dy: i32, dz: i32) -> bool {
    (dx == 0 && (dy.abs() == 2 || dz.abs() == 2))
        || (dy == 0 && (dx.abs() == 2 || dz.abs() == 2))
        || (dz == 0 && (dx.abs() == 2 || dy.abs() == 2))
}

/// `updateShape(Level, BlockPos, List)` — offsets 0-295.
///
/// `is_water` covers the 3x3x3 gate and `frame_block` the outer scan. Returns
/// the frame positions found, or `None` when the water gate failed — which is
/// **not** the same as an empty list: an empty list means water and no frame,
/// and `None` means the conduit is not in water. Vanilla collapses both onto
/// `false` with an empty list; keeping them apart here is what lets a caller
/// tell "deactivated because dry" from "deactivated because dismantled".
pub fn update_shape(
    pos: (i32, i32, i32),
    is_water: &dyn Fn(i32, i32, i32) -> bool,
    frame_block: &dyn Fn(i32, i32, i32) -> Option<&'static str>,
) -> Option<Vec<(i32, i32, i32)>> {
    for dx in -1..=1 {
        for dy in -1..=1 {
            for dz in -1..=1 {
                if !is_water(pos.0 + dx, pos.1 + dy, pos.2 + dz) {
                    return None;
                }
            }
        }
    }
    let mut found = Vec::new();
    for dx in -2..=2 {
        for dy in -2..=2 {
            for dz in -2..=2 {
                if !is_frame_offset(dx, dy, dz) {
                    continue;
                }
                let p = (pos.0 + dx, pos.1 + dy, pos.2 + dz);
                if frame_block(p.0, p.1, p.2).is_some() {
                    found.push(p);
                }
            }
        }
    }
    Some(found)
}

/// Whether that frame activates the conduit — `size >= 16`.
#[must_use]
pub const fn is_active(frame_blocks: usize) -> bool {
    frame_blocks >= ACTIVATE_BLOCKS
}

/// Whether it hunts — `size >= 42`, the full frame.
#[must_use]
pub const fn is_hunting(frame_blocks: usize) -> bool {
    frame_blocks >= HUNT_BLOCKS
}

/// The effect radius — `(size / 7) * 16` in **integer** arithmetic, offsets
/// 8-13 of `applyEffects`.
///
/// The division is integer and comes first, so the radius steps rather than
/// ramping: 16 at 7-13 blocks, 32 at 14-20, 48 at 21-27, and so on. A
/// float version would give a smooth curve and different answers everywhere
/// but the multiples of seven.
#[must_use]
pub const fn effect_radius(frame_blocks: i32) -> i32 {
    (frame_blocks / 7) * 16
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The frame filter's maximum is 42, which is the number
    /// `updateHunting` compares against. Deriving it rather than asserting it
    /// is what makes the constant checkable.
    #[test]
    fn the_frame_holds_exactly_forty_two_positions() {
        let mut n = 0;
        for dx in -2..=2 {
            for dy in -2..=2 {
                for dz in -2..=2 {
                    if is_frame_offset(dx, dy, dz) {
                        n += 1;
                    }
                }
            }
        }
        assert_eq!(n, HUNT_BLOCKS, "the full frame is the hunting threshold");
    }

    /// The centre and the corners are not frame positions, and an edge
    /// midpoint is. A filter that took the whole 5x5x5 shell would include
    /// the corners.
    #[test]
    fn the_corners_are_not_frame_positions() {
        assert!(!is_frame_offset(0, 0, 0));
        assert!(!is_frame_offset(2, 2, 2));
        assert!(!is_frame_offset(1, 1, 1));
        assert!(is_frame_offset(0, 2, 0));
        assert!(is_frame_offset(0, 2, 2));
        assert!(is_frame_offset(2, 0, 1));
    }

    /// One dry block in the 3x3x3 refuses the whole shape, and the refusal is
    /// distinguishable from "in water with no frame".
    #[test]
    fn one_dry_block_refuses_the_shape() {
        let all_frame = |_x: i32, _y: i32, _z: i32| Some("minecraft:prismarine");
        let dry_corner = |x: i32, y: i32, z: i32| !(x == 1 && y == 1 && z == 1);
        assert!(update_shape((0, 0, 0), &dry_corner, &all_frame).is_none());
        let wet = |_x: i32, _y: i32, _z: i32| true;
        let no_frame = |_x: i32, _y: i32, _z: i32| None;
        assert_eq!(
            update_shape((0, 0, 0), &wet, &no_frame),
            Some(Vec::new()),
            "in water, no frame — a different answer from not in water"
        );
    }

    /// A complete frame in water is 42 blocks and both thresholds pass.
    #[test]
    fn a_complete_frame_is_active_and_hunting() {
        let wet = |_x: i32, _y: i32, _z: i32| true;
        let all_frame = |_x: i32, _y: i32, _z: i32| Some("minecraft:sea_lantern");
        let found = update_shape((10, 40, -7), &wet, &all_frame).expect("in water");
        assert_eq!(found.len(), 42);
        assert!(is_active(found.len()));
        assert!(is_hunting(found.len()));
        // Every position is a real offset from the conduit.
        for (x, y, z) in found {
            assert!(is_frame_offset(x - 10, y - 40, z + 7));
        }
    }

    /// The thresholds are `>=`, and sixteen is the smaller of the two.
    #[test]
    fn sixteen_activates_and_forty_one_does_not_hunt() {
        assert!(!is_active(15));
        assert!(is_active(16));
        assert!(!is_hunting(41));
        assert!(is_hunting(42));
    }

    /// The radius steps in sixteens because the divide is integer and comes
    /// first. A float version would differ everywhere but the multiples of
    /// seven, and this is the pair of asserts that separates them.
    #[test]
    fn the_effect_radius_steps_rather_than_ramping() {
        assert_eq!(effect_radius(16), 32);
        assert_eq!(effect_radius(20), 32, "still 32 at twenty blocks");
        assert_eq!(effect_radius(21), 48);
        assert_eq!(effect_radius(42), 96);
        assert_eq!(effect_radius(6), 0, "below seven the radius is zero");
    }
}
