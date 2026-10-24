//! Value providers — the datapack's way of saying "a number, but random".
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `net.minecraft.util.valueproviders.{FloatProvider, ConstantFloat,
//! UniformFloat, TrapezoidFloat, ClampedNormalFloat}`,
//! `net.minecraft.world.level.levelgen.heightproviders.{HeightProvider,
//! ConstantHeight, UniformHeight}`, `net.minecraft.util.Mth#randomBetween`.
//!
//! Carvers reach for these constantly — a tunnel's thickness, a room's
//! y scale, a cave's floor level are all providers sampled from the carver's
//! own stream. That is the important part: **every sample is a draw**, so the
//! provider's *kind* determines how many numbers come out of the stream and
//! therefore what every later carver in the same chunk does.
//!
//! Three consequences worth stating plainly, because each is a way to write
//! something that looks right and generates a different world:
//!
//! - [`FloatProvider::Constant`] draws **nothing**. Swapping a constant for a
//!   `uniform` with equal bounds is not a refactor; it consumes a draw.
//! - [`FloatProvider::Trapezoid`] draws **twice**, not once. It is the sum of
//!   two uniforms, which is what gives it the flat-topped distribution.
//! - [`FloatProvider::ClampedNormal`] draws a **gaussian**, whose cost is not
//!   fixed: `nextGaussian` is Marsaglia polar, so it takes two `nextDouble`s
//!   per attempt and retries until the point lands inside the unit circle —
//!   about 21% of attempts fail. It also caches the second of the pair, so the
//!   *next* gaussian on the same stream draws nothing at all. Counting it as
//!   "one draw" is wrong in both directions.
//! - [`HeightProvider::Uniform`] returns early *without drawing* when its
//!   resolved range is empty. A port that always draws shifts every
//!   subsequent value in the chunk.
//!
//! All the arithmetic is `f32`, in vanilla's order.

use crate::rng::RandomSource;
use crate::surface::rule::VerticalAnchor;

/// `net.minecraft.util.valueproviders.FloatProvider`.
///
/// Only the three kinds the overworld and nether carvers actually use. An
/// unknown kind is a parse error rather than a default: silently treating an
/// unrecognised provider as a constant would consume the wrong number of
/// draws and desynchronise everything after it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FloatProvider {
    /// A bare number in the JSON. Draws nothing.
    Constant(f32),
    /// `min_inclusive` to `max_exclusive`, one draw.
    Uniform { min: f32, max_exclusive: f32 },
    /// `min`/`max`/`plateau` — the sum of two uniforms, two draws.
    Trapezoid { min: f32, max: f32, plateau: f32 },
    /// `mean`/`deviation` clamped to `min`/`max` — one *gaussian*, whose
    /// draw count varies. See the module docs.
    ClampedNormal {
        mean: f32,
        deviation: f32,
        min: f32,
        max: f32,
    },
}

impl FloatProvider {
    /// `FloatProvider#sample`.
    pub fn sample<R: RandomSource + ?Sized>(self, random: &mut R) -> f32 {
        match self {
            Self::Constant(value) => value,
            // `Mth.randomBetween`: `nextFloat() * (max - min) + min`. Note
            // this is *not* guarded by the `min >= max` check that
            // `Mth.nextFloat` has — `UniformFloat` validates its bounds at
            // construction instead, and still draws.
            Self::Uniform { min, max_exclusive } => random.next_f32() * (max_exclusive - min) + min,
            Self::Trapezoid { min, max, plateau } => {
                let range = max - min;
                let plateau_start = (range - plateau) / 2.0;
                let plateau_end = range - plateau_start;
                // Two draws, and the association is left to right:
                // `(min + a*plateauEnd) + b*plateauStart`.
                let first = random.next_f32();
                let second = random.next_f32();
                min + first * plateau_end + second * plateau_start
            }
            // `ClampedNormalFloat.sample` = `Mth.clamp(Mth.normal(...), min,
            // max)`, and `Mth.clamp(float)` is `value < min ? min :
            // Math.min(value, max)` — not a two-sided `max(min, min(max, v))`.
            // They differ on NaN, which a gaussian will not produce, and on
            // the order of the comparisons, which is what is copied here.
            Self::ClampedNormal {
                mean,
                deviation,
                min,
                max,
            } => crate::math::clamp_f32(crate::math::normal(random, mean, deviation), min, max),
        }
    }

