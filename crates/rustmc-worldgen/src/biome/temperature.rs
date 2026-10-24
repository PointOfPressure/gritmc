//! Whether a position is cold enough to snow.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `net.minecraft.world.level.biome.Biome#coldEnoughToSnow`,
//! `#warmEnoughToRain`, `#getHeightAdjustedTemperature`,
//! `Biome.TemperatureModifier`.
//!
//! This backs the surface rule's `temperature` condition — the frozen-ocean
//! test — and, later, ice/snow placement and `freeze_top_layer`. It is the
//! one place in worldgen where a **biome's own data** feeds back into block
//! selection: everywhere else the biome is just a key into a rule.
//!
//! # It is float arithmetic, and that is load-bearing
//!
//! Every value on the temperature path is a Java `float`, mixed with `int`
//! coordinates that promote to `float` rather than `double`. Two spots bite:
//!
//! - `pos.getX() / 8.0F` is a **float** division whose result is then
//!   widened to `double` for the noise call. The division itself is exact
//!   either way — dividing by eight only shifts the exponent — but the
//!   `int`-to-`float` conversion in front of it is not, once `|x|` passes
//!   2^24. That is 16,777,216 blocks out, and the world border is at
//!   29,999,984, so it is a place a player can stand rather than a
//!   hypothetical.
//! - `(v + pos.getY() - snowLevel) * 0.05F / 40.0F` runs entirely in
//!   `float`, left to right. `0.05F` and `0.05` are different numbers, so
//!   doing this in `f64` and narrowing at the end lands on a different side
//!   of the 0.15 threshold near the boundary — and this one bites
//!   everywhere, not just at the border.
//!
//! The `FROZEN` modifier, by contrast, multiplies `int` coordinates by
//! `double` literals, so *that* path is genuinely double. The two
//! conventions sit ten lines apart in the reference.
//!
//! # The temperature cache is not modelled
//!
//! `Biome` memoises `getTemperature` in a 1024-entry thread-local LRU keyed
//! on the packed position. It is a pure function of its inputs and the map's
//! `NaN` default is unreachable (temperature is never `NaN`), so the cache
//! is invisible — unlike `cacheOnce`/`flatCache` in the density layer, where
//! the cache decides *which* position is sampled and is therefore behaviour.
//! Stated here so the omission is a finding rather than a gap.

use crate::noise::PerlinSimplexNoise;
use crate::rng::{LegacyRandomSource, WorldgenRandom};

/// `Biome.TemperatureModifier`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemperatureModifier {
    None,
    /// Punches patches of 0.2 temperature into an otherwise-uniform biome,
    /// which is what makes frozen oceans partly liquid. Only
    /// `frozen_ocean` and `deep_frozen_ocean` carry it.
    Frozen,
}

impl TemperatureModifier {
    /// Parse the datapack spelling. Unknown values are rejected rather than
    /// defaulting to [`Self::None`]: a typo'd modifier that silently means
    /// "none" generates a world that is wrong only in frozen oceans.
    ///
    /// # Errors
    /// Returns the offending string if it names no known modifier.
    pub fn parse(name: &str) -> Result<Self, &str> {
        match name {
            "none" => Ok(Self::None),
            "frozen" => Ok(Self::Frozen),
            other => Err(other),
        }
    }
}

/// A biome's climate settings, as far as temperature is concerned.
///
/// `has_precipitation` and `downfall` live in the same record in vanilla but
/// do not reach this calculation, so they are not carried here.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BiomeClimate {
    /// `getBaseTemperature`.
    pub temperature: f32,
    pub modifier: TemperatureModifier,
}

