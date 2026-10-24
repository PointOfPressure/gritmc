//! Every vector vanilla produced, replayed through the interpreter.
//!
//! The vectors come from `ci/loot-golden.sh`, which makes vanilla roll vanilla's
//! own 1,355 tables at four seeds and four tools and records the item stacks.
//! This test is the only thing that can distinguish an interpreter that read the
//! JSON correctly from one that also draws in the right order, because the JSON
//! contains no statement about draw order at all.
//!
//! # Three verdicts, not two
//!
//! `MATCH` and `MISMATCH` are not enough. A third is needed and it is the point
//! of the exercise: `REFUSED`, for a table that reached an opcode or a context
//! this interpreter does not implement and **said so** rather than returning a
//! plausible empty list. Counting a refusal as a match would be the vacuous
//! green this repo keeps finding; counting it as a mismatch would hide the
//! difference between "wrong" and "not attempted", which is the difference the
//! whole extractability question turns on.
//!
//! The test asserts a floor on MATCH and a ceiling of **zero** on MISMATCH: an
//! opcode is either implemented faithfully or it refuses. There is no allowance
//! for "close".

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use rustmc_loot::{roll, BlockRef, Ctx, Stack};
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

fn tables_from_jar_dump() -> Arc<BTreeMap<String, Value>> {
    // The tables themselves ship in the jar; the golden run copies the set it
    // used next to its vectors so this test is reproducible without a JVM.
    let v = load("data/vanilla/loot_tables.json");
    Arc::new(
        v.as_object()
            .expect("loot_tables.json is an object")
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
    )
}

#[derive(Default)]
struct Tally {
    matched: usize,
    mismatched: usize,
    refused: usize,
    /// Cases the ORACLE cannot answer, as distinct from cases we cannot.
    oracle_blind: usize,
    /// Oracle-blind cases where the divergence actually showed, which is what
    /// makes "the oracle is blind here" evidence rather than an excuse.
    oracle_blind_demonstrated: usize,
    examples: Vec<String>,
    refusals: BTreeMap<String, usize>,
}

/// Does this table reach a `minecraft:loot_table` entry, at any depth?
///
/// Such a table cannot be graded against this oracle. `NestedLootTable`
/// resolves its reference through the context's registry lookup, and a harness
/// built from `VanillaRegistries.createLookup()` has no LOOT_TABLE registry in
/// it — so vanilla silently contributes nothing and records an empty drop.
///
/// This is the amethyst-cluster fault a second time and in a second place: a
/// plausible empty vector that is an artefact of the harness. It is detected
/// here rather than conceded, the affected cases are counted separately from
/// both MATCH and MISMATCH, and the test requires the divergence to be
/// *observed* at least once — otherwise this exclusion would be a way to make
/// disagreements disappear.
fn references_another_table(t: &Value, tables: &BTreeMap<String, Value>, depth: usize) -> bool {
    if depth > 8 {
        return false;
    }
    fn scan(v: &Value, found: &mut bool, names: &mut Vec<String>) {
        match v {
            Value::Object(o) => {
                if o.get("type")
                    .and_then(Value::as_str)
                    .map(|s| s.ends_with("loot_table"))
                    == Some(true)
                {
                    *found = true;
                    if let Some(n) = o.get("value").and_then(Value::as_str) {
                        names.push(n.to_string());
                    }
                }
                for x in o.values() {
                    scan(x, found, names);
                }
            }
            Value::Array(a) => {
                for x in a {
                    scan(x, found, names);
                }
            }
            _ => {}
        }
    }
    let (mut found, mut names) = (false, Vec::new());
    scan(t, &mut found, &mut names);
    if found {
        return true;
    }
    names.iter().any(|n| {
        let key = n.strip_prefix("minecraft:").unwrap_or(n);
        tables
            .get(key)
            .is_some_and(|inner| references_another_table(inner, tables, depth + 1))
    })
}

