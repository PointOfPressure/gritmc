//! The criterion triggers this server can actually fire.
//!
//! # The vocabulary, and why only some of it is here
//!
//! `BuiltInRegistries.TRIGGER_TYPES` holds ~60 trigger types and vanilla's
//! 1,688 documents use **54** of them. A trigger is a call site, not an
//! algorithm: `SimpleCriterionTrigger.trigger(player, predicate)` walks the
//! listeners registered for that trigger and awards the ones whose
//! `TriggerInstance.matches` says yes. So porting a trigger costs nothing
//! except the event that fires it and the predicate that filters it, and
//! **the event is the constraint**. A trigger fired from an event this server
//! does not have would be a call site nothing reaches, which is this
//! project's own defect class: a check whose green cannot be turned red.
//!
//! So this module holds exactly the triggers whose event exists, each named
//! with the call site it is fired from, and nothing else. What is missing is
//! missing because the event is, and `docs/FINDINGS.md` carries the census.
//!
//! # `minecraft:inventory_changed`
//!
//! `ServerPlayer$2.slotChanged` — a `ContainerListener` on the player's own
//! menu — fires `CriteriaTriggers.INVENTORY_CHANGED.trigger(player,
//! inventory, stack)` for every slot change whose `slot.container` is the
//! player's inventory, skipping a `ResultSlot`. That is the same event this
//! server already reads at `crate::world_send`'s `creative_slot_item` call,
//! which is where `ServerRecipeBook.addRecipes` is fired from today — and in
//! vanilla `addRecipes` is *downstream of this trigger*, so firing it here is
//! the more faithful arrangement rather than a second one.
//!
//! `InventoryChangeTrigger.trigger` counts three things over the whole
//! inventory before matching — slots that are full to `getMaxStackSize`,
//! slots that are empty, slots that are occupied — and
//! `TriggerInstance.matches` has three branches:
//!
//! 1. `slots.matches(full, empty, occupied)`, then
//! 2. **no item predicates** — true;
//! 3. **exactly one** — `!changed.isEmpty() && items[0].test(changed)`. Only
//!    the stack that just changed is tested, not the inventory;
//! 4. **more than one** — copy the list, walk every non-empty inventory slot
//!    calling `removeIf(p -> p.test(stack))`, true when the list empties.
//!
//! Branch 3 is 1,735 of the 1,738 `inventory_changed` criteria in vanilla's
//! data, and every one of those 1,735 is a single `ItemPredicate` whose only
//! key is `items` — 1,635 a plain item id, 100 an item tag. It needs the
//! changed stack and nothing else, which is exactly what this server has.
//! The other three are one `slots`-only criterion, one with three predicates
//! and one with four; all three need the whole inventory and are
//! [`Fired::skipped`] rather than silently false.

use super::player::{PlayerAdvancements, Rewards};
use super::{Criterion, Table};
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::Path;

/// `data/vanilla/item_tags.json` — every `#minecraft:…` item tag, flattened,
/// written by `ci/loot-golden.sh`.
#[derive(Debug, Default)]
pub struct ItemTags {
    by_tag: BTreeMap<String, BTreeSet<String>>,
}

