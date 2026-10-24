//! Loading the vanilla extract, once.
//!
//! `rustmc-worldgen` is data-free on purpose: it defines the traits and the
//! caller answers them, which is what keeps a generator testable without a
//! datapack on disk. Somebody still has to read the datapack, and doing it in
//! two places — a gate and a tool — is exactly the shape of the oracle faults
//! this project keeps finding (ORACLE.md §5b): two paths that agree until one
//! of them is updated.
//!
//! So the loaders live here, and both the gates and `worldgen-dump` use them.
//! The material is Mojang's own datagen output plus the golden vectors this
//! repo generates from the jar; none of it is committed, so every entry point
//! has to cope with it being absent.
//!
//! # An example, and why this crate has one
//!
//! This crate's doctest target ran **zero** examples on every gate this
//! project has ever run (`ci/checks/test-targets.toml`, 2026-08-16). What it
//! shows is the property the whole crate turns on — a path that does not
//! depend on the caller's working directory:
//!
//! ```
//! use worldgen_assembly::data_path;
//!
//! let p = data_path("worldgen/noise_settings/overworld.json");
//! // Absolute, because every `cargo test` binary in this workspace runs with
//! // its OWN package directory as the cwd. A loader resolving relative to
//! // the process's cwd fails on every test run regardless of what is on
//! // disk — that is the fault
//! // `crates/rustmc-server/tests/loader_paths_reachable.rs` exists to
//! // witness, and this crate is the one place the extract is reached from.
//! assert!(p.is_absolute(), "{} depends on the caller's cwd", p.display());
//! assert!(p.ends_with("worldgen/noise_settings/overworld.json"));
//! ```

use std::collections::BTreeMap;

use rustmc_worldgen::biome::climate::{Parameter, ParameterPoint};
use rustmc_worldgen::biome::temperature::BiomeClimate;
use rustmc_worldgen::carver::config::ConfiguredCarver;
use rustmc_worldgen::density::router::OverworldNoises;
use rustmc_worldgen::noise::NoiseParameters;
use serde_json::Value;

pub mod facts;
pub mod generate;
pub mod jigsaw_library;
pub mod occupancy;
pub mod parallel;

pub const SURFACE_GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../crates/rustmc-worldgen/golden/surface_chunk_vectors.json"
);

/// `RUSTMC_SURFACE_GOLDEN=<path>` points the surface, carver and assembly
/// gates at a different vector.
///
/// The decoration gate has had this for a while and it is what makes a
/// whole-world disagreement checkable: dump an oracle for the one chunk the
/// world diff blames and replay it. The surface gate had no such knob, and
/// its seed and chunk tables are fixed — so the ocean-floor sand/gravel band
/// that costs 51 blocks at seed 20260722 could not be gated at all without
/// editing the committed vector. `SurfaceChunkGolden` now takes
/// `-Dsurface.seeds` and `-Dsurface.chunks` to produce one.
#[must_use]
pub fn surface_golden() -> String {
    std::env::var("RUSTMC_SURFACE_GOLDEN").unwrap_or_else(|_| SURFACE_GOLDEN.to_string())
}
pub const ROUTER_GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../crates/rustmc-worldgen/golden/router_vectors.json"
);
pub const CLIMATE_GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../crates/rustmc-worldgen/golden/climate_vectors.json"
);
/// Mojang's own datagen output — the same source the surface rules, biome
/// climates, carver configs and block tags are all read from.
pub const DATA: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../oracle-work/vanilla-26.2/generated-data/data/minecraft"
);
/// `overworld.json`'s `sea_level`. The surface context and the `temperature`
/// condition must agree on it — vanilla passes one value to both.
pub const SEA_LEVEL: i32 = 63;

#[must_use]
pub fn data_path(relative: &str) -> std::path::PathBuf {
    std::path::Path::new(DATA).join(relative)
}