/// Whether the vectors were taken with the whole datapack loaded
/// (`LootGolden`'s `"oracle": "worldloader"`), in which case a nested
/// `loot_table` entry is resolved by vanilla and is compared like any other
/// case instead of being excluded as oracle-blind.
fn oracle_resolves_nested(golden: &Value) -> bool {
    golden.get("oracle").and_then(Value::as_str) == Some("worldloader")
}

#[test]
fn the_interpreter_reproduces_vanillas_own_loot_vectors() {
    let golden = load("data/vanilla/loot.json");
    let nested_resolves = oracle_resolves_nested(&golden);
    let tables = tables_from_jar_dump();
    let item_tags: Arc<BTreeMap<String, Vec<String>>> = Arc::new(
        serde_json::from_value(load("data/vanilla/item_tags.json")).expect("item_tags.json shape"),
    );
    let states = golden
        .get("states")
        .and_then(Value::as_object)
        .expect("states");

    let mut t = Tally::default();
    let mut per_table_ok: BTreeMap<String, bool> = BTreeMap::new();

    for case in golden
        .get("cases")
        .and_then(Value::as_array)
        .expect("cases")
    {
        if case.get("error").is_some() {
            continue; // vanilla itself could not evaluate it here; not our claim
        }
        let id = case.get("table").and_then(Value::as_str).unwrap();
        let Some(table) = tables.get(id) else {
            continue;
        };
        // Every family vanilla could evaluate, not just blocks. Restricting this
        // to `blocks/` was the prototype's scope and it flattered the result: the
        // interesting question is whether an opcode set derived from the block
        // corpus generalises, and the only way to find out is to stop excluding
        // the families it was not derived from. Tables vanilla itself refused are
        // already skipped above, so nothing here is being graded on a case the
        // oracle could not answer either.
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
            // No injected faults here. `tests/falsify_loot.rs` is the file that
            // turns them on and requires this gate to go red under each.
            ..Default::default()
        };

        // Id and count are what the vectors carry; components are produced and
        // not yet compared, so they are stripped rather than counted as a miss.
        let raw = roll(table, &mut ctx, seed);
        let got: Vec<Stack> = raw
            .into_iter()
            .map(|s| Stack::new(&s.item, s.count))
            .collect();
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

        if !nested_resolves && references_another_table(table, &tables, 0) {
            t.oracle_blind += 1;
            if got != want {
                t.oracle_blind_demonstrated += 1;
            }
            continue;
        }
        if let Some(why) = ctx.refused {
            t.refused += 1;
            *t.refusals.entry(why).or_default() += 1;
            per_table_ok.entry(id.to_string()).or_insert(false);
        } else if got == want {
            t.matched += 1;
            per_table_ok.entry(id.to_string()).or_insert(true);
        } else {
            t.mismatched += 1;
            per_table_ok.insert(id.to_string(), false);
            if t.examples.len() < 25 {
                t.examples.push(format!(
                    "{id} tool={tool} seed={seed}\n   want {want:?}\n   got  {got:?}"
                ));
            }
        }
    }

    let total = t.matched + t.mismatched + t.refused;
    let mut by_family: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for (id, ok) in &per_table_ok {
        let fam = id.split('/').next().unwrap_or("(root)").to_string();
        let e = by_family.entry(fam).or_default();
        e.1 += 1;
        if *ok {
            e.0 += 1;
        }
    }
    let tables_all_ok = per_table_ok.values().filter(|v| **v).count();
    println!("\n=== loot interpreter vs vanilla's own vectors ===");
    println!("cases        {total}");
    println!(
        "MATCH        {}  ({:.2}%)",
        t.matched,
        100.0 * t.matched as f64 / total as f64
    );
    println!("MISMATCH     {}", t.mismatched);
    println!(
        "REFUSED      {}   (an opcode or a context we do not have)",
        t.refused
    );
    println!(
        "ORACLE-BLIND {}   (a nested table reference this harness cannot resolve; \
{} of them demonstrably diverged)",
        t.oracle_blind, t.oracle_blind_demonstrated
    );
    println!(
        "tables fully reproduced  {tables_all_ok} of {}",
        per_table_ok.len()
    );
    println!("\nby family:");
    for (fam, (ok, n)) in &by_family {
        println!("  {ok:5} of {n:5}  {fam}");
    }
    if !t.refusals.is_empty() {
        println!("\nrefusals by cause:");
        let mut v: Vec<_> = t.refusals.iter().collect();
        v.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
        for (why, n) in v.iter().take(20) {
            println!("  {n:6}  {why}");
        }
    }
    if !t.examples.is_empty() {
        println!("\nmismatches:");
        for e in &t.examples {
            println!("  {e}");
        }
    }

    assert!(
        t.oracle_blind == 0 || t.oracle_blind_demonstrated > 0,
        "{} cases were excluded as oracle-blind and not one of them actually diverged; \
         an exclusion that never fires is a way of hiding disagreements",
        t.oracle_blind
    );
    assert_eq!(
        t.mismatched, 0,
        "an opcode is either faithful or it refuses; {} cases produced a WRONG answer",
        t.mismatched
    );
    assert!(
        t.matched >= 15_000,
        "only {} cases reproduced; the prototype's measured floor was 15,000",
        t.matched
    );
}

