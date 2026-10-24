use super::*;
use crate::item::ItemStack;
use crate::itementity::ItemData;
use crate::players::Connection;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct Recorder(Arc<Mutex<Vec<u8>>>);
impl std::io::Write for Recorder {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("recorder").extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Recorder {
    /// `(packet id, body)` of every frame written.
    fn frames(&self) -> Vec<(i32, Vec<u8>)> {
        let bytes = self.0.lock().expect("recorder").clone();
        let mut cur = &bytes[..];
        let mut out = Vec::new();
        while let Ok(f) = rustmc_proto::frame::read_frame(&mut cur, false) {
            out.push(f);
        }
        out
    }
}

fn at(id: i32, kind: crate::entities::EntityKind, x: f64, y: f64, z: f64) -> Entity {
    Entity {
        id,
        uuid: u128::try_from(id).unwrap_or(0),
        kind,
        x,
        y,
        z,
        y_rot: 0.0,
        x_rot: 0.0,
        y_head_rot: 0.0,
        movement: (0.0, 0.0, 0.0),
        on_ground: true,
        data: 0,
        swinging: false,
        swing_time: 0,
        swinging_arm: None,
        synched: crate::synched::Synched::EntityOnly,
    }
}

fn kinds() -> crate::entities::KindIds {
    let mut m = std::collections::BTreeMap::new();
    m.insert("minecraft:player".to_string(), 156);
    m.insert("minecraft:item".to_string(), 71);
    crate::entities::KindIds::resolve(&m, &[crate::entities::PLAYER, ITEM])
}

struct World {
    store: ItemEntities,
    entities: Entities,
    tracker: Tracker,
    rec: Recorder,
    players: Vec<TrackedPlayer>,
    me: Entity,
}

/// A player at the origin of a floor at y = 0, tracking whatever is added.
fn world() -> World {
    let entities = Entities::new();
    let rec = Recorder::default();
    let conn = Arc::new(Connection::with_entity(
        1,
        "picker",
        Box::new(rec.clone()),
        None,
        entities.next_id(),
        32,
        (0, 0),
    ));
    // The join's two chunk steps (`mob_in_world.rs` does the same): since
    // entity-tracker, `updatePlayer` asks `isChunkTracked`, so a view that
    // has been sent nothing tracks no item.
    conn.with_chunk_view(|v| {
        for dx in -4..=4 {
            for dz in -4..=4 {
                v.mark_sent((dx, dz));
            }
        }
        v.update_tracking((0, 0));
    });
    let me = at(conn.entity_id(), crate::entities::PLAYER, 0.5, 0.0, 0.5);
    entities.insert(me.clone());
    let players = vec![TrackedPlayer {
        conn,
        entity_id: me.id,
        pos: (me.x, me.y, me.z),
        chunk: me.chunk(),
        view_distance: 32,
    }];
    World {
        store: ItemEntities::default(),
        entities,
        tracker: Tracker::new(),
        rec,
        players,
        me,
    }
}

impl World {
    fn drop_item(&self, x: f64, z: f64, stack: ItemStack, delay: i32) -> i32 {
        let e = at(self.entities.next_id(), ITEM, x, 0.0, z);
        self.entities.insert(e.clone());
        let mut d = ItemData::new(stack, Some(1));
        d.pickup_delay = delay;
        self.store.insert(e.id, d);
        self.tracker.add(&e, &self.players, &kinds());
        e.id
    }

