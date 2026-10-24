//! Biome tags resolve, including through nesting.
//!
//! `isValidBiome` samples one biome at the generation stub's position and asks
//! whether the structure's `biomes` HolderSet contains it. Those sets are
//! biome tags, and vanilla's biome tags nest:
//!
//! ```text
//!   has_structure/mineshaft_mesa  ->  ["#minecraft:is_badlands"]
//!   is_badlands                   ->  badlands, eroded_badlands, wooded_badlands
//!   has_structure/mineshaft       ->  eight nested tags plus literals
//! ```
//!
//! A resolver that treated a leading `#` as part of a biome name would return
//! an empty set, which reads as "no biome qualifies" — indistinguishable from
//! a correct refusal, and it would make every structure silently vanish rather
//! than fail. Hence a gate rather than a unit test on a synthetic tag.

use worldgen_assembly::facts::{resolve_biome_tag, resolve_structure_biomes};

fn have_extract() -> bool {
    // A different file from `worldgen_assembly::have_extract`'s, deliberately:
    // this gate reads the biome *tags*, and an extract can carry the noise
    // settings without them. Kept local for that reason, converted for the
    // same reason as the shared one.
    rustmc_data::fixture::available(
        &std::path::Path::new(worldgen_assembly::DATA)
            .join("tags/worldgen/biome/has_structure/mineshaft.json"),
        "biome tag extract",
        "ci/vanilla-server.sh data",
    )
}

