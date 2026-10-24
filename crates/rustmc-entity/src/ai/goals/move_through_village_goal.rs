//! `net.minecraft.world.entity.ai.goal.MoveThroughVillageGoal`, from the
//! 26.2 jar. Flag `MOVE`; the constructor throws without ground
//! navigation. Used through `Raider$RaiderMoveThroughVillageGoal`.
//!
//! # `canUse()`, offsets 0..409
//!
//! ```text
//! if !hasGroundPathNavigation(mob) return false
//! updateVisited()                                   -- drop the oldest past 15
//! if onlyAtNight && level.isBrightOutside() return false
//! pos = blockPosition(); if !level.isCloseToVillage(pos, 6) return false
//! v = LandRandomPos.getPos(mob, 15, 7, p -> isVillage(p)
//!         ? poi(p).map(q -> -q.distSqr(pos)).orElse(-INF) : -INF)    -- poi(p): find(VILLAGE, hasNotVisited, p, 10, IS_OCCUPIED)
//! if v == null return false
//! poiPos = poi(containing(v)) or return false
//! navigation.setCanOpenDoors(canDealWithDoors); path = navigation.createPath(poiPos, 0); setCanOpenDoors(true)
//! if path == null:
//!     w = DefaultRandomPos.getPosTowards(mob, 10, 7, atBottomCenterOf(poiPos), (float) PI / 2); if null return false
//!     setCanOpenDoors(canDealWithDoors); path = createPath(w.x, w.y, w.z, 0); setCanOpenDoors(true)
//!     if path == null return false
//! for i in 0 .. path.getNodeCount():
//!     node = path.getNode(i); door = (node.x, node.y + 1, node.z)
//!     if DoorBlock.isWoodenDoor(level, door): path = createPath(node.x, node.y, node.z, 0); break
//! return path != null
//! ```
//!
//! `canContinueToUse()`: `!navigation.isDone() && !poiPos.closerToCenterThan(position, bbWidth + distanceToPoi)`.
//! `start()`: `navigation.moveTo(path, speed)`. `stop()`: when the
//! navigation is done or the mob is within `distanceToPoi` of the poi,
//! `visited.add(poiPos)`.

use rustmc_data::BlockPos;

use crate::ai::goal::{Flag, FlagSet, Goal};
use crate::ai::goals::move_towards_restriction_goal::HALF_PI_F32_AS_F64;
use crate::ai::mob_ai::MobAi;
use crate::ai::random_pos::{
    default_get_pos_towards, has_ground_path_navigation, land_get_pos_scored,
};
use crate::host::{MobHost, PathLevel};
use crate::pathfinder::Path;
use crate::phys::{block_pos_containing_vec, closer_to_center_than, dist_sqr, Vec3};

/// `MoveThroughVillageGoal`.
pub struct MoveThroughVillageGoal {
    speed_modifier: f64,
    path: Option<Path>,
    poi_pos: Option<BlockPos>,
    only_at_night: bool,
    visited: Vec<BlockPos>,
    distance_to_poi: i32,
    can_deal_with_doors: fn() -> bool,
}

impl MoveThroughVillageGoal {
    /// `MoveThroughVillageGoal(PathfinderMob, double, boolean onlyAtNight, int distanceToPoi, BooleanSupplier)`.
    #[must_use]
    pub fn new(
        speed_modifier: f64,
        only_at_night: bool,
        distance_to_poi: i32,
        can_deal_with_doors: fn() -> bool,
    ) -> Self {
        MoveThroughVillageGoal {
            speed_modifier,
            path: None,
            poi_pos: None,
            only_at_night,
            visited: Vec::new(),
            distance_to_poi,
            can_deal_with_doors,
        }
    }

    /// `updateVisited()`.
    fn update_visited(&mut self) {
        if self.visited.len() > 15 {
            self.visited.remove(0);
        }
    }

    /// `poiPos` and the visited list, for tests.
    #[must_use]
    pub fn state(&self) -> (Option<BlockPos>, &[BlockPos]) {
        (self.poi_pos, &self.visited)
    }
}

