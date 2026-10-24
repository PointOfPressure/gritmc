//! The overworld terrain shaper.
//!
//! Reference: `net.minecraft.data.worldgen.TerrainProvider` (not in the
//! cached decompile — extract `TerrainProvider.class` from the server jar
//! and run vineflower on it to re-read).
//!
//! This is the file that decides what Minecraft *looks like*. Continents,
//! erosion, ridges and weirdness arrive as noise in roughly `[-1, 1]`; these
//! splines map them to the three numbers terrain generation actually uses:
//!
//! - **offset** — where the terrain surface sits vertically.
//! - **factor** — how strongly the 3D noise is squashed toward that surface;
//!   high factor means flat, low factor means dramatic.
//! - **jaggedness** — how much high-frequency spikiness is added on peaks.
//!
//! Every constant here is a tuning decision someone made about how the game
//! should feel, not a derivable quantity. They are transcribed exactly and
//! the local names from the reference are kept, because a "cleanup" here is
//! indistinguishable from a terrain bug.
//!
//! Everything is `f32` — see [`super::spline`].

use super::spline::{CubicSpline, SplineBuilder, ValueTransformer};
use super::DensityFunction;
use crate::math::{java_max_f32, java_min_f32, lerp_f32};

/// `TerrainProvider.peaksAndValleys` — folds weirdness into the
/// peaks-and-valleys coordinate.
///
/// Also exposed as a density function in `NoiseRouterData`; the two must
/// agree, so callers use this one.
#[must_use]
pub fn peaks_and_valleys(weirdness: f32) -> f32 {
    -((weirdness.abs() - 0.666_666_7).abs() - 0.333_333_34) * 3.0
}

fn calculate_slope(y1: f32, y2: f32, x1: f32, x2: f32) -> f32 {
    (y2 - y1) / (x2 - x1)
}

/// `mountainContinentalness`.
///
/// `allow_rivers_below` is the ridge value under which the result may go
/// negative — that negative band is what carves river valleys through
/// mountain ranges instead of damming them.
fn mountain_continentalness(ridge: f32, modulation: f32, allow_rivers_below: f32) -> f32 {
    const RIDGE_OFFSET: f32 = 1.17;
    const RIDGE_AMPLITUDE: f32 = 0.460_829_47;

    let ridge_slope = 1.0 - (1.0 - modulation) * 0.5;
    let ridge_intersect = 0.5 * (1.0 - modulation);
    let adjusted_ridge_height = (ridge + RIDGE_OFFSET) * RIDGE_AMPLITUDE;
    let continentalness = adjusted_ridge_height * ridge_slope - ridge_intersect;

    if ridge < allow_rivers_below {
        java_max_f32(continentalness, -0.2222)
    } else {
        java_max_f32(continentalness, 0.0)
    }
}

/// `calculateMountainRidgeZeroContinentalnessPoint`.
fn mountain_ridge_zero_continentalness_point(modulation: f32) -> f32 {
    const RIDGE_OFFSET: f32 = 1.17;
    const RIDGE_AMPLITUDE: f32 = 0.460_829_47;

    let ridge_slope = 1.0 - (1.0 - modulation) * 0.5;
    let ridge_intersect = 0.5 * (1.0 - modulation);
    ridge_intersect / (RIDGE_AMPLITUDE * ridge_slope) - RIDGE_OFFSET
}

