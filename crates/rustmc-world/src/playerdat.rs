//! `players/data/<uuid>.dat` — one player's saved state.
//!
//! # The path, first, because it changed
//!
//! `LevelResource` calls it **`players/data`**. Every world before 26 used
//! `playerdata/`, which is what a reader working from memory writes and what
//! the pre-26 half of the internet still documents; the constant pool of
//! `LevelResource` in the pinned 26.2 jar holds `players`, `players/data`,
//! `players/stats` and `players/advancements`, and a real 26.2 world on this
//! box has exactly those directories.
//!
//! # The file
//!
//! `PlayerDataStorage.save` builds the tag with `Player.saveWithoutId`, writes
//! it through `NbtIo.writeCompressed` to a temp file in the same directory,
//! and then `Util.safeReplaceFile`s it into `<uuid>.dat` — leaving the
//! previous one at `<uuid>.dat_old`. So the file is gzip around an **unnamed
//! root compound that is the player itself**: no `Data` wrapper, unlike
//! `level.dat`, and `DataVersion` sits among the player's own keys rather than
//! outside them.
//!
//! # Why unknown keys are kept
//!
//! A real 26.2 player carries 48 keys, and this server has state for about a
//! third of them: it has no advancements, no recipe book, no warden spawn
//! tracker, no brain memories. A writer that emitted only what it models would
//! **delete** the rest on the first save — a player who had played on vanilla
//! and then joined this server would come back to vanilla with their recipe
//! book wiped. So [`PlayerData`] carries an `extra` map of every key it does
//! not model, verbatim, and writes it back out.
//!
//! That is also what makes the golden below possible: a real vanilla
//! `<uuid>.dat` decoded and re-encoded here is byte-identical to the original
//! payload, over all 48 keys, which no partial model could be.

use crate::nbt::{self, Nbt};
use std::collections::BTreeMap;
use std::io;

/// The default air supply, `Entity.getMaxAirSupply` — 300 ticks.
pub const MAX_AIR: i16 = 300;

/// One player's saved state.
///
/// Field types are the ones the file actually uses and several are not the
/// obvious guess: `Rotation` is a list of **floats** while `Pos` and `Motion`
/// are lists of **doubles**; `fall_distance` is a `Double` in 26.2 where it
/// was a `Float` for a decade; `Air` and `Fire` are `Short`; `Health`,
/// `XpP`, `AbsorptionAmount` and the food floats are `Float`. Getting one of
/// those wrong writes a file vanilla misreads rather than rejects.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerData {
    /// `UUID` — four ints, most-significant first, the form
    /// `UUIDUtil.uuidToIntArray` produces.
    pub uuid: [i32; 4],
    /// `Pos` — three doubles.
    pub pos: [f64; 3],
    /// `Motion` — three doubles.
    pub motion: [f64; 3],
    /// `Rotation` — yaw then pitch, two floats.
    pub yaw: f32,
    pub pitch: f32,
    /// `OnGround`.
    pub on_ground: bool,
    /// `fall_distance` — a `Double` since 26.x.
    pub fall_distance: f64,
    /// `Air` — ticks of breath left. See [`MAX_AIR`].
    pub air: i16,
    /// `Fire` — negative is the immunity countdown, not burning.
    pub fire: i16,
    /// `Dimension` — a dimension id, `minecraft:overworld` and the like.
    pub dimension: String,
    /// `playerGameType`.
    pub game_type: i32,
    /// `Health`.
    pub health: f32,
    /// `foodLevel`, `foodSaturationLevel`, `foodExhaustionLevel`,
    /// `foodTickTimer` — `FoodData`'s four saved fields, written flat into the
    /// player rather than into a compound of their own.
    pub food_level: i32,
    pub food_saturation: f32,
    pub food_exhaustion: f32,
    pub food_tick_timer: i32,
    /// `SelectedItemSlot` — the hotbar slot in hand.
    pub selected_item_slot: i32,
    /// `Inventory` — one compound per occupied slot,
    /// `{id: String, count: Int, Slot: Byte}` plus components. Kept as raw
    /// NBT: this crate is the save format and has no business knowing what an
    /// item component is.
    pub inventory: Vec<Nbt>,
    /// `EnderItems` — the ender chest, same shape.
    pub ender_items: Vec<Nbt>,
    /// `Score`, `XpLevel`, `XpP`, `XpTotal`, `XpSeed`.
    pub score: i32,
    pub xp_level: i32,
    pub xp_progress: f32,
    pub xp_total: i32,
    pub xp_seed: i32,
    /// `DataVersion` — sits among the player's own keys here, not in an
    /// envelope around them.
    pub data_version: i32,
    /// Every key this does not model, verbatim. See the module doc.
    pub extra: BTreeMap<String, Nbt>,
}

