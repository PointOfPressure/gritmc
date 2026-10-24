//! `net.minecraft.world.entity.ai.goal.MoveTowardsRestrictionGoal`, from
//! the 26.2 jar. Flag `MOVE`. Used by Blaze, Guardian, WanderingTrader.
//!
//! # `canUse()`, offsets 0..68
//!
//! ```text
//! if mob.isWithinHome() return false                                    (0..11)
//! v = DefaultRandomPos.getPosTowards(mob, 16, 7,
//!         Vec3.atBottomCenterOf(mob.getHomePosition()), 1.5707963705062866)
//! if v == null return false
//! wantedX/Y/Z = v; return true
//! ```
//!
//! The angle constant is the float `(float) Math.PI / 2` widened to a
//! double, `1.5707963705062866`, and is used verbatim. `getPosTowards`
//! draws the same three ints per attempt as the first lane's
//! `default_get_pos_towards` documents.
//!
//! `canContinueToUse()` is `!navigation.isDone()`; `start()` is
//! `navigation.moveTo(wantedX, wantedY, wantedZ, speedModifier)`.

use crate::ai::goal::{Flag, FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::ai::random_pos::default_get_pos_towards;
use crate::host::MobHost;
use crate::phys::Vec3;

/// The `ldc2_w` at offset 30: `(float) Math.PI / 2` as a double.
pub const HALF_PI_F32_AS_F64: f64 = 1.570_796_370_506_286_6;

/// `MoveTowardsRestrictionGoal`.
#[derive(Debug, Clone)]
pub struct MoveTowardsRestrictionGoal {
    wanted_x: f64,
    wanted_y: f64,
    wanted_z: f64,
    speed_modifier: f64,
}

impl MoveTowardsRestrictionGoal {
    /// `MoveTowardsRestrictionGoal(PathfinderMob, double)`.
    #[must_use]
    pub fn new(speed_modifier: f64) -> Self {
        MoveTowardsRestrictionGoal {
            wanted_x: 0.0,
            wanted_y: 0.0,
            wanted_z: 0.0,
            speed_modifier,
        }
    }

    /// The wanted position, for tests.
    #[must_use]
    pub fn wanted(&self) -> (f64, f64, f64) {
        (self.wanted_x, self.wanted_y, self.wanted_z)
    }
}

impl<M: MobHost> Goal<MobAi<M>> for MoveTowardsRestrictionGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        if cx.mob.is_within_home() {
            return false;
        }
        let home = Vec3::at_bottom_center_of(cx.mob.home_position());
        let Some(v) = default_get_pos_towards(cx, 16, 7, home, HALF_PI_F32_AS_F64) else {
            return false;
        };
        self.wanted_x = v.x;
        self.wanted_y = v.y;
        self.wanted_z = v.z;
        true
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
    use rustmc_data::BlockPos;

    use crate::testing::{GridLevel, TestMob};

    /// Within home: refused. Outside a 4-block home 12 blocks away the
    /// candidate must land inside the home (`mobRestricted` holds, so
    /// `isRestricted` rejects anything outside it), which most seeds'
    /// ten attempts miss; the first seed that hits gives a position
    /// nearer the home than the mob is.
    #[test]
    fn walks_back_toward_a_home_it_has_left() {
        let mut goal = MoveTowardsRestrictionGoal::new(1.0);
        let mut hit = None;
        for seed in 1..200 {
            let mut g = GridLevel::new();
            g.floor(-40, -40, 40, 40, 63);
            let mut mob = TestMob::new(g, 12.5, 64.0, 0.5, seed);
            mob.home_radius = 4;
            mob.home_position = BlockPos { x: 0, y: 64, z: 0 };
            let mut ai = MobAi::new(mob);
            if goal.can_use(&mut ai) {
                hit = Some((seed, goal.wanted()));
                ai.mob.home_radius = 20;
                assert!(!goal.can_use(&mut ai), "within home");
                break;
            }
        }
        let (seed, (x, _, z)) = hit.expect("some seed lands a candidate in the home");
        assert!(
            x * x + z * z < 16.0,
            "seed {seed}: {x} {z} is inside the home"
        );
    }
}
