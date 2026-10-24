//! `FluidState`, and the five `Fluid` instances a 26.2 world can hold.
//!
//! Every line here is read from `javap -p -c -constants` on the pinned
//! `server-26.2.jar`: `Fluids`' static initialiser, `FlowingFluid`,
//! `WaterFluid` and its two inner classes, and `LiquidBlock`.
//!
//! # There are five fluids and the pair matters
//!
//! `Fluids.<clinit>` registers, in order, `EMPTY`, `FLOWING_WATER`, `WATER`,
//! `FLOWING_LAVA`, `LAVA` — the source and the flowing form of each liquid
//! are **separate registered instances**, not two states of one. That is not
//! a detail of the registry:
//!
//! * `Level.scheduleTick(pos, fluidState.getType(), delay)` is what
//!   `FlowingFluid.tick` and `LiquidBlock.onPlace` both call, so the fluid
//!   queue's `UNIQUE_TICK_HASH` key is the **concrete** instance. A source
//!   tick and a flowing tick at one position are two entries and vanilla runs
//!   both.
//! * `WaterFluid.isSame(f)` is `f == WATER || f == FLOWING_WATER`, so every
//!   *comparison* in `FlowingFluid` collapses the pair again. Reading `isSame`
//!   as `==` makes water stop flowing into water; reading `getType()` as
//!   `isSame` makes two ticks into one.
//!
//! # The state is four fields and `source` is not `amount == 8`
//!
//! `WaterFluid.Source.isSource` is `iconst_1` and `WaterFluid.Flowing.isSource`
//! is `iconst_0` — it is a property of the instance and never of the level.
//! `getFlowing(8, true)` is therefore a **flowing** state of amount 8, which is
//! exactly what falling water is, and any representation that infers
//! sourceness from the amount turns every waterfall into a spring.
//!
//! # Reading one off a block state needs no new table
//!
//! `block_props.json` already carries, per state, the registry id of
//! `getFluidState().getType()` — measured on this tree's own table:
//! 11,792 states carry a fluid, of which 11,761 are `minecraft:water`,
//! 15 `minecraft:flowing_water`, 15 `minecraft:flowing_lava` and 1
//! `minecraft:lava`. So *sourceness* is a lookup.
//!
//! What the column cannot give is `amount` and `falling`, and it does not have
//! to: the 30 flowing rows are exactly `water[level=1..15]` and
//! `lava[level=1..15]`, because `LiquidBlock.getFluidState` is the only
//! producer of a flowing state in the game and every other fluid-bearing state
//! — a waterlogged fence, kelp, seagrass, a bubble column — is a source.
//! `LiquidBlock`'s own `stateCache`, from its constructor:
//!
//! ```text
//!   cache[0]     = getSource(false)            // amount 8, source
//!   cache[1..7]  = getFlowing(8 - i, false)    // amount 7..1
//!   cache[8]     = getFlowing(8, true)         // falling
//!   getFluidState(state) = cache[min(state.level, 8)]
//! ```
//!
//! so `level` 9..15 all read cache[8]. [`of_state`] is that table and nothing
//! else.

/// `Fluids.EMPTY`.
pub const EMPTY: &str = "minecraft:empty";
/// `Fluids.FLOWING_WATER` — `WaterFluid$Flowing`.
pub const FLOWING_WATER: &str = "minecraft:flowing_water";
/// `Fluids.WATER` — `WaterFluid$Source`.
pub const WATER: &str = "minecraft:water";
/// `Fluids.FLOWING_LAVA` — `LavaFluid$Flowing`.
pub const FLOWING_LAVA: &str = "minecraft:flowing_lava";
/// `Fluids.LAVA` — `LavaFluid$Source`.
pub const LAVA: &str = "minecraft:lava";

/// Which liquid, ignoring source-versus-flowing.
///
/// This is the granularity `Fluid.isSame` works at, and the granularity every
/// comparison in `FlowingFluid` wants. [`FluidState::type_name`] is the other
/// one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    #[default]
    Empty,
    Water,
    Lava,
}

impl Kind {
    /// `FluidState.is(FluidTags.WATER)`.
    #[must_use]
    pub const fn is_water(self) -> bool {
        matches!(self, Self::Water)
    }

    /// `FluidState.is(FluidTags.LAVA)`.
    #[must_use]
    pub const fn is_lava(self) -> bool {
        matches!(self, Self::Lava)
    }

    /// The block a fluid of this kind writes — `createLegacyBlock`'s block
    /// half.
    #[must_use]
    pub const fn block(self) -> &'static str {
        match self {
            Self::Empty => "minecraft:air",
            Self::Water => "minecraft:water",
            Self::Lava => "minecraft:lava",
        }
    }
}

/// `FluidState`.
///
/// `amount` and `falling` are the two properties `FlowingFluid`'s state
/// definition declares (`LEVEL` on the flowing form, `FALLING` on both).
/// `source` is the instance, not a property — see the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FluidState {
    pub kind: Kind,
    /// `WaterFluid$Source.isSource` / `$Flowing.isSource`.
    pub source: bool,
    /// `getAmount()`. 8 for a source; 1..8 for a flowing state.
    pub amount: u8,
    /// The `FALLING` property.
    pub falling: bool,
}

/// `Fluids.EMPTY.defaultFluidState()`.
pub const NOTHING: FluidState = FluidState {
    kind: Kind::Empty,
    source: false,
    amount: 0,
    falling: false,
};

impl FluidState {
    /// `FlowingFluid.getSource(falling)`.
    #[must_use]
    pub const fn source(kind: Kind, falling: bool) -> Self {
        Self {
            kind,
            source: true,
            amount: 8,
            falling,
        }
    }

    /// `FlowingFluid.getFlowing(amount, falling)`.
    #[must_use]
    pub const fn flowing(kind: Kind, amount: u8, falling: bool) -> Self {
        Self {
            kind,
            source: false,
            amount,
            falling,
        }
    }

    /// `FluidState.isEmpty()`.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        matches!(self.kind, Kind::Empty)
    }

    /// `FluidState.isSource()`.
    #[must_use]
    pub const fn is_source(self) -> bool {
        self.source
    }

    /// `FluidState.getAmount()`.
    ///
    /// `WaterFluid$Source.getAmount` is `bipush 8` and does not read the
    /// state, which is why this is not `self.amount` for a source.
    #[must_use]
    pub const fn amount(self) -> u8 {
        if self.source {
            8
        } else {
            self.amount
        }
    }

    /// `Fluid.isSame(other)` — the pair, collapsed.
    #[must_use]
    pub const fn is_same(self, other: Self) -> bool {
        !self.is_empty() && self.kind as u8 == other.kind as u8
    }

    /// The column of `fluid_rules.json`'s `canHold*` family this fluid is
    /// asked about — `WATER`, `FLOWING_WATER`, `LAVA`, `FLOWING_LAVA` in that
    /// order, which is the order the table writes them. `None` for empty.
    #[must_use]
    pub const fn hold_index(self) -> Option<usize> {
        Some(match (self.kind, self.source) {
            (Kind::Empty, _) => return None,
            (Kind::Water, true) => 0,
            (Kind::Water, false) => 1,
            (Kind::Lava, true) => 2,
            (Kind::Lava, false) => 3,
        })
    }

    /// `FluidState.getType()`'s registry id — the **concrete** instance, which
    /// is what `Level.scheduleTick` keys the fluid queue on.
    #[must_use]
    pub const fn type_name(self) -> &'static str {
        match (self.kind, self.source) {
            (Kind::Empty, _) => EMPTY,
            (Kind::Water, true) => WATER,
            (Kind::Water, false) => FLOWING_WATER,
            (Kind::Lava, true) => LAVA,
            (Kind::Lava, false) => FLOWING_LAVA,
        }
    }

    /// `FlowingFluid.getLegacyLevel(state)`:
    ///
    /// ```text
    ///   if (isSource()) return 0;
    ///   return 8 - min(getAmount(), 8) + (getValue(FALLING) ? 8 : 0);
    /// ```
    #[must_use]
    pub const fn legacy_level(self) -> u8 {
        if self.source {
            return 0;
        }
        let a = if self.amount > 8 { 8 } else { self.amount };
        (8 - a) + if self.falling { 8 } else { 0 }
    }

    /// `FluidState.createLegacyBlock()` — the block state this fluid is
    /// written to the world as.
    ///
    /// `Fluids.EMPTY`'s is `Blocks.AIR.defaultBlockState()`, which is what
    /// `FlowingFluid.tick` writes when `getNewLiquid` comes back empty.
    #[must_use]
    pub fn legacy_block(self) -> String {
        if self.is_empty() {
            return "minecraft:air".to_string();
        }
        format!("{}[level={}]", self.kind.block(), self.legacy_level())
    }
}

/// `BlockState.getFluidState()`.
///
/// See the module docs for why this needs no table beyond the block state
/// string itself. The three cases are `LiquidBlock`'s `stateCache`, anything
/// carrying `waterlogged=true`, and — reached through `props_fluid` — the
/// handful of blocks whose own `getFluidState` override answers a water
/// source without a `waterlogged` property to say so.
///
/// `props_fluid` is `block_props.json`'s per-state fluid id, or `None` where
/// the caller has no table. Without it kelp, seagrass and a bubble column read
/// as dry, which is wrong in the direction of a fluid refusing to flow rather
/// than one appearing.
#[must_use]
pub fn of_state(state: &str, props_fluid: Option<&str>) -> FluidState {
    let block = state.split('[').next().unwrap_or(state);
    let kind = match block {
        "minecraft:water" => Kind::Water,
        "minecraft:lava" => Kind::Lava,
        _ => {
            return match props_fluid {
                Some(WATER | FLOWING_WATER) => FluidState::source(Kind::Water, false),
                Some(LAVA | FLOWING_LAVA) => FluidState::source(Kind::Lava, false),
                _ if state.contains("waterlogged=true") => FluidState::source(Kind::Water, false),
                _ => NOTHING,
            };
        }
    };
    // `LiquidBlock.getFluidState`: `stateCache.get(min(level, 8))`.
    match level_of(state) {
        0 => FluidState::source(kind, false),
        n if n < 8 => FluidState::flowing(kind, 8 - n, false),
        _ => FluidState::flowing(kind, 8, true),
    }
}