impl ItemTags {
    /// # Errors
    /// If the file is missing or is not a map of tag to id list.
    pub fn load(path: &Path) -> io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let doc: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| io::Error::other(format!("{}: {e}", path.display())))?;
        Self::parse(&doc)
    }

    /// # Errors
    /// If the document is not a map of tag to id list.
    pub fn parse(doc: &serde_json::Value) -> io::Result<Self> {
        let bad = || io::Error::other("item_tags.json is not the shape this reads");
        let obj = doc.as_object().ok_or_else(bad)?;
        let mut by_tag = BTreeMap::new();
        for (tag, ids) in obj {
            let mut set = BTreeSet::new();
            for v in ids.as_array().ok_or_else(bad)? {
                set.insert(v.as_str().ok_or_else(bad)?.to_string());
            }
            by_tag.insert(tag.clone(), set);
        }
        Ok(Self { by_tag })
    }

    /// The one table this process serves, loaded once.
    ///
    /// # Errors
    /// Whatever [`Self::load`] returned the first time, repeated.
    pub fn shared(path: &Path) -> Result<&'static Self, String> {
        static TAGS: std::sync::OnceLock<Result<ItemTags, String>> = std::sync::OnceLock::new();
        TAGS.get_or_init(|| Self::load(path).map_err(|e| e.to_string()))
            .as_ref()
            .map_err(Clone::clone)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.by_tag.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_tag.is_empty()
    }

    /// `ItemInstance.is(HolderSet)` for a tag-shaped holder set.
    ///
    /// `tag` is written as the datapack writes it — leading `#`, namespace
    /// optional — and `ci/loot-golden.sh` keys this table the same way, `#`
    /// included. A tag this table does not hold is **not** a match and is
    /// reported by [`Fired::unknown_tags`]: an absent tag that silently
    /// matched nothing is indistinguishable from an item that is genuinely
    /// not in it.
    #[must_use]
    pub fn contains(&self, tag: &str, item: &str) -> Option<bool> {
        let key = if tag.contains(':') {
            tag.to_string()
        } else {
            format!("#minecraft:{}", tag.trim_start_matches('#'))
        };
        self.by_tag.get(&key).map(|s| s.contains(item))
    }
}

/// What one trigger fire did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Fired {
    /// The `(advancement, criterion)` pairs newly granted.
    pub granted: Vec<(String, String)>,
    /// Rewards from every advancement that completed on this fire.
    pub rewards: Rewards,
    /// Criteria whose shape this trigger declines to evaluate, by
    /// `advancement/criterion`. **Not** the same as a criterion that did not
    /// match: a skip is this server saying it cannot answer, and it is
    /// counted so a datapack that leans on the shape shows up as a number
    /// rather than as an advancement that never fires.
    pub skipped: Vec<String>,
    /// Item tags named by a predicate that `item_tags.json` does not hold.
    pub unknown_tags: Vec<String>,
}

/// `CriteriaTriggers.INVENTORY_CHANGED.trigger(player, inventory, stack)`,
/// narrowed to the branch this server's event can answer.
///
/// `changed_item` is the id of the stack that just changed and is the *whole*
/// input: this fires branch 3 of `TriggerInstance.matches`, which reads the
/// changed stack and nothing else. `obtained_millis` stands in for
/// `Instant.now()` inside `CriterionProgress.grant`.
///
/// A criterion whose predicate list is not exactly one, or whose single
/// predicate carries a `count` or `components` key, or whose criterion
/// carries a `slots` bound, is recorded in [`Fired::skipped`]. Those need
/// counts over the whole inventory, which this server does not model.
pub fn inventory_changed(
    pa: &mut PlayerAdvancements,
    table: &Table,
    tags: Option<&ItemTags>,
    changed_item: &str,
    obtained_millis: i64,
) -> Fired {
    let mut fired = Fired::default();
    // The listener list is recomputed rather than cached, and the pairs are
    // taken by value so `pa` can be borrowed mutably below.
    let candidates: Vec<(String, String, Criterion)> = table
        .listeners("minecraft:inventory_changed")
        .into_iter()
        .map(|(a, name, c)| (a.id.clone(), name.to_string(), c.clone()))
        .collect();
    for (adv, criterion, c) in candidates {
        match single_item_predicate(&c) {
            Shape::Unsupported => fired.skipped.push(format!("{adv}/{criterion}")),
            Shape::NoItems => {
                // Branch 2: an empty predicate list matches whatever changed.
                award(pa, &adv, &criterion, obtained_millis, &mut fired);
            }
            Shape::Item(want) => {
                let hit = if want.starts_with('#') {
                    match tags.and_then(|t| t.contains(&want, changed_item)) {
                        Some(v) => v,
                        None => {
                            if !fired.unknown_tags.contains(&want) {
                                fired.unknown_tags.push(want.clone());
                            }
                            false
                        }
                    }
                } else {
                    want == changed_item
                };
                if hit {
                    award(pa, &adv, &criterion, obtained_millis, &mut fired);
                }
            }
        }
    }
    fired
}

fn award(
    pa: &mut PlayerAdvancements,
    adv: &str,
    criterion: &str,
    obtained_millis: i64,
    fired: &mut Fired,
) {
    let (changed, r) = pa.award(adv, criterion, obtained_millis);
    if changed {
        fired.granted.push((adv.to_string(), criterion.to_string()));
    }
    fired.rewards.experience += r.experience;
    fired.rewards.recipes.extend(r.recipes);
    fired.rewards.loot.extend(r.loot);
    fired.rewards.refused_functions.extend(r.refused_functions);
}

