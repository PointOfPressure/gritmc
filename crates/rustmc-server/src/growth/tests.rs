//! The bodies against a scripted random source: every draw a body takes is
//! asserted by bound and order, so a body that drew one too many, too few
//! or with the wrong bound fails here and not only as a skewed distribution.

use super::*;
use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;

use rustmc_behavior::block_props::BlockPropsTable;

/// A world of explicit states over air, with the real ids and defaults.
struct World {
    blocks: Mutex<BTreeMap<BlockPos, String>>,
    props: BlockPropsTable,
    ids: std::collections::HashMap<String, i32>,
}

fn table() -> Option<std::path::PathBuf> {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/vanilla/block_props.json");
    rustmc_data::fixture::available(
        &p,
        "vanilla block_props extract",
        "ci/block-props-golden.sh",
    )
    .then_some(p)
}

impl World {
    fn with(blocks: &[(BlockPos, &str)]) -> Option<Self> {
        let p = table()?;
        let doc: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&p).ok()?).ok()?;
        let ids = doc["table"]
            .as_array()?
            .iter()
            .filter_map(|r| Some((r["state"].as_str()?.to_string(), r["id"].as_i64()? as i32)))
            .collect();
        Some(Self {
            blocks: Mutex::new(blocks.iter().map(|(p, s)| (*p, (*s).to_string())).collect()),
            props: BlockPropsTable::load(&p).ok()?,
            ids,
        })
    }
    fn at(&self, p: BlockPos) -> String {
        self.state_at(p)
    }
}

impl Blocks for World {
    fn state_at(&self, pos: BlockPos) -> String {
        self.blocks
            .lock()
            .unwrap()
            .get(&pos)
            .cloned()
            .unwrap_or_else(|| "minecraft:air".into())
    }
    fn set_block(&self, pos: BlockPos, state: &str) -> Option<i32> {
        if self.state_at(pos) == state {
            return None;
        }
        let id = *self.ids.get(state)?;
        self.blocks.lock().unwrap().insert(pos, state.into());
        Some(id)
    }
    fn default_state(&self, b: &str) -> Option<String> {
        self.props.default_state(b).map(ToString::to_string)
    }
    fn has_state(&self, s: &str) -> bool {
        self.ids.contains_key(s)
    }
    fn state_id(&self, s: &str) -> Option<i32> {
        self.ids.get(s).copied()
    }
    fn replaceable(&self, _: BlockPos, _: &crate::placement::Replacing<'_>) -> bool {
        false
    }
    fn face_sturdy(&self, _: BlockPos, _: crate::interact::Face) -> bool {
        false
    }
    fn redstone_conductor(&self, _: BlockPos) -> bool {
        false
    }
    fn signal_source(&self, _: BlockPos) -> bool {
        false
    }
    fn water_at(&self, pos: BlockPos) -> bool {
        matches!(
            self.props
                .get(&self.state_at(pos))
                .and_then(|p| p.fluid.as_deref()),
            Some("minecraft:water" | "minecraft:flowing_water")
        )
    }
    fn legacy_fluid_at(&self, _: BlockPos) -> String {
        "minecraft:air".into()
    }
    fn level_min_y(&self) -> Option<i32> {
        Some(-64)
    }
    fn place_props(&self) -> Option<&BlockPropsTable> {
        Some(&self.props)
    }
}

/// A random source that answers from a script and fails on any draw it was
/// not told to expect.
#[derive(Default)]
struct Script {
    ints: VecDeque<(i32, i32)>,
    doubles: VecDeque<f64>,
}

impl Script {
    fn ints(draws: &[(i32, i32)]) -> Self {
        Self {
            ints: draws.iter().copied().collect(),
            doubles: VecDeque::new(),
        }
    }
    fn done(&self) {
        assert!(self.ints.is_empty(), "draws left untaken: {:?}", self.ints);
        assert!(
            self.doubles.is_empty(),
            "doubles left untaken: {:?}",
            self.doubles
        );
    }
}

