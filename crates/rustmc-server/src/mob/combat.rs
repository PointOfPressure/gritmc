//! A mob that can be hurt, knocked back, and killed.
//!
//! Reference (26.2), read from the pinned jar with `javap -p -c -constants`:
//! `LivingEntity.hurtServer` (transcribed in [`crate::hurt`], shared with the
//! player rather than copied), `LivingEntity.dealDefaultKnockback` (0..115),
//! `LivingEntity.knockback(DDDLDamageSource;FZ)` (0..184),
//! `LivingEntity.die` (0..173), `LivingEntity.handleKillingBlow` (0..5),
//! `LivingEntity.tickDeath` (0..53), `ServerLevel.broadcastDamageEvent`
//! (0..17) and `ServerEntity.sendChanges`' `hurtMarked` arm (1206..1232).
//!
//! # What is shared with the player, and why it is shared rather than copied
//!
//! `hurtServer` is a `LivingEntity` method. It is not a player method that a
//! mob resembles — `Player.hurtServer` is an **override** that adds the
//! creative-invulnerability return at offsets 11..32 and then calls `super`.
//! So this file calls [`crate::hurt::hurt_server`] over
//! [`crate::health::Health`], both cherry-picked from `player-systems`
//! (`495f637cb`, `5a5e619f3`, `e067556c9`), and adds only what a *level* does
//! with the outcome: the packets, the knockback and the death.
//!
//! A second transcription would have been the more obvious thing to write and
//! would have been wrong the first time the two drifted, in a way no test
//! here could see, because each copy would have had its own tests.
//!
//! # What a client sees, and three corrections to what it was thought to see
//!
//! ```text
//!   one hit that lands   damage_event{entity, source, cause, direct}
//!                        set_entity_motion{entity}      (hurtMarked, next
//!                                                        sendChanges)
//!                        set_entity_data{entity, health@9}  (next sendChanges)
//!                        sound entity.<kind>.hurt        at the mob
//!   the killing hit      the above, then
//!                        entity_event{entity, 3}
//!                        sound entity.<kind>.death       at the mob
//!                        add_entity  x n   the drops, then the orbs
//!   twenty ticks later   entity_event{entity, 60}
//!                        remove_entities{entity}
//! ```
//!
//! **`hurt_animation` is not in that list, and it is a player-only packet.**
//! `ClientboundHurtAnimationPacket` is referenced by exactly one gameplay
//! class in the jar — `ServerPlayer` — so no mob ever produces one. A check
//! observing `hurt_animation` on a pig would be unmatchable for a reason with
//! nothing to do with damage.
//!
//! **`entity_event 2` is not in it either.** It was the pre-1.19.4 generic
//! hurt event and `broadcastDamageEvent` replaced it; `LivingEntity` in this
//! jar broadcasts 3, 35, 46, 60 and 67 and **no 2 anywhere**. Both of those
//! were named as expected mob-hit packets in this lane's brief and both are
//! refuted by the jar.
//!
//! **The knockback reaches the client as `set_entity_motion`, not as a
//! packet of its own.** `knockback` sets `needsSync` and `hurtServer` calls
//! `markHurt`, which is `hurtMarked = true` on `Entity`; `ServerEntity
//! .sendChanges` reads that field at offset 1206, clears it, and sends
//! `ClientboundSetEntityMotionPacket`. So the knockback is observable only on
//! the tick *after* the hit, through the same channel a mob's ordinary
//! movement uses.
//!
//! # The four things this file does not do, named rather than defaulted
//!
//! * **`getKillCredit` / `awardKillScore` / the combat tracker.** `die`
//!   offsets 20..34 and 95..99. Nothing on this server reads a scoreboard or
//!   a death message.
//! * **`createWitherRose`.** `die` offset 154, gated on the killer being a
//!   `WitherBoss`, which cannot exist here.
//! * **`checkTotemDeathProtection`.** A `TOTEM_OF_UNDYING` data component,
//!   which [`crate::item::ItemStack`] does not carry.
//!
//! `setPose(Pose.DYING)` at `die` offset 170 **is** sent: the pose is
//! `Entity`'s accessor **6** with serializer `POSE` (the twenty-first
//! registered) and `Pose.DYING`'s id is **7** — its own `bipush 7` in `Pose`'s
//! static initialiser, not its ordinal by coincidence — and
//! [`super::data::MobData`] already carries the accessor with `STANDING`'s
//! zero as its initial value. It reaches the client through the ordinary
//! dirty flush, which is where vanilla's goes.
//!
//! (`crate::synched::Synched`, the **player**'s data, still has no arm that
//! could carry a pose. That is a different type for a different entity and
//! the distinction is worth keeping: a mob's synched data is
//! `mob::data::MobData` and the two share only their first fifteen ids.)

use rustmc_entity::host::MobHost;
use rustmc_worldgen::rng::RandomSource;

use super::host::ServerMob;
use crate::health::{DamageTags, Health, HurtContext};
use crate::hurt::{hurt_server, HurtOutcome, HurtRefused, HurtServerContext};

