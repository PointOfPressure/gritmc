//! `NaturalSpawner$SpawnState`, `PotentialCalculator` and
//! `LocalMobCapCalculator` — the per-tick counts and the two caps.

use std::collections::BTreeMap;

use rustmc_data::{BlockPos, ChunkPos};

use super::tables::{MobSpawnCost, SpawnTables};
use super::{MobCategory, MAGIC_NUMBER, SPAWNING_CATEGORIES};

/// `net.minecraft.world.level.PotentialCalculator`.
///
/// A list of point charges and an inverse-distance sum. Vanilla appends in
/// order and never sorts, and the sum is over `double`s, so **the order
/// charges were added is observable through floating-point association**.
/// The list order here is the add order for that reason.
#[derive(Debug, Clone, Default)]
pub struct PotentialCalculator {
    charges: Vec<(BlockPos, f64)>,
}

impl PotentialCalculator {
    /// `addCharge(pos, charge)`. **A zero charge is not stored** — the
    /// method's first two instructions are `dload_2; dconst_0; dcmpl;
    /// ifeq` — so a type with no `spawn_costs` entry leaves the list
    /// untouched and cannot shift a later sum.
    pub fn add_charge(&mut self, pos: BlockPos, charge: f64) {
        if charge != 0.0 {
            self.charges.push((pos, charge));
        }
    }

    /// `getPotentialEnergyChange(pos, charge)`.
    ///
    /// Zero charge short-circuits to zero before the walk. Otherwise it is
    /// `charge * sum over the list of (c / sqrt(distSqr))`, with
    /// `distSqr == 0` contributing `Double.POSITIVE_INFINITY` — vanilla's
    /// `PointCharge.getPotentialChange` returns `Infinity` for a charge on
    /// the same block, which makes any budget comparison false and blocks
    /// the spawn.
    ///
    /// `distSqr` is `Vec3i.distToLowCornerSqr` — the **low corner**, no
    /// `+0.5` anywhere, unlike the spawn-point test in
    /// `isRightDistanceToPlayerAndSpawnPoint`.
    #[must_use]
    pub fn potential_energy_change(&self, pos: BlockPos, charge: f64) -> f64 {
        if charge == 0.0 {
            return 0.0;
        }
        let mut sum = 0.0f64;
        for (p, c) in &self.charges {
            let dx = f64::from(p.x) - f64::from(pos.x);
            let dy = f64::from(p.y) - f64::from(pos.y);
            let dz = f64::from(p.z) - f64::from(pos.z);
            let d = dx * dx + dy * dy + dz * dz;
            sum += if d == 0.0 {
                f64::INFINITY
            } else {
                c / d.sqrt()
            };
        }
        sum * charge
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.charges.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.charges.is_empty()
    }
}

/// `net.minecraft.world.level.LocalMobCapCalculator`.
///
/// Per-player counts over the players `ChunkMap.getPlayersCloseForSpawning`
/// reports for a chunk. **The `ChunkMap` query is not ported** — it is the
/// world join this branch refuses — so the player list per chunk is
/// supplied by the caller, keyed by a player index it chooses.
///
/// `canSpawn` returns true as soon as **any** near player is under the
/// category cap, and **false when the chunk has no near players at all**:
/// the loop falls out to `iconst_0`. So a chunk nobody is near cannot
/// spawn, and that is the local cap and not the distance rule.
#[derive(Debug, Clone, Default)]
pub struct LocalMobCapCalculator {
    /// chunk -> the players close enough for spawning.
    players_near_chunk: BTreeMap<(i32, i32), Vec<u32>>,
    /// player -> per-category count.
    player_mob_counts: BTreeMap<u32, BTreeMap<MobCategory, i32>>,
}

impl LocalMobCapCalculator {
    /// Seed the `playersNearChunk` cache. Vanilla fills it lazily from
    /// `ChunkMap.getPlayersCloseForSpawning`; a caller that has that query
    /// answers it here instead.
    pub fn set_players_near(&mut self, chunk: ChunkPos, players: Vec<u32>) {
        self.players_near_chunk.insert((chunk.x, chunk.z), players);
    }

    fn players_near(&self, chunk: ChunkPos) -> &[u32] {
        self.players_near_chunk
            .get(&(chunk.x, chunk.z))
            .map_or(&[][..], Vec::as_slice)
    }

    /// `addMob(ChunkPos, MobCategory)`.
    pub fn add_mob(&mut self, chunk: ChunkPos, category: MobCategory) {
        let players: Vec<u32> = self.players_near(chunk).to_vec();
        for p in players {
            *self
                .player_mob_counts
                .entry(p)
                .or_default()
                .entry(category)
                .or_insert(0) += 1;
        }
    }

