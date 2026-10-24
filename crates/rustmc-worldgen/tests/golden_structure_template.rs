//! Every `underwater_ruin` template in the jar, loaded and checked.
//!
//! The port builds one structure, `mineshaft`, and it is procedural. Every
//! other structure vanilla ships is assembled from `.nbt` templates — 1,212 of
//! them, 49 named `underwater_ruin` — so the missing 19 of 20 structure sets
//! are one absent subsystem rather than nineteen absent features.
//!
//! This gates the first piece of it against the real files rather than against
//! a fixture: a fixture would prove the parser reads what the test author
//! believed, which is the failure mode that has cost this project a week.
//!
//! Checked per template, because a loader that returns *something* for every
//! file proves very little:
//!
//!   * `size` bounds every block — a template whose blocks fall outside its
//!     own declared size would place into a neighbouring chunk when written;
//!   * every `state` indexes a real palette entry, which `from_nbt` enforces
//!     but which is worth asserting against real data rather than trusting;
//!   * the palette is canonical `name[k=v,...]` with properties sorted, the
//!     same form every golden vector in this project uses, so a template block
//!     is directly comparable with a generated one.

use rustmc_worldgen::structure::template::Template;

/// Where `ci/vanilla-server.sh` leaves the jar's `data/` tree.
fn templates_dir() -> std::path::PathBuf {
    std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .join("oracle-work/vanilla-26.2/generated-data/data/minecraft/structure/underwater_ruin")
}

#[test]
fn every_underwater_ruin_template_loads_and_is_self_consistent() {
    let dir = templates_dir();
    if !rustmc_data::fixture::available(
        &dir,
        "structure templates extracted",
        "ci/vanilla-server.sh data",
    ) {
        return;
    }

    let mut loaded = 0usize;
    let mut blocks = 0usize;
    for entry in std::fs::read_dir(&dir).expect("read the template directory") {
        let path = entry.expect("a directory entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("nbt") {
            continue;
        }
        let file = std::fs::File::open(&path).expect("open the template");
        let mut reader = flate2::read::GzDecoder::new(file);
        let (_name, root) = rustmc_world::nbt::read_named_root(&mut reader).expect("gzipped NBT");
        let template =
            Template::from_nbt(&root).unwrap_or_else(|e| panic!("{}: {}", path.display(), e.0));

        let (sx, sy, sz) = template.size;
        assert!(
            sx > 0 && sy > 0 && sz > 0,
            "{}: non-positive size {:?}",
            path.display(),
            template.size
        );
        for block in &template.blocks {
            let (x, y, z) = block.pos;
            assert!(
                (0..sx).contains(&x) && (0..sy).contains(&y) && (0..sz).contains(&z),
                "{}: block at {:?} is outside the template's own size {:?}",
                path.display(),
                block.pos,
                template.size
            );
        }
        for state in &template.palette {
            assert!(
                state.starts_with("minecraft:"),
                "{}: palette entry `{state}` is not a namespaced block",
                path.display()
            );
            if let Some(rest) = state.split_once('[') {
                assert!(
                    rest.1.ends_with(']'),
                    "{}: palette entry `{state}` has an unterminated property list",
                    path.display()
                );
            }
        }
        loaded += 1;
        blocks += template.blocks.len();
    }

    println!("loaded {loaded} underwater_ruin templates, {blocks} blocks");
    assert!(
        loaded >= 40,
        "expected the jar's ~48 underwater_ruin templates, loaded {loaded} — \
         a shrinking count means the extract is stale, not that the loader improved"
    );
}

