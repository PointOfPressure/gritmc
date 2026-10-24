//! A thrown egg: `EggItem.use`, the `ThrownEgg` it launches, and the chicks
//! that may hatch where it breaks.
//!
//! Reference (26.2, `javap -p -c -constants` on the pinned jar):
//! `EggItem.use` (0-102), `Projectile.spawnProjectileFromRotation` /
//! `spawnProjectile` (`consumer.accept(p)`, `addFreshEntity(p)`,
//! `applyOnProjectileSpawned`), `ThrowableItemProjectile.<init>(type, owner,
//! level, stack)` (`getX(), getEyeY() - 0.10000000149011612, getZ()`,
//! `setOwner`), `setItem` (`DATA_ITEM_STACK` = `stack.copyWithCount(1)`),
//! `Projectile.shootFromRotation` (0-113), `shoot` (0-89),
//! `getMovementToShoot` (0-74), `ThrowableProjectile.tick` (0-93),
//! `applyInertia` (0.99, 0.8 in water), `getDefaultGravity` (0.03),
//! `Projectile.tick` (`checkLeftOwner`, `Entity.tick`), `checkLeftOwner` /
//! `isOutsideOwnerCollisionRange`, `canHitEntity`, `onHit` (0-142),
//! `updateRotation` / `lerpRotation`, `ProjectileUtil.getHitResultOnMoveVector`
//! / `getHitResult` (0-85) / `getEntityHitResult` (0-142) / `computeMargin`,
//! `ThrownEgg.onHit` (0-181), `ThrownEgg.onHitEntity` (0-25),
//! `Entity.fudgePositionAfterSizeChange` (0-258),
//! `CollisionGetter.findFreePosition` and its three lambdas,
//! `EntityTypes.EGG` (`MISC`, `noLootTable`, `sized(0.25F, 0.25F)`,
//! `clientTrackingRange(4)`, `updateInterval(10)`).
//!
//! # The order, and which random each draw comes from
//!
//! ```text
//!   EggItem.use     7-47  level.playSound(null, player xyz, EGG_THROW, PLAYERS,
//!                         0.5F, 0.4F / (level.random.nextFloat() * 0.4F + 0.8F))
//!                  63-77  new ThrownEgg(level, player, stack)     the egg's random:
//!                                                                 a uuid
//!                         shootFromRotation(player, xRot, yRot, 0F, 1.5F, 1F)
//!                                                                 three triangles
//!                         addFreshEntity                          pairing
//!                  92-96  stack.consume(1, player)                not in creative
//!                     99  SUCCESS                                 the client swings
//!   each tick             gravity -0.03, inertia *0.99, the hit test along the
//!                         new velocity, setPos, updateRotation, Entity.tick,
//!                         then on a hit: onHit
//!   ThrownEgg.onHit 15-45 egg.random.nextInt(8) == 0 -> one chick, and then
//!                         nextInt(32) == 0 -> four
//!                 168-178 broadcastEntityEvent(this, 3); discard()
//! ```
//!
//! # Named, not done
//!
//! * `ThrownEgg.onHitEntity` hurts what it hits for `0` through
//!   `damageSources().thrown(...)`; this server has no damage path, so a hit
//!   entity is a hit (the egg breaks there, the chicks may hatch) and nothing
//!   is hurt.
//! * `Projectile.onHitBlock` → `BlockState.onProjectileHit` (a target block, a
//!   bell, a chorus flower, a pointed dripstone): none is reached by an egg on
//!   a corpus rig, and no block's arm is ported.
//! * `onHit`'s `REDIRECTABLE_PROJECTILE` deflection (only for a wind charge or
//!   a fireball as the *target*) and `hitTargetOrDeflectSelf`'s entity
//!   deflections (a breeze); `handleFirstTickBubbleColumn`.
//! * Tick order: eggs tick after every mob, not interleaved with them in
//!   vanilla's one `EntityTickList`. Nothing an egg does in its tick reads a
//!   mob that ticked after it in the same tick, except a mob it hits.
use super::host::ServerMob;
use crate::entities::{Entities, Entity};
use crate::tracker::{TrackedPlayer, Tracker};
use rustmc_data::BlockPos;
use rustmc_entity::host::MobHost;
use rustmc_entity::phys::{Aabb, Vec3};
use rustmc_worldgen::rng::{LegacyRandomSource, RandomSource};
use std::sync::{Mutex, OnceLock};

