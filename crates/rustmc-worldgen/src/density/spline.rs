//! Cubic splines — the terrain shaper's backbone.
//!
//! Reference: `net.minecraft.util.CubicSpline` and
//! `net.minecraft.world.level.levelgen.DensityFunctions.Spline`.
//!
//! # Everything here is `f32`
//!
//! This is the one layer of worldgen that runs in single precision. The
//! coordinate is sampled as `(float) function.compute(ctx)`, every control
//! point and derivative is a `float`, and the interpolation is `float`
//! throughout. Evaluating in `f64` and narrowing at the end produces
//! plausible terrain that is not vanilla's — the rounding compounds through
//! nested splines, and the overworld's shaper is nested three deep.
//!
//! # What it is for
//!
//! Continentalness, erosion and ridges are noise fields in roughly
//! `[-1, 1]`; terrain needs them mapped to offsets, factors and jaggedness
//! with sharp, hand-tuned transitions (a coastline is a cliff in that
//! mapping). A spline is how Mojang expresses those curves as data, with
//! per-point derivatives so the segments join smoothly.

use super::{DensityFunction, FunctionContext};
use crate::math::{binary_search, java_max_f32, java_min_f32, lerp_f32};

/// A cubic spline over a density-function coordinate.
#[derive(Clone, Debug)]
pub enum CubicSpline {
    Constant(f32),
    /// `Arc` rather than `Box`: a spline is built once and read for ever
    /// after — nothing in this module takes `&mut Multipoint`, and
    /// `DensityFunction::number_markers_into` deliberately does not descend
    /// into `Spline`. The router's graph is cloned once per chunk, and this
    /// makes a spline subtree's share of that clone a refcount bump instead
    /// of a walk of every control point.
    Multipoint(std::sync::Arc<Multipoint>),
}

/// `CubicSpline.Multipoint`.
///
/// `locations`, `values` and `derivatives` are parallel and equal-length;
/// [`CubicSpline::multipoint`] is the only constructor and enforces it.
#[derive(Clone, Debug)]
pub struct Multipoint {
    /// Sampled as `f32` — `Spline.Coordinate.apply` casts.
    pub coordinate: DensityFunction,
    pub locations: Vec<f32>,
    pub values: Vec<CubicSpline>,
    pub derivatives: Vec<f32>,
    min_value: f32,
    max_value: f32,
}

impl CubicSpline {
    #[must_use]
    pub const fn constant(value: f32) -> Self {
        Self::Constant(value)
    }

    /// `CubicSpline.Multipoint.create` — builds the node **and** computes
    /// its bounds.
    ///
    /// The bounds are not cosmetic: a `Spline` density function reports them
    /// as its own `min_value`/`max_value`, which `Min`/`Max` nodes above it
    /// read to decide whether to evaluate a branch at all.
    ///
    /// # Panics
    /// If the three arrays differ in length, or are empty.
    #[must_use]
    pub fn multipoint(
        coordinate: DensityFunction,
        locations: Vec<f32>,
        values: Vec<CubicSpline>,
        derivatives: Vec<f32>,
    ) -> Self {
        assert!(
            locations.len() == values.len() && locations.len() == derivatives.len(),
            "All lengths must be equal, got: {} {} {}",
            locations.len(),
            values.len(),
            derivatives.len()
        );
        assert!(
            !locations.is_empty(),
            "Cannot create a multipoint spline with no points"
        );

        let last = locations.len() - 1;
        let mut min_value = f32::INFINITY;
        let mut max_value = f32::NEG_INFINITY;

        // The coordinate's own range, narrowed to f32.
        let min_input = coordinate.min_value() as f32;
        let max_input = coordinate.max_value() as f32;

        // Outside the control points the spline extends linearly along the
        // end derivative, so the extremes can lie beyond any control value.
        if min_input < locations[0] {
            let a = linear_extend(
                min_input,
                &locations,
                values[0].min_value(),
                &derivatives,
                0,
            );
            let b = linear_extend(
                min_input,
                &locations,
                values[0].max_value(),
                &derivatives,
                0,
            );
            min_value = java_min_f32(min_value, java_min_f32(a, b));
            max_value = java_max_f32(max_value, java_max_f32(a, b));
        }
        if max_input > locations[last] {
            let a = linear_extend(
                max_input,
                &locations,
                values[last].min_value(),
                &derivatives,
                last,
            );
            let b = linear_extend(
                max_input,
                &locations,
                values[last].max_value(),
                &derivatives,
                last,
            );
            min_value = java_min_f32(min_value, java_min_f32(a, b));
            max_value = java_max_f32(max_value, java_max_f32(a, b));
        }

        for value in &values {
            min_value = java_min_f32(min_value, value.min_value());
            max_value = java_max_f32(max_value, value.max_value());
        }

        // Interior overshoot: a cubic segment with non-zero end derivatives
        // can leave the box its endpoints define. This bounds that overshoot
        // rather than assuming monotonicity.
        for i in 0..last {
            let x_diff = locations[i + 1] - locations[i];
            let (min1, max1) = (values[i].min_value(), values[i].max_value());
            let (min2, max2) = (values[i + 1].min_value(), values[i + 1].max_value());
            let (d1, d2) = (derivatives[i], derivatives[i + 1]);

            if d1 != 0.0 || d2 != 0.0 {
                let p1 = d1 * x_diff;
                let p2 = d2 * x_diff;
                let min_lerp1 = java_min_f32(min1, min2);
                let max_lerp1 = java_max_f32(max1, max2);
                let min_a = p1 - max2 + min1;
                let max_a = p1 - min2 + max1;
                let min_b = -p2 + min2 - max1;
                let max_b = -p2 + max2 - min1;
                let min_lerp2 = java_min_f32(min_a, min_b);
                let max_lerp2 = java_max_f32(max_a, max_b);
                min_value = java_min_f32(min_value, min_lerp1 + 0.25 * min_lerp2);
                max_value = java_max_f32(max_value, max_lerp1 + 0.25 * max_lerp2);
            }
        }

        Self::Multipoint(std::sync::Arc::new(Multipoint {
            coordinate,
            locations,
            values,
            derivatives,
            min_value,
            max_value,
        }))
    }