/// `LivingEntity.dealDefaultKnockback`'s literal `0.4000000059604645d` at
/// offset 91 — a `double` narrowed from `0.4f`, which is why it is written
/// out rather than as `0.4`.
pub const DEFAULT_KNOCKBACK_STRENGTH: f64 = 0.400_000_005_960_464_5;

/// `LivingEntity.knockback`'s `9.999999747378752E-6d` at offset 39 — the
/// literal is `1.0E-5f` widened, and the widened value is what the comparison
/// uses.
pub const KNOCKBACK_MIN_HORIZONTAL_SQR: f64 = 9.999_999_747_378_752e-6;

/// The scale on the random re-aim inside `knockback`'s loop, offsets 65 and
/// 89.
pub const KNOCKBACK_JITTER: f64 = 0.01;

/// `setLastHurtByPlayer`'s memory, `resolvePlayerResponsibleForDamage`'s
/// `bipush 100` at offset 19.
///
/// A hundred ticks — five seconds — and it is what `dropAllDeathLoot`'s
/// `hitByPlayer` and `dropExperience`'s guard both read. A mob killed more
/// than five seconds after a player last hit it drops its loot without the
/// `LAST_DAMAGE_PLAYER` parameter and drops **no experience at all**.
pub const LAST_HURT_BY_PLAYER_MEMORY_TICKS: i32 = 100;

/// `Level.broadcastEntityEvent(this, 3)` — `die` offset 162.
pub const ENTITY_EVENT_DEATH: u8 = 3;

/// `Level.broadcastEntityEvent(this, 60)` — `tickDeath` offset 41. The
/// client's poof particles.
pub const ENTITY_EVENT_POOF: u8 = 60;

/// `Pose.DYING`'s **id**, its own `bipush 7` in `Pose`'s static initialiser.
///
/// The `POSE` serializer's stream codec is
/// `ByteBufCodecs.idMapper(Pose.BY_ID, Pose::id)`, so the wire carries the
/// `id` field and not the ordinal. They happen to agree for every constant in
/// this enum, which is exactly why the constructor argument is read rather
/// than the declaration index counted.
pub const POSE_DYING: i32 = 7;

/// `tickDeath`'s `bipush 20` at offset 14.
pub const DEATH_TIME_BEFORE_REMOVAL: i32 = 20;

/// One damage source, as much of it as a mob's hurt path reads.
///
/// The registry **id** is here because `ClientboundDamageEventPacket` writes
/// it as `registry id + 1` through `ByteBufCodecs.holderRegistry`, and no
/// amount of correct damage arithmetic makes that field right on its own.
#[derive(Debug, Clone, PartialEq)]
pub struct MobDamageSource {
    /// `DamageType`'s registry id in the `minecraft:damage_type` datapack
    /// registry, as the packet's holder codec needs it.
    pub type_id: i32,
    /// The tag memberships `hurtServer` and `actuallyHurt` read.
    pub tags: DamageTags,
    /// `DamageSource.getEntity()` — the entity credited with the blow.
    pub causing_entity: Option<i32>,
    /// `DamageSource.getDirectEntity()` — what actually touched the target.
    pub direct_entity: Option<i32>,
    /// `DamageSource.getSourcePosition()`. `dealDefaultKnockback` offsets
    /// 55..88 read it for the knockback direction; a null one leaves the
    /// direction at `(0, 0)`, which is what sends `knockback` into its
    /// random-re-aim loop.
    pub source_position: Option<(f64, f64, f64)>,
}

/// Everything one hit on a mob produced that a level has to act on.
#[derive(Debug, Clone, PartialEq)]
pub struct MobHurtOutcome {
    /// The `hurtServer` outcome itself, unchanged.
    pub hurt: HurtOutcome,
    /// The mob's health after the hit, for the `set_entity_data` the next
    /// `sendChanges` sends.
    pub health: f32,
    /// `level.broadcastDamageEvent(this, source)` fired — one `damage_event`
    /// to every tracking player **and to the entity itself**
    /// (`sendToTrackingPlayersAndSelf`), which for a mob is the same set.
    pub damage_event: bool,
    /// `markHurt()` set `hurtMarked`, so the next `sendChanges` will emit a
    /// `set_entity_motion`.
    pub motion_dirty: bool,
    /// `getHurtSound(source)` — `entity.<kind>.hurt`, or `None` when the hit
    /// was inside the cooldown (`fullHit` false suppresses every sound) or
    /// the mob died.
    pub hurt_sound: Option<&'static str>,
    /// `die(source)` ran. Its own packets are [`DeathEvents`].
    pub death: Option<DeathEvents>,
}

