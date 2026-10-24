//! `net.minecraft.world.entity.ai.navigation.PathNavigation` and
//! `GroundPathNavigation`, from the 26.2 jar.
//!
//! One struct: the base class's fields and methods with the ground
//! subclass's overrides folded in where the jar's virtual dispatch would
//! pick them (`createPathFinder`, `canUpdatePath`, `getTempMobPos`,
//! `createPath(BlockPos, int)`, `trimPath`, `canNavigateGround`, plus the
//! ground-only setters). The flying, swimming and amphibious navigations
//! are out of this lane's scope; when they land, the overrides become a
//! kind enum and nothing here moves.
//!
//! # What the level is asked
//!
//! `createPath` in the jar snapshots a `PathNavigationRegion` of
//! `(maxRange + regionOffset)` blocks around the mob and searches inside
//! it. Here the search reads the level directly through [`PathLevel`].
//! The region's one observable is the answer it gives **outside** its
//! bounds, which a search bounded by `maxRange` and `walkedDistance`
//! never asks for on a loaded world; the unloaded-chunk case (the region
//! sees an empty chunk) is deferred and named in the tests.
//!
//! # Tick shape
//!
//! `Mob.serverAiStep` calls `navigation.tick()` after the goal selectors
//! and before the move control, and `tick` ends by handing the next
//! waypoint to `MoveControl.setWantedPosition`. The move control is a
//! parameter here because it is a sibling field of the mob, not a child
//! of the navigation.

use std::cell::RefCell;

use rustmc_data::BlockPos;

use super::control::MoveControl;
use crate::host::{MobHost, PathLevel};
use crate::mth::{floor, floor_f32, java_max_f32};
use crate::pathfinder::walk_node_evaluator::get_floor_level_static;
use crate::pathfinder::{
    NodeEvaluator, Path, PathFinder, PathType, PathTypeCache, PathfindingContext, WalkNodeEvaluator,
};
use crate::phys::{above, below, block_pos_containing, block_pos_containing_vec, Vec3};

/// `MAX_TIME_RECOMPUTE`.
pub const MAX_TIME_RECOMPUTE: i64 = 20;
/// `STUCK_CHECK_INTERVAL`.
pub const STUCK_CHECK_INTERVAL: i32 = 100;
/// `STUCK_THRESHOLD_DISTANCE_FACTOR`.
pub const STUCK_THRESHOLD_DISTANCE_FACTOR: f32 = 0.25;

/// `GroundPathNavigation`, with `PathNavigation`'s state.
pub struct GroundPathNavigation {
    /// `path`.
    path: Option<Path>,
    /// Not a vanilla field: counts assignments to `path`, so a reader that
    /// vanilla compares by object identity (`MoveToTargetSink.tick`'s
    /// `this.path != mob.getNavigation().getPath()`) can ask whether the
    /// navigation holds a different object than it last saw. `moveTo(Path)`
    /// keeps the old object when `sameAs` says so, exactly as vanilla, so
    /// that case does not count.
    path_generation: u64,
    /// `speedModifier`.
    speed_modifier: f64,
    /// `tick`.
    tick: i32,
    /// `lastStuckCheck`.
    last_stuck_check: i32,
    /// `lastStuckCheckPos`, `Vec3.ZERO` at construction.
    last_stuck_check_pos: Vec3,
    /// `timeoutCachedNode`, `Vec3i.ZERO` at construction.
    timeout_cached_node: BlockPos,
    timeout_timer: i64,
    last_timeout_check: i64,
    timeout_limit: f64,
    /// `maxDistanceToWaypoint`, `0.5f` at construction.
    max_distance_to_waypoint: f32,
    has_delayed_recomputation: bool,
    time_last_recompute: i64,
    /// `pathFinder`, holding `nodeEvaluator` — a `WalkNodeEvaluator` from
    /// `GroundPathNavigation.createPathFinder`.
    path_finder: PathFinder<WalkNodeEvaluator>,
    /// `targetPos`.
    target_pos: Option<BlockPos>,
    reach_range: i32,
    /// `maxVisitedNodesMultiplier`, `1.0f`.
    max_visited_nodes_multiplier: f32,
    is_stuck: bool,
    /// `requiredPathLength`, `16.0f`.
    required_path_length: f32,
    /// `GroundPathNavigation.avoidSun`.
    avoid_sun: bool,
    /// `GroundPathNavigation.canPathToTargetsBelowSurface`.
    can_path_to_targets_below_surface: bool,
}

