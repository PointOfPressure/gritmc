//! `net.minecraft.world.entity.ai.behavior.SetWalkTargetFromBlockMemory`,
//! from the 26.2 jar. A `BehaviorBuilder` one-shot over
//! `group(registered(CANT_REACH_WALK_TARGET_SINCE), absent(WALK_TARGET),
//! present(memory))` (`lambda$create$2`, 0..252):
//!
//! ```text
//! gp = memory; since = cantReach.tryGet()
//! if gp.dimension() == level.dimension() && since.isPresent() && level.getGameTime() - since.get() > tooLongUnreachableDuration:
//!     villager.releasePoi(memory); memoryAccessor.erase(); cantReach.set(gameTime)
//! else if gp.pos().distManhattan(villager.blockPosition()) > tooFarDistance:
//!     v = null; i = 0
//!     while v == null || BlockPos.containing(v).distManhattan(villager.blockPosition()) > tooFarDistance:
//!         v = DefaultRandomPos.getPosTowards(villager, 15, 7, Vec3.atBottomCenterOf(gp.pos()), PI/2)
//!         if ++i == 1000: villager.releasePoi(memory); memoryAccessor.erase(); cantReach.set(gameTime); return true
//!     walk.set(new WalkTarget(v, speed, closeEnoughDist))
//! else if gp.pos().distManhattan(villager.blockPosition()) > closeEnoughDist:
//!     walk.set(new WalkTarget(gp.pos(), speed, closeEnoughDist))
//! return true
//! ```
//!
//! Note the dimension test only guards the first branch: a memory in
//! another dimension still falls through to the Manhattan tests against
//! this level's coordinates. Each `getPosTowards` attempt walks the mob's
//! random through `random_pos`; the 1000-attempt cap is a defensive limit.

use crate::ai::goals::move_towards_restriction_goal::HALF_PI_F32_AS_F64;
use crate::ai::mob_ai::MobAi;
use crate::ai::random_pos::default_get_pos_towards;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::memory::{MemoryModuleType, MemoryValue, WalkTarget};
use crate::brain::BrainState;
use crate::host::{MobHost, PathLevel};
use crate::phys::{block_pos_containing_vec, Vec3};
use rustmc_data::BlockPos;

/// `SetWalkTargetFromBlockMemory`'s trigger.
#[derive(Debug, Clone, Copy)]
pub struct SetWalkTargetFromBlockMemory {
    pub memory: MemoryModuleType,
    pub speed_modifier: f32,
    pub close_enough_dist: i32,
    pub too_far_distance: i32,
    pub too_long_unreachable_duration: i32,
}

impl SetWalkTargetFromBlockMemory {
    /// `create(MemoryModuleType<GlobalPos>, float, int, int, int)`.
    #[must_use]
    pub fn create(
        memory: MemoryModuleType,
        speed_modifier: f32,
        close_enough_dist: i32,
        too_far_distance: i32,
        too_long_unreachable_duration: i32,
    ) -> OneShot<Self> {
        OneShot::new(SetWalkTargetFromBlockMemory {
            memory,
            speed_modifier,
            close_enough_dist,
            too_far_distance,
            too_long_unreachable_duration,
        })
    }

    fn manhattan(a: BlockPos, b: BlockPos) -> i32 {
        (a.x - b.x).abs() + (a.y - b.y).abs() + (a.z - b.z).abs()
    }

    fn give_up<M: MobHost>(&self, brain: &mut BrainState, cx: &mut MobAi<M>, game_time: i64) {
        cx.mob.release_poi(self.memory);
        brain.erase_memory(self.memory);
        brain.set_memory_value(
            MemoryModuleType::CantReachWalkTargetSince,
            MemoryValue::Long(game_time),
        );
    }
}

