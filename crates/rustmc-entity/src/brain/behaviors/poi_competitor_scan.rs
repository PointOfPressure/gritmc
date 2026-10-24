//! `net.minecraft.world.entity.ai.behavior.PoiCompetitorScan`, from the
//! 26.2 jar. A `BehaviorBuilder` one-shot over `group(present(JOB_SITE),
//! present(NEAREST_LIVING_ENTITIES))` for `Villager`.
//!
//! ```text
//! lambda$create$2 (0..37):
//!   gp = jobSite
//!   poiManager.getType(gp.pos()).ifPresent(type ->
//!     nearest.stream().filter(e -> e instanceof Villager && e != villager).map(cast)
//!            .filter(LivingEntity::isAlive).filter(v -> competesForSameJobsite(gp, type, v))
//!            .reduce(villager, PoiCompetitorScan::selectWinner))
//!   return true
//! selectWinner(a, b) (0..33): winner = a.xp > b.xp ? a : b; loser.brain.eraseMemory(JOB_SITE); return winner
//! competesForSameJobsite(gp, type, v) (0..48): v.jobSite.isPresent() && gp.equals(v.jobSite) && v.profession.heldJobSite().test(type)
//! ```
//!
//! `reduce(identity, op)` on a sequential stream folds left: the current
//! winner meets each competitor in the sensed order, and a tie goes to the
//! newcomer (`if_icmple` at 8). No random draw.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::memory::{GlobalPos, MemoryModuleType};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `PoiCompetitorScan`'s trigger.
#[derive(Debug, Clone, Copy, Default)]
pub struct PoiCompetitorScan;

impl PoiCompetitorScan {
    /// `create()`.
    #[must_use]
    pub fn create() -> OneShot<Self> {
        OneShot::new(PoiCompetitorScan)
    }

    /// `competesForSameJobsite(gp, type, v)`.
    pub fn competes_for_same_jobsite<M: MobHost>(
        cx: &MobAi<M>,
        gp: GlobalPos,
        poi_type: &str,
        other: i32,
    ) -> bool {
        cx.mob.other_villager_job_site(other) == Some(gp)
            && cx.mob.other_villager_profession_holds(other, poi_type)
    }
}

impl<M: MobHost> Trigger<M> for PoiCompetitorScan {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        let Some(Some(v)) = brain.memory_internal(MemoryModuleType::JobSite) else {
            return false;
        };
        let Some(gp) = v.as_global_pos() else {
            return false;
        };
        let Some(Some(nearest)) = brain.memory_internal(MemoryModuleType::NearestLivingEntities)
        else {
            return false;
        };
        let nearest: Vec<i32> = nearest.as_entities().unwrap_or(&[]).to_vec();
        let Some(poi_type) = cx.mob.poi_type_at(gp.pos) else {
            return true;
        };
        let me = cx.mob.id();
        // `reduce(villager, selectWinner)`: the accumulator is `None` for
        // this villager and `Some(id)` for a competitor that has beaten it.
        let mut winner: Option<i32> = None;
        for id in nearest {
            if !(cx.mob.entity_kind_is(id, "villager")
                && id != me
                && cx.mob.entity(id).is_some_and(|e| e.alive)
                && Self::competes_for_same_jobsite(cx, gp, poi_type, id))
            {
                continue;
            }
            let winner_xp = match winner {
                None => cx.mob.villager_xp(),
                Some(w) => cx.mob.other_villager_xp(w),
            };
            if winner_xp > cx.mob.other_villager_xp(id) {
                cx.mob.other_villager_erase_job_site(id);
            } else {
                match winner {
                    None => brain.erase_memory(MemoryModuleType::JobSite),
                    Some(w) => cx.mob.other_villager_erase_job_site(w),
                }
                winner = Some(id);
            }
        }
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![
            MemoryModuleType::JobSite,
            MemoryModuleType::NearestLivingEntities,
        ]
    }

    fn name(&self) -> &'static str {
        "PoiCompetitorScan"
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
    fn the_most_experienced_claimant_keeps_the_site_and_ties_go_to_the_newcomer() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let site = BlockPos { x: 3, y: 1, z: 0 };
        let gp = GlobalPos {
            dimension: Dimension::Overworld,
            pos: site,
        };
        cx.mob.pois = vec![(
            PoiRecordFacts {
                poi_type: "farmer",
                pos: site,
            },
            true,
        )];
        cx.mob.others = (2..=5)
            .map(|i| {
                (
                    EntityClass::Living,
                    EntityFacts::living(i, f64::from(i), 1.0, 0.5),
                )
            })
            .collect();
        cx.mob.entity_kinds = vec![
            (2, "villager"),
            (3, "villager"),
            (4, "villager"),
            (5, "villager"),
        ];
        cx.mob.other_job_sites = [(2, gp), (3, gp), (5, gp)].into_iter().collect();
        cx.mob.other_holds = vec![(2, "farmer"), (3, "farmer"), (5, "farmer")];
        cx.mob.other_xp = [(2, 5), (3, 10), (5, 10)].into_iter().collect();
        cx.mob.villager_xp = 5;
        let mut brain =
            Brain::<TestMob>::new(&[T::JobSite, T::NearestLivingEntities], vec![]).state;
        brain.set_memory_value(T::JobSite, MemoryValue::GlobalPos(gp));
        brain.set_memory_value(
            T::NearestLivingEntities,
            MemoryValue::Entities(vec![2, 3, 4, 5]),
        );
        let mut b = PoiCompetitorScan::create();
        assert!(b.try_start(&mut brain, &mut cx, 0));
        // me(5) vs 2(5): tie, 2 wins, I lose; 2 vs 3(10): 3 wins; 4 has no site; 3 vs 5(10): tie, 5 wins.
        assert!(!brain.has_memory_value(T::JobSite));
        assert_eq!(
            cx.mob.calls,
            ["other_erase_job_site 2", "other_erase_job_site 3"]
        );
        assert!(cx.mob.other_job_sites.contains_key(&5));
    }
}
