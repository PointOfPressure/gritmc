//! `large_dripstone` — the two big cones that meet across a dripstone cave.
//!
//! Reference (26.2, behaviour ported from `javap -p -c -constants` on the
//! pinned jar and never transcribed): `LargeDripstoneFeature`, its two inner
//! classes `LargeDripstone` and `WindOffsetter`, `LargeDripstoneConfiguration`,
//! `SpeleothemUtils#{getSpeleothemHeight, isCircleMostlyEmbeddedInStone,
//! isEmptyOrWater, isEmptyOrWaterOrLava}`, `net.minecraft.world.level.levelgen.Column`,
//! `Mth#{clamp, cos, floor, randomBetween, randomBetweenInclusive, sin, sqrt}`.
//!
//! # It is the largest refusal by COUNT and the fifth by BLOCKS
//!
//! 4,569 declines across the three census squares — 543, 1,977 and 2,049 —
//! against 1,593 blocks of `dripstone_block` missing. Both halves belong in
//! any sentence that quotes either: a tally ranked by *count* ranks how often
//! something is **attempted**, and `large_dripstone` is attempted 10..48 times
//! per chunk anywhere the dripstone-cave placement fires, at a uniform height
//! over the whole column of the world. Most of those attempts land in solid
//! rock and place nothing. Rank by blocks; corroborate by count.
//!
//! # A refusal here is not free, and that is the point of porting it
//!
//! An unported feature spends **no** randomness while vanilla spends the whole
//! feature's draws, so every feature decorated after it in the same step is
//! displaced. `large_dripstone` sits in step 6 (`UNDERGROUND_DECORATION`) of
//! `dripstone_caves` ahead of the pointed-dripstone clusters, so 4,569 refusals
//! are 4,569 desyncs of the features behind them — which is why the block
//! census can show only 1,593 missing `dripstone_block` while the residual in
//! those biomes is far larger.
//!
//! # The draw sequence, which is the whole contract
//!
//! ```text
//!   isEmptyOrWater(origin)                   guard, before every draw
//!   Column.scan(...)                         no draws; needs BOTH ends
//!   height >= 4                              guard
//!   radius = randomBetweenInclusive(...)     1  nextInt
//!   stalactite bluntness, heightScale        2  makeDripstone(ceiling)
//!   stalagmite bluntness, heightScale        2  makeDripstone(floor)
//!   windSpeed, angle                         2  ONLY if BOTH are wind-suitable
//!   per accepted column of each cone:        1  nextFloat, then
//!   ... 1 more                                  only when that float < 0.2
//! ```
//!
//! So a placement that reaches the first draw costs five values, plus two if
//! the wind fires, plus a world-shaped tail. The guards above the first draw
//! are what make an attempt in solid rock cost the stream nothing — and they
//! are why the count of *attempts* is so much larger than the count of
//! placements.
//!
//! # `moveBack…` shrinks the radius by halving, and restarts the walk
//!
//! Each cone tries to bury its base: it walks up to `min(10, height)` blocks
//! back into the rock looking for a position whose surrounding ring is solid.
//! Failing that it **halves its radius and starts the walk again from the
//! original root** — not from where the walk stopped. A radius of 16 therefore
//! gets five walks (16, 8, 4, 2) before it gives up at 1, and the accepted
//! radius is the one the cone is then drawn with. Restarting from the wrong
//! place is a silent shape change, not a crash.
//!
//! # The wind is one angle for the pair, or no wind at all
//!
//! `WindOffsetter` is constructed once, from the *stalactite's* suitability
//! **and** the stalagmite's — `isSuitableForWind` is checked on both and the
//! two draws happen only if both pass. Every position of both cones is then
//! offset by the same horizontal vector scaled by its distance from the
//! origin's y, clamped to `16 - radius`. A wide column gets almost no lean;
//! a narrow one can lean a long way.

use super::speleothem::{
    get_speleothem_height, is_circle_mostly_embedded_in_stone, is_empty_or_water,
    is_empty_or_water_or_lava, is_replaceable, plain, scan, NeedsRules, SpeleothemWorld,
};
use crate::carver::providers::FloatProvider;
use crate::feature::int_provider::IntProvider;
use crate::math;
use crate::rng::RandomSource;
use crate::tags::BlockTags;

