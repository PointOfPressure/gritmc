//! `StructureTemplatePool` and the `minecraft:jigsaw` structure definition.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `StructureTemplatePool`, `StructurePoolElement` and its five
//! subclasses, `JigsawStructure`, `TerrainAdjustment`, `VerticalAnchor`,
//! `LiquidSettings`, `DimensionPadding`.
//!
//! # Why this is its own layer
//!
//! Villages, pillager outposts, ancient cities, bastions, trail ruins and
//! trial chambers are **all** `minecraft:jigsaw` — ten of the thirty-four
//! structure definitions in 26.2 — and they differ only in their data. So the
//! pool reader is shared spine, and getting it wrong is wrong six times over.
//! It is also the one part of jigsaw generation that can be proved with no
//! world at all: a pool is a static file, and either every one of the 188 in
//! the datapack parses or the schema here is incomplete.
//!
//! Selection lives here too — [`TemplatePool::random_template`] and
//! [`TemplatePool::shuffled_templates`] — because the draw order is a
//! *property of the pool*, not of the walk that consumes it, and it was read
//! off the bytecode rather than assumed. **No piece is placed here**: the
//! walk, the junctions and the free-space carve are the assembler's.
//!
//! The thing that makes selection belong to the pool is [`TemplatePool::expanded`].
//! Vanilla's constructor adds each element to `templates` **`weight` times**,
//! and every selection indexes that expanded list — so a village town-centre
//! pool of four elements at weight 50 and four at weight 1 is 204 entries,
//! `getRandomTemplate` is `nextInt(204)`, and `getShuffledTemplates` spends
//! **203** draws. A cumulative-weight scan over the eight raw entries would
//! give the same distribution and the wrong stream.
//!
//! # The census this was written against
//!
//! Every pool in `worldgen/template_pool`, counted rather than sampled:
//!
//! ```text
//!   188 files, 1198 entries, exactly two keys per pool: elements, fallback
//!   element_type   legacy_single 601   single 527   empty 31
//!                  feature 36          list 3
//!   processors     name 753   inline 375   absent 70
//!   projection     present 1167 of 1198 — absent exactly on empty elements
//!   list elements  single 4, legacy_single 2 — no nesting deeper than one
//! ```
//!
//! and every jigsaw structure:
//!
//! ```text
//!   size 6 (villages, bastion) 7 (outpost, ancient city, trail ruins) 20 (trial chambers)
//!   start_height   absolute x9, uniform(absolute -40, absolute -20) x1
//!   terrain_adaptation  beard_thin 6, beard_box 1, bury 1, encapsulate 1, absent 1
//!   use_expansion_hack  true only for the five villages and the outpost
//!   project_start_to_heightmap  WORLD_SURFACE_WG on 7, absent on 3
//!   start_jigsaw_name   ancient_city only (minecraft:city_anchor)
//!   dimension_padding / liquid_settings   trial_chambers only
//! ```
//!
//! # `legacy_single_pool_element` is not a synonym
//!
//! It is `SinglePoolElement` with `overrideLiquidSettings` = `APPLY_WATERLOGGING`
//! and the `BlockIgnoreProcessor` set that keeps structure voids *and* air,
//! where the modern one keeps only structure voids. Villages are almost
//! entirely legacy elements, so treating the two as one would flood or
//! un-flood most of a village. It is kept as a flag rather than a separate
//! variant because everything else about them is identical.

use serde_json::Value;

use crate::feature::placement::HeightmapKind;

/// A malformed pool or structure definition, named.
#[derive(Debug, PartialEq, Eq)]
pub struct Malformed(pub String);

fn bad<T>(what: impl Into<String>) -> Result<T, Malformed> {
    Err(Malformed(what.into()))
}

/// `StructureTemplatePool.Projection`.
///
/// `rigid` places the element at the y the jigsaw connection asks for;
/// `terrain_matching` re-projects it onto the heightmap and lets the
/// `GravityProcessor` drop it. Village houses are rigid and their paths are
/// terrain-matching, which is why a village road follows a hill and the
/// houses do not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Projection {
    Rigid,
    TerrainMatching,
}

