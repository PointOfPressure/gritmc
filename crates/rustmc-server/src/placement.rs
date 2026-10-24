//! `Block.getStateForPlacement`, from a table vanilla filled in.
//!
//! # The problem, and how it is answered now
//!
//! `getStateForPlacement` is per block *class*, and `block_props.json`
//! carries block *states* — names and their properties — with no idea what
//! class produced them. For most properties that is fatal to a generic rule
//! and not obviously so, which is the trap. `facing` is the example: every
//! block with a horizontal `facing` looks alike in the state table, and
//!
//! ```text
//!   StairBlock          FACING = context.getHorizontalDirection()
//!   AbstractFurnaceBlock FACING = context.getHorizontalDirection().getOpposite()
//! ```
//!
//! are opposites. A generic "set `facing` from the player's yaw" is therefore
//! *backwards* for half the blocks that have it, which is worse than leaving
//! every one of them at the default: wrong-and-confident rather than
//! uniformly unfinished.
//!
//! So it is not a rule any more. `tools/goldengen/vanilla/PlacementGolden.java`
//! clicks every block in a grid of six faces, four yaws, three pitches and two
//! cursor heights and writes down what vanilla returns; `ci/placement-golden.sh`
//! runs it twice, demands the two agree, and produces
//! `data/vanilla/placement.json`. [`PlacementTable`] reads that file and
//! [`state_for_placement`] looks the answer up. A stair faces the placer and a
//! furnace faces away from them because vanilla said so, block by block.
//!
//! # What the table can and cannot answer
//!
//! The grid's axes are **derived vanilla quantities**, not raw angles, which
//! is what makes a 144-cell sample exact rather than approximate:
//!
//!   * the yaw column is `Direction.fromYRot(yaw)` — the same rounding-to-
//!     nearest-cardinal `getHorizontalDirection()` does, ported in
//!     [`from_y_rot`]. A real yaw of 47 degrees lands in the same column as
//!     the sampled 90, and the column is the answer for both.
//!   * the cursor column is `clickLocation.y - pos.getY() > 0.5`. Read from
//!     the jar rather than assumed: **four** block classes touch the click
//!     location — `DoorBlock`, `SlabBlock`, `StairBlock`, `TrapDoorBlock` —
//!     found by searching every class constant pool in the jar for
//!     `getClickLocation`. Three of them compare against 0.5 and nothing
//!     else; `DoorBlock` is the exception and does not, which is why the
//!     0.5 sentence is still true of the cursor column.
//!
//!     This said "exactly three" until 2026-08-05 and named the other three.
//!     `DoorBlock` reads the click location at offset 296, against the
//!     clicked position's **x**, to choose the hinge — a different question
//!     from the cursor column's, which is why the error was invisible here
//!     and showed up as a wrong count rather than a wrong placement.
//!
//! Three kinds of block the table cannot describe, each detected by the
//! dumper and each falling back to the default state here rather than
//! guessing:
//!
//!   * **pitch-sensitive** (10 blocks: dispensers, droppers, both pistons,
//!     observers, barrels, crafters, the three command blocks). These read
//!     `getNearestLookingDirection()`, whose boundary between "looking level"
//!     and "looking up" depends on the yaw as well as the pitch, so no column
//!     indexed by a rounded yaw can hold it. The pitch rows are in the file
//!     for whoever ports `Direction.orderedByNearest`.
//!   * **random** (3 blocks: weeping, twisting and cave vines).
//!     `GrowingPlantHeadBlock.getStateForPlacement` is
//!     `AGE = level.getRandom().nextInt(25)`; a table cannot hold a die roll,
//!     and the two-run check is what found them.
//!   * **not in the file at all** — 535 blocks whose every cell is their own
//!     default. Absent means "the default is the answer", which is what the
//!     fallback does anyway.
//!
//! And one limit that is not detected but is named: every cell was clicked
//! **into open air**, so the 195 blocks whose answer moves when they have
//! neighbours — fences, walls, panes, redstone, the second half of a double
//! chest — get the isolated answer. That is the right answer in the open and
//! the same one the default gives, so it costs nothing here; it will matter
//! when connections do.
//!
//! # Two properties are still computed, because the table cannot carry them
//!
//! `waterlogged` depends on the *world* rather than the click, and the grid's
//! world has no water in it, so it is applied on top of whatever the table
//! returned. `axis` is in the table and is also still computed, which is
//! idempotent — the table's axis comes from the same clicked face — and is
//! what answers a block the file does not carry.
//!
//! Both are read from the jar rather than assumed:
//!
//!   * **`axis`** — `RotatedPillarBlock.getStateForPlacement` is exactly
//!     `defaultBlockState().setValue(AXIS, context.getClickedFace().getAxis())`,
//!     and it is the base class of all 68 blocks whose `axis` takes x, y and
//!     z. (The 69th is `minecraft:nether_portal`, whose axis is x or z only
//!     and which no item places — the lookup guard below covers it without
//!     needing to know that.)
//!   * **`waterlogged`** — every `SimpleWaterloggedBlock` sets it from the
//!     fluid at the position being placed into, and only a water **source**
//!     counts: the test is `getType() == Fluids.WATER`, and flowing water's
//!     type is `Fluids.FLOWING_WATER`.
//!
//! # The guard that makes this safe to be wrong about
//!
//! A property is only overridden if the default state already has it, and the
//! result is only used if it is a state the table knows. So a block whose
//! `axis` cannot take the value this computes falls back to its default
//! rather than becoming a state that does not exist — which the encoder would
//! reject at the next chunk send, poisoning every future send of that chunk
//! rather than the one placement that caused it.

use std::collections::BTreeMap;
use std::io;

use crate::interact::{BlockPos, Face};

/// A water source. Flowing water is `level` 1-15 and does not waterlog.
pub const WATER_SOURCE: &str = "minecraft:water[level=0]";

/// The pitch row the lookup uses: index 1 of `[-90, 0, +90]`, the player
/// looking level.
///
/// Only the ten pitch-sensitive blocks can tell the difference, and they are
/// refused a table answer entirely rather than served this row — see the
/// module docs.
const PITCH_LEVEL: usize = 1;

/// `StairBlock`'s simple name, as `PlacedGolden` records it. Matched with
/// `is_a` so a subclass would be caught; 26.2 has none, and the point is that
/// the next version's would not need finding first.
pub(crate) const STAIR_BLOCK: &str = "StairBlock";

/// `ChestBlock`'s simple name. Covers `chest` and `trapped_chest` and NOT
/// `ender_chest`, which is an `EnderChestBlock` with no `type` property at
/// all — the class map draws that line for free where a name match would not.
pub(crate) const CHEST_BLOCK: &str = "ChestBlock";

/// `instanceof SlabBlock` — 64 blocks in 26.2.
pub(crate) const SLAB_BLOCK: &str = "SlabBlock";
/// `instanceof SnowLayerBlock` — one block, `minecraft:snow`. A class test
/// for a class with one member reads like overkill and is not: the rule is
/// the class's, and `minecraft:snow_block` is a different block that shares
/// most of the name and none of the behaviour.
pub(crate) const SNOW_LAYER_BLOCK: &str = "SnowLayerBlock";
/// `instanceof CandleBlock` — the seventeen candles. Not the candle cakes,
/// which are `CandleCakeBlock` and neither stack nor merge.
pub(crate) const CANDLE_BLOCK: &str = "CandleBlock";

/// The click a placement was made with, in the terms vanilla decides by.
#[derive(Debug, Clone, Copy)]
pub struct Click {
    /// `BlockHitResult.getDirection()`.
    pub face: Face,
    /// The player's yaw in degrees, straight off the move packet — rounded to
    /// a cardinal by [`from_y_rot`], exactly as `getHorizontalDirection()`
    /// does.
    pub yaw: f32,
    /// `clickLocation.y - getClickedPos().getY()`: the cursor's height above
    /// the corner of the block **being placed into**, not of the block that
    /// was clicked. The two differ whenever the placement steps along a
    /// vertical face, and `StairBlock` compares against the former.
    pub cursor_y: f64,
}

/// A state's block id — everything before the `[`.
///
/// Written out once here because five call sites in this module had their own
/// copy of `split_once('[')` and the rules added below need it six more
/// times.
fn name_of(state: &str) -> &str {
    state.split_once('[').map_or(state, |(b, _)| b)
}

/// `Item.BY_BLOCK`, built from the two item maps.
///
/// A free function so [`PlacementTable::load`] can call it before the struct
/// exists. Standing blocks first and wall blocks second, which is the order
/// `StandingAndWallBlockItem`'s constructor registers them in; with the
/// extract measured injective the order changes nothing, and it is fixed
/// rather than incidental so that a future collision resolves the same way
/// twice.
fn invert(
    items: &BTreeMap<String, String>,
    wall_items: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (item, block) in items.iter().chain(wall_items) {
        out.entry(block.clone()).or_insert_with(|| item.clone());
    }
    out
}

/// `BlockPlaceContext`, as much of it as `canBeReplaced` reads.
///
/// # The seeded flag is the subtle part
///
/// `BlockPlaceContext`'s constructor sets `replaceClicked = true` **before**
/// evaluating `getBlockState(clickedPos).canBeReplaced(this)` and only then
/// overwrites it with the answer (offsets 11-47). So a block being asked
/// whether it may be replaced *at the position that was clicked* sees
/// `replacingClickedOnBlock() == true` while it is being asked, and
/// `getClickedPos()` still returns the clicked position rather than the one
/// against the face. Both `SlabBlock` and `SnowLayerBlock` branch on that
/// flag, so a port that computed it first and passed the answer in would take
/// the wrong arm of both.
///
/// The second call — `canPlace()`, offsets 0-30 — is the one where the flag
/// is genuinely false, and it short-circuits to `true` when it is not.
#[derive(Debug, Clone, Copy)]
pub struct Replacing<'a> {
    /// `getItemInHand()`. `None` is an empty stack, which is the case the
    /// default rule admits and the two overrides refuse.
    pub item: Option<&'a str>,
    /// `getClickedFace()`.
    pub face: Face,
    /// `getClickLocation().y - getClickedPos().getY()` — the cursor's height
    /// within the **clicked** block, straight off the wire.
    ///
    /// Not [`Click::cursor_y`], which is measured from the block being placed
    /// *into*. The two differ exactly when the placement steps along the
    /// face, and this one is asked before that step has been decided.
    pub cursor_y: f64,
    /// `replacingClickedOnBlock()`. See the type's own docs.
    pub replacing_clicked: bool,
    /// `isSecondaryUseActive()` — the player is sneaking. Only
    /// `CandleBlock.canBeReplaced` reads it among the overrides here: a
    /// sneaking player stacks a candle beside a candle instead of adding one
    /// to it.
    pub secondary_use: bool,
    /// The placement extract: `Block.asItem()` for `ItemStack.is(asItem())`,
    /// and the class sets for the two overrides.
    ///
    /// `None` means every `is` is false and neither override is reachable, so
    /// the whole rule degrades to the bare `replaceable` flag it was before
    /// this existed — which is a behaviour this server has shipped and not a
    /// new failure mode.
    pub table: Option<&'a PlacementTable>,
}

impl Replacing<'_> {
    /// `ctx.getItemInHand().is(state.getBlock().asItem())`.
    fn item_is(&self, state: &str) -> bool {
        let Some(item) = self.item else {
            return false;
        };
        self.table
            .and_then(|t| t.item_for_block(name_of(state)))
            .is_some_and(|a| a == item)
    }
}

/// `BlockState.canBeReplaced(BlockPlaceContext)`.
///
/// `flag` is the no-argument `canBeReplaced()` — the `replaceable` boolean
/// `block_props.json` carries per state — and is what the default rule
/// consults. **The two overrides do not consult it at all**, which is why it
/// is a parameter rather than a first test here: `oak_slab` is
/// `replaceable = false` in the props table and a slab is nevertheless
/// replaceable by another slab.
///
/// ```text
///   BlockBehaviour.canBeReplaced(state, ctx), 0-36:
///     state.canBeReplaced() && (item.isEmpty() || !item.is(asItem()))
///
///   SlabBlock.canBeReplaced, 0-149:
///     if (type == DOUBLE || !item.is(asItem())) return false;
///     if (!ctx.replacingClickedOnBlock()) return true;
///     above = ctx.getClickLocation().y - ctx.getClickedPos().getY() > 0.5;
///     f = ctx.getClickedFace();
///     type == BOTTOM ? f == UP   || ( above && f.getAxis().isHorizontal())
///                    : f == DOWN || (!above && f.getAxis().isHorizontal());
///
///   SnowLayerBlock.canBeReplaced, 0-69:
///     layers = state.getValue(LAYERS);
///     if (item.is(asItem()) && layers < 8) {
///       return ctx.replacingClickedOnBlock() ? ctx.getClickedFace() == UP
///                                            : true;
///     }
///     return layers == 1;
/// ```
///
/// **The default rule's second conjunct is the one that is easy to miss and
/// it is not an optimisation.** Holding tall grass and clicking tall grass
/// does *not* replace it — the placement steps along the face instead — which
/// is the opposite of what the position-only rule this replaces answered.
///
/// Only two classes override this in a way any corpus scenario reaches. The
/// other overrides in 26.2 that matter to a placement — vines, big dripleaf,
/// sea pickle — are named in `docs/WORKLIST.md` and are not ported here:
/// each is its own rule, and a wrong one is worse than the default, which at
/// least errs towards *not* replacing.
#[must_use]
pub fn can_be_replaced(state: &str, flag: bool, ctx: &Replacing<'_>) -> bool {
    // `instanceof`, never "the name ends in `_slab`" — the same rule
    // `stairs_shape` applies, and for the same reason. 64 blocks answer
    // `SlabBlock` and one answers `SnowLayerBlock`.
    if ctx.table.is_some_and(|t| t.is_slab(state)) {
        return slab_can_be_replaced(state, ctx);
    }
    if ctx.table.is_some_and(|t| t.is_snow_layer(state)) {
        return snow_can_be_replaced(state, ctx);
    }
    // `CandleBlock.canBeReplaced`, offsets 0-39: not sneaking, holding this
    // very candle, and fewer than four — otherwise `super`, the default rule
    // below. The item test is `getItem() == asItem()`, an identity test,
    // which is what [`Replacing::item_is`] answers.
    if ctx.table.is_some_and(|t| t.is_candle(state))
        && !ctx.secondary_use
        && ctx.item_is(state)
        && property_of(state, "candles")
            .and_then(|v| v.parse::<u8>().ok())
            .is_some_and(|n| n < 4)
    {
        return true;
    }
    flag && !ctx.item_is(state)
}

/// `SlabBlock.canBeReplaced`, offsets 0-149.
fn slab_can_be_replaced(state: &str, ctx: &Replacing<'_>) -> bool {
    let ty = property_of(state, "type");
    if ty == Some("double") || !ctx.item_is(state) {
        return false;
    }
    if !ctx.replacing_clicked {
        return true;
    }
    let above = ctx.cursor_y > 0.5;
    let horizontal = ctx.face.axis_is_horizontal();
    if ty == Some("bottom") {
        ctx.face == Face::Up || (above && horizontal)
    } else {
        ctx.face == Face::Down || (!above && horizontal)
    }
}

