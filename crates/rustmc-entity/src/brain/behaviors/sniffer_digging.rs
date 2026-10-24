//! `net.minecraft.world.entity.animal.sniffer.SnifferAi$Digging` and
//! `SnifferAi$FinishedDigging`, the two behaviours of the sniffer's `DIG`
//! activity, from the 26.2 jar.
//!
//! ```text
//! SnifferAi$Digging(int min, int max)   Behavior<Sniffer>
//!   entry Map.of(IS_PANICKING:VALUE_ABSENT, WALK_TARGET:VALUE_ABSENT,
//!                SNIFFER_DIGGING:VALUE_PRESENT, SNIFF_COOLDOWN:VALUE_ABSENT)
//!   checkExtraStartConditions (0..4):   sniffer.canSniff()
//!   canStillUse (0..35):                brain.getMemory(SNIFFER_DIGGING).isPresent()
//!                                       && sniffer.canDig() && !sniffer.isInLove()
//!   start (0..7):                       sniffer.transitionTo(DIGGING)
//!   stop (0..36):                       if timedOut(time):
//!                                         brain.setMemoryWithExpiry(SNIFF_COOLDOWN, Unit.INSTANCE, 9600L)
//!                                       else:
//!                                         SnifferAi.resetSniffing(sniffer)
//!   built by SnifferAi.initDigActivity at Pair.of(0, new Digging(160, 180))
//!
//! SnifferAi$FinishedDigging(int n)   Behavior<Sniffer>, durations (n, n)
//!   entry Map.of(IS_PANICKING:VALUE_ABSENT, WALK_TARGET:VALUE_ABSENT,
//!                SNIFFER_DIGGING:VALUE_PRESENT, SNIFF_COOLDOWN:VALUE_PRESENT)
//!   checkExtraStartConditions (0..1):   true
//!   canStillUse (0..13):                brain.getMemory(SNIFFER_DIGGING).isPresent()
//!   start (0..8):                       sniffer.transitionTo(RISING)
//!   stop (0..44):                       boolean t = timedOut(time)
//!                                       sniffer.transitionTo(IDLING).onDiggingComplete(t)
//!                                       brain.eraseMemory(SNIFFER_DIGGING)
//!                                       brain.setMemory(SNIFFER_HAPPY, true)
//!   built by SnifferAi.initDigActivity at Pair.of(0, new FinishedDigging(40))
//! ```
//!
//! **Both `stop` bodies branch on `this.timedOut(time)`**, which reads the
//! base `Behavior`'s `endTimestamp`; that is what
//! [`crate::brain::behavior::BehaviorBody::stop_timed`] exists for, and it
//! is the only difference between these two and every earlier transcription.
//!
//! **`Digging.stop` is the one that pays out.** The cooldown is set *only*
//! on the timed-out path — a dig cut short by `canStillUse` going false
//! calls `resetSniffing` instead, which erases `SNIFFER_DIGGING` and
//! `SNIFFER_SNIFFING_TARGET` and puts the sniffer back to `IDLING` with no
//! cooldown at all, so it may start sniffing again on the next tick.
//!
//! **`FinishedDigging.stop` runs the same three statements on both paths**
//! and passes `timedOut` on as `onDiggingComplete`'s argument, so a dig
//! interrupted by the memory going absent still transitions to `IDLING`,
//! still erases `SNIFFER_DIGGING` and still sets `SNIFFER_HAPPY` — only the
//! `boolean` differs. `transitionTo` returns `this`, which is what lets the
//! two calls chain in the bytecode; the return is otherwise unused.
//!
//! **A null worth recording:** `FinishedDigging`'s
//! `checkExtraStartConditions` is a bare `iconst_1`, so it has no
//! condition of its own beyond its four entry memories. Its `SNIFF_COOLDOWN`
//! is `VALUE_PRESENT` where `Digging`'s is `VALUE_ABSENT`, which is what
//! orders the pair inside one activity: the cooldown `Digging.stop` sets is
//! exactly what admits `FinishedDigging`.
//!
//! `SnifferAi.SNIFFING_COOLDOWN_TICKS = 9600`.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::memory::{MemoryModuleType, MemoryStatus, MemoryValue};
use crate::brain::BrainState;
use crate::host::{MobHost, SnifferState};

/// `SnifferAi.SNIFFING_COOLDOWN_TICKS`, the expiry `Digging.stop` gives
/// `SNIFF_COOLDOWN`.
pub const SNIFFING_COOLDOWN_TICKS: i64 = 9600;

