//! `IntProvider` — the datapack's way of saying "a whole number, but random".
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `net.minecraft.util.valueproviders.{IntProvider, ConstantInt,
//! UniformInt, BiasedToBottomInt, ClampedInt, ClampedNormalInt,
//! WeightedListInt, TrapezoidInt}`, `net.minecraft.util.random.WeightedList`,
//! `net.minecraft.util.Mth#{randomBetweenInclusive, clamp, normal}`.
//!
//! Placement modifiers are built out of these — how many of a feature to try,
//! how far to scatter it, what height to start at. As with
//! [`crate::carver::providers`], the **draw count** is behaviour: every
//! modifier after one in the same placed feature reads a stream this has
//! already moved.
//!
//! The draw counts vary more here than they look:
//!
//! | kind | draws |
//! |---|---|
//! | `constant` | 0 |
//! | `uniform` | 1 |
//! | `biased_to_bottom` | 2 — nested, `nextInt(nextInt(n) + 1)` |
//! | `trapezoid` | 1 or 2, depending on its own shape |
//! | `clamped` | whatever its inner provider takes |
//! | `weighted_list` | 1 to pick, then the chosen provider's own |
//! | `clamped_normal` | one gaussian, which is two or more |
//!
//! # `clamped_normal` rests on an unproven gaussian
//!
//! It is the only kind here that needs `nextGaussian`, whose Marsaglia polar
//! implementation depends on `Math.log` and has **no golden vectors yet**
//! (see [`crate::rng::gaussian`]). Four placed features use it. It is
//! implemented rather than omitted so the gap is a known one with a test
//! attached, but nothing parity-critical should be built on it until the
//! vectors exist.

use crate::rng::RandomSource;

/// `net.minecraft.util.valueproviders.IntProvider`.
#[derive(Clone, Debug, PartialEq)]
pub enum IntProvider {
    /// A bare number in the JSON, or `{"type": "constant"}`. Draws nothing.
    Constant(i32),
    Uniform {
        min_inclusive: i32,
        max_inclusive: i32,
    },
    /// Two nested draws, which is what biases it: the inner one picks a
    /// smaller range for the outer to land in.
    BiasedToBottom {
        min_inclusive: i32,
        max_inclusive: i32,
    },
    /// Clamps another provider. **Does not change its draw count** — the
    /// inner provider samples first and the clamp is arithmetic.
    Clamped {
        source: Box<IntProvider>,
        min_inclusive: i32,
        max_inclusive: i32,
    },
    ClampedNormal {
        mean: f32,
        deviation: f32,
        min_inclusive: f32,
        max_inclusive: f32,
    },
    /// One draw to pick an entry, then that entry's own draws.
    WeightedList(Vec<(IntProvider, i32)>),
    Trapezoid {
        min_inclusive: i32,
        max_inclusive: i32,
        plateau: i32,
    },
}

