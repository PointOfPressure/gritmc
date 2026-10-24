//! `net.minecraft.world.entity.ai.behavior.GiveGiftToHero`, from the 26.2
//! jar. `Behavior<Villager>` with entry `{WALK_TARGET: REGISTERED,
//! LOOK_TARGET: REGISTERED, INTERACTION_TARGET: REGISTERED,
//! NEAREST_VISIBLE_PLAYER: VALUE_PRESENT}` and a fixed duration.
//! `THROW_GIFT_AT_DISTANCE = 5`, `MIN_TIME_BETWEEN_GIFTS = 600`,
//! `MAX_TIME_BETWEEN_GIFTS = 6600`, `TIME_TO_DELAY_FOR_HEAD_TO_FINISH_TURNING
//! = 20`, `SPEED_MODIFIER = 0.5f`; `timeUntilNextGift` starts at 600.
//!
//! ```text
//! checkExtraStartConditions (0..30): if !isHeroVisible: false; if timeUntilNextGift > 0: timeUntilNextGift--; false; true
//! start (0..38): giftGivenDuringThisRun = false; timeSinceStart = time; hero = getNearestTargetableHero().get()
//!                INTERACTION_TARGET = hero; BehaviorUtils.lookAtEntity(villager, hero)
//! canStillUse (0..20): isHeroVisible && !giftGivenDuringThisRun
//! tick (0..67): hero; lookAtEntity(villager, hero)
//!   if isWithinThrowingDistance(hero): if time - timeSinceStart > 20: throwGift(hero); giftGivenDuringThisRun = true
//!   else: setWalkAndLookTargetMemories(villager, hero, 0.5f, 5)
//! stop (0..38): timeUntilNextGift = 600 + level.getRandom().nextInt(6001); erase INTERACTION_TARGET, WALK_TARGET, LOOK_TARGET   <- LEVEL random
//! throwGift: villager.dropFromGiftLootTable(level, getLootTableToThrow(villager), (l, stack) -> throwItem(villager, stack, hero.position()))
//! getLootTableToThrow: baby -> BABY_VILLAGER_GIFT; profession key absent -> UNEMPLOYED_GIFT; else GIFTS.getOrDefault(key, UNEMPLOYED_GIFT)
//! GIFTS: armorer, butcher, cartographer, cleric, farmer, fisherman, fletcher, leatherworker, librarian, mason, shepherd, toolsmith, weaponsmith -> <profession>_gift
//! isHeroVisible: getNearestTargetableHero().isPresent();  getNearestTargetableHero: NEAREST_VISIBLE_PLAYER.filter(hasEffect(HERO_OF_THE_VILLAGE))
//! isWithinThrowingDistance: villager.blockPosition().closerThan(hero.blockPosition(), 5.0)
//! ```

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::behaviors::behavior_utils::{
    look_at_entity, set_walk_and_look_target_memories_to_entity,
};
use crate::brain::memory::{MemoryModuleType, MemoryStatus, MemoryValue};
use crate::brain::BrainState;
use crate::host::MobHost;
use crate::phys::{dist_sqr, Vec3};

/// `THROW_GIFT_AT_DISTANCE`.
pub const THROW_GIFT_AT_DISTANCE: i32 = 5;
/// `MIN_TIME_BETWEEN_GIFTS`.
pub const MIN_TIME_BETWEEN_GIFTS: i32 = 600;
/// `MAX_TIME_BETWEEN_GIFTS`.
pub const MAX_TIME_BETWEEN_GIFTS: i32 = 6600;
/// `TIME_TO_DELAY_FOR_HEAD_TO_FINISH_TURNING`.
pub const TIME_TO_DELAY_FOR_HEAD_TO_FINISH_TURNING: i64 = 20;
/// `SPEED_MODIFIER`.
pub const SPEED_MODIFIER: f32 = 0.5;

