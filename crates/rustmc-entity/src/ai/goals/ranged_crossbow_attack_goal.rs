//! `net.minecraft.world.entity.ai.goal.RangedCrossbowAttackGoal<T extends
//! Monster & RangedAttackMob & CrossbowAttackMob>` and its `CrossbowState`
//! (`UNCHARGED, CHARGING, CHARGED, READY_TO_ATTACK`), from the 26.2 jar.
//! Flags `MOVE, LOOK`; `PATHFINDING_DELAY_RANGE = TimeUtil.rangeOfSeconds(1, 2)`
//! = `UniformInt(20, 40)`. Used by Pillager.
//!
//! `canUse()`: `isValidTarget() && isHoldingCrossbow()`; `canContinueToUse()`:
//! `isValidTarget() && (canUse() || !navigation.isDone()) && isHoldingCrossbow()`.
//! `stop()`: `setAggressive(false); setTarget(null); seeTime = 0; if
//! isUsingItem(): stopUsingItem(); setChargingCrossbow(false);
//! getUseItem().set(CHARGED_PROJECTILES, EMPTY)`. Every tick.
//!
//! # `tick()`, offsets 0..461
//!
//! ```text
//! target = getTarget(); if null return
//! los = sensing.hasLineOfSight(target); if los != (seeTime > 0): seeTime = 0
//! if los: ++seeTime else --seeTime
//! d = distanceToSqr(target)
//! close = d <= attackRadiusSqr && seeTime >= 5 && attackDelay == 0
//! if !close:
//!     if --updatePathDelay <= 0:
//!         navigation.moveTo(target, canRun() ? speed : speed * 0.5)
//!         updatePathDelay = PATHFINDING_DELAY_RANGE.sample(random)     -- nextInt(21) + 20, offset 185
//! else: updatePathDelay = 0; navigation.stop()
//! lookControl.setLookAt(target, 30, 30)
//! UNCHARGED:  if !close: startUsingItem(crossbow hand); state = CHARGING; setChargingCrossbow(true)
//! CHARGING:   if !isUsingItem(): state = UNCHARGED
//!             i = getTicksUsingItem(); if i >= CrossbowItem.getChargeDuration(getUseItem(), mob):
//!                 releaseUsingItem(); state = CHARGED; attackDelay = 20 + random.nextInt(20); setChargingCrossbow(false)
//! CHARGED:    if --attackDelay == 0: state = READY_TO_ATTACK
//! READY_TO_ATTACK: if los: performRangedAttack(target, 1.0f); state = UNCHARGED
//! ```
//!
//! `canRun()` is `state == UNCHARGED`. `UniformInt.sample` is
//! `nextInt(max - min + 1) + min`.

use rustmc_worldgen::rng::RandomSource;

use crate::ai::goal::{Flag, FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::host::MobHost;

/// `RangedCrossbowAttackGoal$CrossbowState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrossbowState {
    Uncharged,
    Charging,
    Charged,
    ReadyToAttack,
}

/// `RangedCrossbowAttackGoal`.
#[derive(Debug, Clone)]
pub struct RangedCrossbowAttackGoal {
    crossbow_state: CrossbowState,
    speed_modifier: f64,
    attack_radius_sqr: f32,
    see_time: i32,
    attack_delay: i32,
    update_path_delay: i32,
}

impl RangedCrossbowAttackGoal {
    /// `RangedCrossbowAttackGoal(T, double, float attackRadius)`.
    #[must_use]
    pub fn new(speed_modifier: f64, attack_radius: f32) -> Self {
        RangedCrossbowAttackGoal {
            crossbow_state: CrossbowState::Uncharged,
            speed_modifier,
            attack_radius_sqr: attack_radius * attack_radius,
            see_time: 0,
            attack_delay: 0,
            update_path_delay: 0,
        }
    }

    fn is_valid_target<M: MobHost>(cx: &MobAi<M>) -> bool {
        cx.mob
            .target()
            .and_then(|t| cx.mob.entity(t))
            .is_some_and(|f| f.alive)
    }

    fn can_run(&self) -> bool {
        self.crossbow_state == CrossbowState::Uncharged
    }

    /// The state, for tests.
    #[must_use]
    pub fn state(&self) -> (CrossbowState, i32, i32, i32) {
        (
            self.crossbow_state,
            self.see_time,
            self.attack_delay,
            self.update_path_delay,
        )
    }
}

