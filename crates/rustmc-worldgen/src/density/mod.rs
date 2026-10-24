//! Density functions — the graph terrain generation actually evaluates.
//!
//! Reference: `net.minecraft.world.level.levelgen.{DensityFunction,
//! DensityFunctions}`.
//!
//! # Why an enum and not a trait object
//!
//! Vanilla models this as an interface with ~25 implementations, so every
//! sample is a virtual call. Here it is one `enum` evaluated by `match`:
//! same semantics, no vtable, and the whole tree is contiguous enough to
//! stay in cache. That matters because this is the single hottest code path
//! in the server — chunk generation is the declared P1 target (PRODUCT.md),
//! and Minecom measured a 7.5–8.5x deficit against vanilla here.
//!
//! It also leaves the door open for the real fix later: an `enum` tree can
//! be flattened into an instruction array and walked iteratively without
//! changing any of the semantics proved below. Do that *after* parity is
//! green, never before.
//!
//! # Bounds are semantics, not metadata
//!
//! `min_value`/`max_value` look like documentation and are not. Three places
//! read them at *evaluation* time and change the result:
//!
//! - [`Ap2`]'s `Min`/`Max` skip evaluating the second argument when the
//!   first already decides the outcome.
//! - [`build::two_argument`] rewrites `Add`/`Mul` against a constant into
//!   [`DensityFunction::MulOrAdd`], which has **no** zero short-circuit —
//!   an observable difference once infinities are in play.
//! - `Mapped::create` widens `Invert`'s range to infinite when the input
//!   straddles zero.
//!
//! So construction goes through [`build`], never through struct literals.

pub mod spline;

pub use spline::{CubicSpline, SplineBuilder, ValueTransformer};

pub mod aquifer;
pub mod beard;
pub mod cache_once;
pub mod cell;
pub mod chunk;
pub mod end;
pub mod fill;
pub mod flat_cache;
pub mod ore_vein;
pub mod router;
pub mod surface_level;
pub mod terrain;

use crate::math::{clamp, clamped_map, java_max, java_min};
use crate::noise::{BlendedNoise, NormalNoise};

/// Where a density function is being sampled.
///
/// Vanilla's `FunctionContext` also carries a `Blender`; blending is a
/// worker-owned operator (see `swarm/tasks/w2-density-*.md`) and will extend
/// this rather than replace it.
#[derive(Clone, Copy, Debug)]
pub struct FunctionContext<'a> {
    pub block_x: i32,
    pub block_y: i32,
    pub block_z: i32,
    /// Current value of each `Interpolated` marker, by index.
    ///
    /// Empty outside a chunk fill, which is exactly vanilla's
    /// `if (context != NoiseChunk.this) return noiseFiller.compute(context)`
    /// fallback: sampled on its own, an interpolated marker just computes
    /// its wrapped function.
    interpolated: &'a [f64],
    /// The chunk's 2D caches. Empty outside a chunk, which makes every
    /// `FlatCache` marker compute its wrapped function directly — vanilla's
    /// behaviour for a function that was never wrapped by a `NoiseChunk`.
    flat: &'a flat_cache::FlatCaches,
    /// Scratch for `CacheOnce` markers, keyed to the point being evaluated.
    ///
    /// `Option` rather than a shared empty instance because the scratch has
    /// interior mutability, so it cannot be a `static` — and `None` is
    /// exactly the right meaning anyway: no chunk, no memoization.
    cache_once: Option<&'a cache_once::CacheOnceScratch>,
    /// Value of each `CacheAllInCell` marker at the block being evaluated,
    /// by index, filled once per cell.
    ///
    /// Vanilla fills these in `selectCellYZ` inside a `fillingCell` window,
    /// during which an interpolated descendant returns its **raw** value —
    /// so the cache holds raw values and serves them for the whole cell,
    /// bypassing interpolation for that subtree. Empty outside a chunk fill,
    /// where the marker computes its wrapped function directly.
    cell_all: &'a [f64],
}

/// The empty cache every bare context points at, so `new` can stay `const`
/// and no context has to carry an `Option`.
static NO_FLAT_CACHES: flat_cache::FlatCaches = flat_cache::FlatCaches::none();

impl FunctionContext<'static> {
    #[must_use]
    pub const fn new(block_x: i32, block_y: i32, block_z: i32) -> Self {
        Self {
            block_x,
            block_y,
            block_z,
            interpolated: &[],
            flat: &NO_FLAT_CACHES,
            cache_once: None,
            cell_all: &[],
        }
    }
}

impl<'a> FunctionContext<'a> {
    /// A context inside a chunk fill, carrying each interpolator's value for
    /// the block being evaluated.
    #[must_use]
    pub const fn with_interpolated(
        block_x: i32,
        block_y: i32,
        block_z: i32,
        interpolated: &'a [f64],
        flat: &'a flat_cache::FlatCaches,
        cache_once: Option<&'a cache_once::CacheOnceScratch>,
    ) -> Self {
        Self {
            block_x,
            block_y,
            block_z,
            interpolated,
            flat,
            cache_once,
            cell_all: &[],
        }
    }

    /// The same, plus this cell's `CacheAllInCell` values.
    #[must_use]
    pub const fn with_cell_values(
        block_x: i32,
        block_y: i32,
        block_z: i32,
        interpolated: &'a [f64],
        cell_all: &'a [f64],
        flat: &'a flat_cache::FlatCaches,
        cache_once: Option<&'a cache_once::CacheOnceScratch>,
    ) -> Self {
        Self {
            block_x,
            block_y,
            block_z,
            interpolated,
            flat,
            cache_once,
            cell_all,
        }
    }

    /// A context with the chunk's flat caches but no interpolated values —
    /// what the aquifer, the surface-level scan and the cache prefill use.
    #[must_use]
    pub const fn with_flat_caches(
        block_x: i32,
        block_y: i32,
        block_z: i32,
        flat: &'a flat_cache::FlatCaches,
    ) -> Self {
        Self {
            block_x,
            block_y,
            block_z,
            interpolated: &[],
            flat,
            cache_once: None,
            cell_all: &[],
        }
    }

    /// A context with both the chunk's flat caches and its `CacheOnce`
    /// scratch — what the cell-lattice prefill uses, since that is where
    /// `slopedCheese` is read twice per point.
    #[must_use]
    pub const fn with_caches(
        block_x: i32,
        block_y: i32,
        block_z: i32,
        flat: &'a flat_cache::FlatCaches,
        cache_once: &'a cache_once::CacheOnceScratch,
    ) -> Self {
        Self {
            block_x,
            block_y,
            block_z,
            interpolated: &[],
            flat,
            cache_once: Some(cache_once),
            cell_all: &[],
        }
    }
}

/// `DensityFunction.NoiseHolder` — a noise slot that may be unbound.
///
/// Vanilla allows the noise to be absent (the codec resolves it later) and
/// makes the unbound case *defined* rather than an error: value 0, max 2.
/// Treating it as a panic would break loading a settings file that names a
/// noise the registry hasn't supplied yet.
///
/// # Why the noise is behind an `Arc`
///
/// The pipeline clones the router's whole graph **once per chunk**
/// (`stage::noise::inputs`), because `ChunkSampler::new` numbers the markers
/// in place and so needs a graph of its own. A `NormalNoise` is two
/// `PerlinNoise` stacks and each octave carries a 256-byte permutation
/// table, so that clone was copying every noise table in the router for
/// every chunk — a `perf record` of the r=16 pipeline (2026-07-28, seed
/// 20260722, (64,64)) put `PerlinNoise::clone` and the matching
/// `drop_in_place` at 630 and 525 of 24,097 samples.
///
/// Nothing mutates a `NormalNoise` after construction — `noise/normal.rs`
/// and `noise/perlin.rs` have no `&mut self` method between them — so
/// sharing one is the same field values read through one more pointer. It
/// cannot change a generated block.
#[derive(Clone, Debug)]
pub struct NoiseHolder {
    noise: Option<std::sync::Arc<NormalNoise>>,
}

