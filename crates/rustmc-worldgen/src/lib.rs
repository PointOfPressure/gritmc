//! rustmc-worldgen — noise, terrain shaping, carving, surface rules,
//! features/decoration on the shared-canvas defined-order model.
//!
//! Pure function of (seed, pos, data). Depends on world + data only.
//! ORACLE.md §5 (float/RNG exactness) and §6 (determinism contract) are
//! binding on every line in this crate:
//!
//! - Every RNG ships with golden-stream tests (first 10k outputs from known
//!   seeds, exported from the JVM once, committed under `golden/`).
//! - Java quirks ported explicitly: wrapping arithmetic, `Math.sin` lookup
//!   table (not libm), float→double widening at the exact reference sites.
//! - f32/f64 order-of-operations copied from the decompile; no algebraic
//!   simplification; no fast-math anywhere, ever.
//! - Parallelize only stages proven order-independent (noise/terrain
//!   per-chunk); decoration runs in canonical order (see
//!   `rustmc_world::DECORATION_ORDER`).
//!
//! # An example, and why this crate has one
//!
//! This crate's doctest target ran only an ` ```ignore ` fence — zero
//! executed examples — on every gate this project has ever run
//! (`ci/checks/test-targets.toml`, 2026-08-16). The bullet above about Java
//! quirks ported explicitly is the crate's whole thesis, so the example is
//! the one that has already been got wrong in this problem space:
//!
//! ```
//! use rustmc_worldgen::structure::template::position_seed;
//!
//! // vanilla `Mth.getSeed`. `x * 3129871` is an **int** multiply (`imul`
//! // then `i2l`), so it wraps in 32 bits BEFORE widening. Computing it in
//! // 64 bits gives the same answer near spawn and a different one far out,
//! // which is why a spawn-centred region diff never sees the difference.
//! assert_eq!(position_seed(1, 2, 3), -33_674_130_277_896);
//! assert_eq!(position_seed(-1, -1, -1), 60_311_958_933_234);
//! assert_eq!(position_seed(700_000_000, 0, 0), 101_319_246_594_725);
//! ```

use rustmc_data::{ChunkPos, Dimension, Registries};
use rustmc_world::Canvas;

/// FROZEN INTERFACE: a chunk generator for one dimension.
pub trait ChunkGenerator: Send + Sync {
    /// Generate terrain (noise/surface/carvers) for one chunk into `canvas`.
    /// MUST be a pure function of (seed, pos, registries) — safe to run for
    /// many chunks in parallel.
    fn generate_terrain(&self, seed: i64, pos: ChunkPos, canvas: &mut dyn Canvas);

    /// Decorate one chunk (features/structures). Called by the driver in
    /// canonical chunk order; cross-chunk writes go through the canvas.
    /// NOT called in parallel — order is behavior.
    fn decorate(&self, seed: i64, pos: ChunkPos, canvas: &mut dyn Canvas);

    fn dimension(&self) -> Dimension;
}

/// Build the generator for a dimension from loaded data.
pub fn generator_for(_dim: Dimension, _registries: &Registries) -> Box<dyn ChunkGenerator> {
    unimplemented!("wave 2: terrain first, ratchet from zero (SWARM-PLAYBOOK)")
}

pub mod anvil_bridge;
pub mod biome;
pub mod block_entity;
pub mod canvas;
pub mod carver;
pub mod chunk_gen;
pub mod pipeline;
pub mod stage;
/// Vanilla's chunk-status pyramid — which chunks must be how far along
/// before one chunk may take its next step. `docs/DETERMINISM.md` §32.
pub mod status;

/// World-shape limits derived from `BlockPos`'s coordinate packing.
pub mod level;

/// Vanilla math helpers, in vanilla's association order.
pub mod fastutil_long_set;
/// **Moved down to `rustmc-data` on 2026-08-05 and re-exported here.**
///
/// `rustmc-world`'s NBT writer needs the same iteration order — vanilla's
/// `CompoundTag` wraps a `java.util.HashMap` and writes `keySet()` order — and
/// this crate depends on `rustmc-world`, so the call would have been a cycle.
/// The module is at the bottom of the graph now; nothing about this crate's
/// use of it changed, and `crate::java_hash_set` still resolves, so the tree
/// features that depend on `Sets.newHashSet()` order are untouched by the move.
pub use rustmc_data::java_hash_set;
pub mod math;
pub mod precise_log;
pub mod structure;

/// Density functions — the graph terrain generation evaluates.
pub mod density;
pub mod feature;

/// Noise primitives (improved/Perlin/normal), built on [`rng`].
pub mod noise;

/// RNG ports. Every source here is proved against JVM-exported golden
/// streams before anything consumes it (ORACLE.md §5 rule 1) — see
/// `tests/golden_rng.rs`.
pub mod rng;

/// Surface rules — grass, sand, gravel, bedrock.
pub mod surface;
pub mod tags;

/// Cached reads of the `RUSTMC_TRACE_*` switches. `getenv` in a feature's
/// inner loop was 3.8% of a full pipeline; see the module docs.
pub mod trace_switch;
