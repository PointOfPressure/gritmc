//! `SinglePoolElement.place` — turning an assembled jigsaw piece into blocks.
//!
//! Reference (26.2 bytecode, read before porting — CLAUDE.md rule 4):
//! `SinglePoolElement.getSettings`/`place`, `LegacySinglePoolElement.getSettings`,
//! `StructureTemplate.processBlockInfos`/`placeInWorld`, `RuleProcessor`,
//! `ProcessorRule`, `BlockIgnoreProcessor`, `JigsawReplacementProcessor`,
//! `GravityProcessor`, `StructureTemplatePool$Projection`.
//!
//! # The processor list is built in one place and its order is behaviour
//!
//! `SinglePoolElement.getSettings` appends, in this order:
//!
//! ```text
//!   BlockIgnoreProcessor.STRUCTURE_BLOCK
//!   JigsawReplacementProcessor.INSTANCE        (unless keepJigsaws)
//!   every processor of the element's own list
//!   every processor of the projection           (RIGID: none;
//!                                                TERRAIN_MATCHING: gravity)
//! ```
//!
//! and `LegacySinglePoolElement.getSettings` then calls **`popProcessor`** on
//! `STRUCTURE_BLOCK` and **appends `STRUCTURE_AND_AIR`**. Two consequences,
//! both measured off the bytecode rather than inferred:
//!
//! * **A legacy element does NOT write its air.** `STRUCTURE_AND_AIR` ignores
//!   `minecraft:air` as well as `minecraft:structure_block`, and *every*
//!   village element is `legacy_single_pool_element`. A note in this repo said
//!   the opposite — that legacy meant `STRUCTURE_BLOCK` only and a village's
//!   air was written — and it is backwards: it is the plain
//!   `single_pool_element` that keeps its air. Writing a village's air would
//!   hollow out the terrain in the shape of every building.
//! * **The ignore runs LAST for a legacy element**, not first. That is not
//!   cosmetic either: `zombie_savanna` rewrites doors and torches to
//!   `minecraft:air`, and because the ignore is downstream of the rule
//!   processor those rewritten blocks are then *dropped*. A zombie village's
//!   doorway keeps whatever the terrain already had; it does not become air.
//!
//! # What a processor sees
//!
//! `processBlockInfos` hands each processor a `StructureBlockInfo` whose
//! position is the **world** position (`calculateRelativePosition` then
//! `.offset(origin)`) but whose state is the **unrotated** one — the rotation
//! is applied in `placeInWorld`, after the whole processor chain has run. So a
//! rule's `input_predicate` matches the template's own state and its
//! `output_state` is rotated on the way out.
//!
//! `RuleProcessor` seeds one `LegacyRandomSource` per block from
//! `Mth.getSeed(worldPos)` and walks its rules in order, so a
//! `random_block_match` that fails its block test spends nothing and one that
//! passes spends exactly one `nextFloat`. None of this touches the decoration
//! feature stream.

use serde_json::Value;

use crate::rng::legacy::LegacyRandomSource;
use crate::rng::RandomSource;
use crate::structure::pieces::BoundingBox;
use crate::structure::template::{
    mirror_state, position_seed, rotate_state, transform, Mirror, Rotation,
};
use crate::structure::template::{Malformed, Template};
use crate::structure::template_pool::Projection;

/// What a piece needs of the world while it writes.
///
/// # Why the recorder is a supertrait and not an optional method
///
/// `placeInWorld` is where a container's `LootTable` and the `LootTableSeed`
/// drawn beside it both come into being, so a piece world that cannot record is
/// a piece world whose containers can never be reproduced. `docs/DETERMINISM.md`
/// §42 made [`crate::block_entity::BlockEntityRecorder`] a **required** method
/// on the traits whose producers record, for the reason that a defaulted no-op
/// is indistinguishable from a producer that has nothing to say — and absence
/// already means "not yet reproduced" everywhere else in this project. A
/// bound here forces all eight implementors to answer the question out loud.
pub trait PieceWorld: crate::block_entity::BlockEntityRecorder {
    /// The full block state at a world position.
    fn block_at(&mut self, x: i32, y: i32, z: i32) -> String;

    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str);

    /// `Heightmap.Types.WORLD_SURFACE_WG` — the noise-stage fossil the
    /// `GravityProcessor` re-projects a terrain-matching piece onto.
    ///
    /// `GravityProcessor.processBlock` swaps `WORLD_SURFACE_WG` for the live
    /// `WORLD_SURFACE` only when the level `instanceof ServerLevel`, which a
    /// `WorldGenRegion` is not. So during generation it is the `_WG` map, and
    /// a port that reached for the live one would move every street.
    fn world_surface_wg(&mut self, x: i32, z: i32) -> i32;

    /// `BlockState.is(TagKey)` for the `tag_match` rule test.
    fn block_in_tag(&self, tag: &str, block: &str) -> bool;

    /// `Block.defaultBlockState()`, spelled canonically.
    ///
    /// Needed by the jigsaw replacement, which resolves a `final_state`
    /// string through `BlockStateParser.parseForBlock` — vanilla starts from
    /// the block's default state and applies only the named properties.
    ///
    /// `None` is "this world carries no block-property table", and the
    /// replacement then writes the `final_state` string verbatim, which is
    /// what every caller did before 2026-08-06. **That fallback is today's
    /// bug, kept only so a toy piece world in a unit test need not carry the
    /// extract.** Every production `PieceWorld` in `worldgen-assembly`
    /// answers this, and
    /// `the_production_piece_worlds_resolve_default_states` is the gate that
    /// keeps it that way.
    fn default_state(&self, _block: &str) -> Option<String> {
        None
    }

    /// `BlockState.isFaceSturdy(level, pos, direction)` with
    /// `SupportType.FULL`, `direction` being the `Direction` ordinal.
    ///
    /// `None` is "this world cannot answer", and the shape pass then treats
    /// the face as not sturdy. That is only ever asked of a caller that has
    /// opted into [`PlaceSettings::with_shape_update`]; the default exists so
    /// a piece world that never does need not carry a block-property table.
    fn face_is_sturdy(&self, _state: &str, _direction: usize) -> Option<bool> {
        None
    }

    /// `Block.isFaceFull(getCollisionShape(..), direction)`, for the same
    /// callers and with the same `None` meaning.
    fn face_is_collision_full(&self, _state: &str, _direction: usize) -> Option<bool> {
        None
    }

    /// Whether this state's shape is the full cube —
    /// `Block.isShapeFullBlock(state.getShape(level, pos))`, which is what
    /// `LavaSubmergedBlockProcessor` asks before it drowns a block.
    ///
    /// `None` is "this world cannot answer", and the processor then leaves the
    /// block alone — the conservative direction, since not answering must not
    /// invent a lava block. Only a caller that has put a
    /// [`Processor::LavaSubmerged`] in its chain ever needs it.
    fn shape_is_full_block(&self, _state: &str) -> Option<bool> {
        None
    }

    /// `BlockState.isSolid()` — what `WallBannerBlock.canSurvive` asks of the
    /// block its banner hangs on. `None` is "cannot answer", and the banner
    /// is then left in place.
    fn is_solid(&self, _state: &str) -> Option<bool> {
        None
    }

    /// `level.getSeed()`, which `CappedProcessor.finalizeProcessing` forks
    /// its positional random from. `None` and a capped processor changes
    /// nothing — only a caller that has one in its chain ever asks.
    fn world_seed(&self) -> Option<i64> {
        None
    }
}

/// A `PieceWorld` seen as the two fact sources the shape pass wants.
struct WorldFacts<'w, W: PieceWorld + ?Sized>(&'w W);

impl<W: PieceWorld + ?Sized> crate::stage::post_process::TagFacts for WorldFacts<'_, W> {
    fn contains(&self, tag: &str, block: &str) -> bool {
        self.0.block_in_tag(tag, block)
    }
}

impl<W: PieceWorld + ?Sized> crate::stage::post_process::ShapeFacts for WorldFacts<'_, W> {
    fn face_is_sturdy(&self, state: &str, direction: usize) -> Option<bool> {
        self.0.face_is_sturdy(state, direction)
    }
    fn face_is_collision_full(&self, state: &str, direction: usize) -> Option<bool> {
        self.0.face_is_collision_full(state, direction)
    }
    fn is_solid(&self, state: &str) -> Option<bool> {
        self.0.is_solid(state)
    }
}

/// `RuleTest` — the six subtypes a village can reach.
#[derive(Clone, Debug, PartialEq)]
pub enum RuleTest {
    /// `AlwaysTrueTest` — spends nothing.
    AlwaysTrue,
    /// `BlockMatchTest` — `state.is(block)`, so **properties are ignored**.
    BlockMatch(String),
    /// `BlockStateMatchTest` — the whole state, properties included.
    BlockStateMatch(String),
    /// `RandomBlockMatchTest`. The block test short-circuits the draw: a
    /// `nextFloat` is spent only when the block already matches.
    RandomBlockMatch { block: String, probability: f32 },
    /// `RandomBlockStateMatchTest`, same short circuit.
    RandomBlockStateMatch { state: String, probability: f32 },
    /// `TagMatchTest`.
    TagMatch(String),
}

/// The block name out of a full state string.
fn block_of(state: &str) -> &str {
    state.split('[').next().unwrap_or(state)
}

/// A `Name`/`Properties` block state, as a processor's `output_state` writes
/// it, rendered into this project's canonical `name[a=b,c=d]` spelling.
///
/// Properties are emitted in the JSON object's own order. `serde_json` keeps
/// insertion order only with the `preserve_order` feature, and without it a
/// `Map` is a `BTreeMap` — alphabetical, which is also how this project spells
/// every other state. Both village outputs that carry properties have a single
/// one, so the two orders cannot differ here.
fn output_state(value: &Value) -> Result<String, Malformed> {
    let name = value
        .get("Name")
        .and_then(Value::as_str)
        .ok_or_else(|| Malformed("an output_state has no `Name`".to_string()))?;
    let Some(props) = value.get("Properties").and_then(Value::as_object) else {
        return Ok(name.to_string());
    };
    if props.is_empty() {
        return Ok(name.to_string());
    }
    let body: Vec<String> = props
        .iter()
        .map(|(k, v)| {
            let text = match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            format!("{k}={text}")
        })
        .collect();
    Ok(format!("{name}[{}]", body.join(",")))
}

/// `Direction.Axis`, for the one position predicate that reads one.
///
/// Not `Direction`: `AxisAlignedLinearPosTest` stores an `Axis` and rebuilds
/// the direction with `Direction.get(AxisDirection.POSITIVE, axis)` every
/// call, so the sign is a constant of the class and never a field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
    Z,
}

/// `PosRuleTest` — a `ProcessorRule`'s **third** predicate, and the only one
/// of the three that reads a position rather than a block.
///
/// # Why this exists at all, and what it is worth
///
/// Forty `processor_list` documents in 26.2 carry exactly **one**
/// `position_predicate` between them, in `high_rampart.json`, and it is an
/// `axis_aligned_linear_pos`. `Rule::parse` used to refuse any rule carrying
/// one, and a refused rule refuses its whole list — so that single field made
/// every bastion pool element that references `high_rampart` unplaceable.
///
/// `minecraft:linear_pos` is the third registered type and is **deliberately
/// not ported**. Nothing in the jar reaches it: the same walk that finds the
/// one `axis_aligned_linear_pos` finds zero of it, and `Processor::BlockAge`
/// is this repo's own standing lesson about implementing a thing the data
/// cannot construct. It is refused by name, so the refusal says which type
/// rather than "a position predicate".
#[derive(Clone, Debug, PartialEq)]
pub enum PosRuleTest {
    /// `PosAlwaysTrueTest.INSTANCE` — the codec's default when the field is
    /// absent, and it **spends no draw**. That is the whole reason the
    /// default is a variant here rather than an `Option` short-circuit: a
    /// rule without a position predicate must reach `nextFloat` exactly as
    /// often as it did before this type existed.
    AlwaysTrue,
    /// `AxisAlignedLinearPosTest`.
    ///
    /// Every field is optional in the codec. `min_chance` and `max_chance`
    /// default to **0.0**, `min_dist` and `max_dist` to **0**, and `axis` to
    /// **`Axis.Y`** — so an empty object is a predicate that never passes,
    /// and `high_rampart.json`, which names only `max_chance` and `max_dist`,
    /// is a Y-axis test from 0.
    AxisAlignedLinear {
        min_chance: f32,
        max_chance: f32,
        min_dist: i32,
        max_dist: i32,
        axis: Axis,
    },
}

impl PosRuleTest {
    /// # Errors
    /// If the predicate names a `predicate_type` this port does not have, or
    /// if the range is one the vanilla constructor throws on.
    pub fn parse(value: &Value) -> Result<Self, Malformed> {
        let kind = value
            .get("predicate_type")
            .and_then(Value::as_str)
            .ok_or_else(|| Malformed("a position predicate has no `predicate_type`".to_string()))?;
        Ok(match kind {
            "minecraft:always_true" => Self::AlwaysTrue,
            "minecraft:axis_aligned_linear_pos" => {
                // `Codec.FLOAT`, so an f32 all the way down — the draw it is
                // compared against is an f32 too.
                let chance = |key: &str| -> f32 {
                    value.get(key).and_then(Value::as_f64).unwrap_or(0.0) as f32
                };
                let dist = |key: &str| -> i32 {
                    value
                        .get(key)
                        .and_then(Value::as_i64)
                        .and_then(|v| i32::try_from(v).ok())
                        .unwrap_or(0)
                };
                let axis = match value.get("axis").and_then(Value::as_str) {
                    None | Some("y") => Axis::Y,
                    Some("x") => Axis::X,
                    Some("z") => Axis::Z,
                    Some(other) => {
                        return Err(Malformed(format!(
                            "an axis_aligned_linear_pos names axis `{other}`"
                        )))
                    }
                };
                let (min_dist, max_dist) = (dist("min_dist"), dist("max_dist"));
                // The constructor's own guard: `if (minDist >= maxDist) throw
                // new IllegalArgumentException`. Reproduced as a refusal
                // rather than dropped, because the default pair is `0, 0`,
                // which *is* the throwing case — so an object that names no
                // distance at all is malformed in vanilla too, and a port
                // that quietly divided by zero here would answer `NaN` and
                // then `false`, which looks exactly like a predicate that
                // simply never fires.
                if min_dist >= max_dist {
                    return Err(Malformed(format!(
                        "an axis_aligned_linear_pos has an invalid range: [{min_dist},{max_dist}]"
                    )));
                }
                Self::AxisAlignedLinear {
                    min_chance: chance("min_chance"),
                    max_chance: chance("max_chance"),
                    min_dist,
                    max_dist,
                    axis,
                }
            }
            other => return Err(Malformed(format!("unported position predicate `{other}`"))),
        })
    }

    /// `PosRuleTest.test(inTemplatePos, worldPos, worldReference, random)`.
    ///
    /// `inTemplatePos` is not a parameter here because **no registered type
    /// reads it**: `PosAlwaysTrueTest` ignores all four arguments,
    /// `LinearPosTest` uses `worldPos.distManhattan(worldReference)` and
    /// `AxisAlignedLinearPosTest` uses the two world positions. It is named
    /// so that a future type which does read it is a signature change and not
    /// a silent wrong answer.
    ///
    /// Public because vanilla's is, and because a predicate that is correct
    /// and never called is exactly the failure a private one invites: the
    /// golden test drives this entry point rather than a re-derivation of it.
    pub fn test<R: RandomSource + ?Sized>(
        &self,
        world_pos: (i32, i32, i32),
        reference: (i32, i32, i32),
        random: &mut R,
    ) -> bool {
        match self {
            Self::AlwaysTrue => true,
            Self::AxisAlignedLinear {
                min_chance,
                max_chance,
                min_dist,
                max_dist,
                axis,
            } => {
                // `Direction.get(POSITIVE, axis)` has step 1 on the named
                // axis and 0 on the other two, so two of the three terms are
                // exactly zero. Written out as three terms anyway, because
                // collapsing it to one absolute delta is only sound for a
                // unit step vector and hides which fact is being relied on.
                let (sx, sy, sz) = match axis {
                    Axis::X => (1, 0, 0),
                    Axis::Y => (0, 1, 0),
                    Axis::Z => (0, 0, 1),
                };
                let xd = ((world_pos.0 - reference.0) * sx) as f32;
                let yd = ((world_pos.1 - reference.1) * sy) as f32;
                let zd = ((world_pos.2 - reference.2) * sz) as f32;
                // `(int)(xd + yd + zd)` after each has been through
                // `Math.abs`. The sum is taken in f32 and truncated toward
                // zero, which is what a Java `(int)` cast of a float does.
                #[allow(clippy::cast_possible_truncation)]
                let distance = (xd.abs() + yd.abs() + zd.abs()) as i32;
                // **The draw is taken here and unconditionally**, after the
                // input and location predicates have already passed —
                // `ProcessorRule.test` is a chain of `&&`, so a rule whose
                // input predicate fails never reaches this line and spends
                // nothing.
                let rnd = random.next_f32();
                // `Mth.clampedLerp(Mth.inverseLerp(dist, minDist, maxDist),
                // minChance, maxChance)`. **The factor is the FIRST
                // argument**: 26.2's signature is `clampedLerp(float factor,
                // float min, float max)`. Prose in this repo transcribed it
                // as `clampedLerp(minChance, maxChance, factor)`, which is
                // the argument order of an older `Mth` and, read literally
                // against this signature, interpolates between `maxChance`
                // and the factor.
                let factor = inverse_lerp(distance as f32, *min_dist as f32, *max_dist as f32);
                // `<=`, not `<`. Every `RuleTest` in this file compares its
                // draw with `<`; this one does not, and at `min_chance = 0`
                // that difference is the whole behaviour of the predicate at
                // distance `min_dist` — `nextFloat()` can return 0.0.
                rnd <= clamped_lerp(factor, *min_chance, *max_chance)
            }
        }
    }
}

