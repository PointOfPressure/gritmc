//! Every template pool and every jigsaw structure in the datapack parses.
//!
//! Not a golden-vector gate: there is nothing to compare against, because a
//! pool is a static file and the only question is whether this port's schema
//! is the one Mojang shipped. What makes it a gate rather than a smoke test
//! is the **counts**. A version bump that adds an element type, a projection,
//! a height provider or a terrain adjustment currently fails as an unknown
//! name; one that adds a whole pool or a whole jigsaw structure would
//! otherwise pass silently while the port quietly did not generate it.
//!
//! Ten of the thirty-four structure definitions in 26.2 are `minecraft:jigsaw`
//! — five villages, the pillager outpost, the bastion, the ancient city,
//! trail ruins and trial chambers — so this one reader is the shared spine
//! for over half of what is still missing from `structure/`.
//!
//! # What this deliberately does not do
//!
//! It does not choose an element. `StructureTemplatePool.getShuffledTemplates`
//! spends draws, and the order it produces has to come off `Util.shuffle` and
//! `WeightedList` bytecode rather than out of a plausible reading — that is
//! the assembler's gate to write, against `DecorateGolden`-shaped evidence.
//! A parser gate that also asserted a draw order would be asserting a guess.

mod common;

use std::collections::BTreeMap;

use common::{data_path, have_extract, read};
use rustmc_worldgen::structure::template::{Rotation, Template};
use rustmc_worldgen::structure::template_pool::{
    JigsawStructure, PoolElement, TemplatePool, TerrainAdaptation,
};

/// Every `.json` under a datapack directory, by registry name, depth-first so
/// the failure message names a stable file.
fn tree(relative: &str) -> BTreeMap<String, std::path::PathBuf> {
    let root = data_path(relative);
    let mut out = BTreeMap::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "json") {
                let name = path
                    .strip_prefix(&root)
                    .expect("under root")
                    .with_extension("")
                    .to_string_lossy()
                    .replace('\\', "/");
                out.insert(name, path);
            }
        }
    }
    out
}

#[test]
fn every_template_pool_in_the_datapack_parses() {
    if !have_extract() {
        return;
    }
    let files = tree("worldgen/template_pool");
    assert_eq!(
        files.len(),
        188,
        "the datapack's pool count changed — the census in template_pool.rs is stale"
    );

    let mut entries = 0usize;
    let mut expanded_total = 0usize;
    let mut widest = (String::new(), 0usize);
    let mut singles = 0usize;
    let mut legacy = 0usize;
    let mut empties = 0usize;
    let mut features = 0usize;
    let mut lists = 0usize;
    let mut fallbacks = 0usize;

    for (name, path) in &files {
        let value = read(path.to_str().expect("utf-8 path"));
        let pool = TemplatePool::parse(&value)
            .unwrap_or_else(|e| panic!("{name} does not parse: {}", e.0));
        // A fallback that names no pool is a branch that cannot terminate.
        let fallback = pool
            .fallback
            .strip_prefix("minecraft:")
            .unwrap_or_else(|| panic!("{name}: unnamespaced fallback `{}`", pool.fallback));
        assert!(
            files.contains_key(fallback),
            "{name}: fallback `{}` is not a pool in the datapack",
            pool.fallback
        );
        if pool.fallback != "minecraft:empty" {
            fallbacks += 1;
        }
        // Exactly one pool in the datapack has no elements, and it is
        // `minecraft:empty` — the fallback every other pool names. Any second
        // elementless pool is a branch of some structure that can never place
        // anything, which is a data reading worth failing on.
        assert_eq!(
            pool.is_empty(),
            name == "empty",
            "{name}: only `empty` may have no elements"
        );
        assert!(
            pool.is_empty() || pool.total_weight() > 0,
            "{name} sums to a non-positive weight"
        );
        let expanded = pool.expanded();
        expanded_total += expanded.len();
        if expanded.len() > widest.1 {
            widest = (name.clone(), expanded.len());
        }
        assert_eq!(
            expanded.len(),
            usize::try_from(pool.total_weight().max(0)).expect("usize"),
            "{name}: the expanded list is the sum of the weights"
        );
        for entry in &pool.elements {
            entries += 1;
            match &entry.element {
                PoolElement::Empty => empties += 1,
                PoolElement::Single {
                    location,
                    legacy: l,
                    ..
                } => {
                    singles += 1;
                    legacy += usize::from(*l);
                    assert!(
                        location.starts_with("minecraft:"),
                        "{name}: unnamespaced template `{location}`"
                    );
                }
                PoolElement::Feature { .. } => features += 1,
                PoolElement::List { elements, .. } => {
                    lists += 1;
                    assert!(
                        !elements.is_empty(),
                        "{name}: a list element with no children places nothing"
                    );
                    for child in elements {
                        assert!(
                            !matches!(child, PoolElement::List { .. }),
                            "{name}: nested list elements are unported"
                        );
                    }
                }
            }
        }
    }

    // The census template_pool.rs was written against. Every one of these is
    // a ratchet: a bump that moves one has changed the shape of the data.
    assert_eq!(entries, 1198, "pool entries");
    assert_eq!(singles, 527 + 601, "single + legacy_single elements");
    assert_eq!(legacy, 601, "legacy_single elements");
    assert_eq!(empties, 31, "empty elements");
    assert_eq!(features, 36, "feature elements");
    assert_eq!(lists, 3, "list elements");
    // **Not** every pool falls back to `minecraft:empty` — 22 of the 188 name
    // a real pool, and this gate was written asserting 0 and corrected by its
    // own first run. It matters: the assembler's fallback walk has to follow
    // a chain rather than stopping at the first miss, and a terminator pool
    // that falls back to a terminator pool is how a village's streets end.
    assert_eq!(fallbacks, 22, "pools with a non-empty fallback");

    // The expanded list is what every selection draws against, so its size is
    // the draw cost: `getShuffledTemplates` spends one fewer than this every
    // time it touches a pool. 1198 raw entries become **4880** slots, because
    // village pools weight their ordinary elements at 50 against a zombie
    // variant's 1 — so a village's very first decision is a `nextInt` in the
    // hundreds, not in the tens.
    //
    // Ratcheted because a port that scanned cumulative weights over the raw
    // entries instead would agree with vanilla on every distribution here and
    // on no stream at all. (Both numbers below were written from a guess on
    // the first draft and corrected by this assertion; measure, do not
    // estimate.)
    assert_eq!(
        expanded_total, 4880,
        "weight-expanded slots across all pools"
    );
    assert_eq!(
        widest,
        ("trial_chambers/hallway".to_string(), 1231),
        "the widest pool — 1230 draws for one shuffle, and the reason \
         `expanded` returns indices rather than cloning elements"
    );
}

