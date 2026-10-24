//! `net.minecraft.world.entity.ai.behavior.Croak`, from the 26.2 jar.
//! `Behavior<Frog>` with entry `{WALK_TARGET: VALUE_ABSENT}` and a fixed
//! duration of `TIME_OUT_DURATION = 100`.
//!
//! ```text
//! CROAK_TICKS = 60
//! checkExtraStartConditions (0..15): frog.getPose() == Pose.STANDING
//! canStillUse (0..14):               croakCounter < 60
//! start (0..17):  if !frog.isInLiquid(): frog.setPose(Pose.CROAKING)
//!                 croakCounter = 0
//! tick (0..7):    croakCounter++
//! stop (0..4):    frog.setPose(Pose.STANDING)
//! ```
//!
//! **The 100-tick duration is never reached**, because `canStillUse` ends
//! it at 60 counted ticks — so `TIME_OUT_DURATION` is a ceiling that only
//! matters if something stops ticking the behaviour.
//!
//! **`start` sets the pose only out of liquid, and `stop` restores
//! `STANDING` unconditionally.** A frog that starts croaking in water
//! never enters `Pose.CROAKING` and is still snapped to `STANDING` on
//! stop, which is the one asymmetry in the class.
//!
//! `croakCounter` is a plain field, reset by `start`, so a re-start after
//! a stop croaks for a fresh 60.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::memory::{MemoryModuleType, MemoryStatus};
use crate::brain::BrainState;
use crate::host::{MobHost, Pose};

/// `CROAK_TICKS`.
pub const CROAK_TICKS: i32 = 60;
/// `TIME_OUT_DURATION`.
pub const TIME_OUT_DURATION: i32 = 100;

/// `Croak`'s body.
#[derive(Debug, Clone, Copy, Default)]
pub struct Croak {
    /// `croakCounter`.
    pub croak_counter: i32,
}

impl Croak {
    /// `Croak()`.
    #[must_use]
    pub fn new() -> Behavior<Croak> {
        Behavior::with_duration(
            Croak::default(),
            vec![(MemoryModuleType::WalkTarget, MemoryStatus::ValueAbsent)],
            TIME_OUT_DURATION,
        )
    }
}

impl<M: MobHost> BehaviorBody<M> for Croak {
    fn check_extra_start_conditions(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        cx.mob.has_pose(Pose::Standing)
    }

    fn can_still_use(&mut self, _brain: &mut BrainState, _cx: &mut MobAi<M>, _time: i64) -> bool {
        self.croak_counter < CROAK_TICKS
    }

    fn start(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        if !cx.mob.is_in_liquid() {
            cx.mob.set_pose(Pose::Croaking);
        }
        self.croak_counter = 0;
    }

    fn tick(&mut self, _brain: &mut BrainState, _cx: &mut MobAi<M>, _time: i64) {
        self.croak_counter += 1;
    }

    fn stop(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        cx.mob.set_pose(Pose::Standing);
    }

    fn name(&self) -> &'static str {
        "Croak"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::{BehaviorControl, Status};
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn sixty_counted_ticks_end_it_well_inside_the_hundred_tick_duration() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut brain = Brain::<TestMob>::new(&[T::WalkTarget], vec![]).state;
        let mut b = Croak::new();
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(b.end_timestamp(), i64::from(TIME_OUT_DURATION));
        assert_eq!(cx.mob.pose, Pose::Croaking);
        for t in 1..=CROAK_TICKS {
            b.tick_or_stop(&mut brain, &mut cx, i64::from(t));
        }
        assert_eq!(b.body.croak_counter, CROAK_TICKS);
        assert_eq!(BehaviorControl::<TestMob>::status(&b), Status::Running);
        b.tick_or_stop(&mut brain, &mut cx, i64::from(CROAK_TICKS) + 1);
        assert_eq!(
            BehaviorControl::<TestMob>::status(&b),
            Status::Stopped,
            "canStillUse is `< 60`, so the 61st tick ends it"
        );
        assert_eq!(cx.mob.pose, Pose::Standing);
    }

    #[test]
    fn in_liquid_it_never_takes_the_croaking_pose_but_stop_still_stands_it_up() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut brain = Brain::<TestMob>::new(&[T::WalkTarget], vec![]).state;
        let mut b = Croak::new();
        cx.mob.in_liquid = true;
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(cx.mob.pose, Pose::Standing, "no pose in liquid");
        cx.mob.pose = Pose::Sitting;
        b.do_stop(&mut brain, &mut cx, 1);
        assert_eq!(cx.mob.pose, Pose::Standing, "stop is unconditional");
        // A non-standing pose refuses the start outright.
        cx.mob.pose = Pose::Croaking;
        assert!(!b.try_start(&mut brain, &mut cx, 2));
    }
}
