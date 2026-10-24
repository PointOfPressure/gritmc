//! `WallBlock` — how a wall connects to what is beside it and how tall each
//! arm and the post are, from what is above it.
//!
//! Transcribed from the pinned 26.2 jar: `connectsTo` (offsets 0-68),
//! `getStateForPlacement` (0-236), `updateShape` (0-93), `topUpdate`,
//! `sideUpdate`, the private five-argument `updateShape` that both reach,
//! `shouldRaisePost` (0-267), `updateSides` (0-123), `makeWallState` (0-23)
//! and `isCovered` (0-16).
//!
//! # Why this is code and not a table row
//!
//! `update_shape.json` refuses `WallBlock` outright — 41,472 of its refused
//! pairs, the largest single block of them — because the answer reads the
//! **collision shape of the block above** as well as the neighbour that
//! changed, and the placement extract samples every block clicked into open
//! air, where a wall is always a bare post. Both halves are therefore ported
//! here and reached through [`crate::placement`] and
//! [`crate::shapes::ShapeTable::level_update_shape`].
//!
//! Measured on the two corpus checks that exercise it:
//!
//! * `730` — a wall between two walls: vanilla writes
//!   `east=low,west=low,up=false` and raises both neighbours' arms in one
//!   `section_blocks_update`; this server wrote a bare post and moved nothing.
//! * `731` — stone on top of a wall: vanilla turns its arm `tall`; this
//!   server left it `none`.
//!
//! # The shape test, and what is approximated in it
//!
//! `isCovered(face, test)` is `!Shapes.joinIsNotEmpty(test, face,
//! ONLY_FIRST)` — `test` lies wholly inside `face`. `face` is the above
//! block's collision shape's `getFaceShape(DOWN)`: the slice of the shape
//! through `y = 1e-7`, stretched through the whole cell. Every test shape
//! spans the full height, so the question is a 2-D one over the slice, and
//! [`face_down_covers`] answers it exactly on the shape's own grid.
//!
//! A block whose collision shape the table cannot give (the 23 dynamic-shape
//! blocks — shulker boxes, scaffolding, powder snow, …) is taken to cover
//! nothing. That is a guess and is named: vanilla computes those shapes live.

use crate::interact::Face;
use crate::placement::{property_of, with_property, Around};

/// `TEST_SHAPE_POST` — `Block.column(2, 0, 16)`: x and z 7..9, full height.
const TEST_POST: (f64, f64, f64, f64) = (7.0, 7.0, 9.0, 9.0);

/// `TEST_SHAPES_WALL` — `Shapes.rotateHorizontal(Block.boxZ(2, 16, 0, 9))`:
/// a two-wide strip from the centre to the side, `(x0, z0, x1, z1)` in
/// sixteenths, for the four horizontals.
const fn test_arm(d: Face) -> (f64, f64, f64, f64) {
    match d {
        Face::North => (7.0, 0.0, 9.0, 9.0),
        Face::South => (7.0, 7.0, 9.0, 16.0),
        Face::West => (0.0, 7.0, 9.0, 9.0),
        // East, and the two vertical faces which never reach here.
        _ => (7.0, 7.0, 16.0, 9.0),
    }
}

/// The classes and tags the rules below test, as questions — so the rules
/// can be exercised without the extract tables.
pub struct Facts<'a> {
    /// `block instanceof class`, including subclasses.
    pub is_a: &'a dyn Fn(&str, &str) -> bool,
    /// `state.is(BlockTags.<tag>)`, the tag named by its path.
    pub tagged: &'a dyn Fn(&str, &str) -> bool,
    /// A state's collision shape, or `None` when the table has no row.
    pub collision: &'a dyn Fn(&str) -> Option<&'a crate::collide::Shape>,
}

fn block_of(state: &str) -> &str {
    state.split('[').next().unwrap_or(state)
}

/// Whether a state is a wall at all — `instanceof WallBlock`.
#[must_use]
pub fn is_wall(facts: &Facts<'_>, state: &str) -> bool {
    (facts.is_a)(block_of(state), "WallBlock")
}

