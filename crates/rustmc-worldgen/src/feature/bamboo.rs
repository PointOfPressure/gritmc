//! `minecraft:bamboo` — `BambooFeature`, one stalk per placement.
//!
//! # Why this is a jungle-parity item and not a bamboo item
//!
//! `bamboo_light` is `minecraft:bamboo` at probability 0.0, and in
//! `minecraft:jungle` it sits at step 9 **immediately before
//! `minecraft:trees_jungle`**:
//!
//! ```text
//!   glow_lichen, bamboo_light, trees_jungle, flower_warm, patch_grass_jungle, ...
//! ```
//!
//! `minecraft:sparse_jungle` has no bamboo entry at all and is otherwise the
//! same list. That difference is the whole of why `sparse_jungle` sits at
//! 1.13x its floor in window J while `jungle` sat at 3.94x with an identical
//! tree stack: **an unported feature is refused, a refusal spends no
//! randomness, and everything decorated after it in that chunk is displaced.**
//! Porting `cocoa` and then `mega_jungle_trunk_placer` moved the jungle
//! residual the wrong way both times because both are *downstream* of this
//! refusal — the trees were being placed correctly into a stream that had
//! already lost its place.
//!
//! # The feature
//!
//! ```java
//!   if (!level.isEmptyBlock(pos)) return false;              // count stays 0
//!   if (BAMBOO.defaultBlockState().canSurvive(level, pos)) {
//!       int height = random.nextInt(12) + 5;
//!       if (random.nextFloat() < config.probability) { ...podzol disc... }
//!       for (int i = 0; i < height && level.isEmptyBlock(pos); i++) {
//!           level.setBlock(pos, BAMBOO_TRUNK, 2);
//!           pos.move(UP, 1);
//!       }
//!       if (pos.getY() - origin.getY() >= 3) {
//!           level.setBlock(pos,                BAMBOO_FINAL_LARGE, 2);
//!           level.setBlock(pos.move(DOWN, 1),  BAMBOO_TOP_LARGE,   2);
//!           level.setBlock(pos.move(DOWN, 1),  BAMBOO_TOP_SMALL,   2);
//!       }
//!   }
//!   count++;
//! ```
//!
//! Three draw-order facts, in the order they bite:
//!
//!   * **the two draws are unconditional once bamboo can survive.** The
//!     `nextInt(12)` happens before the probability test and the `nextFloat`
//!     happens whatever the probability is — so `bamboo_no_podzol`, whose
//!     probability is a literal `0.0` and which can therefore never place a
//!     single podzol block, still spends **two values per attempt**. A port
//!     that skipped the feature because "probability 0 means it does nothing"
//!     would be right about the blocks and wrong about the stream, which is
//!     the more expensive half.
//!   * **the height loop takes no draws at all**, so a stalk blocked at its
//!     second block costs exactly what a sixteen-block stalk costs.
//!   * **`count++` is outside the `canSurvive` branch and inside the
//!     `isEmptyBlock` one.** The return value is therefore "the origin was
//!     air", not "a stalk was placed", and it is what the enclosing
//!     `random_selector`/`sequence` sees.
//!
//! # The states
//!
//! `BAMBOO_TRUNK` is `bamboo[age=1,leaves=none,stage=0]`, and the three caps
//! are that state with `leaves`/`stage` overridden — written out here rather
//! than derived, because `Blocks.BAMBOO.defaultBlockState()` is `age=0` and
//! the feature's `age=1` is the difference between a stalk that grows and one
//! that does not.

use super::configured::FeatureWorld;
use super::placement::HeightmapKind;
use crate::rng::RandomSource;

/// A refusal, in the shape every other feature module uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeedsRules(pub String);

/// `ProbabilityFeatureConfiguration`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BambooConfig {
    pub probability: f32,
}

impl BambooConfig {
    /// # Errors
    /// If `probability` is missing.
    pub fn parse(config: &serde_json::Value) -> Result<Self, String> {
        Ok(Self {
            #[expect(clippy::cast_possible_truncation, reason = "the codec is Codec.FLOAT")]
            probability: config
                .get("probability")
                .and_then(serde_json::Value::as_f64)
                .ok_or("bamboo: no `probability`")? as f32,
        })
    }
}

const TRUNK: &str = "minecraft:bamboo[age=1,leaves=none,stage=0]";
const FINAL_LARGE: &str = "minecraft:bamboo[age=1,leaves=large,stage=1]";
const TOP_LARGE: &str = "minecraft:bamboo[age=1,leaves=large,stage=0]";
const TOP_SMALL: &str = "minecraft:bamboo[age=1,leaves=small,stage=0]";

fn plain(state: &str) -> &str {
    state.split('[').next().unwrap_or(state)
}

