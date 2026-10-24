//! The post-processing shape pass.
//!
//! Reference (26.2, behaviour ported — never transcribed, CLAUDE.md rule 4):
//! `StructurePiece#placeBlock`'s `markPosForPostprocessing`,
//! `Block#updateFromNeighbourShapes`, `CrossCollisionBlock#updateShape`,
//! `FenceBlock#connectsTo`, `DoorBlock#updateShape`/`canSurvive`,
//! `StairBlock#updateShape`, `VegetationBlock#updateShape`,
//! `SeagrassBlock#mayPlaceOn`.
//!
//! # Why a structure needs a second look at its own blocks
//!
//! A mineshaft support sets its fence to `west=true` when it places it,
//! before the block to the west exists — vanilla does the same, and it is
//! not a mistake. It is corrected afterwards: every block written by a
//! structure whose type is in `SHAPE_CHECK_BLOCKS` is *marked*, and a later
//! pass re-derives it from its six neighbours. A fence against a bare cave
//! wall loses the connection it was placed with.
//!
//! Skipping the pass leaves the block in the right place with the wrong
//! state, which a block-name diff cannot see at all — it took a differ that
//! compares full states to find these, and they were 144 blocks at one seed
//! and zero at another.
//!
//! # Four families, each because a measurement asked for it
//!
//! Fences, stairs, doors and seagrass. `SHAPE_CHECK_BLOCKS` also holds `cave_air`,
//! `torch`, `wall_torch`, `ladder` and `iron_bars`. None of them appears in
//! either seed's diff, so none of them has a number attached, and a shape rule
//! written without one is a rule nobody can check. They are listed in
//! [`super::super::structure::writer::needs_shape_check`] so the marks exist;
//! the rules arrive when a measurement asks for them.
//!
//! Note that `SHAPE_CHECK_BLOCKS` contains neither doors nor seagrass, so
//! [`run`] — the marked-position pass — can never see either. Both are reached
//! only through `placeInWorld`'s own two loops, and the only caller that opts
//! into those today is the shipwreck.
//!
//! # Two entry points, and they are different functions
//!
//! [`update_from_neighbour_shapes`] is `Block#updateFromNeighbourShapes`: the
//! fold over all six directions, which is what the per-position loop wants.
//! [`update_shape`] is `BlockState#updateShape` for **one** direction, which is
//! what [`crate::structure::place`]'s `updateShapeAtEdge` walk wants, because a
//! face has a direction and its far side is a block the structure never wrote.
//! Seagrass is only ever reached through the second: it is the family that
//! exists *outside* the placed volume.

use crate::canvas::Canvas;
use crate::tags::BlockTags;

/// What the pass needs to know about a neighbouring block.
pub trait ShapeFacts {
    /// `isFaceSturdy(direction, SupportType.FULL)` — `direction` indexes
    /// `Direction.values()`.
    fn face_is_sturdy(&self, state: &str, direction: usize) -> Option<bool>;
    /// `Block.isFaceFull(state.getCollisionShape(..), direction)`.
    ///
    /// A **different** question from [`Self::face_is_sturdy`], which asks the
    /// block-support shape. `MultifaceBlock.canAttachTo` is the OR of the two
    /// and needs both: leaves have no sturdy face and a full collision face on
    /// all six, which is exactly why glow lichen grows on them.
    fn face_is_collision_full(&self, state: &str, direction: usize) -> Option<bool>;
    /// `BlockState.isSolid()`. `None` where this world has no table.
    fn is_solid(&self, _state: &str) -> Option<bool> {
        None
    }
}

/// `MultifaceBlock`'s three registry entries in 26.2.
///
/// Read off the jar rather than guessed. `BlockTypes` registers `glow_lichen`
/// with `GlowLichenBlock.CODEC`, `sculk_vein` with `SculkVeinBlock.CODEC` and
/// a plain `multiface` type with `MultifaceBlock.CODEC`; the two named classes
/// are the only subclasses of `MultifaceBlock` in the jar and neither
/// overrides `updateShape`. `resin_clump` is the only other block in the
/// generated block report carrying the full `down/east/north/south/up/west`
/// **plus** `waterlogged` signature, so it is the plain type's one member.
/// `brown_mushroom_block`, `red_mushroom_block`, `mushroom_stem` and
/// `chorus_plant` have the six faces and no `waterlogged`, and are
/// `HugeMushroomBlock` and `ChorusPlantBlock` — a name test on the six
/// properties alone would wrongly sweep all four in.
const MULTIFACE: [&str; 3] = [
    "minecraft:glow_lichen",
    "minecraft:sculk_vein",
    "minecraft:resin_clump",
];

/// `MultifaceBlock.PROPERTY_BY_DIRECTION`, indexed by `Direction.values()`.
const FACE_PROPERTY: [&str; 6] = ["down", "up", "north", "south", "west", "east"];

/// `BlockBehaviour.UPDATE_SHAPE_ORDER`, from its static initialiser.
const UPDATE_SHAPE_ORDER: [usize; 6] = [4, 5, 2, 3, 0, 1];

/// `MultifaceBlock#hasAnyFace`.
fn has_any_face(state: &str) -> bool {
    FACE_PROPERTY
        .iter()
        .any(|p| property_of(state, p) == Some("true"))
}

/// `MultifaceBlock#canAttachTo(level, direction, pos, state)` — the **OR** of
/// the support shape and the collision shape, each asked about the face
/// opposite the direction the lichen looks in.
///
/// `None` from either fact source is `false`, which is the refusing answer:
/// a face this port cannot judge is dropped rather than kept.
fn multiface_can_attach_to(facts: &dyn ShapeFacts, neighbour: &str, direction: usize) -> bool {
    let back = opposite(direction);
    facts.face_is_sturdy(neighbour, back).unwrap_or(false)
        || facts
            .face_is_collision_full(neighbour, back)
            .unwrap_or(false)
}

/// `MultifaceBlock#updateShape` for one direction.
///
/// ```text
///   if (waterlogged) scheduleTick(WATER)          not ported: no block change
///   if (!hasAnyFace(state)) return AIR
///   if (!hasFace(state, dir)) return state
///   if (canAttachTo(level, dir, npos, nstate)) return state
///   return removeFace(state, getFaceProperty(dir))
/// ```
///
/// and `removeFace` clears the one property and returns **AIR** if that was
/// the last face standing. That last line is the whole of cluster E: vanilla's
/// two disputed positions hold a plain `minecraft:air` in a field of
/// `cave_air`, which is what a multiface block looks like after the pass has
/// taken its final face away.
fn multiface_shape(
    state: &str,
    direction: usize,
    neighbour: &str,
    facts: &dyn ShapeFacts,
) -> String {
    const AIR: &str = "minecraft:air";
    if !has_any_face(state) {
        return AIR.to_string();
    }
    if property_of(state, FACE_PROPERTY[direction]) != Some("true") {
        return state.to_string();
    }
    if multiface_can_attach_to(facts, neighbour, direction) {
        return state.to_string();
    }
    let removed = with_property(state, FACE_PROPERTY[direction], "false");
    if has_any_face(&removed) {
        removed
    } else {
        AIR.to_string()
    }
}

fn plain(state: &str) -> &str {
    state.split_once('[').map_or(state, |(name, _)| name)
}

/// Tag membership, so the shape rules can be run by a caller that holds a
/// [`BlockTags`] and by one — [`crate::structure::place::place_piece`] — that
/// only has a `PieceWorld` to ask.
pub trait TagFacts {
    /// `BlockState#is(TagKey)`, on the block name.
    fn contains(&self, tag: &str, block: &str) -> bool;
}

impl TagFacts for BlockTags {
    fn contains(&self, tag: &str, block: &str) -> bool {
        Self::contains(self, tag, block)
    }
}

/// `Direction.values()` offsets: DOWN, UP, NORTH, SOUTH, WEST, EAST. The
/// index **is** the `Direction` ordinal, which is also how `block_props.json`
/// orders `sturdyFaces`, so one number indexes both.
pub const OFFSET: [(i32, i32, i32); 6] = [
    (0, -1, 0),
    (0, 1, 0),
    (0, 0, -1),
    (0, 0, 1),
    (-1, 0, 0),
    (1, 0, 0),
];

