#!/usr/bin/env python3
"""Prove `ci/check-test-targets.py` goes red when a target cannot fail.

    python3 ci/check-test-targets-falsify.py     # ~2 s, no build, no jar, no box

# WHY THIS EXISTS

`ci/check-test-targets.py` is the ledger that says which gate runs each cargo
target, and on 2026-08-16 it was found to be adjudicating exactly half of its
own subject. It re-evaluated `ci/t0.sh`'s argv against the manifest and refused
on a disagreement; it had never opened `ci/t1.sh`. Eighty-eight of the
workspace's 110 targets were marked `runs_in = "t1"`, twelve `reason` fields
argued the point at length, and no line of code checked it. Narrowing
`ci/t1.sh` would have taken 88 targets out of every gate in the project while
every check in the tree stayed green.

That is this project's central defect — a verdict whose green cannot be turned
red — inside the file whose entire subject is that defect. So the repair cannot
be another assertion. **Every rule added that day is injected here and the
expected refusal is asserted by name**, against the real checker, and every
repair is asserted to be accepted: a rule with no green form teaches people to
route round it, and a rule with no red form is decoration.

# THE FIXTURE IS A REAL CARGO WORKSPACE, WHICH IS THE POINT

Following `ci/check-figures-falsify.py` and `ci/check-use-probe-falsify.sh`: a
hermetic tree under `mktemp -d` holding a copy of the checker, its manifest, and
a `ci/t0.sh` and `ci/t1.sh` for it to read. It is a **real** four-target cargo
workspace rather than a fixture list, because the enumeration half of the
checker is `cargo metadata` and a synthetic target list would prove nothing
about whether the two halves agree on what a target is called. Run against this
repo's own manifest the cases would be hostage to which targets happen to exist
on the day.

The four targets are chosen to span the two 2026-08 defects: a lib (reached by
`--lib`), a bin in a second crate (`tools/oracle-runner` was a bin crate and
`--lib` alone matched none of it), an integration test under `tests/` (which
`--lib --bins` cannot build), and a doctest (which no target flag runs).

# THE CASE THAT MATTERS MOST

`orphan-runs-nowhere`. A target is added to the tree and wired to nothing: it
is in a bucket, so its author wrote a reason and the UNACCOUNTED rule is
satisfied, and no stage's selector reaches it. Before 2026-08-16 that was a
clean gate. It must now name the target, and — case `orphan-retired-is-green` —
retiring it on purpose must be accepted, or the only way to close the rule
would be to delete a test, which is the worst available outcome.

# THE SECOND HALF — the assertion floor, 2026-08-16

The rules above answer "does a gate run this target". They cannot answer "and
could that run have failed", and the day they were written the difference was
**twenty of this workspace's 110 targets**, nine of them inside T0's own
`t0-unit` bucket: T0 declared 22 checks and 13 could go red. So the same
discipline is applied to the floor that closes it — every rule injected, every
refusal asserted by name, and the sanctioned repair asserted to be *accepted*.

Two of those cases are the ones that decide whether the floor is worth
anything:

* `empty-target-not-admitted-by-compile-only`. A target with no test is moved
  into the compile-only category, the count goes green and the ledger looks
  closed. It must be refused on the category's criterion rather than on
  somebody's judgement, or the category is a place to put things.
* `ratchet-one-assertion-of-several`. `alpha`'s bin has two tests and loses
  one. The number of *targets* that can go red does not move, so only the
  test-level floor sees it — which is the 2026-08-07 `#[ignore]`-the-failing-
  test finding one level up.

# AND THE CALIBRATION, WHICH IS THE STATIC MODEL'S OWN RED FORM

The floor derives "can this go red" by reading source, because T0 compiles
nothing. A model of what cargo and rustdoc do is not a measurement of it, so
`--calibrate <log>` compares it against a real suite log per target, and
`CALIBRATION_CASES` proves that comparison can fail: a log that disagrees in
either direction, a log missing a target, a `Running` line the parser cannot
attribute, and — the one that was a real bug on the day — a package whose lib
and bin both build to `deps/<name>-<hash>` and must not be conflated.

Exit 0 all cases behaved, 1 a case did not, 2 the harness could not run.
"""

from __future__ import annotations

import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CHECKER = ROOT / "ci" / "check-test-targets.py"

# --- the hermetic workspace -------------------------------------------------