#[test]
fn every_jigsaw_structure_parses_and_names_a_pool_that_exists() {
    if !have_extract() {
        return;
    }
    let pools = tree("worldgen/template_pool");
    let structures = tree("worldgen/structure");
    assert_eq!(
        structures.len(),
        34,
        "structure definitions in the datapack"
    );

    let mut jigsaws = Vec::new();
    for (name, path) in &structures {
        let value = read(path.to_str().expect("utf-8 path"));
        if value["type"].as_str() != Some("minecraft:jigsaw") {
            continue;
        }
        let structure = JigsawStructure::parse(&value)
            .unwrap_or_else(|e| panic!("{name} does not parse: {}", e.0));
        // A start pool that does not exist is a structure that generates
        // nothing, and it is the one cross-file claim this gate can make.
        let key = structure
            .start_pool
            .strip_prefix("minecraft:")
            .expect("namespaced start pool");
        assert!(
            pools.contains_key(key),
            "{name}: start_pool `{}` is not a pool in the datapack",
            structure.start_pool
        );
        assert!(
            structure.size > 0,
            "{name}: size {} places nothing",
            structure.size
        );
        assert!(
            structure.max_distance_from_center > 0,
            "{name}: max_distance_from_center is not positive"
        );
        jigsaws.push((name.clone(), structure));
    }

    assert_eq!(jigsaws.len(), 10, "jigsaw structures");
    let named: BTreeMap<&str, &JigsawStructure> =
        jigsaws.iter().map(|(n, s)| (n.as_str(), s)).collect();

    // Spot claims, each one a fact the assembler will depend on and none of
    // them derivable from another.
    let plains = named["village_plains"];
    assert_eq!(plains.start_pool, "minecraft:village/plains/town_centers");
    assert_eq!(plains.size, 6);
    assert!(plains.use_expansion_hack, "villages use the expansion hack");
    assert_eq!(plains.terrain_adaptation, TerrainAdaptation::BeardThin);
    assert_eq!(plains.step, "surface_structures");
    assert!(
        plains.project_start_to_heightmap.is_some(),
        "a village stands on the terrain, not at its start height"
    );

    let city = named["ancient_city"];
    assert_eq!(
        city.start_jigsaw_name.as_deref(),
        Some("minecraft:city_anchor"),
        "the ancient city is the only structure that anchors its start piece"
    );
    assert!(!city.use_expansion_hack);
    assert_eq!(city.terrain_adaptation, TerrainAdaptation::BeardBox);

    // The expansion hack is on for exactly the five villages and the outpost.
    let hacked: Vec<&str> = jigsaws
        .iter()
        .filter(|(_, s)| s.use_expansion_hack)
        .map(|(n, _)| n.as_str())
        .collect();
    assert_eq!(
        hacked,
        vec![
            "pillager_outpost",
            "village_desert",
            "village_plains",
            "village_savanna",
            "village_snowy",
            "village_taiga",
        ]
    );
}

