//! `net.minecraft.world.level.pathfinder.PathFinder`, from the 26.2 jar:
//! the A* over an evaluator's nodes.
//!
//! # `findPath(Node start, Map<Target, BlockPos> targets, float maxRange, int accuracy, float depthMultiplier)`, offsets 0..619
//!
//! ```text
//! start.g = 0; start.h = getBestH(start, targets); start.f = start.h
//! openSet.clear(); openSet.insert(start)
//! visited = 0; reached = newHashSetWithExpectedSize(targets.size())
//! maxVisited = (int)((float) maxVisitedNodes * depthMultiplier)
//! while !openSet.isEmpty():
//!   if ++visited >= maxVisited break                           (139..146)
//!   node = openSet.pop(); node.closed = true
//!   for t in targets: if node.distanceManhattan(t) <= accuracy: t.setReached(); reached.add(t)
//!   if !reached.isEmpty() break
//!   if node.distanceTo(start) >= maxRange continue              (255..266)
//!   n = eval.getNeighbors(neighbors, node)
//!   for nb in neighbors[0..n]:
//!     d = distance(node, nb)                                     (= node.distanceTo(nb))
//!     nb.walkedDistance = node.walkedDistance + d
//!     g = node.g + d + nb.costMalus
//!     if nb.walkedDistance < maxRange && (!nb.inOpenSet() || g < nb.g):
//!       nb.cameFrom = node; nb.g = g; nb.h = getBestH(nb, targets) * 1.5f
//!       if nb.inOpenSet(): openSet.changeCost(nb, nb.g + nb.h)
//!       else: nb.f = nb.g + nb.h; openSet.insert(nb)
//! best = reached nonempty
//!   ? reached.stream().map(t -> reconstructPath(t.bestNode, targets.get(t), true)).min(comparingInt(nodeCount))
//!   : targets.keySet().stream().map(t -> reconstructPath(t.bestNode, targets.get(t), false))
//!            .min(comparingDouble(distToTarget).thenComparingInt(nodeCount))
//! ```
//!
//! Three things a rewrite gets wrong:
//!
//! * **`visited` is incremented before the test**, so at most
//!   `maxVisited - 1` nodes are popped, and `maxVisited` is a float
//!   product truncated (`i2f; fmul; f2i`).
//! * **The heuristic is inflated by 1.5** (`FUDGING`) for every node but
//!   the start, whose `h` is the raw best distance.
//! * **A reached target ends the search before the popped node is
//!   expanded**, and the returned path ends at the target's *best* node,
//!   not at the node that reached it.
//!
//! # Tie-breaking across targets
//!
//! `Stream.min` keeps the first minimum in stream order, and both streams
//! walk Java hash containers keyed by `Node.hashCode()` (`createHash`).
//! For a single target — `createPath(BlockPos, int)`, every goal in this
//! lane — there is nothing to break. For many targets the walk order is
//! reproduced by [`java_hash_walk_order`], which orders keys the way a
//! `HashMap` of that size iterates them: bucket `(h ^ (h >>> 16)) & (n-1)`
//! ascending, insertion order within a bucket. `Collectors.toMap` starts
//! at 16 and doubles past 0.75 load; Guava's `newHashSetWithExpectedSize`
//! sizes for the expected count up front. Both are computed here; the
//! *input* set's order (the order targets are handed in) is the caller's,
//! because it is the caller's `Set<BlockPos>`.

use rustmc_data::BlockPos;

use super::binary_heap::BinaryHeap;
use super::context::PathfindingContext;
use super::node::{Node, NodeId, Target};
use super::node_evaluator::NodeEvaluator;
use super::path::Path;
use crate::host::{MobHost, PathLevel};
use crate::mth::java_min_f32;

/// `FUDGING`.
pub const FUDGING: f32 = 1.5;

/// `PathFinder`.
pub struct PathFinder<E> {
    /// `neighbors`: `new Node[32]`.
    neighbors: [Option<NodeId>; 32],
    max_visited_nodes: i32,
    /// `nodeEvaluator`.
    pub node_evaluator: E,
    open_set: BinaryHeap,
}

