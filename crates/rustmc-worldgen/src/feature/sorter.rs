//! The global feature order — which index seeds which feature.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `net.minecraft.world.level.biome.FeatureSorter#buildFeaturesPerStep`,
//! `net.minecraft.util.Graph#depthFirstSearch`.
//!
//! Every feature placed in the world is seeded with
//! `setFeatureSeed(decorationSeed, indexWithinStep, step)`, and that index
//! comes from **one global ordering computed once from the whole biome
//! registry** — not from the biome being decorated. So this function decides
//! the seed of every tree, ore and lake in the world, and it has to be right
//! before anything downstream can be checked at all.
//!
//! # It is a topological sort, and the reason is subtle
//!
//! Each biome lists its features grouped by decoration step, and *within a
//! biome* that order is meaningful: lakes before ores before vegetation. But
//! two biomes can disagree about the relative order of two features they
//! share, and the game needs one order for the whole world.
//!
//! So each biome contributes a **chain** of edges — its features in listed
//! order, each pointing at the next, spanning all its steps — and the global
//! order is a topological sort of the union. If the biomes are mutually
//! inconsistent there is a cycle and vanilla refuses to start the world. That
//! is a real failure mode datapack authors hit, not a theoretical one.
//!
//! Only a disagreement *within one step* can close a loop, because the node
//! is `(step, index)`: "a before b" at step 0 and "b before a" at step 1 are
//! statements about four different nodes, and a datapack may say both.
//!
//! # Three orderings are load-bearing
//!
//! - Features are numbered in **first-encounter order** across the biome
//!   list, so the biome iteration order feeds the node ids.
//! - Nodes compare by `(step, index)`, and both the edge map and each edge
//!   set are sorted by that. A hash set anywhere here would make the world
//!   depend on hash order.
//! - The DFS emits **post-order** and the result is then **reversed**.
//!
//! Getting any of them wrong produces a valid-looking world in which every
//! feature is seeded differently.

use std::collections::{BTreeMap, BTreeSet};

/// A node: one feature at one decoration step.
///
/// Ordered by `(step, index)` — vanilla's
/// `comparingInt(step).thenComparingInt(featureIndex)` — which is exactly
/// the derived tuple order with the fields in this sequence. They are in this
/// sequence on purpose.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct FeatureData {
    step: usize,
    index: usize,
}

/// One decoration step's globally-ordered feature list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepFeatures {
    /// Feature names in the order that defines their seeds. A feature's
    /// position here **is** the `indexWithinStep` passed to
    /// `setFeatureSeed`.
    pub features: Vec<String>,
}

impl StepFeatures {
    /// `StepFeatureData#indexMapping`.
    #[must_use]
    pub fn index_of(&self, feature: &str) -> Option<usize> {
        self.features.iter().position(|f| f == feature)
    }
}

/// `FeatureSorter#buildFeaturesPerStep`.
///
/// `sources` is one entry per biome, each a list of decoration steps, each a
/// list of placed-feature names in the biome's own order.
///
/// # Errors
/// Returns the two features that close a cycle, if the biomes disagree
/// irreconcilably about feature order. Vanilla refuses to start the world in
/// that case, having first narrowed down which biomes are involved; the
/// narrowing is a diagnostic and is not reproduced.
pub fn build_features_per_step(sources: &[Vec<Vec<String>>]) -> Result<Vec<StepFeatures>, String> {
    // First-encounter order across the whole biome list, so the biome
    // iteration order feeds the node ids. A map from name to index plus a
    // vector back, because both directions are needed and the forward one
    // must not reorder.
    let mut index_of: BTreeMap<&str, usize> = BTreeMap::new();
    let mut name_of: Vec<&str> = Vec::new();

    let mut edges: BTreeMap<FeatureData, BTreeSet<FeatureData>> = BTreeMap::new();
    let mut max_step = 0usize;

    for source in sources {
        let mut chain: Vec<FeatureData> = Vec::new();
        max_step = max_step.max(source.len());
        for (step, features_in_step) in source.iter().enumerate() {
            for feature in features_in_step {
                let index = *index_of.entry(feature.as_str()).or_insert_with(|| {
                    name_of.push(feature.as_str());
                    name_of.len() - 1
                });
                chain.push(FeatureData { step, index });
            }
        }

        // Every element gets a key, including the last — a feature that is
        // only ever a successor still has to be a node, or the sort would
        // never visit it from the outer loop.
        for i in 0..chain.len() {
            let entry = edges.entry(chain[i]).or_default();
            if i + 1 < chain.len() {
                entry.insert(chain[i + 1]);
            }
        }
    }

    let mut discovered: BTreeSet<FeatureData> = BTreeSet::new();
    let mut visiting: BTreeSet<FeatureData> = BTreeSet::new();
    let mut sorted: Vec<FeatureData> = Vec::new();

    let roots: Vec<FeatureData> = edges.keys().copied().collect();
    for root in roots {
        assert!(
            visiting.is_empty(),
            "DFS left a vertex in progress between roots"
        );
        if !discovered.contains(&root)
            && depth_first_search(&edges, &mut discovered, &mut visiting, &mut sorted, root)
        {
            return Err(format!(
                "feature order cycle involving `{}` at step {}",
                name_of[root.index], root.step
            ));
        }
    }

    // Post-order reversed is the topological order.
    sorted.reverse();

    Ok((0..max_step)
        .map(|step| StepFeatures {
            features: sorted
                .iter()
                .filter(|data| data.step == step)
                .map(|data| name_of[data.index].to_string())
                .collect(),
        })
        .collect())
}

