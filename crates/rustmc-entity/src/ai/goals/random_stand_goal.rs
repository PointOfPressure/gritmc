//! `net.minecraft.world.entity.ai.goal.RandomStandGoal`, from the 26.2
//! jar. No flags. Used by AbstractHorse.
//!
//! The constructor calls `resetStandInterval(horse)`:
//! `nextStand = -horse.getAmbientStandInterval()`, which is
//! `getAmbientSoundInterval()`, `bipush 80` in `Mob` — no draw.
//!
//! # `canUse()`, offsets 0..81
//!
//! ```text
//! ++nextStand
//! if nextStand > 0 && random.nextInt(1000) < nextStand:      -- draw at offset 27
//!     resetStandInterval(horse)
//!     return !horse.isImmobile() && random.nextInt(10) == 0  -- draw at offset 66
//! return false
//! ```
//!
//! `canContinueToUse()` is `iconst_0`; `start()` is `horse.standIfPossible();
//! playStandSound()` where the sound is `getAmbientStandSound()` through
//! `playSound(SoundEvent)` (silent mobs skip it). Every tick.

use rustmc_worldgen::rng::RandomSource;

use crate::ai::goal::{FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::host::MobHost;

/// `RandomStandGoal`.
#[derive(Debug, Clone)]
pub struct RandomStandGoal {
    next_stand: i32,
}

impl RandomStandGoal {
    /// `RandomStandGoal(AbstractHorse)`.
    #[must_use]
    pub fn new<M: MobHost>(cx: &MobAi<M>) -> Self {
        let mut g = RandomStandGoal { next_stand: 0 };
        g.reset_stand_interval(cx);
        g
    }

    /// `resetStandInterval(AbstractHorse)`.
    fn reset_stand_interval<M: MobHost>(&mut self, cx: &MobAi<M>) {
        self.next_stand = -cx.mob.ambient_stand_interval();
    }

    /// `nextStand`, for tests.
    #[must_use]
    pub fn next_stand(&self) -> i32 {
        self.next_stand
    }
}

impl<M: MobHost> Goal<MobAi<M>> for RandomStandGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        self.next_stand += 1;
        if self.next_stand > 0 && cx.mob.random().next_i32_bound(1000) < self.next_stand {
            self.reset_stand_interval(cx);
            return !cx.mob.is_immobile() && cx.mob.random().next_i32_bound(10) == 0;
        }
        false
    }

    fn can_continue_to_use(&mut self, _cx: &mut MobAi<M>) -> bool {
        false
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        cx.mob.stand_if_possible();
        if let Some(s) = cx.mob.ambient_stand_sound() {
            cx.mob.play_sound_simple(&s);
        }
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn flags(&self) -> FlagSet {
        FlagSet::NONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmc_worldgen::rng::LegacyRandomSource;

    use crate::testing::{GridLevel, TestMob};

    /// No draw for the first 80 ticks; then `nextInt(1000) < nextStand`
    /// each tick until it hits, which resets and draws `nextInt(10)`.
    #[test]
    fn eighty_silent_ticks_then_a_rising_chance() {
        let mob = TestMob::new(GridLevel::new(), 0.5, 64.0, 0.5, 9);
        let mut ai = MobAi::new(mob);
        let mut goal = RandomStandGoal::new(&ai);
        assert_eq!(goal.next_stand(), -80);
        let before = format!("{:?}", ai.mob.random);
        for _ in 0..80 {
            assert!(!goal.can_use(&mut ai));
        }
        assert_eq!(before, format!("{:?}", ai.mob.random));
        let mut twin = LegacyRandomSource::new(9);
        let mut n = 0;
        let stood = loop {
            n += 1;
            if twin.next_i32_bound(1000) < n {
                break twin.next_i32_bound(10) == 0;
            }
        };
        for _ in 0..n - 1 {
            assert!(!goal.can_use(&mut ai));
        }
        assert_eq!(goal.can_use(&mut ai), stood);
        assert_eq!(goal.next_stand(), -80, "reset after the hit");
        goal.start(&mut ai);
        assert_eq!(
            ai.mob.calls,
            vec!["standIfPossible"],
            "no ambient sound: none played"
        );
    }
}
