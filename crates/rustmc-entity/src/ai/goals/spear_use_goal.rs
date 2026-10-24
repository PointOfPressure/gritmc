//! `net.minecraft.world.entity.ai.goal.SpearUseGoal<T extends Monster>` and
//! its `SpearUseState`, from the 26.2 jar. Flags `MOVE, LOOK`.
//! `MIN/MAX_REPOSITION_DISTANCE = 6, 7`, `MIN/MAX_COOLDOWN_DISTANCE = 9, 11`,
//! `MAX_FLEEING_TIME = (double) reducedTickDelay(100)` = 50. Used by
//! Ravager riders (the table names the mob).
//!
//! `ableToAttack()`: a target and `getMainHandItem().has(KINETIC_WEAPON)`.
//! `canUse()`: `ableToAttack() && !isUsingItem()`. `canContinueToUse()`:
//! `state != null && !state.done && ableToAttack()`. `start()`:
//! `setAggressive(true); state = new SpearUseState()` (engageTime -1,
//! fleeingTime -1). `stop()`: `navigation.stop(); setAggressive(false);
//! state = null; stopUsingItem()`.
//!
//! # `tick()`, offsets 0..453
//!
//! ```text
//! target = getTarget(); d = distanceToSqr(target)
//! f = getRootVehicle() is a Mob ? vehicle.chargeSpeedModifier() : 1.0f
//! g = isPassenger() ? 2 : 0
//! mob.lookAt(target, 30, 30); lookControl.setLookAt(target, 30, 30)
//! if state.notEngagedYet():
//!     if d > approachDistanceSq: navigation.moveTo(target, f * speedWhenRepositioning); return
//!     state.startEngagement(getKineticWeaponUseDuration()); startUsingItem(MAIN_HAND)
//! if state.tickAndCheckEngagement():
//!     stopUsingItem(); s = sqrt(d)
//!     state.awayPos = LandRandomPos.getPosAway(mob, max(0, 9 + g - s), max(1, 11 + g - s), 7, target.position())
//!     state.fleeingTime = 1
//! if state.tickAndCheckFleeing() return
//! if state.awayPos != null:
//!     navigation.moveTo(awayPos, f * speedWhenRepositioning)
//!     if navigation.isDone(): if state.fleeingTime > 0: state.done = true; return
//!                             else state.awayPos = null
//! else:
//!     navigation.moveTo(target, f * speedWhenCharging)
//!     if d < targetInRangeRadiusSq || navigation.isDone():
//!         s = sqrt(d); state.awayPos = LandRandomPos.getPosAway(mob, 6 + g - s, 7 + g - s, 7, target.position())
//! ```
//!
//! `getKineticWeaponUseDuration()` is `reducedTickDelay(component.useDuration)`.
//! `tickAndCheckEngagement`: while `engageTime > 0`, decrement and answer
//! true on reaching 0. `tickAndCheckFleeing`: while `fleeingTime > 0`,
//! increment and, past `MAX_FLEEING_TIME`, set `done` and answer true.

