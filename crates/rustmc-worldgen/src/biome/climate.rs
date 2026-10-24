//! Climate space — six noise axes, and the nearest-biome search over them.
//!
//! Reference: `net.minecraft.world.level.biome.Climate`.
//!
//! A biome is not chosen by a rule; it is chosen by **proximity**. Each biome
//! declares a box in 6-dimensional climate space (temperature, humidity,
//! continentalness, erosion, depth, weirdness) plus a 7th "offset" axis that
//! acts as a constant penalty, and a point in the world takes whichever box
//! is nearest. That is why biome borders are irregular and why adding one
//! biome can move several others.
//!
//! # Everything is a quantised integer
//!
//! Climate values are `float`s scaled by 10000 and truncated to `long`
//! ([`quantize_coord`]), so the whole search runs in exact integer
//! arithmetic. No epsilon, no float ordering — but also exact **ties**, which
//! matters below.
//!
//! # The search is order-dependent, and that is not a bug to fix
//!
//! Vanilla seeds each search with the previous result from a `ThreadLocal`,
//! and prunes with a strict `>`. So when two boxes are exactly equidistant
//! the tie breaks toward whatever the last query returned. A lookup is
//! therefore a function of the query *sequence*, not just the query.
//!
//! That is modelled here as an explicit `&mut self` field rather than hidden
//! in a thread local: it is real behaviour that a caller has to reproduce to
//! match vanilla, and burying it in ambient state is how it gets
//! accidentally "optimised" into a pure function that returns different
//! biomes.

/// The number of axes, including the offset. Vanilla asserts this is 7 and
/// throws otherwise, so it is a hard shape rather than a parameter.
pub const DIMENSIONS: usize = 7;

/// `Climate.quantizeCoord` — scale by 10000 and truncate.
///
/// The multiply is in `f32` and the cast truncates toward zero, so -0.00005
/// and +0.00005 both land on 0. Doing the multiply in `f64` would round
/// differently near box edges and move biome borders by a block.
#[must_use]
pub fn quantize_coord(coord: f32) -> i64 {
    (coord * 10000.0_f32) as i64
}

/// `Climate.unquantizeCoord`.
#[must_use]
pub fn unquantize_coord(coord: i64) -> f32 {
    coord as f32 / 10000.0_f32
}

/// `Climate.Parameter` — an inclusive interval on one axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parameter {
    pub min: i64,
    pub max: i64,
}

impl Parameter {
    #[must_use]
    pub const fn new(min: i64, max: i64) -> Self {
        Self { min, max }
    }

    /// A degenerate interval, used for the offset axis.
    #[must_use]
    pub const fn point(value: i64) -> Self {
        Self {
            min: value,
            max: value,
        }
    }

    /// From `float` bounds, as the codec does.
    #[must_use]
    pub fn from_floats(min: f32, max: f32) -> Self {
        Self {
            min: quantize_coord(min),
            max: quantize_coord(max),
        }
    }

    /// Distance from a point to this interval — 0 inside it.
    ///
    /// Note the asymmetry: `above` is returned when positive *without*
    /// clamping, but `below` goes through a `max(_, 0)`. The two branches are
    /// equivalent given the interval is well formed, and this is vanilla's
    /// shape.
    #[must_use]
    pub fn distance(self, target: i64) -> i64 {
        let above = target - self.max;
        let below = self.min - target;
        if above > 0 {
            above
        } else {
            below.max(0)
        }
    }

    /// Distance between two intervals — 0 if they overlap.
    #[must_use]
    pub fn distance_to(self, target: Self) -> i64 {
        let above = target.min - self.max;
        let below = self.min - target.max;
        if above > 0 {
            above
        } else {
            below.max(0)
        }
    }

    /// The smallest interval containing both.
    #[must_use]
    pub fn span(self, other: Option<Self>) -> Self {
        match other {
            None => self,
            Some(o) => Self {
                min: self.min.min(o.min),
                max: self.max.max(o.max),
            },
        }
    }
}

