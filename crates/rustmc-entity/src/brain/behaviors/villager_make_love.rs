//! `net.minecraft.world.entity.ai.behavior.VillagerMakeLove`, from the 26.2
//! jar. `Behavior<Villager>` with entry `{BREED_TARGET: VALUE_PRESENT,
//! NEAREST_VISIBLE_LIVING_ENTITIES: VALUE_PRESENT}` and `(350, 350)`.
//!
//! ```text
//! isBreedingPossible (0..68): partner = BREED_TARGET.filter(is VILLAGER); present
//!   && BehaviorUtils.targetIsValid(brain, BREED_TARGET, VILLAGER) && villager.canBreed() && partner.canBreed()
//! checkExtraStartConditions: isBreedingPossible;  canStillUse (0..22): time <= birthTimestamp && isBreedingPossible
//! start (0..65): partner; lockGazeAndWalkToEachOther(villager, partner, 0.5f, 2); event(partner, 18); event(villager, 18)
//!   birthTimestamp = time + 275 + villager.getRandom().nextInt(50)                              <- MOB random
//! tick (0..99): partner; if distanceToSqr(partner) > 5.0: lockGazeAndWalkToEachOther(villager, partner, 0.5f, 2)
//!   if time >= birthTimestamp: villager.eatAndDigestFood(); partner.eatAndDigestFood(); tryToGiveBirth(level, villager, partner)
//!   else if villager.getRandom().nextInt(35) == 0: event(partner, 12); event(villager, 12)     <- MOB random
//! tryToGiveBirth (0..105): bed = takeVacantBed(level, villager)
//!   if empty: event(partner, 13); event(villager, 13)
//!   else: child = breed(level, villager, partner); if present: giveBedToChild(child, bed) else: poiManager.release(bed); debug
//! takeVacantBed: poiManager.take(HOME, (type, pos) -> canReach(villager, pos, type), villager.blockPosition(), 48)
//!   = getInRange(HOME, pos, 48, HAS_SPACE).filter(r -> canReach(r.getPoiType(), r.getPos())).findFirst().map(acquireTicket; getPos)
//! canReach: navigation.createPath(pos, type.validRange()) reaches
//! breed (0..77): child = villager.getBreedOffspring(level, partner); null -> empty
//!   villager.setAge(6000); partner.setAge(6000); child.setAge(-24000); child.snapTo(villager.x, y, z, 0, 0)
//!   level.addFreshEntityWithPassengers(child); event(child, 12); Some(child)
//! giveBedToChild: child.brain.HOME = GlobalPos.of(level.dimension(), bed)
//! stop: erase BREED_TARGET
//! ```
//!
//! `takeVacantBed` paths to each free home in the manager's order until
//! one reaches; a path search draws nothing.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::behaviors::behavior_utils::{lock_gaze_and_walk_to_each_other, target_is_valid};
use crate::brain::memory::{GlobalPos, MemoryModuleType, MemoryStatus, MemoryValue};
use crate::brain::BrainState;
use crate::host::{MobHost, PoiOccupancy};
use rustmc_data::BlockPos;
use rustmc_worldgen::rng::RandomSource;

/// `VillagerMakeLove`'s body; build one with [`VillagerMakeLove::new`].
#[derive(Debug, Clone, Copy, Default)]
pub struct VillagerMakeLove {
    /// `birthTimestamp`.
    pub birth_timestamp: i64,
}

impl VillagerMakeLove {
    /// `VillagerMakeLove()`.
    #[must_use]
    pub fn new() -> Behavior<VillagerMakeLove> {
        Behavior::with_durations(
            VillagerMakeLove::default(),
            vec![
                (MemoryModuleType::BreedTarget, MemoryStatus::ValuePresent),
                (
                    MemoryModuleType::NearestVisibleLivingEntities,
                    MemoryStatus::ValuePresent,
                ),
            ],
            350,
            350,
        )
    }

    fn partner(brain: &BrainState) -> i32 {
        brain
            .get_memory(MemoryModuleType::BreedTarget)
            .and_then(MemoryValue::as_entity)
            .expect("BREED_TARGET is VALUE_PRESENT")
    }

