//! Ruined portals — `RuinedPortalStructure` and `RuinedPortalPiece`.
//!
//! Reference (26.2 bytecode, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `net.minecraft.world.level.levelgen.structure.structures.
//! RuinedPortal{Structure,Piece}`, `RuinedPortalPiece$VerticalPlacement`,
//! `RuinedPortalStructure$Setup`, `Mth.randomBetweenInclusive`,
//! `Util.getRandom`.
//!
//! # Why this structure is gateable without generating anything
//!
//! Vanilla saves a ruined portal's *entire* random outcome into the chunk's
//! `Children[]`, and this port was written against that rather than against
//! blocks. One saved piece carries `Template`, `Rotation`, `Mirror`,
//! `VerticalPlacement`, `TPX/TPY/TPZ` and a `Properties` compound holding all
//! six derived properties. Every one of those is a product of the structure's
//! own `WorldgenRandom`, so a single NBT read pins the whole draw sequence —
//! the same shape of gate that closed `end_city` off its `GD` field. No JVM,
//! no generated world, milliseconds.
//!
//! [`tests`] gates four distinct portals mined out of the references already
//! on the box. **What that corpus does not reach is written down in
//! [`tests::the_corpus_does_not_reach_these_branches`]** rather than left for
//! a later reader to assume: no `partly_buried`, no `in_mountain`, and no
//! giant-portal template. A gate a corpus cannot exercise proves nothing, so
//! those branches are ported from bytecode and explicitly declared
//! unmeasured. `in_nether` was on that list until 2026-10-02, when the nether
//! began placing its portals: the three the r=16 nether references saved are
//! gated position and all in `worldgen-assembly`'s
//! `golden_nether_ruined_portal`, one of them at y 28 — the `27..=29` range
//! only a portal without an air pocket reaches.
//!
//! # The draw order, and the two draws that are not taken
//!
//! `findGenerationPoint`, in order:
//!
//! 1. **the setup**, and only when `setups.size() > 1`. A one-setup structure
//!    — every ruined portal but the standard overworld one — spends
//!    **nothing** here. `ruined_portal.json` has two, so it does.
//! 2. **the air pocket**, via [`sample`] — see below, it is usually free.
//! 3. `nextFloat() < 0.05` — giant portals or normal ones. **Always drawn**,
//!    in both branches.
//! 4. `nextInt(len)` — the template out of the chosen array.
//! 5. `nextInt(4)` — the rotation, `Util.getRandom(Rotation.values(), r)`.
//! 6. `nextFloat() < 0.5` — `Mirror.NONE` else `Mirror.FRONT_BACK`. Note the
//!    sense: **below** the threshold is `NONE`.
//! 7. whatever [`find_suitable_y`] draws, which depends on the placement.
//!
//! **[`sample`] short-circuits at 0.0 and 1.0 and draws nothing**, and that is
//! the trap this structure sets. `ruined_portal.json`'s `underground` setup
//! has `air_pocket_probability: 1.0` and its `on_land_surface` setup has
//! `0.5`; `ruined_portal_ocean` has `0.0`. So the overworld portal in the
//! measured window takes **no** draw for its air pocket while a portal 44
//! chunks away takes one, and a port that always draws lands every later
//! value one place late for the first and is right for the second. That is
//! exactly the failure that looks like "the template array is wrong".
//!
//! # `getRandomWithinInterval` returns the *max* when the interval is empty
//!
//! `if (min < max) return randomBetweenInclusive(r, min, max); return max;` —
//! the degenerate case yields `max`, not `min`, and takes no draw. An
//! underground portal under a low ceiling therefore spends nothing on its y.

use crate::rng::RandomSource;
use crate::structure::template::{Mirror, Rotation};

/// `RuinedPortalStructure.STRUCTURE_LOCATION_PORTALS`, in declaration order.
///
/// The order is behavior: the template is `array[random.nextInt(length)]`, so
/// `portal_3` sitting at index 2 is a load-bearing fact.
pub const PORTALS: [&str; 10] = [
    "ruined_portal/portal_1",
    "ruined_portal/portal_2",
    "ruined_portal/portal_3",
    "ruined_portal/portal_4",
    "ruined_portal/portal_5",
    "ruined_portal/portal_6",
    "ruined_portal/portal_7",
    "ruined_portal/portal_8",
    "ruined_portal/portal_9",
    "ruined_portal/portal_10",
];

/// `RuinedPortalStructure.STRUCTURE_LOCATION_GIANT_PORTALS`.
///
/// Three entries against ten, which is what makes the 5% branch observable in
/// a template name rather than only in a draw count.
pub const GIANT_PORTALS: [&str; 3] = [
    "ruined_portal/giant_portal_1",
    "ruined_portal/giant_portal_2",
    "ruined_portal/giant_portal_3",
];

/// `RuinedPortalStructure.PROBABILITY_OF_GIANT_PORTAL`.
pub const PROBABILITY_OF_GIANT_PORTAL: f32 = 0.05;

/// `RuinedPortalStructure.MIN_Y_INDEX` — added to the level's minimum y to
/// give the floor `findSuitableY` will not search below.
pub const MIN_Y_INDEX: i32 = 15;

/// `RuinedPortalPiece$VerticalPlacement`, **in ordinal order**.
///
/// The ordinal never reaches a draw, but the serialized names are what
/// vanilla writes into the saved piece and therefore what the oracle compares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerticalPlacement {
    OnLandSurface,
    PartlyBuried,
    OnOceanFloor,
    InMountain,
    Underground,
    InNether,
}

impl VerticalPlacement {
    /// `getSerializedName`.
    #[must_use]
    pub const fn serialized_name(self) -> &'static str {
        match self {
            Self::OnLandSurface => "on_land_surface",
            Self::PartlyBuried => "partly_buried",
            Self::OnOceanFloor => "on_ocean_floor",
            Self::InMountain => "in_mountain",
            Self::Underground => "underground",
            Self::InNether => "in_nether",
        }
    }

    /// Parse a `placement` field.
    ///
    /// # Errors
    /// If the name is not one of the six.
    pub fn parse(name: &str) -> Result<Self, String> {
        match name {
            "on_land_surface" => Ok(Self::OnLandSurface),
            "partly_buried" => Ok(Self::PartlyBuried),
            "on_ocean_floor" => Ok(Self::OnOceanFloor),
            "in_mountain" => Ok(Self::InMountain),
            "underground" => Ok(Self::Underground),
            "in_nether" => Ok(Self::InNether),
            other => Err(format!("`{other}` is not a VerticalPlacement")),
        }
    }

    /// `RuinedPortalPiece.getHeightMapType` — `OCEAN_FLOOR_WG` for an ocean
    /// floor portal and `WORLD_SURFACE_WG` for every other placement.
    ///
    /// Returned as the pair of booleans the caller needs rather than as a
    /// heightmap enum this crate does not otherwise carry here: `true` means
    /// ocean floor.
    #[must_use]
    pub const fn uses_ocean_floor_heightmap(self) -> bool {
        matches!(self, Self::OnOceanFloor)
    }
}

/// One entry of a ruined portal structure's `setups` list.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Setup {
    pub placement: VerticalPlacement,
    pub air_pocket_probability: f32,
    pub mossiness: f32,
    pub overgrown: bool,
    pub vines: bool,
    pub can_be_cold: bool,
    pub replace_with_blackstone: bool,
    pub weight: f32,
}

impl Setup {
    /// Parse one element of `worldgen/structure/ruined_portal*.json`'s
    /// `setups`.
    ///
    /// # Errors
    /// If a required field is missing or the wrong type. Nothing defaults:
    /// every one of vanilla's seven ruined-portal files writes all eight
    /// fields, and a silent default here would be a draw-count change hiding
    /// as a data change.
    pub fn parse(value: &serde_json::Value) -> Result<Self, String> {
        let f32_field = |name: &str| -> Result<f32, String> {
            value
                .get(name)
                .and_then(serde_json::Value::as_f64)
                .map(|v| v as f32)
                .ok_or_else(|| format!("setup has no float `{name}`"))
        };
        let bool_field = |name: &str| -> Result<bool, String> {
            value
                .get(name)
                .and_then(serde_json::Value::as_bool)
                .ok_or_else(|| format!("setup has no bool `{name}`"))
        };
        let placement = value
            .get("placement")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "setup has no `placement`".to_string())?;
        Ok(Self {
            placement: VerticalPlacement::parse(placement)?,
            air_pocket_probability: f32_field("air_pocket_probability")?,
            mossiness: f32_field("mossiness")?,
            overgrown: bool_field("overgrown")?,
            vines: bool_field("vines")?,
            can_be_cold: bool_field("can_be_cold")?,
            replace_with_blackstone: bool_field("replace_with_blackstone")?,
            weight: f32_field("weight")?,
        })
    }
}

/// `RuinedPortalPiece$Properties` — what vanilla saves under `Properties`.
///
/// Five of the six come straight off the chosen [`Setup`]; only `cold` and
/// `air_pocket` are computed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Properties {
    pub cold: bool,
    pub mossiness: f32,
    pub air_pocket: bool,
    pub overgrown: bool,
    pub vines: bool,
    pub replace_with_blackstone: bool,
}

/// Everything `findGenerationPoint` decides before it needs a height.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    /// Which `setups` entry won, for the oracle to report against.
    pub setup_index: usize,
    pub placement: VerticalPlacement,
    pub air_pocket: bool,
    pub giant: bool,
    pub template: String,
    pub rotation: Rotation,
    pub mirror: Mirror,
}

/// `RuinedPortalStructure.sample` — **and it does not always draw.**
///
/// `if (p == 0.0f) return false; if (p == 1.0f) return true;
/// return random.nextFloat() < p;`
///
/// The two short circuits are exact float comparisons against the literals, so
/// they fire on the values vanilla's own data files carry and on nothing else.
pub fn sample<R: RandomSource + ?Sized>(random: &mut R, probability: f32) -> bool {
    if probability == 0.0 {
        return false;
    }
    if probability == 1.0 {
        return true;
    }
    random.next_f32() < probability
}

