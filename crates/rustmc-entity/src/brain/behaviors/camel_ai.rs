//! `net.minecraft.world.entity.animal.camel.CamelAi$CamelPanic` and
//! `$RandomSitting`, and `monster.creaking.CreakingAi$1`, from the 26.2 jar.
//!
//! ```text
//! CamelAi$CamelPanic extends AnimalPanic<Camel>
//!   <init>(float) (0..5):  super(speedMultiplier)
//!   checkExtraStartConditions(level, camel) (0..21):
//!        super.checkExtraStartConditions(level, camel) && !camel.isMobControlled()
//!   start(level, camel, t) (0..11):
//!        0  camel.standUpInstantly()
//!        4  super.start(level, camel, t)
//!
//! CamelAi$RandomSitting extends Behavior<Camel>
//!   field: final int minimalPoseTicks
//!   <init>(int seconds) (0..15):
//!        super(ImmutableMap.of())          // -> (60, 60)
//!        minimalPoseTicks = seconds * 20
//!   checkExtraStartConditions (0..53):
//!        !camel.isInWater()
//!        && camel.getPoseTime() >= (long) minimalPoseTicks
//!        && !camel.isLeashed()
//!        && camel.onGround()
//!        && !camel.hasControllingPassenger()
//!        && camel.canCamelChangePose()
//!   start (0..25):
//!        0  if camel.isCamelSitting(): camel.standUp()
//!       14  else if !camel.isPanicking(): camel.sitDown()
//!
//! CreakingAi$1 extends Swim<Creaking>
//!   <init>(float) (0..5):  super(chance)
//!   checkExtraStartConditions(level, creaking) (0..21):
//!        creaking.canMove() && super.checkExtraStartConditions(level, creaking)
//! ```
//!
//! **`RandomSitting`'s constructor takes SECONDS, not ticks**, and multiplies
//! by 20 at offset 9. `getPoseTime()` is compared `>=` against that product,
//! so a camel that has just changed pose cannot change again until the whole
//! window has passed.
//!
//! **`RandomSitting` has an empty entry map and no `canStillUse` override**,
//! so the base's `false` stands: it runs for exactly one brain tick and its
//! `ImmutableMap.of()` 60-tick default duration is never reached. It is a
//! one-shot written as a `Behavior`, like the breeze's `ShootWhenStuck` and
//! `Slide`.
//!
//! **The two branches of `start` are not symmetric.** Standing up is
//! unconditional; sitting down is refused while `isPanicking()`. A panicking
//! camel that happens to be sitting therefore *stands*, which is what makes
//! `CamelPanic.start`'s `standUpInstantly()` a belt-and-braces rather than
//! the only path out of a sit.
//!
//! **`CamelPanic` narrows its superclass rather than widening it:** the
//! `&&` at offset 6 runs `AnimalPanic.checkExtraStartConditions` FIRST and
//! only then asks `isMobControlled()`, so a ridden camel is refused after
//! the superclass has already looked at its damage source.
//!
//! **`CreakingAi$1` narrows `Swim` the other way round** — `canMove()` is
//! asked first, at offset 0, and `Swim`'s own condition second. `Swim`'s
//! condition draws (`random.nextFloat() < chance`), so the order decides
//! whether a creaking that cannot move consumes a draw. It does not.
//!
//! **`CreakingAi$1` and `CamelAi$CamelPanic` differ in one more way worth
//! keeping straight:** `CreakingAi$1` is anonymous, so its `debugString()`
//! is the empty string; `CamelPanic` and `RandomSitting` are named inner
//! classes and answer their own simple names.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::behaviors::animal_panic::AnimalPanic;
use crate::brain::behaviors::swim::Swim;
use crate::brain::memory::MemoryModuleType;
use crate::brain::BrainState;
use crate::host::MobHost;

/// `CamelAi$CamelPanic`'s body.
#[derive(Debug, Clone, Copy)]
pub struct CamelPanic(pub AnimalPanic);

