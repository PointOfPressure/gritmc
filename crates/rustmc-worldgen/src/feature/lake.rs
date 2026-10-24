//! The lake feature — a pool carved into the ground and filled.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `LakeFeature`, `LakeFeature.Configuration`.
//!
//! # Four passes over one boolean mask
//!
//! The whole feature is a 16x16x8 `boolean[2048]`, built by unioning four to
//! seven random ellipsoids, and then read four times:
//!
//! 1. **validity** — every cell just *outside* the mask is checked, and a
//!    single failure abandons the entire lake.
//! 2. **fill** — every cell inside becomes fluid below y=4 and cave air above.
//! 3. **barrier** — the shell is lined with the barrier block, but only half
//!    of it above y=4, so the roof is ragged rather than a dome.
//! 4. **ice** — water lakes freeze at y=4 in cold biomes.
//!
//! The mask index is `(x * 16 + z) * 8 + y`, which is **not** the layout the
//! rest of this crate uses. Getting it wrong transposes the lake rather than
//! breaking it, and a transposed lake still looks like a lake.
//!
//! # Its air is `cave_air`
//!
//! `LakeFeature.AIR` is a static initialised to `Blocks.CAVE_AIR`, not
//! `Blocks.AIR`. The two are indistinguishable in play and completely
//! distinguishable in a region file, which is exactly the class of difference
//! ORACLE.md rule 11 exists for.
//!
//! # The barrier's coin is only spent above y=4
//!
//! ```text
//! if (y >= 4 && random.nextInt(2) == 0) continue;
//! ```
//!
//! Below y=4 there is no draw at all. So the number of draws this feature
//! takes depends on the *shape* of the mask it just built — which means a
//! port that got the ellipsoids right and the shell test wrong would consume
//! a different number of values and move every feature after it.

// Read once, not per lake. See `crate::trace_switch`.
crate::trace_switch_value!(trace_lake, "RUSTMC_TRACE_LAKE");
use super::block_predicate::{BlockPredicate, PredicateWorld};
use super::state_provider::StateProvider;
use super::state_spec::DefaultStates;
use crate::rng::RandomSource;
use crate::tags::BlockTags;

/// `LakeFeature.AIR` — **cave** air. See the module docs.
const AIR: &str = "minecraft:cave_air";

/// The mask is 16 wide, 16 deep and 8 tall.
const SIZE_X: i32 = 16;
const SIZE_Z: i32 = 16;
const SIZE_Y: i32 = 8;
/// The waterline: fluid below, air above.
const SURFACE_Y: i32 = 4;

/// `LakeFeature.Configuration`.
#[derive(Clone, Debug, PartialEq)]
pub struct LakeConfig {
    pub fluid: StateProvider,
    pub barrier: StateProvider,
    pub can_place_feature: BlockPredicate,
    pub can_replace_with_air_or_fluid: BlockPredicate,
    pub can_replace_with_barrier: BlockPredicate,
}

impl LakeConfig {
    /// # Errors
    /// If a field is missing or names something unimplemented.
    pub fn parse(config: &serde_json::Value, states: &dyn DefaultStates) -> Result<Self, String> {
        let provider = |name: &str| -> Result<StateProvider, String> {
            StateProvider::parse(
                config
                    .get(name)
                    .ok_or_else(|| format!("lake config has no `{name}`"))?,
                states,
            )
        };
        let predicate = |name: &str| -> Result<BlockPredicate, String> {
            BlockPredicate::parse(
                config
                    .get(name)
                    .ok_or_else(|| format!("lake config has no `{name}`"))?,
            )
        };
        Ok(Self {
            fluid: provider("fluid")?,
            barrier: provider("barrier")?,
            can_place_feature: predicate("can_place_feature")?,
            can_replace_with_air_or_fluid: predicate("can_replace_with_air_or_fluid")?,
            can_replace_with_barrier: predicate("can_replace_with_barrier")?,
        })
    }

