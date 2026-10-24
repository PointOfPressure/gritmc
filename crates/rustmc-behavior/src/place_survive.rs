//! `BlockState.canSurvive` for the blocks a `BlockItem` places.
//!
//! # Why this is not [`crate::survival`]
//!
//! Same vanilla method, two different questions, two different worlds.
//!
//! [`crate::survival`] answers the **generator's** question: 51 states that a
//! `would_survive` placement modifier names, all vegetation, in a world its
//! oracle can only describe one block of. Its [`crate::survival::Survival`]
//! has three values because that harness genuinely cannot see far enough to
//! answer some rules, and a rule it cannot answer must not be read as a no.
//!
//! This answers `BlockItem.canPlace`'s: 1,115 blocks, in a world the server
//! has whole. There is no `Unknown` for want of a neighbour here — every
//! position is readable — so the only refusal left is *a rule this port has
//! not transcribed*, which is a different fact and is counted rather than
//! blurred into the same value.
//!
//! Merging them is a design change and not a transcription: it would mean
//! giving the worldgen harness a block-property table it does not have.
//!
//! # What `canPlace` is
//!
//! ```text
//!   BlockItem.canPlace, offsets 5-51
//!     5-24   mustSurvive() && !state.canSurvive(level, clickedPos)  -> false
//!    27-43   !level.isUnobstructed(state, clickedPos, placementContext(player))
//!                                                                   -> false
//!       46   true
//! ```
//!
//! `mustSurvive()` is `iconst_1; ireturn` on `BlockItem` and is overridden
//! only by `ScaffoldingBlockItem`. `isUnobstructed` is an entity sweep and
//! belongs to whoever models entities; this module is the first half.
//!
//! # The rule set, and what it is checked against
//!
//! `data/vanilla/item_survive.json` records vanilla's own answer for every
//! one of the 6,600 states whose class overrides `canSurvive`, over 79 fully
//! specified neighbourhoods at two light levels — 1,042,800 answers.
//! `crates/rustmc-behavior/tests/golden_item_survive.rs` replays all of it.
//!
//! So a family here is either **right on every world the table has** or it is
//! [`Survives::Unported`], and the test ratchets how many states are the
//! latter. There is no third state where a rule is written and unchecked.

use crate::block_props::{BlockProps, BlockPropsTable, PlaceTag};

/// `Direction`'s own ordinal order, which is what every face column in
/// `block_props.json` is indexed by.
pub const DOWN: usize = 0;
pub const UP: usize = 1;
pub const NORTH: usize = 2;
pub const SOUTH: usize = 3;
pub const WEST: usize = 4;
pub const EAST: usize = 5;

/// The offset of each direction, in the same order.
const OFFSETS: [(i32, i32, i32); 6] = [
    (0, -1, 0),
    (0, 1, 0),
    (0, 0, -1),
    (0, 0, 1),
    (-1, 0, 0),
    (1, 0, 0),
];

/// `Direction.getOpposite()` — DOWN/UP, NORTH/SOUTH, WEST/EAST, which is the
/// enum's own pairing and the reason the ordinal groups the axes Y, Z, X.
#[must_use]
pub fn opposite(direction: usize) -> usize {
    direction ^ 1
}

/// `Direction.getClockWise()` for a horizontal direction — NORTH -> EAST ->
/// SOUTH -> WEST. Not defined for DOWN or UP, and vanilla throws there;
/// `WallHangingSignBlock.canPlace` is the only caller here and its `FACING`
/// is horizontal by construction.
#[must_use]
fn clockwise(direction: usize) -> Option<usize> {
    Some(match direction {
        NORTH => EAST,
        EAST => SOUTH,
        SOUTH => WEST,
        WEST => NORTH,
        _ => return None,
    })
}

#[must_use]
fn counter_clockwise(direction: usize) -> Option<usize> {
    clockwise(direction).map(opposite)
}

/// What a `canSurvive` question came back with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Survives {
    Yes,
    No,
    /// The block's class overrides `canSurvive` and this port has not
    /// transcribed the rule.
    ///
    /// **Never read as a no.** A caller that refused every placement it could
    /// not adjudicate would silently stop a chorus flower, a scaffold or a
    /// coral being placed at all, which is further from vanilla than placing
    /// it. `vanilla_would_place` is the caller-facing form and answers `true`
    /// here for that reason, and the count of states in this arm is ratcheted
    /// by the golden test so it can only go down by a reviewed change.
    Unported,
}

