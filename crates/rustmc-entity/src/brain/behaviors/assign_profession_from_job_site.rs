//! `net.minecraft.world.entity.ai.behavior.AssignProfessionFromJobSite`,
//! from the 26.2 jar. A `BehaviorBuilder` one-shot over
//! `group(present(POTENTIAL_JOB_SITE), registered(JOB_SITE))` for `Villager`.
//!
//! ```text
//! lambda$create$2 (0..127):
//!   gp = potentialJobSite
//!   if !gp.pos().closerToCenterThan(villager.position(), 2.0) && !villager.assignProfessionWhenSpawned(): return false
//!   potentialJobSite.erase(); jobSite.set(gp); level.broadcastEntityEvent(villager, 14)
//!   if !villager.getVillagerData().profession().is(NONE): return true
//!   Optional.ofNullable(server.getLevel(gp.dimension()))
//!       .flatMap(l -> l.getPoiManager().getType(gp.pos()))
//!       .flatMap(type -> VILLAGER_PROFESSION.listElements().filter(p -> p.value().heldJobSite().test(type)).findFirst())
//!       .ifPresent(p -> villager.setVillagerData(data.withProfession(p)); villager.refreshBrain(level))
//!   return true
//! ```
//!
//! No random draw. The profession lookup crosses dimensions through the
//! server (`poi_type_at_global`).

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::memory::{MemoryModuleType, MemoryValue};
use crate::brain::BrainState;
use crate::host::MobHost;
use crate::phys::closer_to_center_than;

/// `AssignProfessionFromJobSite`'s trigger.
#[derive(Debug, Clone, Copy, Default)]
pub struct AssignProfessionFromJobSite;

impl AssignProfessionFromJobSite {
    /// `create()`.
    #[must_use]
    pub fn create() -> OneShot<Self> {
        OneShot::new(AssignProfessionFromJobSite)
    }
}

impl<M: MobHost> Trigger<M> for AssignProfessionFromJobSite {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        let Some(Some(v)) = brain.memory_internal(MemoryModuleType::PotentialJobSite) else {
            return false;
        };
        let Some(gp) = v.as_global_pos() else {
            return false;
        };
        if brain.memory_internal(MemoryModuleType::JobSite).is_none() {
            return false;
        }
        if !closer_to_center_than(gp.pos, cx.mob.position(), 2.0)
            && !cx.mob.assign_profession_when_spawned()
        {
            return false;
        }
        brain.erase_memory(MemoryModuleType::PotentialJobSite);
        brain.set_memory_value(MemoryModuleType::JobSite, MemoryValue::GlobalPos(gp));
        cx.mob.broadcast_entity_event(14);
        if cx.mob.villager_data().profession != "none" {
            return true;
        }
        if let Some(profession) = cx
            .mob
            .poi_type_at_global(gp)
            .and_then(|t| cx.mob.profession_holding_job_site(t))
        {
            cx.mob.set_villager_profession(profession);
            cx.mob.refresh_brain();
        }
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![
            MemoryModuleType::PotentialJobSite,
            MemoryModuleType::JobSite,
        ]
    }

    fn name(&self) -> &'static str {
        "AssignProfessionFromJobSite"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::{GlobalPos, MemoryModuleType as T};
    use crate::brain::Brain;
    use crate::host::PoiRecordFacts;
    use crate::testing::{GridLevel, TestMob};
    use rustmc_data::{BlockPos, Dimension};

    #[test]
    fn takes_the_site_within_two_blocks_and_learns_the_profession_it_holds() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let site = BlockPos { x: 4, y: 1, z: 0 };
        cx.mob.pois = vec![(
            PoiRecordFacts {
                poi_type: "farmer",
                pos: site,
            },
            true,
        )];
        cx.mob.job_site_professions.insert("farmer", "farmer");
        let mut brain = Brain::<TestMob>::new(&[T::PotentialJobSite, T::JobSite], vec![]).state;
        let gp = GlobalPos {
            dimension: Dimension::Overworld,
            pos: site,
        };
        brain.set_memory_value(T::PotentialJobSite, MemoryValue::GlobalPos(gp));
        let mut b = AssignProfessionFromJobSite::create();
        assert!(!b.try_start(&mut brain, &mut cx, 0), "4 blocks away");
        cx.mob.x = 3.0;
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(!brain.has_memory_value(T::PotentialJobSite));
        assert_eq!(
            brain.get_memory(T::JobSite),
            Some(&MemoryValue::GlobalPos(gp))
        );
        assert_eq!(
            cx.mob.calls,
            [
                "broadcastEntityEvent(14)",
                "set_profession farmer",
                "refresh_brain"
            ]
        );
        // An employed villager only takes the site.
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        cx.mob.villager_data.profession = "cleric";
        cx.mob.assign_profession_when_spawned = true;
        brain.set_memory_value(T::PotentialJobSite, MemoryValue::GlobalPos(gp));
        assert!(
            b.try_start(&mut brain, &mut cx, 0),
            "spawn assignment ignores distance"
        );
        assert_eq!(cx.mob.calls, ["broadcastEntityEvent(14)"]);
    }
}
