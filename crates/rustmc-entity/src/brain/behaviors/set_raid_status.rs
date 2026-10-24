//! `net.minecraft.world.entity.ai.behavior.SetRaidStatus`, from the 26.2
//! jar. A `BehaviorBuilder` one-shot over `point(trigger)` — no memories.
//!
//! ```text
//! lambda$create$1 (0..89):
//!   if level.getRandom().nextInt(20) != 0: return false        <- LEVEL random, offsets 0..11
//!   raid = level.getRaidAt(mob.blockPosition())
//!   if raid != null:
//!     if raid.hasFirstWaveSpawned() && !raid.isBetweenWaves(): a = RAID  else: a = PRE_RAID
//!     brain.setDefaultActivity(a); brain.setActiveActivityIfPossible(a)
//!   return true
//! ```

use crate::ai::mob_ai::MobAi;
use crate::brain::activity::Activity;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `SetRaidStatus`'s trigger.
#[derive(Debug, Clone, Copy, Default)]
pub struct SetRaidStatus;

impl SetRaidStatus {
    /// `create()`.
    #[must_use]
    pub fn create() -> OneShot<Self> {
        OneShot::new(SetRaidStatus)
    }
}

impl<M: MobHost> Trigger<M> for SetRaidStatus {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        if cx.mob.level_random_next_int(20) != 0 {
            return false;
        }
        if let Some(raid) = cx.mob.raid_at(cx.mob.block_position()) {
            let a = if raid.has_first_wave_spawned && !raid.between_waves {
                Activity::Raid
            } else {
                Activity::PreRaid
            };
            brain.set_default_activity(a);
            brain.set_active_activity_if_possible(a);
        }
        true
    }

    fn name(&self) -> &'static str {
        "SetRaidStatus"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::Brain;
    use crate::host::RaidFacts;
    use crate::testing::{GridLevel, TestMob};
    use rustmc_worldgen::rng::{LegacyRandomSource, RandomSource};

    fn fire(
        b: &mut OneShot<SetRaidStatus>,
        brain: &mut BrainState,
        cx: &mut MobAi<TestMob>,
        twin: &mut LegacyRandomSource,
    ) {
        loop {
            let draw = twin.next_i32_bound(20);
            let r = b.try_start(brain, cx, 0);
            assert_eq!(r, draw == 0);
            if r {
                return;
            }
        }
    }

    #[test]
    fn pre_raid_between_waves_and_raid_once_the_first_wave_is_out() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 3));
        let mut twin = LegacyRandomSource::new(3 ^ 0x5DEECE66D);
        let mut brain = Brain::<TestMob>::new(&[], vec![]);
        brain.add_activity_with_priority(Activity::Raid, 0, vec![]);
        brain.add_activity_with_priority(Activity::PreRaid, 0, vec![]);
        let mut brain = brain.state;
        brain.set_core_activities(vec![Activity::Core]);
        let mut b = SetRaidStatus::create();
        fire(&mut b, &mut brain, &mut cx, &mut twin);
        assert_eq!(
            brain.default_activity(),
            Activity::Idle,
            "no raid: untouched"
        );
        cx.mob.raid = Some(RaidFacts {
            has_first_wave_spawned: true,
            between_waves: true,
            ..RaidFacts::default()
        });
        fire(&mut b, &mut brain, &mut cx, &mut twin);
        assert_eq!(brain.default_activity(), Activity::PreRaid);
        assert!(brain.is_active(Activity::PreRaid));
        cx.mob.raid = Some(RaidFacts {
            has_first_wave_spawned: true,
            between_waves: false,
            ..RaidFacts::default()
        });
        fire(&mut b, &mut brain, &mut cx, &mut twin);
        assert_eq!(brain.default_activity(), Activity::Raid);
        assert!(brain.is_active(Activity::Raid));
    }
}
