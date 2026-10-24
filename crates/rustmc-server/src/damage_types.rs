//! The `minecraft:damage_type` registry and the `DamageTypeTags` memberships
//! the hurt path reads.
//!
//! Reference (26.2), read from the pinned jar: the jar's own
//! `data/minecraft/damage_type/*.json` and
//! `data/minecraft/tags/damage_type/*.json`, plus
//! `net.minecraft.world.damagesource.DamageTypes` and
//! `net.minecraft.tags.DamageTypeTags` for the two lists the *code* declares.
//!
//! # This module is a reader, and the table it reads is `damage_types.json`
//!
//! Damage types are a datapack registry in 26.2, so there is nothing in the
//! bytecode to transcribe: `ci/damage-types-golden.sh` reads the jar's own
//! data files. Measured on the Threadripper, identical across two runs:
//! **51 damage types (51 keys declared in `DamageTypes`), 34 tags, 41 distinct
//! tag-set signatures across the 51 types.**
//!
//! # `DamageTypeTags.BYPASSES_COOLDOWN` has no data file, and so is empty
//!
//! `DamageTypeTags` declares **35** `TagKey`s and the jar ships **34** tag
//! files. The missing one is `minecraft:bypasses_cooldown`, and a `TagKey`
//! with no data file is not an error in vanilla — it is an **empty tag**, so
//! `DamageSource.is(BYPASSES_COOLDOWN)` is false for all 51 damage types.
//!
//! What that means for us is concrete and worth writing down rather than
//! rediscovering: the `&& !source.is(BYPASSES_COOLDOWN)` conjunct at
//! `LivingEntity.hurtServer` offset 196 is **dead in 26.2** — no vanilla
//! damage type skips the invulnerability cooldown. `crate::hurt` transcribes
//! the conjunct anyway, because fidelity is not deferred and a datapack can
//! populate the tag; but a corpus scenario cannot exercise that branch with
//! vanilla content, and a test that claimed to would be measuring its own
//! input. `ci/damage-types-golden.sh` pins the set of file-less tags, so a
//! later version that ships the file reds the generator and somebody reads
//! the branch again.
//!
//! # `exhaustion` and `effects` are optional in the codec
//!
//! A damage type JSON may omit either. The table writes the defaults out
//! (`0.0` and `hurt`) so that a reader never has to carry one, which is the
//! same reason `crate::attributes` records `ranged: false` explicitly rather
//! than leaving the range absent.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::Path;

use crate::health::DamageTags;

/// Where the damage-type extract lives, relative to the server's working
/// directory.
///
/// **This literal is load-bearing outside Rust** — see
/// [`crate::effects::TABLE_PATH`] for why.
pub const TABLE_PATH: &str = "data/vanilla/damage_types.json";

/// One damage type's registry entry.
#[derive(Debug, Clone, PartialEq)]
pub struct DamageType {
    /// `message_id` — the death-message stem.
    pub message_id: String,
    /// `scaling` — `never`, `when_caused_by_living_non_player` or `always`.
    pub scaling: String,
    /// `effects` — the hurt sound family; `hurt` when the field is absent.
    pub effects: String,
    /// `death_message_type`; `default` when the field is absent.
    pub death_message_type: String,
    /// `exhaustion`, as raw IEEE-754 bits decoded. `Player.actuallyHurt` adds
    /// this to the food data on every hit a player takes.
    pub exhaustion: f64,
    /// Every `minecraft:`-prefixed tag this type is in, transitively resolved.
    pub tags: BTreeSet<String>,
}

