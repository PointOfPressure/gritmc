//! Surface rules — the tree that decides grass, sand, gravel and bedrock.
//!
//! Reference: `net.minecraft.world.level.levelgen.SurfaceRules`, but the
//! *tree itself* comes from data rather than from that class.
//!
//! # Why this is parsed and not transcribed
//!
//! `SurfaceRuleData` builds the overworld's rule in code, and the result is
//! **35 KB of JSON, 22 levels deep, with 149 conditions**. Hand-copying it
//! would be 149 chances to be quietly wrong somewhere no near-origin test
//! looks — the same shape as the two worst bugs this project has already
//! found. So `ci/vanilla-server.sh data` runs Mojang's own generator and the
//! tree is read as data.
//!
//! That is also what vanilla does at runtime: surface rules are datapack
//! content, and a server has to parse them anyway to support datapacks.
//! Reading them is the *normal* path, not a shortcut.
//!
//! # The whole vocabulary is fifteen operators
//!
//! Counted from the real tree, so this is complete for the overworld: three
//! rule kinds (`sequence`, `condition`, `block`) plus `bandlands`, and eleven
//! condition kinds. Anything else in an input is rejected rather than
//! ignored — a rule silently dropped is a biome that quietly loses its
//! surface.

use serde_json::Value;
use std::collections::BTreeSet;
use std::fmt;

/// A block state as written in the rule tree: a name plus sorted properties.
///
/// Kept as the same canonical `name[k=v,k=v]` string the Anvil layer uses,
/// so a rule's output can be compared and written without a registry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockState(pub String);

impl fmt::Display for BlockState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// `VerticalAnchor` — a y coordinate expressed relative to the world.
///
/// Not a plain int, because a rule that says "five above the bottom" has to
/// mean different blocks in the overworld and the nether.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerticalAnchor {
    Absolute(i32),
    AboveBottom(i32),
    BelowTop(i32),
}

impl VerticalAnchor {
    /// Resolve against a dimension's `min_y` and height.
    #[must_use]
    pub const fn resolve(self, min_y: i32, height: i32) -> i32 {
        match self {
            Self::Absolute(y) => y,
            Self::AboveBottom(y) => min_y + y,
            // `MAX_Y` is inclusive, hence the -1: "below top 0" is the
            // highest placeable block, not one past it.
            Self::BelowTop(y) => min_y + height - 1 - y,
        }
    }
}

/// `SurfaceRules.VerticalAnchor`'s two ends in a `vertical_gradient`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoneDepthSurface {
    Floor,
    Ceiling,
}

/// A condition — `SurfaceRules.ConditionSource`.
#[derive(Clone, Debug, PartialEq)]
pub enum Condition {
    /// `biome` — one or more biome ids. Vanilla writes a bare string for a
    /// single biome and a list otherwise; both parse to this.
    Biome(Vec<String>),
    /// The same condition with its names resolved against the world's biome
    /// palette — what the surface stage actually evaluates.
    ///
    /// The parser never produces this; [`Rule::intern_biomes`] rewrites every
    /// [`Condition::Biome`] into one before the stage runs, so the golden
    /// parse tests keep comparing against names and the inner loop compares
    /// integers. Both variants are evaluated, and they agree by construction:
    /// interning is injective, so `ids.contains(id(a))` is true exactly when
    /// `names.contains(a)`.
    BiomeIds(Vec<crate::surface::BiomeId>),
    /// `noise_threshold` — a named surface noise inside `[min, max]`.
    ///
    /// `is_3d` decides whether the noise is sampled at this block's `y` or at
    /// `y = 0`, and it is **not** a property of the noise: the same noise can
    /// be named by a 2D condition in one rule and a 3D one in another.
    /// Dropping the field — as this port did until the sulfur caves showed up
    /// — turns a 3D condition into a y-independent one, which stops being a
    /// threshold and starts being a vertical stripe.
    NoiseThreshold {
        noise: String,
        min_threshold: f64,
        max_threshold: f64,
        is_3d: bool,
    },
    /// `vertical_gradient` — true below one anchor, false above another, and
    /// a seeded random fade between them. `random_name` seeds that fade, so
    /// two gradients with the same anchors and different names differ.
    VerticalGradient {
        random_name: String,
        true_at_and_below: VerticalAnchor,
        false_at_and_above: VerticalAnchor,
    },
    /// `y_above`.
    YAbove {
        anchor: VerticalAnchor,
        surface_depth_multiplier: i32,
        add_stone_depth: bool,
    },
    /// `water` — depth below the local water level.
    Water {
        offset: i32,
        surface_depth_multiplier: i32,
        add_stone_depth: bool,
    },
    /// `stone_depth` — how far into stone this block is, from the floor or
    /// the ceiling.
    StoneDepth {
        offset: i32,
        add_surface_depth: bool,
        secondary_depth_range: i32,
        surface_type: StoneDepthSurface,
    },
    /// `hole` — the column has no surface depth at all.
    Hole,
    /// `above_preliminary_surface`.
    AbovePreliminarySurface,
    /// `steep` — a sharp local gradient, which is what keeps grass off
    /// cliffs.
    Steep,
    /// `temperature` — the frozen-ocean test.
    Temperature,
    Not(Box<Condition>),
}

