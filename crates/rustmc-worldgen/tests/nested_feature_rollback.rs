//! A tree whose nested feature refuses must leave the chunk byte-identical.
//!
//! `docs/PARITY-ROADMAP.md` §85 asked for this and could not do it: `pale_moss`
//! is the first decorator in this port that places a configured feature, so it
//! is the only path where a refusal can have to roll back a *nested* feature's
//! writes, and nothing exercised that end to end. §89 asked for it again with
//! a reason attached — that branch found an un-delegated method in `UndoWorld`
//! by generating a 225-chunk world and reading a decline tally, which is an
//! expensive way to find a one-line bug.
//!
//! # What this checks that the unit test does not
//!
//! `a_rollback_undoes_a_nested_features_writes_too` builds an `UndoWorld` over
//! a `BTreeMap`, writes through two traits by hand, and rolls back. It is a
//! good test of the undo log and it cannot see the thing that actually breaks:
//! whether the **real** `Decoration` world, wrapped by the **real**
//! `tree::place`, running a **real** datapack tree config whose decorator
//! resolves a **real** configured feature, puts everything back.
//!
//! Every one of those four is where the wrapper can leak. A trait impl that
//! forwards to `inner` instead of `TreeWorld`, a defaulted method inherited
//! rather than delegated, a decorator that writes before the log exists — all
//! of them compile, place the right blocks, and pass the unit test.
//!
//! # Why the patch is made to refuse rather than found refusing
//!
//! `pale_moss_patch` places `pale_moss_carpet` through `simple_block`, which
//! asks `would_survive` first. This test's `SurvivalRules` answers every block
//! except that one and answers `None` for it, which is exactly the shape of an
//! unported rule: `simple_block` turns it into `Unplaceable`, the patch fails,
//! the decorator fails, and `tree::place` rolls back.
//!
//! Doing it that way rather than by deleting a rule means the test keeps
//! working after the rule is ported — which it now is. A test that could only
//! run while something was broken would have been deleted by the commit that
//! fixed it, and this is the one that has to outlive them.

use std::collections::BTreeMap;
use std::path::Path;

use rustmc_worldgen::canvas::LiveHeightmap;
use rustmc_worldgen::feature::block_predicate::PredicateWorld;
use rustmc_worldgen::pipeline::{Area, Pipeline};
use rustmc_worldgen::stage::decorate::SurvivalView;
use rustmc_worldgen::stage::decorate::{BiomeFacts, BlockFacts, Decoration, SurvivalRules};
use rustmc_worldgen::tags::BlockTags;

const EXTRACT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../oracle-work/vanilla-26.2/generated-data/data/minecraft/worldgen"
);

struct Facts;

impl BlockFacts for Facts {
    fn is_air(&self, state: &str) -> Option<bool> {
        Some(state == "minecraft:air" || state == "minecraft:cave_air")
    }
    fn fluid(&self, _state: &str) -> Option<String> {
        Some("minecraft:empty".to_string())
    }
    fn is_solid_render(&self, state: &str) -> Option<bool> {
        self.is_air(state).map(|air| !air)
    }
    fn is_solid(&self, state: &str) -> Option<bool> {
        self.is_air(state).map(|air| !air)
    }
    fn is_replaceable(&self, state: &str) -> Option<bool> {
        self.is_air(state)
    }
    fn can_support_center(&self, state: &str, direction: usize) -> Option<bool> {
        self.face_is_sturdy(state, direction)
    }
    fn face_is_sturdy(&self, state: &str, _: usize) -> Option<bool> {
        self.is_air(state).map(|air| !air)
    }
    fn face_is_full(&self, state: &str, _: usize) -> Option<bool> {
        self.is_air(state).map(|air| !air)
    }
    fn face_is_collision_full(&self, state: &str, _: usize) -> Option<bool> {
        self.is_air(state).map(|air| !air)
    }
    fn matches_heightmap(&self, state: &str, _: LiveHeightmap) -> bool {
        state != "minecraft:air"
    }
}

