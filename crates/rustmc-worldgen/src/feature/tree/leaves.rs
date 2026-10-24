//! `TreeFeature#updateLeaves` — the pass that decides which leaves decay.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `TreeFeature#updateLeaves`, `BitSetDiscreteVoxelShape`,
//! `LeavesBlock#getOptionalDistanceAt`.
//!
//! A tree writes its leaves with whatever `distance` the config's provider
//! carries — 7 for every vanilla tree, which is the value that decays. This
//! pass then floods outward from the logs and rewrites each leaf with its
//! real distance, so the ones within six blocks of wood survive. Without it
//! every tree in the world is placed correctly and then falls apart on the
//! first block tick, which looks like a decay bug and is a worldgen one.
//!
//! # It is a breadth-first flood with a very unusual queue
//!
//! Seven sets, one per distance. Distance 0 starts as every **log** the tree
//! wrote. Then, repeatedly: find the lowest non-empty distance, take **one**
//! position out of it, mark it, and push each unmarked neighbour into the set
//! for `min(itsOwnDistance, thisDistance + 1)`.
//!
//! Two details make it not an ordinary BFS. The current distance is pulled
//! *back down* whenever a neighbour lands in a lower set — `distance =
//! min(distance, next)` — so the scan can revisit a level it has already
//! left. And the position taken out of a set is the first in **`HashSet`
//! iteration order**, which is why this module takes a [`JavaHashSet`]: the
//! order decides which leaf is reached from which log, and so which
//! `distance` a leaf equidistant from two branches ends up with.
//!
//! # The shape is a mask, not a set
//!
//! Logs and roots are painted into a `BitSetDiscreteVoxelShape` before the
//! flood starts, and every position the flood marks joins them. A neighbour
//! already in the shape is skipped — that is what stops the flood walking
//! back over itself, and it is why a log never gets a distance written to it.

use crate::java_hash_set::JavaHashSet;

use super::{Placed, Pos};

/// `LeavesBlock.DECAY_DISTANCE` — the value at which a leaf decays, and the
/// number of sets the flood keeps.
const DECAY_DISTANCE: i32 = 7;

/// The world this pass reads and writes.
pub trait LeafWorld {
    fn block_at(&mut self, x: i32, y: i32, z: i32) -> String;
    /// `setBlockKnownShape` — flag 19, so no shape update, but the block is
    /// the same write as any other as far as a golden dump is concerned.
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str);
}

/// An inclusive box, as `BoundingBox` is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundingBox {
    pub min: Pos,
    pub max: Pos,
}

impl BoundingBox {
    /// `BoundingBox.encapsulatingPositions` — `None` for an empty iterable.
    #[must_use]
    pub fn encapsulating(positions: &[Pos]) -> Option<Self> {
        let mut iter = positions.iter().copied();
        let first = iter.next()?;
        let mut out = Self {
            min: first,
            max: first,
        };
        for p in iter {
            out.min = (out.min.0.min(p.0), out.min.1.min(p.1), out.min.2.min(p.2));
            out.max = (out.max.0.max(p.0), out.max.1.max(p.1), out.max.2.max(p.2));
        }
        Some(out)
    }

    #[must_use]
    pub fn is_inside(&self, p: Pos) -> bool {
        p.0 >= self.min.0
            && p.0 <= self.max.0
            && p.1 >= self.min.1
            && p.1 <= self.max.1
            && p.2 >= self.min.2
            && p.2 <= self.max.2
    }
}

/// `Direction.values()` offsets, in vanilla's order.
const DIRECTIONS: [(i32, i32, i32); 6] = [
    (0, -1, 0),
    (0, 1, 0),
    (0, 0, -1),
    (0, 0, 1),
    (-1, 0, 0),
    (1, 0, 0),
];

