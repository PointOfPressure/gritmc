//! `SnifferAi$1`, `SnifferAi$2` and `SnifferAi$3` — the three anonymous
//! subclasses `SnifferAi` builds, from the 26.2 jar.
//!
//! ```text
//! SnifferAi$1 extends AnimalPanic<Sniffer>
//!   <init>(float) (0..5):            super(speedMultiplier)
//!   start(level, sniffer, t) (0..12):
//!        0  SnifferAi.resetSniffing(sniffer)      // return discarded
//!        5  super.start(level, sniffer, t)
//!   built by initCoreActivity: new SnifferAi$1(2.0f)          // fconst_2
//!
//! SnifferAi$2 extends AnimalMakeLove
//!   <init>(EntityType) (0..5):       super(partnerType)
//!   start(level, animal, t) (0..15):
//!        0  SnifferAi.resetSniffing((Sniffer) animal)
//!        8  super.start(level, animal, t)
//!   built by initIdleActivity: new SnifferAi$2(EntityTypes.SNIFFER)
//!
//! SnifferAi$3 extends FollowTemptation
//!   <init>(Function, Function) (0..6): super(speedModifier, closeEnough)
//!   start(level, mob, t) (0..15):
//!        0  SnifferAi.resetSniffing((Sniffer) mob)
//!        8  super.start(level, mob, t)
//!   built by initIdleActivity: new SnifferAi$3(
//!        lambda$initIdleActivity$0 = e -> 1.25f,                      // Float
//!        lambda$initIdleActivity$1 = e -> e.isBaby() ? 2.5d : 3.5d)   // Double
//! ```
//!
//! **All three override exactly one method and it is `start`**, and all
//! three call `SnifferAi.resetSniffing` *before* `super.start`. So the
//! sniffer's state and its two sniffing memories are cleared first, and
//! then the superclass runs its own unmodified `start` — which for
//! `AnimalPanic` writes `IS_PANICKING` and a walk target, and for
//! `AnimalMakeLove` and `FollowTemptation` is the plain superclass body.
//! Nothing else is overridden: `checkExtraStartConditions`, `canStillUse`,
//! `tick`, `stop` and the durations are the superclass's, which is why
//! [`crate::brain::behavior::Behavior::map_body`] re-wraps the object the
//! superclass constructor already built rather than rebuilding it.
//!
//! **`debugString()` of all three is the empty string.**
//! `Behavior.debugString()` is `getClass().getSimpleName()`, and
//! `Class.getSimpleName()` of an anonymous class is `""` — these classes have
//! no source name at all, only the `SnifferAi$1` binary name the ledger uses.
//! That is a real, observable difference from every named behaviour, so
//! [`BehaviorBody::name`] answers `""` here and a test pins it.
//!
//! **The two `$3` lambdas are constants, not reads of the sniffer.**
//! `lambda$initIdleActivity$0` ignores its argument and returns `1.25f`
//! (`SnifferAi.SPEED_MULTIPLIER_WHEN_TEMPTED`); `lambda$initIdleActivity$1`
//! is the only one that looks at the entity, and only at `isBaby()`.
//!
//! **A null:** none of the three overrides `stop`, so nothing undoes
//! `resetSniffing` when the behaviour ends — the sniffer is left in whatever
//! state the superclass's own `stop` leaves it, which for all three is
//! `IDLING` by omission rather than by assignment.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::behaviors::animal_make_love::AnimalMakeLove;
use crate::brain::behaviors::animal_panic::AnimalPanic;
use crate::brain::behaviors::follow_temptation::FollowTemptation;
use crate::brain::behaviors::sniffer_digging::reset_sniffing;
use crate::brain::BrainState;
use crate::host::MobHost;

/// `SnifferAi.SPEED_MULTIPLIER_WHEN_PANICKING`, `SnifferAi$1`'s argument.
pub const SPEED_MULTIPLIER_WHEN_PANICKING: f32 = 2.0;
/// `SnifferAi.SPEED_MULTIPLIER_WHEN_TEMPTED`, `lambda$initIdleActivity$0`.
pub const SPEED_MULTIPLIER_WHEN_TEMPTED: f32 = 1.25;
/// `lambda$initIdleActivity$1`'s baby branch.
pub const TEMPT_CLOSE_ENOUGH_BABY: f64 = 2.5;
/// `lambda$initIdleActivity$1`'s adult branch.
pub const TEMPT_CLOSE_ENOUGH_ADULT: f64 = 3.5;

/// The shared shape of all three: a superclass body whose `start` is
/// preceded by `SnifferAi.resetSniffing`. `B` is the superclass body.
#[derive(Debug, Clone, Copy, Default)]
pub struct ResetSniffingFirst<B>(pub B);

impl<M: MobHost, B: BehaviorBody<M>> BehaviorBody<M> for ResetSniffingFirst<B> {
    fn check_extra_start_conditions(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        self.0.check_extra_start_conditions(brain, cx)
    }

    fn start(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        reset_sniffing(brain, cx);
        self.0.start(brain, cx, time);
    }

