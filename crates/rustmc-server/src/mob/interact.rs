//! A player right-clicking a mob: `Mob.interact` and every `mobInteract`
//! override down the class chain of the kinds this server has.
//!
//! Reference (26.2, `javap -p -c -constants` on the pinned jar):
//!
//! ```text
//!   Mob.interact(player, hand, location)                     0-92
//!      0  !isAlive()                              -> PASS
//!     11  checkAndHandleImportantInteractions     consumes -> return it
//!     40  super.interact (Entity: leads, shears)  != PASS  -> return it
//!     60  mobInteract(player, hand)               consumes -> return it
//!     89                                          -> PASS
//!
//!   Mob.checkAndHandleImportantInteractions                  0-118
//!      6  NAME_TAG -> stack.interactLivingEntity  (a plain tag: PASS)
//!     38  SpawnEggItem -> spawnOffspringFromSpawnEgg(player, this, getType(),
//!         level, position(), stack); empty -> PASS, else SUCCESS_SERVER
//!
//!   Pig.mobInteract                                          0-107
//!      6  food = isFood(stack)                    #pig_food
//!     10  !food && isSaddled() && !isVehicle() && !secondary -> ride
//!     55  r = Animal.mobInteract
//!     73  !r.consumesAction() && stack equippable in SADDLE
//!                                 -> stack.interactLivingEntity
//!
//!   Animal.mobInteract                                       0-119
//!      6  isFood(stack):
//!     33    age == 0 && canFallInLove()  -> usePlayerItem, setInLove(player),
//!                                           playEatingSound, SUCCESS_SERVER
//!     66    canAgeUp()                   -> usePlayerItem,
//!                                           ageUp(getSpeedUpSecondsWhenFeeding(-age),
//!                                           true), playEatingSound, SUCCESS
//!    113  AgeableMob.mobInteract
//!
//!   AgeableMob.mobInteract                                   0-50
//!      6  canUseGoldenDandelion(stack, isBaby(), ageLockParticleTimer, this)
//!                                        -> setAgeLocked(...), SUCCESS
//!     44  PathfinderMob/Mob.mobInteract  -> PASS
//! ```
//!
//! `SUCCESS_SERVER` is the one result whose swing the **server** sends
//! (`handleInteract` offsets 199-217): `animate` to every tracker of the
//! player and to the player. `SUCCESS` leaves the swing to the client.
//!
//! # Refused, by name
//!
//! * `Entity.interact`'s shears branch (`shearOffAllLeashConnections`); its
//!   lead branches are `crate::mob::leash`.
//! * A saddle on a pig (`Pig.mobInteract` 73-100) and riding one (10-51):
//!   no equipment and no passengers.
//! * A named name tag: the harness can hold no `custom_name` component, and
//!   `NameTagItem.interactLivingEntity` answers `PASS` without one.
//! * `usePlayerItem` in survival — see [`Answer::used`]: the caller shrinks
//!   the stack, as every other consumer on this server does.
use super::breed;
use super::host::ServerMob;
use super::{DataLayout, Mobs};
use crate::tracker::TrackedPlayer;
use rustmc_entity::host::MobHost;

/// `InteractionResult.Success.swingSource()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Swing {
    /// `SwingSource.NONE` — `CONSUME`.
    None,
    /// `SwingSource.CLIENT` — `SUCCESS`: the client swings by itself.
    Client,
    /// `SwingSource.SERVER` — `SUCCESS_SERVER`: `player.swing(hand, true)`.
    Server,
}

/// `InteractionResult`, the three shapes the paths here return.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Pass,
    Success(Swing),
}

impl Outcome {
    /// `InteractionResult.consumesAction()`: true for every `Success`.
    #[must_use]
    pub fn consumes_action(self) -> bool {
        matches!(self, Outcome::Success(_))
    }
}

/// Who is clicking, with what.
#[derive(Debug, Clone, Copy)]
pub struct Interactor<'a> {
    /// The player's entity id — `Animal.loveCause`.
    pub entity_id: i32,
    /// The stack in the hand the packet named, by item id; `None` when
    /// empty.
    pub item: Option<&'a str>,
    /// `Player.isSecondaryUseActive()`: the sneak bit the packet carried.
    pub secondary: bool,
    /// `Player.hasInfiniteMaterials()`: creative.
    pub creative: bool,
}

