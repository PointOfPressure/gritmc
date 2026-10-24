//! `PlayerAdvancements`, `AdvancementProgress`, `AdvancementRequirements` and
//! the `AdvancementVisibilityEvaluator`, transcribed from the pinned 26.2 jar.
//!
//! # What is here, and the one thing that is deliberately not
//!
//! The 1,688 advancement documents are **data**, extracted by
//! `ci/advancements-golden.sh` into `data/vanilla/advancements.json`: parent,
//! requirements, per-criterion trigger id, rewards, the display flags, and
//! `AdvancementHolder.STREAM_CODEC`'s own bytes for the advancement. This
//! module is the engine that walks them — which criteria a player has, when an
//! advancement completes, which of them the client is allowed to see, and what
//! `ClientboundUpdateAdvancementsPacket` carries.
//!
//! What is not here is a Rust reimplementation of `DisplayInfo`'s wire form.
//! It is two `ComponentSerialization.TRUSTED_STREAM_CODEC` component trees and
//! an `ItemStackTemplate` with a `DataComponentPatch`; the captured bytes are
//! vanilla's own encoder's output, for the reason
//! [`crate::recipe_book`]'s header gives about `RecipeDisplayEntry`.
//!
//! # THE PACKET THIS BUILDS CANNOT BE COMPARED TO VANILLA'S BYTE FOR BYTE
//!
//! Measured 2026-09-05, and it is the fact that shapes everything below.
//!
//! * **Eight bytes of every displayed advancement are a per-JVM identity
//!   hash.** `AdvancementNode.children` is an
//!   `it.unimi.dsi.fastutil.objects.ReferenceOpenHashSet` — identity-hashed
//!   — and `ServerAdvancementManager.apply` runs `TreeNodePosition.run` over
//!   it to write `DisplayInfo.x/y`, which `Advancement.write` puts on the
//!   wire. `ci/advancements-golden.sh`'s shuffled run moved **122 of 126**
//!   `y` values and **120 of 126** `holder_wire` strings; worse, two
//!   *identical* runs agreed three times and then disagreed on the fourth.
//!   So the dumper pins `setLocation(0, 0)` and never runs
//!   `TreeNodePosition`: every byte of `holder_wire` this server sends is a
//!   function of the datapack, and those two floats are **deliberately zero
//!   where vanilla's are unreproducible**. They place an icon in the
//!   client's tree view and nothing server-side reads them.
//! * `CriterionProgress.grant()` is `this.obtained = Instant.now()`, and
//!   `serializeToNetwork` writes it. Every `progress` entry on the wire
//!   carries a wall clock.
//!
//! Two independent reasons, either one sufficient. So a check that digests
//! this packet's body and compares it across two servers can be red while
//! this server is right, which is a check that cannot be trusted when it is
//! green either. What IS comparable is the packet's *structure*: whether one
//! was sent, the `reset` flag, how many advancements are in `added`, and the
//! body's LENGTH — the eight moving bytes are two floats and the clock is a
//! fixed-width long, so a wrong `added` set changes the length and a wrong
//! layout does not. `tools/oracle-runner` decodes exactly that and names in
//! its own source what it declines to digest and why.
//!
//! # Sibling order here is sorted, and that is a divergence on purpose
//!
//! [`Tree`] orders a node's children by resource id. Vanilla's order is the
//! identity-hash walk above, which is not reproducible even by vanilla, so
//! there is nothing to be faithful *to*: a port of it would be a port of one
//! JVM run. The order is observable in exactly one place — the sequence
//! `updateTreeVisibility` inserts into the `added` set — and that set is a
//! `java.util.HashSet` keyed on `Identifier.hashCode`, so insertion order
//! survives onto the wire only inside a bucket two ids share. Stated here
//! rather than left to be discovered.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::Path;

pub mod jhash;
pub mod player;
pub mod triggers;

/// One criterion of one advancement.
#[derive(Debug, Clone)]
pub struct Criterion {
    /// The `CriteriaTriggers` registry id the criterion's `trigger` names.
    pub trigger: String,
    /// The criterion's `conditions` object as the datapack wrote it, or
    /// `Null` where the document omitted it. Kept raw rather than parsed
    /// into a predicate tree: 54 distinct triggers appear across the 1,688
    /// documents and this server can fire a handful, so a parser per trigger
    /// would be 54 parsers written against events that do not exist.
    pub conditions: serde_json::Value,
}