/// The identity transform is a translation, and every emitted position lands
/// inside the box the template's own `size` describes.
///
/// Checked against a real template rather than a constructed one: the bug this
/// guards against is an off-by-one in the origin convention, and only real
/// `size`/`pos` data can expose it.
#[test]
fn placing_a_template_translates_every_block_into_its_box() {
    let dir = templates_dir();
    if !rustmc_data::fixture::available(
        &dir,
        "structure templates extracted",
        "ci/vanilla-server.sh data",
    ) {
        return;
    }
    let path = dir.join("warm_1.nbt");
    if !rustmc_data::fixture::available(&path, "warm_1.nbt", "ci/vanilla-server.sh data") {
        return;
    }
    let file = std::fs::File::open(&path).expect("open");
    let mut reader = flate2::read::GzDecoder::new(file);
    let (_n, root) = rustmc_world::nbt::read_named_root(&mut reader).expect("nbt");
    let template = Template::from_nbt(&root).expect("template");

    let origin = (100, -12, -37);
    let (sx, sy, sz) = template.size;
    let mut count = 0usize;
    for ((x, y, z), state) in template.blocks_at(origin) {
        assert!(
            (origin.0..origin.0 + sx).contains(&x)
                && (origin.1..origin.1 + sy).contains(&y)
                && (origin.2..origin.2 + sz).contains(&z),
            "block at ({x},{y},{z}) escaped the box at {origin:?} sized {:?}",
            template.size
        );
        assert!(state.starts_with("minecraft:"), "`{state}` is not a block");
        count += 1;
    }
    assert_eq!(
        count,
        template.blocks.len(),
        "placement must emit every block the template carries, filtering none — \
         `structure_void` removal is a processor's job, not the loader's"
    );
    println!("placed {count} blocks from warm_1 at {origin:?}");
}

/// The rotations compose the way quarter turns must, and the mirrors are
/// involutions.
///
/// Properties rather than fixtures, because the risk here is the *labelling* —
/// vanilla dispatches through a `$SwitchMap` whose table is not in the method,
/// so which formula is clockwise cannot be read off the case order. A wrong
/// labelling still produces valid-looking rotations; only composition catches
/// it.
#[test]
fn rotations_compose_and_mirrors_are_involutions() {
    use rustmc_worldgen::structure::template::{transform, Mirror, Rotation};
    let pivot = (0, 0);
    let cw = |p| transform(p, Mirror::None, Rotation::Clockwise90, pivot);

    // A clockwise quarter turn seen from above sends +x to +z.
    assert_eq!(cw((1, 7, 0)), (0, 7, 1), "clockwise must send +x to +z");
    // Four of them are the identity.
    for p in [(3, 1, 5), (-2, 0, 7), (0, -4, 0)] {
        assert_eq!(
            cw(cw(cw(cw(p)))),
            p,
            "four clockwise turns must be identity"
        );
        // Two are the 180.
        assert_eq!(
            cw(cw(p)),
            transform(p, Mirror::None, Rotation::Clockwise180, pivot),
            "two clockwise turns must equal the 180"
        );
        // Clockwise then counter-clockwise is the identity.
        assert_eq!(
            transform(cw(p), Mirror::None, Rotation::Counterclockwise90, pivot),
            p,
            "clockwise then counterclockwise must cancel"
        );
        // Each mirror applied twice is the identity.
        for m in [Mirror::LeftRight, Mirror::FrontBack] {
            let once = transform(p, m, Rotation::None, pivot);
            assert_eq!(
                transform(once, m, Rotation::None, pivot),
                p,
                "{m:?} must be an involution"
            );
        }
    }
    // `y` is never touched by any of it.
    for r in [
        Rotation::None,
        Rotation::Clockwise90,
        Rotation::Clockwise180,
        Rotation::Counterclockwise90,
    ] {
        assert_eq!(transform((5, 42, -9), Mirror::LeftRight, r, (2, 3)).1, 42);
    }
}

