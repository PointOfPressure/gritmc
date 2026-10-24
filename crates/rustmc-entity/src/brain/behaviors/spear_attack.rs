//! `net.minecraft.world.entity.ai.behavior.SpearAttack`, from the 26.2 jar.
//! `Behavior<PathfinderMob>` with entry `{SPEAR_STATUS: VALUE_PRESENT}`,
//! the default duration and `timedOut` overridden to `false`.
//! `MIN_REPOSITION_DISTANCE = 6`, `MAX_REPOSITION_DISTANCE = 7`;
//! `targetInRangeRadiusSq = radius * radius`.
//!
//! ```text
//! getKineticWeaponUseDuration: mainHandItem.get(KINETIC_WEAPON).map(useDuration).orElse(0)
//! checkExtraStartConditions (0..42): SPEAR_STATUS.orElse(APPROACH) == CHARGING && ableToAttack && !isUsingItem
//! start (0..48): setAggressive(true); SPEAR_ENGAGE_TIME = useDuration; erase SPEAR_CHARGE_POSITION; startUsingItem(MAIN_HAND)
//! canStillUse (0..39): SPEAR_ENGAGE_TIME.orElse(0) > 0 && ableToAttack
//! tick (0..298): t; d = distanceToSqr(t.x, t.y, t.z); speed = rootVehicle Mob ? chargeSpeedModifier : 1; bonus = isPassenger ? 2 : 0
//!   LOOK_TARGET = EntityTracker(t, true); SPEAR_ENGAGE_TIME = orElse(0) - 1
//!   charge = SPEAR_CHARGE_POSITION.orElse(null)
//!   if charge != null: navigation.moveTo(charge.x, charge.y, charge.z, speed * speedModifierWhenRepositioning)
//!                      if navigation.isDone(): erase SPEAR_CHARGE_POSITION
//!   else: navigation.moveTo(t, speed * speedModifierWhenCharging)
//!         if d < radiusSq || navigation.isDone():
//!             dist = sqrt(d); SPEAR_CHARGE_POSITION = LandRandomPos.getPosAway(mob, (6 + bonus) - dist, (7 + bonus) - dist, 7, t.position())   <- MOB random; null erases
//! stop (0..41): navigation.stop(); stopUsingItem(); erase SPEAR_CHARGE_POSITION, SPEAR_ENGAGE_TIME; SPEAR_STATUS = RETREAT
//! ```

use crate::ai::mob_ai::MobAi;
use crate::ai::random_pos::land_get_pos_away_dist;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::behaviors::spear_approach::{able_to_attack, charge_speed, spear_target};
use crate::brain::memory::{
    MemoryModuleType, MemoryStatus, MemoryValue, PositionTracker, SpearStatus,
};
use crate::brain::BrainState;
use crate::host::MobHost;
use crate::phys::Vec3;

/// `MIN_REPOSITION_DISTANCE`.
pub const MIN_REPOSITION_DISTANCE: i32 = 6;
/// `MAX_REPOSITION_DISTANCE`.
pub const MAX_REPOSITION_DISTANCE: i32 = 7;

/// `SpearAttack`'s body; build one with [`SpearAttack::new`].
#[derive(Debug, Clone, Copy)]
pub struct SpearAttack {
    pub speed_modifier_when_charging: f64,
    pub speed_modifier_when_repositioning: f64,
    pub target_in_range_radius_sq: f32,
}

/// `SPEAR_STATUS.orElse(APPROACH)`.
pub fn spear_status(brain: &BrainState) -> SpearStatus {
    match brain.get_memory(MemoryModuleType::SpearStatus) {
        Some(MemoryValue::SpearStatus(s)) => *s,
        _ => SpearStatus::Approach,
    }
}

fn engage_time(brain: &BrainState) -> i32 {
    brain
        .get_memory(MemoryModuleType::SpearEngageTime)
        .and_then(MemoryValue::as_int)
        .unwrap_or(0)
}

impl SpearAttack {
    /// `SpearAttack(double, double, float)`.
    #[must_use]
    pub fn new(
        speed_modifier_when_charging: f64,
        speed_modifier_when_repositioning: f64,
        target_in_range_radius: f32,
    ) -> Behavior<SpearAttack> {
        Behavior::new(
            SpearAttack {
                speed_modifier_when_charging,
                speed_modifier_when_repositioning,
                target_in_range_radius_sq: target_in_range_radius * target_in_range_radius,
            },
            vec![(MemoryModuleType::SpearStatus, MemoryStatus::ValuePresent)],
        )
    }
}

impl<M: MobHost> BehaviorBody<M> for SpearAttack {
    fn check_extra_start_conditions(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        spear_status(brain) == SpearStatus::Charging
            && able_to_attack(brain, cx)
            && !cx.mob.is_using_item()
    }

    fn start(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        cx.mob.set_aggressive(true);
        let duration = cx.mob.kinetic_weapon_use_duration();
        brain.set_memory_value(
            MemoryModuleType::SpearEngageTime,
            MemoryValue::Int(duration),
        );
        brain.erase_memory(MemoryModuleType::SpearChargePosition);
        cx.mob.start_using_main_hand();
    }

    fn can_still_use(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        engage_time(brain) > 0 && able_to_attack(brain, cx)
    }

