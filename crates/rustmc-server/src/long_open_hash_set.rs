//! `it.unimi.dsi.fastutil.longs.LongOpenHashSet` with the default
//! `(16, 0.75f)` constructor: add, remove, contains, growth, shrink, and the
//! order its **spliterator** walks the table.
//!
//! Ported from fastutil 8.5.18 as the 26.2 server ships it
//! (`oracle-work/vanilla-26.2/libraries/it/unimi/dsi/fastutil/8.5.18`), under
//! `javap -p -c -constants`: `LongOpenHashSet.<init>(IF)V`, `add(J)Z`,
//! `remove(J)Z`, `removeEntry(I)Z`, `removeNullEntry()Z`, `shiftKeys(I)V`,
//! `rehash(I)V`, `realSize()I`, `LongOpenHashSet$SetSpliterator.tryAdvance`
//! and `forEachRemaining`, and `HashCommon.mix(J)J`, `arraySize(IF)I`,
//! `maxFill(IF)I`, `nextPowerOfTwo(J)J`.
//!
//! # Why the order is behaviour
//!
//! `PlayerChunkSender.pendingChunks` is one of these, and
//! `collectChunksToSend` streams it into a comparator that looks only at
//! `ChunkPos.distanceSquared`. Every chunk at an equal distance — and a view
//! has dozens at each — goes out in whatever order this table yields them.
//! So the table's layout, including the history of growth and shrinking that
//! moved entries between slots, decides the order of the chunks on the wire.
//!
//! # The spliterator is not the iterator
//!
//! `LongCollection.longStream()` and `stream()` both come from
//! `SetSpliterator`, which returns the zero key **first** (if present) and
//! then walks slots **upwards** from 0. `SetIterator` walks them
//! downwards from the top. `collectChunksToSend` only ever streams, so only
//! the spliterator order is exposed here.

/// `HashCommon.LONG_PHI`, the constant `mix` multiplies by
/// (`-7046029254386353131L`).
const LONG_PHI: u64 = 0x9E37_79B9_7F4A_7C15;

/// `Hash.DEFAULT_INITIAL_SIZE`, which the no-arg constructor passes, and the
/// floor `removeEntry` will not shrink below.
const DEFAULT_INITIAL_SIZE: usize = 16;

/// `Hash.DEFAULT_LOAD_FACTOR`.
const LOAD_FACTOR: f32 = 0.75;

/// `HashCommon.mix(long)`: multiply by phi, then fold the high halves down.
/// Both shifts are `lushr` — logical.
#[must_use]
pub const fn mix(x: i64) -> i64 {
    let mut h = (x as u64).wrapping_mul(LONG_PHI);
    h ^= h >> 32;
    (h ^ (h >> 16)) as i64
}

/// `HashCommon.nextPowerOfTwo(long)`: `1L << (64 - nlz(x - 1))`, where a
/// shift of 64 is a shift of 0 in Java.
const fn next_power_of_two(x: i64) -> i64 {
    let shift = 64 - (x.wrapping_sub(1)).leading_zeros();
    1i64 << (shift & 63)
}

/// `HashCommon.arraySize(int, float)`: `max(2, nextPowerOfTwo(ceil(e / f)))`.
fn array_size(expected: usize, f: f32) -> usize {
    let need = (expected as f64 / f64::from(f)).ceil() as i64;
    let s = next_power_of_two(need).max(2);
    assert!(
        s <= 1 << 30,
        "Too large ({expected} expected elements with load factor {f})"
    );
    s as usize
}

/// `HashCommon.maxFill(int, float)`: `min(ceil(n * f), n - 1)`.
fn max_fill(n: usize, f: f32) -> usize {
    let fill = (n as f64 * f64::from(f)).ceil() as usize;
    fill.min(n - 1)
}

/// `LongOpenHashSet`, as `new LongOpenHashSet()` builds it.
#[derive(Debug, Clone)]
pub struct LongOpenHashSet {
    /// `key` — `n + 1` slots; slot `n` is the zero key's, and it is never
    /// read here because `contains_null` says whether it is occupied.
    key: Vec<i64>,
    /// `n`.
    n: usize,
    /// `mask`, `n - 1`.
    mask: usize,
    /// `maxFill`.
    max_fill: usize,
    /// `minN` — the size the constructor chose; the table never shrinks
    /// below it.
    min_n: usize,
    /// `size`, the zero key included.
    size: usize,
    /// `containsNull` — whether the key `0` is present.
    contains_null: bool,
}

impl Default for LongOpenHashSet {
    fn default() -> Self {
        Self::new()
    }
}