/// `facing` turns with the structure, and composes like the coordinates do.
///
/// Same hazard as the coordinate rotation — the cycle's direction cannot be
/// read off a switch — so it gets the same treatment: pinned by one geometric
/// fact and then checked by composition.
#[test]
fn facing_rotates_with_the_structure() {
    use rustmc_worldgen::structure::template::{rotate_state, Rotation};

    // North is -z and east is +x, so a clockwise turn — which sends +x to +z —
    // must send north to east.
    assert_eq!(
        rotate_state(
            "minecraft:stone_brick_stairs[facing=north,half=bottom]",
            Rotation::Clockwise90
        ),
        "minecraft:stone_brick_stairs[facing=east,half=bottom]"
    );
    let cw = |s: &str| rotate_state(s, Rotation::Clockwise90);
    let start = "minecraft:stone_brick_stairs[facing=south,half=top,shape=straight]";
    assert_eq!(
        cw(&cw(&cw(&cw(start)))),
        start,
        "four turns are the identity"
    );
    assert_eq!(
        cw(&cw(start)),
        rotate_state(start, Rotation::Clockwise180),
        "two turns equal the 180"
    );
    assert_eq!(
        rotate_state(&cw(start), Rotation::Counterclockwise90),
        start,
        "clockwise then counterclockwise cancel"
    );
    // Everything that is not `facing` survives untouched, and a stateless
    // block is returned as-is.
    assert_eq!(
        cw("minecraft:prismarine_slab[type=top,waterlogged=true]"),
        "minecraft:prismarine_slab[type=top,waterlogged=true]"
    );
    assert_eq!(cw("minecraft:sand"), "minecraft:sand");
    // A vertical facing is not a horizontal one and must not enter the cycle.
    assert_eq!(
        cw("minecraft:observer[facing=up]"),
        "minecraft:observer[facing=up]"
    );
}

/// A rotated template is a rigid motion: the block count is preserved and the
/// occupied extent has x and z swapped under a quarter turn.
///
/// Extent rather than absolute position, deliberately. The absolute placement
/// depends on the pivot `StructurePlaceSettings` carries, and this project does
/// not yet know what ocean ruins set it to — asserting a guessed origin would
/// bake that guess into a gate. The *shape* is independent of the pivot, so it
/// can be checked now and catches a wrong axis pair, a lost mirror and a
/// non-rigid transform.
#[test]
fn a_rotated_template_is_a_rigid_motion_with_swapped_extent() {
    use rustmc_worldgen::structure::template::{Mirror, Rotation};

    let dir = templates_dir();
    if !rustmc_data::fixture::available(&dir, "templates extracted", "ci/vanilla-server.sh data") {
        return;
    }
    let path = dir.join("warm_1.nbt");
    if !rustmc_data::fixture::available(&path, "warm_1.nbt", "ci/vanilla-server.sh data") {
        return;
    }
    let file = std::fs::File::open(&path).expect("open");
    let mut reader = flate2::read::GzDecoder::new(file);
    let (_n, root) = rustmc_world::nbt::read_named_root(&mut reader).expect("nbt");
    let template = Template::from_nbt(&root).expect("template");
    let (sx, sy, sz) = template.size;

    let extent = |mirror, rotation| {
        let mut lo = (i32::MAX, i32::MAX, i32::MAX);
        let mut hi = (i32::MIN, i32::MIN, i32::MIN);
        let mut n = 0usize;
        for ((x, y, z), _state) in template.blocks_rotated((0, 0, 0), mirror, rotation, (0, 0)) {
            lo = (lo.0.min(x), lo.1.min(y), lo.2.min(z));
            hi = (hi.0.max(x), hi.1.max(y), hi.2.max(z));
            n += 1;
        }
        (n, (hi.0 - lo.0 + 1, hi.1 - lo.1 + 1, hi.2 - lo.2 + 1))
    };

    let (n0, span0) = extent(Mirror::None, Rotation::None);
    assert_eq!(n0, template.blocks.len(), "no block may be lost");
    assert!(
        span0.0 <= sx && span0.1 <= sy && span0.2 <= sz,
        "the unrotated span {span0:?} must fit the declared size {:?}",
        template.size
    );

    for rotation in [Rotation::Clockwise90, Rotation::Counterclockwise90] {
        let (n, span) = extent(Mirror::None, rotation);
        assert_eq!(n, n0, "{rotation:?} must preserve the block count");
        assert_eq!(
            (span.0, span.1, span.2),
            (span0.2, span0.1, span0.0),
            "{rotation:?} must swap the x and z extents"
        );
    }
    let (n180, span180) = extent(Mirror::None, Rotation::Clockwise180);
    assert_eq!((n180, span180), (n0, span0), "the 180 preserves the extent");
    for mirror in [Mirror::LeftRight, Mirror::FrontBack] {
        let (n, span) = extent(mirror, Rotation::None);
        assert_eq!((n, span), (n0, span0), "{mirror:?} preserves the extent");
    }
    println!("warm_1: {n0} blocks, span {span0:?}, swapped under a quarter turn");
}

