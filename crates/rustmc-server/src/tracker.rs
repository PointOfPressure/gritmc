//! Who is told about which entity, and when — `ChunkMap.TrackedEntity` and
//! `ServerEntity` from the pinned 26.2 jar.
//!
//! # The two halves, and why they are not the same question
//!
//! `TrackedEntity` answers *who*: a per-entity set of connections that have
//! been sent an `add_entity` and not yet a `remove_entities`. `ServerEntity`
//! answers *what*: the last position, rotation and velocity actually put on
//! the wire, and the tests that decide whether this tick produces a packet at
//! all. They are separate objects in vanilla and separate here because they
//! change on different schedules — membership only when an entity crosses a
//! section boundary, content every tick.
//!
//! # The cadence, from `ChunkMap.tick()`
//!
//! ```java
//! for (TrackedEntity t : entityMap.values()) {
//!     SectionPos now = SectionPos.of(t.entity);
//!     boolean movedSection = !Objects.equals(t.lastSectionPos, now);
//!     if (movedSection) { t.updatePlayers(level.players()); ...; t.lastSectionPos = now; }
//!     if (movedSection || entity.needsSync || inEntityTickingRange(now.chunk()))
//!         t.serverEntity.sendChanges();
//! }
//! ```
//!
//! So `updatePlayers` — the expensive all-players range scan — runs **only on
//! a section change**, and `sendChanges` runs every tick for anything in
//! entity-ticking range. An implementation that re-scans every player every
//! tick would produce the same packets and the wrong performance; one that
//! only scanned on a *chunk* change would miss the vertical boundary, because
//! the key is a `SectionPos` and not a `ChunkPos`.

use crate::entities::{self, Entities, Entity, KindIds};
use crate::openhash::IntOpenHashMap;
use crate::players::Connection;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, OnceLock};

/// What the tracker needs to know about one connected player.
///
/// Assembled per tick by the caller rather than held, because two of the four
/// fields live on the connection thread and one lives in the entity list; a
/// cached copy is a chance for them to disagree.
#[derive(Clone)]
pub struct TrackedPlayer {
    pub conn: Arc<Connection>,
    /// The entity id of the player itself. `updatePlayer` returns
    /// immediately when the entity being considered *is* this player, which
    /// is what stops a client being sent an `add_entity` for itself.
    pub entity_id: i32,
    pub pos: (f64, f64, f64),
    /// The player's chunk, which is the centre of its tracking view.
    pub chunk: (i32, i32),
    /// `ChunkMap.getPlayerViewDistance`:
    /// `Mth.clamp(player.requestedViewDistance(), 2, serverViewDistance)`.
    pub view_distance: i32,
}

/// `ServerEntity` — everything that has actually been sent about one entity,
/// and the tests that decide what to send next.
///
/// # What is modelled and what is not, stated rather than discovered later
///
/// Modelled: the update-interval gate, the position/rotation tests, the four
/// packet shapes `sendChanges` can choose between, the forced teleport, the
/// velocity test, the head rotation, and the tick counter that drives the
/// 60-tick heartbeat.
///
/// **Not modelled, because the concepts do not exist in this server yet:**
/// passengers and vehicles (`isPassenger`, `lastPassengers`, `wasRiding` is
/// present but never set), minecart behaviour, item frames and their map
/// updates, `syncPosition`,
/// `hurtMarked`, and
/// `getRequiresPrecisePosition`. (Dirty data and `needsSync` are modelled
/// through [`ServerEntity::send_changes_with`]'s `Dirty`, supplied by the
/// owner of the entity's data: `crate::mob`, `crate::xporb`, and
/// `crate::experience` for a player's score.) Each of those is a limb of a condition that
/// is currently constant, and every one of them is a way for this to send a
/// packet vanilla does not or omit one it does. They are named here so the
/// first check that trips over one finds the list rather than a surprise.
#[derive(Debug)]
pub struct ServerEntity {
    update_interval: i32,
    track_delta: bool,
    /// `VecDeltaCodec.base` — the position every delta is measured from, and
    /// the position `add_entity` announces.
    base: (f64, f64, f64),
    last_sent_y_rot: i8,
    last_sent_x_rot: i8,
    last_sent_y_head_rot: i8,
    last_sent_movement: (f64, f64, f64),
    tick_count: i32,
    teleport_delay: i32,
    was_on_ground: bool,
    /// Never set true today — there are no vehicles. Kept because it is one
    /// of the five conditions that force a teleport, and a reader who finds
    /// it missing would have to re-derive why.
    was_riding: bool,
    /// The synched values as last packed — what `packDirty` compares
    /// against. Seeded from the entity when tracking starts, which is when
    /// the pairing bundle sends its non-default values.
    last_sent_synched: crate::synched::Synched,
}

/// `ServerEntity.FORCED_POS_UPDATE_PERIOD` — 60 ticks, three seconds.
///
/// The heartbeat: an entity that failed the movement test is broadcast anyway
/// on every 60th tick of *its own* counter, which starts when the entity
/// begins being tracked and not at any world time. This is the whole content
/// of `520-a-standing-player-is-rebroadcast-on-a-timer`.
pub const FORCED_POS_UPDATE_PERIOD: i32 = 60;

/// `ServerEntity.FORCED_TELEPORT_PERIOD` — 400.
const FORCED_TELEPORT_PERIOD: i32 = 400;

/// `ServerEntity.TOLERANCE_LEVEL_POSITION` — `7.62939453125E-6`, which is
/// exactly 2⁻¹⁷. The test is `>=`, read from `dcmpl; iflt`.
const TOLERANCE_LEVEL_POSITION: f64 = 7.629_394_531_25E-6;

/// One packet to put on a tracking player's connection.
type Packet = (i32, Vec<u8>);

impl ServerEntity {
    /// The constructor, which seeds every "last sent" field from the entity's
    /// current state — so the first `sendChanges` compares against where the
    /// entity was when tracking began, not against zero.
    #[must_use]
    pub fn new(e: &Entity) -> Self {
        Self {
            update_interval: e.kind.update_interval,
            track_delta: e.kind.track_deltas,
            base: (e.x, e.y, e.z),
            last_sent_y_rot: entities::pack_degrees(e.y_rot),
            last_sent_x_rot: entities::pack_degrees(e.x_rot),
            last_sent_y_head_rot: entities::pack_degrees(e.y_head_rot),
            last_sent_movement: e.movement,
            last_sent_synched: e.synched.clone(),
            tick_count: 0,
            teleport_delay: 0,
            was_on_ground: e.on_ground,
            was_riding: false,
        }
    }

    /// `getPositionBase()` — what `add_entity` announces as the position.
    #[must_use]
    pub const fn position_base(&self) -> (f64, f64, f64) {
        self.base
    }

    #[must_use]
    pub const fn last_sent_movement(&self) -> (f64, f64, f64) {
        self.last_sent_movement
    }

    #[must_use]
    pub const fn last_sent_rotations(&self) -> (i8, i8, i8) {
        (
            self.last_sent_x_rot,
            self.last_sent_y_rot,
            self.last_sent_y_head_rot,
        )
    }

    /// `ServerEntity.sendChanges` — the packets this tick produces, in order.
    ///
    /// # The gate
    ///
    /// The body runs when `tickCount % updateInterval == 0`. Vanilla also
    /// runs it on `needsSync` or a dirty data watcher; neither exists here,
    /// and the omission can only ever make this send *less* than vanilla,
    /// never more.
    ///
    /// # The movement test, and why the heartbeat is not a special case
    ///
    /// ```java
    /// moved   = positionCodec.delta(trackingPosition()).lengthSqr() >= 7.62939453125E-6
    /// sendPos = moved || tickCount % 60 == 0
    /// ```
    ///
    /// `sendPos` is one boolean with two ways to become true, so a standing
    /// entity's three-second re-broadcast takes exactly the same code path,
    /// builds the same `MoveEntity$Pos`, and carries deltas that happen to be
    /// zero. Implementing the heartbeat as a separate "if nothing happened,
    /// send a keepalive" branch would produce a packet of the right shape at
    /// the right time and get the *phase* wrong, because vanilla's counter
    /// also advances on ticks where the body did not run.
    ///
    /// # The four shapes, in the order the bytecode tests them
    ///
    /// 1. forced teleport — `entity_position_sync` — when the entity demands
    ///    precise position, or any delta leaves `i16`, or 400 ticks have
    ///    passed since the last one, or it stopped riding, or its ground flag
    ///    changed;
    /// 2. `move_entity_pos_rot` when both moved and turned (or it is an
    ///    arrow, which always uses this shape);
    /// 3. `move_entity_pos` when only moved;
    /// 4. `move_entity_rot` when only turned.
    ///
    /// A head rotation is independent of all four and follows any of them.
    pub fn send_changes(&mut self, e: &Entity) -> Vec<Packet> {
        self.send_changes_with(e, None)
    }

