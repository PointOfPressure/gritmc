//! Block predicates — the question `block_predicate_filter` asks the world.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `net.minecraft.world.level.levelgen.blockpredicates.*`.
//!
//! The single biggest thing standing between a parsed placed feature and a
//! runnable one: 121 of the 262 chains contain a `block_predicate_filter`,
//! and it is how a feature asks "is there dirt under me", "am I in water",
//! "is this block replaceable".
//!
//! # The offset is on the predicate, not the filter
//!
//! Every state-testing predicate carries its own `offset`, defaulting to
//! `[0, 0, 0]`, and tests the block at `origin + offset`. So one filter can
//! ask about several different blocks by combining predicates with different
//! offsets — which is exactly how `all_of` is used to check a block and the
//! one beneath it in a single filter.
//!
//! # `all_of` and `any_of` short-circuit
//!
//! They stop at the first predicate that decides the answer. Nothing here
//! draws from the random stream, so short-circuiting is not observable
//! through the stream — but it is observable through how many blocks get
//! read, which matters once these run against a real chunk with a cost.
//!
//! # Three kinds need block behaviour this project does not have yet
//!
//! `would_survive` (56 uses), `replaceable` (2) and `solid` (1) ask a block
//! whether it can be placed, replaced or stood on. That is
//! `rustmc-behavior`'s job and it is a stub, so those parse to
//! [`BlockPredicate::NeedsBlockBehaviour`] and make evaluation fail rather
//! than guess. A predicate that quietly answers `true` puts a feature
//! everywhere it was meant to be excluded from.

use crate::tags::BlockTags;

/// What a predicate could not answer.
#[derive(Debug, PartialEq, Eq)]
pub struct NeedsBehaviour(pub String);

impl std::fmt::Display for NeedsBehaviour {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "block predicate `{}` needs block behaviour (rustmc-behavior)",
            self.0
        )
    }
}

impl std::error::Error for NeedsBehaviour {}

/// The world a predicate reads.
///
/// Names only, because that is all the predicates implemented here compare.
pub trait PredicateWorld {
    /// The block at this position, as a full state string.
    fn block_at(&mut self, x: i32, y: i32, z: i32) -> String;

    /// The fluid at this position, as a registry name, or
    /// `"minecraft:empty"`.
    ///
    /// **Not derivable from the block name.** `minecraft:seagrass` has a
    /// fluid state of `minecraft:water`, and so do 11,792 of the game's
    /// 32,366 block states — every waterlogged fence, slab and stair. The
    /// default here is the name-shaped guess this port used before it had a
    /// block-property table, kept only so a world that does not need fluids
    /// need not supply one; anything answering `matching_fluids` for real
    /// overrides it from `rustmc_behavior::block_props`.
    fn fluid_at(&mut self, x: i32, y: i32, z: i32) -> String {
        let block = self.block_at(x, y, z);
        let plain = block.split_once('[').map_or(block.as_str(), |(n, _)| n);
        match plain {
            "minecraft:water" => "minecraft:water".to_string(),
            "minecraft:lava" => "minecraft:lava".to_string(),
            _ => "minecraft:empty".to_string(),
        }
    }

    /// `BlockState#canSurvive` for `state` at this position, or `None` when
    /// this port has not implemented that block's rule.
    ///
    /// `None` is not "no": a feature that quietly places nothing is a world
    /// missing its saplings, and looks like a generator bug rather than a
    /// gap. See `rustmc_behavior::survival`.
    fn would_survive(&mut self, _state: &str, _x: i32, _y: i32, _z: i32) -> Option<bool> {
        None
    }

    /// `isFaceSturdy(direction, SupportType.FULL)`, or `None` when this world
    /// cannot say. `direction` indexes `Direction.values()`.
    fn face_is_sturdy(&mut self, _x: i32, _y: i32, _z: i32, _direction: usize) -> Option<bool> {
        None
    }

    /// `Block.canSupportCenter(level, pos, direction)`, or `None` when this
    /// world cannot say.
    ///
    /// **Not** [`Self::face_is_sturdy`]. That one fixes `SupportType.FULL`;
    /// this one is `SupportType.CENTER`, plus an outright refusal for a DOWN
    /// face on anything in `#minecraft:unstable_bottom_center`. A fence has
    /// no sturdy full face at all and supports a centre on all six, which is
    /// why a chain hangs from one.
    fn can_support_center(&mut self, _x: i32, _y: i32, _z: i32, _direction: usize) -> Option<bool> {
        None
    }

