//! The three generated tables, loaded.
//!
//! Nothing in this module is a constant this crate chose. Every number
//! comes from `data/vanilla/biome_spawners.json`,
//! `data/vanilla/spawn_placements.json` or
//! `data/vanilla/entity_spawn_facts.json`, each written by a generator that
//! cross-checks itself against a second source before it writes anything.

use std::collections::BTreeMap;
use std::path::Path;

use super::host::SpawnRandom;
use super::MobCategory;

/// Anything that can go wrong loading a table. A missing file is an error
/// and never a default: a spawner that silently ran on an empty biome
/// table would spawn nothing and look exactly like a correct server in a
/// mob-free world.
#[derive(Debug)]
pub enum TableError {
    Io(std::io::Error),
    Json(serde_json::Error),
    /// The file parsed but says something this loader refuses to guess at.
    Shape(String),
}

impl std::fmt::Display for TableError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Json(e) => write!(f, "{e}"),
            Self::Shape(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for TableError {}

impl From<std::io::Error> for TableError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for TableError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

/// `MobSpawnSettings$SpawnerData` — a **record of three fields**, and the
/// weight is not one of them.
///
/// The weight lives in the `Weighted<SpawnerData>` wrapper outside it, so
/// `WeightedList.contains(data)` — which is the whole of
/// `NaturalSpawner.canSpawnMobAt` — compares `(type, minCount, maxCount)`
/// and ignores the weight. Modelling the weight as a field of this struct
/// would make `canSpawnMobAt` stricter than vanilla's.
///
/// The record's constructor also **substitutes `EntityTypes.PIG` for any
/// type whose category is `MISC`** (offsets 4-21). No spawner row in 26.2's
/// own data names a MISC type — checked over all 795 rows — so the
/// substitution is unreachable from vanilla data and is recorded here
/// rather than implemented; a datapack could reach it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SpawnerData {
    pub kind: String,
    pub min_count: i32,
    pub max_count: i32,
}

/// One `Weighted<SpawnerData>` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeightedSpawnerData {
    pub weight: i32,
    pub data: SpawnerData,
}

/// `net.minecraft.util.random.WeightedList<SpawnerData>`.
///
/// # The two selectors are one function
///
/// Vanilla picks `Flat` under a total weight of 64 and `Compact` at or
/// above it, and they compute the same thing: `Flat` fills an array of
/// `totalWeight` slots with each value repeated `weight` times **in list
/// order** and indexes it, `Compact` walks the list subtracting weights
/// until the roll goes negative. Same mapping from roll to value, so one
/// implementation is faithful to both and the 64 threshold is an
/// allocation decision with no observable consequence.
///
/// # A zero-weight list draws nothing
///
/// `WeightedList.<init>` leaves `selector` null when the total weight is
/// zero, and `getRandom` returns `Optional.empty()` **before** calling
/// `nextInt`. So an empty biome category costs no RNG, and a port that
/// drew first and checked after would desync the level's shared stream on
/// every empty category of every chunk — which is most of them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WeightedSpawnList {
    entries: Vec<WeightedSpawnerData>,
    total_weight: i32,
}

impl WeightedSpawnList {
    #[must_use]
    pub fn new(entries: Vec<WeightedSpawnerData>) -> Self {
        // `WeightedRandom.getTotalWeight` sums into an int with no
        // overflow check; vanilla's data cannot come near it.
        let total_weight = entries
            .iter()
            .map(|e| e.weight)
            .fold(0i32, i32::wrapping_add);
        Self {
            entries,
            total_weight,
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.total_weight == 0
    }

    #[must_use]
    pub fn total_weight(&self) -> i32 {
        self.total_weight
    }

    #[must_use]
    pub fn entries(&self) -> &[WeightedSpawnerData] {
        &self.entries
    }

    /// `getRandom(RandomSource)`. One `nextInt(totalWeight)` draw, or none
    /// at all on a zero-weight list.
    pub fn get_random<R: SpawnRandom + ?Sized>(&self, rng: &mut R) -> Option<&SpawnerData> {
        if self.total_weight == 0 {
            return None;
        }
        let mut roll = rng.next_i32_bound(self.total_weight);
        for e in &self.entries {
            roll -= e.weight;
            if roll < 0 {
                return Some(&e.data);
            }
        }
        // `Compact.get` throws `IllegalStateException` here. It is
        // unreachable while `totalWeight` is the sum of the weights.
        None
    }

    /// `contains(E)` — record equality over `(type, minCount, maxCount)`.
    #[must_use]
    pub fn contains(&self, data: &SpawnerData) -> bool {
        self.entries.iter().any(|e| &e.data == data)
    }
}

/// `MobSpawnSettings$MobSpawnCost` — the `spawnPotentials` charge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MobSpawnCost {
    pub charge: f64,
    pub energy_budget: f64,
}

