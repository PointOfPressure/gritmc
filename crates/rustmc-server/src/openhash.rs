//! `Int2ObjectOpenHashMap`, ported for its **walk order**.
//!
//! Reference: `it.unimi.dsi.fastutil.ints.Int2ObjectOpenHashMap`,
//! `it.unimi.dsi.fastutil.HashCommon` and
//! `Int2ObjectOpenHashMap$MapIterator`, read with `javap -p -c -constants`
//! from `fastutil-8.5.18.jar` — the fastutil the pinned 26.2 server jar itself
//! ships against, not whatever version a crate registry would hand us.
//!
//! # Why a container is being ported at all
//!
//! `ChunkMap.entityMap` is a plain `Int2ObjectOpenHashMap` — `ChunkMap` has
//! four map fields and the other three are `Long2ObjectLinkedOpenHashMap` —
//! and `ChunkMap.tick()` walks its `values()` into
//! `TrackedEntity.updatePlayers` and `ServerEntity.sendChanges`. So the order
//! every per-entity packet leaves the server in, within one tick, is a hash of
//! the entity id values. Roadmap §101.5 measured that against the shipped
//! fastutil and found 21 of 41 id sets walking differently from our ascending
//! `BTreeMap`; §103 is the port.
//!
//! `Entity.hashCode()` returns `getId()` and `equals` is id equality, so the
//! same walk also orders `PlayerMap.players`, an
//! `Object2BooleanOpenHashMap<ServerPlayer>` whose `keySet` `ChunkMap.tick`
//! walks for `updateChunkTracking`. One walk, two of the four order classes in
//! the census.
//!
//! # This is a port and not an approximation, and the difference is the point
//!
//! §101 refused to reimplement the hash on the ground that "a hash written
//! from memory that is right for {1,2} and wrong for {1,2,7} is worse than no
//! hash". That is the right refusal, and agreeing with a golden table of 41
//! sets does not answer it: a walk order is a function of **ten** separate
//! things and a plausible reimplementation can get nine of them right.
//!
//! `ci/fastutil-open-hash-walk.py` reads all ten out of the jar's bytecode and
//! pins them. Each is named below at the code that reproduces it, with the pin
//! key in the comment, so a parameter that moves in the jar has exactly one
//! place here to be wrong:
//!
//! * `1_mix` — [`mix`]
//! * `2_next_power_of_two` — [`next_power_of_two`]
//! * `3_array_size` — [`array_size`]
//! * `4_max_fill` — [`max_fill`]
//! * `5_default_ctor` — [`IntOpenHashMap::new`]
//! * `6_probe` — [`IntOpenHashMap::find`]
//! * `7_growth` — [`IntOpenHashMap::insert`]
//! * `8_shrink` — [`IntOpenHashMap::remove`]
//! * `9_shift_keys` — [`IntOpenHashMap::shift_keys`]
//! * `10_iterator` — [`IntOpenHashMap::walk_slots`]
//!
//! # The one that a golden table of key sets cannot see
//!
//! Nine of the ten decide the table's *state* and only the tenth decides the
//! walk over it. `8_shrink` is the one no insert-only measurement reaches:
//! `removeEntry` halves the table when the map has grown past its initial size
//! and then emptied out, so **the walk is a pure function of the key set only
//! at a fixed capacity**, and the capacity is a function of the whole
//! insert-and-remove history. §101.5's "pure function of the key set" holds for
//! every set it measured and not in general; §103.2 carries the correction.
//!
//! Linear probing is what makes the weaker statement true: for a fixed table
//! size the set of occupied slots and the key in each one is decided by the key
//! set alone, whatever order the keys arrived in — which is why {5,4,3,2,1} and
//! {1,2,3,4,5} walk identically — and `shiftKeys` is Knuth's algorithm R, which
//! leaves no tombstone and so preserves that invariant across removals.
//!
//! # What is deliberately not here
//!
//! Every method fastutil has that this server does not call. This is not a
//! general-purpose map and must not grow into one: `trim`, the bulk
//! constructors, the entry-set views and the fail-fast iterator's `wrapped`
//! list are all absent, and `wrapped` in particular is absent for a reason
//! worth stating — it only ever fires when an entry is removed *during* an
//! iteration, which is a shape this server's borrow rules make unwritable.

use std::fmt;

