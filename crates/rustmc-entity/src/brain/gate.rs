//! `ShufflingList`, `GateBehavior` (+ `OrderPolicy`, `RunningPolicy`),
//! `RunOne` and `TriggerGate` from the 26.2 jar.
//!
//! # `ShufflingList<U>`
//!
//! `entries: List<WeightedEntry>` in add order and **its own
//! `RandomSource.create()`** — seeded from `RandomSupport.generateUniqueSeed()`,
//! i.e. from the wall clock and a counter, so a vanilla shuffle is not
//! reproducible from any game state. This port takes the seed as a
//! constructor argument and says so; a test can pin it, a server wiring a
//! mob owes it a seed and cannot get vanilla's.
//!
//! `shuffle()` (0..33): for every entry in order `setRandom(random.nextFloat())`
//! — **one `nextFloat` per entry, in list order** — where
//! `setRandom(f)` is `randWeight = -Math.pow((double) f, (double) (1.0f / (float) weight))`
//! (offsets 0..18, note the `f2d` of a `float` quotient); then a stable
//! `List.sort(comparingDouble(getRandWeight))`, ascending, so the largest
//! `pow` comes first. `Math.pow` is an intrinsic within 1 ulp; two entries
//! whose keys differ by less than that could sort the other way from
//! HotSpot. Not observed; noted.
//!
//! # `GateBehavior<E>`
//!
//! ```text
//! tryStart (0..46):   if !hasRequiredMemories(mob) return false
//!                     status = RUNNING; orderPolicy.apply(behaviors); runningPolicy.apply(stream, ...); return true
//! ORDERED:            nothing;  SHUFFLED: behaviors.shuffle()
//! RUN_ONE:            filter STOPPED, filter tryStart(...) -> findFirst(): stops at the first that starts
//! TRY_ALL:            filter STOPPED, forEach tryStart(...)
//! tickOrStop (0..57): every RUNNING child: tickOrStop; then if none RUNNING: doStop
//! doStop (0..60):     status = STOPPED; every RUNNING child: doStop; every exitErasedMemory: brain.eraseMemory
//! getRequiredMemories: entryCondition keys ∪ every child's
//! ```
//!
//! `RunOne(list)` is `GateBehavior(ImmutableMap.of(), ImmutableSet.of(), SHUFFLED, RUN_ONE, list)`;
//! `RunOne(map, list)` keeps the map as the entry condition.
//!
//! # `TriggerGate`
//!
//! `triggerGate(list, order, running)` is a `BehaviorBuilder` one-shot
//! whose trigger (`lambda$triggerGate$2`, 0..69) is: `if order == SHUFFLED:
//! list.shuffle()`; then for each trigger in list order: `if trigger(...)
//! && running == RUN_ONE: break`; **always returns `true`**, so the
//! one-shot counts as started whether or not any child fired.
//! `triggerOneShuffled(list)` is `(SHUFFLED, RUN_ONE)`.

use rustmc_worldgen::rng::{LegacyRandomSource, RandomSource};

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{BehaviorControl, Status, Trigger};
use crate::brain::memory::{MemoryModuleType, MemoryStatus};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `ShufflingList$WeightedEntry<T>`.
#[derive(Debug, Clone)]
pub struct WeightedEntry<T> {
    pub data: T,
    pub weight: i32,
    /// `randWeight`, `0.0` until the first `setRandom`.
    pub rand_weight: f64,
}

impl<T> WeightedEntry<T> {
    /// `setRandom(float)`.
    fn set_random(&mut self, f: f32) {
        #[allow(clippy::cast_precision_loss)]
        let exp = 1.0f32 / (self.weight as f32);
        self.rand_weight = -f64::from(f).powf(f64::from(exp));
    }
}

/// `ShufflingList<U>`.
#[derive(Debug, Clone)]
pub struct ShufflingList<T> {
    /// `entries`.
    pub entries: Vec<WeightedEntry<T>>,
    /// `random` — vanilla's is `RandomSource.create()`, unseeded.
    random: LegacyRandomSource,
}

