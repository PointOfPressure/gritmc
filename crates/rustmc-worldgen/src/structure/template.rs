//! Structure templates — the `.nbt` files vanilla ships for every
//! non-procedural structure.
//!
//! Reference: `StructureTemplate.load`. The jar carries **1,212** of these and
//! 49 of them are `underwater_ruin`; the port builds exactly one structure,
//! `mineshaft`, and that one is *procedural* — it computes its boxes in code.
//! Everything else is templates, so "19 of 20 structure sets unimplemented" is
//! one missing subsystem rather than nineteen missing features, and this is
//! its first piece.
//!
//! # The document
//!
//! A gzipped NBT compound:
//!
//! ```text
//!   size      list of 3 ints
//!   palette   list of { Name: string, Properties: compound? }
//!   blocks    list of { pos: list of 3 ints, state: int, nbt: compound? }
//!   entities  list — not read here; no structure this project needs has them
//! ```
//!
//! `palettes` (plural) also exists for structures with randomised variants.
//! It is deliberately not handled yet: nothing reads it until a structure that
//! uses it is implemented, and guessing at its semantics now would be a rule
//! this project has been bitten by all week — an unexercised code path is an
//! unverified one.
//!
//! # What this does not do
//!
//! No rotation, no processors, no loot, no jigsaw. Those are separable and
//! none of them is needed to establish that a template can be read correctly,
//! which is the only claim this module makes.

use rustmc_world::nbt::Nbt;
use std::collections::BTreeMap;

/// `Rotation` — the four quarter turns about the vertical axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rotation {
    None,
    Clockwise90,
    Clockwise180,
    Counterclockwise90,
}

impl Rotation {
    /// `Rotation.getRotated(Rotation)` — composition, in quarter turns
    /// clockwise.
    ///
    /// The enum's declaration order is `NONE, CLOCKWISE_90, CLOCKWISE_180,
    /// COUNTERCLOCKWISE_90`, so the ordinal is *not* the turn count:
    /// `COUNTERCLOCKWISE_90` is ordinal 3 and three quarter turns clockwise,
    /// which is the same rotation, so the two happen to agree here. Written
    /// as an explicit turn count anyway, because relying on that coincidence
    /// is how a port breaks the first time an enum gains a constant.
    #[must_use]
    pub const fn get_rotated(self, other: Self) -> Self {
        const TURNS: [Rotation; 4] = [
            Rotation::None,
            Rotation::Clockwise90,
            Rotation::Clockwise180,
            Rotation::Counterclockwise90,
        ];
        TURNS[((self.quarter_turns() + other.quarter_turns()) % 4) as usize]
    }

    /// Quarter turns clockwise, seen from above.
    #[must_use]
    pub const fn quarter_turns(self) -> u32 {
        match self {
            Self::None => 0,
            Self::Clockwise90 => 1,
            Self::Clockwise180 => 2,
            Self::Counterclockwise90 => 3,
        }
    }
}

/// `Mirror` — a reflection applied *before* the rotation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mirror {
    None,
    /// Negates `z`.
    LeftRight,
    /// Negates `x`.
    FrontBack,
}

/// `StructureTemplate.transform(pos, mirror, rotation, pivot)`.
///
/// From the bytecode: the mirror negates one horizontal axis, then the
/// rotation is applied about the pivot, and `y` is never touched.
///
/// ```text
///   mirror LEFT_RIGHT   z = -z
///          FRONT_BACK   x = -x
///   CLOCKWISE_90        (px + pz - z, y, pz - px + x)
///   COUNTERCLOCKWISE_90 (px - pz + z, y, px + pz - x)
///   CLOCKWISE_180       (px + px - x, y, pz + pz - z)
/// ```
///
/// **Which formula is which rotation is fixed geometrically, not read off the
/// switch.** The bytecode dispatches through a `$SwitchMap` whose table is not
/// in the method, so the case labels alone do not say which enum constant they
/// serve. Evaluating each at pivot `(0,0)` does: `(-z, x)` is a clockwise
/// quarter turn seen from above and `(z, -x)` is the counter-clockwise one.
/// Guessing from the label order would have been a coin flip.
#[must_use]
pub fn transform(
    pos: (i32, i32, i32),
    mirror: Mirror,
    rotation: Rotation,
    pivot: (i32, i32),
) -> (i32, i32, i32) {
    let (mut x, y, mut z) = pos;
    match mirror {
        Mirror::LeftRight => z = -z,
        Mirror::FrontBack => x = -x,
        Mirror::None => {}
    }
    let (px, pz) = pivot;
    match rotation {
        Rotation::Clockwise90 => (px + pz - z, y, pz - px + x),
        Rotation::Counterclockwise90 => (px - pz + z, y, px + pz - x),
        Rotation::Clockwise180 => (px + px - x, y, pz + pz - z),
        Rotation::None => (x, y, z),
    }
}

/// `Rotation.rotate(Direction)` for the four horizontal directions.
///
/// The vertical ones are unaffected, which is why `up` and `down` fall through
/// unchanged rather than being rejected: a block may legitimately face up.
///
/// The cycle's direction is fixed the same way the coordinate rotation's was —
/// by geometry, not by the order of a switch. A clockwise quarter turn seen
/// from above sends **north to east**, matching `(-z, x)` on coordinates since
/// north is `-z` and east is `+x`.
fn rotate_facing(facing: &str, rotation: Rotation) -> &'static str {
    const CW: [&str; 4] = ["north", "east", "south", "west"];
    let Some(i) = CW.iter().position(|d| *d == facing) else {
        return match facing {
            "up" => "up",
            "down" => "down",
            _ => "",
        };
    };
    let steps = match rotation {
        Rotation::None => 0,
        Rotation::Clockwise90 => 1,
        Rotation::Clockwise180 => 2,
        Rotation::Counterclockwise90 => 3,
    };
    CW[(i + steps) % 4]
}

/// `Mirror.mirror(Direction)` — flip a direction iff its axis is the one the
/// mirror reflects.
///
/// `FRONT_BACK` reflects the **x** axis (`east` <-> `west`) and `LEFT_RIGHT`
/// the **z** axis (`north` <-> `south`); `up`/`down` are fixed points of both.
/// Read straight off the bytecode, which is two `getAxis` comparisons and a
/// `getOpposite`.
fn mirror_facing(facing: &str, mirror: Mirror) -> &'static str {
    match (mirror, facing) {
        (Mirror::FrontBack, "east") => "west",
        (Mirror::FrontBack, "west") => "east",
        (Mirror::LeftRight, "north") => "south",
        (Mirror::LeftRight, "south") => "north",
        (_, "north") => "north",
        (_, "south") => "south",
        (_, "east") => "east",
        (_, "west") => "west",
        (_, "up") => "up",
        (_, "down") => "down",
        _ => "",
    }
}

/// Apply a mirror to a canonical `name[k=v,...]` state — `BlockState.mirror`.
///
/// **Vanilla's order is `state.mirror(m).rotate(r)`, mirror first**, and
/// [`transform`] already reflects the *position* before it turns it. This is
/// the state half of the same operation, and it was missing: `place_piece`
/// passed `Mirror::None` to `transform` unconditionally and never touched the
/// state at all, so every mirrored structure was placed unmirrored. Nothing
/// noticed because no structure this port had built was ever mirrored — a
/// ruined portal is the first, and two of the four in the mined corpus are
/// `FRONT_BACK`.
///
/// # Why one rule covers three different-looking overrides
///
/// Read from the bytecode, the three `mirror` overrides that matter here all
/// reduce to "flip a direction iff its axis is the mirrored one":
///
/// * `HorizontalDirectionalBlock.mirror` is
///   `state.rotate(mirror.getRotation(facing))`, and `getRotation` returns
///   `CLOCKWISE_180` exactly when the facing's axis matches — a 180 turn of a
///   horizontal facing *is* the flip;
/// * `CrossCollisionBlock.mirror` swaps `north`/`south` under `LEFT_RIGHT` and
///   `east`/`west` under `FRONT_BACK`, which is the same flip applied to which
///   connection is read from;
/// * `JigsawBlock.mirror` sends `ORIENTATION` through
///   `Mirror.rotation()`, an `OctahedralGroup` that inverts one axis, so each
///   half of the `FrontAndTop` pair is flipped independently by the same rule.
///
/// Everything else is a fixed point. `axis` is unsigned and a reflection does
/// not move it, `type`, `half` and `waterlogged` are orientation-free, and
/// `shape` is recomputed by the neighbour-shape pass rather than carried —
/// which is live here, because `RuinedPortalPiece.makeSettings` never calls
/// `setKnownShape` and vanilla's default is `false`.
#[must_use]
pub fn mirror_state(state: &str, mirror: Mirror) -> String {
    if mirror == Mirror::None {
        return state.to_string();
    }
    let Some((name, rest)) = state.split_once('[') else {
        return state.to_string();
    };
    let raw = rest.trim_end_matches(']');
    // As in `rotate_state`, the four connection booleans are permuted
    // together and so are read from the *source* string rather than mapped
    // pair by pair. A mirror is an involution, so the source of `north` under
    // `LEFT_RIGHT` is `south` and vice versa — the same table either way.
    let cross = permutes_connections(name);
    let props: Vec<String> = raw
        .split(',')
        .map(|pair| match pair.split_once('=') {
            Some((key @ ("north" | "east" | "south" | "west"), _)) if cross => {
                let from = mirror_facing(key, mirror);
                match prop_value(raw, from) {
                    Some(value) => format!("{key}={value}"),
                    None => pair.to_string(),
                }
            }
            Some(("facing", value)) => {
                let flipped = mirror_facing(value, mirror);
                if flipped.is_empty() {
                    pair.to_string()
                } else {
                    format!("facing={flipped}")
                }
            }
            Some(("shape", value)) if is_rail(name) => {
                format!(
                    "shape={}",
                    map_rail_shape(value, |d| mirror_facing(d, mirror))
                )
            }
            // `DoorBlock.mirror`: `mirror == NONE ? state :
            // state.rotate(mirror.getRotation(facing)).cycle(HINGE)` — the
            // facing flips above, and the hinge always changes sides. A
            // mansion's mirrored rooms are the first to need it: 15 door
            // halves in seed 30's (37,-47) mansion read the template's hinge.
            Some(("hinge", value)) if is_door(name) => {
                format!("hinge={}", if value == "left" { "right" } else { "left" })
            }
            Some(("rotation", value)) => match mirror_sixteen(value, mirror) {
                Some(flipped) => format!("rotation={flipped}"),
                None => pair.to_string(),
            },
            Some(("orientation", value)) => match value.rsplit_once('_') {
                Some((front, top)) => {
                    let (f, t) = (mirror_facing(front, mirror), mirror_facing(top, mirror));
                    if f.is_empty() || t.is_empty() {
                        pair.to_string()
                    } else {
                        format!("orientation={f}_{t}")
                    }
                }
                None => pair.to_string(),
            },
            _ => pair.to_string(),
        })
        .collect();
    format!("{name}[{}]", props.join(","))
}

