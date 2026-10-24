# Cold start — branch `dimensions`

Every `### COLD START` entry ever written for this branch, appended in order.

---

### COLD START — branch `dimensions` off `origin/world-systems-2` (`14df4d84b`), 2026-09-05

**The nether and the End served live: generated, encoded, and travelled to.**
Worktree `~/grit/work-dims`, remote dir `~/rustmc-dims` on the **Threadripper**.
Not merged — the orchestrator gates.

#### WHAT A CLIENT CAN NOW STAND IN, AND THE BYTE PROOF

All three dimensions generate, encode and serve through the **live** chunk
source — `Tiles::prepare_in` then `Tiles::chunk`, which is exactly the pair
`status.rs`'s `chunk_at` closure calls for every chunk a connected client is
sent.

    a served chunk vs the dump's canvas   0 of 196,608 blocks differ
      the nether     16 sections   0 of 65,536
      the End        16 sections   0 of 65,536
      the overworld  24 sections   0 of 98,304

Seed 20260722, chunk (0,0), whole column, `World::chunk` -> `encode` ->
`decode`, against the canvas `worldgen_assembly::generate::generate_nether` /
`_end` / `generate` produced — the same three calls `tools/worldgen-dump`
makes. `crates/rustmc-server/tests/served_dimensions.rs`, 5 tests, 62.3 s,
Threadripper `rustmc-dims`, debug profile.

**So `docs/PARITY.md`'s nether and End figures, all of which were taken on the
dump, are about the world this server serves.** That was the open question and
it is the only reason the number above is worth taking: the generator is
shared by construction, and what is under test is everything between it and
the wire — `World::generate_in`'s arm selection, the dimension type's section
count, the `min_y` offset, the palettes, the biome container. Every one of
those can disagree with the dump while the generator agrees with itself. The
reference arm is chosen by an explicit `match` in the **test**, so a
`generate_in` that mapped `Nether` onto the overworld arm goes red.

#### THE REGISTRY INDICES ARE NOT THE ONES A READING GIVES

`CommonPlayerSpawnInfo.write` opens with `DimensionType.STREAM_CODEC.encode`,
a **varint index** into `minecraft:dimension_type` as the client received it.
That registry has **four** entries:

      0  minecraft:overworld
      1  minecraft:overworld_caves      no level uses it; it is still numbered
      2  minecraft:the_end
      3  minecraft:the_nether

So `the_nether` is **3** and `the_end` is **2**. Guessing 1 and 2 renders the
nether with the End's sky. Checked against two independent sources that also
check each other: Mojang's datagen extract for the six dimension-type fields,
and `data/vanilla/registries.bin` — the bytes the configuration phase already
sends — for the order and the same fields out of its NBT.

`sea_level` is **not** in the dimension type. `createCommonSpawnInfo` fills it
from `ServerLevel.getSeaLevel()` = `chunkSource.getGenerator().getSeaLevel()`,
i.e. the *noise settings'* `sea_level` — 63 / 32 / 0. A server reading it from
the dimension type would find nothing and send 0 for all three.

**The End has `has_skylight: true`.** Its darkness is `sky_light_factor: 0.0`
and `ambient_light: 0.25` in the entry's `attributes`; only the nether has no
skylight.

#### TRAVEL, AND ITS CAPTURES

`world_send::change_dimension` sends what a travelling client receives, in
`ServerPlayer.teleport(TeleportTransition)`'s order. **It is not the join
sequence rearranged** and two positions are the opposite way round:

      16  game_event NO_RESPAWN_BLOCK_AVAILABLE   only if missingRespawnBlock
     115  respawn(createCommonSpawnInfo(newLevel), 3)      <- FIRST
     135  change_difficulty
     172  sendPlayerPermissionLevel   -> entity_event
     253  connection.teleport(PositionMoveRotation.of(t), t.relatives())
     272  connection.resetPosition()
     296  player_abilities
     314  PlayerList.sendLevelInfo    -> the world border, LAST
     321  sendAllPlayerInfo
     327  sendActivePlayerEffects
     334  postTeleportTransition.onTransition

