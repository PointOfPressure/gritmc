# Cold start — branch `block-entity-behaviour`

---

### COLD START — branch `block-entity-behaviour` off `58f1fa5c6`, 2026-09-05

**The ticking and interaction half of thirteen block-entity types,
transcribed from the pinned 26.2 jar. One of them reaches the wire; the
other twelve cannot until `block-entities-2` lands, and that is the single
most useful thing on this page.** Remote dir `~/rustmc-beh`.

    module            type(s)                                  lines  tests
    hopper.rs         HopperBlockEntity transfer                 ~620    13
    cooking.rs        furnace / blast furnace / smoker           ~700    12
    fuel.rs           FuelValues + the hopper's fuel rule        ~220     4
    campfire.rs       CampfireBlockEntity                        ~300     7
    bell.rs           BellBlock + BellBlockEntity                ~480    11
    jukebox.rs        JukeboxBlockEntity + JukeboxSongPlayer     ~520     9
    lectern.rs        LecternBlockEntity                         ~230     8
    bookshelf.rs      ChiseledBookShelfBlockEntity               ~250     6
    crafter.rs        CrafterBlockEntity                         ~340    10
    decoratedpot.rs   DecoratedPotBlockEntity                    ~150     4
    brewing.rs        BrewingStandBlockEntity                    ~400     9
    spawner.rs        BaseSpawner                                ~430     8
    beacon.rs         BeaconBlockEntity                          ~300    10
    conduit.rs        ConduitBlockEntity                         ~230     6
    sculk.rs          VibrationSystem                            ~260     8
                                                                       ----
                                                                        125

Two new jar tables: `data/vanilla/fuel_values.json` (280 fuels,
`ci/fuel-values-golden.sh`) and `data/vanilla/jukebox_songs.json` (22
songs, `ci/jukebox-song-golden.sh`), both registered in
`ci/gen-gameplay-tables.sh`.

## THE BLOCKER, and it is a real one

**None of the tickers is called, and wiring them needs a field on
`crate::blockentity::BlockEntity` that `origin/block-entities-2` is
rewriting.** A furnace needs `litTimeRemaining`, a hopper a
`cooldownTime`, a campfire two four-int arrays; `main`'s `BlockEntity` has
`kind`, `items` and `sign` and nothing else. Adding those fields here
would collide head-on with that branch's eleven commits, and this lane was
briefed not to duplicate its tables.

So every module here is a pure function or a small state struct the caller
owns, and the join is one commit's worth of work **after** `block-entities-2`
merges: give `BlockEntity` an `Option<Box<Behaviour>>` beside its `sign`,
and add one pass to `status.rs`'s `clock.spawn` closure that walks the
loaded block entities. The tick order inside that pass is
`ServerLevel.tickBlockEntities`, which runs after the scheduled block
ticks and before `ServerChunkCache.tick`'s broadcast — the same place the
existing `queue.drain_due` loop sits.

The bell is the exception and it is already wired: it needs no
block-entity state at all, because `attemptToRing`'s whole output is a
queued `block_event` and a sound.

## What is refused, and why — do not re-run these

* **`PotionBrewing.mix` cannot be ported.** A potion is `minecraft:potion`
  plus a `POTION_CONTENTS` data component and `crate::item::ItemStack` has
  no components, so an id-keyed mix table would collapse every potion in
  the game onto one and turn any ingredient into any other potion.
  `brewing.rs` transcribes the *timing* in full and `NoBrewing` refuses
  the three questions; a stand fuels, shows its bottles and never brews.
* **The jukebox song id is not extractable.**
  `VibrationSystem`-style static tables are; a *datapack registry index*
  is not. Three JVM runs of `JukeboxSongGolden` over
  `VanillaRegistries.createLookup()` gave three different orders, so the
  index is a fresh random number per regeneration rather than the wrong
  number. `jukebox_songs.json` therefore sorts its rows by id, carries the
  alphabetical rank as an explicitly unverified stand-in, and flags itself
  `registryIdIsUnverified`. **Closing this needs a vanilla recording of a
  disc going in, not another dumper.**
* **`new ItemStack(item)` throws in a bootstrapped-only JVM** —
  `Holder$Reference.components`, "Components not bound yet". Any future
  goldengen that wants a stack has to read the structure it is probing
  directly, as `FuelValuesGolden` does by reflection over `FuelValues`'
  private map.
* **`VanillaRegistries.createLookup()` cannot back a tag-touching
  dumper** — its item lookup is under construction and
  `ItemTags.LOGS` throws before any datapack tag is consulted. Use
  `RegistryAccess.fromRegistryOfRegistries(BuiltInRegistries.REGISTRY)`
  after a `prepareTagReload(...).apply()`.
* **`Item.getCraftingRemainder` is not ported**, so `consumeFuel` empties
  the furnace's fuel slot rather than leaving a bucket. Wrong for one
  reachable fuel, in the direction of losing an item rather than
  duplicating one, and named at the line.
* **The sculk catalyst's charge spread and `VibrationSystem.Ticker`'s
  listener bookkeeping** need the game-event dispatch this server has
  none of. The frequency table and the comparator arithmetic are here;
  the dispatch is not.
* **A gameplay scenario for anything but the bell would go red rather
  than inert**, because vanilla ticks these block entities and we do not.
  That is a true divergence and it is the blocker above, not a check
  defect; writing the scenarios now would red the corpus for a reason
  already written down.

## The single next item, and the result that closes it

**Wire the tickers, after `block-entities-2` lands.** It closes when
`ci/checks/gameplay/<n>-a-hopper-moves-an-item-into-the-chest-below`
reaches tier 2 in `ci/checks/gameplay-falsifiable.toml` — the hopper is
the right first one because its output is a `container_set_slot` the
recorder already decodes, where a furnace's is a `block_update` on `lit`
that needs the ticker running for two hundred ticks.

`ci/checks/gameplay/795-a-bell-rings-when-its-side-is-clicked.toml` is
registered and **matches vanilla**: the corpus run of 2026-09-05 20:08 put
the same `block_event(1, 3)` and `block.bell.use` on both streams, the
sweep grades it **tier 2** (109/109, 0 unswept) and
`ci/checks/gameplay-baseline.toml` records it at `match`.

**It was deleted once by mistake and the mistake is worth knowing.** The
detached chain refused the scenario on `T2-EXIT != 0`, and that exit was
not a divergence: `ci/t2-gameplay.sh` prints `REGRESSED ...: not in the
baseline — a new check must record what it currently does` and exits 1 for
a check with no baseline row, which is bookkeeping. The scenario itself
read `match` four lines above it in the same log. **A new scenario needs a
row in `gameplay-baseline.toml` in the commit that adds it**, or the first
corpus run that sees it fails for a reason that has nothing to do with the
server.
