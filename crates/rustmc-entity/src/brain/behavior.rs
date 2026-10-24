//! `net.minecraft.world.entity.ai.behavior.{BehaviorControl, Behavior,
//! Behavior$Status, OneShot}` and the `declarative` package's semantics,
//! from the 26.2 jar.
//!
//! # The three shapes
//!
//! * [`BehaviorControl`] is the interface `Brain` drives:
//!   `getStatus / getRequiredMemories / tryStart / tickOrStop / doStop`.
//! * [`Behavior`] is the timed base class. `tryStart` (offsets 0..70):
//!   `hasRequiredMemories(mob) && checkExtraStartConditions(level, mob)`,
//!   then `status = RUNNING`, **`endTimestamp = time + minDuration +
//!   level.getRandom().nextInt(maxDuration + 1 - minDuration)`** — one
//!   draw from the LEVEL's random, not the mob's, on every successful
//!   start — then `start(level, mob, time)`. `tickOrStop` (0..35):
//!   `!timedOut(time) && canStillUse(level, mob, time) ? tick(...) : doStop(...)`.
//!   `doStop`: `status = STOPPED; stop(...)`. `timedOut(t)`: `t > endTimestamp`.
//!   `DEFAULT_DURATION = 60`; `Behavior(map)` is `(map, 60, 60)`, `(map, n)`
//!   is `(map, n, n)`, so a fixed-duration behaviour still makes the draw
//!   (`nextInt(1)`, which consumes a `next(31)` and returns 0).
//! * [`OneShot`] runs `trigger(level, mob, time)` in `tryStart`; on `true`
//!   it is `RUNNING` for exactly one brain tick, since `tickOrStop` is
//!   `doStop`. It draws nothing.
//!
//! # `hasRequiredMemories`
//!
//! `Behavior.entryCondition` is an `ImmutableMap<MemoryModuleType,
//! MemoryStatus>` walked in insertion order; every entry must pass
//! `brain.checkMemory(type, status)`. Since the checks are pure, order
//! cannot change the answer, so a `Vec` of pairs is the whole port.
//!
//! # The declarative builder (`BehaviorBuilder.create(...)`)
//!
//! Most one-shots in the jar are `BehaviorBuilder` lambdas:
//! `i.group(i.present(A), i.absent(B), i.registered(C)).apply(i, (a, b, c) ->
//! (level, mob, time) -> {...})`. `Instance$1..$5.tryTrigger` evaluate the
//! group's conditions **left to right** and return `null` on the first
//! failure, then run the body. Each condition is `PureMemory$1.tryTrigger`:
//! `brain.getMemoryInternal(type)` — which is `null` when the type is not
//! registered in this brain, failing even an `absent(...)` — then
//! `MemoryCondition.createAccessor`: `Present` fails on an empty optional,
//! `Absent` on a present one, `Registered` never. The reads are pure, so a
//! hand-written trigger that checks the same conditions in any order is the
//! same function; [`crate::brain::BrainState::memory_internal`] is the
//! `getMemoryInternal` it needs, and `getRequiredMemories()` for such a
//! one-shot is the union of the group's types, which is what `Brain.addActivity`
//! registers.

use crate::ai::mob_ai::MobAi;
use crate::brain::memory::{MemoryModuleType, MemoryStatus};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `Behavior.DEFAULT_DURATION`.
pub const DEFAULT_DURATION: i32 = 60;

/// `Behavior$Status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Status {
    Stopped,
    Running,
}

/// `BehaviorControl<E>`. `E` is the mob, reached through [`MobAi`]; the
/// brain state is passed beside it because vanilla reaches it through
/// `mob.getBrain()` and this port keeps the two apart to satisfy the
/// borrow checker, not because they differ in kind.
pub trait BehaviorControl<M: MobHost> {
    /// `getStatus()`.
    fn status(&self) -> Status;
    /// `getRequiredMemories()` — a `Set`, so duplicates are harmless.
    fn required_memories(&self) -> Vec<MemoryModuleType>;
    /// `tryStart(level, mob, gameTime)`.
    fn try_start(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) -> bool;
    /// `tickOrStop(level, mob, gameTime)`.
    fn tick_or_stop(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64);
    /// `doStop(level, mob, gameTime)`.
    fn do_stop(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64);
    /// `debugString()` — the class's simple name.
    fn debug_string(&self) -> &'static str;
}

