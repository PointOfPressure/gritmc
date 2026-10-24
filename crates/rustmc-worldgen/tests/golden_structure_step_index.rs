//! The structure decoration index, against the pinned datapack itself.
//!
//! `structure/step_index.rs` carries an in-module fixture listing vanilla's
//! 34 structures and their steps. A fixture is a convenience and a liability
//! in equal measure: it is what makes the unit tests readable, and it is what
//! quietly stops describing the data the day the pin moves. This gate exists
//! so that day is loud.
//!
//! It reads `worldgen/structure/*.json` off the extract, derives the indices
//! from the files themselves, and asserts the three facts that other code
//! now depends on. **It must fail if the extract disagrees with the
//! fixture** — that is its whole job, and it is why it re-reads rather than
//! importing the fixture.
//!
//! Skipped, not failed, without the extract — the extract is gitignored
//! jar-derived material and `have_extract()` is the tree's standing way of
//! saying so.

mod common;

use common::{data_path, have_extract};
use rustmc_worldgen::structure::step_index::{step_indices, StepIndex, StructureEntry};

/// Every `worldgen/structure/*.json`, read as the loader would see it.
fn entries_from_extract() -> Vec<StructureEntry> {
    let dir = data_path("worldgen/structure");
    let mut out = Vec::new();
    for e in std::fs::read_dir(&dir).expect("worldgen/structure is readable") {
        let path = e.expect("dir entry").path();
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("structure json");
        let doc: serde_json::Value = serde_json::from_str(&text).expect("valid json");
        let step = doc["step"]
            .as_str()
            .unwrap_or_else(|| panic!("{stem}.json has no `step`"))
            .to_string();
        out.push(StructureEntry {
            // The key vanilla sorts on is the file identifier, suffix and
            // all. Built the same way here so the gate exercises the same
            // comparison the loader makes.
            sort_path: format!("worldgen/structure/{stem}.json"),
            namespace: "minecraft".to_string(),
            id: format!("minecraft:{stem}"),
            step,
        });
    }
    assert!(!out.is_empty(), "the extract holds no structures at all");
    out
}

/// The mineshaft index, which was measured out of vanilla's blocks long
/// before it could be derived. If the extract ever puts it anywhere but 1,
/// the derivation and the measurement have parted company and every
/// mineshaft in the world is on the wrong stream.
#[test]
fn the_extract_puts_mineshaft_at_the_measured_index() {
    if !have_extract() {
        return;
    }
    let (idx, _) = step_indices(&entries_from_extract()).expect("every step is a real one");
    assert_eq!(
        idx["minecraft:mineshaft"],
        StepIndex { step: 3, index: 1 },
        "measured from vanilla's own corridors as index 1 of underground_structures"
    );
}

#[test]
fn the_extract_agrees_with_the_fixture_on_the_unblocked_two() {
    if !have_extract() {
        return;
    }
    let (idx, _) = step_indices(&entries_from_extract()).expect("steps");
    assert_eq!(
        idx["minecraft:village_savanna"],
        StepIndex { step: 4, index: 23 }
    );
    assert_eq!(
        idx["minecraft:shipwreck_beached"],
        StepIndex { step: 4, index: 18 }
    );
}

/// The per-step structure counts, which are also the offsets that step's
/// *features* start counting from. A change here moves every feature in the
/// step, so it is asserted as a whole rather than sampled.
#[test]
fn the_extract_agrees_with_the_fixture_on_every_count() {
    if !have_extract() {
        return;
    }
    let entries = entries_from_extract();
    let (idx, counts) = step_indices(&entries).expect("steps");
    assert_eq!(entries.len(), 34, "vanilla 26.2 has 34 structures");
    assert_eq!(counts[3], 5, "underground_structures");
    assert_eq!(counts[4], 26, "surface_structures");
    assert_eq!(counts[7], 3, "underground_decoration");
    assert_eq!(idx.len(), 34, "every structure got an index");
    let total: usize = counts.iter().sum();
    assert_eq!(total, 34, "every structure landed in exactly one step");
}

/// Indices are dense and start at zero within each step — vanilla's counter
/// is `index++` per structure with no gaps, so a hole would mean the
/// derivation dropped one.
#[test]
fn each_steps_indices_are_dense_from_zero() {
    if !have_extract() {
        return;
    }
    let (idx, counts) = step_indices(&entries_from_extract()).expect("steps");
    for (step, n) in counts.iter().enumerate() {
        let mut seen: Vec<usize> = idx
            .values()
            .filter(|s| s.step == step)
            .map(|s| s.index)
            .collect();
        seen.sort_unstable();
        assert_eq!(
            seen,
            (0..*n).collect::<Vec<_>>(),
            "step {step} indices are not dense from zero"
        );
    }
}
