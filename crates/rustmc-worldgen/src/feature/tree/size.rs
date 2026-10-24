//! `FeatureSize` — how wide a tree needs its column to be, level by level.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `FeatureSize`, `TwoLayersFeatureSize`, `ThreeLayersFeatureSize`.
//!
//! The size is a *radius*: `getMaxFreeTreeHeight` tests a square of side
//! `2 * size + 1` at each level, so a size of 0 is one column and a size of 1
//! is three by three. Nothing draws.
//!
//! `min_clipped_height` is the other half of the contract, and it is what
//! makes a tree under an overhang grow short rather than not at all. Vanilla
//! spells it `OptionalInt`, absent by default — absent means "no clipping",
//! which refuses, and is not the same as zero, which accepts anything.

/// One of vanilla's two `FeatureSize`s.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FeatureSize {
    /// `two_layers_feature_size`. Defaults: limit 1, lower 0, upper 1.
    TwoLayers {
        limit: i32,
        lower_size: i32,
        upper_size: i32,
        min_clipped_height: Option<i32>,
    },
    /// `three_layers_feature_size`. Defaults: limit 1, upper_limit 1,
    /// lower 0, middle 1, upper 1.
    ThreeLayers {
        limit: i32,
        upper_limit: i32,
        lower_size: i32,
        middle_size: i32,
        upper_size: i32,
        min_clipped_height: Option<i32>,
    },
}

impl FeatureSize {
    /// # Errors
    /// If the type is unknown.
    pub fn parse(value: &serde_json::Value) -> Result<Self, String> {
        let kind = value
            .get("type")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| format!("feature size has no `type`: {value}"))?;
        let int = |name: &str, default: i32| -> i32 {
            value
                .get(name)
                .and_then(serde_json::Value::as_i64)
                .and_then(|v| i32::try_from(v).ok())
                .unwrap_or(default)
        };
        let min_clipped_height = value
            .get("min_clipped_height")
            .and_then(serde_json::Value::as_i64)
            .and_then(|v| i32::try_from(v).ok());
        match kind {
            "minecraft:two_layers_feature_size" => Ok(Self::TwoLayers {
                limit: int("limit", 1),
                lower_size: int("lower_size", 0),
                upper_size: int("upper_size", 1),
                min_clipped_height,
            }),
            "minecraft:three_layers_feature_size" => Ok(Self::ThreeLayers {
                limit: int("limit", 1),
                upper_limit: int("upper_limit", 1),
                lower_size: int("lower_size", 0),
                middle_size: int("middle_size", 1),
                upper_size: int("upper_size", 1),
                min_clipped_height,
            }),
            other => Err(format!("unimplemented feature size `{other}`")),
        }
    }

    /// `FeatureSize#getSizeAtHeight(treeHeight, y)`.
    #[must_use]
    pub fn size_at_height(&self, tree_height: i32, y: i32) -> i32 {
        match self {
            Self::TwoLayers {
                limit,
                lower_size,
                upper_size,
                ..
            } => {
                if y < *limit {
                    *lower_size
                } else {
                    *upper_size
                }
            }
            // The middle band is measured from the *top*: `y >= treeHeight -
            // upperLimit` is the upper size, whatever the tree's height.
            Self::ThreeLayers {
                limit,
                upper_limit,
                lower_size,
                middle_size,
                upper_size,
                ..
            } => {
                if y < *limit {
                    *lower_size
                } else if y >= tree_height - *upper_limit {
                    *upper_size
                } else {
                    *middle_size
                }
            }
        }
    }

    /// `FeatureSize#minClippedHeight`. `None` refuses a clipped tree.
    #[must_use]
    pub fn min_clipped_height(&self) -> Option<i32> {
        match self {
            Self::TwoLayers {
                min_clipped_height, ..
            }
            | Self::ThreeLayers {
                min_clipped_height, ..
            } => *min_clipped_height,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> FeatureSize {
        FeatureSize::parse(&serde_json::from_str(json).expect("json")).expect("parse")
    }

    /// The defaults are what most of the datapack relies on: an absent
    /// `two_layers_feature_size` is limit 1, lower 0, upper 1 — one column at
    /// the base and three by three above it.
    #[test]
    fn an_empty_two_layers_size_is_one_column_then_three_by_three() {
        let size = parse(r#"{"type":"minecraft:two_layers_feature_size"}"#);
        assert_eq!(size.size_at_height(6, 0), 0);
        assert_eq!(size.size_at_height(6, 1), 1);
        assert_eq!(size.size_at_height(6, 5), 1);
        assert_eq!(size.min_clipped_height(), None);
    }

    /// The three-layer form measures its upper band from the top of the tree,
    /// so the same y is a different band in a taller one.
    #[test]
    fn the_upper_band_is_measured_from_the_top() {
        let size = parse(
            r#"{"type":"minecraft:three_layers_feature_size",
                "limit":1,"upper_limit":1,
                "lower_size":0,"middle_size":1,"upper_size":2}"#,
        );
        assert_eq!(size.size_at_height(10, 0), 0, "below the limit");
        assert_eq!(size.size_at_height(10, 5), 1, "middle");
        assert_eq!(
            size.size_at_height(10, 9),
            2,
            "the top band of a ten-high tree"
        );
        assert_eq!(size.size_at_height(6, 5), 2, "and of a six-high one");
    }

    /// Absent is not zero. A tree with no `min_clipped_height` is refused
    /// when it does not fit; one with zero is accepted at any height.
    #[test]
    fn an_absent_min_clipped_height_is_not_zero() {
        assert_eq!(
            parse(r#"{"type":"minecraft:two_layers_feature_size"}"#).min_clipped_height(),
            None
        );
        assert_eq!(
            parse(r#"{"type":"minecraft:two_layers_feature_size","min_clipped_height":0}"#)
                .min_clipped_height(),
            Some(0)
        );
    }

    #[test]
    fn an_unknown_size_is_refused() {
        assert!(FeatureSize::parse(
            &serde_json::from_str(r#"{"type":"minecraft:four_layers"}"#).expect("json")
        )
        .is_err());
    }
}
