//! Villager trading — the trade table, and the deal that turns a level's pool
//! into a merchant's offers.
//!
//! # 26.2 moved the trades out of the jar, and that changes what "transcribed"
//! means here
//!
//! Villager trades used to be `VillagerTrades.ItemListing` lambdas: to know
//! what a farmer sells you had to instantiate one against a seeded
//! `RandomSource` and record what came out. In 26.2
//! `net.minecraft.world.item.trading.VillagerTrades` is a list of
//! `ResourceKey<VillagerTrade>` constants and nothing else, and the trades are
//! **datapack registry data**:
//!
//! ```text
//!   data/minecraft/villager_trade/<profession>/<level>/<name>.json   388
//!   data/minecraft/tags/villager_trade/<profession>/level_N.json      73
//!   data/minecraft/trade_set/<profession>/level_N.json                68
//! ```
//!
//! So the table is *read*, not derived: no JVM, no RNG, no lambda, and no
//! golden to compare against because the extract itself is the reference.
//! `ci/dump-villager-trades.sh` reads it into `data/vanilla/villager_trade_registry.json`
//! and cross-checks it both ways — every tag entry names a trade that exists,
//! every trade is reachable from some tag, every trade set points at a tag
//! that exists.
//!
//! Tags nest: `toolsmith/level_4` is `["#minecraft:common_smith/level_4", …]`,
//! and eight of the 73 pools use a reference. The dumper resolves them
//! transitively, in vanilla's order, splicing a referenced tag's members in
//! where the reference sits.
//!
//! # What is checked and what is not — stated plainly
//!
//! * **The table is exact.** It is vanilla's own data, and the dumper refuses
//!   rather than flattening anything it did not expect.
//! * **The deal loop is transcribed from bytecode and has no oracle.**
//!   [`deal_offers`] reproduces
//!   `AbstractVillager.addOffersFromItemListings` and
//!   `…WithoutDuplicates` instruction for instruction, but running vanilla's
//!   own version needs a `LootContext`, which needs a `ServerLevel`, which is
//!   a whole server boot. Nothing here compares it against a vanilla run. The
//!   draw *order* is pinned by [`tests`] against a scripted RNG so a change to
//!   it is visible, and that is a regression check, not a parity check.
//! * **`VillagerTrade.getOffer` is not implemented.** It applies
//!   `given_item_modifiers` (loot functions: `enchant_with_levels`,
//!   `exploration_map`, `set_potion`, …) and returns null when
//!   `merchant_predicate` fails. Both are the loot layer's, so [`deal_offers`]
//!   takes the offer decision as a callback rather than guessing at it. 41 of
//!   the 388 trades carry modifiers and 27 carry a predicate; the table keeps
//!   both verbatim so the layer that owns them has them.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

/// `net.minecraft.world.item.trading.ItemCost`, or the `gives` side's
/// `ItemStackTemplate` — both are an item and a count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemCost {
    pub id: String,
    pub count: i32,
    /// The exact-component predicate, verbatim as JSON. Exactly one trade in
    /// 26.2 has one — the wandering trader's water bottle, whose `wants` is a
    /// `minecraft:potion` with `potion_contents: {potion: water}` — and
    /// modelling it here would be a second, worse copy of the item layer.
    pub components: Option<String>,
}

/// One `villager_trade` entry.
#[derive(Debug, Clone, PartialEq)]
pub struct Trade {
    pub id: String,
    pub wants: ItemCost,
    /// `additional_wants` — the second cost slot. 18 trades have one.
    pub additional_wants: Option<ItemCost>,
    pub gives: ItemCost,
    /// Codec default 4.
    pub max_uses: i32,
    /// Codec default 0.0.
    pub reputation_discount: f64,
    /// Codec default 1.
    pub xp: i32,
    /// `given_item_modifiers`, verbatim. Empty for 347 of the 388.
    pub given_item_modifiers: Option<String>,
    /// `merchant_predicate`, verbatim. Present on 27.
    pub merchant_predicate: Option<String>,
    /// `double_trade_price_enchantments`, verbatim. Present on 4.
    pub double_trade_price_enchantments: Option<String>,
}