/// What `LivingEntity.die` produced.
#[derive(Debug, Clone, PartialEq)]
pub struct DeathEvents {
    /// `broadcastEntityEvent(this, 3)` at offset 162.
    pub entity_event: u8,
    /// `getDeathSound()` — `entity.<kind>.death`. Played from `hurtServer`'s
    /// death arm rather than from `die`, and only on a full hit.
    pub death_sound: Option<&'static str>,
    /// `setPose(Pose.DYING)` at offset 170 was applied to accessor 6, so the
    /// next `sendDirtyEntityData` carries it. A value rather than a silence,
    /// so a test can assert the pose changed without reading the data map.
    pub pose_dying: bool,
    /// `dropAllDeathLoot` ran: `shouldDropLoot` was true, meaning the mob is
    /// not a baby and `mobDrops` is on.
    pub dropped_loot: bool,
    /// `dropAllDeathLoot`'s local 3 — `lastHurtByPlayerMemoryTime > 0`, the
    /// `hitByPlayer` flag that gates `LAST_DAMAGE_PLAYER` and the luck.
    pub hit_by_player: bool,
}

/// The state `hurtServer` reads and writes, lifted off a [`ServerMob`].
///
/// Round-tripping through a `Health` rather than threading a `&mut ServerMob`
/// into `crate::hurt` is what lets the player and the mob share one
/// transcription: `crate::hurt` knows about health and nothing about hosts.
#[must_use]
pub fn vitals(m: &ServerMob) -> Health {
    let mut h = Health::full(m.max_health(), 0.0);
    h.set_health(m.health());
    h.invulnerable_time = m.invulnerable_time;
    h.hurt_time = m.hurt_time;
    h.hurt_duration = m.hurt_duration;
    // **`lastHurt` and it is not optional.** `hurtServer`'s cooldown arm
    // compares the incoming damage against it, so a `Health` built without it
    // starts every hit at 0.0 and lands every one of them for its whole
    // damage rather than for the difference. That is invisible in a single
    // hit and wrong in every pair.
    h.last_hurt = m.last_hurt;
    h.no_action_time = m.no_action_time;
    h.dead = m.dead;
    h
}

/// The inverse of [`vitals`], applied after `hurtServer` has run.
///
/// `set_health` goes through the host's own setter so the synched `HEALTH`
/// accessor is marked dirty — which is the whole mechanism by which the
/// health reaches a client, and skipping it would move the number and send
/// nothing.
pub fn write_vitals(m: &mut ServerMob, h: &Health) {
    m.set_health(h.health());
    m.invulnerable_time = h.invulnerable_time;
    m.hurt_time = h.hurt_time;
    m.hurt_duration = h.hurt_duration;
    m.last_hurt = h.last_hurt;
    m.no_action_time = h.no_action_time;
    m.dead = h.dead;
}

/// `LivingEntity.hurtServer` for a mob, with the level's half of the work
/// reported rather than performed.
///
/// `causing_player` is `source.getEntity() instanceof Player`, which
/// `resolvePlayerResponsibleForDamage` tests at its offset 5. It is an
/// argument rather than something read off `source.causing_entity` because
/// the entity id alone does not say what kind the entity is, and looking it
/// up here would put an entity-store read inside a function whose whole point
/// is that it takes only the mob.
pub fn hurt(
    m: &mut ServerMob,
    source: &MobDamageSource,
    amount: f32,
    invulnerable: bool,
    causing_player: bool,
) -> MobHurtOutcome {
    let mut h = vitals(m);
    let ctx = HurtServerContext {
        hurt: HurtContext {
            invulnerable,
            // `getArmorValue()` — a pig wears nothing and `Mob` has no base
            // armour, so this is the attribute's own default. Read through
            // the attribute map rather than assumed zero, because the map is
            // where a future armoured mob's value will be.
            armor: armor_value(m),
            armor_toughness: toughness_value(m),
            ..HurtContext::default()
        },
        ..HurtServerContext::default()
    };
    let out = hurt_server(&mut h, amount, source.tags, ctx);
    write_vitals(m, &h);

    // `resolvePlayerResponsibleForDamage(source)` at offsets 272..282 —
    // `setLastHurtByPlayer(player, 100)`, the `bipush 100` at that method's
    // offset 19.
    //
    // **Placement is the whole of this.** It sits AFTER the cooldown branch
    // and `actuallyHurt`, so it runs for both arms that land and for **none**
    // of the four early returns — a hit refused by the invulnerability
    // cooldown does not refresh the memory. And it sits BEFORE the death arm
    // at 370..426, which is what makes `dropAllDeathLoot`'s `hitByPlayer` true
    // for a killing blow struck by a player. Setting it in the caller before
    // the hit is wrong in the first way; setting it after `die` is wrong in
    // the second.
    if out.refused.is_none() && causing_player {
        m.last_hurt_by_player_memory_time = LAST_HURT_BY_PLAYER_MEMORY_TICKS;
    }

    // `markHurt()` at offset 349 — `Entity.hurtMarked = true`, which
    // `ServerEntity.sendChanges` turns into a `set_entity_motion` at 1202,
    // outside the interval gate.
    if out.mark_hurt {
        m.hurt_marked = true;
    }
    if out.knockback {
        deal_default_knockback(m, source);
    }

    let died = out.died;
    let death = died.then(|| die(m, source));
    MobHurtOutcome {
        health: m.health(),
        damage_event: out.broadcast_damage_event,
        motion_dirty: out.mark_hurt,
        hurt_sound: (out.full_hit && !died)
            .then(|| super::hurt_sound(m))
            .flatten(),
        death,
        hurt: out,
    }
}

