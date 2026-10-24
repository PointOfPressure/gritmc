//! `net.minecraft.world.level.pathfinder.BinaryHeap`, from the 26.2 jar.
//!
//! A min-heap on `Node.f`, with each node carrying its own `heapIdx` so
//! `changeCost` and `remove` can find it. Tie-breaking is the whole reason
//! this is transcribed rather than replaced by `std::collections::BinaryHeap`:
//! `upHeap` moves a node past its parent only while `f < parent.f`
//! (`fcmpg; ifge` at offset 32..39), and `downHeap` prefers the left child
//! on a tie (`fcmpg; ifge 123` at 88..93 takes the right child only when
//! `left.f >= right.f` is false, i.e. strictly greater), then moves only
//! while the child is strictly smaller than the node. Equal-`f` nodes
//! therefore come out in an order fixed by insertion history, and that order
//! decides which of two equally good paths a mob takes.
//!
//! The array starts at 128 and doubles (`size << 1`) when full. `pop` nulls
//! the vacated slot and sets the popped node's `heapIdx` to `-1`; `insert`
//! throws `IllegalStateException("OW KNOWS!")` on a node whose `heapIdx`
//! is already non-negative, which is reproduced as a panic because the
//! pathfinder relies on it never happening.

use super::node::{NodeId, Nodes};

/// `BinaryHeap`.
#[derive(Debug, Default)]
pub struct BinaryHeap {
    heap: Vec<NodeId>,
}

impl BinaryHeap {
    /// `BinaryHeap()`: an empty heap. The jar's backing array is 128 long;
    /// capacity is not behaviour, so a `Vec` grows as it likes.
    #[must_use]
    pub fn new() -> Self {
        Self { heap: Vec::new() }
    }

    /// `insert(Node)`.
    ///
    /// # Panics
    ///
    /// On a node already in the heap (`heapIdx >= 0`), as the jar throws.
    pub fn insert(&mut self, nodes: &mut Nodes, id: NodeId) -> NodeId {
        assert!(nodes.get(id).heap_idx < 0, "OW KNOWS!");
        #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
        let idx = self.heap.len() as i32;
        self.heap.push(id);
        nodes.get_mut(id).heap_idx = idx;
        self.up_heap(nodes, idx as usize);
        id
    }

    /// `clear()`: `size = 0`. The nodes' `heapIdx` fields are **not**
    /// reset — the jar leaves them, and `NodeEvaluator.prepare` clearing
    /// the node map is what makes the next search start clean.
    pub fn clear(&mut self) {
        self.heap.clear();
    }

    /// `peek()`: `heap[0]`.
    #[must_use]
    pub fn peek(&self) -> Option<NodeId> {
        self.heap.first().copied()
    }

    /// `pop()`: take `heap[0]`, move the last node into slot 0, `downHeap(0)`
    /// if anything is left, and mark the popped node `heapIdx = -1`.
    ///
    /// # Panics
    ///
    /// On an empty heap (the jar reads `heap[0]`, which is `null`, and then
    /// NPEs on `heapIdx`); `PathFinder` checks `isEmpty` first.
    pub fn pop(&mut self, nodes: &mut Nodes) -> NodeId {
        let top = self.heap[0];
        let last = self.heap.pop().expect("pop on empty heap");
        if !self.heap.is_empty() {
            self.heap[0] = last;
            self.down_heap(nodes, 0);
        }
        nodes.get_mut(top).heap_idx = -1;
        top
    }

    /// `remove(Node)`.
    pub fn remove(&mut self, nodes: &mut Nodes, id: NodeId) {
        #[allow(clippy::cast_sign_loss)]
        let idx = nodes.get(id).heap_idx as usize;
        let last = self.heap.pop().expect("remove on empty heap");
        if idx < self.heap.len() {
            self.heap[idx] = last;
            if nodes.get(last).f < nodes.get(id).f {
                self.up_heap(nodes, idx);
            } else {
                self.down_heap(nodes, idx);
            }
        }
        nodes.get_mut(id).heap_idx = -1;
    }

    /// `changeCost(Node, float)`: set `f`, then `upHeap` if it fell and
    /// `downHeap` otherwise (a tie goes down).
    pub fn change_cost(&mut self, nodes: &mut Nodes, id: NodeId, f: f32) {
        let old = nodes.get(id).f;
        nodes.get_mut(id).f = f;
        #[allow(clippy::cast_sign_loss)]
        let idx = nodes.get(id).heap_idx as usize;
        if f < old {
            self.up_heap(nodes, idx);
        } else {
            self.down_heap(nodes, idx);
        }
    }

    /// `size()`.
    #[must_use]
    pub fn size(&self) -> usize {
        self.heap.len()
    }

    /// `isEmpty()`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }

    /// `getHeap()`: the live prefix, copied.
    #[must_use]
    pub fn heap(&self) -> Vec<NodeId> {
        self.heap.clone()
    }

