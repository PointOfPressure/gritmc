//! Mobs in the world: the `Mob` / `LivingEntity` host for
//! `rustmc-entity`'s AI, the spawn paths, the per-tick loop body and the
//! packets that carry a mob to a client.
//!
//! # Where the pieces are
//!
//! * [`host`] — `ServerMob`, the fields of `Entity`, `LivingEntity`,
//!   `Mob`, `AgeableMob`, `Animal` and `Bat`, answering `MobHost`.
//! * [`level`] — `MobLevel`, the `ServerLevel` reads (`PathLevel`,
//!   collision, line of sight, sky darkness).
//! * [`living`] — `Mob.tick` down to `Entity.move`, transcribed.
//! * [`spawn`] — `SpawnEggItem.useOn`, `EntityType.create`/`spawn`, the
//!   `finalizeSpawn` chain, `SummonCommand.createEntity`.
//! * [`data`] — `SynchedEntityData` per kind; [`attributes`] —
//!   `AttributeMap` and `ClientboundUpdateAttributesPacket`.
//! * [`pig`], [`bat`] — the two kinds that stand in the world.
//!
//! # The loop this hangs off
//!
//! `ServerLevel.tick` (offsets 342-484): `ServerChunkCache.tick`
//! (`chunkMap.tick` → `ServerEntity.sendChanges`) at 342, `runBlockEvents`
//! at 360, **then** `entityTickList.forEach` at 471, then
//! `tickBlockEntities`. A mob therefore moves in tick N and its
//! `move_entity_*` goes out in tick N+1's `chunkMap.tick`, and the dirty
//! data it set in tick N (`sendDirtyEntityData`, `sendChanges` offset 1093,
//! before the interval gate) goes out at the start of N+1's `sendChanges`
//! for that entity. [`tick_all`] is called from the clock after
//! `Tracker::tick` for that reason — see `status.rs`.
//!
//! # What a client is sent, and what each check can grade
//!
//! ```text
//!   spawn tick     bundle[add_entity, set_entity_data{health, sound
//!                  variant, left-handed}]               (pairing)
//!   spawn tick     set_entity_data{the same, dirty}      (sendChanges 1093)
//!   next tick      update_attributes{movement_speed}     (removeFrost's
//!                                                        instance)
//!   4th tick on    move_entity_* / rotate_head / set_entity_motion
//!                  every 3 ticks (updateInterval)
//! ```
//!
//! The spawn yaw, the sound variant and every later movement come from
//! unseeded randoms — vanilla disagrees with itself on all of them
//! (`080` flaps 15/15 on the yaw). `update_attributes` on the next tick
//! is the one packet of a fresh mob every recording agrees on, and
//! `ci/checks/gameplay/081-*` grades it.
pub mod armor_stand;
pub mod attributes;
pub mod bat;
pub mod breed;
pub mod chicken;
pub mod combat;
pub mod convert;
pub mod cow;
pub mod data;
pub mod drops;
pub mod egg;
pub mod golem;
pub mod host;
pub mod interact;
pub mod leash;
pub mod level;
pub mod living;
pub mod pig;
pub mod sheep;
pub mod spawn;
pub mod variants;

use crate::entities::{Entities, Entity, EntityKind, KindIds};
use crate::tracker::{TrackedPlayer, Tracker};
use host::ServerMob;
use level::MobLevel;
use rustmc_data::BlockPos;
use rustmc_entity::ai::mob_ai::MobAi;
use rustmc_entity::host::{Difficulty, EntityClass, EntityFacts, MobHost};
use rustmc_entity::phys::Aabb;
use rustmc_entity::tick::MobCategory;
use rustmc_worldgen::rng::{LegacyRandomSource, RandomSource};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, OnceLock};

/// Which class chain a kind's `SynchedEntityData` and constructor follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataLayout {
    /// `Entity → LivingEntity → Mob → PathfinderMob → AgeableMob → Animal → Pig`.
    Pig,
    /// `… → Animal → AbstractCow → Cow`.
    Cow,
    /// `… → Animal → AbstractCow → MushroomCow`.
    Mooshroom,
    /// `… → Animal → Sheep`.
    Sheep,
    /// `… → Animal → Chicken`.
    Chicken,
    /// `Entity → LivingEntity → Mob → AmbientCreature → Bat`.
    Bat,
    /// `Entity → LivingEntity → Mob → PathfinderMob → AbstractGolem → IronGolem`.
    IronGolem,
    /// `… → AbstractGolem → SnowGolem`.
    SnowGolem,
    /// `… → AbstractGolem → CopperGolem`.
    CopperGolem,
    /// `Entity → LivingEntity → ArmorStand` — not a `Mob`.
    ArmorStand,
}

impl DataLayout {
    /// `this instanceof Animal` (and so `AgeableMob`): the pig, the two cows,
    /// the sheep and the chicken. Not the bat (`AmbientCreature`), not the
    /// golems (`AbstractGolem` is a `PathfinderMob` but no `AgeableMob`, so
    /// it has no `DATA_BABY_ID` or `AGE_LOCKED` — reading accessor 17 off an
    /// iron golem panicked the clock thread on train-g's first corpus run),
    /// and not the armor stand.
    #[must_use]
    pub const fn is_animal(self) -> bool {
        matches!(
            self,
            DataLayout::Pig
                | DataLayout::Cow
                | DataLayout::Mooshroom
                | DataLayout::Sheep
                | DataLayout::Chicken
        )
    }
}

/// A mob type: `EntityTypes.<KIND>`'s builder values plus the per-class
/// constants the tick reads virtually.
#[derive(Debug)]
pub struct MobKind {
    pub name: &'static str,
    /// The tracking constants `ChunkMap.addEntity` reads.
    pub entity_kind: EntityKind,
    pub width: f32,
    pub height: f32,
    pub eye_height: f32,
    pub baby_dimensions: Option<(f32, f32)>,
    pub baby_eye_height: Option<f32>,
    pub category: MobCategory,
    /// `EntityType.isAllowedInPeaceful()`.
    pub allowed_in_peaceful: bool,
    pub data: DataLayout,
    /// `Mob.getAmbientSoundInterval()`.
    pub ambient_sound_interval: i32,
    /// `LivingEntity.getSoundVolume()`.
    pub sound_volume: f32,
    /// A factor on `LivingEntity.getVoicePitch()` (`Bat`'s `0.95F`).
    pub voice_pitch_scale: f32,
    /// `LivingEntity.isPushable()`'s class-level answer (`Bat`: false).
    pub pushable: bool,
    /// `getMovementEmission().emitsSounds()`.
    pub emits_sounds: bool,
    /// `Mob.removeWhenFarAway(double)`.
    pub remove_when_far_away: bool,
    /// `instanceof Enemy` — the marker interface `IronGolem`'s and
    /// `SnowGolem`'s target selectors test.
    pub enemy: bool,
}

impl MobKind {
    /// The `TemptGoal` item predicate for this kind.
    #[must_use]
    pub fn is_tempting_item(&self, item: &str) -> bool {
        match self.data {
            DataLayout::Pig => pig::is_tempting_item(item),
            DataLayout::Cow | DataLayout::Mooshroom => cow::is_food(item),
            DataLayout::Sheep => sheep::is_food(item),
            DataLayout::Chicken => chicken::is_food(item),
            DataLayout::Bat
            | DataLayout::IronGolem
            | DataLayout::SnowGolem
            | DataLayout::CopperGolem
            | DataLayout::ArmorStand => false,
        }
    }

    /// `instanceof Mob` — false only for the armor stand, whose tick skips
    /// every `Mob` layer (see [`armor_stand`]).
    #[must_use]
    pub const fn is_mob(&self) -> bool {
        !matches!(self.data, DataLayout::ArmorStand)
    }
}

/// Every kind this server can put in the world.
pub static KINDS: [&MobKind; 10] = [
    &pig::PIG,
    &bat::BAT,
    &golem::IRON_GOLEM,
    &golem::SNOW_GOLEM,
    &golem::COPPER_GOLEM,
    &armor_stand::ARMOR_STAND,
    &cow::COW,
    &cow::MOOSHROOM,
    &sheep::SHEEP,
    &chicken::CHICKEN,
];

/// A kind by registry id.
#[must_use]
pub fn kind_by_name(name: &str) -> Option<&'static MobKind> {
    KINDS.iter().copied().find(|k| k.name == name)
}

/// `SpawnEggItem.getType(stack)`: the `ENTITY_DATA` component's type.
/// Every vanilla egg's default component names the type its item is
/// named after (`pig_spawn_egg` → `minecraft:pig`), which is how the
/// items are registered (`SpawnEggItem` under `<type>_spawn_egg`); the
/// items lane's `item_props.json` carries the component itself, and this
/// mapping is the same function of the name.
#[must_use]
pub fn kind_for_spawn_egg(item: &str) -> Option<&'static MobKind> {
    let base = item.strip_suffix("_spawn_egg")?;
    kind_by_name(base)
}

// ---- difficulty ----

static DIFFICULTY: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(1);

/// `Difficulty.getId()`: PEACEFUL 0, EASY 1, NORMAL 2, HARD 3.
#[must_use]
pub fn difficulty_id() -> i32 {
    i32::from(DIFFICULTY.load(std::sync::atomic::Ordering::Relaxed))
}

/// `level.getDifficulty()`. Vanilla's `server.properties` default is
/// `easy`; `--difficulty` sets it.
#[must_use]
pub fn difficulty() -> Difficulty {
    match DIFFICULTY.load(std::sync::atomic::Ordering::Relaxed) {
        0 => Difficulty::Peaceful,
        1 => Difficulty::Easy,
        2 => Difficulty::Normal,
        _ => Difficulty::Hard,
    }
}

/// `--difficulty <name>`.
///
/// # Errors
/// The offending name, for a name that is not one of the four.
pub fn set_difficulty(name: &str) -> Result<(), String> {
    let id = match name {
        "peaceful" => 0,
        "easy" => 1,
        "normal" => 2,
        "hard" => 3,
        other => return Err(other.to_string()),
    };
    DIFFICULTY.store(id, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

// ---- the level's random ----

/// `ServerLevel.random` — `RandomSource.create()`, one per level, shared by
/// every spawn draw and every ambient sound seed.
pub fn level_random<T>(f: impl FnOnce(&mut LegacyRandomSource) -> T) -> T {
    static RANDOM: OnceLock<Mutex<LegacyRandomSource>> = OnceLock::new();
    let m = RANDOM.get_or_init(|| Mutex::new(crate::dropitem::unique_random()));
    let mut r = m.lock().expect("level random");
    f(&mut r)
}

// ---- what a player holds, published from its connection thread ----

/// A player's main and off hand, by item id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlayerHands {
    pub main: Option<String>,
    pub off: Option<String>,
}

fn hands() -> &'static Mutex<HashMap<i32, PlayerHands>> {
    static HANDS: OnceLock<Mutex<HashMap<i32, PlayerHands>>> = OnceLock::new();
    HANDS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Called by the play loop once per frame with the player's hands as of
/// the packets handled so far, so a mob's `TemptGoal` can read them from
/// the clock thread.
pub fn publish_hands(entity_id: i32, main: Option<&str>, off: Option<&str>) {
    let next = PlayerHands {
        main: main.map(str::to_string),
        off: off.map(str::to_string),
    };
    let mut h = hands().lock().expect("hands");
    if h.get(&entity_id) != Some(&next) {
        h.insert(entity_id, next);
    }
}

fn known_movements() -> &'static Mutex<HashMap<i32, rustmc_entity::phys::Vec3>> {
    static K: OnceLock<Mutex<HashMap<i32, rustmc_entity::phys::Vec3>>> = OnceLock::new();
    K.get_or_init(|| Mutex::new(HashMap::new()))
}

/// `ServerGamePacketListenerImpl.handlePlayerKnownMovement(delta)`: the
/// position delta of the move just applied, which `ServerPlayer
/// .setKnownMovement` keeps until `handleClientTickEnd` finds a tick with no
/// movement (offsets 12-26) — a packet this harness never sends, so on the
/// corpus the last delta stands.
pub fn publish_known_movement(entity_id: i32, delta: (f64, f64, f64)) {
    known_movements().lock().expect("known movement").insert(
        entity_id,
        rustmc_entity::phys::Vec3::new(delta.0, delta.1, delta.2),
    );
}