/// The world a rule reads, reduced to what these rules actually ask for.
pub trait PlaceWorld {
    /// The full block state at a position, `name[a=b]` as
    /// `BlockStateParser.serialize` spells it.
    fn state_at(&self, x: i32, y: i32, z: i32) -> String;

    /// `LevelReader.getRawBrightness(pos, 0)`.
    ///
    /// Only `CropBlock.hasSufficientLight` (`>= 8`) and
    /// `MushroomBlock.canSurvive` (`< 13`) read it, and both read it at the
    /// **subject's own** position rather than at a neighbour's.
    fn raw_brightness(&self, x: i32, y: i32, z: i32) -> i32;
}

/// The block name of a state, `minecraft:oak_sign[rotation=0]` ->
/// `minecraft:oak_sign`.
#[must_use]
pub fn block_of(state: &str) -> &str {
    state.split_once('[').map_or(state, |(name, _)| name)
}

/// `state.getValue(property)` off the canonical `name[a=b,c=d]` spelling.
///
/// Matched as a whole comma-separated field, for the reason
/// [`crate::survival`]'s equivalent gives: the substring `half=upper` also
/// appears inside `other_half=upper`, and a property read that a prefix can
/// fool is a silent wrong answer rather than a loud one.
#[must_use]
pub fn property<'a>(state: &'a str, name: &str) -> Option<&'a str> {
    let body = state.split_once('[')?.1;
    let body = body.strip_suffix(']').unwrap_or(body);
    body.split(',').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == name).then_some(value)
    })
}

/// A `Direction`-valued property, as its ordinal.
#[must_use]
fn direction_property(state: &str, name: &str) -> Option<usize> {
    Some(match property(state, name)? {
        "down" => DOWN,
        "up" => UP,
        "north" => NORTH,
        "south" => SOUTH,
        "west" => WEST,
        "east" => EAST,
        _ => return None,
    })
}

/// Everything a rule can ask about one position, resolved once.
struct At<'a> {
    state: String,
    props: Option<&'a BlockProps>,
}

impl At<'_> {
    fn block(&self) -> &str {
        block_of(&self.state)
    }

    /// `BlockState.isFaceSturdy(level, pos, dir)` — the three-argument form,
    /// which fixes `SupportType.FULL`.
    fn sturdy(&self, direction: usize) -> bool {
        self.props.is_some_and(|p| p.sturdy_faces[direction])
    }

    /// `BlockState.isFaceSturdy(level, pos, dir, SupportType.RIGID)`.
    fn rigid(&self, direction: usize) -> bool {
        self.props.is_some_and(|p| p.rigid_faces[direction])
    }

    /// `Block.canSupportCenter(level, pos, dir)` — offsets 8-37: the
    /// `#unstable_bottom_center` refusal applies to a DOWN face **only**, and
    /// before any shape is read.
    fn support_center(&self, direction: usize) -> bool {
        let Some(props) = self.props else {
            return false;
        };
        if direction == DOWN && props.unstable_bottom_center {
            return false;
        }
        props.center_faces[direction]
    }

    /// `Block.isFaceFull(state.getCollisionShape(level, pos), dir)`.
    fn collision_full(&self, direction: usize) -> bool {
        self.props
            .is_some_and(|p| p.collision_full_faces[direction])
    }

    fn is_air(&self) -> bool {
        self.props.is_some_and(|p| p.air)
    }

    /// `BlockState.isSolid()`. Not `isSolidRender` — leaves are solid and do
    /// not render solid, and the sign family reads this one.
    fn is_solid(&self) -> bool {
        self.props.is_some_and(|p| p.solid)
    }

    fn is_solid_render(&self) -> bool {
        self.props.is_some_and(|p| p.solid_render)
    }

    fn tagged(&self, tag: PlaceTag) -> bool {
        self.props.is_some_and(|p| p.has_place_tag(tag))
    }
}

/// The reader a rule works through: a world, a block-property table, and the
/// position being asked about.
pub struct Neighbourhood<'a, W: PlaceWorld + ?Sized> {
    world: &'a W,
    props: &'a BlockPropsTable,
    pos: (i32, i32, i32),
}

impl<'a, W: PlaceWorld + ?Sized> Neighbourhood<'a, W> {
    #[must_use]
    pub fn new(world: &'a W, props: &'a BlockPropsTable, pos: (i32, i32, i32)) -> Self {
        Self { world, props, pos }
    }

