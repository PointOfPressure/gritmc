#!/usr/bin/env bash
# corpus-load-band.sh — put this box in the 50-110 load band WITHOUT harming the
# production corpus recording that is holding the lock.
#
#     ./ci/corpus-load-band.sh --calibrate <workers> <cycles> <out.tsv>
#     ./ci/corpus-load-band.sh --hold      <workers> <seconds> [out.tsv]
#     ./ci/corpus-load-band.sh --stop
#     ./ci/corpus-load-band.sh --report    <out.tsv>
#
# # WHY THIS EXISTS
#
# `docs/CORPUS-LOCK-HOLD.md` §15 measured concurrency at loads 0.1 to 18.3 and
# declined to force 50-110, for a reason that was correct: a live production
# recording held the lock and loading the box would have inflicted §14's
# `settle`-cap contamination on somebody else's merge.
#
# **That is a scheduling constraint and this box no longer offers a window to
# schedule into.** At 13:09 on 2026-08-18 the lock was held with four runs
# queued behind it, the oldest waiting 211 minutes. A queue carrying 211 minutes
# of backlog does not drain, so "wait for an idle box" is not a plan, it is a
# way of never taking the measurement and calling that caution.
#
# So the window is constructed. Three properties, and the third is the one that
# turns §15's assumption into a reading:
#
#   * **`nice 19`, for the load AND for both arms of whatever runs beside it.**
#     CFS weights are relative. A `nice 19` recorder among `nice 19` competitors
#     sits in the same place in the runqueue as a `nice 0` recorder among
#     `nice 0` competitors, so the contention the experiment feels is the
#     contention it is trying to reproduce. A production recording at `nice 0`
#     has 68 times the weight of each of these workers and is very nearly
#     insulated from all of them;
#   * **`taskset` to half the box.** The production recording is unpinned, so
#     the scheduler always has twelve CPUs to place it on that this script
#     cannot touch. The quantity being reproduced is runqueue depth per CPU, not
#     the number printed by `uptime`: 55 runnable workers over 12 CPUs is depth
#     4.6, which is what box load 110 over 24 CPUs means. Both numbers are
#     reported and they are not the same number;
#   * **the holder's own runqueue delay is sampled throughout**, by
#     `ci/corpus-holder-rate.sh`, and the load is killed if it rises. §15 argued
#     the risk. This measures it, on the run being protected, while it is being
#     protected.
#
# # THE LOAD CANNOT OUTLIVE ITS BUDGET, EVEN IF THIS SCRIPT IS KILLED
#
# Every worker is `timeout <seconds> bash -c 'while :; do :; done'`. The budget
# is inside the worker, not in a supervisor that has to survive to enforce it.
# SIGKILL this script, close the terminal, lose the ssh connection: the load
# still stops on schedule. A load generator whose stop condition lives in the
# thing most likely to die is how a box gets left at load 110 overnight.
#
# # WHAT --calibrate IS FOR, AND WHY IT ALTERNATES
#
# The box's own load moved between 7.2 and 32.3 inside four minutes today, so
# "the holder's delay share was X while I was loading" is not attributable to
# the loading. `--calibrate` alternates the load on and off in equal cycles and
# pairs each ON window with the OFF window beside it, which differences out a
# background that drifts on the same timescale. It reports the pairing, not an
# average, and it is what licenses `--hold`.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

NCPU="$(nproc)"
CPUS="${LOADBAND_CPUS:-0-$(( NCPU / 2 - 1 ))}"
NICE="${LOADBAND_NICE:-19}"
PIDFILE="${LOADBAND_PIDFILE:-/tmp/rustmc-load-band.$USER.pids}"
# How much of a rise in the holder's runqueue delay share is tolerated before
# the load is killed. The share is dimensionless and sat between 0.07 and 0.70
# on an unloaded-by-me box today, so this is deliberately not a tight bound: it
# is a circuit breaker for a clear harm, not a significance test.
ABORT_DELTA="${LOADBAND_ABORT_DELTA:-0.20}"
# Both windows are overridable so `ci/corpus-load-band-falsify.sh` can drive
# the whole control flow in seconds. A guard that can only be exercised by a
# ninety-second baseline is a guard nobody exercises.
BASELINE_S="${LOADBAND_BASELINE_S:-90}"
SAMPLE_S="${LOADBAND_SAMPLE_S:-30}"
SETTLE_S="${LOADBAND_SETTLE_S:-60}"