/// A rule — `SurfaceRules.RuleSource`.
#[derive(Clone, Debug, PartialEq)]
pub enum Rule {
    /// First child that produces a block wins; `None` if none do.
    Sequence(Vec<Rule>),
    /// Run the rule only if the condition holds.
    Condition {
        if_true: Condition,
        then_run: Box<Rule>,
    },
    /// Place this state.
    Block(BlockState),
    /// The badlands colour banding — a whole generator in one operator,
    /// seeded separately from everything else.
    Bandlands,
}

impl Condition {
    /// See [`Rule::intern_biomes`].
    ///
    /// Order is preserved rather than sorted. The evaluator asks only whether
    /// the actual id is present, so order cannot change an answer — but a
    /// sort would make the `Debug` output stop matching the datapack, and
    /// `RUSTMC_TRACE_BLOCK` prints these lists to explain a wrong block.
    pub fn intern_biomes<F>(&mut self, id_of: &F, missing: &mut Vec<String>)
    where
        F: Fn(&str) -> Option<crate::surface::BiomeId>,
    {
        match self {
            Self::Biome(names) => {
                let mut ids = Vec::with_capacity(names.len());
                for name in names.iter() {
                    match id_of(name) {
                        Some(id) => ids.push(id),
                        None => missing.push(name.clone()),
                    }
                }
                *self = Self::BiomeIds(ids);
            }
            Self::Not(inner) => inner.intern_biomes(id_of, missing),
            _ => {}
        }
    }
}

impl Rule {
    /// Rewrite every [`Condition::Biome`] in this tree into a
    /// [`Condition::BiomeIds`] against the world's palette, once, before the
    /// stage runs.
    ///
    /// Returns the names that did not resolve. **The caller must check it is
    /// empty.** A name that silently failed to resolve would produce a
    /// condition that can never be true, which is exactly the shape of defect
    /// this project keeps paying for: the rule would still evaluate, the world
    /// would still generate, and a biome's whole surface treatment would
    /// vanish with nothing red anywhere. Returning the misses rather than
    /// dropping them is what makes that a loud failure instead of a quiet one.
    ///
    /// The walk visits `Not` because it is the only nesting a `Condition` has.
    pub fn intern_biomes<F>(&mut self, id_of: &F, missing: &mut Vec<String>)
    where
        F: Fn(&str) -> Option<crate::surface::BiomeId>,
    {
        match self {
            Self::Sequence(rules) => {
                for rule in rules {
                    rule.intern_biomes(id_of, missing);
                }
            }
            Self::Condition { if_true, then_run } => {
                if_true.intern_biomes(id_of, missing);
                then_run.intern_biomes(id_of, missing);
            }
            Self::Block(_) | Self::Bandlands => {}
        }
    }

