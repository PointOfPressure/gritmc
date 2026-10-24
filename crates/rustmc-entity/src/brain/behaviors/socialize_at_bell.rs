//! `net.minecraft.world.entity.ai.behavior.SocializeAtBell`, from the 26.2
//! jar. A `BehaviorBuilder` one-shot over `group(registered(WALK_TARGET),
//! registered(LOOK_TARGET), present(MEETING_POINT),
//! present(NEAREST_VISIBLE_LIVING_ENTITIES), absent(INTERACTION_TARGET))`.
//! `SPEED_MODIFIER = 0.3f`.
//!
//! ```text
//! lambda$create$2 (0..108):
//!   gp = meetingPoint; nv = nearestVisible
//!   if level.getRandom().nextInt(100) == 0                                     <- LEVEL random, first, every trigger
//!      && level.dimension() == gp.dimension()
//!      && gp.pos().closerToCenterThan(mob.position(), 4.0)
//!      && nv.contains(e -> e.is(VILLAGER)):
//!     nv.findClosest(e -> e.is(VILLAGER) && e.distanceToSqr(mob) <= 32.0).ifPresent(e ->
//!         interaction.set(e); look.set(EntityTracker(e, true)); walk.set(WalkTarget(EntityTracker(e, false), 0.3f, 1)))
//!     return true
//!   return false
//! ```
//!
//! `contains(Predicate)` and `findClosest` both run the predicate before
//! the line-of-sight memo, in the sensed order.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::behaviors::nearest_visible_find_closest;
use crate::brain::memory::{MemoryModuleType, MemoryValue, PositionTracker, WalkTarget};
use crate::brain::BrainState;
use crate::host::MobHost;
use crate::phys::closer_to_center_than;

/// `SPEED_MODIFIER`.
pub const SPEED_MODIFIER: f32 = 0.3;

/// `SocializeAtBell`'s trigger.
#[derive(Debug, Clone, Copy, Default)]
pub struct SocializeAtBell;

impl SocializeAtBell {
    /// `create()`.
    #[must_use]
    pub fn create() -> OneShot<Self> {
        OneShot::new(SocializeAtBell)
    }
}

impl<M: MobHost> Trigger<M> for SocializeAtBell {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        if brain
            .memory_internal(MemoryModuleType::WalkTarget)
            .is_none()
            || brain
                .memory_internal(MemoryModuleType::LookTarget)
                .is_none()
        {
            return false;
        }
        let Some(Some(v)) = brain.memory_internal(MemoryModuleType::MeetingPoint) else {
            return false;
        };
        let Some(gp) = v.as_global_pos() else {
            return false;
        };
        let Some(Some(_)) = brain.memory_internal(MemoryModuleType::NearestVisibleLivingEntities)
        else {
            return false;
        };
        if brain.memory_internal(MemoryModuleType::InteractionTarget) != Some(None) {
            return false;
        }
        if cx.mob.level_random_next_int(100) == 0
            && cx.mob.dimension() == gp.dimension
            && closer_to_center_than(gp.pos, cx.mob.position(), 4.0)
            && nearest_visible_find_closest(brain, cx, |cx, e| cx.mob.entity_kind_is(e, "villager"))
                .is_some()
        {
            let found = nearest_visible_find_closest(brain, cx, |cx, e| {
                cx.mob.entity_kind_is(e, "villager")
                    && cx.mob.distance_to_sqr_entity(e).is_some_and(|d| d <= 32.0)
            });
            if let Some(e) = found {
                brain.set_memory_value(MemoryModuleType::InteractionTarget, MemoryValue::Entity(e));
                brain.set_memory_value(
                    MemoryModuleType::LookTarget,
                    MemoryValue::PositionTracker(PositionTracker::entity(e, true)),
                );
                brain.set_memory_value(
                    MemoryModuleType::WalkTarget,
                    MemoryValue::WalkTarget(WalkTarget::new(
                        PositionTracker::entity(e, false),
                        SPEED_MODIFIER,
                        1,
                    )),
                );
            }
            return true;
        }
        false
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![
            MemoryModuleType::WalkTarget,
            MemoryModuleType::LookTarget,
            MemoryModuleType::MeetingPoint,
            MemoryModuleType::NearestVisibleLivingEntities,
            MemoryModuleType::InteractionTarget,
        ]
    }

    fn name(&self) -> &'static str {
        "SocializeAtBell"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::{GlobalPos, MemoryModuleType as T, NearestVisibleLivingEntities};
    use crate::brain::Brain;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};
    use rustmc_data::{BlockPos, Dimension};
    use rustmc_worldgen::rng::{LegacyRandomSource, RandomSource};

    #[test]
    fn one_in_a_hundred_near_the_bell_picks_the_closest_villager_within_32_sq() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 10));
        let mut twin = LegacyRandomSource::new(10 ^ 0x5DEECE66D);
        cx.mob.others = vec![
            (EntityClass::Living, EntityFacts::living(2, 2.5, 1.0, 0.5)),
            (EntityClass::Living, EntityFacts::living(3, 4.5, 1.0, 0.5)),
            (EntityClass::Living, EntityFacts::living(4, 7.5, 1.0, 0.5)),
        ];
        cx.mob.entity_kinds = vec![(3, "villager"), (4, "villager")];
        cx.mob.visible = vec![2, 3, 4];
        let mut brain = Brain::<TestMob>::new(
            &[
                T::WalkTarget,
                T::LookTarget,
                T::MeetingPoint,
                T::NearestVisibleLivingEntities,
                T::InteractionTarget,
            ],
            vec![],
        )
        .state;
        brain.set_memory_value(
            T::MeetingPoint,
            MemoryValue::GlobalPos(GlobalPos {
                dimension: Dimension::Overworld,
                pos: BlockPos { x: 2, y: 1, z: 0 },
            }),
        );
        brain.set_memory_value(
            T::NearestVisibleLivingEntities,
            MemoryValue::NearestVisible(NearestVisibleLivingEntities::new(vec![2, 3, 4])),
        );
        let mut b = SocializeAtBell::create();
        let mut t = 0;
        loop {
            let draw = twin.next_i32_bound(100);
            let r = b.try_start(&mut brain, &mut cx, t);
            assert_eq!(r, draw == 0, "tick {t}");
            t += 1;
            if r {
                break;
            }
            assert!(t < 2000);
        }
        assert_eq!(
            brain.get_memory(T::InteractionTarget),
            Some(&MemoryValue::Entity(3))
        );
        assert_eq!(
            brain.get_memory(T::WalkTarget).unwrap().as_walk_target(),
            Some(&WalkTarget::new(PositionTracker::entity(3, false), 0.3, 1))
        );
    }
}
