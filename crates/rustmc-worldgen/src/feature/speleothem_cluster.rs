//! Speleothem clusters — a patch of stalactites and stalagmites in a cave.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `SpeleothemClusterFeature`, `SpeleothemClusterConfiguration`,
//! `SpeleothemUtils`, `net.minecraft.world.level.levelgen.Column`,
//! `ClampedNormalFloat`, `Mth#{clampedMap, normal, randomBetweenInclusive}`.
//!
//! Two configured features use this: `dripstone_cluster` and
//! `sulfur_spike_cluster`. It is by far the most draw-dense feature in the
//! port so far — one placement walks up to a 17x17 footprint and every column
//! in it spends at least three values whether or not anything is built.
//!
//! # The radius is sampled twice, and the loop is x-major
//!
//! ```text
//! height  = config.height().sample(random)
//! wetness = config.wetness().sample(random)
//! density = config.density().sample(random)
//! xRadius = config.radius().sample(random)
//! zRadius = config.radius().sample(random)     // the same provider, again
//! ```
//!
//! One `radius` field, two samples — the footprint is a rectangle, not a
//! square, and reusing one value would both change the shape and eat a draw.
//! Then `x` from `-xRadius..=xRadius` on the outside and `z` on the inside,
//! which fixes the order every column's draws arrive in.
//!
//! # A column spends three draws before it decides anything
//!
//! `placeColumn` draws `nextFloat()` for wetness, `nextDouble()` for the
//! stalactite and `nextDouble()` for the stalagmite **unconditionally**, then
//! tests the world. `sulfur_spike_cluster` has `wetness: 0.0`, so its wetness
//! coin can never pass — and it is still drawn, every column, every time.
//!
//! The one thing that escapes is a column whose `Column.scan` finds nothing,
//! or finds neither ceiling nor floor: those return before the first draw. So
//! the draw count of a cluster depends on the *shape of the cave* it lands in,
//! which is why this feature could not be checked until the carvers were.
//!
//! # `nextGaussian` is not one draw
//!
//! `wetness` on `dripstone_cluster` is a `clamped_normal`, and the height of
//! every speleothem goes through `randomBetweenBiased`, which is
//! `ClampedNormalFloat.sample` — a gaussian. Java's is Marsaglia polar: two
//! `nextDouble`s per attempt, retried until the sample lands in the unit
//! circle, and the second value of the accepted pair is *cached* for the next
//! call. So the first gaussian in a column costs two-or-more draws and the
//! second costs none. See [`crate::carver::providers`].
//!
//! # The pool lowers the floor
//!
//! A wet column with a placeable floor gets a water block *at* the floor and
//! the column's floor is then rebuilt one lower, so the stalagmite grows from
//! under the water rather than out of it. `Column.withFloor` reconstructs the
//! whole column, which is why this port models a column as two `Option`s and
//! rebuilds rather than mutating: `getHeight` is only present when both ends
//! are, and it is read at the end to decide whether the tips merge.

// Read once, not per column of every cluster. See `crate::trace_switch`.
crate::trace_switch!(trace_speleo_columns, "RUSTMC_TRACE_SPELEO_COLUMNS");
crate::trace_switch_value!(trace_speleo, "RUSTMC_TRACE_SPELEO");
use super::speleothem::{
    grow_speleothem, is_empty_or_water, is_water_fluid, neither_empty_nor_water, offset,
    place_base_block_if_possible, plain, scan, Column, NeedsRules, SpeleothemBlocks,
    SpeleothemWorld,
};
use super::state_spec::DefaultStates;
use crate::carver::providers::FloatProvider;
use crate::feature::int_provider::IntProvider;
use crate::math;
use crate::rng::RandomSource;
use crate::tags::BlockTags;

/// `Direction.DOWN` — index 0 of `Direction.values()`.
const DOWN: usize = 0;
/// `Direction.UP` — index 1.
const UP: usize = 1;
/// `Direction.Plane.HORIZONTAL`, in iteration order.
const HORIZONTAL: [usize; 4] = [2, 5, 3, 4];

/// `SpeleothemClusterConfiguration`.
#[derive(Clone, Debug, PartialEq)]
pub struct SpeleothemClusterConfig {
    pub floor_to_ceiling_search_range: i32,
    pub height: IntProvider,
    pub radius: IntProvider,
    pub max_stalagmite_stalactite_height_diff: i32,
    pub height_deviation: i32,
    pub speleothem_block_layer_thickness: IntProvider,
    pub wetness: FloatProvider,
    pub density: FloatProvider,
    pub chance_of_speleothem_at_max_distance_from_center: f32,
    pub max_distance_from_edge_affecting_chance_of_speleothem: i32,
    pub max_distance_from_center_affecting_height_bias: i32,
    pub base_block: String,
    pub pointed_block: String,
    pub replaceable: Vec<String>,
    pub replaceable_tag: Option<String>,
}

