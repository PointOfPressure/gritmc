//! Amethyst geodes.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `GeodeFeature`, `GeodeConfiguration`, `GeodeLayerSettings`,
//! `GeodeCrackSettings`, `BuddingAmethystBlock#canClusterGrowAtState`,
//! `Mth#invSqrt`.
//!
//! The largest single item in the whole-world diff once mineshafts landed:
//! ~700 blocks across six rows, all in one band (docs/BENCH.md).
//!
//! # The distance field is larger nearer the centre
//!
//! A geode is not a sphere test. Every position sums an **inverse** square
//! root of its distance to each of a handful of scattered points, so the
//! field *grows* towards the middle:
//!
//! ```text
//! distance = sum over points of invSqrt(distSqr(point) + point_offset) + noise
//! ```
//!
//! Which means the thresholds read inside-out — `filling > inner > middle >
//! outer` — and the early bail is the *outside* of the geode. Reading them
//! the intuitive way round builds a geode with its layers inverted, which
//! still looks like a geode from the outside and is wrong everywhere.
//!
//! # The noise is seeded from the world seed
//!
//! `NormalNoise.create(new WorldgenRandom(new LegacyRandomSource(level
//! .getSeed())), -4, [1.0])` — not from the feature stream, and not
//! positionally forked. Every geode in a world shares one field, and it is
//! rebuilt from scratch on every placement.
//!
//! # A refusal still costs draws
//!
//! The invalid-block bail happens *after* that point's three offset draws,
//! so a geode that declines has already moved the stream. Checking the
//! blocks first — the obvious way to write it — is cheaper and wrong.

// Read once, not per geode and not per block of one. See `crate::trace_switch`.
crate::trace_switch_value!(trace_geode, "RUSTMC_TRACE_GEODE");
use super::int_provider::IntProvider;
use super::state_spec::DefaultStates;
use crate::noise::normal::NormalNoise;
use crate::rng::legacy::LegacyRandomSource;
use crate::rng::RandomSource;
use crate::tags::BlockTags;

/// `GeodeLayerSettings` — the four radii, as `1/sqrt` thresholds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layers {
    pub filling: f64,
    pub inner: f64,
    pub middle: f64,
    pub outer: f64,
}

impl Default for Layers {
    fn default() -> Self {
        Self {
            filling: 1.7,
            inner: 2.2,
            middle: 3.2,
            outer: 4.2,
        }
    }
}

/// `GeodeCrackSettings`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crack {
    pub generate_chance: f64,
    pub base_size: f64,
    pub point_offset: i32,
}

impl Default for Crack {
    fn default() -> Self {
        Self {
            generate_chance: 1.0,
            base_size: 2.0,
            // Two, not five — the 5.0 next to it in the codec is the upper
            // bound of `base_crack_size`'s range, not this default.
            point_offset: 2,
        }
    }
}

/// `GeodeConfiguration`.
#[derive(Clone, Debug, PartialEq)]
pub struct GeodeConfig {
    pub filling_block: String,
    pub inner_block: String,
    pub alternate_inner_block: String,
    pub middle_block: String,
    pub outer_block: String,
    /// `inner_placements` — the buds, in order.
    pub buds: Vec<String>,
    /// `cannot_replace`, as a tag name without the `#`.
    pub cannot_replace_tag: Option<String>,
    /// `invalid_blocks`, likewise.
    pub invalid_blocks_tag: Option<String>,
    pub layers: Layers,
    pub crack: Crack,
    pub use_potential_placements_chance: f64,
    pub use_alternate_layer0_chance: f64,
    pub placements_require_layer0_alternate: bool,
    pub outer_wall_distance: IntProvider,
    pub distribution_points: IntProvider,
    pub point_offset: IntProvider,
    pub min_gen_offset: i32,
    pub max_gen_offset: i32,
    pub noise_multiplier: f64,
    pub invalid_blocks_threshold: i32,
}

