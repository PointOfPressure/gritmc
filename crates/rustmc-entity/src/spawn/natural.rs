//! `NaturalSpawner`'s own methods, in the order the call graph reaches
//! them.

use rustmc_data::{BlockPos, ChunkPos};

use super::host::{EntitySpawnReason, SpawnHost, SpawnLevel, SpawnRandom, SpawnRequest};
use super::placements;
use super::rules::PredicateVerdict;
use super::state::SpawnState;
use super::tables::{SpawnTables, SpawnerData, WeightedSpawnList};
use super::{MobCategory, MIN_PLAYER_DISTANCE_SQR, MIN_SPAWN_DISTANCE, SPAWN_ATTEMPTS, SPREAD};

/// `NaturalSpawner$SpawnPredicate` and `NaturalSpawner$AfterSpawnCallback`,
/// the two functional interfaces `spawnCategoryForChunk` takes.
///
/// `spawnForChunk` binds both to the `SpawnState`; the three-argument
/// `spawnCategoryForPosition` binds them to `(t, p, c) -> true` and
/// `(m, c) -> {}`, which is [`NoFilter`].
pub trait SpawnFilter {
    /// `SpawnPredicate.test(type, pos, chunk)`.
    fn test(&mut self, kind: &str, pos: BlockPos) -> bool;
    /// `AfterSpawnCallback.run(mob, chunk)`.
    fn after_spawn(&mut self, kind: &str, block_pos: BlockPos);
}

/// The three-argument `spawnCategoryForPosition`'s pair: always true,
/// do nothing.
#[derive(Debug, Default)]
pub struct NoFilter;

impl SpawnFilter for NoFilter {
    fn test(&mut self, _kind: &str, _pos: BlockPos) -> bool {
        true
    }
    fn after_spawn(&mut self, _kind: &str, _block_pos: BlockPos) {}
}

/// `spawnForChunk`'s pair: `state::canSpawn` and `state::afterSpawn`.
pub struct StateFilter<'a, L: SpawnLevel + ?Sized> {
    pub state: &'a mut SpawnState,
    pub level: &'a L,
    pub tables: &'a SpawnTables,
}

impl<L: SpawnLevel + ?Sized> SpawnFilter for StateFilter<'_, L> {
    fn test(&mut self, kind: &str, pos: BlockPos) -> bool {
        let rough = self.level.rough_biome(pos);
        self.state.can_spawn(kind, pos, &rough, self.tables)
    }
    fn after_spawn(&mut self, kind: &str, block_pos: BlockPos) {
        let rough = self.level.rough_biome(block_pos);
        self.state.after_spawn(kind, block_pos, &rough, self.tables);
    }
}

/// Why one pack member was not spawned, or that it was.
///
/// The log is the instrument: a golden over a synthetic chunk compares
/// this sequence, and a reason is more discriminating than a count because
/// two ports can reject the same position for different rules.
#[derive(Debug, Clone, PartialEq)]
pub enum SpawnEvent {
    /// `getRandomPosWithin`'s answer, before any gate.
    ChunkPos { pos: BlockPos },
    /// `pos.getY() < level.getMinY() + 1`.
    BelowMinY,
    /// `chunk.getBlockState(pos).isRedstoneConductor(chunk, pos)`.
    RedstoneConductor,
    /// A new attempt of the three, with its provisional pack size.
    Attempt { index: i32, pack_size: i32 },
    /// The walked position for one pack member.
    Position { pos: BlockPos },
    /// `getNearestPlayer` returned null.
    NoPlayer,
    /// `isRightDistanceToPlayerAndSpawnPoint` was false.
    WrongDistance { player_dist_sqr: f64 },
    /// `getRandomSpawnMobAt` was empty — the attempt ends.
    NoMob,
    /// The drawn `SpawnerData` and the real pack size it implies.
    Picked { data: SpawnerData, pack_size: i32 },
    /// `isValidSpawnPostitionForType` was false, with the gate that failed.
    BadPositionForType { gate: PositionGate },
    /// The `SpawnPredicate` — `SpawnState.canSpawn`'s density check — was
    /// false.
    FilteredOut,
    /// `getMobForSpawn` returned null. The whole call ends here.
    CreateFailed,
    /// `isValidPositionForMob` was false.
    MobPositionRejected,
    /// A mob was placed.
    Spawned {
        kind: String,
        pos: BlockPos,
        yaw: f32,
    },
    /// `spawnedThisCall >= mob.getMaxSpawnClusterSize()`. The call ends.
    ClusterFull,
    /// `mob.isMaxGroupSizeReached(groupCount)`. The attempt ends.
    GroupFull,
}

