//! `ServerPlayer.tickRegeneration` — the peaceful-difficulty heal that runs on
//! the world clock rather than on the food bar.
//!
//! Reference (26.2), read from the pinned jar with `javap -p -c -constants`:
//! `net.minecraft.server.level.ServerPlayer.tickRegeneration` (offsets
//! 0..124).
//!
//! # What main had before this file
//!
//! No health, no food and no per-player tick — see `crate::health` and
//! `crate::food`. This module is the third of the three things that move a
//! player's health without anything hitting them, and the only one keyed on
//! the entity's `tickCount` rather than on state.
//!
//! # Why this is its own module and not a branch inside `crate::food`
//!
//! Because it is a **different mechanism with a different clock**, and the
//! two are easy to conflate. `FoodData.tick`'s regeneration is driven by the
//! food bar and by a counter the food data owns; this one runs off
//! `Entity.tickCount % 20` and `% 10`, is gated on `difficulty == PEACEFUL`,
//! and moves the food bar *upward* — it is the reason a peaceful player never
//! starves and never has to eat.
//!
//! Both are gated on the same `natural_health_regeneration` game rule, which
//! is the fact `ci/t2-gameplay-survival.sh` is built on: turning that one rule
//! off shuts **both** heals, which is why the survival substrate needs one
//! property change rather than a difficulty change as well.
//!
//! # Three separate things on two clocks, and one of them is a `<`
//!
//! ```text
//!   35  tickCount % 20 == 0
//!   45      health < maxHealth        -> heal(1.0f)
//!   62      saturation < 20.0f        -> setSaturation(saturation + 1.0f)
//!   88  tickCount % 10 == 0
//!   98      foodData.needsFood()      -> setFoodLevel(foodLevel + 1)
//! ```
//!
//! The saturation raise is **not** guarded by the heal: a player at full
//! health with low saturation still gains it on the same twentieth tick. And
//! it goes through `setSaturation`, which is the *unclamped* setter — the
//! `< 20.0f` test is the only ceiling, so a saturation of 19.5 becomes 20.5
//! and stays there until something else clamps it.
//!
//! The food raise is on a **different** clock (every 10 ticks, not 20) and
//! uses `needsFood()`, which is `foodLevel < 20`. So a peaceful player refills
//! food twice as fast as they regain saturation.

use crate::food::{Difficulty, FoodData};

/// The heal-and-saturation clock, `bipush 20` at offset 39.
pub const HEAL_PERIOD: i32 = 20;
/// The food-refill clock, `bipush 10` at offset 92.
pub const FOOD_PERIOD: i32 = 10;
/// The saturation ceiling this method tests against, `ldc_w 20.0f`. It is
/// `FoodConstants.MAX_SATURATION`'s value and a separate literal in the
/// bytecode; `crate::food::consts::MAX_SATURATION` is the same number and is
/// where a reader should take it from.
pub const SATURATION_CEILING: f32 = crate::food::consts::MAX_SATURATION;

/// What one `tickRegeneration` asks the caller to do.
///
/// The food-bar writes happen here because the food data is passed in; the
/// heal is returned because health lives on the entity, exactly as in
/// [`crate::food::FoodData::tick`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Regeneration {
    /// `heal(1.0f)` ran. The amount is always one, so this is a flag.
    pub healed: bool,
    /// `setSaturation(saturation + 1.0f)` ran.
    pub saturation_raised: bool,
    /// `setFoodLevel(foodLevel + 1)` ran.
    pub food_raised: bool,
}

