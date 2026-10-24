//! The 26.2 trade tables, as loaded from the two generated extracts.
//!
//! # What a trade is in 26.2, and what it stopped being
//!
//! `VillagerTrades.TRADES` — the `Int2ObjectMap<Int2ObjectMap<ItemListing[]>>`
//! every account of villager trading describes — **is not in the 26.2 jar**,
//! and neither is a single one of the `ItemListing` implementations
//! (`EmeraldForItems`, `ItemsForEmeralds`, `EnchantedItemForEmeralds`,
//! `TreasureMapForEmeralds`, `DyedArmorForEmeralds`, `SuspiciousStewForEmerald`,
//! `TippedArrowForItemsAndEmeralds`, `EnchantBookForEmeralds`,
//! `ItemsAndEmeraldsToItems`, `TypeSpecificTrade`). `unzip -l server-26.2.jar |
//! grep -c ItemListing` is 0.
//!
//! What replaced them is two datapack registries and a tag:
//!
//! * `minecraft:villager_trade` (`Registries.VILLAGER_TRADE`) holds **388**
//!   `VillagerTrade` entries, one per listing, `Codec`-loaded from
//!   `data/minecraft/villager_trade/<profession>/<level>/<name>.json`;
//! * `minecraft:trade_set` (`Registries.TRADE_SET`) holds **68** `TradeSet`
//!   entries — 13 professions x 5 levels plus the wandering trader's three —
//!   each of which is `{trades: #tag, amount, allow_duplicates?, random_sequence?}`;
//! * the pool a set draws from is a **tag over the first registry**, 73 of
//!   them under `data/minecraft/tags/villager_trade/`.
//!
//! The only part that is still Java is the profession -> trade-set map,
//! `VillagerProfession.tradeSetsByLevel`, read through `getTrades(int)`
//! (`VillagerProfession.getTrades`, offsets 0..13) and dumped by the golden
//! from the live registry.
//!
//! # The pool order is load-bearing and it is not the file order
//!
//! `AbstractVillager.addOffersFromItemListings` copies the `HolderSet` into an
//! `ArrayList` and indexes it with `random.nextInt(list.size())`, so the
//! order the tag resolves in decides which listing each draw picks.
//! `TagLoader.tryBuildTag` collects into a **`LinkedHashSet`**: walk `values`
//! in order, splice a `#tag` reference's own resolved list in where it is
//! referenced, and keep the first occurrence of each element. Nine of the
//! seventy pools open with a `#` reference — `armorer/level_1` starts with
//! `#minecraft:common_smith/level_1` — so the first listing in the pool is
//! frequently not the first line of the file. The resolved order is dumped
//! rather than re-derived here.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

/// A `NumberProvider` field, as it appears in these two tables.
///
/// Every one of the 456 providers in the shipped 26.2 data is a bare JSON
/// number, which the codec reads as `ConstantValue.exactly(f)`. The other
/// shapes (`uniform`, `binomial`, `score`, `storage`, `enchantment_level`)
/// are legal in the codec and are carried across as [`Number::Unsupported`]
/// rather than guessed at, so that a datapack using one **refuses at the
/// point of use** with the type name instead of silently rolling a constant.
#[derive(Debug, Clone, PartialEq)]
pub enum Number {
    /// `ConstantValue`, the only provider vanilla's own data uses here.
    Constant(f32),
    /// Any other `NumberProvider` type, kept by name for the refusal message.
    Unsupported(String),
}

impl Number {
    fn parse(v: Option<&Value>, default: f32) -> Self {
        match v {
            None | Some(Value::Null) => Number::Constant(default),
            Some(Value::Number(n)) => Number::Constant(n.as_f64().unwrap_or(0.0) as f32),
            Some(Value::Object(o)) => match o.get("type").and_then(Value::as_str) {
                Some("minecraft:constant") | Some("constant") => {
                    Number::parse(o.get("value"), default)
                }
                Some(other) => Number::Unsupported(other.to_string()),
                None => Number::Unsupported("<no type>".to_string()),
            },
            Some(other) => Number::Unsupported(format!("{other}")),
        }
    }