/// `getArmorValue()`, as the `i2f` at `getDamageAfterArmorAbsorb` sees it.
///
/// `LivingEntity.getArmorValue` is `Mth.floor(getAttributeValue(ARMOR))`.
/// **A pig's is 0 and that is a measured zero, not an assumed one**: the
/// attribute exists on every `LivingEntity` through
/// `createLivingAttributes`, so it is read rather than skipped, and an
/// armoured mob later changes nothing here.
fn armor_value(m: &ServerMob) -> f32 {
    m.attributes
        .value("minecraft:armor")
        .map_or(0.0, |v| v.floor() as f32)
}

/// `getAttributeValue(Attributes.ARMOR_TOUGHNESS)`, narrowed `d2f`.
fn toughness_value(m: &ServerMob) -> f32 {
    m.attributes
        .value("minecraft:armor_toughness")
        .unwrap_or(0.0) as f32
}

/// `LivingEntity.dealDefaultKnockback(DamageSource, float, boolean)`,
/// offsets 0..115.
///
/// The direction is `sourcePosition - selfPosition` in x and z only, and the
/// strength is the `0.4` literal — **not** the damage, which is passed
/// through to `knockback` as its unused `float` argument. A port that scaled
/// the knockback by the damage would be reading offset 99's `fload_2` as an
/// input to the push; it is not.
pub fn deal_default_knockback(m: &mut ServerMob, source: &MobDamageSource) {
    let (mut dx, mut dz) = (0.0, 0.0);
    // 55..88. The projectile arm at offsets 12..52 needs a `Projectile`,
    // which this server has none of; it is a branch with no reachable true
    // arm and is written down rather than given a code path that no input
    // could take.
    if let Some(p) = source.source_position {
        dx = p.0 - m.x();
        dz = p.2 - m.z();
    }
    knockback(m, DEFAULT_KNOCKBACK_STRENGTH, dx, dz);
}

/// `LivingEntity.knockback(double, double, double, DamageSource, float,
/// boolean)`, offsets 0..184.
///
/// # The loop is the part with the draws, and it draws four at a time
///
/// ```text
///   30: while (dx*dx + dz*dz < 9.999999747378752E-6)
///   46:     dx = (nextDouble() - nextDouble()) * 0.01
///   70:     dz = (nextDouble() - nextDouble()) * 0.01
///   95: goto 30
/// ```
///
/// So a hit whose horizontal direction is degenerate — which is every hit
/// with no `sourcePosition`, and any hit from an attacker standing exactly on
/// the target — spins here consuming **four** `nextDouble`s per iteration
/// until the jitter happens to exceed the threshold. The threshold is
/// `1.0E-5` and the jitter is a difference of two uniforms times `0.01`, so
/// one iteration nearly always suffices; but "nearly always" is not "always"
/// and the loop is transcribed as a loop.
///
/// # The y arm is a `Math.min` only when on the ground
///
/// Offsets 134..166: `onGround ? min(0.4, dy/2 + strength) : dy`. Airborne,
/// the y velocity is untouched. This is why a mob hit in mid-air is not
/// lifted, and it is one `ifeq` rather than a clamp applied twice.
pub fn knockback(m: &mut ServerMob, strength: f64, dx: f64, dz: f64) {
    let resistance = m
        .attributes
        .value("minecraft:knockback_resistance")
        .unwrap_or(0.0);
    let before = m.delta_movement();
    let on_ground = m.on_ground();
    if let Some(v) = knockback_delta(
        (before.x, before.y, before.z),
        on_ground,
        resistance,
        strength,
        dx,
        dz,
        &mut m.random,
    ) {
        // Offset 21, and it is **after** the `strength <= 0` return at 12..18
        // — so a mob at full knockback resistance asks for no sync at all,
        // where one set before the check would send a velocity packet for a
        // push that never happened. `knockback_delta`'s `None` is that
        // return, which is why this sits inside the `if` and not above it.
        m.needs_sync = true;
        m.set_delta_movement(v.0, v.1, v.2);
    }
}

