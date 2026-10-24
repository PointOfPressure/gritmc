//! `net.minecraft.world.entity.ai.sensing.WardenEntitySensor` and
//! `BreezeAttackEntitySensor`, from the 26.2 jar. Both extend
//! `NearestLivingEntitySensor`, both add `NEAREST_ATTACKABLE` to its
//! `requires()` (`Iterables.concat` then `ImmutableSet.copyOf`, 0..16 in
//! each), and both run the base `doTick` first and then pick one entity
//! out of the `NEAREST_LIVING_ENTITIES` list the base has just written.
//!
//! ```text
//! WardenEntitySensor.doTick (0..39):
//!   super.doTick(level, warden)
//!   getClosest(warden, e -> e.is(EntityTypes.PLAYER))
//!     .or(() -> getClosest(warden, e -> !e.is(EntityTypes.PLAYER)))
//!     .ifPresentOrElse(e -> brain.setMemory(NEAREST_ATTACKABLE, e),
//!                      () -> brain.eraseMemory(NEAREST_ATTACKABLE))
//! getClosest (0..50):
//!   brain.getMemory(NEAREST_LIVING_ENTITIES).stream().flatMap(List::stream)
//!     .filter(warden::canTargetEntity).filter(pred).findFirst()
//!
//! BreezeAttackEntitySensor.doTick (0..69):
//!   super.doTick(level, breeze)
//!   brain.getMemory(NEAREST_LIVING_ENTITIES).stream().flatMap(List::stream)
//!     .filter(EntitySelector.NO_CREATIVE_OR_SPECTATOR)
//!     .filter(e -> Sensor.isEntityAttackable(level, breeze, e))
//!     .findFirst()
//!     .ifPresentOrElse(set NEAREST_ATTACKABLE, erase NEAREST_ATTACKABLE)
//! ```
//!
//! **The warden's two passes are not one pass with a sort key.** It walks
//! the whole distance-ordered list looking for a player, and only if that
//! finds nothing walks it again for a non-player (`Optional.or`, which is
//! lazy). So the nearest player outranks a closer non-player, and the
//! second walk re-runs `canTargetEntity` on every entity the first walk
//! already rejected.
//!
//! `Warden.canTargetEntity` 0..99 is eight conjuncts —
//! `instanceof LivingEntity`, same level, `NO_CREATIVE_OR_SPECTATOR`,
//! `!isAlliedTo`, not `ARMOR_STAND`, not `WARDEN`, `!isInvulnerable`,
//! `!isDeadOrDying`, inside the world border — none of which this crate
//! can answer, so it is one host call.
//!
//! Neither sensor reads `NEAREST_VISIBLE_LIVING_ENTITIES`: the base
//! writes both memories and these two subclasses read the plain list, so
//! line of sight plays no part in `NEAREST_ATTACKABLE` here.

use crate::ai::mob_ai::MobAi;
use crate::brain::memory::{MemoryModuleType, MemoryValue};
use crate::brain::sensor::{
    attack_target_id, is_entity_attackable_with, Sensor, SensorBody, SensorType,
};
use crate::brain::sensors::nearest_living_entity_sensor::NearestLivingEntitySensor;
use crate::brain::BrainState;
use crate::host::MobHost;

/// Which subclass this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttackEntityKind {
    /// `WardenEntitySensor`: players first, then everything else.
    Warden,
    /// `BreezeAttackEntitySensor`: the first attackable non-creative,
    /// non-spectator entity.
    Breeze,
}

/// `WardenEntitySensor` / `BreezeAttackEntitySensor`.
#[derive(Debug, Clone, Copy)]
pub struct WardenEntitySensor {
    /// Which of the two this is.
    pub kind: AttackEntityKind,
}

impl WardenEntitySensor {
    /// `SensorType.WARDEN_ENTITY_SENSOR.create()`.
    #[must_use]
    pub fn new() -> Sensor<WardenEntitySensor> {
        Sensor::new(WardenEntitySensor {
            kind: AttackEntityKind::Warden,
        })
    }

    /// `SensorType.BREEZE_ATTACK_ENTITY_SENSOR.create()`.
    #[must_use]
    pub fn breeze() -> Sensor<WardenEntitySensor> {
        Sensor::new(WardenEntitySensor {
            kind: AttackEntityKind::Breeze,
        })
    }

    /// The base's `NEAREST_LIVING_ENTITIES` list, or empty when absent
    /// (`Optional.stream()`).
    fn nearest_living(brain: &BrainState) -> Vec<i32> {
        match brain.get_memory(MemoryModuleType::NearestLivingEntities) {
            Some(MemoryValue::Entities(v)) => v.clone(),
            _ => Vec::new(),
        }
    }