impl Projection {
    /// # Errors
    /// If the string names neither projection.
    pub fn parse(name: &str) -> Result<Self, Malformed> {
        match name {
            "rigid" => Ok(Self::Rigid),
            "terrain_matching" => Ok(Self::TerrainMatching),
            other => bad(format!("unknown projection `{other}`")),
        }
    }
}

/// A pool element's processor list: a registry name, or one written inline.
///
/// Kept unresolved on purpose. `processor.rs` resolves a list into rules, and
/// doing that here would make loading a pool depend on the processor
/// registry being present — which is how a parser gate turns into an
/// integration test that skips.
#[derive(Clone, Debug, PartialEq)]
pub enum Processors {
    /// `"minecraft:mossify_20_percent"`.
    Named(String),
    /// `{"processors": [...]}`, written into the pool rather than registered.
    Inline(Value),
}

/// `StructurePoolElement`.
#[derive(Clone, Debug, PartialEq)]
pub enum PoolElement {
    /// `EmptyPoolElement` — occupies a weight slot and places nothing. It is
    /// how a pool says "this branch may stop here", and it carries no
    /// projection.
    Empty,
    /// `SinglePoolElement`, and `LegacySinglePoolElement` when `legacy`.
    Single {
        /// `minecraft:village/plains/houses/plains_small_house_1`.
        location: String,
        processors: Option<Processors>,
        projection: Projection,
        legacy: bool,
    },
    /// `FeaturePoolElement` — a placed feature standing in for a template,
    /// which is how villages plant their trees and how bastions place their
    /// lava.
    Feature {
        feature: String,
        projection: Projection,
    },
    /// `ListPoolElement` — every child placed, in order, at one position.
    List {
        elements: Vec<PoolElement>,
        projection: Projection,
    },
}

impl PoolElement {
    /// # Errors
    /// If the element type is unknown or a field it needs is missing.
    pub fn parse(value: &Value) -> Result<Self, Malformed> {
        let kind = value
            .get("element_type")
            .and_then(Value::as_str)
            .ok_or_else(|| Malformed("pool element has no `element_type`".to_string()))?;
        // Only `empty` has no projection, so it is read after the branch that
        // does not need it rather than being made optional for everyone.
        let projection = || -> Result<Projection, Malformed> {
            let name = value
                .get("projection")
                .and_then(Value::as_str)
                .ok_or_else(|| Malformed(format!("`{kind}` has no `projection`")))?;
            Projection::parse(name)
        };
        match kind {
            "minecraft:empty_pool_element" => Ok(Self::Empty),
            "minecraft:single_pool_element" | "minecraft:legacy_single_pool_element" => {
                let location = value
                    .get("location")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Malformed(format!("`{kind}` has no `location`")))?;
                let processors = match value.get("processors") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(name)) => Some(Processors::Named(name.clone())),
                    Some(other) => Some(Processors::Inline(other.clone())),
                };
                Ok(Self::Single {
                    location: location.to_string(),
                    processors,
                    projection: projection()?,
                    legacy: kind == "minecraft:legacy_single_pool_element",
                })
            }
            "minecraft:feature_pool_element" => {
                let feature = value
                    .get("feature")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        Malformed("`feature_pool_element` has no `feature`".to_string())
                    })?;
                Ok(Self::Feature {
                    feature: feature.to_string(),
                    projection: projection()?,
                })
            }
            "minecraft:list_pool_element" => {
                let entries = value
                    .get("elements")
                    .and_then(Value::as_array)
                    .ok_or_else(|| {
                        Malformed("`list_pool_element` has no `elements`".to_string())
                    })?;
                let elements = entries
                    .iter()
                    .map(Self::parse)
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Self::List {
                    elements,
                    projection: projection()?,
                })
            }
            other => bad(format!("unknown pool element type `{other}`")),
        }
    }

    /// The projection this element places with. `Empty` has none and is never
    /// placed, so it answers `Rigid` — the value vanilla's
    /// `EmptyPoolElement.getProjection` inherits from
    /// `StructurePoolElement`'s field, which it never sets.
    #[must_use]
    pub fn projection(&self) -> Projection {
        match self {
            Self::Empty => Projection::Rigid,
            Self::Single { projection, .. }
            | Self::Feature { projection, .. }
            | Self::List { projection, .. } => *projection,
        }
    }
}

