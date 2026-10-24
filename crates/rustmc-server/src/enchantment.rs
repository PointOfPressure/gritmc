//! The enchantment registry, and the four questions the rest of this area
//! asks of it.
//!
//! # An enchantment is data in 26.2, and that is the whole shape of this file
//!
//! There is one `Enchantment` class in the jar and it is `final`. Everything
//! that used to be a subclass — `SharpnessEnchantment`, `ProtectionEnchantment`
//! and the rest — is now a **datapack registry entry**: a JSON document
//! carrying `weight`, `max_level`, `min_cost`, `max_cost`, `anvil_cost`,
//! `slots`, `supported_items`, an optional `primary_items`, an optional
//! `exclusive_set` and an `effects` map. All 43 ship inside the jar under
//! `data/minecraft/enchantment/`.
//!
//! So this module transcribes the *accessors* out of `Enchantment.class` and
//! reads everything else. The accessors are four lines of bytecode each and
//! they are quoted where they are implemented.
//!
//! # `getMinLevel` is a constant and it is not 0
//!
//! ```text
//!   public int getMinLevel();
//!     0: iconst_1
//!     1: ireturn
//! ```
//!
//! Two instructions, no field. `getAvailableEnchantmentResults` walks
//! `level` down from `getMaxLevel()` and stops when it goes below this, so a 0
//! here would let the table offer a level-0 enchantment — which is what
//! `ItemEnchantments` treats as *absent*, so the offer would appear on the
//! wire and do nothing.
//!
//! # `areCompatible` is symmetric by construction, not by data
//!
//! ```text
//!   public static boolean areCompatible(Holder a, Holder b);
//!      0: a.equals(b)                    -> false
//!     10: a.value().exclusiveSet.contains(b) -> false
//!     31: b.value().exclusiveSet.contains(a) -> false
//!     52: true
//! ```
//!
//! Both directions are tested, so a one-sided `exclusive_set` in the data
//! still excludes both ways. [`Registry::are_compatible`] reproduces the
//! three tests in that order; writing it as one lookup would be correct today
//! and would silently stop being correct the first time a datapack declared
//! only one side.
//!
//! # What is NOT here, named rather than approximated
//!
//! * **the `effects` map's behaviour.** Each entry's `effects` is a map from
//!   an effect-component id (`minecraft:damage`, `minecraft:damage_protection`,
//!   `minecraft:post_attack`, …) to a list of conditional value effects. Only
//!   the *component ids* are kept here, so that a reader can say which effects
//!   an enchantment has; running them needs the combat and durability paths
//!   this crate does not have (`EnchantmentHelper.modifyDamage`,
//!   `processDurabilityChange`). See [`Enchantment::effects`].
//! * **`slots`.** Kept as the raw `EquipmentSlotGroup` names. Nothing reads
//!   them until something equips armour.

use std::collections::{BTreeMap, BTreeSet};
use std::io;

/// Where the extract lives, relative to the server's working directory.
///
/// **Load-bearing outside Rust**: `ci/gen-gameplay-tables.sh` derives the set
/// of tables a tree needs by grepping the repo for
/// `"data/vanilla/<name>.json"`, and `ci/check-generator-arms.sh` refuses a
/// generator arm no such literal reaches.
pub const TABLE_PATH: &str = "data/vanilla/enchantments.json";

/// `Enchantment$Cost` — a record of two ints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cost {
    pub base: i32,
    pub per_level_above_first: i32,
}

impl Cost {
    /// `Enchantment$Cost.calculate(int)`:
    ///
    /// ```text
    ///   0: getfield base
    ///   4: getfield perLevelAboveFirst
    ///   8: iload_1; iconst_1; isub; imul; iadd; ireturn
    /// ```
    ///
    /// `base + perLevelAboveFirst * (level - 1)`. Wrapping, because Java's
    /// `int` arithmetic is: a datapack with a large `per_level_above_first`
    /// overflows there and must overflow identically here rather than panic
    /// in a debug build.
    #[must_use]
    pub const fn calculate(self, level: i32) -> i32 {
        self.base.wrapping_add(
            self.per_level_above_first
                .wrapping_mul(level.wrapping_sub(1)),
        )
    }
}