/// `ServerPlayer.getKnownMovement()`: the last published delta, zero for a
/// player that has not moved.
#[must_use]
pub fn player_known_movement(entity_id: i32) -> rustmc_entity::phys::Vec3 {
    known_movements()
        .lock()
        .expect("known movement")
        .get(&entity_id)
        .copied()
        .unwrap_or(rustmc_entity::phys::Vec3::ZERO)
}

/// `ServerEntity.sendPairingData` 333-387: a leashed mob's
/// `ClientboundSetEntityLinkPacket` body, last in the pairing bundle.
#[must_use]
pub fn pairing_link(id: i32) -> Option<Vec<u8>> {
    let holder = Mobs::shared().with(id, |s| s.ai.mob.leash.map(|d| d.holder))??;
    Some(leash::set_entity_link_body(id, Some(holder)))
}

/// The hands of a player entity, or `None` for anything else.
#[must_use]
pub fn player_hands(entity_id: i32) -> Option<PlayerHands> {
    hands().lock().expect("hands").get(&entity_id).cloned()
}

// ---- item tags ----

fn item_tags() -> Option<&'static crate::recipes::ItemTags> {
    static TAGS: OnceLock<Option<crate::recipes::ItemTags>> = OnceLock::new();
    TAGS.get_or_init(|| {
        let path = table_path("data/vanilla/tags_item.json");
        let text = std::fs::read_to_string(path).ok()?;
        let doc: serde_json::Value = serde_json::from_str(&text).ok()?;
        crate::recipes::ItemTags::parse(&doc).ok()
    })
    .as_ref()
}

/// `stack.is(ItemTags.<TAG>)` by tag name.
#[must_use]
pub fn item_tag_contains(tag: &str, item: &str) -> bool {
    item_tags().is_some_and(|t| t.contains(tag, item))
}

fn table_path(rel: &str) -> std::path::PathBuf {
    let cwd_relative = std::path::Path::new(rel);
    if cwd_relative.exists() {
        cwd_relative.to_path_buf()
    } else {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(rel)
    }
}

// ---- the datapack side of the pig's variants ----

/// One `spawn_conditions` entry of a variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantSelector {
    pub priority: i32,
    /// The `minecraft:biome` condition's resolved set, `None` when the
    /// selector has no condition.
    pub biomes: Option<Vec<String>>,
}

/// A `pig_sound_variant`'s four sound events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PigSoundSet {
    pub ambient: &'static str,
    pub step: &'static str,
    /// `LivingEntity.getHurtSound(DamageSource)`, resolved through the pig's
    /// sound variant rather than through `LivingEntity`'s own
    /// `SoundEvents.GENERIC_HURT` fallback.
    pub hurt: &'static str,
    /// `LivingEntity.getDeathSound()`, likewise instead of `GENERIC_DEATH`.
    pub death: &'static str,
    pub eat: &'static str,
    pub baby_ambient: &'static str,
    pub baby_step: &'static str,
    pub baby_hurt: &'static str,
    pub baby_death: &'static str,
    pub baby_eat: &'static str,
}

/// `timeline/day.json`'s `gameplay/sky_light_level` track.
#[derive(Debug, Clone, PartialEq)]
pub struct SkyLightTrack {
    pub period_ticks: i64,
    /// `(ticks, value)` in file order.
    pub keyframes: Vec<(i32, f32)>,
}

impl SkyLightTrack {
    /// `KeyframeTrackSampler.sample(long)` for a `Float` track with the
    /// `linear` easing: `loopTicks` (`floorMod(t, period)`), the segment
    /// whose `toTicks` is first past `t` (the segments are the wrap from
    /// the last keyframe minus a period to the first, each adjacent pair,
    /// and the first plus a period after the last), `fromValue` at or
    /// before `fromTicks`, `toValue` at or after `toTicks`, else
    /// `Mth.lerp((t - from) / (to - from), fromValue, toValue)` in `float`.
    #[must_use]
    pub fn sample(&self, total_ticks: i64) -> f32 {
        let period = self.period_ticks;
        let t = total_ticks.rem_euclid(period);
        let k = &self.keyframes;
        let n = k.len();
        let (first, last) = (k[0], k[n - 1]);
        let mut segments: Vec<((i64, f32), (i64, f32))> = Vec::with_capacity(n + 1);
        segments.push((
            (i64::from(last.0) - period, last.1),
            (i64::from(first.0), first.1),
        ));
        for w in k.windows(2) {
            segments.push(((i64::from(w[0].0), w[0].1), (i64::from(w[1].0), w[1].1)));
        }
        segments.push((
            (i64::from(last.0), last.1),
            (i64::from(first.0) + period, first.1),
        ));
        let seg = segments
            .iter()
            .find(|(_, to)| t < to.0)
            .unwrap_or(&segments[segments.len() - 1]);
        let ((from_t, from_v), (to_t, to_v)) = *seg;
        if t <= from_t {
            return from_v;
        }
        if t >= to_t {
            return to_v;
        }
        #[expect(clippy::cast_precision_loss, reason = "vanilla's own l2f / i2f")]
        let f = (t - from_t) as f32 / (to_t - from_t) as f32;
        from_v + f * (to_v - from_v)
    }
}

/// `data/vanilla/mob_variants.json`.
#[derive(Debug)]
pub struct MobVariants {
    pub pig_variant: BTreeMap<String, Vec<VariantSelector>>,
    pub pig_sound_variant: BTreeMap<String, PigSoundSet>,
    pub sky_light_level: SkyLightTrack,
    /// `cow_variant`, `chicken_variant`: `pig_variant`'s shape.
    pub cow_variant: BTreeMap<String, Vec<VariantSelector>>,
    pub chicken_variant: BTreeMap<String, Vec<VariantSelector>>,
    /// `cow_sound_variant`: a `CowSoundVariant` has one set for every age.
    pub cow_sound_variant: BTreeMap<String, SoundSet>,
    /// `chicken_sound_variant`: `adultSounds` / `babySounds`.
    pub chicken_sound_variant: BTreeMap<String, SoundSet>,
    /// `#minecraft:spawns_warm_variant_farm_animals`, resolved.
    pub warm_farm_biomes: Vec<String>,
    /// `#minecraft:spawns_cold_variant_farm_animals`, resolved.
    pub cold_farm_biomes: Vec<String>,
}

/// A cow's or a chicken's sound set: the two sounds a living, unhurt mob
/// plays, adult and baby (a cow's baby pair repeats its adult pair).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoundSet {
    pub ambient: &'static str,
    pub step: &'static str,
    pub baby_ambient: &'static str,
    pub baby_step: &'static str,
}

fn parse_selectors(
    doc: &serde_json::Value,
    key: &str,
) -> std::io::Result<BTreeMap<String, Vec<VariantSelector>>> {
    let bad = |w: &str| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("mob_variants.json {key}: {w}"),
        )
    };
    let mut out = BTreeMap::new();
    let Some(map) = doc[key].as_object() else {
        return Ok(out);
    };
    for (name, sels) in map {
        let mut list = Vec::new();
        for s in sels.as_array().ok_or_else(|| bad("selectors"))? {
            let priority = i32::try_from(s["priority"].as_i64().ok_or_else(|| bad("priority"))?)
                .map_err(|_| bad("priority range"))?;
            let biomes = match &s["biomes"] {
                serde_json::Value::Null => None,
                serde_json::Value::Array(a) => Some(
                    a.iter()
                        .map(|v| v.as_str().map(str::to_string).ok_or_else(|| bad("biome")))
                        .collect::<std::io::Result<Vec<_>>>()?,
                ),
                _ => return Err(bad("biomes")),
            };
            list.push(VariantSelector { priority, biomes });
        }
        out.insert(name.clone(), list);
    }
    Ok(out)
}

fn parse_sound_sets(
    doc: &serde_json::Value,
    key: &str,
) -> std::io::Result<BTreeMap<String, SoundSet>> {
    let bad = |w: &str| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("mob_variants.json {key}: {w}"),
        )
    };
    let mut out = BTreeMap::new();
    let Some(map) = doc[key].as_object() else {
        return Ok(out);
    };
    for (name, set) in map {
        let leak = |k: &str| -> std::io::Result<&'static str> {
            let s = set[k].as_str().ok_or_else(|| bad(k))?;
            Ok(&*Box::leak(s.to_string().into_boxed_str()))
        };
        let ambient = leak("ambient")?;
        let step = leak("step")?;
        let baby_ambient = if set["baby_ambient"].is_null() {
            ambient
        } else {
            leak("baby_ambient")?
        };
        let baby_step = if set["baby_step"].is_null() {
            step
        } else {
            leak("baby_step")?
        };
        out.insert(
            name.clone(),
            SoundSet {
                ambient,
                step,
                baby_ambient,
                baby_step,
            },
        );
    }
    Ok(out)
}

fn parse_biome_list(doc: &serde_json::Value, key: &str) -> Vec<String> {
    doc["farm_animal_biomes"][key]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Load-bearing outside Rust: `ci/gen-gameplay-tables.sh` greps for it.
pub const VARIANTS_PATH: &str = "data/vanilla/mob_variants.json";

impl MobVariants {
    /// # Errors
    /// On a malformed document.
    pub fn from_json(doc: &serde_json::Value) -> std::io::Result<Self> {
        let bad = |w: &str| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("mob_variants.json: {w}"),
            )
        };
        let mut pig_variant = BTreeMap::new();
        for (name, sels) in doc["pig_variant"]
            .as_object()
            .ok_or_else(|| bad("pig_variant"))?
        {
            let mut out = Vec::new();
            for s in sels.as_array().ok_or_else(|| bad("selectors"))? {
                let priority =
                    i32::try_from(s["priority"].as_i64().ok_or_else(|| bad("priority"))?)
                        .map_err(|_| bad("priority range"))?;
                let biomes = match &s["biomes"] {
                    serde_json::Value::Null => None,
                    serde_json::Value::Array(a) => Some(
                        a.iter()
                            .map(|v| v.as_str().map(str::to_string).ok_or_else(|| bad("biome")))
                            .collect::<std::io::Result<Vec<_>>>()?,
                    ),
                    _ => return Err(bad("biomes")),
                };
                out.push(VariantSelector { priority, biomes });
            }
            pig_variant.insert(name.clone(), out);
        }
        let mut pig_sound_variant = BTreeMap::new();
        for (name, set) in doc["pig_sound_variant"]
            .as_object()
            .ok_or_else(|| bad("pig_sound_variant"))?
        {
            let leak = |k: &str| -> std::io::Result<&'static str> {
                let s = set[k].as_str().ok_or_else(|| bad(k))?;
                Ok(&*Box::leak(s.to_string().into_boxed_str()))
            };
            pig_sound_variant.insert(
                name.clone(),
                PigSoundSet {
                    ambient: leak("ambient")?,
                    step: leak("step")?,
                    hurt: leak("hurt")?,
                    death: leak("death")?,
                    eat: leak("eat")?,
                    baby_ambient: leak("baby_ambient")?,
                    baby_step: leak("baby_step")?,
                    baby_hurt: leak("baby_hurt")?,
                    baby_death: leak("baby_death")?,
                    baby_eat: leak("baby_eat")?,
                },
            );
        }
        let sky = &doc["sky_light_level"];
        if sky["ease"].as_str() != Some("linear") || sky["modifier"].as_str() != Some("multiply") {
            return Err(bad("sky_light_level is not a linear multiply track"));
        }
        let period_ticks = sky["period_ticks"]
            .as_i64()
            .ok_or_else(|| bad("period_ticks"))?;
        let mut keyframes = Vec::new();
        for k in sky["keyframes"]
            .as_array()
            .ok_or_else(|| bad("keyframes"))?
        {
            let ticks = i32::try_from(k[0].as_i64().ok_or_else(|| bad("keyframe ticks"))?)
                .map_err(|_| bad("keyframe ticks range"))?;
            #[expect(clippy::cast_possible_truncation, reason = "a Float keyframe value")]
            let value = k[1].as_f64().ok_or_else(|| bad("keyframe value"))? as f32;
            keyframes.push((ticks, value));
        }
        if pig_variant.is_empty() || pig_sound_variant.is_empty() || keyframes.len() < 2 {
            return Err(bad("empty"));
        }
        Ok(Self {
            pig_variant,
            pig_sound_variant,
            sky_light_level: SkyLightTrack {
                period_ticks,
                keyframes,
            },
            cow_variant: parse_selectors(doc, "cow_variant")?,
            chicken_variant: parse_selectors(doc, "chicken_variant")?,
            cow_sound_variant: parse_sound_sets(doc, "cow_sound_variant")?,
            chicken_sound_variant: parse_sound_sets(doc, "chicken_sound_variant")?,
            warm_farm_biomes: parse_biome_list(doc, "warm"),
            cold_farm_biomes: parse_biome_list(doc, "cold"),
        })
    }
}

