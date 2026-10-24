//! `net.minecraft.world.level.pathfinder.WalkNodeEvaluator`, from the 26.2
//! jar — the ground evaluator every `GroundPathNavigation` mob uses.
//!
//! Every method is transcribed against its bytecode offsets; the module
//! docs of each say what a rewrite would get wrong. The two per-search
//! caches are the jar's: `pathTypesByPosCacheByMob` (a `Long2ObjectMap`
//! keyed by `BlockPos.asLong`) and `collisionCache` (an
//! `Object2BooleanMap<AABB>`, keyed by the box's six doubles). Both are
//! cleared in `done()`.
//!
//! # Where the level is asked
//!
//! * `getPathTypeFromState` and everything above it: [`BlockFacts`].
//! * `getFloorLevel`: the collision shape's top face, `collision_shape_max_y`.
//! * `hasCollisions`: `!level.noCollision(mob, aabb)`.
//! * `getStart`'s water column walk: `fluid_is_water_source` and
//!   `is_water_block`, which the jar tests separately.
//!
//! [`BlockFacts`]: crate::host::BlockFacts

use std::collections::HashMap;

use rustmc_data::BlockPos;

use super::context::{get_path_type_static, PathfindingContext};
use super::node::{NodeId, Target};
use super::node_evaluator::{NodeEvaluator, NodeEvaluatorBase};
use super::path_type::{PathType, PathTypeSet};
use crate::host::{MobHost, PathLevel};
use crate::mth::{ceil, floor, floor_f32, java_max_f32};
use crate::phys::{below, block_pos_as_long, block_pos_containing, Aabb, Direction, Vec3};

/// `SPACE_BETWEEN_WALL_POSTS`.
pub const SPACE_BETWEEN_WALL_POSTS: f64 = 0.5;
/// `DEFAULT_MOB_JUMP_HEIGHT`.
pub const DEFAULT_MOB_JUMP_HEIGHT: f64 = 1.125;

/// The `AABB` as an `Object2BooleanMap` key: `AABB.equals` compares the six
/// doubles with `Double.compare`, so the bit patterns are the key.
type AabbKey = [u64; 6];

fn aabb_key(b: &Aabb) -> AabbKey {
    [
        b.min_x.to_bits(),
        b.min_y.to_bits(),
        b.min_z.to_bits(),
        b.max_x.to_bits(),
        b.max_y.to_bits(),
        b.max_z.to_bits(),
    ]
}

/// `WalkNodeEvaluator`.
#[derive(Debug)]
pub struct WalkNodeEvaluator {
    pub base: NodeEvaluatorBase,
    path_types_by_pos_cache_by_mob: HashMap<i64, PathType>,
    collision_cache: HashMap<AabbKey, bool>,
    /// `reusableNeighbors`, indexed by `Direction.get2DDataValue()`.
    reusable_neighbors: [Option<NodeId>; 4],
}

impl Default for WalkNodeEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

impl WalkNodeEvaluator {
    /// `WalkNodeEvaluator()`.
    #[must_use]
    pub fn new() -> Self {
        WalkNodeEvaluator {
            base: NodeEvaluatorBase::new(),
            path_types_by_pos_cache_by_mob: HashMap::new(),
            collision_cache: HashMap::new(),
            reusable_neighbors: [None; 4],
        }
    }

    /// `getStartNode(BlockPos)`: intern the node, classify it, take the
    /// mob's malus.
    fn get_start_node<M: MobHost, L: PathLevel>(
        &mut self,
        ctx: &PathfindingContext<'_, M, L>,
        pos: BlockPos,
    ) -> NodeId {
        let id = self.base.node_at(pos);
        let (x, y, z) = {
            let n = self.base.nodes.get(id);
            (n.x, n.y, n.z)
        };
        let t = self.get_cached_path_type(ctx, x, y, z);
        let n = self.base.nodes.get_mut(id);
        n.path_type = t;
        n.cost_malus = ctx.mob.pathfinding_malus(t);
        id
    }

