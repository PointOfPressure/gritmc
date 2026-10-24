//! The dispatch, exercised over every block in the game.
//!
//! `golden_loot.rs` proves the interpreter reproduces vanilla's vectors for a
//! table it is handed. This proves the other half: that the right table is
//! handed to it for the right block, using the extracted
//! `BlockBehaviour.getLootTable()` mapping rather than the file-naming
//! convention.
//!
//! The two halves fail differently and both failures look like success. A wrong
//! interpreter drops the wrong items from the right table; a wrong mapping drops
//! the right items from the wrong table. Only the second is tested here.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use rustmc_loot::{BlockRef, Loot, NoDrops};
use serde_json::Value;

/// Load a fixture **by its full literal path**, and the literal is load-bearing.
///
/// `ci/gen-gameplay-tables.sh` derives the set of tables a tree must generate by
/// grepping `crates`, `tools` and `plugins` for the string
/// `"data/vanilla/<name>.json"`. This crate's three targets used to build that
/// path by joining a directory and a bare filename, so the grep matched none of
/// them, the generator never ran `ci/loot-golden.sh`, and the four fixtures
/// existed in exactly one worktree on the box. Passing the whole path is what
/// puts this crate inside the generated set.
fn load(rel: &str) -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    serde_json::from_str(
        &std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display())),
    )
    .unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

#[test]
fn every_block_in_the_game_gets_an_answer_or_a_named_reason() {
    let tables: Arc<BTreeMap<String, Value>> =
        Arc::new(serde_json::from_value(load("data/vanilla/loot_tables.json")).unwrap());
    let item_tags: Arc<BTreeMap<String, Vec<String>>> =
        Arc::new(serde_json::from_value(load("data/vanilla/item_tags.json")).unwrap());
    let map: BTreeMap<String, Option<String>> =
        serde_json::from_value(load("data/vanilla/block_loot_map.json"))
            .expect("block_loot_map.json");
    let golden = load("data/vanilla/loot.json");
    let states = golden
        .get("states")
        .and_then(Value::as_object)
        .expect("states");

    let total_blocks = map.len();
    let loot = Loot::new(Arc::clone(&tables), Arc::clone(&item_tags), map.clone());

    let (mut dropped, mut empty, mut by_design, mut refused, mut unknown, mut missing) =
        (0, 0, 0, 0, 0, 0);
    let mut refusal_causes: BTreeMap<String, usize> = BTreeMap::new();

    for block in map.keys() {
        // The state the golden used, where there is one; otherwise the bare
        // block with no properties. A block whose table reads a property it was
        // not given REFUSES, which is why that count is reported and not hidden.
        let short = block.strip_prefix("minecraft:").unwrap_or(block);
        let state = states
            .get(&format!("blocks/{short}"))
            .map(|s| BlockRef {
                block: s
                    .get("block")
                    .and_then(Value::as_str)
                    .unwrap_or(block)
                    .to_string(),
                props: s
                    .get("props")
                    .and_then(Value::as_object)
                    .map(|o| {
                        o.iter()
                            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                            .collect()
                    })
                    .unwrap_or_default(),
            })
            .unwrap_or_else(|| BlockRef {
                block: block.clone(),
                props: BTreeMap::new(),
            });

        match loot.block_drops(&state, Some("minecraft:iron_pickaxe"), 0) {
            Ok(v) if v.iter().any(|s| s.count > 0) => dropped += 1,
            Ok(_) => empty += 1,
            Err(NoDrops::NoTableByDesign) => by_design += 1,
            Err(NoDrops::Refused(why)) => {
                refused += 1;
                *refusal_causes.entry(why).or_default() += 1;
            }
            Err(NoDrops::UnknownBlock(_)) => unknown += 1,
            Err(NoDrops::MissingTable(_)) => missing += 1,
        }
    }

    println!("\n=== block -> drops dispatch, all {total_blocks} blocks ===");
    println!("dropped something        {dropped}");
    println!("rolled, dropped nothing  {empty}");
    println!("no table by design       {by_design}   (vanilla's noLootTable)");
    println!("refused                  {refused}");
    println!("unknown block            {unknown}");
    println!("table missing            {missing}");
    if !refusal_causes.is_empty() {
        println!("\nrefusals by cause:");
        let mut v: Vec<_> = refusal_causes.iter().collect();
        v.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
        for (why, n) in v.iter().take(10) {
            println!("  {n:5}  {why}");
        }
    }

    // The mapping is extracted, so no block may be unknown and no table missing.
    // These are data-integrity failures, not behaviour, and they must be zero.
    assert_eq!(unknown, 0, "blocks absent from block_loot_map.json");
    assert_eq!(
        missing, 0,
        "block_loot_map.json names tables not in the corpus"
    );
    // 22 blocks genuinely have no table; that number is vanilla's, not ours.
    assert_eq!(by_design, 22, "vanilla 26.2 has 22 blocks with noLootTable");
    assert!(
        dropped > 1000,
        "only {dropped} of {total_blocks} blocks dropped anything — the dispatch is not wired"
    );
}
