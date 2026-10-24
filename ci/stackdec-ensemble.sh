#!/usr/bin/env bash
# stackdec-ensemble.sh — ONE corpus-lock hold for the whole re-measure.
#
# Branch `trace-itemstack-legible` changes `tools/oracle-runner/src/trace.rs`,
# which is one of the three `PROJECTION_SOURCES`, so the projection identity
# moves and **every floor taken under the old one is stale**. Re-measuring it
# needs fresh vanilla recordings — the item fields do not exist in a recording
# taken before the decoder — and fresh recordings are the expensive part.
#
# The box-wide corpus lock is contended by a live merge-gate chain, and the
# selfcheck holds it for the length of the ensemble. So the floor re-measure,
# the census and the ensemble are one hold and not three: stages 2 and 3 read
# the recordings stage 1 leaves behind and boot nothing, so they cost the
# queue nothing extra by sitting inside the same window.
#
# WINDOW: stage 1 only. At SELFCHECK_RUNS=6 over ~93 scenarios that is roughly
# 1.5-2 hours on the Threadripper; stages 2 and 3 are seconds and are outside
# any lock. A merge gate still jumps the queue ahead of a run that has not
# started — `ci/corpus-lock.sh`, role `run`.
#
# NEVER `--update`. The standing floor in `ci/checks/gameplay-floor.toml` is
# stamped by the orchestrator's own gate, not by a branch measuring itself.
set -u
cd "$(dirname "$0")/.." || exit 2
ROOT="$PWD"
LOG="$ROOT/stackdec-ensemble.log"
REC="$ROOT/oracle-work/gameplay-selfcheck"

{
    echo "== stage 1: selfcheck ensemble, SELFCHECK_RUNS=${SELFCHECK_RUNS:-6}"
    date -Is
    SELFCHECK_RUNS="${SELFCHECK_RUNS:-6}" "$ROOT/ci/t2-gameplay-selfcheck.sh"
    echo "STAGE1-RC=$?"

    echo "== stage 2: corpus floor under the NEW projection"
    date -Is
    "$ROOT/ci/corpus-floor.sh" "$REC"/r[0-9]*.json
    echo "STAGE2-RC=$?"

    echo "== stage 3: vanilla reach census, including the item registry"
    date -Is
    "$ROOT/ci/vanilla-reach-census.py" --recordings "$REC/r[0-9]*.json"
    echo "STAGE3-RC=$?"

    echo "STACKDEC-CHAIN-DONE"
    date -Is
} >"$LOG" 2>&1
