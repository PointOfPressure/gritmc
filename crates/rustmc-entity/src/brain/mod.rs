//! Vanilla 26.2's `Brain` system, transcribed from the jar:
//! `net.minecraft.world.entity.ai.Brain` and its memory, sensor, behaviour
//! and activity machinery.
//!
//! # Where a brain sits
//!
//! A brain mob is still a `Mob`: it has the navigation, controls and
//! selectors of [`crate::ai::MobAi`]. `Brain` is a second struct beside it,
//! ticked by the mob's `customServerAiStep` (`Villager`: `getBrain().tick(level, this)`).
//! Behaviours and sensors take both — `brain: &mut BrainState` for what
//! vanilla reaches through `mob.getBrain()`, `cx: &mut MobAi<M>` for the
//! mob — because a behaviour that lives *inside* the brain cannot also
//! borrow the brain it lives in. The split is the port's, not vanilla's;
//! [`Brain::tick`] takes the behaviour table out while it runs it.
//!
//! # `Brain.tick(level, mob)`, offsets 0..22
//!
//! ```text
//! forgetOutdatedMemories()                 every registered slot: MemorySlot.tick()
//! tickSensors(level, mob)                  sensors in LinkedHashMap (add) order
//! startEachNonRunningBehavior(level, mob)  time = level.getGameTime()
//!   for priority in availableBehaviorsByPriority (TreeMap, ascending):
//!     for (activity, set) in that priority's HashMap (java_hash_map_order):
//!       if activeActivities.contains(activity):
//!         for b in set (LinkedHashSet, add order): if b.getStatus() == STOPPED: b.tryStart(...)
//! tickEachRunningBehavior(level, mob)      time = level.getGameTime()
//!   for b in getRunningBehaviors(): b.tickOrStop(...)
//! ```
//!
//! `getRunningBehaviors()` (offsets 0..140) walks the same three levels and
//! collects every `RUNNING` behaviour into a list **before** any is ticked,
//! so a behaviour stopped by an earlier one in the same pass is still
//! ticked (and stops itself again, harmlessly). `stopAll` uses the same
//! list with `mob.level().getGameTime()`.
//!
//! # The containers, and which orders are behaviour
//!
//! ```text
//! memories                        HashMap<MemoryModuleType, MemorySlot>   identity-hashed; no observable order (memory.rs)
//! sensors                         LinkedHashMap<SensorType, Sensor>       ADD ORDER — sensors write what later sensors read
//! availableBehaviorsByPriority    TreeMap<Integer, HashMap<Activity, LinkedHashSet<BehaviorControl>>>
//!                                                                         priority asc, then String-hash order, then add order
//! activityRequirements            HashMap<Activity, Set<Pair>>            lookups only
//! activityMemoriesToEraseWhenStopped HashMap<Activity, Set<MemoryModuleType>> lookups; the erase walk is order-free
//! coreActivities                  Set<Activity>                           ImmutableSet.of(CORE) unless setCoreActivities
//! activeActivities                HashSet<Activity>                       core ∪ {one}; getActiveNonCoreActivity has one candidate
//! ```
//!
//! # Activity switching
//!
//! ```text
//! setActiveActivity(a):            if isActive(a) return; eraseMemoriesForOtherActivitesThan(a);
//!                                  activeActivities.clear(); addAll(coreActivities); add(a)
//! setActiveActivityIfPossible(a):  activityRequirementsAreMet(a) ? setActiveActivity(a) : useDefaultActivity()
//! activityRequirementsAreMet(a):   activityRequirements.containsKey(a) && every (type, status) passes checkMemory
//! updateActivityFromSchedule(env, gameTime, pos):
//!     if gameTime - lastScheduleUpdate > 20:     (SCHEDULE_UPDATE_DELAY, strict)
//!         lastScheduleUpdate = gameTime
//!         a = schedule == null ? IDLE : env.getValue(schedule, pos)
//!         if !activeActivities.contains(a): setActiveActivityIfPossible(a)
//! setActiveActivityToFirstValid(list): first a with activityRequirementsAreMet(a) -> setActiveActivity(a)
//! ```
//!
//! `lastScheduleUpdate` starts at `-9999L`.
//!
//! # `addActivity(activity, pairs, conditions, memoriesToErase)`, offsets 0..168
//!
//! `activityRequirements.put(activity, conditions)`; if `memoriesToErase`
//! is non-empty, `activityMemoriesToEraseWhenStopped.put(activity, it)`;
//! then for each `(priority, behavior)` pair in list order: `registerMemory`
//! each of `behavior.getRequiredMemories()` (`putIfAbsent` — a fresh slot,
//! never overwriting one), then add it to the `(priority, activity)` set.
//!
//! # What is deferred, by name
//!
//! * `Brain.pack()` / `Brain$Packed` / `MemoryMap` — the NBT round-trip of
//!   memories with codecs. Server-side persistence; a packed memory map
//!   handed to the constructor is a list of `(type, value, Option<ttl>)`
//!   here and nothing reads or writes NBT.
//! * The schedule is an `EnvironmentAttribute<Activity>` resolved by the
//!   level's `EnvironmentAttributeSystem` at the mob's position; see
//!   [`schedule`] for the data and the host call.

