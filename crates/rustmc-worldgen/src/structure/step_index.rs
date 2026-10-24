//! Which index a structure is seeded with inside its decoration step.
//!
//! Reference (26.2 bytecode, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `ChunkGenerator#applyBiomeDecoration`,
//! `ResourceManagerRegistryLoadTask#load`, `RegistryLoadTask#registerElements`,
//! `GenerationStep$Decoration`, `Identifier#compareTo`.
//!
//! Every structure written during decoration is seeded with
//! `setFeatureSeed(populationSeed, index, step)`, and until 2026-08-06 this
//! port could not say what `index` was. The tree recorded the reason in two
//! places — `structure/shipwreck.rs` and `worldgen-assembly`'s decoration
//! hook — as "not readable from a registry whose iteration order is not
//! stable between JVM runs". **That is wrong, and the bytecode says so
//! directly.** It cost a beached shipwreck, a hay pile and sixteen structure
//! sets, so the refutation is written out here in full rather than left as a
//! constant somebody has to trust.
//!
//! # Where the order actually comes from
//!
//! `applyBiomeDecoration` groups the structure registry by step and walks
//! each group with a counter:
//!
//! ```text
//! byStep = registry.stream().collect(groupingBy(s -> s.step().ordinal()))
//! for step in 0..max:
//!     index = 0                                  // reset per step
//!     for structure in byStep.getOrDefault(step, []):
//!         setFeatureSeed(popSeed, index, step)
//!         ... place this structure's starts ...
//!         index += 1
//!     for feature in this step's features:        // SAME counter
//!         setFeatureSeed(popSeed, index, step)
//!         index += 1
//! ```
//!
//! Two things follow, and both are load-bearing. The counter is **reset at
//! the top of every step**, and the features of a step **continue** from
//! where its structures stopped — so a step's features are shifted by exactly
//! the number of structures registered at that step. `Collectors.groupingBy`
//! collects into `ArrayList`s in stream encounter order, so the group order
//! is the registry's own order and nothing else.
//!
//! And the registry's order is not a hash order. `ResourceManagerRegistryLoadTask`
//! sorts before it registers anything:
//!
//! ```text
//! map.entrySet().stream()
//!    .sorted(Map.Entry.comparingByKey())      // <- here
//!    .map(...)
//!    .forEach(this::register)                 // registerElements, in order
//! ```
//!
//! So registration order is a **total order on the resource identifier**, the
//! same on every JVM and every run. The earlier claim confused the
//! *datapack-listing* order, which really is unordered, with the order the
//! loader imposes on it before use.
//!
//! # The corroboration, which is not derived from any of the above
//!
//! An earlier agent measured one of these indices out of vanilla's own
//! blocks, without reading any registry: `worldgen-dump mineshaft-ceiling`
//! recovers the stream offset each corridor entered on, and the first
//! corridor of a chunk's pass must enter at offset 0 because nothing before
//! it draws. Only `index = 1` made that true (27 corridors pinned, 25 of them
//! at offset 0, 8 unexplainable — against ~120 unexplainable for every other
//! candidate).
//!
//! This module, deriving from the sort above, independently puts `mineshaft`
//! at index **1** of `underground_structures`. Two methods with nothing in
//! common agree, and [`tests::the_derived_mineshaft_index_matches_the_measured_one`]
//! keeps them agreeing.
//!
//! # Sorting, exactly
//!
//! `Identifier.compareTo` compares **path first and namespace second** —
//! that way round, which is not the intuitive one and is why it is ported
//! here rather than left to a tuple. With vanilla's single `minecraft`
//! namespace the namespace leg never runs, so a wrong assumption there would
//! be invisible today and would surface the first time a datapack adds a
//! structure.
//!
//! The key vanilla sorts is the **file** identifier
//! (`worldgen/structure/village_plains.json`), not the registry id. For
//! vanilla's data the two orders coincide, but only because structure names
//! are `[a-z0-9_]`: comparing `a.json` against `a_b.json` compares `.`
//! (0x2E) against `_` (0x5F), and every character that can follow a name
//! sorts above `.`. Rather than depend on that, [`step_indices`] takes the
//! sort key the caller actually read off disk and sorts on it.
//!
//! Rust's `str` ordering is by UTF-8 byte; Java's `String.compareTo` is by
//! UTF-16 code unit. They agree on everything in the ASCII range, and a
//! resource path is validated ASCII, so the two are the same order here.

use std::cmp::Ordering;
use std::collections::BTreeMap;

/// `GenerationStep$Decoration`, in ordinal order — read from 26.2 bytecode.
///
/// The **position in this array is the ordinal**, which is the `step` passed
/// to `setFeatureSeed`. It is eleven long, and a stale comment in
/// `structure/place.rs` put `vegetal_decoration` at 7 when it is at 9.
pub const DECORATION_STEPS: [&str; 11] = [
    "raw_generation",
    "lakes",
    "local_modifications",
    "underground_structures",
    "surface_structures",
    "strongholds",
    "underground_ores",
    "underground_decoration",
    "fluid_springs",
    "vegetal_decoration",
    "top_layer_modification",
];