/// `SnowLayerBlock.canBeReplaced`, offsets 0-69.
fn snow_can_be_replaced(state: &str, ctx: &Replacing<'_>) -> bool {
    let layers = property_of(state, "layers").and_then(|v| v.parse::<u8>().ok());
    // A snow state with no `layers` is not one this can answer for, and the
    // `layers == 1` tail is the honest fallback: `getValue` would have thrown
    // rather than defaulted, so anything that reaches here without the
    // property is already outside what vanilla can be asked.
    let layers = layers.unwrap_or(0);
    if ctx.item_is(state) && layers < 8 {
        return if ctx.replacing_clicked {
            ctx.face == Face::Up
        } else {
            true
        };
    }
    layers == 1
}

/// What the table has to say about one click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer<'a> {
    /// No usable answer: the block is absent from the file (its default is
    /// the answer), or its placement is random, or it reads a pitch the
    /// table's columns cannot represent.
    Unknown,
    /// Vanilla returns `null` here — a door with no room above, a crop on no
    /// farmland. Nothing is placed, and that is the answer rather than a
    /// failure.
    Refused,
    /// The state vanilla places.
    State(&'a str),
}

/// `getStateForPlacement`, per block, as extracted by
/// `ci/placement-golden.sh` into `data/vanilla/placement.json`.
#[derive(Debug, Default)]
pub struct PlacementTable {
    blocks: BTreeMap<String, Entry>,
    /// Item name to the block its `BlockItem` places.
    ///
    /// 1,054 of them, of which **14** are registered under a different name
    /// from their block: `minecraft:redstone` places `redstone_wire`,
    /// `minecraft:wheat_seeds` places `wheat`, `minecraft:string` places
    /// `tripwire`. Guessing from the item's own name is right for the other
    /// 1,040 and silently does nothing for these — and "nothing happens" is
    /// also the right answer for a sword, so the two are indistinguishable
    /// from a client. `BlockItem.getBlock()` is the real mapping and is in no
    /// generated report.
    items: BTreeMap<String, String>,
    /// Item name to the block it places on a **wall** — the second block a
    /// `StandingAndWallBlockItem` can put down.
    ///
    /// 61 of them. `BlockItem.getBlock()` returns only the standing variant,
    /// so a server working from that alone puts a floor torch on the side of
    /// a wall. The wall block is a private field on the item class and is in
    /// no generated report, so `PlacementGolden` reflects it out — and it has
    /// to, because guessing from the name gets `torch` -> `wall_torch` right
    /// and `skeleton_skull` -> `skeleton_wall_skull` wrong.
    wall_items: BTreeMap<String, String>,
    /// `Block.asItem()` — the inverse of the two maps above.
    ///
    /// `Item.BY_BLOCK` is populated by `BlockItem`'s constructor, one entry
    /// per block a `BlockItem` was registered for, so it is exactly the
    /// inverse of `blockItems` — plus the wall halves, which
    /// `StandingAndWallBlockItem` registers under the same item as their
    /// standing variant, so `wall_torch.asItem()` is `minecraft:torch`.
    ///
    /// **Built rather than searched, and injective rather than assumed
    /// injective.** Measured on the extract: of the 1,054 item entries, the
    /// number of blocks claimed by more than one item is **0**, so the
    /// inversion loses nothing. If that ever stopped being true this map
    /// would silently keep one of them, which is why the count is written
    /// down here and asserted in [`tests`] rather than left as a property of
    /// today's registry.
    by_block: BTreeMap<String, String>,
    /// Every block id vanilla answers `instanceof StairBlock` for.
    ///
    /// `StairBlock.isStairs(state)` is `state.getBlock() instanceof
    /// StairBlock` and **not** "the name ends in `_stairs`". The two agree on
    /// every block in 26.2, and that agreement is a fact about today's
    /// registry rather than about the rule, so the class map answers it —
    /// the same source [`crate::shapes::ShapeTable`] uses for `DoorBlock`,
    /// `BedBlock` and `DoublePlantBlock`, and for the same reason.
    ///
    /// Empty when [`Self::with_classes`] was never called, which makes
    /// [`stairs_shape`] answer `STRAIGHT` everywhere — the pre-port
    /// behaviour, not a wrong corner.
    stairs: std::collections::BTreeSet<String>,
    /// Every block id vanilla answers `instanceof ChestBlock` for.
    ///
    /// Only used to decide whether the chest *rule* applies. Whether two
    /// chests may pair is a different and stricter question —
    /// `chestCanConnectTo` is `state.is(this)`, the **same block** — so a
    /// trapped chest never pairs with a plain one even though both are in
    /// this set. See [`chest_type`].
    chests: std::collections::BTreeSet<String>,
    /// Every block id vanilla answers `instanceof SlabBlock` for, for
    /// [`can_be_replaced`] and the merge in [`state_for_placement`].
    slabs: std::collections::BTreeSet<String>,
    /// The same for `SnowLayerBlock`.
    snow_layers: std::collections::BTreeSet<String>,
    /// The same for `CandleBlock`.
    candles: std::collections::BTreeSet<String>,
}

#[derive(Debug)]
struct Entry {
    /// False when the row is `random` or not `yawOnly`, or carries no cells.
    /// A row this server must not read is dropped here rather than at every
    /// lookup.
    usable: bool,
    states: Vec<String>,
    cells: Vec<i32>,
}

impl PlacementTable {
    /// Cells per block: 6 faces x 4 yaws x 3 pitches x 2 cursor heights.
    const CELLS: usize = 144;

    /// # Errors
    /// If the file is missing, is not the expected shape, or was dumped
    /// against a different grid from the one this code indexes.
    pub fn load(path: &std::path::Path) -> io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let doc: serde_json::Value = serde_json::from_str(&text).map_err(io::Error::other)?;

        // The header is checked, not skipped. Every axis of the grid is an
        // ordering, and an ordering that changes in the dumper without
        // changing here is a silent transposition — the failure would be
        // "some blocks face the wrong way", which looks exactly like the bug
        // this whole table exists to fix. `block_props.json` learned the same
        // lesson about `directionOrder`.
        expect_array(
            &doc,
            "faceOrder",
            &["DOWN", "UP", "NORTH", "SOUTH", "WEST", "EAST"],
        )?;
        expect_array(&doc, "yawDirections", &["SOUTH", "WEST", "NORTH", "EAST"])?;
        let stride = doc.get("cellStride").and_then(serde_json::Value::as_str);
        if stride != Some("((face * 4 + yaw) * 3 + pitch) * 2 + cursor") {
            return Err(io::Error::other(format!(
                "placement.json indexes its cells as {stride:?}, which is not what this reads"
            )));
        }

        let rows = doc
            .get("table")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| io::Error::other("placement.json has no `table` array"))?;
        let mut blocks = BTreeMap::new();
        for row in rows {
            let name = row
                .get("block")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| io::Error::other("a placement row has no `block`"))?;
            let flag = |k: &str| row.get(k).and_then(serde_json::Value::as_bool) == Some(true);
            let states: Vec<String> = row
                .get("states")
                .and_then(serde_json::Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let cells: Vec<i32> = row
                .get("cells")
                .and_then(serde_json::Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(serde_json::Value::as_i64)
                        .filter_map(|v| i32::try_from(v).ok())
                        .collect()
                })
                .unwrap_or_default();
            let sane = cells.len() == Self::CELLS
                && cells.iter().all(|&c| c < 0 || (c as usize) < states.len());
            blocks.insert(
                name.to_string(),
                Entry {
                    usable: sane && flag("yawOnly") && !flag("random"),
                    states,
                    cells,
                },
            );
        }
        if blocks.is_empty() {
            return Err(io::Error::other("no rows in placement.json"));
        }
        let items = doc
            .get("items")
            .and_then(serde_json::Value::as_object)
            .map(|o| {
                o.iter()
                    .filter_map(|(k, v)| v.as_str().map(|b| (k.clone(), b.to_string())))
                    .collect()
            })
            .unwrap_or_default();
        let wall_items = doc
            .get("wallItems")
            .and_then(serde_json::Value::as_object)
            .map(|o| {
                o.iter()
                    .filter_map(|(k, v)| v.as_str().map(|b| (k.clone(), b.to_string())))
                    .collect()
            })
            .unwrap_or_default();
        let by_block = invert(&items, &wall_items);
        Ok(Self {
            blocks,
            items,
            wall_items,
            by_block,
            stairs: std::collections::BTreeSet::new(),
            chests: std::collections::BTreeSet::new(),
            slabs: std::collections::BTreeSet::new(),
            snow_layers: std::collections::BTreeSet::new(),
            candles: std::collections::BTreeSet::new(),
        })
    }

    /// Resolve the class-keyed sets this table's ported rules need, from the
    /// same `set_placed_by.json` class map [`crate::shapes`] uses.
    ///
    /// Separate from [`Self::load`] because the class map is a different
    /// file with its own failure mode: a table loaded without it still
    /// answers every table lookup correctly and only the ported rules go
    /// quiet, so the two are kept distinguishable rather than folded into one
    /// `Result`.
    #[must_use]
    pub fn with_classes(mut self, blocks: &crate::placedby::PlacedByTable) -> Self {
        let ids = blocks.class_pairs();
        let of = |class: &str| -> std::collections::BTreeSet<String> {
            ids.keys()
                .filter(|b| blocks.is_a(b, class))
                .cloned()
                .collect()
        };
        self.stairs = of(STAIR_BLOCK);
        self.chests = of(CHEST_BLOCK);
        self.slabs = of(SLAB_BLOCK);
        self.snow_layers = of(SNOW_LAYER_BLOCK);
        self.candles = of(CANDLE_BLOCK);
        self
    }

    /// The item -> block map, from an already-resolved one — what the tests
    /// build, beside [`Self::with_stairs`] and for the same reason.
    ///
    /// Rebuilds `Block.asItem()` from it, because the inverse is derived and
    /// a builder that set one without the other would hand out a table whose
    /// two directions disagreed.
    #[must_use]
    pub fn with_items(mut self, items: BTreeMap<String, String>) -> Self {
        self.by_block = invert(&items, &self.wall_items);
        self.items = items;
        self
    }

    /// `instanceof SlabBlock`, from an already-resolved set.
    #[must_use]
    pub fn with_slabs(mut self, slabs: std::collections::BTreeSet<String>) -> Self {
        self.slabs = slabs;
        self
    }

    /// `instanceof SnowLayerBlock`, from an already-resolved set.
    #[must_use]
    pub fn with_snow_layers(mut self, snow: std::collections::BTreeSet<String>) -> Self {
        self.snow_layers = snow;
        self
    }

    /// The `CandleBlock` set, for tests, beside [`Self::with_snow_layers`].
    #[must_use]
    pub fn with_candles(mut self, candles: std::collections::BTreeSet<String>) -> Self {
        self.candles = candles;
        self
    }

    /// The same, from an already-resolved set — what the tests build.
    #[must_use]
    pub fn with_stairs(mut self, stairs: std::collections::BTreeSet<String>) -> Self {
        self.stairs = stairs;
        self
    }

    /// As [`Self::with_stairs`], for `ChestBlock`.
    #[must_use]
    pub fn with_chests(mut self, chests: std::collections::BTreeSet<String>) -> Self {
        self.chests = chests;
        self
    }

    /// Whether the block a state names is a `ChestBlock`.
    #[must_use]
    pub fn is_chest(&self, state: &str) -> bool {
        self.chests
            .contains(state.split_once('[').map_or(state, |(b, _)| b))
    }

    /// `StairBlock.isStairs(BlockState)`.
    #[must_use]
    pub fn is_stairs(&self, state: &str) -> bool {
        self.stairs.contains(name_of(state))
    }

    /// `state.getBlock() instanceof SlabBlock`.
    ///
    /// Empty without [`Self::with_classes`], which makes every slab take the
    /// default `canBeReplaced` and stack rather than merge — the behaviour
    /// this server had before the override existed, reached by the table
    /// being absent rather than by the rule being wrong.
    #[must_use]
    pub fn is_slab(&self, state: &str) -> bool {
        self.slabs.contains(name_of(state))
    }

    /// `state.getBlock() instanceof SnowLayerBlock`.
    #[must_use]
    pub fn is_snow_layer(&self, state: &str) -> bool {
        self.snow_layers.contains(name_of(state))
    }

    /// `state.getBlock() instanceof CandleBlock`.
    #[must_use]
    pub fn is_candle(&self, state: &str) -> bool {
        self.candles.contains(name_of(state))
    }

    /// The block an item places, or `None` if it places nothing.
    ///
    /// `None` is a real answer — a sword, a bucket, a stick — and the caller
    /// must place nothing rather than fall back to the item's own name, which
    /// is what makes this better than the guess it replaces.
    #[must_use]
    pub fn block_for_item(&self, item: &str) -> Option<&str> {
        self.items.get(item).map(String::as_str)
    }

    /// The block this item places when it is put on the side of something.
    ///
    /// `None` for the 993 items that place the same block whichever face was
    /// clicked.
    #[must_use]
    pub fn wall_block_for_item(&self, item: &str) -> Option<&str> {
        self.wall_items.get(item).map(String::as_str)
    }

    /// `Block.asItem()` — the item that puts this block down, if any.
    ///
    /// `None` for a block no `BlockItem` was registered for, which is the
    /// honest answer rather than a guess: `ItemStack.is(asItem())` against a
    /// block with no item is `is(AIR)` and false for every real stack, and a
    /// `None` here reaches the same answer through the same reasoning.
    #[must_use]
    pub fn item_for_block(&self, block: &str) -> Option<&str> {
        self.by_block.get(block).map(String::as_str)
    }

    /// How many items place a block.
    #[must_use]
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// How many blocks the file describes — the loader's own proof that it
    /// read something.
    #[must_use]
    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    /// What vanilla places for this click, if the table can say.
    ///
    /// `default` is the block's default state; only the part before the `[`
    /// is used, because the file is keyed by block and a default state is the
    /// only spelling of a block name this server has to hand.
    #[must_use]
    pub fn answer(&self, default: &str, click: Click) -> Answer<'_> {
        let block = default.split_once('[').map_or(default, |(b, _)| b);
        let Some(entry) = self.blocks.get(block) else {
            return Answer::Unknown;
        };
        if !entry.usable {
            return Answer::Unknown;
        }
        let face = click.face.to_3d() as usize;
        let yaw = from_y_rot(click.yaw);
        let cursor = usize::from(click.cursor_y > 0.5);
        let at = ((face * 4 + yaw) * 3 + PITCH_LEVEL) * 2 + cursor;
        match entry.cells.get(at) {
            None => Answer::Unknown,
            Some(&c) if c < 0 => Answer::Refused,
            Some(&c) => entry
                .states
                .get(c as usize)
                .map_or(Answer::Unknown, |s| Answer::State(s)),
        }
    }
}