/// `HashCommon.INT_PHI`. Pin key `1_mix`.
const INT_PHI: i32 = -1_640_531_527;

/// `HashCommon.DEFAULT_LOAD_FACTOR`, as the no-argument constructor passes it.
/// Pin key `5_default_ctor`.
const DEFAULT_LOAD_FACTOR: f32 = 0.75;

/// `Int2ObjectOpenHashMap.DEFAULT_INITIAL_SIZE`, ditto. Pin key
/// `5_default_ctor`.
const DEFAULT_EXPECTED: i32 = 16;

/// `HashCommon.mix(int)`. Pin key `1_mix`.
///
/// ```java
/// final int h = k * INT_PHI;
/// return h ^ (h >>> 16);
/// ```
///
/// `>>>` is Java's *logical* shift, so the `as u32` is load-bearing: an
/// arithmetic shift here would agree for every non-negative product and
/// disagree for about half of all keys, which is the shape of bug that passes
/// a small golden table.
#[must_use]
pub const fn mix(k: i32) -> i32 {
    let h = k.wrapping_mul(INT_PHI);
    h ^ ((h as u32) >> 16) as i32
}

/// `HashCommon.nextPowerOfTwo(long)`. Pin key `2_next_power_of_two`.
///
/// ```java
/// return 1L << (64 - Long.numberOfLeadingZeros(x - 1));
/// ```
///
/// Java's shift takes its distance mod 64, so `x = 0` gives `1L << 0`. That is
/// reproduced rather than corrected: this function's job is to be the same
/// function, and a caller that would reach it is refused in [`array_size`].
#[must_use]
pub const fn next_power_of_two(x: i64) -> i64 {
    let d = (64 - x.wrapping_sub(1).leading_zeros()) % 64;
    1i64 << d
}

/// `HashCommon.arraySize(int, float)`. Pin key `3_array_size`.
///
/// ```java
/// final long s = Math.max(2, HashCommon.nextPowerOfTwo((long)Math.ceil(expected / f)));
/// if (s > (1 << 30)) throw new IllegalArgumentException(...);
/// return (int)s;
/// ```
///
/// # Panics
/// Above 2^30 entries, where fastutil throws. Reached only by a level holding
/// a billion entities, and a silent wrong answer there would be worse than a
/// panic.
#[must_use]
pub fn array_size(expected: i32, f: f32) -> usize {
    let ceil = (f64::from(expected) / f64::from(f)).ceil() as i64;
    let s = next_power_of_two(ceil).max(2);
    assert!(
        s <= 1 << 30,
        "openhash: table would exceed fastutil's 2^30 ceiling"
    );
    s as usize
}

/// `HashCommon.maxFill(int, float)`. Pin key `4_max_fill`.
///
/// ```java
/// return Math.min((int)Math.ceil(n * f), n - 1);
/// ```
#[must_use]
pub fn max_fill(n: usize, f: f32) -> usize {
    let ceil = (n as f64 * f64::from(f)).ceil() as usize;
    ceil.min(n - 1)
}

/// A map from `i32` to `V` that walks in `Int2ObjectOpenHashMap`'s order.
///
/// The public surface is deliberately small and deliberately *not*
/// `BTreeMap`-shaped: there is no ordered-range API here, because there is no
/// order to range over that means anything. What it offers is the walk, and
/// [`IntOpenHashMap::values`] is the method that carries the parity claim.
pub struct IntOpenHashMap<V> {
    /// `key[]`, length `n + 1`. Slot `n` is the zero key's, and `0` is the
    /// empty marker — which is sound here for the same reason it is in
    /// fastutil, and is why the zero key needs [`Self::contains_zero_key`] to
    /// exist at all.
    key: Vec<i32>,
    /// `value[]`, length `n + 1`, `None` where the slot is free.
    value: Vec<Option<V>>,
    /// `n`: the table size, always a power of two.
    n: usize,
    /// `mask`: `n - 1`.
    mask: usize,
    /// `maxFill`: the size at which the next insert grows the table.
    max_fill: usize,
    /// `minN`: the size the constructor chose, and the floor
    /// [`Self::remove`]'s shrink will not go below. Note that `rehash` does
    /// **not** move this, which is why a map that grows and shrinks returns to
    /// its original table size and not to the smallest that would fit.
    min_n: usize,
    /// `f`: the load factor.
    f: f32,
    /// `containsNullKey`. fastutil calls the zero key "null" because the
    /// primitive map is generated from the object one; the name is kept
    /// literal in the pin and plain here.
    contains_zero_key: bool,
    /// `size`.
    size: usize,
}

