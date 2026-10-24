//! Block state providers — what block a feature places at a position.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `net.minecraft.world.level.levelgen.feature.stateproviders.*`.
//!
//! A feature rarely names one block. It names a *provider*, which may pick
//! from a weighted list, vary with noise, or — as `disk_sand` does — answer
//! differently depending on what is next to the position being filled.
//!
//! # Whether a provider draws is behaviour
//!
//! `simple_state_provider` never touches the stream. `weighted_state_provider`
//! draws once. `rule_based_state_provider` draws only as much as whichever
//! branch it takes. Getting that wrong shifts every feature after it, so the
//! draw count is stated per variant here and asserted in the tests rather
//! than left to be inferred.

use crate::rng::RandomSource;

use crate::tags::BlockTags;

use super::block_predicate::{BlockPredicate, PredicateWorld};

/// `BlockStateProvider`.
/// A built noise field, compared by its inputs rather than its tables.
///
/// `StateProvider` derives `PartialEq`, and two `NormalNoise` values built
/// from the same seed and parameters are equal by construction — comparing
/// their octave tables would be comparing a few thousand doubles to learn
/// something the seed already says. The seed, first octave and amplitudes sit
/// beside this in the variant and are compared normally, so equality stays
/// meaningful rather than being quietly weakened.
#[derive(Clone, Debug)]
pub struct NoiseField(pub std::sync::Arc<crate::noise::normal::NormalNoise>);

