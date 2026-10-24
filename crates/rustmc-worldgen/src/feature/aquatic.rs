//! Seagrass and kelp — the two features that grow out of the ocean floor.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `SeagrassFeature`, `KelpFeature`, `ProbabilityFeatureConfiguration`.
//!
//! Both read `Heightmap.Types.OCEAN_FLOOR` — **not** `OCEAN_FLOOR_WG`. The
//! `_WG` one is a fossil primed when noise filled the chunk and never touched
//! again; `OCEAN_FLOOR` is re-primed immediately before features run and
//! rises as features write. Reading the fossil puts kelp at the pre-carve
//! sea bed.
//!
//! # Seagrass scatters before it looks
//!
//! ```text
//! dx = nextInt(8) - nextInt(8)
//! dz = nextInt(8) - nextInt(8)
//! ```
//!
//! Four draws, always, before any block is read — so a seagrass attempt that
//! finds dry land still costs four. Two `nextInt(8)` subtracted is not
//! `nextInt(15) - 7`: it is triangular, biased towards zero, and it is two
//! draws.
//!
//! # Kelp's stalk height is drawn even when nothing grows
//!
//! `1 + nextInt(10)` comes before the loop, and the loop can place nothing.
//! The one draw inside it — `nextInt(4) + 20`, the age of the tip — happens
//! at most once, because placing the tip ends the walk.
//!
//! # Kelp has two ways to finish and they place different blocks
//!
//! Walking up, each level wants water here, water above, and a surviving
//! plant. When that holds at the *last* level the tip goes in with a random
//! age; below that, plant body. When it fails partway up, the walk backs off
//! one block and tries to put a **tip** there instead — unless the block two
//! below is already kelp, which is the check that stops two stalks fusing.
//!
//! # Why the r7 diff blames these two, and why the blame is misplaced
//!
//! Measured at seed 20260722 over the interior of the r7 window (chunks
//! 56..63, y 40..69), our aquatic features made **8,911 writes, of which
//! 8,238 (92.45%) are the block a real vanilla server has**, and **72 of 91
//! invocations are write-for-write perfect**. Whole columns of ocean chunks —
//! (63,61), (63,62), (63,63) and the rest of x=63 — are exact with 65 to 189
//! writes each. Against `DecorateGolden` the ports are exact on every centre
//! tried: (58,58) seagrass 75/75, (57,58) seagrass 82/82 and kelp 125/125,
//! (61,59) seagrass 81/81, (61,60) seagrass 84/84 and kelp 189/189, (63,63)
//! seagrass 80/80 and kelp 156/156 — positions, blocks and the RNG state
//! after.
//!
//! What the diff is really counting is **amplification**. `is_water` gates
//! the `nextDouble` that decides tall-vs-short, so one attempt that finds a
//! plant where vanilla found water costs a draw, and every attempt after it
//! scatters somewhere else. Eighty attempts hang off that. So these features
//! turn *any* upstream difference in the water column into a few hundred
//! blocks, and they are the loudest thing in the diff without being wrong.
//!
//! Two upstream sources were separated by tracing every write
//! (`RUSTMC_TRACE_FEATURE=seagrass,kelp`) and asking, per invocation, which
//! of *our earlier* aquatic writes inside its 15x15 scatter zone the real
//! world disagrees with:
//!
//! - Most failing chunks are **downstream**: (61,60) has 45 such blocks, all
//!   from (61,59); (58,58) has 27, all from (57,59). Their own arithmetic is
//!   fine; they read a zone someone else already got wrong.
//! - The roots are chunks where **we reproduce the harness exactly and the
//!   harness itself is not the server**. (61,59) is the clean specimen: zero
//!   prior aquatic writes in its scatter zone, our 81 writes identical to
//!   `DecorateGolden`'s 81 — and only 49 of those 81, and 77 of its 122 kelp
//!   blocks, exist in the server's world. (58,58) is the same shape: 38 of
//!   the harness's 75.
//!
//! So do not "fix" this file toward the r7 diff. There is no ordering or
//! draw-order fault here; the box-iteration class of bug cannot apply,
//! because neither feature walks a box. The open question is upstream of
//! both us and the harness, and it is upstream of *this file*.

use super::block_predicate::PredicateWorld;

