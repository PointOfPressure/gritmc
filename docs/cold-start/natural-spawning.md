# Cold start — branch `natural-spawning`

Append-only. Newest entry last.

### COLD START — branch `natural-spawning` off `1d109abcf` (2,788), 2026-09-05

**Vanilla 26.2's `NaturalSpawner` is in `crates/rustmc-entity/src/spawn/`,
transcribed from the jar's bytecode as a host-trait module, and nothing in
`rustmc-server` calls it.** Worktree `~/grit/work-spawn`, remote dir
`~/rustmc-spawn` on the Threadripper. Not merged — the orchestrator gates.

    TABLES     data/vanilla/biome_spawners.json    ci/dump-biome-spawners.sh
                 66 biomes, 795 spawner rows over 52 entity types, 5 spawn costs
               data/vanilla/spawn_placements.json  ci/dump-spawn-placements.sh
                 83 registrations, 39 distinct spawn predicates
               data/vanilla/entity_spawn_facts.json ci/dump-entity-spawn-facts.sh
                 158 types, 86 in a spawning category, 38 notInPeaceful,
                 105 canSpawnFarFromPlayer, 2 noSummon
    MODULE     mod.rs        MobCategory (whole), SPAWNING_CATEGORIES, the five
                             constants incl. MAGIC_NUMBER 289 and
                             INSCRIBED_SQUARE_SPAWN_DISTANCE_CHUNK 5
               tables.rs     the three tables + WeightedList + SpawnerData
               state.rs      SpawnState, PotentialCalculator,
                             LocalMobCapCalculator, createState,
                             getFilteredSpawningCategories, canSpawn, afterSpawn
               placements.rs the four SpawnPlacementTypes, isValidEmptySpawnBlock,
                             checkSpawnRules
               rules.rs      6 of 39 predicates, 48 of 83 registrations
               natural.rs    spawnForChunk, spawnCategoryForChunk/ForPosition,
                             getRandomPosWithin,
                             isRightDistanceToPlayerAndSpawnPoint,
                             isValidSpawnPostitionForType, getRandomSpawnMobAt,
                             mobsAt, canSpawnMobAt, + a per-decision event log
               chunk_cache.rs ServerChunkCache.tickChunks as PROSE, deliberately
               host.rs       SpawnLevel + SpawnHost, every refusal at its method
               testing.rs    GridSpawnLevel + RecordingHost
    TESTS      19 green, 0.10 s (`cargo test -p rustmc-entity --test spawn_tables
               --test spawn_decisions`, Threadripper 2026-09-05, debug).
               clippy `--all-targets -D warnings` clean.

**Each table's generator cross-checks itself against a second source
before it writes:** the 66 biome names against
`assets/minecraft/lang/en_us.json`'s `biome.minecraft.*` keys (sets, not
counts); every spawner type, spawn-cost key and lowercased `EntityTypes`
field name against `minecraft:entity_type` in the generator's own
`registries.json` report; the assembled `SpawnPlacements` registration
count against the number of `invokestatic register` sites; and the
`notInPeaceful` count against `ci/mob-tick-census.py`'s independent
census, which is where **158 / 86 / 38** already stood.

## WHAT THE MODULE DECIDES, AND WHAT IT REFUSES

Everything from "which categories run this tick" to "spawn a `<kind>` at
`(x, y, z)` facing `<yaw>`". The three host calls past that point are
declared and unimplemented, by name:

* **the world join** — `ServerChunkCache.tickChunks`, `ChunkMap
  .collectSpawningChunks`/`.getPlayersCloseForSpawning`,
  `DistanceManager.getNaturalSpawnChunkCount`/`.inEntityTickingRange`, and
  the `Util.shuffle` of the spawning chunks. `spawn/chunk_cache.rs` is that
  method written out with the four facts a porter needs; nothing runs.
* **entity construction** — `EntityType.create`, `Mob.snapTo`,
  `Mob.finalizeSpawn` with its `SpawnGroupData` chain,
  `ServerLevel.addFreshEntityWithPassengers`, `Mob.checkSpawnRules`,
  `Mob.checkSpawnObstruction`. `SpawnHost::create_mob` and
  `SpawnHost::place`, split at the yaw draw because a draw sits between
  them. These are the mob branches' and `origin/mob-in-world`'s.
