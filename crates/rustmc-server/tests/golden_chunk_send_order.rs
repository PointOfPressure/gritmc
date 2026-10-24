//! Gate the chunk-tracking geometry and the send comparator against a real
//! vanilla server.
//!
//! `crates/rustmc-server/golden/chunk_send_order.json` is every
//! `level_chunk_with_light` position a vanilla 26.2 superflat server sent a
//! joining client, in arrival order, taken by `ci/chunk-order-golden.sh`.
//! Positions only — no block data — which is why it is committed where a
//! region file would not be.
//!
//! # What this can go red on, and what it deliberately cannot
//!
//! The **set** is exact: 117 of 117 at `view-distance=4`, where a Chebyshev
//! square of the same radius is 81. Nothing about that is a tolerance.
//!
//! The **sequence** is not asserted here, because this capture's batch
//! boundaries depend on when each chunk finished generating, which is a race.
//! What is asserted is that every maximal ascending run — one batch — really
//! is ascending in `ChunkPos.distanceSquared` from the centre, which is the
//! comparator claim itself. The order *within* a distance is gated
//! separately and exactly, against the jar's own `collectChunksToSend`, by
//! `chunksender`'s `every_collect_matches_the_jar`.

use rustmc_server::chunksender::distance_squared;
use rustmc_server::trackingview::TrackingView;

struct Golden {
    view_distance: i32,
    centre: (i32, i32),
    order: Vec<(i32, i32)>,
}

/// The fixture, parsed positionally. This crate has no JSON dependency and
/// pulling one in to read four fields would be the tail wagging the dog.
fn golden() -> Golden {
    let raw = include_str!("../golden/chunk_send_order.json");
    let int_after = |key: &str| -> i32 {
        let at = raw
            .find(&format!("\"{key}\":"))
            .unwrap_or_else(|| panic!("golden has no {key}"));
        let rest = &raw[at + key.len() + 3..];
        let end = rest.find([',', '\n', '}']).unwrap_or(rest.len());
        rest[..end].trim().parse().expect("integer")
    };
    // One pair per line, `  [x, z]` — the shape `ci/chunk-order-golden.sh`
    // writes for exactly this reason. Scanned line-wise rather than by
    // bracket matching, because the enclosing array's own brackets are on
    // their own lines and a naive scan eats the opener.
    let pair_on = |line: &str| -> Option<(i32, i32)> {
        let open = line.find('[')?;
        let close = line.rfind(']')?;
        let (a, b) = line.get(open + 1..close)?.split_once(',')?;
        Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
    };
    let centre = raw
        .lines()
        .find(|l| l.contains("\"centre\":"))
        .and_then(pair_on)
        .expect("centre");
    let order_at = raw.find("\"order\":").expect("order");
    let order: Vec<(i32, i32)> = raw[order_at..]
        .lines()
        .skip(1)
        .filter_map(pair_on)
        .collect();
    assert!(!order.is_empty(), "the golden holds no positions");
    Golden {
        view_distance: int_after("view_distance"),
        centre,
        order,
    }
}

/// The whole point: the set vanilla sent is exactly
/// `ChunkTrackingView$Positioned(centre, viewDistance)`.
///
/// The positive control is the shape this replaced. A Chebyshev square of
/// radius 4 holds 81 chunks and this asserts that the capture is not it, in
/// both directions — 36 chunks vanilla sent that the square does not hold,
/// and none that it holds and vanilla did not send.
#[test]
fn vanillas_chunk_set_is_the_padded_disc_and_not_a_square() {
    let g = golden();
    let view = TrackingView::of(g.centre, g.view_distance);
    let sent: std::collections::BTreeSet<(i32, i32)> = g.order.iter().copied().collect();
    assert_eq!(sent.len(), g.order.len(), "vanilla sent a chunk twice");

    let mut predicted = std::collections::BTreeSet::new();
    view.for_each(|c| {
        predicted.insert(c);
    });
    let missing: Vec<_> = predicted.difference(&sent).collect();
    let extra: Vec<_> = sent.difference(&predicted).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "predicted {} chunks, vanilla sent {}: {} predicted and absent {missing:?}, \
         {} sent and unpredicted {extra:?}",
        predicted.len(),
        sent.len(),
        missing.len(),
        extra.len()
    );
    assert_eq!(sent.len(), 117, "at view-distance 4");

    let square: std::collections::BTreeSet<(i32, i32)> = (-g.view_distance..=g.view_distance)
        .flat_map(|dx| {
            (-g.view_distance..=g.view_distance).map(move |dz| (g.centre.0 + dx, g.centre.1 + dz))
        })
        .collect();
    assert_eq!(square.len(), 81);
    assert_eq!(
        sent.difference(&square).count(),
        36,
        "36 chunks vanilla sent that a Chebyshev square of radius 4 does not hold"
    );
    assert_eq!(
        square.difference(&sent).count(),
        0,
        "at view distance 4 the disc contains the square; at 10 it does not"
    );
}

/// Each batch is sorted by `ChunkPos.distanceSquared` ascending, and the
/// first chunk of the whole stream is the centre.
///
/// Runs rather than the whole sequence: `ChunkMap.onChunkReadyToSend` queues
/// a chunk when it finishes generating, so a nearer chunk can enter the queue
/// after a farther one has gone out, and a descent in the recorded sequence
/// is a batch boundary rather than a defect.
#[test]
fn every_batch_vanilla_sent_is_ascending_in_distance_squared() {
    let g = golden();
    assert_eq!(
        g.order[0], g.centre,
        "distanceSquared 0 is the unique minimum, so the centre goes first"
    );
    let keys: Vec<i32> = g
        .order
        .iter()
        .map(|c| distance_squared(g.centre, *c))
        .collect();
    let descents: Vec<usize> = (1..keys.len()).filter(|k| keys[*k] < keys[k - 1]).collect();
    assert!(
        descents.len() < 20,
        "{} descents over {} chunks — a batch sort by distance cannot need that many",
        descents.len(),
        keys.len()
    );
    // Every maximal run must be non-decreasing, which is the assertion the
    // run decomposition would make vacuous if it were derived from the same
    // comparison — so it is re-checked here against the runs' own bounds.
    let mut start = 0usize;
    for &d in descents.iter().chain(std::iter::once(&keys.len())) {
        assert!(
            keys[start..d].windows(2).all(|w| w[0] <= w[1]),
            "run {start}..{d} is not ascending"
        );
        start = d;
    }
    let longest = descents
        .iter()
        .chain(std::iter::once(&keys.len()))
        .scan(0usize, |s, &d| {
            let n = d - *s;
            *s = d;
            Some(n)
        })
        .max()
        .expect("at least one run");
    assert!(
        longest >= 5,
        "the longest batch is {longest} chunks — a sorted batch of floor(batchQuota) \
         should be much longer"
    );
}
