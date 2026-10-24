//! `FlowingFluid`, as `LevelChunk#postProcessGeneration` drives it.
//!
//! Reference: `net.minecraft.world.level.material.FlowingFluid` and
//! `WaterFluid` in the 26.2 server jar, read with `javap -p -c -constants`.
//!
//! # Why a generator needs this at all
//!
//! Because a saved region file is not what the generator wrote. It is what
//! the generator wrote **plus one synchronous pass of block and fluid
//! logic**, and that pass is part of chunk generation rather than part of the
//! game loop — `docs/ORACLE.md` §18 has the two independent measurements that
//! establish it. Vanilla's own r=7 reference holds 26 non-source water blocks
//! that no feature writes; flow logic computed them, at generation time, and
//! they count against us.
//!
//! # Scope: both fluids, since 2026-08-10
//!
//! It was water only, deliberately, and the reason it gave was a measurement:
//! "the r=7 reference has **zero** non-source lava". That was true of r=7 and
//! it is false of r=16, which holds **24** `lava[level=2]` blocks that both
//! twins agree on and that we had as air. The decline was not a dead branch
//! being flattered by a window that could not see it — `skipped_other_fluid`
//! read **5,736** marked lava positions over that window, so the omission was
//! live, counted and exactly as large as its own instrument said. The counter
//! did its job; the window it was read on was too small.
//!
//! [`Kind`] carries everything the two fluids disagree about. It is three
//! constants and two game rules, and nothing else in this file branches on
//! which fluid is flowing.
//!
//! # The one approximation, and how it is policed
//!
//! `canPassThroughWall` asks three questions of a *collision shape*: is it
//! exactly `Shapes.block()`, is it exactly `Shapes.empty()`, and — when
//! neither — does `Shapes.mergedFaceOccludes` cover the shared face. The
//! first two are answered here from `collisionFullFaces`, which is all six
//! faces full and no face full respectively. The third is approximated by
//! "either side's face is full on its own", which can differ from vanilla
//! only when two partial shapes meet and neither face is full alone but
//! their union covers the face.
//!
//! That branch is counted, not hoped about: [`Stats::merged_face_branch`] is
//! incremented every time a decision reaches it. A run whose count is zero
//! never exercised the approximation, and the approximation is then not part
//! of that measurement's result. Rule 7 — when a measurement clears every
//! candidate, suspect the measurement — is the reason this is a counter and
//! not a comment.

use crate::canvas::Canvas;
use crate::stage::post_process::{opposite, TagFacts, OFFSET};

/// Which of the two spreading fluids a state carries.
///
/// Everything `FlowingFluid`'s subclasses override that can move a block
/// during generation is here. `getTickDelay` and `getSpreadDelay` are timing
/// and there is no tick queue in a generator; `randomTick` is fire spread and
/// does not run in `postProcessGeneration`; `beforeDestroyingBlock` only
/// drops items.
///
/// `isFastLava` is `LevelReader.environmentAttributes().getDimensionValue(
/// EnvironmentAttributes.FAST_LAVA)` — a dimension property, false outside
/// the nether — so every lava figure below is the overworld branch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Water,
    Lava,
}

impl Kind {
    /// `getDropOff(LevelReader)`. `WaterFluid` is `iconst_1`; `LavaFluid` is
    /// `isFastLava ? 1 : 2`.
    ///
    /// **This constant is the whole of `level=2`.** `getLegacyLevel` writes
    /// `8 - amount`, a source has amount 8, and one step of overworld lava is
    /// `8 - 2 = 6`, so the first ring outside a lava source is `level=2` —
    /// which is the state all 24 of the r=16 window's flowing lava blocks
    /// carry, measured before this was implemented.
    const fn drop_off(self) -> i32 {
        match self {
            Self::Water => 1,
            Self::Lava => 2,
        }
    }

    /// `getSlopeFindDistance(LevelReader)`. `WaterFluid` is `iconst_4`;
    /// `LavaFluid` is `isFastLava ? 4 : 2`.
    const fn slope_find_distance(self) -> i32 {
        match self {
            Self::Water => 4,
            Self::Lava => 2,
        }
    }

    /// The `LiquidBlock` a `FluidState` of this kind becomes.
    const fn block(self) -> &'static str {
        match self {
            Self::Water => "minecraft:water",
            Self::Lava => "minecraft:lava",
        }
    }

    /// `canConvertToSource(ServerLevel)`, which is a game rule read in both
    /// subclasses and nothing else. See [`FluidRules`].
    const fn can_convert_to_source(self, rules: FluidRules) -> bool {
        match self {
            Self::Water => rules.water_source_conversion,
            Self::Lava => rules.lava_source_conversion,
        }
    }
}

/// `FlowingFluid#getSlopeDistance` seeds its best distance with
/// `sipush 1000`, and `getSpread` seeds its minimum with the same. It is a
/// sentinel, not a bound: nothing compares against it except `<`.
const NO_SLOPE: i32 = 1000;

/// `Direction.Plane.HORIZONTAL`, as `Direction` ordinals.
///
/// The *set* is what matters and not the order. Every vanilla loop over this
/// plane either accumulates a max (`getNewLiquid`), a count
/// (`sourceNeighborCount`), a min (`getSlopeDistance`) or a min-set
/// (`getSpread`, whose `clear()` can only ever drop entries that are strictly
/// worse), so none of them is order-dependent. The one loop that *is* — the
/// walk in `spreadToSides` that actually writes — iterates an `EnumMap`,
/// which is in `Direction` ordinal order, and that is this array.
const HORIZONTAL: [usize; 4] = [2, 3, 4, 5];

/// `Direction.DOWN` and `Direction.UP` as `OFFSET` indices.
const DOWN: usize = 0;
const UP: usize = 1;

/// `NeighborUpdater.UPDATE_ORDER` — `{WEST, EAST, DOWN, UP, NORTH, SOUTH}`, as
/// `OFFSET` indices.
///
/// This is the order `CollectingNeighborUpdater.MultiNeighborUpdate` hands the
/// six neighbours to `neighborChanged`, and it is **not** `Direction.values()`.
/// It cannot change this pass's result — see [`World::run_neighbour_updates`]
/// for the invariant that makes the converted set order-free, and
/// `a_cascade_converts_the_same_sheet_from_either_end` for the test that pins
/// it. It is used anyway because "it cannot matter here" is a claim about
/// today's rules, and the cheapest place to be wrong about it is the first
/// rule that is added.
const UPDATE_ORDER: [usize; 6] = [4, 5, DOWN, UP, 2, 3];

/// `Direction#getName`, for the tape only.
const DIRECTION_NAME: [&str; 6] = ["down", "up", "north", "south", "west", "east"];

