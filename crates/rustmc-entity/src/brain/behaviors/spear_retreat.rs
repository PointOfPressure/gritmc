//! `net.minecraft.world.entity.ai.behavior.SpearRetreat`, from the 26.2
//! jar. `Behavior<PathfinderMob>` with entry `{SPEAR_STATUS:
//! VALUE_PRESENT}` and a fixed duration of `MAX_FLEEING_TIME = 100`.
//! `MIN_COOLDOWN_DISTANCE = 9`, `MAX_COOLDOWN_DISTANCE = 11`.
//!
//! ```text
//! checkExtraStartConditions (0..144):
//!   if !ableToAttack || isUsingItem: return false
//!   if SPEAR_STATUS.orElse(APPROACH) != RETREAT: return false
//!   t; d = distanceToSqr(t.x, t.y, t.z); bonus = isPassenger ? 2 : 0; dist = sqrt(d)
//!   v = LandRandomPos.getPosAway(mob, max(0.0, (9 + bonus) - dist), max(1.0, (11 + bonus) - dist), 7, t.position())   <- MOB random
//!   if v == null: return false
//!   SPEAR_FLEEING_POSITION = v; return true
//! start (0..27): setAggressive(true); SPEAR_FLEEING_TIME = 0
//! canStillUse (0..68): SPEAR_FLEEING_TIME.orElse(100) < 100 && SPEAR_FLEEING_POSITION present && !navigation.isDone() && ableToAttack
//! tick (0..116): t; speed = rootVehicle Mob ? chargeSpeedModifier : 1; LOOK_TARGET = EntityTracker(t, true)
//!   SPEAR_FLEEING_TIME = orElse(0) + 1; SPEAR_FLEEING_POSITION.ifPresent(v -> navigation.moveTo(v.x, v.y, v.z, speed * speedModifierWhenRepositioning))
//! stop (0..46): navigation.stop(); setAggressive(false); stopUsingItem(); erase SPEAR_FLEEING_TIME, SPEAR_FLEEING_POSITION, SPEAR_STATUS
//! ```
//!
//! The flee position is drawn in `checkExtraStartConditions`, so a refused
//! start (no position found) has still spent the draws.

use crate::ai::mob_ai::MobAi;
use crate::ai::random_pos::land_get_pos_away_dist;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::behaviors::spear_approach::{able_to_attack, charge_speed, spear_target};
use crate::brain::behaviors::spear_attack::spear_status;
use crate::brain::memory::{
    MemoryModuleType, MemoryStatus, MemoryValue, PositionTracker, SpearStatus,
};
use crate::brain::BrainState;
use crate::host::MobHost;
use crate::phys::Vec3;

/// `MIN_COOLDOWN_DISTANCE`.
pub const MIN_COOLDOWN_DISTANCE: i32 = 9;
/// `MAX_COOLDOWN_DISTANCE`.
pub const MAX_COOLDOWN_DISTANCE: i32 = 11;
/// `MAX_FLEEING_TIME`.
pub const MAX_FLEEING_TIME: i32 = 100;

/// `SpearRetreat`'s body; build one with [`SpearRetreat::new`].
#[derive(Debug, Clone, Copy)]
pub struct SpearRetreat {
    pub speed_modifier_when_repositioning: f64,
}

impl SpearRetreat {
    /// `SpearRetreat(double)`.
    #[must_use]
    pub fn new(speed_modifier_when_repositioning: f64) -> Behavior<SpearRetreat> {
        Behavior::with_duration(
            SpearRetreat {
                speed_modifier_when_repositioning,
            },
            vec![(MemoryModuleType::SpearStatus, MemoryStatus::ValuePresent)],
            MAX_FLEEING_TIME,
        )
    }
}

impl<M: MobHost> BehaviorBody<M> for SpearRetreat {
    fn check_extra_start_conditions(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        if !able_to_attack(brain, cx) || cx.mob.is_using_item() {
            return false;
        }
        if spear_status(brain) != SpearStatus::Retreat {
            return false;
        }
        let Some(target) = spear_target(brain).and_then(|t| cx.mob.entity(t)) else {
            return false;
        };
        let d = cx
            .mob
            .position()
            .distance_to_sqr3(target.x, target.y, target.z);
        let bonus = if cx.mob.is_passenger() { 2 } else { 0 };
        let dist = d.sqrt();
        let v = land_get_pos_away_dist(
            cx,
            (f64::from(MIN_COOLDOWN_DISTANCE + bonus) - dist).max(0.0),
            (f64::from(MAX_COOLDOWN_DISTANCE + bonus) - dist).max(1.0),
            7,
            Vec3::new(target.x, target.y, target.z),
        );
        let Some(v) = v else {
            return false;
        };
        brain.set_memory_value(MemoryModuleType::SpearFleeingPosition, MemoryValue::Vec3(v));
        true
    }

