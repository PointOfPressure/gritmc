//! `net.minecraft.world.level.pathfinder.Node` and `Target`, from the 26.2
//! jar, plus the `Int2ObjectOpenHashMap<Node>` that `NodeEvaluator.nodes`
//! interns them in.
//!
//! # Identity
//!
//! In the jar a `Node` is an object: the evaluator's map, the open-set
//! heap, `cameFrom`, the neighbour array and the finished `Path` all hold
//! references to the same instances. Here the evaluator owns a [`Nodes`]
//! arena and everything else holds a [`NodeId`]; a finished [`Path`]
//! copies the nodes out as values, which is equivalent because the jar
//! never mutates a node after `reconstructPath` runs (the next `prepare`
//! clears the map and makes fresh ones).
//!
//! # The hash
//!
//! `NodeEvaluator.getNode(x, y, z)` is `nodes.computeIfAbsent(createHash(x, y, z), ...)`
//! — keyed by the **int hash**, not the coordinates. `createHash` packs
//! `y` into 8 bits, `x` and `z` into 15 bits each with a sign bit apiece,
//! so two positions that differ by 256 in `y`, or by 32768 in `x` or `z`,
//! alias to one node. That is what the jar does and what [`Nodes::get_or_create`]
//! does.
//!
//! [`Path`]: super::path::Path

use rustmc_data::BlockPos;
use std::collections::HashMap;

use super::path_type::PathType;
use crate::mth::sqrt_f32;
use crate::phys::Vec3;

/// Index of a node in a [`Nodes`] arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

/// `Node`. Public fields as in the jar.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    hash: i32,
    /// `heapIdx`, `-1` when not in the open set.
    pub heap_idx: i32,
    pub g: f32,
    pub h: f32,
    pub f: f32,
    pub came_from: Option<NodeId>,
    pub closed: bool,
    pub walked_distance: f32,
    pub cost_malus: f32,
    pub path_type: PathType,
}

impl Node {
    /// `Node(int, int, int)`: `heapIdx = -1`, `type = BLOCKED`.
    #[must_use]
    pub fn new(x: i32, y: i32, z: i32) -> Self {
        Node {
            x,
            y,
            z,
            hash: create_hash(x, y, z),
            heap_idx: -1,
            g: 0.0,
            h: 0.0,
            f: 0.0,
            came_from: None,
            closed: false,
            walked_distance: 0.0,
            cost_malus: 0.0,
            path_type: PathType::Blocked,
        }
    }

    /// `cloneAndMove(int, int, int)`: a new node at the given position
    /// carrying every other field of this one — `heapIdx` and `cameFrom`
    /// included.
    #[must_use]
    pub fn clone_and_move(&self, x: i32, y: i32, z: i32) -> Node {
        let mut n = Node::new(x, y, z);
        n.heap_idx = self.heap_idx;
        n.g = self.g;
        n.h = self.h;
        n.f = self.f;
        n.came_from = self.came_from;
        n.closed = self.closed;
        n.walked_distance = self.walked_distance;
        n.cost_malus = self.cost_malus;
        n.path_type = self.path_type;
        n
    }

    /// `hashCode()`: the `hash` field.
    #[must_use]
    pub fn hash(&self) -> i32 {
        self.hash
    }

    /// `distanceTo(Node)`: `Mth.sqrt(dx*dx + dy*dy + dz*dz)` in float.
    #[must_use]
    pub fn distance_to(&self, o: &Node) -> f32 {
        distance_f32(self.x, self.y, self.z, o.x, o.y, o.z)
    }

    /// `distanceTo(BlockPos)`.
    #[must_use]
    pub fn distance_to_pos(&self, p: BlockPos) -> f32 {
        distance_f32(self.x, self.y, self.z, p.x, p.y, p.z)
    }

    /// `distanceToXZ(Node)`.
    #[must_use]
    pub fn distance_to_xz(&self, o: &Node) -> f32 {
        #[allow(clippy::cast_precision_loss)]
        let dx = (o.x - self.x) as f32;
        #[allow(clippy::cast_precision_loss)]
        let dz = (o.z - self.z) as f32;
        sqrt_f32(dx * dx + dz * dz)
    }

    /// `distanceToSqr(Node)`.
    #[must_use]
    pub fn distance_to_sqr(&self, o: &Node) -> f32 {
        distance_sqr_f32(self.x, self.y, self.z, o.x, o.y, o.z)
    }

    /// `distanceManhattan(Node)`: `|dx| + |dy| + |dz|`, summed in float in
    /// that order.
    #[must_use]
    pub fn distance_manhattan(&self, o: &Node) -> f32 {
        manhattan_f32(self.x, self.y, self.z, o.x, o.y, o.z)
    }

