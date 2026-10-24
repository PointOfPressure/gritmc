//! The `Mob` hooks that drive the AI, from the 26.2 jar: `serverAiStep`,
//! `tick`'s control-flag refresh, `updateControlFlags`, `tickHeadTurn`
//! and `lookAt`.
//!
//! # Shape
//!
//! [`MobAi`] owns the mob host and every AI field `Mob` declares:
//! `goalSelector`, `targetSelector`, `sensing`, `lookControl`,
//! `moveControl`, `jumpControl`, `bodyRotationControl`, `navigation` and
//! `noActionTime`. Goals are `Goal<MobAi<M>>`: their context is the whole
//! mob, which is what a Java goal's `this.mob` reaches. While a selector is
//! ticking it is taken out of the struct (`mem::take`) so the goals it
//! runs can borrow everything else; a goal that adds or removes goals
//! from *that* selector during its own tick would see an empty one, which
//! no goal in this lane does and which is recorded here rather than
//! hidden.
//!
//! # `serverAiStep()`, offsets 0..270
//!
//! ```text
//! noActionTime++
//! sensing.tick()
//! i = tickCount + getId()
//! if i % 2 != 0 && tickCount > 1:      (46..57)
//!     targetSelector.tickRunningGoals(false)
//!     goalSelector.tickRunningGoals(false)
//! else:
//!     targetSelector.tick()
//!     goalSelector.tick()
//! navigation.tick()
//! customServerAiStep(level)             (bare return in Mob)
//! moveControl.tick(); lookControl.tick(); jumpControl.tick()
//! ```
//!
//! The every-other-tick gate is keyed on `tickCount + id`, so two mobs
//! spawned in the same tick with consecutive ids alternate; and the first
//! tick (`tickCount <= 1`) always runs the full selector.
//!
//! # `tick()` and `updateControlFlags()`
//!
//! `Mob.tick` calls `updateControlFlags()` every fifth tick
//! (`tickCount % 5 == 0`): `MOVE` and `LOOK` are enabled unless a mob is
//! the controlling passenger, `JUMP` additionally unless the vehicle is a
//! boat.

use super::control::{mob_look_at, BodyRotationControl, JumpControl, LookControl, MoveControl};
use super::goal::Flag;
use super::goal_selector::GoalSelector;
use super::navigation::GroundPathNavigation;
use super::sensing::Sensing;
use crate::host::MobHost;

/// `Mob`'s AI fields, plus the mob they drive.
pub struct MobAi<M: MobHost> {
    /// The mob's own fields, through [`MobHost`].
    pub mob: M,
    /// `goalSelector`.
    pub goal_selector: GoalSelector<MobAi<M>>,
    /// `targetSelector`.
    pub target_selector: GoalSelector<MobAi<M>>,
    /// `sensing`.
    pub sensing: Sensing,
    /// `lookControl`.
    pub look_control: LookControl,
    /// `moveControl`.
    pub move_control: MoveControl,
    /// `jumpControl`.
    pub jump_control: JumpControl,
    /// `bodyRotationControl` (`createBodyControl()` in `Mob`).
    pub body_rotation_control: BodyRotationControl,
    /// `navigation` (`createNavigation(level)` in `Mob`: a
    /// `GroundPathNavigation`).
    pub navigation: GroundPathNavigation,
    /// `noActionTime`.
    pub no_action_time: i32,
}

impl<M: MobHost> MobAi<M> {
    /// The `Mob` constructor's AI part: fresh controls, a ground
    /// navigation sized from the mob's follow range, empty selectors.
    #[must_use]
    pub fn new(mob: M) -> Self {
        let navigation = GroundPathNavigation::new(&mob);
        MobAi {
            mob,
            goal_selector: GoalSelector::new(),
            target_selector: GoalSelector::new(),
            sensing: Sensing::new(),
            look_control: LookControl::new(),
            move_control: MoveControl::new(),
            jump_control: JumpControl::new(),
            body_rotation_control: BodyRotationControl::new(),
            navigation,
            no_action_time: 0,
        }
    }