/// Which gate of `isValidSpawnPostitionForType` refused a position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PositionGate {
    /// `type.getCategory() == MISC`.
    MiscCategory,
    /// `!canSpawnFarFromPlayer && distSqr > despawnDistance²`.
    TooFarFromPlayer,
    /// `!type.canSummon()`, or `canSpawnMobAt` said the biome's list for
    /// this position does not hold the drawn `SpawnerData`.
    NotSummonableHere,
    /// `SpawnPlacements.isSpawnPositionOk`.
    Placement,
    /// `SpawnPlacements.checkSpawnRules`.
    SpawnRules,
    /// The registered predicate is one of [`super::rules::PENDING`], named
    /// here rather than answered.
    PredicateUntranscribed { predicate: String },
    /// `level.noCollision(type.getSpawnAABB(x + 0.5, y, z + 0.5))`.
    Collision,
    /// The type is in a biome spawner list but not in
    /// `entity_spawn_facts.json`, which cannot happen with the generated
    /// tables and is a refusal rather than a default.
    UnknownType,
}

/// `NaturalSpawner.spawnForChunk(level, chunk, state, categories)`.
///
/// The category order is the list's, which
/// `getFilteredSpawningCategories` built by walking `SPAWNING_CATEGORIES`.
/// The local cap is consulted **per category, per chunk**, before the
/// position is even drawn — so a category the local cap refuses costs no
/// RNG at all, and that is three draws' difference per chunk per category.
#[allow(clippy::too_many_arguments)]
pub fn spawn_for_chunk<L: SpawnLevel + ?Sized, H: SpawnHost, R: SpawnRandom>(
    level: &L,
    chunk: ChunkPos,
    state: &mut SpawnState,
    categories: &[MobCategory],
    host: &mut H,
    rng: &mut R,
    tables: &SpawnTables,
    log: &mut Vec<SpawnEvent>,
) {
    for &category in categories {
        if !state.can_spawn_for_category_local(category, chunk) {
            continue;
        }
        // The state is borrowed by the filter for the duration of one
        // category, exactly as vanilla's two method references are.
        let mut filter = StateFilter {
            state,
            level,
            tables,
        };
        spawn_category_for_chunk(category, level, chunk, &mut filter, host, rng, tables, log);
    }
}

/// `NaturalSpawner.spawnCategoryForChunk(category, level, chunk, filter,
/// callback)`.
#[allow(clippy::too_many_arguments)]
pub fn spawn_category_for_chunk<
    L: SpawnLevel + ?Sized,
    F: SpawnFilter,
    H: SpawnHost,
    R: SpawnRandom,
>(
    category: MobCategory,
    level: &L,
    chunk: ChunkPos,
    filter: &mut F,
    host: &mut H,
    rng: &mut R,
    tables: &SpawnTables,
    log: &mut Vec<SpawnEvent>,
) {
    let pos = random_pos_within(level, chunk, rng);
    log.push(SpawnEvent::ChunkPos { pos });
    if pos.y < level.min_y() + 1 {
        log.push(SpawnEvent::BelowMinY);
        return;
    }
    spawn_category_for_position(category, level, chunk, pos, filter, host, rng, tables, log);
}

/// `NaturalSpawner.getRandomPosWithin(level, chunk)`.
///
/// **Three draws, in this order**, and they are taken before any gate:
/// `nextInt(16)` for x, `nextInt(16)` for z, then
/// `Mth.randomBetweenInclusive(random, level.getMinY(), height + 1)` where
/// `height` is the chunk's `WORLD_SURFACE` heightmap at that column. The
/// y is uniform over the **whole column**, sea floor to one above the
/// surface, which is why most drawn positions are underground.
pub fn random_pos_within<L: SpawnLevel + ?Sized, R: SpawnRandom>(
    level: &L,
    chunk: ChunkPos,
    rng: &mut R,
) -> BlockPos {
    let x = (chunk.x << 4) + rng.next_i32_bound(16);
    let z = (chunk.z << 4) + rng.next_i32_bound(16);
    let top = level.world_surface_height(x, z) + 1;
    let y = rng.next_i32_between_inclusive(level.min_y(), top);
    BlockPos { x, y, z }
}

/// `NaturalSpawner.spawnCategoryForPosition(category, level, chunk, pos,
/// filter, callback)` — the six-argument one, which is the whole spawner.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub fn spawn_category_for_position<
    L: SpawnLevel + ?Sized,
    F: SpawnFilter,
    H: SpawnHost,
    R: SpawnRandom,
