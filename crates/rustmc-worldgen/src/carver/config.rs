//! Configured carvers, read from datapack JSON.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `net.minecraft.world.level.levelgen.carver.{CarverConfiguration,
//! CaveCarverConfiguration, CanyonCarverConfiguration, ConfiguredWorldCarver}`.
//!
//! Same argument as the surface rules: these are datapack content, so a
//! server has to parse them anyway. Reading them is the normal path.
//!
//! # The field order is a stream order
//!
//! Every provider here is sampled from the carver's own random stream, in
//! the order the carver's `carve` method reads them — not the order they
//! appear in the JSON. So this type is a bag of parsed values and the
//! *sequence* lives in [`super::cave`] and [`super::canyon`], where it can be
//! read against the reference line by line.

use serde_json::Value;

use super::providers::{parse_anchor, FloatProvider, HeightProvider};
use crate::surface::rule::VerticalAnchor;

/// The fields every carver configuration has.
#[derive(Clone, Debug, PartialEq)]
pub struct CarverConfig {
    /// `isStartChunk` compares a `nextFloat()` against this.
    pub probability: f32,
    pub y: HeightProvider,
    pub y_scale: FloatProvider,
    pub lava_level: VerticalAnchor,
    /// The block tag a carver may replace, with its leading `#`.
    pub replaceable: String,
}

impl CarverConfig {
    fn parse(value: &Value) -> Result<Self, String> {
        Ok(Self {
            probability: value
                .get("probability")
                .and_then(Value::as_f64)
                .map(|v| {
                    #[allow(clippy::cast_possible_truncation)]
                    {
                        v as f32
                    }
                })
                .ok_or_else(|| "carver has no numeric `probability`".to_string())?,
            y: HeightProvider::parse(
                value
                    .get("y")
                    .ok_or_else(|| "carver has no `y`".to_string())?,
            )?,
            y_scale: FloatProvider::parse(
                value
                    .get("yScale")
                    .ok_or_else(|| "carver has no `yScale`".to_string())?,
            )?,
            lava_level: parse_anchor(
                value
                    .get("lava_level")
                    .ok_or_else(|| "carver has no `lava_level`".to_string())?,
            )?,
            replaceable: value
                .get("replaceable")
                .and_then(Value::as_str)
                .ok_or_else(|| "carver has no `replaceable`".to_string())?
                .to_string(),
        })
    }
}

/// `CaveCarverConfiguration`.
#[derive(Clone, Debug, PartialEq)]
pub struct CaveConfig {
    pub base: CarverConfig,
    pub horizontal_radius_multiplier: FloatProvider,
    pub vertical_radius_multiplier: FloatProvider,
    pub floor_level: FloatProvider,
}

/// `CanyonCarverConfiguration.CanyonShapeConfiguration`.
#[derive(Clone, Debug, PartialEq)]
pub struct CanyonShape {
    pub distance_factor: FloatProvider,
    pub thickness: FloatProvider,
    /// Plain floats, not providers — they scale a radius rather than being
    /// drawn.
    pub vertical_radius_default_factor: f32,
    pub vertical_radius_center_factor: f32,
    pub horizontal_radius_factor: FloatProvider,
    /// `nextInt(widthSmoothness) == 0` decides when the width factor is
    /// redrawn while building the per-height table, so a wrong value here
    /// changes both the shape *and* the draw count.
    pub width_smoothness: i32,
}

/// `CanyonCarverConfiguration`.
#[derive(Clone, Debug, PartialEq)]
pub struct CanyonConfig {
    pub base: CarverConfig,
    pub vertical_rotation: FloatProvider,
    pub shape: CanyonShape,
}

/// A carver plus its configuration — `ConfiguredWorldCarver`.
#[derive(Clone, Debug, PartialEq)]
pub enum ConfiguredCarver {
    /// `minecraft:cave` and `minecraft:cave_extra_underground`.
    Cave(CaveConfig),
    /// `minecraft:nether_cave` — the same algorithm with a different
    /// thickness curve and no floor level.
    NetherCave(CaveConfig),
    Canyon(CanyonConfig),
}

impl ConfiguredCarver {
    /// The tag of blocks this carver may replace.
    #[must_use]
    pub fn replaceable(&self) -> &str {
        match self {
            Self::Cave(c) | Self::NetherCave(c) => &c.base.replaceable,
            Self::Canyon(c) => &c.base.replaceable,
        }
    }

    /// The shared configuration.
    #[must_use]
    pub const fn base(&self) -> &CarverConfig {
        match self {
            Self::Cave(c) | Self::NetherCave(c) => &c.base,
            Self::Canyon(c) => &c.base,
        }
    }

    /// Parse a `worldgen/configured_carver/*.json`.
    ///
    /// # Errors
    /// If the type is unknown or a field is missing. Unknown types are
    /// rejected rather than skipped: a carver silently dropped is a world
    /// with no ravines, which looks like a seed rather than a bug.
    pub fn parse(value: &Value) -> Result<Self, String> {
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| "configured carver has no `type`".to_string())?;
        // The type is checked before any field is read, matching the
        // dispatch codec — otherwise an unknown carver reports a missing
        // `probability` and the actual problem is two lines further up.
        if !matches!(
            kind,
            "minecraft:cave" | "minecraft:nether_cave" | "minecraft:canyon"
        ) {
            return Err(format!("unknown carver type `{kind}`"));
        }
        let config = value
            .get("config")
            .ok_or_else(|| format!("configured carver {kind} has no `config`"))?;
        let base = CarverConfig::parse(config)?;
        let float = |name: &str| -> Result<FloatProvider, String> {
            FloatProvider::parse(
                config
                    .get(name)
                    .ok_or_else(|| format!("{kind} has no `{name}`"))?,
            )
        };

