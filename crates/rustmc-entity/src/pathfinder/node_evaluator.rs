//! `net.minecraft.world.level.pathfinder.NodeEvaluator`, from the 26.2 jar:
//! the state every evaluator shares and the interface `PathFinder` drives.
//!
//! The jar's abstract class holds the node map, the mob's rounded
//! dimensions and four capability flags. `canPassDoors` starts **true**
//! (`iconst_1; putfield canPassDoors` in the constructor); the other three
//! start false.
//!
//! `prepare` rounds the mob's size with `Mth.floor(bbWidth + 1.0f)` for
//! both width and depth and `Mth.floor(bbHeight + 1.0f)` for height, so a
//! 0.6-wide, 1.95-tall zombie occupies a 1x2x1 block volume and a 1.4-wide
//! spider occupies 2x2x2.

use rustmc_data::BlockPos;

use super::context::PathfindingContext;
use super::node::{NodeId, Nodes, Target};
use super::path_type::PathType;
use crate::host::{MobHost, PathLevel};
use crate::mth::{floor, floor_f32};

/// The fields of `NodeEvaluator`.
#[derive(Debug)]
pub struct NodeEvaluatorBase {
    /// `nodes`.
    pub nodes: Nodes,
    /// `entityWidth`, `entityHeight`, `entityDepth`.
    pub entity_width: i32,
    pub entity_height: i32,
    pub entity_depth: i32,
    /// `canPassDoors` — true from the constructor.
    pub can_pass_doors: bool,
    pub can_open_doors: bool,
    pub can_float: bool,
    pub can_walk_over_fences: bool,
}

impl Default for NodeEvaluatorBase {
    fn default() -> Self {
        Self::new()
    }
}

impl NodeEvaluatorBase {
    /// `NodeEvaluator()`.
    #[must_use]
    pub fn new() -> Self {
        NodeEvaluatorBase {
            nodes: Nodes::new(),
            entity_width: 0,
            entity_height: 0,
            entity_depth: 0,
            can_pass_doors: true,
            can_open_doors: false,
            can_float: false,
            can_walk_over_fences: false,
        }
    }

    /// `prepare(PathNavigationRegion, Mob)`: clear the map, round the
    /// dimensions.
    pub fn prepare<M: MobHost>(&mut self, mob: &M) {
        self.nodes.clear();
        self.entity_width = floor_f32(mob.bb_width() + 1.0);
        self.entity_height = floor_f32(mob.bb_height() + 1.0);
        self.entity_depth = floor_f32(mob.bb_width() + 1.0);
    }

    /// `getNode(BlockPos)`.
    pub fn node_at(&mut self, p: BlockPos) -> NodeId {
        self.nodes.get_or_create(p.x, p.y, p.z)
    }

    /// `getNode(int, int, int)`.
    pub fn node(&mut self, x: i32, y: i32, z: i32) -> NodeId {
        self.nodes.get_or_create(x, y, z)
    }

    /// `getTargetNodeAt(double, double, double)`:
    /// `new Target(getNode(floor(x), floor(y), floor(z)))`. The node is
    /// interned in the map as a side effect, exactly as in the jar.
    pub fn target_node_at(&mut self, x: f64, y: f64, z: f64) -> Target {
        let id = self.node(floor(x), floor(y), floor(z));
        Target::from_node(self.nodes.get(id))
    }
}

/// What `PathFinder.findPath` asks of an evaluator. `WalkNodeEvaluator`
/// is the one implementation here; the fly, swim and amphibious
/// evaluators are out of this lane's scope and would implement the same
/// four calls.
pub trait NodeEvaluator<M: MobHost, L: PathLevel> {
    /// The shared state.
    fn base(&self) -> &NodeEvaluatorBase;
    fn base_mut(&mut self) -> &mut NodeEvaluatorBase;

    /// `prepare(PathNavigationRegion, Mob)`.
    fn prepare(&mut self, ctx: &PathfindingContext<'_, M, L>);

    /// `done()`.
    fn done(&mut self);

    /// `getStart()`: `null` becomes `None`.
    fn get_start(&mut self, ctx: &PathfindingContext<'_, M, L>) -> Option<NodeId>;

    /// `getTarget(double, double, double)`.
    fn get_target(&mut self, ctx: &PathfindingContext<'_, M, L>, x: f64, y: f64, z: f64) -> Target;

    /// `getNeighbors(Node[], Node)`: fills `out` from index 0 and returns
    /// the count.
    fn get_neighbors(
        &mut self,
        ctx: &PathfindingContext<'_, M, L>,
        out: &mut [Option<NodeId>; 32],
        node: NodeId,
    ) -> usize;

    /// `getPathType(PathfindingContext, int, int, int)`: the raw type at a
    /// position, without the mob's volume.
    fn get_path_type(&self, ctx: &PathfindingContext<'_, M, L>, x: i32, y: i32, z: i32)
        -> PathType;

    /// `canFloat()` and friends, on the base.
    fn can_float(&self) -> bool {
        self.base().can_float
    }
    fn set_can_float(&mut self, v: bool) {
        self.base_mut().can_float = v;
    }
    fn can_open_doors(&self) -> bool {
        self.base().can_open_doors
    }
    fn set_can_open_doors(&mut self, v: bool) {
        self.base_mut().can_open_doors = v;
    }
    fn can_pass_doors(&self) -> bool {
        self.base().can_pass_doors
    }
    fn set_can_pass_doors(&mut self, v: bool) {
        self.base_mut().can_pass_doors = v;
    }
    fn can_walk_over_fences(&self) -> bool {
        self.base().can_walk_over_fences
    }
    fn set_can_walk_over_fences(&mut self, v: bool) {
        self.base_mut().can_walk_over_fences = v;
    }
}