/// The `level` property of a `LiquidBlock` state.
///
/// A missing one is 0 — `LiquidBlock`'s `registerDefaultState` sets `LEVEL` to
/// `iconst_0`, so a bare `minecraft:water` is a source.
fn level_of(state: &str) -> u8 {
    crate::placement::property_of(state, "level")
        .and_then(|v| v.parse::<u8>().ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `LiquidBlock`'s constructor, read back through [`of_state`].
    #[test]
    fn the_liquid_block_state_cache_is_source_then_seven_flowing_then_falling() {
        assert_eq!(
            of_state("minecraft:water[level=0]", None),
            FluidState::source(Kind::Water, false)
        );
        for level in 1..8u8 {
            assert_eq!(
                of_state(&format!("minecraft:water[level={level}]"), None),
                FluidState::flowing(Kind::Water, 8 - level, false),
                "level {level}"
            );
        }
        // `min(level, 8)` — every level from 8 up reads the same falling cell.
        for level in 8..16u8 {
            assert_eq!(
                of_state(&format!("minecraft:water[level={level}]"), None),
                FluidState::flowing(Kind::Water, 8, true),
                "level {level}"
            );
        }
    }

    /// **A falling fluid has amount 8 and is not a source.** The whole reason
    /// `source` is a field.
    #[test]
    fn falling_water_is_amount_eight_and_not_a_source() {
        let falling = of_state("minecraft:water[level=8]", None);
        assert_eq!(falling.amount(), 8);
        assert!(!falling.is_source());
        assert_eq!(falling.type_name(), FLOWING_WATER);
        // `getLegacyLevel`: 8 - 8 + 8.
        assert_eq!(falling.legacy_level(), 8);
        assert_eq!(falling.legacy_block(), "minecraft:water[level=8]");
    }

    /// `createLegacyBlock` round-trips every cell of the cache, which is what
    /// makes `spreadTo` idempotent.
    #[test]
    fn legacy_block_round_trips_every_cache_cell() {
        for level in 0..9u8 {
            let state = format!("minecraft:water[level={level}]");
            assert_eq!(
                of_state(&state, None).legacy_block(),
                state,
                "level {level}"
            );
        }
    }

    /// The pair is one fluid to `isSame` and two to `getType`.
    #[test]
    fn is_same_collapses_the_pair_and_get_type_does_not() {
        let source = FluidState::source(Kind::Water, false);
        let flowing = FluidState::flowing(Kind::Water, 7, false);
        assert!(source.is_same(flowing));
        assert!(flowing.is_same(source));
        assert_ne!(source.type_name(), flowing.type_name());
        assert_eq!(source.type_name(), WATER);
        assert_eq!(flowing.type_name(), FLOWING_WATER);
        // Empty is not the same as anything, itself included:
        // `EmptyFluid.isSame` inherits `Fluid.isSame`, which is `this == f`,
        // and the callers all guard on `isEmpty` first.
        assert!(!NOTHING.is_same(NOTHING));
        assert!(!source.is_same(FluidState::source(Kind::Lava, false)));
    }

    /// A waterlogged block is a water **source**, and that is the case
    /// `props_fluid` is not needed for.
    #[test]
    fn a_waterlogged_block_is_a_water_source() {
        let f = of_state("minecraft:oak_fence[waterlogged=true,north=false]", None);
        assert_eq!(f, FluidState::source(Kind::Water, false));
        assert!(f.is_source());
        assert_eq!(f.type_name(), WATER);
    }

    /// Kelp has no `waterlogged` property and is wet anyway. Without the
    /// props column it reads dry, which is the direction this fails in and
    /// is why the argument exists.
    #[test]
    fn kelp_is_wet_only_through_the_props_column() {
        let dry = of_state("minecraft:kelp[age=0]", None);
        assert_eq!(dry, NOTHING);
        let wet = of_state("minecraft:kelp[age=0]", Some(WATER));
        assert_eq!(wet, FluidState::source(Kind::Water, false));
    }

    #[test]
    fn a_dry_block_has_no_fluid() {
        assert_eq!(of_state("minecraft:stone", None), NOTHING);
        assert!(of_state("minecraft:stone", None).is_empty());
        assert_eq!(of_state("minecraft:stone", None).type_name(), EMPTY);
        assert_eq!(NOTHING.legacy_block(), "minecraft:air");
    }
}

/// `data/vanilla/fluid_rules.json` — vanilla's own answers to the three
/// `FlowingFluid` predicates that are about a block's kind rather than about
/// arithmetic.
///
/// See `tools/goldengen/vanilla/FluidRulesGolden.java`. The short version:
/// `canHoldAnyFluid` and `canHoldSpecificFluid` are per state, and
/// `canPassThroughWall` is per **ordered pair of collision-shape identities**
/// and per direction, because that is exactly what its body reads.
#[derive(Debug, Default)]
pub struct Rules {
    hold_any: Vec<bool>,
    /// Indexed by [`FluidState::hold_index`] — the four concrete flowing
    /// fluids, because `canPlaceLiquid` compares the fluid by **identity**
    /// and a source and its flowing form get different answers.
    hold: [Vec<bool>; 4],
    /// Per state id, the index of its collision shape by identity, or
    /// `usize::MAX` for a block with a dynamic shape.
    state_group: Vec<usize>,
    groups: usize,
    /// Per `Face::to_3d` index, one bit per `(from_group, to_group)`.
    wall: [Vec<bool>; 6],
}

/// Where `ci/fluid-rules-golden.sh` writes the table [`Rules`] reads. Spelled
/// out in full so `ci/gen-gameplay-tables.sh`, which derives the tables a
/// tree needs by grepping this crate for `data/vanilla/...` literals, finds it.
pub const RULES_PATH: &str = "data/vanilla/fluid_rules.json";

/// A dynamic-shape block has no fixed collision shape and therefore no group.
const NO_GROUP: usize = usize::MAX;

impl Rules {
    /// # Errors
    /// If the file is missing or does not carry what this type indexes by.
    pub fn load(path: &std::path::Path) -> std::io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let doc: serde_json::Value = serde_json::from_str(&text).map_err(std::io::Error::other)?;
        Self::from_json(&doc)
    }

    /// # Errors
    /// If a field is absent, or a bitstring or the group table is the wrong
    /// length for the state count the file declares — every one of which
    /// would answer about the wrong block in silence.
    pub fn from_json(doc: &serde_json::Value) -> std::io::Result<Self> {
        let err = |m: String| std::io::Error::other(m);
        let n = usize::try_from(
            doc["states"]
                .as_u64()
                .ok_or_else(|| err("fluid_rules has no states".into()))?,
        )
        .map_err(std::io::Error::other)?;
        let groups = usize::try_from(
            doc["shapeGroups"]
                .as_u64()
                .ok_or_else(|| err("fluid_rules has no shapeGroups".into()))?,
        )
        .map_err(std::io::Error::other)?;
        let bits = |key: &str, want: usize| -> std::io::Result<Vec<bool>> {
            let hex = doc[key]
                .as_str()
                .ok_or_else(|| err(format!("fluid_rules has no {key}")))?;
            let mut out = Vec::with_capacity(want);
            for ch in hex.chars() {
                let nibble = ch
                    .to_digit(16)
                    .ok_or_else(|| err(format!("{key} is not hex")))?;
                for bit in (0..4).rev() {
                    if out.len() == want {
                        break;
                    }
                    out.push((nibble >> bit) & 1 == 1);
                }
            }
            if out.len() != want {
                return Err(err(format!(
                    "{key} carries {} bits, {want} expected",
                    out.len()
                )));
            }
            Ok(out)
        };
        // `Direction.values()` order, which the file states and this checks
        // rather than assumes — a transposed table is silent.
        let order: Vec<&str> = doc["directionOrder"]
            .as_array()
            .ok_or_else(|| err("fluid_rules has no directionOrder".into()))?
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect();
        if order != crate::interact::FACE_NAMES {
            return Err(err(format!(
                "fluid_rules directionOrder is {order:?}, not {:?}",
                crate::interact::FACE_NAMES
            )));
        }
        let mut wall: [Vec<bool>; 6] = Default::default();
        for (i, name) in order.iter().enumerate() {
            let hex = doc["canPassThroughWall"][*name]
                .as_str()
                .ok_or_else(|| err(format!("no canPassThroughWall for {name}")))?;
            let want = groups * groups;
            let mut out = Vec::with_capacity(want);
            for ch in hex.chars() {
                let nibble = ch
                    .to_digit(16)
                    .ok_or_else(|| err(format!("canPassThroughWall[{name}] is not hex")))?;
                for bit in (0..4).rev() {
                    if out.len() == want {
                        break;
                    }
                    out.push((nibble >> bit) & 1 == 1);
                }
            }
            if out.len() != want {
                return Err(err(format!(
                    "canPassThroughWall[{name}] carries {} bits, {want} expected",
                    out.len()
                )));
            }
            wall[i] = out;
        }
        let raw = doc["stateGroup"]
            .as_array()
            .ok_or_else(|| err("fluid_rules has no stateGroup".into()))?;
        if raw.len() != n {
            return Err(err(format!(
                "stateGroup carries {} entries for {n} states",
                raw.len()
            )));
        }
        let mut state_group = Vec::with_capacity(n);
        for v in raw {
            let g = v
                .as_i64()
                .ok_or_else(|| err("a stateGroup entry is not a number".into()))?;
            state_group.push(if g < 0 {
                NO_GROUP
            } else {
                usize::try_from(g).map_err(std::io::Error::other)?
            });
        }
        Ok(Self {
            hold_any: bits("canHoldAnyFluid", n)?,
            hold: [
                bits("canHoldWater", n)?,
                bits("canHoldFlowingWater", n)?,
                bits("canHoldLava", n)?,
                bits("canHoldFlowingLava", n)?,
            ],
            state_group,
            groups,
            wall,
        })
    }

    /// The one copy this process reads, loaded on first use.
    ///
    /// `Err` when the table is absent, which a caller must treat as "no fluid
    /// behaviour" rather than as a default: every predicate in [`Flow`] would
    /// otherwise answer from an empty table, and an empty table says nothing
    /// can hold a fluid — a server where water never moves and no test says
    /// so. `crate::status` therefore skips the fluid drain entirely when this
    /// is `Err`, and says so in its log.
    ///
    /// # Errors
    /// If the file is missing or malformed.
    pub fn shared(path: &std::path::Path) -> Result<&'static Self, String> {
        static TABLE: std::sync::OnceLock<Result<Rules, String>> = std::sync::OnceLock::new();
        TABLE
            .get_or_init(|| Self::load(path).map_err(|e| e.to_string()))
            .as_ref()
            .map_err(Clone::clone)
    }

    /// How many states the table answers about — a loader's own proof that it
    /// read something.
    #[must_use]
    pub fn len(&self) -> usize {
        self.hold_any.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.hold_any.is_empty()
    }

    /// How many distinct collision-shape identities the pair table is over.
    #[must_use]
    pub const fn groups(&self) -> usize {
        self.groups
    }

    /// `FlowingFluid.canHoldAnyFluid(state)`.
    #[must_use]
    pub fn can_hold_any_fluid(&self, state_id: i32) -> bool {
        usize::try_from(state_id)
            .ok()
            .and_then(|i| self.hold_any.get(i))
            .copied()
            .unwrap_or(false)
    }

    /// `FlowingFluid.canHoldSpecificFluid(level, pos, state, fluid)`.
    ///
    /// **Keyed on the concrete fluid and not on the liquid.**
    /// `SimpleWaterloggedBlock.canPlaceLiquid` is `fluid == Fluids.WATER` —
    /// `if_acmpne` against the source instance — so a fence takes a bucket's
    /// source and refuses a stream's flowing state, which is why flowing
    /// water goes *around* a fence instead of waterlogging it. A `Kind`-keyed
    /// table waterlogs every fence a stream passes and looks perfectly
    /// plausible doing it.
    ///
    /// `Fluids.EMPTY` is not a question any call site asks — every one passes
    /// `someFluidState.getType()` of a non-empty state — and answers `false`.
    #[must_use]
    pub fn can_hold_specific_fluid(&self, state_id: i32, fluid: FluidState) -> bool {
        let Some(table) = fluid.hold_index().map(|k| &self.hold[k]) else {
            return false;
        };
        usize::try_from(state_id)
            .ok()
            .and_then(|i| table.get(i))
            .copied()
            .unwrap_or(false)
    }

    /// `FlowingFluid.canPassThroughWall(direction, level, from, fromState, to,
    /// toState)`.
    ///
    /// `None` where either state's block has a dynamic shape and therefore no
    /// fixed collision shape — moving pistons are the whole of that family.
    /// The caller names what it does with that; this does not choose for it.
    #[must_use]
    pub fn can_pass_through_wall(
        &self,
        direction: crate::interact::Face,
        from_id: i32,
        to_id: i32,
    ) -> Option<bool> {
        let g = |id: i32| -> Option<usize> {
            let i = usize::try_from(id).ok()?;
            let g = *self.state_group.get(i)?;
            (g != NO_GROUP).then_some(g)
        };
        let (a, b) = (g(from_id)?, g(to_id)?);
        self.wall[direction.to_3d() as usize]
            .get(a * self.groups + b)
            .copied()
    }
}