impl IntProvider {
    /// `IntProvider#minInclusive` — the smallest value this can produce.
    ///
    /// A clamped provider takes `Math.max` of its own bound and its source's,
    /// which is not the same as its own bound: a clamp of 3 over a source that
    /// starts at 5 has a minimum of 5. `large_dripstone` reads this twice —
    /// once as the low end of the clamp and once as the low end of the draw —
    /// so getting it wrong moves both the shape and the value drawn.
    #[must_use]
    pub fn min_value(&self) -> i32 {
        match self {
            Self::Constant(v) => *v,
            Self::Uniform { min_inclusive, .. }
            | Self::BiasedToBottom { min_inclusive, .. }
            | Self::Trapezoid { min_inclusive, .. } => *min_inclusive,
            Self::Clamped {
                source,
                min_inclusive,
                ..
            } => (*min_inclusive).max(source.min_value()),
            #[expect(
                clippy::cast_possible_truncation,
                reason = "`(int) minInclusive` — the reference narrows here"
            )]
            Self::ClampedNormal { min_inclusive, .. } => *min_inclusive as i32,
            Self::WeightedList(entries) => entries
                .iter()
                .map(|(provider, _)| provider.min_value())
                .min()
                .unwrap_or(0),
        }
    }

    /// `IntProvider#getMaxValue` — the largest value this can produce.
    ///
    /// Not a bound for validation: the geode divides by it, so it is part of
    /// the arithmetic.
    ///
    /// **This said "a clamped provider's maximum is the clamp, not its
    /// source's" until 2026-08-18, and the code did that.** `ClampedInt`
    /// takes `Math.min` of the two, so a clamp of 16 over a source that stops
    /// at 12 has a maximum of 12 and not 16. Nothing in the shipped datapack
    /// distinguishes the two — every clamped provider in it clamps *inside*
    /// its source's range — so the old answer was right by data and wrong by
    /// construction, which is the shape of defect that survives every test
    /// until one datapack changes.
    #[must_use]
    pub fn max_value(&self) -> i32 {
        match self {
            Self::Constant(v) => *v,
            Self::Uniform { max_inclusive, .. }
            | Self::BiasedToBottom { max_inclusive, .. }
            | Self::Trapezoid { max_inclusive, .. } => *max_inclusive,
            Self::Clamped {
                source,
                max_inclusive,
                ..
            } => (*max_inclusive).min(source.max_value()),
            #[expect(
                clippy::cast_possible_truncation,
                reason = "`(int) maxInclusive` — the reference narrows here"
            )]
            Self::ClampedNormal { max_inclusive, .. } => *max_inclusive as i32,
            Self::WeightedList(entries) => entries
                .iter()
                .map(|(provider, _)| provider.max_value())
                .max()
                .unwrap_or(0),
        }
    }

    /// `IntProvider#sample`.
    ///
    /// # Panics
    /// If a weighted list is empty or its weights sum to zero — vanilla
    /// throws there too, and a silent zero would be a feature that never
    /// places rather than an error.
    pub fn sample<R: RandomSource + ?Sized>(&self, random: &mut R) -> i32 {
        match self {
            Self::Constant(value) => *value,

            Self::Uniform {
                min_inclusive,
                max_inclusive,
            } => random_between_inclusive(random, *min_inclusive, *max_inclusive),

            // `min + nextInt(nextInt(max - min + 1) + 1)`. The nesting is the
            // distribution; flattening it both changes the shape and takes
            // one draw instead of two.
            Self::BiasedToBottom {
                min_inclusive,
                max_inclusive,
            } => {
                let span = max_inclusive.wrapping_sub(*min_inclusive).wrapping_add(1);
                let inner = random.next_i32_bound(span).wrapping_add(1);
                min_inclusive.wrapping_add(random.next_i32_bound(inner))
            }

            Self::Clamped {
                source,
                min_inclusive,
                max_inclusive,
            } => source
                .sample(random)
                .max(*min_inclusive)
                .min(*max_inclusive),

            Self::ClampedNormal {
                mean,
                deviation,
                min_inclusive,
                max_inclusive,
            } => {
                // `Mth.normal` — shared with the float provider rather than
                // written out again. It had two copies for one round, and a
                // reference method with two implementations is how the
                // narrowing in it gets fixed in one of them.
                let normal = crate::math::normal(random, *mean, *deviation);
                // `Mth.clamp(float, float, float)` is `v < min ? min :
                // min(v, max)`, which differs from `max(min(..))` for NaN.
                let clamped = crate::math::clamp_f32(normal, *min_inclusive, *max_inclusive);
                // Java's `(int)` truncates toward zero, so -0.7 becomes 0 and
                // not -1. Rust's `as` agrees.
                #[allow(clippy::cast_possible_truncation)]
                {
                    clamped as i32
                }
            }

            Self::WeightedList(entries) => {
                let total: i32 = entries.iter().map(|(_, w)| *w).sum();
                assert!(
                    total > 0,
                    "weighted int provider has no weight; vanilla throws here"
                );
                let mut selection = random.next_i32_bound(total);
                for (provider, weight) in entries {
                    selection -= *weight;
                    if selection < 0 {
                        return provider.sample(random);
                    }
                }
                unreachable!("selection exceeded the total weight")
            }

            // Three branches, and the first two are not special cases of the
            // third: they take a different number of draws.
            Self::Trapezoid {
                min_inclusive,
                max_inclusive,
                plateau,
            } => {
                if *plateau == 0 && *max_inclusive == -*min_inclusive {
                    // The symmetric triangle: two draws, subtracted.
                    let bound = max_inclusive.wrapping_add(1);
                    return random
                        .next_i32_bound(bound)
                        .wrapping_sub(random.next_i32_bound(bound));
                }
                let range = max_inclusive.wrapping_sub(*min_inclusive);
                if *plateau == range {
                    // Degenerate: a plain uniform, one draw.
                    return random_between_inclusive(random, *min_inclusive, *max_inclusive);
                }
                let plateau_start = (range - *plateau) / 2;
                let plateau_end = range - plateau_start;
                min_inclusive
                    .wrapping_add(random_between_inclusive(random, 0, plateau_end))
                    .wrapping_add(random_between_inclusive(random, 0, plateau_start))
            }
        }
    }

    /// Parse from datapack JSON.
    ///
    /// A bare number is a constant, matching the codec's `either`.
    ///
    /// # Errors
    /// If the value names no known kind or a field is missing.
    pub fn parse(value: &serde_json::Value) -> Result<Self, String> {
        if let Some(constant) = value.as_i64() {
            return i32::try_from(constant)
                .map(Self::Constant)
                .map_err(|_| format!("constant int out of range: {constant}"));
        }
        let kind = value
            .get("type")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| format!("int provider has no `type`: {value}"))?;
        let int = |name: &str| -> Result<i32, String> {
            value
                .get(name)
                .and_then(serde_json::Value::as_i64)
                .and_then(|v| i32::try_from(v).ok())
                .ok_or_else(|| format!("int provider {kind} has no integer `{name}`"))
        };
        let float = |name: &str| -> Result<f32, String> {
            value
                .get(name)
                .and_then(serde_json::Value::as_f64)
                .map(|v| {
                    #[allow(clippy::cast_possible_truncation)]
                    {
                        v as f32
                    }
                })
                .ok_or_else(|| format!("int provider {kind} has no numeric `{name}`"))
        };

        match kind {
            "minecraft:constant" => Ok(Self::Constant(int("value")?)),
            "minecraft:uniform" => Ok(Self::Uniform {
                min_inclusive: int("min_inclusive")?,
                max_inclusive: int("max_inclusive")?,
            }),
            "minecraft:biased_to_bottom" => Ok(Self::BiasedToBottom {
                min_inclusive: int("min_inclusive")?,
                max_inclusive: int("max_inclusive")?,
            }),
            "minecraft:clamped" => Ok(Self::Clamped {
                source: Box::new(Self::parse(
                    value
                        .get("source")
                        .ok_or_else(|| "clamped int has no `source`".to_string())?,
                )?),
                min_inclusive: int("min_inclusive")?,
                max_inclusive: int("max_inclusive")?,
            }),
            "minecraft:clamped_normal" => Ok(Self::ClampedNormal {
                mean: float("mean")?,
                deviation: float("deviation")?,
                min_inclusive: float("min_inclusive")?,
                max_inclusive: float("max_inclusive")?,
            }),
            // `TrapezoidInt`'s codec spells these `min`/`max`, unlike every
            // other kind here. Read from the codec, not guessed from the
            // struct field names — the first attempt guessed and the real
            // data rejected it.
            "minecraft:trapezoid" => Ok(Self::Trapezoid {
                min_inclusive: int("min")?,
                max_inclusive: int("max")?,
                plateau: int("plateau")?,
            }),
            "minecraft:weighted_list" => {
                let list = value
                    .get("distribution")
                    .and_then(serde_json::Value::as_array)
                    .ok_or_else(|| "weighted int has no `distribution` array".to_string())?;
                let mut entries = Vec::with_capacity(list.len());
                for item in list {
                    let data = item
                        .get("data")
                        .ok_or_else(|| "weighted entry has no `data`".to_string())?;
                    let weight = item
                        .get("weight")
                        .and_then(serde_json::Value::as_i64)
                        .and_then(|v| i32::try_from(v).ok())
                        .ok_or_else(|| "weighted entry has no integer `weight`".to_string())?;
                    entries.push((Self::parse(data)?, weight));
                }
                Ok(Self::WeightedList(entries))
            }
            other => Err(format!("unknown int provider `{other}`")),
        }
    }
}