/// One `trade_set` entry: how many offers a level deals, and from where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeSet {
    /// The tag path the pool comes from, without the `#minecraft:`.
    pub tag: String,
    /// `TradeSet.amount`, a `NumberProvider` that is a plain scalar in every
    /// one of the 68 sets.
    pub amount: i32,
    /// `TradeSet.allow_duplicates`, codec default false. **False in all 68**,
    /// which is why the without-duplicates loop is the one that runs today —
    /// and why the other one is still written: a datapack can set it.
    pub allow_duplicates: bool,
    /// The `RandomSequences` id the deal draws from.
    pub random_sequence: String,
}

/// The whole table.
#[derive(Debug, Clone, Default)]
pub struct TradeTable {
    pub trades: BTreeMap<String, Trade>,
    /// Tag path -> trade ids, references already resolved, in vanilla's order.
    pub tags: BTreeMap<String, Vec<String>>,
    /// `<profession>/level_N` -> its set.
    pub trade_sets: BTreeMap<String, TradeSet>,
}

fn s(v: &serde_json::Value, k: &str) -> Option<String> {
    v.get(k).map(std::string::ToString::to_string)
}

fn cost(v: &serde_json::Value, what: &str) -> io::Result<ItemCost> {
    let o = v
        .as_object()
        .ok_or_else(|| io::Error::other(format!("{what} is not an object")))?;
    Ok(ItemCost {
        id: o
            .get("id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| io::Error::other(format!("{what} has no id")))?
            .to_string(),
        count: o
            .get("count")
            .and_then(serde_json::Value::as_i64)
            .ok_or_else(|| io::Error::other(format!("{what} has no count")))? as i32,
        components: o.get("components").map(std::string::ToString::to_string),
    })
}

impl TradeTable {
    /// Parse `ci/dump-villager-trades.sh`'s output.
    ///
    /// # Errors
    /// If the document is not the shape that script writes.
    pub fn parse(bytes: &[u8]) -> io::Result<Self> {
        let v: serde_json::Value = serde_json::from_slice(bytes).map_err(io::Error::other)?;
        let mut t = Self::default();

        for (id, tv) in v["trades"]
            .as_object()
            .ok_or_else(|| io::Error::other("no trades object"))?
        {
            t.trades.insert(
                id.clone(),
                Trade {
                    id: id.clone(),
                    wants: cost(&tv["wants"], &format!("{id}.wants"))?,
                    additional_wants: match tv.get("additional_wants") {
                        Some(a) => Some(cost(a, &format!("{id}.additional_wants"))?),
                        None => None,
                    },
                    gives: cost(&tv["gives"], &format!("{id}.gives"))?,
                    max_uses: tv["max_uses"].as_i64().unwrap_or(4) as i32,
                    reputation_discount: tv["reputation_discount"].as_f64().unwrap_or(0.0),
                    xp: tv["xp"].as_i64().unwrap_or(1) as i32,
                    given_item_modifiers: s(tv, "given_item_modifiers"),
                    merchant_predicate: s(tv, "merchant_predicate"),
                    double_trade_price_enchantments: s(tv, "double_trade_price_enchantments"),
                },
            );
        }

        for (tag, entries) in v["tags"]
            .as_object()
            .ok_or_else(|| io::Error::other("no tags object"))?
        {
            t.tags.insert(
                tag.clone(),
                entries
                    .as_array()
                    .ok_or_else(|| io::Error::other(format!("tag {tag} is not a list")))?
                    .iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect(),
            );
        }

        for (name, sv) in v["trade_sets"]
            .as_object()
            .ok_or_else(|| io::Error::other("no trade_sets object"))?
        {
            t.trade_sets.insert(
                name.clone(),
                TradeSet {
                    tag: sv["tag"]
                        .as_str()
                        .ok_or_else(|| io::Error::other(format!("{name} has no tag")))?
                        .to_string(),
                    amount: sv["amount"].as_i64().unwrap_or(0) as i32,
                    allow_duplicates: sv["allow_duplicates"].as_bool().unwrap_or(false),
                    random_sequence: sv["random_sequence"].as_str().unwrap_or("").to_string(),
                },
            );
        }
        Ok(t)
    }

    /// Load from `data/vanilla/villager_trade_registry.json`, relative to the working
    /// directory or to the crate.
    ///
    /// # Errors
    /// If the table is absent or malformed.
    pub fn load() -> io::Result<Self> {
        const REL: &str = "data/vanilla/villager_trade_registry.json";
        let cwd = Path::new(REL);
        let path: PathBuf = if cwd.exists() {
            cwd.to_path_buf()
        } else {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(REL)
        };
        Self::parse(&std::fs::read(path)?)
    }

    /// The pool a profession's level draws from, in vanilla's order.
    #[must_use]
    pub fn pool(&self, profession: &str, level: i32) -> Option<&[String]> {
        let set = self
            .trade_sets
            .get(&format!("{profession}/level_{level}"))?;
        self.tags.get(&set.tag).map(Vec::as_slice)
    }

    /// The set a profession's level uses.
    #[must_use]
    pub fn set(&self, profession: &str, level: i32) -> Option<&TradeSet> {
        self.trade_sets.get(&format!("{profession}/level_{level}"))
    }
}

/// `AbstractVillager.addOffersFromItemListings` and
/// `…WithoutDuplicates`, which differ in one instruction and one loop
/// variable.
///
/// Both:
///
/// ```text
///   pool = new ArrayList(holderSet)
///   for (i = 0; i < count && !pool.isEmpty(); ) {
///       k = random.nextInt(pool.size())
///       ...
///   }
/// ```
///
/// With duplicates the entry is read with `get(k)` and **removed only when its
/// offer is null** — so the same trade can be dealt twice, and `i` does not
/// advance on a null. Without duplicates it is `remove(k)` unconditionally and
/// `i` advances only on a non-null. The difference matters for the draw
/// sequence as well as the result: the pool the second `nextInt` sees is a
/// different size in the two loops.
///
/// `next_int(n)` is `RandomSource.nextInt(n)` and `offer_of` is
/// `VillagerTrade.getOffer(ctx) != null`. Neither is guessed at here — see the
/// module docs for why `getOffer` is the loot layer's.
pub fn deal_offers(
    pool: &[String],
    count: i32,
    allow_duplicates: bool,
    next_int: &mut dyn FnMut(usize) -> usize,
    offer_of: &dyn Fn(&str) -> bool,
) -> Vec<String> {
    let mut remaining: Vec<String> = pool.to_vec();
    let mut out = Vec::new();
    let mut dealt = 0;
    while dealt < count && !remaining.is_empty() {
        let k = next_int(remaining.len());
        if allow_duplicates {
            let id = remaining[k].clone();
            if offer_of(&id) {
                out.push(id);
                dealt += 1;
            } else {
                remaining.remove(k);
            }
        } else {
            let id = remaining.remove(k);
            if offer_of(&id) {
                out.push(id);
                dealt += 1;
            }
        }
    }
    out
}

/// `ClientboundMerchantOffersPacket` and the `MerchantOffer` list it carries.
///
/// # The field list is not the record's, and six of ten are fixed-width
///
/// `MerchantOffer.STREAM_CODEC` is `StreamCodec.of(writeToStream, …)` — a
/// hand-written pair, not a composite — and `writeToStream` writes
///
/// ```text
///   ItemCost  costA
///   ItemStack result          (the NON-optional codec: no zero sentinel)
///   Optional<ItemCost> costB
///   boolean   outOfStock
///   int       uses            writeInt, 32-bit big-endian
///   int       maxUses         writeInt
///   int       xp              writeInt
///   int       specialPriceDiff writeInt, SIGNED
///   float     priceMultiplier
///   int       demand          writeInt, SIGNED
/// ```
///
/// Those six `writeInt`s are the trap. A varint agrees with a fixed 32-bit
/// big-endian integer on **no** value except by coincidence of length, but a
/// transcription that reaches for a varint — which is what almost every other
/// integer on this wire is — produces a body that is shorter and shifts every
/// field after it. `specialPriceDiff` and `demand` are also signed, and the
/// golden's fourth offer carries `-3` and `-6` so a two's-complement mistake
/// has somewhere to show.
///
/// The packet around them is `containerId`, the offer list, `villagerLevel`,
/// `villagerXp`, `showProgress`, `canRestock` — and those two integers **are**
/// varints. So the packet mixes both encodings, which is exactly why the
/// golden's `containerId` is 200 and its `villagerXp` is 150: below 128 the
/// two agree.
///
/// # What is not here
///
/// Component patches. Every stack this encoder writes carries an empty
/// `DataComponentPatch` (two zero varints) and every cost an empty
/// `DataComponentExactPredicate` (one zero varint). A trade whose result is
/// enchanted — 41 of the 388 carry `given_item_modifiers` — cannot be encoded
/// by this, and [`MerchantOffer::write`] refuses rather than dropping the
/// components silently.
pub mod merchant {
    use std::io::{self, Write};

    use rustmc_proto::{varint::write_varint, wire};

    /// `net.minecraft.world.item.trading.ItemCost`.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct ItemCostWire {
        /// The item's registry id, resolved by the caller — `crate::item` owns
        /// the name-to-id table and this file is the encoding.
        pub item_id: i32,
        pub count: i32,
        /// True when the cost carries a `DataComponentExactPredicate`. One of
        /// the 388 trades does; this encoder refuses it.
        pub has_components: bool,
    }

    /// `net.minecraft.world.item.trading.MerchantOffer`, wire fields only.
    #[derive(Debug, Clone, PartialEq)]
    pub struct MerchantOffer {
        pub cost_a: ItemCostWire,
        pub cost_b: Option<ItemCostWire>,
        pub result_item_id: i32,
        pub result_count: i32,
        /// True when the result carries a non-empty `DataComponentPatch`.
        pub result_has_components: bool,
        pub out_of_stock: bool,
        pub uses: i32,
        pub max_uses: i32,
        pub xp: i32,
        pub special_price_diff: i32,
        pub price_multiplier: f32,
        pub demand: i32,
    }

    fn write_cost(out: &mut impl Write, c: &ItemCostWire) -> io::Result<()> {
        if c.has_components {
            return Err(io::Error::other(
                "an ItemCost with a component predicate cannot be encoded here — \
                 see the module docs",
            ));
        }
        write_varint(out, c.item_id)?;
        write_varint(out, c.count)?;
        // `DataComponentExactPredicate.EMPTY` — a single zero varint.
        write_varint(out, 0)
    }

    impl MerchantOffer {
        /// # Errors
        /// If any stack carries components, or the sink refuses.
        pub fn write(&self, out: &mut impl Write) -> io::Result<()> {
            write_cost(out, &self.cost_a)?;
            if self.result_has_components {
                return Err(io::Error::other(
                    "an offer whose result carries components cannot be encoded here",
                ));
            }
            // `ItemStack.STREAM_CODEC`, the NON-optional one: count, item,
            // then the patch's two counts. There is no zero sentinel, so an
            // empty result would encode as a count of zero and an item id of
            // zero rather than as one byte.
            write_varint(out, self.result_count)?;
            write_varint(out, self.result_item_id)?;
            write_varint(out, 0)?;
            write_varint(out, 0)?;
            match &self.cost_b {
                Some(c) => {
                    wire::write_bool(out, true)?;
                    write_cost(out, c)?;
                }
                None => wire::write_bool(out, false)?,
            }
            wire::write_bool(out, self.out_of_stock)?;
            wire::write_i32(out, self.uses)?;
            wire::write_i32(out, self.max_uses)?;
            wire::write_i32(out, self.xp)?;
            wire::write_i32(out, self.special_price_diff)?;
            wire::write_f32(out, self.price_multiplier)?;
            wire::write_i32(out, self.demand)
        }
    }

    /// `minecraft:merchant_offers` — 52.
    ///
    /// # Errors
    /// If any offer refuses to encode, or the sink refuses.
    pub fn merchant_offers(
        out: &mut impl Write,
        container_id: i32,
        offers: &[MerchantOffer],
        villager_level: i32,
        villager_xp: i32,
        show_progress: bool,
        can_restock: bool,
    ) -> io::Result<()> {
        write_varint(out, container_id)?;
        write_varint(out, i32::try_from(offers.len()).map_err(io::Error::other)?)?;
        for o in offers {
            o.write(out)?;
        }
        write_varint(out, villager_level)?;
        write_varint(out, villager_xp)?;
        wire::write_bool(out, show_progress)?;
        wire::write_bool(out, can_restock)
    }
}

