//! Structures — villages, mineshafts, monuments and the rest.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `net.minecraft.world.level.levelgen.structure.*`,
//! `ChunkGeneratorStructureState`.
//!
//! Structures are the largest single item left in the whole-world diff —
//! roughly 2,400 of the 5,374 remaining mismatches are one mineshaft, and
//! BENCH.md has the measurement that says so. They run between the carvers
//! and decoration and write into the same canvas, so a parity number taken
//! without them is measuring their absence more than anything else.
//!
//! # Built from the bottom, like everything else
//!
//! [`placement`] first: which chunks a structure set may start in. It is a
//! pure function of the seed and the chunk position — no world, no biomes,
//! no pieces — so it can be gated against vanilla before a single structure
//! block exists. A generator that builds the right pieces in the wrong
//! chunks is not closer than one that builds nothing.

pub mod buried_treasure;
pub mod end_city;
pub mod fortress;
pub mod jigsaw;
pub mod mansion;
pub mod mineshaft;
pub mod nether_fossil;
pub mod ocean_monument;
pub mod ocean_ruin;
pub mod pieces;
pub mod place;
pub mod placement;
pub mod pool_alias;
pub mod processor;
pub mod ruined_portal;
pub mod scattered;
pub mod shipwreck;
pub mod spawned;
pub mod step_index;
pub mod stronghold;
pub mod template;
pub mod template_pool;
pub mod writer;
