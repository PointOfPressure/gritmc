//! `net.minecraft.world.entity.ai.behavior.warden.SetWardenLookTarget`,
//! from the 26.2 jar. A `BehaviorBuilder` one-shot over
//! `group(registered(LOOK_TARGET), registered(DISTURBANCE_LOCATION),
//! registered(ROAR_TARGET), absent(ATTACK_TARGET))`.
//!
//! ```text
//! trigger (lambda$create$2, 0..55):
//!   pos = tryGet(ROAR_TARGET).map(LivingEntity::blockPosition)
//!           .or(() -> tryGet(DISTURBANCE_LOCATION))
//!   if pos.isEmpty(): return false
//!   lookTarget.set(new BlockPosTracker(pos.get()))
//!   return true
//! ```
//!
//! The `or` is lazy, so `DISTURBANCE_LOCATION` is only read when there is
//! no roar target, and the roar target wins even if it is further away.
//! The tracker is a `BlockPosTracker`, so the warden looks at the block
//! rather than tracking the entity's eyes as it moves.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::memory::{MemoryModuleType, MemoryStatus, MemoryValue, PositionTracker};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `SetWardenLookTarget`'s trigger.
#[derive(Debug, Clone, Copy, Default)]
pub struct SetWardenLookTarget;

impl SetWardenLookTarget {
    /// `create()`.
    #[must_use]
    pub fn create() -> OneShot<Self> {
        OneShot::new(SetWardenLookTarget)
    }
}

impl<M: MobHost> Trigger<M> for SetWardenLookTarget {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        if !brain.check_memory(MemoryModuleType::AttackTarget, MemoryStatus::ValueAbsent) {
            return false;
        }
        let from_roar = brain
            .get_memory_if_registered(MemoryModuleType::RoarTarget)
            .and_then(MemoryValue::as_entity)
            .and_then(|id| cx.mob.entity(id))
            .map(|f| f.block_position());
        let pos = match from_roar {
            Some(p) => Some(p),
            None => brain
                .get_memory_if_registered(MemoryModuleType::DisturbanceLocation)
                .and_then(|v| match v {
                    MemoryValue::BlockPos(p) => Some(*p),
                    _ => None,
                }),
        };
        let Some(p) = pos else {
            return false;
        };
        brain.set_memory_value(
            MemoryModuleType::LookTarget,
            MemoryValue::PositionTracker(PositionTracker::block(p)),
        );
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![
            MemoryModuleType::LookTarget,
            MemoryModuleType::DisturbanceLocation,
            MemoryModuleType::RoarTarget,
            MemoryModuleType::AttackTarget,
        ]
    }

    fn name(&self) -> &'static str {
        "SetWardenLookTarget"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};
    use rustmc_data::BlockPos;

    #[test]
    fn the_roar_target_wins_over_a_nearer_disturbance_and_an_attack_target_blocks() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        cx.mob.others = vec![(EntityClass::Living, EntityFacts::living(9, 20.5, 4.0, 30.5))];
        let mut brain = Brain::<TestMob>::new(
            &[
                T::LookTarget,
                T::DisturbanceLocation,
                T::RoarTarget,
                T::AttackTarget,
            ],
            vec![],
        )
        .state;
        let mut b = SetWardenLookTarget::create();
        assert!(!b.try_start(&mut brain, &mut cx, 0), "neither memory set");
        brain.set_memory_value(
            T::DisturbanceLocation,
            MemoryValue::BlockPos(BlockPos { x: 1, y: 2, z: 3 }),
        );
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(
            brain.get_memory(T::LookTarget),
            Some(&MemoryValue::PositionTracker(PositionTracker::block(
                BlockPos { x: 1, y: 2, z: 3 }
            )))
        );
        brain.set_memory_value(T::RoarTarget, MemoryValue::Entity(9));
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(
            brain.get_memory(T::LookTarget),
            Some(&MemoryValue::PositionTracker(PositionTracker::block(
                BlockPos { x: 20, y: 4, z: 30 }
            ))),
            "the roar target's block position wins, far or not"
        );
        brain.set_memory_value(T::AttackTarget, MemoryValue::Entity(9));
        assert!(!b.try_start(&mut brain, &mut cx, 0), "ATTACK_TARGET absent");
    }
}
