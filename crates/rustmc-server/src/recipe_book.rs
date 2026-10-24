//! `ServerRecipeBook.addRecipes` — the narrow slice that closes gameplay
//! check `040-stairs-and-furnace-face-apart`.
//!
//! # What this is not
//!
//! Not advancements. Vanilla's route to this packet is
//! `InventoryChangeTrigger` scanning a player's whole inventory against every
//! `minecraft:inventory_changed` advancement criterion, awarding whichever
//! advancements complete and — through `PlayerAdvancements` — handing their
//! `rewards.recipes` to `ServerRecipeBook.addRecipes`. None of that engine is
//! built here. `data/vanilla/recipe_unlocks.json` is the narrow table the
//! roadmap asks for instead: item id -> the recipe ids an
//! `inventory_changed` criterion naming that item awards, read directly out
//! of vanilla's own advancement JSON at generation time (see
//! `ci/dump-recipe-unlocks.sh`), plus a per-player `known` set. A recipe
//! reached only through an item this table does not name — or through a
//! multi-item criterion the generator declined to resolve — is a gap, not a
//! wrong answer: [`RecipeUnlocks::hold`] returns `None` and nothing is sent,
//! which is what this server did before this module existed.
//!
//! # `ServerRecipeBook.addRecipes`, read from the pinned jar
//!
//! `javap -p -c -constants` on `net.minecraft.stats.ServerRecipeBook`:
//!
//! * offsets 45-56 — `if (known.contains(id)) continue` — idempotent, so a
//!   recipe already known produces nothing the second time an item is held.
//! * offsets 59-69 — `recipe.isSpecial()` recipes are skipped. Not a
//!   concern here: every recipe this table can name came from an
//!   advancement's `rewards.recipes`, and nothing in the extract lets this
//!   table point at a special recipe's id without also having captured its
//!   entry bytes, which is the only thing that would let it emit at all.
//! * offsets 72-98 — add to `known`, mark highlighted, collect the recipe's
//!   display entry.
//! * offsets 115-137 — **one** `ClientboundRecipeBookAddPacket(entries,
//!   false)` — `iconst_0` at offset 133 is the `replace` field — sent only
//!   when the collected list is non-empty.
//! * offsets 103-109 — fires `CriteriaTriggers.RECIPE_UNLOCKED` afterwards.
//!   Not reproduced: nothing downstream in this server reads it, and every
//!   corpus scenario's `about` is the packet, not the trigger.
//!
//! # Why the entry bytes are captured, not reconstructed
//!
//! `ClientboundRecipeBookAddPacket$Entry` carries a
//! `RecipeDisplayEntry` — `RecipeDisplayId`, a polymorphic `RecipeDisplay`
//! (`ShapedCraftingRecipeDisplay` for `minecraft:smoker`, holding a tree of
//! `SlotDisplay`s down to `ItemStackSlotDisplay`/`ItemSlotDisplay`), an
//! `OptionalInt` group, a `RecipeBookCategory` and an
//! `Optional<List<Ingredient>>` — serialised through `StreamCodec.composite`
//! stacks nested several deep. Reimplementing that tree in Rust would be a
//! second, general recipe-display protocol, not "one packet, one call site,
//! one generated table". The corpus compares this packet byte-for-byte
//! regardless (`tools/oracle-runner` has no decoder for it, so an unnamed
//! `packet:0x4A` is compared as an exact digest of its whole body — see
//! `compare::near`'s R2 audit) — so a captured, replayed byte string is not a
//! shortcut against that comparison, it is what satisfies it: vanilla's own
//! bytes, read off the wire once by `ci/dump-recipe-unlocks.sh` connecting to
//! a real vanilla server, never reconstructed from a guess at the codec.
//! The `flags` byte (`FLAG_NOTIFICATION = 1`, `FLAG_HIGHLIGHT = 2`) is
//! captured as part of the same bytes, from `lambda$addRecipes$0`:
//! `notification = recipe.showNotification()`, `highlight` is always `true`
//! (`iconst_1`) for a freshly unlocked recipe.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::Path;

/// Read from real vanilla, per recipe: item -> recipes it unlocks, and each
/// recipe's captured `Entry` bytes (`RecipeDisplayEntry`'s own STREAM_CODEC
/// output, followed by the one flags byte — **not** including the packet's
/// outer list-count `VarInt` or the trailing `replace` byte, which
/// [`RecipeUnlocks::hold`] supplies itself).
#[derive(Debug, Default)]
pub struct RecipeUnlocks {
    by_item: BTreeMap<String, Vec<String>>,
    entries: BTreeMap<String, Vec<u8>>,
}

