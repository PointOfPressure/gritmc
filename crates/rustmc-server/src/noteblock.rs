//! `NoteBlock` — the only sound in this server that travels as a block event,
//! and the only one loud enough to reach past sixteen blocks.
//!
//! # Why it is not in `crate::world_send`'s `use_sound`
//!
//! Every other use-sound this server plays is `level.playSound(...)` from
//! inside `useWithoutItem`, on the wire before the block update. A note block
//! is two hops:
//!
//! ```text
//!   NoteBlock.useWithoutItem, offsets 7-32
//!       state = state.cycle(NOTE); level.setBlock(pos, state, 3);
//!       playNote(player, state, level, pos)
//!   NoteBlock.playNote, offsets 0-40
//!       if (state.getValue(INSTRUMENT).worksAboveNoteBlock()
//!           || level.getBlockState(pos.above()).isAir())
//!           level.blockEvent(pos, this, 0, 0);
//!   NoteBlock.triggerEvent, offsets 0-186        <- at the TICK BOUNDARY
//!       playSeededSound(null, x+0.5, y+0.5, z+0.5, instrument.getSoundEvent(),
//!                       RECORDS, 3.0f, pitch, random.nextLong())
//! ```
//!
//! `Level.blockEvent` queues; `ServerLevel.runBlockEvents` drains at the tick
//! boundary. So the sound is **behind** the block update on the wire, not
//! ahead of it, which is the opposite of every other block sound here — and it
//! is the reason scenario `430` names the block event in its `observe` list
//! alongside `sound`.
//!
//! # Three constants a guess gets wrong
//!
//! * **`SoundSource.RECORDS`, ordinal 2.** Not `BLOCKS`. `getstatic
//!   SoundSource.RECORDS` at `triggerEvent` offset 165, and the recordings
//!   agree: every one of the 60 `block.note_block.harp` observations on this
//!   box carries `source: 2`.
//! * **volume 3.0**, `NoteBlock.NOTE_VOLUME`, `ldc_w float 3.0f` at offset
//!   168. This is the **only** volume above 1.0 anything in this server plays,
//!   so it is the only place `SoundEvent.getRange`'s `16.0f * volume` branch
//!   fires: a note block is audible at **48 blocks** where a placed stone is
//!   audible at 16. See [`crate::sound_events::range`].
//! * **the pitch is a table lookup, not a constant.**
//!   `instrument.isTunable() ? getPitchFromNote(note) : 1.0f`, and
//!   `getPitchFromNote` is `(float) Math.pow(2.0, (note - 12) / 12.0)` —
//!   a *double* pow narrowed to a float, which is not the same as computing
//!   it in `f32`.
//!
//! # What is refused
//!
//! **`CUSTOM_HEAD`.** `triggerEvent` offsets 99-113 call `getCustomSoundId`,
//! which reads the `note_block_sound` off the skull block entity above, and
//! **returns false** — playing nothing — when it is absent. This server has no
//! such block entity field, so [`trigger_event`] returns `None` for that
//! instrument rather than guessing an event. That is the same answer vanilla
//! gives for a head with no custom sound and the wrong answer for a head with
//! one; it is stated here rather than left, and it is unreachable from the
//! corpus, which only ever plays `harp`.
//!
//! **The instrument itself is not chosen here.** `NoteBlock.setInstrument`
//! reads the block above and below at placement time and writes the
//! `instrument` property into the state; this module reads that property off
//! the state it is given. A wrong instrument is a placement defect, not a
//! sound one.

use std::collections::BTreeMap;
use std::io;

/// Where `ci/sound-particle-golden.sh` writes the table.
pub const TABLE_PATH: &str = "data/vanilla/note_block.json";

/// `NoteBlock.NOTE_VOLUME`, `ldc_w float 3.0f` at `triggerEvent` offset 168.
///
/// Asserted against the generated table rather than trusted: it is the one
/// volume in this server that changes an audible radius.
pub const NOTE_VOLUME: f32 = 3.0;