    /// `serverAiStep()`.
    pub fn server_ai_step(&mut self) {
        self.server_ai_step_with(|_| {});
    }

    /// `serverAiStep()` with the mob's own `customServerAiStep(ServerLevel)`
    /// supplied by the caller, run at offsets 175-196 — after the
    /// navigation tick and before the three controls. `Mob`'s own body is a
    /// bare `return`; `Animal` (the in-love reset), `Bat` (its flight) and
    /// 36 other classes override it, and a host that owns one of those
    /// passes it here rather than subclassing.
    pub fn server_ai_step_with(&mut self, custom: impl FnOnce(&mut Self)) {
        self.no_action_time += 1;
        self.sensing.tick();
        let i = self.mob.tick_count() + self.mob.id();
        if i % 2 != 0 && self.mob.tick_count() > 1 {
            let mut ts = std::mem::take(&mut self.target_selector);
            ts.tick_running_goals(self, false);
            self.target_selector = ts;
            let mut gs = std::mem::take(&mut self.goal_selector);
            gs.tick_running_goals(self, false);
            self.goal_selector = gs;
        } else {
            let mut ts = std::mem::take(&mut self.target_selector);
            ts.tick(self);
            self.target_selector = ts;
            let mut gs = std::mem::take(&mut self.goal_selector);
            gs.tick(self);
            self.goal_selector = gs;
        }
        self.navigation.tick(&mut self.mob, &mut self.move_control);
        self.custom_server_ai_step();
        custom(self);
        self.move_control
            .tick(&mut self.mob, &self.navigation, &mut self.jump_control);
        let nav_done = self.navigation.is_done();
        self.look_control.tick(&mut self.mob, nav_done);
        self.jump_control.tick(&mut self.mob);
    }

    /// `customServerAiStep(ServerLevel)`: a bare `return` in `Mob`.
    /// Subclasses that override it are per-kind behaviour, out of scope.
    fn custom_server_ai_step(&mut self) {}

    /// The AI part of `Mob.tick()`: `if (tickCount % 5 == 0) updateControlFlags()`.
    /// The caller has already run `LivingEntity.tick()` and incremented
    /// `tickCount` for the tick.
    pub fn tick(&mut self) {
        if self.mob.tick_count() % 5 == 0 {
            self.update_control_flags();
        }
    }

    /// `updateControlFlags()`, offsets 0..77.
    pub fn update_control_flags(&mut self) {
        let no_mob_driver = !self.mob.controlling_passenger_is_mob();
        let not_in_boat = !self.mob.vehicle_is_boat();
        self.goal_selector
            .set_control_flag(Flag::Move, no_mob_driver);
        self.goal_selector
            .set_control_flag(Flag::Jump, no_mob_driver && not_in_boat);
        self.goal_selector
            .set_control_flag(Flag::Look, no_mob_driver);
    }

    /// `tickHeadTurn(float)`: `bodyRotationControl.clientTick()`.
    pub fn tick_head_turn(&mut self) {
        self.body_rotation_control.client_tick(&mut self.mob);
    }

    /// `lookAt(Entity, float, float)`; the target's coordinates are
    /// passed in, `y_for_pitch` being its eye y for a living entity and
    /// its box's mid-height otherwise.
    pub fn look_at(&mut self, x: f64, y_for_pitch: f64, z: f64, max_yaw: f32, max_pitch: f32) {
        mob_look_at(&mut self.mob, x, y_for_pitch, z, max_yaw, max_pitch);
    }

    /// `getNoActionTime()`.
    #[must_use]
    pub fn no_action_time(&self) -> i32 {
        self.no_action_time
    }

