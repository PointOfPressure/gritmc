//! `net.minecraft.world.entity.animal.sniffer.SnifferAi$Searching`, the sole
//! behaviour of the sniffer's `SNIFF` activity, from the 26.2 jar.
//!
//! ```text
//! SnifferAi$Searching()   Behavior<Sniffer>, Behavior(map, 600) -> (600, 600)
//!   entry Map.of(WALK_TARGET:VALUE_PRESENT, IS_PANICKING:VALUE_ABSENT,
//!                SNIFFER_SNIFFING_TARGET:VALUE_PRESENT)
//!   checkExtraStartConditions (0..4):  sniffer.canSniff()
//!   canStillUse (0..91):
//!       0   if !sniffer.canSniff():
//!       7      sniffer.transitionTo(IDLING)
//!      15      return false
//!      17   walkTarget      = brain.getMemory(WALK_TARGET)
//!      27                      .map(WalkTarget::getTarget)          // BSM 0
//!      35                      .map(PositionTracker::currentBlockPosition)  // BSM 1
//!      45   sniffingTarget  = brain.getMemory(SNIFFER_SNIFFING_TARGET)
//!      57   if walkTarget.isEmpty() || sniffingTarget.isEmpty(): return false
//!      75   return sniffingTarget.get().equals(walkTarget.get())
//!   start (0..7):                      sniffer.transitionTo(SEARCHING)
//!   stop (0..48):
//!       0   if sniffer.canDig() && sniffer.canSniff():
//!      14      brain.setMemory(SNIFFER_DIGGING, true)
//!      28   brain.eraseMemory(WALK_TARGET)
//!      38   brain.eraseMemory(SNIFFER_SNIFFING_TARGET)
//!   built by SnifferAi.initSniffingActivity at Pair.of(0, new Searching())
//! ```
//!
//! **`canStillUse` has a side effect and that is the point.** It is the only
//! `canStillUse` in the sniffer's five behaviours that writes: on
//! `!canSniff()` it transitions the sniffer to `IDLING` *before* answering
//! `false`, so the visual state is corrected on the same tick the search is
//! abandoned rather than waiting for `stop`. `stop` never sets `IDLING`.
//!
//! **The equality is `sniffingTarget.equals(walkTarget)`, in that order** —
//! `BlockPos.equals`, so it is a value comparison and the order does not
//! change the answer; it is kept because the receiver is the operand the
//! bytecode loads first (offset 75 before 83).
//!
//! **The walk target is read through two `Optional.map`s**, `WalkTarget
//! .getTarget()` then `PositionTracker.currentBlockPosition()` (the two
//! `LambdaMetafactory` method references, bootstrap methods 0 and 1). An
//! `EntityTracker` whose entity is gone yields `null` from
//! `currentBlockPosition()`, and `Optional.map` turns that into an empty
//! optional rather than a wrapped null — which is exactly what
//! [`crate::brain::memory::PositionTracker::current_block_position`]
//! answering `None` does here.
//!
//! **`stop` runs on every exit, timer or not, and always erases both
//! memories** — the `canDig() && canSniff()` guard governs only whether
//! `SNIFFER_DIGGING` is set on the way out. So a search that times out at
//! 600 ticks over an undiggable block still clears the sniffing target, and
//! the sniffer starts over.
//!
//! The 600-tick duration comes from `Behavior(Map, int)`, which is
//! `(map, n, n)`, so the start draw is `nextInt(1)` — one `next(31)` for
//! zero.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::memory::{MemoryModuleType, MemoryStatus, MemoryValue};
use crate::brain::BrainState;
use crate::host::{MobHost, SnifferState};

/// `new Searching()`'s duration, both min and max.
pub const SEARCHING_DURATION: i32 = 600;

/// `SnifferAi$Searching`'s body.
#[derive(Debug, Clone, Copy, Default)]
pub struct SnifferSearching;

impl SnifferSearching {
    /// `new Searching()`.
    #[must_use]
    pub fn new() -> Behavior<SnifferSearching> {
        Behavior::with_duration(
            SnifferSearching,
            vec![
                (MemoryModuleType::WalkTarget, MemoryStatus::ValuePresent),
                (MemoryModuleType::IsPanicking, MemoryStatus::ValueAbsent),
                (
                    MemoryModuleType::SnifferSniffingTarget,
                    MemoryStatus::ValuePresent,
                ),
            ],
            SEARCHING_DURATION,
        )
    }
}

impl<M: MobHost> BehaviorBody<M> for SnifferSearching {
    fn check_extra_start_conditions(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        cx.mob.sniffer_can_sniff()
    }

