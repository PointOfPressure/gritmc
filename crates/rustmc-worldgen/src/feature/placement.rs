//! Placement modifiers — from one chunk origin to the positions a feature
//! actually tries.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `PlacedFeature#placeWithContext`, `PlacementModifier`,
//! `RepeatingPlacement`, `CountPlacement`, `InSquarePlacement`,
//! `RarityFilter`, `RandomOffsetPlacement`, `HeightRangePlacement`,
//! `HeightmapPlacement`, `BiomeFilter`, `PlacementFilter`.
//!
//! # The chain is lazy, and that decides the draw order
//!
//! Vanilla writes it as a stream fold:
//!
//! ```text
//! Stream.of(origin)
//!   .flatMap(m1::getPositions)
//!   .flatMap(m2::getPositions)
//!   ...
//!   .forEach(feature::place)
//! ```
//!
//! Java streams are lazy, so this is **not** "run m1 over everything, then m2
//! over everything". It is depth-first: m1 produces its positions, then for
//! its *first* position m2 runs, then m3 on m2's first output, then the
//! feature is placed — and only then does control return for m2's second
//! output.
//!
//! Since every modifier draws from the same shared `RandomSource`, that
//! ordering is the world. A port that materialises each stage into a vector
//! before starting the next runs the same modifiers with the same inputs in
//! breadth-first order, consumes the same *number* of draws, and places
//! everything somewhere else.
//!
//! This module therefore evaluates the chain recursively rather than
//! iteratively, and [`positions`] returns the sequence in the order the
//! feature would be placed so a caller can check it.

use crate::carver::providers::HeightProvider;
use crate::feature::block_predicate::{BlockPredicate, PredicateWorld};
use crate::feature::int_provider::IntProvider;
use crate::rng::RandomSource;
use crate::tags::BlockTags;

// Trace switches, read once each rather than per call: `getenv` in a
// feature inner loop was 3.8% of a full pipeline. See
// `crate::trace_switch`.
crate::trace_switch_value!(trace_scan, "RUSTMC_TRACE_SCAN");

/// Which heightmap a `heightmap` placement reads.
///
/// The `_WG` pair are the worldgen-time maps, primed by the noise stage
/// before any feature runs; the other three are the live ones a decorated
/// chunk keeps updating. Reading the wrong one puts trees on the terrain as
/// it was rather than as it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeightmapKind {
    MotionBlocking,
    MotionBlockingNoLeaves,
    OceanFloor,
    WorldSurface,
    OceanFloorWg,
    WorldSurfaceWg,
}

impl HeightmapKind {
    /// Parse the datapack spelling, which is the enum constant's name.
    ///
    /// # Errors
    /// Returns the offending string if it names no known heightmap. Never
    /// defaults: every kind is a different surface, so a typo would place a
    /// whole feature family somewhere plausible and wrong.
    pub fn parse(name: &str) -> Result<Self, &str> {
        match name {
            "MOTION_BLOCKING" => Ok(Self::MotionBlocking),
            "MOTION_BLOCKING_NO_LEAVES" => Ok(Self::MotionBlockingNoLeaves),
            "OCEAN_FLOOR" => Ok(Self::OceanFloor),
            "WORLD_SURFACE" => Ok(Self::WorldSurface),
            "OCEAN_FLOOR_WG" => Ok(Self::OceanFloorWg),
            "WORLD_SURFACE_WG" => Ok(Self::WorldSurfaceWg),
            other => Err(other),
        }
    }
}

/// What the world-dependent modifiers need to ask.
///
/// A trait so the chain stays testable without a chunk: everything a
/// modifier reads from the world goes through here, and nothing else does.
pub trait PlacementWorld: PredicateWorld {
    /// The block tags a `block_predicate_filter` resolves against.
    fn block_tags(&self) -> &BlockTags;

    /// `Biome.BIOME_INFO_NOISE.getValue(x, y, false)` — the legacy
    /// `PerlinSimplexNoise` the two noise-count modifiers share.
    ///
    /// It is the same static the `frozen` temperature modifier reads, proved
    /// at 100.000000% by `golden_temperature`, and it takes **already
    /// divided** coordinates: the divisor is the modifier's business, and it
    /// differs between them (a literal 200.0 for the threshold variant, a
    /// config field for the other).
    fn biome_info_noise(&mut self, x: f64, z: f64) -> f64;

    /// `context.getMinY()`.
    fn min_y(&self) -> i32;

    /// The dimension's generation height, for resolving vertical anchors.
    fn gen_depth(&self) -> i32;

    /// `context.getHeight(type, x, z)` — one past the top solid block.
    fn heightmap(&mut self, kind: HeightmapKind, x: i32, z: i32) -> i32;

    /// `BiomeFilter#shouldPlace` — does the biome at this position list the
    /// placed feature being placed?
    ///
    /// The question is about *this* feature. Vanilla answers it from
    /// `PlacementContext.topFeature()`, which is context the chain carries
    /// rather than an argument the modifier receives, so the driver sets it
    /// here before running a chain.
    fn set_running_feature(&mut self, _name: &str) {}

    fn biome_lists_this_feature(&mut self, x: i32, y: i32, z: i32) -> bool;
}

/// A block position, in the sense placement uses: a mutable cursor through
/// the chain rather than a world reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

