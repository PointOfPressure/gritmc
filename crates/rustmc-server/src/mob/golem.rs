//! `IronGolem`, `SnowGolem` and `CopperGolem` — the three mobs
//! `CarvedPumpkinBlock.trySpawnGolem` makes (see [`crate::golem`] for the
//! pattern half).
//!
//! All three are `AbstractGolem → PathfinderMob → Mob`. `AbstractGolem`
//! overrides five things and every one is a constant (`javap -p -c
//! -constants`): `getAmbientSound`, `getHurtSound` and `getDeathSound` are
//! `aconst_null`, `getAmbientSoundInterval` is `bipush 120` and
//! `removeWhenFarAway` is `iconst_0`.
//!
//! # What is ported
//!
//! * the three `EntityTypes` builders ([`IRON_GOLEM`], [`SNOW_GOLEM`],
//!   [`COPPER_GOLEM`]);
//! * each `defineSynchedData` ([`super::data::MobData`]);
//! * `IronGolem.registerGoals` and `SnowGolem.registerGoals` over the goals
//!   `rustmc_entity` already carries;
//! * `IronGolem.aiStep`'s two countdowns, `doPush`'s `nextInt(20)` retarget,
//!   `playStepSound` (`IRON_GOLEM_STEP` at 1.0/1.0), hurt and death sounds;
//! * `CopperGolem.tick`'s server arm, `updateWeathering`, which draws the
//!   first weathering tick from the **level** random on the first tick;
//! * `CopperGolem.spawn(WeatherState)`: `setWeatherState` then
//!   `playSpawnSound` (`COPPER_GOLEM_SPAWN` at 1.0/1.0).
//!
//! # What is not, by name
//!
//! * **`CopperGolem`'s brain** (`copperGolemBrain`, `CopperGolemAi`): no
//!   brain of that shape is ported, so a copper golem here stands where it
//!   was made and does not sort chests. Its `customServerAiStep` is the
//!   `AbstractGolem` chain's bare return.
//! * `SnowGolem.aiStep`'s melt damage (`SNOW_GOLEM_MELTS` is an environment
//!   attribute no table here answers) and its snow trail (a mob-tick block
//!   write, which this server's mob tick has no channel for). Named in
//!   `docs/cold-start/entity-ports.md`.
//! * `IronGolem.doHurtTarget` / `hurtServer`'s crackiness sound and
//!   `SnowGolem.performRangedAttack`: both sit behind a target, and nothing
//!   this server can spawn is an `Enemy`.
//! * `NeutralMob`'s anger persistence beyond the in-memory target and end
//!   time: no save format is written here.

use super::host::ServerMob;
use super::MobKind;
use rustmc_entity::ai::goals::defend_village_target_goal::DefendVillageTargetGoal;
use rustmc_entity::ai::goals::golem_random_stroll_in_village_goal::GolemRandomStrollInVillageGoal;
use rustmc_entity::ai::goals::look_at_player_goal::LookAtPlayerGoal;
use rustmc_entity::ai::goals::melee_attack_goal::MeleeAttackGoal;
use rustmc_entity::ai::goals::move_back_to_village_goal::MoveBackToVillageGoal;
use rustmc_entity::ai::goals::move_towards_target_goal::MoveTowardsTargetGoal;
use rustmc_entity::ai::goals::offer_flower_goal::OfferFlowerGoal;
use rustmc_entity::ai::goals::random_look_around_goal::RandomLookAroundGoal;
use rustmc_entity::ai::goals::random_stroll_goal::RandomStrollGoal;
use rustmc_entity::ai::goals::ranged_attack_goal::RangedAttackGoal;
use rustmc_entity::ai::goals::reset_universal_anger_target_goal::ResetUniversalAngerTargetGoal;
use rustmc_entity::ai::goals::target_goal::{HurtByTargetGoal, NearestAttackableTargetGoal};
use rustmc_entity::ai::mob_ai::MobAi;
use rustmc_entity::host::{EntityClass, MobHost};
use rustmc_entity::tick::MobCategory;
use rustmc_worldgen::rng::RandomSource;