/// Which of `InventoryChangeTrigger$TriggerInstance.matches`'s branches a
/// criterion's `conditions` object falls into.
enum Shape {
    NoItems,
    Item(String),
    Unsupported,
}

fn single_item_predicate(c: &Criterion) -> Shape {
    let Some(obj) = c.conditions.as_object() else {
        // No `conditions` at all: no slots bound, no item predicates.
        return Shape::NoItems;
    };
    if obj.contains_key("slots") {
        return Shape::Unsupported;
    }
    let Some(items) = obj.get("items") else {
        return Shape::NoItems;
    };
    let Some(list) = items.as_array() else {
        return Shape::Unsupported;
    };
    if list.len() != 1 {
        return Shape::Unsupported;
    }
    let Some(p) = list[0].as_object() else {
        return Shape::Unsupported;
    };
    // Anything beyond `items` needs a count or a component match, neither of
    // which this call site carries.
    if p.len() != 1 {
        return Shape::Unsupported;
    }
    match p.get("items") {
        Some(serde_json::Value::String(s)) => Shape::Item(s.clone()),
        _ => Shape::Unsupported,
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Table, Tree};
    use super::*;

    fn table() -> Table {
        Table::parse(&serde_json::json!({
            "advancements": {
                "t:root": {"parent": null, "requirements": [["ct"]],
                    "criteria_triggers": {"ct": "minecraft:inventory_changed"},
                    "document": {"criteria": {"ct": {"trigger": "minecraft:inventory_changed",
                        "conditions": {"items": [{"items": "minecraft:crafting_table"}]}}}},
                    "display": {"hidden": false}, "experience": 0,
                    "reward_recipes": [], "reward_loot": [], "reward_function": false,
                    "holder_wire": "aa"},
                "t:planks": {"parent": "t:root", "requirements": [["p"]],
                    "criteria_triggers": {"p": "minecraft:inventory_changed"},
                    "document": {"criteria": {"p": {"trigger": "minecraft:inventory_changed",
                        "conditions": {"items": [{"items": "#minecraft:planks"}]}}}},
                    "display": {"hidden": false}, "experience": 3,
                    "reward_recipes": ["t:stick"], "reward_loot": [], "reward_function": false,
                    "holder_wire": "bb"},
                "t:counted": {"parent": "t:root", "requirements": [["c"]],
                    "criteria_triggers": {"c": "minecraft:inventory_changed"},
                    "document": {"criteria": {"c": {"trigger": "minecraft:inventory_changed",
                        "conditions": {"items": [{"items": "minecraft:stone", "count": 64}]}}}},
                    "display": {"hidden": false}, "experience": 0,
                    "reward_recipes": [], "reward_loot": [], "reward_function": false,
                    "holder_wire": "cc"},
                "t:multi": {"parent": "t:root", "requirements": [["m"]],
                    "criteria_triggers": {"m": "minecraft:inventory_changed"},
                    "document": {"criteria": {"m": {"trigger": "minecraft:inventory_changed",
                        "conditions": {"items": [{"items": "minecraft:a"}, {"items": "minecraft:b"}]}}}},
                    "display": {"hidden": false}, "experience": 0,
                    "reward_recipes": [], "reward_loot": [], "reward_function": false,
                    "holder_wire": "dd"},
                "t:other": {"parent": "t:root", "requirements": [["o"]],
                    "criteria_triggers": {"o": "minecraft:placed_block"},
                    "document": {"criteria": {"o": {"trigger": "minecraft:placed_block",
                        "conditions": {}}}},
                    "display": {"hidden": false}, "experience": 0,
                    "reward_recipes": [], "reward_loot": [], "reward_function": false,
                    "holder_wire": "ee"}
            }
        }))
        .expect("fixture parses")
    }

    fn tags() -> ItemTags {
        ItemTags::parse(&serde_json::json!({
            "#minecraft:planks": ["minecraft:oak_planks", "minecraft:acacia_planks"]
        }))
        .expect("tags parse")
    }

    #[test]
    fn a_plain_item_id_awards_only_its_own_criterion() {
        let t = table();
        let tree = Tree::build(&t);
        let mut pa = PlayerAdvancements::new(&t, &tree);
        let tg = tags();
        let f = inventory_changed(&mut pa, &t, Some(&tg), "minecraft:crafting_table", 1);
        assert_eq!(f.granted, vec![("t:root".to_string(), "ct".to_string())]);
        // The negative control: a different item grants nothing, so the line
        // above is the predicate matching and not the trigger awarding
        // everything it walks.
        let mut pa2 = PlayerAdvancements::new(&t, &tree);
        let g = inventory_changed(&mut pa2, &t, Some(&tg), "minecraft:dirt", 1);
        assert!(g.granted.is_empty());
    }

    #[test]
    fn a_tag_predicate_matches_a_member_and_not_a_non_member() {
        let t = table();
        let tree = Tree::build(&t);
        let tg = tags();
        let mut pa = PlayerAdvancements::new(&t, &tree);
        let f = inventory_changed(&mut pa, &t, Some(&tg), "minecraft:oak_planks", 1);
        assert_eq!(f.granted, vec![("t:planks".to_string(), "p".to_string())]);
        assert_eq!(f.rewards.experience, 3);
        assert_eq!(f.rewards.recipes, vec!["t:stick".to_string()]);
        let mut pa2 = PlayerAdvancements::new(&t, &tree);
        assert!(
            inventory_changed(&mut pa2, &t, Some(&tg), "minecraft:stone_bricks", 1)
                .granted
                .is_empty()
        );
    }

    #[test]
    fn a_missing_tag_table_is_reported_and_never_matches() {
        let t = table();
        let tree = Tree::build(&t);
        let mut pa = PlayerAdvancements::new(&t, &tree);
        let f = inventory_changed(&mut pa, &t, None, "minecraft:oak_planks", 1);
        assert!(f.granted.is_empty());
        assert_eq!(f.unknown_tags, vec!["#minecraft:planks".to_string()]);
    }

    /// The three unsupported shapes are counted, not silently false, and the
    /// count is exactly the shapes named — a criterion for another trigger is
    /// not walked at all.
    #[test]
    fn unsupported_shapes_are_skipped_and_named() {
        let t = table();
        let tree = Tree::build(&t);
        let tg = tags();
        let mut pa = PlayerAdvancements::new(&t, &tree);
        let f = inventory_changed(&mut pa, &t, Some(&tg), "minecraft:stone", 1);
        let mut s = f.skipped;
        s.sort();
        assert_eq!(s, vec!["t:counted/c".to_string(), "t:multi/m".to_string()]);
        assert!(
            f.granted.is_empty(),
            "a skipped criterion must not be awarded either"
        );
    }

    #[test]
    fn a_second_fire_of_the_same_item_grants_nothing_new() {
        let t = table();
        let tree = Tree::build(&t);
        let tg = tags();
        let mut pa = PlayerAdvancements::new(&t, &tree);
        assert_eq!(
            inventory_changed(&mut pa, &t, Some(&tg), "minecraft:crafting_table", 1)
                .granted
                .len(),
            1
        );
        assert!(
            inventory_changed(&mut pa, &t, Some(&tg), "minecraft:crafting_table", 2)
                .granted
                .is_empty()
        );
    }

    /// The whole point, end to end: one item produces one
    /// `update_advancements` body, and the body names the tree.
    #[test]
    fn one_item_produces_one_packet() {
        let t = table();
        let tree = Tree::build(&t);
        let tg = tags();
        let mut pa = PlayerAdvancements::new(&t, &tree);
        assert!(
            pa.flush_dirty(true).expect("write").is_none(),
            "nothing before the item"
        );
        inventory_changed(&mut pa, &t, Some(&tg), "minecraft:crafting_table", 1);
        let body = pa.flush_dirty(true).expect("write").expect("a packet");
        assert!(!body.is_empty());
        assert!(pa.is_visible("t:root"));
        assert!(
            pa.is_visible("t:planks"),
            "the whole visible tree, not just the awarded node"
        );
        assert!(
            pa.flush_dirty(true).expect("write").is_none(),
            "and not again"
        );
    }
}
