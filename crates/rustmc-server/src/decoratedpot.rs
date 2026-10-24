//! `DecoratedPotBlockEntity` — the one block entity whose `triggerEvent`
//! reads its second argument.
//!
//! Transcribed from `.wobble(WobbleStyle)` (offsets 0-41) and
//! `.triggerEvent(int, int)` (0-52) in the pinned 26.2 jar.
//!
//! # `b1` is a `WobbleStyle` ordinal and it is bounds-checked
//!
//! Seven types override `BlockEntity.triggerEvent` and all seven gate on
//! `b0 == 1`; this is the only one that then looks at `b1`. Its test at
//! offsets 12-21 is `b1 >= 0 && b1 < WobbleStyle.values().length`, so the
//! accepted range is derived from the enum rather than written down, and a
//! `b1` outside it falls through to `BlockEntity.triggerEvent` — `iconst_0;
//! ireturn` — rather than clamping.
//!
//! There is also a `level != null` test **before** the `b0` test (offset 4),
//! which is the one ordering detail here: a block entity with no level
//! answers false to *every* event including `b0 == 1`.

/// `DecoratedPotBlockEntity.WobbleStyle`, in `values()` order.
///
/// The length is what `triggerEvent` bounds `b1` against, so the count is
/// load-bearing and not decorative: **two**.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WobbleStyle {
    /// Ordinal 0.
    Positive,
    /// Ordinal 1.
    Negative,
}

impl WobbleStyle {
    /// How many styles there are — `WobbleStyle.values().length`.
    pub const COUNT: i32 = 2;

    /// `ordinal()`, which is the `b1` `wobble` sends.
    #[must_use]
    pub const fn ordinal(self) -> i32 {
        match self {
            Self::Positive => 0,
            Self::Negative => 1,
        }
    }

    /// `WobbleStyle.values()[i]`.
    #[must_use]
    pub const fn from_ordinal(i: i32) -> Option<Self> {
        match i {
            0 => Some(Self::Positive),
            1 => Some(Self::Negative),
            _ => None,
        }
    }
}

/// The `b0` a wobble sends — `iconst_1` at `wobble` offset 33.
pub const WOBBLE_EVENT: i32 = 1;

/// One decorated pot's wobble state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PotWobble {
    /// `wobbleStartedAtTick`, stamped from `Level.getGameTime()`.
    pub started_at_tick: i64,
    /// `lastWobbleStyle`; `None` before the first accepted event.
    pub last_style: Option<WobbleStyle>,
}

/// `DecoratedPotBlockEntity.wobble(WobbleStyle)` — offsets 0-41.
///
/// Returns the `(b0, b1)` of the `block_event` it queues, or `None` when
/// there is no level. **Nothing is written to the block entity here**: the
/// fields are set by `triggerEvent` when the event comes back round, which is
/// why a pot with no level wobbles on no client and remembers no style.
#[must_use]
pub fn wobble(has_level: bool, style: WobbleStyle) -> Option<(i32, i32)> {
    if !has_level {
        return None;
    }
    Some((WOBBLE_EVENT, style.ordinal()))
}

impl PotWobble {
    /// `triggerEvent(int b0, int b1)` — offsets 0-52.
    ///
    /// `now` is `Level.getGameTime()`. All three conditions — a level, `b0 ==
    /// 1`, and `b1` inside `WobbleStyle.values()` — must hold; any failure
    /// falls to `BlockEntity.triggerEvent`, which returns false and writes
    /// nothing.
    pub fn trigger_event(&mut self, has_level: bool, b0: i32, b1: i32, now: i64) -> bool {
        if !has_level || b0 != WOBBLE_EVENT {
            return false;
        }
        let Some(style) = WobbleStyle::from_ordinal(b1) else {
            return false;
        };
        self.started_at_tick = now;
        self.last_style = Some(style);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A wobble is `block_event(1, ordinal)` and the two styles differ, so
    /// this is a test of the ordinal rather than of a constant.
    #[test]
    fn a_wobble_carries_the_styles_ordinal() {
        assert_eq!(wobble(true, WobbleStyle::Positive), Some((1, 0)));
        assert_eq!(wobble(true, WobbleStyle::Negative), Some((1, 1)));
        assert_eq!(wobble(false, WobbleStyle::Positive), None);
    }

    /// `b1` is bounds-checked against the enum, so 2 is refused where 1 is
    /// taken. A pot that clamped would accept both.
    #[test]
    fn b1_is_bounded_by_the_enum_and_not_clamped() {
        let mut p = PotWobble::default();
        assert!(p.trigger_event(true, 1, 1, 40));
        assert_eq!(p.last_style, Some(WobbleStyle::Negative));
        assert_eq!(p.started_at_tick, 40);
        let mut p = PotWobble::default();
        assert!(!p.trigger_event(true, 1, WobbleStyle::COUNT, 40));
        assert!(!p.trigger_event(true, 1, -1, 40));
        assert_eq!(p.last_style, None, "a refused event writes nothing");
    }

    /// Every `b0` but 1 is refused, which is the gate all seven overriding
    /// types share.
    #[test]
    fn only_event_one_is_accepted() {
        let mut p = PotWobble::default();
        assert!(!p.trigger_event(true, 0, 0, 40));
        assert!(!p.trigger_event(true, 2, 0, 40));
        assert!(p.trigger_event(true, 1, 0, 40));
    }

    /// The level test comes first, so a pot with no level refuses even a
    /// well-formed event.
    #[test]
    fn no_level_refuses_a_well_formed_event() {
        let mut p = PotWobble::default();
        assert!(!p.trigger_event(false, 1, 0, 40));
        assert_eq!(p.started_at_tick, 0);
    }
}