impl SpeleothemBlocks for SpeleothemClusterConfig {
    fn base_block(&self) -> &str {
        &self.base_block
    }
    fn pointed_block(&self) -> &str {
        &self.pointed_block
    }
    fn replaceable(&self) -> &[String] {
        &self.replaceable
    }
    fn replaceable_tag(&self) -> Option<&str> {
        self.replaceable_tag.as_deref()
    }
}

impl SpeleothemClusterConfig {
    /// # Errors
    /// If a field is missing or malformed. Every field of this codec is
    /// required — unlike `SpeleothemConfiguration`, none has a default — so a
    /// missing one is a real disagreement with the datapack rather than
    /// something to paper over.
    pub fn parse(config: &serde_json::Value, states: &dyn DefaultStates) -> Result<Self, String> {
        let int = |name: &str| -> Result<i32, String> {
            config
                .get(name)
                .and_then(serde_json::Value::as_i64)
                .and_then(|v| i32::try_from(v).ok())
                .ok_or_else(|| format!("speleothem cluster has no integer `{name}`"))
        };
        let float = |name: &str| -> Result<f32, String> {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "the codec's field is a float, compared against a float"
            )]
            let v = config
                .get(name)
                .and_then(serde_json::Value::as_f64)
                .map(|v| v as f32);
            v.ok_or_else(|| format!("speleothem cluster has no number `{name}`"))
        };
        let ints = |name: &str| -> Result<IntProvider, String> {
            IntProvider::parse(
                config
                    .get(name)
                    .ok_or_else(|| format!("speleothem cluster has no `{name}`"))?,
            )
        };
        let floats = |name: &str| -> Result<FloatProvider, String> {
            FloatProvider::parse(
                config
                    .get(name)
                    .ok_or_else(|| format!("speleothem cluster has no `{name}`"))?,
            )
        };
        let state = |name: &str| -> Result<String, String> {
            super::state_spec::canonical(
                config
                    .get(name)
                    .ok_or_else(|| format!("speleothem cluster has no `{name}`"))?,
                states,
            )
        };
        let replaceable_value = config
            .get("replaceable_blocks")
            .ok_or_else(|| "speleothem cluster has no `replaceable_blocks`".to_string())?;
        let (replaceable, replaceable_tag) = match replaceable_value {
            serde_json::Value::String(one) => match one.strip_prefix('#') {
                Some(tag) => (Vec::new(), Some(tag.to_string())),
                None => (vec![one.clone()], None),
            },
            serde_json::Value::Array(list) => (
                list.iter()
                    .map(|v| {
                        v.as_str()
                            .map(ToString::to_string)
                            .ok_or_else(|| "replaceable_blocks entry is not a name".to_string())
                    })
                    .collect::<Result<Vec<_>, String>>()?,
                None,
            ),
            other => {
                return Err(format!(
                    "replaceable_blocks is neither a name nor a list: {other}"
                ))
            }
        };
        Ok(Self {
            floor_to_ceiling_search_range: int("floor_to_ceiling_search_range")?,
            height: ints("height")?,
            radius: ints("radius")?,
            max_stalagmite_stalactite_height_diff: int("max_stalagmite_stalactite_height_diff")?,
            height_deviation: int("height_deviation")?,
            speleothem_block_layer_thickness: ints("speleothem_block_layer_thickness")?,
            wetness: floats("wetness")?,
            density: floats("density")?,
            chance_of_speleothem_at_max_distance_from_center: float(
                "chance_of_speleothem_at_max_distance_from_center",
            )?,
            max_distance_from_edge_affecting_chance_of_speleothem: int(
                "max_distance_from_edge_affecting_chance_of_speleothem",
            )?,
            max_distance_from_center_affecting_height_bias: int(
                "max_distance_from_center_affecting_height_bias",
            )?,
            base_block: state("base_block")?,
            pointed_block: state("pointed_block")?,
            replaceable,
            replaceable_tag,
        })
    }

    #[must_use]
    pub const fn is_supported(&self) -> bool {
        true
    }
}

/// `SpeleothemClusterFeature#place`.
///
/// # Errors
/// If the world cannot answer a block question.
pub fn place<W: SpeleothemWorld + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    config: &SpeleothemClusterConfig,
    random: &mut dyn RandomSource,
    origin: (i32, i32, i32),
) -> Result<bool, NeedsRules> {
    // The guard is before every draw, so a cluster that lands in rock costs
    // the stream nothing at all.
    if !is_empty_or_water(world, origin)? {
        return Ok(false);
    }
    let height = config.height.sample(random);
    let wetness = config.wetness.sample(random);
    let density = config.density.sample(random);
    let x_radius = config.radius.sample(random);
    let z_radius = config.radius.sample(random);

    for x in -x_radius..=x_radius {
        for z in -z_radius..=z_radius {
            let chance = chance_of_speleothem(x_radius, z_radius, x, z, config);
            let pos = (origin.0 + x, origin.1, origin.2 + z);
            place_column(
                world, tags, config, random, pos, x, z, wetness, chance, height, density,
            )?;
        }
    }
    Ok(true)
}

