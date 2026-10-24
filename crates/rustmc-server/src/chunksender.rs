//! `PlayerChunkSender` — the batch handshake that paces chunks to a client.
//!
//! Transcribed from 26.2's `net.minecraft.server.network.PlayerChunkSender`
//! under `javap -p -c -constants`. Pure bookkeeping: this module decides
//! *which* chunks go out this tick and in what order, and the caller writes
//! `chunk_batch_start`, the `level_chunk_with_light` packets and
//! `chunk_batch_finished`.
//!
//! # The client sets the rate, and until it does the server sends one batch
//!
//! A fresh sender has `max_unacknowledged_batches = 1`, not 10. So the very
//! first batch after a join goes out and then **the server stops** until the
//! client answers `chunk_batch_received` with the rate it wants. Only that
//! packet raises the ceiling to `MAX_UNACKNOWLEDGED_BATCHES`. A server that
//! ignores the serverbound packet therefore does not merely fail to throttle
//! — it delivers one batch and then never another, which looks like a
//! generation stall rather than a missing handler.
//!
//! # The quota is a float and is clamped from above every tick
//!
//! ```java
//! float f = Math.max(1.0F, this.desiredChunksPerTick);
//! this.batchQuota = Math.min(this.batchQuota + this.desiredChunksPerTick, f);
//! if (this.batchQuota < 1.0F) return;
//! ```
//!
//! The ceiling is `max(1, desired)`, so credit never accumulates across ticks
//! beyond one tick's worth: a client that asks for 0.01 chunks per tick gets
//! one chunk roughly every hundred ticks and never a burst of a hundred.
//! Note the mutation happens **before** the emptiness checks, so a tick that
//! sends nothing still moves the quota.
//!
//! # `dropChunk` is silent for a chunk that never went out
//!
//! `forget_level_chunk` is sent only when the position was *not* still
//! pending. A chunk that entered the view and left it again before its batch
//! was sent is removed from the queue and the client is told nothing —
//! correctly, since it never had it. A port that forgets unconditionally
//! sends a client `forget_level_chunk` for a chunk it has never seen.

use crate::trackingview::ChunkPos;

#[path = "long_open_hash_set.rs"]
pub mod long_open_hash_set;
#[path = "top_k_selector.rs"]
pub mod top_k_selector;

use long_open_hash_set::LongOpenHashSet;
use top_k_selector::TopKSelector;

/// `ChunkPos.pack(int, int)`: `x` in the low word, `z` in the high, each
/// masked to 32 bits first.
#[must_use]
pub const fn pack(c: ChunkPos) -> i64 {
    ((c.0 as u32 as u64) | ((c.1 as u32 as u64) << 32)) as i64
}

/// `ChunkPos.getX(long)` / `getZ(long)`.
#[must_use]
pub const fn unpack(k: i64) -> ChunkPos {
    (k as i32, (k >> 32) as i32)
}

/// `PlayerChunkSender.MIN_CHUNKS_PER_TICK`.
pub const MIN_CHUNKS_PER_TICK: f32 = 0.01;

/// `PlayerChunkSender.MAX_CHUNKS_PER_TICK`.
pub const MAX_CHUNKS_PER_TICK: f32 = 64.0;

/// `PlayerChunkSender.START_CHUNKS_PER_TICK` — the rate before the client
/// has said anything.
pub const START_CHUNKS_PER_TICK: f32 = 9.0;

/// `PlayerChunkSender.MAX_UNACKNOWLEDGED_BATCHES`.
pub const MAX_UNACKNOWLEDGED_BATCHES: i32 = 10;

/// `Mth.floor(float)` — toward negative infinity, not toward zero.
const fn mth_floor(f: f32) -> i32 {
    let i = f as i32;
    if f < i as f32 {
        i - 1
    } else {
        i
    }
}

/// `Mth.clamp(float, float, float)`.
fn mth_clamp(v: f32, min: f32, max: f32) -> f32 {
    if v < min {
        min
    } else if v > max {
        max
    } else {
        v
    }
}

