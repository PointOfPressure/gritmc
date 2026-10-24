//! The villager trade table against vanilla's own datapack, and the deal loop
//! against its bytecode.
//!
//! # Two different strengths, said apart
//!
//! * **The table is checked against the reference itself.** It is read out of
//!   `oracle-work/vanilla-26.2/generated-data`, so this test re-reads that
//!   extract and demands the loaded table agree with it row for row. There is
//!   no derivation in between for a mistake to hide in — which is a stronger
//!   position than any golden, and is only available because 26.2 made trades
//!   data.
//! * **The deal loop is checked against a scripted RNG, not against vanilla.**
//!   Running `AbstractVillager.addOffersFromItemListings` needs a
//!   `LootContext`, which needs a `ServerLevel`. So [`the_deal_loop`] pins the
//!   draw sequence and the pool sizes the two variants produce; that catches a
//!   change, and it does not prove parity. Said here rather than implied by a
//!   green result.

use std::collections::BTreeMap;
use std::path::PathBuf;

use rustmc_server::villager::{deal_offers, TradeTable};

fn extract_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../oracle-work/vanilla-26.2/generated-data/data/minecraft")
}

fn table() -> TradeTable {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/vanilla/villager_trade_registry.json");
    assert!(
        rustmc_data::fixture::available(&p, "villager trade table", "ci/dump-villager-trades.sh"),
        "no villager trade table"
    );
    TradeTable::parse(&std::fs::read(&p).expect("read table")).expect("parse table")
}

/// Walk the extract and rebuild the id -> file map the dumper built.
fn extract_trades() -> BTreeMap<String, serde_json::Value> {
    let base = extract_root().join("villager_trade");
    let mut out = BTreeMap::new();
    let mut stack = vec![base.clone()];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).expect("read dir") {
            let p = e.expect("entry").path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "json") {
                let key = p
                    .strip_prefix(&base)
                    .unwrap()
                    .with_extension("")
                    .to_string_lossy()
                    .into_owned();
                out.insert(
                    key,
                    serde_json::from_slice(&std::fs::read(&p).expect("read")).expect("parse"),
                );
            }
        }
    }
    out
}

/// Every trade, against the datapack file it came from.
#[test]
fn every_trade_matches_the_datapack() {
    let t = table();
    let src = extract_trades();
    assert_eq!(
        t.trades.len(),
        src.len(),
        "the table has {} trades and the extract has {}",
        t.trades.len(),
        src.len()
    );
    assert_eq!(t.trades.len(), 388, "26.2 ships 388 villager trades");

    let mut checked = 0usize;
    let mut with_second = 0usize;
    let mut with_modifiers = 0usize;
    let mut with_predicate = 0usize;
    for (id, raw) in &src {
        let got = t.trades.get(id).unwrap_or_else(|| panic!("no trade {id}"));
        assert_eq!(
            got.wants.id,
            raw["wants"]["id"].as_str().unwrap(),
            "{id}.wants.id"
        );
        assert_eq!(
            i64::from(got.wants.count),
            // `as_f64`, not `as_i64`: the extract writes some counts as
            // `4.0` and serde_json's `as_i64` is None for a float, so an
            // integer read here silently falls through to the default 1 and
            // the assertion compares 4 against 1 — which is how this line was
            // written first.
            raw["wants"]["count"].as_f64().unwrap_or(1.0) as i64,
            "{id}.wants.count"
        );
        assert_eq!(
            got.gives.id,
            raw["gives"]["id"].as_str().unwrap(),
            "{id}.gives.id"
        );
        assert_eq!(
            i64::from(got.gives.count),
            raw["gives"]["count"].as_f64().unwrap_or(1.0) as i64,
            "{id}.gives.count"
        );
        // The three codec defaults, which the extract omits where they apply:
        // max_uses 4, reputation_discount 0, xp 1. 16 files omit max_uses and
        // 122 omit xp, so these arms are reached rather than decorative.
        assert_eq!(
            f64::from(got.max_uses),
            raw["max_uses"].as_f64().unwrap_or(4.0),
            "{id}.max_uses"
        );
        assert_eq!(
            got.reputation_discount,
            raw["reputation_discount"].as_f64().unwrap_or(0.0),
            "{id}.reputation_discount"
        );
        assert_eq!(
            f64::from(got.xp),
            raw["xp"].as_f64().unwrap_or(1.0),
            "{id}.xp"
        );
        assert_eq!(
            got.additional_wants.is_some(),
            raw.get("additional_wants").is_some(),
            "{id}.additional_wants"
        );
        assert_eq!(
            got.merchant_predicate.is_some(),
            raw.get("merchant_predicate").is_some(),
            "{id}.merchant_predicate"
        );
        with_second += usize::from(got.additional_wants.is_some());
        with_modifiers += usize::from(got.given_item_modifiers.is_some());
        with_predicate += usize::from(got.merchant_predicate.is_some());
        checked += 8;
    }
    // The three optional-field counts. A dumper that silently dropped one of
    // these fields would still pass every per-trade assertion above, because
    // both sides would be `None`.
    assert_eq!(with_second, 18, "18 trades take a second cost");
    assert_eq!(with_modifiers, 41, "41 trades carry given_item_modifiers");
    assert_eq!(with_predicate, 27, "27 trades carry a merchant_predicate");
    eprintln!(
        "villager trades: {checked} comparisons across {} trades",
        src.len()
    );
}