/// `SpeleothemClusterFeature#placeColumn`.
#[expect(
    clippy::too_many_arguments,
    reason = "ten parameters is the reference's own signature; naming them here \
              keeps the call site checkable against it"
)]
fn place_column<W: SpeleothemWorld + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    config: &SpeleothemClusterConfig,
    random: &mut dyn RandomSource,
    pos: (i32, i32, i32),
    x: i32,
    z: i32,
    wetness: f32,
    chance: f64,
    height: i32,
    density: f32,
) -> Result<(), NeedsRules> {
    let Some(scanned) = scan(
        world,
        pos,
        config.floor_to_ceiling_search_range,
        &mut neither_empty_nor_water,
    )?
    else {
        return Ok(());
    };
    let ceiling = scanned.ceiling;
    if ceiling.is_none() && scanned.floor.is_none() {
        return Ok(());
    }

    // Three coins, all spent before the world is consulted about any of them.
    let wet = random.next_f32() < wetness;
    let mut column = scanned;
    if wet {
        if let Some(floor_y) = scanned.floor {
            if can_place_pool(world, tags, config, (pos.0, floor_y, pos.2))? {
                // `withFloor(floor - 1)`: the pool sits *on* the old floor, so
                // the stalagmite has to start under it.
                column = Column {
                    floor: Some(floor_y - 1),
                    ceiling,
                };
                // `Blocks.WATER.defaultBlockState()` spelled whole: a bare
                // `minecraft:water` is a state the chunk encoder has no network
                // id for, so a served dripstone cave with a wet floor failed to
                // encode (seed 2024 chunk (2,-18), `served_structures`).
                world.set_block(pos.0, floor_y, pos.2, "minecraft:water[level=0]");
            }
        }
    }
    let floor = column.floor;

    let wants_stalactite = random.next_f64() < chance;
    let mut stalactite = 0;
    // The cap handed to the height sampler as its `maxHeight`. Traced because
    // every other input to that sampler has now been read against the
    // bytecode and matches, so a wrong height has to come from `density` or
    // from this — and neither is visible in the blocks.
    let mut stalactite_cap = -1;
    if let Some(ceiling_y) = ceiling {
        if wants_stalactite && !is_lava(world, (pos.0, ceiling_y, pos.2)) {
            let thickness = config.speleothem_block_layer_thickness.sample(random);
            replace_blocks_with_base_blocks(
                world,
                tags,
                config,
                (pos.0, ceiling_y, pos.2),
                thickness,
                UP,
            );
            // Capped at the gap, so a stalactite cannot reach past the floor
            // before the merge step gets a say.
            let cap = floor.map_or(height, |floor_y| height.min(ceiling_y - floor_y));
            stalactite_cap = cap;
            stalactite = speleothem_height(random, x, z, density, cap, config);
        }
    }

    let wants_stalagmite = random.next_f64() < chance;
    let mut stalagmite = 0;
    if let Some(floor_y) = floor {
        if wants_stalagmite && !is_lava(world, (pos.0, floor_y, pos.2)) {
            let thickness = config.speleothem_block_layer_thickness.sample(random);
            replace_blocks_with_base_blocks(
                world,
                tags,
                config,
                (pos.0, floor_y, pos.2),
                thickness,
                DOWN,
            );
            stalagmite = if ceiling.is_some() {
                // With a ceiling the stalagmite is drawn *relative* to the
                // stalactite — a small integer offset — rather than sampled
                // afresh. That is what makes the pairs in a cluster look like
                // they belong to each other, and it is a different number of
                // draws from the branch below.
                0.max(
                    stalactite
                        + random_between_inclusive(
                            random,
                            -config.max_stalagmite_stalactite_height_diff,
                            config.max_stalagmite_stalactite_height_diff,
                        ),
                )
            } else {
                speleothem_height(random, x, z, density, height, config)
            };
        }
    }

    // If the two would overlap, one draw decides where the join is and both
    // heights are recomputed from it. Note it is `hi + 1`, so the split can
    // land one past the top of the overlap.
    let (mut stalactite, mut stalagmite) = (stalactite, stalagmite);
    if let (Some(ceiling_y), Some(floor_y)) = (ceiling, floor) {
        if ceiling_y - stalactite <= floor_y + stalagmite {
            let lo = (ceiling_y - stalactite).max(floor_y + 1);
            let hi = (floor_y + stalagmite).min(ceiling_y - 1);
            let split = random_between_inclusive(random, lo, hi + 1);
            stalactite = ceiling_y - split;
            stalagmite = (split - 1) - floor_y;
        }
    }

    // The coin comes first and is always spent, even when the rest of the
    // conjunction cannot possibly hold — a column with no stalactite still
    // pays for a merge decision it will not use.
    let merged = random.next_bool()
        && stalactite > 0
        && stalagmite > 0
        && column.height() == Some(stalactite + stalagmite);

    // `RUSTMC_TRACE_SPELEO=x,z` prints the merge conjunction's inputs for one
    // column. The rule and `Column::height`'s arithmetic both match vanilla's
    // bytecode, so a wrong `tip_merge` has to come from an input, and the
    // blocks alone cannot say which: a column that writes a merged tip and no
    // stalagmite looks identical to one whose stalagmite was refused.
    // Per-column draw offset. The feature's *total* draws match vanilla, so a
    // shift inside it is invisible from outside — this stamps each column with
    // where in the stream it started, which is what locates one.
    if trace_speleo_columns() {
        eprintln!(
            "SPELEO COL ({},{}) draw={} density={density} stalactite_cap={stalactite_cap} \
             stalactite={stalactite} stalagmite={stalagmite} merged={merged}",
            pos.0,
            pos.2,
            crate::rng::draws::peek()
        );
    }
    if let Some(want) = trace_speleo() {
        // `*` traces every column the cluster visits, **in visit order**.
        //
        // One column cannot answer the question this trace exists for. Every
        // piece of arithmetic in this feature has been read against the
        // bytecode and matches, so a wrong height is a wrong *stream*, and a
        // stream diverges once and then stays diverged — every column after
        // the first bad one is downstream of it and is not evidence. Only the
        // order shows which one was first.
        let all = want == "*";
        let mut parts = want.split(',').map(str::parse::<i32>);
        if let (Some(Ok(tx)), Some(Ok(tz))) = (parts.next(), parts.next()) {
            if tx == pos.0 && tz == pos.2 {
                eprintln!(
                    "SPELEO ({},{}) ceiling={ceiling:?} floor={floor:?} \
                     height={:?} chance={chance:.17} bits={:#018x} \
                     wants_stalactite={wants_stalactite} \
                     wants_stalagmite={wants_stalagmite} \
                     density={density} stalactite_cap={stalactite_cap} \
                     stalactite={stalactite} stalagmite={stalagmite} \
                     merged={merged}",
                    pos.0,
                    pos.2,
                    column.height(),
                    chance.to_bits()
                );
            }
        } else if all {
            eprintln!(
                // `x`/`z` are the column's offsets from the cluster centre,
                // and they reach the height sampler as `|x| + |z|`, which
                // becomes the bias — the *mean* of the clamped normal. Two
                // runs can therefore consume identical draws and still return
                // different heights if these differ, which is exactly the
                // shape of the disagreement here: the stalagmite in this
                // column matches vanilla, so the stream is aligned, while the
                // stalactite does not.
                "SPELEO ({},{}) off=({x},{z}) g={} ceiling={ceiling:?} floor={floor:?} \
                 height={:?} density={density} stalactite={stalactite} \
                 stalagmite={stalagmite} merged={merged}",
                pos.0,
                pos.2,
                math::normal_calls(),
                column.height()
            );
        }
    }

    if let Some(ceiling_y) = ceiling {
        grow_speleothem(
            world,
            tags,
            config,
            (pos.0, ceiling_y - 1, pos.2),
            DOWN,
            stalactite,
            merged,
        );
    }
    if let Some(floor_y) = floor {
        grow_speleothem(
            world,
            tags,
            config,
            (pos.0, floor_y + 1, pos.2),
            UP,
            stalagmite,
            merged,
        );
    }
    Ok(())
}

