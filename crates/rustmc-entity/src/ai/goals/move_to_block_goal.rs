//! `net.minecraft.world.entity.ai.goal.MoveToBlockGoal` and its three
//! subclasses in the goal package, `CatSitOnBlockGoal`, `CatLieOnBedGoal`
//! and `RemoveBlockGoal`, from the 26.2 jar. Flags `MOVE, JUMP`.
//!
//! # `MoveToBlockGoal`
//!
//! `GIVE_UP_TICKS = STAY_TICKS = 1200`, `INTERVAL_TICKS = 200`. Fields:
//! `nextStartTick`, `tryTicks`, `maxStayTicks`, `blockPos` (`ZERO`),
//! `reachedTarget`, `searchRange`, `verticalSearchRange`,
//! `verticalSearchStart` (0).
//!
//! ```text
//! canUse()               if nextStartTick > 0: --nextStartTick; return false
//!                        nextStartTick = nextStartTick(mob); return findNearestBlock()
//! nextStartTick(mob)     reducedTickDelay(200 + mob.getRandom().nextInt(200))
//! canContinueToUse()     tryTicks >= -maxStayTicks && tryTicks <= 1200
//!                          && isValidTarget(level, blockPos)
//! start()                moveMobToBlock(); tryTicks = 0
//!                        maxStayTicks = random.nextInt(random.nextInt(1200) + 1200) + 1200
//! moveMobToBlock()       navigation.moveTo(blockPos.x + 0.5, blockPos.y + 1, blockPos.z + 0.5, speed)
//! acceptedDistance()     1.0;  getMoveToTarget()  blockPos.above()
//! tick()                 t = getMoveToTarget()
//!                        if !t.closerToCenterThan(mob.position(), acceptedDistance()):
//!                            reachedTarget = false; ++tryTicks
//!                            if shouldRecalculatePath(): navigation.moveTo(t.x + 0.5, t.y, t.z + 0.5, speed)
//!                        else: reachedTarget = true; --tryTicks
//! shouldRecalculatePath() tryTicks % 40 == 0
//! ```
//!
//! `findNearestBlock()`, offsets 0..206, walks `k` from
//! `verticalSearchStart` outward (`k > 0 ? -k : 1 - k`) to
//! `verticalSearchRange`, `l` from 0 to `searchRange - 1`, `m` outward
//! to `±l`, and `n` outward to `±l` starting at `l` when `-l < m < l`
//! (the ring) and at 0 otherwise; the offset is `(m, k - 1, n)`, and
//! the first candidate `isWithinHome` and `isValidTarget` wins.
//!
//! # `CatSitOnBlockGoal(cat, speed)`: `super(cat, speed, 8)`
//!
//! `canUse`: `cat.isTame() && !cat.isOrderedToSit() && super.canUse()`.
//! `start`/`stop`: super then `setInSittingPose(false)`; `tick`: super
//! then `setInSittingPose(isReachedTarget())`. `isValidTarget`: the
//! block above `isEmptyBlock`, then a chest with `getOpenCount < 1`, a
//! lit furnace, or a bed whose `PART` is not `HEAD`.
//!
//! # `CatLieOnBedGoal(cat, speed, searchRange)`: `super(cat, speed, range, 6)`
//!
//! `verticalSearchStart = -2`, flags re-set to `JUMP, MOVE`. `canUse`:
//! tame, not ordered to sit, not lying, then super. `nextStartTick` is
//! the constant 40. `start`: super, `setInSittingPose(false)`; `stop`:
//! super, `setLying(false)`; `tick`: super, `setInSittingPose(false)`,
//! then `setLying(false)` when not reached else `setLying(true)` when
//! not already lying. `isValidTarget`: empty above and `is(BEDS)`.
//!
//! # `RemoveBlockGoal(block, mob, speed, verticalSearchRange)`: `super(mob, speed, 24, v)`
//!
//! `canUse`: `MOB_GRIEFING` or false; the countdown as the base; then
//! `findNearestBlock() ? (nextStartTick = reducedTickDelay(20); true)
//! : (nextStartTick = nextStartTick(mob); false)`. `stop`: super, then
//! `fallDistance = 1.0`. `tick`: super; when `isReachedTarget()` and a
//! block is found at the mob's position or below, west, east, north,
//! south, two below (in that order): after the first tick a `0.3`
//! upward kick and three egg particles (three `nextFloat` draws); every
//! second tick a `-0.3` kick, every sixth the progress sound; past 60
//! ticks the block is removed, twenty poof particles are sent (three
//! `nextGaussian` each) and the break sound plays; then `++ticksSinceReachedGoal`.
//! `isValidTarget`: the chunk loaded, the block `is(blockToRemove)`, and
//! two air blocks above it.

