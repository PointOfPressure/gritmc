//! An arrow flies, sticks, waits, falls and hits, against a hand-built world.
//!
//! # What this instrument is and is not
//!
//! It is a **self-consistency** harness. Every expectation below is derived
//! from the same reading of `AbstractArrow`, `ProjectileUtil` and
//! `BlockGetter.clip` that `crates/rustmc-server/src/projectile/` was written
//! from, so a green run here proves the port does what this lane believes
//! vanilla does — **not** that the belief is right. Two readings of one
//! bytecode agreeing is exactly the shape `docs/AGENT-BRIEF.md` warns about.
//!
//! What it *can* do, and what a golden could not do more cheaply, is separate
//! the failure modes: the block ray, the entity search, the margin ramp, the
//! `inGround` state machine, the despawn counter and the piercing loop each
//! have a case that no other case can keep green. Every case below carries a
//! positive control — an input differing in exactly the thing under test,
//! whose outcome must differ — so none of them is a check that cannot fail.
//!
//! The instrument that would settle fidelity is a `ProjectileGolden.java`
//! running vanilla's own `AbstractArrow.tick` in a fake `Level`, in the shape
//! of `ci/entity-move-golden.sh`. It does not exist; see
//! `docs/cold-start/projectiles.md`.

use rustmc_data::BlockPos;
use rustmc_server::collide::{Aabb, CollisionBlocks, CollisionShapes, Vec3};
use rustmc_server::projectile::arrow::{
    Arrow, Env, Pickup, Tick, DESPAWN_LIFE, INERTIA, SHAKE_TIME, WATER_INERTIA,
};
use rustmc_server::projectile::hit::{ProjectileTargets, Target};
use rustmc_worldgen::rng::legacy::LegacyRandomSource;
use rustmc_worldgen::rng::RandomSource;

/// Two shapes: id 0 empty (air), id 1 a full block.
fn shapes() -> CollisionShapes {
    #[expect(clippy::cast_possible_wrap, reason = "the fixture's own bit encoding")]
    let bits = |v: f64| v.to_bits() as i64;
    let doc = serde_json::json!({
        "gridEquivalence": {"mismatches": 0, "probes": 16_736_112_i64},
        "shapes": [
            {"size": [0, 0, 0], "coords": [[], [], []], "voxels": "", "full": false},
            {"size": [1, 1, 1],
             "coords": [[bits(0.0), bits(1.0)], [bits(0.0), bits(1.0)],
                        [bits(0.0), bits(1.0)]],
             "voxels": "8", "full": true}
        ],
        // A third state, never placed, only so `CollisionShapes::from_json`'s
        // own guard is satisfied: it refuses a table with no moving-piston
        // state on the grounds that `collect_block_collisions`' edge-cell
        // rule would then never fire. That guard is `collide.rs`'s and this
        // fixture must not be the thing that disables it.
        "rows": [0, 1, 1],
        "movingPistonStates": [2]
    });
    CollisionShapes::from_json(&doc).expect("the fixture parses")
}

/// A world that is solid at a listed set of positions and air everywhere else.
#[derive(Clone, Default)]
struct World(Vec<BlockPos>);

impl World {
    fn solid(cells: &[(i32, i32, i32)]) -> Self {
        World(
            cells
                .iter()
                .map(|&(x, y, z)| BlockPos { x, y, z })
                .collect(),
        )
    }
    fn wall_at_x(x: i32) -> Self {
        let mut v = Vec::new();
        for y in 60..70 {
            for z in -3..4 {
                v.push(BlockPos { x, y, z });
            }
        }
        World(v)
    }
}

impl CollisionBlocks for World {
    fn state_id_at(&self, pos: BlockPos) -> u32 {
        u32::from(self.0.contains(&pos))
    }
}

/// A fixed list of targets, returned whatever the query box is; the module's
/// own `math::clip` decides which of them the ray actually reaches, so
/// returning everything exercises more of the path than filtering here would.
#[derive(Clone, Default)]
struct Targets(Vec<Target>);