/// `Mth#randomBetweenInclusive`.
fn random_between_inclusive<R: RandomSource + ?Sized>(
    random: &mut R,
    min: i32,
    max_inclusive: i32,
) -> i32 {
    random
        .next_i32_bound(max_inclusive.wrapping_sub(min).wrapping_add(1))
        .wrapping_add(min)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::{LegacyRandomSource, WorldgenRandom};

    fn random(seed: i64) -> WorldgenRandom<LegacyRandomSource> {
        WorldgenRandom::new(LegacyRandomSource::new(seed))
    }

    fn draws(provider: &IntProvider) -> u64 {
        let mut r = random(11);
        let before = r.count();
        let _ = provider.sample(&mut r);
        r.count() - before
    }

    /// `ClampedInt` takes `Math.max`/`Math.min` of ITS bound and its
    /// source's, and this code took only its own until 2026-08-18.
    ///
    /// Nothing in the shipped datapack could tell the difference — its one
    /// clamped provider, `large_dripstone`'s `column_radius`, clamps 3..16
    /// inside a source of 3..19, so both answers agree. A defect that no data
    /// distinguishes is not a defect that no data ever will.
    #[test]
    fn a_clamp_is_bounded_by_its_source_as_well_as_by_itself() {
        let parse = |json: &str| IntProvider::parse(&serde_json::from_str(json).unwrap()).unwrap();
        let inside = parse(
            r#"{"type":"minecraft:clamped","min_inclusive":3,"max_inclusive":16,
                "source":{"type":"minecraft:uniform","min_inclusive":3,"max_inclusive":19}}"#,
        );
        assert_eq!(inside.min_value(), 3);
        assert_eq!(inside.max_value(), 16);
        let outside = parse(
            r#"{"type":"minecraft:clamped","min_inclusive":1,"max_inclusive":40,
                "source":{"type":"minecraft:uniform","min_inclusive":5,"max_inclusive":12}}"#,
        );
        assert_eq!(
            outside.min_value(),
            5,
            "the clamp is looser than the source"
        );
        assert_eq!(
            outside.max_value(),
            12,
            "the clamp is looser than the source"
        );
        // A constant reports itself on both sides.
        assert_eq!(parse("7").min_value(), 7);
        assert_eq!(parse("7").max_value(), 7);
    }

    #[test]
    fn the_draw_counts_are_what_the_table_in_the_docs_says() {
        assert_eq!(draws(&IntProvider::Constant(4)), 0);
        assert_eq!(
            draws(&IntProvider::Uniform {
                min_inclusive: 0,
                max_inclusive: 9
            }),
            1
        );
        assert_eq!(
            draws(&IntProvider::BiasedToBottom {
                min_inclusive: 0,
                max_inclusive: 9
            }),
            2
        );
        // A clamp is arithmetic, not a draw.
        assert_eq!(
            draws(&IntProvider::Clamped {
                source: Box::new(IntProvider::Uniform {
                    min_inclusive: 0,
                    max_inclusive: 9
                }),
                min_inclusive: 2,
                max_inclusive: 5
            }),
            1
        );
        // One draw to pick, then the chosen constant draws nothing.
        assert_eq!(
            draws(&IntProvider::WeightedList(vec![
                (IntProvider::Constant(1), 1),
                (IntProvider::Constant(2), 1)
            ])),
            1
        );
    }

    #[test]
    fn a_trapezoid_takes_a_different_number_of_draws_per_shape() {
        // The three branches are not refactorings of each other. The
        // symmetric triangle takes two draws, the degenerate plateau takes
        // one, the general case takes two.
        assert_eq!(
            draws(&IntProvider::Trapezoid {
                min_inclusive: -4,
                max_inclusive: 4,
                plateau: 0
            }),
            2
        );
        assert_eq!(
            draws(&IntProvider::Trapezoid {
                min_inclusive: 0,
                max_inclusive: 8,
                plateau: 8
            }),
            1,
            "plateau == range is a plain uniform"
        );
        assert_eq!(
            draws(&IntProvider::Trapezoid {
                min_inclusive: 0,
                max_inclusive: 8,
                plateau: 2
            }),
            2
        );
    }

    #[test]
    fn the_symmetric_triangle_is_centred_on_zero() {
        // `nextInt(n) - nextInt(n)`, which is why `ofTriangle` can offset a
        // feature in both directions from one provider.
        let provider = IntProvider::Trapezoid {
            min_inclusive: -3,
            max_inclusive: 3,
            plateau: 0,
        };
        let mut r = random(20_260_725);
        let mut sum = 0i64;
        let mut count = 0i64;
        for _ in 0..20_000 {
            let v = provider.sample(&mut r);
            assert!((-3..=3).contains(&v), "{v} escaped [-3, 3]");
            sum += i64::from(v);
            count += 1;
        }
        assert!(
            sum.abs() < count / 20,
            "mean {sum}/{count} is not near zero"
        );
    }

    #[test]
    fn biased_to_bottom_actually_leans_down() {
        // If the nesting were flattened this would be uniform and the
        // assertion would fail — which is the point of testing the shape and
        // not only the range.
        let provider = IntProvider::BiasedToBottom {
            min_inclusive: 0,
            max_inclusive: 15,
        };
        let mut r = random(3);
        let mut low = 0;
        let mut high = 0;
        for _ in 0..20_000 {
            let v = provider.sample(&mut r);
            assert!((0..=15).contains(&v));
            if v < 8 {
                low += 1;
            } else {
                high += 1;
            }
        }
        assert!(low > high * 3, "low {low} vs high {high} — not biased");
    }

    #[test]
    fn a_clamp_bounds_its_source_without_moving_the_stream() {
        let inner = IntProvider::Uniform {
            min_inclusive: 0,
            max_inclusive: 100,
        };
        let clamped = IntProvider::Clamped {
            source: Box::new(inner.clone()),
            min_inclusive: 10,
            max_inclusive: 20,
        };
        let mut a = random(5);
        let mut b = random(5);
        for _ in 0..200 {
            let raw = inner.sample(&mut a);
            let bounded = clamped.sample(&mut b);
            assert_eq!(bounded, raw.clamp(10, 20));
        }
    }

    #[test]
    fn a_weighted_list_respects_its_weights() {
        let provider = IntProvider::WeightedList(vec![
            (IntProvider::Constant(1), 9),
            (IntProvider::Constant(2), 1),
        ]);
        let mut r = random(7);
        let mut ones = 0;
        for _ in 0..10_000 {
            if provider.sample(&mut r) == 1 {
                ones += 1;
            }
        }
        assert!((8_500..9_500).contains(&ones), "got {ones} ones in 10,000");
    }

    #[test]
    fn the_real_placed_features_parse() {
        // Against the generated data, and the point is coverage: if this
        // stops finding every kind the data uses, a placed feature is being
        // read with an unsupported provider somewhere.
        let dir = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../oracle-work/vanilla-26.2/generated-data/data/minecraft/worldgen/placed_feature"
        ));
        if !rustmc_data::fixture::available(dir, "datagen extract", "ci/vanilla-server.sh data") {
            return;
        }
        // Only the fields that really are IntProviders — `height_range` and
        // friends carry HeightProviders, which share type names and are a
        // different vocabulary.
        let mut seen: std::collections::BTreeSet<&'static str> = std::collections::BTreeSet::new();
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
                let kind = modifier["type"].as_str().unwrap_or_default();
                for field in ["count", "xz_spread", "y_spread"] {
                    if let Some(value) = modifier.get(field) {
                        let provider = IntProvider::parse(value)
                            .unwrap_or_else(|e| panic!("{}: {kind}/{field}: {e}", path.display()));
                        seen.insert(match provider {
                            IntProvider::Constant(_) => "constant",
                            IntProvider::Uniform { .. } => "uniform",
                            IntProvider::BiasedToBottom { .. } => "biased_to_bottom",
                            IntProvider::Clamped { .. } => "clamped",
                            IntProvider::ClampedNormal { .. } => "clamped_normal",
                            IntProvider::WeightedList(_) => "weighted_list",
                            IntProvider::Trapezoid { .. } => "trapezoid",
                        });
                        parsed += 1;
                    }
                }
            }
        }
        assert!(
            parsed > 100,
            "only {parsed} int providers found in the data"
        );
        assert!(
            seen.len() >= 3,
            "only {} distinct provider kinds exercised",
            seen.len()
        );
    }

    #[test]
    fn an_unknown_provider_is_rejected_rather_than_defaulted() {
        assert!(
            IntProvider::parse(&serde_json::json!({"type": "minecraft:gaussian"}))
                .unwrap_err()
                .contains("gaussian")
        );
    }
}
