//! `net.minecraft.world.entity.ai.behavior.EraseMemoryIf`, from the 26.2 jar.
//! A `BehaviorBuilder` one-shot over `group(present(type))`
//! (`lambda$create$2`, 0..17): `if predicate.test(mob): erase; return true;
//! return false`.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::behaviors::MobPredicate;
use crate::brain::memory::MemoryModuleType;
use crate::brain::BrainState;
use crate::host::MobHost;

/// `EraseMemoryIf`'s trigger.
pub struct EraseMemoryIf<M: MobHost> {
    pub predicate: MobPredicate<M>,
    pub memory: MemoryModuleType,
}

impl<M: MobHost + 'static> EraseMemoryIf<M> {
    /// `create(Predicate, MemoryModuleType)`.
    #[must_use]
    pub fn create(predicate: MobPredicate<M>, memory: MemoryModuleType) -> OneShot<Self> {
        OneShot::new(EraseMemoryIf { predicate, memory })
    }
}

impl<M: MobHost> Trigger<M> for EraseMemoryIf<M> {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        if !matches!(brain.memory_internal(self.memory), Some(Some(_))) {
            return false;
        }
        if (self.predicate)(cx) {
            brain.erase_memory(self.memory);
            true
        } else {
            false
        }
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![self.memory]
    }

    fn name(&self) -> &'static str {
        "EraseMemoryIf"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::{MemoryModuleType as T, MemoryValue};
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn erases_only_when_present_and_the_predicate_holds() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut brain = Brain::<TestMob>::new(&[T::Pacified], vec![]).state;
        let mut no = EraseMemoryIf::<TestMob>::create(Box::new(|_| false), T::Pacified);
        let mut yes = EraseMemoryIf::<TestMob>::create(Box::new(|_| true), T::Pacified);
        assert!(!yes.try_start(&mut brain, &mut cx, 0), "absent");
        brain.set_memory_value(T::Pacified, MemoryValue::Bool(true));
        assert!(!no.try_start(&mut brain, &mut cx, 0));
        assert!(brain.has_memory_value(T::Pacified));
        assert!(yes.try_start(&mut brain, &mut cx, 0));
        assert!(!brain.has_memory_value(T::Pacified));
    }
}