/// Everything survives except the one block, which answers `None`.
///
/// `None` is what an unported rule answers, and `simple_block` turns it into
/// `Unplaceable` rather than a quiet skip — which is the whole reason a
/// missing survival rule costs a tree rather than a carpet.
struct AllButOne(&'static str);

impl SurvivalRules for AllButOne {
    fn would_survive(&self, _: &SurvivalView<'_>, state: &str, _: (i32, i32, i32)) -> Option<bool> {
        let block = state.split_once('[').map_or(state, |(name, _)| name);
        if block == self.0 {
            None
        } else {
            Some(true)
        }
    }
}

struct PaleGarden;

impl BiomeFacts for PaleGarden {
    fn biome_at(&mut self, _: i32, _: i32, _: i32) -> String {
        "minecraft:pale_garden".to_string()
    }
    fn biome_lists(&mut self, _: &str, _: &str) -> bool {
        true
    }
    fn info_noise(&mut self, _: f64, _: f64) -> f64 {
        0.0
    }
    fn cold_enough_to_snow(&mut self, _: &str, _: i32, _: i32, _: i32) -> bool {
        false
    }
    fn precipitation_is_snow(&mut self, _: &str, _: i32, _: i32, _: i32) -> bool {
        false
    }
}

/// The real `pale_oak` config out of the datagen extract, with its `pale_moss`
/// decorator resolved through a real registry.
fn pale_oak_config() -> Option<rustmc_worldgen::feature::tree::TreeConfig> {
    let extract = rustmc_worldgen::feature::state_spec::extract_or_skip()?;
    let configured = Path::new(EXTRACT).join("configured_feature");
    let placed = Path::new(EXTRACT).join("placed_feature");
    if !configured.exists() {
        return None;
    }
    let read = |base: &Path, name: &str| -> Option<serde_json::Value> {
        let file = name.strip_prefix("minecraft:")?;
        serde_json::from_str(&std::fs::read_to_string(base.join(format!("{file}.json"))).ok()?).ok()
    };
    let resolve_placed = |name: &str| read(&placed, name);
    let resolve_configured = |name: &str| read(&configured, name);
    let resolve_feature = |name: &str| -> Result<_, String> {
        let json =
            resolve_configured(name).ok_or_else(|| format!("{name} is not in the extract"))?;
        rustmc_worldgen::feature::configured::ConfiguredFeature::parse(
            &json,
            &resolve_placed,
            &resolve_configured,
            &extract,
        )
    };
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(configured.join("pale_oak.json")).ok()?)
            .ok()?;
    rustmc_worldgen::feature::tree::TreeConfig::parse(&json["config"], &extract, &resolve_feature)
        .ok()
}

/// Every non-air block in the decorated area, as a position map.
///
/// Read through `PredicateWorld::block_at` — the same path a feature reads —
/// rather than off the canvas directly, so the snapshot cannot disagree with
/// what the code under test can see.
fn snapshot(world: &mut dyn PredicateWorld) -> BTreeMap<(i32, i32, i32), String> {
    let mut out = BTreeMap::new();
    for x in -16..32 {
        for z in -16..32 {
            for y in 60..100 {
                let state = world.block_at(x, y, z);
                if state != "minecraft:air" {
                    out.insert((x, y, z), state);
                }
            }
        }
    }
    out
}

/// Positions where two snapshots disagree, as readable lines, capped.
fn difference(
    before: &BTreeMap<(i32, i32, i32), String>,
    after: &BTreeMap<(i32, i32, i32), String>,
) -> Vec<String> {
    let mut keys: Vec<&(i32, i32, i32)> = before.keys().chain(after.keys()).collect();
    keys.sort_unstable();
    keys.dedup();
    let mut out: Vec<String> = keys
        .into_iter()
        .filter(|pos| before.get(*pos) != after.get(*pos))
        .map(|pos| {
            let (x, y, z) = *pos;
            format!(
                "({x},{y},{z}) was {:?} now {:?}",
                before.get(pos).map(String::as_str).unwrap_or("air"),
                after.get(pos).map(String::as_str).unwrap_or("air")
            )
        })
        .collect();
    out.truncate(12);
    out
}

/// How many non-air blocks the ground laid, so an unchanged empty world cannot
/// pass for an unchanged full one.
fn ground_count(pipeline: &mut Pipeline) -> usize {
    let canvas = pipeline.canvas_mut();
    let mut n = 0usize;
    for x in -16..32 {
        for z in -16..32 {
            for y in 60..100 {
                if canvas.block_at(x, y, z) != "minecraft:air" {
                    n += 1;
                }
            }
        }
    }
    n
}

