//! `TreeDecorator` — what goes on a tree after it is grown.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `TreeDecorator`, `TreeDecorator.Context`, `BeehiveDecorator`,
//! `PlaceOnGroundDecorator`, `TreeFeature#getLowestTrunkOrRootOfTree`.
//!
//! # The context sorts, but only by y, and stably
//!
//! `TreeDecorator.Context` copies each of the tree's three `HashSet`s into an
//! `ObjectArrayList` and sorts it with `Comparator.comparingInt(Vec3i::getY)`.
//! That is a **stable** sort on one key, so everything at the same height
//! keeps the set's iteration order — which is why `crate::java_hash_set`
//! exists. A beehive attaches to `logs.getFirst()`, and at a tree's base
//! several logs share a y.
//!
//! # `place_on_ground` draws 384 times whatever it does
//!
//! ```text
//! for (i = 0; i < tries; i++) {
//!     cursor.set(nextIntBetweenInclusive(box.minX, box.maxX),
//!                nextIntBetweenInclusive(box.minY, box.maxY),
//!                nextIntBetweenInclusive(box.minZ, box.maxZ));
//!     attemptToPlaceBlockAbove(context, cursor);
//! }
//! ```
//!
//! `tries` defaults to 128 and every try takes three draws before a block is
//! read, so a leaf-litter decorator costs 384 draws whether it places
//! anything or not. It is the single most expensive thing in the decoration
//! stage per call, and a port that skipped it when the ground was wrong would
//! move every feature after it.
//!
//! # What the beehive does not do yet
//!
//! The bee nest block is placed and its **draws are taken** — `2 + nextInt(2)`
//! occupants, each `nextInt(599)` for its time-in-hive — but the block entity
//! that holds them is not written. The stream is right and the NBT is
//! missing, which is the honest half to have: a hive with no bees is visible
//! and fixable, a shifted stream is neither.

use super::leaves::BoundingBox;
use super::{NeedsRules, Placed, Pos};
use crate::feature::state_provider::StateProvider;
use crate::feature::state_spec::DefaultStates;
use crate::rng::RandomSource;
use crate::tags::BlockTags;

/// `Direction.SOUTH` — `BeehiveDecorator.WORLDGEN_FACING`, and the direction
/// the nest's entrance faces.
const WORLDGEN_FACING: (i32, i32, i32) = (0, 0, 1);

/// `BeehiveDecorator.SPAWN_DIRECTIONS` — the horizontal plane in
/// `Direction.Plane` order, minus the opposite of `WORLDGEN_FACING`. So
/// NORTH, EAST, SOUTH, WEST minus NORTH.
const SPAWN_DIRECTIONS: [(i32, i32, i32); 3] = [(1, 0, 0), (0, 0, 1), (-1, 0, 0)];

/// One of vanilla's tree decorators.
#[derive(Clone, Debug, PartialEq)]
pub enum TreeDecorator {
    Beehive {
        probability: f32,
    },
    PlaceOnGround {
        tries: i32,
        radius: i32,
        height: i32,
        provider: StateProvider,
    },
    /// `attached_to_logs` — a mushroom stuck to the side of a fallen log.
    ///
    /// **Two draws per log whether or not anything is placed**, plus the
    /// shuffle: the direction is picked before the coin is flipped, so a
    /// single-entry `directions` list still costs its `nextInt(1)`.
    AttachedToLogs {
        probability: f32,
        provider: StateProvider,
        /// `Direction` offsets in the config's own order — the pick is
        /// `nextInt(len)` into this list, so the order is load-bearing.
        directions: Vec<Pos>,
    },
    /// `trunk_vine` — four coins per log, one per horizontal side, all four
    /// spent whether or not the sides are free.
    ///
    /// The decorator carries no config, so the four states are resolved once
    /// at parse time. The pairs are in the order vanilla spends the coins:
    /// west/`east=true`, east/`west=true`, north/`south=true`,
    /// south/`north=true` — the vine hangs on the face *towards* the log.
    TrunkVine {
        vines: Box<[(Pos, String); 4]>,
    },
    /// `leave_vine` — four coins per LEAF, one per horizontal side, all four
    /// spent whether or not the side is free, and each accepted side then
    /// hangs a vine **downwards through air up to four blocks further**.
    ///
    /// The same four `(step, state)` pairs as [`TreeDecorator::TrunkVine`] and
    /// in the same order — west/`east=true`, east/`west=true`,
    /// north/`south=true`, south/`north=true` — because the vine hangs on the
    /// face *towards* the leaf. What differs is the list it walks, the coin it
    /// spends (`nextFloat() < probability`, not `nextInt(3) > 0`) and the
    /// downward run, which is `LeaveVineDecorator#addHangingVine`.
    LeaveVine {
        probability: f32,
        vines: Box<[(Pos, String); 4]>,
    },
    /// `cocoa` — one coin for the WHOLE decorator, then four per log on the
    /// bottom three levels of the trunk.
    ///
    /// `Direction.Plane.HORIZONTAL` is `NORTH, EAST, SOUTH, WEST` — read off
    /// `Direction$Plane`'s static initialiser in the pinned jar, because the
    /// order is the draw order and a guess here is a different world. The
    /// cocoa goes on the side *opposite* the direction it faces, so the pairs
    /// below are `(step to the pod, its facing)`: north/`facing=north` sits at
    /// `z+1`.
    ///
    /// `states[d][age]`, because the age is `nextInt(3)` and is drawn only
    /// when the side turns out to be air — unlike the 0.25 coin above it,
    /// which is spent unconditionally.
    Cocoa {
        probability: f32,
        pods: Box<[(Pos, [String; 3]); 4]>,
    },
    /// `creaking_heart` — one block, in the middle of a pale oak's trunk.
    ///
    /// Two things about its draws are worth stating, because both are easy
    /// to get backwards and neither is visible in the blocks it writes.
    ///
    /// **The coin comes first and the shuffle only if it passes.** A tree
    /// whose `nextFloat()` fails spends exactly ONE value here; a tree whose
    /// coin passes then spends `logs - 1` more on `Util.shuffle`. So the cost
    /// of this decorator to everything after it swings by the size of the
    /// tree on a single coin.
    ///
    /// **The search is `findFirst` over the shuffled list, and it can find
    /// nothing.** The predicate wants a log with a log on all SIX faces —
    /// which on a 2x2 dark-oak trunk only holds well inside it — and finding
    /// nothing costs no further draws. That is why the shuffle is the whole
    /// randomness: which qualifying log gets the heart is decided entirely by
    /// where the shuffle put it.
    CreakingHeart {
        probability: f32,
        /// `CREAKING_HEART` with `creaking_heart_state=dormant` and
        /// `natural=true`, resolved at parse time.
        state: String,
    },
    /// `pale_moss` — the pale garden's whole reason for looking the way it
    /// does, and the only decorator that places another feature.
    ///
    /// `PaleMossDecorator#place` is four passes over one stream:
    ///
    /// 1. `Util.shuffledCopy(context.logs(), random)`, which is `logs - 1`
    ///    draws, then `return` if the copy is empty. **The shuffle happens
    ///    before the emptiness check**, so it is not skipped for a tree with
    ///    no logs; a tree with no logs also has nothing to shuffle, so the
    ///    two orders happen to agree, but only by arithmetic.
    /// 2. `Collections.min(shuffled, comparingInt(BlockPos::getY))`, then one
    ///    `nextFloat()` against `ground_probability`, and on success
    ///    [`Self::PaleMoss::patch`] is placed at `lowest.above()`.
    ///    **The shuffle decides which log this is.** `Collections.min` keeps
    ///    the first element attaining the minimum, and a pale oak has the dark
    ///    oak's two-by-two trunk, so four logs tie for lowest y every time.
    /// 3. One `nextFloat()` per log against `trunk_probability`, **spent
    ///    whether or not anything is placed**, and only then is the block
    ///    below tested for air.
    /// 4. The same again per leaf against `leaves_probability`.
    ///
    /// Passes 3 and 4 walk `context.logs()` and `context.leaves()`, **not the
    /// shuffled copy** — the shuffle is used for the minimum and then dropped.
    ///
    /// The hanger itself, `addMossHanger`, is a loop that draws only while
    /// there is air below it and writes its tip unconditionally:
    ///
    /// ```text
    ///   loop {
    ///       if !isAir(pos.below())        { break }
    ///       if nextFloat() < 0.5          { break }
    ///       setBlock(pos, moss[tip=false]);
    ///       pos = pos.below();
    ///   }
    ///   setBlock(pos, moss[tip=true]);
    /// ```
    ///
    /// The air test comes first, so a hanger with no room spends no draw at
    /// all and still writes one tip. The 0.5 is a literal `double` in the
    /// bytecode compared against a widened `nextFloat()`, not a config field.
    PaleMoss {
        leaves_probability: f32,
        trunk_probability: f32,
        ground_probability: f32,
        /// `VegetationFeatures.PALE_MOSS_PATCH`, resolved through the registry
        /// at parse time.
        ///
        /// A `ConfiguredFeature`, not a placed one: vanilla calls
        /// `ConfiguredFeature.place` directly, so there is no placement chain
        /// to run and no chain draws to spend.
        patch: Box<crate::feature::configured::ConfiguredFeature>,
        /// `PALE_HANGING_MOSS` with `tip=false` and with `tip=true`, in that
        /// order, resolved at parse time like `creaking_heart`'s state.
        hanging: Box<(String, String)>,
    },
    /// Parsed but not run. Carries its type name so a refusal says which.
    Unimplemented(String),
}

