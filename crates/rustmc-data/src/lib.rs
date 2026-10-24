//! rustmc-data — extracted-vanilla-data loading + the base identity types
//! every other crate speaks in.
//!
//! Loads the same 1,486 jar-extracted JSON files Minecom's
//! `extract_vanilla_data.py` produces (registries, tags, loot tables,
//! worldgen configs). This crate sits at the bottom of the workspace
//! dependency graph, so the tiny shared vocabulary types (positions, ids)
//! live here too.
//!
//! FROZEN INTERFACE (CRATES.md rule): the types below are part of the
//! inter-crate freeze. Internals may change freely; signatures may not
//! without an orchestrator-approved interface bump.

/// `java.util.HashMap` iteration order, reproduced.
///
/// **It lives here by layering rather than by category, and saying so is
/// better than implying a category it does not belong to.** This is a JDK
/// primitive, not extracted vanilla data — but two unrelated things now need
/// it and they are on opposite sides of the graph. `rustmc-worldgen` needs it
/// because `TreeFeature.place` collects into `Sets.newHashSet()` and what
/// comes out is behaviour; `rustmc-world`'s `nbt` writer needs it because
/// `CompoundTag` wraps a `HashMap` and vanilla writes `keySet()` order, so
/// any packet body carrying a multi-key compound is only byte-identical in
/// that order.
///
/// It was in `rustmc-worldgen` until 2026-08-05 and could not be called from
/// `nbt.rs`: `rustmc-worldgen` depends on `rustmc-world`, so the call was a
/// dependency cycle. This crate is the bottom of the graph, so the move adds
/// no edge anywhere and needs none added later — the alternative,
/// `rustmc-world`, would have put it above `rustmc-proto`, which is the next
/// plausible caller.
///
/// **Not a second copy.** Writing one would have been the cheap move and it
/// is the wrong one: this module is verified against real JVM output by
/// `ci/hash-order-golden.sh`, and a second implementation would be an
/// unverified one that agrees with the first until the day it does not.
pub mod java_hash_set;

/// Fixture gating that a gate can see.
///
/// **Here for the same layering reason `java_hash_set` is.** Every crate with
/// a vanilla-backed test — `rustmc-world`, `rustmc-worldgen`, `rustmc-server`,
/// `rustmc-behavior` — already depends on this one, and none of them depends on
/// each other in the direction that would let any of them host it. The bottom
/// of the graph adds no edge anywhere.
///
/// It is test support in a non-test crate deliberately: a `dev-dependencies`
/// home would need a new workspace member, and Cargo.toml changes go through
/// the orchestrator. The module is inert unless called.
pub mod fixture;

/// Named wrong implementations, for asking a T1 gate what would make it red.
///
/// Behind the `t1-sabotage` feature and inert without it. `ci/t0.sh` asserts
/// the feature is off in every default set — see the module docs and
/// `docs/PARITY-ROADMAP.md` §84.
pub mod sabotage;

/// A block position in world space. Vanilla semantics: y is signed,
/// x/z unbounded (i32 like the reference).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

/// A chunk column position (world x/z >> 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChunkPos {
    pub x: i32,
    pub z: i32,
}

impl ChunkPos {
    /// The chunk containing a block position.
    ///
    /// ```
    /// use rustmc_data::{BlockPos, ChunkPos};
    /// let p = BlockPos { x: -1, y: 64, z: 16 };
    /// assert_eq!(ChunkPos::containing(p), ChunkPos { x: -1, z: 1 });
    /// ```
    pub fn containing(p: BlockPos) -> Self {
        // Arithmetic shift = floor division by 16, matching vanilla.
        ChunkPos {
            x: p.x >> 4,
            z: p.z >> 4,
        }
    }
}

/// A full block state (block + property values) as a global palette id.
/// u32: 26.x state count exceeds u16. Id assignment order MUST match the
/// extracted registry order so Anvil palettes serialize identically to
/// vanilla (region-diff G3 compares full state, not just block).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockStateId(pub u32);

/// A biome as a registry id. Id assignment order MUST match the extracted
/// registry order (same rule as [`BlockStateId`]) — biomes serialize into
/// Anvil section data and wire chunk packets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BiomeId(pub u32);

/// An entity type as a registry id, extracted-registry order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EntityKindId(pub u32);

/// Which of vanilla's two light layers to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LightKind {
    Block,
    Sky,
}

/// A dimension we generate/simulate. Matches ORACLE.md genregions contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dimension {
    Overworld,
    Nether,
    End,
}

/// Handle to all loaded registries. Shared read-only across regions/threads
/// (`Arc<Registries>` — regionization rule 1: shared state is immutable).
#[derive(Debug, Default)]
pub struct Registries {
    // Internals free to change: block states, items, biomes, tags,
    // loot tables, worldgen configs, ...
}

impl Registries {
    /// Load from an extracted-data directory (the extractor's output layout).
    /// Fails loudly on schema drift — silent partial loads are how gap bugs
    /// hide.
    pub fn load(_dir: &std::path::Path) -> Result<Self, DataError> {
        Err(DataError::NotYetImplemented)
    }
}

#[derive(Debug)]
pub enum DataError {
    NotYetImplemented,
    Io(std::io::Error),
    Schema(String),
}
