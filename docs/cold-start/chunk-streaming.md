# Cold start — branch `chunk-streaming`

---

### COLD START — branch `chunk-streaming` off `main` (`b6237f29c`), 2026-09-05

**A client is now sent the chunks vanilla sends, in vanilla's order, at
vanilla's rate.** Worktree `~/grit/work-chunks`, remote dir `~/rustmc-chunks`.
Not merged — the orchestrator gates.

The one number: at `--view-distance 4`, `ci/chunk-stream-probe.sh` measures
this server sending a conforming client **117 chunks in 6 batches**, and a
client that never sends `chunk_batch_received` **31 in 2**. 117 is exactly
what real vanilla at `view-distance=4` was measured sending on 2026-08-03
(`ci/chunk-batch-probe.sh`'s header, taken a month before this port existed).
Before this branch the same probe read **25 and 25**.

#### What was ported, and from what

Four pure modules, all from `javap -p -c -constants` on
`oracle-work/vanilla-26.2/versions/26.2/server-26.2.jar`:

* **`crates/rustmc-server/src/trackingview.rs`** — `ChunkTrackingView`,
  `$Positioned`, `$1` and `difference`.
* **`crates/rustmc-server/src/chunklevel.rs`** — `ChunkLevel` and
  `FullChunkStatus`.
* **`crates/rustmc-server/src/chunksender.rs`** — `PlayerChunkSender`.
* **`crates/rustmc-server/src/tickets.rs`** — `TicketType`, the nine
  registered types, and the level arithmetic out of `DistanceManager`,
  `FixedPlayerDistanceChunkTracker`, `PlayerTicketTracker`,
  `LoadingChunkTracker`, `SimulationChunkTracker` and `ChunkTracker`.

`crates/rustmc-server/src/world_send.rs`'s `ChunkView` is now the wiring of
the first and third, and `crates/rustmc-server/src/properties.rs` is the
`server.properties` reader the settings needed.

#### THE FINDING: the tracked set is a padded disc and this server had a square

`ChunkTrackingView.isWithinDistance` is

```java
int pad = includeOuter ? 2 : 1;
long dx = Math.max(0, Math.abs(x - cx) - pad);
long dz = Math.max(0, Math.abs(z - cz) - pad);
return dx * dx + dz * dz < (long) (viewDistance * viewDistance);
```

over a bounding box of `viewDistance + 1` — and the pad and the box are
different numbers, so the box is what bounds the disc rather than the other
way round. `ChunkMap.applyChunkTrackingView` drives the entire send-and-forget
decision through `difference` over two of these, so this is the test for
**which chunks a client gets**, not merely for entity tracking.

`world_send::ChunkView` walked `dx, dz in -radius..=radius`. The two sets are
not nested in either direction: at view distance 10 the square holds
`(10, 10)` and the disc does not (`8² + 8² = 128`, not `< 100`), and the disc
holds `(11, 0)` and the square does not (`9² = 81 < 100`). Sizes: **441
square, 473 disc** at 10; **81 and 117** at 4. A one-chunk step moves 21
chunks under the square and **23** under the disc.

**`crates/rustmc-server/src/broadcast.rs`'s `chunk_is_tracked` had already
ported this function correctly for the entity tracker, and its doc comment
said in so many words that a square was *correct* for deciding which chunks
to send** — "it is about which chunks to send, which is a different
question". It is the same question and the same call. That paragraph is
corrected in the first commit; the stale-caveat class again.

#### THE SECOND FINDING: `MAX_LEVEL` is 44

`ChunkLevel.<clinit>` reads `RADIUS_AROUND_FULL_CHUNK` off the whole
generation pyramid and `MAX_LEVEL = 33 + it`. That is a recipe, not a number,
so `ci/chunklevel-golden.sh` runs vanilla's own initialiser and dumps it:

```
FULL=33 BLOCK_TICKING=32 ENTITY_TICKING=31
RADIUS_AROUND_FULL_CHUNK=11 MAX_LEVEL=44
```

Fixture `crates/rustmc-server/golden/chunk_level.json`, 19 levels, identical
across two runs. A port that assumed `MAX_LEVEL = FULL_CHUNK_LEVEL` — which
it was before the pyramid existed — would unload an eleven-chunk ring vanilla
keeps, and **every chunk inside the guess would still be correct**, so no
value test could see it.

#### What a client receives now

* **On join** — unchanged in shape. `enter_world` still writes the login
  packet, `game_event 13`, one `set_chunk_cache_center`, the spawn square as
  one unbatched blob, and the teleport. The view is then told about that
  square (`ChunkView::mark_sent`), so the first `update_tracking` queues only
  the rest of the disc and writes **no second centre packet**.
* **Immediately after** — `keep_alive_loop` calls `update_chunk_tracking`
  once before its first read. This is `ChunkMap.updatePlayerStatus(player,
  true)`, and **it was missing**: the sender was queued only on a chunk-border
  crossing, so a player who joined and stood still got the join blob and
  nothing else for ever. That is the 25-and-25 the probe read.
* **Once per server tick** — `PlayerChunkSender.sendNextChunks`:
  `chunk_batch_start`, up to `floor(batchQuota)` chunks ordered by
  `ChunkPos.distanceSquared` ascending, `chunk_batch_finished(n)`. Gated on
  `GameClock::ticks` and not on read-loop iterations, because the quota
  arithmetic is per call and that loop spins faster than 20 Hz.
* **On a move across a chunk border** — `set_chunk_cache_center` **only if
  the centre moved** (it was sent on every crossing), then the
  `forget_level_chunk` packets in `difference` order (x-major, both axes
  ascending), then nothing; arrivals are queued.
* **`chunk_batch_received`** is now handled. It was in
  `rustmc-proto::ids::play::sb` and read by nothing. It is what raises
  `maxUnacknowledgedBatches` from **1** to 10, so ignoring it does not skip a
  throttle — it delivers one batch and stops.

#### Settings

Nothing in this workspace opened `server.properties` before this branch.
`view-distance` arrived only as an argv flag and `simulation-distance`
existed nowhere: `play::JoinInfo::default`'s literal 10 went into every
`ClientboundLoginPacket`. `properties.rs` reads both keys (argv > file >
default), names a non-integer value on stderr rather than defaulting
silently, and `StatusInfo::simulation_distance` carries it.

`Mth.clamp(int, int, int)` is `min(max(v, min), max)` — read off
`net/minecraft/util/Mth`, and the order matters: the server ceiling beats the
floor of two. `status::player_view_distance` was already right and now
delegates to `tickets::player_view_distance` so there is one copy.

#### The three radii one player produces, which are three different numbers

At `view-distance = simulation-distance = 10`, from
`tickets::tests::one_player_produces_three_different_radii`:

```
entity ticking   Chebyshev 10    (simulation distance)
block ticking    Chebyshev 11    (+1: isBlockTicking is level <= 32)
loaded           Chebyshev 23    (PLAYER_LOADING at 31 + 13 to reach 44)
tracked/sent     the padded disc, 473 chunks
```

A server driving all of these off one setting is wrong in three directions at
once. `tickets::entity_ticking_radius` and `block_ticking_radius` derive the
first two from the level arithmetic rather than writing them down.

#### KILLED AS NULLS — do not re-run these

* **`ClientboundSetChunkCacheRadiusPacket` and
  `ClientboundSetSimulationDistancePacket` are not sent on join.** The brief
  said they were. `PlayerList.placeNewPlayer` sends login, change_difficulty,
  player_abilities, set_held_slot and update_recipes and neither of these;
  the only senders in the whole jar are `PlayerList.setViewDistance` and
  `setSimulationDistance`, which `broadcastAll` on an **operator changing the
  property**. A joining client learns both out of `ClientboundLoginPacket`,
  which this server already fills. Their ids are 95 and 111
  (`generated/reports/packets.json`); both bodies are a single VarInt. They
  are deliberately **not** added to `ids.rs`, because this server has no
  command that changes either setting and a defined id nothing sends is a
  dead field.
* **The gameplay corpus cannot observe this area at all.**
  `tools/oracle-runner/src/trace::is_housekeeping` drops
  `level_chunk_with_light`, `chunk_batch_start`, `chunk_batch_finished`,
  `forget_level_chunk` **and** `set_chunk_cache_center` before any decoder
  arm runs, so no `ci/checks/gameplay/*.toml` scenario can reach tier 2 on
  chunk streaming and none was written. Refused rather than worked around:
  the projection belongs to the recorder lane and widening it changes all 110
  recordings, not one. `ci/chunk-stream-probe.sh` is what could be built
  without it, and it asserts four values with a stated other-value each.
* **`ChunkMap.processUnloads` and `scheduleUnload` are not portable here.**
  This server has no per-chunk `ChunkHolder` and no `updatingChunkMap`; its
  unit of residency is a **tile**, retired on a timer by
  `Tiles::retain_around`, and the save-on-unload path is
  `origin/persistence-2`'s (whose cold start records `RETIRE_AFTER_SECS` and
  the block-entity eviction, and corrects the "no chunk unload exists"
  premise). Porting the unload queue means inventing the holder map first.
  What is stated instead: the predicate is
  `ChunkLevel::is_loaded(loading_level)`, i.e. Chebyshev `view_distance + 13`
  from a player, where `status::retire_unwatched_tiles` uses
  `max(conn.view_distance, server_view_distance)` — **thirteen rings short**.