/// `RandomSequences.get(id, worldSeed)` against vanilla's own store: the
/// first eight `nextLong()`s of a fresh sequence, for three world seeds and
/// four ids. Both halves of the seed are pinned — the world seed and the md5
/// of the id — and a store that dropped either would be wrong on every row.
#[test]
fn random_sequences_reproduce_vanillas_own_streams() {
    use rustmc_loot::sequence::RandomSequences;
    let golden = load("data/vanilla/loot.json");
    let rows = golden
        .get("sequences")
        .and_then(Value::as_array)
        .expect("loot.json carries no `sequences` — regenerate with ci/loot-golden.sh");
    assert!(rows.len() >= 12, "only {} sequence rows", rows.len());
    for row in rows {
        let ws = row.get("world_seed").and_then(Value::as_i64).unwrap();
        let id = row.get("id").and_then(Value::as_str).unwrap();
        let want: Vec<i64> = row
            .get("longs")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .map(|v| v.as_i64().unwrap())
            .collect();
        let mut seqs = RandomSequences::new(ws);
        let got: Vec<i64> = (0..want.len()).map(|_| seqs.get(id).next_i64()).collect();
        assert_eq!(got, want, "sequence {id} under world seed {ws}");
    }
}

/// `LootTable.fill` against vanilla: the roll, the stack splitter, the slot
/// shuffle and `shuffleAndSplitItems`, all on one generator, compared slot by
/// slot. Both generator arms a chest reaches are covered — an explicit
/// `LootTableSeed`, and seed 0 drawing from the table's random sequence twice
/// in a row from one store.
#[derive(Debug, Default)]
struct FillTally {
    matched: usize,
    mismatched: usize,
    refused: usize,
    oracle_blind: usize,
    sequence_rows: usize,
    examples: Vec<String>,
}

