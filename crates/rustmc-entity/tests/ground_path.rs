//! The ground evaluator and the A* over a small grid level: shapes a
//! rewrite gets wrong, asserted on the path they produce.
//!
//! These are not goldens. The jar's `WalkNodeEvaluator` needs a `Mob` in a
//! `Level` to run, which needs a server, so its vectors are named as
//! deferred by `walk_node_evaluator_golden_needs_a_running_server` below
//! rather than approximated here. `PathFinder`, `BinaryHeap`, `Node` and
//! `Path` **are** golden-checked, over a synthetic evaluator, in
//! `golden_mob_path.rs`.

use rustmc_data::BlockPos;
use rustmc_entity::host::BlockFacts;
use rustmc_entity::pathfinder::{
    NodeEvaluator, PathFinder, PathType, PathfindingContext, WalkNodeEvaluator,
};
use rustmc_entity::testing::{GridLevel, TestMob};

fn flat(x0: i32, z0: i32, x1: i32, z1: i32) -> GridLevel {
    let mut g = GridLevel::new();
    g.floor(x0, z0, x1, z1, 63);
    g
}

fn find(mob: &TestMob, target: BlockPos, accuracy: i32) -> Option<rustmc_entity::pathfinder::Path> {
    let ctx = PathfindingContext::new(mob, &mob.level, None);
    let mut pf = PathFinder::new(WalkNodeEvaluator::new(), 256);
    pf.find_path(&ctx, &[target], 16.0, accuracy, 1.0)
}

fn coords(p: &rustmc_entity::pathfinder::Path) -> Vec<(i32, i32, i32)> {
    p.nodes().iter().map(|n| (n.x, n.y, n.z)).collect()
}

/// On open floor the path is the straight line, the start node is the
/// mob's own block and every node is WALKABLE.
#[test]
fn a_straight_line_over_flat_floor() {
    let mob = TestMob::new(flat(-5, -5, 15, 5), 0.5, 64.0, 0.5, 1);
    let p = find(&mob, BlockPos { x: 5, y: 64, z: 0 }, 0).expect("a path");
    assert!(p.can_reach());
    assert_eq!(
        coords(&p),
        vec![
            (0, 64, 0),
            (1, 64, 0),
            (2, 64, 0),
            (3, 64, 0),
            (4, 64, 0),
            (5, 64, 0)
        ]
    );
    assert!(p.nodes().iter().all(|n| n.path_type == PathType::Walkable));
    assert_eq!(p.dist_to_target(), 0.0);
}

/// A one-block step is jumped (maxUp = floor(max(1, 0.6)) = 1); the path
/// climbs onto it.
#[test]
fn a_one_block_step_is_climbed() {
    let mut g = flat(-5, -5, 15, 5);
    g.fill(3, 64, -5, 15, 64, 5, BlockFacts::STONE);
    let mob = TestMob::new(g, 0.5, 64.0, 0.5, 1);
    let p = find(&mob, BlockPos { x: 5, y: 65, z: 0 }, 0).expect("a path");
    assert!(p.can_reach());
    assert_eq!(
        coords(&p),
        vec![
            (0, 64, 0),
            (1, 64, 0),
            (2, 64, 0),
            (3, 65, 0),
            (4, 65, 0),
            (5, 65, 0)
        ]
    );
}

/// A two-block wall across the whole floor cannot be climbed: the search
/// exhausts its nodes and returns the closest approach, unreached, with
/// distToTarget the Manhattan gap from its last node.
#[test]
fn a_two_block_wall_is_not_climbed() {
    let mut g = flat(-5, -5, 15, 5);
    g.fill(3, 64, -5, 3, 65, 5, BlockFacts::STONE);
    let mob = TestMob::new(g, 0.5, 64.0, 0.5, 1);
    let p = find(&mob, BlockPos { x: 5, y: 64, z: 0 }, 0).expect("a best-effort path");
    assert!(!p.can_reach());
    let last = p.end_node().expect("nodes");
    assert!(last.x <= 2, "stopped before the wall, got {:?}", coords(&p));
    assert_eq!(
        p.dist_to_target(),
        last.distance_manhattan_pos(BlockPos { x: 5, y: 64, z: 0 })
    );
}