/// One advancement, as `ci/advancements-golden.sh` extracted it.
#[derive(Debug, Clone)]
pub struct Advancement {
    pub id: String,
    pub parent: Option<String>,
    /// `AdvancementRequirements.requirements` — AND over the outer list, OR
    /// within each inner list. Vanilla's own parsed value, so a document that
    /// omits `requirements` already carries the `allOf(criteria.keySet())`
    /// default the codec filled in.
    pub requirements: Vec<Vec<String>>,
    /// criterion name -> the criterion's trigger id and its raw `conditions`
    /// object, straight out of the datapack document.
    pub criteria: BTreeMap<String, Criterion>,
    /// `Advancement.display().isPresent()`. An advancement without one is
    /// `HIDE` unconditionally — see [`visibility_rule`].
    pub has_display: bool,
    /// `DisplayInfo.isHidden()`. Meaningless when `has_display` is false.
    pub hidden: bool,
    /// `AdvancementRewards.experience`.
    pub experience: i32,
    /// `AdvancementRewards.recipes`, as ids.
    pub reward_recipes: Vec<String>,
    /// `AdvancementRewards.loot`, as ids.
    pub reward_loot: Vec<String>,
    /// Whether `AdvancementRewards.function` is present. The function itself
    /// is **refused by name**: running one means a command engine driven by
    /// datapack text, which is a different subsystem, and a reward that
    /// silently did nothing would be worse than one that says so.
    pub reward_function: bool,
    /// `AdvancementHolder.STREAM_CODEC`'s bytes for this advancement, with
    /// `DisplayInfo`'s two layout floats pinned to zero — see the module
    /// header — or `None` for the two whose icon carries a
    /// `minecraft:banner_patterns` component the dumper's `RegistryAccess`
    /// cannot resolve. `None` is a gap: such an advancement is never put in
    /// a packet's `added` list.
    pub holder_wire: Option<Vec<u8>>,
}

impl Advancement {
    /// `Advancement.isRoot()` — offsets 0-8, `parent.isEmpty()`.
    #[must_use]
    pub fn is_root(&self) -> bool {
        self.parent.is_none()
    }
}

/// `data/vanilla/advancements.json`.
#[derive(Debug, Default)]
pub struct Table {
    by_id: BTreeMap<String, Advancement>,
}

