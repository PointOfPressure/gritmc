//! `net.minecraft.world.entity.animal.frog.ShootTongue` and its inner
//! `State`, from the 26.2 jar.
//!
//! ```text
//! ShootTongue$State: MOVE_TO_TARGET, CATCH_ANIMATION, EAT_ANIMATION, DONE
//!   (ordinals 0..3 — `tick`'s tableswitch is on `state.ordinal()`)
//!
//! constants: TIME_OUT_DURATION = 100, CATCH_ANIMATION_DURATION = 6,
//!            TONGUE_ANIMATION_DURATION = 10, EATING_DISTANCE = 1.75f,
//!            EATING_MOVEMENT_FACTOR = 0.75f,
//!            UNREACHABLE_TONGUE_TARGETS_COOLDOWN_DURATION = 100,
//!            MAX_UNREACHBLE_TONGUE_TARGETS_IN_MEMORY = 5
//!
//! ShootTongue(SoundEvent tongueSound, SoundEvent eatSound)   (0..50)
//!   entry ImmutableMap.of(WALK_TARGET:VALUE_ABSENT, LOOK_TARGET:REGISTERED,
//!                         ATTACK_TARGET:VALUE_PRESENT, IS_PANICKING:VALUE_ABSENT)
//!   Behavior(map, 100) -> (100, 100);  state = DONE
//!
//!   checkExtraStartConditions (0..73):
//!        0  LivingEntity t = brain.getMemory(ATTACK_TARGET).get()
//!       17  boolean ok = canPathfindToTarget(frog, t)
//!       25  if !ok: brain.eraseMemory(ATTACK_TARGET); addUnreachableTargetToMemory(frog, t)
//!       46  return ok && frog.getPose() != CROAKING && Frog.canEat(t)
//!   canStillUse (0..41):
//!        brain.hasMemoryValue(ATTACK_TARGET) && state != DONE
//!        && !brain.hasMemoryValue(IS_PANICKING)
//!   start (0..67):
//!       18  BehaviorUtils.lookAtEntity(frog, target)
//!       24  frog.setTongueTarget(target)
//!       30  brain.setMemory(WALK_TARGET, new WalkTarget(target.position(), 2.0f, 0))
//!       54  calculatePathCounter = 10
//!       60  state = MOVE_TO_TARGET
//!   stop (0..21):
//!        brain.eraseMemory(ATTACK_TARGET); frog.eraseTongueTarget();
//!        frog.setPose(STANDING)
//!   tick (0..252):
//!        6  LivingEntity target = brain.getMemory(ATTACK_TARGET).get()
//!       18  frog.setTongueTarget(target)
//!       24  switch (state.ordinal()):
//!       60    MOVE_TO_TARGET:
//!       60      if target.distanceTo(frog) < 1.75f:
//!       72         level.playSound(null, frog, tongueSound, NEUTRAL, 2.0f, 1.0f)
//!       87         frog.setPose(USING_TONGUE)
//!       94         target.setDeltaMovement(
//!                     target.position().vectorTo(frog.position()).normalize().scale(0.75d))
//!      120         eatAnimationTimer = 0
//!      125         state = CATCH_ANIMATION
//!      135      else if calculatePathCounter <= 0:
//!      142         brain.setMemory(WALK_TARGET, new WalkTarget(target.position(), 2.0f, 0))
//!      166         calculatePathCounter = 10
//!      175      else: calculatePathCounter--
//!      188    CATCH_ANIMATION:
//!      188      if ++eatAnimationTimer >= 6:      // post-increment, compared after
//!      204         state = EAT_ANIMATION
//!      211         eatEntity(level, frog)
//!      220    EAT_ANIMATION:
//!      220      if eatAnimationTimer >= 10: state = DONE
//!      239      else: eatAnimationTimer++
//!      252    DONE: nothing
//!
//!   eatEntity(level, frog) (0..68):
//!        0  level.playSound(null, frog, eatSound, NEUTRAL, 2.0f, 1.0f)
//!       15  Optional<Entity> t = frog.getTongueTarget()
//!       20  if t.isPresent() && t.get().isAlive():
//!       44     frog.doHurtTarget(level, t.get())
//!       52     if !t.get().isAlive(): t.get().remove(RemovalReason.KILLED)
//!
//!   canPathfindToTarget(frog, target) (0..29):
//!        Path p = frog.getNavigation().createPath(target, 0)
//!        return p != null && p.getDistToTarget() < 1.75f
//!
//!   addUnreachableTargetToMemory(frog, target) (0..95):
//!        0  List<UUID> list = brain.getMemory(UNREACHABLE_TONGUE_TARGETS)
//!                                  .orElseGet(ArrayList::new)
//!       22  boolean isNew = !list.contains(target.getUUID())
//!       42  if list.size() == 5 && isNew: list.remove(0)
//!       65  if isNew: list.add(target.getUUID())
//!       81  brain.setMemoryWithExpiry(UNREACHABLE_TONGUE_TARGETS, list, 100L)
//! ```
//!
//! **`checkExtraStartConditions` has a side effect on the failing path and
//! it is the interesting half.** `canPathfindToTarget` is evaluated first
//! and stored; on `false` the attack target is erased AND the target's UUID
//! is remembered as unreachable for 100 ticks, before the method returns
//! `false`. The pose and `Frog.canEat` tests come after and have no side
//! effect at all — so a frog refused for croaking keeps its attack target
//! and forgets nothing.
//!
//! **The unreachable list is capped at 5 and evicts the OLDEST**, and both
//! the eviction and the append are guarded by `isNew`, so a repeat of a
//! UUID already in the list rewrites the memory's 100-tick expiry without
//! changing the list. `list.remove(0)` fires only when the list is exactly
//! 5 long — `==`, not `>=`.
//!
//! **`tick`'s catch branch is `++eatAnimationTimer >= 6`, a PRE-increment
//! compared after** (offsets 188..201: `dup_x1`, `iadd`, `putfield`, then
//! `bipush 6`, `if_icmplt`). The eat branch is the other way round —
//! `eatAnimationTimer >= 10` is tested first and the increment happens only
//! on the `else` (offsets 220 and 239). So the catch state lasts 6 ticks
//! and the eat state 5 more, not 6 and 10.
//!
//! **The prey is pulled toward the frog, not the frog toward the prey.**
//! `target.setDeltaMovement(target.position().vectorTo(frog.position())
//! .normalize().scale(0.75d))` writes the *target's* velocity;
//! `EATING_MOVEMENT_FACTOR` is 0.75 and it is a `double` scale of a
//! normalised vector.
//!
//! **`eatEntity` hurts and then removes.** `doHurtTarget`'s boolean is
//! discarded (`pop` at offset 51); the removal is conditional on the target
//! being dead *after* the hurt, so a survivor is left alone and a kill is
//! removed with `RemovalReason.KILLED` rather than left to die naturally.
//!
//! **A null:** `tick` re-calls `frog.setTongueTarget(target)` every tick,
//! before the switch, even in `DONE` — the same value `start` already set.
//!
//! **The two sounds are both `NEUTRAL` at volume 2.0f, pitch 1.0f**, and
//! both go through `ServerLevel.playSound(null, frog, ...)` rather than
//! `Entity.playSound`.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::behaviors::behavior_utils;
use crate::brain::memory::{MemoryModuleType, MemoryStatus, MemoryValue, WalkTarget};
use crate::brain::BrainState;
use crate::host::{MobHost, Pose, SoundSpec};
use crate::phys::Vec3;

