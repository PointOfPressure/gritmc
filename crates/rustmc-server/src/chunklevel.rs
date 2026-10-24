//! `ChunkLevel` — the ticket-level table that decides what a chunk is.
//!
//! Transcribed from 26.2's `net.minecraft.server.level.ChunkLevel` and
//! `FullChunkStatus` under `javap -p -c -constants`, with the two derived
//! constants taken from a live JVM by `ci/chunklevel-golden.sh` — see
//! [`MAX_LEVEL`] for why they cannot be read off the bytecode.
//!
//! A ticket level is a **distance, so smaller is more**. Level 31 is the
//! richest state a chunk can be in and level 45 does not exist. Every
//! comparison in this module is therefore `<=` against a ceiling and reads
//! backwards on first acquaintance; vanilla's own predicates are written the
//! same way and are reproduced rather than inverted.

/// `ChunkLevel.ENTITY_TICKING_LEVEL` — 31.
///
/// At or below this a chunk ticks entities: mobs move, items despawn, the
/// player's own physics runs against loaded neighbours.
pub const ENTITY_TICKING_LEVEL: i32 = 31;

/// `ChunkLevel.BLOCK_TICKING_LEVEL` — 32.
///
/// At or below this a chunk gets random ticks and its block entities tick.
/// This is where crops stop growing and hoppers stop moving items for a
/// chunk that is loaded but too far to simulate.
pub const BLOCK_TICKING_LEVEL: i32 = 32;

/// `ChunkLevel.FULL_CHUNK_LEVEL` — 33.
///
/// At or below this the chunk is a `LevelChunk`: it exists as a real chunk
/// and can be sent to a client. Above it the chunk is still *loaded* — it
/// has storage and is a generation dependency for its neighbours — but is
/// `INACCESSIBLE` to gameplay.
pub const FULL_CHUNK_LEVEL: i32 = 33;

/// `ChunkLevel.RADIUS_AROUND_FULL_CHUNK` — **11**.
///
/// The accumulated dependency radius of the pyramid step that reaches
/// `FULL`: how many chunks of context a full chunk needs generated around
/// it. See [`MAX_LEVEL`].
pub const RADIUS_AROUND_FULL_CHUNK: i32 = 11;

/// `ChunkLevel.MAX_LEVEL` — **44**, and it is `33 + 11`, not 33.
///
/// This is the boundary between a chunk that is loaded and a chunk that is
/// unloaded: `ChunkMap.updateChunkScheduling` puts a chunk on `toDrop` the
/// moment `isLoaded` goes false for its new level, and takes it off again
/// when it goes true.
///
/// # Why this is a golden and not a literal
///
/// The bytecode of `ChunkLevel.<clinit>` reads
///
/// ```java
/// RADIUS_AROUND_FULL_CHUNK = ChunkPyramid.GENERATION_PYRAMID
///     .getStepTo(ChunkStatus.FULL).accumulatedDependencies().getRadius();
/// MAX_LEVEL = 33 + RADIUS_AROUND_FULL_CHUNK;
/// ```
///
/// which is a *recipe*, not a number: it sums dependency radii over the
/// whole generation pyramid. Reading it and writing down a plausible answer
/// is the shape of defect this project keeps paying for, so the number comes
/// from `ci/chunklevel-golden.sh` running vanilla's own initialiser, and
/// `crates/rustmc-server/golden/chunk_level.json` is the committed evidence.
/// A port that assumed `MAX_LEVEL = FULL_CHUNK_LEVEL` — which it was in
/// versions before the pyramid existed — would unload an eleven-chunk ring
/// that vanilla keeps, and every chunk inside the guess would still be
/// correct, so no value test would see it.
pub const MAX_LEVEL: i32 = FULL_CHUNK_LEVEL + RADIUS_AROUND_FULL_CHUNK;

