//! `net.minecraft.world.entity.ai.behavior.AcquirePoi` and its
//! `JitteredLinearRetry`, from the 26.2 jar. `SCAN_RANGE = 48`.
//!
//! `create(poiType, entryMemory, acquireMemory, onlyIfAdult, entityEventId,
//! predicate)` (0..64) builds one `BehaviorBuilder` one-shot over
//! `group(absent(acquireMemory))` with the trigger below, and — when
//! `entryMemory != acquireMemory` (`if_acmpne` at 49) — wraps it in a
//! second one-shot over `group(absent(entryMemory))` whose body is the
//! first's `trigger`. The shorter overloads fill `predicate` with
//! `(level, pos) -> true` and `entryMemory = acquireMemory`.
//!
//! ```text
//! lambda$create$3 (0..300), the trigger:
//!   if onlyIfAdult && mob.isBaby(): return false
//!   random = level.getRandom()                                          <- LEVEL random
//!   if nextScheduledStart == 0: nextScheduledStart = level.getGameTime() + random.nextInt(20); return false
//!   if level.getGameTime() < nextScheduledStart: return false
//!   nextScheduledStart = time + 20 + random.nextInt(20)                 <- `time` is the trigger's long
//!   batchCache.removeIf(e -> !e.value.isStillValid(time))
//!   posPredicate = pos -> retry = batchCache.get(pos.asLong());
//!                         retry == null ? true : !retry.shouldRetry(time) ? false : (retry.markAttempt(time); true)
//!   set = poiManager.findAllClosestFirstWithType(poiType, posPredicate, mob.blockPosition(), 48, HAS_SPACE)
//!             .limit(5).filter(p -> predicate.test(level, p.second)).collect(toSet())
//!   path = findPathToPois(mob, set)
//!   if path != null && path.canReach():
//!     target = path.getTarget()
//!     poiManager.getType(target).ifPresent(_ ->
//!       poiManager.take(poiType, (h, p) -> p.equals(target), target, 1)
//!       acquire.set(GlobalPos.of(level.dimension(), target))
//!       entityEventId.ifPresent(b -> level.broadcastEntityEvent(mob, b))
//!       batchCache.clear()
//!       level.debugSynchronizers().updatePoi(target))                   <- debug only, not ported
//!   else: for p in set: batchCache.computeIfAbsent(p.second.asLong(), () -> new JitteredLinearRetry(random, time))
//!   return true
//!
//! findPathToPois(mob, set) (0..102):
//!   if set.isEmpty(): return null
//!   positions = new HashSet; maxRange = 1
//!   for p in set: maxRange = max(maxRange, p.first.value().validRange()); positions.add(p.second)
//!   return mob.getNavigation().createPath(positions, maxRange)
//!
//! JitteredLinearRetry (MIN 40, MAX 80, cap 400):
//!   <init>(random, time): markAttempt(time)
//!   markAttempt(time): previous = time; currentDelay = min(currentDelay + random.nextInt(40) + 40, 400); next = time + currentDelay
//!   isStillValid(time): time - previous < 400
//!   shouldRetry(time):  time >= next
//! ```
//!
//! Draw order on the level random: the `nextInt(20)` reschedule first,
//! then one `nextInt(40)` per cached position whose retry is due — in
//! the manager's in-range order, because `findAllClosestFirstWithType`
//! is `findAllWithType(...).sorted(...)` (`PoiManager`, 0..25) and the
//! position predicate runs on every in-range record before the sort —
//! then one `nextInt(40)` per newly cached position after a failed path.
//! That last loop walks a `HashSet<Pair<Holder<PoiType>, BlockPos>>`
//! whose hash is the holder's identity hash: **its order is unverifiable
//! by construction** and this port uses the closest-first order the set
//! was built in. The `HashSet<BlockPos>` handed to `createPath` is
//! reproduced in Java iteration order (`Vec3i.hashCode`, `HashMap.hash`
//! spread, 16 buckets doubling past three quarters).

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::memory::{GlobalPos, MemoryModuleType, MemoryValue};
use crate::brain::BrainState;
use crate::host::{MobHost, PathLevel, PoiOccupancy, PoiRecordFacts};
use crate::pathfinder::path::Path;
use crate::phys::dist_sqr;
use rustmc_data::BlockPos;

/// `SCAN_RANGE`.
pub const SCAN_RANGE: i32 = 48;
/// The `limit(5)` on the closest-first stream (`iconst_5` at 0).
pub const BATCH_SIZE: usize = 5;