impl TreeDecorator {
    /// # Errors
    /// If a supported decorator is missing a field.
    pub fn parse(
        value: &serde_json::Value,
        states: &dyn DefaultStates,
        resolve_feature: &dyn Fn(
            &str,
        )
            -> Result<crate::feature::configured::ConfiguredFeature, String>,
    ) -> Result<Self, String> {
        let kind = value
            .get("type")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| format!("tree decorator has no `type`: {value}"))?;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the codec's fields are floats and they are compared \
                      against nextFloat()"
        )]
        let prob = |name: &str| -> Result<f32, String> {
            Ok(value
                .get(name)
                .and_then(serde_json::Value::as_f64)
                .ok_or_else(|| format!("pale_moss has no `{name}`"))? as f32)
        };
        let int = |name: &str, default: i32| -> i32 {
            value
                .get(name)
                .and_then(serde_json::Value::as_i64)
                .and_then(|v| i32::try_from(v).ok())
                .unwrap_or(default)
        };
        match kind {
            "minecraft:beehive" => Ok(Self::Beehive {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "the codec's field is a float and it is compared \
                              against nextFloat()"
                )]
                probability: value
                    .get("probability")
                    .and_then(serde_json::Value::as_f64)
                    .ok_or_else(|| "beehive has no `probability`".to_string())?
                    as f32,
            }),
            "minecraft:place_on_ground" => Ok(Self::PlaceOnGround {
                // `optionalFieldOf("tries", 128)`, and 128 tries is 384 draws.
                tries: int("tries", 128),
                radius: int("radius", 2),
                height: int("height", 1),
                provider: StateProvider::parse(
                    value
                        .get("block_state_provider")
                        .ok_or("place_on_ground has no `block_state_provider`")?,
                    states,
                )?,
            }),
            "minecraft:attached_to_logs" => Ok(Self::AttachedToLogs {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "the codec's field is a float and it is compared \
                              against nextFloat()"
                )]
                probability: value
                    .get("probability")
                    .and_then(serde_json::Value::as_f64)
                    .ok_or_else(|| "attached_to_logs has no `probability`".to_string())?
                    as f32,
                provider: StateProvider::parse(
                    value
                        .get("block_provider")
                        .ok_or("attached_to_logs has no `block_provider`")?,
                    states,
                )?,
                directions: value
                    .get("directions")
                    .and_then(serde_json::Value::as_array)
                    .ok_or_else(|| "attached_to_logs has no `directions`".to_string())?
                    .iter()
                    .map(|d| {
                        d.as_str()
                            .and_then(direction_offset)
                            .ok_or_else(|| format!("not a direction: {d}"))
                    })
                    .collect::<Result<Vec<Pos>, String>>()?,
            }),
            "minecraft:trunk_vine" => {
                // `Blocks.VINE.defaultBlockState().setValue(side, true)`, so
                // the other three sides and `up` stay at their defaults.
                let vine = |side: &str| -> Result<String, String> {
                    crate::feature::state_spec::canonical(
                        &serde_json::json!({
                            "Name": "minecraft:vine",
                            "Properties": { side: "true" },
                        }),
                        states,
                    )
                };
                Ok(Self::TrunkVine {
                    vines: Box::new([
                        ((-1, 0, 0), vine("east")?),
                        ((1, 0, 0), vine("west")?),
                        ((0, 0, -1), vine("south")?),
                        ((0, 0, 1), vine("north")?),
                    ]),
                })
            }
            "minecraft:cocoa" => {
                let pod = |facing: &str, age: i32| -> Result<String, String> {
                    crate::feature::state_spec::canonical(
                        &serde_json::json!({
                            "Name": "minecraft:cocoa",
                            "Properties": { "facing": facing, "age": age.to_string() },
                        }),
                        states,
                    )
                };
                let side = |facing: &str, step: Pos| -> Result<(Pos, [String; 3]), String> {
                    Ok((step, [pod(facing, 0)?, pod(facing, 1)?, pod(facing, 2)?]))
                };
                Ok(Self::Cocoa {
                    probability: value
                        .get("probability")
                        .and_then(serde_json::Value::as_f64)
                        .ok_or_else(|| "cocoa has no `probability`".to_string())?
                        as f32,
                    // NORTH, EAST, SOUTH, WEST — Direction.Plane.HORIZONTAL.
                    pods: Box::new([
                        side("north", (0, 0, 1))?,
                        side("east", (-1, 0, 0))?,
                        side("south", (0, 0, -1))?,
                        side("west", (1, 0, 0))?,
                    ]),
                })
            }
            "minecraft:leave_vine" => {
                // Identical state construction to `trunk_vine`:
                // `Blocks.VINE.defaultBlockState().setValue(side, true)`.
                let vine = |side: &str| -> Result<String, String> {
                    crate::feature::state_spec::canonical(
                        &serde_json::json!({
                            "Name": "minecraft:vine",
                            "Properties": { side: "true" },
                        }),
                        states,
                    )
                };
                Ok(Self::LeaveVine {
                    probability: value
                        .get("probability")
                        .and_then(serde_json::Value::as_f64)
                        .ok_or_else(|| "leave_vine has no `probability`".to_string())?
                        as f32,
                    vines: Box::new([
                        ((-1, 0, 0), vine("east")?),
                        ((1, 0, 0), vine("west")?),
                        ((0, 0, -1), vine("south")?),
                        ((0, 0, 1), vine("north")?),
                    ]),
                })
            }
            "minecraft:creaking_heart" => Ok(Self::CreakingHeart {
                probability: value
                    .get("probability")
                    .and_then(serde_json::Value::as_f64)
                    .ok_or_else(|| "creaking_heart has no `probability`".to_string())?
                    as f32,
                // `defaultBlockState().setValue(STATE, DORMANT).setValue(NATURAL, true)`
                // — the two non-default values are written out rather than
                // taken from the default state, because `natural` defaults to
                // false and a creaking heart that is not natural spawns
                // nothing.
                state: crate::feature::state_spec::canonical(
                    &serde_json::json!({
                        "Name": "minecraft:creaking_heart",
                        "Properties": {
                            "creaking_heart_state": "dormant",
                            "natural": "true",
                        },
                    }),
                    states,
                )?,
            }),
            "minecraft:pale_moss" => Ok(Self::PaleMoss {
                leaves_probability: prob("leaves_probability")?,
                trunk_probability: prob("trunk_probability")?,
                ground_probability: prob("ground_probability")?,
                // `VegetationFeatures.PALE_MOSS_PATCH`, which the decorator
                // hardcodes: its codec carries the three probabilities and
                // nothing else, so this name is not configurable.
                patch: Box::new(resolve_feature("minecraft:pale_moss_patch")?),
                // `HangingMossBlock.TIP`. The non-tip state is written down
                // the run and the tip closes it, so both are resolved once
                // here rather than string-built per block.
                hanging: Box::new((
                    crate::feature::state_spec::canonical(
                        &serde_json::json!({
                            "Name": "minecraft:pale_hanging_moss",
                            "Properties": { "tip": "false" },
                        }),
                        states,
                    )?,
                    crate::feature::state_spec::canonical(
                        &serde_json::json!({
                            "Name": "minecraft:pale_hanging_moss",
                            "Properties": { "tip": "true" },
                        }),
                        states,
                    )?,
                )),
            }),
            other => Ok(Self::Unimplemented(other.to_string())),
        }
    }

    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Beehive { .. } => "beehive",
            Self::PlaceOnGround { .. } => "place_on_ground",
            Self::AttachedToLogs { .. } => "attached_to_logs",
            Self::TrunkVine { .. } => "trunk_vine",
            Self::LeaveVine { .. } => "leave_vine",
            Self::Cocoa { .. } => "cocoa",
            Self::CreakingHeart { .. } => "creaking_heart",
            Self::PaleMoss { .. } => "pale_moss",
            Self::Unimplemented(name) => name,
        }
    }

    #[must_use]
    pub fn is_supported(&self) -> bool {
        match self {
            Self::Beehive { .. }
            | Self::TrunkVine { .. }
            | Self::LeaveVine { .. }
            | Self::Cocoa { .. }
            | Self::CreakingHeart { .. } => true,
            Self::PlaceOnGround { provider, .. } | Self::AttachedToLogs { provider, .. } => {
                provider.is_supported()
            }
            // The nested feature carries its own answer, and a pale oak is
            // placeable exactly when `pale_moss_patch` is: an unported patch
            // must refuse the whole tree rather than place three of its four
            // passes and leave the stream displaced.
            Self::PaleMoss { patch, .. } => patch.is_supported(),
            Self::Unimplemented(_) => false,
        }
    }
}