At join `placeNewPlayer` calls `sendLevelInfo` *first*. A wiring that reused
the join order puts the border ahead of the respawn, and a client applies a
border to the level it thinks it is in. `dataToKeep` is **3**
(`iconst_3` at offset 128, `KEEP_ALL_DATA`) on every dimension change.

Captured: `tests/dimension_travel_wire.rs`, 4 tests, 0.07 s, **T0** (it needs
no fixture) — 9 frame ids in that order for both targets, 3 respawn bodies
compared whole and required pairwise distinct, 9 chunks decoded at their own
dimension's section count, and the End's arrival read back out of the
teleport. Its controls are the pairs: three dimensions must give three
*different* respawn bodies, and a 16-section chunk must **fail** to decode as
24.

`ci/spiral-golden.sh` captures vanilla's own `BlockPos.spiralAround` —
`golden/spiral_around.json`, four walks, **60 positions, 0 differ**. That is
the traversal `PortalForcer.createPortal` searches in and the reason it
returns a *particular* portal.

#### WHAT WAS DERIVED FROM BYTECODE AND WOULD HAVE BEEN WRONG

1. **`spiralAround`'s first position is the centre.** The cursor starts one
   step along `second` and the first move is `directions[(-1 + 4) % 4]` =
   `second.getOpposite()`, which undoes it. Leg sizes are `leg / 2 + 1` over
   leg values **0..=4r inclusive** — 1, 9, 25 at r = 0, 1, 2. The half-open
   reading gives 1, 7, 21, and it is what this branch's own validator said
   until the capture refused it.
2. **`createPortal` keeps TWO candidates.** `best` needs the frame to stand at
   lateral −1, 0 **and** +1; `fallback` needs only 0, is recorded only while
   `best` is unset, and is used only if the whole spiral ended with no `best`.
3. **`createPortal` has exactly one `Optional.empty()`** — `maxY - 9 <
   max(minY + 1, 70)`. On any world 79+ tall a portal is *always* built: a
   search that finds no site carves a 3x2x4 box of air over a 3x2 obsidian
   floor and builds on that.
4. **The gap rule.** `gap = yTop - y` after sliding to the bottom of a run of
   replaceable blocks; `gap > 0 && gap < 3` continues, so a portal never lands
   in a two-block crawlspace.
5. **A player entering the End lands at (100.5, 49.0, 0.5) and anything else
   at y 50.** `EndPortalBlock.getPortalDestination` offset 142 subtracts
   `(0, 1, 0)` only when the entity `instanceof ServerPlayer`. Yaw is
   `Direction.WEST.toYRot()` = 90 and absolute; pitch is relative.
6. **`createEndPlatform` is z-major.** `move(III)` takes `(dx, dy, dz)` and
   the bytecode pushes local slots 5, 6, 4, so the loops are dz outer, dx
   middle, dy inner — not the argument list's order. Observable because
   `destroyBlock` drops items.
7. **Leaving the End is absolute where entering it is relative.**
   `createDefault` and `missingRespawnBlock` pass `Set.of()` with no
   arguments; the entering branch passes `Relative.union(DELTA, Set.of(X_ROT))`.
   The two returning methods are byte-identical apart from `iconst_0` vs
   `iconst_1` at offset 37, and that flag is read once, to send
   `NO_RESPAWN_BLOCK_AVAILABLE`.
8. **`findClosestPortalPosition`'s small radius is the nether one.** 16 when
   `toNether`, 128 coming back — the same 8:1 as the coordinate scale.
   `getInSquare`'s filter is Chebyshev in x and z with **no y bound**, and the
   second filter is `hasProperty(HORIZONTAL_AXIS)`, not `is(NETHER_PORTAL)`.
9. **A portal cannot be lit in the End**, and lighting one by clicking the
   **top** of a block consumes a level-RNG draw:
   `Direction.Plane.HORIZONTAL.getRandomAxis(level.getRandom())`.
10. **There is no `DIM-1` and no `DIM1` in 26.2, and the overworld is not the
    world root.** `DimensionType.getStorageFolder` is
    `key.identifier().resolveAgainst(path.resolve("dimensions"))` — every
    dimension at `<world>/dimensions/<ns>/<path>/`, no special case. The
    brief said otherwise.

