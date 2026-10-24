//! `net.minecraft.world.entity.ai.goal.RangedBowAttackGoal<T extends Monster
//! & RangedAttackMob>`, from the 26.2 jar. Flags `MOVE, LOOK`. Used by
//! AbstractSkeleton (and Illusioner).
//!
//! `attackTime = -1`, `strafingTime = -1`, `attackRadiusSqr = r * r`
//! (float). `canUse()`: `getTarget() != null && isHoldingBow()`
//! (`isHolding(Items.BOW)`). `canContinueToUse()`: `(canUse() ||
//! !navigation.isDone()) && isHoldingBow()`. `start()`: `setAggressive(true)`.
//! `stop()`: `setAggressive(false); seeTime = 0; attackTime = -1; stopUsingItem()`.
//!
//! # `tick()`, offsets 0..529
//!
//! ```text
//! target = getTarget(); if null return
//! d = distanceToSqr(target.x, target.y, target.z); los = sensing.hasLineOfSight(target)
//! if los != (seeTime > 0): seeTime = 0
//! if los: ++seeTime else --seeTime
//! if d <= attackRadiusSqr && seeTime >= 20: navigation.stop(); ++strafingTime
//! else: navigation.moveTo(target, speed); strafingTime = -1
//! if strafingTime >= 20:
//!     if random.nextFloat() < 0.3: strafingClockwise = !strafingClockwise      (offset 179)
//!     if random.nextFloat() < 0.3: strafingBackwards = !strafingBackwards      (offset 215)
//!     strafingTime = 0
//! if strafingTime > -1:
//!     if d > attackRadiusSqr * 0.75f: strafingBackwards = false
//!     else if d < attackRadiusSqr * 0.25f: strafingBackwards = true
//!     moveControl.strafe(strafingBackwards ? -0.5 : 0.5, strafingClockwise ? 0.5 : -0.5)
//!     if getControlledVehicle() is a Mob: vehicle.lookAt(target, 30, 30)
//!     mob.lookAt(target, 30, 30)
//! else: lookControl.setLookAt(target, 30, 30)
//! if isUsingItem():
//!     if !los && seeTime < -60: stopUsingItem()
//!     else if los: i = getTicksUsingItem(); if i >= 20: stopUsingItem();
//!         performRangedAttack(target, BowItem.getPowerForTime(i)); attackTime = attackIntervalMin
//! else if --attackTime <= 0 && seeTime >= -60: startUsingItem(getWeaponHoldingHand(BOW))
//! ```
//!
//! `BowItem.getPowerForTime(i)`: `f = i / 20.0f; f = (f * f + f * 2.0f) / 3.0f; min(f, 1.0f)`.

use rustmc_worldgen::rng::RandomSource;

use crate::ai::goal::{Flag, FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::host::MobHost;

/// `BowItem.getPowerForTime(int)`.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn bow_power_for_time(i: i32) -> f32 {
    let mut f = i as f32 / 20.0;
    f = (f * f + f * 2.0) / 3.0;
    if f > 1.0 {
        f = 1.0;
    }
    f
}

/// `RangedBowAttackGoal`.
#[derive(Debug, Clone)]
pub struct RangedBowAttackGoal {
    speed_modifier: f64,
    attack_interval_min: i32,
    attack_radius_sqr: f32,
    attack_time: i32,
    see_time: i32,
    strafing_clockwise: bool,
    strafing_backwards: bool,
    strafing_time: i32,
}

impl RangedBowAttackGoal {
    /// `RangedBowAttackGoal(T, double, int attackIntervalMin, float attackRadius)`.
    #[must_use]
    pub fn new(speed_modifier: f64, attack_interval_min: i32, attack_radius: f32) -> Self {
        RangedBowAttackGoal {
            speed_modifier,
            attack_interval_min,
            attack_radius_sqr: attack_radius * attack_radius,
            attack_time: -1,
            see_time: 0,
            strafing_clockwise: false,
            strafing_backwards: false,
            strafing_time: -1,
        }
    }

    /// `setMinAttackInterval(int)`.
    pub fn set_min_attack_interval(&mut self, n: i32) {
        self.attack_interval_min = n;
    }

    /// `attackTime` / `seeTime` / `strafingTime`, for tests.
    #[must_use]
    pub fn state(&self) -> (i32, i32, i32) {
        (self.attack_time, self.see_time, self.strafing_time)
    }
}

