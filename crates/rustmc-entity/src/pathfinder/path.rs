//! `net.minecraft.world.level.pathfinder.Path`, from the 26.2 jar.
//!
//! A list of nodes, a cursor (`nextNodeIndex`), the target block, the
//! Manhattan distance from the last node to it (`distToTarget`), and
//! whether the target was reached. The nodes are value copies of the
//! evaluator's (see [`super::node`]); `Node.equals` is by position, so
//! `sameAs` and `equals` compare positions.
//!
//! `getEntityPosAtNode(entity, i)` is the one place a mob's width enters
//! the path: `x + (int)(bbWidth + 1) * 0.5`, with the cast to `int`
//! before the multiply (`f2i` at offset 25, then `i2d`, then `dmul`).

use rustmc_data::BlockPos;

use super::node::Node;
use crate::phys::Vec3;

/// `Path`.
#[derive(Debug, Clone, PartialEq)]
pub struct Path {
    nodes: Vec<Node>,
    next_node_index: usize,
    target: BlockPos,
    dist_to_target: f32,
    reached: bool,
}

impl Path {
    /// `Path(List<Node>, BlockPos, boolean)`: `distToTarget` is
    /// `Float.MAX_VALUE` for an empty list, else the last node's
    /// `distanceManhattan(target)`.
    #[must_use]
    pub fn new(nodes: Vec<Node>, target: BlockPos, reached: bool) -> Self {
        let dist_to_target = match nodes.last() {
            None => f32::MAX,
            Some(last) => last.distance_manhattan_pos(target),
        };
        Path {
            nodes,
            next_node_index: 0,
            target,
            dist_to_target,
            reached,
        }
    }

    /// `advance()`: `nextNodeIndex++`.
    pub fn advance(&mut self) {
        self.next_node_index += 1;
    }

    /// `notStarted()`: `nextNodeIndex <= 0`.
    #[must_use]
    pub fn not_started(&self) -> bool {
        self.next_node_index == 0
    }

    /// `isDone()`: `nextNodeIndex >= nodes.size()`.
    #[must_use]
    pub fn is_done(&self) -> bool {
        self.next_node_index >= self.nodes.len()
    }

    /// `getEndNode()`: the last node, or `null` when empty.
    #[must_use]
    pub fn end_node(&self) -> Option<&Node> {
        self.nodes.last()
    }

    /// `getNode(int)`.
    #[must_use]
    pub fn node(&self, i: usize) -> &Node {
        &self.nodes[i]
    }

    /// `truncateNodes(int)`: `if (size > n) nodes.subList(n, size).clear()`.
    pub fn truncate_nodes(&mut self, n: usize) {
        if self.nodes.len() > n {
            self.nodes.truncate(n);
        }
    }

    /// `replaceNode(int, Node)`.
    pub fn replace_node(&mut self, i: usize, node: Node) {
        self.nodes[i] = node;
    }

    /// `getNodeCount()`.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// `getNextNodeIndex()`.
    #[must_use]
    pub fn next_node_index(&self) -> usize {
        self.next_node_index
    }

    /// `setNextNodeIndex(int)`.
    pub fn set_next_node_index(&mut self, i: usize) {
        self.next_node_index = i;
    }

    /// `getEntityPosAtNode(Entity, int)`:
    ///
    /// ```text
    /// x = node.x + (int)(bbWidth + 1.0f) * 0.5
    /// y = node.y
    /// z = node.z + (int)(bbWidth + 1.0f) * 0.5
    /// ```
    #[must_use]
    pub fn entity_pos_at_node(&self, bb_width: f32, i: usize) -> Vec3 {
        let n = &self.nodes[i];
        #[allow(clippy::cast_possible_truncation)]
        let w = f64::from((bb_width + 1.0) as i32) * 0.5;
        Vec3::new(f64::from(n.x) + w, f64::from(n.y), f64::from(n.z) + w)
    }

    /// `getNodePos(int)`.
    #[must_use]
    pub fn node_pos(&self, i: usize) -> BlockPos {
        self.nodes[i].as_block_pos()
    }

    /// `getNextEntityPos(Entity)`: `getEntityPosAtNode(entity, nextNodeIndex)`.
    #[must_use]
    pub fn next_entity_pos(&self, bb_width: f32) -> Vec3 {
        self.entity_pos_at_node(bb_width, self.next_node_index)
    }

