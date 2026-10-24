//! `net.minecraft.world.entity.ai.goal.BreathAirGoal`, from the 26.2 jar.
//! Flags `MOVE, LOOK`; not interruptable. Used by Turtle (the dolphin's
//! copy is a brain behaviour).
//!
//! `canUse()`: `mob.getAirSupply() < 140`; `canContinueToUse()` is the
//! same. `start()`: `navigation.stop(); findAirPosition()`.
//!
//! `findAirPosition()`, offsets 0..151: over
//! `BlockPos.neighborColumn(blockX, blockY, blockZ, blockY + 8)` — for
//! each of the offsets `(0,0,0), NORTH, EAST, SOUTH, WEST`, every y from
//! `blockY` up to `blockY + 8` (`BlockPos$5`: `index % height` is the
//! step, `index / height` the offset) — the first `givesAir` block; when
//! none, `BlockPos.containing(x, y + 8, z)`. Then `navigation.moveTo(p.x,
//! p.y + 1, p.z, 1.0)`.
//!
//! `givesAir(level, pos)`: `(!fluid.isEmpty() || state.is(BUBBLE_COLUMN))
//! && state.isPathfindable(LAND)`; a bubble column's fluid is water, so
//! the fluid test alone decides here.
//!
//! `tick()`: `findAirPosition(); mob.moveRelative(0.02f, (xxa, yya, zza));
//! mob.move(SELF, getDeltaMovement())`.

use rustmc_data::BlockPos;

use crate::ai::goal::{Flag, FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::host::{MobHost, PathLevel};
use crate::phys::{block_pos_containing, Vec3};

/// `BreathAirGoal`.
#[derive(Debug, Clone, Default)]
pub struct BreathAirGoal;

impl BreathAirGoal {
    /// `BreathAirGoal(PathfinderMob)`.
    #[must_use]
    pub fn new() -> Self {
        BreathAirGoal
    }

    /// `BlockPos.neighborColumn(x, y, z, yEnd)` in the jar's order.
    pub fn neighbor_column(x: i32, y: i32, z: i32, y_end: i32) -> Vec<BlockPos> {
        let y_dir = if y_end > y { 1 } else { -1 };
        let height = (y_end - y).abs() + 1;
        let steps: [(i32, i32); 5] = [(0, 0), (0, -1), (1, 0), (0, 1), (-1, 0)];
        (0..5 * height)
            .map(|i| {
                let k = i % height;
                let s = steps[(i / height) as usize];
                BlockPos {
                    x: x + s.0,
                    y: y + k * y_dir,
                    z: z + s.1,
                }
            })
            .collect()
    }

    /// `givesAir(LevelReader, BlockPos)`.
    fn gives_air<M: MobHost>(mob: &M, pos: BlockPos) -> bool {
        let f = mob.level().block_facts(pos);
        (f.fluid_is_water || f.fluid_is_lava) && f.is_pathfindable_land
    }

    /// `findAirPosition()`.
    fn find_air_position<M: MobHost>(cx: &mut MobAi<M>) {
        let bp = cx.mob.block_position();
        let found = Self::neighbor_column(bp.x, bp.y, bp.z, bp.y + 8)
            .into_iter()
            .find(|p| Self::gives_air(&cx.mob, *p));
        let p =
            found.unwrap_or_else(|| block_pos_containing(cx.mob.x(), cx.mob.y() + 8.0, cx.mob.z()));
        cx.navigation.move_to(
            &mut cx.mob,
            f64::from(p.x),
            f64::from(p.y + 1),
            f64::from(p.z),
            1.0,
        );
    }
}

impl<M: MobHost> Goal<MobAi<M>> for BreathAirGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        cx.mob.air_supply() < 140
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        self.can_use(cx)
    }

    fn is_interruptable(&self) -> bool {
        false
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        cx.navigation.stop();
        Self::find_air_position(cx);
    }

    fn tick(&mut self, cx: &mut MobAi<M>) {
        Self::find_air_position(cx);
        let input = Vec3::new(
            f64::from(cx.mob.xxa()),
            f64::from(cx.mob.yya()),
            f64::from(cx.mob.zza()),
        );
        cx.mob.move_relative(0.02, input);
        cx.mob.move_by_delta_movement();
    }

    fn flags(&self) -> FlagSet {
        FlagSet::of2(Flag::Move, Flag::Look)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neighbor_column_walks_each_offset_bottom_to_top() {
        let c = BreathAirGoal::neighbor_column(0, 10, 0, 11);
        assert_eq!(
            c,
            vec![
                BlockPos { x: 0, y: 10, z: 0 },
                BlockPos { x: 0, y: 11, z: 0 },
                BlockPos { x: 0, y: 10, z: -1 },
                BlockPos { x: 0, y: 11, z: -1 },
                BlockPos { x: 1, y: 10, z: 0 },
                BlockPos { x: 1, y: 11, z: 0 },
                BlockPos { x: 0, y: 10, z: 1 },
                BlockPos { x: 0, y: 11, z: 1 },
                BlockPos { x: -1, y: 10, z: 0 },
                BlockPos { x: -1, y: 11, z: 0 },
            ]
        );
    }
}
