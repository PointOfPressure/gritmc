//! Vanilla's own `ServerExplosion.calculateExplodedPositions()`, against ours.
//!
//! `data/vanilla/explosion_ray.json` is recorded by
//! `ci/explosion-ray-golden.sh`, which allocates a `ServerExplosion` and a
//! `ServerLevel` subclass without their constructors and reflectively invokes
//! the private method. So this is not two readings of one bytecode agreeing:
//! it is the jar's own answer.
//!
//! Three things it pins that no transcription check reaches:
//!
//! * the **`java.util.HashSet` iteration order** the position list inherits
//!   from `new ObjectArrayList<>(set)` at offset 416. Our
//!   [`rustmc_data::java_hash_set::JavaHashSet`] has to agree with it
//!   position-for-position, not just as a set;
//! * that `Util.shuffle` continues the **same stream** the 1,352 ray draws
//!   came from. A port that re-seeded would match `positions` and fail
//!   `shuffled`;
//! * the float rounding of `(res + 0.3f) * 0.3f` against `0.22500001f` over
//!   thirty-odd steps.
//!
//! The two lists are compared separately on purpose. Comparing only the
//! shuffled one would be cheaper and would hide which of the two broke.
//!
//! # WHAT THIS TEST CATCHES, AND WHAT IT DOES NOT — measured, 2026-09-06
//!
//! Nine mutations of `crate::explosion` were applied one at a time to the
//! shipping code and this test re-run against the unchanged golden. **Five
//! were caught and four were not**, and the four are all of one kind:
//!
//! ```text
//! CAUGHT  the j/k/l loop order transposed to l/k/j
//! CAUGHT  the position list sorted instead of kept in JavaHashSet order
//! CAUGHT  `f -= (res + 0.3) * 0.3` reduced to `f -= res * 0.3`
//! CAUGHT  the second `f > 0.0` test dropped before `set.add`
//! CAUGHT  the strength `0.7 + nextFloat() * 0.6` changed to `0.75 + .. * 0.5`
//! MISSED  STEP        0.30000001192092896 -> 0.3
//! MISSED  STEP_DECAY  0.22500001f         -> 0.225f
//! MISSED  the ray normalisation done in f32 instead of f64
//! MISSED  the ray direction computed in f64 instead of f32-then-widened
//! ```
//!
//! The four missed mutations move a ray's endpoint by about `1e-7` blocks over
//! its ~23 steps, and this instrument's granularity is `Mth.floor` — one whole
//! block. So **this test does not pin the exact float widths, and no
//! block-granular instrument can**: a coordinate would have to land within
//! `1e-7` of an integer for the difference to change a floor, and over 5,395
//! recorded positions none does.
//!
//! That is a statement about the instrument, not a defence of the constants.
//! The constants stay exactly as the bytecode has them — `ldc2_w #234` and
//! `ldc #236` — because the cost of keeping them right is zero and the cost of
//! discovering later that they mattered is a rewrite. What is refused is the
//! claim that this golden *proves* them.

use rustmc_server::explosion::{
    calculate_exploded_positions, create_fire, shuffle, BlockExplosion, ExplosionWorld, Probe,
};
use rustmc_worldgen::rng::legacy::LegacyRandomSource;
use rustmc_worldgen::rng::RandomSource;
use std::path::{Path, PathBuf};

/// The whole path in one literal, so `ci/gen-gameplay-tables.sh`'s grep sees
/// it and `ci/check-generator-arms.sh` finds the arm reached.
const RAY_TABLE: &str = "data/vanilla/explosion_ray.json";
/// The resistance table this test drives the probe from, named here for the
/// same reason.
const BLOCK_TABLE: &str = "data/vanilla/block_explosion.json";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn load(rel: &str, produced_by: &str) -> Option<serde_json::Value> {
    let p = repo_root().join(rel);
    if !rustmc_data::fixture::available(Path::new(&p), rel, produced_by) {
        return None;
    }
    Some(serde_json::from_str(&std::fs::read_to_string(&p).expect("read")).expect("json"))
}

