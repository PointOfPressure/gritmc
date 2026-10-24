//! The entity tick loop body, from `ServerLevel`'s own per-entity lambda.
//!
//! [`crate::tick_list`] holds the *order* entities are visited in. This holds
//! what happens to each one, and it is deliberately a trait the caller
//! answers rather than a set of silent assumptions: every gate vanilla
//! consults is a method here, so a gate this server cannot yet evaluate is a
//! method somebody has to write an answer for, not a line quietly missing.
//!
//! # The body, read out of the bytecode
//!
//! ```java
//! if (entity.isRemoved()) return;
//! if (trm.isEntityFrozen(entity)) return;
//! entity.checkDespawn();                                    // MUTATES
//! if (!(entity instanceof ServerPlayer)
//!     && !distanceManager.inEntityTickingRange(entity.chunkPosition().pack()))
//!     return;
//! Entity vehicle = entity.getVehicle();
//! if (vehicle != null) {
//!     if (!vehicle.isRemoved() && vehicle.hasPassenger(entity)) return;
//!     entity.stopRiding();
//! }
//! guardEntityTick(this::tickNonPassenger, entity);
//! ```
//!
//! and the two tick paths are
//!
//! ```java
//! tickNonPassenger(e):  e.setOldPosAndRot(); e.tickCount++; e.tick();
//!                       for (p : e.getPassengers()) tickPassenger(e, p);
//!
//! tickPassenger(v, r):  if (r.isRemoved() || r.getVehicle() != v) { r.stopRiding(); return; }
//!                       if (!(r instanceof Player) && !entityTickList.contains(r)) return;
//!                       r.setOldPosAndRot(); r.tickCount++; r.rideTick();
//!                       for (p : r.getPassengers()) tickPassenger(r, p);
//! ```
//!
//! Six things in that a plausible reimplementation gets wrong — and two of
//! them were got wrong here first, by writing the passenger path from memory
//! before reading it:
//!
//! * **`checkDespawn` runs before the range gate, and it is not a filter — it
//!   mutates.** `Mob.checkDespawn` can `discard()` the entity. So an entity
//!   too far away to tick is still *considered for removal* every tick. A
//!   loop that tested the cheap range gate first, as any sensible
//!   optimisation would, would keep mobs alive that vanilla deletes.
//! * **A player is exempt from the range gate**, by `instanceof ServerPlayer`
//!   and nothing else. Players tick wherever they are.
//! * **A ridden entity returns early and is ticked by its vehicle**, so it is
//!   visited twice per tick and ticked once. If the vehicle has been removed,
//!   or no longer lists it as a passenger, it dismounts and ticks itself.
//! * **`setOldPosAndRot` comes before `tick`**, not after, and passengers come
//!   after their vehicle.
//! * **A passenger gets `rideTick()`, not `tick()`.** They are different
//!   methods. Calling `tick` on a passenger is the single easiest way to
//!   write this loop wrongly, and it was the first thing this file did.
//! * **A non-player passenger that is not in the tick list is skipped.**
//!   `if (!(r instanceof Player) && !entityTickList.contains(r)) return;` —
//!   so a passenger the list does not hold is not ticked by its vehicle
//!   either, and a player passenger is exempt from that. `tickCount` is
//!   incremented **by the loop**, in both paths, not by `tick` itself.
//!
//! `guardEntityTick` is a `try`/`catch` that builds a crash report and
//! **rethrows** as `ReportedException`. It does not swallow. A throwing entity
//! stops the server rather than being skipped, so this loop propagates a panic
//! rather than catching it — see [`crate::tick_list`].
//!
//! # What this server can answer today
//!
//! `isRemoved` exists. `isEntityFrozen` is constant-false with no tick-rate
//! manager, which is a gate *satisfied*. **`checkDespawn` is a no-op for every
//! entity whose class does not override it** — and `Entity.checkDespawn()` is
//! a bare `return`, so the only overriders in the whole jar are `Mob`,
//! `ShulkerBullet`, `WitherBoss` and `EnderDragon`. Until 2026-09-05 there
//! were no mobs in this server and the mutation was unreachable; the pig and
//! the bat in `rustmc-server`'s `mob` module now run `Mob.checkDespawn`
//! through their own loop body (`mob::tick_all`), which reproduces this
//! one's order for mobs and reads [`MobCategory`]'s constants from here.

use crate::EntityId;

