//! `net.minecraft.world.entity.ai.behavior.DoNothing`, from the 26.2 jar. Its
//! own `BehaviorControl`, not a `Behavior`: no entry condition, and
//!
//! ```text
//! tryStart (0..44):    status = RUNNING; endTimestamp = time + minDuration + level.getRandom().nextInt(maxDuration + 1 - minDuration); return true
//! tickOrStop (0..16):  if time > endTimestamp: doStop
//! doStop:              status = STOPPED
//! ```
//!
//! So it always starts, and always draws once from the level random.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{BehaviorControl, Status};
use crate::brain::memory::MemoryModuleType;
use crate::brain::BrainState;
use crate::host::MobHost;

/// `DoNothing(int, int)`.
#[derive(Debug, Clone)]
pub struct DoNothing {
    min_duration: i32,
    max_duration: i32,
    status: Status,
    end_timestamp: i64,
}

impl DoNothing {
    #[must_use]
    pub fn new(min_duration: i32, max_duration: i32) -> Self {
        DoNothing {
            min_duration,
            max_duration,
            status: Status::Stopped,
            end_timestamp: 0,
        }
    }

    /// `endTimestamp`, for tests.
    #[must_use]
    pub fn end_timestamp(&self) -> i64 {
        self.end_timestamp
    }
}

impl<M: MobHost> BehaviorControl<M> for DoNothing {
    fn status(&self) -> Status {
        self.status
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        Vec::new()
    }

    fn try_start(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) -> bool {
        self.status = Status::Running;
        let span = cx
            .mob
            .level_random_next_int(self.max_duration + 1 - self.min_duration);
        self.end_timestamp = time + i64::from(self.min_duration + span);
        true
    }

    fn tick_or_stop(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        if time > self.end_timestamp {
            self.do_stop(brain, cx, time);
        }
    }

    fn do_stop(&mut self, _brain: &mut BrainState, _cx: &mut MobAi<M>, _time: i64) {
        self.status = Status::Stopped;
    }

    fn debug_string(&self) -> &'static str {
        "DoNothing"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{GridLevel, TestMob};
    use rustmc_worldgen::rng::{LegacyRandomSource, RandomSource};

    #[test]
    fn always_starts_and_runs_min_plus_a_level_draw() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 5));
        let mut twin = LegacyRandomSource::new(5 ^ 0x5DEECE66D);
        let span = twin.next_i32_bound(31);
        let mut brain = BrainState::new();
        let mut d = DoNothing::new(30, 60);
        assert!(d.try_start(&mut brain, &mut cx, 1000));
        assert_eq!(d.end_timestamp(), 1000 + 30 + i64::from(span));
        d.tick_or_stop(&mut brain, &mut cx, d.end_timestamp());
        assert_eq!(BehaviorControl::<TestMob>::status(&d), Status::Running);
        d.tick_or_stop(&mut brain, &mut cx, d.end_timestamp() + 1);
        assert_eq!(BehaviorControl::<TestMob>::status(&d), Status::Stopped);
    }
}
