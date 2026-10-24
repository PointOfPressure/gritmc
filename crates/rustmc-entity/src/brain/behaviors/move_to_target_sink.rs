//! `net.minecraft.world.entity.ai.behavior.MoveToTargetSink`, from the 26.2
//! jar. `Behavior<Mob>` with entry `{CANT_REACH_WALK_TARGET_SINCE: REGISTERED,
//! PATH: VALUE_ABSENT, WALK_TARGET: VALUE_PRESENT}`; `MoveToTargetSink()` is
//! `(150, 250)`.
//!
//! ```text
//! checkExtraStartConditions (0..103):
//!   if remainingCooldown > 0: remainingCooldown--; return false
//!   walk = brain.getMemory(WALK_TARGET).get()
//!   reached = reachedTarget(mob, walk)
//!   if !reached && tryComputePath(mob, walk, level.getGameTime()):
//!       lastTargetPos = walk.getTarget().currentBlockPosition(); return true
//!   brain.eraseMemory(WALK_TARGET); if reached: brain.eraseMemory(CANT_REACH_WALK_TARGET_SINCE); return false
//! canStillUse (0..101):
//!   if path == null || lastTargetPos == null: return false
//!   walk = brain.getMemory(WALK_TARGET); spectator = walk.map(isWalkTargetSpectator).orElse(false)
//!   return !nav.isDone() && walk.isPresent() && !reachedTarget(mob, walk.get()) && !spectator
//! start (0..31):   brain.setMemory(PATH, path); nav.moveTo(path, speedModifier)
//! tick (0..128):
//!   navPath = nav.getPath()
//!   if path != navPath (identity): path = navPath; brain.setMemory(PATH, navPath)
//!   if navPath == null || lastTargetPos == null: return
//!   walk = brain.getMemory(WALK_TARGET).get()
//!   if walk.getTarget().currentBlockPosition().distSqr(lastTargetPos) > 4.0 && tryComputePath(mob, walk, gameTime):
//!       lastTargetPos = walk.getTarget().currentBlockPosition(); start(level, mob, time)
//! stop (0..94):
//!   if brain.hasMemoryValue(WALK_TARGET) && !reachedTarget(mob, walk) && nav.isStuck():
//!       remainingCooldown = level.getRandom().nextInt(40)          <- MAX_COOLDOWN_BEFORE_RETRYING, level random
//!   nav.stop(); brain.eraseMemory(WALK_TARGET); brain.eraseMemory(PATH); path = null
//! tryComputePath (0..198):
//!   pos = walk.getTarget().currentBlockPosition()
//!   path = nav.createPath(pos, 0); speedModifier = walk.getSpeedModifier()
//!   if reachedTarget(mob, walk): brain.eraseMemory(CANT_REACH...); return false      <- 56: goto 197
//!   canReach = path != null && path.canReach()
//!   if canReach: brain.eraseMemory(CANT_REACH...)
//!   else if !brain.hasMemoryValue(CANT_REACH...): brain.setMemory(CANT_REACH..., gameTime)
//!   if path != null: return true
//!   v = DefaultRandomPos.getPosTowards((PathfinderMob) mob, 10, 7, Vec3.atBottomCenterOf(pos), PI/2)
//!   if v != null: path = nav.createPath(v.x, v.y, v.z, 0); return path != null
//!   return false
//! reachedTarget:   walk.target.currentBlockPosition().distManhattan(mob.blockPosition()) <= walk.closeEnoughDist
//! ```
//!
//! Draws: `tryComputePath`'s `getPosTowards` walks the mob's random (see
//! `random_pos`); `stop` draws `nextInt(40)` from the level's when stuck;
//! the base class draws the duration on every start. The `path != navPath`
//! identity test is [`crate::ai::GroundPathNavigation::path_generation`].

use rustmc_data::BlockPos;

use crate::ai::goals::move_towards_restriction_goal::HALF_PI_F32_AS_F64;
use crate::ai::mob_ai::MobAi;
use crate::ai::random_pos::default_get_pos_towards;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::memory::{MemoryModuleType, MemoryStatus, MemoryValue, WalkTarget};
use crate::brain::BrainState;
use crate::host::{MobHost, PathLevel};
use crate::pathfinder::Path;
use crate::phys::{block_pos_containing, Vec3};

/// `MAX_COOLDOWN_BEFORE_RETRYING`.
pub const MAX_COOLDOWN_BEFORE_RETRYING: i32 = 40;

/// `MoveToTargetSink`'s body; build one with [`MoveToTargetSink::new`].
#[derive(Debug, Clone, Default)]
pub struct MoveToTargetSink {
    remaining_cooldown: i32,
    path: Option<Path>,
    /// The navigation's `path_generation` when `path` was last read from
    /// it, for the identity comparison in `tick`.
    path_gen: Option<u64>,
    last_target_pos: Option<BlockPos>,
    speed_modifier: f32,
}

impl MoveToTargetSink {
    /// `MoveToTargetSink()` — `(150, 250)`.
    #[must_use]
    pub fn new() -> Behavior<MoveToTargetSink> {
        Self::with_durations(150, 250)
    }

