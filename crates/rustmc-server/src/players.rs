//! Who is connected, and how one player's thread reaches another's socket.
//!
//! # Why a connection's writes have to go through one place
//!
//! Until a block could change, no thread ever had a reason to write to a
//! socket it did not own: each connection read its own client and answered
//! it. A block update breaks that — the player who broke it is on one thread
//! and everyone who has to be told is on the others — and two threads writing
//! frames to the same `TcpStream` interleave them. The client does not see
//! two packets in an unexpected order; it sees one packet made of both, and
//! disconnects with a protocol error several frames later.
//!
//! So a connection's *outbound* half is a mutex around a writer, and every
//! packet after the join goes through [`Connection::send`]. The inbound half
//! needs no lock: exactly one thread ever reads a socket.
//!
//! # Failures here are not the sender's problem
//!
//! [`Players::broadcast`] discards write errors. A connection that has gone
//! away will fail its own next read and clean itself up on its own thread;
//! propagating that failure into the thread of the player who happened to
//! break a block would kill the wrong session.

use std::io::{self, Write};
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// One player's outbound half.
pub struct Connection {
    id: u64,
    name: String,
    out: Mutex<Box<dyn Write + Send>>,
    comp: Option<i32>,
    /// The highest block-action sequence this connection owes an
    /// acknowledgement for, or `-1` for none.
    ///
    /// Vanilla does not answer a sequence from inside the handler.
    /// `ServerGamePacketListenerImpl.ackBlockChangesUpTo` only records
    /// `max(seq, ackBlockChangesUpTo)`, and `tick()` sends the packet and
    /// resets it to `-1`. The difference is observable and this server used
    /// to get it wrong: the ack must arrive **after** the tick's block
    /// broadcasts, not before them.
    pending_ack: AtomicI32,
    /// Packets a handler has produced that belong to the **next tick's**
    /// menu sweep, `(packet id, body)` in the order they were produced.
    ///
    /// `AbstractContainerMenu.broadcastChanges` is not called by the code that
    /// changes a slot. `Inventory.add` writes the slot and returns; the
    /// `container_set_slot` goes out from `ServerPlayer.doTick`, inside the
    /// entity tick — so **after** `ServerChunkCache.tick` has flushed that
    /// tick's block updates and **before** the connection tick sends the
    /// acknowledgement.
    ///
    /// That is the same reason [`Self::pending_ack`] exists and it is a
    /// different position in the tick, so it is a second queue rather than a
    /// flag on the first. Measured off vanilla's own recording of
    /// `790-a-flower-pot-empties-when-right-clicked`: `block_update`,
    /// `block_update`, `block_update`, `container_set_slot`,
    /// `block_changed_ack`. A handler that sent the slot itself would put it
    /// second, in front of the tick's broadcast.
    pending_menu: Mutex<Vec<(i32, Vec<u8>)>>,
    /// The entity id this player *is*, as told to its own client in the join
    /// packet and to every other client in an `add_entity`.
    ///
    /// Carried on the connection rather than looked up, because the entity
    /// tracker runs on the clock thread and has to answer "is this entity the
    /// player I am about to send to?" for every pair of entity and player on
    /// every section change. A map lookup per pair for a number that never
    /// changes is the wrong shape.
    entity_id: i32,
    /// The view distance this client was given, which
    /// `ChunkMap.getPlayerViewDistance` clamps and `TrackedEntity`'s range
    /// test then multiplies by 16. It lives here for the same reason
    /// `entity_id` does: the tracker asks for it once per entity per player
    /// per section change.
    view_distance: i32,
    /// `ServerGamePacketListenerImpl.nextChatIndex` — the `globalIndex` of
    /// the next `player_chat` packet **this connection** is sent.
    ///
    /// Per connection, not per speaker: `sendPlayerChatMessage` increments a
    /// counter on the recipient's listener, so two players who joined at
    /// different times see different indices for the same message. An `i32`
    /// because that is what the field is and what the varint carries.
    next_chat_index: AtomicI32,
    /// `ServerPlayer.chatVisibility`, from `client_information`.
    ///
    /// `acceptsChatMessages()` is `chatVisibility == FULL`, so a recipient on
    /// SYSTEM or HIDDEN is sent **no** chat packet at all. Stored as the raw
    /// ordinal so the connection does not have to depend on `crate::chat`.
    chat_visibility: AtomicI32,
    /// The container menu this player has open, shared with the session that
    /// owns it — see [`crate::menu::MenuState::open`]. `None` while the
    /// player's own inventory is the open menu.
    open_menu: Mutex<Option<crate::menu::SharedMenu>>,
    /// `ServerPlayer.chunkTrackingView` and
    /// `ServerGamePacketListenerImpl.chunkSender`, which are the two halves
    /// of `ChunkMap.isChunkTracked` and therefore the **only** filter on
    /// `ChunkHolder.broadcastChanges`'s recipient list.
    ///
    /// # Why it lives here rather than on the session's stack
    ///
    /// It used to be a `&mut ChunkView` on [`crate::world_send::Session`],
    /// which is where a value only that connection's own thread reads
    /// belongs. It is not that: the tick's block broadcast runs on the clock
    /// thread and has to ask *every* player whether it holds the chunk that
    /// changed. In vanilla both fields hang off `ServerPlayer` and its
    /// listener, which the game thread reaches directly; a stack local cannot
    /// be reached at all, and that is the whole reason
    /// [`Players::broadcast`] sent every block update to every player in the
    /// world.
    ///
    /// A `Mutex` rather than a snapshot published beside it: a copy is a
    /// chance for the two to disagree, and this one would disagree exactly
    /// while a player was crossing a chunk border, which is the moment the
    /// answer changes.
    ///
    /// **It is a leaf lock and must stay one.** The clock thread takes
    /// [`crate::broadcast::tick_lock`] and then this; a connection thread
    /// that took this and then the tick lock would close the cycle. Nothing
    /// inside [`Self::with_chunk_view`]'s closures may take the tick lock —
    /// the four production callers are all in the read loop, outside every
    /// handler, and `Tiles::chunk` answers `None` rather than blocking.
    chunk_view: Mutex<crate::world_send::ChunkView>,
    /// `ServerPlayerGameMode.gameModeForPlayer`, as a `GameType` id.
    ///
    /// Per player, as vanilla's is, and seeded from the server's default
    /// (`crate::status::game_type`) when the connection is made — which is
    /// what `PlayerList.placeNewPlayer` does for a player with no saved mode.
    /// `/gamemode` is the one thing that moves it; the play loop reads it
    /// once per frame into `Session::creative`, so every mechanic that asks
    /// "is this player creative" follows a mode change on the next frame.
    game_type: std::sync::atomic::AtomicU8,
    /// Teleports a command issued for this player from another thread, in
    /// order, for the play loop to apply on its next pass — see
    /// [`PendingTeleport`].
    pending_teleports: Mutex<Vec<PendingTeleport>>,
}