/// `FlowingFluid`, bound to one liquid and one level.
///
/// A struct rather than free functions with eight arguments each, because
/// that is what the bytecode is: every predicate below is an instance method
/// on `WaterFluid` or `LavaFluid` and reads `this` for `isSame`,
/// `getDropOff`, `getSlopeFindDistance` and `getFlowing`, and reads the level
/// for the two things that are per dimension and per world.
pub struct Flow<'a> {
    /// Which liquid — the receiver of every `isSame` in `FlowingFluid`.
    pub kind: Kind,
    pub rules: &'a Rules,
    pub blocks: &'a dyn crate::edits::Blocks,
    /// `LavaFluid.isFastLava(level)` — the dimension's
    /// `EnvironmentAttributes.FAST_LAVA`, which 26.2's generated
    /// `dimension_type/the_nether.json` sets and no other dimension type does.
    /// It is what moves lava's tick delay from 30 to 10, its drop-off from 2
    /// to 1 and its slope search from 2 to 4. Water reads none of it.
    pub fast_lava: bool,
    /// `canConvertToSource(level)` — `water_source_conversion` for water,
    /// `lava_source_conversion` for lava. Game rules, so a property of the
    /// world and not of the liquid; the defaults are `true` and `false`.
    pub convert_to_source: bool,
}

/// `WaterFluid.getSlopeFindDistance` — `iconst_4`.
pub const WATER_SLOPE_FIND_DISTANCE: i32 = 4;
/// `WaterFluid.getDropOff` — `iconst_1`.
pub const WATER_DROP_OFF: u8 = 1;
/// `WaterFluid.getTickDelay` — `iconst_5`.
pub const WATER_TICK_DELAY: i64 = 5;
/// `LavaFluid.getTickDelay` — `bipush 30`, or `bipush 10` under `FAST_LAVA`.
pub const LAVA_TICK_DELAY: i64 = 30;
/// `LavaFluid.getTickDelay` under `FAST_LAVA`.
pub const FAST_LAVA_TICK_DELAY: i64 = 10;

/// `getSpread`'s and `getSlopeDistance`'s "no slope found" sentinel —
/// `sipush 1000` at offset 0 of both.
pub const NO_SLOPE: i32 = 1000;

/// `LiquidBlock.fizz` and `LavaFluid.fizz` — `levelEvent(1501, pos, 0)`,
/// `LevelEvent.LAVA_FIZZ`. The same number from both classes, written twice
/// in the jar.
pub const LEVEL_EVENT_LAVA_FIZZ: i32 = 1501;

/// `LavaFluid.canBeReplacedWith`'s threshold — `ldc float 0.44444445f`, which
/// is `4/9`: a lava block of amount 4 or more yields to water.
pub const LAVA_YIELDS_AT_HEIGHT: f32 = 0.444_444_45;

/// `level.getRandom()`, which this server does not share with anything.
///
/// `LavaFluid.getSpreadDelay` draws `nextInt(4)` from the level's own
/// `RandomSource` — a stream every other consumer of the level random also
/// draws from — so the *delay* is not reproducible against vanilla, only its
/// distribution is. It is reached only when a flowing lava block rises
/// (`next` taller than `current`, neither falling), which a pour onto flat
/// ground never does.
pub(crate) fn level_random_next_int(bound: u32) -> u32 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static STATE: AtomicU64 = AtomicU64::new(0x2545_F491_4F6C_DD1D);
    let mut x = STATE.load(Ordering::Relaxed);
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    STATE.store(x, Ordering::Relaxed);
    #[expect(clippy::cast_possible_truncation, reason = "reduced below `bound`")]
    let r = (x % u64::from(bound.max(1))) as u32;
    r
}

impl<'a> Flow<'a> {
    /// A flow under the game rules' **defaults** in a level without
    /// `FAST_LAVA` — the overworld as a fresh world has it.
    #[must_use]
    pub fn new(kind: Kind, rules: &'a Rules, blocks: &'a dyn crate::edits::Blocks) -> Self {
        Self {
            kind,
            rules,
            blocks,
            fast_lava: false,
            convert_to_source: matches!(kind, Kind::Water),
        }
    }

    /// A flow in a live level: the conversion rule read from this server's
    /// game rules, and `FAST_LAVA` from the dimension.
    #[must_use]
    pub fn in_level(
        kind: Kind,
        rules: &'a Rules,
        blocks: &'a dyn crate::edits::Blocks,
        fast_lava: bool,
    ) -> Self {
        let rule = match kind {
            Kind::Lava => "lava_source_conversion",
            _ => "water_source_conversion",
        };
        Self {
            kind,
            rules,
            blocks,
            fast_lava,
            convert_to_source: crate::gamerules::GameRules::shared().boolean(rule),
        }
    }

    /// `FlowingFluid.getDropOff(level)`: water 1; lava 2, or 1 under
    /// `FAST_LAVA`.
    #[must_use]
    pub const fn drop_off(&self) -> u8 {
        match self.kind {
            Kind::Lava if self.fast_lava => 1,
            Kind::Lava => 2,
            _ => WATER_DROP_OFF,
        }
    }

    /// `FlowingFluid.getSlopeFindDistance(level)`: water 4; lava 2, or 4
    /// under `FAST_LAVA`.
    #[must_use]
    pub const fn slope_find_distance(&self) -> i32 {
        match self.kind {
            Kind::Lava if self.fast_lava => 4,
            Kind::Lava => 2,
            _ => WATER_SLOPE_FIND_DISTANCE,
        }
    }

    /// `Fluid.getTickDelay(level)`: water 5; lava 30, or 10 under
    /// `FAST_LAVA`.
    #[must_use]
    pub const fn tick_delay(&self) -> i64 {
        match self.kind {
            Kind::Lava if self.fast_lava => FAST_LAVA_TICK_DELAY,
            Kind::Lava => LAVA_TICK_DELAY,
            _ => WATER_TICK_DELAY,
        }
    }

    /// `FlowingFluid.getSpreadDelay(level, pos, current, next)`.
    ///
    /// Water's is the inherited `getTickDelay; ireturn`. Lava overrides it
    /// (`LavaFluid.getSpreadDelay`, offsets 0-93):
    ///
    /// ```text
    ///   i = getTickDelay(level);
    ///   if (!current.isEmpty() && !next.isEmpty()
    ///           && !current.FALLING && !next.FALLING
    ///           && next.getHeight(level, pos) > current.getHeight(level, pos)
    ///           && level.getRandom().nextInt(4) != 0)
    ///       i *= 4;
    ///   return i;
    /// ```
    ///
    /// Both heights are read at the **same** position, so `hasSameAbove` is
    /// the same answer for both and only the amounts can differ.
    #[must_use]
    pub fn spread_delay(
        &self,
        pos: crate::interact::BlockPos,
        current: FluidState,
        next: FluidState,
    ) -> i64 {
        let i = self.tick_delay();
        if self.kind != Kind::Lava {
            return i;
        }
        if !current.is_empty()
            && !next.is_empty()
            && !current.falling
            && !next.falling
            && self.height(next, pos) > self.height(current, pos)
            && level_random_next_int(4) != 0
        {
            return i * 4;
        }
        i
    }

    /// `FlowingFluid.getHeight(state, level, pos)`:
    /// `hasSameAbove ? 1.0f : getOwnHeight(state)`, where `getOwnHeight` is
    /// `getAmount() / 9.0f` and `hasSameAbove` is
    /// `state.getType().isSame(level.getFluidState(pos.above()).getType())`.
    #[must_use]
    pub fn height(&self, f: FluidState, pos: crate::interact::BlockPos) -> f32 {
        let above = self.fluid_at((pos.0, pos.1 + 1, pos.2));
        if f.is_same(above) {
            return 1.0;
        }
        f32::from(f.amount()) / 9.0
    }

    /// `Level.getBlockState(pos).getFluidState()`.
    #[must_use]
    pub fn fluid_at(&self, pos: crate::interact::BlockPos) -> FluidState {
        self.blocks.fluid_at(pos)
    }

    /// `FlowingFluid.isSourceBlockOfThisType(fluidState)`.
    #[must_use]
    pub fn is_source_of_this_type(&self, f: FluidState) -> bool {
        f.kind == self.kind && !f.is_empty() && f.is_source()
    }

    /// `FlowingFluid.canPassThroughWall(direction, level, from, fromState,
    /// to, toState)`.
    ///
    /// **A dynamic-shape block answers `false`, and that is a decision rather
    /// than a default.** The table cannot carry a shape that is not fixed;
    /// moving pistons are the whole of that family, and a fluid refusing to
    /// enter one is the direction that leaves the world unchanged.
    #[must_use]
    pub fn can_pass_through_wall(
        &self,
        direction: crate::interact::Face,
        from_state: &str,
        to_state: &str,
    ) -> bool {
        let (Some(a), Some(b)) = (
            self.blocks.state_id(from_state),
            self.blocks.state_id(to_state),
        ) else {
            return false;
        };
        self.rules
            .can_pass_through_wall(direction, a, b)
            .unwrap_or(false)
    }

    /// `FlowingFluid.canMaybePassThrough(level, pos, state, direction, toPos,
    /// toState, toFluid)`:
    ///
    /// ```text
    ///   !isSourceBlockOfThisType(toFluid)
    ///     && canHoldAnyFluid(toState)
    ///     && canPassThroughWall(direction, level, pos, state, toPos, toState)
    /// ```
    #[must_use]
    pub fn can_maybe_pass_through(
        &self,
        direction: crate::interact::Face,
        from_state: &str,
        to_state: &str,
        to_fluid: FluidState,
    ) -> bool {
        if self.is_source_of_this_type(to_fluid) {
            return false;
        }
        let Some(to_id) = self.blocks.state_id(to_state) else {
            return false;
        };
        self.rules.can_hold_any_fluid(to_id)
            && self.can_pass_through_wall(direction, from_state, to_state)
    }

    /// `FlowingFluid.canPassThrough(level, fluid, pos, state, direction,
    /// toPos, toState, toFluid)` — `canMaybePassThrough` and then
    /// `canHoldSpecificFluid` for the fluid actually being placed.
    #[must_use]
    pub fn can_pass_through(
        &self,
        fluid: FluidState,
        direction: crate::interact::Face,
        from_state: &str,
        to_state: &str,
        to_fluid: FluidState,
    ) -> bool {
        if !self.can_maybe_pass_through(direction, from_state, to_state, to_fluid) {
            return false;
        }
        self.can_hold_specific_fluid(to_state, fluid)
    }

    /// `FlowingFluid.canHoldSpecificFluid(level, pos, state, fluid)`.
    #[must_use]
    pub fn can_hold_specific_fluid(&self, state: &str, fluid: FluidState) -> bool {
        self.blocks
            .state_id(state)
            .is_some_and(|id| self.rules.can_hold_specific_fluid(id, fluid))
    }

    /// `FlowingFluid.getFlowing()` — the flowing **instance** of this liquid,
    /// which is what `getSlopeDistance` and `isWaterHole` pass to
    /// `canHoldSpecificFluid`. Its amount is never read there.
    #[must_use]
    pub const fn flowing_form(&self) -> FluidState {
        FluidState::flowing(self.kind, 1, false)
    }

    /// `FlowingFluid.sourceNeighborCount(level, pos)` — how many of the four
    /// horizontal neighbours are a source of this fluid.
    #[must_use]
    pub fn source_neighbor_count(&self, pos: crate::interact::BlockPos) -> u32 {
        let mut n = 0;
        for d in crate::interact::HORIZONTAL {
            if self.is_source_of_this_type(self.fluid_at(d.relative(pos))) {
                n += 1;
            }
        }
        n
    }

