//! Fixture gating that a gate can actually see.
//!
//! # The defect this exists to remove
//!
//! Dozens of tests in this workspace check our output against a real vanilla
//! artefact — a region file, a datagen extract, a structure template — and
//! every one of them was written in the same shape:
//!
//! ```ignore
//! if !fixture.exists() {
//!     eprintln!("skipping: no vanilla region at {}", fixture.display());
//!     return;
//! }
//! ```
//!
//! A test that returns early **passes**. `cargo test` captures a *passing*
//! test's stdout and stderr and never prints them, so that `eprintln!` reaches
//! nobody. `ci/handoff.sh` greps its log for `skipping: no vanilla` and refuses
//! a gate that skipped its vanilla-backed tests — and that grep structurally
//! cannot see any of these, because the bytes never leave the test harness.
//! The guard catches skips printed by *shell* stages only.
//!
//! Measured on 2026-08-05: `./ci/t0.sh | grep -c "skipping: no vanilla"` was
//! **0** on a tree where `real_vanilla_chunks_survive_a_rewrite` demonstrably
//! skipped — that test, the only one this project had that claimed to check the
//! chunk writer against real vanilla output, had been skipping in every gate run
//! on the Threadripper for its entire life, because `oracle-work/t2-regiondiff/`
//! did not exist on that box. Seventeenth check found that could not fail, and
//! this one was hiding inside the mechanism that certifies all the others.
//!
//! # The fix, and why it is this one
//!
//! **The default is inverted.** A missing fixture is a *failure*, not a skip.
//! `RUSTMC_FIXTURES=optional` is an explicit, greppable opt-out for a clean
//! clone that has no vanilla extract.
//!
//! That direction is the whole point. Under the old default, a gate went blind
//! by *omission* — nobody had to do anything wrong, the fixture merely had to be
//! absent. Under this one, blindness requires somebody to type a word. Forgetting
//! is safe; only an explicit act is dangerous, and an explicit act leaves a trace.
//!
//! Three properties follow, and none of them depends on anyone reading a log:
//!
//! 1. **A gate cannot skip.** In `Required` mode the missing fixture panics, the
//!    test fails, `cargo test` exits non-zero, and `ci/t0.sh`'s `set -e` never
//!    reaches its sentinel. No grep is involved, so no grep can be too narrow.
//! 2. **An unrecognised value is a failure.** Only the exact string `optional`
//!    opts out. `RUSTMC_FIXTURES=optionl` panics rather than quietly meaning
//!    one thing or the other.
//! 3. **Optional mode cannot be mistaken for a gate.** `ci/t0.sh` and
//!    `ci/t1.sh` print a *different* sentinel when this is not `Required`, so
//!    `grep -c "^T0 GREEN"` in `ci/handoff.sh` returns 0. Two different claims
//!    must not share a string a gate greps for — the same rule `ci/t1-gate.sh`
//!    already applies to `T1 GREEN` vs `T1-GATE GREEN`.
//!
//! # This guard can fail, and that is checked
//!
//! A guard that cannot fail is the thing being fixed here, so replacing one with
//! another would be no progress at all. The tests at the bottom of this file are
//! a deliberate pair:
//!
//! * `a_missing_fixture_in_optional_mode_is_a_skip` is the **degenerate** case.
//!   It passes under the bug and after the fix, and on its own it would prove
//!   nothing.
//! * `a_missing_fixture_in_required_mode_is_a_failure` is the **discriminating**
//!   case. Under the old behaviour — skip unconditionally — it fails.
//!
//! The same pair was run against the real gate on the box: `ci/t0.sh` with the
//! fixture moved aside goes red naming the fixture, and green with it back.

use std::path::Path;

/// What the operator has said about missing fixtures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The default. A fixture that is not there is a failure.
    Required,
    /// `RUSTMC_FIXTURES=optional`. A clean clone with no vanilla extract may
    /// skip the checks that need one, and accepts that it is not gating them.
    Optional,
}

/// The environment variable that selects the mode. Named as a constant because
/// `ci/t0.sh` and `ci/t1.sh` both branch on it and a third copy of the string
/// is how the two drift apart.
pub const MODE_ENV: &str = "RUSTMC_FIXTURES";

impl Mode {
    /// Read the mode from the environment.
    ///
    /// # Panics
    ///
    /// On any value other than `required` or `optional`. A typo must not
    /// resolve to either meaning by accident: one direction silently disarms
    /// every fixture-backed check in the workspace, and the other is a
    /// confusing failure with no explanation. Saying so is cheaper than both.
    #[must_use]
    pub fn from_env() -> Self {
        match std::env::var(MODE_ENV) {
            Err(_) => Self::Required,
            Ok(v) => Self::parse(&v),
        }
    }

    /// The parse alone, so it is testable without mutating process-global
    /// environment state from a threaded test harness.
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        match raw.trim() {
            "" | "required" => Self::Required,
            "optional" => Self::Optional,
            other => panic!(
                "{MODE_ENV}={other:?} is not a mode. Use `required` (the default: a \
                 missing fixture fails the test) or `optional` (a clean clone with no \
                 vanilla extract may skip fixture-backed checks, and no gate result \
                 from such a run is valid)."
            ),
        }
    }
}

/// What to do about one fixture. Pure, so the decision can be tested without a
/// filesystem or an environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The fixture is there. Run the check.
    Present,
    /// Absent, and the operator has accepted the hole. Return early.
    SkipAllowed,
    /// Absent, and nobody accepted anything. The caller must fail.
    MissingAndRequired,
}

