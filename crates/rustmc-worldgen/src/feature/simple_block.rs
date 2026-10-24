//! The simple-block feature — one block, if it can stand there.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `net.minecraft.world.level.levelgen.feature.SimpleBlockFeature`,
//! `configurations.SimpleBlockConfiguration`.
//!
//! Thirty-two configured features are `simple_block` — the largest single
//! type after `ore`, and the one most of the world's grass, flowers and
//! lichen comes through.
//!
//! # It is four lines and three of them are branches
//!
//! ```text
//! state = toPlace.getOptionalState(level, random, pos)
//! if (state == null)                      return false
//! if (!state.canSurvive(level, pos))      return false
//! if (block instanceof DoublePlantBlock)  { needs air above; placeAt }
//! else if (block instanceof MossyCarpetBlock) { placeAt with level.getRandom() }
//! else                                    level.setBlock(pos, state, 2)
//! ```
//!
//! The draw is the provider's and happens **before** the survival test, so a
//! `weighted_state_provider` costs its draw even where nothing can be placed.
//! Moving the survival test earlier would be a natural-looking optimisation
//! and would shift every feature after it.
//!
//! # The two special blocks, and what each of them needed
//!
//! `DoublePlantBlock.placeAt` writes two blocks and is ported here.
//!
//! `MossyCarpetBlock.placeAt` draws from the **level's** random rather than
//! the feature's — a different stream, positional per chunk — and was refused
//! outright on that ground until 2026-08-18. The refusal was wider than the
//! thing being refused: the random reaches one branch, and on open ground that
//! branch takes no draw at all. `crate::feature::mossy_carpet` ports the
//! no-draw case exactly and refuses the other, which is 512 blocks and 68 pale
//! oaks in window A. See `docs/PARITY-ROADMAP.md` §89.4.
//!
//! Neither is guessed at. Where a branch is still unported, [`place`] returns
//! [`Unplaceable`] naming it, so a feature that needs it is visibly unported
//! rather than quietly placing a single flower where vanilla places a
//! sunflower.

use crate::rng::RandomSource;

use super::block_predicate::{NeedsBehaviour, PredicateWorld};
use super::state_provider::StateProvider;
use crate::tags::BlockTags;

// Trace switches, read once each rather than per call: `getenv` in a
// feature inner loop was 3.8% of a full pipeline. See
// `crate::trace_switch`.
crate::trace_switch!(trace_patch, "RUSTMC_TRACE_PATCH");

/// `SimpleBlockConfiguration`.
#[derive(Clone, Debug, PartialEq)]
pub struct SimpleBlockConfig {
    pub to_place: StateProvider,
}

impl SimpleBlockConfig {
    /// # Errors
    /// If `to_place` is missing or names an unimplemented provider.
    pub fn parse(
        config: &serde_json::Value,
        states: &dyn super::state_spec::DefaultStates,
    ) -> Result<Self, String> {
        Ok(Self {
            to_place: StateProvider::parse(
                config
                    .get("to_place")
                    .ok_or_else(|| "simple_block config has no `to_place`".to_string())?,
                states,
            )?,
        })
    }

    #[must_use]
    pub fn is_supported(&self) -> bool {
        self.to_place.is_supported()
    }
}

/// The world this feature reads and writes.
pub trait SimpleBlockWorld: PredicateWorld {
    fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str);

    /// `LevelAccessor#getRandom().nextBoolean()` — one draw off the **level's**
    /// stream, which is not the feature's.
    ///
    /// Inside worldgen that stream is `WorldGenRegion.getRandom()`, a
    /// positional random taken once per generating chunk at its own origin.
    /// It is therefore reproducible, which is the fact
    /// `crate::feature::mossy_carpet` is built on — and that module is the
    /// only caller, because it is the only place vanilla reaches for this
    /// random during decoration.
    ///
    /// `None` means this world has no such stream. That is a **refusal**, not
    /// a false: a caller must turn it into an `Unplaceable` rather than pick a
    /// branch, because both branches write plausible blocks and only a census
    /// would ever notice.
    fn level_random_next_boolean(&mut self) -> Option<bool> {
        None
    }

    /// `BlockGetter#isEmptyBlock` — `getBlockState(pos).isAir()`, which is
    /// true for `cave_air` and `void_air` as well.
    fn is_empty_block(&mut self, x: i32, y: i32, z: i32) -> Option<bool>;

    /// `LevelReader#isWaterAt` — `getFluidState(pos).is(FluidTags.WATER)`,
    /// which is a *fluid* question and not a block-name one: a waterlogged
    /// slab answers yes. Derived from `fluid_at` so a world that already
    /// answers fluids properly answers this properly too.
    ///
    /// The tag has two members. This used to test `== "minecraft:water"`,
    /// which is the *source* fluid only — the doc comment above stated the
    /// rule correctly while the body underneath it did not. Live via
    /// `copy_waterlogged_from`, which is `DoublePlantBlock#copyWaterloggedFrom`
    /// and calls `LevelReader.isWaterAt` in the bytecode.
    fn is_water_at(&mut self, x: i32, y: i32, z: i32) -> bool {
        crate::feature::speleothem::is_water_fluid(&self.fluid_at(x, y, z))
    }
}