/// `ShootTongue.TIME_OUT_DURATION`.
pub const TIME_OUT_DURATION: i32 = 100;
/// `ShootTongue.CATCH_ANIMATION_DURATION`.
pub const CATCH_ANIMATION_DURATION: i32 = 6;
/// `ShootTongue.TONGUE_ANIMATION_DURATION`.
pub const TONGUE_ANIMATION_DURATION: i32 = 10;
/// `ShootTongue.EATING_DISTANCE`.
pub const EATING_DISTANCE: f32 = 1.75;
/// `ShootTongue.EATING_MOVEMENT_FACTOR`.
pub const EATING_MOVEMENT_FACTOR: f64 = 0.75;
/// `UNREACHABLE_TONGUE_TARGETS_COOLDOWN_DURATION`.
pub const UNREACHABLE_TONGUE_TARGETS_COOLDOWN_DURATION: i64 = 100;
/// `MAX_UNREACHBLE_TONGUE_TARGETS_IN_MEMORY` — the jar's spelling.
pub const MAX_UNREACHBLE_TONGUE_TARGETS_IN_MEMORY: usize = 5;
/// The walk speed `start` and the re-path both write.
pub const WALK_SPEED: f32 = 2.0;
/// The tick budget between re-paths.
pub const CALCULATE_PATH_INTERVAL: i32 = 10;
/// `SoundSource.NEUTRAL`.
pub const SOUND_SOURCE_NEUTRAL: &str = "neutral";