/// `Block.isExceptionForConnection`: leaves, barrier, carved pumpkin, jack
/// o'lantern, melon, pumpkin, and the shulker boxes tag.
fn is_exception_for_connection(facts: &Facts<'_>, state: &str) -> bool {
    let b = block_of(state);
    (facts.is_a)(b, "LeavesBlock")
        || matches!(
            b,
            "minecraft:barrier"
                | "minecraft:carved_pumpkin"
                | "minecraft:jack_o_lantern"
                | "minecraft:melon"
                | "minecraft:pumpkin"
        )
        || (facts.tagged)("shulker_boxes", b)
}

/// `FenceGateBlock.connectsToDirection(state, dir)`:
/// `FACING.getAxis() == dir.getClockWise().getAxis()` — a gate meets a wall
/// side-on.
fn fence_gate_connects(state: &str, d: Face) -> bool {
    let facing = property_of(state, "facing").and_then(Face::from_name);
    let facing_x = matches!(facing, Some(Face::East | Face::West));
    // Clockwise of a z-facing direction is on x, and vice versa.
    let clockwise_x = matches!(d, Face::North | Face::South);
    facing.is_some() && facing_x == clockwise_x
}

/// `WallBlock.connectsTo(state, sturdy, direction)`, offsets 0-68.
fn connects_to(facts: &Facts<'_>, state: &str, sturdy: bool, d: Face) -> bool {
    let b = block_of(state);
    let gate = (facts.is_a)(b, "FenceGateBlock") && fence_gate_connects(state, d);
    (facts.tagged)("walls", b)
        || (!is_exception_for_connection(facts, state) && sturdy)
        || (facts.is_a)(b, "IronBarsBlock")
        || gate
}

/// `isCovered(face, test)` for the above block's `getFaceShape(DOWN)` and a
/// full-height test shape given as its `(x0, z0, x1, z1)` in sixteenths.
fn face_down_covers(shape: Option<&crate::collide::Shape>, test: (f64, f64, f64, f64)) -> bool {
    use crate::collide::Axis;
    let Some(shape) = shape else { return false };
    if shape.is_empty() {
        return false;
    }
    let (x0, z0, x1, z1) = (test.0 / 16.0, test.1 / 16.0, test.2 / 16.0, test.3 / 16.0);
    // `findIndex(Y, 1e-7)`: the last cell whose lower bound is <= 1e-7, or
    // -1 when the shape starts above the floor — an empty slice.
    let ys = shape.coords(Axis::Y);
    let iy = ys
        .iter()
        .rposition(|c| *c <= 1.0e-7)
        .map_or(-1, |i| i as i64);
    if iy < 0 || iy >= shape.size(Axis::Y) as i64 {
        return false;
    }
    let cell = |coords: &[f64], v: f64| -> Option<i64> {
        (0..coords.len().saturating_sub(1))
            .find(|&i| coords[i] <= v && v < coords[i + 1])
            .map(|i| i as i64)
    };
    // Split the test rectangle on every grid line of the shape that crosses
    // it, and require each piece to lie in a full cell.
    let cuts = |coords: &[f64], lo: f64, hi: f64| -> Vec<f64> {
        let mut v: Vec<f64> = coords
            .iter()
            .copied()
            .filter(|c| *c > lo && *c < hi)
            .collect();
        v.push(lo);
        v.push(hi);
        v.sort_by(f64::total_cmp);
        v.dedup();
        v
    };
    let xs = cuts(shape.coords(Axis::X), x0, x1);
    let zs = cuts(shape.coords(Axis::Z), z0, z1);
    for wx in xs.windows(2) {
        for wz in zs.windows(2) {
            let (mx, mz) = ((wx[0] + wx[1]) / 2.0, (wz[0] + wz[1]) / 2.0);
            let (Some(ix), Some(iz)) = (
                cell(shape.coords(Axis::X), mx),
                cell(shape.coords(Axis::Z), mz),
            ) else {
                return false;
            };
            if !shape.is_full_wide(ix, iy, iz) {
                return false;
            }
        }
    }
    true
}

