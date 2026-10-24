//! `net.minecraft.world.entity.ai.behavior.AnimalMakeLove`, from the 26.2
//! jar. `Behavior<Animal>` with entry `{NEAREST_VISIBLE_LIVING_ENTITIES:
//! VALUE_PRESENT, BREED_TARGET: VALUE_ABSENT, WALK_TARGET: REGISTERED,
//! LOOK_TARGET: REGISTERED, IS_PANICKING: VALUE_ABSENT}` and a **fixed**
//! duration of 110 (`super(map, 110)` at ctor 34..36 — `MIN_DURATION = 60`
//! and `MAX_DURATION = 110` are the child-timer bounds, not the
//! behaviour's, and only 60 appears in `start`).
//!
//! ```text
//! AnimalMakeLove(type)                 -> this(type, 1.0f, 2)          (ctor 0..7, DEFAULT_CLOSE_ENOUGH_DISTANCE = 2)
//! checkExtraStartConditions (0..23):   animal.isInLove() && findValidBreedPartner(animal).isPresent()
//! findValidBreedPartner (0..41):       NEAREST_VISIBLE_LIVING_ENTITIES.get()
//!                                        .findClosest(e -> e.is(partnerType) && e instanceof Animal a
//!                                                          && animal.canMate(a) && !a.isPanicking())
//!                                        .map(Animal.class::cast)
//! start (0..76):                       partner = findValidBreedPartner(animal).get()   <- a SECOND findClosest walk
//!                                      animal.brain.BREED_TARGET = partner; partner.brain.BREED_TARGET = animal
//!                                      BehaviorUtils.lockGazeAndWalkToEachOther(animal, partner, speedModifier, closeEnoughDistance)
//!                                      spawnChildAtTime = time + (60 + animal.getRandom().nextInt(50))   <- MOB random, one draw
//! canStillUse (0..75):                 hasBreedTargetOfRightType(animal)
//!                                        && target.isAlive() && animal.canMate(target)
//!                                        && BehaviorUtils.entityIsVisible(animal.brain, target)
//!                                        && time <= spawnChildAtTime
//!                                        && !animal.isPanicking() && !target.isPanicking()
//! tick (0..71):                        target = getBreedTarget(animal)
//!                                      lockGazeAndWalkToEachOther(animal, target, speedModifier, closeEnoughDistance)
//!                                      if !animal.closerThan(target, 3.0): return          (BREED_RANGE = 3)
//!                                      if time >= spawnChildAtTime:
//!                                          animal.spawnChildFromBreeding(level, target)
//!                                          animal.brain.eraseMemory(BREED_TARGET); target.brain.eraseMemory(BREED_TARGET)
//! stop (0..35):                        erase BREED_TARGET, WALK_TARGET, LOOK_TARGET; spawnChildAtTime = 0
//! hasBreedTargetOfRightType (0..43):   BREED_TARGET present && ((AgeableMob) it).is(partnerType)
//! getBreedTarget (0..16):              BREED_TARGET.get()   — `Optional.get`, so a cleared slot throws
//! ```
//!
//! **`start` re-runs `findValidBreedPartner`**, so the closest-passing walk
//! and its line-of-sight memo fills happen twice per start: once in
//! `checkExtraStartConditions` and once here. That is reproduced rather
//! than memoised, because the memo is a field of the memory value and the
//! second walk can therefore see a different answer from the first.
//!
//! The one RNG draw is `animal.getRandom().nextInt(50)` at start 54..64, on
//! the **mob** random, after the two `setMemory` calls and after
//! `lockGazeAndWalkToEachOther` — so a twin must run those first.
//!
//! `isPanicking()` is `PathfinderMob.isPanicking()`, which for a brain mob
//! (`!brain.isBrainDead()`) is `IS_PANICKING` being present; the goal-scan
//! fallback at its offset 37 is unreachable for every mob that has one of
//! these behaviours.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::behaviors::behavior_utils::{
    entity_is_visible, lock_gaze_and_walk_to_each_other,
};
use crate::brain::behaviors::nearest_visible_find_closest;
use crate::brain::memory::{MemoryModuleType, MemoryStatus, MemoryValue};
use crate::brain::BrainState;
use crate::host::MobHost;
use rustmc_worldgen::rng::RandomSource;

