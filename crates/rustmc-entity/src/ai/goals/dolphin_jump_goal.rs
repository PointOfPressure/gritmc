//! `net.minecraft.world.entity.ai.goal.JumpGoal` (flags `MOVE, JUMP`) and
//! its one subclass `DolphinJumpGoal`, from the 26.2 jar. Used by Dolphin.
//!
//! `STEPS_TO_CHECK = {0, 1, 4, 5, 6, 7}`; `interval = reducedTickDelay(arg)`.
//!
//! ```text
//! canUse()          if random.nextInt(interval) != 0 return false        (offset 11)
//!                   dir = dolphin.getMotionDirection(); dx = stepX; dz = stepZ
//!                   for step in STEPS_TO_CHECK:
//!                       if !waterIsClear(pos, dx, dz, step) || !surfaceIsClear(pos, dx, dz, step) return false
//!                   return true
//! waterIsClear      p = pos.offset(dx * step, 0, dz * step); fluid is WATER && !state.blocksMotion()
//! surfaceIsClear    offset(dx * step, 1, dz * step).isAir() && offset(dx * step, 2, dz * step).isAir()
//! canContinueToUse  d = deltaMovement.y
//!                   if d * d < 0.03 && xRot != 0 && |xRot| < 10 && isInWater() return false
//!                   return !onGround()
//! isInterruptable   false
//! start()           dir; setDeltaMovement(delta.add(dx * 0.6, 0.7, dz * 0.6)); navigation.stop()
//! stop()            setXRot(0)
//! tick()            bl = breached; if !bl: breached = fluid at blockPosition is WATER
//!                   if breached && !bl: playSound(DOLPHIN_JUMP, 1, 1)
//!                   v = deltaMovement
//!                   if v.y * v.y < 0.03 && xRot != 0: setXRot(Mth.rotLerp(0.2f, xRot, 0))
//!                   else if v.length() > 1.0E-5: e = Math.atan2(-v.y, v.horizontalDistance()) * 57.2957763671875; setXRot((float) e)
//! ```
//!
//! The `0.03` is the double `0.029999999329447746` (`0.03f` widened) and
//! the length floor `9.999999747378752E-6` (`1.0E-5f`); both are used
//! verbatim. `Math.atan2` here is `java.lang.Math`'s, not `Mth`'s table
//! version — this port uses `f64::atan2`, whose last bit against the
//! JVM intrinsic is not verified.

use rustmc_worldgen::rng::RandomSource;

