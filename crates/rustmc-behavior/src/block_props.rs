//! Per-block-state facts that worldgen and gameplay both ask about.
//!
//! Source: `data/vanilla/block_props.json`, produced by
//! `ci/block-props-golden.sh` from `BlockPropsGolden.java`. Regenerable from
//! the jar in one command; gitignored, like the rest of the Mojang-derived
//! extract.
//!
//! # Why this is a table and not code
//!
//! Vanilla answers "is this face a full block" through `VoxelShape`,
//! `Shapes.joinIsNotEmpty` and a shape declaration on every block class —
//! thousands of lines of geometry computed once at bootstrap and cached in
//! `BlockBehaviour.BlockStateBase.cache`. Every question worldgen asks of it
//! reduces to a handful of booleans per state, so the cache is extracted
//! rather than the geometry reimplemented. Porting the geometry would be a
//! large surface with no gate on it; the table has an oracle by construction.
//!
//! This will not stay sufficient. A server has to answer shape questions
//! about *pairs* of blocks (collision, pathfinding, redstone), and those do
//! not reduce to per-state booleans. This is the worldgen-shaped subset,
//! deliberately, and it is named that way so nobody mistakes it for a shape
//! implementation.
//!
//! # The one that catches people
//!
//! `fluid` is not the block's name. `minecraft:seagrass` has a fluid state of
//! `minecraft:water`, and so do 11,792 of the 32,366 states — every
//! waterlogged variant of every fence, slab and stair. A port that answers
//! `matching_fluids` by looking at the block name gets every one of them
//! wrong, and gets them wrong in the direction that silently places nothing.

use std::collections::BTreeMap;
use std::path::Path;

/// `Direction.values()` order: DOWN, UP, NORTH, SOUTH, WEST, EAST.
///
/// Everything in vanilla iterates directions in this order, and the
/// `fullFaces` bitstring is written in it. A table whose column order
/// disagrees with the enum is a silent transposition, so the loaded file's
/// own `directionOrder` is checked against this rather than assumed.
pub const DIRECTIONS: [&str; 6] = ["DOWN", "UP", "NORTH", "SOUTH", "WEST", "EAST"];

/// The tag order [`BlockProps::place_tags`] is indexed by, checked against
/// the table's own `placeTagOrder` at load exactly as [`DIRECTIONS`] is
/// against `directionOrder`. A transposed tag column would be a wrong answer
/// with no symptom, which is what that check exists to prevent.
pub const PLACE_TAG_ORDER: [&str; 14] = [
    "minecraft:supports_vegetation",
    "minecraft:supports_crops",
    "minecraft:cannot_support_snow_layer",
    "minecraft:support_override_snow_layer",
    "minecraft:overrides_mushroom_light_requirement",
    "minecraft:wall_hanging_signs",
    "minecraft:supports_dry_vegetation",
    "minecraft:supports_azalea",
    "minecraft:supports_nether_sprouts",
    "minecraft:supports_nether_wart",
    "minecraft:supports_wither_rose",
    "minecraft:cannot_support_seagrass",
    "minecraft:support_override_cactus_flower",
    "minecraft:supports_lily_pad",
];

/// The **fluid** tags, a separate registry with an overlapping name.
/// `#minecraft:supports_lily_pad` exists in both and `LilyPadBlock.mayPlaceOn`
/// reads both.
pub const PLACE_FLUID_TAG_ORDER: [&str; 1] = ["minecraft:supports_lily_pad"];

/// An index into [`BlockProps::place_tags`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaceTag {
    SupportsVegetation = 0,
    SupportsCrops = 1,
    CannotSupportSnowLayer = 2,
    SupportOverrideSnowLayer = 3,
    OverridesMushroomLightRequirement = 4,
    WallHangingSigns = 5,
    SupportsDryVegetation = 6,
    SupportsAzalea = 7,
    SupportsNetherSprouts = 8,
    SupportsNetherWart = 9,
    SupportsWitherRose = 10,
    CannotSupportSeagrass = 11,
    SupportOverrideCactusFlower = 12,
    SupportsLilyPad = 13,
}

impl BlockProps {
    /// Is this state in that tag?
    #[must_use]
    pub fn has_place_tag(&self, tag: PlaceTag) -> bool {
        self.place_tags[tag as usize]
    }
}

