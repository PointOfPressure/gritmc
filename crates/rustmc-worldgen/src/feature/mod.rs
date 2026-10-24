//! Features — the decoration stage: ores, trees, lakes, springs, patches.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `ChunkGenerator#applyBiomeDecoration`, `PlacedFeature`,
//! `net.minecraft.world.level.levelgen.feature.*`.
//!
//! This runs after [`crate::carver`] and is the largest stage in worldgen by
//! a wide margin. It is being built from the bottom: the piece that decides
//! *which seed each feature gets* comes first, because nothing above it can
//! be checked while it is wrong.
//!
//! # The seeding, in one place
//!
//! ```text
//! decorationSeed = random.setDecorationSeed(worldSeed, minBlockX, minBlockZ)
//! for step in 0..steps:
//!   for feature in the step's features present in this chunk's biomes:
//!     random.setFeatureSeed(decorationSeed, indexWithinStep, step)
//!     feature.place(...)
//! ```
//!
//! `indexWithinStep` is **not** the feature's position in the biome's own
//! list. It is its position in a single global ordering computed once from
//! the entire biome registry — see [`sorter`]. Two worlds that disagree about
//! that ordering disagree about every tree.

pub mod aquatic;
pub mod bamboo;
pub mod block_column;
pub mod block_pile;
pub mod block_predicate;
pub mod blue_ice;
pub mod configured;
pub mod decorate;
pub mod disk;
pub mod end;
pub mod fallen_tree;
pub mod freeze_top_layer;
pub mod geode;
pub mod huge_mushroom;
pub mod iceberg;
pub mod int_provider;
pub mod lake;
pub mod large_dripstone;
pub mod monster_room;
pub mod mossy_carpet;
pub mod multiface_growth;
pub mod nether;
pub mod nether_forest;
pub mod ore;
pub mod placement;
pub mod sculk_patch;
pub mod sculk_spreader;
pub mod sculk_worldgen;
pub mod simple_block;
pub mod sorter;
pub mod speleothem;
pub mod speleothem_cluster;
pub mod spring;
pub mod state_provider;
pub mod state_spec;
pub mod tree;
pub mod underwater_magma;
pub mod vegetation_patch;
pub mod vines;