fn entity_game_types() -> &'static Mutex<std::collections::BTreeMap<i32, u8>> {
    static MAP: std::sync::OnceLock<Mutex<std::collections::BTreeMap<i32, u8>>> =
        std::sync::OnceLock::new();
    MAP.get_or_init(|| Mutex::new(std::collections::BTreeMap::new()))
}

/// The game mode of the player that IS entity `id`, once `/gamemode` has set
/// one; `None` means "the server default", which is every player's mode until
/// a command changes it.
#[must_use]
pub fn game_type_of_entity(id: i32) -> Option<u8> {
    entity_game_types()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&id)
        .copied()
}

/// One `Entity.teleportTo(level, x, y, z, relatives, yRot, xRot, true)` a
/// command asked for, with its arguments exactly as `TeleportCommand.
/// performTeleport` computed them: on a relative axis the coordinate is
/// already the DELTA from the player's position, and `relatives` is the
/// `Relative` bitmask (`1 << bit`, X 0 .. ROTATE_DELTA 8).
///
/// Queued rather than applied where the command ran because the player's
/// position, its awaited-teleport id and its chunk view all belong to the
/// play loop's `Session`, which only that loop's thread holds — the same
/// reason vanilla's `ServerGamePacketListenerImpl.teleport` is only ever
/// called on the server thread.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PendingTeleport {
    pub pos: [f64; 3],
    pub y_rot: f32,
    pub x_rot: f32,
    pub relatives: i32,
    /// `Entity.forceSetRotation` rather than `teleportTo`: `/rotate`. The
    /// position is unused, `relatives` carries only the two rotation bits,
    /// and what goes out is `player_rotation`, not `player_position` — so no
    /// teleport id is spent and the client acknowledges nothing.
    pub rotation_only: bool,
}

