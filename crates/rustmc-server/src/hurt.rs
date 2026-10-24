//! `LivingEntity.hurtServer` — the whole of it, in vanilla's order.
//!
//! Reference (26.2), read from the pinned jar with `javap -p -c -constants`:
//! `net.minecraft.world.entity.LivingEntity.hurtServer`, offsets 0..613.
//!
//! # What main had before this file
//!
//! Nothing takes damage on this server. `crate::attack` decodes an attack and
//! sends a swing animation; there is no receiving side and no `damage_event`
//! has ever gone out, though `tools/oracle-runner` has decoded that packet in
//! full (entity, source, cause, direct, position) since before this branch.
//!
//! # The shape: four early returns, a scaling chain, then a two-armed ladder
//!
//! ```text
//!     0  isInvulnerableTo               -> false
//!    11  isDeadOrDying                  -> false
//!    20  IS_FIRE && FIRE_RESISTANCE     -> false
//!    42  isSleeping                     -> stopSleeping()
//!    53  noActionTime = 0
//!    58  damage < 0                     -> 0
//!    69  applyItemBlocking              -> blocked, damage -= blocked
//!   103  IS_FREEZING && freeze-extra    -> damage *= 5
//!   129  DAMAGES_HELMET && helmet worn  -> hurtHelmet, damage *= 0.75
//!   164  NaN or Infinite                -> Float.MAX_VALUE
//!   184  invulnerableTime > 10 && !BYPASSES_COOLDOWN
//!            damage <= lastHurt         -> false          (the fourth return)
//!            actuallyHurt(damage - lastHurt); lastHurt = damage; fullHit = false
//!   240  otherwise
//!            lastHurt = damage; invulnerableTime = 20
//!            actuallyHurt(damage); hurtDuration = hurtTime = 10
//! ```
//!
//! # The cooldown arm is a *difference*, not a skip
//!
//! This is the reading a reimplementation gets wrong, and it is why hitting a
//! player twice in ten ticks is not the same as hitting them once. Inside the
//! cooldown, a **larger** hit than the last one still lands — for the
//! difference between the two — and the smaller one is refused entirely. So
//! four points followed by six points within ten ticks deals four then two,
//! not four then six and not four then nothing.
//!
//! `invulnerableTime > 10.0f` is a **float** comparison against an `int`
//! field, `i2f` at offset 188, and the threshold is 10 rather than 0: the
//! cooldown only bites for the first ten of the twenty ticks it is set to.
//!
//! # `fullHit` gates the loud half, and it is false inside the cooldown
//!
//! The flag at local 8 (`true` by default, cleared only by the cooldown arm)
//! gates the damage-event broadcast, `markHurt`, the knockback, the death
//! sound and the hurt sound. A hit that lands *inside* the cooldown therefore
//! moves health and is otherwise silent — no `damage_event` on the wire, no
//! knockback, no red flash.
//!
//! # The two predicates that look different and are the same
//!
//! Offsets 337..348 and 426..442 are byte-for-byte the same test written
//! twice: `!wasBlocked || damage > 0.0`. The first gates `markHurt`, the
//! second is the method's **return value**. So a hit that was entirely
//! blocked down to zero returns `false`, and an unblocked hit of zero damage
//! returns `true`.
//!
//! # What is refused, and named rather than defaulted
//!
//! `applyItemBlocking` (shields — a `BlocksAttacks` component and a use-item
//! model), `hurtHelmet`, `dealDefaultKnockback`, `checkTotemDeathProtection`,
//! the four sound calls, `resolveMobResponsibleForDamage`,
//! `resolvePlayerResponsibleForDamage`, the combat tracker and two advancement
//! triggers. Each is a field of [`HurtServerContext`] or a documented `_`, so
//! `HurtServerContext` is the grep that finds every one.

use crate::health::{DamageTags, Health, HurtContext};

