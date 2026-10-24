//! The disk feature — the sand, gravel and clay patches in a river bed.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `net.minecraft.world.level.levelgen.feature.DiskFeature`,
//! `configurations.DiskConfiguration`.
//!
//! Four of the thirty-four features in the `UNDERGROUND_ORES` step are disks,
//! which is not where anyone would look for them.
//!
//! # It draws exactly once
//!
//! The radius, and nothing else. Every column inside that radius is then
//! filled deterministically: the target predicate decides whether a block is
//! replaced, and the state provider decides what with — and neither touches
//! the stream for any disk in the game. So a disk feature costs one draw
//! whether it fills nine hundred blocks or none.
//!
//! # The column walk goes downward, and the direction is load-bearing
//!
//! `placeColumn` runs from `y + halfHeight` down to `y - halfHeight - 1`
//! exclusive, and carries a flag for "the block above this one was also
//! placed". The flag exists to call `markAboveForPostProcessing` exactly once
//! per contiguous run, at the top of it. Walking upward instead would mark
//! the bottom of each run, which is a different set of positions.
//!
//! That marking changes no block, so this port records it rather than
//! performing it — but it is *observable*: it is why `DiskFeature` reaches
//! `WorldGenLevel.getChunk(BlockPos)`, and it is scheduled post-processing
//! that a full server will have to honour.
//!
//! # The bottom bound is exclusive and asymmetric
//!
//! `top = y + halfHeight`, `bottom = y - halfHeight - 1`, and the loop is
//! `for (int i = top; i > bottom; i--)`. So the column is
//! `2 * halfHeight + 1` blocks tall, centred on `y`. The `- 1` and the `>`
//! cancel; either one alone would be off by one.

use crate::rng::RandomSource;

use super::block_predicate::{BlockPredicate, NeedsBehaviour, PredicateWorld};
use super::int_provider::IntProvider;
use super::state_provider::StateProvider;
use crate::tags::BlockTags;

/// `DiskConfiguration`.
#[derive(Clone, Debug, PartialEq)]
pub struct DiskConfig {
    pub state_provider: StateProvider,
    /// What may be replaced. Tested per block, and draws nothing.
    pub target: BlockPredicate,
    pub radius: IntProvider,
    pub half_height: i32,
}

impl DiskConfig {
    /// # Errors
    /// If a field is missing or names something unimplemented.
    pub fn parse(
        config: &serde_json::Value,
        states: &dyn super::state_spec::DefaultStates,
    ) -> Result<Self, String> {
        Ok(Self {
            state_provider: StateProvider::parse(
                config
                    .get("state_provider")
                    .ok_or_else(|| "disk config has no `state_provider`".to_string())?,
                states,
            )?,
            target: BlockPredicate::parse(
                config
                    .get("target")
                    .ok_or_else(|| "disk config has no `target`".to_string())?,
            )?,
            radius: IntProvider::parse(
                config
                    .get("radius")
                    .ok_or_else(|| "disk config has no `radius`".to_string())?,
            )?,
            half_height: config
                .get("half_height")
                .and_then(serde_json::Value::as_i64)
                .and_then(|v| i32::try_from(v).ok())
                .ok_or_else(|| "disk config has no integer `half_height`".to_string())?,
        })
    }

    #[must_use]
    pub fn is_supported(&self) -> bool {
        self.state_provider.is_supported() && self.target.is_supported()
    }
}

/// The world a disk reads and writes.
pub trait DiskWorld: PredicateWorld {
    fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str);
    /// `Feature#markAboveForPostProcessing` — records a position whose
    /// column above wants post-processing. Changes no block.
    ///
    /// **Required, deliberately.** It used to carry a no-op default body, and
    /// the production world never overrode it, so every disk and fallen-tree
    /// mark was dropped for as long as the port existed. A default that does
    /// nothing reads as caution and behaves as a claim; an implementor that
    /// genuinely has nowhere to put a mark has to say so in its own body.
    fn mark_above_for_post_processing(&mut self, x: i32, y: i32, z: i32);
}

/// `DiskFeature#place`. Returns whether any block was placed.
///
/// # Errors
/// If the target predicate or the state provider needs block behaviour the
/// port does not have.
pub fn place<W: DiskWorld + ?Sized, R: RandomSource>(
    world: &mut W,
    tags: &BlockTags,
    config: &DiskConfig,
    random: &mut R,
    origin: (i32, i32, i32),
) -> Result<bool, NeedsBehaviour> {
    let (ox, oy, oz) = origin;
    let top = oy + config.half_height;
    let bottom = oy - config.half_height - 1;

    // The only draw in the whole feature.
    let radius = config.radius.sample(random);

    let mut placed = false;
    // `BlockPos.betweenClosed(origin.offset(-r, 0, -r), origin.offset(r, 0, r))`
    // walks a flat index and decomposes it `i = n % width` first, so **x is
    // the fastest axis** — z outer, x inner. With a single y here that is z
    // then x.
    //
    // This file used to say the opposite, and `underwater_magma` used to *do*
    // the opposite, which cost 402 blocks before it was found. Nothing in a
    // disk column draws today — every disk's state provider resolves to
    // `simple_state_provider`, checked against the datapack — so the order is
    // currently unobservable here. It is still written the right way round,
    // because "unobservable" is a property of this version's data and not of
    // the code.
    for z in oz - radius..=oz + radius {
        for x in ox - radius..=ox + radius {
            let dx = x - ox;
            let dz = z - oz;
            if dx * dx + dz * dz > radius * radius {
                continue;
            }
            placed |= place_column(world, tags, config, random, top, bottom, x, z)?;
        }
    }
    Ok(placed)
}

