//! `net.minecraft.world.entity.ai.behavior.SetClosestHomeAsWalkTarget`,
//! from the 26.2 jar. A `BehaviorBuilder` one-shot over
//! `group(absent(WALK_TARGET), absent(HOME))` with a captured
//! `Long2LongMap batchCache` and `MutableLong lastUpdate`.
//! `CACHE_TIMEOUT = 40`, `BATCH_SIZE = 5`, `RATE = 20`, `OK_DISTANCE_SQR = 4`.
//!
//! ```text
//! lambda$create$2 (0..262):
//!   if level.getGameTime() - lastUpdate < 20: return false
//!   closest = poiManager.findClosest(h -> h.is(HOME), mob.blockPosition(), 48, ANY)   <- Stream.min: first minimal in manager order
//!   if closest.isEmpty() || closest.distSqr(mob.blockPosition()) <= 4.0: return false
//!   triedCount = 0
//!   lastUpdate = level.getGameTime() + level.getRandom().nextInt(20)                <- LEVEL random
//!   posPredicate = pos -> if batchCache.containsKey(pos): false
//!                         else if ++triedCount >= 5: false
//!                         else: batchCache.put(pos, lastUpdate + 40); true
//!   set = poiManager.findAllWithType(HOME, posPredicate, mob.blockPosition(), 48, ANY).collect(toSet())
//!   path = AcquirePoi.findPathToPois(mob, set)
//!   if path != null && path.canReach():
//!     target = path.getTarget()
//!     if poiManager.getType(target).isPresent(): walk.set(WalkTarget(target, speed, 1)); debug updatePoi
//!   else if triedCount < 5: batchCache.removeIf(e -> e.value < lastUpdate)
//!   return true
//! ```
//!
//! One level `nextInt(20)` per accepted trigger; the set walk is only
//! read through `findPathToPois`.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::behaviors::acquire_poi::find_path_to_pois;
use crate::brain::memory::{MemoryModuleType, MemoryValue, WalkTarget};
use crate::brain::BrainState;
use crate::host::{MobHost, PathLevel, PoiOccupancy, PoiRecordFacts};
use crate::phys::dist_sqr;
use rustmc_data::BlockPos;

/// `CACHE_TIMEOUT`.
pub const CACHE_TIMEOUT: i64 = 40;
/// `BATCH_SIZE`.
pub const BATCH_SIZE: i32 = 5;
/// `RATE`.
pub const RATE: i32 = 20;
/// `OK_DISTANCE_SQR`.
pub const OK_DISTANCE_SQR: f64 = 4.0;

/// `SetClosestHomeAsWalkTarget`'s trigger.
#[derive(Debug, Clone)]
pub struct SetClosestHomeAsWalkTarget {
    pub speed_modifier: f32,
    /// `batchCache`, keyed by position, in insertion order.
    pub batch_cache: Vec<(BlockPos, i64)>,
    /// `lastUpdate`.
    pub last_update: i64,
}

impl SetClosestHomeAsWalkTarget {
    /// `create(float)`.
    #[must_use]
    pub fn create(speed_modifier: f32) -> OneShot<Self> {
        OneShot::new(SetClosestHomeAsWalkTarget {
            speed_modifier,
            batch_cache: Vec::new(),
            last_update: 0,
        })
    }
}

