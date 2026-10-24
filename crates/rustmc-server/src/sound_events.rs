//! `BuiltInRegistries.SOUND_EVENT` — 1,968 events, and the one field on them
//! that changes who hears a sound.
//!
//! # What this table is for, given that the ids already come from elsewhere
//!
//! [`crate::protocol_ids`] already carries a name -> registry id map, scraped
//! from Mojang's `generated/reports/registries.json`, and that is what
//! [`crate::sounds::send`] writes into the packet. This table is **not** a
//! second copy of that number for its own sake: it exists for
//! [`SoundEvent::fixed_range`], which the report does not carry and which no
//! amount of reading a sound's *name* will tell you.
//!
//! `ServerLevel.playSeededSound` offsets 41-46:
//!
//! ```text
//!   playerList.broadcast(exceptPlayer, x, y, z,
//!                        sound.value().getRange(volume),   // <- the radius
//!                        dimension(), new ClientboundSoundPacket(...));
//! ```
//!
//! and `SoundEvent.getRange(float)` is, from its own bytecode, offsets 0-31:
//!
//! ```text
//!   fixedRange.orElse(volume > 1.0f ? 16.0f * volume : 16.0f)
//! ```
//!
//! Note the shape: `Optional.orElse` evaluates its argument either way but
//! **returns the fixed range whenever there is one**, so a fixed-range event
//! ignores the volume entirely.
//!
//! # And in 26.2 no built-in event has one
//!
//! This module was written expecting a handful of loud events to carry a
//! fixed range. **Zero of the 1,968 do**, measured 2026-09-05 three ways that
//! agree: the dumper reports 0; `SoundEvents.<clinit>` makes 1,905
//! registrations and not one goes through the private
//! `register(Identifier, Identifier, float)` that would call
//! `createFixedRangeEvent`; and across all 7,434 classes in the jar that
//! method name appears in exactly two constant pools — `SoundEvent`'s own and
//! that unreachable helper. The field is for **datapack** sounds, which
//! supply it through `SoundEvent.DIRECT_CODEC`.
//!
//! So [`range`]'s first branch is unreachable from this table today, and that
//! is said here rather than left to be discovered:
//!
//! * it is **not** deleted, because `getRange` has it and deleting it would
//!   be an approximation rather than a deferral — a datapack sound or a later
//!   version reaches it, and nothing else would then be wrong in a way
//!   anybody could see;
//! * it is exercised by fixture in this module's tests and in
//!   [`crate::sounds`]', which build a fixed-range event by hand, so it is
//!   covered code rather than dead code;
//! * the zero is **asserted** by `ci/sound-particle-golden.sh`, which fails
//!   on any other number. A version that adds one changes how far that sound
//!   carries and has to be read, not absorbed;
//! * and the zero has a positive control, because "no event has one" and
//!   "the dumper cannot read the field" are the same number. The generator
//!   builds a fixed-range event and checks `fixedRange()` and `getRange()`
//!   both see it, and refuses to write the table if they do not.
//!
//! What the table therefore does today is the id and the name: see below.
//!
//! The ids are carried anyway, and that is deliberate rather than
//! redundant. Both this table and `protocol_ids.json` are derived from the
//! same `registries.json` report — `ci/sound-particle-golden.sh` asserts its
//! whole name->id map against it and exits 1 on any disagreement — so
//! carrying the id here makes that agreement a property somebody can check
//! rather than a coincidence two scripts happen to share. If they ever
//! diverge, the generator says so before either reaches a packet.
//!
//! # What is not here
//!
//! * **The radius is not applied by this module.** [`range`] is the rule;
//!   [`crate::sounds::send`] is the only place that can use it, because only
//!   it knows where the listeners are.
//! * **`SoundEvent.location()` is recorded and unused.** It is the identifier
//!   a resource pack resolves, and it is not always the registry key — the
//!   generator counts the disagreements. Nothing on the wire carries it, so
//!   nothing here reads it.

use std::collections::BTreeMap;
use std::io;
use std::sync::OnceLock;

/// Where `ci/sound-particle-golden.sh` writes the table.
pub const TABLE_PATH: &str = "data/vanilla/sound_events.json";