/// The arithmetic of [`knockback`], over values rather than over a host.
///
/// Split out so every branch is a unit test rather than a corpus run: the
/// resistance return, the jitter loop's draw count, the grounded `Math.min`
/// and the airborne pass-through are four separate readings and a test that
/// needed a live `ServerMob` could reach none of them.
///
/// `None` is offsets 12..18 — the strength was not positive after resistance,
/// and vanilla returns without touching the motion or `needsSync`.
#[must_use]
pub fn knockback_delta(
    before: (f64, f64, f64),
    on_ground: bool,
    knockback_resistance: f64,
    strength: f64,
    dx: f64,
    dz: f64,
    rng: &mut impl RandomSource,
) -> Option<(f64, f64, f64)> {
    // 0..11 — `strength * (1.0 - getAttributeValue(KNOCKBACK_RESISTANCE))`.
    let strength = strength * (1.0 - knockback_resistance);
    // 12..18 — `dcmpg; ifgt`, so a NaN strength returns here too.
    if !matches!(
        strength.partial_cmp(&0.0),
        Some(std::cmp::Ordering::Greater)
    ) {
        return None;
    }
    let (mut dx, mut dz) = (dx, dz);
    // 30..95 — four draws per iteration, and the pair order is x then z with
    // each pair a difference of two `nextDouble`s.
    while dx.mul_add(dx, dz * dz) < KNOCKBACK_MIN_HORIZONTAL_SQR {
        dx = (rng.next_f64() - rng.next_f64()) * KNOCKBACK_JITTER;
        dz = (rng.next_f64() - rng.next_f64()) * KNOCKBACK_JITTER;
    }
    // 98..116 — `new Vec3(dx, 0, dz).normalize().scale(strength)`.
    //
    // `Vec3.normalize` answers ZERO below a length of 1.0E-4 rather than
    // dividing. The loop above leaves `dx*dx + dz*dz` at or above
    // 9.999999747378752E-6, so the length is at or above ~3.162E-3 and the
    // guard cannot fire — named rather than written, because writing it would
    // be a branch no input can take.
    let len = dx.mul_add(dx, dz * dz).sqrt();
    let (nx, nz) = (dx / len * strength, dz / len * strength);
    // 118..181. The existing motion is HALVED on x and z whatever happens,
    // and the push is SUBTRACTED — so a knockback from the north pushes
    // south, and a mob already moving keeps half its own momentum.
    let y = if on_ground {
        f64::min(0.4, before.1 / 2.0 + strength)
    } else {
        before.1
    };
    Some((before.0 / 2.0 - nx, y, before.2 / 2.0 - nz))
}

/// `LivingEntity.die(DamageSource)`, offsets 0..173.
///
/// The order matters and is the order below: `handleKillingBlow` sets `dead`
/// **before** `dropAllDeathLoot`, so a drop path that re-entered `hurtServer`
/// would find the mob already dead — which is the guard at `hurtServer`
/// offset 11.
pub fn die(m: &mut ServerMob, source: &MobDamageSource) -> DeathEvents {
    // 0..14 — already removed, or already dead, and it is a return.
    // The caller is `hurtServer`'s death arm, which cannot reach here twice
    // for one hit; the guard is transcribed because a second caller will.
    if m.removed || m.dead {
        return DeathEvents {
            entity_event: ENTITY_EVENT_DEATH,
            death_sound: None,
            pose_dying: false,
            dropped_loot: false,
            hit_by_player: false,
        };
    }
    // 92 — handleKillingBlow: `dead = true`, and nothing else.
    m.dead = true;
    // 166..170 — `setPose(Pose.DYING)`. The last thing `die` does, AFTER the
    // loot and after `broadcastEntityEvent(this, 3)`, and it marks accessor 6
    // dirty so it goes out on the next `sendDirtyEntityData`.
    m.data.set_varint(super::data::id::POSE, POSE_DYING);
    let hit_by_player = m.last_hurt_by_player_memory_time > 0;
    // 145..149 — dropAllDeathLoot, gated on `shouldDropLoot`.
    let dropped_loot = should_drop_loot(m);
    let _ = source;
    DeathEvents {
        entity_event: ENTITY_EVENT_DEATH,
        death_sound: super::death_sound(m),
        pose_dying: true,
        dropped_loot,
        hit_by_player,
    }
}

/// `LivingEntity.shouldDropLoot(ServerLevel)`, offsets 0..31:
/// `!isBaby() && level.getGameRules().get(MOB_DROPS)`.
///
/// The gamerule id in 26.2 is the snake_case `mob_drops` — `GameRules
/// .MOB_DROPS` — and reading it from memory of `doMobLoot` would find
/// nothing. `AGENT-BRIEF.md` names this class of mistake twice.
#[must_use]
pub fn should_drop_loot(m: &ServerMob) -> bool {
    !m.is_baby()
        && matches!(
            crate::gamerules::GameRules::shared().get("mob_drops"),
            Some(crate::gamerules::GameRuleValue::Bool(true))
        )
}

/// `LivingEntity.shouldDropExperience()`, offsets 0..12: `!isBaby()`.
#[must_use]
pub fn should_drop_experience(m: &ServerMob) -> bool {
    !m.is_baby()
}

/// What one `tickDeath` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeathTick {
    /// `deathTime` went up and nothing else happened.
    Counting(i32),
    /// `deathTime` reached 20: `broadcastEntityEvent(this, 60)` then
    /// `remove(RemovalReason.KILLED)`.
    Removed,
}