    fn relative(&self, direction: usize) -> At<'a> {
        let (dx, dy, dz) = OFFSETS[direction];
        self.at((self.pos.0 + dx, self.pos.1 + dy, self.pos.2 + dz))
    }

    fn at(&self, (x, y, z): (i32, i32, i32)) -> At<'a> {
        let state = self.world.state_at(x, y, z);
        let props = self.props.get(&state);
        At { state, props }
    }

    /// This position itself, for the one rule that reads the subject's own
    /// block rather than a neighbour's.
    fn here(&self) -> At<'a> {
        self.at(self.pos)
    }

    fn brightness(&self) -> i32 {
        self.world
            .raw_brightness(self.pos.0, self.pos.1, self.pos.2)
    }
}

/// Every family this port has transcribed, named by the vanilla class that
/// declares `canSurvive`.
///
/// The dispatch is on the **declaring class**, which
/// `data/vanilla/item_survive.json` records per block, so this enum is a
/// projection of the jar's own census rather than a list of block names. That
/// is what makes [`unported_classes`] a fact rather than a guess.
/// Which `VegetationBlock.mayPlaceOn` a block inherits.
///
/// `VegetationBlock.canSurvive` is one line — `mayPlaceOn(below, level,
/// belowPos)` — so the whole rule for 48 of the 1,115 blocks is which
/// override of that method they reach. Eleven classes declare one and they do
/// **not** line up with `canSurvive`'s declaring class at all: every one of
/// them inherits `VegetationBlock.canSurvive` unchanged.
///
/// Keying on `canSurvive` alone answers all eleven with
/// `#supports_vegetation` and is wrong for `lily_pad`, `dead_bush` and
/// `cactus_flower` — 144 cells of the golden, which is how this enum came to
/// exist rather than by reading the class list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MayPlaceOn {
    /// `VegetationBlock.mayPlaceOn` itself — `#supports_vegetation`.
    Tag(PlaceTag),
    /// `SeagrassBlock` — a sturdy UP face **and not**
    /// `#cannot_support_seagrass`.
    Seagrass,
    /// `CactusFlowerBlock` — `#support_override_cactus_flower` **or**
    /// `isFaceSturdy(UP, CENTER)`.
    CactusFlower,
    /// `LilyPadBlock` — a fluid tag, a block tag, and an empty fluid above.
    LilyPad,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Family {
    /// `BaseTorchBlock`, `CandleBlock` — `canSupportCenter(below, UP)`.
    SupportCenterBelow,
    /// `WallTorchBlock`, `RedstoneWallTorchBlock`, `AmethystClusterBlock` —
    /// the block behind `FACING`, `isFaceSturdy(FACING)`.
    SturdyBehindFacing,
    /// `LadderBlock` — the same read, and identical bytecode by a different
    /// route: `canAttachTo` is `getBlockState(pos).isFaceSturdy(dir)`.
    Ladder,
    /// `CarpetBlock` — `!level.isEmptyBlock(below)`.
    NotEmptyBelow,
    /// `SnowLayerBlock`.
    SnowLayer,
    /// `BaseRailBlock` — `canSupportRigidBlock(below)`.
    RigidBelow,
    /// `BasePressurePlateBlock` — rigid **or** centre, in that order.
    RigidOrCenterBelow,
    /// `DiodeBlock` — `canSurviveOn` is `isFaceSturdy(UP, RIGID)`, which is
    /// `RigidBelow` reached through one more virtual call.
    Diode,
    /// `StandingSignBlock`, `BannerBlock` — `below.isSolid()`.
    SolidBelow,
    /// `WallSignBlock`, `WallBannerBlock` — the block behind `FACING`,
    /// `isSolid()`.
    SolidBehindFacing,
    /// `CeilingHangingSignBlock` — `isFaceSturdy(above, DOWN, CENTER)`.
    CenterAbove,
    /// `WallHangingSignBlock`.
    WallHangingSign,
    /// `VegetationBlock`, whose `canSurvive` is `mayPlaceOn(below)` and
    /// nothing else. **Which `mayPlaceOn`** is a second dispatch key — see
    /// [`MayPlaceOn`] — because three of the 48 subclasses override that
    /// method without overriding `canSurvive`.
    Vegetation(MayPlaceOn),
    /// `CropBlock` — sufficient light **and** `below.is(#supports_crops)`.
    Crop,
    /// `MushroomBlock`.
    Mushroom,
    /// `DoorBlock` — the lower half needs a sturdy UP face, the upper half
    /// needs the lower half.
    Door,
    /// `DoublePlantBlock` — the upper half needs the lower half of the same
    /// block, the lower half falls through to `VegetationBlock`.
    DoublePlant,
    /// `LanternBlock` — `canSupportCenter` through `getConnectedDirection`,
    /// which reads `HANGING`.
    Lantern,
    /// `FaceAttachedHorizontalDirectionalBlock` — buttons, levers, the
    /// grindstone: `canAttach` through `getConnectedDirection`, which reads
    /// `FACE` and then `FACING`.
    FaceAttached,
}