/// One `NoteBlockInstrument`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instrument {
    /// The registry name of the event it plays.
    pub sound: String,
    /// `isTunable()` — `type == BASE_BLOCK`. Decides whether the pitch comes
    /// from `NOTE` or is a flat 1.0.
    pub tunable: bool,
    /// `worksAboveNoteBlock()` — `type != BASE_BLOCK`, the exact complement of
    /// [`Self::tunable`] today. Decides whether `playNote` queues the event
    /// without checking the block above.
    pub works_above: bool,
    /// `hasCustomSound()` — `type == CUSTOM`, true for `custom_head` alone.
    pub custom_sound: bool,
}

/// `NoteBlockInstrument`'s 27 values, by their serialized names — which are
/// what the `instrument` block-state property carries.
#[derive(Debug, Default)]
pub struct Instruments {
    by_name: BTreeMap<String, Instrument>,
}

/// `NoteBlock.getPitchFromNote(int)`, offsets 0-16.
///
/// `(float) Math.pow(2.0, (note - 12) / 12.0)`. Three things about that line
/// are load-bearing and all three are easy to lose:
///
/// * the exponent's division is **double** division of an int difference, so
///   `(note - 12) / 12.0` and not an integer divide;
/// * `Math.pow` is evaluated in `f64` and narrowed **once**, at the end.
///   Computing the same expression in `f32` throughout gives a different last
///   bit for most of the 25 notes;
/// * note 12 is the middle and gives exactly 1.0, note 0 gives 0.5 and note
///   24 gives 2.0 — the octave either side.
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    reason = "the `d2f` at offset 15, which is the narrowing being reproduced"
)]
pub fn pitch_from_note(note: i32) -> f32 {
    2.0_f64.powf(f64::from(note - 12) / 12.0) as f32
}

impl Instruments {
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
    /// On a missing `table`, a row without the four fields, a declared count
    /// that disagrees with the rows, a `noteVolume` that is not
    /// [`NOTE_VOLUME`], or a row whose `tunable` and `worksAboveNoteBlock`
    /// are not complements.
    ///
    /// That last refusal is the interesting one. They are complements because
    /// `NoteBlockInstrument.Type` has three values and both predicates test
    /// against `BASE_BLOCK` — one for equality and one for inequality. A
    /// fourth `Type` would separate them, and this reader would then be
    /// treating "queues without looking up" and "reads its pitch from NOTE"
    /// as one fact when they had become two. Refusing is how that gets read
    /// rather than absorbed.
    pub fn from_json(doc: &serde_json::Value) -> io::Result<Self> {
        let bad = |m: String| io::Error::new(io::ErrorKind::InvalidData, m);
        let rows = doc
            .get("table")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| bad("note_block.json has no `table` array".into()))?;
        let declared = doc
            .get("instruments")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| bad("note_block.json has no `instruments` count".into()))?;
        if declared as usize != rows.len() {
            return Err(bad(format!(
                "note_block.json declares {declared} instruments and carries {} rows",
                rows.len()
            )));
        }
        let volume = doc
            .get("noteVolume")
            .and_then(serde_json::Value::as_f64)
            .ok_or_else(|| bad("note_block.json has no `noteVolume`".into()))?;
        // Bit patterns, not `==`. NOTE_VOLUME is an exact small integer on
        // both sides, so this is the same comparison — but it is written so
        // that nobody later relaxes it into a tolerance, which would accept
        // exactly the number this refuses.
        if volume.to_bits() != f64::from(NOTE_VOLUME).to_bits() {
            return Err(bad(format!(
                "NoteBlock.NOTE_VOLUME is {volume} in the jar and {NOTE_VOLUME} here — \
                 that is the note block's audible radius, 16 * volume"
            )));
        }
        let mut by_name = BTreeMap::new();
        for (i, row) in rows.iter().enumerate() {
            let field = |k: &str| row.get(k).and_then(serde_json::Value::as_bool);
            let name = row
                .get("name")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| bad(format!("instrument row {i} has no `name`")))?;
            let sound = row
                .get("sound")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| bad(format!("instrument {name} has no `sound`")))?;
            let (tunable, works_above, custom_sound) = (
                field("tunable")
                    .ok_or_else(|| bad(format!("instrument {name} has no `tunable`")))?,
                field("worksAboveNoteBlock").ok_or_else(|| {
                    bad(format!("instrument {name} has no `worksAboveNoteBlock`"))
                })?,
                field("customSound")
                    .ok_or_else(|| bad(format!("instrument {name} has no `customSound`")))?,
            );
            if tunable == works_above {
                return Err(bad(format!(
                    "instrument {name}: tunable and worksAboveNoteBlock agree, and they are \
                     complements — NoteBlockInstrument.Type has changed shape"
                )));
            }
            by_name.insert(
                name.to_string(),
                Instrument {
                    sound: sound.to_string(),
                    tunable,
                    works_above,
                    custom_sound,
                },
            );
        }
        if by_name.len() != rows.len() {
            return Err(bad("two instruments share a name".into()));
        }
        Ok(Self { by_name })
    }

    /// The process-wide table, read once.
    ///
    /// # Errors
    /// The first caller's error, cloned by message for everyone after it.
    pub fn shared(path: &std::path::Path) -> io::Result<&'static Self> {
        static ONCE: std::sync::OnceLock<io::Result<Instruments>> = std::sync::OnceLock::new();
        match ONCE.get_or_init(|| Self::load(path)) {
            Ok(t) => Ok(t),
            Err(e) => Err(io::Error::new(e.kind(), e.to_string())),
        }
    }

    /// One instrument by the `instrument` property's value, e.g. `harp`.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Instrument> {
        self.by_name.get(name)
    }

    /// How many the table holds.
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