/// `LivingEntity.tickDeath()`, offsets 0..53.
///
/// The increment happens **first and unconditionally**, so the removal is on
/// the tick `deathTime` becomes 20 — the twentieth tick after the death, not
/// the twenty-first. The `isRemoved()` guard at offset 29 is what stops a mob
/// that something else already removed from emitting a second poof.
pub fn tick_death(m: &mut ServerMob) -> DeathTick {
    m.death_time += 1;
    if m.death_time >= DEATH_TIME_BEFORE_REMOVAL && !m.removed {
        m.removed = true;
        DeathTick::Removed
    } else {
        DeathTick::Counting(m.death_time)
    }
}

/// Why a hit did nothing, for a caller that has to decide whether to play the
/// no-damage sound. A re-export so a call site does not have to name
/// `crate::hurt` to match on it.
pub use crate::hurt::HurtRefused as MobHurtRefused;

/// The four early returns, as a predicate: did this hit move any state at
/// all?
#[must_use]
pub const fn was_refused(o: &MobHurtOutcome) -> bool {
    o.hurt.refused.is_some()
}

/// Was the hit refused specifically by the invulnerability cooldown?
///
/// Separated from [`was_refused`] because this one has already mutated the
/// mob — `noActionTime` was zeroed — and a caller that treats it as "nothing
/// happened" would be wrong about the state even though it is right about the
/// packets.
#[must_use]
pub fn was_refused_by_cooldown(o: &MobHurtOutcome) -> bool {
    matches!(o.hurt.refused, Some(HurtRefused::NotLargerThanLastHurt))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmc_worldgen::rng::legacy::LegacyRandomSource;

    /// Full knockback resistance is offsets 12..18 and it is a **return**, not
    /// a zero push: the motion is untouched, so a mob already moving keeps
    /// all of its momentum rather than half of it.
    ///
    /// That distinction is the whole reason this returns `Option` rather than
    /// a vector, and it is invisible to a test that only checks a stationary
    /// mob.
    #[test]
    fn full_resistance_returns_without_halving_the_existing_motion() {
        let mut rng = LegacyRandomSource::new(1);
        assert_eq!(
            knockback_delta((3.0, 1.0, 5.0), true, 1.0, 0.4, 1.0, 0.0, &mut rng),
            None
        );
        // Half resistance still pushes, and it pushes half as hard.
        let half = knockback_delta((0.0, 0.0, 0.0), true, 0.5, 0.4, 1.0, 0.0, &mut rng)
            .expect("half resistance still moves the mob");
        let none = knockback_delta((0.0, 0.0, 0.0), true, 0.0, 0.4, 1.0, 0.0, &mut rng)
            .expect("no resistance moves it further");
        assert!(
            half.0 > none.0,
            "a push along -x: half resistance must be less negative ({half:?} vs {none:?})"
        );
    }

    /// The direction is **subtracted**, so a source to the east pushes the mob
    /// west. Getting this sign backwards produces a knockback that pulls the
    /// target into the attacker and would still look like a knockback in any
    /// test that only asserted "the motion changed".
    #[test]
    fn the_push_is_away_from_the_source_not_towards_it() {
        let mut rng = LegacyRandomSource::new(2);
        // Source at +x relative to the mob: dx is positive.
        let v =
            knockback_delta((0.0, 0.0, 0.0), true, 0.0, 0.4, 1.0, 0.0, &mut rng).expect("pushed");
        assert!(v.0 < 0.0, "pushed away from a source at +x: {v:?}");
        let v =
            knockback_delta((0.0, 0.0, 0.0), true, 0.0, 0.4, 0.0, 1.0, &mut rng).expect("pushed");
        assert!(v.2 < 0.0, "pushed away from a source at +z: {v:?}");
    }

    /// The horizontal components are halved before the push is subtracted,
    /// and the whole push has magnitude `strength` after normalisation.
    #[test]
    fn the_existing_horizontal_motion_is_halved_and_the_push_is_unit_scaled() {
        let mut rng = LegacyRandomSource::new(3);
        // A push straight along +x of magnitude 0.4, from a mob already
        // moving at 1.0 on x.
        let v =
            knockback_delta((1.0, 0.0, 0.0), true, 0.0, 0.4, 1.0, 0.0, &mut rng).expect("pushed");
        assert!((v.0 - (0.5 - 0.4)).abs() < 1e-12, "{v:?}");
        assert!(v.2.abs() < 1e-12, "no z component: {v:?}");
        // A diagonal source normalises: the push is 0.4 total, not 0.4 per
        // axis.
        let d =
            knockback_delta((0.0, 0.0, 0.0), true, 0.0, 0.4, 1.0, 1.0, &mut rng).expect("pushed");
        let mag = d.0.hypot(d.2);
        assert!((mag - 0.4).abs() < 1e-12, "{mag} from {d:?}");
    }

    /// Offsets 134..166: the y arm is a `Math.min(0.4, dy/2 + strength)` only
    /// when `onGround()`. Airborne, `dy` passes through untouched.
    ///
    /// Both halves, because a port that always applied the min would lift a
    /// falling mob and a port that never did would leave a grounded one flat.
    #[test]
    fn the_vertical_arm_is_grounded_only_and_is_a_min_not_an_add() {
        let mut rng = LegacyRandomSource::new(4);
        let airborne =
            knockback_delta((0.0, -0.9, 0.0), false, 0.0, 0.4, 1.0, 0.0, &mut rng).expect("pushed");
        assert_eq!(airborne.1, -0.9, "airborne y is untouched");

        let grounded =
            knockback_delta((0.0, 0.0, 0.0), true, 0.0, 0.4, 1.0, 0.0, &mut rng).expect("pushed");
        assert!((grounded.1 - 0.4).abs() < 1e-12, "{grounded:?}");

        // Already rising fast: the min bites, so the answer is 0.4 and not
        // 0.5 + 0.4.
        let rising =
            knockback_delta((0.0, 1.0, 0.0), true, 0.0, 0.4, 1.0, 0.0, &mut rng).expect("pushed");
        assert!((rising.1 - 0.4).abs() < 1e-12, "the min bites: {rising:?}");

        // Falling: dy/2 + strength is below 0.4, so the sum wins.
        let falling =
            knockback_delta((0.0, -0.2, 0.0), true, 0.0, 0.4, 1.0, 0.0, &mut rng).expect("pushed");
        assert!(
            (falling.1 - (-0.1 + 0.4)).abs() < 1e-12,
            "the sum wins: {falling:?}"
        );
    }

    /// A degenerate direction costs **four** draws per iteration, not two, and
    /// the loop runs at all only when the direction is inside the threshold.
    ///
    /// This is the reading that decides every later draw from that mob's
    /// generator in the same tick, so it is asserted against a hand-stepped
    /// control rather than inspected.
    #[test]
    fn a_zero_direction_costs_four_draws_and_a_real_one_costs_none() {
        let mut with_jitter = LegacyRandomSource::new(5);
        let _ = knockback_delta((0.0, 0.0, 0.0), true, 0.0, 0.4, 0.0, 0.0, &mut with_jitter)
            .expect("pushed");
        let mut control = LegacyRandomSource::new(5);
        for _ in 0..4 {
            let _ = control.next_f64();
        }
        assert_eq!(
            with_jitter.next_i32(),
            control.next_i32(),
            "one iteration of the jitter loop is four nextDouble draws"
        );

        let mut untouched = LegacyRandomSource::new(5);
        let _ = knockback_delta((0.0, 0.0, 0.0), true, 0.0, 0.4, 1.0, 0.0, &mut untouched)
            .expect("pushed");
        let mut fresh = LegacyRandomSource::new(5);
        assert_eq!(
            untouched.next_i32(),
            fresh.next_i32(),
            "a real direction takes no draw at all"
        );
    }

    /// The threshold is on the **squared** horizontal length, so a direction
    /// of `(0.003, 0)` — length 3e-3, squared 9e-6 — is still degenerate and
    /// one of `(0.004, 0)` is not.
    ///
    /// A port comparing the unsquared length against 1.0E-5 would treat both
    /// as fine and never take a draw.
    #[test]
    fn the_threshold_is_on_the_squared_length() {
        let mut a = LegacyRandomSource::new(6);
        let _ = knockback_delta((0.0, 0.0, 0.0), true, 0.0, 0.4, 0.003, 0.0, &mut a);
        let mut b = LegacyRandomSource::new(6);
        for _ in 0..4 {
            let _ = b.next_f64();
        }
        assert_eq!(a.next_i32(), b.next_i32(), "0.003 is inside the threshold");

        let mut c = LegacyRandomSource::new(6);
        let _ = knockback_delta((0.0, 0.0, 0.0), true, 0.0, 0.4, 0.004, 0.0, &mut c);
        let mut d = LegacyRandomSource::new(6);
        assert_eq!(c.next_i32(), d.next_i32(), "0.004 is outside it");
    }

    /// `tickDeath` increments first, so the twentieth tick is the one that
    /// removes. Nineteen ticks leave the mob in the world.
    #[test]
    fn the_removal_is_on_the_twentieth_tick_not_the_twenty_first() {
        assert_eq!(DEATH_TIME_BEFORE_REMOVAL, 20);
        // Modelled over the counter alone, which is all `tickDeath` reads.
        let mut death_time = 0;
        let mut removed_at = None;
        for tick in 1..=25 {
            death_time += 1;
            if death_time >= DEATH_TIME_BEFORE_REMOVAL && removed_at.is_none() {
                removed_at = Some(tick);
            }
        }
        assert_eq!(removed_at, Some(20));
    }
}