impl<V> IntOpenHashMap<V> {
    /// `new Int2ObjectOpenHashMap<>()` — which is exactly how
    /// `ChunkMap.entityMap` and `PlayerMap.players` are both built, checked
    /// from the bytecode by `ci/fastutil-open-hash-walk.py`'s consumer reader
    /// rather than assumed.
    ///
    /// Pin key `5_default_ctor`: expected 16, load factor 0.75, so
    /// `n = arraySize(16, 0.75) = nextPowerOfTwo(ceil(21.33)) = 32`,
    /// `maxFill = min(24, 31) = 24`.
    #[must_use]
    pub fn new() -> Self {
        Self::with_expected(DEFAULT_EXPECTED, DEFAULT_LOAD_FACTOR)
    }

    /// `Int2ObjectOpenHashMap(int expected, float f)`.
    ///
    /// # Panics
    /// On a load factor outside `(0, 1)` or a negative expected count, where
    /// fastutil throws `IllegalArgumentException`.
    #[must_use]
    pub fn with_expected(expected: i32, f: f32) -> Self {
        assert!(
            f > 0.0 && f < 1.0,
            "openhash: load factor must be in (0, 1)"
        );
        assert!(expected >= 0, "openhash: expected must be non-negative");
        let n = array_size(expected, f);
        Self {
            key: vec![0; n + 1],
            value: (0..=n).map(|_| None).collect(),
            n,
            mask: n - 1,
            max_fill: max_fill(n, f),
            min_n: n,
            f,
            contains_zero_key: false,
            size: 0,
        }
    }

    /// The slot a key wants, before probing. Pin key `6_probe`.
    ///
    /// Java's `mix(k) & mask` on a signed `int`; the `as u32` reproduces the
    /// two's-complement bit pattern the mask is applied to.
    fn slot(&self, k: i32) -> usize {
        (mix(k) as u32 as usize) & self.mask
    }

    /// `find(int)`. Pin key `6_probe`: first slot `mix(k) & mask`, then a
    /// linear `+1 & mask` step, with the zero key living at slot `n`.
    ///
    /// `Ok(pos)` where the key is, `Err(pos)` at the free slot an insert would
    /// take — fastutil returns `-(pos + 1)` for the second case and this is
    /// that, in the shape Rust reads.
    fn find(&self, k: i32) -> Result<usize, usize> {
        if k == 0 {
            return if self.contains_zero_key {
                Ok(self.n)
            } else {
                Err(self.n)
            };
        }
        let mut pos = self.slot(k);
        loop {
            let curr = self.key[pos];
            if curr == 0 {
                return Err(pos);
            }
            if curr == k {
                return Ok(pos);
            }
            pos = (pos + 1) & self.mask;
        }
    }

    /// Read one entry.
    #[must_use]
    pub fn get(&self, k: i32) -> Option<&V> {
        match self.find(k) {
            Ok(pos) => self.value[pos].as_ref(),
            Err(_) => None,
        }
    }

    /// Read one entry for mutation. The map's shape does not change, so no
    /// walk order changes either.
    pub fn get_mut(&mut self, k: i32) -> Option<&mut V> {
        match self.find(k) {
            Ok(pos) => self.value[pos].as_mut(),
            Err(_) => None,
        }
    }

    #[must_use]
    pub fn contains_key(&self, k: i32) -> bool {
        self.find(k).is_ok()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.size
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.size == 0
    }

    /// `put(int, V)`, returning the displaced value.
    ///
    /// Pin key `7_growth`, and the exact form matters:
    ///
    /// ```java
    /// if (size++ >= maxFill) rehash(arraySize(size + 1, f));
    /// ```
    ///
    /// The **test** reads the size before the increment and the **target**
    /// reads it after, so the 25th insert into a default map — size 24 before,
    /// 25 after — grows to `arraySize(26, 0.75) = 64`. Reading `size` on the
    /// same side of the `++` in both places is a one-character error that
    /// changes the table size at exactly one boundary, and therefore changes
    /// the walk only for maps of a particular size. That is precisely the
    /// class of divergence this port exists to not have.
    pub fn insert(&mut self, k: i32, v: V) -> Option<V> {
        match self.find(k) {
            Ok(pos) => self.value[pos].replace(v),
            Err(pos) => {
                if pos == self.n {
                    self.contains_zero_key = true;
                }
                self.key[pos] = k;
                self.value[pos] = Some(v);
                let before = self.size;
                self.size += 1;
                if before >= self.max_fill {
                    let target = array_size(
                        i32::try_from(self.size + 1)
                            .expect("openhash: size beyond i32, far past fastutil's own ceiling"),
                        self.f,
                    );
                    self.rehash(target);
                }
                None
            }
        }
    }