WORKSPACE_TOML = """[workspace]
resolver = "2"
members = ["crates/alpha", "tools/beta"]
"""

ALPHA_TOML = """[package]
name = "alpha"
version = "0.0.0"
edition = "2021"
"""

# A doc example, so the `doc` target is not merely enumerated but real. The
# `doctest` flag is a manifest property and cargo reports it either way, but a
# fixture whose doctest target holds nothing would be the vacuous-doctest
# finding this ledger records rather than a control against it.
ALPHA_LIB = """//! ```
//! assert_eq!(alpha::two(), 2);
//! ```
pub fn two() -> i32 {
    2
}

#[cfg(test)]
mod tests {
    #[test]
    fn two_is_two() {
        assert_eq!(super::two(), 2);
    }
}
"""

# alpha has a **bin as well as a lib**, and that is not decoration. Cargo
# builds both to `target/debug/deps/alpha-<hash>`, so a log reader keyed on the
# binary stem maps two different result blocks onto the same pair of names —
# which is exactly how `--calibrate` was wrong about `rustmc-server` on
# 2026-08-16, where the lib executed 588 tests and the bin executed none.
ALPHA_MAIN = """fn main() {}

#[cfg(test)]
mod tests {
    #[test]
    fn one() {
        assert_eq!(alpha::two(), 2);
    }

    #[test]
    fn two() {
        assert_eq!(alpha::two() + 1, 3);
    }
}
"""

# A target that runs and cannot fail: it compiles, links, reports
# `test result: ok. 0 passed` and is the whole subject of the assertion floor.
ALPHA_EMPTY = """//! A test target with no test in it.

fn _helper() -> i32 {
    2
}
"""

# The sanctioned other shape: an instrument, present and deliberately switched
# off, with the reason written where the switch is.
ALPHA_INSTRUMENT = """#[test]
#[ignore = "an instrument; its number means nothing without a control tree"]
fn measures_something() {
    assert_eq!(alpha::two(), 2);
}
"""

ALPHA_WIRED = """#[test]
fn wired() {
    assert_eq!(alpha::two(), 2);
}
"""

ALPHA_ORPHAN = """#[test]
fn orphan() {
    assert_eq!(alpha::two(), 2);
}
"""

BETA_TOML = """[package]
name = "beta"
version = "0.0.0"
edition = "2021"

[[bin]]
name = "beta"
path = "src/main.rs"
"""

BETA_MAIN = """fn main() {}

#[cfg(test)]
mod tests {
    #[test]
    fn runs() {}
}
"""

T0_SH = """#!/usr/bin/env bash
set -euo pipefail
# check-test-targets: the next line is T0's workspace test selector.
cargo test -q --workspace --lib --bins
"""

T1_SH = """#!/usr/bin/env bash
set -euo pipefail
# check-test-targets: the next line is T1's workspace test selector.
cargo test -q --workspace --no-fail-fast
"""

MANIFEST = """[meta]
min_targets = 3

[[bucket]]
id = "t0-unit"
runs_in = "t0"
reason = "the unit tests, which is what T0's --lib --bins reaches"
members = [
    "alpha::bin::alpha",
    "alpha::lib::alpha",
    "beta::bin::beta",
]

[[bucket]]
id = "rest"
runs_in = "t1"
reason = "under tests/ or a doctest, so --lib --bins cannot build it"
members = [
    "alpha::doc::alpha",
    "alpha::test::wired",
]

[assertions]
min_red_capable_t0 = 3
min_red_capable_t1 = 5
min_runnable_tests_t0 = 4
calibrated_on = "a fixture; the real one names a suite log"
blind_to = ["alpha::test::wired"]
"""


def build(tmp: Path) -> None:
    for rel, body in [
        ("Cargo.toml", WORKSPACE_TOML),
        ("crates/alpha/Cargo.toml", ALPHA_TOML),
        ("crates/alpha/src/lib.rs", ALPHA_LIB),
        ("crates/alpha/src/main.rs", ALPHA_MAIN),
        ("crates/alpha/tests/wired.rs", ALPHA_WIRED),
        ("tools/beta/Cargo.toml", BETA_TOML),
        ("tools/beta/src/main.rs", BETA_MAIN),
        ("ci/t0.sh", T0_SH),
        ("ci/t1.sh", T1_SH),
        ("ci/checks/test-targets.toml", MANIFEST),
    ]:
        p = tmp / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(body)
    # The real checker, copied rather than imported: this must exercise the
    # bytes `ci/t0.sh` actually runs, including its `ROOT`-from-`__file__`
    # resolution, which an import would bypass.
    shutil.copy(CHECKER, tmp / "ci" / "check-test-targets.py")