/// A per-position record of what the pass did, and why.
///
/// # Why this exists, and why it is scored against the reference and not a
/// golden
///
/// Everything this pass writes is water, lava, obsidian, cobblestone or
/// stone, and it is the last thing to touch those blocks before the region is
/// saved. So a vanilla reference region **is** a printout of vanilla's own
/// run of it: a saved obsidian is a conversion vanilla made, and a saved lava
/// under water is one it did not. That makes the tape scoreable position-by-position
/// against a reference nobody has to instrument, with no JVM in the loop —
/// the same argument the ruined-portal apron counter runs on.
///
/// It is a tape rather than a counter because the count was never the problem.
/// This pass reached the right *kind* of answer with the wrong *extent* for as
/// long as it has existed, and a total would have agreed with itself all the
/// way through. `ci/fluid-tape.sh` scores it.
///
/// Enabled by `RUSTMC_FLUID_TAPE=<path>`; disabled it costs one `bool` test
/// per event. **Take it at `RUSTMC_WORKERS=1`**: chunks append under a lock so
/// no line is ever torn, but two chunks' runs interleave and the tape's whole
/// value is that a cascade reads in order.
#[derive(Debug, Default)]
pub struct Tape {
    enabled: bool,
    lines: Vec<String>,
}

impl Tape {
    /// A tape that records nothing, for every caller that does not want one.
    #[must_use]
    pub const fn off() -> Self {
        Self {
            enabled: false,
            lines: Vec::new(),
        }
    }

    /// A tape that records, for the harnesses and the tests.
    #[must_use]
    pub const fn on() -> Self {
        Self {
            enabled: true,
            lines: Vec::new(),
        }
    }

    fn record(&mut self, event: &str, position: (i32, i32, i32), detail: &str) {
        if !self.enabled {
            return;
        }
        let (x, y, z) = position;
        self.lines.push(format!("{event} {x} {y} {z} {detail}"));
    }

    /// The recorded lines, oldest first.
    #[must_use]
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// Whether anything is being recorded at all. A scorer that reads an empty
    /// tape has to be able to tell "the pass did nothing" from "the tape was
    /// never switched on", which is this project's most expensive kind of
    /// zero.
    #[must_use]
    pub const fn is_on(&self) -> bool {
        self.enabled
    }
}

/// The per-block-state facts the flow rules ask that a name cannot answer.
pub trait FluidFacts {
    /// `BlockState#blocksMotion`.
    fn blocks_motion(&self, state: &str) -> bool;
    /// `BlockState#isSolid` — the legacy flag, **not** `isSolidRender`.
    /// `getNewLiquid` tests it to decide whether a source may form.
    fn is_solid(&self, state: &str) -> bool;
    /// `Block.isFaceFull(state.getCollisionShape(..), dir)` per `OFFSET`
    /// index.
    fn collision_full_faces(&self, state: &str) -> [bool; 6];
}

/// What one run of the pass did, and what it declined to do.
///
/// Every field exists to make a zero readable. A pass that changes nothing
/// and a pass that never ran write the same region file, and this project has
/// been caught by that distinction more than once.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Marked positions whose block held water when the pass reached them.
    pub ticked: usize,
    /// Marked positions whose block held **lava** when the pass reached them.
    ///
    /// This field replaced `skipped_other_fluid`, which counted the same
    /// positions and then declined them. It is kept as a count rather than
    /// dropped because the two numbers have to be comparable across the
    /// change: 5,736 declined on the r=16 window before, and a `ticked_lava`
    /// that did not land near it would mean the marks moved rather than the
    /// behaviour.
    pub ticked_lava: usize,
    /// `setBlock` calls the flow made.
    pub writes: usize,
    /// Decisions that reached the approximated `mergedFaceOccludes` branch of
    /// `canPassThroughWall`. Zero means the approximation was never used.
    pub merged_face_branch: usize,
    /// Lava blocks a write's neighbour notification turned to stone — the
    /// `LiquidBlock#shouldSpreadLiquid` conversions. Reported because a pass
    /// that never reaches lava and one that reaches it and declines write the
    /// same region file.
    pub lava_conversions: usize,
}

/// A `FluidState`, for the two fluids a block state can carry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fluid {
    Empty,
    /// `amount` is `FluidState#getAmount`, 1..=8; a source is always 8.
    Flowing {
        kind: Kind,
        amount: i32,
        falling: bool,
        source: bool,
    },
}

impl Fluid {
    const fn is_empty(self) -> bool {
        matches!(self, Self::Empty)
    }

    /// `FluidState#isSource`.
    const fn is_source(self) -> bool {
        matches!(self, Self::Flowing { source: true, .. })
    }

    /// `Fluid#isSame(this)`, where `this` is the fluid being ticked.
    ///
    /// This replaced an `is_water`, and the rename is the point: every caller
    /// asks whether the neighbour is *the same fluid as the one spreading*,
    /// never whether it is water. A lava tick that reads a water neighbour as
    /// "same" would flow lava out of a pond.
    const fn is_same(self, kind: Kind) -> bool {
        matches!(self, Self::Flowing { kind: k, .. } if matches!((k, kind), (Kind::Water, Kind::Water) | (Kind::Lava, Kind::Lava)))
    }

    /// `FluidState#getAmount`.
    const fn amount(self) -> i32 {
        match self {
            Self::Flowing { amount, .. } => amount,
            Self::Empty => 0,
        }
    }

    const fn falling(self) -> bool {
        matches!(self, Self::Flowing { falling: true, .. })
    }

    /// `FluidState#getOwnHeight` — `amount / 9.0F`.
    ///
    /// `FluidState#getHeight(BlockGetter, BlockPos)` returns `1.0F` when the
    /// block above holds the same fluid and this otherwise; the one caller
    /// here passes that test in separately.
    fn own_height(self) -> f32 {
        #[expect(
            clippy::cast_precision_loss,
            reason = "an amount bounded by 8, exactly representable"
        )]
        let amount = self.amount() as f32;
        amount / 9.0
    }

    /// `FlowingFluid#getFlowing(int, boolean)`.
    const fn flowing(kind: Kind, amount: i32, falling: bool) -> Self {
        Self::Flowing {
            kind,
            amount,
            falling,
            source: false,
        }
    }

    /// `FlowingFluid#getSource(boolean)`.
    const fn source(kind: Kind, falling: bool) -> Self {
        Self::Flowing {
            kind,
            amount: 8,
            falling,
            source: true,
        }
    }

    /// `FluidState#createLegacyBlock`, through `getLegacyLevel`:
    /// `isSource ? 0 : 8 - min(amount, 8) + (falling ? 8 : 0)`.
    fn legacy_block(self) -> String {
        match self {
            Self::Empty => "minecraft:air".to_string(),
            Self::Flowing {
                kind, source: true, ..
            } => format!("{}[level=0]", kind.block()),
            Self::Flowing {
                kind,
                amount,
                falling,
                ..
            } => {
                let level = 8 - amount.min(8) + i32::from(falling) * 8;
                format!("{}[level={level}]", kind.block())
            }
        }
    }

    /// `FluidState#canBeReplacedWith(.., Fluid incoming, Direction)`, where
    /// `self` is the fluid already at the target.
    ///
    /// The two overrides are **not** the same shape, which is what the old
    /// single-fluid version assumed:
    ///
    /// ```text
    ///   WaterFluid  direction == DOWN && !incoming.is(FluidTags.WATER)
    ///   LavaFluid   getHeight(..) >= 0.44444445f && incoming.is(FluidTags.WATER)
    ///   EmptyFluid  iconst_1
    /// ```
    ///
    /// Lava's has no direction in it at all, and the old code gave it water's
    /// `direction == DOWN`. `above_is_same` is the `getHeight` override:
    /// `FlowingFluid#getHeight` answers `1.0F` when the block above holds the
    /// same fluid, and `getOwnHeight` otherwise.
    fn can_be_replaced_with(self, incoming: Kind, direction: usize, above_is_same: bool) -> bool {
        match self {
            Self::Empty => true,
            Self::Flowing {
                kind: Kind::Water, ..
            } => direction == DOWN && !matches!(incoming, Kind::Water),
            Self::Flowing {
                kind: Kind::Lava, ..
            } => {
                let height = if above_is_same {
                    1.0
                } else {
                    self.own_height()
                };
                height >= 0.444_444_45_f32 && matches!(incoming, Kind::Water)
            }
        }
    }
}