/// `net.minecraft.world.entity.MobCategory`, for the two distances
/// `Mob.checkDespawn` reads.
///
/// Recorded now because they are cheap to read and easy to get wrong later:
/// the despawn distance is a **per-category field**, and the no-despawn
/// distance is a **constant 32** on the enum rather than a field at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MobCategory {
    Monster,
    Creature,
    Ambient,
    Axolotls,
    UndergroundWaterCreature,
    WaterCreature,
    WaterAmbient,
    Misc,
}

impl MobCategory {
    /// `getDespawnDistance()` — the constructor's fifth argument. 128 for
    /// every category except `water_ambient`, which is 64.
    #[must_use]
    pub const fn despawn_distance(self) -> i32 {
        match self {
            Self::WaterAmbient => 64,
            _ => 128,
        }
    }

    /// `getNoDespawnDistance()` — `bipush 32; ireturn`. A constant, the same
    /// for every category, and **not** the field above.
    #[must_use]
    pub const fn no_despawn_distance(self) -> i32 {
        32
    }
}

/// Everything `ServerLevel`'s per-entity body asks about an entity.
///
/// A trait rather than a struct so that a gate this server cannot yet
/// evaluate has to be answered explicitly by whoever calls the loop.
pub trait EntityWorld {
    /// `Entity.isRemoved()`.
    fn is_removed(&self, id: EntityId) -> bool;

    /// `TickRateManager.isEntityFrozen(entity)`:
    /// `!runsNormally() && !(entity instanceof Player) && no player passengers`.
    /// Constant `false` while there is no tick-rate manager.
    fn is_frozen(&self, _id: EntityId) -> bool {
        false
    }

    /// `Entity.checkDespawn()`. **Mutating**, and the base implementation is
    /// a bare `return`, so the default here is a no-op for the same reason
    /// and not as a shortcut.
    fn check_despawn(&mut self, _id: EntityId) {}

    /// `entity instanceof ServerPlayer` — the only thing that exempts an
    /// entity from the ticking-range gate.
    fn is_player(&self, id: EntityId) -> bool;

    /// `DistanceManager.inEntityTickingRange(entity.chunkPosition().pack())`.
    fn in_ticking_range(&self, id: EntityId) -> bool;

    /// `Entity.getVehicle()`.
    fn vehicle(&self, id: EntityId) -> Option<EntityId>;

    /// `Entity.hasPassenger(other)`.
    fn has_passenger(&self, vehicle: EntityId, rider: EntityId) -> bool;

    /// `Entity.stopRiding()`.
    fn stop_riding(&mut self, id: EntityId);

    /// `Entity.getPassengers()`, in list order.
    fn passengers(&self, id: EntityId) -> Vec<EntityId>;

    /// `Entity.setOldPosAndRot()`.
    fn set_old_pos_and_rot(&mut self, id: EntityId);

    /// `entity.tickCount++`, which both tick paths do themselves rather than
    /// leaving to `tick`.
    fn increment_tick_count(&mut self, id: EntityId);

    /// `EntityTickList.contains(entity)`. Consulted only for a **non-player
    /// passenger**, which its vehicle will not tick if the list does not hold
    /// it.
    fn in_tick_list(&self, id: EntityId) -> bool;

    /// `Entity.tick()` — the non-passenger path.
    fn tick(&mut self, id: EntityId);

    /// `Entity.rideTick()` — the passenger path, and a **different method**.
    fn ride_tick(&mut self, id: EntityId);
}

/// One entity's turn: `ServerLevel`'s `forEach` body, gate for gate.
pub fn tick_one(world: &mut impl EntityWorld, id: EntityId) {
    if world.is_removed(id) {
        return;
    }
    if world.is_frozen(id) {
        return;
    }
    // Before the range gate, and it can remove the entity. Reordering these
    // two for speed would keep mobs alive that vanilla deletes.
    world.check_despawn(id);
    if !world.is_player(id) && !world.in_ticking_range(id) {
        return;
    }
    if let Some(vehicle) = world.vehicle(id) {
        if !world.is_removed(vehicle) && world.has_passenger(vehicle, id) {
            // Its vehicle will tick it.
            return;
        }
        world.stop_riding(id);
    }
    tick_non_passenger(world, id);
}

/// `ServerLevel.tickNonPassenger`.
pub fn tick_non_passenger(world: &mut impl EntityWorld, id: EntityId) {
    world.set_old_pos_and_rot(id);
    world.increment_tick_count(id);
    world.tick(id);
    for rider in world.passengers(id) {
        tick_passenger(world, id, rider);
    }
}

