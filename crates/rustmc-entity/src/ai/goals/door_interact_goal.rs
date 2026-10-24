//! `net.minecraft.world.entity.ai.goal.DoorInteractGoal` with its two
//! subclasses `OpenDoorGoal` and `BreakDoorGoal`, from the 26.2 jar. No
//! flags on any of them. Used through `AbstractIllager$RaiderOpenDoorGoal`
//! and `Vindicator$VindicatorBreakDoorGoal` (and the zombie's).
//!
//! # `DoorInteractGoal`
//!
//! Fields `doorPos` (`ZERO`), `hasDoor`, `passed`, `doorOpenDirX/Z`. The
//! constructor throws unless `GoalUtils.hasGroundPathNavigation(mob)`.
//!
//! ```text
//! isOpen()       !hasDoor ? false : (block at doorPos is a DoorBlock ? OPEN : (hasDoor = false, false))
//! setOpen(open)  if hasDoor && block at doorPos is a DoorBlock: DoorBlock.setOpen(mob, level, state, doorPos, open)
//! canUse()       (0..207)
//!   if !hasGroundPathNavigation(mob) return false
//!   if !mob.horizontalCollision return false
//!   path = navigation.getPath(); if path == null || path.isDone() return false
//!   for i in 0 .. min(path.getNextNodeIndex() + 2, path.getNodeCount()):
//!       node = path.getNode(i); doorPos = (node.x, node.y + 1, node.z)
//!       if mob.distanceToSqr(doorPos.x, mob.y, doorPos.z) <= 2.25:
//!           hasDoor = DoorBlock.isWoodenDoor(level, doorPos); if hasDoor return true
//!   doorPos = mob.blockPosition().above(); hasDoor = isWoodenDoor(level, doorPos); return hasDoor
//! canContinueToUse()  !passed
//! start()        passed = false; doorOpenDirX = (float)(doorPos.x + 0.5 - mob.x); doorOpenDirZ likewise
//! tick()         f = (float)(doorPos.x + 0.5 - mob.x); g = (float)(doorPos.z + 0.5 - mob.z)
//!                if doorOpenDirX * f + doorOpenDirZ * g < 0.0f: passed = true
//! ```
//!
//! `DoorBlock.isWoodenDoor` is `block instanceof DoorBlock && type().canOpenByHand()`.
//!
//! # `OpenDoorGoal(mob, closeDoor)`
//!
//! `canContinueToUse`: `closeDoor && forgetTime > 0 && super`; `start`:
//! `forgetTime = 20; setOpen(true)`; `stop`: `setOpen(false)`; `tick`:
//! `--forgetTime; super.tick()`.
//!
//! # `BreakDoorGoal(mob, [doorBreakTime,] validDifficulties)`
//!
//! `DEFAULT_DOOR_BREAK_TIME = 240`, `lastBreakProgress = -1`,
//! `doorBreakTime = -1` unless given; `getDoorBreakTime()` is
//! `max(240, doorBreakTime)`. `canUse`: super, `MOB_GRIEFING`,
//! `validDifficulties.test(difficulty)`, `!isOpen()`. `start`: super,
//! `breakTime = 0`. `canContinueToUse`: `breakTime <= getDoorBreakTime()
//! && !isOpen() && doorPos.closerToCenterThan(position, 2.0) && valid`.
//! `stop`: super, `destroyBlockProgress(id, doorPos, -1)`. `tick`: super;
//! `random.nextInt(20) == 0` (offset 13) sends `levelEvent(1019)` and a
//! swing if not swinging; `++breakTime`; `i = (int)((float) breakTime /
//! (float) getDoorBreakTime() * 10.0f)` sent as progress when it
//! changes; at `breakTime == getDoorBreakTime()` under a valid
//! difficulty the block is removed with events 1021 and 2001.

use rustmc_data::BlockPos;
use rustmc_worldgen::rng::RandomSource;

