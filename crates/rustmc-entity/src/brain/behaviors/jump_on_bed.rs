//! `net.minecraft.world.entity.ai.behavior.JumpOnBed`, from the 26.2 jar.
//! `Behavior<Mob>` with entry `{NEAREST_BED: VALUE_PRESENT, WALK_TARGET:
//! VALUE_ABSENT}`, the default duration (one level `nextInt(1)` per
//! start) and `timedOut` overridden to `false`.
//! `MAX_TIME_TO_REACH_BED = 100`, `MIN_JUMPS = 3`, `MAX_JUMPS = 6`,
//! `COOLDOWN_BETWEEN_JUMPS = 5`.
//!
//! ```text
//! checkExtraStartConditions (0..21): mob.isBaby() && nearBed(level, mob)
//! start (0..21): NEAREST_BED.ifPresent(pos ->
//!     targetBed = pos; remainingTimeToReachBed = 100
//!     remainingJumps = 3 + level.getRandom().nextInt(4)                       <- LEVEL random
//!     remainingCooldownUntilNextJump = 0
//!     WALK_TARGET = WalkTarget(pos, speedModifier, 0))
//! stop (0..28): targetBed = null; the three counters = 0
//! canStillUse (0..49): isBaby && targetBed != null && isBed(targetBed) && !tiredOfWalking && !tiredOfJumping
//! tick (0..69):
//!   if !onOrOverBed: remainingTimeToReachBed--
//!   if remainingCooldownUntilNextJump > 0: remainingCooldownUntilNextJump--; return
//!   if onBedSurface: mob.getJumpControl().jump(); remainingJumps--; remainingCooldownUntilNextJump = 5
//! nearBed: onOrOverBed || NEAREST_BED present;  onOrOverBed: isBed(pos) || isBed(pos.below());  onBedSurface: isBed(pos)
//! tiredOfWalking: !onOrOverBed && remainingTimeToReachBed <= 0;  tiredOfJumping: onOrOverBed && remainingJumps <= 0
//! isBed: level.getBlockState(pos).is(BEDS)
//! ```

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::memory::{MemoryModuleType, MemoryStatus, MemoryValue, WalkTarget};
use crate::brain::BrainState;
use crate::host::MobHost;
use rustmc_data::BlockPos;

/// `MAX_TIME_TO_REACH_BED`.
pub const MAX_TIME_TO_REACH_BED: i32 = 100;
/// `MIN_JUMPS`.
pub const MIN_JUMPS: i32 = 3;
/// `MAX_JUMPS`.
pub const MAX_JUMPS: i32 = 6;
/// `COOLDOWN_BETWEEN_JUMPS`.
pub const COOLDOWN_BETWEEN_JUMPS: i32 = 5;

/// `JumpOnBed`'s body; build one with [`JumpOnBed::new`].
#[derive(Debug, Clone, Copy)]
pub struct JumpOnBed {
    pub speed_modifier: f32,
    pub target_bed: Option<BlockPos>,
    pub remaining_time_to_reach_bed: i32,
    pub remaining_jumps: i32,
    pub remaining_cooldown_until_next_jump: i32,
}

impl JumpOnBed {
    /// `JumpOnBed(float)`.
    #[must_use]
    pub fn new(speed_modifier: f32) -> Behavior<JumpOnBed> {
        Behavior::new(
            JumpOnBed {
                speed_modifier,
                target_bed: None,
                remaining_time_to_reach_bed: 0,
                remaining_jumps: 0,
                remaining_cooldown_until_next_jump: 0,
            },
            vec![
                (MemoryModuleType::NearestBed, MemoryStatus::ValuePresent),
                (MemoryModuleType::WalkTarget, MemoryStatus::ValueAbsent),
            ],
        )
    }

    fn on_or_over_bed<M: MobHost>(cx: &MobAi<M>) -> bool {
        let pos = cx.mob.block_position();
        let below = BlockPos {
            x: pos.x,
            y: pos.y - 1,
            z: pos.z,
        };
        cx.mob.is_bed_at(pos) || cx.mob.is_bed_at(below)
    }

    fn on_bed_surface<M: MobHost>(cx: &MobAi<M>) -> bool {
        cx.mob.is_bed_at(cx.mob.block_position())
    }

    fn nearest_bed(brain: &BrainState) -> Option<BlockPos> {
        brain
            .get_memory(MemoryModuleType::NearestBed)
            .and_then(MemoryValue::as_block_pos)
    }

    fn tired_of_walking<M: MobHost>(&self, cx: &MobAi<M>) -> bool {
        !Self::on_or_over_bed(cx) && self.remaining_time_to_reach_bed <= 0
    }

    fn tired_of_jumping<M: MobHost>(&self, cx: &MobAi<M>) -> bool {
        Self::on_or_over_bed(cx) && self.remaining_jumps <= 0
    }
}