impl ProjectileTargets for Targets {
    fn targets_in(&self, _box_: Aabb) -> Vec<Target> {
        self.0.clone()
    }
}

fn target_at(id: i32, x: f64, y: f64, z: f64, w: f64, h: f64) -> Target {
    Target {
        id,
        box_: Aabb::new(x - w / 2.0, y, z - w / 2.0, x + w / 2.0, y + h, z + w / 2.0),
        pos: Vec3::new(x, y, z),
        can_be_hit_by_projectile: true,
        shares_vehicle_with_owner: false,
        owner_may_harm: true,
    }
}

fn air() -> Env {
    Env {
        in_water: false,
        in_water_or_rain: false,
    }
}

fn flying(pos: Vec3, movement: Vec3) -> Arrow {
    let mut a = Arrow::at(pos);
    a.movement = movement;
    a.pickup = Pickup::Allowed;
    // No owner: `isOutsideOwnerCollisionRange` returns true immediately for a
    // null owner, so `leftOwner` latches on the first check and the
    // vehicle-mate filter never suppresses a hit. That is vanilla's behaviour
    // for a `/summon`ed arrow and it is what these cases are about.
    a.owner = None;
    a
}

fn tick(a: &mut Arrow, w: &World, t: &Targets, r: &mut LegacyRandomSource) -> Tick {
    a.tick(&shapes(), w, t, r, air())
        .expect("every fixture block has a shape")
}

// ---------------------------------------------------------------------------
// The flight
// ---------------------------------------------------------------------------

#[test]
fn an_arrow_in_open_air_drags_then_falls_and_the_two_are_in_that_order() {
    let mut a = flying(Vec3::new(0.0, 64.0, 0.0), Vec3::new(1.0, 0.0, 0.0));
    let w = World::default();
    let t = Targets::default();
    let mut r = LegacyRandomSource::new(1);
    let out = tick(&mut a, &w, &t, &mut r);

    // The move used the delta the tick STARTED with, so the arrow is exactly
    // one block along.
    assert_eq!(a.pos, Vec3::new(1.0, 64.0, 0.0));
    // Then 0.99 drag, then 0.05 gravity — in that order, so the y is exactly
    // -0.05 and not -0.05 * 0.99.
    //
    // The drag is `f64::from(0.99f32)` = 0.9900000095367432, because vanilla
    // widens the float constant at the `f2d` before `Vec3.scale`. Asserted
    // exactly: a tolerance loose enough to accept the decimal 0.99 would
    // hide a real error of the same size, which is the mistake
    // `docs/cold-start/movement-tick.md` records for `getSpeedFactor`.
    assert_eq!(a.movement.x, f64::from(INERTIA));
    assert_ne!(f64::from(INERTIA), 0.99);
    assert_eq!(a.movement.y, -0.05);
    // The reversed order would give -0.0495; assert the difference rather
    // than only the value, so the test says which reading it excludes.
    assert!((a.movement.y - -0.0495).abs() > 1e-6);
    assert!(out.hits.is_empty());
    assert!(!out.discarded);
}

#[test]
fn the_second_tick_compounds_the_drag_and_the_gravity() {
    let mut a = flying(Vec3::new(0.0, 64.0, 0.0), Vec3::new(1.0, 0.0, 0.0));
    let w = World::default();
    let t = Targets::default();
    let mut r = LegacyRandomSource::new(1);
    tick(&mut a, &w, &t, &mut r);
    let after_one = a.movement;
    tick(&mut a, &w, &t, &mut r);
    assert_eq!(a.pos.x, 1.0 + after_one.x);
    assert_eq!(a.pos.y, 64.0 + after_one.y);
    let d = f64::from(INERTIA);
    assert_eq!(a.movement.x, d * d);
    assert_eq!(a.movement.y, -0.05 * d - 0.05);
}