impl NoiseHolder {
    #[must_use]
    pub fn bound(noise: NormalNoise) -> Self {
        Self {
            noise: Some(std::sync::Arc::new(noise)),
        }
    }

    #[must_use]
    pub const fn unbound() -> Self {
        Self { noise: None }
    }

    #[must_use]
    pub fn get_value(&self, x: f64, y: f64, z: f64) -> f64 {
        self.noise.as_ref().map_or(0.0, |n| n.get_value(x, y, z))
    }

    #[must_use]
    pub fn max_value(&self) -> f64 {
        self.noise
            .as_ref()
            .map_or(2.0, |n| NormalNoise::max_value(n))
    }
}

/// `TwoArgumentSimpleFunction.Type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ap2 {
    Add,
    Mul,
    Min,
    Max,
}

/// `MulOrAdd.Type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MulAdd {
    Mul,
    Add,
}

/// `Mapped.Type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mapped {
    Abs,
    Square,
    Cube,
    HalfNegative,
    QuarterNegative,
    Invert,
    Squeeze,
}

impl Mapped {
    /// `Mapped.transform`.
    #[must_use]
    pub fn transform(self, input: f64) -> f64 {
        match self {
            Self::Abs => input.abs(),
            Self::Square => input * input,
            Self::Cube => input * input * input,
            // Note `> 0.0`, so -0.0 and NaN take the multiply branch.
            Self::HalfNegative => {
                if input > 0.0 {
                    input
                } else {
                    input * 0.5
                }
            }
            Self::QuarterNegative => {
                if input > 0.0 {
                    input
                } else {
                    input * 0.25
                }
            }
            Self::Invert => 1.0 / input,
            Self::Squeeze => {
                let c = clamp(input, -1.0, 1.0);
                c / 2.0 - c * c * c / 24.0
            }
        }
    }
}

/// A node in the density graph.
///
/// Construct through [`build`], not directly — see the module docs on why
/// bounds are semantics.
#[derive(Clone, Debug)]
pub enum DensityFunction {
    Constant(f64),

    /// `Ap2` — two general arguments.
    TwoArgument {
        kind: Ap2,
        first: Box<DensityFunction>,
        second: Box<DensityFunction>,
        min: f64,
        max: f64,
        /// The **second argument's** bounds, stored rather than asked for.
        ///
        /// `Min`/`Max` short-circuit on them once per evaluation, and
        /// `min_value()` recurses through markers, range choices and
        /// interval selects — so asking the child each time walks a subtree
        /// per block. It showed up at 1.9% of a whole chunk generation in a
        /// profile. They are constants of the graph, and `build` already
        /// computes them to derive this node's own bounds.
        second_min: f64,
        second_max: f64,
    },

    /// `MulOrAdd` — one argument against a constant. Distinct from
    /// `TwoArgument` because it deliberately lacks the zero short-circuit.
    MulOrAdd {
        kind: MulAdd,
        input: Box<DensityFunction>,
        argument: f64,
        min: f64,
        max: f64,
    },

    /// `Mapped` — a unary transform with precomputed bounds.
    Map {
        kind: Mapped,
        input: Box<DensityFunction>,
        min: f64,
        max: f64,
    },

    /// `Clamp`.
    Clamp {
        input: Box<DensityFunction>,
        min: f64,
        max: f64,
    },

    /// `RangeChoice` — `[min_inclusive, max_exclusive)` on the input value.
    RangeChoice {
        input: Box<DensityFunction>,
        min_inclusive: f64,
        max_exclusive: f64,
        when_in_range: Box<DensityFunction>,
        when_out_of_range: Box<DensityFunction>,
    },

    /// `YClampedGradient`.
    YClampedGradient {
        from_y: i32,
        to_y: i32,
        from_value: f64,
        to_value: f64,
    },

    /// `Noise` — sampled at scaled block coordinates.
    Noise {
        noise: NoiseHolder,
        xz_scale: f64,
        y_scale: f64,
    },

    /// `Shift` / `ShiftA` / `ShiftB` — the same computation over different
    /// coordinate permutations, which is why they share a variant.
    Shift {
        noise: NoiseHolder,
        axes: ShiftAxes,
    },

    /// `ShiftedNoise`.
    ShiftedNoise {
        shift_x: Box<DensityFunction>,
        shift_y: Box<DensityFunction>,
        shift_z: Box<DensityFunction>,
        xz_scale: f64,
        y_scale: f64,
        noise: NoiseHolder,
    },

    /// `old_blended_noise`.
    ///
    /// `Arc` rather than `Box` for the reason given on [`NoiseHolder`]: three
    /// `PerlinNoise` stacks that never change, cloned once per chunk with the
    /// rest of the graph.
    OldBlendedNoise(std::sync::Arc<BlendedNoise>),

    /// `Spline` — evaluated entirely in `f32`; see [`spline`].
    Spline(CubicSpline),

    /// `EndIslandDensityFunction` — 2D, ignores y entirely.
    EndIslands(std::sync::Arc<end::EndIslandNoise>),

    /// `Beardifier` — the terrain a chunk's structures raise around
    /// themselves. See [`beard`].
    ///
    /// The only variant whose value depends on data outside the router: it
    /// carries **this chunk's** structure pieces, so it is built per chunk
    /// and travels in `FillInputs`, never in the shared router graph. The
    /// `Arc` is so cloning the graph per chunk stays cheap.
    ///
    /// In vanilla this node is `DensityFunctions$BeardifierMarker.INSTANCE`
    /// in the tree until `NoiseChunk.wrap` substitutes the real one. There is
    /// no marker here because there is nothing to substitute: the beardifier
    /// is already the per-chunk object by the time the node exists.
    Beardifier(std::sync::Arc<beard::Beardifier>),

    /// `FindTopSurface` — scans a column downwards for the first solid cell.
    ///
    /// The odd one in the family: every other operator is a pure function of
    /// the point it is handed, but this one **re-enters the graph at other y
    /// values**, walking down in `cell_height` steps until `density` turns
    /// positive. It returns a block y, not a density, and feeds
    /// `preliminarySurfaceLevel`, which the aquifer uses to decide whether a
    /// water table is buried or pokes out of a hillside.
    ///
    /// Cost is why the column is walked in cell steps rather than per block,
    /// and why `upper_bound` exists at all: without a cheap ceiling this
    /// would scan 384 blocks of full density graph for every sample.
    FindTopSurface {
        density: Box<DensityFunction>,
        upper_bound: Box<DensityFunction>,
        lower_bound: i32,
        cell_height: i32,
    },

    /// `BlendAlpha` — constant 1.0.
    ///
    /// Deliberately **not** a [`Self::Constant`]: `build::lerp` branches on
    /// `first` being a `Constant`, and vanilla's `blendOffset()` is a
    /// distinct type, so collapsing these into constants would silently
    /// take the wrong lerp expansion.
    BlendAlpha,

    /// `BlendOffset` — constant 0.0. See [`Self::BlendAlpha`].
    BlendOffset,