/// One biome's `MobSpawnSettings`.
#[derive(Debug, Clone, Default)]
pub struct BiomeSpawners {
    per_category: BTreeMap<MobCategory, WeightedSpawnList>,
    costs: BTreeMap<String, MobSpawnCost>,
}

impl BiomeSpawners {
    /// `getMobs(MobCategory)`. A category with no list is an empty list,
    /// which is `MobSpawnSettings`' own `getOrDefault(category,
    /// WeightedList.of())`.
    #[must_use]
    pub fn mobs(&self, category: MobCategory) -> &WeightedSpawnList {
        static EMPTY: WeightedSpawnList = WeightedSpawnList {
            entries: Vec::new(),
            total_weight: 0,
        };
        self.per_category.get(&category).unwrap_or(&EMPTY)
    }

    /// `getMobSpawnCost(EntityType)` — `null` for a type with no entry,
    /// which `SpawnState.canSpawn` reads as "unconditionally allowed".
    #[must_use]
    pub fn spawn_cost(&self, kind: &str) -> Option<MobSpawnCost> {
        self.costs.get(kind).copied()
    }
}

/// `SpawnPlacementTypes`' four values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnPlacementType {
    NoRestrictions,
    InWater,
    InLava,
    OnGround,
}

/// `Heightmap$Types`, the two `SpawnPlacements` uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeightmapType {
    MotionBlocking,
    MotionBlockingNoLeaves,
}

/// One `SpawnPlacements.register` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacementRow {
    pub placement: SpawnPlacementType,
    pub heightmap: HeightmapType,
    /// The declaring class of the `SpawnPredicate`, simple name.
    pub predicate_class: String,
    /// The static method the `invokedynamic`'s bootstrap names.
    pub predicate: String,
}

/// One `MobCategory` construction, read out of the enum's own static
/// initialiser by `ci/dump-entity-spawn-facts.sh`.
///
/// This exists so that [`super::MobCategory`]'s hand-written constants have
/// a jar-derived thing to be checked against. Nothing in the spawner reads
/// it at run time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryConstants {
    pub name: String,
    pub serialized: String,
    pub abbreviation: String,
    pub ordinal: i32,
    pub max_instances_per_chunk: i32,
    pub is_friendly: bool,
    pub is_persistent: bool,
    pub despawn_distance: i32,
}

/// The `EntityType` fields the spawner reads.
#[derive(Debug, Clone, PartialEq)]
pub struct EntitySpawnFacts {
    pub category: MobCategory,
    pub width: f32,
    pub height: f32,
    pub spawn_dimensions_scale: f32,
    pub can_spawn_far_from_player: bool,
    pub can_summon: bool,
    pub allowed_in_peaceful: bool,
    pub fire_immune: bool,
    /// The `immuneTo` block tag's constant name, `DEFAULT_IMMUNE_TO` when
    /// the builder never set one. Read by `EntityType.isBlockDangerous`,
    /// which this crate does not evaluate: see
    /// [`super::host::SpawnLevel::is_block_dangerous`].
    pub immune_to: String,
}

/// All three tables.
#[derive(Debug, Clone, Default)]
pub struct SpawnTables {
    biomes: BTreeMap<String, BiomeSpawners>,
    placements: BTreeMap<String, PlacementRow>,
    types: BTreeMap<String, EntitySpawnFacts>,
    categories: Vec<CategoryConstants>,
    no_despawn_distance: i32,
    default_placement: Option<SpawnPlacementType>,
    default_heightmap: Option<HeightmapType>,
}

fn as_i32(v: &serde_json::Value, what: &str) -> Result<i32, TableError> {
    v.as_i64()
        .and_then(|n| i32::try_from(n).ok())
        .ok_or_else(|| TableError::Shape(format!("{what} is not an i32")))
}

fn as_f64(v: &serde_json::Value, what: &str) -> Result<f64, TableError> {
    v.as_f64()
        .ok_or_else(|| TableError::Shape(format!("{what} is not a number")))
}

fn as_str<'a>(v: &'a serde_json::Value, what: &str) -> Result<&'a str, TableError> {
    v.as_str()
        .ok_or_else(|| TableError::Shape(format!("{what} is not a string")))
}

impl SpawnTables {
    /// Load all three. Every path is required — see [`TableError`].
    ///
    /// # Errors
    /// If any file is missing, unparseable, or names a placement,
    /// heightmap or `MobCategory` this loader does not know. An unknown
    /// name is refused rather than defaulted: a spawn placement silently
    /// read as `NO_RESTRICTIONS` would let a fish spawn on land.
    pub fn load(
        biome_spawners: &Path,
        spawn_placements: &Path,
        entity_spawn_facts: &Path,
    ) -> Result<Self, TableError> {
        let mut out = Self::default();
        out.load_biomes(biome_spawners)?;
        out.load_placements(spawn_placements)?;
        out.load_types(entity_spawn_facts)?;
        Ok(out)
    }

