//! rustmc-world — chunk representation (paletted sections), heightmaps,
//! lighting storage, Anvil read/write byte-compatible with vanilla.
//! No behavior lives here.
//!
//! This crate owns two FROZEN interfaces (CRATES.md):
//! [`ChunkAccess`] — how everything reads/writes loaded world state, and
//! [`Canvas`] — the worldgen shared canvas with defined-order cross-chunk
//! decoration writes (ORACLE.md §6 is binding).
//!
//! # An example, and why this crate has one
//!
//! This crate's doctest target ran **zero** examples on every gate this
//! project has ever run (`ci/checks/test-targets.toml`, 2026-08-16). The
//! headline claim above is byte-compatibility with vanilla's storage, so the
//! example is a round trip through the encoder rather than a tour of the API:
//!
//! ```
//! use rustmc_world::nbt::{self, Nbt};
//! use std::collections::BTreeMap;
//!
//! let mut root = BTreeMap::new();
//! root.insert("DataVersion".to_string(), Nbt::Int(4325));
//! root.insert("Status".to_string(), Nbt::String("minecraft:full".to_string()));
//!
//! let mut bytes = Vec::new();
//! nbt::write_named_root(&mut bytes, "", &Nbt::Compound(root)).unwrap();
//! let (name, back) = nbt::read_named_root(&mut bytes.as_slice()).unwrap();
//!
//! assert_eq!(name, "");
//! assert_eq!(back.get("DataVersion").and_then(Nbt::as_i32), Some(4325));
//! assert_eq!(back.get("Status").and_then(Nbt::as_str), Some("minecraft:full"));
//! assert_eq!(back.get("NoSuchKey"), None);
//! ```

use rustmc_data::{BiomeId, BlockPos, BlockStateId, ChunkPos, LightKind};

/// Read/write access to loaded chunk data. Implemented by the live world
/// (per-region ownership) and by worldgen protochunks.
///
/// Regionization rule (CRATES.md): implementations are NOT globally shared
/// mutable state — a `&mut dyn ChunkAccess` is handed to the code that owns
/// the region/chunk this tick. `Send` so region state can migrate between
/// worker threads at tick boundaries.
pub trait ChunkAccess: Send {
    /// Full block state at `pos`. Out-of-bounds y returns air.
    fn block_state(&self, pos: BlockPos) -> BlockStateId;

    /// Set a full block state. Returns the previous state.
    /// No behavior side effects here (no updates, no light) — behavior is
    /// rustmc-behavior's job; this is storage.
    fn set_block_state(&mut self, pos: BlockPos, state: BlockStateId) -> BlockStateId;

    /// MOTION_BLOCKING-style heightmap read (exact set of maps TBD by the
    /// Anvil work; frozen at the method level, map enum may extend).
    fn height(&self, x: i32, z: i32) -> i32;

    /// Biome at `pos`. Vanilla stores biomes per 4×4×4 cell; reads at block
    /// granularity resolve to the containing cell (freeze review 2026-07-22:
    /// worldgen writes these into Anvil + wire chunks, spawning/features
    /// read them — absent from the day-1 draft, added pre-freeze).
    fn biome(&self, pos: BlockPos) -> BiomeId;

    /// Set the biome cell containing `pos` (4×4×4 quantization is the
    /// implementation's job; callers think in block positions).
    fn set_biome(&mut self, pos: BlockPos, biome: BiomeId);

    /// Stored light level 0–15 at `pos` for the given layer. Read-only:
    /// the light engine (world/server internals) owns writes. Protochunks
    /// mid-generation may legitimately return 0 (light not yet computed).
    fn light(&self, pos: BlockPos, kind: LightKind) -> u8;
}

/// The worldgen shared canvas (ORACLE.md §6, contract is binding):
///
/// 1. Writes are a pure function of (seed, chunk pos, datapack version) —
///    never of scheduling, thread count, or map iteration order.
/// 2. Cross-chunk decoration spills land on this canvas in the canonical
///    chunk order ([`DECORATION_ORDER`]); the canvas NEVER drops border
///    writes (the vibenilla lesson: parallel spill-dropping cost 0.67pp
///    and its nondeterminism invalidated every measurement on top).
/// 3. Any nondeterminism observed downstream is a stop-the-line bug.
pub trait Canvas: ChunkAccess {
    /// The chunk this canvas pass is centered on (features may write into
    /// neighbors; the canvas tracks and applies spills deterministically).
    fn center(&self) -> ChunkPos;
}

/// Canonical decoration chunk order within a generation region.
/// Minecom measured z-major winning for the overworld; the constant is
/// per-dimension A/B-able but MUST be a named constant, never emergent
/// from a container's iteration order.
pub const DECORATION_ORDER: &str = "z-major";

pub mod anvil;
pub mod entitydat;
pub mod entitystore;
pub mod level;
pub mod nbt;
pub mod playerdat;