    /// `isBreedingPossible(villager)`.
    fn is_breeding_possible<M: MobHost>(brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        let Some(partner) = brain
            .get_memory(MemoryModuleType::BreedTarget)
            .and_then(MemoryValue::as_entity)
            .filter(|p| cx.mob.entity_kind_is(*p, "villager"))
        else {
            return false;
        };
        target_is_valid(brain, cx, MemoryModuleType::BreedTarget, "villager")
            && cx.mob.can_breed()
            && cx.mob.other_can_breed(partner)
    }

    /// `takeVacantBed(level, villager)`.
    fn take_vacant_bed<M: MobHost>(cx: &mut MobAi<M>) -> Option<BlockPos> {
        let here = cx.mob.block_position();
        let homes: Vec<_> = cx
            .mob
            .pois_in_range(here, 48, PoiOccupancy::HasSpace)
            .into_iter()
            .filter(|r| r.poi_type == "home")
            .collect();
        for r in homes {
            let range = cx.mob.poi_valid_range(r.poi_type);
            let reaches = cx
                .navigation
                .create_path(&mut cx.mob, r.pos, range)
                .is_some_and(|p| p.can_reach());
            if reaches {
                cx.mob.take_poi(r.pos);
                return Some(r.pos);
            }
        }
        None
    }

    /// `breed(level, villager, partner)`.
    fn breed<M: MobHost>(cx: &mut MobAi<M>, partner: i32) -> Option<i32> {
        let child = cx.mob.breed_offspring(partner)?;
        cx.mob.set_age(6000);
        cx.mob.other_set_age(partner, 6000);
        cx.mob.other_set_age(child, -24000);
        let (x, y, z) = (cx.mob.x(), cx.mob.y(), cx.mob.z());
        cx.mob.other_snap_to(child, x, y, z, 0.0, 0.0);
        cx.mob.add_fresh_entity_with_passengers(child);
        cx.mob.other_broadcast_entity_event(child, 12);
        Some(child)
    }

    fn try_to_give_birth<M: MobHost>(cx: &mut MobAi<M>, partner: i32) {
        match Self::take_vacant_bed(cx) {
            None => {
                cx.mob.other_broadcast_entity_event(partner, 13);
                cx.mob.broadcast_entity_event(13);
            }
            Some(bed) => match Self::breed(cx, partner) {
                Some(child) => {
                    let gp = GlobalPos {
                        dimension: cx.mob.dimension(),
                        pos: bed,
                    };
                    cx.mob.other_set_home(child, gp);
                }
                None => cx.mob.release_poi_at_global(GlobalPos {
                    dimension: cx.mob.dimension(),
                    pos: bed,
                }),
            },
        }
    }
}

impl<M: MobHost> BehaviorBody<M> for VillagerMakeLove {
    fn check_extra_start_conditions(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        Self::is_breeding_possible(brain, cx)
    }

    fn can_still_use(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) -> bool {
        time <= self.birth_timestamp && Self::is_breeding_possible(brain, cx)
    }

    fn start(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        let partner = Self::partner(brain);
        lock_gaze_and_walk_to_each_other(brain, cx, partner, 0.5, 2);
        cx.mob.other_broadcast_entity_event(partner, 18);
        cx.mob.broadcast_entity_event(18);
        let delay = 275 + cx.mob.random().next_i32_bound(50);
        self.birth_timestamp = time + i64::from(delay);
    }