/// `ServerLevel.tickPassenger`, recursively.
fn tick_passenger(world: &mut impl EntityWorld, vehicle: EntityId, rider: EntityId) {
    if world.is_removed(rider) || world.vehicle(rider) != Some(vehicle) {
        world.stop_riding(rider);
        return;
    }
    // A non-player passenger the tick list does not hold is not ticked by its
    // vehicle either.
    if !world.is_player(rider) && !world.in_tick_list(rider) {
        return;
    }
    world.set_old_pos_and_rot(rider);
    world.increment_tick_count(rider);
    // `rideTick`, not `tick`.
    world.ride_tick(rider);
    for inner in world.passengers(rider) {
        tick_passenger(world, rider, inner);
    }
}

#[cfg(test)]
mod tests {
    use super::{tick_one, EntityWorld, MobCategory};
    use crate::EntityId;
    use std::collections::BTreeMap;

    fn e(n: u64) -> EntityId {
        EntityId(n)
    }

    /// Records what the loop asked for, in order, so the gate order itself is
    /// the assertion.
    #[derive(Default)]
    struct Fake {
        removed: Vec<EntityId>,
        frozen: Vec<EntityId>,
        players: Vec<EntityId>,
        out_of_range: Vec<EntityId>,
        riding: BTreeMap<u64, EntityId>,
        riders: BTreeMap<u64, Vec<EntityId>>,
        /// Everything is in the tick list unless named here.
        not_listed: Vec<EntityId>,
        log: Vec<String>,
    }

    impl EntityWorld for Fake {
        fn is_removed(&self, id: EntityId) -> bool {
            self.removed.contains(&id)
        }
        fn is_frozen(&self, id: EntityId) -> bool {
            self.frozen.contains(&id)
        }
        fn check_despawn(&mut self, id: EntityId) {
            self.log.push(format!("checkDespawn {}", id.0));
        }
        fn is_player(&self, id: EntityId) -> bool {
            self.players.contains(&id)
        }
        fn in_ticking_range(&self, id: EntityId) -> bool {
            !self.out_of_range.contains(&id)
        }
        fn vehicle(&self, id: EntityId) -> Option<EntityId> {
            self.riding.get(&id.0).copied()
        }
        fn has_passenger(&self, vehicle: EntityId, rider: EntityId) -> bool {
            self.riders
                .get(&vehicle.0)
                .is_some_and(|v| v.contains(&rider))
        }
        fn stop_riding(&mut self, id: EntityId) {
            self.log.push(format!("stopRiding {}", id.0));
            self.riding.remove(&id.0);
        }
        fn passengers(&self, id: EntityId) -> Vec<EntityId> {
            self.riders.get(&id.0).cloned().unwrap_or_default()
        }
        fn set_old_pos_and_rot(&mut self, id: EntityId) {
            self.log.push(format!("setOldPosAndRot {}", id.0));
        }
        fn increment_tick_count(&mut self, id: EntityId) {
            self.log.push(format!("tickCount++ {}", id.0));
        }
        fn in_tick_list(&self, id: EntityId) -> bool {
            !self.not_listed.contains(&id)
        }
        fn tick(&mut self, id: EntityId) {
            self.log.push(format!("tick {}", id.0));
        }
        fn ride_tick(&mut self, id: EntityId) {
            self.log.push(format!("rideTick {}", id.0));
        }
    }

    #[test]
    fn a_removed_entity_is_not_even_considered_for_despawn() {
        let mut w = Fake {
            removed: vec![e(1)],
            ..Fake::default()
        };
        tick_one(&mut w, e(1));
        assert!(w.log.is_empty());
    }

    #[test]
    fn a_frozen_entity_stops_before_check_despawn() {
        let mut w = Fake {
            frozen: vec![e(1)],
            ..Fake::default()
        };
        tick_one(&mut w, e(1));
        assert!(w.log.is_empty(), "isEntityFrozen precedes checkDespawn");
    }

    /// The one that matters: `checkDespawn` runs **before** the range gate,
    /// so an entity too far away to tick is still considered for removal.
    /// Testing the cheap gate first is the obvious optimisation and it is
    /// wrong.
    #[test]
    fn an_out_of_range_entity_is_still_offered_to_check_despawn() {
        let mut w = Fake {
            out_of_range: vec![e(1)],
            ..Fake::default()
        };
        tick_one(&mut w, e(1));
        assert_eq!(w.log, vec!["checkDespawn 1".to_string()]);
    }