    /// `IntervalSelect` — pick a branch by which band the input falls in.
    ///
    /// `thresholds` must be ascending and exactly one shorter than
    /// `functions`; the last function handles everything at or above the
    /// final threshold. 26.2 leans on this for cave rarity, where the bands
    /// quantise a continuous rarity field into a handful of noise scales.
    IntervalSelect {
        input: Box<DensityFunction>,
        thresholds: Vec<f64>,
        functions: Vec<DensityFunction>,
    },

    /// `Marker` — a tagged passthrough.
    ///
    /// At this layer every marker computes its wrapped function directly;
    /// the caching the names promise materializes only when `NoiseChunk`
    /// wraps them for cell sampling. Treating them as no-ops here is
    /// vanilla's own behavior, not a shortcut.
    Marker {
        kind: MarkerKind,
        wrapped: Box<DensityFunction>,
        /// Index into the chunk's table for this marker's kind — the
        /// interpolated values for `Interpolated`, the flat caches for
        /// `FlatCache`. `None` until numbered, and outside a chunk it stays
        /// `None`, which is what makes a marker a passthrough on its own.
        ///
        /// One field for both because a marker is exactly one kind, so the
        /// two numbering passes can never collide.
        slot: Option<usize>,
    },
}

/// `Marker.Type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkerKind {
    Interpolated,
    FlatCache,
    Cache2D,
    CacheOnce,
    CacheAllInCell,
    /// The odd one out: it reports **unbounded** min/max rather than its
    /// wrapped function's, because blending can push the value anywhere.
    BlendDensity,
}

/// Which coordinates a `Shift*` feeds into the offset noise.
///
/// `ShiftB` is not a typo: it really does pass `(z, x, 0)`. The three exist
/// so one 3D noise can supply independent-looking offsets per axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShiftAxes {
    /// `Shift` — `(x, y, z)`.
    Xyz,
    /// `ShiftA` — `(x, 0, z)`.
    Xz,
    /// `ShiftB` — `(z, x, 0)`.
    Zx,
}

impl DensityFunction {
    /// Evaluate at one point.
    #[must_use]
    pub fn compute(&self, ctx: FunctionContext) -> f64 {
        match self {
            Self::Constant(value) => *value,

            Self::TwoArgument {
                kind,
                first,
                second,
                second_min,
                second_max,
                ..
            } => {
                let a = first.compute(ctx);
                match kind {
                    Ap2::Add => a + second.compute(ctx),
                    // The zero short-circuit is observable: `0 * inf` is NaN
                    // if evaluated, and 0.0 here.
                    Ap2::Mul => {
                        if a == 0.0 {
                            0.0
                        } else {
                            a * second.compute(ctx)
                        }
                    }
                    // Skip the second argument when its whole range is
                    // already losing.
                    Ap2::Min => {
                        if a < *second_min {
                            a
                        } else {
                            java_min(a, second.compute(ctx))
                        }
                    }
                    Ap2::Max => {
                        if a > *second_max {
                            a
                        } else {
                            java_max(a, second.compute(ctx))
                        }
                    }
                }
            }

            Self::MulOrAdd {
                kind,
                input,
                argument,
                ..
            } => {
                let value = input.compute(ctx);
                match kind {
                    MulAdd::Mul => value * argument,
                    MulAdd::Add => value + argument,
                }
            }

            Self::Map { kind, input, .. } => kind.transform(input.compute(ctx)),

            Self::Clamp { input, min, max } => clamp(input.compute(ctx), *min, *max),

            Self::RangeChoice {
                input,
                min_inclusive,
                max_exclusive,
                when_in_range,
                when_out_of_range,
            } => {
                let value = input.compute(ctx);
                if value >= *min_inclusive && value < *max_exclusive {
                    when_in_range.compute(ctx)
                } else {
                    when_out_of_range.compute(ctx)
                }
            }

            Self::YClampedGradient {
                from_y,
                to_y,
                from_value,
                to_value,
            } => clamped_map(
                f64::from(ctx.block_y),
                f64::from(*from_y),
                f64::from(*to_y),
                *from_value,
                *to_value,
            ),

            Self::Noise {
                noise,
                xz_scale,
                y_scale,
            } => noise.get_value(
                f64::from(ctx.block_x) * xz_scale,
                f64::from(ctx.block_y) * y_scale,
                f64::from(ctx.block_z) * xz_scale,
            ),

            Self::Shift { noise, axes } => {
                let (x, y, z) = match axes {
                    ShiftAxes::Xyz => (
                        f64::from(ctx.block_x),
                        f64::from(ctx.block_y),
                        f64::from(ctx.block_z),
                    ),
                    ShiftAxes::Xz => (f64::from(ctx.block_x), 0.0, f64::from(ctx.block_z)),
                    ShiftAxes::Zx => (f64::from(ctx.block_z), f64::from(ctx.block_x), 0.0),
                };
                noise.get_value(x * 0.25, y * 0.25, z * 0.25) * 4.0
            }

            Self::ShiftedNoise {
                shift_x,
                shift_y,
                shift_z,
                xz_scale,
                y_scale,
                noise,
            } => {
                let x = f64::from(ctx.block_x) * xz_scale + shift_x.compute(ctx);
                let y = f64::from(ctx.block_y) * y_scale + shift_y.compute(ctx);
                let z = f64::from(ctx.block_z) * xz_scale + shift_z.compute(ctx);
                noise.get_value(x, y, z)
            }

            Self::OldBlendedNoise(noise) => noise.compute(ctx.block_x, ctx.block_y, ctx.block_z),

            // Widened only at the boundary: the spline itself ran in f32.
            Self::Spline(spline) => f64::from(spline.apply(ctx)),

            Self::EndIslands(islands) => islands.compute(ctx.block_x, ctx.block_z),

            Self::Beardifier(beardifier) => {
                beardifier.compute(ctx.block_x, ctx.block_y, ctx.block_z)
            }

            Self::FindTopSurface {
                density,
                upper_bound,
                lower_bound,
                cell_height,
            } => {
                // `Mth.floor(upper / cellHeight) * cellHeight` — floor, not
                // truncation, so the snap is downward for negative y too.
                // The overworld's ceiling is clamped to -40 before it gets
                // here, but the operator itself is not allowed to assume it.
                let top_y = crate::math::floor(upper_bound.compute(ctx) / f64::from(*cell_height))
                    .wrapping_mul(*cell_height);
                if top_y <= *lower_bound {
                    return f64::from(*lower_bound);
                }
                let mut block_y = top_y;
                while block_y >= *lower_bound {
                    // A fresh single-point context: the probe walks y and
                    // must not inherit the caller's interpolated slots,
                    // which belong to a different block.
                    let probe = FunctionContext::new(ctx.block_x, block_y, ctx.block_z);
                    if density.compute(probe) > 0.0 {
                        return f64::from(block_y);
                    }
                    block_y -= *cell_height;
                }
                f64::from(*lower_bound)
            }

            Self::BlendAlpha => 1.0,
            Self::BlendOffset => 0.0,
            Self::Marker {
                kind,
                wrapped,
                slot,
            } => match kind {
                // Inside a chunk fill this marker's value came off the
                // lattice; reading the slot is the whole point of
                // interpolation, and recursing here would defeat it.
                MarkerKind::Interpolated => match slot {
                    Some(index) if *index < ctx.interpolated.len() => ctx.interpolated[*index],
                    _ => wrapped.compute(ctx),
                },
                // Snapped to the quart column and looked up; outside the
                // cached square vanilla computes directly, and so do we.
                MarkerKind::CacheAllInCell => match slot {
                    Some(index) if *index < ctx.cell_all.len() => ctx.cell_all[*index],
                    _ => wrapped.compute(ctx),
                },
                MarkerKind::FlatCache => slot
                    .and_then(|index| ctx.flat.get(index, ctx.block_x, ctx.block_z))
                    .unwrap_or_else(|| wrapped.compute(ctx)),
                // Memoized for the point being evaluated. Value-transparent
                // — a pure function of position cannot change by being
                // remembered — which is why this could be added after parity
                // rather than before it.
                MarkerKind::CacheOnce => match (slot, ctx.cache_once) {
                    (Some(index), Some(scratch)) => {
                        scratch.get_or_compute(*index, || wrapped.compute(ctx))
                    }
                    _ => wrapped.compute(ctx),
                },
                // Cache2D, CacheAllInCell and BlendDensity are likewise
                // value-transparent: they memoize or, for BlendDensity with
                // an empty blender, vanish. Passing through is exact.
                _ => wrapped.compute(ctx),
            },

            Self::IntervalSelect {
                input,
                thresholds,
                functions,
            } => {
                let value = input.compute(ctx);
                // Strictly `<`, first match wins — so a value exactly on a
                // threshold belongs to the band *above* it.
                let index = thresholds
                    .iter()
                    .position(|&threshold| value < threshold)
                    .unwrap_or(functions.len() - 1);
                functions[index].compute(ctx)
            }
        }
    }