impl<T> ShufflingList<T> {
    /// `ShufflingList()` with the seed vanilla does not let anyone choose.
    #[must_use]
    pub fn new(seed: i64) -> Self {
        ShufflingList {
            entries: Vec::new(),
            random: LegacyRandomSource::new(seed),
        }
    }

    /// `add(U, int)`.
    pub fn add(&mut self, data: T, weight: i32) -> &mut Self {
        self.entries.push(WeightedEntry {
            data,
            weight,
            rand_weight: 0.0,
        });
        self
    }

    /// `shuffle()`.
    pub fn shuffle(&mut self) -> &mut Self {
        for e in &mut self.entries {
            let f = self.random.next_f32();
            e.set_random(f);
        }
        self.entries
            .sort_by(|a, b| a.rand_weight.total_cmp(&b.rand_weight));
        self
    }

    /// `iterator()` / `stream()`: the data in current order.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.entries.iter().map(|e| &e.data)
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.entries.iter_mut().map(|e| &mut e.data)
    }
}

/// `GateBehavior$OrderPolicy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderPolicy {
    Ordered,
    Shuffled,
}

/// `GateBehavior$RunningPolicy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunningPolicy {
    RunOne,
    TryAll,
}

/// `GateBehavior<E>`.
pub struct GateBehavior<M: MobHost> {
    /// `entryCondition`.
    pub entry_condition: Vec<(MemoryModuleType, MemoryStatus)>,
    /// `exitErasedMemories`.
    pub exit_erased_memories: Vec<MemoryModuleType>,
    pub order_policy: OrderPolicy,
    pub running_policy: RunningPolicy,
    /// `behaviors`.
    pub behaviors: ShufflingList<Box<dyn BehaviorControl<M>>>,
    status: Status,
}

impl<M: MobHost> GateBehavior<M> {
    /// `GateBehavior(entryCondition, exitErasedMemories, orderPolicy,
    /// runningPolicy, List<Pair<BehaviorControl, Integer>>)`; `seed` is the
    /// shuffling list's (see the module doc).
    #[must_use]
    pub fn new(
        entry_condition: Vec<(MemoryModuleType, MemoryStatus)>,
        exit_erased_memories: Vec<MemoryModuleType>,
        order_policy: OrderPolicy,
        running_policy: RunningPolicy,
        behaviors: Vec<(Box<dyn BehaviorControl<M>>, i32)>,
        seed: i64,
    ) -> Self {
        let mut list = ShufflingList::new(seed);
        for (b, w) in behaviors {
            list.add(b, w);
        }
        GateBehavior {
            entry_condition,
            exit_erased_memories,
            order_policy,
            running_policy,
            behaviors: list,
            status: Status::Stopped,
        }
    }

    /// `RunOne(List<Pair<BehaviorControl, Integer>>)`.
    #[must_use]
    pub fn run_one(behaviors: Vec<(Box<dyn BehaviorControl<M>>, i32)>, seed: i64) -> Self {
        Self::run_one_with(Vec::new(), behaviors, seed)
    }

    /// `RunOne(Map<MemoryModuleType, MemoryStatus>, List<Pair<..>>)`.
    #[must_use]
    pub fn run_one_with(
        entry_condition: Vec<(MemoryModuleType, MemoryStatus)>,
        behaviors: Vec<(Box<dyn BehaviorControl<M>>, i32)>,
        seed: i64,
    ) -> Self {
        Self::new(
            entry_condition,
            Vec::new(),
            OrderPolicy::Shuffled,
            RunningPolicy::RunOne,
            behaviors,
            seed,
        )
    }

    fn has_required_memories(&self, brain: &BrainState) -> bool {
        self.entry_condition
            .iter()
            .all(|(t, s)| brain.check_memory(*t, *s))
    }

    /// The children's `debugString()`s in current list order.
    #[must_use]
    pub fn child_names(&self) -> Vec<&'static str> {
        self.behaviors.iter().map(|b| b.debug_string()).collect()
    }
}

