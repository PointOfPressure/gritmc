//! `net.minecraft.world.entity.animal.sniffer.SnifferAi$Sniffing`,
//! `SnifferAi$Scenting` and `SnifferAi$FeelingHappy` — the three states the
//! sniffer's `IDLE` activity runs through, from the 26.2 jar.
//!
//! ```text
//! SnifferAi$Sniffing(int min, int max)   Behavior<Sniffer>
//!   entry Map.of(WALK_TARGET:VALUE_ABSENT, SNIFFER_SNIFFING_TARGET:VALUE_ABSENT,
//!                SNIFF_COOLDOWN:VALUE_ABSENT)
//!   checkExtraStartConditions (0..19):  !sniffer.isBaby() && sniffer.canSniff()
//!   canStillUse (0..4):                 sniffer.canSniff()
//!   start (0..8):                       sniffer.transitionTo(SNIFFING)
//!   stop (0..33):                       boolean t = timedOut(time)
//!                                       sniffer.transitionTo(IDLING)
//!                                       if t: sniffer.calculateDigPosition().ifPresent(lambda$stop$0)
//!   lambda$stop$0(sniffer, pos) (0..32):
//!                                       brain.setMemory(SNIFFER_SNIFFING_TARGET, pos)
//!                                       brain.setMemory(WALK_TARGET, new WalkTarget(pos, 1.25f, 0))
//!   built inside the idle RunOne at Pair.of(new Sniffing(40, 80), 1)
//!
//! SnifferAi$Scenting(int min, int max)   Behavior<Sniffer>
//!   entry Map.of(IS_PANICKING:VALUE_ABSENT, SNIFFER_DIGGING:VALUE_ABSENT,
//!                SNIFFER_SNIFFING_TARGET:VALUE_ABSENT, SNIFFER_HAPPY:VALUE_ABSENT,
//!                BREED_TARGET:VALUE_ABSENT)
//!   checkExtraStartConditions (0..12):  !sniffer.isTempted()
//!   canStillUse (0..1):                 true
//!   start (0..8):                       sniffer.transitionTo(SCENTING)
//!   stop (0..8):                        sniffer.transitionTo(IDLING)
//!   built inside the idle RunOne at Pair.of(new Scenting(40, 80), 1)
//!
//! SnifferAi$FeelingHappy(int min, int max)   Behavior<Sniffer>
//!   entry Map.of(SNIFFER_HAPPY:VALUE_PRESENT)
//!   canStillUse (0..1):                 true
//!   start (0..8):                       sniffer.transitionTo(FEELING_HAPPY)
//!   stop (0..18):                       sniffer.transitionTo(IDLING)
//!                                       brain.eraseMemory(SNIFFER_HAPPY)
//!   built by SnifferAi.initIdleActivity at Pair.of(3, new FeelingHappy(40, 100))
//! ```
//!
//! **`Scenting` and `FeelingHappy` both answer `canStillUse` with a bare
//! `iconst_1`,** so the *only* thing that ends either is the base's
//! `timedOut` — their entry conditions are checked at `tryStart` and never
//! again. That is why `FeelingHappy.stop` can erase `SNIFFER_HAPPY`
//! unconditionally: it is reached exactly once, at the end of the timer.
//!
//! **`Sniffing.stop` transitions to `IDLING` on both paths and only then
//! branches**, so a sniff interrupted by `canSniff()` going false leaves no
//! dig target at all. On the timed-out path `calculateDigPosition()` may
//! still answer empty, which is a third outcome: `IDLING`, no target, no
//! walk target — the `ifPresent` simply does not fire.
//!
//! **`Scenting` is the only one of the three with `BREED_TARGET` in its
//! entry map**, and it is the entry map rather than any check: a sniffer
//! that has found a mate does not scent. `FeelingHappy` has a
//! one-entry map and no `checkExtraStartConditions` override at all.
//!
//! The `WalkTarget(pos, 1.25f, 0)` speed is
//! `SnifferAi.SPEED_MULTIPLIER_WHEN_SNIFFING`. The entry maps are
//! `java.util.Map.of`, whose iteration order is unspecified; `Behavior`
//! walks its `ImmutableMap` copy and every check is pure, so the order is
//! not observable and this port keeps the bytecode's argument order.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::memory::{MemoryModuleType, MemoryStatus, WalkTarget};
use crate::brain::BrainState;
use crate::host::{MobHost, SnifferState};

/// `SnifferAi.SPEED_MULTIPLIER_WHEN_SNIFFING`, the speed of the walk target
/// `Sniffing.stop` writes.
pub const SPEED_MULTIPLIER_WHEN_SNIFFING: f32 = 1.25;

/// `SnifferAi$Sniffing`'s body.
#[derive(Debug, Clone, Copy, Default)]
pub struct SnifferSniffing;

