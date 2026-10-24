//! `net.minecraft.world.entity.ai.behavior.BehaviorUtils`, from the 26.2 jar
//! — the static helpers the behaviours share, each a few offsets:
//!
//! ```text
//! lookAtEntity(mob, e):                       brain.setMemory(LOOK_TARGET, new EntityTracker(e, true))
//! lookAtEachOther(a, b):                      lookAtEntity(a, b); lookAtEntity(b, a)
//! setWalkAndLookTargetMemories(mob, tracker, speed, dist):
//!     walk = new WalkTarget(tracker, speed, dist); brain.setMemory(LOOK_TARGET, tracker); brain.setMemory(WALK_TARGET, walk)
//! setWalkAndLookTargetMemories(mob, Entity e, ...):   tracker = new EntityTracker(e, true)
//! setWalkAndLookTargetMemories(mob, BlockPos p, ...): tracker = new BlockPosTracker(p)
//! lockGazeAndWalkToEachOther(a, b, speed, dist):  lookAtEachOther(a, b); both setWalkAndLookTargetMemories(other, speed, dist)
//! entityIsVisible(brain, e):                  NEAREST_VISIBLE_LIVING_ENTITIES present && contains(e)
//! canSee(mob, e):                             hasMemoryValue(NVLE) && get(NVLE).contains(e)
//! targetIsValid(brain, type, EntityType):     getMemory(type).filter(e.is(type)).filter(isAlive).filter(entityIsVisible).isPresent()
//! isWithinAttackRange(mob, target, buffer) (0..56):
//!     item = mob.getMainHandItem().getItem()
//!     if item instanceof ProjectileWeaponItem w && mob.canUseNonMeleeWeapon(mainHand):
//!         return mob.closerThan(target, w.getDefaultProjectileRange() - buffer)      <- closerThan: distanceToSqr < d*d
//!     return mob.isWithinMeleeAttackRange(target)
//! isOtherTargetMuchFurtherAwayThanCurrentAttackTarget(mob, other, d) (0..66):
//!     cur = getMemory(ATTACK_TARGET); if empty: false
//!     a = mob.distanceToSqr(cur.position()); b = mob.distanceToSqr(other.position()); return b > a + d*d
//! getTargetNearestMe(me, a, b):               me.distanceToSqr(a.position()) < me.distanceToSqr(b.position()) ? a : b
//! getNearestTarget(me, Optional a, b):        a.isEmpty() ? b : getTargetNearestMe(me, a.get(), b)
//! isBreeding(e):                              hasMemoryValue(BREED_TARGET)
//! getLivingEntityFromUUIDMemory(e, type):     getMemory(type).map(level::getEntity).map(e -> e instanceof LivingEntity ? e : null)
//! ```
//!
//! `throwItem` (an `ItemEntity` spawn with `setDefaultPickUpDelay`) and
//! `findSectionClosestToVillage` are host calls; `getRandomSwimmablePos`
//! is in `ai::random_pos`.

use rustmc_data::BlockPos;

use crate::ai::mob_ai::MobAi;
use crate::brain::behaviors::nearest_visible_contains;
use crate::brain::memory::{MemoryModuleType, MemoryValue, PositionTracker, WalkTarget};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `lookAtEntity(mob, target)`.
pub fn look_at_entity(brain: &mut BrainState, target: i32) {
    brain.set_memory_value(
        MemoryModuleType::LookTarget,
        MemoryValue::PositionTracker(PositionTracker::entity(target, true)),
    );
}

/// `setWalkAndLookTargetMemories(mob, PositionTracker, float, int)`.
pub fn set_walk_and_look_target_memories(
    brain: &mut BrainState,
    tracker: PositionTracker,
    speed: f32,
    close_enough_dist: i32,
) {
    let walk = WalkTarget::new(tracker.clone(), speed, close_enough_dist);
    brain.set_memory_value(
        MemoryModuleType::LookTarget,
        MemoryValue::PositionTracker(tracker),
    );
    brain.set_memory_value(MemoryModuleType::WalkTarget, MemoryValue::WalkTarget(walk));
}

