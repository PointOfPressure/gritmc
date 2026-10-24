//! `net.minecraft.world.entity.ai.behavior.StrollToPoi`, from the 26.2 jar.
//! A `BehaviorBuilder` one-shot over `group(registered(WALK_TARGET),
//! present(memory))` with a captured `MutableLong nextOkStartTime = 0`
//! (`lambda$create$2`, 0..87):
//!
//! ```text
//! gp = memory
//! if level.dimension() != gp.dimension() || !gp.pos().closerToCenterThan(mob.position(), (double) maxDistanceFromPoi): return false
//! if time <= nextOkStartTime: return true
//! walk.set(new WalkTarget(gp.pos(), speed, closeEnoughDist)); nextOkStartTime = time + 80; return true
//! ```

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::memory::{MemoryModuleType, MemoryValue, WalkTarget};
use crate::brain::BrainState;
use crate::host::MobHost;
use crate::phys::closer_to_center_than;

/// `StrollToPoi`'s trigger.
#[derive(Debug, Clone, Copy)]
pub struct StrollToPoi {
    pub memory: MemoryModuleType,
    pub speed_modifier: f32,
    pub close_enough_dist: i32,
    pub max_distance_from_poi: i32,
    /// `nextOkStartTime`.
    next_ok_start_time: i64,
}

impl StrollToPoi {
    /// `create(MemoryModuleType<GlobalPos>, float, int, int)`.
    #[must_use]
    pub fn create(
        memory: MemoryModuleType,
        speed_modifier: f32,
        close_enough_dist: i32,
        max_distance_from_poi: i32,
    ) -> OneShot<Self> {
        OneShot::new(StrollToPoi {
            memory,
            speed_modifier,
            close_enough_dist,
            max_distance_from_poi,
            next_ok_start_time: 0,
        })
    }
}

impl<M: MobHost> Trigger<M> for StrollToPoi {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) -> bool {
        if brain
            .memory_internal(MemoryModuleType::WalkTarget)
            .is_none()
        {
            return false;
        }
        let Some(Some(v)) = brain.memory_internal(self.memory) else {
            return false;
        };
        let Some(gp) = v.as_global_pos() else {
            return false;
        };
        if cx.mob.dimension() != gp.dimension
            || !closer_to_center_than(
                gp.pos,
                cx.mob.position(),
                f64::from(self.max_distance_from_poi),
            )
        {
            return false;
        }
        if time <= self.next_ok_start_time {
            return true;
        }
        brain.set_memory_value(
            MemoryModuleType::WalkTarget,
            MemoryValue::WalkTarget(WalkTarget::at_block(
                gp.pos,
                self.speed_modifier,
                self.close_enough_dist,
            )),
        );
        self.next_ok_start_time = time + 80;
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![MemoryModuleType::WalkTarget, self.memory]
    }

    fn name(&self) -> &'static str {
        "StrollToPoi"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::{GlobalPos, MemoryModuleType as T};
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};
    use rustmc_data::{BlockPos, Dimension};

    #[test]
    fn walks_to_the_poi_then_answers_true_for_eighty_ticks_without_re_setting() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut brain = Brain::<TestMob>::new(&[T::WalkTarget, T::MeetingPoint], vec![]).state;
        let mut b = StrollToPoi::create(T::MeetingPoint, 0.4, 1, 10);
        brain.set_memory_value(
            T::MeetingPoint,
            MemoryValue::GlobalPos(GlobalPos {
                dimension: Dimension::Overworld,
                pos: BlockPos { x: 4, y: 1, z: 0 },
            }),
        );
        assert!(b.try_start(&mut brain, &mut cx, 100));
        assert_eq!(
            brain.get_memory(T::WalkTarget).unwrap().as_walk_target(),
            Some(&WalkTarget::at_block(BlockPos { x: 4, y: 1, z: 0 }, 0.4, 1))
        );
        brain.erase_memory(T::WalkTarget);
        assert!(
            b.try_start(&mut brain, &mut cx, 180),
            "time <= nextOk: true, nothing set"
        );
        assert!(!brain.has_memory_value(T::WalkTarget));
        assert!(b.try_start(&mut brain, &mut cx, 181));
        assert!(brain.has_memory_value(T::WalkTarget));
        brain.set_memory_value(
            T::MeetingPoint,
            MemoryValue::GlobalPos(GlobalPos {
                dimension: Dimension::Nether,
                pos: BlockPos { x: 4, y: 1, z: 0 },
            }),
        );
        assert!(!b.try_start(&mut brain, &mut cx, 400), "another dimension");
    }
}