/// Whether the datagen extract is present.
///
/// **Twenty gates in two crates ask this before deciding whether to compare
/// anything**, and until now the answer `false` meant `eprintln!("skipping:
/// ...")` and an early return at every one of them. An early return is a
/// *pass*, and cargo does not print a passing test's stderr, so a tree with no
/// extract produced a log identical to one that checked every vector — which
/// is why `ci/t1-gate.sh`'s `grep -c "skipping: no vanilla"` has been 0 in all
/// 143 recorded runs.
///
/// It now goes through `rustmc_data::fixture`, which panics in the default
/// `required` mode and returns `false` only under `RUSTMC_FIXTURES=optional`,
/// where `ci/t1.sh` already prints a different sentinel. The material really
/// is gitignored and jar-derived, so a clean clone still has a way through;
/// what it no longer has is a *silent* one.
#[must_use]
pub fn have_extract() -> bool {
    rustmc_data::fixture::available(
        &data_path("worldgen/noise_settings/overworld.json"),
        "datagen extract",
        "ci/vanilla-server.sh data",
    )
}

#[must_use]
pub fn read(path: &str) -> Value {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path} unreadable: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path} is not JSON: {e}"))
}

/// Every structure's `setFeatureSeed` index, read off the datapack the way
/// `ResourceManagerRegistryLoadTask` reads it.
///
/// The ordering is vanilla's, not ours: the loader sorts its resource map by
/// identifier before registering anything, and `applyBiomeDecoration` then
/// groups the registry by step and counts within each group. See
/// `rustmc_worldgen::structure::step_index` for the bytecode this follows and
/// for the mineshaft measurement that corroborates it.
///
/// Returns the per-id indices and the per-step structure counts — the latter
/// being the offset each step's *features* start counting from, since
/// vanilla shares one counter between the structure and feature loops.
///
/// # Panics
/// If the extract is missing or a structure declares a step that is not a
/// `GenerationStep.Decoration`. Both are "the data is not what this port was
/// built against", which is not a thing to paper over with a default.
#[must_use]
pub fn structure_step_indices() -> (
    BTreeMap<String, rustmc_worldgen::structure::step_index::StepIndex>,
    [usize; 11],
) {
    use rustmc_worldgen::structure::step_index::{step_indices, StructureEntry};
    let dir = data_path("worldgen/structure");
    let mut entries = Vec::new();
    let listing =
        std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{} unreadable: {e}", dir.display()));
    for item in listing {
        let path = item.expect("dir entry").path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let doc = read(path.to_str().expect("utf-8 path"));
        let step = doc["step"]
            .as_str()
            .unwrap_or_else(|| panic!("{stem}.json has no `step`"))
            .to_string();
        entries.push(StructureEntry {
            // Vanilla sorts the **file** identifier, `.json` and all.
            sort_path: format!("worldgen/structure/{stem}.json"),
            namespace: "minecraft".to_string(),
            id: format!("minecraft:{stem}"),
            step,
        });
    }
    assert!(
        !entries.is_empty(),
        "{} holds no structures — the extract is not what this port expects",
        dir.display()
    );
    step_indices(&entries).unwrap_or_else(|e| panic!("{e}"))
}

/// Vectors carry 64-bit values as strings, since JSON numbers are doubles and
/// would round the ones near the top of the range.
#[must_use]
pub fn long(v: &Value) -> i64 {
    v.as_str().expect("long-as-string").parse().expect("i64")
}

#[must_use]
pub fn noise_parameters() -> BTreeMap<String, NoiseParameters> {
    read(ROUTER_GOLDEN)["parameters"]
        .as_array()
        .expect("parameters")
        .iter()
        .map(|entry| {
            let amplitudes: Vec<f64> = entry["amplitudes"]
                .as_array()
                .expect("amplitudes")
                .iter()
                .map(|v| f64::from_bits(long(v) as u64))
                .collect();
            (
                entry["name"].as_str().expect("name").to_string(),
                NoiseParameters::new(
                    i32::try_from(entry["firstOctave"].as_i64().expect("firstOctave"))
                        .expect("i32"),
                    amplitudes,
                ),
            )
        })
        .collect()
}