    /// `canStartAt(BlockPos)`: `type != OPEN && malus(type) >= 0`.
    fn can_start_at<M: MobHost, L: PathLevel>(
        &mut self,
        ctx: &PathfindingContext<'_, M, L>,
        pos: BlockPos,
    ) -> bool {
        let t = self.get_cached_path_type(ctx, pos.x, pos.y, pos.z);
        t != PathType::Open && ctx.mob.pathfinding_malus(t) >= 0.0
    }

    /// `isNeighborValid(Node neighbor, Node node)`:
    /// `neighbor != null && !neighbor.closed && (neighbor.costMalus >= 0 || node.costMalus < 0)`.
    fn is_neighbor_valid(&self, neighbor: Option<NodeId>, node: NodeId) -> bool {
        match neighbor {
            None => false,
            Some(nb) => {
                let nb = self.base.nodes.get(nb);
                !nb.closed && (nb.cost_malus >= 0.0 || self.base.nodes.get(node).cost_malus < 0.0)
            }
        }
    }

    /// `isDiagonalValid(Node node, Node a, Node b)`, offsets 0..183.
    fn is_diagonal_valid3<M: MobHost>(
        &self,
        mob: &M,
        node: NodeId,
        a: Option<NodeId>,
        b: Option<NodeId>,
    ) -> bool {
        let (Some(a), Some(b)) = (a, b) else {
            return false;
        };
        let node = self.base.nodes.get(node);
        let a = self.base.nodes.get(a);
        let b = self.base.nodes.get(b);
        if b.y > node.y || a.y > node.y {
            return false;
        }
        if a.path_type == PathType::WalkableDoor || b.path_type == PathType::WalkableDoor {
            return false;
        }
        if mob.bb_width() > 1.0 && (a.cost_malus > 0.0 || b.cost_malus > 0.0) {
            return false;
        }
        let both_fences = b.path_type == PathType::Fence
            && a.path_type == PathType::Fence
            && f64::from(mob.bb_width()) < 0.5;
        (b.y < node.y || b.cost_malus >= 0.0 || both_fences)
            && (a.y < node.y || a.cost_malus >= 0.0 || both_fences)
    }

    /// `isDiagonalValid(Node)`:
    /// `n != null && !n.closed && n.type != WALKABLE_DOOR && n.costMalus >= 0`.
    fn is_diagonal_valid1(&self, n: Option<NodeId>) -> bool {
        match n {
            None => false,
            Some(n) => {
                let n = self.base.nodes.get(n);
                !n.closed && n.path_type != PathType::WalkableDoor && n.cost_malus >= 0.0
            }
        }
    }

    /// `doesBlockHavePartialCollision(PathType)`.
    fn does_block_have_partial_collision(t: PathType) -> bool {
        matches!(
            t,
            PathType::Fence | PathType::DoorWoodClosed | PathType::DoorIronClosed
        )
    }

    /// `canReachWithoutCollision(Node)`: step the mob's box toward the
    /// node's centre in `ceil(length / aabb.getSize())` steps of
    /// `v.scale(1.0f / steps)` — a **float** division widened
    /// (`fconst_1; i2f; fdiv; f2d` at 96..103) — and refuse on any
    /// collision.
    fn can_reach_without_collision<M: MobHost, L: PathLevel>(
        &mut self,
        ctx: &PathfindingContext<'_, M, L>,
        node: NodeId,
    ) -> bool {
        let mob = ctx.mob;
        let mut aabb = mob.bounding_box();
        let n = self.base.nodes.get(node);
        let v = Vec3::new(
            f64::from(n.x) - mob.x() + aabb.x_size() / 2.0,
            f64::from(n.y) - mob.y() + aabb.y_size() / 2.0,
            f64::from(n.z) - mob.z() + aabb.z_size() / 2.0,
        );
        let steps = ceil(v.length() / aabb.size());
        #[allow(clippy::cast_precision_loss)]
        let v = v.scale(f64::from(1.0f32 / steps as f32));
        for _ in 1..=steps {
            aabb = aabb.moved_by(v);
            if self.has_collisions(ctx, &aabb) {
                return false;
            }
        }
        true
    }