    fn tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        let Some(t) = spear_target(brain) else {
            return;
        };
        let Some(target) = cx.mob.entity(t) else {
            return;
        };
        let d = cx
            .mob
            .position()
            .distance_to_sqr3(target.x, target.y, target.z);
        let speed = f64::from(charge_speed(cx));
        let bonus = if cx.mob.is_passenger() { 2 } else { 0 };
        brain.set_memory_value(
            MemoryModuleType::LookTarget,
            MemoryValue::PositionTracker(PositionTracker::entity(t, true)),
        );
        let engage = engage_time(brain) - 1;
        brain.set_memory_value(MemoryModuleType::SpearEngageTime, MemoryValue::Int(engage));
        let charge = brain
            .get_memory(MemoryModuleType::SpearChargePosition)
            .and_then(MemoryValue::as_vec3);
        if let Some(c) = charge {
            cx.navigation.move_to(
                &mut cx.mob,
                c.x,
                c.y,
                c.z,
                speed * self.speed_modifier_when_repositioning,
            );
            if cx.navigation.is_done() {
                brain.erase_memory(MemoryModuleType::SpearChargePosition);
            }
        } else {
            cx.navigation.move_to_entity_at(
                &mut cx.mob,
                target.block_position(),
                speed * self.speed_modifier_when_charging,
            );
            if d < f64::from(self.target_in_range_radius_sq) || cx.navigation.is_done() {
                let dist = d.sqrt();
                let v = land_get_pos_away_dist(
                    cx,
                    f64::from(MIN_REPOSITION_DISTANCE + bonus) - dist,
                    f64::from(MAX_REPOSITION_DISTANCE + bonus) - dist,
                    7,
                    Vec3::new(target.x, target.y, target.z),
                );
                brain.set_memory(
                    MemoryModuleType::SpearChargePosition,
                    v.map(MemoryValue::Vec3),
                );
            }
        }
    }

    fn stop(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        cx.navigation.stop();
        cx.mob.stop_using_item();
        brain.erase_memory(MemoryModuleType::SpearChargePosition);
        brain.erase_memory(MemoryModuleType::SpearEngageTime);
        brain.set_memory_value(
            MemoryModuleType::SpearStatus,
            MemoryValue::SpearStatus(SpearStatus::Retreat),
        );
    }

    fn timed_out(&self, _end_timestamp: i64, _time: i64) -> bool {
        false
    }

    fn name(&self) -> &'static str {
        "SpearAttack"
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
    fn charges_for_the_weapons_use_duration_repositioning_once_in_range_then_retreats() {
        let mut level = GridLevel::new();
        level.floor(-24, -24, 24, 24, 0);
        let mut cx = MobAi::new(TestMob::new(level, 0.5, 1.0, 0.5, 9));
        cx.mob.others = vec![(EntityClass::Living, EntityFacts::living(7, 2.5, 1.0, 0.5))];
        cx.mob.kinetic_weapon = Some(3);
        let mut brain = Brain::<TestMob>::new(
            &[
                T::SpearStatus,
                T::AttackTarget,
                T::LookTarget,
                T::SpearEngageTime,
                T::SpearChargePosition,
            ],
            vec![],
        )
        .state;
        brain.set_memory_value(T::AttackTarget, MemoryValue::Entity(7));
        let mut b = SpearAttack::new(1.5, 1.2, 3.0);
        assert!(
            !b.try_start(&mut brain, &mut cx, 0),
            "status defaults to APPROACH"
        );
        brain.set_memory_value(
            T::SpearStatus,
            MemoryValue::SpearStatus(SpearStatus::Charging),
        );
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(
            brain.get_memory(T::SpearEngageTime),
            Some(&MemoryValue::Int(3))
        );
        // In range (4 < 9): the first tick picks a reposition target on the mob random.
        let mut twin = MobAi::new(TestMob::new(cx.mob.level.clone(), 0.5, 1.0, 0.5, 9));
        let _ = twin.navigation.move_to_entity_at(
            &mut twin.mob,
            rustmc_data::BlockPos { x: 2, y: 1, z: 0 },
            1.5,
        );
        let expect =
            land_get_pos_away_dist(&mut twin, 6.0 - 2.0, 7.0 - 2.0, 7, Vec3::new(2.5, 1.0, 0.5));
        b.tick_or_stop(&mut brain, &mut cx, 1);
        assert_eq!(
            brain.get_memory(T::SpearEngageTime),
            Some(&MemoryValue::Int(2))
        );
        assert_eq!(
            brain
                .get_memory(T::SpearChargePosition)
                .map(|m| m.as_vec3()),
            expect.map(Some)
        );
        b.tick_or_stop(&mut brain, &mut cx, 2);
        b.tick_or_stop(&mut brain, &mut cx, 3);
        assert_eq!(
            brain.get_memory(T::SpearEngageTime),
            Some(&MemoryValue::Int(0))
        );
        b.tick_or_stop(&mut brain, &mut cx, 4);
        assert_eq!(
            BehaviorControl::<TestMob>::status(&b),
            Status::Stopped,
            "engage time spent"
        );
        assert_eq!(
            brain.get_memory(T::SpearStatus),
            Some(&MemoryValue::SpearStatus(SpearStatus::Retreat))
        );
        assert!(!brain.has_memory_value(T::SpearEngageTime));
    }
}