/// What one block state is, as far as worldgen is concerned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockProps {
    pub air: bool,
    /// The fluid registry name, or `None`. **Not** derivable from the block
    /// name — see the module docs.
    pub fluid: Option<String>,
    /// `BlockState#isSolidRender`, which is what `BlockPredicate.solid`
    /// resolves to.
    pub solid_render: bool,
    /// `BlockStateBase#propagatesSkylightDown()` — sky light passes straight
    /// down through this state undiminished.
    ///
    /// This is the property a daylight detector needs and it is **not**
    /// `!can_occlude` and **not** `!solid_render`: water occludes nothing,
    /// renders as non-solid and propagates no sky light. See `crate::sky`.
    pub propagates_skylight_down: bool,
    /// `BlockStateBase#getLightDampening()` — how much a sky or block light
    /// level loses crossing this state. 0 for air, 15 for a full opaque
    /// block, 1 for water and ice.
    pub light_dampening: i32,
    /// `BlockStateBase#getLightEmission()` — this state's own light level.
    pub light_emission: i32,
    /// Per direction, whether `Block.isShapeFullBlock(getFaceOcclusionShape)`.
    /// Indexed by [`DIRECTIONS`].
    pub full_faces: [bool; 6],
    /// Per direction, `isFaceSturdy(dir, SupportType.FULL)` — what
    /// `has_sturdy_face` asks, and what most `canSurvive` rules reduce to.
    /// Indexed by [`DIRECTIONS`].
    pub sturdy_faces: [bool; 6],
    /// Per direction, `isFaceSturdy(dir, SupportType.CENTER)` — a **different
    /// question** from [`Self::sturdy_faces`] and not derivable from it. A
    /// fence post covers the centre of its own down face and none of the rest
    /// of it: `sturdyFaces` is `000000` and `centerFaces` is `111111`.
    /// `Block.canSupportCenter` asks this one.
    pub center_faces: [bool; 6],
    /// Per direction, `isFaceSturdy(dir, SupportType.RIGID)` — a **third**
    /// question, not derivable from either of the two above.
    /// `SupportType$3.isSupporting` joins the block support shape's face
    /// against a 12x16 column (`Block.column(12.0, 0.0, 16.0)`) under
    /// `ONLY_SECOND` and is true when nothing of that column is left over,
    /// so a face may be FULL and not RIGID and the other way round.
    /// `Block.canSupportRigidBlock` is `isFaceSturdy(UP, RIGID)` and is the
    /// whole of `BaseRailBlock.canSurvive`.
    pub rigid_faces: [bool; 6],
    /// Membership of the six block tags a `canSurvive` rule reads **of the
    /// block it stands on**, in the table's own `placeTagOrder`:
    /// `#supports_vegetation`, `#supports_crops`,
    /// `#cannot_support_snow_layer`, `#support_override_snow_layer`,
    /// `#overrides_mushroom_light_requirement`, `#wall_hanging_signs`.
    /// Indexed by [`PlaceTag`]; the names are in [`PLACE_TAG_ORDER`].
    ///
    /// Resolved by vanilla's own `state.is(tag)` in the dumper rather than by
    /// a tag flattener here, because these nest: `#supports_vegetation` is
    /// two `#` references deep. That is the same decision
    /// [`Self::unstable_bottom_center`] was written under.
    pub place_tags: [bool; 14],
    /// Membership of this state's **fluid** in
    /// `#minecraft:supports_lily_pad`, the fluid tag of that name.
    ///
    /// A separate column because it is a separate registry and the block tag
    /// of the same name is a different set: `LilyPadBlock.mayPlaceOn` reads
    /// the fluid one first (offsets 21-29) and the block one second, and a
    /// port that read only the block tag refuses every lily pad on water.
    pub fluid_supports_lily_pad: bool,
    /// In `#minecraft:unstable_bottom_center`. `Block.canSupportCenter`
    /// refuses a DOWN face outright when this is set, before it looks at any
    /// shape at all.
    pub unstable_bottom_center: bool,
    /// `BlockState#blocksMotion`.
    /// `Block.isFaceFull(state.getCollisionShape(...), dir)` — **not** the
    /// same question as [`Self::sturdy_faces`] and not derivable from it.
    /// Leaves have no sturdy face and a full collision face on all six, which
    /// is why glow lichen grows on them.
    pub collision_full_faces: [bool; 6],
    pub blocks_motion: bool,
    /// `getBlock() instanceof LeavesBlock`, which only
    /// `MOTION_BLOCKING_NO_LEAVES` asks about.
    pub leaves: bool,
    /// `BlockState#canBeReplaced()` — the block's `replaceable` property.
    pub replaceable: bool,
    /// `BlockState#isSolid()`. **Not** [`Self::solid_render`]: leaves are
    /// solid and do not render solid.
    pub solid: bool,
    /// `BlockStateBase.isRedstoneConductor` — a per-block `StatePredicate` on
    /// `BlockBehaviour.Properties`, evaluated per state.
    ///
    /// **Not derivable from any other column, and the table says so
    /// itself.** `minecraft:glass` has `collision_full_faces` `111111` and
    /// this `false`: it is a full collision block that redstone treats as
    /// empty, which is why dust does not climb glass. The default predicate
    /// is a whole-shape `isCollisionShapeFullBlock`, which the per-face
    /// column cannot express, and any block may override it outright —
    /// 2,874 of the 32,366 states are conductors.
    ///
    /// `RedStoneWireBlock.getConnectingSide` reads it at offset 106.
    pub redstone_conductor: bool,
    /// `BlockState#isSignalSource()` — 2,621 states.
    ///
    /// The whole of what `RedStoneWireBlock.shouldConnectTo` needs from a
    /// table. The wire, the repeater and the observer are decided there by
    /// block identity plus their own `facing`, both of which a state string
    /// carries; a comparator has no facing rule there at all and reaches this
    /// column like any other source.
    pub signal_source: bool,
    /// `BlockStateBase.isRandomlyTicking()` — whether
    /// `ServerLevel.tickChunk`'s block pass calls `randomTick` on this state
    /// once it has drawn its position.
    ///
    /// **Per state, and the difference from per-block is not a nicety.** The
    /// field is cached at state construction from `Block.isRandomlyTicking(
    /// state)`, whose default is the block's `Properties.randomTicks` flag —
    /// and twenty-five classes override it per state. `oak_leaves` random
    /// ticks at `distance=7, persistent=false` and at no other of its states;
    /// a crop stops once it is grown. Keyed by block name the table is wrong
    /// in both directions at once.
    pub randomly_ticking: bool,
    /// `state.getFluidState().isRandomlyTicking()`, which `tickChunk` asks
    /// **separately** from [`Self::randomly_ticking`] and immediately after
    /// it, on the same drawn position. So a waterlogged randomly-ticking
    /// block random ticks twice per pick, and a section whose only ticking
    /// content is fluid is still a randomly-ticking section —
    /// `LevelChunkSection.isRandomlyTicking()` is `tickingBlockCount > 0 ||
    /// tickingFluidCount > 0`.
    pub fluid_randomly_ticking: bool,
    /// `BlockState.instrument()` — the `NoteBlockInstrument` this state
    /// gives a note block that rests on it, or that sits under it.
    ///
    /// Spelled as the serialized name, so it is the same string the
    /// `instrument` block property uses and the two can be compared without
    /// a second mapping.
    ///
    /// **This is why it is a table column and not a placement cell.**
    /// `NoteBlock.setInstrument` reads `pos.above()` and then `pos.below()`;
    /// `placement.json` has one cell per clicked face and so records
    /// whichever instrument the golden harness happened to have under the
    /// block. That is a real measured divergence, not a hypothetical: on
    /// grass this server placed `instrument=basedrum` where vanilla places
    /// `instrument=harp`.
    pub instrument: String,
}