/// `TreeFeature#updateLeaves`.
///
/// Returns the number of leaves whose distance it rewrote, which is what the
/// tests assert on — the shape itself is vanilla's return value and nothing
/// downstream of `place` reads it.
///
/// # Errors
/// If one of the flood's own [`JavaHashSet`]s refuses — a bucket treeifying
/// with two equal spread hashes. These three inserts used to discard that
/// answer with `let _ =`, which is the one shape of mistake this whole module
/// exists to avoid: a dropped position changes the flood and so changes what
/// gets written, and it does it *quietly*. The caller wraps the tree in an
/// `UndoWorld`, so a refusal here unwrites the tree and gets counted, exactly
/// like a refusal from a placer.
pub fn update_leaves<W: LeafWorld + ?Sized>(
    world: &mut W,
    tags: &crate::tags::BlockTags,
    box_: BoundingBox,
    placed: &Placed,
) -> Result<usize, super::NeedsRules> {
    // The three sets go in as `updateLeaves(level, box, logs, decorations,
    // roots)` — the call site captures them in that order and the lambda
    // forwards them unchanged. Which set is which is the whole algorithm:
    // the **logs** seed the flood, and *decorations and roots* are pre-marked
    // so the flood never walks into them.
    //
    // Getting it the other way round — seeding from the foliage — is
    // self-consistent and silently wrong: every leaf is then at distance 0,
    // never written, and only its neighbours get a value.
    let mut shape: JavaHashSet<Pos> = JavaHashSet::new();
    // `Sets.union(a, b)` iterates the first set whole, then the second's
    // elements not in the first.
    let mut union: Vec<Pos> = placed.decorations.to_vec();
    for root in placed.roots.iter() {
        if !placed.decorations.contains(root) {
            union.push(*root);
        }
    }
    for pos in union {
        if box_.is_inside(pos) {
            shape.insert(pos).map_err(refused)?;
        }
    }

    let mut layers: Vec<JavaHashSet<Pos>> = (0..DECAY_DISTANCE.unsigned_abs() as usize)
        .map(|_| JavaHashSet::new())
        .collect();
    // Distance zero is the wood, not the leaves. The `distance != 0` guard
    // below is why: the seeds are marked and never written, and their leaf
    // neighbours land in set 1.
    for log in placed.logs.iter() {
        layers[0].insert(*log).map_err(refused)?;
    }

    let mut rewritten = 0usize;
    let mut distance = 0i32;
    loop {
        // Find the lowest non-empty set. `distance` is *not* reset each
        // round: it can be pulled back down by a neighbour, and starting the
        // scan from where it was is what makes the flood cheap.
        #[expect(clippy::cast_sign_loss, reason = "the loop guard keeps it in range")]
        while distance < DECAY_DISTANCE && layers[distance as usize].is_empty() {
            distance += 1;
        }
        if distance >= DECAY_DISTANCE {
            break;
        }
        #[expect(clippy::cast_sign_loss, reason = "checked above")]
        let index = distance as usize;
        // `iterator().next()` then `remove()`: the first in HashSet order.
        let Some(pos) = layers[index].iter().next().copied() else {
            break;
        };
        layers[index].remove(&pos);

        if !box_.is_inside(pos) {
            continue;
        }
        if distance != 0 {
            let state = world.block_at(pos.0, pos.1, pos.2);
            let updated = set_distance(&state, distance);
            world.set_block(pos.0, pos.1, pos.2, &updated);
            rewritten += 1;
        }
        shape.insert(pos).map_err(refused)?;

        for (dx, dy, dz) in DIRECTIONS {
            let next_pos = (pos.0 + dx, pos.1 + dy, pos.2 + dz);
            if !box_.is_inside(next_pos) || shape.contains(&next_pos) {
                continue;
            }
            let state = world.block_at(next_pos.0, next_pos.1, next_pos.2);
            let Some(own) = optional_distance_at(tags, &state) else {
                continue;
            };
            let next = own.min(distance + 1);
            if next < DECAY_DISTANCE {
                #[expect(clippy::cast_sign_loss, reason = "checked above")]
                let slot = next as usize;
                layers[slot].insert(next_pos).map_err(refused)?;
                distance = distance.min(next);
            }
        }
    }
    Ok(rewritten)
}

/// A refusal from the flood's sets, labelled so the count says where it came
/// from rather than only that a bucket somewhere gave up.
fn refused(e: crate::java_hash_set::Untracked) -> super::NeedsRules {
    super::NeedsRules(format!("updateLeaves: {}", e.0))
}

/// `LeavesBlock#getOptionalDistanceAt`.
///
/// A block in `#prevents_nearby_leaf_decay` answers **zero** — that is how a
/// log keeps the leaves around it alive without being a leaf itself — and any
/// other block with a `distance` property answers its own value. Everything
/// else has no answer, which stops the flood.
fn optional_distance_at(tags: &crate::tags::BlockTags, state: &str) -> Option<i32> {
    let name = state.split_once('[').map_or(state, |(n, _)| n);
    if tags.contains("minecraft:prevents_nearby_leaf_decay", name) {
        return Some(0);
    }
    let (_, rest) = state.split_once('[')?;
    rest.trim_end_matches(']')
        .split(',')
        .find_map(|pair| pair.strip_prefix("distance="))
        .and_then(|v| v.parse().ok())
}