    /// `getFloorLevel(BlockPos)` (instance): water counts as a floor half a
    /// block up when the mob floats or is amphibious.
    fn get_floor_level<M: MobHost, L: PathLevel>(
        &self,
        ctx: &PathfindingContext<'_, M, L>,
        pos: BlockPos,
    ) -> f64 {
        if (self.base.can_float || self.is_amphibious())
            && ctx.level.block_facts(pos).fluid_is_water
        {
            return f64::from(pos.y) + 0.5;
        }
        get_floor_level_static(ctx.level, pos)
    }

    /// `isAmphibious()`: `iconst_0`.
    fn is_amphibious(&self) -> bool {
        false
    }

    /// `findAcceptedNode(int x, int y, int z, int maxUp, double floorLevel, Direction dir, PathType atType)`,
    /// offsets 0..305.
    #[allow(clippy::too_many_arguments)]
    fn find_accepted_node<M: MobHost, L: PathLevel>(
        &mut self,
        ctx: &PathfindingContext<'_, M, L>,
        x: i32,
        y: i32,
        z: i32,
        max_up: i32,
        floor_level: f64,
        dir: Direction,
        at_type: PathType,
    ) -> Option<NodeId> {
        let mut node: Option<NodeId> = None;
        let floor_here = self.get_floor_level(ctx, BlockPos { x, y, z });
        if floor_here - floor_level > self.get_mob_jump_height(ctx.mob) {
            return None;
        }
        let t = self.get_cached_path_type(ctx, x, y, z);
        let malus = ctx.mob.pathfinding_malus(t);
        if malus >= 0.0 {
            node = Some(self.get_node_and_update_cost_to_max(x, y, z, t, malus));
        }
        if Self::does_block_have_partial_collision(at_type) {
            if let Some(n) = node {
                if self.base.nodes.get(n).cost_malus >= 0.0
                    && !self.can_reach_without_collision(ctx, n)
                {
                    node = None;
                }
            }
        }
        if t == PathType::Walkable || (self.is_amphibious() && t == PathType::Water) {
            return node;
        }
        let node_missing_or_negative = match node {
            None => true,
            Some(n) => self.base.nodes.get(n).cost_malus < 0.0,
        };
        if node_missing_or_negative
            && max_up > 0
            && (t != PathType::Fence || self.base.can_walk_over_fences)
            && t != PathType::UnpassableRail
            && t != PathType::Trapdoor
            && t != PathType::PowderSnow
        {
            node = self.try_jump_on(ctx, x, y, z, max_up, floor_level, dir, at_type);
        } else if !self.is_amphibious() && t == PathType::Water && !self.base.can_float {
            node = self.try_find_first_non_water_below(ctx, x, y, z, node);
        } else if t == PathType::Open {
            node = Some(self.try_find_first_ground_node_below(ctx, x, y, z));
        } else if Self::does_block_have_partial_collision(t) && node.is_none() {
            node = Some(self.get_closed_node(x, y, z, t));
        }
        node
    }

    /// `getMobJumpHeight()`: `Math.max(1.125, mob.maxUpStep())`.
    fn get_mob_jump_height<M: MobHost>(&self, mob: &M) -> f64 {
        rustmc_worldgen::math::java_max(DEFAULT_MOB_JUMP_HEIGHT, f64::from(mob.max_up_step()))
    }

    /// `getNodeAndUpdateCostToMax(x, y, z, type, malus)`.
    fn get_node_and_update_cost_to_max(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        t: PathType,
        malus: f32,
    ) -> NodeId {
        let id = self.base.node(x, y, z);
        let n = self.base.nodes.get_mut(id);
        n.path_type = t;
        n.cost_malus = java_max_f32(n.cost_malus, malus);
        id
    }