    /// `MoveToTargetSink(int, int)`.
    #[must_use]
    pub fn with_durations(min: i32, max: i32) -> Behavior<MoveToTargetSink> {
        Behavior::with_durations(
            MoveToTargetSink::default(),
            vec![
                (
                    MemoryModuleType::CantReachWalkTargetSince,
                    MemoryStatus::Registered,
                ),
                (MemoryModuleType::Path, MemoryStatus::ValueAbsent),
                (MemoryModuleType::WalkTarget, MemoryStatus::ValuePresent),
            ],
            min,
            max,
        )
    }

    /// `remainingCooldown`, for tests.
    #[must_use]
    pub fn remaining_cooldown(&self) -> i32 {
        self.remaining_cooldown
    }

    fn walk_target(brain: &BrainState) -> Option<WalkTarget> {
        brain
            .get_memory(MemoryModuleType::WalkTarget)
            .and_then(|v| v.as_walk_target())
            .cloned()
    }

    /// `reachedTarget(mob, walk)`.
    fn reached_target<M: MobHost>(cx: &MobAi<M>, walk: &WalkTarget) -> bool {
        let Some(p) = walk.target.current_block_position(&cx.mob) else {
            return false;
        };
        let m = cx.mob.block_position();
        let dist = (p.x - m.x).abs() + (p.y - m.y).abs() + (p.z - m.z).abs();
        dist <= walk.close_enough_dist
    }

    /// `isWalkTargetSpectator(walk)`.
    fn is_walk_target_spectator<M: MobHost>(cx: &MobAi<M>, walk: &WalkTarget) -> bool {
        walk.target
            .entity_id()
            .and_then(|id| cx.mob.entity(id))
            .is_some_and(|e| e.is_spectator)
    }

    /// `tryComputePath(mob, walk, gameTime)`.
    fn try_compute_path<M: MobHost>(
        &mut self,
        brain: &mut BrainState,
        cx: &mut MobAi<M>,
        walk: &WalkTarget,
        time: i64,
    ) -> bool {
        let Some(pos) = walk.target.current_block_position(&cx.mob) else {
            return false;
        };
        self.path = cx.navigation.create_path(&mut cx.mob, pos, 0);
        self.speed_modifier = walk.speed_modifier;
        if Self::reached_target(cx, walk) {
            brain.erase_memory(MemoryModuleType::CantReachWalkTargetSince);
            return false;
        }
        let can_reach = self.path.as_ref().is_some_and(Path::can_reach);
        if can_reach {
            brain.erase_memory(MemoryModuleType::CantReachWalkTargetSince);
        } else if !brain.has_memory_value(MemoryModuleType::CantReachWalkTargetSince) {
            brain.set_memory_value(
                MemoryModuleType::CantReachWalkTargetSince,
                MemoryValue::Long(time),
            );
        }
        if self.path.is_some() {
            return true;
        }
        let towards = Vec3::at_bottom_center_of(pos);
        if let Some(v) = default_get_pos_towards(cx, 10, 7, towards, HALF_PI_F32_AS_F64) {
            self.path =
                cx.navigation
                    .create_path(&mut cx.mob, block_pos_containing(v.x, v.y, v.z), 0);
            return self.path.is_some();
        }
        false
    }
}

impl<M: MobHost> BehaviorBody<M> for MoveToTargetSink {
    fn check_extra_start_conditions(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        if self.remaining_cooldown > 0 {
            self.remaining_cooldown -= 1;
            return false;
        }
        let walk = Self::walk_target(brain).expect("WALK_TARGET is an entry condition");
        let reached = Self::reached_target(cx, &walk);
        let time = cx.mob.level().game_time();
        if !reached && self.try_compute_path(brain, cx, &walk, time) {
            self.last_target_pos = walk.target.current_block_position(&cx.mob);
            return true;
        }
        brain.erase_memory(MemoryModuleType::WalkTarget);
        if reached {
            brain.erase_memory(MemoryModuleType::CantReachWalkTargetSince);
        }
        false
    }

    fn can_still_use(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        if self.path.is_none() || self.last_target_pos.is_none() {
            return false;
        }
        let walk = Self::walk_target(brain);
        let spectator = walk
            .as_ref()
            .is_some_and(|w| Self::is_walk_target_spectator(cx, w));
        !cx.navigation.is_done()
            && walk.as_ref().is_some_and(|w| !Self::reached_target(cx, w))
            && !spectator
    }

    fn start(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        brain.set_memory(
            MemoryModuleType::Path,
            self.path.clone().map(MemoryValue::Path),
        );
        let moved = cx.navigation.move_along(
            &mut cx.mob,
            self.path.clone(),
            f64::from(self.speed_modifier),
        );
        let _ = moved;
        self.path_gen = Some(cx.navigation.path_generation());
    }