        match kind {
            "minecraft:cave" | "minecraft:nether_cave" => {
                let cave = CaveConfig {
                    base,
                    horizontal_radius_multiplier: float("horizontal_radius_multiplier")?,
                    vertical_radius_multiplier: float("vertical_radius_multiplier")?,
                    floor_level: float("floor_level")?,
                };
                Ok(if kind == "minecraft:cave" {
                    Self::Cave(cave)
                } else {
                    Self::NetherCave(cave)
                })
            }
            "minecraft:canyon" => {
                let shape = config
                    .get("shape")
                    .ok_or_else(|| "canyon has no `shape`".to_string())?;
                let shape_float = |name: &str| -> Result<FloatProvider, String> {
                    FloatProvider::parse(
                        shape
                            .get(name)
                            .ok_or_else(|| format!("canyon shape has no `{name}`"))?,
                    )
                };
                let plain = |name: &str| -> Result<f32, String> {
                    shape
                        .get(name)
                        .and_then(Value::as_f64)
                        .map(|v| {
                            #[allow(clippy::cast_possible_truncation)]
                            {
                                v as f32
                            }
                        })
                        .ok_or_else(|| format!("canyon shape has no numeric `{name}`"))
                };
                Ok(Self::Canyon(CanyonConfig {
                    base,
                    vertical_rotation: float("vertical_rotation")?,
                    shape: CanyonShape {
                        distance_factor: shape_float("distance_factor")?,
                        thickness: shape_float("thickness")?,
                        vertical_radius_default_factor: plain("vertical_radius_default_factor")?,
                        vertical_radius_center_factor: plain("vertical_radius_center_factor")?,
                        horizontal_radius_factor: shape_float("horizontal_radius_factor")?,
                        width_smoothness: shape
                            .get("width_smoothness")
                            .and_then(Value::as_i64)
                            .and_then(|v| i32::try_from(v).ok())
                            .ok_or_else(|| "canyon shape has no `width_smoothness`".to_string())?,
                    },
                }))
            }
            // Unreachable: the type was checked above.
            other => Err(format!("unknown carver type `{other}`")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DATA: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../oracle-work/vanilla-26.2/generated-data/data/minecraft/worldgen/configured_carver"
    );

    fn load(name: &str) -> Option<ConfiguredCarver> {
        let path = std::path::Path::new(DATA).join(format!("{name}.json"));
        if !rustmc_data::fixture::available(&path, "carver extract", "ci/vanilla-server.sh data") {
            return None;
        }
        let json: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("readable")).expect("json");
        Some(ConfiguredCarver::parse(&json).unwrap_or_else(|e| panic!("{name}: {e}")))
    }

    #[test]
    fn the_real_configured_carvers_parse() {
        // Against the generated data, not fixtures — a fixture proves the
        // parser reads what this file thinks vanilla writes.
        let Some(cave) = load("cave") else { return };
        let ConfiguredCarver::Cave(cave) = cave else {
            panic!("cave.json is not a cave carver");
        };
        assert!((cave.base.probability - 0.15).abs() < f32::EPSILON);
        assert_eq!(cave.base.lava_level, VerticalAnchor::AboveBottom(8));
        assert_eq!(
            cave.base.replaceable,
            "#minecraft:overworld_carver_replaceables"
        );
        assert_eq!(
            cave.horizontal_radius_multiplier,
            FloatProvider::Uniform {
                min: 0.7,
                max_exclusive: 1.4
            }
        );
        assert_eq!(
            cave.floor_level,
            FloatProvider::Uniform {
                min: -1.0,
                max_exclusive: -0.4
            }
        );

        let Some(ConfiguredCarver::Canyon(canyon)) = load("canyon") else {
            panic!("canyon.json is not a canyon carver");
        };
        assert!((canyon.base.probability - 0.01).abs() < f32::EPSILON);
        // yScale is a bare number here and a provider in the cave config.
        assert_eq!(canyon.base.y_scale, FloatProvider::Constant(3.0));
        assert_eq!(canyon.shape.width_smoothness, 3);
        assert_eq!(
            canyon.shape.thickness,
            FloatProvider::Trapezoid {
                min: 0.0,
                max: 6.0,
                plateau: 2.0
            }
        );
        assert!(canyon.shape.vertical_radius_center_factor.abs() < f32::EPSILON);

        assert!(matches!(
            load("nether_cave"),
            Some(ConfiguredCarver::NetherCave(_))
        ));
        assert!(matches!(
            load("cave_extra_underground"),
            Some(ConfiguredCarver::Cave(_))
        ));
    }

    #[test]
    fn an_unknown_carver_type_is_rejected() {
        // A dropped carver is a world with no ravines, which reads as a seed
        // rather than as a bug.
        let json = serde_json::json!({"type": "minecraft:spaghetti", "config": {}});
        assert!(ConfiguredCarver::parse(&json)
            .unwrap_err()
            .contains("spaghetti"));
    }

    #[test]
    fn a_missing_field_names_itself() {
        let json = serde_json::json!({
            "type": "minecraft:cave",
            "config": {"probability": 0.15}
        });
        let error = ConfiguredCarver::parse(&json).unwrap_err();
        assert!(error.contains('y'), "{error}");
    }
}