/// A `DoorBlock`: every `*_door`, and no `*_trapdoor` (`TrapDoorBlock` is a
/// `HorizontalDirectionalBlock` with no `mirror` of its own).
fn is_door(name: &str) -> bool {
    name.ends_with("_door") && !name.ends_with("_trapdoor")
}

/// `(front, top)` from a jigsaw state's `orientation` property.
///
/// `FrontAndTop` serialises as `<front>_<top>` — `up_north`, `west_up` — and
/// `JigsawBlock.getFrontFacing`/`getTopFacing` are just its two halves. The
/// split is on the **last** underscore, because `down_east` and `counter`-free
/// names both contain one and only the tail is ever a bare direction.
#[must_use]
pub fn orientation(state: &str) -> Option<(Facing, Facing)> {
    let props = state.split_once('[')?.1.trim_end_matches(']');
    let value = props
        .split(',')
        .find_map(|pair| pair.strip_prefix("orientation="))?;
    let (front, top) = value.rsplit_once('_')?;
    Some((Facing::parse(front)?, Facing::parse(top)?))
}

/// Every block whose class is a `CrossCollisionBlock`, and therefore the only
/// blocks whose four connection booleans turn with a rotation.
///
/// **Derived, not guessed.** `generated/reports/blocks.json` gives each block
/// a `definition.type`, and the four types below are exactly the ones whose
/// classes extend `CrossCollisionBlock` — `FenceBlock` and `IronBarsBlock`
/// directly, `StainedGlassPaneBlock` and `WeatheringCopperBarsBlock` through
/// `IronBarsBlock`. None of the four overrides `rotate`, so all 39 use
/// `CrossCollisionBlock.rotate`:
///
/// ```text
///   minecraft:fence                 13
///   minecraft:iron_bars              6
///   minecraft:stained_glass_pane    16
///   minecraft:weathering_copper_bar  4
/// ```
///
/// **The list is explicit because a predicate over the properties would be
/// wrong.** Twelve block types in 26.2 carry boolean `north/east/south/west`
/// and only these four are cross-collision; the other eight are
/// `huge_mushroom`, `chorus_plant`, `fire`, `glow_lichen`, `multiface`,
/// `sculk_vein`, `tripwire` and `vine`. Rotating those with this permutation
/// would silently corrupt them — `VineBlock` and `MultifaceBlock` have their
/// own `rotate` and `ChorusPlantBlock` has none at all — so "has the four
/// booleans" is not the test and never was.
///
/// `golden_cross_collision_blocks` re-derives this from the extract.
pub const CROSS_COLLISION_BLOCKS: [&str; 39] = [
    "minecraft:acacia_fence",
    "minecraft:bamboo_fence",
    "minecraft:birch_fence",
    "minecraft:black_stained_glass_pane",
    "minecraft:blue_stained_glass_pane",
    "minecraft:brown_stained_glass_pane",
    "minecraft:cherry_fence",
    "minecraft:copper_bars",
    "minecraft:crimson_fence",
    "minecraft:cyan_stained_glass_pane",
    "minecraft:dark_oak_fence",
    "minecraft:exposed_copper_bars",
    "minecraft:glass_pane",
    "minecraft:gray_stained_glass_pane",
    "minecraft:green_stained_glass_pane",
    "minecraft:iron_bars",
    "minecraft:jungle_fence",
    "minecraft:light_blue_stained_glass_pane",
    "minecraft:light_gray_stained_glass_pane",
    "minecraft:lime_stained_glass_pane",
    "minecraft:magenta_stained_glass_pane",
    "minecraft:mangrove_fence",
    "minecraft:nether_brick_fence",
    "minecraft:oak_fence",
    "minecraft:orange_stained_glass_pane",
    "minecraft:oxidized_copper_bars",
    "minecraft:pale_oak_fence",
    "minecraft:pink_stained_glass_pane",
    "minecraft:purple_stained_glass_pane",
    "minecraft:red_stained_glass_pane",
    "minecraft:spruce_fence",
    "minecraft:warped_fence",
    "minecraft:waxed_copper_bars",
    "minecraft:waxed_exposed_copper_bars",
    "minecraft:waxed_oxidized_copper_bars",
    "minecraft:waxed_weathered_copper_bars",
    "minecraft:weathered_copper_bars",
    "minecraft:white_stained_glass_pane",
    "minecraft:yellow_stained_glass_pane",
];

/// The `minecraft:wall` blocks, by `definition.type` in
/// `generated/reports/blocks.json`.
///
/// **A wall is not a `CrossCollisionBlock` and it turns like one.** Vanilla's
/// `WallBlock` extends `Block` directly and its four connection properties are
/// `WallSide` enums — `none`/`low`/`tall` — rather than booleans, plus an
/// `up`. But `rotate` permutes those four exactly as `CrossCollisionBlock`
/// does its own: the new `north` is read from the old `west` under
/// `CLOCKWISE_90`, and `up` and `waterlogged` are fixed points.
///
/// Kept as a second list rather than folded into
/// [`CROSS_COLLISION_BLOCKS`] because that constant is derived from the
/// extract's four `CrossCollisionBlock` subtypes and is asserted equal to
/// them; merging the two would make that assertion false and turn a
/// class-hierarchy fact into a convenience.
///
/// **What this cost, and why it survived so long.** No structure this port
/// placed had ever written a ROTATED wall. Villages and End cities have
/// none in a rotated piece, a shipwreck has none at all, and a ruined portal
/// draws a rotation but its templates carry no wall connection state worth
/// turning. The first structure to do it is `bastion_remnant`, whose
/// ramparts are `blackstone_wall`, and on the r=16 nether window at seed
/// 20260722 the two starts land 88 wall states rotated 0 degrees where
/// vanilla rotated them — 73% of everything left inside their boxes after
/// the port. The mis-rotation is invisible to any window whose walls all
/// happen to be unrotated, which is the same shape as the `purpur_pillar`
/// `axis` defect the End city found.
pub const WALL_BLOCKS: [&str; 32] = [
    "minecraft:andesite_wall",
    "minecraft:blackstone_wall",
    "minecraft:brick_wall",
    "minecraft:cinnabar_brick_wall",
    "minecraft:cinnabar_wall",
    "minecraft:cobbled_deepslate_wall",
    "minecraft:cobblestone_wall",
    "minecraft:deepslate_brick_wall",
    "minecraft:deepslate_tile_wall",
    "minecraft:diorite_wall",
    "minecraft:end_stone_brick_wall",
    "minecraft:granite_wall",
    "minecraft:mossy_cobblestone_wall",
    "minecraft:mossy_stone_brick_wall",
    "minecraft:mud_brick_wall",
    "minecraft:nether_brick_wall",
    "minecraft:polished_blackstone_brick_wall",
    "minecraft:polished_blackstone_wall",
    "minecraft:polished_cinnabar_wall",
    "minecraft:polished_deepslate_wall",
    "minecraft:polished_sulfur_wall",
    "minecraft:polished_tuff_wall",
    "minecraft:prismarine_wall",
    "minecraft:red_nether_brick_wall",
    "minecraft:red_sandstone_wall",
    "minecraft:resin_brick_wall",
    "minecraft:sandstone_wall",
    "minecraft:stone_brick_wall",
    "minecraft:sulfur_brick_wall",
    "minecraft:sulfur_wall",
    "minecraft:tuff_brick_wall",
    "minecraft:tuff_wall",
];

/// Whether a block id is one of [`WALL_BLOCKS`].
#[must_use]
pub fn is_wall(name: &str) -> bool {
    WALL_BLOCKS.binary_search(&name).is_ok()
}

