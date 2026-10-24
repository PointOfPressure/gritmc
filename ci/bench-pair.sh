#!/usr/bin/env bash
# bench-pair.sh — a PAIRED comparison of two builds of `worldgen-dump full`,
# which is exactly the workload `ci/bench-servers.sh` times in its `rustmc`
# arm (`run_rustmc` runs the binary under /usr/bin/time; it boots no server).
#
#     ci/bench-pair.sh <name>:<remote-dir>:<sha> <name>:<remote-dir>:<sha> [reps] [radius]
#
# # Why not another `ci/bench-servers.sh` before/after
#
# That harness has 4.0% run-to-run variance on a single three-square run and
# has handed a change the wrong sign twice, so an unpaired before/after
# through it cannot resolve anything below ~8%. Two effects dominate and BOTH
# cancel under pairing:
#
#   * the spread BETWEEN the three centres is 7-17% at every radius, and it is
#     terrain rather than noise. Comparing arm X at centre c against arm Y at
#     centre c removes it exactly;
#   * slow drift in machine state. Running the arms adjacent in time and
#     flipping which one leads on alternate reps (ABBA) removes its linear
#     part.
#
# # INSTRUCTIONS RETIRED IS THE PRIMARY INSTRUMENT, NOT CPU-SECONDS
#
# This box cannot be relied on to be quiet — merge gates, corpus runs and
# other agents' builds are near-continuous, and a bench that requires an idle
# machine simply never runs. CPU-seconds under contention are inflated by
# cache and memory-bandwidth pressure and by SMT sharing, so they measure the
# neighbours as much as the code.
#
# Instructions retired does not. For a fixed deterministic workload it is a
# property of the work performed rather than of the time taken, so it is
# essentially immune to what else is on the box — which makes it the right
# instrument for the question "did this code start doing more work", which is
# what an attribution asks. It is measured at **RUSTMC_WORKERS=1** so that the
# work-stealing pool cannot make the count itself nondeterministic (a spinning
# worker retires instructions while achieving nothing, and how much it spins
# depends on the neighbours — exactly the contamination we are removing).
#
# CPU-seconds is still recorded, at whatever worker count is asked for,
# because CPU-s/chunk is the quantity `docs/BENCH.md` speaks in. It is the
# secondary. Per-run contention is recorded beside every row so a contaminated
# pair is identifiable afterwards rather than silently averaged in.
#
# # What is measured
#
# `/usr/bin/time` %U+%S of the whole process — user plus system, so the Anvil
# region write is charged, exactly as the harness's rustmc row charges it. The
# denominator is the square ASKED FOR, (2r+1)^2, on both arms; the generator
# also pays for a DECORATION_MARGIN ring it is not credited with, identically
# on both arms, so the ring cancels in the ratio and does not in the absolute.
#
# # Stamps this script refuses to leave off
#
# commit, radius, worker count, seed, centres, dimension and box, in the
# header of every output file. The whole point of the item this was written
# for is that the published figures carried a date and a machine name and
# nothing else, so nobody could tell whether they were comparable.
set -uo pipefail

usage() { sed -n '2,7p' "$0"; exit 2; }
[ $# -ge 2 ] || usage

SPEC_X="$1"; SPEC_Y="$2"; REPS="${3:-6}"; R="${4:-7}"
SEED="${BENCH_SEED:-20260722}"
CENTRES="${BENCH_CENTRES:-64,64 256,-192 -320,128}"
# Pinned, not inherited. The published figures did not record this and nothing
# in the tree could reconstruct it; a shell that exported it would have moved
# every number and left no trace.
export RUSTMC_WORKERS="${RUSTMC_WORKERS:-12}"

# perf is optional: without it the CPU-second columns still work, and the
# script says plainly that the contention-immune column is absent rather than
# reporting a blank as a zero.
PERF=""
if perf stat -e instructions -x, /bin/true >/dev/null 2>&1; then
    PERF=1
else
    echo "bench-pair: WARNING — perf unavailable (perf_event_paranoid=$(cat /proc/sys/kernel/perf_event_paranoid 2>/dev/null));"
    echo "bench-pair:           no instruction counts, CPU-seconds only."
fi

name_of() { echo "${1%%:*}"; }
dir_of()  { local t="${1#*:}"; echo "${t%%:*}"; }
sha_of()  { echo "${1##*:}"; }

XN=$(name_of "$SPEC_X"); XD=$(dir_of "$SPEC_X"); XS=$(sha_of "$SPEC_X")
YN=$(name_of "$SPEC_Y"); YD=$(dir_of "$SPEC_Y"); YS=$(sha_of "$SPEC_Y")
XB="$HOME/$XD/target/release/worldgen-dump"
YB="$HOME/$YD/target/release/worldgen-dump"
for b in "$XB" "$YB"; do
    [ -x "$b" ] || { echo "FATAL: no binary at $b"; exit 2; }
done

CHUNKS=$(( (2 * R + 1) * (2 * R + 1) ))
GENERATED=$(( (2 * R + 5) * (2 * R + 5) ))
WORK="${BENCH_PAIR_WORK:-$HOME/bench-pair-work}"
STAMP="$(date +%Y%m%dT%H%M%S)"
LABEL="${BENCH_PAIR_LABEL:-$XN-vs-$YN}"
OUT="${BENCH_PAIR_OUT:-/home/sayer/grit/logs/bench-pair-$LABEL-r$R-w$RUSTMC_WORKERS-$STAMP.tsv}"
mkdir -p "$WORK" "$(dirname "$OUT")"

# Anchored so the grep does not match the shell carrying the pattern.
contention() {
    local n
    n=$(pgrep -c -f '[j]ava|[c]argo|[r]ustc|[o]racle-runner' 2>/dev/null)
    case "$n" in *[!0-9]*|'') echo -1 ;; *) echo "$n" ;; esac
}

