//! `net.minecraft.world.entity.ai.goal.FloatGoal`, from the 26.2 jar.
//! Flag `JUMP`; the constructor also calls
//! `mob.getNavigation().setCanFloat(true)`, which [`FloatGoal::install`]
//! does because a goal here has no mob at construction.
//!
//! `canUse()`: `(isInWater() && getFluidHeight(WATER) > getFluidJumpThreshold()) || isInLava()`.
//! `requiresUpdateEveryTick` is true. `tick()`: `if (nextFloat() < 0.8f) jumpControl.jump()`
//! — one float per tick, offsets 4..15.

use rustmc_worldgen::rng::RandomSource;

use crate::ai::goal::{Flag, FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::host::MobHost;

/// `FloatGoal`.
#[derive(Debug, Clone, Default)]
pub struct FloatGoal;

impl FloatGoal {
    /// `new FloatGoal(mob)`: the constructor's side effect on the mob's
    /// navigation, then the goal.
    pub fn install<M: MobHost>(ai: &mut MobAi<M>) -> Self {
        ai.navigation.set_can_float(true);
        FloatGoal
    }
}

impl<M: MobHost> Goal<MobAi<M>> for FloatGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        (cx.mob.is_in_water() && cx.mob.water_fluid_height() > cx.mob.fluid_jump_threshold())
            || cx.mob.is_in_lava()
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn tick(&mut self, cx: &mut MobAi<M>) {
        if cx.mob.random().next_f32() < 0.8 {
            cx.jump_control.jump();
        }
    }

    fn flags(&self) -> FlagSet {
        FlagSet::of(Flag::Jump)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{GridLevel, TestMob};
    use rustmc_worldgen::rng::LegacyRandomSource;

    #[test]
    fn install_sets_can_float_and_can_use_reads_the_fluid_facts() {
        let mut ai = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 64.0, 0.5, 3));
        assert!(!ai.navigation.can_float());
        let mut g = FloatGoal::install(&mut ai);
        assert!(ai.navigation.can_float());
        assert!(!g.can_use(&mut ai));
        ai.mob.in_water = true;
        ai.mob.water_fluid_height = 0.5;
        ai.mob.fluid_jump_threshold = 0.4;
        assert!(g.can_use(&mut ai));
        ai.mob.water_fluid_height = 0.4;
        assert!(!g.can_use(&mut ai));
        ai.mob.in_lava = true;
        assert!(g.can_use(&mut ai));
    }

    /// One float per tick, jumping below 0.8.
    #[test]
    fn tick_draws_one_float_and_jumps_below_0_8() {
        let mut ai = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 64.0, 0.5, 11));
        let mut g = FloatGoal::install(&mut ai);
        let mut r = LegacyRandomSource::new(11);
        for _ in 0..40 {
            let want = r.next_f32() < 0.8;
            g.tick(&mut ai);
            ai.jump_control.tick(&mut ai.mob);
            assert_eq!(ai.mob.jumping, want);
        }
    }
}