/// `setWalkAndLookTargetMemories(mob, Entity, float, int)`.
pub fn set_walk_and_look_target_memories_to_entity(
    brain: &mut BrainState,
    target: i32,
    speed: f32,
    close_enough_dist: i32,
) {
    set_walk_and_look_target_memories(
        brain,
        PositionTracker::entity(target, true),
        speed,
        close_enough_dist,
    );
}

/// `setWalkAndLookTargetMemories(mob, BlockPos, float, int)`.
pub fn set_walk_and_look_target_memories_to_block(
    brain: &mut BrainState,
    pos: BlockPos,
    speed: f32,
    close_enough_dist: i32,
) {
    set_walk_and_look_target_memories(brain, PositionTracker::block(pos), speed, close_enough_dist);
}

/// `entityIsVisible(brain, e)` and `canSee(mob, e)` — the same test.
pub fn entity_is_visible<M: MobHost>(
    brain: &mut BrainState,
    cx: &mut MobAi<M>,
    target: i32,
) -> bool {
    brain.has_memory_value(MemoryModuleType::NearestVisibleLivingEntities)
        && nearest_visible_contains(brain, cx, target)
}

/// `targetIsValid(brain, type, EntityType)`: the memory holds an entity of
/// that kind, alive and visible.
pub fn target_is_valid<M: MobHost>(
    brain: &mut BrainState,
    cx: &mut MobAi<M>,
    ty: MemoryModuleType,
    kind: &'static str,
) -> bool {
    let Some(id) = brain.get_memory(ty).and_then(MemoryValue::as_entity) else {
        return false;
    };
    cx.mob.entity_kind_is(id, kind)
        && cx.mob.entity(id).is_some_and(|e| e.alive)
        && entity_is_visible(brain, cx, id)
}

/// `isWithinAttackRange(mob, target, buffer)`.
pub fn is_within_attack_range<M: MobHost>(cx: &mut MobAi<M>, target: i32, buffer: i32) -> bool {
    if let Some(range) = cx.mob.usable_projectile_weapon_range() {
        let d = f64::from(range - buffer);
        return cx
            .mob
            .distance_to_sqr_entity(target)
            .is_some_and(|sq| sq < d * d);
    }
    cx.mob.is_within_melee_attack_range(target)
}

/// `isOtherTargetMuchFurtherAwayThanCurrentAttackTarget(mob, other, d)`.
pub fn is_other_target_much_further_away_than_current_attack_target<M: MobHost>(
    brain: &BrainState,
    cx: &MobAi<M>,
    other: i32,
    d: f64,
) -> bool {
    let Some(cur) = brain
        .get_memory(MemoryModuleType::AttackTarget)
        .and_then(MemoryValue::as_entity)
    else {
        return false;
    };
    let (Some(a), Some(b)) = (
        cx.mob
            .entity(cur)
            .map(|e| cx.mob.position().distance_to_sqr(e.position())),
        cx.mob
            .entity(other)
            .map(|e| cx.mob.position().distance_to_sqr(e.position())),
    ) else {
        return false;
    };
    b > a + d * d
}

/// `getTargetNearestMe(me, a, b)`.
pub fn target_nearest_me<M: MobHost>(cx: &MobAi<M>, a: i32, b: i32) -> i32 {
    let da = cx.mob.entity(a).map_or(f64::INFINITY, |e| {
        cx.mob.position().distance_to_sqr(e.position())
    });
    let db = cx.mob.entity(b).map_or(f64::INFINITY, |e| {
        cx.mob.position().distance_to_sqr(e.position())
    });
    if da < db {
        a
    } else {
        b
    }
}

/// `getNearestTarget(me, Optional<LivingEntity> a, b)`.
pub fn nearest_target<M: MobHost>(cx: &MobAi<M>, a: Option<i32>, b: i32) -> i32 {
    match a {
        None => b,
        Some(a) => target_nearest_me(cx, a, b),
    }
}