/// `new CamelAi$CamelPanic(speedMultiplier)`.
#[must_use]
pub fn camel_panic(speed_multiplier: f32) -> Behavior<CamelPanic> {
    AnimalPanic::new(speed_multiplier).map_body(CamelPanic)
}

impl<M: MobHost> BehaviorBody<M> for CamelPanic {
    fn check_extra_start_conditions(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        self.0.check_extra_start_conditions(brain, cx) && !cx.mob.is_mob_controlled()
    }

    fn can_still_use(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) -> bool {
        self.0.can_still_use(brain, cx, time)
    }

    fn start(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        cx.mob.camel_stand_up_instantly();
        self.0.start(brain, cx, time);
    }

    fn tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        self.0.tick(brain, cx, time);
    }

    fn stop(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        self.0.stop(brain, cx, time);
    }

    fn timed_out(&self, end_timestamp: i64, time: i64) -> bool {
        <AnimalPanic as BehaviorBody<M>>::timed_out(&self.0, end_timestamp, time)
    }

    fn name(&self) -> &'static str {
        "CamelPanic"
    }
}

/// `CamelAi$RandomSitting`'s body.
#[derive(Debug, Clone, Copy, Default)]
pub struct RandomSitting {
    /// `minimalPoseTicks`, already the constructor's `seconds * 20`.
    pub minimal_pose_ticks: i32,
}

impl RandomSitting {
    /// `new CamelAi$RandomSitting(seconds)` — the argument is seconds and
    /// the field is `seconds * 20`.
    #[must_use]
    pub fn new(seconds: i32) -> Behavior<RandomSitting> {
        Behavior::new(
            RandomSitting {
                minimal_pose_ticks: seconds * 20,
            },
            Vec::new(),
        )
    }
}

impl<M: MobHost> BehaviorBody<M> for RandomSitting {
    fn check_extra_start_conditions(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        !cx.mob.is_in_water()
            && cx.mob.camel_pose_time() >= i64::from(self.minimal_pose_ticks)
            && !cx.mob.is_leashed()
            && cx.mob.on_ground()
            && !cx.mob.has_controlling_passenger()
            && cx.mob.camel_can_change_pose()
    }

    fn start(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        if cx.mob.camel_is_sitting() {
            cx.mob.camel_stand_up();
        } else if !brain.has_memory_value(MemoryModuleType::IsPanicking) {
            cx.mob.camel_sit_down();
        }
    }

    fn name(&self) -> &'static str {
        "RandomSitting"
    }
}

/// `CreakingAi$1`: a `Swim` a creaking that cannot move refuses.
#[derive(Debug, Clone, Copy)]
pub struct CreakingSwim(pub Swim);

/// `new CreakingAi$1(chance)`.
#[must_use]
pub fn creaking_swim(chance: f32) -> Behavior<CreakingSwim> {
    Swim::new(chance).map_body(CreakingSwim)
}

impl<M: MobHost> BehaviorBody<M> for CreakingSwim {
    fn check_extra_start_conditions(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        cx.mob.creaking_can_move() && self.0.check_extra_start_conditions(brain, cx)
    }

    fn can_still_use(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) -> bool {
        self.0.can_still_use(brain, cx, time)
    }

    fn start(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        self.0.start(brain, cx, time);
    }

