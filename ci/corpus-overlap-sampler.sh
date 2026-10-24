#!/usr/bin/env bash
# corpus-overlap-sampler.sh — how many corpus recordings were actually live at
# once, sampled from the process table rather than assumed from the rig.
#
#     ./ci/corpus-overlap-sampler.sh <seconds> <interval> <out.tsv> <tree> [tree...]
#
# # WHY THIS IS NOT REDUNDANT WITH THE RIG THAT STARTED THEM
#
# `ci/corpus-integrity-108.sh` starts three recordings together and waits for
# all three, so "they overlapped" is a property of the code rather than of the
# run. If one tree's vanilla boot fails and retries, or one recording finishes
# eight minutes before the others, part of the concurrent arm was recorded with
# two live and not three, and the rig cannot tell. `ci/corpus-exclusion-
# measure.py` already refuses to count a concurrent pair that did not overlap;
# this is the same discipline for an arm whose runs are twenty minutes long.
#
# One `pgrep` and one `tr` per tree per sample. It writes nothing into any tree.
set -uo pipefail

live_in() {   # live_in <tree> -> 1 if a recorder in that tree is running
    local t="$1"
    local p
    for p in $(pgrep -f 'oracle-runner record' 2>/dev/null); do
        # **Match the tree path WITH ITS TRAILING SLASH.** A recorder's command
        # line carries `<tree>/target/release/oracle-runner` and
        # `<tree>/ci/checks/gameplay`, so the slash is always there, and without
        # it a tree whose path is a prefix of another's reads as live whenever
        # the longer one is recording. `~/rustmc-lockwhy-a` against
        # `~/rustmc-lockwhy-a-something` is the shape; this branch's own three
        # trees do not collide, so nothing measured here was affected, and the
        # next lane to add a fourth tree would not have been so lucky.
        # Caught by ci/corpus-overlap-sampler-falsify.sh case 3.
        if tr '\0' ' ' < "/proc/$p/cmdline" 2>/dev/null | grep -cF -- "$t/" >/dev/null; then echo 1; return; fi
    done
    echo 0
}

# The one-shot mode is parsed BEFORE the mandatory arguments. Asking this script
# a single question should not require inventing an output file for it, and the
# first version put the check after `OUT="${3:?out.tsv}"`, so `--live-in <tree>`
# died on the missing third argument.
if [ "${1:-}" = "--live-in" ]; then live_in "${2:?tree}"; exit 0; fi

SECS="${1:?usage: $0 <seconds> <interval> <out.tsv> <tree> [tree...]}"
IVL="${2:?interval}"
OUT="${3:?out.tsv}"
shift 3
TREES=("$@")
[ "${#TREES[@]}" -ge 1 ] || { echo "need at least one tree" >&2; exit 2; }

{ printf 'time\tlive\tload1'; for t in "${TREES[@]}"; do printf '\t%s' "$(basename "$t")"; done; printf '\n'; } > "$OUT"
END=$(( $(date +%s) + SECS ))
while [ "$(date +%s)" -lt "$END" ]; do
    ROW=""; N=0
    for t in "${TREES[@]}"; do
        L="$(live_in "$t")"; N=$((N + L)); ROW="$ROW	$L"
    done
    printf '%s\t%s\t%s%s\n' "$(date +%T)" "$N" "$(cut -d' ' -f1 /proc/loadavg)" "$ROW" >> "$OUT"
    sleep "$IVL"
done
