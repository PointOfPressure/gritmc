//! `Pig`: goals, attributes, variants and sounds.
//!
//! Reference (26.2): `Pig.registerGoals` (offsets 0-177), `Pig.createAttributes`,
//! `Pig.defineSynchedData`, `Pig.finalizeSpawn`, `Pig.getDefaultDimensions`,
//! `Pig.<clinit>` (`BABY_DIMENSIONS`), `Pig.playStepSound`, `Pig.getAmbientSound`,
//! `Pig.setVariant` / `setSoundVariant`, `VariantUtils.selectVariantToSpawn`,
//! `PriorityProvider.select` (0-134) / `pick` (0-19), `Animal.getAmbientSoundInterval`,
//! `Pig.lambda$registerGoals$0/1`, `EntityTypes.PIG`.
//!
//! # The goal table, from `Pig.registerGoals`
//!
//! ```text
//!   0  FloatGoal(this)
//!   1  PanicGoal(this, 1.25)
//!   3  BreedGoal(this, 1.0)
//!   4  TemptGoal(this, 1.2, stack -> stack.is(CARROT_ON_A_STICK), false)
//!   4  TemptGoal(this, 1.2, stack -> stack.is(ItemTags.PIG_FOOD), false)
//!   5  FollowParentGoal(this, 1.1)
//!   6  WaterAvoidingRandomStrollGoal(this, 1.0)
//!   7  LookAtPlayerGoal(this, Player.class, 6.0F)
//!   8  RandomLookAroundGoal(this)
//! ```
//!
//! which is row `animal/pig/Pig` of
//! `crates/rustmc-entity/golden/mob_goal_tables.json`, and
//! `tests/mob_goal_tables.rs` there refuses a change to it that this
//! function does not follow.
use super::host::ServerMob;
use super::MobKind;
use rustmc_entity::ai::goals::breed_goal::BreedGoal;
use rustmc_entity::ai::goals::float_goal::FloatGoal;
use rustmc_entity::ai::goals::follow_parent_goal::FollowParentGoal;
use rustmc_entity::ai::goals::look_at_player_goal::LookAtPlayerGoal;
use rustmc_entity::ai::goals::panic_goal::PanicGoal;
use rustmc_entity::ai::goals::random_look_around_goal::RandomLookAroundGoal;
use rustmc_entity::ai::goals::random_stroll_goal::RandomStrollGoal;
use rustmc_entity::ai::goals::tempt_goal::TemptGoal;
use rustmc_entity::ai::mob_ai::MobAi;
use rustmc_entity::host::{EntityClass, MobHost};
use rustmc_entity::tick::MobCategory;
use rustmc_worldgen::rng::RandomSource;

/// `EntityTypes.PIG`: `of(Pig::new, CREATURE).sized(0.9F, 0.9F)
/// .passengerAttachments(0.86875F).clientTrackingRange(10)`; the builder's
/// defaults are `updateInterval` 3 and `allowedInPeaceful` true, and
/// `EntityDimensions.scalable` gives an eye height of `0.85F * height`.
pub static PIG: MobKind = MobKind {
    name: "minecraft:pig",
    entity_kind: crate::entities::PIG,
    width: 0.9,
    height: 0.9,
    eye_height: 0.9 * 0.85,
    // `Pig.BABY_DIMENSIONS`: `scalable(0.45F, 0.45F).withEyeHeight(0.40625F)`.
    baby_dimensions: Some((0.45, 0.45)),
    baby_eye_height: Some(0.406_25),
    category: MobCategory::Creature,
    allowed_in_peaceful: true,
    data: super::DataLayout::Pig,
    // `Animal.getAmbientSoundInterval()`: 120.
    ambient_sound_interval: 120,
    // `LivingEntity.getSoundVolume()`: 1.0F.
    sound_volume: 1.0,
    voice_pitch_scale: 1.0,
    pushable: true,
    emits_sounds: true,
    // `Animal.removeWhenFarAway(double)`: `iconst_0`.
    remove_when_far_away: false,
    enemy: false,
};

/// `Pig.registerGoals`. Returns the `PanicGoal`'s id, which
/// `PathfinderMob.isPanicking()` reads.
pub fn register_goals(ai: &mut MobAi<ServerMob>) -> rustmc_entity::ai::goal_selector::GoalId {
    let float = FloatGoal::install(ai);
    ai.goal_selector.add_goal(0, Box::new(float));
    let panic = ai.goal_selector.add_goal(1, Box::new(PanicGoal::new(1.25)));
    ai.goal_selector.add_goal(3, Box::new(BreedGoal::new(1.0)));
    // Two `TemptGoal`s at priority 4; the host's `holds_tempting_item`
    // answers the union of both predicates — see `ServerMob`.
    ai.goal_selector
        .add_goal(4, Box::new(TemptGoal::new(1.2, false)));
    ai.goal_selector
        .add_goal(4, Box::new(TemptGoal::new(1.2, false)));
    ai.goal_selector
        .add_goal(5, Box::new(FollowParentGoal::new(1.1)));
    ai.goal_selector
        .add_goal(6, Box::new(RandomStrollGoal::water_avoiding(1.0)));
    let look = LookAtPlayerGoal::new(ai, EntityClass::Player, 6.0);
    ai.goal_selector.add_goal(7, Box::new(look));
    ai.goal_selector
        .add_goal(8, Box::new(RandomLookAroundGoal::new()));
    panic
}

