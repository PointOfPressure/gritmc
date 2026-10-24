//! `net.minecraft.world.entity.ai.sensing.NearestBedSensor`, from the 26.2
//! jar. `Sensor(20)`; `requires()` = `{NEAREST_BED}`. `CACHE_TIMEOUT = 40`,
//! `BATCH_SIZE = 5`, `RATE = 20`.
//!
//! ```text
//! doTick(level, Mob) (0..168):
//!   if !mob.isBaby(): return                                           <- only babies look for beds
//!   triedCount = 0
//!   lastUpdate = level.getGameTime() + level.getRandom().nextInt(20)    <- LEVEL random
//!   posPredicate = pos -> if batchCache.containsKey(pos.asLong()): false
//!                         else if ++triedCount >= 5: false
//!                         else: batchCache.put(pos.asLong(), lastUpdate + 40); true
//!   set = poiManager.findAllWithType(h -> h.is(HOME), posPredicate, mob.blockPosition(), 48, ANY).collect(toSet())
//!   path = AcquirePoi.findPathToPois(mob, set)
//!   if path != null && path.canReach():
//!     target = path.getTarget()
//!     if poiManager.getType(target).isPresent(): brain.setMemory(NEAREST_BED, target)
//!   else if triedCount < 5: batchCache.removeIf(e -> e.value < lastUpdate)
//! ```
//!
//! One level `nextInt(20)` per tick of a baby. The set is a
//! `HashSet<Pair<Holder, BlockPos>>` (identity-hashed holders, so its
//! walk is unverifiable); it is only read through `findPathToPois`,
//! whose `HashSet<BlockPos>` is reproduced in Java order.

use crate::ai::mob_ai::MobAi;
use crate::brain::behaviors::acquire_poi::find_path_to_pois;
use crate::brain::memory::{MemoryModuleType, MemoryValue};
use crate::brain::sensor::{Sensor, SensorBody, SensorType};
use crate::brain::BrainState;
use crate::host::{MobHost, PathLevel, PoiOccupancy, PoiRecordFacts};
use rustmc_data::BlockPos;

/// `CACHE_TIMEOUT`.
pub const CACHE_TIMEOUT: i64 = 40;
/// `BATCH_SIZE`.
pub const BATCH_SIZE: i32 = 5;
/// `RATE`.
pub const RATE: i32 = 20;

/// `NearestBedSensor`.
#[derive(Debug, Clone, Default)]
pub struct NearestBedSensor {
    /// `batchCache`, keyed by position, in insertion order (its own order
    /// is never observed).
    pub batch_cache: Vec<(BlockPos, i64)>,
    /// `triedCount`.
    pub tried_count: i32,
    /// `lastUpdate`.
    pub last_update: i64,
}

impl NearestBedSensor {
    /// `SensorType.NEAREST_BED.create()`.
    #[must_use]
    pub fn new() -> Sensor<NearestBedSensor> {
        Sensor::with_scan_rate(NearestBedSensor::default(), RATE)
    }

    /// `lambda$doTick$0`, the position predicate.
    fn pos_predicate(&mut self, pos: BlockPos) -> bool {
        if self.batch_cache.iter().any(|(p, _)| *p == pos) {
            return false;
        }
        self.tried_count += 1;
        if self.tried_count >= BATCH_SIZE {
            return false;
        }
        self.batch_cache
            .push((pos, self.last_update + CACHE_TIMEOUT));
        true
    }
}

impl<M: MobHost> SensorBody<M> for NearestBedSensor {
    fn do_tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) {
        if !cx.mob.is_baby() {
            return;
        }
        self.tried_count = 0;
        self.last_update =
            cx.mob.level().game_time() + i64::from(cx.mob.level_random_next_int(RATE));
        let center = cx.mob.block_position();
        let records = cx.mob.pois_in_range(center, 48, PoiOccupancy::Any);
        let mut set: Vec<PoiRecordFacts> = Vec::new();
        for r in records {
            if r.poi_type == "home" && self.pos_predicate(r.pos) {
                set.push(r);
            }
        }
        match find_path_to_pois(cx, &set) {
            Some(path) if path.can_reach() => {
                let target = path.target();
                if cx.mob.poi_type_at(target).is_some() {
                    brain.set_memory_value(
                        MemoryModuleType::NearestBed,
                        MemoryValue::BlockPos(target),
                    );
                }
            }
            _ => {
                if self.tried_count < BATCH_SIZE {
                    let last = self.last_update;
                    self.batch_cache.retain(|(_, t)| *t >= last);
                }
            }
        }
    }

    fn requires(&self) -> Vec<MemoryModuleType> {
        vec![MemoryModuleType::NearestBed]
    }

    fn sensor_type(&self) -> SensorType {
        SensorType::NearestBed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};
    use rustmc_worldgen::rng::{LegacyRandomSource, RandomSource};

    fn bed(x: i32, y: i32, z: i32) -> (PoiRecordFacts, bool) {
        (
            PoiRecordFacts {
                poi_type: "home",
                pos: BlockPos { x, y, z },
            },
            false,
        )
    }

    #[test]
    fn a_baby_finds_a_reachable_bed_and_an_unreachable_one_is_cached_for_forty_ticks() {
        let mut level = GridLevel::new();
        level.floor(-16, -16, 40, 16, 0);
        let mut cx = MobAi::new(TestMob::new(level, 0.5, 1.0, 0.5, 6));
        let mut twin = LegacyRandomSource::new(6 ^ 0x5DEECE66D);
        cx.mob.pois = vec![bed(5, 1, 0)];
        let mut brain = Brain::<TestMob>::new(&[T::NearestBed], vec![]).state;
        let mut s = NearestBedSensor::default();
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert!(
            !brain.has_memory_value(T::NearestBed),
            "an adult never looks"
        );
        cx.mob.baby = true;
        cx.mob.level.game_time = 100;
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        let last = 100 + i64::from(twin.next_i32_bound(20));
        assert_eq!(s.last_update, last);
        assert_eq!(
            brain.get_memory(T::NearestBed),
            Some(&MemoryValue::BlockPos(BlockPos { x: 5, y: 1, z: 0 }))
        );
        assert_eq!(
            s.batch_cache,
            vec![(BlockPos { x: 5, y: 1, z: 0 }, last + 40)]
        );
        // The cached bed is filtered out next tick; nothing to path to, and
        // with triedCount < 5 the stale entries (older than lastUpdate) go.
        cx.mob.level.game_time = last + 41;
        brain.erase_memory(T::NearestBed);
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        let last2 = last + 41 + i64::from(twin.next_i32_bound(20));
        assert_eq!(s.last_update, last2);
        assert!(!brain.has_memory_value(T::NearestBed));
        assert!(s.batch_cache.is_empty(), "{} < {last2}", last + 40);
        // An unreachable bed still enters the cache and the count.
        cx.mob.pois = vec![bed(3, 12, 0)];
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        let _ = twin.next_i32_bound(20);
        assert_eq!(s.tried_count, 1);
        assert_eq!(s.batch_cache.len(), 1);
    }
}