impl BiomeClimate {
    /// Read from a biome's datapack JSON.
    ///
    /// The same argument as the surface rules: a server has to parse this
    /// form anyway to support datapacks, so reading it is the normal path
    /// rather than a shortcut around a hardcoded table. `temperature_modifier`
    /// is optional and defaults to `none`, matching the codec.
    ///
    /// # Errors
    /// If `temperature` is absent or not a number, or `temperature_modifier`
    /// names no known modifier.
    pub fn from_datapack_json(json: &serde_json::Value) -> Result<Self, String> {
        let temperature = json
            .get("temperature")
            .and_then(serde_json::Value::as_f64)
            .ok_or_else(|| "biome has no numeric `temperature`".to_string())?;
        // Vanilla's codec reads this as a float, so the JSON's decimal is
        // narrowed once here rather than being carried at double width and
        // narrowed later — which would round differently.
        #[allow(clippy::cast_possible_truncation)]
        let temperature = temperature as f32;

        let modifier = match json.get("temperature_modifier") {
            None => TemperatureModifier::None,
            Some(value) => {
                let name = value
                    .as_str()
                    .ok_or_else(|| "`temperature_modifier` is not a string".to_string())?;
                TemperatureModifier::parse(name)
                    .map_err(|bad| format!("unknown temperature_modifier `{bad}`"))?
            }
        };

        Ok(Self {
            temperature,
            modifier,
        })
    }
}

/// The three static, world-independent noises `Biome` holds.
///
/// They are seeded from fixed constants — 1234, 3456, 2345 — and never from
/// the world seed, so every world shares them and one instance can serve the
/// whole server. Building them is not free (each `SimplexNoise` shuffles a
/// 256-entry permutation), so callers should keep one rather than construct
/// per lookup.
#[derive(Clone, Debug)]
pub struct TemperatureNoises {
    /// `TEMPERATURE_NOISE` — the above-sea-level height falloff.
    temperature: PerlinSimplexNoise,
    /// `FROZEN_TEMPERATURE_NOISE` — the large-scale ice-patch variation.
    frozen: PerlinSimplexNoise,
    /// `BIOME_INFO_NOISE` — deprecated in vanilla, still load-bearing here:
    /// the `FROZEN` modifier reads it at two different scales.
    biome_info: PerlinSimplexNoise,
}

impl Default for TemperatureNoises {
    fn default() -> Self {
        Self::new()
    }
}

impl TemperatureNoises {
    #[must_use]
    pub fn new() -> Self {
        // Two of the three are single-octave and one is not. Written as
        // three `single_octave` calls first, and the golden gate caught it:
        // the noise layer passed (it builds stacks from the vectors' own
        // declared octave sets) while every frozen-ocean lookup was wrong.
        // The octave set is part of the noise's identity, not a tuning knob.
        let mut frozen_random = WorldgenRandom::new(LegacyRandomSource::new(3456));
        Self {
            temperature: PerlinSimplexNoise::single_octave(1234),
            frozen: PerlinSimplexNoise::new(&mut frozen_random, &[-2, -1, 0]),
            biome_info: PerlinSimplexNoise::single_octave(2345),
        }
    }

    /// `TEMPERATURE_NOISE`, for gates that need to prove the stack itself
    /// rather than the answer it feeds.
    #[must_use]
    pub const fn temperature_noise(&self) -> &PerlinSimplexNoise {
        &self.temperature
    }

    /// `FROZEN_TEMPERATURE_NOISE`.
    #[must_use]
    pub const fn frozen_noise(&self) -> &PerlinSimplexNoise {
        &self.frozen
    }

    /// `BIOME_INFO_NOISE`.
    #[must_use]
    pub const fn biome_info_noise(&self) -> &PerlinSimplexNoise {
        &self.biome_info
    }