/// Whether this state is a note block at all.
#[must_use]
pub fn is_note_block(state: &str) -> bool {
    state.split('[').next().unwrap_or(state) == "minecraft:note_block"
}

/// `NoteBlock.playNote`'s guard, offsets 0-28.
///
/// ```text
///   state.getValue(INSTRUMENT).worksAboveNoteBlock()
///       || level.getBlockState(pos.above()).isAir()
/// ```
///
/// Short-circuiting: a mob-head or custom instrument queues without the world
/// being read at all, which is why `above_is_air` is a value rather than a
/// closure — the caller has already read it, and reading it twice would be
/// the cheaper mistake to make.
///
/// An unknown instrument answers `false`. That is the conservative direction:
/// a note block whose instrument this table does not know stays silent rather
/// than playing something. It is unreachable while the table is generated
/// from the same enum the block-state property is defined by.
#[must_use]
pub fn should_queue(instruments: &Instruments, state: &str, above_is_air: bool) -> bool {
    let Some(name) = crate::placement::property_of(state, "instrument") else {
        return false;
    };
    instruments
        .get(name)
        .is_some_and(|i| i.works_above || above_is_air)
}

/// `NoteBlock.triggerEvent`, offsets 0-186 — the sound, at the tick boundary.
///
/// `None` where vanilla returns `false` without sounding, which is exactly
/// one case: a `custom_head` whose custom sound id cannot be resolved. See
/// the module header for why that is always the answer here.
///
/// The `NOTE` property is read off the state the caller found in the world at
/// drain time, not off the state that queued the event, because that is what
/// `triggerEvent` does — `ServerLevel.doBlockEvent` re-reads the block first.
#[must_use]
pub fn trigger_event(
    instruments: &Instruments,
    state: &str,
    pos: crate::interact::BlockPos,
) -> Option<crate::sounds::Sound> {
    let instrument = instruments.get(crate::placement::property_of(state, "instrument")?)?;
    if instrument.custom_sound {
        custom_head_unported_once(pos);
        return None;
    }
    // `isTunable() ? getPitchFromNote(state.getValue(NOTE)) : 1.0f`. A note
    // that will not parse is note 0 rather than a refusal: `NOTE` is an
    // `IntegerProperty` with a fixed 0..24 range, so an unparseable one means
    // the state string is malformed, and 0 is the value a fresh note block
    // carries.
    let pitch = if instrument.tunable {
        let note = crate::placement::property_of(state, "note")
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(0);
        pitch_from_note(note)
    } else {
        1.0
    };
    Some(crate::sounds::Sound {
        event: instrument.sound.clone(),
        // `SoundSource.RECORDS`, not BLOCKS. Offset 165.
        source: crate::sounds::SOURCE_RECORDS,
        at: crate::sounds::Eighths::block(pos),
        pos: crate::sounds::block_centre(pos),
        volume: NOTE_VOLUME,
        pitch,
        // `aconst_null` at offset 135: the player who tuned it hears it, and
        // the recordings show this event on both streams.
        except: None,
    })
}

