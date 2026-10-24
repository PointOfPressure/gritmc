//! The projectile path's arithmetic, against vanilla running it.
//!
//! `crates/rustmc-server/golden/projectile.json` is produced by
//! `ci/projectile-golden.sh`, which calls the real methods in the pinned
//! 26.2 jar. **This is the fidelity instrument**, as opposed to
//! `tests/projectile_flight.rs`, which grades the whole-tick assembly against
//! this lane's own reading of the same bytecode and says so in its own
//! header.
//!
//! Every comparison below is on raw bits. A tolerance would hide exactly the
//! errors this file exists to find: `57.2957763671875` against
//! `57.295780181884766` is 1.9e-6, a fused multiply-add against two roundings
//! is one ulp, and both of them move a packed rotation byte.
//!
//! # What is covered and what is not
//!
//! | section | vanilla |
//! |---|---|
//! | `launchDirections` | `Mth.sin`/`cos`, `Vec3.normalize`, `horizontalDistance`, `Mth.atan2 * RAD_TO_DEG` |
//! | `triangleStreams` | `RandomSource.triangle`, six draws a shot, three shots a row |
//! | `lerpRotation` | `Projectile.lerpRotation`, 225 pairs including both seams |
//! | `bowPower` | `BowItem.getPowerForTime`, charge 0..40 |
//! | `crossbowShotPitch` | `CrossbowItem.getShotPitch`, index 0..5 |
//! | `aabbClip` | `AABB.clip(Vec3, Vec3)` and `AABB.getCenter` |
//! | `voxelClip` | `VoxelShape.clip(Vec3, Vec3, BlockPos)`, the inside path included |
//! | `traverse` | `BlockGetter.traverseBlocks`, the whole DDA walk |
//!
//! **Not covered, and named rather than left to be inferred:**
//! `ProjectileUtil.computeMargin` and `getManyEntityHitResult` need a real
//! `Entity` and a real `Level`, and `AbstractArrow.tick` needs both plus a
//! `ServerLevel`. Those want the `MoveLevel` treatment
//! `ci/entity-move-golden.sh` gives `Entity.collide`; until they have it, the
//! 26.2 margin ramp and the three-clip near-miss probe are graded only by
//! `tests/projectile_flight.rs`, which is self-consistency.

use rustmc_data::BlockPos;
use rustmc_entity::mth::{atan2, cos, sin};
use rustmc_server::collide::{Aabb, Axis, CollisionShapes, Vec3};
use rustmc_server::projectile::bow::{bow_power_for_time, crossbow_shot_pitch};
use rustmc_server::projectile::clip::{shape_clip, traverse_blocks};
use rustmc_server::projectile::math;
use rustmc_server::projectile::shoot::{
    lerp_rotation, triangle, DEG_TO_RAD, INACCURACY_SCALE, RAD_TO_DEG,
};
use rustmc_worldgen::rng::legacy::LegacyRandomSource;
use serde_json::Value;

fn golden() -> Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/golden/projectile.json");
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| {
        panic!("{path}: {e} — regenerate with ci/projectile-golden.sh");
    });
    serde_json::from_str(&text).expect("the golden is JSON")
}

fn d(v: &Value, k: &str) -> f64 {
    f64::from_bits(v[k].as_i64().unwrap_or_else(|| panic!("no i64 at {k}")) as u64)
}

fn f(v: &Value, k: &str) -> f32 {
    f32::from_bits(v[k].as_i64().unwrap_or_else(|| panic!("no i64 at {k}")) as u32)
}

fn rows<'a>(g: &'a Value, k: &str) -> &'a Vec<Value> {
    g[k].as_array()
        .unwrap_or_else(|| panic!("{k} is not an array"))
}

#[test]
fn the_golden_is_the_pinned_version_and_is_not_empty() {
    let g = golden();
    assert_eq!(g["version"].as_str(), Some(rustmc_proto::GAME_VERSION));
    assert_eq!(
        f64::from_bits(g["radToDeg"].as_i64().unwrap() as u64),
        RAD_TO_DEG
    );
    // A floor under every section: an empty array compares equal to nothing
    // and is the shape of golden that cannot go red.
    for (k, n) in [
        ("launchDirections", 325),
        ("triangleStreams", 20),
        ("lerpRotation", 225),
        ("bowPower", 41),
        ("crossbowShotPitch", 3),
        ("aabbClip", 55),
        ("voxelClip", 88),
        ("traverse", 9),
    ] {
        assert_eq!(rows(&g, k).len(), n, "{k}");
    }
}