/// Which family a declaring class is, or `None` for a class this port has not
/// transcribed.
///
/// Keyed by the class name `item_survive.json` records, so a block that
/// changes superclass between versions moves families without this file
/// being edited — and a class that appears for the first time is
/// [`Survives::Unported`] rather than silently answered by a name that
/// happens to look similar.
fn family_of(class: &str, may_place_on: Option<&str>) -> Option<Family> {
    let short = class.rsplit('.').next().unwrap_or(class);
    Some(match short {
        "BaseTorchBlock" | "CandleBlock" => Family::SupportCenterBelow,
        "WallTorchBlock" | "RedstoneWallTorchBlock" | "AmethystClusterBlock" => {
            Family::SturdyBehindFacing
        }
        "LadderBlock" => Family::Ladder,
        "CarpetBlock" => Family::NotEmptyBelow,
        "SnowLayerBlock" => Family::SnowLayer,
        "BaseRailBlock" => Family::RigidBelow,
        "BasePressurePlateBlock" => Family::RigidOrCenterBelow,
        "DiodeBlock" => Family::Diode,
        "StandingSignBlock" | "BannerBlock" => Family::SolidBelow,
        "WallSignBlock" | "WallBannerBlock" => Family::SolidBehindFacing,
        "CeilingHangingSignBlock" => Family::CenterAbove,
        "WallHangingSignBlock" => Family::WallHangingSign,
        "VegetationBlock" => Family::Vegetation(may_place_on_of(may_place_on?)?),
        "CropBlock" => Family::Crop,
        "MushroomBlock" => Family::Mushroom,
        "DoorBlock" => Family::Door,
        "DoublePlantBlock" => Family::DoublePlant,
        "LanternBlock" => Family::Lantern,
        "FaceAttachedHorizontalDirectionalBlock" => Family::FaceAttached,
        _ => return None,
    })
}

/// Which `mayPlaceOn` override a block reaches, or `None` for one this port
/// has not transcribed.
///
/// The three left out — `StemBlock`, `NetherFungusBlock`, `NetherRootsBlock`,
/// six blocks between them — read an **instance field** rather than a static
/// tag (`getfield stemSupportBlocks`, `getfield supportBlocks`), so the tag is
/// a constructor argument and cannot be named from the class. Answering them
/// would need a per-block tag column in the extract; they are
/// [`Survives::Unported`] until something asks for one.
fn may_place_on_of(class: &str) -> Option<MayPlaceOn> {
    let short = class.rsplit('.').next().unwrap_or(class);
    Some(match short {
        "VegetationBlock" => MayPlaceOn::Tag(PlaceTag::SupportsVegetation),
        "DryVegetationBlock" => MayPlaceOn::Tag(PlaceTag::SupportsDryVegetation),
        "AzaleaBlock" => MayPlaceOn::Tag(PlaceTag::SupportsAzalea),
        "NetherSproutsBlock" => MayPlaceOn::Tag(PlaceTag::SupportsNetherSprouts),
        "NetherWartBlock" => MayPlaceOn::Tag(PlaceTag::SupportsNetherWart),
        "WitherRoseBlock" => MayPlaceOn::Tag(PlaceTag::SupportsWitherRose),
        "SeagrassBlock" => MayPlaceOn::Seagrass,
        "CactusFlowerBlock" => MayPlaceOn::CactusFlower,
        "LilyPadBlock" => MayPlaceOn::LilyPad,
        _ => return None,
    })
}