/// One weighted entry of a pool.
#[derive(Clone, Debug, PartialEq)]
pub struct PoolEntry {
    pub element: PoolElement,
    /// `WeightedList` weights are positive ints; villages use 1..=50 and the
    /// zombie variants ride at weight 1 against a town centre's 50.
    pub weight: i32,
}

/// `StructureTemplatePool`.
#[derive(Clone, Debug, PartialEq)]
pub struct TemplatePool {
    pub elements: Vec<PoolEntry>,
    /// The pool to fall back to when this one's elements do not fit — always
    /// present in the datapack, and `minecraft:empty` for a terminal pool.
    pub fallback: String,
}

impl TemplatePool {
    /// # Errors
    /// If a required key is missing or an element does not parse.
    pub fn parse(value: &Value) -> Result<Self, Malformed> {
        let entries = value
            .get("elements")
            .and_then(Value::as_array)
            .ok_or_else(|| Malformed("pool has no `elements`".to_string()))?;
        let mut elements = Vec::with_capacity(entries.len());
        for entry in entries {
            let element = entry
                .get("element")
                .ok_or_else(|| Malformed("pool entry has no `element`".to_string()))?;
            let weight = entry
                .get("weight")
                .and_then(Value::as_i64)
                .ok_or_else(|| Malformed("pool entry has no `weight`".to_string()))?;
            let weight = i32::try_from(weight)
                .map_err(|_| Malformed(format!("pool entry weight {weight} is not an i32")))?;
            if weight <= 0 {
                return bad(format!("pool entry weight {weight} is not positive"));
            }
            elements.push(PoolEntry {
                element: PoolElement::parse(element)?,
                weight,
            });
        }
        let fallback = value
            .get("fallback")
            .and_then(Value::as_str)
            .ok_or_else(|| Malformed("pool has no `fallback`".to_string()))?;
        Ok(Self {
            elements,
            fallback: fallback.to_string(),
        })
    }