cpus_in_set() {   # how many CPUs the mask names
    local n=0 part a b
    IFS=, read -ra P <<< "$CPUS"
    for part in "${P[@]}"; do
        case "$part" in
            *-*) a="${part%-*}"; b="${part#*-}"; n=$((n + b - a + 1)) ;;
            *)   n=$((n + 1)) ;;
        esac
    done
    echo "$n"
}

start_load() {    # start_load <workers> <seconds>
    local w="$1" s="$2" i
    : > "$PIDFILE"
    for i in $(seq 1 "$w"); do
        # The trailing argument becomes the spinner's $0, so a stray worker can
        # be found by name from any shell on the box:
        #     pgrep -fa rustmc-corpus-[l]oad-worker
        # The pidfile holds the `timeout` processes, NOT the spinners: timeout
        # forks its child, so the recorded pid is a sleeping supervisor and only
        # its child is runnable. That cost a falsifier case, which reported
        # `0 of 8 workers were runnable` for a load that was working perfectly.
        setsid taskset -c "$CPUS" nice -n "$NICE" \
            timeout "$s" bash -c 'while :; do :; done' rustmc-corpus-load-worker \
            >/dev/null 2>&1 &
        echo $! >> "$PIDFILE"
    done
}
stop_load() {
    [ -s "$PIDFILE" ] || return 0
    local p
    while read -r p; do
        [ -n "$p" ] && kill -- -"$p" 2>/dev/null
        [ -n "$p" ] && kill "$p" 2>/dev/null
    done < "$PIDFILE"
    : > "$PIDFILE"
}
trap 'stop_load' EXIT INT TERM

holder_share() {  # one delay-share sample over <secs>, or "-" if no holder
    local secs="$1" tmp
    tmp="$(mktemp)"
    if "$ROOT/ci/corpus-holder-rate.sh" "$secs" "$secs" "$tmp" band >/dev/null 2>&1; then
        awk -F'\t' 'NR>1 {s=$8} END{print (s=="")? "-" : s}' "$tmp"
    else
        echo "-"
    fi
    rm -f "$tmp"
}

case "${1:-}" in
--stop)   stop_load; echo "load stopped"; exit 0 ;;
--report)
    OUT="${2:?out.tsv}"
    awk -F'\t' 'NR>1 && $1 !~ /^CALIBRATION-/ {
        if ($2=="on")  { on_l+=$3; on_d+=($5=="-"?0:$5); on_n++; if($5!="-") on_dn++ }
        if ($2=="off") { of_l+=$3; of_d+=($5=="-"?0:$5); of_n++; if($5!="-") of_dn++ }
    } END {
        printf "  ON  windows %d  mean box load1 %.1f  mean holder delay share %s\n", on_n, (on_n?on_l/on_n:0), (on_dn? sprintf("%.3f", on_d/on_dn) : "n/a")
        printf "  OFF windows %d  mean box load1 %.1f  mean holder delay share %s\n", of_n, (of_n?of_l/of_n:0), (of_dn? sprintf("%.3f", of_d/of_dn) : "n/a")
        if (on_dn && of_dn) printf "  holder delay share, ON minus OFF: %+.3f\n", on_d/on_dn - of_d/of_dn
    }' "$OUT"
    exit 0 ;;
--calibrate) MODE=calibrate; WORKERS="${2:?workers}"; CYCLES="${3:?cycles}"; OUT="${4:?out.tsv}" ;;
--hold)      MODE=hold;      WORKERS="${2:?workers}"; SECS="${3:?seconds}";  OUT="${4:-/dev/null}" ;;
*) sed -n '2,8p' "$0"; exit 2 ;;
esac

NSET="$(cpus_in_set)"
HOLDER="$("$ROOT/ci/corpus-holder-rate.sh" --holder 2>/dev/null || true)"
echo "corpus-load-band: $WORKERS workers, nice $NICE, cpus $CPUS ($NSET of $NCPU)"
echo "                  runqueue depth this aims at: $(awk -v w="$WORKERS" -v n="$NSET" 'BEGIN{printf "%.2f", w/n}') per CPU in the set"
echo "                  the same depth over the whole box would read as load $(awk -v w="$WORKERS" -v n="$NSET" -v N="$NCPU" 'BEGIN{printf "%.0f", w/n*N}')"
if [ -n "$HOLDER" ]; then
    echo "                  production lock holder: $HOLDER (its delay share is the abort criterion, +$ABORT_DELTA)"
else
    echo "                  NOBODY holds the corpus lock right now, so there is no production recording to protect"
fi