/// The table, loaded once; `None` when the extract is absent.
pub fn mob_variants() -> Option<&'static MobVariants> {
    static TABLE: OnceLock<Option<MobVariants>> = OnceLock::new();
    TABLE
        .get_or_init(|| {
            let text = std::fs::read_to_string(table_path(VARIANTS_PATH)).ok()?;
            let doc: serde_json::Value = serde_json::from_str(&text).ok()?;
            MobVariants::from_json(&doc).ok()
        })
        .as_ref()
}

fn captured_registry(name: &str) -> &'static [String] {
    static CACHE: OnceLock<Mutex<HashMap<String, &'static [String]>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut c = cache.lock().expect("registries");
    if let Some(v) = c.get(name) {
        return v;
    }
    let ids: Vec<String> =
        crate::configuration::CapturedRegistries::load(&table_path("data/vanilla/registries.bin"))
            .ok()
            .and_then(|r| r.entry_ids(name).ok().flatten())
            .unwrap_or_default();
    let leaked: &'static [String] = Box::leak(ids.into_boxed_slice());
    c.insert(name.to_string(), leaked);
    leaked
}

/// The `minecraft:pig_variant` registry's entries in id order, from the
/// captured `registry_data` — the numbers `set_entity_data` writes.
#[must_use]
pub fn pig_variant_ids() -> &'static [String] {
    captured_registry("minecraft:pig_variant")
}

/// The `minecraft:pig_sound_variant` registry's entries in id order.
#[must_use]
pub fn pig_sound_variant_names() -> &'static [String] {
    captured_registry("minecraft:pig_sound_variant")
}

/// `(PigVariants.DEFAULT, PigSoundVariants.CLASSIC)` as registry ids —
/// `Pig.defineSynchedData`'s two initial values. `None` when the
/// registries were not captured, which refuses the spawn.
#[must_use]
pub fn pig_defaults() -> Option<(i32, i32)> {
    let v = pig_variant_ids()
        .iter()
        .position(|n| n == "minecraft:temperate")?;
    let s = pig_sound_variant_names()
        .iter()
        .position(|n| n == "minecraft:classic")?;
    Some((i32::try_from(v).ok()?, i32::try_from(s).ok()?))
}

// ---- the store ----

/// One mob's AI and host.
pub struct MobSlot {
    pub ai: MobAi<ServerMob>,
    /// The `PanicGoal`'s id, for `PathfinderMob.isPanicking()`.
    pub panic_goal: Option<rustmc_entity::ai::goal_selector::GoalId>,
}

// SAFETY: a `MobAi` is goal structs, controls, a navigation and the host.
// The one non-`Send` member is the `Box<dyn Goal>` / `Box<dyn Fn>` objects
// the goal crate boxes without a `Send` bound; every goal and selector
// closure in `rustmc-entity` captures only `Copy` data or nothing (the
// `Rc<RefCell>` uses there are test spies). The host holds `&'static`
// tables and plain fields. The store is only ever accessed under its
// mutex, from the clock thread and the connection threads in turn.
unsafe impl Send for MobSlot {}

/// The level's mobs, in `EntityTickList` order (insertion; a mob added
/// mid-tick waits for the next).
#[derive(Default)]
pub struct Mobs {
    inner: Mutex<MobsInner>,
}

#[derive(Default)]
struct MobsInner {
    by_id: HashMap<i32, MobSlot>,
    order: Vec<i32>,
}

impl Mobs {
    /// The process-wide store.
    pub fn shared() -> &'static Self {
        static SHARED: OnceLock<Mobs> = OnceLock::new();
        SHARED.get_or_init(Mobs::default)
    }

    fn insert(&self, id: i32, slot: MobSlot) {
        let mut g = self.inner.lock().expect("mobs");
        if g.by_id.insert(id, slot).is_none() {
            g.order.push(id);
        }
    }

    fn take(&self, id: i32) -> Option<MobSlot> {
        self.inner.lock().expect("mobs").by_id.remove(&id)
    }

    fn put_back(&self, id: i32, slot: MobSlot) {
        self.inner.lock().expect("mobs").by_id.insert(id, slot);
    }

    fn forget(&self, id: i32) {
        let mut g = self.inner.lock().expect("mobs");
        g.by_id.remove(&id);
        g.order.retain(|&k| k != id);
    }

    /// Ids in tick order.
    #[must_use]
    pub fn ids(&self) -> Vec<i32> {
        self.inner.lock().expect("mobs").order.clone()
    }

    /// How many mobs stand in the world.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.lock().expect("mobs").order.len()
    }

    /// Whether the store is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Read one mob under the lock. `None` while it is being ticked
    /// (taken out) or when it does not exist.
    pub fn with<T>(&self, id: i32, f: impl FnOnce(&MobSlot) -> T) -> Option<T> {
        self.inner.lock().expect("mobs").by_id.get(&id).map(f)
    }

    /// Mutate one mob under the lock.
    pub fn with_mut<T>(&self, id: i32, f: impl FnOnce(&mut MobSlot) -> T) -> Option<T> {
        self.inner.lock().expect("mobs").by_id.get_mut(&id).map(f)
    }
}

/// The facts of a mob that is in the store (not the one being ticked).
fn mob_facts(id: i32) -> Option<EntityFacts> {
    Mobs::shared().with(id, |s| {
        let mut f = s.ai.mob.own_facts();
        // `PathfinderMob.isPanicking()`: a running `PanicGoal` (the brain
        // arm is for brain mobs, of which there are none here).
        f.panicking = s.panic_goal.is_some_and(|id| {
            s.ai.goal_selector
                .available_goals()
                .any(|(g, w)| g == id && w.is_running())
        });
        f
    })
}

/// `EntityFacts` for any entity in the level: a player from the entity
/// list and the server's game type, a mob from the store.
#[must_use]
pub fn facts_of_entity(id: i32) -> Option<EntityFacts> {
    if let Some(f) = mob_facts(id) {
        return Some(f);
    }
    let e = Entities::shared().get(id)?;
    if e.kind != crate::entities::PLAYER {
        return None;
    }
    // The player's OWN mode: `/gamemode` changes one player, not the server.
    let creative = crate::players::game_type_of_entity(e.id)
        .unwrap_or_else(crate::status::game_type)
        == crate::status::CREATIVE;
    Some(EntityFacts {
        id: e.id,
        x: e.x,
        y: e.y,
        z: e.z,
        // A standing player; the sneaking pose is not tracked here.
        eye_y: e.y + crate::dropitem::PLAYER_EYE_HEIGHT,
        bb: Aabb::new(e.x - 0.3, e.y, e.z - 0.3, e.x + 0.3, e.y + 1.8, e.z + 0.3),
        x_rot: e.x_rot,
        y_rot: e.y_rot,
        alive: true,
        is_player: true,
        is_spectator: false,
        is_creative: creative,
        // `Entity.isInvulnerable()` is the `invulnerable` field, not the
        // creative ability.
        is_invulnerable: false,
        is_ghast: false,
        vehicle: None,
        age: 0,
        in_love: false,
        panicking: false,
        target: None,
    })
}

/// `ServerLevel.players()` in list order — join order.
#[must_use]
pub fn player_ids_in_join_order() -> Vec<i32> {
    Entities::shared()
        .all_in_tick_order()
        .into_iter()
        .filter(|e| e.kind == crate::entities::PLAYER)
        .map(|e| e.id)
        .collect()
}

fn entity_bb(e: &Entity) -> Aabb {
    if let Some(f) = mob_facts(e.id) {
        return f.bb;
    }
    if e.kind == crate::entities::PLAYER {
        Aabb::new(e.x - 0.3, e.y, e.z - 0.3, e.x + 0.3, e.y + 1.8, e.z + 0.3)
    } else {
        // An item: `sized(0.25, 0.25)`.
        Aabb::new(
            e.x - 0.125,
            e.y,
            e.z - 0.125,
            e.x + 0.125,
            e.y + 0.25,
            e.z + 0.125,
        )
    }
}

/// Whether an entity id is a mob of this store.
#[must_use]
pub fn is_mob(id: i32) -> bool {
    Mobs::shared().with(id, |_| ()).is_some()
}

/// The kind name of a mob.
#[must_use]
pub fn mob_kind_name(id: i32) -> Option<&'static str> {
    Mobs::shared().with(id, |s| s.ai.mob.kind.name)
}

/// `other.getClass() == clazz` (exact) or `clazz.isInstance(other)`.
#[must_use]
pub fn entity_class_test(id: i32, class: EntityClass, exact: bool) -> bool {
    let kind = mob_kind_name(id);
    let is_player = kind.is_none()
        && Entities::shared()
            .get(id)
            .is_some_and(|e| e.kind == crate::entities::PLAYER);
    match class {
        EntityClass::Player => is_player,
        EntityClass::Living => !exact && (is_player || kind.is_some()),
        EntityClass::Mob => !exact && kind.is_some(),
        EntityClass::Animal => {
            !exact
                && kind
                    .and_then(kind_by_name)
                    .is_some_and(|k| k.data.is_animal())
        }
        EntityClass::SameKind => kind.is_some(),
        EntityClass::Villager => kind == Some("minecraft:villager"),
        // `ItemEntity.class`: never a mob and never a player, so neither
        // `kind` nor `is_player` can answer it; the registry's kind can.
        EntityClass::Item => Entities::shared()
            .get(id)
            .is_some_and(|e| e.kind == crate::entities::ITEM),
    }
}

/// `Level.getEntitiesOfClass(Class, AABB)` — every entity but the asker
/// whose box intersects, by ascending id (**not** the section storage's
/// order; see the module docs).
#[must_use]
pub fn entities_of_class_in(
    self_id: i32,
    self_kind: &MobKind,
    class: EntityClass,
    aabb: &Aabb,
) -> Vec<i32> {
    let mut out: Vec<i32> = Entities::shared()
        .all()
        .into_iter()
        .filter(|e| e.id != self_id)
        .filter(|e| entity_bb(e).intersects(aabb))
        .filter(|e| match class {
            EntityClass::SameKind => mob_kind_name(e.id) == Some(self_kind.name),
            _ => entity_class_test(e.id, class, false),
        })
        .map(|e| e.id)
        .collect();
    out.sort_unstable();
    out
}

/// `entity.isInWater()` for another entity: a mob's flag, a player's
/// block (its `wasTouchingWater` is client-tracked and not replicated).
#[must_use]
pub fn entity_in_water(id: i32) -> bool {
    Mobs::shared()
        .with(id, |s| s.ai.mob.is_in_water())
        .unwrap_or(false)
}

/// `Level.getPushableEntities(entity, box)`: every other entity whose box
/// intersects and which `EntitySelector.pushableBy(entity)` admits —
/// `NO_SPECTATORS`, `isPushable()`, team collision rules (none).
#[must_use]
pub fn pushable_entities(self_id: i32, bb: &Aabb) -> Vec<i32> {
    let mut out: Vec<i32> = Entities::shared()
        .all()
        .into_iter()
        .filter(|e| e.id != self_id)
        .filter(|e| entity_bb(e).intersects(bb))
        .filter(|e| {
            if let Some(p) = Mobs::shared().with(e.id, |s| living::is_pushable(&s.ai.mob)) {
                p
            } else {
                // `LivingEntity.isPushable` for a player: alive, not a
                // spectator, not climbing. An item is not pushable
                // (`Entity.isPushable` is `iconst_0`).
                e.kind == crate::entities::PLAYER
            }
        })
        .map(|e| e.id)
        .collect();
    out.sort_unstable();
    out
}

