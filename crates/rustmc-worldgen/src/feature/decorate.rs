//! The decoration driver — which feature runs, where, and with which seed.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `ChunkGenerator#applyBiomeDecoration`, `PlacedFeature`.
//!
//! [`ore`](super::ore) proves a feature given an origin. This is the code
//! that *chooses* the origin, and it is where the expensive mistakes live: an
//! arithmetic bug moves one feature, a wiring bug moves every feature after
//! it in the chunk and every chunk in the world.
//!
//! # The loop, and the three things in it that are easy to get wrong
//!
//! ```text
//! origin  = chunk's MINIMUM corner at the world's MINIMUM section
//! popSeed = random.setDecorationSeed(worldSeed, origin.x, origin.z)
//! biomes  = every biome present in the 3x3 of chunks around this one,
//!           retained against the biome source's possible set
//! for step in 0..steps:
//!     indices = { globalIndexOf(f) for b in biomes for f in b.features[step] }
//!     for i in SORTED(indices):
//!         random.setFeatureSeed(popSeed, i, step)
//!         features[step][i].place(...)
//! ```
//!
//! 1. **The index is global.** It is the feature's position in one ordering
//!    computed from the entire biome registry ([`sorter`](super::sorter)),
//!    not its position in the biome's own list. Two biomes sharing a feature
//!    seed it identically.
//! 2. **The indices are sorted.** So features run in that global order, not
//!    in the order the biomes happened to be walked. Without the sort the
//!    world still generates, and every feature after the first collision is
//!    somewhere else.
//! 3. **The origin is the minimum corner at the minimum section.** Not the
//!    chunk centre, not y=0. It feeds `setDecorationSeed`, so getting it
//!    wrong reseeds every decoration in the chunk.
//!
//! # A biome is present if it is in the chunk, not in the biome source
//!
//! Vanilla collects the biome set by walking each section's biome *palette*
//! across the 3x3 — the biomes actually written into those chunks — and only
//! then intersects with what the source can produce. A driver that asked the
//! biome source directly would get a different, larger set: the palette is
//! quantised to quarts and stores what was resolved, and the retain step
//! exists precisely because the two disagree.

use std::collections::{BTreeMap, BTreeSet};

use crate::rng::worldgen_random::WorldgenRandom;
use crate::rng::xoroshiro::XoroshiroRandomSource;
use crate::rng::RandomSource;

use super::placement::{PlacementModifier, PlacementWorld, Pos, Unsupported};
use super::sorter::StepFeatures;

// The trace switches, read once each. These sit in `decorate_chunk`'s
// per-feature loop, which runs hundreds of times a chunk; `getenv` and
// `std::env::var` together were 3.8% of a full pipeline with every switch
// off. See `crate::trace_switch`.
crate::trace_switch_value!(trace_attempt, "RUSTMC_TRACE_ATTEMPT");
crate::trace_switch_value!(trace_draws, "RUSTMC_TRACE_DRAWS");
crate::trace_switch!(trace_patch, "RUSTMC_TRACE_PATCH");
crate::trace_switch!(trace_attempt_pos, "RUSTMC_TRACE_ATTEMPT_POS");

/// One placed feature: what to place, and the chain that decides where.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedFeature {
    /// The configured feature's registry name, e.g. `minecraft:ore_iron`.
    pub feature: String,
    pub placement: Vec<PlacementModifier>,
}

impl PlacedFeature {
    /// Parse one `worldgen/placed_feature/*.json`.
    ///
    /// # Errors
    /// If `feature` is missing or a modifier fails to parse. An
    /// *unsupported* modifier is not a parse error — see
    /// [`PlacementModifier::parse`].
    pub fn parse(value: &serde_json::Value) -> Result<Self, String> {
        let feature = value
            .get("feature")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "placed feature has no `feature`".to_string())?
            .to_string();
        let placement = value
            .get("placement")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| format!("{feature} has no `placement`"))?
            .iter()
            .map(PlacementModifier::parse)
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Self { feature, placement })
    }

    /// Whether every modifier in the chain can be evaluated.
    #[must_use]
    pub fn is_supported(&self) -> bool {
        self.placement.iter().all(PlacementModifier::is_supported)
    }
}

