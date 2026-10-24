//! `CROSS_COLLISION_BLOCKS` against the pinned extract.
//!
//! The constant in `structure::template` is a hardcoded 39-name list, and a
//! hardcoded list that nothing re-derives is how a data change becomes a
//! silent mis-rotation. This re-reads `generated/reports/blocks.json` and
//! fails if the set ever parts from it.
//!
//! It also pins the other four-boolean types both ways: the six whose
//! classes override `rotate` with the same table must turn, and fire and the
//! chorus plant, which override nothing, must not.

use rustmc_worldgen::structure::template::{
    is_cross_collision, permutes_connections, rotate_state, Rotation, CROSS_COLLISION_BLOCKS,
};

fn blocks_report() -> serde_json::Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../oracle-work/vanilla-26.2/generated/reports/blocks.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    serde_json::from_str(&text).expect("valid JSON")
}

/// The four `definition.type`s whose classes extend `CrossCollisionBlock`.
const CROSS_TYPES: [&str; 4] = [
    "minecraft:fence",
    "minecraft:iron_bars",
    "minecraft:stained_glass_pane",
    "minecraft:weathering_copper_bar",
];

#[test]
fn the_constant_is_exactly_what_the_extract_says() {
    let report = blocks_report();
    let mut derived: Vec<String> = report
        .as_object()
        .expect("blocks.json is an object")
        .iter()
        .filter(|(_, b)| {
            b.get("definition")
                .and_then(|d| d.get("type"))
                .and_then(serde_json::Value::as_str)
                .is_some_and(|t| CROSS_TYPES.contains(&t))
        })
        .map(|(name, _)| name.clone())
        .collect();
    derived.sort();

    let listed: Vec<String> = CROSS_COLLISION_BLOCKS
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    assert_eq!(derived, listed, "the extract and the constant have parted");
    assert_eq!(derived.len(), 39);
}

/// `binary_search` in `is_cross_collision` is only correct on a sorted list,
/// and an unsorted entry would make a *subset* of the blocks silently stop
/// rotating rather than fail loudly.
#[test]
fn the_constant_is_sorted() {
    let mut sorted = CROSS_COLLISION_BLOCKS;
    sorted.sort_unstable();
    assert_eq!(sorted, CROSS_COLLISION_BLOCKS, "the list must stay sorted");
    for name in CROSS_COLLISION_BLOCKS {
        assert!(is_cross_collision(name), "{name} should be found");
    }
}

/// The four-boolean block types whose classes override `rotate` with the
/// same mapping as `CrossCollisionBlock`, read off the 26.2 jar: `VineBlock`,
/// `TripWireBlock`, `MultifaceBlock` (glow lichen, sculk vein, resin clump)
/// and `HugeMushroomBlock`. `FireBlock` and `ChorusPlantBlock` override
/// neither `rotate` nor `mirror`.
const OWN_TABLE_TYPES: [&str; 6] = [
    "minecraft:vine",
    "minecraft:tripwire",
    "minecraft:glow_lichen",
    "minecraft:sculk_vein",
    "minecraft:multiface",
    "minecraft:huge_mushroom",
];