    /// `sendChanges` with the entity's own dirty state supplied: the gate's
    /// `needsSync || entityData.isDirty()` limbs (offsets 261-281), the
    /// velocity limb's `needsSync` (874), `sendDirtyEntityData` at the end of
    /// the body (1093), and `hurtMarked` after it (1202). `None` is an entity
    /// with none of them, which is every player and item here.
    ///
    /// # `sendDirtyEntityData` is INSIDE the gate, and LAST — corrected 2026-09-30
    ///
    /// This method used to send the dirty data and attributes first, on
    /// every tick, before and regardless of the gate, citing "offset 204".
    /// `javap -p -c -constants` on the pinned jar says otherwise: the call at
    /// **205** is inside `if (entity instanceof ItemFrame && tickCount % 10 ==
    /// 0)` (offsets 61-84), the map-update arm; the two calls every other
    /// entity reaches are at **411** (the riding branch) and **1093** (the
    /// normal one), both inside the `tickCount % updateInterval == 0 ||
    /// needsSync || isDirty()` body and both **after** the position packet
    /// and the velocity packet are sent. Vanilla's own recording of `777`
    /// shows exactly that order on the stand's first gated tick:
    /// `set_entity_motion`, `entity_position`, `update_attributes`. An
    /// attribute that becomes dirty on an ungated tick therefore waits for
    /// the next gated one — `isDirty()` is the synched data's, not the
    /// attribute map's.
    pub fn send_changes_with(
        &mut self,
        e: &Entity,
        dirty: Option<&dyn DirtySource>,
    ) -> Vec<Packet> {
        use rustmc_proto::ids::play::cb;
        let mut out = Vec::new();
        let (data_dirty, mob_needs_sync) = dirty.map_or((false, false), DirtySource::flags);
        // `needsSync` has two writers here: the mob path and a console
        // `/damage` knockback (`damagecmd`). Either one sets it.
        let needs_sync = mob_needs_sync || crate::damagecmd::needs_sync(e.id);
        // `SynchedEntityData.isDirty()` (offsets 272..281) for the values
        // `crate::synched` models (a player's); a mob's goes through
        // `DirtySource` above.
        let dirty_synched = e
            .synched
            .clone()
            .dirty_values(self.last_sent_synched.clone());
        if self.tick_count % self.update_interval == 0
            || needs_sync
            || data_dirty
            || dirty_synched.is_some()
        {
            let y_rot = entities::pack_degrees(e.y_rot);
            let x_rot = entities::pack_degrees(e.x_rot);
            // `Math.abs(a - b) >= 1` on the *unpacked* bytes, so the
            // subtraction is in `i32` and cannot wrap: two rotations 180
            // degrees apart are -128 and 0, whose difference as bytes would
            // overflow and as ints is 128.
            let rot_changed = (i32::from(y_rot) - i32::from(self.last_sent_y_rot)).abs() >= 1
                || (i32::from(x_rot) - i32::from(self.last_sent_x_rot)).abs() >= 1;

            self.teleport_delay += 1;
            let pos = (e.x, e.y, e.z);
            let d = (
                pos.0 - self.base.0,
                pos.1 - self.base.1,
                pos.2 - self.base.2,
            );
            // `Vec3.lengthSqr()` is `x * x + y * y + z * z` with three
            // separate IEEE roundings. **Not `mul_add`**, which fuses the
            // multiply and the add into one rounding and can therefore land
            // the other side of a threshold this exact comparison is testing.
            // The workspace already forbids reassociation for worldgen; the
            // same rule applies to a predicate on the wire.
            let moved = (d.0 * d.0 + d.1 * d.1 + d.2 * d.2) >= TOLERANCE_LEVEL_POSITION;
            let send_pos = moved || self.tick_count % FORCED_POS_UPDATE_PERIOD == 0;

            let dx = entities::encode_pos(pos.0) - entities::encode_pos(self.base.0);
            let dy = entities::encode_pos(pos.1) - entities::encode_pos(self.base.1);
            let dz = entities::encode_pos(pos.2) - entities::encode_pos(self.base.2);
            let too_far = [dx, dy, dz]
                .iter()
                .any(|v| *v < i64::from(i16::MIN) || *v > i64::from(i16::MAX));

            let mut sent_pos = false;
            let mut sent_rot = false;
            let mut packet: Option<Packet> = None;

            if too_far
                || self.teleport_delay > FORCED_TELEPORT_PERIOD
                || self.was_riding
                || self.was_on_ground != e.on_ground
            {
                self.was_on_ground = e.on_ground;
                self.teleport_delay = 0;
                if let Ok(body) = entity_position_sync_body(e) {
                    packet = Some((cb::ENTITY_POSITION_SYNC, body));
                }
                sent_pos = true;
                sent_rot = true;
            } else {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "vanilla's own l2i then i2s, guarded by `too_far`"
                )]
                let short = (dx as i16, dy as i16, dz as i16);
                if send_pos && rot_changed {
                    if let Ok(body) =
                        entities::move_entity_pos_rot_body(e.id, short, y_rot, x_rot, e.on_ground)
                    {
                        packet = Some((cb::MOVE_ENTITY_POS_ROT, body));
                    }
                    sent_pos = true;
                    sent_rot = true;
                } else if send_pos {
                    if let Ok(body) = entities::move_entity_pos_body(e.id, short, e.on_ground) {
                        packet = Some((cb::MOVE_ENTITY_POS, body));
                    }
                    sent_pos = true;
                } else if rot_changed {
                    if let Ok(body) =
                        entities::move_entity_rot_body(e.id, y_rot, x_rot, e.on_ground)
                    {
                        packet = Some((cb::MOVE_ENTITY_ROT, body));
                    }
                    sent_rot = true;
                }
            }

            // The velocity limb. **The sign of this test was inverted until
            // 2026-08-05 and the comment explaining it was inverted too.**
            //
            // `ServerEntity.sendChanges`, `javap -p -c -constants`, offsets
            // 874-919 of the pinned 26.2 jar:
            //
            // ```text
            //   874  entity.needsSync            ifne -> 919   enter
            //   884  this.trackDelta             ifne -> 919   enter
            //   898  entity instanceof LivingEntity  ifeq -> 1076  skip
            //   913  ((LivingEntity) e).isFallFlying ifeq -> 1076  skip
            //   919  the velocity limb
            // ```
            //
            // so it is `needsSync || trackDelta || (LivingEntity &&
            // isFallFlying)` — **`trackDelta`, not `!trackDelta`.** The old
            // comment reasoned that "`trackDeltas` being true means the client
            // extrapolates for itself and the server stays quiet", which is
            // backwards: a client that extrapolates needs a velocity to
            // extrapolate *from*, so `trackDeltas` true is exactly when the
            // server must send one. Vanilla sends velocity for an item and
            // not for a standing player; we did the opposite.
            //
            // It was not latent. `dropitem.rs` gives a thrown item a real
            // `throw_velocity`, and a player's movement is always zero here
            // because there is no server-side physics — so the inverted test
            // took the dead branch in both directions and this server emitted
            // **no `set_entity_motion` at all**, for anything, ever. The
            // corpus measured it as
            // `dropping-an-item-throws-it-on-the-floor`'s
            // `missing_kinds: ["set_entity_motion"]`, and the bytecode above
            // was read afterwards to find out why.
            //
            // `needsSync` and fall-flying stay unmodelled. Both are disjuncts,
            // so omitting them can only make this send less than vanilla,
            // never more — for a player that is now the whole of the limb,
            // and `docs/SERVER.md` carries it.
            // `needsSync || trackDelta || isFallFlying` (offsets 877..916).
            if needs_sync || self.track_delta {
                let m = e.movement;
                let l = self.last_sent_movement;
                let (ax, ay, az) = (m.0 - l.0, m.1 - l.1, m.2 - l.2);
                // Separate roundings, as above: `Vec3.distanceToSqr` and
                // `Vec3.lengthSqr` are plain sums of products in Java.
                let dist = ax * ax + ay * ay + az * az;
                let len = m.0 * m.0 + m.1 * m.1 + m.2 * m.2;
                if dist > 1.0E-7 || (dist > 0.0 && len == 0.0) {
                    self.last_sent_movement = m;
                    if let Ok(body) = set_entity_motion_body(e.id, m) {
                        out.push((cb::SET_ENTITY_MOTION, body));
                    }
                }
            }

            if let Some(p) = packet {
                out.push(p);
            }
            // 1093: `sendDirtyEntityData()` — `set_entity_data` from
            // `packDirty`, then `update_attributes` from
            // `getAttributesToSync`.
            if let Some(d) = dirty {
                let (data, attributes) = d.drain();
                if let Some(body) = data {
                    out.push((cb::SET_ENTITY_DATA, body));
                }
                if let Some(body) = attributes {
                    out.push((cb::UPDATE_ATTRIBUTES, body));
                }
            }
            // The same call for the values `crate::synched` models, to
            // tracking players AND self (`broadcastAndSend`). The self half
            // for a player is its own loop's — `world_send::send_changes_to_self`
            // — so that it lands ahead of that tick's `set_health` as it does
            // in vanilla, where `ChunkMap.tick` runs before the connections.
            if let Some(values) = dirty_synched {
                self.last_sent_synched = e.synched.clone();
                if let Ok(body) = crate::synched::set_entity_data_body(e.id, &values) {
                    out.push((cb::SET_ENTITY_DATA, body));
                }
            }
            if sent_pos {
                self.base = pos;
            }
            if sent_rot {
                self.last_sent_y_rot = y_rot;
                self.last_sent_x_rot = x_rot;
            }
            self.was_riding = false;

            let head = entities::pack_degrees(e.y_head_rot);
            if (i32::from(head) - i32::from(self.last_sent_y_head_rot)).abs() >= 1 {
                if let Ok(body) = entities::rotate_head_body(e.id, head) {
                    out.push((cb::ROTATE_HEAD, body));
                }
                self.last_sent_y_head_rot = head;
            }
            // 1189: `entity.needsSync = false`.
            if needs_sync {
                crate::mob::clear_needs_sync(e.id);
                crate::damagecmd::clear_needs_sync(e.id);
                crate::itementity::clear_needs_sync(e.id);
            }
        }
        // **Outside the gate.** The counter advances on every tick, not only
        // on the ticks the body ran, which is what keeps the heartbeat on a
        // 60-tick period rather than a 60-body-run one. For a player at
        // `updateInterval` 2 those differ by a factor of two, so getting this
        // line inside the `if` would halve the heartbeat rate and still look
        // entirely reasonable.
        self.tick_count += 1;
        // 1202..1240 — `hurtMarked`, and this is the one limb that is not
        // under the gate: `sendToTrackingPlayersAndSelf`, which for a mob is
        // the same set as this method's other packets. Read and cleared here,
        // after `tickCount++`, at 1216's position.
        let hurt_marked = dirty.is_some_and(DirtySource::take_hurt_marked);
        //
        // `last_sent_movement` is deliberately NOT updated here: vanilla's
        // velocity limb at 919 owns that field, and this arm builds its
        // packet from the entity directly without consulting or writing it.
        // Updating it would suppress the next legitimate velocity packet.
        if hurt_marked {
            if let Ok(body) = set_entity_motion_body(e.id, e.movement) {
                out.push((cb::SET_ENTITY_MOTION, body));
            }
        }
        out
    }
}

/// A mob's [`DirtySource`] with a second `hurtMarked` writer OR-ed in:
/// `/damage`'s `markHurt` (`crate::damagecmd`) sets the same vanilla field the
/// mob path does, so the two must produce one read, not two packets.
struct HurtMarkOr<'a> {
    inner: &'a dyn DirtySource,
    extra: bool,
}

impl DirtySource for HurtMarkOr<'_> {
    fn flags(&self) -> (bool, bool) {
        self.inner.flags()
    }
    fn drain(&self) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
        self.inner.drain()
    }
    fn take_hurt_marked(&self) -> bool {
        // Not short-circuited: the inner read clears the mob's own flag.
        self.inner.take_hurt_marked() | self.extra
    }
}

/// A dirty state already drained out of its store — the item entities', the
/// orbs' and a player's score — as a [`DirtySource`]: dirty while it holds a
/// packet, `needsSync` as the store said, never hurt.
pub struct Drained(std::cell::RefCell<crate::mob::Dirty>);

impl Drained {
    #[must_use]
    pub fn new(d: crate::mob::Dirty) -> Self {
        Self(std::cell::RefCell::new(d))
    }
}

impl DirtySource for Drained {
    fn flags(&self) -> (bool, bool) {
        let d = self.0.borrow();
        (d.data.is_some() || d.attributes.is_some(), d.needs_sync)
    }
    fn drain(&self) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
        let mut d = self.0.borrow_mut();
        (d.data.take(), d.attributes.take())
    }
    fn take_hurt_marked(&self) -> bool {
        false
    }
}

/// An entity's own dirty state, as `ServerEntity.sendChanges` reads it.
pub trait DirtySource {
    /// `(entityData.isDirty(), entity.needsSync)` — the two gate limbs.
    fn flags(&self) -> (bool, bool);
    /// `sendDirtyEntityData`'s two packets: `packDirty` and
    /// `getAttributesToSync`, both cleared by the read.
    fn drain(&self) -> (Option<Vec<u8>>, Option<Vec<u8>>);
    /// `hurtMarked`, cleared by the read.
    fn take_hurt_marked(&self) -> bool;
}