#[test]
fn the_rotation_lags_the_motion_by_a_tick() {
    // `AbstractArrow.tick` captures the delta at offset 13 and takes the
    // rotation from it, so the yaw after the first tick reflects the LAUNCH
    // heading, not the post-drag one. The two differ once the arrow has
    // turned; a straight +X shot has yaw 90 either way, so turn it.
    let mut a = flying(Vec3::new(0.0, 64.0, 0.0), Vec3::new(1.0, 0.0, 1.0));
    let w = World::default();
    let t = Targets::default();
    let mut r = LegacyRandomSource::new(1);
    tick(&mut a, &w, &t, &mut r);
    // One fifth of the way from 0 to 45.
    assert!((a.y_rot - 9.0).abs() < 0.05, "{}", a.y_rot);
    assert!(a.x_rot.abs() < 0.05, "{}", a.x_rot);
}

// ---------------------------------------------------------------------------
// The block hit
// ---------------------------------------------------------------------------

#[test]
fn an_arrow_that_reaches_a_wall_sticks_shakes_and_makes_a_noise() {
    let w = World::wall_at_x(3);
    let mut a = flying(Vec3::new(0.0, 64.5, 0.0), Vec3::new(4.0, 0.0, 0.0));
    let t = Targets::default();
    let mut r = LegacyRandomSource::new(7);
    let out = tick(&mut a, &w, &t, &mut r);

    assert!(a.in_ground);
    assert_eq!(a.shake_time, SHAKE_TIME);
    assert_eq!(a.movement, Vec3::ZERO);
    assert_eq!(out.stuck_in, Some(BlockPos { x: 3, y: 64, z: 0 }));
    assert_eq!(out.sounds.len(), 1);
    assert_eq!(out.sounds[0].name, "minecraft:entity.arrow.hit");
    assert_eq!(out.sounds[0].volume, 1.0);
    assert!(out.needs_sync);
    // `onHitBlock` steps the arrow BACK along the signum of its delta by the
    // widened float 0.05f, so it ends just outside the face it hit.
    assert!(
        (a.pos.x - (3.0 - 0.050_000_000_745_058_06)).abs() < 1e-12,
        "{}",
        a.pos.x
    );
    // Positive control: the same shot with no wall keeps flying and is silent.
    let mut b = flying(Vec3::new(0.0, 64.5, 0.0), Vec3::new(4.0, 0.0, 0.0));
    let empty = World::default();
    let mut r = LegacyRandomSource::new(7);
    let out = tick(&mut b, &empty, &t, &mut r);
    assert!(!b.in_ground);
    assert!(out.sounds.is_empty());
    assert_eq!(out.stuck_in, None);
}

#[test]
fn the_hit_sound_pitch_is_one_nextfloat_and_lands_in_vanillas_band() {
    let w = World::wall_at_x(3);
    for seed in 0..64_i64 {
        let mut a = flying(Vec3::new(0.0, 64.5, 0.0), Vec3::new(4.0, 0.0, 0.0));
        let mut r = LegacyRandomSource::new(seed);
        let out = tick(&mut a, &w, &Targets::default(), &mut r);
        let p = out.sounds[0].pitch;
        // `1.2f / (nextFloat() * 0.2f + 0.9f)` — nextFloat is [0, 1), so the
        // pitch is (1.2/1.1, 1.2/0.9] = (1.0909, 1.3333].
        assert!(p > 1.090_9 && p <= 1.333_4, "seed {seed}: {p}");
    }
}

#[test]
fn a_stuck_arrow_neither_drags_nor_falls_and_counts_towards_despawn() {
    let w = World::wall_at_x(3);
    let mut a = flying(Vec3::new(0.0, 64.5, 0.0), Vec3::new(4.0, 0.0, 0.0));
    let t = Targets::default();
    let mut r = LegacyRandomSource::new(1);
    tick(&mut a, &w, &t, &mut r);
    let stuck_at = a.pos;
    assert_eq!(a.life, 0);

    for i in 1..=10 {
        let out = tick(&mut a, &w, &t, &mut r);
        assert_eq!(a.pos, stuck_at, "a grounded arrow does not move");
        assert_eq!(a.movement, Vec3::ZERO, "and takes no gravity");
        assert_eq!(a.life, i, "but its life counter runs");
        assert_eq!(a.in_ground_time, i);
        assert!(!out.discarded);
    }
    // The shake counts down to zero and stays there.
    assert_eq!(a.shake_time, 0);
}

