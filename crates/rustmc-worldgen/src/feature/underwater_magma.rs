//! The underwater magma feature — the magma blocks in an ocean floor.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `net.minecraft.world.level.levelgen.feature.UnderwaterMagmaFeature`,
//! `configurations.UnderwaterMagmaConfiguration`, `levelgen.Column`.
//!
//! One feature, and the last of the thirty-four in the `UNDERGROUND_ORES`
//! step. It is here because it is the only one that needs a block's *shape*.
//!
//! # It draws for every position in the box, including the ones it rejects
//!
//! The probability filter comes **first** in the stream, before the validity
//! test:
//!
//! ```text
//! betweenClosedStream(box)
//!     .filter(p -> random.nextFloat() < placementProbabilityPerValidPosition)
//!     .filter(p -> isValidPlacement(level, p))
//!     .mapToInt(p -> level.setBlock(p, MAGMA, 2) ? 1 : 0)
//!     .sum() > 0
//! ```
//!
//! So a `(2r+1)^3` box costs `(2r+1)^3` draws whatever the terrain looks
//! like — 27 at the vanilla radius of 1. Reordering the two filters would
//! give the same *blocks* on any single call and a different stream for
//! everything after it, which is the kind of change that looks like a
//! harmless optimisation and moves every feature in the chunk.
//!
//! The field name says the same thing and is worth reading twice:
//! `placementProbabilityPerValidPosition`. It is not per valid position. It
//! is per position, and the name is wrong.
//!
//! # "Visible from outside" is the negation of a full face
//!
//! `isVisibleFromOutside(level, pos, dir)` is
//!
//! ```text
//! shape = state.getFaceOcclusionShape(dir)
//! return shape == Shapes.empty() || !Block.isShapeFullBlock(shape)
//! ```
//!
//! and `isShapeFullBlock(empty)` is already false, so the whole thing is
//! `!isShapeFullBlock(getFaceOcclusionShape(dir))` — exactly the bit
//! [`crate::feature::placement::PlacementWorld::face_is_full`] carries. That
//! is the one question in worldgen that cannot be answered from a block name,
//! a tag, or a datapack, and it is why `data/vanilla/block_props.json`
//! exists.
//!
//! # The floor scan looks for water, and `isWaterOrAir` is a different test
//!
//! `Column.scan` walks up and down from the origin while a predicate holds
//! and then tests a *tip* predicate at the end. The two predicates
//! `getFloorY` hands it are
//!
//! ```text
//! lambda$getFloorY$0:  state -> state.is(Blocks.WATER)
//! lambda$getFloorY$1:  state -> !state.is(Blocks.WATER)
//! ```
//!
//! **Water, not water-or-air.** `isWaterOrAir` exists in the same class and is
//! used only by `isValidPlacement`, twenty lines away — and reaching for it
//! here is the mistake this port made. The difference is not cosmetic: an air
//! block deep underground fails `is(WATER)` immediately, so `getFloorY`
//! returns empty and the feature costs **no draws at all**. Treating it as a
//! valid column start finds a cave floor, places magma in the middle of a
//! cavern, and spends 27 draws that vanilla never spends — which moves every
//! feature after it.
//!
//! It was caught by `golden_decorate`: vanilla wrote 0 magma blocks in three
//! chunks and drew exactly `1 + 3n` for its `n` candidates, i.e. its chain
//! draws and nothing more. The port wrote 7 and drew far more.
//!
//! Both scans share one cursor and the downward one resets it — a detail that
//! matters only because forgetting it would start the second scan wherever
//! the first stopped.

use crate::rng::RandomSource;

use super::block_predicate::PredicateWorld;

/// `UnderwaterMagmaConfiguration`.
#[derive(Clone, Debug, PartialEq)]
pub struct UnderwaterMagmaConfig {
    pub floor_search_range: i32,
    pub placement_radius_around_floor: i32,
    /// Despite the name, this is rolled for **every** position in the box,
    /// not for every valid one. See the module docs.
    pub placement_probability_per_valid_position: f32,
}

impl UnderwaterMagmaConfig {
    /// # Errors
    /// If a field is missing.
    pub fn parse(config: &serde_json::Value) -> Result<Self, String> {
        let int = |name: &str| -> Result<i32, String> {
            config
                .get(name)
                .and_then(serde_json::Value::as_i64)
                .and_then(|v| i32::try_from(v).ok())
                .ok_or_else(|| format!("underwater_magma config has no integer `{name}`"))
        };
        Ok(Self {
            floor_search_range: int("floor_search_range")?,
            placement_radius_around_floor: int("placement_radius_around_floor")?,
            #[allow(clippy::cast_possible_truncation)]
            placement_probability_per_valid_position: config
                .get("placement_probability_per_valid_position")
                .and_then(serde_json::Value::as_f64)
                .map(|v| v as f32)
                .ok_or_else(|| {
                    "underwater_magma config has no \
                     `placement_probability_per_valid_position`"
                        .to_string()
                })?,
        })
    }
}