/// `SnifferAi.resetSniffing(Sniffer)` (0..27): erase `SNIFFER_DIGGING`,
/// erase `SNIFFER_SNIFFING_TARGET`, `transitionTo(IDLING)`. The return is
/// the sniffer, discarded at all four call sites.
pub fn reset_sniffing<M: MobHost>(brain: &mut BrainState, cx: &mut MobAi<M>) {
    brain.erase_memory(MemoryModuleType::SnifferDigging);
    brain.erase_memory(MemoryModuleType::SnifferSniffingTarget);
    cx.mob.sniffer_transition_to(SnifferState::Idling);
}

/// `SnifferAi$Digging`'s body.
#[derive(Debug, Clone, Copy, Default)]
pub struct SnifferDigging;

impl SnifferDigging {
    /// `new Digging(min, max)`.
    #[must_use]
    pub fn new(min_duration: i32, max_duration: i32) -> Behavior<SnifferDigging> {
        Behavior::with_durations(
            SnifferDigging,
            vec![
                (MemoryModuleType::IsPanicking, MemoryStatus::ValueAbsent),
                (MemoryModuleType::WalkTarget, MemoryStatus::ValueAbsent),
                (MemoryModuleType::SnifferDigging, MemoryStatus::ValuePresent),
                (MemoryModuleType::SniffCooldown, MemoryStatus::ValueAbsent),
            ],
            min_duration,
            max_duration,
        )
    }
}

impl<M: MobHost> BehaviorBody<M> for SnifferDigging {
    fn check_extra_start_conditions(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        cx.mob.sniffer_can_sniff()
    }

    fn can_still_use(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        brain.has_memory_value(MemoryModuleType::SnifferDigging)
            && cx.mob.sniffer_can_dig()
            && !cx.mob.is_in_love()
    }

    fn start(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        cx.mob.sniffer_transition_to(SnifferState::Digging);
    }

    fn stop_timed(
        &mut self,
        brain: &mut BrainState,
        cx: &mut MobAi<M>,
        time: i64,
        end_timestamp: i64,
    ) {
        if time > end_timestamp {
            brain.set_memory_with_expiry(
                MemoryModuleType::SniffCooldown,
                MemoryValue::Unit,
                SNIFFING_COOLDOWN_TICKS,
            );
        } else {
            reset_sniffing(brain, cx);
        }
    }

    fn name(&self) -> &'static str {
        "Digging"
    }
}

/// `SnifferAi$FinishedDigging`'s body.
#[derive(Debug, Clone, Copy, Default)]
pub struct SnifferFinishedDigging;

impl SnifferFinishedDigging {
    /// `new FinishedDigging(n)` — the one argument is both durations.
    #[must_use]
    pub fn new(duration: i32) -> Behavior<SnifferFinishedDigging> {
        Behavior::with_durations(
            SnifferFinishedDigging,
            vec![
                (MemoryModuleType::IsPanicking, MemoryStatus::ValueAbsent),
                (MemoryModuleType::WalkTarget, MemoryStatus::ValueAbsent),
                (MemoryModuleType::SnifferDigging, MemoryStatus::ValuePresent),
                (MemoryModuleType::SniffCooldown, MemoryStatus::ValuePresent),
            ],
            duration,
            duration,
        )
    }
}

impl<M: MobHost> BehaviorBody<M> for SnifferFinishedDigging {
    fn check_extra_start_conditions(
        &mut self,
        _brain: &mut BrainState,
        _cx: &mut MobAi<M>,
    ) -> bool {
        true
    }

    fn can_still_use(&mut self, brain: &mut BrainState, _cx: &mut MobAi<M>, _time: i64) -> bool {
        brain.has_memory_value(MemoryModuleType::SnifferDigging)
    }

    fn start(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        cx.mob.sniffer_transition_to(SnifferState::Rising);
    }

    fn stop_timed(
        &mut self,
        brain: &mut BrainState,
        cx: &mut MobAi<M>,
        time: i64,
        end_timestamp: i64,
    ) {
        let timed_out = time > end_timestamp;
        cx.mob.sniffer_transition_to(SnifferState::Idling);
        cx.mob.sniffer_on_digging_complete(timed_out);
        brain.erase_memory(MemoryModuleType::SnifferDigging);
        brain.set_memory_value(MemoryModuleType::SnifferHappy, MemoryValue::Bool(true));
    }

