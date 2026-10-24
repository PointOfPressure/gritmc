//! Xoroshiro128++ — the modern worldgen stream.
//!
//! Reference: `net.minecraft.world.level.levelgen.Xoroshiro128PlusPlus` and
//! `...XoroshiroRandomSource` (+ its nested `XoroshiroPositionalRandomFactory`).

use super::gaussian::marsaglia_next_gaussian;
use super::support::{
    block_seed, seed_from_hash_of, upgrade_seed_to_128bit, Seed128, GOLDEN_RATIO_64,
    SILVER_RATIO_64,
};
use super::{PositionalRandomFactory, RandomSource};

/// `float` unit — note the literal is `5.9604645E-8F` in Java, i.e. `f32`.
const FLOAT_MULTIPLIER: f32 = 5.960_464_5e-8;

/// `double` unit. Java writes `1.110223E-16F` — an **`f32` literal** used in
/// double context. It is *not* `2^-53`, and it is *not* the `f64` nearest to
/// the decimal `1.110223e-16`: it is the `f64` you get by widening the `f32`.
/// Writing this as an `f64` literal is the single easiest way to be
/// almost-right here, so the widening is spelled out explicitly.
const DOUBLE_MULTIPLIER: f64 = 1.110_223e-16_f32 as f64;

/// The raw generator (no seed upgrading, no derived draws).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Xoroshiro128PlusPlus {
    seed_lo: i64,
    seed_hi: i64,
}

impl Xoroshiro128PlusPlus {
    /// An all-zero state is a fixed point of the recurrence, so vanilla
    /// substitutes the golden/silver ratios rather than emitting zeros
    /// forever.
    #[must_use]
    pub const fn new(seed_lo: i64, seed_hi: i64) -> Self {
        if (seed_lo | seed_hi) == 0 {
            Self {
                seed_lo: GOLDEN_RATIO_64,
                seed_hi: SILVER_RATIO_64,
            }
        } else {
            Self { seed_lo, seed_hi }
        }
    }

    #[must_use]
    pub const fn from_seed128(seed: Seed128) -> Self {
        Self::new(seed.lo, seed.hi)
    }

    /// `seedLo` and `seedHi` — what `Xoroshiro128PlusPlus.CODEC` saves (a
    /// two-long stream), e.g. a random sequence in `random_sequences.dat`.
    #[must_use]
    pub const fn state(&self) -> (i64, i64) {
        (self.seed_lo, self.seed_hi)
    }

    pub fn next_i64(&mut self) -> i64 {
        let s0 = self.seed_lo;
        let mut s1 = self.seed_hi;
        let result = s0.wrapping_add(s1).rotate_left(17).wrapping_add(s0);
        s1 ^= s0;
        // `s1 << 21` discards the high bits, exactly as Java's `<<` does.
        //
        // The rotate is 49 and the sabotage arm makes it 48. That is §73.9's
        // mutation A, which 794 of 795 lib tests do not notice and 8 of 9
        // golden binaries do; it lives behind a feature rather than in a
        // scratch tree so the control can be re-run. `rustmc_data::sabotage`.
        let rot = if rustmc_data::sabotage::fires(rustmc_data::sabotage::Arm::Xoroshiro) {
            48
        } else {
            49
        };
        self.seed_lo = s0.rotate_left(rot) ^ s1 ^ (s1 << 21);
        self.seed_hi = s1.rotate_left(28);
        result
    }
}

/// `RandomSource` over Xoroshiro128++.
#[derive(Clone, Copy, Debug)]
pub struct XoroshiroRandomSource {
    generator: Xoroshiro128PlusPlus,
    next_next_gaussian: f64,
    have_next_gaussian: bool,
}

impl XoroshiroRandomSource {
    /// From a world seed — upgrades to 128 bits and mixes.
    #[must_use]
    pub fn from_seed(seed: i64) -> Self {
        Self::from_seed128(upgrade_seed_to_128bit(seed))
    }

    /// From explicit halves — **no** upgrade/mix. This is the constructor
    /// the positional factory uses, and confusing the two silently reseeds
    /// every derived stream.
    #[must_use]
    pub const fn from_parts(seed_lo: i64, seed_hi: i64) -> Self {
        Self {
            generator: Xoroshiro128PlusPlus::new(seed_lo, seed_hi),
            next_next_gaussian: 0.0,
            have_next_gaussian: false,
        }
    }

    #[must_use]
    pub const fn from_seed128(seed: Seed128) -> Self {
        Self::from_parts(seed.lo, seed.hi)
    }

    /// The generator's two state words — `XoroshiroRandomSource.CODEC` is
    /// the generator's codec mapped, so this is everything it saves (the
    /// cached gaussian is not).
    #[must_use]
    pub const fn state(&self) -> (i64, i64) {
        self.generator.state()
    }