/// A `Direction` name to the step it takes. `Direction.CODEC` accepts all six.
fn direction_offset(name: &str) -> Option<Pos> {
    match name {
        "down" => Some((0, -1, 0)),
        "up" => Some((0, 1, 0)),
        "north" => Some((0, 0, -1)),
        "south" => Some((0, 0, 1)),
        "west" => Some((-1, 0, 0)),
        "east" => Some((1, 0, 0)),
        _ => None,
    }
}

/// `TreeDecorator.Context` — the three lists, sorted by y, stably.
pub struct Context {
    pub logs: Vec<Pos>,
    pub leaves: Vec<Pos>,
    pub roots: Vec<Pos>,
}

impl Context {
    /// Built from what the tree wrote. Each list is the set's own iteration
    /// order, then sorted by y with a **stable** sort.
    #[must_use]
    pub fn new(placed: &Placed) -> Self {
        let by_y = |set: &crate::java_hash_set::JavaHashSet<Pos>| -> Vec<Pos> {
            let mut out = set.to_vec();
            // `sort_by_key` is stable, which is the point.
            out.sort_by_key(|p| p.1);
            out
        };
        Self {
            logs: by_y(&placed.logs),
            leaves: by_y(&placed.foliage),
            roots: by_y(&placed.roots),
        }
    }

    /// `TreeFeature#getLowestTrunkOrRootOfTree`.
    ///
    /// Roots and logs, or one of them, depending on whether the lowest of
    /// each is at the same height — the case where a tree has roots that
    /// start level with its trunk.
    #[must_use]
    pub fn lowest_trunk_or_root(&self) -> Vec<Pos> {
        if self.roots.is_empty() {
            return self.logs.clone();
        }
        if !self.logs.is_empty() && self.roots[0].1 == self.logs[0].1 {
            let mut out = self.logs.clone();
            out.extend(self.roots.iter().copied());
            return out;
        }
        self.roots.clone()
    }
}

/// A feature resolver that refuses every name.
///
/// For callers that provably cannot meet a nested feature: a `fallen_tree`'s
/// decorator lists, and the unit tests in this crate that parse a decorator
/// on its own. Passing this rather than making the parameter optional keeps
/// the refusal a message that names the feature, instead of a `None` that has
/// to be re-explained at every use.
///
/// # Errors
/// Always.
pub fn no_registry(name: &str) -> Result<crate::feature::configured::ConfiguredFeature, String> {
    Err(format!(
        "`{name}` needs the configured-feature registry, which this caller does not have"
    ))
}

/// The world a decorator reads and writes.
pub trait DecoratorWorld {
    fn block_at(&mut self, x: i32, y: i32, z: i32) -> String;
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str);
    /// `BlockState#isAir`.
    fn is_air(&mut self, x: i32, y: i32, z: i32) -> Option<bool>;
    /// `BlockState#isSolidRender`.
    fn is_solid_render(&mut self, x: i32, y: i32, z: i32) -> Option<bool>;
    /// `getHeightmapPos(MOTION_BLOCKING_NO_LEAVES, pos).getY()`.
    fn motion_blocking_no_leaves(&mut self, x: i32, z: i32) -> i32;
}