/// Keys [`PlayerData`] has a field for. Everything else goes to `extra`.
///
/// A list rather than a match arm per key so the two directions cannot
/// disagree: `from_nbt` moves exactly these out of the map and `to_nbt` puts
/// exactly these back, and a key added to the struct without being added here
/// would be written twice.
const MODELLED: [&str; 24] = [
    "UUID",
    "Pos",
    "Motion",
    "Rotation",
    "OnGround",
    "fall_distance",
    "Air",
    "Fire",
    "Dimension",
    "playerGameType",
    "Health",
    "foodLevel",
    "foodSaturationLevel",
    "foodExhaustionLevel",
    "foodTickTimer",
    "SelectedItemSlot",
    "Inventory",
    "EnderItems",
    "Score",
    "XpLevel",
    "XpP",
    "XpTotal",
    "XpSeed",
    "DataVersion",
];

impl PlayerData {
    /// A player who has never played: full health, full food, nothing in
    /// hand, at `pos`.
    ///
    /// `extra` is empty, which means a file written from this carries 24 keys
    /// where vanilla writes 48. That is a **smaller** file vanilla loads
    /// happily — every absent key falls back to its default — and it is
    /// honest: this server has no advancements to record. It is not
    /// byte-identical to vanilla's own first save and no check here claims it
    /// is.
    #[must_use]
    pub fn fresh(uuid: u128, pos: [f64; 3], game_type: i32, data_version: i32) -> Self {
        Self {
            uuid: uuid_to_ints(uuid),
            pos,
            motion: [0.0; 3],
            yaw: 0.0,
            pitch: 0.0,
            on_ground: true,
            fall_distance: 0.0,
            air: MAX_AIR,
            fire: -20,
            dimension: "minecraft:overworld".into(),
            game_type,
            health: 20.0,
            food_level: 20,
            food_saturation: 5.0,
            food_exhaustion: 0.0,
            food_tick_timer: 0,
            selected_item_slot: 0,
            inventory: Vec::new(),
            ender_items: Vec::new(),
            score: 0,
            xp_level: 0,
            xp_progress: 0.0,
            xp_total: 0,
            xp_seed: 0,
            data_version,
            extra: BTreeMap::new(),
        }
    }

    /// The root compound the file holds.
    #[must_use]
    pub fn to_nbt(&self) -> Nbt {
        let mut m = self.extra.clone();
        let doubles = |v: &[f64]| Nbt::List(v.iter().map(|d| Nbt::Double(*d)).collect());
        m.insert("UUID".into(), Nbt::IntArray(self.uuid.to_vec()));
        m.insert("Pos".into(), doubles(&self.pos));
        m.insert("Motion".into(), doubles(&self.motion));
        m.insert(
            "Rotation".into(),
            Nbt::List(vec![Nbt::Float(self.yaw), Nbt::Float(self.pitch)]),
        );
        m.insert("OnGround".into(), Nbt::Byte(i8::from(self.on_ground)));
        m.insert("fall_distance".into(), Nbt::Double(self.fall_distance));
        m.insert("Air".into(), Nbt::Short(self.air));
        m.insert("Fire".into(), Nbt::Short(self.fire));
        m.insert("Dimension".into(), Nbt::String(self.dimension.clone()));
        m.insert("playerGameType".into(), Nbt::Int(self.game_type));
        m.insert("Health".into(), Nbt::Float(self.health));
        m.insert("foodLevel".into(), Nbt::Int(self.food_level));
        m.insert(
            "foodSaturationLevel".into(),
            Nbt::Float(self.food_saturation),
        );
        m.insert(
            "foodExhaustionLevel".into(),
            Nbt::Float(self.food_exhaustion),
        );
        m.insert("foodTickTimer".into(), Nbt::Int(self.food_tick_timer));
        m.insert("SelectedItemSlot".into(), Nbt::Int(self.selected_item_slot));
        m.insert("Inventory".into(), Nbt::List(self.inventory.clone()));
        m.insert("EnderItems".into(), Nbt::List(self.ender_items.clone()));
        m.insert("Score".into(), Nbt::Int(self.score));
        m.insert("XpLevel".into(), Nbt::Int(self.xp_level));
        m.insert("XpP".into(), Nbt::Float(self.xp_progress));
        m.insert("XpTotal".into(), Nbt::Int(self.xp_total));
        m.insert("XpSeed".into(), Nbt::Int(self.xp_seed));
        m.insert("DataVersion".into(), Nbt::Int(self.data_version));
        Nbt::Compound(m)
    }