    /// `NumberProvider.getInt` for the constant case.
    ///
    /// Vanilla's `ConstantValue.getInt` is `Mth.floor(getFloat(ctx))`, not a
    /// round: `ConstantValue.getInt` -> `NumberProvider.getInt` default ->
    /// `Mth.floor(this.getFloat(ctx))`. Every shipped value is integral so
    /// the two agree on all 456, which is exactly why this has to come from
    /// the bytecode rather than from the data.
    #[must_use]
    pub fn as_int(&self) -> Option<i32> {
        match self {
            Number::Constant(f) => Some(f.floor() as i32),
            Number::Unsupported(_) => None,
        }
    }

    #[must_use]
    pub fn as_float(&self) -> Option<f32> {
        match self {
            Number::Constant(f) => Some(*f),
            Number::Unsupported(_) => None,
        }
    }
}

/// `TradeCost` — what the player hands over. `count` defaults to
/// `ConstantValue.exactly(1.0f)` (`TradeCost.CODEC` offsets 22..28).
#[derive(Debug, Clone, PartialEq)]
pub struct TradeCost {
    pub item: String,
    pub count: Number,
    /// `DataComponentExactPredicate`, carried verbatim. One listing in 26.2
    /// uses it and nothing here interprets it.
    pub components: Option<Value>,
}

impl TradeCost {
    fn parse(v: &Value) -> Option<Self> {
        let o = v.as_object()?;
        Some(TradeCost {
            item: o.get("id")?.as_str()?.to_string(),
            count: Number::parse(o.get("count"), 1.0),
            components: o.get("components").cloned(),
        })
    }
}

/// `ItemStackTemplate` — what the villager hands back, before
/// `given_item_modifiers` run. `count` defaults to 1 and is an `int`, not a
/// `NumberProvider` (`ItemStackTemplate.CODEC` offsets 19..31).
#[derive(Debug, Clone, PartialEq)]
pub struct StackTemplate {
    pub item: String,
    pub count: i32,
    pub components: Option<Value>,
}

impl StackTemplate {
    fn parse(v: &Value) -> Option<Self> {
        let o = v.as_object()?;
        Some(StackTemplate {
            item: o.get("id")?.as_str()?.to_string(),
            count: o
                .get("count")
                .and_then(Value::as_i64)
                .map_or(1, |n| n as i32),
            components: o.get("components").cloned(),
        })
    }
}

/// One `VillagerTrade`.
///
/// The three loot-system fields are held as JSON and interpreted nowhere in
/// this module. That is deliberate and is the brief's own rule: a listing
/// whose result is enchanted, dyed, brewed or turned into an explorer map
/// draws at **offer-creation** time, so recording a rolled instance would be
/// recording one sample and calling it the table. What the shape is, is here;
/// what a roll of it produces belongs to the loot crate, which already owns
/// `LootItemFunction` and `LootItemCondition`.
#[derive(Debug, Clone, PartialEq)]
pub struct Listing {
    pub id: String,
    pub wants: TradeCost,
    pub additional_wants: Option<TradeCost>,
    pub gives: StackTemplate,
    pub max_uses: Number,
    pub reputation_discount: Number,
    pub xp: Number,
    /// `LootItemCondition`; when present and false, `getOffer` returns null and
    /// the listing is dropped from the pool for this villager.
    pub merchant_predicate: Option<Value>,
    /// `List<LootItemFunction>`, applied in order to the result stack.
    pub given_item_modifiers: Vec<Value>,
    /// `HolderSet<Enchantment>`; if the result carries any stored enchantment
    /// in this set, the accumulated additional cost is doubled.
    pub double_trade_price_enchantments: Option<Value>,
    /// Which fields the pack file did **not** carry, so that a defaulted 4 is
    /// distinguishable from a written 4. Without this a check on the defaults
    /// can only compare values, and a value check passes whether or not the
    /// default was ever applied.
    pub defaulted_fields: Vec<String>,
}