/// `BlockState#getFluidState`.
///
/// Three sources, in vanilla's own order of specificity: a `LiquidBlock`
/// carries its fluid's state, a `SimpleWaterloggedBlock` carries
/// `Fluids.WATER.getSource(false)` when its `waterlogged` property is set,
/// and everything else is empty.
#[must_use]
pub fn fluid_state_of(state: &str) -> Fluid {
    let (block, properties) = state
        .split_once('[')
        .map_or((state, ""), |(name, rest)| (name, rest));
    match block {
        "minecraft:water" => from_legacy_level(Kind::Water, level_of(properties)),
        "minecraft:lava" => from_legacy_level(Kind::Lava, level_of(properties)),
        _ => {
            if has_property(properties, "waterlogged", "true") {
                Fluid::source(Kind::Water, false)
            } else {
                Fluid::Empty
            }
        }
    }
}

/// The inverse of `getLegacyLevel` for a `LiquidBlock`'s sixteen states.
const fn from_legacy_level(kind: Kind, level: i32) -> Fluid {
    match level {
        0 => Fluid::source(kind, false),
        1..=7 => Fluid::flowing(kind, 8 - level, false),
        // 8..=15 is the falling half: `8 - amount + 8`, so `amount = 16 - l`.
        _ => Fluid::flowing(kind, 16 - level, true),
    }
}

fn level_of(properties: &str) -> i32 {
    for part in properties.trim_end_matches(']').split(',') {
        if let Some(value) = part.strip_prefix("level=") {
            return value.parse().unwrap_or(0);
        }
    }
    0
}

fn has_property(properties: &str, name: &str, value: &str) -> bool {
    properties
        .trim_end_matches(']')
        .split(',')
        .any(|part| part.strip_prefix(name).and_then(|r| r.strip_prefix('=')) == Some(value))
}

fn plain(state: &str) -> &str {
    state.split_once('[').map_or(state, |(name, _)| name)
}

/// `getBlock() instanceof LiquidBlockContainer`.
///
/// Read off the jar rather than guessed: the interface's implementors in 26.2
/// are `SimpleWaterloggedBlock` — which every waterloggable block gets, and
/// which is exactly the set of states carrying a `waterlogged` property —
/// plus `SeagrassBlock`, `TallSeagrassBlock`, `KelpBlock` and
/// `KelpPlantBlock`, none of which has one.
fn is_liquid_block_container(state: &str) -> bool {
    let (block, properties) = state
        .split_once('[')
        .map_or((state, ""), |(name, rest)| (name, rest));
    matches!(
        block,
        "minecraft:seagrass"
            | "minecraft:tall_seagrass"
            | "minecraft:kelp"
            | "minecraft:kelp_plant"
    ) || properties
        .trim_end_matches(']')
        .split(',')
        .any(|part| part.starts_with("waterlogged="))
}

/// `LiquidBlockContainer#canPlaceLiquid`.
///
/// `SimpleWaterloggedBlock`'s default is `fluid == Fluids.WATER` and nothing
/// else — it does **not** check `waterlogged`, so an already-waterlogged
/// block still answers yes here and is refused later, by `placeLiquid`. The
/// four plant blocks return `iconst_0`.
///
/// **The `fluid == Fluids.WATER` half is why this takes a kind.** Lava can
/// never be placed into a waterloggable block, and a version of this that
/// ignored the argument would have let a lava flow waterlog a fence.
fn can_place_liquid(state: &str, incoming: Kind) -> bool {
    matches!(incoming, Kind::Water)
        && !matches!(
            plain(state),
            "minecraft:seagrass"
                | "minecraft:tall_seagrass"
                | "minecraft:kelp"
                | "minecraft:kelp_plant"
        )
}

/// `FlowingFluid#canHoldAnyFluid`.
fn can_hold_any_fluid(state: &str, tags: &dyn TagFacts, facts: &dyn FluidFacts) -> bool {
    if is_liquid_block_container(state) {
        return true;
    }
    if facts.blocks_motion(state) {
        return false;
    }
    let block = plain(state);
    if tags.contains("minecraft:doors", block) || tags.contains("minecraft:signs", block) {
        return false;
    }
    !matches!(
        block,
        "minecraft:ladder"
            | "minecraft:sugar_cane"
            | "minecraft:bubble_column"
            | "minecraft:nether_portal"
            | "minecraft:end_portal"
            | "minecraft:end_gateway"
            | "minecraft:structure_void"
    )
}

/// `FlowingFluid#canHoldSpecificFluid(BlockGetter, BlockPos, BlockState, Fluid)`.
fn can_hold_specific_fluid(state: &str, incoming: Kind) -> bool {
    if is_liquid_block_container(state) {
        return can_place_liquid(state, incoming);
    }
    true
}

/// `FlowingFluid#canHoldFluid`.
fn can_hold_fluid(
    state: &str,
    incoming: Kind,
    tags: &dyn TagFacts,
    facts: &dyn FluidFacts,
) -> bool {
    can_hold_any_fluid(state, tags, facts) && can_hold_specific_fluid(state, incoming)
}

/// `FlowingFluid#canPassThroughWall`.
///
/// `direction` points from `from` to `to`, which is how every vanilla caller
/// passes it. The two debug fields the method opens with —
/// `DEBUG_DISABLE_LIQUID_SPREADING` and `DEBUG_ONLY_GENERATE_HALF_THE_WORLD`
/// — are compile-time false in a release jar and are not reproduced.
fn can_pass_through_wall(
    direction: usize,
    from_state: &str,
    to_state: &str,
    facts: &dyn FluidFacts,
    stats: &mut Stats,
) -> bool {
    let to_faces = facts.collision_full_faces(to_state);
    if to_faces.iter().all(|&full| full) {
        return false;
    }
    let from_faces = facts.collision_full_faces(from_state);
    if from_faces.iter().all(|&full| full) {
        return false;
    }
    let from_empty = from_faces.iter().all(|&full| !full);
    let to_empty = to_faces.iter().all(|&full| !full);
    if from_empty && to_empty {
        return true;
    }
    stats.merged_face_branch += 1;
    !(from_faces[direction] || to_faces[opposite(direction)])
}

