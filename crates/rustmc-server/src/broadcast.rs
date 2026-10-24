//! The tick boundary: `ChunkHolder.changedBlocksPerSection` and the flush
//! that drains it.
//!
//! # What this fixes, and how it was found
//!
//! Until this existed, every block change was broadcast the instant the
//! world was written, from the connection thread that caused it. Vanilla
//! does not. `Level.setBlock` reaches `ChunkHolder.blockChanged`, which only
//! **records** the position in a per-section set; the packets go out later,
//! from `ServerChunkCache.tick`, once per server tick.
//!
//! Two things follow, and the gameplay oracle measured both against real
//! vanilla on 2026-08-03 with 13 divergences and 8 `inert` verdicts between
//! them:
//!
//! * **Order.** The actor-facing sequence for one placement is
//!   `block_update` (resync of the clicked block), `block_update` (resync of
//!   the block placed into), `block_update` (**this** broadcast), then
//!   `block_changed_ack`. The resyncs are sent from inside
//!   `handleUseItemOn`; the broadcast is a tick-boundary event; and the ack
//!   is later still, because `ackBlockChangesUpTo` only records a number and
//!   `ServerGamePacketListenerImpl.tick` sends it. Broadcasting immediately
//!   put ours first instead of third, and every placement check diverged at
//!   index 0.
//! * **Coalescing.** More than one change in one section in one tick is
//!   **one** `section_blocks_update`, not several `block_update`s. Vanilla
//!   sent 18 of them across the corpus and this server sent none, which is
//!   why every scenario that moves two blocks at once — a door, a bed, a
//!   pair of chests, a fence connecting, a torch falling off its support —
//!   read as having produced nothing to compare.
//!
//! # The order things come out in
//!
//! Sections ascending, and within a section fastutil's own iteration order —
//! see [`crate::shortset`], where the reason a hash set's internal order is a
//! wire format is written down.
//!
//! **Across chunks the order is ours and not vanilla's, and that is a known
//! deferral rather than a claim.** `ServerChunkCache.tick` walks its chunk
//! holders in `ChunkMap`'s own iteration order; this walks them sorted by
//! `(x, z)`. Nothing in the corpus can tell the two apart, because every
//! scenario changes blocks in one chunk. The first check that changes blocks
//! in two chunks in one tick will, and it should be believed over this
//! comment.
//!
//! # The lock
//!
//! Vanilla's game loop is single-threaded, so a packet handler can never run
//! between a tick's broadcasts and its acknowledgements. This server runs a
//! thread per connection, so it needs [`tick_lock`] to say the same thing:
//! held for the whole of a block-action handler, and held across the flush.
//! It is a global serialisation point and a coarse one. Making it finer is a
//! later optimisation and belongs to the rule every optimisation here obeys —
//! prove it output-identical first.
//!
//! **What the lock does not buy, and it is a real difference:** vanilla does
//! not handle a packet when it arrives. `PacketUtils` defers every handler to
//! the server thread, so packets are drained at a defined point in the tick
//! and two that arrive within the same 50 ms are handled in the *same* tick —
//! and their changes therefore coalesce into one `section_blocks_update`.
//! This server handles each packet on arrival, so two that straddle a clock
//! boundary produce two packets where vanilla produces one. Nothing in the
//! corpus separates its steps by less than 60 ms, so no check can currently
//! see it; a check that fired two actions inside one tick would.

use crate::edits::Blocks;
use crate::interact::BlockPos;
use crate::players::Players;
use crate::shortset::{section_relative_pos, ShortOpenHashSet};
use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, OnceLock};

/// A section's identity: the chunk section coordinates, `y` last so the
/// `BTreeMap` orders by column and then by section index ascending — which is
/// the order `broadcastChanges` walks `changedBlocksPerSection`.
type SectionKey = (i32, i32, i32);

/// One `getPlayers(pos, false)` answer: the players a chunk column's changes
/// go to. `ChunkHolder.broadcastChanges` builds it once and hands the same
/// list to every `broadcast` call it makes.
type Recipients = Vec<std::sync::Arc<crate::players::Connection>>;

