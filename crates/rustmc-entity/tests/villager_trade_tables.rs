//! The two trade extracts, read and asserted against facts that came from
//! somewhere other than the reader.
//!
//! # What makes this able to fail
//!
//! A test that loads a table and asserts it loaded is a check that measures
//! its own input — this repository's central defect. So every number below is
//! either a **count from the vanilla registry** (388 listings, 68 trade sets,
//! 73 pools, 13 professions, all four cross-checked inside
//! `ci/villager-trade-golden.sh` against Mojang's data generator as a second
//! source) or a **constant read out of the bytecode** (the level thresholds,
//! `Villager.updateTrades`' null branch), and neither comes from this file's
//! own reader. Corrupt the extract in any of those places and this reds.
//!
//! The two paths are spelled as quoted `"data/vanilla/..."` literals because
//! `ci/gen-gameplay-tables.sh` derives the set of tables a tree must generate
//! by grepping for exactly that shape, and `ci/check-generator-arms.sh` reds
//! T0 on a generator arm no literal reaches.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rustmc_entity::trading::create::{get_offer, OfferEnv};
use rustmc_entity::trading::draw::OfferOutcome;
use rustmc_entity::trading::tables::{Number, TradeTables};

fn repo(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

fn tables() -> Option<TradeTables> {
    let v = repo("data/vanilla/villager_trades.json");
    let w = repo("data/vanilla/wandering_trader_trades.json");
    if !rustmc_data::fixture::available(&v, "villager trades", "ci/villager-trade-golden.sh") {
        return None;
    }
    if !rustmc_data::fixture::available(
        &w,
        "wandering trader trades",
        "ci/villager-trade-golden.sh",
    ) {
        return None;
    }
    Some(TradeTables::load(&v, &w).expect("trade tables"))
}

/// The registry's own sizes. 291 profession listings + 97 wandering-trader
/// ones, 65 + 3 trade sets, 70 + 3 pools.
#[test]
fn the_registry_is_all_there() {
    let Some(t) = tables() else { return };
    assert_eq!(t.listing_count(), 388, "villager_trade registry entries");
    assert_eq!(t.trade_set_count(), 68, "trade_set registry entries");
    assert_eq!(t.pool_count(), 73, "villager_trade tags");
    assert_eq!(
        t.professions_with_trades().len(),
        13,
        "professions with a trade set"
    );
}

/// `Villager.updateTrades` offsets 18..33: `getTrades(level)` returning null
/// **returns** — it does not fall back to another level and it does not draw
/// from an empty pool. `none` and `nitwit` are the two professions that hit
/// it, and reading their absence as "level 1" would give every nitwit a
/// farmer's trades.
#[test]
fn a_profession_without_trades_answers_none_rather_than_empty() {
    let Some(t) = tables() else { return };
    for p in ["minecraft:none", "minecraft:nitwit"] {
        for level in 1..=5u8 {
            assert!(
                t.trade_set_for(p, level).is_none(),
                "{p} level {level} must have no trade set"
            );
        }
    }
    // And the positive control, or the assertion above is satisfied by a
    // reader that returns None for everything.
    assert!(t.trade_set_for("minecraft:farmer", 1).is_some());
    // Levels outside 1..=5 are null for every profession.
    assert!(t.trade_set_for("minecraft:farmer", 0).is_none());
    assert!(t.trade_set_for("minecraft:farmer", 6).is_none());
}

/// Every set names a pool that exists and holds only listings that exist.
/// A dangling id here would be a `nextInt` landing on a hole.
#[test]
fn every_pool_resolves_to_real_listings() {
    let Some(t) = tables() else { return };
    let mut checked = 0usize;
    for p in t.professions_with_trades() {
        for level in 1..=5u8 {
            let set = t
                .trade_set_for(p, level)
                .unwrap_or_else(|| panic!("{p} level {level}"));
            let pool = t
                .pool(&set.pool)
                .unwrap_or_else(|| panic!("{} names missing pool {}", set.id, set.pool));
            assert!(!pool.is_empty(), "{} draws from an empty pool", set.id);
            for id in pool {
                assert!(
                    t.listing(id).is_some(),
                    "{} names missing listing {id}",
                    set.pool
                );
                checked += 1;
            }
        }
    }
    assert!(checked >= 291, "only walked {checked} pool members");
}

/// The tag flattener's own answer, pinned. `armorer/level_1` opens with
/// `#minecraft:common_smith/level_1`, so the spliced member must come
/// **first** — a flattener that appended references instead of splicing them
/// in place would give the same set in a different order, and the order is
/// what `nextInt(list.size())` indexes.
#[test]
fn a_referenced_pool_is_spliced_at_the_point_of_reference() {
    let Some(t) = tables() else { return };
    let set = t.trade_set_for("minecraft:armorer", 1).unwrap();
    let pool = t.pool(&set.pool).unwrap();
    assert_eq!(
        pool.first().map(String::as_str),
        Some("minecraft:smith/1/coal_emerald"),
        "the common_smith reference is the first line of armorer/level_1 and must \
         resolve first; got {pool:?}"
    );
    assert_eq!(pool.len(), 5, "1 spliced + 4 of its own");
    // Levels 3, 4 and 5 splice an EMPTY common_smith tag, so the pool is the
    // armorer's own listings and nothing else. An implementation that treated
    // a missing/empty reference as an error would lose those three levels.
    let l3 = t.trade_set_for("minecraft:armorer", 3).unwrap();
    assert_eq!(t.pool(&l3.pool).unwrap().len(), 5);
}

/// The codec defaults reached the table. 16 listings omit `max_uses` and 122
/// omit `xp`; read as absent-means-zero they would be permanently out of
/// stock or grant no experience, and the villager would never level.
#[test]
fn omitted_fields_carry_their_codec_defaults() {
    let Some(t) = tables() else { return };
    let mut defaulted_uses = 0;
    let mut defaulted_xp = 0;
    for l in t.listings() {
        let uses = l.max_uses.as_int().expect("constant max_uses");
        assert!(uses >= 1, "{}: max_uses {uses}", l.id);
        let xp = l.xp.as_int().expect("constant xp");
        assert!(xp >= 0, "{}: xp {xp}", l.id);
        if l.defaulted_fields.iter().any(|f| f == "max_uses") {
            defaulted_uses += 1;
            assert_eq!(l.max_uses, Number::Constant(4.0), "{}", l.id);
        }
        if l.defaulted_fields.iter().any(|f| f == "xp") {
            defaulted_xp += 1;
            assert_eq!(l.xp, Number::Constant(1.0), "{}", l.id);
        }
    }
    // The counts are the registry's, not this reader's: 16 listings omit
    // `max_uses` (all of them the wandering trader's) and 122 omit `xp`
    // (25 profession listings and all 97 of the wandering trader's).
    assert_eq!(defaulted_uses, 16, "listings defaulting max_uses");
    assert_eq!(defaulted_xp, 122, "listings defaulting xp");
}

/// Every provider in the shipped data is a constant. This is the claim the
/// `Number::Unsupported` refusal rests on: if a future pack introduces a
/// `uniform` here, this test names it rather than the server rolling a
/// silently wrong number.
#[test]
fn every_shipped_number_provider_is_a_constant() {
    let Some(t) = tables() else { return };
    let mut bad = Vec::new();
    for p in t.professions_with_trades() {
        for level in 1..=5u8 {
            let set = t.trade_set_for(p, level).unwrap();
            if let Number::Unsupported(k) = &set.amount {
                bad.push(format!("{} amount: {k}", set.id));
            }
            for id in t.pool(&set.pool).unwrap() {
                let l = t.listing(id).unwrap();
                for (name, n) in [
                    ("max_uses", &l.max_uses),
                    ("xp", &l.xp),
                    ("reputation_discount", &l.reputation_discount),
                    ("wants.count", &l.wants.count),
                ] {
                    if let Number::Unsupported(k) = n {
                        bad.push(format!("{id} {name}: {k}"));
                    }
                }
            }
        }
    }
    assert!(bad.is_empty(), "non-constant providers: {bad:?}");
}

/// `TradeSet.CODEC` defaults `allow_duplicates` to false and no shipped set
/// sets it, so every vanilla draw today takes the
/// `addOffersFromItemListingsWithoutDuplicates` path. If that ever stops
/// being true the draw loop this crate uses changes, so it is pinned here
/// rather than assumed at the call site.
#[test]
fn no_shipped_trade_set_allows_duplicates() {
    let Some(t) = tables() else { return };
    for p in t.professions_with_trades() {
        for level in 1..=5u8 {
            let set = t.trade_set_for(p, level).unwrap();
            assert!(!set.allow_duplicates, "{} allows duplicates", set.id);
            assert!(
                set.random_sequence.is_some(),
                "{} has no random sequence, so its draw would come off the \
                 level's shared RandomSource",
                set.id
            );
        }
    }
}

/// The wandering trader's three sets are in the second table and reachable
/// by id — nothing maps a profession to them, because the wandering trader
/// has none. `WanderingTrader.updateTrades` names them directly.
#[test]
fn the_wandering_traders_three_sets_are_loaded() {
    let Some(t) = tables() else { return };
    let mut total = 0;
    for (id, amount) in [
        ("minecraft:wandering_trader/buying", 2),
        ("minecraft:wandering_trader/common", 5),
        ("minecraft:wandering_trader/uncommon", 2),
    ] {
        let set = t
            .trade_set(id)
            .unwrap_or_else(|| panic!("no trade set {id}"));
        assert_eq!(set.amount.as_int(), Some(amount), "{id} amount");
        let pool = t.pool(&set.pool).unwrap();
        assert!(!pool.is_empty(), "{id} pool");
        total += pool.len();
    }
    assert_eq!(total, 97, "the wandering trader's listings");
}

/// Eight trade sets ask for more listings than their pool holds. That is
/// vanilla's own data, and it matters because
/// `addOffersFromItemListingsWithoutDuplicates` exits on `list.isEmpty()`
/// rather than filling: a level-4 butcher offers **one** trade where its set
/// says two. A reader that sized an offer array from `amount` would be wrong
/// on exactly these eight.
#[test]
fn eight_trade_sets_are_over_subscribed() {
    let Some(t) = tables() else { return };
    let mut over = Vec::new();
    for p in t.professions_with_trades() {
        for level in 1..=5u8 {
            let set = t.trade_set_for(p, level).unwrap();
            let pool = t.pool(&set.pool).unwrap();
            let want = set.amount.as_int().unwrap();
            if want as usize > pool.len() {
                over.push(format!("{} {}>{}", set.id, want, pool.len()));
            }
        }
    }
    over.sort();
    assert_eq!(
        over,
        vec![
            "minecraft:butcher/level_4 2>1",
            "minecraft:butcher/level_5 2>1",
            "minecraft:fisherman/level_4 2>1",
            "minecraft:librarian/level_5 3>2",
            "minecraft:shepherd/level_5 2>1",
            "minecraft:toolsmith/level_5 2>1",
            "minecraft:weaponsmith/level_3 2>1",
            "minecraft:weaponsmith/level_5 2>1",
        ]
    );
}

/// Every item a listing names has a stack size, and 87 of them are not 64.
/// The clamp in `TradeCost.toItemCost` and the one in
/// `getModifiedCostCount` both read it, so defaulting to 64 would let a
/// villager ask for 16 water buckets.
#[test]
fn every_traded_item_has_its_real_stack_size() {
    let Some(t) = tables() else { return };
    let mut missing = Vec::new();
    for l in t.listings() {
        for item in [Some(&l.wants.item), Some(&l.gives.item)]
            .into_iter()
            .flatten()
            .chain(l.additional_wants.as_ref().map(|w| &w.item))
        {
            if t.max_stack_size(item).is_none() {
                missing.push(item.clone());
            }
        }
    }
    missing.sort();
    missing.dedup();
    assert!(missing.is_empty(), "no stack size for {missing:?}");
    assert_eq!(t.max_stack_size("minecraft:water_bucket"), Some(1));
    assert_eq!(t.max_stack_size("minecraft:white_banner"), Some(16));
    assert_eq!(t.max_stack_size("minecraft:emerald"), Some(64));
    assert!(t.max_stack_size("minecraft:bedrock").is_none());
}

/// `VillagerTrade.getOffer` over the whole registry, with an environment that
/// has the stack sizes and nothing else — which is what this server is today.
///
/// **The split is the finding.** 327 listings need no loot system and produce
/// their offer exactly; the other 61 must come back as `Unported` naming the
/// function or the condition that stopped them, never as a null offer, or a
/// missing subsystem would read as a villager who legitimately has no such
/// trade. If a later commit ports `enchant_with_levels`, this count moves and
/// this test says so.
#[test]
fn three_hundred_and_twenty_seven_listings_make_an_exact_offer_today() {
    let Some(t) = tables() else { return };
    struct Env<'a>(&'a TradeTables);
    impl OfferEnv for Env<'_> {
        fn default_max_stack_size(&self, item: &str) -> Option<i32> {
            self.0.max_stack_size(item)
        }
    }
    let mut offered = 0;
    let mut unported: BTreeMap<String, usize> = BTreeMap::new();
    let mut null = Vec::new();
    for l in t.listings() {
        match get_offer(l, &mut Env(&t)) {
            OfferOutcome::Offer(o) => {
                offered += 1;
                // Every offer must be usable: a cost of at least one, a
                // non-empty result, and a stock count above zero.
                assert!(o.cost_a().count >= 1, "{}", l.id);
                assert!(!o.result().is_empty(), "{}", l.id);
                assert!(o.max_uses() >= 1, "{}", l.id);
                assert!(!o.is_out_of_stock(), "{} starts out of stock", l.id);
            }
            OfferOutcome::Unported(w) => {
                let key = w
                    .split(':')
                    .next_back()
                    .unwrap_or(&w)
                    .split(", ")
                    .next()
                    .unwrap_or("")
                    .to_string();
                *unported.entry(key).or_default() += 1;
            }
            OfferOutcome::None(r) => null.push((l.id.clone(), r)),
        }
    }
    assert!(
        null.is_empty(),
        "no listing should be a plain null with an empty environment: {null:?}"
    );
    assert_eq!(offered, 327, "listings that need no loot system");
    assert_eq!(
        unported.values().sum::<usize>(),
        61,
        "listings refused by name: {unported:?}"
    );
}

/// `WanderingTrader.updateTrades` names its three sets in the order BUYING,
/// UNCOMMON, COMMON, and all three must be in the table under those ids — the
/// constant and the data have to agree or the trader draws nothing.
#[test]
fn the_wandering_traders_set_order_names_real_sets() {
    let Some(t) = tables() else { return };
    let mut total = 0;
    for id in rustmc_entity::trading::complete::WANDERING_TRADER_TRADE_SETS {
        let set = t
            .trade_set(id)
            .unwrap_or_else(|| panic!("updateTrades names {id}, which is not in the table"));
        total += set.amount.as_int().unwrap();
    }
    // 2 buying + 2 uncommon + 5 common. The pools are large enough that none
    // of the three is over-subscribed, so a wandering trader carries exactly
    // nine offers.
    assert_eq!(total, 9);
    for id in rustmc_entity::trading::complete::WANDERING_TRADER_TRADE_SETS {
        let set = t.trade_set(id).unwrap();
        let pool = t.pool(&set.pool).unwrap();
        assert!(
            pool.len() >= set.amount.as_int().unwrap() as usize,
            "{id} is over-subscribed and the trader would carry fewer than nine"
        );
    }
}