    fn start(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        cx.mob.set_aggressive(true);
        brain.set_memory_value(MemoryModuleType::SpearFleeingTime, MemoryValue::Int(0));
    }

    fn can_still_use(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        let fleeing = brain
            .get_memory(MemoryModuleType::SpearFleeingTime)
            .and_then(MemoryValue::as_int)
            .unwrap_or(MAX_FLEEING_TIME);
        fleeing < MAX_FLEEING_TIME
            && brain.has_memory_value(MemoryModuleType::SpearFleeingPosition)
            && !cx.navigation.is_done()
            && able_to_attack(brain, cx)
    }

    fn tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        let Some(t) = spear_target(brain) else {
            return;
        };
        let speed = f64::from(charge_speed(cx));
        brain.set_memory_value(
            MemoryModuleType::LookTarget,
            MemoryValue::PositionTracker(PositionTracker::entity(t, true)),
        );
        let fleeing = brain
            .get_memory(MemoryModuleType::SpearFleeingTime)
            .and_then(MemoryValue::as_int)
            .unwrap_or(0);
        brain.set_memory_value(
            MemoryModuleType::SpearFleeingTime,
            MemoryValue::Int(fleeing + 1),
        );
        if let Some(v) = brain
            .get_memory(MemoryModuleType::SpearFleeingPosition)
            .and_then(MemoryValue::as_vec3)
        {
            cx.navigation.move_to(
                &mut cx.mob,
                v.x,
                v.y,
                v.z,
                speed * self.speed_modifier_when_repositioning,
            );
        }
    }

    fn stop(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        cx.navigation.stop();
        cx.mob.set_aggressive(false);
        cx.mob.stop_using_item();
        brain.erase_memory(MemoryModuleType::SpearFleeingTime);
        brain.erase_memory(MemoryModuleType::SpearFleeingPosition);
        brain.erase_memory(MemoryModuleType::SpearStatus);
    }

    fn name(&self) -> &'static str {
        "SpearRetreat"
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

    #[test]
    fn draws_a_flee_position_away_from_the_target_and_clears_the_spear_state_on_stop() {
        let mut level = GridLevel::new();
        level.floor(-24, -24, 24, 24, 0);
        let mut cx = MobAi::new(TestMob::new(level, 0.5, 1.0, 0.5, 11));
        cx.mob.others = vec![(EntityClass::Living, EntityFacts::living(7, 3.5, 1.0, 0.5))];
        cx.mob.kinetic_weapon = Some(3);
        let mut brain = Brain::<TestMob>::new(
            &[
                T::SpearStatus,
                T::AttackTarget,
                T::LookTarget,
                T::SpearFleeingTime,
                T::SpearFleeingPosition,
            ],
            vec![],
        )
        .state;
        brain.set_memory_value(T::AttackTarget, MemoryValue::Entity(7));
        brain.set_memory_value(
            T::SpearStatus,
            MemoryValue::SpearStatus(SpearStatus::Charging),
        );
        let mut b = SpearRetreat::new(1.2);
        assert!(
            !b.try_start(&mut brain, &mut cx, 0),
            "CHARGING is not RETREAT: no draw"
        );
        brain.set_memory_value(
            T::SpearStatus,
            MemoryValue::SpearStatus(SpearStatus::Retreat),
        );
        let mut twin = MobAi::new(TestMob::new(cx.mob.level.clone(), 0.5, 1.0, 0.5, 11));
        let expect = land_get_pos_away_dist(
            &mut twin,
            9.0 - 3.0,
            11.0 - 3.0,
            7,
            Vec3::new(3.5, 1.0, 0.5),
        );
        let started = b.try_start(&mut brain, &mut cx, 0);
        assert_eq!(started, expect.is_some());
        assert_eq!(
            brain
                .get_memory(T::SpearFleeingPosition)
                .map(|m| m.as_vec3()),
            expect.map(Some)
        );
        if started {
            assert_eq!(
                brain.get_memory(T::SpearFleeingTime),
                Some(&MemoryValue::Int(0))
            );
            b.tick_or_stop(&mut brain, &mut cx, 1);
            assert_eq!(
                brain.get_memory(T::SpearFleeingTime),
                Some(&MemoryValue::Int(1))
            );
            b.do_stop(&mut brain, &mut cx, 2);
        }
        assert!(!brain.has_memory_value(T::SpearStatus) || !started);
        assert!(!cx.mob.aggressive || !started);
    }
}
