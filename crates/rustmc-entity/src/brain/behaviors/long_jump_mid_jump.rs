//! `net.minecraft.world.entity.ai.behavior.LongJumpMidJump`, from the 26.2
//! jar. `Behavior<Mob>` with entry `{LOOK_TARGET: REGISTERED,
//! LONG_JUMP_MID_JUMP: VALUE_PRESENT}` and a fixed duration of
//! `TIME_OUT_DURATION = 100`.
//!
//! ```text
//! canStillUse (0..12):  !mob.onGround()
//! start (0..12):        mob.setDiscardFriction(true); mob.setPose(Pose.LONG_JUMPING)
//! stop (0..86):
//!   if mob.onGround():
//!       mob.setDeltaMovement(mob.getDeltaMovement().multiply(0.10000000149011612, 1.0, 0.10000000149011612))
//!       level.playSound(null, mob, landingSound, NEUTRAL, 2.0f, 1.0f)
//!   mob.setDiscardFriction(false); mob.setPose(Pose.STANDING)
//!   brain.eraseMemory(LONG_JUMP_MID_JUMP)
//!   brain.setMemory(LONG_JUMP_COOLDOWN_TICKS, timeBetweenLongJumps.sample(level.random))
//! ```
//!
//! **The horizontal damping is `0.10000000149011612`, the widened `float`
//! `0.1f`**, and the `y` factor is `1.0` — so a landing kills 90 % of
//! horizontal speed and leaves vertical speed alone. The landing sound is
//! at volume **2.0f**, not the 1.0f the take-off sound uses.
//!
//! **The cooldown here is the full sample, not the halved one.** Both of
//! `LongJumpToRandomPos`'s cooldown writes divide by 2 and this one does
//! not, so the cooldown after a completed jump is twice the one after a
//! refused start. One draw on the level random either way.
//!
//! `stop` runs its four unconditional statements whether or not the mob
//! landed, so a jump that times out at 100 ticks still clears the pose,
//! the friction flag and the memory, and still pays the cooldown.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::behaviors::set_entity_look_target_sometimes::UniformInt;
use crate::brain::memory::{MemoryModuleType, MemoryStatus, MemoryValue};
use crate::brain::BrainState;
use crate::host::{MobHost, Pose, SoundSpec};

/// `TIME_OUT_DURATION`.
pub const TIME_OUT_DURATION: i32 = 100;
/// The `0.1f` at 12/16, widened as the `ldc2_w` carries it.
pub const LANDING_HORIZONTAL_DAMPING: f64 = 0.100_000_001_490_116_12;

/// `LongJumpMidJump`'s body.
#[derive(Debug, Clone, Copy)]
pub struct LongJumpMidJump {
    /// `timeBetweenLongJumps`.
    pub time_between_long_jumps: UniformInt,
    /// `landingSound`.
    pub landing_sound: SoundSpec,
}

impl LongJumpMidJump {
    /// `LongJumpMidJump(UniformInt, SoundEvent)`.
    #[must_use]
    pub fn new(
        time_between_long_jumps: UniformInt,
        landing_sound: SoundSpec,
    ) -> Behavior<LongJumpMidJump> {
        Behavior::with_duration(
            LongJumpMidJump {
                time_between_long_jumps,
                landing_sound,
            },
            vec![
                (MemoryModuleType::LookTarget, MemoryStatus::Registered),
                (
                    MemoryModuleType::LongJumpMidJump,
                    MemoryStatus::ValuePresent,
                ),
            ],
            TIME_OUT_DURATION,
        )
    }
}

impl<M: MobHost> BehaviorBody<M> for LongJumpMidJump {
    fn can_still_use(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        !cx.mob.on_ground()
    }

    fn start(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        cx.mob.set_discard_friction(true);
        cx.mob.set_pose(Pose::LongJumping);
    }

    fn stop(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        if cx.mob.on_ground() {
            let d = cx.mob.delta_movement();
            cx.mob.set_delta_movement(
                d.x * LANDING_HORIZONTAL_DAMPING,
                d.y,
                d.z * LANDING_HORIZONTAL_DAMPING,
            );
            let sound = self.landing_sound;
            cx.mob.play_sound(&sound, 2.0, 1.0);
        }
        cx.mob.set_discard_friction(false);
        cx.mob.set_pose(Pose::Standing);
        brain.erase_memory(MemoryModuleType::LongJumpMidJump);
        let n = self.time_between_long_jumps.sample_level(&mut cx.mob);
        brain.set_memory_value(MemoryModuleType::LongJumpCooldownTicks, MemoryValue::Int(n));
    }

    fn name(&self) -> &'static str {
        "LongJumpMidJump"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn landing_damps_x_and_z_only_and_pays_the_full_cooldown() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut brain = Brain::<TestMob>::new(
            &[T::LookTarget, T::LongJumpMidJump, T::LongJumpCooldownTicks],
            vec![],
        )
        .state;
        brain.set_memory_value(T::LongJumpMidJump, MemoryValue::Bool(true));
        let mut b = LongJumpMidJump::new(UniformInt::of(60, 100), SoundSpec("frog.land"));
        cx.mob.on_ground = false;
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(cx.mob.discard_friction);
        assert_eq!(cx.mob.pose, Pose::LongJumping);
        cx.mob.set_delta_movement(1.0, -0.5, 2.0);
        cx.mob.on_ground = true;
        // `Behavior.tryStart` itself draws one `nextInt(1)` for the fixed
        // duration, so the twin replays that first.
        let mut twin = TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1);
        let _ = MobHost::level_random_next_int(&mut twin, 1);
        let expected = UniformInt::of(60, 100).sample_level(&mut twin);
        b.do_stop(&mut brain, &mut cx, 5);
        let d = cx.mob.delta_movement();
        assert!((d.x - LANDING_HORIZONTAL_DAMPING).abs() < 1e-15);
        assert!((d.y + 0.5).abs() < 1e-15, "y is multiplied by 1.0");
        assert!((d.z - 2.0 * LANDING_HORIZONTAL_DAMPING).abs() < 1e-15);
        assert!(!cx.mob.discard_friction);
        assert_eq!(cx.mob.pose, Pose::Standing);
        assert!(!brain.has_memory_value(T::LongJumpMidJump));
        assert_eq!(
            brain.get_memory(T::LongJumpCooldownTicks),
            Some(&MemoryValue::Int(expected)),
            "the FULL sample, unlike LongJumpToRandomPos's halved one"
        );
    }

    #[test]
    fn a_timeout_in_the_air_still_clears_the_pose_and_pays_the_cooldown() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut brain = Brain::<TestMob>::new(
            &[T::LookTarget, T::LongJumpMidJump, T::LongJumpCooldownTicks],
            vec![],
        )
        .state;
        brain.set_memory_value(T::LongJumpMidJump, MemoryValue::Bool(true));
        let mut b = LongJumpMidJump::new(UniformInt::of(60, 100), SoundSpec("frog.land"));
        cx.mob.on_ground = false;
        assert!(b.try_start(&mut brain, &mut cx, 0));
        cx.mob.set_delta_movement(1.0, -0.5, 2.0);
        b.do_stop(&mut brain, &mut cx, 5);
        let d = cx.mob.delta_movement();
        assert!((d.x - 1.0).abs() < 1e-15, "not on the ground: no damping");
        assert_eq!(cx.mob.pose, Pose::Standing);
        assert!(brain.has_memory_value(T::LongJumpCooldownTicks));
        assert!(
            !cx.mob.calls.iter().any(|c| c.starts_with("playSound")),
            "and no landing sound"
        );
    }
}