    /// `getBlockedNode(x, y, z)`: `type = BLOCKED, costMalus = -1`.
    fn get_blocked_node(&mut self, x: i32, y: i32, z: i32) -> NodeId {
        let id = self.base.node(x, y, z);
        let n = self.base.nodes.get_mut(id);
        n.path_type = PathType::Blocked;
        n.cost_malus = -1.0;
        id
    }

    /// `getClosedNode(x, y, z, type)`: `closed = true, type, costMalus = type.getMalus()`.
    fn get_closed_node(&mut self, x: i32, y: i32, z: i32, t: PathType) -> NodeId {
        let id = self.base.node(x, y, z);
        let n = self.base.nodes.get_mut(id);
        n.closed = true;
        n.path_type = t;
        n.cost_malus = t.malus();
        id
    }

    /// `tryJumpOn(...)`, offsets 0..215: the node one up with `maxUp - 1`,
    /// returned as is for a mob a block wide or more, or when it is not
    /// `OPEN`/`WALKABLE`; otherwise a box the mob's width, from the floor
    /// one up behind the step to the mob's height above the node's floor,
    /// is collision-tested.
    #[allow(clippy::too_many_arguments)]
    fn try_jump_on<M: MobHost, L: PathLevel>(
        &mut self,
        ctx: &PathfindingContext<'_, M, L>,
        x: i32,
        y: i32,
        z: i32,
        max_up: i32,
        floor_level: f64,
        dir: Direction,
        at_type: PathType,
    ) -> Option<NodeId> {
        let node =
            self.find_accepted_node(ctx, x, y + 1, z, max_up - 1, floor_level, dir, at_type)?;
        let mob = ctx.mob;
        if mob.bb_width() >= 1.0 {
            return Some(node);
        }
        let (nt, nx, ny, nz) = {
            let n = self.base.nodes.get(node);
            (n.path_type, n.x, n.y, n.z)
        };
        if nt != PathType::Open && nt != PathType::Walkable {
            return Some(node);
        }
        let cx = f64::from(x - dir.step_x()) + 0.5;
        let cz = f64::from(z - dir.step_z()) + 0.5;
        let hw = f64::from(mob.bb_width()) / 2.0;
        let min_y =
            self.get_floor_level(ctx, block_pos_containing(cx, f64::from(y + 1), cz)) + 0.001;
        let max_y = f64::from(mob.bb_height())
            + self.get_floor_level(
                ctx,
                block_pos_containing(f64::from(nx), f64::from(ny), f64::from(nz)),
            )
            - 0.002;
        let aabb = Aabb::new(cx - hw, min_y, cz - hw, cx + hw, max_y, cz + hw);
        if self.has_collisions(ctx, &aabb) {
            None
        } else {
            Some(node)
        }
    }

    /// `tryFindFirstNonWaterBelow(x, y, z, node)`: walk down through
    /// `WATER`, updating the node, until something else or the world
    /// floor.
    fn try_find_first_non_water_below<M: MobHost, L: PathLevel>(
        &mut self,
        ctx: &PathfindingContext<'_, M, L>,
        x: i32,
        y: i32,
        z: i32,
        mut node: Option<NodeId>,
    ) -> Option<NodeId> {
        let min_y = ctx.mob.level().min_y();
        let mut y = y - 1;
        while y > min_y {
            let t = self.get_cached_path_type(ctx, x, y, z);
            if t == PathType::Water {
                let malus = ctx.mob.pathfinding_malus(t);
                node = Some(self.get_node_and_update_cost_to_max(x, y, z, t, malus));
                y -= 1;
            } else {
                return node;
            }
        }
        node
    }

