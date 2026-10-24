//! `com.google.common.collect.TopKSelector.least(k, comparator)` — what
//! `Comparators.least(k, comparator)` collects into.
//!
//! Ported from Guava 33.6.0-jre as the 26.2 server ships it
//! (`oracle-work/vanilla-26.2/libraries/com/google/guava/guava/33.6.0-jre`),
//! under `javap -p -c -constants`: `TopKSelector.offer`, `trim`,
//! `partition`, `swap` and `topK`, and `Comparators.least(int, Comparator)`,
//! which is `Collector.of(() -> TopKSelector.least(k, c), TopKSelector::offer,
//! TopKSelector::combine, TopKSelector::topK, UNORDERED)`. A sequential
//! stream never calls `combine`, so it is not ported.
//!
//! # Why this is not "sort, then take k"
//!
//! The *set* it keeps is the k least, but which of several **equal** elements
//! survive, and in what order, depends on its quickselect: `trim` partitions
//! a `2k` buffer in place around a middle pivot, which is not input-stable,
//! and `topK` then sorts what is left with the stable `Arrays.sort`. So among
//! elements tied at the cut, the ones returned are not necessarily the first
//! ones offered, and a stable sort-and-truncate returns a different list.
//! `PlayerChunkSender.collectChunksToSend` feeds it chunks keyed only by
//! distance, where ties are the common case.

use std::cmp::Ordering;

/// A `TopKSelector` over `T` with the comparator `cmp`.
pub struct TopKSelector<T, C: Fn(&T, &T) -> Ordering> {
    k: usize,
    cmp: C,
    /// `buffer`, `2k` long; `buf.len()` is `bufferSize`.
    buf: Vec<T>,
    /// `threshold`: the greatest element among the `k` least seen so far,
    /// as an index into `buf` would be invalidated by `trim`, so a copy.
    threshold: Option<T>,
}

impl<T: Clone, C: Fn(&T, &T) -> Ordering> TopKSelector<T, C> {
    /// `TopKSelector.least(k, comparator)`.
    pub fn least(k: usize, cmp: C) -> Self {
        Self {
            k,
            cmp,
            buf: Vec::with_capacity(k.saturating_mul(2)),
            threshold: None,
        }
    }

    /// `offer(T)`.
    pub fn offer(&mut self, elem: T) {
        if self.k == 0 {
            return;
        }
        if self.buf.is_empty() {
            self.threshold = Some(elem.clone());
            self.buf.push(elem);
        } else if self.buf.len() < self.k {
            let greater = self
                .threshold
                .as_ref()
                .is_some_and(|t| (self.cmp)(&elem, t) == Ordering::Greater);
            if greater {
                self.threshold = Some(elem.clone());
            }
            self.buf.push(elem);
        } else if self
            .threshold
            .as_ref()
            .is_some_and(|t| (self.cmp)(&elem, t) == Ordering::Less)
        {
            self.buf.push(elem);
            if self.buf.len() == 2 * self.k {
                self.trim();
            }
        }
    }

    /// `trim()` — quickselect the `k` least into `buf[..k]`, then recompute
    /// the threshold from `minThresholdPosition` on.
    fn trim(&mut self) {
        let k = self.k;
        let mut left = 0usize;
        let mut right = 2 * k - 1;
        let mut min_threshold_position = 0usize;
        let mut iterations = 0u32;
        // `IntMath.log2(right - left, CEILING) * 3`.
        let span = (right - left) as u32;
        let max_iterations = (32 - (span - 1).leading_zeros()) * 3;
        while left < right {
            let pivot_index = (left + right + 1) >> 1;
            let pivot_new_index = self.partition(left, right, pivot_index);
            match pivot_new_index.cmp(&k) {
                Ordering::Greater => right = pivot_new_index - 1,
                Ordering::Less => {
                    left = pivot_new_index.max(left + 1);
                    min_threshold_position = pivot_new_index;
                }
                Ordering::Equal => break,
            }
            iterations += 1;
            if iterations >= max_iterations {
                // `Arrays.sort(buffer, left, right + 1, comparator)` — stable.
                let cmp = &self.cmp;
                self.buf[left..=right].sort_by(|a, b| cmp(a, b));
                break;
            }
        }
        self.buf.truncate(k);
        let mut threshold = self.buf[min_threshold_position].clone();
        for i in min_threshold_position + 1..k {
            if (self.cmp)(&self.buf[i], &threshold) == Ordering::Greater {
                threshold = self.buf[i].clone();
            }
        }
        self.threshold = Some(threshold);
    }