/// `BREED_RANGE`.
pub const BREED_RANGE: f64 = 3.0;
/// `MIN_DURATION`, the floor of the child timer.
pub const MIN_DURATION: i32 = 60;
/// `MAX_DURATION`, also the behaviour's fixed duration.
pub const MAX_DURATION: i32 = 110;
/// `DEFAULT_CLOSE_ENOUGH_DISTANCE`.
pub const DEFAULT_CLOSE_ENOUGH_DISTANCE: i32 = 2;

/// `AnimalMakeLove`'s body; build one with [`AnimalMakeLove::new`].
#[derive(Debug, Clone)]
pub struct AnimalMakeLove {
    /// `partnerType`, by registry name (`minecraft:` implied).
    pub partner_type: &'static str,
    /// `speedModifier`.
    pub speed_modifier: f32,
    /// `closeEnoughDistance`.
    pub close_enough_distance: i32,
    /// `spawnChildAtTime`.
    pub spawn_child_at_time: i64,
}

impl AnimalMakeLove {
    /// `AnimalMakeLove(EntityType, float, int)`.
    #[must_use]
    pub fn with_speed(
        partner_type: &'static str,
        speed_modifier: f32,
        close_enough_distance: i32,
    ) -> Behavior<AnimalMakeLove> {
        Behavior::with_duration(
            AnimalMakeLove {
                partner_type,
                speed_modifier,
                close_enough_distance,
                spawn_child_at_time: 0,
            },
            vec![
                (
                    MemoryModuleType::NearestVisibleLivingEntities,
                    MemoryStatus::ValuePresent,
                ),
                (MemoryModuleType::BreedTarget, MemoryStatus::ValueAbsent),
                (MemoryModuleType::WalkTarget, MemoryStatus::Registered),
                (MemoryModuleType::LookTarget, MemoryStatus::Registered),
                (MemoryModuleType::IsPanicking, MemoryStatus::ValueAbsent),
            ],
            MAX_DURATION,
        )
    }

    /// `AnimalMakeLove(EntityType)` — `this(type, 1.0f, 2)`.
    #[must_use]
    pub fn new(partner_type: &'static str) -> Behavior<AnimalMakeLove> {
        Self::with_speed(partner_type, 1.0, DEFAULT_CLOSE_ENOUGH_DISTANCE)
    }

    /// `PathfinderMob.isPanicking()` on the mob whose brain this is.
    fn is_panicking(brain: &BrainState) -> bool {
        brain.has_memory_value(MemoryModuleType::IsPanicking)
    }

    /// `findValidBreedPartner(animal)`.
    fn find_valid_breed_partner<M: MobHost>(
        &self,
        brain: &mut BrainState,
        cx: &mut MobAi<M>,
    ) -> Option<i32> {
        let partner_type = self.partner_type;
        nearest_visible_find_closest(brain, cx, |cx, id| {
            cx.mob.entity_kind_is(id, partner_type)
                && cx.mob.can_mate_with(id)
                && !cx.mob.entity(id).is_some_and(|e| e.panicking)
        })
    }

    /// `getBreedTarget(animal)`.
    fn breed_target(brain: &BrainState) -> i32 {
        brain
            .get_memory(MemoryModuleType::BreedTarget)
            .and_then(MemoryValue::as_entity)
            .expect("BREED_TARGET.get()")
    }

    /// `hasBreedTargetOfRightType(animal)`.
    fn has_breed_target_of_right_type<M: MobHost>(
        &self,
        brain: &BrainState,
        cx: &MobAi<M>,
    ) -> bool {
        brain
            .get_memory(MemoryModuleType::BreedTarget)
            .and_then(MemoryValue::as_entity)
            .is_some_and(|id| cx.mob.entity_kind_is(id, self.partner_type))
    }
}

impl<M: MobHost> BehaviorBody<M> for AnimalMakeLove {
    fn check_extra_start_conditions(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        cx.mob.is_in_love() && self.find_valid_breed_partner(brain, cx).is_some()
    }