    /// The sum of the weights, which is what a weighted draw is taken against.
    #[must_use]
    pub fn total_weight(&self) -> i32 {
        self.elements.iter().map(|e| e.weight).sum()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    /// `StructureTemplatePool`'s `templates` field: each entry repeated
    /// **`weight` times**, in file order, as indices into [`Self::elements`].
    ///
    /// This is not a detail of how a weighted draw is spelled — it *is* the
    /// draw. The constructor's second loop is
    /// `for (int i = 0; i < weight; i++) this.templates.add(element)`, and
    /// every selection below indexes that expanded list directly. A pool of
    /// four elements at weight 50 and four at weight 1 is **204** entries, so
    /// `getRandomTemplate` draws `nextInt(204)` and `getShuffledTemplates`
    /// spends **203** draws, not 7. Implementing the weights as a cumulative
    /// scan over eight entries would give the same *distribution* and the
    /// wrong stream, which moves every later piece in the structure.
    #[must_use]
    pub fn expanded(&self) -> Vec<usize> {
        let mut out = Vec::with_capacity(
            usize::try_from(self.total_weight().max(0)).unwrap_or(self.elements.len()),
        );
        for (index, entry) in self.elements.iter().enumerate() {
            for _ in 0..entry.weight {
                out.push(index);
            }
        }
        out
    }

    /// `StructureTemplatePool.getRandomTemplate` — **one** draw,
    /// `nextInt(templates.size())` over the expanded list.
    ///
    /// An empty pool returns `EmptyPoolElement.INSTANCE` **without drawing**;
    /// the size check comes first in the bytecode (offsets 0-13). Only
    /// `minecraft:empty` is empty in 26.2, and it is the fallback of 166 of
    /// the 188 pools, so this branch is reached constantly and a draw spent
    /// here would desynchronise every structure that terminates.
    pub fn random_template(&self, random: &mut dyn crate::rng::RandomSource) -> PoolElement {
        let expanded = self.expanded();
        let Ok(size) = i32::try_from(expanded.len()) else {
            return PoolElement::Empty;
        };
        if size == 0 {
            return PoolElement::Empty;
        }
        let index = random.next_i32_bound(size);
        let index = usize::try_from(index).expect("nextInt is non-negative");
        self.elements[expanded[index]].element.clone()
    }

    /// `StructureTemplatePool.getShuffledTemplates` —
    /// `Util.shuffledCopy(templates, random)`, so **`size - 1` draws** over
    /// the expanded list.
    ///
    /// `Util.shuffle` is a **downward** Fisher-Yates, read off the bytecode:
    ///
    /// ```text
    ///   for (int i = list.size(); i > 1; i--)
    ///       list.set(i - 1, list.set(random.nextInt(i), list.get(i - 1)));
    /// ```
    ///
    /// The nested `set` is a swap of `i - 1` with `nextInt(i)`, and the bound
    /// is `i`, not `i + 1` — an upward Fisher-Yates over the same list draws
    /// the same *number* of values and produces a different permutation.
    pub fn shuffled_templates(
        &self,
        random: &mut dyn crate::rng::RandomSource,
    ) -> Vec<PoolElement> {
        let mut expanded = self.expanded();
        let mut i = expanded.len();
        while i > 1 {
            let bound = i32::try_from(i).expect("a pool is far smaller than i32::MAX");
            let j = usize::try_from(random.next_i32_bound(bound)).expect("non-negative");
            expanded.swap(i - 1, j);
            i -= 1;
        }
        expanded
            .into_iter()
            .map(|index| self.elements[index].element.clone())
            .collect()
    }
}

/// `VerticalAnchor` — a y written relative to the world rather than absolutely.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerticalAnchor {
    Absolute(i32),
    AboveBottom(i32),
    BelowTop(i32),
}

impl VerticalAnchor {
    /// # Errors
    /// If the object names none of the three anchors.
    pub fn parse(value: &Value) -> Result<Self, Malformed> {
        for (key, make) in [
            ("absolute", Self::Absolute as fn(i32) -> Self),
            ("above_bottom", Self::AboveBottom as fn(i32) -> Self),
            ("below_top", Self::BelowTop as fn(i32) -> Self),
        ] {
            if let Some(v) = value.get(key).and_then(Value::as_i64) {
                return i32::try_from(v)
                    .map(make)
                    .map_err(|_| Malformed(format!("vertical anchor {v} is not an i32")));
            }
        }
        bad(format!("not a vertical anchor: {value}"))
    }

    /// `VerticalAnchor.resolveY(WorldGenerationContext)`. `below_top` is
    /// `minY + height - 1 - n`, not `minY + height - n`: the top y is the
    /// last one in the world, not one past it.
    #[must_use]
    pub fn resolve(self, min_y: i32, height: i32) -> i32 {
        match self {
            Self::Absolute(y) => y,
            Self::AboveBottom(n) => min_y + n,
            Self::BelowTop(n) => min_y + height - 1 - n,
        }
    }
}

/// `JigsawStructure`'s `start_height`, which is a `HeightProvider`.
///
/// Only two forms occur in 26.2 — a bare anchor (`ConstantHeight`) and
/// `uniform`. Everything else is refused by name rather than approximated:
/// a `trapezoid` start height silently read as a constant would put a whole
/// structure family at one y, which reads as a placement bug.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartHeight {
    Constant(VerticalAnchor),
    Uniform {
        min_inclusive: VerticalAnchor,
        max_inclusive: VerticalAnchor,
    },
}

