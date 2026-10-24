//! `ExperienceOrb` — the entity, its tick, its pickup, and the spawn call
//! every experience source goes through.
//!
//! Reference (26.2), read from the pinned jar with `javap -p -c -constants`:
//! `ExperienceOrb` whole (both constructors, `tick`, `followNearbyPlayer`,
//! `scanForMerges`, `award`, `awardWithDirection`, `tryMergeToExisting`,
//! `canMerge` x2, `merge`, `setUnderwaterMovement`, `hurtServer`,
//! `playerTouch`, `repairPlayerItems`, `getExperienceValue`, `getIcon`,
//! `add/readAdditionalSaveData`, `getBlockPosBelowThatAffectsMyMovement`,
//! `getDefaultGravity`, `getAirDrag`), `EntityTypes.EXPERIENCE_ORB`'s builder
//! (offsets 2151-2188), `Player.aiStep` (offsets 78-255, the touch),
//! `LivingEntity.take`, `ServerPlayer.take`,
//! `EnchantmentHelper.getRandomItemWith` / `modifyDurabilityToRepairFromXp`,
//! `Vec3.normalize` and the experience sources listed under
//! [`sources`].
//!
//! # What a client is sent for one orb, and which of it is drawn
//!
//! ```text
//!   spawn          bundle[add_entity, set_entity_data{8: value}]
//!   every 20 ticks move_entity_* / set_entity_motion (updateInterval 20)
//!   pickup         take_item_entity(orb, player, 1)  -> trackers of the orb
//!   count reaches 0 remove_entities                  -> trackers of the orb
//!   same doTick    set_experience                    -> the player only
//!   next tick      set_entity_data{18: score}        -> trackers + the player
//! ```
//!
//! An orb from [`award`] goes through `ExperienceOrb(Level, Vec3, Vec3, int)`,
//! which draws **a yaw and three velocity doubles from the orb's own
//! unseeded random** (offsets 20-102): its `add_entity` rotation and movement
//! are vanilla-against-vanilla noise. An orb from `/summon` goes through
//! `EntityType.create` -> `ExperienceOrb(EntityType, Level)`, which draws
//! nothing but the uuid — so a summoned orb's whole flight is a deterministic
//! function of where it was put and where the player stands, and that is the
//! form the corpus grades.
//!
//! # Not ported, each named
//!
//! * `unstuckIfPossible` (`Level.findFreePosition`) — reached only when a
//!   freshly constructed orb's box already collides. Left where it is.
//! * `moveTowardsClosestSpace` (tick offsets 188-256) — reached only by an
//!   orb inside a block with no player to follow. Left where it is, and
//!   `needsSync` is not raised.
//! * Lava: the orb is not hurt by fire (`Entity.baseTick`'s `lavaHurt`); the
//!   lava *bounce* (offsets 79-149) is ported.
//! * `applyEffectsFromBlocks` — none of the blocks that have an inside effect
//!   are reached by a scenario.
//! * Saving an orb to disk — this server persists no entities.
//! * **Mending wiring.** [`repair_player_items`] is the whole of
//!   `repairPlayerItems` as a pure function, but a player's equipment lives
//!   on its connection thread and is not visible from the clock, so the
//!   touch passes no candidates and the orb's whole value goes to
//!   `giveExperiencePoints` — which is exactly vanilla for a player holding
//!   no damaged Mending item.

use crate::collide::{Aabb, CollisionShapes, Vec3};
use crate::edits::Blocks;
use crate::entities::{Entities, Entity, KindIds};
use crate::movement::WorldCollision;
use crate::movement_props::MovementProps;
use crate::physics::{self, Body, MoverType, Profile};
use crate::tracker::{TrackedPlayer, Tracker};
use rustmc_worldgen::rng::{LegacyRandomSource, RandomSource};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// `ExperienceOrb.LIFETIME`.
pub const LIFETIME: i32 = 6000;
/// `ExperienceOrb.ENTITY_SCAN_PERIOD`.
pub const ENTITY_SCAN_PERIOD: i32 = 20;
/// `ExperienceOrb.MAX_FOLLOW_DIST`, compared squared (`64.0d`) for a player
/// already followed and passed as `8.0d` to `getNearestPlayer`.
pub const MAX_FOLLOW_DIST: f64 = 8.0;
/// `ExperienceOrb.ORB_GROUPS_PER_AREA` — the `nextInt(40)` and `% 40`.
pub const ORB_GROUPS_PER_AREA: i32 = 40;
/// `ExperienceOrb.DEFAULT_HEALTH`.
pub const DEFAULT_HEALTH: i32 = 5;

/// `Avatar`'s standing eye height, the `float` `1.62f`, which
/// `followNearbyPlayer` widens and halves.
const PLAYER_EYE_HEIGHT: f32 = 1.62;
/// `EntityTypes.PLAYER`'s standing dimensions, `sized(0.6f, 1.8f)`.
const PLAYER_WIDTH: f64 = 0.6000000238418579;
const PLAYER_HEIGHT: f64 = 1.7999999523162842;

/// `ExperienceOrb.getExperienceValue(int)`: the largest of
/// `2477, 1237, 617, 307, 149, 73, 37, 17, 7, 3` not above `amount`, else 1.
#[must_use]
pub const fn experience_value(amount: i32) -> i32 {
    if amount >= 2477 {
        2477
    } else if amount >= 1237 {
        1237
    } else if amount >= 617 {
        617
    } else if amount >= 307 {
        307
    } else if amount >= 149 {
        149
    } else if amount >= 73 {
        73
    } else if amount >= 37 {
        37
    } else if amount >= 17 {
        17
    } else if amount >= 7 {
        7
    } else if amount >= 3 {
        3
    } else {
        1
    }
}

/// `ExperienceOrb.getIcon()` — the client's sprite index, 0..=10, on the same
/// thresholds. Server-side nothing reads it; it is here because it is the
/// other half of the same table and a test pins both.
#[must_use]
pub const fn icon(value: i32) -> i32 {
    const T: [i32; 10] = [2477, 1237, 617, 307, 149, 73, 37, 17, 7, 3];
    let mut i = 0;
    while i < T.len() {
        if value >= T[i] {
            #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
            return 10 - i as i32;
        }
        i += 1;
    }
    0
}

/// The split [`award`] makes, without the merge: `awardWithDirection`'s loop.
#[must_use]
pub fn split(mut amount: i32) -> Vec<i32> {
    let mut out = Vec::new();
    while amount > 0 {
        let v = experience_value(amount);
        amount -= v;
        out.push(v);
    }
    out
}

/// `EntityTypes.EXPERIENCE_ORB`'s physics: `sized(0.5f, 0.5f)`,
/// `getDefaultGravity` `ldc2_w 0.03d`, `getAirDrag` `0.98f`.
pub const PROFILE: Profile = Profile {
    width: 0.5,
    height: 0.5,
    max_up_step: 0.0,
    gravity: 0.03,
    air_drag: 0.98,
    entity_bounciness: 0.0,
    client_authoritative: false,
    // `Entity.canSimulateMovement` is `isLocalInstanceAuthoritative()`, true
    // on the server for anything but a player.
    can_simulate_movement: true,
    living: false,
};

