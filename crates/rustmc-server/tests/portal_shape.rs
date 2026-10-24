//! `PortalShape.findAnyShape` against vanilla's own answers on eighteen frames.
//!
//! `crates/rustmc-server/golden/portal_shape.json` is produced by
//! `ci/portal-golden.sh`, which runs vanilla's own `findAnyShape` inside the
//! pinned jar against an in-memory `BlockGetter`. The frames are rebuilt here
//! by the same rules, and every field is compared: width, height,
//! `numPortalBlocks`, the bottom-left corner, `rightDir`, `isValid`,
//! `isComplete`, and the block positions `createPortalBlocks` would write.
//!
//! # What can turn this red
//!
//! Any of the four size bounds read as exclusive; a perimeter scan instead of
//! the two-sides-and-a-top walk (`no-corners-3x4`); a `numPortalBlocks` count
//! taken after the search rather than during it (`min-2x3-x-half-lit`); the
//! downward walk in `calculateBottomLeft` (`entered-from-top`); the leftward
//! walk (`entered-from-right`); `isEmpty` missing the fire tag
//! (`fire-inside-2x3`); and the `betweenClosed` iteration order, which for a
//! WEST-facing portal runs the horizontal axis backwards relative to the
//! search.
//!
//! # What cannot
//!
//! Anything about lighting, teleporting or the portal block itself. This is
//! the frame search and nothing else.

use std::collections::HashMap;
use std::path::PathBuf;

use rustmc_data::BlockPos;
use rustmc_server::dimension::forcer;
use rustmc_server::portal::{self, Axis, PortalShape};

fn golden() -> serde_json::Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("golden/portal_shape.json");
    assert!(
        rustmc_data::fixture::available(&p, "portal shape golden", "ci/portal-golden.sh"),
        "no portal shape golden"
    );
    serde_json::from_slice(&std::fs::read(&p).expect("read golden")).expect("parse golden")
}

/// The same in-memory world the Java `Grid` is.
#[derive(Default, Clone)]
struct Grid(HashMap<(i32, i32, i32), String>);

impl Grid {
    fn put(&mut self, x: i32, y: i32, z: i32, s: &str) {
        self.0.insert((x, y, z), s.to_string());
    }
    fn getter(&self) -> impl Fn(BlockPos) -> String + '_ {
        move |p: BlockPos| {
            self.0
                .get(&(p.x, p.y, p.z))
                .cloned()
                .unwrap_or_else(|| "minecraft:air".to_string())
        }
    }
    /// `PortalGolden.put` — `u` along the portal's axis, `v` is world y.
    fn put_uv(&mut self, axis: Axis, u: i32, v: i32, s: &str) {
        match axis {
            Axis::X => self.put(u, v, 0, s),
            Axis::Z => self.put(0, v, u, s),
        }
    }
}

/// `PortalGolden.frame`.
fn frame(mat: &str, w: i32, h: i32, axis: Axis) -> Grid {
    let mut g = Grid::default();
    for u in -1..=w {
        g.put_uv(axis, u, 0, mat);
        g.put_uv(axis, u, h + 1, mat);
    }
    for v in 1..=h {
        g.put_uv(axis, -1, v, mat);
        g.put_uv(axis, w, v, mat);
    }
    g
}

const OBSIDIAN: &str = "minecraft:obsidian";
const AIR: &str = "minecraft:air";
const PORTAL: &str = "minecraft:nether_portal[axis=x]";