impl<M: MobHost> Trigger<M> for SetClosestHomeAsWalkTarget {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        if brain.memory_internal(MemoryModuleType::WalkTarget) != Some(None)
            || brain.memory_internal(MemoryModuleType::Home) != Some(None)
        {
            return false;
        }
        let game_time = cx.mob.level().game_time();
        if game_time - self.last_update < i64::from(RATE) {
            return false;
        }
        let here = cx.mob.block_position();
        let homes: Vec<PoiRecordFacts> = cx
            .mob
            .pois_in_range(here, 48, PoiOccupancy::Any)
            .into_iter()
            .filter(|r| r.poi_type == "home")
            .collect();
        let closest = homes
            .iter()
            .map(|r| r.pos)
            .min_by(|a, b| dist_sqr(*a, here).total_cmp(&dist_sqr(*b, here)));
        match closest {
            Some(p) if dist_sqr(p, here) > OK_DISTANCE_SQR => {}
            _ => return false,
        }
        let mut tried_count = 0;
        self.last_update = game_time + i64::from(cx.mob.level_random_next_int(RATE));
        let mut set: Vec<PoiRecordFacts> = Vec::new();
        for r in homes {
            if self.batch_cache.iter().any(|(p, _)| *p == r.pos) {
                continue;
            }
            tried_count += 1;
            if tried_count >= BATCH_SIZE {
                continue;
            }
            self.batch_cache
                .push((r.pos, self.last_update + CACHE_TIMEOUT));
            set.push(r);
        }
        match find_path_to_pois(cx, &set) {
            Some(path) if path.can_reach() => {
                let target = path.target();
                if cx.mob.poi_type_at(target).is_some() {
                    brain.set_memory_value(
                        MemoryModuleType::WalkTarget,
                        MemoryValue::WalkTarget(WalkTarget::at_block(
                            target,
                            self.speed_modifier,
                            1,
                        )),
                    );
                }
            }
            _ => {
                if tried_count < BATCH_SIZE {
                    let last = self.last_update;
                    self.batch_cache.retain(|(_, t)| *t >= last);
                }
            }
        }
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![MemoryModuleType::WalkTarget, MemoryModuleType::Home]
    }

    fn name(&self) -> &'static str {
        "SetClosestHomeAsWalkTarget"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};
    use rustmc_worldgen::rng::{LegacyRandomSource, RandomSource};

    fn home(x: i32, y: i32, z: i32) -> (PoiRecordFacts, bool) {
        (
            PoiRecordFacts {
                poi_type: "home",
                pos: BlockPos { x, y, z },
            },
            true,
        )
    }

    #[test]
    fn walks_to_a_reachable_home_beyond_two_blocks_once_per_jittered_twenty_ticks() {
        let mut level = GridLevel::new();
        level.floor(-16, -16, 40, 16, 0);
        let mut cx = MobAi::new(TestMob::new(level, 0.5, 1.0, 0.5, 12));
        let mut twin = LegacyRandomSource::new(12 ^ 0x5DEECE66D);
        let mut brain = Brain::<TestMob>::new(&[T::WalkTarget, T::Home], vec![]).state;
        let mut b = SetClosestHomeAsWalkTarget::create(0.5);
        cx.mob.level.game_time = 30;
        assert!(
            !b.try_start(&mut brain, &mut cx, 30),
            "no home at all: refused before any draw"
        );
        cx.mob.pois = vec![home(1, 1, 0)];
        assert!(
            !b.try_start(&mut brain, &mut cx, 30),
            "the closest is within 2: refused"
        );
        cx.mob.pois = vec![home(8, 1, 0), home(1, 1, 0)];
        assert!(
            !b.try_start(&mut brain, &mut cx, 30),
            "closest is still the near one"
        );
        cx.mob.pois = vec![home(8, 1, 0), home(6, 1, 3)];
        assert!(b.try_start(&mut brain, &mut cx, 30));
        let last = 30 + i64::from(twin.next_i32_bound(20));
        assert_eq!(b.trigger.last_update, last);
        let walk = brain
            .get_memory(T::WalkTarget)
            .unwrap()
            .as_walk_target()
            .unwrap();
        assert_eq!(walk.speed_modifier, 0.5);
        assert_eq!(walk.close_enough_dist, 1);
        assert_eq!(b.trigger.batch_cache.len(), 2);
        brain.erase_memory(T::WalkTarget);
        cx.mob.level.game_time = last + 19;
        assert!(
            !b.try_start(&mut brain, &mut cx, last + 19),
            "within 20 of lastUpdate"
        );
        // Both homes are cached now: nothing to path to, triedCount 0 < 5 purges the stale entries.
        cx.mob.level.game_time = last + 41;
        assert!(b.try_start(&mut brain, &mut cx, last + 41));
        let _ = twin.next_i32_bound(20);
        assert!(!brain.has_memory_value(T::WalkTarget));
        assert!(b.trigger.batch_cache.is_empty());
    }
}