/// A `Predicate<Holder<PoiType>>` over the type's registry key.
pub type PoiPredicate = Box<dyn Fn(&str) -> bool>;
/// A `BiPredicate<ServerLevel, BlockPos>` over the level and a candidate.
pub type PoiPosPredicate<M> = Box<dyn FnMut(&mut MobAi<M>, BlockPos) -> bool>;

/// `MIN_INTERVAL_INCREASE`.
pub const MIN_INTERVAL_INCREASE: i32 = 40;
/// `MAX_INTERVAL_INCREASE`.
pub const MAX_INTERVAL_INCREASE: i32 = 80;
/// `MAX_RETRY_PATHFINDING_INTERVAL`.
pub const MAX_RETRY_PATHFINDING_INTERVAL: i32 = 400;

/// `AcquirePoi$JitteredLinearRetry`. The random it draws from is the
/// level's, reached through the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JitteredLinearRetry {
    pub previous_attempt_timestamp: i64,
    pub next_scheduled_attempt_timestamp: i64,
    pub current_delay: i32,
}

impl JitteredLinearRetry {
    /// `JitteredLinearRetry(random, time)`: one `nextInt(40)`.
    pub fn new<M: MobHost>(mob: &mut M, time: i64) -> Self {
        let mut r = JitteredLinearRetry {
            previous_attempt_timestamp: 0,
            next_scheduled_attempt_timestamp: 0,
            current_delay: 0,
        };
        r.mark_attempt(mob, time);
        r
    }

    /// `markAttempt(time)` (0..47).
    pub fn mark_attempt<M: MobHost>(&mut self, mob: &mut M, time: i64) {
        self.previous_attempt_timestamp = time;
        let d = self.current_delay
            + mob.level_random_next_int(MAX_INTERVAL_INCREASE - MIN_INTERVAL_INCREASE)
            + MIN_INTERVAL_INCREASE;
        self.current_delay = d.min(MAX_RETRY_PATHFINDING_INTERVAL);
        self.next_scheduled_attempt_timestamp = time + i64::from(self.current_delay);
    }

    /// `isStillValid(time)`.
    #[must_use]
    pub fn is_still_valid(&self, time: i64) -> bool {
        time - self.previous_attempt_timestamp < i64::from(MAX_RETRY_PATHFINDING_INTERVAL)
    }

    /// `shouldRetry(time)`.
    #[must_use]
    pub fn should_retry(&self, time: i64) -> bool {
        time >= self.next_scheduled_attempt_timestamp
    }
}

/// `Vec3i.hashCode()`: `(y + z * 31) * 31 + x`.
#[must_use]
pub fn block_pos_java_hash(p: BlockPos) -> i32 {
    p.y.wrapping_add(p.z.wrapping_mul(31))
        .wrapping_mul(31)
        .wrapping_add(p.x)
}

/// A `HashSet<BlockPos>` built by adding `positions` in order, walked in
/// Java's iteration order: `HashMap.hash` spreads the hash
/// (`h ^ (h >>> 16)`), the table starts at 16 buckets and doubles past
/// three quarters full, and a bucket keeps insertion order (tree bins are
/// not modelled: they need eight collisions in one bucket).
#[must_use]
pub fn java_hash_set_order_block_pos(positions: &[BlockPos]) -> Vec<BlockPos> {
    let mut distinct: Vec<BlockPos> = Vec::new();
    for p in positions {
        if !distinct.contains(p) {
            distinct.push(*p);
        }
    }
    let mut n: usize = 16;
    while distinct.len() > n * 3 / 4 {
        n *= 2;
    }
    let mut buckets: Vec<Vec<BlockPos>> = vec![Vec::new(); n];
    for p in &distinct {
        #[allow(clippy::cast_sign_loss)]
        let h = block_pos_java_hash(*p) as u32;
        let h = h ^ (h >> 16);
        buckets[(h as usize) & (n - 1)].push(*p);
    }
    buckets.into_iter().flatten().collect()
}

/// `findPathToPois(mob, set)`: `None` for an empty set, else the
/// navigation's path to the set's positions with accuracy
/// `max(1, max validRange)`.
pub fn find_path_to_pois<M: MobHost>(cx: &mut MobAi<M>, set: &[PoiRecordFacts]) -> Option<Path> {
    if set.is_empty() {
        return None;
    }
    let mut max_range = 1;
    let mut positions = Vec::with_capacity(set.len());
    for r in set {
        max_range = max_range.max(cx.mob.poi_valid_range(r.poi_type));
        positions.push(r.pos);
    }
    let positions = java_hash_set_order_block_pos(&positions);
    cx.navigation
        .create_path_to_any(&mut cx.mob, &positions, max_range)
}