/// The ordinal of a `GenerationStep.Decoration` serialized name.
#[must_use]
pub fn step_ordinal(name: &str) -> Option<usize> {
    DECORATION_STEPS.iter().position(|s| *s == name)
}

/// One structure as the loader sees it, before it has an index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructureEntry {
    /// The identifier vanilla sorts on — the **file** path, e.g.
    /// `worldgen/structure/village_plains.json`.
    pub sort_path: String,
    /// The namespace, sorted on only when two paths tie.
    pub namespace: String,
    /// The registry id this is keyed by in the result, e.g.
    /// `minecraft:village_plains`.
    pub id: String,
    /// The `step` field of the structure's JSON.
    pub step: String,
}

/// Where one structure sits in the decoration stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepIndex {
    /// `GenerationStep.Decoration.ordinal()` — the third argument to
    /// `setFeatureSeed`.
    pub step: usize,
    /// Position among the structures sharing that step — the second
    /// argument to `setFeatureSeed`.
    pub index: usize,
}

/// `Identifier.compareTo` — **path first, then namespace.**
#[must_use]
pub fn identifier_cmp(a: (&str, &str), b: (&str, &str)) -> Ordering {
    match a.0.cmp(b.0) {
        Ordering::Equal => a.1.cmp(b.1),
        other => other,
    }
}