impl GroundPathNavigation {
    /// `GroundPathNavigation(Mob, Level)`: the base constructor, whose
    /// `createPathFinder(Mth.floor(mob.getAttributeBaseValue(FOLLOW_RANGE) * 16.0))`
    /// is the ground one — a `WalkNodeEvaluator` and a `PathFinder` with
    /// that many visited nodes.
    #[must_use]
    pub fn new<M: MobHost>(mob: &M) -> Self {
        let max_visited = floor(mob.follow_range_base() * 16.0);
        GroundPathNavigation {
            path: None,
            path_generation: 0,
            speed_modifier: 0.0,
            tick: 0,
            last_stuck_check: 0,
            last_stuck_check_pos: Vec3::ZERO,
            timeout_cached_node: BlockPos { x: 0, y: 0, z: 0 },
            timeout_timer: 0,
            last_timeout_check: 0,
            timeout_limit: 0.0,
            max_distance_to_waypoint: 0.5,
            has_delayed_recomputation: false,
            time_last_recompute: 0,
            path_finder: PathFinder::new(WalkNodeEvaluator::new(), max_visited),
            target_pos: None,
            reach_range: 0,
            max_visited_nodes_multiplier: 1.0,
            is_stuck: false,
            required_path_length: 16.0,
            avoid_sun: false,
            can_path_to_targets_below_surface: false,
        }
    }

    /// `updatePathfinderMaxVisitedNodes()`: `floor(getMaxPathLength() * 16.0f)`.
    pub fn update_pathfinder_max_visited_nodes<M: MobHost>(&mut self, mob: &M) {
        let n = floor_f32(self.get_max_path_length(mob) * 16.0);
        self.path_finder.set_max_visited_nodes(n);
    }

    /// `setRequiredPathLength(float)`.
    pub fn set_required_path_length<M: MobHost>(&mut self, mob: &M, f: f32) {
        self.required_path_length = f;
        self.update_pathfinder_max_visited_nodes(mob);
    }

    /// `getMaxPathLength()`: `Math.max((float) FOLLOW_RANGE, requiredPathLength)`.
    fn get_max_path_length<M: MobHost>(&self, mob: &M) -> f32 {
        java_max_f32(mob.follow_range() as f32, self.required_path_length)
    }

    /// `resetMaxVisitedNodesMultiplier()`.
    pub fn reset_max_visited_nodes_multiplier(&mut self) {
        self.max_visited_nodes_multiplier = 1.0;
    }

    /// `setMaxVisitedNodesMultiplier(float)`.
    pub fn set_max_visited_nodes_multiplier(&mut self, f: f32) {
        self.max_visited_nodes_multiplier = f;
    }

    /// `getTargetPos()`.
    #[must_use]
    pub fn target_pos(&self) -> Option<BlockPos> {
        self.target_pos
    }

    /// `setSpeedModifier(double)`.
    pub fn set_speed_modifier(&mut self, d: f64) {
        self.speed_modifier = d;
    }

    /// The `speedModifier` field, for tests.
    #[must_use]
    pub fn speed_modifier_for_test(&self) -> f64 {
        self.speed_modifier
    }

    /// `recomputePath()`.
    pub fn recompute_path<M: MobHost>(&mut self, mob: &mut M) {
        let now = mob.level().game_time();
        if now - self.time_last_recompute > MAX_TIME_RECOMPUTE && self.can_update_path(mob) {
            if let Some(target) = self.target_pos {
                self.path = None;
                self.path_generation += 1;
                self.path = self.create_path(mob, target, self.reach_range);
                self.path_generation += 1;
                self.time_last_recompute = mob.level().game_time();
                self.has_delayed_recomputation = false;
            }
        } else {
            self.has_delayed_recomputation = true;
        }
    }

    /// `createPath(double, double, double, int)`: `createPath(BlockPos.containing(x, y, z), accuracy)`.
    pub fn create_path_to<M: MobHost>(
        &mut self,
        mob: &mut M,
        x: f64,
        y: f64,
        z: f64,
        accuracy: i32,
    ) -> Option<Path> {
        self.create_path(mob, block_pos_containing(x, y, z), accuracy)
    }

