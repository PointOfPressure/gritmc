//! Blended noise — the overworld's terrain-shape field.
//!
//! Reference: `net.minecraft.world.level.levelgen.synth.BlendedNoise`.
//!
//! Three legacy Perlin stacks: a `min` and `max` limit surface, and a `main`
//! selector that blends between them. This is pre-1.18 terrain machinery
//! that modern worldgen still calls through the `old_blended_noise` density
//! function, so it is on the overworld critical path rather than a
//! compatibility relic.

use super::improved::ImprovedNoise;
use super::perlin::{wrap, PerlinNoise};
use crate::math::clamped_lerp;
use crate::rng::RandomSource;

/// The historical coordinate scale. Not a round number, and not derived from
/// anything — it is simply the constant the world was generated with.
const BASE_SCALE: f64 = 684.412;

/// `BlendedNoise`.
#[derive(Clone, Debug)]
pub struct BlendedNoise {
    min_limit_noise: PerlinNoise,
    max_limit_noise: PerlinNoise,
    main_noise: PerlinNoise,
    xz_multiplier: f64,
    y_multiplier: f64,
    xz_factor: f64,
    y_factor: f64,
    smear_scale_multiplier: f64,
    max_value: f64,
}

impl BlendedNoise {
    /// The `@VisibleForTesting` constructor — the one real worldgen uses via
    /// `withNewRandom`.
    ///
    /// Octave counts are fixed: 16 for each limit stack (−15..=0) and 8 for
    /// the selector (−7..=0). All three come off the **same** source in that
    /// order, so their streams interleave; building them independently gives
    /// a different world.
    pub fn new<R: RandomSource>(
        random: &mut R,
        xz_scale: f64,
        y_scale: f64,
        xz_factor: f64,
        y_factor: f64,
        smear_scale_multiplier: f64,
    ) -> Self {
        let limit_octaves: Vec<i32> = (-15..=0).collect();
        let main_octaves: Vec<i32> = (-7..=0).collect();

        let min_limit_noise = PerlinNoise::create_legacy_for_blended_noise(random, &limit_octaves);
        let max_limit_noise = PerlinNoise::create_legacy_for_blended_noise(random, &limit_octaves);
        let main_noise = PerlinNoise::create_legacy_for_blended_noise(random, &main_octaves);

        let xz_multiplier = BASE_SCALE * xz_scale;
        let y_multiplier = BASE_SCALE * y_scale;
        let max_value = min_limit_noise.max_broken_value(y_multiplier);

        Self {
            min_limit_noise,
            max_limit_noise,
            main_noise,
            xz_multiplier,
            y_multiplier,
            xz_factor,
            y_factor,
            smear_scale_multiplier,
            max_value,
        }
    }

    /// `createUnseeded` — the codec's default, seeded from Xoroshiro(0).
    #[must_use]
    pub fn create_unseeded(
        xz_scale: f64,
        y_scale: f64,
        xz_factor: f64,
        y_factor: f64,
        smear_scale_multiplier: f64,
    ) -> Self {
        let mut random = crate::rng::XoroshiroRandomSource::from_seed(0);
        Self::new(
            &mut random,
            xz_scale,
            y_scale,
            xz_factor,
            y_factor,
            smear_scale_multiplier,
        )
    }

    #[must_use]
    pub fn max_value(&self) -> f64 {
        self.max_value
    }

    #[must_use]
    pub fn min_value(&self) -> f64 {
        -self.max_value
    }