/// `BlockState.canSurvive(level, pos)` for a state whose declaring class is
/// `class`.
///
/// `class` is the class `item_survive.json` names for the block — the caller
/// holds that table, because it is the only thing that knows which class a
/// block's rule lives on and reading it off the name would be the hand-list
/// this whole design avoids. `None` is `BlockBehaviour`'s unconditional
/// `true`.
#[must_use]
pub fn can_survive<W: PlaceWorld + ?Sized>(
    n: &Neighbourhood<'_, W>,
    state: &str,
    class: Option<&str>,
    may_place_on: Option<&str>,
) -> Survives {
    let Some(class) = class else {
        // No override anywhere above the block: `BlockBehaviour.canSurvive`
        // is `iconst_1; ireturn`. 804 of the 1,115 blocks a `BlockItem`
        // names are in this arm and it is the commonest answer by far.
        return Survives::Yes;
    };
    let Some(family) = family_of(class, may_place_on) else {
        return Survives::Unported;
    };
    let yes = |b: bool| if b { Survives::Yes } else { Survives::No };
    match family {
        Family::SupportCenterBelow => yes(n.relative(DOWN).support_center(UP)),
        Family::SturdyBehindFacing | Family::Ladder | Family::SolidBehindFacing => {
            let Some(facing) = direction_property(state, "facing") else {
                return Survives::Unported;
            };
            let behind = n.relative(opposite(facing));
            yes(if family == Family::SolidBehindFacing {
                behind.is_solid()
            } else {
                behind.sturdy(facing)
            })
        }
        Family::NotEmptyBelow => yes(!n.relative(DOWN).is_air()),
        Family::SnowLayer => {
            // `SnowLayerBlock.canSurvive` in its own order, and the order is
            // load-bearing: the `#cannot_support_snow_layer` refusal is
            // checked before the override, so ice refuses even though its
            // face is full.
            let below = n.relative(DOWN);
            if below.tagged(PlaceTag::CannotSupportSnowLayer) {
                return Survives::No;
            }
            if below.tagged(PlaceTag::SupportOverrideSnowLayer) {
                return Survives::Yes;
            }
            // `below.is(this)` — this block, not any snow layer — and eight
            // layers, which is a full block of snow holding another.
            let stacked =
                below.block() == block_of(state) && property(&below.state, "layers") == Some("8");
            yes(below.collision_full(UP) || stacked)
        }
        Family::RigidBelow => yes(n.relative(DOWN).rigid(UP)),
        Family::Diode => yes(n.relative(DOWN).rigid(UP)),
        Family::RigidOrCenterBelow => {
            let below = n.relative(DOWN);
            yes(below.rigid(UP) || below.support_center(UP))
        }
        Family::SolidBelow => yes(n.relative(DOWN).is_solid()),
        Family::CenterAbove => {
            let above = n.relative(UP);
            // `isFaceSturdy(above, DOWN, CENTER)`, which is
            // `canSupportCenter`'s second half **without** its
            // `#unstable_bottom_center` refusal: this call site is
            // `BlockState.isFaceSturdy` directly and never goes through
            // `Block.canSupportCenter`. A fence gate above a hanging sign
            // holds it; a fence gate under a torch does not hold the torch.
            yes(above.props.is_some_and(|p| p.center_faces[DOWN]))
        }
        Family::WallHangingSign => {
            let Some(facing) = direction_property(state, "facing") else {
                return Survives::Unported;
            };
            let (Some(cw), Some(ccw)) = (clockwise(facing), counter_clockwise(facing)) else {
                return Survives::Unported;
            };
            yes(wall_hanging_attaches(n, state, cw, ccw)
                || wall_hanging_attaches(n, state, ccw, cw))
        }
        Family::Vegetation(may_place_on) => {
            let below = n.relative(DOWN);
            match may_place_on {
                MayPlaceOn::Tag(tag) => yes(below.tagged(tag)),
                // `SeagrassBlock.mayPlaceOn`: the sturdy face first, the tag
                // as a veto second, and the veto is one block wide
                // (`magma_block`).
                MayPlaceOn::Seagrass => {
                    yes(below.sturdy(UP) && !below.tagged(PlaceTag::CannotSupportSeagrass))
                }
                // `CactusFlowerBlock.mayPlaceOn`: the tag OR
                // `isFaceSturdy(UP, CENTER)` — the CENTER form directly, not
                // through `Block.canSupportCenter`, so no
                // `#unstable_bottom_center` refusal applies. A cactus flower
                // sits on a fence.
                MayPlaceOn::CactusFlower => yes(below
                    .tagged(PlaceTag::SupportOverrideCactusFlower)
                    || below.props.is_some_and(|p| p.center_faces[UP])),
                // `LilyPadBlock.mayPlaceOn`, all three terms:
                //   21-29  fluidBelow.is(#minecraft:supports_lily_pad)   [FLUID]
                //   32-39  below.is(#minecraft:supports_lily_pad)        [BLOCK]
                //   42-50  fluidAbove.is(Fluids.EMPTY)
                // The first two are the same name in two registries and the
                // fluid one is the live case: 11,761 states are in it and 5
                // are in the block one. The third is why a pad cannot be
                // placed under water, and it reads the fluid at the **pad's
                // own** position — `pos` here is the position below, so
                // `pos.above()` is the pad.
                MayPlaceOn::LilyPad => {
                    let supported = below.props.is_some_and(|p| p.fluid_supports_lily_pad)
                        || below.tagged(PlaceTag::SupportsLilyPad);
                    let here = n.here();
                    yes(supported && here.props.is_some_and(|p| p.fluid.is_none()))
                }
            }
        }
        Family::Crop => {
            // `hasSufficientLight` FIRST, then `VegetationBlock.canSurvive`
            // with `CropBlock`'s own `mayPlaceOn`, which is
            // `#supports_crops` and not `#supports_vegetation`. Two
            // different tags, and the crop one is `{farmland}` alone.
            if n.brightness() < 8 {
                return Survives::No;
            }
            yes(n.relative(DOWN).tagged(PlaceTag::SupportsCrops))
        }
        Family::Mushroom => {
            let below = n.relative(DOWN);
            if below.tagged(PlaceTag::OverridesMushroomLightRequirement) {
                return Survives::Yes;
            }
            // `< 13`, and `MushroomBlock.mayPlaceOn` is `isSolidRender` —
            // not the `#supports_vegetation` its superclass would use.
            yes(n.brightness() < 13 && below.is_solid_render())
        }
        Family::Door => {
            let below = n.relative(DOWN);
            match property(state, "half") {
                Some("lower") => yes(below.sturdy(UP)),
                Some("upper") => yes(below.block() == block_of(state)),
                _ => Survives::Unported,
            }
        }
        Family::DoublePlant => match property(state, "half") {
            Some("upper") => {
                let below = n.relative(DOWN);
                yes(below.block() == block_of(state)
                    && property(&below.state, "half") == Some("lower"))
            }
            Some("lower") => yes(n.relative(DOWN).tagged(PlaceTag::SupportsVegetation)),
            _ => Survives::Unported,
        },
        Family::Lantern => {
            // `getConnectedDirection` is DOWN when `hanging`, UP otherwise;
            // `canSurvive` then takes its opposite twice, so the block read
            // is at `opposite(connected)` and the face asked for is
            // `connected`.
            let connected = if property(state, "hanging") == Some("true") {
                DOWN
            } else {
                UP
            };
            yes(n.relative(opposite(connected)).support_center(connected))
        }
        Family::FaceAttached => {
            // `getConnectedDirection` is the direction the block points
            // **away** from its support, so a floor button's is UP. The
            // `lookupswitch` in the bytecode reads `1 -> DOWN, 2 -> UP,
            // default -> FACING` over a `$SwitchMap` whose numbering is the
            // compiler's and not `AttachFace`'s, so which arm is FLOOR
            // cannot be read off the listing — it is settled by
            // `LanternBlock.getConnectedDirection`, which spells the same
            // convention in the clear as `hanging ? DOWN : UP`, and by the
            // golden: floor -> DOWN put every button on its ceiling and
            // disagreed with vanilla on 8,784 cells.
            //
            // `canSurvive` then passes the opposite to `canAttach`, which
            // reads the block one step that way and asks for the face
            // pointing back at this one.
            let connected = match property(state, "face") {
                Some("floor") => UP,
                Some("ceiling") => DOWN,
                Some("wall") => match direction_property(state, "facing") {
                    Some(facing) => facing,
                    None => return Survives::Unported,
                },
                _ => return Survives::Unported,
            };
            let away = opposite(connected);
            yes(n.relative(away).sturdy(opposite(away)))
        }
    }
}