/// The overridable half of `Behavior<E>`: what a subclass supplies.
pub trait BehaviorBody<M: MobHost> {
    /// `checkExtraStartConditions(level, mob)`; the base answers `true`.
    fn check_extra_start_conditions(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        let _ = (brain, cx);
        true
    }
    /// `start(level, mob, time)`; the base does nothing.
    fn start(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        let _ = (brain, cx, time);
    }
    /// `tick(level, mob, time)`; the base does nothing.
    fn tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        let _ = (brain, cx, time);
    }
    /// `stop(level, mob, time)`; the base does nothing.
    fn stop(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        let _ = (brain, cx, time);
    }
    /// The same `stop(level, mob, time)`, for the subclasses whose body
    /// calls **`this.timedOut(time)`** — `Behavior.timedOut` reads the
    /// base's private `endTimestamp`, which a subclass reaches through
    /// inheritance and this port's separate body cannot. Overriding this
    /// instead of [`BehaviorBody::stop`] is the whole difference; the
    /// default forwards, so nothing else changes.
    ///
    /// `end_timestamp` is the field as `try_start` last set it, so
    /// `end_timestamp` compared against `time` here is exactly
    /// `timedOut(time)`.
    fn stop_timed(
        &mut self,
        brain: &mut BrainState,
        cx: &mut MobAi<M>,
        time: i64,
        end_timestamp: i64,
    ) {
        let _ = end_timestamp;
        self.stop(brain, cx, time);
    }
    /// `canStillUse(level, mob, time)`; the base answers `false`.
    fn can_still_use(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) -> bool {
        let _ = (brain, cx, time);
        false
    }
    /// `timedOut(time)`; the base is `time > endTimestamp`. A subclass that
    /// overrides it (there are some) gets the end timestamp to compare.
    fn timed_out(&self, end_timestamp: i64, time: i64) -> bool {
        time > end_timestamp
    }
    /// The class's simple name, for `debugString()`.
    fn name(&self) -> &'static str;
}

/// `Behavior<E>`: the timed base class around a [`BehaviorBody`].
pub struct Behavior<B> {
    /// The subclass.
    pub body: B,
    /// `entryCondition`, in the `ImmutableMap`'s insertion order.
    pub entry_condition: Vec<(MemoryModuleType, MemoryStatus)>,
    status: Status,
    end_timestamp: i64,
    min_duration: i32,
    max_duration: i32,
}

impl<B> Behavior<B> {
    /// `Behavior(Map)` — `(map, 60, 60)`.
    #[must_use]
    pub fn new(body: B, entry_condition: Vec<(MemoryModuleType, MemoryStatus)>) -> Self {
        Self::with_durations(body, entry_condition, DEFAULT_DURATION, DEFAULT_DURATION)
    }

    /// `Behavior(Map, int)` — `(map, n, n)`.
    #[must_use]
    pub fn with_duration(
        body: B,
        entry_condition: Vec<(MemoryModuleType, MemoryStatus)>,
        duration: i32,
    ) -> Self {
        Self::with_durations(body, entry_condition, duration, duration)
    }

    /// `Behavior(Map, int, int)`.
    #[must_use]
    pub fn with_durations(
        body: B,
        entry_condition: Vec<(MemoryModuleType, MemoryStatus)>,
        min_duration: i32,
        max_duration: i32,
    ) -> Self {
        Behavior {
            body,
            entry_condition,
            status: Status::Stopped,
            end_timestamp: 0,
            min_duration,
            max_duration,
        }
    }

    /// `endTimestamp`, for tests.
    #[must_use]
    pub fn end_timestamp(&self) -> i64 {
        self.end_timestamp
    }

    /// Re-wrap this behaviour's body, keeping the entry condition and both
    /// durations exactly as the superclass constructor set them.
    ///
    /// This is how an **anonymous subclass** is expressed here. In the jar
    /// `SnifferAi$1 extends AnimalPanic` is one object: the superclass
    /// constructor runs and fills `entryCondition`, `minDuration` and
    /// `maxDuration`, and the subclass only overrides methods. Building
    /// `AnimalPanic::new(2.0)` and mapping its body is the same object,
    /// with the same three fields, by the same constructor.
    #[must_use]
    pub fn map_body<T>(self, f: impl FnOnce(B) -> T) -> Behavior<T> {
        Behavior {
            body: f(self.body),
            entry_condition: self.entry_condition,
            status: self.status,
            end_timestamp: self.end_timestamp,
            min_duration: self.min_duration,
            max_duration: self.max_duration,
        }
    }