/// `16.0f`, the default audible radius, from `SoundEvent.getRange` offset 10.
pub const DEFAULT_RANGE: f32 = 16.0;

/// One registry entry.
#[derive(Debug, Clone, PartialEq)]
pub struct SoundEvent {
    /// `Registry.getId` — the number the packet's `Holder` varint carries,
    /// plus one.
    pub id: i32,
    /// `SoundEvent.fixedRange()`, empty for all but a handful of events.
    pub fixed_range: Option<f32>,
}

/// `SoundEvent.getRange(float)`, exactly.
///
/// `fixedRange.orElse(volume > 1.0f ? 16.0f * volume : 16.0f)`. Three things
/// about it are easy to get wrong and all three are pinned by the tests
/// below:
///
/// * the comparison is `volume > 1.0`, strictly, so volume 1.0 takes the
///   plain 16 rather than the multiply;
/// * the multiply is `16.0f * volume` and the product is an `f32`, not a
///   double promoted at the call site — `ServerLevel` does the `f2d` *after*
///   this returns;
/// * a fixed range wins **regardless of volume**, because `orElse` is not a
///   fallback for a small volume, it is a fallback for an absent field.
#[must_use]
pub fn range(fixed_range: Option<f32>, volume: f32) -> f32 {
    if let Some(r) = fixed_range {
        return r;
    }
    if volume > 1.0 {
        DEFAULT_RANGE * volume
    } else {
        DEFAULT_RANGE
    }
}

/// The whole table, by registry name.
#[derive(Debug, Default)]
pub struct SoundEventTable {
    by_name: BTreeMap<String, SoundEvent>,
    fixed_range_events: usize,
}

