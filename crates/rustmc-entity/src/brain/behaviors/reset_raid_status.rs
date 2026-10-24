//! `net.minecraft.world.entity.ai.behavior.ResetRaidStatus`, from the 26.2
//! jar. A `BehaviorBuilder` one-shot over `point(trigger)` — no memories.
//!
//! ```text
//! lambda$create$1 (0..79):
//!   if level.getRandom().nextInt(20) != 0: return false        <- LEVEL random, offsets 0..11
//!   raid = level.getRaidAt(mob.blockPosition())
//!   if raid == null || raid.isStopped() || raid.isLoss():
//!     brain.setDefaultActivity(IDLE)
//!     brain.updateActivityFromSchedule(level.environmentAttributes(), gameTime, mob.position())
//!   return true
//! ```
//!
//! One level `nextInt(20)` per trigger, before anything else.

use crate::ai::mob_ai::MobAi;
use crate::brain::activity::Activity;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::BrainState;
use crate::host::{MobHost, PathLevel};

/// `ResetRaidStatus`'s trigger.
#[derive(Debug, Clone, Copy, Default)]
pub struct ResetRaidStatus;

impl ResetRaidStatus {
    /// `create()`.
    #[must_use]
    pub fn create() -> OneShot<Self> {
        OneShot::new(ResetRaidStatus)
    }
}

impl<M: MobHost> Trigger<M> for ResetRaidStatus {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        if cx.mob.level_random_next_int(20) != 0 {
            return false;
        }
        let raid = cx.mob.raid_at(cx.mob.block_position());
        if raid.is_none_or(|r| r.stopped || r.loss) {
            brain.set_default_activity(Activity::Idle);
            let game_time = cx.mob.level().game_time();
            brain.update_activity_from_schedule(&cx.mob, game_time);
        }
        true
    }

    fn name(&self) -> &'static str {
        "ResetRaidStatus"
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

    #[test]
    fn one_in_twenty_on_the_level_random_then_idle_when_the_raid_is_gone() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 7));
        let mut twin = LegacyRandomSource::new(7 ^ 0x5DEECE66D);
        let mut brain = Brain::<TestMob>::new(&[], vec![]).state;
        brain.set_core_activities(vec![Activity::Core]);
        brain.set_default_activity(Activity::Raid);
        cx.mob.raid = Some(RaidFacts {
            stopped: false,
            loss: false,
            ..RaidFacts::default()
        });
        let mut b = ResetRaidStatus::create();
        let mut fired = 0;
        for t in 0..200 {
            let draw = twin.next_i32_bound(20);
            let r = b.try_start(&mut brain, &mut cx, t);
            assert_eq!(r, draw == 0, "tick {t}");
            fired += i32::from(r);
        }
        assert!(fired > 0, "seed 7 fires within 200 ticks");
        assert_eq!(
            brain.default_activity(),
            Activity::Raid,
            "a live raid keeps RAID"
        );
        cx.mob.raid = Some(RaidFacts {
            loss: true,
            ..RaidFacts::default()
        });
        let mut t = 200;
        loop {
            let draw = twin.next_i32_bound(20);
            let r = b.try_start(&mut brain, &mut cx, t);
            assert_eq!(r, draw == 0);
            t += 1;
            if r {
                break;
            }
        }
        assert_eq!(brain.default_activity(), Activity::Idle);
    }
}
