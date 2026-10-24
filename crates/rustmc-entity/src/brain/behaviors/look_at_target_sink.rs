//! `net.minecraft.world.entity.ai.behavior.LookAtTargetSink`, from the 26.2
//! jar. `Behavior<Mob>` with entry `{LOOK_TARGET: VALUE_PRESENT}` and the
//! `(min, max)` durations its constructor is given (villager core: 45..90).
//!
//! ```text
//! canStillUse (0..22):  brain.getMemory(LOOK_TARGET).filter(t -> t.isVisibleBy(mob)).isPresent()
//! stop (0..10):         brain.eraseMemory(LOOK_TARGET)
//! tick (0..19):         brain.getMemory(LOOK_TARGET).ifPresent(t -> mob.getLookControl().setLookAt(t.currentPosition()))
//! ```
//!
//! No draws of its own; the base class draws the duration on start.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::behaviors::tracker_is_visible_by;
use crate::brain::memory::{MemoryModuleType, MemoryStatus};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `LookAtTargetSink`'s body; build one with [`LookAtTargetSink::new`].
#[derive(Debug, Clone, Copy, Default)]
pub struct LookAtTargetSink;

impl LookAtTargetSink {
    /// `LookAtTargetSink(int minDuration, int maxDuration)`.
    #[must_use]
    pub fn new(min_duration: i32, max_duration: i32) -> Behavior<LookAtTargetSink> {
        Behavior::with_durations(
            LookAtTargetSink,
            vec![(MemoryModuleType::LookTarget, MemoryStatus::ValuePresent)],
            min_duration,
            max_duration,
        )
    }
}

impl<M: MobHost> BehaviorBody<M> for LookAtTargetSink {
    fn can_still_use(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        let Some(t) = brain
            .get_memory(MemoryModuleType::LookTarget)
            .and_then(|v| v.as_position_tracker())
            .cloned()
        else {
            return false;
        };
        tracker_is_visible_by(&t, brain, cx)
    }

    fn stop(&mut self, brain: &mut BrainState, _cx: &mut MobAi<M>, _time: i64) {
        brain.erase_memory(MemoryModuleType::LookTarget);
    }

    fn tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        let Some(t) = brain
            .get_memory(MemoryModuleType::LookTarget)
            .and_then(|v| v.as_position_tracker())
            .cloned()
        else {
            return;
        };
        if let Some(p) = t.current_position(&cx.mob) {
            let MobAi {
                mob, look_control, ..
            } = cx;
            look_control.set_look_at(mob, p.x, p.y, p.z);
        }
    }

    fn name(&self) -> &'static str {
        "LookAtTargetSink"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::{BehaviorControl, Status};
    use crate::brain::memory::{MemoryValue, PositionTracker};
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};
    use rustmc_data::BlockPos;

    #[test]
    fn looks_at_a_block_target_until_it_is_erased() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 4));
        let mut brain = Brain::<TestMob>::new(&[MemoryModuleType::LookTarget], vec![]).state;
        let mut b = LookAtTargetSink::new(45, 90);
        assert!(!b.try_start(&mut brain, &mut cx, 0), "no look target");
        brain.set_memory_value(
            MemoryModuleType::LookTarget,
            MemoryValue::PositionTracker(PositionTracker::block(BlockPos { x: 5, y: 1, z: 0 })),
        );
        assert!(b.try_start(&mut brain, &mut cx, 0));
        b.tick_or_stop(&mut brain, &mut cx, 1);
        assert_eq!(BehaviorControl::<TestMob>::status(&b), Status::Running);
        assert_eq!(cx.look_control.wanted(), (5.5, 1.5, 0.5));
        brain.erase_memory(MemoryModuleType::LookTarget);
        b.tick_or_stop(&mut brain, &mut cx, 2);
        assert_eq!(BehaviorControl::<TestMob>::status(&b), Status::Stopped);
    }

    #[test]
    fn stopping_erases_the_look_target() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 4));
        let mut brain = Brain::<TestMob>::new(&[MemoryModuleType::LookTarget], vec![]).state;
        let mut b = LookAtTargetSink::new(1, 1);
        brain.set_memory_value(
            MemoryModuleType::LookTarget,
            MemoryValue::PositionTracker(PositionTracker::block(BlockPos { x: 5, y: 1, z: 0 })),
        );
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(b.end_timestamp(), 1);
        b.tick_or_stop(&mut brain, &mut cx, 2);
        assert_eq!(BehaviorControl::<TestMob>::status(&b), Status::Stopped);
        assert!(!brain.has_memory_value(MemoryModuleType::LookTarget));
    }
}