/// The pools and the sets, including the eight that ask for more than they
/// have and the nested tags that make eight of the pools bigger than their own
/// file.
#[test]
fn pools_and_trade_sets_are_consistent() {
    let t = table();
    assert_eq!(t.tags.len(), 73, "73 villager_trade tags");
    assert_eq!(t.trade_sets.len(), 68, "68 trade sets");

    let professions: std::collections::BTreeSet<&str> = t
        .trade_sets
        .keys()
        .map(|k| k.split('/').next().unwrap())
        .collect();
    assert_eq!(professions.len(), 14, "14 professions have trade sets");
    assert!(professions.contains("wandering_trader"));
    // `nitwit` and `none` are professions with no trade set at all, which is
    // why this is 14 and the registry has 15 entries plus `none`.
    assert!(!professions.contains("nitwit"));

    let mut entries = 0usize;
    // Three tags are legitimately empty — `common_smith/level_3`, `_4` and
    // `_5` — so emptiness is counted rather than refused, and the count is
    // asserted so a fourth appearing is visible.
    let mut empty = 0usize;
    for (tag, ids) in &t.tags {
        if ids.is_empty() {
            empty += 1;
        }
        for id in ids {
            assert!(
                t.trades.contains_key(id),
                "tag {tag} names missing trade {id}"
            );
        }
        entries += ids.len();
    }
    assert_eq!(entries, 397, "397 tag entries after resolving nested tags");
    assert_eq!(empty, 3, "three empty tags: common_smith levels 3, 4 and 5");

    // Nested tags. Fifteen of the 73 files carry a `#minecraft:` reference
    // and resolving them changes the pool size in both directions: three of
    // the referenced tags are EMPTY (`common_smith` levels 3, 4 and 5), so a
    // resolved pool can be smaller than its own file's value list as well as
    // larger. `toolsmith/level_2` is one entry in its file and two resolved;
    // `toolsmith/level_4` is four and three. Both are asserted, because a
    // resolver that only ever grew a pool would pass the first and fail the
    // second, and one that did nothing at all would fail both.
    for (tag, file_len, resolved_len) in [
        ("toolsmith/level_2", 1, 2),
        ("toolsmith/level_4", 4, 3),
        ("weaponsmith/level_2", 1, 2),
    ] {
        let raw: serde_json::Value = serde_json::from_slice(
            &std::fs::read(extract_root().join(format!("tags/villager_trade/{tag}.json")))
                .expect("read tag"),
        )
        .expect("parse tag");
        let values = raw["values"].as_array().unwrap();
        assert_eq!(values.len(), file_len, "{tag}: file length moved");
        assert!(
            values.iter().any(|v| v.as_str().unwrap().starts_with('#')),
            "{tag} has no nested reference, so this control is inert"
        );
        assert_eq!(
            t.tags.get(tag).unwrap().len(),
            resolved_len,
            "{tag}: resolution"
        );
    }

    let mut short = 0usize;
    let mut dealt = 0i32;
    for (name, set) in &t.trade_sets {
        let pool = t
            .tags
            .get(&set.tag)
            .unwrap_or_else(|| panic!("{name}: no pool"));
        assert!(
            !set.allow_duplicates,
            "{name}: 26.2 ships no duplicate sets"
        );
        assert!(set.random_sequence.starts_with("minecraft:trade_set/"));
        dealt += set.amount;
        if set.amount as usize > pool.len() {
            short += 1;
        }
    }
    assert_eq!(dealt, 140, "140 offers dealt across every set");
    assert_eq!(short, 8, "eight sets ask for more than their pool holds");

    // The accessors resolve.
    assert_eq!(t.set("farmer", 1).unwrap().amount, 2);
    assert_eq!(t.pool("farmer", 1).unwrap().len(), 5);
    assert!(t.set("nitwit", 1).is_none());
}