/// `PlacementModifier`.
///
/// Seven of the fifteen are implemented: four that are pure functions of a
/// position and the stream, and three that ask the world through
/// [`PlacementWorld`]. The rest are parsed into [`Self::NeedsWorld`] so a
/// chain containing one **fails loudly** rather than silently placing a
/// feature the wrong way. A modifier quietly skipped is a forest that grows
/// underwater.
#[derive(Clone, Debug, PartialEq)]
pub enum PlacementModifier {
    /// `count` — repeat the incoming position n times.
    Count(IntProvider),
    /// `in_square` — scatter within the chunk. Two draws, x then z.
    InSquare,
    /// `rarity_filter` — keep the position with probability `1/chance`.
    RarityFilter { chance: i32 },
    /// `random_offset` — three draws, and the middle one is **y**.
    RandomOffset {
        xz_spread: IntProvider,
        y_spread: IntProvider,
    },
    /// `height_range` — replace the y outright, keeping x and z.
    HeightRange(HeightProvider),
    /// `heightmap` — move to the surface. Produces **nothing** when the
    /// column is empty, which is how features stop at the world floor.
    Heightmap(HeightmapKind),
    /// `biome` — keep the position only if its biome lists this feature.
    /// Draws nothing.
    BiomeFilter,
    /// `surface_relative_threshold_filter` — keep the position only if its y
    /// is within a window measured from a heightmap. Draws nothing.
    ///
    /// The window bounds default to the full `i32` range and vanilla does the
    /// arithmetic in **`long`**, which is not incidental: the default
    /// `min_inclusive` is `Integer.MIN_VALUE`, and `surface + MIN_VALUE` in
    /// 32 bits wraps to a large positive number and rejects everything. The
    /// widening is what makes "no minimum" mean no minimum.
    SurfaceRelativeThresholdFilter {
        heightmap: HeightmapKind,
        min_inclusive: i32,
        max_inclusive: i32,
    },
    /// `block_predicate_filter` — ask the world about the blocks around this
    /// position. Draws nothing.
    BlockPredicateFilter(BlockPredicate),
    /// `surface_water_depth_filter` — keep the position only if the water
    /// above the ocean floor is no deeper than `max_water_depth`.
    ///
    /// Reads **two** heightmaps, neither of them the `_WG` fossil:
    /// `WORLD_SURFACE - OCEAN_FLOOR <= max`. Both are re-primed immediately
    /// before features and move as features write, so a port that treats them
    /// as fixed drifts within a single step.
    SurfaceWaterDepthFilter { max_water_depth: i32 },
    /// `environment_scan` — walk in one direction while `allowed` holds,
    /// looking for `target`.
    ///
    /// Draws nothing, and is the only modifier that can *move* a position
    /// rather than filter or offset it.
    EnvironmentScan {
        direction: ScanDirection,
        target: BlockPredicate,
        allowed: BlockPredicate,
        max_steps: i32,
    },
    /// `noise_threshold_count` — the count is one of two constants, chosen
    /// by a noise sample. Draws nothing.
    NoiseThresholdCount {
        noise_level: f64,
        below_noise: i32,
        above_noise: i32,
    },
    /// `noise_based_count` — the count is a noise sample scaled to an
    /// integer. Draws nothing.
    NoiseBasedCount {
        noise_to_count_ratio: i32,
        noise_factor: f64,
        noise_offset: f64,
    },
    /// `count_on_every_layer` — the only modifier that produces positions in
    /// more than one *layer* of the column, and the only one whose count is
    /// re-sampled inside its own loop.
    ///
    /// ```text
    /// do {
    ///     any = false;
    ///     for (i = 0; i < count.sample(random); i++) { ... }
    ///     layer++;
    /// } while (any);
    /// ```
    ///
    /// `count.sample(random)` sits in the loop **condition**, so a uniform
    /// count draws once per iteration *and once more* for the test that ends
    /// the loop. Reading it as `int n = count.sample(random)` hoisted out of
    /// the loop is the natural transcription and it is wrong by one draw per
    /// iteration.
    ///
    /// The outer loop keeps going while a layer produced anything, so a deep
    /// cave column runs it many times. Vanilla builds the whole result with
    /// `Stream.builder()` before the feature sees the first position, so
    /// unlike the lazy modifiers every draw here happens up front.
    CountOnEveryLayer(IntProvider),
    /// `fixed_placement` — a hand-written list of absolute positions, kept
    /// only when one of them falls in this chunk.
    ///
    /// Draws nothing, ignores the incoming position entirely, and emits the
    /// **whole** list rather than the matching entry: the scan proves at least
    /// one is in the chunk, and the filter that follows re-tests every entry.
    /// Two loops over the same list where one would do, and the difference is
    /// visible — a list with two positions in the chunk emits both, from any
    /// one input position.
    FixedPlacement(Vec<Pos>),
    /// Parsed but not evaluated. Carries its type name so a failure says
    /// which.
    NeedsWorld(String),
}

/// `environment_scan`'s `direction_of_search`, which the datapack spells as
/// a `Direction` name. Only up and down appear in vanilla, and a horizontal
/// scan would need the same care about `Direction.values()` order that the
/// face-occlusion table takes, so anything else is refused rather than
/// guessed at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanDirection {
    Up,
    Down,
}

/// What a chain could not do.
#[derive(Debug, PartialEq, Eq)]
pub struct Unsupported(pub String);

impl std::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "placement modifier `{}` needs world access", self.0)
    }
}

impl std::error::Error for Unsupported {}

