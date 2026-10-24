//! Octave-stacked simplex noise.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `net.minecraft.world.level.levelgen.synth.PerlinSimplexNoise`.
//!
//! This is the *legacy* octave stack — the pre-1.18 one — and it survives in
//! exactly three places, all of them static and all of them
//! world-independent: `Biome`'s `TEMPERATURE_NOISE`, `FROZEN_TEMPERATURE_NOISE`
//! and `BIOME_INFO_NOISE`. Nothing in the modern density pipeline uses it.
//! It exists here because the surface rule's `temperature` condition asks a
//! biome whether it is cold enough to snow, and that answer is a function of
//! these three noises.
//!
//! # It is not [`super::PerlinNoise`] with a different lattice
//!
//! [`super::PerlinNoise`] indexes octaves so that octave 0 is the *lowest*
//! frequency and amplitudes are supplied per octave. Here the octave set is
//! signed, the *highest* frequency sits at index 0, and negative octaves are
//! seeded from a **second, derived random source** rather than continuing the
//! first. The two are different functions with similar names.
//!
//! # Skipped octaves still cost draws
//!
//! A gap in the octave set does not skip the generator, it burns 262 draws —
//! the exact count a `SimplexNoise` construction would have consumed. The
//! count is behaviour: get it wrong and every later octave in the stack is
//! seeded from a shifted stream. Vanilla hardcodes 262 rather than deriving
//! it, so this does too.

use std::collections::BTreeSet;

use super::SimplexNoise;
use crate::rng::{LegacyRandomSource, RandomSource, WorldgenRandom};

/// `PerlinSimplexNoise`.
#[derive(Clone, Debug)]
pub struct PerlinSimplexNoise {
    /// `None` where the octave set had a gap. Vanilla stores a null and
    /// checks for it in the sum; keeping the hole explicit means the
    /// index-to-frequency mapping stays the same as the reference rather
    /// than being compacted.
    noise_levels: Vec<Option<SimplexNoise>>,
    highest_freq_value_factor: f64,
    highest_freq_input_factor: f64,
}

impl PerlinSimplexNoise {
    /// Build from an octave set.
    ///
    /// Vanilla takes a `List<Integer>` and immediately funnels it through an
    /// `IntRBTreeSet`, so duplicates collapse and order is irrelevant — a
    /// [`BTreeSet`] is the same contract, and taking a slice here keeps the
    /// three call sites reading like the reference's `ImmutableList.of(...)`.
    ///
    /// # Panics
    /// If `octaves` is empty, or if the implied octave count is below one —
    /// both `IllegalArgumentException` in vanilla.
    #[must_use]
    pub fn new<R: RandomSource>(random: &mut R, octaves: &[i32]) -> Self {
        let set: BTreeSet<i32> = octaves.iter().copied().collect();
        let (&first, &last) = match (set.iter().next(), set.iter().next_back()) {
            (Some(f), Some(l)) => (f, l),
            _ => panic!("Need some octaves!"),
        };

        let low_freq_octaves = -first;
        let high_freq_octaves = last;
        let octave_count = low_freq_octaves + high_freq_octaves + 1;
        assert!(
            octave_count >= 1,
            "Total number of octaves needs to be >= 1"
        );

        // Drawn unconditionally, before anything is stored. Even an octave
        // set that does not contain 0 pays for this one.
        let zero_octave = SimplexNoise::new(random);
        let zero_octave_index = high_freq_octaves;
        let mut noise_levels: Vec<Option<SimplexNoise>> = vec![None; octave_count as usize];

        if high_freq_octaves >= 0 && high_freq_octaves < octave_count && set.contains(&0) {
            noise_levels[high_freq_octaves as usize] = Some(zero_octave.clone());
        }

        for i in (high_freq_octaves + 1)..octave_count {
            if i >= 0 && set.contains(&(zero_octave_index - i)) {
                noise_levels[i as usize] = Some(SimplexNoise::new(random));
            } else {
                random.consume_count(262);
            }
        }

        if high_freq_octaves > 0 {
            // `9.223372E18F` is a **float** literal — near, but not equal
            // to, `Long.MAX_VALUE`. Widening it to a double literal changes
            // the seed and therefore every high-frequency octave. The `as
            // i64` narrowing saturates and maps NaN to zero, which is what
            // Java's double-to-long cast does.
            let scale = f64::from(9.223_372E18_f32);
            let sample = zero_octave.get_value_3d(zero_octave.xo, zero_octave.yo, zero_octave.zo);
            let positive_octave_seed = (sample * scale) as i64;
            let mut high_freq_random =
                WorldgenRandom::new(LegacyRandomSource::new(positive_octave_seed));

            for i in (0..zero_octave_index).rev() {
                if i < octave_count && set.contains(&(zero_octave_index - i)) {
                    noise_levels[i as usize] = Some(SimplexNoise::new(&mut high_freq_random));
                } else {
                    high_freq_random.consume_count(262);
                }
            }
        }

        Self {
            noise_levels,
            // `Math.pow(2.0, n)` for integral `n` in this range is exact in
            // both languages, so `powi` is the same value and not an
            // approximation of it.
            highest_freq_input_factor: 2.0f64.powi(high_freq_octaves),
            highest_freq_value_factor: 1.0 / (2.0f64.powi(octave_count) - 1.0),
        }
    }