use crate::ai::goal::{FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::ai::random_pos::has_ground_path_navigation;
use crate::host::{Difficulty, MobHost, PathLevel};
use crate::phys::{above, closer_to_center_than};

/// `BreakDoorGoal.DEFAULT_DOOR_BREAK_TIME`.
pub const DEFAULT_DOOR_BREAK_TIME: i32 = 240;

/// `DoorInteractGoal`'s state.
#[derive(Debug, Clone)]
pub struct DoorInteractGoal {
    pub door_pos: BlockPos,
    pub has_door: bool,
    passed: bool,
    door_open_dir_x: f32,
    door_open_dir_z: f32,
}

impl Default for DoorInteractGoal {
    fn default() -> Self {
        Self::new()
    }
}

impl DoorInteractGoal {
    /// `DoorInteractGoal(Mob)`.
    #[must_use]
    pub fn new() -> Self {
        DoorInteractGoal {
            door_pos: BlockPos { x: 0, y: 0, z: 0 },
            has_door: false,
            passed: false,
            door_open_dir_x: 0.0,
            door_open_dir_z: 0.0,
        }
    }

    /// `isOpen()`.
    pub fn is_open<M: MobHost>(&mut self, cx: &MobAi<M>) -> bool {
        if !self.has_door {
            return false;
        }
        match cx.mob.level().block_facts(self.door_pos).door {
            None => {
                self.has_door = false;
                false
            }
            Some(d) => d.open,
        }
    }

    /// `setOpen(boolean)`.
    pub fn set_open<M: MobHost>(&self, cx: &mut MobAi<M>, open: bool) {
        if self.has_door && cx.mob.level().block_facts(self.door_pos).door.is_some() {
            cx.mob.set_door_open(self.door_pos, open);
        }
    }

    fn is_wooden_door<M: MobHost>(mob: &M, pos: BlockPos) -> bool {
        mob.level()
            .block_facts(pos)
            .door
            .is_some_and(|d| d.can_open_by_hand)
    }

    /// The base `canUse()`.
    pub fn base_can_use<M: MobHost>(&mut self, cx: &mut MobAi<M>) -> bool {
        if !has_ground_path_navigation(cx) {
            return false;
        }
        if !cx.mob.horizontal_collision() {
            return false;
        }
        let Some(path) = cx.navigation.path() else {
            return false;
        };
        if path.is_done() {
            return false;
        }
        let end = (path.next_node_index() + 2).min(path.node_count());
        for i in 0..end {
            let node = path.node(i);
            self.door_pos = BlockPos {
                x: node.x,
                y: node.y + 1,
                z: node.z,
            };
            let d = cx.mob.position().distance_to_sqr3(
                f64::from(self.door_pos.x),
                cx.mob.y(),
                f64::from(self.door_pos.z),
            );
            if d > 2.25 {
                continue;
            }
            self.has_door = Self::is_wooden_door(&cx.mob, self.door_pos);
            if self.has_door {
                return true;
            }
        }
        self.door_pos = above(cx.mob.block_position());
        self.has_door = Self::is_wooden_door(&cx.mob, self.door_pos);
        self.has_door
    }

    /// The base `canContinueToUse()`.
    #[must_use]
    pub fn base_can_continue_to_use(&self) -> bool {
        !self.passed
    }

    /// The base `start()`.
    pub fn base_start<M: MobHost>(&mut self, cx: &MobAi<M>) {
        self.passed = false;
        self.door_open_dir_x = (f64::from(self.door_pos.x) + 0.5 - cx.mob.x()) as f32;
        self.door_open_dir_z = (f64::from(self.door_pos.z) + 0.5 - cx.mob.z()) as f32;
    }

    /// The base `tick()`.
    pub fn base_tick<M: MobHost>(&mut self, cx: &MobAi<M>) {
        let f = (f64::from(self.door_pos.x) + 0.5 - cx.mob.x()) as f32;
        let g = (f64::from(self.door_pos.z) + 0.5 - cx.mob.z()) as f32;
        let h = self.door_open_dir_x * f + self.door_open_dir_z * g;
        if h < 0.0 {
            self.passed = true;
        }
    }
}

impl<M: MobHost> Goal<MobAi<M>> for DoorInteractGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        self.base_can_use(cx)
    }

    fn can_continue_to_use(&mut self, _cx: &mut MobAi<M>) -> bool {
        self.base_can_continue_to_use()
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        self.base_start(cx);
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn tick(&mut self, cx: &mut MobAi<M>) {
        self.base_tick(cx);
    }

    fn flags(&self) -> FlagSet {
        FlagSet::NONE
    }
}