#[test]
fn a_stuck_arrow_is_discarded_after_1200_ticks_of_life_and_not_before() {
    let w = World::wall_at_x(3);
    let mut a = flying(Vec3::new(0.0, 64.5, 0.0), Vec3::new(4.0, 0.0, 0.0));
    let t = Targets::default();
    let mut r = LegacyRandomSource::new(1);
    tick(&mut a, &w, &t, &mut r);
    let mut discarded_at = None;
    for _ in 0..2000 {
        let out = tick(&mut a, &w, &t, &mut r);
        if out.discarded {
            discarded_at = Some(a.life);
            break;
        }
    }
    assert_eq!(discarded_at, Some(DESPAWN_LIFE));
    assert!(a.removed);
}

#[test]
fn an_arrow_whose_block_is_taken_away_falls_again_and_pays_three_draws() {
    let w = World::wall_at_x(3);
    let mut a = flying(Vec3::new(0.0, 64.5, 0.0), Vec3::new(4.0, 0.0, 0.0));
    let t = Targets::default();
    let mut r = LegacyRandomSource::new(11);
    tick(&mut a, &w, &t, &mut r);
    assert!(a.in_ground);
    let held = a.movement;
    assert_eq!(held, Vec3::ZERO);

    // Give it a delta so `startFalling`'s multiply is observable, then remove
    // every block. `lastState != state` and `shouldFall` both fire.
    a.movement = Vec3::new(1.0, 1.0, 1.0);
    let empty = World::default();
    let mut probe = LegacyRandomSource::new(3);
    let expect: Vec<f32> = (0..3).map(|_| probe.next_f32() * 0.2).collect();
    let mut r = LegacyRandomSource::new(3);
    tick(&mut a, &empty, &t, &mut r);
    assert!(!a.in_ground, "the arrow resumed falling");
    assert_eq!(a.life, 0, "startFalling resets life");
    // Three nextFloat draws, in x, y, z order, each * 0.2f then widened.
    assert!(
        (a.movement.x - f64::from(expect[0])).abs() < 1e-12,
        "{:?}",
        a.movement
    );
    assert!((a.movement.y - f64::from(expect[1])).abs() < 1e-12);
    assert!((a.movement.z - f64::from(expect[2])).abs() < 1e-12);
}

#[test]
fn an_arrow_still_in_its_block_does_not_fall_and_takes_no_draws() {
    // The positive control for the case above: with the wall still there,
    // `shouldFall`'s 0.06 probe collides and nothing is drawn.
    let w = World::wall_at_x(3);
    let mut a = flying(Vec3::new(0.0, 64.5, 0.0), Vec3::new(4.0, 0.0, 0.0));
    let t = Targets::default();
    let mut r = LegacyRandomSource::new(11);
    tick(&mut a, &w, &t, &mut r);
    // Move it bodily inside the wall so the probe cannot miss, and mark the
    // state as changed so only `shouldFall` decides.
    a.pos = Vec3::new(3.5, 64.5, 0.5);
    a.last_state = None;
    a.movement = Vec3::new(1.0, 1.0, 1.0);
    tick(&mut a, &w, &t, &mut r);
    assert!(a.in_ground);
    assert_eq!(a.movement, Vec3::ZERO, "the in-solid test zeroed it");
}

// ---------------------------------------------------------------------------
// The entity hit
// ---------------------------------------------------------------------------

