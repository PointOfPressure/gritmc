//! Portal timing, the coordinate scale and the border clamp.
//!
//! # Two different strengths, said apart
//!
//! * **The clamp is checked against vanilla.** `world_border.json`'s `clamps`
//!   array is `WorldBorder.clampToBounds` run inside the pinned jar on a
//!   32-wide border, including a probe exactly on the upper edge — which is
//!   the only place the float-widened epsilon changes the answer.
//! * **The timer is checked against its bytecode, not against a run.**
//!   `PortalProcessor` needs a `ServerLevel` and an `Entity` to exercise in
//!   vanilla. The sequence below is derived from the instructions and pins the
//!   two things a rewrite gets wrong — the post-increment and the decay of
//!   four — so it catches a change and does not prove parity.
//! * The coordinate scale is read out of the datapack extract, so the 8.0 is
//!   vanilla's own number rather than a remembered one.

use std::path::PathBuf;

use rustmc_data::BlockPos;
use rustmc_server::dimension::{
    self, clamp_to_bounds, forcer, nether_portal_transition_time, scaled_entry_position,
    teleportation_scale, PortalProcessor,
};
use rustmc_server::worldborder::WorldBorder;

fn golden() -> serde_json::Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("golden/world_border.json");
    assert!(
        rustmc_data::fixture::available(&p, "world border golden", "ci/border-golden.sh"),
        "no world border golden"
    );
    serde_json::from_slice(&std::fs::read(&p).expect("read golden")).expect("parse golden")
}

fn dimension_type(name: &str) -> serde_json::Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../oracle-work/vanilla-26.2/generated-data/data/minecraft/dimension_type/{name}.json"
    ));
    assert!(
        rustmc_data::fixture::available(&p, "dimension type extract", "ci/vanilla-server.sh data"),
        "no dimension type extract"
    );
    serde_json::from_slice(&std::fs::read(&p).expect("read")).expect("parse")
}

/// `clampToBounds`, against vanilla's own answers.
#[test]
fn the_border_clamp_matches_vanilla() {
    let g = golden();
    let size = g["clampSize"].as_f64().unwrap();
    assert_eq!(size, 32.0);
    let mut b = WorldBorder::default();
    b.set_center(0.0, 0.0);
    b.set_size(size);

    let clamps = g["clamps"].as_array().expect("clamps");
    assert!(
        clamps.len() >= 7,
        "too few clamp probes to separate anything"
    );
    let mut on_edge = 0usize;
    for c in clamps {
        let i = c["in"].as_array().unwrap();
        let o = c["out"].as_array().unwrap();
        let (x, y, z) = (
            i[0].as_f64().unwrap(),
            i[1].as_f64().unwrap(),
            i[2].as_f64().unwrap(),
        );
        let want = BlockPos {
            x: o[0].as_i64().unwrap() as i32,
            y: o[1].as_i64().unwrap() as i32,
            z: o[2].as_i64().unwrap() as i32,
        };
        assert_eq!(clamp_to_bounds(&b, x, y, z), want, "clamp {x},{y},{z}");
        // A probe exactly on the upper edge is the one the epsilon moves: 16.0
        // clamps to 15.99999 and floors to 15, not 16.
        if (x - 16.0).abs() < f64::EPSILON {
            assert_eq!(want.x, 15, "the epsilon did not bite at the upper edge");
            on_edge += 1;
        }
    }
    assert_eq!(on_edge, 1, "no probe sits on the upper edge");

    // y is not clamped at all: the probes carry 320 and -64, both outside any
    // border in the horizontal sense, and both come back unchanged but floored.
    let far = clamp_to_bounds(&b, 0.0, 12345.75, 0.0);
    assert_eq!(far.y, 12345, "y is floored, not clamped");
}