impl<M: MobHost> Trigger<M> for SetWalkTargetFromBlockMemory {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        let Some(since) = brain.memory_internal(MemoryModuleType::CantReachWalkTargetSince) else {
            return false;
        };
        let since = since.and_then(MemoryValue::as_long);
        if brain.memory_internal(MemoryModuleType::WalkTarget) != Some(None) {
            return false;
        }
        let Some(Some(v)) = brain.memory_internal(self.memory) else {
            return false;
        };
        let Some(gp) = v.as_global_pos() else {
            return false;
        };
        let game_time = cx.mob.level().game_time();
        let here = cx.mob.block_position();
        if gp.dimension == cx.mob.dimension()
            && since.is_some_and(|s| game_time - s > i64::from(self.too_long_unreachable_duration))
        {
            self.give_up(brain, cx, game_time);
        } else if Self::manhattan(gp.pos, here) > self.too_far_distance {
            let mut v: Option<Vec3> = None;
            let mut i = 0;
            loop {
                if let Some(p) = v {
                    if Self::manhattan(block_pos_containing_vec(p), here) <= self.too_far_distance {
                        break;
                    }
                }
                v = default_get_pos_towards(
                    cx,
                    15,
                    7,
                    Vec3::at_bottom_center_of(gp.pos),
                    HALF_PI_F32_AS_F64,
                );
                i += 1;
                if i == 1000 {
                    self.give_up(brain, cx, game_time);
                    return true;
                }
            }
            let p = v.expect("loop exits with a position");
            brain.set_memory_value(
                MemoryModuleType::WalkTarget,
                MemoryValue::WalkTarget(WalkTarget::at_vec(
                    p,
                    self.speed_modifier,
                    self.close_enough_dist,
                )),
            );
        } else if Self::manhattan(gp.pos, here) > self.close_enough_dist {
            brain.set_memory_value(
                MemoryModuleType::WalkTarget,
                MemoryValue::WalkTarget(WalkTarget::at_block(
                    gp.pos,
                    self.speed_modifier,
                    self.close_enough_dist,
                )),
            );
        }
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![
            MemoryModuleType::CantReachWalkTargetSince,
            MemoryModuleType::WalkTarget,
            self.memory,
        ]
    }

    fn name(&self) -> &'static str {
        "SetWalkTargetFromBlockMemory"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::{GlobalPos, MemoryModuleType as T};
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};
    use rustmc_data::Dimension;

    fn setup(x: i32) -> (MobAi<TestMob>, BrainState) {
        let mut level = GridLevel::new();
        level.floor(-16, -16, 40, 16, 0);
        let cx = MobAi::new(TestMob::new(level, 0.5, 1.0, 0.5, 5));
        let mut brain = Brain::<TestMob>::new(
            &[T::CantReachWalkTargetSince, T::WalkTarget, T::JobSite],
            vec![],
        )
        .state;
        brain.set_memory_value(
            T::JobSite,
            MemoryValue::GlobalPos(GlobalPos {
                dimension: Dimension::Overworld,
                pos: BlockPos { x, y: 1, z: 0 },
            }),
        );
        (cx, brain)
    }

    #[test]
    fn a_near_site_beyond_close_enough_is_walked_to_directly() {
        let (mut cx, mut brain) = setup(5);
        let mut b = SetWalkTargetFromBlockMemory::create(T::JobSite, 0.5, 1, 100, 1200);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(
            brain.get_memory(T::WalkTarget).unwrap().as_walk_target(),
            Some(&WalkTarget::at_block(BlockPos { x: 5, y: 1, z: 0 }, 0.5, 1))
        );
        let (mut cx, mut brain) = setup(1);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(
            !brain.has_memory_value(T::WalkTarget),
            "already within closeEnough"
        );
    }

    #[test]
    fn an_unreachable_site_for_too_long_is_released_and_the_stamp_reset() {
        let (mut cx, mut brain) = setup(5);
        cx.mob.level.game_time = 5000;
        brain.set_memory_value(T::CantReachWalkTargetSince, MemoryValue::Long(1000));
        let mut b = SetWalkTargetFromBlockMemory::create(T::JobSite, 0.5, 1, 100, 1200);
        assert!(b.try_start(&mut brain, &mut cx, 5000));
        assert!(!brain.has_memory_value(T::JobSite));
        assert_eq!(
            brain.get_memory(T::CantReachWalkTargetSince),
            Some(&MemoryValue::Long(5000))
        );
        assert_eq!(cx.mob.released_pois, vec![T::JobSite]);
        assert!(!brain.has_memory_value(T::WalkTarget));
    }

    #[test]
    fn a_far_site_is_approached_through_get_pos_towards() {
        let (mut cx, mut brain) = setup(30);
        let mut twin = MobAi::new(TestMob::new(cx.mob.level.clone(), 0.5, 1.0, 0.5, 5));
        let mut b = SetWalkTargetFromBlockMemory::create(T::JobSite, 0.5, 1, 10, 1200);
        // The twin reproduces the loop: draw until a position within 10 of the mob.
        let mut v = None;
        for _ in 0..1000 {
            if let Some(p) = v {
                let bp = block_pos_containing_vec(p);
                if bp.x.abs() + (bp.y - 1).abs() + bp.z.abs() <= 10 {
                    break;
                }
            }
            v = default_get_pos_towards(
                &mut twin,
                15,
                7,
                Vec3::at_bottom_center_of(BlockPos { x: 30, y: 1, z: 0 }),
                HALF_PI_F32_AS_F64,
            );
        }
        assert!(b.try_start(&mut brain, &mut cx, 0));
        match v {
            Some(p) => assert_eq!(
                brain.get_memory(T::WalkTarget).unwrap().as_walk_target(),
                Some(&WalkTarget::at_vec(p, 0.5, 1))
            ),
            None => assert!(
                !brain.has_memory_value(T::JobSite),
                "1000 misses release the site"
            ),
        }
    }
}