impl PlacementModifier {
    /// Parse one entry of a placed feature's `placement` list.
    ///
    /// # Errors
    /// If the entry names no type, or a supported type is missing a field.
    /// An *unsupported* type is not an error — it parses to
    /// [`Self::NeedsWorld`], so the shape of a chain can be read without
    /// being able to run it.
    pub fn parse(value: &serde_json::Value) -> Result<Self, String> {
        let kind = value
            .get("type")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| format!("placement modifier has no `type`: {value}"))?;
        fn int_field(value: &serde_json::Value, kind: &str, name: &str) -> Result<i32, String> {
            value
                .get(name)
                .and_then(serde_json::Value::as_i64)
                .and_then(|v| i32::try_from(v).ok())
                .ok_or_else(|| format!("{kind} has no integer `{name}`"))
        }
        let provider = |name: &str| -> Result<IntProvider, String> {
            IntProvider::parse(
                value
                    .get(name)
                    .ok_or_else(|| format!("{kind} has no `{name}`"))?,
            )
        };
        match kind {
            "minecraft:count" => Ok(Self::Count(provider("count")?)),
            "minecraft:in_square" => Ok(Self::InSquare),
            "minecraft:rarity_filter" => Ok(Self::RarityFilter {
                chance: value
                    .get("chance")
                    .and_then(serde_json::Value::as_i64)
                    .and_then(|v| i32::try_from(v).ok())
                    .ok_or_else(|| "rarity_filter has no integer `chance`".to_string())?,
            }),
            "minecraft:random_offset" => Ok(Self::RandomOffset {
                xz_spread: provider("xz_spread")?,
                y_spread: provider("y_spread")?,
            }),
            "minecraft:height_range" => Ok(Self::HeightRange(HeightProvider::parse(
                value
                    .get("height")
                    .ok_or_else(|| "height_range has no `height`".to_string())?,
            )?)),
            "minecraft:heightmap" => {
                let name = value
                    .get("heightmap")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| "heightmap has no `heightmap`".to_string())?;
                HeightmapKind::parse(name)
                    .map(Self::Heightmap)
                    .map_err(|bad| format!("unknown heightmap `{bad}`"))
            }
            "minecraft:biome" => Ok(Self::BiomeFilter),
            "minecraft:noise_threshold_count" => Ok(Self::NoiseThresholdCount {
                noise_level: value
                    .get("noise_level")
                    .and_then(serde_json::Value::as_f64)
                    .ok_or_else(|| format!("{kind} has no `noise_level`"))?,
                below_noise: int_field(value, kind, "below_noise")?,
                above_noise: int_field(value, kind, "above_noise")?,
            }),
            "minecraft:fixed_placement" => {
                let list = value
                    .get("positions")
                    .and_then(serde_json::Value::as_array)
                    .ok_or_else(|| format!("{kind} has no `positions`"))?;
                let positions = list
                    .iter()
                    .map(|p| {
                        let p = p
                            .as_array()
                            .filter(|p| p.len() == 3)
                            .ok_or_else(|| format!("{kind}: a position is not [x, y, z]"))?;
                        let at = |i: usize| -> Result<i32, String> {
                            p[i].as_i64()
                                .and_then(|v| i32::try_from(v).ok())
                                .ok_or_else(|| format!("{kind}: a coordinate is not an i32"))
                        };
                        Ok(Pos {
                            x: at(0)?,
                            y: at(1)?,
                            z: at(2)?,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                Ok(Self::FixedPlacement(positions))
            }
            "minecraft:count_on_every_layer" => Ok(Self::CountOnEveryLayer(IntProvider::parse(
                value
                    .get("count")
                    .ok_or_else(|| format!("{kind} has no `count`"))?,
            )?)),
            "minecraft:noise_based_count" => Ok(Self::NoiseBasedCount {
                noise_to_count_ratio: int_field(value, kind, "noise_to_count_ratio")?,
                noise_factor: value
                    .get("noise_factor")
                    .and_then(serde_json::Value::as_f64)
                    .ok_or_else(|| format!("{kind} has no `noise_factor`"))?,
                // `optionalFieldOf("noise_offset", 0.0)`.
                noise_offset: value
                    .get("noise_offset")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(0.0),
            }),
            "minecraft:surface_water_depth_filter" => Ok(Self::SurfaceWaterDepthFilter {
                max_water_depth: value
                    .get("max_water_depth")
                    .and_then(serde_json::Value::as_i64)
                    .and_then(|v| i32::try_from(v).ok())
                    .ok_or_else(|| format!("{kind} has no integer `max_water_depth`"))?,
            }),
            "minecraft:environment_scan" => Ok(Self::EnvironmentScan {
                direction: match value
                    .get("direction_of_search")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| format!("{kind} has no `direction_of_search`"))?
                {
                    "up" => ScanDirection::Up,
                    "down" => ScanDirection::Down,
                    other => return Err(format!("{kind}: unsupported search direction `{other}`")),
                },
                target: BlockPredicate::parse(
                    value
                        .get("target_condition")
                        .ok_or_else(|| format!("{kind} has no `target_condition`"))?,
                )?,
                // `optionalFieldOf("allowed_search_condition",
                // BlockPredicate.alwaysTrue())` — absent means scan through
                // anything, not scan through nothing. `spruce_on_snow` omits
                // it, and defaulting the other way would stop every scan at
                // its first step.
                allowed: match value.get("allowed_search_condition") {
                    Some(predicate) => BlockPredicate::parse(predicate)?,
                    None => BlockPredicate::Always,
                },
                max_steps: value
                    .get("max_steps")
                    .and_then(serde_json::Value::as_i64)
                    .and_then(|v| i32::try_from(v).ok())
                    .ok_or_else(|| format!("{kind} has no integer `max_steps`"))?,
            }),
            "minecraft:surface_relative_threshold_filter" => {
                Ok(Self::SurfaceRelativeThresholdFilter {
                    heightmap: HeightmapKind::parse(
                        value
                            .get("heightmap")
                            .and_then(serde_json::Value::as_str)
                            .ok_or_else(|| format!("{kind} has no `heightmap`"))?,
                    )
                    .map_err(|e| format!("{kind}: unknown heightmap `{e}`"))?,
                    min_inclusive: value
                        .get("min_inclusive")
                        .and_then(serde_json::Value::as_i64)
                        .and_then(|v| i32::try_from(v).ok())
                        .unwrap_or(i32::MIN),
                    max_inclusive: value
                        .get("max_inclusive")
                        .and_then(serde_json::Value::as_i64)
                        .and_then(|v| i32::try_from(v).ok())
                        .unwrap_or(i32::MAX),
                })
            }
            "minecraft:block_predicate_filter" => {
                Ok(Self::BlockPredicateFilter(BlockPredicate::parse(
                    value
                        .get("predicate")
                        .ok_or_else(|| "block_predicate_filter has no `predicate`".to_string())?,
                )?))
            }
            other => Ok(Self::NeedsWorld(other.to_string())),
        }
    }

    /// Whether this modifier can be evaluated at all.
    ///
    /// Looks *into* a `block_predicate_filter`: a chain whose filter carries
    /// a `would_survive` is not runnable, however supported the modifier
    /// itself is. Counting those as runnable was the first version of the
    /// coverage number here, and it overstated by 103 chains.
    /// The datapack's name for this modifier.
    ///
    /// For the attempt trace, which otherwise reports a bare index and makes
    /// the reader cross-reference the placed feature's JSON by hand to work
    /// out what rejected an attempt.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Count(_) => "count",
            Self::InSquare => "in_square",
            Self::RarityFilter { .. } => "rarity_filter",
            Self::RandomOffset { .. } => "random_offset",
            Self::HeightRange(_) => "height_range",
            Self::Heightmap(_) => "heightmap",
            Self::BiomeFilter => "biome",
            Self::SurfaceRelativeThresholdFilter { .. } => "surface_relative_threshold_filter",
            Self::BlockPredicateFilter(_) => "block_predicate_filter",
            Self::SurfaceWaterDepthFilter { .. } => "surface_water_depth_filter",
            Self::EnvironmentScan { .. } => "environment_scan",
            Self::NoiseThresholdCount { .. } => "noise_threshold_count",
            Self::NoiseBasedCount { .. } => "noise_based_count",
            Self::CountOnEveryLayer(_) => "count_on_every_layer",
            Self::FixedPlacement(_) => "fixed_placement",
            Self::NeedsWorld(_) => "needs_world",
        }
    }

    #[must_use]
    pub fn is_supported(&self) -> bool {
        match self {
            Self::NeedsWorld(_) => false,
            Self::BlockPredicateFilter(predicate) => predicate.is_supported(),
            // A scan carries two predicates and is only as answerable as
            // they are — `spruce_on_snow`'s target is fine, `glow_lichen`'s
            // is `has_sturdy_face`, which needs block behaviour. The
            // catch-all below used to swallow both, which meant the chain
            // claimed support and then failed at evaluation.
            Self::EnvironmentScan {
                target, allowed, ..
            } => target.is_supported() && allowed.is_supported(),
            _ => true,
        }
    }

    /// `PlacementModifier#getPositions` for one input position.
    fn get_positions<R: RandomSource, W: PlacementWorld + ?Sized>(
        &self,
        world: &mut W,
        random: &mut R,
        origin: Pos,
    ) -> Result<Vec<Pos>, Unsupported> {
        match self {
            // `IntStream.range(0, count(random, origin)).mapToObj(i -> origin)`
            // — the count is drawn once, then the same position repeats.
            Self::Count(count) => {
                let n = count.sample(random);
                Ok(vec![origin; n.max(0).try_into().unwrap_or(0)])
            }

            // x then z, and the y is carried through untouched.
            Self::InSquare => {
                let x = random.next_i32_bound(16) + origin.x;
                let z = random.next_i32_bound(16) + origin.z;
                Ok(vec![Pos { x, y: origin.y, z }])
            }

            // `SurfaceRelativeThresholdFilter#shouldPlace`, in i64 for the
            // reason given on the variant.
            Self::SurfaceRelativeThresholdFilter {
                heightmap,
                min_inclusive,
                max_inclusive,
            } => {
                let surface = i64::from(world.heightmap(*heightmap, origin.x, origin.z));
                let low = surface + i64::from(*min_inclusive);
                let high = surface + i64::from(*max_inclusive);
                let y = i64::from(origin.y);
                if low <= y && y <= high {
                    Ok(vec![origin])
                } else {
                    Ok(Vec::new())
                }
            }

            // `NoiseThresholdCountPlacement#count`. The divisor is a literal
            // 200.0 in vanilla, not a config field — the only tunable is
            // where the threshold sits.
            Self::NoiseThresholdCount {
                noise_level,
                below_noise,
                above_noise,
            } => {
                let noise = world
                    .biome_info_noise(f64::from(origin.x) / 200.0, f64::from(origin.z) / 200.0);
                let n = if noise < *noise_level {
                    *below_noise
                } else {
                    *above_noise
                };
                Ok(vec![origin; usize::try_from(n.max(0)).unwrap_or(0)])
            }

            // `NoiseBasedCountPlacement#count`:
            // `ceil((noise + offset) * ratio)`, truncated to int.
            Self::NoiseBasedCount {
                noise_to_count_ratio,
                noise_factor,
                noise_offset,
            } => {
                let noise = world.biome_info_noise(
                    f64::from(origin.x) / *noise_factor,
                    f64::from(origin.z) / *noise_factor,
                );
                #[allow(clippy::cast_possible_truncation)]
                let n = ((noise + *noise_offset) * f64::from(*noise_to_count_ratio)).ceil() as i32;
                Ok(vec![origin; usize::try_from(n.max(0)).unwrap_or(0)])
            }

            // `FixedPlacement#getPositions`. `SectionPos.blockToSectionCoord`
            // is an arithmetic shift, so it is right for negative
            // coordinates where a division would not be.
            Self::FixedPlacement(positions) => {
                let (cx, cz) = (origin.x >> 4, origin.z >> 4);
                let here = |p: &Pos| (p.x >> 4, p.z >> 4) == (cx, cz);
                if !positions.iter().any(here) {
                    return Ok(Vec::new());
                }
                Ok(positions.iter().copied().filter(here).collect())
            }

            // `CountOnEveryLayerPlacement#getPositions`. See the variant's
            // docs for the two traps: the re-sampled count and the outer loop.
            Self::CountOnEveryLayer(count) => {
                let mut out = Vec::new();
                let mut layer = 0i32;
                loop {
                    let mut any = false;
                    let mut i = 0i32;
                    // The sample is the loop *condition*, so it is drawn
                    // again for the iteration that fails it.
                    while i < count.sample(random) {
                        let x = random.next_i32_bound(16) + origin.x;
                        let z = random.next_i32_bound(16) + origin.z;
                        let top = world.heightmap(HeightmapKind::MotionBlocking, x, z);
                        if let Some(y) = find_on_ground_y(world, x, top, z, layer) {
                            out.push(Pos { x, y, z });
                            any = true;
                        }
                        i += 1;
                    }
                    layer += 1;
                    if !any {
                        break;
                    }
                }
                Ok(out)
            }

            // `SurfaceWaterDepthFilter#shouldPlace`: the gap between the two
            // heightmaps is the water column.
            //
            // **Both maps are the LIVE ones** — `OCEAN_FLOOR` and
            // `WORLD_SURFACE`, read straight off the bytecode, offsets 1 and
            // 18. This read `WORLD_SURFACE_WG` until 2026-07-29, which is a
            // different surface: the `_WG` pair freeze when the chunk's
            // status reaches CARVERS, so on a decorated chunk they are the
            // *post-carver* terrain and the live pair are the terrain as the
            // features left it.
            //
            // The two agree until something decorates, which is why this
            // survived: the gate's oracle froze its own live maps by
            // accident, so `WORLD_SURFACE_WG == WORLD_SURFACE` there and the
            // filter came out right for the wrong reason. Fixing the oracle
            // turned it into three missed rejections in one chunk — every
            // one of them a tree planted in water.
            Self::SurfaceWaterDepthFilter { max_water_depth } => {
                let floor = world.heightmap(HeightmapKind::OceanFloor, origin.x, origin.z);
                let surface = world.heightmap(HeightmapKind::WorldSurface, origin.x, origin.z);
                if surface - floor <= *max_water_depth {
                    Ok(vec![origin])
                } else {
                    Ok(Vec::new())
                }
            }

            // `EnvironmentScanPlacement#getPositions`.
            //
            // The `allowed` test runs **before** the loop as well as inside
            // it, and the `target` test runs once more after the loop ends —
            // so a position whose own block is already the target is returned
            // without a step, and a scan that runs out of steps still gets one
            // last chance at wherever it stopped.
            Self::EnvironmentScan {
                direction,
                target,
                allowed,
                max_steps,
            } => {
                let tags = world.block_tags().clone();
                let step = match direction {
                    ScanDirection::Up => 1,
                    ScanDirection::Down => -1,
                };
                let hit = |world: &mut W, predicate: &BlockPredicate, pos: Pos| {
                    predicate
                        .test(world, &tags, (pos.x, pos.y, pos.z))
                        .map_err(|e| Unsupported(e.0))
                };
                let mut pos = origin;
                // `RUSTMC_TRACE_SCAN=x,z` prints this scan's decisions for one
                // column. The scan is the only modifier that moves a position,
                // so when a chain yields fewer positions than vanilla's, the
                // question is always "how far up did it get, and which
                // predicate stopped it" — and that is not derivable from the
                // surviving positions, because a scan that fails leaves
                // nothing behind.
                let trace = trace_scan()
                    .and_then(|v| {
                        let (x, z) = v.split_once(',')?;
                        Some((x.trim().parse::<i32>().ok()?, z.trim().parse::<i32>().ok()?))
                    })
                    .is_some_and(|(x, z)| x == origin.x && z == origin.z)
                    || trace_scan() == Some("*");
                if !hit(world, allowed, pos)? {
                    if trace {
                        println!("scan {origin:?}: allowed FAILS at the origin");
                    }
                    return Ok(Vec::new());
                }
                let mut taken = 0;
                while taken < *max_steps {
                    if hit(world, target, pos)? {
                        if trace {
                            println!("scan {origin:?}: target hit at y {} after {taken}", pos.y);
                        }
                        return Ok(vec![pos]);
                    }
                    pos.y += step;
                    if pos.y < world.min_y() || pos.y >= world.min_y() + world.gen_depth() {
                        if trace {
                            println!("scan {origin:?}: left build height at y {}", pos.y);
                        }
                        return Ok(Vec::new());
                    }
                    if !hit(world, allowed, pos)? {
                        if trace {
                            println!(
                                "scan {origin:?}: allowed FAILS at y {} after {taken} steps",
                                pos.y
                            );
                        }
                        break;
                    }
                    taken += 1;
                }
                if hit(world, target, pos)? {
                    if trace {
                        println!(
                            "scan {origin:?}: target hit at y {} on the last check",
                            pos.y
                        );
                    }
                    Ok(vec![pos])
                } else {
                    if trace {
                        println!(
                            "scan {origin:?}: exhausted at y {} — target does not hold",
                            pos.y
                        );
                    }
                    Ok(Vec::new())
                }
            }

            // Draws whether or not it keeps the position.
            Self::RarityFilter { chance } => {
                #[allow(clippy::cast_precision_loss)]
                let threshold = 1.0f32 / *chance as f32;
                if random.next_f32() < threshold {
                    Ok(vec![origin])
                } else {
                    Ok(Vec::new())
                }
            }

            // Three draws in the order x, **y**, z — the y sample sits
            // between the two xz ones, so treating xz as a pair and y as an
            // afterthought reads the stream in the wrong order.
            Self::RandomOffset {
                xz_spread,
                y_spread,
            } => {
                let dx = xz_spread.sample(random);
                let dy = y_spread.sample(random);
                let dz = xz_spread.sample(random);
                // `RUSTMC_TRACE_OFFSET=1` prints the deltas with the
                // providers that produced them. Inferring the deltas from
                // the emitted positions requires knowing the base, and
                // inferring the distribution from four deltas is guesswork —
                // this prints both outright.
                if *TRACE_OFFSET {
                    eprintln!("OFFSET d=({dx},{dy},{dz}) xz={xz_spread:?} y={y_spread:?}");
                }
                Ok(vec![Pos {
                    x: origin.x + dx,
                    y: origin.y + dy,
                    z: origin.z + dz,
                }])
            }

            // `origin.atY(...)` — x and z survive.
            Self::HeightRange(height) => Ok(vec![Pos {
                x: origin.x,
                y: height.sample(random, world.min_y(), world.gen_depth()),
                z: origin.z,
            }]),

            // `height > context.getMinY() ? Stream.of(...) : Stream.of()`.
            // Strictly greater, so a column whose heightmap sits exactly at
            // the world floor produces nothing rather than a position at the
            // floor.
            Self::Heightmap(kind) => {
                let height = world.heightmap(*kind, origin.x, origin.z);
                if height > world.min_y() {
                    Ok(vec![Pos {
                        x: origin.x,
                        y: height,
                        z: origin.z,
                    }])
                } else {
                    Ok(Vec::new())
                }
            }

            // A filter, and unlike `rarity_filter` it draws nothing at all.
            Self::BiomeFilter => {
                if world.biome_lists_this_feature(origin.x, origin.y, origin.z) {
                    Ok(vec![origin])
                } else {
                    Ok(Vec::new())
                }
            }

            // Also draws nothing. The predicate may itself be unanswerable,
            // in which case the failure names the *predicate* rather than
            // the modifier — which is the more useful of the two.
            Self::BlockPredicateFilter(predicate) => {
                let tags = world.block_tags().clone();
                let kept = predicate
                    .test(world, &tags, (origin.x, origin.y, origin.z))
                    .map_err(|e| Unsupported(e.0))?;
                Ok(if kept { vec![origin] } else { Vec::new() })
            }

            Self::NeedsWorld(name) => Err(Unsupported(name.clone())),
        }
    }
}

