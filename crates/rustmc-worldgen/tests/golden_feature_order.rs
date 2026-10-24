//! The global feature order — the index that seeds every feature.
//!
//! Vectors from `ci/feature-order-golden.sh`, which calls vanilla's own
//! `FeatureSorter.buildFeaturesPerStep` over the real biome registry.
//!
//! Two things are checked, and keeping them apart is the point:
//!
//! 1. **The sort**, fed vanilla's own dumped biome lists. A failure here is
//!    the topological sort — the numbering order, the `(step, index)`
//!    comparator, the reversed post-order — and nothing else.
//! 2. **The reading**, comparing the biome lists this port would assemble
//!    from the datapack JSON against the ones vanilla dumped. A failure here
//!    is the data layer.
//!
//! Run together they would answer "something differs". Run apart they say
//! which.

mod common;

use common::{data_path, read, DATA};
use rustmc_worldgen::feature::sorter::build_features_per_step;
use serde_json::Value;

const GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/golden/feature_order_vectors.json"
);

fn vectors() -> Value {
    let text = std::fs::read_to_string(GOLDEN)
        .unwrap_or_else(|e| panic!("{GOLDEN} unreadable ({e}) — run ci/feature-order-golden.sh"));
    serde_json::from_str(&text).expect("vectors are JSON")
}

/// The biome lists exactly as vanilla dumped them.
fn sources_from_vectors(doc: &Value) -> Vec<Vec<Vec<String>>> {
    doc["sources"]
        .as_array()
        .expect("sources")
        .iter()
        .map(|biome| {
            biome["steps"]
                .as_array()
                .expect("steps")
                .iter()
                .map(|step| {
                    step.as_array()
                        .expect("step is a list")
                        .iter()
                        .map(|f| f.as_str().expect("feature name").to_string())
                        .collect()
                })
                .collect()
        })
        .collect()
}

#[test]
fn the_global_feature_order_matches_vanilla() {
    let doc = vectors();
    let sources = sources_from_vectors(&doc);
    // 55, not the registry's 66: the list vanilla sorts is
    // `List.copyOf(biomeSource.possibleBiomes())` — the biomes *this
    // dimension* can produce, in the order they first appear in its climate
    // parameter list. The nether and end biomes are not in it, and neither is
    // anything the overworld cannot reach.
    assert_eq!(
        sources.len(),
        55,
        "the overworld biome source has 55 possible biomes; a different number means \
         the vectors were built from the registry again"
    );

    let sorted = build_features_per_step(&sources).expect("the real registry must not cycle");
    let expected = doc["perStep"].as_array().expect("perStep");

    assert_eq!(
        sorted.len(),
        expected.len(),
        "step count differs: {} vs {}",
        sorted.len(),
        expected.len()
    );

    let mut total = 0usize;
    for (step, want) in expected.iter().enumerate() {
        let want: Vec<&str> = want
            .as_array()
            .expect("step list")
            .iter()
            .map(|f| f.as_str().expect("feature name"))
            .collect();
        let got: Vec<&str> = sorted[step].features.iter().map(String::as_str).collect();
        assert_eq!(
            got, want,
            "step {step} ordering differs.\n  got:  {got:?}\n  want: {want:?}"
        );
        total += want.len();
    }

    assert!(total > 150, "only {total} features — vectors shrank");
    println!(
        "FEATURE ORDER 100.000000% over {total} features in {} steps",
        sorted.len()
    );
}

#[test]
fn the_biome_feature_lists_are_read_the_same_way_vanilla_reports_them() {
    // Proves the data layer separately from the sort. If this passes and the
    // sort test fails, the fault is the algorithm; if this fails, the port is
    // assembling a different input and the sort was never the question.
    if !common::have_extract() {
        return;
    }
    let doc = vectors();

    let mut mismatches = Vec::new();
    for biome in doc["sources"].as_array().expect("sources") {
        let id = biome["biome"].as_str().expect("biome id");
        let stem = id.strip_prefix("minecraft:").unwrap_or(id);
        let path = data_path(&format!("worldgen/biome/{stem}.json"));
        assert!(path.exists(), "{DATA} has no biome {stem}");
        let json: Value = read(&path.to_string_lossy());

        let want: Vec<Vec<&str>> = biome["steps"]
            .as_array()
            .expect("steps")
            .iter()
            .map(|s| {
                s.as_array()
                    .expect("step")
                    .iter()
                    .map(|f| f.as_str().expect("name"))
                    .collect()
            })
            .collect();
        let got: Vec<Vec<&str>> = json["features"]
            .as_array()
            .expect("features")
            .iter()
            .map(|s| {
                s.as_array()
                    .expect("step")
                    .iter()
                    .map(|f| f.as_str().expect("name"))
                    .collect()
            })
            .collect();

        if got != want {
            mismatches.push(format!("{id}:\n  got  {got:?}\n  want {want:?}"));
        }
    }

    assert!(
        mismatches.is_empty(),
        "{} biomes read differently:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
    println!(
        "BIOME FEATURE LISTS 100.000000% over {} biomes",
        doc["sources"].as_array().expect("sources").len()
    );
}

#[test]
fn the_real_registry_has_no_feature_order_cycle() {
    // Not redundant with the ordering test: a cycle is the one failure mode
    // that stops a world loading at all, and it is the reason this is a
    // topological sort rather than a concatenation. Asserting it explicitly
    // means a future datapack change reports "cycle" rather than an opaque
    // ordering diff.
    let doc = vectors();
    let sources = sources_from_vectors(&doc);
    assert!(build_features_per_step(&sources).is_ok());
}