/// `DiskFeature#placeColumn`.
#[allow(
    clippy::too_many_arguments,
    reason = "\
    the same six vanilla passes, plus the tag set the predicate resolves \
    against. Bundling them would hide that `top` and `bottom` are computed by \
    the caller and are asymmetric, which is the part worth seeing."
)]
fn place_column<W: DiskWorld + ?Sized, R: RandomSource>(
    world: &mut W,
    tags: &BlockTags,
    config: &DiskConfig,
    random: &mut R,
    top: i32,
    bottom: i32,
    x: i32,
    z: i32,
) -> Result<bool, NeedsBehaviour> {
    let mut placed = false;
    // "the block one above this was placed by this same column", which is
    // what makes the post-processing mark land once per contiguous run.
    let mut above_was_placed = false;
    let mut y = top;
    while y > bottom {
        if config.target.test(world, tags, (x, y, z))? {
            let state = config
                .state_provider
                .state(world, tags, random, (x, y, z))?;
            world.set_block(x, y, z, &state);
            if !above_was_placed {
                world.mark_above_for_post_processing(x, y, z);
            }
            placed = true;
            above_was_placed = true;
        } else {
            above_was_placed = false;
        }
        y -= 1;
    }
    Ok(placed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::legacy::LegacyRandomSource;
    use crate::rng::worldgen_random::WorldgenRandom;
    use std::collections::{BTreeMap, BTreeSet};

    struct Ground {
        blocks: BTreeMap<(i32, i32, i32), String>,
        marks: BTreeSet<(i32, i32, i32)>,
    }

    impl PredicateWorld for Ground {
        fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
            self.blocks
                .get(&(x, y, z))
                .cloned()
                .unwrap_or_else(|| "minecraft:air".to_string())
        }
    }

    impl DiskWorld for Ground {
        fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str) {
            self.blocks.insert((x, y, z), name.to_string());
        }
        fn mark_above_for_post_processing(&mut self, x: i32, y: i32, z: i32) {
            self.marks.insert((x, y, z));
        }
    }

    /// A slab of dirt from y=-4 to y=4 over a 9x9 footprint.
    fn dirt() -> Ground {
        let mut blocks = BTreeMap::new();
        for x in -8..=8 {
            for z in -8..=8 {
                for y in -4..=4 {
                    blocks.insert((x, y, z), "minecraft:dirt".to_string());
                }
            }
        }
        Ground {
            blocks,
            marks: BTreeSet::new(),
        }
    }

    fn tags() -> BlockTags {
        BlockTags::from_entries(std::collections::BTreeMap::new()).expect("tags")
    }

    /// Real default-state spellings, copied out of the block-props extract.
    fn states() -> super::super::state_spec::DefaultStateTable {
        super::super::state_spec::DefaultStateTable::from_pairs(&[(
            "minecraft:sand",
            "minecraft:sand",
        )])
    }

    fn config(half_height: i32, radius: &str) -> DiskConfig {
        DiskConfig::parse(
            &serde_json::from_str(&format!(
                r#"{{"half_height":{half_height},
                     "radius":{radius},
                     "state_provider":{{"type":"minecraft:simple_state_provider",
                                        "state":{{"Name":"minecraft:sand"}}}},
                     "target":{{"type":"minecraft:matching_blocks",
                                "blocks":"minecraft:dirt"}}}}"#
            ))
            .expect("json"),
            &states(),
        )
        .expect("parse")
    }

    /// One draw, whatever the disk does afterwards.
    #[test]
    fn the_radius_is_the_only_draw() {
        let config = config(2, "3");
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(7));
        let mut world = dirt();
        place(&mut world, &tags(), &config, &mut random, (0, 0, 0)).expect("ok");

        let mut reference = WorldgenRandom::new(LegacyRandomSource::new(7));
        reference.next_i32_bound(1); // stand-in for the constant radius: none
        let mut counted = WorldgenRandom::new(LegacyRandomSource::new(7));
        let mut empty = Ground {
            blocks: BTreeMap::new(),
            marks: BTreeSet::new(),
        };
        place(&mut empty, &tags(), &config, &mut counted, (0, 0, 0)).expect("ok");
        assert_eq!(
            counted.next_i64(),
            random.next_i64(),
            "a disk that filled nothing and one that filled a slab must leave the \
             stream in the same place — only the radius draws"
        );
    }

    /// `2 * halfHeight + 1` tall, centred on the origin. The `- 1` on the
    /// bottom bound and the `>` in the loop cancel; either alone is an
    /// off-by-one.
    #[test]
    fn the_column_is_symmetric_about_the_origin() {
        let config = config(2, "0");
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(1));
        let mut world = dirt();
        place(&mut world, &tags(), &config, &mut random, (0, 0, 0)).expect("ok");
        let sand: Vec<i32> = (-6..=6)
            .filter(|y| {
                world
                    .blocks
                    .get(&(0, *y, 0))
                    .is_some_and(|b| b == "minecraft:sand")
            })
            .collect();
        assert_eq!(sand, vec![-2, -1, 0, 1, 2], "five blocks, centred on y=0");
    }

    /// The disk is a circle by `dx^2 + dz^2 <= r^2`, tested with `>` and
    /// `continue` — so the corners of the square are excluded and the edge
    /// is inclusive.
    #[test]
    fn the_footprint_is_a_circle_with_an_inclusive_edge() {
        let config = config(0, "2");
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(1));
        let mut world = dirt();
        place(&mut world, &tags(), &config, &mut random, (0, 0, 0)).expect("ok");
        assert_eq!(
            world.blocks.get(&(2, 0, 0)).map(String::as_str),
            Some("minecraft:sand"),
            "dx=2, dz=0 is exactly r^2 and must be included"
        );
        assert_eq!(
            world.blocks.get(&(2, 0, 2)).map(String::as_str),
            Some("minecraft:dirt"),
            "the corner is 8 > 4 and must be excluded"
        );
    }

    /// The mark lands once per contiguous run, at its top — which is what the
    /// downward walk plus the carried flag buys.
    #[test]
    fn the_post_processing_mark_lands_at_the_top_of_each_run() {
        let mut world = dirt();
        // Punch a hole so the column has two runs of dirt.
        world.blocks.remove(&(0, 1, 0));
        let config = config(4, "0");
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(1));
        place(&mut world, &tags(), &config, &mut random, (0, 0, 0)).expect("ok");
        let marks: Vec<i32> = world.marks.iter().map(|(_, y, _)| *y).collect();
        assert_eq!(
            marks,
            vec![0, 4],
            "one mark at the top of each run: y=4 down to y=2, then y=0 down to y=-4"
        );
    }

    /// All five parse, and all five are now *askable* — `solid` is a real
    /// predicate rather than a hole. Whether a given world can answer it is a
    /// separate question, raised at evaluation time and tested below.
    #[test]
    fn every_disk_config_parses_and_is_askable() {
        let dir = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../oracle-work/vanilla-26.2/generated-data/data/minecraft/worldgen/configured_feature"
        ));
        if !rustmc_data::fixture::available(dir, "datagen extract", "ci/vanilla-server.sh data") {
            return;
        }
        let Some(extract) = super::super::state_spec::extract_or_skip() else {
            return;
        };
        let mut parsed = Vec::new();
        for entry in std::fs::read_dir(dir).expect("readable") {
            let path = entry.expect("entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let json: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).expect("readable"))
                    .expect("json");
            if json["type"] != "minecraft:disk" {
                continue;
            }
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .expect("utf-8")
                .to_string();
            let config = DiskConfig::parse(&json["config"], &extract)
                .unwrap_or_else(|e| panic!("{stem}: {e}"));
            assert!(config.is_supported(), "{stem}: the port cannot ask this");
            parsed.push(stem);
        }
        parsed.sort();
        assert_eq!(
            parsed,
            [
                "disk_clay",
                "disk_grass",
                "disk_gravel",
                "disk_sand",
                "ice_patch"
            ],
            "the datapack has five disk features, and `ice_patch` is one of them"
        );
    }

    /// A world with no block-property table refuses `solid` rather than
    /// answering "no".
    ///
    /// The distinction is the whole point: `false` would place `disk_grass`'s
    /// grass into open air across the map and look like a generator bug. An
    /// error names the missing table.
    #[test]
    fn solid_is_refused_by_a_world_that_cannot_answer_it() {
        let predicate = BlockPredicate::parse(
            &serde_json::from_str(r#"{"type":"minecraft:solid","offset":[0,1,0]}"#).expect("json"),
        )
        .expect("parses");
        assert!(predicate.is_supported(), "the port knows how to ask");

        let mut world = Ground {
            blocks: BTreeMap::new(),
            marks: BTreeSet::new(),
        };
        let error = predicate
            .test(&mut world, &tags(), (0, 0, 0))
            .expect_err("a world without the table must refuse");
        assert_eq!(error.0, "minecraft:solid");
    }
}
