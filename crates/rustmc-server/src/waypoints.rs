//! `ServerWaypointManager` — the locator bar, and the packet nobody decodes.
//!
//! # What this is for
//!
//! Every living entity with a positive `WAYPOINT_TRANSMIT_RANGE` is a
//! *transmitter*, every `ServerPlayer` is a *receiver*, and the manager keeps
//! one `Connection` per ordered pair. A connection sends
//! `ClientboundTrackedWaypointPacket` — clientbound id 138, `0x8A` — when it
//! is made, when the transmitter has moved, and when it is broken.
//!
//! `tools/oracle-runner` has no decoder for that packet, so its strict
//! comparison hashes the **whole body**. There is therefore no partial credit
//! here: every byte has to be vanilla's or the scenario stays `diverge`.
//! Both of `080-other-player-is-an-entity`'s bodies were reconstructed from
//! the stream codec and checked against the recorded digests before this
//! module was written — see [`body`].
//!
//! # The gamerule, and why it is not a switch here
//!
//! `ServerWaypointManager.createConnection` offsets 6-13 and
//! `updateConnection` offsets 6-13 both begin
//! `if (!isLocatorBarEnabledFor(receiver)) return;`, which reads
//! `GameRules.LOCATOR_BAR`. That rule is `registerBoolean("locator_bar",
//! PLAYER, true)` — default **true** — and `ci/t2-gameplay.sh` never sets it,
//! so both servers run at the default and this server implements the
//! transmitter rather than the rule. `docs/ORACLE.md` rule 27 is about
//! silencing a reference with an instrument; nothing here is silenced. **If a
//! gamerule store is ever added to this server, this is one of the call sites
//! that has to consult it**, and the check belongs at the two `connect`/
//! `update` entry points and nowhere else.
//!
//! # Three connection kinds, one implemented
//!
//! `LivingEntity.makeWaypointConnectionWith` picks between them:
//!
//! ```text
//!   isReallyFar(source, receiver)              -> EntityAzimuthConnection
//!   !isChunkVisible(source.chunkPos, receiver) -> EntityChunkConnection
//!   otherwise                                  -> EntityBlockConnection
//! ```
//!
//! `isReallyFar` is `distanceTo > 332`, and `isChunkVisible` asks the
//! receiver's chunk-tracking view — which this server does not model (see
//! [`crate::players::Players::broadcast`], which sends to everybody for the
//! same reason). **Only the block connection is implemented**, so two players
//! more than 332 blocks apart, or far enough apart that vanilla would fall
//! back to a chunk or an azimuth waypoint, get a position waypoint here and a
//! coarser one there. That is a real gap and it is written down rather than
//! papered over; the corpus's two clients are always in one workspace.
//!
//! # The threshold that decides TRACK against UPDATE
//!
//! `WaypointTransmitter$BlockConnection.isBroken()` is
//! `distanceManhattan() > 1`, measured between the transmitter's current
//! block position and the position the connection last sent. So:
//!
//! ```text
//!   moved 0 blocks   nothing
//!   moved 1 block    update()      -> UPDATE
//!   moved 2+ blocks  isBroken      -> a fresh connection -> connect() -> TRACK
//! ```
//!
//! and the remake does **not** disconnect the old one first
//! (`lambda$updateConnection$0` is `connect(); put()`, with no `disconnect`),
//! so a broken connection costs one packet and not two. That is why both of
//! 080's four-block hops produce a `TRACK` and not an `UPDATE`, and getting it
//! the other way round would be one wrong byte in every waypoint packet the
//! corpus contains.

use std::collections::BTreeMap;
use std::io;
use std::sync::{Mutex, OnceLock};

use crate::interact::BlockPos;

/// `ClientboundTrackedWaypointPacket$Operation`'s ordinals.
///
/// From the enum's own `<clinit>` — `TRACK` at `iconst_0`, `UNTRACK` at
/// `iconst_1`, `UPDATE` at `iconst_2` — and written as a varint by
/// `ByteBufCodecs.idMapper`.
pub const OP_TRACK: i32 = 0;
pub const OP_UNTRACK: i32 = 1;
pub const OP_UPDATE: i32 = 2;

/// `TrackedWaypoint$Type`'s ordinals, written by `FriendlyByteBuf.writeEnum`,
/// which is `writeVarInt(ordinal())`.
pub const TYPE_EMPTY: i32 = 0;
pub const TYPE_VEC3I: i32 = 1;

