//! `net.minecraft.world.entity.ai.sensing.NearestVisibleLivingEntitySensor`,
//! from the 26.2 jar: the abstract base of `VillagerHostilesSensor` and
//! its siblings. `Sensor()` (scan rate 20).
//!
//! ```text
//! requires (0..10): Set.of(getMemoryToSet(), NEAREST_VISIBLE_LIVING_ENTITIES)
//! doTick (0..17):   brain.setMemory(getMemoryToSet(), getNearestEntity(level, mob))
//! getNearestEntity: brain.getMemory(NEAREST_VISIBLE_LIVING_ENTITIES).flatMap(nv -> nv.findClosest(e -> isMatchingEntity(level, mob, e)))
//! ```
//!
//! `findClosest` asks the subclass predicate first and the line-of-sight
//! memo second, in the sensed order; an absent
//! `NEAREST_VISIBLE_LIVING_ENTITIES` erases the target memory.

use crate::ai::mob_ai::MobAi;
use crate::brain::behaviors::nearest_visible_find_closest;
use crate::brain::memory::{MemoryModuleType, MemoryValue};
use crate::brain::BrainState;
use crate::host::MobHost;

/// The base's `doTick` for a subclass's `getMemoryToSet()` and
/// `isMatchingEntity(level, mob, e)`.
pub fn do_tick_with<M: MobHost>(
    brain: &mut BrainState,
    cx: &mut MobAi<M>,
    memory_to_set: MemoryModuleType,
    is_matching: impl FnMut(&mut MobAi<M>, i32) -> bool,
) {
    let nearest = nearest_visible_find_closest(brain, cx, is_matching);
    brain.set_memory(memory_to_set, nearest.map(MemoryValue::Entity));
}

/// The base's `requires()`.
#[must_use]
pub fn requires_with(memory_to_set: MemoryModuleType) -> Vec<MemoryModuleType> {
    vec![
        memory_to_set,
        MemoryModuleType::NearestVisibleLivingEntities,
    ]
}
