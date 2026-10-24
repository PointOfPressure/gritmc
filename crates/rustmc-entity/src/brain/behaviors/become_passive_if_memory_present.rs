//! `net.minecraft.world.entity.ai.behavior.BecomePassiveIfMemoryPresent`,
//! from the 26.2 jar. A `BehaviorBuilder` one-shot over
//! `group(registered(ATTACK_TARGET), absent(PACIFIED), present(memoryType))`,
//! built through `Instance.point(supplier, ...)` so it carries a debug
//! name of `"become passive if " + memoryType` (`lambda$create$1`).
//!
//! ```text
//! trigger (lambda$create$3, 0..15):
//!   pacified.setWithExpiry(true, pacifyDuration)
//!   attackTarget.erase()
//!   return true
//! ```
//!
//! Unconditional past the memory gate. `ATTACK_TARGET` is only
//! `registered`, so the behaviour runs whether or not there was a target
//! — the erase is a no-op in that case, and `PACIFIED` is written either
//! way.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::memory::{MemoryModuleType, MemoryStatus, MemoryValue};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `BecomePassiveIfMemoryPresent`'s trigger.
#[derive(Debug, Clone, Copy)]
pub struct BecomePassiveIfMemoryPresent {
    /// The memory whose presence pacifies the mob.
    pub memory_type: MemoryModuleType,
    /// `pacifyDuration`.
    pub pacify_duration: i32,
}

impl BecomePassiveIfMemoryPresent {
    /// `create(MemoryModuleType, int)`.
    #[must_use]
    pub fn create(memory_type: MemoryModuleType, pacify_duration: i32) -> OneShot<Self> {
        OneShot::new(BecomePassiveIfMemoryPresent {
            memory_type,
            pacify_duration,
        })
    }
}

impl<M: MobHost> Trigger<M> for BecomePassiveIfMemoryPresent {
    fn trigger(&mut self, brain: &mut BrainState, _cx: &mut MobAi<M>, _time: i64) -> bool {
        if !brain.check_memory(MemoryModuleType::Pacified, MemoryStatus::ValueAbsent)
            || !brain.check_memory(self.memory_type, MemoryStatus::ValuePresent)
        {
            return false;
        }
        brain.set_memory_with_expiry(
            MemoryModuleType::Pacified,
            MemoryValue::Bool(true),
            i64::from(self.pacify_duration),
        );
        brain.erase_memory(MemoryModuleType::AttackTarget);
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![
            MemoryModuleType::AttackTarget,
            MemoryModuleType::Pacified,
            self.memory_type,
        ]
    }

    fn name(&self) -> &'static str {
        "BecomePassiveIfMemoryPresent"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn it_pacifies_with_an_expiry_and_drops_the_target_whether_or_not_there_was_one() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut brain =
            Brain::<TestMob>::new(&[T::AttackTarget, T::Pacified, T::HuntedRecently], vec![]).state;
        let mut b = BecomePassiveIfMemoryPresent::create(T::HuntedRecently, 200);
        assert!(!b.try_start(&mut brain, &mut cx, 0), "the memory is absent");
        brain.set_memory_value(T::HuntedRecently, MemoryValue::Bool(true));
        // No attack target: it still pacifies. ATTACK_TARGET is only
        // `registered`, not `present`.
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(brain.time_until_expiry(T::Pacified), 200);
        brain.erase_memory(T::Pacified);
        brain.set_memory_value(T::AttackTarget, MemoryValue::Entity(9));
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(!brain.has_memory_value(T::AttackTarget));
        // Already pacified: refused.
        assert!(!b.try_start(&mut brain, &mut cx, 0));
    }
}
