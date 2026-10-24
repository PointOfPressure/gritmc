//! Entity storage: the layout decision, and why it is this one.
//!
//! # This is a one-way door
//!
//! Almost nothing in this project has to be decided early. An algorithm can
//! be made faster whenever; a check can be tightened at any later date. The
//! *shape* entities are stored in is the exception. Everything else —
//! movement, collision, AI, spawning, item merging, the tick loop — is built
//! on top of it, and object-per-entity with a virtual call per tick cannot
//! be turned into something cache-friendly afterwards without rewriting all
//! of it.
//!
//! So the layout is chosen deliberately and the reasoning is written down.
//!
//! # What is reproduced, and what is ours
//!
//! **Vanilla's behaviour is the specification. Vanilla's representation is
//! not.** Vanilla stores one `Entity` object per entity and dispatches
//! `tick()` virtually. Reproducing that shape is not required to reproduce
//! what a player sees, and this crate has the precedent already: the density
//! function is one enum evaluated by `match` rather than a virtual call per
//! sample, which is semantically identical and much faster.
//!
//! Two things that *are* behaviour, and are therefore not ours to change:
//!
//! * **Iteration order.** Vanilla's tick order is observable — target
//!   selection, item merging and collision resolution all depend on which
//!   entity moved first. [`EntityView::entities_near`]'s frozen contract is
//!   `EntityId`-sorted, which is a *query* order and deliberately not the
//!   tick order. The tick order has to come from the bytecode, and until it
//!   does there is no tick loop here at all — see "What is missing".
//! * **Every field vanilla keeps.** Storing fewer is how a mechanic becomes
//!   impossible to implement later.
//!
//! # The layout
//!
//! **Struct-of-arrays in a generational arena.**
//!
//! * **Struct-of-arrays.** Position, velocity and rotation are read and
//!   written for every entity every tick, and almost nothing else is. Held
//!   as parallel `Vec`s they are contiguous, so a movement pass touches
//!   three dense arrays instead of chasing one pointer per entity into
//!   scattered allocations. This is the decision that cannot be retrofitted.
//! * **A generational arena.** A slot freed by a despawn is reused, so the
//!   arrays stay dense and no rehashing is needed; the generation counter
//!   makes a stale [`EntityId`] fail to resolve rather than silently address
//!   whatever now occupies the slot. `EntityId`'s frozen documentation
//!   already promises ids are never reused, and packing the generation
//!   alongside the index is how that promise is kept while still reusing
//!   storage.
//! * **One `match`, not one `dyn`.** Kind-dependent behaviour will branch on
//!   a plain registry id, not dispatch through a trait object per entity.
//!
//! None of this trades fidelity for speed: it is the same data, in a
//! different arrangement, producing identical answers. That is the only bar
//! an optimisation has to clear in parity mode, and it clears it by
//! construction.
//!
//! # What is missing, named rather than left to be discovered
//!
//! **There is no tick, no behaviour and no spawning policy here.** That is
//! deliberate: vanilla's tick order and its per-kind update rules are
//! observable, so they have to be ported from the bytecode rather than
//! guessed at, and a plausible-but-wrong version of either would be exactly
//! the kind of approximation that forecloses parity. Storage first, measured
//! against the gameplay oracle; behaviour on top of it, from the jar.

use crate::{EntityId, EntityView};
use rustmc_data::{BlockPos, EntityKindId};
use std::collections::BTreeMap;

/// How an [`EntityId`] packs into the frozen `u64`.
///
/// Low 32 bits index, high 32 bits generation. A slot is reused; an id is
/// not, because the generation moves on every despawn.
const INDEX_BITS: u32 = 32;

#[must_use]
const fn pack(index: u32, generation: u32) -> EntityId {
    EntityId(((generation as u64) << INDEX_BITS) | index as u64)
}

#[must_use]
const fn unpack(id: EntityId) -> (u32, u32) {
    #[allow(clippy::cast_possible_truncation)]
    let index = id.0 as u32;
    #[allow(clippy::cast_possible_truncation)]
    let generation = (id.0 >> INDEX_BITS) as u32;
    (index, generation)
}

/// Where an entity is and how it is moving.
///
/// Passed by value at the boundary and stored column-wise inside. Vanilla
/// keeps position and velocity as doubles and rotation as floats, and that
/// is reproduced exactly: narrowing either would change results.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spawn {
    pub kind: EntityKindId,
    pub pos: (f64, f64, f64),
    pub velocity: (f64, f64, f64),
    pub yaw: f32,
    pub pitch: f32,
}