/// The `minecraft:damage_type` registry, **as the client received it**.
///
/// `ClientboundDamageEventPacket` writes the type through
/// `ByteBufCodecs.holderRegistry`, which is a bare `registryId + 1`. A
/// registry id only means something against the registry the client was sent,
/// and this server sends a **capture** of a real vanilla server's
/// `registry_data` (`crate::configuration::CapturedRegistries`) rather than a
/// registry it built. So the numbering is the capture's, and deriving it
/// anywhere else — from a sorted list of the jar's 51 damage-type files, from
/// `DamageTypes`' field order — is the exact mistake
/// `CapturedRegistries::entry_ids`' own doc comment names for biomes: "a world
/// whose biomes are quietly wrong everywhere, which reads as a worldgen bug
/// rather than a registry one".
///
/// The capture is at `data/vanilla/registries.bin`, and the whole literal is
/// written here for `ci/gen-gameplay-tables.sh`'s grep.
pub const REGISTRIES_PATH: &str = "data/vanilla/registries.bin";

/// `minecraft:player_attack` — the type `ItemStack.getDamageSource` gives a
/// melee blow with no weapon component.
pub const PLAYER_ATTACK: &str = "minecraft:player_attack";

fn damage_type_names() -> Option<&'static Vec<String>> {
    static N: std::sync::OnceLock<Option<Vec<String>>> = std::sync::OnceLock::new();
    N.get_or_init(|| {
        crate::configuration::CapturedRegistries::load(std::path::Path::new(REGISTRIES_PATH))
            .ok()?
            .entry_ids("minecraft:damage_type")
            .ok()
            .flatten()
    })
    .as_ref()
}