/// `other.push(mob)` — `Entity.push(Entity)` with `this = other`,
/// offsets 24-169: both entities' horizontal offset is split between them.
pub fn push_pair(other: i32, m: &mut ServerMob) {
    let Some(o) = Entities::shared().get(other) else {
        return;
    };
    let Some((dx, dz)) = living::push_math(o.x, o.z, m.x(), m.z()) else {
        return;
    };
    // 123-146: `if (!other.isVehicle() && other.isPushable()) other.push(-dx, 0, -dz)`.
    let other_pushable = Mobs::shared()
        .with(other, |s| living::is_pushable(&s.ai.mob))
        .unwrap_or(o.kind == crate::entities::PLAYER);
    if other_pushable
        && Mobs::shared()
            .with_mut(other, |s| living::push_by(&mut s.ai.mob, -dx, 0.0, -dz))
            .is_none()
    {
        Entities::shared().update(other, |e| {
            if dx.is_finite() && dz.is_finite() {
                e.movement = (e.movement.0 - dx, e.movement.1, e.movement.2 - dz);
            }
        });
    }
    // 147-168: `if (!mob.isVehicle() && mob.isPushable()) mob.push(dx, 0, dz)`.
    if living::is_pushable(m) {
        living::push_by(m, dx, 0.0, dz);
    }
}

/// `LivingEntity.getHurtSound(DamageSource)` per kind.
///
/// Not a field on [`MobKind`], and the reason is the pig: its hurt sound
/// comes from the `pig_sound_variant` its `SynchedEntityData` carries, so it
/// is a property of the *instance* and not of the type. A `&'static str` on
/// `MobKind` would have had to be one of the three, which is a coin-flip that
/// is right a third of the time.
#[must_use]
pub fn hurt_sound(m: &ServerMob) -> Option<&'static str> {
    match m.kind.data {
        DataLayout::Pig => pig::hurt_sound(m),
        // `Bat.getHurtSound` is a bare `getstatic SoundEvents.BAT_HURT` —
        // no variant, no branch.
        DataLayout::Bat => Some("minecraft:entity.bat.hurt"),
        DataLayout::IronGolem | DataLayout::SnowGolem | DataLayout::CopperGolem => {
            golem::hurt_sound(m)
        }
        // `ArmorStand.getHurtSound`: `ARMOR_STAND_HIT`.
        DataLayout::ArmorStand => Some("minecraft:entity.armor_stand.hit"),
        // `Sheep.getHurtSound`: `SoundEvents.SHEEP_HURT`, no variant.
        DataLayout::Sheep => Some("minecraft:entity.sheep.hurt"),
        // `AbstractCow` and `Chicken` read their sound variant's hurt sound,
        // which `ci/dump-mob-variants.sh` does not extract (ambient and step
        // only): no sound rather than a guessed one.
        DataLayout::Cow | DataLayout::Mooshroom | DataLayout::Chicken => None,
    }
}

/// `LivingEntity.getDeathSound()` per kind. See [`hurt_sound`].
#[must_use]
pub fn death_sound(m: &ServerMob) -> Option<&'static str> {
    match m.kind.data {
        DataLayout::Pig => pig::death_sound(m),
        DataLayout::Bat => Some("minecraft:entity.bat.death"),
        DataLayout::IronGolem | DataLayout::SnowGolem | DataLayout::CopperGolem => {
            golem::death_sound(m)
        }
        // `ArmorStand.getDeathSound`: `ARMOR_STAND_BREAK`.
        DataLayout::ArmorStand => Some("minecraft:entity.armor_stand.break"),
        DataLayout::Sheep => Some("minecraft:entity.sheep.death"),
        // See [`hurt_sound`].
        DataLayout::Cow | DataLayout::Mooshroom | DataLayout::Chicken => None,
    }
}

/// `ambient_sound` per kind.
pub fn ambient_sound(m: &mut ServerMob) -> Option<&'static str> {
    match m.kind.data {
        DataLayout::Pig => pig::ambient_sound(m),
        DataLayout::Cow | DataLayout::Mooshroom => cow::ambient_sound(m),
        DataLayout::Sheep => Some(sheep::AMBIENT),
        DataLayout::Chicken => variants::ambient_sound(m, &variants::CHICKEN),
        DataLayout::Bat => bat::ambient_sound(m),
        DataLayout::IronGolem | DataLayout::SnowGolem | DataLayout::CopperGolem => {
            golem::ambient_sound(m)
        }
        // Not a `Mob`: no ambient sound exists on its chain.
        DataLayout::ArmorStand => None,
    }
}

/// `playStepSound` per kind, as `(event, volume, pitch)`. `Pig`'s override
/// is its sound set's step at `0.15F, 1.0F`; the golems' are in
/// [`golem::step_sound`].
#[must_use]
pub fn step_sound(m: &ServerMob) -> Option<(&'static str, f32, f32)> {
    match m.kind.data {
        // `Pig`, `Cow`, `Sheep` and `Chicken` each override `playStepSound` as
        // `playSound(step, 0.15F, 1.0F)`.
        DataLayout::Pig => pig::step_sound(m).map(|e| (e, 0.15, 1.0)),
        DataLayout::Cow | DataLayout::Mooshroom => cow::step_sound(m).map(|e| (e, 0.15, 1.0)),
        DataLayout::Sheep => Some((sheep::STEP, 0.15, 1.0)),
        DataLayout::Chicken => variants::step_sound(m, &variants::CHICKEN).map(|e| (e, 0.15, 1.0)),
        DataLayout::Bat => None,
        DataLayout::IronGolem | DataLayout::SnowGolem | DataLayout::CopperGolem => {
            golem::step_sound(m)
        }
        // `Entity.playStepSound`'s block sound; the stand does not walk.
        DataLayout::ArmorStand => None,
    }
}

/// `SoundSource.NEUTRAL`'s ordinal: `MASTER 0, MUSIC 1, RECORDS 2,
/// WEATHER 3, BLOCKS 4, HOSTILE 5, NEUTRAL 6, PLAYERS 7, AMBIENT 8, VOICE
/// 9, UI 10` — `crate::sounds` pins `BLOCKS` at 4 and `PLAYERS` at 7.
pub const SOURCE_NEUTRAL: i32 = 6;

/// `ServerLevel.playSound(null, x, y, z, event, NEUTRAL, volume, pitch)`
/// → `playSeededSound(...)` with `random.nextLong()` as the seed, sent to
/// every player within `volume > 1 ? 16 * volume : 16` blocks.
pub fn play_level_sound(m: &ServerMob, event: &str, volume: f32, pitch: f32) {
    play_level_sound_in(m, event, SOURCE_NEUTRAL, volume, pitch);
}

/// [`play_level_sound`] with the source given — `Level.playSound(null, x,
/// y, z, event, source, volume, pitch)` at the entity's position, for a
/// caller whose sound is not the entity's own (`ArmorStandItem.useOn`'s
/// `SoundSource.BLOCKS`).
pub fn play_level_sound_in(m: &ServerMob, event: &str, source: i32, volume: f32, pitch: f32) {
    play_sound_at((m.x(), m.y(), m.z()), event, source, volume, pitch);
}

/// `ServerLevel.playSound(null, x, y, z, event, source, volume, pitch)` at
/// any point: the seed from the level's random, every player within
/// `volume > 1 ? 16 * volume : 16` blocks.
pub fn play_sound_at(at: (f64, f64, f64), event: &str, source: i32, volume: f32, pitch: f32) {
    play_sound_at_except(at, event, source, volume, pitch, None);
}

/// [`play_sound_at`] with `PlayerList.broadcast`'s `except`: the player
/// entity that is not sent it (`Player.playSound`, a `DyeItem`'s
/// `level.playSound(player, ...)`).
pub fn play_sound_at_except(
    at: (f64, f64, f64),
    event: &str,
    source: i32,
    volume: f32,
    pitch: f32,
    except: Option<i32>,
) {
    let _seed = level_random(RandomSource::next_i64);
    let sound = crate::sounds::Sound {
        event: event.to_string(),
        source,
        at: crate::sounds::Eighths::exact(at.0, at.1, at.2),
        pos: at,
        volume,
        pitch,
        except: None,
    };
    let range = if volume > 1.0 {
        f64::from(16.0 * volume)
    } else {
        16.0
    };
    let Some(ids) = crate::protocol_ids::shared() else {
        return;
    };
    let Some(id) = ids.sound(&sound.event) else {
        return;
    };
    let Ok(body) = crate::sounds::sound_body(&sound, id) else {
        return;
    };
    let players = tick_players().lock().expect("players").clone();
    for (pid, conn) in players {
        if except == Some(pid) {
            continue;
        }
        let Some(e) = Entities::shared().get(pid) else {
            continue;
        };
        let (dx, dy, dz) = (e.x - at.0, e.y - at.1, e.z - at.2);
        if dx * dx + dy * dy + dz * dz < range * range {
            let _ = conn.send(rustmc_proto::ids::play::cb::SOUND, &body);
        }
    }
}

/// `ServerLevel.playSound(null, entity, event, source, 1F, 1F)` →
/// `playSeededSound(except, entity, ...)`: a `ClientboundSoundEntityPacket`
/// — the sound follows the entity on the client — to every player within 16
/// blocks of the entity. Not gated on `isSilent()`: that test is
/// `Entity.playSound`'s, and this is the level's.
pub fn play_entity_sound(m: &host::ServerMob, event: &str, source: i32) {
    play_entity_sound_except(m, event, source, None);
}

/// [`play_entity_sound`] with an `except` player (`Level.playSound(player,
/// entity, ...)`).
pub fn play_entity_sound_except(
    m: &host::ServerMob,
    event: &str,
    source: i32,
    except: Option<i32>,
) {
    let _seed = level_random(RandomSource::next_i64);
    let Some(ids) = crate::protocol_ids::shared() else {
        return;
    };
    let Some(sound_id) = ids.sound(event) else {
        return;
    };
    let Ok(body) = sound_entity_body(sound_id, source, m.id, 1.0, 1.0) else {
        return;
    };
    let (x, y, z) = (m.x(), m.y(), m.z());
    let players = tick_players().lock().expect("players").clone();
    for (pid, conn) in players {
        if except == Some(pid) {
            continue;
        }
        let Some(e) = Entities::shared().get(pid) else {
            continue;
        };
        let (dx, dy, dz) = (e.x - x, e.y - y, e.z - z);
        if dx * dx + dy * dy + dz * dz < 16.0 * 16.0 {
            let _ = conn.send(rustmc_proto::ids::play::cb::SOUND_ENTITY, &body);
        }
    }
}

/// `ClientboundSoundEntityPacket.write`: the sound as a registry holder
/// (`id + 1`), the source enum, the entity id (varint), volume, pitch, and
/// the seed — zero here; the recorder reads and drops it.
///
/// # Errors
/// On a write failure, which for a `Vec` cannot happen.
pub fn sound_entity_body(
    sound_id: i32,
    source: i32,
    entity: i32,
    volume: f32,
    pitch: f32,
) -> std::io::Result<Vec<u8>> {
    use rustmc_proto::varint::write_varint;
    use rustmc_proto::wire;
    let mut out = Vec::new();
    write_varint(&mut out, sound_id + 1)?;
    write_varint(&mut out, source)?;
    write_varint(&mut out, entity)?;
    wire::write_f32(&mut out, volume)?;
    wire::write_f32(&mut out, pitch)?;
    wire::write_i64(&mut out, 0)?;
    Ok(out)
}

/// `ServerLevel.levelEvent(null, event, pos, 0)`: broadcast to players
/// within 64 blocks.
pub fn level_event(m: &ServerMob, event: i32, pos: BlockPos) {
    level_event_with(m, event, pos, 0);
}