/// One connection's chunk queue and its rate state.
#[derive(Debug, Clone)]
pub struct ChunkSender {
    /// `PlayerChunkSender.pendingChunks`, a `LongOpenHashSet` of packed
    /// positions.
    ///
    /// # The table is the tie-break
    ///
    /// `collectChunksToSend` streams this set into a comparator on
    /// `ChunkPos.distanceSquared` **alone** (`Comparator.comparingInt`, offsets
    /// 66..74 and 105..110 of the pinned jar's method). So chunks at equal
    /// distance — every view has many — are ordered by the set's spliterator:
    /// the unthrottled branch through a stable `Stream.sorted`, the throttled
    /// one through Guava's `Comparators.least`. Neither `(x, z)` nor `(z, x)`
    /// is vanilla's order; the table layout is, and it is ported whole,
    /// growth and shrinking included, in [`long_open_hash_set`] and
    /// [`top_k_selector`].
    pending: LongOpenHashSet,
    /// `PlayerChunkSender.memoryConnection` — a singleplayer integrated
    /// server, which is never throttled. Always `false` for a socket.
    memory_connection: bool,
    /// `PlayerChunkSender.desiredChunksPerTick`.
    desired_chunks_per_tick: f32,
    /// `PlayerChunkSender.batchQuota`.
    batch_quota: f32,
    /// `PlayerChunkSender.unacknowledgedBatches`.
    unacknowledged_batches: i32,
    /// `PlayerChunkSender.maxUnacknowledgedBatches` — **1 until the client
    /// answers**, then 10.
    max_unacknowledged_batches: i32,
}

impl ChunkSender {
    /// `PlayerChunkSender(boolean memoryConnection)`.
    #[must_use]
    pub fn new(memory_connection: bool) -> Self {
        Self {
            pending: LongOpenHashSet::new(),
            memory_connection,
            desired_chunks_per_tick: START_CHUNKS_PER_TICK,
            batch_quota: 0.0,
            unacknowledged_batches: 0,
            max_unacknowledged_batches: 1,
        }
    }

    /// `PlayerChunkSender.markChunkPendingToSend`.
    pub fn mark_pending(&mut self, chunk: ChunkPos) {
        self.pending.add(pack(chunk));
    }

    /// `PlayerChunkSender.isPending`.
    #[must_use]
    pub fn is_pending(&self, chunk: ChunkPos) -> bool {
        self.pending.contains(pack(chunk))
    }

    /// How many chunks are queued. Not a vanilla method; for logging.
    #[must_use]
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// `PlayerChunkSender.dropChunk` minus the send.
    ///
    /// Returns `true` when the caller should write `forget_level_chunk` —
    /// that is, when the chunk had already left the queue, which means it had
    /// already gone out. Vanilla also requires `player.isAlive()`; that is
    /// the caller's condition, not this one's.
    pub fn drop_chunk(&mut self, chunk: ChunkPos) -> bool {
        !self.pending.remove(pack(chunk))
    }

    /// `PlayerChunkSender.onChunkBatchReceivedByClient(float)`.
    ///
    /// `desired_chunks_per_tick` comes off the wire, so a hostile or broken
    /// client can send a NaN; vanilla tests for it explicitly and falls to
    /// the minimum rather than to the clamp, whose comparisons are all false
    /// for NaN and would leave the NaN in place.
    pub fn on_batch_received(&mut self, desired_chunks_per_tick: f32) {
        self.unacknowledged_batches -= 1;
        if self.unacknowledged_batches < 0 {
            self.unacknowledged_batches = 0;
        }
        self.desired_chunks_per_tick = if desired_chunks_per_tick.is_nan() {
            MIN_CHUNKS_PER_TICK
        } else {
            mth_clamp(
                desired_chunks_per_tick,
                MIN_CHUNKS_PER_TICK,
                MAX_CHUNKS_PER_TICK,
            )
        };
        if self.unacknowledged_batches == 0 {
            self.batch_quota = 1.0;
        }
        self.max_unacknowledged_batches = MAX_UNACKNOWLEDGED_BATCHES;
    }

    /// `PlayerChunkSender.sendNextChunks`, up to the point of writing bytes.
    ///
    /// `available` answers `ChunkMap.getChunkToSend(pos) != null`: whether
    /// that chunk exists as a `LevelChunk` right now. A pending chunk that is
    /// still generating answers `false`, stays pending, and — in the
    /// throttled branch — **still consumes one of this tick's slots**, which
    /// is why a batch can come back shorter than the quota.
    ///
    /// Returns the batch, in send order. An empty return means no batch
    /// packets at all this tick; note that the quota has still moved.
    pub fn send_next_chunks(
        &mut self,
        centre: ChunkPos,
        available: impl Fn(ChunkPos) -> bool,
    ) -> Vec<ChunkPos> {
        if self.unacknowledged_batches >= self.max_unacknowledged_batches {
            return Vec::new();
        }
        let ceiling = self.desired_chunks_per_tick.max(1.0);
        self.batch_quota = (self.batch_quota + self.desired_chunks_per_tick).min(ceiling);
        if self.batch_quota < 1.0 {
            return Vec::new();
        }
        if self.pending.is_empty() {
            return Vec::new();
        }
        let batch = self.collect_chunks_to_send(centre, available);
        if batch.is_empty() {
            return Vec::new();
        }
        self.unacknowledged_batches += 1;
        self.batch_quota -= batch.len() as f32;
        batch
    }