pub mod activity;
pub mod behavior;
pub mod behaviors;
pub mod gate;
pub mod memory;
pub mod schedule;
pub mod sensor;
pub mod sensors;

use std::collections::BTreeMap;

use crate::ai::mob_ai::MobAi;
use crate::host::{MobHost, PathLevel};

pub use activity::{Activity, ActivityData};
pub use behavior::{Behavior, BehaviorControl, OneShot, Status};
pub use memory::{MemoryModuleType, MemorySlot, MemoryStatus, MemoryValue};
pub use schedule::Schedule;
pub use sensor::{Sensor, SensorControl, SensorType};

use activity::java_hash_map_order;

/// `Brain.SCHEDULE_UPDATE_DELAY`.
pub const SCHEDULE_UPDATE_DELAY: i64 = 20;

/// The `Brain` fields that behaviours and sensors reach through
/// `mob.getBrain()`: the memories, the activity state and the schedule.
#[derive(Debug, Clone)]
pub struct BrainState {
    /// `memories`, one slot per registered [`MemoryModuleType`] (by ordinal).
    memories: Vec<Option<MemorySlot>>,
    /// `schedule` — `null` until `setSchedule`.
    schedule: Option<Schedule>,
    /// `activityRequirements`.
    activity_requirements: Vec<(Activity, Vec<(MemoryModuleType, MemoryStatus)>)>,
    /// `activityMemoriesToEraseWhenStopped`.
    activity_memories_to_erase_when_stopped: Vec<(Activity, Vec<MemoryModuleType>)>,
    /// `coreActivities`.
    core_activities: Vec<Activity>,
    /// `activeActivities`.
    active_activities: Vec<Activity>,
    /// `defaultActivity` — `IDLE` at construction.
    default_activity: Activity,
    /// `lastScheduleUpdate` — `-9999L` at construction.
    last_schedule_update: i64,
}

impl Default for BrainState {
    fn default() -> Self {
        Self::new()
    }
}

impl BrainState {
    /// The field initialisers of `Brain.<init>` (offsets 0..64) — before
    /// any memory, sensor or activity is added, and before
    /// `setCoreActivities(ImmutableSet.of(CORE))` / `useDefaultActivity()`.
    #[must_use]
    pub fn new() -> Self {
        BrainState {
            memories: vec![None; MemoryModuleType::ALL.len()],
            schedule: None,
            activity_requirements: Vec::new(),
            activity_memories_to_erase_when_stopped: Vec::new(),
            core_activities: Vec::new(),
            active_activities: Vec::new(),
            default_activity: Activity::Idle,
            last_schedule_update: -9999,
        }
    }

    /// `registerMemory(type)`: `memories.putIfAbsent(type, MemorySlot.create())`.
    pub fn register_memory(&mut self, ty: MemoryModuleType) {
        let slot = &mut self.memories[ty as usize];
        if slot.is_none() {
            *slot = Some(MemorySlot::create());
        }
    }

    /// Whether `type` is registered — `getMemorySlotIfPresent(type) != null`.
    #[must_use]
    pub fn is_registered(&self, ty: MemoryModuleType) -> bool {
        self.memories[ty as usize].is_some()
    }

    /// Every registered type, in ordinal order (a port order; see
    /// `memory.rs` for why no vanilla order exists to reproduce).
    #[must_use]
    pub fn registered_memories(&self) -> Vec<MemoryModuleType> {
        MemoryModuleType::ALL
            .iter()
            .copied()
            .filter(|t| self.is_registered(*t))
            .collect()
    }

    fn slot(&self, ty: MemoryModuleType) -> &MemorySlot {
        self.memories[ty as usize]
            .as_ref()
            .unwrap_or_else(|| panic!("Unregistered memory fetched: {}", ty.name()))
    }

    /// `hasMemoryValue(type)`: `checkMemory(type, VALUE_PRESENT)`.
    #[must_use]
    pub fn has_memory_value(&self, ty: MemoryModuleType) -> bool {
        self.check_memory(ty, MemoryStatus::ValuePresent)
    }

    /// `clearMemories()`: every slot cleared.
    pub fn clear_memories(&mut self) {
        for s in self.memories.iter_mut().flatten() {
            s.clear();
        }
    }

    /// `eraseMemory(type)`: clears the slot if registered; nothing otherwise.
    pub fn erase_memory(&mut self, ty: MemoryModuleType) {
        if let Some(s) = self.memories[ty as usize].as_mut() {
            s.clear();
        }
    }