    /// `hasRequiredMemories(mob)`: every `entryCondition` entry passes
    /// `brain.checkMemory`.
    #[must_use]
    pub fn has_required_memories(&self, brain: &BrainState) -> bool {
        self.entry_condition
            .iter()
            .all(|(t, s)| brain.check_memory(*t, *s))
    }
}

impl<M: MobHost, B: BehaviorBody<M>> BehaviorControl<M> for Behavior<B> {
    fn status(&self) -> Status {
        self.status
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        self.entry_condition.iter().map(|(t, _)| *t).collect()
    }

    fn try_start(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) -> bool {
        if self.has_required_memories(brain) && self.body.check_extra_start_conditions(brain, cx) {
            self.status = Status::Running;
            // Java int arithmetic: `maxDuration + 1 - minDuration` wraps, so a
            // fixed `Integer.MAX_VALUE` duration draws `nextInt(1)`.
            let span = cx.mob.level_random_next_int(
                self.max_duration
                    .wrapping_add(1)
                    .wrapping_sub(self.min_duration),
            );
            let duration = self.min_duration + span;
            self.end_timestamp = time + i64::from(duration);
            self.body.start(brain, cx, time);
            true
        } else {
            false
        }
    }

    fn tick_or_stop(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        if !self.body.timed_out(self.end_timestamp, time)
            && self.body.can_still_use(brain, cx, time)
        {
            self.body.tick(brain, cx, time);
        } else {
            self.do_stop(brain, cx, time);
        }
    }

    fn do_stop(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        self.status = Status::Stopped;
        self.body.stop_timed(brain, cx, time, self.end_timestamp);
    }

    fn debug_string(&self) -> &'static str {
        self.body.name()
    }
}

/// `declarative.Trigger<E>`: `boolean trigger(level, mob, time)`, plus the
/// memory set a `BehaviorBuilder`-made one-shot reports.
pub trait Trigger<M: MobHost> {
    /// `trigger(level, mob, time)`.
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) -> bool;
    /// `getRequiredMemories()` of the resulting `OneShot` — the union of
    /// the builder group's types, or empty for a bare `point(...)`.
    fn required_memories(&self) -> Vec<MemoryModuleType> {
        Vec::new()
    }
    /// The class's simple name, for `debugString()`.
    fn name(&self) -> &'static str;
}

/// `OneShot<E>` around a [`Trigger`].
pub struct OneShot<T> {
    /// The trigger.
    pub trigger: T,
    status: Status,
}

impl<T> OneShot<T> {
    #[must_use]
    pub fn new(trigger: T) -> Self {
        OneShot {
            trigger,
            status: Status::Stopped,
        }
    }
}

impl<M: MobHost, T: Trigger<M>> BehaviorControl<M> for OneShot<T> {
    fn status(&self) -> Status {
        self.status
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        self.trigger.required_memories()
    }

    fn try_start(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) -> bool {
        if self.trigger.trigger(brain, cx, time) {
            self.status = Status::Running;
            true
        } else {
            false
        }
    }

    fn tick_or_stop(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        self.do_stop(brain, cx, time);
    }

    fn do_stop(&mut self, _brain: &mut BrainState, _cx: &mut MobAi<M>, _time: i64) {
        self.status = Status::Stopped;
    }

    fn debug_string(&self) -> &'static str {
        self.trigger.name()
    }
}

/// `BehaviorBuilder.triggerIf(Predicate<E>)`: a one-shot whose trigger is
/// the predicate and which requires no memories.
pub struct TriggerIf<F> {
    pub predicate: F,
}

impl<M: MobHost, F: FnMut(&mut BrainState, &mut MobAi<M>) -> bool> Trigger<M> for TriggerIf<F> {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        (self.predicate)(brain, cx)
    }

    fn name(&self) -> &'static str {
        "TriggerIf"
    }
}

/// `BehaviorBuilder.sequence(Trigger, OneShot)`: `first.trigger(...) &&
/// second.trigger(...)`, requiring both's memories (the first's are reported
/// through the wrapped builder's group; a bare trigger reports none).
pub struct Sequence<A, B> {
    pub first: A,
    pub second: B,
}

