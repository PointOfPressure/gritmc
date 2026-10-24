//! `net.minecraft.world.entity.ai.goal.FollowOwnerGoal`, from the 26.2 jar.
//! Flags `MOVE, LOOK`. Used by Cat, Parrot, Wolf.
//!
//! The constructor is `(TamableAnimal, double speed, float startDistance,
//! float stopDistance)` and throws unless the navigation is ground or
//! flying; this crate only has the ground one.
//!
//! # `canUse()`, offsets 0..56
//!
//! ```text
//! owner = tamable.getOwner(); if null return false
//! if tamable.unableToMoveToOwner() return false
//! if tamable.distanceToSqr(owner) < startDistance * startDistance return false
//! this.owner = owner; return true
//! ```
//!
//! # `canContinueToUse()`, offsets 0..52
//!
//! ```text
//! if navigation.isDone() return false
//! if tamable.unableToMoveToOwner() return false
//! return tamable.distanceToSqr(owner) > stopDistance * stopDistance
//! ```
//!
//! `start()`: `timeToRecalcPath = 0; oldWaterCost = getPathfindingMalus(WATER);
//! setPathfindingMalus(WATER, 0)`. `stop()`: `owner = null; navigation.stop();
//! setPathfindingMalus(WATER, oldWaterCost)`.
//!
//! # `tick()`, offsets 0..91
//!
//! ```text
//! tp = tamable.shouldTryTeleportToOwner()
//! if !tp: lookControl.setLookAt(owner, 10.0f, (float) getMaxHeadXRot())
//! if --timeToRecalcPath > 0 return
//! timeToRecalcPath = adjustedTickDelay(10)                 -- 5
//! if tp: tamable.tryToTeleportToOwner() else navigation.moveTo(owner, speedModifier)
//! ```
//!
//! The owner is the entity `canUse` stored; its position is read again
//! each time it is needed.

use crate::ai::goal::{adjusted_tick_delay, Flag, FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::ai::tamable::try_to_teleport_to_owner;
use crate::host::MobHost;
use crate::pathfinder::PathType;

/// `FollowOwnerGoal`.
#[derive(Debug, Clone)]
pub struct FollowOwnerGoal {
    owner: Option<i32>,
    speed_modifier: f64,
    time_to_recalc_path: i32,
    stop_distance: f32,
    start_distance: f32,
    old_water_cost: f32,
}

impl FollowOwnerGoal {
    /// `FollowOwnerGoal(TamableAnimal, double, float startDistance, float stopDistance)`.
    #[must_use]
    pub fn new(speed_modifier: f64, start_distance: f32, stop_distance: f32) -> Self {
        FollowOwnerGoal {
            owner: None,
            speed_modifier,
            time_to_recalc_path: 0,
            stop_distance,
            start_distance,
            old_water_cost: 0.0,
        }
    }

    /// The `owner` field.
    #[must_use]
    pub fn owner(&self) -> Option<i32> {
        self.owner
    }
}

impl<M: MobHost> Goal<MobAi<M>> for FollowOwnerGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        let Some(owner) = cx.mob.owner() else {
            return false;
        };
        if cx.mob.unable_to_move_to_owner() {
            return false;
        }
        let Some(d) = cx.mob.distance_to_sqr_entity(owner) else {
            return false;
        };
        if d < f64::from(self.start_distance * self.start_distance) {
            return false;
        }
        self.owner = Some(owner);
        true
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        if cx.navigation.is_done() {
            return false;
        }
        if cx.mob.unable_to_move_to_owner() {
            return false;
        }
        let Some(d) = self.owner.and_then(|o| cx.mob.distance_to_sqr_entity(o)) else {
            return false;
        };
        d > f64::from(self.stop_distance * self.stop_distance)
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        self.time_to_recalc_path = 0;
        self.old_water_cost = cx.mob.pathfinding_malus(PathType::Water);
        cx.mob.set_pathfinding_malus(PathType::Water, 0.0);
    }

    fn stop(&mut self, cx: &mut MobAi<M>) {
        self.owner = None;
        cx.navigation.stop();
        cx.mob
            .set_pathfinding_malus(PathType::Water, self.old_water_cost);
    }

    fn tick(&mut self, cx: &mut MobAi<M>) {
        let Some(owner) = self.owner.and_then(|o| cx.mob.entity(o)) else {
            return;
        };
        let tp = cx.mob.should_try_teleport_to_owner();
        if !tp {
            #[allow(clippy::cast_precision_loss)]
            let max_x = cx.mob.max_head_x_rot() as f32;
            cx.look_control
                .set_look_at_with(owner.x, owner.eye_y, owner.z, 10.0, max_x);
        }
        self.time_to_recalc_path -= 1;
        if self.time_to_recalc_path > 0 {
            return;
        }
        self.time_to_recalc_path = adjusted_tick_delay(false, 10);
        if tp {
            try_to_teleport_to_owner(cx);
        } else {
            let s = self.speed_modifier;
            cx.navigation
                .move_to_entity_at(&mut cx.mob, owner.block_position(), s);
        }
    }

    fn flags(&self) -> FlagSet {
        FlagSet::of2(Flag::Move, Flag::Look)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    fn pet_with_owner_at(dx: f64) -> MobAi<TestMob> {
        let mut g = GridLevel::new();
        g.floor(-40, -40, 40, 40, 63);
        let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, 3);
        mob.on_ground = true;
        mob.tame = true;
        mob.owner = Some(9);
        mob.others.push((
            EntityClass::Player,
            EntityFacts::player(9, 0.5 + dx, 64.0, 0.5),
        ));
        MobAi::new(mob)
    }

    /// Start at 10 (100 sq), stop at 2 (4 sq): an owner 5 away is too
    /// close to start; 11 away starts; sitting refuses.
    #[test]
    fn start_and_stop_distances_are_squared_floats() {
        let mut goal = FollowOwnerGoal::new(1.0, 10.0, 2.0);
        let mut ai = pet_with_owner_at(5.0);
        assert!(!goal.can_use(&mut ai));
        let mut ai = pet_with_owner_at(11.0);
        assert!(goal.can_use(&mut ai));
        assert_eq!(goal.owner(), Some(9));
        ai.mob.ordered_to_sit = true;
        assert!(!goal.can_use(&mut ai));
    }

    /// `start` zeroes the water malus and `stop` restores it; the first
    /// tick paths to the owner (5 ticks between recalculations), and an
    /// owner 12+ away is teleported to instead of walked to.
    #[test]
    fn walks_when_near_and_teleports_when_far() {
        let mut goal = FollowOwnerGoal::new(1.0, 10.0, 2.0);
        let mut ai = pet_with_owner_at(11.0);
        assert!(goal.can_use(&mut ai));
        goal.start(&mut ai);
        assert_eq!(ai.mob.pathfinding_malus(PathType::Water), 0.0);
        goal.tick(&mut ai);
        assert!(ai.navigation.is_in_progress(), "walked");
        assert_eq!(goal.time_to_recalc_path, 5);
        assert!(ai.mob.snaps.is_empty());
        goal.stop(&mut ai);
        assert_eq!(ai.mob.pathfinding_malus(PathType::Water), 8.0);
        assert!(ai.navigation.is_done());

        let mut ai = pet_with_owner_at(13.0);
        assert!(goal.can_use(&mut ai));
        goal.start(&mut ai);
        goal.tick(&mut ai);
        assert!(!ai.mob.snaps.is_empty(), "teleported");
        assert!(ai.navigation.is_done());
    }
}