    /// `canSpawn(MobCategory, ChunkPos)`.
    #[must_use]
    pub fn can_spawn(&self, category: MobCategory, chunk: ChunkPos) -> bool {
        for p in self.players_near(chunk) {
            match self.player_mob_counts.get(p) {
                // `MobCounts` absent for that player is `null`, and the
                // branch takes the true arm without consulting a count.
                None => return true,
                Some(counts) => {
                    if counts.get(&category).copied().unwrap_or(0)
                        < category.max_instances_per_chunk()
                    {
                        return true;
                    }
                }
            }
        }
        false
    }
}

/// One entity as `createState` sees it.
#[derive(Debug, Clone)]
pub struct CountedEntity {
    pub kind: String,
    pub pos: BlockPos,
    /// `Mob.isPersistenceRequired()`.
    pub persistence_required: bool,
    /// `Mob.requiresCustomPersistence()`.
    pub requires_custom_persistence: bool,
    /// Whether the entity is a `Mob` at all. A non-`Mob` entity skips both
    /// persistence tests and is counted by category like any other.
    pub is_mob: bool,
}

/// `NaturalSpawner$SpawnState`.
#[derive(Debug, Clone)]
pub struct SpawnState {
    spawnable_chunk_count: i32,
    mob_category_counts: BTreeMap<MobCategory, i32>,
    spawn_potential: PotentialCalculator,
    local_mob_cap: LocalMobCapCalculator,
    last_checked: Option<(BlockPos, String, f64)>,
}

impl SpawnState {
    /// `NaturalSpawner.createState(spawnableChunkCount, entities,
    /// chunkGetter, localMobCapCalculator)`.
    ///
    /// The skip chain, in the bytecode's order (offsets 48-100):
    ///
    /// 1. `entity instanceof Mob` and `isPersistenceRequired()` -> skip;
    /// 2. still a `Mob` and `requiresCustomPersistence()` -> skip;
    /// 3. `getType().getCategory() == MISC` -> skip.
    ///
    /// **The `MISC` test is not inside the `instanceof`**: a non-`Mob`
    /// entity reaches it and is counted if its category is not `MISC`,
    /// which no vanilla non-`Mob` type is. The charge and the local cap
    /// are then fed through `ChunkGetter.query(ChunkPos.pack(pos), …)`,
    /// which **drops the entity entirely if that chunk is not loaded** —
    /// so an entity in an unloaded chunk counts against neither cap.
    #[must_use]
    pub fn create(
        spawnable_chunk_count: i32,
        entities: &[CountedEntity],
        chunk_loaded: &dyn Fn(ChunkPos) -> bool,
        mut local_mob_cap: LocalMobCapCalculator,
        tables: &SpawnTables,
        biome_at: &dyn Fn(BlockPos) -> String,
    ) -> Self {
        let mut spawn_potential = PotentialCalculator::default();
        let mut mob_category_counts: BTreeMap<MobCategory, i32> = BTreeMap::new();
        for e in entities {
            if e.is_mob && (e.persistence_required || e.requires_custom_persistence) {
                continue;
            }
            let Some(facts) = tables.facts(&e.kind) else {
                continue;
            };
            if facts.category == MobCategory::Misc {
                continue;
            }
            let chunk = ChunkPos::containing(e.pos);
            if !chunk_loaded(chunk) {
                continue;
            }
            // The lambda's body, in its order: charge, then category
            // count, then local cap.
            let charge = tables
                .biome(&biome_at(e.pos))
                .and_then(|b| b.spawn_cost(&e.kind))
                .map_or(0.0, |c| c.charge);
            spawn_potential.add_charge(e.pos, charge);
            *mob_category_counts.entry(facts.category).or_insert(0) += 1;
            local_mob_cap.add_mob(chunk, facts.category);
        }
        Self {
            spawnable_chunk_count,
            mob_category_counts,
            spawn_potential,
            local_mob_cap,
            last_checked: None,
        }
    }

    #[must_use]
    pub fn spawnable_chunk_count(&self) -> i32 {
        self.spawnable_chunk_count
    }

    #[must_use]
    pub fn category_count(&self, category: MobCategory) -> i32 {
        self.mob_category_counts
            .get(&category)
            .copied()
            .unwrap_or(0)
    }

    /// `canSpawnForCategoryGlobal(MobCategory)`:
    /// `count < max * spawnableChunkCount / 289`, **integer division**, so
    /// the cap is floored and a small loaded area can floor it to zero.
    #[must_use]
    pub fn can_spawn_for_category_global(&self, category: MobCategory) -> bool {
        let cap = category
            .max_instances_per_chunk()
            .wrapping_mul(self.spawnable_chunk_count)
            / MAGIC_NUMBER;
        self.category_count(category) < cap
    }

