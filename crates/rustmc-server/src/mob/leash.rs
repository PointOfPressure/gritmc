//! Leads: `Leashable` for the mobs of this server, the lead branches of
//! `Entity.interact`, and `Leashable.tickLeash`.
//!
//! Reference (26.2, `javap -p -c -constants` on the pinned jar):
//!
//! ```text
//!   Entity.interact(player, hand, location)                  0-490
//!     10-196  player.isSecondaryUseActive() && canBeLeashed() && alive &&
//!             !isBaby(): every Leashable within 32 (AABB.ofSize centred on
//!             this box) leashed to the player and canHaveALeashAttachedTo(this)
//!             -> setLeashedTo(this, true); any -> LEAD_TIED,
//!             SUCCESS_SERVER.withoutItem()
//!    197-234  SHEARS && shearOffAllLeashConnections        (refused, below)
//!    296-387  alive && leashed to the player: creative -> removeLeash()
//!             else dropLeash(); LEAD_UNTIED; SUCCESS.withoutItem()
//!    388-486  LEAD in hand, holder not a player, canHaveALeashAttachedTo(player):
//!             leashed -> dropLeash(); setLeashedTo(player, true); LEAD_TIED;
//!             stack.shrink(1); SUCCESS_SERVER
//!
//!   Leashable.setLeashedTo(E, holder, broadcast)             0-121
//!             new LeashData(holder) or setLeashHolder; broadcast ->
//!             sendToTrackingPlayers(ClientboundSetEntityLinkPacket(this, holder))
//!   Leashable.dropLeash(E, broadcast, dropItem)              0-108
//!             leashData = null; onLeashRemoved (Mob: clearHome);
//!             dropItem -> spawnAtLocation(LEAD); broadcast -> link(this, null)
//!
//!   Leashable.tickLeash(level, E)                            0-286
//!     38-95   this or the holder cannot interact with the level ->
//!             ENTITY_DROPS ? dropLeash : removeLeash
//!    120-141  d = leashDistanceTo(holder) (box centre to box centre);
//!             whenLeashedTo(holder) (PathfinderMob: setHomeTo(holder pos, 5))
//!    142-191  d > 12  -> LEAD_BREAK at the holder; leashTooFarBehaviour()
//!                        (Mob: dropLeash, disableControlFlag(MOVE))
//!    194-244  d > 6 - holder width - own width && checkElasticInteractions
//!                     -> onElasticLeashPull (checkFallDistanceAccumulation)
//!    247-254  else closeRangeLeashBehaviour(holder) (PathfinderMob: MOVE on,
//!                        navigate toward the holder, stopping 2 short)
//!    257-283  setYRot(yRot - angularMomentum); angularMomentum *= friction
//! ```
//!
//! `Entity.baseTick` calls `tickLeash` last (offsets 265-280), on a server
//! level, for every `Leashable`; a `Mob` is one.
//!
//! # Refused, by name
//!
//! * Shears cutting a lead (`shearOffAllLeashConnections`) — no shears path.
//! * `supportQuadLeash` holders and leashees (happy ghast): neither kind here
//!   is one, so the single-point attachments are the whole of the elastic
//!   model this needs.
//! * A leash knot on a fence (`LeadItem.useOn`): a block interaction.
use super::host::ServerMob;
use rustmc_entity::ai::goal::Flag;
use rustmc_entity::ai::mob_ai::MobAi;
use rustmc_entity::host::MobHost;
use rustmc_entity::phys::Vec3;

/// `Leashable.leashSnapDistance()`: `ldc2_w 12.0`.
pub const LEASH_SNAP_DISTANCE: f64 = 12.0;
/// `Leashable.leashElasticDistance()`: `ldc2_w 6.0`.
pub const LEASH_ELASTIC_DISTANCE: f64 = 6.0;
/// `Leashable.AXIS_SPECIFIC_ELASTICITY`, `<clinit>` 0-16.
const AXIS_SPECIFIC_ELASTICITY: Vec3 = Vec3 {
    x: 0.8,
    y: 0.2,
    z: 0.8,
};
/// `Leashable.ENTITY_ATTACHMENT_POINT`, `<clinit>` 19-36: `(0, 0.5, 0.5)`.
const ENTITY_ATTACHMENT_POINT: Vec3 = Vec3 {
    x: 0.0,
    y: 0.5,
    z: 0.5,
};
/// `Leashable.LEASHER_ATTACHMENT_POINT`, `<clinit>` 39-54: `(0, 0.5, 0)`.
const LEASHER_ATTACHMENT_POINT: Vec3 = Vec3 {
    x: 0.0,
    y: 0.5,
    z: 0.0,
};