/// `ServerLevel.levelEvent(null, event, pos, data)`.
pub fn level_event_with(m: &ServerMob, event: i32, pos: BlockPos, data: i32) {
    let Ok(body) = crate::interact::level_event_body(event, (pos.x, pos.y, pos.z), data, false)
    else {
        return;
    };
    let players = tick_players().lock().expect("players").clone();
    for (pid, conn) in players {
        let Some(e) = Entities::shared().get(pid) else {
            continue;
        };
        let (dx, dy, dz) = (e.x - m.x(), e.y - m.y(), e.z - m.z());
        if dx * dx + dy * dy + dz * dz < 64.0 * 64.0 {
            let _ = conn.send(rustmc_proto::ids::play::cb::LEVEL_EVENT, &body);
        }
    }
}

// ---- entering the world ----

/// The level a mob is built over: the shared tables, or `None` when an
/// extract is absent.
pub fn mob_level(world: &'static crate::tiles::Tiles) -> Option<MobLevel> {
    MobLevel::new(world)
}

fn attribute_table() -> Option<&'static crate::attributes::Attributes> {
    crate::attributes::Attributes::shared()
}

/// `Mob.<init>`'s `registerGoals` (on a server level) for the kind, over
/// a fresh `MobAi`.
fn build_ai(m: ServerMob) -> MobSlot {
    let mut ai = MobAi::new(m);
    let panic_goal = match ai.mob.kind.data {
        DataLayout::Pig => Some(pig::register_goals(&mut ai)),
        DataLayout::Cow | DataLayout::Mooshroom => Some(cow::register_goals(&mut ai)),
        DataLayout::Sheep => Some(sheep::register_goals(&mut ai)),
        DataLayout::Chicken => Some(chicken::register_goals(&mut ai)),
        DataLayout::IronGolem => {
            golem::register_iron_golem_goals(&mut ai);
            None
        }
        DataLayout::SnowGolem => {
            golem::register_snow_golem_goals(&mut ai);
            None
        }
        // `CopperGolem` has a brain, not goals, and the brain is not ported;
        // an armor stand is not a `Mob` and has neither.
        DataLayout::Bat | DataLayout::CopperGolem | DataLayout::ArmorStand => None,
    };
    ai.mob.panic_goal = panic_goal;
    MobSlot { ai, panic_goal }
}

/// The connections a sound or level event can reach this tick, with the
/// entity id each stands at — a snapshot the clock thread takes before
/// ticking and the spawn path takes before the ambient sound.
type TickPlayers = Vec<(i32, std::sync::Arc<crate::players::Connection>)>;

fn tick_players() -> &'static Mutex<TickPlayers> {
    static P: OnceLock<Mutex<TickPlayers>> = OnceLock::new();
    P.get_or_init(|| Mutex::new(Vec::new()))
}

fn set_tick_players(players: &[TrackedPlayer]) {
    *tick_players().lock().expect("players") = players
        .iter()
        .map(|p| (p.entity_id, std::sync::Arc::clone(&p.conn)))
        .collect();
}

/// The `Entity` record the tracker reads, mirrored from the host.
fn mirror(m: &ServerMob) -> Entity {
    Entity {
        id: m.id,
        uuid: m.uuid,
        kind: m.kind.entity_kind,
        x: m.x(),
        y: m.y(),
        z: m.z(),
        y_rot: m.y_rot,
        x_rot: m.x_rot,
        y_head_rot: m.y_head_rot,
        movement: (m.body.movement.x, m.body.movement.y, m.body.movement.z),
        on_ground: m.on_ground(),
        data: 0,
        swinging: false,
        swing_time: 0,
        swinging_arm: None,
        synched: crate::synched::Synched::EntityOnly,
    }
}

/// `EntityType.spawn`'s tail (offsets 15-46): `addFreshEntityWithPassengers`
/// — the entity list, the tracker's pairing — then
/// `Mob.playAmbientSound()`.
fn add_fresh_entity(
    slot: MobSlot,
    entities: &Entities,
    tracker: &Tracker,
    players: &[TrackedPlayer],
    kinds: &KindIds,
) -> i32 {
    add_fresh_entity_with(slot, entities, tracker, players, kinds, true)
}

/// `ServerLevel.addFreshEntityWithPassengers` alone — the entity list and
/// the tracker's pairing, without `EntityType.spawn`'s ambient sound. The
/// path a bred or egg-hatched baby takes.
fn add_fresh_entity_quiet(
    slot: MobSlot,
    entities: &Entities,
    tracker: &Tracker,
    players: &[TrackedPlayer],
    kinds: &KindIds,
) -> i32 {
    add_fresh_entity_with(slot, entities, tracker, players, kinds, false)
}

fn add_fresh_entity_with(
    mut slot: MobSlot,
    entities: &Entities,
    tracker: &Tracker,
    players: &[TrackedPlayer],
    kinds: &KindIds,
    ambient: bool,
) -> i32 {
    let ai = &mut slot.ai;
    let id = ai.mob.id;
    let record = mirror(&ai.mob);
    entities.insert(record.clone());
    set_tick_players(players);
    // The pairing reads `pairing_extras(id)` from the store, so the mob
    // is in it before `ChunkMap.addEntity` runs; the sound below reads
    // nothing from the store.
    let pairing = (
        ai.mob
            .data
            .non_default_values()
            .and_then(|values| crate::synched::set_entity_data_body(id, &values).ok()),
        None,
    );
    PENDING_PAIRING.with(|p| *p.borrow_mut() = Some((id, pairing)));
    tracker.add(&record, players, kinds);
    PENDING_PAIRING.with(|p| *p.borrow_mut() = None);
    if !ambient {
        Mobs::shared().insert(id, slot);
        return id;
    }
    // spawn 26-43: `if (entity instanceof Mob) mob.playAmbientSound()`.
    let event = ambient_sound(&mut ai.mob);
    if let Some(event) = event {
        let volume = ai.mob.kind.sound_volume;
        let pitch = {
            let m = &mut ai.mob;
            let a = m.random.next_f32();
            let b = m.random.next_f32();
            let base = if m.is_baby() {
                (a - b) * 0.2 + 1.5
            } else {
                (a - b) * 0.2 + 1.0
            };
            base * m.kind.voice_pitch_scale
        };
        living::play_sound(&ai.mob, event, volume, pitch);
    }
    Mobs::shared().insert(id, slot);
    id
}

/// Run `f` with `players` as the audience of anything put in the world —
/// [`breed::with_players`], for a caller outside this module.
pub fn breed_with_players<T>(players: &[TrackedPlayer], f: impl FnOnce() -> T) -> T {
    breed::with_players(players, f)
}

/// `ServerLevel.addFreshEntity(newMob)` for a conversion (`Mob.convertTo`
/// 71-75): the entity list, the tracker's pairing with the current
/// audience, the store — no ambient sound. Returns the new id.
pub(super) fn add_converted(m: ServerMob) -> i32 {
    let slot = build_ai(m);
    let id = slot.ai.mob.id;
    match crate::entities::shared_kind_ids() {
        Some(kinds) => add_fresh_entity_quiet(
            slot,
            Entities::shared(),
            Tracker::shared(),
            &breed::current_players(),
            kinds,
        ),
        None => id,
    }
}

/// A spawn egg used on a block: `SpawnEggItem.useOn` for the `item`, at
/// `clicked` with `face`. `Ok(None)` is `PASS` — not a spawn egg;
/// `Ok(Some(id))` is `SUCCESS`; `Err` is `FAIL`.
///
/// # Errors
/// [`spawn::EggRefusal`] for `FAIL`.
#[expect(
    clippy::too_many_arguments,
    reason = "the click's three plus the level's four handles"
)]
pub fn use_spawn_egg(
    world: &'static crate::tiles::Tiles,
    item: &str,
    entity_data: Option<&crate::item::EntityData>,
    clicked: BlockPos,
    face_is_up: bool,
    entities: &Entities,
    tracker: &Tracker,
    players: &[TrackedPlayer],
    kinds: &KindIds,
) -> Result<Option<i32>, spawn::EggRefusal> {
    let Some(kind) = kind_for_spawn_egg(item) else {
        return Ok(None);
    };
    let level = mob_level(world).ok_or(spawn::EggRefusal::NoTables)?;
    let table = attribute_table().ok_or(spawn::EggRefusal::NoTables)?;
    let mut m = spawn::use_spawn_egg(level, kind, clicked, face_is_up, table, || {
        entities.next_id()
    })?;
    // `EntityType.createDefaultStackConfig` → `appendCustomEntityStackConfig`:
    // the stack's `minecraft:entity_data`, applied after `finalizeSpawn` by
    // `updateCustomEntityTag` (javap 17-27: only when the data's type is the
    // type being spawned; a pig is not `onlyOpCanSetNbt`) through
    // `TypedEntityData.loadInto` → `Entity.load`, whose `Silent` read is
    // `setSilent(getBooleanOr("Silent", false))` — `DATA_SILENT`, so it is
    // also in the mob's first `set_entity_data`.
    if let Some(d) = entity_data.filter(|d| d.entity == kind.name) {
        if let Some(silent) = d.silent {
            m.data.set_bool(data::id::SILENT, silent);
        }
        // `Mob.readAdditionalSaveData` 111-120: `setLeftHanded(getBooleanOr(
        // "LeftHanded", false))` — over the roll `finalizeSpawn` 63-83 made,
        // which has already been drawn, so the level random moves the same.
        if let Some(left) = d.left_handed {
            m.set_left_handed(left);
        }
    }
    let slot = build_ai(m);
    Ok(Some(add_fresh_entity(
        slot, entities, tracker, players, kinds,
    )))
}

/// `/summon <kind> <x> <y> <z>`-shaped: `SummonCommand.createEntity` with
/// an empty tag and `finalize = true`, then `tryAddFreshEntityWithPassengers`.
///
/// # Errors
/// [`spawn::EggRefusal`] for the command's own refusals.
#[expect(
    clippy::too_many_arguments,
    reason = "the level's four handles plus the command's four"
)]
pub fn summon(
    world: &'static crate::tiles::Tiles,
    kind: &'static MobKind,
    x: f64,
    y: f64,
    z: f64,
    entities: &Entities,
    tracker: &Tracker,
    players: &[TrackedPlayer],
    kinds: &KindIds,
) -> Result<i32, spawn::EggRefusal> {
    let level = mob_level(world).ok_or(spawn::EggRefusal::NoTables)?;
    let table = attribute_table().ok_or(spawn::EggRefusal::NoTables)?;
    let m = spawn::summon(level, kind, x, y, z, table, || entities.next_id())?;
    let slot = build_ai(m);
    // `tryAddFreshEntityWithPassengers` → `addFreshEntity`: no ambient
    // sound on this path (that is `EntityType.spawn`'s).
    let id = slot.ai.mob.id;
    let record = mirror(&slot.ai.mob);
    entities.insert(record.clone());
    Mobs::shared().insert(id, slot);
    tracker.add(&record, players, kinds);
    Ok(id)
}

/// `CarvedPumpkinBlock.spawnGolemInWorld`'s entity half, for a request
/// [`crate::golem`] left in the handler's effects.
///
/// ```text
///   EntityType.create(level, TRIGGERED)      the constructor, no finalizeSpawn
///   IronGolem.setPlayerCreated(true)          iron only, before the spawn
///   entity.snapTo(x, y, z, 0.0F, 0.0F)
///   level.addFreshEntity(entity)              pairing, no ambient sound
///   CopperGolem.spawn(weather)                copper only, after the chest
/// ```
///
/// **`snapTo` writes `yRot` and `xRot` and nothing else**, so `yHeadRot` is
/// the constructor's unseeded `nextFloat() * 6.2831855F` draw — which is why
/// `788` and `789` do not observe `add_entity` (vanilla against itself
/// disagrees on that one byte). `yBodyRot` stays 0.
///
/// `None` when the tables a mob needs are absent.
pub fn spawn_golem(
    world: &'static crate::tiles::Tiles,
    req: &crate::golem::GolemSpawn,
    entities: &Entities,
    tracker: &Tracker,
    players: &[TrackedPlayer],
    kinds: &KindIds,
) -> Option<i32> {
    let kind = kind_by_name(req.golem.entity_type())?;
    let level = mob_level(world)?;
    let table = attribute_table()?;
    let id = entities.next_id();
    let mut m = ServerMob::new(level, kind, id, table)?;
    if req.golem == crate::golem::Golem::Iron {
        golem::set_player_created(&mut m, true);
    }
    m.snap_to(req.at.0, req.at.1, req.at.2, 0.0, 0.0);
    let slot = build_ai(m);
    let record = mirror(&slot.ai.mob);
    entities.insert(record.clone());
    set_tick_players(players);
    Mobs::shared().insert(id, slot);
    tracker.add(&record, players, kinds);
    if let Some(w) = req.weather {
        Mobs::shared().with_mut(id, |s| golem::copper_golem_spawn(&mut s.ai.mob, w));
    }
    Some(id)
}