/// A gap in the wall is found, and the path goes through it rather than
/// stopping short.
#[test]
fn a_gap_in_the_wall_is_used() {
    let mut g = flat(-5, -5, 15, 5);
    g.fill(3, 64, -5, 3, 65, 5, BlockFacts::STONE);
    // Remove the wall at z = 3 (two blocks high) for a gap.
    g.blocks.remove(&BlockPos { x: 3, y: 64, z: 3 });
    g.blocks.remove(&BlockPos { x: 3, y: 65, z: 3 });
    let mob = TestMob::new(g, 0.5, 64.0, 0.5, 1);
    let p = find(&mob, BlockPos { x: 5, y: 64, z: 0 }, 0).expect("a path");
    assert!(p.can_reach(), "{:?}", coords(&p));
    assert!(coords(&p).contains(&(3, 64, 3)), "{:?}", coords(&p));
    assert_eq!(p.end_node().map(|n| (n.x, n.y, n.z)), Some((5, 64, 0)));
}

/// A drop of one block is taken; a drop of more than `maxFallDistance`
/// is blocked.
#[test]
fn drops_respect_max_fall_distance() {
    let mut g = GridLevel::new();
    g.floor(-5, -5, 2, 5, 63);
    g.floor(3, -5, 15, 5, 62);
    let mob = TestMob::new(g.clone(), 0.5, 64.0, 0.5, 1);
    let p = find(&mob, BlockPos { x: 5, y: 63, z: 0 }, 0).expect("a path");
    assert!(p.can_reach());
    assert_eq!(p.end_node().map(|n| (n.x, n.y, n.z)), Some((5, 63, 0)));

    let mut deep = GridLevel::new();
    deep.floor(-5, -5, 2, 5, 63);
    deep.floor(3, -5, 15, 5, 58);
    let mob = TestMob::new(deep, 0.5, 64.0, 0.5, 1);
    let p = find(&mob, BlockPos { x: 5, y: 59, z: 0 }, 0).expect("a best-effort path");
    assert!(!p.can_reach());
}

/// Accuracy: a target one block away Manhattan is reached with accuracy 1
/// without stepping onto it.
#[test]
fn accuracy_stops_short() {
    let mob = TestMob::new(flat(-5, -5, 15, 5), 0.5, 64.0, 0.5, 1);
    let p = find(&mob, BlockPos { x: 5, y: 64, z: 0 }, 1).expect("a path");
    assert!(p.can_reach());
    assert_eq!(p.end_node().map(|n| (n.x, n.y, n.z)), Some((4, 64, 0)));
}

/// The mob's own block classification: standing on stone with air above
/// is WALKABLE, a block in the air is OPEN, stone is BLOCKED, water
/// beside the path is WATER_BORDER on the neighbour.
#[test]
fn walk_evaluator_classifies_the_column() {
    let mut g = flat(-5, -5, 5, 5);
    g.set(2, 64, 0, BlockFacts::WATER);
    let mob = TestMob::new(g, 0.5, 64.0, 0.5, 1);
    let ctx = PathfindingContext::new(&mob, &mob.level, None);
    let mut e = WalkNodeEvaluator::new();
    NodeEvaluator::prepare(&mut e, &ctx);
    assert_eq!(e.get_cached_path_type(&ctx, 0, 64, 0), PathType::Walkable);
    assert_eq!(e.get_cached_path_type(&ctx, 0, 66, 0), PathType::Open);
    assert_eq!(e.get_cached_path_type(&ctx, 0, 63, 0), PathType::Blocked);
    assert_eq!(
        e.get_cached_path_type(&ctx, 1, 64, 0),
        PathType::WaterBorder
    );
    assert_eq!(e.get_cached_path_type(&ctx, 2, 64, 0), PathType::Water);
    // A 0.6 x 1.95 mob occupies 1 x 2 x 1 blocks.
    assert_eq!(
        (
            e.base.entity_width,
            e.base.entity_height,
            e.base.entity_depth
        ),
        (1, 2, 1)
    );
}

/// Deferred golden, by name: `WalkNodeEvaluator`'s vectors cannot be
/// taken from the jar without a `Mob` in a `Level`, which needs a running
/// server. Until a server-backed golden exists this file's grid cases are
/// the evidence, and they are this port's reading, not the jar's.
#[test]
fn walk_node_evaluator_golden_needs_a_running_server() {
    // Nothing to assert; the name is the record.
}
