//! `net.minecraft.world.entity.ai.behavior.PlayTagWithOtherKids`, from the
//! 26.2 jar. A `BehaviorBuilder` one-shot over
//! `group(present(VISIBLE_VILLAGER_BABIES), absent(WALK_TARGET),
//! registered(LOOK_TARGET), registered(INTERACTION_TARGET))`.
//! `MAX_FLEE_XZ_DIST = 20`, `MAX_FLEE_Y_DIST = 8`, `FLEE_SPEED_MODIFIER =
//! CHASE_SPEED_MODIFIER = 0.6f`, `MAX_CHASERS_PER_TARGET = 5`,
//! `AVERAGE_WAIT_TIME_BETWEEN_RUNS = 10`.
//!
//! ```text
//! lambda$create$2 (0..184):
//!   if level.getRandom().nextInt(10) != 0: return false                        <- LEVEL random, first
//!   babies = visibleVillagerBabies
//!   chaser = babies.stream().filter(b -> isFriendChasingMe(mob, b)).findAny()  <- sequential: the first
//!   if chaser present:
//!     for i in 0..10: v = LandRandomPos.getPos(mob, 20, 8); if v != null && level.isVillage(BlockPos.containing(v)):
//!         walk.set(WalkTarget(v, 0.6f, 0)); break
//!     return true
//!   chased = findSomeoneBeingChased(babies)
//!   if present: chaseKid(interaction, look, walk, chased); return true
//!   babies.stream().findAny().ifPresent(b -> chaseKid(interaction, look, walk, b)); return true
//! chaseKid: interaction.set(b); look.set(EntityTracker(b, true)); walk.set(WalkTarget(EntityTracker(b, false), 0.6f, 1))
//! findSomeoneBeingChased: counts = checkHowManyChasersEachFriendHas(babies)
//!   counts.entrySet().stream().sorted(comparingInt(value)).filter(0 < v <= 5).map(key).findFirst()
//! checkHowManyChasersEachFriendHas: HashMap<LivingEntity, Integer>; for b in babies.filter(isChasingSomeone): map.compute(whoAreYouChasing(b), +1)
//! isFriendChasingMe(me, b): b.INTERACTION_TARGET == me;  isChasingSomeone(b): b.INTERACTION_TARGET present
//! ```
//!
//! The chaser count map is a `HashMap` keyed by entity (identity hash), so
//! ties between equally chased kids resolve in an order **unverifiable by
//! construction**; this port breaks them by first appearance as a chase
//! target, which is what a stable sort over insertion order gives. Up to
//! ten `LandRandomPos.getPos` calls on the mob's random when fleeing.

use crate::ai::mob_ai::MobAi;
use crate::ai::random_pos::land_get_pos;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::memory::{MemoryModuleType, MemoryValue, PositionTracker, WalkTarget};
use crate::brain::BrainState;
use crate::host::MobHost;
use crate::phys::block_pos_containing_vec;

/// `MAX_FLEE_XZ_DIST`.
pub const MAX_FLEE_XZ_DIST: i32 = 20;
/// `MAX_FLEE_Y_DIST`.
pub const MAX_FLEE_Y_DIST: i32 = 8;
/// `FLEE_SPEED_MODIFIER`.
pub const FLEE_SPEED_MODIFIER: f32 = 0.6;
/// `CHASE_SPEED_MODIFIER`.
pub const CHASE_SPEED_MODIFIER: f32 = 0.6;
/// `MAX_CHASERS_PER_TARGET`.
pub const MAX_CHASERS_PER_TARGET: i32 = 5;
/// `AVERAGE_WAIT_TIME_BETWEEN_RUNS`.
pub const AVERAGE_WAIT_TIME_BETWEEN_RUNS: i32 = 10;

/// `PlayTagWithOtherKids`'s trigger.
#[derive(Debug, Clone, Copy, Default)]
pub struct PlayTagWithOtherKids;

impl PlayTagWithOtherKids {
    /// `create()`.
    #[must_use]
    pub fn create() -> OneShot<Self> {
        OneShot::new(PlayTagWithOtherKids)
    }

    /// `chaseKid(interaction, look, walk, kid)`.
    fn chase_kid(brain: &mut BrainState, kid: i32) {
        brain.set_memory_value(
            MemoryModuleType::InteractionTarget,
            MemoryValue::Entity(kid),
        );
        brain.set_memory_value(
            MemoryModuleType::LookTarget,
            MemoryValue::PositionTracker(PositionTracker::entity(kid, true)),
        );
        brain.set_memory_value(
            MemoryModuleType::WalkTarget,
            MemoryValue::WalkTarget(WalkTarget::new(
                PositionTracker::entity(kid, false),
                CHASE_SPEED_MODIFIER,
                1,
            )),
        );
    }

    /// `findSomeoneBeingChased(babies)`.
    pub fn find_someone_being_chased<M: MobHost>(cx: &MobAi<M>, babies: &[i32]) -> Option<i32> {
        let mut counts: Vec<(i32, i32)> = Vec::new();
        for b in babies {
            if let Some(target) = cx.mob.other_interaction_target(*b) {
                match counts.iter_mut().find(|(k, _)| *k == target) {
                    Some(e) => e.1 += 1,
                    None => counts.push((target, 1)),
                }
            }
        }
        counts.sort_by_key(|(_, n)| *n);
        counts
            .into_iter()
            .find(|(_, n)| *n > 0 && *n <= MAX_CHASERS_PER_TARGET)
            .map(|(k, _)| k)
    }
}

