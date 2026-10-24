//! `net.minecraft.world.entity.ai.goal.TryFindWaterGoal`, from the 26.2
//! jar. No flags. Used by Turtle (it lives in the `PathfinderMob`'s water
//! search, so the dolphin's brain is not this).
//!
//! `canUse()`: `mob.onGround() && !level.getFluidState(mob.blockPosition()).is(FluidTags.WATER)`.
//!
//! # `start()`, offsets 0..170
//!
//! ```text
//! for pos in BlockPos.betweenClosed(floor(x - 2), floor(y - 2), floor(z - 2),
//!                                   floor(x + 2), getBlockY(), floor(z + 2)):
//!     if level.getFluidState(pos).is(WATER): found = pos; break
//! if found != null: moveControl.setWantedPosition(found.x, found.y, found.z, 1.0)
//! ```
//!
//! `betweenClosed` walks x fastest, then y, then z (`BlockPos$4`:
//! `index % width`, `index / width % height`, `index / width / height`).
//! The y ceiling is `getBlockY()`, not `floor(y + 2)`.

use rustmc_data::BlockPos;
use rustmc_worldgen::math::floor;

use crate::ai::goal::{FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::host::{MobHost, PathLevel};

/// `TryFindWaterGoal`.
#[derive(Debug, Clone, Default)]
pub struct TryFindWaterGoal;

impl TryFindWaterGoal {
    /// `TryFindWaterGoal(PathfinderMob)`.
    #[must_use]
    pub fn new() -> Self {
        TryFindWaterGoal
    }
}

/// `BlockPos.betweenClosed(x1, y1, z1, x2, y2, z2)` in the jar's order.
pub fn between_closed(x1: i32, y1: i32, z1: i32, x2: i32, y2: i32, z2: i32) -> Vec<BlockPos> {
    let (w, h, d) = (x2 - x1 + 1, y2 - y1 + 1, z2 - z1 + 1);
    let n = w * h * d;
    (0..n.max(0))
        .map(|i| BlockPos {
            x: x1 + i % w,
            y: y1 + i / w % h,
            z: z1 + i / w / h,
        })
        .collect()
}

impl<M: MobHost> Goal<MobAi<M>> for TryFindWaterGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        cx.mob.on_ground()
            && !cx
                .mob
                .level()
                .block_facts(cx.mob.block_position())
                .fluid_is_water
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        let (x, y, z) = (cx.mob.x(), cx.mob.y(), cx.mob.z());
        let found = between_closed(
            floor(x - 2.0),
            floor(y - 2.0),
            floor(z - 2.0),
            floor(x + 2.0),
            cx.mob.block_y(),
            floor(z + 2.0),
        )
        .into_iter()
        .find(|p| cx.mob.level().block_facts(*p).fluid_is_water);
        if let Some(p) = found {
            cx.move_control.set_wanted_position(
                f64::from(p.x),
                f64::from(p.y),
                f64::from(p.z),
                1.0,
            );
        }
    }

    fn flags(&self) -> FlagSet {
        FlagSet::NONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::BlockFacts;
    use crate::testing::{GridLevel, TestMob};

    /// Two waters in the 5x3x5 box: the one with the smaller z wins
    /// regardless of x, and within a z-row the smaller y, then x.
    #[test]
    fn picks_the_first_water_in_x_then_y_then_z_order() {
        assert_eq!(
            between_closed(0, 0, 0, 1, 1, 1),
            vec![
                BlockPos { x: 0, y: 0, z: 0 },
                BlockPos { x: 1, y: 0, z: 0 },
                BlockPos { x: 0, y: 1, z: 0 },
                BlockPos { x: 1, y: 1, z: 0 },
                BlockPos { x: 0, y: 0, z: 1 },
                BlockPos { x: 1, y: 0, z: 1 },
                BlockPos { x: 0, y: 1, z: 1 },
                BlockPos { x: 1, y: 1, z: 1 },
            ]
        );
        let mut g = GridLevel::new();
        g.floor(-10, -10, 10, 10, 63);
        g.set(-2, 64, 2, BlockFacts::WATER);
        g.set(2, 63, -1, BlockFacts::WATER);
        let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, 1);
        mob.on_ground = true;
        let mut ai = MobAi::new(mob);
        let mut goal = TryFindWaterGoal::new();
        assert!(goal.can_use(&mut ai));
        goal.start(&mut ai);
        assert_eq!(ai.move_control.wanted(), (2.0, 63.0, -1.0));
    }
}