/// `SpeleothemClusterFeature#getChanceOfStalagmiteOrStalactite`.
///
/// Distance from the *edge* of the footprint, not the centre: a column in the
/// middle is certain and the rim tails off to
/// `chance_of_speleothem_at_max_distance_from_center`.
///
/// Computed in `f32` and widened, because the reference calls the `float`
/// overload of `clampedMap` and only then returns a `double`.
fn chance_of_speleothem(
    x_radius: i32,
    z_radius: i32,
    x: i32,
    z: i32,
    config: &SpeleothemClusterConfig,
) -> f64 {
    let from_edge = (x_radius - x.abs()).min(z_radius - z.abs());
    #[expect(
        clippy::cast_precision_loss,
        reason = "`(float) d` on a value bounded by the cluster radius"
    )]
    let value = from_edge as f32;
    f64::from(math::clamped_map_f32(
        value,
        0.0,
        #[expect(clippy::cast_precision_loss, reason = "a small configured integer")]
        {
            config.max_distance_from_edge_affecting_chance_of_speleothem as f32
        },
        config.chance_of_speleothem_at_max_distance_from_center,
        1.0,
    ))
}

/// `SpeleothemClusterFeature#getSpeleothemHeight`.
///
/// The density coin is spent **before** the early return, so a column that
/// grows nothing still costs one `nextFloat`. The bias is a mean that falls to
/// zero with distance from the cluster centre, which is what makes the middle
/// of a cluster tall and the rim short.
fn speleothem_height(
    random: &mut dyn RandomSource,
    x: i32,
    z: i32,
    density: f32,
    max_height: i32,
    config: &SpeleothemClusterConfig,
) -> i32 {
    if random.next_f32() > density {
        return 0;
    }
    let distance = x.abs() + z.abs();
    #[expect(
        clippy::cast_possible_truncation,
        reason = "`(float) Mth.clampedMap(double...)` — the reference narrows here"
    )]
    let bias = math::clamped_map(
        f64::from(distance),
        0.0,
        f64::from(config.max_distance_from_center_affecting_height_bias),
        f64::from(max_height) / 2.0,
        0.0,
    ) as f32;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "`(int)` on a float height, which truncates toward zero"
    )]
    let height = random_between_biased(
        random,
        0.0,
        #[expect(clippy::cast_precision_loss, reason = "a small sampled height")]
        {
            max_height as f32
        },
        bias,
        #[expect(clippy::cast_precision_loss, reason = "a small configured integer")]
        {
            config.height_deviation as f32
        },
    ) as i32;
    height
}