/// `PlacedFeature#placeWithContext`'s position chain.
///
/// Returns the positions in the order the feature would be placed at them,
/// having consumed exactly the draws vanilla consumes.
///
/// # Errors
/// If the chain contains a modifier that needs world access.
pub fn positions<R: RandomSource, W: PlacementWorld + ?Sized>(
    modifiers: &[PlacementModifier],
    world: &mut W,
    random: &mut R,
    origin: Pos,
) -> Result<Vec<Pos>, Unsupported> {
    let mut out = Vec::new();
    for_each_position(modifiers, world, random, origin, |_, _, pos| {
        out.push(pos);
        Ok(())
    })?;
    Ok(out)
}

/// The same chain, but handing each position to `emit` **as it is produced**.
///
/// This is not a convenience over [`positions`] — it is the faithful one, and
/// the only one a driver may use.
///
/// `PlacedFeature#placeWithContext` builds its positions as a chain of
/// `Stream.flatMap` and then calls `forEach` on it. Java streams are lazy, so
/// the feature runs at position *n* **before** position *n+1* is computed,
/// and both draw from the same `RandomSource`. Collecting the whole list
/// first and placing afterwards consumes the identical draws in a different
/// order, which produces a different world and a different RNG state — a
/// wiring bug that no amount of correct arithmetic underneath will show.
///
/// It was caught by `golden_decorate`: positions matched, and the RNG state
/// after each feature did not.
///
/// # Errors
/// If the chain contains a modifier that needs world access, or `emit` fails.
pub fn for_each_position<R: RandomSource, W: PlacementWorld + ?Sized, F>(
    modifiers: &[PlacementModifier],
    world: &mut W,
    random: &mut R,
    origin: Pos,
    mut emit: F,
) -> Result<(), Unsupported>
where
    F: FnMut(&mut W, &mut R, Pos) -> Result<(), Unsupported>,
{
    NESTING.with(|n| n.set(n.get() + 1));
    let out = walk(modifiers, world, random, origin, &mut emit);
    NESTING.with(|n| n.set(n.get().saturating_sub(1)));
    out
}