/// The fields of one `ExperienceOrb` beyond what [`Entity`] carries.
#[derive(Debug, Clone)]
pub struct Orb {
    pub value: i32,
    /// `count` — how many pickups this orb is worth; merges add to it.
    pub count: i32,
    pub age: i32,
    pub health: i32,
    /// `followingPlayer`, as the player's entity id.
    pub following: Option<i32>,
    /// `Entity.tickCount`.
    pub tick_count: i32,
    /// The physics state `Entity.move` reads back next tick, kept whole so
    /// `mainSupportingBlockPos` survives between ticks.
    pub body: Body,
    /// `Entity.random`.
    pub random: LegacyRandomSource,
    /// `SynchedEntityData.isDirty()` for `DATA_VALUE`: `setValue` in the
    /// constructor (award) or in `readAdditionalSaveData` (summon) marks it,
    /// the pairing does not clear it, and the next `sendChanges` sends the
    /// value a second time.
    pub data_dirty: bool,
}

/// Every live orb, by entity id.
#[derive(Debug, Default)]
pub struct Orbs {
    live: Mutex<HashMap<i32, Orb>>,
}

impl Orbs {
    #[must_use]
    pub fn shared() -> &'static Self {
        static S: OnceLock<Orbs> = OnceLock::new();
        S.get_or_init(Orbs::default)
    }

    pub fn insert(&self, id: i32, o: Orb) {
        if let Ok(mut m) = self.live.lock() {
            m.insert(id, o);
        }
    }

    #[must_use]
    pub fn get(&self, id: i32) -> Option<Orb> {
        self.live.lock().ok()?.get(&id).cloned()
    }

    pub fn remove(&self, id: i32) -> Option<Orb> {
        self.live.lock().ok()?.remove(&id)
    }

    pub fn with<T>(&self, id: i32, f: impl FnOnce(&mut Orb) -> T) -> Option<T> {
        self.live.lock().ok()?.get_mut(&id).map(f)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.live.lock().map_or(0, |m| m.len())
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// `EntityDimensions.makeBoundingBox` for an orb at `p`.
#[must_use]
pub fn orb_box(p: Vec3) -> Aabb {
    PROFILE.bounding_box(p)
}

/// `AABB.intersects(AABB)`: strict on all six faces.
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

/// `Vec3.normalize()`: `d = Math.sqrt(x*x + y*y + z*z); d < 9.999999747378752E-6
/// ? ZERO : (x/d, y/d, z/d)` — the threshold is the float `1.0E-5f` widened.
#[must_use]
pub fn normalize(v: Vec3) -> Vec3 {
    let d = (v.x * v.x + v.y * v.y + v.z * v.z).sqrt();
    if d < f64::from(1.0E-5_f32) {
        Vec3::ZERO
    } else {
        Vec3::new(v.x / d, v.y / d, v.z / d)
    }
}

/// A player as the orb sees one: where it stands and who it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Seer {
    pub entity_id: i32,
    pub pos: (f64, f64, f64),
}

impl From<&TrackedPlayer> for Seer {
    fn from(p: &TrackedPlayer) -> Self {
        Self {
            entity_id: p.entity_id,
            pos: p.pos,
        }
    }
}

/// `Level.getNearestPlayer(x, y, z, 8.0, NO_SPECTATORS)`: the first strictly
/// nearest of `players` (in level order) with `distSqr < 64`.
#[must_use]
pub fn nearest_player(at: Vec3, players: &[Seer]) -> Option<Seer> {
    let mut best: Option<(f64, Seer)> = None;
    for p in players {
        let (dx, dy, dz) = (p.pos.0 - at.x, p.pos.1 - at.y, p.pos.2 - at.z);
        let d = dx * dx + dy * dy + dz * dz;
        if d < MAX_FOLLOW_DIST * MAX_FOLLOW_DIST && best.is_none_or(|(b, _)| d < b) {
            best = Some((d, *p));
        }
    }
    best.map(|(_, p)| p)
}

/// `followNearbyPlayer()`, offsets 0-183, on the orb's state and movement.
///
/// ```text
///   if (following == null || following.isSpectator() || following.distanceToSqr(this) > 64)
///       { p = getNearestPlayer(this, 8.0); following = (p != null && !spectator && !dying) ? p : null; }
///   if (following != null) {
///       Vec3 d = (p.x - x, p.y + p.getEyeHeight() / 2.0 - y, p.z - z);
///       double l = d.lengthSqr(); double f = 1.0 - Math.sqrt(l) / 8.0;
///       setDeltaMovement(getDeltaMovement().add(d.normalize().scale(f * f * 0.1)));
///   }
/// ```
pub fn follow_nearby_player(o: &mut Orb, players: &[Seer]) {
    let pos = o.body.pos;
    let current = o
        .following
        .and_then(|id| players.iter().find(|p| p.entity_id == id).copied());
    let keep = current.filter(|p| {
        let (dx, dy, dz) = (pos.x - p.pos.0, pos.y - p.pos.1, pos.z - p.pos.2);
        dx * dx + dy * dy + dz * dz <= 64.0
    });
    let target = if keep.is_some() {
        keep
    } else {
        nearest_player(pos, players)
    };
    o.following = target.map(|p| p.entity_id);
    if let Some(p) = target {
        let d = Vec3::new(
            p.pos.0 - pos.x,
            p.pos.1 + f64::from(PLAYER_EYE_HEIGHT) / 2.0 - pos.y,
            p.pos.2 - pos.z,
        );
        let l = d.x * d.x + d.y * d.y + d.z * d.z;
        let f = 1.0 - l.sqrt() / 8.0;
        let n = normalize(d);
        let s = f * f * 0.1;
        let m = o.body.movement;
        o.body.movement = Vec3::new(m.x + n.x * s, m.y + n.y * s, m.z + n.z * s);
    }
}

/// `setUnderwaterMovement()`: `(x * 0.99f, min(y + 5.0E-4f, 0.06f), z * 0.99f)`
/// with every constant the `float` widened.
fn set_underwater_movement(m: Vec3) -> Vec3 {
    Vec3::new(
        m.x * 0.990_000_009_536_743_2,
        (m.y + 5.000_000_237_487_257E-4).min(0.059_999_998_658_895_49),
        m.z * 0.990_000_009_536_743_2,
    )
}

/// The block reads one orb tick needs, separated from the world so the tick
/// is testable on a fake.
pub trait OrbWorld {
    /// `!level.noCollision(box)` — any block collision shape intersecting.
    fn collides(&self, b: &Aabb) -> bool;
    /// `isEyeInFluid(WATER)`: water at the eye whose surface is above the
    /// eye (`Entity.updateFluidOnEyes`).
    fn eye_in_water(&self, eye: Vec3) -> bool;
    /// `level.getFluidState(blockPosition()).is(LAVA)`.
    fn lava_at(&self, feet: Vec3) -> bool;
    /// `Entity.move(SELF, vec)`.
    fn move_body(&self, body: &mut Body, vec: Vec3);
    /// `getBlockState(getOnPos(0.999999f)).getBlock().getFriction()`.
    fn friction_below(&self, body: &Body) -> f32;
}

