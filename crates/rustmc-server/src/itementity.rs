//! `ItemEntity` — the state an item on the floor carries and the tick that
//! moves, merges and expires it.
//!
//! Reference (26.2), read from the pinned jar with `javap -p -c -constants`:
//! `ItemEntity.tick`, `ItemEntity.mergeWithNeighbours`, `ItemEntity.isMergable`,
//! `ItemEntity.tryToMerge`, `ItemEntity.areMergable`, both
//! `ItemEntity.merge` overloads, the pickup-delay setters, `ItemEntity
//! .playerTouch` (the half in [`crate::pickup`]), `Entity.moveTowardsClosestSpace`,
//! `Entity.move`'s `noPhysics` arm, `ServerLevel.tickNonPassenger` (the
//! `tickCount` increment), and `EntitySectionStorage
//! .forEachAccessibleNonEmptySection` (the order [`in_box`] returns).
//!
//! # Where the state lives
//!
//! Beside [`crate::entities::Entity`], keyed by entity id, for the reason
//! [`crate::fallable::Cargo`] gives for a falling block's state: the entity
//! record is shared by every kind and carries what the tracker reads, and an
//! item's stack, age and pickup delay are read by nothing but this module and
//! [`crate::pickup`]. An `ITEM` entity with no row here was made by a path
//! that does not register one; it neither ticks nor can be picked up, which
//! is the old behaviour and is named rather than defaulted.
//!
//! # `ItemEntity.tick`, in its own order
//!
//! ```text
//!   0   if (getItem().isEmpty()) { discard(); return; }
//!  15   super.tick()                    -- baseTick: nothing reachable here
//!  19   if (pickupDelay > 0 && pickupDelay != 32767) pickupDelay--
//!  46   xo, yo, zo = position
//!  70   before = getDeltaMovement()
//!  75   water / lava with fluid height > 0.1 ? setUnder*Movement : applyGravity()
//! 153   noPhysics = !level.noCollision(this, getBoundingBox().deflate(1.0E-7))
//! 183   if (noPhysics) moveTowardsClosestSpace(x, (minY + maxY) / 2, z)
//! 221   if (!onGround || horizontalDistanceSqr > 1.0E-5f || (tickCount + id) % 4 == 0) {
//!           move(SELF, delta); applyEffectsFromBlocks();
//!           f = 0.98f; if (onGround) f *= friction(getOnPos(0.999999f))
//!           delta = delta.multiply(f, 0.98f, f)
//!           if (onGround && delta.y < 0) delta = delta.multiply(1, -0.5, 1)
//!       } else applyEffectsFromBlocksForLastMovements()
//! 366   i = movedBlock ? 2 : 40;  if (tickCount % i == 0 && isMergable()) mergeWithNeighbours()
//! 464   if (age != -32768) age++
//! 484   needsSync |= updateFluidInteraction()
//! 497   if ((delta - before).lengthSqr() > 0.01) needsSync = true
//! 534   if (age >= 6000) discard()
//! ```
//!
//! # Not ported, each unreachable or named
//!
//! * **Fluids.** `isInWater`/`isInLava` are set by `updateFluidInteraction`,
//!   which this server does not run, so both are false and the gravity arm is
//!   always the one taken: an item dropped into water sinks as through air.
//!   `setUnderwaterMovement` (0.99, +5.0E-4 while `y < 0.06`) and
//!   `setUnderLavaMovement` (0.95) are transcribed for the day the flags
//!   exist ([`fluid_movement`]) and called by nothing.
//! * **`applyEffectsFromBlocks`** — an item in fire, lava, a cactus or a
//!   pressure plate. Item health and `hurtServer` are not modelled.
//! * **`Level.noCollision`'s entity and border halves.** Only blocks are
//!   asked: nothing in this server is `canBeCollidedWith` (boats, minecarts,
//!   shulkers) and the world border is not modelled.
//! * The thrower's `ITEM_PICKED_UP` / `THROWN_ITEM_PICKED_UP_BY_PLAYER`
//!   stat and advancement — no statistics or advancements here.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock};

use crate::collide::{Aabb, CollisionBlocks, CollisionShapes, Vec3};
use crate::entities::{Entities, Entity, ITEM};
use crate::item::ItemStack;
use crate::movement_props::MovementProps;
use crate::physics::{self, Body, MoverType, Profile};
use crate::tracker::{TrackedPlayer, Tracker};
use rustmc_data::BlockPos;