    /// `FlowingFluid.isWaterHole(level, from, fromState, to, toState)` — the
    /// misleadingly named "would this fluid fall through here", asked about
    /// the block **below**.
    ///
    /// ```text
    ///   if (!canPassThroughWall(DOWN, level, from, fromState, to, toState))
    ///       return false;
    ///   if (toState.getFluidState().getType().isSame(this)) return true;
    ///   return canHoldFluid(level, to, toState, getFlowing());
    /// ```
    #[must_use]
    pub fn is_water_hole(&self, from_state: &str, below_state: &str) -> bool {
        if !self.can_pass_through_wall(crate::interact::Face::Down, from_state, below_state) {
            return false;
        }
        let f = self.fluid_at_state(below_state);
        if !f.is_empty() && f.kind == self.kind {
            return true;
        }
        // `canHoldFluid` is `canHoldAnyFluid && canHoldSpecificFluid`.
        self.blocks.state_id(below_state).is_some_and(|id| {
            self.rules.can_hold_any_fluid(id)
                && self.rules.can_hold_specific_fluid(id, self.flowing_form())
        })
    }

    /// `BlockState.getFluidState()` for a state this caller already has.
    #[must_use]
    pub fn fluid_at_state(&self, state: &str) -> FluidState {
        self.blocks.fluid_of_state(state)
    }
}

/// `FlowingFluid.SpreadContext` — the per-`getSpread` cache of "is this
/// position a hole". A cache and not a rule: the only property that matters
/// is that it answers what an uncached call would.
#[derive(Default)]
struct SpreadContext {
    states: std::collections::BTreeMap<crate::interact::BlockPos, String>,
    holes: std::collections::BTreeMap<crate::interact::BlockPos, bool>,
}

impl Flow<'_> {
    /// `FlowingFluid.getNewLiquid(level, pos, state)`:
    ///
    /// ```text
    ///   maxAmount = 0; sources = 0;
    ///   for (Direction d : Plane.HORIZONTAL) {                 // 15-123
    ///       nb = pos.relative(d); nbState = getBlockState(nb);
    ///       f = nbState.getFluidState();
    ///       if (!f.getType().isSame(this)) continue;
    ///       if (!canPassThroughWall(d, level, pos, state, nb, nbState)) continue;
    ///       if (f.isSource()) sources++;
    ///       maxAmount = Math.max(maxAmount, f.getAmount());
    ///   }
    ///   if (sources >= 2 && canConvertToSource(level)) {        // 126-185
    ///       below = getBlockState(pos.below());
    ///       if (below.isSolid() || isSourceBlockOfThisType(below.getFluidState()))
    ///           return getSource(false);
    ///   }
    ///   above = getBlockState(pos.above());                     // 188-251
    ///   if (above fluid isSame && canPassThroughWall(UP, ...)) return getFlowing(8, true);
    ///   next = maxAmount - getDropOff(level);                   // 255-281
    ///   return next <= 0 ? EMPTY : getFlowing(next, false);
    /// ```
    #[must_use]
    pub fn get_new_liquid(&self, pos: crate::interact::BlockPos, state: &str) -> FluidState {
        let mut max_amount = 0u8;
        let mut sources = 0u32;
        for d in crate::interact::HORIZONTAL {
            let nb = d.relative(pos);
            let nb_state = self.blocks.state_at(nb);
            let f = self.fluid_at_state(&nb_state);
            if f.is_empty() || f.kind != self.kind {
                continue;
            }
            if !self.can_pass_through_wall(d, state, &nb_state) {
                continue;
            }
            if f.is_source() {
                sources += 1;
            }
            max_amount = max_amount.max(f.amount());
        }
        if sources >= 2 && self.convert_to_source {
            let below_state = self.blocks.state_at((pos.0, pos.1 - 1, pos.2));
            if self.blocks.solid_of_state(&below_state)
                || self.is_source_of_this_type(self.fluid_at_state(&below_state))
            {
                return FluidState::source(self.kind, false);
            }
        }
        let above_state = self.blocks.state_at((pos.0, pos.1 + 1, pos.2));
        let up = self.fluid_at_state(&above_state);
        if !up.is_empty()
            && up.kind == self.kind
            && self.can_pass_through_wall(crate::interact::Face::Up, state, &above_state)
        {
            return FluidState::flowing(self.kind, 8, true);
        }
        let drop = self.drop_off();
        if max_amount <= drop {
            return NOTHING;
        }
        FluidState::flowing(self.kind, max_amount - drop, false)
    }

    /// `FlowingFluid.getSlopeDistance(level, pos, depth, exclude, state, ctx)`
    /// — how far away the nearest hole is, searching outward from `pos` and
    /// never back the way it came. [`NO_SLOPE`] when nothing is found within
    /// `getSlopeFindDistance`.
    fn slope_distance(
        &self,
        pos: crate::interact::BlockPos,
        depth: i32,
        exclude: crate::interact::Face,
        state: &str,
        ctx: &mut SpreadContext,
    ) -> i32 {
        let mut best = NO_SLOPE;
        for d in crate::interact::HORIZONTAL {
            if d == exclude {
                continue;
            }
            let to = d.relative(pos);
            let to_state = self.cached_state(ctx, to);
            let to_fluid = self.fluid_at_state(&to_state);
            if !self.can_pass_through(self.flowing_form(), d, state, &to_state, to_fluid) {
                continue;
            }
            if self.is_hole(ctx, to) {
                return depth;
            }
            if depth < self.slope_find_distance() {
                let inner = self.slope_distance(to, depth + 1, d.opposite(), &to_state, ctx);
                if inner < best {
                    best = inner;
                }
            }
        }
        best
    }

    fn cached_state(&self, ctx: &mut SpreadContext, pos: crate::interact::BlockPos) -> String {
        if let Some(s) = ctx.states.get(&pos) {
            return s.clone();
        }
        let s = self.blocks.state_at(pos);
        ctx.states.insert(pos, s.clone());
        s
    }

    /// `SpreadContext.isHole(pos)` — `isWaterHole(level, pos, state,
    /// pos.below(), belowState)`, memoised. The block below is read through
    /// the **level**, not the context's cache (`lambda$isHole$0`).
    fn is_hole(&self, ctx: &mut SpreadContext, pos: crate::interact::BlockPos) -> bool {
        if let Some(h) = ctx.holes.get(&pos) {
            return *h;
        }
        let state = self.cached_state(ctx, pos);
        let below_state = self.blocks.state_at((pos.0, pos.1 - 1, pos.2));
        let hole = self.is_water_hole(&state, &below_state);
        ctx.holes.insert(pos, hole);
        hole
    }

    /// `FlowingFluid.getSpread(level, pos, state)` — which horizontal
    /// directions the fluid goes and what it becomes in each.
    ///
    /// **Two orders, and they are not the same one.** The scan is
    /// `Plane.HORIZONTAL`, `NORTH, EAST, SOUTH, WEST`; the result is a
    /// `Maps.newEnumMap(Direction.class)`, iterated by the caller in ordinal
    /// order, `NORTH, SOUTH, WEST, EAST`. The scan order matters because a
    /// shorter slope **clears the map** (`Map.clear()` at offset 180).
    #[must_use]
    pub fn get_spread(
        &self,
        pos: crate::interact::BlockPos,
        state: &str,
    ) -> Vec<(crate::interact::Face, FluidState)> {
        let mut min_distance = NO_SLOPE;
        let mut out: std::collections::BTreeMap<i32, (crate::interact::Face, FluidState)> =
            std::collections::BTreeMap::new();
        let mut ctx = SpreadContext::default();
        for d in crate::interact::HORIZONTAL {
            let to = d.relative(pos);
            let to_state = self.blocks.state_at(to);
            let to_fluid = self.fluid_at_state(&to_state);
            if !self.can_maybe_pass_through(d, state, &to_state, to_fluid) {
                continue;
            }
            let new = self.get_new_liquid(to, &to_state);
            if !self.can_hold_specific_fluid(&to_state, new) {
                continue;
            }
            let distance = if self.is_hole(&mut ctx, to) {
                0
            } else {
                self.slope_distance(to, 1, d.opposite(), &to_state, &mut ctx)
            };
            if distance < min_distance {
                out.clear();
            }
            if distance <= min_distance {
                if self.can_be_replaced_with(to_fluid, to, new.kind, d) {
                    out.insert(d.to_3d(), (d, new));
                }
                min_distance = distance;
            }
        }
        out.into_values().collect()
    }

    /// `FluidState.canBeReplacedWith(level, pos, fluid, direction)` — asked
    /// of the fluid **already at** `pos`, about an incoming one.
    ///
    /// ```text
    ///   EmptyFluid:  true
    ///   WaterFluid:  direction == DOWN && !fluid.is(FluidTags.WATER)
    ///   LavaFluid:   state.getHeight(level, pos) >= 0.44444445f
    ///                  && fluid.is(FluidTags.WATER)
    /// ```
    ///
    /// They are **not** mirror images. Water yields only from above and only
    /// to lava — a lava fall onto water. Lava yields from any side to water,
    /// but only while it is at least four ninths high; the direction is not
    /// read at all.
    #[must_use]
    pub fn can_be_replaced_with(
        &self,
        existing: FluidState,
        at: crate::interact::BlockPos,
        incoming: Kind,
        direction: crate::interact::Face,
    ) -> bool {
        match existing.kind {
            Kind::Empty => true,
            Kind::Water => {
                direction == crate::interact::Face::Down && !matches!(incoming, Kind::Water)
            }
            Kind::Lava => {
                self.height(existing, at) >= LAVA_YIELDS_AT_HEIGHT
                    && matches!(incoming, Kind::Water)
            }
        }
    }
}

