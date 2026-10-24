//! Running every carver that can reach a chunk.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `NoiseBasedChunkGenerator#applyCarvers`.
//!
//! Generating one chunk's caves means replaying the carvers of **289 source
//! chunks** — every chunk within `±8` — and keeping whatever lands inside.
//! Each source chunk contributes the carvers its own biome lists, seeded from
//! its own coordinates, so the result is the same no matter which chunk is
//! generated first. That is the whole reason carvers are shaped this way:
//! it buys chunk-order independence at the cost of doing the work 289 times.
//!
//! # The sweep order is not observable, but the seeding is
//!
//! Each carver is reseeded with `setLargeFeatureSeed(seed + index, sourceX,
//! sourceZ)` before it runs, so no carver can see the stream another one
//! left behind — which means the `dx`/`dz` loop order does not change the
//! output.
//!
//! What *is* observable is `index`: it counts a carver's position **within
//! its source biome's list**, and it is added to the world seed. So a biome
//! that lists `[cave, cave_extra_underground, canyon]` seeds its canyon with
//! `seed + 2`, and reordering the list — or resolving the biome's carver list
//! through anything with a nondeterministic order — moves every ravine in
//! that biome.
//!
//! Vanilla seeds the shared `WorldgenRandom` from `generateUniqueSeed()`, a
//! genuinely random value. It is never read: every use is preceded by a
//! reseed. Reproduced as a fixed zero rather than a random one, so nothing
//! *can* depend on it.

// Read once, not per chunk of the carver stage. See `crate::trace_switch`.
crate::trace_switch_value!(trace_carver, "RUSTMC_TRACE_CARVER");
use super::carve::{CarveBounds, CarveTarget};
use super::config::ConfiguredCarver;
use super::mask::CarvingMask;
use super::{canyon, cave};
use crate::rng::{LegacyRandomSource, RandomSource, WorldgenRandom};
use crate::tags::BlockTags;

/// How far, in chunks, the sweep reaches. `NoiseBasedChunkGenerator` writes
/// this as a literal 8 rather than deriving it from `WorldCarver#getRange`,
/// which is 4 — they are not the same number and not the same thing.
pub const SWEEP_RADIUS: i32 = 8;

/// What a source chunk contributes.
///
/// A closure rather than a biome source because resolving a chunk's carvers
/// needs the biome registry and the biome's generation settings, neither of
/// which belongs to this module.
pub type CarversAt<'a> = dyn Fn(i32, i32) -> Vec<ConfiguredCarver> + 'a;