impl RandomSource for Script {
    fn set_seed(&mut self, _: i64) {}
    fn next_i32(&mut self) -> i32 {
        panic!("unexpected nextInt()")
    }
    fn next_i32_bound(&mut self, bound: i32) -> i32 {
        let (want, value) = self
            .ints
            .pop_front()
            .unwrap_or_else(|| panic!("unexpected nextInt({bound})"));
        assert_eq!(bound, want, "nextInt bound");
        value
    }
    fn next_i64(&mut self) -> i64 {
        panic!("unexpected nextLong()")
    }
    fn next_bool(&mut self) -> bool {
        panic!("unexpected nextBoolean()")
    }
    fn next_f32(&mut self) -> f32 {
        panic!("unexpected nextFloat()")
    }
    fn next_f64(&mut self) -> f64 {
        self.doubles.pop_front().expect("unexpected nextDouble()")
    }
    fn next_gaussian(&mut self) -> f64 {
        panic!("unexpected nextGaussian()")
    }
}

fn run(w: &World, rng: &mut Script, kind: Kind, pos: BlockPos) -> Effects {
    let light = Light::new(w, 320, true, 0);
    let mut fx = Effects::new();
    let state = w.at(pos);
    let mut cx = Ctx {
        world: w,
        shapes: None,
        light: &light,
        rng,
        fx: &mut fx,
        raining: false,
    };
    random_tick(&mut cx, kind, pos, &state);
    fx
}

const WHEAT: Kind = Kind::Crop {
    max_age: 7,
    gate: false,
};

fn wheat_rig(age: i32) -> Option<World> {
    World::with(&[
        ((0, -61, 0), "minecraft:farmland[moisture=0]"),
        ((0, -60, 0), &format!("minecraft:wheat[age={age}]")),
    ])
}

#[test]
fn a_lone_wheat_on_dry_farmland_draws_one_in_thirteen() {
    // f = 1 + 1 (its own dry farmland) = 2; (int)(25 / 2) + 1 = 13.
    let Some(w) = wheat_rig(0) else { return };
    let mut rng = Script::ints(&[(13, 0)]);
    let fx = run(&w, &mut rng, WHEAT, (0, -60, 0));
    rng.done();
    assert_eq!(w.at((0, -60, 0)), "minecraft:wheat[age=1]");
    assert_eq!(fx.blocks.len(), 1, "flags 2 sends the one block");
}

#[test]
fn a_failed_wheat_draw_writes_nothing() {
    let Some(w) = wheat_rig(3) else { return };
    let mut rng = Script::ints(&[(13, 5)]);
    run(&w, &mut rng, WHEAT, (0, -60, 0));
    rng.done();
    assert_eq!(w.at((0, -60, 0)), "minecraft:wheat[age=3]");
}

#[test]
fn moist_farmland_and_its_neighbours_raise_the_speed() {
    // Own moist farmland 3, eight moist neighbours 3/4 each: f = 1 + 3 + 6 = 10,
    // (int)(2.5) + 1 = 3.
    let mut blocks: Vec<(BlockPos, String)> = Vec::new();
    for x in -1..=1 {
        for z in -1..=1 {
            blocks.push(((x, -61, z), "minecraft:farmland[moisture=7]".into()));
        }
    }
    blocks.push(((0, -60, 0), "minecraft:wheat[age=0]".into()));
    let refs: Vec<(BlockPos, &str)> = blocks.iter().map(|(p, s)| (*p, s.as_str())).collect();
    let Some(w) = World::with(&refs) else { return };
    let f = growth_speed(&w, "minecraft:wheat", (0, -60, 0), |s| {
        block_of(s) == "minecraft:farmland"
    });
    assert!((f - 10.0).abs() < 1e-6, "{f}");
    let mut rng = Script::ints(&[(3, 0)]);
    run(&w, &mut rng, WHEAT, (0, -60, 0));
    rng.done();
}

#[test]
fn rows_on_both_axes_halve_the_speed() {
    let Some(w) = World::with(&[
        ((0, -61, 0), "minecraft:farmland[moisture=0]"),
        ((0, -60, 0), "minecraft:wheat[age=0]"),
        ((1, -60, 0), "minecraft:wheat[age=0]"),
        ((0, -60, 1), "minecraft:wheat[age=0]"),
    ]) else {
        return;
    };
    let f = growth_speed(&w, "minecraft:wheat", (0, -60, 0), |s| {
        block_of(s) == "minecraft:farmland"
    });
    assert!((f - 1.0).abs() < 1e-6, "(1 + 1) / 2, got {f}");
}