impl SoundEventTable {
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
    /// On a missing `table`, a row missing `id` or `name`, a row count that
    /// disagrees with the declared `events`, or a `fixedRangeBits` that is
    /// not a float this server would broadcast anything with.
    ///
    /// The last of those is the one worth having. A `null` range and a range
    /// of `0.0` are one keystroke apart in a generator and completely
    /// different on the wire: the first means "audible at 16 blocks", the
    /// second means "audible to nobody, ever", and a server that loaded the
    /// second would go quiet for that event with no error anywhere.
    pub fn from_json(doc: &serde_json::Value) -> io::Result<Self> {
        let bad = |m: String| io::Error::new(io::ErrorKind::InvalidData, m);
        let rows = doc
            .get("table")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| bad("sound_events.json has no `table` array".into()))?;
        let declared = doc
            .get("events")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| bad("sound_events.json has no `events` count".into()))?;
        if declared as usize != rows.len() {
            return Err(bad(format!(
                "sound_events.json declares {declared} events and carries {} rows",
                rows.len()
            )));
        }
        let mut by_name = BTreeMap::new();
        let mut fixed_range_events = 0usize;
        for (i, row) in rows.iter().enumerate() {
            let name = row
                .get("name")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| bad(format!("sound event row {i} has no `name`")))?;
            let id = row
                .get("id")
                .and_then(serde_json::Value::as_i64)
                .ok_or_else(|| bad(format!("sound event row {i} ({name}) has no `id`")))?;
            let id = i32::try_from(id).map_err(|_| {
                bad(format!(
                    "sound event {name} has id {id}, which is not an i32"
                ))
            })?;
            // The bits, not the decimal. A `Float.toString` of `16.0f` and a
            // parse of `"16.0"` agree; one of `1.0E-5f` and its parse need
            // not, and this reader has no business knowing which events are
            // which.
            let fixed_range = match row.get("fixedRangeBits") {
                None | Some(serde_json::Value::Null) => None,
                Some(v) => {
                    let bits = v.as_i64().ok_or_else(|| {
                        bad(format!(
                            "sound event {name} has a non-integer fixedRangeBits"
                        ))
                    })?;
                    let bits = u32::try_from(bits & 0xFFFF_FFFF).map_err(|_| {
                        bad(format!(
                            "sound event {name} has out-of-range fixedRangeBits"
                        ))
                    })?;
                    let r = f32::from_bits(bits);
                    if !r.is_finite() || r <= 0.0 {
                        return Err(bad(format!(
                            "sound event {name} has fixed range {r}, which broadcasts to nobody"
                        )));
                    }
                    fixed_range_events += 1;
                    Some(r)
                }
            };
            by_name.insert(name.to_string(), SoundEvent { id, fixed_range });
        }
        if by_name.len() != rows.len() {
            return Err(bad(format!(
                "{} rows collapsed to {} names — the registry has a duplicate key",
                rows.len(),
                by_name.len()
            )));
        }
        Ok(Self {
            by_name,
            fixed_range_events,
        })
    }

    /// The process-wide table, read once.
    ///
    /// # Errors
    /// The first caller's error, cloned by message for everyone after it —
    /// the same shape [`crate::sound_types::SoundTypes::shared`] uses.
    pub fn shared(path: &std::path::Path) -> io::Result<&'static Self> {
        static ONCE: OnceLock<io::Result<SoundEventTable>> = OnceLock::new();
        match ONCE.get_or_init(|| Self::load(path)) {
            Ok(t) => Ok(t),
            Err(e) => Err(io::Error::new(e.kind(), e.to_string())),
        }
    }

    /// One event by registry name, e.g. `minecraft:block.stone.place`.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&SoundEvent> {
        self.by_name.get(name)
    }

    /// `SoundEvent.getRange(volume)` for an event this table knows, and the
    /// variable-range answer for one it does not.
    ///
    /// The fallback is not a guess: an event absent from this table is one
    /// `crate::sounds::send` could not have numbered either, so the only
    /// packets that reach it are ones whose name is in `protocol_ids.json`
    /// and not here — which the generator's cross-check against the same
    /// report makes impossible. It is written as the *default* rather than as
    /// a panic because being audible at 16 blocks is the answer for 1,9xx of
    /// 1,968 events, and going silent would be the answer for none.
    #[must_use]
    pub fn range_of(&self, name: &str, volume: f32) -> f32 {
        range(self.get(name).and_then(|e| e.fixed_range), volume)
    }

    /// How many events the table holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    /// Whether the table is empty, which for a loaded table it never is.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    /// How many carry a fixed range — the column this table exists for.
    #[must_use]
    pub fn fixed_range_events(&self) -> usize {
        self.fixed_range_events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A four-row table with one fixed-range event, written out rather than
    /// read from `data/vanilla` so these tests measure this reader and not
    /// whether a tree has been through `ci/gen-gameplay-tables.sh`.
    fn fixture() -> serde_json::Value {
        serde_json::json!({
            "events": 4,
            "fixedRangeEvents": 1,
            "keyDiffersFromLocation": 0,
            "table": [
                {"id": 0, "name": "minecraft:a.quiet.one",
                 "location": "minecraft:a.quiet.one",
                 "fixedRange": null, "fixedRangeBits": null},
                {"id": 1, "name": "minecraft:block.stone.place",
                 "location": "minecraft:block.stone.place",
                 "fixedRange": null, "fixedRangeBits": null},
                // 64.0f is 0x42800000.
                {"id": 2, "name": "minecraft:a.loud.one",
                 "location": "minecraft:a.loud.one",
                 "fixedRange": 64.0, "fixedRangeBits": 0x4280_0000_i64},
                {"id": 3, "name": "minecraft:block.stone.step",
                 "location": "minecraft:block.stone.step",
                 "fixedRange": null, "fixedRangeBits": null}
            ]
        })
    }

    #[test]
    fn the_table_parses_and_keeps_ids_and_ranges() {
        let t = SoundEventTable::from_json(&fixture()).expect("the fixture parses");
        assert_eq!(t.len(), 4);
        assert_eq!(t.fixed_range_events(), 1);
        assert_eq!(t.get("minecraft:block.stone.place").expect("present").id, 1);
        assert_eq!(
            t.get("minecraft:a.loud.one").expect("present").fixed_range,
            Some(64.0)
        );
        assert_eq!(
            t.get("minecraft:block.stone.place")
                .expect("present")
                .fixed_range,
            None
        );
        assert!(t.get("minecraft:not.a.sound").is_none());
    }

    /// `volume > 1.0f`, strictly — the whole difference between this and a
    /// `>=` is the very common volume-1.0 case, where a `>=` would give the
    /// same 16 by arithmetic and hide the error until somebody passed 0.5.
    #[test]
    fn the_variable_range_is_sixteen_until_the_volume_passes_one() {
        assert_eq!(range(None, 0.15), 16.0);
        assert_eq!(range(None, 0.5), 16.0);
        assert_eq!(range(None, 1.0), 16.0);
        // Strictly greater: the next float above 1.0 already multiplies.
        assert!(range(None, 1.000_000_1) > 16.0);
        assert_eq!(range(None, 2.0), 32.0);
        assert_eq!(range(None, 4.0), 64.0);
    }

    /// The half of `orElse` that is easy to write as a fallback and is not
    /// one: a fixed range is returned at every volume, including volumes far
    /// above 1.0 where the variable rule would give a much larger number.
    #[test]
    fn a_fixed_range_ignores_the_volume_entirely() {
        assert_eq!(range(Some(64.0), 0.1), 64.0);
        assert_eq!(range(Some(64.0), 1.0), 64.0);
        // The variable rule would say 16 * 16 = 256 here. `orElse` does not.
        assert_eq!(range(Some(64.0), 16.0), 64.0);
        let t = SoundEventTable::from_json(&fixture()).expect("the fixture parses");
        assert_eq!(t.range_of("minecraft:a.loud.one", 16.0), 64.0);
        assert_eq!(t.range_of("minecraft:block.stone.place", 16.0), 256.0);
    }

    /// An event nobody has heard of is audible at 16, not silent. See
    /// [`SoundEventTable::range_of`] for why that is the right default and
    /// not a shrug.
    #[test]
    fn an_unknown_event_takes_the_variable_rule() {
        let t = SoundEventTable::from_json(&fixture()).expect("the fixture parses");
        assert_eq!(t.range_of("minecraft:not.a.sound", 1.0), 16.0);
        assert_eq!(t.range_of("minecraft:not.a.sound", 2.0), 32.0);
    }

    /// The positive control for the reader's refusals. Each of these
    /// documents is a table that would load as *something* under a lazier
    /// parse, and each of them would be wrong in a way nothing downstream
    /// could see.
    #[test]
    fn a_table_that_would_be_silently_wrong_is_refused() {
        // A count that disagrees with the rows: the generator wrote a
        // truncated table and said so in the header.
        let mut doc = fixture();
        doc["events"] = serde_json::json!(9);
        assert!(SoundEventTable::from_json(&doc).is_err());

        // A zero fixed range. This is the one that matters: it parses, it is
        // finite, and it broadcasts the event to nobody.
        let mut doc = fixture();
        doc["table"][2]["fixedRangeBits"] = serde_json::json!(0);
        assert!(SoundEventTable::from_json(&doc).is_err());

        // A negative one, from a sign bit that came along for the ride.
        let mut doc = fixture();
        doc["table"][2]["fixedRangeBits"] = serde_json::json!(0xC280_0000_u32);
        assert!(SoundEventTable::from_json(&doc).is_err());

        // NaN, which compares false against every radius and so makes
        // `distSq < r*r` false for every listener.
        let mut doc = fixture();
        doc["table"][2]["fixedRangeBits"] = serde_json::json!(0x7FC0_0000_u32);
        assert!(SoundEventTable::from_json(&doc).is_err());

        // No table at all — an empty document reads as "no sound has a fixed
        // range", which is a sentence, and it is false.
        assert!(SoundEventTable::from_json(&serde_json::json!({"events": 0})).is_err());
    }

    /// Two rows with one name would leave the map short and one of the two
    /// events looking up the other's range.
    #[test]
    fn a_duplicate_registry_name_is_refused() {
        let mut doc = fixture();
        doc["table"][3]["name"] = serde_json::json!("minecraft:a.loud.one");
        assert!(SoundEventTable::from_json(&doc).is_err());
    }
}