/// The world this feature reads and writes.
pub trait MagmaWorld: PredicateWorld {
    fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str);

    /// `Block.isShapeFullBlock(state.getFaceOcclusionShape(direction))` at
    /// this position, or `None` when the world cannot say.
    ///
    /// `direction` indexes `Direction.values()`: DOWN, UP, NORTH, SOUTH,
    /// WEST, EAST. `None` makes the feature refuse rather than guess — a
    /// default of "not full" would call every face visible and place no
    /// magma anywhere, which reads as a quiet nothing rather than a missing
    /// table.
    fn face_is_full(&mut self, x: i32, y: i32, z: i32, direction: usize) -> Option<bool>;

    /// `BlockState#isAir`, or `None` when the world cannot say.
    ///
    /// A name check is not good enough and this is the feature that proves
    /// it: **`minecraft:cave_air` is air**, and carvers write it through
    /// every cave in the world. A scan that stops at cave_air finds a floor
    /// where there is none and drops magma into the middle of a cavern —
    /// which is exactly what this port did before the block-property table
    /// answered the question. `void_air` is the third one.
    fn is_air(&mut self, x: i32, y: i32, z: i32) -> Option<bool>;

    /// `state.is(Blocks.WATER)` — a **block** identity test, so
    /// `water[level=3]` counts and a waterlogged fence does not. That is the
    /// opposite way round from `matching_fluids`, which tests a fluid state.
    ///
    /// This is the predicate the floor scan uses. Do not substitute
    /// [`Self::is_water_or_air`].
    fn is_water(&mut self, x: i32, y: i32, z: i32) -> bool {
        let block = self.block_at(x, y, z);
        block.split_once('[').map_or(block.as_str(), |(n, _)| n) == "minecraft:water"
    }

    /// `UnderwaterMagmaFeature#isWaterOrAir` — `state.is(Blocks.WATER) ||
    /// state.isAir()`.
    ///
    /// Used by `isValidPlacement` **only**. The floor scan uses
    /// [`Self::is_water`]; see the module docs for what happens when the two
    /// are confused.
    fn is_water_or_air(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
        if self.is_water(x, y, z) {
            return Some(true);
        }
        self.is_air(x, y, z)
    }
}

/// `Direction.values()` indices, and the four horizontal ones with their
/// opposites — `Direction.Plane.HORIZONTAL` in its own iteration order.
const DOWN: usize = 0;
const UP: usize = 1;
/// `(direction, its opposite)` for NORTH, SOUTH, WEST, EAST.
const HORIZONTAL: [(usize, usize); 4] = [(2, 3), (3, 2), (4, 5), (5, 4)];

/// The feature could not be evaluated because the world could not answer a
/// shape question.
#[derive(Debug, PartialEq, Eq)]
pub struct NeedsShapes;

/// `UnderwaterMagmaFeature#place`. Returns whether any block was placed.
///
/// # Errors
/// If the world cannot answer a face-occlusion question.
pub fn place<W: MagmaWorld + ?Sized, R: RandomSource>(
    world: &mut W,
    config: &UnderwaterMagmaConfig,
    random: &mut R,
    origin: (i32, i32, i32),
) -> Result<bool, NeedsShapes> {
    let (ox, _oy, oz) = origin;
    let Some(floor_y) = floor_y(world, config, origin)? else {
        // No draws at all on this path: `getFloorY` runs before the stream
        // is touched.
        return Ok(false);
    };

    let r = config.placement_radius_around_floor;
    let mut placed = 0usize;
    // `BlockPos.betweenClosedStream(BoundingBox.fromCorners(...))` — x outer,
    // then y, then z.
    // `BlockPos.betweenClosedStream` walks a flat index and decomposes it
    // `i = n % width` first — read from `BlockPos$4.computeNext`'s bytecode,
    // where the first instruction after loading `index` is
    // `getfield val$width; irem`. So **x is the fastest axis**, then y, then
    // z: the box is z-outer, x-inner.
    //
    // This file had it the other way round, and it was not a free mistake.
    // The `nextFloat` is drawn for every position *before* any question is
    // asked about the world, so the nesting decides which position each draw
    // lands on. Getting it backwards misplaces magma without changing how
    // much of it there is, which is exactly what the whole-world diff showed:
    // 59 blocks too many and 53 too few. Worth 402 blocks at seed 31415926.
    for z in oz - r..=oz + r {
        for y in floor_y - r..=floor_y + r {
            for x in ox - r..=ox + r {
                // The draw happens for every position in the box, before any
                // question is asked about the world. See the module docs.
                if random.next_f32() >= config.placement_probability_per_valid_position {
                    continue;
                }
                if is_valid_placement(world, (x, y, z))? {
                    world.set_block(x, y, z, "minecraft:magma_block");
                    placed += 1;
                }
            }
        }
    }
    Ok(placed > 0)
}

