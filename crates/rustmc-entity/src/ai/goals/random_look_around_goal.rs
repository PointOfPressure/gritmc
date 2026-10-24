//! `net.minecraft.world.entity.ai.goal.RandomLookAroundGoal`, from the
//! 26.2 jar. Flags `MOVE, LOOK`.
//!
//! # Random draws
//!
//! * `canUse()` — `getRandom().nextFloat() < 0.02f` (offsets 4..15): one
//!   float, every time the selector asks.
//! * `start()` — `nextDouble()` at offset 10 for the angle
//!   (`2 * PI * d`), then `nextInt(20)` at offset 45 for `lookTime = 20 + n`.
//!
//! `relX = Math.cos(angle)`, `relZ = Math.sin(angle)` — the platform's
//! `cos`/`sin`, not `Mth`'s tables.
//!
//! `requiresUpdateEveryTick` is true, so `tick()` runs on the half ticks
//! too: `lookTime--` then `lookControl.setLookAt(x + relX, eyeY, z + relZ)`,
//! and `canContinueToUse` is `lookTime >= 0`.

use rustmc_worldgen::rng::RandomSource;

use crate::ai::goal::{Flag, FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::host::MobHost;

/// `RandomLookAroundGoal`.
#[derive(Debug, Clone, Default)]
pub struct RandomLookAroundGoal {
    rel_x: f64,
    rel_z: f64,
    look_time: i32,
}

impl RandomLookAroundGoal {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `lookTime`, for tests.
    #[must_use]
    pub fn look_time(&self) -> i32 {
        self.look_time
    }
}

impl<M: MobHost> Goal<MobAi<M>> for RandomLookAroundGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        cx.mob.random().next_f32() < 0.02
    }

    fn can_continue_to_use(&mut self, _cx: &mut MobAi<M>) -> bool {
        self.look_time >= 0
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        let angle = std::f64::consts::TAU * cx.mob.random().next_f64();
        self.rel_x = angle.cos();
        self.rel_z = angle.sin();
        self.look_time = 20 + cx.mob.random().next_i32_bound(20);
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn tick(&mut self, cx: &mut MobAi<M>) {
        self.look_time -= 1;
        let (x, y, z) = (
            cx.mob.x() + self.rel_x,
            cx.mob.eye_y(),
            cx.mob.z() + self.rel_z,
        );
        cx.look_control.set_look_at(&cx.mob, x, y, z);
    }

    fn flags(&self) -> FlagSet {
        FlagSet::of2(Flag::Move, Flag::Look)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{GridLevel, TestMob};
    use rustmc_worldgen::rng::LegacyRandomSource;

    fn ai(seed: i64) -> MobAi<TestMob> {
        let mut g = GridLevel::new();
        g.floor(-4, -4, 4, 4, 63);
        MobAi::new(TestMob::new(g, 0.5, 64.0, 0.5, seed))
    }

    /// The draws are the ones named above, in that order, and nothing
    /// else touches the random: replaying the same seed by hand gives the
    /// same relX/relZ/lookTime.
    #[test]
    fn start_draws_a_double_then_an_int() {
        let mut ai = ai(42);
        let mut g = RandomLookAroundGoal::new();
        g.start(&mut ai);
        let mut r = LegacyRandomSource::new(42);
        let angle = std::f64::consts::TAU * r.next_f64();
        assert_eq!(g.rel_x, angle.cos());
        assert_eq!(g.rel_z, angle.sin());
        assert_eq!(g.look_time(), 20 + r.next_i32_bound(20));
        assert!(g.can_continue_to_use(&mut ai));
        for _ in 0..=g.look_time() {
            g.tick(&mut ai);
        }
        assert_eq!(g.look_time(), -1);
        assert!(!g.can_continue_to_use(&mut ai));
        assert!(ai.look_control.is_looking_at_target());
    }

    /// `canUse` draws exactly one float per ask.
    #[test]
    fn can_use_draws_one_float() {
        let mut ai = ai(7);
        let mut g = RandomLookAroundGoal::new();
        let mut r = LegacyRandomSource::new(7);
        for _ in 0..50 {
            let want = r.next_f32() < 0.02;
            assert_eq!(g.can_use(&mut ai), want);
        }
    }
}
