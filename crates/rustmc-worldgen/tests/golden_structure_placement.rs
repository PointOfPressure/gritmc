//! Which chunks each structure set may start in, against vanilla.
//!
//! Structures are the largest single item left in the whole-world diff —
//! roughly 2,400 of the 5,374 remaining mismatches are one mineshaft
//! (BENCH.md). This is the floor of that work and it is worth gating on its
//! own: placement is a pure function of the seed and the chunk position, so
//! it can be proved before a single structure block exists. A generator that
//! builds the right pieces in the wrong chunks is not closer than one that
//! builds nothing.
//!
//! # Three columns that fail independently
//!
//! `isStructureChunk` is a conjunction, and comparing only the final answer
//! would let two compensating errors read green. So the vector carries, and
//! this compares:
//!
//! - **`potential`** — the chunk each region picks, for 49 regions per set.
//!   This pins the jitter even where the frequency coin rejects everything in
//!   sight, which for the mineshafts is almost everywhere.
//! - **`placement_chunks`** — the grid alone, over 9,409 chunks.
//! - **`structure_chunks`** — grid *and* frequency coin.
//!
//! The middle and the last differ for exactly three sets, which is the point:
//! 17 of the 20 have no frequency coin at all, so for them the two columns
//! are equal by construction and only the three that matter can disagree.
//!
//! # The gap this vector has, stated out loud
//!
//! It does **not** cover the exclusion zone. `isStructureChunk` takes a
//! `ChunkGeneratorStructureState` and one cannot be built from
//! `VanillaRegistries.createLookup()` — `createForNormal` filters through
//! `hasBiomesForStructureSet`, which dereferences biome tags, and the lookup
//! throws `can't be dereferenced during construction`. The dumper records the
//! two seed-only parts instead, which is `isStructureChunk` in full for 19 of
//! 20 sets; `pillager_outposts` is the exception and its third test is
//! covered by unit tests in `structure::placement` only.
//!
//! That is a real gap. It is asserted below — the gate checks that exactly
//! one set is affected, so the day a second one grows an exclusion zone this
//! fails rather than quietly widening.

mod common;

use common::read;

use rustmc_worldgen::structure::placement::{Spread, StructurePlacement};

const GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/golden/structure_placement_vectors.json"
);

/// The datapack's structure sets, by name.
fn placements() -> Option<std::collections::BTreeMap<String, StructurePlacement>> {
    let dir = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../oracle-work/vanilla-26.2/generated-data/data/minecraft/worldgen/structure_set"
    ));
    if !rustmc_data::fixture::available(dir, "structure-set extract", "ci/vanilla-server.sh data") {
        return None;
    }
    let mut out = std::collections::BTreeMap::new();
    for entry in std::fs::read_dir(dir).ok()? {
        let path = entry.ok()?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let name = format!("minecraft:{}", path.file_stem()?.to_str()?);
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).ok()?).ok()?;
        out.insert(name, StructurePlacement::parse(&json["placement"]).ok()?);
    }
    Some(out)
}

fn pairs(value: &serde_json::Value) -> Vec<(i32, i32)> {
    value
        .as_array()
        .expect("a list of chunks")
        .iter()
        .map(|entry| {
            let entry = entry.as_array().expect("[x, z]");
            (
                i32::try_from(entry[0].as_i64().expect("x")).expect("i32"),
                i32::try_from(entry[1].as_i64().expect("z")).expect("i32"),
            )
        })
        .collect()
}