/// What one orb's tick did that the caller has to act on.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TickOut {
    /// `discard()` of this orb — its age reached [`LIFETIME`].
    pub expired: bool,
    /// Orbs `scanForMerges` absorbed into this one and discarded.
    pub merged_away: Vec<i32>,
}

/// `ExperienceOrb.tick()`, server side, for one orb. `others` is every other
/// live orb as `(id, position, value)`, for `scanForMerges`.
///
/// `tickCount` is incremented first, as `ServerLevel.tickNonPassenger` does
/// before calling `tick()`; so the first tick runs with `tickCount == 1`, and
/// `tickCount % 20 == 1` makes the first merge scan the orb's first tick.
pub fn tick_one(
    id: i32,
    o: &mut Orb,
    world: &impl OrbWorld,
    players: &[Seer],
    others: &[(i32, Vec3, i32, i32, i32)],
) -> TickOut {
    let mut out = TickOut::default();
    o.tick_count += 1;
    // 34-53: colliding before anything moves.
    let colliding = world.collides(&orb_box(o.body.pos));
    // 54-79: water, else gravity unless colliding.
    let eye = Vec3::new(
        o.body.pos.x,
        o.body.pos.y + f64::from(0.425_f32),
        o.body.pos.z,
    );
    if world.eye_in_water(eye) {
        o.body.movement = set_underwater_movement(o.body.movement);
    } else if !colliding {
        physics::apply_gravity(&mut o.body, PROFILE.gravity);
    }
    // 79-149: lava pops the orb with four unseeded draws.
    if world.lava_at(o.body.pos) {
        let a = o.random.next_f32();
        let b = o.random.next_f32();
        let c = o.random.next_f32();
        let d = o.random.next_f32();
        o.body.movement = Vec3::new(
            f64::from((a - b) * 0.2),
            0.200_000_002_980_232_24,
            f64::from((c - d) * 0.2),
        );
    }
    // 152-164.
    if o.tick_count % ENTITY_SCAN_PERIOD == 1 {
        let area = inflate(&orb_box(o.body.pos), 0.5, 0.5, 0.5);
        for &(other, at, value, count, age) in others {
            // `canMerge(this, other)`: `other != this && canMerge(other, getId(), getValue())`
            // -> `!other.isRemoved() && (other.getId() - id) % 40 == 0 && other.getValue() == value`.
            if other == id
                || (other.wrapping_sub(id)) % ORB_GROUPS_PER_AREA != 0
                || value != o.value
                || !intersects(&orb_box(at), &area)
            {
                continue;
            }
            // `merge(other)`.
            o.count = o.count.wrapping_add(count);
            o.age = o.age.min(age);
            out.merged_away.push(other);
        }
    }
    // 167-168.
    follow_nearby_player(o, players);
    // 171-256: `moveTowardsClosestSpace` — not ported, see the module docs.
    // 259-279.
    let dy = o.body.movement.y;
    let vec = o.body.movement;
    world.move_body(&mut o.body, vec);
    // 282-328: air drag, times the friction of the block below when grounded,
    // applied to all three components (`Vec3.scale`).
    let mut f = PROFILE.air_drag;
    if o.body.on_ground {
        f *= world.friction_below(&o.body);
    }
    let s = f64::from(f);
    let m = o.body.movement;
    o.body.movement = Vec3::new(m.x * s, m.y * s, m.z * s);
    // 331-376: the bounce, on the y the orb had *before* the move.
    if o.body.vertical_collision_below && dy < -PROFILE.gravity {
        let m = o.body.movement;
        o.body.movement = Vec3::new(m.x, -dy * 0.4, m.z);
    }
    // 379-400.
    o.age += 1;
    if o.age >= LIFETIME {
        out.expired = true;
    }
    out
}

/// The real world behind [`OrbWorld`].
pub struct LiveWorld<'a, W: Blocks + ?Sized> {
    pub world: &'a W,
    pub shapes: &'a CollisionShapes,
    pub props: &'a MovementProps,
}

impl<W: Blocks + ?Sized> OrbWorld for LiveWorld<'_, W> {
    fn collides(&self, b: &Aabb) -> bool {
        crate::collide::collect_block_collisions(self.shapes, &WorldCollision(self.world), *b)
            // A state with no shape row is treated as colliding — the orb then
            // takes the no-gravity branch, which is the conservative reading.
            .map_or(true, |v| !v.is_empty())
    }

    fn eye_in_water(&self, eye: Vec3) -> bool {
        // `updateFluidOnEyes`: `eyeY = getEyeY() - 0.11111111F`; the fluid at
        // `BlockPos.containing(x, eyeY, z)`; on it if `pos.y + height > eyeY`.
        let ey = eye.y - f64::from(0.111_111_11_f32);
        #[allow(clippy::cast_possible_truncation)]
        let pos = (
            eye.x.floor() as i32,
            ey.floor() as i32,
            eye.z.floor() as i32,
        );
        water_height(
            &self.world.state_at(pos),
            &self.world.state_at((pos.0, pos.1 + 1, pos.2)),
        )
        .is_some_and(|h| f64::from(pos.1) + f64::from(h) > ey)
    }

    fn lava_at(&self, feet: Vec3) -> bool {
        #[allow(clippy::cast_possible_truncation)]
        let pos = (
            feet.x.floor() as i32,
            feet.y.floor() as i32,
            feet.z.floor() as i32,
        );
        crate::blockentity::block_of(&self.world.state_at(pos)) == "minecraft:lava"
    }

    fn move_body(&self, body: &mut Body, vec: Vec3) {
        let _ = physics::move_entity(
            self.shapes,
            self.props,
            &WorldCollision(self.world),
            &PROFILE,
            body,
            MoverType::SelfMove,
            vec,
        );
    }

    fn friction_below(&self, body: &Body) -> f32 {
        let wc = WorldCollision(self.world);
        use crate::collide::CollisionBlocks;
        let p = physics::on_pos(
            body,
            0.999_999_f32,
            |q| self.props.is_fence(wc.state_id_at(q)),
            |q| self.props.is_wall_or_gate(wc.state_id_at(q)),
        );
        self.props.friction(wc.state_id_at(p))
    }
}

/// `FluidState.getHeight` for water, from the state strings: a source or a
/// falling column is 8 of 9 (`8 / 9.0f`), level `n` is `(8 - n) / 9.0f`, and
/// water with water above it is a full block. `None` for no water; a
/// waterlogged block counts as a source.
#[must_use]
pub fn water_height(state: &str, above: &str) -> Option<f32> {
    let block = crate::blockentity::block_of(state);
    let is_water = |s: &str| {
        crate::blockentity::block_of(s) == "minecraft:water"
            || crate::placement::property_of(s, "waterlogged") == Some("true")
    };
    if !is_water(state) {
        return None;
    }
    if is_water(above) {
        return Some(1.0);
    }
    let amount = if block == "minecraft:water" {
        match crate::placement::property_of(state, "level").and_then(|l| l.parse::<i32>().ok()) {
            Some(l) if l < 8 => 8 - l,
            _ => 8,
        }
    } else {
        8
    };
    #[allow(clippy::cast_precision_loss)]
    Some(amount as f32 / 9.0)
}