/// What the click did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Answer {
    pub outcome: Outcome,
    /// How many items `usePlayerItem` / `ItemStack.consume(1, player)` took
    /// from the stack in hand. The caller applies it, and only outside
    /// creative: `ItemStack.consume` skips the shrink for a holder with
    /// infinite materials, and `Player.interactOn` 64-101 restores the count
    /// for creative besides.
    pub used: i32,
    /// `ItemUtils.createFilledResult(stack, player, result, limit)` then
    /// `setItemInHand(hand, ...)`: a bucket milked, a bowl filled. The caller
    /// owns the inventory, so it applies this.
    pub fill: Option<super::cow::Fill>,
}

/// `Mob.interact(player, hand, location)` on the mob `id`, under the tick
/// lock. `None` when `id` is not a mob of this store — a dropped item, a
/// player, an entity being ticked (which the tick lock makes impossible).
#[must_use]
pub fn interact(id: i32, who: &Interactor<'_>, players: &[TrackedPlayer]) -> Option<Answer> {
    let store = Mobs::shared();
    let mut slot = store.take(id)?;
    super::set_tick_players(players);
    let answer = breed::with_players(players, || mob_interact_top(&mut slot.ai.mob, who));
    if slot.ai.mob.removed {
        // `Entity.discard()` from inside the click — a converted mooshroom:
        // `remove(DISCARDED)` unloads it from the tracker at once, so its
        // `remove_entities` goes out now, after the new entity's pairing,
        // and not on the next tick.
        store.forget(id);
        crate::entities::Entities::shared().remove(id);
        crate::tracker::Tracker::shared().remove(id, players);
        return Some(answer);
    }
    store.put_back(id, slot);
    Some(answer)
}

fn mob_interact_top(m: &mut ServerMob, who: &Interactor<'_>) -> Answer {
    let mut used = 0;
    let mut fill = None;
    let pass = |used| Answer {
        outcome: Outcome::Pass,
        used,
        fill: None,
    };
    // 0-10.
    if !m.is_alive() {
        return pass(0);
    }
    // 11-39.
    let important = check_and_handle_important_interactions(m, who, &mut used);
    if important.consumes_action() {
        return Answer {
            outcome: important,
            used,
            fill: None,
        };
    }
    // 40-59: `Entity.interact` (LivingEntity does not override it): the
    // lead branches. Anything but PASS is returned as it is.
    if let Some(r) = super::leash::entity_interact(m, who, who.creative, &mut used) {
        return Answer {
            outcome: r,
            used,
            fill: None,
        };
    }
    // 60-88.
    let r = mob_interact(m, who, &mut used, &mut fill);
    if r.consumes_action() {
        return Answer {
            outcome: r,
            used,
            fill,
        };
    }
    // `Player.interactOn` 107-203: a PASS falls to the stack's own
    // `interactLivingEntity` — for a creative player on a COPY of the stack
    // (129-137), so its shrink is the caller's to skip. A dye on a sheep is
    // the one here that answers anything but PASS.
    if let Some(r) = super::sheep::dye(m, who) {
        return Answer {
            outcome: r,
            used: used + 1,
            fill: None,
        };
    }
    pass(used)
}

/// `Mob.checkAndHandleImportantInteractions`.
fn check_and_handle_important_interactions(
    m: &mut ServerMob,
    who: &Interactor<'_>,
    used: &mut i32,
) -> Outcome {
    let Some(item) = who.item else {
        return Outcome::Pass;
    };
    // 6-37: a name tag. Without a `custom_name` component,
    // `NameTagItem.interactLivingEntity` is PASS.
    // 38-118: `getItem() instanceof SpawnEggItem` — every `*_spawn_egg`.
    let Some(egg_type) = item.strip_suffix("_spawn_egg") else {
        return Outcome::Pass;
    };
    // `spawnOffspringFromSpawnEgg` offset 0: `spawnsEntity(stack, getType())`.
    if egg_type != m.kind.name {
        return Outcome::Pass;
    }
    match breed::spawn_offspring_from_spawn_egg(m) {
        // 114: `stack.consume(1, player)`.
        Some(_) => {
            *used += 1;
            Outcome::Success(Swing::Server)
        }
        None => Outcome::Pass,
    }
}