/// `ShootTongue$State`, in `<clinit>` order, which is the order
/// `tick`'s `tableswitch` uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum State {
    /// ordinal 0.
    MoveToTarget,
    /// ordinal 1.
    CatchAnimation,
    /// ordinal 2.
    EatAnimation,
    /// ordinal 3, and the constructor's initial value.
    #[default]
    Done,
}

/// `ShootTongue`'s body.
#[derive(Debug, Clone, Copy)]
pub struct ShootTongue {
    /// `eatAnimationTimer`.
    pub eat_animation_timer: i32,
    /// `calculatePathCounter`.
    pub calculate_path_counter: i32,
    /// `tongueSound`.
    pub tongue_sound: SoundSpec,
    /// `eatSound`.
    pub eat_sound: SoundSpec,
    /// `state`, `DONE` at construction.
    pub state: State,
}

impl ShootTongue {
    /// `new ShootTongue(tongueSound, eatSound)`.
    #[must_use]
    pub fn new(tongue_sound: SoundSpec, eat_sound: SoundSpec) -> Behavior<ShootTongue> {
        Behavior::with_duration(
            ShootTongue {
                eat_animation_timer: 0,
                calculate_path_counter: 0,
                tongue_sound,
                eat_sound,
                state: State::Done,
            },
            vec![
                (MemoryModuleType::WalkTarget, MemoryStatus::ValueAbsent),
                (MemoryModuleType::LookTarget, MemoryStatus::Registered),
                (MemoryModuleType::AttackTarget, MemoryStatus::ValuePresent),
                (MemoryModuleType::IsPanicking, MemoryStatus::ValueAbsent),
            ],
            TIME_OUT_DURATION,
        )
    }

    /// `canPathfindToTarget(Frog, LivingEntity)` (0..29).
    fn can_pathfind_to_target<M: MobHost>(cx: &mut MobAi<M>, target: i32) -> bool {
        let Some(facts) = cx.mob.entity(target) else {
            return false;
        };
        let pos = facts.block_position();
        cx.navigation
            .create_path_to_entity_at(&mut cx.mob, pos, 0)
            .is_some_and(|p| p.dist_to_target() < EATING_DISTANCE)
    }

    /// `addUnreachableTargetToMemory(Frog, LivingEntity)` (0..95).
    fn add_unreachable_target_to_memory<M: MobHost>(
        brain: &mut BrainState,
        cx: &mut MobAi<M>,
        target: i32,
    ) {
        let mut list = match brain.get_memory(MemoryModuleType::UnreachableTongueTargets) {
            Some(MemoryValue::Uuids(v)) => v.clone(),
            _ => Vec::new(),
        };
        let uuid = cx.mob.entity_uuid(target);
        let is_new = !list.contains(&uuid);
        if list.len() == MAX_UNREACHBLE_TONGUE_TARGETS_IN_MEMORY && is_new {
            list.remove(0);
        }
        if is_new {
            list.push(uuid);
        }
        brain.set_memory_with_expiry(
            MemoryModuleType::UnreachableTongueTargets,
            MemoryValue::Uuids(list),
            UNREACHABLE_TONGUE_TARGETS_COOLDOWN_DURATION,
        );
    }