/// `Mth.inverseLerp(value, min, max)`.
fn inverse_lerp(value: f32, min: f32, max: f32) -> f32 {
    (value - min) / (max - min)
}

/// `Mth.clampedLerp(factor, min, max)` — and the factor is the first
/// argument, not the last.
fn clamped_lerp(factor: f32, min: f32, max: f32) -> f32 {
    if factor < 0.0 {
        min
    } else if factor > 1.0 {
        max
    } else {
        // `Mth.lerp(alpha, p0, p1)` is `p0 + alpha * (p1 - p0)`, which is not
        // `(1 - alpha) * p0 + alpha * p1` in floating point.
        min + factor * (max - min)
    }
}

impl RuleTest {
    /// # Errors
    /// If the predicate names a `predicate_type` this port does not have.
    pub fn parse(value: &Value) -> Result<Self, Malformed> {
        let kind = value
            .get("predicate_type")
            .and_then(Value::as_str)
            .ok_or_else(|| Malformed("a rule test has no `predicate_type`".to_string()))?;
        let text = |key: &str| -> Result<String, Malformed> {
            value
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| Malformed(format!("a rule test has no `{key}`")))
        };
        let probability = || -> f32 {
            // `Codec.FLOAT`, so the JSON number is read as an f32 and
            // compared with an f32 draw. Reading it as f64 and comparing
            // would differ on the last bit for values like 0.2.
            value
                .get("probability")
                .and_then(Value::as_f64)
                .unwrap_or(1.0) as f32
        };
        Ok(match kind {
            "minecraft:always_true" => Self::AlwaysTrue,
            "minecraft:block_match" => Self::BlockMatch(text("block")?),
            "minecraft:blockstate_match" => Self::BlockStateMatch(state_string(value)?),
            "minecraft:random_block_match" => Self::RandomBlockMatch {
                block: text("block")?,
                probability: probability(),
            },
            "minecraft:random_blockstate_match" => Self::RandomBlockStateMatch {
                state: state_string(value)?,
                probability: probability(),
            },
            "minecraft:tag_match" => Self::TagMatch(text("tag")?),
            other => return Err(Malformed(format!("unported rule test `{other}`"))),
        })
    }

    /// `RuleTest.test(state, random)`.
    fn test<W: PieceWorld + ?Sized, R: RandomSource + ?Sized>(
        &self,
        world: &W,
        state: &str,
        random: &mut R,
    ) -> bool {
        match self {
            Self::AlwaysTrue => true,
            Self::BlockMatch(block) => block_of(state) == block,
            Self::BlockStateMatch(want) => state == want,
            Self::RandomBlockMatch { block, probability } => {
                block_of(state) == block && random.next_f32() < *probability
            }
            Self::RandomBlockStateMatch {
                state: want,
                probability,
            } => state == want && random.next_f32() < *probability,
            Self::TagMatch(tag) => world.block_in_tag(tag, block_of(state)),
        }
    }
}

/// `blockstate_match`'s `block_state` field, which is a `Name`/`Properties`
/// object like an `output_state`.
fn state_string(value: &Value) -> Result<String, Malformed> {
    let inner = value
        .get("block_state")
        .ok_or_else(|| Malformed("a blockstate rule test has no `block_state`".to_string()))?;
    output_state(inner)
}

/// `ProcessorRule`.
///
/// The three predicates are tested in declaration order and the test is a
/// chain of `&&`, so a rule whose `input_predicate` fails spends nothing on
/// the two behind it. That short-circuit is the reason `position` is a field
/// on the rule rather than a pass over the rules: its draw has to land at the
/// position in the stream vanilla puts it at, which is *after* the other two
/// have passed.
#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
    pub input: RuleTest,
    pub location: RuleTest,
    /// `PosRuleTest`, defaulting to `PosAlwaysTrueTest.INSTANCE` when the
    /// document omits the field — which all but one of the jar's rules do.
    pub position: PosRuleTest,
    pub output: String,
    /// `block_entity_modifier`, when it is `minecraft:append_loot`: the
    /// table. `None` is `Passthrough`, the codec's default, which hands the
    /// info's own tag on. The four rules in the jar that set one are the
    /// trail ruins' archaeology rules, and each of them costs one `nextLong`
    /// on the rule's positional stream for the `LootTableSeed`.
    pub append_loot: Option<String>,
}

impl Rule {
    /// # Errors
    /// If any of the three predicates is unported.
    pub fn parse(value: &Value) -> Result<Self, Malformed> {
        // `PosRuleTest.CODEC.lenientOptionalFieldOf("position_predicate",
        // PosAlwaysTrueTest.INSTANCE)`. **Lenient**, so vanilla silently
        // takes the default when the field is present but unparseable; this
        // refuses instead, because a lenient default here is a rule that
        // stops spending its draw and every block after it in the same
        // position's stream then reads one place early.
        let position = value
            .get("position_predicate")
            .map_or(Ok(PosRuleTest::AlwaysTrue), PosRuleTest::parse)?;
        let input =
            RuleTest::parse(value.get("input_predicate").ok_or_else(|| {
                Malformed("a processor rule has no input_predicate".to_string())
            })?)?;
        let location = value
            .get("location_predicate")
            .map_or(Ok(RuleTest::AlwaysTrue), RuleTest::parse)?;
        let output = output_state(
            value
                .get("output_state")
                .ok_or_else(|| Malformed("a processor rule has no output_state".to_string()))?,
        )?;
        // `RuleBlockEntityModifier.CODEC.optionalFieldOf("block_entity_modifier",
        // Passthrough.INSTANCE)`. Read rather than ignored: until 2026-10-02
        // this parser dropped the field, so the trail ruins' 174 suspicious
        // blocks were placed with no table and no seed and the stream never
        // spent the draw.
        let append_loot = match value.get("block_entity_modifier") {
            None => None,
            Some(m) => match m.get("type").and_then(Value::as_str) {
                Some("minecraft:passthrough") => None,
                Some("minecraft:append_loot") => Some(
                    m.get("loot_table")
                        .and_then(Value::as_str)
                        .ok_or_else(|| Malformed("append_loot names no loot_table".to_string()))?
                        .to_string(),
                ),
                other => {
                    return Err(Malformed(format!(
                        "block_entity_modifier {other:?} is not ported"
                    )))
                }
            },
        };
        Ok(Self {
            input,
            location,
            position,
            output,
            append_loot,
        })
    }
}

/// One `StructureProcessor`.
#[derive(Clone, Debug, PartialEq)]
pub enum Processor {
    /// `BlockIgnoreProcessor` — the blocks whose infos are dropped.
    BlockIgnore(Vec<String>),
    /// `JigsawReplacementProcessor.INSTANCE`.
    JigsawReplacement,
    /// `RuleProcessor`.
    Rule(Vec<Rule>),
    /// `GravityProcessor(WORLD_SURFACE_WG, offset)`.
    Gravity { offset: i32 },
    /// `ProtectedBlockProcessor(cannotReplace)` — drops the info when the
    /// block **already in the world** at that position is in the tag.
    ///
    /// Note which state it tests: `level.getBlockState(info.pos())`, not
    /// `info.state()`. Every other processor here judges the template's own
    /// block; this one judges the terrain it is about to overwrite, which is
    /// why it needs a world read and why it cannot be folded into
    /// `BlockIgnore`.
    ProtectedBlocks(String),
    /// `NopProcessor`.
    Nop,
    /// `BlockAgeProcessor(mossiness)` — the ruin/decay pass.
    ///
    /// **Its draw sequence is the reason this is a variant and not a rule
    /// list.** Five disjoint arms, dispatched on the template's own state,
    /// each with a different draw count, and one of them
    /// (`maybeReplaceFullStoneBlock`) spends draws on a candidate array it
    /// then throws away. See [`block_age`].
    BlockAge(f32),
    /// `LavaSubmergedBlockProcessor` — a non-full-cube block placed into lava
    /// becomes lava.
    LavaSubmerged,
    /// `CappedProcessor(delegate, limit)` — a pass-through per block, then in
    /// `finalizeProcessing` the delegate is offered the piece's processed
    /// infos in a shuffled order (a legacy positional random off the world
    /// seed at the piece origin) until `limit` of them have changed. It is
    /// the one processor whose `evaluatesEntirePieceState()` is true, which
    /// turns off `processBlockInfos`' pre-clip: the whole piece is processed
    /// in every chunk, so the shuffle is the same wherever it runs. The
    /// trail ruins' archaeology lists are its users here; `limit` is a plain
    /// int (a `ConstantInt`, which draws nothing) in every 26.2 list.
    Capped {
        delegate: Box<Processor>,
        limit: i32,
    },
    /// `BlockRotProcessor(integrity)` with no `rottable_blocks`: each block
    /// survives when `settings.getRandom(pos).nextFloat() <= integrity`, the
    /// settings random being `RandomSource.create(Mth.getSeed(pos))` for a
    /// pool element — [`crate::structure::template::survives_rot`]. The
    /// outposts' overgrown watchtower is its jigsaw user (`outpost_rot`,
    /// 0.05).
    ///
    /// With `rottable_blocks` (a block tag, the only form 26.2 writes), a
    /// block **outside** the tag is kept without a draw being looked at, and
    /// only a block inside it takes the integrity test — the ancient city's
    /// `#minecraft:ancient_city_replaceable` at 0.95. The random is created
    /// from the position either way, so nothing is spent on any stream.
    BlockRot {
        integrity: f32,
        rottable: Option<String>,
    },
    /// `BlackstoneReplaceProcessor.INSTANCE`.
    ///
    /// Ported but **unmeasured**: the only overworld structure that reaches it
    /// is a ruined portal with `replace_with_blackstone`, and
    /// `ruined_portal.json` sets that false in both setups. It is
    /// `ruined_portal_nether.json` that turns it on.
    BlackstoneReplace,
}

impl Processor {
    /// # Errors
    /// If the processor type is one this port does not have.
    pub fn parse(value: &Value) -> Result<Self, Malformed> {
        let kind = value
            .get("processor_type")
            .and_then(Value::as_str)
            .ok_or_else(|| Malformed("a processor has no `processor_type`".to_string()))?;
        Ok(match kind {
            "minecraft:rule" => {
                let rules = value
                    .get("rules")
                    .and_then(Value::as_array)
                    .ok_or_else(|| Malformed("a rule processor has no `rules`".to_string()))?;
                Self::Rule(rules.iter().map(Rule::parse).collect::<Result<_, _>>()?)
            }
            "minecraft:nop" => Self::Nop,
            "minecraft:jigsaw_replacement" => Self::JigsawReplacement,
            // `value` is a `HolderSet<Block>`. Every use in 26.2 writes the
            // tag form; an inline list would need the block registry to
            // resolve, so it is refused rather than half-read — the same
            // choice `JigsawStructure::parse` makes for an inline biome list.
            "minecraft:protected_blocks" => {
                let value = value.get("value").ok_or_else(|| {
                    Malformed("a protected_blocks processor has no `value`".to_string())
                })?;
                let tag = value.as_str().ok_or_else(|| {
                    Malformed(format!(
                        "an inline protected_blocks list is unported: {value}"
                    ))
                })?;
                Self::ProtectedBlocks(tag.to_string())
            }
            "minecraft:gravity" => Self::Gravity {
                offset: value
                    .get("offset")
                    .and_then(Value::as_i64)
                    .unwrap_or(0)
                    .try_into()
                    .unwrap_or(0),
            },
            "minecraft:block_rot" => {
                let rottable = match value.get("rottable_blocks") {
                    None => None,
                    Some(v) => Some(
                        v.as_str()
                            .filter(|t| t.starts_with('#'))
                            .ok_or_else(|| {
                                Malformed(format!(
                                    "an inline block_rot `rottable_blocks` list is unported: {v}"
                                ))
                            })?
                            .to_string(),
                    ),
                };
                let integrity =
                    value
                        .get("integrity")
                        .and_then(Value::as_f64)
                        .ok_or_else(|| {
                            Malformed("a block_rot processor has no `integrity`".to_string())
                        })?;
                #[expect(clippy::cast_possible_truncation, reason = "the codec's own float")]
                Self::BlockRot {
                    integrity: integrity as f32,
                    rottable,
                }
            }
            "minecraft:capped" => {
                let delegate = value
                    .get("delegate")
                    .ok_or_else(|| Malformed("a capped processor has no `delegate`".to_string()))?;
                let limit = value
                    .get("limit")
                    .and_then(Value::as_i64)
                    .and_then(|v| i32::try_from(v).ok())
                    .ok_or_else(|| {
                        Malformed(format!(
                            "a capped processor's `limit` is not a constant int: {:?}",
                            value.get("limit")
                        ))
                    })?;
                Self::Capped {
                    delegate: Box::new(Self::parse(delegate)?),
                    limit,
                }
            }
            other => return Err(Malformed(format!("unported processor `{other}`"))),
        })
    }

    /// Every processor in a `StructureProcessorList` document.
    ///
    /// # Errors
    /// If any processor in the list is unported.
    pub fn parse_list(value: &Value) -> Result<Vec<Self>, Malformed> {
        let list = value
            .get("processors")
            .and_then(Value::as_array)
            .ok_or_else(|| Malformed("a processor list has no `processors`".to_string()))?;
        list.iter().map(Self::parse).collect()
    }
}

/// `BlockIgnoreProcessor.STRUCTURE_AND_AIR`.
#[must_use]
pub fn structure_and_air() -> Processor {
    Processor::BlockIgnore(vec![
        "minecraft:air".to_string(),
        "minecraft:structure_block".to_string(),
    ])
}

/// `BlockIgnoreProcessor.STRUCTURE_BLOCK`.
#[must_use]
pub fn structure_block() -> Processor {
    Processor::BlockIgnore(vec!["minecraft:structure_block".to_string()])
}

/// `SinglePoolElement.getSettings`, including the legacy subclass's
/// `popProcessor`/`addProcessor` swap.
///
/// `keep_jigsaws` is `postProcess`'s `false`, so the jigsaw replacement is
/// always on for a generated structure; the parameter exists because the
/// bytecode has it and the structure-block editor passes `true`.
#[must_use]
pub fn settings_processors(
    legacy: bool,
    keep_jigsaws: bool,
    element: &[Processor],
    projection: Projection,
) -> Vec<Processor> {
    let mut out = Vec::with_capacity(element.len() + 3);
    if !legacy {
        out.push(structure_block());
    }
    if !keep_jigsaws {
        out.push(Processor::JigsawReplacement);
    }
    out.extend(element.iter().cloned());
    if projection == Projection::TerrainMatching {
        // `Projection.TERRAIN_MATCHING`'s static init:
        // `new GravityProcessor(Heightmap.Types.WORLD_SURFACE_WG, -1)`.
        out.push(Processor::Gravity { offset: -1 });
    }
    if legacy {
        out.push(structure_and_air());
    }
    out
}