/// Grass at y=63 across the area, so a tree has somewhere to stand.
fn ground(pipeline: &mut Pipeline) {
    let canvas = pipeline.canvas_mut();
    for x in -16..32 {
        for z in -16..32 {
            canvas.set_block(x, 62, z, "minecraft:dirt");
            canvas.set_block(x, 63, z, "minecraft:grass_block[snowy=false]");
        }
    }
}

/// A pale oak whose `pale_moss` patch cannot place leaves the chunk exactly as
/// it found it.
///
/// The assertion is the **whole** block map and not a sample of positions.
/// Every failure this test exists to catch is a small number of blocks left
/// behind in a world that otherwise looks right, so a spot check is the one
/// shape of assertion that cannot see it.
#[test]
fn a_tree_whose_nested_feature_refuses_leaves_the_chunk_untouched() {
    let Some(config) = pale_oak_config() else {
        return;
    };
    assert!(
        config.is_supported(),
        "the pale oak must be supported, or this test proves nothing about rollback"
    );

    let mut pipeline = Pipeline::new(Area::square(0, 0, 1), -64, 384, "minecraft:air");
    ground(&mut pipeline);
    let before_ground = ground_count(&mut pipeline);
    assert!(
        before_ground > 1000,
        "the ground was not laid, so an unchanged world would prove nothing"
    );

    let facts = Facts;
    // `pale_moss_carpet` is the block the patch places, and refusing it is
    // what an unported survival rule looks like from inside `simple_block`.
    let survival = AllButOne("minecraft:pale_moss_carpet");
    let mut biomes = PaleGarden;
    // Real tags: a tree asks `#replaceable_by_trees` before it writes a log,
    // and an empty tag set makes it panic rather than refuse — which would
    // pass this test for the wrong reason.
    let tags_dir = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../oracle-work/vanilla-26.2/generated-data/data/minecraft/tags/block"
    ));
    if !rustmc_data::fixture::available(
        tags_dir,
        "block tags in the datagen extract",
        "ci/vanilla-server.sh data",
    ) {
        return;
    }
    let tags = BlockTags::load(tags_dir).expect("block tags");

    let mut placed_ok = 0usize;
    let mut refused = 0usize;
    {
        let mut world = Decoration::new(
            pipeline.canvas_mut(),
            &facts,
            &survival,
            &mut biomes,
            &tags,
            63,
            384,
            20260722,
        )
        .with_level_random(20260722, false);
        world.begin_chunk(0, 0);
        let mut baseline = snapshot(&mut world);

        // Several seeds into the SAME world, because the two outcomes are both
        // legitimate and the test needs both. A patch that *declines* is not an
        // error — §85.5, vanilla pops the patch's boolean — so on some seeds the
        // tree places and the world moves. Interleaving them is the point: a
        // leak that only shows up after a successful tree has already written
        // is exactly what a fresh world per seed would hide.
        for seed in 0..24i64 {
            let mut random = rustmc_worldgen::rng::WorldgenRandom::new(
                rustmc_worldgen::rng::LegacyRandomSource::new(seed),
            );
            let outcome = rustmc_worldgen::feature::tree::place(
                &mut world,
                &tags,
                &config,
                &mut random,
                (8, 64, 8),
            );
            let after = snapshot(&mut world);
            match outcome {
                Err(_) => {
                    refused += 1;
                    assert_eq!(
                        difference(&baseline, &after),
                        Vec::<String>::new(),
                        "seed {seed} refused and left blocks behind: the undo log \
                         did not cover them"
                    );
                }
                Ok(placed) => {
                    if placed {
                        placed_ok += 1;
                        assert_ne!(
                            baseline, after,
                            "seed {seed} reported a placed tree and wrote nothing"
                        );
                    }
                    baseline = after;
                }
            }
        }
    }

    assert!(
        refused > 0,
        "no attempt refused, so the rollback path never ran: {placed_ok} placed of 24"
    );
    assert!(
        placed_ok > 0,
        "no attempt succeeded either, so an always-refusing stub would pass this test"
    );
}
