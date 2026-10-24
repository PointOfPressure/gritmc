//! `it.unimi.dsi.fastutil.shorts.ShortOpenHashSet`, reproduced for its
//! **iteration order**.
//!
//! # Why a hash set's internal order is a wire format
//!
//! `ChunkHolder.broadcastChanges` accumulates a tick's block changes per
//! section in a `ShortOpenHashSet` and, when there is more than one, hands
//! the set straight to `ClientboundSectionBlocksUpdatePacket`:
//!
//! ```java
//! ShortIterator it = set.iterator();
//! for (int i = 0; i < set.size(); i++) {
//!     short s = it.nextShort();
//!     this.positions[i] = s;
//!     this.states[i] = section.getBlockState(sectionRelativeX(s), ...);
//! }
//! ```
//!
//! The packet writes `positions` in that order. So the order two block
//! changes appear on the wire is the order fastutil's open-addressed table
//! happens to hold them in — not insertion order, and not sorted. A server
//! that sorted them, or kept them in insertion order, would send the same
//! *information* in different *bytes*, and this project compares bytes.
//!
//! Reproducing a third-party data structure is not a clean-room problem:
//! fastutil is not Mojang's code, and this is written from the published
//! algorithm rather than transcribed from anything.
//!
//! # The algorithm, and the three details that decide the order
//!
//! * **The table is `n = 32` slots by default and the key is placed at
//!   `mix(k) & (n - 1)` with linear probing forward.** `mix` is
//!   `HashCommon.mix`: multiply by `INT_PHI = 0x9E37_79B9`, then xor with
//!   the value shifted right 16.
//! * **Iteration runs *downward*, from `n - 1` to `0`.** Not upward. This
//!   alone reverses most two-element packets.
//! * **Zero is not stored in the table at all.** fastutil keeps a
//!   `containsNull` flag, and the iterator yields the zero key **first**,
//!   before it starts walking the table. `SectionPos.sectionRelativePos` is
//!   `0` for the block at the section's own corner, so this is reachable
//!   from a player standing in the wrong place, not a theoretical case.

/// `HashCommon.INT_PHI`.
const INT_PHI: u32 = 0x9E37_79B9;
/// `Hash.DEFAULT_LOAD_FACTOR`.
const LOAD_FACTOR: f64 = 0.75;
/// `Hash.DEFAULT_INITIAL_SIZE`.
const DEFAULT_EXPECTED: usize = 16;

/// `HashCommon.mix`.
const fn mix(x: i32) -> i32 {
    #[expect(
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap,
        reason = "Java's int arithmetic is wrapping and unsigned-shifts explicitly"
    )]
    {
        let h = (x as u32).wrapping_mul(INT_PHI);
        (h ^ (h >> 16)) as i32
    }
}

/// `HashCommon.arraySize`: the next power of two at or above
/// `ceil(expected / f)`, and never below 2.
fn array_size(expected: usize, f: f64) -> usize {
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "mirrors fastutil's own double arithmetic on small counts"
    )]
    let want = (expected as f64 / f).ceil() as usize;
    want.max(2).next_power_of_two()
}

/// `HashCommon.maxFill`: `ceil(n * f)`, capped at `n - 1`.
fn max_fill(n: usize, f: f64) -> usize {
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "mirrors fastutil's own double arithmetic on small counts"
    )]
    let m = (n as f64 * f).ceil() as usize;
    m.min(n.saturating_sub(1))
}

/// A `ShortOpenHashSet` with fastutil's layout, kept only so that iteration
/// order can be reproduced.
pub struct ShortOpenHashSet {
    /// The open-addressed table. `0` means empty, which is why the real zero
    /// key lives in `contains_zero` instead.
    key: Vec<i16>,
    mask: usize,
    contains_zero: bool,
    /// Number of keys, **including** the zero key if present.
    size: usize,
    max_fill: usize,
}

impl Default for ShortOpenHashSet {
    fn default() -> Self {
        Self::new()
    }
}

