//! `GroundPathNavigation` driven over the grid level: what `moveTo` and
//! `tick` do to the path cursor, the move control and the stuck detector.

use rustmc_data::BlockPos;
use rustmc_entity::ai::{GroundPathNavigation, MoveControl};
use rustmc_entity::host::BlockFacts;
use rustmc_entity::testing::{GridLevel, TestMob};

fn flat_mob(seed: i64) -> TestMob {
    let mut g = GridLevel::new();
    g.floor(-10, -10, 20, 10, 63);
    TestMob::new(g, 0.5, 64.0, 0.5, seed)
}

/// `moveTo(x, y, z, speed)` paths with accuracy 1, so the line to a block
/// five away is five nodes and stops one short; the first `tick` finds
/// the mob within `maxDistanceToWaypoint = 0.75 - 0.6/2 = 0.45` of its own
/// node, advances, and hands node 1's entity position — `x + (int)(1.6) *
/// 0.5 = 1.5` — to the move control at the floor's level.
#[test]
fn move_to_then_tick_hands_the_next_waypoint_to_the_move_control() {
    let mut mob = flat_mob(1);
    let mut nav = GroundPathNavigation::new(&mob);
    let mut mc = MoveControl::new();
    assert!(nav.is_done());
    assert!(nav.move_to(&mut mob, 5.5, 64.0, 0.5, 1.0));
    assert!(!nav.is_done());
    assert_eq!(nav.path().expect("path").node_count(), 5);
    assert_eq!(nav.target_pos(), Some(BlockPos { x: 5, y: 64, z: 0 }));
    assert!(!mc.has_wanted());
    nav.tick(&mut mob, &mut mc);
    assert!(mc.has_wanted());
    assert_eq!(mc.wanted(), (1.5, 64.0, 0.5));
    assert_eq!(mc.speed_modifier(), 1.0);
    assert_eq!(nav.path().expect("path").next_node_index(), 1);
    // The move control then turns and sets the speed on the mob.
    let mut jump = rustmc_entity::ai::JumpControl::new();
    mc.tick(&mut mob, &nav, &mut jump);
    assert!((mob.speed - 0.23).abs() < 1e-6);
    assert!(mob.calls.iter().any(|c| c.starts_with("setSpeed(")));
    // yaw toward +x: atan2(0, 1) * deg - 90 = -90, rotlerp folds to 270.
    assert_eq!(mob.y_rot, 270.0);
}

/// A mob that does not move for more than `STUCK_CHECK_INTERVAL` ticks
/// while carrying a speed is stuck: `tick - lastStuckCheck > 100` first
/// holds on the 101st tick after `moveTo`, and the path is dropped.
#[test]
fn a_mob_that_never_moves_is_stuck_after_101_ticks() {
    let mut mob = flat_mob(1);
    let mut nav = GroundPathNavigation::new(&mob);
    let mut mc = MoveControl::new();
    assert!(nav.move_to(&mut mob, 5.5, 64.0, 0.5, 1.0));
    let mut jump = rustmc_entity::ai::JumpControl::new();
    for _ in 0..100 {
        nav.tick(&mut mob, &mut mc);
        mc.tick(&mut mob, &nav, &mut jump);
        assert!(!nav.is_done());
    }
    assert!(!nav.is_stuck());
    nav.tick(&mut mob, &mut mc);
    assert!(nav.is_stuck());
    assert!(nav.is_done());
}

/// With speed 0 the stuck threshold is 0 and `distance < 0` never holds:
/// a speedless mob is never declared stuck.
#[test]
fn a_speedless_mob_is_never_stuck() {
    let mut mob = flat_mob(1);
    let mut nav = GroundPathNavigation::new(&mob);
    let mut mc = MoveControl::new();
    assert!(nav.move_to(&mut mob, 5.5, 64.0, 0.5, 1.0));
    for _ in 0..300 {
        nav.tick(&mut mob, &mut mc);
    }
    assert!(!nav.is_stuck());
    assert!(!nav.is_done());
}

/// Moving the mob along the path advances the cursor node by node and
/// finishes one block short of the target, which is where accuracy 1
/// put the last node.
#[test]
fn walking_the_path_finishes_it() {
    let mut mob = flat_mob(1);
    let mut nav = GroundPathNavigation::new(&mob);
    let mut mc = MoveControl::new();
    assert!(nav.move_to(&mut mob, 5.5, 64.0, 0.5, 1.0));
    for step in 0..6 {
        nav.tick(&mut mob, &mut mc);
        let (wx, _, wz) = mc.wanted();
        if nav.is_done() {
            break;
        }
        mob.xo = mob.x;
        mob.zo = mob.z;
        mob.x = wx;
        mob.z = wz;
        assert!(step < 6);
    }
    for _ in 0..3 {
        nav.tick(&mut mob, &mut mc);
    }
    assert!(nav.is_done());
    assert_eq!(mob.x, 4.5);
}

/// An unloaded target chunk refuses the path; a target in the air is
/// pulled down to the surface first.
#[test]
fn ground_create_path_refuses_unloaded_chunks_and_finds_the_surface() {
    let mut mob = flat_mob(1);
    mob.level.loaded_chunks = Some(vec![(0, 0), (-1, -1), (0, -1), (-1, 0)]);
    let mut nav = GroundPathNavigation::new(&mob);
    assert!(nav
        .create_path(&mut mob, BlockPos { x: 17, y: 64, z: 0 }, 0)
        .is_none());
    let p = nav
        .create_path(&mut mob, BlockPos { x: 5, y: 70, z: 0 }, 0)
        .expect("surface target");
    assert_eq!(p.target(), BlockPos { x: 5, y: 64, z: 0 });
    assert!(p.can_reach());
    // Below the surface: the first non-solid above the solid column.
    let p = nav
        .create_path(&mut mob, BlockPos { x: 5, y: 63, z: 0 }, 0)
        .expect("buried target");
    assert_eq!(p.target(), BlockPos { x: 5, y: 64, z: 0 });
    // Unless the navigation may path to targets below the surface.
    nav.set_can_path_to_targets_below_surface(true);
    let p = nav
        .create_path(&mut mob, BlockPos { x: 5, y: 63, z: 0 }, 0)
        .expect("path");
    assert_eq!(p.target(), BlockPos { x: 5, y: 63, z: 0 });
    assert!(!p.can_reach());
}

/// `avoidSun` truncates a path at the first node under open sky when the
/// mob itself is covered.
#[test]
fn avoid_sun_truncates_at_the_first_uncovered_node() {
    let mut mob = flat_mob(1);
    // A roof over x in -1..=2 at y = 70.
    mob.level.fill(-1, 70, -10, 2, 70, 10, BlockFacts::STONE);
    let mut nav = GroundPathNavigation::new(&mob);
    nav.set_avoid_sun(true);
    assert!(nav.move_to(&mut mob, 5.5, 64.0, 0.5, 1.0));
    let p = nav.path().expect("path");
    assert_eq!(
        p.node_count(),
        3,
        "{:?}",
        p.nodes().iter().map(|n| n.x).collect::<Vec<_>>()
    );
}

/// Deferred golden, by name: `PathNavigation`'s vectors need a `Mob` in a
/// `ServerLevel` and are not takeable without a running server.
#[test]
fn path_navigation_golden_needs_a_running_server() {}