/// The threshold `hurtServer` compares `invulnerableTime` against, as a float
/// — `i2f` at offset 188 then `ldc 10.0f`.
pub const HURT_COOLDOWN_THRESHOLD: f32 = 10.0;
/// `invulnerableTime` after a full hit.
pub const INVULNERABLE_TIME_AFTER_HIT: i32 = 20;
/// `hurtDuration` after a full hit, copied straight into `hurtTime`.
pub const HURT_DURATION: i32 = 10;
/// The multiplier applied to freezing damage against
/// `EntityTypeTags.FREEZE_HURTS_EXTRA_TYPES`.
pub const FREEZE_EXTRA_MULTIPLIER: f32 = 5.0;
/// What is left of the damage after a helmet takes a `DAMAGES_HELMET` hit.
pub const HELMET_DAMAGE_MULTIPLIER: f32 = 0.75;

/// The parts of `hurtServer` this server has no model for.
///
/// Defaults are the absent case throughout: no Fire Resistance, awake, no
/// shield, not a freeze-vulnerable type, no helmet, no totem.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HurtServerContext {
    /// `hasEffect(MobEffects.FIRE_RESISTANCE)`.
    pub fire_resistance: bool,
    /// `isSleeping()`. When true vanilla calls `stopSleeping()`; this is
    /// reported back through [`HurtOutcome::stopped_sleeping`].
    pub sleeping: bool,
    /// `applyItemBlocking(level, source, damage)` — how much a raised shield
    /// took. `0.0` is "no shield", which is this server today.
    pub blocked_by_item: f32,
    /// `is(EntityTypeTags.FREEZE_HURTS_EXTRA_TYPES)`.
    pub freeze_hurts_extra: bool,
    /// `!getItemBySlot(EquipmentSlot.HEAD).isEmpty()`.
    pub wearing_helmet: bool,
    /// `checkTotemDeathProtection(source)` — consulted only once the entity
    /// is dead or dying.
    pub totem_death_protection: bool,
    /// Everything `actuallyHurt` needs, forwarded unchanged.
    pub hurt: HurtContext,
}

impl Default for HurtServerContext {
    fn default() -> Self {
        Self {
            fire_resistance: false,
            sleeping: false,
            blocked_by_item: 0.0,
            freeze_hurts_extra: false,
            wearing_helmet: false,
            totem_death_protection: false,
            hurt: HurtContext::default(),
        }
    }
}

/// Why `hurtServer` returned without touching health.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HurtRefused {
    /// Offset 0 — `isInvulnerableTo`.
    Invulnerable,
    /// Offset 11 — already dead or dying.
    DeadOrDying,
    /// Offset 20 — a fire damage type against Fire Resistance.
    FireResistant,
    /// Offset 206 — inside the cooldown and no larger than the last hit.
    /// This is the only one of the four that has already mutated the entity
    /// (`noActionTime` was zeroed at offset 53, and sleep was broken at 42).
    NotLargerThanLastHurt,
}

/// Everything one `hurtServer` call did that a caller has to act on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HurtOutcome {
    /// The method's own return value: `!wasBlocked || damage > 0.0`, or
    /// `false` on any of the four early returns.
    pub hurt: bool,
    /// Set when the call took one of the four early returns.
    pub refused: Option<HurtRefused>,
    /// The damage figure after blocking, freezing, the helmet and the NaN
    /// substitution — the value `lastHurt` was set to, and the value the
    /// effect loop and the advancement triggers are handed.
    pub damage: f32,
    /// `f4` — the damage as it arrived, after the negative clamp only.
    pub original_damage: f32,
    /// What `actuallyHurt` was handed. Inside the cooldown this is
    /// `damage - lastHurt`, not `damage`.
    pub dealt_to_actually_hurt: f32,
    /// What `actuallyHurt` took off health, after armour, magic and the
    /// absorption shield. `Player.actuallyHurt` charges the damage type's
    /// food exhaustion only when this is non-zero (offsets 89..101).
    pub health_lost: f32,
    /// The local-8 flag. False means the hit landed inside the cooldown, and
    /// with it go the damage-event broadcast, `markHurt`, the knockback and
    /// every sound.
    pub full_hit: bool,
    /// `applyItemBlocking` returned something positive.
    pub was_blocked: bool,
    /// `stopSleeping()` was called at offset 49.
    pub stopped_sleeping: bool,
    /// `level.broadcastDamageEvent(this, source)` — the `damage_event`
    /// packet. False either because this was not a full hit or because a
    /// `BlocksAttacks` component answered instead.
    pub broadcast_damage_event: bool,
    /// `markHurt()`.
    pub mark_hurt: bool,
    /// `dealDefaultKnockback(source, damage, wasBlocked)`.
    pub knockback: bool,
    /// `die(source)` — reached only when the entity is dead or dying after
    /// the hit and no totem saved it.
    pub died: bool,
}