fn relative(position: (i32, i32, i32), direction: usize) -> (i32, i32, i32) {
    let (dx, dy, dz) = OFFSET[direction];
    (position.0 + dx, position.1 + dy, position.2 + dz)
}

/// The `GameRules` the flow rules read, and the values a world was made
/// under.
///
/// `canConvertToSource` is the only thing in `FlowingFluid` that reaches for a
/// game rule, and both fluids override it separately:
///
/// ```text
///   WaterFluid#canConvertToSource  getGameRules().get(WATER_SOURCE_CONVERSION)
///   LavaFluid#canConvertToSource   getGameRules().get(LAVA_SOURCE_CONVERSION)
/// ```
///
/// **This is a value and not a constant because it is genuinely a property of
/// the world, not of the port**, and the two worlds this code has to be able
/// to produce disagree about it. Getting that wrong is worth 30 blocks on the
/// r=16 window: see [`Self::REFERENCE_HARNESS`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FluidRules {
    /// `GameRules.WATER_SOURCE_CONVERSION`.
    pub water_source_conversion: bool,
    /// `GameRules.LAVA_SOURCE_CONVERSION`.
    pub lava_source_conversion: bool,
}

impl FluidRules {
    /// The registration defaults, read out of `GameRules`'s static
    /// initialiser in the 26.2 jar — the `registerBoolean` call's third
    /// argument:
    ///
    /// ```text
    ///   ldc "water_source_conversion" … iconst_1 … registerBoolean
    ///   ldc "lava_source_conversion"  … iconst_0 … registerBoolean
    /// ```
    ///
    /// Note the asymmetry: **water converts by default and lava does not.**
    pub const VANILLA_DEFAULT: Self = Self {
        water_source_conversion: true,
        lava_source_conversion: false,
    };

    /// What every vanilla reference in `oracle-work` was generated under.
    ///
    /// `ci/gen-vanilla-region.sh` sends `gamerule water_source_conversion
    /// false` and `gamerule lava_source_conversion false` to the reference
    /// server **before** the window is forceloaded, so the flow logic that
    /// `LevelChunk#postProcessGeneration` runs during generation sees the rule
    /// off. Lava's line is a no-op — its default is already false — and
    /// water's is not.
    ///
    /// This mattered only after 2026-07-28. Before then the script used the
    /// pre-26.2 camelCase ids, the console answered `Incorrect argument for
    /// command`, nothing read the reply, and the rule stayed at its default.
    /// `stage/fluid.rs` was written in that window and its `new_liquid` doc
    /// said the harness "is not overridden … so it is taken as true here".
    /// That was true of the references of the day and is false of these.
    pub const REFERENCE_HARNESS: Self = Self {
        water_source_conversion: false,
        lava_source_conversion: false,
    };
}

/// The world the flow reads and writes.
struct World<'a> {
    canvas: &'a mut Canvas,
    tags: &'a dyn TagFacts,
    facts: &'a dyn FluidFacts,
    rules: FluidRules,
    /// The fluid currently being ticked — vanilla's `this` in every
    /// `FlowingFluid` method below. It is set once per marked position and
    /// never changes inside a tick: `spread` keeps calling `getNewLiquid` on
    /// the *spreading* fluid even as it walks into a neighbour holding the
    /// other one.
    kind: Kind,
    stats: Stats,
    tape: &'a mut Tape,
}