/// The same eighteen scenarios `PortalGolden.main` builds, in the same order.
fn scenarios() -> Vec<(&'static str, Grid, Axis, BlockPos)> {
    let from_x = |u: i32, y: i32| BlockPos { x: u, y, z: 0 };
    let from_z = |u: i32, y: i32| BlockPos { x: 0, y, z: u };
    let mut out: Vec<(&'static str, Grid, Axis, BlockPos)> = Vec::new();

    out.push((
        "min-2x3-x",
        frame(OBSIDIAN, 2, 3, Axis::X),
        Axis::X,
        from_x(0, 1),
    ));

    let mut lit = frame(OBSIDIAN, 2, 3, Axis::X);
    for u in 0..2 {
        for v in 1..=3 {
            lit.put_uv(Axis::X, u, v, PORTAL);
        }
    }
    out.push(("min-2x3-x-lit", lit, Axis::X, from_x(0, 1)));

    let mut half = frame(OBSIDIAN, 2, 3, Axis::X);
    half.put(0, 1, 0, PORTAL);
    half.put(1, 1, 0, PORTAL);
    out.push(("min-2x3-x-half-lit", half, Axis::X, from_x(0, 1)));

    out.push((
        "min-2x3-z",
        frame(OBSIDIAN, 2, 3, Axis::Z),
        Axis::Z,
        from_z(0, 1),
    ));
    out.push((
        "too-narrow-1x3",
        frame(OBSIDIAN, 1, 3, Axis::X),
        Axis::X,
        from_x(0, 1),
    ));
    out.push((
        "too-short-2x2",
        frame(OBSIDIAN, 2, 2, Axis::X),
        Axis::X,
        from_x(0, 1),
    ));
    out.push((
        "max-21x21",
        frame(OBSIDIAN, 21, 21, Axis::X),
        Axis::X,
        from_x(0, 1),
    ));
    out.push((
        "too-wide-22x3",
        frame(OBSIDIAN, 22, 3, Axis::X),
        Axis::X,
        from_x(0, 1),
    ));
    out.push((
        "too-tall-2x22",
        frame(OBSIDIAN, 2, 22, Axis::X),
        Axis::X,
        from_x(0, 1),
    ));

    let mut no_corners = frame(OBSIDIAN, 3, 4, Axis::X);
    no_corners.put(-1, 0, 0, AIR);
    no_corners.put(3, 0, 0, AIR);
    no_corners.put(-1, 5, 0, AIR);
    no_corners.put(3, 5, 0, AIR);
    out.push(("no-corners-3x4", no_corners, Axis::X, from_x(0, 1)));

    let mut hole_top = frame(OBSIDIAN, 3, 4, Axis::X);
    hole_top.put(1, 5, 0, AIR);
    out.push(("hole-in-top-3x4", hole_top, Axis::X, from_x(0, 1)));

    let mut hole_side = frame(OBSIDIAN, 3, 4, Axis::X);
    hole_side.put(-1, 3, 0, AIR);
    out.push(("hole-in-side-3x4", hole_side, Axis::X, from_x(0, 1)));

    out.push((
        "stone-frame-2x3",
        frame("minecraft:stone", 2, 3, Axis::X),
        Axis::X,
        from_x(0, 1),
    ));

    let mut with_fire = frame(OBSIDIAN, 2, 3, Axis::X);
    with_fire.put(0, 1, 0, "minecraft:fire");
    out.push(("fire-inside-2x3", with_fire, Axis::X, from_x(0, 1)));

    let mut with_stone = frame(OBSIDIAN, 2, 3, Axis::X);
    with_stone.put(1, 1, 0, "minecraft:stone");
    out.push(("stone-inside-2x3", with_stone, Axis::X, from_x(0, 1)));

    out.push((
        "entered-from-top",
        frame(OBSIDIAN, 2, 3, Axis::X),
        Axis::X,
        from_x(0, 3),
    ));
    out.push((
        "entered-from-right",
        frame(OBSIDIAN, 4, 3, Axis::X),
        Axis::X,
        from_x(3, 1),
    ));
    out.push(("empty-world", Grid::default(), Axis::X, from_x(0, 1)));
    out
}

fn pos(v: &serde_json::Value) -> BlockPos {
    let a = v.as_array().unwrap();
    BlockPos {
        x: a[0].as_i64().unwrap() as i32,
        y: a[1].as_i64().unwrap() as i32,
        z: a[2].as_i64().unwrap() as i32,
    }
}

#[test]
fn every_frame_matches_vanilla() {
    let g = golden();
    assert_eq!(
        g["minWidth"].as_i64().unwrap(),
        i64::from(portal::MIN_WIDTH)
    );
    assert_eq!(
        g["maxWidth"].as_i64().unwrap(),
        i64::from(portal::MAX_WIDTH)
    );
    assert_eq!(
        g["minHeight"].as_i64().unwrap(),
        i64::from(portal::MIN_HEIGHT)
    );
    assert_eq!(
        g["maxHeight"].as_i64().unwrap(),
        i64::from(portal::MAX_HEIGHT)
    );

    let shapes = g["shapes"].as_array().unwrap();
    let ours = scenarios();
    assert_eq!(
        shapes.len(),
        ours.len(),
        "the Java script has {} frames and this one has {} — they have drifted",
        shapes.len(),
        ours.len()
    );

    // The world floor the Java `Grid` reports.
    const MIN_Y: i32 = -64;
    let mut checked = 0usize;
    for (v, (name, grid, axis, from)) in shapes.iter().zip(&ours) {
        assert_eq!(v["name"].as_str().unwrap(), *name, "frame order differs");
        assert_eq!(pos(&v["from"]), *from, "{name}: entry position differs");
        let got = portal::find_any_shape(&grid.getter(), MIN_Y, *from, *axis);

        assert_eq!(
            i64::from(got.width),
            v["width"].as_i64().unwrap(),
            "{name}.width"
        );
        assert_eq!(
            i64::from(got.height),
            v["height"].as_i64().unwrap(),
            "{name}.height"
        );
        assert_eq!(
            i64::from(got.num_portal_blocks),
            v["numPortalBlocks"].as_i64().unwrap(),
            "{name}.numPortalBlocks"
        );
        assert_eq!(got.bottom_left, pos(&v["bottomLeft"]), "{name}.bottomLeft");
        assert_eq!(
            got.right_dir(),
            v["rightDir"].as_str().unwrap(),
            "{name}.rightDir"
        );
        assert_eq!(
            got.is_valid(),
            v["valid"].as_bool().unwrap(),
            "{name}.valid"
        );
        assert_eq!(
            got.is_complete(),
            v["complete"].as_bool().unwrap(),
            "{name}.complete"
        );

        let want: Vec<BlockPos> = v["portalPositions"]
            .as_array()
            .unwrap()
            .iter()
            .map(pos)
            .collect();
        assert_eq!(
            portal::portal_block_positions(&got),
            want,
            "{name}.portalPositions"
        );
        checked += 8;
    }
    assert_eq!(checked, 18 * 8, "eighteen frames, eight fields each");
    eprintln!(
        "portal shape: {checked} comparisons across {} frames",
        ours.len()
    );
}

/// `BlockTags.FIRE`'s membership, against the datapack extract itself.
///
/// The constant in `portal.rs` is a transcription of a data file this crate
/// has no loader for. That is a fine thing to do and a bad thing to leave
/// unchecked: it is exactly the shape of claim that goes stale silently on a
/// version bump. So it is compared against the file.
#[test]
fn fire_tag_matches_the_datapack() {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../oracle-work/vanilla-26.2/generated-data/data/minecraft/tags/block/fire.json");
    assert!(
        rustmc_data::fixture::available(&p, "block tag extract", "ci/vanilla-server.sh data"),
        "no datapack extract"
    );
    let v: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&p).expect("read tag")).expect("parse tag");
    let values: Vec<&str> = v["values"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap())
        .collect();
    assert_eq!(values, portal::FIRE_TAG, "BlockTags.FIRE has moved");
    // And the tag is actually read: fire must be empty and stone must not.
    assert!(portal::is_empty("minecraft:fire"));
    assert!(portal::is_empty("minecraft:soul_fire"));
    assert!(!portal::is_empty("minecraft:stone"));
}

