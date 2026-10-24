//! `net.minecraft.world.entity.ai.sensing.HurtBySensor`, from the 26.2 jar.
//! `Sensor()` (scan rate 20); `requires()` = `{HURT_BY, HURT_BY_ENTITY}`.
//!
//! ```text
//! doTick (0..85):
//!   src = mob.getLastDamageSource()
//!   if src != null:
//!       brain.setMemory(HURT_BY, mob.getLastDamageSource())        <- read a second time; same object
//!       if src.getEntity() instanceof LivingEntity e: brain.setMemory(HURT_BY_ENTITY, e)
//!   else: brain.eraseMemory(HURT_BY)
//!   brain.getMemory(HURT_BY_ENTITY).ifPresent(e -> { if !e.isAlive() || e.level() != level: brain.eraseMemory(HURT_BY_ENTITY) })
//! ```
//!
//! Note the asymmetry: a `null` source erases `HURT_BY` but leaves
//! `HURT_BY_ENTITY` alone until that entity dies or changes level.

use crate::ai::mob_ai::MobAi;
use crate::brain::memory::{MemoryModuleType, MemoryValue};
use crate::brain::sensor::{Sensor, SensorBody, SensorType};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `HurtBySensor`.
#[derive(Debug, Clone, Copy, Default)]
pub struct HurtBySensor;

impl HurtBySensor {
    /// `SensorType.HURT_BY.create()`.
    #[must_use]
    pub fn new() -> Sensor<HurtBySensor> {
        Sensor::new(HurtBySensor)
    }
}

impl<M: MobHost> SensorBody<M> for HurtBySensor {
    fn do_tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) {
        match cx.mob.last_damage_source() {
            Some(src) => {
                let entity = src.entity;
                brain.set_memory_value(MemoryModuleType::HurtBy, MemoryValue::DamageSource(src));
                if let Some(e) = entity {
                    if cx.mob.entity_is_living(e) {
                        brain.set_memory_value(
                            MemoryModuleType::HurtByEntity,
                            MemoryValue::Entity(e),
                        );
                    }
                }
            }
            None => brain.erase_memory(MemoryModuleType::HurtBy),
        }
        if let Some(e) = brain
            .get_memory(MemoryModuleType::HurtByEntity)
            .and_then(MemoryValue::as_entity)
        {
            let alive = cx.mob.entity(e).is_some_and(|f| f.alive);
            if !alive || !cx.mob.entity_in_same_level(e) {
                brain.erase_memory(MemoryModuleType::HurtByEntity);
            }
        }
    }

    fn requires(&self) -> Vec<MemoryModuleType> {
        vec![MemoryModuleType::HurtBy, MemoryModuleType::HurtByEntity]
    }

    fn sensor_type(&self) -> SensorType {
        SensorType::HurtBy
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::memory::{DamageSourceFacts, MemoryModuleType as T};
    use crate::brain::Brain;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn records_the_source_and_its_living_attacker_and_forgets_a_dead_one() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        cx.mob.others = vec![(EntityClass::Living, EntityFacts::living(3, 2.5, 1.0, 0.5))];
        cx.mob.last_damage_source = Some(DamageSourceFacts {
            entity: Some(3),
            direct_entity: Some(3),
            kind: "mob_attack",
        });
        let mut brain = Brain::<TestMob>::new(&[T::HurtBy, T::HurtByEntity], vec![]).state;
        let mut s = HurtBySensor;
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert!(brain.has_memory_value(T::HurtBy));
        assert_eq!(
            brain.get_memory(T::HurtByEntity),
            Some(&MemoryValue::Entity(3))
        );
        cx.mob.last_damage_source = None;
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert!(!brain.has_memory_value(T::HurtBy));
        assert!(
            brain.has_memory_value(T::HurtByEntity),
            "the attacker stays while alive"
        );
        cx.mob.others[0].1.alive = false;
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert!(!brain.has_memory_value(T::HurtByEntity));
    }
}