    /// `canSpawnForCategoryLocal(MobCategory, ChunkPos)`.
    /// `SharedConstants.DEBUG_IGNORE_LOCAL_MOB_CAP` is false in a shipped
    /// jar, so the `||` never short-circuits and is recorded rather than
    /// exposed as a knob.
    #[must_use]
    pub fn can_spawn_for_category_local(&self, category: MobCategory, chunk: ChunkPos) -> bool {
        self.local_mob_cap.can_spawn(category, chunk)
    }

    /// `NaturalSpawner.getFilteredSpawningCategories(state, spawnEnemies,
    /// spawnFriendlies)`.
    ///
    /// Read the two flags off the bytecode rather than off their names,
    /// because the names are the wrong way round for one of them:
    ///
    /// * `spawnEnemies` gates every category that is **not friendly**, and
    ///   `MONSTER` is the only unfriendly one;
    /// * `spawnFriendlies` gates every **persistent** category, and among
    ///   the seven spawning categories that is `CREATURE` alone. It is the
    ///   `gameTime % 400 == 0` flag, so animals get one pass in four
    ///   hundred ticks and everything else gets every tick.
    #[must_use]
    pub fn filtered_spawning_categories(
        &self,
        spawn_enemies: bool,
        spawn_friendlies: bool,
    ) -> Vec<MobCategory> {
        let mut out = Vec::with_capacity(SPAWNING_CATEGORIES.len());
        for c in SPAWNING_CATEGORIES {
            if !spawn_enemies && !c.is_friendly() {
                continue;
            }
            if !spawn_friendlies && c.is_persistent() {
                continue;
            }
            if !self.can_spawn_for_category_global(c) {
                continue;
            }
            out.push(c);
        }
        out
    }

    /// `SpawnState.canSpawn(type, pos, chunk)` — the density check, and the
    /// `SpawnPredicate` `spawnForChunk` hands to `spawnCategoryForChunk`.
    ///
    /// **The biome here is the ROUGH one** —
    /// `NaturalSpawner.getRoughBiome(pos, chunk)`, the chunk's noise biome
    /// at quart resolution — while `getRandomSpawnMobAt` picks its mob
    /// from `ServerLevel.getBiome`. Two different biome lookups on the
    /// same position, and this module keeps them separate rather than
    /// collapsing them.
    ///
    /// Mutating: it caches `(pos, type, charge)` for [`Self::after_spawn`],
    /// and it caches them **before** deciding anything, so a refused
    /// position still moves the cache.
    pub fn can_spawn(
        &mut self,
        kind: &str,
        pos: BlockPos,
        rough_biome: &str,
        tables: &SpawnTables,
    ) -> bool {
        let cost = tables.biome(rough_biome).and_then(|b| b.spawn_cost(kind));
        match cost {
            None => {
                self.last_checked = Some((pos, kind.to_owned(), 0.0));
                true
            }
            Some(MobSpawnCost {
                charge,
                energy_budget,
            }) => {
                self.last_checked = Some((pos, kind.to_owned(), charge));
                self.spawn_potential.potential_energy_change(pos, charge) <= energy_budget
            }
        }
    }

    /// `SpawnState.afterSpawn(mob, chunk)`.
    ///
    /// The `(lastCheckedPos, lastCheckedType)` cache is consulted with the
    /// mob's **actual** `blockPosition()` after `snapTo`, which is not
    /// always the position `canSpawn` tested; on a miss it re-reads the
    /// rough biome's cost at the new position, and a type with no cost
    /// there charges **0**, which `addCharge` then declines to store.
    pub fn after_spawn(
        &mut self,
        kind: &str,
        block_pos: BlockPos,
        rough_biome: &str,
        tables: &SpawnTables,
    ) {
        let charge = match &self.last_checked {
            Some((p, t, c)) if *p == block_pos && t == kind => *c,
            _ => tables
                .biome(rough_biome)
                .and_then(|b| b.spawn_cost(kind))
                .map_or(0.0, |c| c.charge),
        };
        self.spawn_potential.add_charge(block_pos, charge);
        if let Some(facts) = tables.facts(kind) {
            *self.mob_category_counts.entry(facts.category).or_insert(0) += 1;
            self.local_mob_cap
                .add_mob(ChunkPos::containing(block_pos), facts.category);
        }
    }

    #[must_use]
    pub fn potential(&self) -> &PotentialCalculator {
        &self.spawn_potential
    }
}