/// `SoundEvents.LEAD_TIED` / `LEAD_UNTIED` / `LEAD_BREAK`.
pub const LEAD_TIED: &str = "minecraft:item.lead.tied";
pub const LEAD_UNTIED: &str = "minecraft:item.lead.untied";
pub const LEAD_BREAK: &str = "minecraft:item.lead.break";

/// `Leashable.LeashData`, as far as a mob held by an entity needs it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LeashData {
    /// `leashHolder`, an entity id.
    pub holder: i32,
    /// `angularMomentum`.
    pub angular_momentum: f64,
}

/// A holder as `tickLeash` reads it.
#[derive(Debug, Clone, Copy)]
struct Holder {
    pos: Vec3,
    centre: Vec3,
    width: f32,
    height: f32,
    y_rot: f32,
    block_pos: rustmc_data::BlockPos,
    /// `getHolderMovement(holder)`: a no-AI mob's is zero, else
    /// `getKnownMovement()`.
    movement: Vec3,
}

fn holder(id: i32) -> Option<Holder> {
    let f = super::facts_of_entity(id)?;
    if !f.alive {
        return None;
    }
    let width = f.bb.max_x - f.bb.min_x;
    let height = f.bb.max_y - f.bb.min_y;
    #[expect(clippy::cast_possible_truncation, reason = "a box width is an f32")]
    let (width, height) = (width as f32, height as f32);
    let movement = if f.is_player {
        super::player_known_movement(id)
    } else {
        super::Mobs::shared()
            .with(id, |s| s.ai.mob.delta_movement())
            .unwrap_or(Vec3::ZERO)
    };
    Some(Holder {
        pos: Vec3::new(f.x, f.y, f.z),
        centre: Vec3::new(
            (f.bb.min_x + f.bb.max_x) / 2.0,
            (f.bb.min_y + f.bb.max_y) / 2.0,
            (f.bb.min_z + f.bb.max_z) / 2.0,
        ),
        width,
        height,
        y_rot: f.y_rot,
        block_pos: f.block_position(),
        movement,
    })
}

fn centre(m: &ServerMob) -> Vec3 {
    let bb = m.bounding_box();
    Vec3::new(
        (bb.min_x + bb.max_x) / 2.0,
        (bb.min_y + bb.max_y) / 2.0,
        (bb.min_z + bb.max_z) / 2.0,
    )
}

/// `ClientboundSetEntityLinkPacket.write`: `writeInt(sourceId)`,
/// `writeInt(destId)` — big-endian ints, the holder 0 when null.
#[must_use]
pub fn set_entity_link_body(source: i32, holder: Option<i32>) -> Vec<u8> {
    let mut b = Vec::with_capacity(8);
    b.extend_from_slice(&source.to_be_bytes());
    b.extend_from_slice(&holder.unwrap_or(0).to_be_bytes());
    b
}

fn broadcast_link(m: &ServerMob, holder: Option<i32>) {
    super::breed::send_to_tracking_players(
        m.id,
        rustmc_proto::ids::play::cb::SET_ENTITY_LINK,
        &set_entity_link_body(m.id, holder),
    );
}

/// `Mob.canBeLeashed()`: `!(this instanceof Enemy)` — a pig and a bat both.
#[must_use]
pub fn can_be_leashed(_m: &ServerMob) -> bool {
    true
}

/// `Leashable.leashDistanceTo(holder)`: centre to centre.
fn leash_distance_to(m: &ServerMob, h: &Holder) -> f64 {
    h.centre.distance_to(centre(m))
}