/// The four horizontal connection properties against their `Direction` index.
const HORIZONTAL: [(&str, usize); 4] = [("north", 2), ("south", 3), ("west", 4), ("east", 5)];

/// `ChestBlock.getConnectedDirection`: `LEFT` is the facing's clockwise
/// neighbour, anything else (`RIGHT`, and `SINGLE` too) its counter-clockwise
/// one. Direction indices are this file's: north 2, south 3, west 4, east 5.
fn chest_connected_direction(facing: &str, kind: &str) -> Option<usize> {
    let (clockwise, counter) = match facing {
        "north" => (5, 4),
        "east" => (3, 2),
        "south" => (4, 5),
        "west" => (2, 3),
        _ => return None,
    };
    Some(if kind == "left" { clockwise } else { counter })
}

/// `Direction#getOpposite`.
pub const fn opposite(direction: usize) -> usize {
    match direction {
        0 => 1,
        1 => 0,
        2 => 3,
        3 => 2,
        4 => 5,
        _ => 4,
    }
}

/// `Direction.Axis` equality. The ordinals pair up (DOWN/UP, NORTH/SOUTH,
/// WEST/EAST), so setting the low bit collapses each pair to one number.
const fn same_axis(a: usize, b: usize) -> bool {
    (a | 1) == (b | 1)
}

/// `Direction#getCounterClockWise` about the Y axis: NORTH -> WEST -> SOUTH
/// -> EAST -> NORTH.
const fn counter_clockwise(direction: usize) -> usize {
    match direction {
        2 => 4,
        4 => 3,
        3 => 5,
        5 => 2,
        other => other,
    }
}

/// A horizontal `Direction` from the name a `facing` property spells.
fn facing_index(name: &str) -> Option<usize> {
    match name {
        "north" => Some(2),
        "south" => Some(3),
        "west" => Some(4),
        "east" => Some(5),
        _ => None,
    }
}

/// One property's value out of a full state string.
fn property_of<'a>(state: &'a str, key: &str) -> Option<&'a str> {
    let (_, rest) = state.split_once('[')?;
    rest.trim_end_matches(']').split(',').find_map(|pair| {
        pair.split_once('=')
            .filter(|(k, _)| *k == key)
            .map(|(_, v)| v)
    })
}

/// `Block#isExceptionForConnection` — blocks a fence refuses to reach for
/// even when their face is sturdy.
fn is_exception_for_connection(tags: &dyn TagFacts, block: &str) -> bool {
    tags.contains("minecraft:leaves", block)
        || matches!(
            block,
            "minecraft:barrier"
                | "minecraft:carved_pumpkin"
                | "minecraft:jack_o_lantern"
                | "minecraft:melon"
                | "minecraft:pumpkin"
                | "minecraft:shulker_box"
        )
        || block.ends_with("_shulker_box")
}

/// `FenceBlock#connectsTo`.
fn connects_to(tags: &dyn TagFacts, self_block: &str, neighbour: &str, side_solid: bool) -> bool {
    let neighbour_block = plain(neighbour);
    // `isSameFence`: a fence, and wooden-ness must match — a wooden fence
    // does not join a nether brick fence.
    let same_fence = tags.contains("minecraft:fences", neighbour_block)
        && tags.contains("minecraft:wooden_fences", neighbour_block)
            == tags.contains("minecraft:wooden_fences", self_block);
    (!is_exception_for_connection(tags, neighbour_block) && side_solid) || same_fence
}

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

/// `block instanceof IronBarsBlock`: a `CrossCollisionBlock` that is not a
/// fence.
/// The four wall sides in `WallBlock.updateSides`' order — north, east,
/// south, west — with their `Direction` index.
const WALL_SIDES: [(&str, usize); 4] = [("north", 2), ("east", 5), ("south", 3), ("west", 4)];

/// `TEST_SHAPES_WALL`, per side of [`WALL_SIDES`], as `(x0, z0, x1, z1)` in
/// sixteenths: `Shapes.rotateHorizontal(Block.boxZ(2, 16, 0, 9))`.
const WALL_TEST: [(i32, i32, i32, i32); 4] =
    [(7, 0, 9, 9), (7, 7, 16, 9), (7, 7, 9, 16), (0, 7, 9, 9)];

/// `TEST_SHAPE_POST`, `Block.column(2, 0, 16)`.
const WALL_POST_TEST: (i32, i32, i32, i32) = (7, 7, 9, 9);

/// `WallBlock.connectsTo`: another wall, a sturdy face that is not an
/// exception, any `IronBarsBlock`, or a fence gate whose axis lies across
/// `toward`.
fn wall_connects_to(tags: &dyn TagFacts, neighbour: &str, sturdy: bool, toward: usize) -> bool {
    let block = plain(neighbour);
    let gate = tags.contains("minecraft:fence_gates", block)
        && property_of(neighbour, "facing").is_some_and(|facing| {
            // `FenceGateBlock.connectsToDirection`: the gate's facing axis is
            // the axis of `toward.getClockWise()`, the other horizontal one.
            let gate_x = matches!(facing, "east" | "west");
            let toward_z = toward == 2 || toward == 3;
            gate_x == toward_z
        });
    tags.contains("minecraft:walls", block)
        || (!is_exception_for_connection(tags, block) && sturdy)
        || is_iron_bars(tags, block)
        || gate
}

/// The bottom face of `above`'s collision shape, as rectangles in sixteenths,
/// or `None` when this port cannot say.
///
/// Answered for three kinds of block and refused for the rest: a wall (its
/// collision shape is `makeShapes(24, 24)` — the post `column(8)` if `up`,
/// and each connected side `boxZ(6, 0, 24, 0, 11)` rotated), a block whose
/// collision face is full, and air. A slab, a stair, a fence or a torch over
/// a wall is a shape this table has no row for, and the caller leaves the
/// wall as it was rather than guess.
fn collision_down_face(
    above: &str,
    tags: &dyn TagFacts,
    facts: &dyn ShapeFacts,
) -> Option<Vec<(i32, i32, i32, i32)>> {
    let block = plain(above);
    if tags.contains("minecraft:walls", block) {
        let mut rects = Vec::new();
        if property_of(above, "up") == Some("true") {
            rects.push((4, 4, 12, 12));
        }
        for ((property, _), rect) in WALL_SIDES.iter().zip([
            (5, 0, 11, 11),
            (5, 5, 16, 11),
            (5, 5, 11, 16),
            (0, 5, 11, 11),
        ]) {
            if property_of(above, property).is_some_and(|side| side != "none") {
                rects.push(rect);
            }
        }
        return Some(rects);
    }
    if facts.face_is_collision_full(above, 0) == Some(true) {
        return Some(vec![(0, 0, 16, 16)]);
    }
    if matches!(
        block,
        "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
    ) {
        return Some(Vec::new());
    }
    None
}

/// `WallBlock.isCovered`: nothing of `test` is left once the face is taken
/// away (`joinIsNotEmpty(test, face, ONLY_FIRST)` is false).
fn wall_is_covered(face: &[(i32, i32, i32, i32)], test: (i32, i32, i32, i32)) -> bool {
    (test.0..test.2).all(|x| {
        (test.1..test.3).all(|z| {
            face.iter()
                .any(|r| r.0 <= x && x < r.2 && r.1 <= z && z < r.3)
        })
    })
}