impl World<'_> {
    fn state(&self, position: (i32, i32, i32)) -> String {
        self.canvas
            .block_at(position.0, position.1, position.2)
            .to_string()
    }

    fn set(&mut self, position: (i32, i32, i32), state: &str) {
        // The `from` read is behind the tape test on purpose: it is a canvas
        // lookup per write that nothing but the tape wants.
        if self.tape.is_on() {
            let from = self.state(position);
            self.tape
                .record("flow", position, &format!("{from} -> {state}"));
        }
        self.canvas
            .set_block(position.0, position.1, position.2, state);
        self.stats.writes += 1;
        self.run_neighbour_updates(position);
    }

    /// The neighbour half of `Level#setBlock(pos, state, 3)`, run to
    /// exhaustion — `CollectingNeighborUpdater`.
    ///
    /// Every write this pass makes is vanilla's flag **3**, and bit 1 is
    /// `UPDATE_NEIGHBORS`: `blockUpdated` runs `neighborChanged` on the six
    /// neighbours. Of everything that hangs off that, exactly one thing can
    /// change a block during generation — `LiquidBlock#neighborChanged`'s call
    /// to `shouldSpreadLiquid` — so that is the only one reproduced, and the
    /// rest is a named gap rather than a silent one.
    ///
    /// # The conversion CASCADES, and this used to stop after one step
    ///
    /// `shouldSpreadLiquid` ends in
    /// `Level.setBlockAndUpdate(pos, OBSIDIAN)`, and `setBlockAndUpdate` is
    /// `setBlock(pos, state, iconst_3)` — flag 3 again. **The obsidian's own
    /// write notifies its six neighbours**, so a lava sheet under water
    /// converts wholesale from a single trigger. This is why vanilla's
    /// y = -55 aquifer roof is obsidian from edge to edge with exactly one
    /// water block flooded into it, and why ours was lava with three.
    ///
    /// Measured 2026-08-09, seed 20260722, window 48 48 80 80: 28 vanilla
    /// obsidian missing, all at y = -55; 24 of them held as lava by us and 4
    /// as water. The code, its comment ("a second round has nothing to find
    /// and cannot cascade") and the module's scope note all agreed, and all
    /// three were wrong.
    ///
    /// # Iterative, and why the order cannot change the answer
    ///
    /// Vanilla's updater keeps its pending work on the heap and drains it
    /// before returning; recursing here instead would put a lava sheet's
    /// diameter on the Rust stack. The traversal order differs from vanilla's
    /// layered one, and that is safe for a reason worth stating rather than
    /// assuming: a position converts iff it holds lava and has water in one of
    /// `{UP, N, S, E, W}`. This pass never removes water and a conversion
    /// never adds any, so each position's predicate is **monotone and
    /// independent of when it is evaluated**; and a conversion only ever
    /// removes lava, so it cannot enable another. The reachable set through
    /// the notification graph is therefore the same under any order.
    /// `a_cascade_converts_the_same_sheet_from_either_end` is that argument as
    /// a test.
    fn run_neighbour_updates(&mut self, origin: (i32, i32, i32)) {
        let mut pending = vec![origin];
        while let Some(position) = pending.pop() {
            for direction in UPDATE_ORDER {
                let at = relative(position, direction);
                let state = self.state(at);
                if plain(&state) != "minecraft:lava" {
                    continue;
                }
                if self.should_spread_liquid(at, &state) {
                    pending.push(at);
                }
            }
        }
    }

    /// `LiquidBlock#shouldSpreadLiquid`, the lava branch, for its block effect.
    ///
    /// Returns whether it converted, which is the caller's cue to notify the
    /// new block's neighbours in turn. Vanilla returns `false` on the same
    /// path — it is telling `neighborChanged` not to schedule a fluid tick on
    /// a block that is no longer a fluid — so the `bool` here is the opposite
    /// polarity of vanilla's and is deliberately named for what it reports.
    ///
    /// `POSSIBLE_FLOW_DIRECTIONS` is `[DOWN, SOUTH, NORTH, EAST, WEST]` and
    /// the loop tests `getOpposite()` of each, so what is actually examined is
    /// the block **above** and the four horizontals — never the one below.
    /// Water there turns a lava *source* to obsidian and flowing lava to
    /// cobblestone; the choice reads the lava's own fluid state, not the
    /// neighbour's, so the iteration order cannot change the answer.
    ///
    /// The `soul_soil` + `blue_ice` -> basalt arm of the same loop is not
    /// modelled. It is a named gap: neither block is placed anywhere in the
    /// overworld generator, and `lava_conversions` counts what this does reach.
    fn should_spread_liquid(&mut self, position: (i32, i32, i32), state: &str) -> bool {
        const ABOVE_AND_HORIZONTALS: [usize; 5] = [1, 2, 3, 4, 5];
        for direction in ABOVE_AND_HORIZONTALS {
            let neighbour = self.state(relative(position, direction));
            if !is_water_fluid(&neighbour) {
                continue;
            }
            // `level.getFluidState(pos).isSource()` — read off the lava's own
            // `level`, which is 0 for a source.
            let converted = if lava_is_source(state) {
                "minecraft:obsidian"
            } else {
                "minecraft:cobblestone"
            };
            self.canvas
                .set_block(position.0, position.1, position.2, converted);
            self.stats.lava_conversions += 1;
            self.tape.record(
                "convert",
                position,
                &format!("{state} -> {converted} water={}", DIRECTION_NAME[direction]),
            );
            return true;
        }
        false
    }

    /// `FlowingFluid#canMaybePassThrough`.
    fn can_maybe_pass_through(
        &mut self,
        from_state: &str,
        direction: usize,
        to_state: &str,
        to_fluid: Fluid,
    ) -> bool {
        !self.is_source_of_this_type(to_fluid)
            && can_hold_any_fluid(to_state, self.tags, self.facts)
            && can_pass_through_wall(direction, from_state, to_state, self.facts, &mut self.stats)
    }

    /// `FlowingFluid#canPassThrough`.
    fn can_pass_through(
        &mut self,
        from_state: &str,
        direction: usize,
        to_state: &str,
        to_fluid: Fluid,
    ) -> bool {
        self.can_maybe_pass_through(from_state, direction, to_state, to_fluid)
            && can_hold_specific_fluid(to_state, self.kind)
    }

    /// `FlowingFluid#sourceNeighborCount`.
    fn source_neighbour_count(&self, position: (i32, i32, i32)) -> i32 {
        HORIZONTAL
            .iter()
            .filter(|&&direction| {
                self.is_source_of_this_type(fluid_state_of(
                    &self.state(relative(position, direction)),
                ))
            })
            .count()
            .try_into()
            .unwrap_or(i32::MAX)
    }

    /// `FlowingFluid#getNewLiquid`.
    ///
    /// `canConvertToSource` is `WaterFluid`'s, which reads the
    /// `water_source_conversion` game rule — [`FluidRules`] has the id, the
    /// registration default and the value the references carry. It used to be
    /// hardcoded true here, on the stated grounds that the harness producing
    /// the references did not override it. **The harness does override it**,
    /// and that arm is why 27 rim cells of underground water bodies were
    /// sources for us and flowing for vanilla, plus 3 more where it is tested
    /// ahead of the falling arm and pre-empted it.
    fn new_liquid(&mut self, position: (i32, i32, i32), block_state: &str) -> Fluid {
        let mut max_amount = 0;
        let mut sources = 0;
        for &direction in &HORIZONTAL {
            let neighbour = relative(position, direction);
            let neighbour_state = self.state(neighbour);
            let neighbour_fluid = fluid_state_of(&neighbour_state);
            if !neighbour_fluid.is_same(self.kind) {
                continue;
            }
            if can_pass_through_wall(
                direction,
                block_state,
                &neighbour_state,
                self.facts,
                &mut self.stats,
            ) {
                if neighbour_fluid.is_source() {
                    sources += 1;
                }
                max_amount = max_amount.max(neighbour_fluid.amount());
            }
        }

        if sources >= 2 && self.kind.can_convert_to_source(self.rules) {
            let below = relative(position, DOWN);
            let below_state = self.state(below);
            if self.facts.is_solid(&below_state)
                || self.is_source_of_this_type(fluid_state_of(&below_state))
            {
                return Fluid::source(self.kind, false);
            }
        }

        let above = relative(position, UP);
        let above_state = self.state(above);
        let above_fluid = fluid_state_of(&above_state);
        if !above_fluid.is_empty()
            && above_fluid.is_same(self.kind)
            && can_pass_through_wall(UP, block_state, &above_state, self.facts, &mut self.stats)
        {
            return Fluid::flowing(self.kind, 8, true);
        }

        let amount = max_amount - self.kind.drop_off();
        if amount <= 0 {
            return Fluid::Empty;
        }
        Fluid::flowing(self.kind, amount, false)
    }

    /// `FlowingFluid#isWaterHole`.
    fn is_water_hole(
        &mut self,
        position_state: &str,
        below: (i32, i32, i32),
        below_state: &str,
    ) -> bool {
        if !can_pass_through_wall(
            DOWN,
            position_state,
            below_state,
            self.facts,
            &mut self.stats,
        ) {
            return false;
        }
        let _ = below;
        if fluid_state_of(below_state).is_same(self.kind) {
            return true;
        }
        can_hold_fluid(below_state, self.kind, self.tags, self.facts)
    }

    /// `FlowingFluid$SpreadContext#isHole`, with its own state cache.
    ///
    /// The cache is keyed on `(dx, dz)` from the origin and **ignores y** —
    /// `getCacheKey` packs only the two horizontal offsets. That is sound
    /// because the only walk that uses the context, `getSlopeDistance`, moves
    /// horizontally, so every cached position shares the origin's y. The
    /// `below` read deliberately bypasses the cache, exactly as vanilla's
    /// `lambda$isHole$0` reads `this.level.getBlockState(below)` directly.
    fn is_hole(&mut self, cache: &mut SpreadCache, position: (i32, i32, i32)) -> bool {
        if let Some(&answer) = cache.holes.get(&(position.0, position.2)) {
            return answer;
        }
        let state = self.cached_state(cache, position);
        let below = relative(position, DOWN);
        let below_state = self.state(below);
        let answer = self.is_water_hole(&state, below, &below_state);
        cache.holes.insert((position.0, position.2), answer);
        answer
    }

    fn cached_state(&self, cache: &mut SpreadCache, position: (i32, i32, i32)) -> String {
        cache
            .states
            .entry((position.0, position.2))
            .or_insert_with(|| {
                self.canvas
                    .block_at(position.0, position.1, position.2)
                    .to_string()
            })
            .clone()
    }

    /// `FlowingFluid#getSlopeDistance`.
    fn slope_distance(
        &mut self,
        cache: &mut SpreadCache,
        position: (i32, i32, i32),
        depth: i32,
        exclude: usize,
        block_state: &str,
    ) -> i32 {
        let mut best = NO_SLOPE;
        for &direction in &HORIZONTAL {
            if direction == exclude {
                continue;
            }
            let to = relative(position, direction);
            let to_state = self.cached_state(cache, to);
            let to_fluid = fluid_state_of(&to_state);
            if !self.can_pass_through(block_state, direction, &to_state, to_fluid) {
                continue;
            }
            if self.is_hole(cache, to) {
                return depth;
            }
            if depth < self.kind.slope_find_distance() {
                let distance =
                    self.slope_distance(cache, to, depth + 1, opposite(direction), &to_state);
                best = best.min(distance);
            }
        }
        best
    }

    /// `FlowingFluid#getSpread`.
    ///
    /// The returned map is exactly the directions whose slope distance equals
    /// the minimum, which is why it is collected as a `Vec` and filtered
    /// afterwards rather than mimicking the `clear()`: `clear()` can only ever
    /// drop entries strictly worse than the new minimum, so the two are the
    /// same set. The `Vec` is then walked in `HORIZONTAL` order, which is
    /// `Direction` ordinal order, which is `EnumMap` iteration order.
    fn spread_map(
        &mut self,
        position: (i32, i32, i32),
        block_state: &str,
    ) -> Vec<(usize, Fluid, i32)> {
        let mut cache = SpreadCache::default();
        let mut candidates: Vec<(usize, Fluid, i32)> = Vec::new();
        for &direction in &HORIZONTAL {
            let to = relative(position, direction);
            let to_state = self.state(to);
            let to_fluid = fluid_state_of(&to_state);
            if !self.can_maybe_pass_through(block_state, direction, &to_state, to_fluid) {
                continue;
            }
            let new = self.new_liquid(to, &to_state);
            if !can_hold_specific_fluid(&to_state, self.kind) {
                continue;
            }
            let distance = if self.is_hole(&mut cache, to) {
                0
            } else {
                self.slope_distance(&mut cache, to, 1, opposite(direction), &to_state)
            };
            if self.can_be_replaced_with(to, to_fluid, direction) {
                candidates.push((direction, new, distance));
            } else {
                // Still competes for the minimum — vanilla updates `minDist`
                // outside the `canBeReplacedWith` test — but is never placed.
                candidates.push((direction, Fluid::Empty, distance));
            }
        }
        let Some(minimum) = candidates.iter().map(|&(_, _, d)| d).min() else {
            return Vec::new();
        };
        candidates.retain(|&(_, new, d)| d == minimum && !new.is_empty());
        candidates
    }

    /// `FlowingFluid#spreadTo`, and `LavaFluid`'s override of it.
    ///
    /// `beforeDestroyingBlock` only drops items and cannot change a block, so
    /// it has no world effect to reproduce here. `fizz` is a sound and
    /// particles.
    ///
    /// `LavaFluid#spreadTo` intercepts exactly one case before delegating:
    /// lava spreading **DOWN** into water turns the target to stone instead
    /// of placing lava, and only when the target block is a `LiquidBlock`.
    /// That last clause is not decoration — a waterlogged block below lava
    /// has a water fluid state and is *not* a `LiquidBlock`, so it fizzes and
    /// is left alone rather than being turned to stone.
    fn spread_to(&mut self, position: (i32, i32, i32), state: &str, direction: usize, new: Fluid) {
        if matches!(self.kind, Kind::Lava)
            && direction == DOWN
            && fluid_state_of(state).is_same(Kind::Water)
        {
            if matches!(plain(state), "minecraft:water" | "minecraft:lava") {
                self.set(position, "minecraft:stone");
            }
            return;
        }
        if is_liquid_block_container(state) {
            if can_place_liquid(state, self.kind) && !has_waterlogged_true(state) {
                let placed = state.replace("waterlogged=false", "waterlogged=true");
                if placed != state {
                    self.set(position, &placed);
                }
            }
            return;
        }
        self.set(position, &new.legacy_block());
    }

    /// `FluidState#canBeReplacedWith`, with the `getHeight` override supplied.
    fn can_be_replaced_with(
        &mut self,
        target: (i32, i32, i32),
        target_fluid: Fluid,
        direction: usize,
    ) -> bool {
        let above_is_same = match target_fluid {
            Fluid::Flowing { kind, .. } => {
                fluid_state_of(&self.state(relative(target, UP))).is_same(kind)
            }
            Fluid::Empty => false,
        };
        target_fluid.can_be_replaced_with(self.kind, direction, above_is_same)
    }

    /// `FlowingFluid#isSourceBlockOfThisType`.
    fn is_source_of_this_type(&self, fluid: Fluid) -> bool {
        fluid.is_same(self.kind) && fluid.is_source()
    }

    /// `FlowingFluid#spreadToSides`.
    fn spread_to_sides(&mut self, position: (i32, i32, i32), fluid: Fluid, block_state: &str) {
        let mut remaining = fluid.amount() - self.kind.drop_off();
        if fluid.falling() {
            remaining = 7;
        }
        if remaining <= 0 {
            return;
        }
        for (direction, new, _) in self.spread_map(position, block_state) {
            let to = relative(position, direction);
            let to_state = self.state(to);
            self.spread_to(to, &to_state, direction, new);
        }
    }

    /// `FlowingFluid#spread`.
    fn spread(&mut self, position: (i32, i32, i32), block_state: &str, fluid: Fluid) {
        if fluid.is_empty() {
            return;
        }
        let below = relative(position, DOWN);
        let below_state = self.state(below);
        let below_fluid = fluid_state_of(&below_state);

        if self.can_maybe_pass_through(block_state, DOWN, &below_state, below_fluid) {
            let new = self.new_liquid(below, &below_state);
            if self.can_be_replaced_with(below, below_fluid, DOWN)
                && can_hold_specific_fluid(&below_state, self.kind)
            {
                self.spread_to(below, &below_state, DOWN, new);
                if self.source_neighbour_count(position) >= 3 {
                    self.spread_to_sides(position, fluid, block_state);
                }
                return;
            }
        }
        if fluid.is_source() || !self.is_water_hole(block_state, below, &below_state) {
            self.spread_to_sides(position, fluid, block_state);
        }
    }

    /// `FlowingFluid#tick`.
    ///
    /// The scheduled tick the non-source branch queues is not reproduced:
    /// `crates/rustmc-world/src/anvil.rs` writes `fluid_ticks` empty, the
    /// region differ compares blocks, and a tick that is never executed
    /// cannot move one. It is a named divergence, not an oversight.
    fn tick(&mut self, position: (i32, i32, i32)) {
        let mut block_state = self.state(position);
        let mut fluid = fluid_state_of(&block_state);
        if !fluid.is_source() {
            let new = self.new_liquid(position, &block_state);
            if new.is_empty() {
                fluid = new;
                block_state = "minecraft:air".to_string();
                self.set(position, &block_state);
            } else if new != fluid {
                fluid = new;
                block_state = fluid.legacy_block();
                self.set(position, &block_state);
            }
        }
        self.spread(position, &block_state, fluid);
    }
}