/// `Climate.ParameterPoint` — one biome's box in climate space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParameterPoint {
    pub temperature: Parameter,
    pub humidity: Parameter,
    pub continentalness: Parameter,
    pub erosion: Parameter,
    pub depth: Parameter,
    pub weirdness: Parameter,
    /// A constant cost added to every query, as a degenerate 7th axis. It is
    /// how vanilla makes a biome "less preferred" without shrinking its box.
    pub offset: i64,
}

impl ParameterPoint {
    #[must_use]
    pub const fn parameter_space(&self) -> [Parameter; DIMENSIONS] {
        [
            self.temperature,
            self.humidity,
            self.continentalness,
            self.erosion,
            self.depth,
            self.weirdness,
            Parameter::point(self.offset),
        ]
    }
}

/// `Climate.TargetPoint` — a sampled point in climate space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TargetPoint {
    pub temperature: i64,
    pub humidity: i64,
    pub continentalness: i64,
    pub erosion: i64,
    pub depth: i64,
    pub weirdness: i64,
}

impl TargetPoint {
    /// The 7th entry is 0, so a box's `offset` is measured against zero and
    /// acts as a flat penalty.
    #[must_use]
    pub const fn to_parameter_array(self) -> [i64; DIMENSIONS] {
        [
            self.temperature,
            self.humidity,
            self.continentalness,
            self.erosion,
            self.depth,
            self.weirdness,
            0,
        ]
    }
}

/// A node in the search tree. Leaves carry an index into the value list.
#[derive(Clone, Debug)]
enum Node {
    Leaf {
        space: [Parameter; DIMENSIONS],
        value: usize,
    },
    SubTree {
        space: [Parameter; DIMENSIONS],
        children: Vec<Node>,
    },
}

impl Node {
    const fn space(&self) -> &[Parameter; DIMENSIONS] {
        match self {
            Self::Leaf { space, .. } | Self::SubTree { space, .. } => space,
        }
    }

    /// `Node.distance` — squared Euclidean over the seven axes, in `i64`.
    ///
    /// Squared, so it is never rooted; comparisons only ever need the order.
    fn distance(&self, target: &[i64; DIMENSIONS]) -> i64 {
        let space = self.space();
        let mut total: i64 = 0;
        for i in 0..DIMENSIONS {
            let d = space[i].distance(target[i]);
            total += d * d;
        }
        total
    }

    /// The same distance, abandoned as soon as it cannot beat `bound`.
    ///
    /// Every use of a child's distance in [`Self::search`] is guarded by
    /// `bound > distance`, so a partial sum that has already reached `bound`
    /// settles the comparison: the total only grows, each term being a
    /// square. `None` means "not below the bound" and the caller skips the
    /// child — which is the same branch it would have taken with the exact
    /// number, and therefore the same tree walk, the same tie-breaks and the
    /// same biome.
    ///
    /// This is worth a function because the search prunes hard: most children
    /// are rejected, and rejecting one on two axes instead of seven is most
    /// of the arithmetic. It is not an approximation — the value returned in
    /// the `Some` case is exact.
    fn distance_below(&self, target: &[i64; DIMENSIONS], bound: i64) -> Option<i64> {
        let space = self.space();
        let mut total: i64 = 0;
        for i in 0..DIMENSIONS {
            let d = space[i].distance(target[i]);
            total += d * d;
            if total >= bound {
                return None;
            }
        }
        Some(total)
    }