/// Every `.nbt` under a template directory, by registry name.
fn templates(relative: &str) -> BTreeMap<String, std::path::PathBuf> {
    let root = data_path(relative);
    let mut out = BTreeMap::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "nbt") {
                let name = path
                    .strip_prefix(&root)
                    .expect("under root")
                    .with_extension("")
                    .to_string_lossy()
                    .replace('\\', "/");
                out.insert(name, path);
            }
        }
    }
    out
}

/// Every village template's jigsaw blocks parse, and every pool they name is
/// a pool the datapack actually has.
///
/// This is the claim that makes the jigsaw *walk* worth writing: a village is
/// only as connected as its templates say it is, and a template whose jigsaw
/// names a pool that does not exist is a branch that can never expand. It is
/// also the only end-to-end check available before the assembler exists —
/// template NBT on one side, pool JSON on the other, and nothing in between
/// to hide a mistake.
#[test]
fn every_village_template_names_pools_that_exist() {
    if !have_extract() {
        return;
    }
    let pools = tree("worldgen/template_pool");
    let files = templates("structure/village");
    assert!(
        files.len() > 100,
        "only {} village templates found — the extract looks partial",
        files.len()
    );

    let mut jigsaws = 0usize;
    let mut targets: BTreeMap<String, usize> = BTreeMap::new();
    let mut joints: BTreeMap<String, usize> = BTreeMap::new();
    let mut states: BTreeMap<String, usize> = BTreeMap::new();
    let mut prioritised = 0usize;
    for (name, path) in &files {
        let file = std::fs::File::open(path).unwrap_or_else(|e| panic!("{name} unreadable: {e}"));
        let mut reader = flate2::read::GzDecoder::new(file);
        let (_, root) = rustmc_world::nbt::read_named_root(&mut reader)
            .unwrap_or_else(|e| panic!("{name} is not gzipped NBT: {e}"));
        let template =
            Template::from_nbt(&root).unwrap_or_else(|e| panic!("{name} does not parse: {}", e.0));
        // Identity transform: this gate is about what the templates *say*,
        // not where a rotated copy of them lands. `jigsaws` is the rotated
        // accessor because that is the only shape the assembler ever wants,
        // and `(0,0,0)` with `Rotation::None` is its identity case.
        for block in template.jigsaws((0, 0, 0), Rotation::None) {
            jigsaws += 1;
            *states.entry(block.state.clone()).or_default() += 1;
            *targets.entry(block.nbt.target.clone()).or_default() += 1;
            // `joint` is a bool on this side, but the census below is worth
            // keeping in vanilla's own words so a version bump that flips one
            // template reads as a moved count rather than a moved flag.
            *joints
                .entry(if block.nbt.rollable {
                    "rollable".to_string()
                } else {
                    "aligned".to_string()
                })
                .or_default() += 1;
            if block.nbt.selection_priority != 0 || block.nbt.placement_priority != 0 {
                prioritised += 1;
            }
            let key = block
                .nbt
                .pool
                .strip_prefix("minecraft:")
                .unwrap_or_else(|| panic!("{name}: unnamespaced pool `{}`", block.nbt.pool));
            assert!(
                pools.contains_key(key),
                "{name} at {:?}: jigsaw pool `{}` is not a pool in the datapack",
                block.pos,
                block.nbt.pool
            );
            assert!(
                !block.nbt.final_state.is_empty(),
                "{name}: a jigsaw with no final_state cannot be cleared away"
            );
        }
    }

    println!(
        "{} village templates, {jigsaws} jigsaw blocks, {} distinct targets",
        files.len(),
        targets.len()
    );
    println!("joints: {joints:?}");
    println!("jigsaw block states: {states:?}");
    println!("{prioritised} jigsaws carry a non-zero priority");

    // Measured, and ratcheted so a version bump that reshapes the villages
    // fails here rather than in a diff nobody can attribute.
    assert_eq!(files.len(), 483, "village templates");
    assert_eq!(jigsaws, 1917, "jigsaw blocks across the villages");
    // Both joint types are real and the assembler has to honour both: a
    // rollable joint may take any of the four rotations, an aligned one keeps
    // the facing it was authored with. The split is close to even, so a port
    // that implemented only one would look half-right on inspection.
    assert_eq!(joints.get("aligned").copied(), Some(1156));
    assert_eq!(joints.get("rollable").copied(), Some(761));
    // **No village jigsaw sets either priority.** Both default to 0, so for
    // villages `sortBySelectionPriority` is a stable sort of equal keys — a
    // no-op — and the shuffle alone decides the order. That does not make the
    // sort optional: the ancient city and trial chambers do use priorities,
    // and a village gate cannot see a sort that is not exercised.
    assert_eq!(prioritised, 0, "village jigsaws with a non-zero priority");
}
