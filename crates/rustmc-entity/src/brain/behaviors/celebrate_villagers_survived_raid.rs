//! `net.minecraft.world.entity.ai.behavior.CelebrateVillagersSurvivedRaid`,
//! from the 26.2 jar. `Behavior<Villager>` with an empty entry map and
//! `(minDuration, maxDuration)`.
//!
//! ```text
//! checkExtraStartConditions (0..45): pos; currentRaid = level.getRaidAt(pos)
//!   currentRaid != null && currentRaid.isVictory() && MoveToSkySeeingSpot.hasNoBlocksAbove(level, villager, pos)
//! canStillUse (0..22): currentRaid != null && !currentRaid.isStopped()
//! stop (0..24): currentRaid = null; brain.updateActivityFromSchedule(env, gameTime, position)
//! tick (0..113): r = villager.getRandom()                                                     <- MOB random
//!   if r.nextInt(100) == 0: villager.playCelebrateSound()
//!   if r.nextInt(200) == 0 && hasNoBlocksAbove(level, villager, blockPosition):
//!     color = Util.getRandom(DyeColor.values(), r)        <- values()[r.nextInt(16)]
//!     flight = r.nextInt(3)
//!     spawnProjectile(new FireworkRocketEntity(level, villager, x, eyeY, z, getFirework(color, flight)), level, stack)
//! ```
//!
//! `currentRaid` is a live reference in the jar; here it is the raid's
//! id, re-read through `raid_by_id` so `isStopped` is current. Draw order
//! per tick: `nextInt(100)`, `nextInt(200)`, then on a hit under open sky
//! `nextInt(16)` and `nextInt(3)`. `DyeColor.values()`: white, orange,
//! magenta, light_blue, yellow, lime, pink, gray, light_gray, cyan,
//! purple, blue, brown, green, red, black.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::behaviors::move_to_sky_seeing_spot::has_no_blocks_above;
use crate::brain::BrainState;
use crate::host::{MobHost, PathLevel};
use rustmc_worldgen::rng::RandomSource;

/// `DyeColor.values()`, in ordinal order.
pub const DYE_COLORS: [&str; 16] = [
    "white",
    "orange",
    "magenta",
    "light_blue",
    "yellow",
    "lime",
    "pink",
    "gray",
    "light_gray",
    "cyan",
    "purple",
    "blue",
    "brown",
    "green",
    "red",
    "black",
];

/// `CelebrateVillagersSurvivedRaid`'s body; build one with [`CelebrateVillagersSurvivedRaid::new`].
#[derive(Debug, Clone, Copy, Default)]
pub struct CelebrateVillagersSurvivedRaid {
    /// `currentRaid`, by id.
    pub current_raid: Option<i32>,
}

impl CelebrateVillagersSurvivedRaid {
    /// `CelebrateVillagersSurvivedRaid(int, int)`.
    #[must_use]
    pub fn new(min_duration: i32, max_duration: i32) -> Behavior<CelebrateVillagersSurvivedRaid> {
        Behavior::with_durations(
            CelebrateVillagersSurvivedRaid::default(),
            Vec::new(),
            min_duration,
            max_duration,
        )
    }
}

impl<M: MobHost> BehaviorBody<M> for CelebrateVillagersSurvivedRaid {
    fn check_extra_start_conditions(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        let pos = cx.mob.block_position();
        let raid = cx.mob.raid_at(pos);
        self.current_raid = raid.map(|r| r.id);
        raid.is_some_and(|r| r.victory) && has_no_blocks_above(cx, pos)
    }

    fn can_still_use(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        self.current_raid
            .and_then(|id| cx.mob.raid_by_id(id))
            .is_some_and(|r| !r.stopped)
    }

    fn stop(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        self.current_raid = None;
        let game_time = cx.mob.level().game_time();
        brain.update_activity_from_schedule(&cx.mob, game_time);
    }

    fn tick(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        if cx.mob.random().next_i32_bound(100) == 0 {
            cx.mob.play_celebrate_sound();
        }
        if cx.mob.random().next_i32_bound(200) == 0
            && has_no_blocks_above(cx, cx.mob.block_position())
        {
            #[allow(clippy::cast_sign_loss)]
            let color = DYE_COLORS[cx.mob.random().next_i32_bound(16) as usize];
            let flight = cx.mob.random().next_i32_bound(3);
            let (x, y, z) = (cx.mob.x(), cx.mob.eye_y(), cx.mob.z());
            cx.mob.spawn_firework_rocket(color, flight, x, y, z);
        }
    }

    fn name(&self) -> &'static str {
        "CelebrateVillagersSurvivedRaid"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::{BehaviorControl, Status};
    use crate::brain::Brain;
    use crate::host::RaidFacts;
    use crate::testing::{GridLevel, TestMob};
    use rustmc_worldgen::rng::LegacyRandomSource;

    #[test]
    fn celebrates_a_won_raid_under_open_sky_with_seed_replayed_sounds_and_fireworks() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 6));
        let mut twin = LegacyRandomSource::new(6);
        let mut brain = Brain::<TestMob>::new(&[], vec![]).state;
        let mut b = CelebrateVillagersSurvivedRaid::new(600, 600);
        assert!(!b.try_start(&mut brain, &mut cx, 0), "no raid");
        cx.mob.raid = Some(RaidFacts {
            id: 4,
            victory: true,
            ..RaidFacts::default()
        });
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(b.body.current_raid, Some(4));
        let mut expect = Vec::new();
        for t in 1..=500 {
            b.tick_or_stop(&mut brain, &mut cx, t);
            if twin.next_i32_bound(100) == 0 {
                expect.push("celebrate_sound".to_string());
            }
            if twin.next_i32_bound(200) == 0 {
                let c = DYE_COLORS[twin.next_i32_bound(16) as usize];
                let f = twin.next_i32_bound(3);
                expect.push(format!("firework {c} {f} 0.5 2.74 0.5"));
            }
        }
        assert_eq!(cx.mob.calls, expect);
        assert!(expect.len() > 2, "seed 6 celebrates within 500 ticks");
        cx.mob.raid.as_mut().unwrap().stopped = true;
        b.tick_or_stop(&mut brain, &mut cx, 501);
        assert_eq!(BehaviorControl::<TestMob>::status(&b), Status::Stopped);
        assert_eq!(b.body.current_raid, None);
    }
}
