# COLD START — branch `world-send-chunk-block-entities`

### COLD START — branch `world-send-chunk-block-entities`, 2026-08-12

**The item: make `788-a-chest-is-in-its-chunks-payload-when-the-chunk-returns`
go from `absent` to `match`, or say honestly why it cannot.
`docs/PARITY-ROADMAP.md` §32, stacked on `trace-chunk-block-entities`
(`f171f924`, §31).**

### What moved

```text
  world.rs `World::chunk`     block_entities: Vec::new()  ->  read from
                               crate::blockentity::BlockEntities::shared()
  new                         blockentity::chunk_payload_tag_bytes
                               blockentity::chunk_payload_entry
  new tests                   4, all pure functions, blockentity.rs
  T0 (ci/t0-remote.sh)        GREEN — fmt, clippy -D warnings, check,
                               787/787 workspace --lib --bins, session_joint,
                               plugin_storage/plugin_tick_lock/random_tick_pos
  787's gameplay verdict      NOT YET measured by this branch's own run —
                               ci/t2-gameplay.sh launched, box-wide corpus
                               lock had a gate holder + 2 waiters (71m, 63m)
                               queued ahead of it at launch time
```

### The brief named the wrong file, and the tree said so

Briefed site: `crates/rustmc-server/src/world_send.rs`'s `flat_chunk`, which
does carry `block_entities: Vec::new()`. Grepped rather than assumed:
`flat_chunk` is called from nowhere but its own two unit tests. The chunk a
gameplay-corpus scenario is actually sent comes from `world.rs`'s
`World::chunk`, reached through `Session::chunk_at` -> the closure
`status.rs` builds in production -> `Tiles::chunk` (`tiles.rs`). A third
`Vec::new()` lives in `terrain.rs`, on the no-extract fallback `status.rs`
only takes when `worldgen_assembly::have_extract()` is false — dead for this
measurement, since the corpus runs with the extract present (verified:
`oracle-work/vanilla-26.2` is linked in and `have_extract()`'s file exists).
Fixed the one site the corpus exercises. Full account, the exact bytecode
(`ClientboundLevelChunkPacketData$BlockEntityInfo`, `FriendlyByteBuf.writeNbt`,
`NbtIo.writeAnyTag`) and the wire-format derivation: `docs/PARITY-ROADMAP.md`
§32.1.

### What the fix is

`world.rs`'s `World::chunk` now builds `block_entities` from
`crate::blockentity::BlockEntities::shared().in_chunk(chunk_x, chunk_z)` —
the same store `set_block` already populates on placement and the same
`in_chunk` the save path already used, so no new state was needed. Two new
functions in `blockentity.rs` do the encoding:

* `chunk_payload_tag_bytes(tag: Option<&Nbt>)` — `None` -> the single byte
  `0x00` (a null tag on this channel is `EndTag.INSTANCE`, and
  `NbtIo.writeAnyTag` on `TAG_End` writes one byte and returns); `Some(tag)`
  -> the same nameless encoding `block_entity_data_body` already used,
  factored out as `nameless_nbt_bytes` so both call the one implementation.
* `chunk_payload_entry(pos, be, type_id)` — `packedXZ = (x&15)<<4 | (z&15)`,
  `y` as `i16`, `type_id` passed in by the caller (the same seam
  `world_send::StateId` documents for block states), tag via the function
  above.

`ProtocolIds::block_entity_type` already existed and already maps
`minecraft:chest -> 1` off the generated `data/vanilla/protocol_ids.json`
(confirmed against this branch's own `gen-gameplay-tables.sh` run, not
recalled). A missing protocol-id table or a kind with no registry entry
skips that one block entity rather than failing the whole chunk — the same
`None`-means-something-real idiom every other optional table in this file
uses.

### The empty-chunk case

Unchanged, and checked rather than assumed: `in_chunk` on a chunk with
nothing in the store returns an empty `Vec`, `.collect()` over an empty
iterator is `Vec::new()`, so every chunk that carried no block entity before
this branch carries the identical empty list after it. The T0 workspace
suite (787 `--lib --bins` tests, a different and larger count than this
crate's own 552 — that's `--workspace`, not `-p rustmc-server`) is all green
and none of it constructs a `World` with anything in the block-entity store,
so every chunk any of those tests encodes is exactly this path.
`docs/PARITY-ROADMAP.md` §32.3.

### What was witnessed, and what could not be

The two new pure-function tests for the null-tag rule were reverted
(`vec![0]` -> `Vec::new()` in `chunk_payload_tag_bytes`) and re-run before
being restored:

```text
assertion `left == right` failed: a chest's getUpdateTag() is empty
  left: []
 right: [0]
```

Genuinely witnessed failing, genuinely witnessed passing again after the
revert was reverted.

**`World::chunk`'s own wiring — the actual site named in this item — cannot
be witnessed failing or passing by `cargo test`, and this is not something
this branch introduced.** `crate::protocol_ids::shared()` reads
`data/vanilla/protocol_ids.json` relative to the process's *working
directory*. Checked empirically with a throwaway probe test (removed before
commit, not left behind): `cargo test -p rustmc-server --lib` runs with cwd
`crates/rustmc-server/`, not the repo root, so the lookup fails every time
and `shared()` returns `None` regardless of whether `World::chunk` is fixed
or still `Vec::new()` — both produce an empty list under any `cargo test`
invocation, so no unit test can tell them apart. `broadcast_block_entity_if_needed`
(`broadcast.rs`) has carried the identical dependency since before this
branch and is likewise untested directly, for the same reason — this is an
existing property of the crate's test harness, not a new hole. The
discriminating instrument is `787` itself, under `ci/t2-gameplay.sh`, which
starts `rustmc-server` as a subprocess from the repo root where the lookup
resolves. `docs/PARITY-ROADMAP.md` §32.4.

### Nulls — killed with numbers, do not re-run these

* **"the brief's file is the site."** `flat_chunk` (`world_send.rs`) is
  called from nothing but its own two unit tests — grepped for `flat_chunk`
  across `crates/`, one definition and two test call sites, zero production
  callers.
* **"a unit test can prove `World::chunk` wires the block-entity store in
  under `cargo test`."** It cannot: `crate::protocol_ids::shared()`'s cwd
  dependency makes the list empty under every test invocation regardless of
  which code is live. See above.
* **Fixing `terrain.rs`'s and `world_send.rs`'s own `Vec::new()`s.** Neither
  is on the path `787` or any other gameplay scenario measures —
  `terrain.rs`'s is the no-extract fallback, dead while the extract is
  present; `world_send.rs`'s is test-only. Touching either would be
  unverified scope for this item.

### The single next item, and the result that closes it

**Read `787`'s verdict off this branch's own `ci/t2-gameplay.sh` run once the
box-wide corpus lock frees up**, and if it is `match`, move its row in
`ci/checks/gameplay-baseline.toml` — not before, and not on any other
branch's run. The run was launched at commit time; check
`oracle-work/gameplay/rustmc.json` and the gate log for the verdict, and
report the corpus size the run itself prints (moving on four other branches
adding scenarios in parallel, per this item's brief — not comparable to
§31's 93).