/// The kind's `mobInteract`.
fn mob_interact(
    m: &mut ServerMob,
    who: &Interactor<'_>,
    used: &mut i32,
    fill: &mut Option<super::cow::Fill>,
) -> Outcome {
    match m.kind.data {
        // Pig.mobInteract: the ride arm (10-54) needs a saddle this server
        // cannot put on, so `isSaddled()` is false and it never fires.
        DataLayout::Pig | DataLayout::Chicken => animal_mob_interact(m, who, used),
        // AbstractCow.mobInteract: the bucket, else Animal's.
        DataLayout::Cow => {
            if let Some((r, f)) = super::cow::milk(m, who) {
                *fill = Some(f);
                return r;
            }
            animal_mob_interact(m, who, used)
        }
        // MushroomCow.mobInteract: the bowl and the shears, else
        // AbstractCow's.
        DataLayout::Mooshroom => {
            if let Some((r, f)) = super::cow::mooshroom_interact(m, who) {
                *fill = f;
                return r;
            }
            if let Some((r, f)) = super::cow::milk(m, who) {
                *fill = Some(f);
                return r;
            }
            animal_mob_interact(m, who, used)
        }
        // Sheep.mobInteract: the shears, else Animal's.
        DataLayout::Sheep => {
            if let Some(r) = super::sheep::mob_interact(m, who) {
                return r;
            }
            animal_mob_interact(m, who, used)
        }
        // Bat → AmbientCreature → Mob.mobInteract: PASS.
        DataLayout::Bat => Outcome::Pass,
        // `IronGolem.mobInteract` (the iron-ingot repair) and the armor
        // stand's `interactAt` are not ported here: `Mob.mobInteract`'s PASS.
        DataLayout::IronGolem
        | DataLayout::SnowGolem
        | DataLayout::CopperGolem
        | DataLayout::ArmorStand => Outcome::Pass,
    }
}

/// `Animal.isFood(stack)`, per kind.
fn is_food(m: &ServerMob, item: &str) -> bool {
    match m.kind.data {
        // `Pig.isFood`: `ItemTags.PIG_FOOD`.
        DataLayout::Pig => super::item_tag_contains("pig_food", item),
        DataLayout::Cow | DataLayout::Mooshroom => super::cow::is_food(item),
        DataLayout::Sheep => super::sheep::is_food(item),
        DataLayout::Chicken => super::chicken::is_food(item),
        DataLayout::Bat
        | DataLayout::IronGolem
        | DataLayout::SnowGolem
        | DataLayout::CopperGolem
        | DataLayout::ArmorStand => false,
    }
}

/// `Animal.mobInteract`.
fn animal_mob_interact(m: &mut ServerMob, who: &Interactor<'_>, used: &mut i32) -> Outcome {
    let item = who.item;
    if item.is_some_and(|i| is_food(m, i)) {
        let age = m.get_age();
        // 20-65: a `ServerPlayer` (every interactor here), age 0, not
        // already in love.
        if age == 0 && can_fall_in_love(m) {
            *used += 1;
            breed::set_in_love(m, Some(who.entity_id));
            play_eating_sound(m);
            return Outcome::Success(Swing::Server);
        }
        // 66-98.
        if can_age_up(m) {
            *used += 1;
            age_up(m, get_speed_up_seconds_when_feeding(-age), true);
            play_eating_sound(m);
            return Outcome::Success(Swing::Client);
        }
        // 99-112: the client's CONSUME; the server falls through.
    }
    ageable_mob_interact(m, who, used)
}

/// `AgeableMob.mobInteract`: the golden dandelion, else `Mob`'s PASS.
fn ageable_mob_interact(m: &mut ServerMob, who: &Interactor<'_>, used: &mut i32) -> Outcome {
    let Some(item) = who.item else {
        return Outcome::Pass;
    };
    if can_use_golden_dandelion(m, item) {
        set_age_locked(m, used);
        return Outcome::Success(Swing::Client);
    }
    Outcome::Pass
}

/// `AgeableMob.canUseGoldenDandelion(stack, isBaby(), ageLockParticleTimer,
/// mob)`, offsets 0-33: the item, a baby, the 40-tick cooldown run out, and
/// not `#cannot_be_age_locked` (zombie horse, skeleton horse, villager —
/// none of them here).
fn can_use_golden_dandelion(m: &ServerMob, item: &str) -> bool {
    item == "minecraft:golden_dandelion" && m.is_baby() && m.age_lock_particle_timer == 0
}

