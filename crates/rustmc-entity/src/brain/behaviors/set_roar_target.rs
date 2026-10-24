//! `net.minecraft.world.entity.ai.behavior.warden.SetRoarTarget`, from the
//! 26.2 jar. A `BehaviorBuilder` one-shot over
//! `group(absent(ROAR_TARGET), absent(ATTACK_TARGET),
//! registered(CANT_REACH_WALK_TARGET_SINCE))`.
//!
//! ```text
//! trigger (lambda$create$2, 0..55):
//!   o = targetFinder.apply(warden)
//!   if o.filter(warden::canTargetEntity).isEmpty(): return false
//!   roarTarget.set(o.get())
//!   cantReachWalkTargetSince.erase()
//!   return true
//! ```
//!
//! **The `filter` and the `get` read the same `Optional` twice**, so the
//! value written is the finder's, unfiltered — the filter only decides
//! whether to write. That is a distinction without a difference here
//! (`filter` returns the same element) and is transcribed as written.
//!
//! `targetFinder` is a `Function<Warden, Optional<? extends LivingEntity>>`
//! the caller supplies; `WardenAi` passes `Warden::getEntityAngryAt`.
//! `canTargetEntity` is the same eight-conjunct host predicate
//! `WardenEntitySensor` uses.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::memory::{MemoryModuleType, MemoryStatus, MemoryValue};
use crate::brain::BrainState;
use crate::host::MobHost;

/// Which `targetFinder` the caller passed. `WardenAi` uses only the one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoarTargetFinder {
    /// `Warden::getEntityAngryAt`.
    EntityAngryAt,
}

/// `SetRoarTarget`'s trigger.
#[derive(Debug, Clone, Copy)]
pub struct SetRoarTarget {
    /// `targetFinder`.
    pub target_finder: RoarTargetFinder,
}

impl SetRoarTarget {
    /// `create(Function)` with `Warden::getEntityAngryAt`.
    #[must_use]
    pub fn create_entity_angry_at() -> OneShot<Self> {
        OneShot::new(SetRoarTarget {
            target_finder: RoarTargetFinder::EntityAngryAt,
        })
    }
}

impl<M: MobHost> Trigger<M> for SetRoarTarget {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        if !brain.check_memory(MemoryModuleType::RoarTarget, MemoryStatus::ValueAbsent)
            || !brain.check_memory(MemoryModuleType::AttackTarget, MemoryStatus::ValueAbsent)
        {
            return false;
        }
        let found = match self.target_finder {
            RoarTargetFinder::EntityAngryAt => cx.mob.warden_entity_angry_at(),
        };
        let Some(id) = found.filter(|id| cx.mob.can_target_entity(*id)) else {
            return false;
        };
        brain.set_memory_value(MemoryModuleType::RoarTarget, MemoryValue::Entity(id));
        brain.erase_memory(MemoryModuleType::CantReachWalkTargetSince);
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![
            MemoryModuleType::RoarTarget,
            MemoryModuleType::AttackTarget,
            MemoryModuleType::CantReachWalkTargetSince,
        ]
    }

    fn name(&self) -> &'static str {
        "SetRoarTarget"
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
    fn an_untargetable_angry_entity_writes_nothing_and_leaves_the_reach_timer() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut brain = Brain::<TestMob>::new(
            &[T::RoarTarget, T::AttackTarget, T::CantReachWalkTargetSince],
            vec![],
        )
        .state;
        brain.set_memory_value(T::CantReachWalkTargetSince, MemoryValue::Long(400));
        let mut b = SetRoarTarget::create_entity_angry_at();
        assert!(!b.try_start(&mut brain, &mut cx, 0), "nothing angry at");
        cx.mob.angry_at = Some(9);
        assert!(
            !b.try_start(&mut brain, &mut cx, 0),
            "canTargetEntity refuses it"
        );
        assert!(brain.has_memory_value(T::CantReachWalkTargetSince));
        cx.mob.targetable = vec![9];
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(
            brain.get_memory(T::RoarTarget),
            Some(&MemoryValue::Entity(9))
        );
        assert!(!brain.has_memory_value(T::CantReachWalkTargetSince));
        assert!(
            !b.try_start(&mut brain, &mut cx, 0),
            "ROAR_TARGET now present"
        );
    }
}