* **`PlayerChunkSender`'s tie order is ported (landing, 24 Sep).**
  `pendingChunks` is a `LongOpenHashSet`; both branches of
  `collectChunksToSend` resolve chunks at equal `distanceSquared` in its
  spliterator order (zero key first, then slots ascending), the throttled one
  through Guava's `TopKSelector`. Both are ported
  (`chunksender::long_open_hash_set`, `chunksender::top_k_selector`) and
  `every_collect_matches_the_jar` replays 5,167 collects recorded from the
  jar's own method by `ci/chunk-send-ties-golden.sh`. The `(x, z)` tie-break
  this branch had and `main`'s `(z, x)` are each red on it (2,361 and 2,369
  collects).
* **`randomtick::CHUNK_RADIUS` is still 2** where vanilla's is the simulation
  distance — 25 chunks against 441 at the default. The setting it needed did
  not exist until this branch, and it still is not what closes the deviation:
  vanilla's 441 chunks are cheap because
  `LevelChunkSection.isRandomlyTicking`'s two counters skip almost every
  section before any draw, and this server keeps no such counters. The gap is
  now a live number in a test rather than a paragraph.

#### THE SINGLE NEXT ITEM

**Wire `isChunkTracked`'s pending half into the broadcast recipient list.**
`ChunkMap.getPlayers(pos, false)` filters on
`view.contains(x, z) && !chunkSender.isPending(pack(x, z))`, and that is the
**only** filter on `ChunkHolder.broadcastChanges`'s recipients: a block change
in a chunk a client has not yet received is *withheld*, not deferred. That
predicate is ported — `world_send::ChunkView::is_chunk_tracked`, with a test —
and **nothing calls it**, because `broadcast::ChangedBlocks::flush` sends
through `players.broadcast`, which has no view filter of any kind and cannot
reach a `ChunkView`: the view lives on the session thread's stack behind
`Session.view: &mut ChunkView`.

