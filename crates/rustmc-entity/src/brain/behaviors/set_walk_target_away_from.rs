//! `net.minecraft.world.entity.ai.behavior.SetWalkTargetAwayFrom`, from the
//! 26.2 jar. A `BehaviorBuilder` one-shot over `group(registered(WALK_TARGET),
//! present(type))` (`lambda$create$2`, 0..185):
//!
//! ```text
//! cur = walk.tryGet()
//! if cur.isPresent() && !ignoreWalkTargetAlreadySet: return false
//! mobPos = mob.position(); target = toPosition.apply(memory)
//! if !mobPos.closerThan(target, (double) desiredDistance): return false          <- distanceToSqr < d*d
//! if cur.isPresent() && cur.speedModifier == speed:                              <- fcmpl ifne: exact float equality
//!     dir1 = cur.target.currentPosition() - mobPos; dir2 = target - mobPos
//!     if dir1.dot(dir2) < 0: return false
//! for i in 0..10:
//!     v = LandRandomPos.getPosAway(mob, 16, 7, target)
//!     if v != null: walk.set(new WalkTarget(v, speed, 0)); break
//! return true
//! pos(type, speed, dist, ignore):    toPosition = Vec3.atBottomCenterOf(BlockPos)
//! entity(type, speed, dist, ignore): toPosition = Entity.position()
//! ```
//!
//! Up to ten `getPosAway` attempts, each walking the mob's random through
//! `random_pos`; a walk target already leading away from the threat
//! (positive dot product) is kept without a draw.

use crate::ai::mob_ai::MobAi;
use crate::ai::random_pos::land_get_pos_away;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::memory::{MemoryModuleType, MemoryValue, WalkTarget};
use crate::brain::BrainState;
use crate::host::MobHost;
use crate::phys::Vec3;

/// Which memory shape `toPosition` reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AwayFrom {
    /// `pos(...)`: a `BlockPos` memory, `Vec3.atBottomCenterOf`.
    BlockPos,
    /// `entity(...)`: an entity memory, `position()`.
    Entity,
}

/// `SetWalkTargetAwayFrom`'s trigger.
#[derive(Debug, Clone, Copy)]
pub struct SetWalkTargetAwayFrom {
    pub memory: MemoryModuleType,
    pub shape: AwayFrom,
    pub speed_modifier: f32,
    pub desired_distance: i32,
    pub ignore_walk_target_already_set: bool,
}

impl SetWalkTargetAwayFrom {
    /// `pos(MemoryModuleType<BlockPos>, float, int, boolean)`.
    #[must_use]
    pub fn pos(
        memory: MemoryModuleType,
        speed: f32,
        desired_distance: i32,
        ignore: bool,
    ) -> OneShot<Self> {
        OneShot::new(SetWalkTargetAwayFrom {
            memory,
            shape: AwayFrom::BlockPos,
            speed_modifier: speed,
            desired_distance,
            ignore_walk_target_already_set: ignore,
        })
    }

    /// `entity(MemoryModuleType<? extends Entity>, float, int, boolean)`.
    #[must_use]
    pub fn entity(
        memory: MemoryModuleType,
        speed: f32,
        desired_distance: i32,
        ignore: bool,
    ) -> OneShot<Self> {
        OneShot::new(SetWalkTargetAwayFrom {
            memory,
            shape: AwayFrom::Entity,
            speed_modifier: speed,
            desired_distance,
            ignore_walk_target_already_set: ignore,
        })
    }

    fn position_of<M: MobHost>(self, cx: &MobAi<M>, v: &MemoryValue) -> Option<Vec3> {
        match self.shape {
            AwayFrom::BlockPos => v.as_block_pos().map(Vec3::at_bottom_center_of),
            AwayFrom::Entity => v
                .as_entity()
                .and_then(|id| cx.mob.entity(id))
                .map(|e| e.position()),
        }
    }
}