/// `EntityTypes.EGG` — `sized(0.25F, 0.25F)`.
pub const SIZE: f64 = 0.25;
/// `ThrowableProjectile.getDefaultGravity()`.
pub const GRAVITY: f64 = 0.03;
/// `EggItem.use`'s `EGG_THROW`.
pub const THROW_SOUND: &str = "minecraft:entity.egg.throw";
/// `ThrownEgg.onHit` 168-174: `broadcastEntityEvent(this, 3)`.
pub const EVENT_BREAK: u8 = 3;

/// One egg in flight.
#[derive(Clone)]
pub struct ThrownEgg {
    pub id: i32,
    pub uuid: u128,
    pub pos: Vec3,
    pub delta: Vec3,
    pub y_rot: f32,
    pub x_rot: f32,
    pub y_rot_o: f32,
    pub x_rot_o: f32,
    pub tick_count: i32,
    pub owner: Option<i32>,
    pub left_owner: bool,
    pub left_owner_checked: bool,
    pub was_touching_water: bool,
    pub removed: bool,
    /// `getItem()`: `egg`, `blue_egg` or `brown_egg`.
    pub item: String,
    pub random: LegacyRandomSource,
    /// The level it flies through.
    pub level: super::level::MobLevel,
}

impl ThrownEgg {
    fn bounding_box(&self) -> Aabb {
        let h = SIZE / 2.0;
        Aabb::new(
            self.pos.x - h,
            self.pos.y,
            self.pos.z - h,
            self.pos.x + h,
            self.pos.y + SIZE,
            self.pos.z + h,
        )
    }
}

fn store() -> &'static Mutex<Vec<ThrownEgg>> {
    static S: OnceLock<Mutex<Vec<ThrownEgg>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(Vec::new()))
}

/// Whether `item` is one `EggItem.use` answers for.
#[must_use]
pub fn is_egg(item: &str) -> bool {
    matches!(
        item,
        "minecraft:egg" | "minecraft:blue_egg" | "minecraft:brown_egg"
    )
}

/// The thrower, as `EggItem.use` reads it.
#[derive(Debug, Clone, Copy)]
pub struct Thrower {
    pub entity_id: i32,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub x_rot: f32,
    pub y_rot: f32,
    pub on_ground: bool,
    /// `getEyeY()`'s offset.
    pub eye_height: f64,
}

/// `EggItem.use(level, player, hand)`, 0-99, for a server level. Returns the
/// egg's entity id. The caller consumes the stack (`stack.consume(1,
/// player)`, offsets 92-96), which a creative player skips.
pub fn use_egg(
    world: &'static crate::tiles::Tiles,
    item: &str,
    thrower: Thrower,
    players: &[TrackedPlayer],
) -> Option<i32> {
    super::set_tick_players(players);
    // 7-47: the sound, at the player, to everyone (`null` player), PLAYERS,
    // 0.5F, `0.4F / (level.getRandom().nextFloat() * 0.4F + 0.8F)`.
    let f = super::level_random(|r| r.next_f32());
    let pitch = 0.4_f32 / (f * 0.4 + 0.8);
    super::play_sound_at(
        (thrower.x, thrower.y, thrower.z),
        THROW_SOUND,
        crate::sounds::SOURCE_PLAYERS,
        0.5,
        pitch,
    );
    let level = super::mob_level(world)?;
    // `new ThrownEgg(level, player, stack)`: `Entity.<init>` — an id and the
    // egg's own random's uuid — then the position and the owner.
    let entities = Entities::shared();
    let id = entities.next_id();
    let mut random = crate::dropitem::unique_random();
    let uuid = crate::dropitem::insecure_uuid(&mut random);
    let mut egg = ThrownEgg {
        id,
        uuid,
        pos: Vec3::new(
            thrower.x,
            thrower.y + thrower.eye_height - 0.100_000_001_490_116_12,
            thrower.z,
        ),
        delta: Vec3::ZERO,
        y_rot: 0.0,
        x_rot: 0.0,
        y_rot_o: 0.0,
        x_rot_o: 0.0,
        tick_count: 0,
        owner: Some(thrower.entity_id),
        left_owner: false,
        left_owner_checked: false,
        was_touching_water: false,
        removed: false,
        item: item.to_string(),
        random,
        level,
    };
    shoot_from_rotation(&mut egg, &thrower, 0.0, 1.5, 1.0);
    let record = mirror(&egg);
    entities.insert(record.clone());
    if let Some(kinds) = crate::entities::shared_kind_ids() {
        Tracker::shared().add(&record, players, kinds);
    }
    store().lock().expect("eggs").push(egg);
    Some(id)
}