fn expect_array(doc: &serde_json::Value, key: &str, want: &[&str]) -> io::Result<()> {
    let got: Vec<&str> = doc
        .get(key)
        .and_then(serde_json::Value::as_array)
        .map(|a| a.iter().filter_map(serde_json::Value::as_str).collect())
        .unwrap_or_default();
    if got == want {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "placement.json `{key}` is {got:?}, this code indexes {want:?}"
        )))
    }
}

/// `Direction.fromYRot(double)`, as an index into `from2DDataValue` —
/// SOUTH, WEST, NORTH, EAST.
///
/// The body is vanilla's: `Mth.floor(yaw / 90.0 + 0.5) & 3`. `Mth.floor(d)`
/// is `(int) Math.floor(d)`, so a negative yaw floors *down* and the mask
/// brings it back into range the same way Java's `&` on a two's-complement
/// negative does — which is why this is a mask and not a remainder. A yaw of
/// −45 is `floor(0.0) & 3` = 0 = SOUTH; a remainder would say −0 and index
/// out of range.
/// `Direction.BY_2D_DATA`, in lower case, as block-state property values.
///
/// The same four names in the same order that [`from_y_rot`] indexes and that
/// `placement.json`'s `yawDirections` is refused for disagreeing with, so the
/// two cannot drift apart: the loader checks the table against
/// `["SOUTH", "WEST", "NORTH", "EAST"]` and this is that list spelled the way
/// a state string spells it.
pub const HORIZONTAL_BY_2D_DATA: [&str; 4] = ["south", "west", "north", "east"];

#[must_use]
pub fn from_y_rot(yaw: f32) -> usize {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "Mth.floor is a cast to int, and this quotient is small"
    )]
    let index = (f64::from(yaw) / 90.0 + 0.5).floor() as i32 & 3;
    index.unsigned_abs() as usize
}

/// `Direction`'s six names in ordinal order — DOWN, UP, NORTH, SOUTH,
/// WEST, EAST — the indices `rustmc_behavior::place_survive` uses.
const DIRECTION_NAMES: [&str; 6] = ["down", "up", "north", "south", "west", "east"];

/// The ten look-facing blocks `placement.json` flags `pitchSensitive` and
/// this module otherwise answers with their default state — every one of
/// them facing north whatever the player looked at.
///
/// Each `getStateForPlacement`, from `javap -p -c -constants` on the pinned
/// jar, reads `BlockPlaceContext.getNearestLookingDirection()`, which is
/// `Direction.orderedByNearest(player)[0]` (offsets 0-9):
///
/// ```text
///   PistonBaseBlock (piston, sticky_piston)   FACING = looking.getOpposite()
///   DispenserBlock  (dispenser, dropper)      FACING = looking.getOpposite()
///   BarrelBlock                               FACING = looking.getOpposite()
///   CommandBlock    (three command blocks)    FACING = looking.getOpposite()
///   ObserverBlock                             FACING = looking.getOpposite().getOpposite()
///   CrafterBlock    ORIENTATION = FrontAndTop(front = looking.getOpposite(),
///                     top = front DOWN -> horizontal.getOpposite(),
///                           front UP   -> horizontal,
///                           otherwise  -> UP)          (tableswitch 16-86)
/// ```
///
/// So a piston points back at the player and an observer's face looks away
/// from them. The crafter's `TRIGGERED` is `hasNeighborSignal(clickedPos)`,
/// a signal question this table cannot answer; the placement handler sets it
/// (see [`is_crafter`]).
///
/// `x_rot`/`y_rot` are the player's pitch and yaw as the server holds them.
/// `None` for any other block, and for a state `known` does not recognise.
/// Whether `state` is a crafter's — the one look-facing block whose
/// placement state also reads the redstone signal at its position.
#[must_use]
pub fn is_crafter(state: &str) -> bool {
    state.split('[').next() == Some("minecraft:crafter")
}

#[must_use]
pub fn look_facing(
    default: &str,
    x_rot: f32,
    y_rot: f32,
    known: &dyn Fn(&str) -> bool,
) -> Option<String> {
    use rustmc_behavior::place_survive::{opposite, ordered_by_nearest, DOWN, UP};
    let block = default.split('[').next().unwrap_or(default);
    let looking = ordered_by_nearest(x_rot, y_rot)[0];
    let facing = match block {
        "minecraft:piston"
        | "minecraft:sticky_piston"
        | "minecraft:dispenser"
        | "minecraft:dropper"
        | "minecraft:barrel"
        | "minecraft:command_block"
        | "minecraft:chain_command_block"
        | "minecraft:repeating_command_block" => opposite(looking),
        "minecraft:observer" => looking,
        "minecraft:crafter" => {
            let front = opposite(looking);
            let horizontal = from_y_rot(y_rot);
            // `BY_2D_DATA` index -> the six-direction index.
            let horizontal = match HORIZONTAL_BY_2D_DATA[horizontal] {
                "south" => 3,
                "west" => 4,
                "north" => 2,
                _ => 5,
            };
            let top = match front {
                DOWN => opposite(horizontal),
                UP => horizontal,
                _ => UP,
            };
            let orientation = format!("{}_{}", DIRECTION_NAMES[front], DIRECTION_NAMES[top]);
            let state = with_property(default, "orientation", &orientation);
            return known(&state).then_some(state);
        }
        _ => return None,
    };
    let state = with_property(default, "facing", DIRECTION_NAMES[facing]);
    known(&state).then_some(state)
}

/// The state to place, given the default state and the click.
///
/// `None` is vanilla refusing the placement, which is a real answer and not a
/// failure — see [`Answer::Refused`].
///
/// `into` is what is being replaced, which is where the fluid test looks —
/// vanilla asks `getLevel().getFluidState(context.getClickedPos())`, and
/// `getClickedPos` is the position the block will occupy.
///
/// `known` decides whether a computed state exists; see the module docs.
#[must_use]
pub fn state_for_placement(
    table: Option<&PlacementTable>,
    default: &str,
    click: Click,
    into: &str,
    known: &dyn Fn(&str) -> bool,
    around: Option<&Around>,
) -> Option<String> {
    // **The corpus positive control, and it is first because every rule below
    // is what it removes.** `Some(default)` is this function's own answer for
    // a block with no placement rule — the tail of the `around` arm returns
    // exactly that — so the control is not an injected value but the server
    // this one grew out of. A check whose claim is a computed placement
    // property must go red here; one that stays green was never reading the
    // property. See `crate::sabotage`.
    if crate::sabotage::fires_here(crate::sabotage::Kind::Placement) {
        return Some(default.to_string());
    }
    // **A merge into the block that was clicked comes before everything.**
    //
    // `SlabBlock.getStateForPlacement` and `SnowLayerBlock`'s both open by
    // reading the state at `getClickedPos()` and returning early if it is
    // their own block; the sampled table cannot hold either answer, because
    // the grid was clicked into open air and neither rule fires there. This
    // is the other half of [`can_be_replaced`]'s two overrides: that decides
    // the placement lands *on* the clicked block, and this decides what it
    // writes when it does.
    if let Some(merged) = merged_into(table, default, into) {
        return Some(merged);
    }
    // **The ported rules run first, and only for blocks that have one.**
    //
    // The table is vanilla's own return value and is preferred wherever it can
    // answer, but it was sampled by clicking into open air, so for the 195
    // blocks `placement.json` marks `neighbourSensitive` its answer is the
    // isolated one — and for a ladder that is a refusal, because nothing was
    // there to hang it on. A refusal is indistinguishable from the legitimate
    // kind (a door with no room above), so the rule cannot be "override
    // refusals"; it has to be per block and opt-in, which is also what
    // `AGENT-BRIEF` demands of new behaviour until it is measured.
    // `WallBlock.getStateForPlacement` reads all four neighbours and the
    // block above, so the whole state is the level's — see `crate::wall`.
    // Waterlogging is its own first `setValue` (offsets 191-212), from the
    // fluid being placed into, before the neighbours are asked.
    if let Some(a) = around {
        let wall = crate::wall::live(a.state_id, |f| {
            crate::wall::is_wall(f, default).then(|| {
                let d = if into == WATER_SOURCE {
                    with_property(default, "waterlogged", "true")
                } else {
                    default.to_string()
                };
                crate::wall::placement(f, &d, a)
            })
        })
        .flatten();
        if let Some(s) = wall {
            return if known(&s) {
                Some(s)
            } else {
                Some(default.to_string())
            };
        }
    }
    if let Some(a) = around {
        if let Some(s) = computed_with_neighbours(default, click, a) {
            return if s == default || known(&s) {
                Some(s)
            } else {
                Some(default.to_string())
            };
        }
    }
    // The table first, because when it answers it answers completely: it is
    // vanilla's own return value for this click, not a rule reconstructed
    // from one. The `axis` rule below only runs when the table has nothing,
    // which is the 535 blocks whose answer is their default plus the handful
    // the dumper flagged.
    let mut state = match table.map_or(Answer::Unknown, |t| t.answer(default, click)) {
        Answer::Refused => return None,
        // `rotation` is refined **only here**, and that is structural rather
        // than an optimisation: [`refine_rotation`] recovers the block's
        // offset by subtracting the yaw column out of the value it is given,
        // which is only meaningful if that value came from the table. Given a
        // default state instead it would read the default's `rotation` as if
        // it were a sampled answer and invent an offset. All 47 blocks
        // carrying the property are in the file and all 47 are `yawOnly`, so
        // this branch is the only one they reach today; the point is that a
        // future one cannot silently reach the other.
        Answer::State(s) => refine_rotation(s, click.yaw),
        Answer::Unknown => with_property(default, "axis", axis_of(click.face)),
    };
    // Applied on top either way: the grid was clicked in a world with no
    // water in it, so `waterlogged` is the one property the table structurally
    // cannot carry.
    if into == WATER_SOURCE {
        state = with_property(&state, "waterlogged", "true");
    }
    // **The refinements run last, and that is vanilla's own order.**
    //
    // A refinement is not a second kind of port bolted on beside
    // `computed_with_neighbours`; it is the shape three of vanilla's own
    // `getStateForPlacement`s already have. `StairBlock` builds a state from
    // `getHorizontalDirection()`, the clicked face and the fluid — every one
    // of which the table sampled — and only then writes one more property
    // from the level:
    //
    // ```java
    //   BlockState s = defaultBlockState().setValue(FACING, ...)
    //                                     .setValue(HALF, ...)
    //                                     .setValue(WATERLOGGED, ...);
    //   return s.setValue(SHAPE, getStairsShape(s, level, pos));   // 111-132
    // ```
    //
    // So the honest port keeps the table's measured answer for the first
    // three and computes the fourth, rather than re-deriving `FACING` and
    // `HALF` by hand and hoping they agree with what vanilla returned. That
    // matters here in a way it did not for the ladder: a stair's `FACING` is
    // `getHorizontalDirection()` and a furnace's is its opposite, which is
    // the exact confusion this module's docs open with.
    if let Some(a) = around {
        state = refined_with_neighbours(table, state, a);
    }
    if state == default || known(&state) {
        Some(state)
    } else {
        Some(default.to_string())
    }
}

/// The early return both merging placements share.
///
/// ```text
///   SlabBlock.getStateForPlacement, 0-48
///     state = level.getBlockState(ctx.getClickedPos());
///     if (state.is(this))
///       return state.setValue(TYPE, DOUBLE).setValue(WATERLOGGED, false);
///
///   SnowLayerBlock.getStateForPlacement, 0-55
///     state = level.getBlockState(ctx.getClickedPos());
///     if (state.is(this)) {
///       layers = state.getValue(LAYERS);
///       return state.setValue(LAYERS, Math.min(8, layers + 1));
///     }
/// ```
///
/// Both build on **`into`**, the state already there, not on the block's
/// default — so a top slab merging keeps `type` only long enough to be
/// overwritten, and a waterlogged slab is explicitly dried out while a
/// snow layer has no such property to carry. Reading it off the default
/// instead would give a double slab that had forgotten it was waterlogged
/// by luck rather than by rule.
///
/// `is(this)` is a *block* identity test, which is why this compares
/// [`name_of`] rather than the whole state: a bottom slab and a top slab of
/// the same wood are one block.
///
/// Returns `None` for every other block, including a slab clicked onto a
/// different slab — `oak_slab` onto `spruce_slab` is two blocks and stacks.
fn merged_into(table: Option<&PlacementTable>, default: &str, into: &str) -> Option<String> {
    let table = table?;
    if name_of(default) != name_of(into) {
        return None;
    }
    if table.is_slab(default) {
        // `type=double` is reachable from the extract's own state list, so a
        // `known()` check upstream cannot reject it.
        let doubled = with_property(into, "type", "double");
        return Some(with_property(&doubled, "waterlogged", "false"));
    }
    if table.is_snow_layer(default) {
        let layers = property_of(into, "layers")?.parse::<u8>().ok()?;
        return Some(with_property(
            into,
            "layers",
            &layers.saturating_add(1).min(8).to_string(),
        ));
    }
    // `CandleBlock.getStateForPlacement`, offsets 0-30: `state.is(this)`
    // returns `state.cycle(CANDLES)` — the state already there, one more
    // candle, keeping `lit` and `waterlogged` as they were. `cycle` wraps
    // 4 -> 1, which `canBeReplaced`'s `< 4` keeps out of reach.
    if table.is_candle(default) {
        let n = property_of(into, "candles")?.parse::<u8>().ok()?;
        let next = if n >= 4 { 1 } else { n + 1 };
        return Some(with_property(into, "candles", &next.to_string()));
    }
    None
}