impl<M: MobHost> BehaviorBody<M> for JumpOnBed {
    fn check_extra_start_conditions(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        cx.mob.is_baby() && (Self::on_or_over_bed(cx) || Self::nearest_bed(brain).is_some())
    }

    fn start(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        if let Some(pos) = Self::nearest_bed(brain) {
            self.target_bed = Some(pos);
            self.remaining_time_to_reach_bed = MAX_TIME_TO_REACH_BED;
            self.remaining_jumps =
                MIN_JUMPS + cx.mob.level_random_next_int(MAX_JUMPS - MIN_JUMPS + 1);
            self.remaining_cooldown_until_next_jump = 0;
            brain.set_memory_value(
                MemoryModuleType::WalkTarget,
                MemoryValue::WalkTarget(WalkTarget::at_block(pos, self.speed_modifier, 0)),
            );
        }
    }

    fn stop(&mut self, _brain: &mut BrainState, _cx: &mut MobAi<M>, _time: i64) {
        self.target_bed = None;
        self.remaining_time_to_reach_bed = 0;
        self.remaining_jumps = 0;
        self.remaining_cooldown_until_next_jump = 0;
    }

    fn can_still_use(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        cx.mob.is_baby()
            && self.target_bed.is_some_and(|b| cx.mob.is_bed_at(b))
            && !self.tired_of_walking(cx)
            && !self.tired_of_jumping(cx)
    }

    fn timed_out(&self, _end_timestamp: i64, _time: i64) -> bool {
        false
    }

    fn tick(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        if !Self::on_or_over_bed(cx) {
            self.remaining_time_to_reach_bed -= 1;
        }
        if self.remaining_cooldown_until_next_jump > 0 {
            self.remaining_cooldown_until_next_jump -= 1;
            return;
        }
        if Self::on_bed_surface(cx) {
            cx.jump_control.jump();
            self.remaining_jumps -= 1;
            self.remaining_cooldown_until_next_jump = COOLDOWN_BETWEEN_JUMPS;
        }
    }

    fn name(&self) -> &'static str {
        "JumpOnBed"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::{BehaviorControl, Status};
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};
    use rustmc_worldgen::rng::{LegacyRandomSource, RandomSource};

    #[test]
    fn a_baby_walks_to_the_bed_then_jumps_three_to_six_times_five_ticks_apart() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 8));
        let mut twin = LegacyRandomSource::new(8 ^ 0x5DEECE66D);
        let bed = BlockPos { x: 4, y: 1, z: 0 };
        cx.mob.beds = vec![bed];
        let mut brain = Brain::<TestMob>::new(&[T::NearestBed, T::WalkTarget], vec![]).state;
        brain.set_memory_value(T::NearestBed, MemoryValue::BlockPos(bed));
        let mut b = JumpOnBed::new(0.6);
        assert!(!b.try_start(&mut brain, &mut cx, 0), "adults do not");
        cx.mob.baby = true;
        assert!(b.try_start(&mut brain, &mut cx, 0));
        let _ = twin.next_i32_bound(1);
        let jumps = 3 + twin.next_i32_bound(4);
        assert_eq!(b.body.remaining_jumps, jumps);
        assert_eq!(
            brain.get_memory(T::WalkTarget).unwrap().as_walk_target(),
            Some(&WalkTarget::at_block(bed, 0.6, 0))
        );
        for t in 1..=100 {
            b.tick_or_stop(&mut brain, &mut cx, t);
        }
        assert_eq!(b.body.remaining_time_to_reach_bed, 0);
        assert_eq!(BehaviorControl::<TestMob>::status(&b), Status::Running);
        b.tick_or_stop(&mut brain, &mut cx, 101);
        assert_eq!(
            BehaviorControl::<TestMob>::status(&b),
            Status::Stopped,
            "tired of walking"
        );
        assert_eq!(b.body.target_bed, None);
        // On the bed: a jump every sixth tick until the jumps run out.
        cx.mob.x = 4.5;
        brain.erase_memory(T::WalkTarget);
        assert!(b.try_start(&mut brain, &mut cx, 200));
        let _ = twin.next_i32_bound(1);
        let jumps = 3 + twin.next_i32_bound(4);
        brain.erase_memory(T::WalkTarget);
        let mut t = 201;
        let mut jumped = 0;
        while BehaviorControl::<TestMob>::status(&b) == Status::Running {
            b.tick_or_stop(&mut brain, &mut cx, t);
            if cx.jump_control.wants_jump() {
                jumped += 1;
            }
            cx.jump_control.tick(&mut cx.mob);
            t += 1;
            assert!(t < 400, "runaway");
        }
        assert_eq!(jumped, jumps);
        assert_eq!(
            t - 201,
            i64::from(jumps - 1) * 6 + 2,
            "a jump every sixth tick; after the last one the cooldown tick and the exhausted check"
        );
    }
}