/// Whether a placed block's block entity implements `RandomizableContainer`,
/// which is what decides whether `placeInWorld` spends a `nextLong` on it.
///
/// Derived by intersecting two things in the 26.2 bytecode and nothing else:
/// the block-entity classes whose **declaration** names
/// `net.minecraft.world.RandomizableContainer`, directly or through a
/// superclass, against the `validBlocks` each of their types is registered
/// with in `BlockEntityTypes`. Eight concrete classes qualify:
/// `ChestBlockEntity` (and `TrappedChestBlockEntity`, which extends it),
/// `BarrelBlockEntity`, `DispenserBlockEntity` (and the dropper),
/// `HopperBlockEntity`, `ShulkerBoxBlockEntity`, `CrafterBlockEntity` — all
/// six via `extends RandomizableContainerBlockEntity` — and
/// `DecoratedPotBlockEntity`, which implements the interface directly beside
/// `ContainerSingleItem$BlockContainerSingleItem`.
///
/// # `suspicious_sand` and `suspicious_gravel` WERE HERE AND WERE WRONG — 2026-08-17
///
/// **`BrushableBlockEntity extends BlockEntity` and implements nothing.** It
/// carries its own private `lootTable`, `lootTableSeed`, `LOOT_TABLE_TAG`,
/// `LOOT_TABLE_SEED_TAG`, `tryLoadLootTable`, `trySaveLootTable` and a
/// `setLootTable(ResourceKey, long)` that takes the seed **from its caller** —
/// which is why it looks like a randomizable container from every angle except
/// the one `placeInWorld` reads. The `instanceof RandomizableContainer` test at
/// offset 478 is FALSE for it, so a suspicious block placed by a template
/// spends no draw at all, and its loot arrives through `handleDataMarker`
/// instead.
///
/// The list got them because it was derived from the classes that *mention*
/// the interface rather than from the classes that *declare* it, and those are
/// two different sets — this repo's central defect class wearing a different
/// hat: a proxy that agrees with the answer everywhere it was checked. It cost
/// nothing while every jigsaw call site passed `random: None`, and it would
/// have cost the whole of `docs/DETERMINISM.md` §44 the moment one did: trial
/// chambers hold suspicious gravel, so two spurious draws per block would have
/// put every later container in the chamber on the wrong seed while the block
/// states stayed identical and every instrument agreed.
///
/// Two more 26.2 shapes are easy to get wrong from memory and are checked
/// here: the sixteen coloured shulker boxes are one block, `dyed_shulker_box`,
/// with a colour property; and `copper_chest` shares `ChestBlockEntity` —
/// there is no `CopperChestBlockEntity` class in the jar, `CHEST` is
/// registered with `Util.copyAndAdd` over the chest list — so it counts.
/// **`ender_chest` does not**: `EnderChestBlockEntity` is registered next to
/// the chests and is not a `RandomizableContainer`, which is the one entry a
/// list written from the block names would wrongly include.
fn is_randomizable_container(block: &str) -> bool {
    matches!(
        block,
        "minecraft:chest"
            | "minecraft:copper_chest"
            | "minecraft:trapped_chest"
            | "minecraft:barrel"
            | "minecraft:dispenser"
            | "minecraft:dropper"
            | "minecraft:hopper"
            | "minecraft:shulker_box"
            | "minecraft:dyed_shulker_box"
            | "minecraft:decorated_pot"
            | "minecraft:crafter"
    )
}

/// One block on its way through the chain: world position, template-local
/// position, state, and whether it carried an `nbt` compound.
#[derive(Clone, Debug)]
struct Info {
    pos: (i32, i32, i32),
    local: (i32, i32, i32),
    state: String,
    jigsaw: Option<crate::structure::template::JigsawNbt>,
    /// Whether the block still carries an `nbt` compound.
    ///
    /// It rides the whole chain because `placeInWorld` reads it off the
    /// **processed** info, not the template's: `if (info.nbt() != null)` is
    /// the guard on the `LootTableSeed` draw below. All four of the
    /// state-rewriting processors here — `RuleProcessor`, `BlockAgeProcessor`,
    /// `LavaSubmergedBlockProcessor`, `BlackstoneReplaceProcessor` — construct
    /// their replacement with `blockinfo.nbt()`, i.e. they carry it, so it is
    /// preserved by `..info` in every branch of [`apply`] — except a
    /// `RuleProcessor` rule whose `block_entity_modifier` is `append_loot`,
    /// whose `getOutputTag` returns a tag even where the info had none.
    has_nbt: bool,
    /// The template block's whole `nbt` compound, carried for the same reason
    /// the two keys above are and replacing the need for a third.
    ///
    /// `TemplateBlock::payload`'s own doc has the argument: vanilla hands the
    /// whole compound to `loadWithComponents` and the block entity keeps what
    /// it understands, so plucking one key per class here does not terminate.
    /// `block_entity::saved_template_keys` is the single place that decides
    /// what survives into a payload.
    payload: Option<std::collections::BTreeMap<String, rustmc_world::nbt::Nbt>>,
}

/// The part of `StructurePlaceSettings` that changes where and how a block
/// lands, rather than which blocks survive the processors.
///
/// Bundled rather than passed as four positional arguments because vanilla
/// bundles them, and because the pivot and the waterlogging flag are exactly
/// the two settings a caller forgets: both are invisible in a window with no
/// rotated piece and no water, and both move blocks in one that has them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlaceSettings {
    /// `setRotation`.
    pub rotation: Rotation,
    /// `setRotationPivot`, in x and z — the y of vanilla's `BlockPos` pivot is
    /// never read, because `transform` rotates about the vertical axis.
    pub pivot: (i32, i32),
    /// `setMirror`, which vanilla defaults to `Mirror.NONE`.
    ///
    /// **This field is new, and its absence was a silent bug rather than a
    /// missing feature.** `place_piece` passed `Mirror::None` into `transform`
    /// as a literal and never mirrored the state, so a caller that had drawn a
    /// mirror had nowhere to put it. No measurement caught it because no
    /// structure this port builds was ever mirrored: an End city and a village
    /// are jigsaw structures and `SinglePoolElement` never sets a mirror, a
    /// shipwreck and an ocean ruin draw only a rotation. A ruined portal draws
    /// one on every placement — `nextFloat() < 0.5 ? NONE : FRONT_BACK` — and
    /// two of the four portals in the mined corpus are `FRONT_BACK`.
    ///
    /// It defaults to `None`, which is both vanilla's default and this
    /// project's opt-in rule for new behaviour, so adding it cannot move a
    /// block of any structure that does not ask for it.
    pub mirror: Mirror,
    /// `shouldApplyWaterlogging`, which is **on** by default in vanilla.
    ///
    /// It is off by default here so that turning it on for one structure
    /// cannot silently move another's blocks: a village or an End city that
    /// touches water would gain waterlogged states with no measurement behind
    /// them. Each caller opts in when it has a window that prices the change.
    pub waterlogging: bool,
    /// `setKnownShape`, which is **false** by default in vanilla — so vanilla
    /// re-derives every placed block's shape from its neighbours unless the
    /// caller says otherwise.
    ///
    /// `SinglePoolElement.getSettings` *does* call `setKnownShape(true)`, and
    /// that is the whole jigsaw family: an End city and a village skip the
    /// pass. `ShipwreckPieces.makeSettings` does not, and neither does any
    /// other hand-built `StructurePlaceSettings` — a comment in this file used
    /// to claim otherwise and was wrong.
    ///
    /// It defaults to `true` here, against vanilla, so that turning the pass
    /// on for one structure cannot silently move another's blocks. Each
    /// caller opts in when it has a window that prices the change.
    pub known_shape: bool,
}

impl PlaceSettings {
    /// A rotation and nothing else — vanilla's `new StructurePlaceSettings()`
    /// with `setRotation`, which is what an End city or village piece uses.
    #[must_use]
    pub const fn new(rotation: Rotation) -> Self {
        Self {
            rotation,
            pivot: (0, 0),
            mirror: Mirror::None,
            waterlogging: false,
            known_shape: true,
        }
    }

    /// `setMirror`.
    #[must_use]
    pub const fn with_mirror(mut self, mirror: Mirror) -> Self {
        self.mirror = mirror;
        self
    }

    /// `setKnownShape(false)` — run `Block.updateFromNeighbourShapes` over
    /// every block this piece writes, the way a non-jigsaw template does.
    #[must_use]
    pub const fn with_shape_update(mut self) -> Self {
        self.known_shape = false;
        self
    }

    /// `setRotationPivot`.
    #[must_use]
    pub const fn with_pivot(mut self, pivot: (i32, i32)) -> Self {
        self.pivot = pivot;
        self
    }

    /// Turn `shouldApplyWaterlogging` on for this piece.
    #[must_use]
    pub const fn with_waterlogging(mut self) -> Self {
        self.waterlogging = true;
        self
    }
}

/// The two `BlockPos` arguments `placeInWorld` takes.
///
/// They are **different positions** and they are read by different things,
/// which is the whole reason they are bundled here rather than left as one
/// argument and one implicit assumption.
///
/// `origin` is `placeInWorld`'s `position` — the piece's own
/// `templatePosition`. Every block's world position is
/// `calculateRelativePosition(settings, local).offset(origin)`, and it is
/// also what `getRandomPalette` hashes.
///
/// `reference` is `placeInWorld`'s `referencePos`. Nothing but a
/// `PosRuleTest` ever reads it, and `StructureStart.placeInChunk` computes it
/// **once per start**, from piece zero and no other piece.
///
/// ```text
///   BoundingBox centerBB = pieces.get(0).boundingBox;
///   BlockPos centerPos = centerBB.getCenter();
///   BlockPos referencePos =
///       new BlockPos(centerPos.getX(), centerBB.minY(), centerPos.getZ());
/// ```
///
/// So the reference's x and z come from `getCenter`, which is
/// `min + (max - min + 1) / 2` and **not** `(min + max) / 2` — the two differ
/// by one on an even span — while its **y is the box's `minY`, not the
/// centre's y**.
///
/// The trap this type exists to close is that `PlaceSettings::pivot` looks
/// like the same thing and is not. That field is `setRotationPivot`, it feeds
/// `transform`, and its y is dropped on purpose because rotation is about the
/// vertical axis. A position predicate on `axis = Y` reads exactly the y that
/// field discards, so reaching for the pivot here would place the structure
/// at the right x and z with its air-holes cut at the wrong heights.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PieceOrigin {
    /// `placeInWorld`'s `position`.
    pub origin: (i32, i32, i32),
    /// `placeInWorld`'s `referencePos`, when the caller has threaded one.
    pub reference: Option<(i32, i32, i32)>,
}

/// `StructureStart.placeInChunk`'s `referencePos`, from piece zero's box.
///
/// ```text
///   BoundingBox centerBB = pieces.get(0).boundingBox;
///   BlockPos centerPos = centerBB.getCenter();
///   BlockPos referencePos =
///       new BlockPos(centerPos.getX(), centerBB.minY(), centerPos.getZ());
/// ```
///
/// Two details that a reading of "the centre of the start" gets wrong:
///
/// * `getCenter()` is `min + (max - min + 1) / 2`, which is not
///   `(min + max) / 2`. They differ by one whenever the span is even, and a
///   bastion's `air_base` is 48 by 30 — both even — so on both starts in the
///   measured window the two formulas disagree in x and in z.
/// * the y is **`centerBB.minY()`**, the box's floor, not the centre's y. For
///   an `axis = Y` predicate that is the entire input: distance is measured
///   up from the bottom of piece zero.
///
/// It is piece **zero**, never the piece being placed, and it is the same
/// value for every piece of the start — including pieces whose own boxes are
/// nowhere near it.
#[must_use]
pub const fn start_reference_pos(first_piece: BoundingBox) -> (i32, i32, i32) {
    (
        first_piece.min_x + (first_piece.max_x - first_piece.min_x + 1) / 2,
        first_piece.min_y,
        first_piece.min_z + (first_piece.max_z - first_piece.min_z + 1) / 2,
    )
}

impl PieceOrigin {
    /// A piece placed with its start's `referencePos`.
    #[must_use]
    pub const fn new(origin: (i32, i32, i32), reference: (i32, i32, i32)) -> Self {
        Self {
            origin,
            reference: Some(reference),
        }
    }

    /// A caller that has **not** threaded a `referencePos`.
    ///
    /// Correct only while every processor list this caller resolves is free
    /// of a live `PosRuleTest`, which is a property of the datapack and is
    /// therefore asserted from the datapack rather than from a reading:
    /// `ci/structure-surface.py --positions` re-derives that exactly one
    /// `position_predicate` exists in the whole of 26.2, that it is in
    /// `high_rampart.json`, and that `high_rampart` is referenced only by
    /// `bastion/*` pools — none of which any of these callers opens. It is a
    /// named constructor rather than a bare `None` at the call site so that
    /// the claim is greppable.
    #[must_use]
    pub const fn unthreaded(origin: (i32, i32, i32)) -> Self {
        Self {
            origin,
            reference: None,
        }
    }
}