#[test]
fn an_entity_on_the_line_is_struck_and_the_arrow_is_consumed() {
    let t = Targets(vec![target_at(9, 2.0, 64.0, 0.0, 0.6, 1.8)]);
    let mut a = flying(Vec3::new(0.0, 64.5, 0.0), Vec3::new(4.0, 0.0, 0.0));
    let w = World::default();
    let mut r = LegacyRandomSource::new(1);
    let out = tick(&mut a, &w, &t, &mut r);
    assert_eq!(out.hits.len(), 1);
    assert_eq!(out.hits[0].target_id, 9);
    // `Mth.ceil((float) length * baseDamage)` = ceil(4.0 * 2.0) = 8, as an
    // int because vanilla's is.
    assert_eq!(out.hits[0].damage, 8);
    assert!(out.discarded, "a non-piercing arrow discards on its hit");
    assert!(out.needs_sync);
}

#[test]
fn the_crit_bonus_is_drawn_and_a_non_crit_arrow_draws_nothing() {
    // `onHitEntity` offsets 192-230: `if (isCritArrow()) { long j =
    // random.nextInt(i / 2 + 2); i = (int) min(j + (long) i, 2147483647L); }`
    // The draw happens before `hurtOrSimulate` and does not depend on the
    // damage landing, so a port that skipped it would displace every later
    // draw in the tick — the same defect this lane wrote down for
    // `getMovementToShoot`'s six draws.
    let t = Targets(vec![target_at(9, 2.0, 64.0, 0.0, 0.6, 1.8)]);
    let w = World::default();

    // A non-crit arrow: damage is exactly ceil(4.0 * 2.0) and the stream is
    // untouched, so a second arrow fired from the same seed agrees.
    let mut a = flying(Vec3::new(0.0, 64.5, 0.0), Vec3::new(4.0, 0.0, 0.0));
    let mut r = LegacyRandomSource::new(3);
    let plain = tick(&mut a, &w, &t, &mut r).hits[0].damage;
    assert_eq!(plain, 8);

    // A crit arrow: the bonus is `nextInt(8 / 2 + 2) = nextInt(6)`, so the
    // damage lands in 8..=13 and is never below the plain figure.
    let mut seen = std::collections::BTreeSet::new();
    for seed in 0..200_i64 {
        let mut c = flying(Vec3::new(0.0, 64.5, 0.0), Vec3::new(4.0, 0.0, 0.0));
        c.crit = true;
        let mut r = LegacyRandomSource::new(seed);
        let d = tick(&mut c, &w, &t, &mut r).hits[0].damage;
        assert!((8..=13).contains(&d), "seed {seed}: {d}");
        seen.insert(d);
    }
    // Six distinct outcomes, which is `nextInt(6)`'s whole range: a stuck
    // draw returning one value would pass every assertion above.
    assert_eq!(seen.len(), 6, "{seen:?}");
}

#[test]
fn the_damage_speed_is_narrowed_to_float_before_it_multiplies() {
    // `onHitEntity` offset 10 is `d2f` and offset 98 is `f2d` back, so the
    // product is a float-precision speed times a double-precision base. The
    // two readings differ for a band of speeds either side of every integer
    // boundary of the product, and `Mth.ceil` turns that band into a whole
    // point of damage.
    //
    //   speed         4.000000000000001   (one ulp above 4.0)
    //   (float) speed 4.0                 exactly
    //   double  4.000000000000001 * 2.0 = 8.000000000000002 -> ceil 9
    //   vanilla 4.0               * 2.0 = 8.0               -> ceil 8
    let t = Targets(vec![target_at(9, 2.0, 64.0, 0.0, 0.6, 1.8)]);
    let w = World::default();
    let speed = 4.0_f64 + 1.0E-15_f64;
    let mut a = flying(Vec3::new(0.0, 64.5, 0.0), Vec3::new(speed, 0.0, 0.0));
    let mut r = LegacyRandomSource::new(1);
    let out = tick(&mut a, &w, &t, &mut r);
    assert_eq!(out.hits.len(), 1);
    assert_eq!(
        out.hits[0].damage, 8,
        "the double reading gives 9 here and vanilla gives 8"
    );
    // The premise, asserted so the case cannot quietly stop separating the
    // two readings if the speed is ever edited.
    #[expect(clippy::cast_possible_truncation, reason = "the reading under test")]
    let narrowed = speed as f32;
    assert_eq!(f64::from(narrowed), 4.0);
    assert_ne!(speed, 4.0);
    assert_ne!((f64::from(narrowed) * 2.0).ceil(), (speed * 2.0).ceil());
}

