//! `java.util.HashSet` iteration order, against the JVM's own answer.
//!
//! Vectors: `data/vanilla/hash_order.json`, from `ci/hash-order-golden.sh`,
//! which inserts six sequences of `BlockPos` into a real `HashSet` and dumps
//! what came out.
//!
//! # Why this is a gate and not a unit test
//!
//! [`JavaHashSet`] can be made self-consistent — bucket-major, chains in
//! insertion order, a resize that splits — and still be wrong, because the
//! whole thing hangs on details that are easy to get plausibly wrong: an
//! unsigned shift written as a signed one, a resize that rebuilds rather than
//! splits, a load factor of 0.75 applied at `>=` rather than `>`. Each of
//! those produces a *different* order that still looks like a hash table's.
//! Only the JVM knows which one is right.
//!
//! The six cases are chosen to reach those details:
//!
//! - `trunk` — one column, no resize. Proves the bucket walk.
//! - `resize` — thirteen entries, one resize, so the split has to preserve
//!   relative order.
//! - `resize_twice` and `blob` — two and three resizes.
//! - `negative` — coordinates whose `hashCode` is negative, where a signed
//!   `>>` would smear ones through the high half and land in another bucket.
//! - `scatter` — two hundred unrelated positions, so buckets are not walked
//!   in anything like insertion order.
//!
//! # And eleven more that are not insertion lists
//!
//! Every case above is short buckets, so a port could reproduce all six and
//! still be wrong about every spruce in the world: past nine entries in a
//! bucket of a sixty-four bucket table, `java.util.HashMap` replaces the list
//! with a red-black tree and the iterator's answer changes. `ops_cases` is a
//! second block of vectors that reaches that, as **operation streams** rather
//! than insertion lists — removal is half of what a tree does, and the
//! leaf-distance flood removes constantly.
//!
//! Each carries the iteration order's fingerprint after *every* operation, so
//! a divergence names the operation that caused it instead of surfacing seven
//! hundred operations later.

use std::path::Path;

use rustmc_worldgen::java_hash_set::JavaHashSet;

const VECTORS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../data/vanilla/hash_order.json"
);

#[test]
fn hash_set_iteration_order_matches_the_jvm() {
    let path = Path::new(VECTORS);
    if !rustmc_data::fixture::available(path, "hash-order extract", "ci/hash-order-golden.sh") {
        return;
    }
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("readable")).expect("json");

    let mut cases = 0usize;
    let mut positions = 0usize;
    for case in doc["cases"].as_array().expect("cases") {
        let name = case["name"].as_str().expect("name");
        let inserted = triples(&case["inserted"]);
        let expected = triples(&case["order"]);

        let mut set = JavaHashSet::new();
        for key in &inserted {
            set.insert(*key)
                .unwrap_or_else(|e| panic!("{name}: {}", e.0));
        }
        assert_eq!(
            set.len(),
            usize::try_from(case["size"].as_u64().expect("size")).expect("usize"),
            "{name}: the set has a different number of entries than the JVM's"
        );
        let actual = set.to_vec();
        assert_eq!(
            actual,
            expected,
            "{name}: iteration order differs from the JVM's — first difference at {:?}",
            actual.iter().zip(&expected).position(|(a, b)| a != b)
        );
        cases += 1;
        positions += expected.len();
    }

    assert_eq!(cases, 6, "the extract should carry six cases");
    println!("HASH ORDER: {cases} cases, {positions} positions, byte-for-byte with the JVM");
}

