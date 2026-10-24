//! The `minecraft:potion` registry, as data.
//!
//! Reference (26.2), read from the pinned jar with `javap -p -c -constants`:
//! `net.minecraft.world.item.alchemy.Potion` (`getEffects`, `name`,
//! `hasInstantEffects`, `STREAM_CODEC`) and
//! `net.minecraft.world.item.alchemy.PotionContents`.
//!
//! # This module is a reader, not a port
//!
//! `Potion` has one field and three methods and none of them branches, so
//! there is nothing here to transcribe: every potion in the game is a
//! constructor call in `Potions.<clinit>` carrying a duration and an
//! amplifier. Those 46 rows live in `data/vanilla/potions.json`, produced by
//! `ci/potions-golden.sh`, and this module hands them to whatever applies
//! them.
//!
//! Measured on the Threadripper, identical across two runs: **46 potions, 4
//! with instant effects, 4 with none, 45 effect instances in total.**
//! `docs/SERVER.md`'s assessment census says "potions 46 (7 named)", and the
//! 46 agrees; the 7 was a count of how many the *source* mentioned by name.
//!
//! # `name()` is not the registry path
//!
//! `swiftness`, `long_swiftness` and `strong_swiftness` are three registry
//! entries whose `name()` is `swiftness` for all three — it is the translation
//! stem, and it is what makes the three strengths share a display name. A
//! reader keyed on `name()` would collapse them, which is why
//! [`Potions::get`] takes the registry id.
//!
//! # What a potion does is `crate::effects`' job
//!
//! Drinking applies each of these instances through
//! `MobEffectInstance.update` against whatever the drinker already holds, so
//! the durations here are the input to the hidden-effect chain
//! [`crate::effects::MobEffectInstance::update`] transcribes.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

/// Where the potion registry extract lives, relative to the server's working
/// directory.
///
/// **This literal is load-bearing outside Rust** — see
/// [`crate::effects::TABLE_PATH`] for why.
pub const TABLE_PATH: &str = "data/vanilla/potions.json";

/// One `MobEffectInstance` as a potion carries it.
///
/// The three flags are the ones `MobEffectInstance`'s five-argument
/// constructor sets; they are carried rather than defaulted because
/// `crate::effects::MobEffectInstance::update` reads all three and a potion
/// that arrived with the wrong `ambient` would merge differently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PotionEffect {
    /// The effect's registry id, e.g. `minecraft:speed`.
    pub effect: String,
    /// Ticks, or `-1` for `MobEffectInstance.INFINITE_DURATION`.
    pub duration: i32,
    /// 0 is the roman-numeral I.
    pub amplifier: i32,
    /// `isAmbient()`.
    pub ambient: bool,
    /// `isVisible()`.
    pub visible: bool,
    /// `showIcon()`.
    pub show_icon: bool,
}

/// One row of the potion registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Potion {
    /// `BuiltInRegistries.POTION.getId(potion)` — the number
    /// `Potion.STREAM_CODEC` puts on the wire.
    pub registry_id: i32,
    /// `Potion.name()`, the translation stem. Shared across strengths.
    pub name: String,
    /// `Potion.hasInstantEffects()`.
    pub has_instant_effects: bool,
    /// `Potion.getEffects()`, in the registry's own order.
    pub effects: Vec<PotionEffect>,
}

/// The whole `minecraft:potion` registry.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Potions {
    by_id: BTreeMap<String, Potion>,
}