impl<M: MobHost> BehaviorControl<M> for GateBehavior<M> {
    fn status(&self) -> Status {
        self.status
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        let mut v: Vec<MemoryModuleType> = self.entry_condition.iter().map(|(t, _)| *t).collect();
        for b in self.behaviors.iter() {
            v.extend(b.required_memories());
        }
        v
    }

    fn try_start(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) -> bool {
        if !self.has_required_memories(brain) {
            return false;
        }
        self.status = Status::Running;
        if self.order_policy == OrderPolicy::Shuffled {
            self.behaviors.shuffle();
        }
        match self.running_policy {
            RunningPolicy::RunOne => {
                for b in self.behaviors.iter_mut() {
                    if b.status() == Status::Stopped && b.try_start(brain, cx, time) {
                        break;
                    }
                }
            }
            RunningPolicy::TryAll => {
                for b in self.behaviors.iter_mut() {
                    if b.status() == Status::Stopped {
                        b.try_start(brain, cx, time);
                    }
                }
            }
        }
        true
    }

    fn tick_or_stop(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        for b in self.behaviors.iter_mut() {
            if b.status() == Status::Running {
                b.tick_or_stop(brain, cx, time);
            }
        }
        if self.behaviors.iter().all(|b| b.status() != Status::Running) {
            self.do_stop(brain, cx, time);
        }
    }

    fn do_stop(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        self.status = Status::Stopped;
        for b in self.behaviors.iter_mut() {
            if b.status() == Status::Running {
                b.do_stop(brain, cx, time);
            }
        }
        for t in &self.exit_erased_memories {
            brain.erase_memory(*t);
        }
    }

    fn debug_string(&self) -> &'static str {
        "GateBehavior"
    }
}

/// `TriggerGate.triggerGate(list, order, running)` as a [`Trigger`].
pub struct TriggerGate<M: MobHost> {
    pub triggers: ShufflingList<Box<dyn Trigger<M>>>,
    pub order_policy: OrderPolicy,
    pub running_policy: RunningPolicy,
}

impl<M: MobHost> TriggerGate<M> {
    /// `triggerGate(List<Pair<Trigger, Integer>>, OrderPolicy, RunningPolicy)`.
    #[must_use]
    pub fn new(
        triggers: Vec<(Box<dyn Trigger<M>>, i32)>,
        order_policy: OrderPolicy,
        running_policy: RunningPolicy,
        seed: i64,
    ) -> Self {
        let mut list = ShufflingList::new(seed);
        for (t, w) in triggers {
            list.add(t, w);
        }
        TriggerGate {
            triggers: list,
            order_policy,
            running_policy,
        }
    }

    /// `triggerOneShuffled(list)`.
    #[must_use]
    pub fn one_shuffled(triggers: Vec<(Box<dyn Trigger<M>>, i32)>, seed: i64) -> Self {
        Self::new(triggers, OrderPolicy::Shuffled, RunningPolicy::RunOne, seed)
    }
}