/// `Projectile.shootFromRotation(shooter, xRot, yRot, roll, velocity,
/// inaccuracy)`, 0-113.
fn shoot_from_rotation(
    e: &mut ThrownEgg,
    shooter: &Thrower,
    roll: f32,
    velocity: f32,
    inaccuracy: f32,
) {
    use rustmc_entity::mth;
    const RAD: f32 = 0.017_453_292;
    let x_rot = shooter.x_rot;
    let y_rot = shooter.y_rot;
    let xd = -mth::sin(f64::from(y_rot * RAD)) * mth::cos(f64::from(x_rot * RAD));
    let yd = -mth::sin(f64::from((x_rot + roll) * RAD));
    let zd = mth::cos(f64::from(y_rot * RAD)) * mth::cos(f64::from(x_rot * RAD));
    shoot(
        e,
        f64::from(xd),
        f64::from(yd),
        f64::from(zd),
        velocity,
        inaccuracy,
    );
    // 70-110: plus the shooter's known movement, its y only when airborne.
    let known = super::player_known_movement(shooter.entity_id);
    let ky = if shooter.on_ground { 0.0 } else { known.y };
    e.delta = Vec3::new(e.delta.x + known.x, e.delta.y + ky, e.delta.z + known.z);
}

/// `Projectile.shoot(x, y, z, velocity, inaccuracy)`, 0-89, through
/// `getMovementToShoot` (0-74): `new Vec3(x, y, z).normalize().add(
/// random.triangle(0, 0.0172275 * inaccuracy) x3).scale(velocity)`, then
/// the rotation from it.
fn shoot(e: &mut ThrownEgg, x: f64, y: f64, z: f64, velocity: f32, inaccuracy: f32) {
    use rustmc_entity::mth;
    let len = (x * x + y * y + z * z).sqrt();
    let (nx, ny, nz) = if len < 1.0E-5 {
        (0.0, 0.0, 0.0)
    } else {
        (x / len, y / len, z / len)
    };
    let dev = 0.017_227_5 * f64::from(inaccuracy);
    let mut tri = || 0.0 + dev * (e.random.next_f64() - e.random.next_f64());
    let tx = tri();
    let ty = tri();
    let tz = tri();
    let v = Vec3::new(
        (nx + tx) * f64::from(velocity),
        (ny + ty) * f64::from(velocity),
        (nz + tz) * f64::from(velocity),
    );
    e.delta = v;
    let h = (v.x * v.x + v.z * v.z).sqrt();
    #[expect(clippy::cast_possible_truncation, reason = "vanilla's own d2f")]
    {
        e.y_rot = (mth::atan2(v.x, v.z) * 57.295_776_367_187_5) as f32;
        e.x_rot = (mth::atan2(v.y, h) * 57.295_776_367_187_5) as f32;
    }
    e.y_rot_o = e.y_rot;
    e.x_rot_o = e.x_rot;
}

/// The record the tracker reads. `add_entity`'s `data` is the owner's id
/// (`Projectile.getAddEntityPacket`: `owner == null ? 0 : owner.getId()`).
fn mirror(e: &ThrownEgg) -> Entity {
    let item_id = super::drops::item_id(&e.item);
    Entity {
        id: e.id,
        uuid: e.uuid,
        kind: crate::entities::EGG,
        x: e.pos.x,
        y: e.pos.y,
        z: e.pos.z,
        y_rot: e.y_rot,
        x_rot: e.x_rot,
        y_head_rot: 0.0,
        movement: (e.delta.x, e.delta.y, e.delta.z),
        on_ground: false,
        data: e.owner.unwrap_or(0),
        swinging: false,
        swing_time: 0,
        swinging_arm: None,
        synched: crate::synched::Synched::item(item_id, 1),
    }
}

/// The hit `ProjectileUtil.getHitResultOnMoveVector` found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hit {
    Block(Vec3),
    Entity(i32, Vec3),
}

/// Every egg's tick, in throw order, after the mobs'. The caller holds the
/// tick lock; `level` is the level the mobs tick over.
pub fn tick_all(entities: &Entities, tracker: &Tracker, players: &[TrackedPlayer]) {
    let mut eggs = std::mem::take(&mut *store().lock().expect("eggs"));
    for e in &mut eggs {
        if e.removed {
            continue;
        }
        e.tick_count += 1;
        let level = e.level;
        let hit = tick(e, &level);
        if let Some(hit) = hit {
            if !e.removed {
                on_hit(e, &level, hit);
            }
        }
        if e.removed {
            entities.remove(e.id);
            tracker.remove(e.id, players);
        } else {
            let record = mirror(e);
            entities.update(e.id, |x| *x = record.clone());
        }
    }
    eggs.retain(|e| !e.removed);
    let mut g = store().lock().expect("eggs");
    eggs.append(&mut g);
    *g = eggs;
}

