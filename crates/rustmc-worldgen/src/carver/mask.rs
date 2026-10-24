//! The per-chunk record of which blocks a carver has already visited.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `net.minecraft.world.level.chunk.CarvingMask`.
//!
//! One bit per block in the chunk, shared by **every** carver that reaches
//! it — all 289 source chunks in the `±8` sweep write into the same mask.
//! That sharing is not bookkeeping, it is the reason caves intersect the way
//! they do: `carveEllipsoid` skips any position already marked, so the first
//! carver to reach a block owns it and later ones flow around.
//!
//! Two consequences:
//!
//! - The mask must persist across the whole sweep. A per-carver mask
//!   generates visibly different caves — thicker, because every carver
//!   re-digs what the last one did — while still looking entirely plausible.
//! - The mask is marked *before* the block is carved, and marked even when
//!   the carve is refused (the block was not replaceable, or the aquifer
//!   returned nothing). Marking only on success would let a later carver
//!   retry the same block.
//!
//! The mask also survives to disk in vanilla, as `toArray`/the `long[]`
//! constructor, so a chunk saved mid-generation resumes with the same
//! intersections. That round trip is not implemented here yet and is called
//! out rather than quietly skipped — see [`CarvingMask::to_long_array`].

/// A chunk's carving mask.
#[derive(Clone, Debug)]
pub struct CarvingMask {
    min_y: i32,
    /// `BitSet(256 * height)`, indexed `x | z << 4 | (y - min_y) << 8`.
    bits: Box<[u64]>,
    height: i32,
}

impl CarvingMask {
    /// A mask for a chunk of `height` blocks starting at `min_y`.
    #[must_use]
    pub fn new(height: i32, min_y: i32) -> Self {
        #[allow(clippy::cast_sign_loss)]
        let bit_count = 256usize * height.max(0) as usize;
        Self {
            min_y,
            bits: vec![0u64; bit_count.div_ceil(64)].into_boxed_slice(),
            height,
        }
    }

    /// `getIndex`.
    ///
    /// `x` and `z` are masked to the chunk but **`y` is not range-checked**,
    /// matching vanilla: an out-of-range `y` would index outside the bitset
    /// and throw. Callers are expected to have clamped already —
    /// `carveEllipsoid` does, against the context's own generation bounds —
    /// so this returns `None` instead of panicking, and the caller treats
    /// that as "already carved" so nothing is written outside the chunk.
    fn index(&self, x: i32, y: i32, z: i32) -> Option<usize> {
        if y < self.min_y || y >= self.min_y + self.height {
            return None;
        }
        #[allow(clippy::cast_sign_loss)]
        Some(((x & 15) | ((z & 15) << 4) | ((y - self.min_y) << 8)) as usize)
    }

    /// `CarvingMask#set`.
    pub fn set(&mut self, x: i32, y: i32, z: i32) {
        if let Some(index) = self.index(x, y, z) {
            self.bits[index / 64] |= 1u64 << (index % 64);
        }
    }

    /// `CarvingMask#get`.
    ///
    /// Vanilla ORs in an `additionalMask`, which only the debug carver
    /// visualiser sets. Not modelled; a caller that needs it can check it
    /// before calling.
    #[must_use]
    pub fn get(&self, x: i32, y: i32, z: i32) -> bool {
        // Outside the chunk reads as *set*, so a carver whose ellipsoid
        // overhangs the vertical bounds skips those blocks rather than
        // wrapping into another y.
        self.index(x, y, z)
            .is_none_or(|index| self.bits[index / 64] & (1u64 << (index % 64)) != 0)
    }

    /// How many blocks have been marked. Diagnostics only — no vanilla
    /// counterpart — but a carver that marks nothing and a carver that marks
    /// everything both look like "no caves" from outside.
    #[must_use]
    pub fn count(&self) -> u32 {
        self.bits.iter().map(|word| word.count_ones()).sum()
    }

    /// The on-disk form, `CarvingMask#toArray`.
    ///
    /// Java's `BitSet.toLongArray` **truncates trailing zero words**, so a
    /// mask with nothing set serialises to an empty array rather than to
    /// zeroes. Reproduced here because the length is observable in the NBT.
    #[must_use]
    pub fn to_long_array(&self) -> Vec<u64> {
        let last_set = self.bits.iter().rposition(|word| *word != 0);
        last_set.map_or_else(Vec::new, |end| self.bits[..=end].to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mask() -> CarvingMask {
        CarvingMask::new(384, -64)
    }

    #[test]
    fn a_block_reads_back_as_carved_and_its_neighbours_do_not() {
        let mut m = mask();
        assert!(!m.get(3, 40, 7));
        m.set(3, 40, 7);
        assert!(m.get(3, 40, 7));
        for (dx, dy, dz) in [(1, 0, 0), (0, 1, 0), (0, 0, 1), (-1, 0, 0)] {
            assert!(!m.get(3 + dx, 40 + dy, 7 + dz), "bled into {dx},{dy},{dz}");
        }
        assert_eq!(m.count(), 1);
    }

    #[test]
    fn the_index_is_chunk_local_in_xz_and_absolute_in_y() {
        // World coordinates arrive unmasked — `carveEllipsoid` passes chunk
        // indices for x/z but a world y. If y were masked too, every cave
        // would repeat every 16 blocks vertically.
        let mut m = mask();
        m.set(0, 40, 0);
        assert!(m.get(16, 40, 0), "x should be masked to the chunk");
        assert!(m.get(0, 40, 32), "z should be masked to the chunk");
        assert!(!m.get(0, 56, 0), "y must not be masked");
    }

    #[test]
    fn every_position_in_the_chunk_has_its_own_bit() {
        // 256 * 384 distinct indices, no aliasing. A wrong shift in the
        // index would collapse two positions onto one bit and carve holes
        // in places nothing dug.
        let mut m = mask();
        let mut expected = 0u32;
        for y in (-64..320).step_by(37) {
            for z in 0..16 {
                for x in 0..16 {
                    m.set(x, y, z);
                    expected += 1;
                    assert_eq!(m.count(), expected, "aliased at ({x}, {y}, {z})");
                }
            }
        }
    }

    #[test]
    fn outside_the_vertical_bounds_reads_as_already_carved() {
        // So an ellipsoid overhanging the world floor skips those blocks
        // instead of wrapping into a different y.
        let mut m = mask();
        assert!(m.get(0, -65, 0));
        assert!(m.get(0, 320, 0));
        m.set(0, -65, 0);
        assert_eq!(m.count(), 0, "a write outside the chunk must be dropped");
    }

    #[test]
    fn an_empty_mask_serialises_to_nothing() {
        // `BitSet.toLongArray` truncates trailing zero words, and the length
        // is visible in the chunk NBT.
        let mut m = mask();
        assert!(m.to_long_array().is_empty());
        m.set(0, -64, 0);
        assert_eq!(m.to_long_array(), vec![1u64]);
        m.set(15, 319, 15);
        let words = m.to_long_array();
        assert_eq!(
            words.len(),
            256 * 384 / 64,
            "the last bit is in the last word"
        );
    }
}
