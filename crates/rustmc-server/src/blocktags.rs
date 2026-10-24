//! `BlockState.is(TagKey<Block>)` — block tag membership, out of
//! `data/vanilla/tags_block.json`, which `ci/dump-block-tags.sh` writes from
//! Mojang's own data generator.
//!
//! That file is the generator's output **flattened by tag but not resolved**:
//! a tag's `values` may name another tag as `#minecraft:x`, exactly as the
//! data pack does (`wall_post_override` lists `#minecraft:signs`). Resolution
//! is done here, once, at load, depth first, and a reference to a tag the
//! file does not hold resolves to nothing rather than failing the whole
//! table — an optional entry (`{"id": ..., "required": false}`) is the only
//! way a vanilla data pack can name a missing tag, and it means the same.
//!
//! Absent is an answer of its own, as in [`crate::itemtags`]: [`is_in`]
//! returns `None` when the table cannot be read.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

/// Spelled as one quoted `data/vanilla/...` literal so
/// `ci/gen-gameplay-tables.sh` sees that this tree needs the table and runs
/// `ci/dump-block-tags.sh` for it.
const BLOCK_TAGS_PATH: &str = "data/vanilla/tags_block.json";

type Tags = BTreeMap<String, BTreeSet<String>>;

fn entry_id(v: &serde_json::Value) -> Option<&str> {
    v.as_str()
        .or_else(|| v.get("id").and_then(serde_json::Value::as_str))
}

fn resolve(
    raw: &BTreeMap<String, Vec<String>>,
    name: &str,
    seen: &mut BTreeSet<String>,
    out: &mut BTreeSet<String>,
) {
    if !seen.insert(name.to_string()) {
        return;
    }
    let Some(values) = raw.get(name) else { return };
    for v in values {
        if let Some(tag) = v.strip_prefix('#') {
            let tag = tag.strip_prefix("minecraft:").unwrap_or(tag);
            resolve(raw, tag, seen, out);
        } else {
            out.insert(v.clone());
        }
    }
}

/// Parse and resolve the flattened tag file. Keys are tag paths without
/// their namespace (`walls`), as the dump writes them.
fn load(text: &str) -> Option<Tags> {
    let doc: BTreeMap<String, serde_json::Value> = serde_json::from_str(text).ok()?;
    let mut raw: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (k, v) in doc {
        let values = v.get("values")?.as_array()?;
        raw.insert(
            k,
            values
                .iter()
                .filter_map(entry_id)
                .map(str::to_string)
                .collect(),
        );
    }
    let mut out = Tags::new();
    for k in raw.keys() {
        let mut members = BTreeSet::new();
        resolve(&raw, k, &mut BTreeSet::new(), &mut members);
        out.insert(k.clone(), members);
    }
    Some(out)
}

fn shared() -> Option<&'static Tags> {
    static T: OnceLock<Option<Tags>> = OnceLock::new();
    T.get_or_init(|| {
        let text = std::fs::read_to_string(BLOCK_TAGS_PATH).ok()?;
        let tags = load(&text);
        if tags.is_none() {
            eprintln!("{BLOCK_TAGS_PATH} is not the flattened tag map — no block tag answers");
        }
        tags
    })
    .as_ref()
}

/// Whether the block `block` (a registry name, no properties) is in the tag
/// `tag` (a path without namespace, e.g. `walls`), or `None` without the
/// table. A tag the table does not hold is `Some(false)`.
#[must_use]
pub fn is_in(tag: &str, block: &str) -> Option<bool> {
    shared().map(|t| t.get(tag).is_some_and(|s| s.contains(block)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reference_is_resolved_and_a_missing_one_is_empty() {
        let t = load(
            r##"{"a": {"values": ["minecraft:x", "#minecraft:b", {"id": "#minecraft:gone", "required": false}]},
                 "b": {"values": ["minecraft:y", "#minecraft:a"]}}"##,
        )
        .expect("parses");
        let a: Vec<&str> = t["a"].iter().map(String::as_str).collect();
        assert_eq!(
            a,
            ["minecraft:x", "minecraft:y"],
            "b resolved, the cycle back to a stopped"
        );
        assert!(t["b"].contains("minecraft:x"));
        assert!(load(r#"{"a": ["not", "the", "shape"]}"#).is_none());
    }
}