    /// `tryFindFirstGroundNodeBelow(x, y, z)`: walk down through `OPEN`
    /// until a non-open block or the fall limit; a blocked node marks a
    /// fall too far or an impassable landing.
    fn try_find_first_ground_node_below<M: MobHost, L: PathLevel>(
        &mut self,
        ctx: &PathfindingContext<'_, M, L>,
        x: i32,
        y: i32,
        z: i32,
    ) -> NodeId {
        let min_y = ctx.mob.level().min_y();
        let mut yy = y - 1;
        while yy >= min_y {
            if y - yy > ctx.mob.max_fall_distance() {
                return self.get_blocked_node(x, yy, z);
            }
            let t = self.get_cached_path_type(ctx, x, yy, z);
            let malus = ctx.mob.pathfinding_malus(t);
            if t != PathType::Open {
                if malus >= 0.0 {
                    return self.get_node_and_update_cost_to_max(x, yy, z, t, malus);
                }
                return self.get_blocked_node(x, yy, z);
            }
            yy -= 1;
        }
        self.get_blocked_node(x, y, z)
    }

    /// `hasCollisions(AABB)`: `collisionCache.computeIfAbsent(aabb, !level.noCollision(mob, aabb))`.
    fn has_collisions<M: MobHost, L: PathLevel>(
        &mut self,
        ctx: &PathfindingContext<'_, M, L>,
        aabb: &Aabb,
    ) -> bool {
        let key = aabb_key(aabb);
        if let Some(v) = self.collision_cache.get(&key) {
            return *v;
        }
        let v = !ctx.level.no_collision(ctx.mob.id(), aabb);
        self.collision_cache.insert(key, v);
        v
    }

    /// `getCachedPathType(int, int, int)`:
    /// `pathTypesByPosCacheByMob.computeIfAbsent(asLong(x, y, z), getPathTypeOfMob(ctx, x, y, z, mob))`.
    pub fn get_cached_path_type<M: MobHost, L: PathLevel>(
        &mut self,
        ctx: &PathfindingContext<'_, M, L>,
        x: i32,
        y: i32,
        z: i32,
    ) -> PathType {
        let key = block_pos_as_long(x, y, z);
        if let Some(t) = self.path_types_by_pos_cache_by_mob.get(&key) {
            return *t;
        }
        let t = self.get_path_type_of_mob(ctx, x, y, z);
        self.path_types_by_pos_cache_by_mob.insert(key, t);
        t
    }

    /// `getPathTypeOfMob(ctx, x, y, z, mob)`, offsets 0..273: the worst
    /// type in the mob's volume, with the big-mob danger rule and the
    /// open-centre rule.
    pub fn get_path_type_of_mob<M: MobHost, L: PathLevel>(
        &self,
        ctx: &PathfindingContext<'_, M, L>,
        x: i32,
        y: i32,
        z: i32,
    ) -> PathType {
        let mob = ctx.mob;
        let set = self.get_path_type_within_mob_bb(ctx, x, y, z);
        if set.len() == 1 {
            return set.iter().next().expect("size 1");
        }
        if set.contains(PathType::Fence) {
            return PathType::Fence;
        }
        if set.contains(PathType::UnpassableRail) {
            return PathType::UnpassableRail;
        }
        let mut best = PathType::Blocked;
        let mut best_malus = mob.pathfinding_malus(best);
        for t in set.iter() {
            let m = mob.pathfinding_malus(t);
            if m < 0.0 {
                return t;
            }
            if m >= best_malus {
                best_malus = m;
                best = t;
            }
        }
        let center = self.get_path_type(ctx, x, y, z);
        let wide = self.base.entity_width > 1;
        if wide {
            let center_cheaper = mob.pathfinding_malus(center) < best_malus;
            if center_cheaper && mob.pathfinding_malus(PathType::BigMobsCloseToDanger) < best_malus
            {
                return PathType::BigMobsCloseToDanger;
            }
            return best;
        }
        if center == PathType::Open && best != PathType::Open && best_malus == 0.0 {
            return PathType::Open;
        }
        best
    }