#[test]
fn the_launch_direction_and_both_rotations_are_bit_exact() {
    let g = golden();
    let mut checked = 0;
    for r in rows(&g, "launchDirections") {
        let x_rot = i16::try_from(r["xRot"].as_i64().unwrap())
            .map(f32::from)
            .unwrap();
        let y_rot = i16::try_from(r["yRot"].as_i64().unwrap())
            .map(f32::from)
            .unwrap();

        // `shootFromRotation`'s three products, in float.
        let fx = -sin(f64::from(y_rot * DEG_TO_RAD)) * cos(f64::from(x_rot * DEG_TO_RAD));
        let fy = -sin(f64::from((x_rot + 0.0) * DEG_TO_RAD));
        let fz = cos(f64::from(y_rot * DEG_TO_RAD)) * cos(f64::from(x_rot * DEG_TO_RAD));
        assert_eq!(fx.to_bits(), f(r, "f").to_bits(), "f at {x_rot}/{y_rot}");
        assert_eq!(fy.to_bits(), f(r, "g").to_bits(), "g at {x_rot}/{y_rot}");
        assert_eq!(fz.to_bits(), f(r, "h").to_bits(), "h at {x_rot}/{y_rot}");

        let v = Vec3::new(f64::from(fx), f64::from(fy), f64::from(fz));
        let n = math::normalize(v);
        assert_eq!(n.x.to_bits(), d(r, "nx").to_bits(), "nx at {x_rot}/{y_rot}");
        assert_eq!(n.y.to_bits(), d(r, "ny").to_bits(), "ny at {x_rot}/{y_rot}");
        assert_eq!(n.z.to_bits(), d(r, "nz").to_bits(), "nz at {x_rot}/{y_rot}");

        let hd = math::horizontal_distance(n);
        assert_eq!(hd.to_bits(), d(r, "hd").to_bits(), "hd at {x_rot}/{y_rot}");

        #[expect(clippy::cast_possible_truncation, reason = "vanilla's d2f")]
        let yaw = (atan2(n.x, n.z) * RAD_TO_DEG) as f32;
        #[expect(clippy::cast_possible_truncation, reason = "vanilla's d2f")]
        let pitch = (atan2(n.y, hd) * RAD_TO_DEG) as f32;
        assert_eq!(
            yaw.to_bits(),
            f(r, "yawDeg").to_bits(),
            "yaw at {x_rot}/{y_rot}"
        );
        assert_eq!(
            pitch.to_bits(),
            f(r, "pitchDeg").to_bits(),
            "pitch at {x_rot}/{y_rot}"
        );
        checked += 1;
    }
    assert_eq!(checked, 325);
}