impl<M: MobHost, A: Trigger<M>, B: Trigger<M>> Trigger<M> for Sequence<A, B> {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) -> bool {
        // `BehaviorBuilder.sequence` builds `create(i -> i.group(i.ifTriggered(first))
        // .apply(i, unit -> second::trigger))`: the first trigger runs as a
        // condition, and only on `true` does the body — the second — run.
        self.first.trigger(brain, cx, time) && self.second.trigger(brain, cx, time)
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        let mut v = self.first.required_memories();
        v.extend(self.second.required_memories());
        v
    }

    fn name(&self) -> &'static str {
        "OneShot[stuff]"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};
    use rustmc_worldgen::rng::{LegacyRandomSource, RandomSource};

    struct Counter {
        starts: u32,
        ticks: u32,
        stops: u32,
        can_use: bool,
    }

    impl BehaviorBody<TestMob> for Counter {
        fn start(&mut self, _b: &mut BrainState, _cx: &mut MobAi<TestMob>, _t: i64) {
            self.starts += 1;
        }
        fn tick(&mut self, _b: &mut BrainState, _cx: &mut MobAi<TestMob>, _t: i64) {
            self.ticks += 1;
        }
        fn stop(&mut self, _b: &mut BrainState, _cx: &mut MobAi<TestMob>, _t: i64) {
            self.stops += 1;
        }
        fn can_still_use(
            &mut self,
            _b: &mut BrainState,
            _cx: &mut MobAi<TestMob>,
            _t: i64,
        ) -> bool {
            self.can_use
        }
        fn name(&self) -> &'static str {
            "Counter"
        }
    }

    fn cx() -> MobAi<TestMob> {
        MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 7))
    }

    #[test]
    fn timed_behavior_draws_its_duration_from_the_level_random_and_runs_until_it() {
        let mut cx = cx();
        let mut brain = Brain::<TestMob>::new(&[T::LookTarget], vec![]).state;
        let mut b = Behavior::with_durations(
            Counter {
                starts: 0,
                ticks: 0,
                stops: 0,
                can_use: true,
            },
            vec![(T::LookTarget, MemoryStatus::Registered)],
            5,
            10,
        );
        // The level random is `LegacyRandomSource(seed ^ 0x5DEECE66D)` in the
        // fixture; a twin says what nextInt(6) — maxDuration + 1 - minDuration — draws.
        let mut twin = LegacyRandomSource::new(7 ^ 0x5DEECE66D);
        let span = twin.next_i32_bound(6);
        assert!(b.try_start(&mut brain, &mut cx, 100));
        assert_eq!(BehaviorControl::<TestMob>::status(&b), Status::Running);
        assert_eq!(b.end_timestamp(), 100 + 5 + i64::from(span));
        assert_eq!(
            cx.mob.level_random.next_i32(),
            twin.next_i32(),
            "exactly one level draw"
        );
        let end = b.end_timestamp();
        b.tick_or_stop(&mut brain, &mut cx, end);
        assert_eq!(
            (b.body.ticks, b.body.stops),
            (1, 0),
            "time == endTimestamp still ticks"
        );
        b.tick_or_stop(&mut brain, &mut cx, end + 1);
        assert_eq!(
            (b.body.ticks, b.body.stops),
            (1, 1),
            "time > endTimestamp stops"
        );
        assert_eq!(BehaviorControl::<TestMob>::status(&b), Status::Stopped);
    }

    #[test]
    fn a_missing_entry_condition_refuses_without_drawing() {
        let mut cx = cx();
        let mut brain = Brain::<TestMob>::new(&[T::LookTarget], vec![]).state;
        let mut b = Behavior::new(
            Counter {
                starts: 0,
                ticks: 0,
                stops: 0,
                can_use: true,
            },
            vec![(T::LookTarget, MemoryStatus::ValuePresent)],
        );
        let mut twin = LegacyRandomSource::new(7 ^ 0x5DEECE66D);
        assert!(!b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(cx.mob.level_random.next_i32(), twin.next_i32(), "no draw");
        assert_eq!(b.body.starts, 0);
    }

    #[test]
    fn one_shot_runs_for_exactly_one_tick_and_draws_nothing() {
        let mut cx = cx();
        let mut brain = Brain::<TestMob>::new(&[], vec![]).state;
        let mut fired = 0;
        let mut o = OneShot::new(TriggerIf {
            predicate: |_: &mut BrainState, _: &mut MobAi<TestMob>| {
                fired += 1;
                true
            },
        });
        let mut twin = LegacyRandomSource::new(7 ^ 0x5DEECE66D);
        assert!(o.try_start(&mut brain, &mut cx, 0));
        assert_eq!(BehaviorControl::<TestMob>::status(&o), Status::Running);
        o.tick_or_stop(&mut brain, &mut cx, 1);
        assert_eq!(BehaviorControl::<TestMob>::status(&o), Status::Stopped);
        assert_eq!(cx.mob.level_random.next_i32(), twin.next_i32(), "no draw");
        let _ = o;
        assert_eq!(fired, 1);
    }
}
