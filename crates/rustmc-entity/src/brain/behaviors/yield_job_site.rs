//! `net.minecraft.world.entity.ai.behavior.YieldJobSite`, from the 26.2
//! jar. A `BehaviorBuilder` one-shot over `group(present(POTENTIAL_JOB_SITE),
//! absent(JOB_SITE), present(NEAREST_LIVING_ENTITIES), registered(WALK_TARGET),
//! registered(LOOK_TARGET))` for `Villager`.
//!
//! ```text
//! lambda$create$2 (0..149):
//!   if villager.isBaby(): return false
//!   if !profession.is(NONE): return false
//!   pos = potentialJobSite.pos(); type = poiManager.getType(pos)
//!   if type.isEmpty(): return true
//!   nearest.stream().filter(e -> e instanceof Villager && e != villager).map(cast)
//!          .filter(LivingEntity::isAlive).filter(v -> nearbyWantsJobsite(type, v, pos)).findFirst()
//!          .ifPresent(v ->
//!              walk.erase(); look.erase(); potentialJobSite.erase()
//!              if v.brain.getMemory(JOB_SITE).isEmpty():
//!                  BehaviorUtils.setWalkAndLookTargetMemories(v, pos, speed, 1)
//!                  v.brain.setMemory(POTENTIAL_JOB_SITE, GlobalPos.of(level.dimension(), pos))
//!                  level.debugSynchronizers().updatePoi(pos))                       <- debug only
//!   return true
//! nearbyWantsJobsite(type, v, pos) (0..103):
//!   if v.brain.getMemory(POTENTIAL_JOB_SITE).isPresent(): return false
//!   if v.profession.heldJobSite().test(type):
//!     if v.brain.getMemory(JOB_SITE).isEmpty(): return canReachPos(v, pos, type)   <- v.navigation.createPath(pos, type.validRange()) reaches
//!     return v.jobSite.pos().equals(pos)
//!   return false
//! ```
//!
//! `nearbyWantsJobsite` reads and paths on the *other* villager; the host
//! answers it (`other_villager_wants_job_site`), then the hand-over writes
//! to that villager's brain through the host as well. No random draw here;
//! the other mob's path search draws nothing either.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::memory::{GlobalPos, MemoryModuleType};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `YieldJobSite`'s trigger.
#[derive(Debug, Clone, Copy)]
pub struct YieldJobSite {
    pub speed_modifier: f32,
}

impl YieldJobSite {
    /// `create(float)`.
    #[must_use]
    pub fn create(speed_modifier: f32) -> OneShot<Self> {
        OneShot::new(YieldJobSite { speed_modifier })
    }
}

impl<M: MobHost> Trigger<M> for YieldJobSite {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        let Some(Some(v)) = brain.memory_internal(MemoryModuleType::PotentialJobSite) else {
            return false;
        };
        let Some(gp) = v.as_global_pos() else {
            return false;
        };
        if brain.memory_internal(MemoryModuleType::JobSite) != Some(None) {
            return false;
        }
        let Some(Some(nearest)) = brain.memory_internal(MemoryModuleType::NearestLivingEntities)
        else {
            return false;
        };
        let nearest: Vec<i32> = nearest.as_entities().unwrap_or(&[]).to_vec();
        if brain
            .memory_internal(MemoryModuleType::WalkTarget)
            .is_none()
            || brain
                .memory_internal(MemoryModuleType::LookTarget)
                .is_none()
        {
            return false;
        }
        if cx.mob.is_baby() {
            return false;
        }
        if cx.mob.villager_data().profession != "none" {
            return false;
        }
        let pos = gp.pos;
        let Some(poi_type) = cx.mob.poi_type_at(pos) else {
            return true;
        };
        let me = cx.mob.id();
        let taker = nearest.into_iter().find(|id| {
            cx.mob.entity_kind_is(*id, "villager")
                && *id != me
                && cx.mob.entity(*id).is_some_and(|e| e.alive)
                && cx.mob.other_villager_wants_job_site(*id, poi_type, pos)
        });
        if let Some(other) = taker {
            brain.erase_memory(MemoryModuleType::WalkTarget);
            brain.erase_memory(MemoryModuleType::LookTarget);
            brain.erase_memory(MemoryModuleType::PotentialJobSite);
            if cx.mob.other_villager_job_site(other).is_none() {
                cx.mob
                    .other_set_walk_and_look_target(other, pos, self.speed_modifier, 1);
                cx.mob.other_set_potential_job_site(
                    other,
                    GlobalPos {
                        dimension: cx.mob.dimension(),
                        pos,
                    },
                );
            }
        }
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![
            MemoryModuleType::PotentialJobSite,
            MemoryModuleType::JobSite,
            MemoryModuleType::NearestLivingEntities,
            MemoryModuleType::WalkTarget,
            MemoryModuleType::LookTarget,
        ]
    }

    fn name(&self) -> &'static str {
        "YieldJobSite"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::{MemoryModuleType as T, MemoryValue};
    use crate::brain::Brain;
    use crate::host::{EntityClass, EntityFacts, PoiRecordFacts};
    use crate::testing::{GridLevel, TestMob};
    use rustmc_data::{BlockPos, Dimension};

    #[test]
    fn hands_the_site_to_the_first_live_villager_that_wants_it() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let site = BlockPos { x: 3, y: 1, z: 0 };
        cx.mob.pois = vec![(
            PoiRecordFacts {
                poi_type: "farmer",
                pos: site,
            },
            false,
        )];
        let mut dead = EntityFacts::living(4, 2.5, 1.0, 0.5);
        dead.alive = false;
        cx.mob.others = vec![
            (EntityClass::Living, EntityFacts::living(2, 1.5, 1.0, 0.5)),
            (EntityClass::Living, EntityFacts::living(3, 2.5, 1.0, 0.5)),
            (EntityClass::Living, dead),
            (EntityClass::Living, EntityFacts::living(5, 4.5, 1.0, 0.5)),
        ];
        cx.mob.entity_kinds = vec![(3, "villager"), (4, "villager"), (5, "villager")];
        cx.mob.other_wants_job_site = vec![(4, site), (5, site)];
        let mut brain = Brain::<TestMob>::new(
            &[
                T::PotentialJobSite,
                T::JobSite,
                T::NearestLivingEntities,
                T::WalkTarget,
                T::LookTarget,
            ],
            vec![],
        )
        .state;
        let gp = GlobalPos {
            dimension: Dimension::Overworld,
            pos: site,
        };
        brain.set_memory_value(T::PotentialJobSite, MemoryValue::GlobalPos(gp));
        brain.set_memory_value(
            T::NearestLivingEntities,
            MemoryValue::Entities(vec![2, 3, 4, 5]),
        );
        brain.set_memory_value(T::WalkTarget, MemoryValue::Int(0));
        let mut b = YieldJobSite::create(0.5);
        cx.mob.villager_data.profession = "farmer";
        assert!(!b.try_start(&mut brain, &mut cx, 0), "employed: keeps it");
        cx.mob.villager_data.profession = "none";
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(
            cx.mob.calls,
            [
                "wants_job_site 3 farmer",
                "wants_job_site 5 farmer",
                "other_walk_look 5 3 1 0 0.5 1",
                "other_potential_job_site 5 3 1 0"
            ],
            "2 is no villager, 4 is dead"
        );
        assert!(!brain.has_memory_value(T::PotentialJobSite));
        assert!(!brain.has_memory_value(T::WalkTarget));
    }
}
