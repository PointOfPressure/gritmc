//! `minecraft:sculk_patch` — the feature the deep dark is made of.
//!
//! Refused 1658 times in a single badlands window before this existed, which
//! is 1506 missing sculk blocks — the largest single block-count gap this
//! project had, and an order of magnitude above the ~109 oracle noise floor.
//!
//! # The whole feature
//!
//! ```java
//!   if (!canSpreadFrom(level, origin)) return false;
//!   SculkSpreader spreader = SculkSpreader.createWorldGenSpreader();
//!   int rounds = config.spreadRounds() + config.growthRounds();
//!   for (int i = 0; i < rounds; i++) {
//!       for (int j = 0; j < config.chargeCount(); j++)
//!           spreader.addCursors(origin, config.amountPerCharge());
//!       boolean convert = i < config.spreadRounds();
//!       for (int k = 0; k < config.spreadAttempts(); k++)
//!           spreader.updateCursors(level, origin, random, convert);
//!       spreader.clear();
//!   }
//!   BlockPos below = origin.below();
//!   if (random.nextFloat() <= config.catalystChance()
//!       && level.getBlockState(below).isCollisionShapeFullBlock(level, below))
//!       level.setBlock(origin, SCULK_CATALYST.defaultBlockState(), 3);
//!   int extra = config.extraRareGrowths().sample(random);
//!   ...
//!   return true;
//! ```
//!
//! Two draw-order details:
//!
//!   * the catalyst's `nextFloat` is **unconditional** — it is drawn before
//!     the full-block test, so a patch on soft ground still spends it;
//!   * `extraRareGrowths` is `ConstantInt(0)` for both shipped patches, and a
//!     constant provider does not draw.
//!
//! `sculk_patch_deep_dark` is charge_count 10, amount_per_charge 32,
//! spread_attempts 64, spread_rounds 1, growth_rounds 0, catalyst_chance 0.5 —
//! so one round, ten cursors of 32 charge, 64 update passes, and `convert` is
//! true throughout that single round.

/// `SculkPatchConfiguration`.
#[derive(Debug, Clone, PartialEq)]
pub struct SculkPatchConfig {
    pub charge_count: i32,
    pub amount_per_charge: i32,
    pub spread_attempts: i32,
    pub spread_rounds: i32,
    pub growth_rounds: i32,
    /// An `IntProvider`. `sculk_patch_deep_dark` holds a constant 0, which
    /// draws nothing; `sculk_patch_ancient_city` holds `uniform 1..3`, which
    /// draws one `nextInt` and then two more per growth.
    pub extra_rare_growths: super::int_provider::IntProvider,
    pub catalyst_chance: f32,
}

impl SculkPatchConfig {
    /// Parse from a configured feature's `config` object.
    ///
    /// # Errors
    /// If a required field is missing or not a number.
    pub fn parse(config: &serde_json::Value) -> Result<Self, String> {
        let int = |name: &str| -> Result<i32, String> {
            config[name]
                .as_i64()
                .map(|v| v as i32)
                .ok_or_else(|| format!("sculk_patch: no integer `{name}`"))
        };
        Ok(Self {
            charge_count: int("charge_count")?,
            amount_per_charge: int("amount_per_charge")?,
            spread_attempts: int("spread_attempts")?,
            spread_rounds: int("spread_rounds")?,
            growth_rounds: int("growth_rounds")?,
            // Read as the provider it is. This used to be `as_i64()` with an
            // `unwrap_or(0)`, which turned the ancient city's `uniform 1..3`
            // into a silent zero: no shriekers and three draws fewer per patch.
            extra_rare_growths: match config.get("extra_rare_growths") {
                Some(v) => super::int_provider::IntProvider::parse(v)
                    .map_err(|e| format!("sculk_patch: extra_rare_growths: {e}"))?,
                None => super::int_provider::IntProvider::Constant(0),
            },
            catalyst_chance: config["catalyst_chance"]
                .as_f64()
                .ok_or("sculk_patch: no `catalyst_chance`")? as f32,
        })
    }
}

// The feature itself — `SculkPatchFeature.place` — is
// `crate::feature::sculk_worldgen::place`. This module is its configuration.

#[cfg(test)]
mod tests {
    use super::*;

    fn deep_dark() -> SculkPatchConfig {
        SculkPatchConfig {
            charge_count: 10,
            amount_per_charge: 32,
            spread_attempts: 64,
            spread_rounds: 1,
            growth_rounds: 0,
            extra_rare_growths: super::super::int_provider::IntProvider::Constant(0),
            catalyst_chance: 0.5,
        }
    }

    #[test]
    fn the_deep_dark_config_parses() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{"amount_per_charge":32,"catalyst_chance":0.5,"charge_count":10,
                "extra_rare_growths":0,"growth_rounds":0,"spread_attempts":64,
                "spread_rounds":1}"#,
        )
        .expect("json");
        assert_eq!(SculkPatchConfig::parse(&json).expect("parse"), deep_dark());
    }

    /// The ancient city's patch: `extra_rare_growths` is `uniform 1..3`, not
    /// a number, and reading it as one made it a silent zero.
    #[test]
    fn the_ancient_city_patch_reads_its_rare_growths_as_a_provider() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{"amount_per_charge":32,"catalyst_chance":0.5,"charge_count":10,
                "extra_rare_growths":{"type":"minecraft:uniform","max_inclusive":3,
                "min_inclusive":1},"growth_rounds":0,"spread_attempts":64,
                "spread_rounds":1}"#,
        )
        .expect("json");
        let config = SculkPatchConfig::parse(&json).expect("parse");
        assert_eq!(
            config.extra_rare_growths,
            super::super::int_provider::IntProvider::Uniform {
                min_inclusive: 1,
                max_inclusive: 3
            }
        );
    }
}