/// `buildMountainRidgeSplineWithPoints`.
fn mountain_ridge_spline(
    ridges: &DensityFunction,
    modulation: f32,
    saddle: bool,
    transformer: ValueTransformer,
) -> CubicSpline {
    const ALLOW_RIVERS_BELOW: f32 = -0.7;
    let mut build = SplineBuilder::with_transformer(ridges.clone(), transformer);

    let min_point_continentalness = mountain_continentalness(-1.0, modulation, ALLOW_RIVERS_BELOW);
    let max_point_continentalness = mountain_continentalness(1.0, modulation, ALLOW_RIVERS_BELOW);
    let ridge_zero_point = mountain_ridge_zero_continentalness_point(modulation);

    if (-0.65..1.0).contains(&ridge_zero_point) && ridge_zero_point != -0.65 {
        // The curve crosses zero inside the ridge range, so it gets explicit
        // points either side of the river band.
        let after_river = mountain_continentalness(-0.65, modulation, ALLOW_RIVERS_BELOW);
        let before_river = mountain_continentalness(-0.75, modulation, ALLOW_RIVERS_BELOW);
        let min_point_derivative =
            calculate_slope(min_point_continentalness, before_river, -1.0, -0.75);

        build = build
            .point(-1.0, min_point_continentalness, min_point_derivative)
            .point(-0.75, before_river, 0.0)
            .point(-0.65, after_river, 0.0);

        let zero_point_continentalness =
            mountain_continentalness(ridge_zero_point, modulation, ALLOW_RIVERS_BELOW);
        let max_point_derivative = calculate_slope(
            zero_point_continentalness,
            max_point_continentalness,
            ridge_zero_point,
            1.0,
        );

        // The doubled point 0.01 apart is deliberate: it pins the curve flat
        // just before the zero crossing, then hands it the steep derivative.
        build = build
            .point(ridge_zero_point - 0.01, zero_point_continentalness, 0.0)
            .point(
                ridge_zero_point,
                zero_point_continentalness,
                max_point_derivative,
            )
            .point(1.0, max_point_continentalness, max_point_derivative);
    } else {
        let simple_derivative = calculate_slope(
            min_point_continentalness,
            max_point_continentalness,
            -1.0,
            1.0,
        );
        if saddle {
            build = build
                .point(-1.0, java_max_f32(0.2, min_point_continentalness), 0.0)
                .point(
                    0.0,
                    lerp_f32(0.5, min_point_continentalness, max_point_continentalness),
                    simple_derivative,
                );
        } else {
            build = build.point(-1.0, min_point_continentalness, simple_derivative);
        }
        build = build.point(1.0, max_point_continentalness, simple_derivative);
    }

    build.build()
}

/// `ridgeSpline` — the five-point valley/low/mid/high/peaks curve.
///
/// The derivatives are computed from the values so neighbouring segments
/// meet without a visible crease; `min_valley_steepness` stops the valley
/// floor from going completely flat.
#[allow(clippy::too_many_arguments)]
fn ridge_spline(
    ridges: &DensityFunction,
    valley: f32,
    low: f32,
    mid: f32,
    high: f32,
    peaks: f32,
    min_valley_steepness: f32,
    transformer: ValueTransformer,
) -> CubicSpline {
    let d1 = java_max_f32(0.5 * (low - valley), min_valley_steepness);
    let d2 = 5.0 * (mid - low);
    SplineBuilder::with_transformer(ridges.clone(), transformer)
        .point(-1.0, valley, d1)
        .point(-0.4, low, java_min_f32(d1, d2))
        .point(0.0, mid, d2)
        .point(0.4, high, 2.0 * (high - mid))
        .point(1.0, peaks, 0.7 * (peaks - high))
        .build()
}

/// `buildErosionOffsetSpline` — offset as a function of erosion, with each
/// erosion band carrying its own ridge curve.
#[allow(clippy::too_many_arguments)]
pub fn erosion_offset_spline(
    erosion: &DensityFunction,
    ridges: &DensityFunction,
    low_valley: f32,
    hill: f32,
    tall_hill: f32,
    mountain_factor: f32,
    plain: f32,
    swamp: f32,
    include_extreme_hills: bool,
    saddle: bool,
    transformer: ValueTransformer,
) -> CubicSpline {
    let very_low_erosion_mountains = mountain_ridge_spline(
        ridges,
        lerp_f32(mountain_factor, 0.6, 1.5),
        saddle,
        transformer,
    );
    let low_erosion_mountains = mountain_ridge_spline(
        ridges,
        lerp_f32(mountain_factor, 0.6, 1.0),
        saddle,
        transformer,
    );
    let mountains = mountain_ridge_spline(ridges, mountain_factor, saddle, transformer);

    let wide_plateau = ridge_spline(
        ridges,
        low_valley - 0.15,
        0.5 * mountain_factor,
        // Vanilla writes `Mth.lerp(0.5F, 0.5F, 0.5F)` here, which is 0.5.
        // Kept as the literal it evaluates to; the lerp carried no meaning.
        0.5 * mountain_factor,
        0.5 * mountain_factor,
        0.6 * mountain_factor,
        0.5,
        transformer,
    );
    let narrow_plateau = ridge_spline(
        ridges,
        low_valley,
        plain * mountain_factor,
        hill * mountain_factor,
        0.5 * mountain_factor,
        0.6 * mountain_factor,
        0.5,
        transformer,
    );
    let plains = ridge_spline(
        ridges,
        low_valley,
        plain,
        plain,
        hill,
        tall_hill,
        0.5,
        transformer,
    );
    let plains_far_inland = ridge_spline(
        ridges,
        low_valley,
        plain,
        plain,
        hill,
        tall_hill,
        0.5,
        transformer,
    );
    let extreme_hills = SplineBuilder::with_transformer(ridges.clone(), transformer)
        .point(-1.0, low_valley, 0.0)
        .spline_point(-0.4, plains.clone(), 0.0)
        .point(0.0, tall_hill + 0.07, 0.0)
        .build();
    let swamps = ridge_spline(
        ridges,
        -0.02,
        swamp,
        swamp,
        hill,
        tall_hill,
        0.0,
        transformer,
    );

    let mut builder = SplineBuilder::with_transformer(erosion.clone(), transformer)
        .spline_point(-0.85, very_low_erosion_mountains, 0.0)
        .spline_point(-0.7, low_erosion_mountains, 0.0)
        .spline_point(-0.4, mountains, 0.0)
        .spline_point(-0.35, wide_plateau, 0.0)
        .spline_point(-0.1, narrow_plateau, 0.0)
        .spline_point(0.2, plains, 0.0);

    if include_extreme_hills {
        builder = builder
            .spline_point(0.4, plains_far_inland.clone(), 0.0)
            .spline_point(0.45, extreme_hills.clone(), 0.0)
            .spline_point(0.55, extreme_hills, 0.0)
            .spline_point(0.58, plains_far_inland, 0.0);
    }

    builder.spline_point(0.7, swamps, 0.0).build()
}

