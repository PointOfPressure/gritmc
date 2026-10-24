//! `net.minecraft.world.entity.ai.goal.target.OwnerHurtByTargetGoal`, from
//! the 26.2 jar. `TargetGoal(mob, false)`, flag `TARGET`. Used by Wolf.
//!
//! # `canUse()`, offsets 0..91
//!
//! ```text
//! if !tameAnimal.isTame() || tameAnimal.isOrderedToSit() return false
//! owner = tameAnimal.getOwner(); if null return false
//! ownerLastHurtBy = owner.getLastHurtByMob()
//! ts = owner.getLastHurtByMobTimestamp()
//! return ts != timestamp && canAttack(ownerLastHurtBy, TargetingConditions.DEFAULT)
//!     && tameAnimal.wantsToAttack(ownerLastHurtBy, owner)
//! ```
//!
//! `start()`: `mob.setTarget(ownerLastHurtBy); owner = getOwner(); if
//! owner != null timestamp = owner.getLastHurtByMobTimestamp(); super.start()`.
//! `TargetingConditions.DEFAULT` is `forCombat()` with no range.

use crate::ai::goal::{Flag, FlagSet, Goal};
use crate::ai::goals::target_goal::TargetGoalBase;
use crate::ai::mob_ai::MobAi;
use crate::ai::targeting::TargetingConditions;
use crate::host::MobHost;

/// `OwnerHurtByTargetGoal`.
pub struct OwnerHurtByTargetGoal<M: MobHost> {
    pub base: TargetGoalBase,
    owner_last_hurt_by: Option<i32>,
    timestamp: i32,
    default_conditions: TargetingConditions<M>,
}

impl<M: MobHost> OwnerHurtByTargetGoal<M> {
    /// `OwnerHurtByTargetGoal(TamableAnimal)`.
    #[must_use]
    pub fn new() -> Self {
        OwnerHurtByTargetGoal {
            base: TargetGoalBase::new(false, false),
            owner_last_hurt_by: None,
            timestamp: 0,
            default_conditions: TargetingConditions::for_combat(),
        }
    }
}

impl<M: MobHost> Default for OwnerHurtByTargetGoal<M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<M: MobHost> Goal<MobAi<M>> for OwnerHurtByTargetGoal<M> {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        if !cx.mob.is_tame() || cx.mob.is_ordered_to_sit() {
            return false;
        }
        let Some(owner) = cx.mob.owner() else {
            return false;
        };
        self.owner_last_hurt_by = cx.mob.entity_last_hurt_by_mob(owner);
        let ts = cx.mob.entity_last_hurt_by_mob_timestamp(owner);
        if ts == self.timestamp {
            return false;
        }
        let t = self.owner_last_hurt_by;
        if !self.base.can_attack(cx, t, &self.default_conditions) {
            return false;
        }
        t.is_some_and(|t| cx.mob.wants_to_attack(t, owner))
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        self.base.can_continue_to_use(cx)
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        cx.mob.set_target(self.owner_last_hurt_by);
        if let Some(owner) = cx.mob.owner() {
            self.timestamp = cx.mob.entity_last_hurt_by_mob_timestamp(owner);
        }
        self.base.start();
    }

    fn stop(&mut self, cx: &mut MobAi<M>) {
        self.base.stop(cx);
    }

    fn flags(&self) -> FlagSet {
        FlagSet::of(Flag::Target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn a_tame_standing_pet_avenges_each_new_hit_on_its_owner() {
        let mut mob = TestMob::new(GridLevel::new(), 0.5, 64.0, 0.5, 1);
        mob.tame = true;
        mob.owner = Some(9);
        mob.others
            .push((EntityClass::Player, EntityFacts::player(9, 2.5, 64.0, 0.5)));
        mob.others
            .push((EntityClass::Mob, EntityFacts::living(5, 4.5, 64.0, 0.5)));
        mob.last_hurt_by.insert(9, 5);
        mob.last_hurt_by_ts.insert(9, 40);
        mob.visible.push(5);
        let mut ai = MobAi::new(mob);
        let mut goal = OwnerHurtByTargetGoal::new();
        assert!(goal.can_use(&mut ai));
        goal.start(&mut ai);
        assert_eq!(ai.mob.target_raw, Some(5));
        assert!(!goal.can_use(&mut ai), "same timestamp");
        ai.mob.last_hurt_by_ts.insert(9, 41);
        assert!(goal.can_use(&mut ai));
        ai.mob.wants_to_attack_denied.push(5);
        assert!(!goal.can_use(&mut ai), "wantsToAttack refused");
        ai.mob.wants_to_attack_denied.clear();
        ai.mob.ordered_to_sit = true;
        assert!(!goal.can_use(&mut ai), "sitting");
    }
}