impl Flow<'_> {
    /// `FlowingFluid.spreadTo(level, pos, state, direction, fluidState)`,
    /// with `LavaFluid`'s override in front of it:
    ///
    /// ```text
    ///   LavaFluid.spreadTo, offsets 0-81:
    ///     if (direction == DOWN) {
    ///         FluidState f = level.getFluidState(pos);
    ///         if (this.is(FluidTags.LAVA) && f.is(FluidTags.WATER)) {
    ///             if (state.getBlock() instanceof LiquidBlock)
    ///                 level.setBlock(pos, Blocks.STONE.defaultBlockState(), 3);
    ///             fizz(level, pos);
    ///             return;
    ///         }
    ///     }
    ///     super.spreadTo(...);
    ///
    ///   FlowingFluid.spreadTo, offsets 0-65:
    ///     if (state.getBlock() instanceof LiquidBlockContainer c) {
    ///         c.placeLiquid(level, pos, state, fluidState);
    ///         return;
    ///     }
    ///     if (!state.isAir()) beforeDestroyingBlock(level, pos, state);
    ///     level.setBlock(pos, fluidState.createLegacyBlock(), 3);
    /// ```
    ///
    /// **Lava falling onto water is stone, and onto a waterlogged block is
    /// only a fizz** — the `instanceof LiquidBlock` guard sits inside the
    /// water test, so a waterlogged slab under a lava fall is left alone and
    /// still hisses.
    ///
    /// `beforeDestroyingBlock` is `LavaFluid.fizz` for lava and
    /// `Block.dropResources` for water. The lava half is here. **The water
    /// half is not ported and is named rather than skipped silently**: this
    /// server spawns no item entity from any block destruction on any path,
    /// so porting drops here alone would make fluid the one mechanic that
    /// drops.
    fn spread_to(
        &self,
        shapes: Option<&crate::shapes::ShapeTable>,
        pos: crate::interact::BlockPos,
        state: &str,
        direction: crate::interact::Face,
        fluid: FluidState,
        out: &mut crate::shapes::Effects,
    ) {
        if self.kind == Kind::Lava && direction == crate::interact::Face::Down {
            let there = self.fluid_at_state(state);
            if there.kind.is_water() {
                if is_liquid_block(state) {
                    crate::shapes::set_block(
                        self.blocks,
                        shapes,
                        pos,
                        "minecraft:stone",
                        crate::shapes::UPDATE_ALL,
                        crate::shapes::RECURSION_LIMIT,
                        out,
                    );
                }
                out.level_events.push((LEVEL_EVENT_LAVA_FIZZ, pos, 0));
                return;
            }
        }
        if is_liquid_container(state) {
            place_liquid(self.blocks, shapes, pos, state, fluid, out);
            return;
        }
        if !crate::shapes::is_air(state) && self.kind == Kind::Lava {
            // `LavaFluid.beforeDestroyingBlock` — `fizz`, and nothing else.
            out.level_events.push((LEVEL_EVENT_LAVA_FIZZ, pos, 0));
        }
        crate::shapes::set_block(
            self.blocks,
            shapes,
            pos,
            &fluid.legacy_block(),
            crate::shapes::UPDATE_ALL,
            crate::shapes::RECURSION_LIMIT,
            out,
        );
    }

    /// `FlowingFluid.spreadToSides(level, pos, fluidState, blockState)`.
    ///
    /// ```text
    ///   amount = fluidState.getAmount() - getDropOff(level);
    ///   if (fluidState.getValue(FALLING)) amount = 7;
    ///   if (amount <= 0) return;
    ///   for ((d, f) : getSpread(level, pos, blockState))
    ///       spreadTo(level, pos.relative(d), getBlockState(...), d, f);
    /// ```
    ///
    /// **`amount` is a gate and nothing else** — the amounts written come out
    /// of `getNewLiquid` inside `getSpread`.
    fn spread_to_sides(
        &self,
        shapes: Option<&crate::shapes::ShapeTable>,
        pos: crate::interact::BlockPos,
        fluid: FluidState,
        state: &str,
        out: &mut crate::shapes::Effects,
    ) {
        let amount = if fluid.falling {
            7
        } else {
            i32::from(fluid.amount()) - i32::from(self.drop_off())
        };
        if amount <= 0 {
            return;
        }
        for (d, f) in self.get_spread(pos, state) {
            let to = d.relative(pos);
            let to_state = self.blocks.state_at(to);
            self.spread_to(shapes, to, &to_state, d, f, out);
        }
    }

    /// `FlowingFluid.spread(level, pos, blockState, fluidState)`.
    ///
    /// ```text
    ///   if (fluidState.isEmpty()) return;
    ///   below = pos.below(); belowState = ...; belowFluid = ...;
    ///   if (canMaybePassThrough(level, pos, blockState, DOWN, below, belowState, belowFluid)) {
    ///       newBelow = getNewLiquid(level, below, belowState);
    ///       fluid = newBelow.getType();
    ///       if (belowFluid.canBeReplacedWith(level, below, fluid, DOWN)
    ///               && canHoldSpecificFluid(level, below, belowState, fluid)) {
    ///           spreadTo(level, below, belowState, DOWN, newBelow);
    ///           if (sourceNeighborCount(level, pos) >= 3)
    ///               spreadToSides(level, pos, fluidState, blockState);
    ///           return;
    ///       }
    ///   }
    ///   if (fluidState.isSource() || !isWaterHole(level, pos, blockState, below, belowState))
    ///       spreadToSides(level, pos, fluidState, blockState);
    /// ```
    pub fn spread(
        &self,
        shapes: Option<&crate::shapes::ShapeTable>,
        pos: crate::interact::BlockPos,
        state: &str,
        fluid: FluidState,
        out: &mut crate::shapes::Effects,
    ) {
        if fluid.is_empty() {
            return;
        }
        let below = (pos.0, pos.1 - 1, pos.2);
        let below_state = self.blocks.state_at(below);
        let below_fluid = self.fluid_at_state(&below_state);
        let down = crate::interact::Face::Down;
        if self.can_maybe_pass_through(down, state, &below_state, below_fluid) {
            let new_below = self.get_new_liquid(below, &below_state);
            if self.can_be_replaced_with(below_fluid, below, new_below.kind, down)
                && self.can_hold_specific_fluid(&below_state, new_below)
            {
                self.spread_to(shapes, below, &below_state, down, new_below, out);
                if self.source_neighbor_count(pos) >= 3 {
                    self.spread_to_sides(shapes, pos, fluid, state, out);
                }
                return;
            }
        }
        if fluid.is_source() || !self.is_water_hole(state, &below_state) {
            self.spread_to_sides(shapes, pos, fluid, state, out);
        }
    }

    /// `FlowingFluid.tick(level, pos, blockState, fluidState)` — one fluid
    /// tick, offsets 0-109.
    ///
    /// ```text
    ///   if (!fluidState.isSource()) {
    ///       next = getNewLiquid(level, pos, getBlockState(pos));
    ///       delay = getSpreadDelay(level, pos, fluidState, next);
    ///       if (next.isEmpty()) {
    ///           fluidState = next; blockState = AIR;
    ///           level.setBlock(pos, blockState, 3);
    ///       } else if (next != fluidState) {
    ///           fluidState = next; blockState = fluidState.createLegacyBlock();
    ///           level.setBlock(pos, blockState, 3);
    ///           level.scheduleTick(pos, fluidState.getType(), delay);
    ///       }
    ///   }
    ///   spread(level, pos, blockState, fluidState);
    /// ```
    ///
    /// **A source never re-derives itself**, and **`spread` runs on the NEW
    /// state**. Takes no queue and no clock: a tick it asks for is pushed
    /// onto `Effects::fluid_ticks` with a delay, and the drain site stamps it.
    pub fn tick(
        &self,
        shapes: Option<&crate::shapes::ShapeTable>,
        pos: crate::interact::BlockPos,
    ) -> crate::shapes::Effects {
        let mut out = crate::shapes::Effects::new();
        let mut state = self.blocks.state_at(pos);
        let mut fluid = self.fluid_at_state(&state);
        // `ServerLevel.tickFluid` runs `fluidState.tick(...)` only when the
        // fluid at the position is still the one the tick was booked for
        // (`fluidState.is(fluid)`); the caller checks that, this only refuses
        // a position holding some other liquid.
        if fluid.is_empty() || fluid.kind != self.kind {
            return out;
        }
        if !fluid.is_source() {
            let next = self.get_new_liquid(pos, &state);
            let delay = self.spread_delay(pos, fluid, next);
            if next.is_empty() {
                fluid = next;
                state = "minecraft:air".to_string();
                crate::shapes::set_block(
                    self.blocks,
                    shapes,
                    pos,
                    &state,
                    crate::shapes::UPDATE_ALL,
                    crate::shapes::RECURSION_LIMIT,
                    &mut out,
                );
            } else if next != fluid {
                fluid = next;
                state = fluid.legacy_block();
                crate::shapes::set_block(
                    self.blocks,
                    shapes,
                    pos,
                    &state,
                    crate::shapes::UPDATE_ALL,
                    crate::shapes::RECURSION_LIMIT,
                    &mut out,
                );
                out.fluid_ticks.push(crate::shapes::TickRequest {
                    pos,
                    block: fluid.type_name().to_string(),
                    delay,
                    priority: crate::ticks::PRIORITY_NORMAL,
                });
            }
        }
        self.spread(shapes, pos, &state, fluid, &mut out);
        out
    }
}

/// `state.getBlock() instanceof LiquidBlock` — `minecraft:water` and
/// `minecraft:lava`, the two `LiquidBlock`s 26.2 registers.
#[must_use]
pub fn is_liquid_block(state: &str) -> bool {
    matches!(
        state.split('[').next().unwrap_or(state),
        "minecraft:water" | "minecraft:lava"
    )
}

/// `state.getBlock() instanceof LiquidBlockContainer`, for the containers
/// whose `placeLiquid` can do anything: every `SimpleWaterloggedBlock`, which
/// is exactly the set of states carrying a `waterlogged` property.
///
/// Kelp, seagrass and the bubble column are `LiquidBlockContainer`s too, and
/// their `canPlaceLiquid`/`placeLiquid` answer `false` — so treating them as
/// non-containers changes nothing a fluid can do, because
/// `canHoldSpecificFluid` has already refused them upstream.
#[must_use]
pub fn is_liquid_container(state: &str) -> bool {
    crate::placement::property_of(state, "waterlogged").is_some()
}

/// `SimpleWaterloggedBlock.placeLiquid(level, pos, state, fluidState)`:
///
/// ```text
///   if (!state.getValue(WATERLOGGED) && fluidState.is(Fluids.WATER)) {   //  0-24
///       level.setBlock(pos, state.setValue(WATERLOGGED, true), 3);     // 36-53
///       level.scheduleTick(pos, fluidState.getType(),
///                          fluidState.getType().getTickDelay(level));  // 56-75
///       return true;
///   }
///   return false;
/// ```
///
/// `FluidState.is(Fluid)` is an identity test against the **source**, so a
/// flowing state never waterlogs anything.
pub fn place_liquid(
    blocks: &dyn crate::edits::Blocks,
    shapes: Option<&crate::shapes::ShapeTable>,
    pos: crate::interact::BlockPos,
    state: &str,
    fluid: FluidState,
    out: &mut crate::shapes::Effects,
) -> bool {
    if crate::placement::property_of(state, "waterlogged") != Some("false")
        || fluid.kind != Kind::Water
        || !fluid.is_source()
    {
        return false;
    }
    let wet = crate::placement::with_property(state, "waterlogged", "true");
    crate::shapes::set_block(
        blocks,
        shapes,
        pos,
        &wet,
        crate::shapes::UPDATE_ALL,
        crate::shapes::RECURSION_LIMIT,
        out,
    );
    out.fluid_ticks.push(crate::shapes::TickRequest {
        pos,
        block: WATER.to_string(),
        delay: WATER_TICK_DELAY,
        priority: crate::ticks::PRIORITY_NORMAL,
    });
    true
}

/// `LiquidBlock.POSSIBLE_FLOW_DIRECTIONS` — `ImmutableList.of(DOWN, SOUTH,
/// NORTH, EAST, WEST)` out of `LiquidBlock.<clinit>` offsets 43-61. Neither
/// `Direction.values()` nor `Plane.HORIZONTAL`, and the first match wins, so
/// the order picks which neighbour's water a lava block reacts to.
pub const POSSIBLE_FLOW_DIRECTIONS: [crate::interact::Face; 5] = [
    crate::interact::Face::Down,
    crate::interact::Face::South,
    crate::interact::Face::North,
    crate::interact::Face::East,
    crate::interact::Face::West,
];