/// The `setFeatureSeed` index of every structure, keyed by registry id.
///
/// Also returns, per step, how many structures that step registered — which
/// is the offset a step's *features* start counting from, since
/// `applyBiomeDecoration` shares one counter between the two loops.
///
/// # Errors
/// If any entry names a step that is not a `GenerationStep.Decoration`.
pub fn step_indices(
    entries: &[StructureEntry],
) -> Result<(BTreeMap<String, StepIndex>, [usize; 11]), String> {
    let mut sorted: Vec<&StructureEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| {
        identifier_cmp(
            (a.sort_path.as_str(), a.namespace.as_str()),
            (b.sort_path.as_str(), b.namespace.as_str()),
        )
    });

    let mut out = BTreeMap::new();
    let mut counts = [0usize; 11];
    for e in sorted {
        let step = step_ordinal(&e.step).ok_or_else(|| {
            format!(
                "{} names step `{}`, which is not a decoration step",
                e.id, e.step
            )
        })?;
        let index = counts[step];
        counts[step] += 1;
        out.insert(e.id.clone(), StepIndex { step, index });
    }
    Ok((out, counts))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, step: &str) -> StructureEntry {
        StructureEntry {
            sort_path: format!("worldgen/structure/{name}.json"),
            namespace: "minecraft".to_string(),
            id: format!("minecraft:{name}"),
            step: step.to_string(),
        }
    }

    /// The whole of vanilla 26.2's `worldgen/structure` directory, with the
    /// `step` each file declares. Read off the pinned extract on 2026-08-06;
    /// `golden_structure_step_index` re-reads the directory itself and fails
    /// if this list ever stops matching it, so this is a fixture and not a
    /// second source of truth.
    fn vanilla() -> Vec<StructureEntry> {
        [
            ("ancient_city", "underground_decoration"),
            ("bastion_remnant", "surface_structures"),
            ("buried_treasure", "underground_structures"),
            ("desert_pyramid", "surface_structures"),
            ("end_city", "surface_structures"),
            ("fortress", "underground_decoration"),
            ("igloo", "surface_structures"),
            ("jungle_pyramid", "surface_structures"),
            ("mansion", "surface_structures"),
            ("mineshaft", "underground_structures"),
            ("mineshaft_mesa", "underground_structures"),
            ("monument", "surface_structures"),
            ("nether_fossil", "underground_decoration"),
            ("ocean_ruin_cold", "surface_structures"),
            ("ocean_ruin_warm", "surface_structures"),
            ("pillager_outpost", "surface_structures"),
            ("ruined_portal", "surface_structures"),
            ("ruined_portal_desert", "surface_structures"),
            ("ruined_portal_jungle", "surface_structures"),
            ("ruined_portal_mountain", "surface_structures"),
            ("ruined_portal_nether", "surface_structures"),
            ("ruined_portal_ocean", "surface_structures"),
            ("ruined_portal_swamp", "surface_structures"),
            ("shipwreck", "surface_structures"),
            ("shipwreck_beached", "surface_structures"),
            ("stronghold", "surface_structures"),
            ("swamp_hut", "surface_structures"),
            ("trail_ruins", "underground_structures"),
            ("trial_chambers", "underground_structures"),
            ("village_desert", "surface_structures"),
            ("village_plains", "surface_structures"),
            ("village_savanna", "surface_structures"),
            ("village_snowy", "surface_structures"),
            ("village_taiga", "surface_structures"),
        ]
        .iter()
        .map(|(n, s)| entry(n, s))
        .collect()
    }

    /// **The load-bearing test in this file.**
    ///
    /// `MEASURED_MINESHAFT_INDEX` in `worldgen-assembly` was recovered from
    /// vanilla's blocks by reading the stream offset each mineshaft corridor
    /// entered on — no registry involved. This derivation reads the registry
    /// and never looks at a block. They must agree, and if a future data
    /// change moves the mineshaft this fails rather than silently reseeding
    /// every mineshaft in the world.
    #[test]
    fn the_derived_mineshaft_index_matches_the_measured_one() {
        let (idx, _) = step_indices(&vanilla()).expect("vanilla's steps are all real");
        let mineshaft = idx["minecraft:mineshaft"];
        assert_eq!(mineshaft.step, 3, "mineshaft is underground_structures");
        assert_eq!(
            mineshaft.index, 1,
            "measured out of vanilla's blocks as 1 — see MEASURED_MINESHAFT_INDEX"
        );
    }

    #[test]
    fn the_two_blocked_structures_now_have_indices() {
        let (idx, _) = step_indices(&vanilla()).expect("sorts");
        // The hay pile is a `feature_pool_element` inside the savanna
        // village's decor pool, so it draws from the village's stream.
        assert_eq!(
            idx["minecraft:village_savanna"],
            StepIndex { step: 4, index: 23 }
        );
        // `shipwreck_beached`'s sink depth is a `nextInt(3)` on this stream.
        assert_eq!(
            idx["minecraft:shipwreck_beached"],
            StepIndex { step: 4, index: 18 }
        );
    }

    #[test]
    fn a_step_counts_only_its_own_structures() {
        let (idx, counts) = step_indices(&vanilla()).expect("sorts");
        assert_eq!(counts[3], 5, "underground_structures");
        assert_eq!(counts[4], 26, "surface_structures");
        assert_eq!(counts[7], 3, "underground_decoration");
        // Every other step registers no structure at all, so its features
        // start at index 0.
        for (step, n) in counts.iter().enumerate() {
            if ![3, 4, 7].contains(&step) {
                assert_eq!(*n, 0, "step {step} should register no structures");
            }
        }
        // The first structure of each populated step is index 0.
        assert_eq!(idx["minecraft:buried_treasure"].index, 0);
        assert_eq!(idx["minecraft:bastion_remnant"].index, 0);
        assert_eq!(idx["minecraft:ancient_city"].index, 0);
    }

    /// The counter is shared with the feature loop, so a step's features are
    /// offset by its structure count. This is the number that shifts every
    /// tree in the world if it is wrong.
    #[test]
    fn features_start_after_that_steps_structures() {
        let (_, counts) = step_indices(&vanilla()).expect("sorts");
        assert_eq!(counts[4], 26, "surface_structures features start at 26");
        assert_eq!(counts[9], 0, "vegetal_decoration features start at 0");
    }

    #[test]
    fn identifier_compares_path_before_namespace() {
        // Path first: a `minecraft` path that sorts later loses to an
        // `aaa` path that sorts earlier.
        assert_eq!(
            identifier_cmp(("b", "aaa"), ("a", "zzz")),
            Ordering::Greater
        );
        // Namespace only breaks a tie.
        assert_eq!(identifier_cmp(("a", "aaa"), ("a", "zzz")), Ordering::Less);
        assert_eq!(identifier_cmp(("a", "m"), ("a", "m")), Ordering::Equal);
    }

    /// The `.json` suffix cannot reorder a name against a name it prefixes,
    /// which is what lets a caller sort on either key. Asserted rather than
    /// argued, because `shipwreck`/`shipwreck_beached` is a real pair in
    /// vanilla's data and it is one of the two structures this work unblocks.
    #[test]
    fn a_name_sorts_before_the_name_it_prefixes_either_way() {
        assert!("shipwreck.json" < "shipwreck_beached.json");
        assert!("shipwreck" < "shipwreck_beached");
        assert!("ruined_portal.json" < "ruined_portal_desert.json");
        assert!("mineshaft.json" < "mineshaft_mesa.json");
    }

    #[test]
    fn an_unknown_step_is_refused_rather_than_defaulted() {
        let bad = vec![entry("thing", "surface_decoration")];
        assert!(
            step_indices(&bad).is_err(),
            "a typo'd step must not become 0"
        );
    }

    #[test]
    fn the_step_table_is_the_bytecode_order() {
        assert_eq!(step_ordinal("underground_structures"), Some(3));
        assert_eq!(step_ordinal("surface_structures"), Some(4));
        // The one a stale comment in `structure/place.rs` had at 7.
        assert_eq!(step_ordinal("vegetal_decoration"), Some(9));
        assert_eq!(DECORATION_STEPS.len(), 11);
        assert_eq!(step_ordinal("nonsense"), None);
    }
}