/// The nether's two climate noise parameters, from Mojang's own datagen.
///
/// Read from `worldgen/noise/nether/{temperature,vegetation}.json` rather
/// than from `router_vectors.json`: the golden carries the noises the
/// **overworld** router names, and these two are not among them. A missing
/// entry is an error here rather than a default, because
/// `OverworldNoises::get` panics and a zeroed climate noise puts the whole
/// nether in one biome.
///
/// # Errors
/// If the datagen extract is absent or an entry is malformed.
pub fn nether_noises() -> Result<OverworldNoises, String> {
    let mut set = OverworldNoises::new();
    for stem in ["temperature", "vegetation"] {
        let path = data_path(&format!("worldgen/noise/nether/{stem}.json"));
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let doc: Value = serde_json::from_str(&text).map_err(|e| format!("{e}"))?;
        let first_octave = i32::try_from(
            doc["firstOctave"]
                .as_i64()
                .ok_or_else(|| format!("{} has no firstOctave", path.display()))?,
        )
        .map_err(|e| format!("{e}"))?;
        let amplitudes: Vec<f64> = doc["amplitudes"]
            .as_array()
            .ok_or_else(|| format!("{} has no amplitudes", path.display()))?
            .iter()
            .map(|v| v.as_f64().ok_or("an amplitude is not a number"))
            .collect::<Result<_, _>>()?;
        set = set.with(
            &format!("minecraft:nether/{stem}"),
            NoiseParameters::new(first_octave, amplitudes),
        );
    }
    Ok(set)
}

/// The nether's climate table, in `MultiNoiseBiomeSourceParameterList`
/// order.
///
/// **Order is load-bearing** for the same reason the overworld's is:
/// `ParameterList::find_value` walks an RTree whose shape depends on
/// insertion order and resolves exact ties by it. The nether's five entries
/// are single points rather than ranges, so ties are not hypothetical — every
/// query is a distance comparison against five points.
///
/// Read from `data/vanilla/biome_parameters_nether.json`, which carries the
/// thirteen quantised coordinates per entry in the same layout the overworld
/// climate golden uses.
///
/// # Errors
/// If the file is absent or an entry is not thirteen numbers and a name.
pub fn nether_biome_table() -> Result<Vec<(ParameterPoint, String)>, String> {
    let path = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/vanilla/biome_parameters_nether.json"
    ));
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let doc: Value = serde_json::from_str(&text).map_err(|e| format!("{e}"))?;
    doc.as_array()
        .ok_or("the nether parameter list is not an array")?
        .iter()
        .map(|entry| {
            let row = entry.as_array().ok_or("an entry is not an array")?;
            if row.len() != 14 {
                return Err(format!("an entry has {} fields, want 14", row.len()));
            }
            let axis = |i: usize| -> Result<Parameter, String> {
                let lo = row[i * 2].as_i64().ok_or("a coordinate is not a number")?;
                let hi = row[i * 2 + 1]
                    .as_i64()
                    .ok_or("a coordinate is not a number")?;
                Ok(Parameter::new(lo, hi))
            };
            Ok((
                ParameterPoint {
                    temperature: axis(0)?,
                    humidity: axis(1)?,
                    continentalness: axis(2)?,
                    erosion: axis(3)?,
                    depth: axis(4)?,
                    weirdness: axis(5)?,
                    offset: row[12].as_i64().ok_or("no offset")?,
                },
                row[13]
                    .as_str()
                    .ok_or("an entry has no biome name")?
                    .to_string(),
            ))
        })
        .collect()
}

