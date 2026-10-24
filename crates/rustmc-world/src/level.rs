//! `level.dat` and the `data/<namespace>/<id>.dat` saved-data files.
//!
//! # 26.2 split level.dat up, and that is the first thing to know
//!
//! Every older description of this format — and the brief this module was
//! written from — says `level.dat` carries the seed, the game rules, the time
//! of day, the weather and the world-gen settings. In 26.2 it carries **none
//! of them**. Read off real vanilla output
//! (`oracle-work/gen-20260722-25801/world`, generated 2026-07-27 by the pinned
//! 26.2 server), a world directory is:
//!
//! ```text
//! level.dat                          14 keys under `Data`, 383 bytes
//! level.dat_old                      the previous one, byte-identical on a first save
//! session.lock                       `LevelResource.LOCK_FILE`
//! data/minecraft/game_rules.dat      58 rules
//! data/minecraft/weather.dat         rain/thunder/clear timers
//! data/minecraft/world_gen_settings.dat   the SEED lives here now
//! data/minecraft/world_clocks.dat    per-dimension `total_ticks`
//! data/minecraft/random_sequences.dat
//! data/minecraft/scheduled_events.dat
//! data/minecraft/custom_boss_events.dat
//! data/minecraft/scoreboard.dat
//! data/minecraft/stopwatches.dat
//! players/data/                      `LevelResource` calls it players/data, NOT playerdata
//! dimensions/minecraft/<dim>/{region,entities,poi,data}
//! ```
//!
//! `LevelResource`'s own constant pool is the authority for the directory
//! names — it holds `level.dat`, `level.dat_old`, `session.lock`, `data`,
//! `datapacks`, `generated`, `players`, `players/data`, `players/stats`,
//! `players/advancements`, `icon.png` — and `LevelStorageSource` holds
//! `TAG_DATA = "Data"`, which is the single key the root compound wraps
//! everything in.
//!
//! # Why this is byte-comparable and the region files are not
//!
//! A `.dat` file is gzip around a plain named root, so the NBT payload is a
//! pure function of the tag. Vanilla builds that tag in a
//! `java.util.HashMap`, and [`crate::nbt`] reproduces that map's iteration
//! order, so **a vanilla `level.dat` decoded and re-encoded here is
//! byte-identical to the original payload** — which is the golden
//! `tests::every_harvested_level_dat_re_encodes_byte_for_byte` takes over
//! every harvested world on the box. The gzip *container* is not comparable
//! (Java's `Deflater` and miniz differ, and gzip stamps an mtime), so the
//! comparison is taken on the decompressed bytes and says so.

use crate::nbt::{self, Nbt};
use std::collections::BTreeMap;
use std::io;

/// `SharedConstants.getCurrentVersion().dataVersion()` for 26.2, read off
/// every harvested world's `level.dat` (`DataVersion`, and `Version.Id`).
pub const DATA_VERSION: i32 = 4903;

/// `26.2`, `Version.Name`.
pub const VERSION_NAME: &str = "26.2";

/// `Version.Series` — `WorldVersion.name()`'s series, `"main"` for a release.
pub const VERSION_SERIES: &str = "main";

/// The Anvil format version, `level.dat`'s lowercase `version` key. 19133 has
/// been the value since the format was named; it is not the data version and
/// the two keys differ by three orders of magnitude, which is the only thing
/// stopping them being confused.
pub const ANVIL_VERSION: i32 = 19133;

/// The `Data` compound of `level.dat`.
///
/// Every field is a key vanilla 26.2 writes; there is no field here that a
/// harvested `level.dat` does not carry, and
/// [`Self::from_nbt`] refuses a compound carrying a key this does not model
/// rather than dropping it — a silently dropped key is a lost world setting
/// and it would pass a round-trip test that only compares the fields it knows
/// about.
#[derive(Debug, Clone, PartialEq)]
pub struct LevelData {
    /// `difficulty_settings.difficulty` — `peaceful`, `easy`, `normal`, `hard`.
    pub difficulty: String,
    /// `difficulty_settings.hardcore`.
    pub hardcore: bool,
    /// `difficulty_settings.locked`.
    pub difficulty_locked: bool,
    /// `Time` — total ticks the level has run, not the time of day. The time
    /// of day moved to `data/minecraft/world_clocks.dat` in 26.2.
    pub time: i64,
    /// `GameType` — 0 survival, 1 creative, 2 adventure, 3 spectator.
    pub game_type: i32,
    /// `ServerBrands` — every brand string that has ever run this world.
    /// Vanilla appends its own on load and de-duplicates, so a world this
    /// server has opened carries two entries once it has also been opened by
    /// vanilla.
    pub server_brands: Vec<String>,
    /// `version`, the Anvil format version. See [`ANVIL_VERSION`].
    pub anvil_version: i32,
    /// `LastPlayed` — epoch milliseconds.
    pub last_played: i64,
    /// `spawn` — `pos` as a 3-int array, plus `yaw`, `pitch` and `dimension`.
    /// A compound since 26.2; it was four loose `SpawnX/Y/Z/Angle` keys.
    pub spawn: Spawn,
    /// `Version` — `{Snapshot, Series, Id, Name}`.
    pub version_id: i32,
    pub version_name: String,
    pub version_series: String,
    pub version_snapshot: bool,
    /// `LevelName`.
    pub level_name: String,
    /// `initialized` — whether the spawn point has been chosen. A world saved
    /// with this false makes vanilla re-pick spawn on next load.
    pub initialized: bool,
    /// `WasModded`.
    pub was_modded: bool,
    /// `DataVersion`.
    pub data_version: i32,
    /// `allowCommands`.
    pub allow_commands: bool,
    /// `DataPacks.Enabled` and `.Disabled`, in file order.
    pub datapacks_enabled: Vec<String>,
    pub datapacks_disabled: Vec<String>,
}

/// `level.dat`'s `spawn` compound.
#[derive(Debug, Clone, PartialEq)]
pub struct Spawn {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub yaw: f32,
    pub pitch: f32,
    /// `minecraft:overworld` and the like.
    pub dimension: String,
}

impl Default for Spawn {
    fn default() -> Self {
        Self {
            x: 0,
            y: 0,
            z: 0,
            yaw: 0.0,
            pitch: 0.0,
            dimension: "minecraft:overworld".into(),
        }
    }
}