    /// `SubTree.search` / `Leaf.search`.
    ///
    /// The `candidate` is both the running best *and* the pruning bound, and
    /// the comparison is a strict `>`, so an exactly-equidistant box never
    /// displaces the incoming candidate. That is the tie-break that makes the
    /// whole search order-dependent.
    ///
    /// Returns a **reference**. Vanilla's `RTree` keeps a reference in its
    /// ThreadLocal and this returned owned nodes, cloning a leaf at every
    /// level of every query — the biome lookup is the surface stage's hottest
    /// call and nothing about the search needs to own anything.
    fn search<'a>(&'a self, target: &[i64; DIMENSIONS], candidate: Option<&'a Node>) -> &'a Node {
        match self {
            Self::Leaf { .. } => self,
            Self::SubTree { children, .. } => {
                let mut min_distance = candidate.map_or(i64::MAX, |c| c.distance(target));
                let mut closest = candidate;

                for child in children {
                    // `distance_below` returns `None` exactly when
                    // `min_distance > child_distance` is false, so this `else`
                    // is the old `if`'s skipped branch, not a new one.
                    let Some(child_distance) = child.distance_below(target, min_distance) else {
                        continue;
                    };
                    let leaf = child.search(target, closest);
                    // Vanilla skips recomputing when the child *is* the
                    // returned leaf, which is only an optimisation — but
                    // it also means a Leaf child's distance is measured
                    // once, so keep the shape.
                    let leaf_distance = if matches!(child, Self::Leaf { .. }) {
                        child_distance
                    } else {
                        leaf.distance(target)
                    };
                    if min_distance > leaf_distance {
                        min_distance = leaf_distance;
                        closest = Some(leaf);
                    }
                }

                closest.expect("a subtree search always ends with a leaf")
            }
        }
    }
}

/// `Climate.RTree.buildParameterSpace`.
fn build_parameter_space(children: &[Node]) -> [Parameter; DIMENSIONS] {
    assert!(!children.is_empty(), "SubTree needs at least one child");
    let mut bounds: [Option<Parameter>; DIMENSIONS] = [None; DIMENSIONS];
    for child in children {
        let space = child.space();
        for d in 0..DIMENSIONS {
            bounds[d] = Some(space[d].span(bounds[d]));
        }
    }
    bounds.map(|b| b.expect("every axis was spanned"))
}

/// The centre of an axis, truncating — `(min + max) / 2` in `long`.
const fn centre(p: Parameter) -> i64 {
    (p.min + p.max) / 2
}

/// `RTree.sort` — order by the chosen dimension, then every other dimension
/// in wrap-around order as tie-breakers.
fn sort_nodes(children: &mut [Node], dimension: usize, absolute: bool) {
    let key = |node: &Node| -> [i64; DIMENSIONS] {
        let space = node.space();
        let mut out = [0i64; DIMENSIONS];
        for d in 0..DIMENSIONS {
            let c = centre(space[(dimension + d) % DIMENSIONS]);
            out[d] = if absolute { c.abs() } else { c };
        }
        out
    };
    // Stable, matching Java's List.sort — equal keys keep their order, and
    // that order feeds the tie-break in `search`.
    children.sort_by_key(key);
}

/// `RTree.cost` — total edge length of a box.
fn cost(space: &[Parameter; DIMENSIONS]) -> i64 {
    space.iter().map(|p| (p.max - p.min).abs()).sum()
}

/// `RTree.bucketize` — split into groups of `6^floor(log6(n - 0.01))`.
///
/// The `- 0.01` makes an exact power of six bucket one level down rather than
/// producing a single full bucket. `n` is an integer so `n - 0.01` is never
/// within an ulp of a power of six, which is why using Rust's `ln` here
/// cannot disagree with Java's `Math.log` about the floor.
fn bucketize(nodes: &[Node]) -> Vec<Node> {
    let expected = 6.0_f64.powf(((nodes.len() as f64 - 0.01).ln() / 6.0_f64.ln()).floor()) as usize;
    let expected = expected.max(1);

    let mut buckets = Vec::new();
    let mut children: Vec<Node> = Vec::new();
    for child in nodes {
        children.push(child.clone());
        if children.len() >= expected {
            buckets.push(sub_tree(std::mem::take(&mut children)));
        }
    }
    if !children.is_empty() {
        buckets.push(sub_tree(children));
    }
    buckets
}