/// The chunk being decorated, and the world it sits in.
#[derive(Clone, Copy, Debug)]
pub struct ChunkOrigin {
    pub chunk_x: i32,
    pub chunk_z: i32,
    /// The world's minimum section, i.e. `min_y >> 4`.
    pub min_section_y: i32,
}

impl ChunkOrigin {
    /// `SectionPos.of(chunkPos, level.getMinSectionY()).origin()` — the
    /// **minimum** corner, at the **minimum** section.
    #[must_use]
    pub fn block(self) -> Pos {
        Pos {
            x: self.chunk_x * 16,
            y: self.min_section_y * 16,
            z: self.chunk_z * 16,
        }
    }
}

/// One feature's turn: what ran, where it decided to place, and how the
/// stream stood afterwards.
#[derive(Clone, Debug, PartialEq)]
pub struct Attempt {
    /// The feature's index in the step's global ordering — the number handed
    /// to `setFeatureSeed`.
    pub index: usize,
    pub feature: String,
    /// Positions the placement chain produced, in chain order.
    pub positions: Vec<Pos>,
    /// `nextLong()` taken immediately after the feature ran, which pins the
    /// whole draw history that produced it.
    pub after: i64,
    /// The draw counter at the moment each position was emitted — vanilla's
    /// `positionDraws`, one entry per [`positions`](Self::positions).
    ///
    /// Zero-filled unless [`crate::rng::draws`] is armed, and worth carrying
    /// even so, because it is the only field that can see a **feature body**
    /// spending the wrong number of draws. The position list cannot: while
    /// the chain still yields the same first position, a miscounted body
    /// damages the *next* candidate, and a candidate that never survives
    /// leaves no entry to disagree with. That is precisely where the
    /// `moss_patch_ceiling` defect hid.
    pub position_draws: Vec<u64>,
}

/// Which features this step will attempt, in the order it will attempt them.
///
/// Split out from running them because the order is the part worth testing
/// on its own: it depends on the global sort, the biome set and the retain,
/// and none of those need a world to check.
///
/// `biomes_present` is the set collected from the chunk section palettes,
/// already intersected with the biome source's possible set — the caller
/// does that, because only the caller knows how its chunks store biomes.
#[must_use]
pub fn attempt_order(
    step: usize,
    step_features: &StepFeatures,
    biomes_present: &[String],
    biome_features: &BTreeMap<String, Vec<Vec<String>>>,
) -> Vec<usize> {
    let _ = step;
    // A `BTreeSet` rather than a sort afterwards: vanilla builds an
    // `IntArraySet` and calls `Arrays.sort` on it, and the two agree on
    // "unique, ascending". Using an ordered set makes the sort structural
    // instead of a line someone can delete.
    let mut indices = BTreeSet::new();
    for biome in biomes_present {
        let Some(steps) = biome_features.get(biome) else {
            continue;
        };
        let Some(features) = steps.get(step) else {
            continue;
        };
        for feature in features {
            if let Some(index) = step_features.index_of(feature) {
                indices.insert(index);
            }
        }
    }
    indices.into_iter().collect()
}

/// The chunk's decoration seed — `setDecorationSeed(worldSeed, origin.x,
/// origin.z)`, which every feature in the chunk is then seeded from.
#[must_use]
pub fn population_seed(origin: ChunkOrigin, world_seed: i64) -> i64 {
    let block = origin.block();
    // The initial seed is irrelevant — `setDecorationSeed` reseeds — but
    // vanilla constructs the source before reseeding it and the shape is kept
    // so the two read the same.
    let mut random = WorldgenRandom::new(XoroshiroRandomSource::from_seed(0));
    random.set_decoration_seed(world_seed, block.x, block.z)
}