    #[must_use]
    pub fn is_supported(&self) -> bool {
        self.fluid.is_supported()
            && self.barrier.is_supported()
            && self.can_place_feature.is_supported()
            && self.can_replace_with_air_or_fluid.is_supported()
            && self.can_replace_with_barrier.is_supported()
    }
}

/// The world a lake reads and writes.
pub trait LakeWorld: PredicateWorld {
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str);
    /// `level.getMinY()`.
    fn min_y(&self) -> i32;
    /// `BlockState#liquid()` — the block *is* a fluid, not merely waterlogged.
    fn is_liquid(&mut self, x: i32, y: i32, z: i32) -> Option<bool>;
    /// `Biome#shouldFreeze(level, pos, false)`, for the ice pass.
    fn should_freeze(&mut self, x: i32, y: i32, z: i32) -> bool;
    /// `Feature#markAboveForPostProcessing` — the same two-step walk the disk
    /// uses, and required here for the same reason it is required there: a
    /// no-op default is indistinguishable from a port that works, and this
    /// feature's marks were dropped for as long as the default existed.
    fn mark_above_for_post_processing(&mut self, x: i32, y: i32, z: i32);
}

/// The feature could not be evaluated.
#[derive(Debug, PartialEq, Eq)]
pub struct NeedsRules(pub String);

fn index(x: i32, y: i32, z: i32) -> usize {
    usize::try_from((x * SIZE_X + z) * SIZE_Y + y).expect("a positive index")
}

/// Whether a cell is outside the mask but touching it.
///
/// The six neighbour tests are guarded on the mask's own bounds, not the
/// world's: a cell on the edge of the 16x16x8 box simply has fewer
/// neighbours, and reading past the array would be a different lake.
fn is_shell(mask: &[bool], x: i32, y: i32, z: i32) -> bool {
    if mask[index(x, y, z)] {
        return false;
    }
    (x < SIZE_X - 1 && mask[index(x + 1, y, z)])
        || (x > 0 && mask[index(x - 1, y, z)])
        || (z < SIZE_Z - 1 && mask[index(x, y, z + 1)])
        || (z > 0 && mask[index(x, y, z - 1)])
        || (y < SIZE_Y - 1 && mask[index(x, y + 1, z)])
        || (y > 0 && mask[index(x, y - 1, z)])
}

/// `blockState.getFluidState().is(FluidTags.WATER)` on the state the **fluid
/// provider** returned — the lake's own fluid, and nothing about the world.
///
/// The tag holds `water` and `flowing_water`, so both answer yes; a block
/// with no fluid answers no. A state whose fluid comes from *waterlogging*
/// cannot be classified from its name — that needs the block-property table,
/// which a lake has no other reason to hold — so it is refused rather than
/// guessed. No lake in the datapack has one; if one ever does, this says so
/// instead of quietly freezing or quietly not.
///
/// # Errors
/// If the state's fluid cannot be decided from its name.
fn fluid_is_water(state: &str) -> Result<bool, NeedsRules> {
    let plain = state.split_once('[').map_or(state, |(n, _)| n);
    match plain {
        "minecraft:water" | "minecraft:flowing_water" => Ok(true),
        "minecraft:lava" | "minecraft:flowing_lava" | "minecraft:air" => Ok(false),
        other => Err(NeedsRules(format!(
            "a lake whose fluid is `{other}` — its fluid state is not decidable \
             from the block name, and LakeFeature's ice pass is gated on it"
        ))),
    }
}

