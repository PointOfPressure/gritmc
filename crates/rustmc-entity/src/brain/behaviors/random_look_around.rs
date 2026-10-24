//! `net.minecraft.world.entity.ai.behavior.RandomLookAround`, from the
//! 26.2 jar. `Behavior<Mob>` with entry `{LOOK_TARGET: VALUE_ABSENT,
//! GAZE_COOLDOWN_TICKS: VALUE_ABSENT}` and the base's default duration of
//! 60. `start` is the whole class; there is no `tick` and no `stop`.
//!
//! ```text
//! RandomLookAround(interval, maxYaw, minPitch, maxPitch)  (ctor 0..62)
//!   super(map)                                     <- the ONE-argument Behavior ctor: duration 60
//!   if minPitch > maxPitch: throw IllegalArgumentException
//!   pitchRange = maxPitch - minPitch
//!
//! start (0..117), FOUR reads of the mob random in this order:
//!   random = mob.getRandom()
//!   pitch = Mth.clamp(random.nextFloat() * pitchRange + minPitch, -90.0f, 90.0f)          <- draw 1
//!   yaw   = Mth.wrapDegrees(mob.getYRot() + 2.0f * random.nextFloat() * maxYaw - maxYaw)  <- draw 2
//!   dir   = Vec3.directionFromRotation(pitch, yaw)
//!   brain.setMemory(LOOK_TARGET, new BlockPosTracker(mob.getEyePosition().add(dir)))
//!   brain.setMemory(GAZE_COOLDOWN_TICKS, interval.sample(random))                          <- draw 3 (+)
//! ```
//!
//! **`2.0f * nextFloat() * maxYaw - maxYaw` is evaluated left to right in
//! `float`** (36..55: `fconst_2; nextFloat; fmul; getfield maxYaw; fmul;
//! fadd; getfield maxYaw; fsub`), so the yaw offset is
//! `yRot + (2 * r * maxYaw) - maxYaw` with each product rounded to `f32`
//! — not `yRot + maxYaw * (2r - 1)`.
//!
//! **All the draws are on the MOB random, not the level's**, and the
//! interval's sample comes last, after both memories' inputs are computed
//! but interleaved as shown: pitch, yaw, then the interval. A twin must
//! replay them in that order.
//!
//! The constructor's `minPitch > maxPitch` check throws, so the port
//! asserts rather than clamping.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::behaviors::set_entity_look_target_sometimes::UniformInt;
use crate::brain::memory::{MemoryModuleType, MemoryStatus, MemoryValue, PositionTracker};
use crate::brain::BrainState;
use crate::host::MobHost;
use crate::phys::Vec3;
use rustmc_worldgen::rng::RandomSource;

/// `RandomLookAround`'s body.
#[derive(Debug, Clone, Copy)]
pub struct RandomLookAround {
    /// `interval`.
    pub interval: UniformInt,
    /// `maxYaw`.
    pub max_yaw: f32,
    /// `minPitch`.
    pub min_pitch: f32,
    /// `pitchRange` = `maxPitch - minPitch`.
    pub pitch_range: f32,
}

impl RandomLookAround {
    /// `RandomLookAround(IntProvider, float, float, float)`.
    ///
    /// # Panics
    /// When `min_pitch > max_pitch`, as the constructor's
    /// `IllegalArgumentException` does.
    #[must_use]
    pub fn new(
        interval: UniformInt,
        max_yaw: f32,
        min_pitch: f32,
        max_pitch: f32,
    ) -> Behavior<RandomLookAround> {
        assert!(
            min_pitch <= max_pitch,
            "Minimum pitch is larger than maximum pitch! {min_pitch} > {max_pitch}"
        );
        Behavior::new(
            RandomLookAround {
                interval,
                max_yaw,
                min_pitch,
                pitch_range: max_pitch - min_pitch,
            },
            vec![
                (MemoryModuleType::LookTarget, MemoryStatus::ValueAbsent),
                (
                    MemoryModuleType::GazeCooldownTicks,
                    MemoryStatus::ValueAbsent,
                ),
            ],
        )
    }
}

impl<M: MobHost> BehaviorBody<M> for RandomLookAround {
    fn start(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        let r1 = cx.mob.random().next_f32();
        let pitch = crate::mth::clamp_f32(r1 * self.pitch_range + self.min_pitch, -90.0, 90.0);
        let r2 = cx.mob.random().next_f32();
        let yaw = crate::mth::wrap_degrees(cx.mob.y_rot() + 2.0 * r2 * self.max_yaw - self.max_yaw);
        let dir = crate::mth::direction_from_rotation(pitch, yaw);
        let eye = Vec3 {
            x: cx.mob.x(),
            y: cx.mob.eye_y(),
            z: cx.mob.z(),
        };
        brain.set_memory_value(
            MemoryModuleType::LookTarget,
            MemoryValue::PositionTracker(PositionTracker::block_at(Vec3 {
                x: eye.x + dir.x,
                y: eye.y + dir.y,
                z: eye.z + dir.z,
            })),
        );
        let n = self.interval.sample_mob(&mut cx.mob);
        brain.set_memory_value(MemoryModuleType::GazeCooldownTicks, MemoryValue::Int(n));
    }

    fn name(&self) -> &'static str {
        "RandomLookAround"
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
    fn the_three_mob_random_draws_happen_in_pitch_yaw_interval_order() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        cx.mob.y_rot = 30.0;
        let mut brain = Brain::<TestMob>::new(&[T::LookTarget, T::GazeCooldownTicks], vec![]).state;
        let mut b = RandomLookAround::new(UniformInt::of(30, 60), 10.0, -5.0, 5.0);
        assert_eq!(b.body.pitch_range, 10.0, "maxPitch - minPitch");
        let mut twin = TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1);
        let r1 = twin.random().next_f32();
        let r2 = twin.random().next_f32();
        let expected_pitch = crate::mth::clamp_f32(r1 * 10.0 + -5.0, -90.0, 90.0);
        let expected_yaw = crate::mth::wrap_degrees(30.0 + 2.0 * r2 * 10.0 - 10.0);
        let expected_interval = UniformInt::of(30, 60).sample_mob(&mut twin);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        let dir = crate::mth::direction_from_rotation(expected_pitch, expected_yaw);
        let want = PositionTracker::block_at(Vec3 {
            x: 0.5 + dir.x,
            y: cx.mob.eye_y() + dir.y,
            z: 0.5 + dir.z,
        });
        assert_eq!(
            brain.get_memory(T::LookTarget),
            Some(&MemoryValue::PositionTracker(want))
        );
        assert_eq!(
            brain.get_memory(T::GazeCooldownTicks),
            Some(&MemoryValue::Int(expected_interval)),
            "the interval is drawn LAST, on the mob random"
        );
        // The cooldown it just wrote is VALUE_ABSENT in the entry, so it
        // cannot immediately run again.
        brain.erase_memory(T::LookTarget);
        assert!(!b.try_start(&mut brain, &mut cx, 1));
    }
}