impl PartialEq for NoiseField {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum StateProvider {
    /// `simple_state_provider` — one block, no draw.
    Simple(String),
    /// `weighted_state_provider` — one draw, then a walk.
    ///
    /// `nextInt(totalWeight)` and subtract each entry's weight until the
    /// running value goes negative. **One draw whatever the list contains**,
    /// including a single-entry list — a provider that short-circuited there
    /// would place the same block and move every feature after it.
    Weighted(Vec<(String, i32)>),
    /// `randomized_int_state_provider` — a source provider, then one integer
    /// property re-rolled.
    ///
    /// **The source draws first, then the value.** Two draws in that order
    /// for a weighted source, and the property is only set if the state
    /// actually has it — a source that returned a state without the property
    /// still paid for the value.
    RandomizedInt {
        source: Box<StateProvider>,
        property: String,
        values: super::int_provider::IntProvider,
    },
    /// `rotated_block_provider` — one draw, and the config's own axis is
    /// **discarded**.
    ///
    /// `RotatedBlockProvider`'s codec keeps only `BuiltInRegistries.BLOCK`,
    /// so `{"state":{"Name":"minecraft:hay_block","Properties":{"axis":"y"}}}`
    /// contributes the *block* and nothing else. `getState` is
    /// `block.defaultBlockState().trySetValue(RotatedPillarBlock.AXIS,
    /// Direction.Axis.getRandom(random))`, and `Axis.getRandom` is
    /// `VALUES[nextInt(3)]` over `[X, Y, Z]` in that order.
    ///
    /// Storing the *canonical default state* rather than the JSON's state is
    /// therefore not a shortcut, it is the port: a provider that honoured the
    /// written `axis=y` would place a third of its blocks wrong and every
    /// other property of the written state would leak in too.
    Rotated(String),
    /// `rule_based_state_provider` — the first rule whose predicate holds
    /// wins; otherwise the fallback. Draws only what the chosen branch draws,
    /// and the predicates themselves draw nothing.
    /// `rule_based_state_provider`. The fallback is **optional**: with none,
    /// a position no rule matched has no state at all, and what vanilla does
    /// with that depends on which of two methods asked. See [`Self::state`].
    RuleBased {
        fallback: Option<Box<StateProvider>>,
        rules: Vec<(BlockPredicate, StateProvider)>,
    },
    /// `noise_threshold_provider` — a noise field picks the branch, then the
    /// branch draws.
    ///
    /// ```text
    /// n = noise.getValue(x*scale, y*scale, z*scale)
    /// if n < threshold          -> lowStates[nextInt(len)]      1 draw
    /// if nextFloat < highChance -> highStates[nextInt(len)]     2 draws
    /// else                      -> defaultState                 1 draw
    /// ```
    ///
    /// The noise is **not** the world's. `NoiseBasedStateProvider` builds its
    /// own from the config's `seed` through
    /// `new WorldgenRandom(new LegacyRandomSource(seed))` — legacy, not
    /// xoroshiro, and independent of the world seed, so the same block
    /// pattern appears in every world. Seeding it from the world would give a
    /// plausible-looking flower field that is nobody's.
    ///
    /// Draw counts differ per branch and that is the point: the low branch
    /// spends one and the high branch two, so getting the comparison
    /// backwards moves every later feature in the chunk.
    NoiseThreshold {
        noise: NoiseField,
        /// The noise's defining inputs, kept so two providers can still be
        /// compared: the field itself is a pure function of these.
        seed: i64,
        first_octave: i32,
        amplitudes: Vec<f64>,
        scale: f32,
        threshold: f32,
        high_chance: f32,
        default_state: String,
        low_states: Vec<String>,
        high_states: Vec<String>,
    },
}

impl StateProvider {
    /// # Errors
    /// If the provider names no type, a supported type is missing a field, or
    /// the type is one that is not implemented — an unknown provider is an
    /// error rather than a fallback, because a feature that silently places
    /// its fallback block looks like a world, just the wrong one.
    pub fn parse(
        value: &serde_json::Value,
        states: &dyn super::state_spec::DefaultStates,
    ) -> Result<Self, String> {
        let kind = value
            .get("type")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| format!("state provider has no `type`: {value}"))?;
        match kind {
            "minecraft:simple_state_provider" => {
                let spec = value
                    .get("state")
                    .ok_or_else(|| "simple_state_provider has no `state`".to_string())?;
                Ok(Self::Simple(super::state_spec::canonical(spec, states)?))
            }
            // Read for the *block* and then re-defaulted, because the codec
            // keeps only the block. `parse_for_block` is what turns the
            // canonical state back into the block's own default state, which
            // is what `defaultBlockState()` answers.
            "minecraft:rotated_block_provider" => {
                let spec = value
                    .get("state")
                    .ok_or_else(|| "rotated_block_provider has no `state`".to_string())?;
                let canonical = super::state_spec::canonical(spec, states)?;
                let block = canonical
                    .split('[')
                    .next()
                    .unwrap_or(&canonical)
                    .to_string();
                Ok(Self::Rotated(super::state_spec::parse_for_block(
                    &block, states,
                )?))
            }
            "minecraft:weighted_state_provider" => {
                let entries = value
                    .get("entries")
                    .and_then(serde_json::Value::as_array)
                    .ok_or_else(|| "weighted_state_provider has no `entries`".to_string())?
                    .iter()
                    .map(|entry| {
                        let spec = entry
                            .get("data")
                            .ok_or_else(|| "weighted entry has no `data`".to_string())?;
                        let name = super::state_spec::canonical(spec, states)?;
                        // `optionalFieldOf("weight", 1)`.
                        let weight = entry
                            .get("weight")
                            .and_then(serde_json::Value::as_i64)
                            .and_then(|v| i32::try_from(v).ok())
                            .unwrap_or(1);
                        Ok((name, weight))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                if entries.is_empty() {
                    return Err("weighted_state_provider has no entries".to_string());
                }
                Ok(Self::Weighted(entries))
            }
            "minecraft:noise_threshold_provider" => {
                let states_of = |key: &str| -> Result<Vec<String>, String> {
                    value
                        .get(key)
                        .and_then(serde_json::Value::as_array)
                        .ok_or_else(|| format!("noise_threshold_provider has no `{key}`"))?
                        .iter()
                        .map(|spec| super::state_spec::canonical(spec, states))
                        .collect()
                };
                let f32_of = |key: &str| -> Result<f32, String> {
                    value
                        .get(key)
                        .and_then(serde_json::Value::as_f64)
                        .map(|v| v as f32)
                        .ok_or_else(|| format!("noise_threshold_provider has no `{key}`"))
                };
                let seed = value
                    .get("seed")
                    .and_then(serde_json::Value::as_i64)
                    .ok_or_else(|| "noise_threshold_provider has no `seed`".to_string())?;
                let params = value
                    .get("noise")
                    .ok_or_else(|| "noise_threshold_provider has no `noise`".to_string())?;
                let first_octave = i32::try_from(
                    params
                        .get("firstOctave")
                        .and_then(serde_json::Value::as_i64)
                        .ok_or_else(|| "noise has no `firstOctave`".to_string())?,
                )
                .map_err(|e| e.to_string())?;
                let amplitudes: Vec<f64> = params
                    .get("amplitudes")
                    .and_then(serde_json::Value::as_array)
                    .ok_or_else(|| "noise has no `amplitudes`".to_string())?
                    .iter()
                    .filter_map(serde_json::Value::as_f64)
                    .collect();
                // `new WorldgenRandom(new LegacyRandomSource(seed))` — the
                // config's seed, not the world's, and the legacy source.
                // Vanilla wraps this in `new WorldgenRandom(...)`, whose
                // `forkPositional` and draws both delegate straight to the
                // wrapped source, so the bare legacy source is the same
                // stream. `ForkPositional` is implemented on the source here,
                // not on the wrapper.
                let mut random = crate::rng::legacy::LegacyRandomSource::new(seed);
                let noise = crate::noise::normal::NormalNoise::create(
                    &mut random,
                    first_octave,
                    &amplitudes,
                );
                Ok(Self::NoiseThreshold {
                    noise: NoiseField(std::sync::Arc::new(noise)),
                    seed,
                    first_octave,
                    amplitudes,
                    scale: f32_of("scale")?,
                    threshold: f32_of("threshold")?,
                    high_chance: f32_of("high_chance")?,
                    default_state: super::state_spec::canonical(
                        value
                            .get("default_state")
                            .ok_or_else(|| "no `default_state`".to_string())?,
                        states,
                    )?,
                    low_states: states_of("low_states")?,
                    high_states: states_of("high_states")?,
                })
            }
            "minecraft:randomized_int_state_provider" => Ok(Self::RandomizedInt {
                source: Box::new(Self::parse(
                    value.get("source").ok_or_else(|| {
                        "randomized_int_state_provider has no `source`".to_string()
                    })?,
                    states,
                )?),
                property: value
                    .get("property")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| "randomized_int_state_provider has no `property`".to_string())?
                    .to_string(),
                values: super::int_provider::IntProvider::parse(
                    value.get("values").ok_or_else(|| {
                        "randomized_int_state_provider has no `values`".to_string()
                    })?,
                )?,
            }),
            "minecraft:rule_based_state_provider" => {
                let fallback = match value.get("fallback") {
                    Some(v) => Some(Box::new(Self::parse(v, states)?)),
                    None => None,
                };
                let rules = value
                    .get("rules")
                    .and_then(serde_json::Value::as_array)
                    .ok_or_else(|| "rule_based_state_provider has no `rules`".to_string())?
                    .iter()
                    .map(|rule| {
                        let predicate =
                            BlockPredicate::parse(rule.get("if_true").ok_or_else(|| {
                                "state provider rule has no `if_true`".to_string()
                            })?)?;
                        let then = Self::parse(
                            rule.get("then")
                                .ok_or_else(|| "state provider rule has no `then`".to_string())?,
                            states,
                        )?;
                        Ok((predicate, then))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                Ok(Self::RuleBased { fallback, rules })
            }
            other => Err(format!("unimplemented state provider `{other}`")),
        }
    }

    /// Whether every predicate in this provider can be evaluated.
    #[must_use]
    pub fn is_supported(&self) -> bool {
        match self {
            // The noise field is built at parse time and the branch draws
            // are unconditional, so there is nothing left that can fail.
            Self::Simple(_)
            | Self::Weighted(_)
            | Self::NoiseThreshold { .. }
            | Self::Rotated(_) => true,
            Self::RandomizedInt { source, .. } => source.is_supported(),
            Self::RuleBased { fallback, rules } => {
                fallback.as_ref().is_none_or(|f| f.is_supported())
                    && rules
                        .iter()
                        .all(|(p, t)| p.is_supported() && t.is_supported())
            }
        }
    }

    /// `BlockStateProvider#getState`.
    ///
    /// # Errors
    /// If a predicate needs block behaviour the port does not have.
    /// `random` is unused by the two providers implemented here and is not
    /// an oversight: `weighted_state_provider` and `noise_provider` draw, and
    /// a signature that omitted the stream would have to be changed — and
    /// every call site with it — the day one of them lands. Threading it now
    /// keeps that a one-file change.
    #[allow(clippy::only_used_in_recursion, reason = "see the note above")]
    pub fn state<W: PredicateWorld + ?Sized, R: RandomSource + ?Sized>(
        &self,
        world: &mut W,
        tags: &BlockTags,
        random: &mut R,
        pos: (i32, i32, i32),
    ) -> Result<String, super::block_predicate::NeedsBehaviour> {
        match self {
            Self::Simple(name) => Ok(name.clone()),

            // `RotatedBlockProvider#getState`. One `nextInt(3)` into
            // `Direction.Axis.VALUES`, which is declared `[X, Y, Z]` — the
            // order is the whole content of the draw, so it is written out
            // rather than derived from anything that could be re-ordered.
            //
            // `trySetValue` is a *try*: a block with no `axis` keeps its
            // default state and the draw is still spent. That is why the draw
            // happens before the property is looked for.
            Self::Rotated(default_state) => {
                let axis = ["x", "y", "z"][random.next_i32_bound(3) as usize];
                Ok(set_axis(default_state, axis))
            }

            // `NoiseThresholdProvider#getState`. The scale is a float widened
            // to double before it multiplies the coordinates, which is not
            // the same as multiplying in f32.
            Self::NoiseThreshold {
                noise,
                scale,
                seed: _,
                first_octave: _,
                amplitudes: _,
                threshold,
                high_chance,
                default_state,
                low_states,
                high_states,
            } => {
                let scale = f64::from(*scale);
                let n = noise.0.get_value(
                    f64::from(pos.0) * scale,
                    f64::from(pos.1) * scale,
                    f64::from(pos.2) * scale,
                );
                let pick = |list: &[String], random: &mut R| {
                    // `Util.getRandom(list, random)` is
                    // `list.get(random.nextInt(list.size()))`: one draw.
                    let i = random.next_i32_bound(
                        i32::try_from(list.len()).expect("a state list fits in an i32"),
                    );
                    list[i as usize].clone()
                };
                if n < f64::from(*threshold) {
                    return Ok(pick(low_states, random));
                }
                if random.next_f32() < *high_chance {
                    return Ok(pick(high_states, random));
                }
                Ok(default_state.clone())
            }

            // `RandomizedIntStateProvider#getState`: the source first, then
            // the value — and the value is drawn whether or not the state
            // turns out to have the property.
            Self::RandomizedInt {
                source,
                property,
                values,
            } => {
                let state = source.state(world, tags, random, pos)?;
                let value = values.sample(random);
                Ok(with_property(&state, property, value))
            }

            // `WeightedList.getRandomOrThrow`: one `nextInt(total)`, then
            // subtract until negative. The walk order is the list's own, so
            // reordering the datapack's entries changes the world.
            Self::Weighted(entries) => {
                let total: i32 = entries.iter().map(|(_, w)| *w).sum();
                let mut roll = random.next_i32_bound(total.max(1));
                for (name, weight) in entries {
                    roll -= *weight;
                    if roll < 0 {
                        return Ok(name.clone());
                    }
                }
                // Unreachable while the weights are positive, and vanilla
                // throws here rather than falling back.
                Ok(entries
                    .last()
                    .expect("a weighted provider with no entries is refused at parse")
                    .0
                    .clone())
            }
            // `RuleBasedStateProvider#getState` is
            // `getOptionalState(...) ?? level.getBlockState(pos)` — a
            // position no rule matched keeps whatever is already there.
            Self::RuleBased { .. } => Ok(self
                .optional_state(world, tags, random, pos)?
                .unwrap_or_else(|| world.block_at(pos.0, pos.1, pos.2))),
        }
    }

    /// `BlockStateProvider#getOptionalState`.
    ///
    /// The base class's is just `getState`; only the rule-based provider can
    /// answer `None`, and the difference is load-bearing. A trunk placer asks
    /// with *this* one and writes nothing when it gets `None`, so a tree on
    /// stone leaves the stone alone. Asking with [`Self::state`] instead
    /// would write the stone back over itself, which places no different
    /// block but does record a write — and the decoration oracle records a
    /// *difference*, so the two are indistinguishable there and not
    /// indistinguishable in the leaf-distance pass that reads the set
    /// afterwards.
    ///
    /// # Errors
    /// If a predicate needs block behaviour the port does not have.
    pub fn optional_state<W: PredicateWorld + ?Sized, R: RandomSource + ?Sized>(
        &self,
        world: &mut W,
        tags: &BlockTags,
        random: &mut R,
        pos: (i32, i32, i32),
    ) -> Result<Option<String>, super::block_predicate::NeedsBehaviour> {
        match self {
            Self::RuleBased { fallback, rules } => {
                // First match wins, and the scan stops there — a later rule
                // whose predicate would also hold is never asked, so a
                // reordering changes the world.
                for (predicate, then) in rules {
                    if predicate.test(world, tags, pos)? {
                        return then.state(world, tags, random, pos).map(Some);
                    }
                }
                match fallback {
                    Some(fallback) => fallback.state(world, tags, random, pos).map(Some),
                    None => Ok(None),
                }
            }
            other => other.state(world, tags, random, pos).map(Some),
        }
    }
}

/// `state.setValue(property, value)`, on the canonical spelling.
///
/// A state that does not carry the property is returned unchanged, which is
/// vanilla's `hasProperty` guard — setting one it does not have would invent
/// a state that no block has.
fn with_property(state: &str, property: &str, value: i32) -> String {
    let needle = format!("{property}=");
    let Some((name, rest)) = state.split_once('[') else {
        return state.to_string();
    };
    let rest = rest.trim_end_matches(']');
    if !rest.split(',').any(|pair| pair.starts_with(&needle)) {
        return state.to_string();
    }
    let updated: Vec<String> = rest
        .split(',')
        .map(|pair| {
            if pair.starts_with(&needle) {
                format!("{property}={value}")
            } else {
                pair.to_string()
            }
        })
        .collect();
    format!("{name}[{}]", updated.join(","))
}

/// `state.trySetValue(RotatedPillarBlock.AXIS, axis)`.
///
/// Separate from [`with_property`] only because that one takes an `i32`.
/// Same `hasProperty` guard, and the same reason for it: `trySetValue` on a
/// block with no `axis` answers the state it was given.
fn set_axis(state: &str, axis: &str) -> String {
    let Some((name, rest)) = state.split_once('[') else {
        return state.to_string();
    };
    let rest = rest.trim_end_matches(']');
    if !rest.split(',').any(|pair| pair.starts_with("axis=")) {
        return state.to_string();
    }
    let updated: Vec<String> = rest
        .split(',')
        .map(|pair| {
            if pair.starts_with("axis=") {
                format!("axis={axis}")
            } else {
                pair.to_string()
            }
        })
        .collect();
    format!("{name}[{}]", updated.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::legacy::LegacyRandomSource;
    use crate::rng::worldgen_random::WorldgenRandom;
    use crate::tags::BlockTags;

    struct Column {
        blocks: std::collections::BTreeMap<(i32, i32, i32), String>,
    }

    impl PredicateWorld for Column {
        fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
            self.blocks
                .get(&(x, y, z))
                .cloned()
                .unwrap_or_else(|| "minecraft:air".to_string())
        }
    }

    fn world(entries: &[((i32, i32, i32), &str)]) -> Column {
        Column {
            blocks: entries
                .iter()
                .map(|(p, b)| (*p, (*b).to_string()))
                .collect(),
        }
    }

    /// Real default-state spellings, copied out of the block-props extract.
    fn states() -> super::super::state_spec::DefaultStateTable {
        super::super::state_spec::DefaultStateTable::from_pairs(&[
            ("minecraft:sand", "minecraft:sand"),
            ("minecraft:sandstone", "minecraft:sandstone"),
            (
                "minecraft:cave_vines",
                "minecraft:cave_vines[age=0,berries=false]",
            ),
            ("minecraft:moss_block", "minecraft:moss_block"),
        ])
    }

    fn provider(json: &str) -> StateProvider {
        StateProvider::parse(&serde_json::from_str(json).expect("json"), &states()).expect("parse")
    }

    #[test]
    fn a_simple_provider_draws_nothing() {
        let p = provider(
            r#"{"type":"minecraft:simple_state_provider","state":{"Name":"minecraft:sand"}}"#,
        );
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(1));
        let before = random.next_i64();
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(1));
        let mut w = world(&[]);
        let tags = BlockTags::from_entries(std::collections::BTreeMap::new()).expect("tags");
        assert_eq!(
            p.state(&mut w, &tags, &mut random, (0, 0, 0)).expect("ok"),
            "minecraft:sand"
        );
        assert_eq!(
            random.next_i64(),
            before,
            "the provider moved the stream when it should not have"
        );
    }