fn wall_hanging_attaches<W: PlaceWorld + ?Sized>(
    n: &Neighbourhood<'_, W>,
    state: &str,
    step: usize,
    face: usize,
) -> bool {
    // `WallHangingSignBlock.canAttachTo`: a neighbour that is itself in
    // `#wall_hanging_signs` attaches when its FACING **axis** matches this
    // sign's; anything else attaches on `isFaceSturdy(dir, FULL)`.
    let neighbour = n.relative(step);
    if neighbour.tagged(PlaceTag::WallHangingSigns) {
        let (Some(theirs), Some(ours)) = (
            direction_property(&neighbour.state, "facing"),
            direction_property(state, "facing"),
        ) else {
            return false;
        };
        // `Direction.Axis.test(dir)` — the same axis, which for the two
        // horizontals is `theirs >> 1 == ours >> 1` under the ordinal's own
        // Y, Z, X grouping.
        return theirs >> 1 == ours >> 1;
    }
    neighbour.sturdy(face)
}

/// The caller-facing form: would vanilla have let `BlockItem.place` write
/// this state here?
///
/// [`Survives::Unported`] is `true`, and that is the whole point of the
/// distinction. A placement path that refused what it could not adjudicate
/// would stop a scaffold, a coral or a chorus flower from being placed at
/// all — further from vanilla than placing it, and invisible in a trace
/// because the packet that is missing looks like a client that never
/// clicked.
#[must_use]
pub fn vanilla_would_place<W: PlaceWorld + ?Sized>(
    n: &Neighbourhood<'_, W>,
    state: &str,
    class: Option<&str>,
    may_place_on: Option<&str>,
) -> bool {
    can_survive(n, state, class, may_place_on) != Survives::No
}