/// `CountOnEveryLayerPlacement#findOnGroundYPosition`.
///
/// Walks down from `top` looking for the `target_layer`-th solid block with
/// something empty directly above it — the floor of the *n*th cave in the
/// column. Bedrock never counts as a floor, which is what stops features
/// spawning on the world's bottom slab.
///
/// `None` is vanilla's `Integer.MAX_VALUE` sentinel: no such layer.
fn find_on_ground_y<W: PlacementWorld + ?Sized>(
    world: &mut W,
    x: i32,
    top: i32,
    z: i32,
    target_layer: i32,
) -> Option<i32> {
    /// `isEmpty` — air, or the *block* water or lava, so a flowing source
    /// counts and a waterlogged fence does not.
    fn is_empty(state: &str) -> bool {
        let plain = state.split_once('[').map_or(state, |(n, _)| n);
        matches!(
            plain,
            "minecraft:air"
                | "minecraft:cave_air"
                | "minecraft:void_air"
                | "minecraft:water"
                | "minecraft:lava"
        )
    }

    let mut found = 0i32;
    // The state one *above* the block under test, seeded with the column top
    // itself rather than with air.
    let mut above = world.block_at(x, top, z);
    let mut y = top;
    // Vanilla's `y >= getMinY() + 1`, spelled as the strict comparison it is
    // — the loop reads `y - 1`, so it stops one above the floor.
    let floor = world.min_y();
    while y > floor {
        let state = world.block_at(x, y - 1, z);
        let plain = state.split_once('[').map_or(state.as_str(), |(n, _)| n);
        if !is_empty(&state) && is_empty(&above) && plain != "minecraft:bedrock" {
            if found == target_layer {
                return Some(y);
            }
            found += 1;
        }
        above = state;
        y -= 1;
    }
    None
}

/// A world that answers nothing, for chains that do not touch it.
///
/// Panics rather than returning a plausible default if a modifier does ask —
/// a heightmap of zero would place features at y=0 across the map, which
/// looks like a generator bug rather than a missing implementation.
pub struct NoWorld;

impl PredicateWorld for NoWorld {
    fn block_at(&mut self, _: i32, _: i32, _: i32) -> String {
        panic!("this chain was expected not to touch the world")
    }
}

impl PlacementWorld for NoWorld {
    fn biome_info_noise(&mut self, _: f64, _: f64) -> f64 {
        panic!("this chain was expected not to touch the world")
    }
    fn block_tags(&self) -> &BlockTags {
        panic!("this chain was expected not to touch the world")
    }
    fn min_y(&self) -> i32 {
        panic!("this chain was expected not to touch the world")
    }
    fn gen_depth(&self) -> i32 {
        panic!("this chain was expected not to touch the world")
    }
    fn heightmap(&mut self, _: HeightmapKind, _: i32, _: i32) -> i32 {
        panic!("this chain was expected not to touch the world")
    }
    fn biome_lists_this_feature(&mut self, _: i32, _: i32, _: i32) -> bool {
        panic!("this chain was expected not to touch the world")
    }
}

