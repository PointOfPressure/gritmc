//! `CappedProcessor` — apply a processor to at most *n* blocks of a template.
//!
//! Not ocean-ruin-specific: this is the generic wrapper vanilla uses wherever
//! a template should be only partly rewritten. Ocean ruins are simply the
//! first caller this port needed.
//!
//! # The two details that decide the output
//!
//! Both were read from `CappedProcessor.finalizeProcessing`'s bytecode, and
//! both go the way that is *not* the obvious guess.
//!
//! **The random source is positional and comes from the world seed.**
//!
//! ```java
//!   RandomSource random = RandomSource.createThreadLocalInstance(level.getSeed())
//!                                     .forkPositional()
//!                                     .at(pos);          // aload_2, the piece origin
//! ```
//!
//! `createThreadLocalInstance(long)` is `new SingleThreadedRandomSource(seed)`,
//! the legacy LCG — javap-confirmed, not inferred from the name. So this
//! processor spends **nothing** from the structure's own stream, exactly like
//! `BlockRotProcessor`. A port that drew from the feature stream here would
//! still produce plausible-looking output while displacing every later
//! structure in the chunk, which is the kind of error that survives eyeballing
//! and dies only to a block-for-block diff.
//!
//! **The budget counts changes, not visits.**
//!
//! ```java
//!   for (int i : shuffledOrder) {
//!       if (done >= n) break;
//!       StructureBlockInfo out = delegate.processBlock(..., list.get(i), ...);
//!       if (out != null && !list.get(i).equals(out)) {
//!           done++;                                      // iinc 11, 1
//!           ...
//!       }
//!   }
//! ```
//!
//! The increment sits **inside** the null-and-inequality guard. A template is
//! mostly not sand, so counting visits instead of changes would exhaust the
//! budget on blocks the rule declined and yield far fewer than the cap. On a
//! warm ruin the cap is 5 and a server's world holds exactly 5 suspicious
//! sand, so the distinction is directly observable rather than theoretical.
//!
//! `ConstantInt.sample` ignores its argument, so for a constant cap the only
//! draws are the shuffle's.

use crate::rng::legacy::LegacyRandomSource;
use crate::rng::{PositionalRandomFactory, RandomSource};

/// `Util.toShuffledList(IntStream.range(0, len), random)`.
///
/// Descending Fisher-Yates with bound `i`, **not** `i + 1`:
///
/// ```java
///   for (int i = size; i > 1; i--) swap(list, i - 1, random.nextInt(i));
/// ```
///
/// The bound is the part worth stating twice. `nextInt(i + 1)` is the more
/// familiar formulation and would consume the same number of draws while
/// producing a different permutation, so it fails silently.
pub fn shuffled_order<R: RandomSource + ?Sized>(random: &mut R, len: usize) -> Vec<usize> {
    let mut order: Vec<usize> = (0..len).collect();
    let mut i = len;
    while i > 1 {
        let j = random.next_i32_bound(i as i32) as usize;
        order.swap(i - 1, j);
        i -= 1;
    }
    order
}

/// Which template entries a capped processor rewrites, in application order.
///
/// `change` reports whether the delegate would actually change entry `index` —
/// vanilla's `out != null && !original.equals(out)`. Returning `false` costs
/// nothing from the budget, which is the whole subtlety of this wrapper.
///
/// `origin` is the piece's world position, the `BlockPos` vanilla forks at.
pub fn capped_selection<F>(
    world_seed: i64,
    origin: (i32, i32, i32),
    len: usize,
    cap: i32,
    mut change: F,
) -> Vec<usize>
where
    F: FnMut(usize) -> bool,
{
    // `if (limit.maxInclusive() == 0 || list.isEmpty()) return list;`
    if cap == 0 || len == 0 {
        return Vec::new();
    }

    let mut source = LegacyRandomSource::new(world_seed);
    let factory = source.fork_positional();
    let mut random = factory.at(origin.0, origin.1, origin.2);

    // `Math.min(limit.sample(random), list.size())`. A constant provider does
    // not draw, so nothing is spent here for the ocean ruin's ConstantInt(5).
    let n = (cap as usize).min(len);
    if n < 1 {
        return Vec::new();
    }

    let order = shuffled_order(&mut random, len);

    let mut chosen = Vec::new();
    for index in order {
        if chosen.len() >= n {
            break;
        }
        if change(index) {
            chosen.push(index);
        }
    }
    chosen
}