/// `LargeDripstoneConfiguration`.
#[derive(Clone, Debug, PartialEq)]
pub struct LargeDripstoneConfig {
    /// `floor_to_ceiling_search_range`, default 30 — the codec's own default,
    /// which matters because the datapack omits the field.
    pub floor_to_ceiling_search_range: i32,
    pub column_radius: IntProvider,
    pub height_scale: FloatProvider,
    pub max_column_radius_to_cave_height_ratio: f32,
    pub stalactite_bluntness: FloatProvider,
    pub stalagmite_bluntness: FloatProvider,
    pub wind_speed: FloatProvider,
    pub min_radius_for_wind: i32,
    pub min_bluntness_for_wind: f32,
    /// `replaceable_blocks` written as a list.
    replaceable: Vec<String>,
    /// `replaceable_blocks` written as a tag, without the `#`.
    replaceable_tag: Option<String>,
}

impl LargeDripstoneConfig {
    /// Parse one `large_dripstone` config object.
    ///
    /// # Errors
    /// If a required field is absent or malformed.
    pub fn parse(config: &serde_json::Value) -> Result<Self, String> {
        let provider = |name: &str| -> Result<FloatProvider, String> {
            FloatProvider::parse(
                config
                    .get(name)
                    .ok_or_else(|| format!("large_dripstone has no `{name}`"))?,
            )
        };
        let float = |name: &str| -> Result<f32, String> {
            config
                .get(name)
                .and_then(serde_json::Value::as_f64)
                .map(|v| {
                    #[allow(clippy::cast_possible_truncation)]
                    {
                        v as f32
                    }
                })
                .ok_or_else(|| format!("large_dripstone has no numeric `{name}`"))
        };
        let int = |name: &str| -> Result<i32, String> {
            config
                .get(name)
                .and_then(serde_json::Value::as_i64)
                .and_then(|v| i32::try_from(v).ok())
                .ok_or_else(|| format!("large_dripstone has no integer `{name}`"))
        };
        // `replaceable_blocks` is a `HolderSet`: either `#tag` or a list.
        let replaceable_value = config
            .get("replaceable_blocks")
            .ok_or_else(|| "large_dripstone has no `replaceable_blocks`".to_string())?;
        let (replaceable, replaceable_tag) = match replaceable_value {
            serde_json::Value::String(name) => name.strip_prefix('#').map_or_else(
                || (vec![name.clone()], None),
                |tag| (Vec::new(), Some(tag.to_string())),
            ),
            serde_json::Value::Array(list) => (
                list.iter()
                    .map(|v| {
                        v.as_str()
                            .map(str::to_string)
                            .ok_or_else(|| "replaceable_blocks entry is not a string".to_string())
                    })
                    .collect::<Result<Vec<_>, String>>()?,
                None,
            ),
            _ => return Err("replaceable_blocks is neither a tag nor a list".to_string()),
        };
        Ok(Self {
            // `Codec.intRange(1, 512).optionalFieldOf(..., 30)` — absent in
            // the shipped datapack, so the default is the value that runs.
            floor_to_ceiling_search_range: config
                .get("floor_to_ceiling_search_range")
                .and_then(serde_json::Value::as_i64)
                .and_then(|v| i32::try_from(v).ok())
                .unwrap_or(30),
            column_radius: IntProvider::parse(
                config
                    .get("column_radius")
                    .ok_or_else(|| "large_dripstone has no `column_radius`".to_string())?,
            )?,
            height_scale: provider("height_scale")?,
            max_column_radius_to_cave_height_ratio: float(
                "max_column_radius_to_cave_height_ratio",
            )?,
            stalactite_bluntness: provider("stalactite_bluntness")?,
            stalagmite_bluntness: provider("stalagmite_bluntness")?,
            wind_speed: provider("wind_speed")?,
            min_radius_for_wind: int("min_radius_for_wind")?,
            min_bluntness_for_wind: float("min_bluntness_for_wind")?,
            replaceable,
            replaceable_tag,
        })
    }

    #[must_use]
    pub const fn is_supported(&self) -> bool {
        true
    }
}

