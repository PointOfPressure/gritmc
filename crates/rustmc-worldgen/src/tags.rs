//! Block tags, resolved from datapack JSON.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `net.minecraft.tags.TagLoader`, `net.minecraft.tags.TagEntry`.
//!
//! Carvers name what they may replace as a tag —
//! `#minecraft:overworld_carver_replaceables` — and that tag names six more
//! tags, which name more. Resolving it is a graph walk, not a lookup, and
//! getting the membership wrong changes which blocks a cave eats through:
//! too small and caves stop at gravel, too large and they eat the bedrock
//! floor.
//!
//! # Membership only
//!
//! Vanilla tags are ordered — `#minecraft:planks` is iterated by loot tables
//! and recipes, and the order is observable there. Nothing in worldgen reads
//! a tag as a list; carvers and features only ask "is this block in it". So
//! this resolves to a set, and the set is sorted so that iterating it is
//! deterministic anyway (CLAUDE.md: no `HashMap` iteration at
//! behaviour-visible sites). If an ordered reader ever appears, this needs to
//! preserve the datapack's order instead — stated here so that change is a
//! decision rather than a surprise.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Every block tag in a datapack directory, resolved.
///
/// # Why the map is behind an `Arc`
///
/// It is immutable after construction, and it is **cloned in the decoration
/// hot path**: `PlacementModifier::EnvironmentScan` and
/// `BlockPredicateFilter` both do `world.block_tags().clone()`, not because
/// they want a copy but because `predicate.test` needs `&mut world` and
/// `&tags` at the same time and the tags come out of the world. Deep-copying
/// a `BTreeMap<String, BTreeSet<String>>` of every block tag in the game, per
/// placement attempt, cost **6.6% of a whole pipeline** — 3.24% cloning and
/// 3.40% dropping, measured with `perf` on the Threadripper.
///
/// Behind an `Arc` the same `.clone()` is a refcount bump. No call site
/// changes and no behaviour changes: the map was already never written after
/// `from_entries` returned, so sharing it is the same map by a cheaper route.
#[derive(Clone, Debug, Default)]
pub struct BlockTags {
    resolved: std::sync::Arc<BTreeMap<String, BTreeSet<String>>>,
}

impl BlockTags {
    /// Load and resolve every `*.json` in a `tags/block` directory.
    ///
    /// # Errors
    /// If the directory cannot be read, a file is not JSON, a tag reference
    /// names a tag that does not exist, or the references form a cycle.
    pub fn load(dir: &Path) -> Result<Self, String> {
        let mut raw: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let entries =
            std::fs::read_dir(dir).map_err(|e| format!("{} unreadable: {e}", dir.display()))?;
        for entry in entries {
            let path = entry.map_err(|e| format!("dir entry: {e}"))?.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| format!("{} has a non-utf8 name", path.display()))?;
            let text = std::fs::read_to_string(&path)
                .map_err(|e| format!("{} unreadable: {e}", path.display()))?;
            let json: serde_json::Value = serde_json::from_str(&text)
                .map_err(|e| format!("{} is not JSON: {e}", path.display()))?;
            let values = json
                .get("values")
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| format!("{} has no `values` array", path.display()))?;
            let mut names = Vec::with_capacity(values.len());
            for value in values {
                // An entry is either a bare string or `{"id": ..,
                // "required": false}`. The `required` flag only decides
                // whether a *missing* target is an error; it does not change
                // membership when the target exists.
                let id = match value {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Object(_) => value
                        .get("id")
                        .and_then(serde_json::Value::as_str)
                        .ok_or_else(|| format!("{} has an entry with no `id`", path.display()))?
                        .to_string(),
                    other => return Err(format!("{}: bad tag entry {other}", path.display())),
                };
                names.push(id);
            }
            raw.insert(format!("minecraft:{stem}"), names);
        }

        Self::from_entries(raw)
    }

    /// Resolve tags already in memory.
    ///
    /// The same resolution the loader does, without the filesystem. Exists
    /// because a datapack is not the only place tags can come from — and
    /// because tests that each write their own directory race each other for
    /// no reason.
    ///
    /// # Errors
    /// If a reference is dangling or the references form a cycle.
    pub fn from_entries(raw: BTreeMap<String, Vec<String>>) -> Result<Self, String> {
        let mut resolved_map: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let keys: Vec<String> = raw.keys().cloned().collect();
        for name in keys {
            let mut visiting = Vec::new();
            let resolved = resolve(&name, &raw, &mut visiting)?;
            resolved_map.insert(name, resolved);
        }
        Ok(Self {
            resolved: std::sync::Arc::new(resolved_map),
        })
    }

    /// Is `block` in `tag`?
    ///
    /// `tag` is named with its leading `#`, as a datapack writes it, so a
    /// caller can pass a `replaceable` field through unchanged rather than
    /// stripping it and getting it wrong in one of two places.
    ///
    /// # Panics
    /// If the tag does not exist. A missing tag silently answering "no"
    /// would make a carver replace nothing, which reads as "caves are off"
    /// rather than as an error.
    #[must_use]
    pub fn contains(&self, tag: &str, block: &str) -> bool {
        let key = tag.strip_prefix('#').unwrap_or(tag);
        self.resolved
            .get(key)
            .unwrap_or_else(|| panic!("no such block tag: {tag}"))
            .contains(block)
    }

    /// The resolved members of a tag.
    #[must_use]
    pub fn get(&self, tag: &str) -> Option<&BTreeSet<String>> {
        self.resolved.get(tag.strip_prefix('#').unwrap_or(tag))
    }

    /// How many tags were loaded.
    #[must_use]
    pub fn len(&self) -> usize {
        self.resolved.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.resolved.is_empty()
    }
}