    /// `WardenEntitySensor.getClosest(warden, pred)`.
    fn get_closest<M: MobHost>(list: &[i32], cx: &mut MobAi<M>, want_player: bool) -> Option<i32> {
        list.iter().copied().find(|id| {
            cx.mob.can_target_entity(*id)
                && cx.mob.entity(*id).is_some_and(|e| e.is_player) == want_player
        })
    }
}

impl<M: MobHost> SensorBody<M> for WardenEntitySensor {
    fn do_tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) {
        SensorBody::<M>::do_tick(&mut NearestLivingEntitySensor, brain, cx);
        let list = Self::nearest_living(brain);
        let found = match self.kind {
            AttackEntityKind::Warden => {
                Self::get_closest(&list, cx, true).or_else(|| Self::get_closest(&list, cx, false))
            }
            AttackEntityKind::Breeze => {
                let attack_target = attack_target_id(brain);
                list.iter().copied().find(|id| {
                    let Some(f) = cx.mob.entity(*id) else {
                        return false;
                    };
                    if f.is_creative || f.is_spectator {
                        return false;
                    }
                    is_entity_attackable_with(attack_target, cx, &f)
                })
            }
        };
        match found {
            Some(id) => {
                brain
                    .set_memory_value(MemoryModuleType::NearestAttackable, MemoryValue::Entity(id));
            }
            None => brain.erase_memory(MemoryModuleType::NearestAttackable),
        }
    }

    fn requires(&self) -> Vec<MemoryModuleType> {
        vec![
            MemoryModuleType::NearestLivingEntities,
            MemoryModuleType::NearestVisibleLivingEntities,
            MemoryModuleType::NearestAttackable,
        ]
    }

    fn sensor_type(&self) -> SensorType {
        match self.kind {
            AttackEntityKind::Warden => SensorType::WardenEntitySensor,
            AttackEntityKind::Breeze => SensorType::BreezeAttackEntitySensor,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    fn player(id: i32, x: f64) -> EntityFacts {
        let mut f = EntityFacts::living(id, x, 1.0, 0.5);
        f.is_player = true;
        f
    }

    fn setup() -> (MobAi<TestMob>, BrainState) {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        cx.mob.others = vec![
            (EntityClass::Living, EntityFacts::living(2, 1.5, 1.0, 0.5)),
            (EntityClass::Player, player(3, 4.5)),
            (EntityClass::Living, EntityFacts::living(4, 6.5, 1.0, 0.5)),
        ];
        cx.mob.visible = vec![2, 3, 4];
        cx.mob.targetable = vec![2, 3, 4];
        let brain = Brain::<TestMob>::new(
            &[
                T::NearestLivingEntities,
                T::NearestVisibleLivingEntities,
                T::NearestAttackable,
                T::AttackTarget,
            ],
            vec![],
        )
        .state;
        (cx, brain)
    }

    #[test]
    fn the_warden_prefers_a_far_player_over_a_near_mob_and_falls_back_when_there_is_none() {
        let (mut cx, mut brain) = setup();
        let mut s = WardenEntitySensor {
            kind: AttackEntityKind::Warden,
        };
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert_eq!(
            brain.get_memory(T::NearestAttackable),
            Some(&MemoryValue::Entity(3)),
            "the player at 4 beats the mob at 1: Optional.or runs the player pass first"
        );
        // canTargetEntity refuses the player: the second pass takes the
        // nearest non-player, which is the one the first pass skipped.
        cx.mob.targetable = vec![2, 4];
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert_eq!(
            brain.get_memory(T::NearestAttackable),
            Some(&MemoryValue::Entity(2))
        );
        cx.mob.targetable.clear();
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert!(
            !brain.has_memory_value(T::NearestAttackable),
            "ifPresentOrElse's else is an erase"
        );
        // The base ran: it wrote the ordered list this sensor read.
        assert_eq!(
            brain.get_memory(T::NearestLivingEntities),
            Some(&MemoryValue::Entities(vec![2, 3, 4]))
        );
    }

    #[test]
    fn the_breeze_takes_the_first_attackable_and_skips_creative_and_spectator() {
        let (mut cx, mut brain) = setup();
        cx.mob.others[0].1.is_creative = true;
        cx.mob.others[0].1.is_player = true;
        cx.mob.others[1].1.is_spectator = true;
        let mut s = WardenEntitySensor {
            kind: AttackEntityKind::Breeze,
        };
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert_eq!(
            brain.get_memory(T::NearestAttackable),
            Some(&MemoryValue::Entity(4)),
            "2 is creative, 3 is a spectator"
        );
        // Nothing attackable: erase. `isAlliedTo` is inside the combat
        // targeting conditions.
        cx.mob.allies = vec![4];
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert!(!brain.has_memory_value(T::NearestAttackable));
        assert_eq!(
            SensorBody::<TestMob>::requires(&s),
            [
                T::NearestLivingEntities,
                T::NearestVisibleLivingEntities,
                T::NearestAttackable
            ]
        );
    }
}