impl<E> PathFinder<E> {
    /// `PathFinder(NodeEvaluator, int maxVisitedNodes)`.
    #[must_use]
    pub fn new(node_evaluator: E, max_visited_nodes: i32) -> Self {
        PathFinder {
            neighbors: [None; 32],
            max_visited_nodes,
            node_evaluator,
            open_set: BinaryHeap::new(),
        }
    }

    /// `setMaxVisitedNodes(int)`.
    pub fn set_max_visited_nodes(&mut self, n: i32) {
        self.max_visited_nodes = n;
    }

    /// `maxVisitedNodes`.
    #[must_use]
    pub fn max_visited_nodes(&self) -> i32 {
        self.max_visited_nodes
    }

    /// `findPath(PathNavigationRegion, Mob, Set<BlockPos>, float, int, float)`.
    ///
    /// `targets` is the `Set<BlockPos>` in the order that set iterates —
    /// the order `Collectors.toMap` inserts them.
    pub fn find_path<M: MobHost, L: PathLevel>(
        &mut self,
        ctx: &PathfindingContext<'_, M, L>,
        targets: &[BlockPos],
        max_range: f32,
        accuracy: i32,
        search_depth_multiplier: f32,
    ) -> Option<Path>
    where
        E: NodeEvaluator<M, L>,
    {
        self.open_set.clear();
        self.node_evaluator.prepare(ctx);
        let start = self.node_evaluator.get_start(ctx)?;
        // Collectors.toMap(pos -> eval.getTarget(x, y, z), identity):
        // one Target per BlockPos, in the set's order; a duplicate key is
        // an IllegalStateException in the jar.
        let mut map: Vec<(Target, BlockPos)> = Vec::with_capacity(targets.len());
        for p in targets {
            let t =
                self.node_evaluator
                    .get_target(ctx, f64::from(p.x), f64::from(p.y), f64::from(p.z));
            assert!(
                !map.iter()
                    .any(|(existing, _)| existing.node.same_position(&t.node)),
                "Duplicate key {:?} (attempted merging values)",
                p
            );
            map.push((t, *p));
        }
        let path = self.find_path_inner(
            start,
            map,
            max_range,
            accuracy,
            search_depth_multiplier,
            ctx,
        );
        self.node_evaluator.done();
        path
    }

