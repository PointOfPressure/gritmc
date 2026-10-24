//! A synthetic level and a recording host, for tests only.
//!
//! **Everything here is this port's reading and none of it is the jar's.**
//! A test that drives [`GridSpawnLevel`] checks that the transcription is
//! internally consistent — that the draws happen in the order the doc
//! comments claim and that each gate refuses what it says it refuses. It
//! cannot check that vanilla would agree, because no vanilla capture of
//! natural spawning exists on this box: see
//! [`super`]'s module docs for why, with the numbers.

use std::collections::BTreeMap;

use rustmc_data::{BlockPos, Dimension};

use super::host::{
    Difficulty, LightLayer, PlacedMob, RespawnData, SpawnBlockFacts, SpawnHost, SpawnLevel,
    SpawnRandom, SpawnRequest,
};
use super::tables::WeightedSpawnList;
use super::MobCategory;
use crate::phys::Aabb;

/// A flat world answered from fields.
#[derive(Debug)]
pub struct GridSpawnLevel {
    pub min_y: i32,
    pub sea_level: i32,
    pub dimension: Dimension,
    pub respawn: RespawnData,
    pub difficulty: Difficulty,
    pub thundering: bool,
    /// The `WORLD_SURFACE` height for every column.
    pub surface_height: i32,
    /// Per-position overrides; anything not listed gets [`Self::default_facts`].
    pub facts: BTreeMap<(i32, i32, i32), SpawnBlockFacts>,
    pub default_facts: SpawnBlockFacts,
    pub biome: String,
    pub rough_biome: String,
    pub players: Vec<(f64, f64, f64)>,
    pub sky_brightness: i32,
    pub block_brightness: i32,
    pub max_local_raw: i32,
    pub raw_brightness: i32,
    pub can_see_sky: bool,
    pub monster_spawn_block_light_limit: i32,
    /// What `monsterSpawnLightTest.sample(random)` returns. The provider
    /// still **draws** — the overworld's is `UniformInt(0, 7)`, one
    /// `nextInt(8)` — so a test that wants the real stream must set
    /// [`Self::light_test_draws`].
    pub monster_spawn_light_test: i32,
    pub light_test_draws: bool,
    pub no_collision: bool,
    pub within_border: bool,
    pub valid_spawn_below: bool,
    pub block_dangerous: bool,
    pub in_fortress: bool,
    pub reduced_water_ambient: bool,
    pub fortress_enemies: WeightedSpawnList,
    pub chunk_spawn_allowed: bool,
}

impl Default for GridSpawnLevel {
    fn default() -> Self {
        Self {
            min_y: -64,
            sea_level: 63,
            dimension: Dimension::Overworld,
            respawn: RespawnData {
                dimension: Dimension::Overworld,
                pos: BlockPos { x: 0, y: 64, z: 0 },
            },
            difficulty: Difficulty::Normal,
            thundering: false,
            surface_height: 64,
            facts: BTreeMap::new(),
            default_facts: SpawnBlockFacts {
                fluid_is_empty: true,
                ..SpawnBlockFacts::default()
            },
            biome: "plains".into(),
            rough_biome: "plains".into(),
            players: Vec::new(),
            sky_brightness: 0,
            block_brightness: 0,
            max_local_raw: 0,
            raw_brightness: 15,
            can_see_sky: true,
            monster_spawn_block_light_limit: 0,
            monster_spawn_light_test: 7,
            light_test_draws: true,
            no_collision: true,
            within_border: true,
            valid_spawn_below: true,
            block_dangerous: false,
            in_fortress: false,
            reduced_water_ambient: false,
            fortress_enemies: WeightedSpawnList::default(),
            chunk_spawn_allowed: true,
        }
    }
}

impl GridSpawnLevel {
    /// Override the facts at one position.
    pub fn set_facts(&mut self, pos: BlockPos, facts: SpawnBlockFacts) {
        self.facts.insert((pos.x, pos.y, pos.z), facts);
    }
}

