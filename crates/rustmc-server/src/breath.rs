//! Air, drowning, and the two per-tick counters `baseTick` runs down.
//!
//! Reference (26.2), read from the pinned jar with `javap -p -c -constants`:
//! `net.minecraft.world.entity.LivingEntity.baseTick` (offsets 207..533),
//! `.decreaseAirSupply`, `.increaseAirSupply`, `.shouldTakeDrowningDamage`,
//! `.canBreatheUnderwater`, and `net.minecraft.server.level.ServerPlayer.tick`
//! (offsets 21..38).
//!
//! # What main had before this file
//!
//! No air supply, no drowning, and no per-player tick at all — `tick.rs` runs
//! a 20 Hz clock shared by every connection and `docs/SERVER.md` says nothing
//! else ticks.
//!
//! # `invulnerableTime` is decremented in TWO places and a player gets exactly
//! one of them
//!
//! This is the reading that costs a port its hurt cooldown, and neither half
//! is visible from the other:
//!
//! ```text
//!   LivingEntity.baseTick, offsets 468..491
//!     468  invulnerableTime > 0
//!     475  this instanceof ServerPlayer : ifne 491      <- SKIP
//!     483  invulnerableTime--
//!
//!   ServerPlayer.tick, offsets 21..38
//!     22   invulnerableTime > 0
//!     28   invulnerableTime--
//! ```
//!
//! So a player's counter runs down in `ServerPlayer.tick` and every other
//! living entity's in `LivingEntity.baseTick`, and the `instanceof` at 475 is
//! what stops a player getting both. A port that copies `baseTick` alone
//! leaves a player permanently invulnerable after one hit; a port that copies
//! both ticks it **twice a tick** and halves the invulnerability window from
//! ten ticks to five. `hurtTime`, one branch above it at 451..467, has no such
//! exclusion and is decremented for everybody.
//!
//! [`tick_counters`] takes `is_server_player` for exactly this reason, and
//! [`tick_server_player_counters`] is the other half.
//!
//! # The air ladder, and the two things that are not booleans
//!
//! `baseTick`'s water branch (offsets 207..396) is:
//!
//! ```text
//!   isEyeInFluid(WATER) && !state(eyePos).is(BUBBLE_COLUMN)
//!     && !canBreatheUnderwater()
//!     && !MobEffectUtil.hasWaterBreathing(this)
//!     && !(isPlayer && abilities.invulnerable)      -> losing air
//! ```
//!
//! `decreaseAirSupply` is **not** `air - 1`: it reads
//! `Attributes.OXYGEN_BONUS` and, when that is positive, keeps the air
//! unchanged with probability `1 / (bonus + 1)` — one `nextDouble()` from the
//! **entity's** random per losing tick, and the draw happens only when the
//! bonus is positive, so a wrong reading changes the draw count and not just
//! the air.
//!
//! `increaseAirSupply` is `min(air + 4, maxAirSupply)`, so recovering is four
//! times faster than losing.
//!
//! `shouldTakeDrowningDamage` is `getAirSupply() <= -20`, and the damage path
//! sets the air back to **0** (not to the maximum), broadcasts entity event
//! **67**, and deals `2.0`. Measured over a hundred ticks from empty: **five
//! hits, one every twenty ticks** — one a second, not one a tick. The reset
//! being to zero rather than to the maximum is what makes the period 20 and
//! not 320.

use crate::javamath;

/// `LivingEntity.shouldTakeDrowningDamage`'s threshold, `bipush -20`.
pub const DROWNING_AIR: i32 = -20;
/// `increaseAirSupply`'s step, `iconst_4`.
pub const AIR_RECOVERY_STEP: i32 = 4;
/// The damage a drowning tick deals, `fconst_2`.
pub const DROWN_DAMAGE: f32 = 2.0;
/// `ServerLevel.broadcastEntityEvent(this, (byte) 67)` — the drowning bubble
/// burst.
pub const DROWN_ENTITY_EVENT: u8 = 67;

/// What the water branch of `baseTick` reads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreathContext {
    /// `isEyeInFluid(FluidTags.WATER)`.
    pub eye_in_water: bool,
    /// `level.getBlockState(BlockPos.containing(getX(), getEyeY(), getZ()))
    /// .is(Blocks.BUBBLE_COLUMN)`.
    pub eye_in_bubble_column: bool,
    /// `canBreatheUnderwater()` — `is(EntityTypeTags.CAN_BREATHE_UNDER_WATER)`.
    pub can_breathe_underwater: bool,
    /// `MobEffectUtil.hasWaterBreathing(this)`.
    pub water_breathing: bool,
    /// `this instanceof Player && getAbilities().invulnerable`.
    pub invulnerable_player: bool,
    /// `MobEffectUtil.shouldEffectsRefillAirsupply(this)`.
    pub effects_refill_air: bool,
    /// `getAttribute(Attributes.OXYGEN_BONUS)`'s value, or `0.0` when the
    /// entity does not carry the attribute — which is the `ifnull` at offset
    /// 8, and is **not** the attribute's default.
    pub oxygen_bonus: f64,
    /// `getMaxAirSupply()`.
    pub max_air_supply: i32,
}

