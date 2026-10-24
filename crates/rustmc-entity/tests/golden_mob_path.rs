//! `PathFinder`, `BinaryHeap`, `Node`, `Target` and `Path` against the JVM,
//! over a synthetic grid evaluator.
//!
//! Vectors: `crates/rustmc-entity/golden/mob_path_vectors.json`, from
//! `ci/mob-path-golden.sh`, which drives the jar's own `PathFinder` with a
//! `NodeEvaluator` subclass reading a fixed grid — no region, no mob, no
//! server. The grid evaluator below is that subclass's rule, written
//! twice on purpose: it is not `WalkNodeEvaluator`, and the deferred
//! golden for that class is named in `ground_path.rs`.
//!
//! What these vectors do check: the A* loop order and its early exits,
//! the 1.5 heuristic inflation, `g`/`h`/`f`/`walkedDistance` arithmetic in
//! float, the heap's tie-breaking, the reconstructed path, `distToTarget`,
//! and — over two, three, four and fifteen targets — the Java hash walk
//! that breaks ties between targets.

use std::path::Path as FsPath;

use rustmc_data::BlockPos;
use rustmc_entity::pathfinder::{
    create_hash, BinaryHeap, NodeEvaluator, NodeEvaluatorBase, NodeId, Nodes, PathFinder, PathType,
    PathfindingContext, Target,
};
use rustmc_entity::phys::Direction;
use rustmc_entity::testing::{GridLevel, TestMob};

const VECTORS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/golden/mob_path_vectors.json");

fn cell(c: char) -> PathType {
    match c {
        '.' => PathType::Walkable,
        '#' => PathType::Blocked,
        'w' => PathType::Water,
        'f' => PathType::Fire,
        'n' => PathType::FireInNeighbor,
        'o' => PathType::Open,
        'd' => PathType::Damaging,
        'b' => PathType::Breach,
        'h' => PathType::StickyHoney,
        other => panic!("cell {other}"),
    }
}

/// The golden's `GridEvaluator`, rule for rule.
struct GridEvaluator {
    base: NodeEvaluatorBase,
    layers: Vec<Vec<String>>,
    start: (i32, i32, i32),
}

impl GridEvaluator {
    fn at(&self, x: i32, y: i32, z: i32) -> PathType {
        if y < 0 || y as usize >= self.layers.len() {
            return PathType::Blocked;
        }
        let layer = &self.layers[y as usize];
        if z < 0 || z as usize >= layer.len() {
            return PathType::Blocked;
        }
        let row = layer[z as usize].as_bytes();
        if x < 0 || x as usize >= row.len() {
            return PathType::Blocked;
        }
        cell(row[x as usize] as char)
    }

    fn typed(&mut self, x: i32, y: i32, z: i32) -> NodeId {
        let id = self.base.node(x, y, z);
        let t = self.at(x, y, z);
        let n = self.base.nodes.get_mut(id);
        n.path_type = t;
        n.cost_malus = t.malus();
        id
    }

    fn accept(&mut self, x: i32, y: i32, z: i32) -> Option<NodeId> {
        let t = self.at(x, y, z);
        if t.malus() >= 0.0 && t != PathType::Open {
            return Some(self.typed(x, y, z));
        }
        if t == PathType::Open {
            let below = self.at(x, y - 1, z);
            if below.malus() >= 0.0 && below != PathType::Open {
                return Some(self.typed(x, y - 1, z));
            }
            return None;
        }
        let above = self.at(x, y + 1, z);
        if above.malus() >= 0.0 && above != PathType::Open {
            return Some(self.typed(x, y + 1, z));
        }
        None
    }
}