impl HurtOutcome {
    fn refused(reason: HurtRefused) -> Self {
        Self {
            hurt: false,
            refused: Some(reason),
            damage: 0.0,
            original_damage: 0.0,
            dealt_to_actually_hurt: 0.0,
            health_lost: 0.0,
            full_hit: false,
            was_blocked: false,
            stopped_sleeping: false,
            broadcast_damage_event: false,
            mark_hurt: false,
            knockback: false,
            died: false,
        }
    }
}

/// `LivingEntity.hurtServer(ServerLevel, DamageSource, float)`.
///
/// The health, absorption, `lastHurt`, `invulnerableTime`, `hurtTime`,
/// `hurtDuration` and `noActionTime` writes all happen here because vanilla
/// does them here; the sounds, the broadcast, the knockback and the death are
/// reported on [`HurtOutcome`] because they belong to the level.
pub fn hurt_server(
    health: &mut Health,
    damage: f32,
    tags: DamageTags,
    ctx: HurtServerContext,
) -> HurtOutcome {
    if ctx.hurt.invulnerable {
        return HurtOutcome::refused(HurtRefused::Invulnerable);
    }
    if health.is_dead_or_dying() {
        return HurtOutcome::refused(HurtRefused::DeadOrDying);
    }
    if tags.is_fire && ctx.fire_resistance {
        return HurtOutcome::refused(HurtRefused::FireResistant);
    }

    // 42..57 — these two happen before any of the remaining returns, so a hit
    // refused by the cooldown has still woken a sleeper and reset the idle
    // timer.
    let stopped_sleeping = ctx.sleeping;
    health.no_action_time = 0;

    // 58..66 — `fcmpg; ifge` so a NaN damage does NOT become zero here; it
    // survives to the substitution at 164.
    let mut damage = if damage < 0.0 { 0.0 } else { damage };
    let original_damage = damage;

    // 69..101 — the shield.
    let blocked = ctx.blocked_by_item;
    damage -= blocked;
    let was_blocked = blocked > 0.0;

    // 103..128
    if tags.is_freezing && ctx.freeze_hurts_extra {
        damage *= FREEZE_EXTRA_MULTIPLIER;
    }

    // 129..163 — `hurtHelmet(source, damage)` runs on the pre-multiplied
    // figure, then the damage is reduced. No helmet slots on this server.
    if tags.damages_helmet && ctx.wearing_helmet {
        damage *= HELMET_DAMAGE_MULTIPLIER;
    }

    // 164..181
    if damage.is_nan() || damage.is_infinite() {
        damage = f32::MAX;
    }

    let mut full_hit = true;
    let dealt;
    let health_lost;
    #[expect(
        clippy::cast_precision_loss,
        reason = "vanilla's own i2f at hurtServer offset 188"
    )]
    let cooldown_active =
        health.invulnerable_time as f32 > HURT_COOLDOWN_THRESHOLD && !tags.bypasses_cooldown;
    if cooldown_active {
        // 206..216 — `fcmpg; ifgt`, so `damage <= lastHurt` refuses and a NaN
        // would too. NaN cannot reach here: 164 replaced it.
        if !matches!(
            damage.partial_cmp(&health.last_hurt),
            Some(std::cmp::Ordering::Greater)
        ) {
            let mut out = HurtOutcome::refused(HurtRefused::NotLargerThanLastHurt);
            out.stopped_sleeping = stopped_sleeping;
            out.damage = damage;
            out.original_damage = original_damage;
            out.was_blocked = was_blocked;
            return out;
        }
        dealt = damage - health.last_hurt;
        health_lost = health.actually_hurt(dealt, tags, ctx.hurt).dealt;
        health.last_hurt = damage;
        full_hit = false;
    } else {
        health.last_hurt = damage;
        health.invulnerable_time = INVULNERABLE_TIME_AFTER_HIT;
        dealt = damage;
        health_lost = health.actually_hurt(dealt, tags, ctx.hurt).dealt;
        health.hurt_duration = HURT_DURATION;
        health.hurt_time = health.hurt_duration;
    }

    // 272..282 — resolveMobResponsibleForDamage / resolvePlayerResponsible-
    // ForDamage. Both record who to credit; no combat tracking here.

    // The predicate written twice, at 337 and at 426.
    let impactful = !was_blocked || damage > 0.0;

    let mut broadcast_damage_event = false;
    let mut mark_hurt = false;
    let mut knockback = false;
    if full_hit {
        // 288..327 — `BlocksAttacks.onBlocked` replaces the broadcast, and
        // only when something actually blocked. With no shield model the
        // `else` is the only reachable arm.
        broadcast_damage_event = !was_blocked;
        if !tags.no_impact && impactful {
            mark_hurt = true;
        }
        if !tags.no_knockback {
            knockback = true;
        }
    }

    // 370..426 — death, or the hurt sound.
    let mut died = false;
    if health.is_dead_or_dying() {
        if !ctx.totem_death_protection {
            // getDeathSound()/playSecondaryHurtSound under `full_hit`.
            died = true;
        }
    } else if full_hit {
        // playHurtSound / playSecondaryHurtSound.
    }

    HurtOutcome {
        hurt: impactful,
        refused: None,
        damage,
        original_damage,
        dealt_to_actually_hurt: dealt,
        health_lost,
        full_hit,
        was_blocked,
        stopped_sleeping,
        broadcast_damage_event,
        mark_hurt,
        knockback,
        died,
    }
}