    /// `getPathTypeWithinMobBB(ctx, x, y, z)`: every block of the mob's
    /// `entityWidth x entityHeight x entityDepth` volume, `dx` outer, `dy`
    /// middle, `dz` inner, with the door and rail rewrites, into an
    /// `EnumSet`.
    pub fn get_path_type_within_mob_bb<M: MobHost, L: PathLevel>(
        &self,
        ctx: &PathfindingContext<'_, M, L>,
        x: i32,
        y: i32,
        z: i32,
    ) -> PathTypeSet {
        let mut set = PathTypeSet::NONE;
        for dx in 0..self.base.entity_width {
            for dy in 0..self.base.entity_height {
                for dz in 0..self.base.entity_depth {
                    let xx = dx + x;
                    let yy = dy + y;
                    let zz = dz + z;
                    let mut t = self.get_path_type(ctx, xx, yy, zz);
                    let mob_pos = ctx.mob.block_position();
                    let can_pass = self.base.can_pass_doors;
                    if t == PathType::DoorWoodClosed && self.base.can_open_doors && can_pass {
                        t = PathType::WalkableDoor;
                    }
                    if t == PathType::DoorOpen && !can_pass {
                        t = PathType::Blocked;
                    }
                    if t == PathType::Rail
                        && self.get_path_type(ctx, mob_pos.x, mob_pos.y, mob_pos.z)
                            != PathType::Rail
                        && self.get_path_type(ctx, mob_pos.x, mob_pos.y - 1, mob_pos.z)
                            != PathType::Rail
                    {
                        t = PathType::UnpassableRail;
                    }
                    set.add(t);
                }
            }
        }
        set
    }
}

/// `WalkNodeEvaluator.getFloorLevel(BlockGetter, BlockPos)` (static): the
/// block below's collision top, or `below.y` when its shape is empty.
#[must_use]
pub fn get_floor_level_static<L: PathLevel>(level: &L, pos: BlockPos) -> f64 {
    let b = below(pos);
    f64::from(b.y) + level.collision_shape_max_y(b).unwrap_or(0.0)
}

impl<M: MobHost, L: PathLevel> NodeEvaluator<M, L> for WalkNodeEvaluator {
    fn base(&self) -> &NodeEvaluatorBase {
        &self.base
    }

    fn base_mut(&mut self) -> &mut NodeEvaluatorBase {
        &mut self.base
    }

    /// `prepare`: the base's, then `mob.onPathfindingStart()` — which the
    /// caller does, since the context holds the mob immutably.
    fn prepare(&mut self, ctx: &PathfindingContext<'_, M, L>) {
        self.base.prepare(ctx.mob);
    }

    /// `done()`: `mob.onPathfindingDone()` (the caller's), clear both
    /// caches, then the base's (which nulls the context and the mob).
    fn done(&mut self) {
        self.path_types_by_pos_cache_by_mob.clear();
        self.collision_cache.clear();
    }