/// `EntityTypes.IRON_GOLEM`: `of(IronGolem::new, MISC).sized(1.4F, 2.7F)
/// .clientTrackingRange(10)` — no `eyeHeight`, so `0.85F * 2.7F`.
pub static IRON_GOLEM: MobKind = MobKind {
    name: "minecraft:iron_golem",
    entity_kind: crate::entities::IRON_GOLEM,
    width: 1.4,
    height: 2.7,
    eye_height: 2.7 * 0.85,
    baby_dimensions: None,
    baby_eye_height: None,
    category: MobCategory::Misc,
    allowed_in_peaceful: true,
    data: super::DataLayout::IronGolem,
    ambient_sound_interval: 120,
    sound_volume: 1.0,
    voice_pitch_scale: 1.0,
    pushable: true,
    emits_sounds: true,
    remove_when_far_away: false,
    enemy: false,
};

/// `EntityTypes.SNOW_GOLEM`: `of(SnowGolem::new, MISC).immuneTo(POWDER_SNOW)
/// .sized(0.7F, 1.9F).eyeHeight(1.7F).clientTrackingRange(8)`.
pub static SNOW_GOLEM: MobKind = MobKind {
    name: "minecraft:snow_golem",
    entity_kind: crate::entities::SNOW_GOLEM,
    width: 0.7,
    height: 1.9,
    eye_height: 1.7,
    baby_dimensions: None,
    baby_eye_height: None,
    category: MobCategory::Misc,
    allowed_in_peaceful: true,
    data: super::DataLayout::SnowGolem,
    ambient_sound_interval: 120,
    sound_volume: 1.0,
    voice_pitch_scale: 1.0,
    pushable: true,
    emits_sounds: true,
    remove_when_far_away: false,
    enemy: false,
};

/// `EntityTypes.COPPER_GOLEM`: `of(CopperGolem::new, MISC).sized(0.49F,
/// 0.98F).eyeHeight(0.8125F).clientTrackingRange(10)`.
pub static COPPER_GOLEM: MobKind = MobKind {
    name: "minecraft:copper_golem",
    entity_kind: crate::entities::COPPER_GOLEM,
    width: 0.49,
    height: 0.98,
    eye_height: 0.8125,
    baby_dimensions: None,
    baby_eye_height: None,
    category: MobCategory::Misc,
    allowed_in_peaceful: true,
    data: super::DataLayout::CopperGolem,
    ambient_sound_interval: 120,
    sound_volume: 1.0,
    voice_pitch_scale: 1.0,
    pushable: true,
    emits_sounds: true,
    remove_when_far_away: false,
    enemy: false,
};

/// `IronGolem.setPlayerCreated(boolean)`: bit 1 of `DATA_FLAGS_ID`.
pub fn set_player_created(m: &mut ServerMob, v: bool) {
    let b = m.data.get_byte(super::data::id::IRON_GOLEM_FLAGS);
    let n = if v { b | 1 } else { b & 0xFE };
    m.data.set_byte(super::data::id::IRON_GOLEM_FLAGS, n);
}

#[must_use]
pub fn is_player_created(m: &ServerMob) -> bool {
    m.data.get_byte(super::data::id::IRON_GOLEM_FLAGS) & 1 != 0
}

/// `IronGolem.registerGoals`, `javap` offsets 4-232.
pub fn register_iron_golem_goals(ai: &mut MobAi<ServerMob>) {
    ai.goal_selector
        .add_goal(1, Box::new(MeleeAttackGoal::new(1.0, true)));
    ai.goal_selector
        .add_goal(2, Box::new(MoveTowardsTargetGoal::new(0.9, 32.0)));
    ai.goal_selector
        .add_goal(2, Box::new(MoveBackToVillageGoal::new(0.6, false)));
    ai.goal_selector
        .add_goal(4, Box::new(GolemRandomStrollInVillageGoal::new(0.6)));
    ai.goal_selector
        .add_goal(5, Box::new(OfferFlowerGoal::new()));
    let look = LookAtPlayerGoal::new(ai, EntityClass::Player, 6.0);
    ai.goal_selector.add_goal(7, Box::new(look));
    ai.goal_selector
        .add_goal(8, Box::new(RandomLookAroundGoal::new()));
    ai.target_selector
        .add_goal(1, Box::new(DefendVillageTargetGoal::new()));
    ai.target_selector
        .add_goal(2, Box::new(HurtByTargetGoal::new(Vec::new())));
    // 172-196: `(target, level) -> isAngryAt(target, level)`.
    let angry = NearestAttackableTargetGoal::with(
        ai,
        EntityClass::Player,
        10,
        true,
        false,
        Some(Box::new(|cx: &MobAi<ServerMob>, t| is_angry_at(&cx.mob, t))),
    );
    ai.target_selector.add_goal(3, Box::new(angry));
    // 200-222: `(target, level) -> target instanceof Enemy && !(target
    // instanceof Creeper)`.
    let enemies = NearestAttackableTargetGoal::with(
        ai,
        EntityClass::Mob,
        5,
        false,
        false,
        Some(Box::new(|cx: &MobAi<ServerMob>, t| {
            cx.mob.is_enemy(t.id) && !cx.mob.is_creeper(t.id)
        })),
    );
    ai.target_selector.add_goal(3, Box::new(enemies));
    ai.target_selector
        .add_goal(4, Box::new(ResetUniversalAngerTargetGoal::new(false)));
}

