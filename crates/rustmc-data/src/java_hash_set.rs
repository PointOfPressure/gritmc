//! `java.util.HashSet` iteration order, reproduced — including treeified bins.
//!
//! Reference (OpenJDK, behavior ported — never transcribed, CLAUDE.md rule 4):
//! `java.util.HashMap` (`hash`, `putVal`, `resize`, `treeifyBin`,
//! `HashIterator`) and `java.util.HashMap$TreeNode` (`treeify`, `untreeify`,
//! `split`, `putTreeVal`, `removeTreeNode`, `moveRootToFront`,
//! `balanceInsertion`, `balanceDeletion`, `rotateLeft`, `rotateRight`), read
//! from `javap -p -c` against the JDK the oracle runs on (25.0.3).
//! `java.util.HashSet` is a `HashMap` with a constant value.
//!
//! # Why a worldgen crate contains a hash table
//!
//! `TreeFeature.place` collects its logs, foliage, roots and decorations into
//! `Sets.newHashSet()`, and what comes out of those sets is **behaviour**:
//! `TreeDecorator.Context` sorts the logs by y with a *stable* sort, so
//! everything at the same height keeps the set's own order, and a beehive
//! then attaches to the first entry that qualifies. A port using a `BTreeSet`
//! plants the same tree with the hive on the wrong side.
//!
//! Java's order is not arbitrary. It is a pure function of the keys' hash
//! codes and the operation sequence, so it can be reproduced exactly — and
//! this does, including the parts that are easy to miss:
//!
//! - **The spread.** `HashMap.hash(key)` is `h ^ (h >>> 16)`, an *unsigned*
//!   shift, which mixes the high bits down so that a table smaller than
//!   65,536 buckets still sees them. `BlockPos.hashCode` is
//!   `(y + z * 31) * 31 + x`, whose low bits move slowly along a row of
//!   blocks, so without the spread every log in a trunk would land in a
//!   handful of buckets.
//! - **The order.** Iteration walks the bucket array from index 0 upward and
//!   each bucket's chain from its head, and a new entry is appended to the
//!   **tail** of its chain. So order is neither insertion order nor sorted
//!   order; it is bucket-major, insertion-minor.
//! - **The resize.** At more than `capacity * 0.75` entries the table doubles,
//!   and each old bucket splits in two — entries whose extra hash bit is 0
//!   stay at `j`, the rest move to `j + oldCapacity` — with **relative order
//!   preserved** in both halves. This is the part a naive rehash gets wrong:
//!   rebuilding by re-inserting in iteration order happens to give the same
//!   answer, but rebuilding in any other order does not.
//! - **The tree.** A long enough bucket stops being a list. See below.
//!
//! # Treeification
//!
//! Past a threshold a bucket becomes a red-black tree, and iteration order
//! stops being insertion order for that bucket. This used to be a refusal
//! here; over one measured window (seed 20260722, the plains-village square)
//! that refusal cost ~14,000 blocks, every one of them spruce, because a
//! spruce carries enough foliage positions to trip the threshold where oak
//! and birch do not.
//!
//! Three details decide the order, and all three were read from the bytecode
//! rather than reasoned about:
//!
//! - **The trigger is nine, not eight.** `putVal` walks the chain with a
//!   `binCount` that starts at zero *on the head*, so when it appends past a
//!   chain of eight the count is seven and the test is `binCount >= 7`. The
//!   bin is nine long by the time `treeifyBin` sees it. `TREEIFY_THRESHOLD`
//!   being 8 reads like "treeify at eight" and does not mean that.
//! - **Below `MIN_TREEIFY_CAPACITY` it resizes instead**, immediately, from
//!   inside `treeifyBin` — and then `putVal` goes on to its own
//!   `size > threshold` test, so one insert can resize twice.
//! - **The tree is ordered by `hash` alone, here.** `treeify` compares
//!   `p.hash` against `h` and only falls through to
//!   `comparableClassFor`/`tieBreakOrder` when they are **equal**.
//!   `comparableClassFor` walks the interfaces the class *itself* declares
//!   and wants `Comparable<T>` with `T == c`; `BlockPos` declares none — it
//!   inherits `Comparable<Vec3i>` from `Vec3i`, whose type argument is
//!   `Vec3i`, not `BlockPos` — so it answers null and the fallback is
//!   `tieBreakOrder`, which ends in `System.identityHashCode`. That is
//!   allocation-dependent and reproducible by nothing, so a bin holding two
//!   equal spread hashes is **refused**, loudly.
//!
//!   Measured, before any of this was written: over the village window the
//!   old refusal fired 188 times and **not one** of those bins held a tie.
//!   (188 counts bins reaching *eight*, which is where the old code gave up
//!   — one entry before the real trigger — so it is an upper bound on the
//!   treeify events, not a count of them.) A second, independent measurement
//!   agrees: the same window now generates with **zero** refusals of any
//!   kind from this module.
//!
//!   The reason ties do not happen is worth writing down, because it is not
//!   luck and it does not generalise. A tree's positions sit inside a box
//!   perhaps 30 blocks on a side, and over a box that small
//!   `(y + z * 31) * 31 + x` is *injective*: `31y + x` spans fewer than 961
//!   values and `z` steps by exactly 961. Feed the same set positions from a
//!   ±128 box and ties are everywhere — which is not a guess either, it is
//!   what the first draft of the golden harness did, and
//!   `ci/hash-order-golden.sh`'s two-run check caught the JVM disagreeing
//!   with itself.
//!
//! What the tree does to iteration is small but it compounds:
//!
//! - `treeifyBin` rebuilds the bin as tree nodes **preserving the linked
//!   list**, `treeify` builds left/right/parent on top without touching
//!   `next`, and only the closing `moveRootToFront` reorders: the red-black
//!   root is unlinked and relinked at the head. So a freshly treeified bin
//!   iterates as `[root] + (the old order, minus root)`.
//! - `putTreeVal` then splices each later entry into the list **immediately
//!   after its tree parent**, not at the tail, and `moveRootToFront`s again.
//! - `split` walks the bin along `next` from the head — which is the
//!   post-`moveRootToFront` order — so a resize's output depends on the
//!   previous root. A half of six or fewer (`UNTREEIFY_THRESHOLD`)
//!   untreeifies to a plain list in the same order; a larger half
//!   re-treeifies, but **only if the other half is non-empty**. That last
//!   guard turns out not to be observable: if the other half is empty the
//!   list is unchanged, so re-treeifying it would rebuild the identical tree
//!   with the identical root, and the root is already at the head. It is an
//!   optimisation, and it is ported anyway because a mutation that removed
//!   it is the one mutation the golden gate below cannot catch — better to
//!   know that than to have it silently uncovered.
//! - `removeTreeNode` unlinks from the list, and untreeifies the whole bin
//!   when the tree got short (`root.right == null || root.left == null ||
//!   root.left.left == null`).
//!
//! So "is this bin a tree, and which element is its root" has to be carried
//! across resizes rather than computed once, which is why this is an arena of
//! linked nodes rather than a `Vec` per bucket.
//!
//! # What is deliberately not ported
//!
//! Nothing here ever *looks a key up through the tree*. `contains`, the
//! duplicate check in `insert` and the node lookup in `remove` walk the
//! bin's linked list instead, which holds exactly the same nodes and gives
//! exactly the same answer — `TreeNode.find` exists to make lookup
//! logarithmic, and its result is not order-visible. The tree is built and
//! rebalanced only because *where the root ends up* and *where a new node
//! splices in* are order-visible. Code that exists to be correct-but-unused
//! is code nobody checks.

use std::hash::Hash;

/// A key that can live in a Java hash table: it must be able to say what
/// `Object.hashCode()` would return for it.
pub trait JavaHash: Eq + Clone {
    /// The value `hashCode()` returns, as a Java `int`.
    fn java_hash_code(&self) -> i32;
}