    /// Every block state this tree can place, plus whether it can reach
    /// [`Rule::Bandlands`].
    ///
    /// Used by the surface gate to report how much of the rule's output
    /// vocabulary its chunks actually exercise. A parity percentage says how
    /// often the port agrees on the blocks it was asked about; this says how
    /// much of the rule it was asked about at all, and the two are
    /// independent. The overworld rule can place 26 distinct blocks, and the
    /// gate's first honest measurement reached ten of them.
    #[must_use]
    pub fn placeable_states(&self) -> (BTreeSet<String>, bool) {
        let mut states = BTreeSet::new();
        let mut bandlands = false;
        self.collect_placeable(&mut states, &mut bandlands);
        (states, bandlands)
    }

    fn collect_placeable(&self, states: &mut BTreeSet<String>, bandlands: &mut bool) {
        match self {
            Self::Sequence(children) => {
                for child in children {
                    child.collect_placeable(states, bandlands);
                }
            }
            Self::Condition { then_run, .. } => then_run.collect_placeable(states, bandlands),
            Self::Block(state) => {
                states.insert(state.0.clone());
            }
            // Not enumerated as states: the palette it draws from is
            // generated from a stream at world creation, so "which blocks can
            // it place" is a property of the seed, not of the rule.
            Self::Bandlands => *bandlands = true,
        }
    }
}

/// What went wrong parsing a rule tree.
#[derive(Debug, PartialEq, Eq)]
pub struct ParseError(String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "surface rule: {}", self.0)
    }
}

impl std::error::Error for ParseError {}

fn err<T>(message: impl Into<String>) -> Result<T, ParseError> {
    Err(ParseError(message.into()))
}

fn kind(value: &Value) -> Result<&str, ParseError> {
    value
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| ParseError(format!("node has no `type`: {value}")))
}

fn i32_field(value: &Value, key: &str) -> Result<i32, ParseError> {
    value
        .get(key)
        .and_then(Value::as_i64)
        .and_then(|v| i32::try_from(v).ok())
        .ok_or_else(|| ParseError(format!("`{key}` missing or not an i32 in {value}")))
}

fn f64_field(value: &Value, key: &str) -> Result<f64, ParseError> {
    value
        .get(key)
        .and_then(Value::as_f64)
        .ok_or_else(|| ParseError(format!("`{key}` missing or not a number in {value}")))
}

fn str_field<'a>(value: &'a Value, key: &str) -> Result<&'a str, ParseError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ParseError(format!("`{key}` missing or not a string in {value}")))
}

/// Booleans are written explicitly by the generator, so a missing one is a
/// schema change rather than something to default away.
fn bool_field(value: &Value, key: &str) -> Result<bool, ParseError> {
    value
        .get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| ParseError(format!("`{key}` missing or not a bool in {value}")))
}

/// `{"absolute": n}` / `{"above_bottom": n}` / `{"below_top": n}`.
pub fn parse_anchor(value: &Value) -> Result<VerticalAnchor, ParseError> {
    let object = value
        .as_object()
        .ok_or_else(|| ParseError(format!("anchor is not an object: {value}")))?;
    match object.iter().next() {
        Some((key, raw)) if object.len() == 1 => {
            let y = raw
                .as_i64()
                .and_then(|v| i32::try_from(v).ok())
                .ok_or_else(|| ParseError(format!("anchor value not an i32: {raw}")))?;
            match key.as_str() {
                "absolute" => Ok(VerticalAnchor::Absolute(y)),
                "above_bottom" => Ok(VerticalAnchor::AboveBottom(y)),
                "below_top" => Ok(VerticalAnchor::BelowTop(y)),
                other => err(format!("unknown vertical anchor `{other}`")),
            }
        }
        _ => err(format!("anchor must have exactly one key: {value}")),
    }
}

/// `{"Name": "...", "Properties": {...}}` to the canonical
/// `name[k=v,k=v]` spelling, properties sorted.
///
/// The same spelling `rustmc_world::anvil` reads back, so a rule's output can
/// be compared against a chunk on disk without a block registry existing yet.
pub fn parse_block_state(value: &Value) -> Result<BlockState, ParseError> {
    let name = str_field(value, "Name")?;
    let Some(properties) = value.get("Properties") else {
        return Ok(BlockState(name.to_string()));
    };
    let properties = properties
        .as_object()
        .ok_or_else(|| ParseError(format!("Properties is not an object: {properties}")))?;
    // serde_json's Map preserves insertion order by default, so sort here
    // rather than trusting it — the canonical form is sorted.
    let mut pairs: Vec<(&str, String)> = properties
        .iter()
        .map(|(k, v)| {
            let rendered = match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            (k.as_str(), rendered)
        })
        .collect();
    pairs.sort_unstable();
    let rendered = pairs
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(",");
    Ok(BlockState(format!("{name}[{rendered}]")))
}