    /// `distanceManhattan(BlockPos)`.
    #[must_use]
    pub fn distance_manhattan_pos(&self, p: BlockPos) -> f32 {
        manhattan_f32(self.x, self.y, self.z, p.x, p.y, p.z)
    }

    /// `asBlockPos()`.
    #[must_use]
    pub fn as_block_pos(&self) -> BlockPos {
        BlockPos {
            x: self.x,
            y: self.y,
            z: self.z,
        }
    }

    /// `asVec3()`.
    #[must_use]
    pub fn as_vec3(&self) -> Vec3 {
        Vec3::new(f64::from(self.x), f64::from(self.y), f64::from(self.z))
    }

    /// `inOpenSet()`: `heapIdx >= 0`.
    #[must_use]
    pub fn in_open_set(&self) -> bool {
        self.heap_idx >= 0
    }

    /// `equals(Object)`: same hash and same coordinates.
    #[must_use]
    pub fn same_position(&self, o: &Node) -> bool {
        self.hash == o.hash && self.x == o.x && self.y == o.y && self.z == o.z
    }
}

#[allow(clippy::cast_precision_loss)]
fn distance_f32(x1: i32, y1: i32, z1: i32, x2: i32, y2: i32, z2: i32) -> f32 {
    let dx = (x2 - x1) as f32;
    let dy = (y2 - y1) as f32;
    let dz = (z2 - z1) as f32;
    sqrt_f32(dx * dx + dy * dy + dz * dz)
}

#[allow(clippy::cast_precision_loss)]
fn distance_sqr_f32(x1: i32, y1: i32, z1: i32, x2: i32, y2: i32, z2: i32) -> f32 {
    let dx = (x2 - x1) as f32;
    let dy = (y2 - y1) as f32;
    let dz = (z2 - z1) as f32;
    dx * dx + dy * dy + dz * dz
}

#[allow(clippy::cast_precision_loss)]
fn manhattan_f32(x1: i32, y1: i32, z1: i32, x2: i32, y2: i32, z2: i32) -> f32 {
    let dx = (x2 - x1).abs() as f32;
    let dy = (y2 - y1).abs() as f32;
    let dz = (z2 - z1).abs() as f32;
    dx + dy + dz
}

/// `Node.createHash(int x, int y, int z)`:
///
/// ```text
/// (y & 255) | ((x & 32767) << 8) | ((z & 32767) << 24)
///   | (x < 0 ? Integer.MIN_VALUE : 0) | (z < 0 ? 32768 : 0)
/// ```
#[must_use]
pub fn create_hash(x: i32, y: i32, z: i32) -> i32 {
    (y & 255)
        | ((x & 32767) << 8)
        | ((z & 32767) << 24)
        | if x < 0 { i32::MIN } else { 0 }
        | if z < 0 { 32768 } else { 0 }
}

/// `NodeEvaluator.nodes`: an `Int2ObjectOpenHashMap<Node>` keyed by
/// `createHash`, plus the arena the ids index.
#[derive(Debug, Default)]
pub struct Nodes {
    nodes: Vec<Node>,
    by_hash: HashMap<i32, NodeId>,
}

impl Nodes {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `nodes.clear()` — `NodeEvaluator.prepare` does this.
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.by_hash.clear();
    }

    /// `NodeEvaluator.getNode(int, int, int)`:
    /// `nodes.computeIfAbsent(createHash(x, y, z), h -> new Node(x, y, z))`.
    ///
    /// Keyed by the hash, so an aliasing position returns the existing
    /// node at the *other* position, exactly as the jar does.
    pub fn get_or_create(&mut self, x: i32, y: i32, z: i32) -> NodeId {
        let h = create_hash(x, y, z);
        if let Some(id) = self.by_hash.get(&h) {
            return *id;
        }
        #[allow(clippy::cast_possible_truncation)]
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(Node::new(x, y, z));
        self.by_hash.insert(h, id);
        id
    }

    /// A node outside the map — `new Node(x, y, z)` held only by whoever
    /// asked (the `Target` constructor's copy, or a `cloneAndMove`).
    pub fn push_unmapped(&mut self, node: Node) -> NodeId {
        #[allow(clippy::cast_possible_truncation)]
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(node);
        id
    }

    #[must_use]
    pub fn get(&self, id: NodeId) -> &Node {
        &self.nodes[id.0 as usize]
    }

    pub fn get_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id.0 as usize]
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

