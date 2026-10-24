//! `net.minecraft.world.entity.ai.goal.RunAroundLikeCrazyGoal`, from the
//! 26.2 jar. Flag `MOVE`. Used by AbstractHorse and Llama.
//!
//! # `canUse()`, offsets 0..73
//!
//! ```text
//! if horse.isMobControlled() || horse.isTamed() || !horse.isVehicle() return false
//! v = DefaultRandomPos.getPos(horse, 5, 4); if null return false
//! posX/Y/Z = v; return true
//! ```
//!
//! `start()`: `navigation.moveTo(posX, posY, posZ, speedModifier)`.
//! `canContinueToUse()`: `!isTamed() && !navigation.isDone() && isVehicle()`.
//!
//! # `tick()`, offsets 0..145
//!
//! ```text
//! if isTamed() return
//! if random.nextInt(adjustedTickDelay(50)) != 0 return          -- nextInt(25), offset 23
//! p = horse.getFirstPassenger(); if null return
//! if p instanceof Player:
//!     temper = getTemper(); max = getMaxTemper()
//!     if max > 0 && random.nextInt(max) < temper:               -- offset 87
//!         tameWithName(p); return
//!     modifyTemper(5)
//! ejectPassengers(); makeMad(); level.broadcastEntityEvent(horse, 6)
//! ```
//!
//! A non-player passenger skips the temper block and is thrown at once.

use rustmc_worldgen::rng::RandomSource;

use crate::ai::goal::{adjusted_tick_delay, Flag, FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::ai::random_pos::default_get_pos;
use crate::host::MobHost;

/// `RunAroundLikeCrazyGoal`.
#[derive(Debug, Clone)]
pub struct RunAroundLikeCrazyGoal {
    speed_modifier: f64,
    pos_x: f64,
    pos_y: f64,
    pos_z: f64,
}

impl RunAroundLikeCrazyGoal {
    /// `RunAroundLikeCrazyGoal(AbstractHorse, double)`.
    #[must_use]
    pub fn new(speed_modifier: f64) -> Self {
        RunAroundLikeCrazyGoal {
            speed_modifier,
            pos_x: 0.0,
            pos_y: 0.0,
            pos_z: 0.0,
        }
    }
}

impl<M: MobHost> Goal<MobAi<M>> for RunAroundLikeCrazyGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        if cx.mob.is_mob_controlled() || cx.mob.horse_is_tamed() || !cx.mob.is_vehicle() {
            return false;
        }
        let Some(v) = default_get_pos(cx, 5, 4) else {
            return false;
        };
        self.pos_x = v.x;
        self.pos_y = v.y;
        self.pos_z = v.z;
        true
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        let (x, y, z, s) = (self.pos_x, self.pos_y, self.pos_z, self.speed_modifier);
        cx.navigation.move_to(&mut cx.mob, x, y, z, s);
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        !cx.mob.horse_is_tamed() && !cx.navigation.is_done() && cx.mob.is_vehicle()
    }

    fn tick(&mut self, cx: &mut MobAi<M>) {
        if cx.mob.horse_is_tamed() {
            return;
        }
        if cx
            .mob
            .random()
            .next_i32_bound(adjusted_tick_delay(false, 50))
            != 0
        {
            return;
        }
        let Some(p) = cx.mob.first_passenger() else {
            return;
        };
        let is_player = cx.mob.entity(p).is_some_and(|f| f.is_player);
        if is_player {
            let temper = cx.mob.temper();
            let max = cx.mob.max_temper();
            if max > 0 && cx.mob.random().next_i32_bound(max) < temper {
                cx.mob.tame_with_name(p);
                return;
            }
            cx.mob.modify_temper(5);
        }
        cx.mob.eject_passengers();
        cx.mob.make_mad();
        cx.mob.broadcast_entity_event(6);
    }

    fn flags(&self) -> FlagSet {
        FlagSet::of(Flag::Move)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmc_worldgen::rng::LegacyRandomSource;

    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    fn ridden_horse(seed: i64, rider_is_player: bool) -> MobAi<TestMob> {
        let mut g = GridLevel::new();
        g.floor(-20, -20, 20, 20, 63);
        let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, seed);
        mob.on_ground = true;
        mob.is_vehicle = true;
        mob.first_passenger = Some(9);
        let f = if rider_is_player {
            EntityFacts::player(9, 0.5, 64.0, 0.5)
        } else {
            EntityFacts::living(9, 0.5, 64.0, 0.5)
        };
        mob.others.push((EntityClass::Player, f));
        MobAi::new(mob)
    }

    #[test]
    fn only_an_untamed_ridden_horse_runs() {
        let mut goal = RunAroundLikeCrazyGoal::new(1.2);
        let mut ai = ridden_horse(1, true);
        assert!(goal.can_use(&mut ai));
        ai.mob.horse_tamed = true;
        assert!(!goal.can_use(&mut ai));
        ai.mob.horse_tamed = false;
        ai.mob.is_vehicle = false;
        assert!(!goal.can_use(&mut ai));
    }

    /// Replayed by seed: on the tick where `nextInt(25)` is 0, a rider
    /// with `nextInt(100) < temper` tames; otherwise temper rises by 5
    /// and the rider is thrown with entity event 6.
    #[test]
    fn a_zero_draw_decides_taming_or_a_throw() {
        let mut goal = RunAroundLikeCrazyGoal::new(1.2);
        let mut ai = ridden_horse(11, true);
        ai.mob.temper = 0;
        let mut twin = LegacyRandomSource::new(11);
        let mut ticks = 0;
        while twin.next_i32_bound(25) != 0 {
            ticks += 1;
        }
        let _ = twin.next_i32_bound(100);
        for _ in 0..ticks {
            goal.tick(&mut ai);
            assert!(ai.mob.calls.is_empty());
        }
        goal.tick(&mut ai);
        assert_eq!(
            ai.mob.calls,
            vec![
                "modifyTemper(5)",
                "ejectPassengers",
                "makeMad",
                "broadcastEntityEvent(6)"
            ]
        );
        assert_eq!(ai.mob.temper, 5);

        // Temper at the max: `nextInt(100) < 100` always, so it tames.
        let mut ai = ridden_horse(11, true);
        ai.mob.temper = 100;
        for _ in 0..=ticks {
            goal.tick(&mut ai);
        }
        assert_eq!(ai.mob.calls, vec!["tameWithName(9)"]);

        // A non-player rider is thrown without a temper draw.
        let mut ai = ridden_horse(11, false);
        for _ in 0..=ticks {
            goal.tick(&mut ai);
        }
        assert_eq!(
            ai.mob.calls,
            vec!["ejectPassengers", "makeMad", "broadcastEntityEvent(6)"]
        );
    }
}
