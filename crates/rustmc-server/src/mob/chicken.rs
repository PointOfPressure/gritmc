//! `Chicken`: goals, variants, the slowed fall, and the egg it lays.
//!
//! Reference (26.2, `javap -p -c -constants` on the pinned jar):
//! `Chicken.<init>` (0-49), `Chicken.registerGoals` (0-149),
//! `Chicken.createAttributes` (`MAX_HEALTH` 4, `MOVEMENT_SPEED` 0.25),
//! `Chicken.aiStep` (0-268), `isFlapping` / `onFlap`,
//! `Chicken.getSoundSet` / `getAmbientSound` / `playStepSound` (`stepSound`,
//! 0.15F, 1F), `Chicken.defineSynchedData`, `Chicken.finalizeSpawn`,
//! `Chicken.getBreedOffspring` (0-60), `Chicken.isFood` (`#chicken_food`),
//! `Chicken.removeWhenFarAway` (`isChickenJockey()`), `Chicken.<clinit>`,
//! `EntityTypes.CHICKEN`, `Entity.processFlappingMovement`,
//! `Entity.applyMovementEmissionAndPlaySound` (73-81: `flyDist +=
//! movement.length() * 0.6`), `LivingEntity.dropFromGiftLootTable`,
//! `data/minecraft/loot_table/gameplay/chicken_lay.json`.
//!
//! # The goal table, from `Chicken.registerGoals`
//!
//! ```text
//!   0  FloatGoal(this)
//!   1  PanicGoal(this, 1.4)
//!   2  BreedGoal(this, 1.0)
//!   3  TemptGoal(this, 1.0, stack -> stack.is(ItemTags.CHICKEN_FOOD), false)
//!   4  FollowParentGoal(this, 1.1)
//!   5  WaterAvoidingRandomStrollGoal(this, 1.0)
//!   6  LookAtPlayerGoal(this, Player.class, 6.0F)
//!   7  RandomLookAroundGoal(this)
//! ```
//!
//! # The fall, and why no corpus check grades it
//!
//! `aiStep` 91-122 runs **after** `Animal.aiStep` (so after `travel` has moved
//! the chicken this tick): off the ground with `deltaMovement.y < 0` the
//! velocity's `y` is multiplied by `0.6`. With `travelInAir`'s `(v - 0.08) *
//! 0.98` before it, a tick maps `v` to `0.588 * v - 0.04704`, so a falling
//! chicken settles towards `-0.04704 / 0.412`, about `-0.1142` blocks a tick.
//! It is exact and deterministic, and it is graded by the `mob_in_world`
//! test, not by a corpus scenario: the fall is seen through `move_entity`
//! deltas that `ServerEntity.sendChanges` samples on its own three-tick
//! cadence, whose phase against the tick the floor disappears on is set by
//! wall-clock waits — vanilla against itself would differ by which three
//! ticks each packet summed.
//!
//! # The egg timer, and why no corpus check grades it either
//!
//! `eggTime` is `random.nextInt(6000) + 6000` on the chicken's own unseeded
//! random, at construction and again after every lay: when an egg comes is
//! a different tick on every run, vanilla included. The lay itself —
//! `dropFromGiftLootTable(CHICKEN_LAY)` (sequence `gameplay/chicken_lay`, one
//! roll, one conditioned item: no draw), the egg item at the chicken's feet,
//! `playSound(CHICKEN_EGG, 1F, (f - f) * 0.2F + 1F)` — is unit-tested.
use super::host::ServerMob;
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
use rustmc_entity::pathfinder::PathType;
use rustmc_entity::tick::MobCategory;
use rustmc_worldgen::rng::RandomSource;