/// `Leashable.canHaveALeashAttachedTo(holder)`, 0-32.
#[must_use]
pub fn can_have_a_leash_attached_to(m: &ServerMob, holder_id: i32) -> bool {
    if holder_id == m.id {
        return false;
    }
    let Some(h) = holder(holder_id) else {
        return false;
    };
    if leash_distance_to(m, &h) > LEASH_SNAP_DISTANCE {
        return false;
    }
    can_be_leashed(m)
}

/// `Leashable.setLeashedTo(holder, broadcast)`, 0-121.
pub fn set_leashed_to(m: &mut ServerMob, holder_id: i32, broadcast: bool) {
    if holder_id == m.id {
        return;
    }
    match &mut m.leash {
        None => {
            m.leash = Some(LeashData {
                holder: holder_id,
                angular_momentum: 0.0,
            });
        }
        // 36-64: `setLeashHolder` on the same data; the old holder's
        // `notifyLeasheeRemoved` is a no-op for every holder here.
        Some(d) => d.holder = holder_id,
    }
    if broadcast {
        broadcast_link(m, Some(holder_id));
    }
    // 110-121: `isPassenger()` — no mob here rides anything.
}

/// `Leashable.dropLeash(E, broadcast, dropItem)`, 0-108.
fn drop_leash_with(m: &mut ServerMob, broadcast: bool, drop_item: bool) {
    if m.leash.is_none() {
        return;
    }
    m.leash = None;
    // `Mob.onLeashRemoved()`: `getLeashData() == null -> clearHome()`.
    m.home_radius = -1;
    if drop_item {
        spawn_lead_at(m);
    }
    if broadcast {
        broadcast_link(m, None);
    }
}

/// `dropLeash()`: broadcast, and the lead falls.
pub fn drop_leash(m: &mut ServerMob) {
    drop_leash_with(m, true, true);
}

/// `removeLeash()`: broadcast, no item.
pub fn remove_leash(m: &mut ServerMob) {
    drop_leash_with(m, true, false);
}

/// `Entity.spawnAtLocation(level, Items.LEAD)` → `new ItemEntity(level,
/// getX(), getY() + 0, getZ(), stack)`: the five-argument constructor's
/// `(nextDouble() * 0.2 - 0.1, 0.2, nextDouble() * 0.2 - 0.1)` after the
/// two-argument one's `bobOffs` and yaw, all on the item's own random.
fn spawn_lead_at(m: &ServerMob) {
    use rustmc_worldgen::rng::RandomSource;
    let entities = crate::entities::Entities::shared();
    let mut rng = crate::dropitem::unique_random();
    let uuid = crate::dropitem::insecure_uuid(&mut rng);
    let _bob_offs = rng.next_f32() * std::f32::consts::PI * 2.0;
    let y_rot = rng.next_f32() * 360.0;
    let dx = rng.next_f64() * 0.2 - 0.1;
    let dz = rng.next_f64() * 0.2 - 0.1;
    let item = crate::entities::Entity {
        id: entities.next_id(),
        uuid,
        kind: crate::entities::ITEM,
        x: m.x(),
        y: m.y(),
        z: m.z(),
        y_rot,
        x_rot: 0.0,
        y_head_rot: 0.0,
        movement: (dx, 0.2, dz),
        on_ground: false,
        data: 0,
        swinging: false,
        swing_time: 0,
        swinging_arm: None,
        synched: crate::synched::Synched::EntityOnly,
    };
    entities.insert(item.clone());
    super::drops::register_item(
        item.id,
        "minecraft:lead",
        super::drops::item_id("minecraft:lead"),
    );
    if let Some(kinds) = crate::entities::shared_kind_ids() {
        crate::tracker::Tracker::shared().add(&item, &super::breed::current_players(), kinds);
    }
}