impl Table {
    /// # Errors
    /// If the file is missing or not the shape `ci/advancements-golden.sh`
    /// writes.
    pub fn load(path: &Path) -> io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let doc: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| io::Error::other(format!("{}: {e}", path.display())))?;
        Self::parse(&doc)
    }

    /// # Errors
    /// If `advancements` is missing or an entry is not the expected shape.
    pub fn parse(doc: &serde_json::Value) -> io::Result<Self> {
        let bad = || io::Error::other("advancements.json is not the shape this reads");
        let entries = doc
            .get("advancements")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(bad)?;
        let mut by_id = BTreeMap::new();
        for (id, v) in entries {
            let strings = |key: &str| -> io::Result<Vec<String>> {
                v.get(key)
                    .and_then(serde_json::Value::as_array)
                    .ok_or_else(bad)?
                    .iter()
                    .map(|s| s.as_str().map(str::to_string).ok_or_else(bad))
                    .collect()
            };
            let mut requirements = Vec::new();
            for group in v
                .get("requirements")
                .and_then(serde_json::Value::as_array)
                .ok_or_else(bad)?
            {
                let mut inner = Vec::new();
                for name in group.as_array().ok_or_else(bad)? {
                    inner.push(name.as_str().ok_or_else(bad)?.to_string());
                }
                requirements.push(inner);
            }
            // `criteria_triggers` is the trigger id per criterion and
            // `document.criteria` is the source object; both come from the
            // same parse, and reading the ids from their own key rather than
            // from the document is what makes a document this server cannot
            // parse still contribute its trigger vocabulary.
            let doc_criteria = v.get("document").and_then(|d| d.get("criteria"));
            let mut criteria = BTreeMap::new();
            for (name, trig) in v
                .get("criteria_triggers")
                .and_then(serde_json::Value::as_object)
                .ok_or_else(bad)?
            {
                let conditions = doc_criteria
                    .and_then(|c| c.get(name))
                    .and_then(|c| c.get("conditions"))
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                criteria.insert(
                    name.clone(),
                    Criterion {
                        trigger: trig.as_str().ok_or_else(bad)?.to_string(),
                        conditions,
                    },
                );
            }
            let display = v.get("display").ok_or_else(bad)?;
            let has_display = !display.is_null();
            let hidden = display
                .get("hidden")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            let holder_wire = match v.get("holder_wire").ok_or_else(bad)? {
                serde_json::Value::Null => None,
                serde_json::Value::String(h) => Some(unhex(h).ok_or_else(bad)?),
                _ => return Err(bad()),
            };
            by_id.insert(
                id.clone(),
                Advancement {
                    id: id.clone(),
                    parent: v
                        .get("parent")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string),
                    requirements,
                    criteria,
                    has_display,
                    hidden,
                    experience: i32::try_from(
                        v.get("experience")
                            .and_then(serde_json::Value::as_i64)
                            .ok_or_else(bad)?,
                    )
                    .map_err(|_| bad())?,
                    reward_recipes: strings("reward_recipes")?,
                    reward_loot: strings("reward_loot")?,
                    reward_function: v
                        .get("reward_function")
                        .and_then(serde_json::Value::as_bool)
                        .ok_or_else(bad)?,
                    holder_wire,
                },
            );
        }
        Ok(Self { by_id })
    }

    /// The one table this process serves, loaded once.
    ///
    /// # Errors
    /// Whatever [`Self::load`] returned the first time, repeated.
    pub fn shared(path: &Path) -> Result<&'static Self, String> {
        static TABLE: std::sync::OnceLock<Result<Table, String>> = std::sync::OnceLock::new();
        TABLE
            .get_or_init(|| Self::load(path).map_err(|e| e.to_string()))
            .as_ref()
            .map_err(Clone::clone)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Advancement> {
        self.by_id.get(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Advancement> {
        self.by_id.values()
    }

    /// Every advancement holding a criterion whose trigger is `trigger`, with
    /// the criterion names. Sorted by advancement id, then by criterion name;
    /// this is the index a trigger walks, and vanilla's own equivalent
    /// (`PlayerAdvancements.activeTriggers`, an `IdentityHashMap` of
    /// `CriterionTrigger` to a map of listener keys) has an order no port
    /// could reproduce — see this module's header.
    #[must_use]
    pub fn listeners(&self, trigger: &str) -> Vec<(&Advancement, &str, &Criterion)> {
        let mut out = Vec::new();
        for a in self.by_id.values() {
            for (name, c) in &a.criteria {
                if c.trigger == trigger {
                    out.push((a, name.as_str(), c));
                }
            }
        }
        out
    }
}

/// `AdvancementRequirements.test(Predicate<String>)` — offsets 0-46: every
/// outer group must have at least one member satisfying `done`.
///
/// `AdvancementRequirements.EMPTY` is an empty outer list, and an empty
/// `allMatch` is **true**, so an advancement with no requirements is done the
/// moment it is looked at. That is not a corner case introduced here: it is
/// how `checkForAutomaticTriggers` awards a criteria-less advancement on
/// login. None of vanilla's 1,688 documents is criteria-less — the codec
/// refuses one — so the branch is unreachable from vanilla data and is
/// transcribed rather than assumed away.
#[must_use]
pub fn requirements_test(requirements: &[Vec<String>], done: &dyn Fn(&str) -> bool) -> bool {
    requirements
        .iter()
        .all(|group| group.iter().any(|name| done(name)))
}

/// `AdvancementRequirements.count(Predicate<String>)` — how many outer groups
/// are satisfied. Feeds `AdvancementProgress.getProgressText`, which this
/// server does not send; kept because `getPercent` is the same quantity and
/// a future toast needs it.
#[must_use]
pub fn requirements_count(requirements: &[Vec<String>], done: &dyn Fn(&str) -> bool) -> usize {
    requirements
        .iter()
        .filter(|group| group.iter().any(|name| done(name)))
        .count()
}

/// `AdvancementProgress`.
///
/// `criteria` is a `java.util.HashMap<String, CriterionProgress>` in vanilla
/// and a `BTreeMap` here. The difference is observable only in the order the
/// entries are written by `serializeToNetwork`, inside a packet this server
/// cannot compare byte for byte for two other reasons — see the module
/// header — and `CriterionProgress` itself is an `Optional<Instant>`, which
/// is the second of them.
#[derive(Debug, Clone, Default)]
pub struct Progress {
    criteria: BTreeMap<String, Option<i64>>,
    requirements: Vec<Vec<String>>,
}

impl Progress {
    /// `AdvancementProgress.update(AdvancementRequirements)` — offsets 0-80.
    /// Drops every criterion the requirements no longer name, then
    /// `putIfAbsent`s a fresh, ungranted `CriterionProgress` for each one they
    /// do. The `putIfAbsent` is why a datapack reload does not revoke
    /// progress.
    pub fn update(&mut self, requirements: &[Vec<String>]) {
        let names: BTreeSet<&str> = requirements
            .iter()
            .flat_map(|g| g.iter().map(String::as_str))
            .collect();
        self.criteria.retain(|k, _| names.contains(k.as_str()));
        for n in names {
            self.criteria.entry(n.to_string()).or_insert(None);
        }
        self.requirements = requirements.to_vec();
    }

    /// `AdvancementProgress.grantProgress(String)` — true only when the
    /// criterion is known and was not already granted.
    ///
    /// `obtained_millis` stands in for `Instant.now()`. It is a parameter
    /// rather than a call to the clock so that a test can pin it; vanilla
    /// reads the wall clock here and that value reaches the wire.
    pub fn grant(&mut self, criterion: &str, obtained_millis: i64) -> bool {
        match self.criteria.get_mut(criterion) {
            Some(slot @ None) => {
                *slot = Some(obtained_millis);
                true
            }
            _ => false,
        }
    }

    /// `AdvancementProgress.revokeProgress(String)`.
    pub fn revoke(&mut self, criterion: &str) -> bool {
        match self.criteria.get_mut(criterion) {
            Some(slot) if slot.is_some() => {
                *slot = None;
                true
            }
            _ => false,
        }
    }

    /// `AdvancementProgress.isDone()`.
    #[must_use]
    pub fn is_done(&self) -> bool {
        requirements_test(&self.requirements, &|n| self.is_criterion_done(n))
    }

    /// `AdvancementProgress.hasProgress()` — any criterion granted.
    #[must_use]
    pub fn has_progress(&self) -> bool {
        self.criteria.values().any(Option::is_some)
    }

    /// `AdvancementProgress.isCriterionDone(String)`.
    #[must_use]
    pub fn is_criterion_done(&self, criterion: &str) -> bool {
        matches!(self.criteria.get(criterion), Some(Some(_)))
    }

    /// `AdvancementProgress.getPercent()` — 0.0 when there are no
    /// requirements, else satisfied groups over total groups.
    #[must_use]
    pub fn percent(&self) -> f32 {
        if self.requirements.is_empty() {
            return 0.0;
        }
        #[allow(clippy::cast_precision_loss)]
        let n = requirements_count(&self.requirements, &|c| self.is_criterion_done(c)) as f32;
        #[allow(clippy::cast_precision_loss)]
        let d = self.requirements.len() as f32;
        n / d
    }

    /// The granted criteria, by name. The projection the oracle compares —
    /// clock-free, order-free.
    #[must_use]
    pub fn completed(&self) -> Vec<&str> {
        self.criteria
            .iter()
            .filter(|(_, v)| v.is_some())
            .map(|(k, _)| k.as_str())
            .collect()
    }

    /// `AdvancementProgress.serializeToNetwork` — `writeMap` of the criteria,
    /// each value a `writeNullable` of the obtained `Instant` as an epoch
    /// millisecond `long`.
    ///
    /// # Errors
    /// Only from the writer.
    pub fn write(&self, out: &mut Vec<u8>) -> io::Result<()> {
        rustmc_proto::varint::write_varint(out, i32::try_from(self.criteria.len()).unwrap_or(0))?;
        for (name, obtained) in &self.criteria {
            rustmc_proto::wire::write_string(out, name)?;
            match obtained {
                Some(ms) => {
                    rustmc_proto::wire::write_bool(out, true)?;
                    out.extend_from_slice(&ms.to_be_bytes());
                }
                None => rustmc_proto::wire::write_bool(out, false)?,
            }
        }
        Ok(())
    }
}

/// `AdvancementVisibilityEvaluator$VisibilityRule`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisibilityRule {
    Show,
    Hide,
    NoChange,
}