/// `Block.popResource(level, pos, stack)`, offsets 0-104: three
/// `Mth.nextDouble(level.random, -0.25, 0.25)` draws for x, y, z — y lowered
/// by half the item's height — then, when `block_drops` holds, the item
/// entity (`setDefaultPickUpDelay`, `addFreshEntity`).
pub fn pop_resource(
    pos: BlockPos,
    entities: &Entities,
    tracker: &Tracker,
    players: &[TrackedPlayer],
    kinds: &KindIds,
) {
    // `Mth.nextDouble(r, min, max)`: `min >= max ? min : r.nextDouble() *
    // (max - min) + min`.
    let jitter = || level_random(|r| r.next_f64() * 0.5 + -0.25);
    // `EntityTypes.ITEM.getHeight() / 2.0` — `0.25F` widened, halved.
    let h = 0.125;
    let x = f64::from(pos.x) + 0.5 + jitter();
    let y = f64::from(pos.y) + 0.5 + jitter() - h;
    let z = f64::from(pos.z) + 0.5 + jitter();
    let drops_on = matches!(
        crate::gamerules::GameRules::shared().get("block_drops"),
        Some(crate::gamerules::GameRuleValue::Bool(true))
    );
    if !drops_on {
        return;
    }
    let e = drops::item_entity_at(entities, (x, y, z));
    entities.insert(e.clone());
    tracker.add(&e, players, kinds);
}

/// `Player.attack` against an armor stand: the same plan, with
/// `ArmorStand.hurtServer` in place of `LivingEntity`'s — see
/// [`armor_stand::hurt_server`]. The stand's health never moves, so
/// `damageStatsAndHearts` has nothing to show; the attacker's sounds follow
/// the hit exactly as they follow a mob's.
///
/// **Not ported, and named:** `showBreakingParticles` — this server sends no
/// `level_particles`. The dropped item carries no stack on the wire, the
/// known `crate::synched` gap every dropped item here shares.
fn attack_armor_stand(
    target_id: i32,
    attacker_pos: (f64, f64, f64),
    swing: &crate::attack::Swing,
    entities: &Entities,
    tracker: &Tracker,
    players: &[TrackedPlayer],
) -> MobAttack {
    let store = Mobs::shared();
    let plan = crate::attack::melee(swing);
    let early: Vec<&'static str> = plan
        .sounds
        .iter()
        .take(usize::from(plan.knockback_sprint))
        .copied()
        .collect();
    let late: Vec<&'static str> = plan.sounds[early.len()..].to_vec();
    for event in early {
        play_attacker_sound(attacker_pos, event, players);
    }
    let creative = crate::status::game_type() == crate::status::CREATIVE;
    let Some(mut slot) = store.take(target_id) else {
        return MobAttack::NotAMob;
    };
    let game_time = slot.ai.mob.level.game_time;
    let removed = slot.ai.mob.removed;
    let hit = armor_stand::hurt_server(
        &mut slot.ai.mob.stand_last_hit,
        removed,
        game_time,
        creative,
    );
    match hit {
        armor_stand::Hit::Refused => {
            store.put_back(target_id, slot);
            return MobAttack::NoDamage;
        }
        armor_stand::Hit::Wobble => {
            if let Ok(body) = entity_event_body(target_id, 32) {
                tracker.send_to_tracking_players(
                    target_id,
                    players,
                    rustmc_proto::ids::play::cb::ENTITY_EVENT,
                    &body,
                );
            }
            store.put_back(target_id, slot);
        }
        armor_stand::Hit::Broken { drops } => {
            // `brokenByPlayer` 0-32: `Block.popResource(level,
            // blockPosition(), new ItemStack(ARMOR_STAND))` — before the
            // sound, which is `brokenByAnything`'s.
            if drops {
                let at = slot.ai.mob.block_position();
                if let Some(kinds) = crate::entities::shared_kind_ids() {
                    pop_resource(at, entities, tracker, players, kinds);
                }
            }
            // `brokenByAnything` 1: `playBrokenSound` — `ARMOR_STAND_BREAK` at
            // the stand's `getSoundSource()`, `Entity`'s NEUTRAL, 1.0, 1.0.
            // Then `dropAllDeathLoot`: `entities/armor_stand` has no pools,
            // and the stand holds no equipment here.
            play_level_sound(&slot.ai.mob, "minecraft:entity.armor_stand.break", 1.0, 1.0);
            // `kill` -> `remove(KILLED)`.
            slot.ai.mob.removed = true;
            store.forget(target_id);
            entities.remove(target_id);
            tracker.remove(target_id, players);
        }
    }
    for event in late {
        play_attacker_sound(attacker_pos, event, players);
    }
    MobAttack::Hurt {
        health: 20.0,
        died: matches!(hit, armor_stand::Hit::Broken { .. }),
        drops: 0,
        experience: 0,
        sounds: plan.sounds,
    }
}

/// `Level.getEntities(null, aabb)`: every entity whose box intersects,
/// players included (no spectators exist here).
#[must_use]
pub fn entities_in(aabb: &Aabb) -> Vec<i32> {
    Entities::shared()
        .all()
        .into_iter()
        .filter(|e| entity_bb(e).intersects(aabb))
        .map(|e| e.id)
        .collect()
}

/// `ArmorStandItem.useOn` for a placement position the caller resolved:
/// build the stand ([`armor_stand::use_on`]), `addFreshEntityWithPassengers`
/// it — pairing, synchronously — and then play `ARMOR_STAND_PLACE` from its
/// position at `BLOCKS`, `0.75F`, `0.8F`, excluding nobody.
///
/// # Errors
/// [`armor_stand::Refusal`] for the item's `FAIL`.
#[expect(
    clippy::too_many_arguments,
    reason = "the click's three plus the level's four handles"
)]
pub fn use_armor_stand(
    world: &'static crate::tiles::Tiles,
    pos: BlockPos,
    face_is_down: bool,
    rotation: f32,
    entities: &Entities,
    tracker: &Tracker,
    players: &[TrackedPlayer],
    kinds: &KindIds,
) -> Result<i32, armor_stand::Refusal> {
    let level = mob_level(world).ok_or(armor_stand::Refusal::NoTables)?;
    let table = attribute_table().ok_or(armor_stand::Refusal::NoTables)?;
    let m = armor_stand::use_on(level, pos, face_is_down, rotation, table, || {
        entities.next_id()
    })?;
    let slot = build_ai(m);
    let id = slot.ai.mob.id;
    let record = mirror(&slot.ai.mob);
    entities.insert(record.clone());
    set_tick_players(players);
    Mobs::shared().insert(id, slot);
    tracker.add(&record, players, kinds);
    Mobs::shared().with(id, |s| {
        play_level_sound_in(
            &s.ai.mob,
            "minecraft:entity.armor_stand.place",
            crate::sounds::SOURCE_BLOCKS,
            0.75,
            0.8,
        );
    });
    Ok(id)
}

// ---- the tick ----

/// The `EntityTickList.forEach` body for every mob, in order, then the
/// mirror back into the entity list and the removal of anything
/// discarded. `game_time` and `day_time` are the level's clocks.
///
/// ```java
/// if (entity.isRemoved()) return;
/// entity.checkDespawn();
/// if (!(entity instanceof ServerPlayer) && !inEntityTickingRange(chunk)) return;
/// entity.setOldPosAndRot(); entity.tickCount++; entity.tick();
/// ```
pub fn tick_all(
    game_time: i64,
    day_time: i64,
    entities: &Entities,
    tracker: &Tracker,
    players: &[TrackedPlayer],
    server_view_distance: i32,
) {
    breed::with_players(players, || {
        tick_all_inner(
            game_time,
            day_time,
            entities,
            tracker,
            players,
            server_view_distance,
        );
    });
}

fn tick_all_inner(
    game_time: i64,
    day_time: i64,
    entities: &Entities,
    tracker: &Tracker,
    players: &[TrackedPlayer],
    server_view_distance: i32,
) {
    let store = Mobs::shared();
    set_tick_players(players);
    for id in store.ids() {
        let Some(mut slot) = store.take(id) else {
            continue;
        };
        slot.ai.mob.level.game_time = game_time;
        slot.ai.mob.level.day_time = day_time;
        // **Only a mob in the entity tick list is asked to despawn**, and
        // the list is not the level: see [`in_entity_tick_list`].
        if !slot.ai.mob.removed && in_entity_tick_list(&slot.ai.mob, players, server_view_distance)
        {
            check_despawn(&mut slot.ai);
            if !slot.ai.mob.removed && in_entity_ticking_range(&slot.ai.mob, players) {
                slot.ai.mob.set_old_pos_and_rot();
                slot.ai.mob.tick_count += 1;
                // `LivingEntity.tick` offsets 491..513:
                // `isDeadOrDying() && shouldTickDeath()` -> `tickDeath()`,
                // and the whole of the living tick is skipped. A dying mob
                // does not walk, does not look and does not make an ambient
                // sound; it counts to twenty and vanishes. `living::tick`
                // has carried the offsets as a comment since `mob-in-world`
                // and this is the branch they named.
                //
                // `shouldTickDeath()` is `!(this instanceof Player)` on
                // `LivingEntity`, so it is true for every kind here.
                if slot.ai.mob.is_dead_or_dying() {
                    if combat::tick_death(&mut slot.ai.mob) == combat::DeathTick::Removed {
                        if let Ok(body) = entity_event_body(id, combat::ENTITY_EVENT_POOF) {
                            tracker.send_to_tracking_players(
                                id,
                                players,
                                rustmc_proto::ids::play::cb::ENTITY_EVENT,
                                &body,
                            );
                        }
                    }
                } else {
                    living::tick(&mut slot.ai);
                    if let DataLayout::Bat = slot.ai.mob.kind.data {
                        bat::tick_after(&mut slot.ai.mob);
                    }
                }
            }
        }
        if slot.ai.mob.removed {
            store.forget(id);
            entities.remove(id);
            tracker.remove(id, players);
            continue;
        }
        let record = mirror(&slot.ai.mob);
        entities.update(id, |e| *e = record.clone());
        store.put_back(id, slot);
    }
    // Thrown eggs, after every mob — see `egg`'s module docs for the order.
    egg::tick_all(entities, tracker, players);
}

// ---- being attacked ----

/// `ClientboundEntityEventPacket`'s body: a **four-byte int** entity id and
/// one byte.
///
/// The id is `writeInt`, not `writeVarInt` — the only clientbound entity
/// packet in this file that is not a varint, and a varint here would decode
/// as a different entity for every id above 127.
///
/// # Errors
/// Only if the buffer write fails, which for a `Vec` it cannot.
pub fn entity_event_body(entity_id: i32, event: u8) -> std::io::Result<Vec<u8>> {
    let mut b = Vec::with_capacity(5);
    b.extend_from_slice(&entity_id.to_be_bytes());
    b.push(event);
    Ok(b)
}

