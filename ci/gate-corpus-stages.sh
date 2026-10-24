#!/usr/bin/env bash
# gate-corpus-stages.sh — `ci/handoff.sh` stages 6 and 7, run inside ONE corpus-lock hold.
#
#     RUSTMC_CORPUS_LOCK_ROLE=gate ./ci/corpus-lock.sh hold -- \
#         ./ci/gate-corpus-stages.sh <persist:0|1> <remote-dir> <persistence-applies-word>
#
# Only `ci/handoff.sh` calls this. It prints one `GATE-STAGE-RC <stage> <rc>`
# line per stage it ran, and handoff reads its verdicts from those lines, so a
# stage that did not run has no line rather than a made-up status.
#
# WHY ONE HOLD. The two stages used to be two remote calls, each harness taking
# the corpus lock and releasing it. Between them the lock was free, and a run
# already queued behind the gate took it at once; a running run is never
# preempted, so the gate then waited that run out before it could start its
# persistence stage. Measured on train-a's gate by the gate-pipeline lane:
# 8.2 of 146.8 minutes at stage 7. Inside `corpus-lock.sh hold`, each
# harness's own `corpus_lock_acquire` is served from the hold
# (`RUSTMC_CORPUS_LOCK_HELD_BY`), so the box is the gate's from the first
# recording of stage 6 to the last gate of stage 7. The evidence that a queued
# run cannot get between them is `case_hold_spans_harnesses` in
# `ci/corpus-lock-falsify.sh`, with the injection `hold-takes-no-lock`.
set -uo pipefail
PERSIST="${1:?persist 0|1}"; DIR="${2:?remote dir}"; APPLIES="${3:-}"
./ci/t2-gameplay.sh
T2=$?
echo "GATE-STAGE-RC 6 $T2"
if [ "$T2" -eq 0 ] && [ "$PERSIST" = 1 ]; then
    echo "== stage 7: T2 persistence in ~/$DIR ($APPLIES)"
    ./ci/t2-persistence.sh
    echo "GATE-STAGE-RC 7 $?"
fi
exit "$T2"