/// The three answers the ray walk asks of a position, for one of the dumper's
/// worlds. Kept as a `match` on the world's name so that this file and
/// `ExplosionRayGolden.RayLevel` can be read side by side.
fn probe_for(world: &str, table: &BlockExplosion, pos: (i32, i32, i32)) -> Probe {
    if world == "out_of_bounds" {
        return Probe::default();
    }
    // A build-height limit, which BREAKS the ray where a hard block only
    // subtracts from it.
    if world == "ceiling" && pos.1 >= 68 {
        return Probe::default();
    }
    let (state, is_air, fluid): (&str, bool, Option<&str>) = match world {
        "open_air" => ("minecraft:air", true, None),
        "stone" => ("minecraft:stone", false, None),
        "obsidian" => ("minecraft:obsidian", false, None),
        "floor" => {
            if pos.1 >= 64 {
                ("minecraft:air", true, None)
            } else {
                ("minecraft:stone", false, None)
            }
        }
        "water" => ("minecraft:air", true, Some("minecraft:water")),
        // Obsidian on one side only, so an axis transposition shows.
        "wall_east" => {
            if pos.0 >= 2 {
                ("minecraft:obsidian", false, None)
            } else {
                ("minecraft:air", true, None)
            }
        }
        "ceiling" => ("minecraft:air", true, None),
        "air_over_lava" => {
            if pos.1 < 64 {
                ("minecraft:air", true, Some("minecraft:lava"))
            } else {
                ("minecraft:air", true, None)
            }
        }
        other => panic!("unknown world {other} — the dumper grew a case this test does not model"),
    };
    Probe {
        in_bounds: true,
        resistance: rustmc_server::explosion::block_explosion_resistance(
            table, state, is_air, fluid,
        ),
        // The default `ExplosionDamageCalculator.shouldBlockExplode` is
        // `iconst_1`, and the dumper sets that calculator.
        should_explode: true,
    }
}

fn triples(flat: &serde_json::Value) -> Vec<(i32, i32, i32)> {
    let a: Vec<i32> = flat
        .as_array()
        .expect("array")
        .iter()
        .map(|v| i32::try_from(v.as_i64().expect("int")).expect("fits"))
        .collect();
    a.chunks_exact(3).map(|c| (c[0], c[1], c[2])).collect()
}

/// The dumper's `RayLevel`, on the fire pass's three questions. `isAir` and
/// `isSolidRender` are the two the golden's stub answers; `set_fire` records,
/// as `RayLevel.setBlockAndUpdate` does.
struct FireWorld {
    world: &'static str,
    placed: Vec<(i32, i32, i32)>,
}

impl ExplosionWorld for FireWorld {
    fn is_air(&self, pos: (i32, i32, i32)) -> bool {
        match self.world {
            "floor" => pos.1 >= 64,
            "stone" | "obsidian" => false,
            "wall_east" => pos.0 < 2,
            // `open_air`, `out_of_bounds`, `water`, `air_over_lava` and
            // `ceiling` all answer `Blocks.AIR.defaultBlockState()`.
            _ => true,
        }
    }
    fn drops_from_explosion(&self, _pos: (i32, i32, i32)) -> bool {
        unreachable!("the fire pass does not ask")
    }
    fn drops(
        &mut self,
        _pos: (i32, i32, i32),
        _explosion_radius: Option<f32>,
    ) -> Vec<rustmc_server::item::ItemStack> {
        unreachable!("the fire pass does not ask")
    }
    fn set_air(&mut self, _pos: (i32, i32, i32)) {
        unreachable!("the fire pass does not ask")
    }
    fn solid_render_below(&self, pos: (i32, i32, i32)) -> bool {
        // `BlockState.isSolidRender()`: true for stone and obsidian, false for
        // air. `wall_east` is solid where it is not air, so its air-over-solid
        // set is empty by construction and no fire can land — which is the
        // right reason for it to place none, and not the same as pretending
        // the world is all air.
        match self.world {
            "floor" => pos.1 - 1 < 64,
            "stone" | "obsidian" => true,
            "wall_east" => pos.0 >= 2,
            _ => false,
        }
    }
    fn set_fire(&mut self, pos: (i32, i32, i32)) {
        self.placed.push(pos);
    }
}

