//! `net.minecraft.world.entity.ai.sensing.PiglinSpecificSensor`, from the
//! 26.2 jar. `Sensor()` (scan rate 20); `requires()` lists twelve memories
//! in the order below.
//!
//! ```text
//! doTick (0..540):
//!   brain.setMemory(NEAREST_REPELLENT, findNearestRepellent(level, mob))   <- BlockPos.findClosestMatch(pos, 8, 4, isValidRepellent)
//!   nemesis = huntable = babyHoglin = babyPiglin = zombified = playerNotWearingGold = playerHoldingLoved = empty
//!   adultHoglinCount = 0; adultPiglins = []
//!   for e in (NEAREST_VISIBLE_LIVING_ENTITIES or EMPTY).findAll(e -> true):   <- the memo is still filled
//!     if e instanceof Hoglin:
//!       if e.isBaby() && babyHoglin.isEmpty(): babyHoglin = e
//!       else if e.isAdult(): adultHoglinCount++; if huntable.isEmpty() && e.canBeHunted(): huntable = e
//!     else if e instanceof PiglinBrute: adultPiglins.add(e)
//!     else if e instanceof Piglin:
//!       if e.isBaby() && babyPiglin.isEmpty(): babyPiglin = e
//!       else if e.isAdult(): adultPiglins.add(e)
//!     else if e instanceof Player:
//!       if playerNotWearingGold.isEmpty() && !isWearingSafeArmor(e) && mob.canAttack(e): playerNotWearingGold = e
//!       if playerHoldingLoved.isEmpty() && !e.isSpectator() && isPlayerHoldingLovedItem(e): playerHoldingLoved = e
//!     else if nemesis.isEmpty() && (e instanceof WitherSkeleton || e instanceof WitherBoss): nemesis = e
//!     else if zombified.isEmpty() && PiglinAi.isZombified(e): zombified = e
//!   nearby = PiglinAi.findNearbyAdultPiglins(brain)      <- NEAREST_LIVING_ENTITIES filtered AbstractPiglin && isAdult
//!   setMemory NEAREST_VISIBLE_NEMESIS, NEAREST_VISIBLE_HUNTABLE_HOGLIN, NEAREST_VISIBLE_BABY_HOGLIN,
//!             NEAREST_VISIBLE_ZOMBIFIED, NEAREST_TARGETABLE_PLAYER_NOT_WEARING_GOLD, NEAREST_PLAYER_HOLDING_WANTED_ITEM,
//!             NEARBY_ADULT_PIGLINS, NEAREST_VISIBLE_ADULT_PIGLINS, VISIBLE_ADULT_PIGLIN_COUNT, VISIBLE_ADULT_HOGLIN_COUNT
//! ```
//!
//! No random draw. `isValidRepellent` (BlockTags.PIGLIN_REPELLENTS, a soul
//! campfire only when lit) is the host's; `isWearingSafeArmor` and
//! `isPlayerHoldingLovedItem` (`ItemTags.PIGLIN_SAFE_ARMOR`,
//! `ItemTags.PIGLIN_LOVED`) likewise. Entity classes are asked by registry
//! kind: `hoglin`, `piglin_brute`, `piglin`, the player flag,
//! `wither_skeleton`, `wither`, `zombified_piglin`, `zoglin`.

use crate::ai::mob_ai::MobAi;
use crate::brain::behaviors::nearest_visible_find_all;
use crate::brain::memory::{MemoryModuleType, MemoryValue};
use crate::brain::sensor::{Sensor, SensorBody, SensorType};
use crate::brain::BrainState;
use crate::host::MobHost;
use crate::phys::find_closest_match;
use rustmc_data::BlockPos;

/// `PiglinSpecificSensor`.
#[derive(Debug, Clone, Copy, Default)]
pub struct PiglinSpecificSensor;

impl PiglinSpecificSensor {
    /// `SensorType.PIGLIN_SPECIFIC_SENSOR.create()`.
    #[must_use]
    pub fn new() -> Sensor<PiglinSpecificSensor> {
        Sensor::new(PiglinSpecificSensor)
    }

    /// `findNearestRepellent(level, mob)`.
    pub fn find_nearest_repellent<M: MobHost>(cx: &MobAi<M>) -> Option<BlockPos> {
        find_closest_match(cx.mob.block_position(), 8, 4, |p| {
            cx.mob.is_piglin_repellent_at(p)
        })
    }

    /// `PiglinAi.isZombified(e)`.
    pub fn is_zombified<M: MobHost>(cx: &MobAi<M>, id: i32) -> bool {
        cx.mob.entity_kind_is(id, "zombified_piglin") || cx.mob.entity_kind_is(id, "zoglin")
    }