{
    echo "# bench-pair $XN($XS) vs $YN($YS)"
    echo "# radius=$R  chunks_delivered=$CHUNKS  chunks_generated=$GENERATED"
    echo "# seed=$SEED  centres='$CENTRES'  dimension=overworld"
    echo "# RUSTMC_WORKERS=$RUSTMC_WORKERS  reps=$REPS  perf=${PERF:-0}"
    echo "# box=$(hostname) $(nproc)t  kernel=$(uname -r)"
    echo "# started=$(date -Is)"
    echo "# design=ABBA paired by centre; statistic is the per-pair ratio"
    printf 'rep\tcentre\tarm\tsha\tcpu_s\twall_s\tinstructions\tcycles\tcontend\n'
} > "$OUT"

run_one() {                     # run_one <bin> <name> <sha> <cx> <cz> <rep>
    local bin="$1" arm="$2" sha="$3" cx="$4" cz="$5" rep="$6"
    local out="$WORK/w-$arm" tf="$WORK/t-$arm" lg="$WORK/l-$arm" pf="$WORK/p-$arm"
    local cb t0 t1 cpu wall ins cyc rc
    cb=$(contention)
    rm -rf "$out"; mkdir -p "$out"
    : > "$pf"
    t0=$(date +%s.%N)
    if [ -n "$PERF" ]; then
        perf stat -x, -e instructions,cycles -o "$pf" -- \
            /usr/bin/time -f '%U %S %M' -o "$tf" \
            "$bin" full "$SEED" "$R" "$out" "$cx" "$cz" > "$lg" 2>&1
        rc=$?
    else
        /usr/bin/time -f '%U %S %M' -o "$tf" \
            "$bin" full "$SEED" "$R" "$out" "$cx" "$cz" > "$lg" 2>&1
        rc=$?
    fi
    t1=$(date +%s.%N)
    if [ $rc -ne 0 ]; then
        echo "FAIL: $arm at $cx,$cz rc=$rc — see $lg" >&2
        tail -3 "$lg" >&2
        return 1
    fi
    cpu=$(awk '{printf "%.2f", $1 + $2}' "$tf")
    wall=$(awk -v a="$t0" -v b="$t1" 'BEGIN{printf "%.2f", b - a}')
    ins=$(awk -F, '$3=="instructions"{print $1}' "$pf" 2>/dev/null)
    cyc=$(awk -F, '$3=="cycles"{print $1}' "$pf" 2>/dev/null)
    printf '%s\t%s,%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        "$rep" "$cx" "$cz" "$arm" "$sha" "$cpu" "$wall" \
        "${ins:--1}" "${cyc:--1}" "$cb" >> "$OUT"
    echo "  rep$rep $cx,$cz $arm cpu=$cpu wall=$wall ins=${ins:--} contend=$cb"
}

echo "=== warmup pass (discarded): pages the binaries and data in ==="
first="${CENTRES%% *}"
# Checked, not silenced. A warmup redirected to /dev/null hides a broken arm
# until rep 1, and a bench that cannot run is not a bench that measured zero.
run_one "$XB" "$XN" "$XS" "${first%%,*}" "${first##*,}" 0 || exit 1
run_one "$YB" "$YN" "$YS" "${first%%,*}" "${first##*,}" 0 || exit 1
# Drop the warmup rows. Anchored on the rep field at line start: a bare \t0\t
# would also match a legitimate zero in any later column.
sed -i '/^0\t/d' "$OUT"

for rep in $(seq 1 "$REPS"); do
    echo "=== rep $rep/$REPS ==="
    for centre in $CENTRES; do
        cx="${centre%%,*}"; cz="${centre##*,}"
        if [ $(( rep % 2 )) -eq 1 ]; then
            run_one "$XB" "$XN" "$XS" "$cx" "$cz" "$rep" || exit 1
            run_one "$YB" "$YN" "$YS" "$cx" "$cz" "$rep" || exit 1
        else
            run_one "$YB" "$YN" "$YS" "$cx" "$cz" "$rep" || exit 1
            run_one "$XB" "$XN" "$XS" "$cx" "$cz" "$rep" || exit 1
        fi
    done
done

echo "# finished=$(date -Is)" >> "$OUT"
echo
echo "=== paired summary ==="
python3 "$(dirname "$0")/bench-pair-summary.py" "$OUT" "$XN" "$YN" "$CHUNKS"
echo
echo "data: $OUT"