/// Run one placed feature, the way `PlacedFeature#placeWithContext` runs it.
///
/// `place` is called once per **position, as that position is produced**, on
/// the live random. That ordering is load-bearing rather than incidental —
/// see [`for_each_position`](super::placement::for_each_position). A caller
/// that declines to place writes nothing and consumes none of the feature's
/// own draws, which is how a partially-ported feature set is measured rather
/// than faked.
///
/// # Errors
/// If the chain contains a modifier that cannot be evaluated.
#[allow(
    clippy::too_many_arguments,
    reason = "\
    every argument is a distinct part of vanilla's own seeding — the chunk, \
    the population seed, the step, the index, the name, the feature, the \
    sink. Bundling them into a struct would hide which of them feed \
    setFeatureSeed, which is the thing this function exists to get right."
)]
pub fn run_feature<W, F>(
    world: &mut W,
    random: &mut WorldgenRandom<XoroshiroRandomSource>,
    origin: ChunkOrigin,
    population: i64,
    step: usize,
    index: usize,
    name: &str,
    feature: &PlacedFeature,
    mut place: F,
) -> Result<Attempt, Unsupported>
where
    W: PlacementWorld + ?Sized,
    F: FnMut(&mut W, &PlacedFeature, Pos, &mut WorldgenRandom<XoroshiroRandomSource>),
{
    let block = origin.block();
    // `PlacementContext.topFeature()` — what `BiomeFilter` asks about.
    world.set_running_feature(name);
    random.set_feature_seed(
        population,
        i32::try_from(index).expect("feature index fits in i32"),
        i32::try_from(step).expect("step fits in i32"),
    );

    // `RUSTMC_TRACE_ATTEMPT=<substring>` reports, per invocation, how many
    // attempts survived the placement chain and — for those that did not —
    // which modifier index turned them away. A feature that places nothing
    // is invisible to a write-level trace, and the two reasons it can place
    // nothing (the chain rejecting every attempt, the feature declining) are
    // indistinguishable without this.
    let watch = trace_attempt().is_some_and(|want| name.contains(want));
    if watch {
        super::placement::begin_attempt_trace();
        // Armed here rather than around the whole feature so the count
        // covers exactly what vanilla's `Counting` wrapper covers: the
        // placement chain plus the feature body, with the `setSeed` before
        // it excluded.
        crate::rng::draws::arm();
        // `RUSTMC_TRACE_DRAWS=<substring>` prints this feature's draws in
        // `DecorateGolden`'s own `DRAW <n> <method>(<bound>) -> <value>`
        // format, so the two sequences diff line for line.
        //
        // `golden_decorate` already did this from the gate; the real
        // generator could not, so a divergence found in a whole-world dump
        // could be *counted* (`ATTEMPT ... draws=983` against the oracle's
        // 1159) and not *located*. Gated behind `RUSTMC_TRACE_ATTEMPT`
        // matching first, so a server pays nothing.
        crate::rng::draws::set_trace(trace_draws().is_some_and(|want| name.contains(want)));
    }

    if trace_patch() {
        super::vegetation_patch::set_patch_feature(name);
    }
    let mut positions = Vec::new();
    let mut position_draws = Vec::new();
    super::placement::for_each_position(
        &feature.placement,
        world,
        random,
        block,
        |world, random, pos| {
            positions.push(pos);
            // Before the body runs, which is what vanilla's dumper stamps: it
            // records the count as the position leaves the stream, and the
            // feature is what consumes next.
            position_draws.push(crate::rng::draws::peek());
            place(world, feature, pos, random);
            Ok(())
        },
    )?;

    if trace_patch() {
        let (coins, visited, radii) = super::monster_room::take_room_counts();
        if coins > 0 || visited > 0 {
            eprintln!("ROOM {name}: coins={coins} attempts={visited} radii={radii:?}");
        }
        let (sides, layer0, potential) = super::geode::take_geode_counts();
        if sides > 0 || layer0 > 0 || potential > 0 {
            eprintln!(
                "GEODE {name}: crack_sides={sides} layer0_coins={layer0} \
                 potential_coins={potential}"
            );
        }
        let (wrote, no_move, veg) = super::vegetation_patch::take_patch_counts();
        if wrote > 0 || no_move > 0 || veg > 0 {
            let by_chunk = super::vegetation_patch::take_patch_by_chunk();
            let (cols, depth_sum) = super::vegetation_patch::take_patch_columns();
            eprintln!(
                "PATCH {name}: ground wrote={wrote} no_move={no_move} veg_tried={veg} \
                 columns={cols} depth_sum={depth_sum} by_chunk={by_chunk:?}"
            );
        }
    }
    if watch {
        crate::rng::draws::set_trace(false);
        let drew = crate::rng::draws::take();
        if let Some(t) = super::placement::take_attempt_trace() {
            // Named, not indexed. A bare `rejected_at=[0,0,0,159,46,0,47]`
            // means nothing without the placed feature's JSON open beside
            // it, and the whole point of the trace is to answer the question
            // without doing that.
            let by_name: Vec<String> = t
                .rejected_at
                .iter()
                .enumerate()
                .filter(|(_, n)| **n > 0)
                .map(|(i, n)| {
                    let kind = feature
                        .placement
                        .get(i)
                        .map_or("?", super::placement::PlacementModifier::kind);
                    format!("{kind}={n}")
                })
                .collect();
            // `draws` is directly comparable with `DecorateGolden`'s field
            // of the same name — see `rng::draws` for the two ways that
            // comparison goes wrong if the counting is done naively.
            eprintln!(
                "ATTEMPT {name} chunk ({}, {}) reached-feature={} draws={} rejected: {}",
                block.x >> 4,
                block.z >> 4,
                t.emitted,
                drew,
                by_name.join(" ")
            );
            // `RUSTMC_TRACE_ATTEMPT_POS=1` adds the position of every
            // rejection. The counts name the modifier; these let the block
            // at each rejected position be checked against what the filter
            // believed about it.
            if trace_attempt_pos() {
                for (depth, p) in &t.stage_positions {
                    let kind = feature
                        .placement
                        .get(*depth)
                        .map_or("?", super::placement::PlacementModifier::kind);
                    eprintln!("  STAGE #{depth} {kind} -> ({},{},{})", p.x, p.y, p.z);
                }
                for (p, at) in &t.emitted_at {
                    eprintln!("  REACHED at ({},{},{}) draw={at}", p.x, p.y, p.z);
                }
                for (depth, pos) in &t.rejected_positions {
                    let kind = feature
                        .placement
                        .get(*depth)
                        .map_or("?", super::placement::PlacementModifier::kind);
                    eprintln!(
                        "  REJECT #{depth} {kind} at ({},{},{})",
                        pos.x, pos.y, pos.z
                    );
                }
            }
        }
    }

    Ok(Attempt {
        index,
        feature: name.to_string(),
        positions,
        position_draws,
        after: random.next_i64(),
    })
}

