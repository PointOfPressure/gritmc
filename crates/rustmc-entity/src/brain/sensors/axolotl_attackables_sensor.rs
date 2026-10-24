//! `net.minecraft.world.entity.ai.sensing.AxolotlAttackablesSensor`, from
//! the 26.2 jar. `NearestVisibleLivingEntitySensor` with
//! `getMemoryToSet() = NEAREST_ATTACKABLE`; `requires()` is the base's set
//! unioned with `{HAS_HUNTING_COOLDOWN}` (`Sets.union`, 0..13).
//! `TARGET_DETECTION_DISTANCE = 8.0f`.
//!
//! ```text
//! isMatchingEntity (0..47):  isClose(mob, e) && e.isInWater()
//!                              && (isHostileTarget(e) || isHuntTarget(mob, e))
//!                              && Sensor.isEntityAttackable(level, mob, e)
//! isClose (0..17):           e.distanceToSqr(mob) <= 64.0            <- a `<=`, and 64 not 8
//! isHostileTarget (0..7):    e.is(EntityTypeTags.AXOLOTL_ALWAYS_HOSTILES)
//! isHuntTarget (0..28):      !mob.getBrain().hasMemoryValue(HAS_HUNTING_COOLDOWN)
//!                              && e.is(EntityTypeTags.AXOLOTL_HUNT_TARGETS)
//! ```
//!
//! Two details the offsets settle. `isClose` compares against the
//! **squared** 64.0 with `dcmpg; ifgt`, so it is `<= 64.0` — an entity at
//! exactly 8 blocks passes, where the `closerThan` the frog sensor uses is
//! a strict `<`. And the `||` is between the two tag tests only: the water
//! test and `isEntityAttackable` gate both branches.

use crate::ai::mob_ai::MobAi;
use crate::brain::memory::MemoryModuleType;
use crate::brain::sensor::{
    attack_target_id, is_entity_attackable_with, Sensor, SensorBody, SensorType,
};
use crate::brain::sensors::nearest_visible_living_entity_sensor::{do_tick_with, requires_with};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `TARGET_DETECTION_DISTANCE`.
pub const TARGET_DETECTION_DISTANCE: f32 = 8.0;
/// `isClose`'s literal, already squared.
pub const CLOSE_DISTANCE_SQR: f64 = 64.0;
/// `EntityTypeTags.AXOLOTL_ALWAYS_HOSTILES`.
pub const AXOLOTL_ALWAYS_HOSTILES: &str = "axolotl_always_hostiles";
/// `EntityTypeTags.AXOLOTL_HUNT_TARGETS`.
pub const AXOLOTL_HUNT_TARGETS: &str = "axolotl_hunt_targets";

/// `AxolotlAttackablesSensor`.
#[derive(Debug, Clone, Copy, Default)]
pub struct AxolotlAttackablesSensor;

impl AxolotlAttackablesSensor {
    /// `SensorType.AXOLOTL_ATTACKABLES.create()`.
    #[must_use]
    pub fn new() -> Sensor<AxolotlAttackablesSensor> {
        Sensor::new(AxolotlAttackablesSensor)
    }
}

impl<M: MobHost> SensorBody<M> for AxolotlAttackablesSensor {
    fn do_tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) {
        let hunting_cooldown = brain.has_memory_value(MemoryModuleType::HasHuntingCooldown);
        let attack_target = attack_target_id(brain);
        do_tick_with(brain, cx, MemoryModuleType::NearestAttackable, |cx, id| {
            let close = cx
                .mob
                .distance_to_sqr_entity(id)
                .is_some_and(|d| d <= CLOSE_DISTANCE_SQR);
            if !close || !cx.mob.entity_in_water(id) {
                return false;
            }
            let hostile = cx.mob.entity_type_tag_is(id, AXOLOTL_ALWAYS_HOSTILES);
            let hunt = !hunting_cooldown && cx.mob.entity_type_tag_is(id, AXOLOTL_HUNT_TARGETS);
            if !(hostile || hunt) {
                return false;
            }
            let Some(facts) = cx.mob.entity(id) else {
                return false;
            };
            is_entity_attackable_with(attack_target, cx, &facts)
        });
    }

    fn requires(&self) -> Vec<MemoryModuleType> {
        let mut r = requires_with(MemoryModuleType::NearestAttackable);
        r.push(MemoryModuleType::HasHuntingCooldown);
        r
    }

    fn sensor_type(&self) -> SensorType {
        SensorType::AxolotlAttackables
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::memory::{MemoryModuleType as T, MemoryValue, NearestVisibleLivingEntities};
    use crate::brain::Brain;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    fn setup() -> (MobAi<TestMob>, BrainState) {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        cx.mob.others = vec![
            (EntityClass::Living, EntityFacts::living(2, 2.5, 1.0, 0.5)),
            (EntityClass::Living, EntityFacts::living(3, 4.5, 1.0, 0.5)),
            // exactly 8.0 away: distanceToSqr == 64.0, and `<=` passes it.
            (EntityClass::Living, EntityFacts::living(4, 8.5, 1.0, 0.5)),
        ];
        cx.mob.visible = vec![2, 3, 4];
        cx.mob.wet = vec![2, 3, 4];
        let mut brain = Brain::<TestMob>::new(
            &[
                T::NearestAttackable,
                T::NearestVisibleLivingEntities,
                T::HasHuntingCooldown,
                T::AttackTarget,
            ],
            vec![],
        )
        .state;
        brain.set_memory_value(
            T::NearestVisibleLivingEntities,
            MemoryValue::NearestVisible(NearestVisibleLivingEntities::new(vec![2, 3, 4])),
        );
        (cx, brain)
    }

    #[test]
    fn the_hunting_cooldown_gates_only_the_hunt_tag_and_isclose_is_inclusive() {
        let (mut cx, mut brain) = setup();
        cx.mob.entity_type_tags = vec![(3, AXOLOTL_HUNT_TARGETS), (4, AXOLOTL_ALWAYS_HOSTILES)];
        let mut s = AxolotlAttackablesSensor;
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert_eq!(
            brain.get_memory(T::NearestAttackable),
            Some(&MemoryValue::Entity(3))
        );
        // The cooldown kills the hunt branch; the always-hostile at exactly
        // 64.0 squared still passes, which is the `<=`.
        brain.set_memory_value(T::HasHuntingCooldown, MemoryValue::Unit);
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert_eq!(
            brain.get_memory(T::NearestAttackable),
            Some(&MemoryValue::Entity(4))
        );
        // One block further and 64.0 is exceeded.
        cx.mob.others[2].1.x = 9.5;
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert!(!brain.has_memory_value(T::NearestAttackable));
    }

    #[test]
    fn water_and_attackability_gate_both_tag_branches() {
        let (mut cx, mut brain) = setup();
        cx.mob.entity_type_tags = vec![(2, AXOLOTL_ALWAYS_HOSTILES), (3, AXOLOTL_HUNT_TARGETS)];
        let mut s = AxolotlAttackablesSensor;
        cx.mob.wet = vec![3, 4];
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert_eq!(
            brain.get_memory(T::NearestAttackable),
            Some(&MemoryValue::Entity(3)),
            "2 is hostile-tagged but dry"
        );
        cx.mob.allies = vec![3];
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert!(
            !brain.has_memory_value(T::NearestAttackable),
            "isEntityAttackable gates the hunt branch too"
        );
        assert!(SensorBody::<TestMob>::requires(&s).contains(&T::HasHuntingCooldown));
    }
}