// ---- spawning ----

/// Everything the spawn paths need from the level.
pub struct Level<'a> {
    pub entities: &'a Entities,
    pub tracker: &'a Tracker,
    pub players: &'a [TrackedPlayer],
    pub kinds: &'a KindIds,
}

fn add_fresh(level: &Level<'_>, e: Entity, o: Orb) -> i32 {
    let id = e.id;
    level.entities.insert(e.clone());
    Orbs::shared().insert(id, o);
    level.tracker.add(&e, level.players, level.kinds);
    id
}

fn record(id: i32, uuid: u128, body: &Body, y_rot: f32, value: i32) -> Entity {
    Entity {
        id,
        uuid,
        kind: crate::entities::EXPERIENCE_ORB,
        x: body.pos.x,
        y: body.pos.y,
        z: body.pos.z,
        y_rot,
        x_rot: 0.0,
        y_head_rot: y_rot,
        movement: (body.movement.x, body.movement.y, body.movement.z),
        on_ground: body.on_ground,
        data: 0,
        swinging: false,
        swing_time: 0,
        swinging_arm: None,
        synched: crate::synched::Synched::ExperienceOrb { value },
    }
}

/// `new ExperienceOrb(level, pos, dir, value)` then `addFreshEntity`.
///
/// Offsets 20-190: `setYRot(random.nextFloat() * 360)`; a velocity of
/// `((r - 0.1*... ` — `(nextDouble()*0.2 - 0.1) * 2`, `nextDouble()*0.2*2`,
/// `(nextDouble()*0.2 - 0.1) * 2` — flipped whole when it points against a
/// non-zero `dir`; the position pushed `size * 0.5` along `dir`; then
/// `setValue`. `Entity.<init>` drew the uuid before any of it.
pub fn spawn_with_direction(level: &Level<'_>, pos: Vec3, dir: Vec3, value: i32) -> i32 {
    let mut random = crate::dropitem::unique_random();
    let uuid = crate::dropitem::insecure_uuid(&mut random);
    let y_rot = random.next_f32() * 360.0;
    let a = (random.next_f64() * 0.2 - 0.1) * 2.0;
    let b = random.next_f64() * 0.2 * 2.0;
    let c = (random.next_f64() * 0.2 - 0.1) * 2.0;
    let mut v = Vec3::new(a, b, c);
    if dir.length_sqr() > 0.0 && dir.x * v.x + dir.y * v.y + dir.z * v.z < 0.0 {
        v = Vec3::new(-v.x, -v.y, -v.z);
    }
    // `getBoundingBox().getSize()`: the mean of the three extents, 0.5.
    let size = 0.5;
    let n = normalize(dir);
    let at = Vec3::new(
        pos.x + n.x * size * 0.5,
        pos.y + n.y * size * 0.5,
        pos.z + n.z * size * 0.5,
    );
    let mut body = Body::at(at);
    body.movement = v;
    let id = level.entities.next_id();
    let e = record(id, uuid, &body, y_rot, value);
    add_fresh(
        level,
        e,
        Orb {
            value,
            count: 1,
            age: 0,
            health: DEFAULT_HEALTH,
            following: None,
            tick_count: 0,
            body,
            random,
            data_dirty: value != 0,
        },
    )
}

/// `ExperienceOrb.award(level, pos, amount)` — the call every source makes.
///
/// `awardWithDirection(level, pos, Vec3.ZERO, amount)`: split `amount` by
/// [`experience_value`], and for each piece first try
/// `tryMergeToExisting` — one `level.getRandom().nextInt(40)` draw, then the
/// first live orb in a 1x1x1 box around `pos` whose `(id - r) % 40 == 0` and
/// whose value matches gets `count++` and `age = 0` — else spawn a new orb.
///
/// The merge draw is from the **level's** unseeded random, so whether a
/// second award beside an existing orb merges is vanilla-against-vanilla
/// noise at a rate of about one in forty per candidate.
pub fn award(level: &Level<'_>, pos: Vec3, amount: i32) -> Vec<i32> {
    award_with_direction(level, pos, Vec3::ZERO, amount)
}

/// `ExperienceOrb.awardWithDirection`. Returns the ids of new orbs.
pub fn award_with_direction(level: &Level<'_>, pos: Vec3, dir: Vec3, amount: i32) -> Vec<i32> {
    let mut out = Vec::new();
    for v in split(amount) {
        if try_merge_to_existing(level.entities, pos, v) {
            continue;
        }
        out.push(spawn_with_direction(level, pos, dir, v));
    }
    out
}

fn try_merge_to_existing(entities: &Entities, pos: Vec3, value: i32) -> bool {
    let area = Aabb::new(
        pos.x - 0.5,
        pos.y - 0.5,
        pos.z - 0.5,
        pos.x + 0.5,
        pos.y + 0.5,
        pos.z + 0.5,
    );
    let r = crate::mob::level_random(|g| g.next_i32_bound(ORB_GROUPS_PER_AREA));
    for e in entities.all_in_tick_order() {
        if e.kind != crate::entities::EXPERIENCE_ORB {
            continue;
        }
        if !intersects(&orb_box(Vec3::new(e.x, e.y, e.z)), &area) {
            continue;
        }
        if (e.id.wrapping_sub(r)) % ORB_GROUPS_PER_AREA != 0 {
            continue;
        }
        let merged = Orbs::shared().with(e.id, |o| {
            if o.value == value {
                o.count = o.count.wrapping_add(1);
                o.age = 0;
                true
            } else {
                false
            }
        });
        if merged == Some(true) {
            return true;
        }
    }
    false
}

/// A `/summon minecraft:experience_orb` request, resolved enough to queue.
#[derive(Debug, Clone, PartialEq)]
pub struct SummonOrb {
    /// `execute at <player>`: the player whose position `~` is relative to.
    pub relative_to: Option<String>,
    /// `(value, relative)` per axis.
    pub at: [(f64, bool); 3],
    /// `Value`, `Count`, `Age`, `Health` from the tag, with the defaults
    /// `readAdditionalSaveData` falls back to.
    pub value: i32,
    pub count: i32,
    pub age: i32,
    pub health: i32,
}

/// `SummonCommand.createEntity` for an orb: `EntityType.create` (the
/// two-argument constructor — **no yaw, no velocity draw**), `load` of the
/// tag, `snapTo(x, y, z, 0, 0)`, `addFreshEntity`.
pub fn summon(level: &Level<'_>, at: Vec3, s: &SummonOrb) -> i32 {
    let mut random = crate::dropitem::unique_random();
    let uuid = crate::dropitem::insecure_uuid(&mut random);
    let body = Body::at(at);
    let id = level.entities.next_id();
    let e = record(id, uuid, &body, 0.0, s.value);
    add_fresh(
        level,
        e,
        Orb {
            value: s.value,
            count: s.count,
            age: s.age,
            health: s.health,
            following: None,
            tick_count: 0,
            body,
            random,
            data_dirty: s.value != 0,
        },
    )
}