impl<M: MobHost> Goal<MobAi<M>> for RangedCrossbowAttackGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        Self::is_valid_target(cx) && cx.mob.is_holding_crossbow()
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        Self::is_valid_target(cx)
            && (self.can_use(cx) || !cx.navigation.is_done())
            && cx.mob.is_holding_crossbow()
    }

    fn stop(&mut self, cx: &mut MobAi<M>) {
        cx.mob.set_aggressive(false);
        cx.mob.set_target(None);
        self.see_time = 0;
        if cx.mob.is_using_item() {
            cx.mob.stop_using_item();
            cx.mob.set_charging_crossbow(false);
            cx.mob.clear_charged_projectiles();
        }
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
        let d = f.distance_to_sqr(cx.mob.x(), cx.mob.y(), cx.mob.z());
        let close =
            d <= f64::from(self.attack_radius_sqr) && self.see_time >= 5 && self.attack_delay == 0;
        if !close {
            self.update_path_delay -= 1;
            if self.update_path_delay <= 0 {
                let s = if self.can_run() {
                    self.speed_modifier
                } else {
                    self.speed_modifier * 0.5
                };
                cx.navigation
                    .move_to_entity_at(&mut cx.mob, f.block_position(), s);
                self.update_path_delay = cx.mob.random().next_i32_bound(21) + 20;
            }
        } else {
            self.update_path_delay = 0;
            cx.navigation.stop();
        }
        cx.look_control
            .set_look_at_with(f.x, f.eye_y, f.z, 30.0, 30.0);
        match self.crossbow_state {
            CrossbowState::Uncharged => {
                if !close {
                    cx.mob.start_using_crossbow_hand();
                    self.crossbow_state = CrossbowState::Charging;
                    cx.mob.set_charging_crossbow(true);
                }
            }
            CrossbowState::Charging => {
                if !cx.mob.is_using_item() {
                    self.crossbow_state = CrossbowState::Uncharged;
                }
                let i = cx.mob.ticks_using_item();
                if i >= cx.mob.crossbow_charge_duration() {
                    cx.mob.release_using_item();
                    self.crossbow_state = CrossbowState::Charged;
                    self.attack_delay = 20 + cx.mob.random().next_i32_bound(20);
                    cx.mob.set_charging_crossbow(false);
                }
            }
            CrossbowState::Charged => {
                self.attack_delay -= 1;
                if self.attack_delay == 0 {
                    self.crossbow_state = CrossbowState::ReadyToAttack;
                }
            }
            CrossbowState::ReadyToAttack => {
                if los {
                    cx.mob.perform_ranged_attack(t, 1.0);
                    self.crossbow_state = CrossbowState::Uncharged;
                }
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

    /// A pillager 5 blocks from a visible target: charges on tick 1
    /// (walking at half speed), fires the charged bolt after the 20..39
    /// delay once ready, and returns to uncharged.
    #[test]
    fn charge_delay_fire_cycle() {
        let mut g = GridLevel::new();
        g.floor(-40, -40, 40, 40, 63);
        let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, 3);
        mob.holding_crossbow = true;
        mob.others
            .push((EntityClass::Player, EntityFacts::player(9, 5.5, 64.0, 0.5)));
        mob.target_raw = Some(9);
        mob.visible.push(9);
        let mut ai = MobAi::new(mob);
        let mut goal = RangedCrossbowAttackGoal::new(1.0, 8.0);
        assert!(goal.can_use(&mut ai));
        goal.tick(&mut ai);
        assert_eq!(goal.state().0, CrossbowState::Charging);
        assert!(ai
            .mob
            .calls
            .iter()
            .any(|c| c == "setChargingCrossbow(true)"));
        assert!(ai.navigation.is_in_progress());
        ai.mob.ticks_using_item = 25;
        goal.tick(&mut ai);
        assert_eq!(goal.state().0, CrossbowState::Charged);
        let delay = goal.state().2;
        assert!((20..40).contains(&delay), "{delay}");
        for _ in 0..delay - 1 {
            goal.tick(&mut ai);
            assert_eq!(goal.state().0, CrossbowState::Charged);
        }
        goal.tick(&mut ai);
        assert_eq!(goal.state().0, CrossbowState::ReadyToAttack);
        goal.tick(&mut ai);
        assert!(ai
            .mob
            .calls
            .iter()
            .any(|c| c == "performRangedAttack(9, 1)"));
        assert_eq!(goal.state().0, CrossbowState::Uncharged);
        goal.stop(&mut ai);
        assert_eq!(ai.mob.target_raw, None);
    }
}