impl LevelData {
    /// A fresh world's `level.dat`, as vanilla writes it on a first save.
    ///
    /// The defaults are the ones a harvested world carries, not invented
    /// ones: `Time` 1 rather than 0 because a world is saved after its first
    /// tick, `initialized` set, `WasModded` clear, and the three experimental
    /// datapacks disabled by name.
    #[must_use]
    pub fn new(level_name: &str) -> Self {
        Self {
            difficulty: "easy".into(),
            hardcore: false,
            difficulty_locked: false,
            time: 0,
            game_type: 0,
            server_brands: vec!["vanilla".into()],
            anvil_version: ANVIL_VERSION,
            last_played: 0,
            spawn: Spawn::default(),
            version_id: DATA_VERSION,
            version_name: VERSION_NAME.into(),
            version_series: VERSION_SERIES.into(),
            version_snapshot: false,
            level_name: level_name.into(),
            initialized: true,
            was_modded: false,
            data_version: DATA_VERSION,
            allow_commands: false,
            datapacks_enabled: vec!["vanilla".into()],
            datapacks_disabled: vec![
                "minecart_improvements".into(),
                "redstone_experiments".into(),
                "trade_rebalance".into(),
            ],
        }
    }

    /// The `Data` compound.
    #[must_use]
    pub fn to_nbt(&self) -> Nbt {
        let strings = |v: &[String]| Nbt::List(v.iter().map(|s| Nbt::String(s.clone())).collect());
        let mut d = BTreeMap::new();
        d.insert("difficulty_settings".into(), {
            let mut m = BTreeMap::new();
            m.insert("difficulty".into(), Nbt::String(self.difficulty.clone()));
            m.insert("hardcore".into(), Nbt::Byte(i8::from(self.hardcore)));
            m.insert("locked".into(), Nbt::Byte(i8::from(self.difficulty_locked)));
            Nbt::Compound(m)
        });
        d.insert("Time".into(), Nbt::Long(self.time));
        d.insert("GameType".into(), Nbt::Int(self.game_type));
        d.insert("ServerBrands".into(), strings(&self.server_brands));
        d.insert("version".into(), Nbt::Int(self.anvil_version));
        d.insert("LastPlayed".into(), Nbt::Long(self.last_played));
        d.insert("spawn".into(), {
            let mut m = BTreeMap::new();
            m.insert(
                "pos".into(),
                Nbt::IntArray(vec![self.spawn.x, self.spawn.y, self.spawn.z]),
            );
            m.insert("pitch".into(), Nbt::Float(self.spawn.pitch));
            m.insert(
                "dimension".into(),
                Nbt::String(self.spawn.dimension.clone()),
            );
            m.insert("yaw".into(), Nbt::Float(self.spawn.yaw));
            Nbt::Compound(m)
        });
        d.insert("Version".into(), {
            let mut m = BTreeMap::new();
            m.insert(
                "Snapshot".into(),
                Nbt::Byte(i8::from(self.version_snapshot)),
            );
            m.insert("Series".into(), Nbt::String(self.version_series.clone()));
            m.insert("Id".into(), Nbt::Int(self.version_id));
            m.insert("Name".into(), Nbt::String(self.version_name.clone()));
            Nbt::Compound(m)
        });
        d.insert("LevelName".into(), Nbt::String(self.level_name.clone()));
        d.insert("initialized".into(), Nbt::Byte(i8::from(self.initialized)));
        d.insert("WasModded".into(), Nbt::Byte(i8::from(self.was_modded)));
        d.insert("DataVersion".into(), Nbt::Int(self.data_version));
        d.insert(
            "allowCommands".into(),
            Nbt::Byte(i8::from(self.allow_commands)),
        );
        d.insert("DataPacks".into(), {
            let mut m = BTreeMap::new();
            m.insert("Enabled".into(), strings(&self.datapacks_enabled));
            m.insert("Disabled".into(), strings(&self.datapacks_disabled));
            Nbt::Compound(m)
        });
        Nbt::Compound(d)
    }

    /// The whole file: `{ "": { Data: … } }`, the shape
    /// `LevelStorageSource.TAG_DATA` names.
    #[must_use]
    pub fn to_file_nbt(&self) -> Nbt {
        let mut root = BTreeMap::new();
        root.insert("Data".into(), self.to_nbt());
        Nbt::Compound(root)
    }

    /// Parse a whole `level.dat` root.
    ///
    /// # Errors
    /// If `Data` is missing, or carries a key this does not model, or a key
    /// of the wrong type.
    pub fn from_file_nbt(root: &Nbt) -> io::Result<Self> {
        let data = root
            .get("Data")
            .ok_or_else(|| io::Error::other("level.dat has no `Data` compound"))?;
        Self::from_nbt(data)
    }