    /// A player is exempt from the range gate by `instanceof ServerPlayer`.
    #[test]
    fn a_player_ticks_out_of_range() {
        let mut w = Fake {
            out_of_range: vec![e(1)],
            players: vec![e(1)],
            ..Fake::default()
        };
        tick_one(&mut w, e(1));
        assert_eq!(
            w.log,
            vec![
                "checkDespawn 1".to_string(),
                "setOldPosAndRot 1".into(),
                "tickCount++ 1".into(),
                "tick 1".into()
            ]
        );
    }

    #[test]
    fn set_old_pos_and_rot_comes_before_tick() {
        let mut w = Fake::default();
        tick_one(&mut w, e(1));
        assert_eq!(
            w.log,
            vec![
                "checkDespawn 1".to_string(),
                "setOldPosAndRot 1".into(),
                "tickCount++ 1".into(),
                "tick 1".into()
            ]
        );
    }

    /// A rider whose vehicle still holds it returns early — its vehicle ticks
    /// it — so it is visited twice in a tick and ticked once.
    #[test]
    fn a_ridden_entity_is_ticked_by_its_vehicle_and_not_on_its_own() {
        let mut w = Fake::default();
        w.riding.insert(2, e(1));
        w.riders.insert(1, vec![e(2)]);

        tick_one(&mut w, e(2));
        assert_eq!(
            w.log,
            vec!["checkDespawn 2".to_string()],
            "the rider stops after checkDespawn"
        );

        w.log.clear();
        tick_one(&mut w, e(1));
        assert_eq!(
            w.log,
            vec![
                "checkDespawn 1".to_string(),
                "setOldPosAndRot 1".into(),
                "tickCount++ 1".into(),
                "tick 1".into(),
                "setOldPosAndRot 2".into(),
                "tickCount++ 2".into(),
                "rideTick 2".into(),
            ],
            "the passenger ticks after its vehicle, and gets rideTick"
        );
    }

    /// If the vehicle has gone, the rider dismounts and ticks itself.
    #[test]
    fn a_rider_whose_vehicle_was_removed_dismounts_and_ticks() {
        let mut w = Fake {
            removed: vec![e(1)],
            ..Fake::default()
        };
        w.riding.insert(2, e(1));
        w.riders.insert(1, vec![e(2)]);
        tick_one(&mut w, e(2));
        assert_eq!(
            w.log,
            vec![
                "checkDespawn 2".to_string(),
                "stopRiding 2".into(),
                "setOldPosAndRot 2".into(),
                "tickCount++ 2".into(),
                "tick 2".into(),
            ]
        );
    }

    /// A non-player passenger the tick list does not hold is skipped by its
    /// vehicle. A player passenger is exempt.
    #[test]
    fn a_passenger_missing_from_the_tick_list_is_skipped() {
        let mut w = Fake {
            not_listed: vec![e(2)],
            ..Fake::default()
        };
        w.riding.insert(2, e(1));
        w.riders.insert(1, vec![e(2)]);
        tick_one(&mut w, e(1));
        assert_eq!(
            w.log,
            vec![
                "checkDespawn 1".to_string(),
                "setOldPosAndRot 1".into(),
                "tickCount++ 1".into(),
                "tick 1".into(),
            ],
            "the vehicle ticks, the unlisted passenger does not"
        );

        let mut w = Fake {
            not_listed: vec![e(2)],
            players: vec![e(2)],
            ..Fake::default()
        };
        w.riding.insert(2, e(1));
        w.riders.insert(1, vec![e(2)]);
        tick_one(&mut w, e(1));
        assert!(
            w.log.contains(&"rideTick 2".to_string()),
            "a player passenger is exempt from the tick-list check"
        );
    }

    /// The despawn distance is a per-category field and the no-despawn
    /// distance is a constant — not two fields, which is the easy mistake.
    #[test]
    fn mob_category_distances_are_the_measured_ones() {
        assert_eq!(MobCategory::Monster.despawn_distance(), 128);
        assert_eq!(MobCategory::Creature.despawn_distance(), 128);
        assert_eq!(MobCategory::Misc.despawn_distance(), 128);
        assert_eq!(
            MobCategory::WaterAmbient.despawn_distance(),
            64,
            "the only category that differs"
        );
        for c in [
            MobCategory::Monster,
            MobCategory::WaterAmbient,
            MobCategory::Misc,
        ] {
            assert_eq!(c.no_despawn_distance(), 32);
        }
    }
}
