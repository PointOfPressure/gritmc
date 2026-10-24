//! Breeding and offspring: `Animal.spawnChildFromBreeding`,
//! `finalizeSpawnChildFromBreeding`, `Pig.getBreedOffspring`,
//! `SpawnEggItem.spawnOffspringFromSpawnEgg`, and the `ExperienceOrb` a
//! breeding drops.
//!
//! Reference (26.2, `javap -p -c -constants` on the pinned jar):
//!
//! ```text
//!   Animal.spawnChildFromBreeding(level, partner)        0-47
//!      0  baby = getBreedOffspring(level, partner)    -> null: return
//!     12  baby.setBaby(true)                            setAge(-24000)
//!     17  baby.snapTo(getX(), getY(), getZ(), 0F, 0F)
//!     35  finalizeSpawnChildFromBreeding(level, partner, baby)
//!     42  level.addFreshEntityWithPassengers(baby)
//!
//!   Animal.finalizeSpawnChildFromBreeding(level, partner, baby)   0-113
//!      0  loveCause (or the partner's) -> awardStat(ANIMALS_BRED),
//!         BRED_ANIMALS.trigger                          no stats, no
//!                                                       advancements here
//!     27  setAge(6000); partner.setAge(6000)
//!     41  resetLove(); partner.resetLove()
//!     49  level.broadcastEntityEvent(this, 18)          the hearts
//!     56  if MOB_DROPS: addFreshEntity(new ExperienceOrb(level, getX(),
//!         getY(), getZ(), getRandom().nextInt(7) + 1))
//!
//!   Pig.getBreedOffspring(level, partner)                0-60
//!      0  EntityTypes.PIG.create(level, BREEDING)       the constructor
//!                                                       and nothing else
//!     31  setVariant(random.nextBoolean() ? getVariant()
//!                                         : partner.getVariant())
//! ```
//!
//! # The order on the wire, and why the baby is not first
//!
//! The baby is *created* first — so it takes the lower entity id — but it is
//! *added* last: the hearts and the orb are both sent from inside
//! `finalizeSpawnChildFromBreeding`, which runs before
//! `addFreshEntityWithPassengers`. A client therefore sees `entity_event 18`
//! on the parent, then the orb's pairing, then the baby's.
//!
//! # What is not here, by name
//!
//! * The orb is `crate::xporb`'s, made by `spawn_with_direction` and ticked
//!   there. Its `add_entity` yaw and its pairing value are unseeded draws, so
//!   every breeding check in the corpus still turns `mob_drops` off.
//! * `ServerPlayer.awardStat(ANIMALS_BRED)` and the `BRED_ANIMALS` trigger:
//!   this server keeps no statistics and has no advancements.
//! * The orb constructor's `unstuckIfPossible` when it is made inside a
//!   block: a parent's feet are never inside one on the rigs this reaches.
use super::host::ServerMob;
use super::{DataLayout, Mobs};
use crate::entities::Entities;
use crate::tracker::{TrackedPlayer, Tracker};
use rustmc_entity::host::MobHost;
use rustmc_worldgen::rng::RandomSource;
use std::cell::RefCell;

/// `AgeableMob.getBabyStartAge()`: `sipush -24000`.
pub const BABY_START_AGE: i32 = -24_000;

/// `finalizeSpawnChildFromBreeding` offsets 27-38: `sipush 6000`.
pub const PARENT_AGE_AFTER_BREEDING: i32 = 6000;

/// `Animal.setInLove` offset 1: `sipush 600`.
pub const IN_LOVE_TICKS: i32 = 600;

/// `Animal.handleEntityEvent`'s case and `setInLove`'s broadcast:
/// `bipush 18`, the seven heart particles.
pub const ENTITY_EVENT_IN_LOVE_HEARTS: u8 = 18;

