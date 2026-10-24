//! `WorldgenRandom` — the seeding ladder chunk generation actually calls.
//!
//! Reference: `net.minecraft.world.level.levelgen.WorldgenRandom`.
//!
//! This is the layer that turns *one* world seed into a reproducible,
//! position-addressed family of streams. Every constant below is part of the
//! world's identity: change one and every seed in existence generates a
//! different world.

use super::legacy::{
    bit_source_next_f64, bit_source_next_i32_bound, bit_source_next_i64, LegacyRandomSource,
};
use super::xoroshiro::XoroshiroRandomSource;
use super::{gaussian::marsaglia_next_gaussian, RandomSource};

const FLOAT_MULTIPLIER: f32 = 5.960_464_5e-8;

/// A source that can back a [`WorldgenRandom`].
///
/// Vanilla asks `instanceof LegacyRandomSource` inside `next(bits)` and
/// takes a different path for each answer. That runtime test is encoded here
/// as a trait method, so the two behaviors stay attached to their types
/// instead of to a branch that a future edit could get wrong.
pub trait WorldgenBacking: RandomSource {
    /// `WorldgenRandom#next(int)`'s inner half.
    fn worldgen_next_bits(&mut self, bits: i32) -> i32;
}

impl WorldgenBacking for LegacyRandomSource {
    fn worldgen_next_bits(&mut self, bits: i32) -> i32 {
        // Legacy path: use the LCG's own bit extraction.
        self.next_bits(bits)
    }
}

impl WorldgenBacking for XoroshiroRandomSource {
    fn worldgen_next_bits(&mut self, bits: i32) -> i32 {
        // Non-legacy path: take the high bits of a full 64-bit draw
        // (`nextLong() >>> (64 - bits)`), logically shifted.
        ((self.next_i64() as u64) >> (64 - bits)) as i32
    }
}

/// `WorldgenRandom`.
///
/// Wraps a backing source and re-exposes it with **`java.util.Random`'s
/// derived-draw semantics**, whatever the backing algorithm is. That is the
/// subtle part: `WorldgenRandom<XoroshiroRandomSource>::next_i64` is *not*
/// `XoroshiroRandomSource::next_i64` — it is two 32-bit draws recombined
/// (see [`bit_source_next_i64`]), because in Java the method resolves to
/// `BitRandomSource`'s default rather than Xoroshiro's override.
#[derive(Clone, Copy, Debug)]
pub struct WorldgenRandom<R: WorldgenBacking> {
    source: R,
    count: u64,
    next_next_gaussian: f64,
    have_next_gaussian: bool,
}

impl<R: WorldgenBacking> WorldgenRandom<R> {
    #[must_use]
    pub const fn new(source: R) -> Self {
        Self {
            source,
            count: 0,
            next_next_gaussian: 0.0,
            have_next_gaussian: false,
        }
    }

    /// Draws taken so far. Vanilla exposes this so feature placement can be
    /// audited for stream drift; keep it accurate.
    #[must_use]
    pub const fn count(&self) -> u64 {
        self.count
    }

    #[must_use]
    pub const fn source(&self) -> &R {
        &self.source
    }

    pub fn source_mut(&mut self) -> &mut R {
        &mut self.source
    }

    /// `WorldgenRandom#next(int)`.
    pub fn next_bits(&mut self, bits: i32) -> i32 {
        self.count += 1;
        self.source.worldgen_next_bits(bits)
    }

    /// `setDecorationSeed` — per-chunk decoration stream. Returns the
    /// derived seed, which callers hash further per feature.
    ///
    /// **Takes the chunk's minimum BLOCK coordinates, not chunk
    /// coordinates** — unlike [`Self::set_large_feature_seed`] directly
    /// below, which takes chunk coordinates. The decompiler names both
    /// parameter pairs `chunkX`/`chunkZ`; the golden vectors disagree with
    /// it, pairing `minBlockX = -208, minBlockZ = 432` with
    /// `chunkX = -13, chunkZ = 27` for the same chunk. Passing chunk
    /// coordinates here yields a valid-looking seed for the wrong chunk,
    /// and every decoration in the world lands somewhere else.
    pub fn set_decoration_seed(&mut self, seed: i64, min_block_x: i32, min_block_z: i32) -> i64 {
        self.set_seed(seed);
        // `| 1` forces both scales odd, so the lattice they generate covers
        // every chunk instead of collapsing onto even coordinates.
        let x_scale = self.next_i64() | 1;
        let z_scale = self.next_i64() | 1;
        let result = i64::from(min_block_x)
            .wrapping_mul(x_scale)
            .wrapping_add(i64::from(min_block_z).wrapping_mul(z_scale))
            ^ seed;
        self.set_seed(result);
        result
    }

