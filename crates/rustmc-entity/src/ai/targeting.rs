//! `net.minecraft.world.entity.ai.targeting.TargetingConditions` and the
//! `ServerEntityGetter` selections the goals make with it, from the 26.2
//! jar.
//!
//! # `TargetingConditions.test(ServerLevel, LivingEntity attacker, LivingEntity target)`, offsets 0..202
//!
//! ```text
//! if attacker == target                          return false
//! if !target.canBeSeenByAnyone()                 return false
//! if selector != null && !selector.test(target)  return false
//! if attacker == null:
//!     if isCombat && (!target.canBeSeenAsEnemy() || difficulty == PEACEFUL) return false
//! else:
//!     if isCombat && (!attacker.canAttack(target) || attacker.isAlliedTo(target)) return false
//!     if range > 0:
//!         vis = testInvisible ? target.getVisibilityPercent(attacker) : 1.0
//!         r = max(range * vis, 2.0)
//!         if attacker.distanceToSqr(target) > r * r return false
//!     if checkLineOfSight && attacker instanceof Mob && !attacker.sensing.hasLineOfSight(target) return false
//! return true
//! ```
//!
//! The order matters for one side effect only: `Sensing.hasLineOfSight`
//! memoises a ray cast, and it is asked last, after the cheap refusals.
//!
//! # `ServerEntityGetter.getNearestEntity(List, conditions, attacker, x, y, z)`, offsets 0..100
//!
//! The first entity in list order that passes `test` and is strictly
//! nearer (`dcmpg; ifge`) than the best so far, starting from `-1.0`
//! which the first passing entity always beats. Ties keep the earlier.
//! `getNearestPlayer` is the same over `players()`.

use super::mob_ai::MobAi;
use crate::host::{EntityClass, EntityFacts, MobHost};
use crate::phys::Aabb;

/// The `Selector` lambda: `test(LivingEntity, ServerLevel)`.
pub type Selector<M> = Box<dyn Fn(&MobAi<M>, &EntityFacts) -> bool>;

/// `TargetingConditions`.
pub struct TargetingConditions<M: MobHost> {
    is_combat: bool,
    /// `range`, `-1.0` at construction.
    range: f64,
    check_line_of_sight: bool,
    test_invisible: bool,
    selector: Option<Selector<M>>,
}

impl<M: MobHost> TargetingConditions<M> {
    /// `forCombat()`.
    #[must_use]
    pub fn for_combat() -> Self {
        Self::new(true)
    }

    /// `forNonCombat()`.
    #[must_use]
    pub fn for_non_combat() -> Self {
        Self::new(false)
    }

    fn new(is_combat: bool) -> Self {
        TargetingConditions {
            is_combat,
            range: -1.0,
            check_line_of_sight: true,
            test_invisible: true,
            selector: None,
        }
    }

    /// `range(double)` — mutates and returns `this`; here a builder.
    #[must_use]
    pub fn range(mut self, r: f64) -> Self {
        self.range = r;
        self
    }

    /// `range(double)` in place, for the goals that call it on a shared
    /// instance every time (`NearestAttackableTargetGoal.getTargetConditions`).
    pub fn set_range(&mut self, r: f64) {
        self.range = r;
    }

    /// `ignoreLineOfSight()`.
    #[must_use]
    pub fn ignore_line_of_sight(mut self) -> Self {
        self.check_line_of_sight = false;
        self
    }

    /// `ignoreInvisibilityTesting()`.
    #[must_use]
    pub fn ignore_invisibility_testing(mut self) -> Self {
        self.test_invisible = false;
        self
    }

    /// `selector(Selector)`.
    #[must_use]
    pub fn selector(mut self, s: Selector<M>) -> Self {
        self.selector = Some(s);
        self
    }

    /// `copy()` without the selector (a boxed closure is not `Clone`);
    /// the one caller, `TemptGoal`, sets its own selector right after.
    #[must_use]
    pub fn copy_without_selector(&self) -> Self {
        TargetingConditions {
            is_combat: self.is_combat,
            range: self.range,
            check_line_of_sight: self.check_line_of_sight,
            test_invisible: self.test_invisible,
            selector: None,
        }
    }