/// `SnowGolem.registerGoals`, offsets 4-100.
pub fn register_snow_golem_goals(ai: &mut MobAi<ServerMob>) {
    ai.goal_selector
        .add_goal(1, Box::new(RangedAttackGoal::new(1.25, 20, 10.0)));
    ai.goal_selector.add_goal(
        2,
        Box::new(RandomStrollGoal::water_avoiding_with_probability(
            1.0,
            1.000_000_1E-5,
        )),
    );
    let look = LookAtPlayerGoal::new(ai, EntityClass::Player, 6.0);
    ai.goal_selector.add_goal(3, Box::new(look));
    ai.goal_selector
        .add_goal(4, Box::new(RandomLookAroundGoal::new()));
    let enemies = NearestAttackableTargetGoal::with(
        ai,
        EntityClass::Mob,
        10,
        true,
        false,
        Some(Box::new(|cx: &MobAi<ServerMob>, t| cx.mob.is_enemy(t.id))),
    );
    ai.target_selector.add_goal(1, Box::new(enemies));
}

/// `NeutralMob.isAngryAt(target, level)`:
///
/// ```text
///   if (!canAttack(target)) return false;
///   if (target.getType() == PLAYER && isAngryAtAllPlayers(level)) return true;
///   return target.getUUID().equals(getPersistentAngerTarget());
/// ```
///
/// with `isAngryAtAllPlayers` = `UNIVERSAL_ANGER && isAngry() &&
/// getPersistentAngerTarget() == null`.
#[must_use]
pub fn is_angry_at(m: &ServerMob, target: &rustmc_entity::host::EntityFacts) -> bool {
    if !m.can_attack(target) {
        return false;
    }
    let angry = m.golem.anger_end_time > m.level.game_time;
    if target.is_player && m.universal_anger() && angry && m.golem.anger_target.is_none() {
        return true;
    }
    m.golem.anger_target == Some(target.id)
}

/// The per-kind state the three golem classes add to `Mob`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GolemState {
    /// `IronGolem.attackAnimationTick`.
    pub attack_animation_tick: i32,
    /// `IronGolem.offerFlowerTick`.
    pub offer_flower_tick: i32,
    /// `NeutralMob`'s persistent anger target, by entity id.
    pub anger_target: Option<i32>,
    /// `IronGolem.persistentAngerEndTime`.
    pub anger_end_time: i64,
    /// `CopperGolem.nextWeatheringTick`, `-1L` at construction.
    pub next_weathering_tick: i64,
}

impl GolemState {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            attack_animation_tick: 0,
            offer_flower_tick: 0,
            anger_target: None,
            anger_end_time: 0,
            next_weathering_tick: -1,
        }
    }
}

/// `IronGolem.getCrackiness()`: `Crackiness.GOLEM.byFraction(getHealth() /
/// getMaxHealth())`, with `GOLEM` built as `(0.75F, 0.5F, 0.25F)` —
/// below a quarter HIGH (3), below a half MEDIUM (2), below three quarters
/// LOW (1), else NONE (0). `None` for anything that is not an iron golem.
#[must_use]
pub fn crackiness(m: &ServerMob) -> Option<u8> {
    if !matches!(m.kind.data, super::DataLayout::IronGolem) {
        return None;
    }
    let f = m.health() / m.max_health();
    Some(if f < 0.25 {
        3
    } else if f < 0.5 {
        2
    } else if f < 0.75 {
        1
    } else {
        0
    })
}

