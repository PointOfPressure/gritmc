//! `net.minecraft.world.entity.ai.behavior.StrollAroundPoi`, from the 26.2
//! jar. A `BehaviorBuilder` one-shot over `group(registered(WALK_TARGET),
//! present(memory))` with a captured `MutableLong nextOkStartTime = 0`
//! (`lambda$create$2`, 0..97):
//!
//! ```text
//! gp = memory
//! if level.dimension() != gp.dimension() || !gp.pos().closerToCenterThan(mob.position(), (double) distanceFromPoi): return false
//! if time <= nextOkStartTime: return true
//! walk.setOrErase(Optional.ofNullable(LandRandomPos.getPos(mob, 8, 6)).map(v -> new WalkTarget(v, speed, 1)))   <- a null ERASES
//! nextOkStartTime = time + 180; return true
//! ```
//!
//! `MIN_TIME_BETWEEN_STROLLS = 180`, `STROLL_MAX_XZ_DIST = 8`, `STROLL_MAX_Y_DIST = 6`.

use crate::ai::mob_ai::MobAi;
use crate::ai::random_pos::land_get_pos;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::memory::{MemoryModuleType, MemoryValue, WalkTarget};
use crate::brain::BrainState;
use crate::host::MobHost;
use crate::phys::closer_to_center_than;

/// `MIN_TIME_BETWEEN_STROLLS`.
pub const MIN_TIME_BETWEEN_STROLLS: i64 = 180;
/// `STROLL_MAX_XZ_DIST`.
pub const STROLL_MAX_XZ_DIST: i32 = 8;
/// `STROLL_MAX_Y_DIST`.
pub const STROLL_MAX_Y_DIST: i32 = 6;

/// `StrollAroundPoi`'s trigger.
#[derive(Debug, Clone, Copy)]
pub struct StrollAroundPoi {
    pub memory: MemoryModuleType,
    pub speed_modifier: f32,
    pub distance_from_poi: i32,
    next_ok_start_time: i64,
}

impl StrollAroundPoi {
    /// `create(MemoryModuleType<GlobalPos>, float, int)`.
    #[must_use]
    pub fn create(
        memory: MemoryModuleType,
        speed_modifier: f32,
        distance_from_poi: i32,
    ) -> OneShot<Self> {
        OneShot::new(StrollAroundPoi {
            memory,
            speed_modifier,
            distance_from_poi,
            next_ok_start_time: 0,
        })
    }
}

impl<M: MobHost> Trigger<M> for StrollAroundPoi {
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
            || !closer_to_center_than(gp.pos, cx.mob.position(), f64::from(self.distance_from_poi))
        {
            return false;
        }
        if time <= self.next_ok_start_time {
            return true;
        }
        let target = land_get_pos(cx, STROLL_MAX_XZ_DIST, STROLL_MAX_Y_DIST);
        brain.set_memory(
            MemoryModuleType::WalkTarget,
            target.map(|p| MemoryValue::WalkTarget(WalkTarget::at_vec(p, self.speed_modifier, 1))),
        );
        self.next_ok_start_time = time + MIN_TIME_BETWEEN_STROLLS;
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![MemoryModuleType::WalkTarget, self.memory]
    }

    fn name(&self) -> &'static str {
        "StrollAroundPoi"
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
    fn strolls_near_the_poi_with_land_random_pos_draws_and_rests_180_ticks() {
        let mut level = GridLevel::new();
        level.floor(-16, -16, 16, 16, 0);
        let mut cx = MobAi::new(TestMob::new(level.clone(), 0.5, 1.0, 0.5, 31));
        let mut twin = MobAi::new(TestMob::new(level, 0.5, 1.0, 0.5, 31));
        let mut brain = Brain::<TestMob>::new(&[T::WalkTarget, T::JobSite], vec![]).state;
        brain.set_memory_value(
            T::JobSite,
            MemoryValue::GlobalPos(GlobalPos {
                dimension: Dimension::Overworld,
                pos: BlockPos { x: 2, y: 1, z: 0 },
            }),
        );
        let mut b = StrollAroundPoi::create(T::JobSite, 0.4, 4);
        let expected = land_get_pos(&mut twin, 8, 6);
        assert!(b.try_start(&mut brain, &mut cx, 50));
        match expected {
            Some(p) => assert_eq!(
                brain.get_memory(T::WalkTarget).unwrap().as_walk_target(),
                Some(&WalkTarget::at_vec(p, 0.4, 1))
            ),
            None => assert!(!brain.has_memory_value(T::WalkTarget)),
        }
        brain.erase_memory(T::WalkTarget);
        assert!(
            b.try_start(&mut brain, &mut cx, 230),
            "resting: true and nothing set"
        );
        assert!(!brain.has_memory_value(T::WalkTarget));
        cx.mob.x = 20.5;
        assert!(
            !b.try_start(&mut brain, &mut cx, 231),
            "too far from the poi"
        );
    }
}