pub fn parse_condition(value: &Value) -> Result<Condition, ParseError> {
    Ok(match kind(value)? {
        "minecraft:biome" => {
            let raw = value
                .get("biome_is")
                .ok_or_else(|| ParseError(format!("biome has no `biome_is`: {value}")))?;
            let biomes = match raw {
                Value::String(one) => vec![one.clone()],
                Value::Array(many) => many
                    .iter()
                    .map(|v| {
                        v.as_str()
                            .map(str::to_string)
                            .ok_or_else(|| ParseError(format!("biome id not a string: {v}")))
                    })
                    .collect::<Result<_, _>>()?,
                other => return err(format!("`biome_is` must be a string or list: {other}")),
            };
            Condition::Biome(biomes)
        }
        "minecraft:noise_threshold" => Condition::NoiseThreshold {
            noise: str_field(value, "noise")?.to_string(),
            min_threshold: f64_field(value, "min_threshold")?,
            max_threshold: f64_field(value, "max_threshold")?,
            // `Codec.BOOL.optionalFieldOf("is_3d", false)` — absent means 2D,
            // which is what every pre-26.2 rule relied on.
            is_3d: match value.get("is_3d") {
                None => false,
                Some(v) => v
                    .as_bool()
                    .ok_or_else(|| ParseError(format!("`is_3d` must be a boolean: {v}")))?,
            },
        },
        "minecraft:vertical_gradient" => Condition::VerticalGradient {
            random_name: str_field(value, "random_name")?.to_string(),
            true_at_and_below: parse_anchor(
                value
                    .get("true_at_and_below")
                    .ok_or_else(|| ParseError("missing true_at_and_below".into()))?,
            )?,
            false_at_and_above: parse_anchor(
                value
                    .get("false_at_and_above")
                    .ok_or_else(|| ParseError("missing false_at_and_above".into()))?,
            )?,
        },
        "minecraft:y_above" => Condition::YAbove {
            anchor: parse_anchor(
                value
                    .get("anchor")
                    .ok_or_else(|| ParseError("missing anchor".into()))?,
            )?,
            surface_depth_multiplier: i32_field(value, "surface_depth_multiplier")?,
            add_stone_depth: bool_field(value, "add_stone_depth")?,
        },
        "minecraft:water" => Condition::Water {
            offset: i32_field(value, "offset")?,
            surface_depth_multiplier: i32_field(value, "surface_depth_multiplier")?,
            add_stone_depth: bool_field(value, "add_stone_depth")?,
        },
        "minecraft:stone_depth" => Condition::StoneDepth {
            offset: i32_field(value, "offset")?,
            add_surface_depth: bool_field(value, "add_surface_depth")?,
            secondary_depth_range: i32_field(value, "secondary_depth_range")?,
            surface_type: match str_field(value, "surface_type")? {
                "floor" => StoneDepthSurface::Floor,
                "ceiling" => StoneDepthSurface::Ceiling,
                other => return err(format!("unknown surface_type `{other}`")),
            },
        },
        "minecraft:hole" => Condition::Hole,
        "minecraft:above_preliminary_surface" => Condition::AbovePreliminarySurface,
        "minecraft:steep" => Condition::Steep,
        "minecraft:temperature" => Condition::Temperature,
        "minecraft:not" => Condition::Not(Box::new(parse_condition(
            value
                .get("invert")
                .ok_or_else(|| ParseError("not has no `invert`".into()))?,
        )?)),
        other => return err(format!("unknown condition `{other}`")),
    })
}