/// `ItemEntity.LIFETIME` — `sipush 6000`.
pub const LIFETIME: i32 = 6000;
/// `ItemEntity.INFINITE_PICKUP_DELAY` — `setNeverPickUp`'s value.
pub const INFINITE_PICKUP_DELAY: i32 = 32_767;
/// `ItemEntity.INFINITE_LIFETIME` — `setUnlimitedLifetime`'s value.
pub const INFINITE_LIFETIME: i32 = -32_768;
/// `setDefaultPickUpDelay()` — `bipush 10`. What `Block.popResource` uses.
pub const DEFAULT_PICKUP_DELAY: i32 = 10;
/// `LivingEntity.createItemStackToDrop` offset 45 — `bipush 40`. What a
/// thrown item gets.
pub const THROWN_PICKUP_DELAY: i32 = 40;
/// `ItemEntity.merge(ItemEntity, ItemStack, ItemStack)` passes `bipush 64`
/// as the cap to the three-argument `merge`.
pub const MERGE_CAP: i32 = 64;
/// The horizontal-speed gate at tick offset 235: `9.999999747378752E-6`,
/// the float `1.0E-5f` widened.
const RESTING_SPEED_SQR: f64 = 9.999_999_747_378_752e-6;
/// Tick offset 522: `0.01d`.
const NEEDS_SYNC_DELTA_SQR: f64 = 0.01;
/// Tick offset 163: the `deflate` before the `noCollision` test.
const NO_PHYSICS_DEFLATE: f64 = 1.0e-7;
/// `getBlockPosBelowThatAffectsMyMovement` — `ItemEntity`'s override is
/// `getOnPos(0.999999f)`, not `Entity`'s 0.500001.
const FRICTION_PROBE: f32 = 0.999_999;

/// Everything `ItemEntity` keeps that `Entity` does not.
#[derive(Debug, Clone)]
pub struct ItemData {
    /// `DATA_ITEM`.
    pub stack: ItemStack,
    /// The item's network id, for the `DATA_ITEM` value — kept here because
    /// the clock thread that sends a merge's `set_entity_data` has no item
    /// table, and a merge never changes the item.
    pub item_id: Option<i32>,
    /// `ItemEntity.age`.
    pub age: i32,
    /// `ItemEntity.pickupDelay`.
    pub pickup_delay: i32,
    /// `Entity.tickCount` — incremented by `tickNonPassenger` before `tick`.
    pub tick_count: i32,
    /// `ItemEntity.thrower`, by UUID.
    pub thrower: Option<u128>,
    /// `ItemEntity.target` — who alone may pick it up, or anyone.
    pub target: Option<u128>,
    /// `Entity.mainSupportingBlockPos`, carried across ticks.
    pub supporting_block: Option<BlockPos>,
    /// `Entity.onGroundNoBlocks`, carried across ticks.
    pub on_ground_no_blocks: bool,
    /// `Entity.needsSync`, read and cleared by `ServerEntity.sendChanges`.
    pub needs_sync: bool,
    /// `DATA_ITEM` set since the last `sendDirtyEntityData`.
    pub data_dirty: bool,
}

impl ItemData {
    /// A fresh `ItemEntity` holding `stack`: age 0, no delay, no thrower.
    #[must_use]
    pub fn new(stack: ItemStack, item_id: Option<i32>) -> Self {
        Self {
            stack,
            item_id,
            age: 0,
            pickup_delay: 0,
            tick_count: 0,
            thrower: None,
            target: None,
            supporting_block: None,
            on_ground_no_blocks: false,
            needs_sync: false,
            data_dirty: false,
        }
    }

    /// `ItemEntity.isMergable()` — offsets 0-58. `isAlive` is the caller's:
    /// a row that is still in the store is alive.
    #[must_use]
    pub fn is_mergable(&self, max_stack: impl Fn(&str) -> i32) -> bool {
        self.pickup_delay != INFINITE_PICKUP_DELAY
            && self.age != INFINITE_LIFETIME
            && self.age < LIFETIME
            && self.stack.count() < stack_max(&self.stack, &max_stack)
    }
}

fn stack_max(stack: &ItemStack, max_stack: &impl Fn(&str) -> i32) -> i32 {
    if stack.is_empty() {
        max_stack("minecraft:air")
    } else {
        max_stack(stack.item())
    }
}

