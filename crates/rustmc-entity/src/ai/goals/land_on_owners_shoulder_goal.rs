//! `net.minecraft.world.entity.ai.goal.LandOnOwnersShoulderGoal`, from
//! the 26.2 jar. No flags. Used by Parrot.
//!
//! `canUse()`, offsets 0..88: the owner must be a `ServerPlayer`; `ok =
//! !spectator && !abilities.flying && !isInWater() && !isInPowderSnow`;
//! `!isOrderedToSit() && ok && canSitOnShoulder()`. `isInterruptable()`:
//! `!isSittingOnShoulder`. `start()`: `isSittingOnShoulder = false`.
//! `tick()`: unless sitting, in the sitting pose or leashed, when the
//! owner's box intersects this entity's, `isSittingOnShoulder =
//! setEntityOnShoulder(owner)`.

use crate::ai::goal::{FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::host::MobHost;

/// `LandOnOwnersShoulderGoal`.
#[derive(Debug, Clone, Default)]
pub struct LandOnOwnersShoulderGoal {
    is_sitting_on_shoulder: bool,
}

impl LandOnOwnersShoulderGoal {
    /// `LandOnOwnersShoulderGoal(ShoulderRidingEntity)`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl<M: MobHost> Goal<MobAi<M>> for LandOnOwnersShoulderGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        let Some(owner) = cx.mob.owner() else {
            return false;
        };
        let Some(f) = cx.mob.entity(owner) else {
            return false;
        };
        if !f.is_player {
            return false;
        }
        let ok = !f.is_spectator
            && !cx.mob.player_is_flying(owner)
            && !cx.mob.entity_in_water(owner)
            && !cx.mob.entity_in_powder_snow(owner);
        !cx.mob.is_ordered_to_sit() && ok && cx.mob.can_sit_on_shoulder()
    }

    fn is_interruptable(&self) -> bool {
        !self.is_sitting_on_shoulder
    }

    fn start(&mut self, _cx: &mut MobAi<M>) {
        self.is_sitting_on_shoulder = false;
    }

    fn tick(&mut self, cx: &mut MobAi<M>) {
        if self.is_sitting_on_shoulder || cx.mob.is_in_sitting_pose() || cx.mob.is_leashed() {
            return;
        }
        let Some(owner) = cx.mob.owner() else {
            return;
        };
        let Some(f) = cx.mob.entity(owner) else {
            return;
        };
        if !f.is_player {
            return;
        }
        if cx.mob.bounding_box().intersects(&f.bb) {
            self.is_sitting_on_shoulder = cx.mob.set_entity_on_shoulder(owner);
        }
    }

    fn flags(&self) -> FlagSet {
        FlagSet::NONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn lands_when_touching_a_grounded_dry_owner() {
        let mut mob = TestMob::new(GridLevel::new(), 0.5, 64.0, 0.5, 1);
        mob.owner = Some(9);
        mob.can_sit_on_shoulder = true;
        mob.shoulder_accepts = true;
        mob.others
            .push((EntityClass::Player, EntityFacts::player(9, 0.7, 64.0, 0.5)));
        let mut ai = MobAi::new(mob);
        let mut goal = LandOnOwnersShoulderGoal::new();
        assert!(goal.can_use(&mut ai));
        ai.mob.flying_players.push(9);
        assert!(!goal.can_use(&mut ai));
        ai.mob.flying_players.clear();
        goal.start(&mut ai);
        assert!(Goal::<MobAi<TestMob>>::is_interruptable(&goal));
        goal.tick(&mut ai);
        assert!(!Goal::<MobAi<TestMob>>::is_interruptable(&goal));
        assert_eq!(ai.mob.calls, vec!["setEntityOnShoulder(9)"]);
        goal.tick(&mut ai);
        assert_eq!(ai.mob.calls.len(), 1, "already sitting");
    }
}
