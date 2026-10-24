#!/usr/bin/env bash
# corpus-load-band-experiment.sh — §15's rate experiment, re-run inside the
# 50-110 load band it could not reach.
#
#     ./ci/corpus-load-band-experiment.sh <outdir> <tree> <tree> [tree...]
#
# Read `docs/CORPUS-LOCK-HOLD.md` §17.5 first. It is the pre-registration, and
# it commits to the design, the abort criterion and the predicted ceiling before
# any of this ran.
#
# # WHAT IT DOES, IN ORDER
#
#   1  CALIBRATE. `ci/corpus-load-band.sh --calibrate` alternates the load on
#      and off in equal windows and pairs each ON window with the OFF window
#      beside it. Two things come out: how many workers this box needs to read a
#      given load, and **what the load does to the production run that is
#      holding the corpus lock**. §15 argued that harm; this measures it. If the
#      paired difference in the holder's runqueue delay share is not small, the
#      experiment stops here and that reading is the result.
#
#   2  THE ARMS, under sustained load. `ci/corpus-exclusion-experiment.sh`
#      unchanged — the same rig, the same eight checks, the same alternation,
#      the same reader — wrapped in `taskset` and `nice 19` so it experiences
#      the same contention the load does. §15's arms ran at load 0.1 to 18.3;
#      these run in the band.
#
#   3  READ. `ci/corpus-exclusion-measure.py` on the result, which is the same
#      reader with the same overlap-discard rule, so the number is comparable to
#      §15's by construction rather than by argument.
#
# # WHY THE ARMS ARE NICED TOO, WHICH IS THE ONE THING TO CHECK BEFORE TRUSTING
# # THIS
#
# If the load were `nice 19` and the arms were `nice 0`, the arms would preempt
# the load and feel almost nothing: the experiment would report a comfortable
# null about a box that was never really loaded from the recorder's point of
# view. Both at `nice 19` is what makes the contention real, because CFS
# positions are relative. Both at `nice 0` would be real too, and would take the
# production recording down with it.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
case ":$PATH:" in *":$HOME/.cargo/bin:"*) ;; *) PATH="$HOME/.cargo/bin:$PATH"; export PATH ;; esac

OUT="${1:?usage: $0 <outdir> <tree> <tree> [tree...]}"
shift
TREES=("$@")
[ "${#TREES[@]}" -ge 2 ] || { echo "need at least two trees" >&2; exit 2; }

NCPU="$(nproc)"
CPUS="${LOADBAND_CPUS:-0-$(( NCPU / 2 - 1 ))}"
export LOADBAND_CPUS
# Two levels. The lower one puts the box in the bottom of the band; the higher
# one reproduces the runqueue depth that box load 110 over 24 CPUs means, which
# is 4.58 per CPU, and reads as a box load near 110 as well because loadavg
# counts every runnable task whatever it is pinned to.
W_LOW="${LOADBAND_W_LOW:-40}"
W_HIGH="${LOADBAND_W_HIGH:-95}"
REPS="${LOADBAND_REPS:-3}"
HOLD_S="${LOADBAND_HOLD_S:-5400}"

mkdir -p "$OUT"

# `LOADBAND_SKIP_CALIB=<file>` reuses a calibration already taken rather than
# loading the box twice to learn the same thing. The file must be a completed
# `--calibrate` output: the sentinel is checked, so a truncated calibration
# cannot be smuggled in as a reason to proceed.
if [ -n "${LOADBAND_SKIP_CALIB:-}" ]; then
    if ! grep -q 'CALIBRATION-COMPLETE' "$LOADBAND_SKIP_CALIB" 2>/dev/null; then
        echo "REFUSING: $LOADBAND_SKIP_CALIB carries no CALIBRATION-COMPLETE sentinel." >&2
        echo "  A calibration that did not finish is not a calibration." >&2
        exit 9
    fi
    cp -f "$LOADBAND_SKIP_CALIB" "$OUT/calib-high.tsv"
    echo "== 1. calibration reused from $LOADBAND_SKIP_CALIB"
    "$ROOT/ci/corpus-load-band.sh" --report "$OUT/calib-high.tsv" | tee "$OUT/calib-high.log"
else
echo "== 1. calibrate at $W_LOW workers  $(date +%T)"
"$ROOT/ci/corpus-load-band.sh" --calibrate "$W_LOW" 2 "$OUT/calib-low.tsv" 2>&1 | tee "$OUT/calib-low.log"
echo
echo "== 1b. calibrate at $W_HIGH workers  $(date +%T)"
"$ROOT/ci/corpus-load-band.sh" --calibrate "$W_HIGH" 2 "$OUT/calib-high.tsv" 2>&1 | tee "$OUT/calib-high.log"
fi
echo

# The abort criterion is inside `--hold`; this is the same question asked of the
# calibration, so a harmful load is refused before the arms are started rather
# than after ninety minutes of them.
HARM="$("$ROOT/ci/corpus-load-band.sh" --report "$OUT/calib-high.tsv" \
        | sed -n 's/.*ON minus OFF: \([+-][0-9.]*\)/\1/p')"