/// `RuinedPortalStructure.getRandomWithinInterval` — `max` when empty.
pub fn get_random_within_interval<R: RandomSource + ?Sized>(
    random: &mut R,
    min: i32,
    max: i32,
) -> i32 {
    if min < max {
        random.next_i32_between_inclusive(min, max)
    } else {
        max
    }
}

/// The weighted `setups` pick, and the six draws after it.
///
/// `total` is summed in float in declaration order and each candidate
/// subtracts `weight / total`, so the arithmetic is `f32` throughout to match
/// Java's `float`. Summing in `f64` and rounding at the end is a different
/// function and would pick a different setup at a tie.
pub fn plan<R: RandomSource + ?Sized>(random: &mut R, setups: &[Setup]) -> Plan {
    assert!(!setups.is_empty(), "a ruined portal with no setups");

    let setup_index = if setups.len() > 1 {
        let total: f32 = setups.iter().fold(0.0f32, |acc, s| acc + s.weight);
        let mut f = random.next_f32();
        let mut chosen = None;
        for (i, s) in setups.iter().enumerate() {
            f -= s.weight / total;
            if f < 0.0 {
                chosen = Some(i);
                break;
            }
        }
        // `if (setup == null) throw new IllegalStateException()`. Float
        // subtraction can leave `f` at exactly 0.0 after the last candidate,
        // so vanilla really can reach the throw; matching it is what keeps a
        // silent fallback from inventing a setup nobody drew.
        chosen.expect("no setup won the weighted draw — vanilla throws here too")
    } else {
        0
    };
    let setup = setups[setup_index];

    let air_pocket = sample(random, setup.air_pocket_probability);

    let giant = random.next_f32() < PROBABILITY_OF_GIANT_PORTAL;
    let names: &[&str] = if giant { &GIANT_PORTALS } else { &PORTALS };
    let i = usize::try_from(
        random.next_i32_bound(i32::try_from(names.len()).expect("a template count fits an i32")),
    )
    .expect("a bounded draw is non-negative");
    let template = names[i].to_string();

    const ROTATIONS: [Rotation; 4] = [
        Rotation::None,
        Rotation::Clockwise90,
        Rotation::Clockwise180,
        Rotation::Counterclockwise90,
    ];
    let rotation = ROTATIONS[usize::try_from(random.next_i32_bound(4)).expect("0..4 is a usize")];

    // `nextFloat() < 0.5 ? NONE : FRONT_BACK` — below the threshold is NONE.
    let mirror = if random.next_f32() < 0.5 {
        Mirror::None
    } else {
        Mirror::FrontBack
    };

    Plan {
        setup_index,
        placement: setup.placement,
        air_pocket,
        giant,
        template,
        rotation,
        mirror,
    }
}

/// The `Properties` the piece is built with.
///
/// `cold` is `setup.canBeCold() && Biome.coldEnoughToSnow(pos, seaLevel)` at
/// the *stub* position, sampled through `QuartPos.fromBlock` on all three
/// axes — a biome lookup, so the caller supplies it.
#[must_use]
pub fn properties(setup: &Setup, air_pocket: bool, cold_enough_to_snow: bool) -> Properties {
    Properties {
        cold: setup.can_be_cold && cold_enough_to_snow,
        mossiness: setup.mossiness,
        air_pocket,
        overgrown: setup.overgrown,
        vines: setup.vines,
        replace_with_blackstone: setup.replace_with_blackstone,
    }
}

/// The site a portal is being fitted to — everything `findSuitableY` needs
/// that is not the random itself.
///
/// A struct rather than eleven positional arguments, because five of them are
/// `i32` box edges and a transposed pair would be silent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Site {
    pub placement: VerticalPlacement,
    pub air_pocket: bool,
    /// `getBaseHeight(centre, heightmap) - 1`. The `- 1` is applied by the
    /// caller in `findGenerationPoint`, not here.
    pub base_y: i32,
    /// The unrotated template's bounding-box y span.
    pub y_span: i32,
    pub box_min_x: i32,
    pub box_min_z: i32,
    pub box_max_x: i32,
    pub box_max_z: i32,
    /// `LevelHeightAccessor.getMinY()` — -64 in the overworld.
    pub min_level_y: i32,
}

/// `RuinedPortalStructure.findSuitableY`.
///
/// `base_y` is `getBaseHeight(centre, heightmap) - 1` — the `- 1` is applied
/// by the caller in `findGenerationPoint`, not here. `y_span` is the
/// unrotated template's bounding-box y span.
///
/// `opaque_at(x, y, z)` answers the heightmap predicate against the **noise
/// column**, which is the pre-surface, pre-decoration world. It is queried at
/// the box's four corners, in the order `ImmutableList.of` builds them:
/// `(minX,minZ) (maxX,minZ) (minX,maxZ) (maxX,maxZ)`. The order matters only
/// through the early exit at the third hit, which is why it is written out.
pub fn find_suitable_y<R: RandomSource + ?Sized, F: FnMut(i32, i32, i32) -> bool>(
    random: &mut R,
    site: &Site,
    mut opaque_at: F,
) -> i32 {
    let Site {
        placement,
        air_pocket,
        base_y,
        y_span,
        box_min_x,
        box_min_z,
        box_max_x,
        box_max_z,
        min_level_y,
    } = *site;
    let floor = min_level_y + MIN_Y_INDEX;

    let mut y = match placement {
        VerticalPlacement::InNether => {
            if air_pocket {
                random.next_i32_between_inclusive(32, 100)
            } else if random.next_f32() < 0.5 {
                random.next_i32_between_inclusive(27, 29)
            } else {
                random.next_i32_between_inclusive(29, 100)
            }
        }
        VerticalPlacement::InMountain => get_random_within_interval(random, 70, base_y - y_span),
        VerticalPlacement::Underground => {
            get_random_within_interval(random, floor, base_y - y_span)
        }
        VerticalPlacement::PartlyBuried => {
            base_y - y_span + random.next_i32_between_inclusive(2, 8)
        }
        // ON_LAND_SURFACE and ON_OCEAN_FLOOR both fall through, and neither
        // draws.
        VerticalPlacement::OnLandSurface | VerticalPlacement::OnOceanFloor => base_y,
    };

    let corners = [
        (box_min_x, box_min_z),
        (box_max_x, box_min_z),
        (box_min_x, box_max_z),
        (box_max_x, box_max_z),
    ];
    while y > floor {
        let mut opaque = 0;
        for (x, z) in corners {
            if opaque_at(x, y, z) {
                opaque += 1;
                if opaque == 3 {
                    return y;
                }
            }
        }
        y -= 1;
    }
    y
}

// ---------------------------------------------------------------------------
// The piece — `RuinedPortalPiece.postProcess` and everything under it.
// ---------------------------------------------------------------------------

use crate::structure::pieces::BoundingBox;
use crate::structure::place::{
    place_piece, PieceOrigin, PieceWorld, PlaceSettings, PosRuleTest, Processor, Rule, RuleTest,
};
use crate::structure::template::Template;

/// `RuinedPortalPiece.PROBABILITY_OF_GOLD_GONE`.
pub const PROBABILITY_OF_GOLD_GONE: f32 = 0.3;
/// `RuinedPortalPiece.PROBABILITY_OF_MAGMA_INSTEAD_OF_NETHERRACK`.
pub const PROBABILITY_OF_MAGMA_INSTEAD_OF_NETHERRACK: f32 = 0.07;
/// `RuinedPortalPiece.PROBABILITY_OF_MAGMA_INSTEAD_OF_LAVA`: f32 = 0.2;
pub const PROBABILITY_OF_MAGMA_INSTEAD_OF_LAVA: f32 = 0.2;

/// What the netherrack passes need of the world on top of [`PieceWorld`].
///
/// `spreadNetherrack` is the only thing here that reads a heightmap, and it
/// reads it through `LevelAccessor.getHeight(getHeightMapType(placement), ..)`
/// — so the choice of map is the piece's, not the caller's, and it is passed
/// as the `ocean_floor` flag rather than resolved here.
pub trait PortalWorld: PieceWorld {
    /// `LevelAccessor.getHeight(Heightmap.Types, x, z)`.
    ///
    /// **The `_WG` maps, not the live ones.** `ProtoChunk.setBlockState`
    /// updates the maps in `heightmapsAfter()`, which during the decoration
    /// steps is `FINAL_HEIGHTMAPS` — the four non-`_WG` maps. So
    /// `WORLD_SURFACE_WG` and `OCEAN_FLOOR_WG` are still the noise-stage
    /// fossil while a structure writes, and a portal's own blocks do not raise
    /// the surface it measures itself against.
    fn height(&mut self, ocean_floor: bool, x: i32, z: i32) -> i32;
}

/// `BoundingBox.getCenter()` — integer division of the span, so an even span
/// rounds toward the high corner.
#[must_use]
pub const fn box_center(b: BoundingBox) -> (i32, i32, i32) {
    (
        b.min_x + (b.max_x - b.min_x + 1) / 2,
        b.min_y + (b.max_y - b.min_y + 1) / 2,
        b.min_z + (b.max_z - b.min_z + 1) / 2,
    )
}

/// `BoundingBox.isInside(Vec3i)`.
#[must_use]
pub const fn box_contains(b: BoundingBox, p: (i32, i32, i32)) -> bool {
    p.0 >= b.min_x
        && p.0 <= b.max_x
        && p.1 >= b.min_y
        && p.1 <= b.max_y
        && p.2 >= b.min_z
        && p.2 <= b.max_z
}

/// `BoundingBox.encapsulate(BoundingBox)`, which in vanilla **mutates the
/// receiver**.
///
/// That mutation is load-bearing and is the least obvious line in
/// `postProcess`: the box handed in is `ChunkGenerator.getWritableArea`, and
/// widening it to cover the whole template is what lets a ruined portal write
/// its entire self in one chunk's pass instead of being clipped to that chunk.
#[must_use]
pub const fn box_encapsulate(a: BoundingBox, b: BoundingBox) -> BoundingBox {
    BoundingBox {
        min_x: if a.min_x < b.min_x { a.min_x } else { b.min_x },
        min_y: if a.min_y < b.min_y { a.min_y } else { b.min_y },
        min_z: if a.min_z < b.min_z { a.min_z } else { b.min_z },
        max_x: if a.max_x > b.max_x { a.max_x } else { b.max_x },
        max_y: if a.max_y > b.max_y { a.max_y } else { b.max_y },
        max_z: if a.max_z > b.max_z { a.max_z } else { b.max_z },
    }
}

