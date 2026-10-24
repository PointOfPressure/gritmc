//! `net.minecraft.world.entity.ai.goal.BreedGoal`, from the 26.2 jar.
//! Flags `MOVE, LOOK`.
//!
//! `PARTNER_TARGETING = forNonCombat().range(8.0).ignoreLineOfSight()`.
//!
//! `canUse`: `animal.isInLove() && (partner = getFreePartner()) != null`.
//! `getFreePartner`, offsets 0..122: over
//! `getNearbyEntities(partnerClass, PARTNER_TARGETING, animal, bb.inflate(8))`
//! in the level's order, the first strictly nearest that `canMate` and
//! is not panicking (`animal.canMate(other)` before `other.isPanicking()`
//! before the distance).
//!
//! `canContinueToUse`: partner alive, in love, `loveTime < 60`, not
//! panicking. `tick`: look at the partner with yaw 10 and the mob's max
//! head pitch, path to it, `loveTime++`, and at
//! `loveTime >= adjustedTickDelay(60)` within nine blocks squared,
//! `breed()` — `spawnChildFromBreeding`, the host's.

use crate::ai::goal::{adjusted_tick_delay, Flag, FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::ai::targeting::{nearby_entities, TargetingConditions};
use crate::host::{EntityClass, MobHost};

/// `BreedGoal`.
pub struct BreedGoal<M: MobHost> {
    partner_class: EntityClass,
    partner: Option<i32>,
    love_time: i32,
    speed_modifier: f64,
    partner_targeting: TargetingConditions<M>,
}

impl<M: MobHost> BreedGoal<M> {
    /// `BreedGoal(Animal, double)`: the partner class is the animal's own.
    #[must_use]
    pub fn new(speed_modifier: f64) -> Self {
        Self::with_partner_class(speed_modifier, EntityClass::SameKind)
    }

    /// `BreedGoal(Animal, double, Class)`.
    #[must_use]
    pub fn with_partner_class(speed_modifier: f64, partner_class: EntityClass) -> Self {
        BreedGoal {
            partner_class,
            partner: None,
            love_time: 0,
            speed_modifier,
            partner_targeting: TargetingConditions::for_non_combat()
                .range(8.0)
                .ignore_line_of_sight(),
        }
    }

    /// `getFreePartner()`.
    fn get_free_partner(&self, cx: &mut MobAi<M>) -> Option<i32> {
        let area = cx.mob.bounding_box().inflate(8.0, 8.0, 8.0);
        let candidates = nearby_entities(cx, self.partner_class, &self.partner_targeting, &area);
        let mut best_d = f64::MAX;
        let mut best = None;
        for id in candidates {
            if !cx.mob.can_mate_with(id) {
                continue;
            }
            let Some(f) = cx.mob.entity(id) else {
                continue;
            };
            if f.panicking {
                continue;
            }
            let d = cx.mob.position().distance_to_sqr3(f.x, f.y, f.z);
            if d < best_d {
                best = Some(id);
                best_d = d;
            }
        }
        best
    }

    /// `breed()`.
    fn breed(&self, cx: &mut MobAi<M>) {
        if let Some(p) = self.partner {
            cx.mob.spawn_child_from_breeding(p);
        }
    }

    /// The partner, for tests.
    #[must_use]
    pub fn partner(&self) -> Option<i32> {
        self.partner
    }
}

impl<M: MobHost> Goal<MobAi<M>> for BreedGoal<M> {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        if !cx.mob.is_in_love() {
            return false;
        }
        self.partner = self.get_free_partner(cx);
        self.partner.is_some()
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        let Some(p) = self.partner.and_then(|id| cx.mob.entity(id)) else {
            return false;
        };
        p.alive && p.in_love && self.love_time < 60 && !p.panicking
    }

    fn stop(&mut self, _cx: &mut MobAi<M>) {
        self.partner = None;
        self.love_time = 0;
    }

    fn tick(&mut self, cx: &mut MobAi<M>) {
        let Some(p) = self.partner.and_then(|id| cx.mob.entity(id)) else {
            return;
        };
        #[allow(clippy::cast_precision_loss)]
        cx.look_control
            .set_look_at_with(p.x, p.eye_y, p.z, 10.0, cx.mob.max_head_x_rot() as f32);
        cx.navigation
            .move_to_entity_at(&mut cx.mob, p.block_position(), self.speed_modifier);
        self.love_time += 1;
        if self.love_time >= adjusted_tick_delay(false, 60)
            && cx.mob.position().distance_to_sqr3(p.x, p.y, p.z) < 9.0
        {
            self.breed(cx);
        }
    }

    fn flags(&self) -> FlagSet {
        FlagSet::of2(Flag::Move, Flag::Look)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::EntityFacts;
    use crate::testing::{GridLevel, TestMob};

    /// The nearest mate-able, non-panicking partner in love; breeding
    /// after `adjustedTickDelay(60) = 30` ticks within three blocks.
    #[test]
    fn picks_the_nearest_partner_and_breeds_after_thirty_ticks() {
        let mut g = GridLevel::new();
        g.floor(-20, -20, 20, 20, 63);
        let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, 1);
        let mut near = EntityFacts::living(21, 2.5, 64.0, 0.5);
        near.in_love = true;
        let mut far = EntityFacts::living(22, 5.5, 64.0, 0.5);
        far.in_love = true;
        mob.others = vec![(EntityClass::SameKind, far), (EntityClass::SameKind, near)];
        mob.mates = vec![21, 22];
        let mut ai = MobAi::new(mob);
        let mut goal = BreedGoal::new(1.0);
        assert!(!goal.can_use(&mut ai), "not in love");
        ai.mob.in_love = true;
        assert!(goal.can_use(&mut ai));
        assert_eq!(goal.partner(), Some(21));
        for _ in 0..29 {
            assert!(goal.can_continue_to_use(&mut ai));
            goal.tick(&mut ai);
        }
        assert!(ai.mob.bred_with.is_empty());
        goal.tick(&mut ai);
        assert_eq!(ai.mob.bred_with, vec![21]);
        // A panicking partner ends it.
        ai.mob.others[1].1.panicking = true;
        assert!(!goal.can_continue_to_use(&mut ai));
        goal.stop(&mut ai);
        assert_eq!(goal.partner(), None);
    }
}
