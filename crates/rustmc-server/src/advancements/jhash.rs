//! `java.util.HashMap`'s iteration order, and the two hashes that feed it.
//!
//! # Why a container is being ported at all
//!
//! `PlayerAdvancements.flushDirty` builds the three things
//! `ClientboundUpdateAdvancementsPacket` carries out of `java.util`
//! collections and hands them straight to the codec:
//!
//! * `added` — `new HashSet<AdvancementHolder>()`, whose `hashCode` is
//!   `Identifier.hashCode()`, wrapped by `List.copyOf` at the packet
//!   constructor;
//! * `removed` — `new HashSet<Identifier>()`;
//! * the progress map — `new HashMap<Identifier, AdvancementProgress>()`.
//!
//! `HashSet` is a `HashMap` with a constant value, so all three are one
//! order. A `BTreeMap` would put them in lexicographic id order, which is not
//! the order vanilla writes, so this is a port and not a preference.
//!
//! # What this order does and does not buy
//!
//! It is not enough to make the packet byte-comparable — see
//! [`super`]'s header for the two reasons nothing can be — and it is
//! deliberately not sold as such. It is what makes the *sequence* of ids in
//! `added` reproducible from the data for every bucket no two ids share, and
//! the ids in a shared bucket are then in insertion order, which is the one
//! place this server's sorted sibling order is observable.
//!
//! # The ten lines this is, read out of the JDK's own source contract
//!
//! Everything below is `java.util.HashMap` as specified by its own behaviour
//! since Java 8 and unchanged in 25 (the JDK this project builds against):
//! default capacity 16, load factor 0.75, `hash(key) = h ^ (h >>> 16)`,
//! `index = (n - 1) & hash`, a new node appended at the **tail** of its
//! bucket, a resize splitting each bucket into a lo list (index `j`) and a hi
//! list (index `j + oldCap`) by `hash & oldCap` while preserving relative
//! order, and iteration walking table slots ascending and each bucket front
//! to back. Treeification at eight nodes in one bucket is **not** reproduced
//! and cannot be reached here: it also requires a table of at least 64, and
//! the largest set this ever holds is one advancement tree — 47 ids, table
//! 64, largest bucket 2, measured by `ci/advancements-golden.sh`.
//!
//! `ci/advancements-golden.sh` emits vanilla's own answers for the five
//! visible trees under both insertion orders, and
//! `hash_order_matches_the_jvm` compares against them. Without those vectors
//! this file would be a plausible reimplementation, which
//! `crates/rustmc-server/src/openhash.rs`'s header argues at length is worse
//! than none.

/// `java.lang.String.hashCode()` — `s[0]*31^(n-1) + s[1]*31^(n-2) + …`, over
/// UTF-16 code units, wrapping at 32 bits.
#[must_use]
pub fn string_hash(s: &str) -> i32 {
    let mut h: i32 = 0;
    for u in s.encode_utf16() {
        h = h.wrapping_mul(31).wrapping_add(i32::from(u));
    }
    h
}

/// `Identifier.hashCode()` — `31 * namespace.hashCode() + path.hashCode()`.
///
/// An id with no `:` is `minecraft:`-namespaced, matching
/// `Identifier.withDefaultNamespace`; every id this server holds is written
/// out in full, so the fallback exists to keep a caller from silently hashing
/// `"story/root"` as a namespace.
#[must_use]
pub fn identifier_hash(id: &str) -> i32 {
    let (ns, path) = match id.split_once(':') {
        Some((ns, path)) => (ns, path),
        None => ("minecraft", id),
    };
    string_hash(ns)
        .wrapping_mul(31)
        .wrapping_add(string_hash(path))
}

/// `HashMap.hash(Object)` — `h ^ (h >>> 16)`, the spread that stops a hash
/// whose low bits are constant from piling into one bucket.
#[must_use]
pub fn spread(h: i32) -> i32 {
    #[allow(clippy::cast_possible_wrap, clippy::cast_sign_loss)]
    let u = h as u32;
    #[allow(clippy::cast_possible_wrap)]
    let out = (u ^ (u >> 16)) as i32;
    out
}

const DEFAULT_CAPACITY: usize = 16;