impl NodeEvaluator<TestMob, GridLevel> for GridEvaluator {
    fn base(&self) -> &NodeEvaluatorBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut NodeEvaluatorBase {
        &mut self.base
    }
    fn prepare(&mut self, _ctx: &PathfindingContext<'_, TestMob, GridLevel>) {
        self.base.nodes.clear();
    }
    fn done(&mut self) {}
    fn get_start(&mut self, _ctx: &PathfindingContext<'_, TestMob, GridLevel>) -> Option<NodeId> {
        let (x, y, z) = self.start;
        Some(self.typed(x, y, z))
    }
    fn get_target(
        &mut self,
        _ctx: &PathfindingContext<'_, TestMob, GridLevel>,
        x: f64,
        y: f64,
        z: f64,
    ) -> Target {
        self.base.target_node_at(x, y, z)
    }
    fn get_neighbors(
        &mut self,
        _ctx: &PathfindingContext<'_, TestMob, GridLevel>,
        out: &mut [Option<NodeId>; 32],
        node: NodeId,
    ) -> usize {
        let (nx, ny, nz) = {
            let n = self.base.nodes.get(node);
            (n.x, n.y, n.z)
        };
        let mut n = 0;
        let mut ortho: [Option<NodeId>; 4] = [None; 4];
        for (i, d) in Direction::HORIZONTAL.iter().enumerate() {
            let pick = self.accept(nx + d.step_x(), ny, nz + d.step_z());
            ortho[i] = pick;
            if let Some(p) = pick {
                if !self.base.nodes.get(p).closed {
                    out[n] = Some(p);
                    n += 1;
                }
            }
        }
        for (i, d) in Direction::HORIZONTAL.iter().enumerate() {
            let cw = d.clockwise();
            let j = (i + 1) % 4;
            let (Some(a), Some(b)) = (ortho[i], ortho[j]) else {
                continue;
            };
            if self.base.nodes.get(a).y != ny || self.base.nodes.get(b).y != ny {
                continue;
            }
            let x = nx + d.step_x() + cw.step_x();
            let z = nz + d.step_z() + cw.step_z();
            if self.at(x, ny, z).malus() < 0.0 {
                continue;
            }
            let diag = self.typed(x, ny, z);
            if !self.base.nodes.get(diag).closed {
                out[n] = Some(diag);
                n += 1;
            }
        }
        n
    }
    fn get_path_type(
        &self,
        _ctx: &PathfindingContext<'_, TestMob, GridLevel>,
        x: i32,
        y: i32,
        z: i32,
    ) -> PathType {
        self.at(x, y, z)
    }
}

fn i(v: &serde_json::Value) -> i32 {
    v.as_i64().expect("int") as i32
}

fn fbits(v: &serde_json::Value) -> u32 {
    v.as_i64().expect("bits") as i32 as u32
}

#[test]
fn path_finder_matches_the_jvm_over_grids() {
    let path = FsPath::new(VECTORS);
    if !rustmc_data::fixture::available(path, "mob-path extract", "ci/mob-path-golden.sh") {
        return;
    }
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("readable")).expect("json");
    let level = GridLevel::new();
    let mob = TestMob::new(level, 0.0, 0.0, 0.0, 1);
    let ctx = PathfindingContext::new(&mob, &mob.level, None);

    let mut cases = 0;
    for case in doc["grids"].as_array().expect("grids") {
        let name = case["name"].as_str().expect("name");
        let layers: Vec<Vec<String>> = case["layers"]
            .as_array()
            .expect("layers")
            .iter()
            .map(|l| {
                l.as_array()
                    .expect("layer")
                    .iter()
                    .map(|r| r.as_str().expect("row").to_string())
                    .collect()
            })
            .collect();
        let s = case["start"].as_array().expect("start");
        let start = (i(&s[0]), i(&s[1]), i(&s[2]));
        let targets: Vec<BlockPos> = case["targets"]
            .as_array()
            .expect("targets")
            .iter()
            .map(|t| BlockPos {
                x: i(&t[0]),
                y: i(&t[1]),
                z: i(&t[2]),
            })
            .collect();
        let max_range = f32::from_bits(fbits(&case["max_range"]));
        let accuracy = i(&case["accuracy"]);
        let depth_mul = f32::from_bits(fbits(&case["depth_mul"]));
        let max_visited = i(&case["max_visited"]);

        let eval = GridEvaluator {
            base: NodeEvaluatorBase::new(),
            layers,
            start,
        };
        let mut finder = PathFinder::new(eval, max_visited);
        let got = finder.find_path(&ctx, &targets, max_range, accuracy, depth_mul);
        let want = &case["path"];
        if want.is_null() {
            assert!(
                got.is_none(),
                "{name}: jar found no path, port found {got:?}"
            );
            cases += 1;
            continue;
        }
        let got = got.unwrap_or_else(|| panic!("{name}: jar found a path, port found none"));
        assert_eq!(
            got.can_reach(),
            want["reached"].as_bool().expect("reached"),
            "{name}: reached"
        );
        assert_eq!(
            got.dist_to_target().to_bits(),
            fbits(&want["dist_to_target"]),
            "{name}: distToTarget {} vs jar {}",
            got.dist_to_target(),
            f32::from_bits(fbits(&want["dist_to_target"]))
        );
        let t = want["target"].as_array().expect("target");
        assert_eq!(
            got.target(),
            BlockPos {
                x: i(&t[0]),
                y: i(&t[1]),
                z: i(&t[2])
            },
            "{name}: target"
        );
        let nodes = want["nodes"].as_array().expect("nodes");
        let got_nodes: Vec<(i32, i32, i32)> = got.nodes().iter().map(|n| (n.x, n.y, n.z)).collect();
        let want_nodes: Vec<(i32, i32, i32)> = nodes
            .iter()
            .map(|n| (i(&n[0]), i(&n[1]), i(&n[2])))
            .collect();
        assert_eq!(got_nodes, want_nodes, "{name}: node positions");
        for (k, (g, w)) in got.nodes().iter().zip(nodes.iter()).enumerate() {
            assert_eq!(
                g.path_type.name(),
                w[3].as_str().expect("type"),
                "{name}: node {k} type"
            );
            assert_eq!(
                g.cost_malus.to_bits(),
                fbits(&w[4]),
                "{name}: node {k} costMalus"
            );
            assert_eq!(g.g.to_bits(), fbits(&w[5]), "{name}: node {k} g");
            assert_eq!(g.h.to_bits(), fbits(&w[6]), "{name}: node {k} h");
            assert_eq!(g.f.to_bits(), fbits(&w[7]), "{name}: node {k} f");
            assert_eq!(
                g.walked_distance.to_bits(),
                fbits(&w[8]),
                "{name}: node {k} walkedDistance"
            );
        }
        cases += 1;
    }
    assert!(cases >= 15, "only {cases} grid cases");
}