#[test]
fn the_inaccuracy_draw_stream_matches_draw_for_draw() {
    let g = golden();
    let mut checked = 0;
    for r in rows(&g, "triangleStreams") {
        let seed = r["seed"].as_i64().unwrap();
        let inaccuracy = f(r, "inaccuracy");
        let want: Vec<f64> = r["draws"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| f64::from_bits(v.as_i64().unwrap() as u64))
            .collect();
        assert_eq!(want.len(), 9, "three shots of three components");
        let mut rng = LegacyRandomSource::new(seed);
        let e = INACCURACY_SCALE * f64::from(inaccuracy);
        for (i, w) in want.iter().enumerate() {
            let got = triangle(&mut rng, 0.0, e);
            assert_eq!(
                got.to_bits(),
                w.to_bits(),
                "seed {seed} inaccuracy {inaccuracy} draw {i}"
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 180);
}

#[test]
fn lerp_rotation_matches_on_every_pair_including_both_seams() {
    let g = golden();
    let mut checked = 0;
    for r in rows(&g, "lerpRotation") {
        let cur = f(r, "current");
        let tgt = f(r, "target");
        let want = f(r, "out");
        let got = lerp_rotation(cur, tgt);
        assert_eq!(got.to_bits(), want.to_bits(), "lerpRotation({cur}, {tgt})");
        checked += 1;
    }
    assert_eq!(checked, 225);
}

#[test]
fn the_bow_power_curve_matches_bit_for_bit() {
    let g = golden();
    for r in rows(&g, "bowPower") {
        #[expect(clippy::cast_possible_truncation, reason = "small loop bound")]
        let charge = r["charge"].as_i64().unwrap() as i32;
        let want = f(r, "power");
        let got = bow_power_for_time(charge);
        assert_eq!(got.to_bits(), want.to_bits(), "getPowerForTime({charge})");
    }
}

#[test]
fn the_crossbow_shot_pitch_matches_and_index_zero_costs_no_draw() {
    let g = golden();
    for r in rows(&g, "crossbowShotPitch") {
        let seed = r["seed"].as_i64().unwrap();
        let want: Vec<f32> = r["pitches"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| f32::from_bits(v.as_i64().unwrap() as u32))
            .collect();
        let mut rng = LegacyRandomSource::new(seed);
        for (index, w) in want.iter().enumerate() {
            #[expect(clippy::cast_possible_truncation, reason = "small loop bound")]
            let got = crossbow_shot_pitch(&mut rng, index as i32);
            assert_eq!(got.to_bits(), w.to_bits(), "seed {seed} index {index}");
        }
        // The whole stream agreeing after index 0 is what proves index 0 took
        // no draw: one spurious draw there shifts every later value.
        assert_eq!(want[0], 1.0);
    }
}

// ---------------------------------------------------------------------------
// AABB.clip
// ---------------------------------------------------------------------------

fn boxes(g: &Value) -> Vec<Aabb> {
    rows(g, "aabbBoxes")
        .iter()
        .map(|b| {
            Aabb::new(
                d(b, "minX"),
                d(b, "minY"),
                d(b, "minZ"),
                d(b, "maxX"),
                d(b, "maxY"),
                d(b, "maxZ"),
            )
        })
        .collect()
}

fn segments(g: &Value) -> Vec<(Vec3, Vec3)> {
    rows(g, "aabbSegments")
        .iter()
        .map(|s| {
            (
                Vec3::new(d(s, "fx"), d(s, "fy"), d(s, "fz")),
                Vec3::new(d(s, "tx"), d(s, "ty"), d(s, "tz")),
            )
        })
        .collect()
}

#[test]
fn aabb_clip_agrees_on_every_box_and_segment() {
    let g = golden();
    let bs = boxes(&g);
    let ss = segments(&g);
    let mut hits = 0;
    for r in rows(&g, "aabbClip") {
        let bi = usize::try_from(r["box"].as_i64().unwrap()).unwrap();
        let si = usize::try_from(r["seg"].as_i64().unwrap()).unwrap();
        let want_hit = r["hit"].as_bool().unwrap();
        let got = math::clip(&bs[bi], ss[si].0, ss[si].1);
        assert_eq!(got.is_some(), want_hit, "box {bi} seg {si}");
        if let Some((at, _)) = got {
            assert_eq!(at.x.to_bits(), d(r, "x").to_bits(), "box {bi} seg {si} x");
            assert_eq!(at.y.to_bits(), d(r, "y").to_bits(), "box {bi} seg {si} y");
            assert_eq!(at.z.to_bits(), d(r, "z").to_bits(), "box {bi} seg {si} z");
            hits += 1;
        }
    }
    // The instrument's own liveness: a golden in which nothing hits compares
    // 55 `false`s and grades no arithmetic at all.
    assert_eq!(hits, 15, "the sweep must contain real crossings");
}

#[test]
fn aabb_get_center_is_bit_exact() {
    let g = golden();
    for (i, b) in rows(&g, "aabbBoxes").iter().enumerate() {
        let bx = &boxes(&g)[i];
        let c = math::get_center(bx);
        assert_eq!(c.x.to_bits(), d(b, "cx").to_bits(), "box {i} cx");
        assert_eq!(c.y.to_bits(), d(b, "cy").to_bits(), "box {i} cy");
        assert_eq!(c.z.to_bits(), d(b, "cz").to_bits(), "box {i} cz");
    }
}

// ---------------------------------------------------------------------------
// VoxelShape.clip
// ---------------------------------------------------------------------------

/// Build a [`CollisionShapes`] whose grids are the golden's own `toAabbs()`
/// output, one shape per row.
///
/// **The fixture is derived from vanilla rather than assumed.** A grid written
/// out by hand would be a second guess at what `Shapes.or` produced, and the
/// point of this test is that the *clip* agrees, not that two people can build
/// the same shape. Vanilla's internal grid for a box on the 1/8 lattice is the
/// 8x8x8 cube grid rather than the two-coordinate one built here, and that is
/// deliberate: `findIndex` returns different indices on the two, `isFullWide`
/// returns the same occupancy for every point, and this test is what says the
/// difference does not reach a clip result.
fn shapes_from_golden(g: &Value) -> CollisionShapes {
    let mut shapes = Vec::new();
    for sh in rows(g, "voxelShapes") {
        let aabbs: Vec<[f64; 6]> = sh["aabbs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| {
                let v = a.as_array().unwrap();
                let mut out = [0.0; 6];
                for (i, o) in out.iter_mut().enumerate() {
                    *o = f64::from_bits(v[i].as_i64().unwrap() as u64);
                }
                out
            })
            .collect();
        let axis_coords = |lo: usize, hi: usize| -> Vec<f64> {
            let mut c: Vec<f64> = aabbs.iter().flat_map(|a| [a[lo], a[hi]]).collect();
            c.sort_by(|x, y| x.partial_cmp(y).unwrap());
            c.dedup();
            c
        };
        let xs = axis_coords(0, 3);
        let ys = axis_coords(1, 4);
        let zs = axis_coords(2, 5);
        let (nx, ny, nz) = (xs.len() - 1, ys.len() - 1, zs.len() - 1);
        let mut bits = vec![false; nx * ny * nz];
        for x in 0..nx {
            for y in 0..ny {
                for z in 0..nz {
                    let (cx, cy, cz) = (
                        (xs[x] + xs[x + 1]) / 2.0,
                        (ys[y] + ys[y + 1]) / 2.0,
                        (zs[z] + zs[z + 1]) / 2.0,
                    );
                    let inside = aabbs.iter().any(|a| {
                        cx > a[0] && cx < a[3] && cy > a[1] && cy < a[4] && cz > a[2] && cz < a[5]
                    });
                    bits[(x * ny + y) * nz + z] = inside;
                }
            }
        }
        // The extract's `voxels` field is a hex string, one nibble per four
        // cells, most significant bit first — the encoding
        // `CollisionShapes::from_json` reads.
        let mut hex = String::new();
        for chunk in bits.chunks(4) {
            let mut nib = 0u8;
            for (i, b) in chunk.iter().enumerate() {
                if *b {
                    nib |= 1 << (3 - i);
                }
            }
            hex.push(char::from_digit(u32::from(nib), 16).unwrap());
        }
        #[expect(clippy::cast_possible_wrap, reason = "the extract's bit encoding")]
        let bitsof = |v: &f64| v.to_bits() as i64;
        shapes.push(serde_json::json!({
            "size": [nx, ny, nz],
            "coords": [
                xs.iter().map(bitsof).collect::<Vec<_>>(),
                ys.iter().map(bitsof).collect::<Vec<_>>(),
                zs.iter().map(bitsof).collect::<Vec<_>>(),
            ],
            "voxels": hex,
            "full": nx == 1 && ny == 1 && nz == 1
                && xs[0] == 0.0 && xs[1] == 1.0
                && ys[0] == 0.0 && ys[1] == 1.0
                && zs[0] == 0.0 && zs[1] == 1.0,
        }));
    }
    let n = shapes.len();
    // One extra empty shape so the table has a moving-piston row to name;
    // `CollisionShapes::from_json` refuses a table without one, and that guard
    // is `collide.rs`'s.
    shapes.push(serde_json::json!({
        "size": [0, 0, 0], "coords": [[], [], []], "voxels": "", "full": false
    }));
    let doc = serde_json::json!({
        "gridEquivalence": {"mismatches": 0, "probes": 16_736_112_i64},
        "shapes": shapes,
        "rows": (0..=n).collect::<Vec<_>>(),
        "movingPistonStates": [n],
    });
    CollisionShapes::from_json(&doc).expect("the golden-derived fixture parses")
}

#[test]
fn voxel_shape_clip_agrees_on_every_shape_position_and_segment() {
    let g = golden();
    let table = shapes_from_golden(&g);
    let ss = segments(&g);
    let positions: Vec<BlockPos> = rows(&g, "shapePositions")
        .iter()
        .map(|p| BlockPos {
            x: i32::try_from(p["x"].as_i64().unwrap()).unwrap(),
            y: i32::try_from(p["y"].as_i64().unwrap()).unwrap(),
            z: i32::try_from(p["z"].as_i64().unwrap()).unwrap(),
        })
        .collect();

    let mut hits = 0;
    let mut inside_hits = 0;
    for r in rows(&g, "voxelClip") {
        let sh = u32::try_from(r["shape"].as_i64().unwrap()).unwrap();
        let pi = usize::try_from(r["pos"].as_i64().unwrap()).unwrap();
        let si = usize::try_from(r["seg"].as_i64().unwrap()).unwrap();
        let bp = positions[pi];
        let off = |v: Vec3| {
            Vec3::new(
                v.x + f64::from(bp.x),
                v.y + f64::from(bp.y),
                v.z + f64::from(bp.z),
            )
        };
        let shape = table.get(sh).expect("the fixture has this shape");
        let got = shape_clip(shape, bp, off(ss[si].0), off(ss[si].1));
        let want_hit = r["hit"].as_bool().unwrap();
        assert_eq!(got.is_some(), want_hit, "shape {sh} pos {pi} seg {si}");
        if let Some(h) = got {
            let tag = format!("shape {sh} pos {pi} seg {si}");
            assert_eq!(h.location.x.to_bits(), d(r, "x").to_bits(), "{tag} x");
            assert_eq!(h.location.y.to_bits(), d(r, "y").to_bits(), "{tag} y");
            assert_eq!(h.location.z.to_bits(), d(r, "z").to_bits(), "{tag} z");
            assert_eq!(h.inside, r["inside"].as_bool().unwrap(), "{tag} inside");
            assert_eq!(
                face_name(h.direction),
                r["dir"].as_str().unwrap(),
                "{tag} direction"
            );
            hits += 1;
            if h.inside {
                inside_hits += 1;
            }
        }
    }
    assert_eq!(hits, 54, "the sweep must contain real crossings");
    assert_eq!(inside_hits, 4, "and must exercise the inside fast path");
}

fn face_name(f: math::Face) -> &'static str {
    match f {
        math::Face::Down => "down",
        math::Face::Up => "up",
        math::Face::North => "north",
        math::Face::South => "south",
        math::Face::West => "west",
        math::Face::East => "east",
    }
}

