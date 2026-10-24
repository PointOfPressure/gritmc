//! `BeaconBlockEntity.tick` — the pyramid, the beam scan and the effects.
//!
//! Transcribed from `.tick` (offsets 0-637), `.updateBase(Level, int, int,
//! int)` (0-129) and `.applyEffects(Level, BlockPos, int, Holder, Holder)`
//! (0-216) in the pinned 26.2 jar.
//!
//! # The beam is scanned ten blocks a tick, not all at once
//!
//! `tick`'s loop is bounded by `bipush 10` at offset 129 and by the
//! `WORLD_SURFACE` heightmap at 139, and `lastCheckY` carries the cursor
//! between ticks. So a beacon under a tall column takes several ticks to
//! notice its beam, and `beamSections` is only swapped for
//! `checkingBeamSections` on the tick the scan **reaches the surface**
//! (offsets 449-485). A one-pass implementation would light a beacon a few
//! ticks early and would never exercise the resume path.
//!
//! # The pyramid and the effects are recomputed once every eighty ticks
//!
//! `getGameTime() % 80 == 0` — `ldc2_w 80l; lrem` at offset 370 — and the
//! whole of `updateBase`, `applyEffects` and the `BEACON_AMBIENT` sound sits
//! behind it. Between those ticks a beacon does nothing but scan its beam.
//!
//! **`updateBase` only runs when `beamSections` is non-empty** (offset 383),
//! so a beacon whose beam is blocked keeps whatever `levels` it last had
//! rather than dropping to 0 — and the *effects* are then gated on
//! `beamSections` being non-empty too, so it stops applying them anyway. Two
//! tests of the same list, two ticks apart in effect, and only one of them
//! resets anything.

/// How many blocks of beam the ticker scans per tick — `bipush 10`.
pub const BEAM_SCAN_PER_TICK: i32 = 10;

/// How often the pyramid and the effects are recomputed — `ldc2_w 80l`.
pub const EFFECT_PERIOD: i64 = 80;

/// The tallest pyramid — the `iconst_4` bounding `updateBase`'s outer loop.
pub const MAX_LEVELS: i32 = 4;

/// `BeaconBlockEntity.updateBase(Level, int x, int y, int z)` — offsets
/// 0-129.
///
/// Walks `i = 1..=4`. Layer `i` is at `y - i` and is the full square
/// `[x-i, x+i] x [z-i, z+i]` — `(2i+1)^2` blocks, inclusive on both bounds.
/// Every one of them must be in `BlockTags.BEACON_BASE_BLOCKS`.
///
/// **The answer is the last *consecutive* level that passed**, not the count
/// of levels that passed: the `ifeq 127` at offset 114 leaves the loop the
/// moment one fails, so a beacon with a broken second layer and an intact
/// third reads 1. And a layer below the world's minimum Y ends the loop
/// (offsets 18-27) rather than failing it, which is the same answer by a
/// different road and is why a beacon on bedrock does not read 0.
///
/// `is_base_block` is `state.is(BlockTags.BEACON_BASE_BLOCKS)`; the tag is
/// the caller's because this server dumps item tags and not block ones.
pub fn update_base(
    x: i32,
    y: i32,
    z: i32,
    min_y: i32,
    is_base_block: &dyn Fn(i32, i32, i32) -> bool,
) -> i32 {
    let mut levels = 0;
    for i in 1..=MAX_LEVELS {
        let layer_y = y - i;
        if layer_y < min_y {
            break;
        }
        let mut whole = true;
        'layer: for bx in (x - i)..=(x + i) {
            for bz in (z - i)..=(z + i) {
                if !is_base_block(bx, layer_y, bz) {
                    whole = false;
                    break 'layer;
                }
            }
        }
        if !whole {
            break;
        }
        levels = i;
    }
    levels
}

/// What `applyEffects` would hand each player in range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BeaconEffects {
    /// The horizontal radius the AABB is inflated by — `levels * 10 + 10`,
    /// offsets 12-20. So a level-1 beacon reaches 20 blocks and a level-4 one
    /// 50.
    pub radius: i32,
    /// `(9 + levels * 2) * 20` ticks — offsets 42-51. 220 ticks at level 1
    /// and 340 at level 4, both a good deal longer than the eighty between
    /// re-applications, which is what makes the effect look continuous.
    pub duration: i32,
    /// The primary effect's amplifier: **1** when `levels >= 4` **and** the
    /// two chosen effects are equal, 0 otherwise (offsets 22-42).
    pub primary_amplifier: i32,
    /// Whether the secondary effect is applied as well: `levels >= 4`, the
    /// two effects are **not** equal, and the secondary is present (offsets
    /// 143-159).
    pub applies_secondary: bool,
}

/// `applyEffects`' arithmetic, without the entity list.
///
/// `primary` and `secondary` are the chosen effects, `None` for unset. A
/// `None` primary means `applyEffects` returns at offset 11 and nothing at
/// all happens — the caller gets `None` here for exactly that case.
///
/// The two conditions on the secondary are **not** complements of the
/// primary's amplifier rule: at `levels >= 4` with the same effect chosen
/// twice the primary is amplified and the secondary loop does not run, and at
/// `levels >= 4` with two different effects the primary is unamplified and
/// the secondary runs. So one comparison drives both, in opposite directions.
#[must_use]
pub fn effects_for(
    levels: i32,
    primary: Option<&str>,
    secondary: Option<&str>,
) -> Option<BeaconEffects> {
    primary?;
    let same = primary == secondary;
    Some(BeaconEffects {
        radius: levels * 10 + 10,
        duration: (9 + levels * 2) * 20,
        primary_amplifier: i32::from(levels >= MAX_LEVELS && same),
        applies_secondary: levels >= MAX_LEVELS && !same && secondary.is_some(),
    })
}

