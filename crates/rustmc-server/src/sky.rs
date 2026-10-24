//! The sky: time of day, sun angle, and how dark the sky makes the world.
//!
//! # Why this module exists, and what it unblocks
//!
//! Three things in `crates/rustmc-server` were refused outright for want of
//! a sky, and each one said so in its own words rather than pointing here:
//!
//! * `useblock.rs` transcribed `DaylightDetectorBlock.updateSignalStrength`
//!   into a doc comment and **did not run it**, because "every input on its
//!   first two lines is a light and sky model this server does not have";
//! * `data/vanilla/use_without_item.json` refuses 128 states — the sixteen
//!   beds — with `UseGolden's level was asked for environmentAttributes`,
//!   and `BedRule.can_sleep` is `when_dark`, which is this model;
//! * `Level.getSkyDarken()` had no answer at all, so
//!   `getEffectiveSkyBrightness` had none either.
//!
//! Those were **one gap seen from three sides**. This module is the gap.
//!
//! # In 26.2 the sky is DATA, and that is the whole trap
//!
//! Every earlier version of this game computed the sun's angle from the time
//! of day with a closed-form expression, and a port that writes that
//! expression down looks right and is wrong. 26.2 has no such expression.
//! `DaylightDetectorBlock.updateSignalStrength` reads
//!
//! ```text
//!   environmentAttributes().getValue(SUN_ANGLE, pos)
//! ```
//!
//! and `Level.updateSkyBrightness` — the whole method, offsets 0-25 — is
//!
//! ```text
//!   this.skyDarken = (int)(15.0f - environmentAttributes()
//!       .getDimensionValue(SKY_LIGHT_LEVEL));
//! ```
//!
//! Neither names the time. Both resolve through `EnvironmentAttributeSystem`,
//! whose values come from **keyframe tracks** on `minecraft:timeline`
//! registry entries — a datapack registry. So the sun's angle is eighteen
//! tracks in `data/minecraft/timeline/day.json`, sampled through a cubic
//! Bezier easing curve, and the curve's control points are
//! `(0.362, 0.241, 0.638, 0.759)`. Nothing about that is derivable from the
//! bytecode, and nothing about it resembles the old formula closely enough to
//! substitute: measured against the extract, the old trigonometric sun angle
//! and this one disagree by enough to move a daylight detector's `power` by a
//! whole level over most of the morning.
//!
//! The data is therefore **extracted**, by `ci/dump-timelines.sh`, into
//! `data/vanilla/timelines.json` — the same category as `flat_preset.json`
//! and `block_props.json`. What is transcribed here from
//! `javap -p -c -constants` on the pinned jar is only the *machinery* that
//! samples it, and every piece of it carries its offsets below.
//!
//! # What is ported, with the classes it came from
//!
//! * [`Easing`] — `net.minecraft.util.EasingType`. `CONSTANT`, `LINEAR`, and
//!   `CubicBezier` including its Newton-Raphson solve (4 iterations, step
//!   clamped to ±0.25, tolerance 1e-5) and its bisection fallback.
//! * [`KeyframeSampler`] — `net.minecraft.util.KeyframeTrackSampler`,
//!   including `bakeSegments`'s two wrap segments and `loopTicks`'s
//!   `Math.floorMod`.
//! * [`SkyModel::sun_angle_degrees`] and [`SkyModel::sky_darken`] — the two
//!   attributes this server reads today, resolved the way
//!   `EnvironmentAttributeSystem$ValueSampler.computeValueNotPositional`
//!   resolves any of them: a base value, then each layer in order.
//! * [`mth_cos`] — `net.minecraft.util.Mth.cos(double)`, which is **not**
//!   `Math.cos` but a 65,536-entry lookup table. It is re-exported from
//!   [`rustmc_worldgen::math`] rather than transcribed again: that table is
//!   already proved entry-for-entry against the JVM, and a second copy would
//!   be a second claim about the same floats with one golden between them.
//!
//! # What is NOT ported, said out loud
//!
//! * **The positional half.** `SUN_ANGLE` is registered
//!   `spatiallyInterpolated()`, so vanilla's `getValue(attr, pos)` blends
//!   between the attribute maps of whatever dimensions are near `pos`
//!   through a `SpatialAttributeInterpolator`. This server has one dimension
//!   per level with one attribute map, and
//!   `ValueSampler.computeValuePositional` differs from the non-positional
//!   path only in the `Positional` layers it visits — of which a
//!   single-dimension level has none. So the two agree **here** by having
//!   nothing to interpolate, not by approximation. [`SkyModel::sun_angle_degrees`]
//!   takes no position for that reason; the day a second attribute map can
//!   overlap a level, it grows one and this paragraph is the reason it did.
//! * **The other sixteen tracks** on `minecraft:day`, and the `moon`,
//!   `early_game` and `villager_schedule` timelines. They are carried in the
//!   extract and sampled by nothing. That is a missing consumer, not an
//!   approximation: no code here reads a value those tracks would have moved.
//! * **`ClockManager`.** `AttributeTrackSampler.applyTimeBased` samples at
//!   `clockManager.getTotalTicks(clock)`, and every track this server reads
//!   is on the `minecraft:overworld` clock, which is the level's day time.
//!   A timeline on any other clock is refused by [`SkyModel::build`] rather
//!   than sampled against the wrong counter.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::OnceLock;

/// `Mth.cos(double)` — **not** `f64::cos`.
///
/// It is a 65,536-entry lookup table, and `updateSignalStrength` rounds
/// `15 * cos(a)` to an integer, which puts fifteen decision boundaries inside
/// the range it is called over. A platform cosine in its place moves the
/// rounded result at those boundaries.
///
/// This is [`rustmc_worldgen::math::cos`] and deliberately not a second copy.
/// That table is already proved entry-for-entry against the JVM by
/// `golden_mth_sin`, and a transcription here would be a second claim about
/// the same 65,536 floats with only one of them under a golden — which is how
/// two ports of one table drift. Re-exported rather than merely called so a
/// reader of this module can see which cosine the sky uses.
pub use rustmc_worldgen::math::{cos as mth_cos, sin as mth_sin};

