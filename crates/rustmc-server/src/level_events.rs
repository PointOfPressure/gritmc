//! `LevelEvent`'s constants — the 83 ids the `level_event` packet's first
//! field can carry.
//!
//! # This table's job is to make two hand-written numbers falsifiable
//!
//! This server sends three level events, all transcribed by hand:
//! `crate::shapes::LEVEL_EVENT_DESTROY` is 2001,
//! `crate::power::LEVEL_EVENT_REDSTONE_TORCH_BURNOUT` is 1502 and
//! `crate::fluid::LEVEL_EVENT_LAVA_FIZZ` is 1501 (2026-10-01, the lava/water
//! interaction). Until this table, nothing checked any of them against the
//! jar. A wrong id there is not a crash and not a silence:
//! it is a client playing **some other event's** particles and sound at the
//! right place, which reads as a rendering quirk and would survive any test
//! that counts packets.
//!
//! So the table is generated and the constants are checked against it, and
//! that is the whole of what it does today. It is not a dispatch table — a
//! server that emitted an event *because* it was in a list would be emitting
//! events vanilla does not.
//!
//! # 83, and the ceiling that goes with it
//!
//! `LevelEvent` declares 83 `public static final int` fields in 26.2, read by
//! reflection rather than transcribed so that a version which adds one adds
//! it here without anybody editing a file. Against that, the corpus has ever
//! carried **one**: all 833 `level_event` observations across the 32
//! recordings on this box are `2001 PARTICLES_DESTROY_BLOCK`
//! (`./ci/sound-emission-gap.py`'s sibling sweep, `docs/FINDINGS.md`
//! 2026-09-05). That is the honest denominator for any claim about this
//! channel: 2 of 83 emitted by us, 1 of 83 ever adjudicated.
//!
//! # The ids are not dense and the gaps are real
//!
//! 1000-1052, 1500-1505, 2000-2013, 3000-3021, with holes inside each run —
//! there is no 1003, no 2005, no 3010. Nothing here indexes by id for that
//! reason; the map is by name.

use std::collections::BTreeMap;
use std::io;

/// Where `ci/sound-particle-golden.sh` writes the table.
pub const TABLE_PATH: &str = "data/vanilla/level_events.json";

/// `LevelEvent`'s int constants, by field name.
#[derive(Debug, Default)]
pub struct LevelEvents {
    by_name: BTreeMap<String, i32>,
}