def run(tmp: Path) -> tuple[int, str]:
    r = subprocess.run(
        [sys.executable, str(tmp / "ci" / "check-test-targets.py")],
        cwd=tmp,
        capture_output=True,
        text=True,
    )
    return r.returncode, r.stdout + r.stderr


# --- the cases --------------------------------------------------------------
#
# Each is (label, mutate, expected rc, expected substring). `mutate` takes the
# fixture root and edits it in place; the fixture is rebuilt from scratch before
# every case, so a case cannot inherit another's damage — which is how a sweep
# starts reporting a fault it did not inject.


def wire_orphan_to_nothing(tmp: Path) -> None:
    """The brief's case: a target exists, has a bucket and a reason, runs nowhere.

    `ci/t1.sh` is narrowed to name only the targets that were there before, so
    the new one is selected by no stage. Every other rule in the checker is
    satisfied: it is not unaccounted, not stale, not a duplicate, and the floor
    is met.
    """
    (tmp / "crates/alpha/tests/orphan.rs").write_text(ALPHA_ORPHAN)
    (tmp / "ci/checks/test-targets.toml").write_text(
        MANIFEST
        + '\n[[bucket]]\nid = "new"\nruns_in = "t1"\nreason = "somebody wrote a reason"\n'
        'members = [\n    "alpha::test::orphan",\n]\n'
    )
    (tmp / "ci/t1.sh").write_text(
        T1_SH.replace(
            "cargo test -q --workspace --no-fail-fast",
            "cargo test -q --workspace --lib --bins --doc --test wired",
        )
    )


def retire_the_orphan(tmp: Path) -> None:
    wire_orphan_to_nothing(tmp)
    p = tmp / "ci/checks/test-targets.toml"
    p.write_text(p.read_text().replace('id = "new"\nruns_in = "t1"', 'id = "new"\nruns_in = "retired"'))


def add_target_unaccounted(tmp: Path) -> None:
    (tmp / "crates/alpha/tests/orphan.rs").write_text(ALPHA_ORPHAN)


def retire_something_t1_runs(tmp: Path) -> None:
    (tmp / "ci/checks/test-targets.toml").write_text(
        MANIFEST.replace('    "alpha::test::wired",\n', "")
        + '\n[[bucket]]\nid = "gone"\nruns_in = "retired"\nreason = "claimed dead"\n'
        'members = [\n    "alpha::test::wired",\n]\n'
    )


def narrow_t1(tmp: Path) -> None:
    (tmp / "ci/t1.sh").write_text(
        T1_SH.replace("cargo test -q --workspace --no-fail-fast", "cargo test -q --workspace --lib --bins")
    )


def narrow_t0(tmp: Path) -> None:
    (tmp / "ci/t0.sh").write_text(
        T0_SH.replace("cargo test -q --workspace --lib --bins", "cargo test -q --workspace --lib")
    )


def widen_t0(tmp: Path) -> None:
    (tmp / "ci/t0.sh").write_text(
        T0_SH.replace("cargo test -q --workspace --lib --bins", "cargo test -q --workspace --tests")
    )


def unmarked_line_in_t1(tmp: Path) -> None:
    (tmp / "ci/t1.sh").write_text(T1_SH + "cargo test -q --workspace --tests\n")


def env_prefixed_unmarked_line_in_t1(tmp: Path) -> None:
    """The hole the physical-line scan had: an env prefix hid the command.

    `ln.strip().startswith("cargo test")` did not match this, so a whole extra
    suite could run in a stage with no marker, no refusal and no manifest entry.
    """
    (tmp / "ci/t1.sh").write_text(T1_SH + 'RUSTMC_FIXTURES=optional cargo test -q --workspace\n')


def t0_sentinel_in_t1(tmp: Path) -> None:
    """The copy-paste: T1's line marked with T0's sentinel.

    A sentinel that matched either stage would file this argv under T0 and
    report T1 as having no selector at all.
    """
    (tmp / "ci/t1.sh").write_text(T1_SH.replace("T1's workspace", "T0's workspace"))


def no_t1_file(tmp: Path) -> None:
    (tmp / "ci/t1.sh").unlink()


