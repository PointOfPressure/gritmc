//! `net.minecraft.world.entity.animal.armadillo.ArmadilloAi$1`,
//! `$ArmadilloPanic` and `$ArmadilloBallUp`, from the 26.2 jar.
//!
//! ```text
//! Armadillo$ArmadilloState(String name, boolean isThreatened, int animationDuration, int id)
//!    IDLE("idle", false,  0, 0)   ROLLING("rolling", true, 10, 1)
//!    SCARED("scared", true, 50, 2)   UNROLLING("unrolling", true, 30, 3)
//!
//! ArmadilloAi$1 extends MoveToTargetSink
//!   <init>() (0..4):  super()
//!   checkExtraStartConditions(level, Mob) (0..27):
//!        0  if mob instanceof Armadillo a && a.isScared(): return false
//!       21  return super.checkExtraStartConditions(level, mob)
//!
//! ArmadilloAi$ArmadilloPanic extends AnimalPanic<Armadillo>
//!   <init>(float) (0..10):
//!        super(speedMultiplier, lambda$new$0)   // e -> DamageTypeTags.PANIC_ENVIRONMENTAL_CAUSES
//!   start(level, armadillo, t) (0..11):
//!        0  armadillo.rollOut()
//!        4  super.start(level, armadillo, t)
//!
//! ArmadilloAi$ArmadilloBallUp extends Behavior<Armadillo>
//!   <clinit>: BALL_UP_STAY_IN_STATE = 5 * TimeUtil.SECONDS_PER_MINUTE * 20 = 6000
//!   constants: TICKS_DELAY_TO_DETERMINE_IF_DANGER_IS_STILL_AROUND = 5 (dead),
//!              DANGER_DETECTED_RECENTLY_DANGER_THRESHOLD = 75
//!   fields: int nextPeekTimer = 0; boolean dangerWasAround
//!   <init>() (0..15):  Behavior(Map.of(), 6000) -> (6000, 6000); nextPeekTimer = 0
//!   checkExtraStartConditions (0..4):  armadillo.onGround()
//!   canStillUse (0..7):                armadillo.getState().isThreatened()
//!   start (0..4):                      armadillo.rollUp()
//!   stop (0..11):                      if !armadillo.canStayRolledUp(): armadillo.rollOut()
//!   pickNextPeekTimer(armadillo) (0..21):
//!        SCARED.animationDuration() + random.nextIntBetweenInclusive(100, 400)
//!   tick (0..212):
//!        0  super.tick(...)               // Behavior.tick is empty
//!        7  if nextPeekTimer > 0: nextPeekTimer--
//!       24  if armadillo.shouldSwitchToScaredState():
//!       31     armadillo.switchToState(SCARED)
//!       38     if armadillo.onGround(): armadillo.playSound(ARMADILLO_LAND)
//!       52     return
//!       53  state   = armadillo.getState()
//!       59  ttl     = brain.getTimeUntilExpiry(DANGER_DETECTED_RECENTLY)
//!       71  danger  = ttl > 75L
//!       87  if danger != dangerWasAround: nextPeekTimer = pickNextPeekTimer(armadillo)
//!      105  dangerWasAround = danger
//!      111  if state == SCARED:
//!      119     if nextPeekTimer == 0 && armadillo.onGround() && danger:
//!      138        level.broadcastEntityEvent(armadillo, (byte) 64)
//!      145        nextPeekTimer = pickNextPeekTimer(armadillo)
//!      154     if ttl < UNROLLING.animationDuration():
//!      167        armadillo.playSound(ARMADILLO_UNROLL_START)
//!      174        armadillo.switchToState(UNROLLING)
//!      184  else if state == UNROLLING && ttl > UNROLLING.animationDuration():
//!      205     armadillo.switchToState(SCARED)
//! ```
//!
//! **`ArmadilloBallUp`'s entry map is EMPTY** (`Map.of()`), so nothing about
//! the brain admits or refuses it; the only gates are `onGround()` at start
//! and `getState().isThreatened()` thereafter. Its duration is
//! `5 * 60 * 20 = 6000` ticks, and since `canStillUse` reads the armadillo's
//! own state rather than a memory, that timer is a ceiling nothing normally
//! reaches.
//!
//! **The peek is driven by a CHANGE, not by a level.** `dangerWasAround` is
//! compared for inequality with `danger` at offset 87, so the peek timer is
//! re-picked when danger appears *and* when it disappears — one draw of
//! `nextIntBetweenInclusive(100, 400)` on each edge, plus one more each
//! time a peek actually fires at offset 145. The peek itself is
//! `broadcastEntityEvent(armadillo, 64)`, a client-side animation, and it
//! needs all three of a zero timer, being on the ground, and danger.
//!
//! **AN ABSENT `DANGER_DETECTED_RECENTLY` READS AS MAXIMAL DANGER.**
//! `Brain.getTimeUntilExpiry` is `getMemorySlot(type).timeToLive()`, and
//! `MemorySlot.clear()` (offsets 0..12) puts `timeToLive` back to
//! `NEVER_EXPIRE = Long.MAX_VALUE`. So a memory that has expired, or that
//! was never set, answers `Long.MAX_VALUE` here, `danger` is `true`, and the
//! armadillo stays balled up. `danger` is therefore not "is there danger"
//! but "does the danger memory have more than 75 ticks left, or no
//! countdown at all". Checked from the jar rather than assumed, because the
//! obvious reading is the opposite one.
//!
//! **The same `ttl` drives two opposite transitions with the same constant.**
//! `UNROLLING.animationDuration()` is 30: below it the armadillo starts
//! unrolling, above it an already-unrolling armadillo is snapped back to
//! `SCARED`. The threshold for `danger` itself is a different constant, 75.
//!
//! **`TICKS_DELAY_TO_DETERMINE_IF_DANGER_IS_STILL_AROUND = 5` is dead** — it
//! is declared and never read anywhere in the class.
//!
//! **A null:** `tick` calls `super.tick(...)` first, and `Behavior.tick` is
//! empty, so that call does nothing at all.
//!
//! `ArmadilloPanic`'s `lambda$new$0` ignores its argument and answers
//! `PANIC_ENVIRONMENTAL_CAUSES`, which is the two-argument `AnimalPanic`
//! constructor already transcribed as `AnimalPanic::with_tag`.

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::behaviors::animal_panic::AnimalPanic;
use crate::brain::behaviors::move_to_target_sink::MoveToTargetSink;
use crate::brain::memory::MemoryModuleType;
use crate::brain::BrainState;
use crate::host::{ArmadilloState, DamageTypeTag, MobHost, SoundSpec};
use rustmc_worldgen::rng::RandomSource;