/// Professions and the points of interest they work at.
///
/// Both are **code** registries, unlike the trades: `VillagerProfession` and
/// `PoiType` are registered in Java and never appear in the datapack, so this
/// half of the villager needs a JVM where the trade table did not.
///
/// # The join between a profession and its trades is a convention, not a lookup
///
/// `VillagerProfession.tradeSetsByLevel` is an `Int2ObjectMap<ResourceKey<TradeSet>>`
/// and it is the **only** thing relating a profession to the
/// `trade_set/<profession>/level_N` entries in the datapack. That the key
/// happens to be `minecraft:<profession>/level_N` is a naming convention
/// somebody chose; nothing enforces it, and a datapack could point
/// `farmer/level_3` at `librarian/level_5`. [`tests`] asserts the convention
/// holds for all thirteen trading professions rather than assuming it.
pub mod professions {
    use std::collections::BTreeMap;
    use std::io;

    /// `net.minecraft.world.entity.ai.village.poi.PoiType`.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct PoiType {
        pub name: String,
        /// How many villagers can claim it. `1` for a job site, `32` for the
        /// meeting bell, `0` for a marker nothing claims (bee nests, the
        /// nether portal, a lodestone).
        pub max_tickets: i32,
        /// `validRange` — 1 for every job site and 6 for the meeting point.
        pub valid_range: i32,
        /// How many block *states* match. A bed is 128 of them (sixteen
        /// colours, four facings, two halves, occupied or not) and a
        /// lightning rod 192, so a server that matched on the block id alone
        /// would be right here and wrong for a POI that takes only some
        /// states of a block.
        pub states: usize,
        /// The distinct blocks those states belong to.
        pub blocks: Vec<String>,
    }

    /// `net.minecraft.world.entity.npc.villager.VillagerProfession`.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Profession {
        pub name: String,
        /// `None` for `nitwit` and `none`, the two that do not work.
        pub work_sound: Option<String>,
        /// The POI types `acquirableJobSite` accepts, evaluated over the whole
        /// registry — a predicate cannot be dumped, but its answer for every
        /// input can.
        pub acquirable_job_site: Vec<String>,
        pub held_job_site: Vec<String>,
        pub requested_items: Vec<String>,
        pub secondary_poi: Vec<String>,
        /// Level (1..=5) -> the `TradeSet` key, without the `minecraft:`.
        pub trade_sets_by_level: BTreeMap<i32, String>,
    }

    /// Both tables.
    #[derive(Debug, Clone, Default)]
    pub struct Registry {
        pub poi_types: BTreeMap<String, PoiType>,
        pub professions: BTreeMap<String, Profession>,
    }

    fn strings(v: &serde_json::Value) -> Vec<String> {
        v.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    impl Registry {
        /// Parse `ci/villager-golden.sh`'s output.
        ///
        /// # Errors
        /// If the document is not the shape that script writes.
        pub fn parse(bytes: &[u8]) -> io::Result<Self> {
            let v: serde_json::Value = serde_json::from_slice(bytes).map_err(io::Error::other)?;
            let mut r = Self::default();
            for p in v["poiTypes"]
                .as_array()
                .ok_or_else(|| io::Error::other("no poiTypes"))?
            {
                let name = p["name"]
                    .as_str()
                    .ok_or_else(|| io::Error::other("a POI type has no name"))?
                    .to_string();
                r.poi_types.insert(
                    name.clone(),
                    PoiType {
                        name,
                        max_tickets: p["maxTickets"].as_i64().unwrap_or(-1) as i32,
                        valid_range: p["validRange"].as_i64().unwrap_or(-1) as i32,
                        states: p["states"].as_u64().unwrap_or(0) as usize,
                        blocks: strings(&p["blocks"]),
                    },
                );
            }
            for p in v["professions"]
                .as_array()
                .ok_or_else(|| io::Error::other("no professions"))?
            {
                let name = p["name"]
                    .as_str()
                    .ok_or_else(|| io::Error::other("a profession has no name"))?
                    .to_string();
                let mut sets = BTreeMap::new();
                if let Some(m) = p["tradeSetsByLevel"].as_object() {
                    for (k, val) in m {
                        let level: i32 = k.parse().map_err(io::Error::other)?;
                        let key = val.as_str().unwrap_or_default();
                        sets.insert(
                            level,
                            key.strip_prefix("minecraft:").unwrap_or(key).to_string(),
                        );
                    }
                }
                r.professions.insert(
                    name.clone(),
                    Profession {
                        name,
                        work_sound: p["workSound"].as_str().map(str::to_string),
                        acquirable_job_site: strings(&p["acquirableJobSite"]),
                        held_job_site: strings(&p["heldJobSite"]),
                        requested_items: strings(&p["requestedItems"]),
                        secondary_poi: strings(&p["secondaryPoi"]),
                        trade_sets_by_level: sets,
                    },
                );
            }
            Ok(r)
        }
    }
}