/// Four ways to be wrong, each shown to move a number the golden pins.
#[test]
fn positive_control() {
    let g = golden();
    let by = |n: &str| {
        g["shapes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == n)
            .unwrap()
            .clone()
    };

    // 1. The corners. If the search read them, this frame would be invalid.
    assert!(by("no-corners-3x4")["valid"].as_bool().unwrap());

    // 2. The half-lit count is neither 0 nor width*height, so a count taken
    //    after the search over the finished rectangle disagrees.
    let h = by("min-2x3-x-half-lit");
    assert_eq!(h["numPortalBlocks"].as_i64().unwrap(), 2);
    assert!(!h["complete"].as_bool().unwrap());
    assert!(by("min-2x3-x-lit")["complete"].as_bool().unwrap());

    // 3. The fire row. If `is_empty` dropped the tag, this frame would come
    //    back 0x0 — which is exactly what the golden generator produced on its
    //    first run, before it reloaded the tag.
    let f = by("fire-inside-2x3");
    assert_eq!(
        (f["width"].as_i64().unwrap(), f["valid"].as_bool().unwrap()),
        (2, true)
    );
    let mut without_tag = frame(OBSIDIAN, 2, 3, Axis::X);
    without_tag.put(0, 1, 0, "minecraft:stone");
    let s = portal::find_any_shape(
        &without_tag.getter(),
        -64,
        BlockPos { x: 0, y: 1, z: 0 },
        Axis::X,
    );
    assert!(
        !s.is_valid(),
        "a non-empty, non-fire block must stop the walk"
    );

    // 4. The write order really is the box walk and not the search walk. For
    //    a WEST-facing portal they run opposite ways, so the first position
    //    written is the far column rather than `bottomLeft`.
    let shape = PortalShape {
        axis: Axis::X,
        num_portal_blocks: 0,
        bottom_left: BlockPos { x: 1, y: 1, z: 0 },
        width: 2,
        height: 3,
    };
    let ps = portal::portal_block_positions(&shape);
    assert_eq!(ps.len(), 6);
    assert_eq!(ps[0], BlockPos { x: 0, y: 1, z: 0 }, "x ascending is first");
    assert_ne!(ps[0], shape.bottom_left, "a rightDir walk would start here");
    assert_eq!(ps[5], BlockPos { x: 1, y: 3, z: 0 });
    // And on Z, where rightDir is +z, the first position IS the bottom left.
    let zshape = PortalShape {
        axis: Axis::Z,
        num_portal_blocks: 0,
        bottom_left: BlockPos { x: 0, y: 1, z: 0 },
        width: 2,
        height: 3,
    };
    assert_eq!(
        portal::portal_block_positions(&zshape)[0],
        zshape.bottom_left
    );
}

/// `BlockPos.spiralAround`, against vanilla's own iterator.
///
/// `crates/rustmc-server/golden/spiral_around.json` is four walks captured
/// from the pinned jar by `ci/spiral-golden.sh`: the degenerate r=0, the
/// smallest walk with corners, and the same r=2 walk at a negative asymmetric
/// centre with the two directions given both ways round.
///
/// The order is load-bearing rather than cosmetic:
/// `PortalForcer.createPortal` scores every candidate site by `distSqr` to the
/// clicked block and keeps the smallest, so wherever two sites are
/// equidistant — which around a symmetric centre is most of them — the winner
/// is whichever the spiral reached first.
#[test]
fn the_spiral_is_vanillas_own() {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("golden/spiral_around.json");
    assert!(
        rustmc_data::fixture::available(&p, "spiral golden", "ci/spiral-golden.sh"),
        "no spiral golden"
    );
    let doc: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&p).expect("read")).expect("parse");
    let walks = doc["walks"].as_array().expect("walks");
    assert_eq!(walks.len(), 4, "four walks were captured");

    let dir = |s: &str| match s {
        "east" => forcer::Step::East,
        "south" => forcer::Step::South,
        "west" => forcer::Step::West,
        "north" => forcer::Step::North,
        other => panic!("unknown direction {other}"),
    };

    let mut positions_compared = 0usize;
    for w in walks {
        let c = w["centre"].as_array().expect("centre");
        let centre = BlockPos {
            x: c[0].as_i64().expect("x") as i32,
            y: c[1].as_i64().expect("y") as i32,
            z: c[2].as_i64().expect("z") as i32,
        };
        let radius = w["radius"].as_i64().expect("radius") as i32;
        let first = dir(w["first"].as_str().expect("first"));
        let second = dir(w["second"].as_str().expect("second"));
        let want: Vec<BlockPos> = w["positions"]
            .as_array()
            .expect("positions")
            .iter()
            .map(|p| {
                let a = p.as_array().expect("triple");
                BlockPos {
                    x: a[0].as_i64().expect("x") as i32,
                    y: a[1].as_i64().expect("y") as i32,
                    z: a[2].as_i64().expect("z") as i32,
                }
            })
            .collect();

        let got = forcer::spiral_around(centre, radius, first, second);
        assert_eq!(
            got.len(),
            want.len(),
            "r={radius} {first:?}/{second:?}: {} positions, vanilla walked {}",
            got.len(),
            want.len()
        );
        for (i, (g, wnt)) in got.iter().zip(&want).enumerate() {
            assert_eq!(
                (g.x, g.y, g.z),
                (wnt.x, wnt.y, wnt.z),
                "r={radius} {first:?}/{second:?}: position {i} is {g:?}, vanilla's is {wnt:?}"
            );
        }
        positions_compared += want.len();
    }
    assert_eq!(positions_compared, 60, "1 + 9 + 25 + 25 positions compared");
}

