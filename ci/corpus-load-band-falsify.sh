#!/usr/bin/env bash
# corpus-load-band-falsify.sh — the load generator is the one instrument in this
# branch that can hurt somebody, so it is the one with the most cases.
#
#     ./ci/corpus-load-band-falsify.sh
#
# Every case runs against a COPY of `ci/corpus-load-band.sh` in a scratch root
# with a stubbed `ci/corpus-holder-rate.sh`, so the abort criterion can be
# driven to both verdicts without a production recording to endanger. The
# script derives its own `ROOT` from `dirname $0`, which is what makes that
# possible; it is not a hook added for testing.
#
# The cases that matter, and they are 3 and 6:
#
#   3  the budget is enforced INSIDE each worker. SIGKILL the controller and the
#      load must still stop on schedule. A generator whose stop condition lives
#      in a supervisor is how a box gets left at load 110 overnight;
#   6  the abort criterion must actually fire. A circuit breaker that has never
#      been seen to trip is a comment.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# The generator under test. The sweep at the bottom re-invokes THIS script with
# a patched copy, so every case above is falsifiable without any of them being
# restructured into a function.
DRIVER="${LOADBAND_DRIVER:-$ROOT/ci/corpus-load-band.sh}"
W="$(mktemp -d /tmp/loadband-falsify.XXXXXX)"
# `[b]racket` form, always: `pkill -f` matches the caller's own command line,
# and this file has already killed one of its own shells that way.
trap 'rm -rf "$W"; pkill -f "rustmc-corpus-[l]oad-worker" 2>/dev/null || true' EXIT

PASS=0; FAIL=0
ok()  { PASS=$((PASS+1)); printf '  PASS  %s\n' "$*"; }
bad() { FAIL=$((FAIL+1)); printf '  FAIL  %s\n' "$*"; }

# A scratch root: the script under test, plus a stub holder-rate it will call.
# <shares...> are handed out one per invocation, so a case can make the holder
# look calm and then harmed.
mkroot() {  # mkroot <name> <share> [share...]
    local n="$1"; shift
    local R="$W/$n"
    mkdir -p "$R/ci"
    cp "$DRIVER" "$R/ci/corpus-load-band.sh"
    printf '%s\n' "$@" > "$R/ci/shares"
    cat > "$R/ci/corpus-holder-rate.sh" <<'STUB'
#!/usr/bin/env bash
D="$(cd "$(dirname "$0")" && pwd)"
if [ "${1:-}" = "--holder" ]; then
    [ -f "$D/no-holder" ] && exit 0
    echo "/tmp/pretend-holder"; exit 0
fi
# <seconds> <interval> <out.tsv> [label] — emit one row with the next share.
N=$(cat "$D/served" 2>/dev/null || echo 0); N=$((N+1)); echo "$N" > "$D/served"
S="$(sed -n "${N}p" "$D/shares")"; [ -n "$S" ] || S="$(tail -1 "$D/shares")"
printf 'label\ttime\tholder\tpid\tcpu_ns_d\tdelay_ns_d\tslices_d\tdelay_share\tload1\n' > "$3"
printf 'x\t00:00:00\t/tmp/pretend-holder\t1\t1\t1\t1\t%s\t1.0\n' "$S" >> "$3"
sleep 1
STUB
    chmod +x "$R/ci/corpus-holder-rate.sh"
    echo "$R"
}

live_workers() { pgrep -f 'loadband-falsify-worker' 2>/dev/null | wc -l; }

# ---------------------------------------------------------------------------
say() { printf '%s\n' "$*"; }
say "== corpus-load-band-falsify"

# 1. The CPU-set arithmetic. A mask that is miscounted makes every runqueue
#    depth in the report wrong, and the report is the whole deliverable.
R="$(mkroot cpus 0.1)"
for spec in "0-11=12" "0-3,8-11=8" "5=1" "0-23=24" "2,4,6=3"; do
    m="${spec%=*}"; want="${spec#*=}"
    got="$(LOADBAND_CPUS="$m" "$R/ci/corpus-load-band.sh" --hold 0 0 2>&1 \
           | sed -n 's/.*(\([0-9]*\) of [0-9]*)/\1/p' | head -1)"
    if [ "$got" = "$want" ]; then ok "1 cpu mask '$m' counts $want"
    else bad "1 cpu mask '$m' counted '${got:-nothing}', wanted $want"; fi
done