impl<M: MobHost> Trigger<M> for TriggerGate<M> {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) -> bool {
        if self.order_policy == OrderPolicy::Shuffled {
            self.triggers.shuffle();
        }
        for t in self.triggers.iter_mut() {
            if t.trigger(brain, cx, time) && self.running_policy == RunningPolicy::RunOne {
                break;
            }
        }
        true
    }

    fn name(&self) -> &'static str {
        "TriggerGate"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn shuffle_draws_one_float_per_entry_in_order_and_sorts_by_minus_pow() {
        let mut list = ShufflingList::new(99);
        list.add("a", 1).add("b", 3).add("c", 2);
        let mut twin = LegacyRandomSource::new(99);
        let fa = twin.next_f32();
        let fb = twin.next_f32();
        let fc = twin.next_f32();
        list.shuffle();
        let key = |f: f32, w: i32| -> f64 {
            #[allow(clippy::cast_precision_loss)]
            let e = 1.0f32 / (w as f32);
            -f64::from(f).powf(f64::from(e))
        };
        let mut expected = [("a", key(fa, 1)), ("b", key(fb, 3)), ("c", key(fc, 2))];
        expected.sort_by(|x, y| x.1.total_cmp(&y.1));
        let got: Vec<&str> = list.iter().copied().collect();
        assert_eq!(got, expected.iter().map(|e| e.0).collect::<Vec<_>>());
    }

    struct Starts(bool, &'static str, Status);

    impl BehaviorControl<TestMob> for Starts {
        fn status(&self) -> Status {
            self.2
        }
        fn required_memories(&self) -> Vec<MemoryModuleType> {
            vec![]
        }
        fn try_start(&mut self, _b: &mut BrainState, _cx: &mut MobAi<TestMob>, _t: i64) -> bool {
            if self.0 {
                self.2 = Status::Running;
            }
            self.0
        }
        fn tick_or_stop(&mut self, b: &mut BrainState, cx: &mut MobAi<TestMob>, t: i64) {
            self.do_stop(b, cx, t);
        }
        fn do_stop(&mut self, _b: &mut BrainState, _cx: &mut MobAi<TestMob>, _t: i64) {
            self.2 = Status::Stopped;
        }
        fn debug_string(&self) -> &'static str {
            self.1
        }
    }

    #[test]
    fn run_one_stops_at_the_first_child_that_starts_and_erases_on_exit() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut brain = BrainState::new();
        brain.register_memory(MemoryModuleType::LookTarget);
        brain.set_memory_value(
            MemoryModuleType::LookTarget,
            crate::brain::memory::MemoryValue::Unit,
        );
        let mut g = GateBehavior::new(
            vec![],
            vec![MemoryModuleType::LookTarget],
            OrderPolicy::Ordered,
            RunningPolicy::RunOne,
            vec![
                (Box::new(Starts(false, "no", Status::Stopped)), 1),
                (Box::new(Starts(true, "yes1", Status::Stopped)), 1),
                (Box::new(Starts(true, "yes2", Status::Stopped)), 1),
            ],
            0,
        );
        assert!(g.try_start(&mut brain, &mut cx, 0));
        let running: Vec<&str> = g
            .behaviors
            .iter()
            .filter(|b| b.status() == Status::Running)
            .map(|b| b.debug_string())
            .collect();
        assert_eq!(running, vec!["yes1"]);
        // The one-shot child stops on its tick; the gate then stops and erases.
        g.tick_or_stop(&mut brain, &mut cx, 1);
        assert_eq!(g.status(), Status::Stopped);
        assert!(!brain.has_memory_value(MemoryModuleType::LookTarget));
    }

    #[test]
    fn try_all_starts_every_stopped_child() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut brain = BrainState::new();
        let mut g = GateBehavior::new(
            vec![],
            vec![],
            OrderPolicy::Ordered,
            RunningPolicy::TryAll,
            vec![
                (Box::new(Starts(true, "a", Status::Stopped)), 1),
                (Box::new(Starts(true, "b", Status::Stopped)), 1),
            ],
            0,
        );
        assert!(g.try_start(&mut brain, &mut cx, 0));
        assert!(g.behaviors.iter().all(|b| b.status() == Status::Running));
    }

    #[test]
    fn a_gate_with_an_unmet_entry_condition_neither_starts_nor_shuffles() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        let mut brain = BrainState::new();
        let mut g = GateBehavior::run_one_with(
            vec![(MemoryModuleType::LookTarget, MemoryStatus::ValuePresent)],
            vec![(Box::new(Starts(true, "a", Status::Stopped)), 1)],
            5,
        );
        assert!(!g.try_start(&mut brain, &mut cx, 0));
        assert_eq!(g.status(), Status::Stopped);
        assert_eq!(
            g.behaviors.entries[0].rand_weight, 0.0,
            "no shuffle happened"
        );
    }
}