/// One registry entry.
#[derive(Debug, Clone)]
pub struct Enchantment {
    pub id: String,
    /// `EnchantmentDefinition.weight()`, read by `WeightedRandom`.
    pub weight: i32,
    /// `EnchantmentDefinition.maxLevel()`.
    pub max_level: i32,
    /// `EnchantmentDefinition.anvilCost()`, read by `AnvilMenu.createResult`.
    pub anvil_cost: i32,
    pub min_cost: Cost,
    pub max_cost: Cost,
    /// The raw `EquipmentSlotGroup` names. Nothing reads them yet.
    pub slots: Vec<String>,
    /// `EnchantmentDefinition.supportedItems()`, resolved from its tag by the
    /// dumper. Membership, so a set.
    pub supported_items: BTreeSet<String>,
    /// `EnchantmentDefinition.primaryItems()`. `None` when the entry omits
    /// the field, which `isPrimaryItem` treats as "every supported item is
    /// primary" rather than as "none are".
    pub primary_items: Option<BTreeSet<String>>,
    /// The `exclusiveSet` field, resolved. Empty for an entry with no field.
    pub exclusive_set: BTreeSet<String>,
    /// The keys of the `effects` map — the effect-component ids this
    /// enchantment carries. Sorted; the values are not read here.
    pub effects: Vec<String>,
    /// The `minecraft:item_damage` component's conditional effects, raw, in
    /// datapack order — what `crate::item_damage` interprets. Empty for every
    /// enchantment but Unbreaking in 26.2.
    pub item_damage: Vec<serde_json::Value>,
}

impl Enchantment {
    /// `Enchantment.getMinLevel()` — `iconst_1; ireturn`.
    #[must_use]
    pub const fn min_level() -> i32 {
        1
    }

    /// `Enchantment.getMinCost(int)` — `definition.minCost().calculate(level)`.
    #[must_use]
    pub const fn min_cost_at(&self, level: i32) -> i32 {
        self.min_cost.calculate(level)
    }

    /// `Enchantment.getMaxCost(int)`.
    #[must_use]
    pub const fn max_cost_at(&self, level: i32) -> i32 {
        self.max_cost.calculate(level)
    }

    /// `Enchantment.isSupportedItem(ItemStack)` — `stack.is(supportedItems)`.
    #[must_use]
    pub fn is_supported_item(&self, item: &str) -> bool {
        self.supported_items.contains(item)
    }

    /// `Enchantment.isPrimaryItem(ItemStack)`:
    ///
    /// ```text
    ///    0: isSupportedItem(stack)      false -> 45 (false)
    ///    8: definition.primaryItems.isEmpty()  true -> 41 (true)
    ///   21: stack.is(primaryItems.get())
    /// ```
    ///
    /// Note the order: an unsupported item is not primary even if it is in
    /// `primary_items`, and an entry with **no** `primary_items` field has
    /// every supported item as primary. Reading the second arm as "no primary
    /// items" would make `getAvailableEnchantmentResults` offer nothing on a
    /// non-book for the 19 entries that omit the field.
    #[must_use]
    pub fn is_primary_item(&self, item: &str) -> bool {
        if !self.is_supported_item(item) {
            return false;
        }
        self.primary_items.as_ref().is_none_or(|p| p.contains(item))
    }
}

/// The whole table.
#[derive(Debug, Clone)]
pub struct Registry {
    /// Registry order, sorted by identifier — see
    /// `tools/goldengen/vanilla/EnchantmentGolden.java` for why that sort is
    /// the live server's order and the bootstrap's is not. The index into
    /// this is what `EnchantmentMenu` writes into `enchantClue`.
    order: Vec<String>,
    by_id: BTreeMap<String, Enchantment>,
    /// `EnchantmentTags.IN_ENCHANTING_TABLE`, resolved in `TagLoader` order.
    /// **Order is load-bearing** — `WeightedRandom.getWeightedItem` subtracts
    /// weights walking a list built by streaming this.
    in_enchanting_table: Vec<String>,
    /// Every enchantment tag, resolved, in `TagLoader` order.
    ///
    /// The table's own generator asserts that `#in_enchanting_table` is
    /// exactly `#non_treasure` and that `#treasure` and `#non_treasure`
    /// partition the 43 — the two properties the enchanting table's
    /// reachability rests on. The rest are what the loot functions and the
    /// trade providers select over.
    tags: BTreeMap<String, Vec<String>>,
    /// `DataComponents.ENCHANTABLE`, per item. Absent means unenchantable,
    /// which `getEnchantmentCost` answers 0 for.
    enchantability: BTreeMap<String, i32>,
    /// `DataComponents.MAX_DAMAGE`, per item. Absent means not damageable.
    max_damage: BTreeMap<String, i32>,
    /// `DataComponents.REPAIRABLE`, resolved to the material items that
    /// repair each tool.
    repairable: BTreeMap<String, BTreeSet<String>>,
}