/// `LiquidBlock.LEVEL` is 0 for a source, so a bare `minecraft:lava` — which
/// is the default state — is a source too.
fn lava_is_source(state: &str) -> bool {
    state
        .split_once('[')
        .and_then(|(_, properties)| {
            properties
                .trim_end_matches(']')
                .split(',')
                .find_map(|pair| pair.split_once('=').filter(|(k, _)| *k == "level"))
        })
        .is_none_or(|(_, value)| value == "0")
}

/// `level.getFluidState(pos).is(FluidTags.WATER)` — water, or anything
/// waterlogged, since a waterlogged block's fluid state *is* water.
fn is_water_fluid(state: &str) -> bool {
    plain(state) == "minecraft:water" || has_waterlogged_true(state)
}

fn has_waterlogged_true(state: &str) -> bool {
    state
        .split_once('[')
        .is_some_and(|(_, properties)| has_property(properties, "waterlogged", "true"))
}

/// `FlowingFluid$SpreadContext`'s two caches, keyed as vanilla keys them.
#[derive(Default)]
struct SpreadCache {
    states: std::collections::HashMap<(i32, i32), String>,
    holes: std::collections::HashMap<(i32, i32), bool>,
}

/// `FluidState#tick` for one marked position — the first thing
/// `postProcessGeneration` does at each one.
///
/// It is a single position rather than the whole list because vanilla's pass
/// is **one** walk that interleaves this with the shape fold, and every write
/// either half makes is visible to the next position. The caller owns the
/// walk, since it is `Canvas::post_processing` that reproduces the traversal
/// order — sections ascending, insertion order within a section — and it is
/// that order which lets one pass build a column of flowing water more than
/// one block deep.
pub fn tick_at(
    canvas: &mut Canvas,
    position: (i32, i32, i32),
    tags: &dyn TagFacts,
    facts: &dyn FluidFacts,
    rules: FluidRules,
    stats: &mut Stats,
    tape: &mut Tape,
) {
    let mut world = World {
        canvas,
        tags,
        facts,
        rules,
        // Overwritten below before anything reads it; a marked position that
        // holds no fluid at all never enters a `FlowingFluid` method.
        kind: Kind::Water,
        stats: *stats,
        tape,
    };
    let state = world.state(position);
    match fluid_state_of(&state) {
        Fluid::Empty => {}
        Fluid::Flowing { kind, .. } => {
            world.kind = kind;
            match kind {
                Kind::Water => world.stats.ticked += 1,
                Kind::Lava => world.stats.ticked_lava += 1,
            }
            world.tape.record("tick", position, &state);
            world.tick(position);
        }
    }
    *stats = world.stats;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_levels_round_trip_through_the_fluid_state() {
        // `getLegacyLevel` is the only place a block level and a fluid amount
        // meet, and getting it backwards silently turns a trickle into a
        // torrent. Sixteen states, both directions.
        for kind in [Kind::Water, Kind::Lava] {
            for level in 0..16 {
                let state = format!("{}[level={level}]", kind.block());
                let fluid = fluid_state_of(&state);
                assert_eq!(fluid.legacy_block(), state, "{kind:?} level {level}");
            }
        }
        assert_eq!(
            fluid_state_of("minecraft:water[level=0]"),
            Fluid::source(Kind::Water, false)
        );
        assert_eq!(
            fluid_state_of("minecraft:water[level=1]"),
            Fluid::flowing(Kind::Water, 7, false)
        );
        assert_eq!(
            fluid_state_of("minecraft:water[level=8]"),
            Fluid::flowing(Kind::Water, 8, true)
        );
        // The overworld drop-off, which is the whole of `lava[level=2]`: one
        // step out of a source is amount 8 - 2 = 6, and `getLegacyLevel`
        // writes `8 - 6`.
        assert_eq!(Kind::Lava.drop_off(), 2);
        assert_eq!(
            Fluid::flowing(Kind::Lava, 8 - Kind::Lava.drop_off(), false).legacy_block(),
            "minecraft:lava[level=2]"
        );
        // ... and water's first step is level=1, which is what the 27 rim
        // cells that closed with the game rule carry.
        assert_eq!(
            Fluid::flowing(Kind::Water, 8 - Kind::Water.drop_off(), false).legacy_block(),
            "minecraft:water[level=1]"
        );
        // The two fluids are never `isSame` as each other.
        assert!(!fluid_state_of("minecraft:lava[level=0]").is_same(Kind::Water));
        assert!(!fluid_state_of("minecraft:water[level=0]").is_same(Kind::Lava));
    }

    #[test]
    fn a_waterlogged_block_carries_a_water_source_and_a_dry_one_does_not() {
        let wet =
            "minecraft:oak_fence[east=false,north=false,south=false,waterlogged=true,west=false]";
        let dry =
            "minecraft:oak_fence[east=false,north=false,south=false,waterlogged=false,west=false]";
        assert_eq!(fluid_state_of(wet), Fluid::source(Kind::Water, false));
        assert_eq!(fluid_state_of(dry), Fluid::Empty);
        assert!(is_liquid_block_container(wet));
        assert!(is_liquid_block_container(dry));
    }

    #[test]
    fn seagrass_holds_any_fluid_but_no_specific_one() {
        // The asymmetry is real and it is the whole reason the two questions
        // are separate methods: `canHoldAnyFluid` short-circuits true on
        // `instanceof LiquidBlockContainer`, and `SeagrassBlock`'s own
        // `canPlaceLiquid` is `iconst_0`.
        assert!(is_liquid_block_container("minecraft:seagrass"));
        assert!(!can_place_liquid("minecraft:seagrass", Kind::Water));
        assert!(!can_hold_specific_fluid("minecraft:seagrass", Kind::Water));
        assert!(can_hold_specific_fluid("minecraft:air", Kind::Water));
        // `SimpleWaterloggedBlock#canPlaceLiquid` ends in
        // `fluid == Fluids.WATER`, so lava can never enter a waterloggable
        // block however the rest of the predicate answers.
        let fence =
            "minecraft:oak_fence[east=false,north=false,south=false,waterlogged=false,west=false]";
        assert!(can_place_liquid(fence, Kind::Water));
        assert!(!can_place_liquid(fence, Kind::Lava));
        assert!(!can_hold_specific_fluid(fence, Kind::Lava));
    }

    #[test]
    fn water_is_never_replaced_by_water_in_any_direction() {
        // `WaterFluid.canBeReplacedWith` is
        // `direction == DOWN && !fluid.is(FluidTags.WATER)`. A port that read
        // the DOWN clause and forgot the tag clause would let water overwrite
        // water and flood every aquifer it touched.
        for direction in 0..6 {
            let src = Fluid::source(Kind::Water, false);
            let flow = Fluid::flowing(Kind::Water, 4, false);
            assert!(!src.can_be_replaced_with(Kind::Water, direction, false));
            assert!(!flow.can_be_replaced_with(Kind::Water, direction, false));
            assert!(Fluid::Empty.can_be_replaced_with(Kind::Water, direction, false));
            // ... but lava DOWN into water is exactly the case that is
            // allowed, and it is the one `LavaFluid#spreadTo` intercepts.
            assert_eq!(
                src.can_be_replaced_with(Kind::Lava, direction, false),
                direction == DOWN
            );
        }
    }

    #[test]
    fn lava_is_replaced_by_water_in_every_direction_above_a_height_of_four_ninths() {
        // `LavaFluid.canBeReplacedWith` is
        // `getHeight(..) >= 0.44444445f && fluid.is(FluidTags.WATER)`. Two
        // things the old single-fluid version got wrong by giving lava
        // water's rule: there is no direction in it at all, and it is gated
        // on a height rather than being flatly false.
        //
        // `getOwnHeight` is `amount / 9.0F`, and 4/9 as an f32 is exactly the
        // literal 0.44444445f, so amount 4 is the first that passes.
        for direction in 0..6 {
            for amount in 1..=8 {
                let lava = Fluid::flowing(Kind::Lava, amount, false);
                assert_eq!(
                    lava.can_be_replaced_with(Kind::Water, direction, false),
                    amount >= 4,
                    "amount {amount} direction {direction}"
                );
                // Never by lava, whatever the height.
                assert!(!lava.can_be_replaced_with(Kind::Lava, direction, false));
            }
            // `getHeight` answers 1.0F when the block above is the same
            // fluid, which lifts even a one-deep trickle over the threshold.
            assert!(Fluid::flowing(Kind::Lava, 1, false).can_be_replaced_with(
                Kind::Water,
                direction,
                true
            ));
        }
    }

    // ---------------------------------------------------------------------
    // The cascade. `run_neighbour_updates` is the only part of this pass that
    // needs a world, so these four are the only tests here that build one.
    // ---------------------------------------------------------------------

    struct Facts;
    impl FluidFacts for Facts {
        fn blocks_motion(&self, state: &str) -> bool {
            !matches!(
                plain(state),
                "minecraft:water" | "minecraft:lava" | "minecraft:air"
            )
        }
        fn is_solid(&self, state: &str) -> bool {
            self.blocks_motion(state)
        }
        fn collision_full_faces(&self, state: &str) -> [bool; 6] {
            [self.blocks_motion(state); 6]
        }
    }

    struct NoTags;
    impl TagFacts for NoTags {
        fn contains(&self, _tag: &str, _block: &str) -> bool {
            false
        }
    }

    /// A lava sheet of `length` blocks running along +z at y=1, with a water
    /// source sheet directly above it at y=2 and stone under everything.
    ///
    /// This is the y = -55 aquifer roof in miniature: the shape that vanilla
    /// turns entirely to obsidian off a single flood.
    fn sheet(length: i32) -> Canvas {
        let mut canvas = Canvas::new(0, 16);
        canvas.ensure_chunk(0, 0, "minecraft:air");
        for z in 0..length {
            canvas.set_block(0, 0, z, "minecraft:stone");
            canvas.set_block(0, 1, z, "minecraft:lava[level=0]");
            canvas.set_block(0, 2, z, "minecraft:water[level=0]");
        }
        canvas
    }

    fn run_from(canvas: &mut Canvas, marked: (i32, i32, i32), tape: &mut Tape) -> Stats {
        let mut stats = Stats::default();
        tick_at(
            canvas,
            marked,
            &NoTags,
            &Facts,
            FluidRules::REFERENCE_HARNESS,
            &mut stats,
            tape,
        );
        stats
    }

    fn roof(canvas: &Canvas, length: i32) -> String {
        (0..length)
            .map(|z| match plain(canvas.block_at(0, 1, z)) {
                "minecraft:obsidian" => 'O',
                "minecraft:water" => 'W',
                "minecraft:lava" => 'L',
                _ => '?',
            })
            .collect()
    }

    #[test]
    fn the_obsidian_write_notifies_its_own_neighbours() {
        // THE DEFECT, at its minimum size. Three lava blocks under water; the
        // flood lands on the first. The second converts because the *water's*
        // write notified it — that always worked. The third converts only if
        // the *obsidian's* write notified in turn, which is what
        // `setBlockAndUpdate`'s flag 3 does and what this pass used to skip.
        let mut canvas = sheet(3);
        let mut tape = Tape::off();
        let stats = run_from(&mut canvas, (0, 2, 0), &mut tape);
        assert_eq!(roof(&canvas, 3), "WOO", "the far block is the cascade");
        assert_eq!(stats.lava_conversions, 2);
    }

    #[test]
    fn a_flood_into_a_lava_sheet_converts_the_whole_sheet() {
        // Vanilla's y = -55 roof at seed 20260722 is obsidian from edge to
        // edge with exactly one water block flooded into it. Ten blocks, one
        // flood, nine conversions, no lava left.
        let mut canvas = sheet(10);
        let mut tape = Tape::off();
        let stats = run_from(&mut canvas, (0, 2, 0), &mut tape);
        assert_eq!(roof(&canvas, 10), "WOOOOOOOOO");
        assert_eq!(stats.lava_conversions, 9);
    }

    #[test]
    fn a_cascade_converts_the_same_sheet_from_either_end() {
        // The order claim in `run_neighbour_updates`, as a test: the reachable
        // set does not depend on where the cascade starts, because this pass
        // never removes water and a conversion never adds any. Seeded at both
        // ends, everything but the flooded block converts either way.
        let mut left = sheet(10);
        let mut right = sheet(10);
        let mut tape = Tape::off();
        let from_left = run_from(&mut left, (0, 2, 0), &mut tape);
        let from_right = run_from(&mut right, (0, 2, 9), &mut tape);
        assert_eq!(roof(&left, 10), "WOOOOOOOOO");
        assert_eq!(roof(&right, 10), "OOOOOOOOOW");
        assert_eq!(from_left.lava_conversions, from_right.lava_conversions);
    }

    #[test]
    fn a_lava_block_with_no_water_above_it_is_left_alone() {
        // The cascade must stop at the edge of the water sheet, or it would
        // walk the whole global lava sea. Nine covered, one bare.
        let mut canvas = sheet(10);
        canvas.set_block(0, 2, 9, "minecraft:air");
        let mut tape = Tape::off();
        let stats = run_from(&mut canvas, (0, 2, 0), &mut tape);
        assert_eq!(roof(&canvas, 10), "WOOOOOOOOL");
        assert_eq!(stats.lava_conversions, 8);
    }

    #[test]
    fn the_tape_records_every_conversion_with_the_water_that_caused_it() {
        // A tape that records the flood but not the cascade would have agreed
        // with the broken pass, so what is asserted is the *last* conversion —
        // the one only the cascade reaches.
        let mut canvas = sheet(4);
        let mut tape = Tape::on();
        run_from(&mut canvas, (0, 2, 0), &mut tape);
        let lines = tape.lines();
        assert_eq!(
            lines.iter().filter(|l| l.starts_with("convert ")).count(),
            3
        );
        assert_eq!(lines[0], "tick 0 2 0 minecraft:water[level=0]");
        assert!(
            lines.iter().any(
                |l| l == "convert 0 1 3 minecraft:lava[level=0] -> minecraft:obsidian water=up"
            ),
            "the far conversion is missing from {lines:?}"
        );
        assert!(!Tape::off().is_on());
    }
}
