//! Counting `RandomSource` calls the way vanilla's oracle counts them.
//!
//! # Why this is not just a `+= 1`
//!
//! `DecorateGolden` wraps the feature's `RandomSource` in a `Counting`
//! delegate and increments on each **method call** — `nextInt`, `nextLong`,
//! `nextFloat`, `nextDouble`, `nextGaussian`, `fork`. Two properties of that
//! are easy to get wrong, and both change the number:
//!
//!   * It is **not** raw generator advances. `nextInt(bound)` is one draw
//!     however many times the modulo rejection loop retries inside it.
//!   * `nextGaussian` is **one** draw however many `nextDouble`s the
//!     Marsaglia rejection loop spends, because the Java gaussian holds the
//!     *inner* source and its calls never reach the wrapper.
//!
//! Our `next_gaussian` is a default-ish body that calls `self.next_f64()` on
//! the same object, so the nested calls *would* reach the counter. That is
//! what [`gaussian`] is for: it counts one and suppresses everything nested
//! beneath it until the guard drops. A cached `nextNextGaussian` return still
//! counts one, which is also what vanilla does — it counts the call, not the
//! work.
//!
//! `setSeed` and `forkPositional` are not counted, matching `Counting`.
//!
//! # Cost when off
//!
//! Every counted method reads one thread-local `Cell<bool>` and returns. The
//! counter is armed only by the decoration trace, so the generator's hot path
//! pays a predictable-branch load per draw and nothing else.

use std::cell::Cell;

thread_local! {
    /// Whether anything is being counted right now.
    static ON: Cell<bool> = const { Cell::new(false) };
    /// Calls counted since [`arm`].
    static COUNT: Cell<u64> = const { Cell::new(0) };
    /// Whether each draw is printed as it happens.
    static TRACE: Cell<bool> = const { Cell::new(false) };
    /// How deep inside a composite draw we are.
    ///
    /// Non-zero means some outer call already counted itself and everything
    /// underneath it belongs to that one call.
    static DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// Start counting from zero. Any previous count is discarded.
pub fn arm() {
    ON.with(|f| f.set(true));
    COUNT.with(|c| c.set(0));
    DEPTH.with(|d| d.set(0));
}

/// Stop counting and take the count.
#[must_use]
pub fn take() -> u64 {
    ON.with(|f| f.set(false));
    COUNT.with(Cell::take)
}

/// The count so far, without disarming.
///
/// Used to stamp a draw offset onto an event mid-run, so a divergence can be
/// located in the stream rather than only counted at the end.
#[must_use]
pub fn peek() -> u64 {
    COUNT.with(Cell::get)
}

/// Log one draw's method, bound and result when `RUSTMC_TRACE_DRAWS` is set.
///
/// The mirror of `DecorateGolden`'s `-Ddecorate.draws`. Both sides print
/// `DRAW <n> <method>(<bound>) -> <value>` so the two sequences diff directly,
/// which is the comparison `monster_room_deep` needed after its totals stopped
/// decomposing and two separate inferences drawn from aggregate counters had
/// to be retracted.
#[inline]
pub fn log(method: &str, bound: i64, value: i64) {
    if ON.with(Cell::get) && DEPTH.with(Cell::get) == 0 && TRACE.with(Cell::get) {
        println!(
            "DRAW {} {method}({bound}) -> {value}",
            COUNT.with(Cell::get)
        );
    }
}

/// Log one draw whose value is not an integer, as its raw bit pattern.
///
/// `nextInt(bound)` and `nextLong` were the only two methods either side
/// printed a *value* for, and both loggers stayed silent on `nextFloat`,
/// `nextDouble`, `nextBoolean` and `nextGaussian`. That is most of the draws
/// a speleothem column takes, so two logs could agree on every printed line
/// and still be a hundred draws apart — the `DRAW <n>` counters simply jumped
/// by different amounts and the reader was left inferring what filled the gap.
/// `sulfur_spike_cluster` is where that stopped working: the first divergence
/// showed up as "11 unlogged draws here, 27 there".
///
/// Floats and doubles print as `Float.floatToRawIntBits` /
/// `Double.doubleToRawLongBits` rather than decimal, because the two runtimes
/// format the same bits differently and a formatting difference reads exactly
/// like a value difference.
#[inline]
pub fn log_bits(method: &str, value: i64) {
    if ON.with(Cell::get) && DEPTH.with(Cell::get) == 0 && TRACE.with(Cell::get) {
        println!("DRAW {} {method}(0) -> {value}", COUNT.with(Cell::get));
    }
}

/// Log a gaussian, which is the one draw that prints from **inside** its own
/// composite guard.
///
/// [`composite`] has already counted the call and raised the depth by the time
/// the value exists, so [`log_bits`]'s `DEPTH == 0` test would drop it — and a
/// gaussian is exactly the draw whose absence made a log unreadable, because
/// the cached `nextNextGaussian` return spends no underlying draws at all.
#[inline]
pub fn log_gaussian(value: f64) {
    if ON.with(Cell::get) && DEPTH.with(Cell::get) == 1 && TRACE.with(Cell::get) {
        println!(
            "DRAW {} nextGaussian(0) -> {}",
            COUNT.with(Cell::get),
            value.to_bits().cast_signed()
        );
    }
}

/// Whether to print each draw. Read once, since it is checked per draw.
pub fn set_trace(on: bool) {
    TRACE.with(|t| t.set(on));
}

/// Whether the draw trace is on, so a caller can skip building a value for it.
#[must_use]
#[inline]
pub fn tracing() -> bool {
    ON.with(Cell::get) && TRACE.with(Cell::get)
}

/// Count one call, unless a composite draw is already counting for us.
#[inline]
pub fn bump() {
    if ON.with(Cell::get) && DEPTH.with(Cell::get) == 0 {
        COUNT.with(|c| c.set(c.get() + 1));
    }
}

/// Counts one call and suppresses everything nested inside it.
///
/// Held by `next_gaussian` for the length of its body, so the `next_f64`
/// pairs the Marsaglia loop draws do not each count as a call — vanilla's
/// wrapper never sees them.
#[derive(Debug)]
pub struct Composite {
    /// Whether this guard actually incremented the depth, so a guard taken
    /// while the counter is off is a no-op on drop too.
    armed: bool,
}

impl Drop for Composite {
    fn drop(&mut self) {
        if self.armed {
            DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
        }
    }
}

/// Begin a composite draw: count one, suppress the rest until the guard drops.
#[must_use]
pub fn composite() -> Composite {
    let armed = ON.with(Cell::get);
    if armed {
        bump();
        DEPTH.with(|d| d.set(d.get() + 1));
    }
    Composite { armed }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_counted_until_armed() {
        bump();
        bump();
        arm();
        bump();
        assert_eq!(take(), 1);
    }

    /// The property the whole module exists for: a composite draw is one
    /// call, not one per nested draw.
    #[test]
    fn a_composite_draw_counts_once_however_much_it_consumes() {
        arm();
        {
            let _g = composite();
            for _ in 0..10 {
                bump();
            }
        }
        bump();
        assert_eq!(take(), 2, "the composite and the plain call after it");
    }

    #[test]
    fn composites_nest() {
        arm();
        {
            let _outer = composite();
            {
                let _inner = composite();
                bump();
            }
            bump();
        }
        assert_eq!(take(), 1);
    }

    /// Taking the count disarms, so a later draw outside the traced region
    /// cannot leak into the next measurement.
    #[test]
    fn taking_disarms() {
        arm();
        bump();
        assert_eq!(take(), 1);
        bump();
        assert_eq!(take(), 0);
    }
}
