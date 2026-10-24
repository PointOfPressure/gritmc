//! `net.minecraft.world.entity.ai.goal.Goal` and `Goal$Flag`, from the 26.2
//! jar's bytecode.
//!
//! # What the bytecode says
//!
//! `Goal` is an abstract class with one field, `EnumSet<Flag> flags`, and
//! these methods (offsets are into the jar's `Goal.class`):
//!
//! ```text
//! canUse()                   abstract
//! canContinueToUse()         0: aload_0; 1: invokevirtual canUse; 4: ireturn
//! isInterruptable()          iconst_1
//! start() / stop() / tick()  bare return
//! requiresUpdateEveryTick()  iconst_0
//! setFlags(EnumSet)          flags.clear(); flags.addAll(arg)
//! getFlags()                 the field
//! adjustedTickDelay(int)     requiresUpdateEveryTick() ? n : reducedTickDelay(n)
//! reducedTickDelay(int)      Mth.positiveCeilDiv(n, 2)
//! ```
//!
//! and `Mth.positiveCeilDiv(a, b)` is `-Math.floorDiv(-a, b)`.
//!
//! `Flag` is a plain enum in ordinal order `MOVE, LOOK, JUMP, TARGET`. The
//! ordinal order is behaviour-visible: `GoalSelector` iterates a goal's
//! `EnumSet<Flag>`, and `EnumSet` iterates in ordinal order, so the order in
//! which displaced goals are stopped follows it. [`FlagSet`] iterates the
//! same way.
//!
//! # Why the trait is generic over a context
//!
//! Every concrete goal in the jar holds a `Mob` reference and reaches through
//! it for the random source, the look control, the navigation and the level.
//! There are no mobs in this server yet, and this crate is not where an
//! entity's fields live, so a goal here is written against a context type
//! `C` the caller supplies. [`crate::ai::AiCtx`] is the one the mob hooks
//! build; the selector and wrapper do not care what it is.

/// `Goal$Flag`, in ordinal order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum Flag {
    Move = 0,
    Look = 1,
    Jump = 2,
    Target = 3,
}

impl Flag {
    /// `Flag.values()`, in ordinal order.
    pub const ALL: [Flag; 4] = [Flag::Move, Flag::Look, Flag::Jump, Flag::Target];

    /// `ordinal()`.
    #[must_use]
    pub const fn ordinal(self) -> usize {
        self as usize
    }
}

/// `EnumSet<Goal.Flag>` — a four-bit set that iterates in ordinal order,
/// exactly as `EnumSet.iterator()` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct FlagSet(u8);

impl FlagSet {
    /// `EnumSet.noneOf(Flag.class)`.
    pub const NONE: FlagSet = FlagSet(0);

    /// `EnumSet.of(a)`.
    #[must_use]
    pub const fn of(a: Flag) -> Self {
        FlagSet(1 << a as u8)
    }

    /// `EnumSet.of(a, b)`.
    #[must_use]
    pub const fn of2(a: Flag, b: Flag) -> Self {
        FlagSet((1 << a as u8) | (1 << b as u8))
    }

    /// `EnumSet.of(a, b, c)`.
    #[must_use]
    pub const fn of3(a: Flag, b: Flag, c: Flag) -> Self {
        FlagSet((1 << a as u8) | (1 << b as u8) | (1 << c as u8))
    }

    /// `contains`.
    #[must_use]
    pub const fn contains(self, f: Flag) -> bool {
        self.0 & (1 << f as u8) != 0
    }

    /// `add`. Returns whether the set changed, as `EnumSet.add` does.
    pub fn add(&mut self, f: Flag) -> bool {
        let before = self.0;
        self.0 |= 1 << f as u8;
        before != self.0
    }

    /// `remove`. Returns whether the set changed, as `EnumSet.remove` does.
    pub fn remove(&mut self, f: Flag) -> bool {
        let before = self.0;
        self.0 &= !(1 << f as u8);
        before != self.0
    }

    /// `isEmpty`.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// `iterator()` — ordinal order, which is the order `GoalSelector.tick`
    /// stops displaced goals in.
    pub fn iter(self) -> impl Iterator<Item = Flag> {
        Flag::ALL.into_iter().filter(move |f| self.contains(*f))
    }
}

/// `Mth.positiveCeilDiv(int, int)`: `-Math.floorDiv(-a, b)`.
///
/// `Math.floorDiv` rounds toward negative infinity, so for `a >= 0, b > 0`
/// this is the ceiling of `a / b`; for negative `a` it is still what the
/// bytecode computes, and the test below pins that rather than assuming the
/// callers never pass one.
#[must_use]
pub const fn positive_ceil_div(a: i32, b: i32) -> i32 {
    -(floor_div(-a, b))
}

/// `java.lang.Math.floorDiv(int, int)`.
#[must_use]
pub const fn floor_div(x: i32, y: i32) -> i32 {
    let q = x / y;
    if (x ^ y) < 0 && q * y != x {
        q - 1
    } else {
        q
    }
}

/// `Goal.reducedTickDelay(int)`: `Mth.positiveCeilDiv(n, 2)`.
#[must_use]
pub const fn reduced_tick_delay(n: i32) -> i32 {
    positive_ceil_div(n, 2)
}

