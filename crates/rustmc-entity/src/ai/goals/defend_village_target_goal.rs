//! `net.minecraft.world.entity.ai.goal.target.DefendVillageTargetGoal`,
//! from the 26.2 jar. `TargetGoal(golem, false, true)`, flag `TARGET`,
//! `attackTargeting = forCombat().range(64.0)`. Used by IronGolem.
//!
//! # `canUse()`, offsets 0..204
//!
//! ```text
//! box = golem.getBoundingBox().inflate(10.0, 8.0, 10.0)
//! villagers = level.getNearbyEntities(Villager.class, attackTargeting, golem, box)
//! players = level.getNearbyPlayers(attackTargeting, golem, box)
//! for villager in villagers:
//!     for player in players:
//!         if villager.getPlayerReputation(player) <= -100: potentialTarget = player
//! if potentialTarget == null return false
//! if potentialTarget is a Player && (spectator || creative) return false
//! return true
//! ```
//!
//! `potentialTarget` is a field that is never cleared: a hated player
//! found once stays the answer on every later `canUse` until another
//! one overwrites it. The last match in iteration order wins.
//! `start()`: `golem.setTarget(potentialTarget); super.start()`.

use crate::ai::goal::{Flag, FlagSet, Goal};
use crate::ai::goals::target_goal::TargetGoalBase;
use crate::ai::mob_ai::MobAi;
use crate::ai::targeting::{nearby_entities, TargetingConditions};
use crate::host::{EntityClass, MobHost};

/// `DefendVillageTargetGoal`.
pub struct DefendVillageTargetGoal<M: MobHost> {
    pub base: TargetGoalBase,
    potential_target: Option<i32>,
    attack_targeting: TargetingConditions<M>,
}

impl<M: MobHost> DefendVillageTargetGoal<M> {
    /// `DefendVillageTargetGoal(IronGolem)`.
    #[must_use]
    pub fn new() -> Self {
        DefendVillageTargetGoal {
            base: TargetGoalBase::new(false, true),
            potential_target: None,
            attack_targeting: TargetingConditions::for_combat().range(64.0),
        }
    }

    /// The `potentialTarget` field.
    #[must_use]
    pub fn potential_target(&self) -> Option<i32> {
        self.potential_target
    }
}

impl<M: MobHost> Default for DefendVillageTargetGoal<M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<M: MobHost> Goal<MobAi<M>> for DefendVillageTargetGoal<M> {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        let area = cx.mob.bounding_box().inflate(10.0, 8.0, 10.0);
        let villagers = nearby_entities(cx, EntityClass::Villager, &self.attack_targeting, &area);
        let players: Vec<i32> = {
            let ids = cx.mob.players();
            let mut out = Vec::new();
            for id in ids {
                let Some(f) = cx.mob.entity(id) else {
                    continue;
                };
                if f.bb.intersects(&area) && self.attack_targeting.test(cx, &f) {
                    out.push(id);
                }
            }
            out
        };
        for v in &villagers {
            for p in &players {
                if cx.mob.villager_player_reputation(*v, *p) <= -100 {
                    self.potential_target = Some(*p);
                }
            }
        }
        let Some(t) = self.potential_target else {
            return false;
        };
        if let Some(f) = cx.mob.entity(t) {
            if f.is_player && (f.is_spectator || f.is_creative) {
                return false;
            }
        }
        true
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        self.base.can_continue_to_use(cx)
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        cx.mob.set_target(self.potential_target);
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
    use crate::host::EntityFacts;
    use crate::testing::{GridLevel, TestMob};

    /// Two hated players: the later one in the players' order wins; the
    /// field then persists after the reputations are cleared.
    #[test]
    fn last_hated_player_wins_and_is_remembered() {
        let mut mob = TestMob::new(GridLevel::new(), 0.5, 64.0, 0.5, 1);
        mob.others.push((
            EntityClass::Villager,
            EntityFacts::living(3, 2.5, 64.0, 0.5),
        ));
        mob.others
            .push((EntityClass::Player, EntityFacts::player(8, 4.5, 64.0, 0.5)));
        mob.others
            .push((EntityClass::Player, EntityFacts::player(9, 6.5, 64.0, 0.5)));
        mob.visible.extend([3, 8, 9]);
        mob.reputation.insert((3, 8), -100);
        mob.reputation.insert((3, 9), -150);
        let mut ai = MobAi::new(mob);
        let mut goal = DefendVillageTargetGoal::new();
        assert!(goal.can_use(&mut ai));
        assert_eq!(goal.potential_target(), Some(9));
        ai.mob.reputation.clear();
        assert!(goal.can_use(&mut ai), "the field persists");
        ai.mob.others[2].1.is_creative = true;
        assert!(
            !goal.can_use(&mut ai),
            "creative: refused, still remembered"
        );
        assert_eq!(goal.potential_target(), Some(9));
        goal.start(&mut ai);
        assert_eq!(
            ai.mob.target_raw, None,
            "setTarget refuses a creative player"
        );
    }

    #[test]
    fn a_player_outside_the_box_is_not_seen() {
        let mut mob = TestMob::new(GridLevel::new(), 0.5, 64.0, 0.5, 1);
        mob.others.push((
            EntityClass::Villager,
            EntityFacts::living(3, 2.5, 64.0, 0.5),
        ));
        mob.others
            .push((EntityClass::Player, EntityFacts::player(8, 14.5, 64.0, 0.5)));
        mob.visible.extend([3, 8]);
        mob.reputation.insert((3, 8), -100);
        let mut ai = MobAi::new(mob);
        let mut goal = DefendVillageTargetGoal::new();
        assert!(!goal.can_use(&mut ai));
    }
}