impl ShortOpenHashSet {
    /// `new ShortOpenHashSet()` — expected 16, load factor 0.75, so a
    /// 32-slot table.
    #[must_use]
    pub fn new() -> Self {
        let n = array_size(DEFAULT_EXPECTED, LOAD_FACTOR);
        Self {
            key: vec![0; n],
            mask: n - 1,
            contains_zero: false,
            size: 0,
            max_fill: max_fill(n, LOAD_FACTOR),
        }
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.size
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.size == 0
    }

    /// `add`. Returns whether the set changed.
    pub fn insert(&mut self, k: i16) -> bool {
        if k == 0 {
            if self.contains_zero {
                return false;
            }
            self.contains_zero = true;
        } else {
            let mut pos = (mix(i32::from(k)) as usize) & self.mask;
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
        // `if (size++ >= maxFill)`: the test reads the size from BEFORE the
        // add (`dup_x1` in the bytecode), so the 32-slot table takes its
        // 25th key before it grows, not its 24th. Growing one key early
        // reorders a 26-block `section_blocks_update` (scenario 129).
        let before = self.size;
        self.size += 1;
        if before >= self.max_fill {
            self.rehash(array_size(self.size + 1, LOAD_FACTOR));
        }
        true
    }

    /// `rehash`. fastutil walks the old table **downward** and reinserts, so
    /// the probe sequence for colliding keys depends on that walk and not on
    /// the original insertion order.
    fn rehash(&mut self, new_n: usize) {
        let new_mask = new_n - 1;
        let mut new_key = vec![0_i16; new_n];
        let mut i = self.key.len();
        // `realSize` is the count excluding the zero key, which never lived
        // in the table.
        let real = self.size - usize::from(self.contains_zero);
        for _ in 0..real {
            loop {
                i -= 1;
                if self.key[i] != 0 {
                    break;
                }
            }
            let k = self.key[i];
            let mut pos = (mix(i32::from(k)) as usize) & new_mask;
            while new_key[pos] != 0 {
                pos = (pos + 1) & new_mask;
            }
            new_key[pos] = k;
        }
        self.key = new_key;
        self.mask = new_mask;
        self.max_fill = max_fill(new_n, LOAD_FACTOR);
    }

    /// fastutil's `SetIterator` order: the zero key first if present, then
    /// occupied table slots from the top down.
    #[must_use]
    pub fn iter_order(&self) -> Vec<i16> {
        let mut out = Vec::with_capacity(self.size);
        if self.contains_zero {
            out.push(0);
        }
        for pos in (0..self.key.len()).rev() {
            if self.key[pos] != 0 {
                out.push(self.key[pos]);
            }
        }
        out
    }
}

/// `SectionPos.sectionRelativePos`: `(x & 15) << 8 | (z & 15) << 4 | (y & 15)`.
///
/// Note the order — **x, then z, then y** — which is not the order the
/// arguments are usually written in and is the kind of thing that produces a
/// packet full of blocks in plausible-looking wrong places.
#[must_use]
pub const fn section_relative_pos(x: i32, y: i32, z: i32) -> i16 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the mask keeps every term inside 12 bits"
    )]
    {
        ((x & 15) << 8 | (z & 15) << 4 | (y & 15)) as i16
    }
}

#[cfg(test)]
mod tests {
    use super::{mix, section_relative_pos, ShortOpenHashSet};

    /// The two constants everything else rests on.
    #[test]
    fn mix_is_int_phi_then_xor_shift() {
        // Worked by hand from `h = x * 0x9E3779B9; h ^ (h >>> 16)`.
        assert_eq!(mix(0), 0);
        let h = 1_u32.wrapping_mul(0x9E37_79B9);
        assert_eq!(mix(1), (h ^ (h >> 16)) as i32);
    }

    /// The detail that reverses most two-element packets: iteration walks the
    /// table downward, so the wire order is neither insertion order nor
    /// sorted order.
    #[test]
    fn iteration_is_downward_through_the_table() {
        let mut s = ShortOpenHashSet::new();
        for k in [1_i16, 2, 3] {
            s.insert(k);
        }
        let order = s.iter_order();
        assert_eq!(order.len(), 3);
        // Whatever the order is, it is the one the table produces — assert
        // it against the positions rather than against a guess.
        let mask = 31_usize;
        let mut want: Vec<(usize, i16)> = [1_i16, 2, 3]
            .into_iter()
            .map(|k| (((mix(i32::from(k)) as usize) & mask), k))
            .collect();
        want.sort_by_key(|b| std::cmp::Reverse(b.0));
        assert_eq!(order, want.into_iter().map(|(_, k)| k).collect::<Vec<_>>());
    }

