//! `net.minecraft.world.entity.ai.behavior.RandomStroll`, from the 26.2 jar.
//! A `BehaviorBuilder` one-shot over `group(absent(WALK_TARGET))`
//! (`strollFlyOrSwim`, `lambda$strollFlyOrSwim$2`, 0..45):
//!
//! ```text
//! if !canStroll.test(mob): return false
//! v = getTarget.apply(mob)                                   (nullable)
//! walk.setOrErase(Optional.ofNullable(v).map(p -> new WalkTarget(p, speed, 0)))   <- a null ERASES
//! return true
//! ```
//!
//! The four factories:
//!
//! ```text
//! stroll(speed)                = stroll(speed, true)
//! stroll(speed, mayFromWater)  target: LandRandomPos.getPos(mob, 10, 7); canStroll: mayFromWater ? true : !mob.isInWater()
//! stroll(speed, h, v)          target: LandRandomPos.getPos(mob, h, v);   canStroll: true
//! fly(speed)                   target: getTargetFlyPos(mob, 10, 7);       canStroll: true
//! swim(speed)                  target: getTargetSwimPos(mob);             canStroll: true
//! getTargetFlyPos(mob, h, v)   view = mob.getViewVector(0); AirAndWaterRandomPos.getPos(mob, h, v, -2, view.x, view.z, PI/2)
//! getTargetSwimPos(mob) (0..138), SWIM_XY_DISTANCE_TIERS = {1,1},{3,3},{5,5},{6,5},{7,7},{10,7}:
//!   best = null; cand = null
//!   for (h, v) in tiers:
//!     cand = best == null ? BehaviorUtils.getRandomSwimmablePos(mob, h, v)
//!                         : mob.position().add(mob.position().vectorTo(best).normalize().multiply(h, v, h))
//!     restricted = GoalUtils.mobRestricted(mob, h)
//!     if cand == null || level.getFluidState(BlockPos.containing(cand)).isEmpty() || GoalUtils.isRestricted(restricted, mob, cand): return best
//!     best = cand
//!   return cand
//! ```
//!
//! `WalkTarget(Vec3, float, int)` snaps to the block centre (memory.rs).
//! Draws: the position functions walk the mob's random through `random_pos`.

use crate::ai::mob_ai::MobAi;
use crate::ai::random_pos::{
    air_and_water_get_pos, get_random_swimmable_pos, is_restricted, land_get_pos, mob_restricted,
};
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::memory::{MemoryModuleType, MemoryValue, WalkTarget};
use crate::brain::BrainState;
use crate::host::{MobHost, PathLevel};
use crate::phys::{block_pos_containing_vec, Vec3};

/// `MAX_XZ_DIST`.
pub const MAX_XZ_DIST: i32 = 10;
/// `MAX_Y_DIST`.
pub const MAX_Y_DIST: i32 = 7;
/// `SWIM_XY_DISTANCE_TIERS`.
pub const SWIM_XY_DISTANCE_TIERS: [(i32, i32); 6] =
    [(1, 1), (3, 3), (5, 5), (6, 5), (7, 7), (10, 7)];
/// `(float) (Math.PI / 2)` widened to a double, as `ldc2_w 1.5707963705062866d`.
const HALF_PI: f64 = 1.570_796_370_506_286_6;

/// Which factory built the one-shot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrollKind {
    /// `stroll(speed, mayStrollFromWater)` and `stroll(speed, h, v)`.
    Land {
        h: i32,
        v: i32,
        may_stroll_from_water: bool,
    },
    /// `fly(speed)`.
    Fly,
    /// `swim(speed)`.
    Swim,
}

/// `RandomStroll`'s trigger; build one with the factories below.
#[derive(Debug, Clone, Copy)]
pub struct RandomStroll {
    pub kind: StrollKind,
    pub speed_modifier: f32,
}

impl RandomStroll {
    /// `stroll(float)` — `stroll(speed, true)`.
    #[must_use]
    pub fn stroll(speed: f32) -> OneShot<Self> {
        Self::stroll_from_water(speed, true)
    }

    /// `stroll(float, boolean)`.
    #[must_use]
    pub fn stroll_from_water(speed: f32, may_stroll_from_water: bool) -> OneShot<Self> {
        OneShot::new(RandomStroll {
            kind: StrollKind::Land {
                h: MAX_XZ_DIST,
                v: MAX_Y_DIST,
                may_stroll_from_water,
            },
            speed_modifier: speed,
        })
    }

    /// `stroll(float, int, int)`.
    #[must_use]
    pub fn stroll_within(speed: f32, h: i32, v: i32) -> OneShot<Self> {
        OneShot::new(RandomStroll {
            kind: StrollKind::Land {
                h,
                v,
                may_stroll_from_water: true,
            },
            speed_modifier: speed,
        })
    }

    /// `fly(float)`.
    #[must_use]
    pub fn fly(speed: f32) -> OneShot<Self> {
        OneShot::new(RandomStroll {
            kind: StrollKind::Fly,
            speed_modifier: speed,
        })
    }