    fn name(&self) -> &'static str {
        "FinishedDigging"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};

    fn setup() -> (MobAi<TestMob>, BrainState) {
        let cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let brain = Brain::<TestMob>::new(
            &[
                T::IsPanicking,
                T::WalkTarget,
                T::SnifferDigging,
                T::SniffCooldown,
                T::SnifferHappy,
                T::SnifferSniffingTarget,
            ],
            vec![],
        )
        .state;
        (cx, brain)
    }

    #[test]
    fn digging_needs_can_sniff_and_goes_to_the_digging_state() {
        let (mut cx, mut brain) = setup();
        brain.set_memory_value(T::SnifferDigging, MemoryValue::Bool(true));
        let mut b = SnifferDigging::new(160, 180);
        assert!(!b.try_start(&mut brain, &mut cx, 0), "canSniff() is false");
        cx.mob.sniffer_can_sniff = true;
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(cx.mob.sniffer_state, SnifferState::Digging);
        // Behavior draws once from the LEVEL random: 160 + nextInt(21).
        let end = b.end_timestamp();
        assert!((160..=180).contains(&end), "end_timestamp {end}");
    }

    #[test]
    fn digging_sets_the_cooldown_only_when_it_times_out() {
        let (mut cx, mut brain) = setup();
        brain.set_memory_value(T::SnifferDigging, MemoryValue::Bool(true));
        cx.mob.sniffer_can_sniff = true;
        cx.mob.sniffer_can_dig = true;
        let mut b = SnifferDigging::new(160, 180);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        let end = b.end_timestamp();
        b.tick_or_stop(&mut brain, &mut cx, end + 1);
        assert_eq!(
            brain.get_memory(T::SniffCooldown),
            Some(&MemoryValue::Unit),
            "the timed-out path sets the 9600-tick cooldown"
        );
        assert_eq!(brain.time_until_expiry(T::SniffCooldown), 9600);
        assert!(
            brain.has_memory_value(T::SnifferDigging),
            "the timed-out path does NOT reset sniffing"
        );
    }

    #[test]
    fn digging_cut_short_resets_sniffing_and_leaves_no_cooldown() {
        let (mut cx, mut brain) = setup();
        brain.set_memory_value(T::SnifferDigging, MemoryValue::Bool(true));
        brain.set_memory_value(T::SnifferSniffingTarget, MemoryValue::Unit);
        cx.mob.sniffer_can_sniff = true;
        cx.mob.sniffer_can_dig = true;
        let mut b = SnifferDigging::new(160, 180);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        // isInLove() ends it before the timer does.
        cx.mob.in_love = true;
        b.tick_or_stop(&mut brain, &mut cx, 1);
        assert_eq!(
            BehaviorControl::<TestMob>::status(&b),
            crate::brain::behavior::Status::Stopped
        );
        assert!(!brain.has_memory_value(T::SniffCooldown), "no cooldown");
        assert!(!brain.has_memory_value(T::SnifferDigging));
        assert!(!brain.has_memory_value(T::SnifferSniffingTarget));
        assert_eq!(cx.mob.sniffer_state, SnifferState::Idling);
    }

    #[test]
    fn finished_digging_runs_the_same_four_statements_either_way() {
        for timed_out in [false, true] {
            let (mut cx, mut brain) = setup();
            brain.set_memory_value(T::SnifferDigging, MemoryValue::Bool(true));
            brain.set_memory_value(T::SniffCooldown, MemoryValue::Unit);
            let mut b = SnifferFinishedDigging::new(40);
            assert!(b.try_start(&mut brain, &mut cx, 0));
            assert_eq!(b.end_timestamp(), 40, "min == max draws nextInt(1)");
            assert_eq!(cx.mob.sniffer_state, SnifferState::Rising);
            let at = if timed_out {
                41
            } else {
                // The memory going absent is the other way out.
                brain.erase_memory(T::SnifferDigging);
                1
            };
            b.tick_or_stop(&mut brain, &mut cx, at);
            assert_eq!(cx.mob.sniffer_state, SnifferState::Idling);
            assert_eq!(
                cx.mob.calls,
                vec![
                    "transition_to Rising".to_string(),
                    "transition_to Idling".to_string(),
                    format!("on_digging_complete {timed_out}"),
                ],
                "only the boolean differs"
            );
            assert!(!brain.has_memory_value(T::SnifferDigging));
            assert_eq!(
                brain.get_memory(T::SnifferHappy),
                Some(&MemoryValue::Bool(true))
            );
        }
    }
}