/// Parse the tag of a summon: `{Value:3,Count:2}`. Only the four keys
/// `readAdditionalSaveData` reads are honoured; numeric suffixes (`3s`, `3b`)
/// are accepted, as any `NumericTag` is. `Value`, `Age` and `Health` go
/// through `getShortOr`, so they wrap as a `short`; `Count` is
/// `ExtraCodecs.POSITIVE_INT` and falls back to 1 when absent or below 1.
///
/// # Errors
/// On a tag that is not a flat compound of numbers.
pub fn parse_tag(tag: &str) -> Result<(i32, i32, i32, i32), String> {
    let (mut value, mut count, mut age, mut health) = (0, 1, 0, DEFAULT_HEALTH);
    let t = tag.trim();
    if t.is_empty() {
        return Ok((value, count, age, health));
    }
    let inner = t
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
        .ok_or_else(|| format!("not a compound: {t}"))?;
    for kv in inner.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let (k, v) = kv
            .split_once(':')
            .ok_or_else(|| format!("no ':' in {kv}"))?;
        let v = v.trim().trim_end_matches(['s', 'S', 'b', 'B', 'l', 'L']);
        let n: i64 = v.parse().map_err(|e| format!("{k}: {e}"))?;
        #[allow(clippy::cast_possible_truncation)]
        let short = i32::from(n as i16);
        match k.trim() {
            "Value" => value = short,
            "Age" => age = short,
            "Health" => health = short,
            "Count" => {
                count = i32::try_from(n).ok().filter(|&c| c >= 1).unwrap_or(1);
            }
            other => return Err(format!("unsupported key {other}")),
        }
    }
    Ok((value, count, age, health))
}

/// `sendDirtyEntityData` for an orb: its value, once, after the pairing.
#[must_use]
pub fn drain_dirty(id: i32) -> Option<crate::mob::Dirty> {
    let value = Orbs::shared().with(id, |o| {
        let d = o.data_dirty;
        o.data_dirty = false;
        d.then_some(o.value)
    })??;
    let values = crate::synched::Synched::ExperienceOrb { value }.non_default_values()?;
    Some(crate::mob::Dirty {
        data: crate::synched::set_entity_data_body(id, &values).ok(),
        attributes: None,
        needs_sync: false,
    })
}

// ---- the level tick ----

fn discard(id: i32, level: &Level<'_>) {
    Orbs::shared().remove(id);
    level.entities.remove(id);
    level.tracker.remove(id, level.players);
}

/// Every live orb's `tick()`, in the level's entity-tick order.
///
/// Called from `crate::status::listen` beside `crate::mob::tick_all` — the
/// `entityTickList.forEach` phase, after `ServerChunkCache.tick`'s broadcast,
/// so an orb's move goes out in the next tick's `sendChanges`.
pub fn tick_all<W: Blocks + ?Sized>(
    level: &Level<'_>,
    world: &W,
    shapes: Option<&CollisionShapes>,
    props: Option<&MovementProps>,
) {
    let (Some(shapes), Some(props)) = (shapes, props) else {
        return;
    };
    if Orbs::shared().is_empty() {
        return;
    }
    let live = LiveWorld {
        world,
        shapes,
        props,
    };
    let seers: Vec<Seer> = level.players.iter().map(Seer::from).collect();
    let order: Vec<i32> = level
        .entities
        .all_in_tick_order()
        .into_iter()
        .filter(|e| e.kind == crate::entities::EXPERIENCE_ORB)
        .map(|e| e.id)
        .collect();
    for id in order {
        let Some(mut o) = Orbs::shared().get(id) else {
            continue;
        };
        let others: Vec<(i32, Vec3, i32, i32, i32)> = if o.tick_count % ENTITY_SCAN_PERIOD == 0 {
            // Only gathered on the tick the scan will run (tick_count is
            // incremented inside `tick_one`).
            let snapshot = Orbs::shared()
                .live
                .lock()
                .map(|m| {
                    m.iter()
                        .map(|(k, v)| (*k, v.body.pos, v.value, v.count, v.age))
                        .collect()
                })
                .unwrap_or_default();
            snapshot
        } else {
            Vec::new()
        };
        let out = tick_one(id, &mut o, &live, &seers, &others);
        for gone in &out.merged_away {
            discard(*gone, level);
        }
        let body = o.body;
        if out.expired {
            discard(id, level);
            continue;
        }
        Orbs::shared().insert(id, o);
        level.entities.update(id, |e| {
            e.x = body.pos.x;
            e.y = body.pos.y;
            e.z = body.pos.z;
            e.movement = (body.movement.x, body.movement.y, body.movement.z);
            e.on_ground = body.on_ground;
        });
    }
}

// ---- the touch ----

/// `Player.aiStep` offsets 78-255 for one player, and `playerTouch` for the
/// orb it picks. Returns the points given and the level-up sounds they made,
/// for the caller to play.
///
/// ```text
///   if (getHealth() > 0 && !isSpectator()) {
///       AABB box = getBoundingBox().inflate(1.0, 0.5, 1.0);   // not riding
///       List<Entity> orbs = [e in getEntities(this, box) if e.is(EXPERIENCE_ORB)];
///       if (!orbs.isEmpty()) touch(Util.getRandom(orbs, random));
///   }
/// ```
///
/// One orb per player per tick, chosen with the **player's** unseeded random
/// when several touch at once. `playerTouch`: only while `takeXpDelay == 0`;
/// then `takeXpDelay = 2`, `take(orb, 1)` (a `take_item_entity` to the orb's
/// trackers), `repairPlayerItems`, `giveExperiencePoints(remaining)`,
/// `count--`, and `discard()` at zero (a `remove_entities` to the same
/// trackers, immediately).
pub fn touch(
    level: &Level<'_>,
    player: &TrackedPlayer,
    xp: &mut crate::experience::Experience,
    pick: impl FnOnce(usize) -> usize,
) -> Vec<crate::experience::LevelUp> {
    let p = player.pos;
    let w = PLAYER_WIDTH / 2.0;
    let pbox = Aabb::new(p.0 - w, p.1, p.2 - w, p.0 + w, p.1 + PLAYER_HEIGHT, p.2 + w);
    let reach = inflate(&pbox, 1.0, 0.5, 1.0);
    let orbs: Vec<i32> = level
        .entities
        .all_in_tick_order()
        .into_iter()
        .filter(|e| e.kind == crate::entities::EXPERIENCE_ORB)
        .filter(|e| intersects(&orb_box(Vec3::new(e.x, e.y, e.z)), &reach))
        .map(|e| e.id)
        .collect();
    if orbs.is_empty() {
        return Vec::new();
    }
    let chosen = orbs[pick(orbs.len()).min(orbs.len() - 1)];
    if xp.take_xp_delay != 0 {
        return Vec::new();
    }
    let Some(value) = Orbs::shared().get(chosen).map(|o| o.value) else {
        return Vec::new();
    };
    xp.take_xp_delay = 2;
    // `LivingEntity.take`: `ClientboundTakeItemEntityPacket(orb, player, 1)`
    // through `sendToTrackingPlayers(orb, ...)`.
    if let Ok(body) = take_item_entity_body(chosen, player.entity_id, 1) {
        level.tracker.send_to_tracking_players(
            chosen,
            level.players,
            rustmc_proto::ids::play::cb::TAKE_ITEM_ENTITY,
            &body,
        );
    }
    let remaining = repair_player_items(&mut [], value, |_| 0);
    let sounds = if remaining > 0 {
        xp.give_points(remaining)
    } else {
        Vec::new()
    };
    let gone = Orbs::shared()
        .with(chosen, |o| {
            o.count -= 1;
            o.count == 0
        })
        .unwrap_or(true);
    if gone {
        discard(chosen, level);
    }
    sounds
}

