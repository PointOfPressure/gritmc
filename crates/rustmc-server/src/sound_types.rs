//! `SoundType` — the two floats and five events behind every block state.
//!
//! # One table, three arithmetics
//!
//! Vanilla keeps a `SoundType` on `BlockBehaviour.Properties` and every site
//! that plays a block's sound reads the same two floats off it and does its
//! own sum:
//!
//! ```text
//!   BlockItem.place 223-239   volume (v + 1.0F) / 2.0F   pitch p * 0.8F
//!   Entity.playStepSound      volume v * 0.15F           pitch p
//!   Block break particles     volume (v + 1.0F) / 2.0F   pitch p * 0.8F
//! ```
//!
//! So the table carries `v` and `p` and the *event names*, and the arithmetic
//! stays at the call site where the bytecode puts it. Folding `(v+1)/2` into
//! the table would make the same row unusable by the step sound.
//!
//! # Why it is keyed by state and not by block
//!
//! `BlockBehaviour.getSoundType(BlockState)` is virtual. Its base returns the
//! `soundType` field, and subclasses override it. Asking each of the 32,366
//! states costs one `u16` per state in the file and is correct for every
//! override without this module having to know which classes override it —
//! and `ci/sound-types-golden.sh` prints how many blocks actually do, so the
//! claim is measured rather than assumed.
//!
//! **In 26.2 that number is one, and it is `decorated_pot`** (measured
//! 2026-08-08 by joining `stateTypes` against `block_props.json`'s per-state
//! block names). `cracked=true` and `cracked=false` are two distinct
//! `SoundType` instances whose **only** differing field is the break sound —
//! `decorated_pot.shatter` against `decorated_pot.break` — with identical
//! place, step, hit, fall, volume and pitch.
//!
//! So: keying by state buys **nothing today** for the two call sites that
//! exist, and the honest version of the argument is not "this was necessary"
//! but "this costs 64 KB and removes a class of wrong answer that the next
//! call site — a break sound — would immediately have hit". A per-block table
//! would give a cracked pot the wrong break sound the day somebody adds one,
//! and nothing would fail until a scenario listened for it.
//!
//! # What is not here
//!
//! `SoundEvent.fixedRange`. Every event in the table is a variable-range one
//! and the packet does not carry the range anyway; `ServerLevel.playSound`
//! uses it only to decide the broadcast radius, which this server does not
//! model (see [`crate::sounds`]).

use std::collections::BTreeMap;
use std::io;
use std::sync::OnceLock;

/// Where the extract lives, relative to the server's working directory.
///
/// **Load-bearing outside Rust**: `ci/gen-gameplay-tables.sh` derives the set
/// of tables a tree needs by grepping the repo for
/// `"data/vanilla/<name>.json"`.
pub const TABLE_PATH: &str = "data/vanilla/sound_types.json";

/// One `SoundType` instance.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// `SoundType.getVolume()`. Not a packet field: every call site scales it.
    pub volume: f32,
    /// `SoundType.getPitch()`. Same.
    pub pitch: f32,
    pub break_sound: String,
    pub step_sound: String,
    pub place_sound: String,
    pub hit_sound: String,
    pub fall_sound: String,
}

/// The table: distinct sound types, and one index per global state id.
#[derive(Debug, Clone)]
pub struct SoundTypes {
    rows: Vec<Row>,
    state_rows: Vec<u16>,
}

fn bad(what: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("sound_types.json: {what}"),
    )
}

fn f32_bits(v: Option<&serde_json::Value>) -> Option<f32> {
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    v.and_then(serde_json::Value::as_i64)
        .map(|b| f32::from_bits(b as u32))
}

fn text(r: &serde_json::Value, key: &str) -> io::Result<String> {
    r.get(key)
        .and_then(serde_json::Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| bad(&format!("a row with no `{key}`")))
}

