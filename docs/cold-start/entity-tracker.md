# Cold start — branch `entity-tracker`

---

### COLD START — branch `entity-tracker` off `chunk-streaming` (`06562c06e`), 2026-09-05

**A block update now goes to the players holding its chunk and to nobody
else.** Worktree `~/grit/work-track`, remote dir `~/rustmc-track`. Not
merged — the orchestrator gates.

#### THE FINDING: the predicate was ported, tested, and called by nothing

`chunk-streaming` closed with this as its single next item and it was right
about the mechanism. `ChunkHolder.broadcastChanges` resolves its recipients
with `getPlayers(pos, false)` and `ChunkMap.getPlayers`'s whole filter is

```java
ChunkMap.isChunkTracked(player, x, z)
    = player.getChunkTrackingView().contains(x, z)
      && !player.connection.chunkSender.isPending(ChunkPos.pack(x, z))
```

— no distance-to-the-block test, no exclusion of whoever acted.
`world_send::ChunkView::is_chunk_tracked` had been ported with a test and
**nothing called it**, so `ChangedBlocks::flush` went through
`Players::broadcast` and every connected client received every block change
in the world.

The reason it could not be called is structural rather than an oversight:
the view lived on the session thread's stack as `Session.view: &mut
ChunkView`, and the tick's broadcast runs on the clock thread. It is now
`players::Connection.chunk_view: Mutex<ChunkView>`, which is where vanilla
keeps both halves (`ServerPlayer.chunkTrackingView` and
`ServerGamePacketListenerImpl.chunkSender`). **Four production call sites**,
all in the read loop and none under `broadcast::tick_lock`, which is the rule
that keeps it a leaf lock: the clock thread takes the tick lock and then a
view; a connection thread that took a view and then the tick lock would close
the cycle. `Tiles::chunk` answers `None` rather than blocking, so the one
closure that runs under the view lock (`take_batch`'s `available`) is bounded.

#### What else came out of the bytecode

* **The recipient list is per chunk COLUMN, not per packet.** `getPlayers` is
  called once at offsets 111-125 and the section walk re-uses it. Our
  `BTreeMap` key `(x, z, y)` already delivers a column's sections
  contiguously and ascending, so the grouping costs nothing.
  `two_columns_resolve_two_recipient_lists` is the control that it is not
  resolved once for the whole flush.
* **`boundaryOnly = true` is `isChunkOnTrackedBorder`** — tracked **and** at
  least one of the eight neighbours not tracked, centre skipped at offsets
  30-40. Its only caller in the jar is `broadcastChanges`' **light** update at
  offsets 33-47. Ported as `Connection::is_chunk_on_tracked_border`, reached
  by nothing yet because this server sends no `light_update` outside a chunk
  body, and stated as such in `players_for_chunk`'s doc rather than left as a
  surprise. At view distance 4 the centre chunk is 1 block-update recipient
  and 0 light recipients — which is the assertion that the two arms are
  different functions.
* **`getEffectiveRange` ends in `scaledRange` and a dedicated server's
  `scaledRange` is not the identity.** `MinecraftServer
  .getScaledTrackingDistance` is `return d`; `DedicatedServer`'s override is
  `entityBroadcastRangePercentage() * d / 100`. This server is a dedicated one
  and applied no scaling at all. At the default 100 the two agree exactly,
  which is why nothing measured it.
* **`entity-broadcast-range-percentage` is clamped where it is PARSED**, not
  where it is used: `DedicatedServerProperties.<init>` registers it through
  `getMutable(String, Function, Integer)` at offsets 670-686 whose function is
  `Mth.clamp(parseInt(s), 10, 1000)`, default 100 at offset 678.
  `view-distance` is the other way round — unclamped in the file, clamped in
  `ChunkMap.setServerViewDistance`. An operator who writes 5 here gets 10 and
  nothing later corrects it.

#### THE DEVIATION THAT IS STILL OPEN, and it is a cadence and not a value

`ChunkMap.move(player)` (offsets 0-64) runs `updatePlayer` for **every**
tracked entity on **every** move, and `updatePlayers(level.players())` for the
moving player's own tracker. `ChunkMap.tick()`'s `movedPlayers` sweep fires
only on a **section** change. Vanilla therefore re-pairs twice: inside the
move handler and again at the tick. This server has only the tick half
(`tracker::Tracker::tick`), so a pairing arrives at the next tick boundary
rather than inside the handler.

`ChunkMap.tick()` also opens with
`for (ServerPlayer p : playerMap.getAllPlayers()) updateChunkTracking(p)` —
unconditional, per tick, before the entity loop. This server drives
`update_chunk_tracking` from each connection's own read loop, on a chunk-border
crossing plus once at join. Same packets, different thread, different point in
the tick.

Neither is fixed here. Both are the tracker's *cadence*, which is
`ChunkMap`'s shape rather than `TrackedEntity`'s content, and changing it
touches the same joint `chunk-streaming` just moved.

#### The corpus check, and why the corpus could not see this before

**Every existing scenario has both of its clients in the same workspace**, so
they hold the same chunks, so a server that filters and one that does not are
indistinguishable. That is a property of the rig and not of the corpus's size:
110 checks and none of them could have caught it.

`531-a-block-broken-out-of-view-is-not-broadcast` separates them with the only
lever the harness offers. `record::run_one` walks the observer to
`workspace + [0, 0, OBSERVER_OFFSET]` at setup and **every `[[step]]` is
executed by the actor alone** (`actor.act(step, tables)`), so the actor
carries the rig away rather than the observer being moved. It walks to chunk
z 7, digs, walks back and digs beside the observer; the observer's stream
should carry the second and not the first.

Seven chunks and not six: at `view-distance=4` the padded disc reaches
`|dz| = 5` (`3² = 9 < 16`) and stops at 6 (`4² = 16`, not `< 16`), and a check
sitting on the boundary would go red for a `<=` where vanilla has `<` without
saying which defect it had found.

#### KILLED AS NULLS — do not re-run these

* **A `sabotage::Kind` for the recipient filter is the right instrument and is
  not mine to add.** `crates/rustmc-server/src/sabotage.rs` is exactly the
  facility for "what wrong implementation would redden this check", and
  "`isChunkTracked` is not consulted" is precisely a subsystem-shaped wrong
  implementation. Adding a ninth `Kind` ripples into `ci/corpus-controls.py`
  (which picks the armed set per workspace) and `ci/corpus-grades.py`, which
  belong to the gate-instruments lane. The red was established by hand
  instead, with a subset run against a tree with the filter removed.
* **The `isPending` limb cannot be reached by any corpus scenario.** Both
  clients have been standing in their chunks for the whole corpus by the time
  any check runs, so nothing either tracks is still queued. It is covered by
  `broadcast::a_change_in_a_pending_chunk_is_withheld_and_not_deferred`, which
  also asserts the withheld change is **dropped** rather than replayed when the
  chunk lands — `broadcastChanges` nulls `changedBlocksPerSection[j]` at
  offsets 156-163 and only then tests `list.isEmpty()` at 164-173.
* **`ClientboundLightUpdatePacket` is not portable here yet.** It is the only
  consumer of `getPlayers(pos, true)` and this server has no standalone light
  engine broadcast; the light in a `level_chunk_with_light` body is written by
  the chunk encoder. The predicate is ported and the packet is not.

#### The hazard this branch created and closed, worth one line

A test that drained the chunk sender with `while v.pending_len() > 0 { take_batch(...) }`
**hangs for ever**: `PlayerChunkSender.maxUnacknowledgedBatches` is 1 until the
client's first `chunk_batch_received`, so the second call returns `Vec::new()`
with the queue non-empty. It took out a whole `cargo test` run
(`a_change_in_a_pending_chunk_is_withheld_and_not_deferred has been running for
over 60 seconds`). Both drain loops are now bounded at 64 iterations, call
`on_batch_received` after each batch as a real client does, and assert the queue
reached zero.

#### The second fidelity fix, and why it moved the join

`updatePlayer`'s third limb is `ChunkMap.isChunkTracked` and that is **both**
halves. It called `broadcast::chunk_is_tracked` — the geometry alone — for the
same reason `flush` had no filter: the pending set was unreachable. With the
view on `Connection` the limb is whole, and an entity in a chunk the client has
been promised and not yet sent is no longer paired.

That makes `addEntity`'s order load-bearing, and it was wrong:

```text
   99-108  tracked.updatePlayers(level.players())      show the newcomer
  124-128  updatePlayerStatus(player, true)            open its view
  131-186  sweep entityMap, updatePlayer(newcomer)     show it the world
```

This server did the first and the third and left the middle to
`keep_alive_loop`. Against a view still holding `ChunkTrackingView.EMPTY` the
sweep pairs nothing, so the newcomer would have seen an empty world and
corrected itself only at its next section change. It is now where vanilla has
it, with the spawn tile's `mark_sent` moved above it so those chunks are
delivered rather than queued, and a `debug_assert!` that it writes no bytes.

#### THE SINGLE NEXT ITEM

**Port `ChunkMap.move`'s per-move `updatePlayer` sweep**, from
`keep_alive_loop`'s move arm, and measure whether it moves any corpus
observation. The result that closes it: either a check whose verdict changes
(the pairing latency is observable) or a measured statement that it does not
(the tick boundary is inside every scenario's settle window), written down so
the next agent does not re-open it. `530` and `531` are the two checks in
range.
