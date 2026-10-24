#!/usr/bin/env bash
# corpus-exclusion-integrity.sh — did the concurrent recordings disagree with
# each other MORE than the serialised ones did?
#
#     ./ci/corpus-exclusion-integrity.sh <experiment-outdir> <checks-dir>
#
# # WHY RATE IS NOT ENOUGH
#
# `ci/corpus-exclusion-measure.py` answers "were the concurrent runs slower".
# The 2026-08-06 incident was two claims, not one: throughput collapsed AND the
# recordings were garbage. A rig that only measured rate would report a
# comfortable null while saying nothing at all about the half that matters for
# correctness, and somebody would quote it as if it had.
#
# # THE FLOOR IS THE YARDSTICK, AND IT ALREADY EXISTS
#
# Two independent vanilla recordings of the same corpus **do not agree**, for
# reasons that have nothing to do with concurrency: vanilla's harvest is
# non-deterministic in its own right, and `-Dmax.bg.threads=1` does not clamp
# it — that was refuted on 2026-08-18, where the clamped spread came out wider
# (34/438/460 against 75/134/151) because `Util.makeExecutor` passes the value
# as `ForkJoinPool` parallelism rather than as a thread cap.
#
# So "the two recordings differ" is the NULL HYPOTHESIS here. The question is
# whether they differ *more* when concurrent, and the serial arm is what
# measures how much they differ anyway. `ci/corpus-floor.sh` is the existing
# instrument for exactly that quantity, so this file assembles its input per
# arm rather than inventing a second notion of disagreement.
#
# # A REFUSAL IS A RESULT, NOT A FAILURE
#
# `ci/corpus-floor.sh` refuses fewer than two recordings and refuses
# byte-identical ones. Both refusals are reported here verbatim and neither is
# swallowed: byte-identical concurrent recordings would be a strong result in
# its own right, and hiding the refusal behind a `|| true` would turn this into
# the kind of check that cannot go red.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

OUT="${1:?usage: $0 <experiment-outdir> <checks-dir>}"
CHECKS="${2:?checks dir — the subset corpus the runs actually recorded}"

[ -f "$OUT/pairs.tsv" ] || { echo "no pairs.tsv in $OUT" >&2; exit 2; }
[ -d "$CHECKS" ]        || { echo "no checks dir at $CHECKS" >&2; exit 2; }

W="$(mktemp -d "${TMPDIR:-/tmp}/excl-integrity.XXXXXX")"
trap 'rm -rf "$W"' EXIT

echo "== integrity: how much did each arm's recordings disagree with each other"
echo "   checks: $CHECKS"
echo

# The arms are whatever the rig ran, not a fixed pair. With three trees they
# are `serial`, `conc2` and `conc3`; hardcoding `serial conc` silently found
# nothing at all and reported "0 usable recordings" for a complete experiment.
ARMS="$(cut -f2 "$OUT/pairs.tsv" | sort -u | tr '\n' ' ')"
echo "   arms present: $ARMS"
echo

for ARM in $ARMS; do
    # Only groups every one of whose runs exited zero. A refusing run recorded
    # nothing and its vanilla.json is either absent or a partial file; feeding
    # it to the floor would measure the refusal.
    #
    # The row is variable width — one rc column per slot — so the exit codes
    # are read as a list rather than as a fixed RA/RB pair. Assuming two put
    # `0\t0` into RB and discarded every three-slot group.
    FILES=()
    while IFS=$'\t' read -r PT A RCS_REST; do
        [ "$A" = "$ARM" ] || continue
        BAD=0
        for RC in $RCS_REST; do [ "$RC" = "0" ] || BAD=1; done
        [ "$BAD" -eq 0 ] || continue
        for F in "$OUT/$PT"-*.vanilla.json; do
            [ -s "$F" ] && FILES+=("$F")
        done
    done < "$OUT/pairs.tsv"

    echo "-- arm $ARM: ${#FILES[@]} usable vanilla recordings"
    if [ "${#FILES[@]}" -lt 2 ]; then
        echo "   fewer than two — nothing to compare, and this is reported rather than skipped"
        echo
        continue
    fi

    # How many are byte-distinct. If every recording is identical the floor
    # will refuse, and that refusal is the interesting reading rather than an
    # error to route around.
    DISTINCT="$(md5sum "${FILES[@]}" | awk '{print $1}' | sort -u | wc -l)"
    echo "   ${DISTINCT} byte-distinct of ${#FILES[@]}"

    "$ROOT/ci/corpus-floor.sh" --checks "$CHECKS" --id "excl-$ARM" \
        --out "$W/$ARM" "${FILES[@]}" 2>&1 | sed 's/^/   /'
    echo "   corpus-floor exit: ${PIPESTATUS[0]}"
    echo
done

echo "== how to read this"
echo "   A concurrent arm whose disagreement sits inside the serial arm's is a"
echo "   pair of runs that did not interfere. A concurrent arm that disagrees"
echo "   MORE is interference, and is the result that would keep the lock."