/// `SoundEvents.ARMADILLO_LAND`.
pub const ARMADILLO_LAND: SoundSpec = SoundSpec("entity.armadillo.land");
/// `SoundEvents.ARMADILLO_UNROLL_START`.
pub const ARMADILLO_UNROLL_START: SoundSpec = SoundSpec("entity.armadillo.unroll_start");
/// `BALL_UP_STAY_IN_STATE = 5 * TimeUtil.SECONDS_PER_MINUTE * 20`.
pub const BALL_UP_STAY_IN_STATE: i32 = 5 * 60 * 20;
/// `DANGER_DETECTED_RECENTLY_DANGER_THRESHOLD`.
pub const DANGER_DETECTED_RECENTLY_DANGER_THRESHOLD: i64 = 75;
/// `TICKS_DELAY_TO_DETERMINE_IF_DANGER_IS_STILL_AROUND` — declared in the
/// class and read by nothing; kept so the constant set matches the jar.
pub const TICKS_DELAY_TO_DETERMINE_IF_DANGER_IS_STILL_AROUND: i32 = 5;
/// The `broadcastEntityEvent` id a peek sends.
pub const PEEK_EVENT: u8 = 64;

/// `ArmadilloAi$1`: a `MoveToTargetSink` a scared armadillo refuses.
#[derive(Debug, Default)]
pub struct ArmadilloMoveToTargetSink(pub MoveToTargetSink);