/// `WallBlock`'s private `updateShape(level, state, abovePos, above, n, e, s,
/// w)`: `updateSides` against the face of the block above, then `UP` from
/// `shouldRaisePost`. `None` when the face is one [`collision_down_face`]
/// cannot answer and the answer would have read it.
fn wall_recompute(
    state: &str,
    connected: [bool; 4],
    above: &str,
    tags: &dyn TagFacts,
    facts: &dyn ShapeFacts,
) -> Option<String> {
    let face = collision_down_face(above, tags, facts);
    let mut sides = ["none"; 4];
    for i in 0..4 {
        if connected[i] {
            sides[i] = if wall_is_covered(face.as_deref()?, WALL_TEST[i]) {
                "tall"
            } else {
                "low"
            };
        }
    }
    let mut updated = state.to_string();
    for (i, (property, _)) in WALL_SIDES.iter().enumerate() {
        updated = with_property(&updated, property, sides[i]);
    }
    // `shouldRaisePost`.
    let [n, e, s, w] = sides;
    // The first two arms of vanilla's chain both answer true: a wall post
    // above, or a side pattern that is not a straight run.
    let raise = if (tags.contains("minecraft:walls", plain(above))
        && property_of(above, "up") == Some("true"))
        || (n == "none" && s == "none" && w == "none" && e == "none")
        || (n == "none") != (s == "none")
        || (w == "none") != (e == "none")
    {
        true
    } else if (n == "tall" && s == "tall") || (e == "tall" && w == "tall") {
        false
    } else {
        tags.contains("minecraft:wall_post_override", plain(above))
            || wall_is_covered(face.as_deref()?, WALL_POST_TEST)
    };
    Some(with_property(
        &updated,
        "up",
        if raise { "true" } else { "false" },
    ))
}

fn is_iron_bars(tags: &dyn TagFacts, block: &str) -> bool {
    crate::structure::template::is_cross_collision(block)
        && !tags.contains("minecraft:fences", block)
}

/// `StairBlock#isStairs`, which is `block instanceof StairBlock` — the
/// `minecraft:stairs` tag enumerates exactly that class.
fn is_stairs(tags: &dyn TagFacts, state: &str) -> bool {
    tags.contains("minecraft:stairs", plain(state))
}

/// `StairBlock#canTakeShape` — a stair will not corner against another stair
/// that faces the same way at the same half, because the two would already be
/// one straight run.
fn can_take_shape(tags: &dyn TagFacts, state: &str, other: &str) -> bool {
    !is_stairs(tags, other)
        || property_of(other, "facing") != property_of(state, "facing")
        || property_of(other, "half") != property_of(state, "half")
}

/// `StairBlock#getStairsShape`.
///
/// The template stores whatever shape the builder's world gave the block, and
/// vanilla throws that away and re-derives it here from the world it is being
/// placed into. That is why a shipwreck's hull arrives as `outer_right` and
/// leaves as `straight`.
fn stairs_shape(tags: &dyn TagFacts, state: &str, neighbours: &[String; 6]) -> &'static str {
    let Some(facing) = property_of(state, "facing").and_then(facing_index) else {
        return "straight";
    };
    let half = property_of(state, "half");
    let front = &neighbours[facing];
    if is_stairs(tags, front) && property_of(front, "half") == half {
        if let Some(ahead) = property_of(front, "facing").and_then(facing_index) {
            if !same_axis(ahead, facing)
                && can_take_shape(tags, state, &neighbours[opposite(ahead)])
            {
                return if ahead == counter_clockwise(facing) {
                    "outer_left"
                } else {
                    "outer_right"
                };
            }
        }
    }
    let behind = &neighbours[opposite(facing)];
    if is_stairs(tags, behind) && property_of(behind, "half") == half {
        if let Some(back) = property_of(behind, "facing").and_then(facing_index) {
            if !same_axis(back, facing) && can_take_shape(tags, state, &neighbours[back]) {
                return if back == counter_clockwise(facing) {
                    "inner_left"
                } else {
                    "inner_right"
                };
            }
        }
    }
    "straight"
}

/// `DoorBlock#updateShape`.
///
/// A door is two blocks that must agree, and vanilla makes them agree here
/// rather than at placement. The rule is not "copy four properties across":
/// it returns the **whole neighbouring state** with `half` swapped to this
/// block's own, so a lower half under an oak upper becomes an oak lower.
///
/// ```text
///   DoubleBlockHalf half = state.getValue(HALF);
///   if (direction.getAxis() == Axis.Y && (half == LOWER) == (direction == UP)) {
///       return neighborState.getBlock() instanceof DoorBlock
///               && neighborState.getValue(HALF) != half
///           ? neighborState.setValue(HALF, half)
///           : Blocks.AIR.defaultBlockState();
///   }
///   if (half == LOWER && direction == DOWN && !state.canSurvive(level, pos))
///       return Blocks.AIR.defaultBlockState();
///   return super.updateShape(...);
/// ```
///
/// **This is why it matters, and it is not a rotation bug.** Every one of the
/// eight palettes of `shipwreck/rightsideup_fronthalf` stores the two halves
/// of its one door *disagreeing* — the lower at `facing=south,hinge=right`,
/// the upper at `facing=east,hinge=left` — and the template's block list holds
/// the lower first. Vanilla writes both verbatim and then this rule pulls the
/// lower up to the upper's `facing` and `hinge`. A port without it leaves the
/// lower exactly as the file spells it, which is one block wrong at a piece
/// whose rotation is `NONE`.
///
/// **Order-free, and that is checked rather than assumed.** Only two of the
/// six directions do anything. For an upper half it is `DOWN` alone; for a
/// lower half it is `UP` (the copy) and `DOWN` (`canSurvive`), and those two
/// commute — if the block below cannot support the door the fold reaches
/// `AIR` whichever runs first, because `AIR`'s own `updateShape` is the
/// identity and because `canSurvive` for a lower half reads only the block
/// below, which the copy does not change. So `UPDATE_SHAPE_ORDER`'s order is
/// genuinely not observable here.
fn door_shape(
    state: &str,
    neighbours: &[String; 6],
    tags: &dyn TagFacts,
    facts: &dyn ShapeFacts,
) -> String {
    const AIR: &str = "minecraft:air";
    let Some(half) = property_of(state, "half") else {
        return state.to_string();
    };
    let lower = half == "lower";

    // `canSurvive`, which only the lower half asks: the block below must have
    // a sturdy up face. A `None` is "this world has no block-property table",
    // and the honest answer to that is to leave the block alone — deleting a
    // door for want of a lookup would be a worse error than keeping a wrong
    // one, and the only caller that reaches this rule can answer.
    if lower && facts.face_is_sturdy(&neighbours[0], 1) == Some(false) {
        return AIR.to_string();
    }

    // The one Y direction whose branch this half answers: `UP` for a lower
    // half, `DOWN` for an upper one. Indices are `Direction` ordinals.
    let partner = &neighbours[usize::from(lower)];
    if tags.contains("minecraft:doors", plain(partner))
        && property_of(partner, "half").is_some_and(|other| other != half)
    {
        return with_property(partner, "half", half);
    }
    // A half with no partner is destroyed. Vanilla's own answer, and it is
    // unreachable for the shipwreck: both halves sit in one column, so no
    // chunk clip can separate them.
    AIR.to_string()
}

