#!/usr/bin/env bash
# corpus-healthy-baseline.sh — what a HEALTHY corpus run looks like to the three
# instruments the 2026-08-07 overlap was diagnosed with.
#
#     ./ci/corpus-healthy-baseline.sh <minutes> <out.tsv>
#
# # WHY THIS EXISTS
#
# `ci/corpus-lock.sh`'s header diagnoses the 2026-08-07 overlap as a stall, and
# names three instruments that "agree that almost nothing happened":
#
#   * each `oracle-runner record` accumulated 1-2 CPU-seconds in 17 minutes;
#   * each vanilla log sat at 2 client joins for 7+ minutes, with no writes;
#   * the one-minute load average read 0.06 to 0.30.
#
# None of the three was ever read against a run that was known to be *fine*.
# That is the whole gap this file closes. It attaches to whichever run
# currently holds the corpus lock — a run that by construction has the box to
# itself — and reads exactly those three numbers off it, plus the harness's own
# `recorded N/M` counter, which is the only one of the four that actually
# measures throughput.
#
# # IT PERTURBS NOTHING
#
# It reads `/proc`, one `stat`, and one `grep -c` per sample, once every 30
# seconds. It never writes into the run's tree, never signals anything, and
# takes no lock. A control that changed what it was controlling would be
# useless here, because the thing being measured is a run's idleness.
#
# # WHAT IT CANNOT SETTLE
#
# It says what a healthy solo run looks like. It does not say what an
# interfering pair looks like — that is `ci/corpus-exclusion-experiment.sh`.
# The pair of them is the argument: if a healthy run and a "stalled" pair are
# indistinguishable to an instrument, that instrument did not diagnose the
# stall, whatever else may have.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# The holder's tree, out of the lock's own status text.
#
# **The whole risk is picking a WAITER instead of the holder.** `status` prints
# the holder as "... in <tree> since <timestamp>" and each queued run as
# "... in <tree>, waiting 108m". A pattern that matched only " in <path>" would
# happily attach to the oldest queued run, which is a run doing nothing, and
# the baseline would then be a measurement of a process that is blocked on
# `flock` rather than of a healthy recording. Requiring the trailing " since "
# is what separates them, and `ci/corpus-healthy-baseline-falsify.sh` case 3 is
# a status block with four waiters that must still yield the holder.
#
# Split out and fed from stdin so it can be tested without a lock, a box or a
# run. An instrument whose only mode needs the thing it measures cannot be
# falsified before it is trusted.
holder_from_status() {
    sed -n 's/^[[:space:]]*record:.* in \(\/[^ ]*\) since .*/\1/p' | head -1
}

if [ "${1:-}" = "--holder-from-status" ]; then
    holder_from_status
    exit 0
fi

MINS="${1:-17}"
OUT="${2:?usage: $0 <minutes> <out.tsv>}"
SAMPLES=$(( MINS * 2 ))

HOLDER="$("$ROOT/ci/corpus-lock.sh" status 2>/dev/null | holder_from_status)"
if [ -z "$HOLDER" ] || [ ! -d "$HOLDER" ]; then
    echo "corpus-healthy-baseline: nobody holds the corpus lock — nothing to control against" >&2
    exit 3
fi
VLOG="$HOLDER/oracle-work/gameplay/vanilla/logs/latest.log"
echo "corpus-healthy-baseline: sampling $HOLDER for ${MINS}m into $OUT" >&2

HZ="$(getconf CLK_TCK)"
printf 'sample\ttime\tholder\trec_pid\tcpu_s\telapsed_s\tvlog_bytes\tvlog_age_s\tjoins\tload1\n' > "$OUT"

for i in $(seq 1 "$SAMPLES"); do
    # The recorder inside the holder's tree, not any recorder on the box.
    P="$(pgrep -f "oracle-runner record" 2>/dev/null | while read -r p; do
            if tr '\0' ' ' < "/proc/$p/cmdline" 2>/dev/null | grep -cF "$HOLDER" >/dev/null; then
                echo "$p"; break
            fi
         done)"
    # Fall back to any recorder only when the holder's own cannot be identified,
    # and mark it, so a row that is not what it claims is visible rather than
    # averaged in.
    TAG=holder
    if [ -z "$P" ]; then P="$(pgrep -f 'oracle-runner record' | head -1)"; TAG=unattributed; fi

    CPU=-1; ELA=-1
    if [ -n "$P" ] && [ -r "/proc/$P/stat" ]; then
        read -r ut st <<< "$(awk '{print $14, $15}' "/proc/$P/stat" 2>/dev/null)"
        [ -n "${ut:-}" ] && CPU="$(echo "scale=2; ($ut + $st) / $HZ" | bc)"
        ELA="$(ps -o etimes= -p "$P" 2>/dev/null | tr -d ' ')"
    fi
    VB=-1; VA=-1; JN=-1
    if [ -f "$VLOG" ]; then
        VB="$(stat -c %s "$VLOG")"
        VA=$(( $(date +%s) - $(stat -c %Y "$VLOG") ))
        JN="$(grep -c 'joined the game' "$VLOG" 2>/dev/null || echo -1)"
    fi
    printf '%d\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        "$i" "$(date +%T)" "$TAG" "${P:--}" "$CPU" "${ELA:--1}" "$VB" "$VA" "$JN" \
        "$(cut -d' ' -f1 /proc/loadavg)" >> "$OUT"
    sleep 30
done
echo "BASELINE-DONE" >> "$OUT"