#[test]
fn a_crop_in_the_dark_takes_no_draw() {
    let Some(w) = World::with(&[
        ((0, -61, 0), "minecraft:farmland[moisture=0]"),
        ((0, -60, 0), "minecraft:wheat[age=0]"),
        ((0, -59, 0), "minecraft:stone"),
        ((1, -60, 0), "minecraft:stone"),
        ((-1, -60, 0), "minecraft:stone"),
        ((0, -60, 1), "minecraft:stone"),
        ((0, -60, -1), "minecraft:stone"),
        // The farmland is no roof: it dampens nothing, so it has to be
        // walled in too or the sky reaches the crop through it.
        ((1, -61, 0), "minecraft:stone"),
        ((-1, -61, 0), "minecraft:stone"),
        ((0, -61, 1), "minecraft:stone"),
        ((0, -61, -1), "minecraft:stone"),
        ((0, -62, 0), "minecraft:stone"),
    ]) else {
        return;
    };
    let mut rng = Script::default();
    run(&w, &mut rng, WHEAT, (0, -60, 0));
    rng.done();
}

#[test]
fn beetroot_spends_a_third_draw_before_the_crop_draw() {
    let Some(w) = World::with(&[
        ((0, -61, 0), "minecraft:farmland[moisture=0]"),
        ((0, -60, 0), "minecraft:beetroots[age=0]"),
    ]) else {
        return;
    };
    // `nextInt(3) == 0` returns; anything else goes on to the crop draw.
    let mut rng = Script::ints(&[(3, 0)]);
    run(
        &w,
        &mut rng,
        Kind::Crop {
            max_age: 3,
            gate: true,
        },
        (0, -60, 0),
    );
    rng.done();
    let mut rng = Script::ints(&[(3, 1), (13, 0)]);
    run(
        &w,
        &mut rng,
        Kind::Crop {
            max_age: 3,
            gate: true,
        },
        (0, -60, 0),
    );
    rng.done();
    assert_eq!(w.at((0, -60, 0)), "minecraft:beetroots[age=1]");
}

#[test]
fn a_torchflower_crop_at_its_last_age_becomes_the_flower() {
    let Some(w) = World::with(&[
        ((0, -61, 0), "minecraft:farmland[moisture=0]"),
        ((0, -60, 0), "minecraft:torchflower_crop[age=1]"),
    ]) else {
        return;
    };
    let mut rng = Script::ints(&[(3, 2), (13, 0)]);
    run(
        &w,
        &mut rng,
        Kind::Crop {
            max_age: 2,
            gate: true,
        },
        (0, -60, 0),
    );
    rng.done();
    assert_eq!(w.at((0, -60, 0)), "minecraft:torchflower");
}

#[test]
fn sugar_cane_ages_invisibly_and_sends_only_the_new_block() {
    let Some(w) = World::with(&[
        ((0, -61, 0), "minecraft:grass_block[snowy=false]"),
        ((0, -60, 0), "minecraft:sugar_cane[age=14]"),
    ]) else {
        return;
    };
    let mut rng = Script::default();
    let fx = run(&w, &mut rng, Kind::SugarCane, (0, -60, 0));
    assert_eq!(w.at((0, -60, 0)), "minecraft:sugar_cane[age=15]");
    assert!(fx.blocks.is_empty(), "flags 260: no client update");
    let fx = run(&w, &mut rng, Kind::SugarCane, (0, -60, 0));
    assert_eq!(w.at((0, -59, 0)), "minecraft:sugar_cane[age=0]");
    assert_eq!(w.at((0, -60, 0)), "minecraft:sugar_cane[age=0]");
    assert_eq!(
        fx.blocks.iter().map(|b| b.0).collect::<Vec<_>>(),
        vec![(0, -59, 0)]
    );
    rng.done();
}

#[test]
fn sugar_cane_stops_at_three() {
    let Some(w) = World::with(&[
        ((0, -60, 0), "minecraft:sugar_cane[age=0]"),
        ((0, -59, 0), "minecraft:sugar_cane[age=0]"),
        ((0, -58, 0), "minecraft:sugar_cane[age=15]"),
    ]) else {
        return;
    };
    let mut rng = Script::default();
    run(&w, &mut rng, Kind::SugarCane, (0, -58, 0));
    assert_eq!(w.at((0, -57, 0)), "minecraft:air");
    assert_eq!(w.at((0, -58, 0)), "minecraft:sugar_cane[age=15]");
}

