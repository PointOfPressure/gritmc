//! The canyon carver — ravines.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `CanyonWorldCarver`.
//!
//! One long gash per start chunk, not a branching system: `carve` picks a
//! single origin and calls `doCarve` once. Everything that makes a ravine
//! look like a ravine rather than a wide cave is in two places.
//!
//! # The width factors are a per-height table, drawn up front
//!
//! `initWidthFactors` fills one float for every y in the world *before* the
//! walk starts, redrawing the factor whenever `nextInt(widthSmoothness)`
//! comes up zero. Those draws come off the tunnel's own stream, so the table
//! is 384 draws deep and everything after it in the same tunnel depends on
//! having consumed exactly the right number.
//!
//! The first entry is special: `yIndex == 0 || nextInt(...) == 0`
//! short-circuits, so **no draw happens at y index 0**. Writing the
//! condition the other way round consumes one extra value and shifts the
//! entire table.
//!
//! # The vertical squash is what makes it a ravine
//!
//! `shouldSkip` divides `yd²` by six before the ellipsoid test, so the shape
//! is six times taller than it is wide, and multiplies the horizontal term
//! by that height's width factor, so the walls waver. Take either out and
//! you get a long cave.

use super::carve::{can_reach, carve_ellipsoid, CarveBounds, CarveTarget};
use super::config::CanyonConfig;
use super::mask::CarvingMask;
use crate::math::{cos, sin};
use crate::rng::{LegacyRandomSource, RandomSource, WorldgenRandom};
use crate::tags::BlockTags;

use super::cave::RANGE;

/// `CanyonWorldCarver#carve`.
#[allow(clippy::too_many_arguments)]
pub fn carve<T: CarveTarget, R: RandomSource>(
    target: &mut T,
    tags: &BlockTags,
    config: &CanyonConfig,
    bounds: CarveBounds,
    mask: &mut CarvingMask,
    random: &mut R,
    source_chunk_x: i32,
    source_chunk_z: i32,
) {
    let max_distance = (RANGE * 2 - 1) * 16;
    // There is no nether canyon carver, so this is always the standard
    // `WorldCarver#getCarveState`.
    let style = super::carve::CarveStyle::Standard {
        lava_level_y: config
            .base
            .lava_level
            .resolve(bounds.min_gen_y, bounds.gen_depth),
    };

    let x = f64::from(source_chunk_x * 16 + random.next_i32_bound(16));
    let y = f64::from(
        config
            .base
            .y
            .sample(random, bounds.min_gen_y, bounds.gen_depth),
    );
    let z = f64::from(source_chunk_z * 16 + random.next_i32_bound(16));
    let horizontal_rotation = random.next_f32() * std::f32::consts::TAU;
    let vertical_rotation = config.vertical_rotation.sample(random);
    let y_scale = f64::from(config.base.y_scale.sample(random));
    let thickness = config.shape.thickness.sample(random);
    // `(int)(maxDistance * factor)` — int widened to float, multiplied in
    // float, then **truncated toward zero**, not rounded.
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
    let distance = (max_distance as f32 * config.shape.distance_factor.sample(random)) as i32;
    let tunnel_seed = random.next_i64();

    do_carve(
        target,
        tags,
        config,
        style,
        bounds,
        mask,
        tunnel_seed,
        x,
        y,
        z,
        thickness,
        horizontal_rotation,
        vertical_rotation,
        0,
        distance,
        y_scale,
    );
}