fn bad(what: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("enchantments.json: {what}"),
    )
}

fn int(v: &serde_json::Value, key: &str) -> io::Result<i32> {
    v.get(key)
        .and_then(serde_json::Value::as_i64)
        .and_then(|n| i32::try_from(n).ok())
        .ok_or_else(|| bad(&format!("no int `{key}`")))
}

fn cost(v: &serde_json::Value, key: &str) -> io::Result<Cost> {
    let c = v.get(key).ok_or_else(|| bad(&format!("no `{key}`")))?;
    Ok(Cost {
        base: int(c, "base")?,
        per_level_above_first: int(c, "per_level_above_first")?,
    })
}

fn strings(v: Option<&serde_json::Value>) -> Vec<String> {
    v.and_then(serde_json::Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(ToString::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// The process-wide registry, loaded once at boot by
/// `crate::session_tables::SessionTables::load`.
static SHARED: std::sync::OnceLock<Option<Registry>> = std::sync::OnceLock::new();

impl Registry {
    /// Load the process-wide copy from `path` if nothing has yet, and return
    /// it. The first call decides; a missing table is `None` for the life of
    /// the process, which is a server whose anvil and enchanting table
    /// compute nothing rather than one that guesses.
    pub fn load_shared(path: &std::path::Path) -> Option<&'static Self> {
        SHARED.get_or_init(|| Self::load(path).ok()).as_ref()
    }

    /// The process-wide copy, if [`Self::load_shared`] found one.
    #[must_use]
    pub fn shared() -> Option<&'static Self> {
        SHARED.get().and_then(Option::as_ref)
    }

    /// Load from an explicit path.
    ///
    /// # Errors
    /// If the file is missing or malformed.
    pub fn load(path: &std::path::Path) -> io::Result<Self> {
        let doc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path)?)
            .map_err(|e| bad(&format!("not json: {e}")))?;
        Self::from_json(&doc)
    }

    /// Parse the dumper's output.
    ///
    /// # Errors
    /// If `order`, `enchantments` or `resolved` is missing or inconsistent.
    /// An entry named in `order` with no definition is an error and not a
    /// skip: a silently short registry offers fewer enchantments and shifts
    /// every `enchantClue` id above the hole.
    pub fn from_json(doc: &serde_json::Value) -> io::Result<Self> {
        let order: Vec<String> = strings(doc.get("order"));
        if order.is_empty() {
            return Err(bad("no `order` array"));
        }
        let defs = doc
            .get("enchantments")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| bad("no `enchantments` object"))?;
        let resolved = doc
            .get("resolved")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| bad("no `resolved` object"))?;

        let mut by_id = BTreeMap::new();
        for id in &order {
            let d = defs
                .get(id)
                .ok_or_else(|| bad(&format!("{id}: no definition")))?;
            let r = resolved
                .get(id)
                .ok_or_else(|| bad(&format!("{id}: no resolved sets")))?;
            let primary = r
                .get("primary_items")
                .filter(|v| !v.is_null())
                .map(|v| strings(Some(v)).into_iter().collect::<BTreeSet<_>>());
            let mut effects: Vec<String> = d
                .get("effects")
                .and_then(serde_json::Value::as_object)
                .map(|m| m.keys().cloned().collect())
                .unwrap_or_default();
            effects.sort();
            by_id.insert(
                id.clone(),
                Enchantment {
                    id: id.clone(),
                    weight: int(d, "weight")?,
                    max_level: int(d, "max_level")?,
                    anvil_cost: int(d, "anvil_cost")?,
                    min_cost: cost(d, "min_cost")?,
                    max_cost: cost(d, "max_cost")?,
                    slots: strings(d.get("slots")),
                    supported_items: strings(r.get("supported_items")).into_iter().collect(),
                    primary_items: primary,
                    exclusive_set: strings(r.get("exclusive_set")).into_iter().collect(),
                    effects,
                    item_damage: d
                        .get("effects")
                        .and_then(|e| e.get("minecraft:item_damage"))
                        .and_then(serde_json::Value::as_array)
                        .cloned()
                        .unwrap_or_default(),
                },
            );
        }

        let mut enchantability = BTreeMap::new();
        if let Some(m) = doc
            .get("enchantability")
            .and_then(serde_json::Value::as_object)
        {
            for (k, v) in m {
                if let Some(n) = v.as_i64().and_then(|n| i32::try_from(n).ok()) {
                    enchantability.insert(k.clone(), n);
                }
            }
        }
        let mut max_damage = BTreeMap::new();
        if let Some(m) = doc.get("max_damage").and_then(serde_json::Value::as_object) {
            for (k, v) in m {
                if let Some(n) = v.as_i64().and_then(|n| i32::try_from(n).ok()) {
                    max_damage.insert(k.clone(), n);
                }
            }
        }
        let mut repairable = BTreeMap::new();
        if let Some(m) = doc.get("repairable").and_then(serde_json::Value::as_object) {
            for (k, v) in m {
                repairable.insert(k.clone(), strings(Some(v)).into_iter().collect());
            }
        }

        let mut tags = BTreeMap::new();
        if let Some(m) = doc.get("tags").and_then(serde_json::Value::as_object) {
            for (k, v) in m {
                tags.insert(k.clone(), strings(Some(v)));
            }
        }

        Ok(Self {
            order,
            by_id,
            in_enchanting_table: strings(doc.get("in_enchanting_table")),
            tags,
            enchantability,
            max_damage,
            repairable,
        })
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.order.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// Registry order — the id `EnchantmentMenu` puts in `enchantClue`.
    #[must_use]
    pub fn order(&self) -> &[String] {
        &self.order
    }

    /// `Registry.asHolderIdMap().getId(holder)`.
    #[must_use]
    pub fn numeric_id(&self, id: &str) -> Option<i32> {
        self.order
            .iter()
            .position(|x| x == id)
            .and_then(|i| i32::try_from(i).ok())
    }

    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Enchantment> {
        self.by_id.get(id)
    }

    /// The `#minecraft:in_enchanting_table` members, in tag order.
    #[must_use]
    pub fn in_enchanting_table(&self) -> &[String] {
        &self.in_enchanting_table
    }

    /// Any enchantment tag, by its full id, in `TagLoader` order.
    ///
    /// `None` for a tag the datapack does not define — which is different
    /// from an empty tag, and the difference matters to
    /// `EnchantmentMenu.getEnchantmentList`: an *absent* `IN_ENCHANTING_TABLE`
    /// returns `List.of()` at its offset 46, whereas an empty one would go on
    /// to `selectEnchantment` and take its four prologue draws.
    #[must_use]
    pub fn tag(&self, id: &str) -> Option<&[String]> {
        self.tags.get(id).map(Vec::as_slice)
    }

    /// The tag names the table holds.
    pub fn tag_names(&self) -> impl Iterator<Item = &String> {
        self.tags.keys()
    }

    /// `ItemStack.get(DataComponents.ENCHANTABLE)` — `None` for an item with
    /// no such component, which is what makes `getEnchantmentCost` return 0.
    #[must_use]
    pub fn enchantability(&self, item: &str) -> Option<i32> {
        self.enchantability.get(item).copied()
    }

    /// `ItemStack.getMaxDamage()`. `None` means `isDamageableItem()` is false.
    #[must_use]
    pub fn max_damage(&self, item: &str) -> Option<i32> {
        self.max_damage.get(item).copied()
    }

    /// `ItemStack.isValidRepairItem(ItemStack)` — membership of the tool's
    /// `REPAIRABLE` item set. An item with no component repairs from nothing.
    #[must_use]
    pub fn is_valid_repair_item(&self, tool: &str, material: &str) -> bool {
        self.repairable
            .get(tool)
            .is_some_and(|m| m.contains(material))
    }

    /// `Enchantment.areCompatible(Holder, Holder)`, in its own three-test
    /// order. Unknown ids are compatible with everything, which is what the
    /// bytecode does with a holder whose `exclusiveSet` is empty.
    #[must_use]
    pub fn are_compatible(&self, a: &str, b: &str) -> bool {
        if a == b {
            return false;
        }
        if self.get(a).is_some_and(|e| e.exclusive_set.contains(b)) {
            return false;
        }
        if self.get(b).is_some_and(|e| e.exclusive_set.contains(a)) {
            return false;
        }
        true
    }

    /// `EnchantmentHelper.isEnchantmentCompatible(Collection, Holder)` — every
    /// member of `held` must be compatible with `candidate`.
    #[must_use]
    pub fn is_enchantment_compatible(&self, held: &[String], candidate: &str) -> bool {
        held.iter().all(|h| self.are_compatible(h, candidate))
    }
}