fn replay_fills(faults: rustmc_loot::fill::FillFaults) -> FillTally {
    use rustmc_loot::fill;
    use rustmc_loot::roll_with;
    use rustmc_loot::sequence::RandomSequences;
    use rustmc_worldgen::rng::legacy::LegacyRandomSource;

    let golden = load("data/vanilla/loot.json");
    let nested_resolves = oracle_resolves_nested(&golden);
    let tables = tables_from_jar_dump();
    let item_tags: Arc<BTreeMap<String, Vec<String>>> = Arc::new(
        serde_json::from_value(load("data/vanilla/item_tags.json")).expect("item_tags.json shape"),
    );
    let max_stack: BTreeMap<String, i32> = golden
        .get("max_stack")
        .and_then(Value::as_object)
        .expect("loot.json carries no `max_stack`")
        .iter()
        .map(|(k, v)| (k.clone(), v.as_i64().unwrap() as i32))
        .collect();
    let max = |id: &str| *max_stack.get(id).unwrap_or(&64);
    let rows = golden
        .get("fills")
        .and_then(Value::as_array)
        .expect("loot.json carries no `fills` — regenerate with ci/loot-golden.sh");

    let mut stores: BTreeMap<String, RandomSequences> = BTreeMap::new();
    let mut t = FillTally::default();
    for row in rows {
        if row.get("error").is_some() {
            continue;
        }
        let id = row.get("table").and_then(Value::as_str).unwrap();
        let arm = row.get("arm").and_then(Value::as_str).unwrap();
        let seed = row.get("seed").and_then(Value::as_i64).unwrap();
        let table = tables.get(id).expect("fill names a table the dump has");
        // The harness cannot resolve a nested `loot_table` entry (see
        // `references_another_table`): vanilla rolls it as empty there, so the
        // row measures the harness, not the table.
        if !nested_resolves && references_another_table(table, &tables, 0) {
            t.oracle_blind += 1;
            continue;
        }
        let mut ctx = Ctx {
            item_tags: Arc::clone(&item_tags),
            tables: Arc::clone(&tables),
            ..Default::default()
        };
        let mut legacy = LegacyRandomSource::new(seed);
        let rng: &mut dyn rustmc_worldgen::rng::RandomSource = if arm == "seed" {
            &mut legacy
        } else {
            t.sequence_rows += 1;
            let seq = table
                .get("random_sequence")
                .and_then(Value::as_str)
                .expect("a sequence arm for a table naming no sequence");
            // One store per table, kept across its passes: pass 1 must
            // continue the stream pass 0 advanced.
            stores
                .entry(id.to_string())
                .or_insert_with(|| RandomSequences::new(seed))
                .get(seq)
        };
        let raw = roll_with(table, &mut ctx, &mut &mut *rng);
        if ctx.refused.is_some() {
            // The roll refused, so the generator stopped at an unknown point:
            // this row and every later pass of the same store are unmeasured.
            t.refused += 1;
            stores.remove(id);
            continue;
        }
        let mut chest: Vec<Option<Stack>> = vec![None; 27];
        fill::place_with(fill::split_stacks(raw, &max), &mut chest, rng, faults);
        let got: Vec<(usize, String, i32)> = chest
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.as_ref().map(|s| (i, s.item.clone(), s.count)))
            .collect();
        let want: Vec<(usize, String, i32)> = row
            .get("slots")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .map(|t| {
                let t = t.as_array().unwrap();
                (
                    t[0].as_u64().unwrap() as usize,
                    t[1].as_str().unwrap().to_string(),
                    t[2].as_i64().unwrap() as i32,
                )
            })
            .collect();
        if got == want {
            t.matched += 1;
        } else {
            t.mismatched += 1;
            if t.examples.len() < 10 {
                t.examples.push(format!(
                    "{id} {arm} {seed}\n   want {want:?}\n   got  {got:?}"
                ));
            }
        }
    }
    t
}

#[test]
fn chest_fills_reproduce_vanillas_own_slots() {
    let t = replay_fills(rustmc_loot::fill::FillFaults::default());
    println!(
        "fills: {} match, {} mismatch, {} refused, {} oracle-blind (nested table), {} sequence-arm rows",
        t.matched, t.mismatched, t.refused, t.oracle_blind, t.sequence_rows
    );
    for e in &t.examples {
        println!("  {e}");
    }
    assert_eq!(
        t.mismatched, 0,
        "{} chest fills put a wrong item in a wrong slot",
        t.mismatched
    );
    assert!(t.matched >= 150, "only {} fills reproduced", t.matched);
    assert!(
        t.sequence_rows >= 20,
        "only {} sequence-arm rows were compared",
        t.sequence_rows
    );
}