/// `BambooFeature#place`.
///
/// # Errors
/// If the world cannot answer whether a position is air.
pub fn place<W: FeatureWorld + ?Sized>(
    world: &mut W,
    config: &BambooConfig,
    random: &mut dyn RandomSource,
    origin: (i32, i32, i32),
) -> Result<bool, NeedsRules> {
    let tags = world.block_tags().clone();
    let is_air = |w: &mut W, pos: (i32, i32, i32)| -> bool {
        plain(
            &<W as crate::feature::block_predicate::PredicateWorld>::block_at(
                w, pos.0, pos.1, pos.2,
            ),
        )
        .ends_with("air")
    };

    if !is_air(world, origin) {
        return Ok(false);
    }

    // `BambooStalkBlock.canSurvive` — the block BELOW is in
    // `#minecraft:supports_bamboo`. Nothing about the position itself.
    let below = <W as crate::feature::block_predicate::PredicateWorld>::block_at(
        world,
        origin.0,
        origin.1 - 1,
        origin.2,
    );
    if !tags.contains("minecraft:supports_bamboo", plain(&below)) {
        // `count++` still runs: the origin was air, so the feature reports
        // success even though it placed nothing and drew nothing.
        return Ok(true);
    }

    let height = random.next_i32_bound(12) + 5;
    // Unconditional. `bamboo_no_podzol` is probability 0.0 and still spends
    // this value.
    let roll = random.next_f32();
    if roll < config.probability {
        let r = random.next_i32_bound(4) + 1;
        for x in origin.0 - r..=origin.0 + r {
            for z in origin.2 - r..=origin.2 + r {
                let (dx, dz) = (x - origin.0, z - origin.2);
                if dx * dx + dz * dz > r * r {
                    continue;
                }
                let y = world.heightmap(HeightmapKind::WorldSurface, x, z) - 1;
                let here = <W as crate::feature::block_predicate::PredicateWorld>::block_at(
                    world, x, y, z,
                );
                if tags.contains("minecraft:beneath_bamboo_podzol_replaceable", plain(&here)) {
                    <W as super::simple_block::SimpleBlockWorld>::set_block(
                        world,
                        x,
                        y,
                        z,
                        "minecraft:podzol[snowy=false]",
                    );
                }
            }
        }
    }

    // The stalk. `i < height && isEmptyBlock(pos)` — the air test is the loop
    // condition, so a blocked column stops without spending anything.
    let mut y = origin.1;
    let mut i = 0;
    while i < height && is_air(world, (origin.0, y, origin.2)) {
        <W as super::simple_block::SimpleBlockWorld>::set_block(
            world, origin.0, y, origin.2, TRUNK,
        );
        y += 1;
        i += 1;
    }

    // `pos` has been left one ABOVE the last trunk block, and the three caps
    // are written from there downward — so the topmost cap sits in the air
    // block the loop stopped on. Getting this off by one turns every stalk
    // into a stalk with no leaves.
    if y - origin.1 >= 3 {
        <W as super::simple_block::SimpleBlockWorld>::set_block(
            world,
            origin.0,
            y,
            origin.2,
            FINAL_LARGE,
        );
        <W as super::simple_block::SimpleBlockWorld>::set_block(
            world,
            origin.0,
            y - 1,
            origin.2,
            TOP_LARGE,
        );
        <W as super::simple_block::SimpleBlockWorld>::set_block(
            world,
            origin.0,
            y - 2,
            origin.2,
            TOP_SMALL,
        );
    }

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `bamboo_no_podzol` is a literal `0.0`, so it can never place a podzol
    /// block — and it still spends `nextInt(12)` and `nextFloat()` on every
    /// attempt. Parsing it as anything but zero would place podzol in a
    /// jungle; treating zero as "skip the feature" would spend two values
    /// fewer than vanilla on every attempt.
    #[test]
    fn the_light_variant_is_probability_zero_and_is_still_a_feature() {
        let light =
            BambooConfig::parse(&serde_json::from_str(r#"{"probability":0.0}"#).expect("json"))
                .expect("parse");
        assert_eq!(light.probability, 0.0);

        let some =
            BambooConfig::parse(&serde_json::from_str(r#"{"probability":0.2}"#).expect("json"))
                .expect("parse");
        assert!((some.probability - 0.2).abs() < f32::EPSILON);

        assert!(BambooConfig::parse(&serde_json::json!({})).is_err());
    }

    /// The four states are `Blocks.BAMBOO.defaultBlockState()` with `age=1`
    /// — not the default `age=0` — and `leaves`/`stage` overridden from
    /// there. `age` is the difference between a stalk that grows and one that
    /// does not, and all four share it.
    #[test]
    fn every_written_state_carries_age_one() {
        for state in [TRUNK, FINAL_LARGE, TOP_LARGE, TOP_SMALL] {
            assert!(state.contains("age=1"), "{state}");
            assert_eq!(plain(state), "minecraft:bamboo");
        }
        assert!(TRUNK.contains("leaves=none") && TRUNK.contains("stage=0"));
        // Only the topmost cap carries stage=1.
        assert!(FINAL_LARGE.contains("leaves=large") && FINAL_LARGE.contains("stage=1"));
        assert!(TOP_LARGE.contains("leaves=large") && TOP_LARGE.contains("stage=0"));
        assert!(TOP_SMALL.contains("leaves=small") && TOP_SMALL.contains("stage=0"));
    }
}
