//! `net.minecraft.world.entity.ai.goal.FollowPlayerRiddenEntityGoal` and
//! its `FollowEntityGoal` enum (`GO_TO_ENTITY, GO_IN_ENTITY_DIRECTION`),
//! from the 26.2 jar. No flags; interruptable. Used by Dolphin (x2, for
//! boats and player-ridden entities).
//!
//! ```text
//! canUse()      if following != null && following.hasMovedHorizontallyRecently() return true
//!               for e in level.getEntitiesOfClass(entityTypeToFollow, bb.inflate(5.0)):
//!                   if e.getControllingPassenger() is a Player p && p.hasMovedHorizontallyRecently() return true
//!               return false
//! canContinueToUse()  following != null && following.isPassenger() && following.hasMovedHorizontallyRecently()
//! start()       for e in the same list: if its controlling passenger is a Player: following = it; break
//!               timeToRecalcPath = 0; currentGoal = GO_TO_ENTITY
//! stop()        following = null
//! tick()        speed = currentGoal == GO_IN_ENTITY_DIRECTION ? 0.01f : 0.015f
//!               mob.moveRelative(speed, (xxa, yya, zza)); mob.move(SELF, getDeltaMovement())
//!               if --timeToRecalcPath > 0 return
//!               timeToRecalcPath = adjustedTickDelay(10)
//!               GO_TO_ENTITY:  p = following.blockPosition().relative(following.getDirection().getOpposite()).offset(0, -1, 0)
//!                              navigation.moveTo(p.x, p.y, p.z, 1.0)
//!                              if mob.distanceTo(following) < 4.0f: timeToRecalcPath = 0; currentGoal = GO_IN_ENTITY_DIRECTION
//!               GO_IN_ENTITY_DIRECTION: dir = following.getMotionDirection(); p = following.blockPosition().relative(dir, 10)
//!                              navigation.moveTo(p.x, p.y - 1, p.z, 1.0)
//!                              if mob.distanceTo(following) > 12.0f: timeToRecalcPath = 0; currentGoal = GO_TO_ENTITY
//! ```
//!
//! `Entity.distanceTo(Entity)` is float: `Mth.sqrt((float)(dx) * (float)(dx) + ...)`
//! with each difference narrowed to float first.

use crate::ai::direction::Direction;
use crate::ai::goal::{adjusted_tick_delay, FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::host::{EntityClass, EntityFacts, MobHost};
use crate::mth::sqrt_f32;
use crate::phys::{offset, Vec3};

/// `FollowPlayerRiddenEntityGoal$FollowEntityGoal`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FollowEntityGoal {
    GoToEntity,
    GoInEntityDirection,
}

/// `FollowPlayerRiddenEntityGoal`.
#[derive(Debug, Clone)]
pub struct FollowPlayerRiddenEntityGoal {
    time_to_recalc_path: i32,
    entity_type_to_follow: EntityClass,
    following: Option<i32>,
    current_goal: FollowEntityGoal,
}

impl FollowPlayerRiddenEntityGoal {
    /// `FollowPlayerRiddenEntityGoal(PathfinderMob, Class)`.
    #[must_use]
    pub fn new(entity_type_to_follow: EntityClass) -> Self {
        FollowPlayerRiddenEntityGoal {
            time_to_recalc_path: 0,
            entity_type_to_follow,
            following: None,
            current_goal: FollowEntityGoal::GoToEntity,
        }
    }

    /// `Entity.distanceTo(Entity)`, the float one.
    fn distance_to<M: MobHost>(mob: &M, f: &EntityFacts) -> f32 {
        let dx = (mob.x() - f.x) as f32;
        let dy = (mob.y() - f.y) as f32;
        let dz = (mob.z() - f.z) as f32;
        sqrt_f32(dx * dx + dy * dy + dz * dz)
    }

    fn candidates<M: MobHost>(&self, cx: &MobAi<M>) -> Vec<i32> {
        let area = cx.mob.bounding_box().inflate(5.0, 5.0, 5.0);
        cx.mob
            .entities_of_class_in(self.entity_type_to_follow, &area)
    }

    /// `following` and `currentGoal`, for tests.
    #[must_use]
    pub fn state(&self) -> (Option<i32>, FollowEntityGoal) {
        (self.following, self.current_goal)
    }
}