/// `Goal.adjustedTickDelay(int)` given the goal's `requiresUpdateEveryTick`.
///
/// Free-standing so a goal's `start` can call it on its own answer without
/// a `self` borrow fight; the trait method [`Goal::adjusted_tick_delay`]
/// just forwards here.
#[must_use]
pub const fn adjusted_tick_delay(requires_update_every_tick: bool, n: i32) -> i32 {
    if requires_update_every_tick {
        n
    } else {
        reduced_tick_delay(n)
    }
}

/// `net.minecraft.world.entity.ai.goal.Goal`, as a trait over a caller-
/// supplied context `C`.
///
/// Every default here is the base class's body. A goal that overrides
/// nothing behaves as a `Goal` subclass that overrides nothing.
pub trait Goal<C: ?Sized> {
    /// `canUse()` — abstract in the jar.
    fn can_use(&mut self, cx: &mut C) -> bool;

    /// `canContinueToUse()` — `return this.canUse()` unless overridden.
    fn can_continue_to_use(&mut self, cx: &mut C) -> bool {
        self.can_use(cx)
    }

    /// `isInterruptable()` — `iconst_1`.
    fn is_interruptable(&self) -> bool {
        true
    }

    /// `start()` — bare `return`.
    fn start(&mut self, cx: &mut C) {
        let _ = cx;
    }

    /// `stop()` — bare `return`.
    fn stop(&mut self, cx: &mut C) {
        let _ = cx;
    }

    /// `requiresUpdateEveryTick()` — `iconst_0`.
    fn requires_update_every_tick(&self) -> bool {
        false
    }

    /// `tick()` — bare `return`.
    fn tick(&mut self, cx: &mut C) {
        let _ = cx;
    }

    /// `getFlags()` — the `flags` field. Set in the concrete goal's
    /// constructor via `setFlags` in every goal in the jar, so the trait
    /// exposes the read and the struct owns the value.
    fn flags(&self) -> FlagSet;

    /// `adjustedTickDelay(int)`.
    fn adjusted_tick_delay(&self, n: i32) -> i32 {
        adjusted_tick_delay(self.requires_update_every_tick(), n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Math.floorDiv` pinned on both signs, because `positiveCeilDiv` is
    /// built on it and the name suggests a domain the bytecode does not
    /// enforce.
    #[test]
    fn floor_div_matches_java() {
        assert_eq!(floor_div(7, 2), 3);
        assert_eq!(floor_div(-7, 2), -4);
        assert_eq!(floor_div(7, -2), -4);
        assert_eq!(floor_div(-7, -2), 3);
        assert_eq!(floor_div(6, 2), 3);
        assert_eq!(floor_div(-6, 2), -3);
    }

    /// `reducedTickDelay`: 120 -> 60, 119 -> 60, 1 -> 1, 0 -> 0, 40 -> 20.
    /// These are the intervals the stroll and look goals pass in.
    #[test]
    fn reduced_tick_delay_is_positive_ceil_div_by_two() {
        assert_eq!(reduced_tick_delay(120), 60);
        assert_eq!(reduced_tick_delay(119), 60);
        assert_eq!(reduced_tick_delay(1), 1);
        assert_eq!(reduced_tick_delay(0), 0);
        assert_eq!(reduced_tick_delay(40), 20);
        assert_eq!(reduced_tick_delay(41), 21);
        // -Math.floorDiv(3, 2) = -1 for n = -3: what the bytecode does.
        assert_eq!(reduced_tick_delay(-3), -1);
    }

    #[test]
    fn adjusted_tick_delay_only_halves_when_not_every_tick() {
        assert_eq!(adjusted_tick_delay(true, 41), 41);
        assert_eq!(adjusted_tick_delay(false, 41), 21);
    }

    /// `EnumSet` iterates in ordinal order regardless of insertion order.
    #[test]
    fn flag_set_iterates_in_ordinal_order() {
        let mut s = FlagSet::NONE;
        assert!(s.add(Flag::Target));
        assert!(s.add(Flag::Move));
        assert!(!s.add(Flag::Move));
        let order: Vec<Flag> = s.iter().collect();
        assert_eq!(order, vec![Flag::Move, Flag::Target]);
        assert!(s.remove(Flag::Move));
        assert!(!s.remove(Flag::Move));
        assert_eq!(s.iter().collect::<Vec<_>>(), vec![Flag::Target]);
        assert_eq!(
            FlagSet::of2(Flag::Look, Flag::Move)
                .iter()
                .collect::<Vec<_>>(),
            vec![Flag::Move, Flag::Look]
        );
    }

    struct Bare;
    impl Goal<()> for Bare {
        fn can_use(&mut self, _: &mut ()) -> bool {
            true
        }
        fn flags(&self) -> FlagSet {
            FlagSet::NONE
        }
    }

    /// The base-class defaults: `canContinueToUse` forwards to `canUse`,
    /// `isInterruptable` is true, `requiresUpdateEveryTick` is false.
    #[test]
    fn base_class_defaults() {
        let mut g = Bare;
        assert!(g.can_continue_to_use(&mut ()));
        assert!(g.is_interruptable());
        assert!(!g.requires_update_every_tick());
        assert_eq!(g.adjusted_tick_delay(41), 21);
    }
}