def strip_t1_sentinel(tmp: Path) -> None:
    """Delete the marker and the line becomes unmarked, which refuses earlier.

    Kept as its own case because the two refusals are different findings and it
    would be easy to assume the weaker one covers this: an unmarked line means
    "this script cannot tell whose workspace that is", and it fires before any
    question of coverage. The stronger refusal below is the other shape.
    """
    (tmp / "ci/t1.sh").write_text(
        T1_SH.replace("# check-test-targets: the next line is T1's workspace test selector.\n", "")
    )


def t1_tests_only_a_foreign_workspace(tmp: Path) -> None:
    """A T1 whose every `cargo test` is honestly marked as somebody else's.

    Nothing is unmarked, nothing is malformed, and this workspace has no T1
    selector at all — so the manifest's 88 `runs_in = "t1"` entries have no
    evidence behind them and the script must refuse rather than compute an
    empty selection and report 88 targets running nowhere. Refusing names the
    instrument; the alternative names 88 innocent targets.
    """
    (tmp / "ci/t1.sh").write_text(
        T1_SH.replace(
            "# check-test-targets: the next line is T1's workspace test selector.",
            "# check-test-targets: the next line tests a DIFFERENT workspace, not this one.",
        )
    )



# --- the assertion floor, added 2026-08-16 ----------------------------------
#
# Every case below injects a way for a gate to declare a check it cannot fail,
# and asserts the refusal by name. The measured instance they reconstruct is
# this repo's own: 20 of 110 targets executed nothing, nine of them inside
# T0's `t0-unit` bucket, so T0 declared 22 checks and 13 could go red.


def add_empty_target(tmp: Path) -> None:
    """A target that runs and cannot fail — the shape the floor exists for.

    It is in a bucket with a reason, T1's bare selector reaches it, every
    older rule in the checker is satisfied, and it holds no test. Before the
    assertion floor this was a clean gate.
    """
    (tmp / "crates/alpha/tests/hollow.rs").write_text(ALPHA_EMPTY)
    (tmp / "ci/checks/test-targets.toml").write_text(
        MANIFEST
        + '\n[[bucket]]\nid = "new"\nruns_in = "t1"\nreason = "somebody wrote a reason"\n'
        'members = [\n    "alpha::test::hollow",\n]\n'
    )


def empty_target_declared_compile_only(tmp: Path) -> None:
    """The category bent to make the arithmetic work — refused on its criterion.

    This is the failure mode the compile-only category would have if it were
    a label rather than a rule: an empty target is moved into it, the count
    goes green, and the ledger looks closed while nothing changed. Membership
    needs a test that is present and `#[ignore]`d; a target with no test at
    all has nothing switched off and nothing being kept type-checked.
    """
    add_empty_target(tmp)
    p = tmp / "ci/checks/test-targets.toml"
    p.write_text(
        p.read_text()
        + '\n[[compile_only]]\ntarget = "alpha::test::hollow"\n'
        'reason = "it only has to type-check"\n'
    )


def add_ignored_instrument(tmp: Path) -> None:
    """An instrument: a real test, deliberately `#[ignore]`d, undeclared."""
    (tmp / "crates/alpha/tests/instrument.rs").write_text(ALPHA_INSTRUMENT)
    (tmp / "ci/checks/test-targets.toml").write_text(
        MANIFEST
        + '\n[[bucket]]\nid = "instruments"\nruns_in = "t1"\nreason = "measures, does not gate"\n'
        'members = [\n    "alpha::test::instrument",\n]\n'
    )


def declare_instrument_compile_only(tmp: Path) -> None:
    """...and declaring it is the sanctioned repair, so it must be accepted."""
    add_ignored_instrument(tmp)
    p = tmp / "ci/checks/test-targets.toml"
    p.write_text(
        p.read_text()
        + '\n[[compile_only]]\ntarget = "alpha::test::instrument"\n'
        'reason = "an instrument; the gate keeps it type-checking against the API it drives"\n'
    )


def compile_only_without_a_reason(tmp: Path) -> None:
    add_ignored_instrument(tmp)
    p = tmp / "ci/checks/test-targets.toml"
    p.write_text(p.read_text() + '\n[[compile_only]]\ntarget = "alpha::test::instrument"\nreason = "  "\n')


