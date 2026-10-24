//! `net.minecraft.world.entity.ai.behavior.CrossbowAttack`, from the 26.2
//! jar. `Behavior<E extends Mob & CrossbowAttackMob>` with entry
//! `{LOOK_TARGET: REGISTERED, ATTACK_TARGET: VALUE_PRESENT}` and a fixed
//! `TIMEOUT = 1200`; `crossbowState` starts `UNCHARGED`.
//!
//! ```text
//! checkExtraStartConditions (0..37): t = ATTACK_TARGET.get(); mob.isHolding(CROSSBOW) && BehaviorUtils.canSee(mob, t) && isWithinAttackRange(mob, t, 0)
//! canStillUse (0..27): ATTACK_TARGET present && checkExtraStartConditions
//! tick (0..21): t; LOOK_TARGET = EntityTracker(t, true); crossbowAttack(mob, t)
//! stop (0..45): if mob.isUsingItem(): stopUsingItem; if isHolding(CROSSBOW): setChargingCrossbow(false); useItem.set(CHARGED_PROJECTILES, EMPTY)
//! crossbowAttack (0..193):
//!   UNCHARGED: startUsingItem(weaponHoldingHand(CROSSBOW)); state = CHARGING; setChargingCrossbow(true)
//!   CHARGING:  if !isUsingItem(): state = UNCHARGED                       <- and falls through to the check below
//!              if getTicksUsingItem() >= CrossbowItem.getChargeDuration(useItem, mob):
//!                  releaseUsingItem(); state = CHARGED; attackDelay = 20 + mob.getRandom().nextInt(20); setChargingCrossbow(false)   <- MOB random
//!   CHARGED:   if --attackDelay == 0: state = READY_TO_ATTACK
//!   READY_TO_ATTACK: performRangedAttack(t, 1.0f); state = UNCHARGED
//! ```
//!
//! One branch per tick; the `CrossbowState` enum is `UNCHARGED, CHARGING,
//! CHARGED, READY_TO_ATTACK` in ordinal order.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::behaviors::behavior_utils::{can_see, is_within_attack_range};
use crate::brain::memory::{MemoryModuleType, MemoryStatus, MemoryValue, PositionTracker};
use crate::brain::BrainState;
use crate::host::MobHost;
use rustmc_worldgen::rng::RandomSource;

/// `TIMEOUT`.
pub const TIMEOUT: i32 = 1200;

/// `CrossbowAttack$CrossbowState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CrossbowState {
    #[default]
    Uncharged,
    Charging,
    Charged,
    ReadyToAttack,
}

/// `CrossbowAttack`'s body; build one with [`CrossbowAttack::new`].
#[derive(Debug, Clone, Copy, Default)]
pub struct CrossbowAttack {
    pub attack_delay: i32,
    pub crossbow_state: CrossbowState,
}

impl CrossbowAttack {
    /// `CrossbowAttack()`.
    #[must_use]
    pub fn new() -> Behavior<CrossbowAttack> {
        Behavior::with_duration(
            CrossbowAttack::default(),
            vec![
                (MemoryModuleType::LookTarget, MemoryStatus::Registered),
                (MemoryModuleType::AttackTarget, MemoryStatus::ValuePresent),
            ],
            TIMEOUT,
        )
    }

    fn attack_target(brain: &BrainState) -> i32 {
        brain
            .get_memory(MemoryModuleType::AttackTarget)
            .and_then(MemoryValue::as_entity)
            .expect("ATTACK_TARGET is VALUE_PRESENT")
    }

    fn crossbow_attack<M: MobHost>(&mut self, cx: &mut MobAi<M>, target: i32) {
        match self.crossbow_state {
            CrossbowState::Uncharged => {
                cx.mob.start_using_crossbow_hand();
                self.crossbow_state = CrossbowState::Charging;
                cx.mob.set_charging_crossbow(true);
            }
            CrossbowState::Charging => {
                if !cx.mob.is_using_item() {
                    self.crossbow_state = CrossbowState::Uncharged;
                }
                if cx.mob.ticks_using_item() >= cx.mob.crossbow_charge_duration() {
                    cx.mob.release_using_item();
                    self.crossbow_state = CrossbowState::Charged;
                    self.attack_delay = 20 + cx.mob.random().next_i32_bound(20);
                    cx.mob.set_charging_crossbow(false);
                }
            }
            CrossbowState::Charged => {
                self.attack_delay -= 1;
                if self.attack_delay == 0 {
                    self.crossbow_state = CrossbowState::ReadyToAttack;
                }
            }
            CrossbowState::ReadyToAttack => {
                cx.mob.perform_ranged_attack(target, 1.0);
                self.crossbow_state = CrossbowState::Uncharged;
            }
        }
    }
}