    /// `eatEntity(ServerLevel, Frog)` (0..68).
    fn eat_entity<M: MobHost>(&self, cx: &mut MobAi<M>) {
        let sound = self.eat_sound;
        cx.mob
            .level_play_sound(&sound, SOUND_SOURCE_NEUTRAL, 2.0, 1.0);
        let Some(target) = cx.mob.frog_tongue_target() else {
            return;
        };
        if !cx.mob.entity(target).is_some_and(|e| e.alive) {
            return;
        }
        // The boolean is discarded (`pop` at offset 51).
        let _ = cx.mob.do_hurt_target(target);
        if !cx.mob.entity(target).is_some_and(|e| e.alive) {
            cx.mob.remove_entity_killed(target);
        }
    }
}

impl<M: MobHost> BehaviorBody<M> for ShootTongue {
    fn check_extra_start_conditions(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        let Some(target) = brain
            .get_memory(MemoryModuleType::AttackTarget)
            .and_then(MemoryValue::as_entity)
        else {
            return false;
        };
        let ok = ShootTongue::can_pathfind_to_target(cx, target);
        if !ok {
            brain.erase_memory(MemoryModuleType::AttackTarget);
            ShootTongue::add_unreachable_target_to_memory(brain, cx, target);
        }
        ok && !cx.mob.has_pose(Pose::Croaking) && cx.mob.frog_can_eat(target)
    }

    fn can_still_use(&mut self, brain: &mut BrainState, _cx: &mut MobAi<M>, _time: i64) -> bool {
        brain.has_memory_value(MemoryModuleType::AttackTarget)
            && self.state != State::Done
            && !brain.has_memory_value(MemoryModuleType::IsPanicking)
    }

    fn start(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        let Some(target) = brain
            .get_memory(MemoryModuleType::AttackTarget)
            .and_then(MemoryValue::as_entity)
        else {
            return;
        };
        behavior_utils::look_at_entity(brain, target);
        cx.mob.frog_set_tongue_target(target);
        if let Some(facts) = cx.mob.entity(target) {
            brain.set_memory_value(
                MemoryModuleType::WalkTarget,
                MemoryValue::WalkTarget(WalkTarget::at_vec(facts.position(), WALK_SPEED, 0)),
            );
        }
        self.calculate_path_counter = CALCULATE_PATH_INTERVAL;
        self.state = State::MoveToTarget;
    }

    fn stop(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        brain.erase_memory(MemoryModuleType::AttackTarget);
        cx.mob.frog_erase_tongue_target();
        cx.mob.set_pose(Pose::Standing);
    }