/// The registry id of a damage type, or `None` when the capture is absent or
/// does not name it.
///
/// **`None` must refuse, not default to zero.** Zero is a real damage type in
/// this registry, so a fallback would send a plausible wrong one — a client
/// would show the wrong death message and nothing would look broken.
#[must_use]
pub fn damage_type_id(name: &str) -> Option<i32> {
    damage_type_id_in(damage_type_names()?, name)
}

/// The index of `name` in a damage-type registry listing.
///
/// Split from [`damage_type_id`] because that one resolves
/// [`REGISTRIES_PATH`] **against the process working directory**, which is the
/// repo root for the server and the *crate* directory for `cargo test`. A
/// unit test calling it therefore gets `None` for a capture that is present,
/// and the failure reads as a missing registry rather than as a missing
/// `cd`.
///
/// That is not hypothetical: this exact shape landed on this tree as
/// `rustmc-server: the damage type was resolved against the process CWD, and
/// its absence was silent`. The repair there was the same one — a pure
/// function a test can feed, beside a convenience that reads the ambient
/// path.
#[must_use]
pub fn damage_type_id_in(names: &[String], name: &str) -> Option<i32> {
    names
        .iter()
        .position(|n| n == name)
        .and_then(|i| i32::try_from(i).ok())
}

#[cfg(test)]
mod registry_tests {
    use super::*;

    /// The capture holds the damage-type registry, it holds
    /// `minecraft:player_attack`, and the id this server sends is the index
    /// the client was given.
    ///
    /// The second assertion is the one that matters: the registry is
    /// **key-sorted**, exactly as `crate::chat`'s chat-type test asserts for
    /// its own, so a capture that stopped being sorted is a red here rather
    /// than a wrong id in a packet nothing decodes.
    #[test]
    fn the_damage_type_registry_is_the_one_the_client_receives() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(REGISTRIES_PATH);
        let caps = crate::configuration::CapturedRegistries::load(&p)
            .unwrap_or_else(|e| panic!("{}: {e} — run ci/dump-registries.sh", p.display()));
        let ids = caps
            .entry_ids("minecraft:damage_type")
            .expect("the capture parses")
            .expect("the capture holds minecraft:damage_type");

        // `ci/damage-types-golden.sh` on `origin/player-systems` measures 51
        // damage types in this jar, from the 51 keys `DamageTypes` declares.
        // Asserted as a floor rather than an equality because this branch does
        // not carry that generator and a bound it cannot re-derive is a bound
        // it should not pin.
        assert!(ids.len() >= 51, "{} damage types in the capture", ids.len());
        assert!(ids.contains(&PLAYER_ATTACK.to_string()));

        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(
            ids, sorted,
            "the capture's damage_type registry is key-sorted; if it is not, \
             every id this server sends is wrong and nothing else notices"
        );

        // `damage_type_id_in` and not `damage_type_id`: the latter resolves
        // `REGISTRIES_PATH` against the process working directory, which is
        // this crate's directory under `cargo test` and the repo root under
        // the server. Calling it here would answer `None` for a capture this
        // test has just read successfully — see the function's own doc, and
        // the commit that landed the identical defect for the damage type on
        // this tree.
        let id = damage_type_id_in(&ids, PLAYER_ATTACK).expect("player_attack is in the capture");
        assert_eq!(
            usize::try_from(id).expect("non-negative"),
            ids.iter()
                .position(|n| n == PLAYER_ATTACK)
                .expect("present")
        );

        // A name the registry does not hold refuses. Zero IS a real type here,
        // so a defaulting implementation would send a plausible wrong one.
        assert_eq!(damage_type_id_in(&ids, "minecraft:not_a_damage_type"), None);
        assert_eq!(
            damage_type_id_in(&ids, &ids[0]),
            Some(0),
            "index 0 is a real type, which is why None must not become 0"
        );
    }
}