/// `OpenDoorGoal`.
#[derive(Debug, Clone)]
pub struct OpenDoorGoal {
    pub inner: DoorInteractGoal,
    close_door: bool,
    forget_time: i32,
}

impl OpenDoorGoal {
    /// `OpenDoorGoal(Mob, boolean closeDoor)`.
    #[must_use]
    pub fn new(close_door: bool) -> Self {
        OpenDoorGoal {
            inner: DoorInteractGoal::new(),
            close_door,
            forget_time: 0,
        }
    }
}

impl<M: MobHost> Goal<MobAi<M>> for OpenDoorGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        self.inner.base_can_use(cx)
    }

    fn can_continue_to_use(&mut self, _cx: &mut MobAi<M>) -> bool {
        self.close_door && self.forget_time > 0 && self.inner.base_can_continue_to_use()
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        self.forget_time = 20;
        self.inner.set_open(cx, true);
    }

    fn stop(&mut self, cx: &mut MobAi<M>) {
        self.inner.set_open(cx, false);
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn tick(&mut self, cx: &mut MobAi<M>) {
        self.forget_time -= 1;
        self.inner.base_tick(cx);
    }

    fn flags(&self) -> FlagSet {
        FlagSet::NONE
    }
}

/// `BreakDoorGoal`.
pub struct BreakDoorGoal {
    pub inner: DoorInteractGoal,
    valid_difficulties: fn(Difficulty) -> bool,
    pub break_time: i32,
    pub last_break_progress: i32,
    pub door_break_time: i32,
}

impl BreakDoorGoal {
    /// `BreakDoorGoal(Mob, Predicate<Difficulty>)`.
    #[must_use]
    pub fn new(valid_difficulties: fn(Difficulty) -> bool) -> Self {
        BreakDoorGoal {
            inner: DoorInteractGoal::new(),
            valid_difficulties,
            break_time: 0,
            last_break_progress: -1,
            door_break_time: -1,
        }
    }

    /// `BreakDoorGoal(Mob, int doorBreakTime, Predicate<Difficulty>)`.
    #[must_use]
    pub fn with_break_time(
        door_break_time: i32,
        valid_difficulties: fn(Difficulty) -> bool,
    ) -> Self {
        let mut g = Self::new(valid_difficulties);
        g.door_break_time = door_break_time;
        g
    }

    /// `getDoorBreakTime()`.
    #[must_use]
    pub fn get_door_break_time(&self) -> i32 {
        DEFAULT_DOOR_BREAK_TIME.max(self.door_break_time)
    }

    fn is_valid_difficulty<M: MobHost>(&self, cx: &MobAi<M>) -> bool {
        (self.valid_difficulties)(cx.mob.difficulty())
    }
}