impl GeodeConfig {
    /// # Errors
    /// If a required field is missing or malformed.
    ///
    /// Most fields are optional with defaults, and `amethyst_geode` leaves
    /// nine of them unset — which is exactly what makes those defaults
    /// load-bearing. They are read off the codec rather than remembered.
    pub fn parse(config: &serde_json::Value, states: &dyn DefaultStates) -> Result<Self, String> {
        let blocks = config
            .get("blocks")
            .ok_or_else(|| "geode has no `blocks`".to_string())?;
        let provider_state = |name: &str| -> Result<String, String> {
            let provider = blocks
                .get(name)
                .ok_or_else(|| format!("geode blocks has no `{name}`"))?;
            let state = provider
                .get("state")
                .ok_or_else(|| format!("geode `{name}` is not a simple state provider"))?;
            super::state_spec::canonical(state, states)
        };
        let tag = |name: &str| -> Option<String> {
            blocks
                .get(name)
                .and_then(serde_json::Value::as_str)
                .and_then(|v| v.strip_prefix('#'))
                .map(ToString::to_string)
        };
        let buds = blocks
            .get("inner_placements")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| "geode blocks has no `inner_placements`".to_string())?
            .iter()
            .map(|v| super::state_spec::canonical(v, states))
            .collect::<Result<Vec<_>, String>>()?;

        let double = |value: &serde_json::Value, name: &str, default: f64| -> f64 {
            value
                .get(name)
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(default)
        };
        let int = |value: &serde_json::Value, name: &str, default: i32| -> i32 {
            value
                .get(name)
                .and_then(serde_json::Value::as_i64)
                .and_then(|v| i32::try_from(v).ok())
                .unwrap_or(default)
        };
        let empty = serde_json::Value::Object(serde_json::Map::new());
        let layers_json = config.get("layers").unwrap_or(&empty);
        let crack_json = config.get("crack").unwrap_or(&empty);
        let defaults = Layers::default();
        let crack_defaults = Crack::default();

        let provider = |name: &str, min: i32, max: i32| -> Result<IntProvider, String> {
            match config.get(name) {
                None => Ok(IntProvider::Uniform {
                    min_inclusive: min,
                    max_inclusive: max,
                }),
                Some(v) => IntProvider::parse(v),
            }
        };

        Ok(Self {
            filling_block: provider_state("filling_provider")?,
            inner_block: provider_state("inner_layer_provider")?,
            alternate_inner_block: provider_state("alternate_inner_layer_provider")?,
            middle_block: provider_state("middle_layer_provider")?,
            outer_block: provider_state("outer_layer_provider")?,
            buds,
            cannot_replace_tag: tag("cannot_replace"),
            invalid_blocks_tag: tag("invalid_blocks"),
            layers: Layers {
                filling: double(layers_json, "filling", defaults.filling),
                inner: double(layers_json, "inner_layer", defaults.inner),
                middle: double(layers_json, "middle_layer", defaults.middle),
                outer: double(layers_json, "outer_layer", defaults.outer),
            },
            crack: Crack {
                generate_chance: double(
                    crack_json,
                    "generate_crack_chance",
                    crack_defaults.generate_chance,
                ),
                base_size: double(crack_json, "base_crack_size", crack_defaults.base_size),
                point_offset: int(
                    crack_json,
                    "crack_point_offset",
                    crack_defaults.point_offset,
                ),
            },
            use_potential_placements_chance: double(
                config,
                "use_potential_placements_chance",
                0.35,
            ),
            use_alternate_layer0_chance: double(config, "use_alternate_layer0_chance", 0.0),
            placements_require_layer0_alternate: config
                .get("placements_require_layer0_alternate")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true),
            outer_wall_distance: provider("outer_wall_distance", 4, 5)?,
            distribution_points: provider("distribution_points", 3, 4)?,
            point_offset: provider("point_offset", 1, 2)?,
            min_gen_offset: int(config, "min_gen_offset", -16),
            max_gen_offset: int(config, "max_gen_offset", 16),
            noise_multiplier: double(config, "noise_multiplier", 0.05),
            invalid_blocks_threshold: config
                .get("invalid_blocks_threshold")
                .and_then(serde_json::Value::as_i64)
                .and_then(|v| i32::try_from(v).ok())
                .ok_or_else(|| "geode has no `invalid_blocks_threshold`".to_string())?,
        })
    }

    #[must_use]
    pub const fn is_supported(&self) -> bool {
        true
    }

    /// The largest value `outer_wall_distance` can produce, which scales
    /// three of the four layer thresholds — so it is arithmetic, not a
    /// validation bound.
    fn wall_max(&self) -> i32 {
        self.outer_wall_distance.max_value()
    }
}

/// The world a geode reads and writes.
pub trait GeodeWorld {
    fn block_at(&mut self, x: i32, y: i32, z: i32) -> String;
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str);
    /// `state.isAir()`.
    fn is_air(&mut self, x: i32, y: i32, z: i32) -> bool;
    /// `state.getFluidState().isSource()`.
    fn is_fluid_source(&mut self, x: i32, y: i32, z: i32) -> bool;
    /// `!state.getFluidState().isEmpty()`.
    fn has_fluid(&mut self, x: i32, y: i32, z: i32) -> bool;
    /// The world seed, which the geode's noise is built from.
    fn world_seed(&mut self) -> i64;
}