/// `CanyonWorldCarver#doCarve`.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn do_carve<T: CarveTarget>(
    target: &mut T,
    tags: &BlockTags,
    config: &CanyonConfig,
    style: super::carve::CarveStyle,
    bounds: CarveBounds,
    mask: &mut CarvingMask,
    tunnel_seed: i64,
    mut x: f64,
    mut y: f64,
    mut z: f64,
    thickness: f32,
    mut horizontal_rotation: f32,
    mut vertical_rotation: f32,
    step: i32,
    distance: i32,
    y_scale: f64,
) {
    let mut random = WorldgenRandom::new(LegacyRandomSource::new(tunnel_seed));
    let width_factors = init_width_factors(config, bounds.gen_depth, &mut random);
    let mut y_rota = 0.0f32;
    let mut x_rota = 0.0f32;

    for current_step in step..distance {
        #[allow(clippy::cast_precision_loss)]
        let angle = current_step as f32 * std::f32::consts::PI / distance as f32;
        let mut horizontal_radius = 1.5 + f64::from(sin(f64::from(angle))) * f64::from(thickness);
        let mut vertical_radius = horizontal_radius * y_scale;
        horizontal_radius *= f64::from(config.shape.horizontal_radius_factor.sample(&mut random));
        vertical_radius =
            update_vertical_radius(config, &mut random, vertical_radius, distance, current_step);

        let xc = cos(f64::from(vertical_rotation));
        let xs = sin(f64::from(vertical_rotation));
        x += f64::from(cos(f64::from(horizontal_rotation))) * f64::from(xc);
        y += f64::from(xs);
        z += f64::from(sin(f64::from(horizontal_rotation))) * f64::from(xc);
        // A canyon never gets the cave's `steep` treatment — the decay is
        // always 0.7 — and the rotation feedback is 0.05 here against the
        // cave's 0.1, which is why ravines run straighter.
        vertical_rotation *= 0.7;
        vertical_rotation += x_rota * 0.05;
        horizontal_rotation += y_rota * 0.05;
        x_rota *= 0.8;
        y_rota *= 0.5;
        x_rota += (random.next_f32() - random.next_f32()) * random.next_f32() * 2.0;
        y_rota += (random.next_f32() - random.next_f32()) * random.next_f32() * 4.0;

        if random.next_i32_bound(4) != 0 {
            if !can_reach(bounds, x, z, current_step, distance, thickness) {
                return;
            }
            let min_gen_y = bounds.min_gen_y;
            let factors = &width_factors;
            carve_ellipsoid(
                target,
                tags,
                &config.base.replaceable,
                style,
                bounds,
                mask,
                x,
                y,
                z,
                horizontal_radius,
                vertical_radius,
                &move |xd, yd, zd, block_y| should_skip(factors, min_gen_y, xd, yd, zd, block_y),
            );
        }
    }
}

/// `CanyonWorldCarver#initWidthFactors`.
fn init_width_factors<R: RandomSource>(
    config: &CanyonConfig,
    gen_depth: i32,
    random: &mut R,
) -> Vec<f32> {
    #[allow(clippy::cast_sign_loss)]
    let depth = gen_depth.max(0) as usize;
    let mut factors = Vec::with_capacity(depth);
    let mut width_factor = 1.0f32;
    for y_index in 0..depth {
        // `yIndex == 0 || nextInt(...) == 0` — the short circuit means no
        // draw at index 0. Reversing the operands consumes one extra value
        // and shifts the whole table.
        if y_index == 0 || random.next_i32_bound(config.shape.width_smoothness) == 0 {
            width_factor = 1.0 + random.next_f32() * random.next_f32();
        }
        factors.push(width_factor * width_factor);
    }
    factors
}

/// `CanyonWorldCarver#updateVerticalRadius`.
fn update_vertical_radius<R: RandomSource>(
    config: &CanyonConfig,
    random: &mut R,
    vertical_radius: f64,
    distance: i32,
    current_step: i32,
) -> f64 {
    // Both parameters are `float` in the reference, so the ints widen at the
    // call and the division below is a float division.
    #[allow(clippy::cast_precision_loss)]
    let (distance, current_step) = (distance as f32, current_step as f32);
    // Peaks at 1 in the middle of the run and falls to 0 at both ends, so a
    // ravine is deepest halfway along.
    let vertical_multiplier = 1.0 - (0.5 - current_step / distance).abs() * 2.0;
    let factor = config.shape.vertical_radius_default_factor
        + config.shape.vertical_radius_center_factor * vertical_multiplier;
    // `factor * verticalRadius * randomBetween(...)`, left to right: the
    // first product widens to double, then the float draw widens into it.
    f64::from(factor) * vertical_radius * f64::from(random.next_f32() * 0.25 + 0.75)
}

