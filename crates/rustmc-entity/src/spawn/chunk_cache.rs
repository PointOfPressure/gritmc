//! `ServerChunkCache.tickChunks` — the caller, transcribed as a
//! description and **not** as code.
//!
//! This is the world join the branch refuses. Every line below is read off
//! `ServerChunkCache.tickChunks(ProfilerFiller, long)` in the 26.2 jar and
//! is written here so that whoever wires natural spawning into
//! `rustmc-server` does not re-derive it; nothing in this module runs.
//!
//! ```text
//! push("naturalSpawnCount")
//! int spawnableChunkCount = distanceManager.getNaturalSpawnChunkCount()
//! SpawnState state = NaturalSpawner.createState(
//!         spawnableChunkCount,
//!         level.getAllEntities(),
//!         this::getFullChunk,                       // drops unloaded chunks
//!         new LocalMobCapCalculator(chunkMap))
//! this.lastSpawnState = state
//!
//! boolean spawnMobs      = gameRules.get(GameRules.SPAWN_MOBS)        // `spawn_mobs`
//! int     randomTickSpeed = gameRules.get(GameRules.RANDOM_TICK_SPEED)
//!
//! List<MobCategory> categories;
//! if (spawnMobs) {
//!     boolean spawnFriendlies = level.getGameTime() % 400L == 0L;
//!     categories = NaturalSpawner.getFilteredSpawningCategories(
//!             state, this.spawnEnemies, spawnFriendlies);
//! } else {
//!     categories = List.of();
//! }
//!
//! popPush("filteringSpawningChunks")
//! chunkMap.collectSpawningChunks(this.spawningChunks)
//! popPush("shuffleSpawningChunks")
//! Util.shuffle(this.spawningChunks, level.getRandom())
//! popPush("tickSpawningChunks")
//! for (LevelChunk c : this.spawningChunks) tickSpawningChunk(c, gameTime, categories, state)
//! finally { this.spawningChunks.clear() }
//!
//! popPush("tickTickingChunks")
//! chunkMap.forEachBlockTickingChunk(c -> tickChunk(c, randomTickSpeed))
//! if (spawnMobs) { popPush("customSpawners"); level.tickCustomSpawners(this.spawnEnemies) }
//! pop()
//! ```
//!
//! and `tickSpawningChunk(chunk, gameTime, categories, state)`:
//!
//! ```text
//! ChunkPos p = chunk.getPos()
//! chunk.incrementInhabitedTime(gameTime)
//! if (distanceManager.inEntityTickingRange(p.toLong())) level.tickThunder(chunk)
//! if (categories.isEmpty()) return
//! if (level.canSpawnEntitiesInChunk(p)) NaturalSpawner.spawnForChunk(level, chunk, state, categories)
//! ```
//!
//! # The four facts a porter needs out of that, none of them obvious
//!
//! * **`Util.shuffle` draws.** The spawning-chunk list is shuffled on
//!   `level.getRandom()` — the same shared stream every spawn draw comes
//!   from — **every tick**, before any spawning happens, and the shuffle
//!   consumes one `nextInt` per element from the end down. So the number
//!   of loaded spawning chunks changes the stream position for everything
//!   that tick, and a port that iterated chunks in a fixed order would
//!   diverge on the first tick even with a correct spawner.
//! * **The 400-tick cadence gates `CREATURE` and nothing else.**
//!   `spawnFriendlies` is `gameTime % 400 == 0` and
//!   `getFilteredSpawningCategories` uses it to drop **persistent**
//!   categories; of the seven spawning categories only `CREATURE` is
//!   persistent. It is not "friendlies every 400 ticks" — `AMBIENT`,
//!   `AXOLOTLS` and the three water categories are friendly and run every
//!   tick.
//! * **`spawnEnemies` is a field, not a game rule.** It comes from
//!   `spawn-monsters` in `server.properties` through
//!   `ServerChunkCache.setSpawnSettings`, and it gates every **unfriendly**
//!   category, which is `MONSTER` alone. The `spawn_mobs` game rule is the
//!   separate, outer switch: false empties the category list entirely and
//!   also skips `tickCustomSpawners`.
//! * **`createState` walks `level.getAllEntities()` every tick** and
//!   re-derives every cap from scratch. There is no incremental count to
//!   keep correct.
//!
//! # Why none of it is implemented here
//!
//! `getNaturalSpawnChunkCount`, `collectSpawningChunks`,
//! `inEntityTickingRange` and `getPlayersCloseForSpawning` are all
//! `DistanceManager`/`ChunkMap` state, and `crates/rustmc-server` has no
//! equivalent of either at the time of writing. Approximating the
//! spawnable-chunk count would put a wrong denominator under
//! `canSpawnForCategoryGlobal`'s `max * count / 289` — a number that
//! decides how many mobs the world holds — and this project's rule is that
//! an approximation is the one-way door. So the count is an input to
//! [`super::state::SpawnState::create`] and this file is prose.
