//! `net.minecraft.world.entity.ai.behavior.ValidateNearbyPoi`, from the
//! 26.2 jar. A `BehaviorBuilder` one-shot over `group(present(memory))`.
//! `MAX_DISTANCE = 16`.
//!
//! ```text
//! lambda$create$2 (0..133):
//!   gp = memory; pos = gp.pos()
//!   if level.dimension() != gp.dimension() || !pos.closerToCenterThan(mob.position(), 16.0): return false
//!   level' = server.getLevel(gp.dimension())
//!   if level' == null || !level'.poiManager.exists(pos, poiPredicate): memory.erase()
//!   else if bedIsOccupied(level', pos, mob):
//!     memory.erase()
//!     if !bedIsOccupiedByVillager(level', pos): level.poiManager.release(pos); debug updatePoi
//!   return true
//! bedIsOccupied (0..44):          state.is(BEDS) && state.getValue(OCCUPIED) && !mob.isSleeping()
//! bedIsOccupiedByVillager (0..34): !level.getEntitiesOfClass(Villager.class, new AABB(pos), any).isEmpty()
//! ```
//!
//! No random draw. The dimension test means `level'` is always this
//! level; `poi_type_at` answers `exists`.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::behaviors::acquire_poi::PoiPredicate;
use crate::brain::memory::{GlobalPos, MemoryModuleType};
use crate::brain::BrainState;
use crate::host::{EntityClass, MobHost};
use crate::phys::{closer_to_center_than, Aabb};
use rustmc_data::BlockPos;

/// `MAX_DISTANCE`.
pub const MAX_DISTANCE: f64 = 16.0;

/// `ValidateNearbyPoi`'s trigger.
pub struct ValidateNearbyPoi {
    pub poi_predicate: PoiPredicate,
    pub memory: MemoryModuleType,
}

impl ValidateNearbyPoi {
    /// `create(Predicate<Holder<PoiType>>, MemoryModuleType<GlobalPos>)`.
    #[must_use]
    pub fn create(poi_predicate: PoiPredicate, memory: MemoryModuleType) -> OneShot<Self> {
        OneShot::new(ValidateNearbyPoi {
            poi_predicate,
            memory,
        })
    }

    /// `bedIsOccupied(level, pos, mob)`.
    pub fn bed_is_occupied<M: MobHost>(cx: &MobAi<M>, pos: BlockPos) -> bool {
        cx.mob.bed_occupied_at(pos) && !cx.mob.is_sleeping()
    }

    /// `bedIsOccupiedByVillager(level, pos)`.
    pub fn bed_is_occupied_by_villager<M: MobHost>(cx: &MobAi<M>, pos: BlockPos) -> bool {
        let aabb = Aabb {
            min_x: f64::from(pos.x),
            min_y: f64::from(pos.y),
            min_z: f64::from(pos.z),
            max_x: f64::from(pos.x + 1),
            max_y: f64::from(pos.y + 1),
            max_z: f64::from(pos.z + 1),
        };
        !cx.mob
            .entities_of_class_in(EntityClass::Villager, &aabb)
            .is_empty()
    }
}

impl<M: MobHost> Trigger<M> for ValidateNearbyPoi {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        let Some(Some(v)) = brain.memory_internal(self.memory) else {
            return false;
        };
        let Some(gp) = v.as_global_pos() else {
            return false;
        };
        let pos = gp.pos;
        if cx.mob.dimension() != gp.dimension
            || !closer_to_center_than(pos, cx.mob.position(), MAX_DISTANCE)
        {
            return false;
        }
        let exists = cx
            .mob
            .poi_type_at(pos)
            .is_some_and(|t| (self.poi_predicate)(t));
        if !exists {
            brain.erase_memory(self.memory);
        } else if Self::bed_is_occupied(cx, pos) {
            brain.erase_memory(self.memory);
            if !Self::bed_is_occupied_by_villager(cx, pos) {
                cx.mob.release_poi_at_global(GlobalPos {
                    dimension: gp.dimension,
                    pos,
                });
            }
        }
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![self.memory]
    }

    fn name(&self) -> &'static str {
        "ValidateNearbyPoi"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::{MemoryModuleType as T, MemoryValue};
    use crate::brain::Brain;
    use crate::host::{EntityFacts, PoiRecordFacts};
    use crate::testing::{GridLevel, TestMob};
    use rustmc_data::Dimension;

    fn setup(pos: BlockPos) -> (MobAi<TestMob>, BrainState) {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        cx.mob.pois = vec![(
            PoiRecordFacts {
                poi_type: "home",
                pos,
            },
            true,
        )];
        let mut brain = Brain::<TestMob>::new(&[T::Home], vec![]).state;
        brain.set_memory_value(
            T::Home,
            MemoryValue::GlobalPos(GlobalPos {
                dimension: Dimension::Overworld,
                pos,
            }),
        );
        (cx, brain)
    }

    #[test]
    fn a_missing_poi_is_forgotten_and_a_taken_bed_is_forgotten_and_released() {
        let pos = BlockPos { x: 4, y: 1, z: 0 };
        let (mut cx, mut brain) = setup(BlockPos { x: 30, y: 1, z: 0 });
        let mut b = ValidateNearbyPoi::create(Box::new(|t| t == "home"), T::Home);
        assert!(!b.try_start(&mut brain, &mut cx, 0), "beyond 16");
        let (mut cx, mut brain) = setup(pos);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(brain.has_memory_value(T::Home), "present, unoccupied: kept");
        cx.mob.pois.clear();
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(!brain.has_memory_value(T::Home), "no POI there any more");
        let (mut cx, mut brain) = setup(pos);
        cx.mob.occupied_beds = vec![pos];
        cx.mob.sleeping = true;
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(
            brain.has_memory_value(T::Home),
            "occupied by the sleeper itself"
        );
        cx.mob.sleeping = false;
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(!brain.has_memory_value(T::Home));
        assert_eq!(cx.mob.calls, ["release_poi 4 1 0"]);
        let (mut cx, mut brain) = setup(pos);
        cx.mob.occupied_beds = vec![pos];
        cx.mob.others = vec![(EntityClass::Villager, EntityFacts::living(9, 4.5, 1.2, 0.5))];
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(!brain.has_memory_value(T::Home));
        assert!(
            cx.mob.calls.is_empty(),
            "a villager in the bed keeps the claim"
        );
    }
}