/// Which chunk a coordinate belongs to.
///
/// `>> 4` after a floor, not a truncating cast: `-0.5` is in chunk −1, and
/// `as i32` would put it in chunk 0. That is one block of error at every
/// negative chunk border, which is exactly where a bug hides.
#[must_use]
fn chunk_of(x: f64, z: f64) -> (i32, i32) {
    #[allow(clippy::cast_possible_truncation)]
    let (bx, bz) = (x.floor() as i32, z.floor() as i32);
    (bx >> 4, bz >> 4)
}

/// The entity arena.
///
/// Columns are parallel: index `i` of every vector describes the same slot.
#[derive(Default)]
pub struct Entities {
    kind: Vec<EntityKindId>,
    x: Vec<f64>,
    y: Vec<f64>,
    z: Vec<f64>,
    vx: Vec<f64>,
    vy: Vec<f64>,
    vz: Vec<f64>,
    yaw: Vec<f32>,
    pitch: Vec<f32>,
    /// Bumped on every despawn, so a stale id cannot resolve.
    generation: Vec<u32>,
    alive: Vec<bool>,
    /// Vanilla's `Entity.isRemoved()`, which is **not** the same thing as
    /// `alive`.
    ///
    /// An entity marked removed is still in the world and still handed to
    /// the tick loop; `ServerLevel`'s per-entity body opens with
    /// `if (entity.isRemoved()) return;` precisely because
    /// `EntityTickList.forEach` does not skip it — see
    /// [`crate::tick_list`], where the copy-on-write that guarantees that is
    /// written out. The slot itself is not reclaimed until later, by
    /// `PersistentEntitySectionManager.tick`, which runs **after** the entity
    /// loop in `ServerLevel.tick`.
    ///
    /// Collapsing the two into one flag would look identical for
    /// `Entity.tick` and differ for everything the loop body does before it.
    removed: Vec<bool>,
    /// Slots ready for reuse, most recently freed first.
    free: Vec<u32>,
    /// Which entities are in which chunk.
    ///
    /// Vanilla buckets entities by section for the same reason: a proximity
    /// query that scans every entity in the world is fine with ten and not
    /// with ten thousand, and mob AI asks constantly. `BTreeMap` rather than
    /// a hash map so iteration is ordered, which keeps anything built on it
    /// deterministic by default.
    by_chunk: BTreeMap<(i32, i32), Vec<u32>>,
}