    /// `PiglinAi.findNearbyAdultPiglins(brain)`.
    pub fn find_nearby_adult_piglins<M: MobHost>(brain: &BrainState, cx: &MobAi<M>) -> Vec<i32> {
        brain
            .get_memory(MemoryModuleType::NearestLivingEntities)
            .and_then(MemoryValue::as_entities)
            .map(|ids| {
                ids.iter()
                    .copied()
                    .filter(|id| {
                        (cx.mob.entity_kind_is(*id, "piglin")
                            || cx.mob.entity_kind_is(*id, "piglin_brute"))
                            && is_adult(cx, *id)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

fn is_baby<M: MobHost>(cx: &MobAi<M>, id: i32) -> bool {
    cx.mob.entity(id).is_some_and(|e| e.age < 0)
}

fn is_adult<M: MobHost>(cx: &MobAi<M>, id: i32) -> bool {
    !is_baby(cx, id)
}

impl<M: MobHost> SensorBody<M> for PiglinSpecificSensor {
    fn do_tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) {
        let repellent = Self::find_nearest_repellent(cx);
        brain.set_memory(
            MemoryModuleType::NearestRepellent,
            repellent.map(MemoryValue::BlockPos),
        );
        let mut nemesis = None;
        let mut huntable = None;
        let mut baby_hoglin = None;
        let mut baby_piglin = None;
        let mut zombified = None;
        let mut player_not_wearing_gold = None;
        let mut player_holding_loved = None;
        let mut adult_hoglin_count = 0;
        let mut adult_piglins: Vec<i32> = Vec::new();
        for e in nearest_visible_find_all(brain, cx, |_, _| true) {
            if cx.mob.entity_kind_is(e, "hoglin") {
                if is_baby(cx, e) && baby_hoglin.is_none() {
                    baby_hoglin = Some(e);
                } else if is_adult(cx, e) {
                    adult_hoglin_count += 1;
                    if huntable.is_none() && cx.mob.hoglin_can_be_hunted(e) {
                        huntable = Some(e);
                    }
                }
            } else if cx.mob.entity_kind_is(e, "piglin_brute") {
                adult_piglins.push(e);
            } else if cx.mob.entity_kind_is(e, "piglin") {
                if is_baby(cx, e) && baby_piglin.is_none() {
                    baby_piglin = Some(e);
                } else if is_adult(cx, e) {
                    adult_piglins.push(e);
                }
            } else if let Some(facts) = cx.mob.entity(e).filter(|f| f.is_player) {
                if player_not_wearing_gold.is_none()
                    && !cx.mob.is_wearing_safe_armor(e)
                    && cx.mob.can_attack(&facts)
                {
                    player_not_wearing_gold = Some(e);
                }
                if player_holding_loved.is_none()
                    && !facts.is_spectator
                    && cx.mob.is_holding_loved_item(e)
                {
                    player_holding_loved = Some(e);
                }
            } else if nemesis.is_none()
                && (cx.mob.entity_kind_is(e, "wither_skeleton")
                    || cx.mob.entity_kind_is(e, "wither"))
            {
                nemesis = Some(e);
            } else if zombified.is_none() && Self::is_zombified(cx, e) {
                zombified = Some(e);
            }
        }
        let nearby = Self::find_nearby_adult_piglins(brain, cx);
        brain.set_memory(
            MemoryModuleType::NearestVisibleNemesis,
            nemesis.map(MemoryValue::Entity),
        );
        brain.set_memory(
            MemoryModuleType::NearestVisibleHuntableHoglin,
            huntable.map(MemoryValue::Entity),
        );
        brain.set_memory(
            MemoryModuleType::NearestVisibleBabyHoglin,
            baby_hoglin.map(MemoryValue::Entity),
        );
        brain.set_memory(
            MemoryModuleType::NearestVisibleZombified,
            zombified.map(MemoryValue::Entity),
        );
        brain.set_memory(
            MemoryModuleType::NearestTargetablePlayerNotWearingGold,
            player_not_wearing_gold.map(MemoryValue::Entity),
        );
        brain.set_memory(
            MemoryModuleType::NearestPlayerHoldingWantedItem,
            player_holding_loved.map(MemoryValue::Entity),
        );
        brain.set_memory_value(
            MemoryModuleType::NearbyAdultPiglins,
            MemoryValue::Entities(nearby),
        );
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let piglin_count = adult_piglins.len() as i32;
        brain.set_memory_value(
            MemoryModuleType::NearestVisibleAdultPiglins,
            MemoryValue::Entities(adult_piglins),
        );
        brain.set_memory_value(
            MemoryModuleType::VisibleAdultPiglinCount,
            MemoryValue::Int(piglin_count),
        );
        brain.set_memory_value(
            MemoryModuleType::VisibleAdultHoglinCount,
            MemoryValue::Int(adult_hoglin_count),
        );
        let _ = baby_piglin;
    }

    fn requires(&self) -> Vec<MemoryModuleType> {
        vec![
            MemoryModuleType::NearestVisibleLivingEntities,
            MemoryModuleType::NearestLivingEntities,
            MemoryModuleType::NearestVisibleNemesis,
            MemoryModuleType::NearestTargetablePlayerNotWearingGold,
            MemoryModuleType::NearestPlayerHoldingWantedItem,
            MemoryModuleType::NearestVisibleHuntableHoglin,
            MemoryModuleType::NearestVisibleBabyHoglin,
            MemoryModuleType::NearestVisibleAdultPiglins,
            MemoryModuleType::NearbyAdultPiglins,
            MemoryModuleType::VisibleAdultPiglinCount,
            MemoryModuleType::VisibleAdultHoglinCount,
            MemoryModuleType::NearestRepellent,
        ]
    }

    fn sensor_type(&self) -> SensorType {
        SensorType::PiglinSpecificSensor
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::memory::{MemoryModuleType as T, NearestVisibleLivingEntities};
    use crate::brain::Brain;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn sorts_the_visible_crowd_into_the_piglin_memories() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut e = |id: i32, x: f64, age: i32, player: bool| {
            let mut f = EntityFacts::living(id, x, 1.0, 0.5);
            f.age = age;
            f.is_player = player;
            cx.mob.others.push((
                if player {
                    EntityClass::Player
                } else {
                    EntityClass::Living
                },
                f,
            ));
        };
        e(2, 2.5, -1, false); // baby hoglin
        e(3, 3.5, 0, false); // adult hoglin, huntable
        e(4, 4.5, 0, false); // adult hoglin, not huntable
        e(5, 5.5, 0, false); // piglin brute
        e(6, 6.5, -1, false); // baby piglin
        e(7, 7.5, 0, false); // adult piglin
        e(8, 8.5, 0, true); // player, no gold, holding loved
        e(9, 9.5, 0, false); // wither skeleton
        e(10, 10.5, 0, false); // zoglin
        e(11, 11.5, 0, false); // adult piglin, sensed but unseen
        cx.mob.entity_kinds = vec![
            (2, "hoglin"),
            (3, "hoglin"),
            (4, "hoglin"),
            (5, "piglin_brute"),
            (6, "piglin"),
            (7, "piglin"),
            (9, "wither_skeleton"),
            (10, "zoglin"),
            (11, "piglin"),
        ];
        cx.mob.huntable_hoglins = vec![3];
        cx.mob.loved_item_holders = vec![8];
        cx.mob.visible = (2..=10).collect();
        cx.mob.repellents = vec![BlockPos { x: 3, y: 1, z: 0 }, BlockPos { x: 0, y: 2, z: 0 }];
        let all: Vec<T> = {
            let mut v = SensorBody::<TestMob>::requires(&PiglinSpecificSensor);
            v.push(T::NearestVisibleZombified);
            v
        };
        let mut brain = Brain::<TestMob>::new(&all, vec![]).state;
        brain.set_memory_value(
            T::NearestLivingEntities,
            MemoryValue::Entities((2..=11).collect()),
        );
        brain.set_memory_value(
            T::NearestVisibleLivingEntities,
            MemoryValue::NearestVisible(NearestVisibleLivingEntities::new((2..=11).collect())),
        );
        let mut s = PiglinSpecificSensor;
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        let get = |t| brain.get_memory(t).cloned();
        assert_eq!(
            get(T::NearestRepellent),
            Some(MemoryValue::BlockPos(BlockPos { x: 0, y: 2, z: 0 })),
            "withinManhattan walks the origin column first"
        );
        assert_eq!(get(T::NearestVisibleNemesis), Some(MemoryValue::Entity(9)));
        assert_eq!(
            get(T::NearestVisibleHuntableHoglin),
            Some(MemoryValue::Entity(3))
        );
        assert_eq!(
            get(T::NearestVisibleBabyHoglin),
            Some(MemoryValue::Entity(2))
        );
        assert_eq!(
            get(T::NearestVisibleZombified),
            Some(MemoryValue::Entity(10))
        );
        assert_eq!(
            get(T::NearestTargetablePlayerNotWearingGold),
            Some(MemoryValue::Entity(8))
        );
        assert_eq!(
            get(T::NearestPlayerHoldingWantedItem),
            Some(MemoryValue::Entity(8))
        );
        assert_eq!(
            get(T::NearbyAdultPiglins),
            Some(MemoryValue::Entities(vec![5, 7, 11])),
            "sensed, not seen"
        );
        assert_eq!(
            get(T::NearestVisibleAdultPiglins),
            Some(MemoryValue::Entities(vec![5, 7]))
        );
        assert_eq!(get(T::VisibleAdultPiglinCount), Some(MemoryValue::Int(2)));
        assert_eq!(get(T::VisibleAdultHoglinCount), Some(MemoryValue::Int(2)));
        cx.mob.safe_armor = vec![8];
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert!(!brain.has_memory_value(T::NearestTargetablePlayerNotWearingGold));
    }
}