    /// Lower bound of the function's range.
    #[must_use]
    pub fn min_value(&self) -> f64 {
        match self {
            Self::Constant(value) => *value,
            Self::TwoArgument { min, .. }
            | Self::MulOrAdd { min, .. }
            | Self::Map { min, .. }
            | Self::Clamp { min, .. } => *min,
            Self::RangeChoice {
                when_in_range,
                when_out_of_range,
                ..
            } => java_min(when_in_range.min_value(), when_out_of_range.min_value()),
            Self::YClampedGradient {
                from_value,
                to_value,
                ..
            } => java_min(*from_value, *to_value),
            Self::Noise { .. } | Self::Shift { .. } | Self::ShiftedNoise { .. } => {
                -self.max_value()
            }
            Self::OldBlendedNoise(noise) => noise.min_value(),
            Self::Spline(spline) => f64::from(spline.min_value()),
            Self::EndIslands(_) => end::EndIslandNoise::min_value(),
            // `Beardifier.minValue()` is literally `-Infinity` (offset 0) and
            // `maxValue()` `+Infinity`. Vanilla makes no attempt to bound the
            // sum over an arbitrary piece list, and neither does this.
            Self::Beardifier(_) => f64::NEG_INFINITY,
            // The scan can always fall through to the floor.
            Self::FindTopSurface { lower_bound, .. } => f64::from(*lower_bound),
            Self::BlendAlpha => 1.0,
            Self::BlendOffset => 0.0,
            Self::Marker { kind, wrapped, .. } => {
                if *kind == MarkerKind::BlendDensity {
                    f64::NEG_INFINITY
                } else {
                    wrapped.min_value()
                }
            }
            // Any branch may be taken, so the bound is over all of them.
            // Vanilla seeds the fold with Double.MAX_VALUE, not +inf.
            Self::IntervalSelect { functions, .. } => functions
                .iter()
                .fold(f64::MAX, |acc, f| java_min(f.min_value(), acc)),
        }
    }

    /// Upper bound of the function's range.
    #[must_use]
    pub fn max_value(&self) -> f64 {
        match self {
            Self::Constant(value) => *value,
            Self::TwoArgument { max, .. }
            | Self::MulOrAdd { max, .. }
            | Self::Map { max, .. }
            | Self::Clamp { max, .. } => *max,
            Self::RangeChoice {
                when_in_range,
                when_out_of_range,
                ..
            } => java_max(when_in_range.max_value(), when_out_of_range.max_value()),
            Self::YClampedGradient {
                from_value,
                to_value,
                ..
            } => java_max(*from_value, *to_value),
            Self::Noise { noise, .. } => noise.max_value(),
            // Four, because the offset is scaled by 4 after sampling.
            Self::Shift { noise, .. } => noise.max_value() * 4.0,
            Self::ShiftedNoise { noise, .. } => noise.max_value(),
            Self::OldBlendedNoise(noise) => noise.max_value(),
            Self::Spline(spline) => f64::from(spline.max_value()),
            Self::EndIslands(_) => end::EndIslandNoise::max_value(),
            Self::Beardifier(_) => f64::INFINITY,
            // `Math.max(lowerBound, upperBound.maxValue())` — note it is the
            // *unsnapped* ceiling's bound, not the snapped `top_y`.
            Self::FindTopSurface {
                upper_bound,
                lower_bound,
                ..
            } => java_max(f64::from(*lower_bound), upper_bound.max_value()),
            Self::BlendAlpha => 1.0,
            Self::BlendOffset => 0.0,
            Self::Marker { kind, wrapped, .. } => {
                if *kind == MarkerKind::BlendDensity {
                    f64::INFINITY
                } else {
                    wrapped.max_value()
                }
            }
            Self::IntervalSelect { functions, .. } => functions
                .iter()
                .fold(f64::MIN, |acc, f| java_max(f.max_value(), acc)),
        }
    }

    /// Assign an index to every `Interpolated` marker, depth-first, and
    /// return each one's wrapped function.
    ///
    /// Indices must be stable and position-based, not assigned during
    /// evaluation: `Min`/`Max` short-circuit and `RangeChoice` picks a
    /// branch, so a counter incremented while computing would desync
    /// between blocks.
    pub fn number_interpolators(&mut self) -> Vec<DensityFunction> {
        let mut wrapped = Vec::new();
        self.number_markers_into(MarkerKind::Interpolated, &mut wrapped);
        wrapped
    }

    /// How many `CacheOnce` slots this graph uses — one past the highest
    /// slot assigned at construction.
    ///
    /// Walks rather than being tracked, so a graph assembled from several
    /// sources still reports a size big enough for all of them.
    #[must_use]
    pub fn cache_once_slots(&self) -> usize {
        let mut max = 0usize;
        self.visit_cache_once_slots(&mut max);
        max
    }

    fn visit_cache_once_slots(&self, max: &mut usize) {
        match self {
            Self::Constant(_)
            | Self::YClampedGradient { .. }
            | Self::Noise { .. }
            | Self::Shift { .. }
            | Self::OldBlendedNoise(_)
            | Self::BlendAlpha
            | Self::BlendOffset
            | Self::Spline(_)
            | Self::EndIslands(_)
            | Self::Beardifier(_) => {}

            Self::TwoArgument { first, second, .. } => {
                first.visit_cache_once_slots(max);
                second.visit_cache_once_slots(max);
            }
            Self::FindTopSurface {
                density,
                upper_bound,
                ..
            } => {
                density.visit_cache_once_slots(max);
                upper_bound.visit_cache_once_slots(max);
            }
            Self::MulOrAdd { input, .. } | Self::Map { input, .. } | Self::Clamp { input, .. } => {
                input.visit_cache_once_slots(max);
            }
            Self::RangeChoice {
                input,
                when_in_range,
                when_out_of_range,
                ..
            } => {
                input.visit_cache_once_slots(max);
                when_in_range.visit_cache_once_slots(max);
                when_out_of_range.visit_cache_once_slots(max);
            }
            Self::ShiftedNoise {
                shift_x,
                shift_y,
                shift_z,
                ..
            } => {
                shift_x.visit_cache_once_slots(max);
                shift_y.visit_cache_once_slots(max);
                shift_z.visit_cache_once_slots(max);
            }
            Self::IntervalSelect {
                input, functions, ..
            } => {
                input.visit_cache_once_slots(max);
                for f in functions {
                    f.visit_cache_once_slots(max);
                }
            }
            Self::Marker {
                kind,
                wrapped,
                slot,
            } => {
                if *kind == MarkerKind::CacheOnce {
                    if let Some(index) = slot {
                        *max = (*max).max(index + 1);
                    }
                }
                wrapped.visit_cache_once_slots(max);
            }
        }
    }