use crate::ai::goal::{reduced_tick_delay, Flag, FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::host::{MobHost, PathLevel, SoundSpec};
use crate::mth::rot_lerp;
use crate::phys::offset;

/// `STEPS_TO_CHECK`.
pub const STEPS_TO_CHECK: [i32; 6] = [0, 1, 4, 5, 6, 7];
/// `SoundEvents.DOLPHIN_JUMP`.
pub const DOLPHIN_JUMP: SoundSpec = SoundSpec("entity.dolphin.jump");
const Y_SPEED_SQ: f64 = 0.029_999_999_329_447_746;
const LENGTH_FLOOR: f64 = 9.999_999_747_378_752E-6;

/// `DolphinJumpGoal`.
#[derive(Debug, Clone)]
pub struct DolphinJumpGoal {
    interval: i32,
    breached: bool,
}

impl DolphinJumpGoal {
    /// `DolphinJumpGoal(Dolphin, int interval)`.
    #[must_use]
    pub fn new(interval: i32) -> Self {
        DolphinJumpGoal {
            interval: reduced_tick_delay(interval),
            breached: false,
        }
    }

    fn water_is_clear<M: MobHost>(
        mob: &M,
        pos: rustmc_data::BlockPos,
        dx: i32,
        dz: i32,
        step: i32,
    ) -> bool {
        let f = mob
            .level()
            .block_facts(offset(pos, dx * step, 0, dz * step));
        f.fluid_is_water && !f.blocks_motion
    }

    fn surface_is_clear<M: MobHost>(
        mob: &M,
        pos: rustmc_data::BlockPos,
        dx: i32,
        dz: i32,
        step: i32,
    ) -> bool {
        let level = mob.level();
        level
            .block_facts(offset(pos, dx * step, 1, dz * step))
            .is_air
            && level
                .block_facts(offset(pos, dx * step, 2, dz * step))
                .is_air
    }
}

impl<M: MobHost> Goal<MobAi<M>> for DolphinJumpGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        if cx.mob.random().next_i32_bound(self.interval) != 0 {
            return false;
        }
        let dir = cx.mob.motion_direction();
        let (dx, dz) = (dir.step_x(), dir.step_z());
        let pos = cx.mob.block_position();
        for step in STEPS_TO_CHECK {
            if !Self::water_is_clear(&cx.mob, pos, dx, dz, step)
                || !Self::surface_is_clear(&cx.mob, pos, dx, dz, step)
            {
                return false;
            }
        }
        true
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        let d = cx.mob.delta_movement().y;
        let x = cx.mob.x_rot();
        if d * d < Y_SPEED_SQ && x != 0.0 && x.abs() < 10.0 && cx.mob.is_in_water() {
            return false;
        }
        !cx.mob.on_ground()
    }

    fn is_interruptable(&self) -> bool {
        false
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        let dir = cx.mob.motion_direction();
        let v = cx.mob.delta_movement().add3(
            f64::from(dir.step_x()) * 0.6,
            0.7,
            f64::from(dir.step_z()) * 0.6,
        );
        cx.mob.set_delta_movement(v.x, v.y, v.z);
        cx.navigation.stop();
    }

    fn stop(&mut self, cx: &mut MobAi<M>) {
        cx.mob.set_x_rot(0.0);
    }

    #[allow(clippy::cast_possible_truncation)]
    fn tick(&mut self, cx: &mut MobAi<M>) {
        let bl = self.breached;
        if !bl {
            self.breached = cx
                .mob
                .level()
                .block_facts(cx.mob.block_position())
                .fluid_is_water;
        }
        if self.breached && !bl {
            cx.mob.play_sound(&DOLPHIN_JUMP, 1.0, 1.0);
        }
        let v = cx.mob.delta_movement();
        let x = cx.mob.x_rot();
        if v.y * v.y < Y_SPEED_SQ && x != 0.0 {
            cx.mob.set_x_rot(rot_lerp(0.2, x, 0.0));
        } else if v.length() > LENGTH_FLOOR {
            let d = v.horizontal_distance_sqr().sqrt();
            let e = (-v.y).atan2(d) * 57.295_776_367_187_5;
            cx.mob.set_x_rot(e as f32);
        }
    }

    fn flags(&self) -> FlagSet {
        FlagSet::of2(Flag::Move, Flag::Jump)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::BlockFacts;
    use crate::testing::{GridLevel, TestMob};

    /// Facing south in an open sea with air above: the jump fires on the
    /// zero draw of `nextInt(reduced(10)) = nextInt(5)` and kicks the
    /// dolphin (0, 0.7, 0.6).
    #[test]
    fn jumps_along_its_facing_over_clear_water() {
        let mut g = GridLevel::new();
        for z in 0..8 {
            g.set(0, 64, z, BlockFacts::WATER);
        }
        let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, 1);
        mob.y_rot = 0.0;
        mob.on_ground = false;
        let mut ai = MobAi::new(mob);
        let mut goal = DolphinJumpGoal::new(10);
        let mut fired = false;
        for _ in 0..40 {
            if goal.can_use(&mut ai) {
                fired = true;
                break;
            }
        }
        assert!(fired);
        goal.start(&mut ai);
        let d = ai.mob.delta_movement;
        assert_eq!((d.x, d.y, d.z), (0.0, 0.7, 0.6));
        assert!(
            goal.can_continue_to_use(&mut ai),
            "in the air, not on the ground"
        );
        goal.tick(&mut ai);
        assert!(ai.mob.x_rot < 0.0, "nose up while rising: {}", ai.mob.x_rot);
        ai.mob.on_ground = true;
        assert!(!goal.can_continue_to_use(&mut ai));
        goal.stop(&mut ai);
        assert_eq!(ai.mob.x_rot, 0.0);
    }

    #[test]
    fn a_block_in_the_lane_refuses() {
        let mut g = GridLevel::new();
        for z in 0..8 {
            g.set(0, 64, z, BlockFacts::WATER);
        }
        let mut stone = BlockFacts::NONE;
        stone.is_solid = true;
        g.set(0, 65, 5, stone);
        let mob = TestMob::new(g, 0.5, 64.0, 0.5, 1);
        let mut ai = MobAi::new(mob);
        let mut goal = DolphinJumpGoal::new(10);
        for _ in 0..40 {
            assert!(!goal.can_use(&mut ai));
        }
    }
}
