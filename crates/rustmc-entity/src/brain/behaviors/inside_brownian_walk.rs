//! `net.minecraft.world.entity.ai.behavior.InsideBrownianWalk`, from the
//! 26.2 jar. A `BehaviorBuilder` one-shot over `group(absent(WALK_TARGET))`.
//!
//! ```text
//! lambda$create$2 (0..124):
//!   if level.canSeeSky(mob.blockPosition()): return false
//!   pos = mob.blockPosition()
//!   list = BlockPos.betweenClosedStream(pos.offset(-1,-1,-1), pos.offset(1,1,1)).map(immutable).toMutableList()
//!   Collections.shuffle(list)                                       <- java.util.Random, UNSEEDED
//!   list.stream().filter(p -> !level.canSeeSky(p))
//!                .filter(p -> level.loadedAndEntityCanStandOn(p, mob))
//!                .filter(p -> level.noCollision(mob))               <- the candidate is ignored: the mob's own box
//!                .findFirst().ifPresent(p -> walk.set(WalkTarget(p, speed, 0)))
//!   return true
//! ```
//!
//! `Collections.shuffle(List)` draws from `java.util.Random`'s shared
//! unseeded instance: **the order is unverifiable by construction.** The
//! shuffle itself (`for i = size; i > 1; i--: swap(i - 1, nextInt(i))`)
//! is reproduced on a `LegacyRandomSource` the caller seeds, so the walk
//! is deterministic here and differs from vanilla only in that seed. No
//! draw on the mob's or the level's random.

use crate::ai::goals::try_find_water_goal::between_closed;
use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::memory::{MemoryModuleType, MemoryValue, WalkTarget};
use crate::brain::BrainState;
use crate::host::{MobHost, PathLevel};
use rustmc_worldgen::rng::{LegacyRandomSource, RandomSource};

/// `Util.shuffle(List, RandomSource)` / `Collections.shuffle`: `for i =
/// size; i > 1; i--: swap(i - 1, random.nextInt(i))`.
pub fn java_shuffle<T>(list: &mut [T], random: &mut LegacyRandomSource) {
    let mut i = list.len();
    while i > 1 {
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let j = random.next_i32_bound(i as i32);
        #[allow(clippy::cast_sign_loss)]
        list.swap(i - 1, j as usize);
        i -= 1;
    }
}

/// `InsideBrownianWalk`'s trigger.
#[derive(Debug, Clone)]
pub struct InsideBrownianWalk {
    pub speed_modifier: f32,
    /// Stands in for `Collections.shuffle`'s unseeded `java.util.Random`.
    pub shuffle_random: LegacyRandomSource,
}

impl InsideBrownianWalk {
    /// `create(float)`, with the seed vanilla does not let anyone choose.
    #[must_use]
    pub fn create(speed_modifier: f32, shuffle_seed: i64) -> OneShot<Self> {
        OneShot::new(InsideBrownianWalk {
            speed_modifier,
            shuffle_random: LegacyRandomSource::new(shuffle_seed),
        })
    }
}

impl<M: MobHost> Trigger<M> for InsideBrownianWalk {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        if brain.memory_internal(MemoryModuleType::WalkTarget) != Some(None) {
            return false;
        }
        let pos = cx.mob.block_position();
        if cx.mob.level().can_see_sky(pos) {
            return false;
        }
        let mut list = between_closed(
            pos.x - 1,
            pos.y - 1,
            pos.z - 1,
            pos.x + 1,
            pos.y + 1,
            pos.z + 1,
        );
        java_shuffle(&mut list, &mut self.shuffle_random);
        let me = cx.mob.id();
        let bb = cx.mob.bounding_box();
        let found = list.into_iter().find(|p| {
            !cx.mob.level().can_see_sky(*p)
                && cx.mob.loaded_and_entity_can_stand_on(*p)
                && cx.mob.level().no_collision(me, &bb)
        });
        if let Some(p) = found {
            brain.set_memory_value(
                MemoryModuleType::WalkTarget,
                MemoryValue::WalkTarget(WalkTarget::at_block(p, self.speed_modifier, 0)),
            );
        }
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![MemoryModuleType::WalkTarget]
    }

    fn name(&self) -> &'static str {
        "InsideBrownianWalk"
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
    fn shuffle_swaps_from_the_top_with_next_int_of_the_index() {
        let mut v = vec![0, 1, 2, 3, 4];
        let mut r = LegacyRandomSource::new(3);
        java_shuffle(&mut v, &mut r);
        let mut twin = LegacyRandomSource::new(3);
        let mut w = vec![0, 1, 2, 3, 4];
        for i in (2..=5).rev() {
            let j = twin.next_i32_bound(i) as usize;
            w.swap(i as usize - 1, j);
        }
        assert_eq!(v, w);
    }

    #[test]
    fn picks_the_first_shuffled_neighbour_that_is_roofed_and_standable() {
        let mut level = GridLevel::new();
        level.floor(-4, -4, 4, 4, 0);
        let mut cx = MobAi::new(TestMob::new(level, 0.5, 1.0, 0.5, 1));
        let mut brain = Brain::<TestMob>::new(&[T::WalkTarget], vec![]).state;
        let mut b = InsideBrownianWalk::create(0.4, 77);
        assert!(
            !b.try_start(&mut brain, &mut cx, 0),
            "open sky: refused, no draw"
        );
        cx.mob.level.floor(-4, -4, 4, 4, 4);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(!brain.has_memory_value(T::WalkTarget), "nothing standable");
        cx.mob.standable = vec![
            BlockPos { x: 1, y: 1, z: 1 },
            BlockPos { x: -1, y: 0, z: 0 },
        ];
        assert!(b.try_start(&mut brain, &mut cx, 0));
        // Two draws happened: one fresh 27-list shuffle per accepted call.
        let mut twin = LegacyRandomSource::new(77);
        let mut l = between_closed(-1, 0, -1, 1, 2, 1);
        java_shuffle(&mut l, &mut twin);
        let mut l = between_closed(-1, 0, -1, 1, 2, 1);
        java_shuffle(&mut l, &mut twin);
        let expect = l
            .into_iter()
            .find(|p| cx.mob.standable.contains(p))
            .unwrap();
        assert_eq!(
            brain.get_memory(T::WalkTarget).unwrap().as_walk_target(),
            Some(&WalkTarget::at_block(expect, 0.4, 0))
        );
    }
}