/// `EasingType$CubicBezier$CubicCurve` — a cubic through the origin, stored
/// as its three polynomial coefficients rather than as control points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CubicCurve {
    a: f32,
    b: f32,
    c: f32,
}

impl CubicCurve {
    /// `curveFromControls(a, b)`, offsets 0-31: `(3a - 3b + 1, -6a + 3b, 3a)`.
    fn from_controls(a: f32, b: f32) -> Self {
        Self {
            a: 3.0 * a - 3.0 * b + 1.0,
            b: -6.0 * a + 3.0 * b,
            c: 3.0 * a,
        }
    }
    /// `sample(t)` — Horner, in the same association the bytecode uses.
    fn sample(self, t: f32) -> f32 {
        ((self.a * t + self.b) * t + self.c) * t
    }
    /// `sampleGradient(t)`.
    fn gradient(self, t: f32) -> f32 {
        (3.0 * self.a * t + 2.0 * self.b) * t + self.c
    }
}

/// `net.minecraft.util.EasingType`, the three of thirty-one that
/// `data/vanilla/timelines.json` actually names.
///
/// The rest are **refused by name** in [`Easing::parse`] rather than
/// defaulted to linear: an unrecognised easing that silently becomes a
/// straight line is a wrong answer that no test would distinguish from a
/// right one, which is the defect class `docs/AGENT-BRIEF.md` opens with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Easing {
    /// Holds `fromValue` for the whole segment.
    Constant,
    /// `apply(x) == x`.
    Linear,
    /// CSS-style: solve `xCurve(t) == x`, return `yCurve(t)`.
    CubicBezier { x: CubicCurve, y: CubicCurve },
}

impl Easing {
    /// Read an easing out of a track's `ease` field.
    ///
    /// # Errors
    /// On any easing this module has not transcribed. See the type docs.
    pub fn parse(v: Option<&serde_json::Value>) -> io::Result<Self> {
        let Some(v) = v else { return Ok(Self::Linear) };
        if let Some(name) = v.as_str() {
            return match name {
                "constant" => Ok(Self::Constant),
                "linear" => Ok(Self::Linear),
                other => Err(io::Error::other(format!(
                    "timelines: easing `{other}` is not ported — see crate::sky"
                ))),
            };
        }
        if let Some(c) = v.get("cubic_bezier").and_then(|c| c.as_array()) {
            if c.len() != 4 {
                return Err(io::Error::other(
                    "timelines: cubic_bezier needs four controls",
                ));
            }
            let f = |i: usize| c[i].as_f64().unwrap_or(f64::NAN) as f32;
            return Ok(Self::CubicBezier {
                x: CubicCurve::from_controls(f(0), f(2)),
                y: CubicCurve::from_controls(f(1), f(3)),
            });
        }
        Err(io::Error::other(format!(
            "timelines: unported easing shape {v}"
        )))
    }

    /// `EasingType.apply(float)`.
    #[must_use]
    pub fn apply(self, x: f32) -> f32 {
        match self {
            // `CONSTANT` never reaches here — `sample` short-circuits on a
            // zero-width segment and a constant segment returns `fromValue`
            // from the branch above the lerp. Answering `0.0` is what
            // vanilla's own `CONSTANT.apply` does.
            Self::Constant => 0.0,
            Self::Linear => x,
            Self::CubicBezier { x: xc, y: yc } => yc.sample(Self::solve_t(xc, x)),
        }
    }

    /// `CubicBezier.solveT` — four Newton-Raphson steps, then bisection.
    ///
    /// The step clamp (`±0.25`) and the two `1e-5` tolerances are the
    /// constants `NEWTON_RAPHSON_ITERATIONS`, `MAX_STEP` and the two `ldc`s
    /// at offsets 26 and 46.
    fn solve_t(xc: CubicCurve, x: f32) -> f32 {
        let mut t = x;
        let mut fell_through = true;
        for _ in 0..4 {
            let d = xc.sample(t) - x;
            if d.abs() < 1e-5 {
                return t;
            }
            let g = xc.gradient(t);
            if g < 1e-5 {
                break;
            }
            t -= (d / g).clamp(-0.25, 0.25);
        }
        // `solveTBisect(x, t)`: the Newton guess seeds the search and the
        // bracket is the whole unit interval, exactly as at offsets 0-8.
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        let mut tt = t;
        while lo < hi {
            let d = xc.sample(tt) - x;
            if d.abs() < 1e-5 {
                return tt;
            }
            if d < 0.0 {
                lo = tt;
            } else {
                hi = tt;
            }
            let next = (hi + lo) / 2.0;
            // Vanilla's loop terminates because `lo` and `hi` converge to
            // adjacent floats and `lo < hi` goes false. Ours does the same;
            // the explicit break is belt and braces against a NaN input,
            // which would otherwise spin forever rather than answer badly.
            if next == tt {
                break;
            }
            tt = next;
            fell_through = false;
        }
        let _ = fell_through;
        tt
    }
}

/// One `KeyframeTrackSampler$Segment`.
#[derive(Clone, Copy, Debug)]
struct Segment {
    easing: Easing,
    from_value: f32,
    from_ticks: i32,
    to_value: f32,
    to_ticks: i32,
}

/// `net.minecraft.util.KeyframeTrackSampler` over a float track.
///
/// Only the float instantiation is ported. The colour and boolean tracks in
/// the extract use `ColorModifier`/`BooleanModifier` and a different
/// `LerpFunction`, and nothing here reads them; a shared generic that had
/// never been exercised on either would be a second unfalsifiable claim.
#[derive(Clone, Debug)]
pub struct KeyframeSampler {
    period_ticks: Option<i32>,
    segments: Vec<Segment>,
}