/// One chunk's whole decoration — every step, every feature, in order.
///
/// `ChunkGenerator#applyBiomeDecoration`, minus the two things it needs a
/// running server for: the structure pass that shares its step loop, and
/// `FeatureCountTracker`. The rest is here, and the shape is the one
/// `DecorateGolden` reproduces:
///
/// ```text
/// popSeed = setDecorationSeed(worldSeed, origin.x, origin.z)
/// for step in 0..11:
///     for index in sorted(indices of features this step, over biomes present):
///         setFeatureSeed(popSeed, index, step)
///         placeWithBiomeCheck(...)
/// ```
///
/// The three things easy to get wrong, all of them proved by the decoration
/// gate: the index is the feature's place in a **global** ordering computed
/// from every biome the source can produce, not its place in this biome's
/// list; the indices are **sorted**, so features run in that global order
/// rather than in the order the biomes were walked; and the origin is the
/// chunk's **minimum** corner at the world's **minimum** section, not its
/// centre and not y=0.
///
/// `biomes_present` is the set from the region's section palettes — not from
/// the biome source, and not from the centre chunk alone. A harness that
/// collected it wrong decorated nothing and reported no error, because an
/// empty set is a legal answer.
///
/// # Errors
/// If a placement chain names a modifier this port cannot evaluate. The
/// attempts up to that point are lost with it, which is deliberate: a
/// half-decorated chunk is not a thing to hand back.
#[allow(
    clippy::too_many_arguments,
    reason = "\
    vanilla's own inputs, and every one of them feeds the seeding this \
    function exists to get right"
)]
pub fn run_chunk<W, F, S>(
    world: &mut W,
    per_step: &[StepFeatures],
    biome_features: &BTreeMap<String, Vec<Vec<String>>>,
    biomes_present: &[String],
    placed: &dyn Fn(&str) -> Option<PlacedFeature>,
    world_seed: i64,
    origin: ChunkOrigin,
    mut structures: S,
    mut place: F,
) -> Result<Outcome, Unsupported>
where
    W: PlacementWorld + ?Sized,
    F: FnMut(&mut W, &PlacedFeature, &str, Pos, &mut WorldgenRandom<XoroshiroRandomSource>),
    S: FnMut(&mut W, usize, i64),
{
    let population = population_seed(origin, world_seed);
    // ONE source for the whole chunk, reused by every feature in every step.
    //
    // `ChunkGenerator.applyBiomeDecoration` allocates its `WorldgenRandom`
    // once before the step loop and calls `setFeatureSeed` on that same
    // instance per feature. A fresh source per feature reproduces every
    // integer draw — `setFeatureSeed` fully determines the bit stream — but
    // **not** the gaussian cache, which `WorldgenRandom.setSeed` never
    // resets. So a feature drawing an odd number of gaussians leaves half a
    // pair behind, and vanilla hands it to the next feature as its first
    // gaussian. Per-feature sources always start that cache empty, which is
    // invisible to every integer gate and to draw counts alike.
    let mut random = WorldgenRandom::new(XoroshiroRandomSource::from_seed(0));
    let mut attempts = Vec::new();
    let mut refusals = Vec::new();

    for (step, step_features) in per_step.iter().enumerate() {
        // Structures come **before** this step's features, seeded from the
        // same decoration seed with their own counter — see
        // `ChunkGenerator.applyBiomeDecoration`. The two counters share an
        // index space and both start at zero, which is vanilla's own quirk.
        structures(world, step, population);
        for index in attempt_order(step, step_features, biomes_present, biome_features) {
            let name = step_features.features[index].clone();
            let Some(feature) = placed(&name) else {
                refusals.push(Unsupported(format!("{name} is not in the extract")));
                continue;
            };
            match run_feature(
                world,
                &mut random,
                origin,
                population,
                step,
                index,
                &name,
                &feature,
                |world, feature, pos, random| place(world, feature, &name, pos, random),
            ) {
                Ok(attempt) => attempts.push(attempt),
                // **A refusal costs its own feature and nothing else.**
                //
                // This returned early once, which ended the chunk. It looked
                // conservative and was the opposite: one unimplemented
                // survival rule — `would_survive(sugar_cane)` — was deleting
                // every feature that came after it in five chunks, and a
                // whole-world diff blamed the features that never ran.
                //
                // Skipping is safe *because* `setFeatureSeed` re-seeds per
                // feature: the ones after this do not read its draws, so
                // their streams are untouched. What they do lose is its
                // blocks, which is a smaller and honest loss.
                Err(e) => refusals.push(e),
            }
        }
    }
    Ok(Outcome { attempts, refusals })
}