impl StartHeight {
    /// # Errors
    /// If the provider type is one this port has not read the draw order of.
    pub fn parse(value: &Value) -> Result<Self, Malformed> {
        // `HeightProvider`'s codec is `either(VerticalAnchor.CODEC, ...)`, so
        // an object with no `type` is a constant written as a bare anchor —
        // which is how all nine of the non-uniform ones are written.
        let Some(kind) = value.get("type").and_then(Value::as_str) else {
            return VerticalAnchor::parse(value).map(Self::Constant);
        };
        match kind {
            "minecraft:constant" => {
                let value = value
                    .get("value")
                    .ok_or_else(|| Malformed("constant height has no `value`".to_string()))?;
                VerticalAnchor::parse(value).map(Self::Constant)
            }
            "minecraft:uniform" => Ok(Self::Uniform {
                min_inclusive: VerticalAnchor::parse(
                    value
                        .get("min_inclusive")
                        .ok_or_else(|| Malformed("uniform height has no min".to_string()))?,
                )?,
                max_inclusive: VerticalAnchor::parse(
                    value
                        .get("max_inclusive")
                        .ok_or_else(|| Malformed("uniform height has no max".to_string()))?,
                )?,
            }),
            other => bad(format!("unported height provider `{other}`")),
        }
    }
}

/// `TerrainAdjustment`.
///
/// **Not a decoration concern.** These feed `Beardifier`, which is a term in
/// the *density function*, so honouring them is a noise-stage change and not
/// something the structure writer can do on its own. Parsed here so the
/// structure's declaration is available to whoever wires that up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerrainAdaptation {
    None,
    Bury,
    BeardThin,
    BeardBox,
    Encapsulate,
}

impl TerrainAdaptation {
    /// # Errors
    /// If the string names no known adjustment.
    pub fn parse(name: &str) -> Result<Self, Malformed> {
        match name {
            "none" => Ok(Self::None),
            "bury" => Ok(Self::Bury),
            "beard_thin" => Ok(Self::BeardThin),
            "beard_box" => Ok(Self::BeardBox),
            "encapsulate" => Ok(Self::Encapsulate),
            other => bad(format!("unknown terrain adaptation `{other}`")),
        }
    }
}

/// `LiquidSettings`. The default is `apply_waterlogging`; `trial_chambers` is
/// the only structure in 26.2 that overrides it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiquidSettings {
    ApplyWaterlogging,
    IgnoreWaterlogging,
}

impl LiquidSettings {
    /// # Errors
    /// If the string names neither setting.
    pub fn parse(name: &str) -> Result<Self, Malformed> {
        match name {
            "apply_waterlogging" => Ok(Self::ApplyWaterlogging),
            "ignore_waterlogging" => Ok(Self::IgnoreWaterlogging),
            other => bad(format!("unknown liquid settings `{other}`")),
        }
    }
}

/// A `minecraft:jigsaw` structure definition.
///
/// `spawn_overrides` is deliberately not read: it decides mob spawning, which
/// this port does not do, and carrying it would be a field that is never
/// compared against anything.
#[derive(Clone, Debug, PartialEq)]
pub struct JigsawStructure {
    /// A biome tag (`#minecraft:has_structure/village_plains`) or a list.
    pub biomes: String,
    /// Which of the eleven generation steps the structure writes in.
    pub step: String,
    pub terrain_adaptation: TerrainAdaptation,
    pub start_pool: String,
    /// The jigsaw block name the start piece must be anchored by. Ancient
    /// city is the only structure that names one.
    pub start_jigsaw_name: Option<String>,
    /// How many jigsaw generations deep the walk may go.
    pub size: i32,
    pub start_height: StartHeight,
    /// When set, the start piece is dropped onto this heightmap instead of
    /// standing at `start_height`.
    pub project_start_to_heightmap: Option<HeightmapKind>,
    /// The radius, in blocks, beyond which a piece is refused outright.
    pub max_distance_from_center: i32,
    /// `use_expansion_hack` — vanilla's own name for it. It re-runs the
    /// placement of a rigid piece's children against the *unbounded* box, and
    /// it is on for exactly the five villages and the pillager outpost.
    pub use_expansion_hack: bool,
    pub dimension_padding: i32,
    pub liquid_settings: LiquidSettings,
    /// `pool_aliases` — the virtual pool names this structure resolves through
    /// before opening one. Empty for every structure but `trial_chambers`,
    /// which **cannot generate without them**: four of its spawner pools have
    /// no file on disk at all. See [`crate::structure::pool_alias`].
    pub pool_aliases: Vec<crate::structure::pool_alias::Binding>,
}