    /// Parse from datapack JSON.
    ///
    /// A bare number is a constant — the codec is `Codec.either(Codec.FLOAT,
    /// dispatch)`, so `"yScale": 3.0` and `"yScale": {"type": ...}` are both
    /// valid in the same field.
    ///
    /// # Errors
    /// If the value is neither a number nor an object naming a known kind.
    pub fn parse(value: &serde_json::Value) -> Result<Self, String> {
        if let Some(constant) = value.as_f64() {
            #[allow(clippy::cast_possible_truncation)]
            return Ok(Self::Constant(constant as f32));
        }
        let kind = value
            .get("type")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| format!("float provider has no `type`: {value}"))?;
        let field = |name: &str| -> Result<f32, String> {
            value
                .get(name)
                .and_then(serde_json::Value::as_f64)
                .map(|v| {
                    #[allow(clippy::cast_possible_truncation)]
                    {
                        v as f32
                    }
                })
                .ok_or_else(|| format!("float provider {kind} has no numeric `{name}`"))
        };
        match kind {
            "minecraft:constant" => Ok(Self::Constant(field("value")?)),
            "minecraft:uniform" => Ok(Self::Uniform {
                min: field("min_inclusive")?,
                max_exclusive: field("max_exclusive")?,
            }),
            "minecraft:trapezoid" => Ok(Self::Trapezoid {
                min: field("min")?,
                max: field("max")?,
                plateau: field("plateau")?,
            }),
            "minecraft:clamped_normal" => Ok(Self::ClampedNormal {
                mean: field("mean")?,
                deviation: field("deviation")?,
                min: field("min")?,
                max: field("max")?,
            }),
            other => Err(format!("unknown float provider `{other}`")),
        }
    }
}

/// `net.minecraft.world.level.levelgen.heightproviders.HeightProvider`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeightProvider {
    Constant(VerticalAnchor),
    Uniform {
        min_inclusive: VerticalAnchor,
        max_inclusive: VerticalAnchor,
    },
    /// `plateau` defaults to 0, giving a triangle. Two draws unless the
    /// plateau covers the whole range, which degenerates to a uniform's one.
    Trapezoid {
        min_inclusive: VerticalAnchor,
        max_inclusive: VerticalAnchor,
        plateau: i32,
    },
    /// Three nested draws, each narrowing the range for the next, which is
    /// what piles ore near the bottom of its band. `inner` defaults to 1.
    VeryBiasedToBottom {
        min_inclusive: VerticalAnchor,
        max_inclusive: VerticalAnchor,
        inner: i32,
    },
}

impl HeightProvider {
    /// `HeightProvider#sample`.
    ///
    /// `min_y`/`height` resolve the anchors, standing in for vanilla's
    /// `WorldGenerationContext`.
    pub fn sample<R: RandomSource>(self, random: &mut R, min_y: i32, height: i32) -> i32 {
        match self {
            Self::Constant(anchor) => anchor.resolve(min_y, height),
            Self::Uniform {
                min_inclusive,
                max_inclusive,
            } => {
                let min = min_inclusive.resolve(min_y, height);
                let max = max_inclusive.resolve(min_y, height);
                if min > max {
                    // Vanilla logs a warning and returns `min` **without
                    // drawing**. The missing draw is the behaviour; the log
                    // is not.
                    return min;
                }
                // `Mth.randomBetweenInclusive`: `nextInt(max - min + 1) + min`.
                random
                    .next_i32_bound(max.wrapping_sub(min).wrapping_add(1))
                    .wrapping_add(min)
            }

            Self::Trapezoid {
                min_inclusive,
                max_inclusive,
                plateau,
            } => {
                let min = min_inclusive.resolve(min_y, height);
                let max = max_inclusive.resolve(min_y, height);
                if min > max {
                    return min;
                }
                let range = max - min;
                if plateau >= range {
                    return random_between_inclusive(random, min, max);
                }
                let plateau_start = (range - plateau) / 2;
                let plateau_end = range - plateau_start;
                min + random_between_inclusive(random, 0, plateau_end)
                    + random_between_inclusive(random, 0, plateau_start)
            }

            Self::VeryBiasedToBottom {
                min_inclusive,
                max_inclusive,
                inner,
            } => {
                let min = min_inclusive.resolve(min_y, height);
                let max = max_inclusive.resolve(min_y, height);
                // Kept in vanilla's form rather than clippy's equivalent
                // `< 0`: every line here is read against the decompile, and
                // an algebraically-identical rewrite is one more thing to
                // re-derive when checking it.
                #[allow(clippy::int_plus_one)]
                if max - min - inner + 1 <= 0 {
                    return min;
                }
                // `Mth.nextInt`, not `randomBetweenInclusive`: it returns
                // `min` **without drawing** when `min >= max`, so a narrow
                // band here consumes fewer values than a wide one.
                let upper = mth_next_int(random, min + inner, max);
                let biased_upper = mth_next_int(random, min, upper - 1);
                mth_next_int(random, min, biased_upper - 1 + inner)
            }
        }
    }

