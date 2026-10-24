//! `net.minecraft.world.level.entity.EntityTickList` — the order entities
//! tick in, and nothing else.
//!
//! # Why this is a separate thing from the store
//!
//! [`crate::store`] deliberately has no tick loop: vanilla's tick order is
//! observable — target selection, item merging and collision resolution all
//! depend on which entity moved first — so it has to come from the bytecode
//! rather than from a plausible guess. This is that reading, kept apart from
//! the storage layout because it is a *different* order from the one the
//! arena happens to hold entities in, and conflating the two is precisely
//! the mistake worth not making.
//!
//! # What the bytecode says
//!
//! `EntityTickList` holds two maps, `active` and `passive`, both
//! `Int2ObjectLinkedOpenHashMap` keyed by entity id. Linked, so iteration is
//! **insertion order** — not id order, and not the order the storage arena
//! grew in. `forEach` walks `active.values()`.
//!
//! `add` and `remove` both begin with `ensureActiveIsNotIterated`:
//!
//! ```java
//! private void ensureActiveIsNotIterated() {
//!     if (this.iterated == this.active) {
//!         this.passive.clear();
//!         for (Entry<Entity> e : Int2ObjectMaps.fastIterable(this.active))
//!             this.passive.put(e.getIntKey(), e.getValue());
//!         Int2ObjectMap<Entity> tmp = this.active;
//!         this.active = this.passive;
//!         this.passive = tmp;
//!     }
//! }
//! ```
//!
//! So while a `forEach` is running, a mutation copies the whole list, swaps
//! the maps, and lands on the copy — and the walk in progress keeps reading
//! the map it started on. Two consequences, both observable and both easy to
//! get wrong:
//!
//! * **An entity added during a tick does not tick in that tick.** It is in
//!   the new `active`, and the walk is on the old one.
//! * **An entity removed during a tick is still visited by that tick.** The
//!   list does not skip it; `ServerLevel`'s own loop body does, with an
//!   `entity.isRemoved()` check as its very first statement. A list that
//!   dropped it instead would look identical for `Entity.tick` and differ
//!   for anything else that loop body does first.
//!
//! Rust expresses the same semantics by snapshotting the order and walking
//! the snapshot, which is what the Java dance exists to simulate.
//!
//! `add` is `active.put(id, entity)`, and a linked map's `put` on a key it
//! already holds **updates the value without moving it** — so re-adding an
//! entity leaves it where it was, while removing and re-adding sends it to
//! the back.
//!
//! # `guardEntityTick` rethrows — it does not swallow
//!
//! `Level.guardEntityTick(Consumer<T>, T)` is a `try`/`catch` that builds a
//! crash report — `CrashReport.forThrowable(t, "Ticking entity")`, then
//! `addCategory("Entity being ticked")` and `entity.fillCrashReportCategory`
//! — and **throws `ReportedException`**. It does not continue to the next
//! entity. A reimplementation that caught and carried on would survive a
//! throwing entity where vanilla stops, which is a difference a player sees
//! as the server staying up.
//!
//! # What is not here yet
//!
//! The loop body. `ServerLevel.tick`'s per-entity gate, in order, is
//! `isRemoved`, `isEntityFrozen`, `checkDespawn`, `inEntityTickingRange`,
//! then the vehicle check, then `guardEntityTick(this::tickNonPassenger)` —
//! and `tickNonPassenger` is `setOldPosAndRot()`, then `tick()`, then each
//! of `getPassengers()` in list order, recursively. None of `isRemoved`,
//! `checkDespawn` or the ticking range exists in this crate yet, so the loop
//! body would be a guess where the order is not.
//!
//! The gate audit, so the next person does not have to redo it:
//!
//! | gate | status |
//! |---|---|
//! | `isRemoved` | **exists** — [`crate::store::Entities::mark_removed`] |
//! | `isEntityFrozen` | **provably false**, see below |
//! | `checkDespawn` | missing, and it *mutates* — it can discard a mob |
//! | `inEntityTickingRange` | missing, needs chunk tickets |
//!
//! `TickRateManager.isEntityFrozen` is
//! `!runsNormally() && !(entity instanceof Player) && no player passengers`.
//! With no tick-rate manager in this server `runsNormally()` is true and the
//! gate is constant-false — a gate **satisfied**, not a gate skipped, and the
//! distinction is the reason to write it down rather than leave it implied.
//!
//! So two of the four are available and the body still cannot be written,
//! because `checkDespawn` is not a filter: it can remove the entity, and a
//! loop that omits it ticks mobs vanilla would have discarded.
//!
//! **`isRemoved` now does** —
//! see [`crate::store::Entities::mark_removed`], which is deliberately not
//! `despawn`: a removed entity stays in its slot and is still handed to this
//! list, and the slot is reclaimed later by
//! `PersistentEntitySectionManager.tick`, which `ServerLevel.tick` runs after
//! the entity loop.

