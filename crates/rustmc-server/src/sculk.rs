//! `VibrationSystem` and the sculk sensor's arithmetic.
//!
//! Transcribed from `VibrationSystem`'s `static {}` and
//! `.lambda$static$0(Reference2IntOpenHashMap)`, `.getRedstoneStrengthForDistance
//! (float, int)` (offsets 0-21), `SculkSensorBlockEntity$VibrationUser
//! .getListenerRadius/.onReceiveVibration`, and `SculkSensorBlock`'s
//! `ACTIVE_TICKS` / `COOLDOWN_TICKS` in the pinned 26.2 jar.
//!
//! # The frequency table is the whole of what a sculk sensor knows
//!
//! `VIBRATION_FREQUENCY_FOR_EVENT` is a `Reference2IntOpenHashMap` built in
//! one lambda, **41 entries**, and every game event not in it answers
//! `NO_VIBRATION_FREQUENCY` — a declared `0`, which is what a sculk sensor
//! ignores. It is transcribed here rather than dumped because it is a
//! literal-for-literal static initialiser with no tag expansion and no
//! registry order in it: the pairs are `getstatic GameEvent.X; key(); <int>;
//! put`, and reading them off the bytecode is reading the table itself.
//!
//! That is the opposite of the jukebox song ids, which are a *registry
//! index* and therefore a property of load order rather than of the jar; see
//! `crate::jukebox::JukeboxSongs::song_id`. Knowing which of the two kind a
//! table is, is the whole question.
//!
//! # The comparator output is a floor, and the divide is a double
//!
//! `getRedstoneStrengthForDistance(float distance, int radius)` is
//!
//! ```text
//!   max(1, 15 - Mth.floor((15.0 / radius) * distance))
//! ```
//!
//! with `15.0` a **double** (`ldc2_w`) and `distance` widened to one, so the
//! whole product is double arithmetic and the `floor` is
//! `Mth.floor(double)`. Doing it in `f32` gives a different answer at the
//! distances where the product lands within a float ulp of an integer. And
//! the `max(1, …)` means a vibration at the very edge of the radius still
//! reads **1**, never 0 — so "the sensor read 0" and "the sensor read
//! nothing" are different states.

/// `VibrationSystem.NO_VIBRATION_FREQUENCY` — a declared `public static final
/// int` of 0, and the answer for every event absent from the table.
pub const NO_VIBRATION_FREQUENCY: i32 = 0;

/// `SculkSensorBlockEntity$VibrationUser.getListenerRadius()` — `bipush 8`.
/// The shrieker's is the same 8, checked separately rather than assumed.
pub const SENSOR_LISTENER_RADIUS: i32 = 8;

/// `SculkSensorBlock.ACTIVE_TICKS` — a declared constant, 30.
pub const ACTIVE_TICKS: i32 = 30;

/// `SculkSensorBlock.COOLDOWN_TICKS` — a declared constant, 10.
pub const COOLDOWN_TICKS: i32 = 10;

/// `VIBRATION_FREQUENCY_FOR_EVENT`, all 41 entries, in the `put` order of
/// `lambda$static$0`.
///
/// The order is not load-bearing — it is a hash map and the lookup is by key
/// — but it is kept so the table can be diffed against a fresh `javap` by
/// eye.
pub const VIBRATION_FREQUENCIES: [(&str, i32); 41] = [
    ("minecraft:step", 1),
    ("minecraft:swim", 1),
    ("minecraft:flap", 1),
    ("minecraft:projectile_land", 2),
    ("minecraft:hit_ground", 2),
    ("minecraft:splash", 2),
    ("minecraft:bounce", 2),
    ("minecraft:item_interact_finish", 3),
    ("minecraft:projectile_shoot", 3),
    ("minecraft:instrument_play", 3),
    ("minecraft:entity_action", 4),
    ("minecraft:elytra_glide", 4),
    ("minecraft:unequip", 4),
    ("minecraft:entity_dismount", 5),
    ("minecraft:equip", 5),
    ("minecraft:entity_interact", 6),
    ("minecraft:shear", 6),
    ("minecraft:entity_mount", 6),
    ("minecraft:entity_damage", 7),
    ("minecraft:drink", 8),
    ("minecraft:eat", 8),
    ("minecraft:container_close", 9),
    ("minecraft:block_close", 9),
    ("minecraft:block_deactivate", 9),
    ("minecraft:block_detach", 9),
    ("minecraft:container_open", 10),
    ("minecraft:block_open", 10),
    ("minecraft:block_activate", 10),
    ("minecraft:block_attach", 10),
    ("minecraft:prime_fuse", 10),
    ("minecraft:note_block_play", 10),
    ("minecraft:block_change", 11),
    ("minecraft:block_destroy", 12),
    ("minecraft:fluid_pickup", 12),
    ("minecraft:block_place", 13),
    ("minecraft:fluid_place", 13),
    ("minecraft:entity_place", 14),
    ("minecraft:lightning_strike", 14),
    ("minecraft:teleport", 14),
    ("minecraft:entity_die", 15),
    ("minecraft:explode", 15),
];

/// `VibrationSystem.RESONANCE_EVENTS` — `minecraft:resonate_1` through
/// `resonate_15`, in that order, so index `i` is frequency `i + 1`.
///
/// These are the events an amethyst block re-emits when a vibration reaches
/// it, and the list's **position** is the frequency rather than any value in
/// the event, which is why the order is load-bearing here where it is not for
/// the frequency map.
#[must_use]
pub fn resonance_event(frequency: i32) -> Option<String> {
    if (1..=15).contains(&frequency) {
        Some(format!("minecraft:resonate_{frequency}"))
    } else {
        None
    }
}

