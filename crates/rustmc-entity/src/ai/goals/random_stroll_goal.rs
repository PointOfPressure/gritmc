//! `net.minecraft.world.entity.ai.goal.RandomStrollGoal` and
//! `WaterAvoidingRandomStrollGoal`, from the 26.2 jar. Flag `MOVE`.
//!
//! # `canUse()`, offsets 0..105
//!
//! ```text
//! if mob.hasControllingPassenger() return false
//! if !forceTrigger:
//!     if checkNoActionTime && mob.getNoActionTime() >= 100 return false
//!     if random.nextInt(reducedTickDelay(interval)) != 0 return false     (40..59)
//! pos = getPosition(); if pos == null return false
//! wanted = pos; forceTrigger = false; return true
//! ```
//!
//! So the one draw in `canUse` is `nextInt(reducedTickDelay(120)) =
//! nextInt(60)` for the default interval, taken only when the mob is not
//! forced and has acted recently. `getPosition()` then draws through
//! `DefaultRandomPos.getPos(mob, 10, 7)` — see [`crate::ai::random_pos`].
//!
//! # `WaterAvoidingRandomStrollGoal.getPosition()`, offsets 0..71
//!
//! ```text
//! if mob.isInWater():
//!     p = LandRandomPos.getPos(mob, 15, 7)
//!     return p == null ? super.getPosition() : p
//! if random.nextFloat() >= probability:            (39..52, fcmpl iflt)
//!     return LandRandomPos.getPos(mob, 10, 7)
//! return super.getPosition()
//! ```
//!
//! The `nextFloat()` is drawn only when the mob is not in water, and its
//! default `probability` is `0.001f`: nearly always the land position.

use rustmc_worldgen::rng::RandomSource;

use crate::ai::goal::{reduced_tick_delay, Flag, FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::ai::random_pos::{
    air_and_water_get_pos, default_get_pos, default_get_pos_towards, get_random_swimmable_pos,
    hover_get_pos, land_get_pos,
};
use crate::host::MobHost;
use crate::phys::Vec3;

/// `RandomStrollGoal.DEFAULT_INTERVAL`.
pub const DEFAULT_INTERVAL: i32 = 120;
/// `WaterAvoidingRandomStrollGoal.PROBABILITY`.
pub const WATER_AVOIDING_PROBABILITY: f32 = 0.001;
/// The `ldc` float `1.5707964f` (`(float) Math.PI / 2`) the flying and
/// village strolls pass as their angle.
pub const HALF_PI_F32: f32 = 1.570_796_4;

/// Which `getPosition()` a stroll goal has: the base class's or the
/// water-avoiding subclass's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StrollKind {
    /// `RandomStrollGoal`.
    Default,
    /// `WaterAvoidingRandomStrollGoal` with its `probability`.
    WaterAvoiding { probability: f32 },
    /// `RandomSwimmingGoal`: `BehaviorUtils.getRandomSwimmablePos(mob, 10, 7)`.
    Swimming,
    /// `WaterAvoidingRandomFlyingGoal`: `HoverRandomPos.getPos(mob, 8, 7,
    /// view.x, view.z, (float) PI / 2, 3, 1)`, else `AirAndWaterRandomPos
    /// .getPos(mob, 8, 4, -2, view.x, view.z, (double)((float) PI / 2))`
    /// with `view = mob.getViewVector(0.0f)`.
    WaterAvoidingFlying,
    /// `GolemRandomStrollInVillageGoal`: `super(mob, speed, 240, false)`
    /// with the village-aware `getPosition` in
    /// [`crate::ai::goals::golem_random_stroll_in_village_goal`].
    GolemInVillage,
    /// `MoveBackToVillageGoal`: toward the centre of the closest village
    /// section within 2, `getPosTowards(10, 7, ...)`, or `null` when the
    /// mob's own section is the closest.
    BackToVillage,
}