/// The feature could not be evaluated. Carries what stopped it.
#[derive(Debug, PartialEq, Eq)]
pub struct Unplaceable(pub String);

impl std::fmt::Display for Unplaceable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "simple_block cannot place `{}` yet", self.0)
    }
}

impl std::error::Error for Unplaceable {}

/// Blocks whose placement writes more than one block, or draws from a
/// different stream. Named rather than guessed at — see the module docs.
fn is_double_plant(block: &str) -> bool {
    matches!(
        block,
        "minecraft:sunflower"
            | "minecraft:lilac"
            | "minecraft:rose_bush"
            | "minecraft:peony"
            | "minecraft:tall_grass"
            | "minecraft:large_fern"
            | "minecraft:pitcher_plant"
    )
}

fn is_mossy_carpet(block: &str) -> bool {
    block == "minecraft:pale_moss_carpet"
}

/// `DoublePlantBlock#placeAt` — two blocks, lower then upper.
///
/// Both halves get their `waterlogged` from the position they land on
/// individually, not from the lower one, which is why this is two calls to
/// `copyWaterloggedFrom` rather than one state reused twice.
fn place_double_plant<W: SimpleBlockWorld + ?Sized>(
    world: &mut W,
    state: &str,
    origin: (i32, i32, i32),
) {
    let (x, y, z) = origin;
    let lower = copy_waterlogged_from(world, with_property(state, "half", "lower"), (x, y, z));
    world.set_block(x, y, z, &lower);
    let upper = copy_waterlogged_from(world, with_property(state, "half", "upper"), (x, y + 1, z));
    world.set_block(x, y + 1, z, &upper);
}

/// `DoublePlantBlock#copyWaterloggedFrom` — set `waterlogged` from the world
/// **only if the state has that property**. Most double plants do not, so
/// for them this is the identity and the world is never asked.
fn copy_waterlogged_from<W: SimpleBlockWorld + ?Sized>(
    world: &mut W,
    state: String,
    pos: (i32, i32, i32),
) -> String {
    if !state.contains("waterlogged=") {
        return state;
    }
    let wet = world.is_water_at(pos.0, pos.1, pos.2);
    with_property(&state, "waterlogged", if wet { "true" } else { "false" })
}

/// `BlockState#setValue` on a state written out as a string. The property
/// must already be present — every state this port places comes from a
/// datapack that lists all of them.
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