/// The item rows, by entity id.
#[derive(Debug, Default)]
pub struct ItemEntities {
    map: Mutex<HashMap<i32, ItemData>>,
}

impl ItemEntities {
    /// The process-wide store.
    pub fn shared() -> &'static Self {
        static STORE: OnceLock<ItemEntities> = OnceLock::new();
        STORE.get_or_init(Self::default)
    }

    /// The map, locked. A poisoned lock is recovered rather than panicked
    /// on, since every writer leaves the map consistent between statements.
    pub fn lock(&self) -> MutexGuard<'_, HashMap<i32, ItemData>> {
        self.map
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn insert(&self, id: i32, data: ItemData) {
        self.lock().insert(id, data);
    }

    #[must_use]
    pub fn get(&self, id: i32) -> Option<ItemData> {
        self.lock().get(&id).cloned()
    }

    pub fn remove(&self, id: i32) -> Option<ItemData> {
        self.lock().remove(&id)
    }
}

/// `EntityDimensions.makeBoundingBox` for an item: `sized(0.25, 0.25)`.
#[must_use]
pub fn bounding_box(e: &Entity) -> Aabb {
    Profile::ITEM.bounding_box(Vec3::new(e.x, e.y, e.z))
}

/// `AABB.intersects(AABB)` — strict on all six faces.
#[must_use]
pub fn intersects(a: &Aabb, b: &Aabb) -> bool {
    a.min_x < b.max_x
        && a.max_x > b.min_x
        && a.min_y < b.max_y
        && a.max_y > b.min_y
        && a.min_z < b.max_z
        && a.max_z > b.min_z
}

/// `AABB.inflate(x, y, z)`.
#[must_use]
pub fn inflate(a: &Aabb, x: f64, y: f64, z: f64) -> Aabb {
    Aabb::new(
        a.min_x - x,
        a.min_y - y,
        a.min_z - z,
        a.max_x + x,
        a.max_y + y,
        a.max_z + z,
    )
}

/// The key `EntitySectionStorage.forEachAccessibleNonEmptySection` walks
/// sections in: section x ascending, then — within one x, through the
/// sorted `LongSortedSet.subSet` of `SectionPos.asLong` — z as an unsigned
/// 22-bit field, then y as an unsigned 20-bit field. A negative z therefore
/// sorts **after** every non-negative one.
///
/// Within a section vanilla walks `ClassInstanceMultiMap` insertion order,
/// which is the order entities entered the section. The entity id stands in
/// for it: it is spawn order, and agrees with entry order for an entity that
/// has not changed section since it spawned — which is every item a
/// scenario here can drop and pick up. Named, not claimed exact.
#[must_use]
pub fn section_order_key(e: &Entity) -> (i32, u32, u32, i32) {
    let (sx, sy, sz) = e.section();
    #[expect(
        clippy::cast_sign_loss,
        reason = "SectionPos.asLong masks to an unsigned field"
    )]
    let (z, y) = ((sz as u32) & 0x003F_FFFF, (sy as u32) & 0x000F_FFFF);
    (sx, z, y, e.id)
}

/// `Level.getEntities(except, box, predicate)` over a snapshot, in the
/// order vanilla walks them. `EntitySelector.NO_SPECTATORS` is the caller's
/// predicate to add; no entity this module returns can be a spectator.
#[must_use]
pub fn in_box<'a>(
    all: &'a [Entity],
    box_: &Aabb,
    mut keep: impl FnMut(&Entity) -> bool,
    bounding: impl Fn(&Entity) -> Option<Aabb>,
) -> Vec<&'a Entity> {
    let mut out: Vec<&Entity> = all
        .iter()
        .filter(|e| bounding(e).is_some_and(|b| intersects(&b, box_)))
        .filter(|e| keep(e))
        .collect();
    out.sort_by_key(|e| section_order_key(e));
    out
}

/// The `set_entity_data` value list for `DATA_ITEM`: accessor 8,
/// `EntityDataSerializers.ITEM_STACK` (7), then
/// `ItemStack.OPTIONAL_STREAM_CODEC` — the count, and for a non-empty stack
/// the item id and an empty component patch (`0, 0`).
#[must_use]
pub fn data_item_body(entity_id: i32, data: &ItemData) -> Option<Vec<u8>> {
    use rustmc_proto::varint::write_varint;
    let mut value = Vec::new();
    if data.stack.is_empty() {
        write_varint(&mut value, 0).ok()?;
    } else {
        write_varint(&mut value, data.stack.count()).ok()?;
        write_varint(&mut value, data.item_id?).ok()?;
        value.extend_from_slice(&[0, 0]);
    }
    crate::synched::set_entity_data_body(
        entity_id,
        &[crate::synched::DataValue {
            id: DATA_ITEM,
            serializer: ITEM_STACK_SERIALIZER,
            value,
        }],
    )
    .ok()
}

