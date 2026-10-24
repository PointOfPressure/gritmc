//! The legacy 48-bit LCG — `java.util.Random` semantics.
//!
//! Reference: `net.minecraft.world.level.levelgen.{LegacyRandomSource,
//! BitRandomSource}` (+ nested `LegacyPositionalRandomFactory`).
//!
//! Still load-bearing in 26.2: surface rules, several structure paths and
//! anything reached through `WorldgenRandom` with the LEGACY algorithm read
//! this stream, so it is not a compatibility shim that can be skipped.

use super::gaussian::marsaglia_next_gaussian;
use super::support::{block_seed, java_string_hash};
use super::{PositionalRandomFactory, RandomSource};

const MODULUS_MASK: i64 = 281_474_976_710_655; // 2^48 - 1
const MULTIPLIER: i64 = 25_214_903_917;
const INCREMENT: i64 = 11;

/// `float`/`double` units — both are `f32` literals in Java. See
/// [`super::xoroshiro`] for why the widening is spelled out.
const FLOAT_MULTIPLIER: f32 = 5.960_464_5e-8;
const DOUBLE_MULTIPLIER: f64 = 1.110_223e-16_f32 as f64;

/// `LegacyRandomSource`.
///
/// Vanilla guards its seed with an `AtomicLong` + compare-and-set purely to
/// *detect* concurrent misuse (`ThreadingDetector`), not to support it. Rust
/// makes that guard structural: mutation takes `&mut self`, so the misuse
/// vanilla throws on cannot be written here. The observable stream is
/// unchanged.
#[derive(Clone, Copy, Debug)]
pub struct LegacyRandomSource {
    seed: i64,
    next_next_gaussian: f64,
    have_next_gaussian: bool,
}

impl LegacyRandomSource {
    #[must_use]
    pub fn new(seed: i64) -> Self {
        let mut source = Self {
            seed: 0,
            next_next_gaussian: 0.0,
            have_next_gaussian: false,
        };
        source.set_seed(seed);
        source
    }

    /// An independent stream seeded from one draw of this one.
    pub fn fork(&mut self) -> Self {
        Self::new(self.next_i64())
    }

    /// A positional factory seeded from one draw of this one.
    pub fn fork_positional(&mut self) -> LegacyPositionalFactory {
        LegacyPositionalFactory {
            seed: self.next_i64(),
        }
    }

    /// `BitRandomSource#next` — advance the LCG and take the top `bits`.
    ///
    /// The shift is applied to the 48-bit state, so `bits` is at most 48;
    /// the result is the high-order bits, which is what makes low-bit
    /// correlation of this generator invisible to callers that stay here.
    pub fn next_bits(&mut self, bits: i32) -> i32 {
        self.seed = self.seed.wrapping_mul(MULTIPLIER).wrapping_add(INCREMENT) & MODULUS_MASK;
        // The high bits are the whole point (see the doc above). The sabotage
        // arm takes the LOW ones instead: every value stays in range and the
        // stream stays deterministic, so nothing but a comparison against real
        // Java output can see it. `rustmc_data::sabotage`.
        if rustmc_data::sabotage::fires(rustmc_data::sabotage::Arm::LegacyLcg) {
            return (self.seed & ((1i64 << bits) - 1)) as i32;
        }
        (self.seed >> (48 - bits)) as i32
    }
}

impl RandomSource for LegacyRandomSource {
    fn set_seed(&mut self, seed: i64) {
        self.seed = (seed ^ MULTIPLIER) & MODULUS_MASK;
        self.have_next_gaussian = false;
    }

    fn next_i32(&mut self) -> i32 {
        self.next_bits(32)
    }

    fn next_i32_bound(&mut self, bound: i32) -> i32 {
        bit_source_next_i32_bound(bound, |bits| self.next_bits(bits))
    }

    fn next_i64(&mut self) -> i64 {
        bit_source_next_i64(|bits| self.next_bits(bits))
    }

    fn next_bool(&mut self) -> bool {
        self.next_bits(1) != 0
    }

    fn next_f32(&mut self) -> f32 {
        self.next_bits(24) as f32 * FLOAT_MULTIPLIER
    }

    fn next_f64(&mut self) -> f64 {
        bit_source_next_f64(|bits| self.next_bits(bits))
    }

    marsaglia_next_gaussian!();
}

// ---------------------------------------------------------------------------
// BitRandomSource default methods.
//
// Shared as free functions taking a `next` closure so that `WorldgenRandom`
// gets byte-identical behavior without inheriting anything: in Java these are
// interface defaults, and `WorldgenRandom` reaches them by extending
// `LegacyRandomSource`. Re-implementing them per type is exactly how the two
// would silently drift apart.
// ---------------------------------------------------------------------------