impl SnifferSniffing {
    /// `new Sniffing(min, max)`.
    #[must_use]
    pub fn new(min_duration: i32, max_duration: i32) -> Behavior<SnifferSniffing> {
        Behavior::with_durations(
            SnifferSniffing,
            vec![
                (MemoryModuleType::WalkTarget, MemoryStatus::ValueAbsent),
                (
                    MemoryModuleType::SnifferSniffingTarget,
                    MemoryStatus::ValueAbsent,
                ),
                (MemoryModuleType::SniffCooldown, MemoryStatus::ValueAbsent),
            ],
            min_duration,
            max_duration,
        )
    }
}

impl<M: MobHost> BehaviorBody<M> for SnifferSniffing {
    fn check_extra_start_conditions(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        !cx.mob.is_baby() && cx.mob.sniffer_can_sniff()
    }

    fn can_still_use(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        cx.mob.sniffer_can_sniff()
    }

    fn start(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        cx.mob.sniffer_transition_to(SnifferState::Sniffing);
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
        if timed_out {
            if let Some(pos) = cx.mob.sniffer_calculate_dig_position() {
                brain.set_memory_value(
                    MemoryModuleType::SnifferSniffingTarget,
                    crate::brain::memory::MemoryValue::BlockPos(pos),
                );
                brain.set_memory_value(
                    MemoryModuleType::WalkTarget,
                    crate::brain::memory::MemoryValue::WalkTarget(WalkTarget::at_block(
                        pos,
                        SPEED_MULTIPLIER_WHEN_SNIFFING,
                        0,
                    )),
                );
            }
        }
    }

    fn name(&self) -> &'static str {
        "Sniffing"
    }
}

/// `SnifferAi$Scenting`'s body.
#[derive(Debug, Clone, Copy, Default)]
pub struct SnifferScenting;

impl SnifferScenting {
    /// `new Scenting(min, max)`.
    #[must_use]
    pub fn new(min_duration: i32, max_duration: i32) -> Behavior<SnifferScenting> {
        Behavior::with_durations(
            SnifferScenting,
            vec![
                (MemoryModuleType::IsPanicking, MemoryStatus::ValueAbsent),
                (MemoryModuleType::SnifferDigging, MemoryStatus::ValueAbsent),
                (
                    MemoryModuleType::SnifferSniffingTarget,
                    MemoryStatus::ValueAbsent,
                ),
                (MemoryModuleType::SnifferHappy, MemoryStatus::ValueAbsent),
                (MemoryModuleType::BreedTarget, MemoryStatus::ValueAbsent),
            ],
            min_duration,
            max_duration,
        )
    }
}

impl<M: MobHost> BehaviorBody<M> for SnifferScenting {
    fn check_extra_start_conditions(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        !cx.mob.sniffer_is_tempted()
    }

    fn can_still_use(&mut self, _brain: &mut BrainState, _cx: &mut MobAi<M>, _time: i64) -> bool {
        true
    }

    fn start(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        cx.mob.sniffer_transition_to(SnifferState::Scenting);
    }

    fn stop(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        cx.mob.sniffer_transition_to(SnifferState::Idling);
    }

    fn name(&self) -> &'static str {
        "Scenting"
    }
}

/// `SnifferAi$FeelingHappy`'s body.
#[derive(Debug, Clone, Copy, Default)]
pub struct SnifferFeelingHappy;

impl SnifferFeelingHappy {
    /// `new FeelingHappy(min, max)`.
    #[must_use]
    pub fn new(min_duration: i32, max_duration: i32) -> Behavior<SnifferFeelingHappy> {
        Behavior::with_durations(
            SnifferFeelingHappy,
            vec![(MemoryModuleType::SnifferHappy, MemoryStatus::ValuePresent)],
            min_duration,
            max_duration,
        )
    }
}

impl<M: MobHost> BehaviorBody<M> for SnifferFeelingHappy {
    fn can_still_use(&mut self, _brain: &mut BrainState, _cx: &mut MobAi<M>, _time: i64) -> bool {
        true
    }

    fn start(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        cx.mob.sniffer_transition_to(SnifferState::FeelingHappy);
    }

    fn stop(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        cx.mob.sniffer_transition_to(SnifferState::Idling);
        brain.erase_memory(MemoryModuleType::SnifferHappy);
    }

