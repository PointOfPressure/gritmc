//! `ItemStack.is(TagKey<Item>)` — item tag membership, out of the flattened
//! `data/vanilla/item_tags.json` that `ci/loot-golden.sh` writes from the
//! pinned jar's own data pack (tags already resolved through their `#`
//! references, so membership is one lookup).
//!
//! Loaded once, lazily, and **absent is an answer of its own**: [`is_in`]
//! returns `None` when the table cannot be read, so a caller has to decide
//! what an unknown membership means rather than being handed `false` — which
//! would silently read as "vanilla says no" for every tag in the game.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

/// Spelled as one quoted `data/vanilla/...` literal so
/// `ci/gen-gameplay-tables.sh` sees that this tree needs the table.
const ITEM_TAGS_PATH: &str = "data/vanilla/item_tags.json";

type Tags = BTreeMap<String, BTreeSet<String>>;

fn load(text: &str) -> Option<Tags> {
    let raw: BTreeMap<String, Vec<String>> = serde_json::from_str(text).ok()?;
    Some(
        raw.into_iter()
            .map(|(k, v)| (k, v.into_iter().collect()))
            .collect(),
    )
}

fn shared() -> Option<&'static Tags> {
    static T: OnceLock<Option<Tags>> = OnceLock::new();
    T.get_or_init(|| {
        let text = std::fs::read_to_string(ITEM_TAGS_PATH).ok()?;
        let tags = load(&text);
        if tags.is_none() {
            eprintln!("{ITEM_TAGS_PATH} is not the flattened tag map — no item tag answers");
        }
        tags
    })
    .as_ref()
}

/// Whether `item` is in the item tag `tag` (spelled `#minecraft:...`), or
/// `None` if the tag table is missing. A tag the table does not name is
/// `Some(false)`: the flattened map carries every item tag the jar defines.
#[must_use]
pub fn is_in(tag: &str, item: &str) -> Option<bool> {
    shared().map(|t| t.get(tag).is_some_and(|s| s.contains(item)))
}

/// `ItemTags.NOTE_BLOCK_TOP_INSTRUMENTS`.
pub const NOTE_BLOCK_TOP_INSTRUMENTS: &str = "#minecraft:noteblock_top_instruments";

/// `ItemTags.LECTERN_BOOKS`.
pub const LECTERN_BOOKS: &str = "#minecraft:lectern_books";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_flattened_map_answers_membership() {
        let t = load(r##"{"#minecraft:x": ["minecraft:a", "minecraft:b"]}"##).expect("parses");
        assert!(t["#minecraft:x"].contains("minecraft:a"));
        assert!(!t["#minecraft:x"].contains("minecraft:c"));
        assert!(load("[1, 2]").is_none(), "not the map shape");
    }
}
