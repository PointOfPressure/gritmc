#!/usr/bin/env bash
# corpus-holder-rate.sh — is the PRODUCTION lock holder being harmed right now?
#
#     ./ci/corpus-holder-rate.sh <seconds> <interval-s> <out.tsv> [label]
#     ./ci/corpus-holder-rate.sh --schedstat <pid>        sum a process's tasks
#     ./ci/corpus-holder-rate.sh --holder                 print the holder's tree
#
# # WHY THIS EXISTS
#
# `docs/CORPUS-LOCK-HOLD.md` §15 declined to force the 50-110 load band and
# gave the right reason: a live production recording held the lock, and loading
# the box would have inflicted §14's `settle`-cap contamination on somebody
# else's merge. That is an argument about a **risk**, and the branch had no way
# to turn it into a reading. This is the reading.
#
# It samples the run that currently holds `~/.rustmc-corpus-lock` and reports,
# per interval, how much CPU time its recorder got and **how long it spent
# waiting on a runqueue to get it**. That second number is the whole point:
#
#     /proc/<pid>/task/<tid>/schedstat
#         field 1   ns spent running on a CPU
#         field 2   ns spent waiting on a runqueue, runnable, not running
#         field 3   timeslices
#
# Field 2 is exactly what background load does to a process and nothing else
# does. A recording that is merely idle in a `settle` wait is not runnable, so
# it accumulates neither field: idleness and starvation are distinguishable
# here, which is precisely what the three instruments in §13 could not do.
#
# The quantity to read is the **delay share**, `delay / (cpu + delay)`. It is
# dimensionless, it does not care that a corpus recording is asleep most of the
# time, and it is comparable across intervals of different lengths.
#
# # IT PERTURBS NOTHING, AND THAT IS LOAD-BEARING
#
# Two `cat`s of a `/proc` file per sample. It takes no lock, writes nothing
# into the holder's tree, signals nothing, and never touches the queue. An
# instrument that has to disturb a run in order to ask whether the run is being
# disturbed would be worthless.
#
# # HOW IT IS USED, AND WHAT MAKES IT AN ABORT CRITERION RATHER THAN A DIARY
#
# `ci/corpus-load-band.sh` runs this before, during and after every load block
# and compares the delay share between the windows. The load block is aborted
# the moment the holder's delay share rises above its own pre-load baseline by
# more than the threshold that script declares. So the protection is measured
# on the run being protected, at the time it is being protected, rather than
# argued from the design of the load generator.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# `--tree DIR` is extracted before the mode dispatch so it composes with
# `--follow`, which re-invokes this script.
FORCED_TREE=""
if [ "${1:-}" = "--tree" ]; then FORCED_TREE="${2:?tree}"; shift 2; fi