/// `ChunkTrackingView.isWithinDistance` — whether a chunk is inside a
/// player's tracking view, and so whether that player is told about changes
/// in it.
///
/// # This is not the test the name suggests, and the plausible guess is wrong
///
/// `ChunkMap.isChunkTracked(player, x, z)` is
///
/// ```java
/// player.getChunkTrackingView().contains(x, z)
///     && !player.connection.chunkSender.isPending(ChunkPos.pack(x, z))
/// ```
///
/// and `ChunkHolder.broadcastChanges` sends to `getPlayers(pos, false)`,
/// which is filtered on exactly that and on nothing else — no exclusion of
/// the acting player, no distance-to-the-block test. Since this server sends
/// chunks synchronously, `isPending` is always false here and `contains` is
/// the whole predicate.
///
/// `contains(x, z)` delegates to `contains(x, z, true)` — note the `true`,
/// which is a default supplied by the interface and not visible at any call
/// site — and `Positioned` forwards that to
/// `ChunkTrackingView.isWithinDistance(centerX, centerZ, viewDistance, x, z, includeEdge)`,
/// which reads, from the bytecode:
///
/// ```java
/// int pad = includeEdge ? 2 : 1;
/// long dx = Math.max(0, Math.abs(x - cx) - pad);
/// long dz = Math.max(0, Math.abs(z - cz) - pad);
/// return dx * dx + dz * dz < (long) (viewDistance * viewDistance);
/// ```
///
/// So it is **Euclidean, padded by two, and strictly less than** — three
/// things a reasonable implementation gets wrong, because the name says
/// "within distance".
///
/// # This paragraph used to say a square was correct for sending. It is not
///
/// Until 2026-09-05 the sentence here read that `world_send`'s Chebyshev
/// `in_range` was "correctly so" because sending chunks "is a different
/// question". It is the same question: `ChunkMap.applyChunkTrackingView`
/// decides which chunks to send and which to forget through
/// `ChunkTrackingView.difference` over two `Positioned` views, and
/// `Positioned.contains` is this very function with `includeEdge = true`.
/// A square of radius 10 sends 441 chunks where vanilla tracks 473, and the
/// two sets are not nested in either direction. [`crate::trackingview`] is
/// the port; this function stays because the entity tracker calls it
/// directly.
///
/// The two disagree in both directions, which is why neither is a
/// conservative approximation of the other:
///
/// * at the corners, Chebyshev includes chunks this does not — at
///   `viewDistance` 10 the chunk at `(+10, +10)` has `dx = dz = 8`, and
///   `64 + 64 = 128` is not `< 100`;
/// * along the axes it goes *further* than Chebyshev — `(+11, 0)` gives
///   `dx = 9`, `81 < 100`, so a chunk one beyond the nominal view distance
///   is still tracked.
///
/// The pad is what produces the second, and it is the part a guess omits.
#[must_use]
pub fn chunk_is_tracked(center: (i32, i32), view_distance: i32, chunk: (i32, i32)) -> bool {
    // `includeEdge` is true on every path that reaches here, so the pad is
    // two. Kept as a named binding rather than folded into the arithmetic
    // because the one-versus-two choice is the whole content of the flag.
    const PAD: i32 = 2;
    let dx = i64::from(((chunk.0 - center.0).abs() - PAD).max(0));
    let dz = i64::from(((chunk.1 - center.1).abs() - PAD).max(0));
    // Vanilla squares the view distance as an `int` and widens the result,
    // so the comparison is `long < long`. Reproduced rather than simplified:
    // the two differ once `viewDistance` exceeds 46,340, and while no server
    // runs one that large, a silent difference in an overflow rule is
    // exactly the kind of thing that is impossible to find later.
    let limit = i64::from(view_distance.wrapping_mul(view_distance));
    dx * dx + dz * dz < limit
}

/// Java's `Math.floorDiv(v, 16)`, which is **not** `v / 16` for negatives.
const fn section_of(v: i32) -> i32 {
    v >> 4
}

/// The tick's worth of changed blocks, per section.
#[derive(Default)]
pub struct ChangedBlocks {
    per_section: Mutex<BTreeMap<SectionKey, ShortOpenHashSet>>,
}

/// The lock that stands in for vanilla's single game thread.
///
/// # Panics
/// If a previous holder panicked while holding it.
pub fn tick_lock() -> MutexGuard<'static, ()> {
    tick_mutex()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn tick_mutex() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// Whether some thread is holding the tick lock right now.
