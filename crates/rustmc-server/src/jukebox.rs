//! `JukeboxBlockEntity` and `JukeboxSongPlayer` — the record player.
//!
//! Transcribed from `JukeboxBlockEntity.setTheItem` (offsets 0-81),
//! `.notifyItemChangedInJukebox` (0-77) and `.tick` (0-9), and
//! `JukeboxSongPlayer.play` (0-66), `.stop` (0-58), `.tick` (0-79) and
//! `.shouldEmitJukeboxPlayingEvent` (0-18), in the pinned 26.2 jar.
//!
//! # Two packets and a block state, and none of them is a `block_entity_data`
//!
//! `JukeboxBlockEntity` inherits `BlockEntity.getUpdatePacket()`, so putting
//! a disc in one sends nothing about the block entity at all. What the client
//! sees is:
//!
//! * a **block update**, from `notifyItemChangedInJukebox`'s
//!   `setBlock(pos, state.setValue(HAS_RECORD, …), 2)` — flag **2**, not 3,
//!   so no neighbour update goes out;
//! * `level_event(1010, pos, songId)` to start and `level_event(1011, pos, 0)`
//!   to stop, both literals in `JukeboxSongPlayer`.
//!
//! `notifyItemChangedInJukebox` opens with a guard nothing else here has:
//! offsets 0-25 return early unless `level.getBlockState(pos)` **is the same
//! object as** `getBlockState()`. A jukebox whose block was replaced under it
//! writes no state and sends no packet.
//!
//! # The song id is not certified and this module says so at the point of use
//!
//! `play` sends `registryAccess().lookupOrThrow(JUKEBOX_SONG).getId(song)`,
//! a **datapack registry index**. `data/vanilla/jukebox_songs.json` carries
//! the alphabetical rank because that is what a server loading the registry
//! through `RegistryDataLoader` over a sorted resource map is expected to
//! produce — and the only order this project can extract without a running
//! server, the data generator's bootstrap order, turned out not to be
//! reproducible between JVM runs at all. See [`JukeboxSongs::song_id`].

use crate::item::ItemStack;
use std::collections::BTreeMap;
use std::path::Path;

/// Where the golden writes the song table.
pub const DEFAULT_PATH: &str = "data/vanilla/jukebox_songs.json";

/// `LevelEvent.SOUND_PLAY_JUKEBOX_SONG` — `sipush 1010` at
/// `JukeboxSongPlayer.play` offset 44.
pub const PLAY_LEVEL_EVENT: i32 = 1010;

/// `LevelEvent.SOUND_STOP_JUKEBOX_SONG` — `sipush 1011` at `.stop` offset 36.
pub const STOP_LEVEL_EVENT: i32 = 1011;

/// The tail `hasFinished` adds to a song's own length — `bipush 20; iadd` at
/// `JukeboxSong.hasFinished` offset 5.
///
/// So a song stops at `lengthInTicks + 20` and not at `lengthInTicks`, and
/// the test is `>=`. A reimplementation that dropped the twenty would stop
/// every disc a second early.
pub const FINISHED_TAIL_TICKS: i64 = 20;

/// How often a playing jukebox fires `GameEvent.JUKEBOX_PLAY` —
/// `ticksSinceSongStarted % 20 == 0`, the `ldc2_w 20l; lrem` at
/// `shouldEmitJukeboxPlayingEvent` offset 4.
///
/// The counter is incremented **after** the test (offsets 69-76), so tick 0 —
/// the first tick after `play` zeroed it — fires the event.
pub const PLAYING_EVENT_PERIOD: i64 = 20;

/// One row of `data/vanilla/jukebox_songs.json`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Song {
    /// The song's registry id, e.g. `minecraft:pigstep`.
    pub id: String,
    /// Its position when the registry's ids are sorted by name.
    pub alphabetical_rank: i32,
    /// `Mth.ceil(lengthInSeconds * 20.0f)`.
    pub length_in_ticks: i32,
    /// What a comparator reads off the jukebox while this plays.
    pub comparator_output: i32,
}

