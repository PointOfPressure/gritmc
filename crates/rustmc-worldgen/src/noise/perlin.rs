//! Octave-summed Perlin noise.
//!
//! Reference: `net.minecraft.world.level.levelgen.synth.PerlinNoise`.

use super::improved::ImprovedNoise;
use crate::math::lfloor;
use crate::rng::{PositionalRandomFactory, RandomSource};

/// `PerlinNoise.ROUND_OFF`, as the `double` the wrap actually uses.
const ROUND_OFF: f64 = 3.355_443_2e7;

/// Folds a coordinate back toward the origin on a 2²⁵ lattice.
///
/// Perlin noise degrades once the input exceeds the precision of the
/// `floor`/fraction split, so vanilla wraps first. The wrap is periodic,
/// which means terrain repeats at that scale — a vanilla property, not a
/// bug to fix.
#[must_use]
pub fn wrap(x: f64) -> f64 {
    // `lfloor` yields a long; the multiply is long * double.
    x - lfloor(x / ROUND_OFF + 0.5) as f64 * ROUND_OFF
}

/// A stack of [`ImprovedNoise`] octaves with per-octave amplitudes.
///
/// `octaves[i] == None` means that octave's amplitude is zero. Vanilla keeps
/// the slot (rather than compacting) because the index maps to a frequency,
/// and it *still* advances the RNG for a skipped octave on the legacy path —
/// the hole is load-bearing.
#[derive(Clone, Debug)]
pub struct PerlinNoise {
    octaves: Vec<Option<ImprovedNoise>>,
    amplitudes: Vec<f64>,
    first_octave: i32,
    lowest_freq_input_factor: f64,
    lowest_freq_value_factor: f64,
    max_value: f64,
}

impl PerlinNoise {
    /// `PerlinNoise.create(random, firstOctave, amplitudes)` — the modern
    /// path, where each octave is seeded from a positional fork keyed by
    /// `"octave_<n>"` rather than by consuming the parent stream in order.
    /// That is what makes octave seeding independent of how many octaves
    /// precede it.
    pub fn create<R: ForkPositional>(
        random: &mut R,
        first_octave: i32,
        amplitudes: &[f64],
    ) -> Self {
        let positional = random.fork_positional_source();
        let octaves = amplitudes
            .iter()
            .enumerate()
            .map(|(i, &amplitude)| {
                (amplitude != 0.0).then(|| {
                    let octave = first_octave + i as i32;
                    let mut source = positional.from_hash_of(&format!("octave_{octave}"));
                    ImprovedNoise::new(&mut source)
                })
            })
            .collect();

        Self::assemble(octaves, amplitudes.to_vec(), first_octave)
    }

    /// `makeAmplitudes` + `create` — build from a set of octave indices, all
    /// at amplitude 1.
    ///
    /// # Panics
    /// If `octave_set` is empty (vanilla throws "Need some octaves!").
    pub fn from_octaves<R: RandomSource + ForkPositional>(
        random: &mut R,
        octave_set: &[i32],
    ) -> Self {
        let (first_octave, amplitudes) = make_amplitudes(octave_set);
        Self::create(random, first_octave, &amplitudes)
    }

    /// `createLegacyForBlendedNoise` — the pre-1.18 initialization, still
    /// live because the overworld's terrain noise is a `BlendedNoise`.
    ///
    /// # Panics
    /// If `octave_set` is empty.
    pub fn create_legacy_for_blended_noise<R: RandomSource>(
        random: &mut R,
        octave_set: &[i32],
    ) -> Self {
        let (first_octave, amplitudes) = make_amplitudes(octave_set);
        Self::legacy(random, first_octave, amplitudes)
    }

    /// `createLegacyForLegacyNetherBiome`.
    pub fn create_legacy_for_legacy_nether_biome<R: RandomSource>(
        random: &mut R,
        first_octave: i32,
        amplitudes: &[f64],
    ) -> Self {
        Self::legacy(random, first_octave, amplitudes.to_vec())
    }