    fn load_biomes(&mut self, path: &Path) -> Result<(), TableError> {
        let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
        let biomes = raw["biomes"]
            .as_object()
            .ok_or_else(|| TableError::Shape("biome_spawners.json has no `biomes`".into()))?;
        for (name, body) in biomes {
            let mut per_category = BTreeMap::new();
            let spawners = body["spawners"]
                .as_object()
                .ok_or_else(|| TableError::Shape(format!("{name}: `spawners` is not an object")))?;
            for (cat, list) in spawners {
                let category = MobCategory::from_serialized_name(cat).ok_or_else(|| {
                    TableError::Shape(format!("{name}: unknown MobCategory `{cat}`"))
                })?;
                let arr = list
                    .as_array()
                    .ok_or_else(|| TableError::Shape(format!("{name}/{cat}: not an array")))?;
                let mut entries = Vec::with_capacity(arr.len());
                for e in arr {
                    entries.push(WeightedSpawnerData {
                        weight: as_i32(&e["weight"], "weight")?,
                        data: SpawnerData {
                            kind: as_str(&e["type"], "type")?.to_owned(),
                            min_count: as_i32(&e["min_count"], "min_count")?,
                            max_count: as_i32(&e["max_count"], "max_count")?,
                        },
                    });
                }
                per_category.insert(category, WeightedSpawnList::new(entries));
            }
            let mut costs = BTreeMap::new();
            if let Some(obj) = body["spawn_costs"].as_object() {
                for (kind, c) in obj {
                    costs.insert(
                        kind.clone(),
                        MobSpawnCost {
                            charge: as_f64(&c["charge"], "charge")?,
                            energy_budget: as_f64(&c["energy_budget"], "energy_budget")?,
                        },
                    );
                }
            }
            self.biomes.insert(
                name.clone(),
                BiomeSpawners {
                    per_category,
                    costs,
                },
            );
        }
        if self.biomes.is_empty() {
            return Err(TableError::Shape(
                "biome_spawners.json holds no biomes".into(),
            ));
        }
        Ok(())
    }

    fn load_placements(&mut self, path: &Path) -> Result<(), TableError> {
        let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
        self.default_placement = Some(placement_of(as_str(
            &raw["default_placement"],
            "default_placement",
        )?)?);
        self.default_heightmap = Some(heightmap_of(as_str(
            &raw["default_heightmap"],
            "default_heightmap",
        )?)?);
        let arr = raw["placements"]
            .as_array()
            .ok_or_else(|| TableError::Shape("spawn_placements.json has no `placements`".into()))?;
        for row in arr {
            self.placements.insert(
                as_str(&row["type"], "type")?.to_owned(),
                PlacementRow {
                    placement: placement_of(as_str(&row["placement"], "placement")?)?,
                    heightmap: heightmap_of(as_str(&row["heightmap"], "heightmap")?)?,
                    predicate_class: as_str(&row["predicate_class"], "predicate_class")?
                        .rsplit('.')
                        .next()
                        .unwrap_or_default()
                        .to_owned(),
                    predicate: as_str(&row["predicate"], "predicate")?.to_owned(),
                },
            );
        }
        if self.placements.is_empty() {
            return Err(TableError::Shape(
                "spawn_placements.json holds no registrations".into(),
            ));
        }
        Ok(())
    }