/// `ProbabilityFeatureConfiguration`, which is what seagrass takes.
#[derive(Clone, Debug, PartialEq)]
pub struct ProbabilityConfig {
    pub probability: f32,
}

impl ProbabilityConfig {
    /// # Errors
    /// If `probability` is missing.
    pub fn parse(config: &serde_json::Value) -> Result<Self, String> {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the codec's field is a float and vanilla widens it to double \
                      only for the comparison, which this port does too"
        )]
        let probability = config
            .get("probability")
            .and_then(serde_json::Value::as_f64)
            .ok_or_else(|| "seagrass config has no `probability`".to_string())?
            as f32;
        Ok(Self { probability })
    }
}

/// The world these features read and write.
pub trait AquaticWorld: PredicateWorld {
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str);

    /// `getHeight(Heightmap.Types.OCEAN_FLOOR, x, z)`.
    fn ocean_floor_height(&mut self, x: i32, z: i32) -> i32;
}

/// The feature could not be evaluated.
#[derive(Debug, PartialEq, Eq)]
pub struct NeedsRules(pub String);

/// `SeagrassFeature#place`.
///
/// # Errors
/// If the world cannot say whether seagrass survives somewhere.
pub fn place_seagrass<W: AquaticWorld + ?Sized>(
    world: &mut W,
    config: &ProbabilityConfig,
    random: &mut dyn crate::rng::RandomSource,
    origin: (i32, i32, i32),
) -> Result<bool, NeedsRules> {
    // Four draws, unconditionally. See the module docs.
    let dx = random.next_i32_bound(8) - random.next_i32_bound(8);
    let dz = random.next_i32_bound(8) - random.next_i32_bound(8);
    let x = origin.0 + dx;
    let z = origin.2 + dz;
    let y = world.ocean_floor_height(x, z);

    if !is_water(world, x, y, z) {
        return Ok(false);
    }
    // `nextDouble() < (double) config.probability` — vanilla widens the float
    // rather than narrowing the double, and the two disagree on a band of
    // doubles either side of the float's value.
    let tall = random.next_f64() < f64::from(config.probability);
    let state = if tall {
        "minecraft:tall_seagrass[half=lower]"
    } else {
        "minecraft:seagrass"
    };
    let survives = world
        .would_survive(state, x, y, z)
        .ok_or_else(|| NeedsRules(format!("canSurvive({state})")))?;
    if !survives {
        return Ok(false);
    }

    if tall {
        // Both halves or neither: the upper one needs water to sit in.
        if is_water(world, x, y + 1, z) {
            world.set_block(x, y, z, state);
            world.set_block(x, y + 1, z, "minecraft:tall_seagrass[half=upper]");
        }
    } else {
        world.set_block(x, y, z, state);
    }
    // Vanilla sets its flag before the water check, so a tall seagrass that
    // found no room still reports success.
    Ok(true)
}

/// `KelpFeature#place`.
///
/// # Errors
/// If the world cannot say whether kelp survives somewhere.
pub fn place_kelp<W: AquaticWorld + ?Sized>(
    world: &mut W,
    random: &mut dyn crate::rng::RandomSource,
    origin: (i32, i32, i32),
) -> Result<bool, NeedsRules> {
    let x = origin.0;
    let z = origin.2;
    let mut y = world.ocean_floor_height(x, z);
    if !is_water(world, x, y, z) {
        // Not one draw. The height is rolled *after* this test.
        return Ok(false);
    }

    let mut placed = 0u32;
    let height = 1 + random.next_i32_bound(10);
    let mut i = 0;
    while i <= height {
        let grows = is_water(world, x, y, z)
            && is_water(world, x, y + 1, z)
            && world
                .would_survive("minecraft:kelp_plant", x, y, z)
                .ok_or_else(|| NeedsRules("canSurvive(kelp_plant)".to_string()))?;
        if grows {
            if i == height {
                let age = random.next_i32_bound(4) + 20;
                world.set_block(x, y, z, &format!("minecraft:kelp[age={age}]"));
                placed += 1;
            } else {
                world.set_block(x, y, z, "minecraft:kelp_plant");
            }
        } else if i > 0 {
            // Back off one and try to cap the stalk there instead.
            let below = y - 1;
            let survives = world
                .would_survive("minecraft:kelp", x, below, z)
                .ok_or_else(|| NeedsRules("canSurvive(kelp)".to_string()))?;
            if survives && !is_kelp(world, x, below - 1, z) {
                let age = random.next_i32_bound(4) + 20;
                world.set_block(x, below, z, &format!("minecraft:kelp[age={age}]"));
                placed += 1;
            }
            return Ok(placed > 0);
        }
        y += 1;
        i += 1;
    }
    Ok(placed > 0)
}

