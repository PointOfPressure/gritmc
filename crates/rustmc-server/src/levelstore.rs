//! The world **directory**: `level.dat`, `session.lock` and the nine
//! `data/minecraft/*.dat` files that 26.2 split out of `level.dat`.
//!
//! # What this closes
//!
//! Before this, `--world <dir>` wrote exactly one thing: region files, under
//! `dimensions/minecraft/overworld/region`. That is a directory a vanilla
//! server cannot open — `LevelStorageSource` looks for `level.dat`, finds
//! nothing, and treats the directory as a world that does not exist. So the
//! claim "what lands on disk is a world vanilla can open" was true of the
//! *region file* and not of the *world*, and the difference is one an
//! operator meets the first time they try to move a build into a vanilla
//! server.
//!
//! Every shape written here is byte-for-byte against real vanilla output;
//! `rustmc_world::level` holds the format and its goldens. This module holds
//! only the wiring: which value goes in which field, and when the files are
//! written.
//!
//! # What is honestly ours to write and what is a placeholder
//!
//! Stated rather than left to be found, because a `level.dat` full of
//! plausible-looking defaults is exactly the sort of file that reads as
//! authoritative and is not:
//!
//! * **the seed is real** — it is the seed the tiles were generated at;
//! * **`Time` is real** — the server's own tick counter;
//! * **`GameType` is real** — `crate::status::game_type`;
//! * **the game rules are real** — `crate::gamerules`, which carries all 59
//!   with vanilla's own defaults and whatever the console has changed;
//! * **the spawn is a placeholder.** This server has no spawn-point search;
//!   it puts a player at a fixed position. `initialized` is therefore written
//!   **false**, which is not cosmetic: vanilla re-picks the spawn point on
//!   loading a level whose `initialized` is clear, so a world moved to
//!   vanilla gets a real spawn instead of ours. Writing `true` beside a
//!   made-up `spawn.pos` would be the lie.
//! * **the weather is a placeholder** — no weather is simulated, so a fresh
//!   world's values are written every time.

use rustmc_world::level::{self, saved, LevelData, SavedDataFile, Spawn};
use rustmc_world::nbt::Nbt;
use std::io;
use std::path::{Path, PathBuf};

/// Game rules the 26.2 registry carries but `game_rules.dat` does **not**.
///
/// One entry, and it is measured rather than reasoned: every harvested world
/// on this box holds 58 rules in `game_rules.dat` while
/// [`crate::gamerules::DEFAULTS`] holds 59, and the difference is exactly
/// this name. Each of those worlds carries `minecart_improvements` in its
/// `DataPacks.Disabled`, so the rule is gated behind a feature pack that is
/// off, and vanilla writes no entry for it at all.
///
/// The check that keeps this honest is
/// `rustmc_world::level::saved_goldens::the_saved_game_rules_are_the_registry_minus_the_gated_one`,
/// which asserts the count and the absent name against the files themselves —
/// so enabling the pack (which this server cannot yet do) shows up as a red
/// rather than as a silently 58-rule file.
const RULES_GATED_OFF: &[&str] = &["max_minecart_speed"];

/// The world directory, and everything in it that is not a region file.
#[derive(Debug)]
pub struct LevelStore {
    root: PathBuf,
    /// Held for the life of the process. Dropping it releases the advisory
    /// lock, which is why it is a field and not a local — see
    /// [`Self::open`].
    _lock: Option<std::fs::File>,
}

