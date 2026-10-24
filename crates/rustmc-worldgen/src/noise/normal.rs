//! Normalised (two-stack) Perlin noise — what the density functions sample.
//!
//! Reference: `net.minecraft.world.level.levelgen.synth.NormalNoise`.

use super::perlin::{ForkPositional, PerlinNoise};
use crate::rng::RandomSource;

/// `NormalNoise.INPUT_FACTOR`.
///
/// The second stack is sampled at a slightly irrational multiple of the
/// first's coordinates. That detuning is the whole trick: two stacks in
/// phase would just double the same field, whereas beating them against each
/// other pushes the sum toward a normal distribution.
const INPUT_FACTOR: f64 = 1.018_126_888_217_522_7;

/// Two [`PerlinNoise`] stacks, summed and rescaled toward unit deviation.
#[derive(Clone, Debug)]
pub struct NormalNoise {
    first: PerlinNoise,
    second: PerlinNoise,
    value_factor: f64,
    max_value: f64,
}

impl NormalNoise {
    /// `NormalNoise.create(random, firstOctave, amplitudes)`.
    ///
    /// Both stacks are built from the **same** `random`, in order — the
    /// second consumes whatever the first left. Building them from separate
    /// sources gives a valid-looking noise field that is not vanilla's.
    pub fn create<R: RandomSource + ForkPositional>(
        random: &mut R,
        first_octave: i32,
        amplitudes: &[f64],
    ) -> Self {
        let first = PerlinNoise::create(random, first_octave, amplitudes);
        let second = PerlinNoise::create(random, first_octave, amplitudes);
        // Span of the *non-zero* amplitudes, so leading/trailing zeros do
        // not widen it — see [`Self::from_stacks`].
        Self::from_stacks(first, second, amplitudes)
    }

    /// `NormalNoise.createLegacyNetherBiome(random, parameters)`.
    ///
    /// The same constructor with `useLegacyNetherBiome = false` in vanilla's
    /// boolean sense: both stacks come from
    /// `PerlinNoise.createLegacyForLegacyNetherBiome` instead of
    /// `PerlinNoise.create`, so the octaves are drawn **sequentially from
    /// this one source** rather than from positional forks, and the zero
    /// octave is built first.
    ///
    /// Only the nether's `temperature` and `vegetation` noises take this
    /// path, and only because `RandomState$1NoiseWiringHelper.visitNoise`
    /// special-cases those two keys by identity — it does **not** follow from
    /// `useLegacyRandomSource`. The end's noise settings are also legacy and
    /// its noises do not come through here.
    pub fn create_legacy_nether_biome<R: RandomSource>(
        random: &mut R,
        first_octave: i32,
        amplitudes: &[f64],
    ) -> Self {
        let first =
            PerlinNoise::create_legacy_for_legacy_nether_biome(random, first_octave, amplitudes);
        let second =
            PerlinNoise::create_legacy_for_legacy_nether_biome(random, first_octave, amplitudes);
        Self::from_stacks(first, second, amplitudes)
    }

    /// The tail both constructors share: the value factor is a function of
    /// the **non-zero** amplitude span and of nothing else, so it cannot
    /// differ between the two paths.
    fn from_stacks(first: PerlinNoise, second: PerlinNoise, amplitudes: &[f64]) -> Self {
        let mut min_octave = i32::MAX;
        let mut max_octave = i32::MIN;
        for (i, &amplitude) in amplitudes.iter().enumerate() {
            if amplitude != 0.0 {
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_possible_wrap,
                    reason = "an amplitude list is a handful of entries"
                )]
                let i = i as i32;
                min_octave = min_octave.min(i);
                max_octave = max_octave.max(i);
            }
        }
        let value_factor = 0.166_666_666_666_666_66 / expected_deviation(max_octave - min_octave);
        let max_value = (first.max_value() + second.max_value()) * value_factor;
        Self {
            first,
            second,
            value_factor,
            max_value,
        }
    }

    /// `expectedDeviation`.
    #[must_use]
    pub fn max_value(&self) -> f64 {
        self.max_value
    }

    /// `getValue(x, y, z)`.
    #[must_use]
    pub fn get_value(&self, x: f64, y: f64, z: f64) -> f64 {
        let x2 = x * INPUT_FACTOR;
        let y2 = y * INPUT_FACTOR;
        let z2 = z * INPUT_FACTOR;
        (self.first.get_value(x, y, z) + self.second.get_value(x2, y2, z2)) * self.value_factor
    }
}

fn expected_deviation(octave_span: i32) -> f64 {
    0.1 * (1.0 + 1.0 / f64::from(octave_span + 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::XoroshiroRandomSource;

    #[test]
    fn the_two_stacks_are_not_the_same_field() {
        // If both stacks were seeded identically the sum would be a scaled
        // copy of one of them, and `get_value(p)` would equal
        // `2 * first(p) * value_factor` at the origin. Pin that they differ.
        let mut rng = XoroshiroRandomSource::from_seed(20260708);
        let noise = NormalNoise::create(&mut rng, -6, &[1.0, 1.0]);
        let combined = noise.get_value(0.5, 0.5, 0.5);
        let first_only = noise.first.get_value(0.5, 0.5, 0.5);
        assert_ne!(
            combined.to_bits(),
            (2.0 * first_only * noise.value_factor).to_bits()
        );
    }

    #[test]
    fn zero_amplitudes_do_not_widen_the_octave_span() {
        // [1, 0, 1] spans indices 0..=2 either way, but [0, 1, 1] must span
        // 1..=2 — a span of 1, not 2 — or the value factor is wrong.
        let mut rng = XoroshiroRandomSource::from_seed(1);
        let leading_zero = NormalNoise::create(&mut rng, -4, &[0.0, 1.0, 1.0]);
        let mut rng = XoroshiroRandomSource::from_seed(1);
        let full = NormalNoise::create(&mut rng, -4, &[1.0, 1.0, 1.0]);
        assert_ne!(leading_zero.value_factor, full.value_factor);
    }
}