    #[must_use]
    pub fn min_value(&self) -> f32 {
        match self {
            Self::Constant(value) => *value,
            Self::Multipoint(m) => m.min_value,
        }
    }

    #[must_use]
    pub fn max_value(&self) -> f32 {
        match self {
            Self::Constant(value) => *value,
            Self::Multipoint(m) => m.max_value,
        }
    }

    /// `CubicSpline.apply`.
    #[must_use]
    pub fn apply(&self, ctx: FunctionContext) -> f32 {
        match self {
            Self::Constant(value) => *value,
            Self::Multipoint(m) => {
                // The coordinate is a full density function narrowed to f32.
                let input = m.coordinate.compute(ctx) as f32;
                let start = find_interval_start(&m.locations, input);
                let last = m.locations.len() - 1;

                if start < 0 {
                    // Below the first control point.
                    linear_extend(
                        input,
                        &m.locations,
                        m.values[0].apply(ctx),
                        &m.derivatives,
                        0,
                    )
                } else if start as usize == last {
                    // At or beyond the last.
                    linear_extend(
                        input,
                        &m.locations,
                        m.values[last].apply(ctx),
                        &m.derivatives,
                        last,
                    )
                } else {
                    let i = start as usize;
                    let x1 = m.locations[i];
                    let x2 = m.locations[i + 1];
                    let t = (input - x1) / (x2 - x1);
                    let y1 = m.values[i].apply(ctx);
                    let y2 = m.values[i + 1].apply(ctx);
                    let d1 = m.derivatives[i];
                    let d2 = m.derivatives[i + 1];

                    // Hermite form, expressed exactly as vanilla writes it:
                    // the cubic correction is `t(1-t) * lerp(t, a, b)`, with
                    // `a`/`b` the deviation of each end's tangent from the
                    // straight chord. Rearranging into a standard basis is
                    // algebraically identical and bit-wise different.
                    let a = d1 * (x2 - x1) - (y2 - y1);
                    let b = -d2 * (x2 - x1) + (y2 - y1);
                    lerp_f32(t, y1, y2) + t * (1.0 - t) * lerp_f32(t, a, b)
                }
            }
        }
    }
}

/// `CubicSpline.Multipoint.linearExtend`.
fn linear_extend(
    input: f32,
    locations: &[f32],
    value: f32,
    derivatives: &[f32],
    index: usize,
) -> f32 {
    let derivative = derivatives[index];
    if derivative == 0.0 {
        value
    } else {
        value + derivative * (input - locations[index])
    }
}

/// `findIntervalStart` — the index of the segment containing `input`,
/// or `-1` below the first point.
///
/// A NaN input never satisfies `input < location`, so it walks to the far
/// end and is linearly extended from the last point. That is vanilla's
/// behavior, inherited from the same comparison.
fn find_interval_start(locations: &[f32], input: f32) -> i32 {
    binary_search(0, locations.len() as i32, |i| input < locations[i as usize]) - 1
}

/// `CubicSpline.Builder`'s value transformer.
///
/// Applied to **constant control values only** — never to a nested spline,
/// and never to a derivative. That asymmetry is vanilla's: the amplified
/// world type reshapes the terrain curves by transforming the numbers the
/// curve passes through, while leaving the sub-curves to transform
/// themselves (they carry their own transformer).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueTransformer {
    /// `NO_TRANSFORM` — the normal world type.
    Identity,
    /// `AMPLIFIED_OFFSET` — raise land, leave ocean alone.
    AmplifiedOffset,
    /// `AMPLIFIED_FACTOR`.
    AmplifiedFactor,
    /// `AMPLIFIED_JAGGEDNESS`.
    AmplifiedJaggedness,
}

