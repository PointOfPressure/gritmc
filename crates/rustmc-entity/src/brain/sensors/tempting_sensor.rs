//! `net.minecraft.world.entity.ai.sensing.TemptingSensor`, from the 26.2
//! jar. `Sensor()` (scan rate 20); `requires()` = `{TEMPTING_PLAYER}`.
//! Three `SensorType`s create one: `FOOD_TEMPTATIONS` is the
//! `TemptingSensor::forAnimal` method reference, `FROG_TEMPTATIONS` and
//! `NAUTILUS_TEMPTATIONS` are `new TemptingSensor(FrogAi.getTemptations())`
//! and `new TemptingSensor(NautilusAi.getTemptations())`
//! (`SensorType.<clinit>` 234..257 and its `lambda$static$1` / `$2`).
//!
//! ```text
//! TEMPT_TARGETING = TargetingConditions.forNonCombat().ignoreLineOfSight()    <clinit> 0..6
//! doTick (0..153):
//!   conditions = TEMPT_TARGETING.copy().range((double)(float) mob.getAttributeValue(TEMPT_RANGE))   <- d2f;f2d at 18,19
//!   list = level.players().stream()
//!            .filter(EntitySelector.NO_SPECTATORS)
//!            .filter(p -> conditions.test(level, mob, p))
//!            .filter(p -> playerHoldingTemptation(mob, p))
//!            .filter(p -> !mob.hasPassenger(p))
//!            .sorted(Comparator.comparingDouble(mob::distanceToSqr))
//!            .collect(toList())
//!   if !list.isEmpty(): brain.setMemory(TEMPTING_PLAYER, list.get(0))
//!   else:               brain.eraseMemory(TEMPTING_PLAYER)
//! playerHoldingTemptation (0..29): isTemptation(mob, p.getMainHandItem()) || isTemptation(mob, p.getOffhandItem())
//! forAnimal lambda (0..19):        mob instanceof Animal a && a.isFood(stack)
//! FrogAi.getTemptations lambda:    stack.is(ItemTags.FROG_FOOD)
//! NautilusAi.getTemptations lambda: stack.is(ItemTags.NAUTILUS_FOOD)
//! ```
//!
//! **The range is truncated through a float.** Bytecode 15..20 is
//! `getAttributeValue(TEMPT_RANGE); d2f; f2d; range(D)`, so a tempt range
//! that is not exactly representable as an `f32` is rounded before it
//! becomes the radius. Reproduced as `f64::from(range as f32)`; nothing
//! else in the sensor package does this.
//!
//! The four filters run in the order above and each is applied to the
//! whole stream before the next in a `Stream` pipeline only nominally —
//! `filter` is lazy, so per element the four predicates run left to right
//! and the first `false` short-circuits the rest. That is what this port
//! does, and it matters because `conditions.test` is the only one with a
//! side effect (none in vanilla) and because `playerHoldingTemptation`
//! reads two item slots.
//!
//! `sorted` is `Stream.sorted`, a **stable** merge sort, so players at
//! equal `distanceToSqr` keep `level.players()` order — which is what
//! decides the memory when two players hold the same food at the same
//! distance.

use crate::ai::mob_ai::MobAi;
use crate::brain::memory::{MemoryModuleType, MemoryValue};
use crate::brain::sensor::{Sensor, SensorBody, SensorType};
use crate::brain::BrainState;
use crate::host::{MobHost, Temptations};

/// `TemptingSensor`, with the `SensorType` it was created from (the three
/// differ only in the `temptations` `BiPredicate`).
#[derive(Debug, Clone, Copy)]
pub struct TemptingSensor {
    /// `temptations`.
    pub temptations: Temptations,
    ty: SensorType,
}

impl TemptingSensor {
    /// `SensorType.FOOD_TEMPTATIONS.create()` — `TemptingSensor.forAnimal()`.
    #[must_use]
    pub fn for_animal() -> Sensor<TemptingSensor> {
        Sensor::new(TemptingSensor {
            temptations: Temptations::AnimalFood,
            ty: SensorType::FoodTemptations,
        })
    }

    /// `SensorType.FROG_TEMPTATIONS.create()`.
    #[must_use]
    pub fn frog() -> Sensor<TemptingSensor> {
        Sensor::new(TemptingSensor {
            temptations: Temptations::ItemTag("frog_food"),
            ty: SensorType::FrogTemptations,
        })
    }

