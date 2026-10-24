//! `net.minecraft.world.entity.monster.breeze.Slide` and the
//! `BreezeAi$SlideToTargetSink` that carries it out, from the 26.2 jar.
//!
//! ```text
//! Slide()   Behavior<Breeze>, Behavior(map) -> (60, 60)
//!   entry Map.of(ATTACK_TARGET:VALUE_PRESENT, WALK_TARGET:VALUE_ABSENT,
//!                BREEZE_JUMP_COOLDOWN:VALUE_ABSENT, BREEZE_SHOOT:VALUE_ABSENT)
//!   checkExtraStartConditions (0..29):
//!        breeze.onGround() && !breeze.isInWater() && breeze.getPose() == STANDING
//!   start (0..167):
//!       0   target = brain.getMemory(ATTACK_TARGET).orElse(null)
//!      19   if target == null: return
//!      25   boolean inner = breeze.withinInnerCircleRange(target.position())
//!      36   Vec3 chosen = null
//!      39   if inner:
//!      44      Vec3 away = DefaultRandomPos.getPosAway(breeze, 5, 5, target.position())
//!      57      if away != null && BreezeUtil.hasLineOfSight(breeze, away)
//!      71         && target.distanceToSqr(away.x, away.y, away.z) > target.distanceToSqr(breeze):
//!     101            chosen = away
//!     105   if chosen == null:
//!     110      chosen = breeze.getRandom().nextBoolean()
//!     122               ? BreezeUtil.randomPointBehindTarget(target, breeze.getRandom())
//!     134               : randomPointInMiddleCircle(breeze, target)
//!     142   brain.setMemory(WALK_TARGET,
//!                new WalkTarget(BlockPos.containing(chosen), 0.6f, 1))
//!
//!   randomPointInMiddleCircle(breeze, target)  (0..57)
//!       0   Vec3 d = target.position().subtract(breeze.position())
//!      12   double len = d.length() - Mth.lerp(breeze.getRandom().nextDouble(), 8.0d, 4.0d)
//!      36   Vec3 step = d.normalize().multiply(len, len, len)
//!      48   return breeze.position().add(step)
//!
//! BreezeAi$SlideToTargetSink(int, int) extends MoveToTargetSink
//!   start (0..21):  super.start(...); breeze.playSound(BREEZE_SLIDE); setPose(SLIDING)
//!   stop  (0..43):  super.stop(...); setPose(STANDING)
//!                   if brain.hasMemoryValue(ATTACK_TARGET):
//!                       brain.setMemoryWithExpiry(BREEZE_SHOOT, Unit.INSTANCE, 60L)
//! ```
//!
//! **`Slide` writes a walk target and nothing else** — no pose, no sound,
//! no cooldown. It is `SlideToTargetSink`, the `MoveToTargetSink` subclass
//! that consumes that walk target, that plays `BREEZE_SLIDE` and sets the
//! `SLIDING` pose. The two are one behaviour split across two activities,
//! and the split is why `Slide.checkExtraStartConditions` insists on the
//! `STANDING` pose: it must not fire while a slide is already running.
//!
//! **The inner-circle branch has a three-part guard and the third part is
//! the interesting one.** A retreat position is taken only if it is
//! *further from the target than the breeze currently is* — offset 71's
//! `target.distanceToSqr(away)` against offset 93's
//! `target.distanceToSqr(breeze)`, strictly greater. A retreat that would
//! not actually retreat falls through to the coin flip.
//!
//! **The `Mth.lerp` in `randomPointInMiddleCircle` runs 8 down to 4, not up.**
//! `lerp(nextDouble(), 8.0, 4.0)` subtracted from the distance means the
//! breeze stops between 4 and 8 blocks short of the target, and a
//! `nextDouble()` near 1 leaves it *closer*, not further.
//!
//! **RNG:** the inner-circle branch draws whatever `DefaultRandomPos
//! .getPosAway` draws; the fallback draws `nextBoolean()` and then either
//! two draws inside `randomPointBehindTarget` (gaussian, float) or one
//! `nextDouble()` inside `randomPointInMiddleCircle`. All from the mob's
//! own random.
//!
//! **`multiply(len, len, len)` is `scale(len)`** — the same componentwise
//! product with the same operand three times.
//!
//! **A null:** `Slide` has no `canStillUse` override, so the base's `false`
//! stands and it stops on the tick after it starts, exactly as
//! `ShootWhenStuck` does. Its 60-tick duration is never reached either.

use crate::ai::mob_ai::MobAi;
use crate::ai::random_pos::default_get_pos_away;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::behaviors::breeze_util;
use crate::brain::behaviors::move_to_target_sink::MoveToTargetSink;
use crate::brain::memory::{MemoryModuleType, MemoryStatus, MemoryValue, WalkTarget};
use crate::brain::BrainState;
use crate::host::{MobHost, Pose, SoundSpec};
use crate::mth;
use crate::phys::{block_pos_containing_vec, Vec3};
use rustmc_worldgen::rng::RandomSource;