/// `EntityTypes.CHICKEN`: `of(Chicken::new, CREATURE).sized(0.4F, 0.7F)
/// .eyeHeight(0.644F).passengerAttachments(new Vec3(0, 0.7, -0.1))
/// .clientTrackingRange(10)`.
pub static CHICKEN: MobKind = MobKind {
    enemy: false,
    name: "minecraft:chicken",
    entity_kind: crate::entities::CHICKEN,
    width: 0.4,
    height: 0.7,
    eye_height: 0.644,
    // `Chicken.BABY_DIMENSIONS`: `scalable(0.3F, 0.4F).withEyeHeight(0.28125F)`.
    baby_dimensions: Some((0.3, 0.4)),
    baby_eye_height: Some(0.281_25),
    category: MobCategory::Creature,
    allowed_in_peaceful: true,
    data: super::DataLayout::Chicken,
    ambient_sound_interval: 120,
    sound_volume: 1.0,
    voice_pitch_scale: 1.0,
    pushable: true,
    emits_sounds: true,
    // `Chicken.removeWhenFarAway(double)`: `isChickenJockey()` — never here.
    remove_when_far_away: false,
};

/// The chicken's own fields (`Chicken.<init>` 7-38).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChickenState {
    pub flap: f32,
    pub flap_speed: f32,
    pub o_flap_speed: f32,
    pub o_flap: f32,
    /// `flapping`, initialised to `1.0F`.
    pub flapping: f32,
    /// `nextFlap`, initialised to `1.0F`.
    pub next_flap: f32,
    pub egg_time: i32,
    /// `Entity.flyDist`, which only `isFlapping` reads.
    pub fly_dist: f32,
}

/// `Chicken.<init>` after `Animal.<init>`: `flapping = 1F`, `nextFlap =
/// 1F`, `isChickenJockey = false`, `eggTime = random.nextInt(6000) + 6000`
/// — on the mob's own random, after `LivingEntity.<init>`'s yaw draw — and
/// `setPathfindingMalus(WATER, 0F)`.
pub fn construct(m: &mut ServerMob) {
    let egg_time = m.random.next_i32_bound(6000) + 6000;
    m.chicken = Some(ChickenState {
        flap: 0.0,
        flap_speed: 0.0,
        o_flap_speed: 0.0,
        o_flap: 0.0,
        flapping: 1.0,
        next_flap: 1.0,
        egg_time,
        fly_dist: 0.0,
    });
    m.set_pathfinding_malus_raw(PathType::Water, 0.0);
}

/// `Chicken.registerGoals`. Returns the `PanicGoal`'s id.
pub fn register_goals(ai: &mut MobAi<ServerMob>) -> rustmc_entity::ai::goal_selector::GoalId {
    let float = FloatGoal::install(ai);
    ai.goal_selector.add_goal(0, Box::new(float));
    let panic = ai.goal_selector.add_goal(1, Box::new(PanicGoal::new(1.4)));
    ai.goal_selector.add_goal(2, Box::new(BreedGoal::new(1.0)));
    ai.goal_selector
        .add_goal(3, Box::new(TemptGoal::new(1.0, false)));
    ai.goal_selector
        .add_goal(4, Box::new(FollowParentGoal::new(1.1)));
    ai.goal_selector
        .add_goal(5, Box::new(RandomStrollGoal::water_avoiding(1.0)));
    let look = LookAtPlayerGoal::new(ai, EntityClass::Player, 6.0);
    ai.goal_selector.add_goal(6, Box::new(look));
    ai.goal_selector
        .add_goal(7, Box::new(RandomLookAroundGoal::new()));
    panic
}

/// `Chicken.isFood` / `lambda$registerGoals$0`: `#chicken_food`.
#[must_use]
pub fn is_food(item: &str) -> bool {
    super::item_tag_contains("chicken_food", item)
}

/// `Chicken.finalizeSpawn` 0-37, before `Animal.finalizeSpawn`.
pub fn finalize_spawn(m: &mut ServerMob) {
    variants::finalize_spawn(m, &variants::CHICKEN);
}

/// `Chicken.CHICKEN_EGG`'s event, `Chicken.aiStep` 210.
pub const EGG_SOUND: &str = "minecraft:entity.chicken.egg";