    fn load_types(&mut self, path: &Path) -> Result<(), TableError> {
        let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
        let arr = raw["types"]
            .as_array()
            .ok_or_else(|| TableError::Shape("entity_spawn_facts.json has no `types`".into()))?;
        for row in arr {
            let cat = as_str(&row["category"], "category")?;
            let category = MobCategory::from_serialized_name(cat)
                .ok_or_else(|| TableError::Shape(format!("unknown MobCategory `{cat}`")))?;
            self.types.insert(
                as_str(&row["type"], "type")?.to_owned(),
                EntitySpawnFacts {
                    category,
                    width: as_f64(&row["width"], "width")? as f32,
                    height: as_f64(&row["height"], "height")? as f32,
                    spawn_dimensions_scale: as_f64(
                        &row["spawn_dimensions_scale"],
                        "spawn_dimensions_scale",
                    )? as f32,
                    can_spawn_far_from_player: row["can_spawn_far_from_player"] == true,
                    can_summon: row["can_summon"] == true,
                    allowed_in_peaceful: row["allowed_in_peaceful"] == true,
                    fire_immune: row["fire_immune"] == true,
                    immune_to: as_str(&row["immune_to"], "immune_to")?.to_owned(),
                },
            );
        }
        if self.types.is_empty() {
            return Err(TableError::Shape(
                "entity_spawn_facts.json holds no types".into(),
            ));
        }
        self.no_despawn_distance = as_i32(&raw["no_despawn_distance"], "no_despawn_distance")?;
        let cats = raw["categories"].as_array().ok_or_else(|| {
            TableError::Shape("entity_spawn_facts.json has no `categories`".into())
        })?;
        for c in cats {
            self.categories.push(CategoryConstants {
                name: as_str(&c["name"], "name")?.to_owned(),
                serialized: as_str(&c["serialized"], "serialized")?.to_owned(),
                abbreviation: as_str(&c["abbreviation"], "abbreviation")?.to_owned(),
                ordinal: as_i32(&c["ordinal"], "ordinal")?,
                max_instances_per_chunk: as_i32(
                    &c["max_instances_per_chunk"],
                    "max_instances_per_chunk",
                )?,
                is_friendly: c["is_friendly"] == true,
                is_persistent: c["is_persistent"] == true,
                despawn_distance: as_i32(&c["despawn_distance"], "despawn_distance")?,
            });
        }
        if self.categories.is_empty() {
            return Err(TableError::Shape(
                "entity_spawn_facts.json holds no MobCategory constants".into(),
            ));
        }
        Ok(())
    }

    /// `MobCategory.values()` as the jar declares them, in ordinal order.
    #[must_use]
    pub fn categories(&self) -> &[CategoryConstants] {
        &self.categories
    }

    /// `MobCategory.getNoDespawnDistance()`, one value for the whole enum.
    #[must_use]
    pub fn no_despawn_distance(&self) -> i32 {
        self.no_despawn_distance
    }

    /// `biome.getMobSettings()` for a biome by registry name, `minecraft:`
    /// dropped.
    #[must_use]
    pub fn biome(&self, name: &str) -> Option<&BiomeSpawners> {
        self.biomes
            .get(name.strip_prefix("minecraft:").unwrap_or(name))
    }

    #[must_use]
    pub fn biome_count(&self) -> usize {
        self.biomes.len()
    }

    /// `SpawnPlacements.getPlacementType(type)` — the default for an
    /// unregistered type, from the table rather than from a literal here.
    #[must_use]
    pub fn placement_type(&self, kind: &str) -> SpawnPlacementType {
        self.placements
            .get(kind)
            .map_or(SpawnPlacementType::NoRestrictions, |r| r.placement)
    }

    /// `SpawnPlacements.getHeightmapType(type)`.
    #[must_use]
    pub fn heightmap_type(&self, kind: &str) -> HeightmapType {
        self.placements
            .get(kind)
            .map_or(HeightmapType::MotionBlockingNoLeaves, |r| r.heightmap)
    }

    /// The registered row, if any. `SpawnPlacements.checkSpawnRules`
    /// returns **true** for a type with no row, having already applied the
    /// peaceful gate.
    #[must_use]
    pub fn placement(&self, kind: &str) -> Option<&PlacementRow> {
        self.placements.get(kind)
    }

    #[must_use]
    pub fn placement_count(&self) -> usize {
        self.placements.len()
    }

    /// The `EntityType` facts for a type.
    #[must_use]
    pub fn facts(&self, kind: &str) -> Option<&EntitySpawnFacts> {
        self.types.get(kind)
    }

    #[must_use]
    pub fn type_count(&self) -> usize {
        self.types.len()
    }

    /// Every registered placement row, sorted by type — for tests that
    /// sweep the table.
    pub fn placements_iter(&self) -> impl Iterator<Item = (&String, &PlacementRow)> {
        self.placements.iter()
    }
}

fn placement_of(name: &str) -> Result<SpawnPlacementType, TableError> {
    Ok(match name {
        "NO_RESTRICTIONS" => SpawnPlacementType::NoRestrictions,
        "IN_WATER" => SpawnPlacementType::InWater,
        "IN_LAVA" => SpawnPlacementType::InLava,
        "ON_GROUND" => SpawnPlacementType::OnGround,
        other => {
            return Err(TableError::Shape(format!(
                "unknown SpawnPlacementType `{other}`"
            )))
        }
    })
}

fn heightmap_of(name: &str) -> Result<HeightmapType, TableError> {
    Ok(match name {
        "MOTION_BLOCKING" => HeightmapType::MotionBlocking,
        "MOTION_BLOCKING_NO_LEAVES" => HeightmapType::MotionBlockingNoLeaves,
        other => {
            return Err(TableError::Shape(format!(
                "unknown Heightmap$Types `{other}` in a SpawnPlacements row"
            )))
        }
    })
}