impl SoundTypes {
    /// Load from an explicit path.
    ///
    /// # Errors
    /// If the file is missing or malformed.
    pub fn load(path: &std::path::Path) -> io::Result<Self> {
        let doc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path)?)
            .map_err(|e| bad(&format!("not json: {e}")))?;
        Self::from_json(&doc)
    }

    /// Parse the dumper's output.
    ///
    /// # Errors
    /// If a row is malformed, or if any state maps to no row. The generator
    /// refuses to emit an unmapped state, so one arriving here means the file
    /// was edited or truncated — and a default here would be a block that
    /// places silently, which is indistinguishable from this server not having
    /// the feature at all.
    pub fn from_json(doc: &serde_json::Value) -> io::Result<Self> {
        let mut rows = Vec::new();
        for r in doc
            .get("table")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| bad("no `table` array"))?
        {
            rows.push(Row {
                volume: f32_bits(r.get("volumeBits"))
                    .ok_or_else(|| bad("a row with no `volumeBits`"))?,
                pitch: f32_bits(r.get("pitchBits"))
                    .ok_or_else(|| bad("a row with no `pitchBits`"))?,
                break_sound: text(r, "break")?,
                step_sound: text(r, "step")?,
                place_sound: text(r, "place")?,
                hit_sound: text(r, "hit")?,
                fall_sound: text(r, "fall")?,
            });
        }

        let raw = doc
            .get("stateTypes")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| bad("no `stateTypes` array"))?;
        let mut state_rows = Vec::with_capacity(raw.len());
        for (id, v) in raw.iter().enumerate() {
            let i = v
                .as_i64()
                .ok_or_else(|| bad(&format!("state {id} has a non-integer row")))?;
            let i = usize::try_from(i)
                .map_err(|_| bad(&format!("state {id} maps to row {i} — the dump is broken")))?;
            if i >= rows.len() {
                return Err(bad(&format!(
                    "state {id} maps to row {i} of {}",
                    rows.len()
                )));
            }
            #[allow(clippy::cast_possible_truncation)]
            state_rows.push(i as u16);
        }

        if rows.is_empty() || state_rows.is_empty() {
            return Err(bad("empty — every block would place silently"));
        }
        Ok(Self { rows, state_rows })
    }

    /// The process-wide table, loaded once.
    ///
    /// Shared for the reason the shape table is: this is read on the
    /// per-connection path and the file is a megabyte.
    pub fn shared(path: &std::path::Path) -> io::Result<&'static Self> {
        static ONCE: OnceLock<io::Result<SoundTypes>> = OnceLock::new();
        match ONCE.get_or_init(|| Self::load(path)) {
            Ok(t) => Ok(t),
            Err(e) => Err(io::Error::new(e.kind(), e.to_string())),
        }
    }

    /// The row for a global block-state id.
    ///
    /// `None` only for an id outside the table, which is an id from another
    /// version's registry rather than a block with no sound.
    #[must_use]
    pub fn row(&self, state_id: u32) -> Option<&Row> {
        let i = *self.state_rows.get(usize::try_from(state_id).ok()?)?;
        self.rows.get(usize::from(i))
    }

    /// How many distinct sound types the table holds, for the boot log.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether the table holds nothing. Never true for a loaded table —
    /// [`Self::from_json`] refuses an empty one — and present because clippy
    /// asks for it beside `len`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// How many states there are, for the boot log.
    #[must_use]
    pub fn states(&self) -> usize {
        self.state_rows.len()
    }

    /// Which distinct place events the table holds, and how many states each
    /// covers. Only used by tests and by the boot log's one-line summary; a
    /// `BTreeMap` so the order is the event name's.
    #[must_use]
    pub fn place_histogram(&self) -> BTreeMap<&str, usize> {
        let mut out = BTreeMap::new();
        for i in &self.state_rows {
            *out.entry(self.rows[usize::from(*i)].place_sound.as_str())
                .or_insert(0) += 1;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> SoundTypes {
        let doc = serde_json::json!({
            "states": 3,
            "soundTypes": 2,
            "table": [
                {"volume": 1.0, "volumeBits": 1_065_353_216_i64,
                 "pitch": 1.0, "pitchBits": 1_065_353_216_i64,
                 "break": "minecraft:block.grass.break",
                 "step": "minecraft:block.grass.step",
                 "place": "minecraft:block.grass.place",
                 "hit": "minecraft:block.grass.hit",
                 "fall": "minecraft:block.grass.fall"},
                {"volume": 1.0, "volumeBits": 1_065_353_216_i64,
                 "pitch": 1.5, "pitchBits": 1_069_547_520_i64,
                 "break": "minecraft:block.metal.break",
                 "step": "minecraft:block.metal.step",
                 "place": "minecraft:block.metal.place",
                 "hit": "minecraft:block.metal.hit",
                 "fall": "minecraft:block.metal.fall"}
            ],
            "stateTypes": [0, 1, 1]
        });
        SoundTypes::from_json(&doc).expect("the fixture parses")
    }

    /// The floats come from the **bits**, not from the printed decimal.
    /// `1.5f` survives either way; the point of the assertion is that the
    /// reader is reading the field that cannot be lossy.
    #[test]
    fn a_row_is_read_from_its_raw_bits() {
        let t = fixture();
        assert!((t.row(1).expect("row").pitch - 1.5).abs() < f32::EPSILON);
        assert_eq!(
            t.row(1).expect("row").place_sound,
            "minecraft:block.metal.place"
        );
        assert_eq!(
            t.row(0).expect("row").step_sound,
            "minecraft:block.grass.step"
        );
    }

    /// An id past the end is `None` rather than row zero. Row zero would be a
    /// silently plausible answer — every unknown block would sound like
    /// whatever happened to be dumped first.
    #[test]
    fn an_unknown_state_has_no_row() {
        assert!(fixture().row(99).is_none());
    }

    /// A state pointing past the table is a refusal, not a default. This is
    /// the shape of a truncated file, and a reader that clamped would give
    /// every block the last row's sound.
    #[test]
    fn a_state_row_out_of_range_is_refused() {
        let doc = serde_json::json!({
            "table": [{"volume": 1.0, "volumeBits": 1_065_353_216_i64,
                       "pitch": 1.0, "pitchBits": 1_065_353_216_i64,
                       "break": "b", "step": "s", "place": "p", "hit": "h", "fall": "f"}],
            "stateTypes": [0, 7]
        });
        assert!(SoundTypes::from_json(&doc).is_err());
    }
}