/// `new ArmadilloAi$1()`.
#[must_use]
pub fn armadillo_move_to_target_sink() -> Behavior<ArmadilloMoveToTargetSink> {
    MoveToTargetSink::new().map_body(ArmadilloMoveToTargetSink)
}

impl<M: MobHost> BehaviorBody<M> for ArmadilloMoveToTargetSink {
    fn check_extra_start_conditions(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        if cx.mob.armadillo_is_scared() {
            return false;
        }
        self.0.check_extra_start_conditions(brain, cx)
    }

    fn can_still_use(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) -> bool {
        self.0.can_still_use(brain, cx, time)
    }

    fn start(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        self.0.start(brain, cx, time);
    }

    fn tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        self.0.tick(brain, cx, time);
    }

    fn stop(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        self.0.stop(brain, cx, time);
    }

    fn timed_out(&self, end_timestamp: i64, time: i64) -> bool {
        <MoveToTargetSink as BehaviorBody<M>>::timed_out(&self.0, end_timestamp, time)
    }

    /// `getSimpleName()` of an anonymous class.
    fn name(&self) -> &'static str {
        ""
    }
}

/// `ArmadilloAi$ArmadilloPanic`: an `AnimalPanic` that rolls out first.
#[derive(Debug, Clone, Copy)]
pub struct ArmadilloPanic(pub AnimalPanic);

/// `new ArmadilloAi$ArmadilloPanic(speedMultiplier)`.
#[must_use]
pub fn armadillo_panic(speed_multiplier: f32) -> Behavior<ArmadilloPanic> {
    AnimalPanic::with_tag(speed_multiplier, DamageTypeTag::PanicEnvironmentalCauses)
        .map_body(ArmadilloPanic)
}

impl<M: MobHost> BehaviorBody<M> for ArmadilloPanic {
    fn check_extra_start_conditions(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        self.0.check_extra_start_conditions(brain, cx)
    }

    fn can_still_use(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) -> bool {
        self.0.can_still_use(brain, cx, time)
    }

    fn start(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        cx.mob.armadillo_roll_out();
        self.0.start(brain, cx, time);
    }

    fn tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        self.0.tick(brain, cx, time);
    }

    fn stop(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        self.0.stop(brain, cx, time);
    }

    fn timed_out(&self, end_timestamp: i64, time: i64) -> bool {
        <AnimalPanic as BehaviorBody<M>>::timed_out(&self.0, end_timestamp, time)
    }

    fn name(&self) -> &'static str {
        "ArmadilloPanic"
    }
}

/// `ArmadilloAi$ArmadilloBallUp`'s body.
#[derive(Debug, Clone, Copy, Default)]
pub struct ArmadilloBallUp {
    /// `nextPeekTimer`, initialised to 0 by the constructor.
    pub next_peek_timer: i32,
    /// `dangerWasAround`, left at Java's default `false`.
    pub danger_was_around: bool,
}

impl ArmadilloBallUp {
    /// `new ArmadilloAi$ArmadilloBallUp()`.
    #[must_use]
    pub fn new() -> Behavior<ArmadilloBallUp> {
        Behavior::with_duration(
            ArmadilloBallUp::default(),
            Vec::new(),
            BALL_UP_STAY_IN_STATE,
        )
    }

    /// `pickNextPeekTimer(Armadillo)` (0..21) — one draw.
    fn pick_next_peek_timer<M: MobHost>(cx: &mut MobAi<M>) -> i32 {
        ArmadilloState::Scared.animation_duration()
            + cx.mob.random().next_i32_between_inclusive(100, 400)
    }
}

impl<M: MobHost> BehaviorBody<M> for ArmadilloBallUp {
    fn check_extra_start_conditions(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        cx.mob.on_ground()
    }

