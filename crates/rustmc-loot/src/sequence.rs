//! `RandomSequences` — the server-wide, named, persistent generators a loot
//! table with no explicit seed draws from.
//!
//! # From the jar, and where it lives
//!
//! In 26.2 the store hangs off **`MinecraftServer`**, not a level:
//! `MinecraftServer.getRandomSequence(Identifier)` is
//!
//! ```text
//!    0: getfield randomSequences
//!    5: getfield worldGenSettings -> options() -> seed()      -- the WORLD seed
//!   15: RandomSequences.get(Identifier, long)
//! ```
//!
//! so every dimension shares one generator per id, and the sequence a chest
//! in the Nether draws from is the same object an Overworld chest of the same
//! table advanced.
//!
//! `RandomSequences.get(id, worldSeed)` is `sequences.computeIfAbsent(id,
//! k -> createSequence(k, worldSeed))` wrapped in a `DirtyMarkingRandomSource`
//! that marks the saved data dirty and otherwise **delegates every call
//! unchanged** (`nextInt(I)`, `nextFloat()`, … each `setDirty` then forward),
//! so the wrapper is invisible to the stream.
//!
//! `createSequence(id, seed, salt, includeWorldSeed, includeSequenceId)`:
//!
//! ```text
//!    0: iload 5 (includeWorldSeed) ? seed : 0L
//!   10: ^ (long) salt
//!   16: new RandomSequence(that, includeSequenceId ? Optional.of(id) : empty)
//! RandomSequence.createSequence(long, Optional<Identifier>):
//!    0: RandomSupport.upgradeSeedTo128bitUnmixed(seed)
//!   12: if present: .xor(seedForKey(id))       -- seedFromHashOf(id.toString())
//!   27: new XoroshiroRandomSource(seed128.mixed())
//! ```
//!
//! The defaults are `salt = 0`, `includeWorldSeed = true`,
//! `includeSequenceId = true` (the no-argument constructor, offsets 4..11);
//! only `/random reset` changes them, which this server does not implement.
//!
//! # It is state, and the state is the hard part
//!
//! A sequence is created on first use and then **advanced by every roll that
//! uses it for the life of the world** (it is saved in `random_sequences.dat`).
//! So two servers agree on the third pig's drops only if they agree on the
//! first two. That is what makes this a parity surface rather than a formula:
//! a fresh generator per roll would reproduce the first roll of every table
//! exactly and every later one by accident.

use std::collections::HashMap;

use rustmc_worldgen::rng::support::{seed_from_hash_of, upgrade_seed_to_128bit_unmixed};
use rustmc_worldgen::rng::{RandomSource, XoroshiroRandomSource};

/// `RandomSequence.createSequence(long, Optional<Identifier>)` — one fresh
/// generator. `id` is the full identifier string, namespace included, which is
/// what `Identifier.toString()` hashes.
#[must_use]
pub fn create_sequence(seed: i64, id: Option<&str>) -> XoroshiroRandomSource {
    let mut s = upgrade_seed_to_128bit_unmixed(seed);
    if let Some(id) = id {
        let k = seed_from_hash_of(&normalise(id));
        s = s.xor(k.lo, k.hi);
    }
    XoroshiroRandomSource::from_seed128(s.mixed())
}

fn normalise(id: &str) -> String {
    if id.contains(':') {
        id.to_string()
    } else {
        format!("minecraft:{id}")
    }
}

/// The server's whole set of named sequences.
#[derive(Debug, Clone)]
pub struct RandomSequences {
    world_seed: i64,
    salt: i32,
    include_world_seed: bool,
    include_sequence_id: bool,
    sequences: HashMap<String, XoroshiroRandomSource>,
}

impl RandomSequences {
    /// A fresh store for a world of `world_seed`, with vanilla's defaults.
    #[must_use]
    pub fn new(world_seed: i64) -> Self {
        Self {
            world_seed,
            salt: 0,
            include_world_seed: true,
            include_sequence_id: true,
            sequences: HashMap::new(),
        }
    }

    #[must_use]
    pub const fn world_seed(&self) -> i64 {
        self.world_seed
    }

    /// `RandomSequences.get(id, worldSeed)`: the live generator for `id`,
    /// created on first use. Draws on the returned generator advance the
    /// stored one, which is the point.
    pub fn get(&mut self, id: &str) -> &mut dyn RandomSource {
        let key = normalise(id);
        let seed = (if self.include_world_seed {
            self.world_seed
        } else {
            0
        }) ^ i64::from(self.salt);
        let with_id = self.include_sequence_id;
        self.sequences
            .entry(key)
            .or_insert_with_key(|k| create_sequence(seed, with_id.then_some(k.as_str())))
    }

    /// What `RandomSequences.CODEC` saves: `salt`, the two flags (written
    /// only when not their default `true` — `optionalFieldOf(.., true)`),
    /// and every sequence's generator state, `seedLo`/`seedHi`, keyed by id.
    /// Sorted by id for a reproducible file; vanilla's is its hash map's
    /// order, which no reader depends on.
    #[must_use]
    pub fn saved(&self) -> SavedSequences {
        let mut sequences: Vec<(String, i64, i64)> = self
            .sequences
            .iter()
            .map(|(k, v)| {
                let (lo, hi) = v.state();
                (k.clone(), lo, hi)
            })
            .collect();
        sequences.sort();
        SavedSequences {
            salt: self.salt,
            include_world_seed: self.include_world_seed,
            include_sequence_id: self.include_sequence_id,
            sequences,
        }
    }

