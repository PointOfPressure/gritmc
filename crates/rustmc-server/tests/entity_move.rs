//! `Entity.collide` against vanilla running it.
//!
//! The vectors in `crates/rustmc-server/golden/entity_move.json` are produced
//! by `ci/entity-move-golden.sh`, which calls `Entity.collectAllColliders`,
//! `Entity.collideBoundingBox` and the private `Entity.collide` on a real
//! `net.minecraft.world.entity.Entity` in a real `Level`. So this compares
//! `crates/rustmc-server/src/collide.rs` against vanilla's bytecode rather
//! than against a second reading of it.
//!
//! # Three assertions per case, not one
//!
//! `colliders`, `swept` and `moved` fail separately and mean different
//! things:
//!
//! * **`colliders`** is the block set. A port can sweep perfectly against the
//!   wrong blocks — the 1e-7 admission margin, the one-cell shell, the
//!   face/edge/corner filter and `hasLargeCollisionShape` all decide
//!   membership, and none of them show in a resolved delta that happens to
//!   come out as `d` either way. It is compared **in order**, because
//!   `Shapes.collide` folds a clamp-to-zero across the list and the clamp does
//!   not commute.
//! * **`swept`** is `collideBoundingBox`. The axis order lives here and
//!   nowhere else.
//! * **`moved`** is `collide`, auto-step included.
//!
//! # Bit-exact
//!
//! Every double is compared by `to_bits`. A tolerance would be a tolerance on
//! the thing being measured, and the differences this exists to catch —
//! `(c + o) - h` against `c - (h - o)`, `1.0E-5f` promoted against `1.0E-5` —
//! are single ulps.
//!
//! # Which gate runs this
//!
//! **`ci/t1.sh`, not `ci/t0.sh`.** T0 is `cargo test --workspace --lib --bins`
//! and that selector does not reach `tests/`; T1 is `cargo test --workspace`
//! and does. This is written down because the last time a selector failed to
//! reach a target in this tree, nineteen tests had never run in any gate and
//! one of them was failing. Checked when this file was added.
//!
//! # What a red here means
//!
//! The transcription, or the golden being stale against a new pinned version.
//! The second changes every case at once; the first usually changes one
//! layout or one probe family, and the failure message names both.

use std::collections::HashMap;

use rustmc_data::BlockPos;
use rustmc_server::collide::{self, Aabb, CollisionBlocks, CollisionShapes, Vec3};

const GOLDEN: &str = include_str!("../golden/entity_move.json");