impl std::fmt::Debug for Connection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Connection")
            .field("id", &self.id)
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl Connection {
    #[must_use]
    pub fn new(id: u64, name: &str, out: Box<dyn Write + Send>, comp: Option<i32>) -> Self {
        Self::with_entity(id, name, out, comp, 0, 0, (0, 0))
    }

    /// A connection that knows which entity its player is.
    ///
    /// `entity_id` 0 means "not an entity", which is what [`Self::new`]
    /// leaves it as. Zero is safe as that sentinel for the reason
    /// `Entities::next_id` starts at 1: a client reads entity 0 as no entity,
    /// so the level never allocates it.
    /// `spawn_chunk` is only the chunk this server's [`crate::world_send::enter_world`]
    /// has already named in a `set_chunk_cache_center`, so that the first
    /// `update_tracking` does not write a second, identical one. See
    /// `ChunkView::announced_center`.
    #[must_use]
    pub fn with_entity(
        id: u64,
        name: &str,
        out: Box<dyn Write + Send>,
        comp: Option<i32>,
        entity_id: i32,
        view_distance: i32,
        spawn_chunk: (i32, i32),
    ) -> Self {
        Self {
            chunk_view: Mutex::new(crate::world_send::ChunkView::new(
                spawn_chunk,
                view_distance,
            )),
            id,
            name: name.to_string(),
            out: Mutex::new(out),
            comp,
            pending_ack: AtomicI32::new(-1),
            pending_menu: Mutex::new(Vec::new()),
            entity_id,
            view_distance,
            // `nextChatIndex` is a plain field on a fresh listener, so it
            // starts at 0 and the first message a connection is sent carries
            // globalIndex 0.
            next_chat_index: AtomicI32::new(0),
            // FULL until `client_information` says otherwise. Vanilla's
            // `ServerPlayer` is built from a `ClientInformation` that always
            // has one, and this server reads it in configuration; the default
            // matters only for a connection constructed in a test.
            chat_visibility: AtomicI32::new(0),
            open_menu: Mutex::new(None),
            game_type: std::sync::atomic::AtomicU8::new(crate::status::game_type()),
            pending_teleports: Mutex::new(Vec::new()),
        }
    }

    /// Read or change this player's experience under its lock.
    pub fn with_experience<T>(&self, f: impl FnOnce(&mut crate::experience::Experience) -> T) -> T {
        // The one store is `crate::experience::Registry`, by entity id —
        // the clock thread's orb pickup and `set_experience` read it there.
        let mut f = Some(f);
        crate::experience::Registry::shared()
            .with(self.entity_id(), |x, _| (f.take().expect("called once"))(x))
            .unwrap_or_else(|| {
                (f.take().expect("not yet called"))(&mut crate::experience::Experience::new())
            })
    }

    /// Queue a command's teleport for this player's play loop.
    pub fn queue_teleport(&self, t: PendingTeleport) {
        self.pending_teleports
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(t);
    }

    /// Take every queued teleport, oldest first.
    pub fn take_teleports(&self) -> Vec<PendingTeleport> {
        std::mem::take(
            &mut *self
                .pending_teleports
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }

    /// This player's `GameType` id: survival 0, creative 1, adventure 2,
    /// spectator 3.
    #[must_use]
    pub fn game_type(&self) -> u8 {
        self.game_type.load(Ordering::Relaxed)
    }

    /// `ServerPlayerGameMode.setGameModeForPlayer`'s store. Returns the
    /// previous mode.
    ///
    /// Also published by entity id for code that holds an entity and no
    /// connection — `crate::mob`'s view of a player — see
    /// [`game_type_of_entity`].
    pub fn set_game_type(&self, id: u8) -> u8 {
        if self.entity_id != 0 {
            entity_game_types()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(self.entity_id, id);
        }
        self.game_type.swap(id, Ordering::Relaxed)
    }

    /// `nextChatIndex++` — the `globalIndex` for the next `player_chat` sent
    /// here.
    ///
    /// Post-increment, matching `dup_x1; iconst_1; iadd; putfield` at
    /// `sendPlayerChatMessage` offsets 7..15: the packet carries the value
    /// *before* the bump, so the first is 0.
    pub fn next_chat_index(&self) -> i32 {
        self.next_chat_index.fetch_add(1, Ordering::AcqRel)
    }

    /// What this client asked to be shown.
    #[must_use]
    pub fn chat_visibility(&self) -> crate::chat::Visibility {
        crate::chat::Visibility::from_ordinal(self.chat_visibility.load(Ordering::Acquire))
    }

    /// Record the visibility `client_information` carried.
    pub fn set_chat_visibility(&self, ordinal: i32) {
        self.chat_visibility.store(ordinal, Ordering::Release);
    }

    /// The entity id of the player on this connection.
    #[must_use]
    pub const fn entity_id(&self) -> i32 {
        self.entity_id
    }

    /// The view distance this client was given.
    #[must_use]
    pub const fn view_distance(&self) -> i32 {
        self.view_distance
    }

    /// Do something with this connection's chunk view.
    ///
    /// Exactly one thread ever *writes* here — the connection's own read loop
    /// — and the clock thread reads. See the field for why it is a lock and
    /// not a published copy, and for the rule that keeps it a leaf.
    ///
    /// # Panics
    /// If the lock is poisoned.
    pub fn with_chunk_view<R>(&self, f: impl FnOnce(&mut crate::world_send::ChunkView) -> R) -> R {
        f(&mut self.chunk_view.lock().expect("chunk view lock"))
    }

    /// `ChunkMap.isChunkTracked(player, x, z)`:
    ///
    /// ```java
    /// player.getChunkTrackingView().contains(x, z)
    ///     && !player.connection.chunkSender.isPending(ChunkPos.pack(x, z))
    /// ```
    ///
    /// **Both limbs, and the second is not bookkeeping.** A chunk this client
    /// is tracking but has not yet been *sent* is withheld from every
    /// `broadcastChanges` recipient list — withheld, not deferred: the change
    /// is consumed by the tick that produced it and arrives only as part of
    /// the chunk itself.
    ///
    /// # Panics
    /// If the lock is poisoned.
    #[must_use]
    pub fn is_chunk_tracked(&self, chunk: (i32, i32)) -> bool {
        self.with_chunk_view(|v| v.is_chunk_tracked(chunk))
    }

    /// `ChunkMap.isChunkOnTrackedBorder(player, x, z)`, offsets 0-74:
    ///
    /// ```java
    /// if (!isChunkTracked(player, x, z)) return false;
    /// for (int dx = -1; dx <= 1; dx++)
    ///     for (int dz = -1; dz <= 1; dz++) {
    ///         if (dx == 0 && dz == 0) continue;
    ///         if (!isChunkTracked(player, x + dx, z + dz)) return true;
    ///     }
    /// return false;
    /// ```
    ///
    /// Tracked, **and** with at least one of the eight neighbours not
    /// tracked. The centre is skipped by the `dx == 0 && dz == 0` test at
    /// offsets 30-40, which is why it is a ring of eight and not a square of
    /// nine — and the loops run `-1..=1` on both axes, so a chunk on the
    /// diagonal edge qualifies through a diagonal neighbour alone.
    ///
    /// # Panics
    /// If the lock is poisoned.
    #[must_use]
    pub fn is_chunk_on_tracked_border(&self, chunk: (i32, i32)) -> bool {
        self.with_chunk_view(|v| {
            if !v.is_chunk_tracked(chunk) {
                return false;
            }
            for dx in -1..=1 {
                for dz in -1..=1 {
                    if dx == 0 && dz == 0 {
                        continue;
                    }
                    if !v.is_chunk_tracked((chunk.0 + dx, chunk.1 + dz)) {
                        return true;
                    }
                }
            }
            false
        })
    }

    /// `ackBlockChangesUpTo`: record, never send.
    ///
    /// # Panics
    /// Never; the compare-exchange loop is total.
    pub fn ack_block_changes_up_to(&self, sequence: i32) {
        let _ = self
            .pending_ack
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |cur| {
                Some(cur.max(sequence))
            });
    }

    /// Queue a packet for the next menu sweep. See [`Self::pending_menu`].
    ///
    /// # Panics
    /// If the lock is poisoned.
    pub fn defer_to_menu_sweep(&self, packet: i32, body: Vec<u8>) {
        self.pending_menu
            .lock()
            .expect("pending menu lock")
            .push((packet, body));
    }

    /// Register (or clear) the container menu this player has open, for the
    /// clock thread's per-tick sweep.
    ///
    /// # Panics
    /// If the lock is poisoned.
    pub fn set_open_menu(&self, menu: Option<crate::menu::SharedMenu>) {
        *self.open_menu.lock().expect("open menu lock") = menu;
    }

    /// The container menu this player has open, if any.
    ///
    /// # Panics
    /// If the lock is poisoned.
    #[must_use]
    pub fn open_menu(&self) -> Option<crate::menu::SharedMenu> {
        self.open_menu.lock().expect("open menu lock").clone()
    }

    /// `ServerPlayer.doTick`'s `this.containerMenu.broadcastChanges()`: send
    /// what handlers queued, in the order they queued it, and clear.
    ///
    /// # Panics
    /// If the lock is poisoned.
    pub fn flush_menu_sweep(&self) {
        let queued = std::mem::take(&mut *self.pending_menu.lock().expect("pending menu lock"));
        for (packet, body) in queued {
            let _ = self.send(packet, &body);
        }
    }

    /// The tail of `ServerGamePacketListenerImpl.tick`: send the pending
    /// acknowledgement if there is one, and clear it.
    ///
    /// # Errors
    /// Never returns one; a dead socket is dropped like every other send
    /// here.
    pub fn flush_ack(&self) {
        let seq = self.pending_ack.swap(-1, Ordering::AcqRel);
        if seq > -1 {
            if let Ok(body) = crate::interact::block_changed_ack_body(seq) {
                let _ = self.send(rustmc_proto::ids::play::cb::BLOCK_CHANGED_ACK, &body);
            }
        }
    }

    #[must_use]
    pub const fn id(&self) -> u64 {
        self.id
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether frames on this connection are deflated — which the *reading*
    /// half has to agree with, and which is negotiated once during login.
    /// Carried here so a caller cannot hold the threshold for writing and a
    /// stale opinion for reading.
    #[must_use]
    pub const fn compressed(&self) -> bool {
        self.comp.is_some()
    }

    /// Write one frame, holding the connection's write lock for exactly as
    /// long as the frame takes.
    ///
    /// # Errors
    /// On I/O failure.
    ///
    /// # Panics
    /// If the lock is poisoned — a thread died mid-frame, so the stream is
    /// half a packet in and nothing sensible can follow.
    pub fn send(&self, packet: i32, body: &[u8]) -> io::Result<()> {
        let mut out = self.out.lock().expect("connection write lock");
        rustmc_proto::frame::write_frame(&mut *out, self.comp, packet, body)
    }
}

/// Everyone in play.
#[derive(Debug, Default)]
pub struct Players {
    next_id: AtomicU64,
    /// Not a map: this is walked far more often than it is searched, and it
    /// holds one entry per connected player.
    conns: Mutex<Vec<Arc<Connection>>>,
}

impl Players {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A fresh connection id. Monotonic, never reused, so a stale reference
    /// to a departed player can never name a live one.
    pub fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// # Panics
    /// If the lock is poisoned.
    pub fn join(&self, conn: &Arc<Connection>) {
        self.conns
            .lock()
            .expect("players lock")
            .push(Arc::clone(conn));
    }

    /// # Panics
    /// If the lock is poisoned.
    pub fn leave(&self, id: u64) {
        self.conns
            .lock()
            .expect("players lock")
            .retain(|c| c.id() != id);
    }

    /// How many players are in play.
    ///
    /// Counted here rather than kept as a separate counter, so the number in
    /// the server list and the set that receives a broadcast cannot disagree.
    ///
    /// # Panics
    /// If the lock is poisoned.
    #[must_use]
    pub fn count(&self) -> usize {
        self.conns.lock().expect("players lock").len()
    }

    /// Send one packet to everyone, **including the player who caused it**.
    ///
    /// # There is no exclusion, and there was one for a day
    ///
    /// `ChunkHolder.broadcast(List<ServerPlayer>, Packet)` is
    /// `list.forEach(p -> p.connection.send(packet))` — no filter, no actor
    /// check, nothing. The list is `getPlayers(pos, false)`, and the only
    /// thing that removes a player from it is
    ///
    /// ```java
    /// ChunkMap.isChunkTracked(player, x, z)
    ///     = player.getChunkTrackingView().contains(x, z)
    ///       && !player.connection.chunkSender.isPending(ChunkPos.pack(x, z))
    /// ```
    ///
    /// So a player is told about a block change in a chunk it has actually
    /// been sent, and is not told about one in a chunk still queued for
    /// delivery. Nothing about who acted enters into it.
    ///
    /// This method briefly skipped the acting player, on the observation
    /// that vanilla never sent one a broadcast `block_update` for its own
    /// action. **That observation was an artefact of a harness that never
    /// acknowledged a chunk batch**, so vanilla had stopped sending it chunks
    /// after the first nine and every workspace it walked to sat in chunks
    /// that were permanently pending. Re-measured on 2026-08-03 with a
    /// conforming client, vanilla sends the acting player the two
    /// `handleUseItemOn` resyncs, **then the broadcast**, then the ack — four
    /// observations for one placement, and the third is this one. See
    /// `ci/chunk-batch-probe.sh`.
    ///
    /// **The `isChunkTracked` gate is reproduced now and it is not here.**
    /// Until 2026-09-05 this method was what `ChangedBlocks::flush` used, and
    /// it sent to every connected player regardless of what chunks they held.
    /// The block path goes through [`Self::players_for_chunk`] instead, which
    /// is `ChunkMap.getPlayers`; this one stays unfiltered because that is
    /// what `PlayerList.broadcastAll` is, and its remaining callers are the
    /// packets vanilla really does send to everyone.
    ///
    /// # Panics
    /// If the lock is poisoned.
    pub fn flush_acks(&self) {
        let conns: Vec<Arc<Connection>> = self.conns.lock().expect("players lock").clone();
        for c in conns {
            c.flush_ack();
        }
    }

    /// `ServerPlayer.doTick`'s menu sweep, for every connection.
    ///
    /// **Its position in the tick is the whole of it.** It runs after the
    /// block broadcast and before [`Self::flush_acks`], which is where
    /// `ServerLevel.tick` puts the entity ticks: `ServerChunkCache.tick`
    /// flushes block updates, `runBlockEvents` runs, the entity tick list
    /// (`ServerPlayer.doTick` among it) broadcasts menu changes, and only
    /// then does the connection tick send acknowledgements. Called from one
    /// place in [`crate::status`] for that reason.
    ///
    /// # Panics
    /// If the lock is poisoned.
    pub fn flush_menu_sweeps(&self) {
        let conns: Vec<Arc<Connection>> = self.conns.lock().expect("players lock").clone();
        for c in conns {
            c.flush_menu_sweep();
        }
    }

    /// Send one packet to everyone. See the doc above `flush_acks`'s caller
    /// for why the acknowledgement is not one of them.
    ///
    /// # Panics
    /// If the lock is poisoned.
    pub fn broadcast(&self, packet: i32, body: &[u8]) {
        for c in self.connections() {
            let _ = c.send(packet, body);
        }
    }

    /// `PlayerList.broadcast(Player except, …)` — everyone but one.
    ///
    /// # This is a different rule from `broadcast` and it is not an
    /// optimisation of it
    ///
    /// The doc above [`Self::flush_acks`] explains at length why the *block*
    /// broadcast has no exclusion: `ChunkHolder.broadcast` takes a list and
    /// filters it on `isChunkTracked` alone, and a version of this server that
    /// skipped the actor there was wrong. That argument is about
    /// `ChunkHolder`. `PlayerList.broadcast` is a different method with a
    /// different signature, and its first argument is skipped outright —
    /// offsets 33-39, `if (players.get(i) == except) continue;`.
    ///
    /// Three things reach it with a real player rather than `null`, and each
    /// is a case where vanilla deliberately does not tell you about your own
    /// action: `Block.spawnDestroyParticles` (the block you dug),
    /// `ButtonBlock.playSound` on the **press** but not the release, and the
    /// two double-block `playerWillDestroy` overrides. `010-break-block` is a
    /// strict `Match` because of the first, and `330`/`335` depend on the
    /// second — the presser hears the un-press and not the press.
    ///
    /// # Panics
    /// If the lock is poisoned.
    pub fn broadcast_except(&self, except: u64, packet: i32, body: &[u8]) {
        for c in self.connections() {
            if c.id() == except {
                continue;
            }
            let _ = c.send(packet, body);
        }
    }

    /// Everyone in play, cloned out from under the lock.
    ///
    /// Cloned before any I/O for the reason [`Self::broadcast`] always has: a
    /// slow socket must not hold the registry while another thread is trying
    /// to join. The entity tracker needs the same snapshot and must not take
    /// its own — two walks of the list under separate locks can disagree
    /// about who is connected, and a tracker that disagrees with the
    /// broadcaster sends an `add_entity` to a connection that has gone.
    ///
    /// # Panics
    /// If the lock is poisoned.
    #[must_use]
    pub fn connections(&self) -> Vec<Arc<Connection>> {
        self.conns.lock().expect("players lock").clone()
    }

    /// `ChunkMap.getPlayers(ChunkPos, boolean boundaryOnly)`, offsets 0-101.
    ///
    /// ```java
    /// for (ServerPlayer p : playerMap.getAllPlayers())
    ///     if (boundaryOnly ? isChunkOnTrackedBorder(p, pos.x, pos.z)
    ///                      : isChunkTracked(p, pos.x, pos.z))
    ///         builder.add(p);
    /// ```
    ///
    /// The bytecode spells that as two guarded arms rather than a ternary
    /// (`iload_2; ifeq 64` picks the border arm, `iload_2; ifne 93` the
    /// tracked one), and the two arms fall into the same `add`. **There is no
    /// distance-to-the-block test and no exclusion of whoever caused the
    /// change** — the tracking predicate is the whole filter, which is what
    /// makes it the one thing this server had to add.
    ///
    /// `boundary_only` is `true` for exactly one caller in the jar,
    /// `ChunkHolder.broadcastChanges`'s **light** update at offsets 33-47;
    /// the block half at offsets 111-125 passes `false`. This server sends no
    /// `light_update` outside a chunk body, so nothing here passes `true`
    /// yet — see [`Connection::is_chunk_on_tracked_border`], which is where
    /// the arm is and where a light broadcast will find it.
    ///
    /// # Panics
    /// If the lock is poisoned.
    #[must_use]
    pub fn players_for_chunk(
        &self,
        chunk: (i32, i32),
        boundary_only: bool,
    ) -> Vec<Arc<Connection>> {
        self.connections()
            .into_iter()
            .filter(|c| {
                if boundary_only {
                    c.is_chunk_on_tracked_border(chunk)
                } else {
                    c.is_chunk_tracked(chunk)
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A writer that records what it was given and can be read back after
    /// the `Connection` that owns it is gone.
    #[derive(Clone, Default)]
    struct Recorder(Arc<Mutex<Vec<u8>>>);
    impl Write for Recorder {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            self.0.lock().expect("recorder").extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// A connection whose view has been opened the way a join opens one:
    /// the spawn blob is `mark_sent`, then `updateChunkTracking` runs.
    fn joined(
        players: &Players,
        name: &str,
        rec: &Recorder,
        view_distance: i32,
    ) -> Arc<Connection> {
        let c = Arc::new(Connection::with_entity(
            players.next_id(),
            name,
            Box::new(rec.clone()),
            None,
            0,
            view_distance,
            (0, 0),
        ));
        players.join(&c);
        c.with_chunk_view(|v| {
            let side = view_distance + 3;
            for dx in -side..=side {
                for dz in -side..=side {
                    v.mark_sent((dx, dz));
                }
            }
            v.update_tracking((0, 0));
        });
        c
    }

    /// `ChunkMap.getPlayers(pos, false)` is `isChunkTracked` and nothing
    /// else, and a client that has been sent no chunks tracks none.
    ///
    /// The positive control is the same connection after its join has been
    /// replayed: without it this asserts only that some code path returns an
    /// empty vector, which a `getPlayers` hardwired to `Vec::new()` also
    /// satisfies.
    #[test]
    fn a_client_that_holds_no_chunks_receives_no_block_changes() {
        let players = Players::new();
        let rec = Recorder::default();
        let cold = Arc::new(Connection::with_entity(
            players.next_id(),
            "cold",
            Box::new(rec.clone()),
            None,
            0,
            4,
            (0, 0),
        ));
        players.join(&cold);
        assert!(
            players.players_for_chunk((0, 0), false).is_empty(),
            "ChunkTrackingView.EMPTY contains nothing, not even the centre"
        );

        let warm = joined(&players, "warm", &Recorder::default(), 4);
        let got = players.players_for_chunk((0, 0), false);
        assert_eq!(got.len(), 1, "only the joined client");
        assert_eq!(got[0].id(), warm.id());
    }

    /// The recipient set is the padded Euclidean disc, and the two ways it
    /// differs from the Chebyshev square a guess would use are asserted in
    /// **both** directions — a square is not a conservative approximation of
    /// this in either one.
    ///
    /// At view distance 10: `(11, 0)` gives `dx = 9`, `81 < 100`, tracked,
    /// and a square of radius 10 does not hold it; `(10, 10)` gives
    /// `dx = dz = 8`, `128 < 100` false, not tracked, and the square does.
    #[test]
    fn the_recipient_disc_is_padded_and_is_not_a_square() {
        let players = Players::new();
        let c = joined(&players, "seer", &Recorder::default(), 10);
        assert!(c.is_chunk_tracked((11, 0)), "the pad reaches one past 10");
        assert!(
            !c.is_chunk_tracked((10, 10)),
            "the corner of the square is outside the disc"
        );
        // The control on the control: the axis chunk one further out is
        // outside, so the first assertion is not just "everything is in".
        assert!(!c.is_chunk_tracked((13, 0)), "dx = 11, 121 is not < 100");
    }

    /// `isChunkTracked`'s second limb. A chunk inside the tracking view but
    /// still queued for delivery is **withheld**, not deferred: vanilla
    /// consumes the change in the tick that produced it and the client learns
    /// the new state only when the chunk itself arrives.
    #[test]
    fn a_chunk_that_is_tracked_but_still_pending_is_not_a_recipient() {
        let players = Players::new();
        let c = Arc::new(Connection::with_entity(
            players.next_id(),
            "loading",
            Box::new(Recorder::default()),
            None,
            0,
            4,
            (0, 0),
        ));
        players.join(&c);
        // No `mark_sent`: every chunk of the new view goes into the sender's
        // queue, exactly as it does for a player walking into fresh world.
        c.with_chunk_view(|v| {
            v.update_tracking((0, 0));
            assert!(v.pending_len() > 0, "the join queued something");
        });
        assert!(
            players.players_for_chunk((0, 0), false).is_empty(),
            "contains is true and isPending is true, so isChunkTracked is false"
        );

        // The positive control: draining the queue makes the same connection
        // a recipient, so the emptiness above is the `isPending` limb and not
        // the `contains` one.
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
        assert_eq!(players.players_for_chunk((0, 0), false).len(), 1);
    }

    /// `isChunkOnTrackedBorder` is the ring and never the interior: tracked,
    /// **and** at least one of the eight neighbours not tracked.
    ///
    /// The two assertions are the two halves of the `&&`. The centre of a
    /// view has all eight neighbours, so it fails the second; a chunk
    /// outside the view fails the first. Only the rim satisfies both, and the
    /// rim is what `broadcastChanges` sends its light update to.
    #[test]
    fn only_the_rim_of_a_view_is_on_the_tracked_border() {
        let players = Players::new();
        let c = joined(&players, "seer", &Recorder::default(), 4);
        assert!(
            c.is_chunk_tracked((0, 0)) && !c.is_chunk_on_tracked_border((0, 0)),
            "the centre is tracked and has eight tracked neighbours"
        );
        // `(5, 0)`: dx = 3, 9 < 16 so it is tracked; `(6, 0)` gives dx = 4
        // and 16 is not < 16, so the neighbour is not.
        assert!(c.is_chunk_tracked((5, 0)));
        assert!(!c.is_chunk_tracked((6, 0)));
        assert!(
            c.is_chunk_on_tracked_border((5, 0)),
            "tracked with an untracked neighbour"
        );
        assert!(
            !c.is_chunk_on_tracked_border((6, 0)),
            "not tracked at all, so the first line returns false"
        );
        // The border list is a strict subset of the tracked list, which is
        // what makes the light update reach fewer players than the block
        // update rather than different ones.
        let all = players.players_for_chunk((5, 0), false).len();
        let rim = players.players_for_chunk((5, 0), true).len();
        assert_eq!((all, rim), (1, 1));
        assert_eq!(
            (
                players.players_for_chunk((0, 0), false).len(),
                players.players_for_chunk((0, 0), true).len()
            ),
            (1, 0),
            "the centre is a block-update recipient and not a light one"
        );
    }

    #[test]
    fn a_broadcast_reaches_everyone_including_the_sender() {
        let players = Players::new();
        let a = Recorder::default();
        let b = Recorder::default();
        let ca = Arc::new(Connection::new(
            players.next_id(),
            "A",
            Box::new(a.clone()),
            None,
        ));
        let cb = Arc::new(Connection::new(
            players.next_id(),
            "B",
            Box::new(b.clone()),
            None,
        ));
        players.join(&ca);
        players.join(&cb);
        assert_eq!(players.count(), 2);

        players.broadcast(8, &[1, 2, 3]);

        for r in [&a, &b] {
            let bytes = r.0.lock().expect("recorder").clone();
            let (id, body) =
                rustmc_proto::frame::read_frame(&mut &bytes[..], false).expect("a frame");
            assert_eq!(id, 8);
            assert_eq!(body, vec![1, 2, 3]);
        }
    }

    /// Leaving removes exactly one player, and ids are never reused — so a
    /// broadcast after a departure cannot reach the departed.
    #[test]
    fn leaving_removes_one_and_ids_do_not_repeat() {
        let players = Players::new();
        let first = players.next_id();
        let second = players.next_id();
        assert_ne!(first, second);
        players.join(&Arc::new(Connection::new(
            first,
            "A",
            Box::new(Vec::new()),
            None,
        )));
        players.join(&Arc::new(Connection::new(
            second,
            "B",
            Box::new(Vec::new()),
            None,
        )));
        players.leave(first);
        assert_eq!(players.count(), 1);
        players.leave(first);
        assert_eq!(players.count(), 1, "leaving twice is not leaving twice");
    }

    /// Two threads writing frames to one connection must not interleave: the
    /// client sees one packet made of both and drops the connection several
    /// frames later, blaming whatever it was reading at the time.
    #[test]
    fn concurrent_sends_do_not_interleave_frames() {
        let rec = Recorder::default();
        let conn = Arc::new(Connection::new(0, "A", Box::new(rec.clone()), None));
        let mut threads = Vec::new();
        for t in 0..4u8 {
            let conn = Arc::clone(&conn);
            threads.push(std::thread::spawn(move || {
                for _ in 0..50 {
                    conn.send(8, &[t; 64]).expect("sent");
                }
            }));
        }
        for t in threads {
            t.join().expect("joined");
        }

        let bytes = rec.0.lock().expect("recorder").clone();
        let mut cur = &bytes[..];
        let mut frames = 0;
        while let Ok((id, body)) = rustmc_proto::frame::read_frame(&mut cur, false) {
            assert_eq!(id, 8);
            assert_eq!(body.len(), 64);
            assert!(
                body.iter().all(|b| *b == body[0]),
                "a frame carrying two threads' bytes is an interleave"
            );
            frames += 1;
        }
        assert_eq!(frames, 200, "every frame arrived whole");
    }

    /// **`nextChatIndex` post-increments, and the first message a connection
    /// is sent carries 0.**
    ///
    /// Added 2026-08-06 because `ci/chat-commands-falsify.sh` turned
    /// `fetch_add(1, ..)` into `fetch_add(1, ..) + 1` — a pre-increment — and
    /// **nothing caught it**. `crate::chat`'s tests pass `global_index` to
    /// `encode_player_chat` explicitly, which gates the *field* thoroughly
    /// and the *counter* not at all; no test called this method.
    ///
    /// Vanilla's is `dup_x1; iconst_1; iadd; putfield` at
    /// `sendPlayerChatMessage` offsets 7..15 — the packet carries the value
    /// before the bump. An off-by-one here is invisible to a single-message
    /// scenario, because both sides would simply start at 1, and would only
    /// surface as a mismatch against a real client's expectations.
    #[test]
    fn the_chat_index_post_increments_from_zero() {
        let c = Connection::new(1, "oracle", Box::new(Vec::new()), None);
        assert_eq!(c.next_chat_index(), 0, "the first is 0, not 1");
        assert_eq!(c.next_chat_index(), 1);
        assert_eq!(c.next_chat_index(), 2);
    }

    /// A fresh connection accepts chat, and `client_information` can turn it
    /// off. The default matters: it is what a connection built without a
    /// configuration exchange gets, and silencing such a player would be a
    /// stronger claim than we have evidence for.
    #[test]
    fn chat_visibility_defaults_to_full_and_is_settable() {
        let c = Connection::new(1, "oracle", Box::new(Vec::new()), None);
        assert_eq!(c.chat_visibility(), crate::chat::Visibility::Full);
        c.set_chat_visibility(2);
        assert_eq!(c.chat_visibility(), crate::chat::Visibility::Hidden);
        assert!(!c.chat_visibility().accepts_chat());
        c.set_chat_visibility(0);
        assert!(c.chat_visibility().accepts_chat());
    }
}