/// Whether a block's `north`/`east`/`south`/`west` properties are a
/// connection set that is permuted as a whole when the block turns or is
/// mirrored.
///
/// Two vanilla classes answer yes and they are unrelated by inheritance:
/// `CrossCollisionBlock` (fences, panes, bars) and `WallBlock`. The
/// permutation table is the same for both, which is why one predicate feeds
/// both [`rotate_state`] and [`mirror_state`] rather than each block family
/// carrying its own.
#[must_use]
pub fn permutes_connections(name: &str) -> bool {
    // `VineBlock.rotate`/`mirror` is a third, with the same tables over its
    // four side booleans (`up` is a fixed point). Found by the woodland
    // mansion: 12 vines on its walls read the unturned template's faces.
    //
    // And the rest of the four-boolean blocks whose classes override
    // `rotate`/`mirror` with the same mapping: `TripWireBlock` (the CROSS
    // table written out), `MultifaceBlock` (glow lichen, sculk vein, resin
    // clump — `mapDirections`, so `up`/`down` are fixed points of a turn
    // about Y) and `HugeMushroomBlock` (the same six-direction mapping).
    // `golden_cross_collision_blocks` derives this set from the blocks
    // report's `definition.type` and checks it both ways; fire and the
    // chorus plant are the two four-boolean blocks that do NOT turn.
    //
    // `RedStoneWireBlock.rotate`/`mirror` is the last: the same tables over
    // its four `RedstoneSide` enums (`none`/`side`/`up`), which are carried
    // whole. Found by the ancient city, the first structure to rotate a
    // wire: 16 wires in its CLOCKWISE_180 city centre read the unturned
    // template's sides.
    is_cross_collision(name)
        || is_wall(name)
        || matches!(
            name,
            "minecraft:vine"
                | "minecraft:tripwire"
                | "minecraft:glow_lichen"
                | "minecraft:sculk_vein"
                | "minecraft:resin_clump"
                | "minecraft:brown_mushroom_block"
                | "minecraft:red_mushroom_block"
                | "minecraft:mushroom_stem"
                | "minecraft:redstone_wire"
        )
}

/// `Rotation.rotate(value, 16)` over a `ROTATION_16` property — the one
/// skulls, standing and ceiling-hanging signs and banners all turn through:
/// a quarter turn is four steps.
fn rotate_sixteen(value: &str, rotation: Rotation) -> Option<String> {
    let v: i32 = value.parse().ok()?;
    let turned = match rotation {
        Rotation::None => v,
        Rotation::Clockwise90 => (v + 4) % 16,
        Rotation::Clockwise180 => (v + 8) % 16,
        Rotation::Counterclockwise90 => (v + 12) % 16,
    };
    Some(turned.to_string())
}

/// `Mirror.mirror(value, 16)`: values past the half are taken as negative
/// first, then `LEFT_RIGHT` is `(8 - j + 16) % 16` and `FRONT_BACK` is
/// `(16 - j) % 16`.
fn mirror_sixteen(value: &str, mirror: Mirror) -> Option<String> {
    let v: i32 = value.parse().ok()?;
    let j = if v > 8 { v - 16 } else { v };
    let flipped = match mirror {
        Mirror::None => v,
        Mirror::LeftRight => (8 - j + 16) % 16,
        Mirror::FrontBack => (16 - j) % 16,
    };
    Some(flipped.to_string())
}

/// The four `BaseRailBlock`s, whose `shape` is a `RailShape`.
fn is_rail(name: &str) -> bool {
    matches!(
        name,
        "minecraft:rail"
            | "minecraft:powered_rail"
            | "minecraft:detector_rail"
            | "minecraft:activator_rail"
    )
}

/// `BaseRailBlock.rotate`/`mirror` over a `RailShape`: every shape is the pair
/// of directions it joins (an ascending one is the direction it climbs
/// toward), so turning or reflecting the shape is turning or reflecting both
/// and naming the result. Vanilla's tables are exactly that, written out.
fn map_rail_shape(shape: &str, f: impl Fn(&str) -> &'static str) -> String {
    if let Some(dir) = shape.strip_prefix("ascending_") {
        return format!("ascending_{}", f(dir));
    }
    let Some((a, b)) = shape.split_once('_') else {
        return shape.to_string();
    };
    let (a, b) = (f(a), f(b));
    let has = |d: &str| a == d || b == d;
    if has("north") && has("south") {
        "north_south".to_string()
    } else if has("east") && has("west") {
        "east_west".to_string()
    } else {
        let ns = if has("north") { "north" } else { "south" };
        let ew = if has("east") { "east" } else { "west" };
        format!("{ns}_{ew}")
    }
}

/// Whether a block id is one of [`CROSS_COLLISION_BLOCKS`].
#[must_use]
pub fn is_cross_collision(name: &str) -> bool {
    CROSS_COLLISION_BLOCKS.binary_search(&name).is_ok()
}

/// Which direction a cross-collision connection is **read from** when the
/// block turns — `CrossCollisionBlock.rotate`, read out of the tableswitch.
///
/// The three cases in the bytecode are, in source order, `CLOCKWISE_180`,
/// `COUNTERCLOCKWISE_90` and `CLOCKWISE_90`, and each sets all four booleans
/// from the *unrotated* state. So this returns the **source** of `dir`, not
/// its destination, and reading it the other way round produces a rotation
/// that is correct for 180 and inverted for the two quarter turns — which is
/// exactly the bug a 180-only test cannot see.
fn cross_collision_source(dir: &str, rotation: Rotation) -> &'static str {
    match (rotation, dir) {
        (Rotation::Clockwise90, "north") => "west",
        (Rotation::Clockwise90, "east") => "north",
        (Rotation::Clockwise90, "south") => "east",
        (Rotation::Clockwise90, "west") => "south",

        (Rotation::Clockwise180, "north") => "south",
        (Rotation::Clockwise180, "east") => "west",
        (Rotation::Clockwise180, "south") => "north",
        (Rotation::Clockwise180, "west") => "east",

        (Rotation::Counterclockwise90, "north") => "east",
        (Rotation::Counterclockwise90, "east") => "south",
        (Rotation::Counterclockwise90, "south") => "west",
        (Rotation::Counterclockwise90, "west") => "north",

        // `Rotation::None` returns before this is reached, and any other
        // property name is not a connection.
        _ => "",
    }
}

/// The value of one `k=v` pair in a raw property string.
fn prop_value<'a>(props: &'a str, key: &str) -> Option<&'a str> {
    props
        .split(',')
        .find_map(|pair| pair.split_once('=').filter(|(k, _)| *k == key))
        .map(|(_, v)| v)
}

/// Apply a rotation to a canonical `name[k=v,...]` state.
///
/// Three properties turn: `facing`, `orientation` and `axis`. The ruin
/// templates carry `mode`, `facing`, `half`, `shape` and `waterlogged`, and of
/// those only `facing` is rotatable: `half` and `waterlogged` are
/// orientation-free, `mode` is the structure-block marker, and `shape` is
/// recomputed by `updateShape` from neighbours rather than carried through a
/// rotation.
///
/// Bounded by measurement rather than generality — `probe_template_properties`
/// counts what the 48 templates actually use, and a per-block dispatch for
/// properties none of them carry would be code no test could reach. `axis`
/// arrived that way too: it was worth exactly 40 blocks, all of them in the
/// End city's rotated `bridge_end` pieces, and nothing before that placed a
/// rotated pillar.
#[must_use]
pub fn rotate_state(state: &str, rotation: Rotation) -> String {
    if rotation == Rotation::None {
        return state.to_string();
    }
    let Some((name, rest)) = state.split_once('[') else {
        return state.to_string();
    };
    let raw = rest.trim_end_matches(']');
    // The four connection booleans are permuted **together**: the new `north`
    // is the old `west`, not a transform of the old `north`. A per-pair map
    // cannot express that, so the source string is kept and read from.
    let cross = permutes_connections(name);
    let props: Vec<String> = raw
        .split(',')
        .map(|pair| match pair.split_once('=') {
            Some((key @ ("north" | "east" | "south" | "west"), _)) if cross => {
                let from = cross_collision_source(key, rotation);
                match prop_value(raw, from) {
                    Some(value) => format!("{key}={value}"),
                    // A cross-collision state missing one of its four
                    // booleans is not a state vanilla can write; leaving the
                    // pair alone is better than inventing a connection.
                    None => pair.to_string(),
                }
            }
            Some(("facing", value)) => {
                let turned = rotate_facing(value, rotation);
                if turned.is_empty() {
                    pair.to_string()
                } else {
                    format!("facing={turned}")
                }
            }
            Some(("shape", value)) if is_rail(name) => {
                format!(
                    "shape={}",
                    map_rail_shape(value, |d| rotate_facing(d, rotation))
                )
            }
            // `RotatedPillarBlock.rotatePillar` — the two quarter turns swap
            // `x` and `z`, `y` is a fixed point, and CLOCKWISE_180 falls to
            // the default and changes nothing, which is geometrically right
            // for an unsigned axis.
            //
            // This was found by the End city: 40 of its blocks were
            // `purpur_pillar[axis=x]` where vanilla had `axis=z`, every one
            // of them in a `bridge_end` placed at `rot.getRotated(..)`. A
            // rotated pillar is invisible to any parity test whose structures
            // all happen to be unrotated, which is why it survived the
            // village gate.
            // `SkullBlock`, `StandingSignBlock`, `BannerBlock` and
            // `CeilingHangingSignBlock` — every `ROTATION_16` holder — turn
            // it through `Rotation.rotate(value, 16)`. The ancient city's
            // skulls were the first rotated ones: `rotation=6` read 14.
            Some(("rotation", value)) => match rotate_sixteen(value, rotation) {
                Some(turned) => format!("rotation={turned}"),
                None => pair.to_string(),
            },
            Some(("axis", value)) => match (value, rotation) {
                ("x", Rotation::Clockwise90 | Rotation::Counterclockwise90) => "axis=z".to_string(),
                ("z", Rotation::Clockwise90 | Rotation::Counterclockwise90) => "axis=x".to_string(),
                _ => pair.to_string(),
            },
            // `JigsawBlock.rotate` turns the `FrontAndTop` pair through
            // `Rotation.rotation().rotate(..)`, which for the four quarter
            // turns about Y is each half rotated independently. A jigsaw
            // whose orientation did not turn would still point where the
            // unrotated template pointed, and `canAttach` would then refuse
            // every child of a rotated piece.
            Some(("orientation", value)) => match value.rsplit_once('_') {
                Some((front, top)) => match (Facing::parse(front), Facing::parse(top)) {
                    (Some(front), Some(top)) => format!(
                        "orientation={}_{}",
                        front.rotated(rotation).name(),
                        top.rotated(rotation).name()
                    ),
                    _ => pair.to_string(),
                },
                None => pair.to_string(),
            },
            _ => pair.to_string(),
        })
        .collect();
    format!("{name}[{}]", props.join(","))
}