#### WHAT IS REFUSED, AND WHY — SO NOBODY RE-RUNS IT

* **A live player standing in the nether, and the gameplay corpus scenario for
  travel.** This is the refusal that matters and it is not about effort.
  Nothing in this server can produce a lit nether portal:
  `crate::itemuse::flint_and_steel` implements
  `BaseFireBlock.canBePlacedAt` and stops there — the `isPortal` branch is
  absent, and that branch is what writes any `minecraft:nether_portal` block
  into a world. So the *trigger* does not exist, and a
  `ci/checks/gameplay/*.toml` written today would record vanilla travelling
  and this server doing nothing: a check whose reference runs a mechanism the
  subject does not have. That is `tools/oracle-runner`'s **vacuous** verdict
  by construction, and `AGENT-BRIEF.md`'s central defect class by another
  door. **It is refused until the trigger exists, not deferred for time.**
  The predicate itself is ported and gated
  (`portal::in_portal_dimension`, `portal::DIRECTION_VALUES`,
  `portal::find_any_shape`, `portal::portal_block_positions`), so the call
  site is one branch in `itemuse.rs` — which belongs to items-2, not here.
* **The per-connection dimension switch in `status.rs`.** `change_dimension`
  writes the packets and `Tiles::shared_in` serves the world, and what is not
  wired is the connection state between them: `chunk_at` is a closure built
  once per session over one `Tiles`, and `Session.blocks` is one
  `&'a dyn Blocks`. Both would have to become functions of a per-connection
  current dimension. That is a real change and a small one — an `AtomicU8` on
  `Connection`, read by both — and it was **not** made here because the thing
  that would make it observable is the trigger above. Wiring it now would add
  a switch no check could turn red.
* **A direct served-chunk-vs-vanilla parity number for the nether or the
  End.** Not taken and not needed: the served chunk is byte-identical to the
  dump's over the whole column, and `docs/PARITY.md` states the dump against
  vanilla. A third measurement of the same composition would be a number with
  no new content, and the r=16 references it would use carry floors this
  branch did not re-derive.
* **`PoiManager` and therefore `findClosestPortalPosition`'s candidate
  source.** The selection is ported in full; the index is not. What that
  costs is bounded rather than waved at: `Stream.min` keeps the first minimum,
  so the missing index costs exact `(distSqr, y)` ties and costs nothing else.

#### NUMBERS

    752 lib tests, 0 failed
    served_dimensions        5 tests   62.3 s   196,608 blocks compared, 0 differ
    dimension_travel        20 tests    0.01 s
    dimension_travel_wire    4 tests    0.07 s   (T0)
    portal_shape             7 tests    0.00 s   60 spiral positions, 0 differ
    2 new T0 selectors: join_border (declared T0 and never run), dimension_travel_wire
    1 new golden generator: ci/spiral-golden.sh

Threadripper, `rustmc-dims`, 2026-09-05.

#### TWO THINGS INHERITED FROM THE BASE, BOTH FIXED HERE

* `ci/checks/test-targets.toml`'s `world-systems-wiring` bucket declared
  `join_border` as running in T0 and **no `ci/t0.sh` selector reached it** — a
  target under `tests/` is invisible to `--lib --bins`, so T0 was red on its
  own manifest from the commit that added the bucket.
* The three console-driven checks 113/114/115 were baselined and never swept,
  so `ci/corpus-falsifiable.py --check` refused all three: `T0 RED — a
  gameplay check carries no evidence it can go red`, on every branch stacked
  on `world-systems-2`.

#### THE SINGLE NEXT ITEM

**The `isPortal` branch in `itemuse.rs`, and then the per-connection
dimension.** In that order, because the second is only observable after the
first. The result that closes it: a `ci/checks/gameplay/*.toml` in which a
player lights an obsidian frame with flint and steel and steps into it,
observing the `respawn` bytes and the first nether chunk batch, swept with
`ci/corpus-falsifiable.py --sweep` and registered at **tier 2** — an edit of
the step it is named for must move its outcome.
