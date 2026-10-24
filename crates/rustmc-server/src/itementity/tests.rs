use super::*;
use crate::collide::CollisionBlocks;

/// Stone at and below `floor`, air above: global ids 1 and 0.
struct Floor(i32);

impl CollisionBlocks for Floor {
    fn state_id_at(&self, pos: BlockPos) -> u32 {
        u32::from(pos.y <= self.0)
    }
}

fn tables() -> Tables<'static> {
    Tables {
        shapes: CollisionShapes::shared().expect("collision_shapes.json"),
        props: MovementProps::shared().expect("movement_props.json"),
        item_props: None,
    }
}

fn item_at(id: i32, x: f64, y: f64, z: f64) -> Entity {
    Entity {
        id,
        uuid: 0,
        kind: ITEM,
        x,
        y,
        z,
        y_rot: 0.0,
        x_rot: 0.0,
        y_head_rot: 0.0,
        movement: (0.0, 0.0, 0.0),
        on_ground: false,
        data: 0,
        swinging: false,
        swing_time: 0,
        swinging_arm: None,
        synched: crate::synched::Synched::EntityOnly,
    }
}

fn stone(n: i32) -> ItemData {
    ItemData::new(ItemStack::new("minecraft:stone", n), Some(1))
}

fn run(store: &ItemEntities, entities: &Entities, ticks: usize) {
    let t = tables();
    let tracker = Tracker::new();
    for _ in 0..ticks {
        tick(store, entities, &Floor(0), &t, &tracker, &[]);
    }
}

/// A thrown item at a player's hands (`y + 1.32`) with the aimed throw's
/// `+0.1` lift lands on the floor's top face and stays there; the pickup
/// delay has counted down once per tick meanwhile.
#[test]
fn a_thrown_item_falls_lands_and_its_delay_counts_down() {
    let store = ItemEntities::default();
    let entities = Entities::new();
    let mut e = item_at(entities.next_id(), 0.5, 1.0 + 1.32, 0.5);
    e.movement = (0.0, 0.1, 0.0);
    entities.insert(e.clone());
    let mut d = stone(1);
    d.pickup_delay = THROWN_PICKUP_DELAY;
    store.insert(e.id, d);
    run(&store, &entities, 30);
    let now = entities.get(e.id).expect("still there");
    assert!(now.on_ground, "landed after 30 ticks: {now:?}");
    assert!((now.y - 1.0).abs() < 1e-9, "on the top face: {}", now.y);
    let d = store.get(e.id).expect("row");
    assert_eq!(d.pickup_delay, 10);
    assert_eq!(d.age, 30);
    assert_eq!(d.tick_count, 30);
}

/// `setNeverPickUp`'s 32767 is never counted down, and `setUnlimitedLifetime`'s
/// -32768 never ages.
#[test]
fn the_infinite_delay_and_lifetime_are_left_alone() {
    let store = ItemEntities::default();
    let entities = Entities::new();
    let e = item_at(entities.next_id(), 0.5, 1.0, 0.5);
    entities.insert(e.clone());
    let mut d = stone(1);
    d.pickup_delay = INFINITE_PICKUP_DELAY;
    d.age = INFINITE_LIFETIME;
    store.insert(e.id, d);
    run(&store, &entities, 3);
    let d = store.get(e.id).expect("row");
    assert_eq!(d.pickup_delay, INFINITE_PICKUP_DELAY);
    assert_eq!(d.age, INFINITE_LIFETIME);
}

/// Offsets 534-555: the age reaches 6000 inside the tick that increments
/// it, and that same tick discards.
#[test]
fn an_item_is_discarded_on_the_tick_its_age_reaches_six_thousand() {
    let store = ItemEntities::default();
    let entities = Entities::new();
    let e = item_at(entities.next_id(), 0.5, 1.0, 0.5);
    entities.insert(e.clone());
    let mut d = stone(1);
    d.age = LIFETIME - 2;
    store.insert(e.id, d);
    run(&store, &entities, 1);
    assert!(store.get(e.id).is_some(), "5999: still here");
    run(&store, &entities, 1);
    assert!(store.get(e.id).is_none(), "6000: gone from the store");
    assert!(entities.get(e.id).is_none(), "and from the level");
}

/// An empty stack is discarded at offset 0, before anything else.
#[test]
fn an_empty_item_is_discarded_first() {
    let store = ItemEntities::default();
    let entities = Entities::new();
    let e = item_at(entities.next_id(), 0.5, 1.0, 0.5);
    entities.insert(e.clone());
    store.insert(e.id, ItemData::new(ItemStack::empty(), None));
    run(&store, &entities, 1);
    assert!(store.get(e.id).is_none());
}

