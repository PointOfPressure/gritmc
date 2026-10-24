//! One work-splitter, so a stage that is embarrassingly parallel does not
//! have to write the pool out again.
//!
//! # Why this exists
//!
//! Three stages in `generate.rs` had already grown their own copy of the same
//! twenty lines — a `Mutex` over an iterator or an `AtomicUsize` index, a
//! `std::thread::scope`, a `Vec` of slots, and a `threads <= 1` arm that runs
//! the identical closure on the calling thread. A fourth copy is where a
//! transcription error lives, and the error a copy of this makes is not a
//! crash: it is a result assembled in worker-completion order rather than in
//! the caller's order, which is a different world that no type check and no
//! value test can see.
//!
//! So the ordering guarantee is the point of the abstraction, not the thread
//! pool. [`map_indexed`] returns a `Vec` indexed by **input** position,
//! always, whatever order the workers finished in.
//!
//! # What it deliberately does not do
//!
//! It does not hand the closure a worker id, and it does not let the closure
//! see how many workers there are. A stage whose result depends on either has
//! reintroduced exactly the dependency this project spent §31 and §32
//! removing, and it should not be able to express it.
//!
//! `RUSTMC_WORKERS=1` runs everything on the calling thread through the same
//! closure, which is the arm every byte-for-byte comparison in `docs/BENCH.md`
//! is taken against.

/// Run `f` for every index in `0..len`, in parallel, and return the results
/// **in index order**.
///
/// The closure must be `Sync` because every worker shares one reference to
/// it, and its output `Send` because it crosses back. It may not be `FnMut`:
/// a stage that wants per-worker scratch builds it inside the closure, which
/// is what the biome stage already does with `ChunkClimate`.
pub fn map_indexed<T, F>(len: usize, f: F) -> Vec<T>
where
    F: Fn(usize) -> T + Sync,
    T: Send,
{
    if len == 0 {
        return Vec::new();
    }
    let threads = rustmc_worldgen::pipeline::worker_threads().min(len);
    if threads <= 1 {
        return (0..len).map(f).collect();
    }
    // One slot per input, filled by whichever worker takes that index. The
    // `Mutex` is per slot rather than over the whole vector, so workers never
    // wait on each other to store a result; the index they take is the one
    // shared point, and it is a single atomic add.
    let slots: Vec<std::sync::Mutex<Option<T>>> =
        (0..len).map(|_| std::sync::Mutex::new(None)).collect();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let f = &f;
    let next_ref = &next;
    let slots_ref = &slots;
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(move || loop {
                let i = next_ref.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if i >= len {
                    break;
                }
                let out = f(i);
                *slots_ref[i].lock().expect("a work slot") = Some(out);
            });
        }
    });
    slots
        .into_iter()
        .map(|slot| {
            slot.into_inner()
                .expect("a work slot")
                .expect("every index was taken exactly once")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ordering guarantee, against a closure whose completion order is
    /// deliberately the reverse of its index order: index 0 sleeps longest,
    /// so a pool that pushed results as they finished would return them
    /// backwards.
    #[test]
    fn results_come_back_in_index_order_whatever_order_they_finished_in() {
        let out = map_indexed(24, |i| {
            std::thread::sleep(std::time::Duration::from_millis((24 - i) as u64 * 2));
            i * 7
        });
        assert_eq!(out.len(), 24);
        for (i, v) in out.iter().enumerate() {
            assert_eq!(*v, i * 7, "slot {i} holds another index's result");
        }
    }

    /// Every index is run exactly once — not skipped, and not run twice by two
    /// workers racing on the counter. A closure that counted its own calls in
    /// an atomic would catch a double-run; the `expect` in the collect catches
    /// a skip, so both failures are turned into a panic rather than a plausible
    /// answer.
    #[test]
    fn every_index_runs_exactly_once() {
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let seen: Vec<usize> = map_indexed(500, |i| {
            calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            i
        });
        assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 500);
        assert_eq!(seen, (0..500).collect::<Vec<_>>());
    }

    /// An empty input needs no pool and no panic.
    #[test]
    fn an_empty_input_is_an_empty_output() {
        let out: Vec<usize> = map_indexed(0, |i| i);
        assert!(out.is_empty());
    }

    /// The sequential arm and the parallel arm agree. This is the property
    /// every byte-identical proof in `docs/BENCH.md` rests on, asserted here
    /// on a closure whose result is order-sensitive if the split is wrong.
    #[test]
    fn one_worker_and_many_agree() {
        let f = |i: usize| (i * 31 + 7) % 1013;
        let many = map_indexed(2000, f);
        let one: Vec<usize> = (0..2000).map(f).collect();
        assert_eq!(many, one);
    }
}