thread_local! {
    /// The players a mob's own tick can reach, for the one host call that
    /// has to put an entity in the world mid-tick
    /// (`spawn_child_from_breeding`, from inside `BreedGoal.tick`). Set by
    /// [`with_players`] around `tick_all` and the interact handler; empty
    /// everywhere else, which makes a spawn from anywhere else reach nobody
    /// rather than reach a stale list.
    static PLAYERS: RefCell<Vec<TrackedPlayer>> = const { RefCell::new(Vec::new()) };
}

/// Run `f` with `players` as the audience of anything a mob puts in the
/// world while it runs.
pub(super) fn with_players<T>(players: &[TrackedPlayer], f: impl FnOnce() -> T) -> T {
    let snapshot: Vec<TrackedPlayer> = players.to_vec();
    let previous = PLAYERS.with(|p| std::mem::replace(&mut *p.borrow_mut(), snapshot));
    let out = f();
    PLAYERS.with(|p| *p.borrow_mut() = previous);
    out
}

fn players() -> Vec<TrackedPlayer> {
    PLAYERS.with(|p| p.borrow().clone())
}

/// The audience [`with_players`] set, for a sibling module's broadcast.
pub(super) fn current_players() -> Vec<TrackedPlayer> {
    players()
}

/// `ServerChunkCache.sendToTrackingPlayers(entity, packet)` with the
/// current audience.
pub(super) fn send_to_tracking_players(id: i32, packet: i32, body: &[u8]) {
    Tracker::shared().send_to_tracking_players(id, &players(), packet, body);
}

/// `ServerLevel.broadcastEntityEvent(entity, event)` →
/// `ServerChunkCache.broadcastAndSend`: every player tracking the entity
/// (a mob is never itself a player, so the "and send" half reaches nobody).
///
/// `ClientboundEntityEventPacket.write`: a big-endian `int` id — not a
/// varint — then the byte.
pub fn broadcast_entity_event(id: i32, event: u8) {
    let mut body = Vec::with_capacity(5);
    body.extend_from_slice(&id.to_be_bytes());
    body.push(event);
    Tracker::shared().send_to_tracking_players(
        id,
        &players(),
        rustmc_proto::ids::play::cb::ENTITY_EVENT,
        &body,
    );
}

/// `Animal.setInLove(player)`: `inLove = 600`, the love cause, and the
/// hearts.
pub fn set_in_love(m: &mut ServerMob, cause: Option<i32>) {
    m.in_love = IN_LOVE_TICKS;
    m.love_cause = cause;
    broadcast_entity_event(m.id, ENTITY_EVENT_IN_LOVE_HEARTS);
}

/// `Animal.canMate(other)`: not itself, the same class, both in love.
#[must_use]
pub fn can_mate(m: &ServerMob, other: i32) -> bool {
    if other == m.id {
        return false;
    }
    if super::mob_kind_name(other) != Some(m.kind.name) {
        return false;
    }
    m.in_love > 0 && super::facts_of_entity(other).is_some_and(|f| f.in_love)
}

