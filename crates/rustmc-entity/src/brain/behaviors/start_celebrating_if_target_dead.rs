//! `net.minecraft.world.entity.ai.behavior.StartCelebratingIfTargetDead`,
//! from the 26.2 jar. A `BehaviorBuilder` one-shot over
//! `group(present(ATTACK_TARGET), registered(ANGRY_AT),
//! absent(CELEBRATE_LOCATION), registered(DANCING))`.
//!
//! ```text
//! lambda$create$2 (0..98):
//!   target = attackTarget
//!   if !target.isDeadOrDying(): return false
//!   if dancePredicate.test(mob, target): dancing.setWithExpiry(true, celebrateDuration)
//!   celebrate.setWithExpiry(target.blockPosition(), celebrateDuration)
//!   if !target.is(PLAYER) || gameRules.FORGIVE_DEAD_PLAYERS: attack.erase(); angryAt.erase()
//!   return true
//! ```
//!
//! No random draw.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::memory::{MemoryModuleType, MemoryValue};
use crate::brain::BrainState;
use crate::host::MobHost;

/// A `BiPredicate<LivingEntity, LivingEntity>` over the mob and its target.
pub type DancePredicate<M> = Box<dyn FnMut(&mut MobAi<M>, i32) -> bool>;

/// `StartCelebratingIfTargetDead`'s trigger.
pub struct StartCelebratingIfTargetDead<M: MobHost> {
    pub celebrate_duration: i32,
    pub dance_predicate: DancePredicate<M>,
}

impl<M: MobHost> StartCelebratingIfTargetDead<M> {
    /// `create(int, BiPredicate<LivingEntity, LivingEntity>)`.
    #[must_use]
    pub fn create(celebrate_duration: i32, dance_predicate: DancePredicate<M>) -> OneShot<Self> {
        OneShot::new(StartCelebratingIfTargetDead {
            celebrate_duration,
            dance_predicate,
        })
    }
}

impl<M: MobHost> Trigger<M> for StartCelebratingIfTargetDead<M> {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        let Some(Some(v)) = brain.memory_internal(MemoryModuleType::AttackTarget) else {
            return false;
        };
        let Some(target) = v.as_entity() else {
            return false;
        };
        if brain.memory_internal(MemoryModuleType::AngryAt).is_none() {
            return false;
        }
        if brain.memory_internal(MemoryModuleType::CelebrateLocation) != Some(None) {
            return false;
        }
        if brain.memory_internal(MemoryModuleType::Dancing).is_none() {
            return false;
        }
        if !cx.mob.entity_dead_or_dying(target) {
            return false;
        }
        let ttl = i64::from(self.celebrate_duration);
        if (self.dance_predicate)(cx, target) {
            brain.set_memory_with_expiry(MemoryModuleType::Dancing, MemoryValue::Bool(true), ttl);
        }
        let pos = cx
            .mob
            .entity(target)
            .map(|e| e.block_position())
            .unwrap_or_else(|| cx.mob.block_position());
        brain.set_memory_with_expiry(
            MemoryModuleType::CelebrateLocation,
            MemoryValue::BlockPos(pos),
            ttl,
        );
        let is_player = cx.mob.entity(target).is_some_and(|e| e.is_player);
        if !is_player || cx.mob.forgive_dead_players() {
            brain.erase_memory(MemoryModuleType::AttackTarget);
            brain.erase_memory(MemoryModuleType::AngryAt);
        }
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![
            MemoryModuleType::AttackTarget,
            MemoryModuleType::AngryAt,
            MemoryModuleType::CelebrateLocation,
            MemoryModuleType::Dancing,
        ]
    }

    fn name(&self) -> &'static str {
        "StartCelebratingIfTargetDead"
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
    use rustmc_data::BlockPos;

    fn setup(dead_player: bool) -> (MobAi<TestMob>, BrainState) {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut t = EntityFacts::living(7, 3.5, 1.0, 5.5);
        t.alive = false;
        t.is_player = dead_player;
        cx.mob.others = vec![(EntityClass::Living, t)];
        let mut brain = Brain::<TestMob>::new(
            &[
                T::AttackTarget,
                T::AngryAt,
                T::CelebrateLocation,
                T::Dancing,
            ],
            vec![],
        )
        .state;
        brain.set_memory_value(T::AttackTarget, MemoryValue::Entity(7));
        brain.set_memory_value(T::AngryAt, MemoryValue::Uuid(77));
        (cx, brain)
    }

    #[test]
    fn a_dead_mob_target_starts_the_party_and_clears_the_anger() {
        let (mut cx, mut brain) = setup(false);
        let mut b = StartCelebratingIfTargetDead::create(300, Box::new(|_cx, _t| true));
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(brain.get_memory(T::Dancing), Some(&MemoryValue::Bool(true)));
        assert_eq!(brain.time_until_expiry(T::Dancing), 300);
        assert_eq!(
            brain.get_memory(T::CelebrateLocation),
            Some(&MemoryValue::BlockPos(BlockPos { x: 3, y: 1, z: 5 }))
        );
        assert!(!brain.has_memory_value(T::AttackTarget));
        assert!(!brain.has_memory_value(T::AngryAt));
        assert!(
            !b.try_start(&mut brain, &mut cx, 0),
            "celebrate location now present"
        );
    }

    #[test]
    fn a_dead_player_keeps_the_grudge_unless_forgiven_and_no_dance_without_the_predicate() {
        let (mut cx, mut brain) = setup(true);
        let mut b = StartCelebratingIfTargetDead::create(300, Box::new(|_cx, _t| false));
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(!brain.has_memory_value(T::Dancing));
        assert!(brain.has_memory_value(T::AttackTarget));
        assert!(brain.has_memory_value(T::AngryAt));
        let (mut cx, mut brain) = setup(true);
        cx.mob.forgive_dead_players = true;
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(!brain.has_memory_value(T::AttackTarget));
        let (mut cx, mut brain) = setup(false);
        cx.mob.others[0].1.alive = true;
        assert!(!b.try_start(&mut brain, &mut cx, 0), "alive target");
    }
}
