//! `net.minecraft.world.entity.ai.behavior.warden.TryToSniff`, from the
//! 26.2 jar. A `BehaviorBuilder` one-shot over
//! `group(registered(IS_SNIFFING), registered(WALK_TARGET),
//! absent(SNIFF_COOLDOWN), present(NEAREST_ATTACKABLE),
//! absent(DISTURBANCE_LOCATION))`.
//!
//! ```text
//! SNIFF_COOLDOWN = UniformInt.of(100, 200)                          (<clinit> 0..8)
//! trigger (lambda$create$2, 0..40):
//!   isSniffing.set(Unit.INSTANCE)
//!   sniffCooldown.setWithExpiry(Unit.INSTANCE, SNIFF_COOLDOWN.sample(level.random))
//!   walkTarget.erase()
//!   mob.setPose(Pose.SNIFFING)
//!   return true
//! ```
//!
//! Unconditional past the memory gate: the four statements always run in
//! that order, with one draw on the **level** random between the first and
//! the third. The static field is typed `IntProvider` but is a
//! `UniformInt.of(100, 200)`.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::behaviors::set_entity_look_target_sometimes::UniformInt;
use crate::brain::memory::{MemoryModuleType, MemoryStatus, MemoryValue};
use crate::brain::BrainState;
use crate::host::{MobHost, Pose};

/// `SNIFF_COOLDOWN`.
pub const SNIFF_COOLDOWN: UniformInt = UniformInt {
    min_inclusive: 100,
    max_inclusive: 200,
};

/// `TryToSniff`'s trigger.
#[derive(Debug, Clone, Copy, Default)]
pub struct TryToSniff;

impl TryToSniff {
    /// `create()`.
    #[must_use]
    pub fn create() -> OneShot<Self> {
        OneShot::new(TryToSniff)
    }
}

impl<M: MobHost> Trigger<M> for TryToSniff {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        if !brain.check_memory(MemoryModuleType::SniffCooldown, MemoryStatus::ValueAbsent)
            || !brain.check_memory(
                MemoryModuleType::NearestAttackable,
                MemoryStatus::ValuePresent,
            )
            || !brain.check_memory(
                MemoryModuleType::DisturbanceLocation,
                MemoryStatus::ValueAbsent,
            )
        {
            return false;
        }
        brain.set_memory_value(MemoryModuleType::IsSniffing, MemoryValue::Unit);
        let ttl = SNIFF_COOLDOWN.sample_level(&mut cx.mob);
        brain.set_memory_with_expiry(
            MemoryModuleType::SniffCooldown,
            MemoryValue::Unit,
            i64::from(ttl),
        );
        brain.erase_memory(MemoryModuleType::WalkTarget);
        cx.mob.set_pose(Pose::Sniffing);
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![
            MemoryModuleType::IsSniffing,
            MemoryModuleType::WalkTarget,
            MemoryModuleType::SniffCooldown,
            MemoryModuleType::NearestAttackable,
            MemoryModuleType::DisturbanceLocation,
        ]
    }

    fn name(&self) -> &'static str {
        "TryToSniff"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::{MemoryModuleType as T, WalkTarget};
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};
    use rustmc_data::BlockPos;

    #[test]
    fn all_four_writes_happen_in_order_with_one_level_draw_for_the_cooldown() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut brain = Brain::<TestMob>::new(
            &[
                T::IsSniffing,
                T::WalkTarget,
                T::SniffCooldown,
                T::NearestAttackable,
                T::DisturbanceLocation,
            ],
            vec![],
        )
        .state;
        brain.set_memory_value(
            T::WalkTarget,
            MemoryValue::WalkTarget(WalkTarget::at_block(BlockPos { x: 1, y: 1, z: 1 }, 1.0, 0)),
        );
        let mut b = TryToSniff::create();
        assert!(
            !b.try_start(&mut brain, &mut cx, 0),
            "no NEAREST_ATTACKABLE"
        );
        brain.set_memory_value(T::NearestAttackable, MemoryValue::Entity(9));
        let mut twin = TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1);
        let expected = SNIFF_COOLDOWN.sample_level(&mut twin);
        assert!((100..=200).contains(&expected));
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(brain.get_memory(T::IsSniffing), Some(&MemoryValue::Unit));
        assert_eq!(
            brain.time_until_expiry(T::SniffCooldown),
            i64::from(expected)
        );
        assert!(!brain.has_memory_value(T::WalkTarget), "erased");
        assert_eq!(cx.mob.pose, Pose::Sniffing);
        // The cooldown it just wrote blocks the next run.
        assert!(!b.try_start(&mut brain, &mut cx, 0));
        brain.erase_memory(T::SniffCooldown);
        brain.set_memory_value(
            T::DisturbanceLocation,
            MemoryValue::BlockPos(BlockPos { x: 0, y: 0, z: 0 }),
        );
        assert!(
            !b.try_start(&mut brain, &mut cx, 0),
            "a disturbance outranks sniffing"
        );
    }
}