impl DamageType {
    /// The ten memberships `crate::health` and `crate::hurt` read, gathered
    /// into the struct they take.
    ///
    /// Every field is one `DamageSource.is(DamageTypeTags.X)` call site, and
    /// the tag names are the `DamageTypeTags` constants' own paths.
    #[must_use]
    pub fn tags(&self) -> DamageTags {
        let has = |name: &str| self.tags.contains(&format!("minecraft:{name}"));
        DamageTags {
            bypasses_armor: has("bypasses_armor"),
            bypasses_effects: has("bypasses_effects"),
            bypasses_resistance: has("bypasses_resistance"),
            bypasses_enchantments: has("bypasses_enchantments"),
            // Always false in 26.2 — the tag has no data file. See the module
            // docs; this is read rather than hardcoded so a datapack that
            // populates it is honoured.
            bypasses_cooldown: has("bypasses_cooldown"),
            is_fire: has("is_fire"),
            is_freezing: has("is_freezing"),
            damages_helmet: has("damages_helmet"),
            no_impact: has("no_impact"),
            no_knockback: has("no_knockback"),
        }
    }
}

/// The whole registry.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DamageTypeRegistry {
    by_id: BTreeMap<String, DamageType>,
    tags_with_no_file: BTreeSet<String>,
    tags_declared_in_code: usize,
}

impl DamageTypeRegistry {
    /// Read the table at an explicit path.
    ///
    /// # Errors
    ///
    /// The file being unreadable, not JSON, or not shaped like the table.
    pub fn load(path: &Path) -> io::Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        let doc: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Self::from_json(&doc)
    }

    /// Parse an already-loaded document.
    ///
    /// # Errors
    ///
    /// A missing or wrongly typed field. A row that cannot be read is an
    /// error, never a skipped row.
    pub fn from_json(doc: &serde_json::Value) -> io::Result<Self> {
        let bad = |what: &str| io::Error::new(io::ErrorKind::InvalidData, what.to_string());
        let rows = doc
            .get("table")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| bad("damage_types.json has no `table` array"))?;
        let mut by_id = BTreeMap::new();
        for row in rows {
            let s = |k: &str| -> io::Result<String> {
                row.get(k)
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
                    .ok_or_else(|| bad("a damage type row is missing a string field"))
            };
            let id = s("id")?;
            // The bits, not the printed double — the same argument
            // `crate::attributes` makes.
            let bits = row
                .get("exhaustionBits")
                .and_then(serde_json::Value::as_i64)
                .ok_or_else(|| bad("a damage type row has no `exhaustionBits`"))?;
            #[expect(
                clippy::cast_sign_loss,
                reason = "the generator writes Double.doubleToRawLongBits, whose sign bit is data"
            )]
            let exhaustion = f64::from_bits(bits as u64);
            let tags = row
                .get("tags")
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| bad("a damage type row has no `tags` array"))?
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_string)
                        .ok_or_else(|| bad("a tag entry is not a string"))
                })
                .collect::<io::Result<BTreeSet<String>>>()?;
            by_id.insert(
                id,
                DamageType {
                    message_id: s("messageId")?,
                    scaling: s("scaling")?,
                    effects: s("effects")?,
                    death_message_type: s("deathMessageType")?,
                    exhaustion,
                    tags,
                },
            );
        }
        let tags_with_no_file = doc
            .get("tagsWithNoFile")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| bad("damage_types.json has no `tagsWithNoFile` array"))?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| bad("a file-less tag entry is not a string"))
            })
            .collect::<io::Result<BTreeSet<String>>>()?;
        let tags_declared_in_code = usize::try_from(
            doc.get("tagsDeclaredInCode")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| bad("damage_types.json has no `tagsDeclaredInCode`"))?,
        )
        .map_err(|_| bad("tagsDeclaredInCode does not fit a usize"))?;
        Ok(Self {
            by_id,
            tags_with_no_file,
            tags_declared_in_code,
        })
    }

    /// One damage type, by registry id.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&DamageType> {
        self.by_id.get(id)
    }

    /// The tag struct for a damage type, or `None` for an id the registry does
    /// not carry.
    ///
    /// `None` rather than [`DamageTags::default`]: an unknown damage type
    /// defaulting to "bypasses nothing" would be a plausible-looking answer
    /// for a type that bypasses armour, which is exactly the shape
    /// `crate::attributes` refuses for a missing attribute.
    #[must_use]
    pub fn tags_for(&self, id: &str) -> Option<DamageTags> {
        self.by_id.get(id).map(DamageType::tags)
    }

    /// How many types the table carried.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// Whether the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// Every registry id, sorted.
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.by_id.keys().map(String::as_str)
    }

    /// The tags `DamageTypeTags` declares that have no data file, and are
    /// therefore empty for every type. See the module docs.
    #[must_use]
    pub const fn tags_with_no_file(&self) -> &BTreeSet<String> {
        &self.tags_with_no_file
    }

    /// How many `TagKey`s `DamageTypeTags` declares.
    #[must_use]
    pub const fn tags_declared_in_code(&self) -> usize {
        self.tags_declared_in_code
    }
}