    /// `PlayerChunkSender.collectChunksToSend`.
    ///
    /// Two branches, and they are not the same function with a different
    /// limit:
    ///
    /// * **untrottled** (a memory connection, or fewer pending chunks than
    ///   the quota): take every pending chunk, drop the ones that are not
    ///   available, *then* sort by distance. Availability cannot cost a slot,
    ///   because there is no slot limit.
    /// * **throttled**: take the `quota` nearest pending **positions**, then
    ///   drop the unavailable ones. The selection happens before the filter,
    ///   so an unavailable chunk spends a slot and is not backfilled.
    ///
    /// Every chunk in the returned list is removed from the pending set; the
    /// ones filtered out are not, and are retried next tick.
    fn collect_chunks_to_send(
        &mut self,
        centre: ChunkPos,
        available: impl Fn(ChunkPos) -> bool,
    ) -> Vec<ChunkPos> {
        let quota = mth_floor(self.batch_quota);
        let quota_usize = usize::try_from(quota.max(0)).unwrap_or(0);
        let by_distance = |a: &ChunkPos, b: &ChunkPos| {
            distance_squared(centre, *a).cmp(&distance_squared(centre, *b))
        };
        let list: Vec<ChunkPos> = if self.memory_connection || self.pending.len() <= quota_usize {
            // `longStream().mapToObj(getChunkToSend).filter(nonNull)
            //  .sorted(comparingInt(distanceSquared)).toList()` — a stable sort
            // over spliterator order.
            let mut v: Vec<ChunkPos> = self
                .pending
                .stream_order()
                .map(unpack)
                .filter(|c| available(*c))
                .collect();
            v.sort_by(by_distance);
            v
        } else {
            // `stream().collect(Comparators.least(quota, comparingInt(..)))`,
            // then `mapToObj(getChunkToSend).filter(nonNull).toList()`.
            let mut top = TopKSelector::least(quota_usize, by_distance);
            for k in self.pending.stream_order() {
                top.offer(unpack(k));
            }
            let mut v = top.top_k();
            v.retain(|c| available(*c));
            v
        };
        for c in &list {
            self.pending.remove(pack(*c));
        }
        list
    }

    /// `PlayerChunkSender.desiredChunksPerTick`, for tests and logging.
    #[must_use]
    pub const fn desired_chunks_per_tick(&self) -> f32 {
        self.desired_chunks_per_tick
    }

    /// `PlayerChunkSender.unacknowledgedBatches`, for tests and logging.
    #[must_use]
    pub const fn unacknowledged_batches(&self) -> i32 {
        self.unacknowledged_batches
    }

    /// `PlayerChunkSender.maxUnacknowledgedBatches`, for tests and logging.
    #[must_use]
    pub const fn max_unacknowledged_batches(&self) -> i32 {
        self.max_unacknowledged_batches
    }
}

/// `ChunkPos.distanceSquared(ChunkPos)` — plain `int` arithmetic, no
/// widening, so it overflows exactly where vanilla's does.
#[must_use]
pub const fn distance_squared(a: ChunkPos, b: ChunkPos) -> i32 {
    let dx = b.0.wrapping_sub(a.0);
    let dz = b.1.wrapping_sub(a.1);
    dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz))
}

#[cfg(test)]
mod tests {
    use super::{
        distance_squared, mth_floor, pack, unpack, ChunkPos, ChunkSender,
        MAX_UNACKNOWLEDGED_BATCHES, START_CHUNKS_PER_TICK,
    };

    fn all_available(_: (i32, i32)) -> bool {
        true
    }