    /// `setFeatureSeed` — one stream per (feature index, generation step).
    pub fn set_feature_seed(&mut self, seed: i64, index: i32, step: i32) {
        // `10000 * step` is an **int** multiply in Java and wraps at 32 bits
        // before widening.
        let result = seed
            .wrapping_add(i64::from(index))
            .wrapping_add(i64::from(10000i32.wrapping_mul(step)));
        self.set_seed(result);
    }

    /// `setLargeFeatureSeed` — structure-scale stream for a chunk.
    pub fn set_large_feature_seed(&mut self, seed: i64, chunk_x: i32, chunk_z: i32) {
        self.set_seed(seed);
        let x_scale = self.next_i64();
        let z_scale = self.next_i64();
        let result = (i64::from(chunk_x).wrapping_mul(x_scale))
            ^ (i64::from(chunk_z).wrapping_mul(z_scale))
            ^ seed;
        self.set_seed(result);
    }

    /// `setLargeFeatureWithSalt` — salted structure stream.
    ///
    /// The salt is what keeps two structure types from landing on identical
    /// candidate positions given the same world seed, so it is part of each
    /// structure's identity rather than a tuning knob.
    pub fn set_large_feature_with_salt(&mut self, seed: i64, x: i32, z: i32, salt: i32) {
        let result = i64::from(x)
            .wrapping_mul(341_873_128_712)
            .wrapping_add(i64::from(z).wrapping_mul(132_897_987_541))
            .wrapping_add(seed)
            .wrapping_add(i64::from(salt));
        self.set_seed(result);
    }
}

impl<R: WorldgenBacking> RandomSource for WorldgenRandom<R> {
    fn set_seed(&mut self, seed: i64) {
        // Delegates to the backing source, so what `seed` means depends on
        // the algorithm (LCG scramble vs 128-bit upgrade).
        self.source.set_seed(seed);
        // The cached second gaussian is deliberately NOT cleared.
        //
        // `WorldgenRandom extends LegacyRandomSource`, and the parent's
        // `setSeed` does reset its `MarsagliaPolarGaussian`. But
        // `WorldgenRandom` **overrides** `setSeed` and its whole body is
        // `this.randomSource.setSeed(seed)` — it never calls `super`, so the
        // inherited gaussian source it also inherits `nextGaussian` from is
        // never reset. Read from the bytecode, because the reset is exactly
        // what a reasonable port assumes is there.
        //
        // The consequence is that a cached `nextNextGaussian` **survives
        // `setFeatureSeed`** and is handed to the *next* feature as its first
        // gaussian. That is not a call-count difference, which is why it
        // hid: `sulfur_spike`'s emissions matched vanilla's draw offsets
        // exactly (18, 22, 26, 30) while every position differed, because
        // both sides spent the same number of calls on gaussians whose
        // values were one apart in the sequence.
    }

    fn next_i32(&mut self) -> i32 {
        super::draws::bump();
        let v = self.next_bits(32);
        super::draws::log_bits("nextIntUnbounded", i64::from(v));
        v
    }

    fn next_i32_bound(&mut self, bound: i32) -> i32 {
        super::draws::bump();
        let v = bit_source_next_i32_bound(bound, |bits| self.next_bits(bits));
        super::draws::log("nextInt", i64::from(bound), i64::from(v));
        v
    }

    fn next_i64(&mut self) -> i64 {
        super::draws::bump();
        let v = bit_source_next_i64(|bits| self.next_bits(bits));
        super::draws::log("nextLong", 0, v);
        v
    }

    fn next_bool(&mut self) -> bool {
        super::draws::bump();
        let v = self.next_bits(1) != 0;
        super::draws::log_bits("nextBoolean", i64::from(v));
        v
    }

    fn next_f32(&mut self) -> f32 {
        super::draws::bump();
        let v = self.next_bits(24) as f32 * FLOAT_MULTIPLIER;
        super::draws::log_bits("nextFloat", i64::from(v.to_bits().cast_signed()));
        v
    }

    fn next_f64(&mut self) -> f64 {
        super::draws::bump();
        let v = bit_source_next_f64(|bits| self.next_bits(bits));
        super::draws::log_bits("nextDouble", v.to_bits().cast_signed());
        v
    }

    marsaglia_next_gaussian!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xoroshiro_backed_next_i64_differs_from_raw_xoroshiro() {
        // Pins the `BitRandomSource`-default-vs-override distinction: if
        // these ever agree, WorldgenRandom has silently inherited the wrong
        // nextLong and every feature placement shifts.
        let raw = XoroshiroRandomSource::from_seed(20260708).next_i64();
        let wrapped = WorldgenRandom::new(XoroshiroRandomSource::from_seed(20260708)).next_i64();
        assert_ne!(raw, wrapped);
    }

    #[test]
    fn count_tracks_draws() {
        let mut rng = WorldgenRandom::new(LegacyRandomSource::new(1));
        assert_eq!(rng.count(), 0);
        let _ = rng.next_i64(); // two 32-bit draws
        assert_eq!(rng.count(), 2);
    }
}