/// **The negative half.** Every other block type with the four booleans must
/// be left alone, and this is asserted against the extract rather than
/// against a remembered list.
///
/// This said "every block type that is not a `CrossCollisionBlock`" and named
/// the vine, the tripwire, glow lichen and the sculk vein among the protected.
/// All four override `rotate` in the jar; the woodland mansion's vines and the
/// ancient city's veins were wrong while this test was green, and it went red
/// the day the vine was fixed. The positive half below holds them now.
#[test]
fn the_other_four_boolean_blocks_are_not_rotated() {
    let report = blocks_report();
    let mut others = Vec::new();
    for (name, b) in report.as_object().expect("object") {
        let props = b.get("properties");
        let has_all = ["north", "east", "south", "west"].iter().all(|k| {
            props
                .and_then(|p| p.get(*k))
                .and_then(serde_json::Value::as_array)
                .is_some_and(|v| v.len() == 2)
        });
        let ty = b
            .get("definition")
            .and_then(|d| d.get("type"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        if has_all && !CROSS_TYPES.contains(&ty) && !OWN_TABLE_TYPES.contains(&ty) {
            others.push(name.clone());
        }
    }
    others.sort();
    assert!(
        !others.is_empty(),
        "the negative half is the point; if it is empty the filter is broken"
    );
    for name in &others {
        assert!(
            !is_cross_collision(name) && !permutes_connections(name),
            "{name} overrides no rotate and must not be permuted"
        );
        // And rotating one really does leave the connections untouched.
        let state = format!("{name}[east=true,north=false,south=false,west=false]");
        assert_eq!(
            rotate_state(&state, Rotation::Clockwise90),
            state,
            "{name} must not have its connections turned"
        );
    }
    // Named for the record, so a future reader sees which blocks this is
    // protecting rather than only a count.
    for expected in ["minecraft:fire", "minecraft:chorus_plant"] {
        assert!(
            others.contains(&expected.to_string()),
            "{expected} should be in the protected set"
        );
    }
}

/// **The positive half.** Every block of an [`OWN_TABLE_TYPES`] type turns its
/// four booleans with the cross-collision table: a quarter turn clockwise
/// carries `north` to `east`.
#[test]
fn the_four_boolean_blocks_with_their_own_rotate_turn() {
    let report = blocks_report();
    let mut turned = Vec::new();
    for (name, b) in report.as_object().expect("object") {
        let ty = b
            .get("definition")
            .and_then(|d| d.get("type"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        if !OWN_TABLE_TYPES.contains(&ty) {
            continue;
        }
        assert!(permutes_connections(name), "{name} ({ty}) must turn");
        let state = format!("{name}[east=false,north=true,south=false,west=false]");
        assert_eq!(
            rotate_state(&state, Rotation::Clockwise90),
            format!("{name}[east=true,north=false,south=false,west=false]"),
            "{name} must carry north to east under CLOCKWISE_90"
        );
        turned.push(name.clone());
    }
    turned.sort();
    assert_eq!(
        turned.len(),
        8,
        "vine, tripwire, glow lichen, sculk vein, resin clump and the three \
         huge mushroom blocks: {turned:?}"
    );
}

/// `WALL_BLOCKS` against the same extract, derived by `definition.type`.
///
/// A wall is **not** a `CrossCollisionBlock` — `WallBlock` extends `Block`
/// and its four connection properties are `WallSide` enums rather than
/// booleans — so it gets its own list and the two are asserted separately.
/// Merging them would make `the_constant_is_exactly_what_the_extract_says`
/// false and would turn a class-hierarchy fact into a convenience.
#[test]
fn the_wall_list_is_exactly_what_the_extract_says() {
    let report = blocks_report();
    let mut derived: Vec<String> = report
        .as_object()
        .expect("blocks.json is an object")
        .iter()
        .filter(|(_, b)| {
            b.get("definition")
                .and_then(|d| d.get("type"))
                .and_then(serde_json::Value::as_str)
                == Some("minecraft:wall")
        })
        .map(|(name, _)| name.clone())
        .collect();
    derived.sort();

    let listed: Vec<String> = rustmc_worldgen::structure::template::WALL_BLOCKS
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    assert_eq!(
        derived, listed,
        "the extract and the wall constant have parted"
    );
    assert_eq!(derived.len(), 32);

    // Disjoint, and asserted rather than assumed: an entry in both lists
    // would rotate twice through `permutes_connections` if either predicate
    // ever stopped being a pure membership test.
    for name in &listed {
        assert!(
            !rustmc_worldgen::structure::template::is_cross_collision(name),
            "{name} is in both lists"
        );
    }
}

/// `binary_search` needs a sorted list here for the same reason it does
/// above: an unsorted entry makes a SUBSET of the walls silently stop
/// rotating, which is a smaller and much harder-to-see defect than none of
/// them rotating.
#[test]
fn the_wall_list_is_sorted() {
    let mut sorted = rustmc_worldgen::structure::template::WALL_BLOCKS;
    sorted.sort_unstable();
    assert_eq!(
        sorted,
        rustmc_worldgen::structure::template::WALL_BLOCKS,
        "the wall list must stay sorted"
    );
    for name in rustmc_worldgen::structure::template::WALL_BLOCKS {
        assert!(
            rustmc_worldgen::structure::template::is_wall(name),
            "{name} should be found"
        );
    }
}

/// The permutation itself, on the exact state pair the bastion's ramparts
/// disagreed on.
///
/// Vanilla wrote `east=low,west=low` where this port wrote
/// `north=low,south=low` at 15 positions of `bastion_remnant (55,63)`, on a
/// piece placed `CLOCKWISE_90`. `up` and `waterlogged` are fixed points and
/// are asserted to be, because a table that permuted five properties instead
/// of four would pass a test that only looked at the four.
#[test]
fn a_wall_rotates_its_connections_and_nothing_else() {
    use rustmc_worldgen::structure::template::{rotate_state, Rotation};
    let template =
        "minecraft:blackstone_wall[east=none,north=low,south=low,up=false,waterlogged=false,west=none]";
    assert_eq!(
        rotate_state(template, Rotation::Clockwise90),
        "minecraft:blackstone_wall[east=low,north=none,south=none,up=false,waterlogged=false,west=low]"
    );
    // A quarter turn the other way gives the same set here, because the
    // template's connections are on one axis — so the 180 case is the one
    // that distinguishes a correct table from one that is off by a turn.
    assert_eq!(
        rotate_state(
            "minecraft:blackstone_wall[east=none,north=tall,south=none,up=true,waterlogged=false,west=none]",
            Rotation::Clockwise90
        ),
        "minecraft:blackstone_wall[east=tall,north=none,south=none,up=true,waterlogged=false,west=none]"
    );
    assert_eq!(
        rotate_state(
            "minecraft:blackstone_wall[east=none,north=tall,south=none,up=true,waterlogged=false,west=none]",
            Rotation::Counterclockwise90
        ),
        "minecraft:blackstone_wall[east=none,north=none,south=none,up=true,waterlogged=false,west=tall]"
    );
}