    /// `TemperatureModifier#modifyTemperature`.
    ///
    /// Note that `y` is not read: the ice patches are a 2D pattern, so a
    /// frozen ocean's surface and its sea floor get the same adjustment.
    #[must_use]
    pub fn modify_temperature(
        &self,
        modifier: TemperatureModifier,
        x: i32,
        z: i32,
        base_temperature: f32,
    ) -> f32 {
        match modifier {
            TemperatureModifier::None => base_temperature,
            TemperatureModifier::Frozen => {
                // `int * double` — genuinely double arithmetic here, unlike
                // the height adjustment below.
                let large_variation =
                    self.frozen
                        .get_value(f64::from(x) * 0.05, f64::from(z) * 0.05, false)
                        * 7.0;
                let edge_variation =
                    self.biome_info
                        .get_value(f64::from(x) * 0.2, f64::from(z) * 0.2, false);
                let ice_patches = large_variation + edge_variation;
                if ice_patches < 0.3 {
                    let small_variation =
                        self.biome_info
                            .get_value(f64::from(x) * 0.09, f64::from(z) * 0.09, false);
                    if small_variation < 0.8 {
                        return 0.2;
                    }
                }
                base_temperature
            }
        }
    }

    /// `getHeightAdjustedTemperature`.
    #[must_use]
    pub fn height_adjusted_temperature(
        &self,
        climate: BiomeClimate,
        x: i32,
        y: i32,
        z: i32,
        sea_level: i32,
    ) -> f32 {
        let adjusted = self.modify_temperature(climate.modifier, x, z, climate.temperature);
        let snow_level = sea_level + 17;
        if y > snow_level {
            // `pos.getX() / 8.0F`: float division, then widened for the
            // call. Not `f64::from(x) / 8.0`.
            #[allow(clippy::cast_precision_loss)]
            let (nx, nz) = (x as f32 / 8.0, z as f32 / 8.0);
            #[allow(clippy::cast_possible_truncation)]
            let v = (self
                .temperature
                .get_value(f64::from(nx), f64::from(nz), false)
                * 8.0) as f32;
            // Left to right, all in float: ((v + y) - snowLevel) * 0.05 / 40.
            #[allow(clippy::cast_precision_loss)]
            let drop = (v + y as f32 - snow_level as f32) * 0.05 / 40.0;
            adjusted - drop
        } else {
            adjusted
        }
    }

    /// `warmEnoughToRain` — temperature at or above 0.15.
    #[must_use]
    pub fn warm_enough_to_rain(
        &self,
        climate: BiomeClimate,
        x: i32,
        y: i32,
        z: i32,
        sea_level: i32,
    ) -> bool {
        self.height_adjusted_temperature(climate, x, y, z, sea_level) >= 0.15
    }