#[test]
fn a_ripe_stem_fruits_only_on_the_side_it_drew() {
    let Some(w) = World::with(&[
        ((0, -61, 0), "minecraft:farmland[moisture=0]"),
        ((0, -60, 0), "minecraft:melon_stem[age=7]"),
        ((1, -61, 0), "minecraft:grass_block[snowy=false]"),
        ((0, -61, 1), "minecraft:grass_block[snowy=false]"),
    ]) else {
        return;
    };
    // NORTH (index 0): air over air — no support, a wasted draw.
    let mut rng = Script::ints(&[(13, 0), (4, 0)]);
    run(&w, &mut rng, Kind::Stem, (0, -60, 0));
    rng.done();
    assert_eq!(w.at((0, -60, 0)), "minecraft:melon_stem[age=7]");
    // EAST (index 1): air over grass — a melon, and the stem attaches.
    let mut rng = Script::ints(&[(13, 0), (4, 1)]);
    run(&w, &mut rng, Kind::Stem, (0, -60, 0));
    rng.done();
    assert_eq!(w.at((1, -60, 0)), "minecraft:melon");
    assert_eq!(
        w.at((0, -60, 0)),
        "minecraft:attached_melon_stem[facing=east]"
    );
}

#[test]
fn sweet_berries_draw_before_they_look_at_the_light() {
    let Some(w) = World::with(&[
        ((0, -61, 0), "minecraft:grass_block[snowy=false]"),
        ((0, -60, 0), "minecraft:sweet_berry_bush[age=1]"),
    ]) else {
        return;
    };
    let mut rng = Script::ints(&[(5, 0)]);
    run(&w, &mut rng, Kind::SweetBerry, (0, -60, 0));
    rng.done();
    assert_eq!(w.at((0, -60, 0)), "minecraft:sweet_berry_bush[age=2]");
}

#[test]
fn covered_grass_dies_and_takes_no_draw() {
    let Some(w) = World::with(&[
        ((0, -61, 0), "minecraft:grass_block[snowy=false]"),
        ((0, -60, 0), "minecraft:stone"),
    ]) else {
        return;
    };
    let mut rng = Script::default();
    run(&w, &mut rng, Kind::Spreading, (0, -61, 0));
    rng.done();
    assert_eq!(w.at((0, -61, 0)), "minecraft:dirt");
}

#[test]
fn grass_under_glass_lives_because_glass_dampens_nothing() {
    let Some(w) = World::with(&[
        ((0, -61, 0), "minecraft:grass_block[snowy=false]"),
        ((0, -60, 0), "minecraft:glass"),
    ]) else {
        return;
    };
    // Alive; under glass the sky still reaches above at 15, so four attempts
    // of three draws each, all onto air.
    let mut rng = Script::ints(&[(3, 0), (5, 4), (3, 0)].repeat(4));
    run(&w, &mut rng, Kind::Spreading, (0, -61, 0));
    rng.done();
    assert_eq!(w.at((0, -61, 0)), "minecraft:grass_block[snowy=false]");
}

#[test]
fn grass_spreads_onto_exposed_dirt_and_not_onto_covered_dirt() {
    let Some(w) = World::with(&[
        ((0, -61, 0), "minecraft:grass_block[snowy=false]"),
        ((1, -62, 0), "minecraft:dirt"),
        ((1, -61, 0), "minecraft:air"),
        ((0, -62, 0), "minecraft:dirt"),
    ]) else {
        return;
    };
    // Attempt 1 at (+1, -1, 0): exposed dirt, spreads. Attempt 2 at
    // (0, -1, 0): dirt under the grass itself, cannot stay alive there.
    let mut rng = Script::ints(&[
        (3, 2),
        (5, 2),
        (3, 1),
        (3, 1),
        (5, 2),
        (3, 1),
        (3, 1),
        (5, 4),
        (3, 1),
        (3, 1),
        (5, 4),
        (3, 1),
    ]);
    run(&w, &mut rng, Kind::Spreading, (0, -61, 0));
    rng.done();
    assert_eq!(w.at((1, -62, 0)), "minecraft:grass_block[snowy=false]");
    assert_eq!(w.at((0, -62, 0)), "minecraft:dirt");
}