/// `ThrowableProjectile.tick`, 0-93, up to the hit; returns it.
fn tick(e: &mut ThrownEgg, level: &super::level::MobLevel) -> Option<Hit> {
    // 5: `applyGravity()` — `Entity.applyGravity`: `-getGravity()` on y.
    e.delta = Vec3::new(e.delta.x, e.delta.y - GRAVITY, e.delta.z);
    // 9: `applyInertia()` — 0.8 in water, `getAirDrag()` 0.99 otherwise.
    let drag = if e.was_touching_water { 0.8 } else { 0.99_f32 };
    let d = f64::from(drag);
    e.delta = Vec3::new(e.delta.x * d, e.delta.y * d, e.delta.z * d);
    // 14-22: the hit along the new velocity.
    let hit = hit_result_on_move_vector(e, level);
    // 23-55.
    let to = match hit {
        Some(Hit::Block(p) | Hit::Entity(_, p)) => p,
        None => Vec3::new(
            e.pos.x + e.delta.x,
            e.pos.y + e.delta.y,
            e.pos.z + e.delta.z,
        ),
    };
    e.pos = to;
    // 59: `updateRotation()`.
    update_rotation(e);
    // 63: `applyEffectsFromBlocks()` — none an egg feels here. 67:
    // `Projectile.tick`: the shoot game event; `checkLeftOwner`;
    // `Entity.tick` → `baseTick`, whose one effect on an egg is the water
    // state the next tick's inertia reads.
    check_left_owner(e);
    e.was_touching_water = in_water(e, level);
    e.left_owner_checked = false;
    // 70-89: `hitTargetOrDeflectSelf` only while alive — the caller.
    hit
}

/// `Projectile.updateRotation`: both angles eased 20% toward the velocity's.
fn update_rotation(e: &mut ThrownEgg) {
    use rustmc_entity::mth;
    let v = e.delta;
    let h = (v.x * v.x + v.z * v.z).sqrt();
    #[expect(clippy::cast_possible_truncation, reason = "vanilla's own d2f")]
    let x = (mth::atan2(v.y, h) * 57.295_776_367_187_5) as f32;
    #[expect(clippy::cast_possible_truncation, reason = "vanilla's own d2f")]
    let y = (mth::atan2(v.x, v.z) * 57.295_776_367_187_5) as f32;
    e.x_rot = lerp_rotation(e.x_rot_o, x);
    e.y_rot = lerp_rotation(e.y_rot_o, y);
    e.x_rot_o = e.x_rot;
    e.y_rot_o = e.y_rot;
}

/// `Projectile.lerpRotation(from, to)`: `from` unwound to within 180 of
/// `to`, then `Mth.lerp(0.2F, from, to)`.
#[must_use]
pub fn lerp_rotation(mut from: f32, to: f32) -> f32 {
    while to - from < -180.0 {
        from -= 360.0;
    }
    while to - from >= 180.0 {
        from += 360.0;
    }
    from + 0.2 * (to - from)
}

/// `Projectile.checkLeftOwner` / `isOutsideOwnerCollisionRange`: the owner's
/// (and its passengers') pickable boxes all miss this one expanded by the
/// velocity and inflated by one.
fn check_left_owner(e: &mut ThrownEgg) {
    if e.left_owner || e.left_owner_checked {
        return;
    }
    let reach = expand_towards(e.bounding_box(), e.delta).inflate(1.0, 1.0, 1.0);
    e.left_owner = match e.owner.and_then(super::facts_of_entity) {
        None => true,
        Some(f) => !f.bb.intersects(&reach),
    };
    e.left_owner_checked = true;
}

fn expand_towards(b: Aabb, d: Vec3) -> Aabb {
    let mut out = b;
    if d.x < 0.0 {
        out.min_x += d.x;
    } else if d.x > 0.0 {
        out.max_x += d.x;
    }
    if d.y < 0.0 {
        out.min_y += d.y;
    } else if d.y > 0.0 {
        out.max_y += d.y;
    }
    if d.z < 0.0 {
        out.min_z += d.z;
    } else if d.z > 0.0 {
        out.max_z += d.z;
    }
    out
}