/// `ClientboundEntityPositionSyncPacket`'s body:
///
/// ```text
///   varint  entity id
///   double  x, y, z            (PositionMoveRotation.position)
///   double  dx, dy, dz         (PositionMoveRotation.deltaMovement)
///   float   yRot
///   float   xRot
///   bool    onGround
/// ```
///
/// # The velocity here is THREE RAW DOUBLES, not an `LpVec3`
///
/// Written with `LpVec3` first, by analogy with `add_entity` and
/// `set_entity_motion`, which really do use it. This packet does not.
/// `ClientboundEntityPositionSyncPacket` holds a `PositionMoveRotation`, whose
/// codec is
///
/// ```java
/// StreamCodec.composite(Vec3.STREAM_CODEC,  PositionMoveRotation::position,
///                       Vec3.STREAM_CODEC,  PositionMoveRotation::deltaMovement,
///                       ByteBufCodecs.FLOAT, PositionMoveRotation::yRot,
///                       ByteBufCodecs.FLOAT, PositionMoveRotation::xRot,
///                       PositionMoveRotation::new)
/// ```
///
/// and `Vec3.STREAM_CODEC` — `Vec3$1.encode` — is three `writeDouble`s.
/// `Vec3.LP_STREAM_CODEC` is the other field and the two are one identifier
/// apart. The wrong one produces a body six bytes short of what the client
/// reads, which desynchronises the connection several packets later and
/// blames whatever was in flight.
///
/// This is the packet the forced teleport sends, and the teleport fires
/// whenever the ground flag changes — so every jump. It would not have
/// stayed hidden, but it would have been diagnosed a long way from here.
///
/// The rotations are **full floats, not packed bytes**: this is the packet
/// that exists precisely to be exact.
///
/// # Errors
/// Only if the buffer write fails, which for a `Vec` it cannot.
pub fn entity_position_sync_body(e: &Entity) -> std::io::Result<Vec<u8>> {
    use rustmc_proto::{varint::write_varint, wire};
    let mut b = Vec::with_capacity(64);
    write_varint(&mut b, e.id)?;
    wire::write_f64(&mut b, e.x)?;
    wire::write_f64(&mut b, e.y)?;
    wire::write_f64(&mut b, e.z)?;
    wire::write_f64(&mut b, e.movement.0)?;
    wire::write_f64(&mut b, e.movement.1)?;
    wire::write_f64(&mut b, e.movement.2)?;
    wire::write_f32(&mut b, e.y_rot)?;
    wire::write_f32(&mut b, e.x_rot)?;
    wire::write_bool(&mut b, e.on_ground)?;
    Ok(b)
}

/// `ClientboundSetEntityMotionPacket`: `(varint id, LpVec3 velocity)`.
///
/// In 26.2 this is an `LpVec3` and not three shorts; the older three-short
/// form is what a protocol note from a previous version would give.
///
/// # Errors
/// Only if the buffer write fails, which for a `Vec` it cannot.
pub fn set_entity_motion_body(id: i32, v: (f64, f64, f64)) -> std::io::Result<Vec<u8>> {
    use rustmc_proto::varint::write_varint;
    let mut b = Vec::with_capacity(12);
    write_varint(&mut b, id)?;
    entities::write_lp_vec3(&mut b, v)?;
    Ok(b)
}

/// `ClientboundAnimatePacket.write`, offsets 0-17.
///
/// ```text
///   varint  the entity id
///   byte    the action
/// ```
///
/// **A byte, not a varint**, for the action. The five actions are 0, 2, 3, 4
/// and 5, so every one of them encodes identically either way and a wrong
/// choice here would never be caught by a value — only by a packet one byte
/// short of what the next decoder expects, at some later point in the stream.
///
/// # Errors
/// Only if the buffer write fails, which for a `Vec` it cannot.
pub fn animate_body(id: i32, action: u8) -> std::io::Result<Vec<u8>> {
    use rustmc_proto::varint::write_varint;
    let mut b = Vec::with_capacity(6);
    write_varint(&mut b, id)?;
    b.push(action);
    Ok(b)
}

/// `MinecraftServer.getScaledTrackingDistance(int)`, as a **dedicated**
/// server implements it.
///
/// ```java
/// // MinecraftServer, offsets 0-1
/// public int getScaledTrackingDistance(int d) { return d; }
/// // DedicatedServer, offsets 0-9
/// public int getScaledTrackingDistance(int d) {
///     return entityBroadcastRangePercentage() * d / 100;
/// }
/// ```
///
/// **The base class is the identity and the dedicated one is not**, and this
/// server is a dedicated one. At the default 100 the two agree exactly, which
/// is why omitting it cost nothing measurable and why it would have stayed
/// omitted: `entity-broadcast-range-percentage` is the only input that can
/// separate them, and no gate here has ever set it.
///
/// The multiply comes first and the division is Java's, truncating toward
/// zero. Both operands are non-negative here — a range is
/// `clientTrackingRange() * 16` and the percentage is clamped to `10..=1000`
/// by `DedicatedServerProperties` — so it is a floor, but it is written as a
/// truncation because that is what `idiv` is.
#[must_use]
pub const fn scaled_range(range: i32, percentage: i32) -> i32 {
    percentage.wrapping_mul(range) / 100
}

/// `ChunkMap.TrackedEntity` — one entity and the connections that can see it.
#[derive(Debug)]
pub struct TrackedEntity {
    /// `range` — `clientTrackingRange() * 16`, in blocks, **unscaled**.
    ///
    /// `getEffectiveRange` applies `scaledRange` to it on every
    /// `updatePlayer` rather than at construction, so an operator changing
    /// `entity-broadcast-range-percentage` moves every existing tracker.
    /// Held raw here for that reason.
    range: i32,
    /// `lastSectionPos`. `None` until the first tick, which forces the
    /// section comparison to fire once and re-run `updatePlayers`.
    last_section: Option<(i32, i32, i32)>,
    /// `seenBy`, by connection id.
    seen_by: BTreeSet<u64>,
    server: ServerEntity,
}

impl TrackedEntity {
    #[must_use]
    pub fn new(e: &Entity) -> Self {
        Self {
            range: e.kind.tracking_range_chunks * 16,
            last_section: None,
            seen_by: BTreeSet::new(),
            server: ServerEntity::new(e),
        }
    }

    /// `TrackedEntity.updatePlayer` — decide whether this one player can see
    /// this entity, and send the pairing or the removal that follows.
    ///
    /// ```java
    /// if (player == entity) return;
    /// Vec3 d = player.position().subtract(entity.position());
    /// double limit = Math.min(getEffectiveRange(), getPlayerViewDistance(player) * 16);
    /// boolean visible = d.x*d.x + d.z*d.z <= limit*limit
    ///                   && entity.broadcastToPlayer(player)
    ///                   && isChunkTracked(player, entity.chunkPosition());
    /// ```
    ///
    /// # Three things a plausible implementation gets wrong
    ///
    /// * **The distance is horizontal.** `d.y` never enters it. An entity
    ///   directly above a player at any height is in range, and a 3D distance
    ///   test would drop entities vanilla keeps.
    /// * **The comparison is `<=`,** where the chunk-tracking test next to it
    ///   is `<`.
    /// * **The range is the smaller of the type's and the player's view.** A
    ///   player at view distance 4 sees another player at 64 blocks, not at
    ///   the type's 512 — so the tracking range that matters in a test rig is
    ///   almost always the client's, not the entity's.
    ///
    /// `broadcastToPlayer` is `true` for everything except a spectator-only
    /// case that does not arise here.
    fn update_player(
        &mut self,
        e: &Entity,
        p: &TrackedPlayer,
        kinds: &KindIds,
        percentage: i32,
    ) -> Option<Vec<Packet>> {
        if p.entity_id == e.id {
            return None;
        }
        // `getEffectiveRange()`, offsets 0-66: the largest
        // `clientTrackingRange() * 16` over this entity and its indirect
        // passengers, then `scaledRange`. Nothing here rides anything, so the
        // maximum is over one element and the loop is absent rather than
        // wrong — but the **scale is not**, and it was missing until
        // 2026-09-05. See [`scaled_range`].
        let limit = f64::from(scaled_range(self.range, percentage).min(p.view_distance * 16));
        let dx = p.pos.0 - e.x;
        let dz = p.pos.2 - e.z;
        // `d.x * d.x + d.z * d.z`, two roundings, not a fused one.
        //
        // The third limb is `ChunkMap.isChunkTracked(player, x, z)` and it is
        // **both** of that method's halves. This used to call
        // `broadcast::chunk_is_tracked` — the geometry alone — because the
        // `isPending` half lived on the session thread's stack and nothing
        // here could reach it. So an entity in a chunk the client had been
        // promised and not yet sent was paired anyway, where vanilla waits
        // for the chunk. `Connection` carries the view now.
        let visible = (dx * dx + dz * dz) <= limit * limit && p.conn.is_chunk_tracked(e.chunk());

        if visible {
            if self.seen_by.insert(p.conn.id()) {
                return Some(pairing_bundle(e, &self.server, kinds));
            }
            None
        } else if self.seen_by.remove(&p.conn.id()) {
            entities::remove_entities_body(&[e.id])
                .ok()
                .map(|b| vec![(rustmc_proto::ids::play::cb::REMOVE_ENTITIES, b)])
        } else {
            None
        }
    }
}