# 2. Workers get the nice value and the affinity that were asked for. Both are
#    load-bearing: the nice is what protects production and the affinity is what
#    bounds the blast radius, and neither is visible in the output.
R="$(mkroot aff 0.1)"
LOADBAND_CPUS=0-1 LOADBAND_NICE=19 LOADBAND_BASELINE_S=1 LOADBAND_SAMPLE_S=1 \
    LOADBAND_PIDFILE="$W/aff.pids" "$R/ci/corpus-load-band.sh" --hold 2 6 >/dev/null 2>&1 &
CTL=$!
sleep 3
NI="$(ps -o ni= -p "$(head -1 "$W/aff.pids" 2>/dev/null)" 2>/dev/null | tr -d ' ')"
AF="$(taskset -pc "$(head -1 "$W/aff.pids" 2>/dev/null)" 2>/dev/null | sed 's/.*: //')"
wait "$CTL" 2>/dev/null || true
if [ "$NI" = 19 ]; then ok "2 workers run at nice 19"; else bad "2 worker nice was '${NI:-unknown}', wanted 19"; fi
if [ "$AF" = "0,1" ] || [ "$AF" = "0-1" ]; then ok "2 workers are pinned to the requested CPUs ($AF)"
else bad "2 worker affinity was '${AF:-unknown}', wanted 0-1"; fi

# 3. THE BUDGET IS INSIDE THE WORKER. Kill the controller; the load must still
#    stop by itself.
R="$(mkroot budget 0.1)"
LOADBAND_CPUS=0-1 LOADBAND_BASELINE_S=1 LOADBAND_SAMPLE_S=1 \
    LOADBAND_PIDFILE="$W/budget.pids" "$R/ci/corpus-load-band.sh" --hold 2 8 >/dev/null 2>&1 &
CTL=$!
sleep 3
NW_BEFORE="$(pgrep -c -P "$(head -1 "$W/budget.pids" 2>/dev/null)" 2>/dev/null)"; NW_BEFORE="${NW_BEFORE:-0}"
kill -9 "$CTL" 2>/dev/null
sleep 1
ALIVE_AFTER_KILL=0
while read -r p; do kill -0 "$p" 2>/dev/null && ALIVE_AFTER_KILL=$((ALIVE_AFTER_KILL+1)); done < "$W/budget.pids"
sleep 9
ALIVE_AFTER_BUDGET=0
while read -r p; do kill -0 "$p" 2>/dev/null && ALIVE_AFTER_BUDGET=$((ALIVE_AFTER_BUDGET+1)); done < "$W/budget.pids"
if [ "$ALIVE_AFTER_KILL" -ge 1 ]; then
    ok "3 the controller was SIGKILLed with $ALIVE_AFTER_KILL worker(s) still up (so the case tests something)"
else
    bad "3 no worker survived the controller's death, so the budget was never the thing under test"
fi
if [ "$ALIVE_AFTER_BUDGET" -eq 0 ]; then
    ok "3 every worker stopped at its own budget after the controller was killed"
else
    bad "3 $ALIVE_AFTER_BUDGET worker(s) outlived both the controller and the budget — THE BOX WOULD BE LEFT LOADED"
fi

# 4. --stop kills a running load.
R="$(mkroot stop 0.1)"
LOADBAND_CPUS=0-1 LOADBAND_BASELINE_S=1 LOADBAND_SAMPLE_S=1 \
    LOADBAND_PIDFILE="$W/stop.pids" "$R/ci/corpus-load-band.sh" --hold 2 60 >/dev/null 2>&1 &
CTL=$!
sleep 3
LOADBAND_PIDFILE="$W/stop.pids" "$R/ci/corpus-load-band.sh" --stop >/dev/null 2>&1
sleep 1
N=0; while read -r p; do kill -0 "$p" 2>/dev/null && N=$((N+1)); done < "$W/stop.pids" 2>/dev/null
kill -9 "$CTL" 2>/dev/null; wait "$CTL" 2>/dev/null || true
if [ "$N" -eq 0 ]; then ok "4 --stop killed the running load"; else bad "4 --stop left $N worker(s) running"; fi