/// One `TradeSet`: how many listings to draw from which pool, and whether a
/// listing may be drawn twice.
#[derive(Debug, Clone, PartialEq)]
pub struct TradeSet {
    pub id: String,
    /// The tag id, with the leading `#` stripped and the namespace filled in.
    pub pool: String,
    pub amount: Number,
    /// `TradeSet.CODEC` defaults this to **false** and every one of the 68
    /// shipped sets omits it, so every vanilla draw today goes through
    /// `addOffersFromItemListingsWithoutDuplicates`.
    pub allow_duplicates: bool,
    pub random_sequence: Option<String>,
}

/// Both extracts, loaded.
#[derive(Debug, Clone)]
pub struct TradeTables {
    /// profession id -> level (1..=5) -> trade set id.
    professions: BTreeMap<String, BTreeMap<u8, String>>,
    trade_sets: BTreeMap<String, TradeSet>,
    /// tag id -> resolved listing ids, in `TagLoader` order.
    pools: BTreeMap<String, Vec<String>>,
    listings: BTreeMap<String, Listing>,
    /// `Item.getDefaultMaxStackSize()` for every item these listings name.
    /// `TradeCost.toItemCost` clamps a cost count to it and
    /// `MerchantOffer.getModifiedCostCount` clamps the modified count to it
    /// again, and 87 of the 243 items a villager trades are not 64 — buckets,
    /// beds, boats and tools are 1 and banners and ender pearls are 16. It is
    /// not in `items.json`, which is id to protocol index.
    item_stack_sizes: BTreeMap<String, i32>,
}

/// Why a table would not load. Distinguished from "the table says no" on
/// purpose: an absent table and a profession with no trades are different
/// answers and collapsing them is how a nitwit and a farmer become the same.
#[derive(Debug)]
pub enum LoadError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Malformed(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Io(e) => write!(f, "{e}"),
            LoadError::Json(e) => write!(f, "{e}"),
            LoadError::Malformed(s) => write!(f, "{s}"),
        }
    }
}

impl TradeTables {
    /// Load both halves. They are one registry split in two files, so both are
    /// required: a server with only the professions' half would hand a
    /// wandering trader an empty offer list and look like it had traded.
    ///
    /// # Errors
    ///
    /// Any read or parse failure, and any structural claim the extract makes
    /// about itself that does not hold.
    pub fn load(villager: &Path, wandering: &Path) -> Result<Self, LoadError> {
        let mut t = TradeTables {
            professions: BTreeMap::new(),
            trade_sets: BTreeMap::new(),
            pools: BTreeMap::new(),
            listings: BTreeMap::new(),
            item_stack_sizes: BTreeMap::new(),
        };
        t.merge(villager)?;
        t.merge(wandering)?;
        Ok(t)
    }

    /// The two paths this crate reads, spelled as the quoted literals
    /// `ci/gen-gameplay-tables.sh` derives the required table set from. A
    /// `concat!` or a `.join("../../data/vanilla/x.json")` here would be
    /// invisible to that grep and the tables would never be generated in a
    /// gate directory — `ci/check-generator-arms.sh` exists because that
    /// happened.
    #[must_use]
    pub fn default_paths() -> (&'static str, &'static str) {
        (
            "data/vanilla/villager_trades.json",
            "data/vanilla/wandering_trader_trades.json",
        )
    }

    fn merge(&mut self, path: &Path) -> Result<(), LoadError> {
        let raw = std::fs::read_to_string(path).map_err(LoadError::Io)?;
        let v: Value = serde_json::from_str(&raw).map_err(LoadError::Json)?;

        if let Some(arr) = v.get("professions").and_then(Value::as_array) {
            for p in arr {
                let id = p
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| LoadError::Malformed("profession with no id".into()))?;
                let mut levels = BTreeMap::new();
                if let Some(m) = p.get("tradeSetsByLevel").and_then(Value::as_object) {
                    for (k, s) in m {
                        let level: u8 = k.parse().map_err(|_| {
                            LoadError::Malformed(format!("profession {id}: level key {k:?}"))
                        })?;
                        let set = s.as_str().ok_or_else(|| {
                            LoadError::Malformed(format!("profession {id} level {level}"))
                        })?;
                        levels.insert(level, set.to_string());
                    }
                }
                self.professions.insert(id.to_string(), levels);
            }
        }