/// Read access to the world around the position a block is being placed into.
///
/// # Why this is arbitrary-position and not a six-neighbour window
///
/// A six-neighbour window was specified for this on 2026-08-05 and **it was
/// wrong**. The specification came from `update_shape.json`'s `refused` list,
/// which records, per `(state, direction)` pair, the offset of the *first*
/// read outside the two positions `ShapeGolden`'s probe serves — it throws
/// there (`ShapeGolden.java:403`) and never sees what the method would have
/// read next. Those offsets are a **lower bound on the access shape, never the
/// shape**.
///
/// `RedStoneWireBlock.getConnectingSide(BlockGetter, BlockPos, Direction,
/// boolean)` disproves it outright:
///
/// ```text
///     0-5    local5 = pos.relative(dir)
///     7-15   getBlockState(local5)            orthogonal
///    57-63   getBlockState(local5.above())    DIAGONAL
///   120-123  getBlockState(local5.below())    DIAGONAL
/// ```
///
/// which is wire climbing the block beside it — the subject of
/// `450-redstone-wire-climbs-the-block-beside-it`. So the reader is
/// unrestricted and each ported rule documents its own access shape from
/// bytecode.
pub struct Around<'a> {
    /// `getClickedPos()` — the position the block will occupy, not the one
    /// that was clicked.
    pub pos: BlockPos,
    /// `BlockGetter.getBlockState`, at any position.
    pub at: &'a dyn Fn(BlockPos) -> String,
    /// `getBlockState(pos).isFaceSturdy(level, pos, face)` — see
    /// [`crate::edits::Blocks::face_sturdy`], which is where the
    /// `block_props.json` lookup lives. Taken as a closure rather than the
    /// props table so `placement` does not have to be handed a second view of
    /// the world that could disagree with the first.
    pub sturdy: &'a dyn Fn(BlockPos, Face) -> bool,
    /// `getBlockState(pos).isRedstoneConductor(level, pos)` — see
    /// [`crate::edits::Blocks::redstone_conductor`]. Carried here rather than
    /// derived from `at` because no state string answers it: it is a
    /// per-block predicate and `block_props.json` gained a column for it.
    pub conductor: &'a dyn Fn(BlockPos) -> bool,
    /// `getBlockState(pos).isSignalSource()`, the one column
    /// `RedStoneWireBlock.shouldConnectTo` needs.
    pub source: &'a dyn Fn(BlockPos) -> bool,
    /// `NoteBlock.setInstrument` — see
    /// [`crate::edits::Blocks::note_block_instrument`], which is where the
    /// `block_props.json` lookup and the two-position read live.
    ///
    /// A closure for the reason `conductor` is one: no state string answers
    /// it, and handing `placement` the props table would give it a second
    /// view of the world that could disagree with `at`.
    ///
    /// `None` means the table could not answer, and the caller leaves the
    /// state alone.
    pub instrument: &'a dyn Fn(BlockPos) -> Option<String>,
    /// `Block.BLOCK_STATE_REGISTRY` id of a state — see
    /// [`crate::edits::Blocks::state_id`] — which is what indexes
    /// `collision_shapes.json`. A rule that asks for a neighbour's
    /// **collision shape** (a wall reading the block above it) reads it
    /// through this; `None` means the id is unknown and the rule has to say
    /// what that means for it.
    pub state_id: &'a dyn Fn(&str) -> Option<i32>,
}

impl Around<'_> {
    /// The state one step from the placement position along `f`.
    #[must_use]
    pub fn neighbour(&self, f: Face) -> String {
        (self.at)(f.relative(self.pos))
    }

    /// `state.isFaceSturdy(level, pos, face)` for the block one step along
    /// `from`, asked about its face pointing along `face`.
    #[must_use]
    pub fn neighbour_face_sturdy(&self, from: Face, face: Face) -> bool {
        (self.sturdy)(from.relative(self.pos), face)
    }
}

/// The ported `getStateForPlacement` rules that need to read the world.
///
/// `None` means "no ported rule for this block", which is not the same as a
/// refusal and must not be confused with one — the caller falls through to the
/// table. A rule that wants to refuse returns the refusal itself, and today no
/// rule does: see [`ladder_placement`].
fn computed_with_neighbours(default: &str, click: Click, around: &Around) -> Option<String> {
    let block = default.split_once('[').map_or(default, |(b, _)| b);
    match block {
        "minecraft:ladder" => ladder_placement(default, click, around),
        // `RedStoneWireBlock.getStateForPlacement` is one line —
        // `getConnectionState(level, defaultBlockState(), clickedPos)` — and
        // the level decides all four sides, so this replaces the table's
        // answer rather than refining it. The table's own answer is the
        // isolated one: every cell was clicked into open air, where a dust is
        // a dot.
        //
        // **`updateShape` and `updateIndirectNeighbourShapes` are NOT wired
        // in**, so a wire still does not react to a neighbour appearing after
        // it. That is deliberate and is why `450` should not move on this
        // commit: the check places its block diagonally, which only the
        // indirect cascade reaches. See `crate::wire`.
        "minecraft:redstone_wire" => Some(crate::wire::placement(around, around.pos, default)),
        "minecraft:end_rod" => Some(end_rod_placement(default, click, around)),
        "minecraft:bamboo" => bamboo_placement(around),
        _ => None,
    }
}

/// `EndRodBlock.getStateForPlacement`, offsets 0-73.
///
/// ```text
///    0  Direction d = ctx.getClickedFace();
///   20  BlockState s = level.getBlockState(ctx.getClickedPos().relative(d.getOpposite()));
///   24  if (s.is(this) && s.getValue(FACING) == d)
///   43      return defaultBlockState().setValue(FACING, d.getOpposite());
///   61  return defaultBlockState().setValue(FACING, d);
/// ```
///
/// So a rod placed against the tip of a rod pointing the same way points
/// back at it, and the two meet end to end. The table cannot say this: it
/// was sampled by clicking into open air, where the second arm is the only
/// one reachable. Measured on the check
/// `an-end-rod-placed-on-an-end-rod-points-the-other-way`: vanilla writes
/// `end_rod[facing=down]` on top of an `end_rod[facing=up]`, this server wrote
/// `facing=up`.
///
/// **Only `EndRodBlock`.** `LightningRodBlock` extends the same `RodBlock`
/// and does not override this way — its `getStateForPlacement` reads fluid
/// and the clicked face only — so the rule is keyed on the block and not on
/// the superclass.
fn end_rod_placement(default: &str, click: Click, around: &Around) -> String {
    let behind = around.neighbour(click.face.opposite());
    let facing = if name_of(&behind) == "minecraft:end_rod"
        && property_of(&behind, "facing") == Some(face_name(click.face))
    {
        click.face.opposite()
    } else {
        click.face
    };
    with_property(default, "facing", face_name(facing))
}

/// The ported `getStateForPlacement` tails that **refine** the table's answer
/// instead of replacing it.
///
/// Distinct from [`computed_with_neighbours`], and the distinction is
/// vanilla's rather than ours — see the comment at the call site. A rule
/// belongs here when vanilla builds the state from click-only inputs the
/// grid already sampled and *then* writes one more property from the level;
/// it belongs in `computed_with_neighbours` when the level decides the whole
/// state, as it does for a ladder's `FACING`.
///
/// Returns the state unchanged when there is no rule, which is why this
/// returns `String` rather than `Option<String>`: a refinement that declines
/// leaves the measured answer standing, and there is no third outcome. A
/// *refusal* cannot be expressed here and must not be added to it — vanilla's
/// stair and chest placements have no `return null`.
fn refined_with_neighbours(
    table: Option<&PlacementTable>,
    state: String,
    around: &Around,
) -> String {
    let Some(table) = table else { return state };
    if table.is_stairs(&state) {
        if let Some(shape) = stairs_shape(&|s| table.is_stairs(s), &state, around) {
            return with_property(&state, "shape", shape);
        }
    }
    if table.is_chest(&state) {
        if let Some(kind) = chest_type(&state, around) {
            return with_property(&state, "type", kind);
        }
    }
    // `NoteBlock.getStateForPlacement` is `setInstrument(level,
    // getClickedPos(), defaultBlockState())` and nothing else — one property
    // written from the level on top of the default, which is exactly the
    // shape this function exists for.
    //
    // **It has to be here and cannot be in the table.** `placement.json` was
    // sampled by clicking into open air over whatever the harness had
    // underneath, so its cell for a note block records that harness's ground
    // and not the player's. Measured on the 2026-08-17 recordings of
    // scenario 430: this server wrote `instrument=basedrum` on grass where
    // vanilla writes `instrument=harp`.
    //
    // `data/vanilla/update_shape.json` cannot hold the other half either,
    // and says so in its own refusals — `updateShape read 0,1,0, not its own
    // neighbour`, because `setInstrument` reads above and below whichever
    // direction actually changed.
    if name_of(&state) == NOTE_BLOCK {
        if let Some(instrument) = (around.instrument)(around.pos) {
            return with_property(&state, "instrument", &instrument);
        }
    }
    state
}

/// The one block whose placement state is a two-neighbour read.
const NOTE_BLOCK: &str = "minecraft:note_block";

/// `ChestBlock.getChestType(Level, BlockPos, Direction)` and the
/// `candidatePartnerFacing` it is built from, offsets 0-39 and 0-49.
///
/// ```java
/// // getChestType(level, pos, facing)
/// if (facing == candidatePartnerFacing(level, pos, facing.getClockWise()))
///     return LEFT;                                            //  0-17
/// if (facing == candidatePartnerFacing(level, pos, facing.getCounterClockWise()))
///     return RIGHT;                                           // 18-35
/// return SINGLE;                                              //    36
///
/// // candidatePartnerFacing(level, pos, dir)
/// BlockState nb = level.getBlockState(pos.relative(dir));     //  0-9
/// return chestCanConnectTo(nb) && nb.getValue(TYPE) == SINGLE //  11-31
///      ? nb.getValue(FACING) : null;                          //  34-45 / 48
/// ```
///
/// **Access shape: two positions, both orthogonal and both horizontal** —
/// one step clockwise of the placement's facing and one step
/// counter-clockwise.
///
/// # Three ways to get this wrong, all of them from the bytecode
///
///   * **The neighbour must already be `SINGLE`.** Offsets 22-31. Placing a
///     chest beside an existing *pair* leaves it single; without that test a
///     third chest would silently rewrite a finished double.
///   * **It is the neighbour's `FACING` that is compared, not its
///     position.** `candidatePartnerFacing` returns a `Direction`, and
///     `getChestType` asks whether it equals the placement's own facing.
///     Two chests side by side facing different ways do not pair.
///   * **`chestCanConnectTo` is `state.is(this)`** — the same block, not
///     `instanceof`. A trapped chest and a plain chest are both
///     `ChestBlock`s and never pair with each other, which is why this
///     compares block ids rather than asking the class set.
///
/// # `isSecondaryUseActive` is not carried, and that is a known gap
///
/// Vanilla has a branch in front of this one, offsets 37-114, which fires
/// only when the click was on a horizontal face **while sneaking** and takes
/// `FACING` from the partner rather than from the player. `!secondary` is
/// assumed true here, which is exactly the assumption
/// `data/vanilla/placement.json` was sampled under, so the refinement and the
/// table it refines agree about which world they are in.
///
/// **The reason changed on 2026-08-10 and the gap did not.** It used to be
/// that this server did not track the sneak flag at all;
/// `crate::player::PlayerState::shift_key_down` now does, and
/// `crate::world_send::handle_use_item_on` reads it. So the branch is now
/// reachable in principle and is still not written: `Around` carries the
/// world, not the player, and the sampled table has no column for a sneaking
/// click. Closing it means threading the flag into
/// [`state_for_placement`] and re-sampling, which is a table change.
/// Nothing measures it today — no corpus scenario sneaks against a chest's
/// side.
fn chest_type(state: &str, around: &Around) -> Option<&'static str> {
    let facing = Face::from_name(property_of(state, "facing")?)?;
    let block = state.split_once('[').map_or(state, |(b, _)| b);
    // `candidatePartnerFacing`: the facing of a same-block, still-single
    // chest one step along `d`, or `None` for anything else.
    let partner = |d: Face| -> Option<Face> {
        let nb = around.neighbour(d);
        if nb.split_once('[').map_or(&nb[..], |(b, _)| b) != block
            || property_of(&nb, "type") != Some("single")
        {
            return None;
        }
        Face::from_name(property_of(&nb, "facing")?)
    };
    if partner(facing.clockwise()?) == Some(facing) {
        return Some("left");
    }
    if partner(facing.counter_clockwise()?) == Some(facing) {
        return Some("right");
    }
    Some("single")
}

/// `ChestBlock.getConnectedDirection(BlockState)`, offsets 0-35.
///
/// ```java
/// Direction facing = state.getValue(FACING);
/// return state.getValue(TYPE) == LEFT ? facing.getClockWise()
///                                     : facing.getCounterClockWise();
/// ```
///
/// Note the `else` covers **both** `RIGHT` and `SINGLE`, so a single chest
/// has a "connected direction" that points at nothing. That is not a
/// degenerate case to guard against — `updateShape`'s second branch relies on
/// it to decide when a half has lost its partner.
pub(crate) fn chest_connected_direction(state: &str) -> Option<Face> {
    let facing = Face::from_name(property_of(state, "facing")?)?;
    if property_of(state, "type") == Some("left") {
        facing.clockwise()
    } else {
        facing.counter_clockwise()
    }
}

/// `StairBlock.getStairsShape(BlockState, BlockGetter, BlockPos)`, offsets
/// 0-222.
///
/// ```java
/// Direction  facing = state.getValue(FACING);                    //   0-10
/// BlockState front  = level.getBlockState(pos.relative(facing));  //  11-22
/// if (isStairs(front)                                             //  24-29
///     && state.getValue(HALF) == front.getValue(HALF)) {          //  32-47
///     Direction ff = front.getValue(FACING);                      //  50-61
///     if (ff.getAxis() != facing.getAxis()                        //  63-81
///         && canTakeShape(state, level, pos, ff.getOpposite())) { //  84-95
///         return ff == facing.getCounterClockWise()               //  98-104
///              ? OUTER_LEFT : OUTER_RIGHT;                        // 107/111
///     }
/// }
/// BlockState back = level.getBlockState(                          // 115-129
///     pos.relative(facing.getOpposite()));
/// if (isStairs(back)                                              // 131-136
///     && state.getValue(HALF) == back.getValue(HALF)) {           // 139-154
///     Direction bf = back.getValue(FACING);                       // 157-168
///     if (bf.getAxis() != facing.getAxis()                        // 170-188
///         && canTakeShape(state, level, pos, bf)) {               // 191-198
///         return bf == facing.getCounterClockWise()               // 202-208
///              ? INNER_LEFT : INNER_RIGHT;                        // 211/215
///     }
/// }
/// return STRAIGHT;                                                //     219
/// ```
///
/// **Access shape: up to four positions, all orthogonal and all horizontal**
/// — `pos.relative(facing)`, `pos.relative(facing.getOpposite())` and one
/// more per `canTakeShape` call. No diagonals, unlike the wire.
///
/// # The two `canTakeShape` arguments are NOT mirror images, and that is real
///
/// The outer branch passes `ff.getOpposite()` (offset 89) and the inner
/// branch passes `bf` with no `getOpposite` at all (offset 194). Written out
/// because it reads like a transcription slip and is not: both calls name
/// **the block on the far side of the corner from the neighbour that caused
/// it**, and the neighbour sits in front in one case and behind in the other,
/// so the same physical position is reached by opposite expressions. A port
/// that "tidies" this by opposing both makes a stair refuse to corner exactly
/// when a third stair is present, which no single-corner test can see.
///
/// # Why this reads the level twice rather than trusting the neighbour state
///
/// `updateShape` hands the block that changed as `aload 7` and this method is
/// given the level instead — `getStairsShape(state, level, pos)` at offsets
/// 49-53. That is what puts stairs outside the extracted table for good: the
/// answer depends on two neighbours and the caller only ever names one.
pub(crate) fn stairs_shape(
    is_stairs: &dyn Fn(&str) -> bool,
    state: &str,
    around: &Around,
) -> Option<&'static str> {
    let facing = Face::from_name(property_of(state, "facing")?)?;
    let half = property_of(state, "half")?;
    // `isStairs(nb) && state.HALF == nb.HALF`, answering with the neighbour's
    // own `FACING` so the two branches below read the same way vanilla's do.
    let same_half_stair = |f: Face| -> Option<Face> {
        let nb = around.neighbour(f);
        if !is_stairs(&nb) || property_of(&nb, "half") != Some(half) {
            return None;
        }
        Face::from_name(property_of(&nb, "facing")?)
    };
    let ccw = facing.counter_clockwise();
    if let Some(ff) = same_half_stair(facing) {
        if axis_of(ff) != axis_of(facing) && can_take_shape(is_stairs, state, around, ff.opposite())
        {
            return Some(if Some(ff) == ccw {
                "outer_left"
            } else {
                "outer_right"
            });
        }
    }
    if let Some(bf) = same_half_stair(facing.opposite()) {
        if axis_of(bf) != axis_of(facing) && can_take_shape(is_stairs, state, around, bf) {
            return Some(if Some(bf) == ccw {
                "inner_left"
            } else {
                "inner_right"
            });
        }
    }
    Some("straight")
}

