//! `net.minecraft.world.entity.ai.behavior.ResetProfession`, from the 26.2
//! jar. A `BehaviorBuilder` one-shot over `group(absent(JOB_SITE))`, for
//! `Villager`.
//!
//! ```text
//! lambda$create$2 (0..92):
//!   data = villager.getVillagerData()
//!   employed = !data.profession().is(NONE) && !data.profession().is(NITWIT)
//!   if employed && villager.getVillagerXp() == 0 && data.level() <= 1:
//!     villager.setVillagerData(data.withProfession(registryAccess, NONE))
//!     villager.refreshBrain(level); return true
//!   return false
//! ```
//!
//! No random draw. `refreshBrain` rebuilds the brain from the provider;
//! that is the host's to do, because it replaces the `Brain` this
//! trigger is running inside.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::memory::MemoryModuleType;
use crate::brain::BrainState;
use crate::host::MobHost;

/// `ResetProfession`'s trigger.
#[derive(Debug, Clone, Copy, Default)]
pub struct ResetProfession;

impl ResetProfession {
    /// `create()`.
    #[must_use]
    pub fn create() -> OneShot<Self> {
        OneShot::new(ResetProfession)
    }
}

impl<M: MobHost> Trigger<M> for ResetProfession {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        if brain.memory_internal(MemoryModuleType::JobSite) != Some(None) {
            return false;
        }
        let data = cx.mob.villager_data();
        let employed = data.profession != "none" && data.profession != "nitwit";
        if employed && cx.mob.villager_xp() == 0 && data.level <= 1 {
            cx.mob.set_villager_profession("none");
            cx.mob.refresh_brain();
            return true;
        }
        false
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![MemoryModuleType::JobSite]
    }

    fn name(&self) -> &'static str {
        "ResetProfession"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::{GlobalPos, MemoryModuleType as T, MemoryValue};
    use crate::brain::Brain;
    use crate::host::VillagerDataFacts;
    use crate::testing::{GridLevel, TestMob};
    use rustmc_data::{BlockPos, Dimension};

    #[test]
    fn a_jobless_novice_with_no_xp_loses_the_profession() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut brain = Brain::<TestMob>::new(&[T::JobSite], vec![]).state;
        let mut b = ResetProfession::create();
        assert!(!b.try_start(&mut brain, &mut cx, 0), "NONE stays NONE");
        cx.mob.villager_data = VillagerDataFacts {
            profession: "nitwit",
            level: 1,
        };
        assert!(
            !b.try_start(&mut brain, &mut cx, 0),
            "a nitwit is not employed"
        );
        cx.mob.villager_data.profession = "farmer";
        cx.mob.villager_xp = 1;
        assert!(!b.try_start(&mut brain, &mut cx, 0), "any xp keeps it");
        cx.mob.villager_xp = 0;
        cx.mob.villager_data.level = 2;
        assert!(!b.try_start(&mut brain, &mut cx, 0), "level 2 keeps it");
        cx.mob.villager_data.level = 1;
        brain.set_memory_value(
            T::JobSite,
            MemoryValue::GlobalPos(GlobalPos {
                dimension: Dimension::Overworld,
                pos: BlockPos { x: 0, y: 0, z: 0 },
            }),
        );
        assert!(!b.try_start(&mut brain, &mut cx, 0), "a job site blocks");
        brain.erase_memory(T::JobSite);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(cx.mob.villager_data.profession, "none");
        assert_eq!(cx.mob.calls, ["set_profession none", "refresh_brain"]);
    }
}