///
/// **Call this from a thread that is not holding it**, or the answer is
/// `std::sync::Mutex`'s unspecified same-thread `try_lock` behaviour rather
/// than an observation.
///
/// It exists because the plugin contract
/// (`grit-plugins/docs/CONTRACT.md` §3) promises exactly one plugin callback
/// at any instant server-wide, and this server keeps that promise entirely by
/// holding this lock across every packet handler. That is a guarantee with no
/// type to enforce it and, until this, no test that could see it: deleting the
/// `tick_lock()` line from `handle_player_action` would have broken a
/// documented promise to plugin authors and left every test green.
///
/// A poisoned lock answers `false` — a holder panicked, so nobody is holding
/// it now, and reporting "held" would be reporting the wrong fact.
#[must_use]
pub fn tick_lock_is_held() -> bool {
    is_held(tick_mutex())
}

/// Whether `m` is locked, as seen from a thread that does not hold it.
///
/// Split out from [`tick_lock_is_held`] **so that it can be tested**, and that
/// is the whole reason it exists. The falsification sweep
/// (`ci/plugin-hook-falsify.sh` M17) replaced this body with `true` and every
/// test stayed green: the end-to-end check only ever asks the probe to report
/// a lock that *is* held, which a probe hardwired to `true` satisfies
/// perfectly. An instrument that cannot report the negative is not an
/// instrument — this project has paid for that once already, with
/// `RUSTMC_STAGE_TIMES` forcing the stage it was measuring to run
/// sequentially.
///
/// Taking the mutex as an argument is what makes the negative testable: the
/// global tick lock is taken and released by every other test in this process
/// on parallel threads, so asserting it is *free* at any given instant is a
/// coin flip. A local mutex is not.
fn is_held(m: &Mutex<()>) -> bool {
    matches!(m.try_lock(), Err(std::sync::TryLockError::WouldBlock))
}

impl ChangedBlocks {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The process-wide buffer, so a connection thread and the clock thread
    /// are talking about the same tick.
    #[must_use]
    pub fn shared() -> &'static Self {
        static SHARED: OnceLock<ChangedBlocks> = OnceLock::new();
        SHARED.get_or_init(ChangedBlocks::new)
    }

    /// `ChunkHolder.blockChanged`: record the position, send nothing.
    ///
    /// # Panics
    /// If the lock is poisoned.
    pub fn record(&self, pos: BlockPos) {
        let (x, y, z) = pos;
        let key = (section_of(x), section_of(z), section_of(y));
        self.per_section
            .lock()
            .expect("changed blocks")
            .entry(key)
            .or_default()
            .insert(section_relative_pos(x, y, z));
    }

    /// Everything recorded, taken out in one go.
    ///
    /// Vanilla clears `changedBlocksPerSection[j]` **before** it checks
    /// whether anyone is listening, so a change with no audience is still
    /// consumed rather than held over to the next tick.
    ///
    /// # Panics
    /// If the lock is poisoned.
    fn take(&self) -> BTreeMap<SectionKey, ShortOpenHashSet> {
        std::mem::take(&mut *self.per_section.lock().expect("changed blocks"))
    }

    /// `ChunkHolder.broadcastChanges`: one `block_update` for a lone change
    /// in a section, one `section_blocks_update` for more than one.
    ///
    /// The state is read from the world **now**, not remembered from when
    /// the change was recorded — a position written twice in one tick is
    /// broadcast once, carrying what it ended up as.
    ///
    /// # The recipient list, and why it is resolved once per chunk column
    ///
    /// `broadcastChanges` calls `getPlayers(pos, false)` **once**, at offsets
    /// 111-125, and then walks `changedBlocksPerSection` with that one list
    /// in hand. The list is per `ChunkHolder`, so per chunk column — every
    /// section of one column goes to the same set of players, and asking
    /// again per section would be asking a question whose answer vanilla
    /// never re-reads. Grouping is free here because the key is
    /// `(x, z, y)`, so a `BTreeMap` already delivers one column's sections
    /// contiguously and in ascending section order.
    ///
    /// Vanilla nulls `changedBlocksPerSection[j]` **before** it tests
    /// `list.isEmpty()` (offsets 156-170), so a change with no audience is
    /// consumed rather than held over. [`Self::take`] does that for the whole
    /// map at once, which is the same thing: nothing is carried into the next
    /// tick either way.
    pub fn flush(&self, players: &Players, blocks: &dyn Blocks) {
        let mut column: Option<((i32, i32), Recipients)> = None;
        for ((sx, sz, sy), set) in self.take() {
            // `getPlayers(pos, false)`, once per `ChunkHolder`.
            let recipients = match &column {
                Some((c, r)) if *c == (sx, sz) => r,
                _ => {
                    column = Some(((sx, sz), players.players_for_chunk((sx, sz), false)));
                    &column.as_ref().expect("just set").1
                }
            };
            // `if (list.isEmpty()) continue;` — the section is already
            // cleared, so this is a skip and not a deferral.
            if recipients.is_empty() {
                continue;
            }
            let order = set.iter_order();
            if order.len() == 1 {
                let s = order[0];
                let pos = from_section_relative(sx, sy, sz, s);
                let state = blocks.state_at(pos);
                if let Some(id) = blocks.state_id(&state) {
                    if let Ok(body) = crate::interact::block_update_body(pos, id) {
                        broadcast(recipients, rustmc_proto::ids::play::cb::BLOCK_UPDATE, &body);
                    }
                }
                // `broadcastBlockEntityIfNeeded` at offset 261, **after** the
                // block update and inside the same `if` — so the two packets
                // are always adjacent and always in this order. It takes the
                // same recipient list: it is the same `broadcast(List, Packet)`.
                broadcast_block_entity_if_needed(recipients, pos, &state);
            } else if order.len() > 1 {
                let mut entries = Vec::with_capacity(order.len());
                for s in order {
                    let pos = from_section_relative(sx, sy, sz, s);
                    let state = blocks.state_at(pos);
                    if let Some(id) = blocks.state_id(&state) {
                        entries.push((s, id));
                    }
                }
                if let Ok(body) = section_blocks_update_body(sx, sy, sz, &entries) {
                    broadcast(
                        recipients,
                        rustmc_proto::ids::play::cb::SECTION_BLOCKS_UPDATE,
                        &body,
                    );
                }
                // `packet.runUpdates(this::broadcastBlockEntityIfNeeded)` at
                // offset 307, after the section packet, walking the packet's
                // own positions in the packet's own order. Iterating the
                // entries here is that walk: a second pass over the set would
                // be the same positions in the same order, but only because
                // `iter_order` is stable, and relying on that would make the
                // ordering an accident rather than a statement.
                for (s, _) in &entries {
                    let pos = from_section_relative(sx, sy, sz, *s);
                    let state = blocks.state_at(pos);
                    broadcast_block_entity_if_needed(recipients, pos, &state);
                }
            }
        }
    }
}