/// `StructureTemplate.processBlockInfos` then `placeInWorld`, for one piece.
///
/// Returns the number of blocks written.
///
/// # What is knowingly not done here
///
/// * **Entities and data markers.** `handleDataMarker` runs after the blocks
///   and the markers themselves are `structure_block`s, which the ignore
///   processor has already dropped. A village's villagers and cats are whole
///   *pieces* (`village/savanna/zombie/villagers/nitwit` is one of the 181),
///   and their templates contain an entity and no blocks.
/// * **The fluid ticks.** `placeLiquid` also calls `level.scheduleTick` for the
///   water it places. Nothing runs scheduled ticks during generation, so the
///   tick is dropped; the block change it is paired with is not.
///
/// # The one draw this takes, and why it is a parameter rather than nothing
///
/// `placeInWorld` spends **one `nextLong` per placed `RandomizableContainer`**
/// on the piece's own `RandomSource`, writing it into the block entity's
/// `LootTableSeed`. That is the *only* call in the whole template placement
/// that touches that stream: the processors seed themselves per position from
/// `Mth.getSeed(pos)` and never see it.
///
/// The seed itself is loot, which no block diff compares — so the draw looks
/// free and is not. Whatever runs next on the same stream lands one place
/// early without it. `RuinedPortalPiece.postProcess` is exactly that case:
/// `spreadNetherrack`'s `nextInt(max(1, 8 - avgSpan/2))` is the very next
/// draw, and it sets the apron's radius. Measured at seed 20260722 chunk
/// (48,64) with `worldgen-dump ruined-portal-apron`: vanilla enters the pass
/// at draw **1** and draws offset 3; entering at draw 0 draws offset 2 and
/// spreads a ring too far.
///
/// `random` is an `Option` because the four other callers in this tree do not
/// yet thread their piece's stream down here, and a caller that passes `None`
/// behaves exactly as this function did before. Those are named in the
/// worklist; the debt is one draw per chest and it is invisible until
/// something else reads the same stream afterwards.
/// * **Most `updateShape` families.** The edge walk below is faithful; the
///   *rule* it calls is only as wide as
///   [`crate::stage::post_process::update_shape`], which
///   answers fences, stairs, doors and seagrass because those are what a
///   measurement asked for. Walls, panes, iron bars, vines, chests and the
///   double plants are still identities.
pub fn place_piece<W: PieceWorld + ?Sized>(
    world: &mut W,
    template: &Template,
    at: PieceOrigin,
    settings: PlaceSettings,
    processors: &[Processor],
    clip: BoundingBox,
    // `&mut (dyn RandomSource + '_)` and not `&mut dyn RandomSource`: the
    // elided form ties the trait object's lifetime to the reference's, which
    // makes a caller's reborrow-per-piece — `StructureStart.placeInChunk`
    // handing one stream to every piece in turn — demand that the stream
    // outlive the caller itself. Two independent lifetimes, one borrow per
    // piece.
    mut random: Option<&mut (dyn RandomSource + '_)>,
) -> usize {
    let PieceOrigin { origin, reference } = at;
    let PlaceSettings {
        rotation,
        pivot,
        mirror,
        waterlogging,
        known_shape,
    } = settings;
    // `processBlockInfos` skips a block outside the settings' bounding box
    // *before* the processors run, but only when `keepBoundingBox` — which is
    // true unless some processor `evaluatesEntirePieceState()`. None of the
    // processors here does; `CappedProcessor` is the only one that does and no
    // village reaches it.
    // `placeInWorld` opens with `settings.getRandomPalette(this.palettes,
    // offset)`, and `offset` is the piece's `templatePosition` — the first of
    // its two `BlockPos` arguments, not the second. The second is
    // `StructureStart.placeInChunk`'s "centre of piece zero at its minY",
    // which would make the palette depend on which chunk decorated first; it
    // does not, and this call reproduces that.
    let palette = template.choose_palette(origin);
    // `keepBoundingBox` is false as soon as one processor evaluates the
    // entire piece — `CappedProcessor` — and then nothing is pre-clipped.
    let keep_bounding_box = !processors
        .iter()
        .any(|p| matches!(p, Processor::Capped { .. }));
    let mut infos: Vec<Info> = Vec::new();
    let mut originals: Vec<Info> = Vec::new();
    for block in &template.blocks {
        let (x, y, z) = transform(block.pos, mirror, rotation, pivot);
        let pos = (origin.0 + x, origin.1 + y, origin.2 + z);
        if keep_bounding_box && !contains(clip, pos) {
            continue;
        }
        let mut info = Some(Info {
            pos,
            local: block.pos,
            state: palette[block.state].clone(),
            jigsaw: block.jigsaw.clone(),
            has_nbt: block.has_nbt,
            payload: block.payload.clone(),
        });
        let original = if keep_bounding_box {
            None
        } else {
            info.clone()
        };
        for processor in processors {
            let Some(current) = info else { break };
            info = apply(world, processor, current, reference);
        }
        if let Some(info) = info {
            if let Some(original) = original {
                originals.push(original);
            }
            infos.push(info);
        }
    }
    // The finalize pass. Every processor but the capped one finalizes to its
    // input unchanged.
    for processor in processors {
        let Processor::Capped { delegate, limit } = processor else {
            continue;
        };
        let Some(seed) = world.world_seed() else {
            continue;
        };
        if *limit == 0 || infos.is_empty() || originals.len() != infos.len() {
            continue;
        }
        let n = usize::try_from(*limit).unwrap_or(0).min(infos.len());
        if n < 1 {
            continue;
        }
        use crate::rng::PositionalRandomFactory;
        let mut random = LegacyRandomSource::new(seed)
            .fork_positional()
            .at(origin.0, origin.1, origin.2);
        let order = crate::structure::processor::shuffled_order(&mut random, infos.len());
        let mut changed = 0usize;
        for index in order {
            if changed >= n {
                break;
            }
            let current = infos[index].clone();
            let before = (current.pos, current.state.clone());
            // `delegate.processBlock(level, offset, pivot, original,
            // processed, settings)`; the rule processor reads the processed
            // info, so `originals` only gates the size check above.
            if let Some(result) = apply(world, delegate, current, reference) {
                if (result.pos, result.state.as_str()) != (before.0, before.1.as_str()) {
                    changed += 1;
                    infos[index] = result;
                }
            }
        }
    }

    let mut written = 0usize;
    // `placeInWorld` keeps every position it actually wrote, in write order,
    // and re-walks it afterwards. The list is the pass's domain: a block the
    // box clipped away is not revisited.
    let mut placed: Vec<(i32, i32, i32)> = Vec::new();
    // `placeInWorld`'s two fluid lists, in vanilla's order of declaration.
    //
    // `sources` holds every position whose *placed* state is itself a fluid
    // source; `pending` holds every position where a waterloggable block was
    // placed into a fluid that was **not** a source — which, for a wreck sunk
    // into the sea floor, is every stair, fence and chest that landed in dirt,
    // stone or air. A comment here used to claim both lists stay empty for a
    // shipwreck. They do not: `pending` is where 69 blocks of this window's
    // disagreement lived.
    let mut sources: Vec<(i32, i32, i32)> = Vec::new();
    let mut pending: Vec<(i32, i32, i32)> = Vec::new();
    for info in infos {
        // `placeInWorld` re-checks the box, because a processor may have moved
        // the block: the gravity processor does exactly that.
        if !contains(clip, info.pos) {
            continue;
        }
        // The fluid is read **before** the write, per block, in this order —
        // so a block this piece has already placed does answer the question
        // for a later one, and reading the whole footprint up front would be
        // a different generator.
        let fluid = waterlogging.then(|| world.block_at(info.pos.0, info.pos.1, info.pos.2));
        // `placeInWorld`: `info.state().mirror(getMirror()).rotate(getRotation())`
        // — **mirror first, then rotate**, and the two do not commute. Applied
        // here rather than when the info was built because that is where
        // vanilla applies it: a processor sees the *unmirrored, unrotated*
        // template state, which is why a `RuleProcessor` matching
        // `stone_bricks` matches regardless of how the piece is turned.
        let state = rotate_state(&mirror_state(&info.state, mirror), rotation);
        world.set_block(info.pos.0, info.pos.1, info.pos.2, &state);
        // `placeInWorld`, immediately after the write: if the info still
        // carries nbt and the block entity that just appeared is a
        // `RandomizableContainer`, stamp `LootTableSeed = random.nextLong()`.
        // The value is loot and nothing here stores it; the **draw** is the
        // point. Guarded on the placed block rather than the template's,
        // because `getBlockEntity` asks the world what actually landed.
        // **THE DRAW AND THE RECORD ARE TWO DIFFERENT THINGS, and until
        // 2026-08-18 this was one `if` that conflated them.**
        //
        // The draw is `placeInWorld`'s: only a `RandomizableContainer`
        // carrying an `nbt` spends a `nextLong`, and only when a stream was
        // threaded to spend it from. That is a parity-relevant event and its
        // guard is unchanged below.
        //
        // The record is not an event at all. A template's tag is data the
        // piece already holds; carrying it costs no draw, changes no block and
        // needs no stream. Nesting it inside the draw's guard meant `vault`,
        // `trial_spawner` and every non-container class could never be
        // recorded — not because anything was missing but because they are not
        // `RandomizableContainer`s — and meant no class at all was recorded
        // with `RUSTMC_JIGSAW_STREAM` unset, which is the default.
        let seed = if info.has_nbt && is_randomizable_container(block_of(&state)) {
            random.as_deref_mut().map(|r| r.next_i64())
        } else {
            None
        };
        if info.has_nbt && crate::block_entity::entity_id(block_of(&state)).is_some() {
            world.record_block_entity(
                info.pos.0,
                info.pos.1,
                info.pos.2,
                crate::block_entity::Record {
                    nbt: info.payload.clone().unwrap_or_default(),
                    loot_seed: seed,
                },
            );
        }
        if let Some(fluid) = fluid {
            // `blockstate.getFluidState().isSource()` first: a template that
            // places water of its own contributes a source and is never a
            // flood candidate.
            if is_source(&state) {
                sources.push(info.pos);
            } else if is_waterloggable(&state) {
                if is_water(&fluid) {
                    if let Some(flooded) = place_liquid(&state) {
                        world.set_block(info.pos.0, info.pos.1, info.pos.2, &flooded);
                    }
                }
                if !is_source(&fluid) {
                    pending.push(info.pos);
                }
            }
        }
        placed.push(info.pos);
        written += 1;
    }

    flood(world, &mut pending, &sources);

    if !known_shape && !placed.is_empty() {
        update_shape_at_edge(world, &placed);
    }

    if !known_shape {
        // The second of `placeInWorld`'s two shape loops, and the one that
        // moves blocks here: for every position written, re-derive the state
        // from its six neighbours and write it back if it changed. It runs in
        // write order and reads the world as it now stands, so a block does
        // see what an earlier one in the same piece left behind.
        for (x, y, z) in placed {
            let state = world.block_at(x, y, z);
            let neighbours: [String; 6] = std::array::from_fn(|i| {
                let (dx, dy, dz) = crate::stage::post_process::OFFSET[i];
                world.block_at(x + dx, y + dy, z + dz)
            });
            let facts = WorldFacts(&*world);
            let updated = crate::stage::post_process::update_from_neighbour_shapes(
                &state,
                &neighbours,
                &facts,
                &facts,
            );
            if updated != state {
                world.set_block(x, y, z, &updated);
            }
        }
    }
    written
}

/// The six neighbouring states of a position, in `Direction` ordinal order.
fn neighbours_at<W: PieceWorld + ?Sized>(world: &mut W, x: i32, y: i32, z: i32) -> [String; 6] {
    std::array::from_fn(|i| {
        let (dx, dy, dz) = crate::stage::post_process::OFFSET[i];
        world.block_at(x + dx, y + dy, z + dz)
    })
}

/// One face of `updateShapeAtEdge`'s consumer: the filled cell at `(x, y, z)`
/// and the block across `direction`, each asked to `updateShape` against the
/// other.
///
/// The two calls are not symmetric and the order is load-bearing. The filled
/// cell answers first; its result is written; only then does the outside block
/// answer, and it is handed the **updated** inside state, exactly as vanilla
/// passes `blockstate2` into its second call. Writing before the second read
/// is what makes reading the outside block's own neighbours from the world
/// equivalent — `StairBlock#updateShape` re-derives its shape from the level,
/// not from the state it was given, and by then the level agrees.
fn update_face<W: PieceWorld + ?Sized>(world: &mut W, direction: usize, x: i32, y: i32, z: i32) {
    let (dx, dy, dz) = crate::stage::post_process::OFFSET[direction];
    let (nx, ny, nz) = (x + dx, y + dy, z + dz);
    let inside = world.block_at(x, y, z);
    let outside = world.block_at(nx, ny, nz);

    let inside_neighbours = neighbours_at(world, x, y, z);
    let updated_inside = {
        let facts = WorldFacts(&*world);
        crate::stage::post_process::update_shape(
            &inside,
            direction,
            &outside,
            &inside_neighbours,
            &facts,
            &facts,
        )
    };
    if updated_inside != inside {
        world.set_block(x, y, z, &updated_inside);
    }

    let outside_neighbours = neighbours_at(world, nx, ny, nz);
    let updated_outside = {
        let facts = WorldFacts(&*world);
        crate::stage::post_process::update_shape(
            &outside,
            crate::stage::post_process::opposite(direction),
            &updated_inside,
            &outside_neighbours,
            &facts,
            &facts,
        )
    };
    if updated_outside != outside {
        world.set_block(nx, ny, nz, &updated_outside);
    }
}

/// `StructureTemplate.updateShapeAtEdge`, the face walk that runs **before**
/// the per-position pass when the shape is not known.
///
/// # Why it is not the per-position pass again
///
/// The per-position pass only ever rewrites blocks the structure itself wrote.
/// This walk visits *faces*, and a face has two sides: the second of its two
/// `updateShape` calls lands on the block **outside** the placed volume. That
/// is the whole of what it buys, and it is how a shipwreck's waterlogged fence
/// rail deletes the seagrass that the sea floor grew before the wreck arrived.
///
/// # The volume, and why it is the written set and not the template
///
/// Vanilla sizes its `BitSetDiscreteVoxelShape` from the min/max of the
/// positions whose `setBlock` returned true — not from the template's size and
/// not from the clip box. A block the box cut away moves the box in with it,
/// so a piece clipped at a chunk edge walks a smaller volume, and the faces it
/// gains at the cut are real: vanilla updates against the chunk border too.
///
/// # The order
///
/// `DiscreteVoxelShape#forAllFaces` is three `forAllAxisFaces` passes, `NONE`
/// then `FORWARD` then `BACKWARD`. Reading the cycles out (`FORWARD.inverse()`
/// is `BACKWARD`, whose `cycle(x,y,z,axis)` is `axis.choose(y,z,x)`) turns the
/// generic loop into three concrete ones:
///
/// * pass 1 runs along **Z**, `for x { for y { .. } }`, `NORTH` at the first
///   filled cell of a run and `SOUTH` at the last;
/// * pass 2 runs along **Y**, `for z { for x { .. } }`, `DOWN` then `UP`;
/// * pass 3 runs along **X**, `for y { for z { .. } }`, `WEST` then `EAST`.
///
/// The order is not decoration. Every one of these calls can write, and a
/// later pass reads what an earlier one left, so the three passes do not
/// commute in general and are reproduced rather than reasoned about.
fn update_shape_at_edge<W: PieceWorld + ?Sized>(world: &mut W, placed: &[(i32, i32, i32)]) {
    let (mut min_x, mut min_y, mut min_z) = (i32::MAX, i32::MAX, i32::MAX);
    let (mut max_x, mut max_y, mut max_z) = (i32::MIN, i32::MIN, i32::MIN);
    for &(x, y, z) in placed {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        min_z = min_z.min(z);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
        max_z = max_z.max(z);
    }
    let size_x = (max_x - min_x + 1) as usize;
    let size_y = (max_y - min_y + 1) as usize;
    let size_z = (max_z - min_z + 1) as usize;
    let index = |x: usize, y: usize, z: usize| (x * size_y + y) * size_z + z;
    let mut full = vec![false; size_x * size_y * size_z];
    for &(x, y, z) in placed {
        full[index(
            (x - min_x) as usize,
            (y - min_y) as usize,
            (z - min_z) as usize,
        )] = true;
    }

    // Pass 1 — runs along Z.
    for x in 0..size_x {
        for y in 0..size_y {
            let mut previous = false;
            for z in 0..=size_z {
                let current = z < size_z && full[index(x, y, z)];
                let (wx, wy) = (min_x + x as i32, min_y + y as i32);
                if !previous && current {
                    update_face(world, 2, wx, wy, min_z + z as i32);
                }
                if previous && !current {
                    update_face(world, 3, wx, wy, min_z + z as i32 - 1);
                }
                previous = current;
            }
        }
    }
    // Pass 2 — runs along Y.
    for z in 0..size_z {
        for x in 0..size_x {
            let mut previous = false;
            for y in 0..=size_y {
                let current = y < size_y && full[index(x, y, z)];
                let (wx, wz) = (min_x + x as i32, min_z + z as i32);
                if !previous && current {
                    update_face(world, 0, wx, min_y + y as i32, wz);
                }
                if previous && !current {
                    update_face(world, 1, wx, min_y + y as i32 - 1, wz);
                }
                previous = current;
            }
        }
    }
    // Pass 3 — runs along X.
    for y in 0..size_y {
        for z in 0..size_z {
            let mut previous = false;
            for x in 0..=size_x {
                let current = x < size_x && full[index(x, y, z)];
                let (wy, wz) = (min_y + y as i32, min_z + z as i32);
                if !previous && current {
                    update_face(world, 4, min_x + x as i32, wy, wz);
                }
                if previous && !current {
                    update_face(world, 5, min_x + x as i32 - 1, wy, wz);
                }
                previous = current;
            }
        }
    }
}

/// `placeInWorld`'s write loop for a caller that has already run its own
/// processors and clipped to its box: write each state in order, with
/// `LiquidSettings.APPLY_WATERLOGGING`'s two fluid lists and the [`flood`]
/// pass, and nothing else — no shape pass, no block-entity record.
///
/// For the ocean ruin, whose `OceanRuinPiece.makeSettings` never calls
/// `setLiquidSettings` and so places with the default, APPLY. It wrote its
/// blocks raw until the cold ruin's top-half stone-brick stairs, which sit in
/// the sea, read `waterlogged=false` where vanilla has `true` (22 of the 22
/// residual blocks in the measured seed-12345 cold ruin box).
pub fn write_with_waterlogging<W: PieceWorld + ?Sized>(
    world: &mut W,
    blocks: &[((i32, i32, i32), String)],
) {
    let mut sources: Vec<(i32, i32, i32)> = Vec::new();
    let mut pending: Vec<(i32, i32, i32)> = Vec::new();
    for (pos, state) in blocks {
        let fluid = world.block_at(pos.0, pos.1, pos.2);
        world.set_block(pos.0, pos.1, pos.2, state);
        if is_source(state) {
            sources.push(*pos);
        } else if is_waterloggable(state) {
            if is_water(&fluid) {
                if let Some(flooded) = place_liquid(state) {
                    world.set_block(pos.0, pos.1, pos.2, &flooded);
                }
            }
            if !is_source(&fluid) {
                pending.push(*pos);
            }
        }
    }
    flood(world, &mut pending, &sources);
}

/// `placeInWorld`'s flood loop, which runs after every block is written and
/// **before** the shape pass.
///
/// A waterloggable block placed into something that was not a fluid source —
/// dirt, stone, air, the sea floor a wreck is sunk into — is left on `pending`
/// by the write loop. This walks that list to a fixed point and waterlogs any
/// entry that *touches* a source, so the sea reaches into the hull one block at
/// a time: the outer skin fills from the ocean, and the block behind it fills
/// from the skin, because a waterlogged block's own fluid state is a source.
///
/// ```text
///   boolean flag = true;
///   while (flag && !list.isEmpty()) {
///       flag = false;
///       for (Iterator<BlockPos> it = list.iterator(); it.hasNext(); ) {
///           BlockPos p = it.next();
///           FluidState f = level.getFluidState(p);
///           for (int i = 0; i < DIRECTIONS.length && !f.isSource(); i++) { ... }
///           if (f.isSource() && block instanceof LiquidBlockContainer c) {
///               c.placeLiquid(level, p, state, f); flag = true; it.remove();
///           }
///       }
///   }
/// ```
///
/// Two details of it are load-bearing and neither is obvious:
///
/// * **The direction array is five long — `UP, NORTH, EAST, SOUTH, WEST`.**
///   `DOWN` is absent, so water never propagates upward through the list. It is
///   built inline in the bytecode as a five-element `Direction[]`, not
///   `Direction.values()`.
/// * **A source the template itself placed does not count** — the neighbour
///   test skips any position in `sources`. Vanilla is refusing to let the
///   structure's own water flood the structure.
///
/// A position is removed from the list, and the outer sweep restarted, whenever
/// its fluid is a source and its block is waterloggable — vanilla discards
/// `placeLiquid`'s return value, so a block that was already waterlogged still
/// counts as progress.
fn flood<W: PieceWorld + ?Sized>(
    world: &mut W,
    pending: &mut Vec<(i32, i32, i32)>,
    sources: &[(i32, i32, i32)],
) {
    const DIRECTIONS: [(i32, i32, i32); 5] =
        [(0, 1, 0), (0, 0, -1), (1, 0, 0), (0, 0, 1), (-1, 0, 0)];
    let mut changed = true;
    while changed && !pending.is_empty() {
        changed = false;
        let mut i = 0;
        while i < pending.len() {
            let p = pending[i];
            let mut fluid = world.block_at(p.0, p.1, p.2);
            for (dx, dy, dz) in DIRECTIONS {
                if is_source(&fluid) {
                    break;
                }
                let q = (p.0 + dx, p.1 + dy, p.2 + dz);
                let neighbour = world.block_at(q.0, q.1, q.2);
                if is_source(&neighbour) && !sources.contains(&q) {
                    fluid = neighbour;
                }
            }
            if !is_source(&fluid) {
                i += 1;
                continue;
            }
            let state = world.block_at(p.0, p.1, p.2);
            if !is_waterloggable(&state) {
                i += 1;
                continue;
            }
            if is_water(&fluid) {
                if let Some(flooded) = place_liquid(&state) {
                    world.set_block(p.0, p.1, p.2, &flooded);
                }
            }
            changed = true;
            pending.remove(i);
        }
    }
}

/// `blockstate.getBlock() instanceof LiquidBlockContainer`, from a state.
///
/// Every block that carries `waterlogged` is one, and no block that lacks the
/// property can be given the state, so the property is the test. The other
/// implementors — powder snow, the cauldrons — appear in no structure template.
fn is_waterloggable(state: &str) -> bool {
    state.contains("waterlogged=")
}

/// `level.getFluidState(pos).isSource()`, from a block state.
///
/// Still water and any waterlogged block are sources; flowing water, which
/// carries a non-zero `level`, is not. The distinction is what stops the flood
/// loop from treating a falling column as a filled one.
fn is_source(state: &str) -> bool {
    (block_of(state) == "minecraft:water" && state.contains("level=0"))
        || state.contains("waterlogged=true")
}

/// `LiquidBlockContainer.placeLiquid` for the `SimpleWaterloggedBlock` default
/// — the only implementation any structure template reaches.
///
/// ```text
///   state.hasProperty(WATERLOGGED) && !state.getValue(WATERLOGGED)
///     && fluid.getType() == WATER   ->  setBlock(pos, state.setValue(WATERLOGGED, true))
/// ```
///
/// The property test *is* the `instanceof LiquidBlockContainer` test in
/// practice: a block that carries `waterlogged` is a `SimpleWaterloggedBlock`,
/// and one that does not cannot be given the state anyway. `None` is vanilla's
/// `false` — the block stays exactly as it was written.
fn place_liquid(state: &str) -> Option<String> {
    if !state.contains("waterlogged=false") {
        return None;
    }
    Some(state.replace("waterlogged=false", "waterlogged=true"))
}

/// `level.getFluidState(pos).getType() == Fluids.WATER`, from a block state.
///
/// Water itself and anything already waterlogged; `WaterloggedBlock` and the
/// aquatic plants that report a water fluid state without carrying the
/// property are **not** listed, because every one of them is written by
/// `vegetal_decoration`, which is step **9** — five steps after
/// `surface_structures`, so no structure can meet one during generation.
/// (This said "step 7 — three steps after" until 2026-08-06. The conclusion
/// was right and both numbers were wrong; `GenerationStep$Decoration`'s
/// eleven ordinals are listed in
/// [`crate::structure::step_index::DECORATION_STEPS`], read from bytecode.)
fn is_water(state: &str) -> bool {
    block_of(state) == "minecraft:water" || state.contains("waterlogged=true")
}

const fn contains(bb: BoundingBox, pos: (i32, i32, i32)) -> bool {
    pos.0 >= bb.min_x
        && pos.0 <= bb.max_x
        && pos.1 >= bb.min_y
        && pos.1 <= bb.max_y
        && pos.2 >= bb.min_z
        && pos.2 <= bb.max_z
}

/// One processor's `processBlock`. `None` is vanilla's `null` — the block is
/// dropped and the rest of the chain does not run.
/// `BlackstoneReplaceProcessor`'s replacement map, in the order the
/// constructor's `Util.make` consumer fills it.
///
/// A `HashMap` in vanilla, so its iteration order is not behaviour — only the
/// lookup is, which is why this is a match and not a table. Measured since
/// 2026-10-02 through `ruined_portal_nether`, whose setup sets
/// `replace_with_blackstone: true`.
const fn blackstone_replacement(block: &str) -> Option<&'static str> {
    Some(match block.as_bytes() {
        b"minecraft:cobblestone" | b"minecraft:mossy_cobblestone" => "minecraft:blackstone",
        b"minecraft:stone" => "minecraft:polished_blackstone",
        b"minecraft:stone_bricks" | b"minecraft:mossy_stone_bricks" => {
            "minecraft:polished_blackstone_bricks"
        }
        b"minecraft:cobblestone_stairs" | b"minecraft:mossy_cobblestone_stairs" => {
            "minecraft:blackstone_stairs"
        }
        b"minecraft:stone_stairs" => "minecraft:polished_blackstone_stairs",
        b"minecraft:stone_brick_stairs" | b"minecraft:mossy_stone_brick_stairs" => {
            "minecraft:polished_blackstone_brick_stairs"
        }
        b"minecraft:cobblestone_slab" | b"minecraft:mossy_cobblestone_slab" => {
            "minecraft:blackstone_slab"
        }
        b"minecraft:smooth_stone_slab" | b"minecraft:stone_slab" => {
            "minecraft:polished_blackstone_slab"
        }
        b"minecraft:stone_brick_slab" | b"minecraft:mossy_stone_brick_slab" => {
            "minecraft:polished_blackstone_brick_slab"
        }
        b"minecraft:stone_brick_wall" | b"minecraft:mossy_stone_brick_wall" => {
            "minecraft:polished_blackstone_brick_wall"
        }
        b"minecraft:cobblestone_wall" | b"minecraft:mossy_cobblestone_wall" => {
            "minecraft:blackstone_wall"
        }
        // The constructor's last three `put`s, missing here until 2026-10-02
        // because no measured window had reached this processor. The nether
        // ruined portals do, and their cracked bricks were coming out as
        // `cracked_stone_bricks`.
        b"minecraft:chiseled_stone_bricks" => "minecraft:chiseled_polished_blackstone",
        b"minecraft:cracked_stone_bricks" => "minecraft:cracked_polished_blackstone_bricks",
        b"minecraft:iron_bars" => "minecraft:iron_chain",
        _ => return None,
    })
}