/// `WaypointStyleAssets.DEFAULT`, which is `createId("default")` and so
/// `Identifier.withDefaultNamespace("default")`.
///
/// `Waypoint$Icon`'s stream codec is `ResourceKey.streamCodec(ROOT_ID)`, which
/// is `Identifier.STREAM_CODEC` — `ByteBufCodecs.STRING_UTF8` over
/// `Identifier::toString`. `toString` does not elide the default namespace, so
/// the seventeen bytes `minecraft:default` go on the wire and not the seven of
/// `default`. Both spellings were tried against the recorded digest; only this
/// one hashes.
pub const DEFAULT_STYLE: &str = "minecraft:default";

/// One waypoint packet, addressed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    /// The connection id of the **receiver**. A waypoint is never a
    /// broadcast: each receiver has its own connection with its own
    /// `lastPosition`, so two receivers of the same move can legitimately get
    /// different operations.
    pub to: u64,
    pub body: Vec<u8>,
}

/// `ClientboundTrackedWaypointPacket`'s body.
///
/// Composed from `STREAM_CODEC` — `Operation.STREAM_CODEC` then
/// `TrackedWaypoint.STREAM_CODEC` — and `TrackedWaypoint.write`:
///
/// ```text
///   varint  operation ordinal
///   bool    Either.ifLeft -> writeBoolean(true)   (a UUID, not a name)
///   16      the transmitter's UUID, big-endian
///   string  the icon's style asset id
///   bool    the icon's optional colour, absent
///   varint  TrackedWaypoint.Type ordinal
///   varint  x, y, z                               (Vec3iWaypoint.writeContents)
/// ```
///
/// # Two traps, both of which were caught by the digest and not by reading
///
/// * **`VarInt.write`, not zigzag.** The coordinates are written by
///   `net.minecraft.network.VarInt.write`, which LEB128s the raw 32-bit
///   two's complement — so `-252` is five bytes `84 fe ff ff 0f`, not the
///   two a zigzag would give. Every waypoint in the corpus is at a negative
///   x, so a zigzag would have been wrong in every packet.
/// * **`Either`'s discriminator comes first and is a raw byte**, from
///   `FriendlyByteBuf.writeEither`'s `ifLeft` lambda
///   (`writeBoolean(true)`), before the UUID rather than after it.
///
/// # Errors
/// On a write failure, which for a `Vec` cannot happen.
pub fn body(op: i32, uuid: u128, style: &str, at: Option<BlockPos>) -> io::Result<Vec<u8>> {
    use rustmc_proto::varint::write_varint;
    use rustmc_proto::wire;
    let mut out = Vec::new();
    write_varint(&mut out, op)?;
    // `Either.left` — the identifier is a UUID. The name form is only used by
    // `/waypoint`'s command source, which this server has no equivalent of.
    out.push(1);
    wire::write_u128(&mut out, uuid)?;
    match at {
        Some(p) => {
            wire::write_string(&mut out, style)?;
            // `ByteBufCodecs.optional`'s absent branch: one `false` byte.
            out.push(0);
            write_varint(&mut out, TYPE_VEC3I)?;
            write_varint(&mut out, p.0)?;
            write_varint(&mut out, p.1)?;
            write_varint(&mut out, p.2)?;
        }
        // `TrackedWaypoint.empty(uuid)` — `ClientboundTrackedWaypointPacket
        // .removeWaypoint`'s payload. Still carries an icon, because
        // `TrackedWaypoint.write` writes one unconditionally, and
        // `EmptyWaypoint.writeContents` then writes nothing.
        None => {
            wire::write_string(&mut out, style)?;
            out.push(0);
            write_varint(&mut out, TYPE_EMPTY)?;
        }
    }
    Ok(out)
}

/// `BlockPos.distManhattan(Vec3i)`.
fn dist_manhattan(a: BlockPos, b: BlockPos) -> i64 {
    i64::from((a.0 - b.0).abs()) + i64::from((a.1 - b.1).abs()) + i64::from((a.2 - b.2).abs())
}

#[derive(Debug, Clone, Copy)]
struct Transmitter {
    uuid: u128,
    at: BlockPos,
}

#[derive(Debug, Default)]
struct State {
    /// Every connected player. A player is both a transmitter and a receiver
    /// here, which is vanilla's `waypoints` ∪ `players` for a server whose
    /// only living entities are players.
    players: BTreeMap<u64, Transmitter>,
    /// `Table<receiver, transmitter, Connection>`, reduced to the one field a
    /// `BlockConnection` carries: `lastPosition`.
    conns: BTreeMap<(u64, u64), BlockPos>,
}