    /// Parse from datapack JSON.
    ///
    /// # Errors
    /// If the value names no known kind or its anchors do not parse.
    pub fn parse(value: &serde_json::Value) -> Result<Self, String> {
        // A bare anchor object is a constant: `{"above_bottom": 8}`.
        if value.get("type").is_none() {
            return parse_anchor(value).map(Self::Constant);
        }
        let kind = value
            .get("type")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| format!("height provider `type` is not a string: {value}"))?;
        match kind {
            "minecraft:constant" => value
                .get("value")
                .ok_or_else(|| "constant height has no `value`".to_string())
                .and_then(parse_anchor)
                .map(Self::Constant),
            "minecraft:uniform" | "minecraft:trapezoid" | "minecraft:very_biased_to_bottom" => {
                let anchor = |name: &str| -> Result<VerticalAnchor, String> {
                    parse_anchor(
                        value
                            .get(name)
                            .ok_or_else(|| format!("{kind} height has no `{name}`"))?,
                    )
                };
                let min_inclusive = anchor("min_inclusive")?;
                let max_inclusive = anchor("max_inclusive")?;
                let optional = |name: &str, default: i32| -> i32 {
                    value
                        .get(name)
                        .and_then(serde_json::Value::as_i64)
                        .and_then(|v| i32::try_from(v).ok())
                        .unwrap_or(default)
                };
                Ok(match kind {
                    "minecraft:uniform" => Self::Uniform {
                        min_inclusive,
                        max_inclusive,
                    },
                    // Both defaults come from the codec, not from taste:
                    // `plateau` 0 makes a triangle, `inner` 1 the mildest
                    // bias. Guessing either would change every ore band.
                    "minecraft:trapezoid" => Self::Trapezoid {
                        min_inclusive,
                        max_inclusive,
                        plateau: optional("plateau", 0),
                    },
                    _ => Self::VeryBiasedToBottom {
                        min_inclusive,
                        max_inclusive,
                        inner: optional("inner", 1),
                    },
                })
            }
            other => Err(format!("unknown height provider `{other}`")),
        }
    }
}

/// `Mth#randomBetweenInclusive` — always draws.
fn random_between_inclusive<R: RandomSource>(random: &mut R, min: i32, max_inclusive: i32) -> i32 {
    random
        .next_i32_bound(max_inclusive.wrapping_sub(min).wrapping_add(1))
        .wrapping_add(min)
}

/// `Mth#nextInt(random, min, maxInclusive)` — the **guarded** one.
///
/// Returns `min` without drawing when the range is empty or a single value,
/// unlike [`random_between_inclusive`]. `VeryBiasedToBottomHeight` uses this
/// one and `UniformHeight` the other, ten lines apart in the reference.
fn mth_next_int<R: RandomSource>(random: &mut R, min: i32, max_inclusive: i32) -> i32 {
    if min >= max_inclusive {
        return min;
    }
    random_between_inclusive(random, min, max_inclusive)
}