/// `SimpleBlockFeature#place`. Returns whether a block was placed.
///
/// # Errors
/// If the state provider needs behaviour the port lacks, or the block is one
/// whose placement is not ported.
pub fn place<W: SimpleBlockWorld + ?Sized, R: RandomSource>(
    world: &mut W,
    tags: &BlockTags,
    config: &SimpleBlockConfig,
    random: &mut R,
    origin: (i32, i32, i32),
) -> Result<bool, Unplaceable> {
    let (x, y, z) = origin;
    // The draw happens here, before anything is asked about the world.
    let state = config
        .to_place
        .state(world, tags, random, origin)
        .map_err(|NeedsBehaviour(what)| Unplaceable(what))?;

    // `PredicateWorld` already carries this — `would_survive` is the same
    // question the block predicate of that name asks, and answering it twice
    // would be two chances to answer it differently.
    match <W as PredicateWorld>::would_survive(world, &state, x, y, z) {
        Some(false) => {
            // `RUSTMC_TRACE_PATCH=1` counts survival refusals. A
            // `simple_block` that cannot survive writes nothing and costs no
            // draw, so a vegetation pass can consume vanilla's exact stream
            // and still lay almost nothing.
            if trace_patch() {
                let below = world.block_at(x, y - 1, z);
                eprintln!("SIMPLE survive_no `{state}` at ({x},{y},{z}) over `{below}`");
            }
            return Ok(false);
        }
        None => return Err(Unplaceable(format!("canSurvive({state})"))),
        Some(true) => {}
    }

    if trace_patch() {
        eprintln!("SIMPLE survived `{state}` at ({x},{y},{z})");
    }
    let block = state.split_once('[').map_or(state.as_str(), |(n, _)| n);
    if is_double_plant(block) {
        // The air check is vanilla's own guard, and it comes before the
        // write rather than being folded into it.
        return match world.is_empty_block(x, y + 1, z) {
            Some(false) => {
                if trace_patch() {
                    eprintln!("SIMPLE double_blocked `{block}` at ({x},{y},{z})");
                }
                Ok(false)
            }
            None => Err(Unplaceable(format!("isEmptyBlock above {block}"))),
            Some(true) => {
                place_double_plant(world, &state, origin);
                Ok(true)
            }
        };
    }
    if is_mossy_carpet(block) {
        // Not `set_block`: `placeAt` recomputes the state from the block's
        // default and can write a second block above. See
        // `crate::feature::mossy_carpet`.
        super::mossy_carpet::place_at(world, &state, origin)?;
        return Ok(true);
    }

    world.set_block(x, y, z, &state);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::legacy::LegacyRandomSource;
    use crate::rng::worldgen_random::WorldgenRandom;
    use std::collections::BTreeMap;

    struct Ground {
        blocks: BTreeMap<(i32, i32, i32), String>,
        survives: bool,
    }

    impl PredicateWorld for Ground {
        fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
            self.blocks
                .get(&(x, y, z))
                .cloned()
                .unwrap_or_else(|| "minecraft:air".to_string())
        }
        fn would_survive(&mut self, _: &str, _: i32, _: i32, _: i32) -> Option<bool> {
            Some(self.survives)
        }
    }

    impl SimpleBlockWorld for Ground {
        fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str) {
            self.blocks.insert((x, y, z), name.to_string());
        }
        fn is_empty_block(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
            Some(self.block_at(x, y, z) == "minecraft:air")
        }
    }

    fn world(survives: bool) -> Ground {
        Ground {
            blocks: BTreeMap::new(),
            survives,
        }
    }

    fn tags() -> BlockTags {
        BlockTags::from_entries(BTreeMap::new()).expect("tags")
    }

    /// Real default-state spellings, copied out of the block-props extract.
    fn states() -> super::super::state_spec::DefaultStateTable {
        super::super::state_spec::DefaultStateTable::from_pairs(&[
            ("minecraft:dandelion", "minecraft:dandelion"),
            ("minecraft:poppy", "minecraft:poppy"),
            (
                "minecraft:glow_lichen",
                "minecraft:glow_lichen[down=false,east=false,north=false,south=false,\
                 up=false,waterlogged=false,west=false]",
            ),
            ("minecraft:sunflower", "minecraft:sunflower[half=lower]"),
        ])
    }

    fn config(json: &str) -> SimpleBlockConfig {
        SimpleBlockConfig::parse(&serde_json::from_str(json).expect("json"), &states())
            .expect("parse")
    }

    /// The provider draws before the survival test, so a position that can
    /// hold nothing still costs the draw.
    #[test]
    fn the_provider_draws_even_where_nothing_can_be_placed() {
        let config = config(
            r#"{"to_place":{"type":"minecraft:weighted_state_provider","entries":[
                {"data":{"Name":"minecraft:dandelion"},"weight":1},
                {"data":{"Name":"minecraft:poppy"},"weight":1}]}}"#,
        );
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(5));
        let mut barren = world(false);
        assert!(!place(&mut barren, &tags(), &config, &mut random, (0, 0, 0)).expect("ok"));

        let mut reference = WorldgenRandom::new(LegacyRandomSource::new(5));
        reference.next_i32_bound(2);
        assert_eq!(
            random.next_i64(),
            reference.next_i64(),
            "one draw, taken before the survival test"
        );
    }

    /// A single-entry weighted list still draws — the case a short-circuit
    /// would get wrong.
    #[test]
    fn a_one_entry_weighted_list_still_draws() {
        let config = config(
            r#"{"to_place":{"type":"minecraft:weighted_state_provider","entries":[
                {"data":{"Name":"minecraft:dandelion"},"weight":1}]}}"#,
        );
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(9));
        let mut soil = world(true);
        assert!(place(&mut soil, &tags(), &config, &mut random, (0, 0, 0)).expect("ok"));
        assert_eq!(
            soil.blocks.get(&(0, 0, 0)).map(String::as_str),
            Some("minecraft:dandelion")
        );

        let mut reference = WorldgenRandom::new(LegacyRandomSource::new(9));
        reference.next_i32_bound(1);
        assert_eq!(random.next_i64(), reference.next_i64());
    }

    #[test]
    fn a_simple_provider_places_its_block() {
        let config = config(
            r#"{"to_place":{"type":"minecraft:simple_state_provider",
                            "state":{"Name":"minecraft:glow_lichen"}}}"#,
        );
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(1));
        let mut soil = world(true);
        assert!(place(&mut soil, &tags(), &config, &mut random, (4, 5, 6)).expect("ok"));
        assert_eq!(
            soil.blocks.get(&(4, 5, 6)).map(String::as_str),
            // The provider named the block; what lands is its default *state*,
            // every face off. See `state_spec`.
            Some(
                "minecraft:glow_lichen[down=false,east=false,north=false,south=false,\
                 up=false,waterlogged=false,west=false]"
            )
        );
    }

    /// A two-block plant writes **both** halves, and the upper one is not a
    /// copy of the lower — the `half` property differs.
    #[test]
    fn a_double_plant_writes_both_halves() {
        let config = config(
            r#"{"to_place":{"type":"minecraft:simple_state_provider",
                            "state":{"Name":"minecraft:sunflower","Properties":{"half":"lower"}}}}"#,
        );
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(1));
        let mut soil = world(true);
        assert!(place(&mut soil, &tags(), &config, &mut random, (0, 0, 0)).expect("placed"));
        assert_eq!(
            soil.blocks.get(&(0, 0, 0)).map(String::as_str),
            Some("minecraft:sunflower[half=lower]")
        );
        assert_eq!(
            soil.blocks.get(&(0, 1, 0)).map(String::as_str),
            Some("minecraft:sunflower[half=upper]"),
            "the upper half goes one block up, not a second lower half"
        );
    }

    /// With something in the way above, nothing is written at all — the air
    /// check is vanilla's guard and it precedes the whole two-block write.
    #[test]
    fn a_blocked_double_plant_writes_nothing() {
        let config = config(
            r#"{"to_place":{"type":"minecraft:simple_state_provider",
                            "state":{"Name":"minecraft:sunflower","Properties":{"half":"lower"}}}}"#,
        );
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(1));
        let mut soil = world(true);
        soil.blocks.insert((0, 1, 0), "minecraft:stone".to_string());
        assert!(!place(&mut soil, &tags(), &config, &mut random, (0, 0, 0)).expect("no refusal"));
        assert_eq!(
            soil.blocks.get(&(0, 0, 0)),
            None,
            "the lower half must not be written when the upper cannot be"
        );
    }

    /// A world with no survival rule for the block makes the feature refuse
    /// rather than place.
    #[test]
    fn an_unknown_survival_rule_is_refused() {
        struct NoRules(Ground);
        impl PredicateWorld for NoRules {
            fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
                self.0.block_at(x, y, z)
            }
            fn would_survive(&mut self, _: &str, _: i32, _: i32, _: i32) -> Option<bool> {
                None
            }
        }
        impl SimpleBlockWorld for NoRules {
            fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str) {
                self.0.set_block(x, y, z, name);
            }
            fn is_empty_block(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
                self.0.is_empty_block(x, y, z)
            }
        }
        let config = config(
            r#"{"to_place":{"type":"minecraft:simple_state_provider",
                            "state":{"Name":"minecraft:glow_lichen"}}}"#,
        );
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(1));
        let mut world = NoRules(world(true));
        let error =
            place(&mut world, &tags(), &config, &mut random, (0, 0, 0)).expect_err("should refuse");
        assert!(error.0.contains("canSurvive"), "{}", error.0);
    }

    #[test]
    fn every_simple_block_config_in_the_datapack_parses() {
        let dir = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../oracle-work/vanilla-26.2/generated-data/data/minecraft/worldgen/configured_feature"
        ));
        if !rustmc_data::fixture::available(dir, "datagen extract", "ci/vanilla-server.sh data") {
            return;
        }
        let Some(extract) = super::super::state_spec::extract_or_skip() else {
            return;
        };
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
            if json["type"] != "minecraft:simple_block" {
                continue;
            }
            total += 1;
            if let Ok(config) = SimpleBlockConfig::parse(&json["config"], &extract) {
                if config.is_supported() {
                    supported += 1;
                }
            }
        }
        assert_eq!(
            total, 32,
            "the datapack has thirty-two simple_block features"
        );
        // 29 of 32: the three noise-driven providers are not implemented.
        assert!(
            supported >= 29,
            "only {supported}/{total} simple_block configs parse and are supported"
        );
    }
}
