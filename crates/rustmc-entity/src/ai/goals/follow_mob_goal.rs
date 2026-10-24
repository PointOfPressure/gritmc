//! `net.minecraft.world.entity.ai.goal.FollowMobGoal`, from the 26.2 jar.
//! Flags `MOVE, LOOK`; the predicate is `other.getClass() != mob.getClass()`.
//! Used by Parrot.
//!
//! ```text
//! canUse()        list = level.getEntitiesOfClass(Mob.class, bb.inflate(areaSize), followPredicate)
//!                 for m in list: if !m.isInvisible(): followingMob = m; return true
//!                 return false
//! canContinueToUse()  followingMob != null && !navigation.isDone()
//!                     && mob.distanceToSqr(followingMob) > stopDistance * stopDistance
//! start()         timeToRecalcPath = 0; oldWaterCost = getPathfindingMalus(WATER); setPathfindingMalus(WATER, 0)
//! stop()          followingMob = null; navigation.stop(); setPathfindingMalus(WATER, oldWaterCost)
//! tick()          if followingMob == null || mob.isLeashed() return
//!                 lookControl.setLookAt(followingMob, 10.0f, (float) getMaxHeadXRot())
//!                 if --timeToRecalcPath > 0 return; timeToRecalcPath = adjustedTickDelay(10)
//!                 d = (mob - following) squared distance
//!                 if d <= stopDistance²:
//!                     navigation.stop()
//!                     if d <= stopDistance && following's lookControl wants exactly the mob's x, y, z:
//!                         navigation.moveTo(mob.x - (following.x - mob.x), mob.y, mob.z - (following.z - mob.z), speed)
//!                 else navigation.moveTo(followingMob, speed)
//! ```
//!
//! Note the inner test compares the squared distance to `stopDistance`
//! unsquared, as the bytecode does at offset 163..171.