    /// `Block.isFaceFull(state.getCollisionShape(...), direction)`, or `None`
    /// when this world cannot say. `direction` indexes `Direction.values()`.
    ///
    /// A different question from [`Self::face_is_sturdy`], which asks the
    /// *support* shape: leaves have no sturdy face and a full collision face
    /// on all six. `MultifaceBlock.canAttachTo` accepts either, so a port
    /// with only one column grows no glow lichen on leaves.
    fn face_is_collision_full(
        &mut self,
        _x: i32,
        _y: i32,
        _z: i32,
        _direction: usize,
    ) -> Option<bool> {
        None
    }

    /// `LevelHeightAccessor#isInsideBuildHeight(y)`, or `None` when this
    /// world cannot say.
    fn inside_build_height(&self, _y: i32) -> Option<bool> {
        None
    }

    /// `BlockState#isSolid()`, or `None` when this world cannot say. Not the
    /// same as [`Self::solid_at`], which is `isSolidRender`.
    fn is_solid_at(&mut self, _x: i32, _y: i32, _z: i32) -> Option<bool> {
        None
    }

    /// `BlockState#canBeReplaced()`, or `None` when this world cannot say.
    fn replaceable_at(&mut self, _x: i32, _y: i32, _z: i32) -> Option<bool> {
        None
    }

    /// `BlockState#isAir`, or `None` when this world cannot say. `cave_air`
    /// and `void_air` are air; a name check misses both.
    fn air_at(&mut self, _x: i32, _y: i32, _z: i32) -> Option<bool> {
        None
    }

    /// `BlockState#isSolidRender`, or `None` when this world cannot say.
    ///
    /// `None` is not "no" — it makes the predicate fail loudly, because a
    /// `solid` test answered "false" by default would place features into
    /// open air and look like a generator bug rather than a missing table.
    fn solid_at(&mut self, _x: i32, _y: i32, _z: i32) -> Option<bool> {
        None
    }
}

/// `net.minecraft.world.level.levelgen.blockpredicates.BlockPredicate`.
#[derive(Clone, Debug, PartialEq)]
pub enum BlockPredicate {
    /// `true` — always passes. Vanilla's `TrueBlockPredicate`.
    Always,
    MatchingBlockTag {
        offset: [i32; 3],
        tag: String,
    },
    MatchingBlocks {
        offset: [i32; 3],
        /// Either a list of block names or a single `#tag`, matching the
        /// `HolderSet` codec.
        blocks: HolderSet,
    },
    MatchingFluids {
        offset: [i32; 3],
        fluids: HolderSet,
    },
    /// `inside_world_bounds` — is `origin + offset` inside the build
    /// height? Draws nothing and reads no block, only the dimension's
    /// vertical extent.
    ///
    /// It is what stops a lake being sited five blocks above the world floor
    /// and then carved through it, and it is the only predicate that asks the
    /// world a question with no position in it.
    InsideWorldBounds {
        offset: [i32; 3],
    },
    /// `replaceable` — `BlockState#canBeReplaced()`, the block's own
    /// `replaceable` property and not the placement-context form that asks
    /// what is being placed. Grass and water are replaceable; stone is not.
    Replaceable {
        offset: [i32; 3],
    },
    AllOf(Vec<BlockPredicate>),
    AnyOf(Vec<BlockPredicate>),
    Not(Box<BlockPredicate>),
    /// Parsed but unanswerable — carries the type name so a failure says
    /// which.
    /// `would_survive` — would this state survive at `origin + offset`?
    ///
    /// The most-asked unanswerable question in the generator: 56 of the 262
    /// placement chains use it, and 47 of those ask about a sapling. Answered
    /// by the world, which forwards to `rustmc_behavior::survival`.
    WouldSurvive {
        offset: [i32; 3],
        state: String,
    },
    /// `has_sturdy_face` — `isFaceSturdy(direction, SupportType.FULL)` at
    /// `origin + offset`.
    ///
    /// `direction` indexes `Direction.values()`: DOWN, UP, NORTH, SOUTH,
    /// WEST, EAST. Position-independent, because the three-argument
    /// `isFaceSturdy` delegates to `BlockStateBase.Cache` — which is why it
    /// can be answered from a table at all.
    HasSturdyFace {
        offset: [i32; 3],
        direction: usize,
    },
    /// `solid` — `BlockState#isSolidRender` at `origin + offset`.
    ///
    /// Answerable only where the world carries a block-property table; it
    /// fails loudly otherwise rather than defaulting.
    Solid {
        offset: [i32; 3],
    },
    NeedsBlockBehaviour(String),
}

