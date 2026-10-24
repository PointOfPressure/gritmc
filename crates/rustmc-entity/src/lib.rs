//! rustmc-entity — ECS entity storage, spatial index, per-region entity
//! ownership. Systems are registered by behavior/server, not defined here.
//!
//! Storage layout (archetype vs sparse-set) is the implementer's call,
//! benchmarked — that's internal. The FROZEN part is [`EntityView`] and
//! [`EntityId`]: how the rest of the workspace refers to and queries
//! entities.
//!
//! That call is now made and its reasoning is in [`store`], because it is
//! the one decision here that cannot be revisited cheaply: **struct-of-
//! arrays in a generational arena**. Vanilla's *behaviour* is the
//! specification and its *representation* is not — the same argument that
//! made the density function one `match`-evaluated enum rather than a
//! virtual call per sample.
//!
//! **There is no per-kind behaviour here yet, on purpose.** Vanilla's tick
//! order is observable — target selection, item merging and collision
//! resolution all depend on which entity moved first — so it has to come
//! from the bytecode rather than from a plausible guess. A guess would be
//! exactly the kind of approximation that forecloses parity later.
//!
//! The *order* is now read and lives in [`tick_list`]: insertion order
//! through an `Int2ObjectLinkedOpenHashMap`, with an entity added mid-tick
//! waiting for the next one and an entity removed mid-tick still visited by
//! the current one. The loop **body** — `isRemoved`, `checkDespawn`,
//! `inEntityTickingRange`, the vehicle check, then `setOldPosAndRot` before
//! `tick()` and passengers after their vehicle — is written down in
//! docs/WORKLIST.md and not yet implemented, because none of the gates it
//! consults exists in this crate.
//!
//! # An example, and why this crate has one
//!
//! This crate's doctest target ran **zero** examples on every gate this
//! project has ever run (`ci/checks/test-targets.toml`, 2026-08-16). The
//! arena's generational identity is the part a caller has to be able to rely
//! on, so that is what the example shows:
//!
//! ```
//! use rustmc_data::EntityKindId;
//! use rustmc_entity::store::{Entities, Spawn};
//!
//! let mut store = Entities::new();
//! assert!(store.is_empty());
//!
//! let id = store.spawn(Spawn {
//!     kind: EntityKindId(0),
//!     pos: (-0.5, 64.0, 0.5),
//!     velocity: (0.0, 0.0, 0.0),
//!     yaw: 0.0,
//!     pitch: 0.0,
//! });
//! assert_eq!(store.len(), 1);
//!
//! // Removal is two steps on purpose: vanilla still visits an entity removed
//! // mid-tick, so `mark_removed` is observable and `despawn` is the sweep.
//! assert!(store.mark_removed(id));
//! assert!(store.is_removed(id));
//! assert!(store.despawn(id));
//! assert!(store.is_empty());
//! ```

pub mod ai;
pub mod brain;
pub mod host;
pub mod mth;
pub mod mth_tables;
pub mod pathfinder;
pub mod phys;
pub mod spawn;
pub mod store;
pub mod testing;
pub mod tick;
pub mod tick_list;
pub mod trading;

use rustmc_data::{BlockPos, EntityKindId};

/// Stable entity handle. Never reused within a world's lifetime
/// (generational — internals free to change, semantics frozen).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EntityId(pub u64);

/// FROZEN INTERFACE: read access to entities for behavior code and plugins.
/// Mutation goes through systems/commands owned by the region tick, not
/// through this view (regionization rule 2: cross-region effects are
/// messages).
pub trait EntityView {
    /// Entities whose bounding box intersects the box around `center` with
    /// half-extent `radius` blocks. Deterministic iteration order
    /// (EntityId-sorted) — iteration order is behavior-visible in vanilla
    /// (target selection, collision resolution).
    ///
    /// # The declared order is deterministic, and it is not vanilla's
    ///
    /// Frozen, so this is a note and not an edit to the contract. Vanilla's
    /// AABB query walks two nested orders, neither of which is by id:
    ///
    /// * **sections**, major, from `EntitySectionStorage`. The outer loop runs
    ///   section x ascending and the inner one walks
    ///   `sectionIds.subSet(...)`, a `LongAVLTreeSet`, so it is SORTED on the
    ///   packed section key. That half is a function of coordinates, which is
    ///   state, and is portable by writing the same comparator;
    /// * **within a section**, minor, from `ClassInstanceMultiMap.allInstances`
    ///   — a plain `List`. That half is INSERTION order: the order entities
    ///   were added to that section, which a `remove` then `add` moves to the
    ///   end.
    ///
    /// So an `EntityId`-sorted answer differs from vanilla's whenever two
    /// entities share a section and were not added in id order. It is the
    /// right kind of stand-in — deterministic, and the alternative of a hash
    /// order would not be — but it is a second order-keyed divergence beside
    /// `Entities::all`'s, and the two have different fixes: this one needs
    /// per-section insertion order, that one needs a hash walk.
    ///
    /// `ci/order-keyed-census.py` holds both, with the container each is
    /// derived from.
    fn entities_near(&self, center: BlockPos, radius: f64) -> Vec<EntityId>;

    /// Position of a live entity, None if despawned.
    fn position(&self, id: EntityId) -> Option<(f64, f64, f64)>;

    /// Entity type as a registry id, None if despawned (freeze review
    /// 2026-07-22: target selection, spawn caps, and plugins all filter by
    /// kind — position-only was too thin even for wave 2).
    fn kind(&self, id: EntityId) -> Option<EntityKindId>;
}