/// `VerticalAnchor` in its datapack form.
///
/// # Errors
/// If the object names none of the three anchor kinds.
pub fn parse_anchor(value: &serde_json::Value) -> Result<VerticalAnchor, String> {
    for (key, build) in [
        ("absolute", VerticalAnchor::Absolute as fn(i32) -> _),
        ("above_bottom", VerticalAnchor::AboveBottom as fn(i32) -> _),
        ("below_top", VerticalAnchor::BelowTop as fn(i32) -> _),
    ] {
        if let Some(v) = value.get(key).and_then(serde_json::Value::as_i64) {
            return i32::try_from(v)
                .map(build)
                .map_err(|_| format!("anchor `{key}` out of range: {v}"));
        }
    }
    Err(format!("not a vertical anchor: {value}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::{LegacyRandomSource, WorldgenRandom};

    fn random(seed: i64) -> WorldgenRandom<LegacyRandomSource> {
        WorldgenRandom::new(LegacyRandomSource::new(seed))
    }

    /// How many draws a sample consumed, which is the part that desynchronises
    /// everything downstream when it is wrong.
    fn draws(provider: FloatProvider) -> u64 {
        let mut r = random(7);
        let before = r.count();
        let _ = provider.sample(&mut r);
        r.count() - before
    }

    #[test]
    fn a_constant_draws_nothing_and_a_uniform_draws_once() {
        // Swapping `3.0` for `{"type":"uniform","min":3,"max":3}` looks
        // equivalent and is not: it eats a draw, and every carver value after
        // it in the chunk changes.
        assert_eq!(draws(FloatProvider::Constant(3.0)), 0);
        assert_eq!(
            draws(FloatProvider::Uniform {
                min: 0.7,
                max_exclusive: 1.4
            }),
            1,
            "nextFloat is a single next(24) call on the legacy source"
        );
    }

    #[test]
    fn a_trapezoid_draws_twice_as_much_as_a_uniform() {
        let uniform = draws(FloatProvider::Uniform {
            min: 0.0,
            max_exclusive: 6.0,
        });
        let trapezoid = draws(FloatProvider::Trapezoid {
            min: 0.0,
            max: 6.0,
            plateau: 2.0,
        });
        assert_eq!(
            trapezoid,
            uniform * 2,
            "the trapezoid is the sum of two uniforms"
        );
    }

    #[test]
    fn a_uniform_stays_inside_its_bounds() {
        let provider = FloatProvider::Uniform {
            min: 0.7,
            max_exclusive: 1.4,
        };
        let mut r = random(20_260_725);
        for _ in 0..4096 {
            let v = provider.sample(&mut r);
            assert!((0.7..1.4).contains(&v), "{v} escaped [0.7, 1.4)");
        }
    }

    #[test]
    fn a_trapezoid_is_flat_topped_not_triangular() {
        // The distribution is the point of the kind: with min 0, max 6,
        // plateau 2, the middle third should be visited about as often
        // across its width, while a triangular one would peak sharply. A
        // crude check, but it fails if the two draws are collapsed into one.
        let provider = FloatProvider::Trapezoid {
            min: 0.0,
            max: 6.0,
            plateau: 2.0,
        };
        let mut r = random(1);
        let mut buckets = [0usize; 6];
        for _ in 0..60_000 {
            let v = provider.sample(&mut r);
            assert!((0.0..=6.0).contains(&v), "{v} escaped [0, 6]");
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let bucket = (v as usize).min(5);
            buckets[bucket] += 1;
        }
        // Buckets 2 and 3 straddle the plateau and should be the two
        // largest, and close to each other.
        let plateau = buckets[2] + buckets[3];
        let edges = buckets[0] + buckets[5];
        assert!(
            plateau > edges * 2,
            "plateau {plateau} vs edges {edges} — this looks uniform, not trapezoidal"
        );
    }

    #[test]
    fn an_empty_height_range_returns_min_without_drawing() {
        // The missing draw is the behaviour. A port that always draws shifts
        // every value after it in the chunk.
        let provider = HeightProvider::Uniform {
            min_inclusive: VerticalAnchor::Absolute(100),
            max_inclusive: VerticalAnchor::Absolute(10),
        };
        let mut r = random(3);
        let before = r.count();
        assert_eq!(provider.sample(&mut r, -64, 384), 100);
        assert_eq!(r.count(), before, "an empty range must not draw");
    }

    #[test]
    fn a_height_range_is_inclusive_at_both_ends() {
        let provider = HeightProvider::Uniform {
            min_inclusive: VerticalAnchor::AboveBottom(0),
            max_inclusive: VerticalAnchor::AboveBottom(2),
        };
        let mut r = random(11);
        let mut seen = [false; 3];
        for _ in 0..512 {
            let y = provider.sample(&mut r, -64, 384);
            assert!((-64..=-62).contains(&y), "y={y} outside the range");
            seen[(y + 64) as usize] = true;
        }
        assert!(seen.iter().all(|s| *s), "an endpoint was never reached");
    }

    #[test]
    fn a_trapezoid_height_defaults_to_a_triangle() {
        // `plateau` is `optionalFieldOf("plateau", 0)`, and 0 means the
        // plateau is empty — a triangle. Guessing any other default would
        // move every ore band that uses one.
        let parsed = HeightProvider::parse(&serde_json::json!({
            "type": "minecraft:trapezoid",
            "min_inclusive": {"absolute": 0},
            "max_inclusive": {"absolute": 64}
        }))
        .expect("parses");
        assert_eq!(
            parsed,
            HeightProvider::Trapezoid {
                min_inclusive: VerticalAnchor::Absolute(0),
                max_inclusive: VerticalAnchor::Absolute(64),
                plateau: 0,
            }
        );
        // Peaked in the middle, which is the whole point of the kind.
        let mut r = random(4);
        let mut middle = 0;
        let mut edges = 0;
        for _ in 0..20_000 {
            let y = parsed.sample(&mut r, -64, 384);
            assert!((0..=64).contains(&y), "y={y} escaped");
            if (24..=40).contains(&y) {
                middle += 1;
            } else if !(8..=56).contains(&y) {
                edges += 1;
            }
        }
        assert!(
            middle > edges,
            "middle {middle} vs edges {edges} — not peaked"
        );
    }

    #[test]
    fn very_biased_to_bottom_uses_the_guarded_draw() {
        // Three nested draws through `Mth.nextInt`, which returns its
        // minimum *without drawing* when the range collapses. So a narrow
        // band consumes fewer values than a wide one — a difference that
        // shifts everything after it in the same feature.
        let wide = HeightProvider::VeryBiasedToBottom {
            min_inclusive: VerticalAnchor::Absolute(0),
            max_inclusive: VerticalAnchor::Absolute(64),
            inner: 1,
        };
        let narrow = HeightProvider::VeryBiasedToBottom {
            min_inclusive: VerticalAnchor::Absolute(0),
            max_inclusive: VerticalAnchor::Absolute(1),
            inner: 1,
        };
        let count = |p: &HeightProvider| {
            let mut r = random(6);
            let before = r.count();
            let _ = p.sample(&mut r, -64, 384);
            r.count() - before
        };
        assert_eq!(count(&wide), 3);
        assert!(
            count(&narrow) < 3,
            "a collapsed range should skip draws, took {}",
            count(&narrow)
        );

        // And it does lean down, hard.
        let mut r = random(8);
        let mut low = 0;
        let mut high = 0;
        for _ in 0..20_000 {
            let y = wide.sample(&mut r, -64, 384);
            assert!((0..=64).contains(&y), "y={y} escaped");
            if y < 16 {
                low += 1;
            } else if y > 48 {
                high += 1;
            }
        }
        assert!(
            low > high * 5,
            "low {low} vs high {high} — not strongly biased"
        );
    }

    #[test]
    fn the_real_height_range_placements_parse() {
        let dir = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../oracle-work/vanilla-26.2/generated-data/data/minecraft/worldgen/placed_feature"
        ));
        if !rustmc_data::fixture::available(dir, "datagen extract", "ci/vanilla-server.sh data") {
            return;
        }
        let mut parsed = 0usize;
        for entry in std::fs::read_dir(dir).expect("readable") {
            let path = entry.expect("entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let json: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).expect("readable"))
                    .expect("json");
            for modifier in json["placement"].as_array().into_iter().flatten() {
                if modifier["type"] == "minecraft:height_range" {
                    HeightProvider::parse(&modifier["height"])
                        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                    parsed += 1;
                }
            }
        }
        assert!(parsed > 80, "only {parsed} height ranges found");
    }

    #[test]
    fn the_cave_carvers_config_parses_as_written() {
        // The real shapes from the generated data, so the parser is tested
        // against what it will actually be handed rather than a fixture.
        let y = HeightProvider::parse(&serde_json::json!({
            "type": "minecraft:uniform",
            "max_inclusive": {"absolute": 180},
            "min_inclusive": {"above_bottom": 8}
        }))
        .expect("cave y");
        assert_eq!(
            y,
            HeightProvider::Uniform {
                min_inclusive: VerticalAnchor::AboveBottom(8),
                max_inclusive: VerticalAnchor::Absolute(180),
            }
        );

        assert_eq!(
            FloatProvider::parse(&serde_json::json!({
                "type": "minecraft:trapezoid", "max": 6.0, "min": 0.0, "plateau": 2.0
            }))
            .expect("canyon thickness"),
            FloatProvider::Trapezoid {
                min: 0.0,
                max: 6.0,
                plateau: 2.0
            }
        );

        // Canyon's `yScale` is a bare number where cave's is a provider.
        assert_eq!(
            FloatProvider::parse(&serde_json::json!(3.0)).expect("canyon yScale"),
            FloatProvider::Constant(3.0)
        );
    }

    #[test]
    fn an_unknown_provider_is_rejected_rather_than_defaulted() {
        // Defaulting to a constant would consume the wrong number of draws
        // and desynchronise every carver after it — a whole-world difference
        // from one unrecognised string.
        assert!(
            FloatProvider::parse(&serde_json::json!({"type": "minecraft:clamped_normal"}))
                .unwrap_err()
                .contains("clamped_normal")
        );
        // Was `minecraft:trapezoid` until trapezoid heights were
        // implemented, at which point it started passing for the wrong
        // reason — a missing `min_inclusive` rather than an unknown kind.
        assert!(
            HeightProvider::parse(&serde_json::json!({"type": "minecraft:biased_to_top"}))
                .unwrap_err()
                .contains("biased_to_top")
        );
    }
}
