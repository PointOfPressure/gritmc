//! `net.minecraft.world.entity.ai.behavior.SetHiddenState`, from the 26.2
//! jar. A `BehaviorBuilder` one-shot over `group(present(HIDING_PLACE),
//! present(HEARD_BELL_TIME))` with a captured `MutableInt ticksHidden`.
//! `HIDE_TIMEOUT = 300`; `create(int stayHiddenSeconds, int closeEnoughDist)`
//! keeps `stayHiddenTicks = stayHiddenSeconds * 20` (`imul` at 3).
//!
//! ```text
//! lambda$create$2 (0..119):
//!   bell = heardBellTime; timedOut = bell + 300 <= time
//!   if ticksHidden > stayHiddenTicks || timedOut:
//!     heardBell.erase(); hidingPlace.erase()
//!     brain.updateActivityFromSchedule(env, level.getGameTime(), mob.position()); ticksHidden = 0
//!     return true
//!   if hidingPlace.pos().closerThan(mob.blockPosition(), closeEnoughDist): ticksHidden++   <- distSqr < d*d
//!   return true
//! ```
//!
//! No random draw. `ticksHidden` outlives the trigger: it is reset only
//! by the timeout branch, never by a new bell.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::memory::MemoryModuleType;
use crate::brain::BrainState;
use crate::host::{MobHost, PathLevel};
use crate::phys::dist_sqr;

/// `HIDE_TIMEOUT`.
pub const HIDE_TIMEOUT: i64 = 300;

/// `SetHiddenState`'s trigger.
#[derive(Debug, Clone, Copy)]
pub struct SetHiddenState {
    pub stay_hidden_ticks: i32,
    pub close_enough_dist: i32,
    /// `ticksHidden`.
    pub ticks_hidden: i32,
}

impl SetHiddenState {
    /// `create(int, int)`.
    #[must_use]
    pub fn create(stay_hidden_seconds: i32, close_enough_dist: i32) -> OneShot<Self> {
        OneShot::new(SetHiddenState {
            stay_hidden_ticks: stay_hidden_seconds * 20,
            close_enough_dist,
            ticks_hidden: 0,
        })
    }
}

impl<M: MobHost> Trigger<M> for SetHiddenState {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) -> bool {
        let Some(Some(hiding)) = brain.memory_internal(MemoryModuleType::HidingPlace) else {
            return false;
        };
        let Some(hiding) = hiding.as_global_pos() else {
            return false;
        };
        let hiding = hiding.pos;
        let Some(Some(bell)) = brain.memory_internal(MemoryModuleType::HeardBellTime) else {
            return false;
        };
        let Some(bell) = bell.as_long() else {
            return false;
        };
        let timed_out = bell + HIDE_TIMEOUT <= time;
        if self.ticks_hidden > self.stay_hidden_ticks || timed_out {
            brain.erase_memory(MemoryModuleType::HeardBellTime);
            brain.erase_memory(MemoryModuleType::HidingPlace);
            let game_time = cx.mob.level().game_time();
            brain.update_activity_from_schedule(&cx.mob, game_time);
            self.ticks_hidden = 0;
            return true;
        }
        let d = f64::from(self.close_enough_dist);
        if dist_sqr(hiding, cx.mob.block_position()) < d * d {
            self.ticks_hidden += 1;
        }
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![
            MemoryModuleType::HidingPlace,
            MemoryModuleType::HeardBellTime,
        ]
    }

    fn name(&self) -> &'static str {
        "SetHiddenState"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::{GlobalPos, MemoryModuleType as T, MemoryValue};
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};
    use rustmc_data::{BlockPos, Dimension};

    #[test]
    fn counts_ticks_near_the_hiding_place_and_clears_on_the_bell_timeout() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut brain = Brain::<TestMob>::new(&[T::HidingPlace, T::HeardBellTime], vec![]).state;
        brain.set_memory_value(
            T::HidingPlace,
            MemoryValue::GlobalPos(GlobalPos {
                dimension: Dimension::Overworld,
                pos: BlockPos { x: 1, y: 1, z: 0 },
            }),
        );
        let mut b = SetHiddenState::create(1, 2);
        assert!(!b.try_start(&mut brain, &mut cx, 10), "no bell heard");
        brain.set_memory_value(T::HeardBellTime, MemoryValue::Long(10));
        for t in 10..30 {
            assert!(b.try_start(&mut brain, &mut cx, t));
        }
        assert_eq!(b.trigger.ticks_hidden, 20, "within 2 blocks every tick");
        assert!(b.try_start(&mut brain, &mut cx, 30));
        assert_eq!(
            b.trigger.ticks_hidden, 21,
            "21 > 20 is the exit condition, checked next tick"
        );
        assert!(b.try_start(&mut brain, &mut cx, 31));
        assert_eq!(b.trigger.ticks_hidden, 0);
        assert!(!brain.has_memory_value(T::HeardBellTime));
        assert!(!brain.has_memory_value(T::HidingPlace));
        // A bell 300 ticks old times out regardless of the count.
        brain.set_memory_value(
            T::HidingPlace,
            MemoryValue::GlobalPos(GlobalPos {
                dimension: Dimension::Overworld,
                pos: BlockPos { x: 9, y: 1, z: 0 },
            }),
        );
        brain.set_memory_value(T::HeardBellTime, MemoryValue::Long(100));
        assert!(b.try_start(&mut brain, &mut cx, 399));
        assert_eq!(b.trigger.ticks_hidden, 0, "far away: no count");
        assert!(brain.has_memory_value(T::HeardBellTime));
        assert!(b.try_start(&mut brain, &mut cx, 400));
        assert!(
            !brain.has_memory_value(T::HeardBellTime),
            "100 + 300 <= 400"
        );
    }
}