>(
    category: MobCategory,
    level: &L,
    chunk: ChunkPos,
    pos: BlockPos,
    filter: &mut F,
    host: &mut H,
    rng: &mut R,
    tables: &SpawnTables,
    log: &mut Vec<SpawnEvent>,
) {
    let y = pos.y;
    if level.block_facts(pos).is_redstone_conductor {
        log.push(SpawnEvent::RedstoneConductor);
        return;
    }
    // `spawnedThisCall` spans all three attempts — vanilla's local 11 is
    // declared outside the outer loop, and it is what the cluster-size
    // check reads.
    let mut spawned_this_call = 0i32;
    for attempt in 0..SPAWN_ATTEMPTS {
        let mut x = pos.x;
        let mut z = pos.z;
        let mut data: Option<SpawnerData> = None;
        // `Mth.ceil(random.nextFloat() * 4.0f)`: 1..=4, and 0 only if the
        // float is exactly 0.0f, which `nextFloat` can return.
        let mut pack_size = crate::mth::ceil(f64::from(rng.next_f32() * 4.0f32));
        log.push(SpawnEvent::Attempt {
            index: attempt,
            pack_size,
        });
        let mut group_count = 0i32;
        let mut member = 0i32;
        // The bound is re-read every iteration, so the `pack_size`
        // reassignment below genuinely lengthens or shortens the loop
        // mid-flight.
        while member < pack_size {
            x += rng.next_i32_bound(SPREAD) - rng.next_i32_bound(SPREAD);
            z += rng.next_i32_bound(SPREAD) - rng.next_i32_bound(SPREAD);
            let here = BlockPos { x, y, z };
            log.push(SpawnEvent::Position { pos: here });
            let cx = f64::from(x) + 0.5;
            let cz = f64::from(z) + 0.5;
            let Some((px, py, pz)) = level.nearest_player(cx, f64::from(y), cz) else {
                log.push(SpawnEvent::NoPlayer);
                member += 1;
                continue;
            };
            // `Player.distanceToSqr(x, y, z)` — the y term uses the raw
            // block y, not y + 0.5.
            let dx = px - cx;
            let dy = py - f64::from(y);
            let dz = pz - cz;
            let dist_sqr = dx * dx + dy * dy + dz * dz;
            if !is_right_distance_to_player_and_spawn_point(level, chunk, here, dist_sqr) {
                log.push(SpawnEvent::WrongDistance {
                    player_dist_sqr: dist_sqr,
                });
                member += 1;
                continue;
            }
            if data.is_none() {
                let Some(picked) = random_spawn_mob_at(level, category, here, rng, tables) else {
                    log.push(SpawnEvent::NoMob);
                    break;
                };
                // `minCount + nextInt(1 + maxCount - minCount)`.
                pack_size =
                    picked.min_count + rng.next_i32_bound(1 + picked.max_count - picked.min_count);
                log.push(SpawnEvent::Picked {
                    data: picked.clone(),
                    pack_size,
                });
                data = Some(picked);
            }
            let picked = data.as_ref().expect("set on the branch above");
            if let Err(gate) =
                valid_spawn_position_for_type(level, picked, here, dist_sqr, category, rng, tables)
            {
                log.push(SpawnEvent::BadPositionForType { gate });
                member += 1;
                continue;
            }
            if !filter.test(&picked.kind, here) {
                log.push(SpawnEvent::FilteredOut);
                member += 1;
                continue;
            }
            if !host.create_mob(&picked.kind) {
                log.push(SpawnEvent::CreateFailed);
                return;
            }
            let yaw = rng.next_f32() * 360.0f32;
            let placed = match host.place(SpawnRequest {
                kind: &picked.kind,
                x: cx,
                y: f64::from(y),
                z: cz,
                yaw,
                player_dist_sqr: dist_sqr,
                first_of_group: group_count == 0,
            }) {
                None => {
                    log.push(SpawnEvent::MobPositionRejected);
                    member += 1;
                    continue;
                }
                Some(p) => p,
            };
            spawned_this_call += 1;
            group_count += 1;
            log.push(SpawnEvent::Spawned {
                kind: picked.kind.clone(),
                pos: placed.block_pos,
                yaw,
            });
            filter.after_spawn(&picked.kind, placed.block_pos);
            if spawned_this_call >= placed.max_spawn_cluster_size {
                log.push(SpawnEvent::ClusterFull);
                return;
            }
            if placed.max_group_size_reached {
                log.push(SpawnEvent::GroupFull);
                break;
            }
            member += 1;
        }
    }
}