/// The jukebox song table.
#[derive(Debug, Default)]
pub struct JukeboxSongs {
    by_id: BTreeMap<String, Song>,
}

impl JukeboxSongs {
    /// Parse `data/vanilla/jukebox_songs.json`.
    ///
    /// # Errors
    /// If the file is missing, is not JSON, or carries no songs.
    pub fn load(path: &Path) -> std::io::Result<Self> {
        let doc: serde_json::Value =
            serde_json::from_reader(std::io::BufReader::new(std::fs::File::open(path)?))
                .map_err(std::io::Error::other)?;
        Self::parse(&doc)
    }

    /// Parse the golden's document.
    ///
    /// # Errors
    /// If `table` is absent or empty.
    pub fn parse(doc: &serde_json::Value) -> std::io::Result<Self> {
        let rows = doc
            .get("table")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| std::io::Error::other("jukebox_songs.json has no table"))?;
        let mut by_id = BTreeMap::new();
        for row in rows {
            let Some(id) = row.get("song").and_then(serde_json::Value::as_str) else {
                continue;
            };
            let num = |k: &str| {
                row.get(k)
                    .and_then(serde_json::Value::as_i64)
                    .and_then(|v| i32::try_from(v).ok())
            };
            let (Some(rank), Some(ticks), Some(out)) = (
                num("alphabeticalRank"),
                num("lengthInTicks"),
                num("comparatorOutput"),
            ) else {
                continue;
            };
            by_id.insert(
                id.to_string(),
                Song {
                    id: id.to_string(),
                    alphabetical_rank: rank,
                    length_in_ticks: ticks,
                    comparator_output: out,
                },
            );
        }
        if by_id.is_empty() {
            return Err(std::io::Error::other(
                "jukebox_songs.json carries no songs — every disc would play silence",
            ));
        }
        Ok(Self { by_id })
    }

    /// The one table this process serves, loaded once.
    ///
    /// # Errors
    /// Whatever [`Self::load`] returned the first time, repeated.
    pub fn shared(path: &Path) -> Result<&'static Self, String> {
        static TABLE: std::sync::OnceLock<Result<JukeboxSongs, String>> =
            std::sync::OnceLock::new();
        TABLE
            .get_or_init(|| Self::load(path).map_err(|e| e.to_string()))
            .as_ref()
            .map_err(Clone::clone)
    }

    /// `JukeboxSong.fromStack(ItemStack)` — offsets 0-26.
    ///
    /// Vanilla reads the `JUKEBOX_PLAYABLE` data component, which this server
    /// does not carry. The convention every vanilla disc follows is that
    /// `minecraft:music_disc_<name>` is playable and names `minecraft:<name>`,
    /// so that is the mapping used, **and it is checked against the table**:
    /// an item whose derived name is not a song in the extract answers `None`
    /// rather than inventing one.
    ///
    /// `minecraft:music_disc_creator_music_box` therefore finds
    /// `minecraft:creator_music_box`, which is the case a shorter rule that
    /// stopped at the first underscore would get wrong.
    #[must_use]
    pub fn from_stack(&self, stack: &ItemStack) -> Option<&Song> {
        let name = stack.item().strip_prefix("minecraft:music_disc_")?;
        self.by_id.get(&format!("minecraft:{name}"))
    }

    /// A song by its registry id.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Song> {
        self.by_id.get(id)
    }

    /// **The `level_event(1010, …)` data field — one point of it recorded.**
    ///
    /// Vanilla sends `Registry.getId(song)` for the JUKEBOX_SONG registry,
    /// which is a datapack registry whose ids are its load order. This
    /// returns the **alphabetical rank**, on the reasoning that
    /// `RegistryDataLoader` walks `ResourceManager.listResources`, which
    /// hands back a map sorted by `Identifier`.
    ///
    /// Checked against a vanilla recording on 2026-09-24: check 798 puts
    /// `music_disc_cat` in a jukebox and vanilla sends `data = 5`, which is
    /// `minecraft:cat`'s rank among the jar's 22 `data/minecraft/jukebox_song`
    /// files (`11`, `13`, `5`, `blocks`, `bounce`, `cat`, …). That is one song,
    /// not the registry: a disc whose rank differed from its load order would
    /// still be a guess here. The data generator's
    /// `VanillaRegistries.createLookup()` bootstrap order gave three different
    /// orders over three JVM runs and is not the order a server loads
    /// (`ci/jukebox-song-golden.sh`).
    #[must_use]
    pub fn song_id(&self, song: &Song) -> i32 {
        song.alphabetical_rank
    }

    /// How many songs the table holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// Whether the table is empty; always false for one [`Self::parse`] took.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}