/// `Pig.lambda$registerGoals$0` and `$1`: `CARROT_ON_A_STICK`, `#pig_food`.
#[must_use]
pub fn is_tempting_item(item: &str) -> bool {
    item == "minecraft:carrot_on_a_stick" || super::item_tag_contains("pig_food", item)
}

/// `VariantUtils.selectVariantToSpawn(SpawnContext.create(level, pos),
/// PIG_VARIANT)`: `PriorityProvider.pick(registry.listElements(), holder ->
/// holder.value(), level.getRandom(), context)`.
///
/// `select` (offsets 0-134): unpack every `(variant, selector)` pair in
/// registry order, sort by priority descending (a stable sort, so
/// registry order breaks ties), then walk: an entry below the running
/// priority is dropped; one whose condition passes raises the running
/// priority; one whose condition fails is dropped. `pick` draws
/// `Util.getRandomSafe(list, random)` — `nextInt(size)` on the
/// survivors, **a draw even when one survives**.
///
/// Returns the chosen variant's registry id, or `None` when nothing
/// survives (impossible with an unconditional entry present).
pub fn select_variant_to_spawn(m: &mut ServerMob) -> Option<i32> {
    let table = super::mob_variants()?;
    let ids = super::pig_variant_ids();
    let pos = m.block_position();
    let biome = m.level.biome_at(pos);
    // `listElements()` is registry order — the synced id order.
    let mut entries: Vec<(usize, i32, Option<&Vec<String>>)> = Vec::new();
    for (idx, name) in ids.iter().enumerate() {
        let Some(sels) = table.pig_variant.get(name) else {
            continue;
        };
        for s in sels {
            entries.push((idx, s.priority, s.biomes.as_ref()));
        }
    }
    // `HIGHEST_PRIORITY_FIRST`: `Comparator.comparingInt(priority).reversed()`
    // through `List.sort`, which is stable.
    entries.sort_by_key(|e| std::cmp::Reverse(e.1));
    let mut running = i32::MIN;
    let mut survivors: Vec<usize> = Vec::new();
    for (idx, priority, biomes) in entries {
        if priority < running {
            continue;
        }
        let passes = biomes.is_none_or(|b| b.iter().any(|x| x == &biome));
        if passes {
            running = priority;
            survivors.push(idx);
        }
    }
    if survivors.is_empty() {
        return None;
    }
    let n = i32::try_from(survivors.len()).ok()?;
    let i = super::level_random(|r| r.next_i32_bound(n));
    let idx = survivors[usize::try_from(i).ok()?];
    i32::try_from(idx).ok()
}

/// `Pig.setVariant(Holder)`: `entityData.set(DATA_VARIANT_ID, holder)`.
pub fn set_variant(m: &mut ServerMob, id: i32) {
    m.data.set_varint(super::data::id::PIG_VARIANT, id);
}

/// `Pig.setSoundVariant(Holder)`.
pub fn set_sound_variant(m: &mut ServerMob, id: i32) {
    m.data.set_varint(super::data::id::PIG_SOUND_VARIANT, id);
}

/// The `pig_sound_variant` registry's ids in `byId` order — the draw's
/// domain and the wire's values, one list.
#[must_use]
pub fn sound_variant_ids() -> Vec<i32> {
    let n = super::pig_sound_variant_names().len();
    (0..n).filter_map(|i| i32::try_from(i).ok()).collect()
}

/// `Pig.getAmbientSound()`: the sound set's ambient sound, baby or adult.
#[must_use]
pub fn ambient_sound(m: &ServerMob) -> Option<&'static str> {
    sound_of(m, |s| {
        if m.is_baby() {
            s.baby_ambient
        } else {
            s.ambient
        }
    })
}

/// `LivingEntity.getHurtSound(DamageSource)` as `Pig` overrides it: the sound
/// set's hurt sound, baby or adult.
///
/// A `Pig` that resolved no sound variant falls back to nothing here rather
/// than to `SoundEvents.GENERIC_HURT`. That is not vanilla's fallback — a pig
/// in vanilla always has a variant — and it is deliberately a `None` rather
/// than a guess, for the reason `sound_of` is `Option` at all: a missing
/// `mob_variants.json` must be visible as silence, not as a plausible wrong
/// sound.
#[must_use]
pub fn hurt_sound(m: &ServerMob) -> Option<&'static str> {
    sound_of(m, |s| if m.is_baby() { s.baby_hurt } else { s.hurt })
}

/// `Pig.getDeathSound()`: the sound set's death sound.
#[must_use]
pub fn death_sound(m: &ServerMob) -> Option<&'static str> {
    sound_of(m, |s| if m.is_baby() { s.baby_death } else { s.death })
}

/// `Pig.playStepSound`'s event: the sound set's step sound.
#[must_use]
pub fn step_sound(m: &ServerMob) -> Option<&'static str> {
    sound_of(m, |s| if m.is_baby() { s.baby_step } else { s.step })
}

/// `Pig.playEatingSound`'s event: the sound set's eat sound, baby or adult.
#[must_use]
pub fn eat_sound(m: &ServerMob) -> Option<&'static str> {
    sound_of(m, |s| if m.is_baby() { s.baby_eat } else { s.eat })
}

fn sound_of(
    m: &ServerMob,
    pick: impl Fn(&super::PigSoundSet) -> &'static str,
) -> Option<&'static str> {
    let id = m.data.get_varint(super::data::id::PIG_SOUND_VARIANT);
    let names = super::pig_sound_variant_names();
    let name = names.get(usize::try_from(id).ok()?)?;
    let table = super::mob_variants()?;
    table.pig_sound_variant.get(name.as_str()).map(pick)
}