    /// `SensorType.NAUTILUS_TEMPTATIONS.create()`.
    #[must_use]
    pub fn nautilus() -> Sensor<TemptingSensor> {
        Sensor::new(TemptingSensor {
            temptations: Temptations::ItemTag("nautilus_food"),
            ty: SensorType::NautilusTemptations,
        })
    }
}

impl<M: MobHost> SensorBody<M> for TemptingSensor {
    fn do_tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) {
        #[allow(clippy::cast_possible_truncation)]
        let range = f64::from(cx.mob.tempt_range() as f32);
        let conditions = crate::ai::targeting::TargetingConditions::<M>::for_non_combat()
            .ignore_line_of_sight()
            .range(range);
        let temptations = self.temptations;
        let mut passing: Vec<(f64, i32)> = Vec::new();
        for id in cx.mob.players() {
            let Some(facts) = cx.mob.entity(id) else {
                continue;
            };
            if facts.is_spectator {
                continue;
            }
            if !conditions.test(cx, &facts) {
                continue;
            }
            if !cx.mob.player_holding_temptation(id, temptations) {
                continue;
            }
            if cx.mob.has_passenger(id) {
                continue;
            }
            let d = facts.distance_to_sqr(cx.mob.x(), cx.mob.y(), cx.mob.z());
            passing.push((d, id));
        }
        // `Stream.sorted` is a stable merge sort: ties keep `players()` order.
        passing.sort_by(|a, b| a.0.total_cmp(&b.0));
        match passing.first() {
            Some((_, id)) => {
                brain.set_memory_value(MemoryModuleType::TemptingPlayer, MemoryValue::Entity(*id));
            }
            None => brain.erase_memory(MemoryModuleType::TemptingPlayer),
        }
    }

    fn requires(&self) -> Vec<MemoryModuleType> {
        vec![MemoryModuleType::TemptingPlayer]
    }

    fn sensor_type(&self) -> SensorType {
        self.ty
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    fn player(id: i32, x: f64) -> EntityFacts {
        let mut f = EntityFacts::living(id, x, 1.0, 0.5);
        f.is_player = true;
        f
    }

    #[test]
    fn takes_the_closest_holder_and_ties_keep_the_player_list_order() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        cx.mob.others = vec![
            (EntityClass::Player, player(2, 4.5)),
            (EntityClass::Player, player(3, 2.5)),
            (EntityClass::Player, player(4, 2.5)),
        ];
        cx.mob.temptation_holders = vec![2, 3, 4];
        let mut brain = Brain::<TestMob>::new(&[T::TemptingPlayer], vec![]).state;
        let mut s = TemptingSensor {
            temptations: Temptations::ItemTag("frog_food"),
            ty: SensorType::FrogTemptations,
        };
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert_eq!(
            brain.get_memory(T::TemptingPlayer),
            Some(&MemoryValue::Entity(3)),
            "3 and 4 tie at 2.0; the stable sort keeps players() order"
        );
        // Only the far one holds food: it wins despite the distance.
        cx.mob.temptation_holders = vec![2];
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert_eq!(
            brain.get_memory(T::TemptingPlayer),
            Some(&MemoryValue::Entity(2))
        );
        // Nobody holds food: the else branch erases.
        cx.mob.temptation_holders.clear();
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert!(!brain.has_memory_value(T::TemptingPlayer));
    }

    #[test]
    fn spectators_passengers_and_the_range_all_drop_a_holder() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut spectator = player(2, 1.5);
        spectator.is_spectator = true;
        cx.mob.others = vec![
            (EntityClass::Player, spectator),
            (EntityClass::Player, player(3, 2.5)),
            (EntityClass::Player, player(4, 3.5)),
        ];
        cx.mob.temptation_holders = vec![2, 3, 4];
        cx.mob.passengers = vec![3];
        cx.mob.tempt_range = 10.0;
        let mut brain = Brain::<TestMob>::new(&[T::TemptingPlayer], vec![]).state;
        let mut s = TemptingSensor {
            temptations: Temptations::AnimalFood,
            ty: SensorType::FoodTemptations,
        };
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert_eq!(
            brain.get_memory(T::TemptingPlayer),
            Some(&MemoryValue::Entity(4)),
            "2 is a spectator, 3 is a passenger"
        );
        // The range comes from TEMPT_RANGE, truncated through a float.
        cx.mob.tempt_range = 1.0;
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert!(
            !brain.has_memory_value(T::TemptingPlayer),
            "3.0 > range 1.0"
        );
        assert_eq!(SensorBody::<TestMob>::requires(&s), vec![T::TemptingPlayer]);
    }
}