    /// An independent stream, consuming two draws from this one.
    pub fn fork(&mut self) -> Self {
        let lo = self.generator.next_i64();
        let hi = self.generator.next_i64();
        Self::from_parts(lo, hi)
    }

    /// A positional factory, consuming two draws from this one.
    pub fn fork_positional(&mut self) -> XoroshiroPositionalFactory {
        let seed_lo = self.generator.next_i64();
        let seed_hi = self.generator.next_i64();
        XoroshiroPositionalFactory { seed_lo, seed_hi }
    }

    /// Top `bits` bits of a draw: Java `nextLong() >>> (64 - bits)`, a
    /// logical shift.
    pub(crate) fn next_bits(&mut self, bits: u32) -> i64 {
        ((self.generator.next_i64() as u64) >> (64 - bits)) as i64
    }
}

impl RandomSource for XoroshiroRandomSource {
    fn set_seed(&mut self, seed: i64) {
        self.generator = Xoroshiro128PlusPlus::from_seed128(upgrade_seed_to_128bit(seed));
        self.have_next_gaussian = false;
    }

    fn next_i32(&mut self) -> i32 {
        self.generator.next_i64() as i32
    }

    fn next_i32_bound(&mut self, bound: i32) -> i32 {
        assert!(bound > 0, "Bound must be positive");

        // Lemire's multiply-shift with rejection. `random_bits` is the draw
        // reinterpreted as an *unsigned* 32-bit value widened to 64
        // (`Integer.toUnsignedLong`), so the cast chain u32 -> i64 is
        // load-bearing: going i32 -> i64 directly would sign-extend.
        let bound64 = i64::from(bound);
        let mut random_bits = i64::from(self.next_i32() as u32);
        let mut multiplied = random_bits.wrapping_mul(bound64);
        let mut fractional = multiplied & 0xFFFF_FFFF;

        if fractional < bound64 {
            // `Integer.remainderUnsigned(~bound + 1, bound)` — the number of
            // low buckets that would bias the result.
            let threshold = i64::from((!bound).wrapping_add(1) as u32 % bound as u32);
            while fractional < threshold {
                random_bits = i64::from(self.next_i32() as u32);
                multiplied = random_bits.wrapping_mul(bound64);
                fractional = multiplied & 0xFFFF_FFFF;
            }
        }

        (multiplied >> 32) as i32
    }

    fn next_i64(&mut self) -> i64 {
        self.generator.next_i64()
    }

    fn next_bool(&mut self) -> bool {
        (self.generator.next_i64() & 1) != 0
    }

    fn next_f32(&mut self) -> f32 {
        // Java casts to float *before* multiplying, so the product is f32.
        (self.next_bits(24) as f32) * FLOAT_MULTIPLIER
    }

    fn next_f64(&mut self) -> f64 {
        self.next_bits(53) as f64 * DOUBLE_MULTIPLIER
    }

    marsaglia_next_gaussian!();
}

/// `XoroshiroRandomSource.XoroshiroPositionalRandomFactory`.
#[derive(Clone, Copy, Debug)]
pub struct XoroshiroPositionalFactory {
    seed_lo: i64,
    seed_hi: i64,
}

impl XoroshiroPositionalFactory {
    #[must_use]
    pub const fn new(seed_lo: i64, seed_hi: i64) -> Self {
        Self { seed_lo, seed_hi }
    }
}

impl PositionalRandomFactory for XoroshiroPositionalFactory {
    type Source = XoroshiroRandomSource;

    fn at(&self, x: i32, y: i32, z: i32) -> Self::Source {
        // Only the low half is perturbed by position.
        XoroshiroRandomSource::from_parts(block_seed(x, y, z) ^ self.seed_lo, self.seed_hi)
    }

    fn from_hash_of(&self, name: &str) -> Self::Source {
        XoroshiroRandomSource::from_seed128(seed_from_hash_of(name).xor(self.seed_lo, self.seed_hi))
    }

    fn from_seed(&self, seed: i64) -> Self::Source {
        // Both halves, and no seed upgrade — unlike `XoroshiroRandomSource::
        // from_seed`, which shares the name and does the opposite.
        XoroshiroRandomSource::from_parts(seed ^ self.seed_lo, seed ^ self.seed_hi)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_state_is_replaced() {
        let zeroed = Xoroshiro128PlusPlus::new(0, 0);
        assert_eq!(
            zeroed,
            Xoroshiro128PlusPlus::new(GOLDEN_RATIO_64, SILVER_RATIO_64)
        );
    }

    #[test]
    fn double_multiplier_is_the_widened_float() {
        // Guards the quirk: the f64 literal spelling differs from the
        // widened f32 one, and only the latter is vanilla.
        assert_ne!(DOUBLE_MULTIPLIER, 1.110_223e-16_f64);
        assert_eq!(DOUBLE_MULTIPLIER, f64::from(1.110_223e-16_f32));
    }
}