/// Every declaring class this port does not answer, for a census a test can
/// assert rather than a list kept beside one.
#[must_use]
pub fn unported_classes<'a>(
    classes: impl Iterator<Item = (&'a str, Option<&'a str>)>,
) -> Vec<String> {
    let mut out: Vec<String> = classes
        .filter(|(class, may_place_on)| family_of(class, *may_place_on).is_none())
        // A `VegetationBlock` whose `mayPlaceOn` is the unported half is
        // named by **that** class, not by `VegetationBlock` — otherwise the
        // census would say the whole 48-block family is unported when six
        // blocks are.
        .map(|(class, may_place_on)| {
            if class.ends_with("VegetationBlock") {
                may_place_on.unwrap_or(class).to_string()
            } else {
                class.to_string()
            }
        })
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

#[cfg(test)]
mod tests;

/// `Direction.orderedByNearest(entity)` — the six directions in the order the
/// player is looking along them, nearest first.
///
/// # The whole function, and it is arithmetic rather than a table
///
/// ```text
///     0-18   xr = viewXRot * (pi/180);  yr = -viewYRot * (pi/180)
///    19-44   sinX = sin(xr); cosX = cos(xr); sinY = sin(yr); cosY = cos(yr)
///    46-85   east = sinY > 0;  up = sinX < 0;  south = cosY > 0
///   87-128   x = east ? sinY : -sinY
///            y = up   ? -sinX : sinX          <- note the inversion
///            z = south ? cosY : -cosY
///   130-142  xz = x * cosX;  zz = z * cosX
///   144-190  a = east ? EAST : WEST; b = up ? UP : DOWN; c = south ? SOUTH : NORTH
///   192-291  six orderings of (a, b, c) by comparing x, y, xz, zz
///            makeDirectionArray(p, q, r) = {p, q, r, ~r, ~q, ~p}
/// ```
///
/// **`0.017453292f` is the constant in the jar** — `Math.PI / 180` rounded to
/// an `f32` — and it is used as a float multiply on a float, so the
/// intermediate is not a double. Written that way here.
///
/// **The y term is inverted relative to the other two.** `up` is `sinX < 0`
/// and the magnitude taken is `-sinX` when `up`, where `east`/`south` take
/// the positive branch when their flag is set. That is not a transcription
/// slip: `xRot` is positive downwards.
///
/// `entity` here is the player, and the two rotations are the ones
/// `handleUseItemOn` has already snapped from the packet.
#[must_use]
pub fn ordered_by_nearest(x_rot: f32, y_rot: f32) -> [usize; 6] {
    const DEG: f32 = 0.017_453_292;
    let xr = x_rot * DEG;
    let yr = -y_rot * DEG;
    let sin_x = xr.sin();
    let cos_x = xr.cos();
    let sin_y = yr.sin();
    let cos_y = yr.cos();
    let east = sin_y > 0.0;
    let up = sin_x < 0.0;
    let south = cos_y > 0.0;
    let x = if east { sin_y } else { -sin_y };
    let y = if up { -sin_x } else { sin_x };
    let z = if south { cos_y } else { -cos_y };
    let xz = x * cos_x;
    let zz = z * cos_x;
    let a = if east { EAST } else { WEST };
    let b = if up { UP } else { DOWN };
    let c = if south { SOUTH } else { NORTH };
    // The six arms at offsets 192-291, in the jar's own order.
    let (p, q, r) = if x > z {
        if y > xz {
            (b, a, c)
        } else if zz > y {
            (a, c, b)
        } else {
            (a, b, c)
        }
    } else if y > zz {
        (b, c, a)
    } else if xz > y {
        (c, a, b)
    } else {
        (c, b, a)
    };
    [p, q, r, opposite(r), opposite(q), opposite(p)]
}

/// `BlockPlaceContext.getNearestLookingDirections()` — [`ordered_by_nearest`],
/// then the clicked face's **opposite** moved to the front.
///
/// ```text
///    8-16   replaceClicked -> return the array unchanged
///   17-46   find the index of clickedFace.getOpposite()
///   49-67   if that index > 0, arraycopy it down one and put it at 0
/// ```
///
/// **`> 0`, not `>= 0`** — an opposite already at the head is left alone
/// rather than re-inserted, which would be a no-op anyway but is worth
/// reading correctly because the `arraycopy` length is that same index.
///
/// `replace_clicked` is `BlockPlaceContext`'s own flag: true when the clicked
/// block was replaceable and the placement therefore goes *into* it rather
/// than against its face.
#[must_use]
pub fn nearest_looking_directions(
    x_rot: f32,
    y_rot: f32,
    clicked_face: usize,
    replace_clicked: bool,
) -> [usize; 6] {
    let mut order = ordered_by_nearest(x_rot, y_rot);
    if replace_clicked {
        return order;
    }
    let want = opposite(clicked_face);
    let Some(at) = order.iter().position(|d| *d == want) else {
        return order;
    };
    if at > 0 {
        order.copy_within(0..at, 1);
        order[0] = want;
    }
    order
}

/// `StandingAndWallBlockItem.getPlacementState`'s walk — which of the two
/// blocks a torch, sign or banner item places, and whether it places at all.
///
/// ```text
///    0-8    wallState = wallBlock.getStateForPlacement(ctx)
///   23-27   for (Direction d : ctx.getNearestLookingDirections())
///   51-63     if (d == attachmentDirection.getOpposite()) continue
///   66-87     state = (d == attachmentDirection)
///                       ? getBlock().getStateForPlacement(ctx) : wallState
///   89-104    if (state != null && canPlace(level, state, clickedPos)) -> take it
///  119-143   the winner, but only if level.isUnobstructed(state, pos, empty())
/// ```
///
/// Three things this settles that the server's inlined version guessed:
///
/// * **it is not "a side face gets the wall variant".** The walk starts from
///   the player's *look*, and the clicked face only reorders it; a torch
///   clicked on a floor while looking at a wall can still come out standing,
///   and one clicked on a wall whose block cannot hold it walks on.
/// * **the standing block is only reached when `d == attachmentDirection`** —
///   DOWN for a sign, banner and torch, UP for a hanging sign. Every other
///   direction, including the two horizontals the opposite skip does not
///   remove, gets the wall block.
/// * **`canPlace` here is `state.canSurvive` alone** — a different method
///   from `BlockItem.canPlace`, with a different signature, no `mustSurvive`
///   and no `isUnobstructed`. The obstruction test happens once, on the
///   winner, at offsets 119-143.
///
/// Returns which of the two blocks to place, or `None` for vanilla refusing
/// the placement outright. `survives` is the caller's
/// [`can_survive`] over the state it would write.
#[must_use]
pub fn standing_or_wall(
    directions: [usize; 6],
    attachment: usize,
    survives: &dyn Fn(Placement) -> bool,
) -> Option<Placement> {
    let skip = opposite(attachment);
    for direction in directions {
        if direction == skip {
            continue;
        }
        let candidate = if direction == attachment {
            Placement::Standing
        } else {
            Placement::Wall
        };
        if survives(candidate) {
            return Some(candidate);
        }
    }
    None
}

/// Which of a `StandingAndWallBlockItem`'s two blocks a placement writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    /// `getBlock()` — the floor-standing block. A sign, a banner, a torch.
    Standing,
    /// `wallBlock` — the one the item names only through the pairing
    /// `item_survive.json` recovers by reflection.
    Wall,
}