#[test]
fn dry_farmland_with_nothing_on_it_turns_to_dirt_and_a_crop_holds_it() {
    let Some(w) = World::with(&[((0, -61, 0), "minecraft:farmland[moisture=0]")]) else {
        return;
    };
    let mut rng = Script::default();
    run(&w, &mut rng, Kind::Farmland, (0, -61, 0));
    assert_eq!(w.at((0, -61, 0)), "minecraft:dirt");
    let Some(w) = wheat_rig(0) else { return };
    run(&w, &mut rng, Kind::Farmland, (0, -61, 0));
    assert_eq!(w.at((0, -61, 0)), "minecraft:farmland[moisture=0]");
}

#[test]
fn farmland_near_water_is_wetted_to_seven() {
    let Some(w) = World::with(&[
        ((0, -61, 0), "minecraft:farmland[moisture=0]"),
        ((4, -61, 4), "minecraft:water[level=0]"),
    ]) else {
        return;
    };
    let mut rng = Script::default();
    run(&w, &mut rng, Kind::Farmland, (0, -61, 0));
    assert_eq!(w.at((0, -61, 0)), "minecraft:farmland[moisture=7]");
}

#[test]
fn a_decaying_leaf_goes_and_a_persistent_one_stays() {
    let Some(w) = World::with(&[
        (
            (0, -60, 0),
            "minecraft:oak_leaves[distance=7,persistent=false,waterlogged=false]",
        ),
        (
            (2, -60, 0),
            "minecraft:oak_leaves[distance=7,persistent=true,waterlogged=false]",
        ),
    ]) else {
        return;
    };
    let mut rng = Script::default();
    run(&w, &mut rng, Kind::Leaves, (0, -60, 0));
    run(&w, &mut rng, Kind::Leaves, (2, -60, 0));
    assert_eq!(w.at((0, -60, 0)), "minecraft:air");
    assert_ne!(w.at((2, -60, 0)), "minecraft:air");
}

#[test]
fn a_torch_beside_a_snow_layer_melts_it_and_one_further_off_does_not() {
    let Some(w) = World::with(&[
        ((0, -60, 0), "minecraft:snow[layers=1]"),
        ((1, -60, 0), "minecraft:torch"),
        ((10, -60, 0), "minecraft:snow[layers=1]"),
    ]) else {
        return;
    };
    let mut rng = Script::default();
    run(&w, &mut rng, Kind::SnowLayer, (0, -60, 0));
    run(&w, &mut rng, Kind::SnowLayer, (10, -60, 0));
    assert_eq!(w.at((0, -60, 0)), "minecraft:air", "13 > 11");
    assert_eq!(w.at((10, -60, 0)), "minecraft:snow[layers=1]", "5 is not");
}

#[test]
fn ice_melts_at_block_light_above_ten() {
    let Some(w) = World::with(&[
        ((0, -60, 0), "minecraft:ice"),
        ((2, -60, 0), "minecraft:torch"),
        ((6, -60, 0), "minecraft:ice"),
    ]) else {
        return;
    };
    let mut rng = Script::default();
    // (0): 14 - 1 (air) - 1 (ice's own opacity) = 12 > 11 - 1.
    run(&w, &mut rng, Kind::Ice, (0, -60, 0));
    // (6): 14 - 3 - 1 = 10, not > 10.
    run(&w, &mut rng, Kind::Ice, (6, -60, 0));
    assert_eq!(w.at((0, -60, 0)), "minecraft:water[level=0]");
    assert_eq!(w.at((6, -60, 0)), "minecraft:ice");
}

#[test]
fn a_sapling_cycles_its_stage_invisibly_on_one_in_seven() {
    let Some(w) = World::with(&[
        ((0, -61, 0), "minecraft:grass_block[snowy=false]"),
        ((0, -60, 0), "minecraft:oak_sapling[stage=0]"),
    ]) else {
        return;
    };
    let mut rng = Script::ints(&[(7, 0)]);
    let fx = run(&w, &mut rng, Kind::Sapling, (0, -60, 0));
    rng.done();
    assert_eq!(w.at((0, -60, 0)), "minecraft:oak_sapling[stage=1]");
    assert!(fx.blocks.is_empty());
}

