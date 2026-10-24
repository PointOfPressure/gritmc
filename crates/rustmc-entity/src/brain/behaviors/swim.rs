//! `net.minecraft.world.entity.ai.behavior.Swim`, from the 26.2 jar.
//! `Behavior<Mob>` with an empty entry map and the default `(60, 60)`
//! duration — so it draws `nextInt(1)` from the level random on every
//! start and times out 60 ticks later, restarting next tick if still
//! swimming.
//!
//! ```text
//! shouldSwim(mob) (0..34):        mob.isInWater() && mob.getFluidHeight(WATER) > mob.getFluidJumpThreshold() || mob.isInLava()
//! checkExtraStartConditions:      shouldSwim(mob)
//! canStillUse:                    checkExtraStartConditions(level, mob)
//! tick (0..24):                   if mob.getRandom().nextFloat() < chance: mob.getJumpControl().jump()
//! ```

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::BrainState;
use crate::host::MobHost;
use rustmc_worldgen::rng::RandomSource;

/// `Swim(float chance)`'s body; build one with [`Swim::new`].
#[derive(Debug, Clone, Copy)]
pub struct Swim {
    pub chance: f32,
}

impl Swim {
    /// `Swim(float)`.
    #[must_use]
    pub fn new(chance: f32) -> Behavior<Swim> {
        Behavior::new(Swim { chance }, Vec::new())
    }

    /// `Swim.shouldSwim(mob)`.
    #[must_use]
    pub fn should_swim<M: MobHost>(mob: &M) -> bool {
        mob.is_in_water() && mob.water_fluid_height() > mob.fluid_jump_threshold()
            || mob.is_in_lava()
    }
}

impl<M: MobHost> BehaviorBody<M> for Swim {
    fn check_extra_start_conditions(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        Self::should_swim(&cx.mob)
    }

    fn can_still_use(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        self.check_extra_start_conditions(brain, cx)
    }

    fn tick(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        if cx.mob.random().next_f32() < self.chance {
            cx.jump_control.jump();
        }
    }

    fn name(&self) -> &'static str {
        "Swim"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::testing::{GridLevel, TestMob};
    use rustmc_worldgen::rng::LegacyRandomSource;

    #[test]
    fn swims_when_deep_enough_and_jumps_on_a_mob_draw() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 8));
        let mut brain = BrainState::new();
        let mut s = Swim::new(0.8);
        assert!(!s.try_start(&mut brain, &mut cx, 0), "dry");
        cx.mob.in_water = true;
        cx.mob.water_fluid_height = 0.5;
        cx.mob.fluid_jump_threshold = 0.4;
        assert!(s.try_start(&mut brain, &mut cx, 0));
        assert_eq!(s.end_timestamp(), 60);
        let mut twin = LegacyRandomSource::new(8);
        let f = twin.next_f32();
        s.tick_or_stop(&mut brain, &mut cx, 1);
        assert_eq!(cx.jump_control.wants_jump(), f < 0.8);
    }
}