/// A `HolderSet` in its datapack form: a `#tag` or an explicit list.
#[derive(Clone, Debug, PartialEq)]
pub enum HolderSet {
    Tag(String),
    Names(Vec<String>),
}

impl HolderSet {
    fn parse(value: &serde_json::Value) -> Result<Self, String> {
        match value {
            serde_json::Value::String(s) => Ok(if s.starts_with('#') {
                Self::Tag(s.clone())
            } else {
                Self::Names(vec![s.clone()])
            }),
            serde_json::Value::Array(items) => Ok(Self::Names(
                items
                    .iter()
                    .map(|v| {
                        v.as_str()
                            .map(str::to_string)
                            .ok_or_else(|| format!("holder set entry is not a string: {v}"))
                    })
                    .collect::<Result<_, _>>()?,
            )),
            other => Err(format!("not a holder set: {other}")),
        }
    }

    fn contains(&self, tags: &BlockTags, name: &str) -> bool {
        match self {
            Self::Tag(tag) => tags.get(tag).is_some_and(|set| set.contains(name)),
            Self::Names(names) => names.iter().any(|n| n == name),
        }
    }
}

/// Strips a `[k=v]` state suffix — predicates compare blocks, not states.
fn plain(name: &str) -> &str {
    name.split_once('[').map_or(name, |(n, _)| n)
}

