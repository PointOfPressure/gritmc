//! `Cow` and `MushroomCow`: the two `AbstractCow`s — goals, attributes,
//! variants, sounds, and what a bucket, a bowl and shears do to them.
//!
//! Reference (26.2, `javap -p -c -constants` on the pinned jar):
//! `AbstractCow.registerGoals` (0-151), `AbstractCow.createAttributes`
//! (`MAX_HEALTH` 10, `MOVEMENT_SPEED` 0.2F), `AbstractCow.getSoundVolume`
//! (0.4F), `AbstractCow.playStepSound` (`stepSound`, 0.15F, 1F),
//! `AbstractCow.getSoundSet` (`COW_SOUNDS.get(CLASSIC)` — a mooshroom's),
//! `Cow.getSoundSet` (`getSoundVariant().value()`),
//! `AbstractCow.mobInteract` (0-62), `AbstractCow.<clinit>` /
//! `Cow.<clinit>` (`BABY_DIMENSIONS`), `Cow.defineSynchedData`,
//! `Cow.finalizeSpawn`, `Cow.getBreedOffspring`, `MushroomCow.mobInteract`
//! (0-459), `MushroomCow.shear` (0-34), `MushroomCow.readyForShearing`,
//! `MushroomCow.getWalkTargetValue`, `MushroomCow.getOffspringVariant`
//! (0-72), `MushroomCow.lambda$shear$0/1`, `Mob.convertTo` (0-95),
//! `ConversionType$1.convert` (SINGLE), `ItemUtils.createFilledResult`
//! (0-71), `EntityTypes.COW` / `MOOSHROOM`.
//!
//! # The goal table, from `AbstractCow.registerGoals` — both kinds
//!
//! ```text
//!   0  FloatGoal(this)
//!   1  PanicGoal(this, 2.0)
//!   2  BreedGoal(this, 1.0)
//!   3  TemptGoal(this, 1.25, stack -> stack.is(ItemTags.COW_FOOD), false)
//!   4  FollowParentGoal(this, 1.25)
//!   5  WaterAvoidingRandomStrollGoal(this, 1.0)
//!   6  LookAtPlayerGoal(this, Player.class, 6.0F)
//!   7  RandomLookAroundGoal(this)
//! ```
//!
//! row `animal/cow/AbstractCow` of `crates/rustmc-entity/golden/
//! mob_goal_tables.json`.
use super::host::ServerMob;
use super::interact::{Outcome, Swing};
use super::{variants, MobKind};
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

/// `EntityTypes.COW`: `of(Cow::new, CREATURE).sized(0.9F, 1.4F)
/// .eyeHeight(1.3F).passengerAttachments(1.36875F).clientTrackingRange(10)`;
/// `updateInterval` 3 by default, `trackDeltas()` does not name it.
pub static COW: MobKind = MobKind {
    enemy: false,
    name: "minecraft:cow",
    entity_kind: crate::entities::COW,
    width: 0.9,
    height: 1.4,
    eye_height: 1.3,
    // `Cow.BABY_DIMENSIONS`: `scalable(0.45F, 0.7F).withEyeHeight(0.69F)`.
    baby_dimensions: Some((0.45, 0.7)),
    baby_eye_height: Some(0.69),
    category: MobCategory::Creature,
    allowed_in_peaceful: true,
    data: super::DataLayout::Cow,
    ambient_sound_interval: 120,
    // `AbstractCow.getSoundVolume()`: 0.4F.
    sound_volume: 0.4,
    voice_pitch_scale: 1.0,
    pushable: true,
    emits_sounds: true,
    remove_when_far_away: false,
};

/// `EntityTypes.MOOSHROOM`: the cow's builder values exactly (`0.9F, 1.4F`,
/// eye `1.3F`, `1.36875F`, range 10).
pub static MOOSHROOM: MobKind = MobKind {
    enemy: false,
    name: "minecraft:mooshroom",
    entity_kind: crate::entities::MOOSHROOM,
    width: 0.9,
    height: 1.4,
    eye_height: 1.3,
    // `MushroomCow.BABY_DIMENSIONS`: `scalable(0.45F, 0.7F).withEyeHeight(0.69F)`.
    baby_dimensions: Some((0.45, 0.7)),
    baby_eye_height: Some(0.69),
    category: MobCategory::Creature,
    allowed_in_peaceful: true,
    data: super::DataLayout::Mooshroom,
    ambient_sound_interval: 120,
    sound_volume: 0.4,
    voice_pitch_scale: 1.0,
    pushable: true,
    emits_sounds: true,
    remove_when_far_away: false,
};

