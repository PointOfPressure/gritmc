//! How many structure sets are there, and how many does this server serve?
//!
//! # The denominator was remembered, not derived
//!
//! "Twenty structure sets", "34 structures", "8 of 20 placed" have been quoted
//! across `docs/SERVER.md`, `docs/FINDINGS.md` and several briefs, and until
//! 2026-09-05 no file in this tree held either number. `data/vanilla/structure_sets.json`
//! did exist — read by nine source files — and **no script produced it**: it
//! had no `.provenance` stamp, so `ci/t2-gameplay.sh`'s foreign-table refusal
//! could not see it, and `ci/gen-gameplay-tables.sh`'s derived list did not
//! require it. Whatever tree last wrote it decided what every other tree
//! measured.
//!
//! `ci/dump-structure-tables.sh` now produces it out of the pinned jar,
//! reproducing the inherited copy byte for byte, along with three tables
//! beside it that nothing had. This test is what reads them, so they are live
//! data rather than four more files nobody opens.
//!
//! # What is asserted, and what each assertion would catch
//!
//! * the four counts — 20 sets, 34 structure definitions, 188 template pools,
//!   1,212 templates — which go red when the pinned jar moves, which is
//!   exactly when every structure figure in the docs needs re-taking;
//! * that the sets name **all 34** definitions and no more, so neither table
//!   can drift against the other;
//! * that every set this tree claims to serve is a real set name, so a typo
//!   in the census cannot pass as coverage;
//! * that the `data/vanilla/structure/` extract matches the manifest, naming
//!   the files it lacks rather than counting them.
//!
//! # The served census, and what its evidence is
//!
//! [`SERVED`] is a claim about this tree and the test checks the part of it
//! that is checkable: the names are real and the count is 17 (sixteen until
//! ocean monuments landed, fifteen until ancient cities did, seven until
//! 2026-10-01). Sixteen of them have a marker window in
//! `tests/served_structures.rs` that decodes a served
//! chunk and finds that set's blocks in it; `buried_treasures` has something
//! stronger instead — `tests/served_vs_vanilla.rs`'s
//! `the_boxes_outside_the_parity_window` compares its single block against
//! vanilla's own saved chest at (1177,53,745), state string and all. A marker
//! window would have been weaker there, because `minecraft:chest` is a block
//! four other ported structures also place.
//!
//! `Evidence::CodePath` exists because ocean ruin and shipwreck spent one
//! commit in it — laid out by `worldgen_assembly::generate::generate_resuming`
//! with no window located — and the distinction is kept in the type rather
//! than deleted, because the next set ported will start there too.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

fn repo(relative: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

/// The four table paths, as **quoted literals**.
///
/// Not `format!("data/vanilla/{name}")`, and the difference is the whole
/// registration. `ci/gen-gameplay-tables.sh` derives the set of tables a tree
/// must generate by grepping `crates`, `tools` and `plugins` for
/// `"data/vanilla/[A-Za-z0-9_]+\.(json|bin)"`, so a computed path derives
/// nothing — the script's own header says so, and says that the residual is a
/// code-review item rather than something the grep can see. These four
/// literals are what make the tables required, stamped, and refusable when
/// they come from another tree.
const SETS: &str = "data/vanilla/structure_sets.json";
const DEFS: &str = "data/vanilla/structure_defs.json";
const POOLS: &str = "data/vanilla/template_pools.json";
const TEMPLATES: &str = "data/vanilla/structure_templates.json";

/// One table, or `None` if the extract is not on disk.
fn table(relative: &str) -> Option<BTreeMap<String, Value>> {
    let path = repo(relative);
    if !rustmc_data::fixture::available(&path, relative, "ci/dump-structure-tables.sh") {
        return None;
    }
    let text = std::fs::read_to_string(&path).expect("readable table");
    Some(serde_json::from_str(&text).expect("the table is a json object"))
}

/// What evidence there is that a set reaches a player.
///
/// `CodePath` currently has no rows and is kept anyway — see the module
/// header. The next set ported starts there, and a distinction deleted the
/// moment it is empty is a distinction the next agent re-derives.
#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Evidence {
    /// A window in `tests/served_structures.rs` decodes the served chunk and
    /// finds this set's marker blocks in it.
    Wire,
    /// `generate_resuming` lays the set out and no window has been located.
    /// A reading of code. Not the same evidence and not counted as it.
    CodePath,
}

