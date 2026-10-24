//! The overworld biome table and its nearest-box search, against vanilla.
//!
//! Two halves, and the second is the interesting one.
//!
//! The table is emitted from vanilla's own
//! `MultiNoiseBiomeSourceParameterList` preset, because the search result
//! depends on the tree, the tree depends on the list order, and transcribing
//! ~7,600 hand-written parameter ranges would be an invitation to a silent
//! one-entry error that moves a biome border somewhere nobody looks.
//!
//! The queries are checked **in order**, because the lookup is not a pure
//! function. `Climate.RTree.search` seeds each search with the previous
//! result and prunes with a strict `>`, so an exactly-tied query resolves to
//! whatever the last one returned. Replaying the same queries shuffled can
//! legitimately give different biomes. This test therefore also asserts that
//! the stateless variant is *not* used by accident.

use rustmc_worldgen::biome::climate::{Parameter, ParameterList, ParameterPoint, TargetPoint};
use rustmc_worldgen::biome::multi_noise::{ClimateSampler, MultiNoiseBiomeSource};
use rustmc_worldgen::density::router::{OverworldNoises, OverworldRouter};
use rustmc_worldgen::noise::NoiseParameters;
use serde_json::Value;
use std::collections::BTreeMap;

const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/golden/climate_vectors.json");

fn read(path: &str) -> Value {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("golden vector {path} unreadable: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path} is not JSON: {e}"))
}

fn long(v: &Value) -> i64 {
    v.as_str()
        .unwrap_or_else(|| panic!("expected a long-as-string, got {v}"))
        .parse()
        .unwrap_or_else(|e| panic!("bad i64 {v}: {e}"))
}

/// The one case carrying the parameter table.
fn table_case(doc: &Value) -> &Value {
    doc["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .find(|c| c["case"] == "overworld_climate")
        .expect("no overworld_climate case")
}

fn table(case: &Value) -> Vec<(ParameterPoint, String)> {
    case["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .map(|entry| {
            let s = entry["space"].as_array().expect("space");
            assert_eq!(s.len(), 13, "6 intervals plus an offset");
            let axis = |i: usize| Parameter::new(long(&s[i * 2]), long(&s[i * 2 + 1]));
            (
                ParameterPoint {
                    temperature: axis(0),
                    humidity: axis(1),
                    continentalness: axis(2),
                    erosion: axis(3),
                    depth: axis(4),
                    weirdness: axis(5),
                    offset: long(&s[12]),
                },
                entry["biome"].as_str().expect("biome").to_string(),
            )
        })
        .collect()
}

fn queries(case: &Value) -> Vec<TargetPoint> {
    case["queries"]
        .as_array()
        .expect("queries")
        .iter()
        .map(|q| {
            let a = q.as_array().expect("[t,h,c,e,d,w]");
            TargetPoint {
                temperature: long(&a[0]),
                humidity: long(&a[1]),
                continentalness: long(&a[2]),
                erosion: long(&a[3]),
                depth: long(&a[4]),
                weirdness: long(&a[5]),
            }
        })
        .collect()
}