What closes it: `players::Connection` holds the view (an `Arc<Mutex<_>>` or
equivalent), `broadcast::flush` filters recipients through
`is_chunk_tracked(chunk_of(pos))`, and a corpus scenario in which the
observer's chunk is still queued sees the actor's placement **not** arrive —
which, unlike everything else in this area, the existing projection *can*
observe, because `block_update` is not housekeeping. Cost to price first: one
mutex acquisition per player per broadcast packet.

#### The vanilla capture

`ci/chunk-order-golden.sh` boots a real vanilla 26.2 superflat server, joins
it with `ci/regdump.py`'s `CHUNK_DUMP`, and records every
`level_chunk_with_light` position in arrival order into
`crates/rustmc-server/golden/chunk_send_order.json`. Positions only, no block
data, which is what makes it committable where a region file is not.

```
captured 117 chunks around [-1, -1] at view-distance=4
padded disc predicts 117, a Chebyshev square predicts 81
10 ascending runs (batches), lengths [35, 9, 2, 3, 13, 16, 3, 12, 14, 10]
```

`crates/rustmc-server/tests/golden_chunk_send_order.rs` gates it, and it is in
T0 (`t0-wiring`, named on `ci/t0.sh`'s selector line — it needs no extract, so
`golden-vectors`' exclusion does not apply). The red-when-broken control is
the pad: narrowing it from 2 to 1 predicts 77 and the test fails naming all 40
chunks vanilla sent that the narrowed disc does not hold.

**Vanilla's stream is not globally monotone in `distanceSquared`** and a
sequence assertion would be a check that fails on vanilla's own variation:
`ChunkMap.onChunkReadyToSend` queues a chunk when it finishes generating, so a
nearer chunk can enter the queue after a farther one has gone out, and a
descent is a batch boundary. Two captures of the same world type gave 11 runs
and 10 runs with different spawn centres. Asserted instead: the centre goes
first, every maximal run is ascending, fewer than 20 runs, longest run at
least 5.

#### Gate

`ci/t0-remote.sh` with `RUSTMC_REMOTE_DIR=rustmc-chunks`. Two RED runs, both
worth knowing about:

* `ci/entity-id-allocator.py` — "our allocator was not found … the Rust reader
  is dead" — on a file this branch does not touch. The cause was **two
  `t0-remote.sh` runs rsyncing `--delete` into the same remote dir at once**,
  both mine. Green in both trees on re-run. Do not launch a second
  `t0-remote.sh` against a remote dir that already has one.
* `ci/readiness-falsify.sh` — **a readiness proof's variable names are
  load-bearing.** That script `eval`s each registered readiness line against
  four real not-ready servers, supplying `WORK`, `OUR_PORT`, `RUSTMC_PID` and
  `RUSTMC_READY`. `ci/chunk-stream-probe.sh` spelled the port `PORT` and the
  flag `READY`, so under `set -u` the eval died with `PORT: unbound variable` —
  the proof was not transcribed wrong, it was **not exercised at all**, and
  the harness reported that as the script's failure. Renamed, and the reason
  is written at the loop so the next site does not rediscover it.