/// The lead branches of `Entity.interact`, offsets 0-490, for a mob. `None`
/// is every path that falls through to `PASS`.
pub fn entity_interact(
    m: &mut ServerMob,
    who: &super::interact::Interactor<'_>,
    creative: bool,
    used: &mut i32,
) -> Option<super::interact::Outcome> {
    use super::interact::{Outcome, Swing};
    // 10-196: the sneaking transfer.
    if who.secondary && can_be_leashed(m) && m.is_alive() && !m.is_baby() {
        let mut any = false;
        for other in leashable_in_area(m, who.entity_id) {
            if other == m.id {
                continue;
            }
            let attached = super::Mobs::shared()
                .with_mut(other, |s| {
                    if can_have_a_leash_attached_to(&s.ai.mob, m.id) {
                        set_leashed_to(&mut s.ai.mob, m.id, true);
                        true
                    } else {
                        false
                    }
                })
                .unwrap_or(false);
            any |= attached;
        }
        if any {
            play_entity_sound(m, LEAD_TIED);
            return Some(Outcome::Success(Swing::Server));
        }
    }
    // 296-387: the player is already the holder.
    if !m.is_alive() {
        return None;
    }
    if m.leash.map(|d| d.holder) == Some(who.entity_id) {
        if creative {
            remove_leash(m);
        } else {
            drop_leash(m);
        }
        play_entity_sound(m, LEAD_UNTIED);
        return Some(Outcome::Success(Swing::Client));
    }
    // 388-486: a lead in hand, and the current holder is not a player.
    if who.item == Some("minecraft:lead") {
        let held_by_player = m
            .leash
            .and_then(|d| super::facts_of_entity(d.holder))
            .is_some_and(|f| f.is_player);
        if !held_by_player && can_have_a_leash_attached_to(m, who.entity_id) {
            if m.leash.is_some() {
                drop_leash(m);
            }
            set_leashed_to(m, who.entity_id, true);
            play_entity_sound(m, LEAD_TIED);
            // 477-480: `stack.shrink(1)` — creative's count is restored by
            // `Player.interactOn` 64-101.
            *used += 1;
            return Some(Outcome::Success(Swing::Server));
        }
    }
    None
}

/// `Leashable.leashableInArea(this, l -> l.getLeashHolder() == player)`:
/// `AABB.ofSize(getBoundingBox().getCenter(), 32, 32, 32)`, every mob in it
/// leashed to `player`, in the level's entity order.
fn leashable_in_area(m: &ServerMob, player: i32) -> Vec<i32> {
    let c = centre(m);
    let area = rustmc_entity::phys::Aabb::new(
        c.x - 16.0,
        c.y - 16.0,
        c.z - 16.0,
        c.x + 16.0,
        c.y + 16.0,
        c.z + 16.0,
    );
    super::Mobs::shared()
        .ids()
        .into_iter()
        .filter(|&id| {
            super::Mobs::shared()
                .with(id, |s| {
                    s.ai.mob.leash.map(|d| d.holder) == Some(player)
                        && s.ai.mob.bounding_box().intersects(&area)
                })
                .unwrap_or(false)
        })
        .collect()
}

/// `Entity.playSound(sound)`: `playSound(sound, 1F, 1F)` — not silent, at
/// the entity, in its own sound source (`NEUTRAL` for a pig and a bat).
fn play_entity_sound(m: &ServerMob, event: &str) {
    super::living::play_sound(m, event, 1.0, 1.0);
}