impl LongOpenHashSet {
    /// `new LongOpenHashSet()`, which is `this(16, 0.75f)`: `n = 32`,
    /// `maxFill = 24`.
    #[must_use]
    pub fn new() -> Self {
        let n = array_size(DEFAULT_INITIAL_SIZE, LOAD_FACTOR);
        Self {
            key: vec![0; n + 1],
            n,
            mask: n - 1,
            max_fill: max_fill(n, LOAD_FACTOR),
            min_n: n,
            size: 0,
            contains_null: false,
        }
    }

    /// `size()`.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.size
    }

    /// `isEmpty()`.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.size == 0
    }

    /// The table length `n`, for tests of growth and shrinking.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.n
    }

    /// `realSize()` — the count of keys that occupy an ordinary slot.
    const fn real_size(&self) -> usize {
        if self.contains_null {
            self.size - 1
        } else {
            self.size
        }
    }

    /// The home slot of a non-zero key.
    const fn home(&self, k: i64) -> usize {
        (mix(k) as i32 as usize) & self.mask
    }

    /// `add(long)`. Grows to `arraySize(size + 1, f)` once the size *before*
    /// the insert has reached `maxFill`.
    pub fn add(&mut self, k: i64) -> bool {
        if k == 0 {
            if self.contains_null {
                return false;
            }
            self.contains_null = true;
        } else {
            let mut pos = self.home(k);
            let mut curr = self.key[pos];
            if curr != 0 {
                if curr == k {
                    return false;
                }
                loop {
                    pos = (pos + 1) & self.mask;
                    curr = self.key[pos];
                    if curr == 0 {
                        break;
                    }
                    if curr == k {
                        return false;
                    }
                }
            }
            self.key[pos] = k;
        }
        let before = self.size;
        self.size += 1;
        if before >= self.max_fill {
            self.rehash(array_size(self.size + 1, LOAD_FACTOR));
        }
        true
    }

    /// `contains(long)`.
    #[must_use]
    pub fn contains(&self, k: i64) -> bool {
        if k == 0 {
            return self.contains_null;
        }
        let mut pos = self.home(k);
        loop {
            let curr = self.key[pos];
            if curr == 0 {
                return false;
            }
            if curr == k {
                return true;
            }
            pos = (pos + 1) & self.mask;
        }
    }

    /// `remove(long)`.
    pub fn remove(&mut self, k: i64) -> bool {
        if k == 0 {
            if self.contains_null {
                return self.remove_null_entry();
            }
            return false;
        }
        let mut pos = self.home(k);
        loop {
            let curr = self.key[pos];
            if curr == 0 {
                return false;
            }
            if curr == k {
                return self.remove_entry(pos);
            }
            pos = (pos + 1) & self.mask;
        }
    }

    /// The shrink test `removeEntry` and `removeNullEntry` share:
    /// `n > minN && size < maxFill / 4 && n > 16`, then `rehash(n / 2)`.
    fn maybe_shrink(&mut self) {
        if self.n > self.min_n && self.size < self.max_fill / 4 && self.n > DEFAULT_INITIAL_SIZE {
            self.rehash(self.n / 2);
        }
    }

    /// `removeEntry(int)`.
    fn remove_entry(&mut self, pos: usize) -> bool {
        self.size -= 1;
        self.shift_keys(pos);
        self.maybe_shrink();
        true
    }

    /// `removeNullEntry()`.
    fn remove_null_entry(&mut self) -> bool {
        self.contains_null = false;
        self.key[self.n] = 0;
        self.size -= 1;
        self.maybe_shrink();
        true
    }

    /// `shiftKeys(int)` — backward-shift deletion for linear probing.
    ///
    /// A key at `pos` moves into the hole at `last` unless its home `slot`
    /// lies cyclically in `(last, pos]`, in which case moving it would put it
    /// before its home and it stays.
    fn shift_keys(&mut self, mut pos: usize) {
        loop {
            let last = pos;
            pos = (last + 1) & self.mask;
            let curr = loop {
                let curr = self.key[pos];
                if curr == 0 {
                    self.key[last] = 0;
                    return;
                }
                let slot = self.home(curr);
                let moves = if last <= pos {
                    last >= slot || slot > pos
                } else {
                    last >= slot && slot > pos
                };
                if moves {
                    break curr;
                }
                pos = (pos + 1) & self.mask;
            };
            self.key[last] = curr;
        }
    }

    /// `rehash(int)`: re-insert every ordinary key from the old table into a
    /// new one of `new_n` slots, reading the old table **downwards** from
    /// slot `n - 1`. The zero key stays where it is, in `contains_null`.
    fn rehash(&mut self, new_n: usize) {
        let mask = new_n - 1;
        let mut new_key = vec![0i64; new_n + 1];
        let mut i = self.n;
        for _ in 0..self.real_size() {
            loop {
                i -= 1;
                if self.key[i] != 0 {
                    break;
                }
            }
            let k = self.key[i];
            let mut pos = (mix(k) as i32 as usize) & mask;
            while new_key[pos] != 0 {
                pos = (pos + 1) & mask;
            }
            new_key[pos] = k;
        }
        self.n = new_n;
        self.mask = mask;
        self.max_fill = max_fill(new_n, LOAD_FACTOR);
        self.key = new_key;
    }

    /// Every key in `SetSpliterator` order: the zero key first if present,
    /// then slots `0..n` ascending. This is the order `longStream()` and
    /// `stream()` both see.
    pub fn stream_order(&self) -> impl Iterator<Item = i64> + '_ {
        let null = self.contains_null.then_some(0i64);
        null.into_iter()
            .chain(self.key[..self.n].iter().copied().filter(|k| *k != 0))
    }
}