impl Potions {
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
    /// error rather than a skipped row: half a potion registry that reports
    /// itself as whole is the shape this project keeps having to remove.
    pub fn from_json(doc: &serde_json::Value) -> io::Result<Self> {
        let bad = |what: &str| io::Error::new(io::ErrorKind::InvalidData, what.to_string());
        let rows = doc
            .get("table")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| bad("potions.json has no `table` array"))?;
        let mut by_id = BTreeMap::new();
        for row in rows {
            let id = row
                .get("id")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| bad("a potion row has no `id`"))?;
            let registry_id = row
                .get("registryId")
                .and_then(serde_json::Value::as_i64)
                .ok_or_else(|| bad("a potion row has no `registryId`"))?;
            let name = row
                .get("name")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| bad("a potion row has no `name`"))?;
            let has_instant_effects = row
                .get("hasInstantEffects")
                .and_then(serde_json::Value::as_bool)
                .ok_or_else(|| bad("a potion row has no `hasInstantEffects`"))?;
            let mut effects = Vec::new();
            for e in row
                .get("effects")
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| bad("a potion row has no `effects` array"))?
            {
                let field_i = |k: &str| -> io::Result<i32> {
                    e.get(k)
                        .and_then(serde_json::Value::as_i64)
                        .and_then(|v| i32::try_from(v).ok())
                        .ok_or_else(|| bad("an effect instance is missing an int field"))
                };
                let field_b = |k: &str| -> io::Result<bool> {
                    e.get(k)
                        .and_then(serde_json::Value::as_bool)
                        .ok_or_else(|| bad("an effect instance is missing a bool field"))
                };
                effects.push(PotionEffect {
                    effect: e
                        .get("effect")
                        .and_then(serde_json::Value::as_str)
                        .ok_or_else(|| bad("an effect instance has no `effect`"))?
                        .to_string(),
                    duration: field_i("duration")?,
                    amplifier: field_i("amplifier")?,
                    ambient: field_b("ambient")?,
                    visible: field_b("visible")?,
                    show_icon: field_b("showIcon")?,
                });
            }
            by_id.insert(
                id.to_string(),
                Potion {
                    registry_id: i32::try_from(registry_id)
                        .map_err(|_| bad("a potion registryId does not fit an i32"))?,
                    name: name.to_string(),
                    has_instant_effects,
                    effects,
                },
            );
        }
        Ok(Self { by_id })
    }

    /// One potion, by registry id — **not** by `name()`, which three
    /// strengths of the same potion share.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Potion> {
        self.by_id.get(id)
    }

    /// How many rows the table carried.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// Whether the table is empty. A loaded table never is; this exists so a
    /// caller can tell "nothing loaded" from "nothing matched".
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// Every registry id, in sorted order.
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.by_id.keys().map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::Potions;

    fn doc() -> serde_json::Value {
        serde_json::json!({
            "potions": 3,
            "table": [
                {"id": "minecraft:water", "registryId": 0, "name": "water",
                 "hasInstantEffects": false, "effects": []},
                {"id": "minecraft:swiftness", "registryId": 5, "name": "swiftness",
                 "hasInstantEffects": false, "effects": [
                    {"effect": "minecraft:speed", "duration": 3600, "amplifier": 0,
                     "ambient": false, "visible": true, "showIcon": true}
                 ]},
                {"id": "minecraft:strong_swiftness", "registryId": 7, "name": "swiftness",
                 "hasInstantEffects": false, "effects": [
                    {"effect": "minecraft:speed", "duration": 1800, "amplifier": 1,
                     "ambient": false, "visible": true, "showIcon": true}
                 ]}
            ]
        })
    }

    #[test]
    fn a_potion_with_no_effects_is_a_row_and_not_a_parse_failure() {
        let p = Potions::from_json(&doc()).expect("the fixture parses");
        let water = p.get("minecraft:water").expect("water is a potion");
        assert!(water.effects.is_empty());
        assert_eq!(water.registry_id, 0);
    }

    #[test]
    fn the_two_strengths_share_a_name_and_differ_in_everything_else() {
        let p = Potions::from_json(&doc()).expect("the fixture parses");
        let weak = p.get("minecraft:swiftness").expect("swiftness");
        let strong = p
            .get("minecraft:strong_swiftness")
            .expect("strong swiftness");
        assert_eq!(weak.name, strong.name);
        assert_ne!(weak.registry_id, strong.registry_id);
        assert_ne!(weak.effects[0].duration, strong.effects[0].duration);
        assert_ne!(weak.effects[0].amplifier, strong.effects[0].amplifier);
    }

    #[test]
    fn a_row_missing_a_field_is_an_error_and_not_a_dropped_row() {
        let mut d = doc();
        d["table"][1]["effects"][0]
            .as_object_mut()
            .expect("an object")
            .remove("amplifier");
        let err = Potions::from_json(&d).expect_err("a truncated row must not load");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn a_table_with_no_array_is_an_error() {
        let err = Potions::from_json(&serde_json::json!({"potions": 46}))
            .expect_err("a document with no table must not load");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }
}
