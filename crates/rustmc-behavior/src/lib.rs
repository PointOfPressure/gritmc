//! rustmc-behavior — the boring 80%: block interactions, item use, mob AI,
//! redstone, combat, status effects. The crate the swarm grinds on longest.
//!
//! Organized by vanilla's own taxonomy: one module per mechanic family, each
//! citing the decompiled reference classes it ports (Minecom discipline:
//! port behavior, cite the class, never copy code).
//!
//! FROZEN INTERFACE: [`BlockBehavior`]. Everything else in this crate is
//! internal and swarm-editable.
//!
//! # An example, and why this crate has one
//!
//! This crate's doctest target ran **zero** examples on every gate this
//! project has ever run (`ci/checks/test-targets.toml`, 2026-08-16). What it
//! pins is vanilla's `Direction` ordinal order, because getting it wrong is
//! not hypothetical here: a test in `tools/oracle-runner` asserted that the
//! cursor for the `DOWN` face lay on the *x* face, having used the pair index
//! as the coordinate index, and it had never run in any gate
//! (`docs/AGENT-BRIEF.md`).
//!
//! ```
//! use rustmc_behavior::block_props::DIRECTIONS;
//!
//! // `Direction.values()` groups the axes Y, Z, X — so the face index is
//! // NOT the coordinate index, and every `[bool; 6]` in `BlockProps` is
//! // indexed in this order. The loaded table's own `directionOrder` is
//! // checked against this rather than assumed, which only means anything if
//! // this array is itself pinned.
//! assert_eq!(DIRECTIONS, ["DOWN", "UP", "NORTH", "SOUTH", "WEST", "EAST"]);
//! assert_eq!(DIRECTIONS.iter().position(|d| *d == "WEST"), Some(4));
//! assert_ne!(DIRECTIONS.iter().position(|d| *d == "DOWN"), Some(1));
//! ```

pub mod block_props;
pub mod place_survive;
pub mod survival;

use rustmc_data::{BlockPos, BlockStateId};
use rustmc_world::ChunkAccess;

/// Outcome of a use/interact action, wire-visible (swing, sound, block
/// change) — mirrors vanilla's InteractionResult semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractionResult {
    Success,
    Consume,
    Pass,
    Fail,
}

/// FROZEN INTERFACE: per-block gameplay rules. One implementation per block
/// family, registered against block ids at data-load time.
///
/// Update-order-visible mechanics (redstone!) stay serial within a region —
/// CRATES.md regionization rule 3. Nothing in this trait may spawn threads
/// or touch state outside the `ChunkAccess` it is handed.
pub trait BlockBehavior: Send + Sync {
    /// Right-click use. Default: pass through (no behavior).
    fn on_use(
        &self,
        _world: &mut dyn ChunkAccess,
        _pos: BlockPos,
        _state: BlockStateId,
    ) -> InteractionResult {
        InteractionResult::Pass
    }

    /// A neighbor changed (the vanilla "block update"). Ordering of updates
    /// is observable behavior; callers guarantee vanilla dispatch order.
    fn on_neighbor_changed(
        &self,
        _world: &mut dyn ChunkAccess,
        _pos: BlockPos,
        _state: BlockStateId,
        _from: BlockPos,
    ) {
    }

    /// Random tick (crops, leaves, ice...). Called with the region's seeded
    /// RNG stream — implementations never construct their own RNG (ORACLE.md
    /// §5: seeded per-tick streams are part of determinism).
    fn random_tick(
        &self,
        _world: &mut dyn ChunkAccess,
        _pos: BlockPos,
        _state: BlockStateId,
        _rng: &mut dyn FnMut() -> u64,
    ) {
    }
}

// Mechanic-family modules land here in wave 3, one agent per module:
// pub mod redstone;
// pub mod combat;
// pub mod containers;
// pub mod mob_ai;
// ...