/// `UnderwaterMagmaFeature#getFloorY`, which is `Column.scan` with
/// `isWaterOrAir` as the column predicate and its negation as the tip.
fn floor_y<W: MagmaWorld + ?Sized>(
    world: &mut W,
    config: &UnderwaterMagmaConfig,
    origin: (i32, i32, i32),
) -> Result<Option<i32>, NeedsShapes> {
    let (x, y, z) = origin;
    // `state.is(Blocks.WATER)`, not `isWaterOrAir`. See the module docs.
    if !world.is_water(x, y, z) {
        return Ok(None);
    }
    // `Column.scan` runs the upward leg first and the downward second; only
    // the downward one is read here, but the order is kept because the two
    // share a cursor in vanilla and a reader comparing the two files should
    // not have to wonder.
    let _above = scan_direction(world, config.floor_search_range, x, y, z, 1);
    Ok(scan_direction(
        world,
        config.floor_search_range,
        x,
        y,
        z,
        -1,
    ))
}

/// `Column#scanDirection`.
///
/// The loop tests the *current* position before moving, and the caller has
/// already tested the start — so it advances while the column predicate holds
/// and stops after at most `range - 1` steps. The tip test then runs wherever
/// it stopped.
fn scan_direction<W: MagmaWorld + ?Sized>(
    world: &mut W,
    range: i32,
    x: i32,
    start_y: i32,
    z: i32,
    step: i32,
) -> Option<i32> {
    let mut y = start_y;
    let mut i = 1;
    while i < range && world.is_water(x, y, z) {
        y += step;
        i += 1;
    }
    // The tip predicate is the column predicate's negation: the first block
    // that is not water.
    if world.is_water(x, y, z) {
        None
    } else {
        Some(y)
    }
}