    fn touch(&self, inv: &mut Hotbar, creative: bool) -> Touched {
        let who = Toucher {
            entity: &self.me,
            uuid: self.me.uuid,
            crouching: false,
            infinite_materials: creative,
            spectator: false,
        };
        touch(
            &self.store,
            &who,
            inv,
            &self.entities,
            &self.tracker,
            &self.players,
        )
    }
}

/// The whole pickup: `take_item_entity(item, collector, 1)` then
/// `remove_entities`, in that order, the stack lands in hotbar slot 0, and
/// the item is gone from the store and the level.
#[test]
fn an_item_at_the_feet_is_taken_and_removed_in_vanillas_order() {
    let w = world();
    let id = w.drop_item(0.5, 0.5, ItemStack::new("minecraft:stone", 1), 0);
    w.rec.0.lock().expect("rec").clear();
    let mut inv = Hotbar::new();
    let t = w.touch(&mut inv, false);
    assert_eq!(t.taken, vec![(id, 1)]);
    assert_eq!(t.slots, vec![0]);
    assert_eq!(inv.stack(0), ItemStack::new("minecraft:stone", 1));
    assert!(w.store.get(id).is_none());
    assert!(w.entities.get(id).is_none());
    let frames = w.rec.frames();
    let kinds: Vec<i32> = frames.iter().map(|f| f.0).collect();
    assert_eq!(
        kinds,
        vec![
            rustmc_proto::ids::play::cb::TAKE_ITEM_ENTITY,
            rustmc_proto::ids::play::cb::REMOVE_ENTITIES
        ]
    );
    assert_eq!(
        frames[0].1,
        take_item_entity_body(id, w.me.id, 1).expect("body")
    );
}

/// A pickup delay of 1 is not 0: nothing happens.
#[test]
fn a_delay_of_one_tick_still_refuses() {
    let w = world();
    let id = w.drop_item(0.5, 0.5, ItemStack::new("minecraft:stone", 1), 1);
    let mut inv = Hotbar::new();
    assert_eq!(w.touch(&mut inv, false), Touched::default());
    assert!(w.store.get(id).is_some());
}

/// The reach is the box inflated by `1.0, 0.5, 1.0`: an item 1.4 blocks
/// from the player's centre (0.3 + 1.0 + 0.125 = 1.425) is taken, one at
/// 1.5 is not.
#[test]
fn the_reach_is_the_box_inflated_by_one() {
    let w = world();
    let near = w.drop_item(0.5 + 1.4, 0.5, ItemStack::new("minecraft:stone", 1), 0);
    let far = w.drop_item(0.5, 0.5 + 1.5, ItemStack::new("minecraft:dirt", 1), 0);
    let mut inv = Hotbar::new();
    let t = w.touch(&mut inv, false);
    assert_eq!(t.taken, vec![(near, 1)]);
    assert!(w.store.get(far).is_some());
}

/// A survival player with no room takes nothing and nothing is sent.
#[test]
fn a_full_survival_inventory_takes_nothing() {
    let w = world();
    let mut inv = Hotbar::new();
    for i in 0..crate::player::INVENTORY_SIZE {
        inv.set_stack(i, ItemStack::new("minecraft:dirt", 64));
    }
    inv.set_offhand(ItemStack::new("minecraft:dirt", 64));
    let id = w.drop_item(0.5, 0.5, ItemStack::new("minecraft:stone", 1), 0);
    w.rec.0.lock().expect("rec").clear();
    let t = w.touch(&mut inv, false);
    assert_eq!(t, Touched::default());
    assert!(w.rec.frames().is_empty());
    assert_eq!(w.store.get(id).expect("row").stack.count(), 1);
}

/// The creative tail of `Inventory.add`: nothing fits, `hasInfiniteMaterials`,
/// so the stack is zeroed and the answer is `true` — the item is taken (with
/// its full count) and destroyed, and no slot changes.
#[test]
fn a_full_creative_inventory_takes_and_destroys() {
    let w = world();
    let mut inv = Hotbar::new();
    for i in 0..crate::player::INVENTORY_SIZE {
        inv.set_stack(i, ItemStack::new("minecraft:dirt", 1));
    }
    let id = w.drop_item(0.5, 0.5, ItemStack::new("minecraft:stone", 3), 0);
    let t = w.touch(&mut inv, true);
    assert_eq!(t.taken, vec![(id, 3)]);
    assert!(t.slots.is_empty());
    assert!(w.store.get(id).is_none());
}

/// The partial pickup: 10 arrive, 4 fit. `Inventory.add`'s last pass moves
/// nothing, so it answers `false` — no `take_item_entity`, no discard — but
/// the four are in the inventory and the entity holds six.
#[test]
fn a_partial_survival_pickup_sends_no_take_and_keeps_the_rest() {
    let w = world();
    let mut inv = Hotbar::new();
    for i in 0..crate::player::INVENTORY_SIZE {
        inv.set_stack(i, ItemStack::new("minecraft:dirt", 64));
    }
    inv.set_stack(5, ItemStack::new("minecraft:stone", 60));
    let id = w.drop_item(0.5, 0.5, ItemStack::new("minecraft:stone", 10), 0);
    w.rec.0.lock().expect("rec").clear();
    let t = w.touch(&mut inv, false);
    assert!(t.taken.is_empty());
    assert_eq!(t.slots, vec![5]);
    assert_eq!(inv.stack(5).count(), 64);
    assert_eq!(w.store.get(id).expect("row").stack.count(), 6);
    assert!(w.rec.frames().is_empty(), "nothing on the wire");
}

/// The same partial pickup in creative: the second pass is the creative
/// tail, so the rest is destroyed and the take reports all ten.
#[test]
fn a_partial_creative_pickup_takes_all_and_keeps_what_fit() {
    let w = world();
    let mut inv = Hotbar::new();
    for i in 0..crate::player::INVENTORY_SIZE {
        inv.set_stack(i, ItemStack::new("minecraft:dirt", 64));
    }
    inv.set_stack(5, ItemStack::new("minecraft:stone", 60));
    let id = w.drop_item(0.5, 0.5, ItemStack::new("minecraft:stone", 10), 0);
    let t = w.touch(&mut inv, true);
    assert_eq!(t.taken, vec![(id, 10)]);
    assert_eq!(inv.stack(5).count(), 64);
    assert!(w.store.get(id).is_none());
}

/// `getSlotWithRemainingSpace` probes the selected slot, then the
/// **offhand**, then scans: a stone in the offhand is topped up before a
/// stone in slot 3.
#[test]
fn the_offhand_is_topped_up_before_the_scan() {
    let w = world();
    let mut inv = Hotbar::new();
    inv.set_stack(3, ItemStack::new("minecraft:stone", 1));
    inv.set_offhand(ItemStack::new("minecraft:stone", 1));
    w.drop_item(0.5, 0.5, ItemStack::new("minecraft:stone", 1), 0);
    let t = w.touch(&mut inv, false);
    assert_eq!(t.slots, vec![crate::player::SLOT_OFFHAND]);
    assert_eq!(inv.offhand().count(), 2);
    assert_eq!(inv.stack(3).count(), 1);
}

/// A spectator's `aiStep` skips the loop.
#[test]
fn a_spectator_touches_nothing() {
    let w = world();
    let id = w.drop_item(0.5, 0.5, ItemStack::new("minecraft:stone", 1), 0);
    let mut inv = Hotbar::new();
    let who = Toucher {
        entity: &w.me,
        uuid: w.me.uuid,
        crouching: false,
        infinite_materials: false,
        spectator: true,
    };
    let t = touch(
        &w.store,
        &who,
        &mut inv,
        &w.entities,
        &w.tracker,
        &w.players,
    );
    assert_eq!(t, Touched::default());
    assert!(w.store.get(id).is_some());
}

/// A player's half-width is `0.6f / 2.0f` widened, not `0.3`.
#[test]
fn the_half_width_is_the_float_widened() {
    let b = make_bounding_box(STANDING, 0.0, 0.0, 0.0);
    assert_eq!(b.max_x, f64::from(0.3_f32));
    assert_ne!(b.max_x, 0.3);
    assert_eq!(b.max_y, f64::from(1.8_f32));
}