/// The manager.
#[derive(Debug, Default)]
pub struct Waypoints {
    inner: Mutex<State>,
}

impl Waypoints {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The process-wide manager.
    ///
    /// Shared like [`crate::entities::Entities`] and for the same reason: a
    /// connection made on one player's thread has to be found again on
    /// another's. Tests build their own [`Waypoints`] rather than reaching
    /// for this one, so a test's join cannot leak into another test's move.
    pub fn shared() -> &'static Self {
        static ONCE: OnceLock<Waypoints> = OnceLock::new();
        ONCE.get_or_init(Waypoints::new)
    }

    /// `ServerWaypointManager.addPlayer` followed by `trackWaypoint`.
    ///
    /// The order is vanilla's and it is observable: the joining player is
    /// told about everyone else **first**, and only then is everyone else
    /// told about the joiner. Reversing it would put the joiner's own
    /// waypoint on the wire before the ones it is joining.
    ///
    /// # Panics
    /// If the lock is poisoned.
    pub fn add_player(&self, id: u64, uuid: u128, at: BlockPos) -> Vec<Outgoing> {
        let mut st = self.inner.lock().expect("waypoints lock");
        let mut out = Vec::new();
        // addPlayer: the new receiver connects to every existing transmitter.
        let existing: Vec<(u64, Transmitter)> = st.players.iter().map(|(k, v)| (*k, *v)).collect();
        for (tx, t) in &existing {
            out.extend(create(&mut st, id, *tx, t.uuid, t.at));
        }
        st.players.insert(id, Transmitter { uuid, at });
        // trackWaypoint: every receiver — including the joiner, which
        // `createConnection`'s first two offsets then refuse — connects to
        // the new transmitter.
        for (rx, _) in &existing {
            out.extend(create(&mut st, *rx, id, uuid, at));
        }
        out.extend(create(&mut st, id, id, uuid, at));
        out
    }

    /// `Entity.setPosRaw` offsets 214-296: `updateWaypoint(self)` then
    /// `updatePlayer(self)`, in that order.
    ///
    /// The second half is not redundant. A receiver that moves can break its
    /// *own* connections to transmitters that did not move — in vanilla by
    /// leaving their chunk, here never, because the only breakage this server
    /// models is the transmitter's own displacement. It is implemented anyway
    /// so the shape is vanilla's and a later chunk-visibility model has
    /// somewhere to go.
    ///
    /// # Panics
    /// If the lock is poisoned.
    pub fn moved(&self, id: u64, at: BlockPos) -> Vec<Outgoing> {
        let mut st = self.inner.lock().expect("waypoints lock");
        let Some(me) = st.players.get_mut(&id) else {
            return Vec::new();
        };
        me.at = at;
        let uuid = me.uuid;
        let mut out = Vec::new();
        let receivers: Vec<u64> = st.players.keys().copied().collect();
        for rx in &receivers {
            out.extend(update_or_create(&mut st, *rx, id, uuid, at));
        }
        let transmitters: Vec<(u64, Transmitter)> =
            st.players.iter().map(|(k, v)| (*k, *v)).collect();
        for (tx, t) in &transmitters {
            out.extend(update_or_create(&mut st, id, *tx, t.uuid, t.at));
        }
        out
    }

    /// `ServerWaypointManager.removePlayer` then `untrackWaypoint`.
    ///
    /// Every connection the leaver is either end of is disconnected, and a
    /// disconnect is an `UNTRACK` naming the transmitter's UUID. The ones
    /// addressed to the leaver are produced rather than skipped: whether the
    /// socket is still there is the sender's problem, and dropping them here
    /// would make this function's output depend on something it cannot see.
    ///
    /// # Panics
    /// If the lock is poisoned.
    pub fn remove_player(&self, id: u64) -> Vec<Outgoing> {
        let mut st = self.inner.lock().expect("waypoints lock");
        let mut out = Vec::new();
        let pairs: Vec<(u64, u64)> = st
            .conns
            .keys()
            .copied()
            .filter(|(rx, tx)| *rx == id || *tx == id)
            .collect();
        for (rx, tx) in pairs {
            st.conns.remove(&(rx, tx));
            if let Some(t) = st.players.get(&tx) {
                if let Ok(b) = body(OP_UNTRACK, t.uuid, DEFAULT_STYLE, None) {
                    out.push(Outgoing { to: rx, body: b });
                }
            }
        }
        st.players.remove(&id);
        out
    }
}