impl ValueTransformer {
    #[must_use]
    pub fn apply(self, value: f32) -> f32 {
        match self {
            Self::Identity => value,
            Self::AmplifiedOffset => {
                if value < 0.0 {
                    value
                } else {
                    value * 2.0
                }
            }
            Self::AmplifiedFactor => 1.25 - 6.25 / (value + 5.0),
            Self::AmplifiedJaggedness => value * 2.0,
        }
    }
}

/// Builder mirroring `CubicSpline.Builder`.
///
/// Exists mostly to enforce ascending point order, which vanilla checks at
/// build time and which the evaluation's binary search silently depends on.
#[derive(Debug)]
pub struct SplineBuilder {
    coordinate: DensityFunction,
    transformer: ValueTransformer,
    locations: Vec<f32>,
    values: Vec<CubicSpline>,
    derivatives: Vec<f32>,
}

impl SplineBuilder {
    #[must_use]
    pub fn new(coordinate: DensityFunction) -> Self {
        Self::with_transformer(coordinate, ValueTransformer::Identity)
    }

    #[must_use]
    pub fn with_transformer(coordinate: DensityFunction, transformer: ValueTransformer) -> Self {
        Self {
            coordinate,
            transformer,
            locations: Vec::new(),
            values: Vec::new(),
            derivatives: Vec::new(),
        }
    }

    /// Add a constant control point. The value — and only the value — goes
    /// through the transformer.
    #[must_use]
    pub fn point(self, location: f32, value: f32, derivative: f32) -> Self {
        let transformed = self.transformer.apply(value);
        self.spline_point(location, CubicSpline::Constant(transformed), derivative)
    }

    /// Add a control point whose value is itself a spline.
    ///
    /// # Panics
    /// If `location` is not strictly greater than the previous one.
    #[must_use]
    pub fn spline_point(mut self, location: f32, value: CubicSpline, derivative: f32) -> Self {
        assert!(
            self.locations
                .last()
                .is_none_or(|&previous| location > previous),
            "Please register points in ascending order"
        );
        self.locations.push(location);
        self.values.push(value);
        self.derivatives.push(derivative);
        self
    }

    /// # Panics
    /// If no points were added.
    #[must_use]
    pub fn build(self) -> CubicSpline {
        assert!(!self.locations.is_empty(), "No elements added");
        CubicSpline::multipoint(
            self.coordinate,
            self.locations,
            self.values,
            self.derivatives,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::density::build;

    fn ramp() -> DensityFunction {
        build::y_clamped_gradient(0, 100, -1.0, 1.0)
    }

    fn at(y: i32) -> FunctionContext<'static> {
        FunctionContext::new(0, y, 0)
    }

    #[test]
    fn control_points_are_hit_exactly() {
        // With zero derivatives the spline passes through its points.
        let spline = SplineBuilder::new(ramp())
            .point(-1.0, 10.0, 0.0)
            .point(0.0, 20.0, 0.0)
            .point(1.0, 30.0, 0.0)
            .build();
        assert_eq!(spline.apply(at(0)), 10.0, "y=0 -> coordinate -1");
        assert_eq!(spline.apply(at(50)), 20.0, "y=50 -> coordinate 0");
        assert_eq!(spline.apply(at(100)), 30.0, "y=100 -> coordinate 1");
    }

    #[test]
    fn outside_the_span_extends_linearly_along_the_derivative() {
        // Coordinate range is [-1, 1]; put the only point at 0 with slope 2.
        let spline = SplineBuilder::new(ramp()).point(0.0, 5.0, 2.0).build();
        // Coordinate is -1 at y=0 and +1 at y=100, so the value walks
        // 5 +/- the slope.
        assert_eq!(spline.apply(at(0)), 3.0);
        assert_eq!(spline.apply(at(100)), 7.0);
    }

    #[test]
    fn bounds_account_for_linear_extension() {
        // A flat value of 5 but a slope of 2 at the single point: the
        // reachable range is [3, 7], not [5, 5].
        let spline = SplineBuilder::new(ramp()).point(0.0, 5.0, 2.0).build();
        assert_eq!(spline.min_value(), 3.0);
        assert_eq!(spline.max_value(), 7.0);
    }

    #[test]
    fn ascending_order_is_enforced() {
        let result = std::panic::catch_unwind(|| {
            SplineBuilder::new(ramp())
                .point(1.0, 0.0, 0.0)
                .point(0.0, 0.0, 0.0)
        });
        assert!(result.is_err(), "descending points must be rejected");
    }

    #[test]
    fn nested_splines_evaluate_in_f32() {
        // A spline whose control value is another spline — the shape the
        // overworld terrain shaper actually uses.
        let inner = SplineBuilder::new(ramp())
            .point(-1.0, 1.0, 0.0)
            .point(1.0, 2.0, 0.0)
            .build();
        let outer = SplineBuilder::new(ramp())
            .spline_point(-1.0, inner, 0.0)
            .point(1.0, 0.0, 0.0)
            .build();
        // Finite, and inside the union of the two levels' ranges.
        let value = outer.apply(at(25));
        assert!(value.is_finite());
        assert!(outer.min_value() <= value && value <= outer.max_value());
    }
}
