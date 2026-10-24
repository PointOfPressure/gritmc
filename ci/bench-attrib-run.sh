#!/usr/bin/env bash
# bench-attrib-run.sh — the attribution sweep, in two phases.
#
# # Why two phases, and why the first one does not wait for anything
#
# The first attempt at this sweep took the box-wide corpus lock and then
# refused to measure until the box was quiet. It aborted after ten minutes and
# published nothing, and it was right to: a merge gate's recorder was live
# alongside it. But this box is contended for hours at a stretch — a merge
# chain, an agent on the aquifer item, gates back to back — so "wait for
# quiet" is not a strategy that terminates.
#
# So the sweep is split by what each number actually needs:
#
#   PHASE 1  instructions retired, RUSTMC_WORKERS=1, NO LOCK, runs immediately.
#            This is the attribution. Instructions retired measures work done
#            rather than time taken, so a neighbour on the box does not move
#            it; one worker removes the pool's own nondeterminism. It answers
#            "did this code start doing more work", which is the question.
#
#   PHASE 2  CPU-seconds, RUSTMC_WORKERS=12, under the corpus lock, best
#            effort. This is the quantity docs/BENCH.md speaks in, so it is
#            worth having, but it is contention-sensitive and therefore
#            corroboration rather than the verdict. It runs whether or not the
#            box goes quiet, and every row carries its own contention count so
#            a contaminated figure is identifiable rather than silently
#            averaged in.
#
# The ordering is the point: the answer does not depend on the box, and only
# the restatement of it in CPU-seconds does.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
LOGDIR=/home/sayer/grit/logs
mkdir -p "$LOGDIR"

# perf is one of AGENT-BRIEF's named instruments and is unusable at the box's
# default perf_event_paranoid=4. Lower it if we can; the phase still runs
# without perf, it just loses the contention-immune column and says so.
if [ "$(cat /proc/sys/kernel/perf_event_paranoid 2>/dev/null || echo 9)" -gt 1 ]; then
    sudo -n sysctl -w kernel.perf_event_paranoid=1 >/dev/null 2>&1 || true
fi
echo "perf_event_paranoid=$(cat /proc/sys/kernel/perf_event_paranoid)"

PAIRS=(
    "chambers-C-vs-D|C:rustmc-benchC:5b0d5aed|D:rustmc-benchD:d2f7c6e5"
    "span-A-vs-B|A:rustmc-benchA:ed0a29ec|B:rustmc-benchB:9b4afd4e"
    "retake-A-vs-today|A:rustmc-benchA:ed0a29ec|T:rustmc-bench:35b3bde5"
)

run_phase() {                   # run_phase <tag> <workers> <reps> <radius> [only-label]
    local tag="$1" workers="$2" reps="$3" rad="$4" only="${5:-}"
    local spec label sx sy
    for spec in "${PAIRS[@]}"; do
        label="${spec%%|*}"
        [ -n "$only" ] && [ "$label" != "$only" ] && continue
        sx="$(echo "$spec" | cut -d'|' -f2)"
        sy="$(echo "$spec" | cut -d'|' -f3)"
        echo
        echo "############ $tag r=$rad / $label ############"
        RUSTMC_WORKERS="$workers" BENCH_PAIR_LABEL="$tag-r$rad-$label" \
            ./ci/bench-pair.sh "$sx" "$sy" "$reps" "$rad" \
            2>&1 | tee "$LOGDIR/bench-attrib-$tag-r$rad-$label.log"
    done
}

echo "############################################################"
echo "# PHASE 1 — instructions retired, 1 worker, no lock, now"
echo "############################################################"
#
# TWO RADII, and this is the decisive part of the design. A smoke test at
# r=2 put the chambers at +20% instructions with ALL THREE centres moving,
# when only 0.15 chamber starts are expected per r=2 square. That is the
# signature of a FIXED per-process cost — loading a 692-piece template pool
# at startup — not of chamber generation. A fixed cost is diluted as the
# square grows, so it CANNOT produce a drift that is near-uniform at r=7 and
# r=32, which is what docs/BENCH.md §3 reports.
#
# Measuring the same pair at r=7 and r=16 separates them: the absolute
# instruction delta stays constant if the cost is fixed and grows with the
# generated area if it is per-chunk. That is a two-parameter question and
# this is the two-point measurement that answers it.
run_phase ins1 1 "${BENCH_REPS_INS:-2}" 7
run_phase ins1 1 "${BENCH_REPS_INS16:-2}" 16 chambers-C-vs-D

echo
echo "############################################################"
echo "# PHASE 2 — CPU-seconds, 12 workers, under the corpus lock"
echo "############################################################"

# shellcheck source=/dev/null
. "$ROOT/ci/corpus-lock.sh"
released=""
cleanup() {
    [ -n "$released" ] && return 0
    released=1
    sudo -n rm -f /run/ci-busy 2>/dev/null || rm -f /run/ci-busy 2>/dev/null
    corpus_lock_release 2>/dev/null || true
    echo "corpus lock: released at $(date -Is)"
}
trap cleanup EXIT INT TERM

echo "=== acquiring the box-wide corpus lock as role 'run' at $(date -Is) ==="
RUSTMC_CORPUS_LOCK_ROLE=run corpus_lock_acquire
echo "=== got the box at $(date -Is) ==="
sudo -n touch /run/ci-busy 2>/dev/null || touch /run/ci-busy 2>/dev/null || true

# Give a bare `cargo build` in another tree a bounded chance to drain — the
# corpus lock does not exclude one. Unlike the first version this does NOT
# abort: phase 1 already holds the attribution, and a contended CPU-second
# figure that is labelled contended is worth more than no figure at all.
for attempt in 1 2 3 4 5; do
    echo "--- quiet preflight, attempt $attempt ---"
    ./ci/bench-quiet.sh && { echo "=== box is quiet ==="; break; }
    [ "$attempt" -eq 5 ] && {
        echo "!!! STILL CONTENDED — measuring anyway. Every row carries its"
        echo "!!! own contention count and the summary flags the total. CPU-s"
        echo "!!! from this phase is corroboration, NOT a quotable absolute."
        break
    }
    echo "--- contended; sleeping 60s ---"
    sleep 60
done

run_phase cpu12 12 "${BENCH_REPS_CPU:-4}" 7

echo
echo "=== ALL PHASES DONE at $(date -Is) ==="