#[test]
fn our_ray_walk_is_vanillas_position_for_position() {
    let Some(doc) = load(RAY_TABLE, "ci/explosion-ray-golden.sh") else {
        return;
    };
    let Some(blocks) = load(BLOCK_TABLE, "ci/block-explosion-golden.sh") else {
        return;
    };
    let table = BlockExplosion::from_json(&blocks).expect("block table");

    assert_eq!(
        doc["rays"].as_u64(),
        Some(1352),
        "the golden's own ray count moved"
    );

    let cases = doc["cases"].as_array().expect("cases");
    assert!(!cases.is_empty(), "no cases — the golden is empty");
    let mut compared = 0usize;
    let mut positions_seen = 0usize;
    let mut fires_seen = 0usize;
    let mut fire_orders_that_differ = 0usize;

    for c in cases {
        let name = c["name"].as_str().expect("name");
        let world = c["world"].as_str().expect("world");
        let seed = c["seed"].as_i64().expect("seed");
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let radius = f32::from_bits(c["radiusBits"].as_i64().expect("radiusBits") as u32);
        let cb = c["centerBits"].as_array().expect("centerBits");
        #[allow(clippy::cast_sign_loss)]
        let center = (
            f64::from_bits(cb[0].as_i64().expect("cx") as u64),
            f64::from_bits(cb[1].as_i64().expect("cy") as u64),
            f64::from_bits(cb[2].as_i64().expect("cz") as u64),
        );

        let want = triples(&c["positions"]);
        let want_shuffled = triples(&c["shuffled"]);

        let mut rng = LegacyRandomSource::new(seed);
        let got = calculate_exploded_positions(center, radius, &mut rng, |pos| {
            probe_for(world, &table, pos)
        })
        .unwrap_or_else(|e| panic!("{name}: the position set refused: {e:?}"));

        assert_eq!(
            got,
            want,
            "{name}: {} positions against vanilla's {}, in HashSet order",
            got.len(),
            want.len()
        );

        // The same RandomSource, not a fresh one: `interactWithBlocks` shuffles
        // with the stream the ray walk has just spent 1,352 floats from.
        let mut got_shuffled = got;
        shuffle(&mut got_shuffled, &mut rng);
        assert_eq!(
            got_shuffled, want_shuffled,
            "{name}: the shuffle diverged — the stream did not continue"
        );

        // The fire pass, over both orders `explode()` can hand it, each from
        // the stream state the 1,352 ray draws leave behind. The golden
        // records them from vanilla's own `createFire`, invoked reflectively.
        for (label, order, want_fire) in [
            ("fireNoShuffle", want.clone(), triples(&c["fireNoShuffle"])),
            (
                "fireAfterShuffle",
                want_shuffled.clone(),
                triples(&c["fireAfterShuffle"]),
            ),
        ] {
            let mut fr = LegacyRandomSource::new(seed);
            for _ in 0..1352 {
                fr.next_f32();
            }
            if label == "fireAfterShuffle" {
                // `interactWithBlocks` spends the shuffle's draws on the same
                // stream before `createFire` runs.
                let mut scratch = want.clone();
                shuffle(&mut scratch, &mut fr);
            }
            let mut fw = FireWorld {
                world: match world {
                    "floor" => "floor",
                    "stone" => "stone",
                    "obsidian" => "obsidian",
                    "wall_east" => "wall_east",
                    _ => "air",
                },
                placed: Vec::new(),
            };
            let got_fire = create_fire(&order, &mut fr, &mut fw);
            assert_eq!(got_fire, fw.placed, "{name}/{label}: two records disagree");
            assert_eq!(
                got_fire, want_fire,
                "{name}/{label}: the fire pass diverged"
            );
            fires_seen += got_fire.len();
        }
        if triples(&c["fireNoShuffle"]) != triples(&c["fireAfterShuffle"]) {
            fire_orders_that_differ += 1;
        }

        compared += 1;
        positions_seen += want.len();
    }

    // The instrument's own floor. Every assertion above passes vacuously for a
    // golden of twelve empty cases, so the totals are asserted too, against the
    // figures `ci/explosion-ray-golden.sh` printed when the table was recorded.
    assert_eq!(compared, 14, "the golden lost a case");
    // Fire lands in exactly the three `floor` cases, and it lands twice per
    // case because both orders are compared. A zero here would make every
    // fire assertion above vacuous.
    assert_eq!(
        fires_seen, 235,
        "the total fire count moved — recorded 2026-09-06 over both orders of \
         the three `floor` cases"
    );
    // The property the whole fire arm exists for: the same seed, the same
    // world and the same stream state give DIFFERENT fires depending on
    // whether `interactWithBlocks` shuffled the one list first. Without this
    // the two arms above could both be empty and still agree.
    assert_eq!(
        fire_orders_that_differ, 3,
        "the fire pass stopped inheriting the shuffle"
    );
    assert_eq!(
        positions_seen, 6988,
        "the golden's total position count moved"
    );
}
