# COLD START — branch `chunk-send-path`

### COLD START — branch `chunk-send-path`, 2026-08-18

**The area: the chunk-send path end to end — what a client is told when a
chunk loads, in what order, with which block entities and which light, against
what we send.** `docs/PARITY-ROADMAP.md` §70, §70.1, §70.2. Carries the ten
commits of `world-send-chunk-block-entities` rebased onto main 2182.

### The branches, and what the family actually was

```text
  world-send-chunk-block-entities   10 commits, ALL REAL      -> carried here
  trace-chunk-block-entities         8 commits, 7 duplicates  -> SUBSUMED
  chunk-order-residual              15 by rev-list, 0 by cherry -> NOTHING TO LAND
  gameplay-block-entities            0 -> already on main
```

`ci/unlanded.sh world-send-chunk-block-entities trace-chunk-block-entities`
reads **18 shas, 11 distinct patch-ids**. Seven of `trace`'s eight patches are
byte-identical in patch-id to `world-send`'s; the eighth is
`474f41742 ci: sweep corpus-falsifiable for the renumbered 787 check`, which
`world-send`'s own `d968c4cce` supersedes because that branch renumbered the
check again, 787 -> 788. **Landing this branch lands the family.**
`chunk-order-residual` is the `rev-list` trap in its pure form: 15 commits, all
fifteen already on main under different shas, `git cherry` says 0.

### The divergence

**`ChunkView::recenter` sorted the send list on Chebyshev, `max(|dx|,|dz|)`.
Vanilla's `PlayerChunkSender.collectChunksToSend` sorts on
`ChunkPos.distanceSquared`, `dx*dx + dz*dz`.** Fixed.

Charging only pairs whose order **both** metrics determine, so no tie-break
contributes a single count:

```text
  radius   chunks   metric-determined pairs   inversions
     2         25          152                     0
     3         49          752                     0
     4         81        2,320                    48   2.07%
     8        289       35,104                 1,680   4.79%
    10        441       84,696                 4,784   5.65%
    12        625      174,096                11,056   6.35%
    16      1,089      544,128                39,168   7.20%
```

The metrics are **identical out to radius 3**, and `ci/t2-gameplay.sh` runs at
`--view-distance 4` — the first radius that separates them at all.

**NOT a divergence, do not re-open:** the intra-band tie-break. Vanilla's
comparator is fed from a `LongOpenHashSet`, so within one distance band its
order is hash iteration order — unspecified, not reproducible outside the JVM.
Positional-mismatch counts against one vanilla ordering give 90.1% at radius 4
and 99.0% at radius 16 and are **a distribution, not a divergence**, on the
same grounds as the 551 retraction. Ours stays `(z, x)`.

### The difference no existing check could have seen, and why

**Six of the twelve kinds in `cited-audits.toml`'s
`projection-unrecorded-kinds-20260818` are this path** — that set is exactly
`trace::is_housekeeping`, and its chunk-send members are every chunk-send kind
there is. Half the corpus's kind-level blind spot is one subsystem.

**But the binding filter is a different one, and this is the part to carry
forward.** Un-dropping `LEVEL_CHUNK_WITH_LIGHT` (which `world-send` does) is
necessary and not sufficient: `trace::observe` records a chunk packet only when
its block-entity list is non-empty, and `788`'s own measurement is **132 chunk
packets inside the window, of which 1 non-empty**. Order is a statement about a
sequence; 131 of 132 are dropped, so the recorded projection of a chunk window
is a single packet and **no corpus check can distinguish any send order from
any other**. That filter is correct and must stay — `788`'s file records that
counting empty chunks makes a scenario's counts a function of which batch
landed inside the settle timer, which is harness scheduling and not server
behaviour.

**And the join send is outside every window regardless.** `record::run_one`
discards the setup recording and `Session::settle` waits for quiet with chunk
streaming deliberately not counted as liveness, so the batch a *joining* client
gets has finished before any window opens. `788` gets a chunk into a window
only by walking past `view-distance=4` and back — a re-send, and the only door.

### Nulls — killed explicitly, do not re-run

* **`observe_from` does not bite this subsystem.** `788` is
  `observe_from = "actor"` and the actor is itself the client the chunk is sent
  to. Checked rather than assumed, because "a chunk is sent *to* a client" is
  exactly the shape that usually does depend on the discarded one.