# 5. THE GENERATOR MUST ACTUALLY GENERATE. A load band experiment whose load
#    does not move the runqueue would pass every other case here.
R="$(mkroot real 0.1)"
# **The assertion is that the workers are RUNNABLE, not that they burn CPU.**
# `nice 19` workers on a box whose other work is `nice 0` may get almost no CPU
# at all, and that is the design rather than a fault: what a load generator
# generates is runqueue depth. `/proc/loadavg`'s running count was the first
# thing tried here and it moved 35 -> 38 for eight workers, because it is one
# instantaneous sample of a box that was already carrying 35.
# Eight workers on the top four CPUs for four seconds, not a boxful: this file
# is meant to be runnable while something else is being measured, and a case
# that briefly loads all 24 CPUs would land in one arm of whatever experiment
# is in flight and not the other. Eight is plenty to tell a generator that
# generates from one that does not.
LOADBAND_CPUS="$(( $(nproc) - 4 ))-$(( $(nproc) - 1 ))" LOADBAND_BASELINE_S=1 LOADBAND_SAMPLE_S=1 \
    LOADBAND_PIDFILE="$W/real.pids" "$R/ci/corpus-load-band.sh" --hold 8 8 >/dev/null 2>&1 &
CTL=$!
sleep 4
# The spinners are the CHILDREN of the recorded pids: each recorded pid is a
# `timeout` supervisor, which sleeps. Look at what it forked.
RUNNABLE=0; TOTAL=0
while read -r p; do
    [ -n "$p" ] || continue
    for c in $(pgrep -P "$p" 2>/dev/null); do
        TOTAL=$((TOTAL + 1))
        st="$(awk '{print $3}' "/proc/$c/stat" 2>/dev/null)"
        [ "$st" = R ] && RUNNABLE=$((RUNNABLE + 1))
    done
done < "$W/real.pids"
wait "$CTL" 2>/dev/null || true
if [ "$TOTAL" -eq 8 ] && [ "$RUNNABLE" -eq 8 ]; then
    ok "5 all 8 workers were runnable at once, which is what a load is"
else
    bad "5 $RUNNABLE of $TOTAL workers were runnable — this generator does not generate"
fi

# 6. THE ABORT CRITERION FIRES. The stub reports a calm holder for the baseline
#    and a harmed one for both samples after it; the run must exit 7.
R="$(mkroot abort 0.10 0.90 0.90 0.90 0.90)"
LOADBAND_CPUS=0-1 LOADBAND_BASELINE_S=1 LOADBAND_SAMPLE_S=1 LOADBAND_ABORT_DELTA=0.20 \
    LOADBAND_PIDFILE="$W/abort.pids" "$R/ci/corpus-load-band.sh" --hold 2 30 >"$W/abort.log" 2>&1
RC=$?
if [ "$RC" -eq 7 ]; then ok "6 a holder whose delay share rose 0.10 -> 0.90 aborted the load (rc=7)"
else bad "6 the holder's delay share rose 0.10 -> 0.90 and the load ran on (rc=$RC)"; fi
N=0; while read -r p; do kill -0 "$p" 2>/dev/null && N=$((N+1)); done < "$W/abort.pids" 2>/dev/null
if [ "$N" -eq 0 ]; then ok "6 the abort actually stopped the workers"; else bad "6 abort exited 7 but left $N worker(s) running"; fi

# 7. AND IT MUST NOT FIRE ON NOISE. One breach is not two; a holder that dips
#    once must not abort a valid block.
R="$(mkroot noabort 0.10 0.90 0.11 0.12 0.10)"
LOADBAND_CPUS=0-1 LOADBAND_BASELINE_S=1 LOADBAND_SAMPLE_S=1 LOADBAND_ABORT_DELTA=0.20 \
    LOADBAND_PIDFILE="$W/noabort.pids" "$R/ci/corpus-load-band.sh" --hold 2 12 >"$W/noabort.log" 2>&1
RC=$?
if [ "$RC" -eq 0 ]; then ok "7 a single high sample did not abort the block"
else bad "7 one high sample out of four aborted the block (rc=$RC) — the breaker trips on noise"; fi

# 8. With nobody holding the corpus lock there is nothing to protect and the
#    run must proceed rather than refuse.
R="$(mkroot noholder 0.1)"; : > "$R/ci/no-holder"
LOADBAND_CPUS=0-1 LOADBAND_BASELINE_S=1 LOADBAND_SAMPLE_S=1 \
    LOADBAND_PIDFILE="$W/noholder.pids" "$R/ci/corpus-load-band.sh" --hold 2 4 >"$W/noholder.log" 2>&1
RC=$?
if [ "$RC" -eq 0 ] && grep -q "NOBODY holds the corpus lock" "$W/noholder.log"; then
    ok "8 with no production holder the block runs and says so"
else
    bad "8 with no production holder: rc=$RC, log said $(tail -1 "$W/noholder.log")"
fi