/// The coordinate scale, read out of the datapack rather than remembered.
#[test]
fn the_coordinate_scale_comes_from_the_datapack() {
    let nether = dimension_type("the_nether")["coordinate_scale"]
        .as_f64()
        .expect("nether coordinate_scale");
    let overworld = dimension_type("overworld")["coordinate_scale"]
        .as_f64()
        .expect("overworld coordinate_scale");
    assert_eq!(nether, 8.0);
    assert_eq!(overworld, 1.0);

    // The argument order: `getTeleportationScale(from, to)` is `from / to`.
    assert_eq!(teleportation_scale(overworld, nether), 0.125);
    assert_eq!(teleportation_scale(nether, overworld), 8.0);

    let mut b = WorldBorder::default();
    b.set_center(0.0, 0.0);
    b.set_size(60_000_000.0);
    // Overworld (800, 70, -1600) enters the nether at (100, 70, -200): x and z
    // divided by eight, **y unchanged**.
    assert_eq!(
        scaled_entry_position(
            &b,
            800.0,
            70.0,
            -1600.0,
            teleportation_scale(overworld, nether)
        ),
        BlockPos {
            x: 100,
            y: 70,
            z: -200
        }
    );
    // And back the other way, times eight.
    assert_eq!(
        scaled_entry_position(
            &b,
            100.0,
            70.0,
            -200.0,
            teleportation_scale(nether, overworld)
        ),
        BlockPos {
            x: 800,
            y: 70,
            z: -1600
        }
    );
}

/// The timer's shape.
#[test]
fn the_portal_timer() {
    let pos = BlockPos { x: 1, y: 2, z: 3 };

    // A player at the default delay of 80: the counter has to reach 80 before
    // the comparison passes, so the teleport happens on the 81st tick inside.
    let mut p = PortalProcessor::new(pos);
    assert_eq!(p.portal_time(), 0);
    assert!(p.is_inside_portal_this_tick(), "the ctor sets the flag");
    let mut fired = None;
    for tick in 1..=100 {
        p.set_as_inside_portal_this_tick(true);
        if p.process(true, 80) {
            fired = Some(tick);
            break;
        }
    }
    assert_eq!(fired, Some(81), "post-increment: 80 fires on the 81st tick");

    // The decay: four per tick outside, floored at zero.
    let mut p = PortalProcessor::new(pos);
    for _ in 0..10 {
        p.set_as_inside_portal_this_tick(true);
        p.process(true, 80);
    }
    assert_eq!(p.portal_time(), 10);
    assert!(!p.has_expired());
    p.set_as_inside_portal_this_tick(false);
    assert!(!p.process(true, 80));
    assert_eq!(p.portal_time(), 6, "one tick out costs four");
    p.set_as_inside_portal_this_tick(false);
    p.process(true, 80);
    p.set_as_inside_portal_this_tick(false);
    p.process(true, 80);
    assert_eq!(p.portal_time(), 0, "floored at zero, not negative");
    assert!(p.has_expired());

    // `canTeleport = false` still consumes the flag and still counts, so a
    // player in a portal on a server that will not let them through keeps
    // accruing time.
    let mut p = PortalProcessor::new(pos);
    p.set_as_inside_portal_this_tick(true);
    assert!(!p.process(false, 0));
    assert_eq!(p.portal_time(), 0, "a refused teleport does not increment");
    assert!(
        !p.is_inside_portal_this_tick(),
        "but it does clear the flag"
    );
}

/// The transition times.
#[test]
fn transition_times() {
    // Anything that is not a player goes through immediately.
    assert_eq!(nether_portal_transition_time(false, false, 0, 80), 0);
    assert_eq!(nether_portal_transition_time(false, true, 999, 999), 0);
    // A survival player takes the default rule.
    assert_eq!(nether_portal_transition_time(true, false, 0, 80), 80);
    // An invulnerable one takes the creative rule — the test is
    // `abilities.invulnerable`, not the game mode, so a survival player made
    // invulnerable takes the creative delay too.
    assert_eq!(nether_portal_transition_time(true, true, 0, 80), 0);
    // A negative rule is floored, not treated as "never".
    assert_eq!(nether_portal_transition_time(true, false, 0, -5), 0);
    assert_eq!(
        dimension::DEFAULT_DELAY_RULE,
        ("players_nether_portal_default_delay", 80)
    );
    assert_eq!(
        dimension::CREATIVE_DELAY_RULE,
        ("players_nether_portal_creative_delay", 0)
    );
}