/// Run one decorator.
///
/// # Errors
/// If the decorator is unported or the world cannot answer.
pub fn place(
    world: &mut (dyn crate::feature::configured::FeatureWorld + '_),
    tags: &BlockTags,
    decorator: &TreeDecorator,
    random: &mut dyn RandomSource,
    context: &Context,
    placed: &mut Placed,
) -> Result<(), NeedsRules> {
    match decorator {
        TreeDecorator::Unimplemented(name) => Err(NeedsRules(format!("{name} decorator"))),
        TreeDecorator::Beehive { probability } => {
            beehive(world, *probability, random, context, placed)
        }
        TreeDecorator::PlaceOnGround {
            tries,
            radius,
            height,
            provider,
        } => place_on_ground(
            world, tags, *tries, *radius, *height, provider, random, context, placed,
        ),
        TreeDecorator::AttachedToLogs {
            probability,
            provider,
            directions,
        } => attached_to_logs(
            world,
            tags,
            *probability,
            provider,
            directions,
            random,
            context,
            placed,
        ),
        TreeDecorator::TrunkVine { vines } => trunk_vine(world, vines, random, context, placed),
        TreeDecorator::LeaveVine { probability, vines } => {
            leave_vine(world, *probability, vines, random, context, placed)
        }
        TreeDecorator::Cocoa { probability, pods } => {
            cocoa(world, *probability, pods, random, context, placed)
        }
        TreeDecorator::CreakingHeart { probability, state } => {
            creaking_heart(world, tags, *probability, state, random, context, placed)
        }
        TreeDecorator::PaleMoss {
            leaves_probability,
            trunk_probability,
            ground_probability,
            patch,
            hanging,
        } => pale_moss(
            world,
            tags,
            *leaves_probability,
            *trunk_probability,
            *ground_probability,
            patch,
            hanging,
            random,
            context,
            placed,
        ),
    }
}

/// `PaleMossDecorator#place`, and the only decorator that places a feature.
///
/// The four passes are documented on [`TreeDecorator::PaleMoss`]. What is
/// worth repeating at the code is the two places the draw order is easy to
/// get backwards, because neither is visible in the blocks that come out:
///
/// * the coin for a log or leaf is spent **before** the block below it is
///   tested for air, so a tree in a hole spends exactly as many draws as one
///   in the open;
/// * `add_moss_hanger` tests for air **before** its coin, so a hanger with no
///   room spends none and still writes its tip.
#[allow(clippy::too_many_arguments, reason = "vanilla's own field list")]
fn pale_moss(
    world: &mut (dyn crate::feature::configured::FeatureWorld + '_),
    tags: &BlockTags,
    leaves_probability: f32,
    trunk_probability: f32,
    ground_probability: f32,
    patch: &crate::feature::configured::ConfiguredFeature,
    hanging: &(String, String),
    random: &mut dyn RandomSource,
    context: &Context,
    placed: &mut Placed,
) -> Result<(), NeedsRules> {
    // `Util.shuffledCopy(context.logs(), random)` — `logs - 1` draws, and it
    // happens before the emptiness check.
    let mut shuffled = context.logs.clone();
    shuffle(&mut shuffled, random);
    if shuffled.is_empty() {
        return Ok(());
    }
    // `Collections.min(shuffled, comparingInt(BlockPos::getY))`. Java keeps
    // the FIRST element attaining the minimum (`compareTo(..) < 0` is strict)
    // and so does Rust's `min_by_key`, which is what makes the shuffle above
    // decide the answer: a pale oak has a dark oak's two-by-two trunk, so four
    // logs tie for lowest y on every tree.
    let lowest = *shuffled
        .iter()
        .min_by_key(|p| p.1)
        .expect("the list is not empty");
    if random.next_f32() < ground_probability {
        // `ConfiguredFeature.place(level, generator, random, lowest.above())`.
        // Vanilla pops the boolean, so a patch that declines is not an error
        // and the tree carries on. An *unported* patch is a different thing
        // and must refuse: it would leave the stream short by whatever the
        // patch spends, and everything after it in the chunk displaced.
        patch
            .place(
                world,
                tags,
                &mut &mut *random,
                (lowest.0, lowest.1 + 1, lowest.2),
            )
            .map_err(|e| NeedsRules(e.0))?;
    }
    // `context.logs().forEach(..)` and `context.leaves().forEach(..)` — the
    // ORIGINAL lists, not the shuffled copy. The shuffle was for the minimum
    // and is dropped.
    for pass in [
        (&context.logs, trunk_probability),
        (&context.leaves, leaves_probability),
    ] {
        let (list, probability) = pass;
        for pos in list.clone() {
            // Spent whether or not anything is placed.
            if random.next_f32() >= probability {
                continue;
            }
            let below = (pos.0, pos.1 - 1, pos.2);
            if <dyn crate::feature::configured::FeatureWorld as DecoratorWorld>::is_air(
                world, below.0, below.1, below.2,
            ) != Some(true)
            {
                continue;
            }
            add_moss_hanger(world, below, hanging, random, placed);
        }
    }
    Ok(())
}

/// `PaleMossDecorator#addMossHanger`.
///
/// ```java
///   while (context.isAir(pos.below())) {
///       if (context.random().nextFloat() < 0.5F) break;
///       context.setBlock(pos, PALE_HANGING_MOSS.defaultBlockState()
///                                 .setValue(HangingMossBlock.TIP, false));
///       pos = pos.below();
///   }
///   context.setBlock(pos, PALE_HANGING_MOSS.defaultBlockState()
///                             .setValue(HangingMossBlock.TIP, true));
/// ```
///
/// The `0.5` is a literal `double` in the bytecode against a widened
/// `nextFloat()`, not a config field, and the comparison is
/// `nextFloat() >= 0.5` to continue. The tip is written unconditionally, so
/// this always places at least one block once it is called at all.
fn add_moss_hanger(
    world: &mut (dyn crate::feature::configured::FeatureWorld + '_),
    start: Pos,
    hanging: &(String, String),
    random: &mut dyn RandomSource,
    placed: &mut Placed,
) {
    let (body, tip) = hanging;
    let mut pos = start;
    loop {
        if <dyn crate::feature::configured::FeatureWorld as DecoratorWorld>::is_air(
            world,
            pos.0,
            pos.1 - 1,
            pos.2,
        ) != Some(true)
        {
            break;
        }
        if f64::from(random.next_f32()) < 0.5 {
            break;
        }
        <dyn crate::feature::configured::FeatureWorld as DecoratorWorld>::set_block(
            world, pos.0, pos.1, pos.2, body,
        );
        let _ = placed.decorations.insert(pos);
        pos = (pos.0, pos.1 - 1, pos.2);
    }
    <dyn crate::feature::configured::FeatureWorld as DecoratorWorld>::set_block(
        world, pos.0, pos.1, pos.2, tip,
    );
    let _ = placed.decorations.insert(pos);
}

/// `CreakingHeartDecorator#place`.
///
/// ```java
///   if (context.logs().isEmpty()) return;
///   if (random.nextFloat() >= this.probability) return;
///   List<BlockPos> list = new ArrayList<>(context.logs());
///   Util.shuffle(list, random);
///   list.stream().filter(pos -> everyFaceIsALog(context, pos)).findFirst()
///       .ifPresent(pos -> context.setBlock(pos, CREAKING_HEART ...));
/// ```
///
/// The coin is spent BEFORE the shuffle, so a tree that fails it costs one
/// draw and a tree that passes costs `logs - 1` more. Nothing after the
/// shuffle draws at all: `findFirst` over a shuffled list is where all the
/// randomness went, which is why the shuffle direction matters as much here
/// as it does in a jigsaw.
///
/// `everyFaceIsALog` asks about all six `Direction.values()` and each must be
/// in `#minecraft:logs` — a test only the inside of a 2x2 trunk passes, and
/// one that can find nothing at all.
#[allow(clippy::too_many_arguments, reason = "vanilla's own field list")]
fn creaking_heart<W: DecoratorWorld + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    probability: f32,
    state: &str,
    random: &mut dyn RandomSource,
    context: &Context,
    placed: &mut Placed,
) -> Result<(), NeedsRules> {
    if context.logs.is_empty() {
        return Ok(());
    }
    if random.next_f32() >= probability {
        return Ok(());
    }
    let mut logs = context.logs.clone();
    shuffle(&mut logs, random);
    // `Direction.values()` — DOWN, UP, NORTH, SOUTH, WEST, EAST.
    const FACES: [Pos; 6] = [
        (0, -1, 0),
        (0, 1, 0),
        (0, 0, -1),
        (0, 0, 1),
        (-1, 0, 0),
        (1, 0, 0),
    ];
    for log in logs {
        let mut buried = true;
        for face in FACES {
            let at = (log.0 + face.0, log.1 + face.1, log.2 + face.2);
            let block = world.block_at(at.0, at.1, at.2);
            let name = block.split_once('[').map_or(block.as_str(), |(n, _)| n);
            if !tags.contains("minecraft:logs", name) {
                buried = false;
                break;
            }
        }
        if buried {
            world.set_block(log.0, log.1, log.2, state);
            let _ = placed.decorations.insert(log);
            return Ok(());
        }
    }
    Ok(())
}

/// `AttachedToLogsDecorator#place`.
///
/// The logs are walked in a **shuffled copy** — `Util.shuffledCopy`, which is
/// `size - 1` draws before the first log is looked at — and each log then
/// spends a direction pick and a coin whatever the world says. Only the state
/// provider's draw is conditional.
#[allow(clippy::too_many_arguments, reason = "vanilla's own field list")]
fn attached_to_logs<W: DecoratorWorld + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    probability: f32,
    provider: &StateProvider,
    directions: &[Pos],
    random: &mut dyn RandomSource,
    context: &Context,
    placed: &mut Placed,
) -> Result<(), NeedsRules> {
    if directions.is_empty() {
        return Ok(());
    }
    let mut logs = context.logs.clone();
    shuffle(&mut logs, random);
    for log in logs {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_possible_wrap,
            clippy::cast_sign_loss,
            reason = "a direction list is at most the six faces"
        )]
        let index = random.next_i32_bound(directions.len() as i32) as usize;
        let step = directions[index];
        let target = (log.0 + step.0, log.1 + step.1, log.2 + step.2);
        // `nextFloat() <= probability`, and the read is short-circuited
        // behind it — but the read spends nothing, so only the provider's
        // draw is at stake.
        if random.next_f32() > probability {
            continue;
        }
        let air = world
            .is_air(target.0, target.1, target.2)
            .ok_or_else(|| NeedsRules("isAir".to_string()))?;
        if !air {
            continue;
        }
        let state = ProviderWorld(world)
            .state(provider, tags, random, target)
            .map_err(NeedsRules)?;
        world.set_block(target.0, target.1, target.2, &state);
        let _ = placed.decorations.insert(target);
    }
    Ok(())
}

/// `TrunkVineDecorator#place`.
///
/// Every log, in list order — **not** shuffled — and four `nextInt(3)` each,
/// all four spent whether or not the side is free.
fn trunk_vine<W: DecoratorWorld + ?Sized>(
    world: &mut W,
    vines: &[(Pos, String); 4],
    random: &mut dyn RandomSource,
    context: &Context,
    placed: &mut Placed,
) -> Result<(), NeedsRules> {
    for log in &context.logs {
        for (step, state) in vines {
            if random.next_i32_bound(3) <= 0 {
                continue;
            }
            let side = (log.0 + step.0, log.1 + step.1, log.2 + step.2);
            let air = world
                .is_air(side.0, side.1, side.2)
                .ok_or_else(|| NeedsRules("isAir".to_string()))?;
            if !air {
                continue;
            }
            world.set_block(side.0, side.1, side.2, state);
            let _ = placed.decorations.insert(side);
        }
    }
    Ok(())
}