/// `SoundEvents.BREEZE_SLIDE`.
pub const BREEZE_SLIDE: SoundSpec = SoundSpec("entity.breeze.slide");
/// The `WalkTarget` speed `Slide.start` writes.
pub const SLIDE_SPEED: f32 = 0.6;
/// The `WalkTarget` close-enough distance `Slide.start` writes.
pub const SLIDE_CLOSE_ENOUGH: i32 = 1;
/// The expiry `SlideToTargetSink.stop` gives `BREEZE_SHOOT`.
pub const SHOOT_MEMORY_TICKS: i64 = 60;

/// `Slide`'s body.
#[derive(Debug, Clone, Copy, Default)]
pub struct Slide;

impl Slide {
    /// `new Slide()`.
    #[must_use]
    pub fn new() -> Behavior<Slide> {
        Behavior::new(
            Slide,
            vec![
                (MemoryModuleType::AttackTarget, MemoryStatus::ValuePresent),
                (MemoryModuleType::WalkTarget, MemoryStatus::ValueAbsent),
                (
                    MemoryModuleType::BreezeJumpCooldown,
                    MemoryStatus::ValueAbsent,
                ),
                (MemoryModuleType::BreezeShoot, MemoryStatus::ValueAbsent),
            ],
        )
    }

    /// `randomPointInMiddleCircle(Breeze, LivingEntity)` (0..57).
    fn random_point_in_middle_circle<M: MobHost>(cx: &mut MobAi<M>, target: Vec3) -> Vec3 {
        let d = target.subtract(cx.mob.position());
        let next = cx.mob.random().next_f64();
        let len = d.length() - mth::lerp(next, 8.0, 4.0);
        let step = d.normalize().scale(len);
        cx.mob.position() + step
    }
}

impl<M: MobHost> BehaviorBody<M> for Slide {
    fn check_extra_start_conditions(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        cx.mob.on_ground() && !cx.mob.is_in_water() && cx.mob.has_pose(Pose::Standing)
    }

    fn start(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        let Some(target_id) = brain
            .get_memory(MemoryModuleType::AttackTarget)
            .and_then(MemoryValue::as_entity)
        else {
            return;
        };
        let Some(target) = cx.mob.entity(target_id) else {
            return;
        };
        let target_pos = target.position();

        let inner = cx.mob.breeze_within_inner_circle_range(target_pos);
        let mut chosen: Option<Vec3> = None;
        if inner {
            if let Some(away) = default_get_pos_away(cx, 5, 5, target_pos) {
                if breeze_util::has_line_of_sight(cx, away)
                    && target.distance_to_sqr(away.x, away.y, away.z)
                        > target.distance_to_sqr(cx.mob.x(), cx.mob.y(), cx.mob.z())
                {
                    chosen = Some(away);
                }
            }
        }
        let chosen = match chosen {
            Some(v) => v,
            None => {
                if cx.mob.random().next_bool() {
                    let y_head_rot = cx.mob.entity_y_head_rot(target_id);
                    breeze_util::random_point_behind_target(cx, y_head_rot, target_pos)
                } else {
                    Slide::random_point_in_middle_circle(cx, target_pos)
                }
            }
        };
        brain.set_memory_value(
            MemoryModuleType::WalkTarget,
            MemoryValue::WalkTarget(WalkTarget::at_block(
                block_pos_containing_vec(chosen),
                SLIDE_SPEED,
                SLIDE_CLOSE_ENOUGH,
            )),
        );
    }

    fn name(&self) -> &'static str {
        "Slide"
    }
}

/// `BreezeAi$SlideToTargetSink`'s body: a `MoveToTargetSink` whose `start`
/// and `stop` add the breeze's pose and sounds.
#[derive(Debug, Default)]
pub struct SlideToTargetSink(pub MoveToTargetSink);

/// `new BreezeAi$SlideToTargetSink(min, max)`.
#[must_use]
pub fn slide_to_target_sink(min: i32, max: i32) -> Behavior<SlideToTargetSink> {
    MoveToTargetSink::with_durations(min, max).map_body(SlideToTargetSink)
}

impl<M: MobHost> BehaviorBody<M> for SlideToTargetSink {
    fn check_extra_start_conditions(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        self.0.check_extra_start_conditions(brain, cx)
    }

    fn can_still_use(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) -> bool {
        self.0.can_still_use(brain, cx, time)
    }

