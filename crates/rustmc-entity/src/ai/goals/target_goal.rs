//! `net.minecraft.world.entity.ai.goal.target.{TargetGoal,
//! NearestAttackableTargetGoal, HurtByTargetGoal}`, from the 26.2 jar.
//! All three carry flag `TARGET` and run in the mob's `targetSelector`.
//!
//! # `TargetGoal`
//!
//! The base holds `mustSee`, `mustReach`, a reach cache (`0` empty, `1`
//! can, `2` cannot) with its countdown, `unseenTicks`, `targetMob` and
//! `unseenMemoryTicks = 60`.
//!
//! `canContinueToUse()`, offsets 0..146:
//!
//! ```text
//! t = mob.getTarget(); if t == null t = targetMob; if t == null return false
//! if !mob.canAttack(t) return false
//! if mob.getTeam() != null && t.getTeam() == mob.getTeam() return false
//! d = getFollowDistance(); if mob.distanceToSqr(t) > d*d return false
//! if mustSee:
//!     if sensing.hasLineOfSight(t): unseenTicks = 0
//!     else if ++unseenTicks > reducedTickDelay(unseenMemoryTicks) return false
//! mob.setTarget(t); return true
//! ```
//!
//! `canAttack(LivingEntity, TargetingConditions)`, offsets 0..104: the
//! conditions, `isWithinHome(t.blockPosition())`, and when `mustReach`
//! the reach cache — `canReach` draws `nextInt(5)` at offset 11 for its
//! `reducedTickDelay(10 + n)` countdown and paths to the target with
//! accuracy 0, accepting an end node within `sqrt(2.25)` blocks
//! horizontally of the target's block.
//!
//! # `NearestAttackableTargetGoal`
//!
//! `randomInterval = reducedTickDelay(10) = 5`: `canUse` draws
//! `nextInt(5)` (offset 18) and refuses a non-zero, then `findTarget` —
//! the nearest player through `players()` for a player class, else the
//! nearest entity of the class in the follow-range box — and answers
//! whether one was found. `start` sets the mob's target then the base's
//! `start`.
//!
//! # `HurtByTargetGoal`
//!
//! `canUse`: the last-hurt-by timestamp must differ from the remembered
//! one and the attacker exist; a player attacker is refused under
//! `universalAnger`; the ignore-damage classes are refused; then
//! `canAttack(attacker, HURT_BY_TARGETING)` with line of sight and
//! invisibility ignored. `start` sets the target, remembers it and the
//! timestamp, sets `unseenMemoryTicks = 300`, alerts others when asked
//! (every mob of the same kind in the follow-range box inflated 10
//! vertically, skipping self, the already-targeting, other owners' pets,
//! allies of the attacker and the ignore-alert kinds), then the base's
//! `start`.

use rustmc_worldgen::rng::RandomSource;