/// `StairBlock.canTakeShape(BlockState, BlockGetter, BlockPos, Direction)`,
/// offsets 0-62.
///
/// ```java
/// BlockState nb = level.getBlockState(pos.relative(dir));   //  0-11
/// return !isStairs(nb)                                      // 13-18  -> true
///     || nb.getValue(FACING) != state.getValue(FACING)      // 21-36  -> true
///     || nb.getValue(HALF)   != state.getValue(HALF);       // 39-54  -> false
/// ```
///
/// "Is the block that way anything other than a stair lying exactly the same
/// way as me" — the guard that stops three parallel stairs from cornering
/// into each other. `true` is the permissive answer and is what an unloaded
/// or empty position gives, which is the right default: air is not a stair.
fn can_take_shape(
    is_stairs: &dyn Fn(&str) -> bool,
    state: &str,
    around: &Around,
    dir: Face,
) -> bool {
    let nb = around.neighbour(dir);
    !(is_stairs(&nb)
        && property_of(&nb, "facing") == property_of(state, "facing")
        && property_of(&nb, "half") == property_of(state, "half"))
}

/// `BambooStalkBlock.getStateForPlacement`, offsets 0-186 — the bamboo
/// ITEM places the stalk's block, and the stalk decides from the level what
/// that is:
///
/// ```text
///   fluid at pos not empty                  -> null
///   below not in SUPPORTS_BAMBOO            -> null
///   below is bamboo_sapling                 -> BAMBOO[age=0]
///   below is bamboo                         -> BAMBOO[age = below.age > 0 ? 1 : 0]
///   above is bamboo                         -> BAMBOO[age = above.age]
///   otherwise                               -> BAMBOO_SAPLING
/// ```
///
/// `placement.json` holds no state for the block: every grid cell was
/// clicked into open air over nothing, which is the `null` arm. The two
/// `null`s are returned as `None`, which falls through to that table and its
/// refusal.
fn bamboo_placement(around: &Around) -> Option<String> {
    let tags = crate::fallable::tags()?;
    let here = (around.at)(around.pos);
    if crate::growth::is_air(&here) {
        // An air position has no fluid; anything else this path is offered
        // is a replaceable block, and the fluid check is the table's.
    } else if here.contains("waterlogged=true") || here.starts_with("minecraft:water") {
        return None;
    }
    let below = around.neighbour(Face::Down);
    let below_block = below.split_once('[').map_or(below.as_str(), |(b, _)| b);
    if !tags.contains("minecraft:supports_bamboo", below_block) {
        return None;
    }
    let stalk = |age: &str| format!("minecraft:bamboo[age={age},leaves=none,stage=0]");
    match below_block {
        "minecraft:bamboo_sapling" => return Some(stalk("0")),
        "minecraft:bamboo" => {
            let older = property_of(&below, "age").is_some_and(|a| a != "0");
            return Some(stalk(if older { "1" } else { "0" }));
        }
        _ => {}
    }
    let above = around.neighbour(Face::Up);
    if above.starts_with("minecraft:bamboo[") {
        return Some(stalk(property_of(&above, "age").unwrap_or("0")));
    }
    Some("minecraft:bamboo_sapling".to_string())
}

/// `LadderBlock.getStateForPlacement`, all 88 lines of it.
///
/// ```java
/// if (!ctx.replacingClickedOnBlock()) {                        //   0, ifne 53
///     BlockState behind = level.getBlockState(                  //   7-25
///         clickedPos.relative(clickedFace.getOpposite()));
///     if (behind.is(this)                                       //  31, ifeq 53
///         && behind.getValue(FACING) == clickedFace)            //  48, if_acmpne 53
///         return null;                                          //  51-52
/// }
/// for (Direction d : ctx.getNearestLookingDirections()) {       //  82-107
///     if (!d.getAxis().isHorizontal()) continue;                // 115, ifeq 169
///     s = s.setValue(FACING, d.getOpposite());                  // 121-133
///     if (s.canSurvive(level, pos))                             // 141, ifeq 169
///         return s.setValue(WATERLOGGED, fluid.is(WATER));      // 147-168
/// }
/// return null;                                                  // 175-176
/// ```
///
/// **Access shape: two positions, both orthogonal.**
/// `clickedPos.relative(clickedFace.getOpposite())` for the stacking guard,
/// and `pos.relative(FACING.getOpposite())` for `canSurvive` — where `FACING`
/// is the *loop's* direction and not the clicked face, so the two are
/// different positions whenever the click is not head-on.
///
/// # Pitch is not needed, and a note here said it was
///
/// `getNearestLookingDirections()` is `Direction.orderedByNearest(player)`,
/// which takes `sin`/`cos` of **both** the pitch and the yaw. That made it
/// look as though `Click` — which carries no pitch — could not drive this.
/// It can: every horizontal direction's ordering key is scaled by
/// `cos(pitch)`, a factor common to all four and positive for every pitch a
/// client can send, so it cannot reorder them relative to each other. Pitch
/// decides only where `UP`/`DOWN` sit in the sequence, and this loop skips
/// them. The one exposed edge is `|pitch| == 90` exactly, where all four keys
/// collapse to zero; nothing in the corpus places a ladder while looking
/// straight down, and if a check ever needs it, `Click` gains a pitch.
///
/// # Refusal is deliberately not ported yet
///
/// Both of vanilla's `return null`s are refusals — the stacking guard and an
/// exhausted loop. This returns `None` for them, which the caller reads as "no
/// ported rule" and falls through to the table, whose answer for a ladder is
/// `Refused` anyway. The two therefore agree today, and the distinction is
/// written down rather than relied on: the moment a rule here wants to refuse
/// a block whose table cell is *not* a refusal, `computed_with_neighbours`
/// needs a three-way return and this comment is the warning that it does.
fn ladder_placement(default: &str, click: Click, around: &Around) -> Option<String> {
    // The stacking guard reads the block behind the placement — opposite the
    // face that was clicked.
    let behind = around.neighbour(click.face.opposite());
    let behind_block = behind.split_once('[').map_or(&behind[..], |(b, _)| b);
    if behind_block == "minecraft:ladder"
        && property_of(&behind, "facing").is_some_and(|f| f == face_name(click.face))
    {
        return None;
    }
    // `replacingClickedOnBlock()` is not modelled: it is the *guard* on the
    // branch above, and skipping the guard can only make this refuse where
    // vanilla would not. Since a refusal here falls through to a table that
    // refuses too, the observable behaviour is identical. Written down because
    // that equivalence is a property of today's table, not of the rule.
    for d in horizontals_nearest_first(click.yaw) {
        // **`FACING` is the loop direction's OPPOSITE**, and getting this
        // round the wrong way is the natural mistake: vanilla is
        // `s.setValue(FACING, d.getOpposite())` at offsets 121-133, and then
        // `canSurvive` reads that `FACING` back and supports it at
        // `pos.relative(FACING.getOpposite())` — which is `pos.relative(d)`,
        // the block the player is looking towards.
        //
        // Physically: a ladder hangs on the wall in front of the player and
        // its `facing` points back out of that wall, towards them. The first
        // version of this loop set `facing = d` and looked for support at
        // `pos.relative(d.opposite())`, which is the same *relationship*
        // between facing and support and the wrong one of the two for any
        // given yaw — a bug a single-wall test cannot see, which is why the
        // test below builds two walls and pins which one wins.
        let facing = d.opposite();
        if around.neighbour_face_sturdy(d, facing) {
            return Some(with_property(default, "facing", face_name(facing)));
        }
    }
    None
}

/// The four horizontal directions, nearest the player's look first.
///
/// Ordering key is the dot product of each direction's normal with the
/// horizontal component of the look vector, `(-sin(yaw), cos(yaw))` — the
/// `cos(pitch)` factor `Direction.orderedByNearest` applies is common to all
/// four and positive, so it is omitted. See [`ladder_placement`].
fn horizontals_nearest_first(yaw: f32) -> [Face; 4] {
    let r = f64::from(yaw).to_radians();
    let (lx, lz) = (-r.sin(), r.cos());
    let mut all = [Face::North, Face::South, Face::West, Face::East];
    let dot = |f: Face| -> f64 {
        let (dx, _, dz) = f.normal();
        f64::from(dx).mul_add(lx, f64::from(dz) * lz)
    };
    all.sort_by(|a, b| dot(*b).total_cmp(&dot(*a)));
    all
}

/// `Direction.getName()` for the four horizontals plus the two verticals.
const fn face_name(f: Face) -> &'static str {
    match f {
        Face::Down => "down",
        Face::Up => "up",
        Face::North => "north",
        Face::South => "south",
        Face::West => "west",
        Face::East => "east",
    }
}

#[cfg(test)]
mod ladder_tests {
    use super::*;

    /// A world with sturdy blocks at the named offsets from the placement
    /// position and air everywhere else.
    fn around_with(walls: &[(i32, i32, i32)]) -> (Vec<(i32, i32, i32)>,) {
        (walls.to_vec(),)
    }

    fn place(yaw: f32, clicked: Face, walls: &[(i32, i32, i32)]) -> Option<String> {
        let (walls,) = around_with(walls);
        let at = |p: BlockPos| -> String {
            if walls.contains(&p) {
                "minecraft:stone".to_string()
            } else {
                "minecraft:air".to_string()
            }
        };
        let sturdy = |p: BlockPos, _f: Face| -> bool { walls.contains(&p) };
        let around = Around {
            pos: (0, 0, 0),
            at: &at,
            sturdy: &sturdy,
            conductor: &|_| false,
            source: &|_| false,
            instrument: &|_| None,
            state_id: &|_| None,
        };
        ladder_placement(
            "minecraft:ladder[facing=north,waterlogged=false]",
            Click {
                face: clicked,
                yaw,
                cursor_y: 0.0,
            },
            &around,
        )
    }

    /// **A ladder's `facing` points out of the wall it hangs on, towards the
    /// player** — and the wall is the block the player is looking at.
    ///
    /// Yaw 180 is south-to-north in Minecraft's convention, so the player
    /// faces **north**, the wall in front of them is at `z = -1`, and the
    /// ladder must come back `facing=south`.
    ///
    /// This is the assertion that fails under the first version of the loop,
    /// which set `facing = d` instead of `d.getOpposite()`: it answered
    /// `facing=north` and looked for its wall at `z = +1`, behind the player.
    #[test]
    fn a_ladder_faces_out_of_the_wall_the_player_is_looking_at() {
        assert_eq!(
            place(180.0, Face::South, &[(0, 0, -1)]).as_deref(),
            Some("minecraft:ladder[facing=south,waterlogged=false]"),
        );
    }

    /// **Two walls, and the nearer-to-the-look one wins.** One wall cannot
    /// distinguish a correct ordering from a reversed one, because the single
    /// candidate is the only one that can survive either way.
    ///
    /// The player faces north with walls both north and south. Vanilla walks
    /// `getNearestLookingDirections`, so `NORTH` is tried first and the
    /// ladder hangs on the north wall, `facing=south`.
    #[test]
    fn the_wall_nearest_the_look_direction_wins() {
        assert_eq!(
            place(180.0, Face::South, &[(0, 0, -1), (0, 0, 1)]).as_deref(),
            Some("minecraft:ladder[facing=south,waterlogged=false]"),
        );
        // Turn round: yaw 0 faces south, so the south wall wins instead.
        assert_eq!(
            place(0.0, Face::North, &[(0, 0, -1), (0, 0, 1)]).as_deref(),
            Some("minecraft:ladder[facing=north,waterlogged=false]"),
        );
    }

    /// Nothing to hang on is `None` — the loop runs out. The caller reads
    /// that as "no ported rule" and falls through to the table, whose ladder
    /// cells are all refusals, so the two agree. See [`ladder_placement`].
    #[test]
    fn a_ladder_with_no_wall_has_no_answer() {
        assert_eq!(place(180.0, Face::South, &[]), None);
    }

    /// The stacking guard: clicking the face of a ladder that already faces
    /// the clicked direction is refused, before the loop is reached.
    /// Offsets 0-52.
    #[test]
    fn a_ladder_does_not_stack_onto_its_own_face() {
        let walls = [(0, 0, -1)];
        let at = |p: BlockPos| -> String {
            if p == (0, 0, -1) {
                // The block behind a SOUTH click is at z-1, and it is a
                // ladder already facing south.
                "minecraft:ladder[facing=south,waterlogged=false]".to_string()
            } else if walls.contains(&p) {
                "minecraft:stone".to_string()
            } else {
                "minecraft:air".to_string()
            }
        };
        let sturdy = |p: BlockPos, _f: Face| -> bool { p == (0, 0, -1) };
        let around = Around {
            pos: (0, 0, 0),
            at: &at,
            sturdy: &sturdy,
            conductor: &|_| false,
            source: &|_| false,
            instrument: &|_| None,
            state_id: &|_| None,
        };
        assert_eq!(
            ladder_placement(
                "minecraft:ladder[facing=north,waterlogged=false]",
                Click {
                    face: Face::South,
                    yaw: 180.0,
                    cursor_y: 0.0,
                },
                &around,
            ),
            None,
            "a ladder facing the clicked face refuses the placement"
        );
    }
}

/// `ChestBlock.getStateForPlacement`'s `getChestType` tail, port 3 of 5.
///
/// Paired for the same reason the stair's corners are: `LEFT` and `RIGHT` are
/// picked by `getClockWise` against `getCounterClockWise`, and one input
/// cannot tell that apart from a constant.
#[cfg(test)]
mod chest_tests {
    use super::*;

    fn chest(facing: &str, kind: &str) -> String {
        format!("minecraft:chest[facing={facing},type={kind},waterlogged=false]")
    }

    fn trapped(facing: &str, kind: &str) -> String {
        format!("minecraft:trapped_chest[facing={facing},type={kind},waterlogged=false]")
    }