    /// `setMemory(type, U)` / `setMemory(type, Optional<U>)` —
    /// `setMemoryInternal(type, value)`: nothing if unregistered; `null`
    /// **or an empty collection** clears; else `slot.set(value)`.
    pub fn set_memory(&mut self, ty: MemoryModuleType, value: Option<MemoryValue>) {
        let Some(slot) = self.memories[ty as usize].as_mut() else {
            return;
        };
        match value {
            Some(v) if !v.is_empty_collection() => slot.set(v),
            _ => slot.clear(),
        }
    }

    /// `setMemory(type, U)` with a value.
    pub fn set_memory_value(&mut self, ty: MemoryModuleType, value: MemoryValue) {
        self.set_memory(ty, Some(value));
    }

    /// `setMemoryWithExpiry(type, U, long)` — `setMemoryInternal(type, value, ttl)`:
    /// same empty-collection rule, then `slot.set(value, ttl)`.
    pub fn set_memory_with_expiry(&mut self, ty: MemoryModuleType, value: MemoryValue, ttl: i64) {
        let Some(slot) = self.memories[ty as usize].as_mut() else {
            return;
        };
        if value.is_empty_collection() {
            slot.clear();
        } else {
            slot.set_with_expiry(value, ttl);
        }
    }

    /// `getMemory(type)`: `Optional.ofNullable(getMemorySlot(type).value())`.
    ///
    /// # Panics
    /// On an unregistered type, as `getMemorySlot` throws
    /// `IllegalStateException("Unregistered memory fetched: ...")`.
    #[must_use]
    pub fn get_memory(&self, ty: MemoryModuleType) -> Option<&MemoryValue> {
        self.slot(ty).value()
    }

    /// `getMemoryInternal(type)`: `null` when unregistered (`None`), else
    /// the optional value (`Some(..)`). This is what the declarative
    /// builder's conditions read.
    #[must_use]
    pub fn memory_internal(&self, ty: MemoryModuleType) -> Option<Option<&MemoryValue>> {
        self.memories[ty as usize].as_ref().map(MemorySlot::value)
    }

    /// `getMemory(type)` for a caller that has already established the
    /// type is registered — `None` for unregistered as well as absent,
    /// where vanilla would throw. Used where the declarative builder has
    /// checked `registered(...)` first.
    #[must_use]
    pub fn get_memory_if_registered(&self, ty: MemoryModuleType) -> Option<&MemoryValue> {
        self.memories[ty as usize]
            .as_ref()
            .and_then(MemorySlot::value)
    }

    /// The slot's value for in-place mutation of a value vanilla mutates
    /// through its own object (a `Set` grown by `add`, a memo map filled by
    /// `computeIfAbsent`). The slot's expiry is untouched.
    pub fn memory_mut(&mut self, ty: MemoryModuleType) -> Option<&mut MemoryValue> {
        self.memories[ty as usize]
            .as_mut()
            .and_then(MemorySlot::value_mut)
    }

    /// `getTimeUntilExpiry(type)`.
    ///
    /// # Panics
    /// On an unregistered type (see [`Self::get_memory`]).
    #[must_use]
    pub fn time_until_expiry(&self, ty: MemoryModuleType) -> i64 {
        self.slot(ty).time_to_live()
    }

    /// `isMemoryValue(type, U)`: registered and `Objects.equals(value, slot.value())`.
    #[must_use]
    pub fn is_memory_value(&self, ty: MemoryModuleType, value: &MemoryValue) -> bool {
        match self.memories[ty as usize].as_ref() {
            None => false,
            Some(s) => s.value() == Some(value),
        }
    }

    /// `checkMemory(type, status)`, offsets 0..52: unregistered → `false`;
    /// `REGISTERED` → `true`; `VALUE_PRESENT` → `hasValue()`;
    /// `VALUE_ABSENT` → `!hasValue()`.
    #[must_use]
    pub fn check_memory(&self, ty: MemoryModuleType, status: MemoryStatus) -> bool {
        match self.memories[ty as usize].as_ref() {
            None => false,
            Some(s) => match status {
                MemoryStatus::Registered => true,
                MemoryStatus::ValuePresent => s.has_value(),
                MemoryStatus::ValueAbsent => !s.has_value(),
            },
        }
    }

    /// `forgetOutdatedMemories()`: `memories.values().forEach(MemorySlot::tick)`.
    pub fn forget_outdated_memories(&mut self) {
        for s in self.memories.iter_mut().flatten() {
            s.tick();
        }
    }

    /// `setSchedule(EnvironmentAttribute<Activity>)`.
    pub fn set_schedule(&mut self, schedule: Schedule) {
        self.schedule = Some(schedule);
    }

    /// `schedule`.
    #[must_use]
    pub fn schedule(&self) -> Option<Schedule> {
        self.schedule
    }

    /// `setCoreActivities(Set<Activity>)`.
    pub fn set_core_activities(&mut self, core: Vec<Activity>) {
        self.core_activities = core;
    }