/// The order a structure set's entries are attempted in.
///
/// `ChunkGenerator.createStructures` picks *which* structure of a set to try
/// at a placement chunk, and retries the rest if the chosen one refuses —
/// which it does when the chunk's biome is not in that structure's biome tag.
/// Decoded from bytecode:
///
/// ```java
///   List<StructureSelectionEntry> entries = set.structures();
///   if (entries.size() == 1) {                    // <- fast path, NO DRAW
///       tryGenerateStructure(entries.get(0), ...);
///       return;
///   }
///   List<StructureSelectionEntry> remaining = new ArrayList<>(entries);
///   WorldgenRandom random = new WorldgenRandom(new LegacyRandomSource(0));
///   random.setLargeFeatureSeed(levelSeed, chunkPos.x(), chunkPos.z());
///   int total = sum of weights;
///   while (!remaining.isEmpty()) {
///       int roll = random.nextInt(total);
///       int index = 0;
///       for (e : remaining) { roll -= e.weight(); if (roll < 0) break; index++; }
///       if (tryGenerateStructure(remaining.get(index), ...)) return;
///       remaining.remove(index);
///       total -= weight;
///   }
/// ```
///
/// **The single-entry fast path is the load-bearing part.** Fourteen of the
/// twenty sets hold one structure, and for those vanilla never touches the
/// random — so a port that "correctly" drew once per set would diverge on
/// every one of them. The draw exists only where there is a choice to make.
///
/// The random is its own `LegacyRandomSource(0)` reseeded per chunk, so these
/// draws are isolated: they cannot shift a feature or another structure.
///
/// Returns the entry indices in the order vanilla would attempt them. The
/// caller decides which attempt succeeds, because that is a biome question and
/// this is arithmetic.
pub fn weighted_attempt_order(
    seed: i64,
    chunk_x: i32,
    chunk_z: i32,
    weights: &[i32],
) -> Vec<usize> {
    use crate::rng::worldgen_random::WorldgenRandom;

    if weights.len() <= 1 {
        return (0..weights.len()).collect();
    }

    let mut random = WorldgenRandom::new(LegacyRandomSource::new(0));
    random.set_large_feature_seed(seed, chunk_x, chunk_z);

    // `remaining` holds original indices so the caller can name the structure.
    let mut remaining: Vec<usize> = (0..weights.len()).collect();
    let mut total: i32 = weights.iter().sum();
    let mut order = Vec::with_capacity(weights.len());

    while !remaining.is_empty() {
        let mut roll = random.next_i32_bound(total);
        let mut index = 0;
        for (i, &entry) in remaining.iter().enumerate() {
            roll -= weights[entry];
            if roll < 0 {
                index = i;
                break;
            }
        }
        let entry = remaining.remove(index);
        total -= weights[entry];
        order.push(entry);
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shuffle must be a permutation whatever the length — the property
    /// that an off-by-one in the bound would not break, which is why the
    /// permutation test is not the only one here.
    #[test]
    fn the_shuffle_is_a_permutation() {
        for len in 0..40usize {
            let mut random = LegacyRandomSource::new(12345);
            let order = shuffled_order(&mut random, len);
            let mut sorted = order.clone();
            sorted.sort_unstable();
            assert_eq!(sorted, (0..len).collect::<Vec<_>>(), "len {len}");
        }
    }

    /// The bound is `i`, not `i + 1`. Stated as a draw-sequence check so the
    /// two formulations are distinguished by evidence rather than by reading.
    #[test]
    fn the_shuffle_draws_bound_i_not_i_plus_one() {
        // Reproduce the intended sequence by hand and compare permutations.
        let len = 6;
        let mut a = LegacyRandomSource::new(999);
        let got = shuffled_order(&mut a, len);

        let mut b = LegacyRandomSource::new(999);
        let mut want: Vec<usize> = (0..len).collect();
        let mut i = len;
        while i > 1 {
            let j = b.next_i32_bound(i as i32) as usize;
            want.swap(i - 1, j);
            i -= 1;
        }
        assert_eq!(got, want);

        // And the off-by-one really is a different answer, so the test above
        // has something to fail against.
        let mut c = LegacyRandomSource::new(999);
        let mut wrong: Vec<usize> = (0..len).collect();
        let mut i = len;
        while i > 1 {
            let j = c.next_i32_bound(i as i32 + 1) as usize;
            wrong.swap(i - 1, j.min(len - 1));
            i -= 1;
        }
        assert_ne!(
            got, wrong,
            "if these agreed, the bound test would prove nothing"
        );
    }

    /// The budget counts changes. With only three eligible entries out of
    /// twenty and a cap of five, all three must be taken — a visit-counting
    /// implementation would stop early and take fewer.
    #[test]
    fn declined_entries_do_not_consume_the_budget() {
        let eligible = [3usize, 11, 17];
        let chosen = capped_selection(31_415_926, (1072, 48, 1040), 20, 5, |i| {
            eligible.contains(&i)
        });
        let mut sorted = chosen.clone();
        sorted.sort_unstable();
        assert_eq!(
            sorted,
            eligible.to_vec(),
            "every eligible entry is reached because the declined ones are free"
        );
    }

    /// And the cap really does bind when there is more than enough to change.
    #[test]
    fn the_cap_binds_when_everything_is_eligible() {
        let chosen = capped_selection(31_415_926, (1072, 48, 1040), 60, 5, |_| true);
        assert_eq!(chosen.len(), 5);
        let mut sorted = chosen.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 5, "no entry is chosen twice");
    }

    /// A zero cap or an empty list returns before touching the random source,
    /// which matters because reaching it would still be deterministic and so
    /// would hide the early return.
    #[test]
    fn a_zero_cap_or_empty_list_selects_nothing() {
        assert!(capped_selection(1, (0, 0, 0), 10, 0, |_| true).is_empty());
        assert!(capped_selection(1, (0, 0, 0), 0, 5, |_| true).is_empty());
    }

    /// A one-entry set must not touch the random at all — fourteen of the
    /// twenty sets are like this, and a spurious draw there would be a
    /// divergence on every one of them.
    #[test]
    fn a_single_entry_set_is_attempted_without_any_draw() {
        assert_eq!(weighted_attempt_order(31_415_926, 67, 65, &[1]), vec![0]);
        assert_eq!(
            weighted_attempt_order(31_415_926, 67, 65, &[]),
            Vec::<usize>::new()
        );
    }

    /// Every entry is attempted exactly once, in some order — the retry loop
    /// exhausts the list rather than giving up after the first refusal.
    #[test]
    fn the_attempt_order_is_a_permutation_of_the_entries() {
        for weights in [
            vec![1, 1],
            vec![1, 1, 1, 1, 1],
            vec![3, 1],
            vec![1, 2, 4, 8],
        ] {
            let order = weighted_attempt_order(20_260_722, 12, -7, &weights);
            let mut sorted = order.clone();
            sorted.sort_unstable();
            assert_eq!(
                sorted,
                (0..weights.len()).collect::<Vec<_>>(),
                "{weights:?}"
            );
        }
    }

    /// Weight actually steers the first attempt: over many chunks a 9:1 split
    /// must favour the heavy entry, which a bucket walk with the comparison
    /// the wrong way round would invert.
    #[test]
    fn weight_decides_which_entry_is_tried_first() {
        let mut heavy_first = 0;
        for cx in 0..200 {
            if weighted_attempt_order(31_415_926, cx, 0, &[9, 1])[0] == 0 {
                heavy_first += 1;
            }
        }
        assert!(
            heavy_first > 150,
            "a 9:1 weight should lead most of the time, led {heavy_first}/200"
        );
    }

    /// The selection is positional: two different piece origins under one
    /// world seed must not select the same entries.
    #[test]
    fn the_selection_depends_on_the_piece_origin() {
        let a = capped_selection(31_415_926, (1072, 48, 1040), 60, 5, |_| true);
        let b = capped_selection(31_415_926, (1072, 48, 1056), 60, 5, |_| true);
        assert_ne!(a, b, "the fork is positional, so the origin must matter");
    }
}