impl RecipeUnlocks {
    /// # Errors
    /// If the file is missing or not the shape `ci/dump-recipe-unlocks.sh`
    /// writes.
    pub fn load(path: &Path) -> io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let doc: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| io::Error::other(format!("{}: {e}", path.display())))?;
        Self::parse(&doc)
    }

    /// # Errors
    /// If `by_item` or `displays` is missing, or an entry's hex is malformed.
    pub fn parse(doc: &serde_json::Value) -> io::Result<Self> {
        let bad = || io::Error::other("recipe_unlocks.json is not the shape this reads");
        let by_item_doc = doc
            .get("by_item")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(bad)?;
        let mut by_item = BTreeMap::new();
        for (item, recipes) in by_item_doc {
            let list = recipes.as_array().ok_or_else(bad)?;
            let names = list
                .iter()
                .map(|v| v.as_str().map(str::to_string).ok_or_else(bad))
                .collect::<io::Result<Vec<_>>>()?;
            by_item.insert(item.clone(), names);
        }
        let displays_doc = doc
            .get("displays")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(bad)?;
        let mut entries = BTreeMap::new();
        for (recipe, v) in displays_doc {
            let hex = v
                .get("entry_hex")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(bad)?;
            entries.insert(recipe.clone(), unhex(hex).ok_or_else(bad)?);
        }
        Ok(Self { by_item, entries })
    }

    /// The one table this process serves, loaded once.
    ///
    /// # Errors
    /// Whatever [`Self::load`] returned the first time, repeated.
    pub fn shared(path: &Path) -> Result<&'static Self, String> {
        static TABLE: std::sync::OnceLock<Result<RecipeUnlocks, String>> =
            std::sync::OnceLock::new();
        TABLE
            .get_or_init(|| Self::load(path).map_err(|e| e.to_string()))
            .as_ref()
            .map_err(Clone::clone)
    }

    #[must_use]
    pub fn item_count(&self) -> usize {
        self.by_item.len()
    }

    #[must_use]
    pub fn recipe_count(&self) -> usize {
        self.entries.len()
    }

    /// `ServerRecipeBook.addRecipes`, narrowed to the recipes an
    /// `inventory_changed` criterion on `item` names — offsets 45-56
    /// (`known.contains`), 72-98 (collect) and 115-137 (send).
    ///
    /// `known` is the per-connection `ServerRecipeBook.known` set; a recipe
    /// already in it produces nothing, matching offset 56's `ifne 112`
    /// (skip). A recipe this table names for `item` but has no captured
    /// entry for is skipped and **left out of `known`** — never marked
    /// unlocked without the bytes to say so, so a table this generator has
    /// not (yet) captured is a permanent, honest gap rather than a state
    /// vanilla's own player would never reach.
    ///
    /// Returns the packet body (list-count `VarInt`, then each entry's
    /// bytes, then the `replace` byte) or `None` when nothing new was
    /// unlocked — offsets 115-121, `list.isEmpty()` — or when `item` unlocks
    /// nothing this table knows about at all.
    #[must_use]
    pub fn hold(&self, item: &str, known: &mut BTreeSet<String>) -> Option<Vec<u8>> {
        let candidates = self.by_item.get(item)?;
        let mut newly: Vec<&[u8]> = Vec::new();
        for recipe in candidates {
            if known.contains(recipe) {
                continue;
            }
            let Some(bytes) = self.entries.get(recipe) else {
                continue;
            };
            known.insert(recipe.clone());
            newly.push(bytes.as_slice());
        }
        if newly.is_empty() {
            return None;
        }
        let mut body = Vec::new();
        rustmc_proto::varint::write_varint(&mut body, i32::try_from(newly.len()).ok()?).ok()?;
        for e in newly {
            body.extend_from_slice(e);
        }
        // `iconst_0` at `ServerRecipeBook.addRecipes` offset 133: `replace`
        // is always `false` on this path.
        body.push(0);
        Some(body)
    }
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> RecipeUnlocks {
        RecipeUnlocks::parse(&serde_json::json!({
            "by_item": {
                "minecraft:furnace": ["minecraft:smoker"],
                "minecraft:oak_stairs": [],
            },
            "displays": {
                "minecraft:smoker": {"entry_hex": "aabbcc"},
            },
        }))
        .expect("parses")
    }

    #[test]
    fn a_furnace_unlocks_the_smoker_once() {
        let t = table();
        let mut known = BTreeSet::new();
        let body = t.hold("minecraft:furnace", &mut known).expect("unlocks");
        // count=1, then the three captured bytes, then replace=false.
        assert_eq!(body, vec![1, 0xaa, 0xbb, 0xcc, 0]);
        assert!(known.contains("minecraft:smoker"));
    }

    #[test]
    fn the_second_furnace_sends_nothing() {
        let t = table();
        let mut known = BTreeSet::new();
        assert!(t.hold("minecraft:furnace", &mut known).is_some());
        assert_eq!(
            t.hold("minecraft:furnace", &mut known),
            None,
            "offset 56: known.contains -> skip"
        );
    }

    #[test]
    fn oak_stairs_unlocks_nothing() {
        // 040's own negative control, step 0: the item is in the table
        // (every held item is looked up) but names no recipe.
        let mut known = BTreeSet::new();
        assert_eq!(table().hold("minecraft:oak_stairs", &mut known), None);
    }

    #[test]
    fn an_item_never_held_before_unlocks_nothing() {
        let mut known = BTreeSet::new();
        assert_eq!(table().hold("minecraft:oak_log", &mut known), None);
    }

    #[test]
    fn a_named_recipe_with_no_captured_entry_is_a_gap_not_a_send() {
        let t = RecipeUnlocks::parse(&serde_json::json!({
            "by_item": {"minecraft:furnace": ["minecraft:smoker"]},
            "displays": {},
        }))
        .expect("parses");
        let mut known = BTreeSet::new();
        assert_eq!(t.hold("minecraft:furnace", &mut known), None);
        assert!(
            !known.contains("minecraft:smoker"),
            "never marked known without the bytes to back it"
        );
    }
}