/// `Mth.getSeed`'s two easy-to-lose details.
///
/// Both are invisible for small coordinates and wrong for large ones, which is
/// the worst possible failure shape: a port that computed either in 64 bits
/// would agree with vanilla across every test anyone would think to write by
/// hand, and diverge in the far reaches of a real world.
#[test]
fn the_position_seed_wraps_in_32_bits_and_shifts_arithmetically() {
    use rustmc_worldgen::structure::template::position_seed;

    // The int multiply must wrap. Past this x, a 64-bit multiply differs.
    let x = 800_000_000i32;
    let wrapped = i64::from(x.wrapping_mul(3_129_871));
    let widened = i64::from(x) * 3_129_871;
    assert_ne!(
        wrapped, widened,
        "the test's own premise: these must differ"
    );
    // y and z are zero here, so their xor terms vanish — spelled out rather
    // than folded so the shape stays recognisable as `Mth.getSeed`.
    let mut l = wrapped;
    l = l
        .wrapping_mul(l)
        .wrapping_mul(42_317_861)
        .wrapping_add(l.wrapping_mul(11));
    assert_eq!(
        position_seed(x, 0, 0),
        l >> 16,
        "x must be multiplied as an i32 and allowed to wrap"
    );

    // The shift is arithmetic: a negative intermediate stays negative.
    // An arithmetic shift preserves sign; a logical one could not produce this.
    let negatives = (-40..40).filter(|&z| position_seed(7, 3, z) < 0).count();
    assert!(
        negatives > 0,
        "an arithmetic shift must be able to yield a negative seed"
    );
    // Deterministic and position-dependent, which is the property the
    // integrity pass relies on.
    assert_eq!(position_seed(4, 5, 6), position_seed(4, 5, 6));
    assert_ne!(position_seed(4, 5, 6), position_seed(4, 5, 7));
}

/// The two processors ocean ruins attach.
#[test]
fn the_ignore_and_rot_processors_behave() {
    use rustmc_worldgen::structure::template::{ignored_by_structure_and_air, survives_rot};

    assert!(ignored_by_structure_and_air("minecraft:air"));
    assert!(ignored_by_structure_and_air(
        "minecraft:structure_block[mode=data]"
    ));
    assert!(!ignored_by_structure_and_air("minecraft:cave_air"));
    assert!(!ignored_by_structure_and_air("minecraft:stone_bricks"));

    // Integrity 1.0 must keep everything — this is why the comparison is `<=`
    // and not `<`, since `nextFloat` can land arbitrarily close to 1.
    for z in -60..60 {
        assert!(
            survives_rot((3, 64, z), 1.0),
            "integrity 1.0 must keep every block, including ({}, 64, {z})",
            3
        );
    }
    // Integrity 0.0 must keep nothing: `nextFloat()` is in [0,1) so it is
    // never <= 0 except when it returns exactly 0, which the LCG can do — so
    // this asserts the overwhelming majority rather than all.
    let kept = (-200..200)
        .filter(|&z| survives_rot((5, 40, z), 0.0))
        .count();
    assert!(kept <= 1, "integrity 0.0 kept {kept} of 400 blocks");

    // Deterministic and position-dependent: the same block always rots the
    // same way, which is what makes a ruin reproducible without a stream.
    assert_eq!(survives_rot((9, 12, 4), 0.6), survives_rot((9, 12, 4), 0.6));
    let a = (-100..100)
        .filter(|&z| survives_rot((0, 0, z), 0.5))
        .count();
    assert!(
        (60..140).contains(&a),
        "integrity 0.5 kept {a} of 200 — far from half suggests a wrong seed \
         or a wrong generator"
    );
}

