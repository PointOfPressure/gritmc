//! `net.minecraft.world.entity.ai.behavior.VillageBoundRandomStroll`, from
//! the 26.2 jar. A `BehaviorBuilder` one-shot over
//! `group(absent(WALK_TARGET))`. `MAX_XZ_DIST = 10`, `MAX_Y_DIST = 7`.
//!
//! ```text
//! lambda$create$2 (0..104):
//!   pos = mob.blockPosition()
//!   if level.isVillage(pos): v = LandRandomPos.getPos(mob, xz, y)
//!   else:
//!     section = SectionPos.of(pos); closest = BehaviorUtils.findSectionClosestToVillage(level, section, 2)
//!     if closest != section: v = DefaultRandomPos.getPosTowards(mob, xz, y, Vec3.atBottomCenterOf(closest.center()), PI/2)
//!     else: v = LandRandomPos.getPos(mob, xz, y)
//!   walk.setOrErase(Optional.ofNullable(v).map(p -> WalkTarget(p, speed, 0)))
//!   return true
//! ```
//!
//! `closest != section` is a reference comparison (`if_acmpeq`) on the
//! `SectionPos` `findSectionClosestToVillage` returns: the same object
//! comes back only when no section within the radius is a village, so
//! value equality is the port.

use crate::ai::goals::move_towards_restriction_goal::HALF_PI_F32_AS_F64;
use crate::ai::mob_ai::MobAi;
use crate::ai::random_pos::{default_get_pos_towards, land_get_pos};
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::behaviors::go_to_closest_village::section_of;
use crate::brain::memory::{MemoryModuleType, MemoryValue, WalkTarget};
use crate::brain::BrainState;
use crate::host::MobHost;
use crate::phys::Vec3;
use rustmc_data::BlockPos;

/// `MAX_XZ_DIST`.
pub const MAX_XZ_DIST: i32 = 10;
/// `MAX_Y_DIST`.
pub const MAX_Y_DIST: i32 = 7;

/// `SectionPos.center()`: `(x << 4) + 8` on each axis.
#[must_use]
pub fn section_center(s: (i32, i32, i32)) -> BlockPos {
    BlockPos {
        x: (s.0 << 4) + 8,
        y: (s.1 << 4) + 8,
        z: (s.2 << 4) + 8,
    }
}

/// `VillageBoundRandomStroll`'s trigger.
#[derive(Debug, Clone, Copy)]
pub struct VillageBoundRandomStroll {
    pub speed_modifier: f32,
    pub max_xz_distance: i32,
    pub max_y_distance: i32,
}

impl VillageBoundRandomStroll {
    /// `create(float)`.
    #[must_use]
    pub fn create(speed_modifier: f32) -> OneShot<Self> {
        Self::create_with(speed_modifier, MAX_XZ_DIST, MAX_Y_DIST)
    }

    /// `create(float, int, int)`.
    #[must_use]
    pub fn create_with(
        speed_modifier: f32,
        max_xz_distance: i32,
        max_y_distance: i32,
    ) -> OneShot<Self> {
        OneShot::new(VillageBoundRandomStroll {
            speed_modifier,
            max_xz_distance,
            max_y_distance,
        })
    }
}

impl<M: MobHost> Trigger<M> for VillageBoundRandomStroll {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        if brain.memory_internal(MemoryModuleType::WalkTarget) != Some(None) {
            return false;
        }
        let pos = cx.mob.block_position();
        let v = if cx.mob.is_village(pos) {
            land_get_pos(cx, self.max_xz_distance, self.max_y_distance)
        } else {
            let section = section_of(pos);
            let closest = cx.mob.find_section_closest_to_village(section, 2);
            if closest != section {
                default_get_pos_towards(
                    cx,
                    self.max_xz_distance,
                    self.max_y_distance,
                    Vec3::at_bottom_center_of(section_center(closest)),
                    HALF_PI_F32_AS_F64,
                )
            } else {
                land_get_pos(cx, self.max_xz_distance, self.max_y_distance)
            }
        };
        brain.set_memory(
            MemoryModuleType::WalkTarget,
            v.map(|p| MemoryValue::WalkTarget(WalkTarget::at_vec(p, self.speed_modifier, 0))),
        );
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![MemoryModuleType::WalkTarget]
    }

    fn name(&self) -> &'static str {
        "VillageBoundRandomStroll"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};
    use rustmc_worldgen::rng::RandomSource;

    fn pair(seed: i64) -> (MobAi<TestMob>, MobAi<TestMob>) {
        let mut level = GridLevel::new();
        level.floor(-40, -40, 40, 40, 0);
        let a = MobAi::new(TestMob::new(level.clone(), 0.5, 1.0, 0.5, seed));
        let b = MobAi::new(TestMob::new(level, 0.5, 1.0, 0.5, seed));
        (a, b)
    }

    #[test]
    fn inside_a_village_it_strolls_and_outside_it_heads_for_the_nearest_village_section() {
        let (mut cx, mut twin) = pair(8);
        cx.mob.village_sections = vec![(0, 0, 0)];
        let mut brain = Brain::<TestMob>::new(&[T::WalkTarget], vec![]).state;
        let mut b = VillageBoundRandomStroll::create(0.5);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        let v = land_get_pos(&mut twin, 10, 7);
        assert_eq!(
            brain
                .get_memory(T::WalkTarget)
                .map(|m| m.as_walk_target().cloned()),
            v.map(|p| Some(WalkTarget::at_vec(p, 0.5, 0)))
        );
        // Outside, with a village section two away: walk towards its centre.
        let (mut cx, mut twin) = pair(9);
        cx.mob.village_sections = vec![(2, 0, 0)];
        brain.erase_memory(T::WalkTarget);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        let v = default_get_pos_towards(
            &mut twin,
            10,
            7,
            Vec3::at_bottom_center_of(BlockPos { x: 40, y: 8, z: 8 }),
            HALF_PI_F32_AS_F64,
        );
        assert_eq!(
            brain
                .get_memory(T::WalkTarget)
                .map(|m| m.as_walk_target().cloned()),
            v.map(|p| Some(WalkTarget::at_vec(p, 0.5, 0)))
        );
        assert_eq!(cx.mob.random.next_f32(), twin.mob.random.next_f32());
    }
}