    fn type_of(me: &str, world: &[((i32, i32, i32), String)]) -> &'static str {
        let at = |p: BlockPos| -> String {
            world
                .iter()
                .find(|(q, _)| *q == p)
                .map_or_else(|| "minecraft:air".to_string(), |(_, s)| s.clone())
        };
        let sturdy = |_p: BlockPos, _f: Face| -> bool { true };
        let around = Around {
            pos: (0, 0, 0),
            at: &at,
            sturdy: &sturdy,
            conductor: &|_| false,
            source: &|_| false,
            instrument: &|_| None,
            state_id: &|_| None,
        };
        chest_type(me, &around).expect("a chest state has facing and type")
    }

    /// **The pair that pins the handedness.** A north-facing chest is `LEFT`
    /// when its partner is one step **clockwise** of its facing (east) and
    /// `RIGHT` when the partner is counter-clockwise (west) — offsets 0-17
    /// and 18-35 of `getChestType`.
    ///
    /// Either assertion alone is satisfied by a constant, and by a
    /// transcription that swapped the two rotations, since that swaps both
    /// arms together.
    #[test]
    fn a_chest_takes_its_half_from_which_side_the_partner_is_on() {
        assert_eq!(
            type_of(
                &chest("north", "single"),
                &[((1, 0, 0), chest("north", "single"))]
            ),
            "left",
            "east is NORTH.getClockWise()",
        );
        assert_eq!(
            type_of(
                &chest("north", "single"),
                &[((-1, 0, 0), chest("north", "single"))]
            ),
            "right",
            "west is NORTH.getCounterClockWise()",
        );
    }

    /// **Check `240`'s own geometry.** Both chests are placed at yaw 0, which
    /// is `getHorizontalDirection() == SOUTH` and so `FACING == NORTH`, and
    /// the second goes down one step **east** of the first. From the second
    /// chest's point of view the partner is to its west, so it is the `RIGHT`
    /// half — and the first then becomes `LEFT` through the cascade, which
    /// `crate::shapes` tests.
    #[test]
    fn the_second_chest_of_the_checks_pair_is_the_right_half() {
        assert_eq!(
            type_of(
                &chest("north", "single"),
                &[((-1, 0, 0), chest("north", "single"))]
            ),
            "right",
        );
    }

    /// **The degenerate control: a chest on its own.** Passes under a
    /// clockwise/counter-clockwise swap, under a port that never read the
    /// neighbour's `TYPE`, and under one that compared positions instead of
    /// facings. It can only fail if the rule invents a partner.
    #[test]
    fn a_lone_chest_is_single() {
        assert_eq!(type_of(&chest("north", "single"), &[]), "single");
    }

    /// A neighbour facing a different way is not a partner. `getChestType`
    /// compares the placement's own `FACING` against the **facing**
    /// `candidatePartnerFacing` returns, not against whether it returned
    /// anything.
    #[test]
    fn two_chests_facing_different_ways_do_not_pair() {
        assert_eq!(
            type_of(
                &chest("north", "single"),
                &[((1, 0, 0), chest("east", "single"))]
            ),
            "single",
        );
    }

    /// **The neighbour has to still be `SINGLE`** — offsets 22-31. A third
    /// chest arriving beside a finished double stays single rather than
    /// rewriting it. Without that test this answers `left` and the world
    /// grows a three-chest pair.
    #[test]
    fn a_third_chest_does_not_join_a_finished_double() {
        assert_eq!(
            type_of(
                &chest("north", "single"),
                &[((1, 0, 0), chest("north", "left"))]
            ),
            "single",
        );
    }

    /// **`chestCanConnectTo` is `state.is(this)`, not `instanceof`.** Both
    /// blocks below are `ChestBlock`s, so a rule keyed on the class set
    /// pairs them and vanilla does not.
    #[test]
    fn a_plain_chest_does_not_pair_with_a_trapped_one() {
        assert_eq!(
            type_of(
                &chest("north", "single"),
                &[((1, 0, 0), trapped("north", "single"))]
            ),
            "single",
        );
        // ...and the same from the other side, so the test is not passed by
        // a rule that simply never pairs a trapped chest with anything.
        assert_eq!(
            type_of(
                &trapped("north", "single"),
                &[((1, 0, 0), trapped("north", "single"))]
            ),
            "left",
        );
    }
}

/// `StairBlock.getStairsShape`, port 2 of 5.
///
/// # Every corner assertion below is paired, and the pairing is the gate
///
/// The ladder port's red taught this: a rule that picks one of two things
/// **cannot be distinguished from a constant by a single input.** Setting
/// `facing = d` where vanilla sets `d.getOpposite()` passed the one-wall test
/// and only failed the two-wall one, and the same trap is here three times
/// over — `OUTER_LEFT` vs `OUTER_RIGHT`, `INNER_LEFT` vs `INNER_RIGHT`, and
/// `getCounterClockWise` vs `getClockWise`. So each corner is asserted with
/// its mirror image, and `straight_is_the_answer_for_a_stair_in_line` is the
/// degenerate case that goes on passing under every one of those bugs.
#[cfg(test)]
mod stair_tests {
    use super::*;

    /// The two stair blocks these tests use. Real `is_stairs` is
    /// `instanceof StairBlock` off the class map; here it is spelled out so
    /// the test does not depend on `set_placed_by.json` being loadable.
    fn is_stairs(s: &str) -> bool {
        s.starts_with("minecraft:oak_stairs") || s.starts_with("minecraft:stone_stairs")
    }

    fn stair(facing: &str, half: &str) -> String {
        format!(
            "minecraft:oak_stairs[facing={facing},half={half},shape=straight,waterlogged=false]"
        )
    }

    /// The shape a stair at the origin takes, given a world of other states.
    fn shape_of(me: &str, world: &[((i32, i32, i32), String)]) -> &'static str {
        let at = |p: BlockPos| -> String {
            world
                .iter()
                .find(|(q, _)| *q == p)
                .map_or_else(|| "minecraft:air".to_string(), |(_, s)| s.clone())
        };
        let sturdy = |_p: BlockPos, _f: Face| -> bool { true };
        let around = Around {
            pos: (0, 0, 0),
            at: &at,
            sturdy: &sturdy,
            conductor: &|_| false,
            source: &|_| false,
            instrument: &|_| None,
            state_id: &|_| None,
        };
        stairs_shape(&is_stairs, me, &around).expect("a stair state has facing and half")
    }

    /// **The outer corner, and its mirror.** A stair facing east with a
    /// north-facing stair *in front of it* turns `outer_left`; swap the
    /// neighbour to south-facing and the same rule must answer
    /// `outer_right`.
    ///
    /// `EAST.getCounterClockWise()` is `NORTH`, so the first case is the
    /// `== ccw` arm and the second is the fall-through. **Either assertion
    /// alone is passed by a constant**, and by a `getClockWise` transcription
    /// too, since that swaps both arms at once.
    #[test]
    fn an_outer_corner_takes_its_handedness_from_the_stair_in_front() {
        assert_eq!(
            shape_of(
                &stair("east", "bottom"),
                &[((1, 0, 0), stair("north", "bottom"))]
            ),
            "outer_left",
        );
        assert_eq!(
            shape_of(
                &stair("east", "bottom"),
                &[((1, 0, 0), stair("south", "bottom"))]
            ),
            "outer_right",
        );
    }

    /// **The inner corner, and its mirror.** Same stair, same two neighbour
    /// facings, but the neighbour is now *behind* — and behind is what makes
    /// a corner inner rather than outer. A port that read `pos.relative(FACING)`
    /// for both neighbours answers `outer_*` for all four cases in this
    /// module and this test is one of the two that says so.
    #[test]
    fn an_inner_corner_takes_its_handedness_from_the_stair_behind() {
        assert_eq!(
            shape_of(
                &stair("east", "bottom"),
                &[((-1, 0, 0), stair("north", "bottom"))]
            ),
            "inner_left",
        );
        assert_eq!(
            shape_of(
                &stair("east", "bottom"),
                &[((-1, 0, 0), stair("south", "bottom"))]
            ),
            "inner_right",
        );
    }

    /// **The degenerate case: it passes under every bug the tests above
    /// catch.** Three stairs in a row all facing east stay `straight`,
    /// because `ff.getAxis() != facing.getAxis()` fails on both sides. Left
    /// and right, front and back, clockwise and counter-clockwise are all
    /// unreachable from here — which is exactly why a suite containing only
    /// this test would be one of the eighteen checks that cannot fail.
    #[test]
    fn straight_is_the_answer_for_a_stair_in_line() {
        assert_eq!(
            shape_of(
                &stair("east", "bottom"),
                &[
                    ((1, 0, 0), stair("east", "bottom")),
                    ((-1, 0, 0), stair("east", "bottom")),
                ],
            ),
            "straight",
        );
    }

    /// The other degenerate: a corner-shaped neighbour on the **other half**
    /// is not a corner at all. `state.getValue(HALF) == nb.getValue(HALF)`
    /// gates both branches, offsets 32-47 and 139-154.
    #[test]
    fn a_stair_does_not_corner_with_the_other_half() {
        assert_eq!(
            shape_of(
                &stair("east", "bottom"),
                &[((1, 0, 0), stair("north", "top"))]
            ),
            "straight",
        );
    }

    /// **The `canTakeShape` asymmetry, which only a third stair can see.**
    ///
    /// The outer branch calls `canTakeShape(..., ff.getOpposite())` and the
    /// inner branch calls `canTakeShape(..., bf)` — offsets 89 and 194. Both
    /// name the block on the far side of the corner, reached by opposite
    /// expressions because the neighbour is in front in one case and behind
    /// in the other.
    ///
    /// Here the far-side block is an identical stair, so `canTakeShape` is
    /// false and the corner is refused in **both** directions. A port that
    /// "tidied" the asymmetry by opposing both arguments would probe the
    /// empty position on the other side, find air, and answer `outer_left` /
    /// `inner_left`. Neither of the two-case tests above can see that: they
    /// have nothing at either probe position.
    #[test]
    fn a_third_stair_on_the_far_side_refuses_the_corner() {
        // Outer: front neighbour faces north, so the probe is at SOUTH.
        assert_eq!(
            shape_of(
                &stair("east", "bottom"),
                &[
                    ((1, 0, 0), stair("north", "bottom")),
                    ((0, 0, 1), stair("east", "bottom")),
                ],
            ),
            "straight",
            "the outer probe is at ff.getOpposite(), which is SOUTH",
        );
        // Inner: back neighbour faces north, so the probe is at NORTH.
        assert_eq!(
            shape_of(
                &stair("east", "bottom"),
                &[
                    ((-1, 0, 0), stair("north", "bottom")),
                    ((0, 0, -1), stair("east", "bottom")),
                ],
            ),
            "straight",
            "the inner probe is at bf itself, which is NORTH",
        );
    }

    /// The front branch is tried first and returns without consulting the
    /// back one — offsets 107 and 111 are `areturn`. With a cornering stair
    /// on both sides the answer is the **outer** one.
    #[test]
    fn the_front_neighbour_wins_over_the_back_one() {
        assert_eq!(
            shape_of(
                &stair("east", "bottom"),
                &[
                    ((1, 0, 0), stair("north", "bottom")),
                    ((-1, 0, 0), stair("south", "bottom")),
                ],
            ),
            "outer_left",
        );
    }
}

/// Java's `Math.round(float)` — **half up**, not half away from zero.
///
/// Rust's `f32::round` rounds halves away from zero, so the two disagree on
/// every negative half: Java answers `-2` for `-2.5` and Rust answers `-3`.
/// Yaw reaches this function unnormalised, straight off the move packet —
/// `Entity.setYRot` does not wrap and a client may send any float — so the
/// negative half is reachable rather than theoretical.
fn java_round(v: f32) -> i32 {
    // The specification is `(int) Math.floor(a + 0.5f)`, and the addition is
    // in float. Done in f32 for the same reason: vanilla's
    // `degrees * degreeToAngle` is a float multiply and rounding a widened
    // f64 product would disagree at the ties this whole function exists for.
    (v + 0.5).floor() as i32
}

/// The number of sixteenths of a turn a sign, banner or head is placed at.
///
/// # Why this cannot be a table entry
///
/// `placement.json` has **four** yaw columns — `Direction.fromYRot` rounds
/// the placer's yaw to one of four compass directions — and `rotation` has
/// **sixteen** values. Every off-axis yaw therefore lands in a column whose
/// answer is up to two segments wrong. The corpus caught it at yaw 22.5,
/// where vanilla answers 9 and the SOUTH column says 8.
///
/// # The rule, and the trap in it
///
/// From the bytecode, `StandingSignBlock.getStateForPlacement` is
/// `ROTATION = RotationSegment.convertToSegment(ctx.getRotation() + 180.0f)`,
/// `convertToSegment` is `SEGMENTED_ANGLE16.fromDegrees`, and
/// `SegmentedAnglePrecision(4)` gives `mask = (1 << 4) - 1 = 15` and
/// `degreeToAngle = 16f / 360f`, so `fromDegrees(f)` is
/// `Math.round(f * (16f / 360f)) & 15`.
///
/// **The `+ 180.0f` is not shared, and assuming it is would put a quarter of
/// these blocks half a turn out.** Read from the jar, the 47 blocks carrying
/// a `rotation` property split three ways:
///
/// * `StandingSignBlock` and `BannerBlock` — `getRotation() + 180.0f`;
/// * `SkullBlock` — `getRotation()`, with **no offset at all**, so a head
///   faces the way the placer does and a sign faces back at them;
/// * `CeilingHangingSignBlock` — two branches, one of them
///   `convertToSegment(Direction)` for the attached case. Not handled here;
///   see the caveat below.
///
/// # Where the offset comes from, so that nothing is guessed
///
/// Not from a list of block names. The table already holds the right answer
/// at the four yaws it was clicked at, so the offset is recovered from the
/// table's own answer for *this* click:
///
/// Because `180 * (16/360)` is exactly `8` and `0 * (16/360)` is exactly `0`,
/// the offset is a whole number of segments, and adding a whole number
/// commutes with rounding — `round(x + n) == round(x) + n`. So if column `c`
/// is the yaw `90c` that produced the table's value, then
/// `table = (4c + base) & 15` and `base = (table - 4c) & 15`, and the exact
/// answer is `(round(yaw * 16/360) + base) & 15`.
///
/// This reproduces the table exactly at every multiple of 90 and refines it
/// in between, which is the whole change.
///
/// **Caveat, said out loud rather than left to be found:** a ceiling hanging
/// sign whose placement took the `attached` branch is refined by a rule that
/// is not its own. The error is zero at the four sampled yaws and at most two
/// segments away from them, and no check in the corpus places one.
fn refine_rotation(state: &str, yaw: f32) -> String {
    let Some(current) = property_of(state, "rotation").and_then(|v| v.parse::<i32>().ok()) else {
        return state.to_string();
    };
    // Which column the table answered from, and therefore what is already
    // baked into `current`.
    let column = i32::try_from(from_y_rot(yaw)).unwrap_or(0);
    let base = (current - 4 * column) & 15;
    let refined = (java_round(yaw * (16.0 / 360.0)) + base) & 15;
    with_property(state, "rotation", &refined.to_string())
}

/// One property's value out of a state string, or `None` if it has no such
/// property.
pub fn property_of<'a>(state: &'a str, name: &str) -> Option<&'a str> {
    let open = state.find('[')?;
    state[open + 1..]
        .trim_end_matches(']')
        .split(',')
        .find_map(|pair| match pair.split_once('=') {
            Some((k, v)) if k == name => Some(v),
            _ => None,
        })
}