/// The deal loop's draw sequence, both variants.
///
/// This is a regression check on a transcription, not a parity check — see the
/// module header. What it pins is the thing a rewrite would get wrong: which
/// index is drawn, whether the entry is removed, and whether a null offer
/// advances the counter.
#[test]
fn the_deal_loop() {
    let pool: Vec<String> = ["a", "b", "c", "d"]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    let always = |_: &str| true;

    // Without duplicates: every draw removes, so the pool shrinks 4, 3, 2 and
    // the same trade cannot come out twice.
    let mut sizes = Vec::new();
    let mut draws = [0usize, 0, 1].into_iter();
    let mut next = |n: usize| {
        sizes.push(n);
        draws.next().unwrap()
    };
    let got = deal_offers(&pool, 3, false, &mut next, &always);
    assert_eq!(got, ["a", "b", "d"]);
    assert_eq!(sizes, [4, 3, 2]);

    // With duplicates: a successful draw does NOT remove, so the pool stays 4
    // and index 0 can come out three times.
    let mut sizes = Vec::new();
    let mut draws = [0usize, 0, 0].into_iter();
    let mut next = |n: usize| {
        sizes.push(n);
        draws.next().unwrap()
    };
    let got = deal_offers(&pool, 3, true, &mut next, &always);
    assert_eq!(got, ["a", "a", "a"]);
    assert_eq!(sizes, [4, 4, 4]);

    // A null offer. Without duplicates the entry is already gone and the
    // counter simply does not advance; with duplicates the entry is removed
    // *because* the offer was null, which is the only place that loop removes
    // anything.
    let not_b = |id: &str| id != "b";
    let mut sizes = Vec::new();
    let mut draws = [1usize, 0, 0].into_iter();
    let mut next = |n: usize| {
        sizes.push(n);
        draws.next().unwrap()
    };
    let got = deal_offers(&pool, 2, false, &mut next, &not_b);
    assert_eq!(got, ["a", "c"]);
    assert_eq!(
        sizes,
        [4, 3, 2],
        "a null draw still consumed a random number"
    );

    let mut sizes = Vec::new();
    let mut draws = [1usize, 1, 1].into_iter();
    let mut next = |n: usize| {
        sizes.push(n);
        draws.next().unwrap()
    };
    let got = deal_offers(&pool, 2, true, &mut next, &not_b);
    assert_eq!(got, ["c", "c"]);
    assert_eq!(
        sizes,
        [4, 3, 3],
        "the null removed one and the rest did not"
    );

    // The pool emptying is what makes the eight over-asking sets legal: two
    // offers are asked for, one is available, one comes out and the loop stops
    // rather than repeating or panicking.
    let one = vec!["only".to_string()];
    let mut next = |_: usize| 0usize;
    assert_eq!(deal_offers(&one, 2, false, &mut next, &always), ["only"]);

    // And the real short set behaves the same way.
    let t = table();
    let set = t.set("butcher", 4).unwrap();
    let pool = t.pool("butcher", 4).unwrap();
    assert_eq!((set.amount as usize, pool.len()), (2, 1));
    let mut next = |_: usize| 0usize;
    assert_eq!(
        deal_offers(pool, set.amount, set.allow_duplicates, &mut next, &always).len(),
        1
    );
}