    /// As [`Self::number_interpolators`], but continuing an existing list.
    ///
    /// Several roots share one chunk's interpolator table in vanilla — the
    /// `NoiseChunk.wrapped` map is per chunk, not per function — so indices
    /// must stay unique across all of them.
    pub fn number_interpolators_into(&mut self, out: &mut Vec<DensityFunction>) {
        self.number_markers_into(MarkerKind::Interpolated, out);
    }

    pub(crate) fn number_markers_into(&mut self, want: MarkerKind, out: &mut Vec<DensityFunction>) {
        match self {
            Self::Constant(_)
            | Self::YClampedGradient { .. }
            | Self::Noise { .. }
            | Self::Shift { .. }
            | Self::OldBlendedNoise(_)
            | Self::BlendAlpha
            | Self::BlendOffset
            | Self::EndIslands(_)
            | Self::Beardifier(_) => {}

            Self::TwoArgument { first, second, .. } => {
                first.number_markers_into(want, out);
                second.number_markers_into(want, out);
            }
            // `mapChildren` visits density then upperBound, so that is the
            // numbering order too — an interpolated marker under either one
            // would desync if this walked them the other way round.
            Self::FindTopSurface {
                density,
                upper_bound,
                ..
            } => {
                density.number_markers_into(want, out);
                upper_bound.number_markers_into(want, out);
            }
            Self::MulOrAdd { input, .. } | Self::Map { input, .. } | Self::Clamp { input, .. } => {
                input.number_markers_into(want, out);
            }
            Self::RangeChoice {
                input,
                when_in_range,
                when_out_of_range,
                ..
            } => {
                input.number_markers_into(want, out);
                when_in_range.number_markers_into(want, out);
                when_out_of_range.number_markers_into(want, out);
            }
            Self::ShiftedNoise {
                shift_x,
                shift_y,
                shift_z,
                ..
            } => {
                shift_x.number_markers_into(want, out);
                shift_y.number_markers_into(want, out);
                shift_z.number_markers_into(want, out);
            }
            Self::IntervalSelect {
                input, functions, ..
            } => {
                input.number_markers_into(want, out);
                for f in functions {
                    f.number_markers_into(want, out);
                }
            }
            // Splines carry density functions as coordinates, but vanilla
            // never puts an interpolated marker inside one — and if it did,
            // the spline evaluates in f32 and could not carry the value
            // faithfully. Left unvisited deliberately.
            Self::Spline(_) => {}

            Self::Marker {
                kind,
                wrapped,
                slot,
            } => {
                wrapped.number_markers_into(want, out);
                if *kind == want {
                    *slot = Some(out.len());
                    out.push((**wrapped).clone());
                }
            }
        }
    }

    /// Fill `output` by evaluating one context per slot.
    ///
    /// Vanilla overrides `fillArray` per operator to hoist work out of the
    /// loop (`Ap2::ADD` fills both children into buffers, `Constant` uses
    /// `Arrays.fill`). Those are optimizations over this same semantics, and
    /// they are worth adding only once a benchmark says so — writing them
    /// now would be N more places for a parity bug to hide.
    pub fn fill_array(&self, output: &mut [f64], contexts: &[FunctionContext]) {
        assert_eq!(
            output.len(),
            contexts.len(),
            "fill_array needs one context per output slot"
        );
        for (slot, ctx) in output.iter_mut().zip(contexts) {
            *slot = self.compute(*ctx);
        }
    }
}

/// Smart constructors.
///
/// These are the only supported way to build a graph: they compute the
/// bounds that evaluation depends on, and apply vanilla's construction-time
/// rewrites.
pub mod build {
    use super::{Ap2, DensityFunction, Mapped, MulAdd, NoiseHolder, ShiftAxes};
    use crate::math::{java_max, java_min};
    use crate::noise::BlendedNoise;

    #[must_use]
    pub fn constant(value: f64) -> DensityFunction {
        DensityFunction::Constant(value)
    }

    /// `TwoArgumentSimpleFunction.create`.
    ///
    /// Computes bounds, then — for `Add`/`Mul` with a constant on either
    /// side — rewrites into [`DensityFunction::MulOrAdd`]. That rewrite is
    /// not an optimization: `MulOrAdd` drops `Mul`'s zero short-circuit, so
    /// `mul(constant(0), f)` and `mul(f, g)` where `g` happens to be zero
    /// behave differently on infinities. Vanilla does it, so we do.
    #[must_use]
    pub fn two_argument(
        kind: Ap2,
        first: DensityFunction,
        second: DensityFunction,
    ) -> DensityFunction {
        let (min1, max1) = (first.min_value(), first.max_value());
        let (min2, max2) = (second.min_value(), second.max_value());

        let min = match kind {
            Ap2::Add => min1 + min2,
            Ap2::Mul => {
                if min1 > 0.0 && min2 > 0.0 {
                    min1 * min2
                } else if max1 < 0.0 && max2 < 0.0 {
                    max1 * max2
                } else {
                    java_min(min1 * max2, max1 * min2)
                }
            }
            Ap2::Min => java_min(min1, min2),
            Ap2::Max => java_max(min1, min2),
        };
        let max = match kind {
            Ap2::Add => max1 + max2,
            Ap2::Mul => {
                if min1 > 0.0 && min2 > 0.0 {
                    max1 * max2
                } else if max1 < 0.0 && max2 < 0.0 {
                    min1 * min2
                } else {
                    java_max(min1 * min2, max1 * max2)
                }
            }
            Ap2::Min => java_min(max1, max2),
            Ap2::Max => java_max(max1, max2),
        };

        if matches!(kind, Ap2::Mul | Ap2::Add) {
            let mul_add = if kind == Ap2::Add {
                MulAdd::Add
            } else {
                MulAdd::Mul
            };
            // Order matters: vanilla tests argument1 first, so when *both*
            // are constants the surviving input is argument2.
            if let DensityFunction::Constant(argument) = first {
                return DensityFunction::MulOrAdd {
                    kind: mul_add,
                    input: Box::new(second),
                    argument,
                    min,
                    max,
                };
            }
            if let DensityFunction::Constant(argument) = second {
                return DensityFunction::MulOrAdd {
                    kind: mul_add,
                    input: Box::new(first),
                    argument,
                    min,
                    max,
                };
            }
        }

        DensityFunction::TwoArgument {
            kind,
            first: Box::new(first),
            second: Box::new(second),
            min,
            max,
            second_min: min2,
            second_max: max2,
        }
    }

    #[must_use]
    pub fn add(a: DensityFunction, b: DensityFunction) -> DensityFunction {
        two_argument(Ap2::Add, a, b)
    }

    #[must_use]
    pub fn mul(a: DensityFunction, b: DensityFunction) -> DensityFunction {
        two_argument(Ap2::Mul, a, b)
    }

    #[must_use]
    pub fn min(a: DensityFunction, b: DensityFunction) -> DensityFunction {
        two_argument(Ap2::Min, a, b)
    }

    #[must_use]
    pub fn max(a: DensityFunction, b: DensityFunction) -> DensityFunction {
        two_argument(Ap2::Max, a, b)
    }