/// `ClientboundTakeItemEntityPacket`: three varints — the item, the
/// collector, the amount.
///
/// # Errors
/// Only on a failing `Vec` write, which cannot happen.
pub fn take_item_entity_body(item: i32, collector: i32, amount: i32) -> std::io::Result<Vec<u8>> {
    let mut b = Vec::new();
    rustmc_proto::varint::write_varint(&mut b, item)?;
    rustmc_proto::varint::write_varint(&mut b, collector)?;
    rustmc_proto::varint::write_varint(&mut b, amount)?;
    Ok(b)
}

/// One equipped stack that `repairPlayerItems` may mend: damaged and
/// carrying an enchantment with a `minecraft:repair_with_xp` effect.
#[derive(Debug, Clone, PartialEq)]
pub struct Mendable {
    /// `ItemStack.getDamageValue()`.
    pub damage: i32,
    /// `repair_with_xp`'s factor as a multiplier on the orb's value — 2 for
    /// `minecraft:mending`, whose effect is `{"type": "multiply", "factor": 2.0}`.
    pub factor: f32,
}

/// `ExperienceOrb.repairPlayerItems(player, amount)`, offsets 0-101:
///
/// ```text
///   Optional<EnchantedItemInUse> it = EnchantmentHelper.getRandomItemWith(REPAIR_WITH_XP, player, ItemStack::isDamaged);
///   if (it.isPresent()) {
///       int durability = modifyDurabilityToRepairFromXp(level, stack, amount);   // max(0, (int) (amount * factor))
///       int repaired = Math.min(durability, stack.getDamageValue());
///       stack.setDamageValue(stack.getDamageValue() - repaired);
///       if (repaired > 0) {
///           int remaining = amount - repaired * amount / durability;
///           if (remaining > 0) return repairPlayerItems(player, remaining);
///       }
///       return 0;
///   }
///   return amount;
/// ```
///
/// `candidates` are the equipped stacks in `EquipmentSlot.VALUES` order;
/// `pick(n)` is `Util.getRandomSafe`'s `player.random.nextInt(n)`. A
/// candidate stops qualifying once its damage reaches 0.
pub fn repair_player_items(
    candidates: &mut [Mendable],
    amount: i32,
    mut pick: impl FnMut(usize) -> usize,
) -> i32 {
    let mut amount = amount;
    loop {
        let live: Vec<usize> = candidates
            .iter()
            .enumerate()
            .filter(|(_, c)| c.damage > 0)
            .map(|(i, _)| i)
            .collect();
        if live.is_empty() {
            return amount;
        }
        let i = live[pick(live.len()).min(live.len() - 1)];
        #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
        let durability = 0.max((amount as f32 * candidates[i].factor) as i32);
        let repaired = durability.min(candidates[i].damage);
        candidates[i].damage -= repaired;
        if repaired > 0 {
            let remaining = amount - repaired * amount / durability;
            if remaining > 0 {
                amount = remaining;
                continue;
            }
        }
        return 0;
    }
}

/// The experience sources other than the orb's own spawn call. Each is the
/// amount computation, transcribed; the source's own mechanic decides when
/// to call it and hands the result to [`award`] (or, for breeding, to
/// [`spawn_with_direction`] with `Vec3::ZERO`, since `Animal` constructs the
/// orb directly and never merges).
pub mod sources {
    use rustmc_worldgen::rng::RandomSource;

    /// `AbstractFurnaceBlockEntity.createExperience(level, pos, count, xp)`:
    ///
    /// ```text
    ///   int i = Mth.floor((float) count * xp);
    ///   float f = Mth.frac((float) count * xp);
    ///   if (f != 0.0f && level.getRandom().nextFloat() < f) i++;
    ///   ExperienceOrb.award(level, pos, i);
    /// ```
    ///
    /// The draw happens **only when the product has a fraction**, so a
    /// recipe with whole experience (raw gold, 1.0) is deterministic and one
    /// with 0.7 (raw iron) is a coin toss vanilla makes against itself.
    pub fn furnace(count: i32, xp: f32, level_random: &mut impl RandomSource) -> i32 {
        #[allow(clippy::cast_precision_loss)]
        let p = count as f32 * xp;
        #[allow(clippy::cast_possible_truncation)]
        let mut i = p.floor() as i32;
        // `Mth.frac(float)`: `f - Mth.floor(f)`.
        #[allow(clippy::cast_precision_loss)]
        let f = p - i as f32;
        if f != 0.0 && level_random.next_f32() < f {
            i += 1;
        }
        i
    }

    /// `DropExperienceBlock` / `RedStoneOreBlock` / sculk ranges, from the
    /// `Blocks` lambdas and the subclasses' `spawnAfterBreak`:
    /// `(min, max)` of `UniformInt.of` or `ConstantInt.of` (`min == max`).
    ///
    /// ```text
    ///   coal_ore, deepslate_coal_ore            UniformInt(0, 2)
    ///   nether_gold_ore                         UniformInt(0, 1)
    ///   lapis_ore, deepslate_lapis_ore          UniformInt(2, 5)
    ///   nether_quartz_ore                       UniformInt(2, 5)
    ///   diamond_ore, deepslate_diamond_ore      UniformInt(3, 7)
    ///   emerald_ore, deepslate_emerald_ore      UniformInt(3, 7)
    ///   redstone_ore, deepslate_redstone_ore    UniformInt(1, 5)   RedStoneOreBlock.spawnAfterBreak
    ///   sculk                                   ConstantInt(1)     SculkBlock
    ///   sculk_sensor, sculk_shrieker,
    ///   sculk_catalyst                          ConstantInt(5)
    ///   gold/iron/copper ores (both)            ConstantInt(0)
    /// ```
    ///
    /// `calibrated_sculk_sensor` extends `SculkSensorBlock` and inherits its 5.
    #[must_use]
    pub fn block_range(block: &str) -> Option<(i32, i32)> {
        Some(match block.strip_prefix("minecraft:").unwrap_or(block) {
            "coal_ore" | "deepslate_coal_ore" => (0, 2),
            "nether_gold_ore" => (0, 1),
            "lapis_ore" | "deepslate_lapis_ore" | "nether_quartz_ore" => (2, 5),
            "diamond_ore" | "deepslate_diamond_ore" | "emerald_ore" | "deepslate_emerald_ore" => {
                (3, 7)
            }
            "redstone_ore" | "deepslate_redstone_ore" => (1, 5),
            "sculk" => (1, 1),
            "sculk_sensor" | "calibrated_sculk_sensor" | "sculk_shrieker" | "sculk_catalyst" => {
                (5, 5)
            }
            "gold_ore"
            | "deepslate_gold_ore"
            | "iron_ore"
            | "deepslate_iron_ore"
            | "copper_ore"
            | "deepslate_copper_ore" => (0, 0),
            _ => return None,
        })
    }

