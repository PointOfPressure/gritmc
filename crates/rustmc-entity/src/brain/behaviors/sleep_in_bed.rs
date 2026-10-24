//! `net.minecraft.world.entity.ai.behavior.SleepInBed`, from the 26.2 jar.
//! `Behavior<LivingEntity>` with entry `{HOME: VALUE_PRESENT, LAST_WOKEN:
//! REGISTERED, LAST_SLEPT: REGISTERED, WALK_TARGET: REGISTERED,
//! CANT_REACH_WALK_TARGET_SINCE: REGISTERED}`, the default `(60, 60)`
//! duration (one level `nextInt(1)` per start) and `timedOut` overridden
//! to `false`. `COOLDOWN_AFTER_BEING_WOKEN = 100`.
//!
//! ```text
//! checkExtraStartConditions (0..158):
//!   if mob.isPassenger(): return false
//!   home = HOME.get(); if level.dimension() != home.dimension(): return false
//!   woken = LAST_WOKEN; if present: d = gameTime - woken; if d > 0 && d < 100: return false
//!   state = level.getBlockState(home.pos())
//!   return home.pos().closerToCenterThan(mob.position(), 2.0) && state.is(BEDS) && !state.getValue(OCCUPIED)
//! canStillUse (0..86): HOME present && brain.isActive(REST) && mob.getY() > pos.getY() + 0.4 && pos.closerToCenterThan(position, 1.14)
//! start (0..133):
//!   if time > nextOkStartTime:
//!     if brain.hasMemoryValue(DOORS_TO_CLOSE): InteractWithDoor.closeDoorsThatIHaveOpenedOrPassedThrough(level, mob, null, null, doors, NEAREST_LIVING_ENTITIES or empty)
//!     mob.startSleeping(HOME.pos()); LAST_SLEPT = time; erase WALK_TARGET; erase CANT_REACH_WALK_TARGET_SINCE
//! stop (0..20): if mob.isSleeping(): mob.stopSleeping(); nextOkStartTime = time + 40
//! ```
//!
//! `nextOkStartTime` gates only the *effects* of `start`: a start within
//! 40 ticks of being woken by `stop` runs and does nothing, and the
//! behaviour then reports `Running` until `canStillUse` fails.

use crate::ai::mob_ai::MobAi;
use crate::brain::activity::Activity;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::behaviors::interact_with_door::InteractWithDoor;
use crate::brain::memory::{MemoryModuleType, MemoryStatus, MemoryValue};
use crate::brain::BrainState;
use crate::host::{MobHost, PathLevel};
use crate::phys::closer_to_center_than;

/// `COOLDOWN_AFTER_BEING_WOKEN`.
pub const COOLDOWN_AFTER_BEING_WOKEN: i64 = 100;

/// `SleepInBed`'s body; build one with [`SleepInBed::new`].
#[derive(Debug, Clone, Copy, Default)]
pub struct SleepInBed {
    /// `nextOkStartTime`.
    pub next_ok_start_time: i64,
}

impl SleepInBed {
    /// `SleepInBed()`.
    #[must_use]
    pub fn new() -> Behavior<SleepInBed> {
        Behavior::new(
            SleepInBed::default(),
            vec![
                (MemoryModuleType::Home, MemoryStatus::ValuePresent),
                (MemoryModuleType::LastWoken, MemoryStatus::Registered),
                (MemoryModuleType::LastSlept, MemoryStatus::Registered),
                (MemoryModuleType::WalkTarget, MemoryStatus::Registered),
                (
                    MemoryModuleType::CantReachWalkTargetSince,
                    MemoryStatus::Registered,
                ),
            ],
        )
    }
}

impl<M: MobHost> BehaviorBody<M> for SleepInBed {
    fn check_extra_start_conditions(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        if cx.mob.is_passenger() {
            return false;
        }
        let home = brain
            .get_memory(MemoryModuleType::Home)
            .and_then(MemoryValue::as_global_pos)
            .expect("HOME is VALUE_PRESENT");
        if cx.mob.dimension() != home.dimension {
            return false;
        }
        if let Some(woken) = brain
            .get_memory(MemoryModuleType::LastWoken)
            .and_then(MemoryValue::as_long)
        {
            let d = cx.mob.level().game_time() - woken;
            if d > 0 && d < COOLDOWN_AFTER_BEING_WOKEN {
                return false;
            }
        }
        closer_to_center_than(home.pos, cx.mob.position(), 2.0)
            && cx.mob.is_bed_at(home.pos)
            && !cx.mob.bed_occupied_at(home.pos)
    }