/// `ClientboundDamageEventPacket`'s body.
///
/// ```text
///   writeVarInt(entityId)
///   DamageType.STREAM_CODEC          -> writeVarInt(registryId + 1)
///   writeOptionalEntityId(causeId)   -> writeVarInt(id + 1), -1 => 0
///   writeOptionalEntityId(directId)  -> the same
///   writeOptional(sourcePosition)    -> bool, then three f64
/// ```
///
/// The `+ 1` on the damage type is `ByteBufCodecs.holderRegistry`'s, which
/// reserves 0 for a direct (inline) holder; the `+ 1` on the two entity ids is
/// `writeOptionalEntityId`'s, which reserves 0 for "no entity". They are two
/// different conventions that happen to look the same, and `tools/oracle-runner`'s
/// decoder handles them differently for that reason — it normalises the entity
/// ids through `entity_ref` and leaves the source as a raw number.
///
/// # Errors
/// Only if the buffer write fails, which for a `Vec` it cannot.
pub fn damage_event_body(
    entity_id: i32,
    damage_type_id: i32,
    cause: Option<i32>,
    direct: Option<i32>,
    at: Option<(f64, f64, f64)>,
) -> std::io::Result<Vec<u8>> {
    use rustmc_proto::varint::write_varint;
    let mut b = Vec::with_capacity(24);
    write_varint(&mut b, entity_id)?;
    write_varint(&mut b, damage_type_id + 1)?;
    write_varint(&mut b, cause.map_or(0, |id| id + 1))?;
    write_varint(&mut b, direct.map_or(0, |id| id + 1))?;
    match at {
        Some((x, y, z)) => {
            b.push(1);
            b.extend_from_slice(&x.to_be_bytes());
            b.extend_from_slice(&y.to_be_bytes());
            b.extend_from_slice(&z.to_be_bytes());
        }
        None => b.push(0),
    }
    Ok(b)
}

/// What `attack_mob` did, for a caller deciding which sound to play.
#[derive(Debug, Clone, PartialEq)]
pub enum MobAttack {
    /// The id named no mob in the store.
    NotAMob,
    /// `hurtOrSimulate` returned false. The caller plays
    /// `PLAYER_ATTACK_NODAMAGE`, exactly as `Player.attack` offset 344 does.
    NoDamage,
    /// The hit landed. The sounds the **attacker** owes are in `sounds`, in
    /// order; the mob's own hurt or death sound has already been played from
    /// inside `hurtServer`'s arm, which is where vanilla plays it.
    Hurt {
        health: f32,
        died: bool,
        drops: usize,
        experience: i32,
        sounds: Vec<&'static str>,
    },
}

/// A player's melee attack on a mob, from `handleAttack`'s `player.attack`
/// through to the drops.
///
/// The whole sequence lives here rather than in the packet handler because
/// its **order** is the thing a corpus check grades, and an order split across
/// two files is one nobody can read:
///
/// ```text
///   Player.attack     the sprint-knockback sound, if any
///   hurtServer        damage_event, the health, the knockback,
///                     the mob's hurt sound OR its death
///   die               entity_event 3, the death sound,
///                     dropAllDeathLoot -> item entities, then the orbs
///   Player.attack     the crit / strong / weak / sweep sound
/// ```
///
/// The attacker's sounds are split around the hit for that reason: the sprint
/// knockback is played at `Player.attack` offset 133, **before**
/// `hurtOrSimulate` at 242, and everything else at 289 and after.
#[expect(
    clippy::too_many_arguments,
    reason = "the level's four handles plus the attacker's four"
)]
pub fn attack_mob(
    target_id: i32,
    attacker_id: i32,
    attacker_pos: (f64, f64, f64),
    swing: &crate::attack::Swing,
    damage_type_id: i32,
    entities: &Entities,
    tracker: &Tracker,
    players: &[TrackedPlayer],
    kinds: &KindIds,
) -> MobAttack {
    let store = Mobs::shared();
    if store.with(target_id, |_| ()).is_none() {
        return MobAttack::NotAMob;
    }
    set_tick_players(players);
    if store.with(target_id, |s| !s.ai.mob.kind.is_mob()) == Some(true) {
        return attack_armor_stand(target_id, attacker_pos, swing, entities, tracker, players);
    }
    let plan = crate::attack::melee(swing);
    // Offset 133: the one sound that precedes the hit.
    let early: Vec<&'static str> = plan
        .sounds
        .iter()
        .take(usize::from(plan.knockback_sprint))
        .copied()
        .collect();
    let late: Vec<&'static str> = plan.sounds[early.len()..].to_vec();

    let source = combat::MobDamageSource {
        type_id: damage_type_id,
        // `minecraft:player_attack`'s tag memberships. This server resolves
        // damage-type tags nowhere, so the set is empty — which is CORRECT
        // for this type: `ci/damage-types-golden.sh` on `player-systems`
        // measures `player_attack` in no tag that `hurtServer` reads, and
        // `bypasses_cooldown` has no data file at all in 26.2 and is empty
        // for all 51 types. It is an empty set that has been checked, not a
        // default.
        tags: crate::health::DamageTags::default(),
        causing_entity: Some(attacker_id),
        direct_entity: Some(attacker_id),
        // `DamageSource.getSourcePosition()` for an entity source is the
        // causing entity's position, which is what sends the knockback away
        // from the attacker rather than into the jitter loop.
        source_position: Some(attacker_pos),
    };

    for event in early {
        play_attacker_sound(attacker_pos, event, players);
    }

    let Some(mut slot) = store.take(target_id) else {
        return MobAttack::NotAMob;
    };
    // `hurtOrSimulate(source, damage)` at `Player.attack` offset 242. The
    // attacker is a player, which is `resolvePlayerResponsibleForDamage`'s
    // own test and what sets `lastHurtByPlayerMemoryTime` — inside the hit,
    // not before it.
    // `IronGolem.hurtServer` 1: the crackiness before `super.hurtServer`.
    let cracked_before = golem::crackiness(&slot.ai.mob);
    let out = combat::hurt(&mut slot.ai.mob, &source, plan.damage, false, true);

    // `ifeq 344` — the branch is on `hurtOrSimulate`'s **return value** and
    // nothing else. That value is `!wasBlocked || damage > 0.0`, so a hit
    // blocked entirely down to zero takes this arm too even though it was not
    // refused; testing `refused.is_some()` instead would send the wrong sound
    // for the first shield this server holds.
    if !out.hurt.hurt {
        store.put_back(target_id, slot);
        return MobAttack::NoDamage;
    }

    if out.damage_event {
        if let Ok(body) = damage_event_body(
            target_id,
            damage_type_id,
            Some(attacker_id),
            Some(attacker_id),
            None,
        ) {
            tracker.send_to_tracking_players(
                target_id,
                players,
                rustmc_proto::ids::play::cb::DAMAGE_EVENT,
                &body,
            );
        }
    }
    if let Some(event) = out.hurt_sound {
        let volume = slot.ai.mob.kind.sound_volume;
        let pitch = living::voice_pitch(&mut slot.ai.mob);
        living::play_sound(&slot.ai.mob, event, volume, pitch);
    }

    let mut drops = 0usize;
    let mut experience = 0;
    if let Some(death) = &out.death {
        if let Ok(body) = entity_event_body(target_id, death.entity_event) {
            tracker.send_to_tracking_players(
                target_id,
                players,
                rustmc_proto::ids::play::cb::ENTITY_EVENT,
                &body,
            );
        }
        if let Some(event) = death.death_sound {
            let volume = slot.ai.mob.kind.sound_volume;
            let pitch = living::voice_pitch(&mut slot.ai.mob);
            living::play_sound(&slot.ai.mob, event, volume, pitch);
        }
        if death.dropped_loot {
            drops = spawn_death_loot(
                &slot.ai.mob,
                death.hit_by_player,
                attacker_id,
                entities,
                tracker,
                players,
                kinds,
            );
        }
        experience = spawn_experience(&mut slot.ai.mob, entities, tracker, players, kinds);
    }
    // `IronGolem.hurtServer` 17-35: after `super.hurtServer` returned true —
    // so after its hurt sound and any death — a golem whose crackiness moved
    // plays `IRON_GOLEM_DAMAGE` at 1.0, 1.0.
    if cracked_before.is_some() && golem::crackiness(&slot.ai.mob) != cracked_before {
        living::play_sound(&slot.ai.mob, "minecraft:entity.iron_golem.damage", 1.0, 1.0);
    }

    let health = slot.ai.mob.health();
    let died = out.death.is_some();
    // The mob stays in the store while it dies: `tickDeath` counts twenty
    // ticks before `remove(KILLED)`, and `mob::tick_all` runs it.
    entities.update(target_id, |e| {
        let record = mirror(&slot.ai.mob);
        *e = record;
    });
    store.put_back(target_id, slot);

    for event in late {
        play_attacker_sound(attacker_pos, event, players);
    }

    MobAttack::Hurt {
        health,
        died,
        drops,
        experience,
        sounds: plan.sounds,
    }
}

/// `Entity.getBoundingBox()` for a mob, as `handleAttack`'s reach test needs
/// it.
///
/// The box is built from the kind's `sized(w, h)` and the mob's **current**
/// dimensions, so a baby pig's box is the baby pair rather than the adult
/// one — `EntityType.getDimensions` is overridden by
/// `AgeableMob.getDefaultDimensions`, and a reach test against the adult box
/// would let a player hit a piglet from further away than vanilla allows.
#[must_use]
pub fn mob_box(id: i32) -> Option<crate::attack::Aabb> {
    Mobs::shared().with(id, |slot| {
        let m = &slot.ai.mob;
        crate::attack::Aabb::of_entity(
            m.x(),
            m.y(),
            m.z(),
            f64::from(m.bb_width()),
            f64::from(m.bb_height()),
        )
    })
}

/// `Player.playServerSideSound(SoundEvent)` — `aconst_null` in the entity
/// slot, so the attacker hears it too.
fn play_attacker_sound(at: (f64, f64, f64), event: &str, players: &[TrackedPlayer]) {
    let conns: Vec<std::sync::Arc<crate::players::Connection>> = players
        .iter()
        .map(|p| std::sync::Arc::clone(&p.conn))
        .collect();
    let Some(ids) = crate::protocol_ids::shared() else {
        return;
    };
    let Some(id) = ids.sound(event) else {
        return;
    };
    let sound = crate::sounds::Sound {
        event: event.to_string(),
        source: crate::sounds::SOURCE_PLAYERS,
        at: crate::sounds::Eighths::exact(at.0, at.1, at.2),
        pos: at,
        volume: 1.0,
        pitch: 1.0,
        except: None,
    };
    let Ok(body) = crate::sounds::sound_body(&sound, id) else {
        return;
    };
    for c in conns {
        let _ = c.send(rustmc_proto::ids::play::cb::SOUND, &body);
    }
}

/// `dropAllDeathLoot`'s item half. Returns how many stacks reached the world.
fn spawn_death_loot(
    m: &ServerMob,
    hit_by_player: bool,
    attacker_id: i32,
    entities: &Entities,
    tracker: &Tracker,
    players: &[TrackedPlayer],
    kinds: &KindIds,
) -> usize {
    let attacker = drops::attacker_of(attacker_id);
    let ctx = drops::loot_context(m, "minecraft:player_attack", hit_by_player, attacker);
    // `Player.getLuck()` is the LUCK attribute, default 0. Read as the
    // constant it is rather than plumbed, because nothing on this server can
    // change it and a plumbed zero reads as a measurement.
    let Ok(stacks) = drops::death_loot(m, ctx, 0.0) else {
        // A refusal is NOT an empty drop. Nothing is spawned and the count is
        // zero, which is the same number an honestly empty table gives — so
        // the caller cannot tell them apart and must not try. What separates
        // them is `crates/rustmc-loot`'s own `entity_dispatch` test, which
        // grades the refusal by name.
        return 0;
    };
    let mut n = 0;
    for stack in &stacks {
        if stack.count <= 0 {
            continue;
        }
        let e = drops::spawn_death_drop(entities, m, stack);
        entities.insert(e.clone());
        tracker.add(&e, players, kinds);
        n += 1;
    }
    n
}