use crate::ai::goal::{reduced_tick_delay, Flag, FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::ai::targeting::{nearest_entity, nearest_player, Selector, TargetingConditions};
use crate::host::{EntityClass, EntityFacts, MobHost};

/// `TargetGoal`'s state, embedded by the two subclasses.
pub struct TargetGoalBase {
    pub must_see: bool,
    must_reach: bool,
    /// `reachCache`: 0 empty, 1 can, 2 cannot.
    reach_cache: i32,
    reach_cache_time: i32,
    unseen_ticks: i32,
    /// `targetMob`.
    pub target_mob: Option<i32>,
    /// `unseenMemoryTicks`, 60.
    pub unseen_memory_ticks: i32,
}

impl TargetGoalBase {
    /// `TargetGoal(Mob, boolean mustSee, boolean mustReach)`.
    #[must_use]
    pub fn new(must_see: bool, must_reach: bool) -> Self {
        TargetGoalBase {
            must_see,
            must_reach,
            reach_cache: 0,
            reach_cache_time: 0,
            unseen_ticks: 0,
            target_mob: None,
            unseen_memory_ticks: 60,
        }
    }

    /// `getFollowDistance()`: `getAttributeValue(FOLLOW_RANGE)`.
    pub fn follow_distance<M: MobHost>(cx: &MobAi<M>) -> f64 {
        cx.mob.follow_range()
    }

    /// `canContinueToUse()`.
    pub fn can_continue_to_use<M: MobHost>(&mut self, cx: &mut MobAi<M>) -> bool {
        let t = cx.mob.target().or(self.target_mob);
        let Some(t) = t else {
            return false;
        };
        let Some(facts) = cx.mob.entity(t) else {
            return false;
        };
        if !cx.mob.can_attack(&facts) {
            return false;
        }
        if cx.mob.on_same_team_as(t) {
            return false;
        }
        let d = Self::follow_distance(cx);
        if cx
            .mob
            .position()
            .distance_to_sqr3(facts.x, facts.y, facts.z)
            > d * d
        {
            return false;
        }
        if self.must_see {
            let MobAi { mob, sensing, .. } = cx;
            if sensing.has_line_of_sight(t, || mob.has_line_of_sight_to(t)) {
                self.unseen_ticks = 0;
            } else {
                self.unseen_ticks += 1;
                if self.unseen_ticks > reduced_tick_delay(self.unseen_memory_ticks) {
                    return false;
                }
            }
        }
        cx.mob.set_target(Some(t));
        true
    }

    /// `start()`: clear the reach cache and the unseen count.
    pub fn start(&mut self) {
        self.reach_cache = 0;
        self.reach_cache_time = 0;
        self.unseen_ticks = 0;
    }

    /// `stop()`: `mob.setTarget(null); targetMob = null`.
    pub fn stop<M: MobHost>(&mut self, cx: &mut MobAi<M>) {
        cx.mob.set_target(None);
        self.target_mob = None;
    }

    /// `canAttack(LivingEntity, TargetingConditions)`.
    pub fn can_attack<M: MobHost>(
        &mut self,
        cx: &mut MobAi<M>,
        target: Option<i32>,
        conditions: &TargetingConditions<M>,
    ) -> bool {
        let Some(t) = target else {
            return false;
        };
        let Some(facts) = cx.mob.entity(t) else {
            return false;
        };
        if !conditions.test(cx, &facts) {
            return false;
        }
        if !cx.mob.is_within_home_pos(facts.block_position()) {
            return false;
        }
        if self.must_reach {
            self.reach_cache_time -= 1;
            if self.reach_cache_time <= 0 {
                self.reach_cache = 0;
            }
            if self.reach_cache == 0 {
                self.reach_cache = if self.can_reach(cx, &facts) { 1 } else { 2 };
            }
            if self.reach_cache == 2 {
                return false;
            }
        }
        true
    }

    /// `canReach(LivingEntity)`: `nextInt(5)` for the countdown, then a
    /// path to the target's block with accuracy 0 whose end node is within
    /// `sqrt(2.25)` horizontally of it.
    fn can_reach<M: MobHost>(&mut self, cx: &mut MobAi<M>, target: &EntityFacts) -> bool {
        self.reach_cache_time = reduced_tick_delay(10 + cx.mob.random().next_i32_bound(5));
        let Some(path) =
            cx.navigation
                .create_path_to_entity_at(&mut cx.mob, target.block_position(), 0)
        else {
            return false;
        };
        let Some(end) = path.end_node() else {
            return false;
        };
        let tb = target.block_position();
        let dx = end.x - tb.x;
        let dz = end.z - tb.z;
        f64::from(dx * dx + dz * dz) <= 2.25
    }

    /// `setUnseenMemoryTicks(int)`.
    pub fn set_unseen_memory_ticks(&mut self, n: i32) {
        self.unseen_memory_ticks = n;
    }
}

/// `NearestAttackableTargetGoal<T>`.
pub struct NearestAttackableTargetGoal<M: MobHost> {
    pub base: TargetGoalBase,
    target_type: EntityClass,
    random_interval: i32,
    target: Option<i32>,
    target_conditions: TargetingConditions<M>,
}

impl<M: MobHost> NearestAttackableTargetGoal<M> {
    /// `NearestAttackableTargetGoal(Mob, Class, boolean mustSee)`:
    /// interval 10, `mustReach` false, no selector.
    #[must_use]
    pub fn new(cx: &MobAi<M>, target_type: EntityClass, must_see: bool) -> Self {
        Self::with(cx, target_type, 10, must_see, false, None)
    }

    /// `NearestAttackableTargetGoal(Mob, Class, int randomInterval, boolean mustSee, boolean mustReach, Selector)`.
    #[must_use]
    pub fn with(
        cx: &MobAi<M>,
        target_type: EntityClass,
        random_interval: i32,
        must_see: bool,
        must_reach: bool,
        selector: Option<Selector<M>>,
    ) -> Self {
        let base = TargetGoalBase::new(must_see, must_reach);
        let mut conditions =
            TargetingConditions::for_combat().range(TargetGoalBase::follow_distance(cx));
        if let Some(s) = selector {
            conditions = conditions.selector(s);
        }
        NearestAttackableTargetGoal {
            base,
            target_type,
            random_interval: reduced_tick_delay(random_interval),
            target: None,
            target_conditions: conditions,
        }
    }

    /// `findTarget()`.
    pub(crate) fn find_target(&mut self, cx: &mut MobAi<M>) {
        self.target_conditions
            .set_range(TargetGoalBase::follow_distance(cx));
        let (x, y, z) = (cx.mob.x(), cx.mob.eye_y(), cx.mob.z());
        self.target = if self.target_type == EntityClass::Player {
            nearest_player(cx, &self.target_conditions, x, y, z)
        } else {
            let d = TargetGoalBase::follow_distance(cx);
            let area = cx.mob.bounding_box().inflate(d, d, d);
            let ids = cx.mob.entities_of_class_in(self.target_type, &area);
            nearest_entity(cx, &ids, &self.target_conditions, x, y, z)
        };
    }

    /// `setTarget(LivingEntity)`.
    pub fn set_target(&mut self, id: Option<i32>) {
        self.target = id;
    }

    /// The found target, for tests.
    #[must_use]
    pub fn found(&self) -> Option<i32> {
        self.target
    }

    /// `targetConditions.test(level, mob, target)` against the stored
    /// `target`, for `NonTameRandomTargetGoal.canContinueToUse`.
    pub(crate) fn conditions_hold_for_found(&self, cx: &mut MobAi<M>) -> bool {
        let Some(facts) = self.target.and_then(|t| cx.mob.entity(t)) else {
            return false;
        };
        self.target_conditions.test(cx, &facts)
    }
}

impl<M: MobHost> Goal<MobAi<M>> for NearestAttackableTargetGoal<M> {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        if self.random_interval > 0 && cx.mob.random().next_i32_bound(self.random_interval) != 0 {
            return false;
        }
        self.find_target(cx);
        self.target.is_some()
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        self.base.can_continue_to_use(cx)
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        cx.mob.set_target(self.target);
        self.base.start();
    }

    fn stop(&mut self, cx: &mut MobAi<M>) {
        self.base.stop(cx);
    }

    fn flags(&self) -> FlagSet {
        FlagSet::of(Flag::Target)
    }
}