    /// The entry for `k`, inserting one if there is none.
    ///
    /// `BTreeMap::entry(..).or_insert_with(..)` in one call, and the two
    /// lookups are deliberate: [`Self::insert`] can rehash, which moves every
    /// slot, so a position taken before the insert cannot be reused after it.
    pub fn get_or_insert_with(&mut self, k: i32, f: impl FnOnce() -> V) -> &mut V {
        if self.find(k).is_err() {
            self.insert(k, f());
        }
        let pos = self
            .find(k)
            .expect("openhash: the key just inserted is not present");
        self.value[pos]
            .as_mut()
            .expect("openhash: an occupied slot holds a value")
    }

    /// `remove(int)`.
    ///
    /// Pin key `8_shrink`:
    ///
    /// ```java
    /// if (n > minN && size < maxFill / 4 && n > 16) rehash(n / 2);
    /// ```
    ///
    /// This is the input a golden table of key sets cannot see, because it
    /// makes the table size — and so the walk — depend on the history and not
    /// only on the survivors. `removeEntry` and `removeNullEntry` carry the
    /// identical test; both are reproduced here by the one path.
    pub fn remove(&mut self, k: i32) -> Option<V> {
        let pos = self.find(k).ok()?;
        let out = self.value[pos].take();
        self.size -= 1;
        if pos == self.n {
            self.contains_zero_key = false;
            self.key[self.n] = 0;
        } else {
            self.shift_keys(pos);
        }
        if self.n > self.min_n && self.size < self.max_fill / 4 && self.n > 16 {
            self.rehash(self.n / 2);
        }
        out
    }

    /// `shiftKeys(int)` — Knuth's algorithm R. Pin key `9_shift_keys`.
    ///
    /// A remove leaves no tombstone: the run of keys after the hole is pulled
    /// back over it, skipping any key that would then sit before its own home
    /// slot. That is what keeps the table's contents a function of the key set
    /// rather than of the order things were deleted, and so is what keeps
    /// {5,4,3,2,1} walking the same as {1,2,3,4,5} after a removal as before
    /// one.
    ///
    /// The wrap-aware break condition is transcribed from the bytecode's two
    /// branches rather than from the source's one-line ternary, because the
    /// ternary is easy to read as symmetric and it is not: the `last <= pos`
    /// arm is an `or` and the `last > pos` arm is an `and`.
    fn shift_keys(&mut self, mut pos: usize) {
        loop {
            let last = pos;
            pos = (pos + 1) & self.mask;
            let curr = loop {
                let curr = self.key[pos];
                if curr == 0 {
                    self.key[last] = 0;
                    self.value[last] = None;
                    return;
                }
                let slot = self.slot(curr);
                let stop = if last <= pos {
                    last >= slot || slot > pos
                } else {
                    last >= slot && slot > pos
                };
                if stop {
                    break curr;
                }
                pos = (pos + 1) & self.mask;
            };
            self.key[last] = curr;
            self.value[last] = self.value[pos].take();
        }
    }

    /// `realSize()`: the entries outside slot `n`.
    fn real_size(&self) -> usize {
        self.size - usize::from(self.contains_zero_key)
    }