/// The fill gate goes RED under each of three plausible misreadings of
/// `LootTable.fill`'s bytecode. A slot comparison that agreed with vanilla
/// and could not be made to disagree would be this repo's central defect.
#[test]
fn the_fill_gate_goes_red_under_each_fault() {
    use rustmc_loot::fill::FillFaults;
    let cases = [
        (
            "draw_bool_for_singletons",
            FillFaults {
                draw_bool_for_singletons: true,
                ..Default::default()
            },
        ),
        (
            "shuffle_ascending",
            FillFaults {
                shuffle_ascending: true,
                ..Default::default()
            },
        ),
        (
            "pop_from_front",
            FillFaults {
                pop_from_front: true,
                ..Default::default()
            },
        ),
    ];
    for (name, f) in cases {
        let t = replay_fills(f);
        println!(
            "fault {name}: {} of {} fills now differ",
            t.mismatched,
            t.matched + t.mismatched
        );
        assert!(
            t.mismatched > 0,
            "fault {name} left every fill matching — the gate is blind to it"
        );
    }
}

/// Both halves of a sequence's seed are live in the vectors: dropping the id
/// or the world seed changes every row it should.
#[test]
fn the_sequence_gate_sees_both_halves_of_the_seed() {
    use rustmc_loot::sequence::create_sequence;
    use rustmc_worldgen::rng::RandomSource;
    let golden = load("data/vanilla/loot.json");
    let rows = golden
        .get("sequences")
        .and_then(Value::as_array)
        .expect("sequences");
    let (mut no_id, mut no_world, mut world_rows) = (0, 0, 0);
    for row in rows {
        let ws = row.get("world_seed").and_then(Value::as_i64).unwrap();
        let id = row.get("id").and_then(Value::as_str).unwrap();
        let first = row.get("longs").and_then(Value::as_array).unwrap()[0]
            .as_i64()
            .unwrap();
        if create_sequence(ws, None).next_i64() != first {
            no_id += 1;
        }
        if ws != 0 {
            world_rows += 1;
            if create_sequence(0, Some(id)).next_i64() != first {
                no_world += 1;
            }
        }
    }
    assert_eq!(
        no_id,
        rows.len(),
        "dropping the id left {} rows matching",
        rows.len() - no_id
    );
    assert_eq!(
        no_world, world_rows,
        "dropping the world seed left rows matching"
    );
}

/// Every `entities/*` table, rolled by vanilla under a parameter set that
/// requires nothing and supplies ORIGIN alone (`LootGolden.writeEntityCases`):
/// no entity, no attacker, no player, no damage source. That is the context
/// every optional-parameter read in the entity vocabulary falls back to, and
/// for a mob killed by `/kill` — entity present but unburning and adult, no
/// attacker, no player — it is the same draw sequence.
#[test]
fn entity_tables_with_no_entity_reproduce_vanillas_vectors() {
    let t = replay_entity_cases(rustmc_loot::Faults::default());
    println!(
        "entity cases: {} match ({} non-empty), {} mismatch, {} refused, {} oracle-blind, \
         {} on the random-sequence arm",
        t.matched, t.nonempty, t.mismatched, t.refused, t.blind, t.sequence_rows
    );
    for (why, n) in &t.refusals {
        println!("  refused {n:4}  {why}");
    }
    for e in &t.examples {
        println!("  {e}");
    }
    assert_eq!(t.mismatched, 0, "{} entity drops were WRONG", t.mismatched);
    assert!(
        t.matched >= 300,
        "only {} entity cases reproduced",
        t.matched
    );
    assert!(
        t.nonempty >= 150,
        "only {} reproduced cases dropped anything",
        t.nonempty
    );
}