    fn tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        let nav_gen = cx.navigation.path_generation();
        if self.path_gen != Some(nav_gen) {
            self.path = cx.navigation.path().cloned();
            self.path_gen = Some(nav_gen);
            brain.set_memory(
                MemoryModuleType::Path,
                self.path.clone().map(MemoryValue::Path),
            );
        }
        if self.path.is_none() || self.last_target_pos.is_none() {
            return;
        }
        let walk = Self::walk_target(brain).expect("WALK_TARGET present while running");
        let Some(cur) = walk.target.current_block_position(&cx.mob) else {
            return;
        };
        let last = self.last_target_pos.expect("checked");
        let dx = f64::from(cur.x - last.x);
        let dy = f64::from(cur.y - last.y);
        let dz = f64::from(cur.z - last.z);
        if dx * dx + dy * dy + dz * dz > 4.0 {
            let game_time = cx.mob.level().game_time();
            if self.try_compute_path(brain, cx, &walk, game_time) {
                self.last_target_pos = walk.target.current_block_position(&cx.mob);
                BehaviorBody::<M>::start(self, brain, cx, time);
            }
        }
    }

    fn stop(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        if brain.has_memory_value(MemoryModuleType::WalkTarget) {
            let walk = Self::walk_target(brain).expect("present");
            if !Self::reached_target(cx, &walk) && cx.navigation.is_stuck() {
                self.remaining_cooldown =
                    cx.mob.level_random_next_int(MAX_COOLDOWN_BEFORE_RETRYING);
            }
        }
        cx.navigation.stop();
        brain.erase_memory(MemoryModuleType::WalkTarget);
        brain.erase_memory(MemoryModuleType::Path);
        self.path = None;
        self.path_gen = None;
    }

    fn name(&self) -> &'static str {
        "MoveToTargetSink"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::{BehaviorControl, Status};
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};

    fn world() -> MobAi<TestMob> {
        let mut level = GridLevel::new();
        level.floor(-8, -8, 8, 8, 0);
        MobAi::new(TestMob::new(level, 0.5, 1.0, 0.5, 11))
    }

    fn brain() -> BrainState {
        Brain::<TestMob>::new(
            &[T::WalkTarget, T::Path, T::CantReachWalkTargetSince],
            vec![],
        )
        .state
    }

    #[test]
    fn a_reachable_walk_target_starts_publishes_the_path_and_walks() {
        let mut cx = world();
        let mut brain = brain();
        let mut b = MoveToTargetSink::new();
        brain.set_memory_value(
            T::WalkTarget,
            MemoryValue::WalkTarget(WalkTarget::at_block(BlockPos { x: 5, y: 1, z: 0 }, 0.5, 0)),
        );
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(brain.has_memory_value(T::Path), "start() writes PATH");
        assert!(!brain.has_memory_value(T::CantReachWalkTargetSince));
        assert!(!cx.navigation.is_done());
        assert_eq!(cx.navigation.speed_modifier_for_test(), 0.5);
        b.tick_or_stop(&mut brain, &mut cx, 1);
        assert_eq!(BehaviorControl::<TestMob>::status(&b), Status::Running);
    }

    #[test]
    fn an_already_reached_target_is_erased_without_starting() {
        let mut cx = world();
        let mut brain = brain();
        let mut b = MoveToTargetSink::new();
        brain.set_memory_value(
            T::WalkTarget,
            MemoryValue::WalkTarget(WalkTarget::at_block(BlockPos { x: 1, y: 1, z: 0 }, 0.5, 1)),
        );
        brain.set_memory_value(T::CantReachWalkTargetSince, MemoryValue::Long(3));
        assert!(!b.try_start(&mut brain, &mut cx, 0));
        assert!(!brain.has_memory_value(T::WalkTarget));
        assert!(
            !brain.has_memory_value(T::CantReachWalkTargetSince),
            "reached erases the stamp"
        );
    }

    #[test]
    fn a_cooldown_counts_down_one_per_ask_before_anything_else() {
        let mut cx = world();
        let mut brain = brain();
        let mut b = MoveToTargetSink::new();
        b.body.remaining_cooldown = 2;
        brain.set_memory_value(
            T::WalkTarget,
            MemoryValue::WalkTarget(WalkTarget::at_block(BlockPos { x: 5, y: 1, z: 0 }, 0.5, 0)),
        );
        assert!(!b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(b.body.remaining_cooldown(), 1);
        assert!(
            brain.has_memory_value(T::WalkTarget),
            "the target is not erased on cooldown"
        );
        assert!(!b.try_start(&mut brain, &mut cx, 1));
        assert!(b.try_start(&mut brain, &mut cx, 2));
    }

    #[test]
    fn stop_erases_walk_target_and_path_and_stops_the_navigation() {
        let mut cx = world();
        let mut brain = brain();
        let mut b = MoveToTargetSink::new();
        brain.set_memory_value(
            T::WalkTarget,
            MemoryValue::WalkTarget(WalkTarget::at_block(BlockPos { x: 5, y: 1, z: 0 }, 0.5, 0)),
        );
        assert!(b.try_start(&mut brain, &mut cx, 0));
        b.do_stop(&mut brain, &mut cx, 1);
        assert!(!brain.has_memory_value(T::WalkTarget));
        assert!(!brain.has_memory_value(T::Path));
        assert!(cx.navigation.is_done());
        assert_eq!(
            b.body.remaining_cooldown(),
            0,
            "not stuck: no cooldown draw"
        );
    }
}