/// `state.is(Blocks.WATER)` — the *block*, so flowing water counts and a
/// waterlogged fence does not.
fn is_water<W: AquaticWorld + ?Sized>(world: &mut W, x: i32, y: i32, z: i32) -> bool {
    let state = world.block_at(x, y, z);
    plain(&state) == "minecraft:water"
}

fn is_kelp<W: AquaticWorld + ?Sized>(world: &mut W, x: i32, y: i32, z: i32) -> bool {
    let state = world.block_at(x, y, z);
    plain(&state) == "minecraft:kelp"
}

fn plain(state: &str) -> &str {
    state.split_once('[').map_or(state, |(n, _)| n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::legacy::LegacyRandomSource;
    use crate::rng::worldgen_random::WorldgenRandom;
    use crate::rng::RandomSource;
    use std::collections::BTreeMap;

    struct Ocean {
        blocks: BTreeMap<(i32, i32, i32), String>,
        floor: i32,
    }

    impl PredicateWorld for Ocean {
        fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
            self.blocks
                .get(&(x, y, z))
                .cloned()
                .unwrap_or_else(|| "minecraft:air".to_string())
        }
        fn would_survive(&mut self, _state: &str, x: i32, y: i32, z: i32) -> Option<bool> {
            Some(self.block_at(x, y - 1, z) != "minecraft:air")
        }
    }

    impl AquaticWorld for Ocean {
        fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
            self.blocks.insert((x, y, z), state.to_string());
        }
        fn ocean_floor_height(&mut self, _x: i32, _z: i32) -> i32 {
            self.floor
        }
    }

    /// Sand at 61, water from 62 up.
    fn ocean() -> Ocean {
        let mut blocks = BTreeMap::new();
        for x in -8..9 {
            for z in -8..9 {
                blocks.insert((x, 61, z), "minecraft:sand".to_string());
                for y in 62..90 {
                    blocks.insert((x, y, z), "minecraft:water[level=0]".to_string());
                }
            }
        }
        Ocean { blocks, floor: 62 }
    }

    /// Four draws before a single block is read.
    #[test]
    fn seagrass_scatters_before_it_looks() {
        let mut world = Ocean {
            blocks: BTreeMap::new(),
            floor: 62,
        };
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(4));
        let config = ProbabilityConfig { probability: 0.0 };
        assert!(!place_seagrass(&mut world, &config, &mut random, (0, 0, 0)).expect("ok"));

        let mut probe = WorldgenRandom::new(LegacyRandomSource::new(4));
        for _ in 0..4 {
            probe.next_i32_bound(8);
        }
        assert_eq!(
            random.next_i64(),
            probe.next_i64(),
            "a seagrass that found dry land still paid its four draws"
        );
    }

    #[test]
    fn a_probability_of_one_plants_both_halves() {
        let mut world = ocean();
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(11));
        let config = ProbabilityConfig { probability: 1.0 };
        assert!(place_seagrass(&mut world, &config, &mut random, (0, 0, 0)).expect("ok"));
        let lower = world
            .blocks
            .values()
            .filter(|v| v.as_str() == "minecraft:tall_seagrass[half=lower]")
            .count();
        let upper = world
            .blocks
            .values()
            .filter(|v| v.as_str() == "minecraft:tall_seagrass[half=upper]")
            .count();
        assert_eq!((lower, upper), (1, 1));
    }

    #[test]
    fn a_probability_of_zero_plants_one_block() {
        let mut world = ocean();
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(11));
        let config = ProbabilityConfig { probability: 0.0 };
        assert!(place_seagrass(&mut world, &config, &mut random, (0, 0, 0)).expect("ok"));
        assert_eq!(
            world
                .blocks
                .values()
                .filter(|v| v.as_str() == "minecraft:seagrass")
                .count(),
            1
        );
    }

    /// A stalk of plant with one tip, and the tip's age is `20..24`.
    #[test]
    fn kelp_grows_a_stalk_capped_by_a_tip() {
        let mut world = ocean();
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(2));
        assert!(place_kelp(&mut world, &mut random, (0, 0, 0)).expect("ok"));
        let tips: Vec<&String> = world
            .blocks
            .values()
            .filter(|v| v.starts_with("minecraft:kelp["))
            .collect();
        assert_eq!(tips.len(), 1, "exactly one tip");
        let age: i32 = tips[0]
            .trim_start_matches("minecraft:kelp[age=")
            .trim_end_matches(']')
            .parse()
            .expect("age");
        assert!((20..24).contains(&age), "age {age}");
        assert!(
            world
                .blocks
                .values()
                .any(|v| v.as_str() == "minecraft:kelp_plant"),
            "and body below it"
        );
    }

    /// Dry land: no height drawn at all.
    #[test]
    fn kelp_on_dry_land_draws_nothing() {
        let mut world = Ocean {
            blocks: BTreeMap::new(),
            floor: 62,
        };
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(6));
        let mut probe = WorldgenRandom::new(LegacyRandomSource::new(6));
        assert!(!place_kelp(&mut world, &mut random, (0, 0, 0)).expect("ok"));
        assert_eq!(random.next_i64(), probe.next_i64());
    }

    #[test]
    fn every_seagrass_config_in_the_datapack_parses() {
        let dir = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../oracle-work/vanilla-26.2/generated-data/data/minecraft/worldgen/configured_feature"
        ));
        if !rustmc_data::fixture::available(dir, "datagen extract", "ci/vanilla-server.sh data") {
            return;
        }
        let mut seen = 0usize;
        for entry in std::fs::read_dir(dir).expect("readable") {
            let path = entry.expect("entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let json: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).expect("readable"))
                    .expect("json");
            if json["type"] != "minecraft:seagrass" {
                continue;
            }
            ProbabilityConfig::parse(&json["config"])
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            seen += 1;
        }
        assert!(seen >= 3, "only {seen} seagrass features parsed");
    }
}