/// `AdvancementVisibilityEvaluator.evaluateVisibilityRule(Advancement,
/// boolean)` — offsets 0-41, in that order: no display is `HIDE`, then a
/// completed advancement is `SHOW`, then a `hidden` display is `HIDE`, else
/// `NO_CHANGE`. The order matters: a *completed* hidden advancement is
/// `SHOW`, because the `done` test comes first.
#[must_use]
pub fn visibility_rule(a: &Advancement, done: bool) -> VisibilityRule {
    if !a.has_display {
        return VisibilityRule::Hide;
    }
    if done {
        return VisibilityRule::Show;
    }
    if a.hidden {
        return VisibilityRule::Hide;
    }
    VisibilityRule::NoChange
}

/// `AdvancementVisibilityEvaluator.VISIBILITY_DEPTH`.
const VISIBILITY_DEPTH: usize = 2;

/// `evaluateVisiblityForUnfinishedNode(Stack)` — the top three entries, in
/// order, first non-`NO_CHANGE` wins; nothing found is `false`. `peek(0)` is
/// fastutil's TOP of stack, so this reads self, parent, grandparent.
pub(crate) fn visible_for_unfinished(stack: &[VisibilityRule]) -> bool {
    for i in 0..=VISIBILITY_DEPTH {
        match stack[stack.len() - 1 - i] {
            VisibilityRule::Show => return true,
            VisibilityRule::Hide => return false,
            VisibilityRule::NoChange => {}
        }
    }
    false
}