def compile_only_over_a_target_that_asserts(tmp: Path) -> None:
    """The lie in the other direction: a live check filed as unable to fail.

    Its failures would then be nobody's, and the count of what can go red
    would understate itself — which is how a ledger stops being read.
    """
    p = tmp / "ci/checks/test-targets.toml"
    p.write_text(
        p.read_text()
        + '\n[[compile_only]]\ntarget = "alpha::test::wired"\nreason = "claimed inert"\n'
    )


def compile_only_over_a_doctest(tmp: Path) -> None:
    p = tmp / "ci/checks/test-targets.toml"
    p.write_text(
        p.read_text()
        + '\n[[compile_only]]\ntarget = "alpha::doc::alpha"\nreason = "claimed type-check only"\n'
    )


def stale_compile_only_entry(tmp: Path) -> None:
    p = tmp / "ci/checks/test-targets.toml"
    p.write_text(
        p.read_text()
        + '\n[[compile_only]]\ntarget = "alpha::test::ghost"\nreason = "no such target"\n'
    )


def delete_the_only_assertion_in_a_t0_target(tmp: Path) -> None:
    """The ratchet, at target granularity: a T0 target stops being able to fail.

    `beta`'s bin loses its only `#[test]`. It still compiles, still links,
    still reports `test result: ok`, and T0 still declares it.
    """
    (tmp / "tools/beta/src/main.rs").write_text("fn main() {}\n")


def delete_one_assertion_of_several(tmp: Path) -> None:
    """The ratchet, at test granularity, and why the target-level one is not enough.

    `alpha`'s bin has two tests and loses one. The number of targets that can
    go red does **not** move — it is still three — so only a floor on tests
    executed can see it. This is the 2026-08-07 finding one level up: a gate a
    branch can go green through by deleting the test that catches it.
    """
    (tmp / "crates/alpha/src/main.rs").write_text(
        ALPHA_MAIN.replace(
            """
    #[test]
    fn two() {
        assert_eq!(alpha::two() + 1, 3);
    }
""",
            "",
        )
    )


def ignore_the_last_assertion_in_a_t0_target(tmp: Path) -> None:
    """`#[ignore]` rather than deletion — the cheaper way to the same green."""
    (tmp / "tools/beta/src/main.rs").write_text(
        BETA_MAIN.replace("    #[test]\n    fn runs() {}", '    #[test]\n    #[ignore = "flaky"]\n    fn runs() {}')
    )


def drop_the_assertions_section(tmp: Path) -> None:
    (tmp / "ci/checks/test-targets.toml").write_text(MANIFEST.split("[assertions]")[0])


def drop_one_floor(tmp: Path) -> None:
    (tmp / "ci/checks/test-targets.toml").write_text(
        MANIFEST.replace("min_red_capable_t0 = 3\n", "")
    )


def drop_the_test_floor(tmp: Path) -> None:
    (tmp / "ci/checks/test-targets.toml").write_text(
        MANIFEST.replace("min_runnable_tests_t0 = 4\n", "")
    )


# --- the calibration, which is what stops the static model being trusted ----

GOOD_LOG = """   Compiling alpha v0.0.0
    Finished test profile
     Running unittests src/lib.rs (target/debug/deps/alpha-1111111111111111)

running 1 test
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running unittests src/main.rs (target/debug/deps/alpha-2222222222222222)

running 2 tests
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/wired.rs (target/debug/deps/wired-3333333333333333)

running 1 test
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running unittests src/main.rs (target/debug/deps/beta-4444444444444444)

running 1 test
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests alpha

running 1 test
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
"""


def calibrate(tmp: Path, log: str) -> tuple[int, str]:
    (tmp / "suite.log").write_text(log)
    r = subprocess.run(
        [sys.executable, str(tmp / "ci" / "check-test-targets.py"), "--calibrate", str(tmp / "suite.log")],
        cwd=tmp,
        capture_output=True,
        text=True,
    )
    return r.returncode, r.stdout + r.stderr



def drop_the_calibration_note(tmp: Path) -> None:
    """A source-walk derivation with no recorded agreement against a real run.

    The counts above are a model of what cargo and rustdoc do. A model nobody
    checked is the thing this whole file is about, so the note recording that
    somebody did is not optional.
    """
    (tmp / "ci/checks/test-targets.toml").write_text(
        MANIFEST.replace('calibrated_on = "a fixture; the real one names a suite log"\n', "")
    )