pub fn parse_rule(value: &Value) -> Result<Rule, ParseError> {
    Ok(match kind(value)? {
        "minecraft:sequence" => {
            let items = value
                .get("sequence")
                .and_then(Value::as_array)
                .ok_or_else(|| ParseError(format!("sequence has no list: {value}")))?;
            Rule::Sequence(items.iter().map(parse_rule).collect::<Result<_, _>>()?)
        }
        "minecraft:condition" => Rule::Condition {
            if_true: parse_condition(
                value
                    .get("if_true")
                    .ok_or_else(|| ParseError("condition has no `if_true`".into()))?,
            )?,
            then_run: Box::new(parse_rule(
                value
                    .get("then_run")
                    .ok_or_else(|| ParseError("condition has no `then_run`".into()))?,
            )?),
        },
        "minecraft:block" => Rule::Block(parse_block_state(
            value
                .get("result_state")
                .ok_or_else(|| ParseError("block has no `result_state`".into()))?,
        )?),
        "minecraft:bandlands" => Rule::Bandlands,
        other => return err(format!("unknown rule `{other}`")),
    })
}

/// Parse the `surface_rule` out of a noise-settings document.
pub fn parse_noise_settings_surface_rule(settings: &Value) -> Result<Rule, ParseError> {
    let rule = settings
        .get("surface_rule")
        .ok_or_else(|| ParseError("noise settings have no `surface_rule`".into()))?;
    parse_rule(rule)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn anchors_resolve_against_the_dimension() {
        // Overworld: min -64, height 384, so the top placeable block is 319.
        assert_eq!(VerticalAnchor::Absolute(60).resolve(-64, 384), 60);
        assert_eq!(VerticalAnchor::AboveBottom(5).resolve(-64, 384), -59);
        assert_eq!(VerticalAnchor::BelowTop(0).resolve(-64, 384), 319);
        // The -1 is load-bearing: without it "below top 0" is one past the
        // world and bedrock ceilings land nowhere.
        assert_eq!(VerticalAnchor::BelowTop(4).resolve(-64, 384), 315);
        // And the same anchor means a different block in the nether.
        assert_eq!(VerticalAnchor::AboveBottom(5).resolve(0, 128), 5);
    }

    #[test]
    fn block_states_render_in_the_canonical_spelling() {
        assert_eq!(
            parse_block_state(&json!({"Name": "minecraft:stone"})).unwrap(),
            BlockState("minecraft:stone".into())
        );
        // Properties sorted, matching what `rustmc_world::anvil` reads back —
        // otherwise a rule's output could never be compared to a chunk.
        let stairs = parse_block_state(&json!({
            "Name": "minecraft:oak_stairs",
            "Properties": {"half": "top", "facing": "east"}
        }))
        .unwrap();
        assert_eq!(
            stairs,
            BlockState("minecraft:oak_stairs[facing=east,half=top]".into())
        );
    }

    #[test]
    fn a_single_biome_and_a_list_parse_the_same_way() {
        // Vanilla writes a bare string for one biome and a list otherwise.
        let one =
            parse_condition(&json!({"type": "minecraft:biome", "biome_is": "minecraft:plains"}))
                .unwrap();
        assert_eq!(one, Condition::Biome(vec!["minecraft:plains".into()]));
        let many = parse_condition(
            &json!({"type": "minecraft:biome", "biome_is": ["minecraft:plains", "minecraft:desert"]}),
        )
        .unwrap();
        assert_eq!(
            many,
            Condition::Biome(vec!["minecraft:plains".into(), "minecraft:desert".into()])
        );
    }

    #[test]
    fn an_unknown_operator_is_rejected_not_ignored() {
        // A dropped rule is a biome that quietly loses its surface, which
        // would show up as a subtle diff thousands of blocks away rather
        // than as a failure here.
        let bad = parse_rule(&json!({"type": "minecraft:invented"}));
        assert!(bad.is_err());
        let bad = parse_condition(&json!({"type": "minecraft:invented"}));
        assert!(bad.is_err());
        // And a malformed node is an error rather than a default.
        assert!(parse_condition(&json!({"type": "minecraft:y_above"})).is_err());
    }

    #[test]
    fn nesting_round_trips_through_not() {
        let parsed = parse_condition(&json!({
            "type": "minecraft:not",
            "invert": {"type": "minecraft:steep"}
        }))
        .unwrap();
        assert_eq!(parsed, Condition::Not(Box::new(Condition::Steep)));
    }
}
