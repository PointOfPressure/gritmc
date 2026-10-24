#!/usr/bin/env bash
# T1 — G1: unit + golden-vector tests, all crates (CI.md). < 3 min target.
# Seeds: any randomized test MUST log its seed (CI.md seeds rule); prefer
# fixed seeds + golden files.
set -euo pipefail
cd "$(dirname "$0")/.."

# The decorate gate against a world the committed vectors cannot reach.
#
# **THIS RUNS BEFORE THE WORKSPACE SUITE AND THE ORDER IS LOAD-BEARING.**
# `cargo test --workspace` below exits non-zero on every run of this repo —
# `carved_chunks_match_vanilla` and `surface_chunks_match_vanilla` are declared
# reds — and this script is `set -e`. Anything placed *after* that line is
# unreachable. It was written after it first, which made it a gate that could
# never run, inside the commit whose subject was a gate that could never fail.
# `ci/t1-gate.sh` adjudicates this script's *log*, so a check that never
# executes leaves no trace to adjudicate and reads exactly like a check that
# passed.
#
# `golden/decorate_vectors.json` covers 49 features per centre and **declines
# every iceberg it meets**: `iceberg_packed` and `iceberg_blue` are cut by a
# rarity filter on their first draw at every centre in that file, so
# `IcebergFeature` had never been executed by any gate in this repo. That is
# the "a gate a window cannot exercise proves nothing" rule with a name
# attached — the port was green for a week while placing 122 blocks vanilla
# does not place at seed 12345 chunk (76,58) alone, because nothing ran it.
#
# `golden/decorate_vectors_iceberg.json` is one centre chosen *because* an
# iceberg places there: seed 12345, centre chunk (76,58), 10,774 draws and
# 1,678 changes in `minecraft:iceberg_packed`. Regenerate with
#
#   RUSTMC_JAVA_PROPS='-Ddecorate.seed=12345 -Ddecorate.centers=76,58' \
#       ci/decorate-golden.sh crates/rustmc-worldgen/golden/decorate_vectors_iceberg.json
#
# The coverage floor is deliberately not applied to an override dump (see
# `COMPUTED_FLOOR` in the gate) — one centre has a different denominator. Every
# other assertion the gate makes is the same assertion, including the RNG state
# after each feature, which is what actually catches a draw-count defect.
# check-test-targets: the next line is T1's workspace test selector.
RUSTMC_DECORATE_GOLDEN="$PWD/crates/rustmc-worldgen/golden/decorate_vectors_iceberg.json" \
    cargo test -q -p rustmc-worldgen --test golden_decorate --no-fail-fast

# --no-fail-fast, and not as a convenience. Without it cargo stops at the
# first failing test binary, and this repo has a known red (the 3-block
# climate tie in the carver gate). That red hid two more for weeks: the
# surface gate, which fails on the same 3 blocks, and a worldgen-assembly
# assertion left stale when the mineshaft caveat was reworded. A suite that
# stops at a known failure cannot tell you about a new one.
#
# Through ci/t1-par.sh, which runs cargo's test binaries T1_JOBS (default 12)
# at a time and rebuilds the serial log from them: cargo still chooses every
# binary, argument, cwd and environment. Measured on main 606551b54 the
# serial suite is 1430 s of test wall on 2 of 24 threads, 60% of it in four
# binaries of 0-3 tests. T1_JOBS=1 is the plain serial `cargo test`.
#
# **The sentinel is what makes this line the ledger's evidence rather than its
# excuse.** `ci/checks/test-targets.toml` puts 88 of the workspace's 110 cargo
# targets in buckets marked `runs_in = "t1"`, and until 2026-08-16 the only
# thing standing behind that was this line existing and twelve `reason` fields
# saying so — `ci/check-test-targets.py` read `ci/t0.sh` and had never read this
# file. Narrowing this selector took 88 targets out of every gate in the
# project and left every check in the tree green.
#
# `ci/check-test-targets.py` now recomputes what this argv selects and reddens
# T0 if the manifest claims a target runs here and it does not, or if any target
# is left selected by no stage at all. Narrow it and you must retire what you
# orphaned, by name, in the manifest.
# check-test-targets: the next line is T1's workspace test selector.
./ci/t1-par.sh test -q --workspace --no-fail-fast

# Same rule as `ci/t0.sh`: the sentinel is a claim that this run could have
# failed, and under `RUSTMC_FIXTURES=optional` it could not — fixture-backed
# tests skip instead. `ci/t1-gate.sh` adjudicates this suite's output and greps
# for its own sentinel, so the different string here also stops an optional-mode
# run being adjudicated as if it were a gate.
case "${RUSTMC_FIXTURES:-required}" in
    required) echo "T1 GREEN" ;;
    *) echo "T1 PASS — RUSTMC_FIXTURES=${RUSTMC_FIXTURES}, fixture-backed tests may have skipped; NOT a gate result" ;;
esac