use rustmc_data::BlockPos;
use rustmc_worldgen::rng::RandomSource;

use crate::ai::goal::{reduced_tick_delay, Flag, FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::host::{MobHost, PathLevel};
use crate::phys::{above, below, closer_to_center_than, offset};

/// `GIVE_UP_TICKS` / `STAY_TICKS`.
pub const GIVE_UP_TICKS: i32 = 1200;
/// `INTERVAL_TICKS`.
pub const INTERVAL_TICKS: i32 = 200;
/// `RemoveBlockGoal.WAIT_AFTER_BLOCK_FOUND`.
pub const WAIT_AFTER_BLOCK_FOUND: i32 = 20;

/// Which `isValidTarget` / `nextStartTick` a move-to-block goal has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveToBlockKind {
    /// `CatSitOnBlockGoal`.
    CatSitOnBlock,
    /// `CatLieOnBedGoal`.
    CatLieOnBed,
    /// `RemoveBlockGoal` with its `blockToRemove`'s registry id.
    RemoveBlock { block: &'static str },
}

/// `MoveToBlockGoal`.
#[derive(Debug, Clone)]
pub struct MoveToBlockGoal {
    kind: MoveToBlockKind,
    pub speed_modifier: f64,
    next_start_tick: i32,
    try_ticks: i32,
    max_stay_ticks: i32,
    block_pos: BlockPos,
    reached_target: bool,
    search_range: i32,
    vertical_search_range: i32,
    vertical_search_start: i32,
}

impl MoveToBlockGoal {
    /// `MoveToBlockGoal(PathfinderMob, double, int searchRange, int verticalSearchRange)`.
    #[must_use]
    pub fn new(
        kind: MoveToBlockKind,
        speed_modifier: f64,
        search_range: i32,
        vertical_search_range: i32,
    ) -> Self {
        MoveToBlockGoal {
            kind,
            speed_modifier,
            next_start_tick: 0,
            try_ticks: 0,
            max_stay_ticks: 0,
            block_pos: BlockPos { x: 0, y: 0, z: 0 },
            reached_target: false,
            search_range,
            vertical_search_range,
            vertical_search_start: 0,
        }
    }

    /// `nextStartTick(PathfinderMob)`.
    fn next_start_tick<M: MobHost>(&self, cx: &mut MobAi<M>) -> i32 {
        match self.kind {
            MoveToBlockKind::CatLieOnBed => 40,
            _ => reduced_tick_delay(INTERVAL_TICKS + cx.mob.random().next_i32_bound(200)),
        }
    }

    /// `isValidTarget(LevelReader, BlockPos)`, by kind.
    fn is_valid_target<M: MobHost>(&self, mob: &M, pos: BlockPos) -> bool {
        let level = mob.level();
        match self.kind {
            MoveToBlockKind::CatSitOnBlock => {
                if !level.block_facts(above(pos)).is_air {
                    return false;
                }
                let f = level.block_facts(pos);
                if let Some(n) = f.chest_open_count {
                    return n < 1;
                }
                if f.furnace_lit == Some(true) {
                    return true;
                }
                f.is_bed && !f.bed_part_head
            }
            MoveToBlockKind::CatLieOnBed => {
                level.block_facts(above(pos)).is_air && level.block_facts(pos).is_bed
            }
            MoveToBlockKind::RemoveBlock { block } => {
                if !level.is_chunk_loaded(pos.x >> 4, pos.z >> 4) {
                    return false;
                }
                level.block_facts(pos).block_id == Some(block)
                    && level.block_facts(above(pos)).is_air
                    && level.block_facts(offset(pos, 0, 2, 0)).is_air
            }
        }
    }

    /// `findNearestBlock()`.
    fn find_nearest_block<M: MobHost>(&mut self, cx: &MobAi<M>) -> bool {
        let i = self.search_range;
        let j = self.vertical_search_range;
        let pos = cx.mob.block_position();
        let mut k = self.vertical_search_start;
        while k <= j {
            let mut l = 0;
            while l < i {
                let mut m = 0;
                while m <= l {
                    let mut n = if m < l && m > -l { l } else { 0 };
                    while n <= l {
                        let p = offset(pos, m, k - 1, n);
                        if cx.mob.is_within_home_pos(p) && self.is_valid_target(&cx.mob, p) {
                            self.block_pos = p;
                            return true;
                        }
                        n = if n > 0 { -n } else { 1 - n };
                    }
                    m = if m > 0 { -m } else { 1 - m };
                }
                l += 1;
            }
            k = if k > 0 { -k } else { 1 - k };
        }
        false
    }

    /// `moveMobToBlock()`.
    fn move_mob_to_block<M: MobHost>(&self, cx: &mut MobAi<M>) {
        let b = self.block_pos;
        cx.navigation.move_to(
            &mut cx.mob,
            f64::from(b.x) + 0.5,
            f64::from(b.y + 1),
            f64::from(b.z) + 0.5,
            self.speed_modifier,
        );
    }

    /// `acceptedDistance()`: `1.0`.
    #[must_use]
    pub fn accepted_distance(&self) -> f64 {
        1.0
    }

    /// `getMoveToTarget()`: `blockPos.above()`.
    #[must_use]
    pub fn move_to_target(&self) -> BlockPos {
        above(self.block_pos)
    }

    /// `shouldRecalculatePath()`.
    #[must_use]
    pub fn should_recalculate_path(&self) -> bool {
        self.try_ticks % 40 == 0
    }

    /// `isReachedTarget()`.
    #[must_use]
    pub fn is_reached_target(&self) -> bool {
        self.reached_target
    }

    /// `blockPos`, for tests.
    #[must_use]
    pub fn block_pos(&self) -> BlockPos {
        self.block_pos
    }

    /// `nextStartTick`, for tests.
    #[must_use]
    pub fn next_start_tick_value(&self) -> i32 {
        self.next_start_tick
    }

    /// `tryTicks`, for tests.
    #[must_use]
    pub fn try_ticks(&self) -> i32 {
        self.try_ticks
    }

    /// The base `canUse()`.
    fn base_can_use<M: MobHost>(&mut self, cx: &mut MobAi<M>) -> bool {
        if self.next_start_tick > 0 {
            self.next_start_tick -= 1;
            return false;
        }
        self.next_start_tick = self.next_start_tick(cx);
        self.find_nearest_block(cx)
    }

    fn base_can_continue_to_use<M: MobHost>(&mut self, cx: &mut MobAi<M>) -> bool {
        self.try_ticks >= -self.max_stay_ticks
            && self.try_ticks <= GIVE_UP_TICKS
            && self.is_valid_target(&cx.mob, self.block_pos)
    }

    fn base_start<M: MobHost>(&mut self, cx: &mut MobAi<M>) {
        self.move_mob_to_block(cx);
        self.try_ticks = 0;
        let inner = cx.mob.random().next_i32_bound(1200) + 1200;
        self.max_stay_ticks = cx.mob.random().next_i32_bound(inner) + 1200;
    }

    fn base_tick<M: MobHost>(&mut self, cx: &mut MobAi<M>) {
        let t = self.move_to_target();
        if !closer_to_center_than(t, cx.mob.position(), self.accepted_distance()) {
            self.reached_target = false;
            self.try_ticks += 1;
            if self.should_recalculate_path() {
                cx.navigation.move_to(
                    &mut cx.mob,
                    f64::from(t.x) + 0.5,
                    f64::from(t.y),
                    f64::from(t.z) + 0.5,
                    self.speed_modifier,
                );
            }
        } else {
            self.reached_target = true;
            self.try_ticks -= 1;
        }
    }
}

impl<M: MobHost> Goal<MobAi<M>> for MoveToBlockGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        self.base_can_use(cx)
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        self.base_can_continue_to_use(cx)
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
        FlagSet::of2(Flag::Move, Flag::Jump)
    }
}