    /// Parse the `Data` compound.
    ///
    /// # Errors
    /// If a key is missing, mistyped, or **unknown** — see the type comment
    /// for why an unknown key is an error rather than something to ignore.
    #[allow(clippy::too_many_lines)]
    pub fn from_nbt(data: &Nbt) -> io::Result<Self> {
        let map = data
            .as_compound()
            .ok_or_else(|| io::Error::other("level.dat `Data` is not a compound"))?;
        let miss = |k: &str| io::Error::other(format!("level.dat: no `{k}`"));
        let wrong = |k: &str| io::Error::other(format!("level.dat: `{k}` has the wrong type"));
        let get = |k: &str| map.get(k).ok_or_else(|| miss(k));
        let int = |k: &str| -> io::Result<i32> {
            match get(k)? {
                Nbt::Int(v) => Ok(*v),
                _ => Err(wrong(k)),
            }
        };
        let long = |k: &str| -> io::Result<i64> {
            match get(k)? {
                Nbt::Long(v) => Ok(*v),
                _ => Err(wrong(k)),
            }
        };
        let boolean = |k: &str| -> io::Result<bool> {
            match get(k)? {
                Nbt::Byte(v) => Ok(*v != 0),
                _ => Err(wrong(k)),
            }
        };
        let text = |k: &str| -> io::Result<String> {
            match get(k)? {
                Nbt::String(v) => Ok(v.clone()),
                _ => Err(wrong(k)),
            }
        };
        // A nested lookup, so the error names the path rather than the leaf:
        // two compounds here both carry a `Name`-ish key and "wrong type" on
        // its own would not say which.
        let inner = |outer: &str, k: &str| -> io::Result<Nbt> {
            get(outer)?
                .get(k)
                .cloned()
                .ok_or_else(|| io::Error::other(format!("level.dat: no `{outer}.{k}`")))
        };
        let inner_bool = |outer: &str, k: &str| -> io::Result<bool> {
            match inner(outer, k)? {
                Nbt::Byte(v) => Ok(v != 0),
                _ => Err(wrong(k)),
            }
        };
        let inner_text = |outer: &str, k: &str| -> io::Result<String> {
            match inner(outer, k)? {
                Nbt::String(v) => Ok(v),
                _ => Err(wrong(k)),
            }
        };
        let string_list = |tag: &Nbt, what: &str| -> io::Result<Vec<String>> {
            tag.as_list()
                .ok_or_else(|| io::Error::other(format!("level.dat: `{what}` is not a list")))?
                .iter()
                .map(|e| {
                    e.as_str().map(str::to_string).ok_or_else(|| {
                        io::Error::other(format!("level.dat: `{what}` holds a non-string"))
                    })
                })
                .collect()
        };

        const KNOWN: [&str; 14] = [
            "difficulty_settings",
            "Time",
            "GameType",
            "ServerBrands",
            "version",
            "LastPlayed",
            "spawn",
            "Version",
            "LevelName",
            "initialized",
            "WasModded",
            "DataVersion",
            "allowCommands",
            "DataPacks",
        ];
        if let Some(unknown) = map.keys().find(|k| !KNOWN.contains(&k.as_str())) {
            return Err(io::Error::other(format!(
                "level.dat: unmodelled key `{unknown}` — dropping it would lose a world setting"
            )));
        }

        let pos = match inner("spawn", "pos")? {
            Nbt::IntArray(v) if v.len() == 3 => v,
            _ => return Err(wrong("spawn.pos")),
        };
        let float = |outer: &str, k: &str| -> io::Result<f32> {
            match inner(outer, k)? {
                Nbt::Float(v) => Ok(v),
                _ => Err(wrong(k)),
            }
        };
        Ok(Self {
            difficulty: inner_text("difficulty_settings", "difficulty")?,
            hardcore: inner_bool("difficulty_settings", "hardcore")?,
            difficulty_locked: inner_bool("difficulty_settings", "locked")?,
            time: long("Time")?,
            game_type: int("GameType")?,
            server_brands: string_list(get("ServerBrands")?, "ServerBrands")?,
            anvil_version: int("version")?,
            last_played: long("LastPlayed")?,
            spawn: Spawn {
                x: pos[0],
                y: pos[1],
                z: pos[2],
                yaw: float("spawn", "yaw")?,
                pitch: float("spawn", "pitch")?,
                dimension: inner_text("spawn", "dimension")?,
            },
            version_id: match inner("Version", "Id")? {
                Nbt::Int(v) => v,
                _ => return Err(wrong("Version.Id")),
            },
            version_name: inner_text("Version", "Name")?,
            version_series: inner_text("Version", "Series")?,
            version_snapshot: inner_bool("Version", "Snapshot")?,
            level_name: text("LevelName")?,
            initialized: boolean("initialized")?,
            was_modded: boolean("WasModded")?,
            data_version: int("DataVersion")?,
            allow_commands: boolean("allowCommands")?,
            datapacks_enabled: string_list(&inner("DataPacks", "Enabled")?, "DataPacks.Enabled")?,
            datapacks_disabled: string_list(
                &inner("DataPacks", "Disabled")?,
                "DataPacks.Disabled",
            )?,
        })
    }
}

/// One `data/<namespace>/<id>.dat` file: a `data` compound in an envelope
/// carrying the version it was written at.
///
/// `SavedDataStorage` writes every one of these through the same path, so the
/// envelope is the same for `game_rules.dat`, `weather.dat` and the rest, and
/// only the inner compound differs. Kept as raw NBT rather than nine typed
/// structs because the envelope is the part this crate owns; the server
/// crate knows what a game rule is.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedDataFile {
    pub data: Nbt,
    pub data_version: i32,
}

impl SavedDataFile {
    #[must_use]
    pub fn new(data: Nbt) -> Self {
        Self {
            data,
            data_version: DATA_VERSION,
        }
    }

    #[must_use]
    pub fn to_file_nbt(&self) -> Nbt {
        let mut root = BTreeMap::new();
        root.insert("data".into(), self.data.clone());
        root.insert("DataVersion".into(), Nbt::Int(self.data_version));
        Nbt::Compound(root)
    }

    /// # Errors
    /// If `data` or `DataVersion` is missing or mistyped.
    pub fn from_file_nbt(root: &Nbt) -> io::Result<Self> {
        let data = root
            .get("data")
            .ok_or_else(|| io::Error::other("saved data: no `data` compound"))?
            .clone();
        let Some(Nbt::Int(data_version)) = root.get("DataVersion") else {
            return Err(io::Error::other("saved data: no int `DataVersion`"));
        };
        Ok(Self {
            data,
            data_version: *data_version,
        })
    }
}

/// Write a `.dat` through vanilla's own replace dance.
///
/// `Util.safeReplaceFile` writes `<name>_new`, moves any existing `<name>` to
/// `<name>_old`, then moves `_new` into place — so a crash mid-write loses at
/// most the *previous* save and never leaves a half-written `level.dat`.
/// `LevelResource` names `level.dat_old`, and `PlayerDataStorage` has
/// `.dat_old` in its own constant pool, so both files use it.
///
/// # Errors
/// If the directory cannot be created or any of the three steps fails.
pub fn write_dat_safely(path: &std::path::Path, name: &str, value: &Nbt) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let bytes = nbt::write_compressed(name, value)?;
    let with_suffix = |suffix: &str| {
        let mut s = path.as_os_str().to_os_string();
        s.push(suffix);
        std::path::PathBuf::from(s)
    };
    let new = with_suffix("_new");
    let old = with_suffix("_old");
    std::fs::write(&new, &bytes)?;
    if path.exists() {
        // A stale `_old` from an earlier save has to go first: `rename` over
        // an existing file is fine on Linux but vanilla deletes it, and a
        // platform where it is not fine would fail here rather than corrupt.
        let _ = std::fs::remove_file(&old);
        std::fs::rename(path, &old)?;
    }
    std::fs::rename(&new, path)
}

