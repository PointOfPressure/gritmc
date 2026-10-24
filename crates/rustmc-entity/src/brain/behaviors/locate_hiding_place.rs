//! `net.minecraft.world.entity.ai.behavior.LocateHidingPlace`, from the
//! 26.2 jar. A `BehaviorBuilder` one-shot over `group(absent(WALK_TARGET),
//! registered(HOME, HIDING_PLACE, PATH, LOOK_TARGET, BREED_TARGET,
//! INTERACTION_TARGET))`. `create(int maxDistance, float speed, int
//! closeEnoughDist)`.
//!
//! ```text
//! lambda$create$2 (0..91):
//!   poiManager.find(HOME, any, mob.blockPosition(), closeEnoughDist + 1, ANY)     <- first in manager order
//!       .filter(p -> p.closerToCenterThan(mob.position(), closeEnoughDist))
//!       .or(() -> poiManager.getRandom(HOME, any, ANY, mob.blockPosition(), maxDistance, mob.getRandom()))
//!       .or(() -> HOME.tryGet().map(GlobalPos::pos))
//!       .ifPresent(pos ->
//!           erase PATH, LOOK_TARGET, BREED_TARGET, INTERACTION_TARGET
//!           hidingPlace.set(GlobalPos.of(level.dimension(), pos))
//!           if !pos.closerToCenterThan(mob.position(), closeEnoughDist): walk.set(WalkTarget(pos, speed, closeEnoughDist)))
//!   return true
//! PoiManager.getRandom (0..48): Util.toShuffledList(getInRange(type, pos, dist, occ), random)
//!                                   .stream().filter(posPred).findFirst().map(getPos)
//! ```
//!
//! The mob's random is drawn only when no home within
//! `closeEnoughDist + 1` passes the centre test (`Optional.or` is lazy):
//! `Util.shuffle` makes `size - 1` `nextInt(i)` draws over the in-range
//! homes, in the manager's order.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::behaviors::inside_brownian_walk::java_shuffle;
use crate::brain::memory::{GlobalPos, MemoryModuleType, MemoryValue, WalkTarget};
use crate::brain::BrainState;
use crate::host::{MobHost, PoiOccupancy};
use crate::phys::closer_to_center_than;
use rustmc_data::BlockPos;

/// `LocateHidingPlace`'s trigger.
#[derive(Debug, Clone, Copy)]
pub struct LocateHidingPlace {
    pub max_distance: i32,
    pub speed_modifier: f32,
    pub close_enough_dist: i32,
}

impl LocateHidingPlace {
    /// `create(int, float, int)`.
    #[must_use]
    pub fn create(max_distance: i32, speed_modifier: f32, close_enough_dist: i32) -> OneShot<Self> {
        OneShot::new(LocateHidingPlace {
            max_distance,
            speed_modifier,
            close_enough_dist,
        })
    }
}

const REGISTERED: [MemoryModuleType; 6] = [
    MemoryModuleType::Home,
    MemoryModuleType::HidingPlace,
    MemoryModuleType::Path,
    MemoryModuleType::LookTarget,
    MemoryModuleType::BreedTarget,
    MemoryModuleType::InteractionTarget,
];