/// `wasTouchingWater` after `updateInWaterStateAndDoFluidPushing`: a water
/// block in the box (deflated by 0.001) whose fluid surface — `y +
/// getHeight`, a full block under more water — is at or above the box's
/// bottom.
fn in_water(e: &ThrownEgg, level: &super::level::MobLevel) -> bool {
    let b = e.bounding_box();
    let lo = |v: f64| (v + 0.001).floor() as i32;
    let hi = |v: f64| (v - 0.001).ceil() as i32;
    for x in lo(b.min_x)..hi(b.max_x) {
        for y in lo(b.min_y)..hi(b.max_y) {
            for z in lo(b.min_z)..hi(b.max_z) {
                let p = BlockPos { x, y, z };
                let Some(row) = level.facts.row(level.state_id(p)) else {
                    continue;
                };
                if row.fluid_kind != 1 {
                    continue;
                }
                let above = level.facts.row(level.state_id(BlockPos { x, y: y + 1, z }));
                let h = if above.is_some_and(|a| a.fluid_kind == 1) {
                    1.0
                } else {
                    f64::from(row.fluid_own_height)
                };
                if f64::from(y) + h >= b.min_y + 0.001 {
                    return true;
                }
            }
        }
    }
    false
}

/// `ProjectileUtil.getHitResultOnMoveVector(this, this::canHitEntity)` →
/// `getHitResult(position(), this, filter, getDeltaMovement(), level,
/// computeMargin(this), COLLIDER)`.
fn hit_result_on_move_vector(e: &ThrownEgg, level: &super::level::MobLevel) -> Option<Hit> {
    let from = e.pos;
    let mut to = Vec3::new(from.x + e.delta.x, from.y + e.delta.y, from.z + e.delta.z);
    let block = super::level::clip_location(level, from, to);
    if let Some(p) = block {
        to = p;
    }
    let search = expand_towards(e.bounding_box(), e.delta).inflate(1.0, 1.0, 1.0);
    let margin = compute_margin(e.tick_count);
    if let Some((id, at)) = entity_hit_result(e, from, to, &search, margin) {
        return Some(Hit::Entity(id, at));
    }
    block.map(Hit::Block)
}

/// `ProjectileUtil.computeMargin(entity)`: `max(0, min(0.3F, (tickCount -
/// 2) / 20F))`.
#[must_use]
pub fn compute_margin(tick_count: i32) -> f32 {
    #[expect(clippy::cast_precision_loss, reason = "vanilla's own i2f")]
    let t = (tick_count - 2) as f32 / 20.0;
    0.0_f32.max(0.3_f32.min(t))
}

/// `ProjectileUtil.getEntityHitResult(level, projectile, from, to, box,
/// filter, margin)`, 0-142: every entity in the box the filter admits, its
/// box inflated by the margin and clipped by the segment; the nearest
/// strictly-closer entry wins, in `getEntities` order.
fn entity_hit_result(
    e: &ThrownEgg,
    from: Vec3,
    to: Vec3,
    search: &Aabb,
    margin: f32,
) -> Option<(i32, Vec3)> {
    let mut best: Option<(i32, Vec3)> = None;
    let mut best_d = f64::MAX;
    let mut ids: Vec<i32> = Entities::shared().all().into_iter().map(|x| x.id).collect();
    ids.sort_unstable();
    for id in ids {
        if id == e.id {
            continue;
        }
        let Some(f) = super::facts_of_entity(id) else {
            // Not living: an item, an orb, another egg — `isPickable` false
            // for all three, so `canBeHitByProjectile` refuses them.
            continue;
        };
        if !f.bb.intersects(search) || !can_hit_entity(e, id, f.alive, f.is_spectator) {
            continue;
        }
        let m = f64::from(margin);
        let b = f.bb.inflate(m, m, m);
        if let Some(p) = aabb_clip(&b, from, to) {
            let d = (p.x - from.x).powi(2) + (p.y - from.y).powi(2) + (p.z - from.z).powi(2);
            if d < best_d {
                best_d = d;
                best = Some((id, p));
            }
        }
    }
    best
}

/// `Projectile.canHitEntity(target)`: `target.canBeHitByProjectile()`
/// (`isAlive() && isPickable()` — a spectator is not pickable), and the
/// owner only once the egg has left it (`isPassengerOfSameVehicle` is
/// `getRootVehicle() == other.getRootVehicle()`, true for the owner itself).
fn can_hit_entity(e: &ThrownEgg, target: i32, alive: bool, spectator: bool) -> bool {
    if !alive || spectator {
        return false;
    }
    match e.owner {
        Some(o) if !e.left_owner => o != target,
        _ => true,
    }
}