#[cfg(test)]
mod tests {
    use super::{hurt_server, HurtRefused, HurtServerContext};
    use crate::health::{DamageTags, Health, HurtContext};

    fn player() -> Health {
        Health::full(20.0, 0.0)
    }

    fn fire() -> DamageTags {
        DamageTags {
            is_fire: true,
            ..DamageTags::default()
        }
    }

    #[test]
    fn a_plain_hit_lands_and_arms_the_cooldown() {
        let mut h = player();
        let out = hurt_server(
            &mut h,
            6.0,
            DamageTags::default(),
            HurtServerContext::default(),
        );
        assert!(out.hurt);
        assert!(out.full_hit);
        assert!((h.health() - 14.0).abs() < f32::EPSILON);
        assert_eq!(h.invulnerable_time, 20);
        assert_eq!(h.hurt_time, 10);
        assert_eq!(h.hurt_duration, 10);
        assert!((h.last_hurt - 6.0).abs() < f32::EPSILON);
        assert!(out.broadcast_damage_event);
        assert!(out.mark_hurt);
        assert!(out.knockback);
    }

    #[test]
    fn a_smaller_hit_inside_the_cooldown_is_refused_entirely() {
        let mut h = player();
        hurt_server(
            &mut h,
            6.0,
            DamageTags::default(),
            HurtServerContext::default(),
        );
        let before = h.health();
        let out = hurt_server(
            &mut h,
            4.0,
            DamageTags::default(),
            HurtServerContext::default(),
        );
        assert_eq!(out.refused, Some(HurtRefused::NotLargerThanLastHurt));
        assert!(!out.hurt);
        assert!((h.health() - before).abs() < f32::EPSILON);
        // lastHurt is NOT updated on the refusal.
        assert!((h.last_hurt - 6.0).abs() < f32::EPSILON);
    }