#[cfg(test)]
mod diagnosis {
    //! A one-off probe kept as a test: does the kelp feature reach its first
    //! water check at all, given an ocean floor of sand under water?
    use super::*;
    use crate::rng::legacy::LegacyRandomSource;
    use crate::rng::worldgen_random::WorldgenRandom;
    use std::collections::BTreeMap;

    struct Ocean {
        blocks: BTreeMap<(i32, i32, i32), String>,
        floor: i32,
    }

    impl crate::feature::block_predicate::PredicateWorld for Ocean {
        fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
            self.blocks.get(&(x, y, z)).cloned().unwrap_or_else(|| {
                if y <= 44 {
                    "minecraft:sand".to_string()
                } else if y <= 62 {
                    "minecraft:water[level=0]".to_string()
                } else {
                    "minecraft:air".to_string()
                }
            })
        }
        fn air_at(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
            Some(self.block_at(x, y, z) == "minecraft:air")
        }
        fn would_survive(&mut self, _: &str, _: i32, _: i32, _: i32) -> Option<bool> {
            Some(true)
        }
    }

    impl AquaticWorld for Ocean {
        fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
            self.blocks.insert((x, y, z), state.to_string());
        }
        fn ocean_floor_height(&mut self, _: i32, _: i32) -> i32 {
            self.floor
        }
    }

    /// With the floor reported one above the sand — which is what
    /// `getFirstAvailable` means — kelp grows.
    #[test]
    fn kelp_grows_from_the_first_water_above_the_floor() {
        let mut world = Ocean {
            blocks: BTreeMap::new(),
            floor: 45,
        };
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(4));
        assert!(place_kelp(&mut world, &mut random, (0, 45, 0)).expect("ok"));
        assert!(
            world
                .blocks
                .values()
                .any(|b| b.starts_with("minecraft:kelp")),
            "no kelp: {:?}",
            world.blocks
        );
    }

    /// ...and reported *at* the sand, it grows nothing. This is the shape of
    /// the whole-world disagreement, so the heightmap's meaning is the thing
    /// to check.
    #[test]
    fn kelp_grows_nothing_if_the_floor_is_the_solid_block() {
        let mut world = Ocean {
            blocks: BTreeMap::new(),
            floor: 44,
        };
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(4));
        assert!(!place_kelp(&mut world, &mut random, (0, 44, 0)).expect("ok"));
        assert!(world.blocks.is_empty());
    }
}