    /// The private `findPath(Node, Map<Target, BlockPos>, float, int, float)`.
    #[allow(clippy::too_many_lines)]
    fn find_path_inner<M: MobHost, L: PathLevel>(
        &mut self,
        start: NodeId,
        mut map: Vec<(Target, BlockPos)>,
        max_range: f32,
        accuracy: i32,
        search_depth_multiplier: f32,
        ctx: &PathfindingContext<'_, M, L>,
    ) -> Option<Path>
    where
        E: NodeEvaluator<M, L>,
    {
        // targets.keySet() iterates in HashMap order; the map was built
        // by toMap from a default-capacity HashMap.
        let key_order = java_hash_walk_order(
            &map.iter().map(|(t, _)| t.node.hash()).collect::<Vec<_>>(),
            hashmap_table_size_after(map.len()),
        );
        {
            let nodes = &mut self.node_evaluator.base_mut().nodes;
            nodes.get_mut(start).g = 0.0;
            let h = get_best_h(nodes, start, &mut map, &key_order);
            let s = nodes.get_mut(start);
            s.h = h;
            s.f = h;
        }
        self.open_set.clear();
        self.open_set
            .insert(&mut self.node_evaluator.base_mut().nodes, start);
        let mut visited = 0i32;
        // Sets.newHashSetWithExpectedSize(targets.size()): insertion order
        // of the reached targets, walked later in that set's hash order.
        let mut reached: Vec<usize> = Vec::new();
        #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
        let max_visited = (self.max_visited_nodes as f32 * search_depth_multiplier) as i32;
        while !self.open_set.is_empty() {
            visited += 1;
            if visited >= max_visited {
                break;
            }
            let node = self.open_set.pop(&mut self.node_evaluator.base_mut().nodes);
            self.node_evaluator.base_mut().nodes.get_mut(node).closed = true;
            for &ti in &key_order {
                let (t, _) = &mut map[ti];
                let nd = self.node_evaluator.base().nodes.get(node);
                // `accuracy` is what makes `PathNavigation.moveTo` stop one
                // node short of the block it was told to walk to, and it is
                // the one place a T1 positive control can reach `ground_path`
                // and `navigation`, neither of which reads a fixture to
                // perturb. `rustmc_data::sabotage::Arm::PathAccuracy` forces
                // it to 0, which is the shape of this loop before the argument
                // was ported: the path then ends ON the target block. Off
                // without `--features t1-sabotage`, where `fires` is a
                // constant `false` and this folds to `accuracy`.
                let accuracy =
                    if rustmc_data::sabotage::fires(rustmc_data::sabotage::Arm::PathAccuracy) {
                        0
                    } else {
                        accuracy
                    };
                #[allow(clippy::cast_precision_loss)]
                if nd.distance_manhattan(&t.node) <= accuracy as f32 {
                    t.set_reached();
                    if !reached.contains(&ti) {
                        reached.push(ti);
                    }
                }
            }
            if !reached.is_empty() {
                break;
            }
            {
                let nodes = &self.node_evaluator.base().nodes;
                if nodes.get(node).distance_to(nodes.get(start)) >= max_range {
                    continue;
                }
            }
            let mut neighbors = self.neighbors;
            let n = self.node_evaluator.get_neighbors(ctx, &mut neighbors, node);
            self.neighbors = neighbors;
            for i in 0..n {
                let nb = self.neighbors[i].expect("getNeighbors fills 0..n");
                let nodes = &mut self.node_evaluator.base_mut().nodes;
                let d = nodes.get(node).distance_to(nodes.get(nb));
                let walked = nodes.get(node).walked_distance + d;
                nodes.get_mut(nb).walked_distance = walked;
                let g = nodes.get(node).g + d + nodes.get(nb).cost_malus;
                if walked < max_range && (!nodes.get(nb).in_open_set() || g < nodes.get(nb).g) {
                    nodes.get_mut(nb).came_from = Some(node);
                    nodes.get_mut(nb).g = g;
                    let h = get_best_h(nodes, nb, &mut map, &key_order) * FUDGING;
                    nodes.get_mut(nb).h = h;
                    if nodes.get(nb).in_open_set() {
                        let f = nodes.get(nb).g + nodes.get(nb).h;
                        self.open_set.change_cost(nodes, nb, f);
                    } else {
                        let f = nodes.get(nb).g + nodes.get(nb).h;
                        nodes.get_mut(nb).f = f;
                        self.open_set.insert(nodes, nb);
                    }
                }
            }
        }
        let nodes = &self.node_evaluator.base().nodes;
        let best: Option<Path> = if reached.is_empty() {
            // targets.keySet().stream()...min(comparingDouble(distToTarget).thenComparingInt(nodeCount))
            let mut best: Option<Path> = None;
            for &ti in &key_order {
                let (t, pos) = &map[ti];
                let p = reconstruct_path(
                    nodes,
                    t.best_node().expect("start updates every target"),
                    *pos,
                    false,
                );
                best = Some(match best {
                    None => p,
                    Some(b) => {
                        let ord = f64::from(b.dist_to_target())
                            .partial_cmp(&f64::from(p.dist_to_target()))
                            .unwrap_or(std::cmp::Ordering::Equal)
                            .then(b.node_count().cmp(&p.node_count()));
                        if ord.is_le() {
                            b
                        } else {
                            p
                        }
                    }
                });
            }
            best
        } else {
            // reached.stream()...min(comparingInt(nodeCount)), in the
            // reached set's hash order.
            let hashes: Vec<i32> = reached.iter().map(|&ti| map[ti].0.node.hash()).collect();
            let order = java_hash_walk_order(&hashes, guava_expected_table_size(map.len()));
            let mut best: Option<Path> = None;
            for &ri in &order {
                let ti = reached[ri];
                let (t, pos) = &map[ti];
                let p = reconstruct_path(
                    nodes,
                    t.best_node().expect("start updates every target"),
                    *pos,
                    true,
                );
                best = Some(match best {
                    None => p,
                    Some(b) => {
                        if b.node_count() <= p.node_count() {
                            b
                        } else {
                            p
                        }
                    }
                });
            }
            best
        };
        best
    }
}