/// `RandomStrollGoal` (and, by [`StrollKind`], its water-avoiding
/// subclass).
#[derive(Debug, Clone)]
pub struct RandomStrollGoal {
    kind: StrollKind,
    wanted_x: f64,
    wanted_y: f64,
    wanted_z: f64,
    speed_modifier: f64,
    interval: i32,
    force_trigger: bool,
    check_no_action_time: bool,
}

/// `WaterAvoidingRandomStrollGoal`: a [`RandomStrollGoal`] with the
/// water-avoiding `getPosition`.
pub type WaterAvoidingRandomStrollGoal = RandomStrollGoal;

impl RandomStrollGoal {
    /// `RandomStrollGoal(PathfinderMob, double)`: interval 120, check the
    /// no-action time.
    #[must_use]
    pub fn new(speed_modifier: f64) -> Self {
        Self::with_interval(speed_modifier, DEFAULT_INTERVAL)
    }

    /// `RandomStrollGoal(PathfinderMob, double, int)`.
    #[must_use]
    pub fn with_interval(speed_modifier: f64, interval: i32) -> Self {
        Self::with_interval_and_check(speed_modifier, interval, true)
    }

    /// `RandomStrollGoal(PathfinderMob, double, int, boolean)`.
    #[must_use]
    pub fn with_interval_and_check(
        speed_modifier: f64,
        interval: i32,
        check_no_action_time: bool,
    ) -> Self {
        RandomStrollGoal {
            kind: StrollKind::Default,
            wanted_x: 0.0,
            wanted_y: 0.0,
            wanted_z: 0.0,
            speed_modifier,
            interval,
            force_trigger: false,
            check_no_action_time,
        }
    }

    /// `WaterAvoidingRandomStrollGoal(PathfinderMob, double)`: probability
    /// `0.001f`.
    #[must_use]
    pub fn water_avoiding(speed_modifier: f64) -> Self {
        Self::water_avoiding_with_probability(speed_modifier, WATER_AVOIDING_PROBABILITY)
    }

    /// `WaterAvoidingRandomStrollGoal(PathfinderMob, double, float)`: the
    /// base constructor `(mob, speed)` — interval 120, checking the
    /// no-action time — plus the probability.
    #[must_use]
    pub fn water_avoiding_with_probability(speed_modifier: f64, probability: f32) -> Self {
        let mut g = Self::new(speed_modifier);
        g.kind = StrollKind::WaterAvoiding { probability };
        g
    }

    /// `RandomSwimmingGoal(PathfinderMob, double, int)`.
    #[must_use]
    pub fn swimming(speed_modifier: f64, interval: i32) -> Self {
        let mut g = Self::with_interval(speed_modifier, interval);
        g.kind = StrollKind::Swimming;
        g
    }

    /// `WaterAvoidingRandomFlyingGoal(PathfinderMob, double)`: the
    /// water-avoiding base constructor with the flying `getPosition`.
    #[must_use]
    pub fn water_avoiding_flying(speed_modifier: f64) -> Self {
        let mut g = Self::new(speed_modifier);
        g.kind = StrollKind::WaterAvoidingFlying;
        g
    }

    /// `MoveBackToVillageGoal(PathfinderMob, double, boolean checkNoActionTime)`:
    /// `super(mob, speed, 10, check)`. Its `canUse` override lives in
    /// [`crate::ai::goals::MoveBackToVillageGoal`].
    #[must_use]
    pub fn back_to_village(speed_modifier: f64, check_no_action_time: bool) -> Self {
        let mut g = Self::with_interval_and_check(speed_modifier, 10, check_no_action_time);
        g.kind = StrollKind::BackToVillage;
        g
    }

    /// `GolemRandomStrollInVillageGoal(PathfinderMob, double)`:
    /// `super(mob, speed, 240, false)`.
    #[must_use]
    pub fn golem_in_village(speed_modifier: f64) -> Self {
        let mut g = Self::with_interval_and_check(speed_modifier, 240, false);
        g.kind = StrollKind::GolemInVillage;
        g
    }