fn repo(relative: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

/// The layouts the dumper used, rebuilt here from the same block names.
///
/// **The names are duplicated between this file and the dumper deliberately.**
/// The alternative is for the golden to carry the layout as data, which would
/// make a layout that silently became empty — a renamed block, a typo — agree
/// with a port that also saw an empty layout. Spelling them twice means a
/// disagreement about what `floor_fence` contains shows up as a failing case
/// rather than as two matching descriptions of nothing.
fn layouts(ids: &DefaultStates) -> HashMap<String, MapWorld> {
    let stone = id(ids, "minecraft:stone");
    let mut out = HashMap::new();

    let floor = |extra: &[(BlockPos, i32)]| -> MapWorld {
        let mut blocks = HashMap::new();
        for x in -2..=2 {
            for z in -2..=2 {
                blocks.insert(BlockPos { x, y: -1, z }, stone);
            }
        }
        for (p, s) in extra {
            blocks.insert(*p, *s);
        }
        MapWorld {
            blocks,
            air: air(ids),
        }
    };
    let at1 = |name: &str| -> MapWorld { floor(&[(BlockPos { x: 1, y: 0, z: 0 }, id(ids, name))]) };

    out.insert(
        "empty".into(),
        MapWorld {
            blocks: HashMap::new(),
            air: air(ids),
        },
    );
    out.insert("floor".into(), floor(&[]));
    out.insert("floor_slab_step".into(), at1("minecraft:oak_slab"));
    out.insert("floor_stair_step".into(), at1("minecraft:oak_stairs"));
    out.insert("floor_block_step".into(), at1("minecraft:stone"));
    out.insert("floor_fence".into(), at1("minecraft:oak_fence"));
    out.insert("floor_wall".into(), at1("minecraft:cobblestone_wall"));
    out.insert("floor_soul_sand".into(), at1("minecraft:soul_sand"));
    out.insert("floor_honey".into(), at1("minecraft:honey_block"));
    out.insert("floor_anvil".into(), at1("minecraft:anvil"));
    out.insert("floor_trapdoor".into(), at1("minecraft:oak_trapdoor"));
    out.insert("floor_hopper".into(), at1("minecraft:hopper"));
    out.insert("floor_brewing_stand".into(), at1("minecraft:brewing_stand"));
    out.insert("floor_bars".into(), at1("minecraft:iron_bars"));
    out.insert("floor_lantern".into(), at1("minecraft:lantern"));
    out.insert("floor_composter".into(), at1("minecraft:composter"));
    out.insert("floor_cake".into(), at1("minecraft:cake"));
    out.insert("floor_gate".into(), at1("minecraft:oak_fence_gate"));
    out.insert("floor_end_rod".into(), at1("minecraft:end_rod"));
    out.insert("floor_grindstone".into(), at1("minecraft:grindstone"));

    let solo = |name: &str| -> MapWorld {
        let mut blocks = HashMap::new();
        blocks.insert(BlockPos { x: 0, y: 0, z: 0 }, id(ids, name));
        MapWorld {
            blocks,
            air: air(ids),
        }
    };
    out.insert("embedded_candle".into(), solo("minecraft:candle"));
    out.insert("embedded_stone".into(), solo("minecraft:stone"));
    out.insert("embedded_chain".into(), solo("minecraft:iron_chain"));

    out.insert(
        "corner_stone".into(),
        floor(&[
            (BlockPos { x: 1, y: 0, z: 0 }, stone),
            (BlockPos { x: 0, y: 0, z: 1 }, stone),
            (BlockPos { x: 1, y: 0, z: 1 }, stone),
        ]),
    );
    let fence = id(ids, "minecraft:oak_fence");
    out.insert(
        "corner_fence".into(),
        floor(&[
            (BlockPos { x: 1, y: 0, z: 0 }, fence),
            (BlockPos { x: 0, y: 0, z: 1 }, fence),
            (BlockPos { x: 1, y: 0, z: 1 }, fence),
        ]),
    );
    out.insert(
        "floor_and_ceiling".into(),
        floor(&[
            (BlockPos { x: 0, y: 3, z: 0 }, stone),
            (BlockPos { x: 1, y: 3, z: 0 }, stone),
            (BlockPos { x: 0, y: 3, z: 1 }, stone),
            (BlockPos { x: -1, y: 3, z: 0 }, stone),
        ]),
    );
    let mut holed = floor(&[]);
    holed.blocks.remove(&BlockPos { x: 1, y: -1, z: 0 });
    out.insert("floor_with_hole".into(), holed);

    // A lone block, no floor: the only layouts where admitting one particular
    // block to the collider list can change the answer, because nothing
    // behind it gives the same answer.
    let lone = |name: &str| -> MapWorld {
        let mut blocks = HashMap::new();
        blocks.insert(BlockPos { x: 1, y: 0, z: 0 }, id(ids, name));
        MapWorld {
            blocks,
            air: air(ids),
        }
    };
    out.insert("lone_block".into(), lone("minecraft:stone"));
    out.insert("lone_slab".into(), lone("minecraft:oak_slab"));
    out.insert("lone_fence".into(), lone("minecraft:oak_fence"));

    let slab = id(ids, "minecraft:oak_slab");
    let stairs = id(ids, "minecraft:oak_stairs");
    out.insert(
        "ceiling_low".into(),
        floor(&[
            (BlockPos { x: 0, y: 1, z: 0 }, stone),
            (BlockPos { x: 1, y: 1, z: 0 }, stone),
            (BlockPos { x: -1, y: 1, z: 0 }, stone),
            (BlockPos { x: 0, y: 1, z: 1 }, stone),
            (BlockPos { x: 1, y: 0, z: 0 }, slab),
        ]),
    );
    out.insert(
        "ceiling_low_stairs".into(),
        floor(&[
            (BlockPos { x: 0, y: 1, z: 0 }, stone),
            (BlockPos { x: 1, y: 1, z: 0 }, stone),
            (BlockPos { x: -1, y: 1, z: 0 }, stone),
            (BlockPos { x: 0, y: 1, z: 1 }, stone),
            (BlockPos { x: 1, y: 0, z: 0 }, stairs),
        ]),
    );

    // A million blocks out, matching the dumper's offset exactly. `VoxelShape
    // .move` adds the block position to each coordinate at read time, so how
    // far out the world is is a question about doubles and not about blocks.
    const FAR: i32 = 1_000_000;
    let far_floor = floor(&[]).shifted(FAR, 0, FAR);
    out.insert("far_floor".into(), far_floor);
    out.insert(
        "far_slab_step".into(),
        floor(&[(BlockPos { x: 1, y: 0, z: 0 }, slab)]).shifted(FAR, 0, FAR),
    );
    out.insert(
        "far_corner".into(),
        floor(&[
            (BlockPos { x: 1, y: 0, z: 0 }, stone),
            (BlockPos { x: 0, y: 0, z: 1 }, stone),
            (BlockPos { x: 1, y: 0, z: 1 }, stone),
        ])
        .shifted(FAR, 0, FAR),
    );
    out
}

/// Block name to the id of its **default** state.
///
/// **Not `terrain::StateIds`**, which is keyed by the serialized state and so
/// has no entry for a bare `minecraft:oak_slab` — every block with a property
/// spells its key `minecraft:oak_slab[type=bottom,waterlogged=false]`.
/// The dumper placed `Blocks.X.defaultBlockState()`, and `block_props.json`
/// carries a `default` flag per state for exactly this: it is the only thing
/// that says which of a block's states vanilla means by its bare name.
struct DefaultStates(HashMap<String, i32>);

impl DefaultStates {
    fn load(path: &std::path::Path) -> Self {
        let text = std::fs::read_to_string(path).expect("block table");
        let doc: serde_json::Value = serde_json::from_str(&text).expect("block table parses");
        let mut map = HashMap::new();
        for entry in doc["table"].as_array().expect("table array") {
            if entry["default"].as_bool() != Some(true) {
                continue;
            }
            let (Some(block), Some(id)) = (entry["block"].as_str(), entry["id"].as_i64()) else {
                continue;
            };
            map.insert(
                block.to_string(),
                i32::try_from(id).expect("state ids fit in i32"),
            );
        }
        assert!(
            map.len() > 1000,
            "only {} default states — the `default` column has gone missing",
            map.len()
        );
        Self(map)
    }
}

fn id(ids: &DefaultStates, block: &str) -> i32 {
    *ids.0
        .get(block)
        .unwrap_or_else(|| panic!("no default state id for {block}"))
}

fn air(ids: &DefaultStates) -> i32 {
    id(ids, "minecraft:air")
}

struct MapWorld {
    blocks: HashMap<BlockPos, i32>,
    air: i32,
}

impl MapWorld {
    /// The same world, translated. The dumper translates both the blocks and
    /// the probe box; the box arrives here from the golden already translated,
    /// so only the blocks need it.
    fn shifted(&self, dx: i32, dy: i32, dz: i32) -> Self {
        Self {
            blocks: self
                .blocks
                .iter()
                .map(|(p, s)| {
                    (
                        BlockPos {
                            x: p.x + dx,
                            y: p.y + dy,
                            z: p.z + dz,
                        },
                        *s,
                    )
                })
                .collect(),
            air: self.air,
        }
    }
}

impl CollisionBlocks for MapWorld {
    fn state_id_at(&self, pos: BlockPos) -> u32 {
        let raw = *self.blocks.get(&pos).unwrap_or(&self.air);
        u32::try_from(raw).expect("state ids are non-negative")
    }
}

fn bits_to_f64(v: &serde_json::Value) -> f64 {
    #[allow(clippy::cast_sign_loss)]
    f64::from_bits(v.as_i64().expect("a raw bit pattern") as u64)
}

fn vec3(v: &serde_json::Value) -> Vec3 {
    let a = v.as_array().expect("vector triple");
    Vec3::new(bits_to_f64(&a[0]), bits_to_f64(&a[1]), bits_to_f64(&a[2]))
}

fn same(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits()
}

fn same_vec(a: Vec3, b: Vec3) -> bool {
    same(a.x, b.x) && same(a.y, b.y) && same(a.z, b.z)
}

/// Both extracts this file needs, each asked for by name.
///
/// **The comment this replaces said "`ci/t0.sh` greps for this sentinel, so a
/// run that skipped everything cannot be read as a green one."** It does grep
/// for it, and it can never match: `cargo test` does not print a passing
/// test's stderr, and a test that returns early passes. The four tests below
/// ran zero comparisons on any tree without the extract and every gate over
/// them was green. See `crates/rustmc-data/src/fixture.rs`.
///
/// Asked separately rather than as one `||` so the failure names the file that
/// is actually missing rather than whichever the author wrote first.
fn have_both(table_path: &std::path::Path, props_path: &std::path::Path) -> bool {
    rustmc_data::fixture::available(
        table_path,
        "collision shape extract",
        "ci/collision-shapes-golden.sh",
    ) && rustmc_data::fixture::available(
        props_path,
        "block_props extract",
        "ci/block-props-golden.sh",
    )
}

#[test]
fn the_geometry_reproduces_vanillas_own_collisions() {
    let table_path = repo("data/vanilla/collision_shapes.json");
    let props_path = repo("data/vanilla/block_props.json");
    if !have_both(&table_path, &props_path) {
        return;
    }
    let shapes = CollisionShapes::load(&table_path).expect("collision shape table");
    let ids = DefaultStates::load(&props_path);
    let worlds = layouts(&ids);

    let doc: serde_json::Value = serde_json::from_str(GOLDEN).expect("golden parses");
    assert_eq!(
        doc["rng"].as_str(),
        Some("entity_move"),
        "wrong golden file"
    );
    let cases = doc["table"].as_array().expect("case array");
    assert!(!cases.is_empty(), "a golden with no cases cannot fail");

    let mut compared = 0usize;
    let mut collider_hits = 0usize;
    let mut clamped = 0usize;
    let mut stepped = 0usize;

    for case in cases {
        let layout = case["layout"].as_str().expect("layout name");
        let probe = case["probe"].as_str().expect("probe name");
        let world = worlds
            .get(layout)
            .unwrap_or_else(|| panic!("this test has no layout named {layout}"));

        let b = case["box"].as_array().expect("box");
        let box_ = Aabb::new(
            bits_to_f64(&b[0]),
            bits_to_f64(&b[1]),
            bits_to_f64(&b[2]),
            bits_to_f64(&b[3]),
            bits_to_f64(&b[4]),
            bits_to_f64(&b[5]),
        );
        let delta = vec3(&case["delta"]);
        #[allow(clippy::cast_sign_loss)]
        let max_up_step =
            f32::from_bits(case["maxUpStep"].as_i64().expect("maxUpStep bits") as u32);
        let on_ground = case["onGround"].as_bool().expect("onGround");

        let where_ = format!("layout {layout}, probe {probe}");

        // 1. the block set, in order
        let want_positions: Vec<BlockPos> = case["colliders"]
            .as_array()
            .expect("collider array")
            .iter()
            .map(|p| {
                let a = p.as_array().expect("position triple");
                BlockPos {
                    x: i32::try_from(a[0].as_i64().expect("x")).expect("x fits"),
                    y: i32::try_from(a[1].as_i64().expect("y")).expect("y fits"),
                    z: i32::try_from(a[2].as_i64().expect("z")).expect("z fits"),
                }
            })
            .collect();
        let expanded = box_.expand_towards(delta.x, delta.y, delta.z);
        let got = collide::collect_block_collisions(&shapes, world, expanded)
            .unwrap_or_else(|e| panic!("{where_}: {e}"));
        let got_positions: Vec<BlockPos> = got.iter().map(|s| s.pos).collect();
        assert_eq!(
            got_positions, want_positions,
            "{where_}: the collider list differs from vanilla's, in content or in order"
        );
        if !want_positions.is_empty() {
            collider_hits += 1;
        }

        // 2. the sweep
        let want_swept = vec3(&case["swept"]);
        let got_swept = collide::collide_bounding_box(&shapes, world, delta, box_)
            .unwrap_or_else(|e| panic!("{where_}: {e}"));
        assert!(
            same_vec(got_swept, want_swept),
            "{where_}: collideBoundingBox gave {got_swept:?}, vanilla gave {want_swept:?}"
        );

        // 3. the whole of collide, auto-step included
        let want_moved = vec3(&case["moved"]);
        let got_moved = collide::collide(&shapes, world, delta, box_, max_up_step, on_ground)
            .unwrap_or_else(|e| panic!("{where_}: {e}"));
        assert!(
            same_vec(got_moved, want_moved),
            "{where_}: collide gave {got_moved:?}, vanilla gave {want_moved:?}"
        );

        if !same_vec(want_moved, delta) {
            clamped += 1;
        }
        if !same_vec(want_moved, want_swept) {
            stepped += 1;
        }
        compared += 1;
    }

    // The counts the run has to be able to quote, and the guard against a
    // fixture that agrees because it asks nothing. A file on disk that
    // compared no cases, or only cases in empty space, would be green.
    assert!(compared >= 3000, "only {compared} cases compared");
    assert!(
        collider_hits >= compared / 4,
        "only {collider_hits} of {compared} cases had any collider — this is a \
         fixture about empty space"
    );
    assert!(
        clamped >= compared / 10,
        "only {clamped} of {compared} cases were clamped — an implementation that \
         returned its input unchanged would pass most of this"
    );
    assert!(
        stepped > 0,
        "no case had auto-step change the answer — the step-up arm is untested"
    );
    println!(
        "compared {compared} vanilla collide cases: {collider_hits} with colliders, \
         {clamped} clamped, {stepped} moved by auto-step"
    );
}

/// The 23 blocks with `hasDynamicShape()` get no row, and asking about one
/// raises rather than guessing.
///
/// This is the other half of the load-bearing claim in `collide.rs`: the
/// refusal has to actually fire, in the right place, or "no dynamic block was
/// ever asked about" and "dynamic blocks are handled" look identical.
#[test]
fn a_dynamic_shape_block_is_refused_rather_than_guessed() {
    let table_path = repo("data/vanilla/collision_shapes.json");
    let props_path = repo("data/vanilla/block_props.json");
    if !have_both(&table_path, &props_path) {
        return;
    }
    let shapes = CollisionShapes::load(&table_path).expect("collision shape table");
    let ids = DefaultStates::load(&props_path);

    for name in [
        "minecraft:scaffolding",
        "minecraft:powder_snow",
        "minecraft:shulker_box",
        "minecraft:pointed_dripstone",
        "minecraft:moving_piston",
        "minecraft:bamboo",
    ] {
        let raw = id(&ids, name);
        let state_id = u32::try_from(raw).expect("non-negative");
        assert!(
            shapes.get(state_id).is_none(),
            "{name} has a shape row — the table is claiming to answer for a \
             block whose shape is not a function of its state"
        );

        let mut blocks = HashMap::new();
        blocks.insert(BlockPos { x: 0, y: -1, z: 0 }, raw);
        let world = MapWorld {
            blocks,
            air: id(&ids, "minecraft:air"),
        };
        let box_ = Aabb::new(-0.3, 0.0, -0.3, 0.3, 1.8, 0.3);
        let err = collide::collide(&shapes, &world, Vec3::new(0.0, -0.5, 0.0), box_, 0.6, true)
            .expect_err("moving onto a dynamic-shape block must refuse, not guess");
        assert_eq!(err.state_id, state_id, "{name}: refused the wrong state");
        assert_eq!(
            err.pos,
            BlockPos { x: 0, y: -1, z: 0 },
            "{name}: refused the wrong position"
        );
    }
}

/// A shulker box diagonally past a corner of the swept box sits in a
/// `TYPE_EDGE` cell, which vanilla consults **only** for `moving_piston`. So
/// it must NOT refuse — a refusal there would be one vanilla never makes.
///
/// This is the case that separates "the table has no answer" from "vanilla
/// would not have asked", and it was the reason `moving_piston`'s state ids
/// are dumped alongside the shapes.
#[test]
fn a_dynamic_block_in_an_edge_cell_is_skipped_not_refused() {
    let table_path = repo("data/vanilla/collision_shapes.json");
    let props_path = repo("data/vanilla/block_props.json");
    if !have_both(&table_path, &props_path) {
        return;
    }
    let shapes = CollisionShapes::load(&table_path).expect("collision shape table");
    let ids = DefaultStates::load(&props_path);

    // A box strictly inside the cell (0,0,0) so the cursor spans -1..=1 on
    // every axis, making (1,1,1) a corner and (1,1,0) an edge.
    let box_ = Aabb::new(0.2, 0.2, 0.2, 0.8, 0.8, 0.8);
    let edge = BlockPos { x: 1, y: 1, z: 0 };

    let mut blocks = HashMap::new();
    blocks.insert(edge, id(&ids, "minecraft:shulker_box"));
    let world = MapWorld {
        blocks,
        air: id(&ids, "minecraft:air"),
    };
    let out = collide::collect_block_collisions(&shapes, &world, box_);
    assert!(
        out.is_ok(),
        "a shulker box in an edge cell must be skipped, not refused: {:?}",
        out.err()
    );

    // The same cell holding a moving piston IS consulted, so it must refuse.
    let mut blocks = HashMap::new();
    blocks.insert(edge, id(&ids, "minecraft:moving_piston"));
    let world = MapWorld {
        blocks,
        air: id(&ids, "minecraft:air"),
    };
    let err = collide::collect_block_collisions(&shapes, &world, box_)
        .expect_err("a moving piston in an edge cell is consulted and has no row");
    assert_eq!(err.pos, edge);
}

/// A `TYPE_CORNER` cell is skipped **before its block is read at all**, so a
/// block with no shape row sitting in one must not refuse — not even
/// `moving_piston`, which an edge cell does consult.
///
/// This exists because the falsification sweep found that making
/// `collect_block_collisions` stop skipping corners left the whole suite
/// green: every corner in the 3,640 golden cases holds air, so the skip was
/// unmeasured.
#[test]
fn a_corner_cell_is_skipped_before_its_block_is_read() {
    let table_path = repo("data/vanilla/collision_shapes.json");
    let props_path = repo("data/vanilla/block_props.json");
    if !have_both(&table_path, &props_path) {
        return;
    }
    let shapes = CollisionShapes::load(&table_path).expect("collision shape table");
    let ids = DefaultStates::load(&props_path);

    // A box strictly inside cell (0,0,0), so the cursor spans -1..=1 on every
    // axis and (1,1,1) is a corner.
    let box_ = Aabb::new(0.2, 0.2, 0.2, 0.8, 0.8, 0.8);
    let corner = BlockPos { x: 1, y: 1, z: 1 };

    for name in ["minecraft:moving_piston", "minecraft:shulker_box"] {
        let mut blocks = HashMap::new();
        blocks.insert(corner, id(&ids, name));
        let world = MapWorld {
            blocks,
            air: id(&ids, "minecraft:air"),
        };
        let out = collide::collect_block_collisions(&shapes, &world, box_);
        assert!(
            out.is_ok(),
            "{name} in a corner cell must be skipped before it is read: {:?}",
            out.err()
        );
        assert!(
            out.expect("skipped").is_empty(),
            "{name} in a corner cell produced a collider"
        );
    }

    // And a full block in the same corner produces no collider either, which
    // is the reason the skip is safe: a shape that fits inside its own cell
    // cannot reach a box that does not reach that cell.
    let mut blocks = HashMap::new();
    blocks.insert(corner, id(&ids, "minecraft:stone"));
    let world = MapWorld {
        blocks,
        air: id(&ids, "minecraft:air"),
    };
    assert!(
        collide::collect_block_collisions(&shapes, &world, box_)
            .expect("stone has a row")
            .is_empty(),
        "a corner cell must contribute nothing"
    );
}