/// `RuinedPortalPiece.getBlockReplaceRule(Block, float, Block)` — a
/// `RandomBlockMatchTest` whose block test short-circuits the draw.
fn replace_rule_random(block: &str, probability: f32, out: &str) -> Rule {
    Rule {
        input: RuleTest::RandomBlockMatch {
            block: block.to_string(),
            probability,
        },
        location: RuleTest::AlwaysTrue,
        // `new ProcessorRule(input, loc, output)` delegates to the four-arg
        // constructor with `PosAlwaysTrueTest.INSTANCE`, so a rule built from
        // Rust carries the same default a rule parsed from a document without
        // the field does.
        position: PosRuleTest::AlwaysTrue,
        output: out.to_string(),
        append_loot: None,
    }
}

/// `RuinedPortalPiece.getBlockReplaceRule(Block, Block)` — a `BlockMatchTest`,
/// which spends nothing.
fn replace_rule(block: &str, out: &str) -> Rule {
    Rule {
        input: RuleTest::BlockMatch(block.to_string()),
        location: RuleTest::AlwaysTrue,
        position: PosRuleTest::AlwaysTrue,
        output: out.to_string(),
        append_loot: None,
    }
}

/// `RuinedPortalPiece.getLavaProcessorRule`.
///
/// Three branches, and only the last one draws. An ocean-floor portal turns
/// lava to magma unconditionally, a cold one turns it to netherrack
/// unconditionally, and every other portal turns it to magma one time in five.
#[must_use]
pub fn lava_processor_rule(placement: VerticalPlacement, properties: &Properties) -> Rule {
    if placement == VerticalPlacement::OnOceanFloor {
        return replace_rule("minecraft:lava", "minecraft:magma_block");
    }
    if properties.cold {
        return replace_rule("minecraft:lava", "minecraft:netherrack");
    }
    replace_rule_random(
        "minecraft:lava",
        PROBABILITY_OF_MAGMA_INSTEAD_OF_LAVA,
        "minecraft:magma_block",
    )
}

/// `RuinedPortalPiece.makeSettings` — the processor chain, in order.
///
/// The order is behaviour, not presentation: the ignore runs first so a
/// dropped block never reaches the rules, and `LavaSubmergedBlockProcessor`
/// runs last so it judges the state the rules and the decay pass have already
/// settled on.
///
/// Two of the six are conditional, and both conditions are properties rather
/// than data:
///
/// * `air_pocket` picks `STRUCTURE_BLOCK` over `STRUCTURE_AND_AIR`, and **that
///   is the difference between writing this portal's 355 air blocks and
///   writing none of them.** The underground setup's `air_pocket_probability`
///   is 1.0, so the overworld portal in the measured window always has one;
/// * `replace_with_blackstone` appends `BlackstoneReplaceProcessor`, which
///   `ruined_portal.json` never turns on.
#[must_use]
pub fn make_settings(placement: VerticalPlacement, properties: &Properties) -> Vec<Processor> {
    let ignore = if properties.air_pocket {
        crate::structure::place::structure_block()
    } else {
        crate::structure::place::structure_and_air()
    };
    let mut rules = vec![
        replace_rule_random(
            "minecraft:gold_block",
            PROBABILITY_OF_GOLD_GONE,
            "minecraft:air",
        ),
        lava_processor_rule(placement, properties),
    ];
    if !properties.cold {
        rules.push(replace_rule_random(
            "minecraft:netherrack",
            PROBABILITY_OF_MAGMA_INSTEAD_OF_NETHERRACK,
            "minecraft:magma_block",
        ));
    }
    let mut out = vec![
        ignore,
        Processor::Rule(rules),
        Processor::BlockAge(properties.mossiness),
        Processor::ProtectedBlocks("minecraft:features_cannot_replace".to_string()),
        Processor::LavaSubmerged,
    ];
    if properties.replace_with_blackstone {
        out.push(Processor::BlackstoneReplace);
    }
    out
}

/// One placed ruined portal — everything `postProcess` needs that is not the
/// world or the random.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    /// The `GenerationStub` position, which becomes the piece's
    /// `templatePosition` unchanged.
    pub template_position: (i32, i32, i32),
    pub placement: VerticalPlacement,
    pub properties: Properties,
    pub rotation: Rotation,
    pub mirror: Mirror,
    /// `new BlockPos(size.getX() / 2, 0, size.getZ() / 2)` — in x and z.
    pub pivot: (i32, i32),
}

impl Piece {
    /// The pivot `findGenerationPoint` builds for a template of this size.
    #[must_use]
    pub const fn pivot_for(size: (i32, i32, i32)) -> (i32, i32) {
        (size.0 / 2, size.2 / 2)
    }

    /// `this.boundingBox` — `template.getBoundingBox(placeSettings,
    /// templatePosition)`, which `TemplateStructurePiece.postProcess`
    /// recomputes on every pass.
    #[must_use]
    pub fn bounding_box(&self, template: &Template) -> BoundingBox {
        template.bounding_box_mirrored(
            self.template_position,
            self.rotation,
            self.pivot,
            self.mirror,
        )
    }
}

/// `RuinedPortalPiece.postProcess`.
///
/// Returns `false` when the piece declined to run in this chunk, which is the
/// common case: **the whole method is guarded on the box's centre lying inside
/// the chunk's writable area**, so a portal runs exactly once, in the chunk
/// that owns its centre, and writes across chunk borders from there. That is
/// unlike every other structure this port builds, all of which are re-entered
/// per chunk and clipped to it, and it is the single most important structural
/// fact about this piece.
///
/// The order below is the bytecode's and none of it commutes: the template's
/// blocks are written first, then `spreadNetherrack` reads the world those
/// blocks are now part of, then the drip columns read what the spread left.
// `R` is `Sized` here and `?Sized` everywhere below it: the placement needs
// to hand the stream on as `&mut dyn RandomSource`, and an unsized `R` cannot
// be coerced to a trait object.
pub fn post_process<W: PortalWorld + ?Sized, R: RandomSource>(
    world: &mut W,
    random: &mut R,
    template: &Template,
    piece: &Piece,
    writable_area: BoundingBox,
) -> bool {
    let template_box = piece.bounding_box(template);
    if !box_contains(writable_area, box_center(template_box)) {
        return false;
    }
    // `box.encapsulate(box2)` — the clip is widened to the whole template, so
    // the placement below is not cut at the chunk border.
    let clip = box_encapsulate(writable_area, template_box);

    place_piece(
        world,
        template,
        // `makeSettings` resolves no processor list from the datapack — the
        // chain is built in Rust by `RuinedPortalPiece.makeSettings` and
        // carries no `PosRuleTest` — so no reference position is threaded.
        PieceOrigin::unthreaded(piece.template_position),
        // `makeSettings` never calls `setKnownShape`, and vanilla's default is
        // false — so the neighbour-shape pass runs, unlike a jigsaw piece.
        PlaceSettings::new(piece.rotation)
            .with_pivot(piece.pivot)
            .with_mirror(piece.mirror)
            .with_shape_update(),
        &make_settings(piece.placement, &piece.properties),
        clip,
        // The piece's own stream. Every ruined-portal template holds a chest,
        // so this spends exactly one `nextLong` before `spreadNetherrack`
        // asks for its apron radius — and that one draw is the difference
        // between vanilla's offset and ours.
        Some(random),
    );
    // `TemplateStructurePiece.postProcess`'s own second pass, still inside
    // `super.postProcess` and so **before** anything below: every jigsaw block
    // the template wrote is replaced by its `final_state`. A ruined portal's
    // processor chain has no jigsaw replacement, so the block really is placed
    // and really is rubbed out again — and `spreadNetherrack` reads the
    // result, because air is the one thing it will not build over.
    crate::structure::place::resolve_jigsaw_blocks(
        world,
        template,
        piece.template_position,
        PlaceSettings::new(piece.rotation)
            .with_pivot(piece.pivot)
            .with_mirror(piece.mirror),
        clip,
    );

    spread_netherrack(world, random, piece, template_box);
    add_netherrack_drip_columns_below_portal(world, random, piece, template_box);

    // The vine/leaf pass is guarded on the disjunction, so a setup with
    // neither — which is both of `ruined_portal.json`'s — walks nothing at
    // all rather than walking the box and doing nothing per position. The
    // difference is not cosmetic: `maybeAddLeavesAbove` draws a `nextFloat`
    // **unconditionally**, so a port that walked the box anyway would spend
    // one draw per position of a box this setup never touches.
    if piece.properties.vines || piece.properties.overgrown {
        for x in template_box.min_x..=template_box.max_x {
            for y in template_box.min_y..=template_box.max_y {
                for z in template_box.min_z..=template_box.max_z {
                    if piece.properties.vines {
                        maybe_add_vines(world, random, (x, y, z));
                    }
                    if piece.properties.overgrown {
                        maybe_add_leaves_above(world, random, (x, y, z));
                    }
                }
            }
        }
    }
    true
}

/// `RuinedPortalPiece.getSurfaceY` — `getHeight(..) - 1`.
fn surface_y<W: PortalWorld + ?Sized>(world: &mut W, x: i32, z: i32, ocean_floor: bool) -> i32 {
    world.height(ocean_floor, x, z) - 1
}

/// `RuinedPortalPiece.canBlockBeReplacedByNetherrackOrMagma`.
fn can_be_replaced<W: PortalWorld + ?Sized>(
    world: &mut W,
    placement: VerticalPlacement,
    pos: (i32, i32, i32),
) -> bool {
    let state = world.block_at(pos.0, pos.1, pos.2);
    let block = state.split('[').next().unwrap_or(&state).to_string();
    if block == "minecraft:air" || block == "minecraft:obsidian" {
        return false;
    }
    if world.block_in_tag("minecraft:features_cannot_replace", &block) {
        return false;
    }
    // The lava clause is skipped entirely in the nether, where lava is
    // ordinary terrain rather than something a portal should refuse to build
    // over.
    if placement != VerticalPlacement::InNether && block == "minecraft:lava" {
        return false;
    }
    true
}