impl Entities {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many entities are alive.
    #[must_use]
    pub fn len(&self) -> usize {
        self.alive.iter().filter(|a| **a).count()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Add an entity, returning its handle.
    pub fn spawn(&mut self, spawn: Spawn) -> EntityId {
        let index = if let Some(index) = self.free.pop() {
            let i = index as usize;
            self.kind[i] = spawn.kind;
            self.x[i] = spawn.pos.0;
            self.y[i] = spawn.pos.1;
            self.z[i] = spawn.pos.2;
            self.vx[i] = spawn.velocity.0;
            self.vy[i] = spawn.velocity.1;
            self.vz[i] = spawn.velocity.2;
            self.yaw[i] = spawn.yaw;
            self.pitch[i] = spawn.pitch;
            self.alive[i] = true;
            self.removed[i] = false;
            index
        } else {
            self.kind.push(spawn.kind);
            self.x.push(spawn.pos.0);
            self.y.push(spawn.pos.1);
            self.z.push(spawn.pos.2);
            self.vx.push(spawn.velocity.0);
            self.vy.push(spawn.velocity.1);
            self.vz.push(spawn.velocity.2);
            self.yaw.push(spawn.yaw);
            self.pitch.push(spawn.pitch);
            self.generation.push(0);
            self.alive.push(true);
            self.removed.push(false);
            u32::try_from(self.kind.len() - 1).expect("fewer than 4 billion entity slots")
        };
        self.by_chunk
            .entry(chunk_of(spawn.pos.0, spawn.pos.2))
            .or_default()
            .push(index);
        pack(index, self.generation[index as usize])
    }

    /// `Entity.remove(...)`: mark the entity removed, leaving it in place.
    ///
    /// The current tick still visits it — that is the whole reason the flag
    /// exists rather than an immediate [`Self::despawn`]. Returns whether the
    /// handle resolved.
    pub fn mark_removed(&mut self, id: EntityId) -> bool {
        let Some(i) = self.slot(id) else { return false };
        self.removed[i] = true;
        true
    }

    /// `Entity.isRemoved()`. A handle that no longer resolves reads as
    /// removed, because from a caller's point of view it is.
    #[must_use]
    pub fn is_removed(&self, id: EntityId) -> bool {
        self.slot(id).is_none_or(|i| self.removed[i])
    }

    /// Resolve a handle to a live slot.
    fn slot(&self, id: EntityId) -> Option<usize> {
        let (index, generation) = unpack(id);
        let i = index as usize;
        (i < self.alive.len() && self.alive[i] && self.generation[i] == generation).then_some(i)
    }

    /// Remove an entity. Returns whether it was there to remove.
    pub fn despawn(&mut self, id: EntityId) -> bool {
        let Some(i) = self.slot(id) else {
            return false;
        };
        let key = chunk_of(self.x[i], self.z[i]);
        if let Some(bucket) = self.by_chunk.get_mut(&key) {
            bucket.retain(|s| *s as usize != i);
            if bucket.is_empty() {
                self.by_chunk.remove(&key);
            }
        }
        self.alive[i] = false;
        self.removed[i] = false;
        // Wrapping is correct rather than merely safe: after four billion
        // despawns of one slot the generation repeats, and an id held across
        // that many is not a handle anybody still has.
        self.generation[i] = self.generation[i].wrapping_add(1);
        self.free
            .push(u32::try_from(i).expect("fewer than 4 billion entity slots"));
        true
    }

    /// Move an entity, keeping the chunk index correct.
    ///
    /// Returns whether the entity was alive. The chunk bucket is only
    /// touched when the chunk actually changes: most moves are within one
    /// chunk, and rebucketing every tick would undo the point of having an
    /// index.
    pub fn set_position(&mut self, id: EntityId, pos: (f64, f64, f64)) -> bool {
        let Some(i) = self.slot(id) else {
            return false;
        };
        let from = chunk_of(self.x[i], self.z[i]);
        let to = chunk_of(pos.0, pos.2);
        self.x[i] = pos.0;
        self.y[i] = pos.1;
        self.z[i] = pos.2;
        if from != to {
            let index = u32::try_from(i).expect("fewer than 4 billion entity slots");
            if let Some(bucket) = self.by_chunk.get_mut(&from) {
                bucket.retain(|s| *s != index);
                if bucket.is_empty() {
                    self.by_chunk.remove(&from);
                }
            }
            self.by_chunk.entry(to).or_default().push(index);
        }
        true
    }

    /// An entity's velocity, if it is alive.
    #[must_use]
    pub fn velocity(&self, id: EntityId) -> Option<(f64, f64, f64)> {
        self.slot(id).map(|i| (self.vx[i], self.vy[i], self.vz[i]))
    }

    /// Set an entity's velocity. Returns whether it was alive.
    pub fn set_velocity(&mut self, id: EntityId, v: (f64, f64, f64)) -> bool {
        let Some(i) = self.slot(id) else {
            return false;
        };
        self.vx[i] = v.0;
        self.vy[i] = v.1;
        self.vz[i] = v.2;
        true
    }

    /// An entity's rotation, if it is alive.
    #[must_use]
    pub fn rotation(&self, id: EntityId) -> Option<(f32, f32)> {
        self.slot(id).map(|i| (self.yaw[i], self.pitch[i]))
    }

    /// Set an entity's rotation. Returns whether it was alive.
    pub fn set_rotation(&mut self, id: EntityId, yaw: f32, pitch: f32) -> bool {
        let Some(i) = self.slot(id) else {
            return false;
        };
        self.yaw[i] = yaw;
        self.pitch[i] = pitch;
        true
    }

    /// Every live entity, in `EntityId` order.
    ///
    /// Ordered because a caller that iterates the world and does anything
    /// observable with it must not depend on slot-reuse history. This is not
    /// vanilla's tick order and must not be used as one.
    pub fn iter(&self) -> impl Iterator<Item = EntityId> + '_ {
        let mut ids: Vec<EntityId> = (0..self.alive.len())
            .filter(|i| self.alive[*i])
            .map(|i| {
                pack(
                    u32::try_from(i).expect("fewer than 4 billion entity slots"),
                    self.generation[i],
                )
            })
            .collect();
        ids.sort_by_key(|e| e.0);
        ids.into_iter()
    }
}