/// `ChunkHolder.broadcastBlockEntityIfNeeded`, offsets 0-15, then
/// `broadcastBlockEntity`, offsets 0-31.
///
/// # Two guards, and both of them are load-bearing
///
/// * **`state.hasBlockEntity()`** — asked of the *state*, not of the store.
///   A position that has no block entity is never even looked up;
/// * **`getUpdatePacket()` may be null**, and for almost every type it is.
///   `BlockEntity.getUpdatePacket()` returns null and neither
///   `ChestBlockEntity` nor `AbstractFurnaceBlockEntity` overrides it in the
///   pinned jar. So the only thing this sends today is a sign, and a version
///   that broadcast every block entity would put a `block_entity_data` on the
///   wire every time a chest was placed — which vanilla does not, and which
///   four checks currently at `match` would see.
///
/// Silent when the protocol-id table is missing, like every other packet that
/// needs a registry id. `crate::protocol_ids::shared` has already printed by
/// then.
fn broadcast_block_entity_if_needed(recipients: &Recipients, pos: BlockPos, state: &str) {
    if crate::blockentity::for_state(state).is_none() {
        return;
    }
    let Some(be) = crate::blockentity::BlockEntities::shared().get(pos) else {
        return;
    };
    let Some(tag) = be.update_tag() else { return };
    let Some(ids) = crate::protocol_ids::shared() else {
        return;
    };
    let Some(type_id) = ids.block_entity_type(be.kind().id()) else {
        return;
    };
    // The corpus positive control: the block entity exists and nobody is told
    // about it, which is the server the whole 76x family was written against.
    // A check that stays green here is reading the block state and not the
    // block entity, whatever its name says. See `crate::sabotage`.
    if crate::sabotage::fires_at(crate::sabotage::Kind::BlockEntity, pos.0) {
        return;
    }
    if let Ok(body) = crate::blockentity::block_entity_data_body(pos, type_id, &tag) {
        broadcast(
            recipients,
            rustmc_proto::ids::play::cb::BLOCK_ENTITY_DATA,
            &body,
        );
    }
}