impl LevelEvents {
    /// Read the generated table.
    ///
    /// # Errors
    /// On a missing or unreadable file, or a document this reader does not
    /// recognise.
    pub fn load(path: &std::path::Path) -> io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let doc: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Self::from_json(&doc)
    }

    /// Parse a table document.
    ///
    /// # Errors
    /// On a missing `table`, a row without a `name` or an `id`, a declared
    /// count that disagrees with the rows, or two constants sharing an id.
    ///
    /// The last of those is the one that matters and it is why this is a
    /// parse rather than a `serde` derive: `LevelEvent` has no duplicate ids,
    /// and a table that grew one would mean a reflective dump had started
    /// reading something other than the constants — every lookup after it
    /// would still answer, with the wrong number.
    pub fn from_json(doc: &serde_json::Value) -> io::Result<Self> {
        let bad = |m: String| io::Error::new(io::ErrorKind::InvalidData, m);
        let rows = doc
            .get("table")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| bad("level_events.json has no `table` array".into()))?;
        let declared = doc
            .get("levelEvents")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| bad("level_events.json has no `levelEvents` count".into()))?;
        if declared as usize != rows.len() {
            return Err(bad(format!(
                "level_events.json declares {declared} constants and carries {} rows",
                rows.len()
            )));
        }
        let mut by_name = BTreeMap::new();
        let mut by_id: BTreeMap<i32, String> = BTreeMap::new();
        for (i, row) in rows.iter().enumerate() {
            let name = row
                .get("name")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| bad(format!("level-event row {i} has no `name`")))?;
            let id = row
                .get("id")
                .and_then(serde_json::Value::as_i64)
                .and_then(|v| i32::try_from(v).ok())
                .ok_or_else(|| bad(format!("level-event row {i} ({name}) has no i32 `id`")))?;
            if let Some(other) = by_id.insert(id, name.to_string()) {
                return Err(bad(format!(
                    "LevelEvent.{name} and LevelEvent.{other} are both {id}"
                )));
            }
            by_name.insert(name.to_string(), id);
        }
        Ok(Self { by_name })
    }

    /// One constant by field name, e.g. `PARTICLES_DESTROY_BLOCK`.
    #[must_use]
    pub fn id(&self, name: &str) -> Option<i32> {
        self.by_name.get(name).copied()
    }

    /// How many constants the table holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    /// Whether the table is empty, which for a loaded table it never is.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The check this module exists for**, and it reads the real generated
    /// table rather than a fixture: the point is that the two constants this
    /// server sends agree with the jar, and a fixture would only say they
    /// agree with a fixture.
    ///
    /// `rustmc_data::fixture::available` makes a missing table a **panic**
    /// rather than a skip — see that module for the seventeen-checks-that-
    /// could-not-fail argument. `RUSTMC_FIXTURES=optional` is the greppable
    /// opt-out for a clean clone.
    #[test]
    fn the_two_level_events_this_server_sends_are_the_jars() {
        let path = std::path::Path::new("../..").join(TABLE_PATH);
        if !rustmc_data::fixture::available(
            &path,
            "level-event table",
            "ci/sound-particle-golden.sh",
        ) {
            return;
        }
        let t = LevelEvents::load(&path).expect("the generated table loads");
        assert_eq!(
            t.id("PARTICLES_DESTROY_BLOCK"),
            Some(crate::shapes::LEVEL_EVENT_DESTROY),
            "crate::shapes::LEVEL_EVENT_DESTROY disagrees with LevelEvent"
        );
        assert_eq!(
            t.id("REDSTONE_TORCH_BURNOUT"),
            Some(crate::power::LEVEL_EVENT_REDSTONE_TORCH_BURNOUT),
            "crate::power::LEVEL_EVENT_REDSTONE_TORCH_BURNOUT disagrees with LevelEvent"
        );
        assert_eq!(
            t.id("LAVA_FIZZ"),
            Some(crate::fluid::LEVEL_EVENT_LAVA_FIZZ),
            "crate::fluid::LEVEL_EVENT_LAVA_FIZZ disagrees with LevelEvent"
        );
        // A floor, so that a table which parsed to almost nothing could not
        // make the two assertions above vacuously true by holding exactly
        // those two rows.
        assert!(
            t.len() >= 80,
            "only {} level-event constants — the reflective dump has narrowed",
            t.len()
        );
    }

    fn fixture() -> serde_json::Value {
        serde_json::json!({
            "levelEvents": 3,
            "skippedFields": 0,
            "table": [
                {"name": "COMPOSTER_FILL", "id": 1500},
                {"name": "REDSTONE_TORCH_BURNOUT", "id": 1502},
                {"name": "PARTICLES_DESTROY_BLOCK", "id": 2001}
            ]
        })
    }

    #[test]
    fn the_table_parses_and_looks_up_by_name() {
        let t = LevelEvents::from_json(&fixture()).expect("the fixture parses");
        assert_eq!(t.len(), 3);
        assert_eq!(t.id("PARTICLES_DESTROY_BLOCK"), Some(2001));
        assert_eq!(t.id("COMPOSTER_FILL"), Some(1500));
        assert_eq!(t.id("NOT_AN_EVENT"), None);
    }

    /// Two constants sharing an id would mean the dump had started reading
    /// something other than `LevelEvent`'s fields — and every lookup would
    /// still answer, with the wrong number.
    #[test]
    fn a_duplicate_id_is_refused() {
        let mut doc = fixture();
        doc["table"][2]["id"] = serde_json::json!(1500);
        assert!(LevelEvents::from_json(&doc).is_err());

        let mut doc = fixture();
        doc["levelEvents"] = serde_json::json!(9);
        assert!(LevelEvents::from_json(&doc).is_err());

        assert!(LevelEvents::from_json(&serde_json::json!({"levelEvents": 0})).is_err());
    }
}