/// The world a large dripstone reads and writes.
///
/// One question more than [`SpeleothemWorld`] asks, and it is not decoration:
/// a stalagmite stops at `WORLD_SURFACE_WG`, so a cone growing up out of a
/// cave into open sky is truncated rather than built into the air. That
/// heightmap is the same fossil the `height_range` placement modifiers read,
/// and the decorator answers it from the same canvas.
///
/// A required method rather than a blanket impl over `PlacementWorld`: that
/// trait carries six questions this feature never asks, and a test cave that
/// had to answer all six to exercise one truncation is a test nobody writes.
pub trait LargeDripstoneWorld: SpeleothemWorld {
    /// `WorldGenLevel#getHeight(Heightmap.Types.WORLD_SURFACE_WG, x, z)`.
    fn world_surface_wg(&mut self, x: i32, z: i32) -> i32;
}

/// `LargeDripstoneFeature$WindOffsetter`.
///
/// `None` is the reference's `noWind()`, whose `windSpeed` field is null and
/// whose `offset` is the identity — which is exactly what an `Option` says,
/// so there is no separate constructor here.
#[derive(Clone, Copy, Debug, PartialEq)]
struct WindOffsetter {
    origin_y: i32,
    /// `Vec3`, of which only x and z are ever non-zero.
    wind: Option<(f64, f64)>,
    max_offset: i32,
}

impl WindOffsetter {
    /// The two-draw constructor. `max_offset` is `16 - radius`.
    fn sample<R: RandomSource + ?Sized>(
        origin_y: i32,
        random: &mut R,
        wind_speed: FloatProvider,
        max_offset: i32,
    ) -> Self {
        let speed = wind_speed.sample(random);
        // `Mth.randomBetween(random, 0.0F, (float) Math.PI)`.
        let angle = random.next_f32() * (std::f32::consts::PI - 0.0) + 0.0;
        // `new Vec3(Mth.cos(f1) * f, 0.0, Mth.sin(f1) * f)` — the multiply is
        // in `f32` and the widening to `f64` happens at the `Vec3`
        // constructor, not before it.
        let x = math::cos(f64::from(angle)) * speed;
        let z = math::sin(f64::from(angle)) * speed;
        Self {
            origin_y,
            wind: Some((f64::from(x), f64::from(z))),
            max_offset,
        }
    }

    const fn none() -> Self {
        Self {
            origin_y: 0,
            wind: None,
            max_offset: 0,
        }
    }

    /// `WindOffsetter#offset`.
    fn offset(self, pos: (i32, i32, i32)) -> (i32, i32, i32) {
        let Some((wx, wz)) = self.wind else {
            return pos;
        };
        let distance = self.origin_y - pos.1;
        let scaled_x = wx * f64::from(distance);
        let scaled_z = wz * f64::from(distance);
        let dx = math::floor(scaled_x).clamp(-self.max_offset, self.max_offset);
        let dz = math::floor(scaled_z).clamp(-self.max_offset, self.max_offset);
        (pos.0 + dx, pos.1, pos.2 + dz)
    }
}

/// `LargeDripstoneFeature$LargeDripstone`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct LargeDripstone {
    root: (i32, i32, i32),
    /// Which way the *tip* points. The stalactite hanging from the ceiling is
    /// `false`; the stalagmite standing on the floor is `true`.
    pointing_up: bool,
    radius: i32,
    bluntness: f64,
    scale: f64,
}

impl LargeDripstone {
    /// `getHeight` — the height on the axis, i.e. at radius zero.
    fn height(self) -> i32 {
        self.height_at_radius(0.0)
    }

