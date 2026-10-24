//! The real overworld surface rule, parsed.
//!
//! The unit tests in `surface::rule` cover each operator in isolation. This
//! one asserts the parser handles vanilla's actual tree — 35 KB of JSON, 22
//! levels deep, 149 conditions — because "every operator works" and "the
//! whole tree parses" are different claims, and the second is the one that
//! matters before an evaluator is written against it.
//!
//! Skipped rather than failed when the extract is absent. It comes from
//! `ci/vanilla-server.sh data`, is Mojang-derived and gitignored, so a clean
//! clone does not have it and should not fail for that.

use rustmc_worldgen::surface::rule::{parse_noise_settings_surface_rule, Condition, Rule};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn extract(name: &str) -> Option<serde_json::Value> {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "../../oracle-work/vanilla-26.2/generated-data/data/minecraft/worldgen/noise_settings",
        name,
    ]
    .iter()
    .collect();
    if !rustmc_data::fixture::available(&path, "surface rule extract", "ci/vanilla-server.sh data")
    {
        return None;
    }
    let text = std::fs::read_to_string(&path).expect("read the extract");
    Some(serde_json::from_str(&text).expect("extract is JSON"))
}

/// Counts every node by operator, so the shape can be asserted rather than
/// eyeballed.
fn census(rule: &Rule, counts: &mut BTreeMap<&'static str, usize>) {
    match rule {
        Rule::Sequence(children) => {
            *counts.entry("sequence").or_default() += 1;
            for child in children {
                census(child, counts);
            }
        }
        Rule::Condition { if_true, then_run } => {
            *counts.entry("condition").or_default() += 1;
            census_condition(if_true, counts);
            census(then_run, counts);
        }
        Rule::Block(_) => *counts.entry("block").or_default() += 1,
        Rule::Bandlands => *counts.entry("bandlands").or_default() += 1,
    }
}

fn census_condition(condition: &Condition, counts: &mut BTreeMap<&'static str, usize>) {
    let name = match condition {
        // Both forms census as `biome`: they are one condition, before and
        // after interning. The parser only ever produces the first, so this
        // test exercises that one; the arm exists so that adding the interned
        // form did not silently drop it from a count.
        Condition::Biome(_) | Condition::BiomeIds(_) => "biome",
        Condition::NoiseThreshold { .. } => "noise_threshold",
        Condition::VerticalGradient { .. } => "vertical_gradient",
        Condition::YAbove { .. } => "y_above",
        Condition::Water { .. } => "water",
        Condition::StoneDepth { .. } => "stone_depth",
        Condition::Hole => "hole",
        Condition::AbovePreliminarySurface => "above_preliminary_surface",
        Condition::Steep => "steep",
        Condition::Temperature => "temperature",
        Condition::Not(inner) => {
            *counts.entry("not").or_default() += 1;
            census_condition(inner, counts);
            return;
        }
    };
    *counts.entry(name).or_default() += 1;
}

#[test]
fn the_real_overworld_rule_parses_with_the_expected_shape() {
    let Some(settings) = extract("overworld.json") else {
        return;
    };
    let rule = parse_noise_settings_surface_rule(&settings).expect("parse the overworld rule");

    let mut counts = BTreeMap::new();
    census(&rule, &mut counts);

    // Counted from the generator's output. Pinned rather than merely
    // printed: if a version bump adds an operator the parser does not know,
    // `parse` already errors — but if it *changes the mix*, this is what
    // says so, and a surface rule is exactly the kind of thing that changes
    // quietly between versions.
    let expected: BTreeMap<&str, usize> = [
        ("condition", 149),
        ("block", 111),
        ("sequence", 53),
        ("biome", 42),
        ("noise_threshold", 42),
        ("stone_depth", 23),
        ("water", 20),
        ("y_above", 10),
        ("steep", 5),
        ("not", 4),
        ("hole", 3),
        ("vertical_gradient", 2),
        ("bandlands", 2),
        ("above_preliminary_surface", 1),
        ("temperature", 1),
    ]
    .into_iter()
    .collect();

    assert_eq!(
        counts, expected,
        "the overworld surface rule's shape changed; if this is a version \
         bump, re-count from the extract and update the expectation"
    );
}

#[test]
fn every_shipped_dimension_parses() {
    // The overworld is the one with the interesting tree, but a parser that
    // only handles the overworld is a parser that will fail the first time
    // the nether is generated. `large_biomes` and `amplified` matter too:
    // they are separate presets with their own rules.
    for name in [
        "overworld.json",
        "nether.json",
        "end.json",
        "amplified.json",
        "large_biomes.json",
        "caves.json",
        "floating_islands.json",
    ] {
        let Some(settings) = extract(name) else {
            return;
        };
        let parsed = parse_noise_settings_surface_rule(&settings);
        assert!(parsed.is_ok(), "{name} failed to parse: {parsed:?}");
    }
}