impl EntityView for Entities {
    fn entities_near(&self, center: BlockPos, radius: f64) -> Vec<EntityId> {
        let (fx, fy, fz) = (
            f64::from(center.x),
            f64::from(center.y),
            f64::from(center.z),
        );
        // Which chunks the box can touch. `floor` again rather than a cast,
        // for the same reason `chunk_of` uses it.
        #[allow(clippy::cast_possible_truncation)]
        let (min_cx, max_cx) = (
            ((fx - radius).floor() as i32) >> 4,
            ((fx + radius).floor() as i32) >> 4,
        );
        #[allow(clippy::cast_possible_truncation)]
        let (min_cz, max_cz) = (
            ((fz - radius).floor() as i32) >> 4,
            ((fz + radius).floor() as i32) >> 4,
        );

        let mut out = Vec::new();
        for chunk_x in min_cx..=max_cx {
            for chunk_z in min_cz..=max_cz {
                let Some(bucket) = self.by_chunk.get(&(chunk_x, chunk_z)) else {
                    continue;
                };
                for index in bucket {
                    let i = *index as usize;
                    if !self.alive[i] {
                        continue;
                    }
                    if (self.x[i] - fx).abs() <= radius
                        && (self.y[i] - fy).abs() <= radius
                        && (self.z[i] - fz).abs() <= radius
                    {
                        out.push(pack(*index, self.generation[i]));
                    }
                }
            }
        }
        // The frozen contract: deterministic, `EntityId`-sorted. A caller
        // doing anything observable with this order — and target selection
        // does — must not see it change because a slot was reused.
        out.sort_by_key(|e| e.0);
        out
    }

    fn position(&self, id: EntityId) -> Option<(f64, f64, f64)> {
        self.slot(id).map(|i| (self.x[i], self.y[i], self.z[i]))
    }

    fn kind(&self, id: EntityId) -> Option<EntityKindId> {
        self.slot(id).map(|i| self.kind[i])
    }
}

#[cfg(test)]
mod removal_tests {
    use super::{Entities, Spawn};
    use rustmc_data::EntityKindId;

    fn one(e: &mut Entities) -> crate::EntityId {
        e.spawn(Spawn {
            kind: EntityKindId(0),
            pos: (0.0, 0.0, 0.0),
            velocity: (0.0, 0.0, 0.0),
            yaw: 0.0,
            pitch: 0.0,
        })
    }

    /// `isRemoved` is not `despawn`. A removed entity is still there — still
    /// resolvable, still counted, still handed to the tick loop — because
    /// `EntityTickList.forEach` does not skip it and `ServerLevel`'s loop
    /// body is what does.
    #[test]
    fn a_removed_entity_is_still_present() {
        let mut e = Entities::new();
        let id = one(&mut e);
        assert!(!e.is_removed(id));
        assert!(e.mark_removed(id));
        assert!(e.is_removed(id));
        assert_eq!(e.len(), 1, "still occupies its slot");
        assert!(e.velocity(id).is_some(), "still resolvable");
    }

    /// Despawning is the separate, later step —
    /// `PersistentEntitySectionManager.tick`, after the entity loop.
    #[test]
    fn despawning_frees_the_slot_and_the_handle_stops_resolving() {
        let mut e = Entities::new();
        let id = one(&mut e);
        e.mark_removed(id);
        assert!(e.despawn(id));
        assert_eq!(e.len(), 0);
        assert!(e.is_removed(id), "a dead handle reads as removed");
        assert!(!e.mark_removed(id), "and cannot be marked again");
    }

    /// A reused slot must not inherit the previous occupant's flag.
    #[test]
    fn a_reused_slot_starts_unremoved() {
        let mut e = Entities::new();
        let first = one(&mut e);
        e.mark_removed(first);
        e.despawn(first);
        let second = one(&mut e);
        assert!(!e.is_removed(second));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f64, y: f64, z: f64) -> Spawn {
        Spawn {
            kind: EntityKindId(100),
            pos: (x, y, z),
            velocity: (0.0, 0.0, 0.0),
            yaw: 0.0,
            pitch: 0.0,
        }
    }

    #[test]
    fn a_spawned_entity_can_be_read_back() {
        let mut e = Entities::new();
        let id = e.spawn(at(1.5, -60.0, -2.5));
        assert_eq!(e.position(id), Some((1.5, -60.0, -2.5)));
        assert_eq!(e.kind(id), Some(EntityKindId(100)));
        assert_eq!(e.len(), 1);
    }

    #[test]
    fn a_stale_handle_does_not_address_the_slot_that_replaced_it() {
        // The whole reason the generation exists. Without it the second
        // spawn reuses slot 0 and the first id silently starts describing a
        // different entity — which is a use-after-free with no crash.
        let mut e = Entities::new();
        let first = e.spawn(at(0.0, 0.0, 0.0));
        assert!(e.despawn(first));
        let second = e.spawn(at(9.0, 9.0, 9.0));
        assert_ne!(first, second);
        assert_eq!(e.position(first), None);
        assert_eq!(e.position(second), Some((9.0, 9.0, 9.0)));
        assert!(!e.despawn(first));
        assert_eq!(e.len(), 1);
    }