/// `ItemEntity.DATA_ITEM` — the first accessor `ItemEntity` defines, after
/// `Entity`'s eight.
pub const DATA_ITEM: u8 = 8;
/// `EntityDataSerializers.ITEM_STACK`, the eighth registered.
pub const ITEM_STACK_SERIALIZER: i32 = 7;

/// `ServerEntity.sendDirtyEntityData` and `needsSync` for an item, drained
/// the way [`crate::mob::drain_dirty`] drains a mob's. `None` for an id with
/// no item row.
#[must_use]
pub fn drain_dirty(id: i32) -> Option<crate::mob::Dirty> {
    let mut map = ItemEntities::shared().lock();
    let d = map.get_mut(&id)?;
    let data = if d.data_dirty {
        d.data_dirty = false;
        data_item_body(id, d)
    } else {
        None
    };
    Some(crate::mob::Dirty {
        data,
        attributes: None,
        needs_sync: d.needs_sync,
    })
}

/// `Entity.needsSync = false`, at `sendChanges` offset 1189.
pub fn clear_needs_sync(id: i32) {
    if let Some(d) = ItemEntities::shared().lock().get_mut(&id) {
        d.needs_sync = false;
    }
}

/// `setUnderwaterMovement` / `setUnderLavaMovement` — `setFluidMovement(d)`:
/// `(x * d, y + (y < 0.06 ? 5.0E-4f : 0), z * d)`. Unreachable today; see
/// the module docs.
#[must_use]
pub fn fluid_movement(v: Vec3, d: f64) -> Vec3 {
    let lift = if v.y < 0.059_999_998_658_895_49 {
        f64::from(5.0e-4_f32)
    } else {
        0.0
    };
    Vec3::new(v.x * d, v.y + lift, v.z * d)
}

/// What the clock thread hands the item pass.
pub struct Tables<'a> {
    pub shapes: &'a CollisionShapes,
    pub props: &'a MovementProps,
    pub item_props: Option<&'a crate::itemprops::ItemProps>,
}

impl Tables<'_> {
    fn max_stack(&self, item: &str) -> i32 {
        self.item_props
            .map_or(crate::item::DEFAULT_MAX_STACK_SIZE, |p| {
                p.max_stack_size(item)
            })
    }
}

/// Discard an item: out of the store, out of the entity list, and
/// `remove_entities` to everyone tracking it.
pub fn discard(
    id: i32,
    map: &mut HashMap<i32, ItemData>,
    entities: &Entities,
    tracker: &Tracker,
    players: &[TrackedPlayer],
) {
    map.remove(&id);
    entities.remove(id);
    tracker.remove(id, players);
}

/// One server tick for every registered item, in the level's tick order.
///
/// Call it from the entity-tick phase of the clock thread, holding the tick
/// lock.
pub fn tick<W: CollisionBlocks>(
    store: &ItemEntities,
    entities: &Entities,
    world: &W,
    tables: &Tables<'_>,
    tracker: &Tracker,
    players: &[TrackedPlayer],
) {
    let mut map = store.lock();
    if map.is_empty() {
        return;
    }
    let order: Vec<i32> = entities
        .all_in_tick_order()
        .into_iter()
        .filter(|e| e.kind == ITEM && map.contains_key(&e.id))
        .map(|e| e.id)
        .collect();
    for id in order {
        // An item discarded earlier this tick — merged away — is skipped,
        // which is `EntityTickList.forEach`'s `isRemoved` guard.
        if !map.contains_key(&id) {
            continue;
        }
        let Some(e) = entities.get(id) else {
            map.remove(&id);
            continue;
        };
        tick_one(&e, &mut map, entities, world, tables, tracker, players);
    }
}