/// `ChunkHolder.broadcast(List<ServerPlayer>, Packet)`, offsets 0-12:
/// `list.forEach(p -> p.connection.send(packet))`. No filter, no actor check,
/// nothing — the filtering already happened when the list was built.
///
/// Write errors are dropped for the reason [`Players::broadcast`] drops
/// them: a dead socket is its own thread's problem.
fn broadcast(recipients: &Recipients, packet: i32, body: &[u8]) {
    for c in recipients {
        let _ = c.send(packet, body);
    }
}

/// The inverse of [`section_relative_pos`], back to world coordinates.
const fn from_section_relative(sx: i32, sy: i32, sz: i32, s: i16) -> BlockPos {
    let s = s as i32;
    (
        (sx << 4) | ((s >> 8) & 15),
        (sy << 4) | (s & 15),
        (sz << 4) | ((s >> 4) & 15),
    )
}

/// `ClientboundSectionBlocksUpdatePacket`'s body.
///
/// ```text
///   long    the section position, packed as SectionPos.asLong
///   varint  how many entries
///   varlong ((state id) << 12) | section-relative position   * n
/// ```
///
/// `SectionPos.asLong`, read from the bytecode rather than assumed from
/// `BlockPos`:
///
/// ```java
/// (x & 4194303L) << 42 | (y & 1048575L) | (z & 4194303L) << 20
/// ```
///
/// **`x` and `z` are 22 bits and `y` is 20** — the widths are not the same
/// three as `BlockPos`'s in the same order, and writing `z` with a 20-bit
/// mask by analogy produces a correct packet for every non-negative section
/// and a wrong one for every negative `z`, which is half the world. Caught
/// here before it shipped by checking the constants instead of the shape.
///
/// # Errors
/// Only if the buffer write fails, which it cannot for a `Vec`.
pub fn section_blocks_update_body(
    sx: i32,
    sy: i32,
    sz: i32,
    entries: &[(i16, i32)],
) -> std::io::Result<Vec<u8>> {
    let mut b = Vec::with_capacity(12 + entries.len() * 4);
    let packed = (i64::from(sx) & 0x3F_FFFF) << 42
        | (i64::from(sz) & 0x3F_FFFF) << 20
        | (i64::from(sy) & 0xF_FFFF);
    b.extend_from_slice(&packed.to_be_bytes());
    rustmc_proto::varint::write_varint(&mut b, i32::try_from(entries.len()).unwrap_or(i32::MAX))?;
    for &(pos, state) in entries {
        let v = (i64::from(state) << 12) | i64::from(pos);
        rustmc_proto::varint::write_varlong(&mut b, v)?;
    }
    Ok(b)
}

#[cfg(test)]
mod tests {
    use super::{
        chunk_is_tracked, from_section_relative, is_held, section_blocks_update_body, section_of,
        ChangedBlocks,
    };
    use crate::shortset::section_relative_pos;

    /// **The lock probe has to be able to say no.**
    ///
    /// `is_held` is the instrument behind the plugin contract's threading test
    /// (`world_send::plugin_hook_tests::a_handler_runs_with_the_tick_lock_held`),
    /// and the falsification sweep found that replacing its body with `true`
    /// left every other test green — that test only ever asks the probe to
    /// report a lock that is held. A local mutex is used rather than the tick
    /// lock because the tick lock is taken and released by every other test in
    /// this process on parallel threads, so asserting it is free at any given
    /// instant is a coin flip rather than a check.
    /// A world of one block state, which is all `flush` reads: it asks
    /// `state_at` for what a recorded position ended up as and `state_id` for
    /// its network id, and nothing else on the trait is reachable from here.
    struct OneBlock;
    impl crate::edits::Blocks for OneBlock {
        fn state_at(&self, _: crate::interact::BlockPos) -> String {
            "minecraft:stone".into()
        }
        fn state_id(&self, _: &str) -> Option<i32> {
            Some(1)
        }
        fn set_block(&self, _: crate::interact::BlockPos, _: &str) -> Option<i32> {
            unreachable!("flush never writes")
        }
        fn default_state(&self, _: &str) -> Option<String> {
            unreachable!("flush never places")
        }
        fn has_state(&self, _: &str) -> bool {
            unreachable!("flush never validates a computed state")
        }
        fn replaceable(
            &self,
            _: crate::interact::BlockPos,
            _: &crate::placement::Replacing<'_>,
        ) -> bool {
            unreachable!("flush never places")
        }
        fn face_sturdy(&self, _: crate::interact::BlockPos, _: crate::interact::Face) -> bool {
            unreachable!("flush asks no shape question")
        }
        fn redstone_conductor(&self, _: crate::interact::BlockPos) -> bool {
            unreachable!("flush runs no redstone")
        }
        fn signal_source(&self, _: crate::interact::BlockPos) -> bool {
            unreachable!("flush runs no redstone")
        }
        fn water_at(&self, _: crate::interact::BlockPos) -> bool {
            unreachable!("flush asks no fluid question")
        }
        fn legacy_fluid_at(&self, _: crate::interact::BlockPos) -> String {
            unreachable!("flush asks no fluid question")
        }
    }