/// The whole decision, in one place.
#[must_use]
pub const fn verdict(present: bool, mode: Mode) -> Verdict {
    match (present, mode) {
        (true, _) => Verdict::Present,
        (false, Mode::Optional) => Verdict::SkipAllowed,
        (false, Mode::Required) => Verdict::MissingAndRequired,
    }
}

/// Is this fixture usable? `false` means "return from the test now".
///
/// `what` names the artefact in prose (`"vanilla region"`, `"datagen
/// extract"`); `produced_by` names the command that makes it. Both end up in
/// the failure message, because a gate that goes red without saying how to make
/// it green gets its guard deleted rather than its fixture generated.
///
/// The skip line keeps the exact wording the hand-written sites used —
/// `skipping: no <what> at <path>` — so the shell-stage greps that *do* work
/// keep working.
///
/// # Panics
///
/// In [`Mode::Required`] (the default) when the fixture is absent.
#[track_caller]
pub fn available(path: &Path, what: &str, produced_by: &str) -> bool {
    decide(path.exists(), Mode::from_env(), path, what, produced_by)
}

/// [`available`] with the two ambient inputs — the filesystem and the
/// environment — passed in, so the behaviour is reachable from a test that
/// mutates neither.
#[track_caller]
fn decide(present: bool, mode: Mode, path: &Path, what: &str, produced_by: &str) -> bool {
    match verdict(present, mode) {
        Verdict::Present => true,
        Verdict::SkipAllowed => {
            eprintln!("skipping: no {what} at {}", path.display());
            false
        }
        Verdict::MissingAndRequired => panic!(
            "missing fixture: no {what} at {}\n\
             \n\
             This check has nothing to compare against, so passing it would mean \
             nothing. Produce the fixture with:\n    {produced_by}\n\
             \n\
             If this is a clean clone with no vanilla extract, set \
             {MODE_ENV}=optional — and note that no gate result from such a run \
             is valid, which is why ci/t0.sh and ci/t1.sh print a different \
             sentinel under it.",
            path.display()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nowhere() -> &'static Path {
        Path::new("/nonexistent/oracle-work/there-is-no-such-fixture/r.0.0.mca")
    }

    /// The **degenerate** half of the pair. It passed before this module
    /// existed — every skip site behaved exactly this way — and it passes now.
    /// Alone it proves nothing, which is the point of writing down which half
    /// is which.
    #[test]
    fn a_missing_fixture_in_optional_mode_is_a_skip() {
        assert!(!decide(
            false,
            Mode::Optional,
            nowhere(),
            "vanilla region",
            "ci/t2-regiondiff.sh"
        ));
    }

    /// The **discriminating** half. Under the old behaviour — skip whenever the
    /// file is absent — this fails.
    #[test]
    #[should_panic(expected = "missing fixture: no vanilla region at")]
    fn a_missing_fixture_in_required_mode_is_a_failure() {
        decide(
            false,
            Mode::Required,
            nowhere(),
            "vanilla region",
            "ci/t2-regiondiff.sh",
        );
    }

    /// The failure has to be actionable or it gets deleted rather than fixed,
    /// so the command that produces the fixture is part of the contract.
    #[test]
    #[should_panic(expected = "ci/t2-regiondiff.sh")]
    fn the_failure_names_the_command_that_makes_the_fixture() {
        decide(
            false,
            Mode::Required,
            nowhere(),
            "vanilla region",
            "ci/t2-regiondiff.sh",
        );
    }

    /// A present fixture is used in either mode. The mode must not be able to
    /// turn a check *off* when the thing it needs is right there.
    #[test]
    fn a_present_fixture_is_used_in_both_modes() {
        for mode in [Mode::Required, Mode::Optional] {
            assert!(decide(
                true,
                mode,
                nowhere(),
                "vanilla region",
                "ci/t2-regiondiff.sh"
            ));
        }
    }

    #[test]
    fn unset_and_empty_and_required_all_mean_required() {
        assert_eq!(Mode::parse(""), Mode::Required);
        assert_eq!(Mode::parse("required"), Mode::Required);
        assert_eq!(Mode::parse(" required "), Mode::Required);
        assert_eq!(Mode::parse("optional"), Mode::Optional);
    }

    /// A near-miss must not resolve to `Optional`. This is the direction that
    /// silently disarms every fixture-backed check in the workspace, so it is
    /// the one worth a test of its own.
    #[test]
    #[should_panic(expected = "is not a mode")]
    fn a_typo_is_not_a_mode() {
        let _ = Mode::parse("optionl");
    }

    /// And it must not resolve to `Required` either — a value nobody
    /// understands means the operator's intent is unknown, and guessing it is
    /// how a `RUSTMC_FIXTURES=off` ends up meaning `required` for a year.
    #[test]
    #[should_panic(expected = "is not a mode")]
    fn off_is_not_a_mode_either() {
        let _ = Mode::parse("off");
    }

    /// `verdict` is the whole decision and it has three outcomes, so all three
    /// are named here rather than inferred from the two `decide` tests above.
    #[test]
    fn the_three_verdicts() {
        assert_eq!(verdict(true, Mode::Required), Verdict::Present);
        assert_eq!(verdict(true, Mode::Optional), Verdict::Present);
        assert_eq!(verdict(false, Mode::Optional), Verdict::SkipAllowed);
        assert_eq!(verdict(false, Mode::Required), Verdict::MissingAndRequired);
    }
}