    #[test]
    fn an_equal_hit_inside_the_cooldown_is_refused_too() {
        let mut h = player();
        hurt_server(
            &mut h,
            6.0,
            DamageTags::default(),
            HurtServerContext::default(),
        );
        let out = hurt_server(
            &mut h,
            6.0,
            DamageTags::default(),
            HurtServerContext::default(),
        );
        assert_eq!(out.refused, Some(HurtRefused::NotLargerThanLastHurt));
    }

    #[test]
    fn a_larger_hit_inside_the_cooldown_lands_for_the_difference_and_is_silent() {
        let mut h = player();
        hurt_server(
            &mut h,
            4.0,
            DamageTags::default(),
            HurtServerContext::default(),
        );
        assert!((h.health() - 16.0).abs() < f32::EPSILON);
        let out = hurt_server(
            &mut h,
            6.0,
            DamageTags::default(),
            HurtServerContext::default(),
        );
        assert!(out.hurt);
        assert!(!out.full_hit);
        assert!((out.dealt_to_actually_hurt - 2.0).abs() < f32::EPSILON);
        assert!((h.health() - 14.0).abs() < f32::EPSILON);
        assert!((h.last_hurt - 6.0).abs() < f32::EPSILON);
        // Everything loud is suppressed by the cleared flag.
        assert!(!out.broadcast_damage_event);
        assert!(!out.mark_hurt);
        assert!(!out.knockback);
        // ...and the cooldown is NOT re-armed.
        assert_eq!(h.invulnerable_time, 20);
    }

    #[test]
    fn the_cooldown_only_bites_above_ten_ticks() {
        let mut h = player();
        hurt_server(
            &mut h,
            6.0,
            DamageTags::default(),
            HurtServerContext::default(),
        );
        h.invulnerable_time = 10; // exactly the threshold: not > 10.0f
        let out = hurt_server(
            &mut h,
            1.0,
            DamageTags::default(),
            HurtServerContext::default(),
        );
        assert!(out.full_hit);
        assert!((h.health() - 13.0).abs() < f32::EPSILON);
        assert_eq!(h.invulnerable_time, 20);
    }

    #[test]
    fn bypasses_cooldown_takes_the_full_hit_arm_regardless() {
        let mut h = player();
        hurt_server(
            &mut h,
            6.0,
            DamageTags::default(),
            HurtServerContext::default(),
        );
        let tags = DamageTags {
            bypasses_cooldown: true,
            ..DamageTags::default()
        };
        let out = hurt_server(&mut h, 1.0, tags, HurtServerContext::default());
        assert!(out.full_hit);
        assert!((h.health() - 13.0).abs() < f32::EPSILON);
    }

    #[test]
    fn fire_resistance_refuses_a_fire_type_and_nothing_else() {
        let ctx = HurtServerContext {
            fire_resistance: true,
            ..HurtServerContext::default()
        };
        let mut h = player();
        let out = hurt_server(&mut h, 6.0, fire(), ctx);
        assert_eq!(out.refused, Some(HurtRefused::FireResistant));
        assert!((h.health() - 20.0).abs() < f32::EPSILON);

        let mut g = player();
        let out2 = hurt_server(&mut g, 6.0, DamageTags::default(), ctx);
        assert_eq!(out2.refused, None);
        assert!((g.health() - 14.0).abs() < f32::EPSILON);
    }

    #[test]
    fn a_dead_entity_refuses_before_anything_else_happens() {
        let mut h = player();
        h.set_health(0.0);
        h.no_action_time = 99;
        let out = hurt_server(
            &mut h,
            6.0,
            DamageTags::default(),
            HurtServerContext::default(),
        );
        assert_eq!(out.refused, Some(HurtRefused::DeadOrDying));
        // noActionTime is at offset 53, AFTER this return.
        assert_eq!(h.no_action_time, 99);
    }