        for s in v
            .get("tradeSets")
            .and_then(Value::as_array)
            .ok_or_else(|| LoadError::Malformed(format!("{}: no tradeSets", path.display())))?
        {
            let id = s
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| LoadError::Malformed("trade set with no id".into()))?;
            let pool = s
                .get("trades")
                .and_then(Value::as_str)
                .ok_or_else(|| LoadError::Malformed(format!("trade set {id}: trades")))?;
            let pool = qualify(pool.strip_prefix('#').ok_or_else(|| {
                LoadError::Malformed(format!("trade set {id}: {pool} is not a tag reference"))
            })?);
            self.trade_sets.insert(
                id.to_string(),
                TradeSet {
                    id: id.to_string(),
                    pool,
                    amount: Number::parse(s.get("amount"), 0.0),
                    allow_duplicates: s
                        .get("allowDuplicates")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    random_sequence: s
                        .get("randomSequence")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                },
            );
        }

        for t in v
            .get("tags")
            .and_then(Value::as_array)
            .ok_or_else(|| LoadError::Malformed(format!("{}: no tags", path.display())))?
        {
            let id = t
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| LoadError::Malformed("tag with no id".into()))?;
            let resolved = t
                .get("resolved")
                .and_then(Value::as_array)
                .ok_or_else(|| LoadError::Malformed(format!("tag {id}: no resolved order")))?
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect();
            self.pools.insert(id.to_string(), resolved);
        }

        if let Some(m) = v.get("itemStackSizes").and_then(Value::as_object) {
            for (k, n) in m {
                let n = n.as_i64().ok_or_else(|| {
                    LoadError::Malformed(format!("stack size for {k} is not a number"))
                })?;
                self.item_stack_sizes.insert(k.clone(), n as i32);
            }
        }

        for l in v
            .get("trades")
            .and_then(Value::as_array)
            .ok_or_else(|| LoadError::Malformed(format!("{}: no trades", path.display())))?
        {
            let id = l
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| LoadError::Malformed("listing with no id".into()))?;
            let wants = l
                .get("wants")
                .and_then(TradeCost::parse)
                .ok_or_else(|| LoadError::Malformed(format!("listing {id}: wants")))?;
            let gives = l
                .get("gives")
                .and_then(StackTemplate::parse)
                .ok_or_else(|| LoadError::Malformed(format!("listing {id}: gives")))?;
            self.listings.insert(
                id.to_string(),
                Listing {
                    id: id.to_string(),
                    wants,
                    additional_wants: l.get("additionalWants").and_then(TradeCost::parse),
                    gives,
                    max_uses: Number::parse(l.get("maxUses"), 4.0),
                    reputation_discount: Number::parse(l.get("reputationDiscount"), 0.0),
                    xp: Number::parse(l.get("xp"), 1.0),
                    merchant_predicate: l
                        .get("merchantPredicate")
                        .filter(|x| !x.is_null())
                        .cloned(),
                    given_item_modifiers: l
                        .get("givenItemModifiers")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default(),
                    double_trade_price_enchantments: l
                        .get("doubleTradePriceEnchantments")
                        .filter(|x| !x.is_null())
                        .cloned(),
                    defaulted_fields: l
                        .get("defaultedFields")
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter()
                                .filter_map(Value::as_str)
                                .map(str::to_string)
                                .collect()
                        })
                        .unwrap_or_default(),
                },
            );
        }
        Ok(())
    }

    /// `VillagerProfession.getTrades(level)`. `None` where vanilla returns
    /// null — `none` and `nitwit`, and any level outside 1..=5 — which
    /// `Villager.updateTrades` treats as "add no offers at all" rather than
    /// as an empty draw.
    #[must_use]
    pub fn trade_set_for(&self, profession: &str, level: u8) -> Option<&TradeSet> {
        let id = self.professions.get(profession)?.get(&level)?;
        self.trade_sets.get(id)
    }

    #[must_use]
    pub fn trade_set(&self, id: &str) -> Option<&TradeSet> {
        self.trade_sets.get(id)
    }

    /// The pool a set draws from, in `TagLoader` order.
    #[must_use]
    pub fn pool(&self, tag: &str) -> Option<&[String]> {
        self.pools.get(tag).map(Vec::as_slice)
    }

    /// `Item.getDefaultMaxStackSize()`. `None` for an item no listing names,
    /// which a caller must treat as a refusal rather than defaulting to 64:
    /// 87 of the 243 items in these tables are not 64.
    #[must_use]
    pub fn max_stack_size(&self, item: &str) -> Option<i32> {
        self.item_stack_sizes.get(item).copied()
    }

    #[must_use]
    pub fn item_count(&self) -> usize {
        self.item_stack_sizes.len()
    }

    #[must_use]
    pub fn listing(&self, id: &str) -> Option<&Listing> {
        self.listings.get(id)
    }

    /// Every listing in both tables, in id order.
    pub fn listings(&self) -> impl Iterator<Item = &Listing> {
        self.listings.values()
    }

    #[must_use]
    pub fn listing_count(&self) -> usize {
        self.listings.len()
    }

    #[must_use]
    pub fn trade_set_count(&self) -> usize {
        self.trade_sets.len()
    }

    #[must_use]
    pub fn pool_count(&self) -> usize {
        self.pools.len()
    }

    /// Professions that have any trade set at all, sorted.
    #[must_use]
    pub fn professions_with_trades(&self) -> Vec<&str> {
        self.professions
            .iter()
            .filter(|(_, m)| !m.is_empty())
            .map(|(k, _)| k.as_str())
            .collect()
    }
}