/// Warn once per position that a `custom_head` note block played nothing.
fn custom_head_unported_once(pos: crate::interact::BlockPos) {
    static SAID: std::sync::OnceLock<
        std::sync::Mutex<std::collections::BTreeSet<(i32, i32, i32)>>,
    > = std::sync::OnceLock::new();
    let said = SAID.get_or_init(|| std::sync::Mutex::new(std::collections::BTreeSet::new()));
    let mut said = said
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if said.insert(pos) {
        eprintln!(
            "note block: a custom_head instrument at {pos:?} — `getCustomSoundId` reads a \
             `note_block_sound` off the skull block entity above, which this server does not \
             carry, so nothing is played. Vanilla plays nothing here too unless that field is set."
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> serde_json::Value {
        serde_json::json!({
            "instruments": 3,
            "tunableInstruments": 1,
            "customSoundInstruments": 1,
            "noteVolume": 3,
            "table": [
                {"name": "harp", "ordinal": 0, "sound": "minecraft:block.note_block.harp",
                 "soundId": 1167, "tunable": true, "worksAboveNoteBlock": false,
                 "customSound": false},
                {"name": "zombie", "ordinal": 20, "sound": "minecraft:block.note_block.imitate_zombie",
                 "soundId": 1, "tunable": false, "worksAboveNoteBlock": true,
                 "customSound": false},
                {"name": "custom_head", "ordinal": 26, "sound": "minecraft:intentionally_empty",
                 "soundId": 2, "tunable": false, "worksAboveNoteBlock": true,
                 "customSound": true}
            ]
        })
    }

    fn harp(note: i32) -> String {
        format!("minecraft:note_block[instrument=harp,note={note},powered=false]")
    }

    /// `(float) Math.pow(2.0, (note - 12) / 12.0)`, at the three points the
    /// formula is exact and at one where it is not.
    #[test]
    fn the_pitch_is_an_octave_either_side_of_note_twelve() {
        assert_eq!(pitch_from_note(12), 1.0);
        assert_eq!(pitch_from_note(0), 0.5);
        assert_eq!(pitch_from_note(24), 2.0);
        // A semitone. An integer divide in the exponent would give 1.0 here,
        // which is the mistake that passes all three tests above.
        assert!((pitch_from_note(13) - 1.059_463_1).abs() < 1e-6);
        assert!(pitch_from_note(11) < 1.0);
        // Monotonic across the whole property range, which an exponent with
        // the wrong sign would not be.
        for n in 1..=24 {
            assert!(
                pitch_from_note(n) > pitch_from_note(n - 1),
                "note {n} is not above note {}",
                n - 1
            );
        }
    }

    #[test]
    fn a_tuned_harp_sounds_at_records_volume_three() {
        let t = Instruments::from_json(&fixture()).expect("the fixture parses");
        let s = trigger_event(&t, &harp(5), (1, 0, 0)).expect("a harp sounds");
        assert_eq!(s.event, "minecraft:block.note_block.harp");
        assert_eq!(s.source, crate::sounds::SOURCE_RECORDS);
        assert_eq!(
            s.source, 2,
            "RECORDS is ordinal 2, and the recordings say 2"
        );
        assert_eq!(s.volume, 3.0);
        assert_eq!(s.pos, (1.5, 0.5, 0.5));
        assert_eq!(s.except, None);
        assert_eq!(s.pitch, pitch_from_note(5));
    }

    /// The volume is the only one in this server above 1.0, so it is the only
    /// place `getRange`'s multiply fires. 48 blocks, not 16.
    #[test]
    fn a_note_block_is_audible_three_times_as_far_as_anything_else() {
        assert_eq!(crate::sound_events::range(None, NOTE_VOLUME), 48.0);
        assert_eq!(crate::sound_events::range(None, 1.0), 16.0);
    }

    /// An untunable instrument takes the flat 1.0 rather than the note's
    /// pitch — `fconst_1; fstore 6` at offset 88.
    #[test]
    fn an_untunable_instrument_ignores_the_note() {
        let t = Instruments::from_json(&fixture()).expect("the fixture parses");
        let state = "minecraft:note_block[instrument=zombie,note=24,powered=false]";
        let s = trigger_event(&t, state, (0, 0, 0)).expect("a zombie head sounds");
        assert_eq!(s.pitch, 1.0, "note 24 would be 2.0 if it were tunable");
        assert_eq!(s.event, "minecraft:block.note_block.imitate_zombie");
    }

    /// The one refusal, and the reason it is a refusal rather than a guess.
    #[test]
    fn a_custom_head_plays_nothing() {
        let t = Instruments::from_json(&fixture()).expect("the fixture parses");
        let state = "minecraft:note_block[instrument=custom_head,note=0,powered=false]";
        assert!(trigger_event(&t, state, (0, 0, 0)).is_none());
    }

    /// `worksAboveNoteBlock() || above.isAir()`, and the short circuit.
    #[test]
    fn the_queue_guard_reads_the_block_above_only_when_it_has_to() {
        let t = Instruments::from_json(&fixture()).expect("the fixture parses");
        // harp is a BASE_BLOCK instrument: it needs air above.
        assert!(should_queue(&t, &harp(0), true));
        assert!(!should_queue(&t, &harp(0), false));
        // zombie is not: it queues either way.
        let zombie = "minecraft:note_block[instrument=zombie,note=0,powered=false]";
        assert!(should_queue(&t, zombie, true));
        assert!(should_queue(&t, zombie, false));
        // An instrument this table does not know stays silent.
        let unknown = "minecraft:note_block[instrument=kazoo,note=0,powered=false]";
        assert!(!should_queue(&t, unknown, true));
    }

    #[test]
    fn is_note_block_matches_the_block_and_not_a_prefix() {
        assert!(is_note_block("minecraft:note_block"));
        assert!(is_note_block(&harp(3)));
        assert!(!is_note_block("minecraft:note_block_but_not_really"));
        assert!(!is_note_block("minecraft:stone"));
    }

    /// The refusals that keep a table which is wrong in a way nothing else
    /// would notice from loading.
    #[test]
    fn a_table_that_would_be_silently_wrong_is_refused() {
        // The volume is the audible radius. A 1.0 here would quietly bring
        // every note block in from 48 blocks to 16.
        let mut doc = fixture();
        doc["noteVolume"] = serde_json::json!(1);
        assert!(Instruments::from_json(&doc).is_err());

        // tunable and worksAboveNoteBlock are complements today, and a table
        // where they are not means `Type` has grown a fourth value.
        let mut doc = fixture();
        doc["table"][0]["worksAboveNoteBlock"] = serde_json::json!(true);
        assert!(Instruments::from_json(&doc).is_err());

        let mut doc = fixture();
        doc["instruments"] = serde_json::json!(9);
        assert!(Instruments::from_json(&doc).is_err());
    }

    /// The generated table, against the two things the corpus depends on.
    #[test]
    fn the_generated_table_has_all_of_the_instruments_and_a_tunable_harp() {
        let path = std::path::Path::new("../..").join(TABLE_PATH);
        if !rustmc_data::fixture::available(
            &path,
            "note-block instrument table",
            "ci/sound-particle-golden.sh",
        ) {
            return;
        }
        let t = Instruments::load(&path).expect("the generated table loads");
        assert!(
            t.len() >= 25,
            "only {} instruments — NoteBlockInstrument has 27 values in 26.2",
            t.len()
        );
        let harp = t.get("harp").expect("harp is setInstrument's fallback");
        assert_eq!(harp.sound, "minecraft:block.note_block.harp");
        assert!(harp.tunable);
        assert!(!harp.works_above);
        assert!(!harp.custom_sound);
        assert!(
            t.get("custom_head").is_some_and(|i| i.custom_sound),
            "custom_head is the one CUSTOM instrument and the one this server refuses"
        );
    }
}