/// The kind's `getBreedOffspring(level, partner)`: the child, built by
/// `EntityType.create(level, BREEDING)` — the constructor, an entity id,
/// and nothing of `finalizeSpawn` — with whatever the kind copies from its
/// parents. `None` for a kind that breeds nothing.
///
/// `partner_variant` is the partner's `getVariant()`, read by the caller,
/// because the partner may be this same mob (`spawnOffspringFromSpawnEgg`
/// passes the mob as its own partner).
fn get_breed_offspring(m: &mut ServerMob, partner_variant: Option<i32>) -> Option<ServerMob> {
    if !m.kind.data.is_animal() {
        return None;
    }
    let table = super::attribute_table()?;
    let id = Entities::shared().next_id();
    let mut baby = ServerMob::new(m.level, m.kind, id, table)?;
    match m.kind.data {
        DataLayout::Pig => {
            // Pig.getBreedOffspring 31-56: one `nextBoolean` on the *parent's*
            // random, whatever the two variants are.
            let mine = m.data.get_varint(super::data::id::PIG_VARIANT);
            let pick = if m.random.next_bool() {
                mine
            } else {
                partner_variant.unwrap_or(mine)
            };
            super::pig::set_variant(&mut baby, pick);
        }
        // Cow.getBreedOffspring / Chicken.getBreedOffspring 31-56: the same.
        DataLayout::Cow => {
            super::variants::inherit_variant(m, &mut baby, &super::variants::COW, partner_variant);
        }
        DataLayout::Chicken => super::chicken::inherit_variant(m, &mut baby, partner_variant),
        // MushroomCow.getBreedOffspring 18-27: `getOffspringVariant(partner)`.
        DataLayout::Mooshroom => {
            let mine = super::cow::mooshroom_variant(m);
            let v = super::cow::mooshroom_offspring_variant(m, partner_variant.unwrap_or(mine));
            baby.data.set_varint(super::data::id::MOOSHROOM_TYPE, v);
        }
        // Sheep.getBreedOffspring 18-42: `DyeColor.getMixedColor(level,
        // getColor(), partner.getColor())`.
        DataLayout::Sheep => {
            let mine = super::sheep::color(m);
            let theirs = partner_variant
                .and_then(|v| u8::try_from(v).ok())
                .unwrap_or(mine);
            let c = super::sheep::mixed_color(mine, theirs);
            super::sheep::set_color(&mut baby, c);
        }
        // Not animals: nothing breeds them.
        DataLayout::Bat
        | DataLayout::IronGolem
        | DataLayout::SnowGolem
        | DataLayout::CopperGolem
        | DataLayout::ArmorStand => {}
    }
    Some(baby)
}

/// The value a kind's `getBreedOffspring` reads off the partner: its
/// variant, a mooshroom's type, a sheep's colour.
fn breed_key(m: &ServerMob) -> Option<i32> {
    Some(match m.kind.data {
        DataLayout::Pig => m.data.get_varint(super::data::id::PIG_VARIANT),
        DataLayout::Cow => m.data.get_varint(super::data::id::COW_VARIANT),
        DataLayout::Chicken => m.data.get_varint(super::data::id::CHICKEN_VARIANT),
        DataLayout::Mooshroom => super::cow::mooshroom_variant(m),
        DataLayout::Sheep => i32::from(super::sheep::color(m)),
        DataLayout::Bat
        | DataLayout::IronGolem
        | DataLayout::SnowGolem
        | DataLayout::CopperGolem
        | DataLayout::ArmorStand => return None,
    })
}

/// `AgeableMob.setBaby(true)` for a kind that `canBeABaby()`.
fn set_baby(m: &mut ServerMob) {
    m.set_age(BABY_START_AGE);
}

/// `Animal.spawnChildFromBreeding(level, partner)`, from inside
/// `BreedGoal.breed()` on this mob's own tick.
pub fn spawn_child_from_breeding(m: &mut ServerMob, partner: i32) {
    let partner_variant = Mobs::shared()
        .with(partner, |s| breed_key(&s.ai.mob))
        .flatten();
    let Some(mut baby) = get_breed_offspring(m, partner_variant) else {
        return;
    };
    set_baby(&mut baby);
    let (x, y, z) = (m.x(), m.y(), m.z());
    baby.snap_to(x, y, z, 0.0, 0.0);
    finalize_spawn_child_from_breeding(m, partner);
    add_fresh_entity_with_passengers(baby);
}