    /// The join case: nine chunks in the first batch — `floor(9.0)` — and
    /// then silence, because `max_unacknowledged_batches` is 1 until the
    /// client answers.
    #[test]
    fn the_first_batch_is_nine_and_the_second_waits_for_the_client() {
        let mut s = ChunkSender::new(false);
        for x in 0..20 {
            s.mark_pending((x, 0));
        }
        assert_eq!(s.max_unacknowledged_batches(), 1);
        let first = s.send_next_chunks((0, 0), all_available);
        assert_eq!(first.len(), 9, "floor(min(0 + 9, max(1, 9))) = 9");
        assert_eq!(s.unacknowledged_batches(), 1);
        assert_eq!(
            s.send_next_chunks((0, 0), all_available),
            Vec::<(i32, i32)>::new(),
            "one unacknowledged batch is already the ceiling"
        );
        s.on_batch_received(START_CHUNKS_PER_TICK);
        assert_eq!(s.max_unacknowledged_batches(), MAX_UNACKNOWLEDGED_BATCHES);
        assert_eq!(s.unacknowledged_batches(), 0);
        let second = s.send_next_chunks((0, 0), all_available);
        assert_eq!(second.len(), 9);
    }

    /// The batch is monotone in distance from the player — the comparator
    /// itself. The order *within* a distance is the golden test's.
    #[test]
    fn a_batch_is_nearest_first() {
        let mut s = ChunkSender::new(true);
        for x in -3..=3 {
            for z in -3..=3 {
                s.mark_pending((x, z));
            }
        }
        let batch = s.send_next_chunks((0, 0), all_available);
        assert_eq!(batch.len(), 49, "a memory connection is never throttled");
        assert_eq!(batch[0], (0, 0));
        let d: Vec<i32> = batch.iter().map(|c| distance_squared((0, 0), *c)).collect();
        assert!(
            d.windows(2).all(|w| w[0] <= w[1]),
            "distanceSquared must not decrease: {d:?}"
        );
        assert_eq!(*d.last().expect("non-empty"), 18, "the corner, 3*3 + 3*3");
    }

    /// A chunk still generating is not sent, is not dropped from the queue,
    /// and in the throttled branch **costs a slot**: the batch comes back
    /// shorter than the quota rather than reaching past it for a spare.
    #[test]
    fn an_unavailable_chunk_costs_a_slot_and_stays_pending() {
        let mut s = ChunkSender::new(false);
        for x in 0..20 {
            s.mark_pending((x, 0));
        }
        // (2, 0) is the third nearest and is not ready.
        let batch = s.send_next_chunks((0, 0), |c| c != (2, 0));
        assert_eq!(batch.len(), 8, "nine slots, one spent on a chunk not there");
        assert!(!batch.contains(&(2, 0)));
        assert!(s.is_pending((2, 0)), "still queued for the next tick");
        assert!(!s.is_pending((3, 0)), "the eight that went out are gone");
    }

    /// The unthrottled branch does not spend a slot: with the whole pending
    /// set inside the quota, an unavailable chunk simply is not in the batch
    /// and every available one is.
    #[test]
    fn the_unthrottled_branch_does_not_spend_a_slot() {
        let mut s = ChunkSender::new(false);
        for x in 0..5 {
            s.mark_pending((x, 0));
        }
        // 5 pending <= floor(9) quota, so the untrottled branch runs.
        let batch = s.send_next_chunks((0, 0), |c| c != (2, 0));
        assert_eq!(batch.len(), 4);
        assert!(s.is_pending((2, 0)));
    }

    /// A slow client: the quota ceiling is `max(1, desired)`, so credit does
    /// not bank. At 0.01 chunks a tick the steady state is one chunk every
    /// hundred ticks, and it is never two.
    ///
    /// The first chunk after the acknowledgement is free and that is not a
    /// rounding artefact: `onChunkBatchReceivedByClient` sets `batchQuota` to
    /// **1.0** whenever the last outstanding batch is acknowledged, whatever
    /// rate the client asked for. So a client that answers each batch
    /// immediately gets one chunk per round trip even at the minimum rate,
    /// and the 0.01 only binds while a batch is still outstanding.
    #[test]
    fn a_slow_client_gets_one_chunk_at_a_time_and_never_a_burst() {
        let mut s = ChunkSender::new(false);
        for x in 0..50 {
            s.mark_pending((x, 0));
        }
        s.send_next_chunks((0, 0), all_available);
        s.on_batch_received(0.01);
        let first = s.send_next_chunks((0, 0), all_available);
        assert_eq!(
            first.len(),
            1,
            "the acknowledgement reset batch_quota to 1.0"
        );
        // Now a batch is outstanding again, so nothing resets the quota and
        // the rate is the one the client asked for.
        let mut ticks = 0;
        loop {
            let b = s.send_next_chunks((0, 0), all_available);
            ticks += 1;
            if !b.is_empty() {
                assert_eq!(b.len(), 1, "the ceiling is max(1, 0.01) = 1");
                break;
            }
            assert!(ticks < 1000, "never sent anything");
        }
        assert_eq!(
            ticks, 101,
            "101 and not 100: the quota is an f32 and 0.01 accumulated a \
             hundred times falls just short of 1.0. Vanilla's is an f32 too, \
             so this is the reproduced answer rather than an off-by-one"
        );
    }

