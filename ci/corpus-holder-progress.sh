#!/usr/bin/env bash
# corpus-holder-progress.sh — the production run's SCENARIO rate, from its own
# progress counter, while something else is loading the box.
#
#     ./ci/corpus-holder-progress.sh <log> <seconds> <interval> <out.tsv> [label]
#
# # WHY THIS AND NOT `ci/corpus-holder-rate.sh`
#
# The delay share that file reads is a sensitive harm proxy and it is not harm.
# Measured on 2026-08-18 while this branch loaded the box from 16 to 66: the
# production holder's runqueue delay share went 0.53 -> 0.81, and **its CPU
# time per wall second did not move at all** (148-197 ms per 60 s across the
# whole afternoon, 173 ms at the peak). A corpus recording is asleep in
# `settle` waits for most of its life, so it can absorb a great deal of
# runqueue delay without losing any wall clock, which is the same fact §15
# reports from the other side when it finds a recording close to load-invariant.
#
# So the delay share can fire when nothing was actually lost. The quantity that
# decides whether somebody's merge was harmed is **scenarios per minute**, and
# the run prints it: `ci/t2-gameplay.sh` writes `recorded N/M` per scenario to
# its own stdout, which the handoff captures.
#
# Counting lines in somebody else's log file is a read. It takes no lock, opens
# nothing for writing, and cannot perturb the run it measures.
#
# # IT SAYS WHETHER IT FINISHED
#
# A sampler that stopped early and a run that stalled look identical in the
# output. So the row count is asserted against the requested one and the last
# line is a sentinel, for the same reason the vanilla draw oracle spent nine
# days reporting 3 chunks where 15 were real.
set -uo pipefail
LOG="${1:?usage: $0 <log> <seconds> <interval> <out.tsv> [label]}"
SECS="${2:?seconds}"
IVL="${3:?interval}"
OUT="${4:?out.tsv}"
LABEL="${5:-unlabelled}"

[ -r "$LOG" ] || { echo "corpus-holder-progress: cannot read $LOG" >&2; exit 3; }
count() { local n; n=$(grep -c '  recorded ' "$LOG" 2>/dev/null); echo "${n:-0}"; }

[ -s "$OUT" ] || printf 'label\ttime\tepoch\trecorded\tdelta\tper_min\tload1\tworkers\n' > "$OUT"
P="$(count)"; T="$(date +%s)"
END=$(( T + SECS )); ROWS=0; WANT=$(( SECS / IVL ))
while [ "$(date +%s)" -lt "$END" ]; do
    sleep "$IVL"
    C="$(count)"; N="$(date +%s)"
    D=$(( C - P )); DT=$(( N - T ))
    PM="$(awk -v d="$D" -v t="$DT" 'BEGIN{ printf "%.2f", (t>0)? d*60.0/t : 0 }')"
    # **Computed into a variable, not inlined.** `$(( $(pgrep -fc ...) / 2 ))`
    # is a trap: `pgrep -c` prints `0` AND exits 1 when it matches nothing, so
    # `|| echo 0` appends a SECOND zero and the arithmetic expansion gets
    # "0\n0", which is a syntax error that killed this loop before its first
    # row. The failure looked exactly like a sampler that had nothing to sample.
    WK="$(pgrep -fc 'rustmc-corpus-[l]oad-worker' 2>/dev/null | head -1)"
    case "$WK" in ''|*[!0-9]*) WK=0 ;; esac
    WK=$(( WK / 2 ))
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$LABEL" "$(date +%T)" "$N" "$C" "$D" "$PM" \
        "$(cut -d' ' -f1 /proc/loadavg)" "$WK" >> "$OUT"
    P=$C; T=$N; ROWS=$((ROWS + 1))
done
# **The count is of rows that LANDED, not of loop iterations.** The first
# version counted iterations, so a run whose every write failed — an unwritable
# output file, a full disk — completed its loop and stamped itself
# `PROGRESS-COMPLETE 2/2` over an empty file. Its own falsifier caught it on the
# first run, which is the entire argument for writing one: the instrument that
# exists to stop a short file reading as a quiet box had that exact defect.
# `|| echo 0` would be a bug here for the same reason it was one twenty lines
# up: `grep -c` prints `0` AND exits 1 when it matches nothing, so the fallback
# appends a SECOND zero, the comparison below gets "0\n0", and the integer test
# errors out and falls through to the success branch. That is how a run over an
# unwritable file reported itself complete. `head -1` and a digit check instead.
LANDED="$(grep -c -v -e '^label' -e '^PROGRESS-' "$OUT" 2>/dev/null | head -1)"
case "$LANDED" in ''|*[!0-9]*) LANDED=0 ;; esac
if [ "$LANDED" -lt "$WANT" ]; then
    echo "PROGRESS-INCOMPLETE $LANDED of $WANT samples landed ($ROWS loop iterations ran)" >&2
    printf 'PROGRESS-INCOMPLETE\t%s\t%s\t%s\n' "$(date +%T)" "$LANDED" "$WANT" >> "$OUT"
    exit 8
fi
printf 'PROGRESS-COMPLETE\t%s\t%s\t%s\n' "$(date +%T)" "$LANDED" "$WANT" >> "$OUT"
echo "PROGRESS-COMPLETE $LANDED/$WANT samples"