    /// `setNoActionTime(int)` (the `LivingEntity` setter goals reset
    /// through).
    pub fn set_no_action_time(&mut self, v: i32) {
        self.no_action_time = v;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::goal::{FlagSet, Goal};
    use crate::testing::{GridLevel, TestMob};
    use std::cell::RefCell;
    use std::rc::Rc;

    struct Spy(Rc<RefCell<Vec<String>>>, &'static str);
    impl Goal<MobAi<TestMob>> for Spy {
        fn can_use(&mut self, _: &mut MobAi<TestMob>) -> bool {
            self.0.borrow_mut().push(format!("{}.canUse", self.1));
            true
        }
        fn start(&mut self, _: &mut MobAi<TestMob>) {
            self.0.borrow_mut().push(format!("{}.start", self.1));
        }
        fn tick(&mut self, _: &mut MobAi<TestMob>) {
            self.0.borrow_mut().push(format!("{}.tick", self.1));
        }
        fn flags(&self) -> FlagSet {
            FlagSet::NONE
        }
    }

    fn make_ai(id: i32, tick_count: i32) -> (MobAi<TestMob>, Rc<RefCell<Vec<String>>>) {
        let mut g = GridLevel::new();
        g.floor(-4, -4, 4, 4, 63);
        let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, 1);
        mob.id = id;
        mob.tick_count = tick_count;
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut ai = MobAi::new(mob);
        ai.target_selector
            .add_goal(0, Box::new(Spy(log.clone(), "T")));
        ai.goal_selector
            .add_goal(0, Box::new(Spy(log.clone(), "G")));
        (ai, log)
    }

    /// `(tickCount + id) % 2 == 0`: a full tick, target selector first.
    #[test]
    fn an_even_tick_runs_both_selectors_in_full() {
        let (mut ai, log) = make_ai(1, 3);
        ai.server_ai_step();
        assert_eq!(
            *log.borrow(),
            vec!["T.canUse", "T.start", "T.tick", "G.canUse", "G.start", "G.tick"]
        );
        assert_eq!(ai.no_action_time(), 1);
    }

    /// An odd sum past the first tick only ticks running goals, and a
    /// not-yet-started goal is not started.
    #[test]
    fn an_odd_tick_only_ticks_running_goals() {
        let (mut ai, log) = make_ai(2, 3);
        ai.server_ai_step();
        assert!(log.borrow().is_empty());
        // The first tick of a mob's life is always a full tick, whatever
        // the parity.
        let (mut ai, log) = make_ai(2, 1);
        ai.server_ai_step();
        assert_eq!(log.borrow().len(), 6);
    }

    /// The control flags follow the passenger and vehicle facts on every
    /// fifth tick.
    #[test]
    fn update_control_flags_follows_the_driver_and_the_boat() {
        let (mut ai, _) = make_ai(1, 5);
        ai.mob.controlling_passenger_is_mob = true;
        ai.tick();
        let d = ai.goal_selector.disabled_flags();
        assert!(d.contains(Flag::Move) && d.contains(Flag::Look) && d.contains(Flag::Jump));
        ai.mob.controlling_passenger_is_mob = false;
        ai.mob.vehicle_is_boat = true;
        ai.mob.tick_count = 6;
        ai.tick();
        let d = ai.goal_selector.disabled_flags();
        assert!(d.contains(Flag::Move), "not refreshed off the fifth tick");
        ai.mob.tick_count = 10;
        ai.tick();
        let d = ai.goal_selector.disabled_flags();
        assert!(!d.contains(Flag::Move) && !d.contains(Flag::Look) && d.contains(Flag::Jump));
    }

    /// `lookAt` moves both rotations through `Mob.rotlerp`, which does not
    /// fold into `[0, 360]`.
    #[test]
    fn look_at_turns_yaw_and_pitch_within_the_limits() {
        let (mut ai, _) = make_ai(1, 2);
        // Target due +x from (0.5, 64, 0.5): yaw = atan2(0, 1) * deg - 90 = -90.
        ai.look_at(10.5, 64.0 + 1.74, 0.5, 30.0, 30.0);
        assert_eq!(ai.mob.y_rot, -30.0);
        assert_eq!(ai.mob.x_rot, 0.0);
        ai.look_at(10.5, 64.0 + 1.74, 0.5, 90.0, 30.0);
        assert_eq!(ai.mob.y_rot, -90.0);
    }
}