use crate::EntityId;

/// The tick list: membership plus the order.
///
/// `Vec` for the order and a linear scan for membership, which is honest
/// about what it is. Vanilla's structure is a hash map with a linked list
/// through it; matching that is an optimisation, and an optimisation here
/// owes a proof that it is output-identical before it lands.
#[derive(Debug, Default, Clone)]
pub struct EntityTickList {
    order: Vec<EntityId>,
}

impl EntityTickList {
    #[must_use]
    pub const fn new() -> Self {
        Self { order: Vec::new() }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.order.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    #[must_use]
    pub fn contains(&self, id: EntityId) -> bool {
        self.order.contains(&id)
    }

    /// `add`. An id already present keeps its place, because a linked map's
    /// `put` updates in place rather than reinserting.
    pub fn add(&mut self, id: EntityId) {
        if !self.contains(id) {
            self.order.push(id);
        }
    }

    /// `remove`.
    pub fn remove(&mut self, id: EntityId) {
        self.order.retain(|&e| e != id);
    }

    /// `forEach`.
    ///
    /// The callback receives `&mut Self` so it can add and remove during the
    /// walk, exactly as vanilla's does — and, exactly as in vanilla, neither
    /// changes what this walk visits.
    pub fn for_each(&mut self, mut f: impl FnMut(&mut Self, EntityId)) {
        // The snapshot *is* `ensureActiveIsNotIterated`: vanilla copies the
        // list and swaps so the walk keeps reading the map it started on.
        let snapshot = self.order.clone();
        for id in snapshot {
            f(self, id);
        }
    }

    /// The current order, for tests and for anything that needs to assert on
    /// it rather than walk it.
    #[must_use]
    pub fn order(&self) -> &[EntityId] {
        &self.order
    }
}

#[cfg(test)]
mod tests {
    use super::EntityTickList;
    use crate::EntityId;

    fn e(n: u64) -> EntityId {
        EntityId(n)
    }

    /// The headline: insertion order, not id order. An arena that reused a
    /// low slot would tick that entity first if this sorted, and vanilla
    /// does not.
    #[test]
    fn the_order_is_insertion_order_not_id_order() {
        let mut l = EntityTickList::new();
        for id in [e(7), e(2), e(9), e(1)] {
            l.add(id);
        }
        assert_eq!(l.order(), &[e(7), e(2), e(9), e(1)]);
    }

    /// `put` on a key a linked map already holds updates in place.
    #[test]
    fn re_adding_does_not_move_an_entity() {
        let mut l = EntityTickList::new();
        for id in [e(1), e(2), e(3)] {
            l.add(id);
        }
        l.add(e(1));
        assert_eq!(l.order(), &[e(1), e(2), e(3)]);
        assert_eq!(l.len(), 3);
    }

    /// Removing and re-adding does move it, and the difference between this
    /// and the test above is the whole reason both exist.
    #[test]
    fn removing_then_re_adding_sends_it_to_the_back() {
        let mut l = EntityTickList::new();
        for id in [e(1), e(2), e(3)] {
            l.add(id);
        }
        l.remove(e(1));
        l.add(e(1));
        assert_eq!(l.order(), &[e(2), e(3), e(1)]);
    }