/// Depth-first resolve with a cycle guard.
///
/// The guard is not defensive programming: datapacks are user data, and a
/// cycle would otherwise be a stack overflow at world load rather than a
/// message naming the two tags involved.
fn resolve(
    name: &str,
    raw: &BTreeMap<String, Vec<String>>,
    visiting: &mut Vec<String>,
) -> Result<BTreeSet<String>, String> {
    if visiting.iter().any(|v| v == name) {
        visiting.push(name.to_string());
        return Err(format!("block tag cycle: {}", visiting.join(" -> ")));
    }
    visiting.push(name.to_string());

    let entries = raw
        .get(name)
        .ok_or_else(|| format!("block tag `{name}` does not exist"))?;
    let mut members = BTreeSet::new();
    for entry in entries {
        if let Some(referenced) = entry.strip_prefix('#') {
            members.extend(resolve(referenced, raw, visiting)?);
        } else {
            members.insert(entry.clone());
        }
    }

    visiting.pop();
    Ok(members)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, values: &[&str]) {
        let body = serde_json::json!({ "values": values });
        std::fs::write(
            dir.join(format!("{name}.json")),
            serde_json::to_string(&body).unwrap(),
        )
        .unwrap();
    }

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rustmc-tags-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_tag_reference_is_flattened_transitively() {
        let dir = temp_dir("nested");
        write(&dir, "a", &["minecraft:stone", "#minecraft:b"]);
        write(&dir, "b", &["minecraft:dirt", "#minecraft:c"]);
        write(&dir, "c", &["minecraft:gravel"]);
        let tags = BlockTags::load(&dir).expect("load");

        assert!(tags.contains("#minecraft:a", "minecraft:gravel"));
        assert!(tags.contains("minecraft:a", "minecraft:dirt"));
        assert!(!tags.contains("#minecraft:c", "minecraft:stone"));
        assert_eq!(tags.get("minecraft:a").expect("a").len(), 3);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_object_entry_is_read_as_its_id() {
        let dir = temp_dir("object");
        write(&dir, "a", &["minecraft:stone"]);
        let body = serde_json::json!({
            "values": [{"id": "minecraft:tuff", "required": false}, "#minecraft:a"]
        });
        std::fs::write(dir.join("b.json"), serde_json::to_string(&body).unwrap()).unwrap();
        let tags = BlockTags::load(&dir).expect("load");
        assert!(tags.contains("#minecraft:b", "minecraft:tuff"));
        assert!(tags.contains("#minecraft:b", "minecraft:stone"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_cycle_is_an_error_naming_both_tags_not_a_stack_overflow() {
        let dir = temp_dir("cycle");
        write(&dir, "a", &["#minecraft:b"]);
        write(&dir, "b", &["#minecraft:a"]);
        let error = BlockTags::load(&dir).expect_err("cycle should fail");
        assert!(error.contains("cycle"), "{error}");
        assert!(
            error.contains("minecraft:a") && error.contains("minecraft:b"),
            "{error}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_dangling_reference_is_an_error_not_an_empty_set() {
        // A carver whose replaceable set is silently empty replaces nothing,
        // which reads as "caves are switched off" rather than as a fault.
        let dir = temp_dir("dangling");
        write(&dir, "a", &["#minecraft:missing"]);
        let error = BlockTags::load(&dir).expect_err("dangling should fail");
        assert!(error.contains("minecraft:missing"), "{error}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_real_carver_replaceables_resolve() {
        // The tag carvers actually name, resolved from the generated data.
        // Skipped rather than failed when the extract is absent, like the
        // other data-backed gates.
        let dir = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../oracle-work/vanilla-26.2/generated-data/data/minecraft/tags/block"
        ));
        if !rustmc_data::fixture::available(dir, "tag extract", "ci/vanilla-server.sh data") {
            return;
        }
        let tags = BlockTags::load(dir).expect("load real tags");
        let replaceables = tags
            .get("minecraft:overworld_carver_replaceables")
            .expect("overworld_carver_replaceables");

        // Things a cave must eat through, including the two 26.2 blocks the
        // sulfur caves are made of.
        for block in [
            "minecraft:stone",
            "minecraft:deepslate",
            "minecraft:dirt",
            "minecraft:grass_block",
            "minecraft:water",
            "minecraft:gravel",
            "minecraft:sandstone",
            "minecraft:cinnabar",
            "minecraft:sulfur",
        ] {
            assert!(replaceables.contains(block), "{block} should be carvable");
        }
        // Things it must not.
        for block in ["minecraft:bedrock", "minecraft:lava", "minecraft:air"] {
            assert!(
                !replaceables.contains(block),
                "{block} must not be carvable — caves would eat the world floor"
            );
        }
    }
}