/// `Target extends Node`: a node with a best-so-far record.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    /// The `Node` part — `Target(Node)` copies only `x, y, z` into a fresh
    /// node (`invokespecial Node.<init>(III)`), so this is a fresh node.
    pub node: Node,
    /// `bestHeuristic`, starts at `Float.MAX_VALUE`.
    pub best_heuristic: f32,
    /// `bestNode`.
    pub best_node: Option<NodeId>,
    /// `reached`.
    pub reached: bool,
}

impl Target {
    /// `Target(int, int, int)`.
    #[must_use]
    pub fn new(x: i32, y: i32, z: i32) -> Self {
        Target {
            node: Node::new(x, y, z),
            best_heuristic: f32::MAX,
            best_node: None,
            reached: false,
        }
    }

    /// `Target(Node)`: copies the position only.
    #[must_use]
    pub fn from_node(n: &Node) -> Self {
        Self::new(n.x, n.y, n.z)
    }

    /// `updateBest(float, Node)`: `if (h < bestHeuristic) { bestHeuristic = h; bestNode = n; }`.
    pub fn update_best(&mut self, h: f32, n: NodeId) {
        if h < self.best_heuristic {
            self.best_heuristic = h;
            self.best_node = Some(n);
        }
    }

    /// `setReached()`.
    pub fn set_reached(&mut self) {
        self.reached = true;
    }

    /// `isReached()`.
    #[must_use]
    pub fn is_reached(&self) -> bool {
        self.reached
    }

    /// `getBestNode()`.
    #[must_use]
    pub fn best_node(&self) -> Option<NodeId> {
        self.best_node
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The packing, including both sign bits.
    #[test]
    fn create_hash_packs_y8_x15_z15_and_two_sign_bits() {
        assert_eq!(create_hash(0, 0, 0), 0);
        assert_eq!(create_hash(0, 5, 0), 5);
        assert_eq!(create_hash(1, 0, 0), 1 << 8);
        assert_eq!(create_hash(0, 0, 1), 1 << 24);
        assert_eq!(create_hash(-1, 0, 0), (32767 << 8) | i32::MIN);
        assert_eq!(create_hash(0, 0, -1), (32767 << 24) | 32768);
        // y wraps at 256, so (0, 256, 0) aliases (0, 0, 0).
        assert_eq!(create_hash(0, 256, 0), create_hash(0, 0, 0));
    }

    /// `getNode` is keyed by the hash: an aliasing position returns the
    /// existing node, which sits at the other coordinates.
    #[test]
    fn nodes_intern_by_hash_not_by_position() {
        let mut nodes = Nodes::new();
        let a = nodes.get_or_create(3, 4, 5);
        let b = nodes.get_or_create(3, 4, 5);
        assert_eq!(a, b);
        let c = nodes.get_or_create(3, 4 + 256, 5);
        assert_eq!(a, c, "y aliases at 256");
        assert_eq!(nodes.get(c).y, 4);
        assert_eq!(nodes.len(), 1);
        nodes.clear();
        assert_ne!(nodes.get_or_create(3, 4, 5), NodeId(1));
        assert_eq!(nodes.len(), 1);
    }

    #[test]
    fn distances_are_float_arithmetic_in_the_jar_order() {
        let a = Node::new(0, 0, 0);
        let b = Node::new(3, 4, 0);
        assert_eq!(a.distance_to(&b), 5.0);
        assert_eq!(a.distance_to_sqr(&b), 25.0);
        assert_eq!(a.distance_manhattan(&b), 7.0);
        assert_eq!(a.distance_to_xz(&Node::new(3, 9, 4)), 5.0);
        assert_eq!(
            a.distance_manhattan_pos(BlockPos {
                x: -1,
                y: -2,
                z: -3
            }),
            6.0
        );
    }

    #[test]
    fn a_new_node_is_blocked_and_out_of_the_open_set() {
        let n = Node::new(1, 2, 3);
        assert_eq!(n.path_type, PathType::Blocked);
        assert_eq!(n.heap_idx, -1);
        assert!(!n.in_open_set());
        let mut moved = n.clone_and_move(1, 3, 3);
        moved.cost_malus = 2.0;
        assert_eq!(moved.y, 3);
        assert_eq!(moved.hash(), create_hash(1, 3, 3));
        assert!(!moved.same_position(&n));
    }

    #[test]
    fn target_update_best_is_strict() {
        let mut t = Target::new(0, 0, 0);
        assert_eq!(t.best_heuristic, f32::MAX);
        t.update_best(3.0, NodeId(7));
        t.update_best(3.0, NodeId(8));
        assert_eq!(t.best_node(), Some(NodeId(7)));
        t.update_best(2.5, NodeId(8));
        assert_eq!(t.best_node(), Some(NodeId(8)));
        assert!(!t.is_reached());
        t.set_reached();
        assert!(t.is_reached());
    }
}
