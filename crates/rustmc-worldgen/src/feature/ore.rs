//! The ore feature — the blobs every ore in the world is made of.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `net.minecraft.world.level.levelgen.feature.OreFeature`,
//! `configurations.OreConfiguration`.
//!
//! Thirty of the 226 configured features are ores, and they are the most
//! load-bearing single feature type: everything below the surface that a
//! player digs for comes from here.
//!
//! # It is a string of overlapping spheres, not a blob
//!
//! `size` sample points are laid along a line whose direction is one random
//! draw, each with its own radius, and then any sphere entirely swallowed by
//! another is **deleted** (its radius set to -1). What survives is filled in.
//! That deletion pass is why an ore vein looks like a lumpy sausage rather
//! than a cloud, and skipping it would place more ore, not less.
//!
//! # This feature calls `Math.sin`, not `Mth.sin`
//!
//! Twice, at the top, to pick the line's direction — and unlike every carver
//! call, the result stays a `double` and is never narrowed to a float. So the
//! 1-ulp slack in `Math.sin`'s specification is not absorbed by a rounding.
//!
//! Rust's `f64::sin` and the JVM's disagree on **22 of 4,097** of the angles
//! this feature can produce, by 1 ulp. `golden_mth_sin` measures it and
//! ORACLE.md 5c records it. None of those differences moved a floored
//! coordinate in the probe that was run, which is a measurement and not a
//! proof.
//!
//! The `Mth.sin` inside `do_place` — the one shaping the radii — *is* the
//! table, and is bit-exact. Two different sines, twenty lines apart.
//!
//! # The draw count branches
//!
//! Three draws happen before the feature decides whether to run at all: the
//! direction and the two y offsets. Only then does it probe for a column
//! whose ocean floor is high enough, and bail if none is. So a feature that
//! places nothing still moves the stream by three — and by `size` more if it
//! got past the probe.

use crate::math::{floor, lerp, sin};
use crate::rng::RandomSource;
use crate::tags::BlockTags;

/// `RuleTest` — what an ore is allowed to replace.
///
/// Only the two kinds the ore configs use. Both take the random source in
/// vanilla and neither draws from it; the ones that do (`random_block_match`)
/// are not used by any ore.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuleTest {
    TagMatch { tag: String },
    BlockMatch { block: String },
}

impl RuleTest {
    #[must_use]
    pub fn test(&self, tags: &BlockTags, block: &str) -> bool {
        let plain = block.split_once('[').map_or(block, |(n, _)| n);
        match self {
            Self::TagMatch { tag } => tags.get(tag).is_some_and(|set| set.contains(plain)),
            Self::BlockMatch { block: wanted } => wanted == plain,
        }
    }

    /// # Errors
    /// If the predicate type is unknown or its field is missing.
    pub fn parse(value: &serde_json::Value) -> Result<Self, String> {
        let kind = value
            .get("predicate_type")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| format!("rule test has no `predicate_type`: {value}"))?;
        let field = |name: &str| -> Result<String, String> {
            value
                .get(name)
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| format!("{kind} has no `{name}`"))
        };
        match kind {
            "minecraft:tag_match" => Ok(Self::TagMatch { tag: field("tag")? }),
            "minecraft:block_match" => Ok(Self::BlockMatch {
                block: field("block")?,
            }),
            other => Err(format!("unsupported rule test `{other}`")),
        }
    }
}

/// `OreConfiguration`.
#[derive(Clone, Debug, PartialEq)]
pub struct OreConfig {
    /// Tried in order; the first whose rule matches wins.
    pub targets: Vec<(RuleTest, String)>,
    pub size: i32,
    /// Zero means "never check for air", which is the common case.
    pub discard_chance_on_air_exposure: f32,
}