/// The entity vectors go RED under each plausible misreading of the two
/// conditions that decide most mob drops.
#[test]
fn the_entity_gate_goes_red_under_each_fault() {
    use rustmc_loot::Faults;
    let cases = [
        (
            "skip_unenchanted_chance_draw",
            Faults {
                skip_unenchanted_chance_draw: true,
                ..Default::default()
            },
        ),
        (
            "killed_by_player_always",
            Faults {
                killed_by_player_always: true,
                ..Default::default()
            },
        ),
        (
            "always_draw_entry",
            Faults {
                always_draw_entry: true,
                ..Default::default()
            },
        ),
    ];
    for (name, f) in cases {
        let t = replay_entity_cases(f);
        println!(
            "fault {name}: {} of {} entity cases now differ",
            t.mismatched,
            t.matched + t.mismatched
        );
        assert!(
            t.mismatched > 0,
            "fault {name} left every entity case matching"
        );
    }
}

#[derive(Default)]
struct EntityTally {
    sequence_rows: usize,
    matched: usize,
    nonempty: usize,
    mismatched: usize,
    refused: usize,
    blind: usize,
    refusals: BTreeMap<String, usize>,
    examples: Vec<String>,
}

fn replay_entity_cases(faults: rustmc_loot::Faults) -> EntityTally {
    let mut stores: BTreeMap<String, rustmc_loot::sequence::RandomSequences> = BTreeMap::new();
    let golden = load("data/vanilla/loot.json");
    let nested_resolves = oracle_resolves_nested(&golden);
    let tables = tables_from_jar_dump();
    let item_tags: Arc<BTreeMap<String, Vec<String>>> = Arc::new(
        serde_json::from_value(load("data/vanilla/item_tags.json")).expect("item_tags.json shape"),
    );
    let rows = golden
        .get("entity_cases")
        .and_then(Value::as_array)
        .expect("loot.json carries no `entity_cases` — regenerate with ci/loot-golden.sh");
    let mut t = EntityTally::default();
    for case in rows {
        if case.get("error").is_some() {
            continue;
        }
        let id = case.get("table").and_then(Value::as_str).unwrap();
        let seed = case.get("seed").and_then(Value::as_i64).unwrap();
        let table = tables.get(id).expect("entity case names a dumped table");
        if !nested_resolves && references_another_table(table, &tables, 0) {
            t.blind += 1;
            continue;
        }
        let mut ctx = Ctx {
            item_tags: Arc::clone(&item_tags),
            tables: Arc::clone(&tables),
            faults,
            ..Default::default()
        };
        // A `sequence:N` row draws from the table's random sequence, one store
        // per table kept across its passes — the arm a real death takes.
        let raw = match case.get("arm").and_then(Value::as_str) {
            Some(arm) if arm.starts_with("sequence") => {
                t.sequence_rows += 1;
                let ws = case.get("world_seed").and_then(Value::as_i64).unwrap();
                let seq = table
                    .get("random_sequence")
                    .and_then(Value::as_str)
                    .expect("a sequence row for a table naming none");
                let store = stores
                    .entry(id.to_string())
                    .or_insert_with(|| rustmc_loot::sequence::RandomSequences::new(ws));
                rustmc_loot::roll_with(table, &mut ctx, &mut store.get(seq))
            }
            _ => roll(table, &mut ctx, seed),
        };
        let got: Vec<Stack> = raw
            .into_iter()
            .map(|s| Stack::new(&s.item, s.count))
            .collect();
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
        if let Some(why) = ctx.refused {
            t.refused += 1;
            *t.refusals.entry(why).or_default() += 1;
        } else if got == want {
            t.matched += 1;
            if !want.is_empty() {
                t.nonempty += 1;
            }
        } else {
            t.mismatched += 1;
            if t.examples.len() < 10 {
                t.examples.push(format!(
                    "{id} seed={seed}\n   want {want:?}\n   got  {got:?}"
                ));
            }
        }
    }
    t
}