/// `AABB.clip(from, to)`: the entry point on the box's faces, or nothing
/// (a segment starting inside crosses no entry face).
fn aabb_clip(b: &Aabb, from: Vec3, to: Vec3) -> Option<Vec3> {
    let d = Vec3::new(to.x - from.x, to.y - from.y, to.z - from.z);
    let mut t = 1.0_f64;
    let mut hit = false;
    let face = |dist: f64,
                plane: f64,
                f0: f64,
                a: f64,
                a0: f64,
                a1: f64,
                bb: f64,
                b0: f64,
                b1: f64,
                da: f64,
                db: f64,
                t: &mut f64| {
        let s = (plane - f0) / dist;
        let pa = a + s * da;
        let pb = bb + s * db;
        if s > 0.0
            && s < *t
            && pa > a0 - 1.0E-7
            && pa < a1 + 1.0E-7
            && pb > b0 - 1.0E-7
            && pb < b1 + 1.0E-7
        {
            *t = s;
            true
        } else {
            false
        }
    };
    if d.x > 1.0E-7 {
        hit |= face(
            d.x, b.min_x, from.x, from.y, b.min_y, b.max_y, from.z, b.min_z, b.max_z, d.y, d.z,
            &mut t,
        );
    } else if d.x < -1.0E-7 {
        hit |= face(
            d.x, b.max_x, from.x, from.y, b.min_y, b.max_y, from.z, b.min_z, b.max_z, d.y, d.z,
            &mut t,
        );
    }
    if d.y > 1.0E-7 {
        hit |= face(
            d.y, b.min_y, from.y, from.z, b.min_z, b.max_z, from.x, b.min_x, b.max_x, d.z, d.x,
            &mut t,
        );
    } else if d.y < -1.0E-7 {
        hit |= face(
            d.y, b.max_y, from.y, from.z, b.min_z, b.max_z, from.x, b.min_x, b.max_x, d.z, d.x,
            &mut t,
        );
    }
    if d.z > 1.0E-7 {
        hit |= face(
            d.z, b.min_z, from.z, from.x, b.min_x, b.max_x, from.y, b.min_y, b.max_y, d.x, d.y,
            &mut t,
        );
    } else if d.z < -1.0E-7 {
        hit |= face(
            d.z, b.max_z, from.z, from.x, b.min_x, b.max_x, from.y, b.min_y, b.max_y, d.x, d.y,
            &mut t,
        );
    }
    hit.then(|| Vec3::new(from.x + t * d.x, from.y + t * d.y, from.z + t * d.z))
}

/// `Projectile.onHit` (the entity or block arm — see the module docs for
/// what each does not do here) and then `ThrownEgg.onHit` 15-178.
fn on_hit(e: &mut ThrownEgg, level: &super::level::MobLevel, _hit: Hit) {
    // 15-45.
    if e.random.next_i32_bound(8) == 0 {
        let mut count = 1;
        if e.random.next_i32_bound(32) == 0 {
            count = 4;
        }
        for _ in 0..count {
            if !hatch(e, level) {
                // 146-149: `fudgePositionAfterSizeChange` failed — `goto 168`,
                // the loop is left and no later chick is made.
                break;
            }
        }
    }
    // 168-178.
    super::breed::broadcast_entity_event(e.id, EVENT_BREAK);
    e.removed = true;
}

/// One chick, `ThrownEgg.onHit` 54-162: `CHICKEN.create(level, TRIGGERED)`,
/// `setAge(-24000)`, `snapTo(getX(), getY(), getZ(), getYRot(), 0F)`, the
/// egg item's `CHICKEN_VARIANT` component, `fudgePositionAfterSizeChange(
/// ZERO_SIZED_DIMENSIONS)` and, only when that found room,
/// `addFreshEntity`. Returns `fudge`'s answer.
fn hatch(e: &ThrownEgg, level: &super::level::MobLevel) -> bool {
    let Some(table) = super::attribute_table() else {
        return false;
    };
    let id = Entities::shared().next_id();
    let Some(mut chick) = ServerMob::new(*level, &super::chicken::CHICKEN, id, table) else {
        return false;
    };
    chick.set_age(super::breed::BABY_START_AGE);
    chick.snap_to(e.pos.x, e.pos.y, e.pos.z, e.y_rot, 0.0);
    // `Items.EGG`/`BLUE_EGG`/`BROWN_EGG` carry `CHICKEN_VARIANT` temperate,
    // cold, warm; the stack in flight is a one-count copy of the thrown one.
    let variant = match e.item.as_str() {
        "minecraft:blue_egg" => "minecraft:cold",
        "minecraft:brown_egg" => "minecraft:warm",
        _ => "minecraft:temperate",
    };
    if let Some(v) = super::captured_registry(super::variants::CHICKEN.variant_registry)
        .iter()
        .position(|n| n == variant)
        .and_then(|v| i32::try_from(v).ok())
    {
        chick.data.set_varint(super::data::id::CHICKEN_VARIANT, v);
    }
    let (w, h) = chick.dimensions();
    let Some(p) =
        fudge_position_after_size_change(level, chick.position(), f64::from(w), f64::from(h))
    else {
        return false;
    };
    chick.set_pos_raw(p.x, p.y, p.z);
    super::add_converted(chick);
    true
}