/// The nether's surface rule, from `worldgen/noise_settings/nether.json`.
///
/// # Errors
/// If the extract is absent or the rule names something unimplemented.
pub fn nether_surface_rule() -> Result<rustmc_worldgen::surface::rule::Rule, String> {
    let path = data_path("worldgen/noise_settings/nether.json");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let doc: Value = serde_json::from_str(&text).map_err(|e| format!("{e}"))?;
    rustmc_worldgen::surface::rule::parse_noise_settings_surface_rule(&doc)
        .map_err(|e| format!("{e:?}"))
}

/// The End's surface rule, from `worldgen/noise_settings/end.json`.
///
/// One unconditional `block(end_stone)` — no sequence, no conditions, no
/// named noise. Loaded from the datagen JSON rather than written inline so
/// that a version bump which gives the End a real rule is a data change, not
/// a code change nobody notices.
///
/// # Errors
/// If the extract is absent or the rule names something unimplemented.
pub fn end_surface_rule() -> Result<rustmc_worldgen::surface::rule::Rule, String> {
    let path = data_path("worldgen/noise_settings/end.json");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let doc: Value = serde_json::from_str(&text).map_err(|e| format!("{e}"))?;
    rustmc_worldgen::surface::rule::parse_noise_settings_surface_rule(&doc)
        .map_err(|e| format!("{e:?}"))
}

#[must_use]
pub fn overworld_noises(params: &BTreeMap<String, NoiseParameters>) -> OverworldNoises {
    params
        .iter()
        .fold(OverworldNoises::new(), |set, (name, p)| {
            set.with(name, p.clone())
        })
}

/// The overworld climate table, in the order vanilla builds it.
///
/// Order is load-bearing: `ParameterList::find_value` walks an RTree whose
/// shape depends on insertion order, and ties are broken by it.
#[must_use]
pub fn biome_table() -> Vec<(ParameterPoint, String)> {
    let doc = read(CLIMATE_GOLDEN);
    let case = doc["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .find(|c| c["case"] == "overworld_climate")
        .expect("overworld_climate case");
    case["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .map(|entry| {
            let s = entry["space"].as_array().expect("space");
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

/// What a biome contributes to worldgen, keyed by namespaced id.
pub struct BiomeEntry {
    pub climate: BiomeClimate,
    /// **Ordered.** A carver's index in this list is added to the world seed,
    /// so reading it through anything unordered moves every ravine in the
    /// biome.
    pub carvers: Vec<String>,
}

#[must_use]
pub fn biome_data() -> BTreeMap<String, BiomeEntry> {
    let dir = data_path("worldgen/biome");
    let mut out = BTreeMap::new();
    for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| {
        panic!(
            "{} unreadable ({e}) — run ci/vanilla-server.sh data",
            dir.display()
        )
    }) {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).expect("utf-8");
        let json: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("readable")).expect("json");
        let climate =
            BiomeClimate::from_datapack_json(&json).unwrap_or_else(|e| panic!("{stem}: {e}"));
        // `HolderSet`'s codec accepts **either** a list or a bare element,
        // and the datapack uses both: every overworld biome writes
        // `["minecraft:cave", ...]` and every nether biome writes the bare
        // string `"minecraft:nether_cave"`. Reading only the list form gave
        // the nether an empty carver list, so `apply_carvers` swept 289
        // source chunks per chunk, was offered nothing, and carved zero
        // blocks in 0.05 s — which looks exactly like a carver that runs and
        // finds no caves.
        let carvers = match &json["carvers"] {
            Value::Array(a) => a
                .iter()
                .map(|v| v.as_str().expect("carver id").to_string())
                .collect(),
            Value::String(one) => vec![one.clone()],
            _ => Vec::new(),
        };
        out.insert(format!("minecraft:{stem}"), BiomeEntry { climate, carvers });
    }
    out
}