/// `ServerPlayer.tickRegeneration()`.
///
/// `tick_count` is `Entity.tickCount`; `health` and `max_health` are the two
/// `LivingEntity` reads at offsets 45..53.
pub fn tick_regeneration(
    food: &mut FoodData,
    tick_count: i32,
    difficulty: Difficulty,
    natural_regeneration: bool,
    health: f32,
    max_health: f32,
) -> Regeneration {
    let mut out = Regeneration::default();
    if difficulty != Difficulty::Peaceful || !natural_regeneration {
        return out;
    }
    if tick_count % HEAL_PERIOD == 0 {
        // `fcmpg; ifge` — a NaN health does NOT heal.
        if health < max_health {
            out.healed = true;
        }
        let saturation = food.saturation_level();
        if saturation < SATURATION_CEILING {
            // `setSaturation`, not `add` — unclamped, so this can overshoot.
            food.set_saturation(saturation + 1.0);
            out.saturation_raised = true;
        }
    }
    if tick_count % FOOD_PERIOD == 0 && food.needs_food() {
        food.set_food_level(food.food_level() + 1);
        out.food_raised = true;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{tick_regeneration, Regeneration, SATURATION_CEILING};
    use crate::food::{Difficulty, FoodData};

    fn run(food: &mut FoodData, tick: i32, health: f32) -> Regeneration {
        tick_regeneration(food, tick, Difficulty::Peaceful, true, health, 20.0)
    }

    #[test]
    fn nothing_happens_off_peaceful() {
        let mut f = FoodData::restore(10, 0.0, 0.0, 0);
        for d in [Difficulty::Easy, Difficulty::Normal, Difficulty::Hard] {
            let out = tick_regeneration(&mut f, 20, d, true, 10.0, 20.0);
            assert_eq!(out, Regeneration::default());
        }
        assert_eq!(f.food_level(), 10);
    }

    #[test]
    fn the_gamerule_off_shuts_this_heal_as_well_as_the_food_one() {
        let mut f = FoodData::restore(10, 0.0, 0.0, 0);
        let out = tick_regeneration(&mut f, 20, Difficulty::Peaceful, false, 10.0, 20.0);
        assert_eq!(out, Regeneration::default());
        assert_eq!(f.food_level(), 10);
    }

    #[test]
    fn the_heal_is_on_the_twentieth_tick_and_the_food_on_the_tenth() {
        let mut f = FoodData::restore(10, 0.0, 0.0, 0);
        // tick 10: food only.
        let a = run(&mut f, 10, 10.0);
        assert_eq!(
            a,
            Regeneration {
                healed: false,
                saturation_raised: false,
                food_raised: true,
            }
        );
        // tick 20: both clocks.
        let b = run(&mut f, 20, 10.0);
        assert!(b.healed && b.saturation_raised && b.food_raised);
        // tick 15: neither.
        let c = run(&mut f, 15, 10.0);
        assert_eq!(c, Regeneration::default());
    }

    #[test]
    fn a_full_player_still_gains_saturation() {
        // The saturation raise is not inside the heal's `if`.
        let mut f = FoodData::restore(20, 3.0, 0.0, 0);
        let out = run(&mut f, 20, 20.0);
        assert!(!out.healed, "already at max health");
        assert!(out.saturation_raised);
        assert!((f.saturation_level() - 4.0).abs() < f32::EPSILON);
    }

    #[test]
    fn the_saturation_raise_uses_the_unclamped_setter_and_can_overshoot() {
        // 19.5 < 20.0, so the raise runs and lands on 20.5.
        let mut f = FoodData::restore(20, 19.5, 0.0, 0);
        let out = run(&mut f, 20, 20.0);
        assert!(out.saturation_raised);
        assert!(
            (f.saturation_level() - 20.5).abs() < 1e-6,
            "{}",
            f.saturation_level()
        );
        // ...and at exactly 20.0 it does not.
        let mut g = FoodData::restore(20, SATURATION_CEILING, 0.0, 0);
        let out2 = run(&mut g, 20, 20.0);
        assert!(!out2.saturation_raised);
    }

    #[test]
    fn food_refills_twice_as_fast_as_saturation() {
        let mut f = FoodData::restore(0, 0.0, 0.0, 0);
        let mut food_events = 0;
        let mut saturation_events = 0;
        for tick in 1..=200 {
            let out = run(&mut f, tick, 20.0);
            if out.food_raised {
                food_events += 1;
            }
            if out.saturation_raised {
                saturation_events += 1;
            }
        }
        assert_eq!(saturation_events, 10);
        // Twenty ticks of the ten-clock, but `needsFood()` stops it at 20.
        assert_eq!(food_events, 20);
        assert_eq!(f.food_level(), 20);
    }

    #[test]
    fn the_food_raise_stops_at_twenty_rather_than_running_away() {
        let mut f = FoodData::restore(20, 0.0, 0.0, 0);
        let out = run(&mut f, 10, 20.0);
        assert!(!out.food_raised);
        assert_eq!(f.food_level(), 20);
    }

    #[test]
    fn a_nan_health_does_not_heal() {
        // `fcmpg; ifge 62` — NaN takes the not-less-than branch.
        let mut f = FoodData::restore(20, 0.0, 0.0, 0);
        let out = run(&mut f, 20, f32::NAN);
        assert!(!out.healed);
    }

    /// The positive control. Two misreadings, each written out and shown to
    /// disagree with the transcription on a named input.
    #[test]
    fn each_misreading_of_the_peaceful_heal_is_separable() {
        // 1. The saturation raise nested inside the heal's `if`. A player at
        //    full health would then never gain saturation.
        let mut f = FoodData::restore(20, 3.0, 0.0, 0);
        let out = run(&mut f, 20, 20.0);
        assert!(out.saturation_raised && !out.healed);

        // 2. Both raises on one clock. At tick 10 the food raise runs and the
        //    saturation raise does not, so a shared clock is visible in one
        //    call.
        let mut g = FoodData::restore(10, 3.0, 0.0, 0);
        let out2 = run(&mut g, 10, 10.0);
        assert!(out2.food_raised);
        assert!(!out2.saturation_raised);
        assert!(!out2.healed);
    }
}