    #[test]
    fn the_cooldown_refusal_still_wakes_a_sleeper_and_zeroes_no_action_time() {
        let mut h = player();
        hurt_server(
            &mut h,
            6.0,
            DamageTags::default(),
            HurtServerContext::default(),
        );
        h.no_action_time = 99;
        let ctx = HurtServerContext {
            sleeping: true,
            ..HurtServerContext::default()
        };
        let out = hurt_server(&mut h, 1.0, DamageTags::default(), ctx);
        assert_eq!(out.refused, Some(HurtRefused::NotLargerThanLastHurt));
        assert!(out.stopped_sleeping);
        assert_eq!(h.no_action_time, 0);
    }

    #[test]
    fn a_negative_damage_becomes_zero_and_still_counts_as_a_hit() {
        let mut h = player();
        let out = hurt_server(
            &mut h,
            -5.0,
            DamageTags::default(),
            HurtServerContext::default(),
        );
        // `!wasBlocked || damage > 0.0` — nothing blocked, so true.
        assert!(out.hurt);
        assert!((h.health() - 20.0).abs() < f32::EPSILON);
        assert!(out.mark_hurt);
    }

    #[test]
    fn an_infinite_damage_becomes_float_max_and_kills() {
        let mut h = player();
        let out = hurt_server(
            &mut h,
            f32::INFINITY,
            DamageTags::default(),
            HurtServerContext::default(),
        );
        assert!((out.damage - f32::MAX).abs() < f32::EPSILON);
        assert!(h.health().abs() < f32::EPSILON);
        assert!(out.died);
    }

    #[test]
    fn a_nan_damage_survives_the_negative_clamp_and_becomes_float_max() {
        // `damage < 0.0` is false for NaN, so it reaches offset 164.
        let mut h = player();
        let out = hurt_server(
            &mut h,
            f32::NAN,
            DamageTags::default(),
            HurtServerContext::default(),
        );
        assert!((out.damage - f32::MAX).abs() < f32::EPSILON);
    }

    #[test]
    fn freezing_is_quintupled_only_against_the_vulnerable_types() {
        let tags = DamageTags {
            is_freezing: true,
            ..DamageTags::default()
        };
        let mut a = player();
        hurt_server(&mut a, 2.0, tags, HurtServerContext::default());
        assert!((a.health() - 18.0).abs() < f32::EPSILON);

        let mut b = player();
        let ctx = HurtServerContext {
            freeze_hurts_extra: true,
            ..HurtServerContext::default()
        };
        hurt_server(&mut b, 2.0, tags, ctx);
        assert!((b.health() - 10.0).abs() < f32::EPSILON);
    }

    #[test]
    fn a_helmet_takes_a_quarter_off_a_damages_helmet_type() {
        let tags = DamageTags {
            damages_helmet: true,
            ..DamageTags::default()
        };
        let ctx = HurtServerContext {
            wearing_helmet: true,
            ..HurtServerContext::default()
        };
        let mut h = player();
        let out = hurt_server(&mut h, 4.0, tags, ctx);
        assert!((out.damage - 3.0).abs() < f32::EPSILON);
        assert!((h.health() - 17.0).abs() < f32::EPSILON);
    }

    #[test]
    fn a_fully_blocked_hit_returns_false_and_marks_nothing() {
        let ctx = HurtServerContext {
            blocked_by_item: 6.0,
            ..HurtServerContext::default()
        };
        let mut h = player();
        let out = hurt_server(&mut h, 6.0, DamageTags::default(), ctx);
        assert!(out.was_blocked);
        assert!(!out.hurt);
        assert!(!out.mark_hurt);
        // ...but it is still a full hit, so the cooldown IS armed and the
        // knockback still runs.
        assert!(out.full_hit);
        assert_eq!(h.invulnerable_time, 20);
        assert!(out.knockback);
        // A BlocksAttacks component would answer instead of the broadcast.
        assert!(!out.broadcast_damage_event);
    }

