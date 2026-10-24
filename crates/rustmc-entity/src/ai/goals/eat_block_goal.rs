//! `net.minecraft.world.entity.ai.goal.EatBlockGoal`, from the 26.2 jar.
//! Flags `MOVE, LOOK, JUMP`; `EAT_ANIMATION_TICKS = 40`;
//! `IS_EDIBLE = is(BlockTags.EDIBLE_FOR_SHEEP)`. Used by Sheep.
//!
//! `canUse()`, offsets 0..91: `random.nextInt(adjustedTickDelay(isBaby ? 50 : 1000))
//! != 0` refuses (halved: 25 / 500, offset 29); then the block at the
//! mob's position is edible, or the block below is grass.
//! `start()`: `eatAnimationTick = adjustedTickDelay(40)` (20),
//! `level.broadcastEntityEvent(mob, 10)`, `navigation.stop()`. `stop()`:
//! `eatAnimationTick = 0`. `canContinueToUse()`: `eatAnimationTick > 0`.
//! `tick()`: `eatAnimationTick = max(0, eatAnimationTick - 1)`; only at
//! `adjustedTickDelay(4)` (2): an edible block at the position is
//! destroyed under `MOB_GRIEFING` and `ate()` is called; else grass
//! below becomes dirt (event 2001 then `setBlock(DIRT, 2)`) under the
//! rule and `ate()` is called.

use rustmc_worldgen::rng::RandomSource;

use crate::ai::goal::{adjusted_tick_delay, Flag, FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::host::{MobHost, PathLevel};
use crate::phys::below;

/// `EAT_ANIMATION_TICKS`.
pub const EAT_ANIMATION_TICKS: i32 = 40;

/// `EatBlockGoal`.
#[derive(Debug, Clone, Default)]
pub struct EatBlockGoal {
    eat_animation_tick: i32,
}

impl EatBlockGoal {
    /// `EatBlockGoal(Mob)`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `getEatAnimationTick()`.
    #[must_use]
    pub fn eat_animation_tick(&self) -> i32 {
        self.eat_animation_tick
    }
}

impl<M: MobHost> Goal<MobAi<M>> for EatBlockGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        let bound = adjusted_tick_delay(false, if cx.mob.is_baby() { 50 } else { 1000 });
        if cx.mob.random().next_i32_bound(bound) != 0 {
            return false;
        }
        let pos = cx.mob.block_position();
        let level = cx.mob.level();
        if level.block_facts(pos).is_edible_for_sheep {
            return true;
        }
        level.block_facts(below(pos)).is_grass_block
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        self.eat_animation_tick = adjusted_tick_delay(false, EAT_ANIMATION_TICKS);
        cx.mob.broadcast_entity_event(10);
        cx.navigation.stop();
    }

    fn stop(&mut self, _cx: &mut MobAi<M>) {
        self.eat_animation_tick = 0;
    }

    fn can_continue_to_use(&mut self, _cx: &mut MobAi<M>) -> bool {
        self.eat_animation_tick > 0
    }

    fn tick(&mut self, cx: &mut MobAi<M>) {
        self.eat_animation_tick = (self.eat_animation_tick - 1).max(0);
        if self.eat_animation_tick != adjusted_tick_delay(false, 4) {
            return;
        }
        let pos = cx.mob.block_position();
        if cx.mob.level().block_facts(pos).is_edible_for_sheep {
            if cx.mob.mob_griefing() {
                cx.mob.destroy_block(pos);
            }
            cx.mob.ate();
        } else {
            let b = below(pos);
            if cx.mob.level().block_facts(b).is_grass_block {
                if cx.mob.mob_griefing() {
                    cx.mob.turn_grass_to_dirt(b);
                }
                cx.mob.ate();
            }
        }
    }

    fn flags(&self) -> FlagSet {
        FlagSet::of3(Flag::Move, Flag::Look, Flag::Jump)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmc_worldgen::rng::LegacyRandomSource;

    use crate::host::BlockFacts;
    use crate::testing::{GridLevel, TestMob};

    /// A sheep on grass: `nextInt(500)` per tick until a zero; then 20
    /// animation ticks, the bite landing when the counter reaches 2.
    #[test]
    fn grazes_on_the_zero_draw_and_bites_at_two() {
        let mut g = GridLevel::new();
        g.floor(-4, -4, 4, 4, 63);
        let mut grass = BlockFacts::NONE;
        grass.is_solid = true;
        grass.is_grass_block = true;
        g.set(0, 63, 0, grass);
        let mob = TestMob::new(g, 0.5, 64.0, 0.5, 8);
        let mut ai = MobAi::new(mob);
        let mut goal = EatBlockGoal::new();
        let mut twin = LegacyRandomSource::new(8);
        let mut misses = 0;
        while twin.next_i32_bound(500) != 0 {
            misses += 1;
        }
        for _ in 0..misses {
            assert!(!goal.can_use(&mut ai));
        }
        assert!(goal.can_use(&mut ai));
        goal.start(&mut ai);
        assert_eq!(goal.eat_animation_tick(), 20);
        for _ in 0..17 {
            goal.tick(&mut ai);
        }
        assert!(!ai.mob.calls.iter().any(|c| c == "ate"));
        goal.tick(&mut ai);
        assert_eq!(goal.eat_animation_tick(), 2);
        assert!(ai.mob.calls.iter().any(|c| c == "grassToDirt(0, 63, 0)"));
        assert_eq!(ai.mob.calls.last().map(String::as_str), Some("ate"));
        goal.tick(&mut ai);
        goal.tick(&mut ai);
        assert!(!goal.can_continue_to_use(&mut ai));
    }
}