fn qualify(id: &str) -> String {
    if id.contains(':') {
        id.to_string()
    } else {
        format!("minecraft:{id}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_number_is_a_constant_provider_and_anything_else_refuses() {
        assert_eq!(
            Number::parse(Some(&serde_json::json!(16.0)), 4.0),
            Number::Constant(16.0)
        );
        assert_eq!(Number::parse(None, 4.0), Number::Constant(4.0));
        assert_eq!(
            Number::parse(
                Some(&serde_json::json!({"type": "minecraft:constant", "value": 3.0})),
                0.0
            ),
            Number::Constant(3.0)
        );
        // The positive control for the refusal: a provider we do not
        // implement must not answer, and must say which one it was.
        let u = Number::parse(
            Some(&serde_json::json!({"type": "minecraft:uniform", "min": 1.0, "max": 4.0})),
            0.0,
        );
        assert_eq!(u, Number::Unsupported("minecraft:uniform".into()));
        assert_eq!(u.as_int(), None);
        assert_eq!(u.as_float(), None);
    }

    /// `ConstantValue.getInt` is `Mth.floor`, not a round. 1.9 is 1.
    #[test]
    fn get_int_floors() {
        assert_eq!(Number::Constant(1.9).as_int(), Some(1));
        assert_eq!(Number::Constant(2.0).as_int(), Some(2));
        assert_eq!(Number::Constant(-0.5).as_int(), Some(-1));
    }

    #[test]
    fn costs_default_their_count_to_one() {
        let c = TradeCost::parse(&serde_json::json!({"id": "minecraft:wheat"})).unwrap();
        assert_eq!(c.count, Number::Constant(1.0));
        let g = StackTemplate::parse(&serde_json::json!({"id": "minecraft:emerald"})).unwrap();
        assert_eq!(g.count, 1);
        let g2 = StackTemplate::parse(&serde_json::json!({"id": "minecraft:emerald", "count": 3}))
            .unwrap();
        assert_eq!(g2.count, 3);
    }
}