/// `WallBlock.shouldRaisePost(state, above, aboveFace)`, offsets 0-267.
fn should_raise_post(
    facts: &Facts<'_>,
    state: &str,
    above: &str,
    face: Option<&crate::collide::Shape>,
) -> bool {
    if is_wall(facts, above) && property_of(above, "up") == Some("true") {
        return true;
    }
    let side = |p: &str| property_of(state, p).unwrap_or("none");
    let (n, s, e, w) = (side("north"), side("south"), side("east"), side("west"));
    let (n0, s0, e0, w0) = (n == "none", s == "none", e == "none", w == "none");
    if (n0 && s0 && w0 && e0) || n0 != s0 || w0 != e0 {
        return true;
    }
    if (n == "tall" && s == "tall") || (e == "tall" && w == "tall") {
        return false;
    }
    (facts.tagged)("wall_post_override", block_of(above)) || face_down_covers(face, TEST_POST)
}

/// The private five-argument `updateShape`: `updateSides` with the above
/// block's down face, then `UP` from `shouldRaisePost`.
fn with_sides(facts: &Facts<'_>, state: &str, above: &str, conn: [bool; 4]) -> String {
    let face = (facts.collision)(above);
    let mut s = state.to_string();
    for (d, name, c) in [
        (Face::North, "north", conn[0]),
        (Face::East, "east", conn[1]),
        (Face::South, "south", conn[2]),
        (Face::West, "west", conn[3]),
    ] {
        // `makeWallState(connected, face, TEST_SHAPES_WALL.get(d))`.
        let v = if !c {
            "none"
        } else if face_down_covers(face, test_arm(d)) {
            "tall"
        } else {
            "low"
        };
        s = with_property(&s, name, v);
    }
    let up = should_raise_post(facts, &s, above, face);
    with_property(&s, "up", if up { "true" } else { "false" })
}

/// Whether one of the four neighbours connects, as `getStateForPlacement`
/// and `sideUpdate` both ask it: the neighbour's own face towards the wall.
fn neighbour_connects(facts: &Facts<'_>, around: &Around<'_>, d: Face) -> bool {
    let p = d.relative(around.pos);
    let state = (around.at)(p);
    let towards = d.opposite();
    connects_to(facts, &state, (around.sturdy)(p, towards), towards)
}

/// `WallBlock.getStateForPlacement`, offsets 0-236, on top of `default`
/// (whose `waterlogged` the caller has already decided).
#[must_use]
pub fn placement(facts: &Facts<'_>, default: &str, around: &Around<'_>) -> String {
    let conn = [Face::North, Face::East, Face::South, Face::West]
        .map(|d| neighbour_connects(facts, around, d));
    let above = (around.at)(Face::Up.relative(around.pos));
    with_sides(facts, default, &above, conn)
}

/// `WallBlock.updateShape`, offsets 34-93 (the waterlogged fluid tick at
/// 0-33 is not a state change and is not ported here, as for stairs).
///
/// `DOWN` falls to `Block.updateShape` — unchanged, `None`. `UP` is
/// `topUpdate`: the four arms keep whether they are connected and re-read
/// only their height and the post. A side is `sideUpdate`: that one arm
/// re-asks `connectsTo` of the neighbour that changed and the other three
/// keep theirs.
#[must_use]
pub fn update_shape(
    facts: &Facts<'_>,
    state: &str,
    direction: Face,
    neighbour_state: &str,
    around: &Around<'_>,
) -> Option<String> {
    let connected = |p: &str| property_of(state, p).is_some_and(|v| v != "none");
    let mut conn = [
        connected("north"),
        connected("east"),
        connected("south"),
        connected("west"),
    ];
    let above = match direction {
        Face::Down => return None,
        Face::Up => neighbour_state.to_string(),
        d => {
            let towards = d.opposite();
            let sturdy = (around.sturdy)(d.relative(around.pos), towards);
            let now = connects_to(facts, neighbour_state, sturdy, towards);
            match d {
                Face::North => conn[0] = now,
                Face::East => conn[1] = now,
                Face::South => conn[2] = now,
                _ => conn[3] = now,
            }
            (around.at)(Face::Up.relative(around.pos))
        }
    };
    let next = with_sides(facts, state, &above, conn);
    (next != state).then_some(next)
}

