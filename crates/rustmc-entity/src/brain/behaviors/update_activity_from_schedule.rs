//! `net.minecraft.world.entity.ai.behavior.UpdateActivityFromSchedule`, from
//! the 26.2 jar: `BehaviorBuilder.create(i -> i.point((level, mob, time) ->
//! { mob.getBrain().updateActivityFromSchedule(level.environmentAttributes(),
//! level.getGameTime(), mob.position()); return true; }))`. No memories,
//! always "starts".

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::BrainState;
use crate::host::{MobHost, PathLevel};

/// `UpdateActivityFromSchedule.create()`'s trigger.
#[derive(Debug, Clone, Copy, Default)]
pub struct UpdateActivityFromSchedule;

impl UpdateActivityFromSchedule {
    /// `create()`.
    #[must_use]
    pub fn create() -> OneShot<Self> {
        OneShot::new(UpdateActivityFromSchedule)
    }
}

impl<M: MobHost> Trigger<M> for UpdateActivityFromSchedule {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        let game_time = cx.mob.level().game_time();
        brain.update_activity_from_schedule(&cx.mob, game_time);
        true
    }

    fn name(&self) -> &'static str {
        "UpdateActivityFromSchedule"
    }
}