impl<M: MobHost> BehaviorBody<M> for CrossbowAttack {
    fn check_extra_start_conditions(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        let target = Self::attack_target(brain);
        cx.mob.is_holding_crossbow()
            && can_see(brain, cx, target)
            && is_within_attack_range(cx, target, 0)
    }

    fn can_still_use(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        brain.has_memory_value(MemoryModuleType::AttackTarget)
            && self.check_extra_start_conditions(brain, cx)
    }

    fn tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        let target = Self::attack_target(brain);
        brain.set_memory_value(
            MemoryModuleType::LookTarget,
            MemoryValue::PositionTracker(PositionTracker::entity(target, true)),
        );
        self.crossbow_attack(cx, target);
    }

    fn stop(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        if cx.mob.is_using_item() {
            cx.mob.stop_using_item();
        }
        if cx.mob.is_holding_crossbow() {
            cx.mob.set_charging_crossbow(false);
            cx.mob.clear_charged_projectiles();
        }
    }

    fn name(&self) -> &'static str {
        "CrossbowAttack"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::{MemoryModuleType as T, NearestVisibleLivingEntities};
    use crate::brain::Brain;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};
    use rustmc_worldgen::rng::LegacyRandomSource;

    #[test]
    fn charges_waits_twenty_plus_next_int_twenty_then_shoots() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 5));
        let mut twin = LegacyRandomSource::new(5);
        cx.mob.others = vec![(EntityClass::Living, EntityFacts::living(7, 4.5, 1.0, 0.5))];
        cx.mob.visible = vec![7];
        cx.mob.holding_crossbow = true;
        cx.mob.crossbow_charge_duration = 25;
        cx.mob.projectile_weapon_range = Some(8);
        cx.mob.holding_usable_non_melee_weapon = true;
        let mut brain = Brain::<TestMob>::new(
            &[
                T::LookTarget,
                T::AttackTarget,
                T::NearestVisibleLivingEntities,
            ],
            vec![],
        )
        .state;
        brain.set_memory_value(T::AttackTarget, MemoryValue::Entity(7));
        brain.set_memory_value(
            T::NearestVisibleLivingEntities,
            MemoryValue::NearestVisible(NearestVisibleLivingEntities::new(vec![7])),
        );
        let mut b = CrossbowAttack::new();
        assert!(b.try_start(&mut brain, &mut cx, 0));
        b.tick_or_stop(&mut brain, &mut cx, 1);
        assert_eq!(b.body.crossbow_state, CrossbowState::Charging);
        cx.mob.using_item = true;
        cx.mob.ticks_using_item = 24;
        b.tick_or_stop(&mut brain, &mut cx, 2);
        assert_eq!(b.body.crossbow_state, CrossbowState::Charging, "24 < 25");
        cx.mob.ticks_using_item = 25;
        b.tick_or_stop(&mut brain, &mut cx, 3);
        assert_eq!(b.body.crossbow_state, CrossbowState::Charged);
        let delay = 20 + twin.next_i32_bound(20);
        assert_eq!(b.body.attack_delay, delay);
        let mut t = 4;
        for _ in 1..delay {
            b.tick_or_stop(&mut brain, &mut cx, t);
            t += 1;
        }
        assert_eq!(b.body.crossbow_state, CrossbowState::Charged);
        b.tick_or_stop(&mut brain, &mut cx, t);
        assert_eq!(b.body.crossbow_state, CrossbowState::ReadyToAttack);
        b.tick_or_stop(&mut brain, &mut cx, t + 1);
        assert_eq!(b.body.crossbow_state, CrossbowState::Uncharged);
        assert!(
            cx.mob
                .calls
                .iter()
                .any(|c| c.contains("RangedAttack(7, 1)")),
            "{:?}",
            cx.mob.calls
        );
    }
}