/// [`Facts`] over the live extract tables — the class map, the block tags
/// and the collision shapes — for the two call sites in the server. `None`
/// when the class map is missing, in which case no wall rule runs.
pub fn live<T>(
    state_id: &dyn Fn(&str) -> Option<i32>,
    f: impl FnOnce(&Facts<'_>) -> T,
) -> Option<T> {
    let classes = crate::placedby::PlacedByTable::shared(std::path::Path::new(
        "data/vanilla/set_placed_by.json",
    ))
    .ok()?;
    let shapes = crate::collide::CollisionShapes::shared();
    let is_a = |b: &str, c: &str| classes.is_a(b, c);
    let tagged = |t: &str, b: &str| crate::blocktags::is_in(t, b) == Some(true);
    let collision = |s: &str| -> Option<&crate::collide::Shape> {
        let id = u32::try_from(state_id(s)?).ok()?;
        shapes?.get(id)
    };
    Some(f(&Facts {
        is_a: &is_a,
        tagged: &tagged,
        collision: &collision,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interact::BlockPos;

    const WALL: &str = "minecraft:cobblestone_wall[east=none,north=none,south=none,up=true,waterlogged=false,west=none]";

    fn facts_run<T>(f: impl FnOnce(&Facts<'_>) -> T) -> T {
        let is_a = |b: &str, c: &str| c == "WallBlock" && b.ends_with("_wall");
        let tagged = |t: &str, b: &str| t == "walls" && b.ends_with("_wall");
        // No shapes: nothing above covers anything.
        let collision = |_: &str| -> Option<&crate::collide::Shape> { None };
        f(&Facts {
            is_a: &is_a,
            tagged: &tagged,
            collision: &collision,
        })
    }

    fn around<'a>(at: &'a dyn Fn(BlockPos) -> String) -> Around<'a> {
        Around {
            pos: (2, 0, 1),
            at,
            sturdy: &|_, _| false,
            conductor: &|_| false,
            source: &|_| false,
            instrument: &|_| None,
            state_id: &|_| None,
        }
    }

    /// `730`: a wall placed between two walls on x connects both ways, low,
    /// and drops its post — the two arms are opposite and equal, so
    /// `shouldRaisePost`'s asymmetry test is false and, with air above, so is
    /// the post test.
    #[test]
    fn a_wall_between_two_walls_is_low_both_ways_with_no_post() {
        let at = |p: BlockPos| match p {
            (1, 0, 1) | (3, 0, 1) => WALL.to_string(),
            _ => "minecraft:air".to_string(),
        };
        let got = facts_run(|f| placement(f, WALL, &around(&at)));
        assert_eq!(
            got,
            "minecraft:cobblestone_wall[east=low,north=none,south=none,up=false,waterlogged=false,west=low]"
        );
    }

    /// And the neighbour's side of it: a wall that sees a wall arrive to its
    /// east raises that arm and, one arm only, keeps its post.
    #[test]
    fn a_wall_hears_a_wall_arrive_beside_it() {
        let at = |_: BlockPos| "minecraft:air".to_string();
        let got = facts_run(|f| update_shape(f, WALL, Face::East, WALL, &around(&at)));
        assert_eq!(
            got.as_deref(),
            Some("minecraft:cobblestone_wall[east=low,north=none,south=none,up=true,waterlogged=false,west=none]")
        );
        assert_eq!(
            facts_run(|f| update_shape(f, WALL, Face::Down, WALL, &around(&at))),
            None
        );
    }
}