    fn can_still_use(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        cx.mob.armadillo_state().is_threatened()
    }

    fn start(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        cx.mob.armadillo_roll_up();
    }

    fn stop(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        if !cx.mob.can_stay_rolled_up() {
            cx.mob.armadillo_roll_out();
        }
    }

    fn tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        if self.next_peek_timer > 0 {
            self.next_peek_timer -= 1;
        }
        if cx.mob.armadillo_should_switch_to_scared_state() {
            cx.mob.armadillo_switch_to_state(ArmadilloState::Scared);
            if cx.mob.on_ground() {
                cx.mob.play_sound_simple(&ARMADILLO_LAND);
            }
            return;
        }
        let state = cx.mob.armadillo_state();
        let ttl = brain.time_until_expiry(MemoryModuleType::DangerDetectedRecently);
        let danger = ttl > DANGER_DETECTED_RECENTLY_DANGER_THRESHOLD;
        if danger != self.danger_was_around {
            self.next_peek_timer = ArmadilloBallUp::pick_next_peek_timer(cx);
        }
        self.danger_was_around = danger;
        if state == ArmadilloState::Scared {
            if self.next_peek_timer == 0 && cx.mob.on_ground() && danger {
                cx.mob.broadcast_entity_event(PEEK_EVENT);
                self.next_peek_timer = ArmadilloBallUp::pick_next_peek_timer(cx);
            }
            if ttl < i64::from(ArmadilloState::Unrolling.animation_duration()) {
                cx.mob.play_sound_simple(&ARMADILLO_UNROLL_START);
                cx.mob.armadillo_switch_to_state(ArmadilloState::Unrolling);
            }
        } else if state == ArmadilloState::Unrolling
            && ttl > i64::from(ArmadilloState::Unrolling.animation_duration())
        {
            cx.mob.armadillo_switch_to_state(ArmadilloState::Scared);
        }
    }

    fn name(&self) -> &'static str {
        "ArmadilloBallUp"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::{MemoryModuleType as T, MemoryValue};
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};

    fn setup() -> (MobAi<TestMob>, BrainState) {
        let mut mob = TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 11);
        mob.on_ground = true;
        let cx = MobAi::new(mob);
        let brain = Brain::<TestMob>::new(
            &[T::DangerDetectedRecently, T::IsPanicking, T::HurtBy],
            vec![],
        )
        .state;
        (cx, brain)
    }

    #[test]
    fn the_state_table_is_the_jars() {
        for (s, threatened, duration, id) in [
            (ArmadilloState::Idle, false, 0, 0),
            (ArmadilloState::Rolling, true, 10, 1),
            (ArmadilloState::Scared, true, 50, 2),
            (ArmadilloState::Unrolling, true, 30, 3),
        ] {
            assert_eq!(s.is_threatened(), threatened, "{s:?}");
            assert_eq!(s.animation_duration(), duration, "{s:?}");
            assert_eq!(s.id(), id, "{s:?}");
        }
    }

    #[test]
    fn ball_up_has_an_empty_entry_map_and_a_six_thousand_tick_ceiling() {
        let (mut cx, mut brain) = setup();
        let mut b = ArmadilloBallUp::new();
        assert!(b.entry_condition.is_empty(), "Map.of()");
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(b.end_timestamp(), 6000);
        assert_eq!(cx.mob.calls, vec!["armadillo_roll_up".to_string()]);
        // Off the ground it never starts.
        let (mut cx, mut brain) = setup();
        cx.mob.on_ground = false;
        let mut b = ArmadilloBallUp::new();
        assert!(!b.try_start(&mut brain, &mut cx, 0));
    }

    #[test]
    fn the_peek_timer_is_re_picked_on_both_edges_of_danger() {
        let (mut cx, mut brain) = setup();
        cx.mob.armadillo_state = ArmadilloState::Scared;
        let mut b = ArmadilloBallUp::new();
        assert!(b.try_start(&mut brain, &mut cx, 0));

        // A memory with 50 ticks left is BELOW the 75 threshold, so `danger`
        // is false; the first tick sees no edge (dangerWasAround starts
        // false) and leaves the timer alone. Note this is also the branch
        // that starts the unroll, since 50 > UNROLLING's 30.
        brain.set_memory_with_expiry(T::DangerDetectedRecently, MemoryValue::Unit, 50);
        b.body.tick(&mut brain, &mut cx, 1);
        assert!(!b.body.danger_was_around);
        assert_eq!(b.body.next_peek_timer, 0, "no edge, no draw");

        // Rising edge: more than 75 ticks left.
        cx.mob.armadillo_state = ArmadilloState::Scared;
        brain.set_memory_with_expiry(T::DangerDetectedRecently, MemoryValue::Unit, 200);
        b.body.tick(&mut brain, &mut cx, 2);
        let after_rise = b.body.next_peek_timer;
        assert!(
            (150..=450).contains(&after_rise),
            "SCARED.animationDuration() 50 + nextIntBetweenInclusive(100, 400), got {after_rise}"
        );
        assert!(b.body.danger_was_around);

        // No edge: the timer only counts down.
        b.body.tick(&mut brain, &mut cx, 3);
        assert_eq!(b.body.next_peek_timer, after_rise - 1);

        // Falling edge: back below 75. The timer is re-picked, not cleared.
        brain.set_memory_with_expiry(T::DangerDetectedRecently, MemoryValue::Unit, 50);
        b.body.tick(&mut brain, &mut cx, 4);
        assert!(!b.body.danger_was_around);
        assert!((150..=450).contains(&b.body.next_peek_timer));
    }

    #[test]
    fn an_absent_danger_memory_reads_as_maximal_danger() {
        let (mut cx, mut brain) = setup();
        cx.mob.armadillo_state = ArmadilloState::Scared;
        let mut b = ArmadilloBallUp::new();
        assert!(
            !brain.has_memory_value(T::DangerDetectedRecently),
            "never set"
        );
        assert_eq!(
            brain.time_until_expiry(T::DangerDetectedRecently),
            i64::MAX,
            "MemorySlot.clear() puts timeToLive back to NEVER_EXPIRE"
        );
        b.body.tick(&mut brain, &mut cx, 1);
        assert!(
            b.body.danger_was_around,
            "an absent memory is above the 75 threshold, so the armadillo stays balled up"
        );
        assert_eq!(
            cx.mob.armadillo_state,
            ArmadilloState::Scared,
            "and it does NOT start unrolling"
        );
    }

    #[test]
    fn a_peek_needs_a_zero_timer_the_ground_and_danger_all_three() {
        let (mut cx, mut brain) = setup();
        cx.mob.armadillo_state = ArmadilloState::Scared;
        let mut b = ArmadilloBallUp::new();
        // Danger present from the start so the first tick sees the edge and
        // picks a timer, which is what we then zero.
        brain.set_memory_with_expiry(T::DangerDetectedRecently, MemoryValue::Unit, 200);
        b.body.tick(&mut brain, &mut cx, 1);
        b.body.next_peek_timer = 0;
        cx.mob.calls.clear();
        b.body.tick(&mut brain, &mut cx, 2);
        assert!(cx.mob.calls.iter().any(|c| c == "broadcastEntityEvent(64)"));
        assert!(b.body.next_peek_timer > 0, "re-picked after the peek");

        // On the ground is required.
        let (mut cx, mut brain) = setup();
        cx.mob.armadillo_state = ArmadilloState::Scared;
        cx.mob.on_ground = false;
        brain.set_memory_with_expiry(T::DangerDetectedRecently, MemoryValue::Unit, 200);
        let mut b = ArmadilloBallUp::new();
        b.body.tick(&mut brain, &mut cx, 1);
        b.body.next_peek_timer = 0;
        cx.mob.calls.clear();
        b.body.tick(&mut brain, &mut cx, 2);
        assert!(!cx.mob.calls.iter().any(|c| c == "broadcastEntityEvent(64)"));
    }

    #[test]
    fn the_same_thirty_tick_constant_unrolls_and_re_scares() {
        // ttl below 30 while SCARED: start unrolling.
        let (mut cx, mut brain) = setup();
        cx.mob.armadillo_state = ArmadilloState::Scared;
        brain.set_memory_with_expiry(T::DangerDetectedRecently, MemoryValue::Unit, 20);
        let mut b = ArmadilloBallUp::new();
        b.body.tick(&mut brain, &mut cx, 1);
        assert_eq!(cx.mob.armadillo_state, ArmadilloState::Unrolling);

        // ttl above 30 while UNROLLING: back to scared.
        let (mut cx, mut brain) = setup();
        cx.mob.armadillo_state = ArmadilloState::Unrolling;
        brain.set_memory_with_expiry(T::DangerDetectedRecently, MemoryValue::Unit, 200);
        let mut b = ArmadilloBallUp::new();
        b.body.tick(&mut brain, &mut cx, 1);
        assert_eq!(cx.mob.armadillo_state, ArmadilloState::Scared);
    }

    #[test]
    fn should_switch_to_scared_short_circuits_the_whole_rest_of_tick() {
        let (mut cx, mut brain) = setup();
        cx.mob.armadillo_should_switch_to_scared = true;
        cx.mob.armadillo_state = ArmadilloState::Unrolling;
        brain.set_memory_with_expiry(T::DangerDetectedRecently, MemoryValue::Unit, 200);
        let mut b = ArmadilloBallUp::new();
        b.body.tick(&mut brain, &mut cx, 1);
        assert_eq!(cx.mob.armadillo_state, ArmadilloState::Scared);
        assert_eq!(
            cx.mob.calls,
            vec![
                "armadillo_switch_to_state Scared".to_string(),
                "playSound(entity.armadillo.land, 1, 1)".to_string(),
            ]
        );
        assert_eq!(b.body.next_peek_timer, 0, "the edge check never ran");
    }

    #[test]
    fn stop_rolls_out_only_when_it_cannot_stay_rolled_up() {
        for (can_stay, expect) in [(true, 0), (false, 1)] {
            let (mut cx, mut brain) = setup();
            cx.mob.can_stay_rolled_up = can_stay;
            let mut b = ArmadilloBallUp::new();
            assert!(b.try_start(&mut brain, &mut cx, 0));
            cx.mob.calls.clear();
            BehaviorControl::do_stop(&mut b, &mut brain, &mut cx, 1);
            assert_eq!(
                cx.mob
                    .calls
                    .iter()
                    .filter(|c| *c == "armadillo_roll_out")
                    .count(),
                expect
            );
        }
    }

    #[test]
    fn the_scared_armadillo_refuses_the_move_sink_and_the_wrapper_is_anonymous() {
        let (mut cx, mut brain) = setup();
        let mut b = armadillo_move_to_target_sink();
        assert_eq!(BehaviorControl::<TestMob>::debug_string(&b), "");
        cx.mob.armadillo_scared = true;
        assert!(!b.try_start(&mut brain, &mut cx, 0));
    }

    #[test]
    fn the_panic_subclass_rolls_out_before_the_superclass_start() {
        let (mut cx, mut brain) = setup();
        let mut b = armadillo_panic(1.5);
        assert_eq!(
            b.body.0.panic_causing_damage_types,
            DamageTypeTag::PanicEnvironmentalCauses,
            "lambda$new$0 answers PANIC_ENVIRONMENTAL_CAUSES, not PANIC_CAUSES"
        );
        // `AnimalPanic.checkExtraStartConditions` is `hurt || IS_PANICKING`;
        // the second disjunct is the one that does not depend on the tag.
        brain.set_memory_value(T::IsPanicking, MemoryValue::Bool(true));
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(
            cx.mob.calls.first().map(String::as_str),
            Some("armadillo_roll_out")
        );
    }
}