    /// `disk_sand`'s real provider: sandstone when the block *below* is air,
    /// sand otherwise. The offset is what makes a beach have a lip.
    #[test]
    fn the_rule_based_provider_reads_the_offset_block() {
        let p = provider(
            r#"{"type":"minecraft:rule_based_state_provider",
                "fallback":{"type":"minecraft:simple_state_provider",
                            "state":{"Name":"minecraft:sand"}},
                "rules":[{"if_true":{"type":"minecraft:matching_blocks",
                                     "blocks":"minecraft:air","offset":[0,-1,0]},
                          "then":{"type":"minecraft:simple_state_provider",
                                  "state":{"Name":"minecraft:sandstone"}}}]}"#,
        );
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(1));
        let tags = BlockTags::from_entries(std::collections::BTreeMap::new()).expect("tags");

        let mut over_air = world(&[((0, -1, 0), "minecraft:air")]);
        assert_eq!(
            p.state(&mut over_air, &tags, &mut random, (0, 0, 0))
                .expect("ok"),
            "minecraft:sandstone"
        );

        let mut over_stone = world(&[((0, -1, 0), "minecraft:stone")]);
        assert_eq!(
            p.state(&mut over_stone, &tags, &mut random, (0, 0, 0))
                .expect("ok"),
            "minecraft:sand"
        );
    }

    /// An unknown provider is an error, not a fallback. A feature quietly
    /// placing its fallback block still produces a world — the wrong one.
    #[test]
    fn an_unknown_provider_is_refused() {
        let error = StateProvider::parse(
            &serde_json::from_str(r#"{"type":"minecraft:noise_provider"}"#).expect("json"),
            &states(),
        )
        .expect_err("should refuse");
        assert!(error.contains("noise_provider"), "{error}");
    }

    #[test]
    fn every_disk_provider_in_the_datapack_parses() {
        let dir = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../oracle-work/vanilla-26.2/generated-data/data/minecraft/worldgen/configured_feature"
        ));
        let Some(extract) = super::super::state_spec::extract_or_skip() else {
            return;
        };
        if !rustmc_data::fixture::available(dir, "datagen extract", "ci/vanilla-server.sh data") {
            return;
        }
        let mut seen = 0usize;
        for entry in std::fs::read_dir(dir).expect("readable") {
            let path = entry.expect("entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let json: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).expect("readable"))
                    .expect("json");
            if json["type"] != "minecraft:disk" {
                continue;
            }
            StateProvider::parse(&json["config"]["state_provider"], &extract)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            seen += 1;
        }
        assert_eq!(
            seen, 5,
            "the datapack has five disk features — four named disk_*, plus one that is not"
        );
    }
}
