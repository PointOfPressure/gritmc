//! The End's island field.
//!
//! Reference: `net.minecraft.world.level.levelgen.DensityFunctions
//! .EndIslandDensityFunction`.
//!
//! Not noise in the usual sense: it scans a 25x25 neighbourhood of chunk
//! positions, keeps the ones where a simplex sample dips below −0.9, and
//! renders each as a cone of a pseudo-random radius. That is why End islands
//! are discrete blobs with hard edges rather than a continuous surface, and
//! why they only start past a radius of 64 chunks — the `> 4096` test on
//! squared chunk distance is what keeps the central island clear.

use crate::math::{java_max_f32, java_min_f32};
use crate::noise::SimplexNoise;
use crate::rng::{LegacyRandomSource, RandomSource};

/// A simplex sample below this marks a chunk as an island seed.
const ISLAND_THRESHOLD: f32 = -0.9;

/// `Mth.clamp(float, float, float)` — same shape as the `f64` one.
fn clamp_f32(value: f32, min: f32, max: f32) -> f32 {
    if value < min {
        min
    } else {
        java_min_f32(value, max)
    }
}

/// `Mth.sqrt(float)` is `(float) Math.sqrt(value)` — the square root is
/// taken in **double** and then narrowed.
fn mth_sqrt(value: f32) -> f32 {
    f64::from(value).sqrt() as f32
}

#[derive(Clone, Debug)]
pub struct EndIslandNoise {
    island_noise: SimplexNoise,
}

impl EndIslandNoise {
    /// `new EndIslandDensityFunction(seed)`.
    ///
    /// The 17292 burned draws are not decoration — they are what makes the
    /// End's islands independent of everything else seeded from the same
    /// world seed. The count is behavior; changing it regenerates the
    /// dimension.
    #[must_use]
    pub fn new(seed: i64) -> Self {
        let mut random = LegacyRandomSource::new(seed);
        random.consume_count(17292);
        Self {
            island_noise: SimplexNoise::new(&mut random),
        }
    }

    /// `getHeightValue`.
    ///
    /// Note `section / 2` and (in [`Self::compute`]) `block / 8` are Java
    /// `int` division, which **truncates toward zero** — unlike the
    /// aquifer lattice, which floors. Rust's `/` truncates too, so plain
    /// division is the faithful port here and `div_euclid` would be the bug.
    #[must_use]
    pub fn height_value(&self, section_x: i32, section_z: i32) -> f32 {
        let chunk_x = section_x / 2;
        let chunk_z = section_z / 2;
        // Java `%` takes the sign of the dividend, as does Rust's.
        let sub_section_x = section_x % 2;
        let sub_section_z = section_z % 2;

        // Distance falloff from the world origin, before any island.
        let squared = section_x
            .wrapping_mul(section_x)
            .wrapping_add(section_z.wrapping_mul(section_z));
        let mut doffs = 100.0 - mth_sqrt(squared as f32) * 8.0;
        doffs = clamp_f32(doffs, -100.0, 80.0);

        for xo in -12..=12i32 {
            for zo in -12..=12i32 {
                let total_chunk_x = i64::from(chunk_x) + i64::from(xo);
                let total_chunk_z = i64::from(chunk_z) + i64::from(zo);

                // 4096 = 64², so nothing generates within 64 chunks of the
                // origin — that is the gap around the main island.
                if total_chunk_x * total_chunk_x + total_chunk_z * total_chunk_z > 4096
                    && (self
                        .island_noise
                        .get_value_2d(total_chunk_x as f64, total_chunk_z as f64)
                        as f32)
                        < ISLAND_THRESHOLD
                {
                    // A cheap positional hash, in f32 with a float modulo,
                    // giving a radius in [9, 22).
                    let island_size = ((total_chunk_x as f32).abs() * 3439.0
                        + (total_chunk_z as f32).abs() * 147.0)
                        % 13.0
                        + 9.0;
                    let xd = (sub_section_x - xo * 2) as f32;
                    let zd = (sub_section_z - zo * 2) as f32;
                    let mut new_doffs = 100.0 - mth_sqrt(xd * xd + zd * zd) * island_size;
                    new_doffs = clamp_f32(new_doffs, -100.0, 80.0);
                    doffs = java_max_f32(doffs, new_doffs);
                }
            }
        }

        doffs
    }

    /// `compute` — height field rescaled into density.
    #[must_use]
    pub fn compute(&self, block_x: i32, block_z: i32) -> f64 {
        (f64::from(self.height_value(block_x / 8, block_z / 8)) - 8.0) / 128.0
    }

    /// Vanilla hard-codes these rather than deriving them from the clamp.
    #[must_use]
    pub const fn min_value() -> f64 {
        -0.84375
    }

    #[must_use]
    pub const fn max_value() -> f64 {
        0.5625
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_centre_is_clear_of_islands() {
        // Squared chunk distance must exceed 4096 (64 chunks) before any
        // island is considered, so the middle is pure distance falloff.
        let end = EndIslandNoise::new(0);
        let centre = end.compute(0, 0);
        // 100 clamped to 80, then (80 - 8)/128.
        assert!((centre - (80.0 - 8.0) / 128.0).abs() < 1e-12);
    }

    #[test]
    fn density_falls_off_away_from_the_origin() {
        let end = EndIslandNoise::new(0);
        let near = end.compute(0, 0);
        let far = end.compute(600, 600);
        assert!(far < near, "expected falloff: {near} -> {far}");
    }

    #[test]
    fn stated_bounds_hold_over_a_wide_scan() {
        // The bounds are hard-coded in vanilla; check the field actually
        // respects them rather than trusting the constants.
        let end = EndIslandNoise::new(20260708);
        for x in (-4000..4000).step_by(311) {
            for z in (-4000..4000).step_by(457) {
                let v = end.compute(x, z);
                assert!(
                    v >= EndIslandNoise::min_value() && v <= EndIslandNoise::max_value(),
                    "compute({x}, {z}) = {v} outside the declared bounds"
                );
            }
        }
    }

    #[test]
    fn section_division_truncates_rather_than_floors() {
        // The contrast with the aquifer lattice: -1 / 8 is 0 here, not -1.
        // Using div_euclid would shift the whole field by a section for
        // every negative coordinate.
        assert_eq!(-1i32 / 8, 0);
        assert_eq!((-1i32).div_euclid(8), -1);
        let end = EndIslandNoise::new(1);
        // Blocks -1 and 0 share a section, so they must agree.
        assert_eq!(end.compute(-1, -1).to_bits(), end.compute(0, 0).to_bits());
    }
}