/// Three ways to be wrong, each shown to move a number.
#[test]
fn positive_control() {
    let g = golden();
    let mut b = WorldBorder::default();
    b.set_center(0.0, 0.0);
    b.set_size(g["clampSize"].as_f64().unwrap());

    // 1. The epsilon is a float widened, not 1.0E-5. At x = 16 - 1.0E-5 the
    //    two agree; the difference is only visible in the last bits, so the
    //    assertion is on the constant rather than on an output.
    assert_ne!(
        dimension::BORDER_CLAMP_EPSILON,
        1.0E-5,
        "the epsilon is (double)(float)1.0E-5, not 1.0E-5"
    );
    assert_eq!(dimension::BORDER_CLAMP_EPSILON, f64::from(1.0E-5f32));

    // 2. Floor, not truncate. -15.5 goes to -16; a cast to i32 gives -15.
    assert_eq!(
        clamp_to_bounds(&b, -15.5, 0.0, -15.5),
        BlockPos {
            x: -16,
            y: 0,
            z: -16
        }
    );
    assert_ne!((-15.5f64) as i32, -16);

    // 3. A pre-increment fires one tick early, and every "does it teleport at
    //    all" test passes either way.
    let mut p = PortalProcessor::new(BlockPos { x: 0, y: 0, z: 0 });
    let mut ticks = 0;
    loop {
        ticks += 1;
        p.set_as_inside_portal_this_tick(true);
        if p.process(true, 3) {
            break;
        }
        assert!(ticks < 20, "never fired");
    }
    assert_eq!(ticks, 4, "a pre-increment would give 3");

    // 4. The search radii are not equal, and their ratio is the scale.
    assert_eq!(
        forcer::OVERWORLD_PORTAL_RADIUS / forcer::NETHER_PORTAL_RADIUS,
        8
    );
}

/// End portals, and the two ways they differ from nether portals.
#[test]
fn end_portals() {
    use rustmc_server::dimension::end_portal::{
        end_portal_target, Target, END_SPAWN_POINT, TRANSITION_TIME,
    };

    assert_eq!(
        END_SPAWN_POINT,
        BlockPos {
            x: 100,
            y: 50,
            z: 0
        }
    );

    // No delay, for anyone. `Portal.getPortalTransitionTime` is a default
    // method returning 0 and EndPortalBlock does not override it, so the
    // nether's gamerules do not reach it and a player falls through on the
    // first tick.
    assert_eq!(TRANSITION_TIME, 0);
    assert_ne!(
        TRANSITION_TIME,
        nether_portal_transition_time(true, false, 0, 80),
        "an end portal and a nether portal must not share a delay"
    );

    // With a zero transition time the very first tick fires, which is the
    // observable difference: `portalTime++ >= 0` is true at 0.
    let mut p = PortalProcessor::new(BlockPos { x: 0, y: 0, z: 0 });
    assert!(
        p.process(true, TRANSITION_TIME),
        "an end portal fires immediately"
    );

    assert_eq!(
        end_portal_target(false),
        Target::EndPlatform(END_SPAWN_POINT)
    );
    assert_eq!(end_portal_target(true), Target::LevelRespawnData);
}

/// Every field [`Dimension`] claims, scored against the **datapack extract**.
///
/// `data/minecraft/dimension_type/<name>.json` is Mojang's own datagen output
/// for the pinned jar, so this is vanilla's number and not a remembered one.
/// It covers the six fields that live in a dimension type; `sea_level` is not
/// one of them and is checked separately below.
#[test]
fn the_dimension_types_match_the_datapack_extract() {
    for (dim, file) in [
        (dimension::Dimension::Overworld, "overworld"),
        (dimension::Dimension::Nether, "the_nether"),
        (dimension::Dimension::TheEnd, "the_end"),
    ] {
        let j = dimension_type(file);
        let i = |k: &str| j[k].as_i64().unwrap_or_else(|| panic!("{file}.{k}"));
        let b = |k: &str| j[k].as_bool().unwrap_or_else(|| panic!("{file}.{k}"));
        assert_eq!(i64::from(dim.min_y()), i("min_y"), "{file} min_y");
        assert_eq!(i64::from(dim.height()), i("height"), "{file} height");
        assert_eq!(
            i64::from(dim.logical_height()),
            i("logical_height"),
            "{file} logical_height"
        );
        assert!(
            (dim.coordinate_scale() - j["coordinate_scale"].as_f64().expect("scale")).abs()
                < f64::EPSILON,
            "{file} coordinate_scale"
        );
        assert_eq!(dim.has_skylight(), b("has_skylight"), "{file} has_skylight");
        assert_eq!(dim.has_ceiling(), b("has_ceiling"), "{file} has_ceiling");
        assert_eq!(
            dim.sections(),
            (i("height") / 16) as usize,
            "{file} sections"
        );
    }
}

