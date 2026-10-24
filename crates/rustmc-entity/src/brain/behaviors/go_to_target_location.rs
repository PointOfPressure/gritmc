//! `net.minecraft.world.entity.ai.behavior.GoToTargetLocation`, from the
//! 26.2 jar. A `BehaviorBuilder` one-shot over `group(present(type),
//! absent(ATTACK_TARGET), absent(WALK_TARGET), registered(LOOK_TARGET))`
//! (`lambda$create$2`, 0..44):
//!
//! ```text
//! pos = memory
//! if !pos.closerThan(mob.blockPosition(), (double) closeEnoughDist):      <- distSqr < d*d
//!     BehaviorUtils.setWalkAndLookTargetMemories(mob, getNearbyPos(mob, pos), speed, closeEnoughDist)
//! return true
//! getNearbyPos(mob, pos):  r = mob.level().getRandom(); pos.offset(getRandomOffset(r), 0, getRandomOffset(r))   <- x draw, then z
//! getRandomOffset(r):      r.nextInt(3) - 1
//! ```
//!
//! Two `nextInt(3)` draws from the level random per walk-target set.

use rustmc_data::BlockPos;

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::behaviors::behavior_utils::set_walk_and_look_target_memories_to_block;
use crate::brain::memory::MemoryModuleType;
use crate::brain::BrainState;
use crate::host::MobHost;

/// `GoToTargetLocation`'s trigger.
#[derive(Debug, Clone, Copy)]
pub struct GoToTargetLocation {
    pub memory: MemoryModuleType,
    pub close_enough_dist: i32,
    pub speed_modifier: f32,
}

impl GoToTargetLocation {
    /// `create(MemoryModuleType<BlockPos>, int, float)`.
    #[must_use]
    pub fn create(
        memory: MemoryModuleType,
        close_enough_dist: i32,
        speed_modifier: f32,
    ) -> OneShot<Self> {
        OneShot::new(GoToTargetLocation {
            memory,
            close_enough_dist,
            speed_modifier,
        })
    }

    /// `getNearbyPos(mob, pos)`.
    fn nearby_pos<M: MobHost>(mob: &mut M, pos: BlockPos) -> BlockPos {
        let dx = mob.level_random_next_int(3) - 1;
        let dz = mob.level_random_next_int(3) - 1;
        BlockPos {
            x: pos.x + dx,
            y: pos.y,
            z: pos.z + dz,
        }
    }
}

impl<M: MobHost> Trigger<M> for GoToTargetLocation {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        let Some(Some(v)) = brain.memory_internal(self.memory) else {
            return false;
        };
        let Some(pos) = v.as_block_pos() else {
            return false;
        };
        if brain.memory_internal(MemoryModuleType::AttackTarget) != Some(None)
            || brain.memory_internal(MemoryModuleType::WalkTarget) != Some(None)
            || brain
                .memory_internal(MemoryModuleType::LookTarget)
                .is_none()
        {
            return false;
        }
        let m = cx.mob.block_position();
        let d = f64::from(self.close_enough_dist);
        let dist_sqr =
            f64::from((pos.x - m.x).pow(2) + (pos.y - m.y).pow(2) + (pos.z - m.z).pow(2));
        // `!closerThan`: distSqr < d*d negated, NaN-free on integers.
        if dist_sqr >= d * d {
            let target = Self::nearby_pos(&mut cx.mob, pos);
            set_walk_and_look_target_memories_to_block(
                brain,
                target,
                self.speed_modifier,
                self.close_enough_dist,
            );
        }
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![
            self.memory,
            MemoryModuleType::AttackTarget,
            MemoryModuleType::WalkTarget,
            MemoryModuleType::LookTarget,
        ]
    }

    fn name(&self) -> &'static str {
        "GoToTargetLocation"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::{MemoryModuleType as T, MemoryValue, PositionTracker};
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};
    use rustmc_worldgen::rng::{LegacyRandomSource, RandomSource};

    #[test]
    fn walks_to_a_jittered_spot_near_a_far_location_and_not_to_a_near_one() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 9));
        let mut brain = Brain::<TestMob>::new(
            &[
                T::CelebrateLocation,
                T::AttackTarget,
                T::WalkTarget,
                T::LookTarget,
            ],
            vec![],
        )
        .state;
        let mut b = GoToTargetLocation::create(T::CelebrateLocation, 2, 1.0);
        brain.set_memory_value(
            T::CelebrateLocation,
            MemoryValue::BlockPos(BlockPos { x: 1, y: 1, z: 0 }),
        );
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(
            !brain.has_memory_value(T::WalkTarget),
            "distSqr 1 < 4: already close enough"
        );
        brain.set_memory_value(
            T::CelebrateLocation,
            MemoryValue::BlockPos(BlockPos { x: 10, y: 1, z: 0 }),
        );
        let mut twin = LegacyRandomSource::new(9 ^ 0x5DEECE66D);
        let dx = twin.next_i32_bound(3) - 1;
        let dz = twin.next_i32_bound(3) - 1;
        assert!(b.try_start(&mut brain, &mut cx, 1));
        let w = brain
            .get_memory(T::WalkTarget)
            .unwrap()
            .as_walk_target()
            .unwrap()
            .clone();
        assert_eq!(
            w.target,
            PositionTracker::block(BlockPos {
                x: 10 + dx,
                y: 1,
                z: dz
            })
        );
        assert_eq!((w.speed_modifier, w.close_enough_dist), (1.0, 2));
    }
}
