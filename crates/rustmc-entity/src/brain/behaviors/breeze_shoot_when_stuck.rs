//! `net.minecraft.world.entity.monster.breeze.ShootWhenStuck`, from the
//! 26.2 jar.
//!
//! ```text
//! ShootWhenStuck()   Behavior<Breeze>, Behavior(map) -> (60, 60)
//!   entry Map.of(ATTACK_TARGET:VALUE_PRESENT, BREEZE_JUMP_INHALING:VALUE_ABSENT,
//!                BREEZE_JUMP_TARGET:VALUE_ABSENT, WALK_TARGET:VALUE_ABSENT,
//!                BREEZE_SHOOT:VALUE_ABSENT)
//!   checkExtraStartConditions (0..29):
//!        breeze.isPassenger() || breeze.isInWater()
//!            || breeze.getEffect(MobEffects.LEVITATION) != null
//!   canStillUse (0..1):   false            // iconst_0
//!   start (0..16):        brain.setMemoryWithExpiry(BREEZE_SHOOT, Unit.INSTANCE, 60L)
//! ```
//!
//! **`canStillUse` is a bare `iconst_0`**, so this behaviour runs for
//! exactly one brain tick however long its 60-tick duration says: `tryStart`
//! sets `RUNNING` and runs `start`, and the very next `tickOrStop` calls
//! `doStop`. It is a one-shot written as a `Behavior`, and the 60 in
//! `Behavior(Map)`'s `DEFAULT_DURATION` is never reached — the draw it makes
//! at `tryStart` still happens, so it costs one `nextInt(1)` from the level
//! random like every other fixed-duration behaviour.
//!
//! **The condition is a disjunction of three ways of being stuck**, and it
//! is the only `checkExtraStartConditions` in the breeze's set that wants
//! things to be *wrong*: a breeze riding something, swimming, or levitating
//! cannot slide or jump, so it shoots instead. `getEffect(...) != null` is a
//! presence test, not a level test.
//!
//! **A null worth recording:** it has no `stop` and no `tick`. The whole
//! class is the one `setMemoryWithExpiry`, and the 60-tick expiry on
//! `BREEZE_SHOOT` is the same constant `BreezeAi$SlideToTargetSink.stop`
//! writes.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::memory::{MemoryModuleType, MemoryStatus, MemoryValue};
use crate::brain::BrainState;
use crate::host::MobHost;

/// The expiry `start` gives `BREEZE_SHOOT`.
pub const SHOOT_MEMORY_TICKS: i64 = 60;

/// `ShootWhenStuck`'s body.
#[derive(Debug, Clone, Copy, Default)]
pub struct ShootWhenStuck;

impl ShootWhenStuck {
    /// `new ShootWhenStuck()`.
    #[must_use]
    pub fn new() -> Behavior<ShootWhenStuck> {
        Behavior::new(
            ShootWhenStuck,
            vec![
                (MemoryModuleType::AttackTarget, MemoryStatus::ValuePresent),
                (
                    MemoryModuleType::BreezeJumpInhaling,
                    MemoryStatus::ValueAbsent,
                ),
                (
                    MemoryModuleType::BreezeJumpTarget,
                    MemoryStatus::ValueAbsent,
                ),
                (MemoryModuleType::WalkTarget, MemoryStatus::ValueAbsent),
                (MemoryModuleType::BreezeShoot, MemoryStatus::ValueAbsent),
            ],
        )
    }
}

impl<M: MobHost> BehaviorBody<M> for ShootWhenStuck {
    fn check_extra_start_conditions(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        cx.mob.is_passenger() || cx.mob.is_in_water() || cx.mob.has_effect("levitation")
    }

    fn can_still_use(&mut self, _brain: &mut BrainState, _cx: &mut MobAi<M>, _time: i64) -> bool {
        false
    }

    fn start(&mut self, brain: &mut BrainState, _cx: &mut MobAi<M>, _time: i64) {
        brain.set_memory_with_expiry(
            MemoryModuleType::BreezeShoot,
            MemoryValue::Unit,
            SHOOT_MEMORY_TICKS,
        );
    }

    fn name(&self) -> &'static str {
        "ShootWhenStuck"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::{BehaviorControl, Status};
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};

    fn setup() -> (MobAi<TestMob>, BrainState) {
        let cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut brain = Brain::<TestMob>::new(
            &[
                T::AttackTarget,
                T::BreezeJumpInhaling,
                T::BreezeJumpTarget,
                T::WalkTarget,
                T::BreezeShoot,
            ],
            vec![],
        )
        .state;
        brain.set_memory_value(T::AttackTarget, MemoryValue::Entity(7));
        (cx, brain)
    }

    #[test]
    fn each_of_the_three_stuck_conditions_alone_starts_it() {
        for which in 0..3 {
            let (mut cx, mut brain) = setup();
            match which {
                0 => cx.mob.is_passenger = true,
                1 => cx.mob.in_water = true,
                _ => cx.mob.effects.push("levitation"),
            }
            let mut b = ShootWhenStuck::new();
            assert!(b.try_start(&mut brain, &mut cx, 0), "condition {which}");
            assert_eq!(brain.get_memory(T::BreezeShoot), Some(&MemoryValue::Unit));
            assert_eq!(brain.time_until_expiry(T::BreezeShoot), 60);
        }
    }

    #[test]
    fn an_unstuck_breeze_does_not_start_it() {
        let (mut cx, mut brain) = setup();
        let mut b = ShootWhenStuck::new();
        assert!(!b.try_start(&mut brain, &mut cx, 0));
        assert!(!brain.has_memory_value(T::BreezeShoot));
    }

    #[test]
    fn it_stops_on_the_very_next_tick_despite_a_sixty_tick_duration() {
        let (mut cx, mut brain) = setup();
        cx.mob.in_water = true;
        let mut b = ShootWhenStuck::new();
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(b.end_timestamp(), 60, "min == max, so nextInt(1) is 0");
        b.tick_or_stop(&mut brain, &mut cx, 1);
        assert_eq!(BehaviorControl::<TestMob>::status(&b), Status::Stopped);
    }

    #[test]
    fn an_existing_shoot_memory_blocks_the_entry_condition() {
        let (mut cx, mut brain) = setup();
        cx.mob.in_water = true;
        brain.set_memory_value(T::BreezeShoot, MemoryValue::Unit);
        let mut b = ShootWhenStuck::new();
        assert!(!b.try_start(&mut brain, &mut cx, 0));
    }
}