/// `CatSitOnBlockGoal`.
#[derive(Debug, Clone)]
pub struct CatSitOnBlockGoal {
    pub inner: MoveToBlockGoal,
}

impl CatSitOnBlockGoal {
    /// `CatSitOnBlockGoal(Cat, double)`: `super(cat, speed, 8)` (vertical 1).
    #[must_use]
    pub fn new(speed_modifier: f64) -> Self {
        CatSitOnBlockGoal {
            inner: MoveToBlockGoal::new(MoveToBlockKind::CatSitOnBlock, speed_modifier, 8, 1),
        }
    }
}

impl<M: MobHost> Goal<MobAi<M>> for CatSitOnBlockGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        cx.mob.is_tame() && !cx.mob.is_ordered_to_sit() && self.inner.base_can_use(cx)
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        self.inner.base_can_continue_to_use(cx)
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        self.inner.base_start(cx);
        cx.mob.set_in_sitting_pose(false);
    }

    fn stop(&mut self, cx: &mut MobAi<M>) {
        cx.mob.set_in_sitting_pose(false);
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn tick(&mut self, cx: &mut MobAi<M>) {
        self.inner.base_tick(cx);
        let reached = self.inner.is_reached_target();
        cx.mob.set_in_sitting_pose(reached);
    }

    fn flags(&self) -> FlagSet {
        FlagSet::of2(Flag::Move, Flag::Jump)
    }
}