impl<M: MobHost> Trigger<M> for PlayTagWithOtherKids {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        let Some(Some(babies)) = brain.memory_internal(MemoryModuleType::VisibleVillagerBabies)
        else {
            return false;
        };
        let babies: Vec<i32> = babies.as_entities().unwrap_or(&[]).to_vec();
        if brain.memory_internal(MemoryModuleType::WalkTarget) != Some(None) {
            return false;
        }
        if brain
            .memory_internal(MemoryModuleType::LookTarget)
            .is_none()
            || brain
                .memory_internal(MemoryModuleType::InteractionTarget)
                .is_none()
        {
            return false;
        }
        if cx.mob.level_random_next_int(AVERAGE_WAIT_TIME_BETWEEN_RUNS) != 0 {
            return false;
        }
        let me = cx.mob.id();
        let chaser = babies
            .iter()
            .copied()
            .find(|b| cx.mob.other_interaction_target(*b) == Some(me));
        if chaser.is_some() {
            for _ in 0..10 {
                if let Some(v) = land_get_pos(cx, MAX_FLEE_XZ_DIST, MAX_FLEE_Y_DIST) {
                    if cx.mob.is_village(block_pos_containing_vec(v)) {
                        brain.set_memory_value(
                            MemoryModuleType::WalkTarget,
                            MemoryValue::WalkTarget(WalkTarget::at_vec(v, FLEE_SPEED_MODIFIER, 0)),
                        );
                        break;
                    }
                }
            }
            return true;
        }
        if let Some(kid) = Self::find_someone_being_chased(cx, &babies) {
            Self::chase_kid(brain, kid);
            return true;
        }
        if let Some(kid) = babies.first() {
            Self::chase_kid(brain, *kid);
        }
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![
            MemoryModuleType::VisibleVillagerBabies,
            MemoryModuleType::WalkTarget,
            MemoryModuleType::LookTarget,
            MemoryModuleType::InteractionTarget,
        ]
    }

    fn name(&self) -> &'static str {
        "PlayTagWithOtherKids"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};
    use rustmc_worldgen::rng::{LegacyRandomSource, RandomSource};

    fn setup(seed: i64) -> (MobAi<TestMob>, BrainState, LegacyRandomSource) {
        let mut level = GridLevel::new();
        level.floor(-40, -40, 40, 40, 0);
        let cx = MobAi::new(TestMob::new(level, 0.5, 1.0, 0.5, seed));
        let mut brain = Brain::<TestMob>::new(
            &[
                T::VisibleVillagerBabies,
                T::WalkTarget,
                T::LookTarget,
                T::InteractionTarget,
            ],
            vec![],
        )
        .state;
        brain.set_memory_value(
            T::VisibleVillagerBabies,
            MemoryValue::Entities(vec![2, 3, 4]),
        );
        (cx, brain, LegacyRandomSource::new(seed ^ 0x5DEECE66D))
    }

    fn fire(
        b: &mut OneShot<PlayTagWithOtherKids>,
        brain: &mut BrainState,
        cx: &mut MobAi<TestMob>,
        twin: &mut LegacyRandomSource,
    ) {
        loop {
            let d = twin.next_i32_bound(10);
            let r = b.try_start(brain, cx, 0);
            assert_eq!(r, d == 0);
            if r {
                return;
            }
        }
    }

    #[test]
    fn chases_the_most_chased_kid_else_the_first_and_flees_a_chaser() {
        let (mut cx, mut brain, mut twin) = setup(3);
        let mut b = PlayTagWithOtherKids::create();
        fire(&mut b, &mut brain, &mut cx, &mut twin);
        assert_eq!(
            brain.get_memory(T::InteractionTarget),
            Some(&MemoryValue::Entity(2)),
            "nobody chasing: the first kid"
        );
        brain.erase_memory(T::WalkTarget);
        cx.mob.other_interaction_targets = [(2, 4), (3, 4)].into_iter().collect();
        fire(&mut b, &mut brain, &mut cx, &mut twin);
        assert_eq!(
            brain.get_memory(T::InteractionTarget),
            Some(&MemoryValue::Entity(4))
        );
        brain.erase_memory(T::WalkTarget);
        cx.mob.other_interaction_targets.insert(3, 1);
        cx.mob.village_sections = vec![
            (0, 0, 0),
            (1, 0, 0),
            (-1, 0, 0),
            (0, 0, 1),
            (0, 0, -1),
            (1, 0, 1),
            (-1, 0, -1),
            (1, 0, -1),
            (-1, 0, 1),
        ];
        let mut twin_mob = MobAi::new(TestMob::new(cx.mob.level.clone(), 0.5, 1.0, 0.5, 3));
        twin_mob.mob.village_sections = cx.mob.village_sections.clone();
        fire(&mut b, &mut brain, &mut cx, &mut twin);
        let mut expect = None;
        for _ in 0..10 {
            if let Some(v) = land_get_pos(&mut twin_mob, 20, 8) {
                if twin_mob.mob.is_village(block_pos_containing_vec(v)) {
                    expect = Some(v);
                    break;
                }
            }
        }
        assert_eq!(
            brain
                .get_memory(T::WalkTarget)
                .map(|m| m.as_walk_target().cloned()),
            expect.map(|v| Some(WalkTarget::at_vec(v, 0.6, 0)))
        );
        assert_eq!(cx.mob.random.next_f32(), twin_mob.mob.random.next_f32());
    }
}