/// `merchant_offers`, against vanilla's own encoding.
///
/// `crates/rustmc-server/golden/merchant_offers.json` is
/// `ClientboundMerchantOffersPacket.STREAM_CODEC` run inside the pinned jar
/// over four offers chosen so that no field is constant: one with a second
/// cost, one out of stock, one with a negative `specialPriceDiff` and a
/// negative `demand`, two price multipliers and two result counts. The
/// `containerId` is 200 and the `villagerXp` 150, both above 127, so the two
/// varints in the packet are two bytes.
#[test]
fn merchant_offers_matches_vanilla() {
    use rustmc_server::villager::merchant::{merchant_offers, ItemCostWire, MerchantOffer};

    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("golden/merchant_offers.json");
    assert!(
        rustmc_data::fixture::available(&p, "merchant offers golden", "ci/merchant-golden.sh"),
        "no merchant offers golden"
    );
    let g: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&p).expect("read")).expect("parse");
    let id = |k: &str| g[k].as_i64().unwrap() as i32;
    let rows = g["offers"].as_array().unwrap();
    assert_eq!(rows.len(), 4);

    let cost = |item: i32, count: i32| ItemCostWire {
        item_id: item,
        count,
        has_components: false,
    };
    let base = |cost_a: ItemCostWire, result: i32, result_count: i32| MerchantOffer {
        cost_a,
        cost_b: None,
        result_item_id: result,
        result_count,
        result_has_components: false,
        out_of_stock: false,
        uses: 0,
        max_uses: 16,
        xp: 1,
        special_price_diff: 0,
        price_multiplier: 0.05,
        demand: 0,
    };

    let emerald = id("emeraldId");
    let mut wheat = base(cost(id("wheatId"), 20), emerald, 1);
    wheat.xp = 2;
    let mut cooked = base(cost(id("codId"), 6), id("cookedCodId"), 6);
    cooked.cost_b = Some(cost(emerald, 1));
    cooked.xp = 5;
    let mut used = base(cost(id("coalId"), 15), emerald, 1);
    used.uses = 16;
    used.out_of_stock = true;
    let mut priced = base(cost(id("paperId"), 24), emerald, 1);
    priced.uses = 5;
    priced.special_price_diff = -3;
    priced.price_multiplier = 0.2;
    priced.demand = -6;

    // Every scalar the golden recorded beside the bytes, so a body that is
    // right by two compensating mistakes cannot pass.
    let offers = [wheat, cooked, used, priced];
    for (o, r) in offers.iter().zip(rows) {
        let name = r["name"].as_str().unwrap();
        assert_eq!(
            o.out_of_stock,
            r["outOfStock"].as_bool().unwrap(),
            "{name}.outOfStock"
        );
        assert_eq!(
            i64::from(o.uses),
            r["uses"].as_i64().unwrap(),
            "{name}.uses"
        );
        assert_eq!(
            i64::from(o.max_uses),
            r["maxUses"].as_i64().unwrap(),
            "{name}.maxUses"
        );
        assert_eq!(i64::from(o.xp), r["xp"].as_i64().unwrap(), "{name}.xp");
        assert_eq!(
            i64::from(o.special_price_diff),
            r["specialPriceDiff"].as_i64().unwrap(),
            "{name}.specialPriceDiff"
        );
        assert_eq!(
            i64::from(o.demand),
            r["demand"].as_i64().unwrap(),
            "{name}.demand"
        );
        assert_eq!(
            o.cost_b.is_some(),
            r["hasCostB"].as_bool().unwrap(),
            "{name}.hasCostB"
        );
        assert_eq!(
            i64::from(o.result_count),
            r["resultCount"].as_i64().unwrap(),
            "{name}.resultCount"
        );
    }

    let mut body = Vec::new();
    merchant_offers(
        &mut body,
        id("containerId"),
        &offers,
        id("villagerLevel"),
        id("villagerXp"),
        g["showProgress"].as_bool().unwrap(),
        g["canRestock"].as_bool().unwrap(),
    )
    .expect("encode");
    assert_eq!(hex(&body), g["packet"].as_str().unwrap());

    // The empty list: six bytes, and the offer count is one zero varint.
    let mut empty = Vec::new();
    merchant_offers(&mut empty, 0, &[], 1, 0, false, true).expect("encode empty");
    assert_eq!(hex(&empty), g["emptyPacket"].as_str().unwrap());

    // And the refusal: a stack with components is an error, not a silent drop.
    let mut with_components = offers[0].clone();
    with_components.cost_a.has_components = true;
    let mut sink = Vec::new();
    let e = with_components.write(&mut sink).unwrap_err();
    assert!(e.to_string().contains("component predicate"), "{e}");
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The profession and POI tables, and the join to the trade table.
#[test]
fn professions_and_poi_types() {
    use rustmc_server::villager::professions::Registry;

    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("golden/villager_professions.json");
    assert!(
        rustmc_data::fixture::available(&p, "villager profession golden", "ci/villager-golden.sh"),
        "no villager profession golden"
    );
    let r = Registry::parse(&std::fs::read(&p).expect("read")).expect("parse");
    assert_eq!(r.poi_types.len(), 21, "21 POI types");
    assert_eq!(
        r.professions.len(),
        15,
        "15 professions, nitwit and none included"
    );

    // maxTickets is what separates a job site from a meeting point from a
    // marker, and it takes three distinct values: 1, 32 and 0.
    assert_eq!(r.poi_types["minecraft:armorer"].max_tickets, 1);
    assert_eq!(r.poi_types["minecraft:meeting"].max_tickets, 32);
    assert_eq!(r.poi_types["minecraft:meeting"].valid_range, 6);
    assert_eq!(r.poi_types["minecraft:bee_nest"].max_tickets, 0);
    // Every other POI's range is 1, so the meeting bell is the only one where
    // the distinction is observable.
    assert_eq!(
        r.poi_types
            .values()
            .filter(|p| p.valid_range != 1)
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["minecraft:meeting"]
    );

    // A bed is 128 states across sixteen blocks, so a POI matcher keyed on the
    // block id alone is right here and would be wrong for a POI that took only
    // some states of a block.
    let home = &r.poi_types["minecraft:home"];
    assert_eq!(home.states, 128);
    assert_eq!(home.blocks.len(), 16);
    // The leatherworker's cauldron is the opposite shape: four blocks, eight
    // states, so most cauldron states are NOT a job site.
    let lw = &r.poi_types["minecraft:leatherworker"];
    assert_eq!((lw.blocks.len(), lw.states), (4, 8));

    // THE JOIN. Every profession that trades names five trade sets, and every
    // one of those keys must exist in the datapack table. The naming
    // convention is not enforced by anything in the jar, so it is asserted
    // here rather than assumed.
    let t = table();
    let mut joined = 0usize;
    for (name, p) in &r.professions {
        if p.trade_sets_by_level.is_empty() {
            assert!(
                name == "minecraft:nitwit" || name == "minecraft:none",
                "{name} has no trade sets"
            );
            assert!(
                p.work_sound.is_none(),
                "{name} has a work sound but no trades"
            );
            continue;
        }
        assert_eq!(
            p.trade_sets_by_level.keys().copied().collect::<Vec<_>>(),
            [1, 2, 3, 4, 5],
            "{name} levels"
        );
        let short = name.strip_prefix("minecraft:").unwrap();
        for (level, key) in &p.trade_sets_by_level {
            assert_eq!(
                key,
                &format!("{short}/level_{level}"),
                "{name} level {level}"
            );
            assert!(
                t.trade_sets.contains_key(key),
                "{name} names trade set {key}, which the datapack does not have"
            );
            joined += 1;
        }
        // And the profession's job site is its own name, for all thirteen.
        assert_eq!(
            p.acquirable_job_site,
            std::slice::from_ref(name),
            "{name} job site"
        );
        assert!(p.work_sound.is_some(), "{name} has no work sound");
    }
    assert_eq!(joined, 65, "thirteen professions times five levels");

    // The wandering trader has trade sets and is NOT a villager profession —
    // it is a separate entity — so the datapack has 68 sets where the join
    // accounts for 65.
    assert!(!r.professions.contains_key("minecraft:wandering_trader"));
    assert_eq!(
        t.trade_sets.len() - joined,
        3,
        "the wandering trader's three sets"
    );

    // The farmer is the only profession with requested items and a secondary
    // POI, so those two fields are not constant across the table.
    let farmer = &r.professions["minecraft:farmer"];
    assert_eq!(farmer.requested_items.len(), 4);
    assert_eq!(farmer.secondary_poi.len(), 1);
    assert_eq!(
        r.professions
            .values()
            .filter(|p| !p.requested_items.is_empty())
            .count(),
        1
    );
}