    /// `coldEnoughToSnow` — the surface rule's `temperature` condition.
    ///
    /// Defined in vanilla as the negation of [`Self::warm_enough_to_rain`]
    /// rather than as its own comparison, which matters only if the
    /// temperature could be `NaN`; keeping the negation costs nothing and
    /// removes the question.
    #[must_use]
    pub fn cold_enough_to_snow(
        &self,
        climate: BiomeClimate,
        x: i32,
        y: i32,
        z: i32,
        sea_level: i32,
    ) -> bool {
        !self.warm_enough_to_rain(climate, x, y, z, sea_level)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAINS: BiomeClimate = BiomeClimate {
        temperature: 0.8,
        modifier: TemperatureModifier::None,
    };
    const FROZEN_OCEAN: BiomeClimate = BiomeClimate {
        temperature: 0.0,
        modifier: TemperatureModifier::Frozen,
    };

    #[test]
    fn below_the_snow_level_the_temperature_is_the_biomes_own() {
        let noises = TemperatureNoises::new();
        // snowLevel = 63 + 17 = 80, and the branch is `y > snowLevel`, so
        // 80 itself takes the unadjusted path.
        for y in [-64, 0, 63, 80] {
            assert_eq!(
                noises.height_adjusted_temperature(PLAINS, 7, y, -13, 63),
                0.8,
                "y={y} should not be height-adjusted"
            );
        }
        assert_ne!(
            noises.height_adjusted_temperature(PLAINS, 7, 81, -13, 63),
            0.8
        );
    }

    #[test]
    fn height_cools_the_air_monotonically_at_a_fixed_column() {
        // The noise term depends only on x/z, so within one column the
        // adjustment is strictly decreasing in y. If it were not, the
        // arithmetic order would be wrong.
        let noises = TemperatureNoises::new();
        let mut previous = f32::INFINITY;
        for y in 81..320 {
            let t = noises.height_adjusted_temperature(PLAINS, 42, y, 42, 63);
            assert!(t < previous, "temperature rose at y={y}");
            previous = t;
        }
    }

    #[test]
    fn a_warm_biome_at_sea_level_never_snows() {
        let noises = TemperatureNoises::new();
        for x in -40..40 {
            assert!(!noises.cold_enough_to_snow(PLAINS, x, 63, x * 3, 63));
        }
    }

    #[test]
    fn the_frozen_modifier_produces_both_answers_over_an_ocean() {
        // The point of the modifier: a frozen ocean is not uniformly frozen.
        // Base temperature 0.0 is below the 0.15 threshold, and the patches
        // it punches are 0.2, which is above it — so a run of positions must
        // contain both.
        let noises = TemperatureNoises::new();
        let mut frozen = 0;
        let mut liquid = 0;
        for x in -128..128 {
            for z in [-64, 0, 64] {
                if noises.cold_enough_to_snow(FROZEN_OCEAN, x, 63, z, 63) {
                    frozen += 1;
                } else {
                    liquid += 1;
                }
            }
        }
        assert!(frozen > 0, "no frozen positions in a frozen ocean");
        assert!(
            liquid > 0,
            "the frozen modifier never punched a warm patch, so it is not being applied"
        );
    }

    #[test]
    fn the_frozen_modifier_is_the_only_thing_that_moves_a_zero_temperature_biome() {
        // Same base temperature, modifier off: uniformly freezing.
        let noises = TemperatureNoises::new();
        let plain_cold = BiomeClimate {
            temperature: 0.0,
            modifier: TemperatureModifier::None,
        };
        for x in -128..128 {
            assert!(noises.cold_enough_to_snow(plain_cold, x, 63, 0, 63));
        }
    }

    #[test]
    fn the_float_path_only_diverges_past_two_to_the_twenty_fourth() {
        // Written the other way round first, asserting the two disagreed
        // somewhere in 1..10_000, and it failed: dividing by eight is exact
        // in binary, so below 2^24 the float and double paths agree
        // *exactly*. The divergence comes from the int-to-float conversion,
        // not the division — which is a narrower claim than the one the
        // comment originally made, and the reason this test is phrased as
        // "where is the boundary" rather than "they differ".
        let float_path = |x: i32| f64::from(x as f32 / 8.0f32);
        let double_path = |x: i32| f64::from(x) / 8.0;

        for x in [0, 1, 7, 12_345, 1 << 23, (1 << 24) - 1, 1 << 24] {
            assert_eq!(float_path(x), double_path(x), "diverged early at x={x}");
        }
        // First integer f32 cannot represent. The world border is at
        // 29,999,984, comfortably past it, so this is somewhere a player can
        // stand rather than a hypothetical — which is why the port keeps the
        // float division instead of "simplifying" it.
        assert_ne!(float_path((1 << 24) + 1), double_path((1 << 24) + 1));
        // Not every coordinate out there diverges: above 2^24 an f32 still
        // holds the even integers exactly, and the world border itself
        // (29,999,984) is even, so *it* round-trips. The odd ones next to it
        // do not, which is the case that matters.
        const WORLD_BORDER: i32 = 29_999_984;
        assert_eq!(float_path(WORLD_BORDER), double_path(WORLD_BORDER));
        let diverging = (WORLD_BORDER - 64..WORLD_BORDER)
            .filter(|&x| float_path(x) != double_path(x))
            .count();
        assert!(
            diverging > 0,
            "no coordinate near the border diverges, so the float path is unfalsifiable"
        );
    }

    #[test]
    fn the_height_falloff_constants_are_float_not_double() {
        // This is the hazard that bites at ordinary coordinates. `0.05F` is
        // not `0.05`, so the whole expression has to stay in f32.
        assert_ne!(f64::from(0.05f32), 0.05f64);
        let as_float = |v: f32, y: i32, snow: i32| (v + y as f32 - snow as f32) * 0.05 / 40.0;
        let as_double = |v: f32, y: i32, snow: i32| {
            ((f64::from(v) + f64::from(y) - f64::from(snow)) * 0.05 / 40.0) as f32
        };
        let differing = (81..320)
            .filter(|&y| as_float(0.371, y, 80) != as_double(0.371, y, 80))
            .count();
        assert!(
            differing > 0,
            "the two orders agree for every y, so the float chain is unfalsifiable here"
        );
    }

    #[test]
    fn a_datapack_biome_without_a_modifier_gets_none() {
        let json = serde_json::json!({"temperature": 0.8, "downfall": 0.4});
        assert_eq!(
            BiomeClimate::from_datapack_json(&json),
            Ok(BiomeClimate {
                temperature: 0.8,
                modifier: TemperatureModifier::None
            })
        );
    }

    #[test]
    fn a_datapack_biome_with_a_bad_modifier_is_an_error_not_a_default() {
        // Defaulting an unrecognised modifier to `none` would generate a
        // world that is wrong only in frozen oceans, which is exactly the
        // kind of wrong nobody notices.
        let json = serde_json::json!({"temperature": 0.0, "temperature_modifier": "frzoen"});
        assert!(BiomeClimate::from_datapack_json(&json)
            .unwrap_err()
            .contains("frzoen"));
        let missing = serde_json::json!({"downfall": 0.4});
        assert!(BiomeClimate::from_datapack_json(&missing).is_err());
    }

    #[test]
    fn modifier_names_round_trip_and_typos_are_rejected() {
        assert_eq!(
            TemperatureModifier::parse("none"),
            Ok(TemperatureModifier::None)
        );
        assert_eq!(
            TemperatureModifier::parse("frozen"),
            Ok(TemperatureModifier::Frozen)
        );
        assert_eq!(TemperatureModifier::parse("frozen "), Err("frozen "));
        assert_eq!(TemperatureModifier::parse("Frozen"), Err("Frozen"));
    }
}

#[cfg(test)]
mod biome_info_vectors {
    //! `Biome.BIOME_INFO_NOISE` against the JVM's own values.
    //!
    //! This noise is not decorative: `NoiseBasedCountPlacement` divides the
    //! block position by a factor, samples it, and `ceil`s the result into a
    //! **count** — so its value decides how many kelp stalks a chunk gets.
    //! A whole-world diff showed this port placing kelp in a strict *subset*
    //! of vanilla's columns, 34 of 63, which is the signature of a short
    //! count rather than a wrong position.
    use super::TemperatureNoises;

