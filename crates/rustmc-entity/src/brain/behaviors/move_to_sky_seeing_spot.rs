//! `net.minecraft.world.entity.ai.behavior.MoveToSkySeeingSpot`, from the
//! 26.2 jar. A `BehaviorBuilder` one-shot over `group(absent(WALK_TARGET))`.
//!
//! ```text
//! lambda$create$2 (0..36):
//!   if level.canSeeSky(mob.blockPosition()): return false
//!   Optional.ofNullable(getOutdoorPosition(level, mob)).ifPresent(v -> walk.set(WalkTarget(v, speed, 0)))
//!   return true
//! getOutdoorPosition (0..81):                                          <- MOB random
//!   for i in 0..10:
//!     p = pos.offset(random.nextInt(20) - 10, random.nextInt(6) - 3, random.nextInt(20) - 10)
//!     if hasNoBlocksAbove(level, mob, p): return Vec3.atBottomCenterOf(p)
//!   return null
//! hasNoBlocksAbove (0..33): level.canSeeSky(p) && level.getHeightmapPos(MOTION_BLOCKING, p).getY() <= mob.getY()
//! ```
//!
//! Three `nextInt` draws per attempt, x then y then z, up to ten attempts.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::memory::{MemoryModuleType, MemoryValue, WalkTarget};
use crate::brain::BrainState;
use crate::host::{MobHost, PathLevel};
use crate::phys::{offset, Vec3};
use rustmc_data::BlockPos;
use rustmc_worldgen::rng::RandomSource;

/// `hasNoBlocksAbove(level, mob, pos)`.
pub fn has_no_blocks_above<M: MobHost>(cx: &MobAi<M>, pos: BlockPos) -> bool {
    cx.mob.level().can_see_sky(pos)
        && f64::from(cx.mob.heightmap_pos_motion_blocking(pos).y) <= cx.mob.y()
}

/// `getOutdoorPosition(level, mob)`.
pub fn get_outdoor_position<M: MobHost>(cx: &mut MobAi<M>) -> Option<Vec3> {
    let pos = cx.mob.block_position();
    for _ in 0..10 {
        let r = cx.mob.random();
        let dx = r.next_i32_bound(20) - 10;
        let dy = r.next_i32_bound(6) - 3;
        let dz = r.next_i32_bound(20) - 10;
        let p = offset(pos, dx, dy, dz);
        if has_no_blocks_above(cx, p) {
            return Some(Vec3::at_bottom_center_of(p));
        }
    }
    None
}

/// `MoveToSkySeeingSpot`'s trigger.
#[derive(Debug, Clone, Copy)]
pub struct MoveToSkySeeingSpot {
    pub speed_modifier: f32,
}

impl MoveToSkySeeingSpot {
    /// `create(float)`.
    #[must_use]
    pub fn create(speed_modifier: f32) -> OneShot<Self> {
        OneShot::new(MoveToSkySeeingSpot { speed_modifier })
    }
}

impl<M: MobHost> Trigger<M> for MoveToSkySeeingSpot {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        if brain.memory_internal(MemoryModuleType::WalkTarget) != Some(None) {
            return false;
        }
        if cx.mob.level().can_see_sky(cx.mob.block_position()) {
            return false;
        }
        if let Some(v) = get_outdoor_position(cx) {
            brain.set_memory_value(
                MemoryModuleType::WalkTarget,
                MemoryValue::WalkTarget(WalkTarget::at_vec(v, self.speed_modifier, 0)),
            );
        }
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![MemoryModuleType::WalkTarget]
    }

    fn name(&self) -> &'static str {
        "MoveToSkySeeingSpot"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};
    use rustmc_worldgen::rng::LegacyRandomSource;

    #[test]
    fn under_a_roof_it_draws_three_per_attempt_until_a_sky_spot_at_or_below_it() {
        let mut level = GridLevel::new();
        level.floor(-3, -3, 3, 3, 4);
        let mut cx = MobAi::new(TestMob::new(level, 0.5, 1.0, 0.5, 13));
        let mut brain = Brain::<TestMob>::new(&[T::WalkTarget], vec![]).state;
        let mut b = MoveToSkySeeingSpot::create(0.7);
        cx.mob.level.floor(-3, -3, 3, 3, 4);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        let mut twin = LegacyRandomSource::new(13);
        let mut expect = None;
        for _ in 0..10 {
            let dx = twin.next_i32_bound(20) - 10;
            let dy = twin.next_i32_bound(6) - 3;
            let dz = twin.next_i32_bound(20) - 10;
            let p = BlockPos {
                x: dx,
                y: 1 + dy,
                z: dz,
            };
            let roofed = (-3..=3).contains(&p.x) && (-3..=3).contains(&p.z) && p.y < 4;
            if !roofed {
                expect = Some(Vec3::at_bottom_center_of(p));
                break;
            }
        }
        assert_eq!(
            brain
                .get_memory(T::WalkTarget)
                .map(|m| m.as_walk_target().cloned()),
            expect.map(|v| Some(WalkTarget::at_vec(v, 0.7, 0)))
        );
        assert_eq!(cx.mob.random.next_f32(), twin.next_f32());
        // Out in the open the trigger refuses without a draw.
        cx.mob.x = 20.5;
        assert!(!b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(cx.mob.random.next_f32(), twin.next_f32());
    }
}