def stale_blind_spot(tmp: Path) -> None:
    (tmp / "ci/checks/test-targets.toml").write_text(
        MANIFEST.replace('blind_to = ["alpha::test::wired"]', 'blind_to = ["alpha::test::ghost"]')
    )


def blind_spot_that_is_not_blind(tmp: Path) -> None:
    """The declared blind spot's own stale form.

    `blind_to` says "this instrument counts these as able to fail and they are
    not". If the named target stops holding a runnable test at all, the
    ordinary refusal covers it and the entry is a claim about a state that no
    longer exists — which is a stale caveat, and this repo has learned those
    read as authoritative.
    """
    (tmp / "crates/alpha/tests/wired.rs").write_text("fn _helper() {}\n")


CASES = [
    # label, mutate, rc, substring the output must contain
    ("clean-tree-is-green", lambda t: None, 0, "every target is run by a named stage"),
    ("clean-tree-names-no-hole", lambda t: None, 0, "0 retired"),
    ("orphan-runs-nowhere", wire_orphan_to_nothing, 1, "RUNS NOWHERE alpha::test::orphan"),
    ("orphan-retired-is-green", retire_the_orphan, 0, "retired, run by nothing on purpose: alpha::test::orphan"),
    ("unaccounted-target", add_target_unaccounted, 1, "UNACCOUNTED TARGET alpha::test::orphan"),
    ("retired-but-run", retire_something_t1_runs, 1, "RETIRED BUT RUN alpha::test::wired"),
    ("t1-narrowed", narrow_t1, 1, "MANIFEST SAYS T1 RUNS alpha::test::wired AND IT DOES NOT"),
    ("t1-narrowed-orphans-are-named", narrow_t1, 1, "RUNS NOWHERE alpha::doc::alpha"),
    ("t0-narrowed", narrow_t0, 1, "MANIFEST SAYS T0 RUNS beta::bin::beta AND IT DOES NOT"),
    ("t0-widened", widen_t0, 1, "AND THE MANIFEST DOES NOT SAY SO"),
    ("unmarked-line-in-t1", unmarked_line_in_t1, 2, "carries neither marker"),
    ("env-prefixed-unmarked-line", env_prefixed_unmarked_line_in_t1, 2, "carries neither marker"),
    ("t0-sentinel-in-t1", t0_sentinel_in_t1, 2, "carries neither marker"),
    ("t1-script-missing", no_t1_file, 2, "no T1 script at"),
    ("t1-sentinel-stripped", strip_t1_sentinel, 2, "carries neither marker"),
    ("t1-selects-only-a-foreign-workspace", t1_tests_only_a_foreign_workspace, 2, "no T1 selector sentinel"),
    # --- the assertion floor ------------------------------------------------
    ("clean-tree-counts-what-can-go-red", lambda t: None, 0, "T0 declares 3 targets, 3 can go red"),
    ("empty-target-cannot-fail", add_empty_target, 1, "EXECUTES NO ASSERTION alpha::test::hollow"),
    ("empty-target-not-admitted-by-compile-only", empty_target_declared_compile_only, 1, "COMPILE-ONLY AND EMPTY alpha::test::hollow"),
    ("ignored-instrument-undeclared-is-red", add_ignored_instrument, 1, "EXECUTES NO ASSERTION alpha::test::instrument"),
    ("ignored-instrument-declared-is-green", declare_instrument_compile_only, 0, "compile-only, executes nothing on purpose: alpha::test::instrument"),
    ("compile-only-needs-a-reason", compile_only_without_a_reason, 1, "COMPILE-ONLY WITHOUT A REASON alpha::test::instrument"),
    ("compile-only-over-a-live-check", compile_only_over_a_target_that_asserts, 1, "COMPILE-ONLY BUT ASSERTS alpha::test::wired"),
    ("compile-only-over-a-doctest", compile_only_over_a_doctest, 1, "COMPILE-ONLY DOCTEST alpha::doc::alpha"),
    ("stale-compile-only-entry", stale_compile_only_entry, 1, "STALE COMPILE-ONLY ENTRY alpha::test::ghost"),
    # --- the ratchet, both granularities ------------------------------------
    ("ratchet-target-emptied", delete_the_only_assertion_in_a_t0_target, 1, "RED-CAPABLE FLOOR T0"),
    ("ratchet-last-test-ignored", ignore_the_last_assertion_in_a_t0_target, 1, "RED-CAPABLE FLOOR T0"),
    ("ratchet-one-assertion-of-several", delete_one_assertion_of_several, 1, "ASSERTION FLOOR T0"),
    ("ratchet-one-of-several-moves-no-target-count", delete_one_assertion_of_several, 1, "3 runnable tests"),
    ("assertions-section-missing", drop_the_assertions_section, 1, "no [assertions] section"),
    ("red-capable-floor-missing", drop_one_floor, 1, "declares no `min_red_capable_t0`"),
    ("test-floor-missing", drop_the_test_floor, 1, "declares no `min_runnable_tests_t0`"),
    ("calibration-note-missing", drop_the_calibration_note, 1, "no `calibrated_on`"),
    ("stale-blind-spot-entry", stale_blind_spot, 1, "STALE BLIND-SPOT ENTRY alpha::test::ghost"),
    ("blind-spot-that-is-not-blind", blind_spot_that_is_not_blind, 1, "BLIND-SPOT ENTRY THAT IS NOT BLIND alpha::test::wired"),
]