    fn tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        let Some(target) = brain
            .get_memory(MemoryModuleType::AttackTarget)
            .and_then(MemoryValue::as_entity)
        else {
            return;
        };
        cx.mob.frog_set_tongue_target(target);
        match self.state {
            State::MoveToTarget => {
                let Some(facts) = cx.mob.entity(target) else {
                    return;
                };
                #[allow(clippy::cast_possible_truncation)]
                let d = facts
                    .position()
                    .distance_to(Vec3::new(cx.mob.x(), cx.mob.y(), cx.mob.z()))
                    as f32;
                if d < EATING_DISTANCE {
                    let sound = self.tongue_sound;
                    cx.mob
                        .level_play_sound(&sound, SOUND_SOURCE_NEUTRAL, 2.0, 1.0);
                    cx.mob.set_pose(Pose::UsingTongue);
                    let pull = Vec3::new(cx.mob.x(), cx.mob.y(), cx.mob.z())
                        .subtract(facts.position())
                        .normalize()
                        .scale(EATING_MOVEMENT_FACTOR);
                    cx.mob.set_other_delta_movement(target, pull);
                    self.eat_animation_timer = 0;
                    self.state = State::CatchAnimation;
                } else if self.calculate_path_counter <= 0 {
                    brain.set_memory_value(
                        MemoryModuleType::WalkTarget,
                        MemoryValue::WalkTarget(WalkTarget::at_vec(
                            facts.position(),
                            WALK_SPEED,
                            0,
                        )),
                    );
                    self.calculate_path_counter = CALCULATE_PATH_INTERVAL;
                } else {
                    self.calculate_path_counter -= 1;
                }
            }
            State::CatchAnimation => {
                self.eat_animation_timer += 1;
                if self.eat_animation_timer >= CATCH_ANIMATION_DURATION {
                    self.state = State::EatAnimation;
                    self.eat_entity(cx);
                }
            }
            State::EatAnimation => {
                if self.eat_animation_timer >= TONGUE_ANIMATION_DURATION {
                    self.state = State::Done;
                } else {
                    self.eat_animation_timer += 1;
                }
            }
            State::Done => {}
        }
    }

    fn name(&self) -> &'static str {
        "ShootTongue"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::host::{BlockFacts, EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    const TONGUE: SoundSpec = SoundSpec("entity.frog.tongue");
    const EAT: SoundSpec = SoundSpec("entity.frog.eat");

    fn setup(target_at: f64) -> (MobAi<TestMob>, BrainState) {
        let mut level = GridLevel::new();
        level.fill(-8, 0, -8, 8, 0, 8, BlockFacts::STONE);
        let mut mob = TestMob::new(level, 0.5, 1.0, 0.5, 37);
        mob.on_ground = true;
        mob.others.push((
            EntityClass::Living,
            EntityFacts::living(9, target_at, 1.0, 0.5),
        ));
        mob.frog_edible.push(9);
        mob.uuids.push((9, 0x1234));
        let cx = MobAi::new(mob);
        let mut brain = Brain::<TestMob>::new(
            &[
                T::WalkTarget,
                T::LookTarget,
                T::AttackTarget,
                T::IsPanicking,
                T::UnreachableTongueTargets,
            ],
            vec![],
        )
        .state;
        brain.set_memory_value(T::AttackTarget, MemoryValue::Entity(9));
        (cx, brain)
    }

    #[test]
    fn the_state_starts_at_done_and_start_moves_it_to_move_to_target() {
        let (mut cx, mut brain) = setup(1.0);
        let mut b = ShootTongue::new(TONGUE, EAT);
        assert_eq!(b.body.state, State::Done);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(b.body.state, State::MoveToTarget);
        assert_eq!(b.body.calculate_path_counter, 10);
        assert_eq!(b.end_timestamp(), 100);
        assert!(brain.has_memory_value(T::WalkTarget));
        assert!(brain.has_memory_value(T::LookTarget));
        assert_eq!(cx.mob.frog_tongue_target, Some(9));
    }

    #[test]
    fn an_unreachable_target_is_forgotten_and_remembered_as_unreachable() {
        // No path: the target is far away across a hole in the floor.
        let (mut cx, mut brain) = setup(1.0);
        cx.mob.others[0].1 = EntityFacts::living(9, 200.0, 1.0, 0.5);
        let mut b = ShootTongue::new(TONGUE, EAT);
        assert!(!b.try_start(&mut brain, &mut cx, 0));
        assert!(
            !brain.has_memory_value(T::AttackTarget),
            "the failing path erases the attack target"
        );
        assert_eq!(
            brain.get_memory(T::UnreachableTongueTargets),
            Some(&MemoryValue::Uuids(vec![0x1234]))
        );
        assert_eq!(brain.time_until_expiry(T::UnreachableTongueTargets), 100);
    }

    #[test]
    fn a_croaking_frog_keeps_its_target_and_forgets_nothing() {
        let (mut cx, mut brain) = setup(1.0);
        cx.mob.pose = Pose::Croaking;
        let mut b = ShootTongue::new(TONGUE, EAT);
        assert!(!b.try_start(&mut brain, &mut cx, 0));
        assert!(
            brain.has_memory_value(T::AttackTarget),
            "the pose test has no side effect"
        );
        assert!(!brain.has_memory_value(T::UnreachableTongueTargets));
    }

    #[test]
    fn the_unreachable_list_caps_at_five_and_evicts_the_oldest() {
        let (mut cx, mut brain) = setup(1.0);
        brain.set_memory_value(
            T::UnreachableTongueTargets,
            MemoryValue::Uuids(vec![1, 2, 3, 4, 5]),
        );
        ShootTongue::add_unreachable_target_to_memory(&mut brain, &mut cx, 9);
        assert_eq!(
            brain.get_memory(T::UnreachableTongueTargets),
            Some(&MemoryValue::Uuids(vec![2, 3, 4, 5, 0x1234])),
            "the oldest goes, the new one is appended"
        );
        // A repeat changes nothing but the expiry.
        ShootTongue::add_unreachable_target_to_memory(&mut brain, &mut cx, 9);
        assert_eq!(
            brain.get_memory(T::UnreachableTongueTargets),
            Some(&MemoryValue::Uuids(vec![2, 3, 4, 5, 0x1234]))
        );
    }

    #[test]
    fn inside_the_eating_distance_the_prey_is_pulled_toward_the_frog() {
        let (mut cx, mut brain) = setup(2.0);
        let mut b = ShootTongue::new(TONGUE, EAT);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        cx.mob.calls.clear();
        b.body.tick(&mut brain, &mut cx, 1);
        assert_eq!(b.body.state, State::CatchAnimation);
        assert_eq!(b.body.eat_animation_timer, 0);
        assert_eq!(cx.mob.pose, Pose::UsingTongue);
        let calls = cx.mob.calls.join(" | ");
        assert!(
            calls.contains("level_play_sound entity.frog.tongue neutral 2 1"),
            "{calls}"
        );
        // The prey at x = 2.0 is pulled toward the frog at x = 0.5: -0.75.
        assert!(
            calls.contains("set_other_delta_movement 9 (-0.75 0 0)"),
            "{calls}"
        );
    }

    #[test]
    fn the_catch_state_lasts_six_ticks_and_the_eat_state_five_more() {
        let (mut cx, mut brain) = setup(1.0);
        let mut b = ShootTongue::new(TONGUE, EAT);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        b.body.tick(&mut brain, &mut cx, 1);
        assert_eq!(b.body.state, State::CatchAnimation);
        // ++timer >= 6: six ticks, and the sixth eats.
        for i in 1..6 {
            b.body.tick(&mut brain, &mut cx, 1 + i);
            assert_eq!(b.body.state, State::CatchAnimation, "tick {i}");
        }
        cx.mob.calls.clear();
        b.body.tick(&mut brain, &mut cx, 7);
        assert_eq!(b.body.state, State::EatAnimation);
        assert_eq!(b.body.eat_animation_timer, 6);
        assert!(cx
            .mob
            .calls
            .iter()
            .any(|c| c == "level_play_sound entity.frog.eat neutral 2 1"));
        // timer >= 10 is tested BEFORE the increment, so 6 -> 10 is four
        // more ticks and the fifth flips to DONE.
        for i in 0..4 {
            b.body.tick(&mut brain, &mut cx, 8 + i);
            assert_eq!(b.body.state, State::EatAnimation, "eat tick {i}");
        }
        assert_eq!(b.body.eat_animation_timer, 10);
        b.body.tick(&mut brain, &mut cx, 12);
        assert_eq!(b.body.state, State::Done);
    }

    #[test]
    fn a_dead_prey_is_removed_but_a_survivor_is_not() {
        let (mut cx, mut brain) = setup(1.0);
        cx.mob.hurt_kills.push(9);
        let mut b = ShootTongue::new(TONGUE, EAT);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        b.body.state = State::CatchAnimation;
        b.body.eat_animation_timer = 5;
        cx.mob.calls.clear();
        b.body.tick(&mut brain, &mut cx, 1);
        let calls = cx.mob.calls.join(" | ");
        assert!(calls.contains("remove_entity_killed 9"), "{calls}");

        let (mut cx, mut brain) = setup(1.0);
        let mut b = ShootTongue::new(TONGUE, EAT);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        b.body.state = State::CatchAnimation;
        b.body.eat_animation_timer = 5;
        cx.mob.calls.clear();
        b.body.tick(&mut brain, &mut cx, 1);
        let calls = cx.mob.calls.join(" | ");
        assert!(!calls.contains("remove_entity_killed"), "{calls}");
    }
}
