//! The crimson and warped forests' own features: `nether_forest_vegetation`,
//! `huge_fungus`, `weeping_vines` and `twisting_vines`.
//!
//! Reference (26.2 bytecode, behaviour ported from `javap -p -c -constants`):
//! `NetherForestVegetationFeature`, `HugeFungusFeature`,
//! `WeepingVinesFeature`, `TwistingVinesFeature`, and `Mth.nextInt`.
//!
//! # Why these four together
//!
//! They are every feature the two forest biomes list that this port did not
//! place, and they write into each other: a huge fungus hangs
//! `WeepingVinesFeature.placeWeepingVinesColumn` off its own hat, and the
//! roof patches of `weeping_vines` and the hat both write `nether_wart_block`.
//! Porting one without the others leaves a residual that cannot be charged to
//! a single feature.
//!
//! # The draw orders that are easy to get wrong
//!
//! * `NetherForestVegetationFeature` draws the **offset and then the state**,
//!   and draws the state **before** it asks whether the position is empty —
//!   so a weighted provider spends its `nextInt(totalWeight)` on every one of
//!   the `spreadWidth²` tries, placed or not.
//! * Every offset is a Java argument list, evaluated left to right:
//!   `offset(nextInt(w) - nextInt(w), nextInt(h) - nextInt(h), ...)` is six
//!   draws in x, x, y, y, z, z order.
//! * `Mth.nextInt(random, min, max)` returns `min` **without drawing** when
//!   `min >= max`. It is not `nextIntBetweenInclusive`, which always draws.
//! * `HugeFungusFeature.placeHat` draws `nextInt(3)` once per hat layer for
//!   the radius, before any block of that layer is visited.
//! * `placeHatDropBlock` compares `nextFloat()` *widened to double* against
//!   `0.15d`; every other chance in the class is a float comparison.
//! * The weeping column starts at step 0, the twisting column at step 1, and
//!   neither stops at an occupied block — it moves past it and keeps going.

use super::block_predicate::{BlockPredicate, NeedsBehaviour, PredicateWorld};
use super::configured::Unsupported;
use super::nether::NetherWorld;
use super::placement::PlacementWorld;
use super::state_provider::StateProvider;
use super::state_spec::DefaultStates;
use crate::rng::RandomSource;
use crate::tags::BlockTags;

/// The six directions in `Direction.values()` order: down, up, north, south,
/// west, east.
const DIRECTIONS: [(i32, i32, i32); 6] = [
    (0, -1, 0),
    (0, 1, 0),
    (0, 0, -1),
    (0, 0, 1),
    (-1, 0, 0),
    (1, 0, 0),
];

const NETHERRACK: &str = "minecraft:netherrack";
const NETHER_WART_BLOCK: &str = "minecraft:nether_wart_block";
const WARPED_NYLIUM: &str = "minecraft:warped_nylium";
const WARPED_WART_BLOCK: &str = "minecraft:warped_wart_block";
const AIR: &str = "minecraft:air";

fn plain(state: &str) -> &str {
    state.split_once('[').map_or(state, |(name, _)| name)
}

/// `BlockState#isAir` — exactly the three blocks built with `Properties#air`.
fn is_air(state: &str) -> bool {
    matches!(
        plain(state),
        "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
    )
}

fn empty<W: PredicateWorld + ?Sized>(world: &mut W, pos: (i32, i32, i32)) -> bool {
    let state = world.block_at(pos.0, pos.1, pos.2);
    is_air(&state)
}

fn is_block<W: PredicateWorld + ?Sized>(world: &mut W, pos: (i32, i32, i32), block: &str) -> bool {
    plain(&world.block_at(pos.0, pos.1, pos.2)) == block
}

/// `Mth.nextInt(random, min, max)`: `min` with **no draw** when
/// `min >= max`, otherwise `nextInt(max - min + 1) + min`.
fn mth_next_int<R: RandomSource + ?Sized>(random: &mut R, min: i32, max: i32) -> i32 {
    if min >= max {
        min
    } else {
        random.next_i32_bound(max - min + 1) + min
    }
}