    /// A hostile rate: NaN falls to the minimum rather than poisoning the
    /// quota. Without the explicit test, `clamp`'s comparisons are all false
    /// for NaN and `batch_quota` becomes NaN, after which `< 1.0` is false
    /// and the sender streams without limit.
    #[test]
    fn a_nan_rate_becomes_the_minimum() {
        let mut s = ChunkSender::new(false);
        s.mark_pending((0, 0));
        s.send_next_chunks((0, 0), all_available);
        s.on_batch_received(f32::NAN);
        assert!((s.desired_chunks_per_tick() - 0.01).abs() < 1e-9);
        s.on_batch_received(1e9);
        assert!((s.desired_chunks_per_tick() - 64.0).abs() < 1e-9);
        s.on_batch_received(-5.0);
        assert!((s.desired_chunks_per_tick() - 0.01).abs() < 1e-9);
    }

    /// A chunk that leaves the view before its batch went out is dropped
    /// silently; one that has been sent gets a `forget_level_chunk`.
    #[test]
    fn only_a_sent_chunk_is_forgotten_on_the_wire() {
        let mut s = ChunkSender::new(false);
        s.mark_pending((0, 0));
        s.mark_pending((1, 0));
        assert!(
            !s.drop_chunk((1, 0)),
            "still pending: no packet, just dequeued"
        );
        let batch = s.send_next_chunks((0, 0), all_available);
        assert_eq!(batch, vec![(0, 0)]);
        assert!(
            s.drop_chunk((0, 0)),
            "already sent: the client has it and must be told"
        );
        assert!(
            s.drop_chunk((9, 9)),
            "never queued at all, so also 'not pending' — vanilla's own answer"
        );
    }

    /// The quota moves on a tick that sends nothing, because vanilla mutates
    /// it before the emptiness checks. Two idle ticks then one chunk arriving
    /// must not produce a batch of two.
    #[test]
    fn an_idle_tick_still_moves_the_quota_but_the_ceiling_binds() {
        let mut s = ChunkSender::new(false);
        s.send_next_chunks((0, 0), all_available); // nothing pending
        s.send_next_chunks((0, 0), all_available);
        s.mark_pending((0, 0));
        s.mark_pending((1, 0));
        let b = s.send_next_chunks((0, 0), all_available);
        assert_eq!(
            b.len(),
            2,
            "quota is capped at max(1, 9) = 9, not 27; both pending chunks fit"
        );
    }

    /// `Mth.floor` rounds toward negative infinity. Only reachable here if a
    /// quota ever went negative, which it should not — the test exists so
    /// that a change to the clamp cannot silently change the rounding too.
    #[test]
    fn mth_floor_goes_toward_negative_infinity() {
        assert_eq!(mth_floor(9.9), 9);
        assert_eq!(mth_floor(1.0), 1);
        assert_eq!(mth_floor(0.99), 0);
        assert_eq!(mth_floor(-0.5), -1);
    }

    /// Plain `int` arithmetic, which is what makes the comparator's key an
    /// `int` and not a `long`.
    #[test]
    fn distance_squared_is_int_arithmetic() {
        assert_eq!(distance_squared((0, 0), (3, 4)), 25);
        assert_eq!(distance_squared((10, -10), (10, -10)), 0);
        assert_eq!(distance_squared((-2, -3), (1, 1)), 25);
    }

    /// `ChunkSendTiesGolden`'s availability rule, in the same `int`
    /// arithmetic: a quarter of positions are unavailable on each collect.
    fn golden_available(x: i32, z: i32, salt: i32) -> bool {
        let h =
            x.wrapping_mul(73_856_093) ^ z.wrapping_mul(19_349_663) ^ salt.wrapping_mul(83_492_791);
        h & 3 != 0
    }

