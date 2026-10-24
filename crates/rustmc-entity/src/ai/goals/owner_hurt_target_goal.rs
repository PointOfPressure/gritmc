//! `net.minecraft.world.entity.ai.goal.target.OwnerHurtTargetGoal`, from
//! the 26.2 jar. `TargetGoal(mob, false)`, flag `TARGET`. Used by Wolf.
//! Byte for byte the shape of `OwnerHurtByTargetGoal` with
//! `getLastHurtMob` / `getLastHurtMobTimestamp` in place of the
//! `...ByMob` pair: the pet attacks whom its owner last hit.

use crate::ai::goal::{Flag, FlagSet, Goal};
use crate::ai::goals::target_goal::TargetGoalBase;
use crate::ai::mob_ai::MobAi;
use crate::ai::targeting::TargetingConditions;
use crate::host::MobHost;

/// `OwnerHurtTargetGoal`.
pub struct OwnerHurtTargetGoal<M: MobHost> {
    pub base: TargetGoalBase,
    owner_last_hurt: Option<i32>,
    timestamp: i32,
    default_conditions: TargetingConditions<M>,
}

impl<M: MobHost> OwnerHurtTargetGoal<M> {
    /// `OwnerHurtTargetGoal(TamableAnimal)`.
    #[must_use]
    pub fn new() -> Self {
        OwnerHurtTargetGoal {
            base: TargetGoalBase::new(false, false),
            owner_last_hurt: None,
            timestamp: 0,
            default_conditions: TargetingConditions::for_combat(),
        }
    }
}

impl<M: MobHost> Default for OwnerHurtTargetGoal<M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<M: MobHost> Goal<MobAi<M>> for OwnerHurtTargetGoal<M> {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        if !cx.mob.is_tame() || cx.mob.is_ordered_to_sit() {
            return false;
        }
        let Some(owner) = cx.mob.owner() else {
            return false;
        };
        self.owner_last_hurt = cx.mob.entity_last_hurt_mob(owner);
        let ts = cx.mob.entity_last_hurt_mob_timestamp(owner);
        if ts == self.timestamp {
            return false;
        }
        let t = self.owner_last_hurt;
        if !self.base.can_attack(cx, t, &self.default_conditions) {
            return false;
        }
        t.is_some_and(|t| cx.mob.wants_to_attack(t, owner))
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        self.base.can_continue_to_use(cx)
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        cx.mob.set_target(self.owner_last_hurt);
        if let Some(owner) = cx.mob.owner() {
            self.timestamp = cx.mob.entity_last_hurt_mob_timestamp(owner);
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
    fn a_tame_pet_joins_each_new_attack_its_owner_makes() {
        let mut mob = TestMob::new(GridLevel::new(), 0.5, 64.0, 0.5, 1);
        mob.tame = true;
        mob.owner = Some(9);
        mob.others
            .push((EntityClass::Player, EntityFacts::player(9, 2.5, 64.0, 0.5)));
        mob.others
            .push((EntityClass::Mob, EntityFacts::living(5, 4.5, 64.0, 0.5)));
        mob.last_hurt_mob.insert(9, (Some(5), 40));
        mob.visible.push(5);
        let mut ai = MobAi::new(mob);
        let mut goal = OwnerHurtTargetGoal::new();
        assert!(goal.can_use(&mut ai));
        goal.start(&mut ai);
        assert_eq!(ai.mob.target_raw, Some(5));
        assert!(!goal.can_use(&mut ai), "same timestamp");
        ai.mob.last_hurt_mob.insert(9, (Some(5), 41));
        assert!(goal.can_use(&mut ai));
        ai.mob.last_hurt_mob.insert(9, (None, 42));
        assert!(!goal.can_use(&mut ai), "owner hit nothing");
        ai.mob.tame = false;
        assert!(!goal.can_use(&mut ai));
    }
}