/// `AcquirePoi`'s trigger, with the two captured mutables
/// (`MutableLong nextScheduledStart`, `Long2ObjectMap batchCache`).
pub struct AcquirePoi<M: MobHost> {
    pub poi_type: PoiPredicate,
    pub entry_memory: MemoryModuleType,
    pub acquire_memory: MemoryModuleType,
    pub only_if_adult: bool,
    pub entity_event_id: Option<u8>,
    pub predicate: PoiPosPredicate<M>,
    /// `nextScheduledStart`.
    pub next_scheduled_start: i64,
    /// `batchCache`, keyed by position (`asLong` is a bijection on
    /// in-world positions), in insertion order; the map's own order is
    /// never observed.
    pub batch_cache: Vec<(BlockPos, JitteredLinearRetry)>,
}

impl<M: MobHost> AcquirePoi<M> {
    /// `create(Predicate, MemoryModuleType, boolean, Optional<Byte>)`.
    #[must_use]
    pub fn create(
        poi_type: PoiPredicate,
        memory: MemoryModuleType,
        only_if_adult: bool,
        entity_event_id: Option<u8>,
    ) -> OneShot<Self> {
        Self::create_full(
            poi_type,
            memory,
            memory,
            only_if_adult,
            entity_event_id,
            Box::new(|_, _| true),
        )
    }

    /// `create(Predicate, MemoryModuleType, boolean, Optional<Byte>, BiPredicate)`.
    #[must_use]
    pub fn create_with_predicate(
        poi_type: PoiPredicate,
        memory: MemoryModuleType,
        only_if_adult: bool,
        entity_event_id: Option<u8>,
        predicate: PoiPosPredicate<M>,
    ) -> OneShot<Self> {
        Self::create_full(
            poi_type,
            memory,
            memory,
            only_if_adult,
            entity_event_id,
            predicate,
        )
    }

    /// `create(Predicate, MemoryModuleType, MemoryModuleType, boolean, Optional<Byte>, BiPredicate)`.
    #[must_use]
    pub fn create_full(
        poi_type: PoiPredicate,
        entry_memory: MemoryModuleType,
        acquire_memory: MemoryModuleType,
        only_if_adult: bool,
        entity_event_id: Option<u8>,
        predicate: PoiPosPredicate<M>,
    ) -> OneShot<Self> {
        OneShot::new(AcquirePoi {
            poi_type,
            entry_memory,
            acquire_memory,
            only_if_adult,
            entity_event_id,
            predicate,
            next_scheduled_start: 0,
            batch_cache: Vec::new(),
        })
    }

    /// The position predicate handed to the manager (`lambda$create$5`).
    fn pos_predicate(&mut self, cx: &mut MobAi<M>, pos: BlockPos, time: i64) -> bool {
        let Some((_, retry)) = self.batch_cache.iter_mut().find(|(p, _)| *p == pos) else {
            return true;
        };
        if !retry.should_retry(time) {
            return false;
        }
        retry.mark_attempt(&mut cx.mob, time);
        true
    }
}