    /// The legacy octave layout.
    ///
    /// Two things differ from [`Self::create`] and both are observable:
    ///
    /// 1. Octaves are drawn **sequentially from the parent stream**, not from
    ///    a positional fork — so an octave's noise depends on how many
    ///    octaves precede it.
    /// 2. Construction runs from the zero octave *downward*: index
    ///    `-first_octave` first, then `-first_octave - 1 .. 0`. A
    ///    zero-amplitude slot still burns the draws an octave would have
    ///    cost (`skip_octave`), so the stream position stays aligned.
    ///
    /// # Panics
    /// If the built octaves disagree with the non-zero amplitude count, or
    /// if positive octaves are requested (vanilla rejects both).
    fn legacy<R: RandomSource>(random: &mut R, first_octave: i32, amplitudes: Vec<f64>) -> Self {
        let count = amplitudes.len();
        let zero_octave_index = -first_octave;
        let mut octaves: Vec<Option<ImprovedNoise>> = vec![None; count];

        // Built unconditionally — the draws happen even when the slot is
        // out of range or its amplitude is zero.
        let zero_octave = ImprovedNoise::new(random);
        if zero_octave_index >= 0 && (zero_octave_index as usize) < count {
            let index = zero_octave_index as usize;
            if amplitudes[index] != 0.0 {
                octaves[index] = Some(zero_octave);
            }
        }

        for index in (0..zero_octave_index).rev() {
            let index = index as usize;
            if index < count && amplitudes[index] != 0.0 {
                octaves[index] = Some(ImprovedNoise::new(random));
            } else {
                skip_octave(random);
            }
        }

        let built = octaves.iter().filter(|o| o.is_some()).count();
        let expected = amplitudes.iter().filter(|&&a| a != 0.0).count();
        assert_eq!(
            built, expected,
            "Failed to create correct number of noise levels for given non-zero amplitudes"
        );
        assert!(
            zero_octave_index >= count as i32 - 1,
            "Positive octaves are temporarily disabled"
        );

        Self::assemble(octaves, amplitudes, first_octave)
    }

    fn assemble(
        octaves: Vec<Option<ImprovedNoise>>,
        amplitudes: Vec<f64>,
        first_octave: i32,
    ) -> Self {
        let count = amplitudes.len();
        let zero_octave_index = -first_octave;

        // Both exponents are integral and the base is 2, so these are exact
        // in f64 and agree with `Math.pow` bit-for-bit.
        let lowest_freq_input_factor = 2.0_f64.powi(-zero_octave_index);
        let lowest_freq_value_factor =
            2.0_f64.powi(count as i32 - 1) / (2.0_f64.powi(count as i32) - 1.0);

        let mut noise = Self {
            octaves,
            amplitudes,
            first_octave,
            lowest_freq_input_factor,
            lowest_freq_value_factor,
            max_value: 0.0,
        };
        noise.max_value = noise.edge_value(2.0);
        noise
    }

    #[must_use]
    pub fn max_value(&self) -> f64 {
        self.max_value
    }

    #[must_use]
    pub fn first_octave(&self) -> i32 {
        self.first_octave
    }

    #[must_use]
    pub fn amplitudes(&self) -> &[f64] {
        &self.amplitudes
    }

    /// `getValue(x, y, z)`.
    #[must_use]
    pub fn get_value(&self, x: f64, y: f64, z: f64) -> f64 {
        self.get_value_with_fudge(x, y, z, 0.0, 0.0)
    }

    /// `getValue(x, y, z, yScale, yFudge)` — the deprecated overload.
    ///
    /// The accumulation order matters: `value += amplitude * noise * factor`
    /// per octave, low frequency first. Summing in any other order, or
    /// factoring the multiply out, changes the last bits.
    #[must_use]
    pub fn get_value_with_fudge(&self, x: f64, y: f64, z: f64, y_scale: f64, y_fudge: f64) -> f64 {
        let mut value = 0.0;
        let mut factor = self.lowest_freq_input_factor;
        let mut value_factor = self.lowest_freq_value_factor;

        for (i, octave) in self.octaves.iter().enumerate() {
            if let Some(noise) = octave {
                let sample = noise.noise_with_fudge(
                    wrap(x * factor),
                    wrap(y * factor),
                    wrap(z * factor),
                    y_scale * factor,
                    y_fudge * factor,
                );
                value += self.amplitudes[i] * sample * value_factor;
            }
            factor *= 2.0;
            value_factor /= 2.0;
        }

        value
    }

    /// `maxBrokenValue` — the bound used by the legacy blended path.
    #[must_use]
    pub fn max_broken_value(&self, y_scale: f64) -> f64 {
        self.edge_value(y_scale + 2.0)
    }