    /// `getActiveActivities()`.
    #[must_use]
    pub fn active_activities(&self) -> &[Activity] {
        &self.active_activities
    }

    /// `useDefaultActivity()`: `setActiveActivity(defaultActivity)`.
    pub fn use_default_activity(&mut self) {
        self.set_active_activity(self.default_activity);
    }

    /// `getActiveNonCoreActivity()`: the first active activity not in
    /// `coreActivities`.
    #[must_use]
    pub fn active_non_core_activity(&self) -> Option<Activity> {
        self.active_activities
            .iter()
            .copied()
            .find(|a| !self.core_activities.contains(a))
    }

    /// `setActiveActivityIfPossible(activity)`.
    pub fn set_active_activity_if_possible(&mut self, activity: Activity) {
        if self.activity_requirements_are_met(activity) {
            self.set_active_activity(activity);
        } else {
            self.use_default_activity();
        }
    }

    /// `setActiveActivity(activity)`, offsets 0..48.
    fn set_active_activity(&mut self, activity: Activity) {
        if self.is_active(activity) {
            return;
        }
        self.erase_memories_for_other_activities_than(activity);
        self.active_activities.clear();
        for a in &self.core_activities {
            if !self.active_activities.contains(a) {
                self.active_activities.push(*a);
            }
        }
        if !self.active_activities.contains(&activity) {
            self.active_activities.push(activity);
        }
    }

    /// `eraseMemoriesForOtherActivitesThan(activity)`: for every active
    /// activity other than `activity`, erase its
    /// `activityMemoriesToEraseWhenStopped` set.
    fn erase_memories_for_other_activities_than(&mut self, activity: Activity) {
        let to_erase: Vec<MemoryModuleType> = self
            .active_activities
            .iter()
            .filter(|a| **a != activity)
            .flat_map(|a| {
                self.activity_memories_to_erase_when_stopped
                    .iter()
                    .find(|(k, _)| k == a)
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default()
            })
            .collect();
        for t in to_erase {
            self.erase_memory(t);
        }
    }

    /// `updateActivityFromSchedule(env, gameTime, pos)`: the schedule is
    /// resolved through the host (`env.getValue(schedule, pos)`).
    pub fn update_activity_from_schedule<M: MobHost>(&mut self, mob: &M, game_time: i64) {
        if game_time - self.last_schedule_update > SCHEDULE_UPDATE_DELAY {
            self.last_schedule_update = game_time;
            let activity = match self.schedule {
                Some(s) => mob.schedule_activity(s, mob.position()),
                None => Activity::Idle,
            };
            if !self.active_activities.contains(&activity) {
                self.set_active_activity_if_possible(activity);
            }
        }
    }

    /// `setActiveActivityToFirstValid(List<Activity>)`.
    pub fn set_active_activity_to_first_valid(&mut self, activities: &[Activity]) {
        for a in activities {
            if self.activity_requirements_are_met(*a) {
                self.set_active_activity(*a);
                return;
            }
        }
    }

    /// `setDefaultActivity(activity)`.
    pub fn set_default_activity(&mut self, activity: Activity) {
        self.default_activity = activity;
    }

    /// The requirement half of `addActivity` (offsets 0..35).
    fn add_activity_requirements(
        &mut self,
        activity: Activity,
        conditions: Vec<(MemoryModuleType, MemoryStatus)>,
        memories_to_erase: Vec<MemoryModuleType>,
    ) {
        match self
            .activity_requirements
            .iter_mut()
            .find(|(k, _)| *k == activity)
        {
            Some(e) => e.1 = conditions,
            None => self.activity_requirements.push((activity, conditions)),
        }
        if !memories_to_erase.is_empty() {
            match self
                .activity_memories_to_erase_when_stopped
                .iter_mut()
                .find(|(k, _)| *k == activity)
            {
                Some(e) => e.1 = memories_to_erase,
                None => self
                    .activity_memories_to_erase_when_stopped
                    .push((activity, memories_to_erase)),
            }
        }
    }

    /// `isActive(activity)`.
    #[must_use]
    pub fn is_active(&self, activity: Activity) -> bool {
        self.active_activities.contains(&activity)
    }

    /// `activityRequirementsAreMet(activity)`, offsets 0..88.
    #[must_use]
    pub fn activity_requirements_are_met(&self, activity: Activity) -> bool {
        match self
            .activity_requirements
            .iter()
            .find(|(k, _)| *k == activity)
        {
            None => false,
            Some((_, reqs)) => reqs.iter().all(|(t, s)| self.check_memory(*t, *s)),
        }
    }

    /// `lastScheduleUpdate`, for tests.
    #[must_use]
    pub fn last_schedule_update(&self) -> i64 {
        self.last_schedule_update
    }

    /// `defaultActivity`, for tests.
    #[must_use]
    pub fn default_activity(&self) -> Activity {
        self.default_activity
    }
}

/// One activity's `LinkedHashSet<BehaviorControl>`, in add order.
type BehaviorSet<M> = Vec<Box<dyn BehaviorControl<M>>>;