/// `LeaveVineDecorator#place`, and `#addHangingVine` inlined into it.
///
/// Every leaf, in list order — **not** shuffled — and four `nextFloat()` each,
/// all four spent whether or not the side is free. The order is west, east,
/// north, south, and the property set is the side facing back at the leaf,
/// which is why the pairs are the same array `trunk_vine` walks.
///
/// The part that is not a trunk vine: an accepted side does not place one
/// block, it places a **run**. `addHangingVine` writes the vine, then walks
/// down while the block below is air, up to four more times — so a leaf over a
/// clearing can hang five vines from one coin. That run spends no randomness
/// at all, which is what makes it safe to inline: the draw sequence is exactly
/// four `nextFloat()` per leaf however much the world lets it write.
fn leave_vine<W: DecoratorWorld + ?Sized>(
    world: &mut W,
    probability: f32,
    vines: &[(Pos, String); 4],
    random: &mut dyn RandomSource,
    context: &Context,
    placed: &mut Placed,
) -> Result<(), NeedsRules> {
    for leaf in &context.leaves {
        for (step, state) in vines {
            // Unconditional, and before the world is asked anything.
            if random.next_f32() >= probability {
                continue;
            }
            let side = (leaf.0 + step.0, leaf.1 + step.1, leaf.2 + step.2);
            let air = world
                .is_air(side.0, side.1, side.2)
                .ok_or_else(|| NeedsRules("isAir".to_string()))?;
            if !air {
                continue;
            }
            world.set_block(side.0, side.1, side.2, state);
            let _ = placed.decorations.insert(side);
            // `addHangingVine`: `int i = 4; pos = pos.below();
            //  while (ctx.isAir(pos) && i > 0) { placeVine; pos = pos.below(); i--; }`
            let mut down = (side.0, side.1 - 1, side.2);
            let mut left = 4;
            while left > 0 {
                let air = world
                    .is_air(down.0, down.1, down.2)
                    .ok_or_else(|| NeedsRules("isAir".to_string()))?;
                if !air {
                    break;
                }
                world.set_block(down.0, down.1, down.2, state);
                let _ = placed.decorations.insert(down);
                down = (down.0, down.1 - 1, down.2);
                left -= 1;
            }
        }
    }
    Ok(())
}

/// `CocoaDecorator#place`.
///
/// One `nextFloat()` for the whole decorator — if it fails, the logs are never
/// walked and nothing else is drawn. Then the trunk is filtered to its bottom
/// three levels (`y - logs.first().y <= 2`, a filter and not a take, so it is
/// a height test rather than a count) and each surviving log spends four
/// coins, one per horizontal direction in `NORTH, EAST, SOUTH, WEST` order.
///
/// The 0.25 is a literal in the class and NOT the configured probability,
/// which is spent once at the top. Two different numbers that both look like
/// the decorator's chance, which is why the field and the literal are named
/// separately here.
///
/// The age is `nextInt(3)` and is drawn only when the side is air; the coin
/// above it is spent whatever the world says.
fn cocoa<W: DecoratorWorld + ?Sized>(
    world: &mut W,
    probability: f32,
    pods: &[(Pos, [String; 3]); 4],
    random: &mut dyn RandomSource,
    context: &Context,
    placed: &mut Placed,
) -> Result<(), NeedsRules> {
    if random.next_f32() >= probability {
        return Ok(());
    }
    let Some(first) = context.logs.first() else {
        return Ok(());
    };
    let base_y = first.1;
    for log in &context.logs {
        if log.1 - base_y > 2 {
            continue;
        }
        for (step, ages) in pods {
            // `nextFloat() <= 0.25f` — vanilla branches on `ifgt`, so the
            // boundary belongs to the placing side.
            if random.next_f32() > 0.25 {
                continue;
            }
            let side = (log.0 + step.0, log.1 + step.1, log.2 + step.2);
            let air = world
                .is_air(side.0, side.1, side.2)
                .ok_or_else(|| NeedsRules("isAir".to_string()))?;
            if !air {
                continue;
            }
            let age = random.next_i32_bound(3);
            let state = &ages[usize::try_from(age).unwrap_or(0).min(2)];
            world.set_block(side.0, side.1, side.2, state);
            let _ = placed.decorations.insert(side);
        }
    }
    Ok(())
}

/// `BeehiveDecorator#place`.
fn beehive<W: DecoratorWorld + ?Sized>(
    world: &mut W,
    probability: f32,
    random: &mut dyn RandomSource,
    context: &Context,
    placed: &mut Placed,
) -> Result<(), NeedsRules> {
    if context.logs.is_empty() {
        return Ok(());
    }
    if random.next_f32() >= probability {
        return Ok(());
    }
    // The height the nest hangs at. With leaves, just under the lowest of
    // them; without, a random step up the trunk.
    let y = if context.leaves.is_empty() {
        (context.logs[0].1 + 1 + random.next_i32_bound(3))
            .min(context.logs[context.logs.len() - 1].1)
    } else {
        (context.leaves[0].1 - 1).max(context.logs[0].1 + 1)
    };

    let mut candidates: Vec<Pos> = Vec::new();
    for log in &context.logs {
        if log.1 != y {
            continue;
        }
        for (dx, dy, dz) in SPAWN_DIRECTIONS {
            candidates.push((log.0 + dx, log.1 + dy, log.2 + dz));
        }
    }
    if candidates.is_empty() {
        return Ok(());
    }
    // `Util.shuffle` — the same downward Fisher-Yates the multiface feature
    // uses, and `n - 1` draws whether or not a spot is found afterwards.
    shuffle(&mut candidates, random);

    let mut chosen = None;
    for candidate in candidates {
        let here = world
            .is_air(candidate.0, candidate.1, candidate.2)
            .ok_or_else(|| NeedsRules("isAir".to_string()))?;
        let front = (
            candidate.0 + WORLDGEN_FACING.0,
            candidate.1 + WORLDGEN_FACING.1,
            candidate.2 + WORLDGEN_FACING.2,
        );
        let ahead = world
            .is_air(front.0, front.1, front.2)
            .ok_or_else(|| NeedsRules("isAir".to_string()))?;
        if here && ahead {
            chosen = Some(candidate);
            break;
        }
    }
    let Some(spot) = chosen else {
        return Ok(());
    };
    world.set_block(
        spot.0,
        spot.1,
        spot.2,
        "minecraft:bee_nest[facing=south,honey_level=0]",
    );
    let _ = placed.decorations.insert(spot);

    // The occupants. Their draws are on the shared stream and so are
    // behaviour; the block entity that would hold them is not written yet.
    let bees = 2 + random.next_i32_bound(2);
    for _ in 0..bees {
        let _ticks_in_hive = random.next_i32_bound(599);
    }
    Ok(())
}

/// `PlaceOnGroundDecorator#place`.
#[allow(clippy::too_many_arguments, reason = "vanilla's own field list")]
fn place_on_ground<W: DecoratorWorld + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    tries: i32,
    radius: i32,
    height: i32,
    provider: &StateProvider,
    random: &mut dyn RandomSource,
    context: &Context,
    placed: &mut Placed,
) -> Result<(), NeedsRules> {
    let lowest = context.lowest_trunk_or_root();
    let Some(first) = lowest.first().copied() else {
        return Ok(());
    };
    let y = first.1;
    let (mut min_x, mut max_x, mut min_z, mut max_z) = (first.0, first.0, first.2, first.2);
    for p in &lowest {
        if p.1 != y {
            continue;
        }
        min_x = min_x.min(p.0);
        max_x = max_x.max(p.0);
        min_z = min_z.min(p.2);
        max_z = max_z.max(p.2);
    }
    // `new BoundingBox(minX, y, minZ, maxX, y, maxZ).inflatedBy(r, h, r)`.
    let box_ = BoundingBox {
        min: (min_x - radius, y - height, min_z - radius),
        max: (max_x + radius, y + height, max_z + radius),
    };

    for _ in 0..tries {
        // Three draws, unconditionally, before a block is read.
        let x = between(random, box_.min.0, box_.max.0);
        let py = between(random, box_.min.1, box_.max.1);
        let z = between(random, box_.min.2, box_.max.2);
        attempt_above(world, tags, provider, random, (x, py, z), placed)?;
    }
    Ok(())
}