/// `NaturalSpawner.isRightDistanceToPlayerAndSpawnPoint`.
///
/// Three gates:
///
/// 1. `distSqr <= 576.0` -> false. **`dcmpg; ifgt`**, so exactly 576.0
///    fails and the rule is "strictly more than 24 blocks";
/// 2. same dimension as the respawn point **and**
///    `respawnPos.closerToCenterThan(Vec3(x + 0.5, y, z + 0.5), 24.0)` ->
///    false. Note the asymmetry: `closerToCenterThan` adds `+0.5` to all
///    three of the *respawn* block's coordinates, while the vector it is
///    given carries `+0.5` on x and z only. The y term is therefore
///    `(spawnY + 0.5 - y)²`, half a block off from the x and z terms, and
///    that is vanilla's arithmetic and not a transcription slip;
/// 3. the position's chunk must be the chunk being spawned for, or
///    `canSpawnEntitiesInChunk` must accept it. The walk in
///    `spawnCategoryForPosition` can leave the chunk — up to 5 blocks per
///    member and 4 members before the pack size is known — so this is
///    reachable, not defensive.
#[must_use]
pub fn is_right_distance_to_player_and_spawn_point<L: SpawnLevel + ?Sized>(
    level: &L,
    chunk: ChunkPos,
    pos: BlockPos,
    player_dist_sqr: f64,
) -> bool {
    // `dcmpg; ifgt` — the branch is taken only when the comparison is
    // strictly greater, and NaN takes the refusal arm. Written as
    // `<=` plus an `is_nan` rather than a negated `>` so that clippy's
    // partial-order lint is satisfied without changing the answer: a NaN
    // distance is impossible here (both operands are finite) and is
    // refused either way.
    if player_dist_sqr.is_nan() || player_dist_sqr <= MIN_PLAYER_DISTANCE_SQR {
        return false;
    }
    let respawn = level.respawn_data();
    if respawn.dimension == level.dimension() {
        let dx = f64::from(respawn.pos.x) + 0.5 - (f64::from(pos.x) + 0.5);
        let dy = f64::from(respawn.pos.y) + 0.5 - f64::from(pos.y);
        let dz = f64::from(respawn.pos.z) + 0.5 - (f64::from(pos.z) + 0.5);
        let d = dx * dx + dy * dy + dz * dz;
        if d < f64::from(MIN_SPAWN_DISTANCE) * f64::from(MIN_SPAWN_DISTANCE) {
            return false;
        }
    }
    let cp = ChunkPos::containing(pos);
    cp == chunk || level.can_spawn_entities_in_chunk(cp.x, cp.z)
}

/// `NaturalSpawner.getRandomSpawnMobAt(level, sm, gen, category, random,
/// pos)`.
///
/// The `WATER_AMBIENT` short-circuit is a **draw with a 98% refusal**:
/// in a biome tagged `reduced_water_ambient_spawns`, `nextFloat() < 0.98f`
/// returns empty. So that biome tag costs one `nextFloat` on every
/// `WATER_AMBIENT` attempt whether or not it fires, and the surviving 2%
/// then take the weighted draw as well.
pub fn random_spawn_mob_at<L: SpawnLevel + ?Sized, R: SpawnRandom>(
    level: &L,
    category: MobCategory,
    pos: BlockPos,
    rng: &mut R,
    tables: &SpawnTables,
) -> Option<SpawnerData> {
    let biome = level.biome(pos);
    if category == MobCategory::WaterAmbient
        && level.biome_has_reduced_water_ambient_spawns(pos)
        && rng.next_f32() < 0.98f32
    {
        return None;
    }
    mobs_at(level, category, pos, Some(&biome), tables).and_then(|list| {
        // Cloned because vanilla returns the shared record and this port
        // hands it up the stack; the record is immutable either way.
        list.get_random(rng).cloned()
    })
}

/// `NaturalSpawner.mobsAt(level, sm, gen, category, pos, biome)`.
///
/// Three sources, in this order:
///
/// 1. inside a nether fortress and `category == MONSTER` ->
///    `NetherFortressStructure.FORTRESS_ENEMIES`, which is not any
///    biome's list;
/// 2. otherwise `ChunkGenerator.getMobsAt`, which walks
///    `StructureManager.getAllStructuresAt(pos)` looking for a
///    `StructureSpawnOverride` for this category — the swamp hut's witch,
///    the monument's guardian, the outpost's pillager. **That query is
///    refused here**: [`SpawnLevel::structure_spawn_override`] is the
///    caller's, and this branch has no structure manager;
/// 3. otherwise the biome's own list.
///
/// A `None` return means the type is not in the tables at all, which the
/// generated tables cannot produce.
pub fn mobs_at<'a, L: SpawnLevel + ?Sized>(
    level: &'a L,
    category: MobCategory,
    pos: BlockPos,
    biome: Option<&str>,
    tables: &'a SpawnTables,
) -> Option<&'a WeightedSpawnList> {
    if category == MobCategory::Monster && level.in_nether_fortress_bounds(pos) {
        return Some(level.fortress_enemies());
    }
    if let Some(over) = level.structure_spawn_override(pos, category) {
        return Some(over);
    }
    let name = match biome {
        Some(b) => b.to_owned(),
        None => level.biome(pos),
    };
    tables.biome(&name).map(|b| b.mobs(category))
}