/// One priority's `HashMap<Activity, LinkedHashSet<BehaviorControl>>`.
struct PriorityBucket<M: MobHost> {
    /// Keys in insertion order, each with its set in add order.
    entries: Vec<(Activity, BehaviorSet<M>)>,
    /// `entries` indices in `HashMap` iteration order.
    order: Vec<usize>,
}

impl<M: MobHost> PriorityBucket<M> {
    fn new() -> Self {
        PriorityBucket {
            entries: Vec::new(),
            order: Vec::new(),
        }
    }

    fn add(&mut self, activity: Activity, behavior: Box<dyn BehaviorControl<M>>) {
        match self.entries.iter_mut().find(|(k, _)| *k == activity) {
            Some(e) => e.1.push(behavior),
            None => {
                self.entries.push((activity, vec![behavior]));
                let hashes: Vec<i32> = self.entries.iter().map(|(k, _)| k.java_hash()).collect();
                self.order = java_hash_map_order(&hashes);
            }
        }
    }
}

/// A memory handed to the constructor from a `Brain$Packed`: the value
/// and its `ExpirableValue.timeToLive`.
pub struct PackedMemory {
    pub ty: MemoryModuleType,
    pub value: MemoryValue,
    pub time_to_live: Option<i64>,
}

/// `Brain<E>`.
pub struct Brain<M: MobHost> {
    /// The memories, activities and schedule.
    pub state: BrainState,
    /// `sensors`, in add order.
    sensors: Vec<Box<dyn SensorControl<M>>>,
    /// `availableBehaviorsByPriority`.
    behaviors: BTreeMap<i32, PriorityBucket<M>>,
}

impl<M: MobHost> Brain<M> {
    /// `Brain(memoryTypes, sensorTypes, activities, memoryMap, random)` as
    /// `Brain$Provider.makeBrain(mob, packed)` calls it, offsets 0..319:
    ///
    /// ```text
    /// for t in memoryTypes: registerMemory(t)
    /// for st in sensorTypes: s = st.create(); s.randomlyDelayStart(mob.getRandom());   <- one nextInt(scanRate) each, in order
    ///                        sensors.put(st, s); for t in s.requires(): registerMemory(t)
    /// for a in activities: addActivity(a.activityType, a.behaviorPriorityPairs, a.conditions, a.memoriesToEraseWhenStopped)
    /// for v in memoryMap: setMemoryInternal(v)
    /// setCoreActivities(ImmutableSet.of(CORE)); useDefaultActivity()
    /// ```
    ///
    /// The sensors arrive already constructed (`SensorType.create()` draws
    /// nothing); the delay draw happens here, in order.
    pub fn make(
        mob: &mut M,
        memory_types: &[MemoryModuleType],
        sensors: Vec<Box<dyn SensorControl<M>>>,
        activities: Vec<ActivityData<M>>,
        packed: Vec<PackedMemory>,
    ) -> Self {
        let mut brain = Brain {
            state: BrainState::new(),
            sensors: Vec::new(),
            behaviors: BTreeMap::new(),
        };
        for t in memory_types {
            brain.state.register_memory(*t);
        }
        for mut s in sensors {
            s.randomly_delay_start(mob.random());
            let requires = s.requires();
            match brain
                .sensors
                .iter()
                .position(|e| e.sensor_type() == s.sensor_type())
            {
                Some(i) => brain.sensors[i] = s,
                None => brain.sensors.push(s),
            }
            for t in requires {
                brain.state.register_memory(t);
            }
        }
        for a in activities {
            brain.add_activity(a);
        }
        for p in packed {
            // setMemoryInternal(MemoryMap$Value): with a ttl -> the expiring
            // form, else the plain one; both apply the empty-collection rule.
            match p.time_to_live {
                Some(ttl) => brain.state.set_memory_with_expiry(p.ty, p.value, ttl),
                None => brain.state.set_memory(p.ty, Some(p.value)),
            }
        }
        brain.state.set_core_activities(vec![Activity::Core]);
        brain.state.use_default_activity();
        brain
    }

    /// `Brain()` — the no-argument constructor (an empty brain), plus the
    /// same `setCoreActivities({CORE}); useDefaultActivity()` tail. Used by
    /// tests and by mobs whose `makeBrain` adds activities afterwards.
    #[must_use]
    pub fn new(memory_types: &[MemoryModuleType], sensors: Vec<Box<dyn SensorControl<M>>>) -> Self {
        let mut brain = Brain {
            state: BrainState::new(),
            sensors: Vec::new(),
            behaviors: BTreeMap::new(),
        };
        for t in memory_types {
            brain.state.register_memory(*t);
        }
        for s in sensors {
            for t in s.requires() {
                brain.state.register_memory(t);
            }
            brain.sensors.push(s);
        }
        brain.state.set_core_activities(vec![Activity::Core]);
        brain.state.use_default_activity();
        brain
    }