/// `ServerEntity.addPairing` — the bundle a client gets when an entity comes
/// into view.
///
/// **It is a bundle, not a bare packet.** `addPairing` builds a list through
/// `sendPairingData` and sends one `ClientboundBundlePacket` wrapping it,
/// which on the wire is a `bundle_delimiter` (id 0), the contents, and
/// another `bundle_delimiter`. A client applies the whole group in one frame;
/// sending the same packets unbundled makes an entity appear and then be
/// decorated, which is visible as a flicker and is a different byte sequence.
///
/// `sendPairingData` emits, in this order and with these guards, read from
/// the pinned 26.2 jar:
///
/// ```text
///    32-43   getAddEntityPacket(this)                    always
///    48-52   ifnull 79
///    55-74   ClientboundSetEntityDataPacket(id, trackedDataValues)
///                                                        when non-null
///    85-117  instanceof LivingEntity, getSyncableAttributes()
///   120-137  ClientboundUpdateAttributesPacket(id, syncable)
///                                                        when non-empty
///   163-245  over EquipmentSlot.VALUES, skipping empty stacks (209-214)
///   248-265  ClientboundSetEquipmentPacket(id, pairs)     when non-empty
///   285-327  ClientboundSetPassengersPacket, both directions
///   363-382  ClientboundSetEntityLinkPacket               when leashed
/// ```
///
/// **Two of the six are produced here.** `add_entity` and, since the
/// `set_entity_data` derivation in `crate::synched`, the tracked data values
/// — for a player that is its health and its skin-parts byte.
///
/// `update_attributes`, `set_equipment`, `set_passengers` and the leash
/// packet are not, and the reasons are different in kind and worth keeping
/// apart:
///
/// * **`set_equipment` is a gap that usually costs nothing.** Vanilla
///   collects `EquipmentSlot.VALUES`, skips every empty stack, and sends
///   nothing when the list is empty. This server does not carry an entity's
///   equipment at all, so it sends nothing always — which coincides with
///   vanilla for an empty-handed entity and diverges for any other. In
///   `530`'s pairing on the actor's stream that is **usually** nothing:
///   across four full-corpus vanilla recordings on 2026-08-08 the standing
///   observer was paired empty-handed three times and holding something once.
///   Which it is depends on what earlier scenarios left in its inventory, so
///   it is not a property this server can aim at either way.
/// * **`update_attributes` is a real gap** — see `docs/SERVER.md`.
/// * `set_passengers` and the leash packet are unreachable: nothing here
///   rides or is ridden, and nothing is leashable.
fn pairing_bundle(e: &Entity, s: &ServerEntity, kinds: &KindIds) -> Vec<Packet> {
    use rustmc_proto::ids::play::cb;
    let Some(kind_id) = kinds.get(e.kind) else {
        // An unresolved kind means the registry capture does not hold this
        // type. Sending nothing is the honest answer: an entity announced
        // with the wrong type id is a different entity, and would read as a
        // divergence about behaviour rather than about a missing table.
        return Vec::new();
    };
    let (x_rot, y_rot, y_head_rot) = s.last_sent_rotations();
    let Ok(add) = entities::add_entity_body(&entities::Arrival {
        id: e.id,
        uuid: e.uuid,
        kind_id,
        at: entities::hanging_block_pos(e).unwrap_or_else(|| s.position_base()),
        movement: s.last_sent_movement(),
        x_rot,
        y_rot,
        y_head_rot,
        data: e.data,
    }) else {
        return Vec::new();
    };
    let mut out = vec![(cb::BUNDLE_DELIMITER, Vec::new()), (cb::ADD_ENTITY, add)];
    // Offsets 48-74. `getNonDefaultValues` returning null — `None` here — is
    // the whole of the guard: an entity every one of whose accessors is at
    // its definition gets no packet, not an empty one.
    //
    // A mob's values live on its host (`crate::mob`), which also answers
    // offsets 85-137 — `getSyncableAttributes()` — for it.
    if let Some((data, attributes)) = crate::mob::pairing_extras(e.id) {
        if let Some(body) = data {
            out.push((cb::SET_ENTITY_DATA, body));
        }
        if let Some(body) = attributes {
            out.push((cb::UPDATE_ATTRIBUTES, body));
        }
    } else if let Some(values) = e.synched.non_default_values() {
        if let Ok(body) = crate::synched::set_entity_data_body(e.id, &values) {
            out.push((cb::SET_ENTITY_DATA, body));
        }
    }
    // Offsets 333-387: a leashed entity's link, the last thing in the bundle.
    if let Some(body) = crate::mob::pairing_link(e.id) {
        out.push((cb::SET_ENTITY_LINK, body));
    }
    out.push((cb::BUNDLE_DELIMITER, Vec::new()));
    out
}

/// Synched data left dirty on an entity that is not a mob — the part of
/// `SynchedEntityData.isDirty()` this server's generic entities can have.
///
/// A value set after construction (`EndCrystal.setShowBottom(false)` in
/// `EndCrystalItem.useOn`) is both a non-default value, so the pairing bundle
/// carries it, **and** dirty, so the entity's next `sendChanges` carries it
/// again as `sendDirtyEntityData`. Vanilla's recording of `an-end-crystal-
/// sits-on-obsidian` has exactly that: the value in the bundle and a second
/// `set_entity_data` after the click's block resyncs. Mobs keep their own
/// dirty tracking in `crate::mob`; this is the rest.
static DATA_DIRTY: std::sync::OnceLock<Mutex<std::collections::BTreeMap<i32, Vec<u8>>>> =
    std::sync::OnceLock::new();

/// Mark an entity's synched data dirty with the `set_entity_data` body its
/// next `sendChanges` should carry.
pub fn mark_data_dirty(id: i32, body: Vec<u8>) {
    if let Ok(mut m) = DATA_DIRTY.get_or_init(Mutex::default).lock() {
        m.insert(id, body);
    }
}

fn take_data_dirty(id: i32) -> Option<DataDirty> {
    let data = DATA_DIRTY.get()?.lock().ok()?.remove(&id)?;
    Some(DataDirty(std::cell::Cell::new(Some(data))))
}

/// A non-mob's dirty `set_entity_data` body as a [`DirtySource`]: dirty while
/// it holds one, drained by the read at 1093, never `needsSync` or
/// `hurtMarked`.
struct DataDirty(std::cell::Cell<Option<Vec<u8>>>);

impl DirtySource for DataDirty {
    fn flags(&self) -> (bool, bool) {
        let body = self.0.take();
        let dirty = body.is_some();
        self.0.set(body);
        (dirty, false)
    }
    fn drain(&self) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
        (self.0.take(), None)
    }
    fn take_hurt_marked(&self) -> bool {
        false
    }
}

/// The entity half of `ChunkMap`: every tracked entity, and the tick that
/// drives them.
#[derive(Debug)]
pub struct Tracker {
    tracked: Mutex<IntOpenHashMap<TrackedEntity>>,
    /// `DedicatedServerProperties.entityBroadcastRangePercentage`, which
    /// `getEffectiveRange` reads through `scaledRange` on every
    /// `updatePlayer`.
    ///
    /// Atomic and not a plain field because vanilla's is a
    /// `Settings$MutableValue` that `DedicatedServer
    /// .setEntityBroadcastRangePercentage` writes at run time; reading it
    /// live is what makes the tracking range follow the setting rather than
    /// the boot.
    percentage: std::sync::atomic::AtomicI32,
}

impl Default for Tracker {
    fn default() -> Self {
        Self {
            tracked: Mutex::new(IntOpenHashMap::default()),
            // `DedicatedServerProperties` registers the key with a default of
            // 100 (`getMutable(String, Function, Integer)` at offsets
            // 670-686, `bipush 100` at 678), and 100 is what makes
            // `getScaledTrackingDistance` the identity.
            percentage: std::sync::atomic::AtomicI32::new(DEFAULT_BROADCAST_RANGE_PERCENTAGE),
        }
    }
}

/// `entity-broadcast-range-percentage`'s default, from
/// `DedicatedServerProperties.<init>` offset 678.
pub const DEFAULT_BROADCAST_RANGE_PERCENTAGE: i32 = 100;

impl Tracker {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `DedicatedServer.setEntityBroadcastRangePercentage`.
    ///
    /// The value is expected already clamped —
    /// `crate::properties::Properties` does it where vanilla does, at parse.
    pub fn set_broadcast_range_percentage(&self, percentage: i32) {
        self.percentage
            .store(percentage, std::sync::atomic::Ordering::Release);
    }

    /// What `getEffectiveRange` will scale by on the next `updatePlayer`.
    #[must_use]
    pub fn broadcast_range_percentage(&self) -> i32 {
        self.percentage.load(std::sync::atomic::Ordering::Acquire)
    }