/// `GIFTS`: the professions with their own gift table, in put order.
pub const GIFTS: [&str; 13] = [
    "armorer",
    "butcher",
    "cartographer",
    "cleric",
    "farmer",
    "fisherman",
    "fletcher",
    "leatherworker",
    "librarian",
    "mason",
    "shepherd",
    "toolsmith",
    "weaponsmith",
];

/// `GiveGiftToHero`'s body; build one with [`GiveGiftToHero::new`].
#[derive(Debug, Clone, Copy)]
pub struct GiveGiftToHero {
    pub time_until_next_gift: i32,
    pub gift_given_during_this_run: bool,
    pub time_since_start: i64,
}

impl GiveGiftToHero {
    /// `GiveGiftToHero(int)`.
    #[must_use]
    pub fn new(duration: i32) -> Behavior<GiveGiftToHero> {
        Behavior::with_duration(
            GiveGiftToHero {
                time_until_next_gift: MIN_TIME_BETWEEN_GIFTS,
                gift_given_during_this_run: false,
                time_since_start: 0,
            },
            vec![
                (MemoryModuleType::WalkTarget, MemoryStatus::Registered),
                (MemoryModuleType::LookTarget, MemoryStatus::Registered),
                (
                    MemoryModuleType::InteractionTarget,
                    MemoryStatus::Registered,
                ),
                (
                    MemoryModuleType::NearestVisiblePlayer,
                    MemoryStatus::ValuePresent,
                ),
            ],
            duration,
        )
    }

    /// `getLootTableToThrow(villager)`, by `BuiltInLootTables` constant name.
    pub fn loot_table_to_throw<M: MobHost>(cx: &MobAi<M>) -> &'static str {
        if cx.mob.is_baby() {
            return "baby_villager_gift";
        }
        let profession = cx.mob.villager_data().profession;
        GIFTS
            .iter()
            .find(|p| **p == profession)
            .map_or("unemployed_gift", |p| match *p {
                "armorer" => "armorer_gift",
                "butcher" => "butcher_gift",
                "cartographer" => "cartographer_gift",
                "cleric" => "cleric_gift",
                "farmer" => "farmer_gift",
                "fisherman" => "fisherman_gift",
                "fletcher" => "fletcher_gift",
                "leatherworker" => "leatherworker_gift",
                "librarian" => "librarian_gift",
                "mason" => "mason_gift",
                "shepherd" => "shepherd_gift",
                "toolsmith" => "toolsmith_gift",
                _ => "weaponsmith_gift",
            })
    }

    /// `getNearestTargetableHero(villager)`.
    fn nearest_targetable_hero<M: MobHost>(brain: &BrainState, cx: &MobAi<M>) -> Option<i32> {
        brain
            .get_memory(MemoryModuleType::NearestVisiblePlayer)
            .and_then(MemoryValue::as_entity)
            .filter(|p| cx.mob.has_hero_of_the_village(*p))
    }

    fn is_within_throwing_distance<M: MobHost>(cx: &MobAi<M>, hero: i32) -> bool {
        let Some(h) = cx.mob.entity(hero) else {
            return false;
        };
        let d = f64::from(THROW_GIFT_AT_DISTANCE);
        dist_sqr(cx.mob.block_position(), h.block_position()) < d * d
    }
}

impl<M: MobHost> BehaviorBody<M> for GiveGiftToHero {
    fn check_extra_start_conditions(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        if Self::nearest_targetable_hero(brain, cx).is_none() {
            return false;
        }
        if self.time_until_next_gift > 0 {
            self.time_until_next_gift -= 1;
            return false;
        }
        true
    }

    fn start(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        self.gift_given_during_this_run = false;
        self.time_since_start = time;
        let hero = Self::nearest_targetable_hero(brain, cx).expect("a hero is visible");
        brain.set_memory_value(
            MemoryModuleType::InteractionTarget,
            MemoryValue::Entity(hero),
        );
        look_at_entity(brain, hero);
    }

    fn can_still_use(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        Self::nearest_targetable_hero(brain, cx).is_some() && !self.gift_given_during_this_run
    }