* **`ChunkGenerator.getMobsAt`'s `StructureSpawnOverride` walk** and
  `NetherFortressStructure.FORTRESS_ENEMIES` — a `StructureManager` query
  and a `WeightedList` built in a structure class.
* **33 of the 39 spawn predicates.** They return
  `PredicateVerdict::Untranscribed`, never a bool, so "not ported" cannot
  be read as "the position was wrong"; `spawn::rules::evaluate` **panics**
  on a predicate in neither `TRANSCRIBED` nor `PENDING`, and
  `tests/spawn_tables.rs` reaches that without a level, so a version bump
  that adds one is a red.
* **the `spawnPotentials` charge for anything but the numbers in the
  data.** `PotentialCalculator` is transcribed; what it is fed is the five
  `spawn_costs` entries and nothing invented.

## FINDINGS — all in `docs/FINDINGS.md`, the load-bearing ones here

* `EntityType$Builder.<init>` defaults `canSpawnFarFromPlayer` to
  `category == CREATURE || MISC` (offsets 94-114). **103 of the 105 types
  that have it never call the builder method**; only `pillager` and
  `shulker` do. Reading it as "false unless set" puts the 128*128 gate on
  every animal.
* **`minecraft:ocelot` (CREATURE) is in the `monster` spawner list of
  `jungle` and `bamboo_jungle`**, weight 2. Every gate past the draw uses
  the *type's* category, so an ocelot drawn on the MONSTER pass is charged
  to the CREATURE cap.
* `jungle`/`bamboo_jungle`/`sparse_jungle` each list **`chicken 4..4` twice
  at weight 10**. `SpawnerData` is a record of `(type, min, max)` with no
  weight, so de-duplicating on record equality — which is what
  `WeightedList.contains` compares in `canSpawnMobAt` — halves the
  chicken's share.
* A **zero-weight `WeightedList` draws nothing**: the selector is null and
  `getRandom` returns empty before `nextInt`. Most biome/category pairs are
  empty, so drawing first would desync the shared stream every chunk.
* `getFilteredSpawningCategories`' names are misleading. `spawnEnemies`
  (the `spawn-monsters` property, a field not a game rule) gates
  **unfriendly** categories = `MONSTER` alone; `spawnFriendlies`
  (`gameTime % 400 == 0`) gates **persistent** ones = `CREATURE` alone.
* `SpawnPlacementTypes.NO_RESTRICTIONS` is `iconst_1; ireturn` — **no
  world-border test**, unlike the other three. Ten registered types and
  every unregistered one take it, and **six types in a spawning category
  have no row at all**: allay, bee, piglin_brute, sniffer, tadpole,
  zombie_nautilus.
* `ServerChunkCache.tickChunks` **shuffles the spawning-chunk list on
  `level.getRandom()` every tick**, before any spawning. The loaded-chunk
  count therefore shifts the stream for everything that tick.
* The density check is five numbers: 5 `spawn_costs` entries in 2 biomes
  of 66. No `PotentialCalculator` at all is wrong in two biomes.

## WHAT THE INSTRUMENT CAN AND CANNOT SAY — READ THIS BEFORE QUOTING IT

`tests/spawn_decisions.rs` runs the whole decision over a synthetic chunk
against a seeded `LegacyRandomSource` and records every draw and every
rejection. **It verifies this transcription's internal consistency and
nothing else.** No vanilla run has been compared against it and none can
be today.

The one assertion a wrong transcription cannot pass is
`draws_match_the_decision_log_over_sixty_four_seeds`: it recomputes the
expected draw count from the **events** and compares it with the
**generator's** counter — two derivations of one run, so a draw taken on
the wrong branch moves one and not the other. It carries its own vacuity
control (more than 8 of 64 seeds must actually spawn).

Three injections, each applied to the real source, each shown red, and the
restore shown green:

| injection | failed |
|---|---|
| draw the spawn yaw before `getMobForSpawn` instead of after | 1 |
| `x += nextInt(6)` instead of `nextInt(6) - nextInt(6)` | 4 |
| a zero-weight `WeightedList` draws before checking | 1 |