/// `IronGolem.aiStep` offsets 4-60, after `super.aiStep()`: the two
/// countdowns and `updatePersistentAnger(level, true)`.
///
/// `updatePersistentAnger` only acts on an entity that has an anger target
/// or an end time, and this port keeps only those two fields; with neither
/// set it is the method's own fall-through, and with the end time passed
/// the target is dropped the way `stopBeingAngry` drops it.
pub fn iron_golem_ai_step(m: &mut ServerMob) {
    if m.golem.attack_animation_tick > 0 {
        m.golem.attack_animation_tick -= 1;
    }
    if m.golem.offer_flower_tick > 0 {
        m.golem.offer_flower_tick -= 1;
    }
    if m.golem.anger_target.is_some() && m.golem.anger_end_time <= m.level.game_time {
        m.golem.anger_target = None;
        m.golem.anger_end_time = 0;
    }
}

/// `IronGolem.doPush(Entity)` offsets 0-38: an `Enemy` that is not a creeper
/// becomes the target on a one-in-twenty draw from the golem's own random,
/// and the draw is made for every such push. Nothing this server spawns is
/// an `Enemy`, so the draw is never taken; the gate is the class test, not
/// a constant.
pub fn iron_golem_do_push(m: &mut ServerMob, other: i32) {
    if m.is_enemy(other) && !m.is_creeper(other) && m.random.next_i32_bound(20) == 0 {
        m.target = Some(other);
    }
}

/// `CopperGolem.updateWeathering(level, level.getRandom(), gameTime)`.
///
/// ```text
///    1-11   nextWeatheringTick == -2 -> return          (waxed)
///   13-42   nextWeatheringTick == -1 ->
///             nextWeatheringTick = gameTime + random.nextIntBetweenInclusive(504000, 552000)
///   44-133  else if state != OXIDIZED && gameTime >= nextWeatheringTick:
///             next = state.next(); setWeatherState(next)
///             nextWeatheringTick = next == OXIDIZED ? 0 : gameTime + draw
/// ```
///
/// The random is **the level's**, not the golem's (offsets 37-40 in
/// `tick`), so the first tick of every copper golem moves the level stream
/// by one `nextIntBetweenInclusive`. The statue branch (`canTurnToStatue`)
/// is behind `OXIDIZED` and a lightning-free half-million ticks.
pub fn copper_golem_update_weathering(m: &mut ServerMob) {
    let t = m.level.game_time;
    let next = m.golem.next_weathering_tick;
    if next == -2 {
        return;
    }
    if next == -1 {
        let d = super::level_random(|r| next_int_between_inclusive(r, 504_000, 552_000));
        m.golem.next_weathering_tick = t + i64::from(d);
        return;
    }
    let state = weather(m);
    if state != crate::golem::Weather::Oxidized && t >= next {
        let n = state.next();
        set_weather(m, n);
        m.golem.next_weathering_tick = if n == crate::golem::Weather::Oxidized {
            0
        } else {
            let d = super::level_random(|r| next_int_between_inclusive(r, 504_000, 552_000));
            t + i64::from(d)
        };
    }
}

/// `RandomSource.nextIntBetweenInclusive(min, max)`: `nextInt(max - min + 1)
/// + min`.
fn next_int_between_inclusive<R: RandomSource>(r: &mut R, min: i32, max: i32) -> i32 {
    r.next_i32_bound(max - min + 1) + min
}

#[must_use]
pub fn weather(m: &ServerMob) -> crate::golem::Weather {
    match m.data.get_varint(super::data::id::COPPER_GOLEM_WEATHER) {
        1 => crate::golem::Weather::Exposed,
        2 => crate::golem::Weather::Weathered,
        3 => crate::golem::Weather::Oxidized,
        _ => crate::golem::Weather::Unaffected,
    }
}

