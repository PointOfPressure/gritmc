//! `net.minecraft.world.entity.ai.sensing.VillagerBabiesSensor`, from the
//! 26.2 jar. `Sensor()` (scan rate 20); `requires()` =
//! `{VISIBLE_VILLAGER_BABIES}`.
//!
//! ```text
//! doTick (0..15):  brain.setMemory(VISIBLE_VILLAGER_BABIES, getNearestVillagerBabies(mob))
//! getNearestVillagerBabies: ImmutableList.copyOf(getVisibleEntities(mob).findAll(this::isVillagerBaby))
//! isVillagerBaby (0..22): e.is(VILLAGER) && e.isBaby()
//! getVisibleEntities: brain.getMemory(NEAREST_VISIBLE_LIVING_ENTITIES).orElse(NearestVisibleLivingEntities.empty())
//! ```
//!
//! An empty list clears the memory (`Brain.setMemory` erases an empty
//! collection); `EMPTY`'s line-of-sight test is `e -> false`.

use crate::ai::mob_ai::MobAi;
use crate::brain::behaviors::nearest_visible_find_all;
use crate::brain::memory::{MemoryModuleType, MemoryValue};
use crate::brain::sensor::{Sensor, SensorBody, SensorType};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `VillagerBabiesSensor`.
#[derive(Debug, Clone, Copy, Default)]
pub struct VillagerBabiesSensor;

impl VillagerBabiesSensor {
    /// `SensorType.VILLAGER_BABIES.create()`.
    #[must_use]
    pub fn new() -> Sensor<VillagerBabiesSensor> {
        Sensor::new(VillagerBabiesSensor)
    }

    /// `isVillagerBaby(e)`.
    pub fn is_villager_baby<M: MobHost>(cx: &MobAi<M>, id: i32) -> bool {
        cx.mob.entity_kind_is(id, "villager") && cx.mob.entity(id).is_some_and(|e| e.age < 0)
    }
}

impl<M: MobHost> SensorBody<M> for VillagerBabiesSensor {
    fn do_tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) {
        let babies = nearest_visible_find_all(brain, cx, |cx, id| Self::is_villager_baby(cx, id));
        brain.set_memory_value(
            MemoryModuleType::VisibleVillagerBabies,
            MemoryValue::Entities(babies),
        );
    }

    fn requires(&self) -> Vec<MemoryModuleType> {
        vec![MemoryModuleType::VisibleVillagerBabies]
    }

    fn sensor_type(&self) -> SensorType {
        SensorType::VillagerBabies
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
    fn keeps_visible_baby_villagers_in_sensed_order_and_clears_when_none() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut baby = EntityFacts::living(2, 3.5, 1.0, 0.5);
        baby.age = -1000;
        let mut baby2 = EntityFacts::living(3, 5.5, 1.0, 0.5);
        baby2.age = -5;
        let mut adult = EntityFacts::living(4, 2.5, 1.0, 0.5);
        adult.age = 0;
        cx.mob.others = vec![
            (EntityClass::Living, adult),
            (EntityClass::Living, baby2),
            (EntityClass::Living, baby),
        ];
        cx.mob.entity_kinds = vec![(2, "villager"), (3, "villager"), (4, "villager")];
        cx.mob.visible = vec![2, 3, 4];
        let mut brain = Brain::<TestMob>::new(
            &[T::VisibleVillagerBabies, T::NearestVisibleLivingEntities],
            vec![],
        )
        .state;
        brain.set_memory_value(
            T::NearestVisibleLivingEntities,
            MemoryValue::NearestVisible(NearestVisibleLivingEntities::new(vec![4, 3, 2])),
        );
        let mut s = VillagerBabiesSensor;
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert_eq!(
            brain.get_memory(T::VisibleVillagerBabies),
            Some(&MemoryValue::Entities(vec![3, 2]))
        );
        brain.erase_memory(T::NearestVisibleLivingEntities);
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert!(!brain.has_memory_value(T::VisibleVillagerBabies));
    }
}