/// `Mth.getSeed(x, y, z)` — the position hash a structure processor seeds from.
///
/// Read from the bytecode, and two details in it are the kind a transcription
/// drops:
///
///   * `x * 3129871` is an **int** multiply (`imul` then `i2l`), so it wraps in
///     32 bits *before* widening — computing it in 64 bits gives a different
///     hash for any x past ~686 million, and silently the same one below that;
///   * the final shift is `lshr`, **arithmetic**, so a negative intermediate
///     keeps its sign rather than becoming huge and positive.
#[must_use]
pub fn position_seed(x: i32, y: i32, z: i32) -> i64 {
    let mut l = i64::from(x.wrapping_mul(3_129_871))
        ^ (i64::from(z).wrapping_mul(116_129_781))
        ^ i64::from(y);
    l = l
        .wrapping_mul(l)
        .wrapping_mul(42_317_861)
        .wrapping_add(l.wrapping_mul(11));
    l >> 16
}

/// `BlockIgnoreProcessor.STRUCTURE_AND_AIR` — the pair ocean ruins use.
///
/// Read from the class's static initialiser: `STRUCTURE_BLOCK` ignores
/// `Blocks.STRUCTURE_BLOCK`, `AIR` ignores `Blocks.AIR`, and
/// `STRUCTURE_AND_AIR` ignores both.
///
/// Dropping air is not a detail. A template's air cells mean "leave the world
/// alone here"; writing them would carve the template's bounding box out of
/// the seabed around every ruin.
#[must_use]
pub fn ignored_by_structure_and_air(state: &str) -> bool {
    let name = state.split('[').next().unwrap_or(state);
    name == "minecraft:air" || name == "minecraft:structure_block"
}

/// `BlockRotProcessor` — keep a block, or rot it away.
///
/// `processBlock` takes `settings.getRandom(info.pos())`, which with no random
/// set on the settings is `RandomSource.create(Mth.getSeed(pos))` — a
/// **`LegacyRandomSource`**, confirmed from `RandomSource.create`'s body, not
/// the xoroshiro the rest of worldgen uses. Then the block survives when
/// `nextFloat() <= integrity`.
///
/// The comparison is `<=`, not `<`: at `integrity` 1.0 every block must
/// survive, and `nextFloat()` can return values arbitrarily close to 1.
///
/// Position-derived, so this spends none of the feature's stream — which is
/// why it can be implemented independently of the placement order, unlike
/// almost everything else this project has chased.
#[must_use]
pub fn survives_rot(pos: (i32, i32, i32), integrity: f32) -> bool {
    use crate::rng::legacy::LegacyRandomSource;
    use crate::rng::RandomSource as _;
    let mut random = LegacyRandomSource::new(position_seed(pos.0, pos.1, pos.2));
    random.next_f32() <= integrity
}

/// The six directions, as `Direction`.
///
/// Only jigsaw blocks need them, and they need all six: a jigsaw's
/// `orientation` is a `FrontAndTop` pair and either half can be vertical —
/// `up_north` is the one every village street uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Facing {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

impl Facing {
    /// The name vanilla serialises, which is what the state string carries.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "down" => Self::Down,
            "up" => Self::Up,
            "north" => Self::North,
            "south" => Self::South,
            "west" => Self::West,
            "east" => Self::East,
            _ => return None,
        })
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Down => "down",
            Self::Up => "up",
            Self::North => "north",
            Self::South => "south",
            Self::West => "west",
            Self::East => "east",
        }
    }

    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::Down => Self::Up,
            Self::Up => Self::Down,
            Self::North => Self::South,
            Self::South => Self::North,
            Self::West => Self::East,
            Self::East => Self::West,
        }
    }

    /// `Direction.getStepX/Y/Z`.
    #[must_use]
    pub const fn step(self) -> (i32, i32, i32) {
        match self {
            Self::Down => (0, -1, 0),
            Self::Up => (0, 1, 0),
            Self::North => (0, 0, -1),
            Self::South => (0, 0, 1),
            Self::West => (-1, 0, 0),
            Self::East => (1, 0, 0),
        }
    }

    /// `Rotation.rotate(Direction)` — the vertical pair is a fixed point.
    #[must_use]
    pub fn rotated(self, rotation: Rotation) -> Self {
        const CW: [Facing; 4] = [Facing::North, Facing::East, Facing::South, Facing::West];
        let Some(index) = CW.iter().position(|d| *d == self) else {
            return self;
        };
        let steps = match rotation {
            Rotation::None => 0,
            Rotation::Clockwise90 => 1,
            Rotation::Clockwise180 => 2,
            Rotation::Counterclockwise90 => 3,
        };
        CW[(index + steps) % 4]
    }
}

/// The `minecraft:jigsaw` block entity's payload, as the assembler reads it.
///
/// `StructureTemplate$JigsawBlockInfo.of` builds this from the block's `nbt`
/// compound: `name`, `pool`, `target`, `joint` and the two priorities, each
/// with a default rather than an error — a jigsaw with no `pool` connects to
/// `minecraft:empty` and stops, which is a real configuration and not a
/// malformed template.
///
/// `final_state` is what the block *becomes* once the walk is done. It decides
/// nothing, but it is the block a village street actually writes, so dropping
/// it here would leave a dirt path made of jigsaw blocks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JigsawNbt {
    pub name: String,
    pub pool: String,
    pub target: String,
    /// `joint`. `rollable` lets the child take any rotation; `aligned`
    /// requires the two `top` faces to agree, which is what keeps a street
    /// from meeting a house upside down.
    pub rollable: bool,
    pub final_state: String,
    pub placement_priority: i32,
    pub selection_priority: i32,
}

/// `StructureTemplate$JigsawBlockInfo` — one jigsaw block, placed and rotated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JigsawBlock {
    /// World position, after the rotation and the origin offset.
    pub pos: (i32, i32, i32),
    /// `JigsawBlock.getFrontFacing` of the **rotated** state.
    pub front: Facing,
    /// `JigsawBlock.getTopFacing` of the rotated state.
    pub top: Facing,
    pub nbt: JigsawNbt,
    /// The rotated state string, kept so the writer can place `final_state`
    /// against the same orientation the connection was decided with.
    pub state: String,
}

impl JigsawBlock {
    /// `JigsawBlock.canAttach(parent, child)`.
    ///
    /// Three conditions, and the asymmetry is real: the **parent's** joint
    /// type decides whether the tops have to agree, and it is the parent's
    /// `target` that must equal the child's `name`, never the other way
    /// round.
    #[must_use]
    pub fn can_attach(&self, child: &Self) -> bool {
        self.front == child.front.opposite()
            && (self.nbt.rollable || self.top == child.top)
            && self.nbt.target == child.nbt.name
    }
}