/// `net.minecraft.server.level.FullChunkStatus`.
///
/// **Declared in vanilla's ordinal order**, which is capability-ascending:
/// `INACCESSIBLE` is ordinal 0 and `ENTITY_TICKING` is ordinal 3. That is
/// the opposite direction to the level numbers, which is exactly the
/// confusion this ordering exists to make impossible to introduce silently —
/// anything comparing two of these must say which direction it means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FullChunkStatus {
    /// Loaded, but not a gameplay chunk. Cannot be sent to a client.
    Inaccessible,
    /// A real chunk. Sendable; does not tick.
    Full,
    /// Ticks blocks: random ticks, block entities, scheduled ticks.
    BlockTicking,
    /// Ticks entities as well.
    EntityTicking,
}

/// `ChunkLevel.fullStatus(int)`.
#[must_use]
pub const fn full_status(level: i32) -> FullChunkStatus {
    if level <= ENTITY_TICKING_LEVEL {
        FullChunkStatus::EntityTicking
    } else if level <= BLOCK_TICKING_LEVEL {
        FullChunkStatus::BlockTicking
    } else if level <= FULL_CHUNK_LEVEL {
        FullChunkStatus::Full
    } else {
        FullChunkStatus::Inaccessible
    }
}

/// `ChunkLevel.byStatus(FullChunkStatus)` — the *highest* level at which a
/// chunk still has that status, which is the level a ticket must ask for.
#[must_use]
pub const fn by_status(status: FullChunkStatus) -> i32 {
    match status {
        FullChunkStatus::Inaccessible => MAX_LEVEL,
        FullChunkStatus::Full => FULL_CHUNK_LEVEL,
        FullChunkStatus::BlockTicking => BLOCK_TICKING_LEVEL,
        FullChunkStatus::EntityTicking => ENTITY_TICKING_LEVEL,
    }
}

/// `ChunkLevel.isEntityTicking(int)`.
#[must_use]
pub const fn is_entity_ticking(level: i32) -> bool {
    level <= ENTITY_TICKING_LEVEL
}

/// `ChunkLevel.isBlockTicking(int)`.
#[must_use]
pub const fn is_block_ticking(level: i32) -> bool {
    level <= BLOCK_TICKING_LEVEL
}

/// `ChunkLevel.isLoaded(int)`.
///
/// The predicate `ChunkMap.updateChunkScheduling` drives unloading off: a
/// chunk whose new level fails this goes on `toDrop`.
#[must_use]
pub const fn is_loaded(level: i32) -> bool {
    level <= MAX_LEVEL
}

#[cfg(test)]
mod tests {
    use super::{
        by_status, full_status, is_block_ticking, is_entity_ticking, is_loaded, FullChunkStatus,
        BLOCK_TICKING_LEVEL, ENTITY_TICKING_LEVEL, FULL_CHUNK_LEVEL, MAX_LEVEL,
        RADIUS_AROUND_FULL_CHUNK,
    };