impl KeyframeSampler {
    /// `KeyframeTrack.bakeSampler` → `bakeSegments`.
    ///
    /// # Errors
    /// On an empty keyframe list, which `KeyframeTrack.validateKeyframes`
    /// rejects at codec time and which would otherwise leave no segment for
    /// `getSegmentAt`'s `getLast()` to return.
    pub fn bake(
        keyframes: &[(i32, f32)],
        easing: Easing,
        period_ticks: Option<i32>,
    ) -> io::Result<Self> {
        if keyframes.is_empty() {
            return Err(io::Error::other("timelines: a track with no keyframes"));
        }
        // `bakeSegments` offsets 5-45: one keyframe is a CONSTANT segment of
        // zero width, and the period is not consulted at all.
        if keyframes.len() == 1 {
            let v = keyframes[0].1;
            return Ok(Self {
                period_ticks,
                segments: vec![Segment {
                    easing: Easing::Constant,
                    from_value: v,
                    from_ticks: 0,
                    to_value: v,
                    to_ticks: 0,
                }],
            });
        }
        let first = keyframes[0];
        let last = keyframes[keyframes.len() - 1];
        let mut segments = Vec::with_capacity(keyframes.len() + 1);
        // The leading wrap segment, offsets 83-122: last -> first, with the
        // *from* pushed a whole period into the past.
        if let Some(p) = period_ticks {
            segments.push(Segment {
                easing,
                from_value: last.1,
                from_ticks: last.0 - p,
                to_value: first.1,
                to_ticks: first.0,
            });
        }
        for w in keyframes.windows(2) {
            segments.push(Segment {
                easing,
                from_value: w[0].1,
                from_ticks: w[0].0,
                to_value: w[1].1,
                to_ticks: w[1].0,
            });
        }
        // The trailing wrap segment, offsets 129-168.
        if let Some(p) = period_ticks {
            segments.push(Segment {
                easing,
                from_value: last.1,
                from_ticks: last.0,
                to_value: first.1,
                to_ticks: first.0 + p,
            });
        }
        Ok(Self {
            period_ticks,
            segments,
        })
    }

    /// `loopTicks(long)` — `Math.floorMod`, which is not `%` for negatives.
    fn loop_ticks(&self, ticks: i64) -> i64 {
        match self.period_ticks {
            Some(p) => ticks.rem_euclid(i64::from(p)),
            None => ticks,
        }
    }

    /// `getSegmentAt(long)` — the first segment whose `toTicks` is past
    /// `ticks`, else the last one.
    fn segment_at(&self, ticks: i64) -> Segment {
        for s in &self.segments {
            if ticks < i64::from(s.to_ticks) {
                return *s;
            }
        }
        self.segments[self.segments.len() - 1]
    }

    /// `sample(long)`, offsets 0-106, short-circuits and all.
    #[must_use]
    pub fn sample(&self, ticks: i64) -> f32 {
        let t = self.loop_ticks(ticks);
        let s = self.segment_at(t);
        if t <= i64::from(s.from_ticks) {
            return s.from_value;
        }
        if t >= i64::from(s.to_ticks) {
            return s.to_value;
        }
        let progress = (t - i64::from(s.from_ticks)) as f32 / (s.to_ticks - s.from_ticks) as f32;
        let eased = s.easing.apply(progress);
        // `LerpFunction` for a float attribute is `Mth.lerp(delta, from, to)`.
        s.from_value + eased * (s.to_value - s.from_value)
    }
}

/// One `AttributeTrack`'s modifier — `net.minecraft.world.attribute.modifier.FloatModifier`.
///
/// `Override` is the shape a track with no `modifier` key takes: the sampled
/// argument *is* the value. The five named operations are the ones
/// `FloatModifier` registers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FloatModifier {
    Override,
    Add,
    Subtract,
    Multiply,
    Minimum,
    Maximum,
}

impl FloatModifier {
    fn parse(name: Option<&str>) -> io::Result<Self> {
        Ok(match name {
            None => Self::Override,
            Some("add") => Self::Add,
            Some("subtract") => Self::Subtract,
            Some("multiply") => Self::Multiply,
            Some("minimum") => Self::Minimum,
            Some("maximum") => Self::Maximum,
            Some(other) => {
                return Err(io::Error::other(format!(
                    "timelines: float modifier `{other}` is not ported — see crate::sky"
                )))
            }
        })
    }

    /// `AttributeModifier.apply(base, argument)`.
    #[must_use]
    pub fn apply(self, base: f32, arg: f32) -> f32 {
        match self {
            Self::Override => arg,
            Self::Add => base + arg,
            Self::Subtract => base - arg,
            Self::Multiply => base * arg,
            Self::Minimum => base.min(arg),
            Self::Maximum => base.max(arg),
        }
    }
}

/// A float attribute's layer stack for one dimension: a base value and the
/// time-based tracks that modify it, in registry order.
#[derive(Clone, Debug)]
pub struct FloatAttribute {
    base: f32,
    layers: Vec<(FloatModifier, KeyframeSampler)>,
    /// `EnvironmentAttribute.sanitizeValue` clamps to the type's range.
    range: (f32, f32),
}

impl FloatAttribute {
    /// `ValueSampler.computeValueNotPositional` — base, then every layer in
    /// order, then `sanitizeValue`.
    #[must_use]
    pub fn value_at(&self, ticks: i64) -> f32 {
        let mut v = self.base;
        for (m, s) in &self.layers {
            v = m.apply(v, s.sample(ticks));
        }
        v.clamp(self.range.0, self.range.1)
    }
}

/// The sky of one dimension, baked from `data/vanilla/timelines.json`.
#[derive(Clone, Debug)]
pub struct SkyModel {
    dimension: String,
    has_skylight: bool,
    min_y: i32,
    height: i32,
    sun_angle: Option<FloatAttribute>,
    sky_light_level: FloatAttribute,
}

/// `EnvironmentAttributes.SKY_LIGHT_LEVEL`, registered at offsets 786-823 as
/// `defaultValue 15.0f` over the range `[0.0, 15.0]`.
const SKY_LIGHT_LEVEL_DEFAULT: f32 = 15.0;
const SKY_LIGHT_LEVEL_RANGE: (f32, f32) = (0.0, 15.0);
/// `EnvironmentAttributes.SUN_ANGLE`, `visual/sun_angle`, an angle in
/// **degrees** — which is why `updateSignalStrength` multiplies it by
/// `0.017453292f` and nothing else in that method mentions a conversion.
const SUN_ANGLE_DEFAULT: f32 = 0.0;
const SUN_ANGLE_RANGE: (f32, f32) = (0.0, 360.0);