/// `getBestH(Node, Set<Target>)`: the smallest `distanceTo` over the
/// targets, updating each target's best on the way. Walked in the key
/// set's hash order, which `updateBest` and `Math.min` do not depend on,
/// but is kept so the call sequence matches.
fn get_best_h(
    nodes: &super::node::Nodes,
    node: NodeId,
    map: &mut [(Target, BlockPos)],
    key_order: &[usize],
) -> f32 {
    let mut best = f32::MAX;
    for &ti in key_order {
        let (t, _) = &mut map[ti];
        let d = nodes.get(node).distance_to(&t.node);
        t.update_best(d, node);
        best = java_min_f32(d, best);
    }
    best
}

/// `reconstructPath(Node, BlockPos, boolean)`: walk `cameFrom` to the
/// root, building the list front-first.
fn reconstruct_path(
    nodes: &super::node::Nodes,
    end: NodeId,
    target: BlockPos,
    reached: bool,
) -> Path {
    let mut list: Vec<Node> = Vec::new();
    let mut n = end;
    list.insert(0, nodes.get(n).clone());
    while let Some(prev) = nodes.get(n).came_from {
        n = prev;
        list.insert(0, nodes.get(n).clone());
    }
    Path::new(list, target, reached)
}

/// `java.util.HashMap.hash(Object)`: `h ^ (h >>> 16)`.
#[must_use]
pub fn java_hashmap_spread(h: i32) -> i32 {
    h ^ ((h as u32) >> 16) as i32
}

/// `HashMap.tableSizeFor(int)`: the next power of two at or above `cap`,
/// at least 1.
#[must_use]
pub fn table_size_for(cap: usize) -> usize {
    cap.max(1).next_power_of_two()
}

/// The table size a default `HashMap` has after `n` distinct insertions:
/// starts at 16, doubles whenever `size > 0.75 * table`.
#[must_use]
pub fn hashmap_table_size_after(n: usize) -> usize {
    let mut table = 16usize;
    while n > table * 3 / 4 {
        table *= 2;
    }
    table
}

/// `Sets.newHashSetWithExpectedSize(n)`: `new HashSet<>(Maps.capacity(n))`
/// with `capacity(n) = n < 3 ? n + 1 : (int)((float) n / 0.75f + 1.0f)`,
/// then `HashMap.tableSizeFor` of that.
#[must_use]
pub fn guava_expected_table_size(n: usize) -> usize {
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let cap = if n < 3 {
        n + 1
    } else {
        (n as f32 / 0.75f32 + 1.0f32) as usize
    };
    table_size_for(cap)
}

/// The order a `HashMap`/`HashSet` of `table` buckets iterates keys with
/// these hash codes, inserted in slice order: buckets ascending, insertion
/// order within a bucket. Returns indices into `hashes`.
#[must_use]
pub fn java_hash_walk_order(hashes: &[i32], table: usize) -> Vec<usize> {
    let mask = (table - 1) as i32;
    let mut order: Vec<usize> = Vec::with_capacity(hashes.len());
    for bucket in 0..table as i32 {
        for (i, h) in hashes.iter().enumerate() {
            if (java_hashmap_spread(*h) & mask) == bucket {
                order.push(i);
            }
        }
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_sizes() {
        assert_eq!(hashmap_table_size_after(1), 16);
        assert_eq!(hashmap_table_size_after(12), 16);
        assert_eq!(hashmap_table_size_after(13), 32);
        assert_eq!(hashmap_table_size_after(25), 64);
        assert_eq!(guava_expected_table_size(1), 2);
        assert_eq!(guava_expected_table_size(2), 4);
        assert_eq!(guava_expected_table_size(3), 8);
        assert_eq!(guava_expected_table_size(12), 32);
    }

    #[test]
    fn walk_order_is_bucket_then_insertion() {
        // Hashes 0 and 16 share bucket 0 in a 16-table; 1 is bucket 1.
        assert_eq!(java_hash_walk_order(&[16, 1, 0], 16), vec![0, 2, 1]);
        // In a 32-table, 16 goes to bucket 16.
        assert_eq!(java_hash_walk_order(&[16, 1, 0], 32), vec![2, 1, 0]);
        // The spread folds the high half in: 1 << 16 lands in bucket 1.
        assert_eq!(java_hash_walk_order(&[1 << 16, 0], 16), vec![1, 0]);
    }
}
