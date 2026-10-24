//! `net.minecraft.world.entity.ai.sensing.FrogAttackablesSensor`, from the
//! 26.2 jar. `NearestVisibleLivingEntitySensor` with
//! `getMemoryToSet() = NEAREST_ATTACKABLE`; `requires()` is the base's set
//! unioned with `{UNREACHABLE_TONGUE_TARGETS}` (`Sets.union`, 0..13).
//! `TARGET_DETECTION_DISTANCE = 10.0f`.
//!
//! ```text
//! isMatchingEntity (0..35):   Sensor.isEntityAttackable(level, mob, e)
//!                               && Frog.canEat(e)
//!                               && !isUnreachableAttackTarget(mob, e)
//!                               && e.closerThan(mob, 10.0)
//! isUnreachableAttackTarget (0..32):
//!                             mob.getBrain().getMemory(UNREACHABLE_TONGUE_TARGETS)
//!                               .orElseGet(ArrayList::new).contains(e.getUUID())
//! Frog.canEat (0..29):        if e instanceof AbstractCubeMob c && c.getSize() != 1: false
//!                             else: e.is(EntityTypeTags.FROG_FOOD)
//! ```
//!
//! The distance test is **last**, and it is `closerThan` — a strict
//! `< 10.0` on `position()`, unlike the axolotl sensor's `<= 64.0` on the
//! square. The literal is `ldc2_w double 10.0d` at 27, so the `float`
//! constant `TARGET_DETECTION_DISTANCE` is not what the comparison uses.
//!
//! `UNREACHABLE_TONGUE_TARGETS` holds **UUIDs**, not ids, and an absent
//! memory is an empty list rather than a refusal (`orElseGet`).
//! `AbstractCubeMob.getSize() != 1` rejects a big slime or magma cube
//! before the tag is consulted, so a size-2 cube in `FROG_FOOD` is still
//! not food.

use crate::ai::mob_ai::MobAi;
use crate::brain::memory::{MemoryModuleType, MemoryValue};
use crate::brain::sensor::{
    attack_target_id, is_entity_attackable_with, Sensor, SensorBody, SensorType,
};
use crate::brain::sensors::nearest_visible_living_entity_sensor::{do_tick_with, requires_with};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `TARGET_DETECTION_DISTANCE`.
pub const TARGET_DETECTION_DISTANCE: f32 = 10.0;
/// The `closerThan` literal, which is a `double`.
pub const CLOSER_THAN: f64 = 10.0;
/// `EntityTypeTags.FROG_FOOD`.
pub const FROG_FOOD: &str = "frog_food";

/// `FrogAttackablesSensor`.
#[derive(Debug, Clone, Copy, Default)]
pub struct FrogAttackablesSensor;

impl FrogAttackablesSensor {
    /// `SensorType.FROG_ATTACKABLES.create()`.
    #[must_use]
    pub fn new() -> Sensor<FrogAttackablesSensor> {
        Sensor::new(FrogAttackablesSensor)
    }

    /// `Frog.canEat(e)`.
    pub fn can_eat<M: MobHost>(cx: &MobAi<M>, id: i32) -> bool {
        if cx.mob.cube_mob_size(id).is_some_and(|n| n != 1) {
            return false;
        }
        cx.mob.entity_type_tag_is(id, FROG_FOOD)
    }
}

impl<M: MobHost> SensorBody<M> for FrogAttackablesSensor {
    fn do_tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) {
        let attack_target = attack_target_id(brain);
        let unreachable: Vec<u128> =
            match brain.get_memory(MemoryModuleType::UnreachableTongueTargets) {
                Some(MemoryValue::Uuids(v)) => v.clone(),
                _ => Vec::new(),
            };
        do_tick_with(brain, cx, MemoryModuleType::NearestAttackable, |cx, id| {
            let Some(facts) = cx.mob.entity(id) else {
                return false;
            };
            if !is_entity_attackable_with(attack_target, cx, &facts) {
                return false;
            }
            if !Self::can_eat(cx, id) {
                return false;
            }
            if unreachable.contains(&cx.mob.entity_uuid(id)) {
                return false;
            }
            facts.distance_to_sqr(cx.mob.x(), cx.mob.y(), cx.mob.z()) < CLOSER_THAN * CLOSER_THAN
        });
    }

    fn requires(&self) -> Vec<MemoryModuleType> {
        let mut r = requires_with(MemoryModuleType::NearestAttackable);
        r.push(MemoryModuleType::UnreachableTongueTargets);
        r
    }

    fn sensor_type(&self) -> SensorType {
        SensorType::FrogAttackables
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::memory::{MemoryModuleType as T, NearestVisibleLivingEntities};
    use crate::brain::Brain;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    fn setup() -> (MobAi<TestMob>, BrainState) {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        cx.mob.others = vec![
            (EntityClass::Living, EntityFacts::living(2, 2.5, 1.0, 0.5)),
            (EntityClass::Living, EntityFacts::living(3, 4.5, 1.0, 0.5)),
            (EntityClass::Living, EntityFacts::living(4, 6.5, 1.0, 0.5)),
        ];
        cx.mob.visible = vec![2, 3, 4];
        cx.mob.entity_type_tags = vec![(2, FROG_FOOD), (3, FROG_FOOD), (4, FROG_FOOD)];
        cx.mob.uuids = vec![(2, 0x22), (3, 0x33), (4, 0x44)];
        let mut brain = Brain::<TestMob>::new(
            &[
                T::NearestAttackable,
                T::NearestVisibleLivingEntities,
                T::UnreachableTongueTargets,
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
    fn a_uuid_on_the_unreachable_list_and_an_oversized_cube_are_both_skipped() {
        let (mut cx, mut brain) = setup();
        let mut s = FrogAttackablesSensor;
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert_eq!(
            brain.get_memory(T::NearestAttackable),
            Some(&MemoryValue::Entity(2))
        );
        // The memory is by UUID, not by id.
        brain.set_memory_value(T::UnreachableTongueTargets, MemoryValue::Uuids(vec![0x22]));
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert_eq!(
            brain.get_memory(T::NearestAttackable),
            Some(&MemoryValue::Entity(3))
        );
        // A cube mob of size 2 is refused before the FROG_FOOD tag is read.
        cx.mob.cube_sizes = vec![(3, 2)];
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert_eq!(
            brain.get_memory(T::NearestAttackable),
            Some(&MemoryValue::Entity(4))
        );
        // Size 1 is eaten.
        cx.mob.cube_sizes = vec![(3, 1)];
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert_eq!(
            brain.get_memory(T::NearestAttackable),
            Some(&MemoryValue::Entity(3))
        );
    }

    #[test]
    fn closer_than_ten_is_strict_and_an_absent_unreachable_list_is_empty() {
        let (mut cx, mut brain) = setup();
        // Exactly 10.0 away: `closerThan` is `<`, so it fails.
        cx.mob.others[0].1.x = 10.5;
        cx.mob.others[1].1.x = 10.5;
        cx.mob.others[2].1.x = 10.5;
        cx.mob.follow_range = 32.0;
        let mut s = FrogAttackablesSensor;
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert!(
            !brain.has_memory_value(T::NearestAttackable),
            "10.0 is not closerThan 10.0"
        );
        cx.mob.others[1].1.x = 10.4;
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert_eq!(
            brain.get_memory(T::NearestAttackable),
            Some(&MemoryValue::Entity(3)),
            "and the unreachable memory was never set: orElseGet gives []"
        );
        assert!(SensorBody::<TestMob>::requires(&s).contains(&T::UnreachableTongueTargets));
    }
}