/// The registry ids of the two tracks this server reads.
const SUN_ANGLE_ID: &str = "minecraft:visual/sun_angle";
const SKY_LIGHT_LEVEL_ID: &str = "minecraft:gameplay/sky_light_level";
/// Every track this server reads is on this clock; see the module docs.
const OVERWORLD_CLOCK: &str = "minecraft:overworld";

impl SkyModel {
    /// Bake the sky for one dimension out of the extracted registry.
    ///
    /// # Errors
    /// If the extract is missing, is not the shape `ci/dump-timelines.sh`
    /// emits, names a dimension that is not in it, or reaches an easing,
    /// modifier or clock this module has not ported. Every one of those is
    /// reported rather than defaulted.
    pub fn load(path: &Path, dimension: &str) -> io::Result<Self> {
        let text = std::fs::read_to_string(path).map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("{}: {e} — run ci/dump-timelines.sh", path.display()),
            )
        })?;
        let root: serde_json::Value = serde_json::from_str(&text).map_err(io::Error::other)?;
        Self::build(&root, dimension)
    }

    /// The half of [`Self::load`] that does not touch the filesystem, so a
    /// test can drive it from a literal.
    ///
    /// # Errors
    /// As [`Self::load`].
    pub fn build(root: &serde_json::Value, dimension: &str) -> io::Result<Self> {
        let dim = root
            .get("dimension_types")
            .and_then(|d| d.get(dimension))
            .ok_or_else(|| {
                io::Error::other(format!("timelines: no dimension_type `{dimension}`"))
            })?;
        let has_skylight = dim
            .get("has_skylight")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let names: Vec<String> = dim
            .get("timelines")
            .and_then(serde_json::Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();

        // The dimension's own static attribute map is the base value where it
        // carries one; the attribute's registered default otherwise. Neither
        // of the two attributes here appears in any shipped dimension_type's
        // `attributes`, so both fall to their defaults — but reading the map
        // first is what vanilla does and what a datapack would change.
        let statics = dim.get("attributes");
        let base_of = |id: &str, default: f32| -> f32 {
            statics
                .and_then(|s| s.get(id))
                .and_then(serde_json::Value::as_f64)
                .map_or(default, |v| v as f32)
        };

        let mut sun_layers = Vec::new();
        let mut sky_layers = Vec::new();
        for name in &names {
            let tl = root
                .get("timelines")
                .and_then(|t| t.get(name))
                .ok_or_else(|| io::Error::other(format!("timelines: no timeline `{name}`")))?;
            let tracks = match tl.get("tracks").and_then(serde_json::Value::as_object) {
                Some(t) => t,
                None => continue,
            };
            let relevant =
                tracks.contains_key(SUN_ANGLE_ID) || tracks.contains_key(SKY_LIGHT_LEVEL_ID);
            if !relevant {
                continue;
            }
            let clock = tl.get("clock").and_then(serde_json::Value::as_str);
            if clock != Some(OVERWORLD_CLOCK) {
                return Err(io::Error::other(format!(
                    "timelines: `{name}` runs on clock {clock:?}, and crate::sky only \
                     samples {OVERWORLD_CLOCK} — see its module docs"
                )));
            }
            let period = tl
                .get("period_ticks")
                .and_then(serde_json::Value::as_i64)
                .map(|p| p as i32);
            for (id, sink) in [
                (SUN_ANGLE_ID, &mut sun_layers),
                (SKY_LIGHT_LEVEL_ID, &mut sky_layers),
            ] {
                if let Some(track) = tracks.get(id) {
                    sink.push(bake_track(track, period)?);
                }
            }
        }

        Ok(Self {
            dimension: dimension.to_owned(),
            has_skylight,
            min_y: i32::try_from(
                dim.get("min_y")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(-64),
            )
            .unwrap_or(-64),
            height: i32::try_from(
                dim.get("height")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(384),
            )
            .unwrap_or(384),
            sun_angle: if sun_layers.is_empty() {
                None
            } else {
                Some(FloatAttribute {
                    base: base_of(SUN_ANGLE_ID, SUN_ANGLE_DEFAULT),
                    layers: sun_layers,
                    range: SUN_ANGLE_RANGE,
                })
            },
            sky_light_level: FloatAttribute {
                base: base_of(SKY_LIGHT_LEVEL_ID, SKY_LIGHT_LEVEL_DEFAULT),
                layers: sky_layers,
                range: SKY_LIGHT_LEVEL_RANGE,
            },
        })
    }

    /// Which dimension this was baked for.
    #[must_use]
    pub fn dimension(&self) -> &str {
        &self.dimension
    }

    /// `DimensionType.hasSkyLight`.
    #[must_use]
    pub fn has_skylight(&self) -> bool {
        self.has_skylight
    }

    /// One past the top block of the dimension — `min_y + height`.
    ///
    /// Taken from the dimension type in the extract rather than from a
    /// constant beside it, because `crate::flat` already carries its own
    /// `MIN_Y`/`HEIGHT` pair and two hand-written copies of one dimension's
    /// extent is how a column walk quietly stops one block short.
    #[must_use]
    pub fn top_y(&self) -> i32 {
        self.min_y.saturating_add(self.height)
    }

    /// The dimension's lowest block — `min_y`.
    #[must_use]
    pub fn min_y(&self) -> i32 {
        self.min_y
    }

    /// `environmentAttributes().getValue(SUN_ANGLE, pos)`, in **degrees**.
    ///
    /// `None` for a dimension whose timelines carry no `visual/sun_angle`
    /// track — the nether and the end, both of which then take the
    /// attribute's registered default rather than an invented angle. The
    /// caller is told that rather than handed a zero it cannot distinguish
    /// from midday.
    #[must_use]
    pub fn sun_angle_degrees(&self, day_time: i64) -> Option<f32> {
        self.sun_angle.as_ref().map(|a| a.value_at(day_time))
    }

    /// `environmentAttributes().getDimensionValue(SKY_LIGHT_LEVEL)`.
    #[must_use]
    pub fn sky_light_level(&self, day_time: i64) -> f32 {
        self.sky_light_level.value_at(day_time)
    }

    /// `Level.updateSkyBrightness` / `Level.getSkyDarken`, the whole of both:
    /// `(int)(15.0f - getDimensionValue(SKY_LIGHT_LEVEL))`.
    ///
    /// The cast is a `f2i` truncation toward zero, not a round — 15.0 minus
    /// 14.19 is 0, not 1, and that one truncation is the difference between a
    /// daylight detector reading 15 and reading 14 for most of the morning.
    #[must_use]
    pub fn sky_darken(&self, day_time: i64) -> i32 {
        (SKY_LIGHT_LEVEL_DEFAULT - self.sky_light_level(day_time)) as i32
    }
}