/// The feature could not be evaluated.
#[derive(Debug, PartialEq, Eq)]
pub struct NeedsRules(pub String);

fn plain(state: &str) -> &str {
    state.split_once('[').map_or(state, |(n, _)| n)
}

/// `Mth.invSqrt` — `org.joml.Math.invsqrt`, which is plainly
/// `1.0 / Math.sqrt(x)`. Checked, because a fast inverse square root here
/// would be a different function and this port forbids fast math.
fn inv_sqrt(x: f64) -> f64 {
    1.0 / x.sqrt()
}

/// `Direction.values()` offsets.
const DIRECTIONS: [(i32, i32, i32); 6] = [
    (0, -1, 0),
    (0, 1, 0),
    (0, 0, -1),
    (0, 0, 1),
    (-1, 0, 0),
    (1, 0, 0),
];

const FACING_NAMES: [&str; 6] = ["down", "up", "north", "south", "west", "east"];

/// `GeodeFeature#place`.
///
/// # Errors
/// If the world cannot answer a block question.
#[expect(
    clippy::too_many_lines,
    reason = "one method in the reference, and splitting it would hide the \
              order the draws happen in"
)]
pub fn place<W: GeodeWorld + ?Sized, R: RandomSource + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    config: &GeodeConfig,
    random: &mut R,
    origin: (i32, i32, i32),
) -> Result<bool, NeedsRules> {
    // Priced per site under `RUSTMC_TRACE_GEODE`, not as one total. A body
    // short by 26 draws is short somewhere specific, and these sites scale
    // differently — four per distribution point, one per crack point, two
    // coins per block reaching the layer tests — so a total cannot say which
    // loop ran fewer times.
    let trace = trace_geode().is_some();
    let distribution_points = config.distribution_points.sample(random);

    // Rebuilt per placement, from the *world* seed. See the module docs.
    // Vanilla wraps this in a `WorldgenRandom`, which is a pure delegator —
    // it forwards `next(bits)` and `forkPositional` to the source it holds
    // and changes no value. The wrapper is omitted rather than reproduced.
    let seed = world.world_seed();
    let mut noise_random = LegacyRandomSource::new(seed);
    let noise = NormalNoise::create(&mut noise_random, -4, &[1.0]);

    let inv_distribution = f64::from(distribution_points) / f64::from(config.wall_max());
    let filling = 1.0 / config.layers.filling.sqrt();
    let inner = 1.0 / (config.layers.inner + inv_distribution).sqrt();
    let middle = 1.0 / (config.layers.middle + inv_distribution).sqrt();
    let outer = 1.0 / (config.layers.outer + inv_distribution).sqrt();
    let crack_size = 1.0
        / (config.crack.base_size
            + random.next_f64() / 2.0
            + if distribution_points > 3 {
                inv_distribution
            } else {
                0.0
            })
        .sqrt();
    let generate_crack = f64::from(random.next_f32()) < config.crack.generate_chance;

    let mut points: Vec<((i32, i32, i32), i32)> = Vec::new();
    let mut invalid = 0;
    for _ in 0..distribution_points {
        // Three draws, spent before the block is looked at.
        let dx = config.outer_wall_distance.sample(random);
        let dy = config.outer_wall_distance.sample(random);
        let dz = config.outer_wall_distance.sample(random);
        let at = (origin.0 + dx, origin.1 + dy, origin.2 + dz);
        let is_invalid = world.is_air(at.0, at.1, at.2) || {
            let state = world.block_at(at.0, at.1, at.2);
            config
                .invalid_blocks_tag
                .as_ref()
                .is_some_and(|t| tags.contains(t, plain(&state)))
        };
        if is_invalid {
            invalid += 1;
            if invalid > config.invalid_blocks_threshold {
                return Ok(false);
            }
        }
        points.push((at, config.point_offset.sample(random)));
    }

    let mut crack_points: Vec<(i32, i32, i32)> = Vec::new();
    if generate_crack {
        let side = random.next_i32_bound(4);
        if trace {
            GEODE_CRACK_SIDES.with(|c| c.set(c.get() + 1));
        }
        let span = distribution_points * 2 + 1;
        // One face of the geode opens up. Three points at fixed heights —
        // 7, 5 and 1 — and the horizontal offsets say which face.
        //
        // Read, not inferred. An earlier version of this guessed sides 1 and
        // 3 from the symmetry of side 0 and put a 5 where the x offset
        // belongs; the whole-world diff found it as ~60 blocks of geode wall
        // opened in the wrong place. Three of the four sides look alike and
        // the fourth does not.
        let offsets: [(i32, i32, i32); 3] = match side {
            0 => [(span, 7, 0), (span, 5, 0), (span, 1, 0)],
            1 => [(0, 7, span), (0, 5, span), (0, 1, span)],
            2 => [(span, 7, span), (span, 5, span), (span, 1, span)],
            _ => [(0, 7, 0), (0, 5, 0), (0, 1, 0)],
        };
        for (dx, dy, dz) in offsets {
            crack_points.push((origin.0 + dx, origin.1 + dy, origin.2 + dz));
        }
    }

    let mut placements: Vec<(i32, i32, i32)> = Vec::new();
    let cannot_replace = |world: &mut W, x: i32, y: i32, z: i32| -> bool {
        let state = world.block_at(x, y, z);
        config
            .cannot_replace_tag
            .as_ref()
            .is_some_and(|t| tags.contains(t, plain(&state)))
    };

    // `BlockPos.betweenClosed` walks a `Cursor3D`, and its order is
    // `x = i % width; y = (i / width) % height; z = i / width / height` —
    // **x fastest, then y, then z**. So the nesting is z, y, x.
    //
    // That is not cosmetic: the alternate-layer coin is drawn once per
    // position in the inner layer, so the order decides which block each
    // drawn value belongs to. A different nesting produces a geode with the
    // right number of budding amethysts in the wrong places — which is
    // exactly what the whole-world diff showed, 16 blocks each way.
    for z in (origin.2 + config.min_gen_offset)..=(origin.2 + config.max_gen_offset) {
        for y in (origin.1 + config.min_gen_offset)..=(origin.1 + config.max_gen_offset) {
            for x in (origin.0 + config.min_gen_offset)..=(origin.0 + config.max_gen_offset) {
                let n = noise.get_value(f64::from(x), f64::from(y), f64::from(z))
                    * config.noise_multiplier;
                let mut distance = 0.0;
                for ((px, py, pz), offset) in &points {
                    let d = dist_sqr((x, y, z), (*px, *py, *pz));
                    distance += inv_sqrt(d + f64::from(*offset)) + n;
                }
                let mut crack_distance = 0.0;
                for (cx, cy, cz) in &crack_points {
                    let d = dist_sqr((x, y, z), (*cx, *cy, *cz));
                    crack_distance += inv_sqrt(d + f64::from(config.crack.point_offset)) + n;
                }
                // `RUSTMC_TRACE_GEODE=x,y,z` prints the decision terms for
                // one block. The geode's draw count and origin both match
                // vanilla exactly while its block set does not, so the fault
                // is in this arithmetic and nowhere else — and the terms are
                // not recoverable from the blocks it writes.
                if let Some(want) = trace_geode() {
                    let mut it = want.split(',').map(str::parse::<i32>);
                    if let (Some(Ok(tx)), Some(Ok(ty)), Some(Ok(tz))) =
                        (it.next(), it.next(), it.next())
                    {
                        if (tx, ty, tz) == (x, y, z) {
                            eprintln!(
                                "GEODE ({x},{y},{z}) n={n:.17} distance={distance:.17} \
                                 outer={outer:.17} middle={middle:.17} inner={inner:.17} \
                                 filling={filling:.17} points={}",
                                points.len()
                            );
                        }
                    }
                }

                // Outside the geode entirely.
                if distance < outer {
                    continue;
                }

                if generate_crack && crack_distance >= crack_size && distance < filling {
                    if !cannot_replace(world, x, y, z) {
                        world.set_block(x, y, z, "minecraft:air");
                    }
                    // Vanilla schedules a fluid tick on each neighbour so an
                    // opened geode drains. This port has no scheduled ticks,
                    // and the blocks it would move are named in `caveats`.
                    for (dx, dy, dz) in DIRECTIONS {
                        let _ = world.has_fluid(x + dx, y + dy, z + dz);
                    }
                } else if distance >= filling {
                    if !cannot_replace(world, x, y, z) {
                        let state = config.filling_block.clone();
                        world.set_block(x, y, z, &state);
                    }
                } else if distance >= inner {
                    let alternate =
                        f64::from(random.next_f32()) < config.use_alternate_layer0_chance;
                    if trace {
                        GEODE_LAYER0.with(|c| c.set(c.get() + 1));
                    }
                    let state = if alternate {
                        config.alternate_inner_block.clone()
                    } else {
                        config.inner_block.clone()
                    };
                    if !cannot_replace(world, x, y, z) {
                        world.set_block(x, y, z, &state);
                    }
                    // The second draw only follows when the first condition
                    // holds, so the two are not a fixed pair.
                    if !config.placements_require_layer0_alternate || alternate {
                        let take =
                            f64::from(random.next_f32()) < config.use_potential_placements_chance;
                        if trace {
                            GEODE_POTENTIAL.with(|c| c.set(c.get() + 1));
                        }
                        if take {
                            placements.push((x, y, z));
                        }
                    }
                } else if distance >= middle {
                    if !cannot_replace(world, x, y, z) {
                        let state = config.middle_block.clone();
                        world.set_block(x, y, z, &state);
                    }
                } else if distance >= outer && !cannot_replace(world, x, y, z) {
                    let state = config.outer_block.clone();
                    world.set_block(x, y, z, &state);
                }
            }
        }
    }

    // The buds. One draw per placement, then a walk that stops at the first
    // face that will take a cluster.
    for (x, y, z) in placements {
        if config.buds.is_empty() {
            break;
        }
        #[expect(
            clippy::cast_possible_wrap,
            clippy::cast_possible_truncation,
            reason = "a list of four"
        )]
        let index = random.next_i32_bound(config.buds.len() as i32);
        let mut bud = config.buds[usize::try_from(index).unwrap_or(0)].clone();
        for (i, (dx, dy, dz)) in DIRECTIONS.into_iter().enumerate() {
            // `bud` is mutated as the loop goes, so a later direction
            // inherits the facing and waterlogging set by an earlier one.
            // That is the reference's own behaviour.
            if bud.contains("facing=") {
                bud = with_property(&bud, "facing", FACING_NAMES[i]);
            }
            let (nx, ny, nz) = (x + dx, y + dy, z + dz);
            if bud.contains("waterlogged=") {
                let source = world.is_fluid_source(nx, ny, nz);
                bud = with_property(&bud, "waterlogged", if source { "true" } else { "false" });
            }
            // `BuddingAmethystBlock.canClusterGrowAtState`: air, or full
            // water.
            let grows = world.is_air(nx, ny, nz) || {
                let state = world.block_at(nx, ny, nz);
                plain(&state) == "minecraft:water" && world.is_fluid_source(nx, ny, nz)
            };
            if grows {
                if !cannot_replace(world, nx, ny, nz) {
                    world.set_block(nx, ny, nz, &bud);
                }
                break;
            }
        }
    }

    Ok(true)
}