fn int_field(config: &serde_json::Value, name: &str) -> Result<i32, String> {
    config
        .get(name)
        .and_then(serde_json::Value::as_i64)
        .and_then(|v| i32::try_from(v).ok())
        .ok_or_else(|| format!("config has no integer `{name}`: {config}"))
}

fn needs(e: NeedsBehaviour) -> Unsupported {
    Unsupported(e.0)
}

// ---------------------------------------------------------------------------
// nether_forest_vegetation

/// `NetherForestVegetationConfig`: a state provider and the two spreads.
#[derive(Clone, Debug, PartialEq)]
pub struct VegetationConfig {
    pub provider: StateProvider,
    pub spread_width: i32,
    pub spread_height: i32,
}

impl VegetationConfig {
    /// # Errors
    /// If a field is missing or the provider is not one this port reads.
    pub fn parse(config: &serde_json::Value, states: &dyn DefaultStates) -> Result<Self, String> {
        let provider = config
            .get("state_provider")
            .ok_or_else(|| format!("nether_forest_vegetation has no `state_provider`: {config}"))?;
        Ok(Self {
            provider: StateProvider::parse(provider, states)?,
            spread_width: int_field(config, "spread_width")?,
            spread_height: int_field(config, "spread_height")?,
        })
    }

    #[must_use]
    pub fn is_supported(&self) -> bool {
        self.provider.is_supported()
    }
}

/// `NetherForestVegetationFeature.place`.
///
/// # Errors
/// If the provider or a `canSurvive` rule needs behaviour this port lacks.
pub fn place_vegetation<W, R>(
    world: &mut W,
    tags: &BlockTags,
    config: &VegetationConfig,
    random: &mut R,
    origin: (i32, i32, i32),
) -> Result<bool, Unsupported>
where
    W: NetherWorld + ?Sized,
    R: RandomSource,
{
    let (x, y, z) = origin;
    let below = world.block_at(x, y - 1, z);
    if !tags.contains("minecraft:nylium", plain(&below)) {
        return Ok(false);
    }
    // `y < getMinY() + 1 || y + 1 > getMaxY()` — the level's inclusive top.
    if y < world.min_y() + 1 || y + 1 > world.max_y() {
        return Ok(false);
    }
    let (w, h) = (config.spread_width, config.spread_height);
    let mut placed = 0;
    for _ in 0..w * w {
        let dx = random.next_i32_bound(w) - random.next_i32_bound(w);
        let dy = random.next_i32_bound(h) - random.next_i32_bound(h);
        let dz = random.next_i32_bound(w) - random.next_i32_bound(w);
        let pos = (x + dx, y + dy, z + dz);
        // Drawn before the emptiness test, so it is spent on every try.
        let state = config
            .provider
            .state(world, tags, random, pos)
            .map_err(needs)?;
        if empty(world, pos) && pos.1 > world.min_y() {
            let survives = world
                .would_survive(&state, pos.0, pos.1, pos.2)
                .ok_or_else(|| Unsupported(format!("canSurvive for {state}")))?;
            if survives {
                world.set_block(pos.0, pos.1, pos.2, &state);
                placed += 1;
            }
        }
    }
    Ok(placed > 0)
}

// ---------------------------------------------------------------------------
// huge_fungus

/// `HugeFungusConfiguration`.
#[derive(Clone, Debug, PartialEq)]
pub struct HugeFungusConfig {
    /// `validBaseState`, as a **block**: the feature tests
    /// `below.is(validBaseState.getBlock())`, so its properties are never read.
    pub valid_base: String,
    pub stem: String,
    pub hat: String,
    pub decor: String,
    pub replaceable: BlockPredicate,
    pub planted: bool,
}