/// Where a placement chain died, counted by modifier index.
///
/// A write-level trace answers "what did this feature put in the world", and
/// for a feature that put nothing there is nothing to see. Both ways a
/// feature can produce no blocks — the chain rejecting every attempt, and the
/// feature itself running and declining — look identical from the outside.
/// This separates them, and names the modifier when it is the chain.
///
/// Off unless [`begin_attempt_trace`] is called, and thread-local because the
/// generator's stages are not the only caller.
#[derive(Debug, Default, Clone)]
pub struct AttemptTrace {
    /// `rejected_at[i]` — attempts that reached modifier `i` and got nothing
    /// back from it.
    pub rejected_at: Vec<u64>,
    /// Attempts that survived every modifier and reached the feature.
    pub emitted: u64,
    /// What each modifier in the chain produced, as `(index, position)`.
    ///
    /// The emitted positions alone cannot say which modifier moved a
    /// position where, because a chain only reports its last stage. This
    /// can: a modifier's input is the previous index's output, so a
    /// divergence can be attributed to one modifier rather than to the
    /// chain. Capped, because an outer `count` can produce a few hundred.
    pub stage_positions: Vec<(usize, Pos)>,
    /// Each emission with the draw offset it happened at.
    ///
    /// The position list says *what* was placed and the total draw count
    /// says how much stream the feature spent, but neither locates a
    /// divergence: two runs can emit different positions from the same
    /// offset, or the same position from different offsets, and those are
    /// different bugs. Pairing them is what distinguishes the two.
    pub emitted_at: Vec<(Pos, u64)>,
    /// Where each rejection happened, as `(modifier index, position)`.
    ///
    /// The counts say *which* modifier turns attempts away; these say
    /// **where**, which is what lets a rejection be checked against the
    /// world — the question is usually "is the block at this position what
    /// the filter thinks it is".
    pub rejected_positions: Vec<(usize, Pos)>,
}

