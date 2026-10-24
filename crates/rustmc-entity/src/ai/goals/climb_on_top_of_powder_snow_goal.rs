//! `net.minecraft.world.entity.ai.goal.ClimbOnTopOfPowderSnowGoal`, from
//! the 26.2 jar. Flag `JUMP`. Used by Fox, Rabbit, Endermite, Silverfish.
//!
//! # `canUse()`, offsets 0..95
//!
//! ```text
//! inSnow = mob.wasInPowderSnow || mob.isInPowderSnow                (0..25)
//! if !(inSnow && mob.is(EntityTypeTags.POWDER_SNOW_WALKABLE_MOBS)) return false
//! pos = mob.blockPosition().above()                                  (45..55)
//! state = level.getBlockState(pos)
//! return state.is(Blocks.POWDER_SNOW) || state.getCollisionShape(level, pos) == Shapes.empty()
//! ```
//!
//! `requiresUpdateEveryTick()` is `iconst_1`; `tick()` is
//! `mob.getJumpControl().jump()`.

use crate::ai::goal::{Flag, FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::host::{MobHost, PathLevel};
use crate::phys::above;

/// `ClimbOnTopOfPowderSnowGoal`.
#[derive(Debug, Clone, Default)]
pub struct ClimbOnTopOfPowderSnowGoal;

impl ClimbOnTopOfPowderSnowGoal {
    /// `ClimbOnTopOfPowderSnowGoal(Mob, Level)`.
    #[must_use]
    pub fn new() -> Self {
        ClimbOnTopOfPowderSnowGoal
    }
}

impl<M: MobHost> Goal<MobAi<M>> for ClimbOnTopOfPowderSnowGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        let in_snow = cx.mob.was_in_powder_snow() || cx.mob.is_in_powder_snow();
        if !(in_snow && cx.mob.is_powder_snow_walkable()) {
            return false;
        }
        let pos = above(cx.mob.block_position());
        let level = cx.mob.level();
        level.block_facts(pos).is_powder_snow || level.collision_shape_max_y(pos).is_none()
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn tick(&mut self, cx: &mut MobAi<M>) {
        cx.jump_control.jump();
    }

    fn flags(&self) -> FlagSet {
        FlagSet::of(Flag::Jump)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::BlockFacts;
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn only_a_walkable_kind_in_snow_with_room_above_climbs() {
        let mut g = GridLevel::new();
        g.floor(-4, -4, 4, 4, 63);
        let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, 1);
        let mut goal = ClimbOnTopOfPowderSnowGoal::new();
        let mut ai = MobAi::new(mob.clone());
        assert!(!goal.can_use(&mut ai), "not in snow");
        mob.in_powder_snow = true;
        let mut ai = MobAi::new(mob.clone());
        assert!(!goal.can_use(&mut ai), "not a walkable kind");
        mob.powder_snow_walkable = true;
        let mut ai = MobAi::new(mob.clone());
        assert!(goal.can_use(&mut ai), "air above");
        let mut powder = BlockFacts::NONE;
        powder.is_powder_snow = true;
        powder.is_solid = true;
        mob.level.set(0, 65, 0, powder);
        let mut ai = MobAi::new(mob.clone());
        assert!(goal.can_use(&mut ai), "powder snow above");
        let mut stone = BlockFacts::NONE;
        stone.is_solid = true;
        mob.level.set(0, 65, 0, stone);
        mob.in_powder_snow = false;
        mob.was_in_powder_snow = true;
        let mut ai = MobAi::new(mob);
        assert!(!goal.can_use(&mut ai), "a solid roof");
        assert!(Goal::<MobAi<TestMob>>::requires_update_every_tick(&goal));
        goal.tick(&mut ai);
        assert!(ai.jump_control.wants_jump());
    }
}