/// `HurtByTargetGoal`.
pub struct HurtByTargetGoal<M: MobHost> {
    pub base: TargetGoalBase,
    alert_same_type: bool,
    timestamp: i32,
    /// `toIgnoreDamage`: attackers of these classes are ignored.
    to_ignore_damage: Vec<EntityClass>,
    /// `toIgnoreAlert`: kinds not alerted.
    to_ignore_alert: Option<Vec<EntityClass>>,
    hurt_by_targeting: TargetingConditions<M>,
}

impl<M: MobHost> HurtByTargetGoal<M> {
    /// `HurtByTargetGoal(PathfinderMob, Class...)`: `mustSee` true.
    #[must_use]
    pub fn new(to_ignore_damage: Vec<EntityClass>) -> Self {
        HurtByTargetGoal {
            base: TargetGoalBase::new(true, false),
            alert_same_type: false,
            timestamp: 0,
            to_ignore_damage,
            to_ignore_alert: None,
            hurt_by_targeting: TargetingConditions::for_combat()
                .ignore_line_of_sight()
                .ignore_invisibility_testing(),
        }
    }

    /// `setAlertOthers(Class...)`.
    #[must_use]
    pub fn set_alert_others(mut self, to_ignore_alert: Vec<EntityClass>) -> Self {
        self.alert_same_type = true;
        self.to_ignore_alert = Some(to_ignore_alert);
        self
    }

    /// `alertOthers()`: the host is asked for the same-kind mobs in the
    /// box and, for each candidate that passes the jar's skips, told to
    /// target the attacker.
    fn alert_others(&self, cx: &mut MobAi<M>) {
        let d = TargetGoalBase::follow_distance(cx);
        let p = cx.mob.position();
        let area = crate::phys::Aabb::new(p.x, p.y, p.z, p.x + 1.0, p.y + 1.0, p.z + 1.0)
            .inflate(d, 10.0, d);
        let attacker = cx.mob.last_hurt_by_mob();
        let ids = cx.mob.entities_of_class_in(EntityClass::SameKind, &area);
        for id in ids {
            if id == cx.mob.id() {
                continue;
            }
            let Some(other) = cx.mob.entity(id) else {
                continue;
            };
            if !other.no_spectators() {
                continue;
            }
            if other.target.is_some() {
                continue;
            }
            if cx.mob.owner_differs_from(id) {
                continue;
            }
            if let Some(a) = attacker {
                if cx.mob.other_is_allied_to(id, a) {
                    continue;
                }
            }
            if let Some(ignore) = &self.to_ignore_alert {
                if ignore.iter().any(|c| cx.mob.entity_is_exactly(id, *c)) {
                    continue;
                }
            }
            cx.mob.alert_other(id, attacker);
        }
    }
}