/// The positive control for the test above: the two r=2 walks in the golden
/// differ only in the order of their two directions, and they must come out
/// as **different orders over the same set**.
///
/// Without this, a `spiral_around` that ignored its direction arguments
/// entirely — hard-coding EAST then SOUTH, which is what `createPortal`
/// always passes — would satisfy three of the four rows above and be caught by
/// only one, in a way that reads as an off-by-one rather than as an ignored
/// argument.
#[test]
fn the_direction_arguments_change_the_walk() {
    let centre = BlockPos {
        x: -13,
        y: 7,
        z: 41,
    };
    let a = forcer::spiral_around(centre, 2, forcer::Step::East, forcer::Step::South);
    let b = forcer::spiral_around(centre, 2, forcer::Step::South, forcer::Step::East);
    assert_ne!(a, b, "the direction arguments do nothing");

    let set = |v: &[BlockPos]| {
        v.iter()
            .map(|p| (p.x, p.y, p.z))
            .collect::<std::collections::BTreeSet<_>>()
    };
    assert_eq!(
        set(&a),
        set(&b),
        "a spiral covers the same square either way"
    );
    assert_eq!(a[0], centre, "the first position is the centre");
    assert_eq!(b[0], centre, "the first position is the centre");
}

/// A nether portal **cannot be lit in the End**, and the axis is an RNG draw
/// when the clicked face is up or down.
///
/// `BaseFireBlock.isPortal` opens with `inPortalDimension(level)`, which is
/// `dimension() == OVERWORLD || dimension() == NETHER` and nothing else — so
/// flint and steel in the End places fire whatever the frame looks like.
#[test]
fn a_portal_can_only_be_lit_in_the_overworld_or_the_nether() {
    use rustmc_server::dimension::Dimension;
    assert!(portal::in_portal_dimension(Dimension::Overworld));
    assert!(portal::in_portal_dimension(Dimension::Nether));
    assert!(
        !portal::in_portal_dimension(Dimension::TheEnd),
        "inPortalDimension names two dimensions and the End is not one of them"
    );
}

/// `Direction.values()` is **Y, then Z, then X** — the order `isPortal` scans
/// for an obsidian neighbour in, and the order that decides which neighbour a
/// frame with obsidian on two sides is found by.
///
/// Its ordinal grouping is the same one that once made a golden test assert
/// the `DOWN` face's cursor lay on the x face, so it is written down as a
/// table and checked rather than reconstructed at each call site.
#[test]
fn direction_values_are_y_then_z_then_x() {
    assert_eq!(
        portal::DIRECTION_VALUES,
        [
            (0, -1, 0),
            (0, 1, 0),
            (0, 0, -1),
            (0, 0, 1),
            (-1, 0, 0),
            (1, 0, 0),
        ]
    );
    // Each is a unit step, all six are distinct, and they sum to zero.
    let sum = portal::DIRECTION_VALUES
        .iter()
        .fold((0, 0, 0), |a, d| (a.0 + d.0, a.1 + d.1, a.2 + d.2));
    assert_eq!(sum, (0, 0, 0));
    assert!(portal::DIRECTION_VALUES
        .iter()
        .all(|d| d.0.abs() + d.1.abs() + d.2.abs() == 1));
    assert_eq!(
        portal::DIRECTION_VALUES
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        6
    );
}