/// The **End has skylight** and the nether does not, said as its own
/// assertion because it is the one a rewrite gets backwards.
///
/// A reader who knows the End is dark assumes `has_skylight: false`; the
/// darkness comes from `sky_light_factor: 0.0` and `ambient_light: 0.25` in
/// the entry's `attributes`, and the sky-light sections are there. This is the
/// positive control for the loop above: without it, a `has_skylight` that
/// always returned `true` would still have to disagree with the nether, but
/// nothing would say *why* the End's value is the surprising one.
#[test]
fn only_the_nether_has_no_skylight() {
    assert!(dimension_type("the_end")["has_skylight"]
        .as_bool()
        .expect("b"));
    assert!(!dimension_type("the_nether")["has_skylight"]
        .as_bool()
        .expect("b"));
    assert!(dimension_type("overworld")["has_skylight"]
        .as_bool()
        .expect("b"));
}

/// The `dimension_type` **registry indices** are the capture's own order.
///
/// This is the number `CommonPlayerSpawnInfo` puts on the wire, and it is a
/// property of the bytes the configuration phase already sends — not of the
/// datapack, not of any list anybody wrote. It is checked against
/// `data/vanilla/registries.bin`, parsed the same way the sender parses it.
///
/// It fires on the thing worth firing on: the registry holds **four** entries
/// because `minecraft:overworld_caves` is numbered even though no level uses
/// it, so `the_end` is 2 and `the_nether` is 3.
#[test]
fn the_registry_indices_are_the_captures_own_order() {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/vanilla/registries.bin");
    assert!(
        rustmc_data::fixture::available(&p, "registry capture", "ci/regdump.py"),
        "no registry capture"
    );
    let caps = rustmc_server::configuration::CapturedRegistries::load(&p).expect("load");
    let entries = caps
        .entries("minecraft:dimension_type")
        .expect("parse")
        .expect("the capture has minecraft:dimension_type");

    let ids: Vec<&str> = entries.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "minecraft:overworld",
            "minecraft:overworld_caves",
            "minecraft:the_end",
            "minecraft:the_nether",
        ],
        "the registry order IS the numbering, and overworld_caves takes index 1"
    );

    for dim in dimension::DIMENSIONS {
        let want = usize::try_from(dim.registry_index()).expect("index");
        assert_eq!(ids[want], dim.id(), "{} is index {want}", dim.id());
    }
}