/// `state.setValue(DISTANCE, distance)`, on the canonical spelling.
fn set_distance(state: &str, distance: i32) -> String {
    let Some((name, rest)) = state.split_once('[') else {
        return state.to_string();
    };
    let pairs: Vec<String> = rest
        .trim_end_matches(']')
        .split(',')
        .map(|pair| {
            if pair.starts_with("distance=") {
                format!("distance={distance}")
            } else {
                pair.to_string()
            }
        })
        .collect();
    format!("{name}[{}]", pairs.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    struct Grid(BTreeMap<Pos, String>);

    impl LeafWorld for Grid {
        fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
            self.0
                .get(&(x, y, z))
                .cloned()
                .unwrap_or_else(|| "minecraft:air".to_string())
        }
        fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
            self.0.insert((x, y, z), state.to_string());
        }
    }

    fn tags() -> crate::tags::BlockTags {
        crate::tags::BlockTags::from_entries(BTreeMap::from([(
            "minecraft:prevents_nearby_leaf_decay".to_string(),
            vec!["minecraft:oak_log".to_string()],
        )]))
        .expect("tags")
    }

    const LEAF: &str = "minecraft:oak_leaves[distance=7,persistent=false,waterlogged=false]";

    /// A log with a row of leaves beside it: distances count outward, and
    /// the log itself is never written.
    #[test]
    fn distance_counts_outward_from_the_wood() {
        let mut world = Grid(BTreeMap::new());
        let mut placed = Placed::default();
        world
            .0
            .insert((0, 0, 0), "minecraft:oak_log[axis=y]".to_string());
        placed.logs.insert((0, 0, 0)).expect("ok");
        for x in 1..=4 {
            world.0.insert((x, 0, 0), LEAF.to_string());
            placed.foliage.insert((x, 0, 0)).expect("ok");
        }

        let box_ = BoundingBox {
            min: (0, 0, 0),
            max: (4, 0, 0),
        };
        let rewritten = update_leaves(&mut world, &tags(), box_, &placed).expect("no tie");
        assert!(rewritten >= 4, "{rewritten} rewritten");
        for (x, want) in [(1, 1), (2, 2), (3, 3), (4, 4)] {
            let state = world.0.get(&(x, 0, 0)).expect("leaf");
            assert!(
                state.contains(&format!("distance={want}")),
                "at x={x}: {state}"
            );
        }
        assert_eq!(
            world.0.get(&(0, 0, 0)).map(String::as_str),
            Some("minecraft:oak_log[axis=y]"),
            "the log is in the shape from the start, so nothing writes to it"
        );
    }

    /// A leaf out of reach keeps the provider's distance and decays.
    #[test]
    fn a_leaf_too_far_from_wood_keeps_its_decay_distance() {
        let mut world = Grid(BTreeMap::new());
        let mut placed = Placed::default();
        world
            .0
            .insert((0, 0, 0), "minecraft:oak_log[axis=y]".to_string());
        placed.logs.insert((0, 0, 0)).expect("ok");
        for x in 1..=9 {
            world.0.insert((x, 0, 0), LEAF.to_string());
            placed.foliage.insert((x, 0, 0)).expect("ok");
        }
        let box_ = BoundingBox {
            min: (0, 0, 0),
            max: (9, 0, 0),
        };
        update_leaves(&mut world, &tags(), box_, &placed).expect("no tie");
        let far = world.0.get(&(8, 0, 0)).expect("leaf");
        assert!(far.contains("distance=7"), "{far}");
    }

    /// The box is what vanilla computes from every position the tree touched.
    #[test]
    fn the_box_encapsulates_every_position() {
        let box_ = BoundingBox::encapsulating(&[(1, 2, 3), (-4, 0, 9), (0, 7, -2)]).expect("some");
        assert_eq!(box_.min, (-4, 0, -2));
        assert_eq!(box_.max, (1, 7, 9));
        assert!(box_.is_inside((0, 3, 0)));
        assert!(!box_.is_inside((2, 3, 0)));
        assert_eq!(BoundingBox::encapsulating(&[]), None);
    }

    /// A block in `#prevents_nearby_leaf_decay` answers zero, which is how a
    /// log keeps the leaves round it alive without being a leaf.
    #[test]
    fn wood_answers_a_distance_of_zero() {
        assert_eq!(
            optional_distance_at(&tags(), "minecraft:oak_log[axis=y]"),
            Some(0)
        );
        assert_eq!(optional_distance_at(&tags(), LEAF), Some(7));
        assert_eq!(optional_distance_at(&tags(), "minecraft:stone"), None);
    }
}