    /// Zero is not in the table and comes out first.
    #[test]
    fn the_zero_key_is_yielded_before_the_table() {
        let mut s = ShortOpenHashSet::new();
        s.insert(1);
        s.insert(0);
        s.insert(2);
        assert_eq!(s.len(), 3);
        assert_eq!(s.iter_order()[0], 0, "containsNull is returned first");
    }

    #[test]
    fn inserting_twice_does_not_grow_it() {
        let mut s = ShortOpenHashSet::new();
        assert!(s.insert(7));
        assert!(!s.insert(7));
        assert_eq!(s.len(), 1);
        assert!(!s.insert(0) || s.len() == 2);
    }

    /// Past `maxFill` the table doubles and every key survives.
    #[test]
    fn it_rehashes_and_keeps_everything() {
        let mut s = ShortOpenHashSet::new();
        for k in 1..=40_i16 {
            s.insert(k);
        }
        assert_eq!(s.len(), 40);
        let mut got = s.iter_order();
        got.sort_unstable();
        assert_eq!(got, (1..=40_i16).collect::<Vec<_>>());
    }

    /// fastutil grows on `size++ >= maxFill`, after the key that reaches
    /// `maxFill`, not on it. The 26-block hollow glass box of scenario 129
    /// (x 1..=3, y 7..=9, z 4..=6 inside one section, written x-fastest by
    /// `BlockPos.betweenClosed`) comes out of vanilla in exactly this order
    /// (`cmdexec2-rec2`, 2026-10-01); growing one key early swapped
    /// (2,5,6) ahead of (2,3,5) and (1,3,4).
    #[test]
    fn it_grows_after_the_key_that_reaches_max_fill() {
        let mut s = ShortOpenHashSet::new();
        for z in 4..=6 {
            for y in 3..=5 {
                for x in 1..=3 {
                    if (x, y, z) != (2, 4, 5) {
                        s.insert(section_relative_pos(x, y + 4, z));
                    }
                }
            }
        }
        let got: Vec<[i32; 3]> = s
            .iter_order()
            .into_iter()
            .map(|k| {
                let k = i32::from(k);
                [k >> 8 & 15, (k & 15) - 4, k >> 4 & 15]
            })
            .collect();
        let vanilla: [[i32; 3]; 26] = [
            [3, 5, 4],
            [2, 5, 5],
            [1, 4, 6],
            [2, 3, 6],
            [3, 4, 5],
            [3, 5, 5],
            [1, 3, 6],
            [3, 4, 6],
            [1, 4, 4],
            [1, 5, 6],
            [2, 3, 4],
            [3, 3, 5],
            [2, 3, 5],
            [1, 3, 4],
            [2, 5, 6],
            [2, 4, 6],
            [1, 4, 5],
            [1, 5, 5],
            [3, 5, 6],
            [3, 3, 6],
            [3, 4, 4],
            [1, 3, 5],
            [1, 5, 4],
            [2, 4, 4],
            [2, 5, 4],
            [3, 3, 4],
        ];
        assert_eq!(got, vanilla);
    }

    /// `x` is the high nibble, `z` the middle and `y` the low one.
    #[test]
    fn section_relative_pos_packs_x_z_y() {
        assert_eq!(section_relative_pos(0, 0, 0), 0);
        assert_eq!(section_relative_pos(1, 0, 0), 1 << 8);
        assert_eq!(section_relative_pos(0, 0, 1), 1 << 4);
        assert_eq!(section_relative_pos(0, 1, 0), 1);
        // Only the low four bits of each coordinate survive, and a negative
        // world coordinate masks the same way Java's `&` does.
        assert_eq!(section_relative_pos(-1, -1, -1), 0x0FFF);
        assert_eq!(section_relative_pos(15, 15, 15), 0x0FFF);
    }
}