/// `CatLieOnBedGoal`.
#[derive(Debug, Clone)]
pub struct CatLieOnBedGoal {
    pub inner: MoveToBlockGoal,
}

impl CatLieOnBedGoal {
    /// `CatLieOnBedGoal(Cat, double, int searchRange)`: `super(cat, speed, range, 6)`,
    /// `verticalSearchStart = -2`.
    #[must_use]
    pub fn new(speed_modifier: f64, search_range: i32) -> Self {
        let mut inner = MoveToBlockGoal::new(
            MoveToBlockKind::CatLieOnBed,
            speed_modifier,
            search_range,
            6,
        );
        inner.vertical_search_start = -2;
        CatLieOnBedGoal { inner }
    }
}

impl<M: MobHost> Goal<MobAi<M>> for CatLieOnBedGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        cx.mob.is_tame()
            && !cx.mob.is_ordered_to_sit()
            && !cx.mob.is_lying()
            && self.inner.base_can_use(cx)
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        self.inner.base_can_continue_to_use(cx)
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        self.inner.base_start(cx);
        cx.mob.set_in_sitting_pose(false);
    }

    fn stop(&mut self, cx: &mut MobAi<M>) {
        cx.mob.set_lying(false);
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn tick(&mut self, cx: &mut MobAi<M>) {
        self.inner.base_tick(cx);
        cx.mob.set_in_sitting_pose(false);
        if !self.inner.is_reached_target() {
            cx.mob.set_lying(false);
        } else if !cx.mob.is_lying() {
            cx.mob.set_lying(true);
        }
    }

    fn flags(&self) -> FlagSet {
        FlagSet::of2(Flag::Jump, Flag::Move)
    }
}

/// `RemoveBlockGoal`.
#[derive(Debug, Clone)]
pub struct RemoveBlockGoal {
    pub inner: MoveToBlockGoal,
    block_to_remove: &'static str,
    ticks_since_reached_goal: i32,
}

impl RemoveBlockGoal {
    /// `RemoveBlockGoal(Block, PathfinderMob, double, int verticalSearchRange)`:
    /// `super(mob, speed, 24, v)`.
    #[must_use]
    pub fn new(block: &'static str, speed_modifier: f64, vertical_search_range: i32) -> Self {
        RemoveBlockGoal {
            inner: MoveToBlockGoal::new(
                MoveToBlockKind::RemoveBlock { block },
                speed_modifier,
                24,
                vertical_search_range,
            ),
            block_to_remove: block,
            ticks_since_reached_goal: 0,
        }
    }

    /// `getPosWithBlock(BlockPos, BlockGetter)`: the position itself, then
    /// below, west, east, north, south, two below.
    fn pos_with_block<M: MobHost>(&self, mob: &M, pos: BlockPos) -> Option<BlockPos> {
        let level = mob.level();
        let is = |p: BlockPos| level.block_facts(p).block_id == Some(self.block_to_remove);
        if is(pos) {
            return Some(pos);
        }
        [
            below(pos),
            offset(pos, -1, 0, 0),
            offset(pos, 1, 0, 0),
            offset(pos, 0, 0, -1),
            offset(pos, 0, 0, 1),
            below(below(pos)),
        ]
        .into_iter()
        .find(|p| is(*p))
    }

    /// `ticksSinceReachedGoal`, for tests.
    #[must_use]
    pub fn ticks_since_reached_goal(&self) -> i32 {
        self.ticks_since_reached_goal
    }
}