impl HugeFungusConfig {
    /// # Errors
    /// If a state or the replaceable predicate does not parse.
    pub fn parse(config: &serde_json::Value, states: &dyn DefaultStates) -> Result<Self, String> {
        let state = |name: &str| -> Result<String, String> {
            let spec = config
                .get(name)
                .ok_or_else(|| format!("huge_fungus has no `{name}`: {config}"))?;
            super::state_spec::canonical(spec, states)
        };
        let replaceable = config
            .get("replaceable_blocks")
            .ok_or_else(|| format!("huge_fungus has no `replaceable_blocks`: {config}"))?;
        Ok(Self {
            valid_base: plain(&state("valid_base_block")?).to_string(),
            stem: state("stem_state")?,
            hat: state("hat_state")?,
            decor: state("decor_state")?,
            replaceable: BlockPredicate::parse(replaceable)?,
            planted: config
                .get("planted")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
        })
    }

    /// The planted variant is a bonemeal path: it calls
    /// `WorldGenLevel.destroyBlock`, which drops items and has no meaning in
    /// a decorating region. No worldgen document reaches it, and it is
    /// refused rather than approximated.
    #[must_use]
    pub fn is_supported(&self) -> bool {
        !self.planted && self.replaceable.is_supported()
    }
}

/// `HugeFungusFeature.isReplaceable`: `canBeReplaced()`, or the configured
/// predicate when `checkConfig` is set.
fn fungus_replaceable<W: PredicateWorld + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    config: &HugeFungusConfig,
    pos: (i32, i32, i32),
    check_config: bool,
) -> Result<bool, Unsupported> {
    let replaceable = world
        .replaceable_at(pos.0, pos.1, pos.2)
        .ok_or_else(|| Unsupported("huge_fungus: canBeReplaced".to_string()))?;
    if replaceable {
        return Ok(true);
    }
    if check_config {
        return config.replaceable.test(world, tags, pos).map_err(needs);
    }
    Ok(false)
}

/// `HugeFungusFeature.place`.
///
/// # Errors
/// If a replaceability question cannot be answered, or the config is planted.
pub fn place_huge_fungus<W, R>(
    world: &mut W,
    tags: &BlockTags,
    config: &HugeFungusConfig,
    random: &mut R,
    origin: (i32, i32, i32),
) -> Result<bool, Unsupported>
where
    W: NetherWorld + PlacementWorld + ?Sized,
    R: RandomSource,
{
    if config.planted {
        return Err(Unsupported("huge_fungus planted=true".to_string()));
    }
    let (x, y, z) = origin;
    if !is_block(world, (x, y - 1, z), &config.valid_base) {
        return Ok(false);
    }
    let mut height = mth_next_int(random, 4, 13);
    if random.next_i32_bound(12) == 0 {
        height *= 2;
    }
    // `!planted` only: `origin.y + height + 1 >= chunkGenerator.getGenDepth()`.
    if y + height + 1 >= world.gen_depth() {
        return Ok(false);
    }
    let huge = random.next_f32() < 0.06;
    world.set_block(x, y, z, AIR);
    place_stem(world, tags, config, random, origin, height, huge)?;
    place_hat(world, tags, config, random, origin, height, huge)?;
    Ok(true)
}

fn place_stem<W, R>(
    world: &mut W,
    tags: &BlockTags,
    config: &HugeFungusConfig,
    random: &mut R,
    origin: (i32, i32, i32),
    height: i32,
    huge: bool,
) -> Result<(), Unsupported>
where
    W: NetherWorld + ?Sized,
    R: RandomSource,
{
    let r = i32::from(huge);
    for dx in -r..=r {
        for dz in -r..=r {
            let corner = huge && dx.abs() == r && dz.abs() == r;
            for dy in 0..height {
                let pos = (origin.0 + dx, origin.1 + dy, origin.2 + dz);
                if !fungus_replaceable(world, tags, config, pos, true)? {
                    continue;
                }
                if corner {
                    if random.next_f32() < 0.1 {
                        world.set_block(pos.0, pos.1, pos.2, &config.stem);
                    }
                } else {
                    world.set_block(pos.0, pos.1, pos.2, &config.stem);
                }
            }
        }
    }
    Ok(())
}