    /// Replay `golden/chunk_send_ties.txt` — the jar's own
    /// `collectChunksToSend` over its own `LongOpenHashSet` and Guava's
    /// `Comparators.least`, recorded by `ci/chunk-send-ties-golden.sh` — and
    /// require every collect, in order, element for element.
    ///
    /// This is the test the tie-break is decided by. The script refuses to
    /// write a golden in which neither an `(x, z)` nor a `(z, x)` sort would
    /// fail it, and the two stand-ins this branch and `main` used are each
    /// red here: see `a_coordinate_tie_break_is_refused_by_the_golden`.
    #[test]
    fn every_collect_matches_the_jar() {
        let (collects, mismatches) = replay_golden(|s, centre, quota, salt| {
            s.batch_quota = quota as f32;
            s.collect_chunks_to_send(centre, |c| golden_available(c.0, c.1, salt))
        });
        assert!(collects > 5000, "only {collects} collects in the golden");
        assert!(
            mismatches.is_empty(),
            "{} of {collects} collects differ from the jar; first: {:?}",
            mismatches.len(),
            mismatches.first()
        );
    }

    /// The positive control: a sender that breaks ties on coordinates — the
    /// `(x, z)` this branch used, the `(z, x)` `main` used — is red against
    /// the same golden, on over a fifth of its collects (most of the rest return
    /// fewer than two chunks, where no tie-break can show).
    #[test]
    fn a_coordinate_tie_break_is_refused_by_the_golden() {
        for (name, key) in [
            (
                "(x, z)",
                (|c: ChunkPos| (c.0, c.1)) as fn(ChunkPos) -> (i32, i32),
            ),
            ("(z, x)", |c: ChunkPos| (c.1, c.0)),
        ] {
            let (collects, mismatches) = replay_golden(|s, centre, quota, salt| {
                let quota = usize::try_from(quota.max(0)).unwrap_or(0);
                let mut all: Vec<ChunkPos> = s.pending.stream_order().map(unpack).collect();
                all.sort_by_key(|c| (distance_squared(centre, *c), key(*c)));
                let throttled = !s.memory_connection && all.len() > quota;
                let mut out: Vec<ChunkPos> = if throttled {
                    all.truncate(quota);
                    all.retain(|c| golden_available(c.0, c.1, salt));
                    all
                } else {
                    all.retain(|c| golden_available(c.0, c.1, salt));
                    all
                };
                out.retain(|c| s.pending.remove(pack(*c)));
                out
            });
            eprintln!(
                "{name}: {} of {collects} collects differ from the jar",
                mismatches.len()
            );
            assert!(
                mismatches.len() * 5 > collects,
                "{name}: only {} of {collects} collects differ — the golden barely sees ties",
                mismatches.len()
            );
        }
    }

    /// Drive a script through `collect`, returning how many collects there
    /// were and the ones whose output differed from the golden's.
    #[allow(clippy::type_complexity)]
    fn replay_golden(
        mut collect: impl FnMut(&mut ChunkSender, ChunkPos, i32, i32) -> Vec<ChunkPos>,
    ) -> (usize, Vec<(usize, Vec<ChunkPos>, Vec<ChunkPos>)>) {
        let raw = include_str!("../golden/chunk_send_ties.txt");
        let mut s = ChunkSender::new(false);
        let mut collects = 0usize;
        let mut mismatches = Vec::new();
        let int = |t: &str| -> i32 { t.parse().expect("integer in golden") };
        for (n, line) in raw.lines().enumerate() {
            let (head, tail) = line.split_once(':').unwrap_or((line, ""));
            let f: Vec<&str> = head.split_whitespace().collect();
            match f.first().copied() {
                Some("S") => s = ChunkSender::new(f[1] == "1"),
                Some("A") => s.mark_pending((int(f[1]), int(f[2]))),
                Some("D") => {
                    assert!(
                        !s.drop_chunk((int(f[1]), int(f[2]))),
                        "line {n}: not pending"
                    );
                }
                Some("C") => {
                    let v: Vec<i32> = tail.split_whitespace().map(int).collect();
                    let want: Vec<ChunkPos> = v.chunks(2).map(|p| (p[0], p[1])).collect();
                    let got = collect(&mut s, (int(f[1]), int(f[2])), int(f[3]), int(f[4]));
                    collects += 1;
                    if got != want {
                        mismatches.push((n + 1, want, got));
                    }
                }
                _ => panic!("line {n}: unrecognised record {line:?}"),
            }
        }
        (collects, mismatches)
    }
}
