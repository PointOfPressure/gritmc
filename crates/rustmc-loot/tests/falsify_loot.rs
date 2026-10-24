//! Show the loot gate failing before believing it passing.
//!
//! `golden_loot.rs` reports **17,993 of 18,055** cases matching vanilla with
//! **zero** mismatches, over 1,163 of 1,181 tables fully reproduced
//! (Threadripper, 2026-10-01, `cargo test --release -p rustmc-loot --test
//! golden_loot -- --nocapture`, on fixtures `ci/loot-golden.sh` generates with
//! the whole datapack loaded through `WorldLoader`). That number means nothing
//! on its own: a gate that has never been observed to go red is
//! indistinguishable from a gate that cannot.
//!
//! (It said 17,943 of 18,003 over 1,162 of 1,174 from 2026-09-05 until the
//! oracle loaded the full datapack: nested `loot_table` entries stopped being
//! excluded as oracle-blind, and the 31 cases that died on an unbound
//! enchantment tag now roll.)
//!
//! (This paragraph said 17,744 of 17,776 until 2026-09-05. Neither figure was
//! wrong when it was written; the corpus grew when the golden stopped being
//! restricted to `blocks/`. A stale count in a header reads as authoritative,
//! which is worse than none, so it now carries the box and the command that
//! produced it.) This file takes each of five
//! plausible mistakes in turn, injects it, and **requires the comparison to
//! fail**.
//!
//! There are three verdicts, and the third one was forced by the data.
//!
//! * **RED** — the fault was injected and the gate caught it. What we want.
//! * **VACUOUS** — the fault is reachable in the corpus and the gate stayed
//!   green anyway. That is a dimension the gate does not check, and it fails
//!   this test.
//! * **UNREACHABLE** — the fault is real in bytecode but **no shipped table can
//!   exercise it**. `bonus_rolls` is the case: `LootPool.addRandomItems` really
//!   does call `bonusRolls.getFloat(ctx)` before multiplying by luck, so an
//!   implementation that skipped it when luck is zero really would desynchronise
//!   the stream — and **zero of the 1,355 vanilla loot tables declare
//!   `bonus_rolls` at all**. Calling that VACUOUS would blame the gate for the
//!   corpus. Calling it RED would be a lie. So it is measured: the reachability
//!   of each fault is computed from the corpus every run, and if a future
//!   datapack ever ships a `bonus_rolls` pool this case flips to VACUOUS on its
//!   own and starts failing.
//!
//! The faults are not corruptions. Every one is something a careful reader of
//! the loot JSON would plausibly write, and three of them are invisible in the
//! JSON entirely — they are draw-order errors, which is precisely the class the
//! whole extractability argument turns on.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use rustmc_loot::{roll, BlockRef, Ctx, Faults, Stack};
use serde_json::Value;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

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
    let p = repo_root().join(rel);
    let s = std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("{}: {e} — regenerate with ci/loot-golden.sh", p.display()));
    serde_json::from_str(&s).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// Replays every block case under `faults` and returns (matched, mismatched).
fn replay(faults: Faults) -> (usize, usize) {
    let golden = load("data/vanilla/loot.json");
    let tables: Arc<BTreeMap<String, Value>> = Arc::new(
        serde_json::from_value(load("data/vanilla/loot_tables.json")).expect("loot_tables.json"),
    );
    let item_tags: Arc<BTreeMap<String, Vec<String>>> = Arc::new(
        serde_json::from_value(load("data/vanilla/item_tags.json")).expect("item_tags.json"),
    );
    let states = golden
        .get("states")
        .and_then(Value::as_object)
        .expect("states");

    let (mut ok, mut bad) = (0, 0);
    for case in golden
        .get("cases")
        .and_then(Value::as_array)
        .expect("cases")
    {
        if case.get("error").is_some() {
            continue;
        }
        let id = case.get("table").and_then(Value::as_str).unwrap();
        if !id.starts_with("blocks/") {
            continue;
        }
        let Some(table) = tables.get(id) else {
            continue;
        };
        let seed = case.get("seed").and_then(Value::as_i64).unwrap();
        let tool = case.get("tool").and_then(Value::as_str).unwrap();
        let block = states.get(id).map(|s| BlockRef {
            block: s
                .get("block")
                .and_then(Value::as_str)
                .unwrap_or("")
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
        });
        let mut ctx = Ctx {
            block,
            tool: match tool {
                "bare" => None,
                other => Some(format!("minecraft:{other}")),
            },
            explosion_radius: None,
            luck: 0.0,
            enchants: BTreeMap::new(),
            item_tags: Arc::clone(&item_tags),
            tables: Arc::clone(&tables),
            faults,
            ..Default::default()
        };
        // Id and count are what the vectors carry; components are produced and
        // not yet compared, so they are stripped rather than counted as a miss.
        let got: Vec<Stack> = roll(table, &mut ctx, seed)
            .into_iter()
            .map(|s| Stack::new(&s.item, s.count))
            .collect();
        if ctx.refused.is_some() {
            continue;
        }
        let want: Vec<Stack> = case
            .get("stacks")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .map(|s| {
                Stack::new(
                    s.get("item").and_then(Value::as_str).unwrap(),
                    s.get("count").and_then(Value::as_i64).unwrap() as i32,
                )
            })
            .collect();
        if got == want {
            ok += 1;
        } else {
            bad += 1;
        }
    }
    (ok, bad)
}