#[test]
fn binary_heap_matches_the_jvm_replay() {
    let path = FsPath::new(VECTORS);
    if !rustmc_data::fixture::available(path, "mob-path extract", "ci/mob-path-golden.sh") {
        return;
    }
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("readable")).expect("json");
    let heap_doc = &doc["heap"];
    let mut nodes = Nodes::new();
    let mut heap = BinaryHeap::new();
    let mut ids = Vec::new();
    for (k, f) in heap_doc["inserts"]
        .as_array()
        .expect("inserts")
        .iter()
        .enumerate()
    {
        let id = nodes.get_or_create(k as i32, 0, 0);
        nodes.get_mut(id).f = f32::from_bits(fbits(f));
        heap.insert(&mut nodes, id);
        ids.push(id);
    }
    for change in heap_doc["changes"].as_array().expect("changes") {
        let idx = i(&change[0]) as usize;
        let f = f32::from_bits(fbits(&change[1]));
        heap.change_cost(&mut nodes, ids[idx], f);
    }
    let before = i(&heap_doc["pops_before_remove"]) as usize;
    let mut got: Vec<i32> = Vec::new();
    for _ in 0..before {
        let id = heap.pop(&mut nodes);
        got.push(nodes.get(id).x);
    }
    let want: Vec<i32> = heap_doc["pop_order"]
        .as_array()
        .expect("pop_order")
        .iter()
        .map(i)
        .collect();
    assert_eq!(got, want, "pop order before removes");
    for r in heap_doc["removes"].as_array().expect("removes") {
        heap.remove(&mut nodes, ids[i(r) as usize]);
    }
    let mut got: Vec<i32> = Vec::new();
    while !heap.is_empty() {
        let id = heap.pop(&mut nodes);
        got.push(nodes.get(id).x);
    }
    let want: Vec<i32> = heap_doc["pop_order_after"]
        .as_array()
        .expect("pop_order_after")
        .iter()
        .map(i)
        .collect();
    assert_eq!(got, want, "pop order after removes");
}

#[test]
fn create_hash_matches_the_jvm() {
    let path = FsPath::new(VECTORS);
    if !rustmc_data::fixture::available(path, "mob-path extract", "ci/mob-path-golden.sh") {
        return;
    }
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("readable")).expect("json");
    for row in doc["hash"].as_array().expect("hash") {
        let (x, y, z, want) = (i(&row[0]), i(&row[1]), i(&row[2]), i(&row[3]));
        assert_eq!(create_hash(x, y, z), want, "createHash({x}, {y}, {z})");
    }
}