/// `NaturalSpawner.canSpawnMobAt` — `mobsAt(…, null).contains(data)`.
///
/// The `null` biome is not a detail: it makes `mobsAt` re-read
/// `level.getBiome(pos)` rather than reuse the one
/// `getRandomSpawnMobAt` already had. The position has moved between the
/// two calls in general, so the list checked here is not always the list
/// drawn from.
#[must_use]
pub fn can_spawn_mob_at<L: SpawnLevel + ?Sized>(
    level: &L,
    category: MobCategory,
    data: &SpawnerData,
    pos: BlockPos,
    tables: &SpawnTables,
) -> bool {
    mobs_at(level, category, pos, None, tables).is_some_and(|l| l.contains(data))
}

/// `NaturalSpawner.isValidSpawnPostitionForType` — vanilla's spelling.
///
/// Returns which gate refused rather than a bool, so the decision log can
/// carry it.
///
/// # Errors
/// The [`PositionGate`] that refused.
#[allow(clippy::too_many_arguments)]
pub fn valid_spawn_position_for_type<L: SpawnLevel + ?Sized, R: SpawnRandom>(
    level: &L,
    data: &SpawnerData,
    pos: BlockPos,
    player_dist_sqr: f64,
    category: MobCategory,
    rng: &mut R,
    tables: &SpawnTables,
) -> Result<(), PositionGate> {
    let Some(facts) = tables.facts(&data.kind) else {
        return Err(PositionGate::UnknownType);
    };
    if facts.category == MobCategory::Misc {
        return Err(PositionGate::MiscCategory);
    }
    // The despawn distance is the TYPE's category's, not the category
    // being spawned for — which differ whenever a biome lists a type
    // outside the category it is listed under, and 26.2 does exactly that:
    // `minecraft:ocelot` (CREATURE) is in the `monster` list of `jungle`
    // and `bamboo_jungle`.
    if !facts.can_spawn_far_from_player {
        let d = f64::from(facts.category.despawn_distance())
            * f64::from(facts.category.despawn_distance());
        if player_dist_sqr > d {
            return Err(PositionGate::TooFarFromPlayer);
        }
    }
    // `if (!type.canSummon() || !canSpawnMobAt(...)) return false;` —
    // offsets 54-77. `canSummon` false is a refusal, not a skip of the
    // second test.
    if !facts.can_summon || !can_spawn_mob_at(level, category, data, pos, tables) {
        return Err(PositionGate::NotSummonableHere);
    }
    if !placements::is_spawn_position_ok(&data.kind, level, pos, tables) {
        return Err(PositionGate::Placement);
    }
    match placements::check_spawn_rules(
        &data.kind,
        level,
        EntitySpawnReason::Natural,
        pos,
        rng,
        tables,
    ) {
        PredicateVerdict::Allowed => {}
        PredicateVerdict::Refused => return Err(PositionGate::SpawnRules),
        PredicateVerdict::Untranscribed => {
            let predicate = tables
                .placement(&data.kind)
                .map_or_else(String::new, |r| r.predicate.clone());
            return Err(PositionGate::PredicateUntranscribed { predicate });
        }
    }
    // `EntityType.getSpawnAABB(x + 0.5, y, z + 0.5)`: half-width is
    // `spawnDimensionsScale * width / 2` and the height is
    // `spawnDimensionsScale * height`, both in **f32** before widening —
    // the multiply and divide happen at float precision and only the
    // subtraction from the position is double.
    let hw = f64::from(facts.spawn_dimensions_scale * facts.width / 2.0);
    let h = f64::from(facts.spawn_dimensions_scale * facts.height);
    let cx = f64::from(pos.x) + 0.5;
    let cy = f64::from(pos.y);
    let cz = f64::from(pos.z) + 0.5;
    let aabb = crate::phys::Aabb::new(cx - hw, cy, cz - hw, cx + hw, cy + h, cz + hw);
    if !level.no_collision(aabb) {
        return Err(PositionGate::Collision);
    }
    Ok(())
}