    /// **The three golden vectors, measured rather than read.**
    ///
    /// The three tests above assert semantics this file had only ever stated
    /// in prose: that `put` on a present key leaves it alone, that a `remove`
    /// then `add` moves it to the back, and that
    /// `ensureActiveIsNotIterated`'s copy preserves order. All three are the
    /// kind of thing a reasonable person gets backwards, and reading the
    /// bytecode does not settle them — they are properties of
    /// `Int2ObjectLinkedOpenHashMap`, not of `EntityTickList`.
    ///
    /// So they were measured, against the fastutil the pinned jar itself
    /// ships with, by `ci/probes/LinkedOrder.java`. Three runs, identical:
    ///
    /// ```text
    /// baseline      1 2 3 4
    /// put-existing  1 2 3 4      -- key 2 re-put; it did NOT move
    /// remove-readd  1 3 4 2      -- key 2 removed then re-put; it DID
    /// copy-source   4 1 3 2
    /// copy-result   4 1 3 2      -- fastIterable + put preserves order
    /// ```
    ///
    /// This test asserts those four lines against ours. It overlaps the three
    /// above on purpose: they say what we do, this says what vanilla's
    /// container does, and the day the two stop agreeing it should be this
    /// one that fails.
    #[test]
    fn matches_the_measured_linked_map_vectors() {
        let base = [e(1), e(2), e(3), e(4)];

        let mut l = EntityTickList::new();
        for id in base {
            l.add(id);
        }
        assert_eq!(l.order(), &base, "baseline 1 2 3 4");

        let mut l = EntityTickList::new();
        for id in base {
            l.add(id);
        }
        l.add(e(2));
        assert_eq!(l.order(), &[e(1), e(2), e(3), e(4)], "put-existing 1 2 3 4");

        let mut l = EntityTickList::new();
        for id in base {
            l.add(id);
        }
        l.remove(e(2));
        l.add(e(2));
        assert_eq!(l.order(), &[e(1), e(3), e(4), e(2)], "remove-readd 1 3 4 2");

        // copy-source / copy-result. Ours copies in `for_each`; the assertion
        // is that walking it yields the source order unchanged.
        let mut l = EntityTickList::new();
        for id in [e(4), e(1), e(3), e(2)] {
            l.add(id);
        }
        let mut seen = Vec::new();
        l.for_each(|_, id| seen.push(id));
        assert_eq!(seen, vec![e(4), e(1), e(3), e(2)], "copy-result 4 1 3 2");
        assert_eq!(l.order(), &[e(4), e(1), e(3), e(2)], "copy-source 4 1 3 2");
    }

    /// An entity spawned mid-tick waits for the next one.
    #[test]
    fn an_entity_added_during_a_walk_is_not_visited_by_it() {
        let mut l = EntityTickList::new();
        l.add(e(1));
        l.add(e(2));
        let mut seen = Vec::new();
        l.for_each(|list, id| {
            seen.push(id);
            if id == e(1) {
                list.add(e(99));
            }
        });
        assert_eq!(seen, vec![e(1), e(2)], "99 waits for the next tick");
        assert_eq!(l.order(), &[e(1), e(2), e(99)], "but it is in the list");
    }

    /// And the mirror image, which is the one a plausible implementation
    /// gets wrong: an entity removed mid-tick is **still visited**. Vanilla's
    /// loop body opens with `entity.isRemoved()` precisely because the list
    /// hands it over anyway.
    #[test]
    fn an_entity_removed_during_a_walk_is_still_visited_by_it() {
        let mut l = EntityTickList::new();
        for id in [e(1), e(2), e(3)] {
            l.add(id);
        }
        let mut seen = Vec::new();
        l.for_each(|list, id| {
            seen.push(id);
            if id == e(1) {
                list.remove(e(3));
            }
        });
        assert_eq!(seen, vec![e(1), e(2), e(3)]);
        assert_eq!(l.order(), &[e(1), e(2)], "gone from the next tick");
    }
}