    fn tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        self.0.tick(brain, cx, time);
    }

    fn stop(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        self.0.stop(brain, cx, time);
    }

    fn timed_out(&self, end_timestamp: i64, time: i64) -> bool {
        <Swim as BehaviorBody<M>>::timed_out(&self.0, end_timestamp, time)
    }

    /// `getSimpleName()` of an anonymous class.
    fn name(&self) -> &'static str {
        ""
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::{BehaviorControl, Status};
    use crate::brain::memory::{MemoryModuleType as T, MemoryValue};
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};

    fn setup() -> (MobAi<TestMob>, BrainState) {
        let mut mob = TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 13);
        mob.on_ground = true;
        let cx = MobAi::new(mob);
        let brain =
            Brain::<TestMob>::new(&[T::IsPanicking, T::HurtBy, T::WalkTarget], vec![]).state;
        (cx, brain)
    }

    #[test]
    fn random_sitting_takes_seconds_and_stores_ticks() {
        let b = RandomSitting::new(3);
        assert_eq!(b.body.minimal_pose_ticks, 60);
        assert!(b.entry_condition.is_empty(), "ImmutableMap.of()");
    }

    #[test]
    fn every_one_of_the_six_conditions_alone_refuses_it() {
        for which in 0..6 {
            let (mut cx, mut brain) = setup();
            cx.mob.camel_pose_time = 60;
            match which {
                0 => cx.mob.in_water = true,
                1 => cx.mob.camel_pose_time = 59,
                2 => cx.mob.leashed = true,
                3 => cx.mob.on_ground = false,
                4 => cx.mob.has_controlling_passenger = true,
                _ => cx.mob.camel_can_change_pose = false,
            }
            let mut b = RandomSitting::new(3);
            assert!(!b.try_start(&mut brain, &mut cx, 0), "condition {which}");
        }
        // All six satisfied: it starts, and 60 ticks is exactly enough.
        let (mut cx, mut brain) = setup();
        cx.mob.camel_pose_time = 60;
        let mut b = RandomSitting::new(3);
        assert!(b.try_start(&mut brain, &mut cx, 0));
    }

    #[test]
    fn standing_up_is_unconditional_but_sitting_down_is_not() {
        // Sitting camel, panicking: it still stands up.
        let (mut cx, mut brain) = setup();
        cx.mob.camel_pose_time = 60;
        cx.mob.camel_sitting = true;
        brain.set_memory_value(T::IsPanicking, MemoryValue::Bool(true));
        let mut b = RandomSitting::new(3);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(cx.mob.calls, vec!["camel_stand_up".to_string()]);

        // Standing camel, panicking: it does nothing at all.
        let (mut cx, mut brain) = setup();
        cx.mob.camel_pose_time = 60;
        brain.set_memory_value(T::IsPanicking, MemoryValue::Bool(true));
        let mut b = RandomSitting::new(3);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(cx.mob.calls.is_empty());

        // Standing camel, calm: it sits.
        let (mut cx, mut brain) = setup();
        cx.mob.camel_pose_time = 60;
        let mut b = RandomSitting::new(3);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(cx.mob.calls, vec!["camel_sit_down".to_string()]);
        // And it is a one-shot: the base `canStillUse` is false.
        b.tick_or_stop(&mut brain, &mut cx, 1);
        assert_eq!(BehaviorControl::<TestMob>::status(&b), Status::Stopped);
    }

    #[test]
    fn a_ridden_camel_never_panics_and_a_panicking_one_stands_up_first() {
        let (mut cx, mut brain) = setup();
        cx.mob.panic_damage_kinds = vec!["arrow"];
        brain.set_memory_value(
            T::HurtBy,
            MemoryValue::DamageSource(crate::brain::memory::DamageSourceFacts {
                entity: None,
                direct_entity: None,
                kind: "arrow",
            }),
        );
        cx.mob.controlling_passenger_is_mob = true;
        let mut b = camel_panic(2.0);
        assert!(!b.try_start(&mut brain, &mut cx, 0), "isMobControlled()");
        cx.mob.controlling_passenger_is_mob = false;
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(
            cx.mob.calls.first().map(String::as_str),
            Some("camel_stand_up_instantly")
        );
        assert_eq!(BehaviorControl::<TestMob>::debug_string(&b), "CamelPanic");
    }

    #[test]
    fn a_creaking_that_cannot_move_is_refused_before_swim_draws() {
        let (mut cx, mut brain) = setup();
        cx.mob.in_water = true;
        cx.mob.creaking_can_move = false;
        let before = cx.mob.random;
        let mut b = creaking_swim(0.8);
        assert!(!b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(
            format!("{:?}", cx.mob.random),
            format!("{before:?}"),
            "canMove() is asked first, so Swim's nextFloat is not drawn"
        );
        assert_eq!(BehaviorControl::<TestMob>::debug_string(&b), "");
    }
}