    /// `getHeightAtRadius`.
    fn height_at_radius(self, radius: f32) -> i32 {
        // `f64::from` on both widenings, so only the `(int)` narrowing at
        // the end is lossy — `i32` and `f32` both widen to `f64` exactly.
        // Only `cast_possible_truncation` is expected here. `i32` and `f32`
        // both widen to `f64` exactly, so `f64::from` on either loses nothing
        // and `cast_precision_loss` never fires — an unfulfilled `#[expect]`
        // is itself a lint, and an expectation nothing raises is a claim about
        // this code that is not true.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "`(int) getSpeleothemHeight(...)` — the reference's own \
                      narrowing; the two widenings above it are exact"
        )]
        {
            get_speleothem_height(
                f64::from(radius),
                f64::from(self.radius),
                self.scale,
                self.bluntness,
            ) as i32
        }
    }

    /// `isSuitableForWind`.
    fn is_suitable_for_wind(self, config: &LargeDripstoneConfig) -> bool {
        self.radius >= config.min_radius_for_wind
            && self.bluntness >= f64::from(config.min_bluntness_for_wind)
    }

    /// `moveBackUntilBaseIsInsideStoneAndShrinkRadiusIfNecessary`.
    ///
    /// Returns whether this cone is placeable at all. A `false` from lava is
    /// not the same as a `false` from exhausting the radius: the first ends
    /// the whole search, the second is reached only after four halvings.
    fn move_back_until_base_is_inside_stone<W: LargeDripstoneWorld + ?Sized>(
        &mut self,
        world: &mut W,
        wind: WindOffsetter,
    ) -> Result<bool, NeedsRules> {
        while self.radius > 1 {
            // Restarted from `this.root` on every halving — not from where the
            // previous walk stopped.
            let mut pos = self.root;
            let steps = 10.min(self.height());
            for _ in 0..steps {
                if plain(&world.block_at(pos.0, pos.1, pos.2)) == "minecraft:lava" {
                    return Ok(false);
                }
                if is_circle_mostly_embedded_in_stone(world, wind.offset(pos), self.radius)? {
                    self.root = pos;
                    return Ok(true);
                }
                pos.1 += if self.pointing_up { -1 } else { 1 };
            }
            self.radius /= 2;
        }
        Ok(false)
    }

    /// `placeBlocks`.
    fn place_blocks<W: LargeDripstoneWorld + ?Sized>(
        self,
        world: &mut W,
        tags: &BlockTags,
        random: &mut dyn RandomSource,
        wind: WindOffsetter,
    ) -> Result<(), NeedsRules> {
        for x in -self.radius..=self.radius {
            for z in -self.radius..=self.radius {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "`Mth.sqrt((float) (i * i + j * j))` — the reference widens here"
                )]
                let distance = ((x * x + z * z) as f32).sqrt();
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "`(float) this.radius` — the reference's own comparison"
                )]
                if distance > self.radius as f32 {
                    continue;
                }
                let mut height = self.height_at_radius(distance);
                if height <= 0 {
                    continue;
                }
                // `(double) random.nextFloat() < 0.2` — the float is widened
                // before the comparison, so this is a double compare against
                // the double literal and not `0.2f`.
                if f64::from(random.next_f32()) < 0.2 {
                    #[expect(
                        clippy::cast_possible_truncation,
                        clippy::cast_precision_loss,
                        reason = "`(int) ((float) k * Mth.randomBetween(random, 0.8F, 1.0F))`"
                    )]
                    {
                        height = (height as f32 * (random.next_f32() * (1.0 - 0.8) + 0.8)) as i32;
                    }
                }
                let mut pos = (self.root.0 + x, self.root.1, self.root.2 + z);
                let mut placed_any = false;
                // A stalagmite stops at the surface; a stalactite has no
                // ceiling to respect.
                let limit = if self.pointing_up {
                    world.world_surface_wg(pos.0, pos.2)
                } else {
                    i32::MAX
                };
                for _ in 0..height {
                    if pos.1 >= limit {
                        break;
                    }
                    let at = wind.offset(pos);
                    if is_empty_or_water_or_lava(world, at)? {
                        placed_any = true;
                        world.set_block(at.0, at.1, at.2, "minecraft:dripstone_block");
                    } else if placed_any
                        && tags.contains(
                            "minecraft:base_stone_overworld",
                            plain(&world.block_at(at.0, at.1, at.2)),
                        )
                    {
                        break;
                    }
                    pos.1 += if self.pointing_up { 1 } else { -1 };
                }
            }
        }
        Ok(())
    }
}

/// `LargeDripstoneFeature#makeDripstone` — two draws, bluntness then scale.
fn make_dripstone<R: RandomSource + ?Sized>(
    root: (i32, i32, i32),
    pointing_up: bool,
    random: &mut R,
    radius: i32,
    bluntness: FloatProvider,
    height_scale: FloatProvider,
) -> LargeDripstone {
    LargeDripstone {
        root,
        pointing_up,
        radius,
        bluntness: f64::from(bluntness.sample(random)),
        scale: f64::from(height_scale.sample(random)),
    }
}