    /// `createPath(Set<BlockPos>, int)`: `createPath(set, 8, false, accuracy)`.
    ///
    /// `targets` must be in the order the Java `Set<BlockPos>` iterates,
    /// which is the caller's set (`PathFinder` says why it matters).
    pub fn create_path_to_any<M: MobHost>(
        &mut self,
        mob: &mut M,
        targets: &[BlockPos],
        accuracy: i32,
    ) -> Option<Path> {
        self.create_path_inner(mob, targets, 8, false, accuracy, None)
    }

    /// `createPath(BlockPos, int, int)`: `createPath(set, 8, false, accuracy, (float) maxRange)`.
    pub fn create_path_within<M: MobHost>(
        &mut self,
        mob: &mut M,
        pos: BlockPos,
        accuracy: i32,
        max_range: i32,
    ) -> Option<Path> {
        #[allow(clippy::cast_precision_loss)]
        self.create_path_inner(mob, &[pos], 8, false, accuracy, Some(max_range as f32))
    }

    /// `GroundPathNavigation.createPath(BlockPos, int)`: refuse when the
    /// target's chunk is not loaded, move the target to the surface
    /// unless `canPathToTargetsBelowSurface`, then the base search.
    pub fn create_path<M: MobHost>(
        &mut self,
        mob: &mut M,
        pos: BlockPos,
        accuracy: i32,
    ) -> Option<Path> {
        if !mob.level().is_chunk_loaded(pos.x >> 4, pos.z >> 4) {
            return None;
        }
        let pos = if self.can_path_to_targets_below_surface {
            pos
        } else {
            Self::find_surface_position(mob.level(), pos)
        };
        self.create_path_inner(mob, &[pos], 8, false, accuracy, None)
    }

    /// `GroundPathNavigation.createPath(Entity, int)`:
    /// `createPath(entity.blockPosition(), accuracy)`.
    pub fn create_path_to_entity_at<M: MobHost>(
        &mut self,
        mob: &mut M,
        entity_block_pos: BlockPos,
        accuracy: i32,
    ) -> Option<Path> {
        self.create_path(mob, entity_block_pos, accuracy)
    }

    /// `findSurfacePosition(LevelChunk, BlockPos, int)`, offsets 0..206:
    /// from an air block, down to the first non-air and one above it
    /// (or, at the world floor, up to the first non-air); from a solid
    /// block, up to the first non-solid.
    fn find_surface_position<L: PathLevel>(level: &L, pos: BlockPos) -> BlockPos {
        let mut pos = pos;
        if level.block_facts(pos).is_air {
            let mut m = below(pos);
            while m.y >= level.min_y() && level.block_facts(m).is_air {
                m = below(m);
            }
            if m.y >= level.min_y() {
                return above(m);
            }
            m.y = pos.y + 1;
            while m.y <= level.max_y() && level.block_facts(m).is_air {
                m = above(m);
            }
            pos = m;
        }
        if level.block_facts(pos).is_solid {
            let mut m = above(pos);
            while m.y <= level.max_y() && level.block_facts(m).is_solid {
                m = above(m);
            }
            return m;
        }
        pos
    }

    /// `createPath(Set<BlockPos>, int regionOffset, boolean above, int accuracy, float maxRange)`,
    /// offsets 0..227.
    fn create_path_inner<M: MobHost>(
        &mut self,
        mob: &mut M,
        targets: &[BlockPos],
        region_offset: i32,
        above_mob: bool,
        accuracy: i32,
        max_range: Option<f32>,
    ) -> Option<Path> {
        if targets.is_empty() {
            return None;
        }
        if mob.y() < f64::from(mob.level().min_y()) {
            return None;
        }
        if !self.can_update_path(mob) {
            return None;
        }
        if let (Some(path), Some(target)) = (&self.path, self.target_pos) {
            if !path.is_done() && targets.contains(&target) {
                return self.path.clone();
            }
        }
        let max_range = max_range.unwrap_or_else(|| self.get_max_path_length(mob));
        let _start = if above_mob {
            above(mob.block_position())
        } else {
            mob.block_position()
        };
        #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
        let _region_radius = (max_range + region_offset as f32) as i32;
        // PathNavigationRegion(level, start - r, start + r): read-through here.
        mob.on_pathfinding_start();
        let path = {
            let level = mob.level();
            let cache: Option<&RefCell<PathTypeCache>> = level.path_type_cache();
            let ctx = PathfindingContext::new(&*mob, level, cache);
            self.path_finder.find_path(
                &ctx,
                targets,
                max_range,
                accuracy,
                self.max_visited_nodes_multiplier,
            )
        };
        mob.on_pathfinding_done();
        if let Some(p) = &path {
            self.target_pos = Some(p.target());
            self.reach_range = accuracy;
            self.reset_stuck_timeout();
        }
        path
    }