# Sum a process's threads. A single `/proc/<pid>/schedstat` describes the main
# thread only, and the vanilla reference is a JVM with dozens.
schedstat_sum() {
    local pid="$1" cpu=0 delay=0 slices=0 c d s t
    for t in /proc/"$pid"/task/*/schedstat; do
        [ -r "$t" ] || continue
        read -r c d s < "$t" 2>/dev/null || continue
        cpu=$((cpu + c)); delay=$((delay + d)); slices=$((slices + s))
    done
    printf '%s %s %s\n' "$cpu" "$delay" "$slices"
}
if [ "${1:-}" = "--schedstat" ]; then schedstat_sum "${2:?pid}"; exit 0; fi

# The holder's tree, through the extractor `ci/corpus-healthy-baseline.sh`
# already falsifies (its case 3 is a status block with four waiters that must
# still yield the holder). Reimplementing the `record:.* since ` pattern here
# would be a second copy of the one thing in this area that has been got wrong.
holder_tree() {
    "$ROOT/ci/corpus-lock.sh" status 2>/dev/null \
        | "$ROOT/ci/corpus-healthy-baseline.sh" --holder-from-status
}
if [ "${1:-}" = "--holder" ]; then holder_tree; exit 0; fi

# `--follow <total-s> <interval> <out.tsv> [label]` — keep sampling across
# holder changes for the whole budget.
#
# The ordinary mode resolves the holder once and stops when that run's recorder
# exits, which is correct: attributing the next run's numbers to the previous
# one is exactly the confusion this file exists to avoid. But the question
# "what does this box's own load do to whoever is holding" is a question about
# hours, and over hours the holder changes several times. So the following is
# done by RE-RESOLVING, in a loop, rather than by relaxing the attribution.
#
# A gap between two holders, and the gap between a holder's two recordings,
# both come back as exit 3 and are waited out rather than filled in.
if [ "${1:-}" = "--follow" ]; then
    shift
    F_TOT="${1:?total seconds}"; F_IVL="${2:?interval}"; F_OUT="${3:?out.tsv}"; F_LAB="${4:-follow}"
    F_END=$(( $(date +%s) + F_TOT ))
    while [ "$(date +%s)" -lt "$F_END" ]; do
        F_LEFT=$(( F_END - $(date +%s) ))
        [ "$F_LEFT" -gt "$F_IVL" ] || break
        if [ -n "$FORCED_TREE" ]; then
            "$0" --tree "$FORCED_TREE" "$F_LEFT" "$F_IVL" "$F_OUT" "$F_LAB" || sleep 20
        else
            "$0" "$F_LEFT" "$F_IVL" "$F_OUT" "$F_LAB" || sleep 20
        fi
    done
    exit 0
fi

# `--tree <dir>` points the same instrument at a named tree instead of at
# whoever holds the lock.
#
# It exists for one argument that would otherwise be unfalsifiable. The load in
# `ci/corpus-load-band.sh` runs at `nice 19`, and so do the arms beside it, on
# the reasoning that CFS positions are relative and a `nice 19` recorder among
# `nice 19` competitors sits where a `nice 0` recorder among `nice 0` ones would.
# That is a claim about runqueue delay, and runqueue delay is what this file
# reads. Pointed at a scratch tree during a load block it says what the ARMS
# felt, which can be put beside what a production recording feels at the same
# box load with nothing niced. The argument becomes two numbers rather than a
# paragraph about the scheduler.
SECS="${1:?usage: $0 [--tree DIR] <seconds> <interval-s> <out.tsv> [label]}"
IVL="${2:?interval}"
OUT="${3:?out.tsv}"
LABEL="${4:-unlabelled}"

if [ -n "$FORCED_TREE" ]; then HOLDER="$FORCED_TREE"; else HOLDER="$(holder_tree)"; fi
if [ -z "$HOLDER" ] || [ ! -d "$HOLDER" ]; then
    echo "corpus-holder-rate: nobody holds the corpus lock" >&2
    exit 3
fi

# The recorder inside the holder's tree, not any recorder on the box. This rig
# runs its own recorders in scratch trees at the same time, so "any recorder"
# would sample the experiment instead of the run being protected from it.
holder_recorder() {
    local p
    for p in $(pgrep -f 'oracle-runner record' 2>/dev/null); do
        if tr '\0' ' ' < "/proc/$p/cmdline" 2>/dev/null | grep -cF "$HOLDER" >/dev/null; then
            echo "$p"; return 0
        fi
    done
    return 1
}

[ -s "$OUT" ] || printf 'label\ttime\tholder\tpid\tcpu_ns_d\tdelay_ns_d\tslices_d\tdelay_share\tload1\n' > "$OUT"

P="$(holder_recorder || true)"
if [ -z "$P" ]; then
    echo "corpus-holder-rate: $HOLDER has no 'oracle-runner record' running" >&2
    echo "                    (it is probably between the two recordings) — nothing to sample" >&2
    exit 3
fi
# **This file must say what it wrote, and it did not.** Its passive sampler lost
# its first half hour on 2026-08-18 and the loss was invisible: a file holding
# five rows where thirty-five were written reads exactly like a box that was
# quiet. A claim was built on the surviving fragment and had to be withdrawn.
# See docs/CORPUS-LOCK-HOLD.md §22.4.
read -r C0 D0 S0 <<< "$(schedstat_sum "$P")"
ROWS=0
WANT=$(( SECS / IVL ))
END=$(( $(date +%s) + SECS ))
while [ "$(date +%s)" -lt "$END" ]; do
    sleep "$IVL"
    # A holder that finished mid-window is not a contaminated holder, it is a
    # finished one. Stop rather than attributing the next run's numbers to it.
    if [ ! -r "/proc/$P/stat" ]; then
        echo "corpus-holder-rate: recorder $P exited — stopping" >&2
        break
    fi
    read -r C1 D1 S1 <<< "$(schedstat_sum "$P")"
    CD=$((C1 - C0)); DD=$((D1 - D0)); SD=$((S1 - S0))
    SHARE="$(awk -v c="$CD" -v d="$DD" 'BEGIN{ t=c+d; printf "%.4f", (t>0)? d/t : -1 }')"
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        "$LABEL" "$(date +%T)" "$HOLDER" "$P" "$CD" "$DD" "$SD" "$SHARE" \
        "$(cut -d' ' -f1 /proc/loadavg)" >> "$OUT"
    C0=$C1; D0=$D1; S0=$S1
    ROWS=$((ROWS + 1))
done
# A short run is legitimate — the recorder finishing is the normal way this ends
# — so the count is REPORTED rather than treated as a failure. What must not
# happen is that "ended because the run ended" and "ended because something ate
# this process" look the same afterwards.
printf 'SAMPLER-END\t%s\t%s rows of %s requested\t%s\n' "$(date +%T)" "$ROWS" "$WANT" "$LABEL" >> "$OUT"