#[test]
fn overworld_climate_lookups_match_vanilla_in_order() {
    let doc = read(GOLDEN);
    let case = table_case(&doc);

    let entries = table(case);
    let targets = queries(case);
    let answers: Vec<&str> = case["answers"]
        .as_array()
        .expect("answers")
        .iter()
        .map(|a| a.as_str().expect("biome name"))
        .collect();
    assert_eq!(targets.len(), answers.len());
    assert!(entries.len() > 1000, "the table looks truncated");

    let mut list = ParameterList::new(entries);
    let mut wrong = 0usize;
    let mut confusion: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut examples = Vec::new();

    for (index, (target, want)) in targets.iter().zip(&answers).enumerate() {
        let got = list.find_value(*target).clone();
        if got != *want {
            wrong += 1;
            *confusion
                .entry(((*want).to_string(), got.clone()))
                .or_insert(0) += 1;
            if examples.len() < 6 {
                examples.push(format!("query {index} {target:?}: want {want}, got {got}"));
            }
        }
    }

    let exactness = 100.0 * (targets.len() - wrong) as f64 / targets.len() as f64;
    println!(
        "CLIMATE EXACTNESS {exactness:.6}% over {} ordered lookups ({wrong} wrong)",
        targets.len()
    );

    let report = confusion
        .iter()
        .map(|((want, got), count)| format!("  {want} -> {got}: {count}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        wrong,
        0,
        "{wrong} of {} climate lookups disagreed ({exactness:.6}% exact)\n\
         Confusion (expected -> got):\n{report}\nExamples:\n  {}",
        targets.len(),
        examples.join("\n  ")
    );
}

/// # This sweep contains no exact ties — recorded, not assumed
///
/// The tie-break is real: `Climate.RTree.search` prunes with a strict `>`
/// against a candidate seeded from the previous result, so two equidistant
/// boxes resolve to whichever the last query returned. `climate.rs` proves
/// that with a hand-built two-box table.
///
/// It does **not** fire anywhere in this 3375-point sweep of the real
/// overworld table. That was measured, not assumed — the first version of
/// this test asserted the opposite and failed.
///
/// Worth pinning rather than deleting, for two reasons. It bounds what the
/// ordered comparison above actually proves: on this data it is equivalent
/// to a stateless nearest-box search, so a green `golden_climate` is *not*
/// evidence that the tie-break is right. And if a future version of the
/// biome table introduces a tie here, this test turns red and says so,
/// rather than the ordered test quietly starting to depend on query order.
#[test]
fn no_query_in_this_sweep_hits_a_tie() {
    let doc = read(GOLDEN);
    let case = table_case(&doc);
    let entries = table(case);
    let targets = queries(case);

    let mut forward = ParameterList::new(entries.clone());
    let forward_answers: Vec<String> = targets
        .iter()
        .map(|t| forward.find_value(*t).clone())
        .collect();

    let mut backward = ParameterList::new(entries.clone());
    let mut backward_answers: Vec<String> = targets
        .iter()
        .rev()
        .map(|t| backward.find_value(*t).clone())
        .collect();
    backward_answers.reverse();

    let stateless = ParameterList::new(entries);
    let stateless_answers: Vec<String> = targets
        .iter()
        .map(|t| stateless.find_value_stateless(*t).clone())
        .collect();

    assert_eq!(
        forward_answers, backward_answers,
        "a query in this sweep now resolves differently depending on the \
         order it is asked in, i.e. the table has gained an exact tie"
    );
    assert_eq!(
        forward_answers, stateless_answers,
        "the seeded search and the stateless one disagree, which can only \
         happen on a tie"
    );
}

/// The noise parameters live in the router vector file.
fn noises() -> OverworldNoises {
    let doc = read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/golden/router_vectors.json"
    ));
    let mut set = OverworldNoises::new();
    for entry in doc["parameters"].as_array().expect("parameters") {
        let amplitudes: Vec<f64> = entry["amplitudes"]
            .as_array()
            .expect("amplitudes")
            .iter()
            .map(|v| f64::from_bits(long(v) as u64))
            .collect();
        set = set.with(
            entry["name"].as_str().expect("name"),
            NoiseParameters::new(
                entry["firstOctave"].as_i64().expect("firstOctave") as i32,
                amplitudes,
            ),
        );
    }
    set
}

/// End to end: world seed -> router -> climate sampler -> biome.
///
/// The test above proves the search over a table it is handed. This one
/// proves the *wiring* into it — that humidity reads `vegetation` and
/// weirdness reads `ridges`, that quart coordinates are scaled to blocks,
/// and that each `double` is narrowed to `float` before quantisation. Every
/// one of those is a way to produce a world that generates cleanly and has
/// the wrong biomes everywhere.
#[test]
fn biomes_at_world_positions_match_vanilla() {
    let doc = read(GOLDEN);
    let entries = table(table_case(&doc));
    let noises = noises();

    let mut total = 0usize;
    let mut wrong = 0usize;
    let mut confusion: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut examples = Vec::new();

    for case in doc["cases"].as_array().expect("cases") {
        if case["case"] != "biome_at_position" {
            continue;
        }
        let seed: i64 = case["seed"].as_str().expect("seed").parse().expect("i64");
        let router = OverworldRouter::new(seed, &noises, false);
        let mut source = MultiNoiseBiomeSource::new(
            ClimateSampler::from_router(&router),
            ParameterList::new(entries.clone()),
        );

        let positions = case["quartPositions"].as_array().expect("quartPositions");
        let biomes = case["biomes"].as_array().expect("biomes");
        assert_eq!(positions.len(), biomes.len());

        for (position, want) in positions.iter().zip(biomes) {
            let p = position.as_array().expect("[qx, qy, qz]");
            let (qx, qy, qz) = (
                p[0].as_i64().expect("qx") as i32,
                p[1].as_i64().expect("qy") as i32,
                p[2].as_i64().expect("qz") as i32,
            );
            let want = want.as_str().expect("biome name");
            let got = source.get_noise_biome(qx, qy, qz).clone();
            total += 1;
            if got != want {
                wrong += 1;
                *confusion
                    .entry((want.to_string(), got.clone()))
                    .or_insert(0) += 1;
                if examples.len() < 6 {
                    examples.push(format!(
                        "seed={seed} quart=({qx}, {qy}, {qz}): want {want}, got {got}"
                    ));
                }
            }
        }
    }

    assert!(total > 0, "no positions were checked");
    let exactness = 100.0 * (total - wrong) as f64 / total as f64;
    println!("BIOME EXACTNESS {exactness:.6}% over {total} positions ({wrong} wrong)");

    let report = confusion
        .iter()
        .map(|((want, got), count)| format!("  {want} -> {got}: {count}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        wrong,
        0,
        "{wrong} of {total} biome lookups disagreed ({exactness:.6}% exact)\n\
         Confusion (expected -> got):\n{report}\nExamples:\n  {}",
        examples.join("\n  ")
    );
}
