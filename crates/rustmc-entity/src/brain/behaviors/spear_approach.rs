//! `net.minecraft.world.entity.ai.behavior.SpearApproach`, from the 26.2
//! jar. `Behavior<PathfinderMob>` with entry `{SPEAR_STATUS:
//! VALUE_ABSENT}`, the default duration and `timedOut` overridden to
//! `false`. `approachDistanceSq = approachDistance * approachDistance`
//! (`fmul` at 21).
//!
//! ```text
//! ableToAttack (0..26): ATTACK_TARGET present && mainHandItem.has(KINETIC_WEAPON)
//! checkExtraStartConditions (0..20): ableToAttack && !mob.isUsingItem()
//! start (0..26): setAggressive(true); SPEAR_STATUS = APPROACH
//! canStillUse (0..21): ableToAttack && farEnough   (distanceToSqr(target x, y, z) > (double) approachDistanceSq)
//! tick (0..75): t; speed = rootVehicle instanceof Mob ? chargeSpeedModifier() : 1.0f
//!   LOOK_TARGET = EntityTracker(t, true); navigation.moveTo(t, speed * speedModifierWhenRepositioning)
//! stop (0..20): navigation.stop(); SPEAR_STATUS = CHARGING
//! ```

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::memory::{
    MemoryModuleType, MemoryStatus, MemoryValue, PositionTracker, SpearStatus,
};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `SpearApproach`'s body; build one with [`SpearApproach::new`].
#[derive(Debug, Clone, Copy)]
pub struct SpearApproach {
    pub speed_modifier_when_repositioning: f64,
    pub approach_distance_sq: f32,
}

/// `ATTACK_TARGET.orElse(null)`, shared by the three spear behaviours.
pub fn spear_target(brain: &BrainState) -> Option<i32> {
    brain
        .get_memory(MemoryModuleType::AttackTarget)
        .and_then(MemoryValue::as_entity)
}

/// `ableToAttack(mob)`: a target and a kinetic weapon in the main hand.
pub fn able_to_attack<M: MobHost>(brain: &BrainState, cx: &MobAi<M>) -> bool {
    spear_target(brain).is_some() && cx.mob.main_hand_is_kinetic_weapon()
}

/// `rootVehicle instanceof Mob ? chargeSpeedModifier() : 1.0f`.
pub fn charge_speed<M: MobHost>(cx: &MobAi<M>) -> f32 {
    cx.mob.root_vehicle_charge_speed_modifier().unwrap_or(1.0)
}

impl SpearApproach {
    /// `SpearApproach(double, float)`.
    #[must_use]
    pub fn new(
        speed_modifier_when_repositioning: f64,
        approach_distance: f32,
    ) -> Behavior<SpearApproach> {
        Behavior::new(
            SpearApproach {
                speed_modifier_when_repositioning,
                approach_distance_sq: approach_distance * approach_distance,
            },
            vec![(MemoryModuleType::SpearStatus, MemoryStatus::ValueAbsent)],
        )
    }
}

impl<M: MobHost> BehaviorBody<M> for SpearApproach {
    fn check_extra_start_conditions(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        able_to_attack(brain, cx) && !cx.mob.is_using_item()
    }

    fn start(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        cx.mob.set_aggressive(true);
        brain.set_memory_value(
            MemoryModuleType::SpearStatus,
            MemoryValue::SpearStatus(SpearStatus::Approach),
        );
    }

    fn can_still_use(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        able_to_attack(brain, cx)
            && spear_target(brain).is_some_and(|t| {
                cx.mob
                    .distance_to_sqr_entity(t)
                    .is_some_and(|d| d > f64::from(self.approach_distance_sq))
            })
    }

    fn tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        let Some(t) = spear_target(brain) else {
            return;
        };
        let speed = charge_speed(cx);
        brain.set_memory_value(
            MemoryModuleType::LookTarget,
            MemoryValue::PositionTracker(PositionTracker::entity(t, true)),
        );
        if let Some(e) = cx.mob.entity(t) {
            let pos = e.block_position();
            cx.navigation.move_to_entity_at(
                &mut cx.mob,
                pos,
                f64::from(speed) * self.speed_modifier_when_repositioning,
            );
        }
    }

    fn stop(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        cx.navigation.stop();
        brain.set_memory_value(
            MemoryModuleType::SpearStatus,
            MemoryValue::SpearStatus(SpearStatus::Charging),
        );
    }

    fn timed_out(&self, _end_timestamp: i64, _time: i64) -> bool {
        false
    }

    fn name(&self) -> &'static str {
        "SpearApproach"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::{BehaviorControl, Status};
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn approaches_until_within_the_distance_then_hands_over_to_charging() {
        let mut level = GridLevel::new();
        level.floor(-16, -16, 16, 16, 0);
        let mut cx = MobAi::new(TestMob::new(level, 0.5, 1.0, 0.5, 1));
        cx.mob.others = vec![(EntityClass::Living, EntityFacts::living(7, 8.5, 1.0, 0.5))];
        cx.mob.kinetic_weapon = Some(15);
        let mut brain =
            Brain::<TestMob>::new(&[T::SpearStatus, T::AttackTarget, T::LookTarget], vec![]).state;
        let mut b = SpearApproach::new(1.2, 4.0);
        assert!(!b.try_start(&mut brain, &mut cx, 0), "no target");
        brain.set_memory_value(T::AttackTarget, MemoryValue::Entity(7));
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(
            brain.get_memory(T::SpearStatus),
            Some(&MemoryValue::SpearStatus(SpearStatus::Approach))
        );
        assert!(cx.mob.aggressive);
        b.tick_or_stop(&mut brain, &mut cx, 1);
        assert!(!cx.navigation.is_done(), "moving to the target");
        cx.mob.others[0].1.x = 4.5;
        b.tick_or_stop(&mut brain, &mut cx, 2);
        assert_eq!(
            BehaviorControl::<TestMob>::status(&b),
            Status::Stopped,
            "16 is not > 16"
        );
        assert_eq!(
            brain.get_memory(T::SpearStatus),
            Some(&MemoryValue::SpearStatus(SpearStatus::Charging))
        );
        assert!(cx.navigation.is_done());
    }
}