/// `LargeDripstoneFeature#place`.
///
/// # Errors
/// If the world cannot answer a block question.
pub fn place<W: LargeDripstoneWorld + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    config: &LargeDripstoneConfig,
    random: &mut dyn RandomSource,
    origin: (i32, i32, i32),
) -> Result<bool, NeedsRules> {
    // Every guard below is above the first draw, so an attempt that lands in
    // rock costs the stream nothing at all — which is why 4,569 refusals are
    // not 4,569 desyncs of equal size.
    if !is_empty_or_water(world, origin)? {
        return Ok(false);
    }
    let mut tip = |world: &mut W, pos: (i32, i32, i32)| -> Result<bool, NeedsRules> {
        let state = world.block_at(pos.0, pos.1, pos.2);
        Ok(is_replaceable(
            tags,
            config.replaceable_tag.as_deref(),
            &config.replaceable,
            plain(&state),
        ))
    };
    let Some(column) = scan(
        world,
        origin,
        config.floor_to_ceiling_search_range,
        &mut tip,
    )?
    else {
        return Ok(false);
    };
    // `instanceof Column.Range` — only a two-ended column has a height, and
    // the reference declines anything else before it reads one.
    let (Some(floor), Some(ceiling), Some(height)) =
        (column.floor, column.ceiling, column.height())
    else {
        return Ok(false);
    };
    if height < 4 {
        return Ok(false);
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        reason = "`(int) ((float) range.height() * config.maxColumnRadiusToCaveHeightRatio)`"
    )]
    let from_cave = (height as f32 * config.max_column_radius_to_cave_height_ratio) as i32;
    let radius_min = config.column_radius.min_value();
    let capped = from_cave.clamp(radius_min, config.column_radius.max_value());
    // DRAW 1.
    let radius = random.next_i32_bound(capped - radius_min + 1) + radius_min;

    // DRAWS 2-3, then 4-5. The ceiling cone is made first and the order is
    // load-bearing: both read the same two providers.
    let mut stalactite = make_dripstone(
        (origin.0, ceiling - 1, origin.2),
        false,
        random,
        radius,
        config.stalactite_bluntness,
        config.height_scale,
    );
    let mut stalagmite = make_dripstone(
        (origin.0, floor + 1, origin.2),
        true,
        random,
        radius,
        config.stalagmite_bluntness,
        config.height_scale,
    );

    // DRAWS 6-7, and only when BOTH cones are suitable. `&&` short-circuits in
    // the reference too, but both operands are pure, so the only thing that
    // matters is that neither branch draws.
    let wind = if stalactite.is_suitable_for_wind(config) && stalagmite.is_suitable_for_wind(config)
    {
        WindOffsetter::sample(origin.1, random, config.wind_speed, 16 - radius)
    } else {
        WindOffsetter::none()
    };

    // Both cones are moved back before either is placed — a stalactite that
    // writes blocks first would change the rock the stalagmite tests against.
    let stalactite_ok = stalactite.move_back_until_base_is_inside_stone(world, wind)?;
    let stalagmite_ok = stalagmite.move_back_until_base_is_inside_stone(world, wind)?;
    if stalactite_ok {
        stalactite.place_blocks(world, tags, random, wind)?;
    }
    if stalagmite_ok {
        stalagmite.place_blocks(world, tags, random, wind)?;
    }
    // `true` even when neither cone was placeable: the reference returns 1
    // unconditionally once the column scan succeeded.
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feature::block_predicate::PredicateWorld;
    use crate::rng::draws;
    use crate::rng::legacy::LegacyRandomSource;
    use crate::rng::worldgen_random::WorldgenRandom;
    use std::collections::BTreeMap;

    /// A slab cave: stone at and below `floor`, stone at and above `ceiling`,
    /// air between, over the whole horizontal extent any test reaches.
    struct Cave {
        blocks: BTreeMap<(i32, i32, i32), String>,
        surface: i32,
    }

    impl Cave {
        fn slab(floor: i32, ceiling: i32) -> Self {
            let mut blocks = BTreeMap::new();
            for x in -40..=40 {
                for z in -40..=40 {
                    for y in (floor - 8)..=floor {
                        blocks.insert((x, y, z), "minecraft:stone".to_string());
                    }
                    for y in ceiling..=(ceiling + 8) {
                        blocks.insert((x, y, z), "minecraft:stone".to_string());
                    }
                }
            }
            Self {
                blocks,
                surface: i32::MAX,
            }
        }

        fn solid() -> Self {
            let mut blocks = BTreeMap::new();
            for x in -8..=8 {
                for z in -8..=8 {
                    for y in -8..=8 {
                        blocks.insert((x, y, z), "minecraft:stone".to_string());
                    }
                }
            }
            Self {
                blocks,
                surface: i32::MAX,
            }
        }

        fn count(&self, name: &str) -> usize {
            self.blocks.values().filter(|v| v.as_str() == name).count()
        }
    }

    impl PredicateWorld for Cave {
        fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
            self.blocks
                .get(&(x, y, z))
                .cloned()
                .unwrap_or_else(|| "minecraft:air".to_string())
        }
        fn air_at(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
            Some(self.block_at(x, y, z) == "minecraft:air")
        }
    }

    impl SpeleothemWorld for Cave {
        fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
            self.blocks.insert((x, y, z), state.to_string());
        }
        fn is_water_at(&mut self, x: i32, y: i32, z: i32) -> bool {
            self.block_at(x, y, z).starts_with("minecraft:water")
        }
    }

    impl LargeDripstoneWorld for Cave {
        fn world_surface_wg(&mut self, _x: i32, _z: i32) -> i32 {
            self.surface
        }
    }

    fn tags() -> BlockTags {
        BlockTags::from_entries(BTreeMap::from([
            (
                "minecraft:base_stone_overworld".to_string(),
                vec![
                    "minecraft:stone".to_string(),
                    "minecraft:deepslate".to_string(),
                ],
            ),
            (
                "minecraft:dripstone_replaceable_blocks".to_string(),
                vec![
                    "minecraft:stone".to_string(),
                    "minecraft:deepslate".to_string(),
                    "minecraft:dripstone_block".to_string(),
                ],
            ),
        ]))
        .expect("tags")
    }

    /// `minecraft:large_dripstone`, exactly as the pinned jar's datapack
    /// writes it — including the absent `floor_to_ceiling_search_range`.
    fn shipped() -> LargeDripstoneConfig {
        LargeDripstoneConfig::parse(
            &serde_json::from_str(
                r##"{
                    "column_radius": {"type":"minecraft:clamped","max_inclusive":16,"min_inclusive":3,
                        "source":{"type":"minecraft:uniform","max_inclusive":19,"min_inclusive":3}},
                    "height_scale": {"type":"minecraft:uniform","max_exclusive":2.0,"min_inclusive":0.4},
                    "max_column_radius_to_cave_height_ratio": 0.33,
                    "min_bluntness_for_wind": 0.6,
                    "min_radius_for_wind": 4,
                    "replaceable_blocks": "#minecraft:dripstone_replaceable_blocks",
                    "stalactite_bluntness": {"type":"minecraft:uniform","max_exclusive":0.9,"min_inclusive":0.3},
                    "stalagmite_bluntness": {"type":"minecraft:uniform","max_exclusive":1.0,"min_inclusive":0.4},
                    "wind_speed": {"type":"minecraft:uniform","max_exclusive":0.3,"min_inclusive":0.0}
                }"##,
            )
            .expect("json"),
        )
        .expect("config")
    }

    #[test]
    fn the_search_range_defaults_to_thirty_because_the_datapack_omits_it() {
        // `Codec.intRange(1, 512).optionalFieldOf("floor_to_ceiling_search_range", 30)`.
        // A zero here would make `Column#scan` find no ends at all and the
        // feature would place nothing while reporting success.
        assert_eq!(shipped().floor_to_ceiling_search_range, 30);
        assert_eq!(shipped().min_radius_for_wind, 4);
        assert!((shipped().min_bluntness_for_wind - 0.6).abs() < 1e-6);
    }

    /// The one clamped `IntProvider` in the whole shipped datapack is this
    /// feature's, so its two bounds are only exercised here.
    #[test]
    fn the_clamped_column_radius_reports_the_tighter_of_the_two_bounds() {
        let config = shipped();
        assert_eq!(config.column_radius.min_value(), 3);
        // 16 from the clamp, not 19 from the source.
        assert_eq!(config.column_radius.max_value(), 16);
        // And the other way round: a clamp wider than its source is bounded
        // by the source. This is the case the old `max_value` got wrong and
        // no datapack reaches.
        let wide = IntProvider::parse(
            &serde_json::from_str(
                r#"{"type":"minecraft:clamped","min_inclusive":1,"max_inclusive":40,
                    "source":{"type":"minecraft:uniform","min_inclusive":5,"max_inclusive":12}}"#,
            )
            .expect("json"),
        )
        .expect("provider");
        assert_eq!(wide.min_value(), 5);
        assert_eq!(wide.max_value(), 12);
    }

    #[test]
    fn an_attempt_that_lands_in_rock_spends_no_randomness() {
        // The whole reason 4,569 refusals are not 4,569 equal desyncs: every
        // guard is above the first draw.
        let mut world = Cave::solid();
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(1));
        draws::arm();
        let placed = place(&mut world, &tags(), &shipped(), &mut random, (0, 0, 0)).expect("place");
        let spent = draws::take();
        assert!(!placed);
        assert_eq!(spent, 0);
    }

    #[test]
    fn a_cave_with_no_ceiling_in_range_spends_no_randomness() {
        // `Column#scan` returns a one-ended column, which is not a `Range`,
        // and the reference declines before reading a height.
        let mut world = Cave::slab(0, 400);
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(1));
        draws::arm();
        let placed =
            place(&mut world, &tags(), &shipped(), &mut random, (0, 10, 0)).expect("place");
        let spent = draws::take();
        assert!(!placed);
        assert_eq!(spent, 0);
    }

    #[test]
    fn a_cave_under_four_high_spends_no_randomness() {
        // floor 0, ceiling 4 -> height 3.
        let mut world = Cave::slab(0, 4);
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(1));
        draws::arm();
        let placed = place(&mut world, &tags(), &shipped(), &mut random, (0, 2, 0)).expect("place");
        let spent = draws::take();
        assert!(!placed);
        assert_eq!(spent, 0);
    }

    /// A cave three blocks tall in every direction, so both cones compute a
    /// height of zero, `moveBack…` walks zero steps and halves its radius to
    /// nothing, and `placeBlocks` is never reached. What is left is exactly
    /// the fixed prefix.
    ///
    /// `height_scale` is a *uniform* whose range is almost zero rather than a
    /// constant: a constant provider draws nothing, so the shortcut that makes
    /// the arithmetic vanish would also delete the two draws under test.
    fn flat_scale() -> LargeDripstoneConfig {
        let mut config = shipped();
        config.height_scale = FloatProvider::parse(
            &serde_json::from_str(
                r#"{"type":"minecraft:uniform","min_inclusive":0.0,"max_exclusive":0.0001}"#,
            )
            .expect("json"),
        )
        .expect("provider");
        config
    }

    fn slab_20() -> Cave {
        Cave::slab(0, 20)
    }

    #[test]
    fn a_placement_spends_five_fixed_draws_before_the_wind_or_the_blocks() {
        let mut config = flat_scale();
        // No wind: `min_radius_for_wind` above any radius this config can
        // produce, so the two wind draws cannot fire.
        config.min_radius_for_wind = 99;
        let mut world = slab_20();
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(42));
        draws::arm();
        let placed = place(&mut world, &tags(), &config, &mut random, (0, 10, 0)).expect("place");
        let spent = draws::take();
        assert!(placed, "a scanned Range always returns true");
        assert_eq!(spent, 5, "radius, then bluntness+scale twice");
        assert_eq!(world.count("minecraft:dripstone_block"), 0);
    }

    /// The wind is two draws and it is all-or-nothing across the pair.
    #[test]
    fn the_wind_costs_exactly_two_draws_and_only_when_both_cones_qualify() {
        let spend = |min_radius: i32, min_bluntness: f32| -> u64 {
            let mut config = flat_scale();
            config.min_radius_for_wind = min_radius;
            config.min_bluntness_for_wind = min_bluntness;
            let mut world = slab_20();
            let mut random = WorldgenRandom::new(LegacyRandomSource::new(42));
            draws::arm();
            place(&mut world, &tags(), &config, &mut random, (0, 10, 0)).expect("place");
            draws::take()
        };
        // Nothing qualifies.
        assert_eq!(spend(99, 0.6), 5);
        // Both qualify: radius and bluntness thresholds at their floor.
        assert_eq!(spend(0, -1.0), 7);
        // The stalactite's bluntness is drawn from 0.3..0.9 and the
        // stalagmite's from 0.4..1.0, so a threshold above 0.9 can only be
        // met by the stalagmite — and one cone qualifying is not enough.
        assert_eq!(spend(0, 0.95), 5);
    }

    /// Independent golden values for `SpeleothemUtils#getSpeleothemHeight`,
    /// computed from the bytecode's arithmetic in a different language, so a
    /// transposed term or a wrong width shows up here rather than in a world
    /// diff.
    #[test]
    fn the_height_curve_matches_its_bytecode() {
        let cases = [
            (
                0.0_f64,
                8.0_f64,
                1.2_f64,
                0.5_f64,
                29.130_564_043_560_69_f64,
            ),
            (4.0, 8.0, 1.2, 0.5, 7.508_669_980_321_343_5),
            (8.0, 8.0, 1.2, 0.5, 0.001_537_698_398_139_386_6),
            (0.0, 16.0, 2.0, 0.9, 100.442_163_633_538_58),
            (12.0, 16.0, 0.4, 0.3, 2.024_431_908_132_187_5),
            (0.0, 3.0, 0.4, 0.85, 1.720_204_996_453_696_5),
        ];
        for (radius, max_radius, scale, bluntness, expect) in cases {
            let got = get_speleothem_height(radius, max_radius, scale, bluntness);
            assert!(
                (got - expect).abs() < 1e-12,
                "h({radius}, {max_radius}, {scale}, {bluntness}) = {got}, expected {expect}"
            );
        }
        // `bluntness` is a floor on the radius, not a term: anything under it
        // gives the same answer as the axis.
        assert_eq!(
            get_speleothem_height(0.0, 8.0, 1.2, 0.5),
            get_speleothem_height(0.4, 8.0, 1.2, 0.5)
        );
    }

    #[test]
    fn the_embedding_ring_is_sampled_every_six_over_radius_radians() {
        let mut world = Cave::solid();
        assert!(is_circle_mostly_embedded_in_stone(&mut world, (0, 0, 0), 4).expect("ring"));
        // The five sampled angles at radius 4 are 0.0, 1.5, 3.0, 4.5 and 6.0,
        // which is not a full circle of positions — the last one lands at
        // (3, 0, -1). Opening exactly that block refuses the ring, which is
        // what makes this a test of the sampling and not of the radius.
        let mut sparse = Cave::solid();
        sparse.blocks.remove(&(3, 0, -1));
        assert!(!is_circle_mostly_embedded_in_stone(&mut sparse, (0, 0, 0), 4).expect("ring"));
        // The centre is tested before any ring position.
        let mut hollow = Cave::solid();
        hollow.blocks.remove(&(0, 0, 0));
        assert!(!is_circle_mostly_embedded_in_stone(&mut hollow, (0, 0, 0), 4).expect("ring"));
    }

    #[test]
    fn the_wind_offset_is_clamped_and_scales_with_distance_from_the_origin() {
        // One block of lean per block of height, capped at three.
        let wind = WindOffsetter {
            origin_y: 100,
            wind: Some((1.0, -1.0)),
            max_offset: 3,
        };
        assert_eq!(wind.offset((0, 100, 0)), (0, 100, 0));
        assert_eq!(wind.offset((0, 98, 0)), (2, 98, -2));
        assert_eq!(wind.offset((0, 90, 0)), (3, 90, -3));
        // Above the origin the lean reverses, and clamps the other way.
        assert_eq!(wind.offset((0, 110, 0)), (-3, 110, 3));
        // `noWind()` is the identity at every height.
        assert_eq!(WindOffsetter::none().offset((5, 90, -5)), (5, 90, -5));
    }

    #[test]
    fn a_cone_places_dripstone_and_shrinks_its_radius_into_thick_rock() {
        let mut world = Cave::slab(0, 20);
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(7));
        let placed =
            place(&mut world, &tags(), &shipped(), &mut random, (0, 10, 0)).expect("place");
        assert!(placed);
        assert!(
            world.count("minecraft:dripstone_block") > 0,
            "a cave with eight blocks of rock on each side embeds a base"
        );
    }

    #[test]
    fn a_stalagmite_stops_at_the_world_surface_and_a_stalactite_does_not() {
        let tall = |surface: i32| -> usize {
            // 40 blocks, not 60: `floor_to_ceiling_search_range` is 30, so a
            // taller cave has no ceiling within range and places nothing at
            // all — which would make this test pass for the wrong reason.
            let mut world = Cave::slab(0, 40);
            world.surface = surface;
            let mut random = WorldgenRandom::new(LegacyRandomSource::new(7));
            place(&mut world, &tags(), &shipped(), &mut random, (0, 20, 0)).expect("place");
            world.count("minecraft:dripstone_block")
        };
        let unlimited = tall(i32::MAX);
        // A surface below the cave truncates every column of the upward cone
        // at its first block, and leaves the hanging one untouched.
        let truncated = tall(-100);
        assert!(
            unlimited > truncated,
            "{unlimited} should exceed {truncated}"
        );
    }
}