#[allow(clippy::too_many_lines)]
fn tick_one<W: CollisionBlocks>(
    e: &Entity,
    map: &mut HashMap<i32, ItemData>,
    entities: &Entities,
    world: &W,
    tables: &Tables<'_>,
    tracker: &Tracker,
    players: &[TrackedPlayer],
) {
    let id = e.id;
    let Some(d) = map.get_mut(&id) else { return };
    // `tickNonPassenger`: `++entity.tickCount` before `tick()`.
    d.tick_count = d.tick_count.wrapping_add(1);
    // 0-14.
    if d.stack.is_empty() {
        discard(id, map, entities, tracker, players);
        return;
    }
    // 19-43.
    if d.pickup_delay > 0 && d.pickup_delay != INFINITE_PICKUP_DELAY {
        d.pickup_delay -= 1;
    }
    let old = (e.x, e.y, e.z);
    let before = Vec3::new(e.movement.0, e.movement.1, e.movement.2);
    let mut body = Body::at(Vec3::new(e.x, e.y, e.z));
    body.movement = before;
    body.on_ground = e.on_ground;
    body.supporting_block = d.supporting_block;
    body.on_ground_no_blocks = d.on_ground_no_blocks;
    let profile = Profile::ITEM;
    // 75-132: not in water, not in lava — see the module docs.
    physics::apply_gravity(&mut body, profile.gravity);
    // 153-218: `noPhysics`, and the push out of a block it is stuck in.
    let probe = profile.bounding_box(body.pos);
    let deflated = Aabb::new(
        probe.min_x + NO_PHYSICS_DEFLATE,
        probe.min_y + NO_PHYSICS_DEFLATE,
        probe.min_z + NO_PHYSICS_DEFLATE,
        probe.max_x - NO_PHYSICS_DEFLATE,
        probe.max_y - NO_PHYSICS_DEFLATE,
        probe.max_z - NO_PHYSICS_DEFLATE,
    );
    let no_physics = crate::collide::collect_block_collisions(tables.shapes, world, deflated)
        .is_ok_and(|hits| !hits.is_empty());
    if no_physics {
        let mid_y = (probe.min_y + probe.max_y) / 2.0;
        body.movement = move_towards_closest_space(
            tables.shapes,
            world,
            body.movement,
            Vec3::new(body.pos.x, mid_y, body.pos.z),
            &mut crate::dropitem::unique_random(),
        );
    }
    // 221-362.
    let moves = !body.on_ground
        || body.movement.horizontal_distance_sqr() > RESTING_SPEED_SQR
        || (d.tick_count.wrapping_add(id)) % 4 == 0;
    if moves {
        let vec = body.movement;
        if no_physics {
            // `Entity.move` offsets 0-58: straight through, no collision.
            body.pos = Vec3::new(body.pos.x + vec.x, body.pos.y + vec.y, body.pos.z + vec.z);
            body.horizontal_collision = false;
            body.vertical_collision = false;
            body.vertical_collision_below = false;
        } else if physics::move_entity(
            tables.shapes,
            tables.props,
            world,
            &profile,
            &mut body,
            MoverType::SelfMove,
            vec,
        )
        .is_err()
        {
            // A state the collision table cannot answer for: left where it
            // is, as `crate::movement` leaves a falling block, rather than
            // falling through or standing on air.
            body.pos = Vec3::new(e.x, e.y, e.z);
        }
        // 271-319: `getAirDrag()` is `Entity`'s 0.98f; on the ground the
        // horizontal factor is multiplied by the friction under it, in float.
        let air = profile.air_drag;
        let mut f = air;
        if body.on_ground {
            let below = physics::on_pos(
                &body,
                FRICTION_PROBE,
                |p| tables.props.is_fence(world.state_id_at(p)),
                |p| tables.props.is_wall_or_gate(world.state_id_at(p)),
            );
            f *= tables.props.friction(world.state_id_at(below));
        }
        body.movement = Vec3::new(
            body.movement.x * f64::from(f),
            body.movement.y * f64::from(air),
            body.movement.z * f64::from(f),
        );
        // 322-356.
        if body.on_ground && body.movement.y < 0.0 {
            body.movement = Vec3::new(body.movement.x, body.movement.y * -0.5, body.movement.z);
        }
    }
    let new_pos = body.pos;
    let moved_block = old.0.floor() != new_pos.x.floor()
        || old.1.floor() != new_pos.y.floor()
        || old.2.floor() != new_pos.z.floor();
    entities.update(id, |ent| {
        ent.x = new_pos.x;
        ent.y = new_pos.y;
        ent.z = new_pos.z;
        ent.movement = (body.movement.x, body.movement.y, body.movement.z);
        ent.on_ground = body.on_ground;
    });
    let tick_count = {
        let d = map.get_mut(&id).expect("row checked above");
        d.supporting_block = body.supporting_block;
        d.on_ground_no_blocks = body.on_ground_no_blocks;
        d.tick_count
    };
    // 366-461.
    let interval = if moved_block { 2 } else { 40 };
    let mergable = map
        .get(&id)
        .is_some_and(|d| d.is_mergable(|i| tables.max_stack(i)));
    if tick_count % interval == 0 && mergable {
        merge_with_neighbours(id, map, entities, tables, tracker, players);
    }
    // A merge can have discarded this item. Vanilla's tick carries on with
    // the fields of a removed entity, which changes nothing anyone can see.
    let Some(d) = map.get_mut(&id) else { return };
    // 464-481.
    if d.age != INFINITE_LIFETIME {
        d.age += 1;
    }
    // 497-531.
    let delta = Vec3::new(
        body.movement.x - before.x,
        body.movement.y - before.y,
        body.movement.z - before.z,
    );
    if delta.length_sqr() > NEEDS_SYNC_DELTA_SQR {
        d.needs_sync = true;
    }
    // 534-555.
    if d.age >= LIFETIME {
        discard(id, map, entities, tracker, players);
    }
}