/// `LiquidBlock.shouldSpreadLiquid(level, pos, state)`, offsets 0-170 —
/// **the lava/water interaction**, and it writes.
///
/// ```text
///   if (fluid.is(FluidTags.LAVA)) {
///       boolean soulSoil = level.getBlockState(pos.below()).is(Blocks.SOUL_SOIL);
///       for (Direction d : POSSIBLE_FLOW_DIRECTIONS) {
///           BlockPos n = pos.relative(d.getOpposite());
///           if (level.getFluidState(n).is(FluidTags.WATER)) {
///               Block b = level.getFluidState(pos).isSource() ? OBSIDIAN : COBBLESTONE;
///               level.setBlockAndUpdate(pos, b.defaultBlockState());
///               fizz(level, pos);
///               return false;
///           }
///           if (soulSoil && level.getBlockState(n).is(Blocks.BLUE_ICE)) {
///               level.setBlockAndUpdate(pos, Blocks.BASALT.defaultBlockState());
///               fizz(level, pos);
///               return false;
///           }
///       }
///   }
///   return true;
/// ```
///
/// Water never reads the world here: the `LAVA` test fails at offset 10 and
/// jumps straight to `iconst_1`. **The neighbour is `pos.relative(
/// d.getOpposite())`** — the list is of flow directions *into* this block, so
/// `DOWN` asks about the block **above**. The source/flowing test reads the
/// lava at `pos`, not the water. `setBlockAndUpdate` is flags 3.
pub fn should_spread_liquid(
    blocks: &dyn crate::edits::Blocks,
    shapes: Option<&crate::shapes::ShapeTable>,
    pos: crate::interact::BlockPos,
    state: &str,
    out: &mut crate::shapes::Effects,
) -> bool {
    let here = blocks.fluid_of_state(state);
    if !here.kind.is_lava() {
        return true;
    }
    let soul_soil =
        block_name(&blocks.state_at((pos.0, pos.1 - 1, pos.2))) == "minecraft:soul_soil";
    for d in POSSIBLE_FLOW_DIRECTIONS {
        let n = d.opposite().relative(pos);
        if blocks.fluid_at(n).kind.is_water() {
            let into = if here.is_source() {
                "minecraft:obsidian"
            } else {
                "minecraft:cobblestone"
            };
            crate::shapes::set_block(
                blocks,
                shapes,
                pos,
                into,
                crate::shapes::UPDATE_ALL,
                crate::shapes::RECURSION_LIMIT,
                out,
            );
            out.level_events.push((LEVEL_EVENT_LAVA_FIZZ, pos, 0));
            return false;
        }
        if soul_soil && block_name(&blocks.state_at(n)) == "minecraft:blue_ice" {
            crate::shapes::set_block(
                blocks,
                shapes,
                pos,
                "minecraft:basalt",
                crate::shapes::UPDATE_ALL,
                crate::shapes::RECURSION_LIMIT,
                out,
            );
            out.level_events.push((LEVEL_EVENT_LAVA_FIZZ, pos, 0));
            return false;
        }
    }
    true
}

fn block_name(state: &str) -> &str {
    state.split('[').next().unwrap_or(state)
}

/// `FlowingFluid.getTickDelay(level)` for the liquid a `LiquidBlock` state
/// holds, in a level with or without `FAST_LAVA` — `this.fluid.getTickDelay`
/// in `LiquidBlock`, where `this.fluid` is the block's **source** instance and
/// so the delay depends only on the liquid.
#[must_use]
pub const fn liquid_tick_delay(kind: Kind, fast_lava: bool) -> i64 {
    match kind {
        Kind::Lava if fast_lava => FAST_LAVA_TICK_DELAY,
        Kind::Lava => LAVA_TICK_DELAY,
        _ => WATER_TICK_DELAY,
    }
}

/// `LiquidBlock.onPlace` **and** `LiquidBlock.neighborChanged` — one
/// function because the bytecode is the same lines twice (offsets 0-30 of
/// each):
///
/// ```text
///   if (shouldSpreadLiquid(level, pos, state))
///       level.scheduleTick(pos, state.getFluidState().getType(),
///                          this.fluid.getTickDelay(level));
/// ```
///
/// **These are the lines that start every flow and every interaction.**
/// `onPlace` books the first tick when a `LiquidBlock` state is written — a
/// bucket, a spread — and `neighborChanged` books one when something around
/// a settled fluid changes. For lava, `shouldSpreadLiquid` is where obsidian
/// and cobblestone come from, so it can write instead of scheduling.
///
/// The bubble-column tail of both is not ported: `BubbleColumnBlock` does not
/// exist in this server and `shouldBubbleColumnOccupy` is false for every
/// state it can hold.
pub fn on_place_or_neighbor_changed(
    blocks: &dyn crate::edits::Blocks,
    shapes: Option<&crate::shapes::ShapeTable>,
    pos: crate::interact::BlockPos,
    state: &str,
    out: &mut crate::shapes::Effects,
) {
    if !is_liquid_block(state) {
        return;
    }
    let fluid = blocks.fluid_of_state(state);
    if fluid.is_empty() {
        return;
    }
    if should_spread_liquid(blocks, shapes, pos, state, out) {
        out.fluid_ticks.push(crate::shapes::TickRequest {
            pos,
            block: fluid.type_name().to_string(),
            delay: liquid_tick_delay(fluid.kind, blocks.fast_lava()),
            priority: crate::ticks::PRIORITY_NORMAL,
        });
    }
}

/// `LiquidBlock.updateShape`'s side effect, offsets 0-39:
///
/// ```text
///   if (state.getFluidState().isSource() || neighborState.getFluidState().isSource())
///       tickAccess.scheduleTick(pos, state.getFluidState().getType(),
///                               this.fluid.getTickDelay(level));
///   ... bubble column ...
///   return super.updateShape(...);    // the block's own state
/// ```
///
/// It returns the liquid's own state, which is why `update_shape.json` holds
/// no `minecraft:water` rows that schedule: the extractor drops a
/// `(state, direction)` whose answer is only itself. Asked here directly, as
/// the observer's and the falling block's are.
pub fn liquid_update_shape(
    blocks: &dyn crate::edits::Blocks,
    pos: crate::interact::BlockPos,
    state: &str,
    neighbour_state: &str,
    out: &mut crate::shapes::Effects,
) {
    if !is_liquid_block(state) {
        return;
    }
    let fluid = blocks.fluid_of_state(state);
    if fluid.is_empty() {
        return;
    }
    if fluid.is_source() || blocks.fluid_of_state(neighbour_state).is_source() {
        out.fluid_ticks.push(crate::shapes::TickRequest {
            pos,
            block: fluid.type_name().to_string(),
            delay: liquid_tick_delay(fluid.kind, blocks.fast_lava()),
            priority: crate::ticks::PRIORITY_NORMAL,
        });
    }
}

/// `ServerLevel.tickFluid(pos, fluid)`:
///
/// ```text
///   FluidState fs = getFluidState(pos);
///   if (fs.is(fluid)) fs.tick(this, pos, getBlockState(pos));
/// ```
///
/// `is(Fluid)` is identity against the **concrete** instance the tick was
/// booked for, so a `flowing_water` tick at a position that has since become
/// a source does nothing — the source's own tick is a separate entry.
pub fn tick_fluid(
    rules: &Rules,
    blocks: &dyn crate::edits::Blocks,
    shapes: Option<&crate::shapes::ShapeTable>,
    pos: crate::interact::BlockPos,
    booked: &str,
) -> crate::shapes::Effects {
    let fs = blocks.fluid_at(pos);
    if fs.is_empty() || fs.type_name() != booked {
        return crate::shapes::Effects::new();
    }
    Flow::in_level(fs.kind, rules, blocks, blocks.fast_lava()).tick(shapes, pos)
}

#[cfg(test)]
mod spread_tests {
    use super::*;
    use crate::edits::Blocks;
    use crate::interact::{BlockPos, Face};
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    /// A flat world of air over stone, with the smallest state table that can
    /// answer the port's questions.
    ///
    /// The ids are the test's own numbering and the [`Rules`] below is built
    /// against them, so the two cannot drift: `synthetic_rules` reads
    /// [`Fake::id_of`].
    struct Fake {
        blocks: Mutex<BTreeMap<BlockPos, String>>,
        floor: i32,
    }

    impl Fake {
        /// air 0, stone 1, `water[level=N]` 2+N, `lava[level=N]` 18+N, then
        /// the five solids the lava/water rules write or read.
        const STATES: usize = 39;
        const SOLIDS: [&'static str; 5] = [
            "minecraft:obsidian",
            "minecraft:cobblestone",
            "minecraft:soul_soil",
            "minecraft:blue_ice",
            "minecraft:basalt",
        ];

        fn id_of(state: &str) -> Option<i32> {
            if state == "minecraft:air" {
                return Some(0);
            }
            if state == "minecraft:stone" {
                return Some(1);
            }
            if let Some(i) = Self::SOLIDS.iter().position(|s| *s == state) {
                return i32::try_from(34 + i).ok();
            }
            let (base, rest) = if let Some(r) = state.strip_prefix("minecraft:water[level=") {
                (2, r)
            } else {
                (18, state.strip_prefix("minecraft:lava[level=")?)
            };
            let level = rest.strip_suffix(']')?;
            level
                .parse::<i32>()
                .ok()
                .filter(|n| *n < 16)
                .map(|n| base + n)
        }

        /// The states that are a full cube and hold no fluid.
        fn is_solid_id(id: usize) -> bool {
            id == 1 || id >= 34
        }

        fn new(floor: i32) -> Self {
            Self {
                blocks: Mutex::new(BTreeMap::new()),
                floor,
            }
        }
    }

    impl Blocks for Fake {
        fn state_at(&self, pos: BlockPos) -> String {
            if let Some(s) = self.blocks.lock().expect("fake").get(&pos) {
                return s.clone();
            }
            if pos.1 <= self.floor {
                "minecraft:stone".to_string()
            } else {
                "minecraft:air".to_string()
            }
        }
        fn set_block(&self, pos: BlockPos, state: &str) -> Option<i32> {
            if self.state_at(pos) == state {
                return None;
            }
            self.blocks
                .lock()
                .expect("fake")
                .insert(pos, state.to_string());
            Self::id_of(state)
        }
        fn default_state(&self, block: &str) -> Option<String> {
            Some(block.to_string())
        }
        fn has_state(&self, state: &str) -> bool {
            Self::id_of(state).is_some()
        }
        fn state_id(&self, state: &str) -> Option<i32> {
            Self::id_of(state)
        }
        fn face_sturdy(&self, pos: BlockPos, _: Face) -> bool {
            Self::id_of(&self.state_at(pos))
                .and_then(|i| usize::try_from(i).ok())
                .is_some_and(Self::is_solid_id)
        }
        fn redstone_conductor(&self, _: BlockPos) -> bool {
            false
        }
        fn signal_source(&self, _: BlockPos) -> bool {
            false
        }
        fn replaceable(&self, pos: BlockPos, _: &crate::placement::Replacing<'_>) -> bool {
            self.state_at(pos) == "minecraft:air"
        }
        fn water_at(&self, pos: BlockPos) -> bool {
            self.fluid_at(pos).kind.is_water()
        }
        fn legacy_fluid_at(&self, pos: BlockPos) -> String {
            self.fluid_at(pos).legacy_block()
        }
        fn fluid_of_state(&self, state: &str) -> FluidState {
            of_state(state, None)
        }
        fn solid_of_state(&self, state: &str) -> bool {
            Self::id_of(state)
                .and_then(|i| usize::try_from(i).ok())
                .is_some_and(Self::is_solid_id)
        }
    }