/// `getErosionFactor`.
fn erosion_factor(
    erosion: &DensityFunction,
    weirdness: &DensityFunction,
    ridges: &DensityFunction,
    base_value: f32,
    shattered_terrain: bool,
    transformer: ValueTransformer,
) -> CubicSpline {
    let base_spline = SplineBuilder::with_transformer(weirdness.clone(), transformer)
        .point(-0.2, 6.3, 0.0)
        .point(0.2, base_value, 0.0)
        .build();

    let mut points = SplineBuilder::with_transformer(erosion.clone(), transformer)
        .spline_point(-0.6, base_spline.clone(), 0.0)
        .spline_point(
            -0.5,
            SplineBuilder::with_transformer(weirdness.clone(), transformer)
                .point(-0.05, 6.3, 0.0)
                .point(0.05, 2.67, 0.0)
                .build(),
            0.0,
        )
        .spline_point(-0.35, base_spline.clone(), 0.0)
        .spline_point(-0.25, base_spline.clone(), 0.0)
        .spline_point(
            -0.1,
            SplineBuilder::with_transformer(weirdness.clone(), transformer)
                .point(-0.05, 2.67, 0.0)
                .point(0.05, 6.3, 0.0)
                .build(),
            0.0,
        )
        .spline_point(0.03, base_spline.clone(), 0.0);

    if shattered_terrain {
        let weirdness_shattered = SplineBuilder::with_transformer(weirdness.clone(), transformer)
            .point(0.0, base_value, 0.0)
            .point(0.1, 0.625, 0.0)
            .build();
        let ridges_shattered = SplineBuilder::with_transformer(ridges.clone(), transformer)
            .point(-0.9, base_value, 0.0)
            .spline_point(-0.69, weirdness_shattered, 0.0)
            .build();
        points = points
            .point(0.35, base_value, 0.0)
            .spline_point(0.45, ridges_shattered.clone(), 0.0)
            .spline_point(0.55, ridges_shattered, 0.0)
            .point(0.62, base_value, 0.0);
    } else {
        let extreme_hills_from_mid_slice_up =
            SplineBuilder::with_transformer(ridges.clone(), transformer)
                .spline_point(-0.7, base_spline.clone(), 0.0)
                .point(-0.15, 1.37, 0.0)
                .build();
        let extra_3d_noise_on_peaks_only =
            SplineBuilder::with_transformer(ridges.clone(), transformer)
                .spline_point(0.45, base_spline, 0.0)
                .point(0.7, 1.56, 0.0)
                .build();
        points = points
            .spline_point(0.05, extra_3d_noise_on_peaks_only.clone(), 0.0)
            .spline_point(0.4, extra_3d_noise_on_peaks_only, 0.0)
            .spline_point(0.45, extreme_hills_from_mid_slice_up.clone(), 0.0)
            .spline_point(0.55, extreme_hills_from_mid_slice_up, 0.0)
            .point(0.58, base_value, 0.0);
    }

    points.build()
}