    /// The whole table, against the JVM's own answers.
    ///
    /// The fixture is committed, so this is a hard failure and never a skip:
    /// a missing fixture here is a broken checkout, not an absent oracle.
    /// Regenerate with `ci/chunklevel-golden.sh`.
    #[test]
    fn the_level_table_matches_the_jvm() {
        let raw = include_str!("../golden/chunk_level.json");
        let get_i32 = |key: &str| -> i32 {
            let at = raw
                .find(&format!("\"{key}\":"))
                .unwrap_or_else(|| panic!("golden has no {key}"));
            let rest = &raw[at + key.len() + 3..];
            let end = rest.find([',', '\n']).unwrap_or(rest.len());
            rest[..end].trim().parse().expect("integer")
        };
        assert_eq!(get_i32("full_chunk_level"), FULL_CHUNK_LEVEL);
        assert_eq!(get_i32("block_ticking_level"), BLOCK_TICKING_LEVEL);
        assert_eq!(get_i32("entity_ticking_level"), ENTITY_TICKING_LEVEL);
        assert_eq!(
            get_i32("radius_around_full_chunk"),
            RADIUS_AROUND_FULL_CHUNK
        );
        assert_eq!(get_i32("max_level"), MAX_LEVEL);

        // Every `levels` row, parsed positionally rather than with a JSON
        // crate: this crate has no JSON dependency and adding one to read a
        // five-field fixture would be the tail wagging the dog.
        let mut rows = 0usize;
        for line in raw.lines().filter(|l| l.contains("\"full_status\"")) {
            let field = |k: &str| -> String {
                let at = line.find(&format!("\"{k}\":")).expect("field");
                let rest = &line[at + k.len() + 3..];
                let end = rest.find([',', '}']).unwrap_or(rest.len());
                rest[..end].trim().trim_matches('"').to_owned()
            };
            let level: i32 = field("level").parse().expect("level");
            let want = match field("full_status").as_str() {
                "ENTITY_TICKING" => FullChunkStatus::EntityTicking,
                "BLOCK_TICKING" => FullChunkStatus::BlockTicking,
                "FULL" => FullChunkStatus::Full,
                "INACCESSIBLE" => FullChunkStatus::Inaccessible,
                other => panic!("unknown FullChunkStatus {other}"),
            };
            assert_eq!(full_status(level), want, "fullStatus({level})");
            assert_eq!(
                is_entity_ticking(level),
                field("entity_ticking") == "true",
                "isEntityTicking({level})"
            );
            assert_eq!(
                is_block_ticking(level),
                field("block_ticking") == "true",
                "isBlockTicking({level})"
            );
            assert_eq!(
                is_loaded(level),
                field("loaded") == "true",
                "isLoaded({level})"
            );
            rows += 1;
        }
        assert_eq!(rows, 19, "the golden dumps levels 28..=MAX_LEVEL+2");
    }

    /// `byStatus` is the inverse of `fullStatus` at each boundary: the level
    /// it returns must map back to the status it was asked about.
    #[test]
    fn by_status_round_trips_through_full_status() {
        for s in [
            FullChunkStatus::Inaccessible,
            FullChunkStatus::Full,
            FullChunkStatus::BlockTicking,
            FullChunkStatus::EntityTicking,
        ] {
            assert_eq!(full_status(by_status(s)), s, "{s:?}");
        }
        assert_eq!(by_status(FullChunkStatus::Inaccessible), MAX_LEVEL);
    }

    /// The unload boundary, stated as a number rather than as a relation, so
    /// that a change to `RADIUS_AROUND_FULL_CHUNK` moves this test and not
    /// just the constant. 34 is the answer a port that guessed
    /// `MAX_LEVEL = FULL_CHUNK_LEVEL` would give.
    #[test]
    fn the_unload_boundary_is_forty_four_and_not_thirty_three() {
        assert_eq!(MAX_LEVEL, 44);
        assert!(is_loaded(44));
        assert!(!is_loaded(45));
        assert!(
            is_loaded(34),
            "level 34 is INACCESSIBLE but still loaded — the eleven-chunk ring a \
             MAX_LEVEL of 33 would have unloaded"
        );
        assert_eq!(full_status(34), FullChunkStatus::Inaccessible);
    }

    /// The ordinal order is vanilla's, which the golden also records. A
    /// reordering here would silently change `by_status`'s match arms.
    #[test]
    fn the_status_ordinals_are_vanillas() {
        let raw = include_str!("../golden/chunk_level.json");
        for (name, ordinal) in [
            ("INACCESSIBLE", 0),
            ("FULL", 1),
            ("BLOCK_TICKING", 2),
            ("ENTITY_TICKING", 3),
        ] {
            assert!(
                raw.contains(&format!("\"status\": \"{name}\", \"ordinal\": {ordinal}")),
                "{name} is not ordinal {ordinal} in the golden"
            );
        }
        assert!(FullChunkStatus::Inaccessible < FullChunkStatus::EntityTicking);
    }
}