#[test]
fn the_declarer_walk_stops_at_the_nearest_override() {
    let Some(classes) = crate::placedby::PlacedByTable::shared(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/vanilla/set_placed_by.json"),
    )
    .ok() else {
        return;
    };
    assert_eq!(Kind::of(classes, "minecraft:wheat"), Some(WHEAT));
    assert_eq!(
        Kind::of(classes, "minecraft:carrots"),
        Some(WHEAT),
        "CarrotBlock inherits"
    );
    assert_eq!(
        Kind::of(classes, "minecraft:grass_block"),
        Some(Kind::Spreading)
    );
    assert_eq!(
        Kind::of(classes, "minecraft:oak_leaves"),
        Some(Kind::Leaves)
    );
    assert_eq!(
        Kind::of(classes, "minecraft:copper_block"),
        None,
        "WeatheringCopperFullBlock is not ported"
    );
    assert_eq!(Kind::of(classes, "minecraft:stone"), Some(Kind::Nothing));
}

#[test]
fn covered_nylium_turns_to_netherrack_and_open_nylium_does_not_spread() {
    let Some(w) = World::with(&[
        ((0, -61, 0), "minecraft:crimson_nylium"),
        ((0, -60, 0), "minecraft:stone"),
        ((2, -61, 0), "minecraft:warped_nylium"),
        ((3, -62, 0), "minecraft:netherrack"),
    ]) else {
        return;
    };
    let mut rng = Script::default();
    run(&w, &mut rng, Kind::Nylium, (0, -61, 0));
    run(&w, &mut rng, Kind::Nylium, (2, -61, 0));
    rng.done();
    assert_eq!(w.at((0, -61, 0)), "minecraft:netherrack");
    assert_eq!(w.at((2, -61, 0)), "minecraft:warped_nylium");
    assert_eq!(w.at((3, -62, 0)), "minecraft:netherrack");
}

fn meal(w: &World, rng: &mut Script, pos: BlockPos) -> (Meal, Effects) {
    let light = Light::new(w, 320, true, 0);
    let mut fx = Effects::new();
    let state = w.at(pos);
    let mut cx = Ctx {
        world: w,
        shapes: None,
        light: &light,
        rng,
        fx: &mut fx,
        raining: false,
    };
    let m = bone_meal(&mut cx, pos, &state);
    (m, fx)
}

#[test]
fn bone_meal_walks_a_sweet_berry_bush_one_age_at_a_time_and_stops_at_three() {
    let Some(w) = World::with(&[((0, -60, 0), "minecraft:sweet_berry_bush[age=0]")]) else {
        return;
    };
    let mut rng = Script::default();
    for age in 1..=3 {
        assert_eq!(meal(&w, &mut rng, (0, -60, 0)).0, Meal::Grown);
        assert_eq!(
            w.at((0, -60, 0)),
            format!("minecraft:sweet_berry_bush[age={age}]")
        );
    }
    assert_eq!(meal(&w, &mut rng, (0, -60, 0)).0, Meal::Pass);
    rng.done();
}

#[test]
fn bone_meal_on_a_bamboo_sapling_needs_air_above() {
    let Some(w) = World::with(&[
        ((0, -60, 0), "minecraft:bamboo_sapling"),
        ((2, -60, 0), "minecraft:bamboo_sapling"),
        ((2, -59, 0), "minecraft:stone"),
    ]) else {
        return;
    };
    let mut rng = Script::default();
    assert_eq!(meal(&w, &mut rng, (0, -60, 0)).0, Meal::Grown);
    assert_eq!(
        w.at((0, -59, 0)),
        "minecraft:bamboo[age=0,leaves=small,stage=0]"
    );
    assert_eq!(meal(&w, &mut rng, (2, -60, 0)).0, Meal::Pass);
}

#[test]
fn bone_meal_grows_a_pitcher_crop_one_age_and_raises_the_upper_half_at_three() {
    let Some(w) = World::with(&[
        ((0, -61, 0), "minecraft:farmland[moisture=0]"),
        ((0, -60, 0), "minecraft:pitcher_crop[age=2,half=lower]"),
    ]) else {
        return;
    };
    let mut rng = Script::default();
    assert_eq!(meal(&w, &mut rng, (0, -60, 0)).0, Meal::Grown);
    assert_eq!(
        w.at((0, -60, 0)),
        "minecraft:pitcher_crop[age=3,half=lower]"
    );
    assert_eq!(
        w.at((0, -59, 0)),
        "minecraft:pitcher_crop[age=3,half=upper]"
    );
    // Clicking the upper half grows from the lower one.
    assert_eq!(meal(&w, &mut rng, (0, -59, 0)).0, Meal::Grown);
    assert_eq!(
        w.at((0, -60, 0)),
        "minecraft:pitcher_crop[age=4,half=lower]"
    );
    assert_eq!(
        meal(&w, &mut rng, (0, -60, 0)).0,
        Meal::Pass,
        "4 is the last age"
    );
}