fn place_hat<W, R>(
    world: &mut W,
    tags: &BlockTags,
    config: &HugeFungusConfig,
    random: &mut R,
    origin: (i32, i32, i32),
    height: i32,
    huge: bool,
) -> Result<(), Unsupported>
where
    W: NetherWorld + ?Sized,
    R: RandomSource,
{
    let is_wart = plain(&config.hat) == NETHER_WART_BLOCK;
    let hat_height = (random.next_i32_bound(1 + height / 3) + 5).min(height);
    let hat_start = height - hat_height;
    for dy in hat_start..=height {
        let mut radius = if dy < height - random.next_i32_bound(3) {
            2
        } else {
            1
        };
        if hat_height > 8 && dy < hat_start + 4 {
            radius = 3;
        }
        if huge {
            radius += 1;
        }
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                let edge_x = dx == -radius || dx == radius;
                let edge_z = dz == -radius || dz == radius;
                let inside = !edge_x && !edge_z && dy != height;
                let corner = edge_x && edge_z;
                let low = dy < hat_start + 3;
                let pos = (origin.0 + dx, origin.1 + dy, origin.2 + dz);
                if !fungus_replaceable(world, tags, config, pos, false)? {
                    continue;
                }
                if low {
                    if !inside {
                        place_hat_drop_block(world, config, random, pos, is_wart);
                    }
                } else if inside {
                    place_hat_block(
                        world,
                        config,
                        random,
                        pos,
                        0.1,
                        0.2,
                        if is_wart { 0.1 } else { 0.0 },
                    );
                } else if corner {
                    place_hat_block(
                        world,
                        config,
                        random,
                        pos,
                        0.01,
                        0.7,
                        if is_wart { 0.083 } else { 0.0 },
                    );
                } else {
                    place_hat_block(
                        world,
                        config,
                        random,
                        pos,
                        0.0005,
                        0.98,
                        if is_wart { 0.07 } else { 0.0 },
                    );
                }
            }
        }
    }
    Ok(())
}

fn place_hat_block<W, R>(
    world: &mut W,
    config: &HugeFungusConfig,
    random: &mut R,
    pos: (i32, i32, i32),
    decor_chance: f32,
    hat_chance: f32,
    vine_chance: f32,
) where
    W: NetherWorld + ?Sized,
    R: RandomSource,
{
    if random.next_f32() < decor_chance {
        world.set_block(pos.0, pos.1, pos.2, &config.decor);
    } else if random.next_f32() < hat_chance {
        world.set_block(pos.0, pos.1, pos.2, &config.hat);
        if random.next_f32() < vine_chance {
            try_place_weeping_vines(world, random, pos);
        }
    }
}

fn place_hat_drop_block<W, R>(
    world: &mut W,
    config: &HugeFungusConfig,
    random: &mut R,
    pos: (i32, i32, i32),
    is_wart: bool,
) where
    W: NetherWorld + ?Sized,
    R: RandomSource,
{
    let hat_block = plain(&config.hat).to_string();
    if is_block(world, (pos.0, pos.1 - 1, pos.2), &hat_block) {
        world.set_block(pos.0, pos.1, pos.2, &config.hat);
    } else if f64::from(random.next_f32()) < 0.15 {
        world.set_block(pos.0, pos.1, pos.2, &config.hat);
        if is_wart && random.next_i32_bound(11) == 0 {
            try_place_weeping_vines(world, random, pos);
        }
    }
}