impl<M: MobHost> Goal<MobAi<M>> for HurtByTargetGoal<M> {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        let ts = cx.mob.last_hurt_by_mob_timestamp();
        let attacker = cx.mob.last_hurt_by_mob();
        if ts == self.timestamp || attacker.is_none() {
            return false;
        }
        let attacker = attacker.expect("checked");
        let Some(facts) = cx.mob.entity(attacker) else {
            return false;
        };
        if facts.is_player && cx.mob.universal_anger() {
            return false;
        }
        if self
            .to_ignore_damage
            .iter()
            .any(|c| cx.mob.entity_is_assignable_to(attacker, *c))
        {
            return false;
        }
        let conditions = std::mem::replace(
            &mut self.hurt_by_targeting,
            TargetingConditions::for_combat(),
        );
        let ok = self.base.can_attack(cx, Some(attacker), &conditions);
        self.hurt_by_targeting = conditions;
        ok
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        self.base.can_continue_to_use(cx)
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        let attacker = cx.mob.last_hurt_by_mob();
        cx.mob.set_target(attacker);
        self.base.target_mob = cx.mob.target();
        self.timestamp = cx.mob.last_hurt_by_mob_timestamp();
        self.base.unseen_memory_ticks = 300;
        if self.alert_same_type {
            self.alert_others(cx);
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
    use crate::testing::{GridLevel, TestMob};
    use rustmc_worldgen::rng::LegacyRandomSource;

    fn ai_with(others: Vec<(EntityClass, EntityFacts)>, seed: i64) -> MobAi<TestMob> {
        let mut g = GridLevel::new();
        g.floor(-20, -20, 20, 20, 63);
        let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, seed);
        mob.others = others;
        MobAi::new(mob)
    }

    /// `canUse` draws `nextInt(5)` and only a zero looks; the nearest
    /// visible player is taken and set as the target on `start`.
    #[test]
    fn nearest_attackable_gates_on_next_int_5_then_finds_the_nearest_player() {
        let mut ai = ai_with(
            vec![
                (EntityClass::Player, EntityFacts::player(11, 6.5, 64.0, 0.5)),
                (EntityClass::Player, EntityFacts::player(12, 2.5, 64.0, 0.5)),
            ],
            3,
        );
        ai.mob.visible = vec![11, 12];
        let mut g = NearestAttackableTargetGoal::new(&ai, EntityClass::Player, true);
        let mut r = LegacyRandomSource::new(3);
        let mut found = false;
        for _ in 0..40 {
            let draw = r.next_i32_bound(5);
            let got = g.can_use(&mut ai);
            assert_eq!(got, draw == 0);
            if got {
                found = true;
                break;
            }
        }
        assert!(found);
        assert_eq!(g.found(), Some(12));
        g.start(&mut ai);
        assert_eq!(ai.mob.target(), Some(12));
        assert!(g.can_continue_to_use(&mut ai));
        // Out of sight for more than reducedTickDelay(60) = 30 ticks: dropped.
        ai.mob.visible.clear();
        for _ in 0..30 {
            ai.sensing.tick();
            assert!(g.can_continue_to_use(&mut ai));
        }
        ai.sensing.tick();
        assert!(!g.can_continue_to_use(&mut ai));
        g.stop(&mut ai);
        assert_eq!(ai.mob.target(), None);
    }

    /// A creative player is never a valid target: `setTarget` stores null.
    #[test]
    fn a_creative_player_is_not_a_valid_target() {
        let mut creative = EntityFacts::player(11, 2.5, 64.0, 0.5);
        creative.is_creative = true;
        let mut ai = ai_with(vec![(EntityClass::Player, creative)], 1);
        ai.mob.set_target(Some(11));
        assert_eq!(ai.mob.target(), None);
    }

    /// `HurtByTargetGoal`: a new hurt timestamp with a living attacker
    /// starts the goal against it, remembers the timestamp so the same
    /// hit does not restart it, and sets a 300-tick memory.
    #[test]
    fn hurt_by_target_takes_the_attacker_once_per_hit() {
        let mut ai = ai_with(
            vec![(EntityClass::Mob, EntityFacts::living(21, 3.5, 64.0, 0.5))],
            1,
        );
        ai.mob.visible = vec![21];
        let mut g = HurtByTargetGoal::new(Vec::new());
        assert!(!g.can_use(&mut ai), "never hurt");
        ai.mob.last_hurt_by_mob = Some(21);
        ai.mob.last_hurt_by_mob_timestamp = 40;
        assert!(g.can_use(&mut ai));
        g.start(&mut ai);
        assert_eq!(ai.mob.target(), Some(21));
        assert_eq!(g.base.unseen_memory_ticks, 300);
        assert!(!g.can_use(&mut ai), "same timestamp");
        ai.mob.last_hurt_by_mob_timestamp = 41;
        assert!(g.can_use(&mut ai));
    }
}