    /// `getNextNodePos()`.
    #[must_use]
    pub fn next_node_pos(&self) -> BlockPos {
        self.nodes[self.next_node_index].as_block_pos()
    }

    /// `getNextNode()`.
    #[must_use]
    pub fn next_node(&self) -> &Node {
        &self.nodes[self.next_node_index]
    }

    /// `getPreviousNode()`: `nextNodeIndex > 0 ? nodes[nextNodeIndex - 1] : null`.
    #[must_use]
    pub fn previous_node(&self) -> Option<&Node> {
        if self.next_node_index > 0 {
            Some(&self.nodes[self.next_node_index - 1])
        } else {
            None
        }
    }

    /// `sameAs(Path)`: `other != null && nodes.equals(other.nodes)` — list
    /// equality, element-wise `Node.equals` (position).
    #[must_use]
    pub fn same_as(&self, other: Option<&Path>) -> bool {
        match other {
            None => false,
            Some(o) => {
                self.nodes.len() == o.nodes.len()
                    && self
                        .nodes
                        .iter()
                        .zip(o.nodes.iter())
                        .all(|(a, b)| a.same_position(b))
            }
        }
    }

    /// `canReach()`: the `reached` field.
    #[must_use]
    pub fn can_reach(&self) -> bool {
        self.reached
    }

    /// `getTarget()`.
    #[must_use]
    pub fn target(&self) -> BlockPos {
        self.target
    }

    /// `getDistToTarget()`.
    #[must_use]
    pub fn dist_to_target(&self) -> f32 {
        self.dist_to_target
    }

    /// The nodes, for tests and goldens.
    #[must_use]
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(coords: &[(i32, i32, i32)], target: BlockPos, reached: bool) -> Path {
        Path::new(
            coords.iter().map(|&(x, y, z)| Node::new(x, y, z)).collect(),
            target,
            reached,
        )
    }

    #[test]
    fn dist_to_target_is_from_the_last_node_or_max() {
        let t = BlockPos { x: 5, y: 0, z: 0 };
        assert_eq!(path(&[], t, false).dist_to_target(), f32::MAX);
        assert_eq!(
            path(&[(0, 0, 0), (2, 0, 0)], t, false).dist_to_target(),
            3.0
        );
    }

    #[test]
    fn the_cursor_walks_and_is_done_past_the_end() {
        let mut p = path(&[(0, 0, 0), (1, 0, 0)], BlockPos { x: 1, y: 0, z: 0 }, true);
        assert!(p.not_started());
        assert!(!p.is_done());
        assert_eq!(p.next_node_pos(), BlockPos { x: 0, y: 0, z: 0 });
        assert!(p.previous_node().is_none());
        p.advance();
        assert_eq!(p.previous_node().map(|n| n.x), Some(0));
        p.advance();
        assert!(p.is_done());
        assert!(p.can_reach());
    }

    /// `(int)(bbWidth + 1) * 0.5`: a 0.6-wide mob gets +0.5, a 1.4-wide
    /// one gets +1.0, a 2.0-wide one gets +1.5.
    #[test]
    fn entity_pos_at_node_casts_the_width_before_halving() {
        let p = path(
            &[(10, 64, -3)],
            BlockPos {
                x: 10,
                y: 64,
                z: -3,
            },
            true,
        );
        assert_eq!(p.entity_pos_at_node(0.6, 0), Vec3::new(10.5, 64.0, -2.5));
        assert_eq!(p.entity_pos_at_node(1.4, 0), Vec3::new(11.0, 64.0, -2.0));
        assert_eq!(p.entity_pos_at_node(2.0, 0), Vec3::new(11.5, 64.0, -1.5));
    }

    #[test]
    fn same_as_compares_positions_and_truncate_keeps_a_prefix() {
        let a = path(&[(0, 0, 0), (1, 0, 0)], BlockPos { x: 1, y: 0, z: 0 }, true);
        let mut b = path(
            &[(0, 0, 0), (1, 0, 0), (2, 0, 0)],
            BlockPos { x: 9, y: 0, z: 0 },
            false,
        );
        assert!(!a.same_as(Some(&b)));
        assert!(!a.same_as(None));
        b.truncate_nodes(2);
        assert!(a.same_as(Some(&b)));
        b.truncate_nodes(5);
        assert_eq!(b.node_count(), 2);
        b.replace_node(1, Node::new(1, 1, 0));
        assert!(!a.same_as(Some(&b)));
    }
}