    /// `rehash(int)`.
    ///
    /// Walks the old table **downwards** from `n` and reinserts each key by
    /// linear probing. The direction is transcribed for fidelity and is not
    /// what decides the result: linear probing puts the same keys in the same
    /// slots whatever order they arrive in, which is the property the whole
    /// port leans on. Note what this does *not* touch — `min_n` — which is why
    /// a map that grew to 64 and emptied comes back to 32 rather than to 2.
    fn rehash(&mut self, new_n: usize) {
        let new_mask = new_n - 1;
        let mut new_key = vec![0i32; new_n + 1];
        let mut new_value: Vec<Option<V>> = (0..=new_n).map(|_| None).collect();
        let mut i = self.n;
        let mut j = self.real_size();
        while j != 0 {
            j -= 1;
            loop {
                i -= 1;
                if self.key[i] != 0 {
                    break;
                }
            }
            let k = self.key[i];
            let mut pos = (mix(k) as u32 as usize) & new_mask;
            if new_key[pos] != 0 {
                loop {
                    pos = (pos + 1) & new_mask;
                    if new_key[pos] == 0 {
                        break;
                    }
                }
            }
            new_key[pos] = k;
            new_value[pos] = self.value[i].take();
        }
        new_value[new_n] = self.value[self.n].take();
        self.n = new_n;
        self.mask = new_mask;
        self.max_fill = max_fill(new_n, self.f);
        self.key = new_key;
        self.value = new_value;
    }

    /// The slot indices, in the order `MapIterator` hands them out. Pin key
    /// `10_iterator`.
    ///
    /// ```java
    /// pos = n; mustReturnNullKey = containsNullKey;
    /// // nextEntry: the null key first, from slot n; then
    /// while (key[--pos] == 0);
    /// ```
    ///
    /// So: the zero key first if it is present, then the occupied slots in
    /// **descending** index order. Descending is the single fact here that
    /// nothing else in this file implies, and it is the reason {1,2} walks
    /// `2 1`: 2 takes a higher slot than 1 under `mix`, and higher slots come
    /// out first.
    fn walk_slots(&self) -> Vec<usize> {
        let mut out = Vec::with_capacity(self.size);
        if self.contains_zero_key {
            out.push(self.n);
        }
        let mut pos = self.n;
        while pos > 0 {
            pos -= 1;
            if self.key[pos] != 0 {
                out.push(pos);
            }
        }
        out
    }

    /// The keys, in walk order.
    pub fn keys(&self) -> impl Iterator<Item = i32> + '_ {
        self.walk_slots().into_iter().map(|p| self.key[p])
    }

    /// The values, in walk order.
    ///
    /// **This is the method the parity claim is about.** `ChunkMap.tick()`
    /// walks `entityMap.values()`, so this is the order per-entity packets
    /// leave the server in within one tick.
    pub fn values(&self) -> impl Iterator<Item = &V> + '_ {
        self.walk_slots()
            .into_iter()
            .filter_map(move |p| self.value[p].as_ref())
    }

    /// The entries, in walk order.
    pub fn iter(&self) -> impl Iterator<Item = (i32, &V)> + '_ {
        self.walk_slots()
            .into_iter()
            .filter_map(move |p| self.value[p].as_ref().map(|v| (self.key[p], v)))
    }

    /// Every entry, in walk order, for mutation in place.
    ///
    /// A closure rather than an `iter_mut`, and the reason is worth one line:
    /// handing out `&mut V` from a `Vec<Option<V>>` by slot index needs either
    /// `unsafe` or a chain of `split_at_mut`, and a container written to settle
    /// an order question should not also be the place this tree acquires a
    /// pointer cast. Mutating a value cannot move a slot, so the order here is
    /// the order [`Self::iter`] gives.
    pub fn for_each_mut(&mut self, mut f: impl FnMut(i32, &mut V)) {
        for p in self.walk_slots() {
            let k = self.key[p];
            if let Some(v) = self.value[p].as_mut() {
                f(k, v);
            }
        }
    }
}

impl<V> Default for IntOpenHashMap<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V: fmt::Debug> fmt::Debug for IntOpenHashMap<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl<V: Clone> Clone for IntOpenHashMap<V> {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            value: self.value.clone(),
            n: self.n,
            mask: self.mask,
            max_fill: self.max_fill,
            min_n: self.min_n,
            f: self.f,
            contains_zero_key: self.contains_zero_key,
            size: self.size,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn walk(ids: &[i32]) -> Vec<i32> {
        let mut m = IntOpenHashMap::new();
        for &i in ids {
            m.insert(i, i);
        }
        m.keys().collect()
    }