/// One block of a template, in template-local coordinates.
// `Eq` is deliberately absent, for the reason `block_entity::Record` states:
// `sherds` is an `Nbt`, which holds floats, so it is `PartialEq` and cannot be
// `Eq`. Nothing here needs a total equality — the tests compare templates with
// `assert_eq!`, which wants only `PartialEq`, and no map is keyed on one.
#[derive(Clone, Debug, PartialEq)]
pub struct TemplateBlock {
    pub pos: (i32, i32, i32),
    /// Index into [`Template::palette`].
    pub state: usize,
    /// The block's `nbt`, parsed, when it is a jigsaw block.
    pub jigsaw: Option<JigsawNbt>,
    /// Whether the block carries an `nbt` compound at all.
    ///
    /// This is the bucket `StructureTemplate.addToLists` sorts it into, and
    /// the buckets are the load order: full blocks first, then everything
    /// else, then everything with a block entity. Recording the fact at parse
    /// time keeps the ordering rule in one place.
    pub has_nbt: bool,
    /// The block's `nbt.LootTable`, when it carries one.
    ///
    /// `None` for every block that is not a container with a table, which is
    /// almost all of them. Kept separately from [`Self::has_nbt`] because the
    /// two answer different questions: `has_nbt` decides whether
    /// `placeInWorld` spends a `nextLong` on this block, and this decides what
    /// the save writes beside the seed it spent.
    pub loot_table: Option<String>,
    /// The block's `nbt.sherds`, when it carries a non-empty one.
    ///
    /// `None` both when the key is absent and when it is an empty list,
    /// because `DecoratedPotBlockEntity.saveAdditional` skips the key entirely
    /// unless `decorations != PotDecorations.EMPTY` — so an empty list here and
    /// no list here are the same saved tag, and recording the difference would
    /// invent a key.
    pub sherds: Option<Nbt>,
    /// The block's whole `nbt` compound, unfiltered.
    ///
    /// **Unfiltered on purpose.** [`Self::loot_table`] and [`Self::sherds`]
    /// are two keys plucked out of this compound one at a time, each added
    /// when a class turned out to need it, and that pattern does not
    /// terminate: `vault` needs `config`, `trial_spawner` needs
    /// `normal_config` and `ominous_config`, and the next class needs
    /// whatever it needs. Vanilla does not pluck — `placeInWorld` hands the
    /// **whole** compound to `loadWithComponents` and the block entity's own
    /// `loadAdditional` reads what it understands and drops the rest.
    ///
    /// So the compound rides along whole, and the "reads what it understands"
    /// half lives in exactly one place, `block_entity::for_recorded`, which
    /// filters it per class against what that class's `saveAdditional`
    /// writes. Filtering here instead would put the save rules in the parser
    /// and give two places an opinion about a key.
    ///
    /// This carries `id` (the block-entity type) and a jigsaw block's own
    /// keys too. Neither reaches a payload: the emitter writes `id` itself
    /// and no class's allowlist names it.
    pub payload: Option<std::collections::BTreeMap<String, Nbt>>,
    /// A data marker's payload, from the block's own `nbt.metadata`.
    ///
    /// `TemplateStructurePiece.postProcess` scans the placed template for
    /// `minecraft:structure_block` entries carrying this string and hands each
    /// to `handleDataMarker`, which is how an ocean ruin gets its chest and its
    /// drowned. The marker block itself is stripped by
    /// `BlockIgnoreProcessor.STRUCTURE_AND_AIR` and never reaches the world, so
    /// dropping this at parse time loses the chest and nothing complains.
    pub marker: Option<String>,
}

/// A loaded structure template.
// `Eq` absent because `TemplateBlock` cannot have it — see above.
#[derive(Clone, Debug, PartialEq)]
pub struct Template {
    pub size: (i32, i32, i32),
    /// Block states as their canonical string form, `name[prop=value,...]`,
    /// which is what every other stage of this port compares against.
    ///
    /// The **first** palette. Every template loaded before shipwrecks had
    /// exactly one, so this stays the field the rest of the port reads; a
    /// caller that must honour a template's variants asks
    /// [`Template::choose_palette`] instead.
    pub palette: Vec<String>,
    /// Every palette, in file order. One entry for a `palette` template, and
    /// as many as the file holds for a `palettes` one — all twenty shipwreck
    /// templates carry **eight**, and every palette in a file has the same
    /// length because the `blocks` list indexes all of them.
    pub palettes: Vec<Vec<String>>,
    pub blocks: Vec<TemplateBlock>,
}

/// What a template could not provide.
#[derive(Debug, PartialEq, Eq)]
pub struct Malformed(pub String);

fn triple(tag: Option<&Nbt>, what: &str) -> Result<(i32, i32, i32), Malformed> {
    let list = tag
        .and_then(Nbt::as_list)
        .ok_or_else(|| Malformed(format!("`{what}` is not a list")))?;
    if list.len() != 3 {
        return Err(Malformed(format!(
            "`{what}` has {} entries, not 3",
            list.len()
        )));
    }
    let mut out = [0i32; 3];
    for (slot, entry) in out.iter_mut().zip(list) {
        *slot = entry
            .as_i32()
            .ok_or_else(|| Malformed(format!("`{what}` holds a non-int")))?;
    }
    Ok((out[0], out[1], out[2]))
}

/// `name[a=1,b=2]`, properties sorted — the same canonical form the block
/// tables and every golden vector in this project use, so a template block is
/// directly comparable with a generated one.
fn canonical(entry: &BTreeMap<String, Nbt>) -> Result<String, Malformed> {
    let name = entry
        .get("Name")
        .and_then(Nbt::as_str)
        .ok_or_else(|| Malformed("a palette entry has no `Name`".to_string()))?;
    let Some(props) = entry.get("Properties").and_then(Nbt::as_compound) else {
        return Ok(name.to_string());
    };
    if props.is_empty() {
        return Ok(name.to_string());
    }
    // `BTreeMap` is already sorted, which is the order the canonical form
    // wants — vanilla writes its own state strings the same way.
    let joined = props
        .iter()
        .map(|(k, v)| {
            let value = v
                .as_str()
                .ok_or_else(|| Malformed(format!("property `{k}` is not a string")))?;
            Ok(format!("{k}={value}"))
        })
        .collect::<Result<Vec<_>, Malformed>>()?
        .join(",");
    Ok(format!("{name}[{joined}]"))
}