/// `Leashable.tickLeash(level, this)`, the tail of `Entity.baseTick`.
pub fn tick_leash(ai: &mut MobAi<ServerMob>) {
    let Some(data) = ai.mob.leash else {
        return;
    };
    // 38-95: `canInteractWithLevel()` on both sides — this mob is ticking, so
    // only the holder can have left (a disconnected player, a dead mob).
    let Some(h) = holder(data.holder) else {
        if crate::gamerules::GameRules::shared().boolean("entity_drops") {
            drop_leash(&mut ai.mob);
        } else {
            remove_leash(&mut ai.mob);
        }
        return;
    };
    // 120-141.
    let d = leash_distance_to(&ai.mob, &h);
    // `PathfinderMob.whenLeashedTo`: `setHomeTo(holder.blockPosition(),
    // (int) leashElasticDistance() - 1)`.
    ai.mob.home_position = h.block_pos;
    #[expect(clippy::cast_possible_truncation, reason = "vanilla's own d2i")]
    let radius = LEASH_ELASTIC_DISTANCE as i32 - 1;
    ai.mob.home_radius = radius;
    let mut momentum = data.angular_momentum;
    if d > LEASH_SNAP_DISTANCE {
        // 157-191.
        super::play_sound_at(
            (h.pos.x, h.pos.y, h.pos.z),
            LEAD_BREAK,
            super::SOURCE_NEUTRAL,
            1.0,
            1.0,
        );
        // `Mob.leashTooFarBehaviour`: the default (`dropLeash`), then MOVE off.
        drop_leash(&mut ai.mob);
        ai.goal_selector.disable_control_flag(Flag::Move);
    } else if d > LEASH_ELASTIC_DISTANCE - f64::from(h.width) - f64::from(ai.mob.bb_width())
        && check_elastic_interactions(&mut ai.mob, &h, &mut momentum)
    {
        // `onElasticLeashPull` → `checkFallDistanceAccumulation`.
        let dy = ai.mob.delta_movement().y;
        if dy > -0.5 && ai.mob.fall_distance > 1.0 {
            ai.mob.fall_distance = 1.0;
        }
    } else {
        close_range_leash_behaviour(ai, &h);
    }
    // 257-283: on the data the tick started with, even if it was dropped.
    #[expect(clippy::cast_possible_truncation, reason = "vanilla's own d2f")]
    let y_rot = (f64::from(ai.mob.y_rot) - momentum) as f32;
    ai.mob.set_y_rot_raw(y_rot);
    momentum *= f64::from(angular_friction(&ai.mob));
    if let Some(live) = &mut ai.mob.leash {
        live.angular_momentum = momentum;
    }
}

/// `Leashable.angularFriction(E)`, 0-43.
fn angular_friction(m: &ServerMob) -> f32 {
    if m.on_ground() {
        return super::living::block_friction_below(m) * 0.91;
    }
    if m.is_in_liquid() {
        return 0.8;
    }
    0.91
}

/// `PathfinderMob.closeRangeLeashBehaviour(holder)`, 0-129.
fn close_range_leash_behaviour(ai: &mut MobAi<ServerMob>, h: &Holder) {
    if !ai.mob.is_panicking_now(&ai.goal_selector) {
        ai.goal_selector.enable_control_flag(Flag::Move);
        // `distanceTo`: `Mth.sqrt((float) distanceToSqr)` on positions.
        let me = ai.mob.position();
        #[expect(clippy::cast_possible_truncation, reason = "vanilla's own d2f")]
        let f = (me.distance_to_sqr(h.pos) as f32).sqrt();
        let dir = Vec3::new(h.pos.x - me.x, h.pos.y - me.y, h.pos.z - me.z)
            .normalize()
            .scale(f64::from((f - 2.0).max(0.0)));
        ai.navigation
            .move_to(&mut ai.mob, me.x + dir.x, me.y + dir.y, me.z + dir.z, 1.0);
    }
}

/// `Leashable.checkElasticInteractions(holder, data)`, 0-155, for a
/// single-point holder and leashee.
fn check_elastic_interactions(m: &mut ServerMob, h: &Holder, momentum: &mut f64) -> bool {
    let wrenches = compute_elastic_interaction(m, h);
    if wrenches.is_empty() {
        return false;
    }
    // `Wrench.accumulate(list).scale(1.0)`.
    let mut force = Vec3::ZERO;
    let mut torque = 0.0;
    for (f, t) in wrenches {
        force = Vec3::new(force.x + f.x, force.y + f.y, force.z + f.z);
        torque += t;
    }
    *momentum += 10.0 * torque;
    // 109-123: `getHolderMovement(holder).subtract(getKnownMovement())`.
    let mine = known_movement(m);
    let rel = h.movement.subtract(mine);
    let push = Vec3::new(
        force.x * AXIS_SPECIFIC_ELASTICITY.x,
        force.y * AXIS_SPECIFIC_ELASTICITY.y,
        force.z * AXIS_SPECIFIC_ELASTICITY.z,
    );
    let add = Vec3::new(
        push.x + rel.x * 0.11,
        push.y + rel.y * 0.11,
        push.z + rel.z * 0.11,
    );
    // `addDeltaMovement`: only a finite vector.
    if add.x.is_finite() && add.y.is_finite() && add.z.is_finite() {
        let v = m.delta_movement();
        m.set_delta_movement(v.x + add.x, v.y + add.y, v.z + add.z);
    }
    true
}