/// `BlockPos`/`Vec3i.hashCode()` — `(y + z * 31) * 31 + x`, in wrapping
/// arithmetic because Java's `int` wraps and the coordinates are signed.
#[must_use]
pub fn vec3i_hash(x: i32, y: i32, z: i32) -> i32 {
    y.wrapping_add(z.wrapping_mul(31))
        .wrapping_mul(31)
        .wrapping_add(x)
}

impl JavaHash for (i32, i32, i32) {
    fn java_hash_code(&self) -> i32 {
        vec3i_hash(self.0, self.1, self.2)
    }
}

/// `java.lang.String.hashCode()` — `s[0]*31^(n-1) + s[1]*31^(n-2) + ... +
/// s[n-1]`, over UTF-16 code units, in wrapping `int` arithmetic.
///
/// **Specified in the Java Language Specification**, unlike almost everything
/// else this module reproduces. `HashMap`'s spread and bucket index are
/// implementation details of one JDK; a `String`'s hash code is part of the
/// language and cannot change without breaking serialised data everywhere, so
/// this half is guaranteed rather than observed.
///
/// UTF-16 code units, not Rust `char`s: a key outside the basic multilingual
/// plane hashes as its surrogate pair. Every NBT key vanilla emits is ASCII,
/// so nothing measured today can tell the two apart — which is exactly why it
/// is written the correct way here rather than the convenient way.
#[must_use]
pub fn string_hash(s: &str) -> i32 {
    s.encode_utf16()
        .fold(0i32, |h, u| h.wrapping_mul(31).wrapping_add(i32::from(u)))
}

impl JavaHash for String {
    fn java_hash_code(&self) -> i32 {
        string_hash(self)
    }
}

/// The table refused to answer, because reproducing it would need a feature
/// this port has not implemented.
#[derive(Debug, PartialEq, Eq)]
pub struct Untracked(pub String);

/// `HashMap.DEFAULT_INITIAL_CAPACITY`.
const DEFAULT_CAPACITY: usize = 16;
/// `HashMap.DEFAULT_LOAD_FACTOR`.
const LOAD_FACTOR: f32 = 0.75;
/// `HashMap.TREEIFY_THRESHOLD`. Note the trigger: `putVal` treeifies when
/// `binCount >= TREEIFY_THRESHOLD - 1` and `binCount` does not count the
/// head, so the bin is **nine** entries long when it happens.
const TREEIFY_THRESHOLD: usize = 8;
/// `HashMap.UNTREEIFY_THRESHOLD` — a split half this size or smaller goes
/// back to a plain list.
const UNTREEIFY_THRESHOLD: usize = 6;
/// `HashMap.MIN_TREEIFY_CAPACITY` — below this the table resizes instead of
/// treeifying, so a long chain in a small table is not a problem.
const MIN_TREEIFY_CAPACITY: usize = 64;

/// One entry. `next` is the iteration link every bin has; the rest are
/// `TreeNode`'s fields and are only meaningful while the bin is a tree.
#[derive(Clone, Debug)]
struct Node<K> {
    hash: i32,
    key: K,
    next: Option<u32>,
    prev: Option<u32>,
    parent: Option<u32>,
    left: Option<u32>,
    right: Option<u32>,
    red: bool,
}

/// One bucket: its list head, and whether that head is a `TreeNode` — which
/// is how `HashMap` itself asks the question (`instanceof TreeNode`).
#[derive(Clone, Copy, Debug)]
struct Bin {
    head: Option<u32>,
    tree: bool,
}

impl Bin {
    const EMPTY: Self = Self {
        head: None,
        tree: false,
    };
}

/// A set whose iteration order is `java.util.HashSet`'s.
#[derive(Clone, Debug)]
pub struct JavaHashSet<K: JavaHash> {
    /// Every node ever allocated, addressed by index. Java has object
    /// references and this port has a `u32` into here; nothing else differs,
    /// and node *identity* is never observable because the one place Java
    /// uses it — `tieBreakOrder`'s `System.identityHashCode` — is refused.
    nodes: Vec<Node<K>>,
    /// Indices freed by `remove`, reused by the next insert. Invisible: a
    /// reused slot is a fresh node, exactly as `new Node(...)` is.
    free: Vec<u32>,
    table: Vec<Bin>,
    size: usize,
    threshold: usize,
    /// How many times a bucket has been turned into a tree. Behaviour reads
    /// this never; the golden gate reads it to check that its vectors still
    /// *reach* the tree path, because a gate that stopped exercising the
    /// thing it gates would keep passing and say nothing.
    treeifications: u32,
}