#[test]
fn every_plausible_mistake_turns_the_loot_gate_red() {
    let (clean_ok, clean_bad) = replay(Faults::default());
    println!("\nunfaulted: {clean_ok} matched, {clean_bad} mismatched");
    assert_eq!(
        clean_bad, 0,
        "the unfaulted interpreter must match before any fault means anything"
    );

    // Reachability, computed from the corpus rather than asserted.
    let tables: Arc<BTreeMap<String, Value>> = Arc::new(
        serde_json::from_value(load("data/vanilla/loot_tables.json")).expect("loot_tables.json"),
    );
    let bonus_rolls_pools = tables
        .values()
        .filter_map(|t| t.get("pools").and_then(Value::as_array))
        .flatten()
        .filter(|p| p.get("bonus_rolls").is_some())
        .count();

    let cases: &[(&str, Faults, &str, bool)] = &[
        (
            "draws the weighted choice for a single-entry pool",
            Faults {
                always_draw_entry: true,
                ..Default::default()
            },
            "the JSON cannot show that nextInt is skipped when a pool expands to one entry",
            true,
        ),
        (
            "skips bonus_rolls when luck is zero",
            Faults {
                skip_bonus_rolls_draw: true,
                ..Default::default()
            },
            "getFloat is called before the luck multiply, so the stream advances anyway",
            bonus_rolls_pools > 0,
        ),
        (
            "filters zero-count stacks instead of emitting air/0",
            Faults {
                filter_empty_stacks: true,
                ..Default::default()
            },
            "getRandomItemsRaw does not filter; the private getRandomItems does",
            true,
        ),
        (
            "rolls survives_explosion with no explosion in context",
            Faults {
                always_roll_survives_explosion: true,
                ..Default::default()
            },
            "a short-circuiting condition does not touch the generator",
            true,
        ),
        (
            "treats item tags as matching nothing",
            Faults {
                ignore_item_tags: true,
                ..Default::default()
            },
            "this is the exact fault the ORACLE had, and it produced plausible vectors",
            true,
        ),
    ];

    println!("corpus reachability: {bonus_rolls_pools} pools declare bonus_rolls");
    let mut vacuous = Vec::new();
    let mut red = 0;
    for (name, faults, why, reachable) in cases {
        let (ok, bad) = replay(*faults);
        let verdict = match (bad > 0, reachable) {
            (true, _) => "RED",
            (false, true) => "VACUOUS",
            (false, false) => "UNREACHABLE",
        };
        println!("{verdict:11} {bad:6} mismatched  {name}\n            {why}");
        match (bad > 0, reachable) {
            (true, _) => red += 1,
            (false, true) => vacuous.push(*name),
            (false, false) => {}
        }
        let _ = ok;
    }

    assert!(
        vacuous.is_empty(),
        "these faults are reachable in the corpus and left the gate green, \
         so it does not check that dimension: {vacuous:?}"
    );
    println!(
        "\n{red} of {} faults turned the gate red; the rest are unreachable in the shipped corpus",
        cases.len()
    );
}