/// `dropExperience` — the guard, the reward draw and `ExperienceOrb.award`,
/// through `crate::xporb`'s orb entity (split, `tryMergeToExisting` on the
/// level's random, a fresh `RandomSource` per orb), so the orbs a death
/// leaves tick and can be picked up.
fn spawn_experience(
    m: &mut ServerMob,
    entities: &Entities,
    tracker: &Tracker,
    players: &[TrackedPlayer],
    kinds: &KindIds,
) -> i32 {
    let amount = drops::experience_to_drop(m);
    if amount <= 0 {
        return 0;
    }
    let level = crate::xporb::Level {
        entities,
        tracker,
        players,
        kinds,
    };
    crate::xporb::award(
        &level,
        crate::collide::Vec3::new(m.x(), m.y(), m.z()),
        amount,
    );
    amount
}

/// Whether a mob is in `ServerLevel.entityTickList` at all — the list
/// `ServerLevel.tick`'s `forEach` walks, and so the only mobs `checkDespawn`
/// ever runs on.
///
/// `PersistentEntitySectionManager` puts an entity in that list when its
/// chunk reaches `FullChunkStatus.ENTITY_TICKING` (`Visibility.TICKING`) and
/// takes it out when the chunk drops below. With only players holding
/// tickets, a chunk is `ENTITY_TICKING` exactly when it carries
/// `TicketType.PLAYER_LOADING` at `PLAYER_TICKET_LEVEL` =
/// `ChunkLevel.byStatus(ENTITY_TICKING)`, and
/// `DistanceManager.PlayerTicketTracker.haveTicketFor(level)` gives one to
/// every chunk whose Chebyshev distance to a player's chunk is at most the
/// view distance `ChunkMap.setServerViewDistance` passed to
/// `updatePlayerTickets` — `Mth.clamp(viewDistance, 2, 32)`. The
/// simulation-distance ticket (below) decides ticking; it loads nothing.
///
/// **Found on `082`**: a bat left 150 blocks away while the player walks
/// back to the origin at view distance 4 is in a chunk nobody holds a
/// loading ticket for, so vanilla neither ticks it nor despawns it, and it
/// is still there — `add_entity` kind 10 — when the player returns. This
/// server ran `checkDespawn` on every mob every tick, and the bat, past the
/// 128-block despawn distance, was discarded.
fn in_entity_tick_list(
    m: &ServerMob,
    players: &[TrackedPlayer],
    server_view_distance: i32,
) -> bool {
    let reach = server_view_distance.clamp(2, 32);
    let (cx, cz) = (m.block_position().x >> 4, m.block_position().z >> 4);
    players
        .iter()
        .any(|p| (p.chunk.0 - cx).abs() <= reach && (p.chunk.1 - cz).abs() <= reach)
}

/// `DistanceManager.inEntityTickingRange(chunkPos)`: the chunk holds an
/// entity-ticking ticket, which a player's simulation ticket
/// (`simulation-distance`, 10 on the corpus's vanilla) grants to every
/// chunk within that many chunks of it. This server has no ticket
/// levels; the Chebyshev radius is the same predicate for the loaded
/// square a player stands in.
fn in_entity_ticking_range(m: &ServerMob, players: &[TrackedPlayer]) -> bool {
    const SIMULATION_DISTANCE: i32 = 10;
    let (cx, cz) = (m.block_position().x >> 4, m.block_position().z >> 4);
    players.iter().any(|p| {
        (p.chunk.0 - cx).abs() <= SIMULATION_DISTANCE
            && (p.chunk.1 - cz).abs() <= SIMULATION_DISTANCE
    })
}

/// `Mob.checkDespawn()`, offsets 0-189.
fn check_despawn(ai: &mut MobAi<ServerMob>) {
    let m = &mut ai.mob;
    // `Entity.checkDespawn()` is empty; only `Mob` overrides it.
    if !m.kind.is_mob() {
        return;
    }
    // 0-27: peaceful and not allowed → discard.
    if difficulty() == Difficulty::Peaceful && !m.kind.allowed_in_peaceful {
        m.discard();
        return;
    }
    // 28-47: persistence, or custom persistence (passenger / leashed).
    if m.persistence_required {
        ai.no_action_time = 0;
        return;
    }
    // 48-59: `level.getNearestPlayer(this, -1.0)` — NO_SPECTATORS, no range.
    let (mx, my, mz) = (m.x(), m.y(), m.z());
    let mut nearest: Option<(i32, f64)> = None;
    for pid in player_ids_in_join_order() {
        let Some(p) = Entities::shared().get(pid) else {
            continue;
        };
        let d = (p.x - mx) * (p.x - mx) + (p.y - my) * (p.y - my) + (p.z - mz) * (p.z - mz);
        // `-1 < 0 || d < -1 * -1` → `range < 0` admits every distance; the
        // running minimum takes the strictly closer.
        if nearest.is_none_or(|(_, best)| d < best) {
            nearest = Some((pid, d));
        }
    }
    let Some((_, d0)) = nearest else {
        return;
    };
    // 70-108.
    let i = m.kind.category.despawn_distance();
    let j = i * i;
    if d0 > f64::from(j) && m.kind.remove_when_far_away {
        m.discard();
    }
    // 109-189.
    let k = m.kind.category.no_despawn_distance();
    let l = k * k;
    if ai.no_action_time > 600
        && ai.mob.random.next_i32_bound(800) == 0
        && d0 > f64::from(l)
        && ai.mob.kind.remove_when_far_away
    {
        ai.mob.discard();
    } else if d0 < f64::from(l) {
        ai.no_action_time = 0;
    }
}

// ---- what the tracker asks ----

/// A mob as `ServerEntity.sendChanges` reads it — see
/// [`crate::tracker::DirtySource`].
pub struct MobSync(pub i32);

/// A drained dirty state, as the item and orb stores hand it over:
/// `sendDirtyEntityData`'s two packets and `needsSync`. See
/// `crate::tracker::Drained`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Dirty {
    pub data: Option<Vec<u8>>,
    pub attributes: Option<Vec<u8>>,
    pub needs_sync: bool,
}

impl crate::tracker::DirtySource for MobSync {
    fn flags(&self) -> (bool, bool) {
        Mobs::shared()
            .with(self.0, |s| (s.ai.mob.data.is_dirty(), s.ai.mob.needs_sync))
            .unwrap_or((false, false))
    }

    fn drain(&self) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
        let id = self.0;
        Mobs::shared()
            .with_mut(id, |s| {
                let m = &mut s.ai.mob;
                let data = m
                    .data
                    .pack_dirty()
                    .and_then(|values| crate::synched::set_entity_data_body(id, &values).ok());
                let attrs = m.attributes.take_to_sync();
                let attributes = if attrs.is_empty() {
                    None
                } else {
                    crate::protocol_ids::shared()
                        .and_then(|ids| attributes::update_attributes_body(ids, id, &attrs).ok())
                };
                (data, attributes)
            })
            .unwrap_or((None, None))
    }

    fn take_hurt_marked(&self) -> bool {
        Mobs::shared()
            .with_mut(self.0, |s| std::mem::take(&mut s.ai.mob.hurt_marked))
            .unwrap_or(false)
    }
}

/// `Entity.needsSync = false`, at `sendChanges` offset 1189.
pub fn clear_needs_sync(id: i32) {
    Mobs::shared().with_mut(id, |s| s.ai.mob.needs_sync = false);
}

/// `(set_entity_data body, update_attributes body)`, either absent.
pub type PairingExtras = (Option<Vec<u8>>, Option<Vec<u8>>);

thread_local! {
    /// A mob being paired on the thread that spawned it, before it is in
    /// the store: `(id, (set_entity_data body, update_attributes body))`.
    static PENDING_PAIRING: std::cell::RefCell<Option<(i32, PairingExtras)>> =
        const { std::cell::RefCell::new(None) };
}

/// The pairing bundle's mob half: `trackedDataValues` — the non-default
/// values — and `getSyncableAttributes()`. `None` for a non-mob.
#[must_use]
pub fn pairing_extras(id: i32) -> Option<PairingExtras> {
    if let Some(p) = PENDING_PAIRING.with(|p| p.borrow().clone()) {
        if p.0 == id {
            return Some(p.1);
        }
    }
    Mobs::shared().with(id, |s| {
        let m = &s.ai.mob;
        let data = m
            .data
            .non_default_values()
            .and_then(|values| crate::synched::set_entity_data_body(id, &values).ok());
        let syncable = m.attributes.syncable();
        let attributes = if syncable.is_empty() {
            None
        } else {
            crate::protocol_ids::shared()
                .and_then(|ids| attributes::update_attributes_body(ids, id, &syncable).ok())
        };
        (data, attributes)
    })
}

/// `PathLevel` for a pathfinding-cost read the host makes through the
/// level.
pub fn light_cost(level: &MobLevel, pos: BlockPos) -> f32 {
    level::pathfinding_cost_from_light_levels(level, pos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spawn_egg_names_its_kind_by_the_item_name() {
        assert_eq!(
            kind_for_spawn_egg("minecraft:pig_spawn_egg").map(|k| k.name),
            Some("minecraft:pig")
        );
        assert_eq!(
            kind_for_spawn_egg("minecraft:bat_spawn_egg").map(|k| k.name),
            Some("minecraft:bat")
        );
        assert!(kind_for_spawn_egg("minecraft:zombie_spawn_egg").is_none());
        assert!(kind_for_spawn_egg("minecraft:egg").is_none());
    }

    #[test]
    fn the_two_kinds_carry_the_jars_builder_values() {
        assert_eq!(pig::PIG.entity_kind.tracking_range_chunks, 10);
        assert_eq!(pig::PIG.entity_kind.update_interval, 3);
        assert!(pig::PIG.entity_kind.track_deltas);
        assert!((pig::PIG.eye_height - 0.765).abs() < 1e-6);
        assert_eq!(bat::BAT.entity_kind.tracking_range_chunks, 5);
        assert!(!bat::BAT.entity_kind.track_deltas);
        assert_eq!(bat::BAT.eye_height, 0.45);
        assert!(!pig::PIG.remove_when_far_away && bat::BAT.remove_when_far_away);
    }

    #[test]
    fn difficulty_defaults_to_easy_and_names_are_the_four() {
        assert_eq!(set_difficulty("peaceful"), Ok(()));
        assert_eq!(difficulty(), Difficulty::Peaceful);
        assert_eq!(difficulty_id(), 0);
        assert_eq!(set_difficulty("hard"), Ok(()));
        assert_eq!(difficulty_id(), 3);
        assert_eq!(set_difficulty("creative"), Err("creative".to_string()));
        assert_eq!(set_difficulty("easy"), Ok(()));
    }

    #[test]
    fn the_variants_table_parses_its_own_dumpers_shape() {
        let doc = serde_json::json!({
            "pig_variant": {
                "minecraft:cold": [{"priority": 1, "biomes": ["minecraft:snowy_plains"]}],
                "minecraft:temperate": [{"priority": 0, "biomes": null}]
            },
            "pig_sound_variant": {
                "minecraft:classic": {"ambient": "a", "step": "s", "hurt": "h", "death": "d",
                                      "eat": "e", "baby_ambient": "ba", "baby_step": "bs",
                                      "baby_hurt": "bh", "baby_death": "bd", "baby_eat": "be"}
            },
            "sky_light_level": {"period_ticks": 24000, "ease": "linear", "modifier": "multiply",
                                "keyframes": [[133, 1.0], [11867, 1.0], [13670, 0.26666668], [22330, 0.26666668]]}
        });
        let t = MobVariants::from_json(&doc).expect("parses");
        assert_eq!(t.pig_variant["minecraft:temperate"][0].biomes, None);
        assert_eq!(t.pig_sound_variant["minecraft:classic"].ambient, "a");
        assert_eq!(t.sky_light_level.keyframes.len(), 4);
        // `KeyframeTrackSampler.sample`, linear: noon sits on the flat top,
        // midnight on the flat bottom, dawn (0) on the rising segment that
        // wraps from 22330 to 133 + 24000.
        let s = &t.sky_light_level;
        assert_eq!(s.sample(6000), 1.0);
        assert_eq!(s.sample(18000), 0.266_666_68);
        let dawn = s.sample(0);
        assert!(dawn > 0.9 && dawn < 1.0, "{dawn}");
        assert_eq!(level::sky_darken_from(s, 6000), 0);
        assert_eq!(level::sky_darken_from(s, 18000), 11);
    }
}