/// `Entity.fudgePositionAfterSizeChange(ZERO_SIZED_DIMENSIONS)`, 0-258, for
/// an entity whose current dimensions are `w` x `h` and whose previous ones
/// were zero: the position to move to, or `None` for `false`.
///
/// ```text
///   p3 = position + (0, 0 / 2, 0)
///   d4 = max(0, w - 0) + 1e-6;  d6 = max(0, h - 0) + 1e-6
///   free = findFreePosition(this, box(ofSize(p3, d4, d6, d4)), p3, w, h, w)
///   free -> setPos(free + (0, -h / 2, 0)); true
///   w > 0 && h > 0:
///     free = findFreePosition(this, box(ofSize(p3, d4, 1e-6, d4)), p3, w, 0, w)
///     free -> setPos(free + (0, -0 / 2 + 1e-6, 0)); true
///   false
/// ```
#[must_use]
pub fn fudge_position_after_size_change(
    level: &super::level::MobLevel,
    pos: Vec3,
    w: f64,
    h: f64,
) -> Option<Vec3> {
    let w = f64::from(w as f32);
    let h = f64::from(h as f32);
    let d4 = 0.0_f64.max(w - 0.0) + 1.0E-6;
    let d6 = 0.0_f64.max(h - 0.0) + 1.0E-6;
    let p3 = pos;
    if let Some(f) = find_free_position(level, of_size(p3, d4, d6, d4), p3, w, h, w) {
        return Some(Vec3::new(f.x, f.y + (-h / 2.0), f.z));
    }
    if w > 0.0 && h > 0.0 {
        if let Some(f) = find_free_position(level, of_size(p3, d4, 1.0E-6, d4), p3, w, 0.0, w) {
            return Some(Vec3::new(f.x, f.y + (-0.0 / 2.0 + 1.0E-6), f.z));
        }
    }
    None
}

/// `AABB.ofSize(center, xs, ys, zs)`.
fn of_size(c: Vec3, xs: f64, ys: f64, zs: f64) -> Aabb {
    Aabb::new(
        c.x - xs / 2.0,
        c.y - ys / 2.0,
        c.z - zs / 2.0,
        c.x + xs / 2.0,
        c.y + ys / 2.0,
        c.z + zs / 2.0,
    )
}

/// `IndirectMerger`'s coordinate tolerance: a coordinate within `1e-7` of
/// the previous one kept is the same grid line.
const MERGE_EPS: f64 = 1.0E-7;