/// `Entity.getKnownMovement()` for a mob nobody rides: `getDeltaMovement()`.
fn known_movement(m: &ServerMob) -> Vec3 {
    m.delta_movement()
}

/// `Vec3.yRot(float)`: `Mth.cos`/`Mth.sin` of the angle as a double.
fn y_rot(v: Vec3, a: f32) -> Vec3 {
    let c = f64::from(rustmc_worldgen::math::cos(f64::from(a)));
    let s = f64::from(rustmc_worldgen::math::sin(f64::from(a)));
    Vec3::new(v.x * c + v.z * s, v.y, v.z * c - v.x * s)
}

/// `Leashable.computeElasticInteraction(E, holder, ENTITY_ATTACHMENT_POINT,
/// LEASHER_ATTACHMENT_POINT)`, 0-214, and
/// `computeDampenedSpringInteraction`, 0-92: `(force, torque)` per pair.
fn compute_elastic_interaction(m: &ServerMob, h: &Holder) -> Vec<(Vec3, f64)> {
    let elastic = LEASH_ELASTIC_DISTANCE;
    // 11-15: `getHolderMovement(this)` — the LEASHEE's own movement, whatever
    // the name says.
    let movement = known_movement(m);
    let my_rot = m.y_rot * 0.017_453_292_f32;
    let my_size = Vec3::new(
        f64::from(m.bb_width()),
        f64::from(m.bb_height()),
        f64::from(m.bb_width()),
    );
    let h_rot = h.y_rot * 0.017_453_292_f32;
    let h_size = Vec3::new(f64::from(h.width), f64::from(h.height), f64::from(h.width));
    let offset = y_rot(
        Vec3::new(
            ENTITY_ATTACHMENT_POINT.x * my_size.x,
            ENTITY_ATTACHMENT_POINT.y * my_size.y,
            ENTITY_ATTACHMENT_POINT.z * my_size.z,
        ),
        -my_rot,
    );
    let me = m.position();
    let mine = Vec3::new(me.x + offset.x, me.y + offset.y, me.z + offset.z);
    let h_off = y_rot(
        Vec3::new(
            LEASHER_ATTACHMENT_POINT.x * h_size.x,
            LEASHER_ATTACHMENT_POINT.y * h_size.y,
            LEASHER_ATTACHMENT_POINT.z * h_size.z,
        ),
        -h_rot,
    );
    let theirs = Vec3::new(h.pos.x + h_off.x, h.pos.y + h_off.y, h.pos.z + h_off.z);
    // computeDampenedSpringInteraction(theirs, mine, elastic, movement, offset)
    let dist = mine.distance_to(theirs);
    if dist < elastic {
        return Vec::new();
    }
    let mut force = theirs.subtract(mine).normalize().scale(dist - elastic);
    // `Wrench.torqueFromForce(offset, force)`: `a.z * b.x - a.x * b.z`.
    let torque = offset.z * force.x - offset.x * force.z;
    if movement.dot(force) >= 0.0 {
        force = force.scale(0.300_000_011_920_928_96);
    }
    vec![(force, torque)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_link_body_is_two_big_endian_ints_and_zero_for_none() {
        assert_eq!(
            set_entity_link_body(5, Some(1)),
            vec![0, 0, 0, 5, 0, 0, 0, 1]
        );
        assert_eq!(set_entity_link_body(5, None), vec![0, 0, 0, 5, 0, 0, 0, 0]);
    }

    #[test]
    fn y_rot_is_vanillas_rotation_about_y() {
        let v = y_rot(Vec3::new(0.0, 0.5, 0.5), 0.0);
        assert_eq!((v.x, v.y, v.z), (0.0, 0.5, 0.5));
        let q = y_rot(Vec3::new(1.0, 0.0, 0.0), std::f32::consts::FRAC_PI_2);
        assert!(q.x.abs() < 1e-6 && (q.z + 1.0).abs() < 1e-6, "{q:?}");
    }
}