/// `Block#updateFromNeighbourShapes`, for the block families this port has a
/// measured rule for.
///
/// `neighbours` is indexed by `Direction` ordinal — see [`OFFSET`].
///
/// Vanilla walks `UPDATE_SHAPE_ORDER` and folds each direction's
/// `BlockState#updateShape` into the state. For all three families here the
/// fold is order-free: a stair recomputes its whole `shape` from the world on
/// *any* horizontal direction and ignores the state it was handed, a fence
/// writes one property per direction from that direction's neighbour alone,
/// and a door's two live directions commute (see [`door_shape`]). So the
/// array's order is not observable here and is deliberately not claimed. A
/// family whose directions interact would have to establish it.
///
/// Seagrass is deliberately **not** here even though [`update_shape`] answers
/// it. This fold only ever sees positions the structure itself wrote, and no
/// template in the port writes seagrass; putting it in would be a rule with no
/// measurement behind it, and it would be reached by exactly nothing.
///
/// Anything else is returned unchanged — deliberately, and it is a real gap:
/// vanilla has an `updateShape` for panes, chests, vines and more. Each needs
/// a measurement before it needs a rule. Walls got theirs from the woodland
/// mansion: 394 of the 502 positions still wrong inside seed 8's (32,16)
/// mansion before this rule were wall connections and heights.
#[must_use]
pub fn update_from_neighbour_shapes(
    state: &str,
    neighbours: &[String; 6],
    tags: &dyn TagFacts,
    facts: &dyn ShapeFacts,
) -> String {
    let block = plain(state);
    if tags.contains("minecraft:fences", block) {
        let mut updated = state.to_string();
        for (property, direction) in HORIZONTAL {
            let neighbour = &neighbours[direction];
            let side_solid = facts
                .face_is_sturdy(neighbour, opposite(direction))
                .unwrap_or(false);
            let connects = connects_to(tags, block, neighbour, side_solid);
            updated = with_property(&updated, property, if connects { "true" } else { "false" });
        }
        return updated;
    }
    if is_iron_bars(tags, block) {
        // Each horizontal direction sets only its own property from only its
        // own neighbour, so the fold is the four [`update_shape`] calls.
        let mut updated = state.to_string();
        for (_, direction) in HORIZONTAL {
            updated = update_shape(
                &updated,
                direction,
                &neighbours[direction],
                neighbours,
                tags,
                facts,
            );
        }
        return updated;
    }
    if block.ends_with("_wall_banner") {
        // The fold over six directions; only the face behind the banner can
        // change it, and only to air. See [`update_shape`].
        let mut updated = state.to_string();
        for direction in UPDATE_SHAPE_ORDER {
            updated = update_shape(
                &updated,
                direction,
                &neighbours[direction],
                neighbours,
                tags,
                facts,
            );
            if plain(&updated) == "minecraft:air" {
                return updated;
            }
        }
        return updated;
    }

    if block == "minecraft:ladder" {
        // Only the direction behind the ladder can change it, and it can
        // only turn it to air; the fold is the six calls.
        let mut updated = state.to_string();
        for direction in UPDATE_SHAPE_ORDER {
            updated = update_shape(
                &updated,
                direction,
                &neighbours[direction],
                neighbours,
                tags,
                facts,
            );
            if plain(&updated) == "minecraft:air" {
                return updated;
            }
        }
        return updated;
    }
    if tags.contains("minecraft:walls", block) {
        // `Block.updateFromNeighbourShapes`, folded in `UPDATE_SHAPE_ORDER`:
        // each side update rewrites its own connection and then re-derives
        // all four heights and the post from the block above, so the fold is
        // order-bearing in principle and is reproduced rather than argued.
        let mut updated = state.to_string();
        for direction in UPDATE_SHAPE_ORDER {
            updated = update_shape(
                &updated,
                direction,
                &neighbours[direction],
                neighbours,
                tags,
                facts,
            );
        }
        return updated;
    }
    if is_stairs(tags, state) {
        return with_property(state, "shape", stairs_shape(tags, state, neighbours));
    }
    if tags.contains("minecraft:doors", block) {
        return door_shape(state, neighbours, tags, facts);
    }
    if tags.contains("minecraft:speleothems", block) {
        return speleothem_shape(state, neighbours, tags);
    }
    if MULTIFACE.contains(&block) {
        // Folded in `UPDATE_SHAPE_ORDER` for faithfulness rather than
        // necessity: each direction touches only its own face property and
        // reads only the neighbour, so the fold is order-free — but that is a
        // property of this family, not of the array, and the speleothem
        // proved the difference.
        let mut updated = state.to_string();
        for direction in UPDATE_SHAPE_ORDER {
            updated = multiface_shape(&updated, direction, &neighbours[direction], facts);
            if plain(&updated) == "minecraft:air" {
                // `AirBlock#updateShape` returns the state, so the remaining
                // directions are a no-op in vanilla too.
                return updated;
            }
        }
        return updated;
    }
    if matches!(block, "minecraft:chest" | "minecraft:trapped_chest") {
        // Folded: a half that goes single on one side can be joined from
        // another later in `UPDATE_SHAPE_ORDER`, so the order is the
        // function. See the chest arm of [`update_shape`].
        let mut updated = state.to_string();
        for direction in UPDATE_SHAPE_ORDER {
            updated = update_shape(
                &updated,
                direction,
                &neighbours[direction],
                neighbours,
                tags,
                facts,
            );
        }
        return updated;
    }
    state.to_string()
}

/// `SpeleothemBlock#updateShape`, folded over `UPDATE_SHAPE_ORDER`.
///
/// The four horizontal directions return the state untouched, so only DOWN
/// and UP do anything — but this is the **first** family here whose fold is
/// genuinely order-bearing, and `UPDATE_SHAPE_ORDER` is
/// `{WEST, EAST, NORTH, SOUTH, DOWN, UP}` (BlockBehaviour's static
/// initialiser), so DOWN runs and then UP runs on what DOWN left.
///
/// It matters because `calculateSpeleothemThickness`'s first branch reads
/// `isTipMerge` off the state it was **handed**: a tip that DOWN demotes from
/// `tip_merge` to `tip` is then asked again as a `tip`. Folding the other way,
/// or once, is a different function.
///
/// Two branches of vanilla's `updateShape` are deliberately not modelled, and
/// neither can change the resulting block:
///
/// * the `canSurvive` branch schedules a tick and returns the state as the
///   *previous* direction left it. For a stalactite that is DOWN's freshly
///   recomputed answer and for a stalagmite UP recomputes afterwards anyway,
///   so with UP last the state is the same either way. Only the tick is lost.
/// * the `hasScheduledTick` early-out applies to downward tips only and needs
///   a block-tick list, which is empty through generation — the one way to
///   fill it is that same `canSurvive` branch.
fn speleothem_shape(state: &str, neighbours: &[String; 6], tags: &dyn TagFacts) -> String {
    const DOWN: usize = 0;
    const UP: usize = 1;
    let mut updated = state.to_string();
    for direction in [DOWN, UP] {
        let _ = direction;
        let Some(tip) = property_of(&updated, "vertical_direction") else {
            return updated;
        };
        let tip_direction = if tip == "down" { DOWN } else { UP };
        let is_tip_merge = property_of(&updated, "thickness") == Some("tip_merge");
        let thickness = speleothem_thickness(
            plain(&updated),
            tip_direction,
            neighbours,
            tags,
            is_tip_merge,
        );
        updated = with_property(&updated, "thickness", thickness);
    }
    updated
}

/// `SpeleothemBlock#calculateSpeleothemThickness`.
///
/// `is_tip_merge` is vanilla's fourth argument: whether the block being asked
/// about is *currently* `tip_merge`. It only reaches the first branch, which
/// is the one that decides whether two tips meeting head-on both merge.
///
/// The `block == plain(ahead)` test is new in 26.2 and is not cosmetic: there
/// is more than one speleothem now, and a sulfur spike meeting a pointed
/// dripstone is two tips, not one merge.
fn speleothem_thickness<'a>(
    block: &str,
    tip_direction: usize,
    neighbours: &'a [String; 6],
    tags: &dyn TagFacts,
    is_tip_merge: bool,
) -> &'a str {
    let ahead = &neighbours[tip_direction];
    let behind = &neighbours[opposite(tip_direction)];

    if is_speleothem_facing(tags, ahead, opposite(tip_direction)) && plain(ahead) == block {
        return if is_tip_merge || property_of(ahead, "thickness") == Some("tip_merge") {
            "tip_merge"
        } else {
            "tip"
        };
    }
    if !is_speleothem_facing(tags, ahead, tip_direction) {
        return "tip";
    }
    match property_of(ahead, "thickness") {
        Some("tip" | "tip_merge") => "frustum",
        _ if !is_speleothem_facing(tags, behind, tip_direction) => "base",
        _ => "middle",
    }
}

/// `SpeleothemBlock#isSpeleothemWithDirection` — in the tag *and* pointing the
/// way asked. The tag is the whole membership test, so a block outside it is
/// never a speleothem here however much it looks like one.
fn is_speleothem_facing(tags: &dyn TagFacts, state: &str, direction: usize) -> bool {
    if !tags.contains("minecraft:speleothems", plain(state)) {
        return false;
    }
    let wanted = if direction == 0 { "down" } else { "up" };
    property_of(state, "vertical_direction") == Some(wanted)
}