    /// The `[0]` stack: one octave, unit factors.
    ///
    /// Both `TEMPERATURE_NOISE` (seed 1234) and `BIOME_INFO_NOISE` (seed
    /// 2345) have this shape.
    #[must_use]
    pub fn single_octave(seed: i64) -> Self {
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(seed));
        Self::new(&mut random, &[0])
    }

    /// 2D value.
    ///
    /// `use_noise_start` folds each octave's own offset into the input. All
    /// three `Biome` call sites pass `false`, so the offsets are unused
    /// there — but the parameter is part of the function and a caller that
    /// hardcoded `false` would be a different one.
    #[must_use]
    pub fn get_value(&self, x: f64, y: f64, use_noise_start: bool) -> f64 {
        let mut value = 0.0;
        let mut factor = self.highest_freq_input_factor;
        let mut value_factor = self.highest_freq_value_factor;

        for level in &self.noise_levels {
            if let Some(noise) = level {
                let (ox, oy) = if use_noise_start {
                    (noise.xo, noise.yo)
                } else {
                    (0.0, 0.0)
                };
                value += noise.get_value_2d(x * factor + ox, y * factor + oy) * value_factor;
            }
            // Stepped even for a missing octave — the frequency ladder is a
            // property of the index, not of which slots are occupied.
            factor /= 2.0;
            value_factor *= 2.0;
        }

        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_zero_octave_has_unit_factors() {
        // octaves = 1, so the input factor is 2^0 and the value factor is
        // 1/(2^1 - 1). Both one, which is why `BIOME_INFO_NOISE.getValue`
        // is exactly one SimplexNoise sample.
        let noise = PerlinSimplexNoise::single_octave(2345);
        assert_eq!(noise.noise_levels.len(), 1);
        assert!((noise.highest_freq_input_factor - 1.0).abs() < f64::EPSILON);
        assert!((noise.highest_freq_value_factor - 1.0).abs() < f64::EPSILON);

        let mut direct_random = WorldgenRandom::new(LegacyRandomSource::new(2345));
        let direct = SimplexNoise::new(&mut direct_random);
        for (x, y) in [(0.0, 0.0), (13.5, -7.25), (1000.125, 4.0)] {
            assert_eq!(noise.get_value(x, y, false), direct.get_value_2d(x, y));
        }
    }

    #[test]
    fn the_frozen_stack_is_three_octaves_topped_at_index_zero() {
        // `[-2, -1, 0]`: first = -2 so lowFreq = 2, last = 0 so highFreq =
        // 0, giving three octaves with the zero octave at index 0. No
        // derived random source, because highFreq is not positive.
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(3456));
        let noise = PerlinSimplexNoise::new(&mut random, &[-2, -1, 0]);
        assert_eq!(noise.noise_levels.len(), 3);
        assert!(noise.noise_levels.iter().all(Option::is_some));
        assert!((noise.highest_freq_input_factor - 1.0).abs() < f64::EPSILON);
        assert!((noise.highest_freq_value_factor - 1.0 / 7.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_gap_in_the_octave_set_burns_draws_rather_than_skipping() {
        // The whole point of `consumeCount(262)`. `[0, -2]` leaves index 1
        // empty; the octave at index 2 must still be seeded from the same
        // stream position it would have had if index 1 were present.
        let mut with_gap = WorldgenRandom::new(LegacyRandomSource::new(99));
        let gapped = PerlinSimplexNoise::new(&mut with_gap, &[0, -2]);
        assert_eq!(gapped.noise_levels.len(), 3);
        assert!(gapped.noise_levels[1].is_none());

        let mut without_gap = WorldgenRandom::new(LegacyRandomSource::new(99));
        let full = PerlinSimplexNoise::new(&mut without_gap, &[0, -1, -2]);
        assert_eq!(
            gapped.noise_levels[2].as_ref().unwrap().xo,
            full.noise_levels[2].as_ref().unwrap().xo,
            "the skipped octave did not consume the draws its presence would have"
        );
    }

    #[test]
    fn the_stack_is_deterministic_and_seed_dependent() {
        let a = PerlinSimplexNoise::single_octave(1234);
        let b = PerlinSimplexNoise::single_octave(3456);
        let sample = |n: &PerlinSimplexNoise| {
            (0..32)
                .map(|i| n.get_value(f64::from(i) * 0.05, f64::from(i) * -0.11, false))
                .collect::<Vec<_>>()
        };
        assert_eq!(sample(&a), sample(&a));
        assert_ne!(sample(&a), sample(&b));
    }

    #[test]
    #[should_panic(expected = "Need some octaves!")]
    fn an_empty_octave_set_is_rejected() {
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(0));
        let _ = PerlinSimplexNoise::new(&mut random, &[]);
    }
}
