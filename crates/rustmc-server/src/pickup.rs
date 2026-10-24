//! A player walking into an item: `Player.aiStep`'s touch loop and
//! `ItemEntity.playerTouch`.
//!
//! Reference (26.2), `javap -p -c -constants` on the pinned jar:
//! `Player.aiStep` offsets 77-252, `Player.touch`, `ItemEntity.playerTouch`,
//! `LivingEntity.take`, `Inventory.add` (in [`crate::player::Hotbar::add_mut`]),
//! `Avatar.<clinit>` (the pose dimensions) and `EntityDimensions
//! .makeBoundingBox`.
//!
//! # Where in the tick, and on which thread
//!
//! `ServerGamePacketListenerImpl.tick` → `tickPlayer` → `ServerPlayer.doTick`
//! → `Player.tick` → `aiStep`: the **connection** tick, after every level has
//! ticked — so an item's pickup delay reaches zero in the level tick and the
//! player can take it in the same server tick. This server runs a player's
//! own tick on its connection thread, woken at least once a tick by the
//! one-tick read timeout (`crate::status`); [`touch`] is called from there,
//! under the tick lock, once per server tick that has passed.
//!
//! # What goes on the wire, in order
//!
//! 1. `take_item_entity(item, collector, count)` to everyone **tracking the
//!    item** (`ServerChunkCache.sendToTrackingPlayers(entity, packet)` —
//!    `seenBy`, which includes the collector, who is tracking it). `count` is
//!    the stack's count **before** `Inventory.add` — `playerTouch` reads it
//!    at offset 22 and passes that local, not what moved.
//! 2. `remove_entities` from the discard, when the stack emptied.
//! 3. `container_set_slot` for each inventory slot that changed, from the
//!    **next** `ServerPlayer.tick`'s `containerMenu.broadcastChanges()` — so
//!    it is deferred to the menu sweep, not sent here.
//!
//! **No sound.** The pickup pop is played by the client on receiving
//! `take_item_entity`; neither `playerTouch` nor `take` plays one.
//!
//! # A partial pickup sends nothing at the time, and that is vanilla
//!
//! `Inventory.add` returns `false` when its last pass moved nothing, even if
//! an earlier pass did (see [`crate::player::Hotbar::add_mut`]). So a stack of
//! which only part fits is shrunk in place and the inventory grows, but
//! `playerTouch`'s `if (... && add(stack))` is false: no `take_item_entity`,
//! no discard, and — because the stack was shrunk in place rather than
//! through `setItem` — no `set_entity_data` either. The slots still go out
//! from the next sweep.
//!
//! # Not modelled, named
//!
//! * the pose: `CROUCHING` (0.6 x 1.5) when the shift key is down, else
//!   `STANDING` (0.6 x 1.8). Vanilla's `updatePlayerPose` also forces
//!   `CROUCHING`/`SWIMMING` when the standing box does not fit and never
//!   crouches a flier; neither can happen to a corpus player on open ground;
//! * riding (`isPassenger`'s `minmax` box) — nothing here rides;
//! * health: a corpus player is never dead;
//! * `EXPERIENCE_ORB`s — no such entity exists in this server, so the orb
//!   list and its `Util.getRandom` draw are always empty;
//! * stats and `onItemPickup`'s advancement trigger.

use crate::collide::Aabb;
use crate::entities::{Entities, Entity, ITEM};
use crate::itementity::{self, ItemEntities, INFINITE_PICKUP_DELAY};
use crate::player::Hotbar;
use crate::tracker::{TrackedPlayer, Tracker};

/// `Avatar.STANDING_DIMENSIONS` — `scalable(0.6f, 1.8f)`.
pub const STANDING: (f32, f32) = (0.6, 1.8);
/// `Avatar.POSES`' `CROUCHING` entry — `scalable(0.6f, 1.5f)`.
pub const CROUCHING: (f32, f32) = (0.6, 1.5);

/// `EntityDimensions.makeBoundingBox(x, y, z)`: the half-width is
/// `width / 2.0f` **in float**, then widened, so a player's is
/// `0.30000001192092896` and not `0.3`.
#[must_use]
pub fn make_bounding_box((width, height): (f32, f32), x: f64, y: f64, z: f64) -> Aabb {
    let half = f64::from(width / 2.0);
    Aabb::new(
        x - half,
        y,
        z - half,
        x + half,
        y + f64::from(height),
        z + half,
    )
}

/// A player's box at `e`'s position in the pose its shift key gives.
#[must_use]
pub fn player_box(e: &Entity, crouching: bool) -> Aabb {
    make_bounding_box(if crouching { CROUCHING } else { STANDING }, e.x, e.y, e.z)
}

/// Who is touching: the bits of `Player` that `playerTouch` reads.
pub struct Toucher<'a> {
    /// The player's own entity, for its position and id.
    pub entity: &'a Entity,
    pub uuid: u128,
    pub crouching: bool,
    /// `Player.hasInfiniteMaterials()` — creative.
    pub infinite_materials: bool,
    /// `isSpectator()`, which skips the whole loop.
    pub spectator: bool,
}