/// `JukeboxSongPlayer`'s two fields.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SongPlayer {
    /// The song playing, or `None`. `isPlaying()` is `song != null`.
    pub song: Option<String>,
    /// `ticksSinceSongStarted`.
    pub ticks: i64,
}

/// What a jukebox emitted this tick or this interaction.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JukeboxEffects {
    /// `level_event(id, pos, data)` calls, in order.
    pub level_events: Vec<(i32, i32)>,
    /// `HAS_RECORD` was written with `setBlock(..., 2)`. `None` when it was
    /// not touched.
    pub has_record: Option<bool>,
    /// `GameEvent.JUKEBOX_PLAY` fired this tick.
    pub playing_event: bool,
    /// `GameEvent.JUKEBOX_STOP_PLAY` fired.
    pub stop_event: bool,
}

impl SongPlayer {
    /// `JukeboxSongPlayer.play(LevelAccessor, Holder<JukeboxSong>)` — offsets
    /// 0-66.
    ///
    /// Sets the song, **zeroes the tick counter**, and sends
    /// `level_event(1010, pos, songId)`. There is no `JUKEBOX_PLAY` game
    /// event here; the first one comes from the next `tick`, whose counter is
    /// 0 and so passes the `% 20` test.
    pub fn play(&mut self, song: &Song, song_id: i32, out: &mut JukeboxEffects) {
        self.song = Some(song.id.clone());
        self.ticks = 0;
        out.level_events.push((PLAY_LEVEL_EVENT, song_id));
    }

    /// `JukeboxSongPlayer.stop(LevelAccessor, BlockState)` — offsets 0-58.
    ///
    /// **Returns immediately when nothing is playing** (offsets 0-7), so
    /// stopping a silent jukebox sends no `level_event(1011)`. That guard is
    /// what stops a jukebox emitting a stop packet every time an empty one is
    /// clicked.
    pub fn stop(&mut self, out: &mut JukeboxEffects) {
        if self.song.is_none() {
            return;
        }
        self.song = None;
        self.ticks = 0;
        out.stop_event = true;
        out.level_events.push((STOP_LEVEL_EVENT, 0));
    }

    /// `JukeboxSongPlayer.tick(LevelAccessor, BlockState)` — offsets 0-79.
    ///
    /// The finish test runs **before** the counter is incremented, so a song
    /// of `n` ticks stops on the tick where the counter has already reached
    /// `n + 20`.
    pub fn tick(&mut self, songs: &JukeboxSongs, out: &mut JukeboxEffects) {
        let Some(id) = self.song.clone() else {
            return;
        };
        let finished = songs
            .get(&id)
            .is_some_and(|s| self.ticks >= i64::from(s.length_in_ticks) + FINISHED_TAIL_TICKS);
        if finished {
            self.stop(out);
            return;
        }
        if self.ticks % PLAYING_EVENT_PERIOD == 0 {
            out.playing_event = true;
        }
        self.ticks += 1;
    }
}

