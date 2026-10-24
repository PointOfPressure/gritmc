//! `net.minecraft.world.entity.ai.goal.AvoidEntityGoal<T>`, from the 26.2
//! jar. Flag `MOVE`.
//!
//! `avoidEntityTargeting = forCombat().range(maxDist).selector(e -> predicateOnAvoidEntity.test(e) && avoidPredicate.test(e))`,
//! the default `predicateOnAvoidEntity` being
//! `EntitySelector.NO_CREATIVE_OR_SPECTATOR` and `avoidPredicate` `true`.
//!
//! # `canUse()`, offsets 0..191
//!
//! ```text
//! toAvoid = level.getNearestEntity(getEntitiesOfClass(avoidClass, bb.inflate(maxDist, 3, maxDist)), avoidEntityTargeting, mob, mob.x, mob.y, mob.z)
//! if toAvoid == null return false
//! v = DefaultRandomPos.getPosAway(mob, 16, 7, toAvoid.position())
//! if v == null return false
//! if toAvoid.distanceToSqr(v) < toAvoid.distanceToSqr(mob) return false
//! path = pathNav.createPath(v.x, v.y, v.z, 0)
//! return path != null
//! ```
//!
//! A flight target that would bring the mob nearer to the threat than it
//! already is refuses; `getPosAway` draws through `RandomPos` (see
//! [`crate::ai::random_pos`]).
//!
//! `tick`: within seven blocks the sprint speed, else the walk speed, set
//! on the navigation each tick.

use crate::ai::goal::{Flag, FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::ai::random_pos::default_get_pos_away;
use crate::ai::targeting::{nearest_entity, Selector, TargetingConditions};
use crate::host::{EntityClass, MobHost};
use crate::pathfinder::Path;

/// `AvoidEntityGoal<T>`.
pub struct AvoidEntityGoal<M: MobHost> {
    walk_speed_modifier: f64,
    sprint_speed_modifier: f64,
    to_avoid: Option<i32>,
    max_dist: f32,
    path: Option<Path>,
    avoid_class: EntityClass,
    avoid_entity_targeting: TargetingConditions<M>,
}

/// The combined selector captures two boxed predicates, which pins `M` to
/// `'static` — every host in this crate is.
impl<M: MobHost + 'static> AvoidEntityGoal<M> {
    /// `AvoidEntityGoal(PathfinderMob, Class, float maxDist, double walkSpeed, double sprintSpeed)`:
    /// `avoidPredicate` true, `predicateOnAvoidEntity` `NO_CREATIVE_OR_SPECTATOR`.
    #[must_use]
    pub fn new(
        avoid_class: EntityClass,
        max_dist: f32,
        walk_speed: f64,
        sprint_speed: f64,
    ) -> Self {
        Self::with_predicates(avoid_class, max_dist, walk_speed, sprint_speed, None, None)
    }

    /// `AvoidEntityGoal(PathfinderMob, Class, Predicate avoidPredicate, float, double, double, Predicate predicateOnAvoidEntity)`.
    /// `None` for either predicate is the constructor default.
    #[must_use]
    pub fn with_predicates(
        avoid_class: EntityClass,
        max_dist: f32,
        walk_speed: f64,
        sprint_speed: f64,
        avoid_predicate: Option<Selector<M>>,
        predicate_on_avoid_entity: Option<Selector<M>>,
    ) -> Self {
        let selector: Selector<M> = Box::new(move |ai, f| {
            let on_avoid = match &predicate_on_avoid_entity {
                None => f.no_creative_or_spectator(),
                Some(p) => p(ai, f),
            };
            on_avoid && avoid_predicate.as_ref().is_none_or(|p| p(ai, f))
        });
        AvoidEntityGoal {
            walk_speed_modifier: walk_speed,
            sprint_speed_modifier: sprint_speed,
            to_avoid: None,
            max_dist,
            path: None,
            avoid_class,
            avoid_entity_targeting: TargetingConditions::for_combat()
                .range(f64::from(max_dist))
                .selector(selector),
        }
    }

    /// The entity being avoided, for tests.
    #[must_use]
    pub fn avoiding(&self) -> Option<i32> {
        self.to_avoid
    }
}

impl<M: MobHost> Goal<MobAi<M>> for AvoidEntityGoal<M> {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        let d = f64::from(self.max_dist);
        let area = cx.mob.bounding_box().inflate(d, 3.0, d);
        let ids = cx.mob.entities_of_class_in(self.avoid_class, &area);
        let (x, y, z) = (cx.mob.x(), cx.mob.y(), cx.mob.z());
        self.to_avoid = nearest_entity(cx, &ids, &self.avoid_entity_targeting, x, y, z);
        let Some(threat) = self.to_avoid.and_then(|id| cx.mob.entity(id)) else {
            return false;
        };
        let Some(v) = default_get_pos_away(cx, 16, 7, threat.position()) else {
            return false;
        };
        if threat.distance_to_sqr(v.x, v.y, v.z)
            < threat.distance_to_sqr(cx.mob.x(), cx.mob.y(), cx.mob.z())
        {
            return false;
        }
        self.path = cx.navigation.create_path_to(&mut cx.mob, v.x, v.y, v.z, 0);
        self.path.is_some()
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        !cx.navigation.is_done()
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        let p = self.path.clone();
        cx.navigation
            .move_along(&mut cx.mob, p, self.walk_speed_modifier);
    }

    fn stop(&mut self, _cx: &mut MobAi<M>) {
        self.to_avoid = None;
    }

    fn tick(&mut self, cx: &mut MobAi<M>) {
        let Some(t) = self.to_avoid.and_then(|id| cx.mob.entity(id)) else {
            return;
        };
        if cx.mob.position().distance_to_sqr3(t.x, t.y, t.z) < 49.0 {
            cx.navigation.set_speed_modifier(self.sprint_speed_modifier);
        } else {
            cx.navigation.set_speed_modifier(self.walk_speed_modifier);
        }
    }

    fn flags(&self) -> FlagSet {
        FlagSet::of(Flag::Move)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::EntityFacts;
    use crate::testing::{GridLevel, TestMob};

    /// A visible survival player within range is fled from along a path
    /// that ends farther from it; a creative player is not a threat.
    #[test]
    fn flees_a_nearby_player_along_a_path_that_gains_distance() {
        let mut g = GridLevel::new();
        g.floor(-30, -30, 30, 30, 63);
        let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, 1);
        mob.others = vec![(EntityClass::Player, EntityFacts::player(11, 3.5, 64.0, 0.5))];
        mob.visible = vec![11];
        let mut ai = MobAi::new(mob);
        let mut goal = AvoidEntityGoal::new(EntityClass::Player, 6.0, 1.0, 1.2);
        let mut found = false;
        for _ in 0..40 {
            if goal.can_use(&mut ai) {
                found = true;
                break;
            }
        }
        assert!(found, "some seed step yields a flight path on open floor");
        assert_eq!(goal.avoiding(), Some(11));
        goal.start(&mut ai);
        assert!(goal.can_continue_to_use(&mut ai));
        let end = ai
            .navigation
            .path()
            .expect("path")
            .end_node()
            .expect("node")
            .as_block_pos();
        let threat = ai.mob.entity(11).expect("player");
        assert!(
            threat.distance_to_sqr(f64::from(end.x) + 0.5, 64.0, f64::from(end.z) + 0.5) >= 9.0
        );
        goal.tick(&mut ai);
        ai.mob.others[0].1.is_creative = true;
        let mut g2 = AvoidEntityGoal::new(EntityClass::Player, 6.0, 1.0, 1.2);
        assert!(!g2.can_use(&mut ai));
    }
}