#[cfg(test)]
mod tests {
    use super::{array_size, max_fill, mix, next_power_of_two, LongOpenHashSet};

    /// Values computed from `HashCommon.mix` in the pinned fastutil jar's
    /// arithmetic by hand: `mix(1) = phi ^ phi>>>32 ^ ...`.
    #[test]
    fn mix_is_phi_then_two_logical_folds() {
        let phi = 0x9E37_79B9_7F4A_7C15_u64;
        let h = phi ^ (phi >> 32);
        assert_eq!(mix(1) as u64, h ^ (h >> 16));
        assert_eq!(mix(0), 0);
        // A logical shift, not an arithmetic one: a negative input must not
        // smear its sign bit into the fold.
        let m = (-1i64 as u64).wrapping_mul(phi);
        let m = m ^ (m >> 32);
        assert_eq!(mix(-1) as u64, m ^ (m >> 16));
    }

    #[test]
    fn the_default_table_is_32_slots_filling_to_24() {
        assert_eq!(array_size(16, 0.75), 32);
        assert_eq!(max_fill(32, 0.75), 24);
        assert_eq!(next_power_of_two(22), 32);
        assert_eq!(next_power_of_two(32), 32);
        assert_eq!(next_power_of_two(1), 1);
        let s = LongOpenHashSet::new();
        assert_eq!((s.capacity(), s.len()), (32, 0));
    }

    /// `add` tests the size *before* the insert against `maxFill`, so the
    /// 25th key grows the table, to `arraySize(26, .75) = 64`.
    #[test]
    fn the_twenty_fifth_key_grows_the_table() {
        let mut s = LongOpenHashSet::new();
        for k in 1..=24 {
            assert!(s.add(k));
        }
        assert_eq!(s.capacity(), 32);
        assert!(s.add(25));
        assert_eq!(s.capacity(), 64);
        assert!(!s.add(25), "a duplicate is refused");
        assert_eq!(s.len(), 25);
    }

    /// Shrinks halve the table once `size < maxFill / 4`, but never below the
    /// constructor's 32.
    #[test]
    fn removal_shrinks_back_to_min_n_and_no_further() {
        let mut s = LongOpenHashSet::new();
        for k in 1..=200 {
            s.add(k * 7919);
        }
        let grown = s.capacity();
        assert!(grown >= 256, "{grown}");
        for k in 1..=200 {
            assert!(s.remove(k * 7919));
        }
        assert_eq!(s.capacity(), 32);
        assert!(s.is_empty());
        assert!(!s.remove(7919));
    }

    #[test]
    fn the_zero_key_streams_first_and_counts_toward_size() {
        let mut s = LongOpenHashSet::new();
        s.add(5);
        s.add(0);
        s.add(-3);
        assert_eq!(s.len(), 3);
        assert_eq!(s.stream_order().next(), Some(0));
        assert!(s.contains(0));
        assert!(s.remove(0));
        assert!(!s.contains(0));
        assert_eq!(s.stream_order().count(), 2);
    }

    /// Backward-shift deletion must keep every surviving key findable. A
    /// broken `shiftKeys` either loses keys or leaves them past a hole.
    #[test]
    fn every_survivor_stays_findable_through_interleaved_removal() {
        let mut s = LongOpenHashSet::new();
        let keys: Vec<i64> = (0..300)
            .map(|i| (i * 2_654_435_761_i64) ^ (i << 40))
            .collect();
        for k in &keys {
            s.add(*k);
        }
        for (i, k) in keys.iter().enumerate() {
            if i % 3 != 0 {
                assert!(s.remove(*k));
            }
        }
        for (i, k) in keys.iter().enumerate() {
            assert_eq!(s.contains(*k), i % 3 == 0, "key {i}");
        }
        assert_eq!(s.stream_order().count(), s.len());
    }
}