impl SpawnLevel for GridSpawnLevel {
    fn min_y(&self) -> i32 {
        self.min_y
    }
    fn sea_level(&self) -> i32 {
        self.sea_level
    }
    fn dimension(&self) -> Dimension {
        self.dimension
    }
    fn respawn_data(&self) -> RespawnData {
        self.respawn
    }
    fn difficulty(&self) -> Difficulty {
        self.difficulty
    }
    fn is_thundering(&self) -> bool {
        self.thundering
    }
    fn world_surface_height(&self, _x: i32, _z: i32) -> i32 {
        self.surface_height
    }
    fn block_facts(&self, pos: BlockPos) -> SpawnBlockFacts {
        self.facts
            .get(&(pos.x, pos.y, pos.z))
            .copied()
            .unwrap_or(self.default_facts)
    }
    fn biome(&self, _pos: BlockPos) -> String {
        self.biome.clone()
    }
    fn rough_biome(&self, _pos: BlockPos) -> String {
        self.rough_biome.clone()
    }
    fn nearest_player(&self, x: f64, y: f64, z: f64) -> Option<(f64, f64, f64)> {
        // `getNearestPlayer(…, -1.0, false)` has no distance limit, so the
        // nearest by squared distance wins outright. Ties go to the first
        // in the list, which is this fixture's order and not vanilla's
        // player list order — a test that depends on a tie is testing the
        // fixture.
        self.players.iter().copied().min_by(|a, b| {
            let da = (a.0 - x).powi(2) + (a.1 - y).powi(2) + (a.2 - z).powi(2);
            let db = (b.0 - x).powi(2) + (b.1 - y).powi(2) + (b.2 - z).powi(2);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
    }
    fn can_spawn_entities_in_chunk(&self, _x: i32, _z: i32) -> bool {
        self.chunk_spawn_allowed
    }
    fn no_collision(&self, _aabb: Aabb) -> bool {
        self.no_collision
    }
    fn is_within_world_border(&self, _pos: BlockPos) -> bool {
        self.within_border
    }
    fn brightness(&self, layer: LightLayer, _pos: BlockPos) -> i32 {
        match layer {
            LightLayer::Sky => self.sky_brightness,
            LightLayer::Block => self.block_brightness,
        }
    }
    fn max_local_raw_brightness(&self, _pos: BlockPos, _amount: Option<i32>) -> i32 {
        self.max_local_raw
    }
    fn raw_brightness(&self, _pos: BlockPos, _amount: i32) -> i32 {
        self.raw_brightness
    }
    fn can_see_sky(&self, _pos: BlockPos) -> bool {
        self.can_see_sky
    }
    fn monster_spawn_block_light_limit(&self) -> i32 {
        self.monster_spawn_block_light_limit
    }
    fn sample_monster_spawn_light_test(&self, rng: &mut dyn SpawnRandom) -> i32 {
        if self.light_test_draws {
            // `UniformInt(0, 7).sample` is `randomBetweenInclusive(0, 7)`,
            // one draw. Taken and discarded so the stream matches even
            // when the fixture pins the value.
            let _ = rng.next_i32_between_inclusive(0, 7);
        }
        self.monster_spawn_light_test
    }
    fn is_valid_spawn(&self, _pos: BlockPos, _kind: &str) -> bool {
        self.valid_spawn_below
    }
    fn is_block_dangerous(&self, _pos: BlockPos, _kind: &str) -> bool {
        self.block_dangerous
    }
    fn in_nether_fortress_bounds(&self, _pos: BlockPos) -> bool {
        self.in_fortress
    }
    fn fortress_enemies(&self) -> &WeightedSpawnList {
        &self.fortress_enemies
    }
    fn biome_has_reduced_water_ambient_spawns(&self, _pos: BlockPos) -> bool {
        self.reduced_water_ambient
    }
    fn structure_spawn_override(
        &self,
        _pos: BlockPos,
        _category: MobCategory,
    ) -> Option<&WeightedSpawnList> {
        None
    }
}

/// A host that records what it was asked to place and answers from fields.
#[derive(Debug, Default)]
pub struct RecordingHost {
    /// Every `(kind, x, y, z, yaw, first_of_group)` handed to `place`.
    pub placed: Vec<(String, f64, f64, f64, f32, bool)>,
    /// Kinds `create_mob` refuses, i.e. `getMobForSpawn` returns null.
    pub uncreatable: Vec<String>,
    /// Positions `isValidPositionForMob` rejects, by index into
    /// [`Self::placed`] as it would have been.
    pub reject_nth: Vec<usize>,
    pub max_spawn_cluster_size: i32,
    pub max_group_size: i32,
    attempts: usize,
    group: i32,
}

impl RecordingHost {
    #[must_use]
    pub fn new(max_spawn_cluster_size: i32, max_group_size: i32) -> Self {
        Self {
            max_spawn_cluster_size,
            max_group_size,
            ..Self::default()
        }
    }
}

impl SpawnHost for RecordingHost {
    fn create_mob(&mut self, kind: &str) -> bool {
        !self.uncreatable.iter().any(|k| k == kind)
    }

    fn place(&mut self, request: SpawnRequest<'_>) -> Option<PlacedMob> {
        let n = self.attempts;
        self.attempts += 1;
        if self.reject_nth.contains(&n) {
            return None;
        }
        if request.first_of_group {
            self.group = 0;
        }
        self.group += 1;
        self.placed.push((
            request.kind.to_owned(),
            request.x,
            request.y,
            request.z,
            request.yaw,
            request.first_of_group,
        ));
        Some(PlacedMob {
            max_spawn_cluster_size: self.max_spawn_cluster_size,
            max_group_size_reached: self.group >= self.max_group_size,
            block_pos: BlockPos {
                #[allow(clippy::cast_possible_truncation)]
                x: request.x.floor() as i32,
                #[allow(clippy::cast_possible_truncation)]
                y: request.y.floor() as i32,
                #[allow(clippy::cast_possible_truncation)]
                z: request.z.floor() as i32,
            },
        })
    }
}