/// `AgeableMob.setAgeLocked(mob, this::isAgeLocked, player, stack,
/// mob -> setAgeLockedData())`, offsets 0-68, with `setAgeLockedData`
/// (0-30) inlined as the consumer it is.
///
/// ```text
///   consumer: setAgeLocked(!isAgeLocked()); setAge(getBabyStartAge());
///             ageLockParticleTimer = 40
///    8  stack.consume(1, player)
///   14  locked = isAgeLocked()          read AFTER the toggle
///   30  locked -> setPersistenceRequired()
///   37  level.playSound(null, blockPosition(),
///                       locked ? GOLDEN_DANDELION_USE : GOLDEN_DANDELION_UNUSE,
///                       PLAYERS, 1F, 1F)
/// ```
fn set_age_locked(m: &mut ServerMob, used: &mut i32) {
    let was = m.data.get_bool(super::data::id::AGE_LOCKED);
    m.data.set_bool(super::data::id::AGE_LOCKED, !was);
    m.set_age(breed::BABY_START_AGE);
    m.age_lock_particle_timer = AGE_LOCK_COOLDOWN_TICKS;
    *used += 1;
    let locked = m.data.get_bool(super::data::id::AGE_LOCKED);
    if locked {
        m.persistence_required = true;
    }
    let pos = m.block_position();
    let event = if locked {
        "minecraft:item.golden_dandelion.use"
    } else {
        "minecraft:item.golden_dandelion.unuse"
    };
    super::play_sound_at(
        (
            f64::from(pos.x) + 0.5,
            f64::from(pos.y) + 0.5,
            f64::from(pos.z) + 0.5,
        ),
        event,
        crate::sounds::SOURCE_PLAYERS,
        1.0,
        1.0,
    );
}

/// `AgeableMob.AGE_LOCK_COOLDOWN_TICKS`: 40.
pub const AGE_LOCK_COOLDOWN_TICKS: i32 = 40;

/// `AgeableMob.FORCED_AGE_PARTICLE_TICKS`: 40.
pub const FORCED_AGE_PARTICLE_TICKS: i32 = 40;

/// `Animal.canFallInLove()`: `inLove <= 0`.
fn can_fall_in_love(m: &ServerMob) -> bool {
    m.in_love <= 0
}

/// `AgeableMob.canAgeUp()`: `isBaby() && !isAgeLocked()`.
fn can_age_up(m: &ServerMob) -> bool {
    m.is_baby() && !m.data.get_bool(super::data::id::AGE_LOCKED)
}

/// `AgeableMob.getSpeedUpSecondsWhenFeeding(ticksUntilAdult)`, 0-10:
/// `(int)((float)(ticks / 20) * 0.1F)` — the integer division first.
#[must_use]
pub fn get_speed_up_seconds_when_feeding(ticks_until_adult: i32) -> i32 {
    #[expect(clippy::cast_precision_loss, reason = "vanilla's own i2f")]
    let seconds = (ticks_until_adult / 20) as f32;
    #[expect(clippy::cast_possible_truncation, reason = "vanilla's own f2i")]
    let out = (seconds * 0.1_f32) as i32;
    out
}

/// `AgeableMob.ageUp(seconds, forced)`, offsets 0-75.
///
/// ```text
///    0  age = getAge(); old = age
///    8  age += seconds * 20; if (age > 0) age = 0
///   21  delta = age - old
///   27  setAge(age)
///   32  forced: forcedAge += delta; forcedAgeTimer == 0 -> 40
///   60  getAge() == 0 -> setAge(forcedAge)
/// ```
///
/// `forcedAgeTimer` only counts down on a client (`aiStep` 14-71), so on a
/// server it is 40 from the first forced feed onward.
pub fn age_up(m: &mut ServerMob, seconds: i32, forced: bool) {
    let old = m.get_age();
    let mut age = old.wrapping_add(seconds.wrapping_mul(20));
    if age > 0 {
        age = 0;
    }
    let delta = age - old;
    m.set_age(age);
    if forced {
        m.forced_age += delta;
        if m.forced_age_timer == 0 {
            m.forced_age_timer = FORCED_AGE_PARTICLE_TICKS;
        }
    }
    if m.get_age() == 0 {
        let f = m.forced_age;
        m.set_age(f);
    }
}

/// `playEatingSound()`: `Pig`'s override is `makeSound(getSoundSet()
/// .eatSound())` — `playSound(sound, getSoundVolume(), getVoicePitch())`;
/// `Animal`'s own is empty.
fn play_eating_sound(m: &mut ServerMob) {
    if !matches!(m.kind.data, DataLayout::Pig) {
        return;
    }
    let Some(event) = super::pig::eat_sound(m) else {
        return;
    };
    let volume = m.kind.sound_volume;
    let pitch = super::living::voice_pitch(m);
    super::living::play_sound(m, event, volume, pitch);
}

/// `ServerboundInteractPacket`, decoded: `STREAM_CODEC` is
/// `composite(VAR_INT entityId, InteractionHand.STREAM_CODEC hand,
/// Vec3.LP_STREAM_CODEC location, BOOL usingSecondaryAction)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InteractPacket {
    pub entity_id: i32,
    /// `InteractionHand` by its `id`: 0 main, 1 off.
    pub hand: u8,
    pub location: (f64, f64, f64),
    pub secondary: bool,
}