    /// A writer whose bytes survive the connection that owned it.
    #[derive(Clone, Default)]
    struct Tap(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
    impl std::io::Write for Tap {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("tap").extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// One connection whose chunk view has been opened around `centre` the
    /// way a real join opens one — the spawn blob is `mark_sent`, then
    /// `updateChunkTracking` runs.
    fn joined(
        players: &crate::players::Players,
        name: &str,
        tap: &Tap,
        centre: (i32, i32),
        view_distance: i32,
    ) -> std::sync::Arc<crate::players::Connection> {
        let c = std::sync::Arc::new(crate::players::Connection::with_entity(
            players.next_id(),
            name,
            Box::new(tap.clone()),
            None,
            0,
            view_distance,
            centre,
        ));
        players.join(&c);
        c.with_chunk_view(|v| {
            let side = view_distance + 3;
            for dx in -side..=side {
                for dz in -side..=side {
                    v.mark_sent((centre.0 + dx, centre.1 + dz));
                }
            }
            v.update_tracking(centre);
        });
        c
    }

    /// **The whole of this area in one assertion.**
    ///
    /// `ChunkHolder.broadcastChanges` sends to `getPlayers(pos, false)`, and
    /// the only thing that removes a player from that list is
    /// `ChunkMap.isChunkTracked`. Until this branch `flush` went through
    /// `Players::broadcast`, which has no filter of any kind, so **every
    /// connected client received every block change in the world** — a player
    /// a thousand blocks away was told about a block it has never held the
    /// chunk for.
    ///
    /// Both halves are asserted, because a filter that drops everybody is as
    /// wrong as one that drops nobody and is much easier to write by
    /// accident: the near client must still get its packet.
    #[test]
    fn a_block_update_reaches_the_client_holding_its_chunk_and_no_other() {
        let players = crate::players::Players::new();
        let near = Tap::default();
        let far = Tap::default();
        // View distance 4, so the padded disc reaches five chunks along an
        // axis and stops: `|dz| = 5` gives `3 * 3 = 9 < 16`, `|dz| = 6` gives
        // `4 * 4 = 16`, which is not `< 16`.
        let _n = joined(&players, "near", &near, (0, 0), 4);
        let _f = joined(&players, "far", &far, (0, 40), 4);

        let changed = ChangedBlocks::new();
        changed.record((8, 64, 8));
        changed.flush(&players, &OneBlock);

        let near_bytes = near.0.lock().expect("tap").clone();
        let far_bytes = far.0.lock().expect("tap").clone();
        let (id, _) =
            rustmc_proto::frame::read_frame(&mut &near_bytes[..], false).expect("one frame");
        assert_eq!(
            id,
            rustmc_proto::ids::play::cb::BLOCK_UPDATE,
            "the client holding chunk (0, 0) is told"
        );
        assert!(
            far_bytes.is_empty(),
            "a client 40 chunks away holds no such chunk and vanilla tells it nothing, \
             got {} bytes",
            far_bytes.len()
        );
    }

    /// The second limb of `isChunkTracked`, at the level the tick runs it:
    /// a change in a chunk that is tracked but still **queued** is withheld
    /// and then dropped, not held over to the tick after the chunk lands.
    ///
    /// `broadcastChanges` nulls `changedBlocksPerSection[j]` at offsets
    /// 156-163 and only then tests `list.isEmpty()` at 164-173, so the set is
    /// consumed whether or not anyone was listening. The positive control is
    /// the same server one line later with the queue drained.
    #[test]
    fn a_change_in_a_pending_chunk_is_withheld_and_not_deferred() {
        let players = crate::players::Players::new();
        let tap = Tap::default();
        let c = std::sync::Arc::new(crate::players::Connection::with_entity(
            players.next_id(),
            "loading",
            Box::new(tap.clone()),
            None,
            0,
            4,
            (0, 0),
        ));
        players.join(&c);
        c.with_chunk_view(|v| v.update_tracking((0, 0)));

        let changed = ChangedBlocks::new();
        changed.record((8, 64, 8));
        changed.flush(&players, &OneBlock);
        assert!(
            tap.0.lock().expect("tap").is_empty(),
            "a queued chunk's changes go nowhere"
        );

        // The chunk arrives, and the change is *gone* — that is the claim.
        c.with_chunk_view(|v| {
            // The client's own `chunk_batch_received` after each batch, which
            // is not optional bookkeeping: `maxUnacknowledgedBatches` is
            // **1** until the first one arrives, so a loop that only calls
            // `take_batch` gets one batch and then `Vec::new()` for ever.
            // Bounded and asserted rather than `while`, because "for ever" is
            // how this test first hung a whole `cargo test` run.
            for _ in 0..64 {
                if v.pending_len() == 0 {
                    break;
                }
                let batch = v.take_batch((0, 0), |_| true);
                assert!(
                    !batch.is_empty(),
                    "the sender makes progress once acknowledged"
                );
                v.on_batch_received(64.0);
            }
            assert_eq!(v.pending_len(), 0, "the queue drains");
        });
        changed.flush(&players, &OneBlock);
        assert!(
            tap.0.lock().expect("tap").is_empty(),
            "the tick that produced it consumed it; nothing is replayed"
        );

        // The control: with the queue drained, a *new* change does arrive.
        changed.record((8, 64, 8));
        changed.flush(&players, &OneBlock);
        assert!(
            !tap.0.lock().expect("tap").is_empty(),
            "the client is a recipient once the chunk is no longer pending"
        );
    }

    /// `getPlayers` is called once per `ChunkHolder`, so every section of one
    /// chunk column goes to the same list — and two changes in two different
    /// sections of one column are two packets, not one.
    ///
    /// The far client is the control on the grouping: if the recipient list
    /// were resolved once for the whole flush rather than once per column, a
    /// column the far client *does* hold would leak the other column's
    /// packets to it.
    #[test]
    fn two_columns_resolve_two_recipient_lists() {
        let players = crate::players::Players::new();
        let near = Tap::default();
        let far = Tap::default();
        let _n = joined(&players, "near", &near, (0, 0), 4);
        let _f = joined(&players, "far", &far, (40, 0), 4);

        let changed = ChangedBlocks::new();
        changed.record((8, 64, 8)); // chunk (0, 0), near only
        changed.record((8, 80, 8)); // chunk (0, 0), a second section
        changed.record((644, 64, 8)); // chunk (40, 0), far only
        changed.flush(&players, &OneBlock);

        let count = |t: &Tap| {
            let bytes = t.0.lock().expect("tap").clone();
            let mut r = &bytes[..];
            let mut n = 0;
            while !r.is_empty() {
                rustmc_proto::frame::read_frame(&mut r, false).expect("a frame");
                n += 1;
            }
            n
        };
        assert_eq!(count(&near), 2, "both sections of the column it holds");
        assert_eq!(count(&far), 1, "its own column and not the other one");
    }

    #[test]
    fn the_lock_probe_reports_free_as_well_as_held() {
        let m = std::sync::Mutex::new(());
        assert!(!is_held(&m), "nobody holds it");
        let guard = m.lock().expect("fresh mutex");
        assert!(
            std::thread::scope(|s| s.spawn(|| is_held(&m)).join().expect("prober")),
            "held, as seen from another thread"
        );
        drop(guard);
        assert!(!is_held(&m), "released again");
    }

    /// A negative world coordinate is in a negative section, and the naive
    /// `/ 16` puts it in the wrong one for everything above `-16`.
    #[test]
    fn sections_floor_rather_than_truncate() {
        assert_eq!(section_of(0), 0);
        assert_eq!(section_of(15), 0);
        assert_eq!(section_of(16), 1);
        assert_eq!(section_of(-1), -1, "not 0, which is what -1 / 16 gives");
        assert_eq!(section_of(-16), -1);
        assert_eq!(section_of(-17), -2);
    }

    #[test]
    fn section_relative_positions_round_trip() {
        for pos in [(0, 0, 0), (5, -61, -3), (-1, -1, -1), (255, 319, -300)] {
            let (sx, sy, sz) = (section_of(pos.0), section_of(pos.1), section_of(pos.2));
            let s = section_relative_pos(pos.0, pos.1, pos.2);
            assert_eq!(from_section_relative(sx, sy, sz, s), pos, "{pos:?}");
        }
    }

    /// The body's shape, checked against the field order in the bytecode:
    /// the packed section long, then a count, then one varlong per entry.
    /// The packing, over coordinates of both signs, decoded back with the
    /// shifts the harness uses. `x` and `z` are 22 bits and `y` is 20; a
    /// 20-bit mask on `z` round-trips every non-negative section and no
    /// negative one, which is why this test exists at all.
    #[test]
    fn the_section_position_round_trips_through_both_signs() {
        for (sx, sy, sz) in [(0, 0, 0), (1, -4, 2), (-1, -4, -1), (-46, -4, -3)] {
            let body = section_blocks_update_body(sx, sy, sz, &[(0, 1)]).expect("buf");
            let packed = i64::from_be_bytes(body[..8].try_into().expect("8 bytes"));
            assert_eq!((packed >> 42) as i32, sx, "x of {sx},{sy},{sz}");
            assert_eq!(((packed << 44) >> 44) as i32, sy, "y of {sx},{sy},{sz}");
            assert_eq!(((packed << 22) >> 42) as i32, sz, "z of {sx},{sy},{sz}");
        }
    }

    #[test]
    fn the_body_is_a_long_a_count_and_a_varlong_each() {
        let body = section_blocks_update_body(0, 0, 0, &[(1, 1), (2, 9)]).expect("buf");
        assert_eq!(&body[..8], &0_i64.to_be_bytes());
        assert_eq!(body[8], 2, "the count");
        // (1 << 12) | 1 = 4097, and (9 << 12) | 2 = 36866, both varlongs.
        let mut want = Vec::new();
        rustmc_proto::varint::write_varlong(&mut want, 4097).expect("buf");
        rustmc_proto::varint::write_varlong(&mut want, 36866).expect("buf");
        assert_eq!(&body[9..], &want[..]);
    }

    /// The three properties a Chebyshev implementation would get wrong,
    /// each asserted at the position where the two disagree rather than
    /// somewhere they happen to agree.
    #[test]
    fn the_tracking_view_is_euclidean_padded_and_exclusive() {
        let c = (0, 0);
        // The centre and everything inside the pad: `dx` clamps to zero.
        assert!(chunk_is_tracked(c, 10, (0, 0)));
        assert!(chunk_is_tracked(c, 10, (2, 2)));

        // **Excluded at the corner, where Chebyshev would include it.**
        // |dx| = |dz| = 10, minus the pad is 8, and 64 + 64 = 128 is not
        // < 100.
        assert!(!chunk_is_tracked(c, 10, (10, 10)));
        // Included one beyond the nominal view distance along an axis,
        // where Chebyshev would exclude it: 11 - 2 = 9, and 81 < 100.
        assert!(chunk_is_tracked(c, 10, (11, 0)));
        // And the first one out along that axis: 12 - 2 = 10, 100 is not
        // < 100. This is the `<` rather than `<=`.
        assert!(!chunk_is_tracked(c, 10, (12, 0)));

        // Symmetric in all four quadrants, which the `abs` is there for.
        for (x, z) in [(11, 0), (-11, 0), (0, 11), (0, -11)] {
            assert!(chunk_is_tracked(c, 10, (x, z)), "at ({x}, {z})");
        }

        // Off-centre, so a wrong sign on the subtraction cannot pass.
        assert!(chunk_is_tracked((100, -50), 10, (111, -50)));
        assert!(!chunk_is_tracked((100, -50), 10, (112, -50)));

        // A view distance of 2 tracks only what the pad already covers:
        // dx clamps to 0 out to 2, and 0 < 4, but at 3 the value is 1 and
        // 1 < 4 still holds, while at 4 it is 2 and 4 is not < 4.
        assert!(chunk_is_tracked(c, 2, (3, 0)));
        assert!(!chunk_is_tracked(c, 2, (4, 0)));
    }
}
