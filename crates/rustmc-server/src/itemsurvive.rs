//! Which `canSurvive` rule a block reaches, off `data/vanilla/item_survive.json`.
//!
//! # Why the class name is a table and not a match arm
//!
//! [`rustmc_behavior::place_survive::can_survive`] dispatches on **the class
//! that declares `canSurvive`** and, for the vegetation family, on the class
//! that declares `mayPlaceOn`. Neither is derivable from a block's name:
//! `minecraft:torch` reaches `BaseTorchBlock` and `minecraft:soul_torch` the
//! same one, but `minecraft:redstone_torch` reaches a different class again,
//! and `minecraft:lily_pad` and `minecraft:oak_sapling` are both
//! `VegetationBlock` with different `mayPlaceOn`s.
//!
//! A hand-written name-to-class map would be 1,115 entries, would go stale at
//! the next version with no symptom, and is the shape this project has paid
//! for repeatedly. `ci/item-survive-golden.sh` reads both class names off the
//! jar by reflection; this module is the two columns of that table and
//! nothing else, so the sweep the golden test runs and the answer the server
//! gives are driven by the same rows.
//!
//! # When the table is absent
//!
//! [`SurviveClasses::rule`] answers `None`, which
//! `place_survive::can_survive` reads as `BlockBehaviour`'s unconditional
//! `true` — so every placement is allowed, which is exactly what this server
//! did before this table existed. That is the same shape as `placed_by` and
//! `shapes` being `None` in [`crate::world_send`]: the absence costs the
//! gate and nothing else, and it is said out loud rather than shown as a
//! placement that mysteriously fails.

use std::collections::BTreeMap;
use std::path::Path;

/// The two dispatch keys for one block.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rule {
    /// The class declaring `canSurvive`, or `None` when nothing above the
    /// block does — `BlockBehaviour`'s `iconst_1; ireturn`.
    pub can_survive: Option<String>,
    /// The class declaring `mayPlaceOn`, or `None`. Only read when
    /// `can_survive` is `VegetationBlock`, but carried for every block
    /// because the extract has it and a column that exists for some rows and
    /// not others is a column a reader has to reason about.
    pub may_place_on: Option<String>,
}

/// `block name -> [`Rule`]`, for the 1,115 blocks a `BlockItem` names.
#[derive(Clone, Debug, Default)]
pub struct SurviveClasses {
    by_block: BTreeMap<String, Rule>,
}

/// Where `ci/item-survive-golden.sh` writes it.
pub const TABLE_PATH: &str = "data/vanilla/item_survive.json";

impl SurviveClasses {
    /// Load once per process, as every other table on the session path does.
    ///
    /// # Errors
    /// If the file is missing or malformed.
    pub fn shared(path: &Path) -> Result<&'static Self, String> {
        static ONCE: std::sync::OnceLock<Result<SurviveClasses, String>> =
            std::sync::OnceLock::new();
        match ONCE.get_or_init(|| Self::load(path)) {
            Ok(t) => Ok(t),
            Err(e) => Err(e.clone()),
        }
    }

    /// # Errors
    /// If the file is missing or malformed.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let json: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&json)
    }

    /// # Errors
    /// If the document has no `table`, or a row has no `block`.
    pub fn parse(json: &serde_json::Value) -> Result<Self, String> {
        let rows = json["table"]
            .as_array()
            .ok_or_else(|| "item_survive.json has no `table`".to_string())?;
        let mut by_block = BTreeMap::new();
        for row in rows {
            let block = row["block"]
                .as_str()
                .ok_or_else(|| "a row has no `block`".to_string())?;
            by_block.insert(
                block.to_string(),
                Rule {
                    can_survive: row["rule"].as_str().map(ToString::to_string),
                    may_place_on: row["mayPlaceOn"].as_str().map(ToString::to_string),
                },
            );
        }
        Ok(Self { by_block })
    }

    /// The rule for a **block**, not a state.
    ///
    /// `canSurvive` is a virtual method and a class is a property of the
    /// block, so `minecraft:snow[layers=3]` and `minecraft:snow[layers=8]`
    /// reach the same one. The caller passes a state and the name is taken
    /// off it here, because every call site already holds a state.
    #[must_use]
    pub fn rule(&self, state: &str) -> Option<&Rule> {
        self.by_block
            .get(rustmc_behavior::place_survive::block_of(state))
    }

    /// How many blocks the table names, for the boot line.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_block.len()
    }

    /// How many of them have a `canSurvive` rule at all — 311 of 1,115 in
    /// 26.2, and the figure worth printing because it is the size of the gate
    /// rather than the size of the table.
    #[must_use]
    pub fn with_a_rule(&self) -> usize {
        self.by_block
            .values()
            .filter(|r| r.can_survive.is_some())
            .count()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_block.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Option<SurviveClasses> {
        let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.pop();
        path.pop();
        path.push("data/vanilla/item_survive.json");
        SurviveClasses::load(&path).ok()
    }

    /// The two columns are read, and the second one is not the first: this is
    /// the pair that made the vegetation family need two keys.
    #[test]
    fn a_lily_pad_and_a_sapling_share_a_can_survive_and_not_a_may_place_on() {
        let Some(table) = table() else { return };
        let pad = table.rule("minecraft:lily_pad").expect("lily_pad");
        let sapling = table.rule("minecraft:oak_sapling").expect("oak_sapling");
        assert_eq!(pad.can_survive, sapling.can_survive);
        assert!(pad
            .can_survive
            .as_deref()
            .unwrap()
            .ends_with("VegetationBlock"));
        assert_ne!(pad.may_place_on, sapling.may_place_on);
        assert!(pad
            .may_place_on
            .as_deref()
            .unwrap()
            .ends_with("LilyPadBlock"));
    }

    /// A block with no rule is in the table with a `None`, not absent from
    /// it, so "no rule" and "not a BlockItem" stay distinguishable.
    #[test]
    fn a_block_with_no_rule_is_present_and_says_so() {
        let Some(table) = table() else { return };
        let stone = table
            .rule("minecraft:stone")
            .expect("stone is in the table");
        assert_eq!(stone.can_survive, None);
        assert_eq!(table.rule("minecraft:not_a_block"), None);
    }

    /// The state form is accepted and the properties are ignored.
    #[test]
    fn a_state_resolves_to_its_blocks_rule() {
        let Some(table) = table() else { return };
        assert_eq!(
            table.rule("minecraft:snow[layers=3]"),
            table.rule("minecraft:snow")
        );
    }

    #[test]
    fn the_census_is_the_jars() {
        let Some(table) = table() else { return };
        assert_eq!(table.len(), 1115);
        assert_eq!(table.with_a_rule(), 311);
    }
}