/// One row of `NoteBlockInstrument` — the enum, not any block state.
///
/// The three predicates are `type ==`/`!=` tests on one private field in the
/// jar, and all three are dumped because `NoteBlock` asks all three in
/// different places: [`Self::works_above`] picks which neighbour wins in
/// `setInstrument`, [`Self::tunable`] decides whether `triggerEvent` reads
/// NOTE for a pitch or uses `1.0`, and [`Self::custom_sound`] sends it to a
/// block entity for a sound id instead of using [`Self::sound_id`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Instrument {
    /// Serialized name — `harp`, `basedrum`, `custom_head`.
    pub name: String,
    /// `BASE_BLOCK`, `MOB_HEAD` or `CUSTOM`.
    pub kind: String,
    /// `isTunable()`, which is `type == BASE_BLOCK`.
    pub tunable: bool,
    /// `worksAboveNoteBlock()`, which is `type != BASE_BLOCK` — and so is
    /// exactly `!tunable` today. Both are carried because they are asked in
    /// different places for different reasons, and a version that split the
    /// two would otherwise change behaviour silently.
    pub works_above: bool,
    /// `hasCustomSound()`, which is `type == CUSTOM`. The sound then comes
    /// from the skull block entity at play time and [`Self::sound_id`] is
    /// `None`.
    pub custom_sound: bool,
    /// The registry id of `getSoundEvent()`, or `None` for a CUSTOM
    /// instrument, which has none.
    pub sound_id: Option<i32>,
    /// The registry name of `getSoundEvent()`, empty for CUSTOM.
    pub sound: String,
}

impl BlockProps {
    /// The four heightmap predicates, by [`Heightmap`].
    ///
    /// `_WG` and non-`_WG` share a predicate and differ only in *when* they
    /// are primed — see the `OCEAN_FLOOR_WG` note in ORACLE.md 5b.
    #[must_use]
    pub fn matches(&self, heightmap: Heightmap) -> bool {
        match heightmap {
            Heightmap::WorldSurface => !self.air,
            Heightmap::OceanFloor => self.blocks_motion,
            Heightmap::MotionBlocking => self.blocks_motion || self.fluid.is_some(),
            Heightmap::MotionBlockingNoLeaves => {
                (self.blocks_motion || self.fluid.is_some()) && !self.leaves
            }
        }
    }
}

/// The four distinct heightmap predicates. `WORLD_SURFACE_WG` and
/// `WORLD_SURFACE` are the same predicate, as are the two `OCEAN_FLOOR`s.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Heightmap {
    WorldSurface,
    OceanFloor,
    MotionBlocking,
    MotionBlockingNoLeaves,
}

/// The whole table, keyed by **block state**.
///
/// By state — `minecraft:water[level=3]` — and not by block name, because
/// those are different questions and the name has no answer for the first.
/// `minecraft:water` is sixteen states: level 0 has fluid `minecraft:water`
/// and levels 1-15 have `minecraft:flowing_water`. An earlier version of this
/// table was keyed by name and had to carry an "ambiguous" set for exactly
/// that reason; the golden palettes now record full states, so the ambiguity
/// is gone rather than managed.
///
/// A lookup for a name with no properties still works when the block has only
/// one state, since that is how `BlockStateParser` spells it.
#[derive(Clone, Debug, Default)]
pub struct BlockPropsTable {
    by_state: BTreeMap<String, BlockProps>,
    /// Block name to its **default** state's spelling.
    ///
    /// A datapack writes `{"Name": "minecraft:redstone_ore"}` and vanilla
    /// resolves that through `defaultBlockState()` — which carries every
    /// property the block has, so what lands in the world is
    /// `minecraft:redstone_ore[lit=false]`. Without this a port writes the
    /// bare name, the block is right, the state is not, and only a gate that
    /// compares states can tell.
    default_state: BTreeMap<String, String>,
    /// `NoteBlockInstrument`, by serialized name. 27 rows.
    instruments: BTreeMap<String, Instrument>,
}