    /// A [`Rules`] over [`Fake`]'s eighteen states: stone is a full collision
    /// block that holds nothing, everything else is empty and holds anything.
    ///
    /// Built through [`Rules::from_json`] rather than by constructing the
    /// struct, so the loader is exercised too — a table this port could not
    /// load is a table it would silently answer `false` from.
    fn synthetic_rules() -> Rules {
        let n = Fake::STATES;
        let hex = |bits: &[bool]| -> String {
            let mut out = String::new();
            for chunk in bits.chunks(4) {
                let mut nibble = 0u32;
                for i in 0..4 {
                    nibble = (nibble << 1) | u32::from(*chunk.get(i).unwrap_or(&false));
                }
                out.push(char::from_digit(nibble, 16).expect("nibble"));
            }
            out
        };
        // The solids hold nothing; air and every liquid level hold anything.
        let holds: Vec<bool> = (0..n).map(|i| !Fake::is_solid_id(i)).collect();
        // Two collision-shape groups: 0 empty, 1 the full cube.
        let groups: Vec<i32> = (0..n).map(|i| i32::from(Fake::is_solid_id(i))).collect();
        // `canPassThroughWall` is true only between two empty shapes, which
        // is `canPassThroughWall`'s own `Shapes.empty()` arm at offset 76.
        let wall: Vec<bool> = (0..2 * 2).map(|i| i == 0).collect();
        let wall_hex = hex(&wall);
        let holds_hex = hex(&holds);
        let doc = format!(
            r#"{{
              "states": {n},
              "shapeGroups": 2,
              "directionOrder": ["DOWN", "UP", "NORTH", "SOUTH", "WEST", "EAST"],
              "canHoldAnyFluid": "{holds_hex}",
              "canHoldWater": "{holds_hex}",
              "canHoldFlowingWater": "{holds_hex}",
              "canHoldLava": "{holds_hex}",
              "canHoldFlowingLava": "{holds_hex}",
              "stateGroup": [{}],
              "canPassThroughWall": {{
                "DOWN": "{wall_hex}", "UP": "{wall_hex}", "NORTH": "{wall_hex}",
                "SOUTH": "{wall_hex}", "WEST": "{wall_hex}", "EAST": "{wall_hex}"
              }}
            }}"#,
            groups
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        );
        let value: serde_json::Value = serde_json::from_str(&doc).expect("synthetic table is json");
        Rules::from_json(&value).expect("synthetic table loads")
    }

    /// **The whole mechanic, end to end: a source on flat ground spreads four
    /// ways at level 1 and books four more ticks.**
    ///
    /// Before this branch there was no queue, no rule and no bucket, and this
    /// test could not be written. The amounts are the port's own arithmetic:
    /// `getNewLiquid` at each neighbour sees the source's amount 8, subtracts
    /// `getDropOff` 1, and writes `getFlowing(7, false)` — whose
    /// `getLegacyLevel` is `8 - 7 = 1`.
    #[test]
    fn a_source_on_flat_ground_spreads_four_ways_at_level_one() {
        let world = Fake::new(63);
        let rules = synthetic_rules();
        let flow = Flow::new(Kind::Water, &rules, &world);
        let pos = (0, 64, 0);
        world.set_block(pos, "minecraft:water[level=0]");
        let effects = flow.tick(None, pos);

        for d in crate::interact::HORIZONTAL {
            assert_eq!(
                world.state_at(d.relative(pos)),
                "minecraft:water[level=1]",
                "{d:?} of the source"
            );
        }
        assert_eq!(
            world.state_at(pos),
            "minecraft:water[level=0]",
            "the source itself is untouched — `tick` skips getNewLiquid for one"
        );
        assert_eq!(
            world.state_at((0, 63, 0)),
            "minecraft:stone",
            "the floor is not replaced"
        );
        // **Each write books its own tick through `LiquidBlock.onPlace`**, at
        // the concrete `flowing_water` instance and water's delay. The other
        // half of vanilla's count — `updateNeighborsAt` reaching the source's
        // `neighborChanged`, and the source's `updateShape` — needs the signal
        // and shape tables, which this double does not load, so it is
        // asserted where those are: in the corpus.
        let booked: Vec<(BlockPos, &str, i64)> = effects
            .fluid_ticks
            .iter()
            .map(|t| (t.pos, t.block.as_str(), t.delay))
            .collect();
        for d in crate::interact::HORIZONTAL {
            assert_eq!(
                booked
                    .iter()
                    .filter(|b| **b == (d.relative(pos), FLOWING_WATER, WATER_TICK_DELAY))
                    .count(),
                1,
                "{d:?}: one onPlace per block written: {booked:?}"
            );
        }
        // The rest are `LiquidBlock.neighborChanged` at the source, one per
        // write — reached only through `Level.updateNeighborsAt`, which this
        // crate runs when the signal table is loaded. Both environments are
        // asserted rather than one skipped.
        let at_source: Vec<_> = booked.iter().filter(|b| b.0 == pos).collect();
        let neighbour_updates = crate::power::shared().is_some();
        assert_eq!(
            at_source.len(),
            if neighbour_updates { 4 } else { 0 },
            "{booked:?}"
        );
        assert!(at_source
            .iter()
            .all(|b| b.1 == WATER && b.2 == WATER_TICK_DELAY));
        assert_eq!(
            booked.len(),
            4 + at_source.len(),
            "nothing else: {booked:?}"
        );
        assert!(effects.ticks.is_empty(), "nothing enters the block queue");
        let queue = crate::ticks::ScheduledTicks::new();
        crate::ticks::drain_effects(&queue, &effects, 0);
        // `UNIQUE_TICK_HASH` collapses the four source bookings to one.
        assert_eq!(queue.fluid_len(), 4 + usize::from(neighbour_updates));
        assert_eq!(queue.len(), 0);
    }

    /// **The writes come out in `EnumMap` order, not in scan order.**
    ///
    /// `getSpread` scans `Plane.HORIZONTAL` — NORTH, EAST, SOUTH, WEST — and
    /// returns a `Maps.newEnumMap(Direction.class)`, which `spreadToSides`
    /// iterates as `entrySet`. An `EnumMap` iterates by ordinal, so the
    /// writes are NORTH, SOUTH, WEST, EAST. Two orders in one mechanic, and
    /// a port that used one for both is wrong in whichever half it reused.
    #[test]
    fn the_writes_are_in_ordinal_order_and_the_scan_is_not() {
        let world = Fake::new(63);
        let rules = synthetic_rules();
        let flow = Flow::new(Kind::Water, &rules, &world);
        let pos = (0, 64, 0);
        // The source has to be in the world, not just named: `getSpread`
        // asks `getNewLiquid` at each neighbour, which reads the world back.
        // Without this the answer is legitimately empty — which is what the
        // first run of this test measured, and is the right answer to the
        // wrong question.
        world.set_block(pos, "minecraft:water[level=0]");
        let spread = flow.get_spread(pos, "minecraft:water[level=0]");
        let order: Vec<Face> = spread.iter().map(|(d, _)| *d).collect();
        assert_eq!(
            order,
            vec![Face::North, Face::South, Face::West, Face::East],
            "EnumMap order"
        );
        assert_ne!(
            order,
            crate::interact::HORIZONTAL.to_vec(),
            "and it is NOT the scan order the same function used"
        );
    }

    /// **The second wave decays.** A source spreads at amount 7; the blocks
    /// it wrote spread at 6, and so on until `getNewLiquid` returns empty.
    /// Water's reach is therefore seven blocks — which, from a corpus anchor
    /// on a four-chunk corner, reaches into all four chunks.
    #[test]
    fn water_reaches_seven_blocks_and_then_stops() {
        let world = Fake::new(63);
        let rules = synthetic_rules();
        let flow = Flow::new(Kind::Water, &rules, &world);
        let pos = (0, 64, 0);
        world.set_block(pos, "minecraft:water[level=0]");
        // Run the queue to quiescence rather than a fixed number of waves,
        // so the answer is the mechanic's and not the loop's.
        let queue = crate::ticks::ScheduledTicks::new();
        let mut effects = flow.tick(None, pos);
        crate::ticks::drain_effects(&queue, &effects, 0);
        let mut now = 0;
        for _ in 0..64 {
            now += WATER_TICK_DELAY;
            let due = queue.drain_due_fluid(now);
            if due.is_empty() {
                break;
            }
            for tick in due {
                effects = flow.tick(None, tick.pos);
                crate::ticks::drain_effects(&queue, &effects, now);
            }
        }
        // Along +x: level 1 at distance 1 up to level 7 at distance 7, then
        // air.
        for d in 1..=7i32 {
            assert_eq!(
                world.state_at((d, 64, 0)),
                format!("minecraft:water[level={d}]"),
                "{d} blocks from the source"
            );
        }
        assert_eq!(
            world.state_at((8, 64, 0)),
            "minecraft:air",
            "eight blocks out is dry — `getNewLiquid` returned empty"
        );
        assert!(
            queue.fluid_len() == 0,
            "the queue drained: a spread that never stops is the other failure"
        );
    }

    /// Run a world's fluid queue to quiescence from one tick at `pos`.
    fn settle(world: &Fake, rules: &Rules, kind: Kind, pos: BlockPos) {
        let queue = crate::ticks::ScheduledTicks::new();
        let flow = Flow::new(kind, rules, world);
        let effects = flow.tick(None, pos);
        crate::ticks::drain_effects(&queue, &effects, 0);
        let mut now = 0;
        for _ in 0..400 {
            now += 1;
            for tick in queue.drain_due_fluid(now) {
                let effects = tick_fluid(rules, world, None, tick.pos, &tick.block);
                crate::ticks::drain_effects(&queue, &effects, now);
            }
            if queue.fluid_len() == 0 {
                break;
            }
        }
        assert_eq!(queue.fluid_len(), 0, "the queue drained");
    }

    /// `LavaFluid`'s three per-dimension numbers, out of `isFastLava`:
    /// tick delay 30/10, drop-off 2/1, slope search 2/4. Water reads none of
    /// it.
    #[test]
    fn lava_reads_fast_lava_and_water_does_not() {
        let world = Fake::new(63);
        let rules = synthetic_rules();
        let mut lava = Flow::new(Kind::Lava, &rules, &world);
        assert_eq!(
            (
                lava.tick_delay(),
                lava.drop_off(),
                lava.slope_find_distance()
            ),
            (30, 2, 2)
        );
        lava.fast_lava = true;
        assert_eq!(
            (
                lava.tick_delay(),
                lava.drop_off(),
                lava.slope_find_distance()
            ),
            (10, 1, 4)
        );
        let mut water = Flow::new(Kind::Water, &rules, &world);
        water.fast_lava = true;
        assert_eq!(
            (
                water.tick_delay(),
                water.drop_off(),
                water.slope_find_distance()
            ),
            (5, 1, 4)
        );
        assert!(water.convert_to_source && !lava.convert_to_source);
    }

    /// **Overworld lava reaches three blocks**: amounts 6, 4, 2 — legacy
    /// levels 2, 4, 6 — and the fourth is dry, because 2 minus the drop-off
    /// of 2 is not above zero.
    #[test]
    fn overworld_lava_reaches_three_blocks() {
        let world = Fake::new(63);
        let rules = synthetic_rules();
        let pos = (0, 64, 0);
        world.set_block(pos, "minecraft:lava[level=0]");
        settle(&world, &rules, Kind::Lava, pos);
        for (d, level) in [(1, 2), (2, 4), (3, 6)] {
            assert_eq!(
                world.state_at((d, 64, 0)),
                format!("minecraft:lava[level={level}]"),
                "{d} blocks out"
            );
        }
        assert_eq!(world.state_at((4, 64, 0)), "minecraft:air");
    }

    /// `shouldSpreadLiquid`: a lava **source** with water beside it becomes
    /// obsidian, a **flowing** lava block becomes cobblestone, and both fizz
    /// (`levelEvent(1501, pos, 0)`) and book no tick.
    #[test]
    fn lava_beside_water_is_obsidian_from_a_source_and_cobblestone_from_a_flow() {
        let world = Fake::new(63);
        world.set_block((1, 64, 0), "minecraft:water[level=0]");
        for (lava, into) in [
            ("minecraft:lava[level=0]", "minecraft:obsidian"),
            ("minecraft:lava[level=2]", "minecraft:cobblestone"),
        ] {
            world.set_block((0, 64, 0), lava);
            let mut out = crate::shapes::Effects::new();
            on_place_or_neighbor_changed(&world, None, (0, 64, 0), lava, &mut out);
            assert_eq!(world.state_at((0, 64, 0)), into, "{lava}");
            assert_eq!(
                out.level_events,
                vec![(LEVEL_EVENT_LAVA_FIZZ, (0, 64, 0), 0)]
            );
            // The lava's own tick is not booked — `shouldSpreadLiquid`
            // returned false. (The water beside it may book one: the new
            // block's `updateNeighborsAt` reaches its `neighborChanged`.)
            assert!(
                !out.fluid_ticks.iter().any(|t| t.pos == (0, 64, 0)),
                "a reaction books no tick where it happened: {:?}",
                out.fluid_ticks
            );
        }
        // Water itself never reacts: `shouldSpreadLiquid` is `true` for it
        // without reading the world, and it books its tick.
        let mut out = crate::shapes::Effects::new();
        on_place_or_neighbor_changed(
            &world,
            None,
            (1, 64, 0),
            "minecraft:water[level=0]",
            &mut out,
        );
        assert_eq!(world.state_at((1, 64, 0)), "minecraft:water[level=0]");
        assert_eq!(out.fluid_ticks.len(), 1);
        assert_eq!(out.fluid_ticks[0].block, WATER);
    }

    /// **The neighbour is `pos.relative(d.getOpposite())`** — `DOWN` in
    /// `POSSIBLE_FLOW_DIRECTIONS` asks about the block ABOVE. Water directly
    /// below a lava block does not touch it off; water above does.
    #[test]
    fn water_below_lava_does_not_react_and_water_above_does() {
        let world = Fake::new(63);
        world.set_block((0, 64, 0), "minecraft:water[level=0]");
        world.set_block((0, 65, 0), "minecraft:lava[level=0]");
        let mut out = crate::shapes::Effects::new();
        on_place_or_neighbor_changed(
            &world,
            None,
            (0, 65, 0),
            "minecraft:lava[level=0]",
            &mut out,
        );
        assert_eq!(world.state_at((0, 65, 0)), "minecraft:lava[level=0]");
        assert_eq!(out.fluid_ticks.len(), 1, "it spreads instead");
        assert_eq!(out.fluid_ticks[0].delay, LAVA_TICK_DELAY);
        world.set_block((0, 66, 0), "minecraft:water[level=0]");
        let mut out = crate::shapes::Effects::new();
        on_place_or_neighbor_changed(
            &world,
            None,
            (0, 65, 0),
            "minecraft:lava[level=0]",
            &mut out,
        );
        assert_eq!(world.state_at((0, 65, 0)), "minecraft:obsidian");
    }

    /// Lava over soul soil beside blue ice is basalt.
    #[test]
    fn lava_on_soul_soil_beside_blue_ice_is_basalt() {
        let world = Fake::new(63);
        world.set_block((0, 63, 0), "minecraft:soul_soil");
        world.set_block((1, 64, 0), "minecraft:blue_ice");
        world.set_block((0, 64, 0), "minecraft:lava[level=0]");
        let mut out = crate::shapes::Effects::new();
        on_place_or_neighbor_changed(
            &world,
            None,
            (0, 64, 0),
            "minecraft:lava[level=0]",
            &mut out,
        );
        assert_eq!(world.state_at((0, 64, 0)), "minecraft:basalt");
        assert_eq!(
            out.level_events,
            vec![(LEVEL_EVENT_LAVA_FIZZ, (0, 64, 0), 0)]
        );
    }

    /// `LavaFluid.spreadTo` DOWN into water: the water block becomes stone
    /// and fizzes — the lava does not go there.
    #[test]
    fn lava_falling_onto_water_makes_stone() {
        let world = Fake::new(62);
        let rules = synthetic_rules();
        world.set_block((0, 63, 0), "minecraft:water[level=0]");
        world.set_block((0, 64, 0), "minecraft:lava[level=0]");
        let flow = Flow::new(Kind::Lava, &rules, &world);
        let out = flow.tick(None, (0, 64, 0));
        assert_eq!(world.state_at((0, 63, 0)), "minecraft:stone");
        assert!(out
            .level_events
            .contains(&(LEVEL_EVENT_LAVA_FIZZ, (0, 63, 0), 0)));
    }

    /// `canBeReplacedWith` is not symmetric. Lava yields to water from any
    /// side once it is at least 4/9 high (amount 4); water yields only from
    /// above and only to lava.
    #[test]
    fn lava_yields_to_water_by_height_and_water_to_lava_only_from_above() {
        let world = Fake::new(63);
        let rules = synthetic_rules();
        let flow = Flow::new(Kind::Water, &rules, &world);
        let at = (0, 64, 0);
        let lava4 = FluidState::flowing(Kind::Lava, 4, false);
        let lava3 = FluidState::flowing(Kind::Lava, 3, false);
        assert!(flow.can_be_replaced_with(lava4, at, Kind::Water, Face::North));
        assert!(!flow.can_be_replaced_with(lava3, at, Kind::Water, Face::North));
        assert!(!flow.can_be_replaced_with(lava4, at, Kind::Lava, Face::Down));
        let water = FluidState::flowing(Kind::Water, 7, false);
        assert!(flow.can_be_replaced_with(water, at, Kind::Lava, Face::Down));
        assert!(!flow.can_be_replaced_with(water, at, Kind::Lava, Face::North));
        assert!(!flow.can_be_replaced_with(water, at, Kind::Water, Face::Down));
        assert!(flow.can_be_replaced_with(NOTHING, at, Kind::Water, Face::North));
    }

    /// `tickFluid`'s `fs.is(fluid)` is identity against the booked instance:
    /// a `flowing_water` tick at a position that is now a source does
    /// nothing.
    #[test]
    fn a_tick_booked_for_another_instance_does_nothing() {
        let world = Fake::new(63);
        let rules = synthetic_rules();
        world.set_block((0, 64, 0), "minecraft:water[level=0]");
        let out = tick_fluid(&rules, &world, None, (0, 64, 0), FLOWING_WATER);
        assert!(out.blocks.is_empty() && out.fluid_ticks.is_empty());
        let out = tick_fluid(&rules, &world, None, (0, 64, 0), WATER);
        assert_eq!(out.blocks.len(), 4, "the source's own tick spreads");
    }

    /// **`LavaFluid.getSpreadDelay`'s x4 is dead in vanilla.** A tick that
    /// changes a flowing block writes it first, and `LiquidBlock.onPlace`
    /// inside that write books `(pos, type)` at the plain tick delay; the
    /// tick's own `scheduleTick` with the spread delay comes second and
    /// `LevelChunkTicks.schedule` drops it. Here: a flowing lava block that
    /// rises from amount 2 to 4 books its 30 before any 120 can land.
    #[test]
    fn the_first_booking_wins_so_lavas_slow_delay_is_dead() {
        let world = Fake::new(63);
        let rules = synthetic_rules();
        // A lava source two blocks west of a level-6 (amount 2) block: the
        // middle cell holds amount 6, so the east cell becomes amount 4.
        world.set_block((0, 64, 0), "minecraft:lava[level=0]");
        world.set_block((1, 64, 0), "minecraft:lava[level=2]");
        world.set_block((2, 64, 0), "minecraft:lava[level=6]");
        let out = tick_fluid(&rules, &world, None, (2, 64, 0), FLOWING_LAVA);
        assert_eq!(world.state_at((2, 64, 0)), "minecraft:lava[level=4]");
        let here: Vec<i64> = out
            .fluid_ticks
            .iter()
            .filter(|t| t.pos == (2, 64, 0))
            .map(|t| t.delay)
            .collect();
        assert_eq!(
            here.first(),
            Some(&LAVA_TICK_DELAY),
            "onPlace books first: {here:?}"
        );
        let queue = crate::ticks::ScheduledTicks::new();
        crate::ticks::drain_effects(&queue, &out, 0);
        let due = queue.drain_due_fluid(LAVA_TICK_DELAY);
        assert!(
            due.iter().any(|t| t.pos == (2, 64, 0)),
            "the 30-tick booking is the one in the queue"
        );
    }

    /// `LiquidBlock.updateShape`: a source on either side books a tick at the
    /// liquid; two flowing blocks do not.
    #[test]
    fn a_liquid_update_shape_books_a_tick_when_either_side_is_a_source() {
        let world = Fake::new(63);
        let mut out = crate::shapes::Effects::new();
        liquid_update_shape(
            &world,
            (0, 64, 0),
            "minecraft:water[level=3]",
            "minecraft:water[level=2]",
            &mut out,
        );
        assert!(out.fluid_ticks.is_empty());
        liquid_update_shape(
            &world,
            (0, 64, 0),
            "minecraft:water[level=3]",
            "minecraft:water[level=0]",
            &mut out,
        );
        liquid_update_shape(
            &world,
            (0, 64, 0),
            "minecraft:lava[level=0]",
            "minecraft:air",
            &mut out,
        );
        assert_eq!(
            out.fluid_ticks
                .iter()
                .map(|t| (t.block.as_str(), t.delay))
                .collect::<Vec<_>>(),
            vec![(FLOWING_WATER, WATER_TICK_DELAY), (LAVA, LAVA_TICK_DELAY)]
        );
    }
}