    #[test]
    fn no_impact_and_no_knockback_suppress_one_thing_each() {
        let mut h = player();
        let out = hurt_server(
            &mut h,
            4.0,
            DamageTags {
                no_impact: true,
                ..DamageTags::default()
            },
            HurtServerContext::default(),
        );
        assert!(!out.mark_hurt);
        assert!(out.knockback);

        let mut g = player();
        let out2 = hurt_server(
            &mut g,
            4.0,
            DamageTags {
                no_knockback: true,
                ..DamageTags::default()
            },
            HurtServerContext::default(),
        );
        assert!(out2.mark_hurt);
        assert!(!out2.knockback);
    }

    #[test]
    fn a_totem_stops_the_death_but_not_the_damage() {
        let mut h = player();
        let ctx = HurtServerContext {
            totem_death_protection: true,
            ..HurtServerContext::default()
        };
        let out = hurt_server(&mut h, 100.0, DamageTags::default(), ctx);
        assert!(h.health().abs() < f32::EPSILON);
        assert!(!out.died);
    }

    /// The positive control. Three misreadings of the cooldown arm, each
    /// written out and shown disagreeing with the transcription on a named
    /// input. Every test above would pass against an implementation that
    /// skipped the second hit outright, which is the commonest wrong reading.
    #[test]
    fn each_misreading_of_the_cooldown_is_separable() {
        // 1. The cooldown read as "refuse everything within 10 ticks".
        //    4 then 6 must deal 2, not 0.
        let mut real = player();
        hurt_server(
            &mut real,
            4.0,
            DamageTags::default(),
            HurtServerContext::default(),
        );
        hurt_server(
            &mut real,
            6.0,
            DamageTags::default(),
            HurtServerContext::default(),
        );
        assert!(
            (real.health() - 14.0).abs() < f32::EPSILON,
            "{}",
            real.health()
        );
        let skipped = 16.0_f32; // health after the first hit only
        assert_ne!(real.health(), skipped);

        // 2. The cooldown read as "let everything through". 4 then 6 would
        //    then deal 10.
        let all_through = 20.0_f32 - 4.0 - 6.0;
        assert_ne!(real.health(), all_through);

        // 3. The threshold read as `>= 10` rather than `> 10`. At exactly 10
        //    ticks vanilla takes the FULL-hit arm.
        let mut at_ten = player();
        hurt_server(
            &mut at_ten,
            6.0,
            DamageTags::default(),
            HurtServerContext::default(),
        );
        at_ten.invulnerable_time = 10;
        let out = hurt_server(
            &mut at_ten,
            1.0,
            DamageTags::default(),
            HurtServerContext::default(),
        );
        assert!(out.full_hit);
        // The `>=` reading would refuse this hit, leaving health at 14.
        assert_ne!(at_ten.health(), 14.0);
        assert!((at_ten.health() - 13.0).abs() < f32::EPSILON);

        // 4. `full_hit` read as "the hit landed" rather than "the cooldown
        //    was not in play": inside the cooldown a landing hit is silent.
        let mut silent = player();
        hurt_server(
            &mut silent,
            4.0,
            DamageTags::default(),
            HurtServerContext::default(),
        );
        let out2 = hurt_server(
            &mut silent,
            6.0,
            DamageTags::default(),
            HurtServerContext::default(),
        );
        assert!(out2.hurt);
        assert!(!out2.broadcast_damage_event);
    }

    #[test]
    fn the_hurt_context_is_forwarded_to_actually_hurt() {
        // Armour applies inside hurtServer because actuallyHurt is reached
        // with the same context. Without this the two modules could be wired
        // together wrongly and every test above would still pass.
        let mut h = player();
        let ctx = HurtServerContext {
            hurt: HurtContext {
                armor: 20.0,
                ..HurtContext::default()
            },
            ..HurtServerContext::default()
        };
        hurt_server(&mut h, 10.0, DamageTags::default(), ctx);
        assert!(h.health() > 10.0, "{}", h.health());
    }
}