#[test]
fn bone_meal_on_a_stem_draws_two_to_five_and_a_seven_runs_the_random_tick() {
    let Some(w) = World::with(&[
        ((0, -61, 0), "minecraft:farmland[moisture=0]"),
        ((0, -60, 0), "minecraft:pumpkin_stem[age=4]"),
        ((0, -61, 1), "minecraft:grass_block[snowy=false]"),
    ]) else {
        return;
    };
    // nextInt(4) = 1 -> +3 -> 7, then the random tick: growth draw, then
    // SOUTH (index 2) over grass.
    let mut rng = Script::ints(&[(4, 1), (13, 0), (4, 2)]);
    assert_eq!(meal(&w, &mut rng, (0, -60, 0)).0, Meal::Grown);
    rng.done();
    assert_eq!(w.at((0, -60, 1)), "minecraft:pumpkin");
    assert_eq!(
        w.at((0, -60, 0)),
        "minecraft:attached_pumpkin_stem[facing=south]"
    );
}

#[test]
fn a_twisting_vine_grows_its_head_up_into_air_and_stops_at_age_25() {
    let Some(w) = World::with(&[
        ((0, -61, 0), "minecraft:stone"),
        ((0, -60, 0), "minecraft:twisting_vines[age=3]"),
        ((2, -60, 0), "minecraft:twisting_vines[age=25]"),
    ]) else {
        return;
    };
    let mut rng = Script {
        doubles: [0.05].into_iter().collect(),
        ..Script::default()
    };
    run(&w, &mut rng, Kind::PlantHead, (0, -60, 0));
    run(&w, &mut rng, Kind::PlantHead, (2, -60, 0));
    rng.done();
    assert_eq!(w.at((0, -59, 0)), "minecraft:twisting_vines[age=4]");
    assert_eq!(w.at((2, -59, 0)), "minecraft:air");
}

#[test]
fn bone_meal_on_a_sapling_spends_a_float_and_advances_the_stage_invisibly() {
    let Some(w) = World::with(&[((0, -60, 0), "minecraft:oak_sapling[stage=0]")]) else {
        return;
    };
    // A float draw nobody scripted panics, so script it through `doubles`
    // being empty and `next_f32` being the only float this body takes.
    struct Floats(Vec<f32>);
    impl RandomSource for Floats {
        fn set_seed(&mut self, _: i64) {}
        fn next_i32(&mut self) -> i32 {
            panic!()
        }
        fn next_i32_bound(&mut self, _: i32) -> i32 {
            panic!("no int draw")
        }
        fn next_i64(&mut self) -> i64 {
            panic!()
        }
        fn next_bool(&mut self) -> bool {
            panic!()
        }
        fn next_f32(&mut self) -> f32 {
            self.0.remove(0)
        }
        fn next_f64(&mut self) -> f64 {
            panic!()
        }
        fn next_gaussian(&mut self) -> f64 {
            panic!()
        }
    }
    let light = Light::new(&w, 320, true, 0);
    let mut fx = Effects::new();
    let mut rng = Floats(vec![0.5, 0.2]);
    let mut cx = Ctx {
        world: &w,
        shapes: None,
        light: &light,
        rng: &mut rng,
        fx: &mut fx,
        raining: false,
    };
    // 0.5 fails the 0.45 roll: still spent, nothing written.
    assert_eq!(
        bone_meal(&mut cx, (0, -60, 0), "minecraft:oak_sapling[stage=0]"),
        Meal::Grown
    );
    assert_eq!(w.at((0, -60, 0)), "minecraft:oak_sapling[stage=0]");
    // 0.2 passes: stage 1, with flags 260, so nothing is sent.
    assert_eq!(
        bone_meal(&mut cx, (0, -60, 0), "minecraft:oak_sapling[stage=0]"),
        Meal::Grown
    );
    assert_eq!(w.at((0, -60, 0)), "minecraft:oak_sapling[stage=1]");
    assert!(fx.blocks.is_empty());
}