#[test]
fn structure_placement_matches_vanilla() {
    let Some(placements) = placements() else {
        return;
    };
    let doc = read(GOLDEN);
    let radius = i32::try_from(doc["radius"].as_i64().expect("radius")).expect("i32");
    let cases = doc["cases"].as_array().expect("cases");
    assert!(!cases.is_empty(), "the vector file has no cases");

    let (mut grid_checked, mut grid_wrong) = (0usize, 0usize);
    let mut full_wrong = 0usize;
    let (mut potential_checked, mut potential_wrong) = (0usize, 0usize);
    let mut sets_seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    let mut sets_with_a_coin: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    let mut examples: Vec<String> = Vec::new();

    for case in cases {
        let name = case["set"].as_str().expect("set");
        let seed: i64 = case["seed"].as_str().expect("seed").parse().expect("i64");
        let random_spread = case["random_spread"].as_bool().expect("random_spread");
        let placement = placements
            .get(name)
            .unwrap_or_else(|| panic!("{name} is not in the datapack extract"));
        sets_seen.insert(name);

        // The one set vanilla does not spread randomly is the one this port
        // refuses by name, and the dump agrees about which it is.
        assert_eq!(
            random_spread,
            matches!(placement.spread, Spread::RandomSpread { .. }),
            "{name}: the port and the oracle disagree about the placement kind"
        );
        if !random_spread {
            continue;
        }

        // ---- the region picks ----
        for entry in case["potential"].as_array().expect("potential") {
            let entry = entry.as_array().expect("[x, z, px, pz]");
            let x = i32::try_from(entry[0].as_i64().expect("x")).expect("i32");
            let z = i32::try_from(entry[1].as_i64().expect("z")).expect("i32");
            let want = (
                i32::try_from(entry[2].as_i64().expect("px")).expect("i32"),
                i32::try_from(entry[3].as_i64().expect("pz")).expect("i32"),
            );
            let got = placement
                .potential_structure_chunk(seed, x, z)
                .expect("a random spread");
            potential_checked += 1;
            if got != want {
                potential_wrong += 1;
                if examples.len() < 8 {
                    examples.push(format!(
                        "{name} seed {seed} region at ({x}, {z}): picked {got:?}, want {want:?}"
                    ));
                }
            }
        }

        // ---- the grid, and the grid plus the coin ----
        let want_grid: std::collections::BTreeSet<(i32, i32)> =
            pairs(&case["placement_chunks"]).into_iter().collect();
        let want_full: std::collections::BTreeSet<(i32, i32)> =
            pairs(&case["structure_chunks"]).into_iter().collect();
        if want_grid != want_full {
            sets_with_a_coin.insert(name);
        }

        let mut got_grid = std::collections::BTreeSet::new();
        let mut got_full = std::collections::BTreeSet::new();
        for x in -radius..=radius {
            for z in -radius..=radius {
                if placement
                    .is_placement_chunk(seed, x, z)
                    .expect("a random spread")
                {
                    got_grid.insert((x, z));
                    if placement.passes_frequency(seed, x, z) {
                        got_full.insert((x, z));
                    }
                }
            }
        }

        let area = usize::try_from((radius * 2 + 1) * (radius * 2 + 1)).expect("usize");
        grid_checked += area;
        let grid_diff = got_grid.symmetric_difference(&want_grid).count();
        let full_diff = got_full.symmetric_difference(&want_full).count();
        grid_wrong += grid_diff;
        full_wrong += full_diff;
        if grid_diff > 0 && examples.len() < 8 {
            let sample: Vec<_> = got_grid.symmetric_difference(&want_grid).take(3).collect();
            examples.push(format!(
                "{name} seed {seed}: {grid_diff} grid chunks differ, e.g. {sample:?}"
            ));
        }
        if full_diff > 0 && grid_diff == 0 && examples.len() < 8 {
            let sample: Vec<_> = got_full.symmetric_difference(&want_full).take(3).collect();
            examples.push(format!(
                "{name} seed {seed}: the grid agrees but {full_diff} frequency answers differ, \
                 e.g. {sample:?}"
            ));
        }
    }

    for line in &examples {
        println!("  {line}");
    }
    println!(
        "STRUCTURE PLACEMENT: {} sets over {} chunk decisions — grid {} wrong, \
         grid+frequency {} wrong, {} region picks {} wrong",
        sets_seen.len(),
        grid_checked,
        grid_wrong,
        full_wrong,
        potential_checked,
        potential_wrong
    );

    assert_eq!(potential_wrong, 0, "the jitter is wrong");
    assert_eq!(grid_wrong, 0, "the spread grid is wrong");
    assert_eq!(full_wrong, 0, "the frequency reducer is wrong");

    // Coverage, asserted rather than assumed. Without these the gate would
    // still read 100% if the dumper started emitting one seed and one set.
    assert_eq!(
        sets_seen.len(),
        20,
        "the structure set registry changed size"
    );
    assert!(
        potential_checked >= 3000,
        "only {potential_checked} region picks compared"
    );
    assert_eq!(
        sets_with_a_coin.len(),
        3,
        "exactly three sets have a frequency coin: {sets_with_a_coin:?}"
    );

    // The named gap. See the module docs: the exclusion zone is not in this
    // vector, and this fails the day a second set grows one.
    let with_exclusion: Vec<&String> = placements
        .iter()
        .filter(|(_, p)| p.exclusion_zone.is_some())
        .map(|(name, _)| name)
        .collect();
    assert_eq!(
        with_exclusion,
        vec!["minecraft:pillager_outposts"],
        "a set grew an exclusion zone, which this vector cannot express"
    );
}