if [ "$MODE" = calibrate ]; then
    WIN="${LOADBAND_WINDOW:-180}"
    printf 'cycle\tphase\tload1\tload5\tshare\ttime\n' > "$OUT"
    for c in $(seq 1 "$CYCLES"); do
        for phase in off on; do
            [ "$phase" = on ] && start_load "$WORKERS" "$WIN"
            # Let the one-minute average catch up before the sample window that
            # is kept. A 60s EWMA read 5s after a step change is a reading of
            # the previous phase. Measured on 2026-08-18: 40 workers took the
            # box from 16 to 62 over about ninety seconds, and a sample at ten
            # seconds would have recorded 29.
            #
            # Overridable so `ci/corpus-load-band-falsify.sh` can exercise the
            # completeness assertion in seconds. It is NOT a tuning knob: the
            # settle is what makes `load1` mean the phase it is filed under.
            sleep "$SETTLE_S"
            S="$(holder_share $(( WIN - SETTLE_S - 10 )))"
            read -r L1 L5 _ < /proc/loadavg
            printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$c" "$phase" "$L1" "$L5" "$S" "$(date +%T)" >> "$OUT"
            echo "  cycle $c $phase: load1 $L1 load5 $L5 holder-delay-share $S"
            [ "$phase" = on ] && stop_load
            sleep 10
        done
    done
    # **It must be able to say it FINISHED, not merely that it stopped.**
    # A truncated run and a small answer are indistinguishable without this:
    # the vanilla draw oracle was found on 2026-08-18 to have been silently
    # aborting for nine days, reporting 3 chunks where 15 were real, and every
    # reader of it saw a plausible small number rather than a failure. So the
    # expected row count is asserted against the actual one and the sentinel is
    # written last, after the assertion.
    WANT=$(( CYCLES * 2 ))
    GOT="$(awk 'NR>1' "$OUT" | wc -l)"
    if [ "$GOT" -ne "$WANT" ]; then
        echo "CALIBRATION INCOMPLETE: $GOT of $WANT windows written." >&2
        echo "  Do not read the report below as a calibration; it is a fragment." >&2
        "$0" --report "$OUT"
        exit 8
    fi
    echo
    "$0" --report "$OUT"
    printf 'CALIBRATION-COMPLETE\t%s\t%s windows\t%s\n' "$(date +%T)" "$GOT" "$WORKERS" >> "$OUT"
    echo "CALIBRATION-COMPLETE $GOT/$WANT windows at $WORKERS workers"
    exit 0
fi

# --hold: sustained, with the circuit breaker armed.
BASE="-"
if [ -n "$HOLDER" ]; then
    echo "  baseline: sampling the holder for ${BASELINE_S}s before any load"
    BASE="$(holder_share "$BASELINE_S")"
    echo "  baseline delay share: $BASE"
fi
start_load "$WORKERS" "$SECS"
echo "  load up at $(date +%T), budget ${SECS}s (enforced inside each worker)"
END=$(( $(date +%s) + SECS ))
BREACHES=0
while [ "$(date +%s)" -lt "$END" ]; do
    S="$(holder_share "$SAMPLE_S")"
    read -r L1 _ < /proc/loadavg
    [ "$OUT" != /dev/null ] && printf '%s\tload\t%s\t-\t%s\t%s\n' 0 "$L1" "$S" "$(date +%T)" >> "$OUT"
    echo "  load1 $L1  holder-delay-share $S  (baseline $BASE)"
    if [ "$BASE" != "-" ] && [ "$S" != "-" ]; then
        if awk -v s="$S" -v b="$BASE" -v d="$ABORT_DELTA" 'BEGIN{exit !(s - b > d)}'; then
            BREACHES=$((BREACHES + 1))
            echo "  !! holder delay share $S is more than $ABORT_DELTA above baseline $BASE (breach $BREACHES/2)"
            if [ "$BREACHES" -ge 2 ]; then
                echo "  ABORTING the load: the production holder is being harmed."
                stop_load
                exit 7
            fi
        else
            BREACHES=0
        fi
    fi
done
stop_load
# Same discipline for --hold: a block that was cut short must not read as a
# block that ran. `ci/corpus-load-band-experiment.sh` greps for this.
if [ "$(date +%s)" -ge "$END" ]; then
    echo "  load down at $(date +%T)"
    echo "LOAD-BLOCK-COMPLETE ${SECS}s at $WORKERS workers"
else
    echo "  load down EARLY at $(date +%T), $(( END - $(date +%s) ))s short of its budget"
    echo "LOAD-BLOCK-INCOMPLETE"
fi