    /// `swim(float)`.
    #[must_use]
    pub fn swim(speed: f32) -> OneShot<Self> {
        OneShot::new(RandomStroll {
            kind: StrollKind::Swim,
            speed_modifier: speed,
        })
    }

    /// `getTargetFlyPos(mob, h, v)`.
    pub fn target_fly_pos<M: MobHost>(cx: &mut MobAi<M>, h: i32, v: i32) -> Option<Vec3> {
        let view = cx.mob.view_vector_at_zero();
        air_and_water_get_pos(cx, h, v, -2, view.x, view.z, HALF_PI)
    }

    /// `getTargetSwimPos(mob)`.
    pub fn target_swim_pos<M: MobHost>(cx: &mut MobAi<M>) -> Option<Vec3> {
        let mut best: Option<Vec3> = None;
        let mut cand: Option<Vec3> = None;
        for (h, v) in SWIM_XY_DISTANCE_TIERS {
            cand = match best {
                None => get_random_swimmable_pos(cx, h, v),
                Some(b) => {
                    let p = cx.mob.position();
                    let dir = b.subtract(p).normalize();
                    Some(Vec3::new(
                        p.x + dir.x * f64::from(h),
                        p.y + dir.y * f64::from(v),
                        p.z + dir.z * f64::from(h),
                    ))
                }
            };
            let restricted = mob_restricted(&cx.mob, f64::from(h));
            let Some(c) = cand else {
                return best;
            };
            let bp = block_pos_containing_vec(c);
            let f = cx.mob.level().block_facts(bp);
            if !(f.fluid_is_water || f.fluid_is_lava) || is_restricted(restricted, &cx.mob, bp) {
                return best;
            }
            best = cand;
        }
        cand
    }
}

impl<M: MobHost> Trigger<M> for RandomStroll {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        if brain.memory_internal(MemoryModuleType::WalkTarget) != Some(None) {
            return false;
        }
        let can_stroll = match self.kind {
            StrollKind::Land {
                may_stroll_from_water,
                ..
            } => may_stroll_from_water || !cx.mob.is_in_water(),
            StrollKind::Fly | StrollKind::Swim => true,
        };
        if !can_stroll {
            return false;
        }
        let target = match self.kind {
            StrollKind::Land { h, v, .. } => land_get_pos(cx, h, v),
            StrollKind::Fly => Self::target_fly_pos(cx, MAX_XZ_DIST, MAX_Y_DIST),
            StrollKind::Swim => Self::target_swim_pos(cx),
        };
        brain.set_memory(
            MemoryModuleType::WalkTarget,
            target.map(|p| MemoryValue::WalkTarget(WalkTarget::at_vec(p, self.speed_modifier, 0))),
        );
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![MemoryModuleType::WalkTarget]
    }

    fn name(&self) -> &'static str {
        "RandomStroll"
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
    fn a_land_stroll_draws_like_land_random_pos_and_snaps_to_the_block_centre() {
        let mut level = GridLevel::new();
        level.floor(-16, -16, 16, 16, 0);
        let mut cx = MobAi::new(TestMob::new(level.clone(), 0.5, 1.0, 0.5, 21));
        let mut twin = MobAi::new(TestMob::new(level, 0.5, 1.0, 0.5, 21));
        let expected = land_get_pos(&mut twin, 10, 7);
        let mut brain = Brain::<TestMob>::new(&[T::WalkTarget], vec![]).state;
        let mut b = RandomStroll::stroll(0.3);
        assert!(
            b.try_start(&mut brain, &mut cx, 0),
            "the trigger answers true even with no position"
        );
        match expected {
            Some(p) => {
                let w = brain
                    .get_memory(T::WalkTarget)
                    .unwrap()
                    .as_walk_target()
                    .unwrap()
                    .clone();
                assert_eq!(w, WalkTarget::at_vec(p, 0.3, 0));
            }
            None => assert!(!brain.has_memory_value(T::WalkTarget), "a null erases"),
        }
    }

    #[test]
    fn stroll_refuses_from_water_unless_allowed_and_when_a_walk_target_exists() {
        let mut level = GridLevel::new();
        level.floor(-16, -16, 16, 16, 0);
        let mut cx = MobAi::new(TestMob::new(level, 0.5, 1.0, 0.5, 21));
        cx.mob.in_water = true;
        let mut brain = Brain::<TestMob>::new(&[T::WalkTarget], vec![]).state;
        assert!(!RandomStroll::stroll_from_water(0.3, false).try_start(&mut brain, &mut cx, 0));
        assert!(RandomStroll::stroll_from_water(0.3, true).try_start(&mut brain, &mut cx, 0));
        brain.set_memory_value(
            T::WalkTarget,
            MemoryValue::WalkTarget(WalkTarget::at_vec(Vec3::new(1.0, 1.0, 1.0), 1.0, 0)),
        );
        assert!(!RandomStroll::stroll(0.3).try_start(&mut brain, &mut cx, 0));
    }
}
