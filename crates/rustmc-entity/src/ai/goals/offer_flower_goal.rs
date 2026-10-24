//! `net.minecraft.world.entity.ai.goal.OfferFlowerGoal`, from the 26.2
//! jar. Flags `MOVE, LOOK`. `OFFER_TARGET_CONTEXT = forNonCombat().range(6.0)`,
//! `OFFER_ITEM = POPPY`, `OFFER_TICKS = 400`. Used by IronGolem.
//!
//! `canUse()`, offsets 0..96: `level.isBrightOutside()`, then
//! `random.nextInt(8000) == 0` (offset 25), then `entity =
//! level.getNearestEntity(CANDIDATE_FOR_IRON_GOLEM_GIFT, context, golem,
//! x, y, z, golemBox)` with `golemBox = bb.inflate(6.0, 2.0, 6.0)`.
//!
//! `start()`: `tick = adjustedTickDelay(400)` (200), `golem.offerFlower(true)`.
//! `tick()`: look at the entity `(30, 30)` when there is one; `--tick`.
//! `stop()`: `offerFlower(false)`; when `tick == 0` and the entity is a
//! mob in `ACCEPTS_IRON_GOLEM_GIFT` with an empty antenna whose box
//! intersects the golem box, it is given the poppy; `entity = null`.
//! `canContinueToUse()`: `tick > 0`.

use rustmc_worldgen::rng::RandomSource;

use crate::ai::goal::{adjusted_tick_delay, Flag, FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::ai::targeting::{nearest_entity, TargetingConditions};
use crate::host::{MobHost, PathLevel};
use crate::phys::Aabb;

/// `OFFER_TICKS`.
pub const OFFER_TICKS: i32 = 400;

/// `OfferFlowerGoal`.
pub struct OfferFlowerGoal<M: MobHost> {
    entity: Option<i32>,
    tick: i32,
    offer_target_context: TargetingConditions<M>,
}

impl<M: MobHost> OfferFlowerGoal<M> {
    /// `OfferFlowerGoal(IronGolem)`.
    #[must_use]
    pub fn new() -> Self {
        OfferFlowerGoal {
            entity: None,
            tick: 0,
            offer_target_context: TargetingConditions::for_non_combat().range(6.0),
        }
    }

    /// `getGolemBoundingBox()`.
    fn golem_bounding_box(cx: &MobAi<M>) -> Aabb {
        cx.mob.bounding_box().inflate(6.0, 2.0, 6.0)
    }

    /// The offered-to entity, for tests.
    #[must_use]
    pub fn entity(&self) -> Option<i32> {
        self.entity
    }
}

impl<M: MobHost> Default for OfferFlowerGoal<M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<M: MobHost> Goal<MobAi<M>> for OfferFlowerGoal<M> {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        if !cx.mob.level().is_bright_outside() {
            return false;
        }
        if cx.mob.random().next_i32_bound(8000) != 0 {
            return false;
        }
        let area = Self::golem_bounding_box(cx);
        let ids = cx.mob.iron_golem_gift_candidates_in(&area);
        let (x, y, z) = (cx.mob.x(), cx.mob.y(), cx.mob.z());
        self.entity = nearest_entity(cx, &ids, &self.offer_target_context, x, y, z);
        self.entity.is_some()
    }

    fn can_continue_to_use(&mut self, _cx: &mut MobAi<M>) -> bool {
        self.tick > 0
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        self.tick = adjusted_tick_delay(false, OFFER_TICKS);
        cx.mob.offer_flower(true);
    }

    fn stop(&mut self, cx: &mut MobAi<M>) {
        cx.mob.offer_flower(false);
        if self.tick == 0 {
            if let Some(id) = self.entity {
                if cx.mob.accepts_iron_golem_gift_with_empty_antenna(id) {
                    let intersects = cx
                        .mob
                        .entity(id)
                        .is_some_and(|f| Self::golem_bounding_box(cx).intersects(&f.bb));
                    if intersects {
                        cx.mob.give_iron_golem_gift(id);
                    }
                }
            }
        }
        self.entity = None;
    }

    fn tick(&mut self, cx: &mut MobAi<M>) {
        if let Some(f) = self.entity.and_then(|id| cx.mob.entity(id)) {
            cx.look_control
                .set_look_at_with(f.x, f.eye_y, f.z, 30.0, 30.0);
        }
        self.tick -= 1;
    }

    fn flags(&self) -> FlagSet {
        FlagSet::of2(Flag::Move, Flag::Look)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmc_worldgen::rng::LegacyRandomSource;

    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    /// One `nextInt(8000)` per bright tick; on the zero the nearest
    /// candidate within 6 is offered a flower for 200 ticks, and a
    /// copper golem still touching the box at the end gets the poppy.
    #[test]
    fn offers_on_the_zero_draw_and_gifts_at_the_end() {
        let mut g = GridLevel::new();
        g.bright_outside = true;
        let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, 4);
        mob.others.push((
            EntityClass::Villager,
            EntityFacts::living(3, 3.5, 64.0, 0.5),
        ));
        mob.others
            .push((EntityClass::Mob, EntityFacts::living(5, 2.5, 64.0, 0.5)));
        mob.visible.extend([3, 5]);
        mob.gift_candidates.extend([3, 5]);
        mob.accepts_gift.push(5);
        let mut ai = MobAi::new(mob);
        let mut goal = OfferFlowerGoal::new();
        let mut twin = LegacyRandomSource::new(4);
        let mut misses = 0;
        while twin.next_i32_bound(8000) != 0 {
            misses += 1;
        }
        for _ in 0..misses {
            assert!(!goal.can_use(&mut ai));
        }
        assert!(goal.can_use(&mut ai));
        assert_eq!(goal.entity(), Some(5), "the nearer candidate");
        goal.start(&mut ai);
        for _ in 0..200 {
            assert!(goal.can_continue_to_use(&mut ai));
            goal.tick(&mut ai);
        }
        assert!(!goal.can_continue_to_use(&mut ai));
        goal.stop(&mut ai);
        assert_eq!(
            ai.mob.calls,
            vec!["offerFlower(true)", "offerFlower(false)", "giveGift(5)"]
        );
        assert_eq!(goal.entity(), None);
    }
}