/// `AbstractCow.registerGoals`. Returns the `PanicGoal`'s id.
pub fn register_goals(ai: &mut MobAi<ServerMob>) -> rustmc_entity::ai::goal_selector::GoalId {
    let float = FloatGoal::install(ai);
    ai.goal_selector.add_goal(0, Box::new(float));
    let panic = ai.goal_selector.add_goal(1, Box::new(PanicGoal::new(2.0)));
    ai.goal_selector.add_goal(2, Box::new(BreedGoal::new(1.0)));
    ai.goal_selector
        .add_goal(3, Box::new(TemptGoal::new(1.25, false)));
    ai.goal_selector
        .add_goal(4, Box::new(FollowParentGoal::new(1.25)));
    ai.goal_selector
        .add_goal(5, Box::new(RandomStrollGoal::water_avoiding(1.0)));
    let look = LookAtPlayerGoal::new(ai, EntityClass::Player, 6.0);
    ai.goal_selector.add_goal(6, Box::new(look));
    ai.goal_selector
        .add_goal(7, Box::new(RandomLookAroundGoal::new()));
    panic
}

/// `AbstractCow.isFood` / `lambda$registerGoals$0`: `#cow_food`.
#[must_use]
pub fn is_food(item: &str) -> bool {
    super::item_tag_contains("cow_food", item)
}

/// `Cow.getAmbientSound` / `getStepSound` through `Cow.getSoundSet`, and
/// a mooshroom's through `AbstractCow.getSoundSet` — `COW_SOUNDS.get(
/// CLASSIC)`, the classic registry entry whatever the mob carries.
#[must_use]
pub fn ambient_sound(m: &ServerMob) -> Option<&'static str> {
    match m.kind.data {
        super::DataLayout::Mooshroom => classic_set().map(|s| s.ambient),
        _ => variants::sound_set(m, &variants::COW).map(|s| s.ambient),
    }
}

/// `AbstractCow.playStepSound`'s event.
#[must_use]
pub fn step_sound(m: &ServerMob) -> Option<&'static str> {
    match m.kind.data {
        super::DataLayout::Mooshroom => classic_set().map(|s| s.step),
        _ => variants::sound_set(m, &variants::COW).map(|s| s.step),
    }
}

fn classic_set() -> Option<super::SoundSet> {
    super::mob_variants()?
        .cow_sound_variant
        .get("minecraft:classic")
        .copied()
}

/// `AbstractCow.playStepSound`: `playSound(stepSound, 0.15F, 1.0F)` — a
/// literal volume and pitch, not `getSoundVolume`/`getVoicePitch`.
pub const STEP_VOLUME: f32 = 0.15;

/// `Cow.finalizeSpawn` 0-37, before `AbstractCow` → `Animal` →
/// `AgeableMob.finalizeSpawn`.
pub fn finalize_spawn(m: &mut ServerMob) {
    variants::finalize_spawn(m, &variants::COW);
}

/// What a click with a container did to the hand, for the caller to
/// apply: `ItemUtils.createFilledResult(stack, player, result,
/// limitCreativeStackSize)` then `player.setItemInHand(hand, ...)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fill {
    /// The item the container becomes.
    pub result: &'static str,
    /// `limitCreativeStackSize`: the three-argument overload passes `true`
    /// (a bucket); `MushroomCow`'s bowl passes `false`.
    pub limit_creative: bool,
}

/// `AbstractCow.mobInteract` 0-55: a bucket on an adult —
/// `player.playSound(COW_MILK, 1F, 1F)` (to everyone **but** the player:
/// `Player.playSound` is `level.playSound(this, ...)`), the filled result,
/// `SUCCESS`. `None` falls through to `Animal.mobInteract`.
pub fn milk(m: &ServerMob, who: &super::interact::Interactor<'_>) -> Option<(Outcome, Fill)> {
    if who.item != Some("minecraft:bucket") || m.is_baby() {
        return None;
    }
    play_sound_to_others(who.entity_id, "minecraft:entity.cow.milk");
    Some((
        Outcome::Success(Swing::Client),
        Fill {
            result: "minecraft:milk_bucket",
            limit_creative: true,
        },
    ))
}

/// `Player.playSound(sound, volume, pitch)` for a server player: `level
/// .playSound(this, getX(), getY(), getZ(), sound, getSoundSource() =
/// PLAYERS, volume, pitch)` — every player in range except the one making
/// it, which predicted it.
pub fn play_sound_to_others(player: i32, event: &str) {
    let Some(p) = crate::entities::Entities::shared().get(player) else {
        return;
    };
    super::play_sound_at_except(
        (p.x, p.y, p.z),
        event,
        crate::sounds::SOURCE_PLAYERS,
        1.0,
        1.0,
        Some(player),
    );
}

/// `MushroomCow.Variant.id`: `RED` 0, `BROWN` 1.
pub const MOOSHROOM_RED: i32 = 0;
pub const MOOSHROOM_BROWN: i32 = 1;

/// `MushroomCow.getVariant()`.
#[must_use]
pub fn mooshroom_variant(m: &ServerMob) -> i32 {
    m.data.get_varint(super::data::id::MOOSHROOM_TYPE)
}