impl Template {
    /// `new StructureTemplate()` — what `StructureTemplateManager.getOrCreate`
    /// hands back for a name the jar has no file for.
    ///
    /// It is not an error in vanilla and it must not be one here. 26.2's
    /// `ancient_city/walls/no_corners` pool names
    /// `ancient_city/walls/intact_horizontal_wall_stairs_5`, which is not in
    /// the jar; refusing the pool for it cut every wall that pool joins, and
    /// was the whole of the ancient city's 33-of-89 piece match. The empty
    /// template has size zero and no jigsaws, so it can never attach, but it
    /// stays in the pool's expanded list: it is drawn, shuffled, and spends
    /// its rotation shuffle like any other candidate.
    ///
    /// Vanilla's has no palettes at all; one empty palette is kept here so
    /// [`Template::choose_palette`] has something to index. Nothing can place
    /// it, so the difference is unobservable.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            size: (0, 0, 0),
            palette: Vec::new(),
            palettes: vec![Vec::new()],
            blocks: Vec::new(),
        }
    }

    /// Parse a template from its already-decompressed NBT root.
    ///
    /// # Errors
    /// If a required tag is missing or has the wrong shape.
    pub fn from_nbt(root: &Nbt) -> Result<Self, Malformed> {
        let size = triple(root.get("size"), "size")?;
        let read_palette = |tag: &[Nbt]| -> Result<Vec<String>, Malformed> {
            tag.iter()
                .map(|entry| {
                    entry
                        .as_compound()
                        .ok_or_else(|| Malformed("a palette entry is not a compound".to_string()))
                        .and_then(canonical)
                })
                .collect()
        };
        // `StructureTemplate.load` reads `palettes` when it is present and
        // `palette` otherwise — a template with variants stores one list per
        // variant and a single `blocks` list indexing all of them. Reading
        // only `palette` rejected every shipwreck template outright.
        let palettes = match root.get("palettes").and_then(Nbt::as_list) {
            Some(list) => list
                .iter()
                .map(|entry| {
                    entry
                        .as_list()
                        .ok_or_else(|| Malformed("a `palettes` entry is not a list".to_string()))
                        .and_then(read_palette)
                })
                .collect::<Result<Vec<_>, Malformed>>()?,
            None => vec![read_palette(
                root.get("palette")
                    .and_then(Nbt::as_list)
                    .ok_or_else(|| Malformed("no `palette` or `palettes` list".to_string()))?,
            )?],
        };
        if palettes.is_empty() {
            return Err(Malformed("`palettes` is empty".to_string()));
        }
        let palette = palettes[0].clone();

        let blocks_tag = root
            .get("blocks")
            .and_then(Nbt::as_list)
            .ok_or_else(|| Malformed("no `blocks` list".to_string()))?;
        let mut blocks = Vec::with_capacity(blocks_tag.len());
        for entry in blocks_tag {
            let pos = triple(entry.get("pos"), "pos")?;
            let state = entry
                .get("state")
                .and_then(Nbt::as_i32)
                .ok_or_else(|| Malformed("a block has no `state`".to_string()))?;
            let state = usize::try_from(state)
                .map_err(|_| Malformed(format!("negative palette index {state}")))?;
            if state >= palette.len() {
                return Err(Malformed(format!(
                    "palette index {state} outside a palette of {}",
                    palette.len()
                )));
            }
            // `metadata` on modern templates; `Metadata` on older ones. Both
            // are read because the extract is whatever version the jar ships.
            let nbt = entry.get("nbt").and_then(Nbt::as_compound);
            let marker = nbt
                .and_then(|nbt| {
                    nbt.get("metadata")
                        .or_else(|| nbt.get("Metadata"))
                        .and_then(Nbt::as_str)
                })
                .map(str::to_string);
            // The container's unresolved loot reference, kept because the save
            // writes it and the placer is the only thing that ever sees it.
            //
            // `placeInWorld` never reads this key — it stamps `LootTableSeed`
            // beside it and hands the whole compound to `loadWithComponents`,
            // which is what puts the table on the block entity. So keeping the
            // string is not modelling a draw or a decision, it is keeping the
            // one payload key a template supplies and this port was throwing
            // away. `has_nbt` alone could say a container had *some* tag and
            // never which table, and a `LootTable` with no seed is refused by
            // `block_entity::for_recorded` rather than emitted wrong on one key.
            let loot_table = nbt
                .and_then(|nbt| nbt.get("LootTable").and_then(Nbt::as_str))
                .map(str::to_string);
            // A decorated pot's four faces, kept for the same reason and
            // measured into existence by `docs/DETERMINISM.md` §44.8: carrying
            // only `LootTable` emitted eight pots on the r=16 window that were
            // right on the loot keys and missing this one, which `bediff`
            // scores as a payload mismatch rather than an absence.
            //
            // `DecoratedPotBlockEntity.saveAdditional` writes
            // `store("sherds", PotDecorations.CODEC, decorations)` **only when
            // `decorations != PotDecorations.EMPTY`**, and `loadAdditional`
            // reads `decorations` from this key with `PotDecorations.EMPTY` as
            // the default. So an absent or empty `sherds` here must stay absent
            // in the record: writing an empty list would be a key vanilla does
            // not save, which is the same class of wrong answer in the other
            // direction.
            // The whole compound, for the reason on the field.
            let payload = nbt.filter(|m| !m.is_empty()).cloned();
            let sherds = nbt
                .and_then(|nbt| nbt.get("sherds"))
                .filter(|v| !matches!(v, Nbt::List(items) if items.is_empty()))
                .cloned();
            // A jigsaw block is exactly a block whose `nbt.id` says so.
            // Reading it off the payload rather than the palette name means
            // the two cannot disagree, and it costs one string compare.
            let jigsaw = nbt
                .filter(|nbt| nbt.get("id").and_then(Nbt::as_str) == Some("minecraft:jigsaw"))
                .map(|nbt| {
                    let text = |key: &str, fallback: &str| {
                        nbt.get(key)
                            .and_then(Nbt::as_str)
                            .unwrap_or(fallback)
                            .to_string()
                    };
                    let int = |key: &str| nbt.get(key).and_then(Nbt::as_i32).unwrap_or(0);
                    JigsawNbt {
                        // `JigsawBlockEntity.EMPTY_ID` and `Pools.EMPTY` are
                        // the defaults vanilla's `orElse` supplies.
                        name: text("name", "minecraft:empty"),
                        pool: text("pool", "minecraft:empty"),
                        target: text("target", "minecraft:empty"),
                        // `StructureTemplate.getJointType` reads `joint` and
                        // falls back to `getDefaultJointType(state)`, which
                        // is ALIGNED for a horizontal front and ROLLABLE for
                        // a vertical one. The fallback is not decoration:
                        // every jigsaw whose front points up or down would
                        // otherwise be refused a rotation it is entitled to.
                        rollable: match nbt.get("joint").and_then(Nbt::as_str) {
                            Some("aligned") => false,
                            Some(_) => true,
                            None => !matches!(
                                orientation(&palette[state]).map(|(front, _)| front),
                                Some(Facing::North | Facing::South | Facing::West | Facing::East)
                            ),
                        },
                        final_state: text("final_state", "minecraft:air"),
                        placement_priority: int("placement_priority"),
                        selection_priority: int("selection_priority"),
                    }
                });
            blocks.push(TemplateBlock {
                pos,
                state,
                marker,
                jigsaw,
                has_nbt: nbt.is_some(),
                loot_table,
                sherds,
                payload,
            });
        }
        Ok(Self {
            size,
            palette,
            palettes,
            blocks,
        })
    }

    /// `StructurePlaceSettings.getRandomPalette(palettes, templatePosition)`.
    ///
    /// From the bytecode, and the second half is the part a guess gets wrong:
    ///
    /// ```text
    ///   getRandomPalette(list, pos):  list.get(getRandom(pos).nextInt(list.size()))
    ///   getRandom(pos):               this.random != null ? this.random
    ///                                 : RandomSource.create(Mth.getSeed(pos))
    /// ```
    ///
    /// It is **`Mth.getSeed`**, not `BlockPos.asLong` — the same position hash
    /// [`position_seed`] already carries for the structure processors — and
    /// `RandomSource.create(long)` is a `LegacyRandomSource`. So the variant is
    /// a pure function of the piece's final `templatePosition` and spends
    /// nothing from any shared stream: it cannot move a feature or another
    /// structure, and it must be asked *after* any height adjustment, because
    /// the y is in the hash.
    ///
    /// A single-palette template still goes through `nextInt(1)`, which is 0,
    /// so this is the right call for every template rather than a special case.
    #[must_use]
    pub fn choose_palette(&self, template_position: (i32, i32, i32)) -> &[String] {
        use crate::rng::RandomSource;
        let mut random = crate::rng::legacy::LegacyRandomSource::new(position_seed(
            template_position.0,
            template_position.1,
            template_position.2,
        ));
        let i = random.next_i32_bound(
            i32::try_from(self.palettes.len()).expect("a palette count fits an i32"),
        );
        &self.palettes[i as usize]
    }

    /// Every block this template would write, as `(world position, state)`,
    /// for the **identity** transform.
    ///
    /// `StructureTemplate.placeInWorld` maps each block through
    /// `calculateRelativePosition(settings, pos).offset(origin)`, and with no
    /// rotation and no mirror that reduces to a translation. Rotation and
    /// mirroring are deferred rather than guessed: they change the origin
    /// convention as well as the coordinates, and a wrong convention would put
    /// a whole structure one block out in a way only a server diff would show.
    ///
    /// **Nothing is filtered here, deliberately.** Vanilla removes
    /// `structure_void` and `jigsaw` blocks with
    /// `BlockIgnoreProcessor.STRUCTURE_BLOCK`, which is a *processor* attached
    /// by the caller — not something the template does to itself. Filtering
    /// here would bake one caller's processor list into the loader and hide
    /// which structures actually use it, so the caller gets everything and
    /// decides.
    pub fn blocks_at(
        &self,
        origin: (i32, i32, i32),
    ) -> impl Iterator<Item = ((i32, i32, i32), &str)> {
        self.blocks.iter().map(move |b| {
            (
                (origin.0 + b.pos.0, origin.1 + b.pos.1, origin.2 + b.pos.2),
                self.palette[b.state].as_str(),
            )
        })
    }

    /// Every block this template would write under a rotation and mirror.
    ///
    /// `placeInWorld` is `calculateRelativePosition(settings, pos)` then
    /// `.offset(origin)`, and the block's own state goes through
    /// `state.rotate(rotation)` — so both halves apply, the position and the
    /// block.
    ///
    /// The **pivot is a parameter** rather than assumed.
    /// `StructurePlaceSettings` stores one and ocean ruins set it; rotating
    /// about the wrong point translates an entire structure, which is a
    /// difference only a server diff would show. Passing it in keeps that
    /// decision at the call site, where the structure's own configuration can
    /// answer it, instead of hard-coding a guess here.
    pub fn blocks_rotated(
        &self,
        origin: (i32, i32, i32),
        mirror: Mirror,
        rotation: Rotation,
        pivot: (i32, i32),
    ) -> impl Iterator<Item = ((i32, i32, i32), String)> + '_ {
        self.blocks.iter().map(move |b| {
            let (x, y, z) = transform(b.pos, mirror, rotation, pivot);
            (
                (origin.0 + x, origin.1 + y, origin.2 + z),
                rotate_state(&self.palette[b.state], rotation),
            )
        })
    }

    /// `StructureTemplate.getJigsaws(pos, rotation)`.
    ///
    /// # The order is the whole point
    ///
    /// The list this returns is what `Util.shuffle` is handed, so its order
    /// decides every draw the assembler makes afterwards. Two rules produce
    /// it, and both come from `StructureTemplate.loadPalette` rather than
    /// from the file:
    ///
    ///   * every jigsaw block carries an `nbt` compound, so `addToLists` puts
    ///     all of them in the third bucket — the one appended last;
    ///   * `buildInfoList` sorts each bucket by **y, then x, then z**.
    ///
    /// So the jigsaws come out sorted by `(y, x, z)`, never in the file's
    /// order. The file's order looks plausible and is wrong: the four
    /// `savanna_meeting_point_4` bottom jigsaws are already y-major in the
    /// file, which is exactly the sort of coincidence that hides this.
    #[must_use]
    pub fn jigsaws(&self, origin: (i32, i32, i32), rotation: Rotation) -> Vec<JigsawBlock> {
        let mut raw: Vec<&TemplateBlock> =
            self.blocks.iter().filter(|b| b.jigsaw.is_some()).collect();
        raw.sort_by_key(|b| (b.pos.1, b.pos.0, b.pos.2));
        raw.into_iter()
            .map(|b| {
                // `calculateRelativePosition(settings, pos).offset(origin)`,
                // and `getJigsaws` leaves the settings' pivot at ZERO.
                let (x, y, z) = transform(b.pos, Mirror::None, rotation, (0, 0));
                let state = rotate_state(&self.palette[b.state], rotation);
                let (front, top) = orientation(&state).unwrap_or((Facing::Up, Facing::North));
                JigsawBlock {
                    pos: (origin.0 + x, origin.1 + y, origin.2 + z),
                    front,
                    top,
                    nbt: b.jigsaw.clone().expect("filtered to jigsaw blocks"),
                    state,
                }
            })
            .collect()
    }

    /// `StructureTemplate.getBoundingBox(offset, rotation, pivot, mirror)`.
    ///
    /// The two corners are the transformed `(0,0,0)` and the transformed
    /// `size - (1,1,1)`, taken as a *pair of corners* rather than as a min and
    /// a max — under a quarter turn the second one is not the larger of the
    /// two on either horizontal axis.
    #[must_use]
    pub fn bounding_box(
        &self,
        origin: (i32, i32, i32),
        rotation: Rotation,
    ) -> crate::structure::pieces::BoundingBox {
        self.bounding_box_with_pivot(origin, rotation, (0, 0))
    }

    /// [`Template::bounding_box`] for a piece whose settings carry a rotation
    /// pivot.
    ///
    /// `getBoundingBox` passes `settings.getRotationPivot()` into the same
    /// `transform` the block walk uses, so a piece with a pivot has its box
    /// shifted exactly as its blocks are. Every End city and village piece
    /// leaves the pivot at the origin, which is why this was folded into the
    /// no-pivot call until a shipwreck needed it; under `Rotation::None` the
    /// pivot cancels, so a window with no rotated piece cannot tell the two
    /// apart.
    #[must_use]
    pub fn bounding_box_with_pivot(
        &self,
        origin: (i32, i32, i32),
        rotation: Rotation,
        pivot: (i32, i32),
    ) -> crate::structure::pieces::BoundingBox {
        self.bounding_box_mirrored(origin, rotation, pivot, Mirror::None)
    }

    /// `StructureTemplate.calculateBoundingBox` in full — the four-argument
    /// form that also takes a mirror.
    ///
    /// `calculateBoundingBox` transforms the two extreme corners
    /// (`BlockPos.ZERO` and `ZERO.offset(size - 1)`) through the *same*
    /// `transform` the block walk uses, then `fromCorners` and `move(pos)`.
    /// Because the mirror is applied before the rotation and the pivot only
    /// enters the rotation, a mirrored box is not the unmirrored one shifted:
    /// `FRONT_BACK` about a pivot of `size.x / 2` sends the box to the other
    /// side of the pivot, and a ruined portal is placed at the **centre** of
    /// this box, so getting it wrong moves the whole structure.
    #[must_use]
    pub fn bounding_box_mirrored(
        &self,
        origin: (i32, i32, i32),
        rotation: Rotation,
        pivot: (i32, i32),
        mirror: Mirror,
    ) -> crate::structure::pieces::BoundingBox {
        let far = (self.size.0 - 1, self.size.1 - 1, self.size.2 - 1);
        let a = transform((0, 0, 0), mirror, rotation, pivot);
        let b = transform(far, mirror, rotation, pivot);
        crate::structure::pieces::BoundingBox::new(
            origin.0 + a.0.min(b.0),
            origin.1 + a.1.min(b.1),
            origin.2 + a.2.min(b.2),
            origin.0 + a.0.max(b.0),
            origin.1 + a.1.max(b.1),
            origin.2 + a.2.max(b.2),
        )
    }

    /// The block at a template-local position, if the template places one.
    ///
    /// Templates are sparse: a position with no entry is left alone rather
    /// than set to air, which is why this returns an option instead of a
    /// default.
    #[must_use]
    pub fn block_at(&self, pos: (i32, i32, i32)) -> Option<&str> {
        self.blocks
            .iter()
            .find(|b| b.pos == pos)
            .map(|b| self.palette[b.state].as_str())
    }
}