/// What one air tick did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreathOutcome {
    /// The air supply after the tick.
    pub air_supply: i32,
    /// `hurtServer(drown(), 2.0f)` was called, and the air was reset to 0 and
    /// entity event 67 broadcast.
    pub drowned: bool,
    /// `decreaseAirSupply` consumed a `nextDouble()` from the entity's random.
    /// Recorded because the draw count is a property of the shared stream and
    /// not only of the answer.
    pub drew_random: bool,
}

/// `LivingEntity.decreaseAirSupply(int)`.
///
/// `next_double` is the **entity's** `RandomSource.nextDouble()`, taken as a
/// closure for the reason `crate::weather` takes `next_int`: the stream is
/// shared and a private generator here would produce a plausible answer off a
/// stream nothing else agrees with. It is called **only** when the bonus is
/// positive.
pub fn decrease_air_supply(
    air: i32,
    oxygen_bonus: f64,
    next_double: &mut impl FnMut() -> f64,
) -> (i32, bool) {
    if oxygen_bonus > 0.0 {
        // `nextDouble() >= 1.0 / (bonus + 1.0)` keeps the air. Note the
        // direction: a LARGER bonus makes the threshold smaller and the keep
        // more likely.
        let drew = next_double();
        if drew >= 1.0 / (oxygen_bonus + 1.0) {
            return (air, true);
        }
        return (air - 1, true);
    }
    (air - 1, false)
}

/// `LivingEntity.increaseAirSupply(int)` — `min(air + 4, maxAirSupply)`.
#[must_use]
pub const fn increase_air_supply(air: i32, max_air_supply: i32) -> i32 {
    javamath::min_i(air + AIR_RECOVERY_STEP, max_air_supply)
}

/// `LivingEntity.shouldTakeDrowningDamage()` — `getAirSupply() <= -20`.
#[must_use]
pub const fn should_take_drowning_damage(air: i32) -> bool {
    air <= DROWNING_AIR
}

/// The water branch of `LivingEntity.baseTick`, offsets 207..419.
///
/// Returns the new air supply and whether the entity drowned this tick. The
/// `stopRiding` at offsets 366..393 is not modelled — nothing on this server
/// rides anything — and it does not touch the air.
pub fn tick_air(
    air: i32,
    ctx: BreathContext,
    next_double: &mut impl FnMut() -> f64,
) -> BreathOutcome {
    if !ctx.eye_in_water || ctx.eye_in_bubble_column {
        // 396..419 — out of water, recover if below the maximum.
        let air = if air < ctx.max_air_supply {
            increase_air_supply(air, ctx.max_air_supply)
        } else {
            air
        };
        return BreathOutcome {
            air_supply: air,
            drowned: false,
            drew_random: false,
        };
    }

    // 245..279 — the four things that stop an entity losing air.
    let losing = !ctx.can_breathe_underwater && !ctx.water_breathing && !ctx.invulnerable_player;

    if losing {
        let (air, drew_random) = decrease_air_supply(air, ctx.oxygen_bonus, next_double);
        if should_take_drowning_damage(air) {
            // 307..328 — the air is set to ZERO, not to the maximum.
            return BreathOutcome {
                air_supply: 0,
                drowned: true,
                drew_random,
            };
        }
        return BreathOutcome {
            air_supply: air,
            drowned: false,
            drew_random,
        };
    }

    // 335..365 — underwater but not losing: refill only when an effect says
    // so. Note this arm is the `else` of `losing`, so a water-breathing
    // entity recovers and an entity that merely failed the OXYGEN_BONUS roll
    // does not.
    let air = if air < ctx.max_air_supply && ctx.effects_refill_air {
        increase_air_supply(air, ctx.max_air_supply)
    } else {
        air
    };
    BreathOutcome {
        air_supply: air,
        drowned: false,
        drew_random: false,
    }
}

/// `LivingEntity.baseTick` offsets 451..491 — the two counters.
///
/// `is_server_player` gates the `invulnerableTime` decrement OUT, which is the
/// `instanceof ServerPlayer; ifne 491` at offset 475. See the module docs.
pub fn tick_counters(hurt_time: &mut i32, invulnerable_time: &mut i32, is_server_player: bool) {
    if *hurt_time > 0 {
        *hurt_time -= 1;
    }
    if *invulnerable_time > 0 && !is_server_player {
        *invulnerable_time -= 1;
    }
}