fn sub_tree(children: Vec<Node>) -> Node {
    Node::SubTree {
        space: build_parameter_space(&children),
        children,
    }
}

/// `RTree.build`.
fn build(mut children: Vec<Node>) -> Node {
    assert!(
        !children.is_empty(),
        "need at least one child to build a node"
    );
    if children.len() == 1 {
        return children.pop().expect("length checked");
    }
    if children.len() <= 6 {
        // Ordered by total distance from the origin across all axes, which
        // groups climatically central boxes together.
        children.sort_by_key(|node| node.space().iter().map(|p| centre(*p).abs()).sum::<i64>());
        return sub_tree(children);
    }

    // Try splitting along each axis and keep whichever produces the
    // tightest total bounding boxes.
    let mut min_cost = i64::MAX;
    let mut min_dimension = 0usize;
    let mut min_buckets: Vec<Node> = Vec::new();
    for d in 0..DIMENSIONS {
        sort_nodes(&mut children, d, false);
        let buckets = bucketize(&children);
        let total: i64 = buckets.iter().map(|b| cost(b.space())).sum();
        // Strict `>`, so the *first* dimension achieving the minimum wins.
        if min_cost > total {
            min_cost = total;
            min_dimension = d;
            min_buckets = buckets;
        }
    }

    sort_nodes(&mut min_buckets, min_dimension, true);
    let rebuilt = min_buckets
        .into_iter()
        .map(|bucket| match bucket {
            Node::SubTree { children, .. } => build(children),
            leaf @ Node::Leaf { .. } => leaf,
        })
        .collect();
    sub_tree(rebuilt)
}

/// `Climate.ParameterList` — the biome table plus its search index.
///
/// Generic over the value so the same structure serves biomes and the
/// spawn-target list.
#[derive(Clone, Debug)]
pub struct ParameterList<T> {
    values: Vec<(ParameterPoint, T)>,
    root: Node,
    /// Vanilla's `ThreadLocal lastResult`, made explicit. See the module
    /// docs: it is the tie-break, not a cache.
    last_result: Option<Node>,
}

impl<T> ParameterList<T> {
    /// # Panics
    /// If `values` is empty — vanilla throws for the same reason, since an
    /// empty tree has no nearest anything.
    #[must_use]
    pub fn new(values: Vec<(ParameterPoint, T)>) -> Self {
        assert!(
            !values.is_empty(),
            "need at least one value to build the search tree"
        );
        let leaves = values
            .iter()
            .enumerate()
            .map(|(index, (point, _))| Node::Leaf {
                space: point.parameter_space(),
                value: index,
            })
            .collect();
        Self {
            values,
            root: build(leaves),
            last_result: None,
        }
    }

    #[must_use]
    pub fn values(&self) -> &[(ParameterPoint, T)] {
        &self.values
    }

    /// `ParameterList.findValue` — the nearest box's value.
    ///
    /// Takes `&mut self` because the previous result seeds the next search
    /// and breaks its ties. Calling this out of vanilla's query order can
    /// return a different biome on an exact tie.
    pub fn find_value(&mut self, target: TargetPoint) -> &T {
        let array = target.to_parameter_array();
        // One clone, for the hint the next query prunes with — not one per
        // level of the tree. The borrow of `root` and `last_result` has to
        // end before `last_result` can be written, which is what the block
        // is for.
        let (index, hint) = {
            let leaf = self.root.search(&array, self.last_result.as_ref());
            match leaf {
                Node::Leaf { value, .. } => (*value, leaf.clone()),
                Node::SubTree { .. } => unreachable!("search returns a leaf"),
            }
        };
        self.last_result = Some(hint);
        &self.values[index].1
    }