/// `buildWeirdnessJaggednessSpline`.
fn weirdness_jaggedness_spline(
    weirdness: &DensityFunction,
    jaggedness_factor: f32,
    transformer: ValueTransformer,
) -> CubicSpline {
    SplineBuilder::with_transformer(weirdness.clone(), transformer)
        .point(-0.01, 0.63 * jaggedness_factor, 0.0)
        .point(0.01, 0.3 * jaggedness_factor, 0.0)
        .build()
}

/// `buildRidgeJaggednessSpline`.
fn ridge_jaggedness_spline(
    weirdness: &DensityFunction,
    ridges: &DensityFunction,
    factor_at_peak_ridge: f32,
    factor_at_high_ridge: f32,
    transformer: ValueTransformer,
) -> CubicSpline {
    // The slice bounds come from peaks_and_valleys, so jaggedness lines up
    // with where the peaks actually are rather than a hand-picked ridge value.
    let high_slice_start = peaks_and_valleys(0.4);
    let high_slice_end = peaks_and_valleys(0.566_666_66);
    let high_slice_middle = (high_slice_start + high_slice_end) / 2.0;

    let mut builder = SplineBuilder::with_transformer(ridges.clone(), transformer).point(
        high_slice_start,
        0.0,
        0.0,
    );

    builder = if factor_at_high_ridge > 0.0 {
        builder.spline_point(
            high_slice_middle,
            weirdness_jaggedness_spline(weirdness, factor_at_high_ridge, transformer),
            0.0,
        )
    } else {
        builder.point(high_slice_middle, 0.0, 0.0)
    };

    builder = if factor_at_peak_ridge > 0.0 {
        builder.spline_point(
            1.0,
            weirdness_jaggedness_spline(weirdness, factor_at_peak_ridge, transformer),
            0.0,
        )
    } else {
        builder.point(1.0, 0.0, 0.0)
    };

    builder.build()
}

/// `buildErosionJaggednessSpline`.
#[allow(clippy::too_many_arguments)]
fn erosion_jaggedness_spline(
    erosion: &DensityFunction,
    weirdness: &DensityFunction,
    ridges: &DensityFunction,
    factor_at_peak_ridge_erosion0: f32,
    factor_at_peak_ridge_erosion1: f32,
    factor_at_high_ridge_erosion0: f32,
    factor_at_high_ridge_erosion1: f32,
    transformer: ValueTransformer,
) -> CubicSpline {
    let at_erosion0 = ridge_jaggedness_spline(
        weirdness,
        ridges,
        factor_at_peak_ridge_erosion0,
        factor_at_high_ridge_erosion0,
        transformer,
    );
    let at_erosion1 = ridge_jaggedness_spline(
        weirdness,
        ridges,
        factor_at_peak_ridge_erosion1,
        factor_at_high_ridge_erosion1,
        transformer,
    );
    SplineBuilder::with_transformer(erosion.clone(), transformer)
        .spline_point(-1.0, at_erosion0, 0.0)
        .spline_point(-0.78, at_erosion1.clone(), 0.0)
        .spline_point(-0.5775, at_erosion1, 0.0)
        .point(-0.375, 0.0, 0.0)
        .build()
}

/// `TerrainProvider.overworldOffset`.
#[must_use]
pub fn overworld_offset(
    continents: &DensityFunction,
    erosion: &DensityFunction,
    ridges: &DensityFunction,
    amplified: bool,
) -> CubicSpline {
    let transformer = if amplified {
        ValueTransformer::AmplifiedOffset
    } else {
        ValueTransformer::Identity
    };

    let beach = erosion_offset_spline(
        erosion,
        ridges,
        -0.15,
        0.0,
        0.0,
        0.1,
        0.0,
        -0.03,
        false,
        false,
        transformer,
    );
    let low = erosion_offset_spline(
        erosion,
        ridges,
        -0.1,
        0.03,
        0.1,
        0.1,
        0.01,
        -0.03,
        false,
        false,
        transformer,
    );
    let mid = erosion_offset_spline(
        erosion,
        ridges,
        -0.1,
        0.03,
        0.1,
        0.7,
        0.01,
        -0.03,
        true,
        true,
        transformer,
    );
    let high = erosion_offset_spline(
        erosion,
        ridges,
        -0.05,
        0.03,
        0.1,
        1.0,
        0.01,
        0.01,
        true,
        true,
        transformer,
    );

    SplineBuilder::with_transformer(continents.clone(), transformer)
        .point(-1.1, 0.044, 0.0)
        .point(-1.02, -0.2222, 0.0)
        .point(-0.51, -0.2222, 0.0)
        .point(-0.44, -0.12, 0.0)
        .point(-0.18, -0.12, 0.0)
        // The beach spline appears twice, 0.01 apart: that is the coastline,
        // and the near-vertical step is what makes it a shore instead of a
        // gradual ramp.
        .spline_point(-0.16, beach.clone(), 0.0)
        .spline_point(-0.15, beach, 0.0)
        .spline_point(-0.1, low, 0.0)
        .spline_point(0.25, mid, 0.0)
        .spline_point(1.0, high, 0.0)
        .build()
}

