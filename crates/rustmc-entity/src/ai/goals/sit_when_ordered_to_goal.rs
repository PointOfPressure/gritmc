//! `net.minecraft.world.entity.ai.goal.SitWhenOrderedToGoal`, from the 26.2
//! jar. Flags `JUMP, MOVE`. Used by Cat, Parrot, Wolf.
//!
//! # `canUse()`, offsets 0..101
//!
//! ```text
//! ordered = mob.isOrderedToSit()                                  (0..7)
//! if !ordered && !mob.isTame() return false                       (8..23)
//! if mob.isInWater() return false                                 (24..35)
//! if !mob.onGround() return false                                 (36..47)
//! owner = mob.getOwner()                                          (48..55)
//! if owner == null || owner.level() != mob.level() return true    (56..75)
//! if mob.distanceToSqr(owner) < 144.0 && owner.getLastHurtByMob() != null return false
//! return ordered                                                  (100..101)
//! ```
//!
//! `canContinueToUse()` is `mob.isOrderedToSit()`; `start()` is
//! `navigation.stop(); setInSittingPose(true)`; `stop()` is
//! `setInSittingPose(false)`.

use crate::ai::goal::{Flag, FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::host::MobHost;

/// `SitWhenOrderedToGoal`.
#[derive(Debug, Clone, Default)]
pub struct SitWhenOrderedToGoal;

impl SitWhenOrderedToGoal {
    /// `SitWhenOrderedToGoal(TamableAnimal)`.
    #[must_use]
    pub fn new() -> Self {
        SitWhenOrderedToGoal
    }
}

impl<M: MobHost> Goal<MobAi<M>> for SitWhenOrderedToGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        let ordered = cx.mob.is_ordered_to_sit();
        if !ordered && !cx.mob.is_tame() {
            return false;
        }
        if cx.mob.is_in_water() {
            return false;
        }
        if !cx.mob.on_ground() {
            return false;
        }
        let Some(owner) = cx.mob.owner() else {
            return true;
        };
        if cx.mob.owner_in_other_level(owner) {
            return true;
        }
        let close = cx
            .mob
            .distance_to_sqr_entity(owner)
            .is_some_and(|d| d < 144.0);
        if close && cx.mob.entity_last_hurt_by_mob(owner).is_some() {
            return false;
        }
        ordered
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        cx.mob.is_ordered_to_sit()
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        cx.navigation.stop();
        cx.mob.set_in_sitting_pose(true);
    }

    fn stop(&mut self, cx: &mut MobAi<M>) {
        cx.mob.set_in_sitting_pose(false);
    }

    fn flags(&self) -> FlagSet {
        FlagSet::of2(Flag::Jump, Flag::Move)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    fn tame_mob() -> TestMob {
        let mut g = GridLevel::new();
        g.floor(-4, -4, 4, 4, 63);
        let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, 1);
        mob.on_ground = true;
        mob.tame = true;
        mob.owner = Some(9);
        mob.others
            .push((EntityClass::Player, EntityFacts::player(9, 4.5, 64.0, 0.5)));
        mob
    }

    /// A tame pet not ordered to sit can still `canUse` (it answers
    /// `ordered`, false) — but the gate order matters: an owner under
    /// attack within 12 blocks refuses before `ordered` is consulted, and
    /// an owner in another level accepts regardless.
    #[test]
    fn sit_order_gates_in_bytecode_order() {
        let mut goal = SitWhenOrderedToGoal::new();
        let mut ai = MobAi::new(tame_mob());
        assert!(!goal.can_use(&mut ai), "tame, not ordered: answers ordered");
        ai.mob.ordered_to_sit = true;
        assert!(goal.can_use(&mut ai));
        ai.mob.last_hurt_by.insert(9, 3);
        assert!(!goal.can_use(&mut ai), "owner under attack, close");
        ai.mob.owner_in_other_level = true;
        assert!(goal.can_use(&mut ai), "owner elsewhere");
        ai.mob.owner_in_other_level = false;
        ai.mob.in_water = true;
        assert!(!goal.can_use(&mut ai));
        ai.mob.in_water = false;
        ai.mob.on_ground = false;
        assert!(!goal.can_use(&mut ai));
        ai.mob.on_ground = true;
        ai.mob.tame = false;
        ai.mob.ordered_to_sit = false;
        assert!(!goal.can_use(&mut ai), "wild and not ordered");
    }

    #[test]
    fn start_and_stop_toggle_the_sitting_pose() {
        let mut goal = SitWhenOrderedToGoal::new();
        let mut ai = MobAi::new(tame_mob());
        goal.start(&mut ai);
        assert!(ai.mob.in_sitting_pose);
        assert!(!goal.can_continue_to_use(&mut ai));
        ai.mob.ordered_to_sit = true;
        assert!(goal.can_continue_to_use(&mut ai));
        goal.stop(&mut ai);
        assert!(!ai.mob.in_sitting_pose);
    }
}