    /// Replay every measured history and require the identical walk.
    ///
    /// **This is the test the port stands on**, and the reason it replays
    /// histories rather than key sets is the whole difference between a port
    /// and a reimplementation. §101 was right that agreeing with a golden
    /// table of 41 key sets proves very little: nine of the ten inputs
    /// `ci/fastutil-open-hash-walk.py` pins decide the table's *state*, and an
    /// insert-only corpus at one size cannot separate them. The corpus here
    /// grows past three table boundaries and drains back through the shrink
    /// rule, which is the input no set-shaped measurement can reach.
    ///
    /// The golden is measured from the fastutil the pinned server jar ships
    /// with, by `ci/probes/WalkHistory.java`, and carries that jar's sha256 in
    /// its header. If the file is missing this test **fails** rather than
    /// skipping: a test that quietly passes when its fixture is gone is the
    /// defect class this tree files as unfalsifiable.
    #[test]
    fn agrees_with_the_measured_vanilla_walks() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../ci/checks/fastutil-walk-history-golden.txt"
        );
        let text = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("the measured walk golden is unreadable at {path}: {e}"));

        let mut checked = 0usize;
        let mut permuted = 0usize;
        let mut with_removes = 0usize;
        let mut drained = 0usize;
        for line in text.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            let (ops, rest) = line
                .split_once('|')
                .unwrap_or_else(|| panic!("golden line has no `|`: {line}"));
            // The third field is the same walk out of an object-keyed map,
            // measured to check that `PlayerMap`'s container agrees with
            // `ChunkMap.entityMap`'s. `ci/fastutil-open-hash-walk.py` refuses
            // if the two ever disagree, so this side only needs the first.
            let want = rest.split('|').next().unwrap_or(rest);
            let want: Vec<i32> = want
                .split_whitespace()
                .map(|s| s.parse().expect("golden walk holds a non-integer"))
                .collect();

            let mut m: IntOpenHashMap<i32> = IntOpenHashMap::new();
            let mut puts = 0usize;
            let mut removes = 0usize;
            for op in ops.split_whitespace() {
                let k: i32 = op[1..].parse().expect("golden op holds a non-integer");
                match op.as_bytes()[0] {
                    b'+' => {
                        m.insert(k, k);
                        puts += 1;
                    }
                    b'-' => {
                        m.remove(k);
                        removes += 1;
                    }
                    _ => panic!("golden op is neither a put nor a remove: {op}"),
                }
            }
            let got: Vec<i32> = m.keys().collect();
            assert_eq!(got, want, "walk diverged on history `{ops}`");

            checked += 1;
            if want != sorted(&want) {
                permuted += 1;
            }
            if removes > 0 {
                with_removes += 1;
            }
            if puts > 24 && want.len() < 8 {
                drained += 1;
            }
        }

        // The same floors `ci/fastutil-open-hash-walk.py` applies to the file
        // it writes, restated here so this test cannot be made vacuous by
        // handing it a shorter one. A replay over an empty corpus passes.
        assert!(checked >= 900, "only {checked} histories replayed");
        assert!(
            permuted >= checked / 4,
            "only {permuted} of {checked} walks are a real permutation; a map \
             that walked ascending would look like this"
        );
        assert!(
            with_removes >= 200,
            "only {with_removes} histories exercise removal"
        );
        assert!(
            drained >= 30,
            "only {drained} histories reach the shrink rule, which is the one \
             input a key-set corpus cannot see"
        );
    }

    fn sorted(v: &[i32]) -> Vec<i32> {
        let mut s = v.to_vec();
        s.sort_unstable();
        s
    }

    #[test]
    fn the_four_rows_from_the_roadmap() {
        // §101.5's table, which was measured from the shipped fastutil.
        assert_eq!(walk(&[1, 2]), vec![2, 1]);
        assert_eq!(walk(&[1, 2, 3]), vec![2, 1, 3]);
        assert_eq!(walk(&[1, 3]), vec![1, 3]);
        assert_eq!(walk(&[5, 4, 3, 2, 1]), vec![2, 4, 1, 3, 5]);
    }

    #[test]
    fn insertion_order_does_not_reach_the_walk() {
        // The property linear probing gives and a linked map does not. This is
        // why {5,4,3,2,1} and {1,2,3,4,5} are one row and not two.
        assert_eq!(walk(&[5, 4, 3, 2, 1]), walk(&[1, 2, 3, 4, 5]));
        assert_eq!(walk(&[3, 1, 2]), walk(&[1, 2, 3]));
    }

    #[test]
    fn the_table_starts_at_thirty_two_and_grows_at_the_twenty_fifth() {
        let mut m: IntOpenHashMap<i32> = IntOpenHashMap::new();
        assert_eq!(m.n, 32);
        assert_eq!(m.mask, 31);
        assert_eq!(m.max_fill, 24);
        assert_eq!(m.min_n, 32);
        for i in 1..=24 {
            m.insert(i, i);
            assert_eq!(m.n, 32, "grew early, at {i}");
        }
        m.insert(25, 25);
        assert_eq!(m.n, 64, "the 25th insert must grow the table");
        assert_eq!(m.max_fill, 48);
    }

    #[test]
    fn removal_shrinks_only_back_to_the_constructed_size() {
        // Pin key `8_shrink`, and the reason §101.5's "pure function of the
        // key set" needed the correction in §103.2: these two maps hold the
        // same key and walk it from different table sizes.
        let mut grown: IntOpenHashMap<i32> = IntOpenHashMap::new();
        for i in 1..=200 {
            grown.insert(i, i);
        }
        let big = grown.n;
        assert!(big > 32, "200 entries should have grown past 32, got {big}");
        for i in 2..=200 {
            grown.remove(i);
        }
        assert_eq!(grown.len(), 1);
        assert_eq!(
            grown.n, 32,
            "shrink floors at minN, not at the smallest fit"
        );
    }

    #[test]
    fn a_removed_key_leaves_no_trace_in_the_walk() {
        // Algorithm R's contract: after removing a key the table is exactly
        // what inserting the survivors alone would have built, provided the
        // capacity did not move.
        for victim in 1..=8 {
            let mut m = IntOpenHashMap::new();
            for i in 1..=8 {
                m.insert(i, i);
            }
            m.remove(victim);
            let survivors: Vec<i32> = (1..=8).filter(|&i| i != victim).collect();
            assert_eq!(
                m.keys().collect::<Vec<_>>(),
                walk(&survivors),
                "removing {victim} left a table a plain insert would not build"
            );
        }
    }

    #[test]
    fn the_zero_key_comes_out_first_and_lives_at_slot_n() {
        // No entity is ever id 0 — `Entity.getId` throws on one and our
        // allocator starts at 1 — so this is fidelity rather than a live path.
        // It is tested because an implementation that quietly cannot hold 0 is
        // a different container, and the difference would surface as a panic
        // in whichever future path first tried.
        let mut m = IntOpenHashMap::new();
        m.insert(3, 3);
        m.insert(0, 0);
        m.insert(1, 1);
        let ks: Vec<i32> = m.keys().collect();
        assert_eq!(ks[0], 0, "the zero key is returned before the table walk");
        assert_eq!(ks.len(), 3);
        assert_eq!(m.get(0), Some(&0));
        assert_eq!(m.remove(0), Some(0));
        assert_eq!(m.keys().collect::<Vec<_>>(), vec![1, 3]);
    }

    #[test]
    fn mix_uses_a_logical_shift() {
        // The `as u32` in `mix`. A key whose product is negative is where an
        // arithmetic shift would diverge, and a table of small positive ids
        // would never show it.
        assert_eq!(
            mix(1),
            -1_640_531_527i32 ^ ((-1_640_531_527i32 as u32) >> 16) as i32
        );
        assert_ne!(mix(-1), 0);
        // Ids are unique, so slots must not be either — but the map must
        // survive a full collision run regardless.
        let mut m = IntOpenHashMap::new();
        for i in [-1, i32::MIN, i32::MAX, 1, 2] {
            m.insert(i, i);
        }
        assert_eq!(m.len(), 5);
        for i in [-1, i32::MIN, i32::MAX, 1, 2] {
            assert_eq!(m.get(i), Some(&i));
        }
    }

    #[test]
    fn for_each_mut_visits_every_entry_once_in_walk_order() {
        let mut m = IntOpenHashMap::new();
        for i in 1..=10 {
            m.insert(i, i);
        }
        let order: Vec<i32> = m.keys().collect();
        let mut seen = Vec::new();
        m.for_each_mut(|k, v| {
            seen.push(k);
            *v += 100;
        });
        assert_eq!(seen, order);
        assert_eq!(m.get(1), Some(&101));
        assert_eq!(m.len(), 10);
    }
}