/// `SpeleothemClusterFeature#randomBetweenBiased` — `ClampedNormalFloat`
/// spelled out, with the mean and deviation *after* the bounds.
///
/// The argument order is worth keeping in view: the reference takes
/// `(min, max, mean, deviation)` and forwards `(mean, deviation, min, max)`.
fn random_between_biased(
    random: &mut dyn RandomSource,
    min: f32,
    max: f32,
    mean: f32,
    deviation: f32,
) -> f32 {
    math::clamp_f32(math::normal(random, mean, deviation), min, max)
}

fn is_lava<W: SpeleothemWorld + ?Sized>(world: &mut W, pos: (i32, i32, i32)) -> bool {
    plain(&world.block_at(pos.0, pos.1, pos.2)) == "minecraft:lava"
}

/// `SpeleothemClusterFeature#canPlacePool`.
///
/// Draws nothing — five block questions and an early exit on each.
fn can_place_pool<W: SpeleothemWorld + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    config: &SpeleothemClusterConfig,
    pos: (i32, i32, i32),
) -> Result<bool, NeedsRules> {
    let state = world.block_at(pos.0, pos.1, pos.2);
    let name = plain(&state);
    if name == "minecraft:water"
        || name == plain(&config.base_block)
        || name == plain(&config.pointed_block)
    {
        return Ok(false);
    }
    // A fluid *state*, not a block name: the block above may be a waterlogged
    // anything and still hold water.
    if is_water_fluid(&world.fluid_at(pos.0, pos.1 + 1, pos.2)) {
        return Ok(false);
    }
    for direction in HORIZONTAL {
        if !can_be_adjacent_to_water(world, tags, offset(pos, direction)) {
            return Ok(false);
        }
    }
    Ok(can_be_adjacent_to_water(
        world,
        tags,
        (pos.0, pos.1 - 1, pos.2),
    ))
}

/// `SpeleothemClusterFeature#canBeAdjacentToWater` — stone, or water.
fn can_be_adjacent_to_water<W: SpeleothemWorld + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    pos: (i32, i32, i32),
) -> bool {
    let state = world.block_at(pos.0, pos.1, pos.2);
    if tags.contains("minecraft:base_stone_overworld", plain(&state)) {
        return true;
    }
    is_water_fluid(&world.fluid_at(pos.0, pos.1, pos.2))
}

/// `SpeleothemClusterFeature#replaceBlocksWithBaseBlocks` — a run of base
/// block, stopping at the first position that would not take one.
fn replace_blocks_with_base_blocks<W: SpeleothemWorld + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    config: &SpeleothemClusterConfig,
    pos: (i32, i32, i32),
    count: i32,
    direction: usize,
) {
    let mut cursor = pos;
    for _ in 0..count {
        if !place_base_block_if_possible(world, tags, config, cursor) {
            return;
        }
        cursor = offset(cursor, direction);
    }
}