/// `RuinedPortalPiece.placeNetherrackOrMagma`.
///
/// **The draw is conditional on `!cold`** — a cold portal short-circuits
/// before `nextFloat` and spends nothing here, which shifts every later draw
/// in the pass.
fn place_netherrack_or_magma<W: PortalWorld + ?Sized, R: RandomSource + ?Sized>(
    world: &mut W,
    random: &mut R,
    properties: &Properties,
    pos: (i32, i32, i32),
) {
    if !properties.cold && random.next_f32() < PROBABILITY_OF_MAGMA_INSTEAD_OF_NETHERRACK {
        world.set_block(pos.0, pos.1, pos.2, "minecraft:magma_block");
    } else {
        world.set_block(pos.0, pos.1, pos.2, "minecraft:netherrack");
    }
}

/// `RuinedPortalPiece.addNetherrackDripColumn` — one column, at most eight
/// blocks deep.
///
/// The loop condition is `i > 0 && random.nextFloat() < 0.5`, and the order of
/// the two halves is behaviour: at the eighth block `i` is zero and **no float
/// is drawn**, so a column that runs its full length costs eight draws, not
/// nine.
fn add_netherrack_drip_column<W: PortalWorld + ?Sized, R: RandomSource + ?Sized>(
    world: &mut W,
    random: &mut R,
    properties: &Properties,
    pos: (i32, i32, i32),
) {
    let (x, mut y, z) = pos;
    place_netherrack_or_magma(world, random, properties, (x, y, z));
    let mut i = 8;
    while i > 0 && random.next_f32() < 0.5 {
        y -= 1;
        i -= 1;
        place_netherrack_or_magma(world, random, properties, (x, y, z));
    }
}

/// `RuinedPortalPiece.addNetherrackDripColumnsBelowPortal`.
///
/// The bounds are **exclusive of the maxima and one in from the minima** —
/// `minX + 1 .. < maxX` — so the portal's own rim grows no columns. x is the
/// outer loop.
fn add_netherrack_drip_columns_below_portal<W: PortalWorld + ?Sized, R: RandomSource + ?Sized>(
    world: &mut W,
    random: &mut R,
    piece: &Piece,
    b: BoundingBox,
) {
    for x in (b.min_x + 1)..b.max_x {
        for z in (b.min_z + 1)..b.max_z {
            let pos = (x, b.min_y, z);
            let state = world.block_at(pos.0, pos.1, pos.2);
            if state.split('[').next().unwrap_or(&state) == "minecraft:netherrack" {
                add_netherrack_drip_column(
                    world,
                    random,
                    &piece.properties,
                    (pos.0, pos.1 - 1, pos.2),
                );
            }
        }
    }
}

/// `RuinedPortalPiece.spreadNetherrack` — the decayed apron around the portal.
///
/// # The weight ramp and the offset
///
/// A fixed 14-entry `float[]`, flat at 1.0 for the first seven and tapering to
/// 0.2, indexed by **Manhattan distance from the box centre plus an offset**.
/// The offset is `nextInt(max(1, 8 - avgSpan / 2))` where `avgSpan` is
/// `(xSpan + zSpan) / 2`, and it is the pass's only draw outside the
/// per-position ones — a bigger portal gets a smaller offset range and so a
/// wider apron.
///
/// `Math.max(0, dist + offset)` then clamps, and an index at or past 14 skips
/// the position **without drawing**. Every index inside the ramp costs one
/// `nextDouble`, whether or not anything is placed, so the draw count depends
/// on the geometry and not on the terrain.
///
/// # Why `nextDouble` against a `float`
///
/// The comparison is `random.nextDouble() < (double) weights[i]` — the float
/// is widened, not the double narrowed. At 0.9 and 0.7 the widened float is
/// not the decimal, so comparing in `f32` would take a different branch on a
/// draw that lands between the two representations.
fn spread_netherrack<W: PortalWorld + ?Sized, R: RandomSource + ?Sized>(
    world: &mut W,
    random: &mut R,
    piece: &Piece,
    b: BoundingBox,
) {
    const WEIGHTS: [f32; 14] = [
        1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.9, 0.9, 0.8, 0.7, 0.6, 0.4, 0.2,
    ];
    let n = i32::try_from(WEIGHTS.len()).expect("14 fits an i32");
    let on_surface = matches!(
        piece.placement,
        VerticalPlacement::OnLandSurface | VerticalPlacement::OnOceanFloor
    );
    let (cx, _, cz) = box_center(b);
    let avg_span = ((b.max_x - b.min_x + 1) + (b.max_z - b.min_z + 1)) / 2;
    let offset = random.next_i32_bound((8 - avg_span / 2).max(1));
    let ocean_floor = piece.placement.uses_ocean_floor_heightmap();

    for x in (cx - n)..=(cx + n) {
        for z in (cz - n)..=(cz + n) {
            let dist = (x - cx).abs() + (z - cz).abs();
            let index = (dist + offset).max(0);
            if index >= n {
                continue;
            }
            let weight = WEIGHTS[usize::try_from(index).expect("0..14 is a usize")];
            if random.next_f64() >= f64::from(weight) {
                continue;
            }
            let surface = surface_y(world, x, z, ocean_floor);
            // A surface portal sits on the ground and decays at the ground; a
            // buried one decays at its own floor, or at the surface if that is
            // somehow lower.
            let y = if on_surface {
                surface
            } else {
                b.min_y.min(surface)
            };
            if (y - b.min_y).abs() > 3 {
                continue;
            }
            if !can_be_replaced(world, piece.placement, (x, y, z)) {
                continue;
            }
            place_netherrack_or_magma(world, random, &piece.properties, (x, y, z));
            if piece.properties.overgrown {
                maybe_add_leaves_above(world, random, (x, y, z));
            }
            add_netherrack_drip_column(world, random, &piece.properties, (x, y - 1, z));
        }
    }
}

/// `RuinedPortalPiece.maybeAddVines`.
///
/// The draw is **conditional**: the air/vine test comes first and returns
/// before `getRandomHorizontalDirection` is reached, so a position that is air
/// or already vine spends nothing.
fn maybe_add_vines<W: PortalWorld + ?Sized, R: RandomSource + ?Sized>(
    world: &mut W,
    random: &mut R,
    pos: (i32, i32, i32),
) {
    let state = world.block_at(pos.0, pos.1, pos.2);
    let block = state.split('[').next().unwrap_or(&state).to_string();
    if block == "minecraft:air" || block == "minecraft:vine" {
        return;
    }
    // `Direction.Plane.HORIZONTAL`'s declared order, indexed by `nextInt(4)`.
    const HORIZONTAL: [(&str, i32, i32); 4] = [
        ("north", 0, -1),
        ("east", 1, 0),
        ("south", 0, 1),
        ("west", -1, 0),
    ];
    let (name, dx, dz) =
        HORIZONTAL[usize::try_from(random.next_i32_bound(4)).expect("0..4 is a usize")];
    let side = (pos.0 + dx, pos.1, pos.2 + dz);
    let neighbour = world.block_at(side.0, side.1, side.2);
    if neighbour.split('[').next().unwrap_or(&neighbour) != "minecraft:air" {
        return;
    }
    // `Block.isFaceFull(state.getCollisionShape(..), direction)` on the
    // *source* block's face toward the vine.
    let direction = match name {
        "north" => 2,
        "south" => 3,
        "west" => 4,
        _ => 5,
    };
    if world.face_is_collision_full(&state, direction) != Some(true) {
        return;
    }
    // `VineBlock.getPropertyForFace(direction.getOpposite())` — the vine hangs
    // on the face pointing back at the block it grew from.
    let opposite = match name {
        "north" => "south",
        "south" => "north",
        "west" => "east",
        _ => "west",
    };
    let vine = format!(
        "minecraft:vine[east={},north={},south={},up=false,west={}]",
        opposite == "east",
        opposite == "north",
        opposite == "south",
        opposite == "west"
    );
    world.set_block(side.0, side.1, side.2, &vine);
}