    fn tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        self.0.tick(brain, cx, time);
    }

    fn timed_out(&self, end_timestamp: i64, time: i64) -> bool {
        <MoveToTargetSink as BehaviorBody<M>>::timed_out(&self.0, end_timestamp, time)
    }

    fn start(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        self.0.start(brain, cx, time);
        cx.mob.play_sound_simple(&BREEZE_SLIDE);
        cx.mob.set_pose(Pose::Sliding);
    }

    fn stop(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        self.0.stop(brain, cx, time);
        cx.mob.set_pose(Pose::Standing);
        if brain.has_memory_value(MemoryModuleType::AttackTarget) {
            brain.set_memory_with_expiry(
                MemoryModuleType::BreezeShoot,
                MemoryValue::Unit,
                SHOOT_MEMORY_TICKS,
            );
        }
    }

    fn name(&self) -> &'static str {
        "SlideToTargetSink"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    fn setup() -> (MobAi<TestMob>, BrainState) {
        let mut mob = TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 7);
        mob.on_ground = true;
        mob.others
            .push((EntityClass::Living, EntityFacts::living(9, 10.5, 1.0, 0.5)));
        let cx = MobAi::new(mob);
        let mut brain = Brain::<TestMob>::new(
            &[
                T::AttackTarget,
                T::WalkTarget,
                T::BreezeJumpCooldown,
                T::BreezeShoot,
                T::CantReachWalkTargetSince,
                T::Path,
            ],
            vec![],
        )
        .state;
        brain.set_memory_value(T::AttackTarget, MemoryValue::Entity(9));
        (cx, brain)
    }

    #[test]
    fn slide_refuses_off_the_ground_in_water_and_out_of_the_standing_pose() {
        for (ground, water, pose, expect) in [
            (false, false, Pose::Standing, false),
            (true, true, Pose::Standing, false),
            (true, false, Pose::Sliding, false),
            (true, false, Pose::Standing, true),
        ] {
            let (mut cx, mut brain) = setup();
            cx.mob.on_ground = ground;
            cx.mob.in_water = water;
            cx.mob.pose = pose;
            let mut b = Slide::new();
            assert_eq!(b.try_start(&mut brain, &mut cx, 0), expect, "{pose:?}");
        }
    }

    #[test]
    fn outside_the_inner_circle_it_flips_a_coin_and_writes_one_walk_target() {
        let (mut cx, mut brain) = setup();
        let mut b = Slide::new();
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(
            brain.has_memory_value(T::WalkTarget),
            "start always ends in a walk target"
        );
        let MemoryValue::WalkTarget(w) = brain.get_memory(T::WalkTarget).unwrap() else {
            panic!("walk target")
        };
        assert_eq!(w.speed_modifier, 0.6);
        assert_eq!(w.close_enough_dist, 1);
    }

    #[test]
    fn the_middle_circle_point_falls_short_of_the_target_by_four_to_eight() {
        let (mut cx, _brain) = setup();
        let target = Vec3::new(30.5, 1.0, 0.5);
        let here = cx.mob.position();
        let v = Slide::random_point_in_middle_circle(&mut cx, target);
        let shortfall = target.distance_to(here) - v.distance_to(here);
        assert!(
            (4.0..=8.0).contains(&shortfall),
            "lerp(nextDouble(), 8.0, 4.0) short of the target, got {shortfall}"
        );
    }

    #[test]
    fn the_sink_sets_sliding_on_start_and_arms_the_shoot_memory_on_stop() {
        let (mut cx, mut brain) = setup();
        brain.set_memory_value(
            T::WalkTarget,
            MemoryValue::WalkTarget(WalkTarget::at_block(
                rustmc_data::BlockPos { x: 4, y: 1, z: 0 },
                0.6,
                1,
            )),
        );
        let mut b = slide_to_target_sink(20, 40);
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(cx.mob.pose, Pose::Sliding);
        assert!(cx
            .mob
            .calls
            .iter()
            .any(|c| c == "playSound(entity.breeze.slide, 1, 1)"));
        BehaviorControl::do_stop(&mut b, &mut brain, &mut cx, 1);
        assert_eq!(cx.mob.pose, Pose::Standing);
        assert_eq!(brain.get_memory(T::BreezeShoot), Some(&MemoryValue::Unit));
        assert_eq!(brain.time_until_expiry(T::BreezeShoot), 60);
    }

    #[test]
    fn the_sink_arms_no_shoot_memory_without_an_attack_target() {
        let (mut cx, mut brain) = setup();
        brain.erase_memory(T::AttackTarget);
        let mut b = slide_to_target_sink(20, 40);
        BehaviorControl::do_stop(&mut b, &mut brain, &mut cx, 1);
        assert_eq!(cx.mob.pose, Pose::Standing, "the pose is reset either way");
        assert!(!brain.has_memory_value(T::BreezeShoot));
    }
}