/// Set one property of a canonical `name[k=v,...]` state, if the state already
/// declares it.
///
/// "If it already declares it" is `trySetValue`, not `setValue`: vanilla's
/// `copyProperty` silently keeps the target's default for a property the
/// target does not have, and inventing the pair here would spell a state that
/// does not exist.
fn set_prop(state: &str, key: &str, value: &str) -> String {
    let Some((name, rest)) = state.split_once('[') else {
        return state.to_string();
    };
    let raw = rest.trim_end_matches(']');
    let mut seen = false;
    let props: Vec<String> = raw
        .split(',')
        .map(|pair| match pair.split_once('=') {
            Some((k, _)) if k == key => {
                seen = true;
                format!("{key}={value}")
            }
            _ => pair.to_string(),
        })
        .collect();
    if !seen {
        return state.to_string();
    }
    format!("{name}[{}]", props.join(","))
}

/// `state.getValue(key)`, off the canonical `name[k=v,...]` spelling, or
/// `None` when the state does not declare it (`hasProperty`).
fn property_of<'s>(state: &'s str, key: &str) -> Option<&'s str> {
    let (_, rest) = state.split_once('[')?;
    rest.trim_end_matches(']').split(',').find_map(|pair| {
        pair.split_once('=')
            .filter(|(k, _)| *k == key)
            .map(|(_, v)| v)
    })
}

/// `Block.withPropertiesOf(BlockState)` — the target block's default state
/// with every property the **source** carries that the target also declares.
///
/// The iteration is over the *source's* properties, and the guard is
/// `blockstate.hasProperty`, so a property the target lacks is dropped rather
/// than added. A stair copied onto a slab keeps nothing but `waterlogged` and
/// `type`, which is the whole reason `maybeReplaceStairs` can hand a stair's
/// state to a slab at all.
fn with_properties_of(target_default: &str, source: &str) -> String {
    let Some((_, rest)) = source.split_once('[') else {
        return target_default.to_string();
    };
    let mut out = target_default.to_string();
    for pair in rest.trim_end_matches(']').split(',') {
        if let Some((k, v)) = pair.split_once('=') {
            out = set_prop(&out, k, v);
        }
    }
    out
}

/// `Direction.Plane.HORIZONTAL`'s `faces` array, **in its declared order**.
///
/// Read out of `Direction$Plane`'s static initialiser rather than from the
/// `Direction` ordinal, which groups the axes differently — `Util.getRandom`
/// indexes this array, so the order is behaviour. This project has already
/// paid once for assuming a direction order (`tools/oracle-runner`'s `DOWN`
/// face test).
const HORIZONTAL: [&str; 4] = ["north", "east", "south", "west"];

/// `Half.values()`, **in its declared order** — `TOP` is ordinal 0.
///
/// Read out of `Half`'s static initialiser and `$values()` rather than from
/// the alphabetical or the intuitive order, because `Util.getRandom` indexes
/// this array and the index is behaviour. `bottom` first would be a coin that
/// lands the other way up on every stair in the game.
const HALF: [&str; 2] = ["top", "bottom"];

/// `BlockAgeProcessor.getRandomFacingStairs` — **two** draws, not one.
///
/// ```text
///   stairs.defaultBlockState()
///       .setValue(StairBlock.FACING, Direction.Plane.HORIZONTAL.getRandomDirection(random))
///       .setValue(StairBlock.HALF,   Util.getRandom(Half.values(), random))
/// ```
///
/// Both `getRandomDirection` and `Util.getRandom(T[], RandomSource)` are one
/// `array[random.nextInt(array.length)]`, so this is `nextInt(4)` then
/// `nextInt(2)`, in that order — offsets 11 and 27 of the method.
///
/// This method is called only from `maybeReplaceFullStoneBlock`, and it is
/// called **twice** there, once per candidate array. Missing the `HALF` draw
/// therefore costs that arm two draws per qualifying block as well as leaving
/// every aged stair at the default `half=bottom`.
fn random_facing_stairs<W: PieceWorld + ?Sized, R: RandomSource + ?Sized>(
    world: &W,
    random: &mut R,
    block: &str,
) -> String {
    let base = world
        .default_state(block)
        .unwrap_or_else(|| block.to_string());
    let facing = HORIZONTAL[usize::try_from(random.next_i32_bound(4)).expect("0..4 is a usize")];
    let half = HALF[usize::try_from(random.next_i32_bound(2)).expect("0..2 is a usize")];
    set_prop(&set_prop(&base, "facing", facing), "half", half)
}

/// `BlockAgeProcessor.getRandomBlock(random, nonMossy, mossy)` and the static
/// `getRandomBlock(random, array)` it delegates to — a `nextFloat` against
/// `mossiness`, then a `nextInt(2)` into whichever array won.
fn random_block<R: RandomSource + ?Sized>(
    random: &mut R,
    mossiness: f32,
    non_mossy: &[String; 2],
    mossy: &[String; 2],
) -> String {
    let arm = if random.next_f32() < mossiness {
        mossy
    } else {
        non_mossy
    };
    arm[usize::try_from(random.next_i32_bound(2)).expect("0..2 is a usize")].clone()
}

/// `BlockAgeProcessor.processBlock` — the whole decay pass for one block.
///
/// Returns the replacement state, or `None` to leave the block alone.
///
/// # The draw order, which is the entire difficulty
///
/// Every arm opens with its own gate draw and the arms are **disjoint**, so
/// the number of draws a block spends depends on which arm it took. The trap
/// is `maybeReplaceFullStoneBlock`: it builds **both** candidate arrays before
/// calling `getRandomBlock`, and each array's second entry is a
/// `getRandomFacingStairs` — which is **two** draws, `nextInt(4)` for
/// `StairBlock.FACING` and `nextInt(2)` for `StairBlock.HALF`. So a
/// qualifying stone block spends
///
/// ```text
///   0  nextFloat   the 0.5 gate
///   1  nextInt(4)  non-mossy stairs FACING
///   2  nextInt(2)  non-mossy stairs HALF
///   3  nextInt(4)  mossy stairs FACING      <- spent on the losing array
///   4  nextInt(2)  mossy stairs HALF        <- spent on the losing array
///   5  nextFloat   mossiness
///   6  nextInt(2)  which entry of the winning pair
/// ```
///
/// **seven draws, four of them on the array that loses**. Building the arrays
/// lazily gives identical output for the first block and a stream short for
/// every block after it; dropping the `HALF` draw diverges at index 2 and
/// additionally leaves every aged stair at `half=bottom`.
///
/// The count was **five** in this comment and in the code until 2026-08-09,
/// when the tape put the first divergent index at 2. See the branch's
/// worklist block: it cost 14 blocks in the r=16 overworld portal box.
///
/// It is a fresh, position-seeded stream per block —
/// `settings.getRandom(info.pos())` with no `random` set on the settings
/// returns `RandomSource.create(Mth.getSeed(pos))` on **every call** — so this
/// does not share a stream with the `RuleProcessor` that ran before it, even
/// though both seed from the same position.
fn block_age<W: PieceWorld + ?Sized>(world: &W, mossiness: f32, info: &Info) -> Option<String> {
    let seed = position_seed(info.pos.0, info.pos.1, info.pos.2);
    let mut random = LegacyRandomSource::new(seed);
    if !tape::enabled() {
        return block_age_with(world, mossiness, info, &mut random);
    }
    // The tape wraps the **production** stream and the production call. A
    // recorder that re-derived the sequence from a copy of `block_age_with`
    // would agree with that copy no matter what the generator does, which is
    // the class of instrument this project has already been bitten by.
    let mut recorder = tape::Recording::new(&mut random);
    let out = block_age_with(world, mossiness, info, &mut recorder);
    let draws = recorder.finish();
    tape::write(info.pos, seed, &info.state, &draws, out.as_deref());
    out
}

/// The `block_age` draw tape — `RUSTMC_BLOCK_AGE_TAPE=<path>` and nothing else.
///
/// # Why this exists at all
///
/// `BlockAgeProcessor`'s stream is **per position** (`Mth.getSeed(pos)`), so a
/// wrong draw *order within one position* is enough to change that block and
/// nothing else. A block diff cannot see it: the arm that loses its
/// `getRandomFacingStairs` never reaches the world, so a draw spent on it is
/// invisible in the output of the block it was spent on. Only the ordered
/// sequence separates "wrong rule" from "wrong number of draws", and the
/// positions are few enough here that the tape is exhaustive rather than
/// sampled.
///
/// Off by default and gated on the env var alone: with the var unset
/// [`enabled`] is a relaxed load of an `AtomicU8` and `block_age` is byte for
/// byte the function it was before the tape existed.
mod tape {
    use std::io::Write as _;
    use std::sync::atomic::{AtomicU8, Ordering};
    use std::sync::{Mutex, OnceLock};

    use crate::rng::RandomSource;

    /// One draw, in the order the production path made it.
    ///
    /// `kind` is the vanilla call, not the Rust one, so the tape can be read
    /// straight against `javap` output.
    pub struct Draw {
        pub kind: &'static str,
        pub value: String,
    }

    /// 0 = not yet looked up, 1 = off, 2 = on.
    static STATE: AtomicU8 = AtomicU8::new(0);

    pub fn enabled() -> bool {
        match STATE.load(Ordering::Relaxed) {
            1 => false,
            2 => true,
            _ => {
                let on = std::env::var_os("RUSTMC_BLOCK_AGE_TAPE").is_some();
                STATE.store(u8::from(on) + 1, Ordering::Relaxed);
                on
            }
        }
    }