impl<M: MobHost> Trigger<M> for AcquirePoi<M> {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) -> bool {
        if self.entry_memory != self.acquire_memory
            && brain.memory_internal(self.entry_memory) != Some(None)
        {
            return false;
        }
        if brain.memory_internal(self.acquire_memory) != Some(None) {
            return false;
        }
        if self.only_if_adult && cx.mob.is_baby() {
            return false;
        }
        if self.next_scheduled_start == 0 {
            self.next_scheduled_start =
                cx.mob.level().game_time() + i64::from(cx.mob.level_random_next_int(20));
            return false;
        }
        if cx.mob.level().game_time() < self.next_scheduled_start {
            return false;
        }
        self.next_scheduled_start = time + 20 + i64::from(cx.mob.level_random_next_int(20));
        self.batch_cache.retain(|(_, r)| r.is_still_valid(time));
        let center = cx.mob.block_position();
        let records = cx
            .mob
            .pois_in_range(center, SCAN_RANGE, PoiOccupancy::HasSpace);
        let mut passing: Vec<PoiRecordFacts> = Vec::new();
        for r in records {
            if !(self.poi_type)(r.poi_type) {
                continue;
            }
            if self.pos_predicate(cx, r.pos, time) {
                passing.push(r);
            }
        }
        passing.sort_by(|a, b| dist_sqr(a.pos, center).total_cmp(&dist_sqr(b.pos, center)));
        let mut set: Vec<PoiRecordFacts> = Vec::new();
        for r in passing.into_iter().take(BATCH_SIZE) {
            if (self.predicate)(cx, r.pos) {
                set.push(r);
            }
        }
        match find_path_to_pois(cx, &set) {
            Some(path) if path.can_reach() => {
                let target = path.target();
                if cx.mob.poi_type_at(target).is_some() {
                    cx.mob.take_poi(target);
                    brain.set_memory_value(
                        self.acquire_memory,
                        MemoryValue::GlobalPos(GlobalPos {
                            dimension: cx.mob.dimension(),
                            pos: target,
                        }),
                    );
                    if let Some(b) = self.entity_event_id {
                        cx.mob.broadcast_entity_event(b);
                    }
                    self.batch_cache.clear();
                }
            }
            _ => {
                for r in &set {
                    if !self.batch_cache.iter().any(|(p, _)| *p == r.pos) {
                        let retry = JitteredLinearRetry::new(&mut cx.mob, time);
                        self.batch_cache.push((r.pos, retry));
                    }
                }
            }
        }
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![self.entry_memory]
    }

    fn name(&self) -> &'static str {
        "AcquirePoi"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};
    use rustmc_data::Dimension;
    use rustmc_worldgen::rng::{LegacyRandomSource, RandomSource};

    fn poi(ty: &'static str, x: i32, y: i32, z: i32) -> (PoiRecordFacts, bool) {
        (
            PoiRecordFacts {
                poi_type: ty,
                pos: BlockPos { x, y, z },
            },
            false,
        )
    }

    fn setup(seed: i64) -> (MobAi<TestMob>, BrainState, LegacyRandomSource) {
        let mut level = GridLevel::new();
        level.floor(-16, -16, 40, 16, 0);
        let cx = MobAi::new(TestMob::new(level, 0.5, 1.0, 0.5, seed));
        let brain = Brain::<TestMob>::new(&[T::Home, T::JobSite], vec![]).state;
        (cx, brain, LegacyRandomSource::new(seed ^ 0x5DEECE66D))
    }

    #[test]
    fn hash_set_order_of_block_positions_is_by_spread_hash_bucket() {
        let a = BlockPos { x: 0, y: 0, z: 0 }; // hash 0 -> bucket 0
        let b = BlockPos { x: 1, y: 0, z: 0 }; // hash 1 -> bucket 1
        let c = BlockPos { x: 17, y: 0, z: 0 }; // hash 17 -> bucket 1, after b
        let d = BlockPos { x: -1, y: 0, z: 0 }; // hash -1 -> spread ...FFFF0000 -> bucket 0, after a
        assert_eq!(
            java_hash_set_order_block_pos(&[c, d, b, a]),
            vec![d, a, c, b]
        );
        assert_eq!(
            block_pos_java_hash(BlockPos { x: 1, y: 2, z: 3 }),
            (2 + 3 * 31) * 31 + 1
        );
    }

    #[test]
    fn first_trigger_schedules_then_a_reachable_poi_is_taken_and_remembered() {
        let (mut cx, mut brain, mut twin) = setup(9);
        cx.mob.pois = vec![poi("home", 6, 1, 0), poi("meeting", 2, 1, 0)];
        let mut b = AcquirePoi::create(Box::new(|k| k == "home"), T::Home, false, Some(14));
        cx.mob.level.game_time = 100;
        assert!(
            !b.try_start(&mut brain, &mut cx, 100),
            "first call only schedules"
        );
        let first = 100 + i64::from(twin.next_i32_bound(20));
        assert_eq!(b.trigger.next_scheduled_start, first);
        cx.mob.level.game_time = first - 1;
        assert!(
            !b.try_start(&mut brain, &mut cx, first - 1),
            "not yet due: no draw"
        );
        cx.mob.level.game_time = first;
        assert!(b.try_start(&mut brain, &mut cx, first));
        let next = first + 20 + i64::from(twin.next_i32_bound(20));
        assert_eq!(b.trigger.next_scheduled_start, next);
        assert_eq!(
            brain.get_memory(T::Home),
            Some(&MemoryValue::GlobalPos(GlobalPos {
                dimension: Dimension::Overworld,
                pos: BlockPos { x: 6, y: 1, z: 0 },
            }))
        );
        assert_eq!(cx.mob.calls, ["take_poi 6 1 0", "broadcastEntityEvent(14)"]);
        assert!(cx.mob.pois[0].1, "occupied now");
        assert!(b.trigger.batch_cache.is_empty());
        assert!(
            !b.try_start(&mut brain, &mut cx, next),
            "HOME present: refused, no draw"
        );
        assert_eq!(b.trigger.next_scheduled_start, next);
    }

    #[test]
    fn an_unreachable_poi_enters_the_jittered_retry_cache_and_is_skipped_until_due() {
        let (mut cx, mut brain, mut twin) = setup(4);
        cx.mob.pois = vec![poi("home", 3, 12, 0)];
        let mut b = AcquirePoi::create(Box::new(|k| k == "home"), T::Home, false, None);
        cx.mob.level.game_time = 50;
        assert!(!b.try_start(&mut brain, &mut cx, 50));
        let t1 = 50 + i64::from(twin.next_i32_bound(20));
        cx.mob.level.game_time = t1;
        assert!(b.try_start(&mut brain, &mut cx, t1));
        let t2 = t1 + 20 + i64::from(twin.next_i32_bound(20));
        let delay = twin.next_i32_bound(40) + 40;
        assert!(!brain.has_memory_value(T::Home));
        assert_eq!(
            b.trigger.batch_cache,
            vec![(
                BlockPos { x: 3, y: 12, z: 0 },
                JitteredLinearRetry {
                    previous_attempt_timestamp: t1,
                    next_scheduled_attempt_timestamp: t1 + i64::from(delay),
                    current_delay: delay,
                }
            )]
        );
        // Due again but the retry is not: the position is filtered out and
        // only the reschedule draws.
        cx.mob.level.game_time = t2;
        assert!(b.try_start(&mut brain, &mut cx, t2));
        let t3 = t2 + 20 + i64::from(twin.next_i32_bound(20));
        assert_eq!(b.trigger.batch_cache[0].1.current_delay, delay);
        // Once the retry is due, markAttempt draws again and the delay grows.
        let t = t3.max(t1 + i64::from(delay));
        b.trigger.next_scheduled_start = t;
        cx.mob.level.game_time = t;
        assert!(b.try_start(&mut brain, &mut cx, t));
        let _ = twin.next_i32_bound(20);
        let delay2 = (delay + twin.next_i32_bound(40) + 40).min(400);
        assert_eq!(b.trigger.batch_cache[0].1.current_delay, delay2);
        assert_eq!(b.trigger.batch_cache[0].1.previous_attempt_timestamp, t);
        // 400 ticks after the last attempt the entry is purged and re-added fresh.
        b.trigger.next_scheduled_start = t + 400;
        cx.mob.level.game_time = t + 400;
        assert!(b.try_start(&mut brain, &mut cx, t + 400));
        let _ = twin.next_i32_bound(20);
        assert_eq!(b.trigger.batch_cache.len(), 1);
        assert_eq!(
            b.trigger.batch_cache[0].1.previous_attempt_timestamp,
            t + 400
        );
        let delay3 = twin.next_i32_bound(40) + 40;
        assert_eq!(
            b.trigger.batch_cache[0].1.current_delay, delay3,
            "a fresh entry starts from zero"
        );
    }

    #[test]
    fn a_baby_is_refused_when_only_adults_may_and_a_different_entry_memory_gates() {
        let (mut cx, mut brain, _twin) = setup(1);
        cx.mob.baby = true;
        let mut b = AcquirePoi::create(Box::new(|_| true), T::Home, true, None);
        assert!(!b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(
            b.trigger.next_scheduled_start, 0,
            "refused before the schedule"
        );
        let mut b = AcquirePoi::create_full(
            Box::new(|_| true),
            T::JobSite,
            T::Home,
            false,
            None,
            Box::new(|_, _| true),
        );
        brain.set_memory_value(
            T::JobSite,
            MemoryValue::GlobalPos(GlobalPos {
                dimension: Dimension::Overworld,
                pos: BlockPos { x: 0, y: 0, z: 0 },
            }),
        );
        assert!(!b.try_start(&mut brain, &mut cx, 0), "entry memory present");
        assert_eq!(
            Trigger::<TestMob>::required_memories(&b.trigger),
            vec![T::JobSite]
        );
    }
}
