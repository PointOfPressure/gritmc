//! What a projectile looks like on the wire.
//!
//! # `ClientboundAddEntityPacket`'s trailing `data` varint is the OWNER'S ID
//!
//! `Projectile.getAddEntityPacket(ServerEntity)`, offsets 0-26:
//!
//! ```text
//!   Entity owner = getOwner();
//!   return new ClientboundAddEntityPacket(this, serverEntity,
//!                                         owner == null ? 0 : owner.getId());
//! ```
//!
//! and `Projectile.recreateFromPacket` reads it straight back:
//! `level.getEntity(packet.getData())`, and calls `setOwner` when that
//! resolves. Every other entity this server announces reaches the
//! two-argument constructor, whose `data` is 0 —
//! [`crate::entities::Arrival::data`] says so — so a projectile is the first
//! thing here for which the field is not a constant.
//!
//! **A zero is not a neutral value.** It is the id the allocator never hands
//! out (`docs/cold-start/entity-id-and-order-keyed.md`: the loop head tests
//! `i == 0` before anything else, so the first id in a session is 1 and 0 is
//! a tripwire), which is exactly why vanilla can use it as "no owner". A port
//! that left `data` at 0 for an owned arrow does not merely lose an
//! attribution: the client's `recreateFromPacket` looks up entity 0, finds
//! nothing, and the arrow renders with no shooter — indistinguishable from a
//! `/summon`ed one.
//!
//! # The five kinds, and why they are not in `entities::KINDS`
//!
//! [`crate::entities::KINDS`] is documented as "every `EntityKind` this
//! server can construct", and it is enumerated by
//! `the_broadcast_walk_assumes_every_live_entity_is_tracked`. **This server
//! constructs none of these yet** — there is no `/summon`, no in-air item use
//! and therefore no caller for `BowItem.releaseUsing` — so putting them in
//! that list would make a test that exists to enumerate live kinds pass by
//! enumerating dead ones. They live here, in their own list, until something
//! spawns one.

use crate::entities::EntityKind;

/// `EntityTypes.ARROW`: `noLootTable().sized(0.5f, 0.5f).eyeHeight(0.13f)
/// .clientTrackingRange(4).updateInterval(20)`, `MobCategory.MISC`.
///
/// Not named in `trackDeltas()`'s false list, so its velocity IS tracked —
/// which is what puts a `set_entity_motion` on the wire for a fired arrow and
/// makes `add_entity`'s `LpVec3` non-zero.
pub const ARROW: EntityKind = EntityKind {
    name: "minecraft:arrow",
    tracking_range_chunks: 4,
    update_interval: 20,
    track_deltas: true,
};

/// `EntityTypes.SNOWBALL`: `noLootTable().sized(0.25f, 0.25f)
/// .clientTrackingRange(4).updateInterval(10)`.
///
/// **Interval 10, not the arrow's 20.** Every `ThrowableItemProjectile` in
/// 26.2 carries the same four values and they are not the arrow's.
pub const SNOWBALL: EntityKind = EntityKind {
    name: "minecraft:snowball",
    tracking_range_chunks: 4,
    update_interval: 10,
    track_deltas: true,
};

/// `EntityTypes.EGG`, same builder as [`SNOWBALL`].
pub const EGG: EntityKind = EntityKind {
    name: "minecraft:egg",
    tracking_range_chunks: 4,
    update_interval: 10,
    track_deltas: true,
};

/// `EntityTypes.ENDER_PEARL`, same builder as [`SNOWBALL`].
pub const ENDER_PEARL: EntityKind = EntityKind {
    name: "minecraft:ender_pearl",
    tracking_range_chunks: 4,
    update_interval: 10,
    track_deltas: true,
};

/// `EntityTypes.SPLASH_POTION`, same builder as [`SNOWBALL`].
pub const SPLASH_POTION: EntityKind = EntityKind {
    name: "minecraft:splash_potion",
    tracking_range_chunks: 4,
    update_interval: 10,
    track_deltas: true,
};

/// `EntityTypes.EXPERIENCE_BOTTLE`, same builder as [`SNOWBALL`].
pub const EXPERIENCE_BOTTLE: EntityKind = EntityKind {
    name: "minecraft:experience_bottle",
    tracking_range_chunks: 4,
    update_interval: 10,
    track_deltas: true,
};

/// The six kinds this module carries, for a test that wants to enumerate them.
pub const PROJECTILE_KINDS: &[EntityKind] = &[
    ARROW,
    SNOWBALL,
    EGG,
    ENDER_PEARL,
    SPLASH_POTION,
    EXPERIENCE_BOTTLE,
];

/// `Projectile.getAddEntityPacket`'s third argument.
#[must_use]
pub const fn add_entity_data(owner: Option<i32>) -> i32 {
    match owner {
        Some(id) => id,
        None => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{add_entity_body, Arrival};

    #[test]
    fn the_data_varint_carries_the_owner_and_zero_means_none() {
        assert_eq!(add_entity_data(Some(7)), 7);
        assert_eq!(add_entity_data(None), 0);
        // 0 is the id the allocator never hands out, which is what makes it
        // usable as a sentinel; a real owner can never be mistaken for one.
        assert_ne!(add_entity_data(Some(1)), add_entity_data(None));
    }

    #[test]
    fn an_owned_arrows_add_entity_ends_in_the_owner_id() {
        let arrival = Arrival {
            id: 12,
            uuid: 0,
            kind_id: 3,
            at: (1.0, 64.0, 2.0),
            movement: (0.5, 0.0, 0.0),
            x_rot: 0,
            y_rot: 0,
            y_head_rot: 0,
            data: add_entity_data(Some(7)),
        };
        let owned = add_entity_body(&arrival).expect("a Vec write cannot fail");
        let unowned = add_entity_body(&Arrival {
            data: add_entity_data(None),
            ..arrival
        })
        .expect("a Vec write cannot fail");
        // The two differ in exactly one byte, the trailing varint.
        assert_eq!(owned.len(), unowned.len());
        assert_eq!(*owned.last().unwrap(), 7);
        assert_eq!(*unowned.last().unwrap(), 0);
        assert_eq!(owned[..owned.len() - 1], unowned[..unowned.len() - 1]);
    }

    #[test]
    fn the_throwables_share_an_interval_and_the_arrow_does_not() {
        for k in PROJECTILE_KINDS {
            assert_eq!(k.tracking_range_chunks, 4, "{}", k.name);
            assert!(k.track_deltas, "{}", k.name);
        }
        assert_eq!(ARROW.update_interval, 20);
        for k in &PROJECTILE_KINDS[1..] {
            assert_eq!(k.update_interval, 10, "{}", k.name);
        }
    }

    /// The list is deliberately NOT `crate::entities::KINDS`. A projectile
    /// kind a spawn path makes (`passive-mobs`' thrown egg) is in both; every
    /// other one must stay out, and this is the assertion that fails loudly
    /// when a dead kind is enumerated as a live one.
    #[test]
    fn only_a_spawned_projectile_kind_is_in_the_servers_constructible_list() {
        for k in PROJECTILE_KINDS {
            let live = crate::entities::KINDS.iter().any(|c| c.name == k.name);
            assert_eq!(
                live,
                k.name == "minecraft:egg",
                "{} constructible: {live}",
                k.name
            );
        }
    }
}