/// The advancement forest, built from the `parent` links.
///
/// Children are ordered by resource id. See the module header for why that is
/// a divergence taken deliberately rather than a shortcut.
#[derive(Debug, Default)]
pub struct Tree {
    children: BTreeMap<String, Vec<String>>,
    root_of: BTreeMap<String, String>,
    roots: Vec<String>,
}

impl Tree {
    /// `AdvancementTree.addAll` — an advancement whose parent is not in the
    /// table is not inserted at all (`tryInsert` returns false and the holder
    /// is deferred, then dropped with a warning). Vanilla's own 1,688
    /// documents have no such orphan; a datapack could.
    #[must_use]
    pub fn build(table: &Table) -> Self {
        let mut t = Self::default();
        for a in table.iter() {
            match &a.parent {
                None => t.roots.push(a.id.clone()),
                Some(p) => {
                    if table.get(p).is_some() {
                        t.children.entry(p.clone()).or_default().push(a.id.clone());
                    }
                }
            }
        }
        for kids in t.children.values_mut() {
            kids.sort();
        }
        t.roots.sort();
        // `AdvancementNode.root()` walks parents to the top; precomputed
        // because `markForVisibilityUpdate` asks for it on every award.
        for a in table.iter() {
            let mut cur = a;
            let mut guard = 0usize;
            while let Some(p) = cur.parent.as_ref().and_then(|p| table.get(p)) {
                cur = p;
                guard += 1;
                if guard > table.len() {
                    break; // a cycle a datapack could write; vanilla has none
                }
            }
            t.root_of.insert(a.id.clone(), cur.id.clone());
        }
        t
    }

    #[must_use]
    pub fn roots(&self) -> &[String] {
        &self.roots
    }

    #[must_use]
    pub fn children(&self, id: &str) -> &[String] {
        self.children.get(id).map_or(&[][..], Vec::as_slice)
    }

    /// `AdvancementNode.root()`.
    #[must_use]
    pub fn root_of(&self, id: &str) -> Option<&str> {
        self.root_of.get(id).map(String::as_str)
    }
}

/// The three tables one server process serves, loaded once, so a
/// [`player::PlayerAdvancements`] can borrow them for `'static`.
///
/// `tags` is optional and the other two are not: without
/// `data/vanilla/item_tags.json` the 100 tag-shaped `inventory_changed`
/// predicates cannot be answered, and
/// [`triggers::Fired::unknown_tags`] says so per fire rather than
/// silently matching nothing.
#[derive(Debug, Clone, Copy)]
pub struct Engine {
    pub table: &'static Table,
    pub tree: &'static Tree,
    pub tags: Option<&'static triggers::ItemTags>,
}