/// The payload of each entry agrees with the accessors, so the two sources —
/// the datapack extract and the bytes the client is actually sent — are
/// checked against each other as well as against us.
///
/// This is the check that would catch a capture taken from a different
/// version than the extract, which no single-source test can.
#[test]
fn the_registry_payloads_agree_with_the_accessors() {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/vanilla/registries.bin");
    assert!(
        rustmc_data::fixture::available(&p, "registry capture", "ci/regdump.py"),
        "no registry capture"
    );
    let caps = rustmc_server::configuration::CapturedRegistries::load(&p).expect("load");
    let entries = caps
        .entries("minecraft:dimension_type")
        .expect("parse")
        .expect("dimension_type");

    let mut checked = 0;
    for dim in dimension::DIMENSIONS {
        let idx = usize::try_from(dim.registry_index()).expect("index");
        let tag = entries[idx]
            .1
            .as_ref()
            .unwrap_or_else(|| panic!("{} was sent with no payload", dim.id()));
        let i = |k: &str| {
            tag.get(k)
                .and_then(rustmc_world::nbt::Nbt::as_i32)
                .unwrap_or_else(|| panic!("{}.{k}", dim.id()))
        };
        assert_eq!(dim.min_y(), i("min_y"), "{} min_y", dim.id());
        assert_eq!(dim.height(), i("height"), "{} height", dim.id());
        assert_eq!(
            dim.logical_height(),
            i("logical_height"),
            "{} logical_height",
            dim.id()
        );
        // NBT has no booleans: `has_skylight` arrives as a byte.
        assert_eq!(
            i32::from(dim.has_skylight()),
            i("has_skylight"),
            "{} has_skylight",
            dim.id()
        );
        assert_eq!(
            i32::from(dim.has_ceiling()),
            i("has_ceiling"),
            "{} has_ceiling",
            dim.id()
        );
        checked += 1;
    }
    assert_eq!(checked, 3, "all three levels checked");
}

/// `sea_level` is the **chunk generator's**, not the dimension type's.
///
/// `createCommonSpawnInfo` fills that field from `ServerLevel.getSeaLevel()`,
/// which is `chunkSource.getGenerator().getSeaLevel()`. The value lives in
/// `worldgen/noise_settings/<name>.json` and appears nowhere in the
/// `dimension_type` registry entry — so a server that read it from the
/// dimension type would find nothing there and send a zero for all three.
#[test]
fn the_sea_level_comes_from_the_noise_settings() {
    for (dim, file) in [
        (dimension::Dimension::Overworld, "overworld"),
        (dimension::Dimension::Nether, "nether"),
        (dimension::Dimension::TheEnd, "end"),
    ] {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../oracle-work/vanilla-26.2/generated-data/data/minecraft/worldgen/noise_settings/{file}.json"
        ));
        assert!(
            rustmc_data::fixture::available(&p, "noise settings", "ci/vanilla-server.sh data"),
            "no noise settings extract"
        );
        let j: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&p).expect("read")).expect("parse");
        assert_eq!(
            i64::from(dim.sea_level()),
            j["sea_level"].as_i64().expect("sea_level"),
            "{file} sea_level"
        );
        // And it is NOT in the dimension type, which is the point.
        let dt = dimension_type(match dim {
            dimension::Dimension::Overworld => "overworld",
            dimension::Dimension::Nether => "the_nether",
            dimension::Dimension::TheEnd => "the_end",
        });
        assert!(
            dt.get("sea_level").is_none(),
            "{file}: sea_level must not be in the dimension type"
        );
    }
}