/// `HugeFungusFeature.tryPlaceWeepingVines`.
fn try_place_weeping_vines<W, R>(world: &mut W, random: &mut R, pos: (i32, i32, i32))
where
    W: NetherWorld + ?Sized,
    R: RandomSource,
{
    let below = (pos.0, pos.1 - 1, pos.2);
    if !empty(world, below) {
        return;
    }
    let mut length = mth_next_int(random, 1, 5);
    if random.next_i32_bound(7) == 0 {
        length *= 2;
    }
    weeping_vines_column(world, random, below, length, 23, 25);
}

// ---------------------------------------------------------------------------
// weeping_vines

fn is_roof<W: PredicateWorld + ?Sized>(world: &mut W, pos: (i32, i32, i32)) -> bool {
    let state = world.block_at(pos.0, pos.1, pos.2);
    matches!(plain(&state), NETHERRACK | NETHER_WART_BLOCK)
}

/// `WeepingVinesFeature.place`.
pub fn place_weeping_vines<W, R>(world: &mut W, random: &mut R, origin: (i32, i32, i32)) -> bool
where
    W: NetherWorld + ?Sized,
    R: RandomSource,
{
    if !empty(world, origin) {
        return false;
    }
    if !is_roof(world, (origin.0, origin.1 + 1, origin.2)) {
        return false;
    }
    place_roof_nether_wart(world, random, origin);
    place_roof_weeping_vines(world, random, origin);
    true
}

fn place_roof_nether_wart<W, R>(world: &mut W, random: &mut R, origin: (i32, i32, i32))
where
    W: NetherWorld + ?Sized,
    R: RandomSource,
{
    world.set_block(origin.0, origin.1, origin.2, NETHER_WART_BLOCK);
    for _ in 0..200 {
        let dx = random.next_i32_bound(6) - random.next_i32_bound(6);
        let dy = random.next_i32_bound(2) - random.next_i32_bound(5);
        let dz = random.next_i32_bound(6) - random.next_i32_bound(6);
        let pos = (origin.0 + dx, origin.1 + dy, origin.2 + dz);
        if !empty(world, pos) {
            continue;
        }
        let mut count = 0;
        for d in DIRECTIONS {
            if is_roof(world, (pos.0 + d.0, pos.1 + d.1, pos.2 + d.2)) {
                count += 1;
            }
            if count > 1 {
                break;
            }
        }
        if count == 1 {
            world.set_block(pos.0, pos.1, pos.2, NETHER_WART_BLOCK);
        }
    }
}

fn place_roof_weeping_vines<W, R>(world: &mut W, random: &mut R, origin: (i32, i32, i32))
where
    W: NetherWorld + ?Sized,
    R: RandomSource,
{
    for _ in 0..100 {
        let dx = random.next_i32_bound(8) - random.next_i32_bound(8);
        let dy = random.next_i32_bound(2) - random.next_i32_bound(7);
        let dz = random.next_i32_bound(8) - random.next_i32_bound(8);
        let pos = (origin.0 + dx, origin.1 + dy, origin.2 + dz);
        if !empty(world, pos) || !is_roof(world, (pos.0, pos.1 + 1, pos.2)) {
            continue;
        }
        let mut length = mth_next_int(random, 1, 8);
        if random.next_i32_bound(6) == 0 {
            length *= 2;
        }
        if random.next_i32_bound(5) == 0 {
            length = 1;
        }
        weeping_vines_column(world, random, pos, length, 17, 25);
    }
}

/// `WeepingVinesFeature.placeWeepingVinesColumn` — downward, from step 0.
fn weeping_vines_column<W, R>(
    world: &mut W,
    random: &mut R,
    start: (i32, i32, i32),
    length: i32,
    min_age: i32,
    max_age: i32,
) where
    W: NetherWorld + ?Sized,
    R: RandomSource,
{
    let mut pos = start;
    for step in 0..=length {
        if empty(world, pos) {
            if step == length || !empty(world, (pos.0, pos.1 - 1, pos.2)) {
                let age = mth_next_int(random, min_age, max_age);
                world.set_block(
                    pos.0,
                    pos.1,
                    pos.2,
                    &format!("minecraft:weeping_vines[age={age}]"),
                );
                return;
            }
            world.set_block(pos.0, pos.1, pos.2, "minecraft:weeping_vines_plant");
        }
        pos.1 -= 1;
    }
}