/// Decode an interact body. `Err` for anything vanilla's own codec would
/// throw on: a hand id outside the enum (`ByIdMap.continuous` with
/// `ZERO` would map it to `MAIN_HAND`, so an out-of-range id is *not* a
/// throw there — it is answered as the main hand, and so it is here), a
/// short body, a boolean byte that is not 0 or 1 (`readBoolean` takes any
/// non-zero byte as true, and so does this).
///
/// # Errors
/// On a truncated body.
pub fn decode_interact(body: &[u8]) -> std::io::Result<InteractPacket> {
    let mut r = body;
    let entity_id = rustmc_proto::varint::read_varint(&mut r)?;
    let hand_id = rustmc_proto::varint::read_varint(&mut r)?;
    // `ByIdMap.continuous(id, values, OutOfBoundsStrategy.ZERO)`.
    let hand = if hand_id == 1 { 1 } else { 0 };
    let location = read_lp_vec3(&mut r)?;
    let secondary = rustmc_proto::wire::read_u8(&mut r)? != 0;
    Ok(InteractPacket {
        entity_id,
        hand,
        location,
        secondary,
    })
}

/// `LpVec3.read`, offsets 0-100: a zero first byte is the zero vector;
/// otherwise two bytes and a big-endian `int` make a 48-bit word whose low
/// two bits (with a varint continuation when bit 2 is set) are the scale and
/// whose three 15-bit fields are `min(v & 32767, 32766) * 2 / 32766 - 1`.
fn read_lp_vec3(r: &mut &[u8]) -> std::io::Result<(f64, f64, f64)> {
    let b0 = i64::from(rustmc_proto::wire::read_u8(r)?);
    if b0 == 0 {
        return Ok((0.0, 0.0, 0.0));
    }
    let b1 = i64::from(rustmc_proto::wire::read_u8(r)?);
    let hi = i64::from(rustmc_proto::wire::read_i32(r)?) & 0xFFFF_FFFF;
    let word = (hi << 16) | (b1 << 8) | b0;
    let mut scale = b0 & 3;
    if b0 & 4 == 4 {
        scale |= (i64::from(rustmc_proto::varint::read_varint(r)?) & 0xFFFF_FFFF) << 2;
    }
    #[expect(clippy::cast_precision_loss, reason = "vanilla's own l2d")]
    let unpack = |shifted: i64| -> f64 {
        let q = (shifted & 32767).min(32766);
        (q as f64 * 2.0 / 32766.0 - 1.0) * scale as f64
    };
    Ok((unpack(word >> 3), unpack(word >> 18), unpack(word >> 33)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The harness's body — id, main hand, the one-byte zero location, the
    /// sneak bit — and a non-zero location, which must be consumed whole so
    /// the boolean after it is read from the right byte.
    #[test]
    fn an_interact_body_decodes_through_its_location() {
        let p = decode_interact(&[5, 0, 0, 1]).unwrap();
        assert_eq!(p.entity_id, 5);
        assert_eq!(p.hand, 0);
        assert_eq!(p.location, (0.0, 0.0, 0.0));
        assert!(p.secondary);
        let mut b = vec![0xAC, 0x02, 1];
        crate::entities::write_lp_vec3(&mut b, (0.25, 0.5, -0.25)).unwrap();
        b.push(0);
        let p = decode_interact(&b).unwrap();
        assert_eq!(p.entity_id, 300);
        assert_eq!(p.hand, 1);
        assert!((p.location.0 - 0.25).abs() < 1e-3);
        assert!((p.location.1 - 0.5).abs() < 1e-3);
        assert!((p.location.2 + 0.25).abs() < 1e-3);
        assert!(!p.secondary);
        assert!(decode_interact(&[5, 0]).is_err(), "a truncated body");
    }

    /// `getSpeedUpSecondsWhenFeeding` divides in integers before it scales:
    /// a fresh baby (24,000 ticks to go) is sped up by 120 seconds, and 19
    /// ticks to go is 0 seconds, not 0.095 rounded.
    #[test]
    fn the_feeding_speed_up_is_a_tenth_of_the_whole_seconds_left() {
        assert_eq!(get_speed_up_seconds_when_feeding(24_000), 120);
        assert_eq!(get_speed_up_seconds_when_feeding(19), 0);
        assert_eq!(get_speed_up_seconds_when_feeding(200), 1);
        assert_eq!(get_speed_up_seconds_when_feeding(399), 1);
        assert_eq!(get_speed_up_seconds_when_feeding(1_999), 9);
    }

    #[test]
    fn only_a_success_consumes_the_action() {
        assert!(!Outcome::Pass.consumes_action());
        assert!(Outcome::Success(Swing::Client).consumes_action());
        assert!(Outcome::Success(Swing::Server).consumes_action());
    }
}