/// `Entity.moveTowardsClosestSpace(x, y, z)` — the delta it sets.
///
/// Of `NORTH, SOUTH, WEST, EAST, UP` (that array, in that order, with `UP`
/// as the default at distance `Double.MAX_VALUE`), the first strictly
/// nearest face whose neighbour is not a full collision cube; the delta is
/// the old one scaled by 0.75 with that axis replaced by
/// `step * (random.nextFloat() * 0.2f + 0.1f)`, in float. The draw comes
/// from the entity's own unseeded `RandomSource`, so no implementation can
/// reproduce its value — vanilla included.
fn move_towards_closest_space<W: CollisionBlocks>(
    shapes: &CollisionShapes,
    world: &W,
    delta: Vec3,
    at: Vec3,
    rng: &mut impl rustmc_worldgen::rng::RandomSource,
) -> Vec3 {
    use crate::collide::Axis;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "BlockPos.containing: Mth.floor of a world coordinate"
    )]
    let pos = BlockPos {
        x: at.x.floor() as i32,
        y: at.y.floor() as i32,
        z: at.z.floor() as i32,
    };
    let local = Vec3::new(
        at.x - f64::from(pos.x),
        at.y - f64::from(pos.y),
        at.z - f64::from(pos.z),
    );
    // (axis, step, offset)
    let dirs: [(Axis, i32, (i32, i32, i32)); 5] = [
        (Axis::Z, -1, (0, 0, -1)),
        (Axis::Z, 1, (0, 0, 1)),
        (Axis::X, -1, (-1, 0, 0)),
        (Axis::X, 1, (1, 0, 0)),
        (Axis::Y, 1, (0, 1, 0)),
    ];
    let mut best = (Axis::Y, 1);
    let mut best_dist = f64::MAX;
    for (axis, step, (dx, dy, dz)) in dirs {
        let n = BlockPos {
            x: pos.x + dx,
            y: pos.y + dy,
            z: pos.z + dz,
        };
        let full = shapes
            .get(world.state_id_at(n))
            .is_some_and(crate::collide::Shape::is_full_cube);
        if full {
            continue;
        }
        let d = match axis {
            Axis::X => local.x,
            Axis::Y => local.y,
            Axis::Z => local.z,
        };
        let dist = if step > 0 { 1.0 - d } else { d };
        if dist < best_dist {
            best_dist = dist;
            best = (axis, step);
        }
    }
    let f = rng.next_f32() * 0.2 + 0.1;
    #[expect(clippy::cast_precision_loss, reason = "a step of -1 or 1")]
    let push = f64::from(best.1 as f32 * f);
    let scaled = Vec3::new(delta.x * 0.75, delta.y * 0.75, delta.z * 0.75);
    match best.0 {
        Axis::X => Vec3::new(push, scaled.y, scaled.z),
        Axis::Y => Vec3::new(scaled.x, push, scaled.z),
        Axis::Z => Vec3::new(scaled.x, scaled.y, push),
    }
}