/// `Direction.getAxis`, from the enum's own normals.
const fn axis_of(face: Face) -> &'static str {
    match face {
        Face::Down | Face::Up => "y",
        Face::North | Face::South => "z",
        Face::West | Face::East => "x",
    }
}

/// Replace one property's value in a state string, in place.
///
/// In place, not appended: vanilla's state strings list properties in a fixed
/// order and the table is keyed by the whole string, so a rebuilt string in a
/// different order is a state nobody has heard of. A block without the
/// property is returned unchanged, which is how "only if the default has it"
/// is enforced.
pub fn with_property(state: &str, name: &str, value: &str) -> String {
    let Some(open) = state.find('[') else {
        return state.to_string();
    };
    let (block, props) = state.split_at(open);
    let inner = props.trim_start_matches('[').trim_end_matches(']');
    let mut out = Vec::new();
    let mut found = false;
    for pair in inner.split(',') {
        match pair.split_once('=') {
            Some((k, _)) if k == name => {
                found = true;
                out.push(format!("{name}={value}"));
            }
            _ => out.push(pair.to_string()),
        }
    }
    if !found {
        return state.to_string();
    }
    format!("{block}[{}]", out.join(","))
}

#[cfg(test)]
mod replaceable_tests {
    use super::*;
    use crate::interact::Face;

    /// A table that knows four items and which of their blocks are slabs and
    /// snow. Built from `Default` plus the two resolved-set builders rather
    /// than from a file: the rule under test reads `asItem()` and two class
    /// sets and nothing else, and a fixture that loaded the real extract
    /// would be testing the loader.
    fn table() -> PlacementTable {
        let items = [
            ("minecraft:stone", "minecraft:stone"),
            ("minecraft:oak_slab", "minecraft:oak_slab"),
            ("minecraft:spruce_slab", "minecraft:spruce_slab"),
            ("minecraft:snow", "minecraft:snow"),
            ("minecraft:tall_grass", "minecraft:tall_grass"),
            ("minecraft:candle", "minecraft:candle"),
        ]
        .into_iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
        PlacementTable::default()
            .with_items(items)
            .with_slabs(
                ["minecraft:oak_slab", "minecraft:spruce_slab"]
                    .into_iter()
                    .map(String::from)
                    .collect(),
            )
            .with_snow_layers(std::iter::once("minecraft:snow".to_string()).collect())
            .with_candles(std::iter::once("minecraft:candle".to_string()).collect())
    }

    /// `CandleBlock.canBeReplaced` and `getStateForPlacement` together: a
    /// candle clicked with a candle takes a second one — the check
    /// `a-candle-placed-on-a-candle-makes-it-two-candles`, whose vanilla
    /// recording writes `candles=2` at the clicked position — until four, and
    /// never while sneaking.
    #[test]
    fn a_candle_takes_another_candle_up_to_four_and_not_while_sneaking() {
        let t = table();
        let one = "minecraft:candle[candles=1,lit=false,waterlogged=false]";
        let four = "minecraft:candle[candles=4,lit=false,waterlogged=false]";
        let c = ctx(&t, "minecraft:candle", Face::Up, 1.0, true);
        assert!(can_be_replaced(one, false, &c));
        assert!(!can_be_replaced(four, false, &c), "four is full");
        assert!(!can_be_replaced(
            one,
            false,
            &Replacing {
                secondary_use: true,
                ..c
            }
        ));
        assert!(!can_be_replaced(
            one,
            false,
            &ctx(&t, "minecraft:stone", Face::Up, 1.0, true)
        ));
        assert_eq!(
            merged_into(
                Some(&t),
                "minecraft:candle[candles=1,lit=false,waterlogged=false]",
                one
            )
            .as_deref(),
            Some("minecraft:candle[candles=2,lit=false,waterlogged=false]")
        );
    }