    /// A `RandomSource` that delegates every call and keeps the sequence.
    pub struct Recording<'a, R: RandomSource + ?Sized> {
        inner: &'a mut R,
        draws: Vec<Draw>,
    }

    impl<'a, R: RandomSource + ?Sized> Recording<'a, R> {
        pub fn new(inner: &'a mut R) -> Self {
            Self {
                inner,
                draws: Vec::new(),
            }
        }
        pub fn finish(self) -> Vec<Draw> {
            self.draws
        }
        fn push(&mut self, kind: &'static str, value: String) {
            self.draws.push(Draw { kind, value });
        }
    }

    impl<R: RandomSource + ?Sized> RandomSource for Recording<'_, R> {
        fn set_seed(&mut self, seed: i64) {
            self.inner.set_seed(seed);
            self.push("setSeed", seed.to_string());
        }
        fn next_i32(&mut self) -> i32 {
            let v = self.inner.next_i32();
            self.push("nextInt", v.to_string());
            v
        }
        fn next_i32_bound(&mut self, bound: i32) -> i32 {
            let v = self.inner.next_i32_bound(bound);
            self.draws.push(Draw {
                kind: if bound == 2 {
                    "nextInt(2)"
                } else if bound == 4 {
                    "nextInt(4)"
                } else {
                    "nextInt(n)"
                },
                value: v.to_string(),
            });
            v
        }
        fn next_i64(&mut self) -> i64 {
            let v = self.inner.next_i64();
            self.push("nextLong", v.to_string());
            v
        }
        fn next_bool(&mut self) -> bool {
            let v = self.inner.next_bool();
            self.push("nextBoolean", v.to_string());
            v
        }
        fn next_f32(&mut self) -> f32 {
            let v = self.inner.next_f32();
            self.push("nextFloat", format!("{v:.8}"));
            v
        }
        fn next_f64(&mut self) -> f64 {
            let v = self.inner.next_f64();
            self.push("nextDouble", format!("{v:.10}"));
            v
        }
        fn next_gaussian(&mut self) -> f64 {
            let v = self.inner.next_gaussian();
            self.push("nextGaussian", format!("{v:.10}"));
            v
        }
    }

    fn sink() -> &'static Mutex<Option<std::fs::File>> {
        static SINK: OnceLock<Mutex<Option<std::fs::File>>> = OnceLock::new();
        SINK.get_or_init(|| {
            let file = std::env::var_os("RUSTMC_BLOCK_AGE_TAPE").and_then(|p| {
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(p)
                    .ok()
            });
            Mutex::new(file)
        })
    }

    /// One line per `block_age` call. Sorting the file by its `pos=` field
    /// makes the tape order-independent, which it has to be: generation is
    /// threaded and the walk order is not the thing being measured here.
    pub fn write(pos: (i32, i32, i32), seed: i64, src: &str, draws: &[Draw], out: Option<&str>) {
        let mut line = format!(
            "TAPE pos={},{},{} seed={seed} src={src} draws={}",
            pos.0,
            pos.1,
            pos.2,
            draws.len()
        );
        for (i, d) in draws.iter().enumerate() {
            line.push_str(&format!(" {i}:{}={}", d.kind, d.value));
        }
        line.push_str(&format!(" out={}", out.unwrap_or("-")));
        if let Ok(mut guard) = sink().lock() {
            if let Some(file) = guard.as_mut() {
                let _ = writeln!(file, "{line}");
            }
        }
    }
}

/// [`block_age`] with the stream supplied, so a test can assert the draw
/// *sequence* against the production path rather than against a copy of it.
fn block_age_with<W: PieceWorld + ?Sized, R: RandomSource + ?Sized>(
    world: &W,
    mossiness: f32,
    info: &Info,
    random: &mut R,
) -> Option<String> {
    let state = info.state.as_str();
    let block = block_of(state);
    let default = |name: &str| -> String {
        world
            .default_state(name)
            .unwrap_or_else(|| name.to_string())
    };

    if matches!(
        block,
        "minecraft:stone_bricks" | "minecraft:stone" | "minecraft:chiseled_stone_bricks"
    ) {
        // `maybeReplaceFullStoneBlock`
        if random.next_f32() >= 0.5 {
            return None;
        }
        let non_mossy = [
            default("minecraft:cracked_stone_bricks"),
            random_facing_stairs(world, random, "minecraft:stone_brick_stairs"),
        ];
        let mossy = [
            default("minecraft:mossy_stone_bricks"),
            random_facing_stairs(world, random, "minecraft:mossy_stone_brick_stairs"),
        ];
        return Some(random_block(random, mossiness, &non_mossy, &mossy));
    }
    if world.block_in_tag("minecraft:stairs", block) {
        // `maybeReplaceStairs`
        if random.next_f32() >= 0.5 {
            return None;
        }
        // `NON_MOSSY_REPLACEMENTS`, a static pair of *default* states — note
        // it is not built from the source stair and costs no draw.
        let non_mossy = [
            default("minecraft:stone_slab"),
            default("minecraft:stone_brick_slab"),
        ];
        let mossy = [
            with_properties_of(&default("minecraft:mossy_stone_brick_stairs"), state),
            default("minecraft:mossy_stone_brick_slab"),
        ];
        return Some(random_block(random, mossiness, &non_mossy, &mossy));
    }
    if world.block_in_tag("minecraft:slabs", block) {
        // `maybeReplaceSlab` — one draw, and against `mossiness`, not 0.5.
        return (random.next_f32() < mossiness)
            .then(|| with_properties_of(&default("minecraft:mossy_stone_brick_slab"), state));
    }
    if world.block_in_tag("minecraft:walls", block) {
        // `maybeReplaceWall`
        return (random.next_f32() < mossiness)
            .then(|| with_properties_of(&default("minecraft:mossy_stone_brick_wall"), state));
    }
    if block == "minecraft:obsidian" {
        // `maybeReplaceObsidian` — 0.15, and the only arm with no array.
        return (random.next_f32() < 0.15).then(|| default("minecraft:crying_obsidian"));
    }
    None
}

/// `BlockStateParser.parseForBlock(holderLookup, spec, true).blockState()`.
///
/// Resolves against the block's **default** state, so a bare
/// `minecraft:grass_block` is really `grass_block[snowy=false]` and one naming
/// a single property acquires the rest at their defaults. Worth 19 blocks in
/// the r=16 overworld window when it was first written.
fn parse_final_state<W: PieceWorld + ?Sized>(world: &W, spec: &str) -> String {
    match world.default_state(block_of(spec)) {
        Some(base) => {
            struct One<'a>(&'a str, &'a str);
            impl crate::feature::state_spec::DefaultStates for One<'_> {
                fn default_state(&self, block: &str) -> Option<&str> {
                    (block == self.0).then_some(self.1)
                }
            }
            crate::feature::state_spec::parse_for_block(spec, &One(block_of(spec), &base))
                .unwrap_or_else(|_| spec.to_string())
        }
        // No table: the pre-2026-08-06 behaviour. See the trait's
        // `default_state` for why this is deliberately not a panic.
        None => spec.to_string(),
    }
}

/// `TemplateStructurePiece.postProcess`'s **second** pass: every jigsaw block
/// the template placed is overwritten with its own `final_state`.
///
/// Returns how many blocks it rewrote.
///
/// # Why this is not the jigsaw processor
///
/// `JigsawReplacementProcessor` resolves a jigsaw block *before* it is placed
/// and is only in the chain of structures that ask for it — a jigsaw
/// structure's `settings_processors` adds it, and `RuinedPortalPiece.
/// makeSettings` does not. So a ruined portal really does write a
/// `minecraft:jigsaw` block into the world, and then `postProcess` rubs it out
/// again, unconditionally, immediately after `placeInWorld` and **before**
/// anything a subclass does.
///
/// Skipping this is not cosmetic for a ruined portal and that is measured:
/// `ruined_portal/portal_3` has one jigsaw at template-local (3,0,4), which
/// under `FRONT_BACK` + `COUNTERCLOCKWISE_90` about pivot (4,4) from
/// `TP=768,42,1024` lands at (772,42,1035) — on the box floor, inside
/// `spreadNetherrack`'s inner disk. Vanilla leaves air there and the apron
/// refuses to replace air; a port that leaves the jigsaw block there sees
/// something replaceable, writes netherrack, spends a `nextFloat` and a whole
/// drip column of draws vanilla never spent, and **every later position in
/// the walk is off by that many draws**. One block, six wrong apron columns
/// behind it.
///
/// Only [`crate::structure::ruined_portal::post_process`] calls this today.
/// The other `place_piece` callers are jigsaw structures, whose chain already
/// dropped the jigsaw blocks, or the End city, which is measured at zero and
/// is not being moved on this branch without its own before/after.
pub fn resolve_jigsaw_blocks<W: PieceWorld + ?Sized>(
    world: &mut W,
    template: &Template,
    origin: (i32, i32, i32),
    settings: PlaceSettings,
    clip: BoundingBox,
) -> usize {
    let palette = template.choose_palette(origin);
    let mut rewritten = 0;
    for block in &template.blocks {
        if block_of(&palette[block.state]) != "minecraft:jigsaw" {
            continue;
        }
        // `filterBlocks` walks the **raw** palette, not the processed list, so
        // a processor that changed the block is irrelevant here; and it skips
        // an entry whose `nbt()` is null before reading `final_state`.
        let Some(jigsaw) = block.jigsaw.as_ref() else {
            continue;
        };
        let (x, y, z) = transform(
            block.pos,
            settings.mirror,
            settings.rotation,
            settings.pivot,
        );
        let pos = (origin.0 + x, origin.1 + y, origin.2 + z);
        if !contains(clip, pos) {
            continue;
        }
        // The final state is parsed fresh and written as it stands — vanilla
        // rotates the *jigsaw* block's own state inside `filterBlocks` and
        // then never uses it, and does not rotate what it parses.
        let resolved = parse_final_state(world, &jigsaw.final_state);
        world.set_block(pos.0, pos.1, pos.2, &resolved);
        rewritten += 1;
    }
    rewritten
}

