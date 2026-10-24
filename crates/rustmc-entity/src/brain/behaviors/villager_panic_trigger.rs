//! `net.minecraft.world.entity.ai.behavior.VillagerPanicTrigger`, from the
//! 26.2 jar. `Behavior<Villager>` with an empty entry map and the default
//! `(60, 60)` duration, so each start makes the level `nextInt(1)` draw.
//!
//! ```text
//! isHurt(mob):     brain.hasMemoryValue(HURT_BY)
//! hasHostile(mob): brain.hasMemoryValue(NEAREST_HOSTILE)
//! canStillUse (0..19): isHurt || hasHostile
//! start (0..79):
//!   if isHurt || hasHostile:
//!     if !brain.isActive(PANIC): erase PATH, WALK_TARGET, LOOK_TARGET, BREED_TARGET, INTERACTION_TARGET
//!     brain.setActiveActivityIfPossible(PANIC)
//! tick (0..17): if time % 100 == 0: villager.spawnGolemIfNeeded(level, time, 3)
//! ```

use crate::ai::mob_ai::MobAi;
use crate::brain::activity::Activity;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::memory::MemoryModuleType;
use crate::brain::BrainState;
use crate::host::MobHost;

/// `VillagerPanicTrigger`'s body; build one with [`VillagerPanicTrigger::new`].
#[derive(Debug, Clone, Copy, Default)]
pub struct VillagerPanicTrigger;

impl VillagerPanicTrigger {
    /// `VillagerPanicTrigger()`.
    #[must_use]
    pub fn new() -> Behavior<VillagerPanicTrigger> {
        Behavior::new(VillagerPanicTrigger, Vec::new())
    }

    /// `hasHostile(mob)`.
    #[must_use]
    pub fn has_hostile(brain: &BrainState) -> bool {
        brain.has_memory_value(MemoryModuleType::NearestHostile)
    }

    /// `isHurt(mob)`.
    #[must_use]
    pub fn is_hurt(brain: &BrainState) -> bool {
        brain.has_memory_value(MemoryModuleType::HurtBy)
    }
}

impl<M: MobHost> BehaviorBody<M> for VillagerPanicTrigger {
    fn can_still_use(&mut self, brain: &mut BrainState, _cx: &mut MobAi<M>, _time: i64) -> bool {
        Self::is_hurt(brain) || Self::has_hostile(brain)
    }

    fn start(&mut self, brain: &mut BrainState, _cx: &mut MobAi<M>, _time: i64) {
        if Self::is_hurt(brain) || Self::has_hostile(brain) {
            if !brain.is_active(Activity::Panic) {
                for t in [
                    MemoryModuleType::Path,
                    MemoryModuleType::WalkTarget,
                    MemoryModuleType::LookTarget,
                    MemoryModuleType::BreedTarget,
                    MemoryModuleType::InteractionTarget,
                ] {
                    brain.erase_memory(t);
                }
            }
            brain.set_active_activity_if_possible(Activity::Panic);
        }
    }

    fn tick(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        if time % 100 == 0 {
            cx.mob.spawn_golem_if_needed(time, 3);
        }
    }

    fn name(&self) -> &'static str {
        "VillagerPanicTrigger"
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
    fn a_hostile_starts_panic_clears_the_targets_and_asks_for_a_golem_every_100() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut brain = Brain::<TestMob>::new(
            &[T::HurtBy, T::NearestHostile, T::WalkTarget, T::LookTarget],
            vec![],
        );
        brain.add_activity_with_priority(Activity::Panic, 0, vec![]);
        let mut brain = brain.state;
        brain.set_core_activities(vec![Activity::Core]);
        brain.set_memory_value(T::WalkTarget, MemoryValue::Int(0));
        let mut b = VillagerPanicTrigger::new();
        assert!(b.try_start(&mut brain, &mut cx, 0), "no entry conditions");
        assert!(
            brain.has_memory_value(T::WalkTarget),
            "nothing hostile: untouched"
        );
        b.do_stop(&mut brain, &mut cx, 1);
        brain.set_memory_value(T::NearestHostile, MemoryValue::Entity(5));
        // Started at 40 the default 60-tick duration ends at 100, so both ticks land.
        assert!(b.try_start(&mut brain, &mut cx, 40));
        assert!(!brain.has_memory_value(T::WalkTarget));
        assert!(brain.is_active(Activity::Panic));
        b.tick_or_stop(&mut brain, &mut cx, 99);
        assert!(cx.mob.calls.is_empty());
        b.tick_or_stop(&mut brain, &mut cx, 100);
        assert_eq!(cx.mob.calls, ["spawn_golem_if_needed 100 3"]);
        brain.erase_memory(T::NearestHostile);
        b.tick_or_stop(&mut brain, &mut cx, 101);
        assert_eq!(
            BehaviorControl::<TestMob>::status(&b),
            crate::brain::behavior::Status::Stopped
        );
    }
}