/// `ServerPlayer.tick` offsets 21..38 — the other half.
pub fn tick_server_player_counters(invulnerable_time: &mut i32) {
    if *invulnerable_time > 0 {
        *invulnerable_time -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        decrease_air_supply, increase_air_supply, should_take_drowning_damage, tick_air,
        tick_counters, tick_server_player_counters, BreathContext, AIR_RECOVERY_STEP, DROWNING_AIR,
    };

    fn land() -> BreathContext {
        BreathContext {
            eye_in_water: false,
            eye_in_bubble_column: false,
            can_breathe_underwater: false,
            water_breathing: false,
            invulnerable_player: false,
            effects_refill_air: false,
            oxygen_bonus: 0.0,
            max_air_supply: 300,
        }
    }

    fn water() -> BreathContext {
        BreathContext {
            eye_in_water: true,
            ..land()
        }
    }

    fn no_draws() -> impl FnMut() -> f64 {
        || panic!("decrease_air_supply must not draw with a zero oxygen bonus")
    }

    #[test]
    fn a_player_gets_exactly_one_invulnerable_time_decrement() {
        // The whole of the finding, as an arithmetic claim. Vanilla runs both
        // methods on a ServerPlayer every tick and the counter falls by ONE.
        let mut hurt = 10;
        let mut inv = 20;
        tick_counters(&mut hurt, &mut inv, true);
        tick_server_player_counters(&mut inv);
        assert_eq!(inv, 19);
        assert_eq!(hurt, 9, "hurtTime has no ServerPlayer exclusion");

        // A non-player runs only baseTick.
        let mut mob_inv = 20;
        let mut mob_hurt = 10;
        tick_counters(&mut mob_hurt, &mut mob_inv, false);
        assert_eq!(mob_inv, 19);
    }

    #[test]
    fn copying_base_tick_alone_leaves_a_player_permanently_invulnerable() {
        let mut hurt = 0;
        let mut inv = 20;
        for _ in 0..100 {
            tick_counters(&mut hurt, &mut inv, true);
        }
        assert_eq!(inv, 20, "the exclusion is what this test is about");
        // ...and with the other half, ten ticks takes it to the cooldown's
        // own threshold.
        for _ in 0..10 {
            tick_server_player_counters(&mut inv);
        }
        assert_eq!(inv, 10, "which is exactly hurt::HURT_COOLDOWN_THRESHOLD");
    }

    #[test]
    fn neither_counter_goes_below_zero() {
        let mut hurt = 0;
        let mut inv = 0;
        tick_counters(&mut hurt, &mut inv, false);
        tick_server_player_counters(&mut inv);
        assert_eq!((hurt, inv), (0, 0));
    }

    #[test]
    fn out_of_water_air_recovers_four_at_a_time() {
        let out = tick_air(100, land(), &mut no_draws());
        assert_eq!(out.air_supply, 104);
        assert!(!out.drowned);
        assert!(!out.drew_random);
    }

    #[test]
    fn recovery_is_clamped_to_the_maximum_and_does_nothing_at_it() {
        let out = tick_air(299, land(), &mut no_draws());
        assert_eq!(out.air_supply, 300);
        let full = tick_air(300, land(), &mut no_draws());
        assert_eq!(full.air_supply, 300);
        // ...and above it, which `setAirSupply` permits, it stays put rather
        // than being pulled down.
        let over = tick_air(400, land(), &mut no_draws());
        assert_eq!(over.air_supply, 400);
    }

    #[test]
    fn a_bubble_column_counts_as_out_of_water() {
        let ctx = BreathContext {
            eye_in_bubble_column: true,
            ..water()
        };
        let out = tick_air(100, ctx, &mut no_draws());
        assert_eq!(out.air_supply, 104);
    }

    #[test]
    fn underwater_air_falls_by_one_a_tick_with_no_oxygen_bonus() {
        let mut air = 10;
        for expect in (0..10).rev() {
            let out = tick_air(air, water(), &mut no_draws());
            assert!(!out.drew_random, "no draw without a positive bonus");
            air = out.air_supply;
            assert_eq!(air, expect);
        }
    }

    #[test]
    fn drowning_starts_at_minus_twenty_and_resets_the_air_to_zero() {
        assert!(!should_take_drowning_damage(DROWNING_AIR + 1));
        assert!(should_take_drowning_damage(DROWNING_AIR));
        let out = tick_air(DROWNING_AIR + 1, water(), &mut no_draws());
        assert!(out.drowned);
        assert_eq!(out.air_supply, 0, "reset to zero, not to the maximum");
    }

    #[test]
    fn a_drowning_entity_takes_damage_every_twenty_ticks_and_not_every_tick() {
        // From 0 air: twenty losing ticks reach -20, and -20 IS the
        // threshold (`<=`), so the twentieth tick is the hit and the reset to
        // 0 starts the cycle again. Measured rather than reasoned: this
        // assertion said 21 and 4 first, and the run said 20 and 5.
        let mut air = 0;
        let mut hits = 0;
        let mut ticks_between = Vec::new();
        let mut since = 0;
        for _ in 0..100 {
            let out = tick_air(air, water(), &mut no_draws());
            air = out.air_supply;
            since += 1;
            if out.drowned {
                hits += 1;
                ticks_between.push(since);
                since = 0;
            }
        }
        assert_eq!(hits, 5, "five hits in a hundred ticks — one a second");
        assert!(ticks_between.iter().all(|t| *t == 20), "{ticks_between:?}");
    }

    #[test]
    fn water_breathing_and_a_breathing_type_both_stop_the_loss() {
        for ctx in [
            BreathContext {
                water_breathing: true,
                ..water()
            },
            BreathContext {
                can_breathe_underwater: true,
                ..water()
            },
            BreathContext {
                invulnerable_player: true,
                ..water()
            },
        ] {
            let out = tick_air(100, ctx, &mut no_draws());
            assert_eq!(out.air_supply, 100, "it must not fall");
            assert!(!out.drowned);
        }
    }

    #[test]
    fn underwater_recovery_needs_an_effect_and_a_deficit() {
        let ctx = BreathContext {
            water_breathing: true,
            effects_refill_air: true,
            ..water()
        };
        let out = tick_air(100, ctx, &mut no_draws());
        assert_eq!(out.air_supply, 100 + AIR_RECOVERY_STEP);
        // Without the effect flag it holds rather than recovering.
        let ctx2 = BreathContext {
            water_breathing: true,
            ..water()
        };
        assert_eq!(tick_air(100, ctx2, &mut no_draws()).air_supply, 100);
    }

    #[test]
    fn the_oxygen_bonus_draws_once_and_may_keep_the_air() {
        // bonus 3 -> threshold 0.25. A draw of 0.5 is >= 0.25, so the air is
        // kept; a draw of 0.1 is not, so it falls.
        let mut kept = || 0.5_f64;
        assert_eq!(decrease_air_supply(100, 3.0, &mut kept), (100, true));
        let mut lost = || 0.1_f64;
        assert_eq!(decrease_air_supply(100, 3.0, &mut lost), (99, true));
        // At the threshold exactly it is kept: `dcmpl; iflt` means
        // `drawn >= threshold` keeps.
        let mut exact = || 0.25_f64;
        assert_eq!(decrease_air_supply(100, 3.0, &mut exact), (100, true));
    }

    #[test]
    fn a_larger_bonus_keeps_the_air_more_often() {
        // The direction of the inequality, stated as a property.
        let mut half = || 0.4_f64;
        // bonus 1 -> threshold 0.5, and 0.4 < 0.5, so it falls.
        assert_eq!(decrease_air_supply(100, 1.0, &mut half).0, 99);
        let mut half2 = || 0.4_f64;
        // bonus 3 -> threshold 0.25, and 0.4 >= 0.25, so it is kept.
        assert_eq!(decrease_air_supply(100, 3.0, &mut half2).0, 100);
    }

    #[test]
    fn a_zero_or_negative_bonus_draws_nothing_at_all() {
        let mut never = || panic!("must not draw");
        assert_eq!(decrease_air_supply(100, 0.0, &mut never), (99, false));
        assert_eq!(decrease_air_supply(100, -5.0, &mut never), (99, false));
    }

    /// The positive control. Three misreadings, each written out and shown to
    /// disagree with the transcription on a named input.
    #[test]
    fn each_misreading_of_the_air_ladder_is_separable() {
        // 1. `invulnerableTime` decremented in both places. Ten ticks would
        //    take a player from 20 to 0 rather than to 10, halving the hurt
        //    cooldown window.
        let mut inv = 20;
        for _ in 0..10 {
            let mut hurt = 0;
            tick_counters(&mut hurt, &mut inv, true);
            tick_server_player_counters(&mut inv);
        }
        assert_eq!(inv, 10);
        let both_ways = 20 - 2 * 10;
        assert_ne!(inv, both_ways);

        // 2. The drowning reset read as "back to full". The next hit would
        //    then be 320 ticks away rather than 21.
        let out = tick_air(DROWNING_AIR + 1, water(), &mut no_draws());
        assert_eq!(out.air_supply, 0);
        assert_ne!(out.air_supply, water().max_air_supply);

        // 3. `increaseAirSupply` read as `air + 1`. Recovery from empty would
        //    take 300 ticks rather than 75.
        assert_eq!(increase_air_supply(0, 300), AIR_RECOVERY_STEP);
        assert_ne!(increase_air_supply(0, 300), 1);
    }
}