#[must_use]
pub fn configured_carvers() -> BTreeMap<String, ConfiguredCarver> {
    let dir = data_path("worldgen/configured_carver");
    let mut out = BTreeMap::new();
    for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).expect("utf-8");
        let json: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("readable")).expect("json");
        out.insert(
            format!("minecraft:{stem}"),
            ConfiguredCarver::parse(&json).unwrap_or_else(|e| panic!("{stem}: {e}")),
        );
    }
    out
}

// ---------------------------------------------------------------------------
// The rest of the extract, which the per-stage gates each loaded for
// themselves because none of them needed all of it at once. The pipeline
// does.
// ---------------------------------------------------------------------------

/// The overworld surface rule, parsed from `noise_settings/overworld.json`.
///
/// # Errors
/// If the extract is absent or the rule names something unimplemented.
pub fn surface_rule() -> Result<rustmc_worldgen::surface::rule::Rule, String> {
    let path = data_path("worldgen/noise_settings/overworld.json");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let doc: Value = serde_json::from_str(&text).map_err(|e| format!("{e}"))?;
    rustmc_worldgen::surface::rule::parse_noise_settings_surface_rule(&doc)
        .map_err(|e| format!("{e:?}"))
}

/// Every block tag, flattened.
///
/// # Errors
/// If the tag directory is absent or a tag cycles.
pub fn block_tags() -> Result<rustmc_worldgen::tags::BlockTags, String> {
    rustmc_worldgen::tags::BlockTags::load(&data_path("tags/block"))
}

/// The per-state block facts, from `data/vanilla/block_props.json`.
///
/// # Errors
/// If the extract is absent or written in a different direction order.
pub fn block_props() -> Result<rustmc_behavior::block_props::BlockPropsTable, String> {
    let path = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/vanilla/block_props.json"
    ));
    rustmc_behavior::block_props::BlockPropsTable::load(path)
}

/// Whether the block-props extract is present.
#[must_use]
pub fn have_block_props() -> bool {
    std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/vanilla/block_props.json"
    ))
    .exists()
}

/// Every biome's feature list, as eleven steps of placed-feature names.
///
/// Order within a step is load-bearing — it is what the global feature
/// ordering is built from — so this preserves the datapack's.
///
/// # Errors
/// If the biome directory is absent or a file is malformed.
pub fn biome_features() -> Result<BTreeMap<String, Vec<Vec<String>>>, String> {
    let dir = data_path("worldgen/biome");
    let mut out = BTreeMap::new();
    for entry in std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let path = entry.map_err(|e| format!("{e}"))?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or("a biome file has a non-utf8 name")?;
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{e}"))?;
        let doc: Value = serde_json::from_str(&text).map_err(|e| format!("{e}"))?;
        let steps = doc["features"]
            .as_array()
            .map(|steps| {
                steps
                    .iter()
                    .map(|step| {
                        step.as_array()
                            .map(|list| {
                                list.iter()
                                    .filter_map(|f| f.as_str().map(ToString::to_string))
                                    .collect()
                            })
                            .unwrap_or_default()
                    })
                    .collect()
            })
            .unwrap_or_default();
        out.insert(format!("minecraft:{stem}"), steps);
    }
    Ok(out)
}

/// Every placed feature in the datapack, by name.
///
/// A feature whose placement chain names an unimplemented modifier still
/// parses — the modifier becomes `NeedsWorld` and the chain refuses at
/// evaluation — so this returning a name is not a claim that it can run.
///
/// # Errors
/// If the directory is absent or a file is malformed.
pub fn placed_features(
) -> Result<BTreeMap<String, rustmc_worldgen::feature::decorate::PlacedFeature>, String> {
    let dir = data_path("worldgen/placed_feature");
    let mut out = BTreeMap::new();
    for entry in std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let path = entry.map_err(|e| format!("{e}"))?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or("a placed feature has a non-utf8 name")?;
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{e}"))?;
        let doc: Value = serde_json::from_str(&text).map_err(|e| format!("{e}"))?;
        if let Ok(feature) = rustmc_worldgen::feature::decorate::PlacedFeature::parse(&doc) {
            out.insert(format!("minecraft:{stem}"), feature);
        }
    }
    Ok(out)
}

