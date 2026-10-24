//! `net.minecraft.world.entity.ai.goal.target.ResetUniversalAngerTargetGoal<T
//! extends Mob & NeutralMob>`, from the 26.2 jar. No flags (`setFlags`
//! is never called). Used by Bee, EnderMan, IronGolem, Wolf, PolarBear
//! (5 mobs).
//!
//! ```text
//! canUse()            level.gameRules.get(UNIVERSAL_ANGER) && wasHurtByPlayer()
//! wasHurtByPlayer()   lastHurtByMob != null && lastHurtByMob.is(EntityTypes.PLAYER)
//!                       && mob.getLastHurtByMobTimestamp() > lastHurtByPlayerTimestamp
//! start()             lastHurtByPlayerTimestamp = mob.getLastHurtByMobTimestamp()
//!                     ((NeutralMob) mob).forgetCurrentTargetAndRefreshUniversalAnger()
//!                     if alertOthersOfSameType:
//!                         getNearbyMobsOfSameType().stream().filter(m -> m != mob)
//!                             .forEach(NeutralMob::forgetCurrentTargetAndRefreshUniversalAnger)
//!                     super.start()
//! getNearbyMobsOfSameType()
//!                     d = getAttributeValue(FOLLOW_RANGE)
//!                     aabb = AABB.unitCubeFromLowerCorner(mob.position()).inflate(d, 10.0, d)
//!                     level.getEntitiesOfClass(mob.getClass(), aabb, EntitySelector.NO_SPECTATORS)
//! ```
//!
//! `ALERT_RANGE_Y = 10`. The box is the unit cube at the mob's position,
//! not its bounding box.

use crate::ai::goal::{FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::host::{EntityClass, MobHost};
use crate::phys::Aabb;

/// `ResetUniversalAngerTargetGoal.ALERT_RANGE_Y`.
pub const ALERT_RANGE_Y: f64 = 10.0;

/// `ResetUniversalAngerTargetGoal`.
#[derive(Debug, Clone)]
pub struct ResetUniversalAngerTargetGoal {
    alert_others_of_same_type: bool,
    last_hurt_by_player_timestamp: i32,
}

impl ResetUniversalAngerTargetGoal {
    /// `ResetUniversalAngerTargetGoal(T, boolean)`.
    #[must_use]
    pub fn new(alert_others_of_same_type: bool) -> Self {
        ResetUniversalAngerTargetGoal {
            alert_others_of_same_type,
            last_hurt_by_player_timestamp: 0,
        }
    }

    /// `wasHurtByPlayer()`.
    fn was_hurt_by_player<M: MobHost>(&self, cx: &MobAi<M>) -> bool {
        let Some(f) = cx.mob.last_hurt_by_mob().and_then(|id| cx.mob.entity(id)) else {
            return false;
        };
        f.is_player && cx.mob.last_hurt_by_mob_timestamp() > self.last_hurt_by_player_timestamp
    }

    /// `getNearbyMobsOfSameType()`, in the level's order.
    fn nearby_mobs_of_same_type<M: MobHost>(cx: &MobAi<M>) -> Vec<i32> {
        let d = cx.mob.follow_range();
        let p = cx.mob.position();
        let aabb =
            Aabb::new(p.x, p.y, p.z, p.x + 1.0, p.y + 1.0, p.z + 1.0).inflate(d, ALERT_RANGE_Y, d);
        cx.mob
            .entities_of_class_in(EntityClass::SameKind, &aabb)
            .into_iter()
            .filter(|id| cx.mob.entity(*id).is_some_and(|f| f.no_spectators()))
            .collect()
    }
}

impl<M: MobHost> Goal<MobAi<M>> for ResetUniversalAngerTargetGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        cx.mob.universal_anger() && self.was_hurt_by_player(cx)
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        self.last_hurt_by_player_timestamp = cx.mob.last_hurt_by_mob_timestamp();
        cx.mob.forget_current_target_and_refresh_universal_anger();
        if self.alert_others_of_same_type {
            let me = cx.mob.id();
            for id in Self::nearby_mobs_of_same_type(cx) {
                if id != me {
                    cx.mob
                        .other_forget_current_target_and_refresh_universal_anger(id);
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
    use crate::host::EntityFacts;
    use crate::testing::{GridLevel, TestMob};

    /// Only under the rule, only for a player hit newer than the last
    /// one handled; `start` forgets on self and on the same-kind
    /// neighbours in the follow-range box (10 vertically), skipping
    /// spectators and itself.
    #[test]
    fn resets_anger_once_per_player_hit_and_alerts_the_kind() {
        let mut mob = TestMob::new(GridLevel::new(), 0.5, 64.0, 0.5, 1);
        mob.follow_range = 16.0;
        mob.others
            .push((EntityClass::Player, EntityFacts::player(9, 3.5, 64.0, 0.5)));
        mob.others.push((
            EntityClass::SameKind,
            EntityFacts::living(2, 10.5, 70.0, 0.5),
        ));
        mob.others.push((
            EntityClass::SameKind,
            EntityFacts::living(3, 10.5, 80.0, 0.5),
        ));
        let mut spec = EntityFacts::living(4, 2.5, 64.0, 0.5);
        spec.is_spectator = true;
        mob.others.push((EntityClass::SameKind, spec));
        mob.last_hurt_by_mob = Some(9);
        mob.last_hurt_by_mob_timestamp = 100;
        let mut ai = MobAi::new(mob);
        let mut goal = ResetUniversalAngerTargetGoal::new(true);
        assert!(!goal.can_use(&mut ai), "rule off");
        ai.mob.universal_anger = true;
        assert!(goal.can_use(&mut ai));
        goal.start(&mut ai);
        assert_eq!(
            ai.mob.calls,
            vec![
                "forgetCurrentTargetAndRefreshUniversalAnger",
                "forgetCurrentTargetAndRefreshUniversalAnger(2)"
            ]
        );
        assert!(!goal.can_use(&mut ai), "same timestamp handled");
        ai.mob.last_hurt_by_mob_timestamp = 101;
        assert!(goal.can_use(&mut ai));
        ai.mob.last_hurt_by_mob = Some(2);
        assert!(!goal.can_use(&mut ai), "hit by a non-player");
        assert!(Goal::<MobAi<TestMob>>::flags(&goal).is_empty());
    }
}