/// `PlaceOnGroundDecorator#attemptToPlaceBlockAbove`.
fn attempt_above<W: DecoratorWorld + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    provider: &StateProvider,
    random: &mut dyn RandomSource,
    pos: Pos,
    placed: &mut Placed,
) -> Result<(), NeedsRules> {
    let above = (pos.0, pos.1 + 1, pos.2);
    let air = world
        .is_air(above.0, above.1, above.2)
        .ok_or_else(|| NeedsRules("isAir".to_string()))?;
    let vine = {
        let state = world.block_at(above.0, above.1, above.2);
        state.split_once('[').map_or(state.as_str(), |(n, _)| n) == "minecraft:vine"
    };
    if !air && !vine {
        return Ok(());
    }
    let solid = world
        .is_solid_render(pos.0, pos.1, pos.2)
        .ok_or_else(|| NeedsRules("isSolidRender".to_string()))?;
    if !solid {
        return Ok(());
    }
    // Nothing may be standing over the spot: the surface here has to be the
    // block we are about to sit on.
    if world.motion_blocking_no_leaves(pos.0, pos.2) > above.1 {
        return Ok(());
    }
    let state = ProviderWorld(world)
        .state(provider, tags, random, above)
        .map_err(NeedsRules)?;
    world.set_block(above.0, above.1, above.2, &state);
    let _ = placed.decorations.insert(above);
    Ok(())
}

/// A `PredicateWorld` view of a decorator world, so the state providers can
/// be reused rather than restated.
struct ProviderWorld<'a, W: ?Sized>(&'a mut W);

impl<W: DecoratorWorld + ?Sized> crate::feature::block_predicate::PredicateWorld
    for ProviderWorld<'_, W>
{
    fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
        self.0.block_at(x, y, z)
    }
    fn air_at(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
        self.0.is_air(x, y, z)
    }
    fn solid_at(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
        self.0.is_solid_render(x, y, z)
    }
}

impl<W: DecoratorWorld + ?Sized> ProviderWorld<'_, W> {
    fn state(
        &mut self,
        provider: &StateProvider,
        tags: &BlockTags,
        random: &mut dyn RandomSource,
        pos: Pos,
    ) -> Result<String, String> {
        provider.state(self, tags, random, pos).map_err(|e| e.0)
    }
}

/// `RandomSource#nextIntBetweenInclusive` — `min + nextInt(max - min + 1)`.
fn between(random: &mut dyn RandomSource, min: i32, max: i32) -> i32 {
    min + random.next_i32_bound(max - min + 1)
}