    /// The process-wide tracker.
    #[must_use]
    pub fn shared() -> &'static Self {
        static SHARED: OnceLock<Tracker> = OnceLock::new();
        SHARED.get_or_init(Tracker::new)
    }

    /// `ChunkMap.addEntity`.
    ///
    /// An entity whose type has `clientTrackingRange() == 0` is **not tracked
    /// at all** — vanilla returns before constructing anything — so the guard
    /// is a real branch and not defensive coding.
    ///
    /// # Panics
    /// If the lock is poisoned.
    pub fn add(&self, e: &Entity, players: &[TrackedPlayer], kinds: &KindIds) {
        if e.kind.tracking_range_chunks * 16 == 0 {
            return;
        }
        let pct = self.broadcast_range_percentage();
        let mut map = self.tracked.lock().expect("tracker");
        let t = map.get_or_insert_with(e.id, || TrackedEntity::new(e));
        // `tracked.updatePlayers(level.players())`: show the new entity to
        // everyone already here.
        for p in players {
            if let Some(packets) = t.update_player(e, p, kinds, pct) {
                send(&p.conn, &packets);
            }
        }
    }

    /// The second half of `ChunkMap.addEntity`, for a joining **player**:
    /// show the newcomer every entity that is already tracked.
    ///
    /// Vanilla walks `entityMap.values()` and calls `updatePlayer(newPlayer)`
    /// on each, skipping the one whose entity is the newcomer itself. Without
    /// this a second player sees an empty world and the first player sees the
    /// second — which is exactly the asymmetry the corpus would report as one
    /// client diverging and the other matching.
    ///
    /// # Panics
    /// If the lock is poisoned.
    pub fn show_world_to(&self, joiner: &TrackedPlayer, entities: &Entities, kinds: &KindIds) {
        let pct = self.broadcast_range_percentage();
        let mut map = self.tracked.lock().expect("tracker");
        map.for_each_mut(|id, t| {
            if id == joiner.entity_id {
                return;
            }
            if let Some(e) = entities.get(id) {
                if let Some(packets) = t.update_player(&e, joiner, kinds, pct) {
                    send(&joiner.conn, &packets);
                }
            }
        });
    }

    /// `ChunkMap.removeEntity`: drop it from the map and tell everyone who
    /// could see it.
    ///
    /// # Panics
    /// If the lock is poisoned.
    pub fn remove(&self, id: i32, players: &[TrackedPlayer]) {
        let Some(t) = self.tracked.lock().expect("tracker").remove(id) else {
            return;
        };
        let Ok(body) = entities::remove_entities_body(&[id]) else {
            return;
        };
        for p in players {
            if t.seen_by.contains(&p.conn.id()) {
                let _ = p
                    .conn
                    .send(rustmc_proto::ids::play::cb::REMOVE_ENTITIES, &body);
            }
        }
    }

    /// A departing **player** also stops being an observer: vanilla calls
    /// `removePlayer` on every tracked entity so no set holds a dead
    /// connection.
    ///
    /// One deliberate difference, named rather than left: vanilla's
    /// `TrackedEntity.removePlayer` goes through `ServerEntity.removePairing`,
    /// which *sends* a `remove_entities` to the player who is leaving. This
    /// only drops them from the set. The packets vanilla sends there go to a
    /// socket that is closing and no client ever acts on them, so the
    /// difference is unobservable — but it is a difference, and if a future
    /// check ever records a departing client's last frames it is this line
    /// that is wrong.
    ///
    /// # Panics
    /// If the lock is poisoned.
    pub fn forget_observer(&self, conn_id: u64) {
        self.tracked.lock().expect("tracker").for_each_mut(|_, t| {
            t.seen_by.remove(&conn_id);
        });
    }

    /// `ChunkMap.removeEntity` for a **player** that stays connected — the
    /// removal `LivingEntity.tickDeath` and `PlayerList.respawn` make:
    /// every tracked entity's `removePlayer`, which goes through
    /// `ServerEntity.removePairing` and sends that one entity's
    /// `ClientboundRemoveEntitiesPacket` to the player, one packet per entity,
    /// in the entity map's own walk order.
    ///
    /// Unlike [`Self::forget_observer`] the client is still reading, so the
    /// packets are sent rather than skipped.
    ///
    /// # Panics
    /// If the lock is poisoned.
    pub fn untrack_observer(&self, observer: &Connection) {
        let mut gone = Vec::new();
        self.tracked.lock().expect("tracker").for_each_mut(|id, t| {
            if t.seen_by.remove(&observer.id()) {
                gone.push(id);
            }
        });
        for id in gone {
            if let Ok(body) = entities::remove_entities_body(&[id]) {
                let _ = observer.send(rustmc_proto::ids::play::cb::REMOVE_ENTITIES, &body);
            }
        }
    }

    /// `ServerChunkCache.sendToTrackingPlayers` -> `ChunkMap.broadcast`:
    /// one packet to everybody in this entity's `seenBy`, and to nobody else.
    ///
    /// # This is not `Players::broadcast` and the difference is the check
    ///
    /// `seenBy` **never contains the entity's own player** —
    /// `TrackedEntity.updatePlayer` returns at its first line when
    /// `player == entity`, and [`TrackedEntity::update_player`] reproduces
    /// that. So a swing sends the actor nothing, and a check that watched the
    /// actor would compare silence with silence and pass against a server
    /// that ignores `swing` entirely.
    ///
    /// The sibling `sendToTrackingPlayersAndSelf` exists and is the *other*
    /// branch of `LivingEntity.swing(hand, boolean)`, at offset 89. Nothing
    /// reaches it from here: `swing(hand)` passes `iconst_0`.
    ///
    /// # Panics
    /// If the lock is poisoned.
    pub fn send_to_tracking_players(
        &self,
        entity_id: i32,
        players: &[TrackedPlayer],
        packet: i32,
        body: &[u8],
    ) {
        let map = self.tracked.lock().expect("tracker");
        let Some(t) = map.get(entity_id) else {
            return;
        };
        for p in players {
            if t.seen_by.contains(&p.conn.id()) {
                send(&p.conn, &[(packet, body.to_vec())]);
            }
        }
    }

    /// `ChunkMap.tick()`, entity half.
    ///
    /// # Panics
    /// If the lock is poisoned.
    pub fn tick(&self, entities: &Entities, players: &[TrackedPlayer], kinds: &KindIds) {
        // Read once for the whole tick, not once per pair: vanilla's
        // `scaledRange` re-reads the property on every `updatePlayer`, but
        // within one `ChunkMap.tick` nothing can write it — the game thread
        // is the only writer and it is running this.
        let pct = self.broadcast_range_percentage();
        let mut map = self.tracked.lock().expect("tracker");
        let live = entities.all();
        // `movedPlayers` — the players whose *own* entity changed section this
        // tick, collected here and swept against every tracked entity below.
        let mut moved_players: Vec<usize> = Vec::new();
        for e in &live {
            let Some(t) = map.get_mut(e.id) else {
                continue;
            };
            let now = e.section();
            let moved_section = t.last_section != Some(now);
            if moved_section {
                for p in players {
                    if let Some(packets) = t.update_player(e, p, kinds, pct) {
                        send(&p.conn, &packets);
                    }
                }
                if let Some(i) = players.iter().position(|p| p.entity_id == e.id) {
                    moved_players.push(i);
                }
                t.last_section = Some(now);
            }
            // Vanilla's third limb is `inEntityTickingRange`, which asks
            // whether the entity's chunk is close enough to *some* player to
            // be ticked at all. This server has no ticking-range manager, so
            // the condition is true whenever anyone can see the entity —
            // which is a superset only in the case where nobody can, and in
            // that case `seen_by` is empty and the packets go nowhere.
            // `hurtMarked` is one boolean in vanilla with two writers here:
            // the mob path (`DirtySource::take_hurt_marked`) and `/damage`'s
            // `markHurt` (`damagecmd`). For a mob the two are OR-ed into the
            // one read inside `send_changes_with`; for anything else the
            // damagecmd arm below sends it. One `set_entity_motion`, not two.
            let dmg_hurt_mark = crate::damagecmd::take_hurt_mark(e.id);
            let mob = crate::mob::is_mob(e.id).then_some(crate::mob::MobSync(e.id));
            // Anything else with data marked dirty (`mark_data_dirty`: an end
            // crystal, an item frame) reads it through the same gate.
            let data = if mob.is_none() {
                take_data_dirty(e.id)
            } else {
                None
            };
            let source = mob.as_ref().map(|m| HurtMarkOr {
                inner: m,
                extra: dmg_hurt_mark,
            });
            let dmg_hurt_mark = dmg_hurt_mark && mob.is_none();
            // An item entity a dispenser spawned is the third writer of
            // `entityData` here (`DATA_ITEM`, once): `crate::dispenser`.
            let item = crate::dispenser::ItemSync(e);
            // And a block's drop (`crate::synched`'s dirty set, mining's
            // `DATA_ITEM` once): the fourth.
            let dropped = crate::synched::DroppedSync(e);
            // `train-f`'s three: a player's score (`crate::experience`) —
            // `sendDirtyEntityData` -> `broadcastAndSend`, so it also goes
            // to the player itself, which `seen_by` never contains — an
            // orb's value (`crate::xporb`) and an item entity's state
            // (`crate::itementity`).
            let (other, own) = if mob.is_some() {
                (None, None)
            } else if e.kind == crate::entities::PLAYER {
                let body = crate::experience::drain_dirty_score(e.id);
                let own = body.clone();
                (
                    body.map(|data| {
                        Drained::new(crate::mob::Dirty {
                            data: Some(data),
                            attributes: None,
                            needs_sync: false,
                        })
                    }),
                    own,
                )
            } else if e.kind == crate::entities::EXPERIENCE_ORB {
                (crate::xporb::drain_dirty(e.id).map(Drained::new), None)
            } else {
                (crate::itementity::drain_dirty(e.id).map(Drained::new), None)
            };
            let packets = t.server.send_changes_with(
                e,
                source
                    .as_ref()
                    .map(|m| m as &dyn DirtySource)
                    .or(data.as_ref().map(|d| d as &dyn DirtySource))
                    .or(other.as_ref().map(|d| d as &dyn DirtySource))
                    .or_else(|| {
                        crate::dispenser::is_dirty_item(e.id).then_some(&item as &dyn DirtySource)
                    })
                    .or_else(|| {
                        crate::synched::is_dirty(e.id).then_some(&dropped as &dyn DirtySource)
                    }),
            );
            for p in players {
                if !packets.is_empty() && t.seen_by.contains(&p.conn.id()) {
                    send(&p.conn, &packets);
                }
            }
            if let Some(body) = own {
                if let Some(me) = players.iter().find(|p| p.entity_id == e.id) {
                    send(
                        &me.conn,
                        &[(rustmc_proto::ids::play::cb::SET_ENTITY_DATA, body)],
                    );
                }
            }
            // `ServerEntity.sendChanges` offsets 1202..1235, after
            // `tickCount++` and outside the interval block: a hurt entity's
            // velocity goes to `sendToTrackingPlayersAndSelf` on the very
            // next tick, whatever the interval. `markHurt` is the only
            // writer, in `LivingEntity.hurtServer` offset 349. The "and
            // self" half is the player's own loop's, for the ordering reason
            // given at `sendDirtyEntityData` above.
            if dmg_hurt_mark {
                if let Ok(body) = set_entity_motion_body(e.id, e.movement) {
                    let packet = [(rustmc_proto::ids::play::cb::SET_ENTITY_MOTION, body)];
                    for p in players {
                        if t.seen_by.contains(&p.conn.id()) {
                            send(&p.conn, &packet);
                        }
                    }
                }
            }
        }

        // `if (!movedPlayers.isEmpty()) for (TrackedEntity t : entityMap.values())
        //      t.updatePlayers(movedPlayers);`
        //
        // **This loop was missing from the first version of this port and the
        // omission is not cosmetic.** The main loop above re-runs
        // `updatePlayer` when the *entity* changes section, which covers an
        // entity moving towards or away from a stationary player. It does not
        // cover the other half — a **player** walking towards or away from a
        // **stationary entity** — because nothing about that entity changed,
        // so its branch never fires.
        //
        // Without this, a player who walks up to a dropped item is never told
        // the item exists, and a player who walks away from one keeps it on
        // screen for ever. It is invisible in any scenario where the entity is
        // another player, because then both entities move and the main loop
        // happens to cover it, which is exactly why it survived the two checks
        // this build was measured on.
        if !moved_players.is_empty() {
            let by_id: BTreeMap<i32, &Entity> = live.iter().map(|e| (e.id, e)).collect();
            map.for_each_mut(|id, t| {
                let Some(e) = by_id.get(&id) else { return };
                for &i in &moved_players {
                    let p = &players[i];
                    if let Some(packets) = t.update_player(e, p, kinds, pct) {
                        send(&p.conn, &packets);
                    }
                }
            });
        }
    }
}

