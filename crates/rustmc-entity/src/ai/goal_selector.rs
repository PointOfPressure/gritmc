//! `net.minecraft.world.entity.ai.goal.GoalSelector`, from the 26.2 jar.
//!
//! # The three containers, and what each one's ordering means
//!
//! ```text
//! lockedFlags     EnumMap<Flag, WrappedGoal>          which goal holds each flag
//! availableGoals  ObjectLinkedOpenHashSet<WrappedGoal> every goal, in ADD order
//! disabledFlags   EnumSet<Flag>                        flags no goal may hold
//! ```
//!
//! `availableGoals` is linked, so every loop below walks goals in the order
//! `addGoal` was called — **not** priority order. Priority only decides who
//! may displace whom; who gets *asked first* is add order. That is why a
//! mob's `registerGoals` order is behaviour, and why this port keeps a `Vec`.
//! The set's dedupe (by wrapped-goal identity) has nothing to do here: every
//! `Box<dyn Goal>` is its own instance.
//!
//! # `tick()`, offset by offset (from `GoalSelector.class`)
//!
//! ```text
//! goalCleanup (12..70):
//!   for g in availableGoals:
//!     if g.isRunning() && (goalContainsAnyFlags(g, disabledFlags) || !g.canContinueToUse())
//!       g.stop()
//! 73..92:  lockedFlags.entrySet().removeIf(e -> !e.getValue().isRunning())
//! goalUpdate (107..250):
//!   for g in availableGoals:
//!     if g.isRunning()                                   continue   (136..140)
//!     if goalContainsAnyFlags(g, disabledFlags)          continue   (143..151)
//!     if !goalCanBeReplacedForAllFlags(g, lockedFlags)   continue   (154..162)
//!     if !g.canUse()                                     continue   (165..172)
//!     for flag in g.getFlags():                                     (175..243)
//!       lockedFlags.getOrDefault(flag, NO_GOAL).stop()
//!       lockedFlags.put(flag, g)
//!     g.start()                                                     (246)
//! 259..261: tickRunningGoals(true)
//! ```
//!
//! Things a plausible rewrite gets wrong, each pinned by a test below:
//!
//! * **`canUse` is asked only after the flag check passes.** A goal whose
//!   flags are locked by a higher-priority goal is never asked `canUse` that
//!   tick, so any random draw inside its `canUse` does not happen. Swapping
//!   the two checks changes the RNG stream.
//! * **A goal can start and be stopped in the same `goalUpdate` pass.** With
//!   goals A (priority 2) then B (priority 1) added in that order and both
//!   wanting `MOVE`, one tick logs `A.start, A.stop, B.start`, because A is
//!   asked first, starts, and is then displaced when B is asked.
//! * **Equal priority does not displace**: `WrappedGoal.canBeReplacedBy` is
//!   strict, and an unlocked flag is held by `NO_GOAL`, whose priority is
//!   `Integer.MAX_VALUE` and which is interruptable — so any goal with a
//!   priority below `MAX_VALUE` may take a free flag, and a goal *at*
//!   `MAX_VALUE` may not.
//! * **`NO_GOAL.stop()` is a no-op** (see `WrappedGoal`): stopping the
//!   holder of a free flag stops nothing.
//! * **The flag loop is in ordinal order** (`EnumSet.iterator`), so when a
//!   goal claims `MOVE` and `LOOK` held by two different goals, the `MOVE`
//!   holder is stopped first.
//!
//! # `tickRunningGoals(boolean all)`
//!
//! ```text
//! for g in availableGoals:
//!   if g.isRunning() && (all || g.requiresUpdateEveryTick()) g.tick()
//! ```
//!
//! `Mob.serverAiStep` calls `tick()` on even `(tickCount + id)` ticks (or on
//! the first tick) and `tickRunningGoals(false)` on the others; that gate
//! lives in [`crate::ai::mob_ai`], not here.

use super::goal::{Flag, FlagSet, Goal};
use super::wrapped_goal::WrappedGoal;

/// Handle to a goal added to a selector — the identity `removeGoal(Goal)`
/// compares on in the jar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GoalId(u64);

struct Slot<C: ?Sized> {
    id: GoalId,
    goal: WrappedGoal<C>,
}