/// Every `worldgen/structure/*.json` in the extract, id and parsed value.
fn structures() -> Vec<(String, serde_json::Value)> {
    let dir = std::path::Path::new(worldgen_assembly::DATA).join("worldgen/structure");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("worldgen/structure") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(std::ffi::OsStr::to_str) != Some("json") {
            continue;
        }
        let id = path
            .file_stem()
            .and_then(std::ffi::OsStr::to_str)
            .expect("stem")
            .to_string();
        let text = std::fs::read_to_string(&path).expect("read structure");
        out.push((id, serde_json::from_str(&text).expect("parse structure")));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[test]
fn the_mesa_tag_resolves_through_one_level_of_nesting() {
    if !have_extract() {
        return;
    }
    let mesa = resolve_biome_tag("has_structure/mineshaft_mesa").expect("resolve");
    println!("mineshaft_mesa -> {mesa:?}");
    assert_eq!(
        mesa.iter().map(String::as_str).collect::<Vec<_>>(),
        vec![
            "minecraft:badlands",
            "minecraft:eroded_badlands",
            "minecraft:wooded_badlands"
        ],
        "the mesa tag is exactly #is_badlands, resolved"
    );
}

/// The two mineshaft variants must not overlap, or the pick between them would
/// be ambiguous in some biome and this port's unconditional `mineshaft` could
/// be wrong even outside the badlands.
#[test]
fn the_two_mineshaft_tags_are_disjoint() {
    if !have_extract() {
        return;
    }
    let plain = resolve_biome_tag("has_structure/mineshaft").expect("resolve");
    let mesa = resolve_biome_tag("has_structure/mineshaft_mesa").expect("resolve");

    let both: Vec<&String> = plain.intersection(&mesa).collect();
    println!(
        "mineshaft {} biomes, mineshaft_mesa {} biomes, shared {}",
        plain.len(),
        mesa.len(),
        both.len()
    );
    assert!(
        both.is_empty(),
        "a biome in both tags makes the weighted pick observable outside the \
         badlands: {both:?}"
    );
    assert!(
        plain.len() > 20,
        "the plain tag should be most of the overworld, got {}",
        plain.len()
    );
}

/// Nothing resolves to a `#` — the marker of a nested tag that was stored as
/// a biome name instead of being followed.
#[test]
fn no_resolved_entry_is_still_a_tag_reference() {
    if !have_extract() {
        return;
    }
    for tag in [
        "has_structure/mineshaft",
        "has_structure/mineshaft_mesa",
        "has_structure/ocean_ruin_warm",
        "has_structure/ocean_ruin_cold",
    ] {
        let set = resolve_biome_tag(tag).expect("resolve");
        assert!(!set.is_empty(), "{tag} resolved to nothing");
        for biome in &set {
            assert!(
                !biome.starts_with('#'),
                "{tag} kept a nested reference {biome} instead of following it"
            );
        }
    }
}

/// The defect this file was extended for: `has_structure/<id>` is a convention,
/// not a rule, and `ruined_portal` is the one that makes it fatal.
///
/// `ruined_portal.json` declares `#minecraft:has_structure/ruined_portal_standard`
/// and no `has_structure/ruined_portal.json` exists, so a derived name does not
/// resolve to the wrong set — it does not resolve at all, and
/// `the_full_pipeline_decorates` failed with `No such file or directory`.
#[test]
fn the_derived_tag_name_is_wrong_for_ruined_portal() {
    if !have_extract() {
        return;
    }
    let derived = std::path::Path::new(worldgen_assembly::DATA)
        .join("tags/worldgen/biome/has_structure/ruined_portal.json");
    assert!(
        !derived.exists(),
        "the extract has grown a {}; the whole premise of reading `biomes` \
         instead of deriving it needs re-checking",
        derived.display()
    );

    let (_, value) = structures()
        .into_iter()
        .find(|(id, _)| id == "ruined_portal")
        .expect("ruined_portal.json");
    assert_eq!(
        value["biomes"].as_str(),
        Some("#minecraft:has_structure/ruined_portal_standard")
    );
    let read = resolve_structure_biomes(&value, "ruined_portal.json").expect("resolve");
    let standard = resolve_biome_tag("has_structure/ruined_portal_standard").expect("resolve");
    println!("ruined_portal -> {} biomes", read.len());
    assert_eq!(read, standard);
    assert!(!read.is_empty());
}

/// The other half of the same question, and the answer is the opposite one:
/// **main's derived name was right for the structures main's call site
/// serves.** Both shipwreck variants declare exactly the tag the convention
/// builds, so moving that site onto the read field changes no biome.
#[test]
fn mains_derived_name_was_right_for_shipwreck() {
    if !have_extract() {
        return;
    }
    for id in ["shipwreck", "shipwreck_beached"] {
        let (_, value) = structures()
            .into_iter()
            .find(|(s, _)| s == id)
            .unwrap_or_else(|| panic!("{id}.json"));
        let read = resolve_structure_biomes(&value, id).expect("resolve");
        let derived = resolve_biome_tag(&format!("has_structure/{id}")).expect("resolve");
        println!(
            "{id}: read {} biomes, derived {}",
            read.len(),
            derived.len()
        );
        assert!(!read.is_empty());
        assert_eq!(read, derived, "{id} moved when it should not have");
    }
}

/// A census of the convention over the whole extract, so the next agent does
/// not have to re-derive which structures it holds for. Five of 34 break it,
/// and the list is pinned: a sixth, or one leaving the list, is a change to the
/// data this port reads and should be read deliberately rather than absorbed.
#[test]
fn the_derived_name_census_over_the_whole_extract() {
    if !have_extract() {
        return;
    }
    let all = structures();
    let mut broken: Vec<(String, String)> = Vec::new();
    let mut lists = 0usize;
    for (id, value) in &all {
        match &value["biomes"] {
            serde_json::Value::String(s) => {
                if s != &format!("#minecraft:has_structure/{id}") {
                    broken.push((id.clone(), s.clone()));
                }
            }
            serde_json::Value::Array(_) => lists += 1,
            other => panic!("{id}: `biomes` is {other}"),
        }
        // Whatever the form, it must resolve to at least one biome. An empty
        // set places the structure nowhere and looks exactly like a refusal.
        let set = resolve_structure_biomes(value, id).expect("resolve");
        assert!(!set.is_empty(), "{id} resolved to nothing");
    }
    println!(
        "{} structures, {} tag strings, {} lists, {} break has_structure/<id>",
        all.len(),
        all.len() - lists,
        lists,
        broken.len()
    );
    for (id, tag) in &broken {
        println!("   {id} -> {tag}");
    }
    assert_eq!(all.len(), 34);
    assert_eq!(
        lists, 0,
        "a list form has appeared; the list branch of \
                          resolve_structure_biomes is now live and untested"
    );
    assert_eq!(
        broken
            .iter()
            .map(|(id, tag)| (id.as_str(), tag.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("fortress", "#minecraft:has_structure/nether_fortress"),
            ("jungle_pyramid", "#minecraft:has_structure/jungle_temple"),
            ("mansion", "#minecraft:has_structure/woodland_mansion"),
            ("monument", "#minecraft:has_structure/ocean_monument"),
            (
                "ruined_portal",
                "#minecraft:has_structure/ruined_portal_standard"
            ),
        ]
    );
}

/// The resolver refuses rather than returning an empty set, for every shape it
/// does not understand. Without this the failure mode is the one the module
/// doc names: nothing qualifies, nothing places, and no error is raised.
///
/// These run without the extract — they never reach the filesystem.
#[test]
fn an_unreadable_biomes_field_refuses_instead_of_resolving_to_nothing() {
    let cases = [
        (serde_json::json!({}), "no `biomes`"),
        (serde_json::json!({"biomes": []}), "no biomes at all"),
        (
            serde_json::json!({"biomes": 7}),
            "neither a string nor a list",
        ),
        (
            serde_json::json!({"biomes": ["#minecraft:is_forest"]}),
            "tag reference",
        ),
        (serde_json::json!({"biomes": [3]}), "not a string"),
    ];
    for (value, wanted) in cases {
        let err =
            resolve_structure_biomes(&value, "probe").expect_err(&format!("{value} resolved"));
        assert!(
            err.contains(wanted),
            "{value} gave {err:?}, wanted {wanted:?}"
        );
    }
    // And the two element shapes it *does* understand do not touch the disk.
    let one = resolve_structure_biomes(&serde_json::json!({"biomes": "minecraft:plains"}), "probe")
        .expect("bare id is compactListCodec's one-element form, not a tag");
    assert_eq!(
        one.iter().map(String::as_str).collect::<Vec<_>>(),
        vec!["minecraft:plains"]
    );
    let many = resolve_structure_biomes(
        &serde_json::json!({"biomes": ["minecraft:plains", "minecraft:swamp"]}),
        "probe",
    )
    .expect("list of elements");
    assert_eq!(many.len(), 2);
}