    /// `getPosition()`, by kind.
    fn get_position<M: MobHost>(&mut self, cx: &mut MobAi<M>) -> Option<Vec3> {
        match self.kind {
            StrollKind::GolemInVillage => {
                crate::ai::goals::golem_random_stroll_in_village_goal::get_position(cx)
            }
            StrollKind::Default => default_get_pos(cx, 10, 7),
            StrollKind::Swimming => get_random_swimmable_pos(cx, 10, 7),
            StrollKind::WaterAvoidingFlying => {
                let view = cx.mob.view_vector_at_zero();
                let p = hover_get_pos(cx, 8, 7, view.x, view.z, HALF_PI_F32, 3, 1);
                if p.is_some() {
                    return p;
                }
                air_and_water_get_pos(cx, 8, 4, -2, view.x, view.z, f64::from(HALF_PI_F32))
            }
            StrollKind::BackToVillage => {
                let bp = cx.mob.block_position();
                let section = (bp.x >> 4, bp.y >> 4, bp.z >> 4);
                let closest = cx.mob.find_section_closest_to_village(section, 2);
                if closest == section {
                    return None;
                }
                let centre = rustmc_data::BlockPos {
                    x: (closest.0 << 4) + 8,
                    y: (closest.1 << 4) + 8,
                    z: (closest.2 << 4) + 8,
                };
                default_get_pos_towards(
                    cx,
                    10,
                    7,
                    Vec3::at_bottom_center_of(centre),
                    f64::from(HALF_PI_F32),
                )
            }
            StrollKind::WaterAvoiding { probability } => {
                if cx.mob.is_in_water() {
                    let p = land_get_pos(cx, 15, 7);
                    match p {
                        None => default_get_pos(cx, 10, 7),
                        Some(p) => Some(p),
                    }
                } else if cx.mob.random().next_f32() >= probability {
                    land_get_pos(cx, 10, 7)
                } else {
                    default_get_pos(cx, 10, 7)
                }
            }
        }
    }

    /// `trigger()`: `forceTrigger = true`.
    pub fn trigger(&mut self) {
        self.force_trigger = true;
    }

    /// `setInterval(int)`.
    pub fn set_interval(&mut self, interval: i32) {
        self.interval = interval;
    }

    /// The wanted position, for tests.
    #[must_use]
    pub fn wanted(&self) -> Vec3 {
        Vec3::new(self.wanted_x, self.wanted_y, self.wanted_z)
    }
}