/// The global feature ordering's input: every biome the **biome source** can
/// produce, with its eleven steps, in the source's own order.
///
/// Read from `feature_order_vectors.json` rather than rebuilt from the biome
/// directory, and the difference is oracle fault 5. A port that feeds the
/// biome *registry* here gets 66 biomes instead of 55 and 209 features
/// instead of 168, and every feature index after the first divergence moves —
/// which moves the seed of every feature after it. `possibleBiomes()` is the
/// list, and the golden carries it because deriving it needs the multi-noise
/// parameter list rather than a directory listing.
///
/// # Errors
/// If the golden is absent or malformed.
pub fn feature_order_sources() -> Result<Vec<Vec<Vec<String>>>, String> {
    feature_order_sources_from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../crates/rustmc-worldgen/golden/feature_order_vectors.json"
    ))
}

/// The same, for the **nether** biome source.
///
/// A separate file rather than a second array in the same one, because they
/// are two orderings over two disjoint biome sets: the nether's five biomes
/// are not among the overworld's 55, and a feature numbered against the wrong
/// list gets its seed from a list it is not in. That is oracle fault 5 one
/// dimension over.
///
/// Produced by `DIM=nether ci/feature-order-golden.sh`.
///
/// # Errors
/// If the golden is absent or malformed.
pub fn feature_order_sources_nether() -> Result<Vec<Vec<Vec<String>>>, String> {
    feature_order_sources_from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../crates/rustmc-worldgen/golden/feature_order_vectors_nether.json"
    ))
}

/// The End's feature-order sources, in `TheEndBiomeSource.collectPossibleBiomes`
/// order.
///
/// # Why this is built from the biome JSONs rather than from a golden vector
///
/// The other two dimensions read a `feature_order_vectors_*.json` emitted by
/// goldengen. This does not, and the reason is that the *only* thing the golden
/// adds over the datagen output is the biome **order** — and here that order is
/// five names in a fixed sequence read straight out of `collectPossibleBiomes`
/// (bytecode, offsets 0-42). `BiomeSource.possibleBiomes()` collects that
/// stream into an `ImmutableSet`, which preserves insertion order, and
/// `FeatureSorter.buildFeaturesPerStep` is handed it directly — so
/// `biome::end::BIOMES` *is* the golden, and it is checkable by eye against one
/// method.
///
/// The order decides each feature's **index within its step**, which is what
/// `WorldgenRandom.setFeatureSeed` mixes into the seed. Reorder those five
/// names and every End feature draws from a different stream.
///
/// # Errors
/// If a biome file is absent or malformed.
pub fn feature_order_sources_end() -> Result<Vec<Vec<Vec<String>>>, String> {
    let by_biome = biome_features()?;
    rustmc_worldgen::biome::end::BIOMES
        .iter()
        .map(|name| {
            by_biome
                .get(*name)
                .cloned()
                .ok_or_else(|| format!("no biome data for {name}"))
        })
        .collect()
}

/// The End's per-step global feature orderings.
///
/// # Errors
/// If a biome file is absent, or the registry contains a cycle.
pub fn features_per_step_end() -> Result<Vec<rustmc_worldgen::feature::sorter::StepFeatures>, String>
{
    let sources = feature_order_sources_end()?;
    rustmc_worldgen::feature::sorter::build_features_per_step(&sources)
}

fn feature_order_sources_from(path: &str) -> Result<Vec<Vec<Vec<String>>>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let doc: Value = serde_json::from_str(&text).map_err(|e| format!("{e}"))?;
    Ok(doc["sources"]
        .as_array()
        .ok_or("no `sources`")?
        .iter()
        .map(|biome| {
            biome["steps"]
                .as_array()
                .map(|steps| {
                    steps
                        .iter()
                        .map(|step| {
                            step.as_array()
                                .map(|list| {
                                    list.iter()
                                        .filter_map(|f| f.as_str().map(ToString::to_string))
                                        .collect()
                                })
                                .unwrap_or_default()
                        })
                        .collect()
                })
                .unwrap_or_default()
        })
        .collect())
}