/// `Mth#randomBetweenInclusive`.
fn random_between_inclusive(random: &mut dyn RandomSource, min: i32, max_inclusive: i32) -> i32 {
    random
        .next_i32_bound(max_inclusive.wrapping_sub(min).wrapping_add(1))
        .wrapping_add(min)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feature::block_predicate::PredicateWorld;
    use crate::feature::speleothem::{blocks_are_replaceable, is_base};
    use crate::rng::legacy::LegacyRandomSource;
    use crate::rng::worldgen_random::WorldgenRandom;
    use std::collections::BTreeMap;

    struct Cave {
        blocks: BTreeMap<(i32, i32, i32), String>,
    }

    impl Cave {
        fn empty() -> Self {
            Self {
                blocks: BTreeMap::new(),
            }
        }

        /// A cave with a stone floor at `floor` and a stone ceiling at
        /// `ceiling`, over the whole horizontal extent the tests use.
        ///
        /// Three layers thick on each side rather than one, because
        /// `canPlacePool` asks about the block *below* the floor as well as
        /// the four beside it — a one-block-thick floor is not watertight.
        fn slab(floor: i32, ceiling: i32) -> Self {
            let mut blocks = BTreeMap::new();
            for x in -20..=20 {
                for z in -20..=20 {
                    for d in 0..3 {
                        blocks.insert((x, floor - d, z), "minecraft:stone".to_string());
                        blocks.insert((x, ceiling + d, z), "minecraft:stone".to_string());
                    }
                }
            }
            Self { blocks }
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
                ],
            ),
        ]))
        .expect("tags")
    }

    fn states() -> crate::feature::state_spec::DefaultStateTable {
        crate::feature::state_spec::DefaultStateTable::from_pairs(&[
            ("minecraft:dripstone_block", "minecraft:dripstone_block"),
            (
                "minecraft:pointed_dripstone",
                "minecraft:pointed_dripstone[thickness=tip,vertical_direction=up,waterlogged=false]",
            ),
        ])
    }

    /// `dripstone_cluster`, as the datapack writes it.
    fn dripstone() -> SpeleothemClusterConfig {
        SpeleothemClusterConfig::parse(
            &serde_json::from_str(
                r##"{
                    "base_block": {"Name": "minecraft:dripstone_block"},
                    "chance_of_speleothem_at_max_distance_from_center": 0.1,
                    "density": {"type":"minecraft:uniform","max_exclusive":0.7,"min_inclusive":0.3},
                    "floor_to_ceiling_search_range": 12,
                    "height": {"type":"minecraft:uniform","max_inclusive":6,"min_inclusive":3},
                    "height_deviation": 3,
                    "max_distance_from_center_affecting_height_bias": 8,
                    "max_distance_from_edge_affecting_chance_of_speleothem": 3,
                    "max_stalagmite_stalactite_height_diff": 1,
                    "pointed_block": {"Name":"minecraft:pointed_dripstone","Properties":{"thickness":"tip","vertical_direction":"up","waterlogged":"false"}},
                    "radius": {"type":"minecraft:uniform","max_inclusive":8,"min_inclusive":2},
                    "replaceable_blocks": "#minecraft:dripstone_replaceable_blocks",
                    "speleothem_block_layer_thickness": {"type":"minecraft:uniform","max_inclusive":4,"min_inclusive":2},
                    "wetness": {"type":"minecraft:clamped_normal","deviation":0.3,"max":0.9,"mean":0.1,"min":0.1}
                }"##,
            )
            .expect("json"),
            &states(),
        )
        .expect("parse")
    }

    /// Solid rock costs the stream nothing — the guard is before every draw.
    #[test]
    fn rock_refuses_for_free() {
        let mut world = Cave::empty();
        world
            .blocks
            .insert((0, 0, 0), "minecraft:stone".to_string());
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(7));
        let mut probe = WorldgenRandom::new(LegacyRandomSource::new(7));
        assert!(!place(&mut world, &tags(), &dripstone(), &mut random, (0, 0, 0)).expect("ok"));
        assert_eq!(random.next_i64(), probe.next_i64());
    }

    /// The radius provider is sampled twice, so the footprint is a rectangle
    /// and the two sides genuinely differ for some seed.
    #[test]
    fn the_radius_is_two_samples_not_one() {
        let config = dripstone();
        let mut unequal = 0;
        for seed in 0..64i64 {
            let mut random = WorldgenRandom::new(LegacyRandomSource::new(seed));
            let _height = config.height.sample(&mut random);
            let _wetness = config.wetness.sample(&mut random);
            let _density = config.density.sample(&mut random);
            let x = config.radius.sample(&mut random);
            let z = config.radius.sample(&mut random);
            if x != z {
                unequal += 1;
            }
        }
        assert!(
            unequal > 20,
            "only {unequal}/64 footprints were rectangular — one sample reused?"
        );
    }

    /// A column that finds neither ceiling nor floor returns before the first
    /// coin. Open air over a single stone block is the case: the scan up finds
    /// nothing and the scan down runs out of range.
    #[test]
    fn a_column_with_no_ends_spends_nothing() {
        let mut world = Cave::empty();
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(3));
        let mut probe = WorldgenRandom::new(LegacyRandomSource::new(3));
        place_column(
            &mut world,
            &tags(),
            &dripstone(),
            &mut random,
            (0, 0, 0),
            0,
            0,
            0.5,
            0.5,
            4,
            0.5,
        )
        .expect("ok");
        assert_eq!(random.next_i64(), probe.next_i64());
    }

    /// A column with both ends spends its three coins whatever the world says
    /// — `sulfur_spike_cluster` has `wetness: 0.0` and still draws for it.
    #[test]
    fn a_column_with_ends_spends_three_coins_before_deciding() {
        let mut world = Cave::slab(-4, 4);
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(11));
        place_column(
            &mut world,
            &tags(),
            &dripstone(),
            &mut random,
            (0, 0, 0),
            0,
            0,
            // Wetness zero: the coin can never pass.
            0.0,
            // Chance zero: neither speleothem coin can pass either.
            0.0,
            4,
            0.5,
        )
        .expect("ok");

        let mut probe = WorldgenRandom::new(LegacyRandomSource::new(11));
        probe.next_f32();
        probe.next_f64();
        probe.next_f64();
        probe.next_bool();
        assert_eq!(
            random.next_i64(),
            probe.next_i64(),
            "wetness, two chances and the merge coin, all unconditional"
        );
    }

    /// A `Range` knows its height; a one-ended column does not, which is what
    /// stops a lone stalactite from ever being told its tip merged.
    #[test]
    fn only_a_two_ended_column_has_a_height() {
        assert_eq!(
            Column {
                floor: Some(10),
                ceiling: Some(14)
            }
            .height(),
            Some(3),
            "the empty blocks between, not the distance"
        );
        assert_eq!(
            Column {
                floor: Some(10),
                ceiling: None
            }
            .height(),
            None
        );
        assert_eq!(
            Column {
                floor: None,
                ceiling: None
            }
            .height(),
            None
        );
    }

    /// The scan records an end only when it *lands* on something solid.
    #[test]
    fn a_scan_that_runs_out_of_range_finds_no_end() {
        let mut world = Cave::slab(-4, 4);
        let column = scan(&mut world, (0, 0, 0), 12, &mut neither_empty_nor_water)
            .expect("ok")
            .expect("scanned");
        assert_eq!(column.ceiling, Some(4));
        assert_eq!(column.floor, Some(-4));

        // Range 3 can move at most two blocks, which is not far enough.
        let short = scan(&mut world, (0, 0, 0), 3, &mut neither_empty_nor_water)
            .expect("ok")
            .expect("scanned");
        assert_eq!(short.ceiling, None);
        assert_eq!(short.floor, None);
    }

    /// A wet column lowers its own floor and puts water on the old one, so the
    /// stalagmite starts underneath rather than in the pool.
    #[test]
    fn a_pool_lowers_the_floor_it_sits_on() {
        let mut world = Cave::slab(-4, 4);
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(5));
        place_column(
            &mut world,
            &tags(),
            &dripstone(),
            &mut random,
            (0, 0, 0),
            0,
            0,
            // Certain wetness, no speleothems, so only the pool happens.
            1.0,
            0.0,
            4,
            0.5,
        )
        .expect("ok");
        assert_eq!(
            world.block_at(0, -4, 0),
            "minecraft:water[level=0]",
            "the pool replaces the floor block, with the default state spelled whole"
        );
    }

    /// A pool needs stone all the way round. One hole in the rim and the
    /// water would run out, so the feature declines.
    #[test]
    fn a_pool_needs_a_watertight_rim() {
        let mut world = Cave::slab(-4, 4);
        assert!(
            can_place_pool(&mut world, &tags(), &dripstone(), (0, -4, 0)).expect("ok"),
            "solid stone all round"
        );
        world.blocks.remove(&(1, -4, 0));
        assert!(
            !can_place_pool(&mut world, &tags(), &dripstone(), (0, -4, 0)).expect("ok"),
            "one air block in the rim is enough"
        );
    }

    /// Lava under the floor is not water-tight either, and lava *at* an end
    /// suppresses the speleothem that would have grown from it.
    #[test]
    fn lava_is_not_a_speleothem_anchor() {
        let mut world = Cave::slab(-4, 4);
        world.blocks.insert((0, 4, 0), "minecraft:lava".to_string());
        assert!(is_lava(&mut world, (0, 4, 0)));
        assert!(!is_lava(&mut world, (0, -4, 0)));
    }

    /// The chance falls off towards the rim and is flat in the middle, and it
    /// is computed in single precision before being widened.
    #[test]
    fn the_chance_is_measured_from_the_edge() {
        let config = dripstone();
        // Dead centre of an 8x8 footprint: far inside `max_distance_from_edge`.
        assert!((chance_of_speleothem(8, 8, 0, 0, &config) - 1.0).abs() < 1e-9);
        // On the rim: distance zero, so the configured floor.
        let rim = chance_of_speleothem(8, 8, 8, 0, &config);
        assert!(
            (rim - f64::from(0.1f32)).abs() < 1e-9,
            "{rim} should be the f32 0.1 widened, not the f64 one"
        );
        // Two in from the rim: two thirds of the way up the ramp.
        let inner = chance_of_speleothem(8, 8, 6, 0, &config);
        assert!(inner > rim && inner < 1.0, "{inner}");
    }

    /// The density coin is spent even when it fails, which is the difference
    /// between a cluster that is sparse and a cluster that is desynchronised.
    #[test]
    fn a_column_that_grows_nothing_still_pays_the_density_coin() {
        let config = dripstone();
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(2));
        // Density zero: `nextFloat() > 0.0` for every value the source can
        // produce except an exact zero, so this returns 0 having drawn once.
        assert_eq!(speleothem_height(&mut random, 0, 0, 0.0, 6, &config), 0);

        let mut probe = WorldgenRandom::new(LegacyRandomSource::new(2));
        probe.next_f32();
        assert_eq!(random.next_i64(), probe.next_i64());
    }

    /// A gaussian is not one draw, and the second one on a stream is free.
    #[test]
    fn the_first_gaussian_costs_more_than_the_second() {
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(13));
        let _ = math::normal(&mut random, 0.0, 1.0);
        let after_first = random.next_i64();

        let mut paired = WorldgenRandom::new(LegacyRandomSource::new(13));
        let _ = math::normal(&mut paired, 0.0, 1.0);
        let _ = math::normal(&mut paired, 0.0, 1.0);
        assert_eq!(
            after_first,
            paired.next_i64(),
            "the cached half of the pair means the second gaussian draws nothing"
        );
    }

    /// The tips only merge when they exactly fill the gap.
    #[test]
    fn tips_merge_only_when_they_meet() {
        let column = Column {
            floor: Some(-4),
            ceiling: Some(4),
        };
        assert_eq!(column.height(), Some(7));
        assert!(column.height() == Some(3 + 4), "3 down and 4 up fills it");
        assert!(column.height() != Some(3 + 3), "6 blocks leaves a gap");
    }

    /// The base block is grown from, and the growth helper agrees with the
    /// cluster's own view of what counts as base.
    #[test]
    fn the_cluster_and_the_speleothem_agree_on_what_base_is() {
        let mut world = Cave::empty();
        world
            .blocks
            .insert((0, 0, 0), "minecraft:dripstone_block".to_string());
        world
            .blocks
            .insert((0, 1, 0), "minecraft:stone".to_string());
        world
            .blocks
            .insert((0, 2, 0), "minecraft:gravel".to_string());
        let config = dripstone();
        assert!(is_base(&mut world, &tags(), &config, (0, 0, 0)));
        assert!(
            is_base(&mut world, &tags(), &config, (0, 1, 0)),
            "replaceable counts as base"
        );
        assert!(!is_base(&mut world, &tags(), &config, (0, 2, 0)));
        assert!(blocks_are_replaceable(&tags(), &config, "minecraft:stone"));
    }

    /// A whole cluster in a real slab writes pointed blocks of the configured
    /// kind, hanging down from the ceiling and standing up from the floor.
    #[test]
    fn a_cluster_builds_both_ways_up() {
        let mut world = Cave::slab(-6, 6);
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(20260725));
        assert!(place(&mut world, &tags(), &dripstone(), &mut random, (0, 0, 0)).expect("ok"));

        let spikes: Vec<&String> = world
            .blocks
            .values()
            .filter(|b| b.starts_with("minecraft:pointed_dripstone"))
            .collect();
        assert!(!spikes.is_empty(), "a cluster with no speleothems in it");
        assert!(
            spikes.iter().any(|b| b.contains("vertical_direction=down")),
            "nothing hanging"
        );
        assert!(
            spikes.iter().any(|b| b.contains("vertical_direction=up")),
            "nothing standing"
        );
        // The layer under each end is turned to base block first.
        assert!(
            world
                .blocks
                .values()
                .any(|b| b == "minecraft:dripstone_block"),
            "no base block layer"
        );
    }

    /// Two runs of one seed agree — the whole feature is a pure function of
    /// the stream and the world.
    #[test]
    fn two_runs_of_one_seed_agree() {
        let run = || {
            let mut world = Cave::slab(-6, 6);
            let mut random = WorldgenRandom::new(LegacyRandomSource::new(99));
            place(&mut world, &tags(), &dripstone(), &mut random, (0, 0, 0)).expect("ok");
            world.blocks
        };
        assert_eq!(run(), run());
    }

    /// Both cluster configs in the datapack parse, including the one whose
    /// `wetness` is a bare `0.0` rather than a provider object.
    #[test]
    fn every_cluster_config_in_the_datapack_parses() {
        let Some(extract) = crate::feature::state_spec::extract_or_skip() else {
            return;
        };
        let dir = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../oracle-work/vanilla-26.2/generated-data/data/minecraft/worldgen/configured_feature"
        ));
        if !rustmc_data::fixture::available(dir, "datagen extract", "ci/vanilla-server.sh data") {
            return;
        }
        let mut seen = 0usize;
        let mut constant_wetness = 0usize;
        for entry in std::fs::read_dir(dir).expect("readable") {
            let path = entry.expect("entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let json: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).expect("readable"))
                    .expect("json");
            if json.get("type").and_then(serde_json::Value::as_str)
                != Some("minecraft:speleothem_cluster")
            {
                continue;
            }
            let config = SpeleothemClusterConfig::parse(
                json.get("config").unwrap_or(&serde_json::Value::Null),
                &extract,
            )
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            if matches!(config.wetness, FloatProvider::Constant(_)) {
                constant_wetness += 1;
            }
            seen += 1;
        }
        assert!(seen >= 2, "only {seen} cluster configs parsed");
        assert!(
            constant_wetness >= 1,
            "no config had a bare-number wetness — the either-codec is untested"
        );
        println!("CLUSTER CONFIGS: {seen} parsed, {constant_wetness} with a constant wetness");
    }
}