/// What a touch pass did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Touched {
    /// `Inventory` indices written, in write order, de-duplicated.
    pub slots: Vec<usize>,
    /// `(item entity id, count)` of each `take_item_entity` sent.
    pub taken: Vec<(i32, i32)>,
}

/// `Player.aiStep` offsets 77-252 — every item the player's box (inflated by
/// `1.0, 0.5, 1.0`) intersects, in the level's section order, touched in turn.
///
/// Packets 1 and 2 of the module docs go out from here; the slot changes are
/// returned for the caller to queue for the menu sweep.
pub fn touch(
    store: &ItemEntities,
    who: &Toucher<'_>,
    inventory: &mut Hotbar,
    entities: &Entities,
    tracker: &Tracker,
    players: &[TrackedPlayer],
) -> Touched {
    let mut out = Touched::default();
    // 77-90: `getHealth() > 0 && !isSpectator()`.
    if who.spectator {
        return out;
    }
    // Every connection runs this every tick, and the entity snapshot below
    // is a clone of the level's whole list; with no item anywhere there is
    // nothing it could find.
    if store.lock().is_empty() {
        return out;
    }
    let reach = itementity::inflate(&player_box(who.entity, who.crouching), 1.0, 0.5, 1.0);
    let all = entities.all();
    let candidates: Vec<i32> = {
        let map = store.lock();
        itementity::in_box(
            &all,
            &reach,
            |e| e.id != who.entity.id,
            |e| (e.kind == ITEM && map.contains_key(&e.id)).then(|| itementity::bounding_box(e)),
        )
        .into_iter()
        .map(|e| e.id)
        .collect()
    };
    for id in candidates {
        // 214-225: `if (!entity.isRemoved()) touch(entity)`.
        player_touch(
            store, id, who, inventory, entities, tracker, players, &mut out,
        );
    }
    out
}

/// `ItemEntity.playerTouch(Player)` — offsets 11-108.
#[allow(clippy::too_many_arguments)]
fn player_touch(
    store: &ItemEntities,
    id: i32,
    who: &Toucher<'_>,
    inventory: &mut Hotbar,
    entities: &Entities,
    tracker: &Tracker,
    players: &[TrackedPlayer],
    out: &mut Touched,
) {
    // The row is changed under the store's lock and the packets go out after
    // it is released, so this thread never holds the item store while it
    // takes the tracker's lock — the clock thread takes them the other way
    // round (`Tracker::tick` drains an item's dirty data).
    let (count, emptied) = {
        let mut map = store.lock();
        let Some(d) = map.get_mut(&id) else {
            return;
        };
        // 22-25: the count is read **before** `add` and is what `take`
        // reports.
        let count = d.stack.count();
        // 27-52: no delay, and no target or this player is the target.
        if d.pickup_delay != 0 || d.target.is_some_and(|t| t != who.uuid) {
            return;
        }
        debug_assert_ne!(d.pickup_delay, INFINITE_PICKUP_DELAY);
        // 55-63: `player.getInventory().add(stack)`, which shrinks the
        // entity's own stack in place.
        let (added, slots) = inventory.add_mut(&mut d.stack, who.infinite_materials);
        for s in slots {
            if !out.slots.contains(&s) {
                out.slots.push(s);
            }
        }
        if !added {
            return;
        }
        let emptied = d.stack.is_empty();
        if emptied {
            map.remove(&id);
        }
        (count, emptied)
    };
    // 66-70: `player.take(this, count)` — `LivingEntity.take`, offsets
    // 0-65: an `ItemEntity` that is not removed gets the packet. It is sent
    // before the discard below, which is the order the bytecode has.
    if let Ok(body) = take_item_entity_body(id, who.entity.id, count) {
        tracker.send_to_tracking_players(
            id,
            players,
            rustmc_proto::ids::play::cb::TAKE_ITEM_ENTITY,
            &body,
        );
    }
    out.taken.push((id, count));
    // 73-87: `if (stack.isEmpty()) { discard(); stack.setCount(count); }` —
    // the restored count belongs to an entity nobody can see any more.
    if emptied {
        entities.remove(id);
        tracker.remove(id, players);
    }
}

/// `ClientboundTakeItemEntityPacket` — three varints: the item, the
/// collector, the amount.
///
/// # Errors
/// Only on a write to the buffer, which cannot fail for a `Vec`.
pub fn take_item_entity_body(item: i32, collector: i32, amount: i32) -> std::io::Result<Vec<u8>> {
    use rustmc_proto::varint::write_varint;
    let mut b = Vec::new();
    write_varint(&mut b, item)?;
    write_varint(&mut b, collector)?;
    write_varint(&mut b, amount)?;
    Ok(b)
}

#[cfg(test)]
mod tests;