/// `Vec3i.distSqr` — the plain squared distance, in doubles.
fn dist_sqr(a: (i32, i32, i32), b: (i32, i32, i32)) -> f64 {
    let dx = f64::from(a.0 - b.0);
    let dy = f64::from(a.1 - b.1);
    let dz = f64::from(a.2 - b.2);
    dx * dx + dy * dy + dz * dz
}

fn with_property(state: &str, property: &str, value: &str) -> String {
    let Some((name, rest)) = state.split_once('[') else {
        return state.to_string();
    };
    let pairs: Vec<String> = rest
        .trim_end_matches(']')
        .split(',')
        .map(|pair| match pair.split_once('=') {
            Some((k, _)) if k == property => format!("{property}={value}"),
            _ => pair.to_string(),
        })
        .collect();
    format!("{name}[{}]", pairs.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every default `amethyst_geode` leaves unset, read off the codec.
    #[test]
    fn the_codec_defaults_are_the_reference_ones() {
        let layers = Layers::default();
        assert!((layers.filling - 1.7).abs() < 1e-12);
        assert!((layers.inner - 2.2).abs() < 1e-12);
        assert!((layers.middle - 3.2).abs() < 1e-12);
        assert!((layers.outer - 4.2).abs() < 1e-12);
        let crack = Crack::default();
        assert!((crack.generate_chance - 1.0).abs() < 1e-12);
        assert!((crack.base_size - 2.0).abs() < 1e-12);
        assert_eq!(
            crack.point_offset, 2,
            "two, not the 5.0 that sits next to it as a range bound"
        );
    }

    /// The thresholds read inside-out: a position deep in the geode has the
    /// *largest* distance, because the field is a sum of inverse roots.
    #[test]
    fn the_distance_field_grows_towards_the_centre() {
        let centre = inv_sqrt(dist_sqr((0, 0, 0), (0, 0, 0)) + 1.0);
        let near = inv_sqrt(dist_sqr((2, 0, 0), (0, 0, 0)) + 1.0);
        let far = inv_sqrt(dist_sqr((10, 0, 0), (0, 0, 0)) + 1.0);
        assert!(centre > near, "{centre} should exceed {near}");
        assert!(near > far, "{near} should exceed {far}");

        // And the layer thresholds order the same way, so `filling` is the
        // innermost test and `outer` the outermost.
        let layers = Layers::default();
        let filling = 1.0 / layers.filling.sqrt();
        let inner = 1.0 / (layers.inner + 0.5).sqrt();
        let middle = 1.0 / (layers.middle + 0.5).sqrt();
        let outer = 1.0 / (layers.outer + 0.5).sqrt();
        assert!(filling > inner && inner > middle && middle > outer);
    }

    /// `invSqrt` is plain — `org.joml.Math.invsqrt` is `1/sqrt`, not a fast
    /// approximation, and this port forbids fast math.
    #[test]
    fn inv_sqrt_is_exact() {
        for x in [1.0, 2.0, 4.0, 100.0, 1e6] {
            assert_eq!(inv_sqrt(x), 1.0 / x.sqrt());
        }
        assert_eq!(inv_sqrt(4.0), 0.5);
    }

    #[test]
    fn the_datapack_geode_parses_with_its_defaults() {
        let Some(extract) = super::super::state_spec::extract_or_skip() else {
            return;
        };
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../oracle-work/vanilla-26.2/generated-data/data/minecraft/worldgen/configured_feature/amethyst_geode.json"
        );
        if !rustmc_data::fixture::available(
            std::path::Path::new(path),
            "datagen extract",
            "ci/vanilla-server.sh data",
        ) {
            return;
        }
        // Present, so an unreadable one is a fault and not an absence.
        let text = std::fs::read_to_string(path).expect("the geode config is readable");
        let json: serde_json::Value = serde_json::from_str(&text).expect("json");
        let config = GeodeConfig::parse(&json["config"], &extract).expect("parses");

        // What the datapack sets.
        assert!((config.use_alternate_layer0_chance - 0.083).abs() < 1e-12);
        assert_eq!(config.invalid_blocks_threshold, 1);
        assert!((config.crack.generate_chance - 0.95).abs() < 1e-12);
        // What it leaves to the codec, which is most of it.
        assert_eq!(config.layers, Layers::default(), "`layers: {{}}`");
        assert!((config.noise_multiplier - 0.05).abs() < 1e-12);
        assert!((config.use_potential_placements_chance - 0.35).abs() < 1e-12);
        assert!(config.placements_require_layer0_alternate);
        assert_eq!(config.min_gen_offset, -16);
        assert_eq!(config.max_gen_offset, 16);
        assert_eq!(config.crack.point_offset, 2);
        // And the blocks, as states rather than names.
        assert_eq!(config.outer_block, "minecraft:smooth_basalt");
        assert_eq!(config.middle_block, "minecraft:calcite");
        assert_eq!(config.inner_block, "minecraft:amethyst_block");
        assert_eq!(config.alternate_inner_block, "minecraft:budding_amethyst");
        assert_eq!(config.buds.len(), 4, "four sizes of bud");
        assert!(
            config.buds[0].contains("facing=") && config.buds[0].contains("waterlogged="),
            "a bud carries both properties: {}",
            config.buds[0]
        );
        assert_eq!(
            config.cannot_replace_tag.as_deref(),
            Some("minecraft:features_cannot_replace")
        );
    }
}

thread_local! {
    /// Crack-side draws, one per distribution point when a crack generates.
    static GEODE_CRACK_SIDES: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// `use_alternate_layer0_chance` coins — one per block reaching layer 0.
    static GEODE_LAYER0: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// `use_potential_placements_chance` coins, which follow the layer-0 coin
    /// only when its condition holds, so the two are not a fixed pair.
    static GEODE_POTENTIAL: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// The geode's per-site draw counts since the last call.
#[must_use]
pub fn take_geode_counts() -> (u32, u32, u32) {
    (
        GEODE_CRACK_SIDES.with(std::cell::Cell::take),
        GEODE_LAYER0.with(std::cell::Cell::take),
        GEODE_POTENTIAL.with(std::cell::Cell::take),
    )
}