fn apply<W: PieceWorld + ?Sized>(
    world: &mut W,
    processor: &Processor,
    info: Info,
    reference: Option<(i32, i32, i32)>,
) -> Option<Info> {
    match processor {
        Processor::Nop | Processor::Capped { .. } => Some(info),
        Processor::BlockRot {
            integrity,
            rottable,
        } => {
            // `rottableBlocks.isPresent() && !state.is(rottableBlocks)` keeps
            // the block before the float is compared.
            if let Some(tag) = rottable {
                if !world.block_in_tag(tag, block_of(&info.state)) {
                    return Some(info);
                }
            }
            crate::structure::template::survives_rot(info.pos, *integrity).then_some(info)
        }
        Processor::BlockIgnore(blocks) => {
            if blocks.iter().any(|b| b == block_of(&info.state)) {
                None
            } else {
                Some(info)
            }
        }
        Processor::JigsawReplacement => {
            if block_of(&info.state) != "minecraft:jigsaw" {
                return Some(info);
            }
            // `getStringOr("final_state", "minecraft:air")`, then
            // `BlockStateParser.parseForBlock`. A `structure_void` final state
            // drops the block instead of writing one.
            let final_state = info
                .jigsaw
                .as_ref()
                .map_or("minecraft:air", |j| j.final_state.as_str());
            if block_of(final_state) == "minecraft:structure_void" {
                return None;
            }
            // **This comment used to be the only place `parseForBlock`
            // appeared: the string was written through verbatim.** Vanilla
            // resolves it against the block's default state, so a
            // `final_state` of `minecraft:grass_block` is really
            // `grass_block[snowy=false]` and one naming a single property
            // acquires the rest at their defaults. Worth 19 blocks in the
            // r=16 overworld window — 16 `grass_block` `snowy`, plus
            // `waxed_copper_bulb` `powered`, `waxed_oxidized_copper_grate`
            // `waterlogged` and `acacia_stairs` `half`/`shape`/`waterlogged`.
            let resolved = parse_final_state(world, final_state);
            Some(Info {
                state: resolved,
                jigsaw: None,
                ..info
            })
        }
        Processor::ProtectedBlocks(tag) => {
            // `level.getBlockState(info.pos()).is(cannotReplace)` — the world's
            // block, at the info's own world position, not the template's.
            let existing = world.block_at(info.pos.0, info.pos.1, info.pos.2);
            if world.block_in_tag(tag, block_of(&existing)) {
                None
            } else {
                Some(info)
            }
        }
        Processor::Gravity { offset } => {
            let height = world.world_surface_wg(info.pos.0, info.pos.2) + offset;
            Some(Info {
                pos: (info.pos.0, height + info.local.1, info.pos.2),
                ..info
            })
        }
        Processor::BlockAge(mossiness) => {
            let aged = block_age(world, *mossiness, &info);
            Some(match aged {
                Some(state) => Info {
                    state,
                    jigsaw: None,
                    ..info
                },
                None => info,
            })
        }
        Processor::LavaSubmerged => {
            // `level.getBlockState(info.pos()).is(LAVA)` — the world's block,
            // not the template's, so this is a terrain question and it is
            // asked before the block is written.
            let existing = world.block_at(info.pos.0, info.pos.1, info.pos.2);
            if block_of(&existing) != "minecraft:lava" {
                return Some(info);
            }
            // `!Block.isShapeFullBlock(info.state().getShape(level, pos))` —
            // a full cube displaces the lava, anything else drowns in it.
            //
            // **This is asked of the collision shape, and the two are not the
            // same question.** The outline shape is what vanilla reads and no
            // column of `block_props.json` carries it; the collision shape is
            // carried, and the two agree for every block in every
            // ruined-portal template. Where they part is blocks with no
            // collision but a full outline — a lily pad, powder snow,
            // scaffolding, soul sand's dip — none of which any of these
            // templates contains. Regenerating the table with the outline
            // shape is the fix if a template ever gains one; this is written
            // down rather than left as an equivalence the next reader has to
            // rediscover.
            let full = world.shape_is_full_block(&info.state).unwrap_or(false);
            if full {
                return Some(info);
            }
            Some(Info {
                state: "minecraft:lava[level=0]".to_string(),
                jigsaw: None,
                ..info
            })
        }
        Processor::BlackstoneReplace => {
            let Some(target) = blackstone_replacement(block_of(&info.state)) else {
                return Some(info);
            };
            let base = world
                .default_state(target)
                .unwrap_or_else(|| target.to_string());
            // `processBlock` copies exactly three properties, each guarded by
            // `state.hasProperty` on the SOURCE: `StairBlock.FACING`, then
            // `StairBlock.HALF`, then `SlabBlock.TYPE` (offsets 46..141 in the
            // pinned jar). Nothing else — not a stair's `shape`, not a wall's
            // sides, not `waterlogged` — so the replacement keeps its own
            // defaults for those, and the post-process pass reshapes it. This
            // was `withPropertiesOf`, which copies every shared property.
            let mut state = base;
            for key in ["facing", "half", "type"] {
                if let Some(value) = property_of(&info.state, key) {
                    state = set_prop(&state, key, value);
                }
            }
            Some(Info {
                state,
                jigsaw: None,
                ..info
            })
        }
        Processor::Rule(rules) => {
            // One random per block, seeded from the **world** position, and
            // shared by every rule of this processor.
            let mut random =
                LegacyRandomSource::new(position_seed(info.pos.0, info.pos.1, info.pos.2));
            for rule in rules {
                if !rule.input.test(world, &info.state, &mut random) {
                    continue;
                }
                // `testAgainstWorldState(level, blockInfo.pos(), random)` —
                // the block's own world position, read from the world as it
                // stands, which for a village is the terrain plus whatever
                // pieces have already been written.
                let at = world.block_at(info.pos.0, info.pos.1, info.pos.2);
                if !rule.location.test(world, &at, &mut random) {
                    continue;
                }
                // `posPredicate.test(originalBlockInfo.pos(),
                // processedBlockInfo.pos(), referencePos, random)`.
                //
                // A caller that has not threaded a reference position cannot
                // be given a guess here: the y of the reference is exactly
                // what an `axis = Y` predicate reads, and inventing one
                // produces a plausible structure with the wrong blocks
                // removed. `PosRuleTest::AlwaysTrue` needs no reference and
                // spends no draw, so an unthreaded caller is only stopped by
                // a rule that actually has a live predicate — and
                // `golden_structure_place`'s reference test asserts that no
                // list any unthreaded caller resolves contains one, which is
                // what makes this arm unreachable rather than merely
                // defensive.
                match (&rule.position, reference) {
                    (PosRuleTest::AlwaysTrue, _) => {}
                    (predicate, Some(reference)) => {
                        if !predicate.test(info.pos, reference, &mut random) {
                            continue;
                        }
                    }
                    (_, None) => continue,
                }
                // `getOutputTag(random, info.nbt())`: `Passthrough` hands the
                // info's tag on; `AppendLoot` copies it (or starts empty),
                // stores the table and puts `LootTableSeed = random.nextLong()`
                // — the same positional stream the predicates drew from.
                if let Some(table) = &rule.append_loot {
                    let mut tag = info.payload.clone().unwrap_or_default();
                    tag.insert(
                        "LootTable".to_string(),
                        rustmc_world::nbt::Nbt::String(table.clone()),
                    );
                    tag.insert(
                        "LootTableSeed".to_string(),
                        rustmc_world::nbt::Nbt::Long(random.next_i64()),
                    );
                    return Some(Info {
                        state: rule.output.clone(),
                        jigsaw: None,
                        has_nbt: true,
                        payload: Some(tag),
                        ..info
                    });
                }
                return Some(Info {
                    state: rule.output.clone(),
                    jigsaw: None,
                    ..info
                });
            }
            Some(info)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `RandomSource` that answers from a script and **records what it was
    /// asked**, so a test can assert the draw *sequence* and not just the
    /// result.
    ///
    /// The sequence is the thing at risk here: two ports can agree on every
    /// block of the first position and diverge on the second because one of
    /// them spent a draw the other did not.
    struct Tape {
        floats: std::cell::RefCell<Vec<f32>>,
        ints: std::cell::RefCell<Vec<i32>>,
        log: std::cell::RefCell<Vec<String>>,
    }

    impl Tape {
        fn new(floats: &[f32], ints: &[i32]) -> Self {
            Self {
                floats: std::cell::RefCell::new(floats.iter().copied().rev().collect()),
                ints: std::cell::RefCell::new(ints.iter().copied().rev().collect()),
                log: std::cell::RefCell::new(Vec::new()),
            }
        }
        fn calls(&self) -> Vec<String> {
            self.log.borrow().clone()
        }
    }

    impl RandomSource for Tape {
        fn next_i32(&mut self) -> i32 {
            self.log.borrow_mut().push("nextInt".to_string());
            self.ints.borrow_mut().pop().unwrap_or(0)
        }
        fn next_i32_bound(&mut self, bound: i32) -> i32 {
            self.log.borrow_mut().push(format!("nextInt({bound})"));
            self.ints.borrow_mut().pop().unwrap_or(0)
        }
        fn next_i64(&mut self) -> i64 {
            self.log.borrow_mut().push("nextLong".to_string());
            0
        }
        fn next_bool(&mut self) -> bool {
            self.log.borrow_mut().push("nextBoolean".to_string());
            false
        }
        fn next_f32(&mut self) -> f32 {
            self.log.borrow_mut().push("nextFloat".to_string());
            self.floats.borrow_mut().pop().unwrap_or(0.0)
        }
        fn next_f64(&mut self) -> f64 {
            self.log.borrow_mut().push("nextDouble".to_string());
            f64::from(self.floats.borrow_mut().pop().unwrap_or(0.0))
        }
        fn next_gaussian(&mut self) -> f64 {
            self.log.borrow_mut().push("nextGaussian".to_string());
            0.0
        }
        fn set_seed(&mut self, _seed: i64) {
            self.log.borrow_mut().push("setSeed".to_string());
        }
    }

    /// A world that knows the three tags and a handful of default states.
    struct Facts;
    // A NO-OP, SAID OUT LOUD: `Facts` is a unit struct that answers questions
    // about block properties and holds nothing, so it has nowhere to put a
    // record. The recording branch has its own world and its own test below,
    // `the_container_draw_records_the_table_and_the_seed`.
    impl crate::block_entity::BlockEntityRecorder for Facts {
        fn record_block_entity(
            &mut self,
            _x: i32,
            _y: i32,
            _z: i32,
            _record: crate::block_entity::Record,
        ) {
        }
    }

    impl PieceWorld for Facts {
        fn block_at(&mut self, _x: i32, _y: i32, _z: i32) -> String {
            "minecraft:air".to_string()
        }
        fn set_block(&mut self, _x: i32, _y: i32, _z: i32, _state: &str) {}
        fn world_surface_wg(&mut self, _x: i32, _z: i32) -> i32 {
            0
        }
        fn block_in_tag(&self, tag: &str, block: &str) -> bool {
            match tag {
                "minecraft:stairs" => block.ends_with("_stairs"),
                "minecraft:slabs" => block.ends_with("_slab"),
                "minecraft:walls" => block.ends_with("_wall"),
                "#minecraft:ancient_city_replaceable" => block == "minecraft:deepslate_bricks",
                _ => false,
            }
        }
        fn default_state(&self, block: &str) -> Option<String> {
            Some(match block {
                "minecraft:stone_brick_stairs" | "minecraft:mossy_stone_brick_stairs" => {
                    format!("{block}[facing=north,half=bottom,shape=straight,waterlogged=false]")
                }
                "minecraft:stone_slab"
                | "minecraft:stone_brick_slab"
                | "minecraft:mossy_stone_brick_slab" => {
                    format!("{block}[type=bottom,waterlogged=false]")
                }
                other => other.to_string(),
            })
        }
    }

    fn info(state: &str) -> Info {
        Info {
            pos: (0, 0, 0),
            local: (0, 0, 0),
            state: state.to_string(),
            jigsaw: None,
            has_nbt: false,
            payload: None,
        }
    }

    /// `AppendLoot`: the matched rule's output carries `LootTable` and a
    /// `LootTableSeed` drawn from the rule's own positional stream after its
    /// predicates (a `block_match` and an `always_true` draw nothing, so the
    /// seed is that stream's first `nextLong`). A modifier this port does not
    /// know is refused, not ignored.
    #[test]
    fn an_append_loot_rule_gives_its_output_a_table_and_a_positional_seed() {
        let p = Processor::parse(&serde_json::json!({
            "processor_type": "minecraft:rule",
            "rules": [{
                "block_entity_modifier": {
                    "type": "minecraft:append_loot",
                    "loot_table": "minecraft:archaeology/trail_ruins_common"
                },
                "input_predicate": {"predicate_type": "minecraft:block_match", "block": "minecraft:gravel"},
                "location_predicate": {"predicate_type": "minecraft:always_true"},
                "output_state": {"Name": "minecraft:suspicious_gravel", "Properties": {"dusted": "0"}}
            }]
        }))
        .expect("an append_loot rule parses");
        let mut w = Facts;
        let mut at = info("minecraft:gravel");
        at.pos = (880, 64, 16);
        let out = apply(&mut w, &p, at, None).expect("kept");
        assert!(out.has_nbt);
        let tag = out.payload.expect("a tag");
        assert_eq!(
            tag.get("LootTable"),
            Some(&rustmc_world::nbt::Nbt::String(
                "minecraft:archaeology/trail_ruins_common".into()
            ))
        );
        let mut stream = LegacyRandomSource::new(position_seed(880, 64, 16));
        assert_eq!(
            tag.get("LootTableSeed"),
            Some(&rustmc_world::nbt::Nbt::Long(stream.next_i64()))
        );
        assert!(Processor::parse(&serde_json::json!({
            "processor_type": "minecraft:rule",
            "rules": [{
                "block_entity_modifier": {"type": "minecraft:append_static", "data": {}},
                "input_predicate": {"predicate_type": "minecraft:always_true"},
                "output_state": {"Name": "minecraft:stone"}
            }]
        }))
        .is_err());
    }

    /// `BlockRotProcessor` with `rottable_blocks`: a block outside the tag
    /// is kept before the integrity is compared, one inside it takes the
    /// test. At integrity 0 the test drops everything it reaches, so the two
    /// outcomes separate. This list was a parse error until the ancient city
    /// needed it — 314 refused pieces in one window.
    #[test]
    fn block_rot_with_a_tag_rots_only_the_tagged_blocks() {
        let p = Processor::parse(&serde_json::json!({
            "processor_type": "minecraft:block_rot",
            "integrity": 0.0,
            "rottable_blocks": "#minecraft:ancient_city_replaceable"
        }))
        .expect("a tagged block_rot parses");
        let mut w = Facts;
        assert!(apply(&mut w, &p, info("minecraft:deepslate_bricks"), None).is_none());
        assert!(apply(&mut w, &p, info("minecraft:soul_lantern"), None).is_some());
        assert!(Processor::parse(&serde_json::json!({
            "processor_type": "minecraft:block_rot",
            "integrity": 0.5,
            "rottable_blocks": ["minecraft:stone"]
        }))
        .is_err());
    }

    /// **The pre-registered risk, and the draw count it was registered with
    /// was wrong.** `maybeReplaceFullStoneBlock` builds both candidate arrays
    /// before it picks one, and `getRandomFacingStairs` is TWO draws — FACING
    /// then HALF. Seven draws, in this order, every time the 0.5 gate opens;
    /// four of them are spent on the array that loses.
    ///
    /// This test asserted **five** until 2026-08-09 and passed, because the
    /// port and the test made the same omission. The thing that moved it was
    /// the tape, which reads against `javap` rather than against the port.
    #[test]
    fn a_stone_block_spends_seven_draws_four_of_them_on_the_losing_array() {
        let mut t = Tape::new(&[0.1, 0.9], &[1, 0, 2, 1, 0]);
        let out = block_age_with(&Facts, 0.2, &info("minecraft:stone_bricks"), &mut t);
        assert_eq!(
            t.calls(),
            vec![
                "nextFloat".to_string(),  // the 0.5 gate
                "nextInt(4)".to_string(), // non-mossy stairs FACING
                "nextInt(2)".to_string(), // non-mossy stairs HALF
                "nextInt(4)".to_string(), // mossy stairs FACING — spent anyway
                "nextInt(2)".to_string(), // mossy stairs HALF    — spent anyway
                "nextFloat".to_string(),  // mossiness
                "nextInt(2)".to_string(), // which of the winning pair
            ],
            "seven draws in this order, or every later block is on the wrong stream"
        );
        // 0.9 >= mossiness 0.2, so the NON-mossy array wins, and the last
        // int (0) indexes its first entry.
        assert_eq!(out, Some("minecraft:cracked_stone_bricks".to_string()));
    }

    /// The same block with the mossy array winning picks entry 1 — the
    /// stairs whose facing came from the **second** `nextInt(4)` and whose
    /// half came from the **second** `nextInt(2)`, which are the two draws a
    /// lazy port would never have spent.
    #[test]
    fn the_mossy_array_uses_the_second_facing_and_half_draws() {
        // ints: 1 -> non-mossy stairs face EAST, 1 -> non-mossy half BOTTOM,
        //       2 -> mossy stairs face SOUTH,    0 -> mossy half TOP,
        //       1 -> take entry 1 of the winning array.
        let mut t = Tape::new(&[0.1, 0.05], &[1, 1, 2, 0, 1]);
        let out = block_age_with(&Facts, 0.2, &info("minecraft:stone_bricks"), &mut t);
        assert_eq!(
            out,
            Some(
                "minecraft:mossy_stone_brick_stairs[facing=south,half=top,shape=straight,waterlogged=false]"
                    .to_string()
            ),
            "the mossy stairs must carry the SECOND facing and the SECOND half"
        );
    }

    /// **The golden: vanilla's own `BlockAgeProcessor` draw tape, replayed.**
    ///
    /// `crates/rustmc-worldgen/golden/block_age_tape.json` is not a table of
    /// values — it is 554 ordered *draw sequences*, one per position the
    /// measured ruined portal hands to the processor, taken by running
    /// vanilla's own class in the JVM with a recording `RandomSource`
    /// injected through `StructurePlaceSettings.setRandom`. See
    /// `ci/block-age-golden.sh`.
    ///
    /// It gates the thing no block diff can reach. Both candidate arrays are
    /// built before `getRandomBlock` chooses, so the `getRandomFacingStairs`
    /// draws spent on the losing array are discarded — invisible in the
    /// output of the block that spent them, and visible only as a shift in
    /// the next block. Against this golden the pre-2026-08-09 port shows 18
    /// positions diverging at **draw index 2** and 14 wrong outputs; the
    /// fixed port shows 0 and 0.
    ///
    /// The seed is asserted too. `Mth.getSeed(pos)` is what makes every
    /// position independent, and a port that agreed on every draw while
    /// seeding differently would be right here and wrong everywhere else.
    #[test]
    fn the_block_age_tape_matches_vanillas_own_processor() {
        let raw = include_str!("../../golden/block_age_tape.json");
        let golden: Value = serde_json::from_str(raw).expect("golden json parses");
        let mossiness = golden["mossiness"].as_f64().expect("mossiness") as f32;
        let entries = golden["positions"].as_array().expect("positions");
        // A golden that shrank to nothing would pass every assertion below.
        assert_eq!(
            entries.len(),
            golden["count"].as_u64().expect("count") as usize,
            "the golden's own count disagrees with its position list"
        );
        assert!(entries.len() > 500, "the portal box is 554 positions");

        let mut checked_arms = std::collections::BTreeSet::new();
        for entry in entries {
            let p = entry["pos"].as_array().expect("pos");
            let pos = (
                p[0].as_i64().expect("x") as i32,
                p[1].as_i64().expect("y") as i32,
                p[2].as_i64().expect("z") as i32,
            );
            let src = entry["src"].as_str().expect("src");
            let seed = position_seed(pos.0, pos.1, pos.2);
            assert_eq!(
                seed,
                entry["seed"].as_i64().expect("seed"),
                "Mth.getSeed disagrees at {pos:?}"
            );

            let mut random = LegacyRandomSource::new(seed);
            let mut recorder = tape::Recording::new(&mut random);
            let info = Info {
                pos,
                local: (0, 0, 0),
                state: src.to_string(),
                jigsaw: None,
                has_nbt: false,
                payload: None,
            };
            let out = block_age_with(&Facts, mossiness, &info, &mut recorder);
            let ours: Vec<String> = recorder
                .finish()
                .iter()
                .map(|d| format!("{}={}", d.kind, d.value))
                .collect();

            let want: Vec<String> = entry["draws"]
                .as_array()
                .expect("draws")
                .iter()
                .map(|d| d.as_str().expect("draw").to_string())
                .collect();
            assert_eq!(
                ours, want,
                "draw sequence disagrees with vanilla at {pos:?} src={src}"
            );
            assert_eq!(
                out.as_deref(),
                entry["out"].as_str(),
                "output disagrees with vanilla at {pos:?} src={src}"
            );
            checked_arms.insert(want.len());
        }

        // **A gate that cannot fail is not a gate.** The stone arm is the
        // reason this golden exists; if the portal ever stops exercising it
        // the test must say so rather than passing on 554 empty tapes.
        assert!(
            checked_arms.contains(&7),
            "no seven-draw position in the golden — the stone arm is not being exercised"
        );
        assert!(
            checked_arms.contains(&1),
            "no one-draw position — the gated arms are not being exercised"
        );
    }

    /// `Half.values()` is `[TOP, BOTTOM]`, so ordinal **0 is top**.
    ///
    /// Alphabetical order and the order a reader expects both put `bottom`
    /// first, and either would land every aged stair the other way up while
    /// spending exactly the right number of draws — so no draw-count test
    /// and no stream-offset test can see it. `Direction.Plane`'s `faces`
    /// array cost this project a wrong assumption once already
    /// (`tools/oracle-runner`'s `DOWN` face test); this is the same shape.
    #[test]
    fn half_ordinal_zero_is_top() {
        // Both arrays draw facing=north (0) and half index 0; mossiness 0.0
        // so the non-mossy array wins, and the pick lands on entry 1 — the
        // stairs.
        let mut t = Tape::new(&[0.1, 0.9], &[0, 0, 0, 0, 1]);
        assert_eq!(
            block_age_with(&Facts, 0.0, &info("minecraft:stone_bricks"), &mut t),
            Some(
                "minecraft:stone_brick_stairs[facing=north,half=top,shape=straight,waterlogged=false]"
                    .to_string()
            ),
            "Half.values()[0] is TOP — read it out of Half's $values(), not from the alphabet"
        );
    }

    /// The gate is `>= 0.5 -> leave it alone`, and a closed gate spends
    /// exactly one draw — not five.
    #[test]
    fn a_closed_stone_gate_spends_one_draw_and_nothing_else() {
        let mut t = Tape::new(&[0.7], &[]);
        let out = block_age_with(&Facts, 0.2, &info("minecraft:stone_bricks"), &mut t);
        assert_eq!(out, None);
        assert_eq!(t.calls(), vec!["nextFloat".to_string()]);
    }

    /// A slab tests against `mossiness`, not against 0.5, and spends one draw.
    /// Getting the threshold wrong is invisible at mossiness 0.5 and wrong
    /// everywhere else — `ruined_portal.json` uses 0.2.
    #[test]
    fn a_slab_tests_against_mossiness_and_a_wall_does_too() {
        let mut t = Tape::new(&[0.1], &[]);
        let out = block_age_with(
            &Facts,
            0.2,
            &info("minecraft:stone_brick_slab[type=top,waterlogged=false]"),
            &mut t,
        );
        assert_eq!(t.calls(), vec!["nextFloat".to_string()]);
        // `withPropertiesOf` carries `type=top` across, which is the whole
        // point of it — a mossy slab must keep the original's half.
        assert_eq!(
            out,
            Some("minecraft:mossy_stone_brick_slab[type=top,waterlogged=false]".to_string())
        );

        let mut t = Tape::new(&[0.3], &[]);
        assert_eq!(
            block_age_with(
                &Facts,
                0.2,
                &info("minecraft:stone_brick_slab[type=top]"),
                &mut t
            ),
            None,
            "0.3 >= 0.2 leaves the slab alone"
        );
    }

    /// Obsidian is the only arm with its own probability, and 0.15 is not
    /// `mossiness`.
    #[test]
    fn obsidian_uses_its_own_fifteen_percent() {
        let mut t = Tape::new(&[0.14], &[]);
        assert_eq!(
            block_age_with(&Facts, 0.0, &info("minecraft:obsidian"), &mut t),
            Some("minecraft:crying_obsidian".to_string()),
            "0.14 < 0.15 even though mossiness is 0"
        );
        let mut t = Tape::new(&[0.16], &[]);
        assert_eq!(
            block_age_with(&Facts, 1.0, &info("minecraft:obsidian"), &mut t),
            None,
            "0.16 >= 0.15 even though mossiness is 1"
        );
    }

    /// The negative half: a block in none of the five arms spends **nothing**.
    /// A processor that drew first and dispatched afterwards would be
    /// output-identical on this block and shift every later one.
    #[test]
    fn a_block_no_arm_matches_spends_no_draw_at_all() {
        for block in [
            "minecraft:netherrack",
            "minecraft:air",
            "minecraft:gold_block",
            "minecraft:lava[level=0]",
        ] {
            let mut t = Tape::new(&[0.0], &[0]);
            assert_eq!(block_age_with(&Facts, 0.5, &info(block), &mut t), None);
            assert!(
                t.calls().is_empty(),
                "{block} reached a draw it should not have"
            );
        }
    }

    #[test]
    fn a_legacy_element_ignores_air_and_does_it_last() {
        // The whole point of reading the bytecode: `LegacySinglePoolElement`
        // pops `STRUCTURE_BLOCK` and appends `STRUCTURE_AND_AIR`, so the
        // ignore sits *after* the rule processors. A rule that rewrites a door
        // to air therefore drops the block rather than writing air.
        let list = settings_processors(true, false, &[Processor::Nop], Projection::Rigid);
        assert_eq!(
            list,
            vec![
                Processor::JigsawReplacement,
                Processor::Nop,
                structure_and_air()
            ]
        );
        let plain = settings_processors(false, false, &[Processor::Nop], Projection::Rigid);
        assert_eq!(
            plain,
            vec![
                structure_block(),
                Processor::JigsawReplacement,
                Processor::Nop
            ]
        );
    }

    #[test]
    fn a_terrain_matching_element_gets_the_gravity_processor_before_the_ignore() {
        let list = settings_processors(true, false, &[], Projection::TerrainMatching);
        assert_eq!(
            list,
            vec![
                Processor::JigsawReplacement,
                Processor::Gravity { offset: -1 },
                structure_and_air(),
            ]
        );
    }

    #[test]
    fn an_output_state_keeps_its_properties() {
        let value: Value = serde_json::from_str(
            r#"{"Name":"minecraft:grass_block","Properties":{"snowy":"false"}}"#,
        )
        .unwrap();
        assert_eq!(
            output_state(&value).unwrap(),
            "minecraft:grass_block[snowy=false]"
        );
        let bare: Value = serde_json::from_str(r#"{"Name":"minecraft:cobweb"}"#).unwrap();
        assert_eq!(output_state(&bare).unwrap(), "minecraft:cobweb");
    }

    #[test]
    fn a_random_block_match_spends_a_draw_only_when_the_block_matches() {
        struct NoWorld;
        // A NO-OP, SAID OUT LOUD: `NoWorld` answers nothing and writes
        // nothing, and is here to check the processor list rather than a
        // placement.
        impl crate::block_entity::BlockEntityRecorder for NoWorld {
            fn record_block_entity(
                &mut self,
                _x: i32,
                _y: i32,
                _z: i32,
                _record: crate::block_entity::Record,
            ) {
            }
        }

        impl PieceWorld for NoWorld {
            fn block_at(&mut self, _: i32, _: i32, _: i32) -> String {
                "minecraft:air".to_string()
            }
            fn set_block(&mut self, _: i32, _: i32, _: i32, _: &str) {}
            fn world_surface_wg(&mut self, _: i32, _: i32) -> i32 {
                63
            }
            fn block_in_tag(&self, _: &str, _: &str) -> bool {
                false
            }
        }
        let test = RuleTest::RandomBlockMatch {
            block: "minecraft:acacia_planks".to_string(),
            probability: 0.2,
        };
        let mut random = LegacyRandomSource::new(1);
        let before = random.next_f32();
        let mut a = LegacyRandomSource::new(1);
        // A block that does not match must not touch the stream at all, so the
        // next draw is still the first one.
        assert!(!test.test(&NoWorld, "minecraft:stone", &mut a));
        assert!((a.next_f32() - before).abs() < f32::EPSILON);
        // One that does match spends exactly one.
        let mut b = LegacyRandomSource::new(1);
        let hit = test.test(&NoWorld, "minecraft:acacia_planks", &mut b);
        assert_eq!(hit, before < 0.2);
    }

    /// The draw and the payload come out of the same branch, and a `None`
    /// stream produces neither.
    ///
    /// # What would make this go red
    ///
    /// Four things, and they are the four ways this could be wrong: recording
    /// a container the placer spent no draw on, spending a draw and recording
    /// nothing, recording the wrong table, and recording a seed that is not the
    /// value the stream actually yielded. The last is the one a
    /// value-comparing test cannot see, so the expected seed is taken from an
    /// independent replay of the same seeded stream rather than from the
    /// placer.
    #[test]
    fn the_container_draw_records_the_table_and_the_seed() {
        use crate::block_entity::{BlockEntityRecorder, Record};
        use crate::structure::template::TemplateBlock;
        use std::collections::BTreeMap;

        #[derive(Default)]
        struct Tally {
            blocks: BTreeMap<(i32, i32, i32), String>,
            records: Vec<((i32, i32, i32), Record)>,
        }
        impl BlockEntityRecorder for Tally {
            fn record_block_entity(&mut self, x: i32, y: i32, z: i32, record: Record) {
                self.records.push(((x, y, z), record));
            }
        }
        impl PieceWorld for Tally {
            fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
                self.blocks
                    .get(&(x, y, z))
                    .cloned()
                    .unwrap_or_else(|| "minecraft:air".to_string())
            }
            fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
                self.blocks.insert((x, y, z), state.to_string());
            }
            fn world_surface_wg(&mut self, _: i32, _: i32) -> i32 {
                63
            }
            fn block_in_tag(&self, _: &str, _: &str) -> bool {
                false
            }
        }

        let table = "minecraft:chests/trial_chambers/reward";
        let template = Template {
            size: (2, 1, 1),
            palette: vec![
                "minecraft:chest".to_string(),
                "minecraft:barrel".to_string(),
            ],
            palettes: vec![vec![
                "minecraft:chest".to_string(),
                "minecraft:barrel".to_string(),
            ]],
            blocks: vec![
                TemplateBlock {
                    pos: (0, 0, 0),
                    state: 0,
                    jigsaw: None,
                    has_nbt: true,
                    loot_table: Some(table.to_string()),
                    sherds: None,
                    // The tag now rides whole rather than as plucked keys, and
                    // this is the same tag: a chest whose template names a
                    // table and nothing else.
                    payload: Some(std::collections::BTreeMap::from([(
                        "LootTable".to_string(),
                        rustmc_world::nbt::Nbt::String(table.to_string()),
                    )])),
                    marker: None,
                },
                // A bare container: `placeInWorld` spends a draw on it because
                // it is a `RandomizableContainer` carrying an `nbt`, and the
                // save writes no seed because no table was set. The draw is
                // still the parity-relevant half, so the record must exist and
                // must carry the seed with no `LootTable`.
                TemplateBlock {
                    pos: (1, 0, 0),
                    state: 1,
                    jigsaw: None,
                    has_nbt: true,
                    loot_table: None,
                    sherds: None,
                    payload: None,
                    marker: None,
                },
            ],
        };
        let clip = BoundingBox::new(-16, -64, -16, 31, 319, 31);

        // The expected seeds, from an independent replay of the same stream.
        let mut replay = LegacyRandomSource::new(20260817);
        let want = [replay.next_i64(), replay.next_i64()];

        let mut with = Tally::default();
        let mut random = LegacyRandomSource::new(20260817);
        place_piece(
            &mut with,
            &template,
            PieceOrigin::unthreaded((0, 64, 0)),
            PlaceSettings::new(Rotation::None),
            &[],
            clip,
            Some(&mut random),
        );
        assert_eq!(with.records.len(), 2, "one record per placed container");
        assert_eq!(with.records[0].0, (0, 64, 0));
        assert_eq!(
            with.records[0]
                .1
                .nbt
                .get("LootTable")
                .and_then(|v| match v {
                    rustmc_world::nbt::Nbt::String(s) => Some(s.as_str()),
                    _ => None,
                }),
            Some(table)
        );
        assert_eq!(with.records[0].1.loot_seed, Some(want[0]));
        assert!(
            with.records[1].1.nbt.is_empty(),
            "a bare container records the draw and no table"
        );
        assert_eq!(with.records[1].1.loot_seed, Some(want[1]));

        // And with no stream: **the records still exist and carry no seed.**
        //
        // This assertion used to read `without.records.is_empty()`, and that
        // was the draw and the record conflated in a test as well as in the
        // code. A template tag is not a draw: the piece holds it either way,
        // and withholding it meant no class at all was recorded under the
        // default setting.
        //
        // What must stay silent is the EMISSION, and that guard lives one
        // layer down where it can be stated per class:
        // `block_entity::for_recorded` refuses a `LootTable` with no seed and
        // counts it as `EmitStats::incomplete`
        // (`a_loot_table_with_no_seed_is_refused_and_counted_separately`).
        // A bare container has no table to be seedless about, so it emits
        // `{Items:[]}` here as vanilla does — which is the gain this split
        // buys and is measured in `docs/DETERMINISM.md` §45.
        let mut without = Tally::default();
        place_piece(
            &mut without,
            &template,
            PieceOrigin::unthreaded((0, 64, 0)),
            PlaceSettings::new(Rotation::None),
            &[],
            clip,
            None,
        );
        assert_eq!(without.records.len(), 2, "the tags are held either way");
        assert!(
            without.records.iter().all(|r| r.1.loot_seed.is_none()),
            "no stream, so no draw was spent and no seed may be recorded"
        );
        // The blocks are identical either way — the whole affordability
        // question in one assertion.
        assert_eq!(with.blocks, without.blocks);
    }

    /// A suspicious block spends NO draw, and a chest beside it spends exactly
    /// one.
    ///
    /// # What would make this go red, and why it is a whole test
    ///
    /// `BrushableBlockEntity extends BlockEntity` — it is not a
    /// `RandomizableContainer`, so `placeInWorld`'s `instanceof` test at offset
    /// 478 is false for it. `suspicious_sand` and `suspicious_gravel` were in
    /// `is_randomizable_container` until 2026-08-17 because the list was
    /// derived from the classes that *mention* the interface instead of the
    /// classes that *declare* it.
    ///
    /// **That defect is invisible to every block-state comparison this project
    /// has**, because a spurious `nextLong` writes no block. It shows up only as
    /// a wrong `LootTableSeed` on whatever container comes next in the stream —
    /// and trial chambers hold suspicious gravel, so it would have put the 89
    /// positions §44 exists to emit onto the wrong seeds while the world stayed
    /// byte-identical. This test is the falsifier: re-add either block name and
    /// it goes red on the draw count and on the chest's recorded seed.
    #[test]
    fn a_suspicious_block_is_not_a_randomizable_container_and_spends_no_draw() {
        use crate::block_entity::{BlockEntityRecorder, Record};
        use crate::structure::template::TemplateBlock;
        use std::collections::BTreeMap;

        #[derive(Default)]
        struct Sink {
            records: Vec<((i32, i32, i32), Record)>,
        }
        impl BlockEntityRecorder for Sink {
            fn record_block_entity(&mut self, x: i32, y: i32, z: i32, record: Record) {
                self.records.push(((x, y, z), record));
            }
        }
        impl PieceWorld for Sink {
            fn block_at(&mut self, _: i32, _: i32, _: i32) -> String {
                "minecraft:air".to_string()
            }
            fn set_block(&mut self, _: i32, _: i32, _: i32, _: &str) {}
            fn world_surface_wg(&mut self, _: i32, _: i32) -> i32 {
                63
            }
            fn block_in_tag(&self, _: &str, _: &str) -> bool {
                false
            }
        }

        // Both blocks carry an `nbt`, which is the other half of the guard, so
        // the only thing separating them is the class test.
        let blocks: Vec<TemplateBlock> = ["minecraft:suspicious_gravel", "minecraft:chest"]
            .iter()
            .enumerate()
            .map(|(i, _)| TemplateBlock {
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_possible_wrap,
                    reason = "two blocks"
                )]
                pos: (i as i32, 0, 0),
                state: i,
                jigsaw: None,
                has_nbt: true,
                loot_table: None,
                sherds: None,
                payload: None,
                marker: None,
            })
            .collect();
        let palette = vec![
            "minecraft:suspicious_gravel".to_string(),
            "minecraft:chest".to_string(),
        ];
        let template = Template {
            size: (2, 1, 1),
            palette: palette.clone(),
            palettes: vec![palette],
            blocks,
        };

        let mut sink = Sink::default();
        let mut random = LegacyRandomSource::new(4242);
        place_piece(
            &mut sink,
            &template,
            PieceOrigin::unthreaded((0, 64, 0)),
            PlaceSettings::new(Rotation::None),
            &[],
            BoundingBox::new(-16, -64, -16, 31, 319, 31),
            Some(&mut random),
        );

        // **Both are recorded and only one spends a draw**, which is the
        // distinction this test exists for and is sharper than the record
        // count it used to assert. `suspicious_gravel` is a
        // `BrushableBlockEntity`, so it bears a block entity and its template
        // tag is worth carrying; it is not a `RandomizableContainer`, so
        // `placeInWorld` spends no `nextLong` on it.
        assert_eq!(sink.records.len(), 2, "both blocks bear a block entity");
        let gravel = sink
            .records
            .iter()
            .find(|r| r.0 == (0, 64, 0))
            .expect("the suspicious block is recorded");
        let chest = sink
            .records
            .iter()
            .find(|r| r.0 == (1, 64, 0))
            .expect("the chest is recorded");
        assert!(
            gravel.1.loot_seed.is_none(),
            "a suspicious block is not a RandomizableContainer and spends no draw"
        );
        assert!(
            chest.1.loot_seed.is_some(),
            "the chest is one, and spends exactly one"
        );

        // And exactly one value came off the stream: the next draw is the
        // SECOND of a fresh replay, not the third.
        let mut replay = LegacyRandomSource::new(4242);
        let first = replay.next_i64();
        // The chest's, not `records[0]`'s: the suspicious block is recorded
        // first now and carries no seed at all.
        assert_eq!(chest.1.loot_seed, Some(first));
        assert_eq!(random.next_i64(), replay.next_i64(), "one draw, not two");

        // Belt and braces on the predicate itself, so the intent survives a
        // rewrite of the placer.
        let want: BTreeMap<&str, bool> = BTreeMap::from([
            ("minecraft:chest", true),
            ("minecraft:copper_chest", true),
            ("minecraft:decorated_pot", true),
            ("minecraft:crafter", true),
            ("minecraft:suspicious_sand", false),
            ("minecraft:suspicious_gravel", false),
            ("minecraft:ender_chest", false),
            ("minecraft:vault", false),
            ("minecraft:trial_spawner", false),
        ]);
        for (block, expected) in want {
            assert_eq!(
                is_randomizable_container(block),
                expected,
                "{block} should be {expected}"
            );
        }
    }
}