/// `Chicken.aiStep` 4-268, after the `Animal.aiStep` chain the caller ran.
pub fn ai_step_tail(m: &mut ServerMob) {
    let on_ground = m.on_ground();
    let Some(mut c) = m.chicken else {
        return;
    };
    // 4-58.
    c.o_flap = c.flap;
    c.o_flap_speed = c.flap_speed;
    c.flap_speed += (if on_ground { -1.0 } else { 4.0 }) * 0.3;
    c.flap_speed = c.flap_speed.clamp(0.0, 1.0);
    // 59-90.
    if !on_ground && c.flapping < 1.0 {
        c.flapping = 1.0;
    }
    c.flapping *= 0.9;
    // 91-122: the slowed fall.
    let v = m.body.movement;
    if !on_ground && v.y < 0.0 {
        m.body.movement = crate::collide::Vec3::new(v.x, v.y * 0.6, v.z);
    }
    // 125-137.
    c.flap += c.flapping * 2.0;
    m.chicken = Some(c);
    // 140-268: the egg, on the server, alive, adult, not a jockey.
    if !m.is_alive() || m.is_baby() {
        return;
    }
    let Some(mut c) = m.chicken else {
        return;
    };
    c.egg_time -= 1;
    m.chicken = Some(c);
    if c.egg_time > 0 {
        return;
    }
    if lay_egg(m) {
        // 209-238: `playSound(CHICKEN_EGG, 1F, (nextFloat() - nextFloat())
        // * 0.2F + 1F)` — Entity's, to everyone, NEUTRAL.
        let a = m.random.next_f32();
        let b = m.random.next_f32();
        let pitch = (a - b) * 0.2 + 1.0;
        super::living::play_sound(m, EGG_SOUND, 1.0, pitch);
        // 241-245: `gameEvent(ENTITY_PLACE)` — no packet.
    }
    // 248-265.
    let next = m.random.next_i32_bound(6000) + 6000;
    if let Some(mut c) = m.chicken {
        c.egg_time = next;
        m.chicken = Some(c);
    }
}

/// `dropFromGiftLootTable(level, CHICKEN_LAY, this::spawnAtLocation)`:
/// `gameplay/chicken_lay` is one roll of an `alternatives` whose three
/// children are conditioned on the chicken's variant component — temperate
/// an `egg`, warm a `brown_egg`, cold a `blue_egg` — so the stream
/// `minecraft:gameplay/chicken_lay` is created but not drawn from.
/// `spawnAtLocation(level, stack)` is offset `0F`: the egg item appears at
/// the chicken's feet. Returns whether anything dropped (`dropFromLootTable`
/// 43-69: a non-empty list).
fn lay_egg(m: &mut ServerMob) -> bool {
    let variant = m.data.get_varint(super::data::id::CHICKEN_VARIANT);
    let names = super::captured_registry(variants::CHICKEN.variant_registry);
    let item = match names
        .get(usize::try_from(variant).unwrap_or(usize::MAX))
        .map(String::as_str)
    {
        Some("minecraft:temperate") => "minecraft:egg",
        Some("minecraft:warm") => "minecraft:brown_egg",
        Some("minecraft:cold") => "minecraft:blue_egg",
        _ => return false,
    };
    super::drops::random_sequence("minecraft:gameplay/chicken_lay", |_| ());
    super::drops::spawn_item_at((m.x(), m.y(), m.z()), item);
    true
}

/// `Entity.applyMovementEmissionAndPlaySound` 73-81 (`flyDist +=
/// length * 0.6F`) and, off a block that is air, `processFlappingMovement`:
/// `isFlapping()` (`flyDist > nextFlap`) → `onFlap()` (`nextFlap = flyDist
/// + flapSpeed / 2F`) and the `FLAP` game event, which is no packet.
pub fn movement_emission(m: &mut ServerMob, length: f64, on_air: bool) {
    let Some(mut c) = m.chicken else {
        return;
    };
    #[expect(clippy::cast_possible_truncation, reason = "vanilla's own d2f")]
    let d = (length * 0.600_000_023_841_857_9) as f32;
    c.fly_dist += d;
    if on_air && c.fly_dist > c.next_flap {
        c.next_flap = c.fly_dist + c.flap_speed / 2.0;
    }
    m.chicken = Some(c);
}

/// `Chicken.getBreedOffspring` 31-56.
pub fn inherit_variant(parent: &mut ServerMob, baby: &mut ServerMob, partner_variant: Option<i32>) {
    variants::inherit_variant(parent, baby, &variants::CHICKEN, partner_variant);
}