/// The seventeen overworld sets `worldgen_assembly::generate::generate_resuming`
/// lays out, and what is known about each — every overworld set in the jar.
const SERVED: [(&str, Evidence); 17] = [
    ("minecraft:ancient_cities", Evidence::Wire),
    ("minecraft:buried_treasures", Evidence::Wire),
    ("minecraft:desert_pyramids", Evidence::Wire),
    ("minecraft:igloos", Evidence::Wire),
    ("minecraft:jungle_temples", Evidence::Wire),
    ("minecraft:mineshafts", Evidence::Wire),
    ("minecraft:ocean_monuments", Evidence::Wire),
    ("minecraft:ocean_ruins", Evidence::Wire),
    ("minecraft:pillager_outposts", Evidence::Wire),
    ("minecraft:ruined_portals", Evidence::Wire),
    ("minecraft:shipwrecks", Evidence::Wire),
    ("minecraft:strongholds", Evidence::Wire),
    ("minecraft:swamp_huts", Evidence::Wire),
    ("minecraft:trail_ruins", Evidence::Wire),
    ("minecraft:trial_chambers", Evidence::Wire),
    ("minecraft:villages", Evidence::Wire),
    ("minecraft:woodland_mansions", Evidence::Wire),
];

#[test]
fn the_jar_has_twenty_sets_naming_thirty_four_structures() {
    let (Some(sets), Some(defs)) = (table(SETS), table(DEFS)) else {
        return;
    };
    assert_eq!(sets.len(), 20, "structure sets in the pinned jar");
    assert_eq!(defs.len(), 34, "structure definitions in the pinned jar");

    // Every set names structures, and between them they name every
    // definition. A set pointing at a structure with no definition, or a
    // definition no set ever places, would both be silent otherwise.
    let mut named: BTreeSet<String> = BTreeSet::new();
    for (set, body) in &sets {
        let entries = body["structures"]
            .as_array()
            .unwrap_or_else(|| panic!("{set} has no `structures` array"));
        assert!(!entries.is_empty(), "{set} names no structures");
        for e in entries {
            named.insert(
                e["structure"]
                    .as_str()
                    .unwrap_or_else(|| panic!("{set} has a nameless structure entry"))
                    .to_string(),
            );
        }
    }
    let defined: BTreeSet<String> = defs.keys().cloned().collect();
    assert_eq!(
        named, defined,
        "the sets and the definitions do not name the same structures"
    );
    assert_eq!(named.len(), 34);
}

#[test]
fn the_pools_and_templates_are_the_jars() {
    let (Some(pools), Some(templates)) = (table(POOLS), table(TEMPLATES)) else {
        return;
    };
    assert_eq!(pools.len(), 188, "jigsaw template pools in the pinned jar");
    assert_eq!(templates.len(), 1212, "structure .nbt templates in the jar");

    // The extract on disk against the manifest. This is what makes
    // `data/vanilla/structure/` a derived directory rather than an inherited
    // one: 1,195 of the 1,212 were present on the copy every tree on the box
    // was seeded from, all CRC-clean, and the seventeen absent were
    // `empty.nbt` plus the sixteen `fossil/*` — which belong to the overworld
    // `fossil` FEATURE, not to `nether_fossils`.
    let dir = repo("data/vanilla/structure");
    if !dir.is_dir() {
        return;
    }
    let missing: Vec<&String> = templates
        .keys()
        .filter(|k| {
            let rel = k.strip_prefix("minecraft:").unwrap_or(k);
            !dir.join(format!("{rel}.nbt")).exists()
        })
        .collect();
    assert!(
        missing.is_empty(),
        "{} template(s) in the jar manifest are not in data/vanilla/structure: {:?}. \
         Run ci/dump-structure-tables.sh, which extracts and CRC-verifies them.",
        missing.len(),
        &missing[..missing.len().min(6)]
    );
}

#[test]
fn the_served_census_names_real_sets_and_says_what_it_measured() {
    let Some(sets) = table(SETS) else {
        return;
    };
    for (name, _) in SERVED {
        assert!(
            sets.contains_key(name),
            "{name} is in this tree's served census and is not a structure set \
             in the pinned jar — the census is naming something that does not exist"
        );
    }
    assert_eq!(SERVED.len(), 17, "structure sets this server lays out");
    let on_the_wire = SERVED.iter().filter(|(_, e)| *e == Evidence::Wire).count();
    assert_eq!(
        on_the_wire, 17,
        "sets with a decoded-chunk window in tests/served_structures.rs"
    );
    assert_eq!(
        sets.len() - SERVED.len(),
        3,
        "structure sets this server does not lay out"
    );
}