/// The obsidian platform an arriving entity lands on: 100 positions, 25
/// obsidian and 75 air, in **z-major** order.
///
/// The order is the finding. `MutableBlockPos.move(III)` takes `(dx, dy, dz)`
/// and `createEndPlatform`'s bytecode pushes local slots 5, 6, 4 into it, so
/// the loop that reads as the outermost in the argument list is the innermost
/// in the code: `dz` outer, `dx` middle, `dy` inner. Transcribing the
/// signature's order instead gives the same hundred positions in a different
/// sequence, and the sequence is observable because `destroyBlock` drops items
/// and fires neighbour updates.
#[test]
fn the_end_platform_is_a_hundred_blocks_z_major() {
    let origin = dimension::end_portal::player_end_arrival().platform_origin;
    assert_eq!(
        origin,
        BlockPos {
            x: 100,
            y: 49,
            z: 0
        },
        "`BlockPos.containing(atBottomCenterOf(END_SPAWN_POINT)).below()` — one \
         BELOW the spawn point, not the spawn point"
    );

    let ps = dimension::end_portal::end_platform_positions(origin);
    assert_eq!(ps.len(), 100, "5 x 5 x 4");
    assert_eq!(
        ps.iter()
            .filter(|(_, b)| *b == "minecraft:obsidian")
            .count(),
        25,
        "one layer of obsidian, at dy == -1"
    );
    assert_eq!(ps.iter().filter(|(_, b)| *b == "minecraft:air").count(), 75);
    assert!(
        ps.iter()
            .all(|(p, b)| (*b == "minecraft:obsidian") == (p.y == origin.y - 1)),
        "obsidian is exactly the dy == -1 layer"
    );

    // The order: the first four positions share a column and walk y, then x
    // steps, and z only moves after twenty.
    let at = |i: usize| ps[i].0;
    assert_eq!(
        at(0),
        BlockPos {
            x: 98,
            y: 48,
            z: -2
        }
    );
    assert_eq!(
        at(1),
        BlockPos {
            x: 98,
            y: 49,
            z: -2
        },
        "dy is the inner loop"
    );
    assert_eq!(
        at(4),
        BlockPos {
            x: 99,
            y: 48,
            z: -2
        },
        "dx is the middle loop"
    );
    assert_eq!(
        at(20),
        BlockPos {
            x: 98,
            y: 48,
            z: -1
        },
        "dz is the OUTER loop"
    );

    // No position repeats, and the box is exactly 5 x 4 x 5 around the origin.
    let set: std::collections::BTreeSet<_> = ps.iter().map(|(p, _)| (p.x, p.y, p.z)).collect();
    assert_eq!(set.len(), 100);
    assert_eq!(set.iter().map(|p| p.0).min(), Some(origin.x - 2));
    assert_eq!(set.iter().map(|p| p.0).max(), Some(origin.x + 2));
    assert_eq!(set.iter().map(|p| p.1).min(), Some(origin.y - 1));
    assert_eq!(set.iter().map(|p| p.1).max(), Some(origin.y + 2));
    assert_eq!(set.iter().map(|p| p.2).min(), Some(origin.z - 2));
    assert_eq!(set.iter().map(|p| p.2).max(), Some(origin.z + 2));
}

/// A **player** entering the End lands one block below where anything else
/// does, at (100.5, 49.0, 0.5) facing west.
///
/// `getPortalDestination` computes `pos = Vec3.atBottomCenterOf(
/// END_SPAWN_POINT)` = (100.5, 50.0, 0.5), and then, at offset 142, subtracts
/// `(0, 1, 0)` **only if the entity is a `ServerPlayer`**. So a thrown item
/// arrives at y 50 and a player at 49, standing on the obsidian layer the
/// platform just wrote at 48.
#[test]
fn a_player_lands_one_block_below_the_end_spawn_point() {
    let a = dimension::end_portal::player_end_arrival();
    let spawn = dimension::end_portal::END_SPAWN_POINT;

    // atBottomCenterOf: +0.5 on x and z, the integer y unchanged.
    assert!((a.x - (f64::from(spawn.x) + 0.5)).abs() < f64::EPSILON);
    assert!((a.z - (f64::from(spawn.z) + 0.5)).abs() < f64::EPSILON);
    assert!(
        (a.y - (f64::from(spawn.y) - 1.0)).abs() < f64::EPSILON,
        "the ServerPlayer branch subtracts one from the bottom-centre y"
    );

    // Direction.WEST.toYRot() is 90. North is 180, so a transcription that
    // took the wrong direction's yRot is caught by the value, not by the name.
    assert!((a.y_rot - 90.0).abs() < f32::EPSILON, "WEST.toYRot() is 90");
    assert!((a.x_rot - 0.0).abs() < f32::EPSILON);

    // And the player stands ON the platform: the obsidian layer is at
    // origin.y - 1 = 48 and the player's feet at 49.
    assert!(
        (a.y - f64::from(a.platform_origin.y - 1) - 1.0).abs() < f64::EPSILON,
        "feet one block above the obsidian"
    );
}

/// The branch itself: leaving the End is not the same journey as entering it.
///
/// Entering is a constant — the platform. Leaving reads the level's respawn
/// data and, for a `ServerPlayer`, is overridden entirely by
/// `findRespawnPositionAndUseSpawnBlock`, so it is not a constant and this
/// module does not pretend it is.
#[test]
fn the_end_portal_branches_on_which_side_you_are() {
    use dimension::end_portal::Target;
    assert_eq!(
        dimension::end_portal::end_portal_target(false),
        Target::EndPlatform(dimension::end_portal::END_SPAWN_POINT)
    );
    assert_eq!(
        dimension::end_portal::end_portal_target(true),
        Target::LevelRespawnData
    );
}