    fn can_still_use(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        if !cx.mob.sniffer_can_sniff() {
            cx.mob.sniffer_transition_to(SnifferState::Idling);
            return false;
        }
        let walk_target = brain
            .get_memory(MemoryModuleType::WalkTarget)
            .and_then(MemoryValue::as_walk_target)
            .map(|w| w.target.clone())
            .and_then(|t| t.current_block_position(&cx.mob));
        let sniffing_target = match brain.get_memory(MemoryModuleType::SnifferSniffingTarget) {
            Some(MemoryValue::BlockPos(p)) => Some(*p),
            _ => None,
        };
        let (Some(walk_target), Some(sniffing_target)) = (walk_target, sniffing_target) else {
            return false;
        };
        sniffing_target == walk_target
    }

    fn start(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        cx.mob.sniffer_transition_to(SnifferState::Searching);
    }

    fn stop(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        if cx.mob.sniffer_can_dig() && cx.mob.sniffer_can_sniff() {
            brain.set_memory_value(MemoryModuleType::SnifferDigging, MemoryValue::Bool(true));
        }
        brain.erase_memory(MemoryModuleType::WalkTarget);
        brain.erase_memory(MemoryModuleType::SnifferSniffingTarget);
    }

    fn name(&self) -> &'static str {
        "Searching"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::{MemoryModuleType as T, WalkTarget};
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};
    use rustmc_data::BlockPos;

    fn setup() -> (MobAi<TestMob>, BrainState) {
        let cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let brain = Brain::<TestMob>::new(
            &[
                T::WalkTarget,
                T::IsPanicking,
                T::SnifferSniffingTarget,
                T::SnifferDigging,
            ],
            vec![],
        )
        .state;
        (cx, brain)
    }

    fn armed(cx: &mut MobAi<TestMob>, brain: &mut BrainState, pos: BlockPos) {
        cx.mob.sniffer_can_sniff = true;
        brain.set_memory_value(T::SnifferSniffingTarget, MemoryValue::BlockPos(pos));
        brain.set_memory_value(
            T::WalkTarget,
            MemoryValue::WalkTarget(WalkTarget::at_block(pos, 1.25, 0)),
        );
    }

    #[test]
    fn searching_runs_while_the_two_targets_agree() {
        let (mut cx, mut brain) = setup();
        let pos = BlockPos { x: 2, y: 1, z: 3 };
        armed(&mut cx, &mut brain, pos);
        let mut b = SnifferSearching::new();
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(b.end_timestamp(), 600);
        assert_eq!(cx.mob.sniffer_state, SnifferState::Searching);
        assert!(b.body.can_still_use(&mut brain, &mut cx, 1));
        // Walking somewhere else ends it.
        brain.set_memory_value(
            T::WalkTarget,
            MemoryValue::WalkTarget(WalkTarget::at_block(BlockPos { x: 9, y: 1, z: 9 }, 1.25, 0)),
        );
        assert!(!b.body.can_still_use(&mut brain, &mut cx, 2));
    }

    #[test]
    fn can_still_use_transitions_to_idling_before_it_answers_false() {
        let (mut cx, mut brain) = setup();
        let pos = BlockPos { x: 2, y: 1, z: 3 };
        armed(&mut cx, &mut brain, pos);
        let mut b = SnifferSearching::new();
        assert!(b.try_start(&mut brain, &mut cx, 0));
        cx.mob.calls.clear();
        cx.mob.sniffer_can_sniff = false;
        assert!(!b.body.can_still_use(&mut brain, &mut cx, 1));
        assert_eq!(cx.mob.calls, vec!["transition_to Idling".to_string()]);
    }

    #[test]
    fn an_absent_walk_target_answers_false_without_touching_the_state() {
        let (mut cx, mut brain) = setup();
        cx.mob.sniffer_can_sniff = true;
        brain.set_memory_value(
            T::SnifferSniffingTarget,
            MemoryValue::BlockPos(BlockPos { x: 2, y: 1, z: 3 }),
        );
        let mut b = SnifferSearching::new();
        cx.mob.calls.clear();
        assert!(!b.body.can_still_use(&mut brain, &mut cx, 1));
        assert!(cx.mob.calls.is_empty(), "no transition on this path");
    }

    #[test]
    fn stop_erases_both_and_only_sets_digging_when_both_predicates_hold() {
        for (can_dig, can_sniff, expect) in [
            (false, false, false),
            (true, false, false),
            (false, true, false),
            (true, true, true),
        ] {
            let (mut cx, mut brain) = setup();
            let pos = BlockPos { x: 2, y: 1, z: 3 };
            armed(&mut cx, &mut brain, pos);
            let mut b = SnifferSearching::new();
            assert!(b.try_start(&mut brain, &mut cx, 0));
            cx.mob.sniffer_can_dig = can_dig;
            cx.mob.sniffer_can_sniff = can_sniff;
            crate::brain::behavior::BehaviorControl::do_stop(&mut b, &mut brain, &mut cx, 1);
            assert_eq!(
                brain.has_memory_value(T::SnifferDigging),
                expect,
                "canDig {can_dig} canSniff {can_sniff}"
            );
            assert!(!brain.has_memory_value(T::WalkTarget), "always erased");
            assert!(!brain.has_memory_value(T::SnifferSniffingTarget));
        }
    }
}