impl LevelStore {
    /// Take the session lock and make the directory skeleton.
    ///
    /// `DirectoryLock.create` writes three bytes — `E2 98 83`, U+2603
    /// SNOWMAN — into `session.lock`, forces them to disk, and then takes an
    /// exclusive `FileChannel.tryLock` on the channel, throwing
    /// `LockException.alreadyLocked` if it cannot. Read off the bytecode and
    /// confirmed against a real `session.lock`, which is those three bytes and
    /// nothing else.
    ///
    /// The lock is what stops two servers writing one world, and it is the
    /// same advisory `flock` vanilla takes, so **vanilla and this server
    /// exclude each other** rather than each holding a lock the other cannot
    /// see. That is the whole point of reproducing it rather than inventing
    /// one.
    ///
    /// # Errors
    /// If the directory cannot be created, or another process holds the lock.
    pub fn open(root: PathBuf) -> io::Result<Self> {
        std::fs::create_dir_all(&root)?;
        let path = root.join("session.lock");
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&path)?;
        {
            use std::io::Write;
            file.write_all("\u{2603}".as_bytes())?;
            file.sync_all()?;
        }
        file.try_lock().map_err(|_| {
            io::Error::new(
                io::ErrorKind::WouldBlock,
                format!(
                    "{} is locked by another server — refusing to write a second \
                     world into it",
                    root.display()
                ),
            )
        })?;
        let store = Self {
            root,
            _lock: Some(file),
        };
        for dir in [
            store.data_dir(),
            store.player_data_dir(),
            store.root.join("datapacks"),
            store.dimension_dir("minecraft:overworld").join("region"),
            store.dimension_dir("minecraft:overworld").join("entities"),
        ] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(store)
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `data/minecraft` — `SavedDataStorage` writes `<id>.dat` here, one
    /// directory per namespace.
    #[must_use]
    pub fn data_dir(&self) -> PathBuf {
        self.root.join("data/minecraft")
    }

    /// `players/data` — `LevelResource.PLAYER_DATA_DIR`, and **not**
    /// `playerdata`, which is what every pre-26 world used and what a reader
    /// working from memory writes.
    #[must_use]
    pub fn player_data_dir(&self) -> PathBuf {
        self.root.join("players/data")
    }

    /// `dimensions/<namespace>/<path>` for a dimension id.
    #[must_use]
    pub fn dimension_dir(&self, dimension: &str) -> PathBuf {
        let (namespace, path) = dimension
            .split_once(':')
            .unwrap_or(("minecraft", dimension));
        self.root.join("dimensions").join(namespace).join(path)
    }

    /// Write `level.dat` and every `data/minecraft/*.dat`.
    ///
    /// Called on the same triggers the region files are: the autosave timer
    /// and the shutdown save. Cheap — the nine files total under 2 KiB — so
    /// it is not conditional on anything having changed.
    ///
    /// # Errors
    /// If any file cannot be written.
    pub fn save(&self, state: &WorldState) -> io::Result<()> {
        let mut data = LevelData::new(&self.level_name());
        data.time = state.ticks;
        data.game_type = i32::from(state.game_type);
        data.last_played = state.epoch_millis;
        data.server_brands = vec![state.brand.clone()];
        data.spawn = Spawn {
            x: state.spawn.0,
            y: state.spawn.1,
            z: state.spawn.2,
            ..Spawn::default()
        };
        // **False on purpose.** See the module doc: this server picks no
        // spawn, so a world it wrote must let vanilla pick one.
        data.initialized = false;
        level::write_dat_safely(&self.root.join("level.dat"), "", &data.to_file_nbt())?;

        let dir = self.data_dir();
        let write = |name: &str, payload: Nbt| -> io::Result<()> {
            level::write_dat_safely(
                &dir.join(name),
                "",
                &SavedDataFile::new(payload).to_file_nbt(),
            )
        };
        write(
            "world_gen_settings.dat",
            saved::world_gen_settings(state.seed, true, false),
        )?;
        write("game_rules.dat", saved::game_rules(&saved_game_rules()))?;
        // The overworld and the End share this server's one clock; there is
        // no per-dimension time because there is one dimension.
        write(
            "world_clocks.dat",
            saved::world_clocks(state.ticks, state.ticks),
        )?;
        write(
            "weather.dat",
            saved::weather(false, 0, false, 0, CLEAR_WEATHER_TIME),
        )?;
        // The loot tables' random sequences, as far as this session drew
        // them (`crate::lootroll`). Empty on a world nothing has rolled in.
        write(
            "random_sequences.dat",
            saved::random_sequences(0, crate::lootroll::sequences_nbt()),
        )?;
        write("scheduled_events.dat", saved::scheduled_events())?;
        write("stopwatches.dat", saved::stopwatches())?;
        write("scoreboard.dat", saved::empty())?;
        write("custom_boss_events.dat", saved::empty())?;
        Ok(())
    }

