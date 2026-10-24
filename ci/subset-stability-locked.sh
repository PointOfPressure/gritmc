#!/usr/bin/env bash
# subset-stability-locked.sh — one ensemble, one corpus lock.
#
#     ./ci/subset-stability-locked.sh 6 730-*.toml 731-*.toml ...
#
# `ci/subset-stability.sh` drives `ci/t2-gameplay-subset.sh` once per run, and
# that script takes the box-wide corpus lock and hands it back **per run** —
# deliberately, so a merge gate can cut in between recordings instead of
# waiting out the whole ensemble. That is the right default and this file does
# not change it.
#
# # WHY THE DEFAULT IS WRONG WHEN A GATE CHAIN IS LIVE
#
# Handing the lock back between runs means re-queueing between runs. Measured
# on 2026-08-12 while this was written: the lock was held by a merge gate in
# `~/rustmc-stack` with a second `run` already waiting behind it. An N = 6
# ensemble that releases after each recording joins the back of that queue six
# times, so its wall clock is not six recordings but six *queue traversals* of
# a chain that is landing branches all afternoon.
#
# Worse than slow, it is **not the same measurement**. The ensemble's whole
# claim is that the N recordings are independent samples of one unchanged
# reference. Six recordings spread across an afternoon of merges are taken
# against a box whose state moved between them, and a flap that correlates
# with what else was running is indistinguishable from a flap that does not.
#
# # WHAT THIS DOES
#
# Takes the real box-wide lock **once**, as role `run` so a real merge gate
# still jumps the queue ahead of us, and holds it for the whole ensemble. The
# inner per-run acquisitions are then pointed at a private lock namespace via
# `RUSTMC_CORPUS_LOCK_DIR`, the same escape `ci/corpus-lock-falsify.sh` uses,
# where they succeed instantly and exclude nobody. Exclusion is not lost — it
# moved from six short holds to one long one, which is the point.
#
# # THE WINDOW, STATED, BECAUSE THE RULE REQUIRES IT
#
# `docs/AGENT-BRIEF.md` sanctions a bench sitting inside the corpus lock only
# if its header says how long the window is. **This one is bounded by
# `MAX_WINDOW_S`, default 5400s (90 min), enforced rather than promised**: the
# ensemble is killed and the lock released if it overruns, because a hold whose
# ceiling is a comment is a hold with no ceiling. Six checks at roughly two to
# five minutes a recording is 12-30 minutes of real work; the ceiling is set
# well above that so a slow box does not throw away a half-finished ensemble,
# and an overrun is reported as a refusal rather than a floor.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

[ "$#" -ge 2 ] || { echo "usage: $0 <runs> <check.toml> [check.toml ...]" >&2; exit 2; }

MAX_WINDOW_S="${MAX_WINDOW_S:-5400}"

# Sourced BEFORE any private namespace is exported — ci/corpus-lock.sh reads
# RUSTMC_CORPUS_LOCK_DIR at source time, so exporting it first would take the
# outer lock in the private namespace too and exclude nobody at all.
[ -z "${RUSTMC_CORPUS_LOCK_DIR:-}" ] || {
    echo "subset-stability-locked: refusing — RUSTMC_CORPUS_LOCK_DIR is already set" >&2
    echo "  the outer lock must be the real box-wide one, not a private namespace" >&2
    exit 2
}
RUSTMC_CORPUS_LOCK_ROLE=run
export RUSTMC_CORPUS_LOCK_ROLE
. "$ROOT/ci/corpus-lock.sh"

NS=""
CHILD=""
cleanup() {
    [ -n "$CHILD" ] && kill "$CHILD" 2>/dev/null || true
    if declare -F corpus_lock_release >/dev/null 2>&1; then corpus_lock_release; fi
    [ -n "$NS" ] && rm -rf "$NS" || true
}
trap cleanup EXIT

echo "subset-stability-locked: taking the box-wide corpus lock as role 'run' for the whole ensemble"
echo "subset-stability-locked:   window ceiling ${MAX_WINDOW_S}s — overrun is a refusal, not a floor"
corpus_lock_acquire

NS="$(mktemp -d "${TMPDIR:-/tmp}/rustmc-subset-ns.XXXXXX")"
echo "subset-stability-locked: inner per-run locks -> private namespace $NS"

START="$(date +%s)"
RUSTMC_CORPUS_LOCK_DIR="$NS" ./ci/subset-stability.sh "$@" &
CHILD=$!

# Enforce the ceiling without a foreground `sleep` loop over the whole window:
# poll cheaply, and only until the child exits or the window closes.
RC=0
while kill -0 "$CHILD" 2>/dev/null; do
    if [ "$(( $(date +%s) - START ))" -ge "$MAX_WINDOW_S" ]; then
        echo "subset-stability-locked: REFUSED — ensemble exceeded ${MAX_WINDOW_S}s, killing and releasing" >&2
        kill "$CHILD" 2>/dev/null || true
        wait "$CHILD" 2>/dev/null || true
        CHILD=""
        exit 5
    fi
    sleep 10
done
wait "$CHILD" || RC=$?
CHILD=""
echo "subset-stability-locked: ensemble finished in $(( $(date +%s) - START ))s, rc=$RC"
exit "$RC"