    /// `upHeap(int)`: sift up while `f < parent.f`.
    fn up_heap(&mut self, nodes: &mut Nodes, mut i: usize) {
        let node = self.heap[i];
        let f = nodes.get(node).f;
        while i > 0 {
            let parent_i = (i - 1) >> 1;
            let parent = self.heap[parent_i];
            if f < nodes.get(parent).f {
                self.heap[i] = parent;
                #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
                {
                    nodes.get_mut(parent).heap_idx = i as i32;
                }
                i = parent_i;
            } else {
                break;
            }
        }
        self.heap[i] = node;
        #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
        {
            nodes.get_mut(node).heap_idx = i as i32;
        }
    }

    /// `downHeap(int)`:
    ///
    /// ```text
    /// loop:
    ///   l = 2i + 1; r = l + 1
    ///   if l >= size break
    ///   lf = heap[l].f
    ///   rf = r < size ? heap[r].f : +Infinity
    ///   if lf < rf:                       (88..93, fcmpg ifge)
    ///     if lf >= f break                (96..100)
    ///     heap[i] = left; i = l
    ///   else:
    ///     if rf >= f break                (123..127)
    ///     heap[i] = right; i = r
    /// heap[i] = node
    /// ```
    fn down_heap(&mut self, nodes: &mut Nodes, mut i: usize) {
        let node = self.heap[i];
        let f = nodes.get(node).f;
        loop {
            let l = 1 + (i << 1);
            let r = l + 1;
            if l >= self.heap.len() {
                break;
            }
            let left = self.heap[l];
            let lf = nodes.get(left).f;
            let (right, rf) = if r >= self.heap.len() {
                (None, f32::INFINITY)
            } else {
                let right = self.heap[r];
                (Some(right), nodes.get(right).f)
            };
            if lf < rf {
                if lf >= f {
                    break;
                }
                self.heap[i] = left;
                #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
                {
                    nodes.get_mut(left).heap_idx = i as i32;
                }
                i = l;
            } else {
                if rf >= f {
                    break;
                }
                let right = right.expect("rf finite implies a right child");
                self.heap[i] = right;
                #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
                {
                    nodes.get_mut(right).heap_idx = i as i32;
                }
                i = r;
            }
        }
        self.heap[i] = node;
        #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
        {
            nodes.get_mut(node).heap_idx = i as i32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(nodes: &mut Nodes, i: i32, f: f32) -> NodeId {
        let id = nodes.get_or_create(i, 0, 0);
        nodes.get_mut(id).f = f;
        id
    }

    fn drain(heap: &mut BinaryHeap, nodes: &mut Nodes) -> Vec<i32> {
        let mut out = Vec::new();
        while !heap.is_empty() {
            let id = heap.pop(nodes);
            out.push(nodes.get(id).x);
        }
        out
    }

    #[test]
    fn pops_in_ascending_f() {
        let mut nodes = Nodes::new();
        let mut heap = BinaryHeap::new();
        for (i, f) in [(0, 5.0), (1, 1.0), (2, 3.0), (3, 4.0), (4, 2.0)] {
            let id = node(&mut nodes, i, f);
            heap.insert(&mut nodes, id);
        }
        assert_eq!(heap.size(), 5);
        assert_eq!(drain(&mut heap, &mut nodes), vec![1, 4, 2, 3, 0]);
        assert!(heap.is_empty());
    }

    /// Equal `f` values: the order is fixed by the sift rules, not by
    /// insertion order alone. Five equal nodes inserted 0..5 come out
    /// 0, 4, 3, 2, 1: a strict-less sift-up never moves an equal node past
    /// its parent, and each pop moves the last node to the root and sifts it
    /// down only past strictly smaller children, so the root is always the
    /// most recently moved-up tail.
    #[test]
    fn equal_costs_come_out_in_the_sift_order_not_insertion_order() {
        let mut nodes = Nodes::new();
        let mut heap = BinaryHeap::new();
        for i in 0..5 {
            let id = node(&mut nodes, i, 1.0);
            heap.insert(&mut nodes, id);
        }
        assert_eq!(drain(&mut heap, &mut nodes), vec![0, 4, 3, 2, 1]);
    }

    #[test]
    fn change_cost_reorders_and_pop_clears_heap_idx() {
        let mut nodes = Nodes::new();
        let mut heap = BinaryHeap::new();
        let a = node(&mut nodes, 0, 5.0);
        let b = node(&mut nodes, 1, 6.0);
        let c = node(&mut nodes, 2, 7.0);
        heap.insert(&mut nodes, a);
        heap.insert(&mut nodes, b);
        heap.insert(&mut nodes, c);
        assert!(nodes.get(c).in_open_set());
        heap.change_cost(&mut nodes, c, 1.0);
        assert_eq!(heap.peek(), Some(c));
        let popped = heap.pop(&mut nodes);
        assert_eq!(popped, c);
        assert_eq!(nodes.get(c).heap_idx, -1);
        assert!(!nodes.get(c).in_open_set());
        heap.remove(&mut nodes, a);
        assert_eq!(nodes.get(a).heap_idx, -1);
        assert_eq!(drain(&mut heap, &mut nodes), vec![1]);
    }

    #[test]
    #[should_panic(expected = "OW KNOWS!")]
    fn inserting_a_node_twice_throws() {
        let mut nodes = Nodes::new();
        let mut heap = BinaryHeap::new();
        let a = node(&mut nodes, 0, 5.0);
        heap.insert(&mut nodes, a);
        heap.insert(&mut nodes, a);
    }
}