impl Engine {
    /// # Errors
    /// Whatever loading `data/vanilla/advancements.json` returned the first
    /// time, repeated. A missing item-tag table is **not** an error.
    pub fn shared() -> Result<Self, String> {
        let table = Table::shared(Path::new("data/vanilla/advancements.json"))?;
        static TREE: std::sync::OnceLock<Tree> = std::sync::OnceLock::new();
        let tree = TREE.get_or_init(|| Tree::build(table));
        let tags = triggers::ItemTags::shared(Path::new("data/vanilla/item_tags.json")).ok();
        Ok(Self { table, tree, tags })
    }

    /// A fresh player's state. Vanilla loads `advancements/<uuid>.json` here;
    /// this server does not persist advancement progress, so every join is a
    /// first join — which is exactly the state
    /// [`player::PlayerAdvancements`]'s header describes as sending no packet
    /// at all.
    #[must_use]
    pub fn new_player(&self) -> player::PlayerAdvancements<'static> {
        player::PlayerAdvancements::new(self.table, self.tree)
    }
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Table {
        // Two roots, one two-deep chain, one hidden leaf, one display-less
        // leaf. Small enough to reason about and shaped to exercise every
        // arm of `visibility_rule`.
        Table::parse(&serde_json::json!({
            "advancements": {
                "t:root": {"parent": null, "requirements": [["a"]],
                    "criteria_triggers": {"a": "minecraft:inventory_changed"},
                    "display": {"hidden": false}, "experience": 0,
                    "reward_recipes": [], "reward_loot": [], "reward_function": false,
                    "holder_wire": "00"},
                "t:child": {"parent": "t:root", "requirements": [["a"], ["b"]],
                    "criteria_triggers": {"a": "minecraft:placed_block",
                                          "b": "minecraft:consume_item"},
                    "display": {"hidden": false}, "experience": 7,
                    "reward_recipes": ["t:r"], "reward_loot": [], "reward_function": false,
                    "holder_wire": "01"},
                "t:hidden": {"parent": "t:child", "requirements": [["a"]],
                    "criteria_triggers": {"a": "minecraft:tick"},
                    "display": {"hidden": true}, "experience": 0,
                    "reward_recipes": [], "reward_loot": [], "reward_function": false,
                    "holder_wire": "02"},
                "t:nodisplay": {"parent": "t:root", "requirements": [["a"]],
                    "criteria_triggers": {"a": "minecraft:recipe_unlocked"},
                    "display": null, "experience": 0,
                    "reward_recipes": ["t:r2"], "reward_loot": [], "reward_function": false,
                    "holder_wire": null}
            }
        }))
        .expect("fixture parses")
    }

    #[test]
    fn table_parses_every_field() {
        let t = fixture();
        assert_eq!(t.len(), 4);
        let c = t.get("t:child").expect("child");
        assert_eq!(c.parent.as_deref(), Some("t:root"));
        assert_eq!(
            c.requirements,
            vec![vec!["a".to_string()], vec!["b".to_string()]]
        );
        assert_eq!(c.experience, 7);
        assert_eq!(c.reward_recipes, vec!["t:r".to_string()]);
        assert_eq!(c.holder_wire.as_deref(), Some(&[1u8][..]));
        assert!(t.get("t:nodisplay").expect("nd").holder_wire.is_none());
    }

    /// The positive control for [`requirements_test`]: the AND-of-ORs must
    /// distinguish all four states of a two-group requirement, not just the
    /// all-or-nothing ends.
    #[test]
    fn requirements_are_and_of_ors() {
        let req = vec![
            vec!["a".to_string(), "b".to_string()],
            vec!["c".to_string()],
        ];
        let done = |set: &[&str]| {
            let owned: Vec<String> = set.iter().map(|s| (*s).to_string()).collect();
            requirements_test(&req, &move |n| owned.iter().any(|o| o == n))
        };
        assert!(!done(&[]));
        assert!(!done(&["a"]), "one group of two satisfied is not done");
        assert!(!done(&["c"]), "the other group alone is not done");
        assert!(done(&["a", "c"]));
        assert!(done(&["b", "c"]), "either member of the OR group serves");
    }

    #[test]
    fn progress_grants_once_and_completes_on_the_last_group() {
        let t = fixture();
        let c = t.get("t:child").expect("child");
        let mut p = Progress::default();
        p.update(&c.requirements);
        assert!(!p.is_done());
        assert!(p.grant("a", 1000));
        assert!(
            !p.grant("a", 2000),
            "a second grant of the same criterion is false"
        );
        assert!(!p.is_done(), "one of two groups");
        assert!((p.percent() - 0.5).abs() < f32::EPSILON);
        assert!(p.grant("b", 3000));
        assert!(p.is_done());
        assert_eq!(p.completed(), vec!["a", "b"]);
        assert!(p.revoke("a"));
        assert!(!p.is_done(), "revoking un-completes it");
    }

    #[test]
    fn progress_ignores_a_criterion_the_requirements_do_not_name() {
        let mut p = Progress::default();
        p.update(&[vec!["a".to_string()]]);
        assert!(!p.grant("nonexistent", 1));
        assert!(!p.has_progress());
    }

    #[test]
    fn update_keeps_granted_progress_and_drops_removed_criteria() {
        let mut p = Progress::default();
        p.update(&[vec!["a".to_string()], vec!["gone".to_string()]]);
        assert!(p.grant("a", 5));
        assert!(p.grant("gone", 6));
        p.update(&[vec!["a".to_string()], vec!["new".to_string()]]);
        assert!(
            p.is_criterion_done("a"),
            "putIfAbsent does not clear a grant"
        );
        assert!(
            !p.is_criterion_done("gone"),
            "a dropped criterion is removed"
        );
        assert!(!p.is_criterion_done("new"));
    }

    #[test]
    fn visibility_rule_order_puts_done_before_hidden() {
        let t = fixture();
        let hidden = t.get("t:hidden").expect("hidden");
        assert_eq!(visibility_rule(hidden, false), VisibilityRule::Hide);
        assert_eq!(
            visibility_rule(hidden, true),
            VisibilityRule::Show,
            "a completed hidden advancement is SHOW — the done test comes first"
        );
        let nd = t.get("t:nodisplay").expect("nd");
        assert_eq!(
            visibility_rule(nd, true),
            VisibilityRule::Hide,
            "no display beats done"
        );
    }

    #[test]
    fn unfinished_visibility_reads_three_deep_and_first_wins() {
        use VisibilityRule::{Hide, NoChange, Show};
        // peek(0) is the top of the stack.
        assert!(visible_for_unfinished(&[
            NoChange, NoChange, NoChange, Show
        ]));
        assert!(!visible_for_unfinished(&[
            NoChange, NoChange, NoChange, Hide
        ]));
        assert!(
            visible_for_unfinished(&[NoChange, Show, NoChange, NoChange]),
            "a SHOW two above still reaches"
        );
        assert!(
            !visible_for_unfinished(&[Show, NoChange, NoChange, NoChange]),
            "four above is outside VISIBILITY_DEPTH"
        );
        assert!(
            !visible_for_unfinished(&[NoChange, Show, Hide, NoChange]),
            "the nearer HIDE wins over the further SHOW"
        );
    }

    #[test]
    fn tree_links_parents_children_and_roots() {
        let t = Tree::build(&fixture());
        assert_eq!(t.roots(), ["t:root".to_string()]);
        assert_eq!(
            t.children("t:root"),
            ["t:child".to_string(), "t:nodisplay".to_string()]
        );
        assert_eq!(t.children("t:child"), ["t:hidden".to_string()]);
        assert!(t.children("t:hidden").is_empty());
        assert_eq!(t.root_of("t:hidden"), Some("t:root"));
        assert_eq!(t.root_of("t:root"), Some("t:root"));
    }

    #[test]
    fn listeners_index_finds_every_criterion_for_a_trigger() {
        let t = fixture();
        let l = t.listeners("minecraft:inventory_changed");
        assert_eq!(l.len(), 1);
        assert_eq!((l[0].0.id.as_str(), l[0].1), ("t:root", "a"));
        assert_eq!(l[0].2.trigger, "minecraft:inventory_changed");
        assert!(t.listeners("minecraft:never_registered").is_empty());
    }
}