    /// `addActivity(activity, ImmutableList<Pair<Integer, BehaviorControl>>, Set, Set)`.
    pub fn add_activity(&mut self, data: ActivityData<M>) {
        self.state.add_activity_requirements(
            data.activity,
            data.conditions,
            data.memories_to_erase_when_stopped,
        );
        for (priority, behavior) in data.behaviors {
            for t in behavior.required_memories() {
                self.state.register_memory(t);
            }
            self.behaviors
                .entry(priority)
                .or_insert_with(PriorityBucket::new)
                .add(data.activity, behavior);
        }
    }

    /// `addActivity(activity, int, ImmutableList<BehaviorControl>)`.
    pub fn add_activity_with_priority(
        &mut self,
        activity: Activity,
        start_priority: i32,
        behaviors: Vec<Box<dyn BehaviorControl<M>>>,
    ) {
        self.add_activity(ActivityData::with_priority(
            activity,
            start_priority,
            behaviors,
        ));
    }

    /// `removeAllBehaviors()`: `availableBehaviorsByPriority.clear()`.
    pub fn remove_all_behaviors(&mut self) {
        self.behaviors.clear();
    }

    /// `isBrainDead()`: no memories, no sensors, no behaviours.
    #[must_use]
    pub fn is_brain_dead(&self) -> bool {
        self.state.registered_memories().is_empty()
            && self.sensors.is_empty()
            && self.behaviors.is_empty()
    }

    /// `tick(level, mob)`.
    pub fn tick(&mut self, cx: &mut MobAi<M>) {
        self.state.forget_outdated_memories();
        self.tick_sensors(cx);
        self.start_each_non_running_behavior(cx);
        self.tick_each_running_behavior(cx);
    }

    /// `tickSensors(level, mob)`.
    fn tick_sensors(&mut self, cx: &mut MobAi<M>) {
        for s in &mut self.sensors {
            s.tick(&mut self.state, cx);
        }
    }

    /// `startEachNonRunningBehavior(level, mob)`.
    fn start_each_non_running_behavior(&mut self, cx: &mut MobAi<M>) {
        let time = cx.mob.level().game_time();
        let state = &mut self.state;
        for bucket in self.behaviors.values_mut() {
            for &i in &bucket.order {
                let (activity, set) = &mut bucket.entries[i];
                if !state.is_active(*activity) {
                    continue;
                }
                for b in set.iter_mut() {
                    if b.status() == Status::Stopped {
                        b.try_start(state, cx, time);
                    }
                }
            }
        }
    }

    /// `getRunningBehaviors()` as a list of `(priority, entry, index)`
    /// positions, in walk order.
    fn running_positions(&self) -> Vec<(i32, usize, usize)> {
        let mut out = Vec::new();
        for (p, bucket) in &self.behaviors {
            for &i in &bucket.order {
                for (j, b) in bucket.entries[i].1.iter().enumerate() {
                    if b.status() == Status::Running {
                        out.push((*p, i, j));
                    }
                }
            }
        }
        out
    }