// ---------------------------------------------------------------------------
// twisting_vines

/// `TwistingVinesConfig`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TwistingVinesConfig {
    pub spread_width: i32,
    pub spread_height: i32,
    pub max_height: i32,
}

impl TwistingVinesConfig {
    /// # Errors
    /// If a field is missing.
    pub fn parse(config: &serde_json::Value) -> Result<Self, String> {
        Ok(Self {
            spread_width: int_field(config, "spread_width")?,
            spread_height: int_field(config, "spread_height")?,
            max_height: int_field(config, "max_height")?,
        })
    }
}

/// `TwistingVinesFeature.isInvalidPlacementLocation`.
fn twisting_invalid<W: PredicateWorld + ?Sized>(world: &mut W, pos: (i32, i32, i32)) -> bool {
    if !empty(world, pos) {
        return true;
    }
    let below = world.block_at(pos.0, pos.1 - 1, pos.2);
    !matches!(
        plain(&below),
        NETHERRACK | WARPED_NYLIUM | WARPED_WART_BLOCK
    )
}

/// `TwistingVinesFeature.findFirstAirBlockAboveGround`: step down until the
/// block is not air, then back up one. `false` if it leaves the level first.
fn first_air_above_ground<W: NetherWorld + ?Sized>(
    world: &mut W,
    pos: &mut (i32, i32, i32),
) -> bool {
    loop {
        pos.1 -= 1;
        if pos.1 < world.min_y() || pos.1 > world.max_y() {
            return false;
        }
        if !empty(world, *pos) {
            break;
        }
    }
    pos.1 += 1;
    true
}

/// `TwistingVinesFeature.place`.
pub fn place_twisting_vines<W, R>(
    world: &mut W,
    config: TwistingVinesConfig,
    random: &mut R,
    origin: (i32, i32, i32),
) -> bool
where
    W: NetherWorld + ?Sized,
    R: RandomSource,
{
    if twisting_invalid(world, origin) {
        return false;
    }
    let (w, h) = (config.spread_width, config.spread_height);
    for _ in 0..w * w {
        let dx = mth_next_int(random, -w, w);
        let dy = mth_next_int(random, -h, h);
        let dz = mth_next_int(random, -w, w);
        let mut pos = (origin.0 + dx, origin.1 + dy, origin.2 + dz);
        if !first_air_above_ground(world, &mut pos) || twisting_invalid(world, pos) {
            continue;
        }
        let mut length = mth_next_int(random, 1, config.max_height);
        if random.next_i32_bound(6) == 0 {
            length *= 2;
        }
        if random.next_i32_bound(5) == 0 {
            length = 1;
        }
        twisting_vines_column(world, random, pos, length, 17, 25);
    }
    true
}

/// `TwistingVinesFeature.placeWeepingVinesColumn` — upward, from step **1**.
fn twisting_vines_column<W, R>(
    world: &mut W,
    random: &mut R,
    start: (i32, i32, i32),
    length: i32,
    min_age: i32,
    max_age: i32,
) where
    W: NetherWorld + ?Sized,
    R: RandomSource,
{
    let mut pos = start;
    for step in 1..=length {
        if empty(world, pos) {
            if step == length || !empty(world, (pos.0, pos.1 + 1, pos.2)) {
                let age = mth_next_int(random, min_age, max_age);
                world.set_block(
                    pos.0,
                    pos.1,
                    pos.2,
                    &format!("minecraft:twisting_vines[age={age}]"),
                );
                return;
            }
            world.set_block(pos.0, pos.1, pos.2, "minecraft:twisting_vines_plant");
        }
        pos.1 += 1;
    }
}