    /// `test(ServerLevel, LivingEntity attacker, LivingEntity target)` with
    /// the mob as the attacker.
    pub fn test(&self, ai: &mut MobAi<M>, target: &EntityFacts) -> bool {
        if ai.mob.id() == target.id {
            return false;
        }
        if !target.can_be_seen_by_anyone() {
            return false;
        }
        if let Some(sel) = &self.selector {
            if !sel(ai, target) {
                return false;
            }
        }
        if self.is_combat && (!ai.mob.can_attack(target) || ai.mob.is_allied_to(target.id)) {
            return false;
        }
        if self.range > 0.0 {
            let vis = if self.test_invisible {
                ai.mob.visibility_percent_of(target.id)
            } else {
                1.0
            };
            let r = rustmc_worldgen::math::java_max(self.range * vis, 2.0);
            if ai
                .mob
                .position()
                .distance_to_sqr3(target.x, target.y, target.z)
                > r * r
            {
                return false;
            }
        }
        if self.check_line_of_sight {
            let id = target.id;
            let MobAi { mob, sensing, .. } = ai;
            if !sensing.has_line_of_sight(id, || mob.has_line_of_sight_to(id)) {
                return false;
            }
        }
        true
    }

    /// `test` with no attacker (`attacker == null`): the combat refusal is
    /// on the target alone.
    pub fn test_without_attacker(&self, ai: &MobAi<M>, target: &EntityFacts) -> bool {
        if !target.can_be_seen_by_anyone() {
            return false;
        }
        if let Some(sel) = &self.selector {
            if !sel(ai, target) {
                return false;
            }
        }
        if self.is_combat && (!target.can_be_seen_as_enemy() || ai.mob.difficulty_is_peaceful()) {
            return false;
        }
        true
    }
}

/// `ServerEntityGetter.getNearestEntity(List<T>, TargetingConditions, LivingEntity, double, double, double)`
/// over the given ids in their list order.
pub fn nearest_entity<M: MobHost>(
    ai: &mut MobAi<M>,
    ids: &[i32],
    conditions: &TargetingConditions<M>,
    x: f64,
    y: f64,
    z: f64,
) -> Option<i32> {
    let mut best_d = -1.0f64;
    let mut best: Option<i32> = None;
    for &id in ids {
        let Some(facts) = ai.mob.entity(id) else {
            continue;
        };
        if !conditions.test(ai, &facts) {
            continue;
        }
        let d = facts.distance_to_sqr(x, y, z);
        if best_d == -1.0 || d < best_d {
            best_d = d;
            best = Some(id);
        }
    }
    best
}

/// `getNearestPlayer(TargetingConditions, LivingEntity, double, double, double)`:
/// `getNearestEntity(players(), ...)`.
pub fn nearest_player<M: MobHost>(
    ai: &mut MobAi<M>,
    conditions: &TargetingConditions<M>,
    x: f64,
    y: f64,
    z: f64,
) -> Option<i32> {
    let players = ai.mob.players();
    nearest_entity(ai, &players, conditions, x, y, z)
}

/// `getNearestPlayer(TargetingConditions, LivingEntity)`: at the mob's
/// own position.
pub fn nearest_player_to_mob<M: MobHost>(
    ai: &mut MobAi<M>,
    conditions: &TargetingConditions<M>,
) -> Option<i32> {
    let (x, y, z) = (ai.mob.x(), ai.mob.y(), ai.mob.z());
    nearest_player(ai, conditions, x, y, z)
}

