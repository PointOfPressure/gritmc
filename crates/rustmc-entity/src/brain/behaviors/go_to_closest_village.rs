//! `net.minecraft.world.entity.ai.behavior.GoToClosestVillage`, from the
//! 26.2 jar. A `BehaviorBuilder` one-shot over `group(absent(WALK_TARGET))`
//! for `Villager`.
//!
//! ```text
//! lambda$create$2 (0..140):
//!   if level.isVillage(mob.blockPosition()): return false
//!   here = poiManager.sectionsToVillage(SectionPos.of(mob.blockPosition()))
//!   best = null
//!   for i in 0..5:
//!     v = LandRandomPos.getPos(mob, 15, 7, p -> -sectionsToVillage(SectionPos.of(p)))   <- MOB random
//!     if v == null: continue
//!     s = sectionsToVillage(SectionPos.of(BlockPos.containing(v)))
//!     if s < here: best = v; break
//!     if s == here: best = v
//!   if best != null: walk.set(WalkTarget(best, speed, closeEnoughDist))
//!   return true
//! ```
//!
//! Up to five scored `LandRandomPos.getPos` calls on the mob's random,
//! each ten attempts; an equal distance keeps the latest candidate, a
//! closer one stops the loop.

use crate::ai::mob_ai::MobAi;
use crate::ai::random_pos::land_get_pos_scored;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::memory::{MemoryModuleType, MemoryValue, WalkTarget};
use crate::brain::BrainState;
use crate::host::MobHost;
use crate::phys::{block_pos_containing_vec, Vec3};
use rustmc_data::BlockPos;

/// `SectionPos.of(BlockPos)`.
#[must_use]
pub fn section_of(p: BlockPos) -> (i32, i32, i32) {
    (p.x >> 4, p.y >> 4, p.z >> 4)
}

/// `GoToClosestVillage`'s trigger.
#[derive(Debug, Clone, Copy)]
pub struct GoToClosestVillage {
    pub speed_modifier: f32,
    pub close_enough_distance: i32,
}

impl GoToClosestVillage {
    /// `create(float, int)`.
    #[must_use]
    pub fn create(speed_modifier: f32, close_enough_distance: i32) -> OneShot<Self> {
        OneShot::new(GoToClosestVillage {
            speed_modifier,
            close_enough_distance,
        })
    }
}

impl<M: MobHost> Trigger<M> for GoToClosestVillage {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        if brain.memory_internal(MemoryModuleType::WalkTarget) != Some(None) {
            return false;
        }
        let here = cx.mob.block_position();
        if cx.mob.is_village(here) {
            return false;
        }
        let current = cx.mob.sections_to_village(section_of(here));
        let mut best: Option<Vec3> = None;
        for _ in 0..5 {
            let v = land_get_pos_scored(cx, 15, 7, |ai, p| {
                -f64::from(ai.mob.sections_to_village(section_of(p)))
            });
            let Some(v) = v else {
                continue;
            };
            let s = cx
                .mob
                .sections_to_village(section_of(block_pos_containing_vec(v)));
            if s < current {
                best = Some(v);
                break;
            }
            if s == current {
                best = Some(v);
            }
        }
        if let Some(v) = best {
            brain.set_memory_value(
                MemoryModuleType::WalkTarget,
                MemoryValue::WalkTarget(WalkTarget::at_vec(
                    v,
                    self.speed_modifier,
                    self.close_enough_distance,
                )),
            );
        }
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![MemoryModuleType::WalkTarget]
    }

    fn name(&self) -> &'static str {
        "GoToClosestVillage"
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

    fn setup() -> MobAi<TestMob> {
        let mut level = GridLevel::new();
        level.floor(-40, -40, 40, 40, 0);
        let mut cx = MobAi::new(TestMob::new(level, 0.5, 1.0, 0.5, 21));
        // Sections at x >= 16 are one closer to the village than here.
        for sx in -3..3 {
            for sz in -3..3 {
                cx.mob
                    .sections_to_village
                    .insert((sx, 0, sz), if sx >= 1 { 2 } else { 3 });
            }
        }
        cx
    }

    #[test]
    fn walks_towards_a_closer_section_replaying_the_scored_random_pos() {
        let mut cx = setup();
        let mut twin = MobAi::new(TestMob::new(cx.mob.level.clone(), 0.5, 1.0, 0.5, 21));
        twin.mob.sections_to_village = cx.mob.sections_to_village.clone();
        let mut brain = Brain::<TestMob>::new(&[T::WalkTarget], vec![]).state;
        let mut b = GoToClosestVillage::create(0.6, 2);
        cx.mob.village_sections = vec![(0, 0, 0)];
        assert!(!b.try_start(&mut brain, &mut cx, 0), "already in a village");
        cx.mob.village_sections.clear();
        assert!(b.try_start(&mut brain, &mut cx, 0));
        let mut best = None;
        for _ in 0..5 {
            let v = land_get_pos_scored(&mut twin, 15, 7, |ai, p| {
                -f64::from(ai.mob.sections_to_village(section_of(p)))
            });
            let Some(v) = v else { continue };
            let s = twin
                .mob
                .sections_to_village(section_of(block_pos_containing_vec(v)));
            if s < 3 {
                best = Some(v);
                break;
            }
            if s == 3 {
                best = Some(v);
            }
        }
        assert_eq!(
            brain
                .get_memory(T::WalkTarget)
                .map(|m| m.as_walk_target().cloned()),
            best.map(|v| Some(WalkTarget::at_vec(v, 0.6, 2)))
        );
        assert_eq!(
            cx.mob.random.next_f32(),
            twin.mob.random.next_f32(),
            "same draws"
        );
    }
}