impl<M: MobHost> Goal<MobAi<M>> for RemoveBlockGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        if !cx.mob.mob_griefing() {
            return false;
        }
        if self.inner.next_start_tick > 0 {
            self.inner.next_start_tick -= 1;
            return false;
        }
        if self.inner.find_nearest_block(cx) {
            self.inner.next_start_tick = reduced_tick_delay(WAIT_AFTER_BLOCK_FOUND);
            return true;
        }
        self.inner.next_start_tick = self.inner.next_start_tick(cx);
        false
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        self.inner.base_can_continue_to_use(cx)
    }

    fn stop(&mut self, cx: &mut MobAi<M>) {
        cx.mob.set_fall_distance(1.0);
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        self.inner.base_start(cx);
        self.ticks_since_reached_goal = 0;
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn tick(&mut self, cx: &mut MobAi<M>) {
        self.inner.base_tick(cx);
        let here = cx.mob.block_position();
        let found = self.pos_with_block(&cx.mob, here);
        if self.inner.is_reached_target() {
            if let Some(p) = found {
                if self.ticks_since_reached_goal > 0 {
                    let d = cx.mob.delta_movement();
                    cx.mob.set_delta_movement(d.x, 0.3, d.z);
                    let dx = (f64::from(cx.mob.random().next_f32()) - 0.5) * 0.08;
                    let dy = (f64::from(cx.mob.random().next_f32()) - 0.5) * 0.08;
                    let dz = (f64::from(cx.mob.random().next_f32()) - 0.5) * 0.08;
                    cx.mob.send_egg_particles(
                        f64::from(p.x) + 0.5,
                        f64::from(p.y) + 0.7,
                        f64::from(p.z) + 0.5,
                        dx,
                        dy,
                        dz,
                    );
                }
                if self.ticks_since_reached_goal % 2 == 0 {
                    let d = cx.mob.delta_movement();
                    cx.mob.set_delta_movement(d.x, -0.3, d.z);
                    if self.ticks_since_reached_goal % 6 == 0 {
                        let bp = self.inner.block_pos;
                        cx.mob.play_destroy_progress_sound(bp);
                    }
                }
                if self.ticks_since_reached_goal > 60 {
                    cx.mob.remove_block(p);
                    for _ in 0..20 {
                        let dx = cx.mob.random().next_gaussian() * 0.02;
                        let dy = cx.mob.random().next_gaussian() * 0.02;
                        let dz = cx.mob.random().next_gaussian() * 0.02;
                        cx.mob.send_poof_particle(
                            f64::from(p.x) + 0.5,
                            f64::from(p.y),
                            f64::from(p.z) + 0.5,
                            dx,
                            dy,
                            dz,
                        );
                    }
                    cx.mob.play_break_sound(p);
                }
                self.ticks_since_reached_goal += 1;
            }
        }
    }

    fn flags(&self) -> FlagSet {
        FlagSet::of2(Flag::Move, Flag::Jump)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::BlockFacts;
    use crate::testing::{GridLevel, TestMob};

    fn bed() -> BlockFacts {
        let mut b = BlockFacts::NONE;
        b.is_solid = true;
        b.is_bed = true;
        b
    }

    /// `verticalSearchStart = -2` makes the `k` walk `-2, 3, -3, 4, -4, 5,
    /// -5, 6, -6` (`k > 0 ? -k : 1 - k`), so the y offsets `k - 1` are
    /// `-3, 2, -4, 3, ...`: a bed level with the cat's feet (offset 0, or
    /// -1 and +1) is never a candidate, one two blocks up is the first.
    #[test]
    fn cat_lie_on_bed_skips_its_own_level() {
        let mut g = GridLevel::new();
        g.floor(-10, -10, 10, 10, 63);
        g.set(0, 64, 1, bed());
        g.set(0, 63, 1, bed());
        g.set(2, 66, 0, bed());
        let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, 1);
        mob.tame = true;
        let mut ai = MobAi::new(mob);
        let mut goal = CatLieOnBedGoal::new(1.0, 8);
        assert!(goal.can_use(&mut ai));
        assert_eq!(goal.inner.block_pos(), BlockPos { x: 2, y: 66, z: 0 });
        assert_eq!(goal.inner.next_start_tick_value(), 40);
        for _ in 0..40 {
            assert!(!goal.can_use(&mut ai), "countdown");
        }
        assert!(goal.can_use(&mut ai));
    }

    /// With the base start (0) the first ring is `k = 0` (offset -1); at
    /// `l = 1` the walk is `m = 0` with `n = 1, -1` (an inner `m` starts
    /// `n` on the ring edge), then `m = 1` with `n = 0, 1, -1`: pinned by
    /// beds at the second and third positions of that walk.
    #[test]
    fn ring_walk_order_for_the_first_ring() {
        let mut g = GridLevel::new();
        g.floor(-10, -10, 10, 10, 63);
        g.set(0, 63, -1, bed());
        g.set(1, 63, 0, bed());
        let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, 1);
        mob.tame = true;
        let mut ai = MobAi::new(mob);
        let mut goal = CatSitOnBlockGoal::new(1.0);
        assert!(goal.can_use(&mut ai));
        assert_eq!(goal.inner.block_pos(), BlockPos { x: 0, y: 63, z: -1 });
        assert!(goal.inner.next_start_tick_value() >= 100);
    }

    /// A tame cat sits on a lit furnace, an unopened chest or a bed
    /// foot, never a bed head or an open chest, and the reached flag
    /// drives the pose.
    #[test]
    fn cat_sit_targets() {
        let mut g = GridLevel::new();
        g.floor(-10, -10, 10, 10, 63);
        let mut chest = BlockFacts::NONE;
        chest.is_solid = true;
        chest.chest_open_count = Some(1);
        g.set(1, 63, 0, chest);
        let mut head = bed();
        head.bed_part_head = true;
        g.set(-1, 63, 0, head);
        let mut furnace = BlockFacts::NONE;
        furnace.is_solid = true;
        furnace.furnace_lit = Some(true);
        g.set(0, 63, 2, furnace);
        let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, 1);
        mob.tame = true;
        let mut ai = MobAi::new(mob);
        let mut goal = CatSitOnBlockGoal::new(1.0);
        assert!(goal.can_use(&mut ai));
        assert_eq!(goal.inner.block_pos(), BlockPos { x: 0, y: 63, z: 2 });
        goal.start(&mut ai);
        assert!(goal.can_continue_to_use(&mut ai));
        goal.tick(&mut ai);
        assert!(!ai.mob.in_sitting_pose, "two blocks off: not reached");
        assert_eq!(goal.inner.try_ticks(), 1);
        ai.mob.z = 2.5;
        goal.tick(&mut ai);
        assert!(ai.mob.in_sitting_pose);
        assert_eq!(goal.inner.try_ticks(), 0);
    }

    /// The zombie's egg goal: griefing off refuses before any draw;
    /// once reached the timeline is kick, particles, and removal after
    /// the sixty-first reached tick.
    #[test]
    fn remove_block_timeline() {
        let mut g = GridLevel::new();
        g.floor(-30, -30, 30, 30, 63);
        let mut egg = BlockFacts::NONE;
        egg.block_id = Some("turtle_egg");
        g.set(0, 64, 3, egg);
        let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, 1);
        mob.mob_griefing = false;
        let mut ai = MobAi::new(mob);
        let mut goal = RemoveBlockGoal::new("turtle_egg", 1.0, 3);
        let before = format!("{:?}", ai.mob.random);
        assert!(!goal.can_use(&mut ai));
        assert_eq!(before, format!("{:?}", ai.mob.random));
        ai.mob.mob_griefing = true;
        assert!(goal.can_use(&mut ai));
        assert_eq!(goal.inner.block_pos(), BlockPos { x: 0, y: 64, z: 3 });
        assert_eq!(goal.inner.next_start_tick_value(), 10);
        goal.start(&mut ai);
        ai.mob.z = 3.5;
        ai.mob.y = 65.0;
        goal.tick(&mut ai);
        assert!(goal.inner.is_reached_target());
        assert_eq!(goal.ticks_since_reached_goal(), 1);
        assert_eq!(
            ai.mob.calls.last().map(String::as_str),
            Some("destroyProgressSound")
        );
        for _ in 0..60 {
            goal.tick(&mut ai);
        }
        assert_eq!(goal.ticks_since_reached_goal(), 61);
        assert!(!ai.mob.calls.iter().any(|c| c.starts_with("removeBlock")));
        goal.tick(&mut ai);
        assert!(ai.mob.calls.iter().any(|c| c == "removeBlock(0, 64, 3)"));
        assert_eq!(ai.mob.calls.iter().filter(|c| *c == "poof").count(), 20);
        assert_eq!(ai.mob.calls.last().map(String::as_str), Some("breakSound"));
        goal.stop(&mut ai);
        assert_eq!(
            ai.mob.calls.last().map(String::as_str),
            Some("fallDistance=1")
        );
    }
}