/// `ItemEntity.mergeWithNeighbours()` — offsets 0-89.
fn merge_with_neighbours(
    id: i32,
    map: &mut HashMap<i32, ItemData>,
    entities: &Entities,
    tables: &Tables<'_>,
    tracker: &Tracker,
    players: &[TrackedPlayer],
) {
    let max = |i: &str| tables.max_stack(i);
    if !map.get(&id).is_some_and(|d| d.is_mergable(max)) {
        return;
    }
    let Some(me) = entities.get(id) else { return };
    let query = inflate(&bounding_box(&me), 0.5, 0.0, 0.5);
    let all = entities.all();
    // `getEntitiesOfClass(ItemEntity.class, box, e -> e != this && e.isMergable())`:
    // the predicate runs while the list is built.
    let candidates: Vec<i32> = in_box(
        &all,
        &query,
        |e| e.id != id && e.kind == ITEM && map.get(&e.id).is_some_and(|d| d.is_mergable(max)),
        |e| (e.kind == ITEM).then(|| bounding_box(e)),
    )
    .into_iter()
    .map(|e| e.id)
    .collect();
    for other in candidates {
        // 64-68: `isMergable()` asked again — an earlier merge in this loop
        // can have filled or emptied it.
        if !map.get(&other).is_some_and(|d| d.is_mergable(max)) {
            continue;
        }
        try_to_merge(id, other, map, entities, tables, tracker, players);
        if !map.contains_key(&id) {
            break;
        }
    }
}

/// `ItemEntity.tryToMerge(ItemEntity)` — offsets 0-61.
fn try_to_merge(
    me: i32,
    other: i32,
    map: &mut HashMap<i32, ItemData>,
    entities: &Entities,
    tables: &Tables<'_>,
    tracker: &Tracker,
    players: &[TrackedPlayer],
) {
    let (Some(a), Some(b)) = (map.get(&me), map.get(&other)) else {
        return;
    };
    if a.target != b.target || !are_mergable(&a.stack, &b.stack, |i| tables.max_stack(i)) {
        return;
    }
    // 33-58: the smaller stack moves into the larger; on a tie, **this**
    // one moves into `other`.
    let (target, source) = if b.stack.count() < a.stack.count() {
        (me, other)
    } else {
        (other, me)
    };
    merge_entities(target, source, map, entities, tables, tracker, players);
}

/// `ItemEntity.areMergable(ItemStack, ItemStack)` — the second stack's
/// maximum, then `isSameItemSameComponents`.
#[must_use]
pub fn are_mergable(a: &ItemStack, b: &ItemStack, max_stack: impl Fn(&str) -> i32) -> bool {
    if b.count() + a.count() > stack_max(b, &max_stack) {
        return false;
    }
    ItemStack::is_same_item_same_components(a, b)
}

/// `ItemEntity.merge(ItemStack, ItemStack, int)` — the merged copy of the
/// first, and the second shrunk by what moved.
#[must_use]
pub fn merge_stacks(
    into: &ItemStack,
    from: &mut ItemStack,
    cap: i32,
    max_stack: impl Fn(&str) -> i32,
) -> ItemStack {
    let k = (stack_max(into, &max_stack).min(cap) - into.count()).min(from.count());
    let out = into.copy_with_count(into.count() + k);
    from.shrink(k);
    out
}

/// `ItemEntity.merge(ItemEntity, ItemStack, ItemEntity, ItemStack)` —
/// offsets 0-47: `setItem` on the target (which dirties `DATA_ITEM`), the
/// larger pickup delay, the smaller age, and the source discarded if it is
/// empty. The source's own stack shrinks in place and is **not** re-sent.
fn merge_entities(
    target: i32,
    source: i32,
    map: &mut HashMap<i32, ItemData>,
    entities: &Entities,
    tables: &Tables<'_>,
    tracker: &Tracker,
    players: &[TrackedPlayer],
) {
    let Some(mut src) = map.get(&source).cloned() else {
        return;
    };
    let Some(dst) = map.get_mut(&target) else {
        return;
    };
    let merged = merge_stacks(&dst.stack, &mut src.stack, MERGE_CAP, |i| {
        tables.max_stack(i)
    });
    dst.stack = merged;
    dst.data_dirty = true;
    dst.pickup_delay = dst.pickup_delay.max(src.pickup_delay);
    dst.age = dst.age.min(src.age);
    let emptied = src.stack.is_empty();
    if let Some(s) = map.get_mut(&source) {
        s.stack = src.stack;
    }
    if emptied {
        discard(source, map, entities, tracker, players);
    }
}

#[cfg(test)]
mod tests;