    /// `compute(FunctionContext)` — block coordinates in, density out.
    #[must_use]
    pub fn compute(&self, block_x: i32, block_y: i32, block_z: i32) -> f64 {
        let limit_x = f64::from(block_x) * self.xz_multiplier;
        let limit_y = f64::from(block_y) * self.y_multiplier;
        let limit_z = f64::from(block_z) * self.xz_multiplier;

        let main_x = limit_x / self.xz_factor;
        let main_y = limit_y / self.y_factor;
        let main_z = limit_z / self.xz_factor;

        let limit_smear = self.y_multiplier * self.smear_scale_multiplier;
        let main_smear = limit_smear / self.y_factor;

        // Selector pass. Note the y-fudge argument is the *unwrapped*
        // `main_y * pow` while the coordinates are wrapped — passing the
        // wrapped value to both is the natural mistake and changes terrain.
        let mut main_value = 0.0;
        let mut pow = 1.0;
        for i in 0..8 {
            if let Some(noise) = self.main_noise.octave_noise(i) {
                main_value += noise.noise_with_fudge(
                    wrap(main_x * pow),
                    wrap(main_y * pow),
                    wrap(main_z * pow),
                    main_smear * pow,
                    main_y * pow,
                ) / pow;
            }
            pow /= 2.0;
        }

        let factor = (main_value / 10.0 + 1.0) / 2.0;
        // Saturated selector: only one limit surface can matter, so vanilla
        // skips sampling the other. Sampling is pure, so this is an
        // optimization that cannot change the result — clamped_lerp returns
        // exactly the surviving endpoint at these factors.
        let is_max = factor >= 1.0;
        let is_min = factor <= 0.0;

        let mut blend_min = 0.0;
        let mut blend_max = 0.0;
        let mut pow = 1.0;
        for i in 0..16 {
            let wx = wrap(limit_x * pow);
            let wy = wrap(limit_y * pow);
            let wz = wrap(limit_z * pow);
            let y_scale_pow = limit_smear * pow;

            if !is_max {
                if let Some(noise) = self.min_limit_noise.octave_noise(i) {
                    blend_min +=
                        noise.noise_with_fudge(wx, wy, wz, y_scale_pow, limit_y * pow) / pow;
                }
            }
            if !is_min {
                if let Some(noise) = self.max_limit_noise.octave_noise(i) {
                    blend_max +=
                        noise.noise_with_fudge(wx, wy, wz, y_scale_pow, limit_y * pow) / pow;
                }
            }
            pow /= 2.0;
        }

        clamped_lerp(factor, blend_min / 512.0, blend_max / 512.0) / 128.0
    }

    /// The selector stack, exposed for the density-function layer.
    #[must_use]
    pub fn main_octave(&self, i: usize) -> Option<&ImprovedNoise> {
        self.main_noise.octave_noise(i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::XoroshiroRandomSource;

    #[test]
    fn the_three_stacks_are_distinct() {
        // All three are drawn from one source in sequence, so they must not
        // be copies of each other — if they were, the selector would track
        // the limits exactly and terrain would collapse.
        let mut rng = XoroshiroRandomSource::from_seed(20260708);
        let noise = BlendedNoise::new(&mut rng, 1.0, 1.0, 80.0, 160.0, 8.0);
        let a = noise.min_limit_noise.octave_noise(0).expect("octave 0");
        let b = noise.max_limit_noise.octave_noise(0).expect("octave 0");
        let c = noise.main_noise.octave_noise(0).expect("octave 0");
        assert_ne!(a.xo.to_bits(), b.xo.to_bits());
        assert_ne!(b.xo.to_bits(), c.xo.to_bits());
    }

    #[test]
    fn density_is_bounded_by_max_value() {
        let mut rng = XoroshiroRandomSource::from_seed(1);
        let noise = BlendedNoise::new(&mut rng, 1.0, 1.0, 80.0, 160.0, 8.0);
        for (x, y, z) in [(0, 0, 0), (100, 64, -100), (-2048, -64, 2048)] {
            let value = noise.compute(x, y, z);
            assert!(
                value >= noise.min_value() && value <= noise.max_value(),
                "compute({x},{y},{z}) = {value} outside [{}, {}]",
                noise.min_value(),
                noise.max_value()
            );
        }
    }
}