    fn edge_value(&self, noise_value: f64) -> f64 {
        let mut value = 0.0;
        let mut value_factor = self.lowest_freq_value_factor;
        for (i, octave) in self.octaves.iter().enumerate() {
            if octave.is_some() {
                value += self.amplitudes[i] * noise_value * value_factor;
            }
            value_factor /= 2.0;
        }
        value
    }

    /// `getOctaveNoise(i)` — indexed from the **highest** frequency down,
    /// the reverse of the internal order.
    #[must_use]
    pub fn octave_noise(&self, i: usize) -> Option<&ImprovedNoise> {
        self.octaves[self.octaves.len() - 1 - i].as_ref()
    }
}

/// `PerlinNoise.skipOctave` — burn exactly what an `ImprovedNoise`
/// construction costs on the legacy LCG: 3 `nextDouble` (2 draws each) plus
/// 256 `nextInt` = 262.
fn skip_octave<R: RandomSource>(random: &mut R) {
    random.consume_count(262);
}

/// `PerlinNoise.makeAmplitudes` — turn a set of octave indices into a
/// contiguous amplitude list plus its first octave.
///
/// # Panics
/// If `octave_set` is empty, or the span works out below one octave.
#[must_use]
pub fn make_amplitudes(octave_set: &[i32]) -> (i32, Vec<f64>) {
    assert!(!octave_set.is_empty(), "Need some octaves!");

    // Vanilla feeds this through an IntRBTreeSet, so duplicates collapse and
    // the ends are the sorted extremes — not first/last as given.
    let low_freq_octaves = -octave_set.iter().copied().min().expect("non-empty");
    let high_freq_octaves = octave_set.iter().copied().max().expect("non-empty");
    let count = low_freq_octaves + high_freq_octaves + 1;
    assert!(count >= 1, "Total number of octaves needs to be >= 1");

    let mut amplitudes = vec![0.0; count as usize];
    for &octave in octave_set {
        amplitudes[(octave + low_freq_octaves) as usize] = 1.0;
    }
    (-low_freq_octaves, amplitudes)
}

/// Sources that can hand out a positional factory by value.
///
/// A small shim so [`PerlinNoise::create`] can stay generic: `fork_positional`
/// is an inherent method with a different concrete return type per source,
/// and vanilla reaches it through the `RandomSource` interface.
pub trait ForkPositional: RandomSource {
    type Factory: PositionalRandomFactory;
    fn fork_positional_source(&mut self) -> Self::Factory;
}

impl ForkPositional for crate::rng::XoroshiroRandomSource {
    type Factory = crate::rng::XoroshiroPositionalFactory;
    fn fork_positional_source(&mut self) -> Self::Factory {
        self.fork_positional()
    }
}

impl ForkPositional for crate::rng::LegacyRandomSource {
    type Factory = crate::rng::LegacyPositionalFactory;
    fn fork_positional_source(&mut self) -> Self::Factory {
        self.fork_positional()
    }
}

/// So a noise can be instantiated from whichever algorithm the dimension's
/// noise settings named — `PerlinNoise.create` forks positionally once per
/// octave, and for a legacy dimension those forks must be legacy ones.
impl ForkPositional for crate::rng::AnyRandomSource {
    type Factory = crate::rng::AnyPositionalFactory;
    fn fork_positional_source(&mut self) -> Self::Factory {
        match self {
            Self::Xoroshiro(r) => Self::Factory::Xoroshiro(r.fork_positional()),
            Self::Legacy(r) => Self::Factory::Legacy(r.fork_positional()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_is_identity_near_the_origin() {
        for x in [0.0, 1.0, -1.0, 1234.5, -9876.25] {
            assert_eq!(wrap(x).to_bits(), x.to_bits(), "wrap moved {x}");
        }
    }

    #[test]
    fn wrap_folds_large_coordinates() {
        let far = 5.0e7;
        assert!(wrap(far).abs() < ROUND_OFF, "wrap left {far} unfolded");
    }

    #[test]
    fn make_amplitudes_matches_the_octave_set() {
        // -3..=0 -> four slots, first octave -3, all ones.
        let (first, amplitudes) = make_amplitudes(&[-3, -2, -1, 0]);
        assert_eq!(first, -3);
        assert_eq!(amplitudes, vec![1.0, 1.0, 1.0, 1.0]);

        // A gap leaves a zero-amplitude hole rather than compacting.
        let (first, amplitudes) = make_amplitudes(&[-2, 0]);
        assert_eq!(first, -2);
        assert_eq!(amplitudes, vec![1.0, 0.0, 1.0]);
    }
}