    /// Read `level.dat` back, if there is one.
    ///
    /// # Errors
    /// If the file exists and cannot be parsed. A missing file is `Ok(None)`
    /// — that is a first boot.
    pub fn load(&self) -> io::Result<Option<LevelData>> {
        let path = self.root.join("level.dat");
        if !path.exists() {
            return Ok(None);
        }
        LevelData::from_file_nbt(&level::read_dat(&path)?).map(Some)
    }

    /// The seed a previous session recorded, if this directory has one.
    ///
    /// **This is the one saved value that can make a world wrong rather than
    /// merely stale.** The overlay is a diff against the generated canvas
    /// (`World::load_from`), so a world reopened under a different seed keeps
    /// the player's build and regenerates *everything else* differently — the
    /// build then floats in unrelated terrain. Reading the seed back is what
    /// lets a caller refuse instead.
    ///
    /// # Errors
    /// If `world_gen_settings.dat` exists and cannot be parsed.
    pub fn saved_seed(&self) -> io::Result<Option<i64>> {
        let path = self.data_dir().join("world_gen_settings.dat");
        if !path.exists() {
            return Ok(None);
        }
        let root = level::read_dat(&path)?;
        Ok(match root.get("data").and_then(|d| d.get("seed")) {
            Some(Nbt::Long(v)) => Some(*v),
            _ => None,
        })
    }

    /// `random_sequences.dat`'s `data` compound, if this directory has one.
    ///
    /// # Errors
    /// If the file exists and cannot be parsed.
    pub fn saved_random_sequences(&self) -> io::Result<Option<Nbt>> {
        let path = self.data_dir().join("random_sequences.dat");
        if !path.exists() {
            return Ok(None);
        }
        Ok(level::read_dat(&path)?.get("data").cloned())
    }

    /// `LevelName` — the directory's own name, which is what vanilla shows in
    /// the world list.
    fn level_name(&self) -> String {
        self.root
            .file_name()
            .map_or_else(|| "world".to_string(), |n| n.to_string_lossy().into_owned())
    }
}

/// `ServerLevelData`'s initial `clearWeatherTime`. Not 0: a fresh world is
/// clear for a very long time, and 0 would make a reloaded world start
/// rolling for rain on its first tick.
const CLEAR_WEATHER_TIME: i32 = 1_000_000;

/// Everything `level.dat` needs that lives outside this module.
///
/// A struct rather than eight arguments because every one of them is a number
/// and three of them are counts — a transposed pair would be silent.
#[derive(Debug, Clone)]
pub struct WorldState {
    /// The seed the world was generated at.
    pub seed: i64,
    /// `Time` — total ticks run.
    pub ticks: i64,
    /// `GameType`, from `crate::status::game_type`.
    pub game_type: u8,
    /// `LastPlayed`, epoch milliseconds.
    pub epoch_millis: i64,
    /// The spawn position this server puts a player at.
    pub spawn: (i32, i32, i32),
    /// `ServerBrands` — ours, so a world that has been through this server
    /// says so rather than claiming to be vanilla's.
    pub brand: String,
}