/// `CanyonWorldCarver#shouldSkip`.
fn should_skip(
    width_factors: &[f32],
    min_gen_y: i32,
    xd: f64,
    yd: f64,
    zd: f64,
    block_y: i32,
) -> bool {
    // `widthFactorPerHeight[yIndex - 1]` — one *below* this block's index.
    // `carveEllipsoid` never visits `minGenY + 1` or lower, so this cannot
    // go negative there; it is checked anyway because a caller with
    // different bounds would otherwise read out of the slice.
    let y_index = block_y - min_gen_y;
    let Some(width) = usize::try_from(y_index - 1)
        .ok()
        .and_then(|i| width_factors.get(i))
    else {
        return true;
    };
    // `yd * yd / 6.0` — the squash that makes it a ravine rather than a
    // long cave.
    (xd * xd + zd * zd) * f64::from(*width) + yd * yd / 6.0 >= 1.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::carver::config::{CanyonShape, CarverConfig};
    use crate::carver::providers::{FloatProvider, HeightProvider};
    use crate::surface::rule::VerticalAnchor;

    fn config() -> CanyonConfig {
        CanyonConfig {
            base: CarverConfig {
                probability: 0.01,
                y: HeightProvider::Uniform {
                    min_inclusive: VerticalAnchor::Absolute(10),
                    max_inclusive: VerticalAnchor::Absolute(67),
                },
                y_scale: FloatProvider::Constant(3.0),
                lava_level: VerticalAnchor::AboveBottom(8),
                replaceable: "#minecraft:overworld_carver_replaceables".to_string(),
            },
            vertical_rotation: FloatProvider::Uniform {
                min: -0.125,
                max_exclusive: 0.125,
            },
            shape: CanyonShape {
                distance_factor: FloatProvider::Uniform {
                    min: 0.75,
                    max_exclusive: 1.0,
                },
                thickness: FloatProvider::Trapezoid {
                    min: 0.0,
                    max: 6.0,
                    plateau: 2.0,
                },
                vertical_radius_default_factor: 1.0,
                vertical_radius_center_factor: 0.0,
                horizontal_radius_factor: FloatProvider::Uniform {
                    min: 0.75,
                    max_exclusive: 1.0,
                },
                width_smoothness: 3,
            },
        }
    }

    fn random(seed: i64) -> WorldgenRandom<LegacyRandomSource> {
        WorldgenRandom::new(LegacyRandomSource::new(seed))
    }

    #[test]
    fn the_first_width_factor_costs_no_smoothness_draw() {
        // `yIndex == 0 || nextInt(...)` short-circuits. One draw either way
        // for the factor itself, but the *smoothness* draw is skipped, and
        // getting that wrong shifts all 384 entries.
        let mut r = random(5);
        let before = r.count();
        let factors = init_width_factors(&config(), 1, &mut r);
        assert_eq!(factors.len(), 1);
        assert_eq!(
            r.count() - before,
            2,
            "index 0 should draw the factor's two floats and nothing else"
        );
    }

    #[test]
    fn the_width_table_is_piecewise_constant() {
        // The point of `widthSmoothness`: the factor is held for a run of
        // heights rather than redrawn every block, which is what makes the
        // walls waver smoothly instead of jittering.
        let factors = init_width_factors(&config(), 384, &mut random(9));
        assert_eq!(factors.len(), 384);
        let repeats = factors.windows(2).filter(|w| w[0] == w[1]).count();
        assert!(
            repeats > 200,
            "only {repeats} of 383 neighbours repeat — the factor is being redrawn too often"
        );
        assert!(
            factors.windows(2).any(|w| w[0] != w[1]),
            "the factor never changed at all"
        );
        // `widthFactor * widthFactor` with widthFactor in [1, 2).
        for f in factors {
            assert!((1.0..4.0).contains(&f), "width factor {f} out of range");
        }
    }

    #[test]
    fn the_vertical_squash_is_what_makes_it_a_ravine() {
        // yd is divided by six, so the shape reaches six times further
        // vertically than horizontally. Without it a canyon is a long cave.
        let flat = vec![1.0f32; 384];
        // Straight up, well outside a unit sphere, still carved.
        assert!(!should_skip(&flat, -64, 0.0, 2.0, 0.0, 0));
        // The same distance sideways is not.
        assert!(should_skip(&flat, -64, 2.0, 0.0, 0.0, 0));
    }

    #[test]
    fn a_wider_width_factor_narrows_the_ravine() {
        // The factor multiplies the *horizontal* term, so a larger factor
        // makes the test fail sooner — narrower walls, not wider.
        let narrow = vec![4.0f32; 384];
        let wide = vec![1.0f32; 384];
        assert!(should_skip(&narrow, -64, 0.6, 0.0, 0.0, 0));
        assert!(!should_skip(&wide, -64, 0.6, 0.0, 0.0, 0));
    }

    #[test]
    fn the_vertical_radius_peaks_halfway_along() {
        // With `verticalRadiusCenterFactor` at 0 the overworld canyon has a
        // flat profile, so this checks the multiplier itself rather than the
        // configured result.
        let mut shaped = config();
        shaped.shape.vertical_radius_center_factor = 1.0;
        shaped.shape.vertical_radius_default_factor = 0.0;
        let at = |step: i32| {
            let mut r = random(1);
            update_vertical_radius(&shaped, &mut r, 10.0, 100, step)
        };
        assert!(at(50) > at(25), "midpoint should be deepest");
        assert!(at(25) > at(1), "and it should taper to the ends");
        assert!(at(0).abs() < 1.0e-9, "the very start should vanish");
    }

    #[test]
    fn an_out_of_range_height_is_skipped_rather_than_panicking() {
        // `carveEllipsoid` never visits low enough for this to fire, but a
        // caller with different bounds would read before the slice.
        let flat = vec![1.0f32; 8];
        assert!(should_skip(&flat, -64, 0.0, 0.0, 0.0, -64));
        assert!(should_skip(&flat, -64, 0.0, 0.0, 0.0, 1000));
    }
}