/// `RuinedPortalPiece.maybeAddLeavesAbove`.
///
/// **The `nextFloat` is drawn before anything is tested**, so this costs one
/// draw at every position it is called at, netherrack or not.
fn maybe_add_leaves_above<W: PortalWorld + ?Sized, R: RandomSource + ?Sized>(
    world: &mut W,
    random: &mut R,
    pos: (i32, i32, i32),
) {
    if random.next_f32() >= 0.5 {
        return;
    }
    let state = world.block_at(pos.0, pos.1, pos.2);
    if state.split('[').next().unwrap_or(&state) != "minecraft:netherrack" {
        return;
    }
    let above = world.block_at(pos.0, pos.1 + 1, pos.2);
    if above.split('[').next().unwrap_or(&above) != "minecraft:air" {
        return;
    }
    world.set_block(
        pos.0,
        pos.1 + 1,
        pos.2,
        "minecraft:jungle_leaves[distance=7,persistent=true,waterlogged=false]",
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::legacy::LegacyRandomSource;
    use crate::rng::worldgen_random::WorldgenRandom;

    /// `worldgen/structure/ruined_portal.json`, both setups, in file order.
    ///
    /// `golden_ruined_portal_setups` re-reads the extract and fails if this
    /// ever stops matching it, so this is a fixture and not a second source
    /// of truth.
    fn standard() -> Vec<Setup> {
        vec![
            Setup {
                placement: VerticalPlacement::Underground,
                air_pocket_probability: 1.0,
                mossiness: 0.2,
                overgrown: false,
                vines: false,
                can_be_cold: true,
                replace_with_blackstone: false,
                weight: 0.5,
            },
            Setup {
                placement: VerticalPlacement::OnLandSurface,
                air_pocket_probability: 0.5,
                mossiness: 0.2,
                overgrown: false,
                vines: false,
                can_be_cold: true,
                replace_with_blackstone: false,
                weight: 0.5,
            },
        ]
    }

    /// `worldgen/structure/ruined_portal_ocean.json` — **one** setup, so no
    /// weighted draw, and `air_pocket_probability: 0.0`, so no sample draw.
    fn ocean() -> Vec<Setup> {
        vec![Setup {
            placement: VerticalPlacement::OnOceanFloor,
            air_pocket_probability: 0.0,
            mossiness: 0.8,
            overgrown: false,
            vines: false,
            can_be_cold: true,
            replace_with_blackstone: false,
            weight: 1.0,
        }]
    }

    fn at(seed: i64, cx: i32, cz: i32) -> WorldgenRandom<LegacyRandomSource> {
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(0));
        random.set_large_feature_seed(seed, cx, cz);
        random
    }

    /// **The load-bearing test in this file.**
    ///
    /// Four distinct portals, every field taken out of vanilla's own saved
    /// `Children[]` in references that already existed on the box — nothing
    /// was generated to produce them. Between them they pin the setup pick,
    /// the air-pocket short circuit in both directions, the template array
    /// order, the rotation array order and the mirror threshold's sense.
    #[test]
    fn the_four_saved_portals_reproduce_field_for_field() {
        // seed 20260722, chunk (48,64). Saved: portal_3, CCW90, FRONT_BACK,
        // underground, air_pocket 1. Present identically in four independent
        // references (r16-ow-a, r16-ow-b, vanilla-r7, van-rows-c64).
        let p = plan(&mut at(20_260_722, 48, 64), &standard());
        assert_eq!(p.placement, VerticalPlacement::Underground);
        assert_eq!(p.setup_index, 0);
        assert!(p.air_pocket, "underground's probability is 1.0");
        assert!(!p.giant);
        assert_eq!(p.template, "ruined_portal/portal_3");
        assert_eq!(p.rotation, Rotation::Counterclockwise90);
        assert_eq!(p.mirror, Mirror::FrontBack);

        // seed 20260722, chunk (43,88). Saved: portal_9, CW180, FRONT_BACK,
        // underground.
        let p = plan(&mut at(20_260_722, 43, 88), &standard());
        assert_eq!(p.placement, VerticalPlacement::Underground);
        assert_eq!(p.template, "ruined_portal/portal_9");
        assert_eq!(p.rotation, Rotation::Clockwise180);
        assert_eq!(p.mirror, Mirror::FrontBack);

        // seed 20260722, chunk (92,81). Saved: portal_8, NONE, NONE,
        // on_land_surface, air_pocket 1 — the ONE case in the corpus where
        // `sample` really draws (probability 0.5), and the one that catches a
        // port that always draws.
        let p = plan(&mut at(20_260_722, 92, 81), &standard());
        assert_eq!(p.placement, VerticalPlacement::OnLandSurface);
        assert_eq!(p.setup_index, 1);
        assert!(p.air_pocket);
        assert_eq!(p.template, "ruined_portal/portal_8");
        assert_eq!(p.rotation, Rotation::None);
        assert_eq!(p.mirror, Mirror::None);

        // seed 31415926, chunk (49,46), `ruined_portal_ocean`. Saved:
        // portal_10, CCW90, FRONT_BACK, on_ocean_floor, air_pocket 0,
        // mossiness 0.8. One setup and probability 0.0, so this portal's
        // first draw is the giant-portal float.
        let p = plan(&mut at(31_415_926, 49, 46), &ocean());
        assert_eq!(p.placement, VerticalPlacement::OnOceanFloor);
        assert_eq!(p.setup_index, 0);
        assert!(!p.air_pocket, "the ocean setup's probability is 0.0");
        assert_eq!(p.template, "ruined_portal/portal_10");
        assert_eq!(p.rotation, Rotation::Counterclockwise90);
        assert_eq!(p.mirror, Mirror::FrontBack);
    }

    /// What the corpus above does **not** reach, stated so that no later
    /// reader mistakes four green cases for coverage.
    ///
    /// Vanilla saved no `partly_buried` or `in_mountain` portal in any
    /// reference on the box, and no giant-portal template (the `in_nether`
    /// portals it did save are gated in `worldgen-assembly`'s
    /// `golden_nether_ruined_portal`, not here). Those
    /// branches are ported from bytecode and are **unmeasured**. This test
    /// asserts only that they are reachable and take the draws the bytecode
    /// says, which is a shape check and not a parity result.
    #[test]
    fn the_corpus_does_not_reach_these_branches() {
        // A giant portal picks out of a three-entry array.
        assert_eq!(GIANT_PORTALS.len(), 3);
        assert_eq!(PORTALS.len(), 10);

        // `partly_buried` draws exactly one value, `randomBetweenInclusive(2,8)`.
        let mut r = at(1, 0, 0);
        let _shape = plan(&mut r, &standard());

        // `in_nether` with an air pocket takes one draw, `randomBetweenInclusive
        // (32, 100)`. The corner scan runs after **every** placement branch,
        // so the column predicate has to be solid for the drawn y to survive
        // to the return — with an empty world this walks to the floor instead,
        // which is what the first version of this assertion actually measured.
        let mut r = at(1, 0, 0);
        let with_pocket = find_suitable_y(
            &mut r,
            &Site {
                placement: VerticalPlacement::InNether,
                air_pocket: true,
                base_y: 64,
                y_span: 8,
                box_min_x: 0,
                box_min_z: 0,
                box_max_x: 1,
                box_max_z: 1,
                min_level_y: -64,
            },
            |_, _, _| true,
        );
        assert!(
            (32..=100).contains(&with_pocket),
            "in_nether with an air pocket draws into 32..=100, got {with_pocket}"
        );

        // And with nothing opaque anywhere the scan bottoms out at minY + 15,
        // whatever the branch drew.
        let mut r = at(1, 0, 0);
        let hollow = find_suitable_y(
            &mut r,
            &Site {
                placement: VerticalPlacement::InNether,
                air_pocket: true,
                base_y: 64,
                y_span: 8,
                box_min_x: 0,
                box_min_z: 0,
                box_max_x: 1,
                box_max_z: 1,
                min_level_y: -64,
            },
            |_, _, _| false,
        );
        assert_eq!(hollow, -49);
    }

    /// `sample` takes no draw at 0.0 or 1.0, and the proof is that the *next*
    /// value is unmoved.
    ///
    /// This is the trap the module header describes, asserted directly rather
    /// than only through the four-portal test that would catch it downstream.
    #[test]
    fn sample_short_circuits_without_drawing() {
        let mut a = at(20_260_722, 48, 64);
        assert!(sample(&mut a, 1.0));
        let after_short_circuit = a.next_i32_bound(1000);

        let mut b = at(20_260_722, 48, 64);
        let untouched = b.next_i32_bound(1000);
        assert_eq!(
            after_short_circuit, untouched,
            "probability 1.0 must consume nothing"
        );

        let mut c = at(20_260_722, 48, 64);
        assert!(!sample(&mut c, 0.0));
        assert_eq!(
            c.next_i32_bound(1000),
            untouched,
            "0.0 must consume nothing"
        );

        // And at any other probability it does draw.
        let mut d = at(20_260_722, 48, 64);
        let _ = sample(&mut d, 0.5);
        assert_ne!(
            d.next_i32_bound(1000),
            untouched,
            "0.5 must consume exactly one float"
        );
    }

    /// The degenerate interval yields `max` and takes no draw.
    #[test]
    fn an_empty_interval_returns_max_and_draws_nothing() {
        let mut r = at(7, 1, 2);
        let baseline = {
            let mut s = at(7, 1, 2);
            s.next_i32_bound(1000)
        };
        assert_eq!(get_random_within_interval(&mut r, 70, 40), 40);
        assert_eq!(r.next_i32_bound(1000), baseline);
        // min == max is also empty.
        let mut r = at(7, 1, 2);
        assert_eq!(get_random_within_interval(&mut r, 40, 40), 40);
        assert_eq!(r.next_i32_bound(1000), baseline);
    }

    /// The corner scan stops at the third opaque column, not the fourth, and
    /// it stops *at that y* rather than one below.
    #[test]
    fn the_scan_stops_at_three_opaque_corners() {
        let mut r = at(7, 1, 2);
        // Solid everywhere: the very first y tested wins.
        let y = find_suitable_y(
            &mut r,
            &Site {
                placement: VerticalPlacement::OnLandSurface,
                air_pocket: false,
                base_y: 70,
                y_span: 8,
                box_min_x: 0,
                box_min_z: 0,
                box_max_x: 4,
                box_max_z: 4,
                min_level_y: -64,
            },
            |_, _, _| true,
        );
        assert_eq!(y, 70);

        // Only two corners solid: it never reaches three and walks to the
        // floor, which is minY + 15.
        let mut r = at(7, 1, 2);
        let y = find_suitable_y(
            &mut r,
            &Site {
                placement: VerticalPlacement::OnLandSurface,
                air_pocket: false,
                base_y: 70,
                y_span: 8,
                box_min_x: 0,
                box_min_z: 0,
                box_max_x: 4,
                box_max_z: 4,
                min_level_y: -64,
            },
            |x, _, _| x == 0,
        );
        assert_eq!(y, -49);
    }

    /// `Mirror` is `NONE` *below* 0.5. Asserted on its own because the sense
    /// is the one that reads backwards, and seven of the nine saved pieces
    /// are `FRONT_BACK`, so a flipped threshold would still look plausible.
    #[test]
    fn the_mirror_threshold_puts_none_below_half() {
        // Two seeds either side of the threshold, taken from the corpus:
        // (92,81) gives NONE and (48,64) gives FRONT_BACK.
        assert_eq!(
            plan(&mut at(20_260_722, 92, 81), &standard()).mirror,
            Mirror::None
        );
        assert_eq!(
            plan(&mut at(20_260_722, 48, 64), &standard()).mirror,
            Mirror::FrontBack
        );
    }

    #[test]
    fn a_bad_placement_name_is_refused_rather_than_defaulted() {
        assert!(VerticalPlacement::parse("underground").is_ok());
        assert!(VerticalPlacement::parse("on_the_moon").is_err());
    }
}

#[cfg(test)]
mod piece_tests {
    use super::*;
    use crate::structure::place::Processor;
    use std::collections::{BTreeMap, BTreeSet};