    /// `getRunningBehaviors()`'s `debugString()`s, in order — what the
    /// debug packet would carry.
    #[must_use]
    pub fn running_behavior_names(&self) -> Vec<&'static str> {
        self.running_positions()
            .into_iter()
            .map(|(p, i, j)| self.behaviors[&p].entries[i].1[j].debug_string())
            .collect()
    }

    /// `tickEachRunningBehavior(level, mob)`.
    fn tick_each_running_behavior(&mut self, cx: &mut MobAi<M>) {
        let time = cx.mob.level().game_time();
        for (p, i, j) in self.running_positions() {
            let b = &mut self
                .behaviors
                .get_mut(&p)
                .expect("priority present")
                .entries[i]
                .1[j];
            b.tick_or_stop(&mut self.state, cx, time);
        }
    }

    /// `stopAll(level, mob)`: `doStop` on every running behaviour with
    /// `mob.level().getGameTime()`.
    pub fn stop_all(&mut self, cx: &mut MobAi<M>) {
        let time = cx.mob.level().game_time();
        for (p, i, j) in self.running_positions() {
            let b = &mut self
                .behaviors
                .get_mut(&p)
                .expect("priority present")
                .entries[i]
                .1[j];
            b.do_stop(&mut self.state, cx, time);
        }
    }

    /// The sensors, in add order (for tests and the debug packet).
    #[must_use]
    pub fn sensor_types(&self) -> Vec<SensorType> {
        self.sensors.iter().map(|s| s.sensor_type()).collect()
    }

    /// Every `(priority, activity, debugString)` in walk order, for tests.
    #[must_use]
    pub fn behavior_table(&self) -> Vec<(i32, Activity, &'static str)> {
        let mut out = Vec::new();
        for (p, bucket) in &self.behaviors {
            for &i in &bucket.order {
                for b in &bucket.entries[i].1 {
                    out.push((*p, bucket.entries[i].0, b.debug_string()));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::testing::{GridLevel, TestMob};
    use std::cell::RefCell;
    use std::rc::Rc;

    /// A behaviour that logs its calls and starts when `arm` says so.
    struct Log {
        name: &'static str,
        log: Rc<RefCell<Vec<String>>>,
        starts: bool,
        continues: bool,
        status: Status,
        requires: Vec<T>,
    }

    impl BehaviorControl<TestMob> for Log {
        fn status(&self) -> Status {
            self.status
        }
        fn required_memories(&self) -> Vec<T> {
            self.requires.clone()
        }
        fn try_start(&mut self, _b: &mut BrainState, _cx: &mut MobAi<TestMob>, t: i64) -> bool {
            self.log
                .borrow_mut()
                .push(format!("{}.tryStart@{t}", self.name));
            if self.starts {
                self.status = Status::Running;
            }
            self.starts
        }
        fn tick_or_stop(&mut self, b: &mut BrainState, cx: &mut MobAi<TestMob>, t: i64) {
            self.log
                .borrow_mut()
                .push(format!("{}.tick@{t}", self.name));
            if !self.continues {
                self.do_stop(b, cx, t);
            }
        }
        fn do_stop(&mut self, _b: &mut BrainState, _cx: &mut MobAi<TestMob>, t: i64) {
            self.log
                .borrow_mut()
                .push(format!("{}.stop@{t}", self.name));
            self.status = Status::Stopped;
        }
        fn debug_string(&self) -> &'static str {
            self.name
        }
    }

    fn cx() -> MobAi<TestMob> {
        MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1))
    }

    fn logged(
        name: &'static str,
        log: &Rc<RefCell<Vec<String>>>,
        starts: bool,
        requires: Vec<T>,
    ) -> Box<dyn BehaviorControl<TestMob>> {
        Box::new(Log {
            name,
            log: Rc::clone(log),
            starts,
            continues: true,
            status: Status::Stopped,
            requires,
        })
    }

    #[test]
    fn construction_registers_core_and_idle_and_defaults() {
        let brain = Brain::<TestMob>::new(&[T::WalkTarget], vec![]);
        assert_eq!(
            brain.state.active_activities(),
            &[Activity::Core, Activity::Idle]
        );
        assert!(brain.state.is_active(Activity::Core));
        assert_eq!(brain.state.active_non_core_activity(), Some(Activity::Idle));
        assert!(brain.state.is_registered(T::WalkTarget));
        assert!(!brain.state.is_registered(T::LookTarget));
        assert_eq!(brain.state.last_schedule_update(), -9999);
        assert!(!brain.is_brain_dead());
        assert!(Brain::<TestMob>::new(&[], vec![]).is_brain_dead());
    }

    #[test]
    fn check_memory_is_false_on_an_unregistered_type_for_every_status() {
        let mut s = BrainState::new();
        for st in [
            MemoryStatus::Registered,
            MemoryStatus::ValuePresent,
            MemoryStatus::ValueAbsent,
        ] {
            assert!(!s.check_memory(T::Path, st));
        }
        s.register_memory(T::Path);
        assert!(s.check_memory(T::Path, MemoryStatus::Registered));
        assert!(s.check_memory(T::Path, MemoryStatus::ValueAbsent));
        assert!(!s.check_memory(T::Path, MemoryStatus::ValuePresent));
        assert_eq!(s.memory_internal(T::Path), Some(None));
        assert_eq!(s.memory_internal(T::LookTarget), None);
    }

    #[test]
    fn set_memory_with_an_empty_collection_clears_and_unregistered_is_ignored() {
        let mut s = BrainState::new();
        s.set_memory_value(T::NearestPlayers, MemoryValue::Entities(vec![1]));
        assert!(
            !s.is_registered(T::NearestPlayers),
            "setMemory on an unregistered type is a no-op"
        );
        s.register_memory(T::NearestPlayers);
        s.set_memory_value(T::NearestPlayers, MemoryValue::Entities(vec![1]));
        assert!(s.has_memory_value(T::NearestPlayers));
        s.set_memory_with_expiry(T::NearestPlayers, MemoryValue::Entities(vec![]), 5);
        assert!(
            !s.has_memory_value(T::NearestPlayers),
            "an empty collection clears"
        );
        assert_eq!(s.time_until_expiry(T::NearestPlayers), memory::NEVER_EXPIRE);
        s.set_memory_with_expiry(T::NearestPlayers, MemoryValue::Entities(vec![2]), 5);
        assert_eq!(s.time_until_expiry(T::NearestPlayers), 5);
        assert!(s.is_memory_value(T::NearestPlayers, &MemoryValue::Entities(vec![2])));
        assert!(!s.is_memory_value(T::LookTarget, &MemoryValue::Unit));
    }

    #[test]
    fn add_activity_registers_required_memories_and_requirements() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut brain = Brain::<TestMob>::new(&[], vec![]);
        brain.add_activity(ActivityData::with_priority_and_memory(
            Activity::Panic,
            0,
            vec![logged("a", &log, true, vec![T::HurtBy, T::NearestHostile])],
            T::HurtBy,
        ));
        assert!(brain.state.is_registered(T::HurtBy));
        assert!(brain.state.is_registered(T::NearestHostile));
        assert!(!brain.state.activity_requirements_are_met(Activity::Panic));
        assert!(
            !brain.state.activity_requirements_are_met(Activity::Idle),
            "no requirements registered -> false"
        );
        brain.state.set_memory_value(T::HurtBy, MemoryValue::Unit);
        assert!(brain.state.activity_requirements_are_met(Activity::Panic));
        brain.state.set_active_activity_if_possible(Activity::Panic);
        assert_eq!(
            brain.state.active_activities(),
            &[Activity::Core, Activity::Panic]
        );
        // Switching away erases the activity's memories-to-erase set.
        brain.state.set_active_activity_if_possible(Activity::Work);
        assert_eq!(
            brain.state.active_activities(),
            &[Activity::Core, Activity::Idle],
            "unmet -> default"
        );
        assert!(!brain.state.has_memory_value(T::HurtBy));
    }

    #[test]
    fn tick_walks_priority_then_string_hash_then_add_order_and_ticks_a_prebuilt_list() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut cx = cx();
        cx.mob.level.game_time = 50;
        let mut brain = Brain::<TestMob>::new(&[], vec![]);
        // Priority 1 for IDLE added BEFORE priority 0 for CORE: TreeMap
        // puts 0 first. At priority 0, IDLE was added before CORE, but the
        // HashMap walks core (bucket 1) before idle (bucket 5).
        brain.add_activity_with_priority(
            Activity::Idle,
            1,
            vec![logged("idle1", &log, true, vec![])],
        );
        brain.add_activity_with_priority(
            Activity::Idle,
            0,
            vec![logged("idle0", &log, true, vec![])],
        );
        brain.add_activity_with_priority(
            Activity::Core,
            0,
            vec![
                logged("core0a", &log, false, vec![]),
                logged("core0b", &log, true, vec![]),
            ],
        );
        brain.add_activity_with_priority(
            Activity::Work,
            0,
            vec![logged("work0", &log, true, vec![])],
        );
        // createPriorityPairs numbers a list from its start priority, so
        // core0b lands at priority 1 beside idle1.
        assert_eq!(
            brain.behavior_table(),
            vec![
                (0, Activity::Core, "core0a"),
                (0, Activity::Idle, "idle0"),
                (0, Activity::Work, "work0"),
                (1, Activity::Core, "core0b"),
                (1, Activity::Idle, "idle1"),
            ]
        );
        brain.tick(&mut cx);
        assert_eq!(
            log.borrow().as_slice(),
            [
                "core0a.tryStart@50",
                "idle0.tryStart@50",
                "core0b.tryStart@50",
                "idle1.tryStart@50",
                "idle0.tick@50",
                "core0b.tick@50",
                "idle1.tick@50",
            ],
            "work0 is not active; core0a refused; the running list is ticked after every start"
        );
        assert_eq!(
            brain.running_behavior_names(),
            vec!["idle0", "core0b", "idle1"]
        );
        log.borrow_mut().clear();
        cx.mob.level.game_time = 51;
        brain.tick(&mut cx);
        assert_eq!(
            log.borrow().as_slice(),
            [
                "core0a.tryStart@51",
                "idle0.tick@51",
                "core0b.tick@51",
                "idle1.tick@51"
            ],
            "running behaviours are not re-asked to start"
        );
        brain.stop_all(&mut cx);
        assert!(brain.running_behavior_names().is_empty());
    }

    #[test]
    fn schedule_update_is_gated_to_more_than_twenty_ticks_and_falls_back_to_idle() {
        let mut cx = cx();
        let mut s = BrainState::new();
        s.set_core_activities(vec![Activity::Core]);
        s.use_default_activity();
        // No schedule: the activity is IDLE, already active, nothing moves.
        s.update_activity_from_schedule(&cx.mob, 0);
        assert_eq!(s.last_schedule_update(), 0, "-9999 -> 0 is > 20 ticks");
        s.update_activity_from_schedule(&cx.mob, 20);
        assert_eq!(s.last_schedule_update(), 0, "exactly 20 later is not > 20");
        s.update_activity_from_schedule(&cx.mob, 21);
        assert_eq!(s.last_schedule_update(), 21);
        // With the villager schedule at work time and WORK unrequirable, it
        // falls back to the default (IDLE) — and stays there.
        s.set_schedule(Schedule::VillagerDefault);
        cx.mob.day_time = 3000;
        s.update_activity_from_schedule(&cx.mob, 100);
        assert_eq!(s.active_activities(), &[Activity::Core, Activity::Idle]);
    }
}