impl<K: JavaHash> Default for JavaHashSet<K> {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Field access. `self.nodes[i].left = x` does not borrow-check inside the
// algorithms below, which read one node while writing another, so every field
// goes through a pair of one-line accessors.
// ---------------------------------------------------------------------------
impl<K: JavaHash> JavaHashSet<K> {
    #[inline]
    fn at(&self, i: u32) -> &Node<K> {
        &self.nodes[i as usize]
    }
    #[inline]
    fn at_mut(&mut self, i: u32) -> &mut Node<K> {
        &mut self.nodes[i as usize]
    }
    #[inline]
    fn hash_of(&self, i: u32) -> i32 {
        self.at(i).hash
    }
    #[inline]
    fn next(&self, i: u32) -> Option<u32> {
        self.at(i).next
    }
    #[inline]
    fn set_next(&mut self, i: u32, v: Option<u32>) {
        self.at_mut(i).next = v;
    }
    #[inline]
    fn prev(&self, i: u32) -> Option<u32> {
        self.at(i).prev
    }
    #[inline]
    fn set_prev(&mut self, i: u32, v: Option<u32>) {
        self.at_mut(i).prev = v;
    }
    #[inline]
    fn parent(&self, i: u32) -> Option<u32> {
        self.at(i).parent
    }
    #[inline]
    fn set_parent(&mut self, i: u32, v: Option<u32>) {
        self.at_mut(i).parent = v;
    }
    #[inline]
    fn left(&self, i: u32) -> Option<u32> {
        self.at(i).left
    }
    #[inline]
    fn set_left(&mut self, i: u32, v: Option<u32>) {
        self.at_mut(i).left = v;
    }
    #[inline]
    fn right(&self, i: u32) -> Option<u32> {
        self.at(i).right
    }
    #[inline]
    fn set_right(&mut self, i: u32, v: Option<u32>) {
        self.at_mut(i).right = v;
    }
    #[inline]
    fn red(&self, i: u32) -> bool {
        self.at(i).red
    }
    #[inline]
    fn set_red(&mut self, i: u32, v: bool) {
        self.at_mut(i).red = v;
    }
    /// `x != null && x.red` — the shape of nearly every colour test in
    /// `balanceInsertion` and `balanceDeletion`, where a null child counts
    /// black.
    #[inline]
    fn is_red(&self, i: Option<u32>) -> bool {
        match i {
            Some(i) => self.red(i),
            None => false,
        }
    }
}

impl<K: JavaHash> JavaHashSet<K> {
    /// `new HashSet<>()` — sixteen buckets, resized at twelve entries.
    ///
    /// Vanilla's `Sets.newHashSet()` is exactly this; the sized variants are
    /// not used by the tree feature and are not offered here, because the
    /// initial capacity changes the whole order and guessing it would be
    /// silently wrong.
    #[must_use]
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            free: Vec::new(),
            table: vec![Bin::EMPTY; DEFAULT_CAPACITY],
            size: 0,
            treeifications: 0,
            #[expect(
                clippy::cast_precision_loss,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "`(int)(cap * loadFactor)` is what HashMap computes, \
                          in float, and the capacities are powers of two well \
                          inside f32's exact range"
            )]
            threshold: (DEFAULT_CAPACITY as f32 * LOAD_FACTOR) as usize,
        }
    }

    /// `HashMap.hash(key)` — `h ^ (h >>> 16)`, the *unsigned* shift.
    fn spread(key: &K) -> i32 {
        let h = key.java_hash_code();
        #[expect(
            clippy::cast_possible_wrap,
            reason = "the round trip through u32 is how `>>>` is spelled; the \
                      result is a Java int again"
        )]
        let spread = h ^ ((h as u32) >> 16) as i32;
        spread
    }

    /// The bucket a hash lands in: `hash & (table.length - 1)`.
    fn bucket_of(hash: i32, capacity: usize) -> usize {
        #[expect(
            clippy::cast_sign_loss,
            reason = "the mask makes the result positive and smaller than the \
                      capacity, whatever the sign of the hash"
        )]
        let index = (hash as u32 as usize) & (capacity - 1);
        index
    }

    /// A fresh node, from the free list if one is waiting.
    fn alloc(&mut self, hash: i32, key: K) -> u32 {
        let node = Node {
            hash,
            key,
            next: None,
            prev: None,
            parent: None,
            left: None,
            right: None,
            red: false,
        };
        if let Some(i) = self.free.pop() {
            self.nodes[i as usize] = node;
            return i;
        }
        let i = u32::try_from(self.nodes.len())
            .expect("a JavaHashSet with more than 4 billion entries");
        self.nodes.push(node);
        i
    }

    /// The node in this bin holding `key`, by walking the list.
    ///
    /// A tree bin would use `TreeNode.find` in Java; the two see the same
    /// nodes and the answer is not order-visible, so this is the whole
    /// lookup path. See the module note on what is deliberately not ported.
    fn find_in_bin(&self, head: u32, hash: i32, key: &K) -> Option<u32> {
        let mut e = Some(head);
        while let Some(i) = e {
            if self.hash_of(i) == hash && self.at(i).key == *key {
                return Some(i);
            }
            e = self.next(i);
        }
        None
    }

    /// Whether the bin, plus one incoming hash, holds two equal spread
    /// hashes — the case whose tree order `System.identityHashCode` decides.
    ///
    /// Checked *before* anything is mutated, so a refusal leaves the set
    /// exactly as it was. `extra` is the hash about to be inserted.
    fn has_hash_tie(&self, head: u32, extra: i32) -> bool {
        // Pairwise along the list rather than sort-and-scan: a bin that is
        // about to treeify holds nine entries, and this runs on every insert
        // into one, so it must not allocate.
        let mut a = Some(head);
        while let Some(i) = a {
            let h = self.hash_of(i);
            if h == extra {
                return true;
            }
            let mut b = self.next(i);
            while let Some(j) = b {
                if self.hash_of(j) == h {
                    return true;
                }
                b = self.next(j);
            }
            a = self.next(i);
        }
        false
    }

    fn tie_refusal(&self) -> Untracked {
        Untracked(format!(
            "a bucket reached {} entries in a table of {} with a SPREAD-HASH \
             TIE — java.util.HashMap would order the tree by \
             System.identityHashCode, which nothing can reproduce",
            TREEIFY_THRESHOLD + 1,
            self.table.len()
        ))
    }

    /// `HashSet.add`. Returns whether the key was new.
    ///
    /// # Errors
    /// If a bin that is (or is becoming) a red-black tree holds two equal
    /// spread hashes, because `java.util.HashMap` breaks that tie with
    /// `System.identityHashCode`. Measured never to happen on the windows
    /// this project generates; kept because the alternative is a plausible
    /// tree in the wrong place instead of a counted absence.
    pub fn insert(&mut self, key: K) -> Result<bool, Untracked> {
        let hash = Self::spread(&key);
        let capacity = self.table.len();
        let index = Self::bucket_of(hash, capacity);
        let bin = self.table[index];

        match bin.head {
            // `if ((p = tab[i = (n - 1) & hash]) == null) tab[i] = newNode(...)`.
            None => {
                let x = self.alloc(hash, key);
                self.table[index] = Bin {
                    head: Some(x),
                    tree: false,
                };
            }
            Some(head) if bin.tree => {
                // `putTreeVal` returns the existing node if the key is there,
                // and changes nothing on that path.
                if self.find_in_bin(head, hash, &key).is_some() {
                    return Ok(false);
                }
                if self.has_hash_tie(head, hash) {
                    return Err(self.tie_refusal());
                }
                self.put_tree_val(index, hash, key);
            }
            Some(head) => {
                // The chain walk, with `putVal`'s `binCount`: it starts at
                // zero on the head, so at the tail it is (length - 1).
                let mut bin_count = 0usize;
                let mut p = head;
                loop {
                    if self.hash_of(p) == hash && self.at(p).key == key {
                        return Ok(false);
                    }
                    match self.next(p) {
                        Some(e) => {
                            p = e;
                            bin_count += 1;
                        }
                        None => break,
                    }
                }
                // `if (binCount >= TREEIFY_THRESHOLD - 1) treeifyBin(tab, hash)`.
                let treeifies = bin_count >= TREEIFY_THRESHOLD - 1;
                if treeifies && capacity >= MIN_TREEIFY_CAPACITY && self.has_hash_tie(head, hash) {
                    return Err(self.tie_refusal());
                }
                let x = self.alloc(hash, key);
                self.set_next(p, Some(x));
                if treeifies {
                    self.treeify_bin(hash);
                }
            }
        }

        // `if (++size > threshold) resize()`, after `treeifyBin` has had its
        // own chance to resize.
        self.size += 1;
        if self.size > self.threshold {
            self.resize();
        }
        Ok(true)
    }

    /// `HashMap.treeifyBin` — turn one bucket into a tree, or resize instead
    /// if the table is too small for that to be worthwhile.
    fn treeify_bin(&mut self, hash: i32) {
        if self.table.len() < MIN_TREEIFY_CAPACITY {
            self.resize();
            return;
        }
        let index = Self::bucket_of(hash, self.table.len());
        let Some(head) = self.table[index].head else {
            return;
        };
        // Java replaces each `Node` with a `TreeNode` carrying the same hash
        // and key, in the same order, linking `prev` as it goes. Here the
        // nodes stay put and only `prev` has to be laid down.
        let mut prev: Option<u32> = None;
        let mut e = Some(head);
        while let Some(i) = e {
            self.set_prev(i, prev);
            prev = Some(i);
            e = self.next(i);
        }
        self.table[index].tree = true;
        self.treeifications += 1;
        self.treeify(index);
    }

    /// `TreeNode.treeify` — build the red-black tree over a bin's list, then
    /// `moveRootToFront`.
    ///
    /// The list is walked from the head and each node inserted by hash;
    /// `next` is never touched, so the only order change is the closing
    /// `moveRootToFront`.
    fn treeify(&mut self, index: usize) {
        let mut root: Option<u32> = None;
        let mut x = self.table[index].head;
        while let Some(xi) = x {
            let next = self.next(xi);
            self.set_left(xi, None);
            self.set_right(xi, None);
            if root.is_none() {
                self.set_parent(xi, None);
                self.set_red(xi, false);
                root = Some(xi);
            } else {
                let h = self.hash_of(xi);
                let mut p = root.expect("root is set on the first node");
                loop {
                    let ph = self.hash_of(p);
                    // `if (ph > h) dir = -1; else if (ph < h) dir = 1; else
                    // <comparable/tieBreakOrder>` — and the third case cannot
                    // arise here: `insert` refuses a bin with two equal
                    // hashes before it can become a tree, `split` only ever
                    // hands `treeify` a subset of one, and `put_tree_val`
                    // refuses the same way. If this fires the invariant is
                    // broken and the answer would be a fabricated order.
                    assert!(
                        ph != h,
                        "two equal spread hashes inside a tree bin: \
                         java.util.HashMap would break the tie with \
                         System.identityHashCode. insert() refuses this case \
                         before the bin treeifies, so reaching it is a bug in \
                         java_hash_set, not a limit of it"
                    );
                    let left = ph > h;
                    let xp = p;
                    let child = if left { self.left(p) } else { self.right(p) };
                    match child {
                        Some(c) => p = c,
                        None => {
                            self.set_parent(xi, Some(xp));
                            if left {
                                self.set_left(xp, Some(xi));
                            } else {
                                self.set_right(xp, Some(xi));
                            }
                            root = self.balance_insertion(root, xi);
                            break;
                        }
                    }
                }
            }
            x = next;
        }
        self.move_root_to_front(root);
    }

    /// `TreeNode.untreeify` — back to a plain list, in the order the list is
    /// already in. Java allocates fresh `Node`s, which is how the tree
    /// fields stop existing; here they are cleared.
    fn untreeify(&mut self, index: usize) {
        self.table[index].tree = false;
        let mut e = self.table[index].head;
        while let Some(i) = e {
            let node = self.at_mut(i);
            node.prev = None;
            node.parent = None;
            node.left = None;
            node.right = None;
            node.red = false;
            e = self.next(i);
        }
    }

    /// `TreeNode.moveRootToFront` — unlink the red-black root and relink it
    /// at the head of its bin. This is the only place a tree reorders
    /// iteration, and it moves exactly one element.
    fn move_root_to_front(&mut self, root: Option<u32>) {
        let Some(root) = root else {
            return;
        };
        let n = self.table.len();
        if n == 0 {
            return;
        }
        let index = Self::bucket_of(self.hash_of(root), n);
        let first = self.table[index].head;
        if first == Some(root) {
            return;
        }
        self.table[index].head = Some(root);
        let rp = self.prev(root);
        let rn = self.next(root);
        if let Some(rn) = rn {
            self.set_prev(rn, rp);
        }
        if let Some(rp) = rp {
            self.set_next(rp, rn);
        }
        if let Some(first) = first {
            self.set_prev(first, Some(root));
        }
        self.set_next(root, first);
        self.set_prev(root, None);
    }

    /// `TreeNode.putTreeVal`, for a key already known to be absent.
    ///
    /// The descent is by hash, and where it lands decides two things: the new
    /// node is spliced into the list **immediately after its tree parent**
    /// (not appended), and the rebalance may hand a different root to
    /// `moveRootToFront`.
    fn put_tree_val(&mut self, index: usize, hash: i32, key: K) {
        let root = self
            .table
            .get(index)
            .and_then(|b| b.head)
            .expect("a tree bin has a head");
        debug_assert!(
            self.parent(root).is_none(),
            "the head of a tree bin is its root — moveRootToFront guarantees it"
        );
        let mut p = root;
        loop {
            let ph = self.hash_of(p);
            assert!(
                ph != hash,
                "two equal spread hashes inside a tree bin — refused in insert()"
            );
            let left = ph > hash;
            let xp = p;
            let child = if left { self.left(p) } else { self.right(p) };
            match child {
                Some(c) => p = c,
                None => {
                    let xpn = self.next(xp);
                    let x = self.alloc(hash, key);
                    self.set_next(x, xpn);
                    if left {
                        self.set_left(xp, Some(x));
                    } else {
                        self.set_right(xp, Some(x));
                    }
                    self.set_next(xp, Some(x));
                    self.set_parent(x, Some(xp));
                    self.set_prev(x, Some(xp));
                    if let Some(xpn) = xpn {
                        self.set_prev(xpn, Some(x));
                    }
                    let r = self.balance_insertion(Some(root), x);
                    self.move_root_to_front(r);
                    return;
                }
            }
        }
    }

    /// `HashMap.resize` — double the table; each old bucket either moves
    /// whole, splits in two preserving relative order in both halves, or
    /// goes through `TreeNode.split`.
    fn resize(&mut self) {
        let old_capacity = self.table.len();
        let new_capacity = old_capacity * 2;
        #[expect(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "see `new`"
        )]
        let threshold = (new_capacity as f32 * LOAD_FACTOR) as usize;
        self.threshold = threshold;
        let old = std::mem::replace(&mut self.table, vec![Bin::EMPTY; new_capacity]);

        for (j, bin) in old.into_iter().enumerate() {
            let Some(head) = bin.head else {
                continue;
            };
            if self.next(head).is_none() {
                // `if (e.next == null) newTab[e.hash & (newCap - 1)] = e;`.
                // A one-node bin keeps whatever it was — Java asks
                // `instanceof TreeNode`, and moving the reference does not
                // change the answer.
                let target = Self::bucket_of(self.hash_of(head), new_capacity);
                self.table[target] = Bin {
                    head: Some(head),
                    tree: bin.tree,
                };
            } else if bin.tree {
                self.split(head, j, old_capacity);
            } else {
                // The lo/hi walk: `(hash & oldCap) == 0` stays at j, the rest
                // move to j + oldCap, both in walk order.
                let (mut lo_head, mut lo_tail) = (None, None);
                let (mut hi_head, mut hi_tail) = (None, None);
                let mut e = Some(head);
                while let Some(i) = e {
                    let next = self.next(i);
                    self.set_next(i, None);
                    if self.stays_put(i, old_capacity) {
                        match lo_tail {
                            None => lo_head = Some(i),
                            Some(t) => self.set_next(t, Some(i)),
                        }
                        lo_tail = Some(i);
                    } else {
                        match hi_tail {
                            None => hi_head = Some(i),
                            Some(t) => self.set_next(t, Some(i)),
                        }
                        hi_tail = Some(i);
                    }
                    e = next;
                }
                self.table[j] = Bin {
                    head: lo_head,
                    tree: false,
                };
                self.table[j + old_capacity] = Bin {
                    head: hi_head,
                    tree: false,
                };
            }
        }
    }

    /// `(e.hash & bit) == 0` — the one bit the doubled table added.
    #[inline]
    fn stays_put(&self, i: u32, bit: usize) -> bool {
        #[expect(
            clippy::cast_sign_loss,
            reason = "the round trip through u32 is how the sign is dropped, \
                      and only the one bit matters"
        )]
        let stays = (self.hash_of(i) as u32 as usize) & bit == 0;
        stays
    }

    /// `TreeNode.split` — a tree bin across a resize.
    ///
    /// Walks the bin along `next` **from the head**, which after a treeify is
    /// the post-`moveRootToFront` order rather than insertion order, so this
    /// carries the previous root's effect forward. A half of six or fewer
    /// untreeifies; a larger half is re-treeified, but only when the other
    /// half is non-empty — if everything landed on one side the bin is left
    /// exactly as it was, tree and all.
    fn split(&mut self, head: u32, index: usize, bit: usize) {
        let (mut lo_head, mut lo_tail) = (None, None);
        let (mut hi_head, mut hi_tail) = (None, None);
        let (mut lc, mut hc) = (0usize, 0usize);
        let mut e = Some(head);
        while let Some(i) = e {
            let next = self.next(i);
            self.set_next(i, None);
            if self.stays_put(i, bit) {
                self.set_prev(i, lo_tail);
                match lo_tail {
                    None => lo_head = Some(i),
                    Some(t) => self.set_next(t, Some(i)),
                }
                lo_tail = Some(i);
                lc += 1;
            } else {
                self.set_prev(i, hi_tail);
                match hi_tail {
                    None => hi_head = Some(i),
                    Some(t) => self.set_next(t, Some(i)),
                }
                hi_tail = Some(i);
                hc += 1;
            }
            e = next;
        }

        if let Some(lo) = lo_head {
            self.table[index] = Bin {
                head: Some(lo),
                tree: true,
            };
            if lc <= UNTREEIFY_THRESHOLD {
                self.untreeify(index);
            } else if hi_head.is_some() {
                self.treeify(index);
            }
        }
        if let Some(hi) = hi_head {
            let target = index + bit;
            self.table[target] = Bin {
                head: Some(hi),
                tree: true,
            };
            if hc <= UNTREEIFY_THRESHOLD {
                self.untreeify(target);
            } else if lo_head.is_some() {
                self.treeify(target);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Red-black machinery. `rotateLeft`/`rotateRight` and
    // `balanceInsertion`/`balanceDeletion` are exact mirrors of each other in
    // the JDK — two textually separate branches per method. They are written
    // once here with the side as a flag; the mapping is spelled out at each
    // use so the correspondence is checkable line by line.
    // -----------------------------------------------------------------------

    /// `TreeNode.rotateLeft`.
    fn rotate_left(&mut self, mut root: Option<u32>, p: u32) -> Option<u32> {
        let Some(r) = self.right(p) else {
            return root;
        };
        let rl = self.left(r);
        self.set_right(p, rl);
        if let Some(rl) = rl {
            self.set_parent(rl, Some(p));
        }
        let pp = self.parent(p);
        self.set_parent(r, pp);
        match pp {
            None => {
                self.set_red(r, false);
                root = Some(r);
            }
            Some(pp) => {
                if self.left(pp) == Some(p) {
                    self.set_left(pp, Some(r));
                } else {
                    self.set_right(pp, Some(r));
                }
            }
        }
        self.set_left(r, Some(p));
        self.set_parent(p, Some(r));
        root
    }

    /// `TreeNode.rotateRight`.
    fn rotate_right(&mut self, mut root: Option<u32>, p: u32) -> Option<u32> {
        let Some(l) = self.left(p) else {
            return root;
        };
        let lr = self.right(l);
        self.set_left(p, lr);
        if let Some(lr) = lr {
            self.set_parent(lr, Some(p));
        }
        let pp = self.parent(p);
        self.set_parent(l, pp);
        match pp {
            None => {
                self.set_red(l, false);
                root = Some(l);
            }
            Some(pp) => {
                if self.right(pp) == Some(p) {
                    self.set_right(pp, Some(l));
                } else {
                    self.set_left(pp, Some(l));
                }
            }
        }
        self.set_right(l, Some(p));
        self.set_parent(p, Some(l));
        root
    }

    /// Rotate `p` "towards x's side": left when x hangs off the left of its
    /// grandparent, right when it hangs off the right. The JDK writes the two
    /// out separately; they differ only in which rotation is called.
    fn rotate_toward(&mut self, root: Option<u32>, p: u32, left_side: bool) -> Option<u32> {
        if left_side {
            self.rotate_left(root, p)
        } else {
            self.rotate_right(root, p)
        }
    }

    /// The mirror of [`Self::rotate_toward`].
    fn rotate_away(&mut self, root: Option<u32>, p: u32, left_side: bool) -> Option<u32> {
        if left_side {
            self.rotate_right(root, p)
        } else {
            self.rotate_left(root, p)
        }
    }

    /// `TreeNode.balanceInsertion`.
    fn balance_insertion(&mut self, mut root: Option<u32>, x_in: u32) -> Option<u32> {
        let mut x = x_in;
        self.set_red(x, true);
        loop {
            // `if ((xp = x.parent) == null) { x.red = false; return x; }`
            let Some(xp0) = self.parent(x) else {
                self.set_red(x, false);
                return Some(x);
            };
            // `else if (!xp.red || (xpp = xp.parent) == null) return root;`
            if !self.red(xp0) {
                return root;
            }
            let Some(xpp0) = self.parent(xp0) else {
                return root;
            };
            // `left_side` is Java's `xp == (xppl = xpp.left)`: the first of
            // the two mirrored blocks.
            let xppl = self.left(xpp0);
            let left_side = xppl == Some(xp0);
            // The uncle: `xpp.right` on the left side, `xppl` on the other.
            let uncle = if left_side { self.right(xpp0) } else { xppl };
            if self.is_red(uncle) {
                let u = uncle.expect("is_red is false for None");
                self.set_red(u, false);
                self.set_red(xp0, false);
                self.set_red(xpp0, true);
                x = xpp0;
                continue;
            }
            let mut xp = Some(xp0);
            let mut xpp = Some(xpp0);
            // `if (x == xp.right)` on the left side, `x == xp.left` on the
            // other: x is the "inner" grandchild and has to be rotated out.
            let inner = if left_side {
                self.right(xp0)
            } else {
                self.left(xp0)
            };
            if inner == Some(x) {
                x = xp0;
                root = self.rotate_toward(root, x, left_side);
                xp = self.parent(x);
                xpp = match xp {
                    Some(v) => self.parent(v),
                    None => None,
                };
            }
            if let Some(xpv) = xp {
                self.set_red(xpv, false);
                if let Some(xppv) = xpp {
                    self.set_red(xppv, true);
                    root = self.rotate_away(root, xppv, left_side);
                }
            }
        }
    }

    /// `TreeNode.balanceDeletion`.
    fn balance_deletion(&mut self, mut root: Option<u32>, x_in: Option<u32>) -> Option<u32> {
        let mut x = x_in;
        loop {
            let Some(xi) = x else {
                return root;
            };
            if Some(xi) == root {
                return root;
            }
            let Some(xp0) = self.parent(xi) else {
                self.set_red(xi, false);
                return Some(xi);
            };
            if self.red(xi) {
                self.set_red(xi, false);
                return root;
            }
            // `left_side` is Java's `(xpl = xp.left) == x`.
            let left_side = self.left(xp0) == Some(xi);
            let mut xp = Some(xp0);
            // The sibling: `xp.right` on the left side, `xp.left` otherwise.
            let mut sib = if left_side {
                self.right(xp0)
            } else {
                self.left(xp0)
            };
            if self.is_red(sib) {
                let s = sib.expect("is_red is false for None");
                self.set_red(s, false);
                self.set_red(xp0, true);
                root = self.rotate_toward(root, xp0, left_side);
                xp = self.parent(xi);
                sib = self.child_toward(xp, !left_side);
            }
            let Some(s) = sib else {
                x = xp;
                continue;
            };
            // "near" is the sibling's child on x's side, "far" the other one.
            // On the left side those are `xpr.left` and `xpr.right`; the JDK's
            // mirror block reads `xpl.right` and `xpl.left`.
            let near = self.child_toward(Some(s), left_side);
            let far = self.child_toward(Some(s), !left_side);
            if !self.is_red(far) && !self.is_red(near) {
                self.set_red(s, true);
                x = xp;
                continue;
            }
            let mut sib = Some(s);
            if !self.is_red(far) {
                if let Some(n) = near {
                    self.set_red(n, false);
                }
                self.set_red(s, true);
                root = self.rotate_away(root, s, left_side);
                xp = self.parent(xi);
                sib = self.child_toward(xp, !left_side);
            }
            if let Some(sv) = sib {
                // `xpr.red = (xp == null) ? false : xp.red`
                let inherited = match xp {
                    Some(p) => self.red(p),
                    None => false,
                };
                self.set_red(sv, inherited);
                if let Some(f) = self.child_toward(Some(sv), !left_side) {
                    self.set_red(f, false);
                }
            }
            if let Some(pv) = xp {
                self.set_red(pv, false);
                root = self.rotate_toward(root, pv, left_side);
            }
            x = root;
        }
    }

    /// `p.left` when `left_side`, `p.right` otherwise — the accessor the two
    /// mirrored halves of `balanceDeletion` differ by.
    #[inline]
    fn child_toward(&self, p: Option<u32>, left_side: bool) -> Option<u32> {
        let p = p?;
        if left_side {
            self.left(p)
        } else {
            self.right(p)
        }
    }

    /// `TreeNode.root()` — climb to the top.
    fn root_of(&self, mut i: u32) -> u32 {
        while let Some(p) = self.parent(i) {
            i = p;
        }
        i
    }

    /// `TreeNode.removeTreeNode(map, tab, movable = true)`.
    ///
    /// `movable` is always true here: `HashMap.remove` reaches this through
    /// `removeNode(hash, key, null, false, true)`.
    fn remove_tree_node(&mut self, this: u32) {
        let n = self.table.len();
        if n == 0 {
            return;
        }
        let index = Self::bucket_of(self.hash_of(this), n);
        // Java's `first` is reassigned when the head is the node going away;
        // `root` keeps the *original* head, which in that case is this very
        // node. That is load-bearing below, so it is written the same way.
        let mut first = self.table[index].head;
        let root_start = first;
        let succ = self.next(this);
        let pred = self.prev(this);
        match pred {
            None => {
                first = succ;
                self.table[index].head = succ;
            }
            Some(pr) => self.set_next(pr, succ),
        }
        if let Some(s) = succ {
            self.set_prev(s, pred);
        }
        let Some(first) = first else {
            // The bin is empty. Java leaves `tab[index] == null`, which
            // carries no "is a tree" answer at all.
            self.table[index].tree = false;
            return;
        };
        let mut root = root_start.expect("the bin had a head");
        if self.parent(root).is_some() {
            root = self.root_of(root);
        }
        // `root.right == null || (rl = root.left) == null || rl.left == null`
        // — a tree this short is not worth keeping.
        let rl = self.left(root);
        let short =
            self.right(root).is_none() || rl.is_none() || rl.and_then(|rl| self.left(rl)).is_none();
        if short {
            debug_assert_eq!(self.table[index].head, Some(first));
            self.untreeify(index);
            return;
        }

        let p = this;
        let pl = self.left(p);
        let pr = self.right(p);
        let replacement;
        if let (Some(plv), Some(prv)) = (pl, pr) {
            // Two children: swap with the in-order successor, colours and all.
            let mut s = prv;
            while let Some(sl) = self.left(s) {
                s = sl;
            }
            let c = self.red(s);
            let p_red = self.red(p);
            self.set_red(s, p_red);
            self.set_red(p, c);
            let sr = self.right(s);
            let pp = self.parent(p);
            if s == prv {
                self.set_parent(p, Some(s));
                self.set_right(s, Some(p));
            } else {
                let sp = self.parent(s);
                self.set_parent(p, sp);
                if let Some(sp) = sp {
                    if self.left(sp) == Some(s) {
                        self.set_left(sp, Some(p));
                    } else {
                        self.set_right(sp, Some(p));
                    }
                }
                self.set_right(s, Some(prv));
                self.set_parent(prv, Some(s));
            }
            self.set_left(p, None);
            self.set_right(p, sr);
            if let Some(sr) = sr {
                self.set_parent(sr, Some(p));
            }
            self.set_left(s, Some(plv));
            self.set_parent(plv, Some(s));
            self.set_parent(s, pp);
            match pp {
                None => root = s,
                Some(pp) => {
                    if self.left(pp) == Some(p) {
                        self.set_left(pp, Some(s));
                    } else {
                        self.set_right(pp, Some(s));
                    }
                }
            }
            replacement = sr.unwrap_or(p);
        } else if let Some(plv) = pl {
            replacement = plv;
        } else if let Some(prv) = pr {
            replacement = prv;
        } else {
            replacement = p;
        }

        if replacement != p {
            let pp = self.parent(p);
            self.set_parent(replacement, pp);
            match pp {
                None => {
                    root = replacement;
                    self.set_red(replacement, false);
                }
                Some(pp) => {
                    if self.left(pp) == Some(p) {
                        self.set_left(pp, Some(replacement));
                    } else {
                        self.set_right(pp, Some(replacement));
                    }
                }
            }
            self.set_left(p, None);
            self.set_right(p, None);
            self.set_parent(p, None);
        }

        let r = if self.red(p) {
            Some(root)
        } else {
            self.balance_deletion(Some(root), Some(replacement))
        };

        if replacement == p {
            // A leaf: detach it after the rebalance, not before.
            let pp = self.parent(p);
            self.set_parent(p, None);
            if let Some(pp) = pp {
                if self.left(pp) == Some(p) {
                    self.set_left(pp, None);
                } else if self.right(pp) == Some(p) {
                    self.set_right(pp, None);
                }
            }
        }
        self.move_root_to_front(r);
    }

    #[must_use]
    pub fn contains(&self, key: &K) -> bool {
        let hash = Self::spread(key);
        let index = Self::bucket_of(hash, self.table.len());
        match self.table[index].head {
            Some(head) => self.find_in_bin(head, hash, key).is_some(),
            None => false,
        }
    }

    /// `HashSet.remove`. Returns whether the key was there.
    ///
    /// Removal does **not** shrink the table — `java.util.HashMap` never
    /// shrinks — so the iteration order of what is left is exactly what it
    /// was, minus one entry, except in a tree bin where `removeTreeNode` may
    /// rebalance and move the root. The leaf-distance flood empties and
    /// refills its sets repeatedly and that stability is what makes its order
    /// defined.
    pub fn remove(&mut self, key: &K) -> bool {
        let hash = Self::spread(key);
        let index = Self::bucket_of(hash, self.table.len());
        let bin = self.table[index];
        let Some(head) = bin.head else {
            return false;
        };
        if bin.tree {
            let Some(node) = self.find_in_bin(head, hash, key) else {
                return false;
            };
            self.remove_tree_node(node);
            self.free.push(node);
            self.size -= 1;
            return true;
        }
        // A plain chain: unlink, and nothing else. `removeNode` does not
        // untreeify, resize or reorder.
        let mut before: Option<u32> = None;
        let mut p = head;
        loop {
            if self.hash_of(p) == hash && self.at(p).key == *key {
                let after = self.next(p);
                match before {
                    None => self.table[index].head = after,
                    Some(b) => self.set_next(b, after),
                }
                self.free.push(p);
                self.size -= 1;
                return true;
            }
            match self.next(p) {
                Some(e) => {
                    before = Some(p);
                    p = e;
                }
                None => return false,
            }
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.size
    }

    /// How many buckets this set has turned into red-black trees over its
    /// life — counted at `treeifyBin`, so a `split` that re-treeifies a half
    /// is not counted again. Diagnostic only.
    #[must_use]
    pub fn treeifications(&self) -> u32 {
        self.treeifications
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.size == 0
    }

    /// The keys in `java.util.HashSet` iteration order.
    ///
    /// `HashIterator` walks the bucket array from index 0 and each bucket
    /// along `next`, which is all this is — the tree fields are not consulted
    /// even when the bucket is a tree, because `next` is the list `treeify`
    /// and `putTreeVal` maintain.
    pub fn iter(&self) -> impl Iterator<Item = &K> + '_ {
        let mut index = 0usize;
        let mut next: Option<u32> = None;
        std::iter::from_fn(move || loop {
            if let Some(i) = next {
                let node = self.at(i);
                next = node.next;
                return Some(&node.key);
            }
            if index >= self.table.len() {
                return None;
            }
            next = self.table[index].head;
            index += 1;
        })
    }

    /// The keys, cloned, in iteration order.
    #[must_use]
    pub fn to_vec(&self) -> Vec<K> {
        self.iter().cloned().collect()
    }
}

impl<K: JavaHash + Hash> FromIterator<K> for JavaHashSet<K> {
    /// # Panics
    /// If a bin holds two equal spread hashes as it treeifies. Use
    /// [`JavaHashSet::insert`] where that has to be handled.
    fn from_iter<I: IntoIterator<Item = K>>(iter: I) -> Self {
        let mut out = Self::new();
        for key in iter {
            out.insert(key)
                .expect("no spread-hash tie in a treeified bin");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `String.hashCode`, at the two values the JLS pins by example and at
    /// one that overflows.
    #[test]
    fn the_string_hash_is_the_specified_polynomial() {
        assert_eq!(string_hash(""), 0);
        assert_eq!(string_hash("a"), 97);
        assert_eq!(string_hash("ab"), 97 * 31 + 98);
        // Long enough to wrap a signed int several times over, so the sign
        // is part of what is being asserted. Computed independently rather
        // than copied out of this function's own output.
        assert_eq!(string_hash("hover_event"), -225_721_897);
    }

    /// **The two NBT key orders measured off real vanilla's wire, predicted
    /// from the hash table rather than recorded from the packet.**
    ///
    /// `rustmc_world::nbt::Nbt::Compound` is a `BTreeMap` and writes keys
    /// sorted; vanilla's `CompoundTag` wraps a `java.util.HashMap` and
    /// `write` iterates `keySet()`. On 2026-08-04 the reply to `/list` was
    /// read straight out of the recorded bytes and carries **`with` before
    /// `translate`**, and a display name carries **`click_event`,
    /// `insertion`, `text`, `hover_event`** — neither alphabetical, and both
    /// stable across runs.
    ///
    /// This asserts that this module reproduces both without being told
    /// them. It matters because it settles a question that was open: the
    /// order is not "accidentally deterministic", it is a pure function of
    /// `String.hashCode` — which the JLS specifies — and the table capacity,
    /// which is the default 16 for a compound this small. Four keys in
    /// sixteen buckets with no collision means **insertion order does not
    /// enter at all**, so nothing has to know how the codec built the tag.
    ///
    /// The reproduction is checked here against vanilla's own bytes rather
    /// than against itself, which is the standing rule for this module.
    #[test]
    fn the_nbt_key_orders_vanilla_was_measured_writing_fall_out_of_the_table() {
        let order = |keys: &[&str]| -> Vec<String> {
            let mut set = JavaHashSet::new();
            for k in keys {
                set.insert((*k).to_string()).expect("no treeified bin");
            }
            set.iter().cloned().collect()
        };
        // The `/list` reply: `Component.translatable(key, args)` serialises
        // to a two-key compound and vanilla puts `with` first.
        assert_eq!(
            order(&["translate", "with"]),
            vec!["with".to_string(), "translate".to_string()]
        );
        // A display name's four keys, in the order the recording carries.
        assert_eq!(
            order(&["text", "click_event", "hover_event", "insertion"]),
            vec![
                "click_event".to_string(),
                "insertion".to_string(),
                "text".to_string(),
                "hover_event".to_string()
            ]
        );
        // And it is genuinely bucket order rather than insertion order:
        // the same keys inserted backwards come out the same way.
        assert_eq!(
            order(&["insertion", "hover_event", "click_event", "text"]),
            order(&["text", "click_event", "hover_event", "insertion"]),
            "no two of these four share a bucket, so insertion order cannot show"
        );
    }

    /// The hash Java computes for a `BlockPos`, at values where the
    /// multiplications overflow a signed 32-bit int.
    #[test]
    fn the_position_hash_wraps_the_way_javas_int_does() {
        assert_eq!(vec3i_hash(0, 0, 0), 0);
        assert_eq!(vec3i_hash(1, 2, 3), (2 + 3 * 31) * 31 + 1);
        assert_eq!(vec3i_hash(-1, -1, -1), (-1 + -31) * 31 + -1);
        // y = 2^26 overflows on the second multiply.
        assert_eq!(
            vec3i_hash(0, 1 << 26, 0),
            (1i32 << 26).wrapping_mul(31),
            "the product wraps rather than saturating"
        );
    }

    /// `h ^ (h >>> 16)` — the shift is unsigned, so a negative hash does not
    /// smear ones through the high half.
    #[test]
    fn the_spread_uses_an_unsigned_shift() {
        let key = (-1, -1, -1);
        let h = key.java_hash_code();
        assert!(h < 0, "this key hashes negative: {h}");
        let unsigned = h ^ ((h as u32) >> 16) as i32;
        let signed = h ^ (h >> 16);
        assert_ne!(unsigned, signed, "the two shifts must disagree here");
        assert_eq!(JavaHashSet::<(i32, i32, i32)>::spread(&key), unsigned);
    }

    /// Bucket-major, insertion-minor — neither insertion order nor sorted.
    #[test]
    fn iteration_is_bucket_major_and_insertion_minor() {
        // Two keys chosen to share a bucket in a sixteen-slot table, inserted
        // after a key that lands in a lower bucket.
        let mut set = JavaHashSet::new();
        let mut colliding = Vec::new();
        let mut probe = 0;
        while colliding.len() < 2 {
            let key = (probe, 0, 0);
            if JavaHashSet::<(i32, i32, i32)>::bucket_of(
                JavaHashSet::<(i32, i32, i32)>::spread(&key),
                16,
            ) == 5
            {
                colliding.push(key);
            }
            probe += 1;
        }
        let mut low = (0, 0, 0);
        for candidate in 0..10_000 {
            let key = (candidate, 1, 0);
            if JavaHashSet::<(i32, i32, i32)>::bucket_of(
                JavaHashSet::<(i32, i32, i32)>::spread(&key),
                16,
            ) == 1
            {
                low = key;
                break;
            }
        }

        set.insert(colliding[0]).expect("ok");
        set.insert(colliding[1]).expect("ok");
        set.insert(low).expect("ok");

        let order = set.to_vec();
        assert_eq!(
            order,
            vec![low, colliding[0], colliding[1]],
            "bucket 1 before bucket 5, and within bucket 5 the earlier insert first"
        );
    }

    /// A resize splits each bucket in two and keeps relative order in both.
    #[test]
    fn a_resize_preserves_relative_order_within_each_half() {
        let mut set = JavaHashSet::new();
        for i in 0..12 {
            set.insert((i, 0, 0)).expect("ok");
        }
        assert_eq!(
            set.table.len(),
            16,
            "twelve entries is still under the threshold"
        );
        set.insert((12, 0, 0)).expect("ok");
        assert_eq!(set.table.len(), 32, "the thirteenth doubles the table");
        assert_eq!(set.len(), 13);

        // Every entry is still findable, and the order is the table's.
        for i in 0..13 {
            assert!(set.contains(&(i, 0, 0)), "{i} went missing in the resize");
        }
        let order = set.to_vec();
        assert_eq!(order.len(), 13);
    }

    #[test]
    fn inserting_the_same_key_twice_is_not_two_entries() {
        let mut set = JavaHashSet::new();
        assert!(set.insert((4, 5, 6)).expect("ok"));
        assert!(!set.insert((4, 5, 6)).expect("ok"));
        assert_eq!(set.len(), 1);
    }

    fn bucket_at(key: &(i32, i32, i32), capacity: usize) -> usize {
        JavaHashSet::<(i32, i32, i32)>::bucket_of(
            JavaHashSet::<(i32, i32, i32)>::spread(key),
            capacity,
        )
    }

    /// Grow `set` to exactly 64 buckets, then return forty keys that all land
    /// in one bucket of it.
    ///
    /// The capacity is asserted rather than assumed: `treeifyBin` can resize
    /// on its own and `putVal` can resize again straight after, so "insert
    /// until the table is 64" is not by itself a guarantee that it stopped
    /// there — and every caller's arithmetic is against 64.
    fn colliding_keys(set: &mut JavaHashSet<(i32, i32, i32)>) -> Vec<(i32, i32, i32)> {
        let mut filler = 0;
        while set.table.len() < 64 {
            set.insert((filler, 7, 7)).expect("ok");
            filler += 1;
        }
        assert_eq!(set.table.len(), 64, "the filler overshot the capacity");
        assert!(
            set.table.iter().all(|b| !b.tree),
            "the filler treeified something and the test is no longer measuring what it says"
        );
        let target = bucket_at(&(0, 0, 0), 64);
        let mut out = Vec::new();
        let mut probe = 0;
        while out.len() < 40 {
            let key = (probe, 0, 0);
            probe += 1;
            if bucket_at(&key, 64) == target && !set.contains(&key) {
                out.push(key);
            }
        }
        out
    }

    /// Nine, not eight: `putVal`'s `binCount` does not count the head, so a
    /// bin of eight is still a plain list.
    #[test]
    fn a_bucket_treeifies_at_nine_entries_not_eight() {
        let mut set: JavaHashSet<(i32, i32, i32)> = JavaHashSet::new();
        let keys = colliding_keys(&mut set);
        let index = bucket_at(&keys[0], 64);
        for key in keys.iter().take(8) {
            set.insert(*key).expect("no tie");
        }
        assert!(
            !set.table[index].tree,
            "eight in a bucket is still a list — TREEIFY_THRESHOLD is not the bin size"
        );
        set.insert(keys[8]).expect("no tie");
        assert!(set.table[index].tree, "the ninth treeifies it");
        assert_eq!(set.table.len(), 64, "and does not resize at this capacity");
    }

    /// Under `MIN_TREEIFY_CAPACITY` the bin does not treeify — the table
    /// doubles instead, from inside `treeifyBin`.
    #[test]
    fn a_small_table_resizes_rather_than_treeifying() {
        let mut set: JavaHashSet<(i32, i32, i32)> = JavaHashSet::new();
        // Nine keys in one bucket of a sixteen-slot table: under the load
        // threshold of twelve, so only `treeifyBin` can grow it.
        let target = bucket_at(&(0, 0, 0), 16);
        let mut added = 0;
        let mut probe = 0;
        while added < 9 {
            let key = (probe, 0, 0);
            probe += 1;
            if bucket_at(&key, 16) == target {
                set.insert(key).expect("no tie");
                added += 1;
            }
        }
        assert_eq!(set.len(), 9);
        assert!(
            set.table.len() > 16,
            "treeifyBin resizes below MIN_TREEIFY_CAPACITY instead of treeifying"
        );
        assert!(
            set.table.iter().all(|b| !b.tree),
            "and nothing became a tree on the way"
        );
    }

    /// A treeified bin iterates as `[root] + (the old order, minus root)` —
    /// one element moves, and it is the same set of elements.
    #[test]
    fn treeifying_moves_exactly_one_element_to_the_front() {
        let mut set: JavaHashSet<(i32, i32, i32)> = JavaHashSet::new();
        let keys = colliding_keys(&mut set);
        for key in keys.iter().take(8) {
            set.insert(*key).expect("no tie");
        }
        let before = set.to_vec();
        set.insert(keys[8]).expect("no tie");
        let after = set.to_vec();

        assert_eq!(after.len(), before.len() + 1);
        let mut sorted_before = before.clone();
        let mut sorted_after = after.clone();
        sorted_before.push(keys[8]);
        sorted_before.sort_unstable();
        sorted_after.sort_unstable();
        assert_eq!(sorted_before, sorted_after, "same members, different order");

        // Everything outside the treeified bin kept its place.
        let index = bucket_at(&keys[0], 64);
        let in_bin: Vec<_> = keys.iter().take(9).copied().collect();
        let outside_before: Vec<_> = before.iter().filter(|k| !in_bin.contains(*k)).collect();
        let outside_after: Vec<_> = after.iter().filter(|k| !in_bin.contains(*k)).collect();
        assert_eq!(outside_before, outside_after, "bucket {index} only");

        // Nine entries in the bin, and the first of them is the root.
        let bin: Vec<_> = after
            .iter()
            .filter(|k| in_bin.contains(*k))
            .copied()
            .collect();
        assert_eq!(bin.len(), 9);
        let root = set.table[index].head.expect("a head");
        assert_eq!(
            set.at(root).key,
            bin[0],
            "moveRootToFront put the root first"
        );
    }

    /// A tree survives a resize: `split` rebuilds both halves and the set
    /// still holds every key.
    #[test]
    fn a_tree_bin_survives_a_resize() {
        let mut set: JavaHashSet<(i32, i32, i32)> = JavaHashSet::new();
        let keys = colliding_keys(&mut set);
        for key in &keys {
            set.insert(*key).expect("no tie");
        }
        assert!(set.table.len() > 64, "forty more entries resized past 64");
        for key in &keys {
            assert!(set.contains(key), "{key:?} went missing across the split");
        }
        assert_eq!(set.to_vec().len(), set.len());
    }

    /// Removing from a tree bin keeps the rest, and eventually untreeifies.
    #[test]
    fn removing_from_a_tree_bin_keeps_the_rest() {
        let mut set: JavaHashSet<(i32, i32, i32)> = JavaHashSet::new();
        let keys = colliding_keys(&mut set);
        for key in keys.iter().take(12) {
            set.insert(*key).expect("no tie");
        }
        let index = bucket_at(&keys[0], 64);
        assert!(set.table[index].tree);
        let expected = set.len();
        for (removed, key) in keys.iter().take(12).enumerate() {
            assert!(set.remove(key), "{key:?} should have been there");
            assert_eq!(set.len(), expected - removed - 1);
            assert_eq!(
                set.to_vec().len(),
                set.len(),
                "the list and the count agree"
            );
            for later in keys.iter().take(12).skip(removed + 1) {
                assert!(set.contains(later), "{later:?} was collateral damage");
            }
        }
        assert!(!set.table[index].tree, "an empty bin is not a tree");
    }

    /// A bin with two equal spread hashes is refused rather than guessed at,
    /// and the refusal leaves the set untouched.
    #[test]
    fn a_spread_hash_tie_in_a_treeifying_bin_is_refused() {
        // A key type whose hash we choose, so a tie can be built at all: two
        // distinct `BlockPos` with the same spread hash exist but are tedious
        // to find, and the point here is the refusal, not the search. Every
        // hash below is under 2^16, where `h >>> 16` is zero and the spread is
        // the identity — so `hash & (cap - 1)` is directly readable.
        #[derive(Clone, PartialEq, Eq, Debug)]
        struct Keyed(i32, i32);
        impl JavaHash for Keyed {
            fn java_hash_code(&self) -> i32 {
                self.0
            }
        }

        let mut set: JavaHashSet<Keyed> = JavaHashSet::new();
        // Thirty keys, seven apart, which reach 64 buckets and land one per
        // bucket there — and none in bucket 3, since 7i == 3 (mod 64) wants
        // i == 37.
        for i in 1..=30 {
            set.insert(Keyed(i * 7, i)).expect("distinct");
        }
        assert_eq!(set.table.len(), 64);
        // Eight into bucket 3, all with distinct hashes: still a plain list.
        for k in 1..=8 {
            set.insert(Keyed(3 + 64 * k, 1000 + k)).expect("distinct");
        }
        assert!(!set.table[3].tree, "eight is not yet a tree");

        let before = set.to_vec();
        // A ninth, which would treeify the bin — and whose hash is one the
        // bin already holds, so the tree's shape would be decided by
        // `System.identityHashCode`.
        let error = set
            .insert(Keyed(3 + 64, 9999))
            .expect_err("a tie must be refused");
        assert!(error.0.contains("SPREAD-HASH TIE"), "{}", error.0);
        assert_eq!(
            set.to_vec(),
            before,
            "a refusal must not have changed the set on the way to refusing"
        );
        assert_eq!(set.len(), 38, "nor the count");
    }
}