/// `Animal.finalizeSpawnChildFromBreeding(level, partner, baby)`.
fn finalize_spawn_child_from_breeding(m: &mut ServerMob, partner: i32) {
    // 0-24: the stat and the trigger — nothing on this server records them.
    m.set_age(PARENT_AGE_AFTER_BREEDING);
    Mobs::shared().with_mut(partner, |s| {
        s.ai.mob.set_age(PARENT_AGE_AFTER_BREEDING);
    });
    m.in_love = 0;
    Mobs::shared().with_mut(partner, |s| s.ai.mob.in_love = 0);
    broadcast_entity_event(m.id, ENTITY_EVENT_IN_LOVE_HEARTS);
    if crate::gamerules::GameRules::shared().boolean("mob_drops") {
        // `getRandom().nextInt(7) + 1` — the *breeder's* random, drawn before
        // the orb's constructor draws from its own. `Animal` constructs the
        // orb directly, so it never merges; `crate::xporb` then ticks it.
        let value = crate::xporb::sources::breeding(&mut m.random);
        if let Some(kinds) = crate::entities::shared_kind_ids() {
            let (x, y, z) = x_y_z(m);
            crate::xporb::spawn_with_direction(
                &crate::xporb::Level {
                    entities: Entities::shared(),
                    tracker: Tracker::shared(),
                    players: &players(),
                    kinds,
                },
                crate::collide::Vec3::new(x, y, z),
                crate::collide::Vec3::ZERO,
                value,
            );
        }
    }
}

fn x_y_z(m: &ServerMob) -> (f64, f64, f64) {
    (m.x(), m.y(), m.z())
}

/// `ServerLevel.addFreshEntityWithPassengers(baby)`: the entity list, the
/// tracker's pairing, the mob store — and **no** ambient sound, which is
/// `EntityType.spawn`'s and not this path's.
fn add_fresh_entity_with_passengers(baby: ServerMob) -> i32 {
    let slot = super::build_ai(baby);
    let id = slot.ai.mob.id;
    match crate::entities::shared_kind_ids() {
        Some(kinds) => super::add_fresh_entity_quiet(
            slot,
            Entities::shared(),
            Tracker::shared(),
            &players(),
            kinds,
        ),
        None => id,
    }
}

/// `SpawnEggItem.spawnOffspringFromSpawnEgg(player, mob, mob.getType(),
/// level, mob.position(), stack)`, offsets 0-126, for a spawn egg of the
/// mob's own type (offset 0's `spawnsEntity` is the caller's test).
///
/// ```text
///   13  mob instanceof AgeableMob -> baby = mob.getBreedOffspring(level, mob)
///   39  else                      -> baby = type.create(level, SPAWN_ITEM_USE)
///   61  baby.setBaby(true)
///   67  !baby.isBaby() -> Optional.empty()        a bat: no baby, PASS
///   79  baby.snapTo(pos.x, pos.y, pos.z, 0F, 0F)
///  101  applyComponentsFromItemStack(stack)       nothing on a plain egg
///  108  level.addFreshEntityWithPassengers(baby)
///  114  stack.consume(1, player)
/// ```
///
/// Returns the baby's entity id, or `None` for `Optional.empty()`. A kind
/// that is not an `AgeableMob` still costs an entity id: `type.create` runs
/// the constructor before `setBaby` finds there is no baby to make.
pub fn spawn_offspring_from_spawn_egg(m: &mut ServerMob) -> Option<i32> {
    if !m.kind.data.is_animal() {
        let _ = Entities::shared().next_id();
        return None;
    }
    let own = breed_key(m);
    let mut baby = get_breed_offspring(m, own)?;
    set_baby(&mut baby);
    let (x, y, z) = (m.x(), m.y(), m.z());
    baby.snap_to(x, y, z, 0.0, 0.0);
    Some(add_fresh_entity_with_passengers(baby))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `getSpeedUpSecondsWhenFeeding`, `ageUp` and the constants are checked
    /// in `interact`; this pins the three numbers the breeding path writes.
    #[test]
    fn the_breeding_constants_are_the_bytecodes() {
        assert_eq!(BABY_START_AGE, -24_000);
        assert_eq!(PARENT_AGE_AFTER_BREEDING, 6000);
        assert_eq!(IN_LOVE_TICKS, 600);
        assert_eq!(ENTITY_EVENT_IN_LOVE_HEARTS, 18);
    }
}