    /// `Mapped.create`.
    ///
    /// The bound rules are irregular and deliberately so:
    /// - `Invert` over a range straddling zero is unbounded (it passes
    ///   through a pole), and otherwise its image is *reversed*.
    /// - `Abs`/`Square` clamp the lower bound at zero rather than trusting
    ///   the transformed endpoints, which would be wrong whenever the input
    ///   range spans zero.
    #[must_use]
    pub fn map(kind: Mapped, input: DensityFunction) -> DensityFunction {
        let min_value = input.min_value();
        let max_value = input.max_value();
        let min_image = kind.transform(min_value);
        let max_image = kind.transform(max_value);

        let (min, max) = if kind == Mapped::Invert {
            if min_value < 0.0 && max_value > 0.0 {
                (f64::NEG_INFINITY, f64::INFINITY)
            } else {
                (max_image, min_image)
            }
        } else if matches!(kind, Mapped::Abs | Mapped::Square) {
            (java_max(0.0, min_value), java_max(min_image, max_image))
        } else {
            (min_image, max_image)
        };

        DensityFunction::Map {
            kind,
            input: Box::new(input),
            min,
            max,
        }
    }

    #[must_use]
    pub fn clamp(input: DensityFunction, min: f64, max: f64) -> DensityFunction {
        DensityFunction::Clamp {
            input: Box::new(input),
            min,
            max,
        }
    }

    #[must_use]
    pub fn range_choice(
        input: DensityFunction,
        min_inclusive: f64,
        max_exclusive: f64,
        when_in_range: DensityFunction,
        when_out_of_range: DensityFunction,
    ) -> DensityFunction {
        DensityFunction::RangeChoice {
            input: Box::new(input),
            min_inclusive,
            max_exclusive,
            when_in_range: Box::new(when_in_range),
            when_out_of_range: Box::new(when_out_of_range),
        }
    }

    #[must_use]
    pub fn y_clamped_gradient(
        from_y: i32,
        to_y: i32,
        from_value: f64,
        to_value: f64,
    ) -> DensityFunction {
        DensityFunction::YClampedGradient {
            from_y,
            to_y,
            from_value,
            to_value,
        }
    }

    #[must_use]
    pub fn noise(noise: NoiseHolder, xz_scale: f64, y_scale: f64) -> DensityFunction {
        DensityFunction::Noise {
            noise,
            xz_scale,
            y_scale,
        }
    }

    #[must_use]
    pub fn shift(noise: NoiseHolder) -> DensityFunction {
        DensityFunction::Shift {
            noise,
            axes: ShiftAxes::Xyz,
        }
    }

    #[must_use]
    pub fn shift_a(noise: NoiseHolder) -> DensityFunction {
        DensityFunction::Shift {
            noise,
            axes: ShiftAxes::Xz,
        }
    }

    #[must_use]
    pub fn shift_b(noise: NoiseHolder) -> DensityFunction {
        DensityFunction::Shift {
            noise,
            axes: ShiftAxes::Zx,
        }
    }

    #[must_use]
    pub fn shifted_noise(
        shift_x: DensityFunction,
        shift_y: DensityFunction,
        shift_z: DensityFunction,
        xz_scale: f64,
        y_scale: f64,
        noise: NoiseHolder,
    ) -> DensityFunction {
        DensityFunction::ShiftedNoise {
            shift_x: Box::new(shift_x),
            shift_y: Box::new(shift_y),
            shift_z: Box::new(shift_z),
            xz_scale,
            y_scale,
            noise,
        }
    }

    #[must_use]
    pub fn old_blended_noise(noise: BlendedNoise) -> DensityFunction {
        DensityFunction::OldBlendedNoise(std::sync::Arc::new(noise))
    }

    /// `DensityFunctions.spline` — bounds come from the spline itself, which
    /// computed them at construction.
    #[must_use]
    pub fn spline(spline: super::CubicSpline) -> DensityFunction {
        DensityFunction::Spline(spline)
    }

    /// `DensityFunctions.lerp(factor, first, second)` for a **constant**
    /// `first` — a composite, not a node.
    ///
    /// Expanded exactly as vanilla expands it:
    /// `add(mul(factor, add(second, constant(-first))), constant(first))`.
    /// The obvious `first*(1-t) + second*t` is algebraically equal and
    /// bit-wise different, and `build` is where the constant-folding
    /// rewrites happen, so the expansion has to go through these helpers.
    ///
    /// The general (non-constant `first`) overload additionally wraps the
    /// factor in `cacheOnce`, which is a wave-2 operator — see
    /// `swarm/tasks/w2-density-operators.md`.
    /// `DensityFunctions.intervalSelect`.
    ///
    /// # Panics
    /// If `thresholds` is not exactly one shorter than `functions`, or is not
    /// ascending — both are validation errors in vanilla's codec, and both
    /// silently corrupt band selection if allowed through.
    #[must_use]
    pub fn interval_select(
        input: DensityFunction,
        thresholds: Vec<f64>,
        functions: Vec<DensityFunction>,
    ) -> DensityFunction {
        assert_eq!(
            thresholds.len(),
            functions.len().saturating_sub(1),
            "Expected {} thresholds for {} functions, but got {}",
            functions.len().saturating_sub(1),
            functions.len(),
            thresholds.len()
        );
        assert!(
            thresholds.windows(2).all(|w| w[0] <= w[1]),
            "Threshold values must be ordered from smallest to largest"
        );
        DensityFunction::IntervalSelect {
            input: Box::new(input),
            thresholds,
            functions,
        }
    }

    /// `QuantizedSpaghettiRarity.noiseFunctionForRarity`.
    ///
    /// Rarer bands sample the same noise at a coarser scale and scale the
    /// result up, so cave frequency and cave size move together.
    #[must_use]
    pub fn noise_for_rarity(noise: NoiseHolder, rarity: f64) -> DensityFunction {
        mul(
            constant(rarity),
            self::noise(noise, 1.0 / rarity, 1.0 / rarity),
        )
    }

    /// `DensityFunctions.endIslands`.
    #[must_use]
    pub fn end_islands(seed: i64) -> DensityFunction {
        DensityFunction::EndIslands(std::sync::Arc::new(super::end::EndIslandNoise::new(seed)))
    }

    #[must_use]
    pub fn blend_alpha() -> DensityFunction {
        DensityFunction::BlendAlpha
    }

    #[must_use]
    pub fn blend_offset() -> DensityFunction {
        DensityFunction::BlendOffset
    }

    #[must_use]
    pub fn marker(kind: super::MarkerKind, wrapped: DensityFunction) -> DensityFunction {
        DensityFunction::Marker {
            kind,
            wrapped: Box::new(wrapped),
            slot: None,
        }
    }

    #[must_use]
    pub fn interpolated(f: DensityFunction) -> DensityFunction {
        marker(super::MarkerKind::Interpolated, f)
    }

    #[must_use]
    pub fn flat_cache(f: DensityFunction) -> DensityFunction {
        marker(super::MarkerKind::FlatCache, f)
    }

    #[must_use]
    pub fn cache_2d(f: DensityFunction) -> DensityFunction {
        marker(super::MarkerKind::Cache2D, f)
    }

    /// `DensityFunctions.cacheOnce`, with its cache slot fixed at
    /// construction.
    ///
    /// The slot cannot be assigned by a later tree walk, and that is the
    /// whole point. In vanilla `slopedCheese` is **one object referenced
    /// three times**, and `NoiseChunk.wrap` maps it to one cache instance, so
    /// all three reads share it. Our graph is a tree, so those three
    /// references are three cloned subtrees — a numbering pass would give
    /// each its own slot and the memoization would never hit.
    ///
    /// Fixing the slot before the clones happen makes them share it, which is
    /// what reproduces vanilla's sharing. `slot` must be unique per *logical*
    /// function, so callers thread a counter; see
    /// [`DensityFunction::cache_once_slots`] for reading the total back.
    #[must_use]
    pub fn cache_once(slot: &mut usize, f: DensityFunction) -> DensityFunction {
        let index = *slot;
        *slot += 1;
        DensityFunction::Marker {
            kind: super::MarkerKind::CacheOnce,
            wrapped: Box::new(f),
            slot: Some(index),
        }
    }