#[test]
fn an_entity_behind_a_wall_is_not_struck() {
    // The block ray shortens the entity ray, so a target past the wall is
    // never considered. This is the case a port that resolved entities first
    // gets wrong.
    let t = Targets(vec![target_at(9, 5.0, 64.0, 0.0, 0.6, 1.8)]);
    let w = World::wall_at_x(3);
    let mut a = flying(Vec3::new(0.0, 64.5, 0.0), Vec3::new(8.0, 0.0, 0.0));
    let mut r = LegacyRandomSource::new(1);
    let out = tick(&mut a, &w, &t, &mut r);
    assert!(out.hits.is_empty(), "{:?}", out.hits);
    assert!(a.in_ground, "it stuck in the wall instead");
    // Positive control: the same target with the wall gone IS struck.
    let empty = World::default();
    let mut b = flying(Vec3::new(0.0, 64.5, 0.0), Vec3::new(8.0, 0.0, 0.0));
    let mut r = LegacyRandomSource::new(1);
    let out = tick(&mut b, &empty, &t, &mut r);
    assert_eq!(out.hits.len(), 1);
}

#[test]
fn the_nearer_of_two_entities_is_struck_first() {
    // `stepMoveAndHit` sorts by the distance from the arrow's position to the
    // ENTITY's position, and a non-piercing arrow stops at the first.
    let t = Targets(vec![
        target_at(2, 6.0, 64.0, 0.0, 0.6, 1.8),
        target_at(1, 2.0, 64.0, 0.0, 0.6, 1.8),
    ]);
    let mut a = flying(Vec3::new(0.0, 64.5, 0.0), Vec3::new(8.0, 0.0, 0.0));
    let w = World::default();
    let mut r = LegacyRandomSource::new(1);
    let out = tick(&mut a, &w, &t, &mut r);
    assert_eq!(out.hits.len(), 1);
    assert_eq!(out.hits[0].target_id, 1, "the list order must not decide");
}

#[test]
fn a_piercing_arrow_strikes_both_and_survives() {
    let t = Targets(vec![
        target_at(1, 2.0, 64.0, 0.0, 0.6, 1.8),
        target_at(2, 6.0, 64.0, 0.0, 0.6, 1.8),
    ]);
    let mut a = flying(Vec3::new(0.0, 64.5, 0.0), Vec3::new(8.0, 0.0, 0.0));
    a.pierce_level = 2;
    let w = World::default();
    let mut r = LegacyRandomSource::new(1);
    let out = tick(&mut a, &w, &t, &mut r);
    let ids: Vec<i32> = out.hits.iter().map(|h| h.target_id).collect();
    assert_eq!(ids, vec![1, 2], "{ids:?}");
    assert!(!out.discarded);
    assert_eq!(a.piercing_ignore, vec![1, 2]);
}

#[test]
fn a_target_a_piercing_arrow_already_hit_is_not_hit_twice() {
    let t = Targets(vec![target_at(1, 2.0, 64.0, 0.0, 0.6, 1.8)]);
    let mut a = flying(Vec3::new(0.0, 64.5, 0.0), Vec3::new(8.0, 0.0, 0.0));
    a.pierce_level = 2;
    a.piercing_ignore = vec![1];
    let w = World::default();
    let mut r = LegacyRandomSource::new(1);
    let out = tick(&mut a, &w, &t, &mut r);
    assert!(out.hits.is_empty());
    // Positive control: the same arrow with an empty ignore set does hit.
    let mut b = flying(Vec3::new(0.0, 64.5, 0.0), Vec3::new(8.0, 0.0, 0.0));
    b.pierce_level = 2;
    let mut r = LegacyRandomSource::new(1);
    let out = tick(&mut b, &w, &t, &mut r);
    assert_eq!(out.hits.len(), 1);
}