/// `LakeFeature#place`.
///
/// # Errors
/// If the world cannot answer a block question.
pub fn place<W: LakeWorld + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    config: &LakeConfig,
    random: &mut dyn RandomSource,
    origin: (i32, i32, i32),
) -> Result<bool, NeedsRules> {
    // No draws before this: a lake too close to the world floor is refused
    // for free, which is the one early exit that costs nothing.
    if origin.1 <= world.min_y() + 4 {
        return Ok(false);
    }
    // The origin moves to the box's minimum corner. Everything after this is
    // relative to *that*, not to where the placement chain put it.
    let origin = (origin.0 - 8, origin.1 - 4, origin.2 - 8);

    let mut mask = vec![false; 2048];
    let rounds = random.next_i32_bound(4) + 4;
    for _ in 0..rounds {
        // Six draws per round, in this order. The sizes come first and the
        // centres are computed from them, so a port that drew the centres
        // first would build different ellipsoids from the same stream.
        let size_x = random.next_f64() * 6.0 + 3.0;
        let size_y = random.next_f64() * 4.0 + 2.0;
        let size_z = random.next_f64() * 6.0 + 3.0;
        let center_x = random.next_f64() * (16.0 - size_x - 2.0) + 1.0 + size_x / 2.0;
        let center_y = random.next_f64() * (8.0 - size_y - 4.0) + 2.0 + size_y / 2.0;
        let center_z = random.next_f64() * (16.0 - size_z - 2.0) + 1.0 + size_z / 2.0;

        // 1..15, 1..15, 1..7 — the box's outermost shell is never part of an
        // ellipsoid, which is what leaves room for the barrier.
        for x in 1..SIZE_X - 1 {
            for z in 1..SIZE_Z - 1 {
                for y in 1..SIZE_Y - 1 {
                    let dx = (f64::from(x) - center_x) / (size_x / 2.0);
                    let dy = (f64::from(y) - center_y) / (size_y / 2.0);
                    let dz = (f64::from(z) - center_z) / (size_z / 2.0);
                    if dx * dx + dy * dy + dz * dz < 1.0 {
                        mask[index(x, y, z)] = true;
                    }
                }
            }
        }
    }

    let fluid = config
        .fluid
        .state(world, tags, random, origin)
        .map_err(|e| NeedsRules(e.0))?;

    // ---- pass 1: validity. One failure abandons the whole lake. ----
    //
    // `RUSTMC_TRACE_LAKE=1` names the cell and the reason. A lake that
    // abandons writes nothing, so from outside it is indistinguishable from
    // a lake that was never attempted — and the three ways it can abandon
    // are three different bugs.
    let trace = trace_lake().is_some();
    // Every distinct state pass 1 saw, split at the waterline, for the lakes
    // that *survive* it.
    //
    // The abandon reasons alone answer "why did this lake stop" and cannot
    // answer the harder question, which is why a lake vanilla stops does
    // **not** stop here. That needs the states the surviving pass saw, because
    // the disagreement has to be one block whose `isSolid()` or `liquid()` we
    // answer differently — vanilla's shell loop and ours are the same three
    // tests in the same order, read from the bytecode. Seed 20260722 chunk
    // (58,63): vanilla's lake at (942,-18,1010) reports `placed=false,
    // changes=0` and ours writes.
    let mut seen_below: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut seen_above: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    // `RUSTMC_TRACE_LAKE=cells` additionally lists every shell cell with its
    // position. The distinct-state form answers "could anything here have
    // abandoned it"; when the answer is no and vanilla abandoned anyway, the
    // question becomes *which cell*, and that needs coordinates.
    let cells = trace_lake() == Some("cells");
    let mut shell: Vec<(i32, i32, i32, i32, String)> = Vec::new();
    for x in 0..SIZE_X {
        for z in 0..SIZE_Z {
            for y in 0..SIZE_Y {
                if !is_shell(&mask, x, y, z) {
                    continue;
                }
                let pos = (origin.0 + x, origin.1 + y, origin.2 + z);
                let state = world.block_at(pos.0, pos.1, pos.2);
                if trace {
                    if y >= SURFACE_Y {
                        seen_above.insert(state.clone());
                    } else {
                        seen_below.insert(state.clone());
                    }
                    if cells {
                        shell.push((pos.0, pos.1, pos.2, y, state.clone()));
                    }
                }
                // Above the waterline, a liquid neighbour would drain it.
                if y >= SURFACE_Y
                    && world
                        .is_liquid(pos.0, pos.1, pos.2)
                        .ok_or_else(|| NeedsRules("liquid".to_string()))?
                {
                    if trace {
                        eprintln!(
                            "LAKE abandon at ({},{},{}) reason=liquid-above-waterline block={state}",
                            pos.0, pos.1, pos.2
                        );
                    }
                    return Ok(false);
                }
                // Below it, the wall has to be solid — or already this lake's
                // own fluid, which is how two lakes merge instead of one
                // refusing next to the other.
                // `isSolid()`, **not** `isSolidRender()`. Both exist on
                // `BlockState`, this file called the wrong one until
                // 2026-07-28, and the error string above it already said
                // `isSolid` — the intent was right and the call was not.
                // The bytecode is `invokevirtual BlockState.isSolid:()Z` in
                // both of `LakeFeature.place`'s passes.
                if y < SURFACE_Y
                    && !world
                        .is_solid_at(pos.0, pos.1, pos.2)
                        .ok_or_else(|| NeedsRules("isSolid".to_string()))?
                    && state != fluid
                {
                    if trace {
                        eprintln!(
                            "LAKE abandon at ({},{},{}) reason=wall-not-solid block={state}",
                            pos.0, pos.1, pos.2
                        );
                    }
                    return Ok(false);
                }
                if !config
                    .can_place_feature
                    .test(world, tags, pos)
                    .map_err(|e| NeedsRules(e.0))?
                {
                    if trace {
                        eprintln!(
                            "LAKE abandon at ({},{},{}) reason=can_place_feature block={state}",
                            pos.0, pos.1, pos.2
                        );
                    }
                    return Ok(false);
                }
            }
        }
    }
    if trace {
        eprintln!(
            "LAKE survives pass 1 at origin ({},{},{})  below-waterline states {:?}  above {:?}",
            origin.0 + 8,
            origin.1 + 4,
            origin.2 + 8,
            seen_below,
            seen_above
        );
        if cells {
            for (px, py, pz, ly, st) in &shell {
                eprintln!(
                    "LAKE cell ({px},{py},{pz}) local_y={ly} {} {st}",
                    if *ly >= SURFACE_Y { "above" } else { "below" }
                );
            }
        }
    }

    // ---- pass 2: fill ----
    for x in 0..SIZE_X {
        for z in 0..SIZE_Z {
            for y in 0..SIZE_Y {
                if !mask[index(x, y, z)] {
                    continue;
                }
                let pos = (origin.0 + x, origin.1 + y, origin.2 + z);
                if !config
                    .can_replace_with_air_or_fluid
                    .test(world, tags, pos)
                    .map_err(|e| NeedsRules(e.0))?
                {
                    continue;
                }
                let above_water = y >= SURFACE_Y;
                let state = if above_water { AIR } else { fluid.as_str() };
                world.set_block(pos.0, pos.1, pos.2, state);
                if above_water {
                    // `scheduleTick(pos, AIR.getBlock(), 0)` follows here and
                    // is not ported: air has no tick. The mark is, and used
                    // not to be — see the barrier pass below for why that
                    // mattered.
                    world.mark_above_for_post_processing(pos.0, pos.1, pos.2);
                }
            }
        }
    }

    // ---- pass 3: the barrier shell ----
    let barrier = config
        .barrier
        .state(world, tags, random, origin)
        .map_err(|e| NeedsRules(e.0))?;
    if barrier != "minecraft:air" {
        for x in 0..SIZE_X {
            for z in 0..SIZE_Z {
                for y in 0..SIZE_Y {
                    if !is_shell(&mask, x, y, z) {
                        continue;
                    }
                    // The coin, and only above the waterline. Below it every
                    // shell cell is lined; above it half are, which is what
                    // makes the roof ragged. The draw happens *before* the
                    // block is read, so a solid check that short-circuited it
                    // would spend a different number of values.
                    if y >= SURFACE_Y && random.next_i32_bound(2) == 0 {
                        continue;
                    }
                    let pos = (origin.0 + x, origin.1 + y, origin.2 + z);
                    // `isSolid()`, as in pass 1 — same fault, same fix.
                    if !world
                        .is_solid_at(pos.0, pos.1, pos.2)
                        .ok_or_else(|| NeedsRules("isSolid".to_string()))?
                    {
                        continue;
                    }
                    if !config
                        .can_replace_with_barrier
                        .test(world, tags, pos)
                        .map_err(|e| NeedsRules(e.0))?
                    {
                        continue;
                    }
                    world.set_block(pos.0, pos.1, pos.2, &barrier);
                    // Vanilla marks the two positions **above** each barrier
                    // block it lays. Over a `sulfur_pool` — a lake whose
                    // barrier is `minecraft:sulfur` — this one call is the
                    // largest block of the post-processing marks vanilla
                    // makes and we did not; see `docs/ORACLE.md` §24.
                    world.mark_above_for_post_processing(pos.0, pos.1, pos.2);
                }
            }
        }
    }

    // ---- pass 4: ice, for water lakes in cold biomes ----
    if fluid_is_water(&fluid)? {
        for x in 0..SIZE_X {
            for z in 0..SIZE_Z {
                let pos = (origin.0 + x, origin.1 + SURFACE_Y, origin.2 + z);
                if world.should_freeze(pos.0, pos.1, pos.2)
                    && config
                        .can_replace_with_air_or_fluid
                        .test(world, tags, pos)
                        .map_err(|e| NeedsRules(e.0))?
                {
                    world.set_block(pos.0, pos.1, pos.2, "minecraft:ice");
                }
            }
        }
    }

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
        /// `Biome#shouldFreeze` — a constant here, so a test can put the
        /// lake in a cold biome without a climate sampler.
        freezing: bool,
        /// Every position handed to `markAboveForPostProcessing`, in call
        /// order. The walk itself belongs to the production world; what a
        /// unit test can check is that the calls happen at all.
        marks: Vec<(i32, i32, i32)>,
    }

    impl PredicateWorld for Ground {
        fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
            self.blocks
                .get(&(x, y, z))
                .cloned()
                .unwrap_or_else(|| "minecraft:stone".to_string())
        }
        fn solid_at(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
            Some(self.block_at(x, y, z) == "minecraft:stone")
        }
        /// The lake reads `isSolid()`, not `isSolidRender()`. Stone is both,
        /// so this test world answers them the same — which is exactly why
        /// the unit tests stayed green while the real generator built lakes
        /// vanilla abandons.
        fn is_solid_at(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
            Some(self.block_at(x, y, z) == "minecraft:stone")
        }
        fn air_at(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
            Some(self.block_at(x, y, z).ends_with("air"))
        }
    }

    impl LakeWorld for Ground {
        fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
            self.blocks.insert((x, y, z), state.to_string());
        }
        fn min_y(&self) -> i32 {
            -64
        }
        fn is_liquid(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
            let b = self.block_at(x, y, z);
            Some(b.starts_with("minecraft:water") || b.starts_with("minecraft:lava"))
        }
        fn should_freeze(&mut self, _: i32, _: i32, _: i32) -> bool {
            self.freezing
        }
        fn mark_above_for_post_processing(&mut self, x: i32, y: i32, z: i32) {
            self.marks.push((x, y, z));
        }
    }

    fn tags() -> BlockTags {
        BlockTags::from_entries(BTreeMap::new()).expect("tags")
    }

    fn states() -> crate::feature::state_spec::DefaultStateTable {
        crate::feature::state_spec::DefaultStateTable::from_pairs(&[
            ("minecraft:lava", "minecraft:lava[level=0]"),
            ("minecraft:stone", "minecraft:stone"),
        ])
    }

    fn config() -> LakeConfig {
        LakeConfig::parse(
            &serde_json::from_str(
                r#"{"barrier":{"type":"minecraft:simple_state_provider",
                               "state":{"Name":"minecraft:stone"}},
                    "can_place_feature":{"type":"minecraft:true"},
                    "can_replace_with_air_or_fluid":{"type":"minecraft:true"},
                    "can_replace_with_barrier":{"type":"minecraft:true"},
                    "fluid":{"type":"minecraft:simple_state_provider",
                             "state":{"Name":"minecraft:lava",
                                      "Properties":{"level":"0"}}}}"#,
            )
            .expect("json"),
            &states(),
        )
        .expect("parse")
    }

    /// A water config, for the ice pass.
    fn water_config() -> LakeConfig {
        LakeConfig::parse(
            &serde_json::from_str(
                r#"{"barrier":{"type":"minecraft:simple_state_provider",
                               "state":{"Name":"minecraft:stone"}},
                    "can_place_feature":{"type":"minecraft:true"},
                    "can_replace_with_air_or_fluid":{"type":"minecraft:true"},
                    "can_replace_with_barrier":{"type":"minecraft:true"},
                    "fluid":{"type":"minecraft:simple_state_provider",
                             "state":{"Name":"minecraft:water",
                                      "Properties":{"level":"0"}}}}"#,
            )
            .expect("json"),
            &crate::feature::state_spec::DefaultStateTable::from_pairs(&[
                ("minecraft:water", "minecraft:water[level=0]"),
                ("minecraft:stone", "minecraft:stone"),
            ]),
        )
        .expect("parse")
    }

    /// The ice pass is gated on the **fluid the config names**, not on
    /// anything in the world.
    ///
    /// The port used to read the world at the box's minimum corner and
    /// freeze if *that* was water. Nothing in `LakeFeature` reads that
    /// block: the test is `blockState.getFluidState().is(FluidTags.WATER)`
    /// on the state the fluid provider just returned. So a lava lake dug
    /// beside water froze — ice blocks in a lava lake, from a world read
    /// vanilla never makes.
    #[test]
    fn a_lava_lake_beside_water_does_not_freeze() {
        let mut world = Ground {
            blocks: BTreeMap::new(),
            freezing: true,
            marks: Vec::new(),
        };
        // The old gate's input: the box's minimum corner, which `place`
        // reaches as origin - (8, 4, 8).
        world
            .blocks
            .insert((-8, 36, -8), "minecraft:water[level=0]".to_string());
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(11));
        assert!(place(&mut world, &tags(), &config(), &mut random, (0, 40, 0)).expect("ok"));
        assert_eq!(
            world
                .blocks
                .values()
                .filter(|v| v.as_str() == "minecraft:ice")
                .count(),
            0,
            "a lava lake has no ice pass, whatever happens to be next to it"
        );
    }

    /// And a water lake in a cold biome still freezes, so the gate was
    /// tightened rather than removed.
    #[test]
    fn a_water_lake_in_a_cold_biome_freezes() {
        let mut world = Ground {
            blocks: BTreeMap::new(),
            freezing: true,
            marks: Vec::new(),
        };
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(11));
        assert!(place(
            &mut world,
            &tags(),
            &water_config(),
            &mut random,
            (0, 40, 0)
        )
        .expect("ok"));
        assert!(
            world.blocks.values().any(|v| v.as_str() == "minecraft:ice"),
            "the waterline of a water lake freezes"
        );
    }

    /// A fluid whose fluid state is not decidable from its name is refused,
    /// not guessed at. Draw counts are behaviour and so is a block written.
    #[test]
    fn an_undecidable_fluid_is_refused() {
        assert_eq!(fluid_is_water("minecraft:water[level=0]"), Ok(true));
        assert_eq!(fluid_is_water("minecraft:flowing_water[level=3]"), Ok(true));
        assert_eq!(fluid_is_water("minecraft:lava[level=0]"), Ok(false));
        assert!(
            fluid_is_water("minecraft:water_cauldron[level=3]").is_err(),
            "a name that merely starts with `minecraft:water` is not water — \
             the old test said it was"
        );
        assert!(fluid_is_water("minecraft:seagrass").is_err());
    }

    /// Too near the floor is refused before a single draw.
    #[test]
    fn a_lake_at_the_world_floor_costs_nothing() {
        let mut world = Ground {
            blocks: BTreeMap::new(),
            freezing: false,
            marks: Vec::new(),
        };
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(3));
        let mut probe = WorldgenRandom::new(LegacyRandomSource::new(3));
        assert!(!place(&mut world, &tags(), &config(), &mut random, (0, -62, 0)).expect("ok"));
        assert_eq!(random.next_i64(), probe.next_i64());
    }

    /// In solid stone a lake digs a pool: lava below the waterline, cave air
    /// above it, and the air is **cave** air.
    #[test]
    fn a_lake_in_stone_fills_with_fluid_and_cave_air() {
        let mut world = Ground {
            blocks: BTreeMap::new(),
            freezing: false,
            marks: Vec::new(),
        };
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(11));
        assert!(place(&mut world, &tags(), &config(), &mut random, (0, 40, 0)).expect("ok"));

        let lava = world
            .blocks
            .values()
            .filter(|v| v.as_str() == "minecraft:lava[level=0]")
            .count();
        let cave_air = world
            .blocks
            .values()
            .filter(|v| v.as_str() == "minecraft:cave_air")
            .count();
        assert!(lava > 0, "the pool has fluid in it");
        assert!(cave_air > 0, "and a space above it");
        assert_eq!(
            world
                .blocks
                .values()
                .filter(|v| v.as_str() == "minecraft:air")
                .count(),
            0,
            "vanilla's LakeFeature.AIR is CAVE_AIR, and the two are \
             indistinguishable in play and not in a region file"
        );
    }

    /// The mask index is `(x * 16 + z) * 8 + y`. A transposed lake still
    /// looks like a lake, so this is a test rather than a comment.
    #[test]
    fn the_mask_index_is_x_major_then_z_then_y() {
        assert_eq!(index(0, 0, 0), 0);
        assert_eq!(index(0, 1, 0), 1, "y is the fastest axis");
        assert_eq!(index(0, 0, 1), 8, "then z");
        assert_eq!(index(1, 0, 0), 128, "and x is the slowest");
        assert_eq!(index(15, 7, 15), 2047, "the last cell is the last index");
    }

    /// A cell inside the mask is never a shell cell, and one touching it is.
    #[test]
    fn the_shell_is_outside_the_mask_and_touching_it() {
        let mut mask = vec![false; 2048];
        mask[index(5, 3, 5)] = true;
        assert!(!is_shell(&mask, 5, 3, 5), "inside is not shell");
        assert!(is_shell(&mask, 6, 3, 5));
        assert!(is_shell(&mask, 5, 4, 5));
        assert!(!is_shell(&mask, 7, 3, 5), "two away is not touching");
    }

    #[test]
    fn every_lake_config_in_the_datapack_parses() {
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
        for entry in std::fs::read_dir(dir).expect("readable") {
            let path = entry.expect("entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let json: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).expect("readable"))
                    .expect("json");
            if json["type"] != "minecraft:lake" {
                continue;
            }
            let config = LakeConfig::parse(&json["config"], &extract)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert!(config.is_supported(), "{}", path.display());
            seen += 1;
        }
        // One top-level lake in the whole datapack. The others are written
        // *inline* inside `sequence` configs — `sulfur_pool` is a lake
        // followed by what decorates it — so a directory scan finds one and
        // the world contains several.
        assert_eq!(seen, 1, "the datapack has one top-level lake feature");
    }
}