    fn props(cold: bool, air_pocket: bool, blackstone: bool) -> Properties {
        Properties {
            cold,
            mossiness: 0.2,
            air_pocket,
            overgrown: false,
            vines: false,
            replace_with_blackstone: blackstone,
        }
    }

    fn kinds(list: &[Processor]) -> Vec<&'static str> {
        list.iter()
            .map(|p| match p {
                Processor::BlockIgnore(b) => {
                    if b.len() == 1 {
                        "ignore:structure_block"
                    } else {
                        "ignore:structure_and_air"
                    }
                }
                Processor::Rule(_) => "rule",
                Processor::BlockAge(_) => "age",
                Processor::ProtectedBlocks(_) => "protected",
                Processor::LavaSubmerged => "lava_submerged",
                Processor::BlackstoneReplace => "blackstone",
                Processor::JigsawReplacement => "jigsaw",
                Processor::Gravity { .. } => "gravity",
                Processor::Nop => "nop",
                Processor::Capped { .. } => "capped",
                Processor::BlockRot { .. } => "block_rot",
            })
            .collect()
    }

    /// The chain, in order. The order is behaviour: the ignore must run first
    /// so a dropped block never reaches the rules, and `LavaSubmerged` last so
    /// it judges the state the rules and the decay pass settled on.
    ///
    /// **There is no `JigsawReplacement` in it.** A ruined portal template can
    /// carry a jigsaw block — portal_3 has one — and with no replacement
    /// processor vanilla writes it through as a jigsaw block. A port that
    /// helpfully added the replacement would write that block's `final_state`
    /// instead.
    #[test]
    fn the_processor_chain_is_six_in_this_order_and_has_no_jigsaw_replacement() {
        let s = make_settings(VerticalPlacement::Underground, &props(false, true, false));
        assert_eq!(
            kinds(&s),
            vec![
                "ignore:structure_block",
                "rule",
                "age",
                "protected",
                "lava_submerged"
            ]
        );
        let with_blackstone =
            make_settings(VerticalPlacement::Underground, &props(false, true, true));
        assert_eq!(
            kinds(&with_blackstone).last().copied(),
            Some("blackstone"),
            "BlackstoneReplaceProcessor is appended AFTER LavaSubmerged"
        );
    }

    /// `air_pocket` is the single boolean that decides whether the template's
    /// air is written. portal_3 is 355 air of 554 blocks, so inverting this
    /// loses 64% of the piece.
    #[test]
    fn the_air_pocket_picks_the_ignore_processor() {
        let with = make_settings(VerticalPlacement::Underground, &props(false, true, false));
        let without = make_settings(VerticalPlacement::Underground, &props(false, false, false));
        assert_eq!(kinds(&with)[0], "ignore:structure_block");
        assert_eq!(kinds(&without)[0], "ignore:structure_and_air");
    }

    /// The netherrack-to-magma rule exists **only** when the portal is not
    /// cold, so `cold` changes the rule *count* and not just an output. A cold
    /// portal has two rules and a warm one has three.
    #[test]
    fn cold_removes_the_magma_rule_entirely() {
        let warm = make_settings(VerticalPlacement::Underground, &props(false, true, false));
        let cold = make_settings(VerticalPlacement::Underground, &props(true, true, false));
        let count = |s: &[Processor]| match &s[1] {
            Processor::Rule(r) => r.len(),
            _ => panic!("slot 1 is the rule processor"),
        };
        assert_eq!(count(&warm), 3);
        assert_eq!(count(&cold), 2);
    }

    /// The lava rule's three branches, and only the last one draws — the other
    /// two are `BlockMatchTest`, which spends nothing. Confusing them changes
    /// the stream for every later block at that position.
    #[test]
    fn the_lava_rule_has_three_branches_and_only_one_draws() {
        use crate::structure::place::RuleTest;
        let ocean =
            lava_processor_rule(VerticalPlacement::OnOceanFloor, &props(false, true, false));
        assert_eq!(ocean.input, RuleTest::BlockMatch("minecraft:lava".into()));
        assert_eq!(ocean.output, "minecraft:magma_block");

        let cold = lava_processor_rule(VerticalPlacement::Underground, &props(true, true, false));
        assert_eq!(cold.input, RuleTest::BlockMatch("minecraft:lava".into()));
        assert_eq!(cold.output, "minecraft:netherrack");

        let warm = lava_processor_rule(VerticalPlacement::Underground, &props(false, true, false));
        assert_eq!(
            warm.input,
            RuleTest::RandomBlockMatch {
                block: "minecraft:lava".into(),
                probability: 0.2
            }
        );
        assert_eq!(warm.output, "minecraft:magma_block");

        // An ocean-floor portal that is ALSO cold takes the ocean branch: the
        // ocean test comes first in the bytecode.
        let both = lava_processor_rule(VerticalPlacement::OnOceanFloor, &props(true, true, false));
        assert_eq!(both.output, "minecraft:magma_block");
    }

    /// A world of solid stone with a flat surface, recording every write.
    struct Flat {
        set: BTreeMap<(i32, i32, i32), String>,
        surface: i32,
    }

    // A NO-OP, SAID OUT LOUD. This world exists to count the draws a portal
    // piece spends and to compare the blocks it writes; no test here reads a
    // record, and a record it kept would be a fixture nothing asserts. The
    // production forwarding is in `worldgen-assembly` and is exercised by
    // `ci/be-channel-identical.sh`.
    impl crate::block_entity::BlockEntityRecorder for Flat {
        fn record_block_entity(
            &mut self,
            _x: i32,
            _y: i32,
            _z: i32,
            _record: crate::block_entity::Record,
        ) {
        }
    }

    impl crate::structure::place::PieceWorld for Flat {
        fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
            self.set.get(&(x, y, z)).cloned().unwrap_or_else(|| {
                if y > self.surface {
                    "minecraft:air"
                } else {
                    "minecraft:stone"
                }
                .to_string()
            })
        }
        fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
            self.set.insert((x, y, z), state.to_string());
        }
        fn world_surface_wg(&mut self, _x: i32, _z: i32) -> i32 {
            self.surface + 1
        }
        fn block_in_tag(&self, _tag: &str, _block: &str) -> bool {
            false
        }
    }
    impl PortalWorld for Flat {
        fn height(&mut self, _ocean_floor: bool, _x: i32, _z: i32) -> i32 {
            self.surface + 1
        }
    }

    /// A random that always returns the same float and int, so a test can
    /// count writes without modelling a stream.
    struct Fixed(f32, i32);
    impl RandomSource for Fixed {
        fn next_i32(&mut self) -> i32 {
            self.1
        }
        fn next_i32_bound(&mut self, _b: i32) -> i32 {
            self.1
        }
        fn next_i64(&mut self) -> i64 {
            0
        }
        fn next_bool(&mut self) -> bool {
            false
        }
        fn next_f32(&mut self) -> f32 {
            self.0
        }
        fn next_f64(&mut self) -> f64 {
            f64::from(self.0)
        }
        fn next_gaussian(&mut self) -> f64 {
            0.0
        }
        fn set_seed(&mut self, _s: i64) {}
    }

    /// `addNetherrackDripColumn`'s loop is `i > 0 && nextFloat() < 0.5`, and
    /// the order of the two halves is behaviour: at the eighth block `i` is
    /// zero and **no float is drawn**. So a column that runs its full length
    /// writes nine blocks — the first plus eight — and never a tenth.
    #[test]
    fn a_drip_column_is_at_most_nine_blocks_and_stops_without_a_tenth_draw() {
        let mut w = Flat {
            set: BTreeMap::new(),
            surface: 100,
        };
        // Always below 0.5, so the loop only ever ends on the counter.
        let mut r = Fixed(0.0, 0);
        add_netherrack_drip_column(&mut w, &mut r, &props(true, true, false), (0, 50, 0));
        assert_eq!(
            w.set.len(),
            9,
            "one unconditional placement plus eight loop iterations"
        );
        assert!(w.set.contains_key(&(0, 50, 0)));
        assert!(w.set.contains_key(&(0, 42, 0)));
        assert!(
            !w.set.contains_key(&(0, 41, 0)),
            "a tenth block means the counter was not the stop"
        );
    }

    /// A cold portal never draws in `placeNetherrackOrMagma` and always writes
    /// netherrack; a warm one below 0.07 writes magma. The cold short circuit
    /// is what makes the two spend different numbers of draws.
    #[test]
    fn cold_writes_netherrack_without_drawing_and_warm_can_write_magma() {
        let mut w = Flat {
            set: BTreeMap::new(),
            surface: 100,
        };
        let mut r = Fixed(0.0, 0); // 0.0 < 0.07, so warm would take magma
        place_netherrack_or_magma(&mut w, &mut r, &props(true, true, false), (0, 50, 0));
        assert_eq!(w.set[&(0, 50, 0)], "minecraft:netherrack");
        place_netherrack_or_magma(&mut w, &mut r, &props(false, true, false), (1, 50, 0));
        assert_eq!(w.set[&(1, 50, 0)], "minecraft:magma_block");
    }

    /// `canBlockBeReplacedByNetherrackOrMagma` refuses air, obsidian and — off
    /// the nether — lava. Obsidian is the portal's own frame, so replacing it
    /// would eat the structure.
    #[test]
    fn the_replace_predicate_refuses_air_obsidian_and_lava_outside_the_nether() {
        let mut w = Flat {
            set: BTreeMap::new(),
            surface: 100,
        };
        w.set_block(0, 50, 0, "minecraft:air");
        w.set_block(1, 50, 0, "minecraft:obsidian");
        w.set_block(2, 50, 0, "minecraft:lava[level=0]");
        w.set_block(3, 50, 0, "minecraft:stone");
        let u = VerticalPlacement::Underground;
        assert!(!can_be_replaced(&mut w, u, (0, 50, 0)));
        assert!(!can_be_replaced(&mut w, u, (1, 50, 0)));
        assert!(!can_be_replaced(&mut w, u, (2, 50, 0)));
        assert!(can_be_replaced(&mut w, u, (3, 50, 0)));
        // In the nether the lava clause is skipped entirely.
        assert!(can_be_replaced(
            &mut w,
            VerticalPlacement::InNether,
            (2, 50, 0)
        ));
    }

    /// `box_center` is integer division of the **span**, so an even span
    /// rounds toward the high corner. A ruined portal's whole position derives
    /// from this, twice — once for the stub and once for the chunk guard.
    #[test]
    fn the_box_centre_uses_the_span_and_rounds_high() {
        let b = BoundingBox::new(768, 0, 1024, 776, 8, 1039);
        // xSpan 9 -> 4, zSpan 16 -> 8.
        assert_eq!(box_center(b), (772, 4, 1032));
    }

    /// `encapsulate` widens the chunk's writable area to the whole template,
    /// which is what lets the portal write itself in one pass rather than
    /// being clipped to the chunk that owns its centre.
    #[test]
    fn encapsulate_widens_the_clip_to_cover_the_template() {
        let chunk = BoundingBox::new(768, -63, 1024, 783, 319, 1039);
        let template = BoundingBox::new(772, 40, 1032, 780, 48, 1047);
        let clip = box_encapsulate(chunk, template);
        assert_eq!(clip.min_x, 768);
        assert_eq!(clip.max_x, 783);
        assert_eq!(
            clip.max_z, 1047,
            "the template reaches into the next chunk and the clip must follow it"
        );
        assert_eq!(clip.min_y, -63);
    }

    // ---------------------------------------------------------------------
    // The apron. Nine of the fourteen injections `ci/falsify-ruined-portal.py`
    // could not catch on 2026-08-09 were in this code, which had no unit
    // coverage at all while the r=16 window was independently blaming it.
    // ---------------------------------------------------------------------

    /// A random that answers each method from its own constant and **records
    /// the call sequence**, so a test can assert the draw *kind* and *count*
    /// and not only the blocks.
    ///
    /// The kinds are separate on purpose. `spreadNetherrack` compares
    /// `nextDouble()` against a widened `float`, and a port that reaches for
    /// `nextFloat` instead is right about the threshold and wrong about the
    /// stream — with one constant for both, that injection is invisible.
    struct Log {
        f32v: f32,
        f64v: f64,
        intv: i32,
        calls: std::cell::RefCell<Vec<&'static str>>,
    }

    impl Log {
        fn new(f32v: f32, f64v: f64, intv: i32) -> Self {
            Self {
                f32v,
                f64v,
                intv,
                calls: std::cell::RefCell::new(Vec::new()),
            }
        }
        fn count(&self, what: &str) -> usize {
            self.calls.borrow().iter().filter(|c| **c == what).count()
        }
    }

    impl RandomSource for Log {
        fn next_i32(&mut self) -> i32 {
            self.calls.borrow_mut().push("int");
            self.intv
        }
        fn next_i32_bound(&mut self, _b: i32) -> i32 {
            self.calls.borrow_mut().push("int");
            self.intv
        }
        fn next_i64(&mut self) -> i64 {
            self.calls.borrow_mut().push("long");
            0
        }
        fn next_bool(&mut self) -> bool {
            self.calls.borrow_mut().push("bool");
            false
        }
        fn next_f32(&mut self) -> f32 {
            self.calls.borrow_mut().push("float");
            self.f32v
        }
        fn next_f64(&mut self) -> f64 {
            self.calls.borrow_mut().push("double");
            self.f64v
        }
        fn next_gaussian(&mut self) -> f64 {
            self.calls.borrow_mut().push("gaussian");
            0.0
        }
        fn set_seed(&mut self, _s: i64) {}
    }

    /// A cold, buried portal whose box is `[0,50,0 .. 8,58,7]` — the same
    /// 9x8 footprint as the measured `portal_3`, so `avgSpan` is 8 and the
    /// offset draw is `nextInt(4)`.
    fn buried_piece() -> Piece {
        Piece {
            template_position: (0, 50, 0),
            placement: VerticalPlacement::Underground,
            properties: props(/* cold */ true, true, false),
            rotation: Rotation::None,
            mirror: Mirror::None,
            pivot: (0, 0),
        }
    }

    fn box9x8() -> BoundingBox {
        BoundingBox::new(0, 50, 0, 8, 58, 7)
    }

    /// Every `(x, z)` the pass wrote at the apron layer.
    fn apron_columns(set: &BTreeMap<(i32, i32, i32), String>, y: i32) -> BTreeSet<(i32, i32)> {
        set.keys()
            .filter(|(_, py, _)| *py == y)
            .map(|(x, _, z)| (*x, *z))
            .collect()
    }

    /// The apron is a **Manhattan disk**, and its radius is `13 - offset`:
    /// the ramp is 14 long, indexed by `dist + offset`, and an index at or
    /// past the end skips the position without drawing.
    ///
    /// Taken with a draw of 0.95, which passes the flat 1.0 head of the ramp
    /// and fails every tapered entry — so the written disk is exactly
    /// `dist + offset <= 6`. A reversed ramp puts the 1.0s at the *outside*
    /// and writes an annulus instead.
    #[test]
    fn the_apron_is_a_manhattan_disk_whose_radius_the_ramp_and_the_offset_set() {
        for offset in 0..4 {
            let mut w = Flat {
                set: BTreeMap::new(),
                surface: 100,
            };
            let mut r = Log::new(0.99, 0.95, offset);
            spread_netherrack(&mut w, &mut r, &buried_piece(), box9x8());
            let (cx, _, cz) = box_center(box9x8());
            let want: BTreeSet<(i32, i32)> = (cx - 14..=cx + 14)
                .flat_map(|x| (cz - 14..=cz + 14).map(move |z| (x, z)))
                .filter(|(x, z)| (x - cx).abs() + (z - cz).abs() + offset <= 6)
                .collect();
            assert_eq!(
                apron_columns(&w.set, 50),
                want,
                "offset {offset}: the written columns are not the disk the ramp implies"
            );
        }
    }

    /// One `nextDouble` per position inside the ramp and **none** outside it,
    /// which is what makes the offset set the draw count and not only the
    /// radius.
    ///
    /// Also pins the draw *kind*: the comparison is `nextDouble()` against a
    /// widened `float`. Every float here is 0.99, so a port that drew a float
    /// would place nothing at all and the disk assertion above would fail
    /// too — but the count is what says which method was called.
    #[test]
    fn the_spread_spends_one_double_per_in_ramp_position_and_nothing_outside() {
        let offset = 3;
        let mut w = Flat {
            set: BTreeMap::new(),
            surface: 100,
        };
        let mut r = Log::new(0.99, 0.99, offset);
        spread_netherrack(&mut w, &mut r, &buried_piece(), box9x8());
        // A Manhattan disk of radius 13 - offset holds 2r(r+1) + 1 positions.
        let radius = 13 - offset;
        let want = 2 * radius * (radius + 1) + 1;
        assert_eq!(
            r.count("double"),
            usize::try_from(want).expect("positive"),
            "the pass must draw once per position with dist + offset < 14"
        );
        assert_eq!(r.count("int"), 1, "exactly one offset draw");
        // 0.99 passes the flat 1.0 head of the ramp and fails every tapered
        // entry, so the writes stop at `dist + offset == 6` while the DRAWS
        // run all the way to 13 — which is the distinction being asserted.
        let (cx, _, cz) = box_center(box9x8());
        assert_eq!(
            apron_columns(&w.set, 50)
                .iter()
                .map(|(x, z)| (x - cx).abs() + (z - cz).abs())
                .max(),
            Some(6 - offset)
        );
    }

    /// `Math.abs(y - box.minY) > 3` — three, not four. The band is what stops
    /// a buried portal decaying down a cliff it happens to sit beside.
    #[test]
    fn the_apron_y_band_is_three_blocks_from_the_box_floor() {
        for (surface, expect) in [(47, true), (46, false)] {
            let mut w = Flat {
                set: BTreeMap::new(),
                surface,
            };
            let mut r = Log::new(0.0, 0.0, 0);
            spread_netherrack(&mut w, &mut r, &buried_piece(), box9x8());
            assert_eq!(
                !w.set.is_empty(),
                expect,
                "surface {surface}: |y - minY| is {} and the band is 3",
                (surface - 50i32).abs()
            );
        }
    }

    /// A surface portal decays at the ground; a buried one at
    /// `min(box.minY, surface)`. Swapping the two arms is invisible whenever
    /// the surface happens to sit at the box floor, so the test puts them 30
    /// blocks apart — and then the buried arm has to be the *floor*.
    #[test]
    fn a_buried_portal_decays_at_its_floor_and_a_surface_one_at_the_ground() {
        let mut buried = buried_piece();
        buried.placement = VerticalPlacement::Underground;
        let mut w = Flat {
            set: BTreeMap::new(),
            surface: 80,
        };
        let mut r = Log::new(0.99, 0.0, 3);
        spread_netherrack(&mut w, &mut r, &buried, box9x8());
        assert!(
            w.set.keys().any(|(_, y, _)| *y == 50),
            "a buried portal's apron is at the box floor"
        );
        assert!(
            !w.set.keys().any(|(_, y, _)| *y == 80),
            "…and never at a surface 30 blocks above it"
        );

        let mut surface_piece = buried_piece();
        surface_piece.placement = VerticalPlacement::OnLandSurface;
        // The band is measured from the box floor either way, so the box has
        // to sit at the surface for a surface portal to write anything.
        let b = BoundingBox::new(0, 80, 0, 8, 88, 7);
        let mut w = Flat {
            set: BTreeMap::new(),
            surface: 80,
        };
        let mut r = Log::new(0.99, 0.0, 3);
        spread_netherrack(&mut w, &mut r, &surface_piece, b);
        assert!(
            w.set.keys().any(|(_, y, _)| *y == 80),
            "a surface portal's apron is at the ground"
        );
    }

    /// `addNetherrackDripColumn`'s loop is `i > 0 && nextFloat() < 0.5`. The
    /// **order** is behaviour and it is invisible in the blocks: testing the
    /// counter first means the eighth block is placed and no ninth float is
    /// drawn, so the column costs 8 draws and not 9. A port that draws first
    /// writes the same nine blocks one draw late, and everything after it in
    /// the pass lands one place early.
    #[test]
    fn a_drip_column_tests_its_counter_before_it_draws() {
        let mut w = Flat {
            set: BTreeMap::new(),
            surface: 100,
        };
        // Cold, so `placeNetherrackOrMagma` spends nothing and every float
        // counted here belongs to the loop.
        let mut r = Log::new(0.0, 0.0, 0);
        add_netherrack_drip_column(&mut w, &mut r, &props(true, true, false), (0, 50, 0));
        assert_eq!(w.set.len(), 9, "one placement plus eight iterations");
        assert_eq!(
            r.count("float"),
            8,
            "the counter reaching zero must end the loop WITHOUT a ninth draw"
        );
    }

    /// `maybeAddLeavesAbove` draws **before** it looks at the block, so a
    /// position it declines still costs the stream a float.
    #[test]
    fn maybe_add_leaves_above_spends_its_draw_before_it_looks() {
        let mut w = Flat {
            set: BTreeMap::new(),
            surface: 100,
        };
        // Not netherrack, so nothing is written whichever order it is in.
        w.set_block(0, 50, 0, "minecraft:stone");
        let mut r = Log::new(0.0, 0.0, 0);
        maybe_add_leaves_above(&mut w, &mut r, (0, 50, 0));
        assert_eq!(
            r.count("float"),
            1,
            "the unconditional draw is what keeps the stream in step"
        );
    }

    /// `addNetherrackDripColumnsBelowPortal` walks `minX + 1 .. < maxX`, so
    /// the portal's own rim grows no columns — seven by six here, not nine by
    /// eight.
    #[test]
    fn the_drip_columns_below_the_portal_skip_the_box_rim() {
        let b = box9x8();
        let mut w = Flat {
            set: BTreeMap::new(),
            surface: 100,
        };
        for x in b.min_x..=b.max_x {
            for z in b.min_z..=b.max_z {
                w.set_block(x, b.min_y, z, "minecraft:netherrack");
            }
        }
        let before = w.set.len();
        // 0.99 stops every column after its first block, so the count of new
        // positions is the count of columns.
        let mut r = Log::new(0.99, 0.99, 0);
        add_netherrack_drip_columns_below_portal(&mut w, &mut r, &buried_piece(), b);
        assert_eq!(
            w.set.len() - before,
            7 * 6,
            "the rim rows and columns must be excluded at both ends"
        );
    }

    /// A template with one jigsaw block at the origin and one stone block
    /// beside it — the smallest thing that exercises `postProcess`'s two
    /// halves.
    fn tiny_template() -> crate::structure::template::Template {
        use crate::structure::template::{JigsawNbt, Template, TemplateBlock};
        // Netherrack rather than stone: `BlockAgeProcessor` turns a full
        // stone block into stairs or cracked bricks off a per-position random
        // these tests do not drive, and netherrack is the one block in a
        // portal template that no processor in the chain touches.
        let plain = |pos| TemplateBlock {
            pos,
            state: 0,
            jigsaw: None,
            has_nbt: false,
            loot_table: None,
            sherds: None,
            payload: None,
            marker: None,
        };
        Template {
            // Four wide so a piece can straddle a chunk border while its
            // centre — which is what `postProcess` guards on — stays inside
            // the owning chunk.
            size: (4, 1, 1),
            palette: vec![
                "minecraft:netherrack".to_string(),
                "minecraft:jigsaw".to_string(),
            ],
            palettes: vec![vec![
                "minecraft:netherrack".to_string(),
                "minecraft:jigsaw".to_string(),
            ]],
            blocks: vec![
                plain((0, 0, 0)),
                plain((2, 0, 0)),
                plain((3, 0, 0)),
                TemplateBlock {
                    pos: (1, 0, 0),
                    state: 1,
                    jigsaw: Some(JigsawNbt {
                        name: "minecraft:empty".to_string(),
                        target: "minecraft:empty".to_string(),
                        pool: "minecraft:empty".to_string(),
                        final_state: "minecraft:air".to_string(),
                        rollable: true,
                        selection_priority: 0,
                        placement_priority: 0,
                    }),
                    has_nbt: true,
                    loot_table: None,
                    sherds: None,
                    payload: None,
                    marker: None,
                },
            ],
        }
    }

    fn tiny_piece() -> Piece {
        Piece {
            template_position: (0, 50, 0),
            placement: VerticalPlacement::Underground,
            properties: props(true, true, false),
            rotation: Rotation::None,
            mirror: Mirror::None,
            pivot: (0, 0),
        }
    }

    /// `postProcess` is guarded on the box **centre** lying in the chunk's
    /// writable area, and returns without writing a block when it does not.
    /// That guard is why a portal is written once and whole rather than
    /// re-entered per chunk, and dropping it writes the piece again in every
    /// chunk its box touches.
    #[test]
    fn post_process_runs_only_in_the_chunk_that_owns_the_box_centre() {
        let template = tiny_template();
        let piece = tiny_piece();
        let mut w = Flat {
            set: BTreeMap::new(),
            surface: 100,
        };
        let mut r = Log::new(0.99, 0.99, 0);
        let elsewhere = BoundingBox::new(64, -63, 64, 79, 319, 79);
        assert!(!post_process(&mut w, &mut r, &template, &piece, elsewhere));
        assert!(w.set.is_empty(), "a declining pass writes nothing");
        assert_eq!(
            r.calls.borrow().len(),
            0,
            "and spends nothing: the guard is before every draw"
        );

        let owner = BoundingBox::new(0, -63, 0, 15, 319, 15);
        assert!(post_process(&mut w, &mut r, &template, &piece, owner));
        assert!(!w.set.is_empty());
    }

    /// The clip is `writableArea.encapsulate(templateBox)`, so a template
    /// that reaches out of the owning chunk is still written whole. With the
    /// chunk itself as the clip the far half of the piece is lost.
    #[test]
    fn the_clip_is_widened_so_a_piece_crossing_a_border_is_written_whole() {
        let template = tiny_template();
        let mut piece = tiny_piece();
        // Box [13..16]: its centre is x = 15, inside the owning chunk, and
        // its last block is x = 16, outside it. A clip that is the chunk
        // loses that block; the encapsulated one keeps it.
        piece.template_position = (13, 50, 0);
        // The surface is ten blocks under the box floor, so the apron's y
        // band rejects every position and what is left is the template pass.
        let mut w = Flat {
            set: BTreeMap::new(),
            surface: 40,
        };
        let mut r = Log::new(0.99, 0.99, 0);
        let owner = BoundingBox::new(0, -63, 0, 15, 319, 15);
        assert!(post_process(&mut w, &mut r, &template, &piece, owner));
        assert!(
            w.set.contains_key(&(16, 50, 0)),
            "the block past the chunk border must still be written; wrote {:?}",
            w.set.keys().collect::<Vec<_>>()
        );
    }

    /// `placeInWorld` stamps `LootTableSeed = random.nextLong()` into every
    /// placed `RandomizableContainer`, off the **piece's** stream — and that
    /// is the whole reason the apron was one ring too wide for a day.
    ///
    /// The value is loot and nothing in this port stores it, so the only
    /// observable is the draw. Asserted as a count, because a block test
    /// cannot see it: every ruined-portal template holds exactly one chest,
    /// and one `nextLong` is the difference between vanilla's apron offset
    /// and ours.
    #[test]
    fn a_placed_chest_costs_the_piece_stream_one_long() {
        use crate::structure::template::{Template, TemplateBlock};
        let chest = |has_nbt| Template {
            size: (1, 1, 1),
            palette: vec!["minecraft:chest".to_string()],
            palettes: vec![vec!["minecraft:chest".to_string()]],
            blocks: vec![TemplateBlock {
                pos: (0, 0, 0),
                state: 0,
                jigsaw: None,
                has_nbt,
                // The draw-count test: what this asserts is how many
                // `nextLong`s the piece spends, which `has_nbt` alone decides.
                // A table here would add a recorded payload and test something
                // else.
                loot_table: None,
                sherds: None,
                payload: None,
                marker: None,
            }],
        };
        let owner = BoundingBox::new(0, -63, 0, 15, 319, 15);
        for (has_nbt, want) in [(true, 1), (false, 0)] {
            let mut w = Flat {
                set: BTreeMap::new(),
                surface: 40,
            };
            let mut r = Log::new(0.99, 0.99, 0);
            assert!(post_process(
                &mut w,
                &mut r,
                &chest(has_nbt),
                &tiny_piece(),
                owner
            ));
            assert_eq!(
                r.count("long"),
                want,
                "nbt={has_nbt}: the guard is `info.nbt() != null` AND the block \
                 entity being a RandomizableContainer"
            );
        }
    }

    /// A ruined portal's processor chain has **no** jigsaw replacement, so
    /// the jigsaw block really is placed — and then
    /// `TemplateStructurePiece.postProcess` overwrites it with its
    /// `final_state`, which for `portal_3` is `minecraft:air`.
    ///
    /// This is not cosmetic. `spreadNetherrack` runs next and refuses to
    /// build over air; leaving the jigsaw block there gives it something
    /// replaceable, which costs a `nextFloat` and a whole drip column of
    /// draws vanilla never spent and shifts every later position in the walk.
    #[test]
    fn a_jigsaw_block_becomes_its_final_state_before_the_apron_reads_the_world() {
        let template = tiny_template();
        let piece = tiny_piece();
        // The surface is ten blocks under the box floor, so `spreadNetherrack`
        // fails its y band everywhere and writes nothing — what is left at the
        // jigsaw position is what the template pass left. (A weight of 0.99
        // would NOT do it: the ramp's first seven entries are 1.0.)
        let mut w = Flat {
            set: BTreeMap::new(),
            surface: 40,
        };
        let mut r = Log::new(0.99, 0.99, 0);
        let owner = BoundingBox::new(0, -63, 0, 15, 319, 15);
        assert!(post_process(&mut w, &mut r, &template, &piece, owner));
        assert_eq!(
            w.set.get(&(0, 50, 0)).map(String::as_str),
            Some("minecraft:netherrack")
        );
        assert_eq!(
            w.set.get(&(1, 50, 0)).map(String::as_str),
            Some("minecraft:air"),
            "the jigsaw block must be rubbed out, not left in the world"
        );
    }
}