    fn can_still_use(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        let Some(home) = brain
            .get_memory(MemoryModuleType::Home)
            .and_then(MemoryValue::as_global_pos)
        else {
            return false;
        };
        let pos = home.pos;
        brain.is_active(Activity::Rest)
            && cx.mob.y() > f64::from(pos.y) + 0.4
            && closer_to_center_than(pos, cx.mob.position(), 1.14)
    }

    fn start(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        if time > self.next_ok_start_time {
            if brain.has_memory_value(MemoryModuleType::DoorsToClose) {
                InteractWithDoor::close_doors_that_i_have_opened_or_passed_through(
                    brain, cx, None, None,
                );
            }
            let pos = brain
                .get_memory(MemoryModuleType::Home)
                .and_then(MemoryValue::as_global_pos)
                .expect("HOME is VALUE_PRESENT")
                .pos;
            cx.mob.start_sleeping(pos);
            brain.set_memory_value(MemoryModuleType::LastSlept, MemoryValue::Long(time));
            brain.erase_memory(MemoryModuleType::WalkTarget);
            brain.erase_memory(MemoryModuleType::CantReachWalkTargetSince);
        }
    }

    fn timed_out(&self, _end_timestamp: i64, _time: i64) -> bool {
        false
    }

    fn stop(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        if cx.mob.is_sleeping() {
            cx.mob.stop_sleeping();
            self.next_ok_start_time = time + 40;
        }
    }

    fn name(&self) -> &'static str {
        "SleepInBed"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::{GlobalPos, MemoryModuleType as T};
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};
    use rustmc_data::{BlockPos, Dimension};

    fn setup() -> (MobAi<TestMob>, BrainState) {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let bed = BlockPos { x: 1, y: 0, z: 0 };
        cx.mob.beds = vec![bed];
        let mut brain = Brain::<TestMob>::new(
            &[
                T::Home,
                T::LastWoken,
                T::LastSlept,
                T::WalkTarget,
                T::CantReachWalkTargetSince,
                T::DoorsToClose,
            ],
            vec![],
        );
        brain.add_activity_with_priority(Activity::Rest, 0, vec![]);
        let mut brain = brain.state;
        brain.set_core_activities(vec![Activity::Core]);
        brain.set_active_activity_if_possible(Activity::Rest);
        brain.set_memory_value(
            T::Home,
            MemoryValue::GlobalPos(GlobalPos {
                dimension: Dimension::Overworld,
                pos: bed,
            }),
        );
        (cx, brain)
    }

    #[test]
    fn sleeps_in_a_free_bed_within_two_blocks_outside_the_woken_cooldown() {
        let (mut cx, mut brain) = setup();
        brain.set_memory_value(T::WalkTarget, MemoryValue::Int(0));
        let mut b = SleepInBed::new();
        cx.mob.level.game_time = 150;
        brain.set_memory_value(T::LastWoken, MemoryValue::Long(100));
        assert!(!b.try_start(&mut brain, &mut cx, 150), "woken 50 ticks ago");
        brain.set_memory_value(T::LastWoken, MemoryValue::Long(50));
        cx.mob.occupied_beds = vec![BlockPos { x: 1, y: 0, z: 0 }];
        assert!(!b.try_start(&mut brain, &mut cx, 150), "occupied");
        cx.mob.occupied_beds.clear();
        assert!(b.try_start(&mut brain, &mut cx, 150));
        assert_eq!(cx.mob.calls, ["start_sleeping 1 0 0"]);
        assert_eq!(
            brain.get_memory(T::LastSlept),
            Some(&MemoryValue::Long(150))
        );
        assert!(!brain.has_memory_value(T::WalkTarget));
        b.tick_or_stop(&mut brain, &mut cx, 1000);
        assert_eq!(
            BehaviorControl::<TestMob>::status(&b),
            crate::brain::behavior::Status::Running,
            "never times out"
        );
        cx.mob.y = 0.3;
        b.tick_or_stop(&mut brain, &mut cx, 1001);
        assert_eq!(
            BehaviorControl::<TestMob>::status(&b),
            crate::brain::behavior::Status::Stopped,
            "below pos.y + 0.4"
        );
        assert_eq!(cx.mob.calls.last().unwrap(), "stop_sleeping");
        assert_eq!(b.body.next_ok_start_time, 1041);
        cx.mob.y = 1.0;
        cx.mob.calls.clear();
        assert!(
            b.try_start(&mut brain, &mut cx, 1020),
            "starts, but within the 40-tick gate it does nothing"
        );
        assert!(cx.mob.calls.is_empty());
    }
}