/// `MushroomCow.mobInteract`, offsets 0-197: the bowl and the shears. The
/// brown mooshroom's flower feeding (198-452, which needs a
/// `SUSPICIOUS_STEW_INGREDIENT` stack and the stew effects it carries) is
/// not here: `getEffectsFromItemStack` reads a component this server's
/// stacks do not carry, so an ingredient always reads as `Optional.empty()`
/// and offset 230 falls through to `AbstractCow.mobInteract` — which is
/// what this returns `None` for.
///
/// `stewEffects` is therefore always `null` here, so the bowl yields plain
/// `MUSHROOM_STEW` and `MOOSHROOM_MILK`, never the suspicious pair.
pub fn mooshroom_interact(
    m: &mut ServerMob,
    who: &super::interact::Interactor<'_>,
) -> Option<(Outcome, Option<Fill>)> {
    // 6-127: a bowl on an adult.
    if who.item == Some("minecraft:bowl") && !m.is_baby() {
        // 116-121: `this.playSound(MOOSHROOM_MILK, 1F, 1F)` — Entity's,
        // to everyone, the player included, at the mooshroom.
        super::living::play_sound(m, "minecraft:entity.mooshroom.milk", 1.0, 1.0);
        return Some((
            Outcome::Success(Swing::Client),
            Some(Fill {
                result: "minecraft:mushroom_stew",
                limit_creative: false,
            }),
        ));
    }
    // 128-197: shears, `readyForShearing()` = `!isBaby()`.
    if who.item == Some("minecraft:shears") && !m.is_baby() {
        shear_mooshroom(m);
        // 184-191: `stack.hurtAndBreak(1, player, hand)` — the caller's,
        // and a no-op for a creative player.
        return Some((Outcome::Success(Swing::Client), None));
    }
    None
}

/// `MushroomCow.shear(level, PLAYERS, tool)`, 0-34:
///
/// ```text
///    0  level.playSound(null, this, MOOSHROOM_SHEAR, PLAYERS, 1F, 1F)
///   12  convertTo(COW, ConversionParams.single(this, false, false),
///                 cow -> { sendParticles(EXPLOSION, ...);
///                          dropFromShearingLootTable(SHEAR_MOOSHROOM, ...) })
/// ```
///
/// `Mob.convertTo` (0-95): `COW.create(level, CONVERSION)` — the
/// constructor and no `finalizeSpawn`, so the cow carries `TEMPERATE` and
/// `CLASSIC`; `ConversionType.SINGLE.convert` copies the position and both
/// rotations (`copyPosition`), the movement, `onGround`, the leash, and
/// `convertCommon`'s list (the baby flag and age, left-handedness, no-AI,
/// persistence, silence, no-gravity, name visibility); the after-conversion
/// callback runs (the particle — client-only, the `level_particles` packet is
/// not sent here — and the five mushrooms); THEN `addFreshEntity(cow)`, THEN
/// `discard()` on the mooshroom.
///
/// The loot table `shearing/mooshroom/<variant>` is one pool, `rolls: 5`,
/// one item entry: five mushrooms whatever the draw, so its
/// `random_sequence` is drawn from but decides nothing; each is a separate
/// `ItemEntity` at `(getX(), getY(1.0), getZ())` — the mooshroom's feet plus
/// its height — from `lambda$shear$1`.
fn shear_mooshroom(m: &mut ServerMob) {
    super::play_entity_sound(
        m,
        "minecraft:entity.mooshroom.shear",
        crate::sounds::SOURCE_PLAYERS,
    );
    let mushroom = if mooshroom_variant(m) == MOOSHROOM_BROWN {
        "minecraft:brown_mushroom"
    } else {
        "minecraft:red_mushroom"
    };
    super::convert::convert_to_cow(m, |from| {
        let at = (
            from.x(),
            from.y() + f64::from(from.dimensions().1),
            from.z(),
        );
        for _ in 0..5 {
            super::drops::spawn_item_at(at, mushroom);
        }
    });
}

/// `MushroomCow.getWalkTargetValue`: 10 on mycelium, else the light cost —
/// `Animal`'s with mycelium for grass.
#[must_use]
pub fn mooshroom_walk_target_below(below: Option<&str>) -> bool {
    below == Some("minecraft:mycelium")
}

/// `MushroomCow.getOffspringVariant(partner)`, 0-72: the same variant and
/// `random.nextInt(1024) == 0` mutates to the other; otherwise
/// `random.nextBoolean() ? mine : partner's` — on the parent's random,
/// and the `nextBoolean` is only drawn when the mutation roll was not.
pub fn mooshroom_offspring_variant(parent: &mut ServerMob, partner_variant: i32) -> i32 {
    use rustmc_worldgen::rng::RandomSource;
    let mine = mooshroom_variant(parent);
    if mine == partner_variant && parent.random.next_i32_bound(1024) == 0 {
        return if mine == MOOSHROOM_BROWN {
            MOOSHROOM_RED
        } else {
            MOOSHROOM_BROWN
        };
    }
    if parent.random.next_bool() {
        mine
    } else {
        partner_variant
    }
}