/// `CollisionGetter.findFreePosition(entity, allowedCenters, target, w, h,
/// d)`: the block collision boxes around the allowed region, each inflated
/// by half the entity's size and unioned (`Shapes.or`); the allowed region
/// minus that (`Shapes.join(..., ONLY_FIRST)`); its point closest to
/// `target` (`VoxelShape.closestPointTo`).
///
/// The difference is computed on the merged grid of every box edge — the
/// grid `IndirectMerger` builds, lines within `1e-7` merged — and each grid
/// cell is in or out whole, which is what a `VoxelShape` is. The closest
/// point is the clamp of `target` into each free cell, the first strictly
/// closer kept; `forAllBoxes` would visit merged boxes rather than cells,
/// which can only change which of two exactly equidistant points wins.
fn find_free_position(
    level: &super::level::MobLevel,
    allowed: Aabb,
    target: Vec3,
    w: f64,
    h: f64,
    d: f64,
) -> Option<Vec3> {
    if allowed.max_x - allowed.min_x < 1.0E-7
        || allowed.max_y - allowed.min_y < 1.0E-7
        || allowed.max_z - allowed.min_z < 1.0E-7
    {
        return None;
    }
    let query = allowed.inflate(w / 2.0, h / 2.0, d / 2.0);
    let probe = crate::collide::Aabb::new(
        query.min_x,
        query.min_y,
        query.min_z,
        query.max_x,
        query.max_y,
        query.max_z,
    );
    let placed = crate::collide::collect_block_collisions(level.shapes, level, probe).ok()?;
    let mut boxes: Vec<Aabb> = Vec::new();
    for p in &placed {
        let s = p.shape;
        if s.is_empty() {
            continue;
        }
        let (cx, cy, cz) = (
            s.coords(crate::collide::Axis::X),
            s.coords(crate::collide::Axis::Y),
            s.coords(crate::collide::Axis::Z),
        );
        let (sx, sy, sz) = (
            s.size(crate::collide::Axis::X),
            s.size(crate::collide::Axis::Y),
            s.size(crate::collide::Axis::Z),
        );
        let (ox, oy, oz) = (f64::from(p.pos.x), f64::from(p.pos.y), f64::from(p.pos.z));
        for x in 0..sx {
            for y in 0..sy {
                for z in 0..sz {
                    if !s.is_full_wide(
                        i64::try_from(x).unwrap_or(0),
                        i64::try_from(y).unwrap_or(0),
                        i64::try_from(z).unwrap_or(0),
                    ) {
                        continue;
                    }
                    let b = Aabb::new(
                        cx[x] + ox,
                        cy[y] + oy,
                        cz[z] + oz,
                        cx[x + 1] + ox,
                        cy[y + 1] + oy,
                        cz[z + 1] + oz,
                    );
                    // `BlockCollisions` yields only shapes that touch the
                    // query box; the rest cannot reach `allowed` inflated.
                    if b.intersects(&query) {
                        boxes.push(b.inflate(w / 2.0, h / 2.0, d / 2.0));
                    }
                }
            }
        }
    }
    let lines = |lo: f64, hi: f64, pick: &dyn Fn(&Aabb) -> (f64, f64)| -> Vec<f64> {
        let mut v = vec![lo, hi];
        for b in &boxes {
            let (a, c) = pick(b);
            for x in [a, c] {
                if x > lo && x < hi {
                    v.push(x);
                }
            }
        }
        v.sort_by(f64::total_cmp);
        let mut out: Vec<f64> = Vec::with_capacity(v.len());
        for x in v {
            // `!(last >= x - eps)`, NaN-free here: every line is finite.
            if out.last().is_none_or(|&l| l < x - MERGE_EPS) {
                out.push(x);
            }
        }
        out
    };
    let xs = lines(allowed.min_x, allowed.max_x, &|b| (b.min_x, b.max_x));
    let ys = lines(allowed.min_y, allowed.max_y, &|b| (b.min_y, b.max_y));
    let zs = lines(allowed.min_z, allowed.max_z, &|b| (b.min_z, b.max_z));
    let mut best: Option<Vec3> = None;
    let mut best_d = f64::MAX;
    for i in 0..xs.len().saturating_sub(1) {
        for j in 0..ys.len().saturating_sub(1) {
            for k in 0..zs.len().saturating_sub(1) {
                let (mx, my, mz) = (
                    (xs[i] + xs[i + 1]) / 2.0,
                    (ys[j] + ys[j + 1]) / 2.0,
                    (zs[k] + zs[k + 1]) / 2.0,
                );
                let blocked = boxes.iter().any(|b| {
                    mx > b.min_x
                        && mx < b.max_x
                        && my > b.min_y
                        && my < b.max_y
                        && mz > b.min_z
                        && mz < b.max_z
                });
                if blocked {
                    continue;
                }
                let p = Vec3::new(
                    target.x.clamp(xs[i], xs[i + 1]),
                    target.y.clamp(ys[j], ys[j + 1]),
                    target.z.clamp(zs[k], zs[k + 1]),
                );
                let dd =
                    (p.x - target.x).powi(2) + (p.y - target.y).powi(2) + (p.z - target.z).powi(2);
                if best.is_none() || dd < best_d {
                    best_d = dd;
                    best = Some(p);
                }
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lerp_rotation_unwinds_then_eases_a_fifth() {
        assert_eq!(lerp_rotation(0.0, 10.0), 2.0);
        // 350 -> 10 is a 20-degree turn the short way, not 340 the long way:
        // `from` unwinds to -10 and eases a fifth of 20.
        assert!((lerp_rotation(350.0, 10.0) - -6.0).abs() < 1e-4);
    }

    #[test]
    fn the_margin_grows_from_the_third_tick_to_three_tenths() {
        assert_eq!(compute_margin(0), 0.0);
        assert_eq!(compute_margin(2), 0.0);
        assert!((compute_margin(4) - 0.1).abs() < 1e-7);
        assert_eq!(compute_margin(100), 0.3);
    }

    #[test]
    fn a_segment_entering_a_box_clips_at_the_face_and_one_starting_inside_does_not() {
        let b = Aabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
        let p = aabb_clip(&b, Vec3::new(0.5, 2.0, 0.5), Vec3::new(0.5, 0.5, 0.5))
            .expect("enters the top");
        assert!((p.y - 1.0).abs() < 1e-12);
        assert!(aabb_clip(&b, Vec3::new(0.5, 0.5, 0.5), Vec3::new(0.5, -1.0, 0.5)).is_none());
    }
}