/// The eleven steps' global feature orderings.
///
/// # Errors
/// If the golden is absent, or the real registry contains a cycle — which it
/// cannot, and the error exists so a datapack that does says so.
pub fn features_per_step() -> Result<Vec<rustmc_worldgen::feature::sorter::StepFeatures>, String> {
    let sources = feature_order_sources()?;
    rustmc_worldgen::feature::sorter::build_features_per_step(&sources)
}

/// The nether's per-step global feature orderings.
///
/// # Errors
/// If the golden is absent, or the registry contains a cycle.
pub fn features_per_step_nether(
) -> Result<Vec<rustmc_worldgen::feature::sorter::StepFeatures>, String> {
    let sources = feature_order_sources_nether()?;
    rustmc_worldgen::feature::sorter::build_features_per_step(&sources)
}

/// Every configured feature, parsed against the block-state extract.
///
/// A feature whose type is unported becomes `Unimplemented` rather than an
/// error, so this returning a name is not a claim it can be placed —
/// `is_supported` is.
///
/// # Errors
/// If the directory or the block-state extract is absent.
pub fn configured_features(
) -> Result<BTreeMap<String, rustmc_worldgen::feature::configured::ConfiguredFeature>, String> {
    let states = rustmc_worldgen::feature::state_spec::extract_or_skip()
        .ok_or("no block-props extract, so no default states")?;
    let dir = data_path("worldgen/configured_feature");
    let placed_dir = data_path("worldgen/placed_feature");
    let read = |dir: &std::path::Path, name: &str| -> Option<Value> {
        let stem = name.strip_prefix("minecraft:")?;
        let text = std::fs::read_to_string(dir.join(format!("{stem}.json"))).ok()?;
        serde_json::from_str(&text).ok()
    };
    let resolve_placed = |name: &str| read(&placed_dir, name);
    let resolve_configured = |name: &str| read(&dir, name);

    let mut out = BTreeMap::new();
    for entry in std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let path = entry.map_err(|e| format!("{e}"))?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or("a configured feature has a non-utf8 name")?;
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{e}"))?;
        let doc: Value = serde_json::from_str(&text).map_err(|e| format!("{e}"))?;
        if let Ok(feature) = rustmc_worldgen::feature::configured::ConfiguredFeature::parse(
            &doc,
            &resolve_placed,
            &resolve_configured,
            &states,
        ) {
            out.insert(format!("minecraft:{stem}"), feature);
        }
    }
    Ok(out)
}