/// Write a group of packets to one connection, dropping errors for the reason
/// `Players::broadcast` does: a dead socket is its own thread's problem.
fn send(conn: &Connection, packets: &[Packet]) {
    for (id, body) in packets {
        let _ = conn.send(*id, body);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{Entity, PLAYER};

    /// A dirty data watcher forces the body off the interval and its packet
    /// follows the position packet (`sendChanges` 1076-1093).
    #[test]
    fn dirty_data_forces_the_body_and_follows_the_position_packet() {
        use rustmc_proto::ids::play::cb;
        let mut e = Entity {
            id: 7,
            uuid: 0,
            kind: crate::entities::EXPERIENCE_ORB,
            x: 0.0,
            y: 0.0,
            z: 0.0,
            y_rot: 0.0,
            x_rot: 0.0,
            y_head_rot: 0.0,
            movement: (0.0, 0.0, 0.0),
            on_ground: false,
            data: 0,
            swinging: false,
            swing_time: 0,
            swinging_arm: None,
            synched: crate::synched::Synched::ExperienceOrb { value: 3 },
        };
        let mut s = ServerEntity::new(&e);
        let dirty = || {
            Drained::new(crate::mob::Dirty {
                data: Some(vec![7, 8, 1, 3, 0xff]),
                attributes: None,
                needs_sync: false,
            })
        };
        // Tick 0 is on the interval: the heartbeat, then the data.
        let kinds: Vec<i32> = s
            .send_changes_with(&e, Some(&dirty() as &dyn DirtySource))
            .iter()
            .map(|p| p.0)
            .collect();
        assert_eq!(kinds, vec![cb::MOVE_ENTITY_POS, cb::SET_ENTITY_DATA]);
        // Tick 1 is off the interval (20): nothing without dirt ...
        e.x = 1.0;
        assert!(s.send_changes_with(&e, None).is_empty());
        // ... and the body with it, position first.
        let kinds: Vec<i32> = s
            .send_changes_with(&e, Some(&dirty() as &dyn DirtySource))
            .iter()
            .map(|p| p.0)
            .collect();
        assert_eq!(kinds, vec![cb::MOVE_ENTITY_POS, cb::SET_ENTITY_DATA]);
    }

    /// `animate`'s body is a varint and then a **byte**.
    ///
    /// The entity id is chosen above 127 on purpose: every action value fits
    /// in one byte either way, so a varint written where a byte belongs is
    /// invisible for the action and only shows up as a body one byte too
    /// long — which is why the length is asserted rather than the fields
    /// alone. A two-byte id is what makes the two lengths differ.
    #[test]
    fn an_animate_body_is_a_varint_id_then_one_byte() {
        let b = animate_body(300, 3).expect("buf");
        assert_eq!(b.len(), 3, "two varint bytes for 300, one for the action");
        assert_eq!(&b[..2], &[0xAC, 0x02]);
        assert_eq!(b[2], 3);
        // And a small id is two bytes in total, not three.
        assert_eq!(animate_body(1, 0).expect("buf"), vec![1, 0]);
    }

    fn player_at(id: i32, x: f64, y: f64, z: f64) -> Entity {
        Entity {
            id,
            uuid: 0,
            kind: PLAYER,
            x,
            y,
            z,
            y_rot: 0.0,
            x_rot: 0.0,
            y_head_rot: 0.0,
            movement: (0.0, 0.0, 0.0),
            on_ground: true,
            data: 0,
            // `LivingEntity`'s own initial values: not swinging, and a null
            // `swingingArm`, which `getCurrentSwingDuration` reads as the main
            // hand.
            swinging: false,
            swing_time: 0,
            swinging_arm: None,
            // What a client this harness — or a real one — configures with:
            // max health, and every skin part displayed.
            synched: crate::synched::Synched::Avatar {
                health: 20.0,
                model_customisation: 0x7f,
                score: 0,
            },
        }
    }

    /// `sendPairingData`'s order, and the negative control that goes with it.
    ///
    /// The order is load-bearing because the corpus compares **sequences**:
    /// `add_entity` then `set_entity_data`, both inside one bundle. Emitting
    /// the same two packets the other way round, or outside the bundle, is a
    /// divergence that a per-kind count cannot see.
    ///
    /// The control is `add_entity`'s body: adding a packet to this bundle
    /// must not move a byte of it. `530`, `520` and `080` all read it, and
    /// its recorded tail digest on both sides is `dc545a7f9dce13f5`, so a
    /// refactor that changed it would trade one scenario for three.
    #[test]
    fn the_pairing_bundle_is_add_entity_then_set_entity_data_inside_one_bundle() {
        use rustmc_proto::ids::play::cb;
        let kinds = kinds_for_test();
        let p = player_at(7, 0.0, 64.0, 0.0);
        let s = ServerEntity::new(&p);
        let before = pairing_bundle(&p, &s, &kinds);
        assert_eq!(
            before.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec![
                cb::BUNDLE_DELIMITER,
                cb::ADD_ENTITY,
                cb::SET_ENTITY_DATA,
                cb::BUNDLE_DELIMITER
            ]
        );
        // The payload, not the packet: accessor 9 as a float and accessor 16
        // as a byte, after the entity varint, terminated by the EOF marker.
        assert_eq!(
            before[2].1,
            vec![0x07, 0x09, 0x03, 0x41, 0xA0, 0x00, 0x00, 0x10, 0x00, 0x7F, 0xFF]
        );

        // An entity with nothing away from its definitions gets the bundle it
        // got before this existed — three packets, not four with an empty one.
        let mut bare = player_at(7, 0.0, 64.0, 0.0);
        bare.synched = crate::synched::Synched::EntityOnly;
        let after = pairing_bundle(&bare, &s, &kinds);
        assert_eq!(
            after.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec![cb::BUNDLE_DELIMITER, cb::ADD_ENTITY, cb::BUNDLE_DELIMITER]
        );
        // The negative control: same `add_entity` bytes either way.
        assert_eq!(before[1].1, after[1].1);
    }

    /// The heartbeat's period and phase, which is the whole content of
    /// `520-a-standing-player-is-rebroadcast-on-a-timer`.
    ///
    /// A standing player at `updateInterval` 2 must produce a `move_entity_pos`
    /// with zero deltas on tick 0 and then every 60 ticks — **not every 60
    /// body runs**, which would be every 120 ticks and would look just as
    /// plausible.
    #[test]
    fn a_standing_player_is_rebroadcast_every_sixty_ticks() {
        let e = player_at(7, 10.0, 64.0, 10.0);
        let mut s = ServerEntity::new(&e);
        let mut beats = Vec::new();
        for tick in 0..181 {
            for (id, body) in s.send_changes(&e) {
                assert_eq!(id, rustmc_proto::ids::play::cb::MOVE_ENTITY_POS);
                assert_eq!(
                    body,
                    vec![7, 0, 0, 0, 0, 0, 0, 1],
                    "zero deltas and onGround, at tick {tick}"
                );
                beats.push(tick);
            }
        }
        assert_eq!(
            beats,
            vec![0, 60, 120, 180],
            "period 60 from the entity's own counter"
        );
    }

    /// Two periods of the counter is two beats for every phase — the property
    /// the check's 6000 ms window is chosen for. Asserted over every offset
    /// so a fencepost cannot pass.
    #[test]
    fn any_120_tick_window_holds_exactly_two_heartbeats() {
        for phase in 0..60 {
            let e = player_at(1, 0.0, 64.0, 0.0);
            let mut s = ServerEntity::new(&e);
            for _ in 0..phase {
                s.send_changes(&e);
            }
            let mut n = 0;
            for _ in 0..120 {
                n += s.send_changes(&e).len();
            }
            assert_eq!(n, 2, "phase {phase}");
        }
    }

    /// A player that walks produces real deltas, in 1/4096 of a block, and
    /// the base advances so the next tick measures from the new position.
    #[test]
    fn a_walking_player_sends_a_delta_and_moves_its_base() {
        let mut e = player_at(3, 0.0, 64.0, 0.0);
        let mut s = ServerEntity::new(&e);
        s.send_changes(&e); // tick 0: the heartbeat, base unchanged at 0
        e.x = 0.5;
        // tick 1 is not a multiple of updateInterval 2, so nothing yet.
        assert!(s.send_changes(&e).is_empty(), "the interval gate holds");
        let out = s.send_changes(&e);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, rustmc_proto::ids::play::cb::MOVE_ENTITY_POS);
        // 0.5 * 4096 = 2048.
        assert_eq!(&out[0].1[1..3], &2048_i16.to_be_bytes());
        assert!((s.position_base().0 - 0.5).abs() < f64::EPSILON);
    }

    /// A turn with no movement is a `move_entity_rot`, and it does **not**
    /// advance the position base — the two "last sent" states are
    /// independent, and conflating them makes a player who turns on the spot
    /// lose a fraction of a block on its next real move.
    #[test]
    fn turning_on_the_spot_sends_a_rotation_and_leaves_the_base_alone() {
        let mut e = player_at(4, 100.25, 64.0, -3.5);
        let mut s = ServerEntity::new(&e);
        s.send_changes(&e); // tick 0 heartbeat
        e.y_rot = 90.0;
        s.send_changes(&e); // tick 1: gated out
        let out = s.send_changes(&e);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, rustmc_proto::ids::play::cb::MOVE_ENTITY_ROT);
        assert_eq!(out[0].1[1], 64, "packDegrees(90)");
        assert!((s.position_base().0 - 100.25).abs() < f64::EPSILON);
    }

    /// Landing changes the ground flag, which forces a full position sync
    /// rather than a delta — one of the five teleport conditions.
    #[test]
    fn a_change_of_ground_flag_forces_a_position_sync() {
        let mut e = player_at(5, 0.0, 64.0, 0.0);
        e.on_ground = false;
        let mut s = ServerEntity::new(&e);
        s.send_changes(&e);
        e.on_ground = true;
        s.send_changes(&e);
        let out = s.send_changes(&e);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, rustmc_proto::ids::play::cb::ENTITY_POSITION_SYNC);
    }

    /// The teleport packet's exact size, which is the assertion that catches
    /// an `LpVec3` written where three doubles belong: 1 + 24 + 24 + 4 + 4 + 1.
    /// With `LpVec3` for a stationary entity the body is 34 bytes, not 58,
    /// and every field the client reads after it is garbage.
    #[test]
    fn a_position_sync_carries_two_full_vectors_and_two_floats() {
        let e = player_at(3, 1.0, 2.0, 3.0);
        let body = entity_position_sync_body(&e).expect("buf");
        assert_eq!(body.len(), 1 + 24 + 24 + 4 + 4 + 1);
        assert_eq!(&body[1..9], &1.0_f64.to_be_bytes(), "x");
        assert_eq!(&body[25..33], &0.0_f64.to_be_bytes(), "dx, a whole double");
        assert_eq!(body[57], 1, "onGround");
    }

    /// The range test is horizontal: an entity far above a player is still in
    /// range, where a 3D test would drop it.
    #[test]
    fn the_tracking_range_ignores_height() {
        let mut t = TrackedEntity::new(&player_at(1, 0.0, 0.0, 0.0));
        assert_eq!(t.range, 512, "32 chunks * 16");
        // Reproduce the predicate directly: the y separation is 300 blocks
        // and the horizontal one is zero.
        let limit = f64::from(t.range.min(10 * 16));
        let (dx, dz) = (0.0_f64, 0.0_f64);
        assert!((dx * dx + dz * dz) <= limit * limit);
        t.last_section = Some((0, 0, 0));
    }

    /// A writer whose bytes can be read back after the `Connection` that owns
    /// it is gone.
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

    /// Open a test connection's chunk view the way a join opens one.
    ///
    /// `updatePlayer`'s third limb is `ChunkMap.isChunkTracked`, which reads
    /// this. A connection that has never had `updatePlayerStatus(p, true)`
    /// run on it still holds `ChunkTrackingView.EMPTY`, which contains
    /// nothing — so without this every pairing test measures the empty view
    /// rather than the range it names.
    ///
    /// The square is `view_distance + 3` on a side: the tracked set is a disc
    /// padded by two, so it reaches one chunk past `view_distance` along an
    /// axis, and a chunk left out of `mark_sent` would be *pending* and
    /// therefore not tracked either.
    fn open_view(conn: &Connection, centre: (i32, i32), view_distance: i32) {
        conn.with_chunk_view(|v| {
            let side = view_distance + 3;
            for dx in -side..=side {
                for dz in -side..=side {
                    v.mark_sent((centre.0 + dx, centre.1 + dz));
                }
            }
            v.update_tracking(centre);
        });
    }

    fn kinds_for_test() -> KindIds {
        let mut m = std::collections::BTreeMap::new();
        m.insert("minecraft:player".to_string(), 156);
        m.insert("minecraft:item".to_string(), 71);
        KindIds::resolve(&m, &[crate::entities::PLAYER, crate::entities::ITEM])
    }

    /// How many `add_entity` frames a recorder saw.
    fn added(rec: &Recorder) -> usize {
        let bytes = rec.0.lock().expect("recorder").clone();
        let mut cur = &bytes[..];
        let mut n = 0;
        while let Ok((id, _)) = rustmc_proto::frame::read_frame(&mut cur, false) {
            if id == rustmc_proto::ids::play::cb::ADD_ENTITY {
                n += 1;
            }
        }
        n
    }

    /// **The velocity limb fires for an item and not for a player**, which is
    /// the opposite of what this server did until 2026-08-05.
    ///
    /// `ServerEntity.sendChanges` enters it on
    /// `needsSync || trackDelta || (LivingEntity && isFallFlying)` — offsets
    /// 874-919, `ifne` on `trackDelta` at 887. `EntityType.trackDeltas()` is
    /// false for exactly ten types, PLAYER among them, and true for ITEM.
    ///
    /// Written as both directions because the defect was a **sign**, and a
    /// one-sided test passes under either sign whenever the other case
    /// happens to emit nothing — which is precisely how it survived: a
    /// player's movement is always zero here, so the branch we wrongly took
    /// was silent, and the branch we wrongly skipped was the only one with a
    /// velocity in it.
    #[test]
    fn velocity_is_broadcast_for_an_item_and_not_for_a_player() {
        use rustmc_proto::ids::play::cb;

        fn motions(kind: crate::entities::EntityKind, movement: (f64, f64, f64)) -> usize {
            let mut e = player_at(7, 0.0, 64.0, 0.0);
            e.kind = kind;
            let mut s = ServerEntity::new(&e);
            // `ServerEntity::new` seeds `last_sent_movement` from the entity,
            // so the velocity has to arrive *after* tracking began for the
            // delta test to see it — which is what a throw does.
            e.movement = movement;
            (0..4)
                .flat_map(|_| s.send_changes(&e))
                .filter(|(id, _)| *id == cb::SET_ENTITY_MOTION)
                .count()
        }

        assert_eq!(
            motions(crate::entities::ITEM, (0.3, 0.2, -0.1)),
            1,
            "an item's throw velocity is broadcast once: trackDeltas is true"
        );
        assert_eq!(
            motions(crate::entities::PLAYER, (0.3, 0.2, -0.1)),
            0,
            "a player's is not: trackDeltas is false and needsSync is unmodelled"
        );
        // Not passing by never sending anything: a still item is silent too,
        // so the item case above is the velocity and not the kind.
        assert_eq!(
            motions(crate::entities::ITEM, (0.0, 0.0, 0.0)),
            0,
            "no velocity, no packet"
        );
    }

    /// **`add_entity` announces the tracker's last-sent movement, and after
    /// ORACLE rule 26 this test is the only thing claiming that it does.**
    ///
    /// `trace::decode` now consumes and discards
    /// `ClientboundAddEntityPacket`'s movement, because that field is nobody's
    /// behaviour at the moment an entity is announced: for a standing player
    /// it depends on which tick vanilla's last motion packet went out on, and
    /// for a thrown item it is four fresh `nextFloat()` draws from an unseeded
    /// `RandomSource`. It cost `530-walking-away-drops-a-standing-player-from-view`
    /// a `match` floor and put `dropping-an-item-throws-it-on-the-floor` in
    /// `ci/checks/gameplay-unstable.toml` at 25/25.
    ///
    /// The gameplay corpus was covering this plumbing incidentally and no
    /// longer does, so the coverage moves here. An exemption with nothing
    /// behind it manufactures a check that cannot fail, and this corpus has
    /// found fifteen of those by accident already.
    ///
    /// Two properties, and the second is one no corpus check could have caught
    /// anyway:
    ///
    ///   * a moving entity's `add_entity` carries its movement — six bytes of
    ///     `LpVec3` — and not the single zero byte of `Vec3.ZERO`;
    ///   * it carries the **last-sent** movement, not the entity's live one.
    ///     The packet's two-argument constructor reads
    ///     `serverEntity.getLastSentMovement()`, so an entity whose velocity
    ///     has changed since its last broadcast is announced with the older
    ///     value — the one the client's dead reckoning is already extrapolating
    ///     from. Seeding the packet off `e.movement` instead would look
    ///     correct in every scenario where nothing has changed yet, which is
    ///     every scenario the corpus has.
    #[test]
    fn add_entity_announces_the_last_sent_movement() {
        let kinds = kinds_for_test();
        // varint id 7, 16 bytes of uuid, varint 156 for `minecraft:player`
        // (two bytes, being >= 128), three doubles of position — then the
        // movement.
        const MOVEMENT_AT: usize = 1 + 16 + 2 + 24;

        fn add_body_of(pkts: &[Packet]) -> Vec<u8> {
            let (id, body) = pkts
                .iter()
                .find(|(id, _)| *id == rustmc_proto::ids::play::cb::ADD_ENTITY)
                .expect("an add_entity in the pairing bundle");
            assert_eq!(*id, rustmc_proto::ids::play::cb::ADD_ENTITY);
            body.clone()
        }

        // Control: a still entity is announced with one zero byte, which is
        // what makes the positive case below mean something.
        let still = player_at(7, 0.0, 64.0, 0.0);
        let s = ServerEntity::new(&still);
        let body = add_body_of(&pairing_bundle(&still, &s, &kinds));
        assert_eq!(
            body[MOVEMENT_AT], 0,
            "Vec3.ZERO is a single zero byte at {MOVEMENT_AT}"
        );
        assert_eq!(body.len(), MOVEMENT_AT + 1 + 3 + 1, "one byte of movement");

        // A tracker whose last broadcast carried a real velocity...
        let mut moving = player_at(7, 0.0, 64.0, 0.0);
        moving.movement = (0.5, 0.0, 0.0);
        let s = ServerEntity::new(&moving);

        // ...and an entity that has since stopped, with no broadcast in
        // between. The packet must follow the tracker, not the entity.
        let mut stopped = moving.clone();
        stopped.movement = (0.0, 0.0, 0.0);

        let body = add_body_of(&pairing_bundle(&stopped, &s, &kinds));
        assert_ne!(
            body[MOVEMENT_AT], 0,
            "the last-sent movement, not the entity's live zero"
        );
        assert_eq!(
            body.len(),
            MOVEMENT_AT + 6 + 3 + 1,
            "six bytes of LpVec3: scale 1 needs no varint continuation"
        );
        assert_eq!(body[MOVEMENT_AT] & 3, 1, "ceil(0.5) == 1");
        assert_eq!(body[MOVEMENT_AT] & 4, 0, "no scale continuation");
    }

    /// `getScaledTrackingDistance` is arithmetic, and here is the arithmetic.
    ///
    /// `DedicatedServer`'s override is `pct * range / 100` with Java's
    /// truncating `idiv`, so 33% of a player's 512 blocks is 168 and not
    /// 168.96 or 169. The identity at 100 is asserted too, because that is
    /// the value every gate this project runs uses — and it is exactly why
    /// this could be missing for a month without a single number moving.
    #[test]
    fn the_broadcast_percentage_scales_a_range_and_truncates() {
        assert_eq!(scaled_range(512, 100), 512, "the identity at the default");
        assert_eq!(scaled_range(512, 10), 51, "5120 / 100, truncated from 51.2");
        assert_eq!(scaled_range(512, 33), 168, "16896 / 100, not 169");
        assert_eq!(scaled_range(512, 1000), 5120);
        assert_eq!(scaled_range(0, 1000), 0, "a range of zero stays zero");
        // The multiply is before the divide, which is what makes a small
        // range survive a small percentage. Doing it the other way round
        // gives `(80 / 100) * 10 = 0` and a chicken nobody can see.
        assert_eq!(scaled_range(80, 10), 8, "not (80 / 100) * 10 == 0");
    }

    /// The scale is not a number in a table — it moves what a client is sent.
    ///
    /// Two identical trackers, two identical players 200 blocks apart, and
    /// the only difference is the percentage. At 100 the item's 96-block
    /// range binds and nothing is paired; at 1000 the range is 960 and the
    /// pairing goes out. Without [`scaled_range`] in `getEffectiveRange`
    /// both halves would say the same thing, which is the shape of a setting
    /// that is read and never used.
    #[test]
    fn the_broadcast_percentage_changes_who_is_paired() {
        let paired_at = |pct: i32| {
            let entities = Entities::new();
            let tracker = Tracker::new();
            tracker.set_broadcast_range_percentage(pct);
            assert_eq!(tracker.broadcast_range_percentage(), pct);
            let kinds = kinds_for_test();

            let mut item = player_at(entities.next_id(), 0.0, 64.0, 0.0);
            item.kind = crate::entities::ITEM;
            entities.insert(item.clone());

            let rec = Recorder::default();
            let conn = Arc::new(Connection::with_entity(
                1,
                "watcher",
                Box::new(rec.clone()),
                None,
                entities.next_id(),
                // 32 chunks is 512 blocks, so the *player's* half of the
                // `min` never binds and the item's own range is what is
                // being scaled.
                32,
                (0, 0),
            ));
            let me = player_at(conn.entity_id(), 200.0, 64.0, 0.0);
            entities.insert(me.clone());
            open_view(&conn, me.chunk(), 32);
            let p = TrackedPlayer {
                conn: Arc::clone(&conn),
                entity_id: me.id,
                pos: (me.x, me.y, me.z),
                chunk: me.chunk(),
                view_distance: 32,
            };
            tracker.add(&item, &[p], &kinds);
            added(&rec)
        };
        assert_eq!(
            paired_at(crate::tracker::DEFAULT_BROADCAST_RANGE_PERCENTAGE),
            0,
            "96 blocks of item range does not reach 200"
        );
        assert_eq!(paired_at(1000), 1, "960 blocks does");
        // The boundary, so this is measuring the range and not the flag:
        // `ITEM` is 6 chunks, 96 blocks, and 200 needs at least 209%.
        assert_eq!(paired_at(208), 0, "scaled_range(96, 208) == 199");
        assert_eq!(
            paired_at(209),
            1,
            "scaled_range(96, 209) == 200, and <= wins"
        );
    }

    /// `updatePlayer`'s third limb is `ChunkMap.isChunkTracked`, which is
    /// `contains && !isPending` — **both halves**. An entity standing in a
    /// chunk the client has been promised and not yet sent is not paired.
    ///
    /// This limb used to be `broadcast::chunk_is_tracked`, the geometry
    /// alone, because the pending set lived on the session thread's stack. So
    /// the assertion that bites is the first one: a client whose whole view
    /// is still queued was previously told about every entity in it.
    ///
    /// Three states, so neither limb can be mistaken for the other:
    /// **queued** (contains, pending) pairs nothing; **delivered** (contains,
    /// not pending) pairs; **outside the view** pairs nothing however
    /// thoroughly it has been delivered.
    #[test]
    fn an_entity_in_a_chunk_the_client_has_not_been_sent_is_not_paired() {
        let entities = Entities::new();
        let tracker = Tracker::new();
        let kinds = kinds_for_test();

        let mut item = player_at(entities.next_id(), 0.0, 64.0, 0.0);
        item.kind = crate::entities::ITEM;
        entities.insert(item.clone());

        let rec = Recorder::default();
        let conn = Arc::new(Connection::with_entity(
            1,
            "loading",
            Box::new(rec.clone()),
            None,
            entities.next_id(),
            4,
            (0, 0),
        ));
        let me = player_at(conn.entity_id(), 8.0, 64.0, 8.0);
        entities.insert(me.clone());
        let p = || TrackedPlayer {
            conn: Arc::clone(&conn),
            entity_id: me.id,
            pos: (me.x, me.y, me.z),
            chunk: me.chunk(),
            view_distance: 4,
        };

        // Queued: the whole view is pending, nothing has been `mark_sent`.
        conn.with_chunk_view(|v| {
            v.update_tracking((0, 0));
            assert!(v.pending_len() > 0, "the join queued the disc");
            assert!(
                !v.is_chunk_tracked((0, 0)),
                "contains, but pending — so not tracked"
            );
        });
        tracker.add(&item, &[p()], &kinds);
        assert_eq!(added(&rec), 0, "an item in a queued chunk is withheld");

        // Delivered: same tracker, same distance, same everything but the
        // queue. See `broadcast`'s drain loop for why the acknowledgement is
        // not optional.
        conn.with_chunk_view(|v| {
            for _ in 0..64 {
                if v.pending_len() == 0 {
                    break;
                }
                let batch = v.take_batch((0, 0), |_| true);
                assert!(!batch.is_empty(), "the sender makes progress");
                v.on_batch_received(64.0);
            }
            assert_eq!(v.pending_len(), 0);
        });
        tracker.add(&item, &[p()], &kinds);
        assert_eq!(added(&rec), 1, "and paired once the chunk has landed");

        // Outside the view: `|dz| = 6` gives `(6 - 2)^2 = 16`, not `< 16`.
        // Delivered as thoroughly as the chunk above and still not tracked,
        // so the first assertion above is the pending limb and not the
        // `contains` one.
        let far = Recorder::default();
        let fc = Arc::new(Connection::with_entity(
            2,
            "far",
            Box::new(far.clone()),
            None,
            entities.next_id(),
            4,
            (0, 6),
        ));
        open_view(&fc, (0, 6), 4);
        assert!(!fc.is_chunk_tracked((0, 0)), "six chunks is outside");
        let mut t = TrackedEntity::new(&item);
        assert!(t
            .update_player(
                &item,
                &TrackedPlayer {
                    conn: Arc::clone(&fc),
                    entity_id: fc.entity_id(),
                    pos: (8.0, 64.0, 8.0),
                    chunk: (0, 0),
                    view_distance: 4,
                },
                &kinds,
                DEFAULT_BROADCAST_RANGE_PERCENTAGE
            )
            .is_none());
    }

    /// **A player walking towards a stationary entity must be told it is
    /// there**, and this is the half of the cadence the first version of this
    /// port left out.
    ///
    /// The main loop re-runs `updatePlayer` when the *entity* changes section.
    /// A dropped item never changes section, so nothing about it fires — and
    /// vanilla covers exactly this with a second sweep over every tracked
    /// entity for the players who moved. Without that sweep this test sees
    /// zero `add_entity` and the item is invisible for ever.
    ///
    /// It cannot be caught by any scenario in which the entity is another
    /// player, because then both entities move and the main loop happens to
    /// cover it. That is why it survived the two checks this build was first
    /// measured on.
    #[test]
    fn a_player_walking_up_to_a_stationary_item_is_told_about_it() {
        let entities = Entities::new();
        let tracker = Tracker::new();
        let kinds = kinds_for_test();

        // An item sitting at the origin, and a player 400 blocks away — well
        // outside the 6-chunk (96 block) item tracking range.
        let mut item = player_at(entities.next_id(), 0.0, 64.0, 0.0);
        item.kind = crate::entities::ITEM;
        entities.insert(item.clone());

        let rec = Recorder::default();
        let conn = Arc::new(Connection::with_entity(
            1,
            "walker",
            Box::new(rec.clone()),
            None,
            entities.next_id(),
            32,
            (0, 0),
        ));
        let mut me = player_at(conn.entity_id(), 400.0, 64.0, 0.0);
        entities.insert(me.clone());
        open_view(&conn, me.chunk(), 32);

        let far = |c: &Arc<Connection>, e: &Entity| TrackedPlayer {
            conn: Arc::clone(c),
            entity_id: e.id,
            pos: (e.x, e.y, e.z),
            chunk: e.chunk(),
            view_distance: 32,
        };

        tracker.add(&item, &[far(&conn, &me)], &kinds);
        tracker.add(&me, &[far(&conn, &me)], &kinds);
        tracker.tick(&entities, &[far(&conn, &me)], &kinds);
        assert_eq!(added(&rec), 0, "400 blocks away: nothing to see");

        // The player walks to the item. Only the *player* changed section;
        // the item did not move at all.
        me.x = 2.0;
        entities.insert(me.clone());
        tracker.tick(&entities, &[far(&conn, &me)], &kinds);
        assert_eq!(
            added(&rec),
            1,
            "the item must be announced when the player arrives beside it"
        );
    }

    /// **The wire order.** Three entities move in one tick and their packets
    /// leave in `ChunkMap.entityMap`'s walk, not in ascending id.
    ///
    /// Every other test of this port grades a container or a registry. This
    /// one grades the bytes: it reads the frames a real `Connection` received
    /// and asserts the order they arrived in. That is the only place the
    /// parity claim actually lives — roadmap §101.5's finding is about what
    /// leaves the socket within one tick, and a walk that is right in
    /// `Entities::all` and re-sorted anywhere between there and `send` would
    /// pass everything else in this branch.
    ///
    /// Ids 1..4 with the player at 1, so `ChunkMap.entityMap` holds {1,2,3,4}
    /// and vanilla walks it `2 4 1 3`. The player's own entity is skipped —
    /// `updatePlayer` returns early for it and `sendChanges` reaches nobody
    /// through its own `seenBy` — so the three items arrive as `2 4 3`, which
    /// is that walk with 1 struck out. Ascending would be `2 3 4`.
    #[test]
    fn three_entities_moving_in_one_tick_leave_in_vanillas_broadcast_order() {
        use rustmc_proto::ids::play::cb;
        let entities = Entities::new();
        let tracker = Tracker::new();
        let kinds = kinds_for_test();

        let rec = Recorder::default();
        let conn = Arc::new(Connection::with_entity(
            1,
            "watcher",
            Box::new(rec.clone()),
            None,
            entities.next_id(),
            32,
            (0, 0),
        ));
        let me = player_at(conn.entity_id(), 0.0, 64.0, 0.0);
        assert_eq!(me.id, 1, "the player draws id 1");
        entities.insert(me.clone());
        open_view(&conn, me.chunk(), 32);

        // Players rather than items only because `EntityType.ITEM`'s
        // `updateInterval` is 20 and a player's is 2, so the firing tick is
        // two calls away instead of twenty. The walk does not depend on the
        // kind: `ChunkMap.entityMap` is keyed on the id and nothing else.
        let mut items = Vec::new();
        for _ in 0..3 {
            let it = player_at(entities.next_id(), 1.0, 64.0, 1.0);
            entities.insert(it.clone());
            items.push(it);
        }
        assert_eq!(
            items.iter().map(|e| e.id).collect::<Vec<_>>(),
            vec![2, 3, 4]
        );

        let me_view = |e: &Entity| TrackedPlayer {
            conn: Arc::clone(&conn),
            entity_id: 1,
            pos: (e.x, e.y, e.z),
            chunk: e.chunk(),
            view_distance: 32,
        };
        let players = [me_view(&me)];
        tracker.add(&me, &players, &kinds);
        for it in &items {
            tracker.add(it, &players, &kinds);
        }
        // Settle the adds so the frames below are the tick's and not the
        // pairing bundles'.
        tracker.tick(&entities, &players, &kinds);

        // Every entity moves far enough to force a packet, and they are
        // updated in ASCENDING id order, so insertion order and update order
        // both point the wrong way if the walk is not doing the work.
        for it in &mut items {
            it.x += 0.5;
            entities.insert(it.clone());
        }
        // `sendChanges` runs its body on `tickCount % updateInterval == 0`.
        // The settle tick above left every counter at 1, so this one is
        // silent and the next one fires; the recorder is cleared in between
        // so what is read below is ONE tick's frames.
        tracker.tick(&entities, &players, &kinds);
        rec.0.lock().expect("recorder").clear();
        tracker.tick(&entities, &players, &kinds);

        let bytes = rec.0.lock().expect("recorder").clone();
        let mut cur = &bytes[..];
        let mut order = Vec::new();
        while let Ok((id, body)) = rustmc_proto::frame::read_frame(&mut cur, false) {
            if id == cb::MOVE_ENTITY_POS || id == cb::MOVE_ENTITY_POS_ROT {
                order.push(i32::from(body[0]));
            }
        }
        assert_eq!(
            order,
            vec![2, 4, 3],
            "the tick's per-entity packets leave in entityMap's walk over \
             {{1,2,3,4}} = 2 4 1 3, with the player's own id struck out; \
             ascending id would be 2 3 4"
        );
    }

    /// **The walk grades the LIVE set; vanilla's grades the TRACKED set.**
    ///
    /// `ChunkMap.entityMap` holds only entities that are being tracked, and
    /// `ChunkMap.tick` walks that. Ours walks `Entities::all`, which is every
    /// live entity, and skips the ones the tracker has no record of. Those are
    /// the same walk exactly while the two key sets are the same set — and
    /// they are not merely "close" if one diverges, because an open hash map's
    /// walk is a function of the WHOLE key set: an untracked entity in the
    /// live map moves the slots of the tracked ones and reorders packets that
    /// have nothing to do with it.
    ///
    /// One thing can make the sets differ. `Tracker::add` returns without
    /// recording anything when `clientTrackingRange() * 16 == 0`, which is
    /// vanilla's own test in `ChunkMap.addEntity`. No kind here has a zero
    /// range, so today the sets coincide and the walk is right.
    ///
    /// This test is the tripwire for the day one does. It is deliberately a
    /// test over the declared kinds rather than a comment, because the failure
    /// it guards is silent: the untracked entity would simply never appear,
    /// everything else would still be broadcast, and only the ORDER would be
    /// wrong — which nothing in the corpus can see. Whoever adds such a kind
    /// has to move the broadcast onto `Tracker.tracked`, which is the real
    /// `entityMap`.
    #[test]
    fn the_broadcast_walk_assumes_every_live_entity_is_tracked() {
        for k in crate::entities::KINDS {
            assert_ne!(
                k.tracking_range_chunks * 16,
                0,
                "{} has a zero tracking range, so it can be live and untracked \
                 — Entities::all's walk is then no longer entityMap's walk, \
                 and the packets of every OTHER entity come out in the wrong \
                 order. Move Tracker::tick's main loop onto self.tracked.",
                k.name
            );
        }
        // Not passing by having nothing to look at.
        assert!(crate::entities::KINDS.len() >= 2);
    }

    /// An entity id is never handed to the client for itself.
    #[test]
    fn a_player_is_never_paired_with_itself() {
        let e = player_at(9, 0.0, 64.0, 0.0);
        let mut t = TrackedEntity::new(&e);
        let conn = Arc::new(Connection::new(0, "A", Box::new(Vec::new()), None));
        let p = TrackedPlayer {
            conn,
            entity_id: 9,
            pos: (0.0, 64.0, 0.0),
            chunk: (0, 0),
            view_distance: 10,
        };
        assert!(t
            .update_player(
                &e,
                &p,
                &KindIds::default(),
                DEFAULT_BROADCAST_RANGE_PERCENTAGE
            )
            .is_none());
        assert!(t.seen_by.is_empty(), "and it does not enter seenBy");
    }
}