/// The treeified cases: add/remove streams, checked after every step.
#[test]
fn tree_bin_iteration_order_matches_the_jvm() {
    let path = Path::new(VECTORS);
    if !rustmc_data::fixture::available(path, "hash-order extract", "ci/hash-order-golden.sh") {
        return;
    }
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("readable")).expect("json");
    let Some(cases) = doc["ops_cases"].as_array() else {
        panic!("the extract has no `ops_cases` — regenerate with ci/hash-order-golden.sh");
    };

    let mut checked = 0usize;
    let mut operations = 0usize;
    let mut reached = 0usize;
    let mut by_case: Vec<String> = Vec::new();
    for case in cases {
        let name = case["name"].as_str().expect("name");
        let expected_fp: Vec<i64> = case["fingerprints"]
            .as_array()
            .expect("fingerprints")
            .iter()
            .map(|v| v.as_i64().expect("a long"))
            .collect();
        let ops = case["ops"].as_array().expect("ops");
        assert_eq!(
            ops.len(),
            expected_fp.len(),
            "{name}: one fingerprint per op"
        );

        let mut set: JavaHashSet<(i32, i32, i32)> = JavaHashSet::new();
        for (i, op) in ops.iter().enumerate() {
            let op = op.as_array().expect("[add, x, y, z]");
            let at = |k: usize| i32::try_from(op[k].as_i64().expect("int")).expect("i32");
            let pos = (at(1), at(2), at(3));
            if at(0) == 1 {
                set.insert(pos)
                    .unwrap_or_else(|e| panic!("{name}: op {i} refused: {}", e.0));
            } else {
                set.remove(&pos);
            }
            assert_eq!(
                fingerprint(&set),
                expected_fp[i],
                "{name}: iteration order diverges from the JVM's at operation {i} \
                 ({} {pos:?}), with {} entries — this is the FIRST operation that \
                 differs, not merely one that does",
                if at(0) == 1 { "add" } else { "remove" },
                set.len()
            );
        }

        assert_eq!(
            set.len(),
            usize::try_from(case["size"].as_u64().expect("size")).expect("usize"),
            "{name}: the set has a different number of entries than the JVM's"
        );
        assert_eq!(
            set.to_vec(),
            triples(&case["order"]),
            "{name}: the final order differs even though every fingerprint matched"
        );
        // A gate that stopped reaching the thing it gates would keep passing.
        // These two pin the boundary the whole port turns on.
        let treeified = set.treeifications();
        match name {
            "tree_min" => assert!(
                treeified >= 1,
                "tree_min is nine in one bucket and must have treeified"
            ),
            "tree_eight_is_a_list" => assert_eq!(
                treeified, 0,
                "eight in a bucket must NOT treeify — the trigger is nine"
            ),
            _ => {}
        }
        reached += usize::from(treeified > 0);
        by_case.push(format!("{name}={treeified}"));

        checked += 1;
        operations += ops.len();
    }

    assert!(checked >= 11, "the extract should carry the tree cases");
    assert!(
        reached >= 8,
        "only {reached} of {checked} cases reached a tree at all: {}",
        by_case.join(" ")
    );
    println!("TREE BINS: {checked} cases, {operations} operations, byte-for-byte with the JVM");
    println!("           treeify events per case: {}", by_case.join(" "));
}

/// FNV-1a over the iteration order, as `HashOrderGolden.fingerprint` computes
/// it: order-sensitive, so two sets with the same members in a different
/// order disagree here.
fn fingerprint(set: &JavaHashSet<(i32, i32, i32)>) -> i64 {
    const PRIME: i64 = 0x0000_0100_0000_01b3;
    let mut fp = 0xcbf2_9ce4_8422_2325_u64 as i64;
    for p in set.iter() {
        fp = (fp ^ i64::from(p.0)).wrapping_mul(PRIME);
        fp = (fp ^ i64::from(p.1)).wrapping_mul(PRIME);
        fp = (fp ^ i64::from(p.2)).wrapping_mul(PRIME);
    }
    fp
}

fn triples(value: &serde_json::Value) -> Vec<(i32, i32, i32)> {
    value
        .as_array()
        .expect("a list of positions")
        .iter()
        .map(|p| {
            let p = p.as_array().expect("[x, y, z]");
            let at = |i: usize| i32::try_from(p[i].as_i64().expect("coordinate")).expect("i32");
            (at(0), at(1), at(2))
        })
        .collect()
}