    fn tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        let hero = Self::nearest_targetable_hero(brain, cx).expect("a hero is visible");
        look_at_entity(brain, hero);
        if Self::is_within_throwing_distance(cx, hero) {
            if time - self.time_since_start > TIME_TO_DELAY_FOR_HEAD_TO_FINISH_TURNING {
                let table = Self::loot_table_to_throw(cx);
                if let Some(h) = cx.mob.entity(hero) {
                    cx.mob
                        .drop_from_gift_loot_table(table, Vec3::new(h.x, h.y, h.z));
                }
                self.gift_given_during_this_run = true;
            }
        } else {
            set_walk_and_look_target_memories_to_entity(
                brain,
                hero,
                SPEED_MODIFIER,
                THROW_GIFT_AT_DISTANCE,
            );
        }
    }

    fn stop(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        self.time_until_next_gift = MIN_TIME_BETWEEN_GIFTS
            + cx.mob
                .level_random_next_int(MAX_TIME_BETWEEN_GIFTS - MIN_TIME_BETWEEN_GIFTS + 1);
        brain.erase_memory(MemoryModuleType::InteractionTarget);
        brain.erase_memory(MemoryModuleType::WalkTarget);
        brain.erase_memory(MemoryModuleType::LookTarget);
    }

    fn name(&self) -> &'static str {
        "GiveGiftToHero"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::{BehaviorControl, Status};
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};
    use rustmc_worldgen::rng::{LegacyRandomSource, RandomSource};

    #[test]
    fn counts_down_600_then_walks_to_the_hero_and_throws_after_20_ticks_in_range() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 2));
        let mut twin = LegacyRandomSource::new(2 ^ 0x5DEECE66D);
        cx.mob.others = vec![(EntityClass::Player, EntityFacts::living(9, 8.5, 1.0, 0.5))];
        cx.mob.heroes = vec![9];
        cx.mob.villager_data.profession = "cleric";
        let mut brain = Brain::<TestMob>::new(
            &[
                T::WalkTarget,
                T::LookTarget,
                T::InteractionTarget,
                T::NearestVisiblePlayer,
            ],
            vec![],
        )
        .state;
        brain.set_memory_value(T::NearestVisiblePlayer, MemoryValue::Entity(9));
        let mut b = GiveGiftToHero::new(100);
        for _ in 0..600 {
            assert!(!b.try_start(&mut brain, &mut cx, 0));
        }
        assert_eq!(b.body.time_until_next_gift, 0);
        assert!(b.try_start(&mut brain, &mut cx, 1000));
        let _ = twin.next_i32_bound(1);
        assert_eq!(
            brain.get_memory(T::InteractionTarget),
            Some(&MemoryValue::Entity(9))
        );
        b.tick_or_stop(&mut brain, &mut cx, 1001);
        assert!(brain.has_memory_value(T::WalkTarget), "8 blocks away: walk");
        cx.mob.others[0].1.x = 3.5;
        b.tick_or_stop(&mut brain, &mut cx, 1020);
        assert!(
            cx.mob.calls.is_empty(),
            "in range but the head is still turning"
        );
        b.tick_or_stop(&mut brain, &mut cx, 1021);
        assert_eq!(cx.mob.calls, ["gift cleric_gift at 3.5 1 0.5"]);
        b.tick_or_stop(&mut brain, &mut cx, 1022);
        assert_eq!(
            BehaviorControl::<TestMob>::status(&b),
            Status::Stopped,
            "gift given"
        );
        assert_eq!(b.body.time_until_next_gift, 600 + twin.next_i32_bound(6001));
        assert!(!brain.has_memory_value(T::InteractionTarget));
        cx.mob.baby = true;
        assert_eq!(
            GiveGiftToHero::loot_table_to_throw(&cx),
            "baby_villager_gift"
        );
        cx.mob.baby = false;
        cx.mob.villager_data.profession = "nitwit";
        assert_eq!(GiveGiftToHero::loot_table_to_throw(&cx), "unemployed_gift");
    }
}