/// Whether this tick is one of the eightieth ticks that recomputes anything.
#[must_use]
pub const fn is_effect_tick(game_time: i64) -> bool {
    game_time % EFFECT_PERIOD == 0
}

/// Whether the beacon plays `BEACON_ACTIVATE` this tick — offsets 468-524.
///
/// The rising edge of `levels > 0`, sampled around the beam-section swap:
/// `was_active` is `levels > 0` read **before** the scan finished, and the
/// new one after. Only a false-to-true transition plays the sound, so a
/// beacon that stays lit is silent apart from its eighty-tick ambient.
#[must_use]
pub const fn activates(was_active: bool, levels: i32) -> bool {
    !was_active && levels > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A solid pyramid four deep reads 4; one block missing from the third
    /// layer reads 2 — **not** 3, because the loop leaves on the first
    /// failure and never sees the intact fourth.
    #[test]
    fn a_broken_layer_ends_the_count_rather_than_skipping_it() {
        let solid = |_x: i32, _y: i32, _z: i32| true;
        assert_eq!(update_base(0, 64, 0, -64, &solid), 4);

        // Punch one block out of layer 3, which sits at y = 61.
        let holed = |x: i32, y: i32, _z: i32| !(y == 61 && x == -3);
        assert_eq!(
            update_base(0, 64, 0, -64, &holed),
            2,
            "the intact fourth layer is never reached"
        );
    }

    /// Each layer is the whole `(2i+1)^2` square, so a pyramid that is only
    /// wide enough for two levels reads 2.
    #[test]
    fn a_layer_must_be_the_whole_square() {
        // Base blocks only within 2 of the axis, so layers 1 and 2 are whole
        // and layer 3 needs x = ±3.
        let narrow = |x: i32, _y: i32, z: i32| x.abs() <= 2 && z.abs() <= 2;
        assert_eq!(update_base(0, 64, 0, -64, &narrow), 2);
        // And one wide enough for three.
        let wider = |x: i32, _y: i32, z: i32| x.abs() <= 3 && z.abs() <= 3;
        assert_eq!(update_base(0, 64, 0, -64, &wider), 3);
    }

    /// A layer below the world floor **ends** the loop rather than failing
    /// it, so a beacon four blocks above bedrock still reads what it built.
    #[test]
    fn the_world_floor_ends_the_count_without_failing_it() {
        let solid = |_x: i32, _y: i32, _z: i32| true;
        // min_y = 61 leaves room for exactly two layers (63 and 62).
        assert_eq!(update_base(0, 64, 0, 62, &solid), 2);
        // And a beacon sitting on the floor reads 0 rather than looping.
        assert_eq!(update_base(0, 64, 0, 64, &solid), 0);
    }

    /// A beacon with no base at all reads 0 and does not panic.
    #[test]
    fn no_base_reads_zero() {
        let none = |_x: i32, _y: i32, _z: i32| false;
        assert_eq!(update_base(0, 64, 0, -64, &none), 0);
    }

    /// The radius and duration formulas, at both ends of the range.
    #[test]
    fn the_radius_and_duration_scale_with_the_levels() {
        let one = effects_for(1, Some("minecraft:speed"), None).expect("a primary");
        assert_eq!(one.radius, 20);
        assert_eq!(one.duration, 220);
        let four = effects_for(4, Some("minecraft:speed"), None).expect("a primary");
        assert_eq!(four.radius, 50);
        assert_eq!(four.duration, 340);
        // The duration always outlasts the eighty ticks between
        // re-applications, which is what makes the effect look continuous.
        assert!(one.duration > i32::try_from(EFFECT_PERIOD).unwrap());
    }

    /// One comparison drives both halves in opposite directions: the same
    /// effect twice amplifies the primary and applies no secondary; two
    /// different effects apply the secondary and leave the primary at 0.
    #[test]
    fn choosing_the_same_effect_twice_amplifies_instead_of_doubling() {
        let same =
            effects_for(4, Some("minecraft:speed"), Some("minecraft:speed")).expect("a primary");
        assert_eq!(same.primary_amplifier, 1);
        assert!(!same.applies_secondary);

        let different =
            effects_for(4, Some("minecraft:speed"), Some("minecraft:haste")).expect("a primary");
        assert_eq!(different.primary_amplifier, 0);
        assert!(different.applies_secondary);
    }

    /// Below four levels neither half fires, whatever is chosen.
    #[test]
    fn a_three_level_beacon_amplifies_nothing_and_applies_no_secondary() {
        for secondary in [None, Some("minecraft:speed"), Some("minecraft:haste")] {
            let e = effects_for(3, Some("minecraft:speed"), secondary).expect("a primary");
            assert_eq!(e.primary_amplifier, 0);
            assert!(!e.applies_secondary);
        }
    }

    /// No primary means `applyEffects` returns before anything, so there is
    /// no radius to speak of.
    #[test]
    fn no_primary_effect_means_no_effects_at_all() {
        assert!(effects_for(4, None, Some("minecraft:speed")).is_none());
    }

    /// The recompute is every eightieth tick, counted from tick 0.
    #[test]
    fn the_effect_tick_is_every_eightieth() {
        assert!(is_effect_tick(0));
        assert!(!is_effect_tick(79));
        assert!(is_effect_tick(80));
        assert!(is_effect_tick(160));
    }

    /// The activation sound is a rising edge, not a state.
    #[test]
    fn the_activation_sound_is_a_rising_edge() {
        assert!(activates(false, 1));
        assert!(!activates(true, 1), "already lit, no sound");
        assert!(!activates(false, 0));
    }
}