    /// Dumped from `Biome.BIOME_INFO_NOISE.getValue(x/80, z/80, false)`.
    const VECTORS: [(i32, i32, i64); 6] = [
        (1010, 1027, 4_598_622_127_623_918_444),
        (1017, 1027, -4_637_360_169_455_361_669),
        (1014, 1029, 4_591_407_144_659_075_854),
        (1024, 1024, -4_623_083_481_535_908_610),
        (0, 0, 0),
        (-33, 77, 4_600_917_716_394_925_932),
    ];

    #[test]
    fn the_biome_info_noise_matches_the_jvm() {
        let noises = TemperatureNoises::new();
        let mut wrong = 0;
        for (x, z, want) in VECTORS {
            let got = noises.biome_info_noise().get_value(
                f64::from(x) / 80.0,
                f64::from(z) / 80.0,
                false,
            );
            #[expect(clippy::cast_sign_loss, reason = "raw IEEE-754 bits through an i64")]
            let want_bits = want as u64;
            if got.to_bits() != want_bits {
                wrong += 1;
                println!(
                    "  ({x}, {z}): got {got:.17e} ({}), want {:.17e} ({want})",
                    got.to_bits() as i64,
                    f64::from_bits(want_bits)
                );
            }
        }
        assert_eq!(
            wrong, 0,
            "the count placement's noise disagrees with vanilla"
        );
    }
}