/// Leaving the End is a different journey from entering it, and the
/// difference that matters on the wire is the relatives set.
///
/// `EndPortalBlock.getPortalDestination`'s entering branch passes
/// `Relative.union(DELTA, Set.of(X_ROT))` — so an arriving player keeps its
/// velocity and its pitch. `TeleportTransition.createDefault` and
/// `missingRespawnBlock`, which are the two returning branches, both pass
/// `Set.of()` with no arguments: everything is absolute, the delta included.
#[test]
fn leaving_the_end_is_absolute_and_entering_it_is_not() {
    assert!(dimension::end_return::relatives_are_empty());
    // Entering keeps the pitch, which is what a relative X_ROT means: the
    // constant this module carries for it is a delta of zero.
    let a = dimension::end_portal::player_end_arrival();
    assert!((a.x_rot - 0.0).abs() < f32::EPSILON);
}

/// The three outcomes of `findRespawnPositionAndUseSpawnBlock`, and the one
/// boolean that separates two of them.
///
/// `createDefault` and `missingRespawnBlock` are the same method twice over,
/// differing only at offset 37 — `iconst_0` against `iconst_1` — and that
/// flag is read once, by `ServerPlayer.teleport` at its offset 16, to decide
/// whether a `NO_RESPAWN_BLOCK_AVAILABLE` game event goes out ahead of the
/// respawn packet.
#[test]
fn the_missing_respawn_block_event_is_the_only_difference() {
    use dimension::end_return::Outcome;
    assert_eq!(Outcome::of(false, false), Outcome::WorldSpawn);
    assert_eq!(Outcome::of(false, true), Outcome::WorldSpawn);
    assert_eq!(Outcome::of(true, true), Outcome::RespawnBlock);
    assert_eq!(Outcome::of(true, false), Outcome::MissingRespawnBlock);

    assert!(Outcome::MissingRespawnBlock.sends_no_respawn_block_event());
    assert!(!Outcome::WorldSpawn.sends_no_respawn_block_event());
    assert!(!Outcome::RespawnBlock.sends_no_respawn_block_event());
}

/// `findRespawnDimension` reads the **overworld level data's** respawn
/// dimension, not the player's and not the level being left, and falls back
/// to the overworld when that level does not exist.
#[test]
fn the_return_dimension_falls_back_to_the_overworld() {
    use dimension::Dimension;
    assert_eq!(
        dimension::end_return::respawn_dimension(None),
        Dimension::Overworld
    );
    assert_eq!(
        dimension::end_return::respawn_dimension(Some(Dimension::Nether)),
        Dimension::Nether,
        "a world whose respawn data names another dimension returns you there"
    );

    // `atBottomCenterOf` is +0.5 on x and z and nothing on y — the same
    // function that turns END_SPAWN_POINT (100, 50, 0) into (100.5, 50, 0.5).
    let (x, y, z) = dimension::end_return::bottom_centre_of(BlockPos {
        x: -7,
        y: 64,
        z: 12,
    });
    assert!((x - -6.5).abs() < f64::EPSILON);
    assert!((y - 64.0).abs() < f64::EPSILON);
    assert!((z - 12.5).abs() < f64::EPSILON);
}