impl<M: MobHost> Trigger<M> for SetWalkTargetAwayFrom {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        let Some(cur) = brain.memory_internal(MemoryModuleType::WalkTarget) else {
            return false;
        };
        let cur = cur.and_then(MemoryValue::as_walk_target).cloned();
        let Some(Some(v)) = brain.memory_internal(self.memory) else {
            return false;
        };
        if cur.is_some() && !self.ignore_walk_target_already_set {
            return false;
        }
        let mob_pos = cx.mob.position();
        let Some(target) = self.position_of(cx, v) else {
            return false;
        };
        let d = f64::from(self.desired_distance);
        if mob_pos.distance_to_sqr(target) >= d * d {
            return false;
        }
        if let Some(c) = &cur {
            if c.speed_modifier == self.speed_modifier {
                if let Some(cp) = c.target.current_position(&cx.mob) {
                    let dir1 = cp.subtract(mob_pos);
                    let dir2 = target.subtract(mob_pos);
                    if dir1.dot(dir2) < 0.0 {
                        return false;
                    }
                }
            }
        }
        for _ in 0..10 {
            if let Some(v) = land_get_pos_away(cx, 16, 7, target) {
                brain.set_memory_value(
                    MemoryModuleType::WalkTarget,
                    MemoryValue::WalkTarget(WalkTarget::at_vec(v, self.speed_modifier, 0)),
                );
                break;
            }
        }
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![MemoryModuleType::WalkTarget, self.memory]
    }

    fn name(&self) -> &'static str {
        "SetWalkTargetAwayFrom"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::{MemoryModuleType as T, PositionTracker};
    use crate::brain::Brain;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};
    use rustmc_data::BlockPos;

    fn world() -> MobAi<TestMob> {
        let mut level = GridLevel::new();
        level.floor(-24, -24, 24, 24, 0);
        MobAi::new(TestMob::new(level, 0.5, 1.0, 0.5, 17))
    }

    #[test]
    fn flees_a_close_entity_with_the_same_draws_as_get_pos_away() {
        let mut cx = world();
        cx.mob.others = vec![(EntityClass::Living, EntityFacts::living(3, 3.5, 1.0, 0.5))];
        let mut twin = world();
        let mut brain = Brain::<TestMob>::new(&[T::WalkTarget, T::AvoidTarget], vec![]).state;
        brain.set_memory_value(T::AvoidTarget, MemoryValue::Entity(3));
        let mut b = SetWalkTargetAwayFrom::entity(T::AvoidTarget, 1.0, 6, false);
        let mut expected = None;
        for _ in 0..10 {
            expected = land_get_pos_away(&mut twin, 16, 7, Vec3::new(3.5, 1.0, 0.5));
            if expected.is_some() {
                break;
            }
        }
        assert!(b.try_start(&mut brain, &mut cx, 0));
        match expected {
            Some(v) => assert_eq!(
                brain.get_memory(T::WalkTarget).unwrap().as_walk_target(),
                Some(&WalkTarget::at_vec(v, 1.0, 0))
            ),
            None => assert!(!brain.has_memory_value(T::WalkTarget)),
        }
    }

    #[test]
    fn a_far_threat_or_an_existing_walk_target_refuses() {
        let mut cx = world();
        let mut brain = Brain::<TestMob>::new(&[T::WalkTarget, T::NearestRepellent], vec![]).state;
        brain.set_memory_value(
            T::NearestRepellent,
            MemoryValue::BlockPos(BlockPos { x: 20, y: 1, z: 0 }),
        );
        let mut b = SetWalkTargetAwayFrom::pos(T::NearestRepellent, 1.0, 8, false);
        assert!(
            !b.try_start(&mut brain, &mut cx, 0),
            "20 blocks off is not within 8"
        );
        brain.set_memory_value(
            T::NearestRepellent,
            MemoryValue::BlockPos(BlockPos { x: 3, y: 1, z: 0 }),
        );
        brain.set_memory_value(
            T::WalkTarget,
            MemoryValue::WalkTarget(WalkTarget::new(
                PositionTracker::block(BlockPos { x: -5, y: 1, z: 0 }),
                1.0,
                0,
            )),
        );
        assert!(
            !b.try_start(&mut brain, &mut cx, 0),
            "a walk target already set, not ignored"
        );
        // Ignoring it: the existing target at the same speed already leads
        // away (dot > 0 would refuse; here the threat is +x and the target -x).
        let mut b = SetWalkTargetAwayFrom::pos(T::NearestRepellent, 1.0, 8, true);
        assert!(
            !b.try_start(&mut brain, &mut cx, 0),
            "dot < 0: leading away already"
        );
        brain.set_memory_value(
            T::WalkTarget,
            MemoryValue::WalkTarget(WalkTarget::new(
                PositionTracker::block(BlockPos { x: 6, y: 1, z: 0 }),
                1.0,
                0,
            )),
        );
        assert!(
            b.try_start(&mut brain, &mut cx, 0),
            "dot >= 0: towards the threat, re-aim"
        );
    }
}