    /// `partition(left, right, pivotIndex)` — Lomuto around the value at
    /// `pivot_index`, which is first swapped out to `right`.
    fn partition(&mut self, left: usize, right: usize, pivot_index: usize) -> usize {
        let pivot_value = self.buf[pivot_index].clone();
        self.buf[pivot_index] = self.buf[right].clone();
        let mut pivot_new_index = left;
        for i in left..right {
            if (self.cmp)(&self.buf[i], &pivot_value) == Ordering::Less {
                self.buf.swap(pivot_new_index, i);
                pivot_new_index += 1;
            }
        }
        self.buf[right] = self.buf[pivot_new_index].clone();
        self.buf[pivot_new_index] = pivot_value;
        pivot_new_index
    }

    /// `topK()` — stable-sort what is buffered, keep the first `k`.
    pub fn top_k(mut self) -> Vec<T> {
        let cmp = &self.cmp;
        self.buf.sort_by(|a, b| cmp(a, b));
        self.buf.truncate(self.k);
        self.buf
    }
}

#[cfg(test)]
mod tests {
    use super::TopKSelector;

    fn least_by_key(k: usize, xs: &[(i32, u32)]) -> Vec<(i32, u32)> {
        let mut t = TopKSelector::least(k, |a: &(i32, u32), b: &(i32, u32)| a.0.cmp(&b.0));
        for x in xs {
            t.offer(*x);
        }
        t.top_k()
    }

    #[test]
    fn k_zero_keeps_nothing() {
        assert!(least_by_key(0, &[(1, 0), (0, 1)]).is_empty());
    }

    #[test]
    fn fewer_than_k_is_a_stable_sort() {
        let got = least_by_key(10, &[(3, 0), (1, 1), (3, 2), (1, 3)]);
        assert_eq!(got, vec![(1, 1), (1, 3), (3, 0), (3, 2)]);
    }

    /// An element equal to the threshold is refused once the buffer holds k
    /// (`compare(elem, threshold) < 0`, and equal is not less), and strictly
    /// smaller late arrivals are admitted until the buffer reaches `2k` and
    /// trims. Hand-traced through `trim` for the first case: the pivots are
    /// slots 2, 2 and 3, and the buffer ends `[(0,2), (0,3), ..]`. Agreement
    /// with the jar on tie order is `chunksender`'s golden test, not this.
    #[test]
    fn a_trim_reorders_ties() {
        // 4 elements with key 1, then two with key 0: the 0s are admitted,
        // the buffer reaches 2k = 4 and trims.
        let got = least_by_key(2, &[(1, 0), (1, 1), (0, 2), (0, 3)]);
        assert_eq!(got, vec![(0, 2), (0, 3)]);
        let got = least_by_key(3, &[(1, 0), (1, 1), (1, 2), (0, 3), (0, 4), (1, 5), (0, 6)]);
        assert_eq!(got.iter().filter(|e| e.0 == 0).count(), 3);
    }

    /// The kept set is always the k least by key, whatever the tie order.
    #[test]
    fn the_kept_keys_are_the_k_least() {
        let xs: Vec<(i32, u32)> = (0..500u32)
            .map(|i| (((i.wrapping_mul(2_654_435_761)) >> 24) as i32 % 13, i))
            .collect();
        for k in [1usize, 2, 3, 7, 16, 49, 100] {
            let got = least_by_key(k, &xs);
            let mut keys: Vec<i32> = xs.iter().map(|e| e.0).collect();
            keys.sort_unstable();
            let want: Vec<i32> = keys[..k].to_vec();
            let have: Vec<i32> = got.iter().map(|e| e.0).collect();
            assert_eq!(have, want, "k = {k}");
        }
    }
}
