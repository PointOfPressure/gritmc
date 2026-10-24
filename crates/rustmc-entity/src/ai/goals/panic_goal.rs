//! `net.minecraft.world.entity.ai.goal.PanicGoal`, from the 26.2 jar.
//! Flag `MOVE`.
//!
//! # `canUse()`, offsets 0..73
//!
//! ```text
//! if !shouldPanic() return false
//! if mob.isOnFire():
//!     pos = lookForWater(level, mob, 5)
//!     if pos != null: posX/Y/Z = pos; return true
//! return findRandomPosition()
//! ```
//!
//! `shouldPanic()` is `getLastDamageSource() != null && it.is(panicCausingDamageTypes.apply(mob))`,
//! `DamageTypeTags.PANIC_CAUSES` by default; the host answers it as
//! [`MobHost::last_damage_source_causes_panic`]. `findRandomPosition()` is
//! `DefaultRandomPos.getPos(mob, 5, 4)` — draws `nextInt(11)`,
//! `nextInt(9)`, `nextInt(11)` per attempt.
//!
//! `lookForWater(level, mob, radius)`: `null` when the mob's own block has
//! a collision shape, else the first water fluid in
//! `BlockPos.findClosestMatch(pos, 5, 1, ...)` — note the literal `5` and
//! `1`, not the `radius` argument, at offsets 32..33.

use rustmc_data::BlockPos;

use crate::ai::goal::{Flag, FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::ai::random_pos::default_get_pos;
use crate::host::{MobHost, PathLevel};
use crate::phys::find_closest_match;

/// `PanicGoal.WATER_CHECK_DISTANCE_VERTICAL`.
pub const WATER_CHECK_DISTANCE_VERTICAL: i32 = 1;

/// `PanicGoal`.
#[derive(Debug, Clone)]
pub struct PanicGoal {
    speed_modifier: f64,
    pos_x: f64,
    pos_y: f64,
    pos_z: f64,
    is_running: bool,
}

impl PanicGoal {
    /// `PanicGoal(PathfinderMob, double)`, with `DamageTypeTags.PANIC_CAUSES`.
    #[must_use]
    pub fn new(speed_modifier: f64) -> Self {
        PanicGoal {
            speed_modifier,
            pos_x: 0.0,
            pos_y: 0.0,
            pos_z: 0.0,
            is_running: false,
        }
    }

    /// `shouldPanic()`.
    fn should_panic<M: MobHost>(cx: &MobAi<M>) -> bool {
        cx.mob.last_damage_source_causes_panic()
    }

    /// `findRandomPosition()`.
    fn find_random_position<M: MobHost>(&mut self, cx: &mut MobAi<M>) -> bool {
        let Some(v) = default_get_pos(cx, 5, 4) else {
            return false;
        };
        self.pos_x = v.x;
        self.pos_y = v.y;
        self.pos_z = v.z;
        true
    }

    /// `lookForWater(BlockGetter, Entity, int)`.
    #[must_use]
    pub fn look_for_water<M: MobHost>(mob: &M, _radius: i32) -> Option<BlockPos> {
        let pos = mob.block_position();
        let level = mob.level();
        if level.collision_shape_max_y(pos).is_some() {
            return None;
        }
        find_closest_match(pos, 5, 1, |p| level.block_facts(p).fluid_is_water)
    }

    /// `isRunning()`.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.is_running
    }

    /// The panic target, for tests.
    #[must_use]
    pub fn target(&self) -> (f64, f64, f64) {
        (self.pos_x, self.pos_y, self.pos_z)
    }
}

impl<M: MobHost> Goal<MobAi<M>> for PanicGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        if !Self::should_panic(cx) {
            return false;
        }
        if cx.mob.is_on_fire() {
            if let Some(p) = Self::look_for_water(&cx.mob, 5) {
                self.pos_x = f64::from(p.x);
                self.pos_y = f64::from(p.y);
                self.pos_z = f64::from(p.z);
                return true;
            }
        }
        self.find_random_position(cx)
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        let (x, y, z, s) = (self.pos_x, self.pos_y, self.pos_z, self.speed_modifier);
        cx.navigation.move_to(&mut cx.mob, x, y, z, s);
        self.is_running = true;
    }

    fn stop(&mut self, _cx: &mut MobAi<M>) {
        self.is_running = false;
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        !cx.navigation.is_done()
    }

    fn flags(&self) -> FlagSet {
        FlagSet::of(Flag::Move)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::BlockFacts;
    use crate::testing::{GridLevel, TestMob};

    /// A mob whose last damage was not a panic cause never panics; one
    /// that was and is on fire runs for the nearest water, found in the
    /// Manhattan order (the water at -x is found before the one at +z).
    #[test]
    fn a_burning_panicker_runs_for_the_closest_water() {
        let mut g = GridLevel::new();
        g.floor(-20, -20, 20, 20, 63);
        g.set(-2, 64, 0, BlockFacts::WATER);
        g.set(0, 64, 2, BlockFacts::WATER);
        let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, 4);
        mob.on_fire = true;
        let mut ai = MobAi::new(mob);
        let mut goal = PanicGoal::new(1.25);
        assert!(!goal.can_use(&mut ai), "no panic cause");
        ai.mob.panics = true;
        assert!(goal.can_use(&mut ai));
        assert_eq!(goal.target(), (-2.0, 64.0, 0.0));
        goal.start(&mut ai);
        assert!(goal.is_running());
        assert!(goal.can_continue_to_use(&mut ai));
        goal.stop(&mut ai);
        assert!(!goal.is_running());
    }

    /// Not on fire: a random position within 5 x 4 of the mob.
    #[test]
    fn a_panicker_not_on_fire_picks_a_random_position() {
        let mut g = GridLevel::new();
        g.floor(-20, -20, 20, 20, 63);
        let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, 4);
        mob.panics = true;
        let mut ai = MobAi::new(mob);
        let mut goal = PanicGoal::new(1.25);
        assert!(goal.can_use(&mut ai));
        let (x, y, z) = goal.target();
        assert!((x - 0.5).abs() <= 5.0 && (z - 0.5).abs() <= 5.0, "{x} {z}");
        assert_eq!(y, 64.0);
    }
}