**The first one did not redden the first time it was tried.** `RecordingHost`
never refused a creation, so no test reached the branch;
`an_uncreatable_type_ends_the_call_before_the_yaw_is_drawn` exists because
of that and is the assertion that pins the split.

## THE SCENARIO I WOULD WRITE, AND WHAT IT NEEDS

**No gameplay scenario is registered, deliberately.** One written today
would be vacuous by construction, and `ci/mob-substrate.py` would be
right to say so: `ci/t2-gameplay.sh` boots `difficulty=peaceful`,
`spawn-monsters=false` and game rule `spawn_mobs=false` — natural spawning
is off **three times over** — and `origin/corpus-dark-systems` measured
lifting `spawn_mobs` through the `[[gamerule]]` door as a **NULL, 15/15
pairs over 6 runs, 6 distinct projections**.

The scenario is **`NNN-monster-spawning-saturates-the-global-cap`**:

* substrate: the survival pass (`ci/t2-gameplay-survival.sh`,
  `origin/combat-parity`), `difficulty=easy` or harder,
  `spawn-monsters=true`, `[[gamerule]] spawn_mobs=true`, a sealed dark
  room at a known chunk radius so `getNaturalSpawnChunkCount` is fixed;
* the observable is **not a position and not a yaw** — both ride on the
  unseeded stream and `origin/entity-parity` has twice ruled out a
  type-keyed yaw exemption. It is the **count of live MONSTER-category
  entities once it stops rising**, which
  `SpawnState.canSpawnForCategoryGlobal` pins at
  `70 * spawnableChunkCount / 289`, integer division — an integer both
  sides can agree on exactly;
* it therefore needs a projection that keeps `add_entity`'s kind and
  `remove_entities`, drops `x/y/z/y_rot/y_head_rot`, and reduces to a
  count. `ci/mob-projection-search.py` already showed `{add_entity}` minus
  the two yaw fields is **120/120 identical across vanilla runs**; the
  positions are the untested half and are why the reduction is a count and
  not a set.

**What it needs before it can be written, in order:** (1) a monster kind
that exists in this server — `origin/mob-in-world` has pig and bat, both
CREATURE/AMBIENT, so a MONSTER is new work; (2) the world join above, so
`spawnForChunk` is reachable at all; (3) the survival substrate landed on
`main`; (4) a resolving-power measurement first — `ci/corpus-floor.sh`
over at least two vanilla recordings of that scenario, because a
saturating count that saturates *late* is a number about the window and
not about the server, and this project has published one of those before.

## KILLED AS NULLS — DO NOT RE-RUN

* **A reflection dump of `SpawnPlacements.DATA_BY_TYPE`.** It is a
  `HashMap` keyed on `EntityType` objects; its iteration order is an
  identity hash, different every JVM run, and this project's golden rules
  refuse that. The `javap` + `BootstrapMethods` walk gives source order,
  which is stable, and the output is sorted by registry name anyway
  because **registration order is not behaviour** — nothing iterates the
  map, every read is `get(type)`.
* **`registries.bin` as the second source for the biome count.** It holds
  `minecraft:worldgen/biome` with the right count, but it is itself a
  generated table in `data/vanilla` and the generation order does not
  guarantee it exists first. `assets/minecraft/lang/en_us.json` is in the
  jar, is written by a different part of the build from
  `data/minecraft/worldgen/biome/`, and gives set equality rather than a
  count.
* **A golden of the spawn decisions taken from the jar.** Every entry
  point needs a `ServerLevel` and a `Mob`; that is the same wall the goal
  and brain lanes hit, and `docs/cold-start/mobs-goals.md` records it.

## NEEDS HANDS

* **`data/vanilla` grew by three tables and every tree now pays ~6 s more
  in `ci/gen-gameplay-tables.sh`.** All three are `javap`/`unzip` only —
  no JVM run, no cargo — so they are the cheapest rows in that table, but
  the count in the log moves from 28 to 31.
* Nothing else. The next lane on this area writes the server-side
  `SpawnLevel`/`SpawnHost` over `rustmc-server`'s level and
  `origin/mob-in-world`'s `ServerMob`, and the 33 pending predicates
  most-shared first — every one of them is a `SpawnLevel` method before it
  is a line in `rules.rs`.