impl BlockPredicate {
    /// `BlockPredicate#test`.
    ///
    /// # Errors
    /// If the predicate needs block behaviour that does not exist yet.
    pub fn test<W: PredicateWorld + ?Sized>(
        &self,
        world: &mut W,
        tags: &BlockTags,
        origin: (i32, i32, i32),
    ) -> Result<bool, NeedsBehaviour> {
        let at = |world: &mut W, offset: &[i32; 3]| {
            world.block_at(
                origin.0 + offset[0],
                origin.1 + offset[1],
                origin.2 + offset[2],
            )
        };
        match self {
            Self::Always => Ok(true),

            Self::MatchingBlockTag { offset, tag } => {
                let block = at(world, offset);
                Ok(tags.get(tag).is_some_and(|set| set.contains(plain(&block))))
            }

            Self::MatchingBlocks { offset, blocks } => {
                let block = at(world, offset);
                Ok(blocks.contains(tags, plain(&block)))
            }

            // `state.getFluidState().is(fluids)` — the *fluid* of the block,
            // so a waterlogged block matches water even though its name is
            // not water. Not modelled beyond the plain fluids, which is
            // stated rather than silently assumed: see the module note.
            Self::MatchingFluids { offset, fluids } => {
                let fluid = world.fluid_at(
                    origin.0 + offset[0],
                    origin.1 + offset[1],
                    origin.2 + offset[2],
                );
                Ok(fluids.contains(tags, &fluid))
            }

            // Short-circuits, as vanilla's loops do.
            Self::AllOf(predicates) => {
                for predicate in predicates {
                    if !predicate.test(world, tags, origin)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }

            Self::AnyOf(predicates) => {
                for predicate in predicates {
                    if predicate.test(world, tags, origin)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }

            Self::Not(inner) => Ok(!inner.test(world, tags, origin)?),

            // `ReplaceablePredicate#test`.
            Self::Replaceable { offset } => {
                let (x, y, z) = (
                    origin.0 + offset[0],
                    origin.1 + offset[1],
                    origin.2 + offset[2],
                );
                world
                    .replaceable_at(x, y, z)
                    .ok_or_else(|| NeedsBehaviour("canBeReplaced".to_string()))
            }

            // `LevelHeightAccessor.isInsideBuildHeight(y)`.
            Self::InsideWorldBounds { offset } => {
                let y = origin.1 + offset[1];
                world
                    .inside_build_height(y)
                    .ok_or_else(|| NeedsBehaviour("isInsideBuildHeight".to_string()))
            }

            Self::WouldSurvive { offset, state } => world
                .would_survive(
                    state,
                    origin.0 + offset[0],
                    origin.1 + offset[1],
                    origin.2 + offset[2],
                )
                .ok_or_else(|| NeedsBehaviour(format!("minecraft:would_survive ({state})"))),
            Self::HasSturdyFace { offset, direction } => world
                .face_is_sturdy(
                    origin.0 + offset[0],
                    origin.1 + offset[1],
                    origin.2 + offset[2],
                    *direction,
                )
                .ok_or_else(|| NeedsBehaviour("minecraft:has_sturdy_face".to_string())),
            Self::Solid { offset } => world
                .solid_at(
                    origin.0 + offset[0],
                    origin.1 + offset[1],
                    origin.2 + offset[2],
                )
                .ok_or_else(|| NeedsBehaviour("minecraft:solid".to_string())),
            Self::NeedsBlockBehaviour(name) => Err(NeedsBehaviour(name.clone())),
        }
    }

    /// Whether this predicate can be evaluated at all.
    #[must_use]
    pub fn is_supported(&self) -> bool {
        match self {
            // Supported in the sense that the port knows how to ask; whether
            // the *world* can answer is a separate question it raises at
            // evaluation time.
            Self::WouldSurvive { .. }
            | Self::HasSturdyFace { .. }
            | Self::Solid { .. }
            | Self::InsideWorldBounds { .. }
            | Self::Replaceable { .. } => true,
            Self::NeedsBlockBehaviour(_) => false,
            Self::AllOf(inner) | Self::AnyOf(inner) => inner.iter().all(Self::is_supported),
            Self::Not(inner) => inner.is_supported(),
            _ => true,
        }
    }

    /// Parse from datapack JSON.
    ///
    /// # Errors
    /// If the value names no type or a supported type is missing a field.
    /// An unsupported type is not an error — see
    /// [`Self::NeedsBlockBehaviour`].
    pub fn parse(value: &serde_json::Value) -> Result<Self, String> {
        let kind = value
            .get("type")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| format!("block predicate has no `type`: {value}"))?;

        // `Vec3i.offsetCodec(16).optionalFieldOf("offset", Vec3i.ZERO)` — an
        // absent offset means the block under test, not a default of one.
        let offset = || -> Result<[i32; 3], String> {
            match value.get("offset") {
                None => Ok([0, 0, 0]),
                Some(v) => {
                    let list = v
                        .as_array()
                        .ok_or_else(|| format!("offset is not a list: {v}"))?;
                    if list.len() != 3 {
                        return Err(format!("offset must have three components: {v}"));
                    }
                    let mut out = [0i32; 3];
                    for (slot, item) in out.iter_mut().zip(list) {
                        *slot = item
                            .as_i64()
                            .and_then(|n| i32::try_from(n).ok())
                            .ok_or_else(|| format!("offset component is not an int: {item}"))?;
                    }
                    Ok(out)
                }
            }
        };
        let children = |field: &str| -> Result<Vec<Self>, String> {
            value
                .get(field)
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| format!("{kind} has no `{field}` list"))?
                .iter()
                .map(Self::parse)
                .collect()
        };

        match kind {
            "minecraft:true" => Ok(Self::Always),
            "minecraft:matching_block_tag" => Ok(Self::MatchingBlockTag {
                offset: offset()?,
                tag: {
                    let tag = value
                        .get("tag")
                        .and_then(serde_json::Value::as_str)
                        .ok_or_else(|| "matching_block_tag has no `tag`".to_string())?;
                    // The codec stores a bare TagKey, so the JSON omits the
                    // `#` that a HolderSet would carry.
                    tag.to_string()
                },
            }),
            "minecraft:matching_blocks" => Ok(Self::MatchingBlocks {
                offset: offset()?,
                blocks: HolderSet::parse(
                    value
                        .get("blocks")
                        .ok_or_else(|| "matching_blocks has no `blocks`".to_string())?,
                )?,
            }),
            "minecraft:would_survive" => Ok(Self::WouldSurvive {
                offset: offset()?,
                state: {
                    let state = value
                        .get("state")
                        .ok_or_else(|| format!("{kind} has no `state`"))?;
                    let name = state
                        .get("Name")
                        .and_then(serde_json::Value::as_str)
                        .ok_or_else(|| format!("{kind} has no `state.Name`"))?;
                    name.to_string()
                },
            }),
            "minecraft:has_sturdy_face" => Ok(Self::HasSturdyFace {
                offset: offset()?,
                direction: match value
                    .get("direction")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| format!("{kind} has no `direction`"))?
                {
                    "down" => 0,
                    "up" => 1,
                    "north" => 2,
                    "south" => 3,
                    "west" => 4,
                    "east" => 5,
                    other => return Err(format!("{kind}: unknown direction `{other}`")),
                },
            }),
            "minecraft:solid" => Ok(Self::Solid { offset: offset()? }),
            "minecraft:matching_fluids" => Ok(Self::MatchingFluids {
                offset: offset()?,
                fluids: HolderSet::parse(
                    value
                        .get("fluids")
                        .ok_or_else(|| "matching_fluids has no `fluids`".to_string())?,
                )?,
            }),
            "minecraft:inside_world_bounds" => Ok(Self::InsideWorldBounds { offset: offset()? }),
            "minecraft:replaceable" => Ok(Self::Replaceable { offset: offset()? }),
            "minecraft:all_of" => Ok(Self::AllOf(children("predicates")?)),
            "minecraft:any_of" => Ok(Self::AnyOf(children("predicates")?)),
            "minecraft:not" => Ok(Self::Not(Box::new(Self::parse(
                value
                    .get("predicate")
                    .ok_or_else(|| "not has no `predicate`".to_string())?,
            )?))),
            other => Ok(Self::NeedsBlockBehaviour(other.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    struct Stack(BTreeMap<(i32, i32, i32), String>);

    impl PredicateWorld for Stack {
        fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
            self.0
                .get(&(x, y, z))
                .cloned()
                .unwrap_or_else(|| "minecraft:air".to_string())
        }
    }

    fn world(entries: &[((i32, i32, i32), &str)]) -> Stack {
        Stack(
            entries
                .iter()
                .map(|(p, b)| (*p, (*b).to_string()))
                .collect(),
        )
    }

    fn tags() -> BlockTags {
        BlockTags::from_entries(
            [(
                "minecraft:dirt".to_string(),
                vec![
                    "minecraft:dirt".to_string(),
                    "minecraft:grass_block".to_string(),
                    "minecraft:podzol".to_string(),
                ],
            )]
            .into_iter()
            .collect(),
        )
        .expect("tags")
    }

    #[test]
    fn an_absent_offset_tests_the_block_itself() {
        // `optionalFieldOf("offset", Vec3i.ZERO)`. Defaulting to anything
        // else would move every predicate in the game by a block.
        let parsed = BlockPredicate::parse(&serde_json::json!({
            "type": "minecraft:matching_block_tag", "tag": "minecraft:dirt"
        }))
        .expect("parses");
        assert_eq!(
            parsed,
            BlockPredicate::MatchingBlockTag {
                offset: [0, 0, 0],
                tag: "minecraft:dirt".into()
            }
        );
    }

    #[test]
    fn the_offset_is_applied_to_the_origin() {
        let predicate = BlockPredicate::MatchingBlockTag {
            offset: [0, -1, 0],
            tag: "minecraft:dirt".into(),
        };
        let tags = tags();
        let mut w = world(&[((5, 9, 5), "minecraft:grass_block")]);
        assert!(predicate
            .test(&mut w, &tags, (5, 10, 5))
            .expect("supported"));
        // The block *at* the origin is air, so a zero offset would fail —
        // which is what makes the offset observable.
        assert!(!predicate
            .test(&mut w, &tags, (5, 11, 5))
            .expect("supported"));
    }

    #[test]
    fn matching_blocks_takes_a_list_or_a_tag() {
        let tags = tags();
        let mut w = world(&[((0, 0, 0), "minecraft:podzol")]);

        let list = BlockPredicate::parse(&serde_json::json!({
            "type": "minecraft:matching_blocks", "blocks": ["minecraft:podzol", "minecraft:sand"]
        }))
        .expect("parses");
        assert!(list.test(&mut w, &tags, (0, 0, 0)).expect("supported"));

        let tag = BlockPredicate::parse(&serde_json::json!({
            "type": "minecraft:matching_blocks", "blocks": "#minecraft:dirt"
        }))
        .expect("parses");
        assert!(tag.test(&mut w, &tags, (0, 0, 0)).expect("supported"));

        let single = BlockPredicate::parse(&serde_json::json!({
            "type": "minecraft:matching_blocks", "blocks": "minecraft:sand"
        }))
        .expect("parses");
        assert!(!single.test(&mut w, &tags, (0, 0, 0)).expect("supported"));
    }

    #[test]
    fn a_block_state_suffix_does_not_stop_a_match() {
        // The golden palettes and the surface rules both carry states like
        // `water[level=0]`; predicates compare blocks.
        let tags = tags();
        let mut w = world(&[((0, 0, 0), "minecraft:grass_block[snowy=false]")]);
        let predicate = BlockPredicate::MatchingBlockTag {
            offset: [0, 0, 0],
            tag: "minecraft:dirt".into(),
        };
        assert!(predicate.test(&mut w, &tags, (0, 0, 0)).expect("supported"));
    }

    #[test]
    fn all_of_and_any_of_short_circuit() {
        let tags = tags();
        let mut w = world(&[]);
        let unanswerable = BlockPredicate::NeedsBlockBehaviour("minecraft:would_survive".into());
        let no = BlockPredicate::MatchingBlocks {
            offset: [0, 0, 0],
            blocks: HolderSet::Names(vec!["minecraft:stone".into()]),
        };
        let yes = BlockPredicate::MatchingBlocks {
            offset: [0, 0, 0],
            blocks: HolderSet::Names(vec!["minecraft:air".into()]),
        };

        // A failing first predicate means the unanswerable one is never
        // reached, so the whole thing answers rather than erroring.
        let all = BlockPredicate::AllOf(vec![no.clone(), unanswerable.clone()]);
        assert_eq!(all.test(&mut w, &tags, (0, 0, 0)), Ok(false));

        let any = BlockPredicate::AnyOf(vec![yes, unanswerable.clone()]);
        assert_eq!(any.test(&mut w, &tags, (0, 0, 0)), Ok(true));

        // And when it *is* reached, it fails loudly.
        let reached = BlockPredicate::AllOf(vec![unanswerable, no]);
        assert!(reached.test(&mut w, &tags, (0, 0, 0)).is_err());
    }

    #[test]
    fn is_supported_looks_through_the_combinators() {
        let unanswerable = BlockPredicate::NeedsBlockBehaviour("x".into());
        assert!(!unanswerable.is_supported());
        assert!(!BlockPredicate::Not(Box::new(unanswerable.clone())).is_supported());
        assert!(!BlockPredicate::AllOf(vec![BlockPredicate::Always, unanswerable]).is_supported());
        assert!(BlockPredicate::AllOf(vec![BlockPredicate::Always]).is_supported());
    }

    #[test]
    fn the_real_predicates_parse_and_most_are_answerable() {
        let dir = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../oracle-work/vanilla-26.2/generated-data/data/minecraft/worldgen/placed_feature"
        ));
        if !rustmc_data::fixture::available(dir, "datagen extract", "ci/vanilla-server.sh data") {
            return;
        }
        let mut total = 0usize;
        let mut supported = 0usize;
        for entry in std::fs::read_dir(dir).expect("readable") {
            let path = entry.expect("entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let json: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).expect("readable"))
                    .expect("json");
            for modifier in json["placement"].as_array().into_iter().flatten() {
                if modifier["type"] != "minecraft:block_predicate_filter" {
                    continue;
                }
                let predicate = BlockPredicate::parse(&modifier["predicate"])
                    .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                total += 1;
                if predicate.is_supported() {
                    supported += 1;
                }
            }
        }
        assert!(total > 100, "only {total} block predicate filters");
        println!("BLOCK PREDICATES: {supported}/{total} filters answerable");
        assert!(
            supported > 50,
            "only {supported}/{total} answerable — coverage went backwards"
        );
    }
}