impl<M: MobHost> Goal<MobAi<M>> for BreakDoorGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        if !self.inner.base_can_use(cx) {
            return false;
        }
        if !cx.mob.mob_griefing() {
            return false;
        }
        self.is_valid_difficulty(cx) && !self.inner.is_open(cx)
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        self.inner.base_start(cx);
        self.break_time = 0;
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        self.break_time <= self.get_door_break_time()
            && !self.inner.is_open(cx)
            && closer_to_center_than(self.inner.door_pos, cx.mob.position(), 2.0)
            && self.is_valid_difficulty(cx)
    }

    fn stop(&mut self, cx: &mut MobAi<M>) {
        let p = self.inner.door_pos;
        cx.mob.destroy_block_progress(p, -1);
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    fn tick(&mut self, cx: &mut MobAi<M>) {
        self.inner.base_tick(cx);
        let p = self.inner.door_pos;
        if cx.mob.random().next_i32_bound(20) == 0 {
            cx.mob.level_event(1019, p, 0);
            if !cx.mob.swinging() {
                cx.mob.swing_used_item_hand();
            }
        }
        self.break_time += 1;
        let i = (self.break_time as f32 / self.get_door_break_time() as f32 * 10.0) as i32;
        if i != self.last_break_progress {
            cx.mob.destroy_block_progress(p, i);
            self.last_break_progress = i;
        }
        if self.break_time == self.get_door_break_time() && self.is_valid_difficulty(cx) {
            cx.mob.remove_block(p);
            cx.mob.level_event(1021, p, 0);
            cx.mob.level_event_block_broken(p);
        }
    }

    fn flags(&self) -> FlagSet {
        FlagSet::NONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{BlockFacts, DoorFacts};
    use crate::testing::{GridLevel, TestMob};

    fn door(open: bool, wooden: bool) -> BlockFacts {
        let mut b = BlockFacts::NONE;
        b.door = Some(DoorFacts {
            open,
            can_open_by_hand: wooden,
        });
        b.is_pathfindable_land = true;
        b
    }

    /// With no path the fallback door is the block above the mob; an
    /// iron door is not a door for this goal.
    #[test]
    fn fallback_door_is_the_block_above_the_mob() {
        let mut g = GridLevel::new();
        g.floor(-10, -10, 10, 10, 63);
        g.set(0, 65, 0, door(false, true));
        let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, 1);
        mob.horizontal_collision = true;
        let mut ai = MobAi::new(mob);
        let mut goal = OpenDoorGoal::new(true);
        assert!(
            !goal.can_use(&mut ai),
            "no path: refused before the fallback"
        );
        // A path standing on the mob's own block, not done.
        ai.navigation.move_to(&mut ai.mob, 3.5, 64.0, 0.5, 1.0);
        assert!(ai.navigation.is_in_progress());
        assert!(goal.can_use(&mut ai));
        assert_eq!(goal.inner.door_pos, BlockPos { x: 0, y: 65, z: 0 });
        goal.start(&mut ai);
        assert!(ai.mob.calls.iter().any(|c| c == "setOpen(0, 65, 0, true)"));
        assert!(goal.can_continue_to_use(&mut ai));
        for _ in 0..20 {
            goal.tick(&mut ai);
        }
        assert!(!goal.can_continue_to_use(&mut ai), "forgetTime ran out");
        goal.stop(&mut ai);
        assert_eq!(
            ai.mob.calls.last().map(String::as_str),
            Some("setOpen(0, 65, 0, false)")
        );
        ai.mob.level.set(0, 65, 0, door(false, false));
        assert!(!goal.can_use(&mut ai), "iron door");
    }

    /// Breaking: 240 ticks, progress in tenths, the block goes at 240
    /// under a valid difficulty; each tick draws `nextInt(20)`.
    #[test]
    fn break_door_progress_and_removal() {
        let mut g = GridLevel::new();
        g.floor(-10, -10, 10, 10, 63);
        g.set(0, 65, 0, door(false, true));
        let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, 1);
        mob.horizontal_collision = true;
        mob.difficulty = Difficulty::Hard;
        let mut ai = MobAi::new(mob);
        ai.navigation.move_to(&mut ai.mob, 3.5, 64.0, 0.5, 1.0);
        let mut goal = BreakDoorGoal::new(|d| d == Difficulty::Hard);
        assert!(goal.can_use(&mut ai));
        goal.start(&mut ai);
        for t in 1..=239 {
            assert!(goal.can_continue_to_use(&mut ai), "tick {t}");
            goal.tick(&mut ai);
            assert!(!ai.mob.calls.iter().any(|c| c.starts_with("removeBlock")));
        }
        assert_eq!(goal.last_break_progress, 9);
        goal.tick(&mut ai);
        assert!(ai.mob.calls.iter().any(|c| c == "removeBlock(0, 65, 0)"));
        assert!(ai
            .mob
            .calls
            .iter()
            .any(|c| c == "levelEvent(1021, 0, 65, 0, 0)"));
        assert!(ai
            .mob
            .calls
            .iter()
            .any(|c| c == "destroyBlockProgress(0, 65, 0, 10)"));
        ai.mob.difficulty = Difficulty::Easy;
        assert!(!goal.can_continue_to_use(&mut ai));
        goal.stop(&mut ai);
        assert_eq!(
            ai.mob.calls.last().map(String::as_str),
            Some("destroyBlockProgress(0, 65, 0, -1)")
        );
    }
}