/// `BlockState#updateShape` for **one** direction.
///
/// [`update_from_neighbour_shapes`] is the six-direction fold and is what the
/// per-position pass wants. `updateShapeAtEdge` wants the single call, because
/// it visits a *face* — one cell, one direction — and the two are not the same
/// function. A fence folded over six directions rewrites all four of its
/// connections; asked about one direction it rewrites one. On a block the
/// structure never wrote, that difference is the whole answer.
///
/// `neighbours` is the six neighbours of `state`'s own position, needed
/// because `StairBlock#updateShape` re-derives its `shape` from the world
/// rather than from the state it was handed. `neighbour` is the state across
/// `direction`, which the caller may already have updated — vanilla passes the
/// *updated* one into the second of its two calls.
#[must_use]
pub fn update_shape(
    state: &str,
    direction: usize,
    neighbour: &str,
    neighbours: &[String; 6],
    tags: &dyn TagFacts,
    facts: &dyn ShapeFacts,
) -> String {
    const AIR: &str = "minecraft:air";
    let block = plain(state);
    let horizontal = direction >= 2;

    // `MultifaceBlock#updateShape`. Reached from `updateShapeAtEdge` as well
    // as from the fold, because a lichen on the boundary of a placed volume
    // has a neighbour the structure never wrote.
    if MULTIFACE.contains(&block) {
        return multiface_shape(state, direction, neighbour, facts);
    }

    // `CrossCollisionBlock#updateShape`: one property, and only for a
    // horizontal direction. A vertical one falls through to `Block` and the
    // state is returned untouched.
    if tags.contains("minecraft:fences", block) {
        if !horizontal {
            return state.to_string();
        }
        let Some((property, _)) = HORIZONTAL.iter().find(|(_, d)| *d == direction) else {
            return state.to_string();
        };
        let side_solid = facts
            .face_is_sturdy(neighbour, opposite(direction))
            .unwrap_or(false);
        let connects = connects_to(tags, block, neighbour, side_solid);
        return with_property(state, property, if connects { "true" } else { "false" });
    }

    // `IronBarsBlock#updateShape` — iron bars, glass panes and copper bars,
    // every `CrossCollisionBlock` that is not a fence. One horizontal
    // property, and `attachsTo` differs from the fence's: a sturdy face that
    // is not an exception, **or any other IronBarsBlock, or a wall**. The
    // stronghold's prison cells and portal-room grilles were its first
    // measured users (51 positions in seed 12345's (110,23) stronghold).
    if is_iron_bars(tags, block) {
        if !horizontal {
            return state.to_string();
        }
        let Some((property, _)) = HORIZONTAL.iter().find(|(_, d)| *d == direction) else {
            return state.to_string();
        };
        let neighbour_block = plain(neighbour);
        let side_solid = facts
            .face_is_sturdy(neighbour, opposite(direction))
            .unwrap_or(false);
        let connects = (!is_exception_for_connection(tags, neighbour_block) && side_solid)
            || is_iron_bars(tags, neighbour_block)
            || tags.contains("minecraft:walls", neighbour_block);
        return with_property(state, property, if connects { "true" } else { "false" });
    }

    // `LadderBlock#updateShape`: from the block behind it (`direction` is
    // the opposite of `facing`), a ladder that cannot survive — that block's
    // `facing` face not sturdy — becomes air. Found by the woodland mansion:
    // a room's ladder whose wall lies in the next chunk south is placed
    // before that wall exists, and vanilla drops it (5 positions in seed 8's
    // (32,16) mansion).
    // `WallBannerBlock.updateShape`: from the face it hangs on
    // (`direction == facing.getOpposite()`), it becomes AIR unless
    // `canSurvive`, which is `level.getBlockState(pos.relative(facing
    // .getOpposite())).isSolid()` — the neighbour this call is handed.
    // Measured 2026-10-04: an end city at seed 12345 and one at 31415926 each
    // kept four magenta wall banners all three vanilla worlds remove.
    if block.ends_with("_wall_banner") {
        let facing = match property_of(state, "facing") {
            Some("north") => 2,
            Some("south") => 3,
            Some("west") => 4,
            Some("east") => 5,
            _ => return state.to_string(),
        };
        if direction == opposite(facing) && facts.is_solid(neighbour) == Some(false) {
            return AIR.to_string();
        }
        return state.to_string();
    }

    if block == "minecraft:ladder" {
        let facing = match property_of(state, "facing") {
            Some("north") => 2,
            Some("south") => 3,
            Some("west") => 4,
            Some("east") => 5,
            _ => return state.to_string(),
        };
        if direction == opposite(facing) && facts.face_is_sturdy(neighbour, facing) == Some(false) {
            return AIR.to_string();
        }
        return state.to_string();
    }

    // `WallBlock#updateShape`. DOWN returns the state; UP is `topUpdate`
    // against the state handed in; a side is `sideUpdate`, which recomputes
    // that one connection and reads the block above from the world.
    if tags.contains("minecraft:walls", block) {
        if direction == 0 {
            return state.to_string();
        }
        let mut connected = WALL_SIDES
            .map(|(property, _)| property_of(state, property).is_some_and(|side| side != "none"));
        let above: &str = if direction == 1 {
            neighbour
        } else {
            let toward = opposite(direction);
            let sturdy = facts.face_is_sturdy(neighbour, toward).unwrap_or(false);
            if let Some(i) = WALL_SIDES.iter().position(|(_, d)| *d == direction) {
                connected[i] = wall_connects_to(tags, neighbour, sturdy, toward);
            }
            &neighbours[1]
        };
        return wall_recompute(state, connected, above, tags, facts)
            .unwrap_or_else(|| state.to_string());
    }

    // `StairBlock#updateShape`: `direction.getAxis().isHorizontal()` gates the
    // whole thing, and the recompute reads the world, not `neighbour`.
    if is_stairs(tags, state) {
        if !horizontal {
            return state.to_string();
        }
        return with_property(state, "shape", stairs_shape(tags, state, neighbours));
    }

    // `DoorBlock#updateShape`, one direction of it. See [`door_shape`] for the
    // rule and for why the two live directions commute.
    if tags.contains("minecraft:doors", block) {
        let Some(half) = property_of(state, "half") else {
            return state.to_string();
        };
        let lower = half == "lower";
        if direction < 2 {
            if lower == (direction == 1) {
                if tags.contains("minecraft:doors", plain(neighbour))
                    && property_of(neighbour, "half").is_some_and(|other| other != half)
                {
                    return with_property(neighbour, "half", half);
                }
                return AIR.to_string();
            }
            if lower && direction == 0 && facts.face_is_sturdy(neighbour, 1) == Some(false) {
                return AIR.to_string();
            }
        }
        return state.to_string();
    }

    // `ChestBlock#updateShape`, horizontal half. `chestCanConnectTo` is
    // `state.is(this)`, so a chest pairs only with a chest and a trapped
    // chest only with a trapped chest. A single chest joins a neighbour that
    // is already one half of a pair pointing at it; a half whose connected
    // side no longer holds its kind of chest goes single. That second arm is
    // the one a template reaches: `ChestBlock.mirror` turns the facing and
    // leaves `type` alone, so a mirrored pair's halves point away from each
    // other and the shape pass splits them — a mansion's 26-chest storeroom.
    if matches!(block, "minecraft:chest" | "minecraft:trapped_chest") {
        if !horizontal {
            return state.to_string();
        }
        let (Some(facing), Some(kind)) = (property_of(state, "facing"), property_of(state, "type"))
        else {
            return state.to_string();
        };
        if plain(neighbour) == block {
            let (Some(their_facing), Some(their_kind)) = (
                property_of(neighbour, "facing"),
                property_of(neighbour, "type"),
            ) else {
                return state.to_string();
            };
            if kind == "single"
                && their_kind != "single"
                && facing == their_facing
                && chest_connected_direction(their_facing, their_kind) == Some(opposite(direction))
            {
                let flipped = if their_kind == "left" {
                    "right"
                } else {
                    "left"
                };
                return with_property(state, "type", flipped);
            }
            return state.to_string();
        }
        if chest_connected_direction(facing, kind) == Some(direction) {
            return with_property(state, "type", "single");
        }
        return state.to_string();
    }

    // `VegetationBlock#updateShape` — `!canSurvive` destroys the block, on
    // *any* direction, which is how a structure's own rail deletes the
    // seagrass that grew before the structure did.
    //
    // Only `minecraft:seagrass` is answered. `tall_seagrass` is a
    // `DoublePlantBlock` with a different `updateShape`, and every other
    // member of the family needs its own `canSurvive`; both are named gaps
    // rather than claims, and neither appears in the measured window.
    if block == "minecraft:seagrass" {
        if tags.contains("minecraft:cannot_support_seagrass", plain(&neighbours[0])) {
            return AIR.to_string();
        }
        if facts.face_is_sturdy(&neighbours[0], 1) == Some(false) {
            return AIR.to_string();
        }
        return state.to_string();
    }

    state.to_string()
}