/// `GoalSelector`.
pub struct GoalSelector<C: ?Sized> {
    /// `lockedFlags`, indexed by `Flag::ordinal()`. `None` is the map
    /// having no entry, which `getOrDefault` answers with `NO_GOAL`.
    locked_flags: [Option<GoalId>; 4],
    /// `availableGoals`, in insertion order.
    available_goals: Vec<Slot<C>>,
    /// `disabledFlags`.
    disabled_flags: FlagSet,
    next_id: u64,
}

impl<C: ?Sized> Default for GoalSelector<C> {
    fn default() -> Self {
        Self::new()
    }
}

impl<C: ?Sized> GoalSelector<C> {
    /// `GoalSelector()`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            locked_flags: [None; 4],
            available_goals: Vec::new(),
            disabled_flags: FlagSet::NONE,
            next_id: 0,
        }
    }

    /// `addGoal(int priority, Goal goal)`: `availableGoals.add(new WrappedGoal(priority, goal))`.
    pub fn add_goal(&mut self, priority: i32, goal: Box<dyn Goal<C>>) -> GoalId {
        let id = GoalId(self.next_id);
        self.next_id += 1;
        self.available_goals.push(Slot {
            id,
            goal: WrappedGoal::new(priority, goal),
        });
        id
    }

    /// `removeAllGoals(Predicate<Goal>)`: stop every matching running goal,
    /// in add order, then drop the matches from the set. Lock entries are
    /// left to the next `tick`'s `removeIf`, as in the jar.
    pub fn remove_all_goals(&mut self, cx: &mut C, mut pred: impl FnMut(&dyn Goal<C>) -> bool) {
        for slot in &mut self.available_goals {
            if pred(slot.goal.goal()) && slot.goal.is_running() {
                slot.goal.stop(cx);
            }
        }
        self.available_goals.retain(|slot| !pred(slot.goal.goal()));
    }

    /// `removeGoal(Goal)`: `removeAllGoals(g -> g == goal)`, by handle.
    pub fn remove_goal(&mut self, cx: &mut C, id: GoalId) {
        for slot in &mut self.available_goals {
            if slot.id == id && slot.goal.is_running() {
                slot.goal.stop(cx);
            }
        }
        self.available_goals.retain(|slot| slot.id != id);
    }

    /// `goalContainsAnyFlags(goal, flags)`.
    fn goal_contains_any_flags(goal: &WrappedGoal<C>, flags: FlagSet) -> bool {
        goal.flags().iter().any(|f| flags.contains(f))
    }

    fn index_of(&self, id: GoalId) -> Option<usize> {
        self.available_goals.iter().position(|s| s.id == id)
    }

    /// `goalCanBeReplacedForAllFlags(goal, lockedFlags)`: for every flag the
    /// goal wants, the holder (or `NO_GOAL`) must answer `canBeReplacedBy`.
    fn goal_can_be_replaced_for_all_flags(&self, candidate: usize) -> bool {
        let priority = self.available_goals[candidate].goal.priority();
        for flag in self.available_goals[candidate].goal.flags().iter() {
            let ok = match self.locked_flags[flag.ordinal()].and_then(|id| self.index_of(id)) {
                // NO_GOAL: interruptable, priority Integer.MAX_VALUE.
                None => priority < i32::MAX,
                Some(holder) => self.available_goals[holder]
                    .goal
                    .can_be_replaced_by_priority(priority),
            };
            if !ok {
                return false;
            }
        }
        true
    }

    /// `tick()`.
    pub fn tick(&mut self, cx: &mut C) {
        // goalCleanup
        for slot in &mut self.available_goals {
            if slot.goal.is_running()
                && (Self::goal_contains_any_flags(&slot.goal, self.disabled_flags)
                    || !slot.goal.can_continue_to_use(cx))
            {
                slot.goal.stop(cx);
            }
        }
        // lockedFlags.entrySet().removeIf(e -> !e.getValue().isRunning())
        for entry in &mut self.locked_flags {
            let keep = entry
                .and_then(|id| self.available_goals.iter().find(|s| s.id == id))
                .is_some_and(|s| s.goal.is_running());
            if !keep {
                *entry = None;
            }
        }
        // goalUpdate
        for i in 0..self.available_goals.len() {
            if self.available_goals[i].goal.is_running() {
                continue;
            }
            if Self::goal_contains_any_flags(&self.available_goals[i].goal, self.disabled_flags) {
                continue;
            }
            if !self.goal_can_be_replaced_for_all_flags(i) {
                continue;
            }
            if !self.available_goals[i].goal.can_use(cx) {
                continue;
            }
            let id = self.available_goals[i].id;
            for flag in self.available_goals[i].goal.flags().iter() {
                // lockedFlags.getOrDefault(flag, NO_GOAL).stop() — NO_GOAL's
                // stop is a no-op, and a holder is never the candidate
                // itself (holders are running, the candidate is not).
                if let Some(holder) =
                    self.locked_flags[flag.ordinal()].and_then(|h| self.index_of(h))
                {
                    debug_assert_ne!(holder, i);
                    self.available_goals[holder].goal.stop(cx);
                }
                self.locked_flags[flag.ordinal()] = Some(id);
            }
            self.available_goals[i].goal.start(cx);
        }
        self.tick_running_goals(cx, true);
    }

    /// `tickRunningGoals(boolean)`.
    pub fn tick_running_goals(&mut self, cx: &mut C, all: bool) {
        for slot in &mut self.available_goals {
            if slot.goal.is_running() && (all || slot.goal.requires_update_every_tick()) {
                slot.goal.tick(cx);
            }
        }
    }

    /// `getAvailableGoals()`, in add order.
    pub fn available_goals(&self) -> impl Iterator<Item = (GoalId, &WrappedGoal<C>)> + '_ {
        self.available_goals.iter().map(|s| (s.id, &s.goal))
    }

    /// Mutable access to one wrapped goal, for callers that poke a goal's own
    /// state (`RandomStrollGoal.trigger()`).
    pub fn goal_mut(&mut self, id: GoalId) -> Option<&mut WrappedGoal<C>> {
        self.available_goals
            .iter_mut()
            .find(|s| s.id == id)
            .map(|s| &mut s.goal)
    }

    /// `disableControlFlag(Flag)`: `disabledFlags.add(flag)`.
    pub fn disable_control_flag(&mut self, flag: Flag) {
        self.disabled_flags.add(flag);
    }

    /// `enableControlFlag(Flag)`: `disabledFlags.remove(flag)`.
    pub fn enable_control_flag(&mut self, flag: Flag) {
        self.disabled_flags.remove(flag);
    }

    /// `setControlFlag(Flag, boolean)`.
    pub fn set_control_flag(&mut self, flag: Flag, enabled: bool) {
        if enabled {
            self.enable_control_flag(flag);
        } else {
            self.disable_control_flag(flag);
        }
    }

    /// The `disabledFlags` set, for tests and for the mob hooks' own asserts.
    #[must_use]
    pub fn disabled_flags(&self) -> FlagSet {
        self.disabled_flags
    }

    /// Which goal holds `flag` right now, `None` for `NO_GOAL`.
    #[must_use]
    pub fn locked_by(&self, flag: Flag) -> Option<GoalId> {
        self.locked_flags[flag.ordinal()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    type Log = Rc<RefCell<Vec<String>>>;

    /// A scripted goal: fixed answers, every call logged with its name.
    struct Scripted {
        name: &'static str,
        log: Log,
        flags: FlagSet,
        can_use: bool,
        can_continue: bool,
        interruptable: bool,
        every_tick: bool,
    }

    impl Goal<()> for Scripted {
        fn can_use(&mut self, _: &mut ()) -> bool {
            self.log.borrow_mut().push(format!("{}.canUse", self.name));
            self.can_use
        }
        fn can_continue_to_use(&mut self, _: &mut ()) -> bool {
            self.log
                .borrow_mut()
                .push(format!("{}.canContinueToUse", self.name));
            self.can_continue
        }
        fn is_interruptable(&self) -> bool {
            self.interruptable
        }
        fn start(&mut self, _: &mut ()) {
            self.log.borrow_mut().push(format!("{}.start", self.name));
        }
        fn stop(&mut self, _: &mut ()) {
            self.log.borrow_mut().push(format!("{}.stop", self.name));
        }
        fn requires_update_every_tick(&self) -> bool {
            self.every_tick
        }
        fn tick(&mut self, _: &mut ()) {
            self.log.borrow_mut().push(format!("{}.tick", self.name));
        }
        fn flags(&self) -> FlagSet {
            self.flags
        }
    }

    fn goal(name: &'static str, log: &Log, flags: FlagSet) -> Box<Scripted> {
        Box::new(Scripted {
            name,
            log: log.clone(),
            flags,
            can_use: true,
            can_continue: true,
            interruptable: true,
            every_tick: false,
        })
    }

    fn drain(log: &Log) -> Vec<String> {
        std::mem::take(&mut *log.borrow_mut())
    }

    /// Add order A(2), B(1), both MOVE: A is asked first and starts, then B
    /// is asked, displaces A, starts. Same tick, one log.
    #[test]
    fn a_goal_can_start_and_be_displaced_in_one_tick() {
        let log: Log = Rc::default();
        let mut sel = GoalSelector::new();
        let a = sel.add_goal(2, goal("A", &log, FlagSet::of(Flag::Move)));
        let b = sel.add_goal(1, goal("B", &log, FlagSet::of(Flag::Move)));
        sel.tick(&mut ());
        assert_eq!(
            drain(&log),
            vec!["A.canUse", "A.start", "B.canUse", "A.stop", "B.start", "B.tick"]
        );
        assert_eq!(sel.locked_by(Flag::Move), Some(b));
        assert_ne!(sel.locked_by(Flag::Move), Some(a));

        // Next tick: B continues; A's flags are held by a lower number, so
        // A is never asked canUse — no draw from A's random this tick.
        sel.tick(&mut ());
        assert_eq!(drain(&log), vec!["B.canContinueToUse", "B.tick"]);
    }

    /// Add order B(1), A(2): B starts first and A is never asked.
    #[test]
    fn a_locked_flag_skips_can_use_entirely() {
        let log: Log = Rc::default();
        let mut sel = GoalSelector::new();
        sel.add_goal(1, goal("B", &log, FlagSet::of(Flag::Move)));
        sel.add_goal(2, goal("A", &log, FlagSet::of(Flag::Move)));
        sel.tick(&mut ());
        assert_eq!(drain(&log), vec!["B.canUse", "B.start", "B.tick"]);
    }

    /// `if_icmpge`: equal priority does not displace.
    #[test]
    fn equal_priority_does_not_displace() {
        let log: Log = Rc::default();
        let mut sel = GoalSelector::new();
        sel.add_goal(1, goal("A", &log, FlagSet::of(Flag::Move)));
        sel.add_goal(1, goal("B", &log, FlagSet::of(Flag::Move)));
        sel.tick(&mut ());
        assert_eq!(drain(&log), vec!["A.canUse", "A.start", "A.tick"]);
    }

    /// `NO_GOAL` has priority `Integer.MAX_VALUE`, and replacement is
    /// strict, so a goal at `MAX_VALUE` can never take even a free flag.
    #[test]
    fn max_value_priority_cannot_take_a_free_flag() {
        let log: Log = Rc::default();
        let mut sel = GoalSelector::new();
        sel.add_goal(i32::MAX, goal("A", &log, FlagSet::of(Flag::Move)));
        sel.tick(&mut ());
        assert!(drain(&log).is_empty());
        // ...but a flagless goal at MAX_VALUE has nothing to claim and runs.
        sel.add_goal(i32::MAX, goal("F", &log, FlagSet::NONE));
        sel.tick(&mut ());
        assert_eq!(drain(&log), vec!["F.canUse", "F.start", "F.tick"]);
    }

    /// A non-interruptable holder blocks a strictly higher-priority goal.
    #[test]
    fn non_interruptable_holder_is_not_displaced() {
        let log: Log = Rc::default();
        let mut sel = GoalSelector::new();
        let mut a = goal("A", &log, FlagSet::of(Flag::Move));
        a.interruptable = false;
        sel.add_goal(5, a);
        sel.add_goal(0, goal("B", &log, FlagSet::of(Flag::Move)));
        sel.tick(&mut ());
        assert_eq!(drain(&log), vec!["A.canUse", "A.start", "A.tick"]);
        sel.tick(&mut ());
        assert_eq!(drain(&log), vec!["A.canContinueToUse", "A.tick"]);
    }

    /// Disabled flags: a running goal holding one is stopped at cleanup and
    /// a goal wanting one is not asked; the lock is cleared for that tick.
    #[test]
    fn disabled_flag_stops_and_blocks() {
        let log: Log = Rc::default();
        let mut sel = GoalSelector::new();
        let a = sel.add_goal(1, goal("A", &log, FlagSet::of(Flag::Move)));
        sel.tick(&mut ());
        assert_eq!(drain(&log), vec!["A.canUse", "A.start", "A.tick"]);
        assert_eq!(sel.locked_by(Flag::Move), Some(a));
        sel.set_control_flag(Flag::Move, false);
        sel.tick(&mut ());
        // Stopped without being asked canContinueToUse (the || short-circuits
        // on the disabled check first), then not asked canUse.
        assert_eq!(drain(&log), vec!["A.stop"]);
        assert_eq!(sel.locked_by(Flag::Move), None);
        sel.set_control_flag(Flag::Move, true);
        sel.tick(&mut ());
        assert_eq!(drain(&log), vec!["A.canUse", "A.start", "A.tick"]);
    }

    /// A goal that stops wanting to continue is stopped in cleanup and its
    /// flags are free for the update pass of the same tick. A is stopped,
    /// its lock removed; in the update pass A (add order first) is asked
    /// again, can use, and restarts, so B at priority 1 against A's 0 is
    /// refused without being asked `canUse`.
    #[test]
    fn a_finished_goal_frees_its_flags_the_same_tick() {
        let log: Log = Rc::default();
        let mut sel = GoalSelector::new();
        let mut a = goal("A", &log, FlagSet::of(Flag::Move));
        a.can_continue = false;
        sel.add_goal(0, a);
        sel.add_goal(1, goal("B", &log, FlagSet::of(Flag::Move)));
        sel.tick(&mut ());
        assert_eq!(drain(&log), vec!["A.canUse", "A.start", "A.tick"]);
        sel.tick(&mut ());
        assert_eq!(
            drain(&log),
            vec![
                "A.canContinueToUse",
                "A.stop",
                "A.canUse",
                "A.start",
                "A.tick"
            ]
        );
    }

    /// Ordinal order of the flag loop: claiming MOVE and LOOK held by two
    /// goals stops the MOVE holder first.
    #[test]
    fn displaced_holders_are_stopped_in_flag_ordinal_order() {
        let log: Log = Rc::default();
        let mut sel = GoalSelector::new();
        sel.add_goal(3, goal("L", &log, FlagSet::of(Flag::Look)));
        sel.add_goal(3, goal("M", &log, FlagSet::of(Flag::Move)));
        sel.add_goal(1, goal("C", &log, FlagSet::of2(Flag::Look, Flag::Move)));
        sel.tick(&mut ());
        assert_eq!(
            drain(&log),
            vec![
                "L.canUse", "L.start", "M.canUse", "M.start", "C.canUse", "M.stop", "L.stop",
                "C.start", "C.tick"
            ]
        );
    }

    /// `tickRunningGoals(false)` reaches only goals that require an update
    /// every tick; `tick()` reaches all running goals.
    #[test]
    fn tick_running_goals_false_is_gated_on_requires_update_every_tick() {
        let log: Log = Rc::default();
        let mut sel = GoalSelector::new();
        let mut e = goal("E", &log, FlagSet::of(Flag::Jump));
        e.every_tick = true;
        sel.add_goal(0, e);
        sel.add_goal(0, goal("N", &log, FlagSet::of(Flag::Move)));
        sel.tick(&mut ());
        assert_eq!(
            drain(&log),
            vec!["E.canUse", "E.start", "N.canUse", "N.start", "E.tick", "N.tick"]
        );
        sel.tick_running_goals(&mut (), false);
        assert_eq!(drain(&log), vec!["E.tick"]);
    }

    /// `removeAllGoals` stops matching running goals in add order and drops
    /// them; the next tick's `removeIf` clears their stale lock.
    #[test]
    fn remove_goal_stops_it_and_frees_the_flag_next_tick() {
        let log: Log = Rc::default();
        let mut sel = GoalSelector::new();
        let a = sel.add_goal(0, goal("A", &log, FlagSet::of(Flag::Move)));
        sel.add_goal(1, goal("B", &log, FlagSet::of(Flag::Move)));
        sel.tick(&mut ());
        drain(&log);
        sel.remove_goal(&mut (), a);
        assert_eq!(drain(&log), vec!["A.stop"]);
        assert_eq!(sel.locked_by(Flag::Move), Some(a), "stale until next tick");
        sel.tick(&mut ());
        assert_eq!(drain(&log), vec!["B.canUse", "B.start", "B.tick"]);
    }
}
