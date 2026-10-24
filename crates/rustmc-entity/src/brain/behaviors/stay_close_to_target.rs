//! `net.minecraft.world.entity.ai.behavior.StayCloseToTarget`, from the
//! 26.2 jar. A `BehaviorBuilder` one-shot over
//! `group(registered(LOOK_TARGET), registered(WALK_TARGET))`.
//!
//! ```text
//! create(targetPositionGetter, canTravelTo, closeEnough, tooFar, speed)
//! trigger (lambda$create$2, 0..101):
//!   o = targetPositionGetter.apply(mob)
//!   if o.isEmpty() || !canTravelTo.test(mob): return false
//!   tracker = o.get()
//!   if mob.position().closerThan(tracker.currentPosition(), (double) tooFar): return false
//!   lookTarget.set(tracker)
//!   walkTarget.set(new WalkTarget(tracker, speed, closeEnough))
//!   return true
//! ```
//!
//! **The distance test is inverted from the name**: the behaviour refuses
//! when the mob is *already* within `tooFar`, and only walks when it is
//! further out. `closerThan` on `Vec3`/`Position` is a strict `<` on the
//! squared distance, and the `int` is widened at the call (57: `i2d`).
//!
//! **The same `PositionTracker` object is put in both memories**, so the
//! look target and the walk target's tracker are the identical value
//! rather than two constructions.
//!
//! `targetPositionGetter` and `canTravelTo` are caller-supplied
//! (`AllayAi` passes `Allay::getLikedPlayer`-shaped ones), so they are a
//! named host call rather than a port.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::memory::{MemoryModuleType, MemoryValue, PositionTracker, WalkTarget};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `StayCloseToTarget`'s trigger.
#[derive(Debug, Clone, Copy)]
pub struct StayCloseToTarget {
    /// Which caller-supplied `targetPositionGetter`/`canTravelTo` pair
    /// this instance carries, by name.
    pub target: &'static str,
    /// `closeEnough`.
    pub close_enough: i32,
    /// `tooFar`.
    pub too_far: i32,
    /// `speedModifier`.
    pub speed_modifier: f32,
}

impl StayCloseToTarget {
    /// `create(Function, Predicate, int, int, float)`.
    #[must_use]
    pub fn create(
        target: &'static str,
        close_enough: i32,
        too_far: i32,
        speed_modifier: f32,
    ) -> OneShot<Self> {
        OneShot::new(StayCloseToTarget {
            target,
            close_enough,
            too_far,
            speed_modifier,
        })
    }
}

impl<M: MobHost> Trigger<M> for StayCloseToTarget {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        let Some(tracker) = cx.mob.stay_close_target(self.target) else {
            return false;
        };
        if !cx.mob.stay_close_can_travel_to(self.target) {
            return false;
        }
        let p = current_position(&tracker, cx);
        let d = f64::from(self.too_far);
        let dx = p.x - cx.mob.x();
        let dy = p.y - cx.mob.y();
        let dz = p.z - cx.mob.z();
        if dx * dx + dy * dy + dz * dz < d * d {
            return false;
        }
        brain.set_memory_value(
            MemoryModuleType::LookTarget,
            MemoryValue::PositionTracker(tracker.clone()),
        );
        brain.set_memory_value(
            MemoryModuleType::WalkTarget,
            MemoryValue::WalkTarget(WalkTarget::new(
                tracker.clone(),
                self.speed_modifier,
                self.close_enough,
            )),
        );
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![MemoryModuleType::LookTarget, MemoryModuleType::WalkTarget]
    }

    fn name(&self) -> &'static str {
        "StayCloseToTarget"
    }
}

/// `PositionTracker.currentPosition()`.
fn current_position<M: MobHost>(t: &PositionTracker, cx: &MobAi<M>) -> crate::phys::Vec3 {
    match t {
        PositionTracker::Block { center, .. } => *center,
        PositionTracker::Entity {
            id,
            track_eye_height,
            ..
        } => cx.mob.entity(*id).map_or(
            crate::phys::Vec3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            |f| crate::phys::Vec3 {
                x: f.x,
                y: if *track_eye_height { f.eye_y } else { f.y },
                z: f.z,
            },
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};
    use rustmc_data::BlockPos;

    #[test]
    fn it_walks_only_when_already_further_than_too_far() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut brain = Brain::<TestMob>::new(&[T::LookTarget, T::WalkTarget], vec![]).state;
        let mut b = StayCloseToTarget::create("liked_player", 2, 4, 0.6);
        assert!(!b.try_start(&mut brain, &mut cx, 0), "no target");
        // 3 blocks away, tooFar is 4: refused, because it is close enough.
        let near = BlockPos { x: 3, y: 1, z: 0 };
        cx.mob.stay_close_targets = vec![("liked_player", PositionTracker::block(near))];
        cx.mob.stay_close_travelable = vec!["liked_player"];
        assert!(!b.try_start(&mut brain, &mut cx, 0), "3.0 < 4.0");
        // 9 blocks away: it walks.
        let far = BlockPos { x: 9, y: 1, z: 0 };
        let tracker = PositionTracker::block(far);
        cx.mob.stay_close_targets = vec![("liked_player", tracker.clone())];
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(
            brain.get_memory(T::LookTarget),
            Some(&MemoryValue::PositionTracker(tracker.clone()))
        );
        assert_eq!(
            brain.get_memory(T::WalkTarget).unwrap().as_walk_target(),
            Some(&WalkTarget::new(tracker.clone(), 0.6, 2)),
            "the SAME tracker object goes in both memories"
        );
        // canTravelTo alone can refuse it.
        brain.erase_memory(T::WalkTarget);
        cx.mob.stay_close_travelable.clear();
        assert!(!b.try_start(&mut brain, &mut cx, 0));
    }
}
