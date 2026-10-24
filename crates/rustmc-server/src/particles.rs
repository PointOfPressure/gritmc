//! `BuiltInRegistries.PARTICLE_TYPE` — which particle types exist, their
//! registry ids, and which of them carry options.
//!
//! Read from `data/vanilla/particle_types.json`, which
//! `ci/sound-particle-golden.sh` dumps from a bootstrapped registry and checks
//! against the count in Mojang's own `generated/reports/registries.json`. The
//! one column the report does not carry is `simple`: whether the type is a
//! `SimpleParticleType`, whose `STREAM_CODEC` writes nothing after the
//! registry id and whose `codec()` is a unit — so `/particle minecraft:flame`
//! is a whole particle, where `/particle minecraft:dust` needs options.
//!
//! The first reader is `/particle` (`crate::cmdexec`), through the
//! `minecraft:particle` argument parser in `crate::commandtree`.

use std::collections::HashMap;
use std::io;
use std::path::Path;

/// One registered particle type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParticleType {
    /// The registry id, which `ParticleTypes.STREAM_CODEC` writes as a varint.
    pub id: i32,
    /// `instanceof SimpleParticleType`: no options on the wire or in the
    /// command.
    pub simple: bool,
}

/// The table, by namespaced name.
#[derive(Debug, Default)]
pub struct ParticleTypes {
    by_name: HashMap<String, ParticleType>,
}

/// Where `ci/sound-particle-golden.sh` writes the table, relative to the tree.
pub const TABLE_PATH: &str = "data/vanilla/particle_types.json";

impl ParticleTypes {
    /// Parse the dumper's JSON.
    ///
    /// # Errors
    /// If it is not the shape `SoundParticleGolden` writes, or if its `types`
    /// count disagrees with its own table.
    pub fn from_json(doc: &serde_json::Value) -> io::Result<Self> {
        let bad = |m: &str| io::Error::new(io::ErrorKind::InvalidData, m.to_string());
        let rows = doc
            .get("table")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| bad("particle_types.json: no `table`"))?;
        let mut by_name = HashMap::with_capacity(rows.len());
        for row in rows {
            let name = row
                .get("name")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| bad("particle_types.json: a row with no `name`"))?;
            let id = row
                .get("id")
                .and_then(serde_json::Value::as_i64)
                .and_then(|v| i32::try_from(v).ok())
                .ok_or_else(|| bad("particle_types.json: a row with no `id`"))?;
            let simple = row
                .get("simple")
                .and_then(serde_json::Value::as_bool)
                .ok_or_else(|| bad("particle_types.json: a row with no `simple`"))?;
            by_name.insert(name.to_string(), ParticleType { id, simple });
        }
        let declared = doc.get("types").and_then(serde_json::Value::as_u64);
        if declared != Some(by_name.len() as u64) {
            return Err(bad("particle_types.json: `types` disagrees with the table"));
        }
        Ok(Self { by_name })
    }

    /// Load the table from a path.
    ///
    /// # Errors
    /// If the file is unreadable or malformed.
    pub fn load(path: &Path) -> io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let doc: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Self::from_json(&doc)
    }

    /// A type by identifier, as `Identifier.read` resolves one: a name with
    /// no namespace, or an empty one (`:flame`), is `minecraft`'s.
    #[must_use]
    pub fn get(&self, identifier: &str) -> Option<ParticleType> {
        let full = match identifier.split_once(':') {
            None => format!("minecraft:{identifier}"),
            Some(("", path)) => format!("minecraft:{path}"),
            Some(_) => identifier.to_string(),
        };
        self.by_name.get(&full).copied()
    }

    /// How many types the table holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    /// Whether the table is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }
}

/// The process-wide table: the tree's relative path, then the repository's.
/// `None` when neither loads, which a caller turns into "not answered" rather
/// than a guess.
#[must_use]
pub fn shared() -> Option<&'static ParticleTypes> {
    static TABLE: std::sync::OnceLock<Option<ParticleTypes>> = std::sync::OnceLock::new();
    TABLE
        .get_or_init(|| {
            let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(TABLE_PATH);
            ParticleTypes::load(Path::new(TABLE_PATH))
                .or_else(|_| ParticleTypes::load(&repo))
                .ok()
        })
        .as_ref()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> ParticleTypes {
        ParticleTypes::from_json(&serde_json::json!({
            "types": 2,
            "table": [
                {"id": 0, "name": "minecraft:angry_villager", "simple": true},
                {"id": 1, "name": "minecraft:block", "simple": false},
            ]
        }))
        .unwrap()
    }

    #[test]
    fn a_bare_or_empty_namespace_is_minecrafts() {
        let t = table();
        assert_eq!(t.get("angry_villager").map(|p| p.id), Some(0));
        assert_eq!(t.get(":angry_villager").map(|p| p.id), Some(0));
        assert_eq!(t.get("minecraft:block").map(|p| p.simple), Some(false));
        assert_eq!(t.get("other:angry_villager"), None);
    }

    #[test]
    fn a_count_that_disagrees_with_the_table_is_refused() {
        let doc = serde_json::json!({"types": 3, "table": [
            {"id": 0, "name": "minecraft:a", "simple": true}]});
        assert!(ParticleTypes::from_json(&doc).is_err());
    }
}
