### COLD START — branch `fluids`, off `main` (3973), 2026-10-01

**Area: fluids — flow, buckets, interactions, waterlogging.** Worktree
`~/grit/work-fluids` (detached), remote dir `~/rustmc-fluids`. Not merged —
the orchestrator gates.

**What was there.** `main` had no fluid tick queue: `crate::ticks::on_used`
dropped every fluid tick by name, nothing could put a fluid into the world and
nothing moved one that was there. `origin/fluid-tickqueue-bucket` (26 commits,
2026-08-18, REBASE CONFLICT 1790 behind, never landed) had a careful
water-only port. Its sound parts were carried over file by file, not rebased;
its levelling/broadcast-order infrastructure (`chunk_order = "blind"`) was not
needed and was left behind.

**What is ported now** (every line from `javap -c` on the pinned jar):

* `ServerLevel`'s second `LevelTicks` (block then fluid, offsets 269/296),
  `Effects::fluid_ticks`, `drain_effects`, `fluid_ticks` saved per chunk.
* `FlowingFluid` for water AND lava: `tick`/`spread`/`spreadToSides`/
  `spreadTo`/`getNewLiquid`/`getSpread`/`getSlopeDistance`; `LavaFluid`'s
  `isFastLava` arms (30/10 ticks, drop 2/1, slope 2/4), `canBeReplacedWith`
  (height >= 4/9, any side), `spreadTo` DOWN into water = stone + 1501,
  `beforeDestroyingBlock` fizz; game-rule source conversion.
* `LiquidBlock`: `onPlace`/`neighborChanged` -> `shouldSpreadLiquid`
  (obsidian / cobblestone / basalt + 1501), `updateShape`'s source tick,
  `pickupBlock`. Every waterlogged state's `updateShape` water tick.
* `Level.setBlock`'s `now == state` guard after `onPlace` (offset 64) — was
  missing for every block; rails can move.
* `BucketItem.use` with the server's own ray (`calculateViewVector` through
  the Mth table, `traverseBlocks`, `VoxelShape.clip`, fluid boxes), fill and
  empty, `placeLiquid` waterlogging, nether evaporation; cauldron bucket
  entries (`CauldronInteractions`); `FAST_LAVA`/`WATER_EVAPORATES` from the
  level's dimension.
* `data/vanilla/fluid_rules.json` via `ci/fluid-rules-golden.sh`.

**Scenarios 161-169**, all dug at dx 0..1, dz 0..3 so every write is in one
chunk section (see FINDINGS 2026-10-01 for why only those columns are
reachable by a bucket).

**KILLED / REFUSED — do not re-open without a new mechanism:**

* Mob buckets: `MobBucketItem.checkExtraContent` creates and ticks a fish;
  this server ticks no fish. Refused as `PASS`, named in `bucket::content_of`.
* Water's `beforeDestroyingBlock` drops: no block destruction drops an item in
  this server on any path.
* Kelp/seagrass/bubble column fluid ticks: not ported; nothing on superflat
  reaches them.
* Outline vs collision shape for the bucket ray: identical for every block the
  rigs touch; differs for plants and fences (named in `crate::bucket`).

**MEASURED, 2026-10-01 04:06, `ci/t2-gameplay-subset.sh` over 161-169 in
`~/rustmc-fluids` (tip `440a43f02`): declared 9/9 match, strict 7/9.** Every
mechanic was confirmed live on the vanilla side first — the trench waves at
260/520/738 ms (water) and 1493/2992/4491 ms (lava), obsidian + 1501 at
(1,-1,1), obsidian north and cobblestone south with both fizzes before one
`section_blocks_update`, falling water `level=8` at the hole's floor. The two
strict diverges were vanilla's creative `createFilledResult` inventory add
(`container_set_slot` slot 37 / 38); fixed in `55236f8d4`, re-measured only
by the full run below.

**The full corpus run is the one at the end of the lane**, chained as
`fluids-full2` (t2-gameplay + sweep + `corpus-grades --write`) then
`fluids-land` (commit both ledgers if `--check` passes, push, T0). The first
attempt (`fluids-full`) was killed by its own 3 h `timeout` after 177 min in
the corpus-lock queue without ever recording — the queue, not the run.

**THE SINGLE NEXT ITEM**, and what closes it: land the ledgers from
`fluids-land`. If the full run moved anything outside 161-169, look first at
`300-rail-curves-towards-its-neighbour` — the `Level.setBlock` guard changes
what a bending rail's outer write does.