    /// `getStart()`, offsets 0..479.
    fn get_start(&mut self, ctx: &PathfindingContext<'_, M, L>) -> Option<NodeId> {
        let mob = ctx.mob;
        let level = ctx.level;
        let mut y = mob.block_y();
        let mut pos = block_pos_containing(mob.x(), f64::from(y), mob.z());
        let mut facts = ctx.block_facts(pos);
        if mob.can_stand_on_fluid(&facts) {
            while mob.can_stand_on_fluid(&facts) {
                y += 1;
                pos = block_pos_containing(mob.x(), f64::from(y), mob.z());
                facts = ctx.block_facts(pos);
            }
            y -= 1;
        } else if self.base.can_float && mob.is_in_water() {
            while facts.is_water_block || facts.fluid_is_water_source {
                y += 1;
                pos = block_pos_containing(mob.x(), f64::from(y), mob.z());
                facts = ctx.block_facts(pos);
            }
            y -= 1;
        } else if mob.on_ground() {
            y = floor(mob.y() + 0.5);
        } else {
            pos = block_pos_containing(mob.x(), mob.y() + 1.0, mob.z());
            while pos.y > level.min_y() {
                y = pos.y;
                pos.y -= 1;
                let f = ctx.block_facts(pos);
                if !(f.is_air || f.is_pathfindable_land) {
                    break;
                }
            }
        }
        let block_pos = mob.block_position();
        pos = BlockPos {
            x: block_pos.x,
            y,
            z: block_pos.z,
        };
        if !self.can_start_at(ctx, pos) {
            let aabb = mob.bounding_box();
            let corners = [
                (aabb.min_x, aabb.min_z),
                (aabb.min_x, aabb.max_z),
                (aabb.max_x, aabb.min_z),
                (aabb.max_x, aabb.max_z),
            ];
            for (cx, cz) in corners {
                pos = block_pos_containing(cx, f64::from(y), cz);
                if self.can_start_at(ctx, pos) {
                    return Some(self.get_start_node(ctx, pos));
                }
            }
        } else {
            return Some(self.get_start_node(ctx, pos));
        }
        Some(self.get_start_node(
            ctx,
            BlockPos {
                x: block_pos.x,
                y,
                z: block_pos.z,
            },
        ))
    }

    /// `getTarget(double, double, double)`: `getTargetNodeAt`.
    fn get_target(
        &mut self,
        _ctx: &PathfindingContext<'_, M, L>,
        x: f64,
        y: f64,
        z: f64,
    ) -> Target {
        self.base.target_node_at(x, y, z)
    }

    /// `getNeighbors(Node[], Node)`, offsets 0..343.
    fn get_neighbors(
        &mut self,
        ctx: &PathfindingContext<'_, M, L>,
        out: &mut [Option<NodeId>; 32],
        node: NodeId,
    ) -> usize {
        let mob = ctx.mob;
        let mut n = 0usize;
        let mut max_up = 0i32;
        let (x, y, z) = {
            let nd = self.base.nodes.get(node);
            (nd.x, nd.y, nd.z)
        };
        let above = self.get_cached_path_type(ctx, x, y + 1, z);
        let at = self.get_cached_path_type(ctx, x, y, z);
        if mob.pathfinding_malus(above) >= 0.0 && at != PathType::StickyHoney {
            max_up = floor_f32(java_max_f32(1.0, mob.max_up_step()));
        }
        let floor_level = self.get_floor_level(ctx, BlockPos { x, y, z });
        for dir in Direction::HORIZONTAL {
            let nb = self.find_accepted_node(
                ctx,
                x + dir.step_x(),
                y,
                z + dir.step_z(),
                max_up,
                floor_level,
                dir,
                at,
            );
            #[allow(clippy::cast_sign_loss)]
            {
                self.reusable_neighbors[dir.data_2d() as usize] = nb;
            }
            if self.is_neighbor_valid(nb, node) {
                out[n] = nb;
                n += 1;
            }
        }
        for dir in Direction::HORIZONTAL {
            let cw = dir.clockwise();
            #[allow(clippy::cast_sign_loss)]
            let (a, b) = (
                self.reusable_neighbors[dir.data_2d() as usize],
                self.reusable_neighbors[cw.data_2d() as usize],
            );
            if self.is_diagonal_valid3(mob, node, a, b) {
                let nb = self.find_accepted_node(
                    ctx,
                    x + dir.step_x() + cw.step_x(),
                    y,
                    z + dir.step_z() + cw.step_z(),
                    max_up,
                    floor_level,
                    dir,
                    at,
                );
                if self.is_diagonal_valid1(nb) {
                    out[n] = nb;
                    n += 1;
                }
            }
        }
        n
    }

    /// `getPathType(PathfindingContext, int, int, int)`: `getPathTypeStatic`.
    fn get_path_type(
        &self,
        ctx: &PathfindingContext<'_, M, L>,
        x: i32,
        y: i32,
        z: i32,
    ) -> PathType {
        get_path_type_static(ctx, x, y, z)
    }
}