impl JigsawStructure {
    /// # Errors
    /// If the definition is not a jigsaw, or a required field is missing or
    /// unrecognised.
    #[allow(clippy::too_many_lines, reason = "one field per line, and no logic")]
    pub fn parse(value: &Value) -> Result<Self, Malformed> {
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| Malformed("structure has no `type`".to_string()))?;
        if kind != "minecraft:jigsaw" {
            return bad(format!("`{kind}` is not a jigsaw structure"));
        }
        let string = |key: &str| -> Result<String, Malformed> {
            value
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| Malformed(format!("jigsaw structure has no `{key}`")))
        };
        let int = |key: &str| -> Result<i32, Malformed> {
            let v = value
                .get(key)
                .and_then(Value::as_i64)
                .ok_or_else(|| Malformed(format!("jigsaw structure has no `{key}`")))?;
            i32::try_from(v).map_err(|_| Malformed(format!("`{key}` = {v} is not an i32")))
        };
        // `biomes` is either a tag string or an inline list. Only the tag form
        // occurs, and a list would need the biome registry to resolve, so the
        // list form is refused rather than half-read.
        let biomes = match value.get("biomes") {
            Some(Value::String(tag)) => tag.clone(),
            Some(other) => return bad(format!("inline biome list is unported: {other}")),
            None => return bad("jigsaw structure has no `biomes`".to_string()),
        };
        Ok(Self {
            biomes,
            step: string("step")?,
            // Absent means `none`: `TerrainAdjustment.NONE` is the codec's
            // default, and bastion_remnant is the one that leaves it out.
            terrain_adaptation: match value.get("terrain_adaptation") {
                None | Some(Value::Null) => TerrainAdaptation::None,
                Some(Value::String(name)) => TerrainAdaptation::parse(name)?,
                Some(other) => return bad(format!("terrain_adaptation is not a string: {other}")),
            },
            start_pool: string("start_pool")?,
            start_jigsaw_name: value
                .get("start_jigsaw_name")
                .and_then(Value::as_str)
                .map(str::to_string),
            size: int("size")?,
            start_height: StartHeight::parse(
                value.get("start_height").ok_or_else(|| {
                    Malformed("jigsaw structure has no `start_height`".to_string())
                })?,
            )?,
            project_start_to_heightmap: match value
                .get("project_start_to_heightmap")
                .and_then(Value::as_str)
            {
                None => None,
                Some(name) => Some(
                    HeightmapKind::parse(name)
                        .map_err(|n| Malformed(format!("unknown heightmap `{n}`")))?,
                ),
            },
            max_distance_from_center: int("max_distance_from_center")?,
            use_expansion_hack: value
                .get("use_expansion_hack")
                .and_then(Value::as_bool)
                .ok_or_else(|| {
                    Malformed("jigsaw structure has no `use_expansion_hack`".to_string())
                })?,
            // `DimensionPadding.ZERO` is the default; only trial_chambers
            // names one, and it names the symmetric form.
            dimension_padding: match value.get("dimension_padding") {
                None | Some(Value::Null) => 0,
                Some(Value::Number(n)) => {
                    let v = n.as_i64().ok_or_else(|| {
                        Malformed(format!("dimension_padding {n} is not an integer"))
                    })?;
                    i32::try_from(v)
                        .map_err(|_| Malformed(format!("dimension_padding {v} is not an i32")))?
                }
                Some(other) => {
                    return bad(format!("asymmetric dimension_padding is unported: {other}"))
                }
            },
            liquid_settings: match value.get("liquid_settings").and_then(Value::as_str) {
                None => LiquidSettings::ApplyWaterlogging,
                Some(name) => LiquidSettings::parse(name)?,
            },
            pool_aliases: crate::structure::pool_alias::parse_list(value.get("pool_aliases"))?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(text: &str) -> Value {
        serde_json::from_str(text).expect("test json")
    }

    #[test]
    fn a_named_processor_and_an_inline_one_are_both_kept() {
        let named = PoolElement::parse(&json(
            r#"{"element_type": "minecraft:legacy_single_pool_element",
                "location": "minecraft:village/plains/town_centers/plains_fountain_01",
                "processors": "minecraft:mossify_20_percent", "projection": "rigid"}"#,
        ))
        .expect("parses");
        let PoolElement::Single {
            processors, legacy, ..
        } = &named
        else {
            panic!("expected a single element");
        };
        assert!(*legacy, "legacy_single is flagged, not aliased");
        assert_eq!(
            processors.as_ref(),
            Some(&Processors::Named(
                "minecraft:mossify_20_percent".to_string()
            ))
        );

        let inline = PoolElement::parse(&json(
            r#"{"element_type": "minecraft:single_pool_element",
                "location": "minecraft:village/plains/houses/plains_small_house_1",
                "processors": {"processors": []}, "projection": "terrain_matching"}"#,
        ))
        .expect("parses");
        let PoolElement::Single {
            processors,
            legacy,
            projection,
            ..
        } = &inline
        else {
            panic!("expected a single element");
        };
        assert!(!*legacy);
        assert_eq!(*projection, Projection::TerrainMatching);
        assert!(matches!(processors, Some(Processors::Inline(_))));
    }

    #[test]
    fn an_empty_element_needs_no_projection_and_a_single_one_does() {
        assert_eq!(
            PoolElement::parse(&json(r#"{"element_type": "minecraft:empty_pool_element"}"#)),
            Ok(PoolElement::Empty)
        );
        let missing = PoolElement::parse(&json(
            r#"{"element_type": "minecraft:single_pool_element", "location": "x"}"#,
        ));
        assert_eq!(
            missing,
            Err(Malformed(
                "`minecraft:single_pool_element` has no `projection`".to_string()
            ))
        );
    }

    #[test]
    fn a_pool_sums_its_weights_and_keeps_its_fallback() {
        let pool = TemplatePool::parse(&json(
            r#"{"elements": [
                 {"element": {"element_type": "minecraft:empty_pool_element"}, "weight": 3},
                 {"element": {"element_type": "minecraft:empty_pool_element"}, "weight": 50}],
               "fallback": "minecraft:empty"}"#,
        ))
        .expect("parses");
        assert_eq!(pool.total_weight(), 53);
        assert_eq!(pool.fallback, "minecraft:empty");
    }

    #[test]
    fn a_zero_weight_is_refused_rather_than_silently_dropped() {
        let pool = TemplatePool::parse(&json(
            r#"{"elements": [
                 {"element": {"element_type": "minecraft:empty_pool_element"}, "weight": 0}],
               "fallback": "minecraft:empty"}"#,
        ));
        assert_eq!(
            pool,
            Err(Malformed("pool entry weight 0 is not positive".to_string()))
        );
    }

    #[test]
    fn below_top_is_one_less_than_the_height() {
        // minY -64, height 384 -> top y is 319, not 320.
        assert_eq!(VerticalAnchor::BelowTop(0).resolve(-64, 384), 319);
        assert_eq!(VerticalAnchor::AboveBottom(0).resolve(-64, 384), -64);
        assert_eq!(VerticalAnchor::Absolute(7).resolve(-64, 384), 7);
    }

    #[test]
    fn a_bare_anchor_is_a_constant_height() {
        assert_eq!(
            StartHeight::parse(&json(r#"{"absolute": -27}"#)),
            Ok(StartHeight::Constant(VerticalAnchor::Absolute(-27)))
        );
        assert_eq!(
            StartHeight::parse(&json(
                r#"{"type": "minecraft:uniform",
                    "min_inclusive": {"absolute": -40},
                    "max_inclusive": {"absolute": -20}}"#
            )),
            Ok(StartHeight::Uniform {
                min_inclusive: VerticalAnchor::Absolute(-40),
                max_inclusive: VerticalAnchor::Absolute(-20),
            })
        );
    }

    fn counting(
        seed: i64,
    ) -> crate::rng::worldgen_random::WorldgenRandom<crate::rng::legacy::LegacyRandomSource> {
        crate::rng::worldgen_random::WorldgenRandom::new(
            crate::rng::legacy::LegacyRandomSource::new(seed),
        )
    }

    /// The four-at-50 / four-at-1 shape of a village town-centre pool.
    fn town_centres() -> TemplatePool {
        let entry = |weight: i32| {
            format!(
                r#"{{"element": {{"element_type": "minecraft:legacy_single_pool_element",
                    "location": "minecraft:village/plains/town_centers/p{weight}",
                    "processors": "minecraft:empty", "projection": "rigid"}},
                    "weight": {weight}}}"#
            )
        };
        let body = format!(
            r#"{{"elements": [{}, {}, {}, {}, {}, {}, {}, {}], "fallback": "minecraft:empty"}}"#,
            entry(50),
            entry(50),
            entry(50),
            entry(50),
            entry(1),
            entry(1),
            entry(1),
            entry(1)
        );
        TemplatePool::parse(&json(&body)).expect("parses")
    }

    #[test]
    fn the_expanded_list_is_the_draw_and_not_a_cumulative_scan() {
        let pool = town_centres();
        assert_eq!(pool.elements.len(), 8);
        assert_eq!(pool.total_weight(), 204);
        let expanded = pool.expanded();
        assert_eq!(expanded.len(), 204, "one slot per weight, not per entry");
        // File order, each entry contiguous: 50 of entry 0, then 50 of 1...
        assert_eq!(expanded[0], 0);
        assert_eq!(expanded[49], 0);
        assert_eq!(expanded[50], 1);
        assert_eq!(expanded[199], 3);
        assert_eq!(expanded[200], 4);
        assert_eq!(expanded[203], 7);
    }

    #[test]
    fn one_draw_to_pick_and_size_minus_one_to_shuffle() {
        let pool = town_centres();

        let mut random = counting(1234);
        crate::rng::draws::arm();
        let _ = pool.random_template(&mut random);
        assert_eq!(
            crate::rng::draws::take(),
            1,
            "getRandomTemplate is one draw"
        );

        let mut random = counting(1234);
        crate::rng::draws::arm();
        let shuffled = pool.shuffled_templates(&mut random);
        assert_eq!(
            crate::rng::draws::take(),
            203,
            "Util.shuffle spends size - 1 draws over the EXPANDED list"
        );
        assert_eq!(shuffled.len(), 204);
    }

    #[test]
    fn an_empty_pool_answers_empty_without_drawing() {
        let pool = TemplatePool::parse(&json(r#"{"elements": [], "fallback": "minecraft:empty"}"#))
            .expect("parses");
        let mut random = counting(1);
        crate::rng::draws::arm();
        assert_eq!(pool.random_template(&mut random), PoolElement::Empty);
        assert_eq!(
            crate::rng::draws::take(),
            0,
            "the size check comes before the draw"
        );
    }

    #[test]
    fn a_non_jigsaw_structure_is_refused_by_name() {
        assert_eq!(
            JigsawStructure::parse(&json(r#"{"type": "minecraft:shipwreck"}"#)),
            Err(Malformed(
                "`minecraft:shipwreck` is not a jigsaw structure".to_string()
            ))
        );
    }

    #[test]
    fn a_missing_terrain_adaptation_is_none_not_an_error() {
        let bastion = JigsawStructure::parse(&json(
            r##"{"type": "minecraft:jigsaw", "biomes": "#minecraft:has_structure/bastion_remnant",
                "step": "surface_structures", "start_pool": "minecraft:bastion/starts",
                "size": 6, "start_height": {"absolute": 33},
                "max_distance_from_center": 80, "use_expansion_hack": false}"##,
        ))
        .expect("parses");
        assert_eq!(bastion.terrain_adaptation, TerrainAdaptation::None);
        assert_eq!(bastion.liquid_settings, LiquidSettings::ApplyWaterlogging);
        assert_eq!(bastion.dimension_padding, 0);
        assert_eq!(bastion.project_start_to_heightmap, None);
    }
}