# ===========================================================================
# The injection sweep. Fifteen greens are worth nothing until each of them has
# been seen red, and this file guards the one instrument here that can leave the
# box loaded or slow somebody's merge.
# ===========================================================================
# 9. A RUN MUST BE ABLE TO SAY IT FINISHED. A truncated load block and a load
#    block that ran and found nothing are the same bytes otherwise. The vanilla
#    draw oracle spent nine days reporting 3 chunks where 15 were real for
#    exactly this reason, and every reader saw a plausible small number.
R="$(mkroot complete 0.1)"
LOADBAND_CPUS=0-1 LOADBAND_SETTLE_S=1 LOADBAND_WINDOW=6 \
    LOADBAND_PIDFILE="$W/cal.pids" "$R/ci/corpus-load-band.sh" \
    --calibrate 2 2 "$W/cal.tsv" > "$W/cal.log" 2>&1
RC=$?
if [ "$RC" -eq 0 ] && grep -q 'CALIBRATION-COMPLETE 4/4' "$W/cal.log"; then
    ok "9 a calibration that wrote every window says so (4/4)"
else
    bad "9 a complete calibration did not report itself complete (rc=$RC): $(tail -2 "$W/cal.log" | tr '\n' ' ')"
fi
grep -q 'CALIBRATION-COMPLETE' "$W/cal.tsv" \
    && ok "9 the sentinel is written into the data file, not only to stdout" \
    || bad "9 no sentinel in the data file, so a later reader cannot tell"

# 10. A calibration that was KILLED partway must leave no completion sentinel.
#     This is the protection that actually matters: every reader of the file
#     greps for `CALIBRATION-COMPLETE`, and a truncated run must not carry one.
#     (The exit-8 path is covered by the `incomplete-reports-as-complete`
#     injection below, which is the only way to reach it without inventing a
#     hook in the script under test purely to be tested.)
R="$(mkroot killed 0.1)"
LOADBAND_CPUS=0-1 LOADBAND_SETTLE_S=1 LOADBAND_WINDOW=8 \
    LOADBAND_PIDFILE="$W/k.pids" "$R/ci/corpus-load-band.sh" \
    --calibrate 2 4 "$W/killed.tsv" > "$W/killed.log" 2>&1 &
KPID=$!
sleep 6
kill -9 "$KPID" 2>/dev/null
LOADBAND_PIDFILE="$W/k.pids" "$R/ci/corpus-load-band.sh" --stop >/dev/null 2>&1
sleep 1
if grep -q 'CALIBRATION-COMPLETE' "$W/killed.tsv" 2>/dev/null; then
    bad "10 a calibration killed after $(awk 'NR>1' "$W/killed.tsv" 2>/dev/null | wc -l) of 8 windows still carries a completion sentinel"
else
    ok "10 a killed calibration leaves no completion sentinel for a reader to trust"
fi

# 12. THE EXIT-8 PATH, reached without killing anything.
#     Cases 9 and 10 both pass whether or not the completeness assertion
#     exists — 9 because a complete run reports complete either way, 10 because
#     a killed run never reaches the assertion at all. So the
#     `incomplete-reports-as-complete` injection came back MISSED: nothing
#     exercised the branch it removes. This reaches it by making the output
#     file unwritable, which is what a full disk or a bad mode looks like: the
#     windows run, none of their rows land, and the script must notice that
#     what it wrote is not what it was asked to write.
R="$(mkroot unwritable 0.1)"
printf 'cycle\tphase\tload1\tload5\tshare\ttime\n' > "$W/ro.tsv"
chmod 0444 "$W/ro.tsv"
LOADBAND_CPUS=0-1 LOADBAND_SETTLE_S=1 LOADBAND_WINDOW=6 \
    LOADBAND_PIDFILE="$W/ro.pids" "$R/ci/corpus-load-band.sh" \
    --calibrate 2 2 "$W/ro.tsv" > "$W/ro.log" 2>&1
RC=$?
chmod 0644 "$W/ro.tsv" 2>/dev/null
if [ "$RC" -eq 8 ] && grep -q 'CALIBRATION INCOMPLETE' "$W/ro.log"; then
    ok "12 a calibration that wrote 0 of 4 windows exits 8 and refuses to report"
else
    bad "12 a calibration that wrote $(awk 'NR>1' "$W/ro.tsv" | wc -l) of 4 windows exited $RC: $(tail -2 "$W/ro.log" | tr '\n' ' ')"
fi

# 11. --hold reports completion the same way.
R="$(mkroot held 0.1)"
LOADBAND_CPUS=0-1 LOADBAND_BASELINE_S=1 LOADBAND_SAMPLE_S=1 \
    LOADBAND_PIDFILE="$W/h.pids" "$R/ci/corpus-load-band.sh" --hold 2 5 > "$W/h.log" 2>&1