    fn name(&self) -> &'static str {
        "FeelingHappy"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::memory::MemoryValue;
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};
    use rustmc_data::BlockPos;

    fn setup() -> (MobAi<TestMob>, BrainState) {
        let cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let brain = Brain::<TestMob>::new(
            &[
                T::WalkTarget,
                T::SnifferSniffingTarget,
                T::SniffCooldown,
                T::IsPanicking,
                T::SnifferDigging,
                T::SnifferHappy,
                T::BreedTarget,
            ],
            vec![],
        )
        .state;
        (cx, brain)
    }

    #[test]
    fn sniffing_refuses_a_baby_and_writes_the_dig_target_only_on_timeout() {
        let (mut cx, mut brain) = setup();
        cx.mob.sniffer_can_sniff = true;
        cx.mob.baby = true;
        let mut b = SnifferSniffing::new(40, 80);
        assert!(!b.try_start(&mut brain, &mut cx, 0), "isBaby()");
        cx.mob.baby = false;
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(cx.mob.sniffer_state, SnifferState::Sniffing);

        let pos = BlockPos { x: 3, y: 1, z: 4 };
        cx.mob.sniffer_dig_position = Some(pos);
        let end = b.end_timestamp();
        b.tick_or_stop(&mut brain, &mut cx, end + 1);
        assert_eq!(cx.mob.sniffer_state, SnifferState::Idling);
        assert_eq!(
            brain.get_memory(T::SnifferSniffingTarget),
            Some(&MemoryValue::BlockPos(pos))
        );
        assert_eq!(
            brain.get_memory(T::WalkTarget),
            Some(&MemoryValue::WalkTarget(WalkTarget::at_block(pos, 1.25, 0)))
        );
    }

    #[test]
    fn sniffing_cut_short_leaves_no_target_even_with_a_dig_position() {
        let (mut cx, mut brain) = setup();
        cx.mob.sniffer_can_sniff = true;
        cx.mob.sniffer_dig_position = Some(BlockPos { x: 3, y: 1, z: 4 });
        let mut b = SnifferSniffing::new(40, 80);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        cx.mob.sniffer_can_sniff = false;
        b.tick_or_stop(&mut brain, &mut cx, 1);
        assert_eq!(
            BehaviorControl::<TestMob>::status(&b),
            crate::brain::behavior::Status::Stopped
        );
        assert_eq!(cx.mob.sniffer_state, SnifferState::Idling);
        assert!(!brain.has_memory_value(T::SnifferSniffingTarget));
        assert!(!brain.has_memory_value(T::WalkTarget));
    }

    #[test]
    fn a_timed_out_sniff_with_no_dig_position_writes_nothing() {
        let (mut cx, mut brain) = setup();
        cx.mob.sniffer_can_sniff = true;
        let mut b = SnifferSniffing::new(40, 80);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        let end = b.end_timestamp();
        b.tick_or_stop(&mut brain, &mut cx, end + 1);
        assert_eq!(cx.mob.sniffer_state, SnifferState::Idling);
        assert!(!brain.has_memory_value(T::SnifferSniffingTarget));
        assert!(!brain.has_memory_value(T::WalkTarget));
    }

    #[test]
    fn scenting_refuses_a_tempted_sniffer_and_only_the_timer_ends_it() {
        let (mut cx, mut brain) = setup();
        cx.mob.sniffer_tempted = true;
        let mut b = SnifferScenting::new(40, 80);
        assert!(!b.try_start(&mut brain, &mut cx, 0), "isTempted()");
        cx.mob.sniffer_tempted = false;
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(cx.mob.sniffer_state, SnifferState::Scenting);
        // Becoming tempted mid-run does NOT stop it: canStillUse is `true`.
        cx.mob.sniffer_tempted = true;
        let end = b.end_timestamp();
        b.tick_or_stop(&mut brain, &mut cx, end);
        assert_eq!(cx.mob.sniffer_state, SnifferState::Scenting);
        b.tick_or_stop(&mut brain, &mut cx, end + 1);
        assert_eq!(cx.mob.sniffer_state, SnifferState::Idling);
    }

    #[test]
    fn scenting_will_not_start_with_a_breed_target() {
        let (mut cx, mut brain) = setup();
        brain.set_memory_value(T::BreedTarget, MemoryValue::Entity(7));
        let mut b = SnifferScenting::new(40, 80);
        assert!(!b.try_start(&mut brain, &mut cx, 0));
    }

    #[test]
    fn feeling_happy_needs_the_memory_and_erases_it_at_the_end() {
        let (mut cx, mut brain) = setup();
        let mut b = SnifferFeelingHappy::new(40, 100);
        assert!(!b.try_start(&mut brain, &mut cx, 0), "SNIFFER_HAPPY absent");
        brain.set_memory_value(T::SnifferHappy, MemoryValue::Bool(true));
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(cx.mob.sniffer_state, SnifferState::FeelingHappy);
        // Erasing the memory does not stop it; the timer does.
        brain.erase_memory(T::SnifferHappy);
        let end = b.end_timestamp();
        b.tick_or_stop(&mut brain, &mut cx, end);
        assert_eq!(cx.mob.sniffer_state, SnifferState::FeelingHappy);
        b.tick_or_stop(&mut brain, &mut cx, end + 1);
        assert_eq!(cx.mob.sniffer_state, SnifferState::Idling);
        assert!(!brain.has_memory_value(T::SnifferHappy));
    }
}