/// One `AttributeTrack`: a modifier plus a keyframe track.
fn bake_track(
    track: &serde_json::Value,
    period: Option<i32>,
) -> io::Result<(FloatModifier, KeyframeSampler)> {
    let modifier = FloatModifier::parse(track.get("modifier").and_then(serde_json::Value::as_str))?;
    let easing = Easing::parse(track.get("ease"))?;
    let kfs = track
        .get("keyframes")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| io::Error::other("timelines: a track with no keyframes array"))?;
    let mut out = Vec::with_capacity(kfs.len());
    for k in kfs {
        let ticks = k
            .get("ticks")
            .and_then(serde_json::Value::as_i64)
            .ok_or_else(|| io::Error::other("timelines: a keyframe with no ticks"))?;
        let value = k
            .get("value")
            .and_then(serde_json::Value::as_f64)
            .ok_or_else(|| io::Error::other("timelines: a keyframe with a non-float value"))?;
        out.push((ticks as i32, value as f32));
    }
    Ok((modifier, KeyframeSampler::bake(&out, easing, period)?))
}

/// The extract's own summary, for a caller that wants to say what it loaded
/// without re-reading the file. Ordered so a printout is stable.
#[must_use]
pub fn dimension_names(root: &serde_json::Value) -> BTreeMap<String, bool> {
    root.get("dimension_types")
        .and_then(serde_json::Value::as_object)
        .map(|o| {
            o.iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        v.get("has_skylight")
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(false),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The level's sky, chosen once at boot.
///
/// A global for the same reason [`crate::flat`]'s preset is one: the sky is a
/// property of the level, every consumer is deep inside a per-connection
/// call stack, and threading it through would touch several hundred
/// signatures to say one thing.
static ACTIVE: OnceLock<SkyModel> = OnceLock::new();

/// The initial day time, and whether it advances.
///
/// # Why these are settings and not a game rule table
///
/// Vanilla's `advance_time` is a game rule, and this server has no game rule
/// store to put it in — a real one is its own item. What this needs is
/// narrower and is worth stating rather than growing a half table for: the
/// gameplay corpus runs both servers with `advance_time=false`, and a
/// differential harness whose reference and subject disagree about **what
/// time it is** cannot grade anything the sky touches. Two servers each
/// freezing time "wherever it had got to by the time the console command
/// landed" are not frozen at the same time, and the daylight detector's
/// `power` moves by one for roughly every 200 ticks of that drift.
///
/// So the substrate pins a day time on both sides from one constant, and
/// these are how this side is told. See `ci/t2-gameplay.sh`.
static DAY_TIME_AT_BOOT: AtomicI64 = AtomicI64::new(0);
static ADVANCE_TIME: AtomicBool = AtomicBool::new(true);

/// Load the sky for `dimension` and make it the level's.
///
/// # Errors
/// As [`SkyModel::load`]. A caller that cannot load a sky must say so rather
/// than continue: a daylight detector that silently reads midnight is worse
/// than one that declines, because only one of the two is visible.
pub fn select(path: &Path, dimension: &str) -> io::Result<()> {
    let model = SkyModel::load(path, dimension)?;
    let _ = ACTIVE.set(model);
    Ok(())
}

/// The level's sky, or `None` if none was selected.
#[must_use]
pub fn active() -> Option<&'static SkyModel> {
    ACTIVE.get()
}

/// Serialises the tests that move [`set_day_time`]'s two process-wide
/// atomics. `cargo test` runs `tests` and `ticker_tests` on parallel threads
/// in one process, and one of them freezing the day at 6000 while the other
/// restores `(0, true)` made the detector ticker read a dawn it never set:
/// `ticker_tests::a_detector_is_registered_ticked_on_the_twentieth_and_deregistered`
/// failed that way once in T0 (2026-09-24, `persistence-2` on 3316) with
/// nothing changed in this file.
#[cfg(test)]
pub(crate) static DAY_TIME_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Set the day time this level starts at, and whether it advances.
///
/// `advance` false is vanilla's `advance_time=false`: the clock still ticks —
/// `Level.getGameTime` keeps moving, scheduled ticks still fire — and only
/// the *day* time stops. Conflating the two would stop the world.
pub fn set_day_time(at_boot: i64, advance: bool) {
    DAY_TIME_AT_BOOT.store(at_boot, Ordering::Relaxed);
    ADVANCE_TIME.store(advance, Ordering::Relaxed);
}

/// Bumped on every `/time` change, so each play loop sends its `set_time`
/// at once rather than at its next one-second beat — `ServerClockManager.
/// modifyClock` broadcasts the full sync packet as soon as a clock changes.
static SYNC_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// `ServerClockManager.setTotalTicks` for the overworld clock: make the day
/// time `day` now, whatever the game time is, and keep `advance_time` as it
/// was.
pub fn set_total_ticks(day: i64, game_ticks: i64) {
    let base = if ADVANCE_TIME.load(Ordering::Relaxed) {
        day.wrapping_sub(game_ticks)
    } else {
        day
    };
    DAY_TIME_AT_BOOT.store(base, Ordering::Relaxed);
    SYNC_GENERATION.fetch_add(1, Ordering::Relaxed);
}

/// How many `/time` changes there have been; see [`set_total_ticks`].
#[must_use]
pub fn sync_generation() -> u64 {
    SYNC_GENERATION.load(Ordering::Relaxed)
}

/// The current day time, given the game clock's tick count.
///
/// `Level.dayTime` in vanilla, which `ClockManager.getTotalTicks` answers
/// with for the `minecraft:overworld` clock.
#[must_use]
pub fn day_time(game_ticks: i64) -> i64 {
    let base = DAY_TIME_AT_BOOT.load(Ordering::Relaxed);
    if ADVANCE_TIME.load(Ordering::Relaxed) {
        base.wrapping_add(game_ticks)
    } else {
        base
    }
}

/// Whether the day time advances — `advance_time`.
#[must_use]
pub fn advances() -> bool {
    ADVANCE_TIME.load(Ordering::Relaxed)
}

/// `DaylightDetectorBlock.updateSignalStrength`, offsets 0-148.
///
/// ```text
///   int b = level.getEffectiveSkyBrightness(pos);
///   float a = environmentAttributes.getValue(SUN_ANGLE, pos) * 0.017453292f;
///   if (state.getValue(INVERTED)) b = 15 - b;
///   else if (b > 0) {
///       float f = a < PI ? 0.0f : 6.2831855f;
///       a += (f - a) * 0.2f;
///       b = Math.round(b * Mth.cos(a));
///   }
///   b = Mth.clamp(b, 0, 15);
/// ```
///
/// `effective_sky_brightness` is `getBrightness(SKY, pos) - getSkyDarken()`,
/// which the caller computes because only it knows the position's sky light.
/// It is an `i32` and **may be negative**: `LevelReader.getEffectiveSkyBrightness`
/// does not clamp, and the `b > 0` guard on the non-inverted arm is the only
/// thing that stops a negative reaching the cosine.
///
/// Returns `None` when the level has no sun — the nether and the end — where
/// vanilla would read `SUN_ANGLE`'s registered default of zero. Zero is
/// midday, not "no sun", and a detector in the nether answering as if it were
/// noon is a wrong answer rather than a missing one, so this refuses instead.
/// No dimension this server serves reaches it today.
#[must_use]
pub fn daylight_detector_power(
    sky: &SkyModel,
    day_time: i64,
    effective_sky_brightness: i32,
    inverted: bool,
) -> Option<i32> {
    let angle_degrees = sky.sun_angle_degrees(day_time)?;
    let a = angle_degrees * 0.017_453_292_f32;
    let mut b = effective_sky_brightness;
    if inverted {
        b = 15 - b;
    } else if b > 0 {
        let pull = if a < std::f32::consts::PI {
            0.0
        } else {
            6.283_185_5_f32
        };
        let a = a + (pull - a) * 0.2;
        // `Math.round(float)` is `floor(x + 0.5)`, which is NOT Rust's
        // `f32::round` — the two disagree on exact halves of negative
        // numbers, and `15 * cos` is negative for half the day.
        let scaled = b as f32 * mth_cos(f64::from(a));
        b = (scaled + 0.5).floor() as i32;
    }
    Some(b.clamp(0, 15))
}

#[cfg(test)]
mod tests {
    use super::{day_time, daylight_detector_power, set_day_time, SkyModel};

    fn overworld() -> Option<SkyModel> {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/vanilla/timelines.json");
        if !rustmc_data::fixture::available(
            &p,
            "vanilla timelines extract",
            "ci/vanilla-server.sh data",
        ) {
            return None;
        }
        Some(
            SkyModel::load(&p, "minecraft:overworld")
                .expect("timelines.json is present but did not load"),
        )
    }

    /// **The two ticks of the day that tell `Mth.cos` from a platform cosine.**
    ///
    /// Swept over all 24,000 ticks: these two are the only ones where
    /// `round(15 * cos(a))` differs between the 65,536-entry lookup table and
    /// `f64::cos`. Two in 24,000 is 0.008% of the day, so a test that sampled
    /// noon, dawn and dusk would let the whole table be replaced by `cos()`
    /// and stay green.
    ///
    /// The values are the **table's**: `libm` answers 13 and 10 here.
    #[test]
    fn the_lookup_table_cosine_is_not_the_platform_cosine() {
        let Some(sky) = overworld() else { return };
        assert_eq!(daylight_detector_power(&sky, 2447, 15, false), Some(12));
        assert_eq!(daylight_detector_power(&sky, 10_625, 15, false), Some(11));
    }

    /// The corpus superflat, at the substrate's pinned noon: a detector under
    /// open sky reads 15, and inverting it reads 0.
    ///
    /// **This pair is why the corpus check can see the port at all.** Our
    /// unported answer was 0 for both, which agreed with vanilla on the
    /// inverted half and nothing else.
    #[test]
    fn at_noon_an_open_sky_detector_reads_fifteen_and_inverts_to_zero() {
        let Some(sky) = overworld() else { return };
        assert_eq!(daylight_detector_power(&sky, 6000, 15, false), Some(15));
        assert_eq!(daylight_detector_power(&sky, 6000, 15, true), Some(0));
    }

    /// The `b > 0` guard, and why a covered detector is time-independent.
    ///
    /// `getEffectiveSkyBrightness` does not clamp, so a covered position in
    /// full day gives 0 and a covered position at night gives −11. Neither
    /// reaches the cosine, so the non-inverted answer is 0 and the inverted
    /// answer is `15 - b` — no sun angle anywhere in it.
    #[test]
    fn a_covered_detector_never_consults_the_sun() {
        let Some(sky) = overworld() else { return };
        for t in [0_i64, 2447, 6000, 10_625, 13_000, 18_000] {
            assert_eq!(daylight_detector_power(&sky, t, 0, false), Some(0));
            assert_eq!(daylight_detector_power(&sky, t, 0, true), Some(15));
            assert_eq!(daylight_detector_power(&sky, t, -11, false), Some(0));
        }
    }

    /// **A null, recorded so nobody re-runs it.**
    ///
    /// `Math.round(float)` is `floor(x + 0.5)` and Rust's `f32::round` is
    /// half-away-from-zero; they disagree only on exact halves of negative
    /// numbers. Swept over all 24,000 ticks at every sky brightness 1..=15:
    /// **no `15 * cos(a)` lands on an exact half**, and every negative result
    /// is clamped to 0 by the line after it, which erases the difference even
    /// where one existed. So the distinction is unreachable through this
    /// function and `ci/sky-falsify.sh` records it as a green rather than
    /// pretending a red.
    ///
    /// The `floor(x + 0.5)` form is kept anyway, because it is what the
    /// bytecode does and the next caller may not have the clamp.
    #[test]
    fn the_round_half_rule_is_unreachable_here_and_that_is_deliberate() {
        let Some(sky) = overworld() else { return };
        let mut differed = 0;
        for t in 0..24_000_i64 {
            for b in 1..=15 {
                let vanilla = daylight_detector_power(&sky, t, b, false);
                // The same arithmetic with Rust's rounding.
                let angle = sky.sun_angle_degrees(t).unwrap() * 0.017_453_292_f32;
                let pull = if angle < std::f32::consts::PI {
                    0.0
                } else {
                    6.283_185_5_f32
                };
                let a = angle + (pull - angle) * 0.2;
                #[allow(clippy::cast_possible_truncation)]
                let rust = (b as f32 * super::mth_cos(f64::from(a)))
                    .round()
                    .clamp(0.0, 15.0) as i32;
                if vanilla != Some(rust) {
                    differed += 1;
                }
            }
        }
        assert_eq!(
            differed, 0,
            "f32::round now differs from Math.round somewhere in the day — \
             ci/sky-falsify.sh case 5 has become falsifiable and should be \
             turned back into a red expectation"
        );
    }

    /// `advance_time=false` stops the day and not the world.
    #[test]
    fn a_frozen_day_time_ignores_the_game_clock() {
        let _day = super::DAY_TIME_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        set_day_time(6000, false);
        assert_eq!(day_time(0), 6000);
        assert_eq!(day_time(15_600), 6000);
        set_day_time(6000, true);
        assert_eq!(day_time(15_600), 21_600);
        // Restore the default so test order cannot matter.
        set_day_time(0, true);
    }
}

/// Every position holding a daylight detector — vanilla's
/// `Level.blockEntityTickers`, for the one block entity this server ticks.
///
/// `DaylightDetectorBlock.getTicker` hands the level a ticker for each
/// detector, and only when `dimensionType().hasSkyLight()`. A `BTreeSet` and
/// not a `HashSet` so the sweep order is the same on every run: the order is
/// unobservable today — one detector cannot see another's write inside one
/// tick, because both land in the same coalesced flush — but an unordered
/// sweep is the kind of thing that becomes observable later and is then very
/// hard to attribute.
static DETECTORS: std::sync::Mutex<std::collections::BTreeSet<(i32, i32, i32)>> =
    std::sync::Mutex::new(std::collections::BTreeSet::new());

/// `minecraft:daylight_detector`.
const DAYLIGHT_DETECTOR: &str = "minecraft:daylight_detector";

/// Whether a state is the one block this module ticks.
///
/// Public because the click path has to ask *before* reaching for
/// [`crate::light::sky_light`], which walks the whole column above a
/// position — 380 blocks in the overworld — and would otherwise be paid by
/// every lever flip and door open in the game.
#[must_use]
pub fn is_daylight_detector(state: &str) -> bool {
    state.split('[').next() == Some(DAYLIGHT_DETECTOR)
}

/// Keep the ticker list in step with a write. Called from
/// [`crate::shapes::set_block`], which is the one funnel every write goes
/// through.
pub fn note_block(pos: (i32, i32, i32), state: &str) {
    let is_detector = is_daylight_detector(state);
    let Ok(mut set) = DETECTORS.lock() else {
        return;
    };
    if is_detector {
        set.insert(pos);
    } else {
        set.remove(&pos);
    }
}

/// How many detectors are being ticked. For tests and for a caller that wants
/// to say the list is empty rather than assume it.
#[must_use]
pub fn detector_count() -> usize {
    DETECTORS.lock().map_or(0, |s| s.len())
}

/// `DaylightDetectorBlock.tickEntity` for every detector in the level.
///
/// ```text
///   if (level.getGameTime() % 20L == 0L) updateSignalStrength(state, level, pos);
/// ```
///
/// The `% 20` is on the **game time**, not on a per-detector countdown, so
/// every detector in the world updates on the same tick and a detector placed
/// at game time 100 first updates at 120 rather than 20 ticks after it was
/// placed. Getting that wrong would be invisible in a one-detector world and
/// wrong in every other.
///
/// Returns the writes to make: `(pos, next_state)`, each with vanilla's own
/// flag 3 from `updateSignalStrength`'s `setBlock` call. Returning them
/// rather than performing them keeps this module free of the write path, and
/// keeps `crate::shapes::set_block`'s re-entry into [`note_block`] out of a
/// lock this function holds.
///
/// `state_at` and `sky_light` are closures for the same reason
/// [`crate::light::sky_light`]'s is.
#[must_use]
pub fn tick_detectors(
    game_time: i64,
    state_at: &dyn Fn((i32, i32, i32)) -> String,
    sky_light: &dyn Fn((i32, i32, i32)) -> Option<i32>,
) -> Vec<((i32, i32, i32), String)> {
    if game_time % 20 != 0 {
        return Vec::new();
    }
    let Some(sky) = active() else {
        return Vec::new();
    };
    // `getTicker` returns null outright when the dimension has no sky light,
    // so the nether's detectors are not ticked at all — they are not ticked
    // *and then found to be unchanged*, which is the same result today and a
    // different one the moment anything counts ticker work.
    if !sky.has_skylight() {
        return Vec::new();
    }
    let day = day_time(game_time);
    let positions: Vec<_> = {
        let Ok(set) = DETECTORS.lock() else {
            return Vec::new();
        };
        set.iter().copied().collect()
    };
    let mut writes = Vec::new();
    for pos in positions {
        let state = state_at(pos);
        if !is_daylight_detector(&state) {
            continue;
        }
        let Some(brightness) = sky_light(pos) else {
            // No provable sky light here: `crate::light` refused, so this
            // detector is left exactly as it is. See that module for why a
            // refusal and not a zero.
            continue;
        };
        if let Some(next) = signal_strength_write(&state, day, Some(brightness)) {
            writes.push((pos, next));
        }
    }
    writes
}

/// `updateSignalStrength`'s decision for one detector: the state to write, or
/// `None` for "write nothing".
///
/// `None` covers all three of vanilla's ways of writing nothing and one of
/// this server's, and they are not the same thing:
///
/// * `if (state.getValue(POWER) != b)` was false — the strength has not
///   moved. This is the common case and is what stops a detector emitting a
///   packet every twenty ticks for ever;
/// * the level has no sun, so there is no `SUN_ANGLE` to read (see
///   [`daylight_detector_power`]);
/// * the state is not a detector at all, or carries no `power` property;
/// * **`sky_light` is `None`** — `crate::light` could not prove the sky light
///   at this position and this server declines rather than inventing one.
///   That fourth case is ours and not vanilla's, and it is the one to look at
///   first when a detector does not move.
#[must_use]
pub fn signal_strength_write(state: &str, day_time: i64, sky_light: Option<i32>) -> Option<String> {
    if !is_daylight_detector(state) {
        return None;
    }
    let sky = active()?;
    let brightness = sky_light?;
    let inverted = crate::placement::property_of(state, "inverted") == Some("true");
    let power = daylight_detector_power(
        sky,
        day_time,
        brightness - sky.sky_darken(day_time),
        inverted,
    )?;
    let now = crate::placement::property_of(state, "power").and_then(|p| p.parse::<i32>().ok())?;
    if now == power {
        return None;
    }
    Some(crate::placement::with_property(
        state,
        "power",
        &power.to_string(),
    ))
}

/// `updateSignalStrength`'s own `Level.setBlock` flag — `iconst_3` at offset 143.
///
/// `UPDATE_NEIGHBORS | UPDATE_CLIENTS`, and **not** the flag 2 the `inverted`
/// cycle above it writes with. The two are a byte apart and only one of them
/// tells the neighbours: that is why `DaylightDetectorBlock.useWithoutItem`
/// can afford flag 2, because this line does the neighbour half.
pub const UPDATE_SIGNAL_STRENGTH_FLAGS: i32 = 3;

#[cfg(test)]
mod ticker_tests {
    use super::{
        active, detector_count, note_block, set_day_time, signal_strength_write, tick_detectors,
    };

    const DET: &str = "minecraft:daylight_detector[inverted=false,power=0]";
    const LIT: &str = "minecraft:daylight_detector[inverted=false,power=15]";

    fn ready() -> bool {
        if active().is_some() {
            return true;
        }
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/vanilla/timelines.json");
        if !rustmc_data::fixture::available(
            &p,
            "vanilla timelines extract",
            "ci/vanilla-server.sh data",
        ) {
            return false;
        }
        super::select(&p, "minecraft:overworld")
            .expect("timelines.json is present but did not load");
        true
    }

    /// **The test the missing `crate::light::select` call would have failed.**
    ///
    /// A detector reaches the ticker list through [`note_block`], is ticked on
    /// a game time divisible by 20, and is left alone on the other nineteen.
    /// The whole chain is exercised with closures so the failure names the
    /// step rather than the world.
    #[test]
    fn a_detector_is_registered_ticked_on_the_twentieth_and_deregistered() {
        if !ready() {
            return;
        }
        let _day = super::DAY_TIME_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        set_day_time(6000, false);
        let pos = (7, 0, 7);
        note_block(pos, DET);
        assert!(detector_count() >= 1);

        let state_at = |p: (i32, i32, i32)| {
            if p == pos {
                DET.to_owned()
            } else {
                "minecraft:air".to_owned()
            }
        };
        let open_sky = |_p: (i32, i32, i32)| Some(15);

        // Nineteen ticks in twenty do nothing at all — the modulo is on the
        // game time, not on a per-detector countdown.
        for t in [1_i64, 19, 21, 39] {
            assert!(
                tick_detectors(t, &state_at, &open_sky).is_empty(),
                "tick {t}"
            );
        }
        let writes = tick_detectors(20, &state_at, &open_sky);
        assert_eq!(writes, vec![(pos, LIT.to_owned())]);

        // A position whose sky light cannot be proved is left alone, not
        // zeroed. See `crate::light`.
        let refused = |_p: (i32, i32, i32)| None;
        assert!(tick_detectors(20, &state_at, &refused).is_empty());

        // Once it is lit, the ticker writes nothing more — vanilla's
        // `if (state.getValue(POWER) != b)`.
        let lit_at = |p: (i32, i32, i32)| {
            if p == pos {
                LIT.to_owned()
            } else {
                "minecraft:air".to_owned()
            }
        };
        assert!(tick_detectors(20, &lit_at, &open_sky).is_empty());

        // Breaking it takes it off the list.
        note_block(pos, "minecraft:air");
        assert!(tick_detectors(20, &state_at, &open_sky).is_empty());
        set_day_time(0, true);
    }

    /// `signal_strength_write` refuses four different ways and only one of
    /// them is "nothing changed".
    #[test]
    fn the_four_ways_a_detector_writes_nothing() {
        if !ready() {
            return;
        }
        // Not a detector at all.
        assert_eq!(
            signal_strength_write("minecraft:stone", 6000, Some(15)),
            None
        );
        // No provable sky light — ours, not vanilla's.
        assert_eq!(signal_strength_write(DET, 6000, None), None);
        // Already right.
        assert_eq!(signal_strength_write(LIT, 6000, Some(15)), None);
        // And the one that does write.
        assert_eq!(
            signal_strength_write(DET, 6000, Some(15)),
            Some(LIT.to_owned())
        );
    }
}
