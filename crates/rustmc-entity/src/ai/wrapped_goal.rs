//! `net.minecraft.world.entity.ai.goal.WrappedGoal`, from the 26.2 jar.
//!
//! A `WrappedGoal` is a `Goal` holding another goal, a priority and an
//! `isRunning` flag. Every method forwards to the wrapped goal except the
//! three that carry state:
//!
//! ```text
//! canBeReplacedBy(other)   isInterruptable() && other.getPriority() < this.getPriority()
//! start()                  if (isRunning) return; isRunning = true; goal.start()
//! stop()                   if (!isRunning) return; isRunning = false; goal.stop()
//! ```
//!
//! Two things the bytecode fixes that a reimplementation might not:
//!
//! * **`canBeReplacedBy` is strict.** `if_icmpge 22` at offset 15 jumps to
//!   `iconst_0` when `other.priority >= this.priority`, so an equal priority
//!   does not replace.
//! * **`start` and `stop` are idempotent on the `isRunning` field**, not on
//!   the wrapped goal's opinion. `GoalSelector.NO_GOAL` (`GoalSelector$2`)
//!   overrides `isRunning()` to return false but `stop()` reads the *field*
//!   (`getfield #32`), which is never set on it, so `NO_GOAL.stop()` is a
//!   no-op: the selector calls it for every unlocked flag a starting goal
//!   claims, and nothing happens.
//!
//! `equals` compares the wrapped goal by identity and `hashCode` is the
//! wrapped goal's. In the jar that lets `availableGoals` (an
//! `ObjectLinkedOpenHashSet`) reject the same goal instance added twice; here
//! every `Box<dyn Goal>` is a distinct instance, so the set is a `Vec` and
//! the dedupe has nothing to dedupe. [`crate::ai::GoalSelector`] says so
//! again where it matters.

use super::goal::{FlagSet, Goal};

/// `WrappedGoal`.
pub struct WrappedGoal<C: ?Sized> {
    goal: Box<dyn Goal<C>>,
    priority: i32,
    is_running: bool,
}

impl<C: ?Sized> WrappedGoal<C> {
    /// `WrappedGoal(int priority, Goal goal)`.
    #[must_use]
    pub fn new(priority: i32, goal: Box<dyn Goal<C>>) -> Self {
        Self {
            goal,
            priority,
            is_running: false,
        }
    }

    /// `canBeReplacedBy(WrappedGoal)`:
    /// `this.isInterruptable() && other.getPriority() < this.getPriority()`.
    #[must_use]
    pub fn can_be_replaced_by(&self, other: &WrappedGoal<C>) -> bool {
        self.is_interruptable() && other.priority() < self.priority()
    }

    /// `canBeReplacedBy` against a candidate's priority alone — what the
    /// selector needs when the lock table holds an index rather than a
    /// reference. Same comparison, same strictness.
    #[must_use]
    pub fn can_be_replaced_by_priority(&self, other_priority: i32) -> bool {
        self.is_interruptable() && other_priority < self.priority()
    }

    /// `canUse()` — forwards.
    pub fn can_use(&mut self, cx: &mut C) -> bool {
        self.goal.can_use(cx)
    }

    /// `canContinueToUse()` — forwards.
    pub fn can_continue_to_use(&mut self, cx: &mut C) -> bool {
        self.goal.can_continue_to_use(cx)
    }

    /// `isInterruptable()` — forwards.
    #[must_use]
    pub fn is_interruptable(&self) -> bool {
        self.goal.is_interruptable()
    }

    /// `start()`: `if (isRunning) return; isRunning = true; goal.start()`.
    pub fn start(&mut self, cx: &mut C) {
        if self.is_running {
            return;
        }
        self.is_running = true;
        self.goal.start(cx);
    }

    /// `stop()`: `if (!isRunning) return; isRunning = false; goal.stop()`.
    pub fn stop(&mut self, cx: &mut C) {
        if !self.is_running {
            return;
        }
        self.is_running = false;
        self.goal.stop(cx);
    }