    /// A store reopened from what [`Self::saved`] wrote: each sequence
    /// continues exactly where it stopped, and one never drawn from is
    /// created afresh from the world seed on first use, as before the save.
    #[must_use]
    pub fn restore(world_seed: i64, saved: &SavedSequences) -> Self {
        Self {
            world_seed,
            salt: saved.salt,
            include_world_seed: saved.include_world_seed,
            include_sequence_id: saved.include_sequence_id,
            sequences: saved
                .sequences
                .iter()
                .map(|(k, lo, hi)| (normalise(k), XoroshiroRandomSource::from_parts(*lo, *hi)))
                .collect(),
        }
    }

    /// How many sequences exist — `RandomSequences.clear()`'s return value.
    #[must_use]
    pub fn len(&self) -> usize {
        self.sequences.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sequences.is_empty()
    }
}

/// `random_sequences.dat`'s `data` compound, as plain values.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SavedSequences {
    pub salt: i32,
    pub include_world_seed: bool,
    pub include_sequence_id: bool,
    /// `(id, seedLo, seedHi)`.
    pub sequences: Vec<(String, i64, i64)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The store hands back the SAME generator on the second call, advanced
    /// by the first. A store that re-created it would pass every single-roll
    /// test there is and fail the second chest.
    #[test]
    fn a_sequence_is_created_once_and_then_advanced() {
        let mut seqs = RandomSequences::new(20_260_803);
        let a = seqs.get("minecraft:chests/simple_dungeon").next_i64();
        let b = seqs.get("chests/simple_dungeon").next_i64();
        let mut fresh = create_sequence(20_260_803, Some("minecraft:chests/simple_dungeon"));
        assert_eq!(a, fresh.next_i64());
        assert_eq!(
            b,
            fresh.next_i64(),
            "the second draw continues the first stream"
        );
        assert_eq!(seqs.len(), 1, "the bare and namespaced ids are one key");
    }

    /// Against a world vanilla generated and saved: seed 20260722's
    /// `data/minecraft/random_sequences.dat` in
    /// `oracle-work/gen-20260722-25852` holds five block-drop sequences
    /// (plants that decoration broke). Each saved state must be this
    /// crate's fresh sequence for that id advanced by some number of draws —
    /// vanilla's own world, not a vector this repo's harness produced. The
    /// draw counts found are pinned too, so a seeding that happened to land
    /// on a state further down some other stream cannot pass. Four were
    /// created and never drawn from — their tables' pools have one live
    /// entry, which takes no `nextInt` — and `short_grass` is 27 draws in.
    #[test]
    fn a_vanilla_saved_world_holds_these_sequences_a_few_draws_in() {
        let saved: [(&str, i64, i64, usize); 5] = [
            (
                "minecraft:blocks/brown_mushroom",
                -5_300_478_071_468_614_248,
                -9_117_539_583_877_138_803,
                0,
            ),
            (
                "minecraft:blocks/lily_pad",
                473_497_248_599_286_509,
                -345_979_707_754_697_255,
                0,
            ),
            (
                "minecraft:blocks/moss_carpet",
                1_824_778_007_280_304_611,
                -7_600_214_923_233_612_270,
                0,
            ),
            (
                "minecraft:blocks/short_grass",
                -5_179_943_105_361_610_497,
                3_047_743_234_313_531_773,
                27,
            ),
            (
                "minecraft:blocks/vine",
                645_116_517_030_548_946,
                -2_528_457_923_960_760_173,
                0,
            ),
        ];
        for (id, lo, hi, draws) in saved {
            let mut s = create_sequence(20_260_722, Some(id));
            let mut found = None;
            for k in 0..100_000 {
                if s.state() == (lo, hi) {
                    found = Some(k);
                    break;
                }
                s.next_i64();
            }
            println!("{id}: {found:?} draws");
            assert_eq!(
                found,
                Some(draws),
                "{id}: vanilla's saved state is not this crate's stream after {draws} draws"
            );
        }
    }

    /// Save then restore: a restored store continues every stream exactly.
    #[test]
    fn a_restored_store_continues_where_it_stopped() {
        let mut a = RandomSequences::new(7);
        a.get("minecraft:chests/igloo_chest").next_i64();
        a.get("minecraft:entities/pig").next_i32_bound(3);
        let mut b = RandomSequences::restore(7, &a.saved());
        for id in [
            "minecraft:chests/igloo_chest",
            "minecraft:entities/pig",
            "minecraft:entities/cow",
        ] {
            assert_eq!(a.get(id).next_i64(), b.get(id).next_i64(), "{id}");
        }
    }

    /// Two ids under one world seed are independent streams, and one id under
    /// two world seeds differs: both halves of the seed are live.
    #[test]
    fn both_the_world_seed_and_the_id_reach_the_stream() {
        let x = create_sequence(1, Some("minecraft:entities/pig")).next_i64();
        let y = create_sequence(1, Some("minecraft:entities/cow")).next_i64();
        let z = create_sequence(2, Some("minecraft:entities/pig")).next_i64();
        assert_ne!(x, y);
        assert_ne!(x, z);
    }
}