impl OreConfig {
    /// # Errors
    /// If a field is missing or a rule test is unsupported.
    pub fn parse(
        config: &serde_json::Value,
        states: &dyn super::state_spec::DefaultStates,
    ) -> Result<Self, String> {
        let targets = config
            .get("targets")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| "ore config has no `targets`".to_string())?
            .iter()
            .map(|entry| {
                let rule = RuleTest::parse(
                    entry
                        .get("target")
                        .ok_or_else(|| "ore target has no `target`".to_string())?,
                )?;
                let state = super::state_spec::canonical(
                    entry
                        .get("state")
                        .ok_or_else(|| "ore target has no `state`".to_string())?,
                    states,
                )?;
                Ok((rule, state))
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Self {
            targets,
            size: config
                .get("size")
                .and_then(serde_json::Value::as_i64)
                .and_then(|v| i32::try_from(v).ok())
                .ok_or_else(|| "ore config has no integer `size`".to_string())?,
            discard_chance_on_air_exposure: config
                .get("discard_chance_on_air_exposure")
                .and_then(serde_json::Value::as_f64)
                .map(|v| {
                    #[allow(clippy::cast_possible_truncation)]
                    {
                        v as f32
                    }
                })
                .unwrap_or(0.0),
        })
    }
}

/// The world an ore feature reads and writes.
pub trait OreWorld {
    /// `level.getHeight(Types.OCEAN_FLOOR_WG, x, z)`.
    fn ocean_floor(&mut self, x: i32, z: i32) -> i32;
    fn block_at(&mut self, x: i32, y: i32, z: i32) -> String;
    fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str);
    /// `level.isOutsideBuildHeight(y)`.
    fn outside_build_height(&self, y: i32) -> bool;
    /// `level.ensureCanWrite(pos)` — whether this position is in a chunk the
    /// feature is allowed to touch.
    fn can_write(&self, x: i32, y: i32, z: i32) -> bool;
}

/// One sphere along the vein's line.
#[derive(Clone, Copy, Debug)]
struct Blob {
    x: f64,
    y: f64,
    z: f64,
    /// Negative means "swallowed by another sphere and deleted".
    radius: f64,
}

/// `OreFeature#place`. Returns whether any block was placed.
pub fn place<W: OreWorld + ?Sized, R: RandomSource>(
    world: &mut W,
    tags: &BlockTags,
    config: &OreConfig,
    random: &mut R,
    origin: (i32, i32, i32),
) -> bool {
    let (ox, oy, oz) = origin;

    // The three draws that happen whether or not the feature runs.
    let dir = random.next_f32() * std::f32::consts::PI;
    #[allow(clippy::cast_precision_loss)]
    let spread_xz = f64::from(config.size as f32 / 8.0);
    let max_radius = ceil(f64::from(
        #[allow(clippy::cast_precision_loss)]
        {
            (config.size as f32 / 16.0 * 2.0 + 1.0) / 2.0
        },
    ));

    // `Math.sin`/`Math.cos`, not the table. See the module docs.
    let angle = f64::from(dir);
    let x0 = f64::from(ox) + angle.sin() * spread_xz;
    let x1 = f64::from(ox) - angle.sin() * spread_xz;
    let z0 = f64::from(oz) + angle.cos() * spread_xz;
    let z1 = f64::from(oz) - angle.cos() * spread_xz;
    let y0 = f64::from(oy + random.next_i32_bound(3) - 2);
    let y1 = f64::from(oy + random.next_i32_bound(3) - 2);

    let x_start = ox - ceil(spread_xz) - max_radius;
    let y_start = oy - 2 - max_radius;
    let z_start = oz - ceil(spread_xz) - max_radius;
    let size_xz = 2 * (ceil(spread_xz) + max_radius);
    let size_y = 2 * (2 + max_radius);

    // Probe for any column in the box whose ocean floor reaches the bottom of
    // it. Returns on the **first** hit rather than collecting, so a vein high
    // above the terrain costs nothing beyond the scan.
    for x_probe in x_start..=x_start + size_xz {
        for z_probe in z_start..=z_start + size_xz {
            if y_start <= world.ocean_floor(x_probe, z_probe) {
                return do_place(
                    world, tags, config, random, x0, x1, z0, z1, y0, y1, x_start, y_start, z_start,
                    size_xz, size_y,
                );
            }
        }
    }
    false
}