    /// `moveTo(double, double, double, double)`: `moveTo(createPath(x, y, z, 1), speed)`.
    pub fn move_to<M: MobHost>(&mut self, mob: &mut M, x: f64, y: f64, z: f64, speed: f64) -> bool {
        let p = self.create_path_to(mob, x, y, z, 1);
        self.move_along(mob, p, speed)
    }

    /// `moveTo(double, double, double, int, double)`.
    pub fn move_to_with_accuracy<M: MobHost>(
        &mut self,
        mob: &mut M,
        x: f64,
        y: f64,
        z: f64,
        accuracy: i32,
        speed: f64,
    ) -> bool {
        let p = self.create_path_to(mob, x, y, z, accuracy);
        self.move_along(mob, p, speed)
    }

    /// `moveTo(Entity, double)`: `path = createPath(entity, 1); path != null && moveTo(path, speed)`.
    pub fn move_to_entity_at<M: MobHost>(
        &mut self,
        mob: &mut M,
        entity_block_pos: BlockPos,
        speed: f64,
    ) -> bool {
        match self.create_path_to_entity_at(mob, entity_block_pos, 1) {
            None => false,
            Some(p) => self.move_along(mob, Some(p), speed),
        }
    }

    /// `moveTo(Path, double)`, offsets 0..78.
    pub fn move_along<M: MobHost>(&mut self, mob: &mut M, path: Option<Path>, speed: f64) -> bool {
        let Some(path) = path else {
            self.path = None;
            self.path_generation += 1;
            return false;
        };
        if !path.same_as(self.path.as_ref()) {
            self.path = Some(path);
            self.path_generation += 1;
        }
        if self.is_done() {
            return false;
        }
        self.trim_path(mob);
        if self.path.as_ref().is_none_or(|p| p.node_count() == 0) {
            return false;
        }
        self.speed_modifier = speed;
        let temp = self.get_temp_mob_pos(mob);
        self.last_stuck_check = self.tick;
        self.last_stuck_check_pos = temp;
        true
    }