* **Heightmaps match.** Vanilla filters on `Heightmap.Types::sendToClient` =
  `usage == Usage.CLIENT`, true for ordinals 1, 4, 5 (`WORLD_SURFACE`,
  `MOTION_BLOCKING`, `MOTION_BLOCKING_NO_LEAVES`). `world.rs`'s
  `CLIENT_HEIGHTMAPS` is those three in that order. Nothing to do.
* **`Terrain::chunk` and `flat_chunk` sending no block entities is NOT a
  defect.** First reading called it one; it is wrong. `flat_chunk` is stone and
  air, reached only from its own two unit tests. `Terrain::chunk` is the
  noise-stage fallback with `beardifier: None` — the live server generates no
  structures, so there is no block entity to send. Both sites now carry a
  comment naming the precondition.
* **One packet per chunk.** `sendChunk` sends only
  `ClientboundLevelChunkWithLightPacket`; there is no separate per-chunk
  `LIGHT_UPDATE` to port.

### What is still open, with the result that closes each

1. **The pacing port is unwired, and wiring it is a server-loop item, not a
   chunk-send one.** `chunk_pace.rs` is a faithful, tested port of
   `PlayerChunkSender`'s arithmetic. At `--view-distance 4`, with the harness's
   own `DESIRED_CHUNKS_PER_TICK = 0x42800000 = 64.0f`, vanilla's join is
   **three batches of 9, 64 and 8**; ours is **one of 81**. It is unwired
   because `stream_chunks` runs on a chunk-border crossing and nowhere else —
   there is no per-tick chunk pump, and throttling without one would strand the
   excluded chunks until the player crossed another border. **Closes when a
   per-tick pump exists and `CHUNK_BATCH_FINISHED` carries 9, 64, 8.**
   Deliberately NOT done: decoding `CHUNK_BATCH_RECEIVED` into the pacer while
   the send loop ignores the allowance — that is a live value in a field no
   behaviour reads, the `ACCEPT_TELEPORTATION` defect.
2. **`788`'s baseline row still reads `absent` and is stale.** §32 fixed
   `World::chunk` to build the list; no corpus run has measured it since. This
   branch launches exactly one. **Closes when that run reports `match`, or
   reports something else and says why.**
3. **Light classifies 0 of 26 sections** where vanilla's
   `ClientboundLightUpdatePacketData` puts every one of `getLightSectionCount()`
   into a data mask or an explicit empty mask. A documented deferral, restated
   exactly. **Closes with a light engine, not with a payload change.**
4. **`player.rs` carries its own private `mod ids`** rather than
   `rustmc_proto::ids::play::sb` — which is why a `grep` for `sb::` finds 21
   handled kinds and misses `MOVE_PLAYER_*`. The two tables agree today at
   30/31/32/33/43, checked. Nothing guards that they continue to.

### One figure of my own to correct

`00b4e5500`'s message says `chunk_pace.rs` carries **"Ten unit tests"**. It
carries **nine** — `grep -c '#\[test\]' crates/rustmc-server/src/chunk_pace.rs`.
A commit message cannot be edited in place once pushed, so the correction lives
here, which is the record the next agent reads. The nine are: the join case,
the never-acknowledging client, the reopened allowance, the ceiling that does
not bank, fewer-pending-than-quota, both clamps and NaN, the sub-one-per-tick
rate, the floor at zero, and the memory-connection branch.

Small, and written down anyway, because "a number in a file is a number every
agent reads first and nobody re-checks" is this project's own rule and it does
not stop applying at commit messages.

### The new instrument

`ci/chunk-send-unrecorded.py` + `ci/checks/chunk-send-unrecorded.toml`, wired
into `ci/t0.sh` selftest-first. The kind-level companion to
`projection-drops.toml`'s field-level ledger, covering what
`corpus-falsifiable.py` cannot: when the projection filters a kind before the
trace is written, no mutation of the subject moves anything, so the scenario
reads tier 1 — the same signature as a scenario that measures nothing. Those
two are indistinguishable to that gate **by construction**.

`ci/chunk-send-unrecorded-falsify.sh`: 7 cases, 7 pass, 1.45 s. Case 1 is a
positive control. **Case 2 reconstructs the real shipped instance** — putting
`LEVEL_CHUNK_WITH_LIGHT` back into `is_housekeeping` exactly as `origin/main`
f82b2ebbf carries it, the projection §31 and §32 were both written against.
