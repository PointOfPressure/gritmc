//! Cached reads of the `RUSTMC_TRACE_*` switches.
//!
//! # Why this exists
//!
//! Every instrument in this crate is env-gated, and the gate is read *at the
//! point of use* — inside the placement chain, inside a feature's inner loop.
//! `std::env::var_os` is a `getenv`, which is a linear scan of `environ`
//! doing a `strncmp` per entry, and it allocates for `var`. A `perf record`
//! of a full pipeline on the Threadripper (2026-07-28, seed 20260722, r=6 at
//! (64,64)) put `getenv` at **2.17%** and `std::env::__var` at **1.67%** —
//! **3.8% of the whole pipeline spent asking whether tracing is on**, with
//! every switch off.
//!
//! The environment does not change while a world generates, so each switch is
//! read once and the answer kept. After the first call a lookup is one
//! relaxed atomic load.
//!
//! # This cannot change what is generated
//!
//! Every switch here controls *printing* only, and each is read exactly as
//! `std::env` would have read it at process start. The one behaviour this
//! changes is that setting a switch part-way through a run no longer takes
//! effect — which was never a supported thing to do and is not something a
//! generated world can observe.
//!
//! `trace_write` and `trace_feature` in `canvas.rs` already did this with a
//! `thread_local!`; this is the same idea where a `OnceLock` is the better
//! fit, because these are read from code that a future worker pool will run
//! on several threads and a `thread_local!` would pay the init check per
//! thread.

use std::sync::OnceLock;

/// Whether a switch is set to anything at all — `var_os(..).is_some()`.
///
/// `name` is `&'static str` so a caller cannot accidentally build the key in
/// the hot loop it is trying to keep out of.
pub fn flag(name: &'static str, cell: &'static OnceLock<bool>) -> bool {
    *cell.get_or_init(|| std::env::var_os(name).is_some())
}

/// A switch's value, or `None` when it is unset or not UTF-8.
///
/// The `'static` on the result is free rather than forced: `get_or_init`
/// borrows from the cell, and the cell is `'static`.
pub fn value(name: &'static str, cell: &'static OnceLock<Option<String>>) -> Option<&'static str> {
    cell.get_or_init(|| std::env::var(name).ok()).as_deref()
}

/// One switch, read once.
///
/// ```ignore
/// trace_switch!(patch, "RUSTMC_TRACE_PATCH");
/// if patch() { ... }
/// ```
#[macro_export]
macro_rules! trace_switch {
    ($fn_name:ident, $env:literal) => {
        fn $fn_name() -> bool {
            static CELL: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
            $crate::trace_switch::flag($env, &CELL)
        }
    };
}

/// One switch with a value, read once.
#[macro_export]
macro_rules! trace_switch_value {
    ($fn_name:ident, $env:literal) => {
        fn $fn_name() -> Option<&'static str> {
            static CELL: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
            $crate::trace_switch::value($env, &CELL)
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An unset switch answers false, and answers it from the cache the
    /// second time — which is the only thing a test can check without
    /// mutating the process environment, since `OnceLock` is one-shot.
    #[test]
    fn an_unset_switch_is_false_and_stays_false() {
        static CELL: OnceLock<bool> = OnceLock::new();
        assert!(!flag("RUSTMC_TRACE_A_SWITCH_THAT_IS_NOT_SET", &CELL));
        assert!(!flag("RUSTMC_TRACE_A_SWITCH_THAT_IS_NOT_SET", &CELL));
    }

    #[test]
    fn an_unset_value_switch_is_none() {
        static CELL: OnceLock<Option<String>> = OnceLock::new();
        assert_eq!(value("RUSTMC_TRACE_NO_SUCH_VALUE_SWITCH", &CELL), None);
    }

    /// A switch that *is* set is seen, and its value comes back whole.
    ///
    /// `PATH` rather than a variable the test sets itself: `std::env::set_var`
    /// is `unsafe` for the reason that matters here — the gate runs the test
    /// suite on several threads and another one may be reading the
    /// environment at the same instant. A variable every process already has
    /// tests the same two code paths without mutating anything.
    #[test]
    fn a_set_switch_is_seen_with_its_value() {
        static FLAG: OnceLock<bool> = OnceLock::new();
        static VALUE: OnceLock<Option<String>> = OnceLock::new();
        assert!(flag("PATH", &FLAG));
        assert_eq!(
            value("PATH", &VALUE),
            std::env::var("PATH").ok().as_deref(),
            "the cached value is the one std::env would have given"
        );
    }
}