    /// `Block.tryDropExperience(level, pos, tool, provider)` up to the
    /// `popExperience` call: `provider.sample(level.getRandom())`, then
    /// `EnchantmentHelper.processBlockExperience` — `minecraft:silk_touch`'s
    /// `block_experience` effect is `set 0` — then pop if `> 0`.
    ///
    /// `UniformInt.sample` is `Mth.randomBetweenInclusive(r, min, max)` =
    /// `r.nextInt(max - min + 1) + min`; `ConstantInt.sample` draws nothing.
    /// `popExperience` then awards at `Vec3.atCenterOf(pos)` when the
    /// `block_drops` game rule is on. Only a **survival** break reaches it:
    /// `Block.playerDestroy` -> `dropResources` -> `spawnAfterBreak(..., true)`.
    pub fn block(range: (i32, i32), silk_touch: bool, level_random: &mut impl RandomSource) -> i32 {
        let (lo, hi) = range;
        let sampled = if lo == hi {
            lo
        } else {
            level_random.next_i32_bound(hi - lo + 1) + lo
        };
        if silk_touch {
            0
        } else {
            sampled
        }
    }

    /// `SpawnerBlock.spawnAfterBreak`: `15 + r.nextInt(15) + r.nextInt(15)`,
    /// with **no** silk-touch test.
    pub fn spawner(level_random: &mut impl RandomSource) -> i32 {
        let a = level_random.next_i32_bound(15);
        let b = level_random.next_i32_bound(15);
        15 + a + b
    }

    /// `Animal.finalizeSpawnChildFromBreeding` offsets 56-113: when
    /// `mob_drops` is on, `new ExperienceOrb(level, getX(), getY(), getZ(),
    /// getRandom().nextInt(7) + 1)` at the **parent's** feet, drawn from the
    /// parent's random, and `addFreshEntity` — no `award`, so no merge.
    pub fn breeding(parent_random: &mut impl RandomSource) -> i32 {
        parent_random.next_i32_bound(7) + 1
    }

    /// `GrindstoneMenu$4.getExperienceAmount(level)`: for each non-curse
    /// enchantment on either input, `enchantment.getMinCost(level)` summed;
    /// `l = ceil(total / 2.0)`; `l + random.nextInt(l)` (0 when `l == 0`,
    /// without drawing). `min_costs` is that per-enchantment list for both
    /// inputs.
    pub fn grindstone(min_costs: &[i32], level_random: &mut impl RandomSource) -> i32 {
        let total: i32 = min_costs.iter().sum();
        let l = (f64::from(total) / 2.0).ceil();
        #[allow(clippy::cast_possible_truncation)]
        let l = l as i32;
        if l <= 0 {
            return 0;
        }
        l + level_random.next_i32_bound(l)
    }

    /// `ThrownExperienceBottle.onHit`: `3 + random.nextInt(5) + random.nextInt(5)`
    /// from the level random, awarded at the bottle's position after
    /// `level_event 2002`.
    pub fn experience_bottle(level_random: &mut impl RandomSource) -> i32 {
        let a = level_random.next_i32_bound(5);
        let b = level_random.next_i32_bound(5);
        3 + a + b
    }

    /// `Animal.getBaseExperienceReward`: `1 + random.nextInt(3)` from the
    /// animal's own random. `LivingEntity.dropExperience` awards it at the
    /// animal's position when it was last hurt by a player within 100 ticks
    /// and `mob_drops` is on.
    pub fn animal_kill(animal_random: &mut impl RandomSource) -> i32 {
        1 + animal_random.next_i32_bound(3)
    }

    /// `Player.getBaseExperienceReward` — see `crate::death::base_experience_reward`.
    #[must_use]
    pub fn player_death(level: i32, keep_inventory: bool, spectator: bool) -> i32 {
        crate::death::base_experience_reward(level, keep_inventory, spectator)
    }
}

// ---- the furnace's experience ledger ----

/// `AbstractFurnaceBlockEntity.recipesUsed`, per furnace position:
/// `setRecipeUsed` adds one per completed smelt, and
/// `getRecipesToAwardAndPopExperience` turns each entry into
/// `createExperience(level, pos, count, recipe.experience())` and clears it.
///
/// Held beside the block entity rather than in `crate::cooking::FurnaceState`
/// (a `Copy` struct of four ints). Two differences, named:
///
/// * vanilla's map is a `Reference2IntOpenHashMap` keyed by `ResourceKey`
///   **identity**, so with two or more recipes the award order — and so which
///   `nextFloat` each fraction gets — follows identity hashes no other server
///   can reproduce; this keeps first-use order;
/// * the ledger is not written to the `RecipesUsed` tag, so a furnace's
///   unclaimed experience does not survive a restart here.
#[derive(Debug, Default)]
pub struct FurnaceLedger {
    used: Mutex<HashMap<crate::interact::BlockPos, Vec<(String, i32)>>>,
}

impl FurnaceLedger {
    #[must_use]
    pub fn shared() -> &'static Self {
        static S: OnceLock<FurnaceLedger> = OnceLock::new();
        S.get_or_init(FurnaceLedger::default)
    }

    /// `setRecipeUsed(holder)`: `recipesUsed.addTo(holder.id(), 1)`.
    pub fn record(&self, pos: crate::interact::BlockPos, recipe: &str) {
        if let Ok(mut m) = self.used.lock() {
            let v = m.entry(pos).or_default();
            if let Some(e) = v.iter_mut().find(|(id, _)| id == recipe) {
                e.1 += 1;
            } else {
                v.push((recipe.to_string(), 1));
            }
        }
    }

    /// Everything recorded at `pos`, cleared.
    pub fn take(&self, pos: crate::interact::BlockPos) -> Vec<(String, i32)> {
        self.used
            .lock()
            .ok()
            .and_then(|mut m| m.remove(&pos))
            .unwrap_or_default()
    }
}