thread_local! {
    static ATTEMPTS: std::cell::RefCell<Option<AttemptTrace>> =
        const { std::cell::RefCell::new(None) };
    /// How many chains are running on this thread.
    ///
    /// A `simple_random_selector`'s children are `PlacedFeature`s with
    /// **their own** placement chains, and those run inside the outer
    /// chain's walk. Without this guard their rejections land in the outer
    /// feature's histogram and are labelled against the *outer* modifier
    /// list, which reported 78 rejections at `sulfur_spike`'s modifier 0 —
    /// a `count` that runs once per invocation and cannot reject 78 times.
    /// The number was the inner `environment_scan`'s, wearing the outer
    /// chain's name.
    static NESTING: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Whether `RUSTMC_TRACE_OFFSET` asked for `random_offset`'s deltas.
static TRACE_OFFSET: std::sync::LazyLock<bool> =
    std::sync::LazyLock::new(|| std::env::var_os("RUSTMC_TRACE_OFFSET").is_some());

/// How many stage entries [`AttemptTrace::stage_positions`] keeps **per
/// modifier**.
///
/// Per modifier rather than overall: the first `count` in a chain emits its
/// whole batch — two hundred copies of one position for `sulfur_spike` — and
/// a single shared cap is spent entirely on that before the interesting
/// stages produce anything.
const STAGE_TRACE_CAP: usize = 40;

/// Start counting. Any previous count is discarded.
pub fn begin_attempt_trace() {
    ATTEMPTS.with(|a| *a.borrow_mut() = Some(AttemptTrace::default()));
}

/// Stop counting and take what was counted.
#[must_use]
pub fn take_attempt_trace() -> Option<AttemptTrace> {
    ATTEMPTS.with(|a| a.borrow_mut().take())
}

/// Whether the chain currently running is the one being traced, rather than
/// a nested chain inside it.
fn outermost() -> bool {
    NESTING.with(|n| n.get() <= 1)
}

fn note_rejected(depth: usize, at: Pos) {
    if !outermost() {
        return;
    }
    ATTEMPTS.with(|a| {
        if let Some(t) = a.borrow_mut().as_mut() {
            if t.rejected_at.len() <= depth {
                t.rejected_at.resize(depth + 1, 0);
            }
            t.rejected_at[depth] += 1;
            t.rejected_positions.push((depth, at));
        }
    });
}

fn note_emitted(position: Pos) {
    if !outermost() {
        return;
    }
    let at = crate::rng::draws::peek();
    ATTEMPTS.with(|a| {
        if let Some(t) = a.borrow_mut().as_mut() {
            t.emitted += 1;
            t.emitted_at.push((position, at));
        }
    });
}

/// The depth-first walk. See the module docs for why it is not a loop.
fn walk<R: RandomSource, W: PlacementWorld + ?Sized, F>(
    modifiers: &[PlacementModifier],
    world: &mut W,
    random: &mut R,
    position: Pos,
    emit: &mut F,
) -> Result<(), Unsupported>
where
    F: FnMut(&mut W, &mut R, Pos) -> Result<(), Unsupported>,
{
    walk_at(modifiers, 0, world, random, position, emit)
}

fn walk_at<R: RandomSource, W: PlacementWorld + ?Sized, F>(
    modifiers: &[PlacementModifier],
    depth: usize,
    world: &mut W,
    random: &mut R,
    position: Pos,
    emit: &mut F,
) -> Result<(), Unsupported>
where
    F: FnMut(&mut W, &mut R, Pos) -> Result<(), Unsupported>,
{
    let Some((first, rest)) = modifiers.split_first() else {
        note_emitted(position);
        return emit(world, random, position);
    };
    let next = first.get_positions(world, random, position)?;
    if outermost() {
        ATTEMPTS.with(|a| {
            if let Some(t) = a.borrow_mut().as_mut() {
                let kept = t
                    .stage_positions
                    .iter()
                    .filter(|(d, _)| *d == depth)
                    .count();
                let room = STAGE_TRACE_CAP.saturating_sub(kept);
                for p in next.iter().take(room) {
                    t.stage_positions.push((depth, *p));
                }
            }
        });
    }
    if next.is_empty() {
        note_rejected(depth, position);
    }
    for position in next {
        walk_at(rest, depth + 1, world, random, position, emit)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::{LegacyRandomSource, WorldgenRandom};

    fn random(seed: i64) -> WorldgenRandom<LegacyRandomSource> {
        WorldgenRandom::new(LegacyRandomSource::new(seed))
    }

    const ORIGIN: Pos = Pos { x: 0, y: 0, z: 0 };

    #[test]
    fn the_chain_is_depth_first_not_stage_by_stage() {
        // The whole point of the module. `count(2)` then `in_square` draws:
        //   count -> 2, then in_square for the first, then for the second.
        // Breadth-first would draw the same *number* of values in the same
        // order here — so the observable difference needs a third stage.
        //
        // With count(2), in_square, count(2): depth-first runs
        //   c1=2, sq(a), c2(a)=n, sq(b), c2(b)=m
        // while breadth-first runs
        //   c1=2, sq(a), sq(b), c2(a), c2(b)
        // The second `in_square` therefore sees a different stream position,
        // and the resulting position lists differ.
        let chain = vec![
            PlacementModifier::Count(IntProvider::Constant(2)),
            PlacementModifier::InSquare,
            PlacementModifier::Count(IntProvider::Uniform {
                min_inclusive: 1,
                max_inclusive: 3,
            }),
        ];
        let depth_first =
            positions(&chain, &mut NoWorld, &mut random(42), ORIGIN).expect("supported");

        // Breadth-first, written out longhand as the mistake would be.
        let mut r = random(42);
        let mut stage: Vec<Pos> = vec![ORIGIN];
        for modifier in &chain {
            let mut next = Vec::new();
            for p in stage {
                next.extend(
                    modifier
                        .get_positions(&mut NoWorld, &mut r, p)
                        .expect("supported"),
                );
            }
            stage = next;
        }

        assert_ne!(
            depth_first, stage,
            "depth-first and breadth-first agreed, so this test proves nothing"
        );
    }

    #[test]
    fn count_repeats_the_same_position() {
        let chain = vec![PlacementModifier::Count(IntProvider::Constant(3))];
        let out = positions(&chain, &mut NoWorld, &mut random(1), ORIGIN).expect("supported");
        assert_eq!(out, vec![ORIGIN; 3]);
    }

    #[test]
    fn an_empty_chain_yields_the_origin_once() {
        // `Stream.of(origin)` with nothing applied. A feature with no
        // placement modifiers places exactly once, at the chunk corner.
        let out = positions(&[], &mut NoWorld, &mut random(1), ORIGIN).expect("supported");
        assert_eq!(out, vec![ORIGIN]);
    }

    #[test]
    fn a_zero_count_ends_the_chain_for_that_branch() {
        let chain = vec![
            PlacementModifier::Count(IntProvider::Constant(0)),
            PlacementModifier::InSquare,
        ];
        let mut r = random(1);
        let before = r.count();
        let out = positions(&chain, &mut NoWorld, &mut r, ORIGIN).expect("supported");
        assert!(out.is_empty());
        assert_eq!(
            r.count(),
            before,
            "a zero count must not run the modifiers after it"
        );
    }

    #[test]
    fn in_square_stays_inside_the_chunk_and_keeps_the_y() {
        let chain = vec![PlacementModifier::InSquare];
        let origin = Pos {
            x: 48,
            y: 70,
            z: -32,
        };
        let mut r = random(9);
        for _ in 0..500 {
            let out = positions(&chain, &mut NoWorld, &mut r, origin).expect("supported");
            assert_eq!(out.len(), 1);
            assert!((48..64).contains(&out[0].x), "x {} escaped", out[0].x);
            assert!((-32..-16).contains(&out[0].z), "z {} escaped", out[0].z);
            assert_eq!(out[0].y, 70, "in_square must not move y");
        }
    }

    #[test]
    fn the_rarity_filter_draws_even_when_it_rejects() {
        // The draw is unconditional; only the output is filtered. If it were
        // skipped on rejection, every later modifier in the chain would read
        // a different stream position depending on whether this fired.
        let chain = vec![PlacementModifier::RarityFilter { chance: 4 }];
        let mut r = random(2);
        let mut kept = 0;
        for _ in 0..4000 {
            let before = r.count();
            if !positions(&chain, &mut NoWorld, &mut r, ORIGIN)
                .expect("supported")
                .is_empty()
            {
                kept += 1;
            }
            assert_eq!(r.count() - before, 1, "exactly one draw either way");
        }
        assert!((800..1200).contains(&kept), "kept {kept} of 4000 at 1-in-4");
    }

    #[test]
    fn random_offset_samples_x_then_y_then_z() {
        // The y sample sits between the two xz ones. Using a provider whose
        // successive values differ makes the order observable: if y were
        // sampled last, the z offset would take the value y actually got.
        let chain = vec![PlacementModifier::RandomOffset {
            xz_spread: IntProvider::Uniform {
                min_inclusive: 0,
                max_inclusive: 1000,
            },
            y_spread: IntProvider::Uniform {
                min_inclusive: 0,
                max_inclusive: 1000,
            },
        }];
        let mut r = random(5);
        let out = positions(&chain, &mut NoWorld, &mut r, ORIGIN).expect("supported")[0];

        let mut expected = random(5);
        let spread = IntProvider::Uniform {
            min_inclusive: 0,
            max_inclusive: 1000,
        };
        let x = spread.sample(&mut expected);
        let y = spread.sample(&mut expected);
        let z = spread.sample(&mut expected);
        assert_eq!((out.x, out.y, out.z), (x, y, z));
    }

    /// A world with a flat heightmap and a biome that accepts everything.
    struct FlatWorld {
        surface: i32,
        accepts: bool,
        heightmap_calls: std::cell::Cell<usize>,
        tags: BlockTags,
        noise: f64,
    }

    impl FlatWorld {
        fn new(surface: i32) -> Self {
            Self {
                surface,
                accepts: true,
                heightmap_calls: std::cell::Cell::new(0),
                tags: BlockTags::from_entries(
                    [(
                        "minecraft:dirt".to_string(),
                        vec!["minecraft:stone".to_string()],
                    )]
                    .into_iter()
                    .collect(),
                )
                .expect("tags"),
                noise: 0.0,
            }
        }
    }

    impl PredicateWorld for FlatWorld {
        fn block_at(&mut self, _: i32, y: i32, _: i32) -> String {
            if y < self.surface {
                "minecraft:stone".to_string()
            } else {
                "minecraft:air".to_string()
            }
        }
    }

    impl PlacementWorld for FlatWorld {
        /// A flat noise field, so a count is a function of the config rather
        /// than of the noise — the real values are proved by
        /// `golden_temperature` and reproduced end to end by
        /// `golden_decorate`.
        fn biome_info_noise(&mut self, _: f64, _: f64) -> f64 {
            self.noise
        }
        fn block_tags(&self) -> &BlockTags {
            &self.tags
        }
        fn min_y(&self) -> i32 {
            -64
        }
        fn gen_depth(&self) -> i32 {
            384
        }
        fn heightmap(&mut self, _: HeightmapKind, _: i32, _: i32) -> i32 {
            self.heightmap_calls.set(self.heightmap_calls.get() + 1);
            self.surface
        }
        fn biome_lists_this_feature(&mut self, _: i32, _: i32, _: i32) -> bool {
            self.accepts
        }
    }

    /// The trap in `count_on_every_layer`: the count is the loop *condition*,
    /// so a uniform provider draws once per iteration and once more for the
    /// test that ends the loop.
    ///
    /// `FlatWorld` is solid below the surface and air above, so every column
    /// has exactly one ground layer — layer 0 finds it, layer 1 finds
    /// nothing, and the outer loop stops after two passes.
    #[test]
    fn the_layer_count_is_redrawn_every_iteration() {
        let modifier = PlacementModifier::CountOnEveryLayer(IntProvider::Uniform {
            min_inclusive: 2,
            max_inclusive: 2,
        });
        let mut world = FlatWorld::new(70);
        let mut r = random(9);
        let positions = modifier
            .get_positions(&mut world, &mut r, ORIGIN)
            .expect("supported");
        assert_eq!(positions.len(), 2, "two positions on the one ground layer");

        // Two passes of two iterations. Each iteration: one count sample, one
        // x, one z. Each pass also samples the count once more to fail the
        // condition. A port that hoisted the sample out of the loop would
        // take six draws fewer.
        let mut probe = random(9);
        let mut expected = 0;
        for _ in 0..2 {
            for _ in 0..2 {
                probe.next_i32_bound(1); // the uniform count
                probe.next_i32_bound(16);
                probe.next_i32_bound(16);
                expected += 3;
            }
            probe.next_i32_bound(1); // the sample that ends the loop
            expected += 1;
        }
        assert_eq!(expected, 14, "twelve inside the loops, two to end them");
        assert_eq!(
            r.next_i64(),
            probe.next_i64(),
            "the modifier and the hand-counted stream must be at the same place"
        );
    }

    /// Bedrock is never a layer floor, which is what stops features spawning
    /// on the world's bottom slab.
    #[test]
    fn bedrock_is_not_a_floor() {
        struct Bedrock;
        impl PredicateWorld for Bedrock {
            fn block_at(&mut self, _: i32, y: i32, _: i32) -> String {
                if y <= -64 {
                    "minecraft:bedrock".to_string()
                } else {
                    "minecraft:air".to_string()
                }
            }
        }
        impl PlacementWorld for Bedrock {
            fn biome_info_noise(&mut self, _: f64, _: f64) -> f64 {
                0.0
            }
            fn block_tags(&self) -> &BlockTags {
                unreachable!("no tag is asked for")
            }
            fn min_y(&self) -> i32 {
                -64
            }
            fn gen_depth(&self) -> i32 {
                384
            }
            fn heightmap(&mut self, _: HeightmapKind, _: i32, _: i32) -> i32 {
                0
            }
            fn biome_lists_this_feature(&mut self, _: i32, _: i32, _: i32) -> bool {
                true
            }
        }

        let modifier = PlacementModifier::CountOnEveryLayer(IntProvider::Constant(1));
        let positions = modifier
            .get_positions(&mut Bedrock, &mut random(3), ORIGIN)
            .expect("supported");
        assert!(positions.is_empty(), "{positions:?}");
    }

    #[test]
    fn height_range_replaces_the_y_and_keeps_x_and_z() {
        let chain = vec![PlacementModifier::HeightRange(HeightProvider::Uniform {
            min_inclusive: crate::surface::rule::VerticalAnchor::Absolute(0),
            max_inclusive: crate::surface::rule::VerticalAnchor::Absolute(64),
        })];
        let origin = Pos {
            x: 13,
            y: 999,
            z: -7,
        };
        let mut world = FlatWorld::new(70);
        let mut r = random(4);
        for _ in 0..200 {
            let out = positions(&chain, &mut world, &mut r, origin).expect("supported");
            assert_eq!(out.len(), 1);
            assert_eq!((out[0].x, out[0].z), (13, -7));
            assert!((0..=64).contains(&out[0].y), "y {} escaped", out[0].y);
        }
    }

    #[test]
    fn a_heightmap_at_the_world_floor_produces_nothing() {
        // `height > context.getMinY()`, strictly. A column whose heightmap
        // sits exactly at the floor yields no position rather than one at
        // the floor — which is how features stop at the bottom of the world
        // instead of piling up there.
        let chain = vec![PlacementModifier::Heightmap(HeightmapKind::WorldSurfaceWg)];
        let mut r = random(1);

        let mut at_floor = FlatWorld::new(-64);
        assert!(positions(&chain, &mut at_floor, &mut r, ORIGIN)
            .expect("supported")
            .is_empty());

        let mut above = FlatWorld::new(-63);
        let out = positions(&chain, &mut above, &mut r, ORIGIN).expect("supported");
        assert_eq!(out, vec![Pos { x: 0, y: -63, z: 0 }]);
    }

    #[test]
    fn the_biome_filter_draws_nothing_either_way() {
        // Unlike `rarity_filter`, which draws whether or not it keeps the
        // position. Adding a draw here would shift every later modifier in
        // every chain that has one — which is 208 of the 262.
        let chain = vec![PlacementModifier::BiomeFilter];
        let mut r = random(2);

        let mut accepting = FlatWorld::new(70);
        let before = r.count();
        assert_eq!(
            positions(&chain, &mut accepting, &mut r, ORIGIN).expect("supported"),
            vec![ORIGIN]
        );
        assert_eq!(r.count(), before, "the biome filter must not draw");

        let mut rejecting = FlatWorld::new(70);
        rejecting.accepts = false;
        assert!(positions(&chain, &mut rejecting, &mut r, ORIGIN)
            .expect("supported")
            .is_empty());
        assert_eq!(r.count(), before, "still no draw when it rejects");
    }

    #[test]
    fn an_ore_shaped_chain_runs_end_to_end() {
        // The shape most ore features use: count, in_square, height_range,
        // biome. Checks the pieces compose and that the heightmap is not
        // consulted by a chain that does not name it.
        let chain = vec![
            PlacementModifier::Count(IntProvider::Constant(4)),
            PlacementModifier::InSquare,
            PlacementModifier::HeightRange(HeightProvider::Uniform {
                min_inclusive: crate::surface::rule::VerticalAnchor::AboveBottom(0),
                max_inclusive: crate::surface::rule::VerticalAnchor::Absolute(64),
            }),
            PlacementModifier::BiomeFilter,
        ];
        let mut world = FlatWorld::new(70);
        let out =
            positions(&chain, &mut world, &mut random(20_260_725), ORIGIN).expect("supported");
        assert_eq!(out.len(), 4);
        for p in &out {
            assert!((0..16).contains(&p.x));
            assert!((0..16).contains(&p.z));
            assert!((-64..=64).contains(&p.y));
        }
        assert_eq!(world.heightmap_calls.get(), 0);
    }

    #[test]
    fn a_world_dependent_modifier_fails_loudly() {
        // Skipping it would place the feature anyway, at whatever position
        // reached it — a forest growing underwater rather than an error.
        let chain = vec![
            PlacementModifier::Count(IntProvider::Constant(1)),
            PlacementModifier::NeedsWorld("minecraft:heightmap".into()),
        ];
        let error =
            positions(&chain, &mut NoWorld, &mut random(1), ORIGIN).expect_err("should refuse");
        assert_eq!(error.0, "minecraft:heightmap");
        assert!(error.to_string().contains("heightmap"));
    }

    #[test]
    fn the_real_placed_features_parse() {
        let dir = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../oracle-work/vanilla-26.2/generated-data/data/minecraft/worldgen/placed_feature"
        ));
        if !rustmc_data::fixture::available(dir, "datagen extract", "ci/vanilla-server.sh data") {
            return;
        }
        let mut total = 0usize;
        let mut runnable = 0usize;
        let mut needs_world: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        for entry in std::fs::read_dir(dir).expect("readable") {
            let path = entry.expect("entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let json: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).expect("readable"))
                    .expect("json");
            let chain: Vec<PlacementModifier> = json["placement"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|m| {
                    PlacementModifier::parse(m)
                        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
                })
                .collect();
            total += 1;
            if chain.iter().all(PlacementModifier::is_supported) {
                runnable += 1;
            } else {
                for m in &chain {
                    match m {
                        PlacementModifier::NeedsWorld(name) => {
                            *needs_world.entry(name.clone()).or_insert(0) += 1;
                        }
                        PlacementModifier::BlockPredicateFilter(p) if !p.is_supported() => {
                            *needs_world
                                .entry("block predicate needing behaviour".to_string())
                                .or_insert(0) += 1;
                        }
                        _ => {}
                    }
                }
            }
        }
        assert!(total > 200, "only {total} placed features");
        assert!(
            runnable >= 262,
            "only {runnable}/{total} chains runnable — coverage went backwards"
        );
        // Reported and floored: this is how much of the stage is built, and
        // it should only go up. The floor is a ratchet like the surface
        // gate's coverage number.
        println!("PLACEMENT: {runnable}/{total} chains fully implemented");
        println!("  still needed: {needs_world:?}");
        assert!(runnable > 0, "no chain is runnable at all");
    }
}