    fn start(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        let partner = self
            .find_valid_breed_partner(brain, cx)
            .expect("findValidBreedPartner().get()");
        brain.set_memory_value(MemoryModuleType::BreedTarget, MemoryValue::Entity(partner));
        cx.mob.other_set_breed_target(partner, cx.mob.id());
        lock_gaze_and_walk_to_each_other(
            brain,
            cx,
            partner,
            self.speed_modifier,
            self.close_enough_distance,
        );
        let n = MIN_DURATION + cx.mob.random().next_i32_bound(50);
        self.spawn_child_at_time = time + i64::from(n);
    }

    fn can_still_use(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) -> bool {
        if !self.has_breed_target_of_right_type(brain, cx) {
            return false;
        }
        let target = Self::breed_target(brain);
        cx.mob.entity(target).is_some_and(|e| e.alive)
            && cx.mob.can_mate_with(target)
            && entity_is_visible(brain, cx, target)
            && time <= self.spawn_child_at_time
            && !Self::is_panicking(brain)
            && !cx.mob.entity(target).is_some_and(|e| e.panicking)
    }

    fn tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        let target = Self::breed_target(brain);
        lock_gaze_and_walk_to_each_other(
            brain,
            cx,
            target,
            self.speed_modifier,
            self.close_enough_distance,
        );
        let closer = cx
            .mob
            .distance_to_sqr_entity(target)
            .is_some_and(|d| d < BREED_RANGE * BREED_RANGE);
        if !closer {
            return;
        }
        if time >= self.spawn_child_at_time {
            cx.mob.spawn_child_from_breeding(target);
            brain.erase_memory(MemoryModuleType::BreedTarget);
            cx.mob.other_erase_breed_target(target);
        }
    }

    fn stop(&mut self, brain: &mut BrainState, _cx: &mut MobAi<M>, _time: i64) {
        brain.erase_memory(MemoryModuleType::BreedTarget);
        brain.erase_memory(MemoryModuleType::WalkTarget);
        brain.erase_memory(MemoryModuleType::LookTarget);
        self.spawn_child_at_time = 0;
    }

    fn name(&self) -> &'static str {
        "AnimalMakeLove"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::{BehaviorControl, Status};
    use crate::brain::memory::{MemoryModuleType as T, NearestVisibleLivingEntities, WalkTarget};
    use crate::brain::Brain;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    fn setup() -> (MobAi<TestMob>, BrainState) {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut panicking = EntityFacts::living(2, 1.5, 1.0, 0.5);
        panicking.panicking = true;
        cx.mob.others = vec![
            (EntityClass::Living, panicking),
            (EntityClass::Living, EntityFacts::living(3, 2.5, 1.0, 0.5)),
            (EntityClass::Living, EntityFacts::living(4, 3.5, 1.0, 0.5)),
        ];
        cx.mob.entity_kinds = vec![(2, "sniffer"), (3, "sniffer"), (4, "goat")];
        cx.mob.mates = vec![2, 3, 4];
        cx.mob.visible = vec![2, 3, 4];
        cx.mob.in_love = true;
        let mut brain = Brain::<TestMob>::new(
            &[
                T::NearestVisibleLivingEntities,
                T::BreedTarget,
                T::WalkTarget,
                T::LookTarget,
                T::IsPanicking,
            ],
            vec![],
        )
        .state;
        brain.set_memory_value(
            T::NearestVisibleLivingEntities,
            MemoryValue::NearestVisible(NearestVisibleLivingEntities::new(vec![2, 3, 4])),
        );
        (cx, brain)
    }

    #[test]
    fn skips_the_panicking_partner_and_the_wrong_type_and_times_the_child_off_the_mob_random() {
        let (mut cx, mut brain) = setup();
        let mut b = AnimalMakeLove::new("sniffer");
        // 2 is a sniffer and a mate but is panicking; 4 is not a sniffer.
        // The twin replays the one draw the jar takes at start 54..64,
        // after the memories are set and after lockGazeAndWalkToEachOther.
        let mut twin = TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1);
        let expected = MIN_DURATION + twin.random().next_i32_bound(50);
        assert!(b.try_start(&mut brain, &mut cx, 7));
        assert_eq!(
            brain.get_memory(T::BreedTarget),
            Some(&MemoryValue::Entity(3))
        );
        assert!(cx
            .mob
            .calls
            .contains(&"other_set_breed_target 3 1".to_string()));
        assert_eq!(b.body.spawn_child_at_time, 7 + i64::from(expected));
        assert!(brain.has_memory_value(T::WalkTarget));
        assert!(brain.has_memory_value(T::LookTarget));

        // canStillUse: alive, mateable, visible, in time, neither panicking.
        assert!(BehaviorBody::<TestMob>::can_still_use(
            &mut b.body,
            &mut brain,
            &mut cx,
            7
        ));
        assert!(!BehaviorBody::<TestMob>::can_still_use(
            &mut b.body,
            &mut brain,
            &mut cx,
            7 + i64::from(expected) + 1
        ));
        brain.set_memory_value(T::IsPanicking, MemoryValue::Bool(true));
        assert!(
            !BehaviorBody::<TestMob>::can_still_use(&mut b.body, &mut brain, &mut cx, 7),
            "own panic"
        );
        brain.erase_memory(T::IsPanicking);
        cx.mob.others[1].1.panicking = true;
        assert!(
            !BehaviorBody::<TestMob>::can_still_use(&mut b.body, &mut brain, &mut cx, 7),
            "partner panic"
        );
        cx.mob.others[1].1.panicking = false;

        // tick: BREED_RANGE is 3.0 on position(), so 2.5-0.5 = 2.0 is close.
        b.tick_or_stop(&mut brain, &mut cx, 7);
        assert!(cx.mob.bred_with.is_empty(), "not yet due");
        b.tick_or_stop(&mut brain, &mut cx, 7 + i64::from(expected));
        assert_eq!(cx.mob.bred_with, vec![3]);
        assert!(!brain.has_memory_value(T::BreedTarget));
        assert!(cx
            .mob
            .calls
            .contains(&"other_erase_breed_target 3".to_string()));
    }

    #[test]
    fn out_of_breed_range_does_not_spawn_and_stop_clears_the_three_memories() {
        let (mut cx, mut brain) = setup();
        cx.mob.others[1].1.x = 9.5;
        let mut b = AnimalMakeLove::with_speed("sniffer", 0.6, 2);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        let due = b.body.spawn_child_at_time;
        // `due` itself still passes canStillUse (`time <= spawnChildAtTime`)
        // and reaches the `time >= spawnChildAtTime` branch, so the only
        // thing stopping the child is the 9.0-block gap.
        b.tick_or_stop(&mut brain, &mut cx, due);
        assert!(cx.mob.bred_with.is_empty(), "9.0 blocks away, closerThan 3");
        assert_eq!(
            brain.get_memory(T::WalkTarget).unwrap().as_walk_target(),
            Some(&WalkTarget::new(
                crate::brain::memory::PositionTracker::entity(3, true),
                0.6,
                2
            ))
        );
        b.do_stop(&mut brain, &mut cx, due + 1);
        assert!(!brain.has_memory_value(T::BreedTarget));
        assert!(!brain.has_memory_value(T::WalkTarget));
        assert!(!brain.has_memory_value(T::LookTarget));
        assert_eq!(b.body.spawn_child_at_time, 0);
    }

    #[test]
    fn the_entry_condition_and_is_in_love_both_gate_the_start() {
        let (mut cx, mut brain) = setup();
        cx.mob.in_love = false;
        let mut b = AnimalMakeLove::new("sniffer");
        assert!(!b.try_start(&mut brain, &mut cx, 0), "not in love");
        cx.mob.in_love = true;
        brain.set_memory_value(T::IsPanicking, MemoryValue::Bool(true));
        assert!(
            !b.try_start(&mut brain, &mut cx, 0),
            "IS_PANICKING is VALUE_ABSENT in the entry condition"
        );
        brain.erase_memory(T::IsPanicking);
        cx.mob.visible.clear();
        assert!(
            !b.try_start(&mut brain, &mut cx, 0),
            "findClosest fails the line-of-sight memo"
        );
        assert_eq!(BehaviorControl::<TestMob>::status(&b), Status::Stopped);
    }
}