    #[must_use]
    pub fn cache_all_in_cell(f: DensityFunction) -> DensityFunction {
        marker(super::MarkerKind::CacheAllInCell, f)
    }

    #[must_use]
    pub fn blend_density(f: DensityFunction) -> DensityFunction {
        marker(super::MarkerKind::BlendDensity, f)
    }

    /// `DensityFunctions.lerp` — the general form.
    ///
    /// Dispatches on `first` being a `Constant`, exactly as vanilla does.
    /// That is why [`DensityFunction::BlendAlpha`] / `BlendOffset` are their
    /// own variants rather than constants: `blendOffset()` is not a
    /// `Constant`, so it must take the `cache_once` branch.
    ///
    /// Vanilla shares one `cacheOnce` node between both uses of the alpha;
    /// our tree clones it. Identical semantics — the marker is a passthrough
    /// — and it costs a second evaluation once real caching exists, which is
    /// a `NoiseChunk` concern rather than a correctness one.
    #[must_use]
    pub fn lerp(
        slot: &mut usize,
        alpha: DensityFunction,
        first: DensityFunction,
        second: DensityFunction,
    ) -> DensityFunction {
        if let DensityFunction::Constant(value) = first {
            return lerp_constant(alpha, value, second);
        }
        let alpha_cached = cache_once(slot, alpha);
        let one_minus_alpha = add(mul(alpha_cached.clone(), constant(-1.0)), constant(1.0));
        add(mul(first, one_minus_alpha), mul(second, alpha_cached))
    }

    /// `NoiseRouterData.slide` — force density toward fixed targets near the
    /// world's ceiling and floor.
    ///
    /// Without it terrain would simply stop at the build limit and leave a
    /// flat lid; the slide ramps density to `top_target` over the last
    /// `top_start_y - top_end_y` blocks so mountains thin out, and to
    /// `bottom_target` at the bottom so the world has a solid floor.
    ///
    /// Both ends use the `double`-first `lerp` overload, which is the direct
    /// expansion and never touches `cache_once`.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn slide(
        caves: DensityFunction,
        min_y: i32,
        height: i32,
        top_start_y: i32,
        top_end_y: i32,
        top_target: f64,
        bottom_start_y: i32,
        bottom_end_y: i32,
        bottom_target: f64,
    ) -> DensityFunction {
        // Note the top gradient runs 1 -> 0 measured *down* from the
        // ceiling, and the bottom one 0 -> 1 measured up from the floor.
        let top_factor = y_clamped_gradient(
            min_y + height - top_start_y,
            min_y + height - top_end_y,
            1.0,
            0.0,
        );
        let with_top = lerp_constant(top_factor, top_target, caves);
        let bottom_factor =
            y_clamped_gradient(min_y + bottom_start_y, min_y + bottom_end_y, 0.0, 1.0);
        lerp_constant(bottom_factor, bottom_target, with_top)
    }

    /// `NoiseRouterData.slideOverworld`.
    ///
    /// Amplified slides over a much shorter distance (16 blocks instead of
    /// 80) toward a far higher floor target, which is why its mountains keep
    /// going almost to the build limit.
    #[must_use]
    pub fn slide_overworld(amplified: bool, caves: DensityFunction) -> DensityFunction {
        slide(
            caves,
            -64,
            384,
            if amplified { 16 } else { 80 },
            if amplified { 0 } else { 64 },
            -0.078_125,
            0,
            24,
            if amplified { 0.4 } else { 0.117_187_5 },
        )
    }

    /// `NoiseRouterData.postProcess` — the last step before density becomes
    /// blocks.
    ///
    /// `squeeze` is what stops extreme densities from producing knife-edge
    /// geometry: it compresses the tails while leaving the sign — and so the
    /// air/solid boundary — untouched.
    ///
    /// # The `0.64` is inside the marker, and that is not cosmetic
    ///
    /// Vanilla is `interpolated(mul(blended, 0.64)).squeeze()`, so the scale
    /// is applied **at the lattice corners**, before the blend. Hoisting it
    /// outside — `mul(interpolated(blended), 0.64)` — is algebraically the
    /// same and gives identical results at every corner, because there the
    /// blend is the identity. In a cell interior it is not:
    /// `0.64 * lerp(t, a, b)` and `lerp(t, 0.64a, 0.64b)` round differently.
    ///
    /// That misplacement was the residual gap in `golden_interpolated` after
    /// the lerp-order fix, and it survived so long precisely because every
    /// corner-based check is blind to it. It was caught by dumping each
    /// marker's wrapped value directly from vanilla — the ratio came out as
    /// exactly 1.5625 = 1/0.64 at all 392 corners
    /// (`golden_interpolator_corners`).
    #[must_use]
    pub fn post_process(slide: DensityFunction) -> DensityFunction {
        let blended = blend_density(slide);
        map(
            super::Mapped::Squeeze,
            interpolated(mul(blended, constant(0.64))),
        )
    }

    /// `DensityFunctions.findTopSurface`.
    ///
    /// `lower_bound` is a block y and `cell_height` the scan step; both are
    /// integers because the result is a block y, not a density.
    #[must_use]
    pub fn find_top_surface(
        density: DensityFunction,
        upper_bound: DensityFunction,
        lower_bound: i32,
        cell_height: i32,
    ) -> DensityFunction {
        assert!(cell_height > 0, "cell_height must be positive");
        DensityFunction::FindTopSurface {
            density: Box::new(density),
            upper_bound: Box::new(upper_bound),
            lower_bound,
            cell_height,
        }
    }

    /// `NoiseRouterData.remap` — an affine rescale from one range to another.
    ///
    /// The factor and offset are folded at build time exactly as vanilla
    /// folds them, so the arithmetic that reaches `compute` is the same two
    /// constants rather than a four-constant expression evaluated per point.
    #[must_use]
    pub fn remap(
        input: DensityFunction,
        from_min: f64,
        from_max: f64,
        to_min: f64,
        to_max: f64,
    ) -> DensityFunction {
        let factor = (to_max - to_min) / (from_max - from_min);
        let offset = to_min - from_min * factor;
        add(mul(input, constant(factor)), constant(offset))
    }

    /// `NoiseRouterData.splineWithBlending`.
    #[must_use]
    pub fn spline_with_blending(
        slot: &mut usize,
        spline: DensityFunction,
        blending_target: DensityFunction,
    ) -> DensityFunction {
        flat_cache(cache_2d(lerp(slot, blend_alpha(), blending_target, spline)))
    }

    #[must_use]
    pub fn lerp_constant(
        factor: DensityFunction,
        first: f64,
        second: DensityFunction,
    ) -> DensityFunction {
        add(mul(factor, add(second, constant(-first))), constant(first))
    }

    /// `noiseGradientDensity` — how depth and factor become density.
    ///
    /// `quarter_negative` before the scale is what makes terrain asymmetric:
    /// density above the surface is damped to a quarter, so the ground has a
    /// hard top and a soft bottom.
    #[must_use]
    pub fn noise_gradient_density(
        factor: DensityFunction,
        depth_with_jaggedness: DensityFunction,
    ) -> DensityFunction {
        let unscaled = mul(depth_with_jaggedness, factor);
        mul(constant(4.0), map(super::Mapped::QuarterNegative, unscaled))
    }

    /// `offsetToDepth` — depth is the offset plus a fixed vertical ramp.
    ///
    /// The ramp runs 1.5 at y=-64 down to -1.5 at y=320, which is what makes
    /// "deeper" mean "denser" before any noise is involved.
    #[must_use]
    pub fn offset_to_depth(offset: DensityFunction) -> DensityFunction {
        add(y_clamped_gradient(-64, 320, 1.5, -1.5), offset)
    }
}