/// Two resting stones a few hundredths apart merge on the first tick whose
/// `tickCount % 40 == 0`. On a tie, `tryToMerge` moves **this** item — the
/// one being ticked — into the other, so the earlier-ticked one goes and the
/// later one holds two. The survivor takes the larger delay and the smaller
/// age, and its `DATA_ITEM` is dirty.
#[test]
fn two_resting_stones_merge_and_the_ticked_one_is_the_one_that_goes() {
    let store = ItemEntities::default();
    let entities = Entities::new();
    let mut a = item_at(entities.next_id(), 0.5, 1.0, 0.5);
    let mut b = item_at(entities.next_id(), 0.55, 1.0, 0.52);
    a.on_ground = true;
    b.on_ground = true;
    entities.insert(a.clone());
    entities.insert(b.clone());
    let mut da = stone(1);
    da.tick_count = 39;
    da.pickup_delay = 5;
    da.age = 100;
    let mut db = stone(1);
    db.tick_count = 20;
    db.pickup_delay = 30;
    db.age = 50;
    store.insert(a.id, da);
    store.insert(b.id, db);
    run(&store, &entities, 1);
    assert!(store.get(a.id).is_none(), "a moved into b");
    assert!(entities.get(a.id).is_none());
    let b = store.get(b.id).expect("b survives");
    assert_eq!(b.stack.count(), 2);
    assert!(b.data_dirty);
    // max(30, 4): a ticked first (5 -> 4) and merged into b, whose own tick
    // then counts the 30 down to 29.
    assert_eq!(b.pickup_delay, 29);
    // min(50, 100) — a had not aged yet; the merge is before offset 464 —
    // then b's own tick ages it once.
    assert_eq!(b.age, 51);
}

/// The larger stack absorbs the smaller regardless of who is ticking.
#[test]
fn the_larger_stack_absorbs_the_smaller() {
    let store = ItemEntities::default();
    let entities = Entities::new();
    let mut a = item_at(entities.next_id(), 0.5, 1.0, 0.5);
    let mut b = item_at(entities.next_id(), 0.6, 1.0, 0.5);
    a.on_ground = true;
    b.on_ground = true;
    entities.insert(a.clone());
    entities.insert(b.clone());
    let mut da = stone(5);
    da.tick_count = 39;
    store.insert(a.id, da);
    store.insert(b.id, stone(2));
    run(&store, &entities, 1);
    assert_eq!(store.get(a.id).expect("a stays").stack.count(), 7);
    assert!(store.get(b.id).is_none());
}

/// More than half a block apart horizontally (beyond `inflate(0.5, 0, 0.5)`
/// of a 0.25 box) does not merge.
#[test]
fn items_out_of_reach_of_each_other_do_not_merge() {
    let store = ItemEntities::default();
    let entities = Entities::new();
    let mut a = item_at(entities.next_id(), 0.5, 1.0, 0.5);
    let mut b = item_at(entities.next_id(), 1.3, 1.0, 0.5);
    a.on_ground = true;
    b.on_ground = true;
    entities.insert(a.clone());
    entities.insert(b.clone());
    let mut da = stone(1);
    da.tick_count = 39;
    store.insert(a.id, da);
    store.insert(b.id, stone(1));
    run(&store, &entities, 1);
    assert!(store.get(a.id).is_some() && store.get(b.id).is_some());
}

/// `areMergable` reads the **second** stack's maximum, and a full pair does
/// not merge; `merge` caps at `min(max, 64)` and shrinks the source by what
/// moved.
#[test]
fn stack_arithmetic_is_vanillas() {
    let max = |i: &str| if i == "minecraft:ender_pearl" { 16 } else { 64 };
    let a = ItemStack::new("minecraft:stone", 40);
    let b = ItemStack::new("minecraft:stone", 30);
    assert!(!are_mergable(&a, &b, max), "70 > 64");
    let c = ItemStack::new("minecraft:stone", 24);
    assert!(are_mergable(&a, &c, max));
    let mut from = ItemStack::new("minecraft:ender_pearl", 10);
    let into = ItemStack::new("minecraft:ender_pearl", 10);
    let out = merge_stacks(&into, &mut from, MERGE_CAP, max);
    assert_eq!(out.count(), 16);
    assert_eq!(from.count(), 4);
    assert!(!are_mergable(
        &ItemStack::new("minecraft:stone", 1),
        &ItemStack::new("minecraft:dirt", 1),
        max
    ));
}

/// `SectionPos.asLong` packs z and y as unsigned fields under x, so within
/// one section column a negative z sorts after a positive one.
#[test]
fn section_order_puts_negative_z_after_positive() {
    let pos = item_at(1, 0.5, 1.0, 20.0);
    let neg = item_at(2, 0.5, 1.0, -20.0);
    assert!(section_order_key(&pos) < section_order_key(&neg));
    let west = item_at(3, -20.0, 1.0, -20.0);
    assert!(
        section_order_key(&west) < section_order_key(&pos),
        "x first"
    );
}

/// `DATA_ITEM` on the wire: entity id, accessor 8, serializer 7, then the
/// `OPTIONAL_STREAM_CODEC` stack — count, item id, and an empty patch —
/// then the 0xff terminator.
#[test]
fn data_item_is_count_id_and_an_empty_patch() {
    let body = data_item_body(5, &stone(2)).expect("body");
    assert_eq!(body, vec![5, 8, 7, 2, 1, 0, 0, 0xff]);
    let empty = data_item_body(5, &ItemData::new(ItemStack::empty(), None)).expect("body");
    assert_eq!(empty, vec![5, 8, 7, 0, 0xff]);
}

/// `setFluidMovement`: the lift is `5.0E-4f` widened and only below
/// `0.06` (the float, widened).
#[test]
fn fluid_movement_lifts_only_a_slow_item() {
    let v = fluid_movement(Vec3::new(1.0, 0.0, 1.0), 0.99);
    assert_eq!(v.y, f64::from(5.0e-4_f32));
    assert_eq!(v.x, 0.99);
    let fast = fluid_movement(Vec3::new(0.0, 0.07, 0.0), 0.99);
    assert_eq!(fast.y, 0.07);
}