/// `JukeboxBlockEntity.setTheItem(ItemStack)` — offsets 0-81.
///
/// The order is: write the item, compute `hasRecord`, look the song up,
/// **`notifyItemChangedInJukebox(hasRecord)` first**, and only then start or
/// stop the player. So the block state changes before the `level_event` goes
/// out, and a client sees the block turn on and then hear the disc.
///
/// A disc with no song — `hasRecord` true, `fromStack` empty — takes the
/// **stop** branch at offset 66, not the play one. That is how an item that
/// is not a record but was somehow inserted turns the block on and plays
/// nothing.
///
/// `state_matches` is `notifyItemChangedInJukebox`'s own guard: `level
/// .getBlockState(pos) == getBlockState()`. When it is false the state write
/// and its game event are skipped, and the player is still started.
pub fn set_the_item(
    player: &mut SongPlayer,
    slot: &mut ItemStack,
    stack: ItemStack,
    songs: &JukeboxSongs,
    state_matches: bool,
) -> JukeboxEffects {
    let mut out = JukeboxEffects::default();
    *slot = stack;
    let has_record = !slot.is_empty();
    let song = songs.from_stack(slot).cloned();
    if state_matches {
        out.has_record = Some(has_record);
    }
    if has_record {
        if let Some(song) = song {
            let id = songs.song_id(&song);
            player.play(&song, id, &mut out);
            return out;
        }
    }
    player.stop(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn songs() -> JukeboxSongs {
        JukeboxSongs::parse(&serde_json::json!({
            "table": [
                {"song": "minecraft:11", "alphabeticalRank": 0,
                 "lengthInTicks": 1420, "comparatorOutput": 11},
                {"song": "minecraft:creator_music_box", "alphabeticalRank": 1,
                 "lengthInTicks": 1460, "comparatorOutput": 11},
                {"song": "minecraft:pigstep", "alphabeticalRank": 2,
                 "lengthInTicks": 2980, "comparatorOutput": 13}
            ]
        }))
        .expect("songs parse")
    }

    /// The item-to-song rule keeps the whole suffix. A rule that stopped at
    /// the first underscore would find `minecraft:creator` — which is a real
    /// song in vanilla — and play the wrong disc, silently.
    #[test]
    fn a_multi_word_disc_maps_to_its_whole_song_name() {
        let s = songs();
        let disc = ItemStack::one("minecraft:music_disc_creator_music_box");
        assert_eq!(
            s.from_stack(&disc).map(|x| x.id.as_str()),
            Some("minecraft:creator_music_box")
        );
        // And an item that is not a disc at all finds nothing.
        assert!(s.from_stack(&ItemStack::one("minecraft:stone")).is_none());
        // As does a disc naming a song the table does not hold, rather than
        // an invented row.
        assert!(s
            .from_stack(&ItemStack::one("minecraft:music_disc_nonesuch"))
            .is_none());
    }

    /// Putting a disc in turns `HAS_RECORD` on and sends 1010 with the
    /// song's id, in that order.
    #[test]
    fn inserting_a_disc_sets_the_state_and_sends_the_play_event() {
        let s = songs();
        let mut p = SongPlayer::default();
        let mut slot = ItemStack::empty();
        let out = set_the_item(
            &mut p,
            &mut slot,
            ItemStack::one("minecraft:music_disc_pigstep"),
            &s,
            true,
        );
        assert_eq!(out.has_record, Some(true));
        assert_eq!(out.level_events, vec![(1010, 2)]);
        assert_eq!(p.song.as_deref(), Some("minecraft:pigstep"));
        assert_eq!(p.ticks, 0);
    }

    /// Taking the disc out turns the state off and sends 1011 with data 0.
    #[test]
    fn removing_the_disc_sends_the_stop_event() {
        let s = songs();
        let mut p = SongPlayer::default();
        let mut slot = ItemStack::empty();
        set_the_item(
            &mut p,
            &mut slot,
            ItemStack::one("minecraft:music_disc_pigstep"),
            &s,
            true,
        );
        let out = set_the_item(&mut p, &mut slot, ItemStack::empty(), &s, true);
        assert_eq!(out.has_record, Some(false));
        assert_eq!(out.level_events, vec![(1011, 0)]);
        assert!(out.stop_event);
        assert!(p.song.is_none());
    }

    /// Emptying an **already empty** jukebox sends no stop event: `stop`
    /// returns at offset 7 when nothing is playing. The state write still
    /// happens, so this is a test of the player's guard and not of the
    /// caller.
    #[test]
    fn stopping_a_silent_jukebox_sends_nothing() {
        let s = songs();
        let mut p = SongPlayer::default();
        let mut slot = ItemStack::empty();
        let out = set_the_item(&mut p, &mut slot, ItemStack::empty(), &s, true);
        assert_eq!(out.has_record, Some(false));
        assert!(out.level_events.is_empty());
        assert!(!out.stop_event);
    }

    /// A non-disc item still turns the block on and takes the **stop**
    /// branch, so nothing plays.
    #[test]
    fn a_non_disc_item_turns_the_block_on_and_plays_nothing() {
        let s = songs();
        let mut p = SongPlayer::default();
        let mut slot = ItemStack::empty();
        let out = set_the_item(
            &mut p,
            &mut slot,
            ItemStack::one("minecraft:stone"),
            &s,
            true,
        );
        assert_eq!(out.has_record, Some(true));
        assert!(out.level_events.is_empty());
        assert!(p.song.is_none());
    }

    /// A jukebox whose block was replaced under it writes no state, and the
    /// player still starts. The guard is `notifyItemChangedInJukebox`'s, not
    /// `setTheItem`'s.
    #[test]
    fn a_stale_block_state_skips_the_state_write_but_not_the_song() {
        let s = songs();
        let mut p = SongPlayer::default();
        let mut slot = ItemStack::empty();
        let out = set_the_item(
            &mut p,
            &mut slot,
            ItemStack::one("minecraft:music_disc_11"),
            &s,
            false,
        );
        assert_eq!(out.has_record, None);
        assert_eq!(out.level_events, vec![(1010, 0)]);
    }

    /// The song stops at `lengthInTicks + 20` and not at `lengthInTicks`.
    /// Dropping the twenty is the plausible error and this is the pair of
    /// asserts that catches it.
    #[test]
    fn a_song_runs_twenty_ticks_past_its_own_length() {
        let s = songs();
        let mut p = SongPlayer {
            song: Some("minecraft:11".to_string()),
            ticks: 1420,
        };
        let mut out = JukeboxEffects::default();
        p.tick(&s, &mut out);
        assert!(p.song.is_some(), "1420 of 1420 is not finished");
        p.ticks = 1439;
        p.tick(&s, &mut out);
        assert!(p.song.is_some(), "1439 is not finished either");
        p.ticks = 1440;
        let mut out = JukeboxEffects::default();
        p.tick(&s, &mut out);
        assert!(p.song.is_none());
        assert_eq!(out.level_events, vec![(1011, 0)]);
    }

    /// The playing event fires on the tick the counter is 0 — the first tick
    /// after `play` — because the increment comes after the test.
    #[test]
    fn the_playing_event_fires_on_the_first_tick_and_every_twentieth() {
        let s = songs();
        let mut p = SongPlayer {
            song: Some("minecraft:pigstep".to_string()),
            ticks: 0,
        };
        let mut fired = Vec::new();
        for _ in 0..41 {
            let mut out = JukeboxEffects::default();
            p.tick(&s, &mut out);
            fired.push(out.playing_event);
        }
        assert!(fired[0]);
        assert!(!fired[1]);
        assert!(fired[20]);
        assert!(fired[40]);
        assert_eq!(fired.iter().filter(|x| **x).count(), 3);
    }

    /// An empty table is refused rather than served silently.
    #[test]
    fn a_table_with_no_songs_is_refused() {
        assert!(JukeboxSongs::parse(&serde_json::json!({"table": []})).is_err());
        assert!(JukeboxSongs::parse(&serde_json::json!({})).is_err());
    }
}