/// `TerrainProvider.overworldFactor`.
#[must_use]
pub fn overworld_factor(
    continents: &DensityFunction,
    erosion: &DensityFunction,
    weirdness: &DensityFunction,
    ridges: &DensityFunction,
    amplified: bool,
) -> CubicSpline {
    let transformer = if amplified {
        ValueTransformer::AmplifiedFactor
    } else {
        ValueTransformer::Identity
    };

    // The outer builder is deliberately NO_TRANSFORM even when amplified —
    // only the inner erosion factors are reshaped.
    SplineBuilder::with_transformer(continents.clone(), ValueTransformer::Identity)
        .point(-0.19, 3.95, 0.0)
        .spline_point(
            -0.15,
            erosion_factor(
                erosion,
                weirdness,
                ridges,
                6.25,
                true,
                ValueTransformer::Identity,
            ),
            0.0,
        )
        .spline_point(
            -0.1,
            erosion_factor(erosion, weirdness, ridges, 5.47, true, transformer),
            0.0,
        )
        .spline_point(
            0.03,
            erosion_factor(erosion, weirdness, ridges, 5.08, true, transformer),
            0.0,
        )
        .spline_point(
            0.06,
            erosion_factor(erosion, weirdness, ridges, 4.69, false, transformer),
            0.0,
        )
        .build()
}

/// `TerrainProvider.overworldJaggedness`.
#[must_use]
pub fn overworld_jaggedness(
    continents: &DensityFunction,
    erosion: &DensityFunction,
    weirdness: &DensityFunction,
    ridges: &DensityFunction,
    amplified: bool,
) -> CubicSpline {
    let transformer = if amplified {
        ValueTransformer::AmplifiedJaggedness
    } else {
        ValueTransformer::Identity
    };

    SplineBuilder::with_transformer(continents.clone(), transformer)
        .point(-0.11, 0.0, 0.0)
        .spline_point(
            0.03,
            erosion_jaggedness_spline(erosion, weirdness, ridges, 1.0, 0.5, 0.0, 0.0, transformer),
            0.0,
        )
        .spline_point(
            0.65,
            erosion_jaggedness_spline(erosion, weirdness, ridges, 1.0, 1.0, 1.0, 0.0, transformer),
            0.0,
        )
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::density::build;

    fn coord(from: f64, to: f64) -> DensityFunction {
        build::y_clamped_gradient(-64, 320, from, to)
    }

    #[test]
    fn peaks_and_valleys_folds_weirdness() {
        // Symmetric in weirdness, and peaks at the fold points.
        assert_eq!(peaks_and_valleys(0.5), peaks_and_valleys(-0.5));
        assert!(peaks_and_valleys(0.666_666_7) > peaks_and_valleys(0.0));
    }

    #[test]
    fn amplified_offset_only_raises_land() {
        // Ocean (negative offset) is left alone; land is doubled. That is
        // why amplified worlds have normal seas and absurd mountains.
        assert_eq!(ValueTransformer::AmplifiedOffset.apply(-0.5), -0.5);
        assert_eq!(ValueTransformer::AmplifiedOffset.apply(0.5), 1.0);
    }

    #[test]
    fn the_three_shapers_build_and_stay_bounded() {
        let (continents, erosion, ridges, weirdness) = (
            coord(-1.0, 1.0),
            coord(1.0, -1.0),
            coord(-1.0, 1.0),
            coord(-1.0, 1.0),
        );
        for amplified in [false, true] {
            let offset = overworld_offset(&continents, &erosion, &ridges, amplified);
            let factor = overworld_factor(&continents, &erosion, &weirdness, &ridges, amplified);
            let jagged =
                overworld_jaggedness(&continents, &erosion, &weirdness, &ridges, amplified);

            for spline in [&offset, &factor, &jagged] {
                assert!(
                    spline.min_value() <= spline.max_value(),
                    "bounds inverted (amplified = {amplified})"
                );
                for y in [-64, 0, 64, 128, 320] {
                    let value = spline.apply(crate::density::FunctionContext::new(0, y, 0));
                    assert!(value.is_finite(), "non-finite at y={y}");
                }
            }
        }
    }
}