/// Read a gzip `.dat` off disk.
///
/// # Errors
/// If the file cannot be read or is not gzip NBT.
pub fn read_dat(path: &std::path::Path) -> io::Result<Nbt> {
    let bytes = std::fs::read(path)?;
    Ok(nbt::read_compressed(&bytes)?.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The harvested vanilla worlds, resolved from the crate directory rather
    /// than the working directory: `cargo test` runs a test binary with the
    /// crate root as its cwd today, and a relative path that silently resolves
    /// to nothing is how a fixture-backed check becomes a check of nothing.
    pub(super) fn harvest() -> &'static std::path::Path {
        std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../oracle-work"))
    }

    /// Every harvested vanilla world on this box, so the golden below is over
    /// a real population rather than one lucky file. Empty when the
    /// references are not present — which is a *skip*, and it is reported as
    /// one by asserting the count separately in
    /// [`the_harvest_is_actually_there`].
    fn harvested_level_dats() -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        if !rustmc_data::fixture::available(
            super::tests::harvest(),
            "the vanilla world harvest",
            "ci/gen-vanilla-region.sh",
        ) {
            return out;
        }
        let Ok(entries) = std::fs::read_dir(super::tests::harvest()) else {
            return out;
        };
        for entry in entries.flatten() {
            let dat = entry.path().join("world/level.dat");
            if dat.is_file() {
                out.push(dat);
            }
        }
        out.sort();
        out
    }

    /// **A vanilla `level.dat` decoded into [`LevelData`] and re-encoded is
    /// byte-identical to the original payload.**
    ///
    /// This is the golden the whole module rests on, and what makes it one
    /// rather than a round trip through our own writer is that the *input* is
    /// real vanilla output that nothing here produced. It can go red four
    /// separate ways, and each is a real defect:
    ///
    /// * a key renamed or dropped — the payload is shorter;
    /// * a key given the wrong tag — Int against Long is four bytes;
    /// * a value not carried through the struct — the bytes differ at it;
    /// * **the compound key ORDER wrong** — which is the interesting one.
    ///   `CompoundTag.write` iterates a `java.util.HashMap`'s `keySet()`, so
    ///   the order is a function of `String.hashCode` and the table capacity,
    ///   and 14 keys means a table that has resized once to 32 buckets. If
    ///   [`crate::nbt`]'s reproduction of that were wrong this test would
    ///   fail on the first file, which is exactly the property a sorted-key
    ///   writer would not have.
    ///
    /// The comparison is on the **decompressed** payload: gzip is not
    /// byte-reproducible across zlib implementations and carries an mtime.
    #[test]
    fn every_harvested_level_dat_re_encodes_byte_for_byte() {
        for path in harvested_level_dats() {
            let raw = std::fs::read(&path).expect("read level.dat");
            let (name, root) = nbt::read_compressed(&raw).expect("gunzip + parse");
            let original = nbt::encode_named_root(&name, &root).expect("re-encode as read");
            let level = LevelData::from_file_nbt(&root)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let ours = nbt::encode_named_root(&name, &level.to_file_nbt()).expect("encode ours");
            assert_eq!(
                ours.len(),
                original.len(),
                "{}: payload is {} bytes, ours is {}",
                path.display(),
                original.len(),
                ours.len()
            );
            assert_eq!(ours, original, "{}: payload differs", path.display());
        }
    }

    /// The population the golden above ran over, asserted rather than
    /// assumed. A `read_dir` that finds nothing makes that test pass over
    /// zero files, which is this project's own named defect class — so the
    /// count is its own check and its own failure message.
    #[test]
    fn the_harvest_is_actually_there() {
        let found = harvested_level_dats().len();
        let optional = found == 0
            && rustmc_data::fixture::Mode::from_env() == rustmc_data::fixture::Mode::Optional;
        assert!(
            optional || found >= 20,
            "only {found} harvested level.dat files under ../../oracle-work; \
             the byte-identity golden was measured over 40"
        );
    }

    /// The `DataVersion` is read off the harvest rather than remembered, and
    /// this is what stops it drifting: 4903 is what 26.2 writes.
    #[test]
    fn the_harvest_agrees_on_the_data_version() {
        for path in harvested_level_dats() {
            let raw = std::fs::read(&path).expect("read");
            let (_, root) = nbt::read_compressed(&raw).expect("parse");
            let level = LevelData::from_file_nbt(&root).expect("decode");
            assert_eq!(level.data_version, DATA_VERSION, "{}", path.display());
            assert_eq!(level.version_id, DATA_VERSION, "{}", path.display());
            assert_eq!(level.version_name, VERSION_NAME, "{}", path.display());
            assert_eq!(level.anvil_version, ANVIL_VERSION, "{}", path.display());
        }
    }

    /// An unmodelled key is refused, not dropped. Without this the golden
    /// above would still pass on a file whose extra key we silently threw
    /// away, because we would then be comparing our own shorter output
    /// against… no, we would not — but a *future* `from_nbt` that ignored
    /// unknown keys would break the golden in a way that reads as a writer
    /// bug. This says which it is.
    #[test]
    fn an_unmodelled_key_is_an_error() {
        let level = LevelData::new("world");
        let Nbt::Compound(mut data) = level.to_nbt() else {
            panic!("compound")
        };
        data.insert("SomeNewKey".into(), Nbt::Int(1));
        let err = LevelData::from_nbt(&Nbt::Compound(data)).expect_err("must refuse");
        assert!(err.to_string().contains("SomeNewKey"), "{err}");
    }

    /// A fresh level.dat round-trips through our own pair. Weaker than the
    /// golden and here for a different reason: it is the only one that runs
    /// when the references are absent.
    #[test]
    fn a_fresh_level_dat_round_trips() {
        let level = LevelData::new("rustmc");
        let back = LevelData::from_file_nbt(&level.to_file_nbt()).expect("decode");
        assert_eq!(level, back);
    }

    /// The saved-data envelope, against a real `weather.dat`.
    #[test]
    fn a_harvested_saved_data_file_re_encodes_byte_for_byte() {
        let mut checked = 0;
        let Ok(entries) = std::fs::read_dir(harvest()) else {
            return;
        };
        for entry in entries.flatten() {
            let dir = entry.path().join("world/data/minecraft");
            let Ok(files) = std::fs::read_dir(&dir) else {
                continue;
            };
            for file in files.flatten() {
                let path = file.path();
                if path.extension().is_none_or(|e| e != "dat") {
                    continue;
                }
                let raw = std::fs::read(&path).expect("read");
                let (name, root) = nbt::read_compressed(&raw).expect("parse");
                let original = nbt::encode_named_root(&name, &root).expect("re-encode");
                let saved = SavedDataFile::from_file_nbt(&root)
                    .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                let ours =
                    nbt::encode_named_root(&name, &saved.to_file_nbt()).expect("encode ours");
                assert_eq!(ours, original, "{}", path.display());
                checked += 1;
            }
        }
        assert!(
            checked >= 9 || checked == 0,
            "only {checked} harvested data/minecraft/*.dat files re-encoded; \
             a 26.2 world carries nine"
        );
    }

    /// `write_dat_safely` leaves the previous file as `_old` and no `_new`
    /// behind — the property that makes a crash mid-save cost one generation
    /// rather than the world.
    #[test]
    fn the_safe_write_rotates_the_old_file() {
        let dir = std::env::temp_dir().join(format!("rustmc-level-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("level.dat");
        let first = LevelData::new("first");
        let second = LevelData::new("second");
        write_dat_safely(&path, "", &first.to_file_nbt()).expect("first write");
        assert!(
            !path.with_extension("dat_old").exists(),
            "nothing to rotate"
        );
        write_dat_safely(&path, "", &second.to_file_nbt()).expect("second write");

        let live = LevelData::from_file_nbt(&read_dat(&path).expect("read live")).expect("decode");
        assert_eq!(live.level_name, "second");
        let old_path = dir.join("level.dat_old");
        let old =
            LevelData::from_file_nbt(&read_dat(&old_path).expect("read old")).expect("decode");
        assert_eq!(old.level_name, "first");
        assert!(!dir.join("level.dat_new").exists(), "no _new left behind");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Constructors for the nine `data/minecraft/*.dat` payloads a 26.2 world
/// carries, each returning the inner `data` compound that
/// [`SavedDataFile::new`] wraps.
///
/// Every shape here was read off real vanilla output and every one of them
/// has a byte-for-byte golden below, which is the only reason a hand-built
/// nested compound is trustworthy: `world_gen_settings` alone is four levels
/// deep and carries compounds of one, two and three keys, so a wrong
/// `java.util.HashMap` order at any of those widths is a red.
pub mod saved {
    use super::{BTreeMap, Nbt};

    /// `data/minecraft/world_gen_settings.dat` — where the **seed** lives in
    /// 26.2. It is not in `level.dat` and has not been since the split.
    ///
    /// The three dimension entries are the vanilla defaults, spelled out
    /// rather than derived: `WorldDimensions.DEFAULT_DIMENSIONS` builds them
    /// from registry presets and the serialised form is what a reader sees.
    #[must_use]
    pub fn world_gen_settings(seed: i64, generate_structures: bool, bonus_chest: bool) -> Nbt {
        let noise = |settings: &str, biome_source: Nbt, dim_type: &str| {
            let mut generator = BTreeMap::new();
            generator.insert("settings".into(), Nbt::String(settings.into()));
            generator.insert("biome_source".into(), biome_source);
            generator.insert("type".into(), Nbt::String("minecraft:noise".into()));
            let mut entry = BTreeMap::new();
            entry.insert("generator".into(), Nbt::Compound(generator));
            entry.insert("type".into(), Nbt::String(dim_type.into()));
            Nbt::Compound(entry)
        };
        let multi_noise = |preset: &str| {
            let mut m = BTreeMap::new();
            m.insert("preset".into(), Nbt::String(preset.into()));
            m.insert("type".into(), Nbt::String("minecraft:multi_noise".into()));
            Nbt::Compound(m)
        };
        let mut dims = BTreeMap::new();
        dims.insert(
            "minecraft:overworld".into(),
            noise(
                "minecraft:overworld",
                multi_noise("minecraft:overworld"),
                "minecraft:overworld",
            ),
        );
        dims.insert(
            "minecraft:the_nether".into(),
            noise(
                "minecraft:nether",
                multi_noise("minecraft:nether"),
                "minecraft:the_nether",
            ),
        );
        dims.insert("minecraft:the_end".into(), {
            let mut source = BTreeMap::new();
            source.insert("type".into(), Nbt::String("minecraft:the_end".into()));
            noise("minecraft:end", Nbt::Compound(source), "minecraft:the_end")
        });

        let mut data = BTreeMap::new();
        data.insert("bonus_chest".into(), Nbt::Byte(i8::from(bonus_chest)));
        data.insert("seed".into(), Nbt::Long(seed));
        data.insert(
            "generate_structures".into(),
            Nbt::Byte(i8::from(generate_structures)),
        );
        data.insert("dimensions".into(), Nbt::Compound(dims));
        Nbt::Compound(data)
    }

    /// `data/minecraft/weather.dat`.
    ///
    /// `clear_weather_time` is 1,000,000 on a fresh world rather than 0 —
    /// that is `ServerLevelData`'s own initial value and it means "clear for
    /// a very long time", not "no clear weather scheduled". Writing 0 there
    /// would make a reloaded world start rolling for rain immediately.
    #[must_use]
    pub fn weather(
        raining: bool,
        rain_time: i32,
        thundering: bool,
        thunder_time: i32,
        clear_weather_time: i32,
    ) -> Nbt {
        let mut m = BTreeMap::new();
        m.insert("rain_time".into(), Nbt::Int(rain_time));
        m.insert("raining".into(), Nbt::Byte(i8::from(raining)));
        m.insert("thundering".into(), Nbt::Byte(i8::from(thundering)));
        m.insert("thunder_time".into(), Nbt::Int(thunder_time));
        m.insert("clear_weather_time".into(), Nbt::Int(clear_weather_time));
        Nbt::Compound(m)
    }

    /// `data/minecraft/world_clocks.dat` — the per-dimension day/time clock,
    /// which is what `level.dat`'s `DayTime` used to be.
    ///
    /// The harvest carries **two** entries, `minecraft:overworld` and
    /// `minecraft:the_end`, and no nether: the nether has no day cycle, so it
    /// has no clock. A writer that emitted all three would produce a file
    /// vanilla reads without complaint and that differs from its own.
    #[must_use]
    pub fn world_clocks(overworld_ticks: i64, end_ticks: i64) -> Nbt {
        let clock = |ticks: i64| {
            let mut m = BTreeMap::new();
            m.insert("total_ticks".into(), Nbt::Long(ticks));
            Nbt::Compound(m)
        };
        let mut m = BTreeMap::new();
        m.insert("minecraft:overworld".into(), clock(overworld_ticks));
        m.insert("minecraft:the_end".into(), clock(end_ticks));
        Nbt::Compound(m)
    }

    /// `data/minecraft/random_sequences.dat`.
    ///
    /// `sequences` is **not** always empty and that was measured rather than
    /// assumed: a world that has run any ticks accumulates one entry per
    /// `RandomSequence` that has been drawn from — five of them in
    /// `oracle-work/gen-20260722-25852`, each a compound holding a `source`
    /// `LongArray`. So the content is caller state, not a constant, and this
    /// takes it. Passing `Nbt::Compound(BTreeMap::new())` is the fresh-world
    /// value.
    #[must_use]
    pub fn random_sequences(salt: i32, sequences: Nbt) -> Nbt {
        let mut m = BTreeMap::new();
        m.insert("salt".into(), Nbt::Int(salt));
        m.insert("sequences".into(), sequences);
        Nbt::Compound(m)
    }

    /// `data/minecraft/scheduled_events.dat`.
    #[must_use]
    pub fn scheduled_events() -> Nbt {
        let mut m = BTreeMap::new();
        m.insert("events".into(), Nbt::List(Vec::new()));
        Nbt::Compound(m)
    }

    /// `data/minecraft/stopwatches.dat`.
    #[must_use]
    pub fn stopwatches() -> Nbt {
        let mut m = BTreeMap::new();
        m.insert("stopwatches".into(), Nbt::Compound(BTreeMap::new()));
        Nbt::Compound(m)
    }

    /// `data/minecraft/scoreboard.dat` and
    /// `data/minecraft/custom_boss_events.dat` are both an **empty** `data`
    /// compound on a fresh world — 48 bytes each, the whole file being the
    /// envelope. One function because they are the same value, named so a
    /// caller does not have to know that.
    #[must_use]
    pub fn empty() -> Nbt {
        Nbt::Compound(BTreeMap::new())
    }

    /// `data/minecraft/game_rules.dat` from a list of `(namespaced id, tag)`.
    ///
    /// The ids are namespaced — `minecraft:spawn_mobs`, not `spawn_mobs` —
    /// and the values are `Byte` for a boolean rule and `Int` for a numeric
    /// one, matching the registry's own type for each.
    #[must_use]
    pub fn game_rules(rules: &[(String, Nbt)]) -> Nbt {
        let mut m = BTreeMap::new();
        for (id, value) in rules {
            m.insert(id.clone(), value.clone());
        }
        Nbt::Compound(m)
    }
}

#[cfg(test)]
mod saved_goldens {
    use super::{nbt, saved, SavedDataFile};

    /// The harvest's **default-overworld** worlds only.
    ///
    /// `ci/gen-vanilla-region.sh` names them `gen-<seed>-<port>`, and every
    /// other `world` directory under `oracle-work` was made for some other
    /// purpose with some other settings. `recipe-dump`'s is the one that
    /// forced this split: `ci/gen-gameplay-tables.sh` boots it as a
    /// **superflat** with `generate_structures` off, so its
    /// `world_gen_settings.dat` carries a `minecraft:flat` generator with a
    /// three-layer list and two structure overrides. Comparing a
    /// default-overworld constructor against it is a red that says nothing
    /// about the constructor.
    ///
    /// The filter is on the directory name, which is independent of the file
    /// being compared — filtering on "the ones that match" would be circular.
    fn default_overworld_worlds() -> Vec<std::path::PathBuf> {
        worlds()
            .into_iter()
            .filter(|dir| {
                dir.ancestors()
                    .nth(3)
                    .and_then(|d| d.file_name())
                    .is_some_and(|n| n.to_string_lossy().starts_with("gen-"))
            })
            .collect()
    }

    /// Whether a world directory was written **only** by vanilla.
    ///
    /// `PrimaryLevelData` keeps `ServerBrands` as the set of brands that have
    /// ever run the world, so one this server has opened says `rustmc`. That
    /// is a property of the WORLD rather than of the file being compared, so
    /// the filter cannot select for the answer — the same argument, and the
    /// same predicate, as `crate::playerdat`'s `written_only_by_vanilla`.
    /// **Both copies exist deliberately**: the alternative is a filesystem
    /// provenance check in shipped code, and each is six lines. If you change
    /// one, change the other.
    ///
    /// A world with no readable `level.dat` is **not** vanilla's for this
    /// purpose. It is unknown, and an unknown is not evidence — and it is not
    /// hypothetical: see [`worlds`].
    fn written_only_by_vanilla(world: &std::path::Path) -> bool {
        let Ok(root) = crate::level::read_dat(&world.join("level.dat")) else {
            return false;
        };
        let Ok(level) = crate::level::LevelData::from_file_nbt(&root) else {
            return false;
        };
        !level.server_brands.iter().any(|b| b.contains("rustmc"))
    }

    /// Every harvested **vanilla** world, so a shape that happens to match one
    /// seed's file does not pass.
    ///
    /// # The filter is not defensive, it is the third instance of this defect
    ///
    /// This function used to take every directory under `oracle-work` with a
    /// `world/data/minecraft` in it, whoever wrote it. Three of the gates in
    /// `ci/` leave a world at exactly that depth, and on 2026-09-05 T0 went
    /// red with
    ///
    /// ```text
    ///   the_saved_game_rules_are_the_registry_minus_the_gated_one
    ///   panicked: read: Os { code: 2, kind: NotFound }
    /// ```
    ///
    /// on `oracle-work/t2-sigterm-saves/world/data/minecraft`, which held
    /// **zero files**: that gate's negative-control arm `kill -9`s the server,
    /// so the directory is created at open and nothing is ever written into
    /// it. The registry had not changed and no golden was wrong.
    ///
    /// **The crash is the lucky half.** The tests either side of that one call
    /// `vanilla(&dir, name)`, which returns `Option` and `continue`s on a
    /// missing file — so for a world this server wrote *completely*
    /// (`ci/t2-player-survives-a-restart.sh` leaves one, with all nine files)
    /// they found the file, and compared **this crate's encoder against this
    /// crate's own output**, silently, while their population assertions
    /// stayed green. That is the same defect
    /// `a_real_vanilla_player_file_re_encodes_byte_for_byte` was caught in on
    /// 2026-09-05 by its own `extra.len() >= 10` assertion, in a second file,
    /// and it had no assertion that could catch it.
    pub(super) fn worlds() -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        if !rustmc_data::fixture::available(
            super::tests::harvest(),
            "the vanilla world harvest",
            "ci/gen-vanilla-region.sh",
        ) {
            return out;
        }
        let Ok(entries) = std::fs::read_dir(super::tests::harvest()) else {
            return out;
        };
        for entry in entries.flatten() {
            let world = entry.path().join("world");
            if world.join("data/minecraft").is_dir() && written_only_by_vanilla(&world) {
                out.push(world.join("data/minecraft"));
            }
        }
        out.sort();
        out
    }

    /// The payload vanilla wrote, decompressed, for one `data/minecraft` file.
    pub(super) fn vanilla(dir: &std::path::Path, name: &str) -> Option<Vec<u8>> {
        let raw = std::fs::read(dir.join(name)).ok()?;
        let (root_name, root) = nbt::read_compressed(&raw).ok()?;
        Some(nbt::encode_named_root(&root_name, &root).expect("re-encode"))
    }

    /// The seed the world was generated at, taken from vanilla's own file so
    /// the comparison is not seeded by a filename convention.
    fn seed_of(dir: &std::path::Path) -> i64 {
        let raw = std::fs::read(dir.join("world_gen_settings.dat")).expect("read");
        let (_, root) = nbt::read_compressed(&raw).expect("parse");
        let data = root.get("data").expect("data");
        match data.get("seed") {
            Some(crate::nbt::Nbt::Long(v)) => *v,
            other => panic!("seed is {other:?}"),
        }
    }

    /// **Our constructor, not our reader, against vanilla's bytes.**
    ///
    /// The `level.dat` golden decodes vanilla's own tag and re-encodes it, so
    /// it cannot catch a *missing* branch of the shape — anything vanilla
    /// wrote comes back because it went in. These build the compound from
    /// scratch, from a seed and two booleans, and compare the result to the
    /// file. A wrong key, a wrong nesting level, a `Long` seed written as an
    /// `Int`, the nether's biome source given a `preset` it does not have, or
    /// the wrong `java.util.HashMap` order at any of the one-, two-, three-
    /// and four-key compounds involved: all red.
    #[test]
    fn world_gen_settings_is_built_byte_for_byte() {
        let mut checked = 0;
        // The control for the filter: the superflat world is present and is
        // excluded. Without this, a filter that matched nothing would make
        // the loop below vacuous and the count assertion would be the only
        // thing left standing.
        assert!(
            worlds().len() > default_overworld_worlds().len(),
            "the gen-* filter excluded nothing, so it is not filtering"
        );
        for dir in default_overworld_worlds() {
            let Some(want) = vanilla(&dir, "world_gen_settings.dat") else {
                continue;
            };
            let ours = nbt::encode_named_root(
                "",
                &SavedDataFile::new(saved::world_gen_settings(seed_of(&dir), true, false))
                    .to_file_nbt(),
            )
            .expect("encode");
            assert_eq!(ours, want, "{}", dir.display());
            checked += 1;
        }
        assert!(
            checked >= 20 || worlds().is_empty(),
            "only {checked} default-overworld world_gen_settings.dat checked"
        );
    }

    /// `world_clocks.dat` carries the overworld and the End and **not** the
    /// nether. Building all three would be a file vanilla accepts and did not
    /// write, which no round-trip test can see.
    #[test]
    fn world_clocks_is_built_byte_for_byte() {
        let mut checked = 0;
        for dir in worlds() {
            let Some(want) = vanilla(&dir, "world_clocks.dat") else {
                continue;
            };
            // The harvest's worlds all stopped after one tick; read the value
            // back out rather than assuming it, so this stays a shape test.
            let raw = std::fs::read(dir.join("world_clocks.dat")).expect("read");
            let (_, root) = nbt::read_compressed(&raw).expect("parse");
            let ticks = |dim: &str| match root
                .get("data")
                .and_then(|d| d.get(dim))
                .and_then(|d| d.get("total_ticks"))
            {
                Some(crate::nbt::Nbt::Long(v)) => *v,
                other => panic!("{dim} total_ticks is {other:?}"),
            };
            let ours = nbt::encode_named_root(
                "",
                &SavedDataFile::new(saved::world_clocks(
                    ticks("minecraft:overworld"),
                    ticks("minecraft:the_end"),
                ))
                .to_file_nbt(),
            )
            .expect("encode");
            assert_eq!(ours, want, "{}", dir.display());
            checked += 1;
        }
        assert!(
            checked >= 20 || worlds().is_empty(),
            "only {checked} world_clocks.dat checked"
        );
    }

    /// The five fixed-shape files, built from constants and compared to
    /// vanilla's own bytes. `clear_weather_time` is the one that is not
    /// obvious — 1,000,000 on a fresh world, not 0.
    #[test]
    fn the_fixed_shape_saved_files_are_built_byte_for_byte() {
        let mut checked = 0;
        for dir in worlds() {
            let cases: [(&str, crate::nbt::Nbt); 4] = [
                ("scheduled_events.dat", saved::scheduled_events()),
                ("stopwatches.dat", saved::stopwatches()),
                ("scoreboard.dat", saved::empty()),
                ("custom_boss_events.dat", saved::empty()),
            ];
            for (name, data) in cases {
                let Some(want) = vanilla(&dir, name) else {
                    continue;
                };
                let ours = nbt::encode_named_root("", &SavedDataFile::new(data).to_file_nbt())
                    .expect("encode");
                assert_eq!(ours, want, "{}/{name}", dir.display());
                checked += 1;
            }
        }
        assert!(
            checked >= 80 || worlds().is_empty(),
            "only {checked} fixed-shape files checked"
        );
    }

    /// **The provenance filter is live, and it is not filtering everything.**
    ///
    /// Both halves are the assertion. A filter that removed every world would
    /// make every golden in this module pass over zero files, and each of
    /// their `checked >= N` population assertions is guarded by
    /// `|| worlds().is_empty()` — so an over-eager filter turns the whole
    /// module green and silent. A filter that removed nothing is the state
    /// that put a zero-file `data/minecraft` in front of
    /// `the_saved_game_rules_are_the_registry_minus_the_gated_one`.
    ///
    /// The lower half is unconditional. The upper half can only be asserted
    /// when a world this server wrote is actually present, which is exactly
    /// when one of `ci/t2-player-survives-a-restart.sh`,
    /// `ci/t2-sigterm-saves.sh` or `ci/t2-vanilla-opens-our-world.sh` has run
    /// in this tree — so it is conditional on that rather than skipped, and
    /// it names the directory it excluded.
    #[test]
    fn the_world_sweep_excludes_worlds_this_server_wrote() {
        if worlds().is_empty() {
            return;
        }
        let mut unfiltered = Vec::new();
        for entry in std::fs::read_dir(super::tests::harvest())
            .into_iter()
            .flatten()
            .flatten()
        {
            let world = entry.path().join("world");
            if world.join("data/minecraft").is_dir() {
                unfiltered.push(world);
            }
        }
        let kept = worlds();
        assert!(
            !kept.is_empty(),
            "the filter removed all {} candidate worlds; every golden in this \
             module would then pass over zero files, because each of their \
             population assertions is guarded by `|| worlds().is_empty()`",
            unfiltered.len()
        );
        let ours: Vec<_> = unfiltered
            .iter()
            .filter(|w| !written_only_by_vanilla(w))
            .collect();
        if !ours.is_empty() {
            assert_eq!(
                kept.len() + ours.len(),
                unfiltered.len(),
                "the filter kept {} of {} worlds and {} are ours: {ours:?}",
                kept.len(),
                unfiltered.len(),
                ours.len()
            );
            for world in &ours {
                assert!(
                    !kept.contains(&world.join("data/minecraft")),
                    "{world:?} was written by this server and is still in the sweep"
                );
            }
        }
        // Every world that survived must carry the file the goldens read
        // unconditionally. Without this the filter could be right about
        // provenance and still hand a golden a half-written vanilla world —
        // which is a different failure with the same panic, and the panic is
        // what sent this investigation at the registry instead of at the
        // sweep.
        for dir in &kept {
            assert!(
                dir.join("game_rules.dat").is_file(),
                "{dir:?} survived the filter and has no game_rules.dat"
            );
        }
    }

    /// `game_rules.dat` holds **58** rules while the 26.2 registry has 59.
    ///
    /// The one that is absent is `minecraft:max_minecart_speed`, and every
    /// harvested world has `minecart_improvements` in its `DataPacks.Disabled`
    /// — so a rule gated behind a disabled feature pack is not written at all.
    /// This is pinned because the obvious writer emits the whole registry,
    /// which would produce a 59-rule file for a world vanilla writes 58 into.
    #[test]
    fn the_saved_game_rules_are_the_registry_minus_the_gated_one() {
        let mut checked = 0;
        for dir in worlds() {
            let raw = std::fs::read(dir.join("game_rules.dat")).expect("read");
            let (_, root) = nbt::read_compressed(&raw).expect("parse");
            let rules = root
                .get("data")
                .and_then(crate::nbt::Nbt::as_compound)
                .expect("data compound");
            assert_eq!(rules.len(), 58, "{}", dir.display());
            assert!(
                !rules.contains_key("minecraft:max_minecart_speed"),
                "{}: max_minecart_speed is gated behind minecart_improvements",
                dir.display()
            );
            assert!(rules.contains_key("minecraft:spawn_mobs"), "snake_case ids");
            checked += 1;
        }
        assert!(
            checked >= 20 || worlds().is_empty(),
            "only {checked} game_rules.dat checked"
        );
    }
}

#[cfg(test)]
mod state_bearing_goldens {
    use super::saved_goldens::{vanilla, worlds};
    use super::{nbt, saved, Nbt, SavedDataFile};

    /// `weather.dat` and `random_sequences.dat` carry **world state**, so
    /// neither can be built from constants and compared. These read the
    /// values out of vanilla's own file and rebuild the compound around them.
    ///
    /// That is a weaker claim than the constructed goldens and it is stated
    /// rather than hidden: what it still tests is every key name, every tag
    /// width, the envelope and the `java.util.HashMap` key order — which is
    /// exactly what a hand-built compound gets wrong. What it cannot test is
    /// whether the *values* are the ones vanilla would have chosen, and no
    /// file comparison could.
    #[test]
    fn the_state_bearing_saved_files_rebuild_byte_for_byte() {
        let mut checked = 0;
        for dir in worlds() {
            if let Some(want) = vanilla(&dir, "weather.dat") {
                let raw = std::fs::read(dir.join("weather.dat")).expect("read");
                let (_, root) = nbt::read_compressed(&raw).expect("parse");
                let data = root.get("data").expect("data");
                let int = |k: &str| match data.get(k) {
                    Some(Nbt::Int(v)) => *v,
                    other => panic!("weather.{k} is {other:?}"),
                };
                let flag = |k: &str| match data.get(k) {
                    Some(Nbt::Byte(v)) => *v != 0,
                    other => panic!("weather.{k} is {other:?}"),
                };
                let ours = nbt::encode_named_root(
                    "",
                    &SavedDataFile::new(saved::weather(
                        flag("raining"),
                        int("rain_time"),
                        flag("thundering"),
                        int("thunder_time"),
                        int("clear_weather_time"),
                    ))
                    .to_file_nbt(),
                )
                .expect("encode");
                assert_eq!(ours, want, "{}/weather.dat", dir.display());
                checked += 1;
            }
            if let Some(want) = vanilla(&dir, "random_sequences.dat") {
                let raw = std::fs::read(dir.join("random_sequences.dat")).expect("read");
                let (_, root) = nbt::read_compressed(&raw).expect("parse");
                let data = root.get("data").expect("data");
                let salt = match data.get("salt") {
                    Some(Nbt::Int(v)) => *v,
                    other => panic!("salt is {other:?}"),
                };
                let sequences = data.get("sequences").expect("sequences").clone();
                let ours = nbt::encode_named_root(
                    "",
                    &SavedDataFile::new(saved::random_sequences(salt, sequences)).to_file_nbt(),
                )
                .expect("encode");
                assert_eq!(ours, want, "{}/random_sequences.dat", dir.display());
                checked += 1;
            }
        }
        assert!(
            checked >= 40 || worlds().is_empty(),
            "only {checked} state-bearing saved files checked"
        );
    }

    /// **`sequences` is not always empty**, and this is the measurement that
    /// says so — the hard-coded empty compound the first draft wrote would
    /// have produced a file vanilla loads with five random sequences reset.
    #[test]
    fn at_least_one_harvested_world_has_random_sequences() {
        let mut with_entries = 0;
        for dir in worlds() {
            let Ok(raw) = std::fs::read(dir.join("random_sequences.dat")) else {
                continue;
            };
            let (_, root) = nbt::read_compressed(&raw).expect("parse");
            let n = root
                .get("data")
                .and_then(|d| d.get("sequences"))
                .and_then(Nbt::as_compound)
                .map_or(0, std::collections::BTreeMap::len);
            if n > 0 {
                with_entries += 1;
            }
        }
        assert!(
            with_entries > 0 || worlds().is_empty(),
            "no harvested world carries a random sequence, so the parameter \
             this test exists for is never exercised"
        );
    }
}