impl<M: MobHost> Trigger<M> for LocateHidingPlace {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        if brain.memory_internal(MemoryModuleType::WalkTarget) != Some(None) {
            return false;
        }
        if REGISTERED
            .iter()
            .any(|t| brain.memory_internal(*t).is_none())
        {
            return false;
        }
        let here = cx.mob.block_position();
        let position = cx.mob.position();
        let close = f64::from(self.close_enough_dist);
        let mut found: Option<BlockPos> = cx
            .mob
            .pois_in_range(here, self.close_enough_dist + 1, PoiOccupancy::Any)
            .into_iter()
            .find(|r| r.poi_type == "home")
            .map(|r| r.pos)
            .filter(|p| closer_to_center_than(*p, position, close));
        if found.is_none() {
            let mut homes: Vec<BlockPos> = cx
                .mob
                .pois_in_range(here, self.max_distance, PoiOccupancy::Any)
                .into_iter()
                .filter(|r| r.poi_type == "home")
                .map(|r| r.pos)
                .collect();
            java_shuffle(&mut homes, cx.mob.random());
            found = homes.first().copied();
        }
        if found.is_none() {
            found = brain
                .get_memory(MemoryModuleType::Home)
                .and_then(MemoryValue::as_global_pos)
                .map(|gp| gp.pos);
        }
        if let Some(pos) = found {
            for t in [
                MemoryModuleType::Path,
                MemoryModuleType::LookTarget,
                MemoryModuleType::BreedTarget,
                MemoryModuleType::InteractionTarget,
            ] {
                brain.erase_memory(t);
            }
            brain.set_memory_value(
                MemoryModuleType::HidingPlace,
                MemoryValue::GlobalPos(GlobalPos {
                    dimension: cx.mob.dimension(),
                    pos,
                }),
            );
            if !closer_to_center_than(pos, position, close) {
                brain.set_memory_value(
                    MemoryModuleType::WalkTarget,
                    MemoryValue::WalkTarget(WalkTarget::at_block(
                        pos,
                        self.speed_modifier,
                        self.close_enough_dist,
                    )),
                );
            }
        }
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        let mut v = vec![MemoryModuleType::WalkTarget];
        v.extend(REGISTERED);
        v
    }

    fn name(&self) -> &'static str {
        "LocateHidingPlace"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::host::PoiRecordFacts;
    use crate::testing::{GridLevel, TestMob};
    use rustmc_data::Dimension;
    use rustmc_worldgen::rng::{LegacyRandomSource, RandomSource};

    fn home(x: i32, z: i32) -> (PoiRecordFacts, bool) {
        (
            PoiRecordFacts {
                poi_type: "home",
                pos: BlockPos { x, y: 1, z },
            },
            false,
        )
    }

    fn fresh_brain() -> BrainState {
        let mut all = vec![T::WalkTarget];
        all.extend(REGISTERED);
        Brain::<TestMob>::new(&all, vec![]).state
    }

    #[test]
    fn a_home_within_close_enough_is_taken_without_a_draw() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 2));
        cx.mob.pois = vec![home(2, 0), home(20, 0)];
        let mut brain = fresh_brain();
        brain.set_memory_value(T::LookTarget, MemoryValue::Int(1));
        let mut b = LocateHidingPlace::create(32, 0.5, 3);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(!brain.has_memory_value(T::LookTarget));
        assert_eq!(
            brain.get_memory(T::HidingPlace),
            Some(&MemoryValue::GlobalPos(GlobalPos {
                dimension: Dimension::Overworld,
                pos: BlockPos { x: 2, y: 1, z: 0 }
            }))
        );
        assert!(
            !brain.has_memory_value(T::WalkTarget),
            "already within 3 of its centre"
        );
        assert_eq!(
            cx.mob.random.next_f32(),
            LegacyRandomSource::new(2).next_f32()
        );
    }

    #[test]
    fn otherwise_a_shuffled_home_in_range_then_the_home_memory() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 5));
        cx.mob.pois = vec![home(10, 0), home(0, 12), home(-9, 3)];
        let mut brain = fresh_brain();
        let mut b = LocateHidingPlace::create(32, 0.5, 3);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        let mut twin = LegacyRandomSource::new(5);
        let mut homes = vec![
            BlockPos { x: 10, y: 1, z: 0 },
            BlockPos { x: 0, y: 1, z: 12 },
            BlockPos { x: -9, y: 1, z: 3 },
        ];
        java_shuffle(&mut homes, &mut twin);
        assert_eq!(
            brain.get_memory(T::WalkTarget).unwrap().as_walk_target(),
            Some(&WalkTarget::at_block(homes[0], 0.5, 3))
        );
        assert_eq!(
            cx.mob.random.next_f32(),
            twin.next_f32(),
            "two nextInt draws"
        );
        // No home in range at all: the HOME memory is the fallback.
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 5));
        let mut brain = fresh_brain();
        let far = GlobalPos {
            dimension: Dimension::Overworld,
            pos: BlockPos { x: 50, y: 1, z: 50 },
        };
        brain.set_memory_value(T::Home, MemoryValue::GlobalPos(far));
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(
            brain.get_memory(T::HidingPlace),
            Some(&MemoryValue::GlobalPos(far))
        );
        assert_eq!(
            cx.mob.random.next_f32(),
            LegacyRandomSource::new(5).next_f32(),
            "an empty list shuffles nothing"
        );
    }
}