#[cfg(test)]
mod tests {
    /// The ancient city's CLOCKWISE_180 city centre, read off vanilla's own
    /// blocks at seed 12345 (513,-49,583) and (487,-46,544): the template's
    /// wire and skull are what our unturned placement wrote, and the turned
    /// ones are vanilla's.
    #[test]
    fn a_wire_and_a_skull_turn_with_the_template() {
        let wire = "minecraft:redstone_wire[east=side,north=side,power=14,south=none,west=none]";
        assert_eq!(
            rotate_state(wire, Rotation::Clockwise180),
            "minecraft:redstone_wire[east=none,north=none,power=14,south=side,west=side]"
        );
        assert_eq!(
            rotate_state(wire, Rotation::Clockwise90),
            "minecraft:redstone_wire[east=side,north=none,power=14,south=side,west=none]"
        );
        assert_eq!(
            mirror_state(wire, Mirror::LeftRight),
            "minecraft:redstone_wire[east=side,north=none,power=14,south=side,west=none]"
        );
        let skull = "minecraft:skeleton_skull[powered=false,rotation=14]";
        assert_eq!(
            rotate_state(skull, Rotation::Clockwise180),
            "minecraft:skeleton_skull[powered=false,rotation=6]"
        );
        assert_eq!(
            rotate_state(skull, Rotation::Clockwise90),
            "minecraft:skeleton_skull[powered=false,rotation=2]"
        );
        assert_eq!(
            rotate_state(skull, Rotation::Counterclockwise90),
            "minecraft:skeleton_skull[powered=false,rotation=10]"
        );
        // `Mirror.mirror(14, 16)`: 14 > 8 so j = -2; LEFT_RIGHT is
        // (8 + 2 + 16) % 16 = 10, FRONT_BACK is (16 + 2) % 16 = 2.
        assert_eq!(
            mirror_state(skull, Mirror::LeftRight),
            "minecraft:skeleton_skull[powered=false,rotation=10]"
        );
        assert_eq!(
            mirror_state(skull, Mirror::FrontBack),
            "minecraft:skeleton_skull[powered=false,rotation=2]"
        );
    }

    /// `getOrCreate`'s stand-in: no jigsaws, so it can never attach, and a
    /// box that is `fromCorners(size - 1, 0)` of a zero size.
    #[test]
    fn the_empty_template_has_no_jigsaws_and_a_one_corner_box() {
        let t = Template::empty();
        assert!(t.jigsaws((10, 20, 30), Rotation::Clockwise90).is_empty());
        let b = t.bounding_box((10, 20, 30), Rotation::None);
        assert_eq!(
            (b.min_x, b.min_y, b.min_z, b.max_x, b.max_y, b.max_z),
            (9, 19, 29, 10, 20, 30)
        );
    }

    #[test]
    fn a_rail_turns_and_reflects_its_shape() {
        let r = "minecraft:rail[shape=north_south,waterlogged=false]";
        assert_eq!(
            rotate_state(r, Rotation::Clockwise90),
            "minecraft:rail[shape=east_west,waterlogged=false]"
        );
        let c = "minecraft:rail[shape=south_east,waterlogged=false]";
        assert_eq!(
            rotate_state(c, Rotation::Clockwise90),
            "minecraft:rail[shape=south_west,waterlogged=false]"
        );
        assert_eq!(
            rotate_state(c, Rotation::Clockwise180),
            "minecraft:rail[shape=north_west,waterlogged=false]"
        );
        assert_eq!(
            rotate_state(c, Rotation::Counterclockwise90),
            "minecraft:rail[shape=north_east,waterlogged=false]"
        );
        assert_eq!(
            mirror_state(c, Mirror::LeftRight),
            "minecraft:rail[shape=north_east,waterlogged=false]"
        );
        assert_eq!(
            mirror_state(c, Mirror::FrontBack),
            "minecraft:rail[shape=south_west,waterlogged=false]"
        );
        let a = "minecraft:powered_rail[powered=false,shape=ascending_east,waterlogged=false]";
        assert_eq!(
            rotate_state(a, Rotation::Clockwise90),
            "minecraft:powered_rail[powered=false,shape=ascending_south,waterlogged=false]"
        );
        // Stairs carry a `shape` too, and it is not a rail's.
        let st =
            "minecraft:oak_stairs[facing=north,half=bottom,shape=inner_left,waterlogged=false]";
        assert!(rotate_state(st, Rotation::Clockwise90).contains("shape=inner_left"));
    }

    #[test]
    fn a_vine_turns_its_faces() {
        let v = "minecraft:vine[east=false,north=true,south=false,up=false,west=false]";
        assert_eq!(
            rotate_state(v, Rotation::Clockwise90),
            "minecraft:vine[east=true,north=false,south=false,up=false,west=false]"
        );
    }

    use super::*;

    /// `FRONT_BACK` reflects the x axis, so an x-facing block flips and a
    /// z-facing one does not. Getting the axis backwards is a bug no
    /// unmirrored structure can see, and until this branch nothing this port
    /// built was ever mirrored.
    #[test]
    fn front_back_flips_only_the_x_axis_and_left_right_only_the_z() {
        let chest = "minecraft:chest[facing=west,type=single,waterlogged=false]";
        assert_eq!(
            mirror_state(chest, Mirror::FrontBack),
            "minecraft:chest[facing=east,type=single,waterlogged=false]"
        );
        assert_eq!(
            mirror_state(chest, Mirror::LeftRight),
            chest,
            "west is on the x axis, which LEFT_RIGHT does not reflect"
        );

        let north = "minecraft:chest[facing=north,type=single,waterlogged=false]";
        assert_eq!(mirror_state(north, Mirror::FrontBack), north);
        assert_eq!(
            mirror_state(north, Mirror::LeftRight),
            "minecraft:chest[facing=south,type=single,waterlogged=false]"
        );
    }