/// The game rules as `game_rules.dat` holds them: namespaced ids, `Byte` for
/// a boolean and `Int` for a number, and **without** the ones gated behind a
/// disabled feature pack.
fn saved_game_rules() -> Vec<(String, Nbt)> {
    let rules = crate::gamerules::GameRules::shared();
    crate::gamerules::DEFAULTS
        .iter()
        .filter(|(name, _)| !RULES_GATED_OFF.contains(name))
        .map(|(name, default)| {
            let value = rules.get(name).unwrap_or(*default);
            let tag = match value {
                crate::gamerules::GameRuleValue::Bool(b) => Nbt::Byte(i8::from(b)),
                crate::gamerules::GameRuleValue::Int(i) => Nbt::Int(i),
            };
            (format!("minecraft:{name}"), tag)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rustmc-levelstore-{}-{name}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn state() -> WorldState {
        WorldState {
            seed: 20_260_722,
            ticks: 1,
            game_type: 0,
            epoch_millis: 1_785_166_227_074,
            spawn: (0, 64, 0),
            brand: "rustmc".into(),
        }
    }

    /// The directory a vanilla server looks for, made and then read back.
    #[test]
    fn a_saved_world_has_the_files_vanilla_opens_a_world_by() {
        let dir = scratch("layout");
        let store = LevelStore::open(dir.clone()).expect("open");
        store.save(&state()).expect("save");
        for relative in [
            "level.dat",
            "session.lock",
            "data/minecraft/game_rules.dat",
            "data/minecraft/weather.dat",
            "data/minecraft/world_clocks.dat",
            "data/minecraft/world_gen_settings.dat",
            "data/minecraft/random_sequences.dat",
            "data/minecraft/scheduled_events.dat",
            "data/minecraft/stopwatches.dat",
            "data/minecraft/scoreboard.dat",
            "data/minecraft/custom_boss_events.dat",
            "players/data",
            "dimensions/minecraft/overworld/region",
        ] {
            assert!(
                dir.join(relative).exists(),
                "{relative} is missing from the world directory"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `session.lock` is three bytes and they are the ones vanilla writes.
    /// A different byte string is a file vanilla still locks — the content is
    /// not what excludes anyone — so this is pinned by value rather than by
    /// length.
    #[test]
    fn the_session_lock_is_the_snowman() {
        let dir = scratch("lock");
        let store = LevelStore::open(dir.clone()).expect("open");
        let bytes = std::fs::read(dir.join("session.lock")).expect("read");
        assert_eq!(bytes, vec![0xE2, 0x98, 0x83], "U+2603 SNOWMAN, UTF-8");
        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A second opener is refused while the first holds the lock, and
    /// admitted once it is dropped. **The second half is the half that makes
    /// this a check**: without it, an `open` that always failed would pass.
    #[test]
    fn a_second_server_cannot_open_a_locked_world() {
        let dir = scratch("exclusion");
        let first = LevelStore::open(dir.clone()).expect("first open");
        let err = LevelStore::open(dir.clone()).expect_err("second must be refused");
        assert_eq!(err.kind(), io::ErrorKind::WouldBlock, "{err}");
        drop(first);
        LevelStore::open(dir.clone()).expect("open after the lock is released");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The seed round-trips through `world_gen_settings.dat`, which is where
    /// 26.2 keeps it — not `level.dat`.
    #[test]
    fn the_seed_round_trips_through_world_gen_settings() {
        let dir = scratch("seed");
        let store = LevelStore::open(dir.clone()).expect("open");
        assert_eq!(store.saved_seed().expect("no file yet"), None);
        store.save(&state()).expect("save");
        assert_eq!(store.saved_seed().expect("read"), Some(20_260_722));
        let level = store.load().expect("read level.dat").expect("present");
        assert_eq!(level.time, 1);
        assert_eq!(level.game_type, 0);
        assert_eq!(level.server_brands, vec!["rustmc".to_string()]);
        assert!(
            !level.initialized,
            "this server picks no spawn, so vanilla must pick one"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 58 rules, not 59, and the missing one is named.
    #[test]
    fn the_written_game_rules_match_the_count_vanilla_writes() {
        let rules = saved_game_rules();
        assert_eq!(
            rules.len(),
            crate::gamerules::DEFAULTS.len() - RULES_GATED_OFF.len()
        );
        assert_eq!(rules.len(), 58, "a 26.2 game_rules.dat holds 58 rules");
        assert!(
            !rules
                .iter()
                .any(|(id, _)| id == "minecraft:max_minecart_speed"),
            "gated behind the disabled minecart_improvements pack"
        );
        assert!(rules.iter().any(|(id, _)| id == "minecraft:spawn_mobs"));
    }

    /// A second save leaves `level.dat_old` behind and no `_new`.
    #[test]
    fn a_second_save_rotates_the_previous_level_dat() {
        let dir = scratch("rotate");
        let store = LevelStore::open(dir.clone()).expect("open");
        store.save(&state()).expect("first");
        let mut later = state();
        later.ticks = 6000;
        store.save(&later).expect("second");
        assert!(dir.join("level.dat_old").exists());
        assert!(!dir.join("level.dat_new").exists());
        assert_eq!(store.load().expect("read").expect("present").time, 6000);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

// ---------------------------------------------------------------------------
// The process-wide store, and the player files
// ---------------------------------------------------------------------------

/// The server's one [`LevelStore`], or `None` when `--world` was not passed.
///
/// A singleton for the same reason `crate::tiles`'s world directory is one:
/// the autosave thread, the join path and the disconnect path all need it and
/// none of them can be handed a reference by the others. `None` inside the
/// `OnceLock` is the meaningful state — "persistence was considered and is
/// off" — as distinct from "nobody has asked yet".
static SHARED: std::sync::OnceLock<Option<LevelStore>> = std::sync::OnceLock::new();

/// Game ticks, published by the tick loop so the autosave thread can write
/// `level.dat`'s `Time` without holding the clock.
static TICKS: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

/// Open the world directory and take its session lock. Call once, from `main`,
/// before the first tile is generated.
///
/// # Errors
/// If the directory cannot be created or another server holds its lock. That
/// is a **refusal to start**, not a warning: two servers writing one world is
/// the corruption `session.lock` exists to prevent, and carrying on with
/// persistence quietly disabled would lose the operator's world without
/// saying so.
pub fn open_shared(root: PathBuf) -> io::Result<()> {
    let store = LevelStore::open(root)?;
    let _ = SHARED.set(Some(store));
    Ok(())
}

/// The store, if persistence is on.
#[must_use]
pub fn shared() -> Option<&'static LevelStore> {
    SHARED.get().and_then(Option::as_ref)
}

/// Publish the current game tick. One `Relaxed` store per tick.
pub fn record_tick(ticks: i64) {
    TICKS.store(ticks, std::sync::atomic::Ordering::Relaxed);
}

/// The last tick the loop published.
#[must_use]
pub fn ticks() -> i64 {
    TICKS.load(std::sync::atomic::Ordering::Relaxed)
}

/// Epoch milliseconds, for `LastPlayed`. Zero if the clock is before the
/// epoch, which is not a case worth an error.
#[must_use]
pub fn epoch_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// What this server knows about a player when they disconnect.
///
/// **Position, rotation, the ground flag and experience, and nothing else —
/// that is the whole of it, and it is stated rather than left to be
/// discovered.** This server has no per-player inventory (nothing in
/// `crate::containers` is keyed by player), no health and no hunger, so
/// there is nothing to write into `Inventory`, `Health` or `foodLevel`
/// beyond the defaults. Experience (`XpLevel`, `XpP`, `XpTotal`, `Score`)
/// comes from `crate::experience`. A saved file therefore restores where a player was
/// standing and nothing about what they were carrying, because there is
/// nothing to carry.
#[derive(Debug, Clone, Copy)]
pub struct PlayerSnapshot {
    pub uuid: u128,
    pub pos: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
    pub game_type: i32,
    /// `(XpLevel, XpP, XpTotal, Score)`, when `crate::experience` holds the
    /// player. `None` leaves whatever the file had.
    pub xp: Option<(i32, f32, i32, i32)>,
}

impl LevelStore {
    /// Write `players/data/<uuid>.dat`.
    ///
    /// **Merges onto the existing file rather than replacing it.** A player
    /// who has played on vanilla and then here has 48 keys in that file and
    /// this server models 24; writing a fresh compound would delete their
    /// recipe book, their advancements-adjacent state and their ender chest.
    /// `PlayerData::extra` carries the difference, so this reads first,
    /// overwrites the fields it owns, and writes the whole thing back.
    ///
    /// # Errors
    /// If the file cannot be read, written, or is not parseable NBT.
    pub fn save_player(&self, snapshot: PlayerSnapshot) -> io::Result<()> {
        let dir = self.player_data_dir();
        let mut player = rustmc_world::playerdat::read(&dir, snapshot.uuid)?.unwrap_or_else(|| {
            rustmc_world::playerdat::PlayerData::fresh(
                snapshot.uuid,
                snapshot.pos,
                snapshot.game_type,
                level::DATA_VERSION,
            )
        });
        player.pos = snapshot.pos;
        player.yaw = snapshot.yaw;
        player.pitch = snapshot.pitch;
        player.on_ground = snapshot.on_ground;
        player.game_type = snapshot.game_type;
        if let Some((level, progress, total, score)) = snapshot.xp {
            player.xp_level = level;
            player.xp_progress = progress;
            player.xp_total = total;
            player.score = score;
        }
        player.data_version = level::DATA_VERSION;
        rustmc_world::playerdat::write(&dir, &player)
    }

    /// `PlayerList.saveAll()` — every player currently in the level, and
    /// how many were written.
    ///
    /// # Why this reads the entity list and not the connection list
    ///
    /// A [`PlayerSnapshot`] is a position, and the position lives on the
    /// entity. `crate::players::Connection` carries an `entity_id` and no
    /// coordinates, so a sweep over connections would have to look every one
    /// of them up here anyway — and would additionally have to decide what to
    /// do about a connection whose entity is not in the list, which is a real
    /// state this server passes through on every join (between
    /// `players.join` and the `entities.insert` a few lines later in
    /// `crate::status`). Reading the entity list instead makes that window
    /// simply invisible: a player with no entity has no position to save, so
    /// there is nothing to decide.
    ///
    /// It also means this is exactly what `crate::status`'s disconnect arm
    /// saves, through the same struct — vanilla's `PlayerList.saveAll` and
    /// `PlayerList.remove`'s `save(player)` are likewise one method called
    /// from two places.
    ///
    /// # It does not stop at the first failure
    ///
    /// One unwritable player file must not cost the other nineteen theirs.
    /// The error is printed with the uuid and the sweep continues, and the
    /// count returned is of players actually written — so a caller that logs
    /// it can see the shortfall.
    ///
    /// # Errors
    /// Never. The signature is `io::Result` for symmetry with
    /// [`Self::save_player`] and so that a future failure mode that IS fatal
    /// — the player directory being unmakeable, say — has somewhere to go
    /// without changing every caller.
    pub fn save_all_players(&self, entities: &crate::entities::Entities) -> io::Result<usize> {
        let game_type = i32::from(crate::status::game_type());
        let mut written = 0;
        for entity in entities.all() {
            if entity.kind.name != crate::entities::PLAYER.name {
                continue;
            }
            let snapshot = PlayerSnapshot {
                uuid: entity.uuid,
                pos: [entity.x, entity.y, entity.z],
                yaw: entity.y_rot,
                pitch: entity.x_rot,
                on_ground: entity.on_ground,
                game_type,
                xp: crate::experience::save_fields(entity.id),
            };
            match self.save_player(snapshot) {
                Ok(()) => written += 1,
                Err(e) => eprintln!("could not save player {:032x}: {e}", entity.uuid),
            }
        }
        Ok(written)
    }

    /// Read a player back, if they have played here before.
    ///
    /// # Errors
    /// If the file exists and cannot be parsed.
    pub fn load_player(
        &self,
        uuid: u128,
    ) -> io::Result<Option<rustmc_world::playerdat::PlayerData>> {
        rustmc_world::playerdat::read(&self.player_data_dir(), uuid)
    }
}