    /// `requiresUpdateEveryTick()` — forwards.
    #[must_use]
    pub fn requires_update_every_tick(&self) -> bool {
        self.goal.requires_update_every_tick()
    }

    /// `adjustedTickDelay(int)` — forwards.
    #[must_use]
    pub fn adjusted_tick_delay(&self, n: i32) -> i32 {
        self.goal.adjusted_tick_delay(n)
    }

    /// `tick()` — forwards.
    pub fn tick(&mut self, cx: &mut C) {
        self.goal.tick(cx);
    }

    /// `getFlags()` — forwards.
    #[must_use]
    pub fn flags(&self) -> FlagSet {
        self.goal.flags()
    }

    /// `isRunning()` — the field.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.is_running
    }

    /// `getPriority()`.
    #[must_use]
    pub fn priority(&self) -> i32 {
        self.priority
    }

    /// `getGoal()`.
    #[must_use]
    pub fn goal(&self) -> &dyn Goal<C> {
        &*self.goal
    }

    /// `getGoal()`, mutably — for a caller that has to poke the wrapped
    /// goal's own state (`RandomStrollGoal.trigger()` and the like).
    pub fn goal_mut(&mut self) -> &mut dyn Goal<C> {
        &mut *self.goal
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::goal::Flag;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// A goal that records what it was asked into a shared log, so the
    /// wrapper's gating can be read off it through the trait object.
    struct Spy(Rc<RefCell<Vec<&'static str>>>);

    impl Goal<()> for Spy {
        fn can_use(&mut self, _: &mut ()) -> bool {
            self.0.borrow_mut().push("canUse");
            true
        }
        fn start(&mut self, _: &mut ()) {
            self.0.borrow_mut().push("start");
        }
        fn stop(&mut self, _: &mut ()) {
            self.0.borrow_mut().push("stop");
        }
        fn flags(&self) -> FlagSet {
            FlagSet::of(Flag::Move)
        }
    }

    fn spy(priority: i32) -> (WrappedGoal<()>, Rc<RefCell<Vec<&'static str>>>) {
        let log = Rc::new(RefCell::new(Vec::new()));
        (WrappedGoal::new(priority, Box::new(Spy(log.clone()))), log)
    }

    /// `if_icmpge`: equal priority does not replace; only strictly lower.
    #[test]
    fn can_be_replaced_by_is_strictly_lower_priority() {
        let (three, _) = spy(3);
        let (two, _) = spy(2);
        let (also_three, _) = spy(3);
        assert!(three.can_be_replaced_by(&two));
        assert!(!three.can_be_replaced_by(&also_three));
        assert!(!two.can_be_replaced_by(&three));
        assert!(three.can_be_replaced_by_priority(2));
        assert!(!three.can_be_replaced_by_priority(3));
    }

    /// `start` twice reaches the goal once; `stop` before `start` reaches it
    /// not at all; the field is what gates, not the goal.
    #[test]
    fn start_and_stop_are_gated_by_the_running_field() {
        let (mut w, log) = spy(3);
        assert!(!w.is_running());
        w.stop(&mut ());
        w.start(&mut ());
        w.start(&mut ());
        assert!(w.is_running());
        w.stop(&mut ());
        w.stop(&mut ());
        assert!(!w.is_running());
        assert_eq!(*log.borrow(), vec!["start", "stop"]);
    }

    /// A non-interruptable goal cannot be replaced by anything, whatever the
    /// priority.
    #[test]
    fn non_interruptable_is_never_replaced() {
        struct Stubborn;
        impl Goal<()> for Stubborn {
            fn can_use(&mut self, _: &mut ()) -> bool {
                true
            }
            fn is_interruptable(&self) -> bool {
                false
            }
            fn flags(&self) -> FlagSet {
                FlagSet::NONE
            }
        }
        let w = WrappedGoal::new(5, Box::new(Stubborn));
        assert!(!w.can_be_replaced_by_priority(0));
        assert!(!w.can_be_replaced_by_priority(i32::MIN));
    }
}