/// The six neighbouring states of a position, in `Direction` order.
fn neighbours_of(canvas: &Canvas, x: i32, y: i32, z: i32) -> [String; 6] {
    std::array::from_fn(|i| {
        let (dx, dy, dz) = OFFSET[i];
        canvas.block_at(x + dx, y + dy, z + dz).to_string()
    })
}

/// `RUSTMC_DUMP_POST_MARKS=<path>` — every marked position and the state
/// standing on it when the pass begins, one `x y z state` per line.
///
/// It exists because a mark set is invisible in the output: a position that is
/// marked and then not acted on writes exactly what an unmarked one writes, so
/// "the marks are there" and "the marks are absent" are the same region file.
/// The port has already been caught once by a measurement that could not
/// distinguish a real zero from a broken probe, and this is the cheapest way
/// to make the set itself readable before anything acts on it.
fn dump_marks(canvas: &Canvas, marked: &[(i32, i32, i32)]) {
    let Some(path) = std::env::var_os("RUSTMC_DUMP_POST_MARKS") else {
        return;
    };
    let mut out = String::new();
    for &(x, y, z) in marked {
        out.push_str(&format!("{x} {y} {z} {}\n", canvas.block_at(x, y, z)));
    }
    if let Err(error) = std::fs::write(&path, out) {
        eprintln!("post-process: could not write the mark dump: {error}");
    }
}

/// What one run of the pass did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Blocks the shape half changed.
    pub shapes: usize,
    /// What the fluid half did — see [`crate::stage::fluid::Stats`].
    pub fluid: crate::stage::fluid::Stats,
}

/// Run `LevelChunk#postProcessGeneration` over every marked position.
///
/// **One loop, and every write is visible to the next position.** Vanilla's
/// method is a single walk of the `postProcessing` lists doing, per position:
/// tick the fluid if the state has one, then either tick the block if it is a
/// `LiquidBlock` or fold `updateFromNeighbourShapes` over it and `setBlock`
/// the result. This had the second half, with its writes **deferred** to the
/// end of the pass — which was invisible for fences, because a fence's own
/// connections do not depend on its neighbour's connections, and is not
/// invisible at all once a fluid is spreading through the same list.
///
/// The `LiquidBlock` arm ticks the block, which in 26.2 is bubble columns and
/// nothing else (`LiquidBlock.tick` is a single `shouldBubbleColumnOccupy`
/// test). The pipeline already runs `decorate::form_bubble_columns`
/// immediately after this pass, so that arm is deliberately empty here rather
/// than duplicated.
pub fn run(
    canvas: &mut Canvas,
    tags: &BlockTags,
    facts: &dyn ShapeFacts,
    fluid_facts: &dyn crate::stage::fluid::FluidFacts,
    rules: crate::stage::fluid::FluidRules,
) -> Outcome {
    let marked = canvas.post_processing();
    dump_marks(canvas, &marked);

    let mut fluid = crate::stage::fluid::Stats::default();
    let mut tape = fluid_tape_from_env();
    let mut shapes = 0;
    for (x, y, z) in marked {
        crate::stage::fluid::tick_at(
            canvas,
            (x, y, z),
            tags,
            fluid_facts,
            rules,
            &mut fluid,
            &mut tape,
        );

        let state = canvas.block_at(x, y, z).to_string();
        let block = plain(&state);
        // Vanilla's only test here is `!(block instanceof LiquidBlock)`, and
        // the fold below already returns every unmodelled family untouched —
        // so gating on a family list as well would be a second, narrower
        // filter that vanilla does not have. It was one: `fences` only, which
        // meant the stairs, door and speleothem rules the fold has were
        // unreachable from this pass however many positions were marked.
        if block == "minecraft:water" || block == "minecraft:lava" {
            continue;
        }
        let neighbours = neighbours_of(canvas, x, y, z);
        let updated = update_from_neighbour_shapes(&state, &neighbours, tags, facts);
        if updated != state {
            canvas.set_block(x, y, z, &updated);
            shapes += 1;
        }
    }

    dump_fluid_tape(&tape);
    Outcome { shapes, fluid }
}

/// `RUSTMC_FLUID_TAPE=<path>` switches the fluid tape on. See
/// [`crate::stage::fluid::Tape`] for what it records and why it is scored
/// against a vanilla reference rather than a golden.
fn fluid_tape_from_env() -> crate::stage::fluid::Tape {
    if std::env::var_os("RUSTMC_FLUID_TAPE").is_some() {
        crate::stage::fluid::Tape::on()
    } else {
        crate::stage::fluid::Tape::off()
    }
}