    fn tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        let partner = Self::partner(brain);
        if cx
            .mob
            .distance_to_sqr_entity(partner)
            .is_some_and(|d| d > 5.0)
        {
            lock_gaze_and_walk_to_each_other(brain, cx, partner, 0.5, 2);
        }
        if time >= self.birth_timestamp {
            cx.mob.eat_and_digest_food();
            cx.mob.other_eat_and_digest_food(partner);
            Self::try_to_give_birth(cx, partner);
        } else if cx.mob.random().next_i32_bound(35) == 0 {
            cx.mob.other_broadcast_entity_event(partner, 12);
            cx.mob.broadcast_entity_event(12);
        }
    }

    fn stop(&mut self, brain: &mut BrainState, _cx: &mut MobAi<M>, _time: i64) {
        brain.erase_memory(MemoryModuleType::BreedTarget);
    }

    fn name(&self) -> &'static str {
        "VillagerMakeLove"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::{MemoryModuleType as T, NearestVisibleLivingEntities};
    use crate::brain::Brain;
    use crate::host::{EntityClass, EntityFacts, PoiRecordFacts};
    use crate::testing::{GridLevel, TestMob};
    use rustmc_worldgen::rng::LegacyRandomSource;

    #[test]
    fn courts_for_275_plus_next_int_50_ticks_then_takes_a_reachable_bed_and_breeds() {
        let mut level = GridLevel::new();
        level.floor(-16, -16, 40, 16, 0);
        let mut cx = MobAi::new(TestMob::new(level, 0.5, 1.0, 0.5, 4));
        let mut twin = LegacyRandomSource::new(4);
        cx.mob.others = vec![(EntityClass::Villager, EntityFacts::living(7, 1.5, 1.0, 0.5))];
        cx.mob.entity_kinds = vec![(7, "villager")];
        cx.mob.can_breed = true;
        cx.mob.others_can_breed = vec![7];
        cx.mob.offspring = Some(30);
        cx.mob.pois = vec![
            (
                PoiRecordFacts {
                    poi_type: "home",
                    pos: BlockPos { x: 44, y: 1, z: 0 },
                },
                false,
            ),
            (
                PoiRecordFacts {
                    poi_type: "home",
                    pos: BlockPos { x: 6, y: 1, z: 0 },
                },
                false,
            ),
        ];
        let mut brain = Brain::<TestMob>::new(
            &[
                T::BreedTarget,
                T::NearestVisibleLivingEntities,
                T::WalkTarget,
                T::LookTarget,
            ],
            vec![],
        )
        .state;
        brain.set_memory_value(T::BreedTarget, MemoryValue::Entity(7));
        brain.set_memory_value(
            T::NearestVisibleLivingEntities,
            MemoryValue::NearestVisible(NearestVisibleLivingEntities::new(vec![7])),
        );
        cx.mob.visible = vec![7];
        let mut b = VillagerMakeLove::new();
        assert!(b.try_start(&mut brain, &mut cx, 100));
        let birth = 100 + 275 + i64::from(twin.next_i32_bound(50));
        assert_eq!(b.body.birth_timestamp, birth);
        assert!(cx.mob.calls.ends_with(&[
            "other_event 7 18".to_string(),
            "broadcastEntityEvent(18)".to_string()
        ]));
        cx.mob.calls.clear();
        // Courting ticks: one nextInt(35) each, hearts on a zero.
        for t in 101..birth {
            b.tick_or_stop(&mut brain, &mut cx, t);
            let hearts = twin.next_i32_bound(35) == 0;
            assert_eq!(
                cx.mob
                    .calls
                    .contains(&"broadcastEntityEvent(12)".to_string()),
                hearts,
                "tick {t}"
            );
            cx.mob.calls.clear();
        }
        b.tick_or_stop(&mut brain, &mut cx, birth);
        assert_eq!(
            cx.mob.calls,
            [
                "eat",
                "other_eat 7",
                "take_poi 6 1 0",
                "breed_offspring 7",
                "set_age 6000",
                "other_set_age 7 6000",
                "other_set_age 30 -24000",
                "other_snap_to 30 0.5 1 0.5 0 0",
                "add_entity 30",
                "other_event 30 12",
                "other_set_home 30 6 1 0",
            ],
            "the bed past the floor edge is unreachable; the one on the floor is taken"
        );
        assert!(cx.mob.pois[1].1);
        b.tick_or_stop(&mut brain, &mut cx, birth + 1);
        assert!(
            !brain.has_memory_value(T::BreedTarget),
            "past the birth: stops"
        );
    }
}
