//! `net.minecraft.world.entity.ai.behavior.GoToWantedItem`, from the 26.2
//! jar. A `BehaviorBuilder` one-shot over `group(registered(LOOK_TARGET),
//! hasWalkTargetMemoryModuleType ? registered(WALK_TARGET) : absent(WALK_TARGET),
//! present(NEAREST_VISIBLE_WANTED_ITEM), registered(ITEM_PICKUP_COOLDOWN_TICKS))`
//! (`lambda$create$3`, 0..119):
//!
//! ```text
//! item = wanted
//! if cooldown.tryGet().isEmpty() && predicate.test(mob)
//!    && item.closerThan(mob, (double) maxDistToWalk)                 <- distanceToSqr < d*d
//!    && mob.level().getWorldBorder().isWithinBounds(item.blockPosition())
//!    && mob.canPickUpLoot():
//!     walk = new WalkTarget(new EntityTracker(item, false), speed, 0)
//!     look.set(new EntityTracker(item, true)); walkAccessor.set(walk); return true
//! return false
//! ```
//!
//! `create(float, boolean, int)` is `create(e -> true, ...)`.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::behaviors::MobPredicate;
use crate::brain::memory::{MemoryModuleType, MemoryValue, PositionTracker, WalkTarget};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `GoToWantedItem`'s trigger.
pub struct GoToWantedItem<M: MobHost> {
    pub predicate: MobPredicate<M>,
    pub speed_modifier: f32,
    pub has_walk_target_memory_module_type: bool,
    pub max_dist_to_walk: i32,
}

impl<M: MobHost + 'static> GoToWantedItem<M> {
    /// `create(float, boolean, int)`.
    #[must_use]
    pub fn create(
        speed_modifier: f32,
        has_walk_target: bool,
        max_dist_to_walk: i32,
    ) -> OneShot<Self> {
        Self::create_with(
            Box::new(|_| true),
            speed_modifier,
            has_walk_target,
            max_dist_to_walk,
        )
    }

    /// `create(Predicate, float, boolean, int)`.
    #[must_use]
    pub fn create_with(
        predicate: MobPredicate<M>,
        speed_modifier: f32,
        has_walk_target_memory_module_type: bool,
        max_dist_to_walk: i32,
    ) -> OneShot<Self> {
        OneShot::new(GoToWantedItem {
            predicate,
            speed_modifier,
            has_walk_target_memory_module_type,
            max_dist_to_walk,
        })
    }
}

impl<M: MobHost> Trigger<M> for GoToWantedItem<M> {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        if brain
            .memory_internal(MemoryModuleType::LookTarget)
            .is_none()
        {
            return false;
        }
        let walk = brain.memory_internal(MemoryModuleType::WalkTarget);
        if self.has_walk_target_memory_module_type {
            if walk.is_none() {
                return false;
            }
        } else if walk != Some(None) {
            return false;
        }
        let Some(Some(item)) = brain.memory_internal(MemoryModuleType::NearestVisibleWantedItem)
        else {
            return false;
        };
        let Some(item) = item.as_entity() else {
            return false;
        };
        let Some(cooldown) = brain.memory_internal(MemoryModuleType::ItemPickupCooldownTicks)
        else {
            return false;
        };
        let cooldown_absent = cooldown.is_none();
        let d = f64::from(self.max_dist_to_walk);
        if cooldown_absent
            && (self.predicate)(cx)
            && cx
                .mob
                .distance_to_sqr_entity(item)
                .is_some_and(|sq| sq < d * d)
            && cx
                .mob
                .entity(item)
                .is_some_and(|e| cx.mob.world_border_contains(e.block_position()))
            && cx.mob.can_pick_up_loot()
        {
            let walk =
                WalkTarget::new(PositionTracker::entity(item, false), self.speed_modifier, 0);
            brain.set_memory_value(
                MemoryModuleType::LookTarget,
                MemoryValue::PositionTracker(PositionTracker::entity(item, true)),
            );
            brain.set_memory_value(MemoryModuleType::WalkTarget, MemoryValue::WalkTarget(walk));
            true
        } else {
            false
        }
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![
            MemoryModuleType::LookTarget,
            MemoryModuleType::WalkTarget,
            MemoryModuleType::NearestVisibleWantedItem,
            MemoryModuleType::ItemPickupCooldownTicks,
        ]
    }

    fn name(&self) -> &'static str {
        "GoToWantedItem"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    fn setup() -> (MobAi<TestMob>, BrainState) {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        cx.mob.others = vec![(EntityClass::Living, EntityFacts::living(7, 4.5, 1.0, 0.5))];
        cx.mob.can_pick_up_loot = true;
        let mut brain = Brain::<TestMob>::new(
            &[
                T::LookTarget,
                T::WalkTarget,
                T::NearestVisibleWantedItem,
                T::ItemPickupCooldownTicks,
            ],
            vec![],
        )
        .state;
        brain.set_memory_value(T::NearestVisibleWantedItem, MemoryValue::Entity(7));
        (cx, brain)
    }

    #[test]
    fn walks_to_a_wanted_item_within_range_unless_cooling_down() {
        let (mut cx, mut brain) = setup();
        let mut b = GoToWantedItem::<TestMob>::create(1.0, false, 9);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(
            brain.get_memory(T::WalkTarget).unwrap().as_walk_target(),
            Some(&WalkTarget::new(PositionTracker::entity(7, false), 1.0, 0))
        );
        assert!(
            !b.try_start(&mut brain, &mut cx, 1),
            "WALK_TARGET present and not allowed"
        );
        let mut b2 = GoToWantedItem::<TestMob>::create(1.0, true, 9);
        assert!(
            b2.try_start(&mut brain, &mut cx, 1),
            "the has-walk-target form only needs it registered"
        );
        brain.set_memory_value(T::ItemPickupCooldownTicks, MemoryValue::Int(5));
        assert!(!b2.try_start(&mut brain, &mut cx, 2), "cooling down");
    }

    #[test]
    fn the_range_is_strict_and_loot_pickup_gates_it() {
        let (mut cx, mut brain) = setup();
        let mut b = GoToWantedItem::<TestMob>::create(1.0, false, 4);
        assert!(!b.try_start(&mut brain, &mut cx, 0), "16 is not < 16");
        let mut b = GoToWantedItem::<TestMob>::create(1.0, false, 5);
        cx.mob.can_pick_up_loot = false;
        assert!(!b.try_start(&mut brain, &mut cx, 0));
    }
}