use crate::ai::goal::{adjusted_tick_delay, Flag, FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::host::{EntityClass, MobHost};
use crate::pathfinder::PathType;

/// `FollowMobGoal`.
#[derive(Debug, Clone)]
pub struct FollowMobGoal {
    following_mob: Option<i32>,
    speed_modifier: f64,
    time_to_recalc_path: i32,
    stop_distance: f32,
    old_water_cost: f32,
    area_size: f32,
}

impl FollowMobGoal {
    /// `FollowMobGoal(Mob, double, float stopDistance, float areaSize)`.
    #[must_use]
    pub fn new(speed_modifier: f64, stop_distance: f32, area_size: f32) -> Self {
        FollowMobGoal {
            following_mob: None,
            speed_modifier,
            time_to_recalc_path: 0,
            stop_distance,
            old_water_cost: 0.0,
            area_size,
        }
    }

    /// `followingMob`, for tests.
    #[must_use]
    pub fn following(&self) -> Option<i32> {
        self.following_mob
    }
}

impl<M: MobHost> Goal<MobAi<M>> for FollowMobGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        let a = f64::from(self.area_size);
        let area = cx.mob.bounding_box().inflate(a, a, a);
        let list: Vec<i32> = cx
            .mob
            .entities_of_class_in(EntityClass::Mob, &area)
            .into_iter()
            .filter(|id| !cx.mob.entity_is_exactly(*id, EntityClass::SameKind))
            .collect();
        for m in list {
            if !cx.mob.entity_is_invisible(m) {
                self.following_mob = Some(m);
                return true;
            }
        }
        false
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        let Some(f) = self.following_mob else {
            return false;
        };
        if cx.navigation.is_done() {
            return false;
        }
        cx.mob
            .distance_to_sqr_entity(f)
            .is_some_and(|d| d > f64::from(self.stop_distance * self.stop_distance))
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        self.time_to_recalc_path = 0;
        self.old_water_cost = cx.mob.pathfinding_malus(PathType::Water);
        cx.mob.set_pathfinding_malus(PathType::Water, 0.0);
    }

    fn stop(&mut self, cx: &mut MobAi<M>) {
        self.following_mob = None;
        cx.navigation.stop();
        cx.mob
            .set_pathfinding_malus(PathType::Water, self.old_water_cost);
    }

    fn tick(&mut self, cx: &mut MobAi<M>) {
        let Some(f) = self.following_mob else {
            return;
        };
        if cx.mob.is_leashed() {
            return;
        }
        let Some(other) = cx.mob.entity(f) else {
            return;
        };
        #[allow(clippy::cast_precision_loss)]
        let max_x = cx.mob.max_head_x_rot() as f32;
        cx.look_control
            .set_look_at_with(other.x, other.eye_y, other.z, 10.0, max_x);
        self.time_to_recalc_path -= 1;
        if self.time_to_recalc_path > 0 {
            return;
        }
        self.time_to_recalc_path = adjusted_tick_delay(false, 10);
        let dx = cx.mob.x() - other.x;
        let dy = cx.mob.y() - other.y;
        let dz = cx.mob.z() - other.z;
        let d = dx * dx + dy * dy + dz * dz;
        if d <= f64::from(self.stop_distance * self.stop_distance) {
            cx.navigation.stop();
            if d <= f64::from(self.stop_distance) {
                let looking_at_me = cx
                    .mob
                    .other_look_wanted(f)
                    .is_some_and(|w| w.x == cx.mob.x() && w.y == cx.mob.y() && w.z == cx.mob.z());
                if looking_at_me {
                    let ox = other.x - cx.mob.x();
                    let oz = other.z - cx.mob.z();
                    let (x, y, z) = (cx.mob.x() - ox, cx.mob.y(), cx.mob.z() - oz);
                    let s = self.speed_modifier;
                    cx.navigation.move_to(&mut cx.mob, x, y, z, s);
                }
            }
        } else {
            let s = self.speed_modifier;
            cx.navigation
                .move_to_entity_at(&mut cx.mob, other.block_position(), s);
        }
    }

    fn flags(&self) -> FlagSet {
        FlagSet::of2(Flag::Move, Flag::Look)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::EntityFacts;
    use crate::testing::{GridLevel, TestMob};

    /// A parrot follows the first visible mob of another kind in the
    /// box, zeroes its water malus, and steps back when the followed mob
    /// stares at it from within the stop distance.
    #[test]
    fn follows_another_kind_and_steps_back_from_a_stare() {
        let mut g = GridLevel::new();
        g.floor(-20, -20, 20, 20, 63);
        let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, 1);
        mob.others.push((
            EntityClass::SameKind,
            EntityFacts::living(2, 1.5, 64.0, 0.5),
        ));
        mob.others
            .push((EntityClass::Mob, EntityFacts::living(3, 2.5, 64.0, 0.5)));
        mob.others
            .push((EntityClass::Mob, EntityFacts::living(4, 5.5, 64.0, 0.5)));
        mob.invisible.push(3);
        let mut ai = MobAi::new(mob);
        let mut goal = FollowMobGoal::new(1.0, 3.0, 7.0);
        assert!(goal.can_use(&mut ai));
        assert_eq!(
            goal.following(),
            Some(4),
            "own kind and the invisible one skipped"
        );
        goal.start(&mut ai);
        assert_eq!(ai.mob.pathfinding_malus(PathType::Water), 0.0);
        goal.tick(&mut ai);
        assert!(
            ai.navigation.is_in_progress(),
            "5 away on a stop of 3: walks"
        );
    }

    /// Distance 3 on a stop distance of 3: `d = 9 <= 9` stops the walk,
    /// and `9 <= 3` is false, so no side-step even under a stare.
    #[test]
    fn at_the_stop_distance_it_stands() {
        let mut g = GridLevel::new();
        g.floor(-20, -20, 20, 20, 63);
        let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, 1);
        mob.others
            .push((EntityClass::Mob, EntityFacts::living(4, 3.5, 64.0, 0.5)));
        mob.other_look_wanted
            .insert(4, crate::phys::Vec3::new(0.5, 64.0, 0.5));
        let mut ai = MobAi::new(mob);
        let mut goal = FollowMobGoal::new(1.0, 3.0, 7.0);
        assert!(goal.can_use(&mut ai));
        goal.start(&mut ai);
        goal.tick(&mut ai);
        assert!(ai.navigation.is_done());
        ai.mob.others[0].1.x = 1.5;
        for _ in 0..5 {
            goal.tick(&mut ai);
        }
        assert!(
            ai.navigation.is_in_progress(),
            "1 away under a stare: steps back"
        );
        goal.stop(&mut ai);
        assert_eq!(ai.mob.pathfinding_malus(PathType::Water), 8.0);
    }
}