impl BlockPropsTable {
    /// # Errors
    /// If the file is missing, malformed, or written in a different direction
    /// order than [`DIRECTIONS`].
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let json: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&json)
    }

    /// # Errors
    /// If the document is malformed or its direction order is not
    /// [`DIRECTIONS`].
    pub fn parse(json: &serde_json::Value) -> Result<Self, String> {
        let order: Vec<&str> = json["directionOrder"]
            .as_array()
            .ok_or_else(|| "no `directionOrder`".to_string())?
            .iter()
            .map(|v| v.as_str().unwrap_or(""))
            .collect();
        if order != DIRECTIONS {
            return Err(format!(
                "the table was written in direction order {order:?}, this port reads \
                 {DIRECTIONS:?} — the face columns would be transposed"
            ));
        }
        let tag_order: Vec<&str> = json["placeTagOrder"]
            .as_array()
            .ok_or_else(|| "no `placeTagOrder`".to_string())?
            .iter()
            .map(|v| v.as_str().unwrap_or(""))
            .collect();
        let fluid_tag_order: Vec<&str> = json["placeFluidTagOrder"]
            .as_array()
            .ok_or_else(|| "no `placeFluidTagOrder`".to_string())?
            .iter()
            .map(|v| v.as_str().unwrap_or(""))
            .collect();
        if fluid_tag_order != PLACE_FLUID_TAG_ORDER {
            return Err(format!(
                "the table was written in fluid place-tag order {fluid_tag_order:?}, this port \
                 reads {PLACE_FLUID_TAG_ORDER:?}"
            ));
        }
        if tag_order != PLACE_TAG_ORDER {
            return Err(format!(
                "the table was written in place-tag order {tag_order:?}, this port reads \
                 {PLACE_TAG_ORDER:?} — the tag column would be transposed"
            ));
        }

        let mut by_state: BTreeMap<String, BlockProps> = BTreeMap::new();
        let mut default_state: BTreeMap<String, String> = BTreeMap::new();
        for row in json["table"]
            .as_array()
            .ok_or_else(|| "no `table`".to_string())?
        {
            let state = row["state"]
                .as_str()
                .ok_or_else(|| "a row has no `state`".to_string())?
                .to_string();
            let faces = row["fullFaces"]
                .as_str()
                .ok_or_else(|| format!("{state} has no `fullFaces`"))?;
            let mut full_faces = [false; 6];
            for (i, c) in faces.chars().take(6).enumerate() {
                full_faces[i] = c == '1';
            }
            let sturdy = row["sturdyFaces"]
                .as_str()
                .ok_or_else(|| format!("{state} has no `sturdyFaces`"))?;
            let mut sturdy_faces = [false; 6];
            for (i, c) in sturdy.chars().take(6).enumerate() {
                sturdy_faces[i] = c == '1';
            }
            let collision = row["collisionFullFaces"]
                .as_str()
                .ok_or_else(|| format!("{state} has no `collisionFullFaces`"))?;
            let mut collision_full_faces = [false; 6];
            for (i, c) in collision.chars().take(6).enumerate() {
                collision_full_faces[i] = c == '1';
            }
            let mut center_faces = [false; 6];
            let center = row["centerFaces"]
                .as_str()
                .ok_or_else(|| format!("{state} has no `centerFaces`"))?;
            for (i, c) in center.chars().take(6).enumerate() {
                center_faces[i] = c == '1';
            }
            let mut rigid_faces = [false; 6];
            let rigid = row["rigidFaces"]
                .as_str()
                .ok_or_else(|| format!("{state} has no `rigidFaces`"))?;
            for (i, c) in rigid.chars().take(6).enumerate() {
                rigid_faces[i] = c == '1';
            }
            let mut place_tags = [false; 14];
            let tags = row["placeTags"]
                .as_str()
                .ok_or_else(|| format!("{state} has no `placeTags`"))?;
            for (i, c) in tags.chars().take(14).enumerate() {
                place_tags[i] = c == '1';
            }
            let fluid_supports_lily_pad = row["placeFluidTags"]
                .as_str()
                .ok_or_else(|| format!("{state} has no `placeFluidTags`"))?
                .starts_with('1');
            let fluid = row["fluid"].as_str().unwrap_or("");
            let props = BlockProps {
                air: row["air"].as_bool().unwrap_or(false),
                fluid: if fluid.is_empty() {
                    None
                } else {
                    Some(fluid.to_string())
                },
                solid_render: row["solidRender"].as_bool().unwrap_or(false),
                // A missing light column reads as "opaque and dark", which
                // makes a detector decline rather than read a made-up 15.
                // The generator always writes all three; the defaults are for
                // a table older than 2026-08-18, and declining is the safe
                // direction for both of them.
                propagates_skylight_down: row["propagatesSkylightDown"].as_bool().unwrap_or(false),
                light_dampening: row["lightDampening"].as_i64().unwrap_or(15) as i32,
                light_emission: row["lightEmission"].as_i64().unwrap_or(0) as i32,
                full_faces,
                sturdy_faces,
                center_faces,
                rigid_faces,
                place_tags,
                fluid_supports_lily_pad,
                unstable_bottom_center: row["unstableBottomCenter"].as_bool().unwrap_or(false),
                collision_full_faces,
                blocks_motion: row["blocksMotion"].as_bool().unwrap_or(false),
                leaves: row["leaves"].as_bool().unwrap_or(false),
                replaceable: row["replaceable"].as_bool().unwrap_or(false),
                solid: row["solid"].as_bool().unwrap_or(false),
                redstone_conductor: row["redstoneConductor"].as_bool().unwrap_or(false),
                signal_source: row["signalSource"].as_bool().unwrap_or(false),
                // These two are `ok_or_else` where every column above is
                // `unwrap_or(false)`, and the asymmetry is deliberate.
                //
                // A missing `solidRender` reads as false and something
                // downstream behaves oddly in a way a gate can see. A missing
                // `randomlyTicking` reads as false for **every state at
                // once**, which does not look like a defect at all: the
                // random tick loop draws its positions, finds nothing opted
                // in, and produces silence — the exact output of a correct
                // implementation of a world where nothing happens to be
                // growing. There is no gate that can tell those apart, so the
                // stale table has to refuse instead.
                randomly_ticking: row["randomlyTicking"].as_bool().ok_or_else(|| {
                    format!(
                        "{state} has no `randomlyTicking` — this block_props.json predates the \
                         column and would silently make every state non-random-ticking. \
                         Regenerate with ci/gen-gameplay-tables.sh."
                    )
                })?,
                fluid_randomly_ticking: row["fluidRandomlyTicking"].as_bool().ok_or_else(|| {
                    format!(
                        "{state} has no `fluidRandomlyTicking` — regenerate block_props.json \
                         with ci/gen-gameplay-tables.sh."
                    )
                })?,
                // Refused rather than defaulted, for the reason the two
                // columns above are. `harp` is the commonest answer and the
                // one a default would pick, so a table predating this column
                // would place every note block on `harp` — which is *right*
                // on grass, right on 8,132 of 32,366 states, and wrong on
                // the other 24,234 without ever failing to produce a state.
                instrument: row["instrument"]
                    .as_str()
                    .ok_or_else(|| {
                        format!(
                            "{state} has no `instrument` — this block_props.json predates the \
                             column and would silently make every note block a harp. \
                             Regenerate with ci/gen-gameplay-tables.sh."
                        )
                    })?
                    .to_string(),
            };
            if row["default"].as_bool().unwrap_or(false) {
                let block = row["block"]
                    .as_str()
                    .ok_or_else(|| format!("{state} has no `block`"))?;
                if let Some(other) = default_state.insert(block.to_string(), state.clone()) {
                    return Err(format!(
                        "{block} has two default states, {other} and {state}"
                    ));
                }
            }
            by_state.insert(state, props);
        }
        // The enum. Refused when absent for the same reason the column is:
        // an empty map makes every `setInstrument` fall through to its
        // default and produces a plausible answer.
        let rows = json["instruments"].as_array().ok_or_else(|| {
            "no `instruments` — this block_props.json predates the note block enum. \
             Regenerate with ci/gen-gameplay-tables.sh."
                .to_string()
        })?;
        let mut instruments = BTreeMap::new();
        for row in rows {
            let name = row["name"]
                .as_str()
                .ok_or_else(|| "an `instruments` row has no `name`".to_string())?;
            let kind = row["type"]
                .as_str()
                .ok_or_else(|| format!("instrument {name} has no `type`"))?;
            let tunable = row["tunable"]
                .as_bool()
                .ok_or_else(|| format!("instrument {name} has no `tunable`"))?;
            let works_above = row["worksAbove"]
                .as_bool()
                .ok_or_else(|| format!("instrument {name} has no `worksAbove`"))?;
            let custom_sound = row["customSound"]
                .as_bool()
                .ok_or_else(|| format!("instrument {name} has no `customSound`"))?;
            // `-1` is the dumper's spelling of "this instrument has no sound
            // event of its own", which is true of exactly the CUSTOM type.
            // Reading it as a real registry id would send sound -1 down the
            // wire, so the two are cross-checked rather than trusted apart.
            let raw = row["soundId"]
                .as_i64()
                .ok_or_else(|| format!("instrument {name} has no `soundId`"))?;
            let sound_id = i32::try_from(raw).ok().filter(|id| *id >= 0);
            if custom_sound != sound_id.is_none() {
                return Err(format!(
                    "instrument {name} says customSound={custom_sound} and soundId={raw}; \
                     a CUSTOM instrument has no sound event and every other kind has one"
                ));
            }
            if tunable == works_above {
                return Err(format!(
                    "instrument {name} says tunable={tunable} and worksAbove={works_above}; \
                     both are tests on `type` against BASE_BLOCK and must disagree"
                ));
            }
            instruments.insert(
                name.to_string(),
                Instrument {
                    name: name.to_string(),
                    kind: kind.to_string(),
                    tunable,
                    works_above,
                    custom_sound,
                    sound_id,
                    sound: row["sound"].as_str().unwrap_or_default().to_string(),
                },
            );
        }
        // Every instrument a state names must be in the enum, or a lookup
        // would fall back and the fallback would be `harp`.
        for (state, props) in &by_state {
            if !instruments.contains_key(&props.instrument) {
                return Err(format!(
                    "{state} names instrument `{}`, which is not in `instruments`",
                    props.instrument
                ));
            }
        }
        Ok(Self {
            by_state,
            default_state,
            instruments,
        })
    }

    /// One `NoteBlockInstrument` by its serialized name.
    #[must_use]
    pub fn instrument(&self, name: &str) -> Option<&Instrument> {
        self.instruments.get(name)
    }

    /// The instrument a block state gives — `BlockState.instrument()`.
    ///
    /// `None` means the table has never heard of the state, which for a
    /// state out of a real chunk means the extract is stale.
    #[must_use]
    pub fn instrument_of(&self, state: &str) -> Option<&Instrument> {
        let props = self.by_state.get(state)?;
        self.instruments.get(&props.instrument)
    }

    /// Every instrument, in serialized-name order.
    pub fn instruments(&self) -> impl Iterator<Item = &Instrument> {
        self.instruments.values()
    }

    /// `NoteBlock.setInstrument`, from the jar.
    ///
    /// ```text
    ///   0  above = level.getBlockState(pos.above()).instrument()
    ///  17  if (above.worksAboveNoteBlock()) return setValue(INSTRUMENT, above)
    ///  36  below = level.getBlockState(pos.below()).instrument()
    ///  51  chosen = below.worksAboveNoteBlock() ? HARP : below
    ///  69  return setValue(INSTRUMENT, chosen)
    /// ```
    ///
    /// Two things in that are easy to get backwards and both are measured
    /// consequences rather than style:
    ///
    /// * **`above` is read first**, so a mob head on top of a note block
    ///   beats whatever the block is standing on. A port that reads only
    ///   `below` is right until somebody stacks a skull.
    /// * **the `below` branch inverts.** If the block *below* is one that
    ///   works above — a head — the answer is `harp`, not that head's
    ///   instrument. So a skull under a note block silences the skull rather
    ///   than sounding it.
    ///
    /// `None` when either neighbour's state is not in the table.
    #[must_use]
    pub fn note_block_instrument(&self, above: &str, below: &str) -> Option<&Instrument> {
        let above = self.instrument_of(above)?;
        if above.works_above {
            return Some(above);
        }
        let below = self.instrument_of(below)?;
        if below.works_above {
            return self.instruments.get("harp");
        }
        Some(below)
    }

    /// The facts for a block state, spelled as `BlockStateParser` spells it.
    ///
    /// `None` means the table has never heard of it — which for a state that
    /// came out of a real chunk means the extract is stale, not that the
    /// answer is no.
    #[must_use]
    pub fn get(&self, state: &str) -> Option<&BlockProps> {
        self.by_state.get(state)
    }

    /// The spelling of a block's default state — `minecraft:redstone_ore`
    /// answers `minecraft:redstone_ore[lit=false]`.
    ///
    /// `None` means the table has never heard of the block.
    #[must_use]
    pub fn default_state(&self, block: &str) -> Option<&str> {
        self.default_state.get(block).map(String::as_str)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.by_state.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_state.is_empty()
    }

    /// Every state and its facts, in state-name order.
    ///
    /// For asking a question **of the whole table** rather than of one state
    /// — how many states a column separates, which is how a column that has
    /// silently become a constant is caught. `get` cannot answer that, and a
    /// column nobody counts is one nobody would notice going flat.
    pub fn states(&self) -> impl Iterator<Item = (&str, &BlockProps)> {
        self.by_state.iter().map(|(k, v)| (k.as_str(), v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXTRACT: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/vanilla/block_props.json"
    );

    fn table() -> Option<BlockPropsTable> {
        let path = Path::new(EXTRACT);
        // `fixture::available`, not a bare `exists()`: the early return this
        // replaces made every test below **pass** when the extract was absent,
        // and cargo never prints a passing test's stderr, so the line it wrote
        // reached no log and no grep. In the default `required` mode this
        // panics instead. See crates/rustmc-data/src/fixture.rs.
        if !rustmc_data::fixture::available(path, "block_props extract", "ci/block-props-golden.sh")
        {
            return None;
        }
        // **Named, not `expect("loads")`.** That form printed
        // `loads: "no `placeTagOrder`"` nine times over, which says what is
        // missing and nothing about where or why — and the why was never a
        // branch defect: `ci/t0-remote.sh` rsyncs with `--exclude
        // data/vanilla`, so a table regenerated in the working tree does not
        // reach the gate directory and the gate reads whatever `cp -rn`
        // seeded it with. Nine tests then red on a branch whose only
        // relation to them is adding a column.
        //
        // A skip is not the alternative: the early return this whole
        // function replaces made every test below pass when the extract was
        // absent. So it panics, and it panics with the path and the command.
        Some(BlockPropsTable::load(path).unwrap_or_else(|e| {
            panic!(
                "{}: {e}\n\n\
                 This table is not this tree's. Regenerate it WHERE THE TEST RUNS:\n\
                     ./ci/block-props-golden.sh          (or ./ci/gen-gameplay-tables.sh)\n\n\
                 `ci/t0-remote.sh` rsyncs with `--exclude data/vanilla`, so generating it \n\
                 in the working tree does NOT put it in the gate directory. Run \n\
                 `ci/gen-gameplay-tables.sh` inside $RUSTMC_REMOTE_DIR on the box, which is \n\
                 what `ci/handoff.sh` does between its rsync and its T0.",
                path.display()
            )
        }))
    }

    /// The fact the whole table exists for.
    #[test]
    fn a_waterlogged_block_carries_water_though_its_name_does_not() {
        let Some(table) = table() else { return };
        assert_eq!(
            table
                .get("minecraft:seagrass")
                .expect("seagrass")
                .fluid
                .as_deref(),
            Some("minecraft:water"),
            "seagrass is not water and its fluid state is — this is why \
             matching_fluids cannot be answered from a block name"
        );
        assert_eq!(table.get("minecraft:stone").expect("stone").fluid, None);

        // The reason the key is a state and not a name: one block, two
        // fluids, decided by a property.
        assert_eq!(
            table
                .get("minecraft:water[level=0]")
                .expect("still")
                .fluid
                .as_deref(),
            Some("minecraft:water")
        );
        assert_eq!(
            table
                .get("minecraft:water[level=3]")
                .expect("flowing")
                .fluid
                .as_deref(),
            Some("minecraft:flowing_water")
        );
    }

    #[test]
    fn air_is_air_and_stone_is_solid_on_every_face() {
        let Some(table) = table() else { return };
        assert!(table.get("minecraft:air").expect("air").air);
        let stone = table.get("minecraft:stone").expect("stone");
        assert!(!stone.air);
        assert!(stone.solid_render);
        assert_eq!(stone.full_faces, [true; 6]);
        assert_eq!(
            table
                .get("minecraft:water[level=0]")
                .expect("water")
                .full_faces,
            [false; 6]
        );
    }

    /// A transposed face table would be invisible in any single lookup, so
    /// the loader refuses a file whose direction order is not the one it
    /// reads.
    #[test]
    fn a_table_in_a_different_direction_order_is_refused() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{"directionOrder":["UP","DOWN","NORTH","SOUTH","WEST","EAST"],"table":[]}"#,
        )
        .expect("json");
        let error = BlockPropsTable::parse(&json).expect_err("should refuse");
        assert!(error.contains("transposed"), "{error}");
    }

    /// The same argument for the tag columns, and it needs its own vector:
    /// the direction check passes on a file whose tag order is wrong, so one
    /// test cannot cover both. A transposed tag column would make
    /// `#supports_crops` answer `#supports_vegetation`'s question and every
    /// crop placeable on grass.
    #[test]
    fn a_table_in_a_different_place_tag_order_is_refused() {
        let mut order: Vec<&str> = PLACE_TAG_ORDER.to_vec();
        order.swap(0, 1);
        let json: serde_json::Value = serde_json::json!({
            "directionOrder": DIRECTIONS,
            "placeTagOrder": order,
            "placeFluidTagOrder": PLACE_FLUID_TAG_ORDER,
            "table": [],
        });
        let error = BlockPropsTable::parse(&json).expect_err("should refuse");
        assert!(error.contains("place-tag order"), "{error}");

        // And the fluid one, which is a different registry with an
        // overlapping name and so a different way to be wrong.
        let json: serde_json::Value = serde_json::json!({
            "directionOrder": DIRECTIONS,
            "placeTagOrder": PLACE_TAG_ORDER,
            "placeFluidTagOrder": ["minecraft:water"],
            "table": [],
        });
        let error = BlockPropsTable::parse(&json).expect_err("should refuse");
        assert!(error.contains("fluid place-tag order"), "{error}");
    }

    /// **The reconstruction of the defect this column shipped with.**
    ///
    /// `unstableBottomCenter` read `0` for all 32,366 states for as long as
    /// it existed, because the dumper never bound block tags and an unbound
    /// tag answers `false` rather than throwing. Nothing on this side could
    /// have noticed: every consumer asked the column about one state and got
    /// a plausible `false`.
    ///
    /// So the guard is a **population** assertion, which is the only shape
    /// that can see it. The numbers are vanilla's own tag sizes and are
    /// exact rather than a floor: a tag that gains or loses a member at the
    /// next version moves this test, which is the right outcome for a column
    /// whose whole failure mode is being quietly empty.
    #[test]
    fn the_tag_columns_are_not_silently_empty() {
        let Some(table) = table() else { return };
        let mut counts = [0usize; PLACE_TAG_ORDER.len()];
        let mut unstable = 0usize;
        let mut lily = 0usize;
        for (_state, props) in table.states() {
            for (i, count) in counts.iter_mut().enumerate() {
                if props.place_tags[i] {
                    *count += 1;
                }
            }
            if props.unstable_bottom_center {
                unstable += 1;
            }
            if props.fluid_supports_lily_pad {
                lily += 1;
            }
        }
        assert_eq!(
            unstable, 384,
            "#unstable_bottom_center — the column that was 0"
        );
        assert_eq!(lily, 11_761, "the FLUID #supports_lily_pad");
        assert_eq!(
            counts.to_vec(),
            vec![23, 8, 4, 3, 6, 96, 46, 24, 26, 1, 26, 1, 24, 5],
            "in PLACE_TAG_ORDER: {PLACE_TAG_ORDER:?}"
        );
        // Not one of them may be zero, said separately from the exact
        // figures above so that the reason survives a version bump that
        // legitimately moves one of them.
        for (i, count) in counts.iter().enumerate() {
            assert!(
                *count > 0,
                "{} resolved to no states — the tags did not bind",
                PLACE_TAG_ORDER[i]
            );
        }
    }

    /// A bottom slab is sturdy on exactly one face — the case that shows the
    /// table is per *state* and not per block.
    #[test]
    fn a_bottom_slab_is_sturdy_downwards_only() {
        let Some(table) = table() else { return };
        let slab = table
            .get("minecraft:oak_slab[type=bottom,waterlogged=false]")
            .expect("bottom slab");
        // DOWN, UP, NORTH, SOUTH, WEST, EAST
        assert_eq!(slab.sturdy_faces, [true, false, false, false, false, false]);
        let top = table
            .get("minecraft:oak_slab[type=top,waterlogged=false]")
            .expect("top slab");
        assert_eq!(top.sturdy_faces, [false, true, false, false, false, false]);
        assert_eq!(
            table.get("minecraft:stone").expect("stone").sturdy_faces,
            [true; 6]
        );
    }

    /// The four heightmap predicates, on blocks whose answers differ.
    #[test]
    fn the_heightmap_predicates_separate_the_cases_they_exist_to_separate() {
        let Some(table) = table() else { return };
        let water = table.get("minecraft:water[level=0]").expect("water");
        assert!(
            !water.matches(Heightmap::OceanFloor),
            "water does not block motion"
        );
        assert!(
            water.matches(Heightmap::MotionBlocking),
            "but it is a fluid"
        );
        assert!(water.matches(Heightmap::WorldSurface), "and it is not air");

        let leaves = table
            .get("minecraft:oak_leaves[distance=1,persistent=false,waterlogged=false]")
            .expect("leaves");
        assert!(leaves.matches(Heightmap::MotionBlocking));
        assert!(
            !leaves.matches(Heightmap::MotionBlockingNoLeaves),
            "the one case that map exists for"
        );

        let air = table.get("minecraft:air").expect("air");
        for map in [
            Heightmap::WorldSurface,
            Heightmap::OceanFloor,
            Heightmap::MotionBlocking,
            Heightmap::MotionBlockingNoLeaves,
        ] {
            assert!(!air.matches(map), "air matches nothing");
        }
    }

    /// Every state in the game is in the table, keyed exactly once.
    #[test]
    fn the_table_covers_every_state() {
        let Some(table) = table() else { return };
        println!("BLOCK PROPS: {} states keyed by state", table.len());
        assert_eq!(
            table.len(),
            32_366,
            "the table should hold one row per block state in 26.2"
        );
    }

    /// Random ticking is decided **per state**, and leaves are the cheapest
    /// proof: one block, four states named here, two answers.
    ///
    /// `LeavesBlock.isRandomlyTicking(state)` is `distance == 7 &&
    /// !persistent` and nothing else. A table keyed by block name has to
    /// answer one way for all 28 oak-leaf states and is therefore wrong for
    /// 27 of them whichever way it answers.
    #[test]
    fn leaves_random_tick_only_where_they_decay() {
        let Some(table) = table() else { return };
        let ticks = |state: &str| {
            table
                .get(state)
                .unwrap_or_else(|| panic!("{state} missing from the table"))
                .randomly_ticking
        };
        assert!(
            ticks("minecraft:oak_leaves[distance=7,persistent=false,waterlogged=false]"),
            "distance 7 and not persistent is the decaying state"
        );
        assert!(
            !ticks("minecraft:oak_leaves[distance=1,persistent=false,waterlogged=false]"),
            "a leaf near its log does not random tick"
        );
        assert!(
            !ticks("minecraft:oak_leaves[distance=7,persistent=true,waterlogged=false]"),
            "a placed leaf is persistent and does not random tick however far it is"
        );
        assert!(!ticks("minecraft:stone"), "stone never random ticks");
    }

    /// The fluid column is a **separate question** asked on the same drawn
    /// position, and lava is where the two come apart from water.
    ///
    /// `Fluid.isRandomlyTicking()` is `false`; `LavaFluid` overrides it to
    /// `true` and `WaterFluid` does not override it at all. And
    /// `LiquidBlock.isRandomlyTicking(state)` forwards to the fluid — so a
    /// lava block answers **true on both columns** and `tickChunk` calls
    /// `randomTick` twice for one pick that lands on it, once through the
    /// block and once through the fluid.
    #[test]
    fn lava_random_ticks_on_both_columns_and_water_on_neither() {
        let Some(table) = table() else { return };
        let lava = table.get("minecraft:lava[level=0]").expect("lava");
        assert!(lava.randomly_ticking, "LiquidBlock forwards to the fluid");
        assert!(lava.fluid_randomly_ticking, "LavaFluid overrides to true");

        let water = table.get("minecraft:water[level=0]").expect("water");
        assert!(!water.randomly_ticking);
        assert!(
            !water.fluid_randomly_ticking,
            "WaterFluid does not override Fluid.isRandomlyTicking"
        );
    }

    /// The column is not vacuously false, and it is not vacuously true.
    ///
    /// A stale extract read through a `unwrap_or(false)` loader would make
    /// every state non-random-ticking, which produces silence — the same
    /// output as a correct server on a world where nothing is growing. The
    /// loader refuses a row without the column for exactly that reason; this
    /// is the other half, and it fails if the column is ever wired to a
    /// constant.
    #[test]
    fn the_random_tick_column_separates_the_table() {
        let Some(table) = table() else { return };
        let ticking = table.states().filter(|(_, p)| p.randomly_ticking).count();
        let fluid = table
            .states()
            .filter(|(_, p)| p.fluid_randomly_ticking)
            .count();
        println!("BLOCK PROPS: {ticking} randomly-ticking states, {fluid} randomly-ticking fluids");
        assert!(
            ticking > 0 && ticking < table.len(),
            "{ticking} of {} — a column that is all one value is a constant",
            table.len()
        );
        assert!(fluid > 0 && fluid < table.len(), "{fluid} fluid states");
    }

    /// The twelve states this column was verified against by hand, from the
    /// jar. Twelve rather than one because the column is a whole
    /// `BlockBehaviour.Properties` field and a single row could agree by
    /// accident with a port that returned a constant.
    #[test]
    fn twelve_named_states_carry_the_instrument_the_jar_gives_them() {
        let Some(table) = table() else { return };
        for (state, want) in [
            ("minecraft:grass_block[snowy=false]", "harp"),
            ("minecraft:stone", "basedrum"),
            ("minecraft:air", "harp"),
            ("minecraft:sand", "snare"),
            ("minecraft:glass", "hat"),
            ("minecraft:oak_planks", "bass"),
            ("minecraft:gold_block", "bell"),
            ("minecraft:packed_ice", "chime"),
            ("minecraft:clay", "flute"),
            ("minecraft:pumpkin", "didgeridoo"),
            ("minecraft:emerald_block", "bit"),
            ("minecraft:hay_block[axis=y]", "banjo"),
        ] {
            assert_eq!(
                table.instrument_of(state).map(|i| i.name.as_str()),
                Some(want),
                "{state}"
            );
        }
    }

    /// The distribution, so that a regenerated table that quietly lost the
    /// column cannot pass by making everything `harp`.
    #[test]
    fn the_instrument_column_is_not_one_answer_repeated() {
        let Some(table) = table() else { return };
        let harp = table
            .states()
            .filter(|(_, p)| p.instrument == "harp")
            .count();
        assert_eq!(table.instruments().count(), 27, "NoteBlockInstrument");
        assert_eq!(harp, 8_132, "harp states");
        assert_eq!(table.len() - harp, 24_234, "non-harp states");
    }

    /// `setInstrument`'s two branches, both of which are easy to get
    /// backwards. Neither is exercised by the corpus, which only ever places
    /// a note block on flat ground with air above it.
    #[test]
    fn set_instrument_reads_above_first_and_inverts_the_below_branch() {
        let Some(table) = table() else { return };
        let name = |i: Option<&Instrument>| i.map(|i| i.name.clone()).unwrap_or_default();

        // Plain ground, air above: the below branch, uninverted.
        assert_eq!(
            name(table.note_block_instrument("minecraft:air", "minecraft:stone")),
            "basedrum"
        );
        // This is the corpus's own case, and the one this server got wrong.
        assert_eq!(
            name(
                table.note_block_instrument("minecraft:air", "minecraft:grass_block[snowy=false]")
            ),
            "harp"
        );

        // A head ABOVE wins outright, whatever the ground is.
        let head = "minecraft:zombie_head[powered=false,rotation=0]";
        assert!(
            table.instrument_of(head).expect("zombie head").works_above,
            "a mob head is the whole reason `above` is read first"
        );
        assert_eq!(
            name(table.note_block_instrument(head, "minecraft:stone")),
            "zombie",
            "a head on top beats the block underneath"
        );

        // A head BELOW inverts to harp — it does NOT sound the head.
        assert_eq!(
            name(table.note_block_instrument("minecraft:air", head)),
            "harp",
            "the below branch returns HARP for anything that works above"
        );
    }

    /// The enum's own invariants, which the parser also enforces — asserted
    /// here too so that a parser that stopped checking is visible.
    #[test]
    fn the_three_instrument_predicates_are_one_field() {
        let Some(table) = table() else { return };
        let mut custom = 0;
        for instrument in table.instruments() {
            assert_ne!(
                instrument.tunable, instrument.works_above,
                "{}: both are tests on `type` against BASE_BLOCK",
                instrument.name
            );
            assert_eq!(
                instrument.custom_sound,
                instrument.sound_id.is_none(),
                "{}: only a CUSTOM instrument lacks a sound event",
                instrument.name
            );
            match instrument.kind.as_str() {
                "BASE_BLOCK" => assert!(instrument.tunable),
                "MOB_HEAD" => assert!(instrument.works_above && !instrument.custom_sound),
                "CUSTOM" => custom += 1,
                other => panic!("unknown instrument type {other}"),
            }
        }
        assert_eq!(custom, 1, "custom_head is the only CUSTOM instrument");
        assert_eq!(
            table.instrument("harp").and_then(|i| i.sound_id),
            Some(1_167),
            "the sound id the vanilla recording of scenario 430 carries"
        );
    }
}