grep -q 'LOAD-BLOCK-COMPLETE' "$W/h.log" \
    && ok "11 a load block that reached its budget says LOAD-BLOCK-COMPLETE" \
    || bad "11 a completed load block did not say so: $(tail -1 "$W/h.log")"

MISSED=0
if [ -z "${LOADBAND_NO_SWEEP:-}" ]; then
    echo
    echo "== injections (each MUST turn this suite red)"
    inject() {
        local n="$1" e="$2"
        local out="$W/drv.$n.sh"
        cp "$DRIVER" "$out"
        perl -0pi -e "$e" "$out"
        if cmp -s "$DRIVER" "$out"; then echo STALE; return; fi
        if diff <(grep -vE '^\s*#' "$DRIVER") <(grep -vE '^\s*#' "$out") >/dev/null; then
            echo COMMENT-ONLY; return; fi
        echo "$out"
    }
    check() {
        local n="$1" d="$2"
        [ -n "$d" ] || { printf '  MISSED  %-32s <-- inject produced nothing\n' "$n"; MISSED=$((MISSED+1)); return; }
        case "$d" in
            STALE)        printf '  MISSED  %-32s <-- the patch did not apply; it is stale, not the code\n' "$n"; MISSED=$((MISSED+1)); return ;;
            COMMENT-ONLY) printf '  MISSED  %-32s <-- the patch hit only comments\n' "$n"; MISSED=$((MISSED+1)); return ;;
        esac
        if LOADBAND_DRIVER="$d" LOADBAND_NO_SWEEP=1 "$0" >/dev/null 2>&1; then
            printf '  MISSED  %-32s <-- the suite stayed green with this broken\n' "$n"; MISSED=$((MISSED+1))
        else
            printf '  CAUGHT  %-32s\n' "$n"
        fi
    }
    # 1. The budget moves out of the worker. This is the injection this file
    #    exists for: without it the load outlives anything that kills the
    #    controller, which is how a box gets left at load 110 overnight.
    check "budget-not-in-the-worker" "$(inject nobudget 's/timeout "\$s" bash -c/bash -c/')"
    # 2. The workers stop being niced, so they compete with production at equal
    #    weight instead of at 1/68 of it.
    check "workers-not-niced"        "$(inject nonice 's/nice -n "\$NICE" \\\n//')"
    # 3. The pinning goes, so the blast radius is the whole box.
    check "workers-not-pinned"       "$(inject nopin 's/taskset -c "\$CPUS" //')"
    # 4. The circuit breaker can never trip.
    check "abort-never-fires"        "$(inject noabort 's/if \[ "\$BREACHES" -ge 2 \]; then/if false; then/')"
    # 5. The circuit breaker trips on one sample, so a noisy box aborts a valid
    #    block and the band never gets measured.
    check "abort-fires-on-one-sample" "$(inject hair 's/if \[ "\$BREACHES" -ge 2 \]; then/if [ "$BREACHES" -ge 1 ]; then/')"
    # 6. --stop stops nothing.
    check "stop-does-nothing"        "$(inject nostop 's/^stop_load\(\) \{$/stop_load() { return 0;/m')"
    # 7. Ranges in the CPU mask counted as one CPU each, so every runqueue depth
    #    in the report is wrong by a factor of twelve.
    check "cpu-ranges-miscounted"    "$(inject badcpu 's/n=\$\(\(n \+ b - a \+ 1\)\)/n=$((n + 1))/')"
    # 8. The workers sleep instead of spinning: a load generator that generates
    #    nothing, which every other case here would pass.
    # The completeness assertion goes: a truncated calibration reports as a
    # calibration.
    check "incomplete-reports-as-complete" \
        "$(inject nocomplete 's/    if \[ "\$GOT" -ne "\$WANT" \]; then/    if false; then/')"
    check "workers-do-not-spin"      "$(inject nospin "s/while :; do :; done/while :; do sleep 1; done/g")"
fi

echo
echo "=================================================================="
echo "cases: $PASS passed, $FAIL failed"
[ -z "${LOADBAND_NO_SWEEP:-}" ] && echo "injections: $MISSED not caught"
if [ "$FAIL" -eq 0 ] && [ "$MISSED" -eq 0 ]; then echo "CORPUS-LOAD-BAND-FALSIFY GREEN"
else echo "CORPUS-LOAD-BAND-FALSIFY RED"; fi
echo "=================================================================="
exit $(( FAIL > 0 || MISSED > 0 ))