impl<M: MobHost> Goal<MobAi<M>> for RangedBowAttackGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        cx.mob.target().is_some() && cx.mob.is_holding_bow()
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        (self.can_use(cx) || !cx.navigation.is_done()) && cx.mob.is_holding_bow()
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        cx.mob.set_aggressive(true);
    }

    fn stop(&mut self, cx: &mut MobAi<M>) {
        cx.mob.set_aggressive(false);
        self.see_time = 0;
        self.attack_time = -1;
        cx.mob.stop_using_item();
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn tick(&mut self, cx: &mut MobAi<M>) {
        let Some(t) = cx.mob.target() else {
            return;
        };
        let Some(f) = cx.mob.entity(t) else {
            return;
        };
        let d = f.distance_to_sqr(cx.mob.x(), cx.mob.y(), cx.mob.z());
        let los = {
            let MobAi { mob, sensing, .. } = cx;
            sensing.has_line_of_sight(t, || mob.has_line_of_sight_to(t))
        };
        if los != (self.see_time > 0) {
            self.see_time = 0;
        }
        if los {
            self.see_time += 1;
        } else {
            self.see_time -= 1;
        }
        if d <= f64::from(self.attack_radius_sqr) && self.see_time >= 20 {
            cx.navigation.stop();
            self.strafing_time += 1;
        } else {
            let s = self.speed_modifier;
            cx.navigation
                .move_to_entity_at(&mut cx.mob, f.block_position(), s);
            self.strafing_time = -1;
        }
        if self.strafing_time >= 20 {
            if f64::from(cx.mob.random().next_f32()) < 0.3 {
                self.strafing_clockwise = !self.strafing_clockwise;
            }
            if f64::from(cx.mob.random().next_f32()) < 0.3 {
                self.strafing_backwards = !self.strafing_backwards;
            }
            self.strafing_time = 0;
        }
        if self.strafing_time > -1 {
            if d > f64::from(self.attack_radius_sqr * 0.75) {
                self.strafing_backwards = false;
            } else if d < f64::from(self.attack_radius_sqr * 0.25) {
                self.strafing_backwards = true;
            }
            cx.move_control.strafe(
                if self.strafing_backwards { -0.5 } else { 0.5 },
                if self.strafing_clockwise { 0.5 } else { -0.5 },
            );
            if let Some(v) = cx.mob.controlled_vehicle_mob() {
                cx.mob.other_mob_look_at(v, f.x, f.eye_y, f.z, 30.0, 30.0);
            }
            cx.look_at(f.x, f.eye_y, f.z, 30.0, 30.0);
        } else {
            cx.look_control
                .set_look_at_with(f.x, f.eye_y, f.z, 30.0, 30.0);
        }
        if cx.mob.is_using_item() {
            if !los && self.see_time < -60 {
                cx.mob.stop_using_item();
            } else if los {
                let i = cx.mob.ticks_using_item();
                if i >= 20 {
                    cx.mob.stop_using_item();
                    cx.mob.perform_ranged_attack(t, bow_power_for_time(i));
                    self.attack_time = self.attack_interval_min;
                }
            }
        } else {
            self.attack_time -= 1;
            if self.attack_time <= 0 && self.see_time >= -60 {
                cx.mob.start_using_bow_hand();
            }
        }
    }

    fn flags(&self) -> FlagSet {
        FlagSet::of2(Flag::Move, Flag::Look)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn bow_power_curve() {
        assert_eq!(bow_power_for_time(0), 0.0);
        assert_eq!(bow_power_for_time(10), (0.25 + 1.0) / 3.0);
        assert_eq!(bow_power_for_time(20), 1.0);
        assert_eq!(bow_power_for_time(40), 1.0);
    }

    /// A skeleton (`1.0, 20, 15.0f`) with a target 5 blocks off in
    /// sight: it draws on the first tick (attackTime -1 -> -2 <= 0),
    /// fires when the host reports 20 ticks of use, and after 20 seen
    /// ticks stops walking and strafes.
    #[test]
    fn draws_fires_and_strafes() {
        let mut g = GridLevel::new();
        g.floor(-40, -40, 40, 40, 63);
        let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, 3);
        mob.on_ground = true;
        mob.holding_bow = true;
        mob.others
            .push((EntityClass::Player, EntityFacts::player(9, 5.5, 64.0, 0.5)));
        mob.target_raw = Some(9);
        mob.visible.push(9);
        let mut ai = MobAi::new(mob);
        let mut goal = RangedBowAttackGoal::new(1.0, 20, 15.0);
        assert!(goal.can_use(&mut ai));
        goal.start(&mut ai);
        goal.tick(&mut ai);
        assert!(ai.mob.using_item, "started drawing");
        assert_eq!(goal.state(), (-2, 1, -1));
        ai.mob.ticks_using_item = 20;
        goal.tick(&mut ai);
        assert!(ai
            .mob
            .calls
            .iter()
            .any(|c| c == "performRangedAttack(9, 1)"));
        assert_eq!(goal.state().0, 20);
        for _ in 0..18 {
            goal.tick(&mut ai);
        }
        assert_eq!(
            goal.state(),
            (2, 20, 0),
            "twenty seen ticks: strafing begins"
        );
        assert!(ai.navigation.is_done());
        goal.stop(&mut ai);
        assert_eq!(goal.state(), (-1, 0, 0));
    }
}