impl<M: MobHost> Goal<MobAi<M>> for MoveThroughVillageGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        if !has_ground_path_navigation(cx) {
            return false;
        }
        self.update_visited();
        if self.only_at_night && cx.mob.level().is_bright_outside() {
            return false;
        }
        let pos = cx.mob.block_position();
        if !cx.mob.is_close_to_village(pos, 6) {
            return false;
        }
        let visited = self.visited.clone();
        let v = land_get_pos_scored(cx, 15, 7, |ai, p| {
            if !ai.mob.is_village(p) {
                return f64::NEG_INFINITY;
            }
            ai.mob
                .find_village_poi(p, 10, &visited)
                .map_or(f64::NEG_INFINITY, |q| -dist_sqr(q, pos))
        });
        let Some(v) = v else {
            return false;
        };
        let Some(poi) = cx
            .mob
            .find_village_poi(block_pos_containing_vec(v), 10, &visited)
        else {
            return false;
        };
        self.poi_pos = Some(poi);
        cx.navigation
            .set_can_open_doors((self.can_deal_with_doors)());
        self.path = cx.navigation.create_path_to(
            &mut cx.mob,
            f64::from(poi.x),
            f64::from(poi.y),
            f64::from(poi.z),
            0,
        );
        cx.navigation.set_can_open_doors(true);
        if self.path.is_none() {
            let Some(w) = default_get_pos_towards(
                cx,
                10,
                7,
                Vec3::at_bottom_center_of(poi),
                HALF_PI_F32_AS_F64,
            ) else {
                return false;
            };
            cx.navigation
                .set_can_open_doors((self.can_deal_with_doors)());
            self.path = cx.navigation.create_path_to(&mut cx.mob, w.x, w.y, w.z, 0);
            cx.navigation.set_can_open_doors(true);
            if self.path.is_none() {
                return false;
            }
        }
        if let Some(path) = &self.path {
            let mut door_node = None;
            for i in 0..path.node_count() {
                let n = path.node(i);
                let door = BlockPos {
                    x: n.x,
                    y: n.y + 1,
                    z: n.z,
                };
                if cx
                    .mob
                    .level()
                    .block_facts(door)
                    .door
                    .is_some_and(|d| d.can_open_by_hand)
                {
                    door_node = Some((n.x, n.y, n.z));
                    break;
                }
            }
            if let Some((x, y, z)) = door_node {
                self.path = cx.navigation.create_path_to(
                    &mut cx.mob,
                    f64::from(x),
                    f64::from(y),
                    f64::from(z),
                    0,
                );
            }
        }
        self.path.is_some()
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        if cx.navigation.is_done() {
            return false;
        }
        let Some(poi) = self.poi_pos else {
            return false;
        };
        #[allow(clippy::cast_precision_loss)]
        let d = f64::from(cx.mob.bb_width() + self.distance_to_poi as f32);
        !closer_to_center_than(poi, cx.mob.position(), d)
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        let path = self.path.clone();
        cx.navigation
            .move_along(&mut cx.mob, path, self.speed_modifier);
    }

    fn stop(&mut self, cx: &mut MobAi<M>) {
        if let Some(poi) = self.poi_pos {
            if cx.navigation.is_done()
                || closer_to_center_than(poi, cx.mob.position(), f64::from(self.distance_to_poi))
            {
                self.visited.push(poi);
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

    /// One occupied village poi 8 blocks east: with a seed whose ten land
    /// candidates include one inside the village within ten of the poi,
    /// the poi becomes the target, the raider paths to it, and after
    /// arriving the poi is remembered as visited.
    #[test]
    fn paths_to_a_village_poi_and_remembers_it() {
        let poi = BlockPos { x: 8, y: 64, z: 0 };
        let mut goal = MoveThroughVillageGoal::new(1.0, false, 1, || true);
        let mut hit = None;
        for seed in 1..60 {
            let mut g = GridLevel::new();
            g.floor(-60, -60, 60, 60, 63);
            let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, seed);
            mob.close_to_village = true;
            for x in -4..=4 {
                for z in -4..=4 {
                    mob.sections_to_village.insert((x, 4, z), 0);
                    mob.village_sections.push((x, 4, z));
                }
            }
            mob.village_pois.push(poi);
            let mut ai = MobAi::new(mob);
            if goal.can_use(&mut ai) {
                hit = Some(ai);
                break;
            }
        }
        let mut ai = hit.expect("some seed lands a candidate in the village near the poi");
        assert_eq!(goal.state().0, Some(poi));
        goal.start(&mut ai);
        assert!(ai.navigation.is_in_progress());
        assert!(goal.can_continue_to_use(&mut ai));
        ai.mob.x = 8.5;
        assert!(!goal.can_continue_to_use(&mut ai), "within bbWidth + 1");
        goal.stop(&mut ai);
        assert_eq!(goal.state().1, &[poi]);
        assert!(!goal.can_use(&mut ai), "the only poi is visited");
    }
}
