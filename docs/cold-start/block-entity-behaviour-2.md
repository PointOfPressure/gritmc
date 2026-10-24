# Cold start — branch `block-entity-behaviour-2`

---

### COLD START — branch `block-entity-behaviour-2` off `b6237f29c`, 2026-09-05

**The join. `Level.tickBlockEntities` is called, four of the fifteen
ported tickers run against the world, and the furnace family looks up
through the gated recipe matcher instead of a second parser.** Remote dir
`~/rustmc-beh2`.

`block-entity-behaviour` is rebased onto the `main` that carries
`block-entities-2`; its nineteen commits are underneath this branch
unchanged.

## What runs now

    arm            vanilla method                          reaches the wire as
    furnace x3     AbstractFurnaceBlockEntity.serverTick   block_update on `lit`
    jukebox        JukeboxBlockEntity.tick                 level_event(1011) + has_record
    campfire       cookTick / cooldownTick                 block_update + block_entity_data
    hopper         HopperBlockEntity.pushItemsTick         nothing — see the refusal below
    bell           BellBlockEntity.serverTick              (use path only, from the parent branch)

Six ported arms are still `{}` in `tick_one` — brewing stand, spawner,
beacon, conduit, crafter and the bell's raid counter. Each is a pure
function with tests already; what each needs is written under "the next
item" below.

## The table, and why it is three-valued

`crate::behaviour::server_ticker` answers for all forty-nine kinds:
**12 Ported, 15 Unported, 22 None.** Every `Unported` row names the
vanilla method this server is not running, because collapsing that group
into "we do not tick it" is how a gap stops being visible.

`ci/block-entity-golden.sh` grew two columns to check it — `tickers`, the
declared static `(Level, BlockPos, BlockState, T)V` methods on each
entity's class chain, and `blockTicker`, whether its first block's class
declares `getTicker`. Both are **declaration** walks for the reason the
`updatePacket` column is one: asking `getTicker` needs a `Level` and a
bootstrapped-only JVM has none.

The predicate is `Level.class.isAssignableFrom(p[0])` with an arity of 4
or 5. An exact four-parameter match on `Level` reported the **whole
furnace family** as having no ticker: five tickers in the registry are
declared on `ServerLevel`, and `CampfireBlockEntity.cookTick` captures a
`RecipeManager.CachedCheck` as a fifth parameter.

**What the golden cannot see is which side a ticker is handed to**, and
four types prove it: `ChestBlockEntity`, `TrappedChestBlockEntity`,
`EnderChestBlockEntity` and `SkullBlockEntity` all declare one and all
four blocks null the server arm. Those five ids are a fixed exception
list, so a fifth type acquiring a client-only ticker reds the check.

## Two things the brief said that the bytecode refuted

* **`tickBlockEntities` runs LAST, not before `ServerChunkCache.tick`.**
  The order is `LevelTicks.tick` twice, `Raids.tick`,
  `ServerChunkCache.tick`, `runBlockEvents`, `EnderDragonFight.tick`,
  `EntityTickList.forEach`, `tickBlockEntities`. The broadcast lives
  inside `ServerChunkCache.tick`, so a block a ticker rewrites is
  broadcast on the **next** tick. A furnace lighting at tick N sends its
  `lit` update at N+1.
* **A furnace has no `block_entity_data`.**
  `AbstractFurnaceBlockEntity` does not declare `getUpdatePacket` — 19
  of 49 do and it is not one — so "the block_entity_data of cooking
  progress" names a packet vanilla never sends. The four counters travel
  as the menu's data slots, and nothing here moves those yet.

## The cooking join

`cooking.rs` carried a second parser over the same 116 documents
(`CookingRecipes`, `CookingRecipe`, a private `parse_result` and a
private `find`). It is deleted. `cooking::server_tick`,
`total_cook_time`, `set_item` and `campfire::cook_tick`/`place_food` take
`&crate::recipes::Recipes` and go through `find_single(RecipeType, stack)`,
which is `RecipeManager.getRecipeFor(type, new SingleRecipeInput(stack),
level)` gated against vanilla's own answer over 3,122 vectors.
`cooking::CookingKind` is a re-export of the matcher's, so no call site
outside those two files moved.

**`origin/recipes`' nine commits are cherry-picked underneath** —
56d3fd9d9, 0d15ab81c, dd2795e94, aef5356c9, 3063ac380, 46ed4dac8,
6a73dd1d7, 0adfd416d, 827db4499 — because the branch had not landed and
the join does not build without `find_single`. A rebase after `recipes`
merges drops them.

## What is refused, and why — do not re-run these

* **The hopper's `container_set_slot` scenario cannot be written on
  `main` and the ticker is not the reason.** A container write sends no
  packet of its own; the slot comes from
  `AbstractContainerMenu.broadcastChanges`, which vanilla runs every tick
  from `ServerPlayer.doTick`. Here `click::broadcast_changes` is called
  only from a click handler, and the open menu lives in
  `Session.menu.open` on the **connection thread's stack**, where the
  clock thread cannot reach it. **The join point is exactly that**: the
  open `Menu` has to move onto `crate::players::Connection` behind a lock
  before the tick pass can diff it. Menus are `origin/stack-gameplay`'s
  area and this branch does not move them. A scenario written today would
  record `inert` and be refused by `ci/corpus-falsifiable.py`, correctly.
* **The drops a campfire finishes with are reported, not spawned.**
  `Containers.dropItemStack` draws a velocity and `crate::dropitem` owns
  that; spawning one with a guessed velocity would put a wrong
  `add_entity` on the wire where today there is none.
* **The jukebox eject half is not ported.**
  `JukeboxBlockEntity.popOutTheItem` spawns an item entity, same reason.
* **Everything the parent branch refused still stands** —
  `PotionBrewing.mix`, the jukebox song id, `new ItemStack(item)` in a
  bootstrapped-only JVM, `VanillaRegistries.createLookup()` against a
  tag, `Item.getCraftingRemainder`, the sculk catalyst's charge spread.
  See `docs/cold-start/block-entity-behaviour.md`.

## The single next item, and the result that closes it

**Wire the remaining six arms, cheapest observable first.** The crafter
is the one with a wire effect that needs nothing new — `server_tick`
returns a bool and `CrafterBlock` writes `TRIGGERED` and a
`level_event(1050)` on a failed craft — and the brewing stand is next,
because `BrewTick::bottle_bits` is a `setBlock(..., 2)` on three
`has_bottle_N` properties. The beacon and the conduit both need a world
scan this pass does not do yet; the spawner needs mob spawning.

It closes when a seventh `ci/checks/gameplay/*` reaches tier 2 in
`ci/checks/gameplay-falsifiable.toml` on a mechanism that runs from the
tick pass rather than from a handler — 797 is the first and the only one
today.