/// The four heightmaps vanilla saves, taken from the maps the canvas
/// **maintained** rather than recomputed from the finished blocks.
///
/// That distinction is the whole value of writing them. A recomputation is
/// correct by construction and tests nothing: it would agree with vanilla
/// even if every `Heightmap#update` in this port were wrong, because it never
/// consults them. Reading the live maps instead makes vanilla's own saved
/// heightmaps a gate on our maintenance of them — which matters because
/// features anchor to these maps *during* decoration, long before anything
/// could recompute.
#[must_use]
pub fn heightmaps_from_canvas(
    canvas: &rustmc_worldgen::canvas::Canvas,
    chunk_x: i32,
    chunk_z: i32,
) -> std::collections::BTreeMap<String, Vec<i32>> {
    use rustmc_worldgen::canvas::LiveHeightmap;
    const MAPS: [(&str, LiveHeightmap); 4] = [
        ("WORLD_SURFACE", LiveHeightmap::WorldSurface),
        ("OCEAN_FLOOR", LiveHeightmap::OceanFloor),
        ("MOTION_BLOCKING", LiveHeightmap::MotionBlocking),
        (
            "MOTION_BLOCKING_NO_LEAVES",
            LiveHeightmap::MotionBlockingNoLeaves,
        ),
    ];
    let mut out = std::collections::BTreeMap::new();
    for (name, map) in MAPS {
        let mut values = Vec::with_capacity(256);
        for local_z in 0..16 {
            for local_x in 0..16 {
                values.push(canvas.height_of(
                    map,
                    (chunk_x << 4) + local_x,
                    (chunk_z << 4) + local_z,
                ));
            }
        }
        out.insert(name.to_string(), values);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_extract_loads_or_says_why() {
        if !have_extract() {
            return;
        }
        let rule = surface_rule().expect("the overworld surface rule parses");
        assert!(
            !matches!(rule, rustmc_worldgen::surface::rule::Rule::Bandlands),
            "the top-level rule is a sequence, not a leaf"
        );

        let tags = block_tags().expect("block tags load");
        assert!(tags.len() > 100, "only {} tags", tags.len());

        let features = biome_features().expect("biome features load");
        assert!(features.len() > 50, "only {} biomes", features.len());
        // A biome's list can be **shorter** than eleven: the datapack trims
        // trailing empty steps, so `basalt_deltas` has eight. Vanilla guards
        // with `step < features.size()` and `attempt_order` does the same
        // through `steps.get(step)`. Padding them to eleven here would work
        // and would hide the fact from the next person to read this.
        let mut shortest = 11;
        for (biome, steps) in &features {
            assert!(steps.len() <= 11, "{biome} has {} steps", steps.len());
            shortest = shortest.min(steps.len());
        }
        assert!(
            shortest < 11,
            "no biome has a trimmed step list, which the guard exists for"
        );

        let placed = placed_features().expect("placed features load");
        assert!(placed.len() > 200, "only {} placed features", placed.len());
    }

    /// The nether has its own ordering over its own five biomes, and our
    /// sorter reproduces vanilla's `buildFeaturesPerStep` on it.
    ///
    /// The numbers are vanilla's, taken from
    /// `DIM=nether ci/feature-order-golden.sh`: five biomes, ten steps (the
    /// datapack trims the trailing empty ones), 37 features. Spelled out
    /// rather than derived, so a change to the sorter or to the golden fails
    /// here rather than moving every nether feature's seed quietly.
    #[test]
    fn the_nether_has_its_own_feature_ordering() {
        let sources = feature_order_sources_nether()
            .unwrap_or_else(|e| panic!("the nether feature-order golden did not load: {e}"));
        assert_eq!(sources.len(), 5, "the nether biome source has five biomes");
        let steps = features_per_step_nether().expect("the nether registry does not cycle");
        let total: usize = steps.iter().map(|s| s.features.len()).sum();
        assert_eq!(total, 37, "37 features across the nether's steps");
        assert!(
            steps.len() >= 10,
            "the nether's biomes reach step 10, {} found",
            steps.len()
        );
    }

    /// The ordering's input is `possibleBiomes()`, not the registry — oracle
    /// fault 5, which moved 105 of 106 vegetal positions.
    #[test]
    fn the_feature_order_comes_from_the_possible_biomes() {
        // Both goldens are committed files, so an `Err` is a golden this code
        // cannot read rather than an absent extract. A pass for that is how a
        // committed vector quietly stops being a vector.
        let sources = feature_order_sources()
            .unwrap_or_else(|e| panic!("the feature-order golden did not load: {e}"));
        assert_eq!(
            sources.len(),
            55,
            "the biome *source* has 55 possible biomes; the registry has 66"
        );
        let steps = features_per_step().expect("the real registry does not cycle");
        assert_eq!(steps.len(), 11);
        let total: usize = steps.iter().map(|s| s.features.len()).sum();
        assert_eq!(total, 168, "168 features across the eleven steps, not 209");
    }
}