#[test]
fn the_golden_shapes_have_the_grid_the_clip_argument_assumes() {
    // The occupied-cell claim in `projectile/clip.rs` rests on the two-part
    // shape actually having a hole in the middle. If `Shapes.or` had merged
    // it, the whole comparison above would be four cubes and would prove
    // nothing about interior faces.
    let g = golden();
    let table = shapes_from_golden(&g);
    let two_part = table.get(2).expect("shape 2");
    assert_eq!(two_part.size(Axis::X), 3, "three cells across x");
    assert!(two_part.is_full_wide(0, 0, 0));
    assert!(!two_part.is_full_wide(1, 0, 0), "the gap is real");
    assert!(two_part.is_full_wide(2, 0, 0));
}

// ---------------------------------------------------------------------------
// traverseBlocks
// ---------------------------------------------------------------------------

#[test]
fn the_dda_walk_visits_the_same_cells_in_the_same_order() {
    let g = golden();
    let mut total = 0;
    for (i, r) in rows(&g, "traverse").iter().enumerate() {
        let from = Vec3::new(d(r, "fx"), d(r, "fy"), d(r, "fz"));
        let to = Vec3::new(d(r, "tx"), d(r, "ty"), d(r, "tz"));
        let want: Vec<(i32, i32, i32)> = r["cells"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                let v = c.as_array().unwrap();
                (
                    i32::try_from(v[0].as_i64().unwrap()).unwrap(),
                    i32::try_from(v[1].as_i64().unwrap()).unwrap(),
                    i32::try_from(v[2].as_i64().unwrap()).unwrap(),
                )
            })
            .collect();
        let mut got = Vec::new();
        let missed = traverse_blocks(from, to, &mut |p| {
            got.push((p.x, p.y, p.z));
            true
        });
        assert!(missed, "walk {i} must run to the end when nothing stops it");
        assert_eq!(got, want, "walk {i}");
        total += got.len();
    }
    // The order and the length are the whole content of this comparison; a
    // walk that visited one cell every time would agree on nothing else.
    assert_eq!(total, 73);
}