    #[test]
    fn despawning_frees_the_slot_for_reuse() {
        let mut e = Entities::new();
        let a = e.spawn(at(0.0, 0.0, 0.0));
        e.spawn(at(1.0, 0.0, 0.0));
        assert!(e.despawn(a));
        let c = e.spawn(at(2.0, 0.0, 0.0));
        // Reused slot 0, so no third column entry was allocated.
        assert_eq!(unpack(c).0, 0);
        assert_eq!(e.len(), 2);
    }

    #[test]
    fn chunks_are_floored_not_truncated() {
        // `-0.5 as i32` is 0, and 0 >> 4 is chunk 0 — one block of error at
        // every negative chunk border.
        assert_eq!(chunk_of(-0.5, -0.5), (-1, -1));
        assert_eq!(chunk_of(0.0, 0.0), (0, 0));
        assert_eq!(chunk_of(-16.0, 15.9), (-1, 0));
        assert_eq!(chunk_of(16.0, -17.0), (1, -2));
    }

    #[test]
    fn moving_across_a_chunk_border_keeps_the_index_correct() {
        let mut e = Entities::new();
        let id = e.spawn(at(1.0, 0.0, 1.0));
        assert_eq!(
            e.entities_near(BlockPos { x: 1, y: 0, z: 1 }, 2.0),
            vec![id]
        );
        assert!(e.set_position(id, (40.0, 0.0, 1.0)));
        // Gone from where it was, present where it went. A stale bucket
        // entry would make it findable in both, which is how an entity gets
        // ticked twice.
        assert!(e
            .entities_near(BlockPos { x: 1, y: 0, z: 1 }, 2.0)
            .is_empty());
        assert_eq!(
            e.entities_near(BlockPos { x: 40, y: 0, z: 1 }, 2.0),
            vec![id]
        );
    }

    #[test]
    fn a_query_spans_every_chunk_the_box_touches() {
        let mut e = Entities::new();
        let near = e.spawn(at(-1.0, 0.0, -1.0)); // chunk (-1,-1)
        let also = e.spawn(at(1.0, 0.0, 1.0)); // chunk (0,0)
        let far = e.spawn(at(100.0, 0.0, 100.0));
        let found = e.entities_near(BlockPos { x: 0, y: 0, z: 0 }, 4.0);
        assert!(found.contains(&near) && found.contains(&also));
        assert!(!found.contains(&far));
    }

    #[test]
    fn a_query_is_entity_id_sorted() {
        // Frozen contract, and it is behaviour: target selection reads it.
        let mut e = Entities::new();
        let mut ids = Vec::new();
        for i in 0..8 {
            ids.push(e.spawn(at(f64::from(i), 0.0, 0.0)));
        }
        // Free and refill a middle slot so insertion order and id order
        // genuinely disagree.
        assert!(e.despawn(ids[3]));
        let reused = e.spawn(at(3.0, 0.0, 0.0));
        let found = e.entities_near(BlockPos { x: 4, y: 0, z: 0 }, 16.0);
        let mut sorted = found.clone();
        sorted.sort_by_key(|x| x.0);
        assert_eq!(found, sorted);
        assert!(found.contains(&reused));
    }

    #[test]
    fn a_despawned_entity_is_not_found_by_a_query() {
        let mut e = Entities::new();
        let id = e.spawn(at(0.0, 0.0, 0.0));
        assert!(e.despawn(id));
        assert!(e
            .entities_near(BlockPos { x: 0, y: 0, z: 0 }, 8.0)
            .is_empty());
        assert!(e.is_empty());
    }

    #[test]
    fn velocity_and_rotation_round_trip_at_their_own_widths() {
        // Doubles for motion, floats for rotation, exactly as vanilla keeps
        // them: narrowing either would change results.
        let mut e = Entities::new();
        let id = e.spawn(at(0.0, 0.0, 0.0));
        assert!(e.set_velocity(id, (0.1, -0.078_4, 0.3)));
        assert_eq!(e.velocity(id), Some((0.1, -0.078_4, 0.3)));
        assert!(e.set_rotation(id, -90.0, 12.5));
        assert_eq!(e.rotation(id), Some((-90.0, 12.5)));
    }

    #[test]
    fn iteration_is_ordered_and_skips_the_dead() {
        let mut e = Entities::new();
        let a = e.spawn(at(0.0, 0.0, 0.0));
        let b = e.spawn(at(1.0, 0.0, 0.0));
        let c = e.spawn(at(2.0, 0.0, 0.0));
        assert!(e.despawn(b));
        let live: Vec<_> = e.iter().collect();
        assert_eq!(live, vec![a, c]);
    }
}