# Calibration cases run a different entry point, so they carry their own list:
# (label, mutate-the-log, expected rc, expected substring).
CALIBRATION_CASES = [
    ("calibration-agrees-with-a-true-log", lambda s: s, 0, "the static model agrees with every one of them"),
    # The bug this harness's own fixture exists for: alpha's lib and bin both
    # build to `deps/alpha-<hash>`, so a reader keyed on the stem cannot tell
    # 1-passed from 2-passed and would report an ambiguity or, worse, pick one.
    (
        "calibration-tells-a-lib-from-a-bin-of-the-same-name",
        lambda s: s.replace(
            "running 2 tests\ntest result: ok. 2 passed",
            "running 0 tests\ntest result: ok. 0 passed",
        ),
        1,
        "alpha::bin::alpha: the log executed 0 test(s)",
    ),
    (
        "calibration-catches-a-model-that-overstates",
        lambda s: s.replace(
            "running 1 test\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s",
            "running 0 tests\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s",
        ),
        1,
        "alpha::doc::alpha: the log executed 0 test(s)",
    ),
    (
        "calibration-refuses-a-log-missing-a-target",
        lambda s: s.split("     Running tests/wired.rs")[0],
        1,
        "target(s) declared and absent from the log",
    ),
    (
        "calibration-refuses-a-running-line-it-cannot-attribute",
        lambda s: s.replace("Running unittests src/lib.rs", "Running unittests src/somewhere_else.rs"),
        1,
        "cannot attribute",
    ),
]


def main() -> int:
    if not CHECKER.exists():
        print(f"check-test-targets-falsify: no checker at {CHECKER}", file=sys.stderr)
        return 2
    if shutil.which("cargo") is None:
        # A refusal, not a skip. This harness enumerates with `cargo metadata`;
        # without cargo every case would report the same refusal for the same
        # reason and the sweep would pass by measuring nothing.
        print("check-test-targets-falsify: cargo is not on PATH — the fixture cannot be enumerated", file=sys.stderr)
        return 2

    fail = 0
    for label, mutate, want_rc, want_sub in CASES:
        with tempfile.TemporaryDirectory() as d:
            tmp = Path(d)
            build(tmp)
            mutate(tmp)
            rc, out = run(tmp)
        if rc == want_rc and want_sub in out:
            print(f"ok   {label}  (exit {rc})")
            continue
        fail += 1
        print(f"FAIL {label}: expected exit {want_rc} and {want_sub!r}, got exit {rc}")
        for ln in out.strip().splitlines()[:8]:
            print(f"       | {ln}")

    for label, mutate_log, want_rc, want_sub in CALIBRATION_CASES:
        with tempfile.TemporaryDirectory() as d:
            tmp = Path(d)
            build(tmp)
            rc, out = calibrate(tmp, mutate_log(GOOD_LOG))
        if rc == want_rc and want_sub in out:
            print(f"ok   {label}  (exit {rc})")
            continue
        fail += 1
        print(f"FAIL {label}: expected exit {want_rc} and {want_sub!r}, got exit {rc}")
        for ln in out.strip().splitlines()[:8]:
            print(f"       | {ln}")

    n = len(CASES) + len(CALIBRATION_CASES)
    if fail:
        print(f"check-test-targets-falsify: {fail} of {n} cases did not behave")
        return 1
    print(f"check-test-targets-falsify: {n} cases, every injected fault refused and every repair accepted")
    return 0


if __name__ == "__main__":
    sys.exit(main())
