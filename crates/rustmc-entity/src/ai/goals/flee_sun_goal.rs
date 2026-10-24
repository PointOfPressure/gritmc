//! `net.minecraft.world.entity.ai.goal.FleeSunGoal`, from the 26.2 jar.
//! Flag `MOVE`. Used by AbstractSkeleton.
//!
//! `canUse()`, offsets 0..77: `mob.getTarget() == null && level.isBrightOutside()
//! && mob.isOnFire() && level.canSeeSky(mob.blockPosition())
//! && mob.getItemBySlot(HEAD).isEmpty() && setWantedPos()`.
//!
//! `getHidePos()`, offsets 0..101: ten candidates
//! `pos.offset(nextInt(20) - 10, nextInt(6) - 3, nextInt(20) - 10)`
//! (three draws each, in that order); the first that cannot see the sky
//! and has `getWalkTargetValue(p) < 0.0f` is answered as
//! `Vec3.atBottomCenterOf(p)`.
//!
//! `canContinueToUse()`: `!navigation.isDone()`. `start()`:
//! `navigation.moveTo(wantedX, wantedY, wantedZ, speedModifier)`.

use rustmc_worldgen::rng::RandomSource;

use crate::ai::goal::{Flag, FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::host::{MobHost, PathLevel};
use crate::phys::{offset, Vec3};

/// `FleeSunGoal`.
#[derive(Debug, Clone)]
pub struct FleeSunGoal {
    wanted_x: f64,
    wanted_y: f64,
    wanted_z: f64,
    speed_modifier: f64,
}

impl FleeSunGoal {
    /// `FleeSunGoal(PathfinderMob, double)`.
    #[must_use]
    pub fn new(speed_modifier: f64) -> Self {
        FleeSunGoal {
            wanted_x: 0.0,
            wanted_y: 0.0,
            wanted_z: 0.0,
            speed_modifier,
        }
    }

    /// `setWantedPos()`.
    fn set_wanted_pos<M: MobHost>(&mut self, cx: &mut MobAi<M>) -> bool {
        let Some(v) = Self::hide_pos(cx) else {
            return false;
        };
        self.wanted_x = v.x;
        self.wanted_y = v.y;
        self.wanted_z = v.z;
        true
    }

    /// `getHidePos()`.
    pub fn hide_pos<M: MobHost>(cx: &mut MobAi<M>) -> Option<Vec3> {
        let pos = cx.mob.block_position();
        for _ in 0..10 {
            let dx = cx.mob.random().next_i32_bound(20) - 10;
            let dy = cx.mob.random().next_i32_bound(6) - 3;
            let dz = cx.mob.random().next_i32_bound(20) - 10;
            let p = offset(pos, dx, dy, dz);
            if !cx.mob.level().can_see_sky(p) && cx.mob.walk_target_value(p) < 0.0 {
                return Some(Vec3::at_bottom_center_of(p));
            }
        }
        None
    }

    /// The wanted position, for tests.
    #[must_use]
    pub fn wanted(&self) -> (f64, f64, f64) {
        (self.wanted_x, self.wanted_y, self.wanted_z)
    }
}

impl<M: MobHost> Goal<MobAi<M>> for FleeSunGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        if cx.mob.target().is_some() {
            return false;
        }
        if !cx.mob.level().is_bright_outside() {
            return false;
        }
        if !cx.mob.is_on_fire() {
            return false;
        }
        if !cx.mob.level().can_see_sky(cx.mob.block_position()) {
            return false;
        }
        if !cx.mob.head_slot_empty() {
            return false;
        }
        self.set_wanted_pos(cx)
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        !cx.navigation.is_done()
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        let (x, y, z, s) = (
            self.wanted_x,
            self.wanted_y,
            self.wanted_z,
            self.speed_modifier,
        );
        cx.navigation.move_to(&mut cx.mob, x, y, z, s);
    }

    fn flags(&self) -> FlagSet {
        FlagSet::of(Flag::Move)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{GridLevel, TestMob};

    /// The default `getWalkTargetValue` is `0.0f`, never `< 0`, so a mob
    /// that does not override it (the base skeleton does: it is
    /// `Monster`'s `-level.getPathfindingCostFromLightLevels`) finds no
    /// hide position — the ten candidates still draw thirty ints.
    #[test]
    fn base_walk_target_value_never_hides_and_still_draws() {
        let mut g = GridLevel::new();
        g.floor(-20, -20, 20, 20, 63);
        g.bright_outside = true;
        let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, 1);
        mob.on_fire = true;
        let mut ai = MobAi::new(mob);
        let mut goal = FleeSunGoal::new(1.0);
        let mut twin = ai.mob.random;
        for _ in 0..30 {
            let _ = twin.next_i32_bound(20);
        }
        assert!(!goal.can_use(&mut ai));
        assert_eq!(format!("{:?}", ai.mob.random), format!("{twin:?}"));
        ai.mob.on_fire = false;
        let before = format!("{:?}", ai.mob.random);
        assert!(!goal.can_use(&mut ai));
        assert_eq!(
            before,
            format!("{:?}", ai.mob.random),
            "not on fire: no draw"
        );
    }
}