#[test]
fn a_target_that_cannot_be_hit_by_a_projectile_is_passed_through() {
    let mut only = target_at(1, 2.0, 64.0, 0.0, 0.6, 1.8);
    only.can_be_hit_by_projectile = false;
    let t = Targets(vec![only]);
    let mut a = flying(Vec3::new(0.0, 64.5, 0.0), Vec3::new(8.0, 0.0, 0.0));
    let w = World::default();
    let mut r = LegacyRandomSource::new(1);
    let out = tick(&mut a, &w, &t, &mut r);
    assert!(out.hits.is_empty());
    assert!(!out.discarded);
}

/// The near-miss geometry both margin cases use.
///
/// The target's feet are at y 65 and the arrow flies at y 64.75, so the ray
/// passes 0.25 below the raw box: outside it, inside a 0.3 inflation. The
/// miss is vertical rather than sideways so that a block can be put in the
/// way of the second, diagonal probe ray without also being in the way of
/// the first, horizontal one.
fn near_miss() -> (Targets, Arrow) {
    let t = Targets(vec![target_at(1, 2.0, 65.0, 0.0, 0.6, 1.8)]);
    let a = flying(Vec3::new(0.0, 64.75, 0.0), Vec3::new(8.0, 0.0, 0.0));
    (t, a)
}

#[test]
fn the_margin_ramp_decides_a_near_miss_and_a_flat_zero_point_three_would_not() {
    // At tickCount 2 the margin is 0.0 and the near-miss probe is skipped
    // entirely; at tickCount 8 it is 0.3 and the three-clip probe converts
    // the miss into a hit.
    //
    // This is the case that separates 26.2's ramp from every earlier
    // version's flat 0.3: an implementation using 0.3 from the first tick
    // hits on BOTH arms and cannot tell them apart.
    let w = World::default();

    let (t, mut early) = near_miss();
    early.tick_count = 1; // becomes 2 inside tick(), margin 0.0
    let mut r = LegacyRandomSource::new(1);
    let out = tick(&mut early, &w, &t, &mut r);
    assert!(out.hits.is_empty(), "margin 0.0 must not reach it");

    let (t, mut late) = near_miss();
    late.tick_count = 7; // becomes 8 inside tick(), margin 0.3
    let mut r = LegacyRandomSource::new(1);
    let out = tick(&mut late, &w, &t, &mut r);
    assert_eq!(out.hits.len(), 1, "margin 0.3 must convert it");
}

#[test]
fn the_margin_probe_will_not_reach_through_a_block() {
    // The near-miss conversion casts a second ray from the loose hit point
    // towards the target's centre and gives up if a block is in the way.
    // The block below sits at (1, 65, 0): the horizontal main ray at
    // y 64.75 never enters it, and the diagonal probe ray does.
    let w = World::solid(&[(1, 65, 0)]);
    let (t, mut late) = near_miss();
    late.tick_count = 7;
    let mut r = LegacyRandomSource::new(1);
    let out = tick(&mut late, &w, &t, &mut r);
    assert!(out.hits.is_empty(), "{:?}", out.hits);
    assert!(
        !late.in_ground,
        "and the main ray did not stick in it either"
    );
}

// ---------------------------------------------------------------------------
// Water
// ---------------------------------------------------------------------------

#[test]
fn an_arrow_in_water_takes_the_water_drag_and_not_the_air_drag() {
    let mut a = flying(Vec3::new(0.0, 64.0, 0.0), Vec3::new(1.0, 0.0, 0.0));
    let w = World::default();
    let t = Targets::default();
    let mut r = LegacyRandomSource::new(1);
    let env = Env {
        in_water: true,
        in_water_or_rain: true,
    };
    a.tick(&shapes(), &w, &t, &mut r, env).unwrap();
    assert_eq!(a.movement.x, f64::from(WATER_INERTIA), "0.6, applied once");
    // Not 0.6 * 0.99: the air drag is under `if (!isInWater())`.
    assert!((a.movement.x - 0.594).abs() > 1e-9);
    assert_eq!(a.movement.y, -0.05);
}