/// `AbstractFurnaceBlockEntity.awardUsedRecipesAndPopExperience(player)`'s
/// experience half: for each recorded recipe, `createExperience` at the
/// **player's** position (`player.position()`, offsets 0-9) — a furnace's
/// experience appears at the taker's feet, not at the furnace. Returns the
/// new orbs' ids.
pub fn award_furnace(
    level: &Level<'_>,
    pos: crate::interact::BlockPos,
    at: Vec3,
    recipes: &crate::recipes::Recipes,
) -> Vec<i32> {
    let mut out = Vec::new();
    for (id, count) in FurnaceLedger::shared().take(pos) {
        let xp = recipes
            .all()
            .iter()
            .find(|r| r.id == id)
            .map_or(0.0, crate::recipes::Recipe::experience);
        let amount = crate::mob::level_random(|r| sources::furnace(count, xp, r));
        out.extend(award(level, at, amount));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_value_table_and_the_icon_table_share_thresholds() {
        assert_eq!(experience_value(0), 1);
        assert_eq!(experience_value(2), 1);
        assert_eq!(experience_value(3), 3);
        assert_eq!(experience_value(16), 7);
        assert_eq!(experience_value(10_000), 2477);
        assert_eq!(icon(1), 0);
        assert_eq!(icon(3), 1);
        assert_eq!(icon(2477), 10);
    }

    #[test]
    fn an_award_splits_greedily() {
        assert_eq!(split(10), vec![7, 3]);
        assert_eq!(split(11), vec![7, 3, 1]);
        assert_eq!(split(0), Vec::<i32>::new());
        assert_eq!(split(100), vec![73, 17, 7, 3]);
    }

    #[test]
    fn a_summon_tag_reads_shorts_and_a_positive_count() {
        assert_eq!(parse_tag("{Value:3}"), Ok((3, 1, 0, 5)));
        assert_eq!(parse_tag("{Value:3s,Count:2}"), Ok((3, 2, 0, 5)));
        assert_eq!(parse_tag("{Count:0}"), Ok((0, 1, 0, 5)));
        assert_eq!(parse_tag("{Value:40000}"), Ok((-25536, 1, 0, 5)));
        assert!(parse_tag("{Motion:[0d,1d,0d]}").is_err());
    }

    #[test]
    fn mending_doubles_the_value_and_returns_the_rest() {
        // 3 points, one item with 4 damage: durability 6, repaired 4,
        // remaining 3 - 4*3/6 = 1.
        let mut c = [Mendable {
            damage: 4,
            factor: 2.0,
        }];
        assert_eq!(repair_player_items(&mut c, 3, |_| 0), 1);
        assert_eq!(c[0].damage, 0);
        // No damaged item: the whole amount comes back.
        assert_eq!(repair_player_items(&mut c, 3, |_| 0), 3);
        // Enough damage to absorb all of it.
        let mut d = [Mendable {
            damage: 100,
            factor: 2.0,
        }];
        assert_eq!(repair_player_items(&mut d, 5, |_| 0), 0);
        assert_eq!(d[0].damage, 90);
    }

    struct Flat;
    impl OrbWorld for Flat {
        fn collides(&self, b: &Aabb) -> bool {
            b.min_y < 0.0
        }
        fn eye_in_water(&self, _: Vec3) -> bool {
            false
        }
        fn lava_at(&self, _: Vec3) -> bool {
            false
        }
        fn move_body(&self, body: &mut Body, vec: Vec3) {
            let ny = body.pos.y + vec.y;
            body.vertical_collision_below = false;
            if ny < 0.0 {
                body.pos = Vec3::new(body.pos.x + vec.x, 0.0, body.pos.z + vec.z);
                body.on_ground = true;
                body.vertical_collision_below = true;
                body.movement = Vec3::new(vec.x, 0.0, vec.z);
            } else {
                body.pos = Vec3::new(body.pos.x + vec.x, ny, body.pos.z + vec.z);
                body.on_ground = false;
            }
        }
        fn friction_below(&self, _: &Body) -> f32 {
            0.6
        }
    }

    fn orb_at(x: f64, y: f64) -> Orb {
        Orb {
            value: 3,
            count: 1,
            age: 0,
            health: 5,
            following: None,
            tick_count: 0,
            body: Body::at(Vec3::new(x, y, 0.5)),
            random: LegacyRandomSource::new(0),
            data_dirty: false,
        }
    }

    #[test]
    fn a_resting_orb_feels_gravity_and_drag_only_with_nobody_near() {
        let mut o = orb_at(0.5, 0.0);
        o.body.on_ground = true;
        let out = tick_one(1, &mut o, &Flat, &[], &[]);
        assert!(!out.expired);
        assert_eq!(o.tick_count, 1);
        assert_eq!(o.age, 1);
        assert_eq!(o.following, None);
    }

    #[test]
    fn an_orb_pulls_towards_a_player_inside_eight_blocks() {
        let mut o = orb_at(4.5, 0.0);
        let p = Seer {
            entity_id: 9,
            pos: (0.5, 0.0, 0.5),
        };
        tick_one(1, &mut o, &Flat, &[p], &[]);
        assert_eq!(o.following, Some(9));
        assert!(o.body.pos.x < 4.5, "moved towards the player");
        let mut far = orb_at(9.5, 0.0);
        tick_one(2, &mut far, &Flat, &[p], &[]);
        assert_eq!(far.following, None, "64 is the squared limit, exclusive");
    }

    /// Vanilla's own numbers for `683`: an orb summoned four blocks east of
    /// a standing player, on grass, sends its first `entity_position` twenty
    /// ticks later at **2.66451143196403** blocks from the player with an
    /// x velocity of **-0.05389733260086671** (the trace's `LpVec3`-decoded
    /// value). Recorded 2026-10-01, `xp-orbs` run `r2`, vanilla 26.2. The
    /// flat fake below is a grass floor at y = 0, as the substrate is.
    #[test]
    fn twenty_ticks_of_creep_land_on_vanillas_recorded_position() {
        let player = Seer {
            entity_id: 1,
            pos: (0.5, 0.0, 0.5),
        };
        let mut o = orb_at(4.5, 0.0);
        for _ in 0..20 {
            tick_one(9, &mut o, &Flat, &[player], &[]);
        }
        let dx = o.body.pos.x - player.pos.0;
        assert!((dx - 2.664_511_431_964_03).abs() < 1e-9, "dx = {dx}");
        // `LpVec3` carries 15 bits of mantissa over a shared scale.
        assert!(
            (o.body.movement.x - -0.053_897_332_600_866_71).abs() < 1e-4,
            "vx = {}",
            o.body.movement.x
        );
        assert!(o.body.on_ground);
    }

    #[test]
    fn the_merge_scan_needs_matching_values_and_ids_forty_apart() {
        let mut o = orb_at(0.5, 0.0);
        let near = Vec3::new(0.6, 0.0, 0.5);
        let out = tick_one(
            1,
            &mut o,
            &Flat,
            &[],
            &[(41, near, 3, 2, 7), (2, near, 3, 1, 0), (81, near, 4, 1, 0)],
        );
        assert_eq!(out.merged_away, vec![41]);
        assert_eq!(o.count, 3);
        assert_eq!(o.age, 1, "min(0, 7), then this tick's increment");
    }

    #[test]
    fn the_furnace_draws_only_for_a_fraction() {
        let mut r = LegacyRandomSource::new(1);
        let mut before = r;
        assert_eq!(sources::furnace(1, 1.0, &mut r), 1);
        assert_eq!(r.next_i64(), before.next_i64(), "no draw for 1.0");
        let v = sources::furnace(1, 0.7, &mut r);
        assert!(v == 0 || v == 1);
    }

    #[test]
    fn ore_ranges_match_the_blocks_table() {
        assert_eq!(sources::block_range("minecraft:diamond_ore"), Some((3, 7)));
        assert_eq!(sources::block_range("minecraft:redstone_ore"), Some((1, 5)));
        assert_eq!(sources::block_range("minecraft:stone"), None);
        let mut r = LegacyRandomSource::new(5);
        for _ in 0..100 {
            let v = sources::block((3, 7), false, &mut r);
            assert!((3..=7).contains(&v));
        }
        assert_eq!(sources::block((3, 7), true, &mut r), 0);
    }
}