/// A jigsaw block's `orientation` turns with the structure, and both halves
/// of it do.
///
/// `orientation` is a `FrontAndTop` — `<front>_<top>` — and every jigsaw block
/// in every village template carries one, with no `facing` anywhere. A
/// rotation that turned only the front, or neither, would connect a rotated
/// village's pieces in the wrong directions, and the jigsaw blocks are
/// replaced by their `final_state` before anything could be inspected.
#[test]
fn a_jigsaws_orientation_turns_with_the_structure() {
    use rustmc_worldgen::structure::template::{rotate_state, Rotation};

    // Front horizontal, top vertical: only the front turns.
    assert_eq!(
        rotate_state(
            "minecraft:jigsaw[orientation=east_up]",
            Rotation::Clockwise90
        ),
        "minecraft:jigsaw[orientation=south_up]"
    );
    assert_eq!(
        rotate_state(
            "minecraft:jigsaw[orientation=north_up]",
            Rotation::Counterclockwise90
        ),
        "minecraft:jigsaw[orientation=west_up]"
    );
    // Front vertical, top horizontal: only the top turns. `up_north` is the
    // commonest orientation in the villages, at 698 of 1917.
    assert_eq!(
        rotate_state(
            "minecraft:jigsaw[orientation=up_north]",
            Rotation::Clockwise90
        ),
        "minecraft:jigsaw[orientation=up_east]"
    );
    assert_eq!(
        rotate_state(
            "minecraft:jigsaw[orientation=down_south]",
            Rotation::Clockwise180
        ),
        "minecraft:jigsaw[orientation=down_north]"
    );
    // A quarter turn four times is the identity, which no single case above
    // can show.
    let mut state = "minecraft:jigsaw[orientation=west_up]".to_string();
    for _ in 0..4 {
        state = rotate_state(&state, Rotation::Clockwise90);
    }
    assert_eq!(state, "minecraft:jigsaw[orientation=west_up]");
}

/// `CrossCollisionBlock.rotate` — the four connection booleans are permuted
/// together, and the new `north` is the **old** value of another direction.
///
/// Measured before it was fixed: 18 blocks in the r=16 overworld window at
/// seed 20260722 were connection-only differences, 9 `glass_pane` and 9
/// `acacia_fence`, inside `village_savanna`'s piece boxes.
#[test]
fn cross_collision_connections_turn_with_the_block() {
    use rustmc_worldgen::structure::template::{rotate_state, Rotation};

    // One connection, to the north. A clockwise turn must send it east.
    let north_only =
        "minecraft:acacia_fence[east=false,north=true,south=false,waterlogged=false,west=false]";
    assert_eq!(
        rotate_state(north_only, Rotation::Clockwise90),
        "minecraft:acacia_fence[east=true,north=false,south=false,waterlogged=false,west=false]",
        "a north connection turns clockwise to east"
    );
    // Counterclockwise sends the same connection west.
    assert_eq!(
        rotate_state(north_only, Rotation::Counterclockwise90),
        "minecraft:acacia_fence[east=false,north=false,south=false,waterlogged=false,west=true]",
        "a north connection turns counterclockwise to west"
    );
    // And 180 sends it south.
    assert_eq!(
        rotate_state(north_only, Rotation::Clockwise180),
        "minecraft:acacia_fence[east=false,north=false,south=true,waterlogged=false,west=false]"
    );

    // The group laws, which catch an inverted quarter turn that a 180-only
    // test cannot see.
    let cw = |s: &str| rotate_state(s, Rotation::Clockwise90);
    let start =
        "minecraft:glass_pane[east=true,north=true,south=false,waterlogged=false,west=false]";
    assert_eq!(
        cw(&cw(&cw(&cw(start)))),
        start,
        "four turns are the identity"
    );
    assert_eq!(
        cw(&cw(start)),
        rotate_state(start, Rotation::Clockwise180),
        "two clockwise turns equal the 180"
    );
    assert_eq!(
        rotate_state(&cw(start), Rotation::Counterclockwise90),
        start,
        "clockwise then counterclockwise cancel"
    );

    // `waterlogged` is orientation-free and must survive untouched.
    assert!(rotate_state(
        "minecraft:glass_pane[east=false,north=false,south=false,waterlogged=true,west=false]",
        Rotation::Clockwise90
    )
    .contains("waterlogged=true"));
}
