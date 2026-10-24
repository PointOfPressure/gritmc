#!/usr/bin/env bash
# ci/t1-gate.sh — run T1 and adjudicate it against the declared known reds.
#
# `ci/t1.sh` exits 101 and always has: two golden-vector gates are honestly
# red (`carved_chunks_match_vanilla`, `surface_chunks_match_vanilla` — one
# defect in the surface rule, reported twice). A permanently-red stage gets
# switched off within a day, which is why T1 has never been in a merge gate
# and why **every test binary beyond T0's `--lib --bins` selection — every
# golden vector and every integration test under `tests/` — has been outside
# every merge gate for the project's whole history.** The count is not
# written here because it moves as crates are added; `T1-GATE GREEN` below
# prints the measured figure, and `min_binaries` in
# `ci/checks/t1-known-red.toml` is the floor actually enforced.
#
# This is the piece that lets it in: run the suite, then let
# `ci/t1-allowlist.py` decide, by **signature** rather than by name. See that
# file for why a name-only allowlist is the stale-baseline defect wearing a
# new hat.
#
# It prints one line, `T1-GATE GREEN` or `T1-GATE RED`, and keeps the full
# suite's log on disk rather than in its caller's log. The sentinel is
# deliberately not `T1 GREEN`: `ci/t1.sh` prints that itself on a suite with
# no failures at all, which — with two reds declared — is a *stale allowlist*
# and must be red here. Two different claims must not share a string a gate
# greps for.
#
# Usage:  ./ci/t1-gate.sh
#   T1_LOG   where to keep the full suite output; default ci/logs/t1-<stamp>.log
# Exit:   0 green | 1 red | 2 could not run
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2

STAMP="$(date +%Y%m%d-%H%M%S)"
LOG="${T1_LOG:-$ROOT/ci/logs/t1-$STAMP.log}"
mkdir -p "$(dirname "$LOG")" || exit 2

# Refuse before running rather than adjudicate afterwards.
#
# `RUSTMC_FIXTURES=optional` turns every vanilla-backed test in the workspace
# from "fails without its fixture" back into "skips without its fixture"
# (`rustmc_data::fixture`). A suite run that way cannot fail for the reason
# this stage exists to detect, so adjudicating its output would be adjudicating
# a run that could not have been red. `ci/t1.sh` already prints a different
# sentinel under it; this refuses eleven minutes earlier and says why.
if [ "${RUSTMC_FIXTURES:-required}" != "required" ]; then
    echo "T1-GATE RED — RUSTMC_FIXTURES=${RUSTMC_FIXTURES} is set in this environment"
    echo "              fixture-backed tests would skip rather than fail, so this"
    echo "              suite could not have gone red for a missing fixture."
    exit 1
fi

# Not piped, and not `&&`. A pipeline exits with its *last* command's status,
# which has masked a failure here twice; and T1's own exit is 101 by design,
# so it is captured and reported rather than acted on. The adjudication is the
# verdict.
START=$(date +%s)
./ci/t1.sh > "$LOG" 2>&1
T1_RC=$?
WALL=$(( $(date +%s) - START ))

echo "T1 ran ${WALL}s, exit $T1_RC, log $LOG"

# **The skip guard was here, it could not fire, and this is what replaced it.**
#
# What stood here greps `$LOG` for `skipping: no vanilla` and refuses a run that
# matched, over a comment calling it "the whole point" of the stage and ending
# "The guard lives where the output does." The output does not exist. Two
# independent reasons, either alone fatal:
#
#   * **cargo captures a passing test's output.** A test that prints
#     `skipping: ...` and returns early *passes*, and `cargo test` never prints
#     a passing test's stdout or stderr. `ci/t1.sh` passes no `--nocapture`, so
#     the bytes are never written. `crates/rustmc-data/src/fixture.rs` says this
#     outright in its module docs — "that grep structurally cannot see any of
#     these" — and the guard was written here afterwards anyway.
#   * **the pattern came from one example.** Of the 119 skip-message sites in
#     `crates/` and `tools/`, `skipping: no vanilla` matches 27. The largest
#     single wording, `skipping: no datagen extract` (36 sites), does not.
#
# Measured 2026-08-18, ~/rustmc-t1 on `main`, `oracle-work/vanilla-26.2` moved
# aside: 111 binaries, 1749 passed against 1758 with it present, 8 ignored
# either way, `grep -c skipping` **0**, 122 s against 1521 s. Every floor in
# `t1-known-red.toml` passed. `docs/PARITY-ROADMAP.md` §73.
#
# The replacement is the same inversion this file already applies to
# `RUSTMC_FIXTURES` thirty lines up: **refuse before running rather than
# adjudicate afterwards.** A fixture that is absent is knowable in
# milliseconds, from the filesystem, without any test having emitted anything —
# so nothing here depends on a grep, and no grep can be too narrow.
#
# The exit is 2, not 1. A gate directory missing its references says nothing
# about the branch, which is exactly the distinction `T0 REFUSED` draws; a
# stage that reports RED for an environment fault sends the wrong lane looking.
#
# NAMED INDIVIDUALLY, NOT AS A DIRECTORY. `oracle-work/` exists in every gate
# dir as soon as `ci/handoff.sh` stage 1 has run, whether or not the references
# under it resolve — they are symlinks into the shared tree, and a broken link
# leaves the parent looking perfectly healthy. Each entry below is a file some
# gate actually opens.
REQUIRED_FIXTURES="
oracle-work/vanilla-26.2/generated-data/data/minecraft/worldgen/noise_settings/overworld.json|the datagen extract, asked by worldgen_assembly::have_extract for 20 gates|ci/vanilla-server.sh data
data/vanilla/block_props.json|the block-props extract, asked by state_spec::extract_or_skip for 23 gates|ci/block-props-golden.sh
data/vanilla/collision_shapes.json|the collision-shape extract, asked by 4 entity_move gates|ci/collision-shapes-golden.sh
"
MISSING=0
while IFS='|' read -r rel what cmd; do
    [ -n "$rel" ] || continue
    if [ ! -e "$ROOT/$rel" ]; then
        [ "$MISSING" -eq 0 ] && echo "T1-GATE REFUSED — this gate directory cannot compare against vanilla"
        echo "              missing: $rel"
        echo "                       $what"
        echo "                       produce it with: $cmd"
        MISSING=$((MISSING + 1))
    fi
done <<EOF
$REQUIRED_FIXTURES
EOF
if [ "$MISSING" -ne 0 ]; then
    echo "              $MISSING fixture(s) absent. The suite would still have printed"
    echo "              111 binaries and ~1749 passed, because a test that skips passes"
    echo "              and cargo prints nothing for it. See docs/PARITY-ROADMAP.md 73."
    exit 2
fi

./ci/t1-allowlist.py "$LOG"
RC=$?

# On red, show enough to act on without shipping the whole suite's output up
# the wire. The adjudicator has already printed *which* signatures moved.
if [ "$RC" -ne 0 ]; then
    echo "--- failing tests in $LOG ---"
    # `thread .*panicked`, not `thread '.*' panicked`: the panic line carries a
    # thread id in parentheses between the name and the verb, so the anchored
    # form matches nothing. Same bug as `ci/t1-allowlist.py`'s first PANIC
    # regex, written twice within the hour — which is the argument for it being
    # a named constant somewhere if a third copy ever appears.
    grep -E "^(---- .* stdout ----|error(\[|:)|thread .*panicked)" "$LOG" | head -20
fi
exit "$RC"