#[cfg(test)]
mod tests {
    use super::build::*;
    use super::*;

    const ORIGIN: FunctionContext = FunctionContext::new(0, 0, 0);

    #[test]
    fn constant_folding_rewrites_into_mul_or_add() {
        // A constant on either side must produce MulOrAdd, not TwoArgument.
        let folded = mul(constant(3.0), constant(4.0));
        assert!(matches!(folded, DensityFunction::MulOrAdd { .. }));
        assert_eq!(folded.compute(ORIGIN), 12.0);

        let general = min(
            y_clamped_gradient(0, 10, 0.0, 1.0),
            y_clamped_gradient(0, 10, 1.0, 0.0),
        );
        assert!(matches!(general, DensityFunction::TwoArgument { .. }));
    }

    #[test]
    fn mul_short_circuits_on_zero_but_mul_or_add_does_not() {
        // The rewrite is observable, which is the whole reason `build`
        // exists. `Ap2::Mul` returns 0 without evaluating the right side;
        // `MulOrAdd` multiplies, and 0 * inf is NaN.
        let short_circuit = DensityFunction::TwoArgument {
            kind: Ap2::Mul,
            first: Box::new(constant(0.0)),
            second: Box::new(constant(f64::INFINITY)),
            min: 0.0,
            max: 0.0,
            second_min: f64::INFINITY,
            second_max: f64::INFINITY,
        };
        assert_eq!(short_circuit.compute(ORIGIN), 0.0);

        let rewritten = mul(constant(0.0), constant(f64::INFINITY));
        assert!(rewritten.compute(ORIGIN).is_nan());
    }

    #[test]
    fn min_skips_the_second_argument_when_it_cannot_win() {
        // first is always 0; second's range is [1, 1]. `Min` must return 0
        // without consulting second — and the result must still be 0 if it
        // does. This pins the branch rather than the arithmetic.
        let f = min(constant(0.0), y_clamped_gradient(0, 1, 1.0, 1.0));
        assert_eq!(f.compute(ORIGIN), 0.0);
    }

    #[test]
    fn invert_bounds_go_infinite_across_a_pole() {
        let straddling = map(Mapped::Invert, y_clamped_gradient(0, 10, -1.0, 1.0));
        assert_eq!(straddling.min_value(), f64::NEG_INFINITY);
        assert_eq!(straddling.max_value(), f64::INFINITY);

        // Entirely positive: the image is reversed, not passed through.
        let positive = map(Mapped::Invert, y_clamped_gradient(0, 10, 2.0, 4.0));
        assert_eq!(positive.min_value(), 0.25);
        assert_eq!(positive.max_value(), 0.5);
    }

    #[test]
    fn abs_and_square_clamp_the_lower_bound_at_zero() {
        let f = map(Mapped::Square, y_clamped_gradient(0, 10, -3.0, 2.0));
        assert_eq!(f.min_value(), 0.0, "a range spanning zero has min 0");
        assert_eq!(f.max_value(), 9.0);
    }

    #[test]
    fn y_clamped_gradient_clamps_outside_the_span() {
        let f = y_clamped_gradient(0, 10, 0.0, 1.0);
        assert_eq!(f.compute(FunctionContext::new(0, -5, 0)), 0.0);
        assert_eq!(f.compute(FunctionContext::new(0, 5, 0)), 0.5);
        assert_eq!(f.compute(FunctionContext::new(0, 50, 0)), 1.0);
    }

    #[test]
    fn range_choice_is_half_open() {
        let f = range_choice(
            y_clamped_gradient(0, 10, 0.0, 10.0),
            2.0,
            5.0,
            constant(1.0),
            constant(0.0),
        );
        assert_eq!(
            f.compute(FunctionContext::new(0, 2, 0)),
            1.0,
            "min inclusive"
        );
        assert_eq!(
            f.compute(FunctionContext::new(0, 5, 0)),
            0.0,
            "max exclusive"
        );
    }

    #[test]
    fn unbound_noise_is_defined_not_fatal() {
        let f = noise(NoiseHolder::unbound(), 1.0, 1.0);
        assert_eq!(f.compute(ORIGIN), 0.0);
        assert_eq!(f.max_value(), 2.0);
    }

    /// A density that is positive only at or below `solid_at_or_below`, so
    /// the scan's stopping point is known exactly.
    fn step_density(solid_at_or_below: i32) -> DensityFunction {
        // y_clamped_gradient from +1 at the floor to -1 at the ceiling
        // crosses zero once; using a gradient rather than a constant keeps
        // the test honest about the `> 0.0` comparison.
        build::y_clamped_gradient(solid_at_or_below, solid_at_or_below + 1, 1.0, -1.0)
    }

    #[test]
    fn find_top_surface_snaps_the_ceiling_down_to_a_cell() {
        // upperBound 30 with cellHeight 8 snaps to 24, so a column solid
        // everywhere returns 24 and never reports 30.
        let f = build::find_top_surface(build::constant(1.0), build::constant(30.0), -64, 8);
        assert_eq!(f.compute(FunctionContext::new(0, 0, 0)), 24.0);
    }

    #[test]
    fn find_top_surface_walks_down_in_cell_steps() {
        // Solid at y <= 3. Starting at 24 and stepping by 8 visits
        // 24, 16, 8, 0 — so it lands on 0, not on 3. The scan resolution is
        // the cell, which is why this feeds a *preliminary* surface level.
        let f = build::find_top_surface(step_density(3), build::constant(30.0), -64, 8);
        assert_eq!(f.compute(FunctionContext::new(0, 0, 0)), 0.0);
    }

    #[test]
    fn find_top_surface_falls_through_to_the_floor() {
        let f = build::find_top_surface(build::constant(-1.0), build::constant(300.0), -64, 8);
        assert_eq!(f.compute(FunctionContext::new(0, 0, 0)), -64.0);
    }

    #[test]
    fn find_top_surface_short_circuits_below_the_floor() {
        // topY <= lowerBound returns the floor without evaluating density at
        // all — a constant 1.0 would otherwise report the ceiling.
        let f = build::find_top_surface(build::constant(1.0), build::constant(-100.0), -64, 8);
        assert_eq!(f.compute(FunctionContext::new(0, 0, 0)), -64.0);
    }

    #[test]
    fn find_top_surface_snap_floors_rather_than_truncates() {
        // -60 / 8 is -7.5. Flooring gives -8 -> -64; truncating would give
        // -7 -> -56, one cell too high, and every aquifer below y=0 would
        // sit a cell off.
        let f = build::find_top_surface(build::constant(1.0), build::constant(-60.0), -128, 8);
        assert_eq!(f.compute(FunctionContext::new(0, 0, 0)), -64.0);
    }

    #[test]
    fn find_top_surface_bounds_use_the_unsnapped_ceiling() {
        let f = build::find_top_surface(build::constant(1.0), build::constant(30.0), -64, 8);
        assert_eq!(f.min_value(), -64.0);
        // 30, not the snapped 24: vanilla takes upperBound.maxValue().
        assert_eq!(f.max_value(), 30.0);
    }
}