/// `NoiseBasedChunkGenerator#applyCarvers`.
pub fn apply_carvers<T: CarveTarget>(
    target: &mut T,
    tags: &BlockTags,
    carvers_at: &CarversAt<'_>,
    seed: i64,
    bounds: CarveBounds,
    mask: &mut CarvingMask,
) {
    // Vanilla's initial seed is random and never read — every use is
    // preceded by `setLargeFeatureSeed`. Fixed here so nothing can come to
    // depend on it.
    let mut random = WorldgenRandom::new(LegacyRandomSource::new(0));

    // `RUSTMC_TRACE_CARVER=<chunk_x>,<chunk_z>` reports what the sweep did
    // for one chunk: every source that offered a carver, whether it passed
    // `isStartChunk`, and how many blocks it then wrote.
    //
    // A whole-world diff can say a chunk has no cave in it. It cannot say
    // whether the sweep never offered one, offered one and lost the
    // probability roll, or ran a carve that wrote nothing — and those are
    // three different bugs. Measured on chunk (62,62) of seed 20260722,
    // where vanilla has 119 `cave_air` and this port has none while all 24
    // of its neighbours agree.
    let trace = trace_carver().and_then(|v| {
        let (x, z) = v.split_once(',')?;
        let (x, z) = (x.trim().parse::<i32>().ok()?, z.trim().parse::<i32>().ok()?);
        (x == bounds.chunk_x && z == bounds.chunk_z).then_some(())
    });
    let mut offered = 0u32;
    let mut started = 0u32;

    for dx in -SWEEP_RADIUS..=SWEEP_RADIUS {
        for dz in -SWEEP_RADIUS..=SWEEP_RADIUS {
            let source_x = bounds.chunk_x + dx;
            let source_z = bounds.chunk_z + dz;

            for (index, carver) in carvers_at(source_x, source_z).iter().enumerate() {
                let index = i64::try_from(index).expect("carver index fits in i64");
                random.set_large_feature_seed(seed.wrapping_add(index), source_x, source_z);
                offered += 1;

                // `isStartChunk` — one `nextFloat()` against the
                // probability, drawn *whether or not* the carver then runs.
                if random.next_f32() > carver.base().probability {
                    continue;
                }
                started += 1;
                if trace.is_some() {
                    eprintln!(
                        "CARVE ({},{}) <- source ({source_x},{source_z}) carver #{index} {}",
                        bounds.chunk_x,
                        bounds.chunk_z,
                        match carver {
                            ConfiguredCarver::Cave(_) => "cave",
                            ConfiguredCarver::NetherCave(_) => "nether_cave",
                            ConfiguredCarver::Canyon(_) => "canyon",
                        }
                    );
                }

                match carver {
                    ConfiguredCarver::Cave(config) => cave::carve(
                        target,
                        tags,
                        config,
                        cave::CaveKind::Overworld,
                        bounds,
                        mask,
                        &mut random,
                        source_x,
                        source_z,
                    ),
                    ConfiguredCarver::NetherCave(config) => cave::carve(
                        target,
                        tags,
                        config,
                        cave::CaveKind::Nether,
                        bounds,
                        mask,
                        &mut random,
                        source_x,
                        source_z,
                    ),
                    ConfiguredCarver::Canyon(config) => canyon::carve(
                        target,
                        tags,
                        config,
                        bounds,
                        mask,
                        &mut random,
                        source_x,
                        source_z,
                    ),
                }
            }
        }
    }

    if trace.is_some() {
        eprintln!(
            "CARVE ({},{}) summary: {offered} offered, {started} passed isStartChunk",
            bounds.chunk_x, bounds.chunk_z
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::carver::config::{CarverConfig, CaveConfig};
    use crate::carver::providers::{FloatProvider, HeightProvider};
    use crate::surface::rule::VerticalAnchor;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    struct Chunk {
        blocks: BTreeMap<(i32, i32, i32), String>,
    }

    impl CarveTarget for Chunk {
        fn block(&self, x: i32, y: i32, z: i32) -> String {
            self.blocks
                .get(&(x, y, z))
                .cloned()
                .unwrap_or_else(|| "minecraft:stone".to_string())
        }
        fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str) {
            self.blocks.insert((x, y, z), name.to_string());
        }
        fn aquifer_substance(&mut self, _: i32, _: i32, _: i32) -> Option<String> {
            Some("minecraft:cave_air".to_string())
        }
        fn top_material(&mut self, _: i32, _: i32, _: i32, _: bool) -> Option<String> {
            None
        }
        // The geometry tests carve dry air, so no position is ever marked.
        fn should_schedule_fluid_update(&self) -> bool {
            false
        }
        fn mark_post_processing(&mut self, _: i32, _: i32, _: i32) {}
    }

    fn tags() -> BlockTags {
        BlockTags::from_entries(
            [(
                "minecraft:overworld_carver_replaceables".to_string(),
                vec!["minecraft:stone".to_string()],
            )]
            .into_iter()
            .collect(),
        )
        .expect("tags")
    }

    fn cave_config(probability: f32) -> ConfiguredCarver {
        ConfiguredCarver::Cave(CaveConfig {
            base: CarverConfig {
                probability,
                y: HeightProvider::Uniform {
                    min_inclusive: VerticalAnchor::AboveBottom(8),
                    max_inclusive: VerticalAnchor::Absolute(180),
                },
                y_scale: FloatProvider::Uniform {
                    min: 0.1,
                    max_exclusive: 0.9,
                },
                lava_level: VerticalAnchor::AboveBottom(8),
                replaceable: "#minecraft:overworld_carver_replaceables".to_string(),
            },
            horizontal_radius_multiplier: FloatProvider::Uniform {
                min: 0.7,
                max_exclusive: 1.4,
            },
            vertical_radius_multiplier: FloatProvider::Uniform {
                min: 0.8,
                max_exclusive: 1.3,
            },
            floor_level: FloatProvider::Uniform {
                min: -1.0,
                max_exclusive: -0.4,
            },
        })
    }

    fn bounds(chunk_x: i32, chunk_z: i32) -> CarveBounds {
        CarveBounds {
            min_gen_y: -64,
            gen_depth: 384,
            chunk_x,
            chunk_z,
        }
    }

    fn run(seed: i64, chunk_x: i32, chunk_z: i32, probability: f32) -> Chunk {
        let mut chunk = Chunk {
            blocks: BTreeMap::new(),
        };
        let mut mask = CarvingMask::new(384, -64);
        apply_carvers(
            &mut chunk,
            &tags(),
            &|_, _| vec![cave_config(probability)],
            seed,
            bounds(chunk_x, chunk_z),
            &mut mask,
        );
        chunk
    }

    #[test]
    fn the_sweep_visits_every_chunk_within_eight() {
        // 17 x 17 source chunks. Fewer means caves stop at an invisible
        // boundary; more is wasted work that still produces nothing, since
        // no carver from further out can reach.
        let visited = RefCell::new(Vec::new());
        let mut chunk = Chunk {
            blocks: BTreeMap::new(),
        };
        let mut mask = CarvingMask::new(384, -64);
        apply_carvers(
            &mut chunk,
            &tags(),
            &|x, z| {
                visited.borrow_mut().push((x, z));
                Vec::new()
            },
            1,
            bounds(0, 0),
            &mut mask,
        );
        let visited = visited.into_inner();
        assert_eq!(visited.len(), 17 * 17);
        assert!(visited.contains(&(-8, -8)));
        assert!(visited.contains(&(8, 8)));
        assert!(!visited.contains(&(9, 0)));
    }

    #[test]
    fn a_zero_probability_carver_never_runs_but_still_draws() {
        // `isStartChunk` draws whether or not it succeeds. If the draw were
        // skipped on failure the carvers after it in the same biome list
        // would be unaffected — they are reseeded — but the *shape* of this
        // one's stream is what a golden vector will pin, so the draw is
        // load-bearing.
        let carved = run(1, 0, 0, 0.0);
        assert!(carved.blocks.is_empty(), "a zero-probability carver carved");
    }

    #[test]
    fn carving_is_deterministic_and_depends_on_the_seed() {
        let a = run(20_260_725, 0, 0, 1.0);
        let b = run(20_260_725, 0, 0, 1.0);
        let c = run(20_260_726, 0, 0, 1.0);
        assert_eq!(a.blocks, b.blocks, "same seed gave a different world");
        assert_ne!(a.blocks, c.blocks, "the seed did not reach the carvers");
        assert!(!a.blocks.is_empty(), "nothing was carved at all");
    }

    #[test]
    fn a_chunk_is_carved_the_same_however_its_neighbours_are_generated() {
        // The property the whole 289-chunk sweep exists to buy: the result
        // for a chunk does not depend on generation order. Here that shows
        // up as chunk (0,0) being identical whether or not its neighbours
        // were generated first, which holds because nothing is shared
        // between runs.
        let first = run(7, 0, 0, 1.0);
        let _neighbour = run(7, 1, 0, 1.0);
        let again = run(7, 0, 0, 1.0);
        assert_eq!(first.blocks, again.blocks);
    }

    #[test]
    fn the_carver_index_is_added_to_the_seed() {
        // A biome listing the same carver twice must not carve the same
        // caves twice — the index shifts the second one's seed. If the index
        // were dropped, the two would be identical and the second would
        // carve nothing new.
        let mut one = Chunk {
            blocks: BTreeMap::new(),
        };
        let mut mask_one = CarvingMask::new(384, -64);
        apply_carvers(
            &mut one,
            &tags(),
            &|_, _| vec![cave_config(1.0)],
            3,
            bounds(0, 0),
            &mut mask_one,
        );

        let mut two = Chunk {
            blocks: BTreeMap::new(),
        };
        let mut mask_two = CarvingMask::new(384, -64);
        apply_carvers(
            &mut two,
            &tags(),
            &|_, _| vec![cave_config(1.0), cave_config(1.0)],
            3,
            bounds(0, 0),
            &mut mask_two,
        );

        assert!(
            two.blocks.len() > one.blocks.len(),
            "the second copy of the carver added nothing: {} vs {}",
            two.blocks.len(),
            one.blocks.len()
        );
    }
}