/// `isBreeding(e)`.
#[must_use]
pub fn is_breeding(brain: &BrainState) -> bool {
    brain.has_memory_value(MemoryModuleType::BreedTarget)
}

/// `BehaviorUtils.canSee(mob, target)` (0..34): `NEAREST_VISIBLE_LIVING_ENTITIES`
/// present and `contains(target)`.
pub fn can_see<M: MobHost>(brain: &mut BrainState, cx: &mut MobAi<M>, target: i32) -> bool {
    if !brain.has_memory_value(MemoryModuleType::NearestVisibleLivingEntities) {
        return false;
    }
    crate::brain::behaviors::nearest_visible_contains(brain, cx, target)
}

/// `BehaviorUtils.lockGazeAndWalkToEachOther(mob, other, speed, close)` (0..12):
/// `lookAtEachOther` (`lookAtEntity(mob, other)` then `lookAtEntity(other,
/// mob)`) and `setWalkAndLookTargetMemoriesToEachOther` (`mob` first, then
/// `other`), the other mob's half through the host.
pub fn lock_gaze_and_walk_to_each_other<M: MobHost>(
    brain: &mut BrainState,
    cx: &mut MobAi<M>,
    other: i32,
    speed: f32,
    close_enough_dist: i32,
) {
    let me = cx.mob.id();
    look_at_entity(brain, other);
    cx.mob.other_look_at_entity(other, me);
    set_walk_and_look_target_memories_to_entity(brain, other, speed, close_enough_dist);
    cx.mob
        .other_set_walk_and_look_target_to_entity(other, me, speed, close_enough_dist);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn walk_and_look_share_one_tracker() {
        let mut brain = Brain::<TestMob>::new(&[T::WalkTarget, T::LookTarget], vec![]).state;
        set_walk_and_look_target_memories_to_entity(&mut brain, 5, 0.5, 2);
        assert_eq!(
            brain
                .get_memory(T::LookTarget)
                .unwrap()
                .as_position_tracker(),
            Some(&PositionTracker::entity(5, true))
        );
        assert_eq!(
            brain.get_memory(T::WalkTarget).unwrap().as_walk_target(),
            Some(&WalkTarget::new(PositionTracker::entity(5, true), 0.5, 2))
        );
    }

    #[test]
    fn attack_range_uses_the_projectile_weapon_minus_the_buffer_strictly() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        cx.mob.others = vec![(EntityClass::Living, EntityFacts::living(3, 7.5, 1.0, 0.5))];
        assert!(
            !is_within_attack_range(&mut cx, 3, 1),
            "no weapon: melee, and the fixture says no"
        );
        cx.mob.projectile_weapon_range = Some(8);
        assert!(
            !is_within_attack_range(&mut cx, 3, 1),
            "7 blocks is not < 7"
        );
        cx.mob.projectile_weapon_range = Some(9);
        assert!(is_within_attack_range(&mut cx, 3, 1));
    }

    #[test]
    fn much_further_away_compares_squares_plus_d_squared() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        cx.mob.others = vec![
            (EntityClass::Living, EntityFacts::living(1, 3.5, 1.0, 0.5)),
            (EntityClass::Living, EntityFacts::living(2, 5.5, 1.0, 0.5)),
        ];
        let mut brain = Brain::<TestMob>::new(&[T::AttackTarget], vec![]).state;
        assert!(!is_other_target_much_further_away_than_current_attack_target(&brain, &cx, 2, 1.0));
        brain.set_memory_value(T::AttackTarget, MemoryValue::Entity(1));
        // 25 > 9 + 4 ; 25 > 9 + 16 is false
        assert!(is_other_target_much_further_away_than_current_attack_target(&brain, &cx, 2, 2.0));
        assert!(!is_other_target_much_further_away_than_current_attack_target(&brain, &cx, 2, 4.0));
        assert_eq!(target_nearest_me(&cx, 1, 2), 1);
        assert_eq!(nearest_target(&cx, None, 2), 2);
    }
}