    /// The nearest box ignoring any previous query — a pure function.
    ///
    /// Not what vanilla calls during generation, and deliberately named so
    /// nobody reaches for it by accident. Useful for tests and for tools that
    /// have no query order to reproduce.
    #[must_use]
    pub fn find_value_stateless(&self, target: TargetPoint) -> &T {
        let array = target.to_parameter_array();
        match self.root.search(&array, None) {
            Node::Leaf { value, .. } => &self.values[*value].1,
            Node::SubTree { .. } => unreachable!("search returns a leaf"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(t: f32, h: f32, c: f32, e: f32, d: f32, w: f32, offset: f32) -> ParameterPoint {
        ParameterPoint {
            temperature: Parameter::from_floats(t, t),
            humidity: Parameter::from_floats(h, h),
            continentalness: Parameter::from_floats(c, c),
            erosion: Parameter::from_floats(e, e),
            depth: Parameter::from_floats(d, d),
            weirdness: Parameter::from_floats(w, w),
            offset: quantize_coord(offset),
        }
    }

    fn target(t: f32, h: f32) -> TargetPoint {
        TargetPoint {
            temperature: quantize_coord(t),
            humidity: quantize_coord(h),
            continentalness: 0,
            erosion: 0,
            depth: 0,
            weirdness: 0,
        }
    }

    #[test]
    fn quantisation_truncates_toward_zero() {
        assert_eq!(quantize_coord(0.5), 5000);
        assert_eq!(quantize_coord(-0.5), -5000);
        // Both sides of zero collapse to 0 rather than rounding outward.
        assert_eq!(quantize_coord(0.000_049), 0);
        assert_eq!(quantize_coord(-0.000_049), 0);
        assert_eq!(quantize_coord(2.0), 20000);
    }

    #[test]
    fn interval_distance_is_zero_inside_and_signed_outside() {
        let p = Parameter::new(-100, 100);
        assert_eq!(p.distance(0), 0);
        assert_eq!(p.distance(-100), 0);
        assert_eq!(p.distance(100), 0);
        assert_eq!(p.distance(150), 50);
        assert_eq!(p.distance(-150), 50);
    }

    #[test]
    fn span_grows_to_cover_both() {
        let a = Parameter::new(-10, 10);
        let b = Parameter::new(5, 40);
        assert_eq!(a.span(Some(b)), Parameter::new(-10, 40));
        assert_eq!(a.span(None), a);
    }

    #[test]
    fn a_single_entry_list_always_answers_itself() {
        let mut list = ParameterList::new(vec![(point(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0), "only")]);
        assert_eq!(*list.find_value(target(1.5, -1.5)), "only");
    }

    #[test]
    fn the_nearest_box_wins() {
        let mut list = ParameterList::new(vec![
            (point(-1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0), "cold"),
            (point(1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0), "hot"),
        ]);
        assert_eq!(*list.find_value(target(-0.9, 0.0)), "cold");
        assert_eq!(*list.find_value(target(0.9, 0.0)), "hot");
    }

    #[test]
    fn offset_is_a_flat_penalty_not_a_box() {
        // Two boxes at the same place; the one with a nonzero offset is
        // always further away, so it can never win.
        let mut list = ParameterList::new(vec![
            (point(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0), "free"),
            (point(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.5), "penalised"),
        ]);
        assert_eq!(*list.find_value(target(0.0, 0.0)), "free");
        assert_eq!(*list.find_value(target(1.9, 1.9)), "free");
    }

    #[test]
    fn a_tie_breaks_toward_the_previous_result() {
        // Two boxes exactly equidistant from the target. Vanilla's strict `>`
        // keeps whatever came in as the candidate, so the answer depends on
        // the query before it. This is the behaviour that makes find_value
        // take &mut self.
        let mut list = ParameterList::new(vec![
            (point(-1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0), "left"),
            (point(1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0), "right"),
        ]);
        let midpoint = target(0.0, 0.0);

        // Prime with something unambiguous, then ask the tied question.
        assert_eq!(*list.find_value(target(-1.9, 0.0)), "left");
        let after_left = *list.find_value(midpoint);

        let mut list = ParameterList::new(vec![
            (point(-1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0), "left"),
            (point(1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0), "right"),
        ]);
        assert_eq!(*list.find_value(target(1.9, 0.0)), "right");
        let after_right = *list.find_value(midpoint);

        assert_ne!(
            after_left, after_right,
            "an exact tie should follow the previous result; if this ever \
             passes as equal the pruning comparison has been loosened from \
             `>` to `>=` and biome borders will move"
        );
    }

    #[test]
    fn the_stateless_lookup_ignores_history() {
        let mut list = ParameterList::new(vec![
            (point(-1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0), "left"),
            (point(1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0), "right"),
        ]);
        let midpoint = target(0.0, 0.0);
        let _ = list.find_value(target(-1.9, 0.0));
        let stateless_a = *list.find_value_stateless(midpoint);
        let _ = list.find_value(target(1.9, 0.0));
        let stateless_b = *list.find_value_stateless(midpoint);
        assert_eq!(stateless_a, stateless_b);
    }

    /// The pruning shortcut, checked against the thing it replaces at every
    /// bound that matters — including the exact distance itself, which is the
    /// one the strict `>` turns on and therefore the one a tie rides on.
    #[test]
    fn the_bounded_distance_agrees_with_the_exact_one() {
        let leaf = |t: i64, h: i64| Node::Leaf {
            space: [
                Parameter::new(t - 100, t + 100),
                Parameter::new(h - 50, h + 50),
                Parameter::new(-10, 10),
                Parameter::new(-10, 10),
                Parameter::new(-10, 10),
                Parameter::new(-10, 10),
                Parameter::point(0),
            ],
            value: 0,
        };
        for t in [-9000i64, -3, 0, 17, 20000] {
            for h in [-5000i64, 0, 8000] {
                let node = leaf(t, h);
                let target = [0i64, 0, 0, 0, 0, 0, 0];
                let exact = node.distance(&target);
                for bound in [0, 1, exact.saturating_sub(1), exact, exact + 1, i64::MAX] {
                    let bounded = node.distance_below(&target, bound);
                    assert_eq!(
                        bounded.is_some(),
                        bound > exact,
                        "bound {bound} against exact {exact}"
                    );
                    if let Some(value) = bounded {
                        assert_eq!(value, exact, "the value below the bound must be exact");
                    }
                }
            }
        }
    }

    #[test]
    fn a_large_list_still_finds_the_exact_box() {
        // Past 6 entries the tree bucketizes, so this exercises the split
        // search rather than the flat one. Every box is asked for its own
        // centre and must answer itself.
        let entries: Vec<(ParameterPoint, usize)> = (0..40)
            .map(|i| {
                let t = -1.9 + (i as f32) * 0.09;
                (point(t, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0), i)
            })
            .collect();
        let mut list = ParameterList::new(entries);
        for i in 0..40 {
            let t = -1.9 + (i as f32) * 0.09;
            assert_eq!(*list.find_value(target(t, 0.0)), i, "box {i}");
        }
    }

    #[test]
    fn bucketize_splits_at_powers_of_six() {
        let leaf = |i: i64| Node::Leaf {
            space: [Parameter::new(i, i); DIMENSIONS],
            value: 0,
        };
        // 7 nodes: 6^floor(log6(6.99)) = 6^1 = 6, so 6 + 1.
        let nodes: Vec<Node> = (0..7).map(leaf).collect();
        let buckets = bucketize(&nodes);
        assert_eq!(buckets.len(), 2);
        // 36 nodes: 6^floor(log6(35.99)) = 6^1 = 6, so six buckets of six.
        let nodes: Vec<Node> = (0..36).map(leaf).collect();
        assert_eq!(bucketize(&nodes).len(), 6);
        // 37 nodes: 6^floor(log6(36.99)) = 6^2 = 36, so 36 + 1.
        let nodes: Vec<Node> = (0..37).map(leaf).collect();
        assert_eq!(bucketize(&nodes).len(), 2);
    }
}