/// [`Rules`] against the generated table, on states whose answers are read
/// straight off the bytecode — so a transposed column, a shifted bit or a
/// table keyed on the liquid rather than the instance is a red here and not a
/// river that flows the wrong way.
#[cfg(test)]
mod table_tests {
    use super::*;

    fn repo(relative: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(relative)
    }

    #[test]
    fn the_generated_table_answers_what_the_bytecode_says() {
        let rules_path = repo(RULES_PATH);
        let ids_path = repo("data/vanilla/block_props.json");
        if !rustmc_data::fixture::available(&rules_path, "fluid rules", "ci/fluid-rules-golden.sh")
            || !rustmc_data::fixture::available(
                &ids_path,
                "block props",
                "ci/block-props-golden.sh",
            )
        {
            return;
        }
        let rules = Rules::load(&rules_path).expect("fluid_rules.json loads");
        let ids = crate::terrain::StateIds::load(&ids_path).expect("block_props.json loads");
        let id = |s: &str| ids.get(s).unwrap_or_else(|e| panic!("{s}: {e}"));
        assert!(
            rules.len() > 30_000,
            "{} states — the table has narrowed",
            rules.len()
        );

        // `canHoldAnyFluid`: air and a liquid hold; a full cube does not.
        assert!(rules.can_hold_any_fluid(id("minecraft:air")));
        assert!(rules.can_hold_any_fluid(id("minecraft:water[level=3]")));
        assert!(!rules.can_hold_any_fluid(id("minecraft:stone")));

        // `SimpleWaterloggedBlock.canPlaceLiquid` is `fluid == Fluids.WATER`:
        // the dry fence takes the source and refuses the flowing instance,
        // and lava in either form.
        let fence =
            "minecraft:oak_fence[east=false,north=false,south=false,waterlogged=false,west=false]";
        let water = FluidState::source(Kind::Water, false);
        assert!(rules.can_hold_specific_fluid(id(fence), water));
        assert!(
            !rules.can_hold_specific_fluid(id(fence), FluidState::flowing(Kind::Water, 7, false))
        );
        assert!(!rules.can_hold_specific_fluid(id(fence), FluidState::source(Kind::Lava, false)));
        // A non-container answers `true` — `canHoldSpecificFluid`'s
        // fall-through `iconst_1`; `canHoldAnyFluid` is what refuses stone.
        assert!(rules.can_hold_specific_fluid(
            id("minecraft:air"),
            FluidState::flowing(Kind::Lava, 2, false)
        ));

        // `canPassThroughWall`: two empty shapes never occlude; a full cube
        // on either side always does.
        for face in [
            crate::interact::Face::Down,
            crate::interact::Face::North,
            crate::interact::Face::Up,
        ] {
            assert_eq!(
                rules.can_pass_through_wall(face, id("minecraft:air"), id("minecraft:air")),
                Some(true),
                "{face:?} air -> air"
            );
            assert_eq!(
                rules.can_pass_through_wall(face, id("minecraft:air"), id("minecraft:stone")),
                Some(false),
                "{face:?} air -> stone"
            );
        }
        // A bottom slab occludes downward flow out of itself (its bottom face
        // is full) and not sideways flow (its side faces are half).
        let slab = "minecraft:oak_slab[type=bottom,waterlogged=true]";
        assert_eq!(
            rules.can_pass_through_wall(crate::interact::Face::Down, id(slab), id("minecraft:air")),
            Some(false),
            "out of the slab's floor"
        );
        assert_eq!(
            rules.can_pass_through_wall(
                crate::interact::Face::North,
                id(slab),
                id("minecraft:air")
            ),
            Some(true),
            "out of the slab's side"
        );
    }
}