impl<M: MobHost> Goal<MobAi<M>> for FollowPlayerRiddenEntityGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        if let Some(p) = self.following {
            if cx.mob.player_has_moved_horizontally_recently(p) {
                return true;
            }
        }
        for e in self.candidates(cx) {
            if let Some(p) = cx.mob.controlling_passenger_player(e) {
                if cx.mob.player_has_moved_horizontally_recently(p) {
                    return true;
                }
            }
        }
        false
    }

    fn is_interruptable(&self) -> bool {
        true
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        let Some(p) = self.following else {
            return false;
        };
        cx.mob.entity(p).is_some_and(|f| f.vehicle.is_some())
            && cx.mob.player_has_moved_horizontally_recently(p)
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        for e in self.candidates(cx) {
            if let Some(p) = cx.mob.controlling_passenger_player(e) {
                self.following = Some(p);
                break;
            }
        }
        self.time_to_recalc_path = 0;
        self.current_goal = FollowEntityGoal::GoToEntity;
    }

    fn stop(&mut self, _cx: &mut MobAi<M>) {
        self.following = None;
    }

    fn tick(&mut self, cx: &mut MobAi<M>) {
        let speed = if self.current_goal == FollowEntityGoal::GoInEntityDirection {
            0.01
        } else {
            0.015
        };
        let input = Vec3::new(
            f64::from(cx.mob.xxa()),
            f64::from(cx.mob.yya()),
            f64::from(cx.mob.zza()),
        );
        cx.mob.move_relative(speed, input);
        cx.mob.move_by_delta_movement();
        self.time_to_recalc_path -= 1;
        if self.time_to_recalc_path > 0 {
            return;
        }
        self.time_to_recalc_path = adjusted_tick_delay(false, 10);
        let Some(f) = self.following.and_then(|p| cx.mob.entity(p)) else {
            return;
        };
        match self.current_goal {
            FollowEntityGoal::GoToEntity => {
                let dir = Direction::from_y_rot(f64::from(f.y_rot)).opposite();
                let p = offset(dir.relative(f.block_position(), 1), 0, -1, 0);
                cx.navigation.move_to(
                    &mut cx.mob,
                    f64::from(p.x),
                    f64::from(p.y),
                    f64::from(p.z),
                    1.0,
                );
                if Self::distance_to(&cx.mob, &f) < 4.0 {
                    self.time_to_recalc_path = 0;
                    self.current_goal = FollowEntityGoal::GoInEntityDirection;
                }
            }
            FollowEntityGoal::GoInEntityDirection => {
                let dir = Direction::from_y_rot(f64::from(f.y_rot));
                let p = dir.relative(f.block_position(), 10);
                cx.navigation.move_to(
                    &mut cx.mob,
                    f64::from(p.x),
                    f64::from(p.y - 1),
                    f64::from(p.z),
                    1.0,
                );
                if Self::distance_to(&cx.mob, &f) > 12.0 {
                    self.time_to_recalc_path = 0;
                    self.current_goal = FollowEntityGoal::GoToEntity;
                }
            }
        }
    }

    fn flags(&self) -> FlagSet {
        FlagSet::NONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{GridLevel, TestMob};

    /// A boat 3 blocks off with a moving rider: the dolphin follows,
    /// heads behind the boat, and being within 4 switches to running
    /// ahead of it.
    #[test]
    fn follows_a_moving_rider_then_runs_ahead() {
        let mut g = GridLevel::new();
        g.floor(-40, -40, 40, 40, 63);
        let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, 1);
        mob.others
            .push((EntityClass::Living, EntityFacts::living(4, 3.5, 64.0, 0.5)));
        let mut rider = EntityFacts::player(9, 3.5, 64.0, 0.5);
        rider.vehicle = Some(4);
        rider.y_rot = 0.0;
        mob.others.push((EntityClass::Player, rider));
        mob.riders.insert(4, 9);
        let mut ai = MobAi::new(mob);
        let mut goal = FollowPlayerRiddenEntityGoal::new(EntityClass::Living);
        assert!(!goal.can_use(&mut ai), "rider still");
        ai.mob.moved_recently.push(9);
        assert!(goal.can_use(&mut ai));
        goal.start(&mut ai);
        assert_eq!(goal.state(), (Some(9), FollowEntityGoal::GoToEntity));
        assert!(goal.can_continue_to_use(&mut ai));
        goal.tick(&mut ai);
        assert_eq!(
            goal.state().1,
            FollowEntityGoal::GoInEntityDirection,
            "within 4"
        );
        assert_eq!(ai.mob.moves, 1);
        goal.tick(&mut ai);
        assert!(ai.navigation.is_in_progress());
        goal.stop(&mut ai);
        assert_eq!(goal.state().0, None);
    }
}