    /// `getPath()`.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        self.path.as_ref()
    }

    /// The identity counter of `path` (see the field).
    #[must_use]
    pub fn path_generation(&self) -> u64 {
        self.path_generation
    }

    /// `tick()`, offsets 0..187.
    pub fn tick<M: MobHost>(&mut self, mob: &mut M, move_control: &mut MoveControl) {
        self.tick += 1;
        if self.has_delayed_recomputation {
            self.recompute_path(mob);
        }
        if self.is_done() {
            return;
        }
        if self.can_update_path(mob) {
            self.follow_the_path(mob);
        } else if let Some(path) = &self.path {
            if !path.is_done() {
                let temp = self.get_temp_mob_pos(mob);
                let next = path.next_entity_pos(mob.bb_width());
                if temp.y > next.y
                    && !mob.on_ground()
                    && floor(temp.x) == floor(next.x)
                    && floor(temp.z) == floor(next.z)
                {
                    if let Some(p) = &mut self.path {
                        p.advance();
                    }
                }
            }
        }
        if self.is_done() {
            return;
        }
        let next = self
            .path
            .as_ref()
            .expect("not done implies a path")
            .next_entity_pos(mob.bb_width());
        let ground_y = self.get_ground_y(mob, next);
        move_control.set_wanted_position(next.x, ground_y, next.z, self.speed_modifier);
    }

    /// `getGroundY(Vec3)`: the vector's own y over air, else the floor
    /// level under the block.
    fn get_ground_y<M: MobHost>(&self, mob: &M, v: Vec3) -> f64 {
        let pos = block_pos_containing_vec(v);
        if mob.level().block_facts(below(pos)).is_air {
            v.y
        } else {
            get_floor_level_static(mob.level(), pos)
        }
    }

    /// `followThePath()`, offsets 0..198.
    fn follow_the_path<M: MobHost>(&mut self, mob: &mut M) {
        let temp = self.get_temp_mob_pos(mob);
        let w = mob.bb_width();
        self.max_distance_to_waypoint = if w > 0.75 { w / 2.0 } else { 0.75 - w / 2.0 };
        let (next_pos, next_type) = {
            let p = self.path.as_ref().expect("followThePath needs a path");
            (p.next_node_pos(), p.next_node().path_type)
        };
        let dx = (mob.x() - (f64::from(next_pos.x) + 0.5)).abs();
        let dy = (mob.y() - f64::from(next_pos.y)).abs();
        let dz = (mob.z() - (f64::from(next_pos.z) + 0.5)).abs();
        let close = dx < f64::from(self.max_distance_to_waypoint)
            && dz < f64::from(self.max_distance_to_waypoint)
            && dy < f64::from(self.get_max_vertical_distance_to_waypoint());
        if close
            || (Self::can_cut_corner(next_type)
                && self.should_target_next_node_in_direction(mob, temp))
        {
            if let Some(p) = &mut self.path {
                p.advance();
            }
        }
        self.do_stuck_detection(mob, temp);
    }

    /// `shouldTargetNextNodeInDirection(Vec3)`, offsets 0..188.
    fn should_target_next_node_in_direction<M: MobHost>(&self, mob: &M, temp: Vec3) -> bool {
        let p = self.path.as_ref().expect("path");
        if p.next_node_index() + 1 >= p.node_count() {
            return false;
        }
        let center = Vec3::at_bottom_center_of(p.next_node_pos());
        if !temp.closer_than(center, 2.0) {
            return false;
        }
        if self.can_move_directly(temp, p.next_entity_pos(mob.bb_width())) {
            return true;
        }
        let next_center = Vec3::at_bottom_center_of(p.node_pos(p.next_node_index() + 1));
        let a = center.subtract(temp);
        let b = next_center.subtract(temp);
        let la = a.length_sqr();
        let lb = b.length_sqr();
        let b_closer = lb < la;
        let a_close = la < 0.5;
        if !b_closer && !a_close {
            return false;
        }
        let an = a.normalize();
        let bn = b.normalize();
        bn.dot(an) < 0.0
    }

    /// `doStuckDetection(Vec3)`, offsets 0..258.
    fn do_stuck_detection<M: MobHost>(&mut self, mob: &mut M, temp: Vec3) {
        if self.tick - self.last_stuck_check > STUCK_CHECK_INTERVAL {
            let spd = if mob.speed() >= 1.0 {
                mob.speed()
            } else {
                mob.speed() * mob.speed()
            };
            let threshold = spd * 100.0 * STUCK_THRESHOLD_DISTANCE_FACTOR;
            if temp.distance_to_sqr(self.last_stuck_check_pos) < f64::from(threshold * threshold) {
                self.is_stuck = true;
                self.stop();
            } else {
                self.is_stuck = false;
            }
            self.last_stuck_check = self.tick;
            self.last_stuck_check_pos = temp;
        }
        if let Some(path) = &self.path {
            if !path.is_done() {
                let next_pos = path.next_node_pos();
                let now = mob.level().game_time();
                if next_pos == self.timeout_cached_node {
                    self.timeout_timer += now - self.last_timeout_check;
                } else {
                    self.timeout_cached_node = next_pos;
                    let d = temp.distance_to(Vec3::at_bottom_center_of(self.timeout_cached_node));
                    self.timeout_limit = if mob.speed() > 0.0 {
                        d / f64::from(mob.speed()) * 20.0
                    } else {
                        0.0
                    };
                }
                #[allow(clippy::cast_precision_loss)]
                if self.timeout_limit > 0.0 && self.timeout_timer as f64 > self.timeout_limit * 3.0
                {
                    self.timeout_path();
                }
                self.last_timeout_check = now;
            }
        }
    }

    /// `timeoutPath()`.
    fn timeout_path(&mut self) {
        self.reset_stuck_timeout();
        self.stop();
    }

    /// `resetStuckTimeout()`.
    fn reset_stuck_timeout(&mut self) {
        self.timeout_cached_node = BlockPos { x: 0, y: 0, z: 0 };
        self.timeout_timer = 0;
        self.timeout_limit = 0.0;
        self.is_stuck = false;
    }

    /// `isDone()`: `path == null || path.isDone()`.
    #[must_use]
    pub fn is_done(&self) -> bool {
        self.path.as_ref().is_none_or(Path::is_done)
    }

    /// `isInProgress()`.
    #[must_use]
    pub fn is_in_progress(&self) -> bool {
        !self.is_done()
    }

    /// `stop()`: `path = null`.
    pub fn stop(&mut self) {
        self.path = None;
        self.path_generation += 1;
    }

    /// `GroundPathNavigation.getTempMobPos()`: `(mob.x, getSurfaceY(), mob.z)`.
    fn get_temp_mob_pos<M: MobHost>(&self, mob: &M) -> Vec3 {
        Vec3::new(mob.x(), f64::from(self.get_surface_y(mob)), mob.z())
    }

    /// `GroundPathNavigation.canUpdatePath()`: `onGround || isInLiquid || isPassenger`.
    fn can_update_path<M: MobHost>(&self, mob: &M) -> bool {
        mob.on_ground() || mob.is_in_liquid() || mob.is_passenger()
    }

    /// `GroundPathNavigation.getSurfaceY()`, offsets 0..127: in water and
    /// floating, walk up through water blocks (at most 16, else the mob's
    /// block y); otherwise `floor(y + 0.5)`.
    fn get_surface_y<M: MobHost>(&self, mob: &M) -> i32 {
        if mob.is_in_water() && self.can_float() {
            let mut y = mob.block_y();
            let mut facts =
                mob.level()
                    .block_facts(block_pos_containing(mob.x(), f64::from(y), mob.z()));
            let mut n = 0;
            while facts.is_water_block {
                y += 1;
                facts =
                    mob.level()
                        .block_facts(block_pos_containing(mob.x(), f64::from(y), mob.z()));
                n += 1;
                if n > 16 {
                    return mob.block_y();
                }
            }
            y
        } else {
            floor(mob.y() + 0.5)
        }
    }

    /// `PathNavigation.trimPath()` then `GroundPathNavigation.trimPath()`.
    fn trim_path<M: MobHost>(&mut self, mob: &M) {
        let Some(path) = &mut self.path else {
            return;
        };
        let level = mob.level();
        for i in 0..path.node_count() {
            let node = path.node(i).clone();
            let next = if i + 1 < path.node_count() {
                Some(path.node(i + 1).clone())
            } else {
                None
            };
            if level.block_facts(node.as_block_pos()).is_cauldron {
                path.replace_node(i, node.clone_and_move(node.x, node.y + 1, node.z));
                if let Some(next) = next {
                    if node.y >= next.y {
                        path.replace_node(i + 1, node.clone_and_move(next.x, node.y + 1, next.z));
                    }
                }
            }
        }
        // GroundPathNavigation.trimPath: cut the path at the first node
        // under open sky when avoiding the sun, unless already in it.
        if self.avoid_sun {
            if level.can_see_sky(block_pos_containing(mob.x(), mob.y() + 0.5, mob.z())) {
                return;
            }
            for i in 0..path.node_count() {
                if level.can_see_sky(path.node(i).as_block_pos()) {
                    path.truncate_nodes(i);
                    return;
                }
            }
        }
    }

    /// `canMoveDirectly(Vec3, Vec3)`: `iconst_0` in the base and not
    /// overridden by the ground navigation.
    fn can_move_directly(&self, _from: Vec3, _to: Vec3) -> bool {
        false
    }

    /// `canCutCorner(PathType)`.
    #[must_use]
    pub fn can_cut_corner(t: PathType) -> bool {
        !matches!(
            t,
            PathType::FireInNeighbor | PathType::DamagingInNeighbor | PathType::WalkableDoor
        )
    }

    /// `isClearForMovementBetween(Mob, Vec3, Vec3, boolean)`: a clip from
    /// `from` to `to` raised by half the mob's height, colliders only.
    #[must_use]
    pub fn is_clear_for_movement_between<M: MobHost>(
        mob: &M,
        from: Vec3,
        to: Vec3,
        fluids: bool,
    ) -> bool {
        let to = Vec3::new(to.x, to.y + f64::from(mob.bb_height()) * 0.5, to.z);
        mob.level().clip_misses(mob.id(), from, to, fluids)
    }

    /// `isStableDestination(BlockPos)`: `getBlockState(pos.below()).isSolidRender()`.
    #[must_use]
    pub fn is_stable_destination<M: MobHost>(&self, mob: &M, pos: BlockPos) -> bool {
        mob.level().block_facts(below(pos)).is_solid_render
    }

    /// `getNodeEvaluator()`.
    #[must_use]
    pub fn node_evaluator(&self) -> &WalkNodeEvaluator {
        &self.path_finder.node_evaluator
    }

    /// `getNodeEvaluator()`, mutably.
    pub fn node_evaluator_mut(&mut self) -> &mut WalkNodeEvaluator {
        &mut self.path_finder.node_evaluator
    }

    /// `setCanFloat(boolean)`.
    pub fn set_can_float(&mut self, v: bool) {
        self.path_finder.node_evaluator.base.can_float = v;
    }

    /// `canFloat()`.
    #[must_use]
    pub fn can_float(&self) -> bool {
        self.path_finder.node_evaluator.base.can_float
    }

    /// `setCanOpenDoors(boolean)`.
    pub fn set_can_open_doors(&mut self, v: bool) {
        self.path_finder.node_evaluator.base.can_open_doors = v;
    }

    /// `GroundPathNavigation.setCanWalkOverFences(boolean)`.
    pub fn set_can_walk_over_fences(&mut self, v: bool) {
        self.path_finder.node_evaluator.base.can_walk_over_fences = v;
    }

    /// `GroundPathNavigation.setAvoidSun(boolean)`.
    pub fn set_avoid_sun(&mut self, v: bool) {
        self.avoid_sun = v;
    }

    /// The `avoidSun` field, for tests.
    #[must_use]
    pub fn avoids_sun(&self) -> bool {
        self.avoid_sun
    }

    /// `GroundPathNavigation.setCanPathToTargetsBelowSurface(boolean)`.
    pub fn set_can_path_to_targets_below_surface(&mut self, v: bool) {
        self.can_path_to_targets_below_surface = v;
    }

    /// The same flag, for tests that assert a behaviour set it.
    #[must_use]
    pub fn can_path_to_targets_below_surface_for_test(&self) -> bool {
        self.can_path_to_targets_below_surface
    }

    /// `shouldRecomputePath(BlockPos)`, offsets 0..126.
    #[must_use]
    pub fn should_recompute_path<M: MobHost>(&self, mob: &M, pos: BlockPos) -> bool {
        if self.has_delayed_recomputation {
            return false;
        }
        let Some(path) = &self.path else {
            return false;
        };
        if path.is_done() || path.node_count() == 0 {
            return false;
        }
        let end = path.end_node().expect("non-empty");
        let mid = Vec3::new(
            (f64::from(end.x) + mob.x()) / 2.0,
            (f64::from(end.y) + mob.y()) / 2.0,
            (f64::from(end.z) + mob.z()) / 2.0,
        );
        #[allow(clippy::cast_precision_loss)]
        crate::phys::closer_to_center_than(
            pos,
            mid,
            (path.node_count() - path.next_node_index()) as f64,
        )
    }

    /// `getMaxDistanceToWaypoint()`.
    #[must_use]
    pub fn max_distance_to_waypoint(&self) -> f32 {
        self.max_distance_to_waypoint
    }

    /// `getMaxVerticalDistanceToWaypoint()`: `fconst_1`.
    #[must_use]
    pub fn get_max_vertical_distance_to_waypoint(&self) -> f32 {
        1.0
    }

    /// `isStuck()`.
    #[must_use]
    pub fn is_stuck(&self) -> bool {
        self.is_stuck
    }

    /// `GroundPathNavigation.canNavigateGround()`: `iconst_1`.
    #[must_use]
    pub fn can_navigate_ground(&self) -> bool {
        true
    }

    /// `NodeEvaluator.getPathType(Mob, BlockPos)` through this navigation's
    /// evaluator — what `MoveControl.isWalkable` asks.
    #[must_use]
    pub fn path_type_at<M: MobHost>(&self, mob: &M, pos: BlockPos) -> PathType {
        let level = mob.level();
        let ctx = PathfindingContext::new(mob, level, level.path_type_cache());
        NodeEvaluator::<M, M::Level>::get_path_type(
            &self.path_finder.node_evaluator,
            &ctx,
            pos.x,
            pos.y,
            pos.z,
        )
    }

    /// `tick` counter, for tests.
    #[must_use]
    pub fn tick_count(&self) -> i32 {
        self.tick
    }
}