/// `createConnection(receiver, transmitter)`.
///
/// Offsets 0-5 are `if (receiver == transmitter) return` — **a player never
/// receives its own waypoint**, which is why 080's actor sees nothing about
/// itself while it walks.
fn create(st: &mut State, rx: u64, tx: u64, uuid: u128, at: BlockPos) -> Option<Outgoing> {
    if rx == tx {
        return None;
    }
    st.conns.insert((rx, tx), at);
    body(OP_TRACK, uuid, DEFAULT_STYLE, Some(at))
        .ok()
        .map(|b| Outgoing { to: rx, body: b })
}

/// `updateConnection(receiver, transmitter, connection)`, or
/// `createConnection` when there is no connection yet.
fn update_or_create(
    st: &mut State,
    rx: u64,
    tx: u64,
    uuid: u128,
    at: BlockPos,
) -> Option<Outgoing> {
    if rx == tx {
        return None;
    }
    let Some(last) = st.conns.get(&(rx, tx)).copied() else {
        return create(st, rx, tx, uuid, at);
    };
    let moved = dist_manhattan(at, last);
    if moved > 1 {
        // `isBroken()` — a fresh connection, whose constructor takes
        // `lastPosition = source.blockPosition()`, and then `connect()`.
        st.conns.insert((rx, tx), at);
        return body(OP_TRACK, uuid, DEFAULT_STYLE, Some(at))
            .ok()
            .map(|b| Outgoing { to: rx, body: b });
    }
    if moved > 0 {
        // `EntityBlockConnection.update()`: `distManhattan > 0`.
        st.conns.insert((rx, tx), at);
        return body(OP_UPDATE, uuid, DEFAULT_STYLE, Some(at))
            .ok()
            .map(|b| Outgoing { to: rx, body: b });
    }
    None
}