/// `UnderwaterMagmaFeature#isValidPlacement`.
fn is_valid_placement<W: MagmaWorld + ?Sized>(
    world: &mut W,
    pos: (i32, i32, i32),
) -> Result<bool, NeedsShapes> {
    let (x, y, z) = pos;
    if world.is_water_or_air(x, y, z).ok_or(NeedsShapes)? {
        return Ok(false);
    }
    // The block *below*, looked at from above: magma needs something solid
    // under it or the player falls through into a cave.
    if visible_from_outside(world, (x, y - 1, z), UP)? {
        return Ok(false);
    }
    for (direction, opposite) in HORIZONTAL {
        let neighbour = offset(pos, direction);
        if visible_from_outside(world, neighbour, opposite)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// `UnderwaterMagmaFeature#isVisibleFromOutside` — the negation of a full
/// occluding face. See the module docs for why the two-clause original
/// reduces to this.
fn visible_from_outside<W: MagmaWorld + ?Sized>(
    world: &mut W,
    pos: (i32, i32, i32),
    direction: usize,
) -> Result<bool, NeedsShapes> {
    world
        .face_is_full(pos.0, pos.1, pos.2, direction)
        .map(|full| !full)
        .ok_or(NeedsShapes)
}

/// One step in a `Direction.values()` direction.
const fn offset(pos: (i32, i32, i32), direction: usize) -> (i32, i32, i32) {
    let (x, y, z) = pos;
    match direction {
        DOWN => (x, y - 1, z),
        UP => (x, y + 1, z),
        2 => (x, y, z - 1),
        3 => (x, y, z + 1),
        4 => (x - 1, y, z),
        _ => (x + 1, y, z),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::legacy::LegacyRandomSource;
    use crate::rng::worldgen_random::WorldgenRandom;
    use std::collections::BTreeMap;

    struct Sea {
        blocks: BTreeMap<(i32, i32, i32), String>,
        /// Every position asked a shape question, so a test can assert the
        /// feature looked where it should.
        faces_asked: Vec<(i32, i32, i32, usize)>,
    }

    impl PredicateWorld for Sea {
        fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
            self.blocks
                .get(&(x, y, z))
                .cloned()
                .unwrap_or_else(|| "minecraft:air".to_string())
        }
    }

    impl MagmaWorld for Sea {
        fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str) {
            self.blocks.insert((x, y, z), name.to_string());
        }
        fn face_is_full(&mut self, x: i32, y: i32, z: i32, direction: usize) -> Option<bool> {
            self.faces_asked.push((x, y, z, direction));
            // Stone is a full cube on every face; everything else is not.
            Some(self.block_at(x, y, z) == "minecraft:stone")
        }
        fn is_air(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
            let block = self.block_at(x, y, z);
            Some(block == "minecraft:air" || block == "minecraft:cave_air")
        }
    }

    /// Water down to y=0, stone below.
    fn ocean() -> Sea {
        let mut blocks = BTreeMap::new();
        for x in -4..=4 {
            for z in -4..=4 {
                for y in 1..=8 {
                    blocks.insert((x, y, z), "minecraft:water[level=0]".to_string());
                }
                for y in -8..=0 {
                    blocks.insert((x, y, z), "minecraft:stone".to_string());
                }
            }
        }
        Sea {
            blocks,
            faces_asked: Vec::new(),
        }
    }

    fn config(radius: i32, probability: f64) -> UnderwaterMagmaConfig {
        UnderwaterMagmaConfig::parse(
            &serde_json::from_str(&format!(
                r#"{{"floor_search_range":5,
                     "placement_radius_around_floor":{radius},
                     "placement_probability_per_valid_position":{probability}}}"#
            ))
            .expect("json"),
        )
        .expect("parse")
    }

    /// The draw count is the box, not the valid positions — the filter order
    /// in the stream is behaviour.
    #[test]
    fn every_position_in_the_box_costs_a_draw() {
        let config = config(1, 0.0);
        let mut world = ocean();
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(3));
        place(&mut world, &config, &mut random, (0, 4, 0)).expect("ok");

        let mut reference = WorldgenRandom::new(LegacyRandomSource::new(3));
        // 3x3x3 = 27 positions, 27 draws, even at probability zero where not
        // one of them can be placed.
        for _ in 0..27 {
            reference.next_f32();
        }
        assert_eq!(
            random.next_i64(),
            reference.next_i64(),
            "the probability filter runs before the validity filter, so it costs \
             a draw for every position in the box"
        );
    }

    /// A column with no floor inside the search range costs nothing at all —
    /// `getFloorY` runs before the stream is touched.
    #[test]
    fn a_bottomless_column_draws_nothing() {
        let config = config(1, 1.0);
        let mut world = Sea {
            blocks: BTreeMap::new(),
            faces_asked: Vec::new(),
        };
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(3));
        let mut reference = WorldgenRandom::new(LegacyRandomSource::new(3));
        assert!(!place(&mut world, &config, &mut random, (0, 4, 0)).expect("ok"));
        assert_eq!(random.next_i64(), reference.next_i64());
    }

    /// The floor is the first non-water block below, and magma lands on it.
    #[test]
    fn magma_lands_on_the_floor_of_the_water_column() {
        let config = config(0, 1.0);
        let mut world = ocean();
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(3));
        assert!(place(&mut world, &config, &mut random, (0, 4, 0)).expect("ok"));
        assert_eq!(
            world.blocks.get(&(0, 0, 0)).map(String::as_str),
            Some("minecraft:magma_block"),
            "y=0 is the first block below the water that is not water"
        );
    }

    /// A block with a hole beside it is rejected, because a player would see
    /// the magma from the cave next door.
    #[test]
    fn a_position_open_to_a_cave_is_rejected() {
        let config = config(0, 1.0);
        let mut world = ocean();
        world.blocks.insert((1, 0, 0), "minecraft:air".to_string());
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(3));
        assert!(!place(&mut world, &config, &mut random, (0, 4, 0)).expect("ok"));
        assert_ne!(
            world.blocks.get(&(0, 0, 0)).map(String::as_str),
            Some("minecraft:magma_block")
        );
    }

    /// A world that cannot answer a shape question makes the feature refuse
    /// rather than place nothing quietly.
    #[test]
    fn a_world_without_shapes_is_refused() {
        struct Blind(Sea);
        impl PredicateWorld for Blind {
            fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
                self.0.block_at(x, y, z)
            }
        }
        impl MagmaWorld for Blind {
            fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str) {
                self.0.set_block(x, y, z, name);
            }
            fn face_is_full(&mut self, _: i32, _: i32, _: i32, _: usize) -> Option<bool> {
                None
            }
            fn is_air(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
                self.0.is_air(x, y, z)
            }
        }
        let config = config(0, 1.0);
        let mut world = Blind(ocean());
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(3));
        assert_eq!(
            place(&mut world, &config, &mut random, (0, 4, 0)),
            Err(NeedsShapes)
        );
    }

    #[test]
    fn the_real_config_parses() {
        let path = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../oracle-work/vanilla-26.2/generated-data/data/minecraft/worldgen/configured_feature/underwater_magma.json"
        ));
        if !rustmc_data::fixture::available(path, "datagen extract", "ci/vanilla-server.sh data") {
            return;
        }
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).expect("readable")).expect("json");
        let config = UnderwaterMagmaConfig::parse(&json["config"]).expect("parses");
        assert_eq!(config.floor_search_range, 5);
        assert_eq!(config.placement_radius_around_floor, 1);
        assert!((config.placement_probability_per_valid_position - 0.5).abs() < f32::EPSILON);
    }
}