/// `Graph#depthFirstSearch`. Returns whether a cycle was found.
fn depth_first_search(
    edges: &BTreeMap<FeatureData, BTreeSet<FeatureData>>,
    discovered: &mut BTreeSet<FeatureData>,
    visiting: &mut BTreeSet<FeatureData>,
    order: &mut Vec<FeatureData>,
    current: FeatureData,
) -> bool {
    if discovered.contains(&current) {
        return false;
    }
    if visiting.contains(&current) {
        return true;
    }
    visiting.insert(current);
    if let Some(next_set) = edges.get(&current) {
        for next in next_set {
            if depth_first_search(edges, discovered, visiting, order, *next) {
                return true;
            }
        }
    }
    visiting.remove(&current);
    discovered.insert(current);
    order.push(current);
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steps(names: &[&[&str]]) -> Vec<Vec<String>> {
        names
            .iter()
            .map(|step| step.iter().map(|s| (*s).to_string()).collect())
            .collect()
    }

    #[test]
    fn one_biome_keeps_its_own_order() {
        let out = build_features_per_step(&[steps(&[&["a", "b"], &["c"]])]).expect("sorts");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].features, vec!["a", "b"]);
        assert_eq!(out[1].features, vec!["c"]);
    }

    #[test]
    fn two_biomes_are_merged_consistently() {
        // Neither biome lists both `b` and `x`, so the sort has freedom —
        // but it must place `a` before `b` and `a` before `x`, since both
        // biomes say so.
        let out = build_features_per_step(&[steps(&[&["a", "b"]]), steps(&[&["a", "x"]])])
            .expect("sorts");
        let s = &out[0].features;
        assert_eq!(s.len(), 3);
        let pos = |n: &str| s.iter().position(|f| f == n).expect("present");
        assert!(pos("a") < pos("b"));
        assert!(pos("a") < pos("x"));
    }

    #[test]
    fn a_contradiction_between_biomes_is_a_cycle() {
        // One biome says a then b, the other says b then a. Vanilla refuses
        // to start the world; a port that silently picked one would generate
        // a world nobody else can reproduce.
        let error = build_features_per_step(&[steps(&[&["a", "b"]]), steps(&[&["b", "a"]])])
            .expect_err("should be a cycle");
        assert!(error.contains("cycle"), "{error}");
    }

    #[test]
    fn a_feature_at_different_steps_in_different_biomes_appears_in_both() {
        // Written first expecting step 0 to be exactly `[a, b]`, on the
        // assumption that listing `c` anywhere put it in one step. It does
        // not: the node is `(step, index)`, so a feature two biomes place at
        // different steps is two nodes and appears in both lists — with a
        // different seed in each, since the step is an argument to
        // `setFeatureSeed`. That is a fact about how worlds are seeded, not a
        // quirk of this implementation.
        let out = build_features_per_step(&[steps(&[&["a", "b"], &["c"]]), steps(&[&["c"], &[]])])
            .expect("sorts");
        assert!(out[0].features.contains(&"c".to_string()));
        assert_eq!(out[1].features, vec!["c"]);
        // And the within-biome order still holds where both are in a step.
        let s = &out[0].features;
        let pos = |n: &str| s.iter().position(|f| f == n).expect("present");
        assert!(pos("a") < pos("b"));
    }

    #[test]
    fn a_contradiction_across_steps_is_not_a_cycle() {
        // Written expecting this to throw, by analogy with the same-step
        // case. It does not, and the reason is worth keeping: the node is
        // `(step, index)`, so "a before b" in step 0 and "b before a" in
        // step 1 are statements about four different nodes. Only a
        // disagreement *within* one step can close a loop.
        //
        // Which is also why a datapack can list the same two features in
        // opposite orders at different steps and the world still loads.
        let out = build_features_per_step(&[steps(&[&["a"], &["b"]]), steps(&[&["b"], &["a"]])])
            .expect("cross-step disagreement is legal");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].features.len(), 2);
        assert_eq!(out[1].features.len(), 2);
    }

    #[test]
    fn a_features_index_is_its_position_in_its_step() {
        let out = build_features_per_step(&[steps(&[&["a", "b", "c"]])]).expect("sorts");
        assert_eq!(out[0].index_of("a"), Some(0));
        assert_eq!(out[0].index_of("c"), Some(2));
        assert_eq!(out[0].index_of("missing"), None);
    }

    #[test]
    fn a_feature_at_two_steps_is_two_nodes() {
        // The node is `(step, index)`, so the same feature listed at two
        // steps appears in both — with a different seed in each, since the
        // step is part of `setFeatureSeed`.
        let out = build_features_per_step(&[steps(&[&["a"], &["a"]])]).expect("sorts");
        assert_eq!(out[0].features, vec!["a"]);
        assert_eq!(out[1].features, vec!["a"]);
    }

    #[test]
    fn an_empty_step_still_occupies_its_slot() {
        // Steps are positional: an empty one in the middle must not shift
        // the ones after it, or every later feature's seed changes.
        let out = build_features_per_step(&[steps(&[&["a"], &[], &["b"]])]).expect("sorts");
        assert_eq!(out.len(), 3);
        assert!(out[1].features.is_empty());
        assert_eq!(out[2].features, vec!["b"]);
    }

    #[test]
    fn the_result_is_independent_of_how_often_a_feature_is_listed() {
        // A feature shared by fifty biomes is still one node with one index.
        let many: Vec<Vec<Vec<String>>> = (0..50).map(|_| steps(&[&["a", "b"]])).collect();
        let out = build_features_per_step(&many).expect("sorts");
        assert_eq!(out[0].features, vec!["a", "b"]);
    }
}