/// `BitRandomSource#nextInt(int)`.
pub(crate) fn bit_source_next_i32_bound(bound: i32, mut next: impl FnMut(i32) -> i32) -> i32 {
    assert!(bound > 0, "Bound must be positive");

    if bound & (bound - 1) == 0 {
        // Power of two: scale a 31-bit draw. The multiply is 64-bit.
        return ((i64::from(bound) * i64::from(next(31))) >> 31) as i32;
    }

    loop {
        let sample = next(31);
        let modulo = sample % bound;
        // Rejection test, in wrapping int arithmetic: vanilla detects the
        // biased tail by letting `sample - modulo + (bound - 1)` overflow
        // negative. Using checked math here would never reject and would
        // skew the distribution away from vanilla's.
        if sample
            .wrapping_sub(modulo)
            .wrapping_add(bound.wrapping_sub(1))
            >= 0
        {
            return modulo;
        }
    }
}

/// `BitRandomSource#nextLong`.
///
/// Two 32-bit draws, combined with **addition** of a *sign-extended* low
/// word — not a bitwise OR. When the low draw is negative this borrows from
/// the high half, so the result is not the concatenation of the two words.
pub(crate) fn bit_source_next_i64(mut next: impl FnMut(i32) -> i32) -> i64 {
    let upper = i64::from(next(32));
    let lower = i64::from(next(32));
    (upper << 32).wrapping_add(lower)
}

/// `BitRandomSource#nextDouble`.
pub(crate) fn bit_source_next_f64(mut next: impl FnMut(i32) -> i32) -> f64 {
    let upper = i64::from(next(26));
    let lower = i64::from(next(27));
    ((upper << 27).wrapping_add(lower)) as f64 * DOUBLE_MULTIPLIER
}

/// `LegacyRandomSource.LegacyPositionalRandomFactory`.
#[derive(Clone, Copy, Debug)]
pub struct LegacyPositionalFactory {
    seed: i64,
}

impl LegacyPositionalFactory {
    #[must_use]
    pub const fn new(seed: i64) -> Self {
        Self { seed }
    }
}

impl PositionalRandomFactory for LegacyPositionalFactory {
    type Source = LegacyRandomSource;

    fn at(&self, x: i32, y: i32, z: i32) -> Self::Source {
        LegacyRandomSource::new(block_seed(x, y, z) ^ self.seed)
    }

    fn from_hash_of(&self, name: &str) -> Self::Source {
        // Java's `String#hashCode` (an int) sign-extends before the xor —
        // unlike the Xoroshiro factory, which hashes with MD5.
        LegacyRandomSource::new(i64::from(java_string_hash(name)) ^ self.seed)
    }

    fn from_seed(&self, seed: i64) -> Self::Source {
        // Note: the parent seed is *not* mixed in here. Vanilla really does
        // ignore `self.seed` on this path.
        LegacyRandomSource::new(seed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_java_util_random_first_draws() {
        // java.util.Random(42): nextInt() stream. LegacyRandomSource is
        // java.util.Random's algorithm, so these are checkable independently
        // of any Minecraft export.
        let mut rng = LegacyRandomSource::new(42);
        assert_eq!(rng.next_i32(), -1170105035);
        assert_eq!(rng.next_i32(), 234785527);
        assert_eq!(rng.next_i32(), -1360544799);
    }

    #[test]
    fn next_i64_adds_a_signed_low_word() {
        // Pins add-vs-OR without depending on any recalled constant: the
        // absolute values are the golden streams' job (tests/golden_rng.rs),
        // so all this needs to prove is the composition rule.
        //
        // The two only disagree when the low word is negative, so find a
        // seed where it is rather than assuming one does.
        let seed = (0i64..1000)
            .find(|&s| {
                let mut rng = LegacyRandomSource::new(s);
                let _ = rng.next_i32();
                rng.next_i32() < 0
            })
            .expect("some seed in 0..1000 has a negative second draw");

        let (upper, lower) = {
            let mut rng = LegacyRandomSource::new(seed);
            (rng.next_i32(), rng.next_i32())
        };
        let added = (i64::from(upper) << 32).wrapping_add(i64::from(lower));
        let ored = (i64::from(upper) << 32) | (i64::from(lower) & 0xFFFF_FFFF);
        assert_ne!(
            added, ored,
            "test premise: add and OR must differ (seed {seed})"
        );

        let mut rng = LegacyRandomSource::new(seed);
        assert_eq!(
            rng.next_i64(),
            added,
            "next_i64 must sign-extend and add the low word (seed {seed})"
        );
    }
}