/// What a chunk's decoration produced, and what it could not.
#[derive(Debug)]
pub struct Outcome {
    pub attempts: Vec<Attempt>,
    /// One per feature that could not run. Counted rather than fatal — see
    /// the note in [`run_chunk`].
    pub refusals: Vec<Unsupported>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steps(names: &[&str]) -> StepFeatures {
        StepFeatures {
            features: names.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    /// The order is the *global* one, not the order the biomes list things.
    ///
    /// Two biomes disagreeing about which of two shared features comes first
    /// is the normal case, and the sort is what makes the world single-valued
    /// about it.
    #[test]
    fn the_order_is_global_and_sorted_not_biome_order() {
        let step_features = steps(&["a", "b", "c", "d"]);
        let mut biome_features = BTreeMap::new();
        // This biome lists them backwards.
        biome_features.insert(
            "plains".to_string(),
            vec![
                vec![],
                vec![],
                vec![],
                vec![],
                vec![],
                vec![],
                vec!["d".to_string(), "b".to_string()],
            ],
        );
        biome_features.insert(
            "desert".to_string(),
            vec![
                vec![],
                vec![],
                vec![],
                vec![],
                vec![],
                vec![],
                vec!["c".to_string(), "a".to_string()],
            ],
        );
        let order = attempt_order(
            6,
            &step_features,
            &["plains".to_string(), "desert".to_string()],
            &biome_features,
        );
        assert_eq!(
            order,
            vec![0, 1, 2, 3],
            "sorted global indices, not listed order"
        );
    }

    /// A feature two biomes share is attempted once, not twice.
    #[test]
    fn a_shared_feature_is_attempted_once() {
        let step_features = steps(&["a", "b"]);
        let mut biome_features = BTreeMap::new();
        for biome in ["plains", "desert"] {
            biome_features.insert(
                biome.to_string(),
                vec![vec!["a".to_string(), "b".to_string()]],
            );
        }
        let order = attempt_order(
            0,
            &step_features,
            &["plains".to_string(), "desert".to_string()],
            &biome_features,
        );
        assert_eq!(order, vec![0, 1]);
    }

    /// A biome whose feature list is shorter than the step index contributes
    /// nothing, rather than panicking — vanilla's `step >= features.size()`
    /// guard.
    #[test]
    fn a_biome_with_no_features_at_this_step_is_skipped() {
        let step_features = steps(&["a"]);
        let mut biome_features = BTreeMap::new();
        biome_features.insert("void".to_string(), vec![vec![]]);
        biome_features.insert("plains".to_string(), vec![vec![], vec!["a".to_string()]]);
        let order = attempt_order(
            1,
            &step_features,
            &["void".to_string(), "plains".to_string()],
            &biome_features,
        );
        assert_eq!(order, vec![0]);
    }

    /// The origin is the chunk's minimum corner at the world's minimum
    /// section — the value `setDecorationSeed` hashes.
    #[test]
    fn the_origin_is_the_minimum_corner_at_the_minimum_section() {
        let origin = ChunkOrigin {
            chunk_x: -3,
            chunk_z: 7,
            min_section_y: -4,
        };
        assert_eq!(
            origin.block(),
            Pos {
                x: -48,
                y: -64,
                z: 112
            }
        );
    }

    /// Two chunks get different decoration seeds, and the same chunk gets the
    /// same one twice — the property the whole stage rests on.
    #[test]
    fn the_decoration_seed_is_a_function_of_the_chunk() {
        let seed_for = |chunk_x, chunk_z| {
            let mut random = WorldgenRandom::new(XoroshiroRandomSource::from_seed(0));
            let origin = ChunkOrigin {
                chunk_x,
                chunk_z,
                min_section_y: -4,
            }
            .block();
            random.set_decoration_seed(20260725, origin.x, origin.z)
        };
        assert_eq!(seed_for(4, -9), seed_for(4, -9));
        assert_ne!(seed_for(4, -9), seed_for(4, -8));
        assert_ne!(seed_for(4, -9), seed_for(5, -9));
    }
}

#[cfg(test)]
mod chunk_tests {
    use super::*;
    use crate::feature::placement::NoWorld;

    fn steps(names: &[&str]) -> StepFeatures {
        StepFeatures {
            features: names.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    /// A chain that produces one position and draws nothing but the count.
    fn placed(count: i32) -> PlacedFeature {
        PlacedFeature {
            feature: "minecraft:test".to_string(),
            placement: vec![PlacementModifier::Count(
                crate::feature::int_provider::IntProvider::Constant(count),
            )],
        }
    }

    fn biome_features(entries: &[(&str, usize, &[&str])]) -> BTreeMap<String, Vec<Vec<String>>> {
        let mut out: BTreeMap<String, Vec<Vec<String>>> = BTreeMap::new();
        for (biome, step, features) in entries {
            let steps = out
                .entry((*biome).to_string())
                .or_insert_with(|| vec![Vec::new(); 11]);
            steps[*step] = features.iter().map(|f| (*f).to_string()).collect();
        }
        out
    }

    /// The whole reason this function exists: composing `attempt_order` and
    /// `run_feature` by hand and calling `run_chunk` must give the same
    /// attempts, in the same order, from the same stream.
    ///
    /// The two pieces are each gated against vanilla; what is unproved is the
    /// *loop* around them, and a loop that reseeded per step or walked the
    /// steps backwards would pass every gate they have.
    #[test]
    fn run_chunk_is_the_hand_written_loop() {
        let per_step: Vec<StepFeatures> = (0..11)
            .map(|s| {
                if s == 3 {
                    steps(&["a", "b"])
                } else if s == 7 {
                    steps(&["c"])
                } else {
                    steps(&[])
                }
            })
            .collect();
        let features = biome_features(&[
            ("plains", 3, &["b", "a"]),
            ("desert", 3, &["a"]),
            ("plains", 7, &["c"]),
        ]);
        let biomes = vec!["desert".to_string(), "plains".to_string()];
        let origin = ChunkOrigin {
            chunk_x: 2,
            chunk_z: -5,
            min_section_y: -4,
        };
        let lookup = |name: &str| match name {
            "a" => Some(placed(2)),
            "b" => Some(placed(1)),
            "c" => Some(placed(3)),
            _ => None,
        };

        let mine = run_chunk(
            &mut NoWorld,
            &per_step,
            &features,
            &biomes,
            &lookup,
            777,
            origin,
            |_: &mut NoWorld, _: usize, _: i64| {},
            |_, _, _, _, _| {},
        )
        .expect("supported")
        .attempts;

        // The same thing, spelled out.
        let population = population_seed(origin, 777);
        let mut random = WorldgenRandom::new(XoroshiroRandomSource::from_seed(0));
        let mut theirs = Vec::new();
        for (step, step_features) in per_step.iter().enumerate() {
            for index in attempt_order(step, step_features, &biomes, &features) {
                let name = step_features.features[index].clone();
                let feature = lookup(&name).expect("in the extract");
                theirs.push(
                    run_feature(
                        &mut NoWorld,
                        &mut random,
                        origin,
                        population,
                        step,
                        index,
                        &name,
                        &feature,
                        |_, _, _, _| {},
                    )
                    .expect("supported"),
                );
            }
        }

        assert_eq!(mine.len(), 3, "a and b in step 3, c in step 7 — {mine:?}");
        assert_eq!(mine, theirs);
        // And the order is the global one: `a` before `b` even though plains
        // lists them the other way round.
        assert_eq!(mine[0].feature, "a");
        assert_eq!(mine[1].feature, "b");
        assert_eq!(mine[2].feature, "c");
    }

    /// A feature the extract does not carry is an error, not a skip. Skipping
    /// it would leave its draws unspent and move every feature after it in
    /// the step.
    #[test]
    fn a_missing_feature_is_refused_rather_than_skipped() {
        let per_step: Vec<StepFeatures> = (0..11)
            .map(|s| if s == 0 { steps(&["gone"]) } else { steps(&[]) })
            .collect();
        let features = biome_features(&[("plains", 0, &["gone"])]);
        let error = run_chunk(
            &mut NoWorld,
            &per_step,
            &features,
            &["plains".to_string()],
            &|_| None,
            1,
            ChunkOrigin {
                chunk_x: 0,
                chunk_z: 0,
                min_section_y: -4,
            },
            |_: &mut NoWorld, _: usize, _: i64| {},
            |_, _, _, _, _| {},
        )
        .expect("a refusal is not fatal");
        assert_eq!(
            error.refusals.len(),
            1,
            "the one feature that could not run"
        );
        assert!(
            error.refusals[0].0.contains("gone"),
            "{}",
            error.refusals[0].0
        );
        assert!(
            error.attempts.is_empty(),
            "and nothing else was in that step to run"
        );
    }

    /// Each step reseeds from the *population* seed, not from wherever the
    /// last step left the stream — so an empty step costs nothing and a
    /// feature's seed depends only on its index and step.
    #[test]
    fn a_features_seed_does_not_depend_on_what_ran_before_it() {
        let origin = ChunkOrigin {
            chunk_x: 0,
            chunk_z: 0,
            min_section_y: -4,
        };
        let lonely: Vec<StepFeatures> = (0..11)
            .map(|s| if s == 5 { steps(&["x"]) } else { steps(&[]) })
            .collect();
        let crowded: Vec<StepFeatures> = (0..11)
            .map(|s| match s {
                0 => steps(&["noise"]),
                5 => steps(&["x"]),
                _ => steps(&[]),
            })
            .collect();
        let lookup = |_: &str| Some(placed(4));

        let run = |per_step: &[StepFeatures], features| {
            run_chunk(
                &mut NoWorld,
                per_step,
                &features,
                &["plains".to_string()],
                &lookup,
                99,
                origin,
                |_: &mut NoWorld, _: usize, _: i64| {},
                |_, _, _, _, _| {},
            )
            .expect("supported")
            .attempts
        };

        let a = run(&lonely, biome_features(&[("plains", 5, &["x"])]));
        let b = run(
            &crowded,
            biome_features(&[("plains", 0, &["noise"]), ("plains", 5, &["x"])]),
        );

        let x_a = a.iter().find(|at| at.feature == "x").expect("x ran");
        let x_b = b.iter().find(|at| at.feature == "x").expect("x ran");
        assert_eq!(
            x_a.after, x_b.after,
            "a whole feature ran before it in the crowded world and its \
             stream is still the same"
        );
    }
}