/// Put a batch of waypoint packets on their receivers' sockets.
///
/// Addressed rather than broadcast, and a receiver that has gone is silently
/// skipped for the reason [`crate::players::Players::broadcast`] discards
/// write errors: a departed connection is not the mover's problem.
pub fn send(players: &crate::players::Players, out: &[Outgoing]) {
    if out.is_empty() {
        return;
    }
    let conns = players.connections();
    for o in out {
        if let Some(c) = conns.iter().find(|c| c.id() == o.to) {
            let _ = c.send(rustmc_proto::ids::play::cb::WAYPOINT, &o.body);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The check that this module is right at all.**
    ///
    /// `080-other-player-is-an-entity`'s two `packet:0x8A` observations in the
    /// vanilla recording of 2026-08-08 21:26 carry `rest` digests
    /// `48d2aef46e816d05` and `a94195be44c89f19`. The scenario's anchor is
    /// `[-256, -60, 0]` and the actor walks to `+4` and back, so the two block
    /// positions are `(-252, -60, 0)` and `(-256, -60, 0)`; the actor logs in
    /// as `oracle`, whose offline UUID is `501b21fe-0ed2-3c3c-9399-e5f69a092dcb`.
    ///
    /// This reproduces the harness's digest here rather than in the harness,
    /// because the failure it is guarding against is a **byte** difference and
    /// no other test in this server can see one: the strict comparison hashes
    /// the whole body, so a wrong operation, a zigzagged coordinate or a short
    /// style string all read as "the packet differs" and none of them read as
    /// anything more specific.
    fn digest(bytes: &[u8]) -> String {
        // `tools/oracle-runner`'s constant, copied from the line that defines
        // it. It is NOT FNV's prime — the real one is a hex digit shorter, and
        // using it gives an answer that agrees in the bottom five bytes and
        // differs in the top three.
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in bytes {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x1000_0000_01b3);
        }
        format!("{h:016x}")
    }

    const ORACLE: u128 = 0x501b_21fe_0ed2_3c3c_9399_e5f6_9a09_2dcb;

    #[test]
    fn the_two_waypoints_080_records_are_reproduced_byte_for_byte() {
        for (at, want) in [
            ((-252, -60, 0), "48d2aef46e816d05"),
            ((-256, -60, 0), "a94195be44c89f19"),
        ] {
            let b = body(OP_TRACK, ORACLE, DEFAULT_STYLE, Some(at)).expect("buf");
            assert_eq!(digest(&b), want, "at {at:?}");
        }
    }

    /// The three ways to be one byte wrong, each shown to be wrong. Without
    /// this the test above passes for a body that is right by luck.
    #[test]
    fn the_three_near_misses_do_not_hash() {
        let want = "48d2aef46e816d05";
        let at = (-252, -60, 0);
        assert_ne!(
            digest(&body(OP_UPDATE, ORACLE, DEFAULT_STYLE, Some(at)).expect("buf")),
            want,
            "UPDATE is not TRACK — a four-block hop breaks the connection"
        );
        assert_ne!(
            digest(&body(OP_TRACK, ORACLE, "default", Some(at)).expect("buf")),
            want,
            "the style keeps its namespace on the wire"
        );
        assert_ne!(
            digest(&body(OP_TRACK, ORACLE + 1, DEFAULT_STYLE, Some(at)).expect("buf")),
            want,
            "the UUID is the transmitter's"
        );
    }

    /// A negative coordinate is five bytes of raw two's complement, not the
    /// two a zigzag would produce. Pinned separately from the digest so the
    /// reason a digest fails is visible when it does.
    #[test]
    fn coordinates_are_varints_and_not_zigzagged() {
        let b = body(OP_TRACK, 0, "", Some((-252, 0, 0))).expect("buf");
        assert!(
            b.ends_with(&[0x84, 0xfe, 0xff, 0xff, 0x0f, 0x00, 0x00]),
            "{b:02x?}"
        );
    }

    /// `createConnection`'s first two offsets. A player must never be told
    /// about itself, and the joining player is a receiver *and* a transmitter
    /// in the same call, which is where a naive loop would send it one.
    #[test]
    fn a_player_never_receives_its_own_waypoint() {
        let w = Waypoints::new();
        let out = w.add_player(1, 0x11, (0, 0, 0));
        assert!(out.is_empty(), "the first player has nobody to tell");
        let out = w.add_player(2, 0x22, (10, 0, 0));
        assert_eq!(out.len(), 2, "{out:?}");
        // One each way, and neither is addressed to the player it describes.
        assert_eq!(out[0].to, 2, "the joiner hears about the resident first");
        assert_eq!(out[1].to, 1, "then the resident hears about the joiner");
        assert!(w.moved(1, (0, 0, 0)).is_empty(), "a move of zero is silent");
    }

    /// The threshold, at each of the three distances that matter. This is the
    /// difference between 080's recorded `TRACK` and an `UPDATE`, and it is
    /// one comparison in `BlockConnection.isBroken`.
    #[test]
    fn one_block_updates_and_two_blocks_retracks() {
        let w = Waypoints::new();
        w.add_player(1, 0x11, (0, 0, 0));
        w.add_player(2, 0x22, (0, 0, 0));
        let one = w.moved(2, (1, 0, 0));
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].body[0], 2, "UPDATE");
        let two = w.moved(2, (3, 0, 0));
        assert_eq!(two.len(), 1);
        assert_eq!(two[0].body[0], 0, "TRACK — the connection broke");
        assert!(w.moved(2, (3, 0, 0)).is_empty(), "standing still is silent");
    }

    /// 080 end to end: two players in one workspace, four blocks out and four
    /// back, and the observer gets exactly two packets while the actor gets
    /// none.
    #[test]
    fn eighty_produces_two_packets_and_both_go_to_the_observer() {
        let w = Waypoints::new();
        w.add_player(1, ORACLE, (-256, -60, 0));
        w.add_player(2, 0x22, (-256, -60, -5));
        let a = w.moved(1, (-252, -60, 0));
        let b = w.moved(1, (-256, -60, 0));
        assert_eq!(a.len(), 1);
        assert_eq!(b.len(), 1);
        assert_eq!((a[0].to, b[0].to), (2, 2), "both to the observer");
        assert_eq!(digest(&a[0].body), "48d2aef46e816d05");
        assert_eq!(digest(&b[0].body), "a94195be44c89f19");
    }

    /// A leaver takes its waypoint off every other client. Not a broadcast:
    /// the packet naming the leaver goes to the others, and the ones naming
    /// the others go to the leaver.
    #[test]
    fn leaving_untracks_in_both_directions() {
        let w = Waypoints::new();
        w.add_player(1, 0x11, (0, 0, 0));
        w.add_player(2, 0x22, (0, 0, 0));
        let out = w.remove_player(2);
        assert_eq!(out.len(), 2, "{out:?}");
        assert!(out.iter().all(|o| o.body[0] == 1), "every one an UNTRACK");
        assert_eq!(
            out.iter().filter(|o| o.to == 1).count(),
            1,
            "the survivor is told the leaver has gone"
        );
    }
}
