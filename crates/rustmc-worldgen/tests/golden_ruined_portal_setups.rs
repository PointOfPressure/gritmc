//! The ruined-portal `setups` fixtures against the pinned extract.
//!
//! `structure::ruined_portal`'s unit tests carry `ruined_portal.json` and
//! `ruined_portal_ocean.json` as hardcoded fixtures, because the four-portal
//! oracle has to run without touching the disk. That makes them a **second
//! copy** of vanilla's data, and a second copy that nothing checks is how a
//! data change becomes a silent parity regression.
//!
//! This re-reads the extract and fails if the two ever part. It also parses
//! every one of the seven ruined-portal structure files, so a new placement
//! name or a missing field is caught here rather than in a world diff.

use rustmc_worldgen::structure::ruined_portal::{Setup, VerticalPlacement};

fn structure_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../oracle-work/vanilla-26.2/generated-data/data/minecraft/worldgen/structure")
}

fn setups_of(name: &str) -> Vec<Setup> {
    let path = structure_dir().join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    let value: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    value
        .get("setups")
        .and_then(serde_json::Value::as_array)
        .unwrap_or_else(|| panic!("{name}.json has no `setups` array"))
        .iter()
        .map(|v| Setup::parse(v).unwrap_or_else(|e| panic!("{name}: {e}")))
        .collect()
}

/// The exact fixture the four-portal oracle runs against.
#[test]
fn the_standard_portal_has_the_two_setups_the_fixture_claims() {
    let setups = setups_of("ruined_portal");
    assert_eq!(
        setups.len(),
        2,
        "the weighted draw depends on there being two"
    );

    assert_eq!(setups[0].placement, VerticalPlacement::Underground);
    assert_eq!(setups[0].air_pocket_probability, 1.0);
    assert_eq!(setups[0].mossiness, 0.2);
    assert_eq!(setups[0].weight, 0.5);
    assert!(setups[0].can_be_cold);
    assert!(!setups[0].overgrown);
    assert!(!setups[0].vines);
    assert!(!setups[0].replace_with_blackstone);

    assert_eq!(setups[1].placement, VerticalPlacement::OnLandSurface);
    assert_eq!(setups[1].air_pocket_probability, 0.5);
    assert_eq!(setups[1].mossiness, 0.2);
    assert_eq!(setups[1].weight, 0.5);
}

/// One setup and probability 0.0 — the case that takes neither the weighted
/// draw nor the air-pocket draw.
#[test]
fn the_ocean_portal_takes_neither_of_the_two_optional_draws() {
    let setups = setups_of("ruined_portal_ocean");
    assert_eq!(setups.len(), 1, "one setup means no weighted draw");
    assert_eq!(setups[0].placement, VerticalPlacement::OnOceanFloor);
    assert_eq!(
        setups[0].air_pocket_probability, 0.0,
        "0.0 short-circuits and draws nothing"
    );
    assert_eq!(setups[0].mossiness, 0.8);
}

/// Every ruined-portal file parses, and the set of them is the one
/// `structure::step_index` sorts.
#[test]
fn all_seven_ruined_portal_files_parse() {
    const NAMES: [&str; 7] = [
        "ruined_portal",
        "ruined_portal_desert",
        "ruined_portal_jungle",
        "ruined_portal_mountain",
        "ruined_portal_nether",
        "ruined_portal_ocean",
        "ruined_portal_swamp",
    ];
    for name in NAMES {
        let setups = setups_of(name);
        assert!(!setups.is_empty(), "{name} has no setups");
        // A weight of zero would make the weighted pick divide by a total
        // that a single-entry list never computes, so it is worth refusing
        // to believe.
        assert!(
            setups.iter().all(|s| s.weight > 0.0),
            "{name} has a zero weight"
        );
    }
}

/// The nether portal is the only one that reaches the `in_nether` branch, and
/// it is the branch no reference on the box exercises. Recorded here so the
/// unmeasured branch is named in a gate rather than only in a doc comment.
#[test]
fn the_nether_portal_is_the_only_in_nether_placement() {
    let nether = setups_of("ruined_portal_nether");
    assert!(
        nether
            .iter()
            .any(|s| s.placement == VerticalPlacement::InNether),
        "ruined_portal_nether should place in_nether"
    );
    for name in [
        "ruined_portal",
        "ruined_portal_ocean",
        "ruined_portal_desert",
    ] {
        assert!(
            setups_of(name)
                .iter()
                .all(|s| s.placement != VerticalPlacement::InNether),
            "{name} should not place in_nether"
        );
    }
}