/// `getNearbyEntities(Class, TargetingConditions, LivingEntity, AABB)`:
/// every entity of the class in the box that passes `test`, in the
/// level's order.
pub fn nearby_entities<M: MobHost>(
    ai: &mut MobAi<M>,
    class: EntityClass,
    conditions: &TargetingConditions<M>,
    aabb: &Aabb,
) -> Vec<i32> {
    let ids = ai.mob.entities_of_class_in(class, aabb);
    let mut out = Vec::new();
    for id in ids {
        let Some(facts) = ai.mob.entity(id) else {
            continue;
        };
        if conditions.test(ai, &facts) {
            out.push(id);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{GridLevel, TestMob};

    fn ai_with(others: Vec<(EntityClass, EntityFacts)>) -> MobAi<TestMob> {
        let mut mob = TestMob::new(GridLevel::new(), 0.5, 64.0, 0.5, 1);
        mob.others = others;
        MobAi::new(mob)
    }

    /// The nearest player in list order, strictly nearer wins, ties keep
    /// the earlier, and a spectator is never seen.
    #[test]
    fn nearest_player_is_strictly_nearest_in_list_order() {
        let mut spectator = EntityFacts::player(13, 1.5, 64.0, 0.5);
        spectator.is_spectator = true;
        let mut ai = ai_with(vec![
            (EntityClass::Player, EntityFacts::player(11, 3.5, 64.0, 0.5)),
            (EntityClass::Player, EntityFacts::player(12, 0.5, 64.0, 3.5)),
            (EntityClass::Player, spectator),
        ]);
        ai.mob.visible = vec![11, 12, 13];
        let c = TargetingConditions::for_non_combat().range(16.0);
        assert_eq!(nearest_player_to_mob(&mut ai, &c), Some(11));
        // Line of sight refused for 11: 12 wins.
        ai.mob.visible = vec![12, 13];
        ai.sensing.tick();
        assert_eq!(nearest_player_to_mob(&mut ai, &c), Some(12));
        // Ignoring line of sight: 11 again.
        let c2 = TargetingConditions::for_non_combat()
            .range(16.0)
            .ignore_line_of_sight();
        assert_eq!(nearest_player_to_mob(&mut ai, &c2), Some(11));
    }

    /// Range: `max(range * visibility, 2.0)`, squared, strictly greater
    /// refuses; visibility is not tested when ignored.
    #[test]
    fn range_is_scaled_by_visibility_with_a_floor_of_two() {
        let mut ai = ai_with(vec![(
            EntityClass::Player,
            EntityFacts::player(11, 5.5, 64.0, 0.5),
        )]);
        ai.mob.visible = vec![11];
        let c = TargetingConditions::for_non_combat().range(6.0);
        assert_eq!(nearest_player_to_mob(&mut ai, &c), Some(11));
        ai.mob.visibility.insert(11, 0.5);
        // 6 * 0.5 = 3 < 5: refused.
        assert_eq!(nearest_player_to_mob(&mut ai, &c), None);
        let c = TargetingConditions::for_non_combat()
            .range(6.0)
            .ignore_invisibility_testing();
        assert_eq!(nearest_player_to_mob(&mut ai, &c), Some(11));
        // The floor: range 1 * visibility 1 = 1, raised to 2; a target at
        // 1.9 passes and one at 2.1 does not.
        ai.mob.visibility.clear();
        ai.mob.others[0].1.x = 0.5 + 1.9;
        let c = TargetingConditions::for_non_combat().range(1.0);
        assert_eq!(nearest_player_to_mob(&mut ai, &c), Some(11));
        ai.mob.others[0].1.x = 0.5 + 2.1;
        assert_eq!(nearest_player_to_mob(&mut ai, &c), None);
    }

    /// Combat: peaceful difficulty refuses players, allies are refused,
    /// a ghast is never attackable, the selector runs before all of that.
    #[test]
    fn combat_refusals() {
        let mut ai = ai_with(vec![(
            EntityClass::Player,
            EntityFacts::player(11, 3.5, 64.0, 0.5),
        )]);
        ai.mob.visible = vec![11];
        let c = TargetingConditions::for_combat().range(16.0);
        assert_eq!(nearest_player_to_mob(&mut ai, &c), Some(11));
        ai.mob.peaceful = true;
        assert_eq!(nearest_player_to_mob(&mut ai, &c), None);
        ai.mob.peaceful = false;
        ai.mob.allies = vec![11];
        assert_eq!(nearest_player_to_mob(&mut ai, &c), None);
        ai.mob.allies.clear();
        let c = TargetingConditions::for_combat()
            .range(16.0)
            .selector(Box::new(|_, f| f.id != 11));
        assert_eq!(nearest_player_to_mob(&mut ai, &c), None);
        let mut ghast = EntityFacts::living(19, 2.5, 64.0, 0.5);
        ghast.is_ghast = true;
        ai.mob.others.push((EntityClass::Mob, ghast));
        ai.mob.visible.push(19);
        let c = TargetingConditions::for_combat().range(16.0);
        let box_ = ai.mob.bounding_box().inflate(16.0, 16.0, 16.0);
        assert_eq!(
            nearby_entities(&mut ai, EntityClass::Living, &c, &box_),
            vec![11]
        );
    }
}