/// `Mth#ceil(double)` — `(int) Math.ceil(v)`.
fn ceil(value: f64) -> i32 {
    #[allow(clippy::cast_possible_truncation)]
    {
        value.ceil() as i32
    }
}

/// One cell to narrate, from `RUSTMC_TRACE_ORE=x,y,z`.
///
/// A cell that disagrees with vanilla has two very different explanations and
/// the finished world cannot tell them apart: the blob reached the cell and
/// something refused the write, or the blob never reached it at all. The first
/// is a target-test bug, the second is the ellipsoid walk. This prints only
/// when a blob *reaches* the cell, so silence is itself the answer.
fn watching(x: i32, y: i32, z: i32) -> bool {
    use std::sync::OnceLock;
    static WATCH: OnceLock<Option<(i32, i32, i32)>> = OnceLock::new();
    let want = WATCH.get_or_init(|| {
        let raw = std::env::var("RUSTMC_TRACE_ORE").ok()?;
        let mut parts = raw.split(',').map(str::trim).map(str::parse::<i32>);
        Some((
            parts.next()?.ok()?,
            parts.next()?.ok()?,
            parts.next()?.ok()?,
        ))
    });
    *want == Some((x, y, z))
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn do_place<W: OreWorld + ?Sized, R: RandomSource>(
    world: &mut W,
    tags: &BlockTags,
    config: &OreConfig,
    random: &mut R,
    x0: f64,
    x1: f64,
    z0: f64,
    z1: f64,
    y0: f64,
    y1: f64,
    x_start: i32,
    y_start: i32,
    z_start: i32,
    size_xz: i32,
    size_y: i32,
) -> bool {
    let mut placed = 0usize;
    let size = config.size.max(0);
    #[allow(clippy::cast_sign_loss)]
    let mut blobs: Vec<Blob> = Vec::with_capacity(size as usize);

    for i in 0..size {
        #[allow(clippy::cast_precision_loss)]
        let step = i as f32 / size as f32;
        let step = f64::from(step);
        let xx = lerp(step, x0, x1);
        let yy = lerp(step, y0, y1);
        let zz = lerp(step, z0, z1);
        // `nextDouble() * size / 16.0`, left to right.
        #[allow(clippy::cast_precision_loss)]
        let ss = random.next_f64() * f64::from(size) / 16.0;
        // `Mth.sin` here — the table. `(sin + 1.0F)` is float arithmetic,
        // then it widens.
        #[allow(clippy::cast_possible_truncation)]
        let wave = f64::from(sin(f64::from(std::f32::consts::PI * step as f32)) + 1.0);
        let radius = (wave * ss + 1.0) / 2.0;
        blobs.push(Blob {
            x: xx,
            y: yy,
            z: zz,
            radius,
        });
    }

    // Delete any sphere entirely swallowed by another. `dr * dr > d²` with
    // `dr` signed decides *which* of the pair goes: the smaller one.
    for i in 0..size.saturating_sub(1) {
        #[allow(clippy::cast_sign_loss)]
        let i = i as usize;
        if blobs[i].radius <= 0.0 {
            continue;
        }
        #[allow(clippy::cast_sign_loss)]
        for j in (i + 1)..size as usize {
            if blobs[j].radius <= 0.0 {
                continue;
            }
            let dx = blobs[i].x - blobs[j].x;
            let dy = blobs[i].y - blobs[j].y;
            let dz = blobs[i].z - blobs[j].z;
            let dr = blobs[i].radius - blobs[j].radius;
            if dr * dr > dx * dx + dy * dy + dz * dz {
                if dr > 0.0 {
                    blobs[j].radius = -1.0;
                } else {
                    blobs[i].radius = -1.0;
                }
            }
        }
    }

    // One bit per position in the bounding box, so a block reached by two
    // spheres is tested once. The set is not an optimisation: `canPlaceOre`
    // can draw, so testing twice would move the stream.
    #[allow(clippy::cast_sign_loss)]
    let mut tested = vec![false; (size_xz.max(0) * size_y.max(0) * size_xz.max(0)) as usize];

    for blob in &blobs {
        if blob.radius < 0.0 {
            continue;
        }
        let r = blob.radius;
        let x_min = floor(blob.x - r).max(x_start);
        let y_min = floor(blob.y - r).max(y_start);
        let z_min = floor(blob.z - r).max(z_start);
        let x_max = floor(blob.x + r).max(x_min);
        let y_max = floor(blob.y + r).max(y_min);
        let z_max = floor(blob.z + r).max(z_min);

        for x in x_min..=x_max {
            let xd = (f64::from(x) + 0.5 - blob.x) / r;
            if xd * xd >= 1.0 {
                continue;
            }
            for y in y_min..=y_max {
                let yd = (f64::from(y) + 0.5 - blob.y) / r;
                if xd * xd + yd * yd >= 1.0 {
                    continue;
                }
                for z in z_min..=z_max {
                    let zd = (f64::from(z) + 0.5 - blob.z) / r;
                    if xd * xd + yd * yd + zd * zd >= 1.0 || world.outside_build_height(y) {
                        continue;
                    }
                    let index =
                        (x - x_start) + (y - y_start) * size_xz + (z - z_start) * size_xz * size_y;
                    let Ok(index) = usize::try_from(index) else {
                        continue;
                    };
                    if index >= tested.len() || tested[index] {
                        continue;
                    }
                    tested[index] = true;
                    let watched = watching(x, y, z);
                    if !world.can_write(x, y, z) {
                        if watched {
                            println!("ORE ({x},{y},{z}) reached, refused by can_write");
                        }
                        continue;
                    }
                    let existing = world.block_at(x, y, z);
                    let mut wrote = false;
                    for (rule, state) in &config.targets {
                        if can_place_ore(world, tags, config, random, rule, &existing, (x, y, z)) {
                            world.set_block(x, y, z, state);
                            placed += 1;
                            wrote = true;
                            if watched {
                                println!("ORE ({x},{y},{z}) reached, {existing} -> {state}");
                            }
                            break;
                        }
                    }
                    if watched && !wrote {
                        println!(
                            "ORE ({x},{y},{z}) reached, target test refused it; \
                             existing {existing}"
                        );
                    }
                }
            }
        }
    }

    placed > 0
}

/// `ScatteredOreFeature#place` — ancient debris, and nothing else in the game.
///
/// Shares `OreConfiguration` and `canPlaceOre` with [`place`] and shares no
/// geometry at all: there is no vein, no line between two ends, no sphere and
/// no `OCEAN_FLOOR_WG` probe. It draws a count, then throws that many single
/// blocks at a cloud around the origin, and **always returns true** whether
/// or not one of them landed.
///
/// The spread is the interesting part. `Math.round((nextFloat() -
/// nextFloat()) * spread)` is a triangular distribution about zero, and
/// `spread` is `min(i, 7)` — the *index* of the attempt. So the first block
/// lands on the origin exactly (spread 0, three draws that can only produce
/// 0), the next few land beside it, and only from the eighth onward is the
/// cloud its full width. That is what makes a debris cluster tight in the
/// middle and ragged at the edge, and it is why the draws cannot be hoisted.
///
/// Java's `Math.round(float)` is not Rust's `f32::round`: it is
/// `floor(x + 0.5)` with the half-way case going *up*, including for negative
/// values, so `-0.5` rounds to `0` and not to `-1`. See [`java_round_f32`].
pub fn place_scattered<W: OreWorld + ?Sized, R: RandomSource>(
    world: &mut W,
    tags: &BlockTags,
    config: &OreConfig,
    random: &mut R,
    origin: (i32, i32, i32),
) -> bool {
    let count = random.next_i32_bound(config.size + 1);
    for index in 0..count {
        let spread = index.min(7);
        // Three axes, three pairs of draws, always — `offsetTargetPos` runs
        // before anything about the position is tested.
        let dx = axis_offset(random, spread);
        let dy = axis_offset(random, spread);
        let dz = axis_offset(random, spread);
        let (x, y, z) = (origin.0 + dx, origin.1 + dy, origin.2 + dz);
        let existing = world.block_at(x, y, z);
        for (rule, state) in &config.targets {
            if can_place_ore(world, tags, config, random, rule, &existing, (x, y, z)) {
                world.set_block(x, y, z, state);
                break;
            }
        }
    }
    true
}

/// `ScatteredOreFeature#getRandomPlacementInOneAxisRelativeToOrigin`.
fn axis_offset<R: RandomSource>(random: &mut R, spread: i32) -> i32 {
    #[allow(clippy::cast_precision_loss)]
    let value = (random.next_f32() - random.next_f32()) * spread as f32;
    java_round_f32(value)
}

/// `Math.round(float)`, which is neither `f32::round` nor `floor(x + 0.5)`
/// evaluated in `f32`.
///
/// The JDK does this on the bits rather than in floating point, and the
/// difference is real for one value: `0x1.fffffep-2`, the float just below
/// one half. Adding `0.5f` to it rounds up to exactly `1.0f` in float
/// arithmetic, so the naive form answers 1 where Java answers 0. That value
/// is reachable here — `nextFloat() - nextFloat()` produces arbitrary floats
/// and `spread` scales them — so the bit form is the one ported.
fn java_round_f32(value: f32) -> i32 {
    #[allow(clippy::cast_possible_wrap)]
    let bits = value.to_bits() as i32;
    let biased_exponent = (bits & 0x7f80_0000) >> 23;
    // (SIGNIFICAND_WIDTH - 2 + EXP_BIAS) - biasedExp, with the float's
    // 24-bit significand and 127 bias.
    let shift = (24 - 2 + 127) - biased_exponent;
    if shift & -32 == 0 {
        let mut r = (bits & 0x007f_ffff) | 0x0080_0000;
        if bits < 0 {
            r = -r;
        }
        ((r >> shift) + 1) >> 1
    } else {
        // Java's `(int)` cast of a float: truncate toward zero, clamp to the
        // int range, NaN to zero — which is exactly what Rust's saturating
        // `as` does.
        #[allow(clippy::cast_possible_truncation)]
        {
            value as i32
        }
    }
}

/// `OreFeature#canPlaceOre`.
#[allow(clippy::too_many_arguments)]
fn can_place_ore<W: OreWorld + ?Sized, R: RandomSource>(
    world: &mut W,
    tags: &BlockTags,
    config: &OreConfig,
    random: &mut R,
    rule: &RuleTest,
    existing: &str,
    pos: (i32, i32, i32),
) -> bool {
    if !rule.test(tags, existing) {
        return false;
    }
    if should_skip_air_check(random, config.discard_chance_on_air_exposure) {
        return true;
    }
    !is_adjacent_to_air(world, pos)
}

/// `OreFeature#shouldSkipAirCheck`.
///
/// Draws **only** in the middle case. A chance of zero or one is decided
/// without touching the stream, which is why almost every ore in the game
/// costs no draw here.
fn should_skip_air_check<R: RandomSource>(random: &mut R, discard_chance: f32) -> bool {
    if discard_chance <= 0.0 {
        return true;
    }
    if discard_chance >= 1.0 {
        return false;
    }
    random.next_f32() >= discard_chance
}

/// `Feature#isAdjacentToAir` — `checkNeighbors` over `Direction.values()`
/// with `BlockState::isAir`.
///
/// **`isAir` is three blocks, not one.** `minecraft:air`,
/// `minecraft:cave_air` and `minecraft:void_air` all answer true, and the one
/// that matters underground is `cave_air`, which is what every carver writes.
/// Comparing against `"minecraft:air"` alone means an ore beside a cave is
/// never seen as exposed, so the buried variants — `ore_diamond_buried` and
/// `ore_lapis_buried`, both `discard_chance_on_air_exposure` **1.0**, which
/// vanilla refuses to place next to air at all — get placed anyway.
///
/// Measured, not reasoned: chunk (67,61) at seed 20260722 held exactly six
/// blocks a vanilla server does not, three gold, two lapis and one diamond,
/// and `worldgen-dump column` puts `cave_air` immediately above or below
/// every one of them. Always ours-has-ore and never the reverse, which is the
/// signature of a discard that never fires.
fn is_adjacent_to_air<W: OreWorld + ?Sized>(world: &mut W, pos: (i32, i32, i32)) -> bool {
    let (x, y, z) = pos;
    [
        (x - 1, y, z),
        (x + 1, y, z),
        (x, y - 1, z),
        (x, y + 1, z),
        (x, y, z - 1),
        (x, y, z + 1),
    ]
    .into_iter()
    .any(|(nx, ny, nz)| {
        let block = world.block_at(nx, ny, nz);
        let name = block.split('[').next().unwrap_or(&block);
        matches!(
            name,
            "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::{LegacyRandomSource, WorldgenRandom};
    use std::collections::BTreeMap;

    struct Stone {
        blocks: BTreeMap<(i32, i32, i32), String>,
        floor: i32,
    }

    impl OreWorld for Stone {
        fn ocean_floor(&mut self, _: i32, _: i32) -> i32 {
            self.floor
        }
        fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
            self.blocks
                .get(&(x, y, z))
                .cloned()
                .unwrap_or_else(|| "minecraft:stone".to_string())
        }
        fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str) {
            self.blocks.insert((x, y, z), name.to_string());
        }
        fn outside_build_height(&self, y: i32) -> bool {
            !(-64..320).contains(&y)
        }
        fn can_write(&self, _: i32, _: i32, _: i32) -> bool {
            true
        }
    }

    fn world() -> Stone {
        Stone {
            blocks: BTreeMap::new(),
            floor: 100,
        }
    }

    fn tags() -> BlockTags {
        BlockTags::from_entries(
            [(
                "minecraft:stone_ore_replaceables".to_string(),
                vec!["minecraft:stone".to_string()],
            )]
            .into_iter()
            .collect(),
        )
        .expect("tags")
    }

    fn config(size: i32) -> OreConfig {
        OreConfig {
            targets: vec![(
                RuleTest::TagMatch {
                    tag: "minecraft:stone_ore_replaceables".into(),
                },
                "minecraft:coal_ore".into(),
            )],
            size,
            discard_chance_on_air_exposure: 0.0,
        }
    }

    fn random(seed: i64) -> WorldgenRandom<LegacyRandomSource> {
        WorldgenRandom::new(LegacyRandomSource::new(seed))
    }

    fn ore_count(w: &Stone) -> usize {
        w.blocks
            .values()
            .filter(|b| *b == "minecraft:coal_ore")
            .count()
    }

    #[test]
    fn a_vein_places_ore_in_a_connected_lump() {
        let mut w = world();
        let placed = place(
            &mut w,
            &tags(),
            &config(17),
            &mut random(20_260_725),
            (8, 40, 8),
        );
        assert!(placed);
        let n = ore_count(&w);
        assert!((4..600).contains(&n), "placed {n} ore blocks");
        // Everything is near the origin — the line is at most `size/8` long.
        for (x, y, z) in w.blocks.keys() {
            assert!((x - 8).abs() < 12, "x={x} too far");
            assert!((y - 40).abs() < 12, "y={y} too far");
            assert!((z - 8).abs() < 12, "z={z} too far");
        }
    }

    #[test]
    fn the_three_setup_draws_happen_even_when_nothing_is_placed() {
        // The probe bails when no column's ocean floor reaches the box, but
        // the direction and both y offsets have already been drawn. A port
        // that checks first and draws second moves every later feature in
        // the chunk.
        let mut w = world();
        w.floor = -60; // far below the vein
        let mut r = random(5);
        let before = r.count();
        assert!(!place(&mut w, &tags(), &config(17), &mut r, (8, 200, 8)));
        assert_eq!(
            r.count() - before,
            3,
            "expected exactly the direction and two y offsets"
        );
        assert!(w.blocks.is_empty());
    }

    #[test]
    fn a_vein_that_runs_consumes_three_draws_plus_one_per_sample() {
        // `size` calls to nextDouble in do_place, and nextDouble is two
        // next(26)/next(27) draws on the legacy source.
        let mut w = world();
        let mut r = random(5);
        let before = r.count();
        place(&mut w, &tags(), &config(9), &mut r, (8, 40, 8));
        assert_eq!(
            r.count() - before,
            3 + 9 * 2,
            "three setup draws plus one nextDouble per sample point"
        );
    }

    #[test]
    fn a_zero_discard_chance_never_touches_the_stream() {
        // `discardChanceOnAirExposure <= 0` short-circuits before the draw,
        // which is why almost every ore in the game costs nothing here.
        let mut r = random(1);
        let before = r.count();
        assert!(should_skip_air_check(&mut r, 0.0));
        assert!(!should_skip_air_check(&mut r, 1.0));
        assert_eq!(r.count(), before, "neither extreme may draw");
        let _ = should_skip_air_check(&mut r, 0.5);
        assert_eq!(r.count(), before + 1, "the middle case draws once");
    }

    #[test]
    fn ore_only_replaces_what_its_rule_allows() {
        let mut w = world();
        // Fill the whole neighbourhood with something the tag excludes.
        for x in -8..24 {
            for y in 30..50 {
                for z in -8..24 {
                    w.blocks.insert((x, y, z), "minecraft:deepslate".into());
                }
            }
        }
        let placed = place(&mut w, &tags(), &config(17), &mut random(3), (8, 40, 8));
        assert!(!placed, "ore replaced a block its rule excludes");
        assert_eq!(ore_count(&w), 0);
    }

    #[test]
    fn a_block_match_rule_is_exact_and_ignores_state() {
        let tags = tags();
        let rule = RuleTest::BlockMatch {
            block: "minecraft:stone".into(),
        };
        assert!(rule.test(&tags, "minecraft:stone"));
        assert!(rule.test(&tags, "minecraft:stone[foo=bar]"));
        assert!(!rule.test(&tags, "minecraft:deepslate"));
    }

    #[test]
    fn the_real_ore_configs_parse() {
        let dir = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../oracle-work/vanilla-26.2/generated-data/data/minecraft/worldgen/configured_feature"
        ));
        if !rustmc_data::fixture::available(dir, "datagen extract", "ci/vanilla-server.sh data") {
            return;
        }
        let Some(extract) = crate::feature::state_spec::extract_or_skip() else {
            return;
        };
        let mut parsed = 0usize;
        for entry in std::fs::read_dir(dir).expect("readable") {
            let path = entry.expect("entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let json: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).expect("readable"))
                    .expect("json");
            if json["type"] != "minecraft:ore" {
                continue;
            }
            let config = OreConfig::parse(&json["config"], &extract)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert!(config.size > 0, "{}: size {}", path.display(), config.size);
            assert!(!config.targets.is_empty());
            parsed += 1;
        }
        assert!(parsed >= 30, "only {parsed} ore configs found");
        println!("ORE CONFIGS: {parsed} parsed");
    }
}