/// The order a `java.util.HashMap` iterates keys inserted in `insertion`
/// order, starting from a default-constructed map.
///
/// Duplicate keys are ignored after the first, as `put` on an existing key
/// replaces the value and leaves the node where it is.
#[must_use]
pub fn iteration_order(insertion: &[&str], hash_of: &dyn Fn(&str) -> i32) -> Vec<String> {
    let mut cap = DEFAULT_CAPACITY;
    let mut table: Vec<Vec<(i32, String)>> = vec![Vec::new(); cap];
    let mut size = 0usize;

    for key in insertion {
        let h = spread(hash_of(key));
        #[allow(clippy::cast_sign_loss)]
        let idx = (h as u32 as usize) & (cap - 1);
        if table[idx].iter().any(|(_, k)| k == key) {
            continue; // put() on an existing key does not move it
        }
        table[idx].push((h, (*key).to_string()));
        size += 1;
        // `if (++size > threshold) resize()`, threshold = cap * 0.75.
        if size > cap * 3 / 4 {
            let old_cap = cap;
            cap *= 2;
            let mut next: Vec<Vec<(i32, String)>> = vec![Vec::new(); cap];
            for (j, bucket) in table.into_iter().enumerate() {
                for (h, k) in bucket {
                    #[allow(clippy::cast_sign_loss)]
                    let hi = (h as u32 as usize) & old_cap != 0;
                    next[if hi { j + old_cap } else { j }].push((h, k));
                }
            }
            table = next;
        }
    }

    table
        .into_iter()
        .flat_map(|b| b.into_iter().map(|(_, k)| k))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Taken from a real JVM (`javac`/`java` on the pinned JDK 25), not from
    /// memory. Two of the five values first written here were wrong from
    /// recall — `"minecraft"` and `spread(-1)` — and both were caught by this
    /// case before anything read them, which is the argument for the case.
    #[test]
    fn string_hash_matches_java() {
        assert_eq!(string_hash(""), 0);
        assert_eq!(string_hash("a"), 97);
        assert_eq!(string_hash("hello"), 99_162_322);
        assert_eq!(string_hash("minecraft"), 695_073_197);
        assert_eq!(string_hash("story/root"), 1_450_125_756);
        // Wraps past i32 rather than saturating or panicking.
        assert_eq!(string_hash("polygenelubricants"), -2_147_483_648);
    }

    #[test]
    fn identifier_hash_is_31_times_namespace_plus_path() {
        let expect = string_hash("minecraft")
            .wrapping_mul(31)
            .wrapping_add(string_hash("story/root"));
        assert_eq!(identifier_hash("minecraft:story/root"), expect);
        assert_eq!(
            identifier_hash("story/root"),
            expect,
            "a bare path takes the default namespace"
        );
        assert_ne!(
            identifier_hash("minecraft:story/root"),
            identifier_hash("minecraft:story/roof")
        );
    }

    #[test]
    fn spread_folds_the_high_half_down() {
        assert_eq!(spread(0), 0);
        assert_eq!(spread(1), 1);
        // 0x00010000 ^ 0x00000001
        assert_eq!(spread(0x0001_0000), 0x0001_0001);
        assert_eq!(spread(-1), -65_536); // 0xFFFFFFFF ^ 0x0000FFFF = 0xFFFF0000
    }

    /// The property the whole file exists for: the order is NOT sorted, and
    /// it is not insertion order either.
    #[test]
    fn iteration_order_is_neither_sorted_nor_insertion() {
        let keys = [
            "minecraft:story/root",
            "minecraft:story/mine_stone",
            "minecraft:story/upgrade_tools",
            "minecraft:story/smelt_iron",
            "minecraft:story/obtain_armor",
        ];
        let order = iteration_order(&keys, &identifier_hash);
        assert_eq!(order.len(), keys.len());
        let sorted = {
            let mut s: Vec<String> = keys.iter().map(|k| (*k).to_string()).collect();
            s.sort();
            s
        };
        let inserted: Vec<String> = keys.iter().map(|k| (*k).to_string()).collect();
        assert!(
            order != sorted || order != inserted,
            "a walk that agreed with both would prove nothing"
        );
        // Every key appears exactly once whatever the buckets did.
        let mut seen = order.clone();
        seen.sort();
        assert_eq!(seen, sorted);
    }

    /// A resize must not lose or duplicate a key, and must move exactly the
    /// keys whose `hash & oldCap` bit is set.
    #[test]
    fn resize_preserves_every_key() {
        let keys: Vec<String> = (0..200).map(|i| format!("minecraft:a{i}")).collect();
        let refs: Vec<&str> = keys.iter().map(String::as_str).collect();
        let order = iteration_order(&refs, &identifier_hash);
        assert_eq!(order.len(), 200);
        let mut a = order;
        a.sort();
        let mut b = keys;
        b.sort();
        assert_eq!(a, b);
    }

    /// The vector table: vanilla's own `java.util.HashSet<Identifier>` walk
    /// over each visible tree's ids, under two insertion orders, from
    /// `ci/advancements-golden.sh`.
    ///
    /// **The equal-orders trees are the ones this case cannot score**, and
    /// that is checked rather than assumed: `end/root`'s nine ids share no
    /// bucket of sixteen, so its sorted and reversed walks are identical and
    /// a port that ignored insertion order entirely would pass on it. Four of
    /// the five trees do have a shared bucket and this asserts that at least
    /// one does, so a jar whose ids stopped colliding would turn this test
    /// into a weaker one loudly instead of quietly.
    #[test]
    fn hash_order_matches_the_jvm() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/vanilla/advancements.json");
        if !rustmc_data::fixture::available(
            &path,
            "advancement extract",
            "ci/advancements-golden.sh",
        ) {
            return;
        }
        let text = std::fs::read_to_string(&path).expect("read advancements.json");
        let doc: serde_json::Value = serde_json::from_str(&text).expect("parse");
        let orders = doc
            .get("hash_orders")
            .and_then(serde_json::Value::as_object)
            .expect("hash_orders");
        let mut trees = 0usize;
        let mut discriminating = 0usize;
        for (root, v) in orders {
            let read = |key: &str| -> Vec<String> {
                v.get(key)
                    .and_then(serde_json::Value::as_array)
                    .expect("order list")
                    .iter()
                    .map(|s| s.as_str().expect("id").to_string())
                    .collect()
            };
            let sorted = read("sorted");
            let reversed = read("reversed");
            if sorted.is_empty() {
                continue;
            }
            trees += 1;
            if sorted != reversed {
                discriminating += 1;
            }
            let mut ins: Vec<String> = sorted.clone();
            ins.sort();
            let refs: Vec<&str> = ins.iter().map(String::as_str).collect();
            assert_eq!(
                iteration_order(&refs, &identifier_hash),
                sorted,
                "{root}: sorted-insertion walk"
            );
            let rev_refs: Vec<&str> = ins.iter().rev().map(String::as_str).collect();
            assert_eq!(
                iteration_order(&rev_refs, &identifier_hash),
                reversed,
                "{root}: reversed-insertion walk"
            );
        }
        assert!(
            trees >= 5,
            "only {trees} non-empty trees in the vector table"
        );
        assert!(
            discriminating >= 1,
            "no tree's two insertion orders differ, so this case cannot tell a port \
             that honours insertion order from one that ignores it"
        );
    }

    #[test]
    fn a_repeated_key_does_not_move_or_duplicate() {
        let once = iteration_order(&["x:a", "x:b", "x:c"], &identifier_hash);
        let twice = iteration_order(&["x:a", "x:b", "x:a", "x:c", "x:b"], &identifier_hash);
        assert_eq!(once, twice);
    }

    /// Insertion order is observable only inside a shared bucket. Built to
    /// have one: two keys forced into the same slot of a 16-wide table.
    #[test]
    fn insertion_order_shows_only_inside_a_shared_bucket() {
        // Find a colliding pair under a 16-wide table.
        let mut pair = None;
        'outer: for i in 0..500 {
            for j in (i + 1)..500 {
                let a = format!("x:{i}");
                let b = format!("x:{j}");
                #[allow(clippy::cast_sign_loss)]
                let ia = (spread(identifier_hash(&a)) as u32 as usize) & 15;
                #[allow(clippy::cast_sign_loss)]
                let ib = (spread(identifier_hash(&b)) as u32 as usize) & 15;
                if ia == ib {
                    pair = Some((a, b));
                    break 'outer;
                }
            }
        }
        let (a, b) = pair.expect("some pair of 500 shares a bucket of 16");
        let fwd = iteration_order(&[a.as_str(), b.as_str()], &identifier_hash);
        let rev = iteration_order(&[b.as_str(), a.as_str()], &identifier_hash);
        assert_ne!(fwd, rev, "a shared bucket keeps insertion order");
        let far = iteration_order(&["x:0"], &identifier_hash);
        assert_eq!(far.len(), 1);
    }
}