use crate::ai::goal::{reduced_tick_delay, Flag, FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::ai::random_pos::land_get_pos_away_dist;
use crate::host::MobHost;
use crate::phys::Vec3;

/// `MAX_FLEEING_TIME`: `(double) reducedTickDelay(100)`.
pub const MAX_FLEEING_TIME: f64 = 50.0;

/// `SpearUseGoal$SpearUseState`.
#[derive(Debug, Clone)]
pub struct SpearUseState {
    engage_time: i32,
    fleeing_time: i32,
    away_pos: Option<Vec3>,
    done: bool,
}

impl SpearUseState {
    fn new() -> Self {
        SpearUseState {
            engage_time: -1,
            fleeing_time: -1,
            away_pos: None,
            done: false,
        }
    }

    fn not_engaged_yet(&self) -> bool {
        self.engage_time < 0
    }

    fn start_engagement(&mut self, n: i32) {
        self.engage_time = n;
    }

    fn tick_and_check_engagement(&mut self) -> bool {
        if self.engage_time > 0 {
            self.engage_time -= 1;
            if self.engage_time == 0 {
                return true;
            }
        }
        false
    }

    fn tick_and_check_fleeing(&mut self) -> bool {
        if self.fleeing_time > 0 {
            self.fleeing_time += 1;
            if f64::from(self.fleeing_time) > MAX_FLEEING_TIME {
                self.done = true;
                return true;
            }
        }
        false
    }
}

/// `SpearUseGoal`.
#[derive(Debug, Clone)]
pub struct SpearUseGoal {
    state: Option<SpearUseState>,
    speed_modifier_when_charging: f64,
    speed_modifier_when_repositioning: f64,
    approach_distance_sq: f32,
    target_in_range_radius_sq: f32,
}

impl SpearUseGoal {
    /// `SpearUseGoal(T, double charging, double repositioning, float approachDistance, float targetInRangeRadius)`.
    #[must_use]
    pub fn new(
        speed_modifier_when_charging: f64,
        speed_modifier_when_repositioning: f64,
        approach_distance: f32,
        target_in_range_radius: f32,
    ) -> Self {
        SpearUseGoal {
            state: None,
            speed_modifier_when_charging,
            speed_modifier_when_repositioning,
            approach_distance_sq: approach_distance * approach_distance,
            target_in_range_radius_sq: target_in_range_radius * target_in_range_radius,
        }
    }

    fn able_to_attack<M: MobHost>(cx: &MobAi<M>) -> bool {
        cx.mob.target().is_some() && cx.mob.main_hand_is_kinetic_weapon()
    }

    /// The state, for tests: `(engageTime, fleeingTime, awayPos set, done)`.
    #[must_use]
    pub fn state(&self) -> Option<(i32, i32, bool, bool)> {
        self.state
            .as_ref()
            .map(|s| (s.engage_time, s.fleeing_time, s.away_pos.is_some(), s.done))
    }
}

impl<M: MobHost> Goal<MobAi<M>> for SpearUseGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        Self::able_to_attack(cx) && !cx.mob.is_using_item()
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        self.state.as_ref().is_some_and(|s| !s.done) && Self::able_to_attack(cx)
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        cx.mob.set_aggressive(true);
        self.state = Some(SpearUseState::new());
    }

    fn stop(&mut self, cx: &mut MobAi<M>) {
        cx.navigation.stop();
        cx.mob.set_aggressive(false);
        self.state = None;
        cx.mob.stop_using_item();
    }

    fn tick(&mut self, cx: &mut MobAi<M>) {
        let Some(state) = self.state.as_mut() else {
            return;
        };
        let Some(t) = cx.mob.target() else {
            return;
        };
        let Some(f) = cx.mob.entity(t) else {
            return;
        };
        let d = f.distance_to_sqr(cx.mob.x(), cx.mob.y(), cx.mob.z());
        let speed = f64::from(cx.mob.root_vehicle_charge_speed_modifier().unwrap_or(1.0));
        let g = if cx.mob.is_passenger() { 2 } else { 0 };
        cx.look_at(f.x, f.eye_y, f.z, 30.0, 30.0);
        cx.look_control
            .set_look_at_with(f.x, f.eye_y, f.z, 30.0, 30.0);
        if state.not_engaged_yet() {
            if d > f64::from(self.approach_distance_sq) {
                let s = speed * self.speed_modifier_when_repositioning;
                cx.navigation
                    .move_to_entity_at(&mut cx.mob, f.block_position(), s);
                return;
            }
            state.start_engagement(reduced_tick_delay(cx.mob.kinetic_weapon_use_duration()));
            cx.mob.start_using_main_hand();
        }
        if state.tick_and_check_engagement() {
            cx.mob.stop_using_item();
            let s = d.sqrt();
            let min = rustmc_worldgen::math::java_max(0.0, f64::from(9 + g) - s);
            let max = rustmc_worldgen::math::java_max(1.0, f64::from(11 + g) - s);
            state.away_pos = land_get_pos_away_dist(cx, min, max, 7, f.position());
            state.fleeing_time = 1;
        }
        if state.tick_and_check_fleeing() {
            return;
        }
        if let Some(away) = state.away_pos {
            let s = speed * self.speed_modifier_when_repositioning;
            cx.navigation
                .move_to(&mut cx.mob, away.x, away.y, away.z, s);
            if cx.navigation.is_done() {
                if state.fleeing_time > 0 {
                    state.done = true;
                    return;
                }
                state.away_pos = None;
            }
        } else {
            let s = speed * self.speed_modifier_when_charging;
            cx.navigation
                .move_to_entity_at(&mut cx.mob, f.block_position(), s);
            if d < f64::from(self.target_in_range_radius_sq) || cx.navigation.is_done() {
                let sd = d.sqrt();
                state.away_pos = land_get_pos_away_dist(
                    cx,
                    f64::from(6 + g) - sd,
                    f64::from(7 + g) - sd,
                    7,
                    f.position(),
                );
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

    /// Within the approach distance the spear is raised for the halved
    /// use duration, then dropped; the state then flees for at most 50
    /// ticks before the goal is done.
    #[test]
    fn engages_then_flees() {
        let mut g = GridLevel::new();
        g.floor(-40, -40, 40, 40, 63);
        let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, 3);
        mob.kinetic_weapon = Some(10);
        mob.others
            .push((EntityClass::Player, EntityFacts::player(9, 2.5, 64.0, 0.5)));
        mob.target_raw = Some(9);
        let mut ai = MobAi::new(mob);
        let mut goal = SpearUseGoal::new(1.0, 1.0, 4.0, 2.0);
        assert!(goal.can_use(&mut ai));
        goal.start(&mut ai);
        goal.tick(&mut ai);
        assert!(ai.mob.using_item, "engaged: using the spear");
        assert_eq!(
            goal.state().map(|s| s.0),
            Some(4),
            "reducedTickDelay(10) - 1"
        );
        for _ in 0..4 {
            goal.tick(&mut ai);
        }
        assert!(!ai.mob.using_item, "released at 0");
        assert_eq!(goal.state().map(|s| s.1 > 0), Some(true), "fleeing");
        let mut n = 0;
        while goal.can_continue_to_use(&mut ai) && n < 200 {
            goal.tick(&mut ai);
            n += 1;
        }
        assert!(n <= 60, "done within the fleeing cap: {n}");
        goal.stop(&mut ai);
        assert!(goal.state().is_none());
    }
}