#[cfg(test)]
mod tests {
    use super::DamageTypeRegistry;

    fn doc() -> serde_json::Value {
        serde_json::json!({
            "damageTypes": 2,
            "tagsDeclaredInCode": 35,
            "tagsWithNoFile": ["minecraft:bypasses_cooldown"],
            "table": [
                {"id": "minecraft:starve", "messageId": "starve", "scaling": "never",
                 "exhaustion": 0.0, "exhaustionBits": 0, "effects": "hurt",
                 "deathMessageType": "default",
                 "tags": ["minecraft:bypasses_armor", "minecraft:bypasses_shield"]},
                {"id": "minecraft:in_fire", "messageId": "inFire",
                 "scaling": "when_caused_by_living_non_player",
                 "exhaustion": 0.1, "exhaustionBits": 0.1_f64.to_bits() as i64,
                 "effects": "burning", "deathMessageType": "default",
                 "tags": ["minecraft:is_fire"]}
            ]
        })
    }

    #[test]
    fn the_ten_tag_fields_are_read_off_the_tag_list() {
        let r = DamageTypeRegistry::from_json(&doc()).expect("the fixture parses");
        let starve = r
            .tags_for("minecraft:starve")
            .expect("starve is a damage type");
        assert!(starve.bypasses_armor);
        assert!(!starve.is_fire);
        let fire = r
            .tags_for("minecraft:in_fire")
            .expect("in_fire is a damage type");
        assert!(fire.is_fire);
        assert!(!fire.bypasses_armor);
    }

    #[test]
    fn an_unknown_damage_type_is_none_and_not_a_default() {
        let r = DamageTypeRegistry::from_json(&doc()).expect("the fixture parses");
        assert!(r.tags_for("minecraft:not_a_damage_type").is_none());
    }

    #[test]
    fn exhaustion_comes_from_the_bits_and_not_the_printed_double() {
        let mut d = doc();
        // Put a value in the readable field that disagrees with the bits. The
        // bits must win, or the reader is taking a shortest-repr round trip.
        d["table"][1]["exhaustion"] = serde_json::json!(999.0);
        let r = DamageTypeRegistry::from_json(&d).expect("the fixture parses");
        let fire = r.get("minecraft:in_fire").expect("in_fire");
        assert!(
            (fire.exhaustion - 0.1).abs() < f64::EPSILON,
            "{}",
            fire.exhaustion
        );
    }

    #[test]
    fn a_row_missing_a_field_is_an_error_and_not_a_dropped_row() {
        let mut d = doc();
        d["table"][0]
            .as_object_mut()
            .expect("an object")
            .remove("scaling");
        let err = DamageTypeRegistry::from_json(&d).expect_err("a truncated row must not load");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn the_file_less_tag_list_is_carried_rather_than_inferred() {
        let r = DamageTypeRegistry::from_json(&doc()).expect("the fixture parses");
        assert_eq!(r.tags_declared_in_code(), 35);
        assert!(r
            .tags_with_no_file()
            .contains("minecraft:bypasses_cooldown"));
    }
}