    /// Parse a player root compound.
    ///
    /// Missing keys take the value [`Self::fresh`] would give them rather than
    /// erroring: vanilla's own reader is defaulting, a datapack or a version
    /// change legitimately drops keys, and a player who cannot log in because
    /// their file has no `XpSeed` is a worse outcome than one who logs in with
    /// a zero. A key of the **wrong type** is an error, because that is a
    /// writer defect rather than an absence.
    ///
    /// # Errors
    /// If the root is not a compound, or a modelled key has the wrong tag.
    pub fn from_nbt(root: &Nbt) -> io::Result<Self> {
        let map = root
            .as_compound()
            .ok_or_else(|| io::Error::other("player data root is not a compound"))?;
        let wrong = |k: &str| io::Error::other(format!("player data: `{k}` has the wrong type"));
        macro_rules! scalar {
            ($key:literal, $variant:path, $default:expr) => {
                match map.get($key) {
                    None => $default,
                    Some($variant(v)) => *v,
                    Some(_) => return Err(wrong($key)),
                }
            };
        }
        let list_of = |key: &str| -> io::Result<Vec<Nbt>> {
            match map.get(key) {
                None => Ok(Vec::new()),
                Some(Nbt::List(items)) => Ok(items.clone()),
                Some(_) => Err(wrong(key)),
            }
        };
        let triple = |key: &str, default: [f64; 3]| -> io::Result<[f64; 3]> {
            let Some(tag) = map.get(key) else {
                return Ok(default);
            };
            let Some(items) = tag.as_list() else {
                return Err(wrong(key));
            };
            if items.len() != 3 {
                return Err(wrong(key));
            }
            let mut out = [0.0; 3];
            for (slot, item) in out.iter_mut().zip(items) {
                let Nbt::Double(v) = item else {
                    return Err(wrong(key));
                };
                *slot = *v;
            }
            Ok(out)
        };
        let (yaw, pitch) = match map.get("Rotation") {
            None => (0.0, 0.0),
            Some(Nbt::List(items)) => match items.as_slice() {
                [Nbt::Float(y), Nbt::Float(p)] => (*y, *p),
                _ => return Err(wrong("Rotation")),
            },
            Some(_) => return Err(wrong("Rotation")),
        };
        let uuid = match map.get("UUID") {
            None => [0; 4],
            Some(Nbt::IntArray(v)) if v.len() == 4 => [v[0], v[1], v[2], v[3]],
            Some(_) => return Err(wrong("UUID")),
        };
        let mut extra = map.clone();
        for key in MODELLED {
            extra.remove(key);
        }
        Ok(Self {
            uuid,
            pos: triple("Pos", [0.0, 0.0, 0.0])?,
            motion: triple("Motion", [0.0, 0.0, 0.0])?,
            yaw,
            pitch,
            on_ground: scalar!("OnGround", Nbt::Byte, 0) != 0,
            fall_distance: scalar!("fall_distance", Nbt::Double, 0.0),
            air: scalar!("Air", Nbt::Short, MAX_AIR),
            fire: scalar!("Fire", Nbt::Short, -20),
            dimension: match map.get("Dimension") {
                None => "minecraft:overworld".to_string(),
                Some(Nbt::String(s)) => s.clone(),
                Some(_) => return Err(wrong("Dimension")),
            },
            game_type: scalar!("playerGameType", Nbt::Int, 0),
            health: scalar!("Health", Nbt::Float, 20.0),
            food_level: scalar!("foodLevel", Nbt::Int, 20),
            food_saturation: scalar!("foodSaturationLevel", Nbt::Float, 5.0),
            food_exhaustion: scalar!("foodExhaustionLevel", Nbt::Float, 0.0),
            food_tick_timer: scalar!("foodTickTimer", Nbt::Int, 0),
            selected_item_slot: scalar!("SelectedItemSlot", Nbt::Int, 0),
            inventory: list_of("Inventory")?,
            ender_items: list_of("EnderItems")?,
            score: scalar!("Score", Nbt::Int, 0),
            xp_level: scalar!("XpLevel", Nbt::Int, 0),
            xp_progress: scalar!("XpP", Nbt::Float, 0.0),
            xp_total: scalar!("XpTotal", Nbt::Int, 0),
            xp_seed: scalar!("XpSeed", Nbt::Int, 0),
            data_version: scalar!("DataVersion", Nbt::Int, 0),
            extra,
        })
    }
}