/// `VibrationSystem.getGameEventFrequency(ResourceKey)`.
///
/// Every event absent from the table answers [`NO_VIBRATION_FREQUENCY`],
/// which is what makes an unlisted event invisible to a sensor rather than a
/// frequency-0 vibration.
#[must_use]
pub fn game_event_frequency(event: &str) -> i32 {
    VIBRATION_FREQUENCIES
        .iter()
        .find(|(name, _)| *name == event)
        .map_or(NO_VIBRATION_FREQUENCY, |(_, f)| *f)
}

/// `VibrationSystem.getRedstoneStrengthForDistance(float, int)` — offsets
/// 0-21.
///
/// `max(1, 15 - Mth.floor((15.0 / radius) * distance))`, in **double**
/// arithmetic throughout: `15.0` is an `ldc2_w` and the float distance is
/// widened by `f2d` before the multiply.
#[must_use]
pub fn redstone_strength_for_distance(distance: f32, radius: i32) -> i32 {
    let per_block = 15.0_f64 / f64::from(radius);
    #[allow(clippy::cast_possible_truncation)]
    let floored = (per_block * f64::from(distance)).floor() as i32;
    1.max(15 - floored)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Forty-one entries and no duplicate keys — a duplicate would mean the
    /// transcription lost a `put` and the later value silently won.
    #[test]
    fn the_table_is_forty_one_distinct_events() {
        assert_eq!(VIBRATION_FREQUENCIES.len(), 41);
        let names: std::collections::BTreeSet<_> =
            VIBRATION_FREQUENCIES.iter().map(|(n, _)| *n).collect();
        assert_eq!(names.len(), 41, "a duplicated key lost an entry");
    }

    /// Every frequency is in 1..=15, and all fifteen are used. A table that
    /// had collapsed onto one value would pass a spot check and fail this.
    #[test]
    fn every_frequency_from_one_to_fifteen_is_used() {
        let used: std::collections::BTreeSet<i32> =
            VIBRATION_FREQUENCIES.iter().map(|(_, f)| *f).collect();
        assert_eq!(
            used.iter().copied().collect::<Vec<_>>(),
            (1..=15).collect::<Vec<_>>()
        );
    }

    /// The lookup answers 0 for an event that is not in the table, which is
    /// what makes it invisible rather than a frequency-0 vibration.
    #[test]
    fn an_unlisted_event_has_no_frequency() {
        assert_eq!(game_event_frequency("minecraft:step"), 1);
        assert_eq!(game_event_frequency("minecraft:explode"), 15);
        assert_eq!(
            game_event_frequency("minecraft:jukebox_play"),
            NO_VIBRATION_FREQUENCY
        );
        assert_eq!(game_event_frequency("nonsense"), NO_VIBRATION_FREQUENCY);
    }

    /// A few pairs that are easy to get backwards: walking is the quietest
    /// signal and an explosion the loudest, and `block_place` and
    /// `block_destroy` are **not** adjacent — placing is 13 and breaking 12,
    /// with `fluid_pickup` sharing the 12.
    #[test]
    fn placing_reads_higher_than_breaking() {
        assert_eq!(game_event_frequency("minecraft:block_destroy"), 12);
        assert_eq!(game_event_frequency("minecraft:block_place"), 13);
        assert_eq!(game_event_frequency("minecraft:fluid_pickup"), 12);
        assert_eq!(game_event_frequency("minecraft:fluid_place"), 13);
    }

    /// The comparator floor: a vibration on the sensor itself reads 15, and
    /// one at the very edge of the radius reads **1** rather than 0.
    #[test]
    fn the_strength_never_falls_below_one() {
        assert_eq!(
            redstone_strength_for_distance(0.0, SENSOR_LISTENER_RADIUS),
            15
        );
        assert_eq!(
            redstone_strength_for_distance(8.0, SENSOR_LISTENER_RADIUS),
            1
        );
        assert_eq!(
            redstone_strength_for_distance(100.0, SENSOR_LISTENER_RADIUS),
            1,
            "the max(1, ...) holds beyond the radius too"
        );
    }

    /// The step down happens at each multiple of `radius / 15`, and the
    /// division is a floor — so the whole first `8/15` of a block still reads
    /// 15.
    #[test]
    fn the_strength_steps_down_by_the_floor() {
        let r = SENSOR_LISTENER_RADIUS;
        assert_eq!(redstone_strength_for_distance(0.5, r), 15);
        assert_eq!(redstone_strength_for_distance(0.6, r), 14);
        assert_eq!(redstone_strength_for_distance(1.0, r), 14);
        assert_eq!(redstone_strength_for_distance(4.0, r), 8);
    }

    /// The resonance events are indexed by frequency and cover 1..=15 only.
    #[test]
    fn the_resonance_events_cover_one_to_fifteen() {
        assert_eq!(resonance_event(1).as_deref(), Some("minecraft:resonate_1"));
        assert_eq!(
            resonance_event(15).as_deref(),
            Some("minecraft:resonate_15")
        );
        assert!(resonance_event(0).is_none());
        assert!(resonance_event(16).is_none());
    }

    /// The two declared constants, which govern how long a sensor stays lit
    /// and how long it will not listen for.
    #[test]
    fn the_sensor_phase_lengths_are_thirty_and_ten() {
        assert_eq!(ACTIVE_TICKS, 30);
        assert_eq!(COOLDOWN_TICKS, 10);
    }
}