    /// `Mirror::None` is the identity, including for a state with no
    /// properties at all.
    #[test]
    fn no_mirror_changes_nothing() {
        for state in [
            "minecraft:netherrack",
            "minecraft:chest[facing=west,type=single,waterlogged=false]",
            "minecraft:jigsaw[orientation=north_up]",
        ] {
            assert_eq!(mirror_state(state, Mirror::None), state);
        }
    }

    /// **The negative half.** A reflection does not move an unsigned `axis`,
    /// and it does not touch a slab's `type`, a stair's `half`, or
    /// `waterlogged`. Mirroring those would corrupt every portal's slabs.
    #[test]
    fn a_mirror_leaves_orientation_free_properties_alone() {
        for state in [
            "minecraft:purpur_pillar[axis=x]",
            "minecraft:purpur_pillar[axis=z]",
            "minecraft:stone_brick_slab[type=bottom,waterlogged=false]",
            "minecraft:stone_brick_slab[type=top,waterlogged=true]",
        ] {
            assert_eq!(mirror_state(state, Mirror::FrontBack), state);
            assert_eq!(mirror_state(state, Mirror::LeftRight), state);
        }
    }

    /// `DoorBlock.mirror` turns the facing and cycles the hinge; a trapdoor
    /// is a `HorizontalDirectionalBlock` and keeps its other properties.
    #[test]
    fn a_mirrored_door_changes_its_hinge_and_a_trapdoor_does_not() {
        assert_eq!(
            mirror_state(
                "minecraft:iron_door[facing=east,half=lower,hinge=right,open=false,powered=false]",
                Mirror::FrontBack
            ),
            "minecraft:iron_door[facing=west,half=lower,hinge=left,open=false,powered=false]"
        );
        assert_eq!(
            mirror_state(
                "minecraft:dark_oak_door[facing=east,half=upper,hinge=left,open=false,powered=false]",
                Mirror::LeftRight
            ),
            "minecraft:dark_oak_door[facing=east,half=upper,hinge=right,open=false,powered=false]"
        );
        let trapdoor = "minecraft:oak_trapdoor[facing=north,half=top,open=false,powered=false,waterlogged=false]";
        assert_eq!(
            mirror_state(trapdoor, Mirror::LeftRight),
            trapdoor.replace("north", "south")
        );
    }

    /// `JigsawBlock.mirror` sends `ORIENTATION` through an `OctahedralGroup`
    /// that inverts one axis, which is the same flip applied to each half of
    /// the `FrontAndTop` pair independently. `north_up` is a fixed point of
    /// `FRONT_BACK` — both halves are off the x axis — and portal_3's only
    /// jigsaw block carries exactly that, so a wrong rule here would be
    /// invisible in the measured window and wrong elsewhere.
    #[test]
    fn a_jigsaw_orientation_mirrors_each_half_independently() {
        assert_eq!(
            mirror_state("minecraft:jigsaw[orientation=north_up]", Mirror::FrontBack),
            "minecraft:jigsaw[orientation=north_up]"
        );
        assert_eq!(
            mirror_state("minecraft:jigsaw[orientation=east_up]", Mirror::FrontBack),
            "minecraft:jigsaw[orientation=west_up]"
        );
        assert_eq!(
            mirror_state("minecraft:jigsaw[orientation=up_east]", Mirror::FrontBack),
            "minecraft:jigsaw[orientation=up_west]"
        );
        assert_eq!(
            mirror_state("minecraft:jigsaw[orientation=north_up]", Mirror::LeftRight),
            "minecraft:jigsaw[orientation=south_up]"
        );
    }

    /// `CrossCollisionBlock.mirror` swaps the pair on the reflected axis and
    /// leaves the other pair alone. The four booleans permute **together**,
    /// so this reads from the source string exactly as the rotation does.
    #[test]
    fn a_cross_collision_block_swaps_the_pair_on_the_mirrored_axis() {
        let pane =
            "minecraft:glass_pane[east=true,north=false,south=true,waterlogged=false,west=false]";
        assert_eq!(
            mirror_state(pane, Mirror::FrontBack),
            "minecraft:glass_pane[east=false,north=false,south=true,waterlogged=false,west=true]",
            "FRONT_BACK swaps east and west only"
        );
        assert_eq!(
            mirror_state(pane, Mirror::LeftRight),
            "minecraft:glass_pane[east=true,north=true,south=false,waterlogged=false,west=false]",
            "LEFT_RIGHT swaps north and south only"
        );
    }

    /// `VineBlock` is not a `CrossCollisionBlock`, but its own `mirror` (read
    /// off the 26.2 jar: `LEFT_RIGHT` sets `north` from `south` and back,
    /// `FRONT_BACK` sets `east` from `west` and back) is the same swap. This
    /// test asserted the opposite until the woodland mansion's mirrored rooms
    /// put vines on the wire facing the unreflected wall.
    #[test]
    fn a_vine_reflects_its_faces_like_a_pane() {
        let vine = "minecraft:vine[east=true,north=false,south=false,up=false,west=false]";
        assert_eq!(
            mirror_state(vine, Mirror::FrontBack),
            "minecraft:vine[east=false,north=false,south=false,up=false,west=true]"
        );
        assert_eq!(mirror_state(vine, Mirror::LeftRight), vine);
    }

    /// A mirrored box is not the unmirrored one shifted. A ruined portal is
    /// placed at the **centre** of this box, so a box that ignored the mirror
    /// would move the whole structure.
    #[test]
    fn the_bounding_box_reflects_about_the_pivot() {
        // portal_3's size. Only the size participates in the box.
        let t = Template {
            size: (8, 9, 9),
            palette: Vec::new(),
            palettes: Vec::new(),
            blocks: Vec::new(),
        };
        let plain = t.bounding_box_mirrored((0, 0, 0), Rotation::None, (4, 4), Mirror::None);
        let flipped = t.bounding_box_mirrored((0, 0, 0), Rotation::None, (4, 4), Mirror::FrontBack);
        assert_eq!((plain.min_x, plain.max_x), (0, 7));
        assert_eq!(
            (flipped.min_x, flipped.max_x),
            (-7, 0),
            "FRONT_BACK sends x to -x before the rotation, and the pivot only enters the rotation"
        );
        assert_eq!((plain.min_z, plain.max_z), (flipped.min_z, flipped.max_z));
    }

    /// An absent `sherds` and an empty one are the same saved tag, so the
    /// parser must not tell them apart.
    ///
    /// # What would make this go red
    ///
    /// `DecoratedPotBlockEntity.saveAdditional` writes the key only when
    /// `decorations != PotDecorations.EMPTY`, so a template carrying
    /// `sherds: []` saves no `sherds` at all. Keeping the empty list would put
    /// a key in the payload that vanilla does not write — a payload wrong on
    /// one key in the OTHER direction from the eight
    /// `docs/DETERMINISM.md` §44.8 measured, and exactly as bad. Drop the
    /// `filter` in the parser and this reddens on the empty case.
    #[test]
    fn an_empty_sherds_list_is_recorded_as_no_sherds_at_all() {
        use std::collections::BTreeMap;

        fn compound(pairs: Vec<(&str, Nbt)>) -> Nbt {
            Nbt::Compound(
                pairs
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), v))
                    .collect::<BTreeMap<_, _>>(),
            )
        }
        // One 1x1x1 template holding one decorated pot with the given tag.
        let block = |tag: Nbt| -> TemplateBlock {
            let root = compound(vec![
                (
                    "size",
                    Nbt::List(vec![Nbt::Int(1), Nbt::Int(1), Nbt::Int(1)]),
                ),
                (
                    "palette",
                    Nbt::List(vec![compound(vec![(
                        "Name",
                        Nbt::String("minecraft:decorated_pot".to_string()),
                    )])]),
                ),
                (
                    "blocks",
                    Nbt::List(vec![compound(vec![
                        (
                            "pos",
                            Nbt::List(vec![Nbt::Int(0), Nbt::Int(0), Nbt::Int(0)]),
                        ),
                        ("state", Nbt::Int(0)),
                        ("nbt", tag),
                    ])]),
                ),
            ]);
            Template::from_nbt(&root)
                .expect("template loads")
                .blocks
                .into_iter()
                .next()
                .expect("one block")
        };

        let sherd = |name: &str| Nbt::String(name.to_string());
        let four = block(compound(vec![(
            "sherds",
            Nbt::List(vec![
                sherd("minecraft:brick"),
                sherd("minecraft:brick"),
                sherd("minecraft:brick"),
                sherd("minecraft:angler_pottery_sherd"),
            ]),
        )]));
        match four.sherds.as_ref().expect("a non-empty sherds is kept") {
            Nbt::List(items) => assert_eq!(items.len(), 4),
            other => panic!("sherds should be a list, got {other:?}"),
        }

        // Empty list and absent key are indistinguishable in the save, so they
        // must be indistinguishable here.
        assert!(
            block(compound(vec![("sherds", Nbt::List(Vec::new()))]))
                .sherds
                .is_none(),
            "an empty sherds list must record as no sherds"
        );
        assert!(
            block(compound(vec![(
                "LootTable",
                sherd("minecraft:chests/trial_chambers/reward"),
            )]))
            .sherds
            .is_none(),
            "a tag with no sherds key must record as no sherds"
        );

        // The two keys are independent: a pot can carry sherds and no table.
        assert!(four.loot_table.is_none());
        assert!(four.has_nbt);
    }
}