/// `UUIDUtil.uuidToIntArray` — the 128 bits as four big-endian ints, most
/// significant first.
#[must_use]
pub const fn uuid_to_ints(uuid: u128) -> [i32; 4] {
    [
        (uuid >> 96) as i32,
        (uuid >> 64) as i32,
        (uuid >> 32) as i32,
        uuid as i32,
    ]
}

/// The inverse.
#[must_use]
pub const fn ints_to_uuid(v: [i32; 4]) -> u128 {
    ((v[0] as u32 as u128) << 96)
        | ((v[1] as u32 as u128) << 64)
        | ((v[2] as u32 as u128) << 32)
        | (v[3] as u32 as u128)
}

/// `<uuid>.dat`, in the hyphenated form `UUID.toString` produces — which is
/// what `PlayerDataStorage` names the file with, so a file named any other way
/// is a file vanilla will not find.
#[must_use]
pub fn file_name(uuid: u128) -> String {
    let h = format!("{uuid:032x}");
    format!(
        "{}-{}-{}-{}-{}.dat",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

/// Read `<dir>/<uuid>.dat`, if it is there.
///
/// # Errors
/// If the file exists and cannot be read or parsed.
pub fn read(dir: &std::path::Path, uuid: u128) -> io::Result<Option<PlayerData>> {
    let path = dir.join(file_name(uuid));
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = std::fs::read(&path)?;
    let (_, root) = nbt::read_compressed(&bytes)?;
    PlayerData::from_nbt(&root).map(Some)
}

/// Write `<dir>/<uuid>.dat`, rotating the previous one to `<uuid>.dat_old`
/// exactly as `Util.safeReplaceFile` does.
///
/// # Errors
/// If the directory cannot be created or the file cannot be written.
pub fn write(dir: &std::path::Path, player: &PlayerData) -> io::Result<()> {
    let path = dir.join(file_name(ints_to_uuid(player.uuid)));
    crate::level::write_dat_safely(&path, "", &player.to_nbt())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real vanilla 26.2 player files. `ci/gen-gameplay-tables.sh` boots a
    /// vanilla server with a joining client to dump recipes, and that boot
    /// leaves one behind at `oracle-work/recipe-dump/world/players/data`;
    /// worlds harvested by `ci/gen-vanilla-region.sh` have an empty
    /// `players/data` because nothing ever joined them.
    ///
    /// # The filter, and why it is read out of `level.dat` rather than guessed
    ///
    /// **`oracle-work` also holds worlds THIS SERVER wrote**, and it did not
    /// until `ci/t2-player-survives-a-restart.sh` was added on the same day as
    /// this test. Its player files carry the 24 keys this struct models and no
    /// others, so feeding them to a golden about *vanilla's* 48 is comparing
    /// our own output to itself — the exact instrument defect this repo is
    /// named for. It was caught by the `extra.len() >= 10` assertion at the
    /// bottom of the golden rather than by anyone noticing.
    ///
    /// The discriminator is `level.dat`'s `ServerBrands`, which
    /// `PrimaryLevelData` maintains as the set of brands that have ever run
    /// the world: a world this server has opened carries `rustmc` there, and
    /// that is a property of the world rather than of the file being compared,
    /// so the filter cannot select for the answer.
    fn vanilla_player_files() -> Vec<std::path::PathBuf> {
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../oracle-work"));
        let mut out = Vec::new();
        let Ok(entries) = std::fs::read_dir(root) else {
            return out;
        };
        for entry in entries.flatten() {
            let world = entry.path().join("world");
            if !written_only_by_vanilla(&world) {
                continue;
            }
            let Ok(files) = std::fs::read_dir(world.join("players/data")) else {
                continue;
            };
            for file in files.flatten() {
                let path = file.path();
                if path.extension().is_some_and(|e| e == "dat") {
                    out.push(path);
                }
            }
        }
        out.sort();
        out
    }

    /// Whether `level.dat` says no brand but vanilla's has ever opened this
    /// world. A world with no readable `level.dat` is **not** vanilla's for
    /// this purpose: it is unknown, and an unknown is not evidence.
    fn written_only_by_vanilla(world: &std::path::Path) -> bool {
        let Ok(root) = crate::level::read_dat(&world.join("level.dat")) else {
            return false;
        };
        let Ok(level) = crate::level::LevelData::from_file_nbt(&root) else {
            return false;
        };
        !level.server_brands.iter().any(|b| b.contains("rustmc"))
    }

    /// **A real vanilla player file, decoded and re-encoded, is byte-identical
    /// to the original payload.**
    ///
    /// 48 keys, of which this models 24, so the other 24 pass through `extra`.
    /// It goes red on a wrong tag width (`Health` as a Double is four bytes
    /// out), on a dropped key, and on a wrong `java.util.HashMap` iteration
    /// order — 48 keys put the table at 128 buckets, a width nothing else in
    /// this repo exercises.
    ///
    /// Gated through `rustmc_data::fixture` so a tree without the dump fails
    /// rather than silently passing over zero files.
    #[test]
    fn a_real_vanilla_player_file_re_encodes_byte_for_byte() {
        let files = vanilla_player_files();
        let probe = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../oracle-work/recipe-dump/world/players/data"
        ));
        if !rustmc_data::fixture::available(
            probe,
            "a vanilla player .dat",
            "ci/gen-gameplay-tables.sh",
        ) {
            return;
        }
        assert!(!files.is_empty(), "no vanilla player files under {probe:?}");
        // The control for the filter. It can only be taken when a world this
        // server wrote is present — which it is exactly when
        // `ci/t2-player-survives-a-restart.sh` has run in this tree — so it is
        // conditional on that rather than unconditional. Without it, a filter
        // that excluded *everything but* the vanilla ones by accident would be
        // indistinguishable from one that worked.
        let ours: Vec<_> = std::fs::read_dir(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../oracle-work"
        )))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path().join("world"))
        .filter(|w| w.join("players/data").is_dir() && !written_only_by_vanilla(w))
        .collect();
        for world in &ours {
            assert!(
                !files.iter().any(|f| f.starts_with(world)),
                "{}: a world this server wrote is in the vanilla population",
                world.display()
            );
        }
        for path in files {
            let raw = std::fs::read(&path).expect("read");
            let (name, root) = nbt::read_compressed(&raw).expect("gunzip + parse");
            let original = nbt::encode_named_root(&name, &root).expect("re-encode as read");
            let player =
                PlayerData::from_nbt(&root).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let ours = nbt::encode_named_root(&name, &player.to_nbt()).expect("encode ours");
            assert_eq!(
                ours.len(),
                original.len(),
                "{}: {} bytes, ours {}",
                path.display(),
                original.len(),
                ours.len()
            );
            assert_eq!(ours, original, "{}", path.display());
            // The pass-through is the whole design; assert it carried
            // something, or this test would also pass on a file with 24 keys.
            assert!(
                player.extra.len() >= 10,
                "{}: only {} unmodelled keys passed through",
                path.display(),
                player.extra.len()
            );
        }
    }

    /// The uuid round trip and the file name, against the real one on disk:
    /// `dddb86e0-ce5d-37b5-8848-5d92851308d1.dat` is the offline-mode uuid of
    /// the harness's own client.
    #[test]
    fn the_file_name_is_the_hyphenated_uuid() {
        let uuid = 0xdddb_86e0_ce5d_37b5_8848_5d92_8513_08d1_u128;
        assert_eq!(file_name(uuid), "dddb86e0-ce5d-37b5-8848-5d92851308d1.dat");
        assert_eq!(ints_to_uuid(uuid_to_ints(uuid)), uuid);
    }

    /// A fresh player round-trips, and a wrong tag is refused rather than
    /// defaulted — the distinction `from_nbt` documents.
    #[test]
    fn a_fresh_player_round_trips_and_a_mistyped_key_is_refused() {
        let player = PlayerData::fresh(1, [8.5, 64.0, 8.5], 1, 4903);
        let back = PlayerData::from_nbt(&player.to_nbt()).expect("decode");
        assert_eq!(player, back);

        let Nbt::Compound(mut m) = player.to_nbt() else {
            panic!("compound")
        };
        // `Health` as a Double: the shape a writer that used the wrong
        // constructor produces, and the one vanilla misreads rather than
        // rejects.
        m.insert("Health".into(), Nbt::Double(20.0));
        let err = PlayerData::from_nbt(&Nbt::Compound(m)).expect_err("must refuse");
        assert!(err.to_string().contains("Health"), "{err}");
    }

    /// An unmodelled key survives a save/load cycle. Without this a player who
    /// had played on vanilla would lose their recipe book the first time they
    /// joined here.
    #[test]
    fn an_unmodelled_key_survives_a_round_trip() {
        let mut player = PlayerData::fresh(7, [0.0, 0.0, 0.0], 0, 4903);
        player.extra.insert("recipeBook".into(), Nbt::Byte(1));
        let dir = std::env::temp_dir().join(format!("rustmc-playerdat-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");
        write(&dir, &player).expect("write");
        let back = read(&dir, 7).expect("read").expect("present");
        assert_eq!(back.extra.get("recipeBook"), Some(&Nbt::Byte(1)));
        assert_eq!(back, player);
        assert_eq!(read(&dir, 8).expect("absent"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