    fn ctx<'a>(
        t: &'a PlacementTable,
        item: &'a str,
        face: Face,
        cursor_y: f64,
        replacing_clicked: bool,
    ) -> Replacing<'a> {
        Replacing {
            item: Some(item),
            face,
            cursor_y,
            replacing_clicked,
            secondary_use: false,
            table: Some(t),
        }
    }

    /// `BlockBehaviour.canBeReplaced(state, ctx)`, offsets 0-36 — and the
    /// second conjunct, which is the whole difference from the no-argument
    /// overload this used to call.
    #[test]
    fn the_default_rule_refuses_the_block_you_are_already_holding() {
        let t = table();
        let grass = "minecraft:tall_grass[half=lower]";
        assert!(
            can_be_replaced(
                grass,
                true,
                &ctx(&t, "minecraft:stone", Face::Up, 0.5, true)
            ),
            "tall grass with stone in hand: replaceable, which is what 020 \
             places into and what the old position-only rule also said"
        );
        assert!(
            !can_be_replaced(
                grass,
                true,
                &ctx(&t, "minecraft:tall_grass", Face::Up, 0.5, true)
            ),
            "tall grass with TALL GRASS in hand: not replaceable — \
             `!item.is(asItem())` at offsets 17-28, and the placement steps \
             along the face instead. The position-only rule could not say this"
        );
        assert!(
            can_be_replaced(
                grass,
                true,
                &Replacing {
                    item: None,
                    ..ctx(&t, "minecraft:tall_grass", Face::Up, 0.5, true)
                }
            ),
            "an empty hand short-circuits the conjunct at offset 14 — \
             `item.isEmpty() ||`"
        );
        assert!(
            !can_be_replaced(
                "minecraft:stone",
                false,
                &ctx(&t, "minecraft:oak_slab", Face::Up, 0.5, true)
            ),
            "and the flag still gates the default rule: stone is not \
             replaceable by anything"
        );
    }

    /// `SlabBlock.canBeReplaced`, offsets 0-149 — all four of its answers.
    ///
    /// The last two are `120`'s territory: a slab clicked on its **side**
    /// merges or does not depending on which half of the block the cursor is
    /// in, which is the same quantity `120` reads to decide `type`.
    #[test]
    fn a_slab_is_replaced_only_by_its_own_kind_from_the_right_side() {
        let t = table();
        let bottom = "minecraft:oak_slab[type=bottom,waterlogged=false]";
        let top = "minecraft:oak_slab[type=top,waterlogged=false]";
        let double = "minecraft:oak_slab[type=double,waterlogged=false]";
        let held = |i: &'static str, f: Face, y: f64| {
            can_be_replaced(bottom, false, &ctx(&t, i, f, y, true))
        };

        assert!(
            held("minecraft:oak_slab", Face::Up, 0.5),
            "170: a bottom slab clicked from UP takes the second slab — and \
             note the props flag is FALSE for every slab state, so this \
             answer cannot come from the table"
        );
        assert!(
            !held("minecraft:spruce_slab", Face::Up, 0.5),
            "a different wood is a different block: `is(asItem())` is false \
             and it stacks instead"
        );
        assert!(
            !can_be_replaced(
                double,
                false,
                &ctx(&t, "minecraft:oak_slab", Face::Up, 0.5, true)
            ),
            "a double slab is full — the `type == DOUBLE` test at offsets 17-22"
        );
        assert!(
            !held("minecraft:oak_slab", Face::Down, 0.5),
            "a BOTTOM slab clicked from below is not replaced"
        );
        assert!(
            held("minecraft:oak_slab", Face::North, 0.7),
            "clicked on the side above the halfway line: `above && \
             f.getAxis().isHorizontal()`"
        );
        assert!(
            !held("minecraft:oak_slab", Face::North, 0.3),
            "and below it, not — 0.5 exactly is `dcmpl; ifle`, so the \
             comparison is strictly greater"
        );
        assert!(
            !held("minecraft:oak_slab", Face::North, 0.5),
            "0.5 exactly is not above"
        );
        // The TOP slab arm is the mirror image and is asserted rather than
        // assumed symmetric, because the two branches are written out
        // separately in the bytecode (119-143) and either could be wrong.
        let top_held = |f: Face, y: f64| {
            can_be_replaced(top, false, &ctx(&t, "minecraft:oak_slab", f, y, true))
        };
        assert!(top_held(Face::Down, 0.5), "a TOP slab is taken from below");
        assert!(!top_held(Face::Up, 0.5), "and not from above");
        assert!(top_held(Face::North, 0.3), "side, below the line");
        assert!(!top_held(Face::North, 0.7), "side, above it: no");
        // `canPlace()`'s context, where the flag is genuinely false.
        assert!(
            can_be_replaced(
                bottom,
                false,
                &ctx(&t, "minecraft:oak_slab", Face::Down, 0.5, false)
            ),
            "with replacingClickedOnBlock() false the face is not consulted \
             at all — offset 42 jumps to the bare `return true` at 149"
        );
    }

    /// `SnowLayerBlock.canBeReplaced`, offsets 0-69.
    #[test]
    fn snow_deepens_up_to_eight_and_a_single_layer_is_replaceable_by_anything() {
        let t = table();
        let snow = |n: u8| format!("minecraft:snow[layers={n}]");
        assert!(
            can_be_replaced(
                &snow(1),
                true,
                &ctx(&t, "minecraft:snow", Face::Up, 0.5, true)
            ),
            "250: snow on snow from above"
        );
        assert!(
            can_be_replaced(
                &snow(7),
                true,
                &ctx(&t, "minecraft:snow", Face::Up, 0.5, true)
            ),
            "up to seven"
        );
        assert!(
            !can_be_replaced(
                &snow(8),
                true,
                &ctx(&t, "minecraft:snow", Face::Up, 0.5, true)
            ),
            "eight is full: `layers < 8` at offsets 28-31, and the tail then \
             asks `layers == 1`, which is false"
        );
        assert!(
            !can_be_replaced(
                &snow(3),
                true,
                &ctx(&t, "minecraft:snow", Face::North, 0.5, true)
            ),
            "clicked on the side while replacing the clicked block: the \
             answer is `getClickedFace() == UP` and nothing else"
        );
        assert!(
            can_be_replaced(
                &snow(1),
                true,
                &ctx(&t, "minecraft:stone", Face::Up, 0.5, true)
            ),
            "one layer is replaceable by anything at all — the `layers == 1` \
             tail at 59-69, which is what makes a single layer disappear \
             under a block placed on it"
        );
        assert!(
            !can_be_replaced(
                &snow(2),
                true,
                &ctx(&t, "minecraft:stone", Face::Up, 0.5, true)
            ),
            "and two layers are not"
        );
    }

    /// The other half: what a merging placement writes.
    #[test]
    fn a_merge_builds_on_the_state_that_is_already_there() {
        let t = table();
        let m = |default: &str, into: &str| merged_into(Some(&t), default, into);
        assert_eq!(
            m(
                "minecraft:oak_slab[type=bottom,waterlogged=false]",
                "minecraft:oak_slab[type=top,waterlogged=true]"
            )
            .as_deref(),
            Some("minecraft:oak_slab[type=double,waterlogged=false]"),
            "built on `into`, and the waterlogging is cleared explicitly at \
             offsets 35-42 rather than inherited from the default"
        );
        assert_eq!(
            m(
                "minecraft:oak_slab[type=bottom,waterlogged=false]",
                "minecraft:spruce_slab[type=bottom,waterlogged=false]"
            ),
            None,
            "`is(this)` is a block identity test: two woods do not merge"
        );
        assert_eq!(
            m("minecraft:snow[layers=1]", "minecraft:snow[layers=1]").as_deref(),
            Some("minecraft:snow[layers=2]")
        );
        assert_eq!(
            m("minecraft:snow[layers=1]", "minecraft:snow[layers=8]").as_deref(),
            Some("minecraft:snow[layers=8]"),
            "`Math.min(8, layers + 1)` — clamped, not wrapped"
        );
        assert_eq!(
            m("minecraft:stone", "minecraft:stone"),
            None,
            "no rule for anything else, however equal the two states are"
        );
        assert_eq!(
            merged_into(None, "minecraft:snow[layers=1]", "minecraft:snow[layers=1]"),
            None,
            "and without the class sets nothing merges, which is the \
             behaviour this server had before the rule existed"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `getNearestLookingDirection().getOpposite()` for a piston, the
    /// looking direction itself for an observer, and the crafter's
    /// `FrontAndTop` switch — at the four yaws and both vertical extremes.
    #[test]
    fn a_look_facing_block_faces_the_way_the_player_looked() {
        let any = |_: &str| true;
        let piston = "minecraft:piston[extended=false,facing=north]";
        let face = |x: f32, y: f32| look_facing(piston, x, y, &any).expect("a piston");
        // yaw 0 looks south, 90 west, 180 north, -90 east.
        assert_eq!(
            face(0.0, 0.0),
            "minecraft:piston[extended=false,facing=north]"
        );
        assert_eq!(
            face(0.0, 90.0),
            "minecraft:piston[extended=false,facing=east]"
        );
        assert_eq!(
            face(0.0, 180.0),
            "minecraft:piston[extended=false,facing=south]"
        );
        assert_eq!(
            face(0.0, -90.0),
            "minecraft:piston[extended=false,facing=west]"
        );
        // Positive pitch is looking down.
        assert_eq!(
            face(90.0, 0.0),
            "minecraft:piston[extended=false,facing=up]"
        );
        assert_eq!(
            face(-90.0, 0.0),
            "minecraft:piston[extended=false,facing=down]"
        );
        assert_eq!(
            look_facing(
                "minecraft:observer[facing=south,powered=false]",
                0.0,
                90.0,
                &any
            )
            .as_deref(),
            Some("minecraft:observer[facing=west,powered=false]")
        );
        let crafter = "minecraft:crafter[crafting=false,orientation=north_up,triggered=false]";
        let orient = |x: f32, y: f32| look_facing(crafter, x, y, &any).expect("a crafter");
        assert!(orient(0.0, 0.0).contains("orientation=north_up"));
        assert!(orient(90.0, 0.0).contains("orientation=up_south"));
        assert!(orient(-90.0, 0.0).contains("orientation=down_north"));
        assert_eq!(
            look_facing("minecraft:furnace[facing=north,lit=false]", 0.0, 90.0, &any),
            None
        );
        assert_eq!(
            look_facing(piston, 0.0, 90.0, &|_| false),
            None,
            "an unknown state is not placed"
        );
    }

    /// Everything is a known state, for the tests that are about the rule
    /// rather than about the guard.
    fn anything(_: &str) -> bool {
        true
    }

    /// A click with no table behind it: the fallback path, where only the
    /// `axis` and `waterlogged` rules run.
    fn clicked(face: Face) -> Click {
        Click {
            face,
            yaw: 0.0,
            cursor_y: 0.25,
        }
    }

    /// A log takes the axis of the face it was clicked on — the whole of
    /// `RotatedPillarBlock.getStateForPlacement`, and the answer when the
    /// table is absent.
    #[test]
    fn a_pillar_takes_the_clicked_faces_axis() {
        let log = "minecraft:oak_log[axis=y]";
        for (face, want) in [
            (Face::Up, "minecraft:oak_log[axis=y]"),
            (Face::North, "minecraft:oak_log[axis=z]"),
            (Face::East, "minecraft:oak_log[axis=x]"),
        ] {
            assert_eq!(
                state_for_placement(None, log, clicked(face), "minecraft:air", &anything, None)
                    .as_deref(),
                Some(want)
            );
        }
    }

    /// A block without the property is untouched — the rule applies to the
    /// blocks that have it and to no others.
    #[test]
    fn a_block_without_an_axis_is_unchanged() {
        for state in ["minecraft:stone", "minecraft:oak_stairs[facing=north]"] {
            assert_eq!(
                state_for_placement(
                    None,
                    state,
                    clicked(Face::East),
                    "minecraft:air",
                    &anything,
                    None
                )
                .as_deref(),
                Some(state)
            );
        }
    }

    /// Only a **source** waterlogs: flowing water's fluid type is
    /// `FLOWING_WATER` and the test in every `SimpleWaterloggedBlock` is
    /// against `WATER`.
    #[test]
    fn only_a_water_source_waterlogs() {
        let slab = "minecraft:oak_slab[type=bottom,waterlogged=false]";
        let place =
            |into: &str| state_for_placement(None, slab, clicked(Face::Up), into, &anything, None);
        assert_eq!(
            place(WATER_SOURCE).as_deref(),
            Some("minecraft:oak_slab[type=bottom,waterlogged=true]")
        );
        assert_eq!(
            place("minecraft:water[level=3]").as_deref(),
            Some(slab),
            "flowing water is not a source"
        );
        assert_eq!(place("minecraft:air").as_deref(), Some(slab));
    }

    /// Both at once, and the property order is the table's rather than
    /// whatever rebuilding it would produce.
    #[test]
    fn two_properties_change_without_reordering_the_rest() {
        let chain = "minecraft:chain[axis=y,waterlogged=false]";
        assert_eq!(
            state_for_placement(
                None,
                chain,
                clicked(Face::East),
                WATER_SOURCE,
                &anything,
                None
            )
            .as_deref(),
            Some("minecraft:chain[axis=x,waterlogged=true]")
        );
    }

    /// The guard. `minecraft:nether_portal`'s axis takes x or z and never y,
    /// so a click on its top face computes a state that does not exist — and
    /// a state that does not exist has no network id, which would fail the
    /// *next* encode of the whole chunk rather than this one placement.
    #[test]
    fn an_impossible_state_falls_back_to_the_default() {
        let portal = "minecraft:nether_portal[axis=x]";
        let real = |s: &str| s != "minecraft:nether_portal[axis=y]";
        assert_eq!(
            state_for_placement(
                None,
                portal,
                clicked(Face::Up),
                "minecraft:air",
                &real,
                None
            )
            .as_deref(),
            Some(portal),
            "the default, not a state nobody has heard of"
        );
        // And the guard does not fire when the state is fine.
        assert_eq!(
            state_for_placement(
                None,
                portal,
                clicked(Face::North),
                "minecraft:air",
                &real,
                None
            )
            .as_deref(),
            Some("minecraft:nether_portal[axis=z]")
        );
    }

    /// `Direction.fromYRot`, at the four sampled yaws and between them.
    ///
    /// The between-them cases are the point: the table has four yaw columns
    /// and a player's yaw is continuous, so this is what makes a 4-column
    /// table exact rather than a 4-point sample. 44 degrees is still south,
    /// 46 is already west, and the boundary is vanilla's own rounding.
    #[test]
    fn a_yaw_rounds_to_the_cardinal_vanilla_rounds_it_to() {
        // SOUTH 0, WEST 1, NORTH 2, EAST 3 — `from2DDataValue` order.
        for (yaw, want) in [
            (0.0, 0),
            (44.0, 0),
            (46.0, 1),
            (90.0, 1),
            (180.0, 2),
            (270.0, 3),
            (359.0, 0),
            (-90.0, 3),
            (-180.0, 2),
            (720.0, 0),
        ] {
            assert_eq!(from_y_rot(yaw), want, "yaw {yaw}");
        }
    }

    /// The whole point of the table, on the two blocks that motivated it:
    /// a stair faces the placer and a furnace faces away, from one yaw.
    ///
    /// Built here as a two-row file rather than loaded, so the test says what
    /// it is testing — the indexing — and does not depend on an extract a
    /// checkout may not have. The cells are the real ones: the stair row is
    /// what `data/vanilla/placement.json` carries for `minecraft:oak_stairs`
    /// at the UP face, and the furnace row is its opposite.
    #[test]
    fn a_stair_faces_the_placer_and_a_furnace_faces_away() {
        let table = two_row_table();
        let yaws = [
            (0.0, "south"),
            (90.0, "west"),
            (180.0, "north"),
            (270.0, "east"),
        ];
        for (yaw, facing) in yaws {
            let click = Click {
                face: Face::Up,
                yaw,
                cursor_y: 0.25,
            };
            assert_eq!(
                table.answer("minecraft:oak_stairs[facing=north]", click),
                Answer::State(&format!("minecraft:oak_stairs[facing={facing}]"))
            );
        }
        // And the opposite, from the same yaws.
        for ((yaw, _), away) in yaws.into_iter().zip(["north", "east", "south", "west"]) {
            let click = Click {
                face: Face::Up,
                yaw,
                cursor_y: 0.25,
            };
            assert_eq!(
                table.answer("minecraft:furnace[facing=north]", click),
                Answer::State(&format!("minecraft:furnace[facing={away}]"))
            );
        }
    }

    /// A row the dumper flagged is not read at all. `minecraft:dispenser` is
    /// pitch-sensitive and `minecraft:weeping_vines` is random, and serving
    /// either from the level-pitch row would be wrong-and-confident — the
    /// exact failure this module exists to avoid.
    #[test]
    fn a_flagged_row_answers_unknown_rather_than_guessing() {
        let table = flagged_table();
        let click = Click {
            face: Face::Up,
            yaw: 0.0,
            cursor_y: 0.25,
        };
        assert_eq!(
            table.answer("minecraft:dispenser[facing=north]", click),
            Answer::Unknown
        );
        assert_eq!(
            table.answer("minecraft:weeping_vines[age=0]", click),
            Answer::Unknown
        );
        // And a refusal is carried through as a refusal, not as the default.
        assert_eq!(
            table.answer("minecraft:oak_door[half=lower]", click),
            Answer::Refused
        );
        // Which `state_for_placement` turns into "place nothing".
        assert_eq!(
            state_for_placement(
                Some(&table),
                "minecraft:oak_door[half=lower]",
                click,
                "minecraft:air",
                &anything,
                None
            ),
            None
        );
    }

    /// The header is checked. A dump whose face order changed would index
    /// every block's answer through the wrong column, and the symptom would
    /// be "some blocks face the wrong way" — indistinguishable from the bug
    /// this table replaced.
    #[test]
    fn a_dump_with_a_different_grid_is_refused() {
        let dir = std::env::temp_dir().join(format!("rustmc-placement-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("placement.json");
        std::fs::write(
            &path,
            r#"{"faceOrder": ["UP", "DOWN", "NORTH", "SOUTH", "WEST", "EAST"],
                "yawDirections": ["SOUTH", "WEST", "NORTH", "EAST"],
                "cellStride": "((face * 4 + yaw) * 3 + pitch) * 2 + cursor",
                "table": []}"#,
        )
        .expect("write");
        let err = PlacementTable::load(&path).expect_err("a transposed grid is refused");
        assert!(
            err.to_string().contains("faceOrder"),
            "the error names the axis that moved: {err}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// One `UP`-face row per block, every cell the same, so the tests above
    /// are about the yaw column and nothing else.
    fn table_from(rows: &[(&str, [&str; 4], bool, bool)]) -> PlacementTable {
        let mut blocks = BTreeMap::new();
        for &(block, facings, usable, refused) in rows {
            let states: Vec<String> = facings.iter().map(|s| (*s).to_string()).collect();
            let mut cells = vec![0; PlacementTable::CELLS];
            for face in 0..6 {
                for yaw in 0..4 {
                    for pitch in 0..3 {
                        for cursor in 0..2 {
                            let at = ((face * 4 + yaw) * 3 + pitch) * 2 + cursor;
                            cells[at] = if refused {
                                -1
                            } else {
                                i32::try_from(yaw).expect("small")
                            };
                        }
                    }
                }
            }
            blocks.insert(
                block.to_string(),
                Entry {
                    usable,
                    states,
                    cells,
                },
            );
        }
        PlacementTable {
            blocks,
            items: BTreeMap::new(),
            wall_items: BTreeMap::new(),
            by_block: BTreeMap::new(),
            stairs: std::collections::BTreeSet::new(),
            chests: std::collections::BTreeSet::new(),
            slabs: std::collections::BTreeSet::new(),
            snow_layers: std::collections::BTreeSet::new(),
            candles: std::collections::BTreeSet::new(),
        }
    }

    fn two_row_table() -> PlacementTable {
        table_from(&[
            (
                "minecraft:oak_stairs",
                [
                    "minecraft:oak_stairs[facing=south]",
                    "minecraft:oak_stairs[facing=west]",
                    "minecraft:oak_stairs[facing=north]",
                    "minecraft:oak_stairs[facing=east]",
                ],
                true,
                false,
            ),
            (
                "minecraft:furnace",
                [
                    "minecraft:furnace[facing=north]",
                    "minecraft:furnace[facing=east]",
                    "minecraft:furnace[facing=south]",
                    "minecraft:furnace[facing=west]",
                ],
                true,
                false,
            ),
        ])
    }

    fn flagged_table() -> PlacementTable {
        table_from(&[
            ("minecraft:dispenser", ["a", "b", "c", "d"], false, false),
            (
                "minecraft:weeping_vines",
                ["a", "b", "c", "d"],
                false,
                false,
            ),
            ("minecraft:oak_door", ["a", "b", "c", "d"], true, true),
        ])
    }

    /// The case the corpus caught: yaw 22.5 on a standing sign. Vanilla
    /// answered `rotation=9`; the SOUTH column of the table says 8, and
    /// `round(22.5 * 16/360)` is 1, so `1 + 8 = 9`.
    #[test]
    fn a_sign_takes_a_sixteenth_of_a_turn_between_the_columns() {
        let s = "minecraft:oak_sign[rotation=8,waterlogged=false]";
        assert_eq!(
            refine_rotation(s, 22.5),
            "minecraft:oak_sign[rotation=9,waterlogged=false]"
        );
        // And it reproduces the table exactly on the axes it was sampled at.
        for (yaw, want) in [(0.0, 8), (90.0, 12), (180.0, 0), (270.0, 4)] {
            let col = from_y_rot(yaw) as i32;
            let table = format!(
                "minecraft:oak_sign[rotation={},waterlogged=false]",
                (4 * col + 8) & 15
            );
            assert_eq!(
                property_of(&refine_rotation(&table, yaw), "rotation"),
                Some(want.to_string().as_str()),
                "at yaw {yaw}"
            );
        }
    }

    /// **A head has no `+ 180`.** Same yaw, same machinery, an answer half a
    /// turn away from the sign's — which is the whole reason the offset is
    /// recovered from the table instead of written down as a constant.
    #[test]
    fn a_head_faces_the_way_the_placer_does_and_a_sign_faces_back() {
        let head = "minecraft:skeleton_skull[powered=false,rotation=0]";
        assert_eq!(
            property_of(&refine_rotation(head, 22.5), "rotation"),
            Some("1")
        );
        let sign = "minecraft:oak_sign[rotation=8,waterlogged=false]";
        assert_eq!(
            property_of(&refine_rotation(sign, 22.5), "rotation"),
            Some("9")
        );
    }

    /// Java rounds halves **up**, Rust rounds them away from zero, and yaw
    /// arrives unnormalised so the negative half is reachable. `f32::round`
    /// would answer -3 where Java answers -2.
    #[test]
    fn the_rounding_is_javas_and_not_rusts() {
        assert_eq!(java_round(-2.5), -2);
        assert_eq!(java_round(2.5), 3);
        assert_eq!(java_round(-2.5_f32), -2, "and f32::round would say -3");
        assert_eq!((-2.5_f32).round() as i32, -3, "which is the disagreement");
    }

    /// A block with no `rotation` property is returned untouched, which is
    /// what stops this from being applied to the other thousand blocks.
    #[test]
    fn a_block_without_the_property_is_left_alone() {
        let s = "minecraft:stone";
        assert_eq!(refine_rotation(s, 22.5), s);
        let t = "minecraft:oak_stairs[facing=north,half=bottom]";
        assert_eq!(refine_rotation(t, 22.5), t);
    }

    /// The note block's instrument is written from the level and **not**
    /// from the table, and the table is deliberately given the wrong answer
    /// here so that a refinement which stopped running would be visible.
    ///
    /// This is the shape of the real defect: `placement.json` was sampled
    /// over the golden harness's own ground, so its cell carries that
    /// ground's instrument. Scenario 430's recording on 2026-08-17 has this
    /// server writing `instrument=basedrum` where vanilla writes
    /// `instrument=harp`, on grass.
    #[test]
    fn a_note_block_takes_its_instrument_from_the_level_not_the_table() {
        let table = PlacementTable::default();
        let sampled = "minecraft:note_block[instrument=basedrum,note=0,powered=false]";

        let refine = |answer: Option<&str>| -> String {
            let around = Around {
                pos: (0, 0, 0),
                at: &|_| "minecraft:air".to_string(),
                sturdy: &|_, _| false,
                conductor: &|_| false,
                source: &|_| false,
                instrument: &|_| answer.map(str::to_string),
                state_id: &|_| None,
            };
            refined_with_neighbours(Some(&table), sampled.to_string(), &around)
        };

        assert_eq!(
            refine(Some("harp")),
            "minecraft:note_block[instrument=harp,note=0,powered=false]",
            "the level's answer replaces the table's"
        );
        // Not one constant: a refinement hard-coded to `harp` would pass the
        // assertion above and fail this one.
        assert_eq!(
            refine(Some("bass")),
            "minecraft:note_block[instrument=bass,note=0,powered=false]"
        );
        // A table that cannot answer leaves the state alone rather than
        // guessing, so a stale `block_props.json` stays visible.
        assert_eq!(refine(None), sampled);
    }

    /// The refinement is keyed on the block and must not touch anything
    /// else, however the `instrument` closure answers.
    #[test]
    fn the_instrument_refinement_touches_only_note_blocks() {
        let table = PlacementTable::default();
        let around = Around {
            pos: (0, 0, 0),
            at: &|_| "minecraft:air".to_string(),
            sturdy: &|_, _| false,
            conductor: &|_| false,
            source: &|_| false,
            instrument: &|_| Some("bass".to_string()),
            state_id: &|_| None,
        };
        for state in [
            "minecraft:stone",
            "minecraft:grass_block[snowy=false]",
            "minecraft:jukebox[has_record=false]",
        ] {
            assert_eq!(
                refined_with_neighbours(Some(&table), state.to_string(), &around),
                state
            );
        }
    }
}