/// `CopperGolem.setWeatherState`.
pub fn set_weather(m: &mut ServerMob, w: crate::golem::Weather) {
    m.data
        .set_varint(super::data::id::COPPER_GOLEM_WEATHER, w as i32);
}

/// `CopperGolem.spawn(WeatherState)`: `setWeatherState(state)` then
/// `playSpawnSound()` — `playSound(COPPER_GOLEM_SPAWN)`, i.e. volume and
/// pitch 1.0, from the golem's position, excluding nobody.
pub fn copper_golem_spawn(m: &mut ServerMob, w: crate::golem::Weather) {
    set_weather(m, w);
    super::living::play_sound(m, "minecraft:entity.copper_golem.spawn", 1.0, 1.0);
}

/// `CopperGolemOxidationLevels.getOxidationLevel(state)`'s sound family:
/// `UNAFFECTED` and `EXPOSED` share the plain set, `WEATHERED` and
/// `OXIDIZED` have their own.
fn copper_family(m: &ServerMob) -> &'static str {
    match weather(m) {
        crate::golem::Weather::Unaffected | crate::golem::Weather::Exposed => "copper_golem",
        crate::golem::Weather::Weathered => "copper_golem_weathered",
        crate::golem::Weather::Oxidized => "copper_golem_oxidized",
    }
}

fn copper_sound(m: &ServerMob, what: &str) -> &'static str {
    match (copper_family(m), what) {
        ("copper_golem_weathered", "hurt") => "minecraft:entity.copper_golem_weathered.hurt",
        ("copper_golem_weathered", "death") => "minecraft:entity.copper_golem_weathered.death",
        ("copper_golem_weathered", _) => "minecraft:entity.copper_golem_weathered.step",
        ("copper_golem_oxidized", "hurt") => "minecraft:entity.copper_golem_oxidized.hurt",
        ("copper_golem_oxidized", "death") => "minecraft:entity.copper_golem_oxidized.death",
        ("copper_golem_oxidized", _) => "minecraft:entity.copper_golem_oxidized.step",
        (_, "hurt") => "minecraft:entity.copper_golem.hurt",
        (_, "death") => "minecraft:entity.copper_golem.death",
        _ => "minecraft:entity.copper_golem.step",
    }
}

/// `getHurtSound` per golem kind; the copper golem's is its oxidation
/// level's.
#[must_use]
pub fn hurt_sound(m: &ServerMob) -> Option<&'static str> {
    match m.kind.data {
        super::DataLayout::IronGolem => Some("minecraft:entity.iron_golem.hurt"),
        super::DataLayout::SnowGolem => Some("minecraft:entity.snow_golem.hurt"),
        super::DataLayout::CopperGolem => Some(copper_sound(m, "hurt")),
        _ => None,
    }
}

/// `getDeathSound` per golem kind.
#[must_use]
pub fn death_sound(m: &ServerMob) -> Option<&'static str> {
    match m.kind.data {
        super::DataLayout::IronGolem => Some("minecraft:entity.iron_golem.death"),
        super::DataLayout::SnowGolem => Some("minecraft:entity.snow_golem.death"),
        super::DataLayout::CopperGolem => Some(copper_sound(m, "death")),
        _ => None,
    }
}

/// `playStepSound` per golem kind, as `(event, volume, pitch)`:
/// `IronGolem`'s and `CopperGolem`'s overrides are `playSound(e, 1.0F,
/// 1.0F)`; `SnowGolem` has none, so it is `Entity.playStepSound`'s block
/// sound — which the caller answers, hence `None`.
#[must_use]
pub fn step_sound(m: &ServerMob) -> Option<(&'static str, f32, f32)> {
    match m.kind.data {
        super::DataLayout::IronGolem => Some(("minecraft:entity.iron_golem.step", 1.0, 1.0)),
        super::DataLayout::CopperGolem => Some((copper_sound(m, "step"), 1.0, 1.0)),
        _ => None,
    }
}

/// `getAmbientSound`: `AbstractGolem`'s null, `SnowGolem`'s
/// `SNOW_GOLEM_AMBIENT`.
#[must_use]
pub fn ambient_sound(m: &ServerMob) -> Option<&'static str> {
    match m.kind.data {
        super::DataLayout::SnowGolem => Some("minecraft:entity.snow_golem.ambient"),
        _ => None,
    }
}
