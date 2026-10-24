//! `net.minecraft.world.entity.ai.goal.StrollThroughVillageGoal`, from the
//! 26.2 jar. Flag `MOVE`; `DISTANCE_THRESHOLD = 10`; `interval =
//! reducedTickDelay(arg)`. Used by AbstractIllager.
//!
//! ```text
//! canUse()   !hasControllingPassenger() && !level.isBrightOutside()
//!            && random.nextInt(interval) == 0                                (offset 38)
//!            && level.isCloseToVillage(blockPosition(), 6)
//!            wantedPos = containing(LandRandomPos.getPos(mob, 15, 7, p -> -sectionsToVillage(SectionPos.of(p))))
//!            return wantedPos != null
//! canContinueToUse()  wantedPos != null && !navigation.isDone() && navigation.getTargetPos().equals(wantedPos)
//! tick()     if wantedPos == null return
//!            if navigation.isDone() && !wantedPos.closerToCenterThan(position, 10.0):
//!                c = atBottomCenterOf(wantedPos); p = position
//!                c = (p - c).scale(0.4) + (c)
//!                t = (c - p).normalize().scale(10.0) + (p)
//!                b = level.getHeightmapPos(MOTION_BLOCKING_NO_LEAVES, containing(t))
//!                if !navigation.moveTo(b.x, b.y, b.z, 1.0): moveRandomly()
//! moveRandomly()  b = getHeightmapPos(MOTION_BLOCKING_NO_LEAVES, blockPosition().offset(-8 + nextInt(16), 0, -8 + nextInt(16)))
//!            navigation.moveTo(b.x, b.y, b.z, 1.0)
//! ```

use rustmc_data::BlockPos;
use rustmc_worldgen::rng::RandomSource;

use crate::ai::goal::{reduced_tick_delay, Flag, FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::ai::random_pos::land_get_pos_scored;
use crate::host::{MobHost, PathLevel};
use crate::phys::{block_pos_containing_vec, closer_to_center_than, offset, Vec3};

/// `DISTANCE_THRESHOLD`.
pub const DISTANCE_THRESHOLD: i32 = 10;

/// `StrollThroughVillageGoal`.
#[derive(Debug, Clone)]
pub struct StrollThroughVillageGoal {
    interval: i32,
    wanted_pos: Option<BlockPos>,
}

impl StrollThroughVillageGoal {
    /// `StrollThroughVillageGoal(PathfinderMob, int interval)`.
    #[must_use]
    pub fn new(interval: i32) -> Self {
        StrollThroughVillageGoal {
            interval: reduced_tick_delay(interval),
            wanted_pos: None,
        }
    }

    /// `moveRandomly()`.
    fn move_randomly<M: MobHost>(cx: &mut MobAi<M>) {
        let dx = -8 + cx.mob.random().next_i32_bound(16);
        let dz = -8 + cx.mob.random().next_i32_bound(16);
        let p = offset(cx.mob.block_position(), dx, 0, dz);
        let b = cx.mob.heightmap_pos_motion_blocking_no_leaves(p);
        cx.navigation.move_to(
            &mut cx.mob,
            f64::from(b.x),
            f64::from(b.y),
            f64::from(b.z),
            1.0,
        );
    }

    /// `wantedPos`, for tests.
    #[must_use]
    pub fn wanted_pos(&self) -> Option<BlockPos> {
        self.wanted_pos
    }
}

impl<M: MobHost> Goal<MobAi<M>> for StrollThroughVillageGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        if cx.mob.has_controlling_passenger() {
            return false;
        }
        if cx.mob.level().is_bright_outside() {
            return false;
        }
        if cx.mob.random().next_i32_bound(self.interval) != 0 {
            return false;
        }
        let pos = cx.mob.block_position();
        if !cx.mob.is_close_to_village(pos, 6) {
            return false;
        }
        let v = land_get_pos_scored(cx, 15, 7, |ai, p| {
            -f64::from(ai.mob.sections_to_village((p.x >> 4, p.y >> 4, p.z >> 4)))
        });
        self.wanted_pos = v.map(block_pos_containing_vec);
        self.wanted_pos.is_some()
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        let Some(w) = self.wanted_pos else {
            return false;
        };
        !cx.navigation.is_done() && cx.navigation.target_pos() == Some(w)
    }

    fn tick(&mut self, cx: &mut MobAi<M>) {
        let Some(w) = self.wanted_pos else {
            return;
        };
        if cx.navigation.is_done() && !closer_to_center_than(w, cx.mob.position(), 10.0) {
            let c = Vec3::at_bottom_center_of(w);
            let p = cx.mob.position();
            let c = p.subtract(c).scale(0.4) + (c);
            let t = c.subtract(p).normalize().scale(10.0) + (p);
            let b = cx
                .mob
                .heightmap_pos_motion_blocking_no_leaves(block_pos_containing_vec(t));
            let moved = cx.navigation.move_to(
                &mut cx.mob,
                f64::from(b.x),
                f64::from(b.y),
                f64::from(b.z),
                1.0,
            );
            if !moved {
                Self::move_randomly(cx);
            }
        }
    }

    fn flags(&self) -> FlagSet {
        FlagSet::of(Flag::Move)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{GridLevel, TestMob};

    /// At night near a village the scored land search prefers the
    /// candidate whose section is nearest the village; the walk starts
    /// on the tick after the interval draw hits.
    #[test]
    fn strolls_toward_the_village_at_night() {
        let mut g = GridLevel::new();
        g.floor(-60, -60, 60, 60, 63);
        let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, 4);
        mob.close_to_village = true;
        // Sections to the east (x >= 16) are the village; everything else is far.
        for x in -4..=4 {
            for z in -4..=4 {
                mob.sections_to_village
                    .insert((x, 4, z), if x >= 1 { 0 } else { 5 - x });
            }
        }
        let mut ai = MobAi::new(mob);
        let mut goal = StrollThroughVillageGoal::new(100);
        let mut hit = false;
        for _ in 0..400 {
            if goal.can_use(&mut ai) {
                hit = true;
                break;
            }
        }
        assert!(hit);
        let w = goal.wanted_pos().unwrap();
        assert!(w.x > 0, "{w:?}: eastward, toward the village");
        goal.tick(&mut ai);
        assert!(ai.navigation.is_in_progress());
        // The tick walks to a point ten blocks along the line, so the
        // navigation's target is never `wantedPos` and `canContinueToUse`
        // is false after the first tick: this goal re-arms itself only
        // through its interval draw.
        assert_ne!(ai.navigation.target_pos(), Some(w));
        assert!(!goal.can_continue_to_use(&mut ai));
    }
}