    fn tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        self.0.tick(brain, cx, time);
    }

    fn stop(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        self.0.stop(brain, cx, time);
    }

    fn stop_timed(
        &mut self,
        brain: &mut BrainState,
        cx: &mut MobAi<M>,
        time: i64,
        end_timestamp: i64,
    ) {
        self.0.stop_timed(brain, cx, time, end_timestamp);
    }

    fn can_still_use(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) -> bool {
        self.0.can_still_use(brain, cx, time)
    }

    fn timed_out(&self, end_timestamp: i64, time: i64) -> bool {
        self.0.timed_out(end_timestamp, time)
    }

    /// `getClass().getSimpleName()` of an anonymous class.
    fn name(&self) -> &'static str {
        ""
    }
}

/// `new SnifferAi$1(2.0f)`.
#[must_use]
pub fn sniffer_panic() -> Behavior<ResetSniffingFirst<AnimalPanic>> {
    AnimalPanic::new(SPEED_MULTIPLIER_WHEN_PANICKING).map_body(ResetSniffingFirst)
}

/// `new SnifferAi$2(EntityTypes.SNIFFER)`.
#[must_use]
pub fn sniffer_make_love() -> Behavior<ResetSniffingFirst<AnimalMakeLove>> {
    AnimalMakeLove::new("sniffer").map_body(ResetSniffingFirst)
}

/// `new SnifferAi$3(e -> 1.25f, e -> e.isBaby() ? 2.5d : 3.5d)`.
#[must_use]
pub fn sniffer_follow_temptation<M: MobHost + 'static>(
) -> Behavior<ResetSniffingFirst<FollowTemptation<M>>> {
    FollowTemptation::<M>::with_distance(
        Box::new(|_| SPEED_MULTIPLIER_WHEN_TEMPTED),
        Box::new(|cx| {
            if cx.mob.is_baby() {
                TEMPT_CLOSE_ENOUGH_BABY
            } else {
                TEMPT_CLOSE_ENOUGH_ADULT
            }
        }),
    )
    .map_body(ResetSniffingFirst)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::{MemoryModuleType as T, MemoryValue};
    use crate::brain::Brain;
    use crate::host::SnifferState;
    use crate::testing::{GridLevel, TestMob};

    fn brain_for(types: &[T]) -> BrainState {
        Brain::<TestMob>::new(types, vec![]).state
    }

    #[test]
    fn all_three_debug_strings_are_empty() {
        assert_eq!(
            BehaviorControl::<TestMob>::debug_string(&sniffer_panic()),
            ""
        );
        assert_eq!(
            BehaviorControl::<TestMob>::debug_string(&sniffer_make_love()),
            ""
        );
        assert_eq!(
            BehaviorControl::<TestMob>::debug_string(&sniffer_follow_temptation::<TestMob>()),
            ""
        );
    }

    #[test]
    fn the_panic_subclass_resets_sniffing_before_the_superclass_start() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut brain = brain_for(&[
            T::IsPanicking,
            T::HurtBy,
            T::WalkTarget,
            T::SnifferDigging,
            T::SnifferSniffingTarget,
        ]);
        brain.set_memory_value(T::SnifferDigging, MemoryValue::Bool(true));
        brain.set_memory_value(T::SnifferSniffingTarget, MemoryValue::Unit);
        cx.mob.panic_damage_kinds = vec!["arrow"];
        brain.set_memory_value(
            T::HurtBy,
            MemoryValue::DamageSource(crate::brain::memory::DamageSourceFacts {
                entity: None,
                direct_entity: None,
                kind: "arrow",
            }),
        );
        let mut b = sniffer_panic();
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(
            !brain.has_memory_value(T::SnifferDigging),
            "resetSniffing ran"
        );
        assert!(!brain.has_memory_value(T::SnifferSniffingTarget));
        assert_eq!(cx.mob.sniffer_state, SnifferState::Idling);
        assert!(
            brain.has_memory_value(T::IsPanicking),
            "and then AnimalPanic.start ran"
        );
    }

    #[test]
    fn the_superclass_entry_conditions_and_durations_are_untouched() {
        // `map_body` keeps what the superclass constructor built: the two
        // `Registered` entries `AnimalPanic` declares, and its (100, 120)
        // durations.
        let plain = AnimalPanic::new(SPEED_MULTIPLIER_WHEN_PANICKING);
        let wrapped = sniffer_panic();
        assert_eq!(wrapped.entry_condition, plain.entry_condition);
        assert_eq!(
            wrapped.entry_condition,
            vec![
                (
                    T::IsPanicking,
                    crate::brain::memory::MemoryStatus::Registered
                ),
                (T::HurtBy, crate::brain::memory::MemoryStatus::Registered),
            ]
        );

        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut brain = brain_for(&[T::IsPanicking, T::HurtBy, T::WalkTarget]);
        let mut b = sniffer_panic();
        // Neither `HURT_BY` nor a freeze reason is set, so `AnimalPanic`'s own
        // `checkExtraStartConditions` refuses — the wrapper adds no condition.
        assert!(!b.try_start(&mut brain, &mut cx, 0));
        assert!(cx.mob.calls.is_empty(), "resetSniffing did not run");
    }

    #[test]
    fn the_temptation_distance_lambda_reads_only_is_baby() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut b = sniffer_follow_temptation::<TestMob>();
        assert_eq!((b.body.0.close_enough_distance)(&mut cx), 3.5);
        cx.mob.baby = true;
        assert_eq!((b.body.0.close_enough_distance)(&mut cx), 2.5);
        assert_eq!((b.body.0.speed_modifier)(&mut cx), 1.25);
    }
}