/// Append one chunk's tape to the file, under a lock.
///
/// Appending rather than writing is what lets a whole-window dump produce one
/// tape; the lock is what stops two chunks tearing a line. It does **not**
/// make a parallel tape ordered — see [`crate::stage::fluid::Tape`] — and the
/// scorer is set-based for exactly that reason.
fn dump_fluid_tape(tape: &crate::stage::fluid::Tape) {
    if !tape.is_on() {
        return;
    }
    let Some(path) = std::env::var_os("RUSTMC_FLUID_TAPE") else {
        return;
    };
    if tape.lines().is_empty() {
        return;
    }
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let mut body = tape.lines().join("\n");
    body.push('\n');
    // A poisoned lock means some other chunk panicked mid-append. That is a
    // torn tape line, not a reason to panic a second time in the writer that
    // is trying to record why.
    let guard = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let opened = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path);
    match opened {
        Ok(mut file) => {
            use std::io::Write as _;
            if let Err(error) = file.write_all(body.as_bytes()) {
                eprintln!("post-process: could not write the fluid tape: {error}");
            }
        }
        Err(error) => eprintln!("post-process: could not open the fluid tape: {error}"),
    }
    drop(guard);
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Solid;
    impl crate::stage::fluid::FluidFacts for Solid {
        // The shape tests here are about fences, and the fluid half must not
        // silently join in: a full cube that blocks motion refuses every flow
        // decision, so these fixtures measure the shape fold alone.
        fn blocks_motion(&self, _state: &str) -> bool {
            true
        }
        fn is_solid(&self, _state: &str) -> bool {
            true
        }
        fn collision_full_faces(&self, _state: &str) -> [bool; 6] {
            [true; 6]
        }
    }

    impl ShapeFacts for Solid {
        fn face_is_sturdy(&self, state: &str, _direction: usize) -> Option<bool> {
            Some(plain(state) == "minecraft:stone")
        }
        fn face_is_collision_full(&self, state: &str, _direction: usize) -> Option<bool> {
            Some(plain(state) == "minecraft:stone")
        }
    }

    fn tags() -> BlockTags {
        let mut raw = std::collections::BTreeMap::new();
        raw.insert(
            "minecraft:fences".to_string(),
            vec!["minecraft:oak_fence".to_string()],
        );
        raw.insert(
            "minecraft:wooden_fences".to_string(),
            vec!["minecraft:oak_fence".to_string()],
        );
        raw.insert("minecraft:leaves".to_string(), Vec::new());
        raw.insert("minecraft:stairs".to_string(), Vec::new());
        raw.insert("minecraft:walls".to_string(), Vec::new());
        raw.insert(
            "minecraft:cannot_support_seagrass".to_string(),
            vec!["minecraft:magma_block".to_string()],
        );
        raw.insert(
            "minecraft:doors".to_string(),
            vec![
                "minecraft:spruce_door".to_string(),
                "minecraft:oak_door".to_string(),
            ],
        );
        raw.insert("minecraft:speleothems".to_string(), Vec::new());
        BlockTags::from_entries(raw).expect("tags")
    }

    fn wall_tags() -> BlockTags {
        let mut raw = std::collections::BTreeMap::new();
        raw.insert(
            "minecraft:walls".to_string(),
            vec!["minecraft:cobblestone_wall".to_string()],
        );
        raw.insert("minecraft:leaves".to_string(), Vec::new());
        raw.insert("minecraft:fences".to_string(), Vec::new());
        raw.insert("minecraft:fence_gates".to_string(), Vec::new());
        raw.insert("minecraft:wall_post_override".to_string(), Vec::new());
        BlockTags::from_entries(raw).expect("tags")
    }

    /// The measured case: seed 8, the woodland mansion at chunk (32,16), the
    /// cobblestone wall at (511,63,225) on the mansion's west face. The
    /// template's state has no west arm; vanilla's per-position fold connects
    /// it to the terrain's dirt there, and stays `low` under the air above.
    #[test]
    fn a_wall_connects_to_the_terrain_it_was_placed_against() {
        let state = "minecraft:cobblestone_wall[east=low,north=low,south=none,up=true,waterlogged=false,west=none]";
        let mut neighbours: [String; 6] = std::array::from_fn(|_| "minecraft:air".to_string());
        neighbours[2] = "minecraft:stone".to_string(); // north: the mansion's cobblestone
        neighbours[5] = "minecraft:stone".to_string(); // east: planks
        neighbours[4] = "minecraft:stone".to_string(); // west: dirt
        let updated = update_from_neighbour_shapes(state, &neighbours, &wall_tags(), &Solid);
        assert_eq!(
            updated,
            "minecraft:cobblestone_wall[east=low,north=low,south=none,up=true,waterlogged=false,west=low]"
        );
    }

    /// A side under a full block is `tall`, and two opposite tall sides take
    /// the post away. Under air the same wall is two `low` sides and keeps
    /// no post either — `shouldRaisePost` is false for a straight run.
    #[test]
    fn a_wall_under_a_full_block_is_tall_and_a_straight_run_drops_its_post() {
        let state = "minecraft:cobblestone_wall[east=none,north=low,south=low,up=true,waterlogged=false,west=none]";
        let mut neighbours: [String; 6] = std::array::from_fn(|_| "minecraft:air".to_string());
        neighbours[2] = "minecraft:cobblestone_wall[east=none,north=none,south=none,up=true,waterlogged=false,west=none]".to_string();
        neighbours[3] = "minecraft:stone".to_string();
        neighbours[1] = "minecraft:stone".to_string();
        let tags = wall_tags();
        assert_eq!(
            update_from_neighbour_shapes(state, &neighbours, &tags, &Solid),
            "minecraft:cobblestone_wall[east=none,north=tall,south=tall,up=false,waterlogged=false,west=none]"
        );
        neighbours[1] = "minecraft:air".to_string();
        assert_eq!(
            update_from_neighbour_shapes(state, &neighbours, &tags, &Solid),
            "minecraft:cobblestone_wall[east=none,north=low,south=low,up=false,waterlogged=false,west=none]"
        );
    }

    /// A block over the wall whose face this port has no row for leaves the
    /// wall exactly as it was.
    #[test]
    fn a_wall_under_an_unknown_shape_is_left_alone() {
        let state = "minecraft:cobblestone_wall[east=none,north=low,south=none,up=true,waterlogged=false,west=none]";
        let mut neighbours: [String; 6] = std::array::from_fn(|_| "minecraft:air".to_string());
        neighbours[2] = "minecraft:stone".to_string();
        neighbours[1] = "minecraft:oak_slab[type=top,waterlogged=false]".to_string();
        assert_eq!(
            update_from_neighbour_shapes(state, &neighbours, &wall_tags(), &Solid),
            state
        );
    }

    /// The six neighbours of a position with everything empty but the two
    /// indices a door cares about.
    fn column(below: &str, above: &str) -> [String; 6] {
        std::array::from_fn(|i| match i {
            0 => below.to_string(),
            1 => above.to_string(),
            _ => "minecraft:air".to_string(),
        })
    }

    /// The four horizontal neighbours, N S W E, around a block with air
    /// above and below.
    fn ring(n: &str, s: &str, w: &str, e: &str) -> [String; 6] {
        ["minecraft:air", "minecraft:air", n, s, w, e].map(str::to_string)
    }

    /// Seed 30's (37,-47) mansion, its `2x2_a2` storeroom mirrored: the
    /// template's east-facing pair becomes a west-facing pair whose halves
    /// point away from each other, and vanilla saves all 26 as `single`.
    /// A `LEFT` west chest is connected to its clockwise side, north.
    #[test]
    fn a_chest_half_whose_partner_is_not_beside_it_goes_single() {
        let tags = tags();
        let left = "minecraft:chest[facing=west,type=left,waterlogged=false]";
        let partner_south = "minecraft:chest[facing=west,type=right,waterlogged=false]";
        assert_eq!(
            update_from_neighbour_shapes(
                left,
                &ring(
                    "minecraft:air",
                    partner_south,
                    "minecraft:air",
                    "minecraft:air"
                ),
                &tags,
                &Solid
            ),
            "minecraft:chest[facing=west,type=single,waterlogged=false]"
        );
        // With the partner on the connected (north) side the pair stands.
        assert_eq!(
            update_from_neighbour_shapes(
                left,
                &ring(
                    partner_south,
                    "minecraft:air",
                    "minecraft:air",
                    "minecraft:air"
                ),
                &tags,
                &Solid
            ),
            left
        );
    }

    /// The first arm: a single chest beside half of a pair that points at it
    /// takes the opposite half; beside another single it stays single, and a
    /// trapped chest never pairs with a plain one (`chestCanConnectTo` is
    /// `state.is(this)`).
    #[test]
    fn a_single_chest_joins_only_a_half_pointing_at_it() {
        let tags = tags();
        let single = "minecraft:chest[facing=north,type=single,waterlogged=false]";
        // A north-facing RIGHT is connected counter-clockwise, to its west;
        // standing east of `single`, it points at it.
        let right_east = "minecraft:chest[facing=north,type=right,waterlogged=false]";
        let air = "minecraft:air";
        assert_eq!(
            update_from_neighbour_shapes(single, &ring(air, air, air, right_east), &tags, &Solid),
            "minecraft:chest[facing=north,type=left,waterlogged=false]"
        );
        let lone = "minecraft:chest[facing=north,type=single,waterlogged=false]";
        assert_eq!(
            update_from_neighbour_shapes(single, &ring(air, air, air, lone), &tags, &Solid),
            single
        );
        let trapped = "minecraft:trapped_chest[facing=north,type=right,waterlogged=false]";
        assert_eq!(
            update_from_neighbour_shapes(single, &ring(air, air, air, trapped), &tags, &Solid),
            single
        );
    }

    /// The measured case, block for block: seed 20260722, the shipwreck at
    /// chunk (57,65), `shipwreck/rightsideup_fronthalf` at `rot=NONE`, the
    /// door pair at (915,56,1056)/(915,57,1056). The template stores the two
    /// halves disagreeing and vanilla's lower half ends up wearing the
    /// upper's `facing` and `hinge`.
    #[test]
    fn a_lower_door_half_takes_the_upper_halfs_state() {
        let lower = "minecraft:spruce_door[facing=south,half=lower,hinge=right,\
                     open=false,powered=false]";
        let upper = "minecraft:spruce_door[facing=east,half=upper,hinge=left,\
                     open=false,powered=false]";
        let tags = tags();
        assert_eq!(
            update_from_neighbour_shapes(lower, &column("minecraft:stone", upper), &tags, &Solid),
            "minecraft:spruce_door[facing=east,half=lower,hinge=left,\
             open=false,powered=false]"
        );
    }

    /// And the upper half then reads the *updated* lower and does not move —
    /// which is what makes the pair converge rather than oscillate. Written
    /// in the order `placeInWorld` walks them: the template's block list
    /// holds the lower first.
    #[test]
    fn the_upper_half_is_unchanged_once_the_lower_agrees() {
        let lower_updated = "minecraft:spruce_door[facing=east,half=lower,hinge=left,\
                             open=false,powered=false]";
        let upper = "minecraft:spruce_door[facing=east,half=upper,hinge=left,\
                     open=false,powered=false]";
        assert_eq!(
            update_from_neighbour_shapes(
                upper,
                &column(lower_updated, "minecraft:air"),
                &tags(),
                &Solid
            ),
            upper
        );
    }

    /// `neighborState.setValue(HALF, half)` carries the neighbour's *block*,
    /// not just its properties — a distinction a "copy facing/hinge/open/
    /// powered" reading of the rule would lose.
    #[test]
    fn the_whole_neighbour_state_is_taken_block_and_all() {
        let lower = "minecraft:spruce_door[facing=south,half=lower,hinge=right,\
                     open=false,powered=false]";
        let upper = "minecraft:oak_door[facing=west,half=upper,hinge=left,\
                     open=false,powered=false]";
        assert_eq!(
            update_from_neighbour_shapes(lower, &column("minecraft:stone", upper), &tags(), &Solid),
            "minecraft:oak_door[facing=west,half=lower,hinge=left,\
             open=false,powered=false]"
        );
    }

    /// A half with no partner is destroyed, and a lower half over nothing
    /// sturdy is destroyed too. Both are vanilla's answers and both are
    /// unreachable for the shipwreck — asserted so that a caller that *does*
    /// reach them finds the behaviour written down.
    #[test]
    fn a_door_half_without_its_partner_becomes_air() {
        let lower = "minecraft:spruce_door[facing=south,half=lower,hinge=right,\
                     open=false,powered=false]";
        let tags = tags();
        assert_eq!(
            update_from_neighbour_shapes(
                lower,
                &column("minecraft:stone", "minecraft:water[level=0]"),
                &tags,
                &Solid
            ),
            "minecraft:air",
            "no upper half above"
        );
        let upper = "minecraft:spruce_door[facing=east,half=upper,hinge=left,\
                     open=false,powered=false]";
        assert_eq!(
            update_from_neighbour_shapes(lower, &column("minecraft:dirt", upper), &tags, &Solid),
            "minecraft:air",
            "nothing sturdy below"
        );
    }

    /// The measured `updateShapeAtEdge` case: seed 20260722, (920,60,1056)
    /// and (920,60,1057), where seagrass grown before the wreck arrived is
    /// left sitting on the wreck's waterlogged `dark_oak_fence` rail. Vanilla
    /// holds plain `air` at both, and this is why.
    ///
    /// Asked from `DOWN`, which is the direction the face walk emits when the
    /// fence below is the last filled cell of a Y run.
    #[test]
    fn seagrass_on_a_fence_rail_is_destroyed() {
        let rail = "minecraft:dark_oak_fence[east=true,north=true,south=true,\
                    waterlogged=true,west=true]";
        assert_eq!(
            update_shape(
                "minecraft:seagrass",
                0,
                rail,
                &column(rail, "minecraft:water[level=0]"),
                &tags(),
                &Solid
            ),
            "minecraft:air"
        );
    }

    /// And it survives what it can stand on, from the same direction — so the
    /// rule is not "delete every seagrass the walk touches".
    #[test]
    fn seagrass_on_stone_survives_the_walk() {
        assert_eq!(
            update_shape(
                "minecraft:seagrass",
                0,
                "minecraft:stone",
                &column("minecraft:stone", "minecraft:water[level=0]"),
                &tags(),
                &Solid
            ),
            "minecraft:seagrass"
        );
        assert_eq!(
            update_shape(
                "minecraft:seagrass",
                0,
                "minecraft:magma_block",
                &column("minecraft:magma_block", "minecraft:water[level=0]"),
                &tags(),
                &Solid
            ),
            "minecraft:air",
            "the refusing tag beats a sturdy face"
        );
    }

    /// `updateShape` is one direction, and that is the whole difference from
    /// [`update_from_neighbour_shapes`]: a fence asked about `WEST` rewrites
    /// `west` and touches nothing else, and asked about a vertical direction
    /// rewrites nothing at all.
    #[test]
    fn one_direction_moves_one_fence_property() {
        let placed = "minecraft:oak_fence[east=true,north=false,south=false,\
                      waterlogged=false,west=false]";
        let neighbours = std::array::from_fn(|_| "minecraft:stone".to_string());
        assert_eq!(
            update_shape(placed, 4, "minecraft:stone", &neighbours, &tags(), &Solid),
            "minecraft:oak_fence[east=true,north=false,south=false,\
             waterlogged=false,west=true]",
            "west connects; east is left as placed even though stone is there too"
        );
        assert_eq!(
            update_shape(placed, 1, "minecraft:stone", &neighbours, &tags(), &Solid),
            placed,
            "a vertical direction falls through to Block#updateShape"
        );
    }

    /// The door rule reached through the one-direction dispatcher fires on its
    /// one live direction and is the identity on the rest — including `DOWN`
    /// for an *upper* half, which shares a direction with the lower half's
    /// `canSurvive` and must not be confused with it.
    #[test]
    fn a_door_answers_one_direction_through_the_walk() {
        let lower = "minecraft:spruce_door[facing=south,half=lower,hinge=right,\
                     open=false,powered=false]";
        let upper = "minecraft:spruce_door[facing=east,half=upper,hinge=left,\
                     open=false,powered=false]";
        let tags = tags();
        assert_eq!(
            update_shape(
                lower,
                1,
                upper,
                &column("minecraft:stone", upper),
                &tags,
                &Solid
            ),
            "minecraft:spruce_door[facing=east,half=lower,hinge=left,\
             open=false,powered=false]",
            "UP is the lower half's copy"
        );
        assert_eq!(
            update_shape(
                lower,
                2,
                upper,
                &column("minecraft:stone", upper),
                &tags,
                &Solid
            ),
            lower,
            "a horizontal direction moves nothing"
        );
        assert_eq!(
            update_shape(
                upper,
                0,
                "minecraft:water[level=0]",
                &column("minecraft:water[level=0]", "minecraft:air"),
                &tags,
                &Solid
            ),
            "minecraft:air",
            "DOWN for an upper half is the partner test, not canSurvive"
        );
    }

    /// The case that motivated the pass: a fence placed with a connection
    /// against a wall that does not support one loses it.
    #[test]
    fn a_fence_against_nothing_loses_its_connection() {
        let mut canvas = Canvas::new(-64, 384);
        canvas.ensure_chunk(0, 0, "minecraft:cave_air");
        let placed = "minecraft:oak_fence[east=false,north=false,south=false,\
                      waterlogged=false,west=true]";
        canvas.set_block(1, 0, 1, placed);
        canvas.mark_post_processing(1, 0, 1);

        assert_eq!(
            run(
                &mut canvas,
                &tags(),
                &Solid,
                &Solid,
                crate::stage::fluid::FluidRules::REFERENCE_HARNESS,
            )
            .shapes,
            1,
            "one block changed"
        );
        assert_eq!(
            canvas.block_at(1, 0, 1),
            "minecraft:oak_fence[east=false,north=false,south=false,\
             waterlogged=false,west=false]"
        );
    }

    /// And it keeps one it can actually reach — the pass must not simply
    /// clear everything, which would pass the test above for the wrong
    /// reason.
    #[test]
    fn a_fence_against_stone_keeps_its_connection() {
        let mut canvas = Canvas::new(-64, 384);
        canvas.ensure_chunk(0, 0, "minecraft:cave_air");
        canvas.set_block(0, 0, 1, "minecraft:stone");
        let placed = "minecraft:oak_fence[east=false,north=false,south=false,\
                      waterlogged=false,west=true]";
        canvas.set_block(1, 0, 1, placed);
        canvas.mark_post_processing(1, 0, 1);

        assert_eq!(
            run(
                &mut canvas,
                &tags(),
                &Solid,
                &Solid,
                crate::stage::fluid::FluidRules::REFERENCE_HARNESS,
            )
            .shapes,
            0,
            "nothing to change"
        );
        assert!(canvas.block_at(1, 0, 1).contains("west=true"));
    }
}