if [ -n "$HARM" ]; then
    if [ -n "${LOADBAND_ACCEPT_HARM:-}" ]; then
        echo "  holder delay share ON minus OFF: $HARM"
        echo "  LOADBAND_ACCEPT_HARM is set: proceeding with a PRICED cost rather than a"
        echo "  threshold. See docs/CORPUS-LOCK-HOLD.md §22.5 — the delay-share line was set"
        echo "  at +0.20 before anything was known about a quantity that swings 0.24-0.94 on"
        echo "  this box unaided, so it cannot separate this load from the box's own weather."
        echo "  The cost that IS measured is ~14% of the block's duration, added to one hold."
    elif awk -v h="$HARM" 'BEGIN{exit !(h > 0.20)}'; then
        echo "REFUSING to run the arms: at $W_HIGH workers the production holder's"
        echo "runqueue delay share rose by $HARM, which is harm rather than headroom."
        echo "That reading IS the result for §17.5, and it is the one §15 predicted."
        exit 7
    fi
    echo "  holder delay share, ON minus OFF at $W_HIGH workers: $HARM (under the 0.20 abort line)"
fi

echo
echo "== 2. the arms, under $W_HIGH workers of load  $(date +%T)"
LOAD_DEADLINE=$(( $(date +%s) + HOLD_S ))
"$ROOT/ci/corpus-load-band.sh" --hold "$W_HIGH" "$HOLD_S" "$OUT/hold.tsv" \
    > "$OUT/hold.log" 2>&1 &
LOADPID=$!
# Let the one-minute average climb into the band before the first run starts, or
# the first rep is a measurement of the ramp.
sleep 90
read -r L1 _ < /proc/loadavg
echo "  load at arm start: $L1"
taskset -c "$CPUS" nice -n 19 \
    "$ROOT/ci/corpus-exclusion-experiment.sh" "$REPS" "$OUT/arms" "${TREES[@]}" \
    2>&1 | tee "$OUT/arms.log"
ARMS_RC=${PIPESTATUS[0]}
ARMS_END=$(date +%s)
kill "$LOADPID" 2>/dev/null
"$ROOT/ci/corpus-load-band.sh" --stop >/dev/null 2>&1
echo
echo "== 3. what the block actually reached, and what it cost the holder"
# `--hold` writes phase `load` rows, not the `on`/`off` pairs `--report`
# summarises, so it gets its own read here rather than a summary that silently
# matches nothing.
awk -F'\t' 'NR>=1 && $2=="load" {
    n++; s+=$3; if($3>m) m=$3
    if ($5 != "-") { d+=$5; dn++; if ($5>dm) dm=$5 }
} END {
    if (n) printf "  box load1 over %d samples: mean %.1f, max %.1f\n", n, s/n, m
    if (dn) printf "  holder delay share over %d samples: mean %.3f, max %.3f\n", dn, d/dn, dm
    else    printf "  holder delay share: no samples (nobody held the lock during the block)\n"
}' "$OUT/hold.tsv" 2>/dev/null || true
# **An aborted load means the arms ran on an UNLOADED box for part of the
# block, and a ratio computed over that is not a load-band ratio.** Say so
# loudly rather than letting the number be quoted.
if grep -q "ABORTING the load" "$OUT/hold.log" 2>/dev/null; then
    echo
    echo "  !! THE LOAD WAS ABORTED MID-BLOCK because the production holder was"
    echo "     being harmed. Everything after that point in $OUT/arms ran at"
    echo "     whatever load the box had, NOT in the band. The abort is itself a"
    echo "     §17.5 result and is the outcome §15 predicted; the arms are not."
    grep -n "holder-delay-share\|ABORTING" "$OUT/hold.log" | tail -5 | sed 's/^/     /'
fi
# **The arms must have finished INSIDE the load budget.** If they ran past it
# the load stopped underneath them and the last reps were measured on whatever
# the box happened to be doing, which is not a band measurement and is
# indistinguishable from one in the output file. Same failure shape as a
# truncated oracle reporting a small answer.
if [ "$ARMS_END" -gt "$LOAD_DEADLINE" ]; then
    echo
    echo "  !! THE ARMS OUTLASTED THE LOAD by $(( ARMS_END - LOAD_DEADLINE ))s."
    echo "     Everything after $(date -d "@$LOAD_DEADLINE" +%T) ran on an unloaded box."
    echo "     Raise LOADBAND_HOLD_S or lower LOADBAND_REPS and re-run; do not quote this ratio."
    OVERRAN=1
else
    echo "  the arms finished $(( LOAD_DEADLINE - ARMS_END ))s inside the load budget"
    OVERRAN=0
fi
if ! grep -q 'LOAD-BLOCK-COMPLETE' "$OUT/hold.log" 2>/dev/null; then
    echo "  !! the load block did not report LOAD-BLOCK-COMPLETE — it stopped early"
    OVERRAN=1
fi
grep -c 'EXPERIMENT-DONE' "$OUT/arms.log" >/dev/null 2>&1 || true
if ! grep -q 'EXPERIMENT-DONE' "$OUT/arms.log" 2>/dev/null; then
    echo "  !! the arms did not print EXPERIMENT-DONE — the rig was truncated, not merely short"
    OVERRAN=1
fi
echo "  arms exit: $ARMS_RC"
[ "${OVERRAN:-0}" -eq 0 ] && echo "LOAD-BAND-EXPERIMENT-COMPLETE" || echo "LOAD-BAND-EXPERIMENT-INCOMPLETE"
