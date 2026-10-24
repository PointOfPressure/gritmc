//! `net.minecraft.world.entity.ai.behavior.GoToPotentialJobSite`, from the
//! 26.2 jar. `Behavior<Villager>` with entry `{POTENTIAL_JOB_SITE:
//! VALUE_PRESENT}` and a fixed duration of `TICKS_UNTIL_TIMEOUT = 1200`
//! (one level `nextInt(1)` per start all the same).
//!
//! ```text
//! checkExtraStartConditions (0..28): brain.getActiveNonCoreActivity().map(a -> a == IDLE || a == WORK || a == PLAY).orElse(true)
//! canStillUse (0..10): brain.hasMemoryValue(POTENTIAL_JOB_SITE)
//! tick (0..28):        BehaviorUtils.setWalkAndLookTargetMemories(villager, potentialJobSite.pos(), speedModifier, 1)
//! stop (0..33):        brain.getMemory(POTENTIAL_JOB_SITE).ifPresent(gp ->
//!                          level' = server.getLevel(gp.dimension()); if level' != null:
//!                          if level'.poiManager.exists(gp.pos(), any): level'.poiManager.release(gp.pos())   <- popped
//!                          level.debugSynchronizers().updatePoi(gp.pos()))                                  <- debug only
//!                      brain.eraseMemory(POTENTIAL_JOB_SITE)
//! ```

use crate::ai::mob_ai::MobAi;
use crate::brain::activity::Activity;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::behaviors::behavior_utils::set_walk_and_look_target_memories_to_block;
use crate::brain::memory::{MemoryModuleType, MemoryStatus, MemoryValue};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `TICKS_UNTIL_TIMEOUT`.
pub const TICKS_UNTIL_TIMEOUT: i32 = 1200;

/// `GoToPotentialJobSite`'s body; build one with [`GoToPotentialJobSite::new`].
#[derive(Debug, Clone, Copy)]
pub struct GoToPotentialJobSite {
    pub speed_modifier: f32,
}

impl GoToPotentialJobSite {
    /// `GoToPotentialJobSite(float)`.
    #[must_use]
    pub fn new(speed_modifier: f32) -> Behavior<GoToPotentialJobSite> {
        Behavior::with_duration(
            GoToPotentialJobSite { speed_modifier },
            vec![(
                MemoryModuleType::PotentialJobSite,
                MemoryStatus::ValuePresent,
            )],
            TICKS_UNTIL_TIMEOUT,
        )
    }
}

impl<M: MobHost> BehaviorBody<M> for GoToPotentialJobSite {
    fn check_extra_start_conditions(&mut self, brain: &mut BrainState, _cx: &mut MobAi<M>) -> bool {
        brain
            .active_non_core_activity()
            .is_none_or(|a| a == Activity::Idle || a == Activity::Work || a == Activity::Play)
    }

    fn can_still_use(&mut self, brain: &mut BrainState, _cx: &mut MobAi<M>, _time: i64) -> bool {
        brain.has_memory_value(MemoryModuleType::PotentialJobSite)
    }

    fn tick(&mut self, brain: &mut BrainState, _cx: &mut MobAi<M>, _time: i64) {
        let pos = brain
            .get_memory(MemoryModuleType::PotentialJobSite)
            .and_then(MemoryValue::as_global_pos)
            .expect("present while running")
            .pos;
        set_walk_and_look_target_memories_to_block(brain, pos, self.speed_modifier, 1);
    }

    fn stop(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        if let Some(gp) = brain
            .get_memory(MemoryModuleType::PotentialJobSite)
            .and_then(MemoryValue::as_global_pos)
        {
            if cx.mob.poi_exists_at_global(gp) {
                cx.mob.release_poi_at_global(gp);
            }
        }
        brain.erase_memory(MemoryModuleType::PotentialJobSite);
    }

    fn name(&self) -> &'static str {
        "GoToPotentialJobSite"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::{GlobalPos, MemoryModuleType as T, WalkTarget};
    use crate::brain::Brain;
    use crate::host::PoiRecordFacts;
    use crate::testing::{GridLevel, TestMob};
    use rustmc_data::{BlockPos, Dimension};

    #[test]
    fn walks_to_the_potential_site_while_idle_and_releases_it_on_stop() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let site = BlockPos { x: 7, y: 1, z: 0 };
        cx.mob.pois = vec![(
            PoiRecordFacts {
                poi_type: "farmer",
                pos: site,
            },
            true,
        )];
        let mut brain =
            Brain::<TestMob>::new(&[T::PotentialJobSite, T::WalkTarget, T::LookTarget], vec![]);
        brain.add_activity_with_priority(Activity::Rest, 0, vec![]);
        brain.add_activity_with_priority(Activity::Idle, 0, vec![]);
        let mut brain = brain.state;
        brain.set_core_activities(vec![Activity::Core]);
        let gp = GlobalPos {
            dimension: Dimension::Overworld,
            pos: site,
        };
        brain.set_memory_value(T::PotentialJobSite, MemoryValue::GlobalPos(gp));
        let mut b = GoToPotentialJobSite::new(0.5);
        brain.set_active_activity_if_possible(Activity::Rest);
        assert!(
            !b.try_start(&mut brain, &mut cx, 0),
            "REST is not IDLE/WORK/PLAY"
        );
        brain.set_active_activity_if_possible(Activity::Idle);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        b.tick_or_stop(&mut brain, &mut cx, 1);
        assert_eq!(
            brain.get_memory(T::WalkTarget).unwrap().as_walk_target(),
            Some(&WalkTarget::at_block(site, 0.5, 1))
        );
        b.tick_or_stop(&mut brain, &mut cx, 1201);
        assert!(
            !brain.has_memory_value(T::PotentialJobSite),
            "timed out at 1200 and released"
        );
        assert_eq!(cx.mob.calls, ["release_poi 7 1 0"]);
        assert!(!cx.mob.pois[0].1);
    }
}
