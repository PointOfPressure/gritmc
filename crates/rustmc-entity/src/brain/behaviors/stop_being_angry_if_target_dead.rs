//! `net.minecraft.world.entity.ai.behavior.StopBeingAngryIfTargetDead`, from
//! the 26.2 jar. A `BehaviorBuilder` one-shot over `group(present(ANGRY_AT))`
//! (`lambda$create$2`, 0..50):
//!
//! ```text
//! Optional.ofNullable(level.getEntity(angryAt))            <- by UUID
//!     .map(e -> e instanceof LivingEntity ? e : null)
//!     .filter(LivingEntity::isDeadOrDying)
//!     .filter(e -> !e.is(PLAYER) || gamerules.FORGIVE_DEAD_PLAYERS)
//!     .ifPresent(e -> angryAt.erase())
//! return true                                              <- always
//! ```

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::memory::{MemoryModuleType, MemoryValue};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `StopBeingAngryIfTargetDead.create()`'s trigger.
#[derive(Debug, Clone, Copy, Default)]
pub struct StopBeingAngryIfTargetDead;

impl StopBeingAngryIfTargetDead {
    /// `create()`.
    #[must_use]
    pub fn create() -> OneShot<Self> {
        OneShot::new(StopBeingAngryIfTargetDead)
    }
}

impl<M: MobHost> Trigger<M> for StopBeingAngryIfTargetDead {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        let Some(Some(v)) = brain.memory_internal(MemoryModuleType::AngryAt) else {
            return false;
        };
        let MemoryValue::Uuid(uuid) = v else {
            return false;
        };
        let uuid = *uuid;
        if let Some(id) = cx.mob.entity_by_uuid(uuid) {
            if cx.mob.entity_is_living(id) && cx.mob.entity_dead_or_dying(id) {
                let is_player = cx.mob.entity(id).is_some_and(|e| e.is_player);
                if !is_player || cx.mob.forgive_dead_players() {
                    brain.erase_memory(MemoryModuleType::AngryAt);
                }
            }
        }
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![MemoryModuleType::AngryAt]
    }

    fn name(&self) -> &'static str {
        "StopBeingAngryIfTargetDead"
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
    fn forgets_a_dead_mob_but_a_dead_player_only_under_the_gamerule() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        cx.mob.others = vec![
            (EntityClass::Living, EntityFacts::living(3, 2.5, 1.0, 0.5)),
            (EntityClass::Player, EntityFacts::player(4, 3.5, 1.0, 0.5)),
        ];
        cx.mob.uuids = vec![(3, 0x33), (4, 0x44)];
        cx.mob.others[0].1.alive = false;
        cx.mob.others[1].1.alive = false;
        let mut brain = Brain::<TestMob>::new(&[T::AngryAt], vec![]).state;
        let mut b = StopBeingAngryIfTargetDead::create();
        assert!(!b.try_start(&mut brain, &mut cx, 0), "absent");
        brain.set_memory_value(T::AngryAt, MemoryValue::Uuid(0x44));
        assert!(
            b.try_start(&mut brain, &mut cx, 0),
            "always true once present"
        );
        assert!(
            brain.has_memory_value(T::AngryAt),
            "a dead player is not forgiven by default"
        );
        cx.mob.forgive_dead_players = true;
        assert!(b.try_start(&mut brain, &mut cx, 1));
        assert!(!brain.has_memory_value(T::AngryAt));
        brain.set_memory_value(T::AngryAt, MemoryValue::Uuid(0x33));
        cx.mob.forgive_dead_players = false;
        assert!(b.try_start(&mut brain, &mut cx, 2));
        assert!(
            !brain.has_memory_value(T::AngryAt),
            "a dead mob is forgotten regardless"
        );
        brain.set_memory_value(T::AngryAt, MemoryValue::Uuid(0x99));
        assert!(b.try_start(&mut brain, &mut cx, 3));
        assert!(
            brain.has_memory_value(T::AngryAt),
            "an unknown uuid keeps the anger"
        );
    }
}