impl<M: MobHost> Goal<MobAi<M>> for RandomStrollGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        if cx.mob.has_controlling_passenger() {
            return false;
        }
        if !self.force_trigger {
            if self.check_no_action_time && cx.no_action_time() >= 100 {
                return false;
            }
            if cx
                .mob
                .random()
                .next_i32_bound(reduced_tick_delay(self.interval))
                != 0
            {
                return false;
            }
        }
        let Some(pos) = self.get_position(cx) else {
            return false;
        };
        self.wanted_x = pos.x;
        self.wanted_y = pos.y;
        self.wanted_z = pos.z;
        self.force_trigger = false;
        true
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        !cx.navigation.is_done() && !cx.mob.has_controlling_passenger()
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

    fn stop(&mut self, cx: &mut MobAi<M>) {
        cx.navigation.stop();
    }

    fn flags(&self) -> FlagSet {
        FlagSet::of(Flag::Move)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{GridLevel, TestMob};
    use rustmc_worldgen::rng::LegacyRandomSource;

    fn make_ai(seed: i64) -> MobAi<TestMob> {
        let mut g = GridLevel::new();
        g.floor(-20, -20, 20, 20, 63);
        MobAi::new(TestMob::new(g, 0.5, 64.0, 0.5, seed))
    }

    /// `DefaultRandomPos.getPos(mob, h, v)` replayed by hand on the flat
    /// floor: ten attempts of three `nextInt` draws each, and only a
    /// candidate with `dy == 0` is stable (stone under it) and malus-free;
    /// scores are all 0.0 and the comparison is strict, so the first
    /// accepted candidate is the answer.
    fn replay_default_get_pos(r: &mut LegacyRandomSource, h: i32, v: i32) -> Option<Vec3> {
        let mut best = None;
        for _ in 0..10 {
            let dx = r.next_i32_bound(2 * h + 1) - h;
            let dy = r.next_i32_bound(2 * v + 1) - v;
            let dz = r.next_i32_bound(2 * h + 1) - h;
            if dy == 0 && best.is_none() {
                best = Some(Vec3::new(f64::from(dx) + 0.5, 64.0, f64::from(dz) + 0.5));
            }
        }
        best
    }

    /// The gate draw is `nextInt(60)` for the default interval; a forced
    /// goal skips it, and a mob idle 100 ticks refuses before drawing.
    #[test]
    fn can_use_gates_on_next_int_60_unless_forced() {
        let mut ai = make_ai(5);
        let mut g = RandomStrollGoal::new(1.0);
        let mut r = LegacyRandomSource::new(5);
        let mut asks = 0;
        let mut passed = 0;
        while passed < 3 {
            let draw = r.next_i32_bound(60);
            let got = g.can_use(&mut ai);
            asks += 1;
            if draw == 0 {
                let want = replay_default_get_pos(&mut r, 10, 7);
                assert_eq!(got, want.is_some(), "ask {asks}: gate passed");
                if let Some(w) = want {
                    assert_eq!(g.wanted(), w, "ask {asks}");
                }
                passed += 1;
            } else {
                assert!(!got, "ask {asks}: gate refused");
            }
            assert!(asks < 5000);
        }
        // Idle mob: refused with no draw at all.
        ai.set_no_action_time(100);
        let mut before = *ai.mob.random();
        assert!(!g.can_use(&mut ai));
        assert_eq!(before.next_i32(), ai.mob.random().next_i32());
        // Forced: no gate draw, straight to the position draws.
        ai.set_no_action_time(0);
        g.trigger();
        let mut r = *ai.mob.random();
        let want = replay_default_get_pos(&mut r, 10, 7);
        assert_eq!(g.can_use(&mut ai), want.is_some());
        assert_eq!(
            ai.mob.random().next_i64(),
            r.next_i64(),
            "forced: exactly the position draws"
        );
    }

    /// Start hands the wanted point to the navigation; stop clears it.
    #[test]
    fn start_moves_and_stop_stops() {
        for seed in 0..40 {
            let mut ai = make_ai(seed);
            let mut g = RandomStrollGoal::new(1.0);
            g.trigger();
            if !g.can_use(&mut ai) {
                continue;
            }
            g.start(&mut ai);
            assert!(
                !ai.navigation.is_done(),
                "seed {seed}: a path to {:?}",
                g.wanted()
            );
            assert!(g.can_continue_to_use(&mut ai));
            g.stop(&mut ai);
            assert!(ai.navigation.is_done());
            return;
        }
        panic!("no seed in 0..40 produced a stroll target on flat floor");
    }

    /// Water-avoiding: out of water, one float is drawn before the land
    /// position; in water, none, and the land search is 15 x 7.
    #[test]
    fn water_avoiding_draws_its_float_only_out_of_water() {
        let mut ai = make_ai(21);
        let mut g = RandomStrollGoal::water_avoiding(1.0);
        g.trigger();
        let mut r = LegacyRandomSource::new(21);
        let _f = r.next_f32();
        let want = replay_default_get_pos(&mut r, 10, 7);
        assert_eq!(g.can_use(&mut ai), want.is_some());
        if let Some(w) = want {
            assert_eq!(g.wanted(), w);
        }
        assert_eq!(ai.mob.random().next_i64(), r.next_i64());

        let mut ai2 = make_ai(21);
        ai2.mob.in_water = true;
        let mut g2 = RandomStrollGoal::water_avoiding(1.0);
        g2.trigger();
        let mut r2 = LegacyRandomSource::new(21);
        let want = replay_default_get_pos(&mut r2, 15, 7);
        let got = g2.can_use(&mut ai2);
        if let Some(w) = want {
            assert!(got);
            assert_eq!(g2.wanted(), w);
            assert_eq!(ai2.mob.random().next_i64(), r2.next_i64());
        }
    }
}