/// `Util#shuffle` — downward Fisher-Yates, `size - 1` draws.
fn shuffle(list: &mut [Pos], random: &mut dyn RandomSource) {
    let mut i = list.len();
    while i > 1 {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_possible_wrap,
            clippy::cast_sign_loss,
            reason = "a candidate list is three per log at one height"
        )]
        let j = random.next_i32_bound(i as i32) as usize;
        list.swap(i - 1, j);
        i -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::legacy::LegacyRandomSource;
    use crate::rng::worldgen_random::WorldgenRandom;
    use std::collections::BTreeMap;

    struct Ground(BTreeMap<Pos, String>);

    impl DecoratorWorld for Ground {
        fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
            self.0
                .get(&(x, y, z))
                .cloned()
                .unwrap_or_else(|| "minecraft:air".to_string())
        }
        fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
            self.0.insert((x, y, z), state.to_string());
        }
        fn is_air(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
            Some(self.block_at(x, y, z) == "minecraft:air")
        }
        fn is_solid_render(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
            Some(self.block_at(x, y, z) == "minecraft:grass_block")
        }
        fn motion_blocking_no_leaves(&mut self, _x: i32, _z: i32) -> i32 {
            0
        }
    }

    /// A world for `pale_moss`, which needs a `FeatureWorld` rather than a
    /// `DecoratorWorld`. `PredicateWorld` + `TreeWorld` is enough to reach
    /// `DecoratorWorld` through the blanket impl, and `super::super::TreeOnly`
    /// supplies the twenty-two it must not touch.
    #[derive(Default)]
    struct Air(BTreeMap<Pos, String>);

    impl crate::feature::block_predicate::PredicateWorld for Air {
        fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
            self.0
                .get(&(x, y, z))
                .cloned()
                .unwrap_or_else(|| "minecraft:air".to_string())
        }
        fn air_at(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
            Some(
                crate::feature::block_predicate::PredicateWorld::block_at(self, x, y, z)
                    == "minecraft:air",
            )
        }
    }

    impl crate::feature::tree::TreeWorld for Air {
        fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
            self.0.insert((x, y, z), state.to_string());
        }
        fn min_y(&self) -> i32 {
            -64
        }
        fn max_y(&self) -> i32 {
            319
        }
        fn motion_blocking_no_leaves(&mut self, _x: i32, _z: i32) -> i32 {
            0
        }
    }

    /// Counts every draw, whatever its shape. `Util.shuffle` spends
    /// `next_i32_bound`, the probabilities spend `next_f32`, and a draw
    /// accounting that only watched one of them would miss the shuffle
    /// entirely.
    struct Counting<R> {
        inner: R,
        draws: usize,
    }

    impl<R: RandomSource> RandomSource for Counting<R> {
        fn set_seed(&mut self, seed: i64) {
            self.inner.set_seed(seed);
        }
        fn next_i32(&mut self) -> i32 {
            self.draws += 1;
            self.inner.next_i32()
        }
        fn next_i32_bound(&mut self, bound: i32) -> i32 {
            self.draws += 1;
            self.inner.next_i32_bound(bound)
        }
        fn next_i64(&mut self) -> i64 {
            self.draws += 1;
            self.inner.next_i64()
        }
        fn next_bool(&mut self) -> bool {
            self.draws += 1;
            self.inner.next_bool()
        }
        fn next_f32(&mut self) -> f32 {
            self.draws += 1;
            self.inner.next_f32()
        }
        fn next_f64(&mut self) -> f64 {
            self.draws += 1;
            self.inner.next_f64()
        }
        fn next_gaussian(&mut self) -> f64 {
            self.draws += 1;
            self.inner.next_gaussian()
        }
    }

    /// A float sequence, for the cases where a real stream leaves the branch
    /// under test unexercised. Panics rather than wrapping: running off the
    /// end means the draw accounting moved and the test is no longer testing
    /// what it says.
    struct Scripted(std::vec::IntoIter<f32>);

    impl RandomSource for Scripted {
        fn set_seed(&mut self, _seed: i64) {}
        fn next_i32(&mut self) -> i32 {
            unimplemented!("scripted rng: only next_f32")
        }
        fn next_i32_bound(&mut self, _bound: i32) -> i32 {
            unimplemented!("scripted rng: only next_f32")
        }
        fn next_i64(&mut self) -> i64 {
            unimplemented!("scripted rng: only next_f32")
        }
        fn next_bool(&mut self) -> bool {
            unimplemented!("scripted rng: only next_f32")
        }
        fn next_f32(&mut self) -> f32 {
            self.0.next().expect("the script ran out of draws")
        }
        fn next_f64(&mut self) -> f64 {
            unimplemented!("scripted rng: only next_f32")
        }
        fn next_gaussian(&mut self) -> f64 {
            unimplemented!("scripted rng: only next_f32")
        }
    }

    fn counting() -> Counting<WorldgenRandom<LegacyRandomSource>> {
        Counting {
            inner: WorldgenRandom::new(LegacyRandomSource::new(20_260_722)),
            draws: 0,
        }
    }

    /// `pale_moss` with the three probabilities set so nothing is placed.
    fn moss(ground: f32, trunk_p: f32, leaves_p: f32) -> TreeDecorator {
        TreeDecorator::PaleMoss {
            leaves_probability: leaves_p,
            trunk_probability: trunk_p,
            ground_probability: ground,
            // Never reached while `ground` is 0.0, and it costs nothing to
            // construct: `freeze_top_layer` has no configuration at all.
            patch: Box::new(crate::feature::configured::ConfiguredFeature::FreezeTopLayer),
            hanging: Box::new((
                "minecraft:pale_hanging_moss[tip=false]".to_string(),
                "minecraft:pale_hanging_moss[tip=true]".to_string(),
            )),
        }
    }

    /// A pale oak with five logs and three leaves, none of which can hang
    /// anything, costs `(5 - 1) + 1 + 5 + 3` draws: the shuffle, the ground
    /// coin, then one per log and one per leaf.
    #[test]
    fn pale_moss_spends_the_shuffle_a_ground_coin_and_one_per_position() {
        let mut placed = trunk();
        for z in 1..4 {
            placed.foliage.insert((0, 5, z)).expect("ok");
        }
        let context = Context::new(&placed);
        let mut world = Air::default();
        let mut random = counting();
        let mut written = Placed::default();
        place(
            &mut super::super::TreeOnly(&mut world),
            &tags(),
            &moss(0.0, 0.0, 0.0),
            &mut random,
            &context,
            &mut written,
        )
        .expect("ok");
        assert_eq!(random.draws, 4 + 1 + 5 + 3);
        assert!(world.0.is_empty(), "nothing is placed at probability zero");
    }

    /// The inversion that is easy to get backwards: the per-position coin is
    /// spent BEFORE the block below is tested for air, so a tree whose logs
    /// are all buried costs the stream exactly what one in the open costs.
    ///
    /// Same world, same context, `trunk_probability` 0.0 against 1.0, and the
    /// draw count must not move. Reading the air first would make the 1.0 case
    /// cheaper, and every feature after it in the chunk would land displaced.
    #[test]
    fn a_buried_log_still_spends_its_coin() {
        let placed = trunk();
        let context = Context::new(&placed);
        let counts: Vec<usize> = [0.0_f32, 1.0_f32]
            .into_iter()
            .map(|p| {
                let mut world = Air::default();
                // Solid under the lowest log, and the logs bury each other.
                crate::feature::tree::TreeWorld::set_block(&mut world, 0, -1, 0, "minecraft:stone");
                for y in 0..5 {
                    crate::feature::tree::TreeWorld::set_block(
                        &mut world,
                        0,
                        y,
                        0,
                        "minecraft:pale_oak_log",
                    );
                }
                let mut random = counting();
                let mut written = Placed::default();
                place(
                    &mut super::super::TreeOnly(&mut world),
                    &tags(),
                    &moss(0.0, p, 0.0),
                    &mut random,
                    &context,
                    &mut written,
                )
                .expect("ok");
                assert!(
                    !world
                        .0
                        .values()
                        .any(|b| b.starts_with("minecraft:pale_hanging_moss")),
                    "nothing can hang: every log has a block below it"
                );
                random.draws
            })
            .collect();
        assert_eq!(counts[0], counts[1], "the coin costs the same either way");
        assert_eq!(counts[0], 4 + 1 + 5);
    }

    /// `addMossHanger` writes `tip=false` down the run and closes it with a
    /// single `tip=true`, written AFTER the loop and unconditionally.
    ///
    /// Scripted, not seeded. The first version of this test used a real
    /// stream, and on that seed the loop body never ran: the hanger was one
    /// block long, so writing the tip inside the loop as well changed nothing
    /// and the test passed under the mutation it was written to catch. One
    /// log, no shuffle draw, then ground / trunk / three continues / one stop.
    #[test]
    fn a_hanger_is_a_contiguous_run_with_exactly_one_tip() {
        let mut placed = Placed::default();
        placed.logs.insert((0, 0, 0)).expect("ok");
        let context = Context::new(&placed);
        let mut world = Air::default();
        // ground coin (never passes, `< 0.0`), trunk coin (passes, `< 1.0`),
        // then `>= 0.5` three times to descend and `< 0.5` to stop.
        let mut random = Scripted(vec![0.9, 0.1, 0.9, 0.9, 0.9, 0.1].into_iter());
        let mut written = Placed::default();
        place(
            &mut super::super::TreeOnly(&mut world),
            &tags(),
            &moss(0.0, 1.0, 0.0),
            &mut random,
            &context,
            &mut written,
        )
        .expect("ok");

        let mut moss_ys: Vec<i32> = world
            .0
            .iter()
            .filter(|(_, b)| b.starts_with("minecraft:pale_hanging_moss"))
            .map(|(pos, _)| pos.1)
            .collect();
        moss_ys.sort_unstable();
        assert_eq!(
            moss_ys,
            vec![-4, -3, -2, -1],
            "three continues place three blocks and the stop places the tip"
        );
        let tips: Vec<i32> = world
            .0
            .iter()
            .filter(|(_, b)| b.contains("tip=true"))
            .map(|(pos, _)| pos.1)
            .collect();
        assert_eq!(tips, vec![-4], "exactly one tip, at the bottom of the run");
        // Every position it wrote is a decoration, which is what
        // `TreeDecorator$Context.setBlock` does through `decorationSetter`.
        assert_eq!(written.decorations.to_vec().len(), 4);
    }

    fn tags() -> BlockTags {
        BlockTags::from_entries(BTreeMap::new()).expect("tags")
    }

    fn trunk() -> Placed {
        let mut placed = Placed::default();
        for y in 0..5 {
            placed.logs.insert((0, y, 0)).expect("ok");
        }
        placed
    }

    /// A probability of zero costs exactly one draw — the `nextFloat` that
    /// rejects it — and nothing else.
    #[test]
    fn a_hive_that_does_not_roll_costs_one_draw() {
        let placed = trunk();
        let context = Context::new(&placed);
        let mut world = Ground(BTreeMap::new());
        let mut placed = trunk();
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(4));
        beehive(&mut world, 0.0, &mut random, &context, &mut placed).expect("ok");

        let mut probe = WorldgenRandom::new(LegacyRandomSource::new(4));
        probe.next_f32();
        assert_eq!(random.next_i64(), probe.next_i64());
        assert!(world.0.is_empty());
    }

    /// A hive that rolls places a south-facing nest and takes the occupant
    /// draws even though the block entity is not written.
    #[test]
    fn a_hive_that_rolls_places_a_nest_and_pays_for_its_bees() {
        let placed = trunk();
        let context = Context::new(&placed);
        let mut world = Ground(BTreeMap::new());
        let mut placed = trunk();
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(4));
        beehive(&mut world, 1.0, &mut random, &context, &mut placed).expect("ok");

        let nests: Vec<&String> = world
            .0
            .values()
            .filter(|v| v.starts_with("minecraft:bee_nest"))
            .collect();
        assert_eq!(nests.len(), 1, "{:?}", world.0);
        assert!(nests[0].contains("facing=south"), "{}", nests[0]);
        assert_eq!(placed.decorations.len(), 1);
    }

    /// 128 tries times three draws, whatever the ground is.
    #[test]
    fn place_on_ground_draws_three_times_per_try_on_bare_rock() {
        let placed = trunk();
        let context = Context::new(&placed);
        let mut world = Ground(BTreeMap::new());
        let mut placed = trunk();
        let provider = StateProvider::Simple("minecraft:short_grass".to_string());
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(6));
        place_on_ground(
            &mut world,
            &tags(),
            128,
            2,
            1,
            &provider,
            &mut random,
            &context,
            &mut placed,
        )
        .expect("ok");
        assert!(world.0.is_empty(), "nothing is solid, so nothing is placed");

        let mut probe = WorldgenRandom::new(LegacyRandomSource::new(6));
        for _ in 0..128 * 3 {
            probe.next_i32_bound(5);
        }
        assert_eq!(
            random.next_i64(),
            probe.next_i64(),
            "384 draws, and the bounds are the inflated box's spans"
        );
    }

    /// The lowest-trunk rule: with no roots it is the logs; with roots that
    /// start level with the trunk it is both.
    #[test]
    fn the_lowest_trunk_rule_picks_the_right_list() {
        let placed = trunk();
        assert_eq!(Context::new(&placed).lowest_trunk_or_root().len(), 5);

        let mut with_roots = trunk();
        with_roots.roots.insert((1, 0, 0)).expect("ok");
        let context = Context::new(&with_roots);
        assert_eq!(
            context.lowest_trunk_or_root().len(),
            6,
            "a root level with the trunk contributes both lists"
        );

        let mut deep_roots = trunk();
        deep_roots.roots.insert((1, -3, 0)).expect("ok");
        let context = Context::new(&deep_roots);
        assert_eq!(
            context.lowest_trunk_or_root().len(),
            1,
            "a root below the trunk contributes only the roots"
        );
    }

    #[test]
    fn an_unported_decorator_is_refused() {
        // Was `minecraft:cocoa` until cocoa was ported. `alter_ground` is the
        // replacement because it is genuinely unported and is refused on a
        // real window — 150 times in the r=7 square at (-1152,864), on the
        // giant-trunk spruce of an old-growth taiga.
        let decorator = TreeDecorator::parse(
            &serde_json::from_str(r#"{"type":"minecraft:alter_ground"}"#).expect("json"),
            &crate::feature::state_spec::NoDefaults,
            &no_registry,
        )
        .expect("parse");
        assert!(!decorator.is_supported());
        assert_eq!(decorator.name(), "minecraft:alter_ground");
    }

    #[test]
    fn cocoa_resolves_twelve_states_and_is_supported() {
        // Four facings by three ages, all resolved at parse time so the place
        // path never touches the registry. `canonical` needs the block's
        // default state even when every property is written explicitly — it
        // is filling the ones the spec omits — so a one-block stub stands in
        // for the extract rather than `NoDefaults`.
        struct One;
        impl crate::feature::state_spec::DefaultStates for One {
            fn default_state(&self, block: &str) -> Option<&str> {
                (block == "minecraft:cocoa").then_some("minecraft:cocoa[age=0,facing=north]")
            }
        }
        let decorator = TreeDecorator::parse(
            &serde_json::from_str(r#"{"type":"minecraft:cocoa","probability":0.2}"#).expect("json"),
            &One,
            &no_registry,
        )
        .expect("parse");
        assert!(decorator.is_supported());
        assert_eq!(decorator.name(), "cocoa");
        let TreeDecorator::Cocoa { pods, .. } = &decorator else {
            panic!("not a cocoa");
        };
        // NORTH, EAST, SOUTH, WEST, and the pod sits OPPOSITE the way it
        // faces — `Direction$Plane`'s static initialiser in the pinned jar.
        assert_eq!(pods[0].0, (0, 0, 1));
        assert_eq!(pods[1].0, (-1, 0, 0));
        assert_eq!(pods[2].0, (0, 0, -1));
        assert_eq!(pods[3].0, (1, 0, 0));
        assert!(pods[0].1[0].contains("facing=north"), "{}", pods[0].1[0]);
        assert!(pods[0].1[2].contains("age=2"), "{}", pods[0].1[2]);
    }
}

#[cfg(test)]
mod creaking_heart_tests {
    use super::*;
    use crate::rng::draws;
    use crate::rng::legacy::LegacyRandomSource;
    use crate::rng::worldgen_random::WorldgenRandom;
    use std::collections::BTreeMap;

    struct Logs(BTreeMap<Pos, String>);

    impl DecoratorWorld for Logs {
        fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
            self.0
                .get(&(x, y, z))
                .cloned()
                .unwrap_or_else(|| "minecraft:air".to_string())
        }
        fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
            self.0.insert((x, y, z), state.to_string());
        }
        fn is_air(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
            Some(self.block_at(x, y, z) == "minecraft:air")
        }
        fn is_solid_render(&mut self, _x: i32, _y: i32, _z: i32) -> Option<bool> {
            Some(true)
        }
        fn motion_blocking_no_leaves(&mut self, _x: i32, _z: i32) -> i32 {
            0
        }
    }

    fn log_tags() -> BlockTags {
        BlockTags::from_entries(BTreeMap::from([(
            "minecraft:logs".to_string(),
            vec!["minecraft:pale_oak_log".to_string()],
        )]))
        .expect("tags")
    }

    /// A solid 3x3x4 block of logs. Only its interior column has a log on all
    /// six faces, and that is the point: a bare 2x2 dark-oak trunk has **no**
    /// qualifying position at all, because its outer faces are air. The
    /// decorator only ever fires where the stub ring has filled a face in —
    /// which is why "all six neighbours are logs" is a much narrower test than
    /// it reads, and why it must not be relaxed to four or five.
    fn trunk() -> (Placed, Logs) {
        let mut placed = Placed::default();
        let mut world = Logs(BTreeMap::new());
        for y in 0..4 {
            for x in -1..2 {
                for z in -1..2 {
                    placed.logs.insert((x, y, z)).expect("ok");
                    world
                        .0
                        .insert((x, y, z), "minecraft:pale_oak_log[axis=y]".to_string());
                }
            }
        }
        (placed, world)
    }

    const HEART: &str =
        "minecraft:creaking_heart[axis=y,creaking_heart_state=dormant,natural=true]";

    /// The coin comes FIRST and the shuffle only if it passes, so a tree that
    /// fails costs exactly one draw whatever its size.
    #[test]
    fn a_failed_coin_costs_one_draw_and_no_shuffle() {
        let (placed, mut world) = trunk();
        let context = Context::new(&placed);
        let mut out = Placed::default();
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(4));
        draws::arm();
        creaking_heart(
            &mut world,
            &log_tags(),
            0.0,
            HEART,
            &mut random,
            &context,
            &mut out,
        )
        .expect("ok");
        assert_eq!(draws::take(), 1, "the coin, and nothing else");
        assert!(!world
            .0
            .values()
            .any(|v| v.starts_with("minecraft:creaking_heart")));
    }

    /// A coin that passes then pays for the shuffle: `logs - 1` draws on a
    /// 32-log trunk, so 32 in total.
    #[test]
    fn a_passed_coin_pays_for_the_shuffle_and_places_exactly_one_heart() {
        let (placed, mut world) = trunk();
        let context = Context::new(&placed);
        assert_eq!(context.logs.len(), 36);
        let mut out = Placed::default();
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(4));
        draws::arm();
        creaking_heart(
            &mut world,
            &log_tags(),
            1.0,
            HEART,
            &mut random,
            &context,
            &mut out,
        )
        .expect("ok");
        assert_eq!(
            draws::take(),
            36,
            "one coin plus `logs - 1` for the shuffle"
        );
        let hearts: Vec<&Pos> = world
            .0
            .iter()
            .filter(|(_, v)| v.starts_with("minecraft:creaking_heart"))
            .map(|(k, _)| k)
            .collect();
        assert_eq!(hearts.len(), 1, "findFirst, not forEach");
        // The only position with a log on all six faces is (0, y, 0) with
        // y strictly inside the block — the faces of every other column, and
        // of the top and bottom layers, are air.
        assert_eq!((hearts[0].0, hearts[0].2), (0, 0), "{hearts:?}");
        assert!((1..3).contains(&hearts[0].1), "{hearts:?}");
        assert_eq!(out.decorations.len(), 1);
    }

    /// A trunk one block wide has no position with six log faces, so the
    /// search finds nothing — and finding nothing costs no extra draw.
    #[test]
    fn a_search_that_finds_nothing_still_paid_for_its_shuffle() {
        let mut placed = Placed::default();
        let mut world = Logs(BTreeMap::new());
        for y in 0..5 {
            placed.logs.insert((0, y, 0)).expect("ok");
            world
                .0
                .insert((0, y, 0), "minecraft:pale_oak_log[axis=y]".to_string());
        }
        let context = Context::new(&placed);
        let mut out = Placed::default();
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(9));
        draws::arm();
        creaking_heart(
            &mut world,
            &log_tags(),
            1.0,
            HEART,
            &mut random,
            &context,
            &mut out,
        )
        .expect("ok");
        assert_eq!(draws::take(), 5, "coin plus four shuffle draws");
        assert!(!world
            .0
            .values()
            .any(|v| v.starts_with("minecraft:creaking_heart")));
    }

    /// An empty log list returns before the coin — zero draws, not one.
    #[test]
    fn an_empty_log_list_does_not_even_flip_the_coin() {
        let placed = Placed::default();
        let context = Context::new(&placed);
        let mut out = Placed::default();
        let mut world = Logs(BTreeMap::new());
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(9));
        draws::arm();
        creaking_heart(
            &mut world,
            &log_tags(),
            1.0,
            HEART,
            &mut random,
            &context,
            &mut out,
        )
        .expect("ok");
        assert_eq!(draws::take(), 0);
    }
}
