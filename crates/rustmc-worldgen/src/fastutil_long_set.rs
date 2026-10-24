//! `it.unimi.dsi.fastutil.longs.LongOpenHashSet` — insertion and iteration
//! order.
//!
//! Reference (fastutil, as shipped in the 26.2 server jar; behaviour ported —
//! never transcribed, CLAUDE.md rule 4): `LongOpenHashSet#add`,
//! `LongOpenHashSet$SetIterator`, `HashCommon#mix`, `HashCommon#arraySize`.
//!
//! # Why a hash set's iteration order is behaviour
//!
//! A chunk stores the structures that reach it as
//! `Map<Structure, LongSet>`, and the set is a `LongOpenHashSet` built by
//! `ChunkAccess#addReferenceForStructure`. `StructureManager#startsForStructure`
//! then walks that set and hands each start to `placeInChunk` **in iteration
//! order**, and every start shares one `RandomSource`. So when two mineshafts
//! reach the same chunk, which one goes first decides every random value the
//! second one sees.
//!
//! Iterating them in our own order instead was worth 1,344 blocks at seed
//! 31415926 — see the mineshaft section of `docs/BENCH.md`. This is the same
//! class of fault as [`crate::java_hash_set`], and it is here for the same
//! reason: the container's order is not an implementation detail once a
//! seeded stream is threaded through it.
//!
//! # What is reproduced, and what is refused
//!
//! Open addressing with linear probing, fastutil's `mix`, and its iterator,
//! which walks the table **downwards** from the top and returns the zero key
//! first if present. Growth is **refused** rather than guessed: `rehash`
//! re-inserts from the old table top-down, which is a second order-sensitive
//! path that nothing here has ever exercised, and a wrong guess would be
//! invisible. A refusal is loud.

/// `HashCommon.LONG_PHI`.
const LONG_PHI: u64 = 0x9E37_79B9_7F4A_7C15;

/// `HashCommon.mix(long)`.
fn mix(x: i64) -> u64 {
    let mut h = (x as u64).wrapping_mul(LONG_PHI);
    h ^= h >> 32;
    h ^ (h >> 16)
}

/// `HashCommon.arraySize(expected, f)` for the default `(16, 0.75f)`.
///
/// `ceil(16 / 0.75) = 22`, and the next power of two is 32. Spelled out
/// rather than computed because the no-arg constructor is the only one
/// vanilla uses here, and a general `arraySize` would be untested code.
const DEFAULT_N: usize = 32;
/// `maxFill = ceil(n * f)` — 24 for `n = 32`.
const DEFAULT_MAX_FILL: usize = 24;

/// The subset of the set this port needs: add, then read the order out.
pub struct LongOpenHashSet {
    key: Vec<i64>,
    mask: u32,
    contains_zero: bool,
    size: usize,
}

impl Default for LongOpenHashSet {
    fn default() -> Self {
        Self::new()
    }
}

impl LongOpenHashSet {
    /// `new LongOpenHashSet()`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            key: vec![0; DEFAULT_N + 1],
            mask: u32::try_from(DEFAULT_N - 1).expect("32 fits"),
            contains_zero: false,
            size: 0,
        }
    }

    /// `add(long)`.
    ///
    /// # Errors
    /// If the set would grow past `maxFill`, which would rehash. See the
    /// module docs on why that is refused rather than approximated.
    pub fn add(&mut self, k: i64) -> Result<bool, String> {
        if k == 0 {
            if self.contains_zero {
                return Ok(false);
            }
            self.contains_zero = true;
        } else {
            // `(int) mix(k) & mask` — the long is truncated to an int first,
            // and only then masked.
            let mut pos = ((mix(k) as u32) & self.mask) as usize;
            if self.key[pos] != 0 {
                if self.key[pos] == k {
                    return Ok(false);
                }
                loop {
                    pos = (pos + 1) & self.mask as usize;
                    if self.key[pos] == 0 {
                        break;
                    }
                    if self.key[pos] == k {
                        return Ok(false);
                    }
                }
            }
            self.key[pos] = k;
        }
        self.size += 1;
        if self.size >= DEFAULT_MAX_FILL {
            return Err(format!(
                "a LongOpenHashSet reached {} entries — fastutil would rehash \
                 and this port does not reproduce the resulting order",
                self.size
            ));
        }
        Ok(true)
    }

    /// The iteration order of `SetIterator`.
    ///
    /// The zero key first when present — the iterator's `mustReturnNull`
    /// branch is checked before the table walk — then the table from the top
    /// index downwards.
    #[must_use]
    pub fn iter_order(&self) -> Vec<i64> {
        let mut out = Vec::with_capacity(self.size);
        if self.contains_zero {
            out.push(0);
        }
        for pos in (0..=self.mask as usize).rev() {
            if self.key[pos] != 0 {
                out.push(self.key[pos]);
            }
        }
        out
    }
}

/// `ChunkPos.asLong(x, z)` — z in the high word, x in the low, each as an
/// unsigned 32-bit half.
#[must_use]
pub const fn chunk_pos_as_long(x: i32, z: i32) -> i64 {
    ((z as u32 as u64) << 32 | (x as u32 as u64)) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The order is not insertion order and not sorted order — if it were
    /// either, this whole module would be unnecessary, so the test says so.
    #[test]
    fn iteration_is_neither_insertion_nor_sorted_order() {
        let keys: Vec<i64> = (0..6)
            .map(|i| chunk_pos_as_long(57 + i, 65 + i * 2))
            .collect();
        let mut set = LongOpenHashSet::new();
        for k in &keys {
            set.add(*k).expect("small set");
        }
        let order = set.iter_order();

        assert_eq!(order.len(), keys.len(), "every key comes back once");
        let mut sorted_in = keys.clone();
        sorted_in.sort_unstable();
        let mut sorted_out = order.clone();
        sorted_out.sort_unstable();
        assert_eq!(sorted_in, sorted_out, "same multiset");

        assert_ne!(order, keys, "not insertion order");
        assert_ne!(order, sorted_in, "not ascending order");
    }

    /// Re-adding a key changes nothing, including the order.
    #[test]
    fn adding_twice_is_a_no_op() {
        let mut set = LongOpenHashSet::new();
        assert!(set.add(chunk_pos_as_long(3, 4)).expect("ok"));
        let first = set.iter_order();
        assert!(!set.add(chunk_pos_as_long(3, 4)).expect("ok"), "already in");
        assert_eq!(set.iter_order(), first);
    }

    /// The zero key is the one the iterator special-cases, and it comes
    /// first. `ChunkPos.asLong(0, 0)` is zero, so chunk (0, 0) really does
    /// take this path — it is not a hypothetical.
    #[test]
    fn the_zero_key_comes_first() {
        let mut set = LongOpenHashSet::new();
        set.add(chunk_pos_as_long(5, 9)).expect("ok");
        set.add(chunk_pos_as_long(0, 0)).expect("ok");
        set.add(chunk_pos_as_long(-3, 2)).expect("ok");
        assert_eq!(set.iter_order()[0], 0, "chunk (0, 0) sorts to the front");
    }

    /// Growth is refused, loudly, rather than approximated.
    #[test]
    fn growing_past_max_fill_refuses() {
        let mut set = LongOpenHashSet::new();
        let mut last = Ok(true);
        for i in 0..40 {
            last = set.add(chunk_pos_as_long(i, i * 7));
            if last.is_err() {
                break;
            }
        }
        assert!(last.is_err(), "should refuse before rehashing");
    }
}