/// The filter chain and comparator of `PortalForcer.findClosestPortalPosition`.
#[test]
fn the_closest_portal_is_chosen_by_distance_then_y_then_arrival() {
    let from = BlockPos { x: 0, y: 64, z: 0 };
    let yes = |_: BlockPos| true;

    // Distance first: (3,64,0) beats (10,64,0).
    let near = BlockPos { x: 3, y: 64, z: 0 };
    let far = BlockPos { x: 10, y: 64, z: 0 };
    assert_eq!(
        dimension::find_closest_portal_position(&[far, near], from, true, &yes, &yes),
        Some(near)
    );

    // Then y, ASCENDING — `thenComparingInt(BlockPos::getY)`, so the LOWER of
    // two equidistant portals wins. Both of these are 25 away.
    let low = BlockPos { x: 3, y: 60, z: 4 };
    let high = BlockPos { x: 5, y: 60, z: 0 };
    let higher = BlockPos { x: 5, y: 68, z: 0 };
    assert_eq!(
        dimension::find_closest_portal_position(&[higher, high], from, true, &yes, &yes),
        Some(high),
        "equal distance: the lower y wins"
    );
    let _ = low;

    // Then arrival order, and only then: `Stream.min` keeps the FIRST
    // minimum, so two candidates equal in both distance and y are separated
    // by which arrived first and by nothing else.
    let a = BlockPos { x: 5, y: 64, z: 0 };
    let b = BlockPos { x: -5, y: 64, z: 0 };
    assert_eq!(
        dimension::find_closest_portal_position(&[a, b], from, true, &yes, &yes),
        Some(a)
    );
    assert_eq!(
        dimension::find_closest_portal_position(&[b, a], from, true, &yes, &yes),
        Some(b),
        "an exact (distSqr, y) tie is broken by arrival order"
    );
}

/// The square is Chebyshev in x and z with **no y bound**, and its radius is
/// 16 going to the nether and 128 coming back.
///
/// The small radius is the one used when ARRIVING IN the nether, which is the
/// way round a reading gets backwards. It is the same 8:1 as the coordinate
/// scale: a 128-block overworld square maps onto a 16-block nether one.
#[test]
fn the_search_square_is_sixteen_going_in_and_a_hundred_and_twenty_eight_coming_back() {
    assert_eq!(forcer::NETHER_PORTAL_RADIUS, 16);
    assert_eq!(forcer::OVERWORLD_PORTAL_RADIUS, 128);
    assert_eq!(
        forcer::OVERWORLD_PORTAL_RADIUS / forcer::NETHER_PORTAL_RADIUS,
        8,
        "the same 8:1 as the coordinate scale"
    );

    let from = BlockPos { x: 0, y: 64, z: 0 };
    let yes = |_: BlockPos| true;
    let edge = BlockPos {
        x: 16,
        y: 64,
        z: 16,
    };
    let outside = BlockPos { x: 17, y: 64, z: 0 };
    let very_high = BlockPos { x: 0, y: 250, z: 0 };

    assert_eq!(
        dimension::find_closest_portal_position(&[edge], from, true, &yes, &yes),
        Some(edge),
        "the square is inclusive: abs(dx) <= r"
    );
    assert_eq!(
        dimension::find_closest_portal_position(&[outside], from, true, &yes, &yes),
        None,
        "one block further is out"
    );
    assert_eq!(
        dimension::find_closest_portal_position(&[very_high], from, true, &yes, &yes),
        Some(very_high),
        "there is no y bound at all — 186 blocks up is still in the square"
    );
    // And going the other way it is not out.
    assert_eq!(
        dimension::find_closest_portal_position(&[outside], from, false, &yes, &yes),
        Some(outside),
        "128 overworld-side"
    );
}

/// Both filters are consulted, and the second one is `hasProperty(
/// HORIZONTAL_AXIS)` rather than a block-identity test.
///
/// The positive control for the test above: without it, a chain that dropped
/// the filters entirely would satisfy every distance and order assertion.
#[test]
fn a_portal_outside_the_border_or_without_an_axis_is_not_a_candidate() {
    let from = BlockPos { x: 0, y: 64, z: 0 };
    let p = BlockPos { x: 4, y: 64, z: 0 };
    let yes = |_: BlockPos| true;
    let no = |_: BlockPos| false;

    assert_eq!(
        dimension::find_closest_portal_position(&[p], from, true, &yes, &yes),
        Some(p)
    );
    assert_eq!(
        dimension::find_closest_portal_position(&[p], from, true, &no, &yes),
        None,
        "outside the border"
    );
    assert_eq!(
        dimension::find_closest_portal_position(&[p], from, true, &yes, &no),
        None,
        "the block no longer has a horizontal axis"
    );
}
